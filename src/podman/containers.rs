//! Containers: listing, actions, `podman run`, logs, stats, exec and inspect.

use super::*;

#[derive(Debug, Clone, Default)]
pub struct Container {
    pub id: String,
    pub name: String,
    pub image: String,
    pub state: String,
    pub status: String,
    pub ports: String,
    /// Present when Podman reports the container as part of a compose project.
    pub compose: Option<ComposeMembership>,
}

/// One `podman stats --no-stream` sample for a container.
#[derive(Debug, Clone)]
pub struct StatsSample {
    pub cpu_percent: f64,
    pub mem_usage: u64,
    pub mem_limit: u64,
    pub mem_percent: f64,
    pub net_in: u64,
    pub net_out: u64,
    pub block_in: u64,
    pub block_out: u64,
    pub pids: u64,
    pub at: Instant,
}

impl StatsSample {
    pub fn memory_label(&self) -> String {
        format!(
            "{} / {}",
            human_size(self.mem_usage),
            human_size(self.mem_limit)
        )
    }

    pub fn network_label(&self) -> String {
        format!(
            "{} in / {} out",
            human_size(self.net_in),
            human_size(self.net_out)
        )
    }

    pub fn block_label(&self) -> String {
        format!(
            "{} in / {} out",
            human_size(self.block_in),
            human_size(self.block_out)
        )
    }
}

impl Container {
    pub fn id_short(&self) -> String {
        short_id(&self.id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerAction {
    Start,
    Stop,
    Restart,
    Pause,
    Unpause,
}

impl ContainerAction {
    pub fn verb(self) -> &'static str {
        match self {
            ContainerAction::Start => "start",
            ContainerAction::Stop => "stop",
            ContainerAction::Restart => "restart",
            ContainerAction::Pause => "pause",
            ContainerAction::Unpause => "unpause",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ContainerAction::Start => "Starting container",
            ContainerAction::Stop => "Stopping container",
            ContainerAction::Restart => "Restarting container",
            ContainerAction::Pause => "Pausing container",
            ContainerAction::Unpause => "Resuming container",
        }
    }

    pub fn done(self) -> &'static str {
        match self {
            ContainerAction::Start => "Container started",
            ContainerAction::Stop => "Container stopped",
            ContainerAction::Restart => "Container restarted",
            ContainerAction::Pause => "Container paused",
            ContainerAction::Unpause => "Container resumed",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ContainerDetails {
    pub id: String,
    pub name: String,
    pub image: String,
    pub command: String,
    pub created: String,
    pub status: String,
    pub pid: i64,
    pub exit_code: i64,
    pub env: Vec<String>,
    pub mounts: Vec<MountDetails>,
    pub networks: Vec<String>,
    pub compose_project: Option<String>,
    pub compose_service: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct MountDetails {
    pub kind: String,
    pub source: String,
    pub destination: String,
}

pub fn parse_containers(json: &str) -> Result<Vec<Container>, String> {
    let raw: Vec<RawContainer> = parse_json(json, "podman ps")?;
    Ok(raw
        .into_iter()
        .map(|raw| {
            let id = raw.id;
            let name = first_non_empty(raw.names.as_deref()).unwrap_or_else(|| short_id(&id));
            Container {
                name,
                image: raw.image.unwrap_or_else(|| "<none>".to_string()),
                state: raw.state.unwrap_or_else(|| "unknown".to_string()),
                status: raw.status.unwrap_or_default(),
                ports: ports_label(raw.ports.as_deref().unwrap_or_default()),
                compose: compose_membership(raw.labels.as_ref()),
                id,
            }
        })
        .collect())
}

/// `podman logs -f --since <window> --timestamps` for a container.
pub fn container_logs_command(id: &str, window: &str) -> Command {
    let mut command = base_command();
    command.args(["logs", "-f", "--since", window, "--timestamps", id]);
    command
}

pub fn container_action(id: &str, action: ContainerAction) -> Result<String, String> {
    if engine::uses_api() {
        return api::container_action(id, action);
    }
    run(&[action.verb(), id])
}

pub fn remove_container(id: &str) -> Result<String, String> {
    if engine::uses_api() {
        return api::remove_container(id);
    }
    run(&["rm", "-f", id])
}

pub fn rename_container(id: &str, name: &str) -> Result<String, String> {
    let name = validate_container_name(name)?;
    if engine::uses_api() {
        return api::rename_container(id, name);
    }
    run(&["rename", id, name])
}

/// Names must be a valid Podman container name: no whitespace, no leading `-`
/// (which would be parsed as a flag), and only characters Podman accepts.
pub fn validate_container_name(value: &str) -> Result<&str, String> {
    validate_name(value, "container")
}

/// `podman exec -it` for a container's Terminal tab.
pub fn exec_terminal_command(container: &str) -> Command {
    let mut command = base_command();
    command.args([
        "exec",
        "-it",
        "-e",
        "TERM=xterm-256color",
        container,
        "/bin/sh",
    ]);
    command
}

/// Sample resource usage for one container.
///
/// Returns `None` when Podman reports no statistics for the container. A
/// stopped container yields a zeroed sample rather than an error.
pub fn stats(id: &str) -> Result<Option<StatsSample>, String> {
    if engine::uses_api() {
        return api::stats(id);
    }
    let json = query(&[
        "stats",
        "--no-stream",
        "--interval",
        "1",
        "--format",
        "json",
        id,
    ])?;
    let raw: Vec<RawStats> = parse_json(&json, "podman stats")?;
    Ok(raw.into_iter().next().map(RawStats::into_sample))
}

/// Options for `podman run`.
#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    pub image: String,
    pub name: Option<String>,
    /// `host:container[/proto]` mappings.
    pub ports: Vec<String>,
    /// `KEY=VALUE` (or `KEY`) environment entries.
    pub env: Vec<String>,
    /// `source:destination[:options]` mounts.
    pub volumes: Vec<String>,
    /// `--secret` values, as produced by [`parse_secret_mappings`].
    pub secrets: Vec<String>,
    /// Command and arguments appended after the image.
    pub command: Vec<String>,
}

/// Build a detached `podman run` command.
pub fn container_run_command(options: &RunOptions) -> Result<Command, String> {
    let image = validate_image_reference(&options.image)?;
    let name = options
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty());
    if let Some(name) = name {
        validate_container_name(name)?;
    }
    for port in &options.ports {
        validate_port_mapping(port)?;
    }
    for entry in &options.env {
        validate_env_var(entry)?;
    }
    for mount in &options.volumes {
        validate_volume_mount(mount)?;
    }
    for secret in &options.secrets {
        if secret.is_empty() || secret.starts_with('-') {
            return Err(format!("invalid secret mapping: {secret}"));
        }
    }
    if engine::uses_api() && !options.secrets.is_empty() {
        return Err(format!(
            "secrets are not available with the {} engine; clear the Secrets field",
            engine::engine().label()
        ));
    }

    let mut command = base_command();
    // The app is a container manager, so a run should leave a container behind
    // rather than attaching a TTY to it; the Terminal tab covers interactive use.
    command.args(["run", "-d"]);
    if let Some(name) = name {
        command.args(["--name", name]);
    }
    for port in &options.ports {
        command.args(["-p", port]);
    }
    for entry in &options.env {
        command.args(["-e", entry]);
    }
    for mount in &options.volumes {
        command.args(["-v", mount]);
    }
    for secret in &options.secrets {
        command.args(["--secret", secret]);
    }
    command.arg(image);
    command.args(&options.command);
    Ok(command)
}

/// Run a container and return Podman's stdout (the container ID).
pub fn run_container(options: &RunOptions) -> Result<String, String> {
    let command = container_run_command(options)?;
    execute(command, &format!("run {}", options.image))
}

/// Split whitespace-separated port mappings and validate each one.
pub fn parse_ports(value: &str) -> Result<Vec<String>, String> {
    parse_tokens(value, validate_port_mapping)
}

/// Split whitespace-separated environment entries and validate each one.
pub fn parse_env_vars(value: &str) -> Result<Vec<String>, String> {
    parse_tokens(value, validate_env_var)
}

/// Split whitespace-separated volume mounts and validate each one.
pub fn parse_volume_mounts(value: &str) -> Result<Vec<String>, String> {
    parse_tokens(value, validate_volume_mount)
}

/// Split a command line into arguments with POSIX-shell-style quoting:
/// `'single'` is literal, `"double"` honours `\\`, `\"`, `\$` and `` \` ``,
/// and a backslash outside quotes escapes the next character. Nothing is
/// expanded; the arguments are passed to Podman as-is, without a shell.
pub fn parse_command_args(value: &str) -> Result<Vec<String>, String> {
    let mut args = Vec::new();
    let mut current = String::new();
    // Distinguishes `''` (an empty argument) from no argument at all.
    let mut in_arg = false;
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '\'' => {
                in_arg = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(next) => current.push(next),
                        None => return Err("command has an unterminated ' quote".to_string()),
                    }
                }
            }
            '"' => {
                in_arg = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(next @ ('\\' | '"' | '$' | '`')) => current.push(next),
                            Some(next) => {
                                current.push('\\');
                                current.push(next);
                            }
                            None => return Err("command has an unterminated \" quote".to_string()),
                        },
                        Some(next) => current.push(next),
                        None => return Err("command has an unterminated \" quote".to_string()),
                    }
                }
            }
            '\\' => {
                in_arg = true;
                match chars.next() {
                    Some(next) => current.push(next),
                    None => return Err("command ends with a dangling backslash".to_string()),
                }
            }
            ch if ch.is_whitespace() => {
                if in_arg {
                    args.push(std::mem::take(&mut current));
                    in_arg = false;
                }
            }
            ch => {
                in_arg = true;
                current.push(ch);
            }
        }
    }
    if in_arg {
        args.push(current);
    }
    Ok(args)
}

pub(super) fn parse_tokens(
    value: &str,
    validate: fn(&str) -> Result<&str, String>,
) -> Result<Vec<String>, String> {
    let mut tokens = Vec::new();
    for token in value.split_whitespace() {
        validate(token)?;
        tokens.push(token.to_string());
    }
    Ok(tokens)
}

/// Accept `[host:]hostPort:containerPort[/proto]`-style mappings.
pub fn validate_port_mapping(value: &str) -> Result<&str, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("port mapping cannot be empty".to_string());
    }
    if value.starts_with('-') {
        return Err("port mapping cannot start with '-'".to_string());
    }
    if !value.contains(':') {
        return Err(format!(
            "invalid port mapping: {value} (expected host:container, e.g. 8080:80)"
        ));
    }
    let valid = value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | ':' | '/' | '-'));
    if !valid {
        return Err(format!("invalid port mapping: {value}"));
    }
    Ok(value)
}

pub fn validate_env_var(value: &str) -> Result<&str, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("environment entry cannot be empty".to_string());
    }
    if value.starts_with('-') {
        return Err("environment entries cannot start with '-'".to_string());
    }
    let key = value.split('=').next().unwrap_or_default();
    let valid_key = !key.is_empty()
        && key
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-'));
    if !valid_key {
        return Err(format!(
            "invalid environment entry: {value} (expected KEY=VALUE)"
        ));
    }
    Ok(value)
}

/// Accept named volumes and bind mounts: `source:destination[:options]`.
pub fn validate_volume_mount(value: &str) -> Result<&str, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("volume mount cannot be empty".to_string());
    }
    if value.starts_with('-') {
        return Err("volume mounts cannot start with '-'".to_string());
    }
    let mut parts = value.split(':');
    let source = parts.next().unwrap_or_default();
    let destination = parts.next().unwrap_or_default();
    if source.is_empty() || destination.is_empty() {
        return Err(format!(
            "invalid volume mount: {value} (expected source:destination)"
        ));
    }
    Ok(value)
}

/// Fetch the last `lines` log lines without following, for "load older".
pub fn logs_tail_command(id: &str, lines: usize) -> Command {
    let mut command = base_command();
    command.args(["logs", "--timestamps", "--tail", &lines.to_string(), id]);
    command
}

/// The RFC3339 timestamp that `--timestamps` prefixes to each log line.
pub fn log_timestamp(line: &str) -> Option<&str> {
    let token = line.split_whitespace().next()?;
    let year = token.as_bytes().get(..4)?;
    if token.len() >= 20 && year.iter().all(u8::is_ascii_digit) && token.contains('T') {
        Some(token)
    } else {
        None
    }
}

pub fn inspect_container(id: &str) -> Result<ContainerDetails, String> {
    if engine::uses_api() {
        return api::inspect_container(id);
    }
    let json = query(&["inspect", id])?;
    parse_inspect(json.trim()).map_err(|err| format!("could not inspect {id}: {err}"))
}

pub(super) fn parse_inspect(json: &str) -> Result<ContainerDetails, String> {
    let mut raw: Vec<RawInspect> = parse_json(json, "podman inspect")?;
    let raw = raw
        .pop()
        .ok_or_else(|| "no inspect data returned".to_string())?;
    let state = raw.state.unwrap_or_default();
    let config = raw.config.unwrap_or_default();
    let compose = compose_membership(config.labels.as_ref());
    let mut command = config.entrypoint.unwrap_or_default();
    command.extend(config.cmd.unwrap_or_default());

    Ok(ContainerDetails {
        id: raw.id,
        name: raw.name.trim_start_matches('/').to_string(),
        image: raw
            .image_name
            .or(config.image)
            .unwrap_or_else(|| "<none>".to_string()),
        command: command.join(" "),
        created: format_timestamp(raw.created.as_deref()),
        status: state.status.unwrap_or_else(|| "unknown".to_string()),
        pid: state.pid.unwrap_or_default(),
        exit_code: state.exit_code.unwrap_or_default(),
        env: config.env.unwrap_or_default(),
        mounts: raw
            .mounts
            .unwrap_or_default()
            .into_iter()
            .map(|mount| MountDetails {
                kind: mount.kind.unwrap_or_default(),
                source: mount.source.unwrap_or_default(),
                destination: mount.destination.unwrap_or_default(),
            })
            .collect(),
        networks: raw
            .network_settings
            .and_then(|settings| settings.networks)
            .map(|networks| networks.into_keys().collect())
            .unwrap_or_default(),
        compose_project: compose
            .as_ref()
            .map(|membership| membership.project.clone()),
        compose_service: compose.and_then(|membership| membership.service),
    })
}

pub(super) fn ports_label(ports: &[RawPort]) -> String {
    if ports.is_empty() {
        return "-".to_string();
    }

    ports
        .iter()
        .map(|port| {
            let protocol = if port.protocol.is_empty() {
                "tcp"
            } else {
                &port.protocol
            };
            if port.host_port == 0 {
                format!("{}/{protocol}", port.container_port)
            } else if port.host_ip.is_empty() || port.host_ip == "0.0.0.0" || port.host_ip == "::" {
                format!("{}->{}/{protocol}", port.host_port, port.container_port)
            } else {
                format!(
                    "{}:{}->{}/{protocol}",
                    port.host_ip, port.host_port, port.container_port
                )
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct RawStats {
    #[serde(default)]
    cpu_percent: String,
    #[serde(default)]
    mem_usage: String,
    #[serde(default)]
    mem_percent: String,
    #[serde(default)]
    net_io: String,
    #[serde(default)]
    block_io: String,
    #[serde(default)]
    pids: String,
}

impl RawStats {
    pub(super) fn into_sample(self) -> StatsSample {
        let (mem_usage, mem_limit) = parse_io(&self.mem_usage);
        let (net_in, net_out) = parse_io(&self.net_io);
        let (block_in, block_out) = parse_io(&self.block_io);
        StatsSample {
            cpu_percent: parse_percent(&self.cpu_percent),
            mem_usage,
            mem_limit,
            mem_percent: parse_percent(&self.mem_percent),
            net_in,
            net_out,
            block_in,
            block_out,
            pids: self.pids.trim().parse().unwrap_or_default(),
            at: Instant::now(),
        }
    }
}

pub(super) fn parse_percent(value: &str) -> f64 {
    value
        .trim()
        .trim_end_matches('%')
        .parse()
        .unwrap_or_default()
}

/// Parse Podman's `"<used> / <limit>"` strings.
pub(super) fn parse_io(value: &str) -> (u64, u64) {
    let mut parts = value.split('/');
    let used = parts.next().map(parse_size).unwrap_or_default();
    let limit = parts.next().map(parse_size).unwrap_or_default();
    (used, limit)
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub(super) struct RawContainer {
    id: String,
    #[serde(default)]
    names: Option<Vec<String>>,
    #[serde(default)]
    image: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    ports: Option<Vec<RawPort>>,
    #[serde(default)]
    labels: Option<HashMap<String, String>>,
}

// Fields are visible to the parent module so tests can build ports directly.
#[derive(Debug, Deserialize, Default)]
pub(super) struct RawPort {
    #[serde(default)]
    pub(super) host_ip: String,
    #[serde(default)]
    pub(super) host_port: u16,
    #[serde(default)]
    pub(super) container_port: u16,
    #[serde(default)]
    pub(super) protocol: String,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub(super) struct RawInspect {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    image_name: Option<String>,
    #[serde(default)]
    created: Option<String>,
    #[serde(default)]
    state: Option<RawState>,
    #[serde(default)]
    config: Option<RawConfig>,
    #[serde(default)]
    mounts: Option<Vec<RawMount>>,
    #[serde(default)]
    network_settings: Option<RawNetworkSettings>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub(super) struct RawState {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    pid: Option<i64>,
    #[serde(default)]
    exit_code: Option<i64>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub(super) struct RawConfig {
    #[serde(default)]
    image: Option<String>,
    #[serde(default)]
    entrypoint: Option<Vec<String>>,
    #[serde(default)]
    cmd: Option<Vec<String>>,
    #[serde(default)]
    env: Option<Vec<String>>,
    #[serde(default)]
    labels: Option<HashMap<String, String>>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub(super) struct RawMount {
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    destination: Option<String>,
    #[serde(default, rename = "Type")]
    kind: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub(super) struct RawNetworkSettings {
    #[serde(default)]
    networks: Option<BTreeMap<String, serde_json::Value>>,
}
