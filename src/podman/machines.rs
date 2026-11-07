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
    run_local(&["machine", "rm", "-f", name])
}

/// `podman machine ssh` for the details terminal. Machine commands are local, so
/// they deliberately ignore the selected engine connection.
pub fn machine_ssh_command(name: &str) -> Result<Command, String> {
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
    LocalState {
        machines: machine_list().unwrap_or_default(),
        connections: connection_list().unwrap_or_default(),
    }
}

pub fn machine_list() -> Result<Vec<Machine>, String> {
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
    let json = query_local(&["system", "connection", "list", "--format", "json"])?;
    parse_connections(&json)
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
