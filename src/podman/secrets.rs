//! Podman secrets: listing, create (data on stdin), delete and run mappings.

use super::*;

/// One secret as `podman secret inspect` reports it. The secret data itself is
/// never read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Secret {
    pub id: String,
    pub name: String,
    pub driver: String,
    /// `YYYY-MM-DD HH:MM`, or `-`.
    pub created: String,
    /// `YYYY-MM-DD HH:MM`, or `-`.
    pub updated: String,
    /// RFC 3339 creation time, kept for sorting.
    pub created_at: String,
    pub labels: BTreeMap<String, String>,
}

/// List every secret with its timestamps and labels.
///
/// `podman secret ls --format json` is not supported (the flag is a Go
/// template there), and the template output only carries relative dates, so
/// the IDs are listed and then inspected in one call.
pub fn secret_list() -> Result<Vec<Secret>, String> {
    engine::podman_only("secrets")?;
    // A secret removed between the two calls makes `inspect` fail; one retry
    // with a fresh listing covers that race.
    match list_and_inspect() {
        Ok(secrets) => Ok(secrets),
        Err(_) => list_and_inspect(),
    }
}

fn list_and_inspect() -> Result<Vec<Secret>, String> {
    let ids = query(&["secret", "ls", "--quiet"])?;
    let ids: Vec<&str> = ids.split_whitespace().collect();
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut args = vec!["secret", "inspect"];
    args.extend(ids);
    parse_secrets(&query(&args)?)
}

pub(super) fn parse_secrets(json: &str) -> Result<Vec<Secret>, String> {
    let raw: Vec<RawSecret> = parse_json(json, "podman secret inspect")?;
    let mut secrets: Vec<Secret> = raw
        .into_iter()
        .filter(|secret| !secret.id.is_empty())
        .map(|secret| {
            let spec = secret.spec.unwrap_or_default();
            Secret {
                name: spec.name.unwrap_or_else(|| short_id(&secret.id)),
                driver: spec
                    .driver
                    .and_then(|driver| driver.name)
                    .unwrap_or_else(|| "-".to_string()),
                created: display_created(&secret.created_at),
                updated: display_created(&secret.updated_at),
                created_at: secret.created_at,
                labels: spec.labels.unwrap_or_default().into_iter().collect(),
                id: secret.id,
            }
        })
        .collect();
    secrets.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(secrets)
}

pub fn validate_secret_name(value: &str) -> Result<&str, String> {
    let name = validate_name(value, "secret")?;
    // Podman's own limit.
    if name.len() > 253 {
        return Err("secret names may be at most 253 characters".to_string());
    }
    Ok(name)
}

/// `podman secret create <name> -`; the data is written to stdin by
/// [`create_secret`], so it never appears on the command line.
pub fn secret_create_command(name: &str) -> Result<Command, String> {
    engine::podman_only("secrets")?;
    let name = validate_secret_name(name)?;
    let mut command = base_command();
    command.args(["secret", "create", name, "-"]);
    Ok(command)
}

/// Create a secret holding exactly `data` (no trailing newline is added).
pub fn create_secret(name: &str, data: &str) -> Result<String, String> {
    if data.is_empty() {
        return Err("secret data cannot be empty".to_string());
    }
    let mut command = secret_create_command(name)?;
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| format!("failed to launch podman: {error}"))?;
    {
        use std::io::Write;
        let Some(mut stdin) = child.stdin.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err("could not open podman's stdin".to_string());
        };
        if let Err(error) = stdin.write_all(data.as_bytes()) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("could not send the secret data to podman: {error}"));
        }
        // Dropping stdin closes it, which ends the data.
    }
    let output = child
        .wait_with_output()
        .map_err(|error| format!("podman secret create failed: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            format!(
                "`podman secret create {name}` exited with {}",
                output.status
            )
        } else {
            stderr
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub fn remove_secret(id: &str) -> Result<String, String> {
    engine::podman_only("secrets")?;
    run(&["secret", "rm", id])
}

/// Split whitespace-separated secret mappings for `podman run --secret`.
///
/// Each token is `name` (mounted at `/run/secrets/<name>`), `name:/path`
/// (mounted at that path) or `name:VAR` (exposed as environment variable
/// `VAR`), mirroring the upstream mount/env choice. Returns the `--secret`
/// values in Podman's syntax.
pub fn parse_secret_mappings(value: &str) -> Result<Vec<String>, String> {
    value.split_whitespace().map(secret_mapping).collect()
}

fn secret_mapping(token: &str) -> Result<String, String> {
    let (name, target) = match token.split_once(':') {
        Some((name, target)) => (name, Some(target)),
        None => (token, None),
    };
    let name = validate_secret_name(name)?;
    match target {
        None => Ok(name.to_string()),
        Some("") => Err(format!(
            "secret mapping {token} has an empty target (use {name}, {name}:/path or {name}:VAR)"
        )),
        Some(path) if path.starts_with('/') => {
            if path.contains(',') || path.chars().any(char::is_whitespace) {
                return Err(format!("invalid secret mount path: {path}"));
            }
            Ok(format!("{name},type=mount,target={path}"))
        }
        Some(variable) => {
            let valid = !variable.starts_with(|ch: char| ch.is_ascii_digit())
                && variable
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_');
            if !valid {
                return Err(format!(
                    "invalid environment variable for secret {name}: {variable}"
                ));
            }
            Ok(format!("{name},type=env,target={variable}"))
        }
    }
}

/// The secret names referenced by mappings (for checking they exist).
pub fn secret_mapping_names(value: &str) -> Vec<&str> {
    value
        .split_whitespace()
        .map(|token| token.split_once(':').map_or(token, |(name, _)| name))
        .collect()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct RawSecret {
    #[serde(rename = "ID", default)]
    id: String,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    updated_at: String,
    #[serde(default)]
    spec: Option<RawSecretSpec>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub(super) struct RawSecretSpec {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    driver: Option<RawSecretDriver>,
    #[serde(default)]
    labels: Option<HashMap<String, String>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct RawSecretDriver {
    #[serde(default)]
    name: Option<String>,
}
