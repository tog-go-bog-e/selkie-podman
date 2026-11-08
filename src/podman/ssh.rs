//! SSH keys for remote Podman connections.
//!
//! Podman's SSH client never prompts for a passphrase: an encrypted identity
//! fails with `bcrypt_pbkdf: empty password`, and with no identity it offers
//! every key in `SSH_AUTH_SOCK` (which sshd rejects once `MaxAuthTries` is
//! exceeded). The app therefore unlocks encrypted keys itself:
//!
//! - one private `ssh-agent` per connection, holding only that connection's
//!   key, so unrelated agent keys are never offered;
//! - the passphrase is read in the UI and piped to `ssh-add` through a
//!   `--askpass` reinvocation of this binary, never written to disk or argv;
//! - the key path is cached in the app's settings because Podman cannot read an
//!   encrypted `Identity` from its connection record.
//!
//! Unencrypted keys keep working through Podman's own `Identity`; they need no
//! agent at all.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Write;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::io::FromRawFd;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Duration;

/// State of the key remembered for a connection.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyState {
    /// No app-managed key (Podman's own identity may still be set).
    None,
    /// An unencrypted key, stored in Podman's connection as before.
    Unencrypted,
    /// An encrypted key whose passphrase has not been entered this session.
    Locked,
    /// An encrypted key loaded into the connection's private agent.
    Unlocked,
}

struct CachedKey {
    path: PathBuf,
    encrypted: bool,
}

struct Agent {
    socket: PathBuf,
    child: Child,
}

#[derive(Default)]
struct State {
    agents: HashMap<String, Agent>,
    cached: HashMap<String, CachedKey>,
    /// Identity files configured in Podman, by connection name.
    identities: HashMap<String, String>,
    /// Connections whose key is loaded in their private agent.
    loaded: HashSet<String>,
}

fn state() -> MutexGuard<'static, State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Runs of `ssh-add` are serialised so two commands cannot add the same key at
/// the same time.
fn add_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Replace the remembered keys (called when settings are loaded).
pub fn set_cached_keys(keys: impl IntoIterator<Item = (String, String)>) {
    let mut state = state();
    state.cached.clear();
    for (name, path) in keys {
        let path = PathBuf::from(path);
        let encrypted = key_needs_passphrase(&path).unwrap_or(false);
        state.cached.insert(name, CachedKey { path, encrypted });
    }
}

/// Remember (or forget) the identity file for one connection. Replacing a key
/// stops the old agent so the next use starts clean.
pub fn set_cached_key(name: &str, path: Option<&str>) {
    let mut state = state();
    stop_agent(&mut state, name);
    match path.map(str::trim).filter(|path| !path.is_empty()) {
        Some(path) => {
            let path = PathBuf::from(path);
            let encrypted = key_needs_passphrase(&path).unwrap_or(false);
            state
                .cached
                .insert(name.to_string(), CachedKey { path, encrypted });
        }
        None => {
            state.cached.remove(name);
        }
    }
}

/// Kill agents left behind by a previous run and remove their sockets. The
/// app is single-instance, so every `ssh-agent-*.sock` in our private runtime
/// directory belongs to us (or to a crash from an earlier run).
pub fn cleanup_stale_agents() {
    let Ok(dir) = agent_dir() else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if !name.to_string_lossy().starts_with("ssh-agent-") {
            continue;
        }
        kill_processes_for_socket(&path);
        let _ = std::fs::remove_file(&path);
    }
}

/// Stop every agent this process started. Called when the app exits normally;
/// crashes are covered by [`cleanup_stale_agents`] on the next start.
pub fn shutdown() {
    let mut state = state();
    let names: Vec<String> = state.agents.keys().cloned().collect();
    for name in names {
        stop_agent(&mut state, &name);
    }
}

#[cfg(target_os = "linux")]
fn kill_processes_for_socket(socket: &Path) {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return;
    };
    let needle = socket.as_os_str().as_encoded_bytes();
    let self_pid = std::process::id() as libc::pid_t;
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<libc::pid_t>().ok())
        else {
            continue;
        };
        if pid == self_pid {
            continue;
        }
        let Ok(cmdline) = std::fs::read(entry.path().join("cmdline")) else {
            continue;
        };
        if !cmdline.windows(needle.len()).any(|window| window == needle) {
            continue;
        }
        // SAFETY: signalling another process owned by the same user; the socket
        // path is in our private runtime directory.
        unsafe {
            libc::kill(pid, libc::SIGKILL);
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn kill_processes_for_socket(_socket: &Path) {}

pub fn cached_key(name: &str) -> Option<String> {
    state()
        .cached
        .get(name)
        .map(|key| key.path.to_string_lossy().into_owned())
}

/// Whether the connection has an app-managed key at all (locked or not).
pub fn has_cached_key(name: &str) -> bool {
    state().cached.contains_key(name)
}

pub fn key_state(name: &str) -> KeyState {
    let mut state = state();
    if live_socket(&mut state, name).is_some() && state.loaded.contains(name) {
        return KeyState::Unlocked;
    }
    match state.cached.get(name) {
        Some(key) if key.encrypted => KeyState::Locked,
        Some(_) => KeyState::Unencrypted,
        None => KeyState::None,
    }
}

/// The key path to prompt for, when the connection's key is locked.
pub fn needs_unlock(name: &str) -> Option<String> {
    let mut state = state();
    let encrypted = state.cached.get(name).is_some_and(|key| key.encrypted);
    if !encrypted || (live_socket(&mut state, name).is_some() && state.loaded.contains(name)) {
        return None;
    }
    state
        .cached
        .get(name)
        .map(|key| key.path.to_string_lossy().into_owned())
}

/// The live agent for an unlocked connection.
pub fn agent_socket(name: &str) -> Option<PathBuf> {
    let mut state = state();
    if !state.loaded.contains(name) {
        return None;
    }
    live_socket(&mut state, name)
}

/// Record the identities Podman has configured. Empty identities are dropped:
/// `--connection` then uses the agent instead.
pub fn note_connections(connections: impl IntoIterator<Item = (String, String)>) {
    let mut state = state();
    state.identities = connections
        .into_iter()
        .filter(|(_, identity)| !identity.trim().is_empty())
        .collect();
}

/// Whether Podman has an identity for this connection (so its agent keys are
/// not needed and must not be offered).
pub fn has_identity(name: &str) -> bool {
    state().identities.contains_key(name)
}

/// Unlock `key` into the connection's private agent. `passphrase` is required
/// for encrypted keys and is never stored.
pub fn add_key(name: &str, key: &Path, passphrase: Option<&str>) -> Result<(), String> {
    let _serial = add_lock();
    {
        let mut state = state();
        if state.loaded.contains(name) && live_socket(&mut state, name).is_some() {
            return Ok(());
        }
    }

    if !key.is_file() {
        return Err(format!("identity file not found: {}", key.display()));
    }
    let encrypted = key_needs_passphrase(key)?;
    if encrypted && passphrase.is_none() {
        return Err("this SSH key needs a passphrase".to_string());
    }

    let socket = {
        let mut state = state();
        ensure_agent(&mut state, name)?
    };

    let mut command = Command::new("ssh-add");
    command
        .env("SSH_AUTH_SOCK", &socket)
        .arg(key)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let askpass = match passphrase {
        Some(passphrase) => {
            if passphrase.is_empty() {
                return Err("the passphrase is empty".to_string());
            }
            Some(askpass_env(&mut command)?)
        }
        None => None,
    };

    let child = command
        .spawn()
        .map_err(|error| format!("could not run ssh-add: {error}"))?;

    if let Some(askpass) = askpass {
        // SAFETY: the read end is inherited by ssh-add and its askpass helper;
        // the parent only needs the write side.
        unsafe {
            libc::close(askpass.read_fd);
        }
        // SAFETY: `write_fd` came from `pipe()` and is owned here.
        let mut writer = unsafe { File::from_raw_fd(askpass.write_fd) };
        let passphrase = passphrase.unwrap_or_default();
        let _ = writer.write_all(passphrase.as_bytes());
        let _ = writer.write_all(b"\n");
    }

    let output = child
        .wait_with_output()
        .map_err(|error| format!("ssh-add did not finish: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = stderr.trim();
        let detail = if detail.is_empty() {
            stdout.trim()
        } else {
            detail
        };
        let detail = detail.strip_prefix("Error: ").unwrap_or(detail);
        let message = if detail.is_empty()
            || detail.contains("Bad passphrase")
            || detail.contains("incorrect passphrase")
        {
            "the passphrase was not accepted".to_string()
        } else {
            detail.to_string()
        };
        return Err(message);
    }

    state().loaded.insert(name.to_string());
    Ok(())
}

/// Whether the private key is passphrase-protected.
///
/// `ssh-keygen -y` decrypts the key and prints its public half; an empty
/// passphrase succeeds only for unencrypted keys.
pub fn key_needs_passphrase(path: &Path) -> Result<bool, String> {
    let output = Command::new("ssh-keygen")
        .args(["-y", "-P", "", "-f"])
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| format!("could not run ssh-keygen: {error}"))?;
    if output.status.success() {
        return Ok(false);
    }
    let stderr = String::from_utf8_lossy(&output.stderr).to_lowercase();
    if stderr.contains("passphrase") || stderr.contains("bcrypt") || stderr.contains("decrypt") {
        return Ok(true);
    }
    // PKCS#8 errors can come from libcrypto without a useful message; the PEM
    // header still says so.
    if let Ok(contents) = std::fs::read_to_string(path) {
        if contents.contains("ENCRYPTED") || contents.contains("Proc-Type: 4,ENCRYPTED") {
            return Ok(true);
        }
    }
    let message = String::from_utf8_lossy(&output.stderr);
    Err(format!(
        "could not read SSH key {}: {}",
        path.display(),
        message.trim()
    ))
}

/// Answer a `ssh-add` askpass invocation. Called from `main` before GPUI starts.
///
/// The passphrase arrives on the inherited descriptor, not in argv or the
/// environment.
pub fn answer_askpass() {
    let fd = std::env::var("SELKIE_ASKPASS_FD")
        .ok()
        .and_then(|value| value.parse::<i32>().ok())
        .unwrap_or(3);
    // SAFETY: the parent set up this descriptor and passed it to us.
    let mut reader = unsafe { File::from_raw_fd(fd) };
    let mut answer = String::new();
    // A read error just yields an empty answer; ssh-add then reports the
    // failure to the app.
    let _ = std::io::BufRead::read_line(&mut std::io::BufReader::new(&mut reader), &mut answer);
    let _ = std::io::stdout().write_all(answer.as_bytes());
    let _ = std::io::stdout().flush();
}

struct Askpass {
    read_fd: i32,
    write_fd: i32,
}

/// Wire `ssh-add` to re-invoke this binary for its passphrase prompt.
fn askpass_env(command: &mut Command) -> Result<Askpass, String> {
    let mut fds = [0 as libc::c_int; 2];
    // SAFETY: `fds` has room for the two descriptors `pipe` writes.
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return Err(format!(
            "could not create the passphrase pipe: {}",
            std::io::Error::last_os_error()
        ));
    }
    // The read end must survive exec; the write end stays in this process.
    // SAFETY: both descriptors come from `pipe` above.
    unsafe {
        libc::fcntl(fds[0], libc::F_SETFD, 0);
        libc::fcntl(fds[1], libc::F_SETFD, libc::FD_CLOEXEC);
    }
    let executable = match std::env::current_exe() {
        Ok(executable) => executable,
        Err(error) => {
            // SAFETY: both descriptors come from the pipe created above.
            unsafe {
                libc::close(fds[0]);
                libc::close(fds[1]);
            }
            return Err(format!("could not locate the selkie binary: {error}"));
        }
    };
    command.env("SELKIE_ASKPASS_FD", fds[0].to_string());
    command.env("SSH_ASKPASS", executable);
    command.env("SSH_ASKPASS_REQUIRE", "force");
    // Old OpenSSH only consults `SSH_ASKPASS` when `DISPLAY` is set; a desktop
    // launch may not have one.
    if std::env::var_os("DISPLAY").is_none() {
        command.env("DISPLAY", ":0");
    }
    Ok(Askpass {
        read_fd: fds[0],
        write_fd: fds[1],
    })
}

fn live_socket(state: &mut State, name: &str) -> Option<PathBuf> {
    if let Some(agent) = state.agents.get_mut(name) {
        let alive = agent.child.try_wait().ok().flatten().is_none()
            && std::fs::metadata(&agent.socket).is_ok_and(|meta| meta.file_type().is_socket());
        if alive {
            return Some(agent.socket.clone());
        }
    }
    stop_agent(state, name);
    None
}

fn stop_agent(state: &mut State, name: &str) {
    state.loaded.remove(name);
    if let Some(mut agent) = state.agents.remove(name) {
        let _ = agent.child.kill();
        let _ = agent.child.wait();
        let _ = std::fs::remove_file(&agent.socket);
    }
}

fn ensure_agent(state: &mut State, name: &str) -> Result<PathBuf, String> {
    if let Some(socket) = live_socket(state, name) {
        return Ok(socket);
    }
    let socket = socket_path(name)?;
    let _ = std::fs::remove_file(&socket);

    let mut command = Command::new("ssh-agent");
    command
        .args(["-D", "-a"])
        .arg(&socket)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: `pre_exec` only calls async-signal-safe functions.
    #[cfg(target_os = "linux")]
    unsafe {
        use std::os::unix::process::CommandExt;
        command.pre_exec(|| {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }

    let mut child = command
        .spawn()
        .map_err(|error| format!("could not start ssh-agent: {error}"))?;
    let agent_pid = child.id();
    // `ssh-agent` clears/ignores `PR_SET_PDEATHSIG`, so a tiny child watches the
    // app and stops the agent when it dies (including SIGKILL). Normal exits
    // remove the socket first, which also ends the watchdog.
    spawn_watchdog(&socket, agent_pid);
    // `ssh-agent` creates the socket a moment after it starts.
    for _ in 0..300 {
        if std::fs::metadata(&socket).is_ok_and(|meta| meta.file_type().is_socket()) {
            state.agents.insert(
                name.to_string(),
                Agent {
                    socket: socket.clone(),
                    child,
                },
            );
            return Ok(socket);
        }
        if let Ok(Some(status)) = child.try_wait() {
            return Err(format!("ssh-agent exited before it was ready ({status})"));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let _ = child.kill();
    let _ = child.wait();
    Err("ssh-agent did not create its socket".to_string())
}

/// Keep the agent's lifetime tied to the app even across `SIGKILL`.
fn spawn_watchdog(socket: &Path, agent_pid: u32) {
    const SCRIPT: &str = "app=$1; agent=$2; socket=$3; \
         while kill -0 \"$app\" 2>/dev/null; do sleep 2; done; \
         [ -S \"$socket\" ] && kill \"$agent\" 2>/dev/null; exit 0";
    let _ = Command::new("sh")
        .arg("-c")
        .arg(SCRIPT)
        .arg("selkie-agent-watchdog")
        .arg(std::process::id().to_string())
        .arg(agent_pid.to_string())
        .arg(socket)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

fn agent_dir() -> Result<PathBuf, String> {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            // A shared /tmp is fine for the socket (0600) but the directory
            // must be per-user to keep stale agents apart.
            let uid = unsafe { libc::getuid() };
            std::env::temp_dir().join(format!("selkie-{uid}"))
        });
    let dir = base.join("selkie");
    std::fs::create_dir_all(&dir).map_err(|error| {
        format!(
            "could not create the agent directory {}: {error}",
            dir.display()
        )
    })?;
    let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    Ok(dir)
}

/// A short, stable socket name: connection names are user data and may contain
/// characters that do not belong in a filename.
fn socket_path(name: &str) -> Result<PathBuf, String> {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in name.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    Ok(agent_dir()?.join(format!("ssh-agent-{hash:016x}.sock")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique(name: &str) -> String {
        format!("selkie-test-{name}-{}", std::process::id())
    }

    fn generate_key(path: &Path, passphrase: &str) -> bool {
        std::fs::remove_file(path).ok();
        Command::new("ssh-keygen")
            .args(["-q", "-t", "ed25519", "-N", passphrase, "-f"])
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    #[test]
    fn detects_encrypted_keys() {
        let dir = std::env::temp_dir().join(unique("keys"));
        std::fs::create_dir_all(&dir).unwrap();
        let plain = dir.join("plain");
        let encrypted = dir.join("encrypted");
        if !generate_key(&plain, "") || !generate_key(&encrypted, "secret") {
            // ssh-keygen is not available: nothing to verify.
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }

        assert_eq!(key_needs_passphrase(&plain), Ok(false));
        assert_eq!(key_needs_passphrase(&encrypted), Ok(true));
        assert!(key_needs_passphrase(&dir.join("missing")).is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn state_follows_the_cached_key() {
        let name = unique("state");
        let dir = std::env::temp_dir().join(unique("state-keys"));
        std::fs::create_dir_all(&dir).unwrap();
        let plain = dir.join("plain");
        let encrypted = dir.join("encrypted");
        if !generate_key(&plain, "") || !generate_key(&encrypted, "secret") {
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }

        set_cached_key(&name, Some(&plain.to_string_lossy()));
        assert_eq!(key_state(&name), KeyState::Unencrypted);
        assert!(needs_unlock(&name).is_none());

        set_cached_key(&name, Some(&encrypted.to_string_lossy()));
        assert_eq!(key_state(&name), KeyState::Locked);
        assert_eq!(
            needs_unlock(&name).as_deref(),
            Some(encrypted.to_string_lossy().as_ref())
        );

        set_cached_key(&name, None);
        assert_eq!(key_state(&name), KeyState::None);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
