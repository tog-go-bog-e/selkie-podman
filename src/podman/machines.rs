//! Podman machines and system connections (local configuration).

use super::*;

/// Data that describes the local Podman installation rather than an engine.
///
/// Kept separate from [`Snapshot`] so it stays visible when the engine (or a
/// remote connection) is unreachable.
#[derive(Debug, Clone, Default)]
pub struct LocalState {
    pub machines: Vec<Machine>,
    pub connections: Vec<Connection>,
}

#[derive(Debug, Clone, Default)]
pub struct Machine {
    pub name: String,
    pub running: bool,
    pub default: bool,
    pub cpus: u64,
    pub memory: u64,
    pub disk_size: u64,
    pub last_up: String,
    pub vm_type: String,
}

impl Machine {
    pub fn state_label(&self) -> &'static str {
        if self.running {
            "running"
        } else {
            "stopped"
        }
    }

    pub fn memory_label(&self) -> String {
        human_size(self.memory)
    }

    pub fn disk_label(&self) -> String {
        human_size(self.disk_size)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Connection {
    pub name: String,
    pub uri: String,
    pub identity: String,
    pub default: bool,
}

impl Connection {
    pub fn identity_label(&self) -> String {
        if self.identity.is_empty() {
            "-".to_string()
        } else {
            self.identity.clone()
        }
    }
}

pub fn machine_action(name: &str, action: MachineAction) -> Result<String, String> {
    engine::podman_only("machines")?;
    run_local(&["machine", action.verb(), name])
}

/// Options for `podman machine init`.
///
/// An empty `name` lets Podman use its own default machine name. Memory is in
/// MiB and disk size in GiB, matching the CLI flags.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MachineInitOptions {
    pub name: String,
    pub cpus: Option<u64>,
    pub memory: Option<u64>,
    pub disk: Option<u64>,
}

/// Validate the machine-init form. Empty numeric fields mean "use the default".
pub fn parse_machine_init(
    name: &str,
    cpus: &str,
    memory: &str,
    disk: &str,
) -> Result<MachineInitOptions, String> {
    let name = name.trim();
    if !name.is_empty() {
        validate_machine_name(name)?;
    }
    Ok(MachineInitOptions {
        name: name.to_string(),
        cpus: parse_machine_number(cpus, "--cpus", MACHINE_CPUS)?,
        memory: parse_machine_number(memory, "--memory", MACHINE_MEMORY)?,
        disk: parse_machine_number(disk, "--disk-size", MACHINE_DISK)?,
    })
}

pub(super) const MACHINE_CPUS: (u64, u64) = (1, 128);

/// MiB.
pub(super) const MACHINE_MEMORY: (u64, u64) = (512, 262_144);

/// GiB.
pub(super) const MACHINE_DISK: (u64, u64) = (1, 4096);

pub(super) fn parse_machine_number(
    value: &str,
    flag: &str,
    (min, max): (u64, u64),
) -> Result<Option<u64>, String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    let number: u64 = value
        .parse()
        .map_err(|_| format!("{flag} must be a whole number (got “{value}”)"))?;
    if !(min..=max).contains(&number) {
        return Err(format!(
            "{flag} must be between {min} and {max} (got {number})"
        ));
    }
    Ok(Some(number))
}

pub fn machine_init_command(options: &MachineInitOptions) -> Result<Command, String> {
    engine::podman_only("machines")?;
    if !options.name.is_empty() {
        validate_machine_name(&options.name)?;
    }
    for (flag, value, (min, max)) in [
        ("--cpus", options.cpus, MACHINE_CPUS),
        ("--memory", options.memory, MACHINE_MEMORY),
        ("--disk-size", options.disk, MACHINE_DISK),
    ] {
        if let Some(value) = value {
            if !(min..=max).contains(&value) {
                return Err(format!(
                    "{flag} must be between {min} and {max} (got {value})"
                ));
            }
        }
    }

    let mut command = Command::new("podman");
    command.args(["machine", "init"]);
    for (flag, value) in [
        ("--cpus", options.cpus),
        ("--memory", options.memory),
        ("--disk-size", options.disk),
    ] {
        if let Some(value) = value {
            command.args([flag, &value.to_string()]);
        }
    }
    if !options.name.is_empty() {
        command.arg(&options.name);
    }
    Ok(command)
}

/// The command line echoed in the output panel, without the leading `$`.
pub fn machine_init_display(options: &MachineInitOptions) -> String {
    let mut line = "podman machine init".to_string();
    for (flag, value) in [
        ("--cpus", options.cpus),
        ("--memory", options.memory),
        ("--disk-size", options.disk),
    ] {
        if let Some(value) = value {
            line.push_str(&format!(" {flag} {value}"));
        }
    }
    if !options.name.is_empty() {
        line.push(' ');
        line.push_str(&options.name);
    }
    line
}

pub fn machine_remove(name: &str) -> Result<String, String> {
    engine::podman_only("machines")?;
    run_local(&["machine", "rm", "-f", name])
}

/// `podman machine ssh` for the details terminal. Machine commands are local, so
/// they deliberately ignore the selected engine connection.
pub fn machine_ssh_command(name: &str) -> Result<Command, String> {
    engine::podman_only("machines")?;
    let name = validate_machine_name(name)?;
    let mut command = Command::new("podman");
    command.args(["machine", "ssh", name]);
    Ok(command)
}

pub fn validate_machine_name(value: &str) -> Result<&str, String> {
    validate_name(value, "machine")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MachineAction {
    Start,
    Stop,
}

impl MachineAction {
    pub fn verb(self) -> &'static str {
        match self {
            MachineAction::Start => "start",
            MachineAction::Stop => "stop",
        }
    }

    pub fn gerund(self) -> &'static str {
        match self {
            MachineAction::Start => "Starting",
            MachineAction::Stop => "Stopping",
        }
    }

    pub fn done(self) -> &'static str {
        match self {
            MachineAction::Start => "started",
            MachineAction::Stop => "stopped",
        }
    }
}

/// Machines and connections come from local Podman configuration, so they stay
/// available even when the selected engine is unreachable.
pub fn local_state() -> LocalState {
    if !engine::has_machines() {
        return LocalState::default();
    }
    LocalState {
        machines: machine_list().unwrap_or_default(),
        connections: connection_list().unwrap_or_default(),
    }
}

pub fn machine_list() -> Result<Vec<Machine>, String> {
    engine::podman_only("machines")?;
    let json = query_local(&["machine", "list", "--format", "json"])?;
    parse_machines(&json)
}

pub(super) fn parse_machines(json: &str) -> Result<Vec<Machine>, String> {
    let raw: Vec<RawMachine> = parse_json(json, "podman machine list")?;
    Ok(raw
        .into_iter()
        .filter(|machine| !machine.name.is_empty())
        .map(|machine| Machine {
            name: machine.name,
            running: machine.running,
            default: machine.default,
            cpus: machine.cpus.unwrap_or_default(),
            memory: machine.memory.unwrap_or_default(),
            disk_size: machine.disk_size.unwrap_or_default(),
            last_up: machine
                .last_up
                .as_deref()
                .filter(|timestamp| !timestamp.starts_with("0001-01-01"))
                .map_or_else(|| "-".to_string(), |value| format_timestamp(Some(value))),
            vm_type: machine.vm_type.unwrap_or_else(|| "-".to_string()),
        })
        .collect())
}

pub fn connection_list() -> Result<Vec<Connection>, String> {
    engine::podman_only("connections")?;
    let json = query_local(&["system", "connection", "list", "--format", "json"])?;
    parse_connections(&json)
}

/// How long a reachability probe may take before the connection is considered
/// unreachable. Shorter than [`QUERY_TIMEOUT`] because several probes can run
/// at once and the SSH handshake either works quickly or not at all.
const CONNECTION_CHECK_TIMEOUT: Duration = Duration::from_secs(15);

/// Check that `podman --connection <name> info` answers. This never changes the
/// connection the app uses.
pub fn probe_connection(name: &str) -> Result<(), String> {
    engine::podman_only("connections")?;
    let name = name.trim();
    if name.is_empty() {
        return Err("connection name cannot be empty".to_string());
    }
    let mut command = Command::new("podman");
    apply_ssh_agent(&mut command);
    command.args([
        "--connection",
        name,
        "info",
        "--format",
        "{{.Version.Version}}",
    ]);
    execute_with(
        command,
        &format!("--connection {name} info"),
        Some(CONNECTION_CHECK_TIMEOUT),
    )
    .map(|_| ())
}

/// Names for new connections: letters, digits, `-`, `_` and `.`.
pub fn validate_connection_name(value: &str) -> Result<&str, String> {
    validate_name(value, "connection")
}

/// Only the transports this app can hand to Podman without extra settings:
/// SSH (`ssh://[user@]host[:port][/socket]`) and a local Unix socket.
pub fn validate_connection_uri(value: &str) -> Result<&str, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("connection URI cannot be empty".to_string());
    }
    if value.starts_with('-') {
        return Err("connection URI cannot start with '-'".to_string());
    }
    if value.chars().any(char::is_whitespace) {
        return Err("connection URI cannot contain whitespace".to_string());
    }
    if let Some(rest) = value.strip_prefix("ssh://") {
        if rest.is_empty() {
            return Err("connection URI needs a host (ssh://user@host:22/path)".to_string());
        }
    } else if let Some(rest) = value.strip_prefix("unix://") {
        if !rest.starts_with('/') {
            return Err("a unix:// URI needs an absolute socket path".to_string());
        }
    } else {
        return Err("connection URI must start with ssh:// or unix://".to_string());
    }
    Ok(value)
}

/// Build `podman system connection add` for a new connection. Connection
/// commands are local, so the selected engine is deliberately not applied.
pub fn connection_add_command(
    name: &str,
    uri: &str,
    identity: Option<&str>,
) -> Result<Command, String> {
    engine::podman_only("connections")?;
    let name = validate_connection_name(name)?;
    let uri = validate_connection_uri(uri)?;
    let identity = identity
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(expand_home);
    if let Some(identity) = &identity {
        if uri.starts_with("unix://") {
            return Err("an identity file only applies to ssh:// connections".to_string());
        }
        if !std::path::Path::new(identity).is_file() {
            return Err(format!("identity file not found: {identity}"));
        }
    }

    // For ssh:// URIs Podman dials the host while adding, so it needs the agent.
    let mut command = Command::new("podman");
    apply_ssh_agent(&mut command);
    command.args(["system", "connection", "add"]);
    if let Some(identity) = &identity {
        command.args(["--identity", identity]);
    }
    command.args([name, uri]);
    Ok(command)
}

pub fn connection_add(name: &str, uri: &str, identity: Option<&str>) -> Result<String, String> {
    let command = connection_add_command(name, uri, identity)?;
    execute(command, &format!("system connection add {name}"))
}

/// Update a connection in place. Podman has no `edit`, but re-running `add`
/// with an existing name replaces its URI and identity; a rename keeps the
/// default flag, so it is applied before the update.
///
/// A rename onto a name that is already configured is refused (checked against
/// Podman's current list, not the UI's), since `add` would overwrite that
/// connection. If `add` fails after a rename, the rename is rolled back so the
/// configuration is left as it was.
pub fn connection_update(
    old_name: &str,
    name: &str,
    uri: &str,
    identity: Option<&str>,
) -> Result<String, String> {
    engine::podman_only("connections")?;
    let command = connection_add_command(name, uri, identity)?;
    let old_name = old_name.trim();
    let name = name.trim();
    if old_name == name {
        return execute(command, &format!("system connection add {name}"));
    }

    if connection_list()?
        .iter()
        .any(|connection| connection.name == name)
    {
        return Err(format!("a connection named {name} already exists"));
    }
    run_local(&["system", "connection", "rename", old_name, name])?;
    execute(command, &format!("system connection add {name}")).map_err(|error| {
        match run_local(&["system", "connection", "rename", name, old_name]) {
            Ok(_) => error,
            Err(rollback) => format!(
                "{error} (the connection is now named {name}; renaming it back failed: {rollback})"
            ),
        }
    })
}

/// Remove a connection from Podman's configuration. Local, like `add`.
pub fn connection_remove(name: &str) -> Result<String, String> {
    engine::podman_only("connections")?;
    let name = name.trim();
    if name.is_empty() {
        return Err("connection name cannot be empty".to_string());
    }
    run_local(&["system", "connection", "remove", name])
}

/// `~` is expanded here because the command is spawned without a shell.
fn expand_home(path: &str) -> String {
    match (path.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => std::path::Path::new(&home)
            .join(rest)
            .to_string_lossy()
            .into_owned(),
        _ => path.to_string(),
    }
}

pub(super) fn parse_connections(json: &str) -> Result<Vec<Connection>, String> {
    let raw: Vec<RawConnection> = parse_json(json, "podman system connection list")?;
    Ok(raw
        .into_iter()
        .filter(|connection| !connection.name.is_empty())
        .map(|connection| Connection {
            name: connection.name,
            uri: connection.uri,
            identity: connection.identity,
            default: connection.default,
        })
        .collect())
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub(super) struct RawMachine {
    #[serde(default)]
    name: String,
    #[serde(default)]
    default: bool,
    #[serde(default)]
    running: bool,
    #[serde(default)]
    last_up: Option<String>,
    #[serde(default, rename = "VMType")]
    vm_type: Option<String>,
    #[serde(default, rename = "CPUs", deserialize_with = "lenient_u64")]
    cpus: Option<u64>,
    #[serde(default, deserialize_with = "lenient_u64")]
    memory: Option<u64>,
    #[serde(default, deserialize_with = "lenient_u64")]
    disk_size: Option<u64>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub(super) struct RawConnection {
    #[serde(default)]
    name: String,
    #[serde(default, rename = "URI")]
    uri: String,
    #[serde(default)]
    identity: String,
    #[serde(default)]
    default: bool,
}
