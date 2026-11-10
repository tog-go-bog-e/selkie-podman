//! Registry push, login/logout and the credentials Podman would use.

use super::*;

/// Push an image to a registry destination (e.g. `quay.io/user/app:latest`).
///
/// Podman pushes any local image to any destination. `docker push` takes a
/// single name, so for Docker the image is first tagged as the destination
/// (in one `sh` process, so the output panel streams and cancels both steps;
/// the references travel as positional arguments, never through the script).
pub fn push_command(image: &str, destination: &str) -> Result<Command, String> {
    let image = validate_image_reference(image)?;
    let destination = validate_image_reference(destination)?;
    if engine() == EngineKind::Docker {
        return Ok(docker_push_command(image, destination));
    }
    let mut command = base_command();
    command.arg("push");
    // Local registries (docker-registry on localhost) usually run without TLS.
    // `--tls-verify` is a Podman flag; Docker configures insecure registries
    // on the daemon instead.
    if is_loopback_registry(destination) {
        command.arg("--tls-verify=false");
    }
    command.args([image, destination]);
    Ok(command)
}

/// `docker push` for validated references, tagging first when they differ.
pub(super) fn docker_push_command(image: &str, destination: &str) -> Command {
    if image == destination {
        let mut command = Command::new("docker");
        command.args(["push", destination]);
        return command;
    }
    let mut command = Command::new("sh");
    command.args([
        "-c",
        r#"docker tag "$1" "$2" && exec docker push "$2""#,
        "selkie-push",
        image,
        destination,
    ]);
    command
}

/// The command line [`push_command`] runs, for the output panel's header.
pub fn push_display(image: &str, destination: &str) -> String {
    push_display_for(engine(), image, destination)
}

pub(super) fn push_display_for(kind: EngineKind, image: &str, destination: &str) -> String {
    let (image, destination) = (image.trim(), destination.trim());
    match kind {
        EngineKind::Docker if image == destination => format!("docker push {destination}"),
        EngineKind::Docker => {
            format!("docker tag {image} {destination} && docker push {destination}")
        }
        EngineKind::Podman | EngineKind::PodmanApi => {
            format!("podman push {image} {destination}")
        }
    }
}

/// Log in to a registry. The password is fed on stdin, never as an argument.
pub fn login_command(registry: &str, username: &str) -> Result<Command, String> {
    login_command_with(persistent_auth_file().as_deref(), registry, username)
}

/// [`login_command`] with an explicit auth file, so it can be tested without
/// depending on the process environment.
pub(super) fn login_command_with(
    auth_file: Option<&std::path::Path>,
    registry: &str,
    username: &str,
) -> Result<Command, String> {
    let registry = validate_registry(registry)?;
    let username = validate_username(username)?;
    let mut command = base_command();
    command.args(["login", "--username", username, "--password-stdin"]);
    if engine() == EngineKind::Podman && is_loopback_registry(registry) {
        command.arg("--tls-verify=false");
    }
    if engine() != EngineKind::Docker {
        if let Some(auth_file) = auth_file {
            command.arg("--authfile").arg(auth_file);
        }
    }
    command.arg(registry);
    Ok(command)
}

/// Log out of one registry, or all of them when `registry` is `None`.
///
/// For Docker, "all" has no flag (`docker logout` takes one server), so it is
/// handled by [`logout`]; this returns an error for that case.
pub fn logout_command(registry: Option<&str>) -> Result<Command, String> {
    logout_command_with(logout_auth_file(registry).as_deref(), registry)
}

/// The auth file a logout should clear.
///
/// Prefer the file that actually holds the credential, so an entry created by an
/// older Selkie (or another tool) in the runtime file is still removed. Otherwise
/// fall back to the persistent file logins are written to.
fn logout_auth_file(registry: Option<&str>) -> Option<std::path::PathBuf> {
    if engine::is_docker() {
        return None;
    }
    match registry.map(str::trim).filter(|value| !value.is_empty()) {
        Some(registry) => auth_file_containing(registry).or_else(persistent_auth_file),
        None => persistent_auth_file(),
    }
}

/// [`logout_command`] with an explicit auth file, for testing.
pub(super) fn logout_command_with(
    auth_file: Option<&std::path::Path>,
    registry: Option<&str>,
) -> Result<Command, String> {
    let mut command = base_command();
    command.arg("logout");
    match registry.map(str::trim).filter(|value| !value.is_empty()) {
        Some(registry) => {
            validate_registry(registry)?;
            if engine() == EngineKind::Podman && is_loopback_registry(registry) {
                command.arg("--tls-verify=false");
            }
            if engine() != EngineKind::Docker {
                if let Some(auth_file) = auth_file {
                    command.arg("--authfile").arg(auth_file);
                }
            }
            command.arg(registry);
        }
        None if engine() == EngineKind::Docker => {
            return Err("docker logout takes a single registry".to_string());
        }
        None => {
            if engine() != EngineKind::Docker {
                if let Some(auth_file) = auth_file {
                    command.arg("--authfile").arg(auth_file);
                }
            }
            command.arg("--all");
        }
    }
    Ok(command)
}

/// Execute a logout. For Docker, "all registries" logs out of every registry
/// listed in its config, one at a time, and reports the ones that failed.
pub fn logout(registry: Option<&str>) -> Result<String, String> {
    let registry = registry.map(str::trim).filter(|value| !value.is_empty());
    if registry.is_some() || engine() == EngineKind::Podman {
        return execute(logout_command(registry)?, "logout");
    }
    let mut failures = Vec::new();
    for account in accounts()? {
        if let Err(error) = execute(
            logout_command(Some(&account.registry))?,
            &format!("logout {}", account.registry),
        ) {
            failures.push(error);
        }
    }
    if failures.is_empty() {
        Ok(String::new())
    } else {
        Err(failures.join("\n"))
    }
}

/// One registry credential Podman would use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    pub registry: String,
    /// Username from the stored credential, or `unknown` when only a token is kept.
    pub username: String,
    pub kind: AccountKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountKind {
    Password,
    Token,
    /// Stored by a credential helper (`credHelpers` or `credsStore`); the
    /// secret is not in the file.
    Helper,
}

impl AccountKind {
    /// What the PASSWORD column shows: a mask for stored passwords, and the
    /// word itself for token-based entries (there is no password to hide).
    pub fn masked(self) -> &'static str {
        match self {
            AccountKind::Password => "••••••••",
            AccountKind::Token => "token",
            AccountKind::Helper => "helper",
        }
    }
}

/// Files Podman may read credentials from, in precedence order.
///
/// `podman login` writes to the runtime file for rootless users; the others are
/// honoured so an entry created with another tool (or `REGISTRY_AUTH_FILE`) is
/// still listed.
pub fn auth_files() -> Vec<std::path::PathBuf> {
    if engine::is_docker() {
        return docker_config_files();
    }
    let mut files = podman_auth_files();
    if let Some(home) = std::env::var_os("HOME") {
        files.push(std::path::PathBuf::from(home).join(".docker/config.json"));
    }
    files
}

/// The auth files Podman itself reads and writes, in precedence order.
fn podman_auth_files() -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    if let Some(path) = std::env::var_os("REGISTRY_AUTH_FILE").filter(|value| !value.is_empty()) {
        files.push(std::path::PathBuf::from(path));
    }
    if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") {
        files.push(std::path::PathBuf::from(runtime).join("containers/auth.json"));
    }
    if let Some(config) = config_dir() {
        files.push(config.join("containers/auth.json"));
    }
    files
}

/// Docker's own credential file. `DOCKER_CONFIG` replaces `~/.docker` entirely;
/// Docker does not fall back to the home directory when it is set.
fn docker_config_files() -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    match std::env::var_os("DOCKER_CONFIG").filter(|value| !value.is_empty()) {
        Some(config) => files.push(std::path::PathBuf::from(config).join("config.json")),
        None => {
            if let Some(home) = std::env::var_os("HOME") {
                files.push(std::path::PathBuf::from(home).join(".docker/config.json"));
            }
        }
    }
    files
}

/// `$XDG_CONFIG_HOME`, or `~/.config` when it is unset.
fn config_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".config"))
        })
}

/// The auth file Selkie writes logins to so they survive a restart.
///
/// Podman's primary auth file for rootless users is
/// `$XDG_RUNTIME_DIR/containers/auth.json`, which lives on a tmpfs and is gone
/// after logout or reboot. Logins are written to the XDG config file instead:
/// Podman reads it after its primary file, so pulls and pushes keep working, and
/// it survives restarts. An explicit `REGISTRY_AUTH_FILE` already chooses the
/// file, so no override is returned then.
pub(super) fn persistent_auth_file() -> Option<std::path::PathBuf> {
    if std::env::var_os("REGISTRY_AUTH_FILE")
        .filter(|value| !value.is_empty())
        .is_some()
    {
        return None;
    }
    Some(config_dir()?.join("containers/auth.json"))
}

/// The first Podman auth file that holds a credential for `registry`.
fn auth_file_containing(registry: &str) -> Option<std::path::PathBuf> {
    podman_auth_files().into_iter().find(|path| {
        std::fs::read_to_string(path).is_ok_and(|contents| {
            parse_accounts(&contents)
                .iter()
                .any(|account| account.registry == registry)
        })
    })
}

/// Registries this user is logged in to, merged across [`auth_files`].
///
/// Credentials themselves are never returned: only the registry, the username
/// and whether a password or a token backs the entry.
pub fn accounts() -> Result<Vec<Account>, String> {
    let mut accounts: BTreeMap<String, Account> = BTreeMap::new();
    for path in auth_files() {
        let Ok(contents) = std::fs::read_to_string(&path) else {
            continue;
        };
        for account in parse_accounts(&contents) {
            accounts.entry(account.registry.clone()).or_insert(account);
        }
    }
    Ok(accounts.into_values().collect())
}

pub(super) fn parse_accounts(contents: &str) -> Vec<Account> {
    let Ok(json) = serde_json::from_str::<serde_json::Value>(contents) else {
        return Vec::new();
    };
    // A global helper (`credsStore`) keeps the secrets elsewhere; `auths` then
    // only lists the registries with empty entries.
    let store = json
        .get("credsStore")
        .and_then(|value| value.as_str())
        .is_some_and(|value| !value.is_empty());

    let mut accounts: Vec<Account> = json
        .get("auths")
        .and_then(|value| value.as_object())
        .into_iter()
        .flatten()
        .map(|(registry, entry)| {
            let auth = entry
                .get("auth")
                .and_then(|value| value.as_str())
                .filter(|value| !value.is_empty());
            let kind =
                if entry.get("identitytoken").is_some() || entry.get("registrytoken").is_some() {
                    AccountKind::Token
                } else if auth.is_none() && store {
                    AccountKind::Helper
                } else {
                    AccountKind::Password
                };
            let username = auth
                .and_then(base64_decode)
                .and_then(|bytes| String::from_utf8(bytes).ok())
                .and_then(|decoded| decoded.split(':').next().map(str::to_string))
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| "unknown".to_string());
            Account {
                registry: registry.clone(),
                username,
                kind,
            }
        })
        .collect();

    // Per-registry helpers. A registry listed in both keeps its `auths` entry,
    // which carries more detail (the username).
    if let Some(helpers) = json.get("credHelpers").and_then(|value| value.as_object()) {
        for registry in helpers.keys() {
            if !accounts.iter().any(|account| &account.registry == registry) {
                accounts.push(Account {
                    registry: registry.clone(),
                    username: "unknown".to_string(),
                    kind: AccountKind::Helper,
                });
            }
        }
    }
    accounts
}

/// Standard-alphabet base64, which is how auth files store `user:password`.
pub(super) fn base64_decode(input: &str) -> Option<Vec<u8>> {
    fn sextet(byte: u8) -> Option<u32> {
        Some(match byte {
            b'A'..=b'Z' => u32::from(byte - b'A'),
            b'a'..=b'z' => u32::from(byte - b'a') + 26,
            b'0'..=b'9' => u32::from(byte - b'0') + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        })
    }

    let mut bits = 0u32;
    let mut available = 0u32;
    let mut decoded = Vec::new();
    for &byte in input.as_bytes() {
        if matches!(byte, b'=' | b'\n' | b'\r') {
            continue;
        }
        bits = (bits << 6) | sextet(byte)?;
        available += 6;
        if available >= 8 {
            available -= 8;
            decoded.push((bits >> available) as u8);
            bits &= (1 << available) - 1;
        }
    }
    Some(decoded)
}

/// `localhost`, `127.0.0.1` and `[::1]` (with optional port) are treated as
/// local development registries without TLS.
pub(super) fn is_loopback_registry(reference: &str) -> bool {
    let reference = reference.trim();
    let reference = reference.strip_prefix("docker://").unwrap_or(reference);
    let host = reference.split('/').next().unwrap_or_default();
    if let Some(bracketed) = host.strip_prefix('[') {
        // `[::1]` or `[::1]:port`; anything else in brackets is a real host.
        return bracketed.split_once(']').is_some_and(|(address, rest)| {
            address == "::1" && (rest.is_empty() || rest.starts_with(':'))
        });
    }
    let host = host.split(':').next().unwrap_or_default();
    matches!(host, "localhost" | "127.0.0.1")
}

/// A registry host with an optional port, without a path.
pub fn validate_registry(value: &str) -> Result<&str, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("registry cannot be empty".to_string());
    }
    if value.starts_with('-') {
        return Err("registry cannot start with '-'".to_string());
    }
    if value.contains('/') {
        return Err("registry must be host[:port], without a path".to_string());
    }
    let valid = value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | ':' | '_' | '[' | ']'));
    if !valid {
        return Err(format!("invalid registry: {value}"));
    }
    Ok(value)
}

pub fn validate_username(value: &str) -> Result<&str, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("username cannot be empty".to_string());
    }
    if value.starts_with('-') {
        return Err("username cannot start with '-'".to_string());
    }
    let valid = value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_' | '@' | '+'));
    if !valid {
        return Err(format!("invalid username: {value}"));
    }
    Ok(value)
}
