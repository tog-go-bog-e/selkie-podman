//! Registry push, login/logout and the credentials Podman would use.

use super::*;

/// Push an image to a registry destination (e.g. `quay.io/user/app:latest`).
pub fn push_command(image: &str, destination: &str) -> Result<Command, String> {
    let image = validate_image_reference(image)?;
    let destination = validate_image_reference(destination)?;
    let mut command = base_command();
    command.arg("push");
    // Local registries (docker-registry on localhost) usually run without TLS.
    if is_loopback_registry(destination) {
        command.arg("--tls-verify=false");
    }
    command.args([image, destination]);
    Ok(command)
}

/// Log in to a registry. The password is fed on stdin, never as an argument.
pub fn login_command(registry: &str, username: &str) -> Result<Command, String> {
    let registry = validate_registry(registry)?;
    let username = validate_username(username)?;
    let mut command = base_command();
    command.args(["login", "--username", username, "--password-stdin"]);
    if is_loopback_registry(registry) {
        command.arg("--tls-verify=false");
    }
    command.arg(registry);
    Ok(command)
}

/// Log out of one registry, or all of them when `registry` is `None`.
pub fn logout_command(registry: Option<&str>) -> Result<Command, String> {
    let mut command = base_command();
    command.arg("logout");
    match registry.map(str::trim).filter(|value| !value.is_empty()) {
        Some(registry) => {
            validate_registry(registry)?;
            if is_loopback_registry(registry) {
                command.arg("--tls-verify=false");
            }
            command.arg(registry);
        }
        None => {
            command.arg("--all");
        }
    }
    Ok(command)
}

/// Execute a logout command.
pub fn logout(registry: Option<&str>) -> Result<String, String> {
    execute(logout_command(registry)?, "logout")
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
    use std::path::PathBuf;

    let mut files = Vec::new();
    if let Some(path) = std::env::var_os("REGISTRY_AUTH_FILE") {
        files.push(PathBuf::from(path));
    }
    if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") {
        files.push(PathBuf::from(runtime).join("containers/auth.json"));
    }
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")));
    if let Some(config) = config {
        files.push(config.join("containers/auth.json"));
    }
    if let Some(home) = std::env::var_os("HOME") {
        files.push(PathBuf::from(home).join(".docker/config.json"));
    }
    files
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
