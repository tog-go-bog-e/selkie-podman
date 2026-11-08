//! Running `podman`: the selected connection, actions and time-bounded queries.

use std::os::unix::fs::FileTypeExt;

use super::*;

/// Upper bound for read-only queries (lists, inspect, info, stats).
///
/// An unreachable remote connection can leave `podman` waiting indefinitely;
/// without a bound the refresh never completes and the UI stays "loading".
/// Actions (start, pull, machine init, ...) are not bounded: they can
/// legitimately take minutes.
pub const QUERY_TIMEOUT: Duration = Duration::from_secs(45);

/// An action against the selected engine. Not time-bounded.
pub(super) fn run(args: &[&str]) -> Result<String, String> {
    run_command(base_command(), args, None)
}

/// An action against the local Podman installation. Not time-bounded.
pub(super) fn run_local(args: &[&str]) -> Result<String, String> {
    run_command(Command::new("podman"), args, None)
}

/// A read-only query against the selected engine, bounded by [`QUERY_TIMEOUT`].
pub(super) fn query(args: &[&str]) -> Result<String, String> {
    run_command(base_command(), args, Some(QUERY_TIMEOUT))
}

/// A read-only local query, bounded by [`QUERY_TIMEOUT`].
pub(super) fn query_local(args: &[&str]) -> Result<String, String> {
    run_command(Command::new("podman"), args, Some(QUERY_TIMEOUT))
}

pub(super) fn run_command(
    mut command: Command,
    args: &[&str],
    timeout: Option<Duration>,
) -> Result<String, String> {
    command.args(args);
    execute_with(command, &args.join(" "), timeout)
}

/// Run a fully built command. `display` is only used in the error message.
pub(super) fn execute(command: Command, display: &str) -> Result<String, String> {
    execute_with(command, display, None)
}

pub(super) fn launch_error(err: std::io::Error) -> String {
    let binary = engine().binary();
    if err.kind() == std::io::ErrorKind::NotFound {
        format!("{binary} was not found in PATH")
    } else {
        format!("failed to launch {binary}: {err}")
    }
}

/// Like [`Command::output`], but kills the child when `timeout` elapses.
pub(super) fn output_with_timeout(
    command: &mut Command,
    timeout: Duration,
) -> std::io::Result<Option<std::process::Output>> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;

    // Drain both pipes concurrently so a chatty child cannot block on a full
    // pipe while we wait for it.
    fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> mpsc::Receiver<Vec<u8>> {
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut bytes);
            }
            let _ = sender.send(bytes);
        });
        receiver
    }
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());

    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            // The reader threads finish on their own once the pipes close; do
            // not wait for them in case a grandchild still holds a pipe.
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    Ok(Some(std::process::Output {
        status,
        stdout: stdout.recv().unwrap_or_default(),
        stderr: stderr.recv().unwrap_or_default(),
    }))
}

pub(super) fn execute_with(
    mut command: Command,
    display: &str,
    timeout: Option<Duration>,
) -> Result<String, String> {
    let output = match timeout {
        None => command.output().map_err(launch_error)?,
        Some(timeout) => output_with_timeout(&mut command, timeout)
            .map_err(launch_error)?
            .ok_or_else(|| {
                format!(
                    "`{} {display}` did not answer within {} s; is the engine reachable?",
                    engine().binary(),
                    timeout.as_secs()
                )
            })?,
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            format!(
                "`{} {display}` exited with {}",
                engine().binary(),
                output.status
            )
        } else {
            stderr
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Select the connection every engine command targets. `None` uses Podman's own
/// default (the local engine unless configured otherwise).
pub fn set_connection(name: Option<String>) {
    if let Ok(mut slot) = connection_slot().write() {
        *slot = name;
    }
}

pub fn connection() -> Option<String> {
    connection_slot().read().ok().and_then(|slot| slot.clone())
}

/// Point Podman at the session's ssh-agent when the app did not inherit
/// `SSH_AUTH_SOCK` (a desktop launch does not always get a shell-exported one).
///
/// Only applies when the variable is unset: an explicit value always wins.
pub(super) fn apply_ssh_agent(command: &mut Command) {
    if std::env::var_os("SSH_AUTH_SOCK").is_some() {
        return;
    }
    let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") else {
        return;
    };
    if let Some(socket) = ssh_agent_socket(std::path::Path::new(&runtime)) {
        command.env("SSH_AUTH_SOCK", socket);
    }
}

/// Sockets for the agents Linux sessions usually run, in order: OpenSSH's
/// systemd user unit (Fedora/Debian, then Arch's name for it), GNOME's
/// gcr-ssh-agent (GNOME 46+) and the older GNOME Keyring agent.
pub(super) fn ssh_agent_socket(runtime: &std::path::Path) -> Option<std::path::PathBuf> {
    [
        "openssh_agent",
        "ssh-agent.socket",
        "gcr/ssh",
        "keyring/ssh",
    ]
    .into_iter()
    .map(|candidate| runtime.join(candidate))
    .find(|path| std::fs::metadata(path).is_ok_and(|meta| meta.file_type().is_socket()))
}

/// A command for the selected engine. Podman receives the selected connection;
/// Docker reads `DOCKER_HOST` itself, so no `--connection` is added for it.
pub fn base_command() -> Command {
    let mut command = Command::new(engine().binary());
    if engine() == EngineKind::Podman {
        apply_ssh_agent(&mut command);
        apply_connection(&mut command, connection().as_deref());
    }
    command
}

pub(super) fn apply_connection(command: &mut Command, name: Option<&str>) {
    if let Some(name) = name {
        command.arg("--connection").arg(name);
    }
}

pub(super) fn connection_slot() -> &'static RwLock<Option<String>> {
    static CONNECTION: OnceLock<RwLock<Option<String>>> = OnceLock::new();
    CONNECTION.get_or_init(|| RwLock::new(None))
}
