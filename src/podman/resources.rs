//! Pods, volumes and networks, including `kube play` and inspect.

use super::*;

#[derive(Debug, Clone, Default)]
pub struct Pod {
    pub id: String,
    pub name: String,
    pub status: String,
    pub created: String,
    pub containers: u64,
}

impl Pod {
    pub fn id_short(&self) -> String {
        short_id(&self.id)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Volume {
    pub name: String,
    pub driver: String,
    pub mountpoint: String,
    pub created: String,
    pub scope: String,
    pub mounts: u64,
}

#[derive(Debug, Clone, Default)]
pub struct Network {
    pub id: String,
    pub name: String,
    pub driver: String,
    pub interface: String,
    pub subnets: Vec<NetworkSubnet>,
    pub dns: bool,
}

impl Network {
    pub fn id_short(&self) -> String {
        short_id(&self.id)
    }

    pub fn subnet_label(&self) -> String {
        self.subnets
            .first()
            .map(|subnet| subnet.subnet.clone())
            .unwrap_or_else(|| "-".to_string())
    }
}

#[derive(Debug, Clone, Default)]
pub struct NetworkSubnet {
    pub subnet: String,
    pub gateway: String,
}

#[derive(Debug, Clone, Default)]
pub struct PodDetails {
    pub id: String,
    pub name: String,
    pub state: String,
    pub created: String,
    pub containers: u64,
    pub exit_policy: String,
    pub hostname: String,
    pub create_infra: bool,
}

#[derive(Debug, Clone, Default)]
pub struct VolumeDetails {
    pub name: String,
    pub driver: String,
    pub mountpoint: String,
    pub created: String,
    pub scope: String,
    pub anonymous: bool,
    pub mounts: u64,
}

#[derive(Debug, Clone, Default)]
pub struct NetworkDetails {
    pub id: String,
    pub name: String,
    pub driver: String,
    pub interface: String,
    pub created: String,
    pub subnets: Vec<NetworkSubnet>,
    pub internal: bool,
    pub ipv6: bool,
    pub dns: bool,
    pub containers: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PodAction {
    Start,
    Stop,
    Restart,
}

impl PodAction {
    pub fn verb(self) -> &'static str {
        match self {
            PodAction::Start => "start",
            PodAction::Stop => "stop",
            PodAction::Restart => "restart",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            PodAction::Start => "Starting pod",
            PodAction::Stop => "Stopping pod",
            PodAction::Restart => "Restarting pod",
        }
    }

    pub fn done(self) -> &'static str {
        match self {
            PodAction::Start => "Pod started",
            PodAction::Stop => "Pod stopped",
            PodAction::Restart => "Pod restarted",
        }
    }
}

pub fn pod_action(id: &str, action: PodAction) -> Result<String, String> {
    run(&["pod", action.verb(), id])
}

pub fn remove_pod(id: &str) -> Result<String, String> {
    run(&["pod", "rm", "-f", id])
}

pub fn remove_volume(name: &str) -> Result<String, String> {
    run(&["volume", "rm", "-f", name])
}

pub fn volume_create_command(name: &str, driver: Option<&str>) -> Result<Command, String> {
    let name = validate_volume_name(name)?;
    let driver = driver.map(str::trim).filter(|value| !value.is_empty());
    if let Some(driver) = driver {
        validate_driver(driver)?;
    }
    let mut command = base_command();
    command.args(["volume", "create"]);
    if let Some(driver) = driver {
        command.args(["--driver", driver]);
    }
    command.arg(name);
    Ok(command)
}

pub fn create_volume(name: &str, driver: Option<&str>) -> Result<String, String> {
    execute(volume_create_command(name, driver)?, "volume create")
}

pub fn prune_volumes() -> Result<String, String> {
    run(&["volume", "prune", "-f"])
}

pub fn network_create_command(
    name: &str,
    driver: Option<&str>,
    subnet: Option<&str>,
    gateway: Option<&str>,
) -> Result<Command, String> {
    let name = validate_network_name(name)?;
    let driver = driver.map(str::trim).filter(|value| !value.is_empty());
    let subnet = subnet.map(str::trim).filter(|value| !value.is_empty());
    let gateway = gateway.map(str::trim).filter(|value| !value.is_empty());
    if let Some(driver) = driver {
        validate_driver(driver)?;
    }
    if let Some(subnet) = subnet {
        validate_subnet(subnet)?;
    }
    if let Some(gateway) = gateway {
        validate_gateway(gateway)?;
    }

    let mut command = base_command();
    command.args(["network", "create"]);
    if let Some(driver) = driver {
        command.args(["--driver", driver]);
    }
    if let Some(subnet) = subnet {
        command.args(["--subnet", subnet]);
    }
    if let Some(gateway) = gateway {
        command.args(["--gateway", gateway]);
    }
    command.arg(name);
    Ok(command)
}

pub fn create_network(
    name: &str,
    driver: Option<&str>,
    subnet: Option<&str>,
    gateway: Option<&str>,
) -> Result<String, String> {
    execute(
        network_create_command(name, driver, subnet, gateway)?,
        "network create",
    )
}

pub fn prune_networks() -> Result<String, String> {
    run(&["network", "prune", "-f"])
}

pub fn pod_create_command(name: &str, ports: &[String]) -> Result<Command, String> {
    let name = validate_pod_name(name)?;
    for port in ports {
        validate_port_mapping(port)?;
    }
    let mut command = base_command();
    command.args(["pod", "create", "--name", name]);
    for port in ports {
        command.args(["-p", port]);
    }
    Ok(command)
}

pub fn create_pod(name: &str, ports: &[String]) -> Result<String, String> {
    execute(pod_create_command(name, ports)?, "pod create")
}

pub fn kube_play_command(path: &str) -> Result<Command, String> {
    let path = validate_kube_file(path)?;
    let mut command = base_command();
    command.args(["kube", "play", path]);
    Ok(command)
}

pub fn validate_kube_file(value: &str) -> Result<&str, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("Kubernetes YAML path cannot be empty".to_string());
    }
    if !std::path::Path::new(value).is_file() {
        return Err(format!("Kubernetes YAML file not found: {value}"));
    }
    Ok(value)
}

pub fn validate_driver(value: &str) -> Result<&str, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("driver cannot be empty".to_string());
    }
    if value.starts_with('-') {
        return Err("driver cannot start with '-'".to_string());
    }
    let valid = value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'));
    if !valid {
        return Err(format!("invalid driver: {value}"));
    }
    Ok(value)
}

pub fn validate_subnet(value: &str) -> Result<&str, String> {
    let value = value.trim();
    let Some((address, prefix)) = value.split_once('/') else {
        return Err(format!(
            "invalid subnet: {value} (expected CIDR, e.g. 10.88.0.0/24)"
        ));
    };
    let prefix: u8 = prefix
        .parse()
        .map_err(|_| format!("invalid subnet prefix: {value}"))?;
    address
        .parse::<std::net::Ipv4Addr>()
        .map_err(|_| format!("invalid IPv4 subnet: {value}"))?;
    if prefix > 32 {
        return Err(format!("invalid subnet prefix: {value}"));
    }
    Ok(value)
}

pub fn validate_gateway(value: &str) -> Result<&str, String> {
    value
        .trim()
        .parse::<std::net::Ipv4Addr>()
        .map_err(|_| format!("invalid IPv4 gateway: {value}"))?;
    Ok(value.trim())
}

pub fn validate_volume_name(value: &str) -> Result<&str, String> {
    validate_name(value, "volume")
}

pub fn validate_network_name(value: &str) -> Result<&str, String> {
    validate_name(value, "network")
}

pub fn validate_pod_name(value: &str) -> Result<&str, String> {
    validate_name(value, "pod")
}

pub fn remove_network(name: &str) -> Result<String, String> {
    run(&["network", "rm", "-f", name])
}

pub fn inspect_pod(id: &str) -> Result<PodDetails, String> {
    let json = query(&["pod", "inspect", id])?;
    parse_pod_inspect(json.trim()).map_err(|err| format!("could not inspect pod {id}: {err}"))
}

pub fn inspect_volume(name: &str) -> Result<VolumeDetails, String> {
    let json = query(&["volume", "inspect", name])?;
    parse_volume_inspect(json.trim())
        .map_err(|err| format!("could not inspect volume {name}: {err}"))
}

pub fn inspect_network(name: &str) -> Result<NetworkDetails, String> {
    let json = query(&["network", "inspect", name])?;
    parse_network_inspect(json.trim())
        .map_err(|err| format!("could not inspect network {name}: {err}"))
}

/// Resources that support `podman … inspect` for the raw JSON viewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InspectKind {
    Container,
    Image,
    Pod,
    Volume,
    Network,
}

impl InspectKind {
    pub(super) fn args(self, key: &str) -> Vec<String> {
        let prefix: &[&str] = match self {
            InspectKind::Container => &[],
            InspectKind::Image => &["image"],
            InspectKind::Pod => &["pod"],
            InspectKind::Volume => &["volume"],
            InspectKind::Network => &["network"],
        };
        let mut args: Vec<String> = prefix.iter().map(|arg| arg.to_string()).collect();
        args.push("inspect".to_string());
        args.push(key.to_string());
        args
    }
}

/// Pretty-printed `inspect` output for the raw JSON viewer.
pub fn inspect_json(kind: InspectKind, key: &str) -> Result<String, String> {
    let args = kind.args(key);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let raw = query(&refs)?;
    Ok(pretty_json(&raw))
}

pub(super) fn pretty_json(raw: &str) -> String {
    serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .and_then(|value| serde_json::to_string_pretty(&value).ok())
        .unwrap_or_else(|| raw.trim().to_string())
}

pub fn parse_pods(json: &str) -> Result<Vec<Pod>, String> {
    let raw: Vec<RawPod> = parse_json(json, "podman pod ls")?;
    Ok(raw
        .into_iter()
        .map(|pod| Pod {
            id: pod.id,
            name: pod.name,
            status: pod.status,
            created: display_created(&pod.created),
            containers: pod
                .containers
                .map_or(0, |containers| containers.len() as u64),
        })
        .collect())
}

pub fn parse_volumes(json: &str) -> Result<Vec<Volume>, String> {
    let raw: Vec<RawVolume> = parse_json(json, "podman volume ls")?;
    Ok(raw
        .into_iter()
        .map(|volume| Volume {
            name: volume.name,
            driver: volume.driver,
            mountpoint: volume.mountpoint,
            created: display_created(&volume.created_at),
            scope: volume.scope,
            mounts: volume.mount_count,
        })
        .collect())
}

pub fn parse_networks(json: &str) -> Result<Vec<Network>, String> {
    let raw: Vec<RawNetwork> = parse_json(json, "podman network ls")?;
    Ok(raw
        .into_iter()
        .map(|network| Network {
            id: network.id,
            name: network.name,
            driver: network.driver,
            interface: network.network_interface,
            subnets: network
                .subnets
                .unwrap_or_default()
                .into_iter()
                .map(|subnet| NetworkSubnet {
                    subnet: subnet.subnet,
                    gateway: subnet.gateway,
                })
                .collect(),
            dns: network.dns_enabled,
        })
        .collect())
}

pub(super) fn parse_pod_inspect(json: &str) -> Result<PodDetails, String> {
    let mut raw: Vec<RawPodInspect> = parse_json(json, "podman pod inspect")?;
    let raw = raw
        .pop()
        .ok_or_else(|| "no pod inspect data returned".to_string())?;
    Ok(PodDetails {
        id: raw.id,
        name: raw.name,
        state: raw.state,
        created: display_created(&raw.created),
        containers: raw.num_containers,
        exit_policy: raw.exit_policy,
        hostname: raw.hostname,
        create_infra: raw.create_infra,
    })
}

pub(super) fn parse_volume_inspect(json: &str) -> Result<VolumeDetails, String> {
    let mut raw: Vec<RawVolume> = parse_json(json, "podman volume inspect")?;
    let raw = raw
        .pop()
        .ok_or_else(|| "no volume inspect data returned".to_string())?;
    Ok(VolumeDetails {
        name: raw.name,
        driver: raw.driver,
        mountpoint: raw.mountpoint,
        created: display_created(&raw.created_at),
        scope: raw.scope,
        anonymous: raw.anonymous,
        mounts: raw.mount_count,
    })
}

pub(super) fn parse_network_inspect(json: &str) -> Result<NetworkDetails, String> {
    let mut raw: Vec<RawNetwork> = parse_json(json, "podman network inspect")?;
    let raw = raw
        .pop()
        .ok_or_else(|| "no network inspect data returned".to_string())?;
    Ok(NetworkDetails {
        id: raw.id,
        name: raw.name,
        driver: raw.driver,
        interface: raw.network_interface,
        created: display_created(&raw.created),
        subnets: raw
            .subnets
            .unwrap_or_default()
            .into_iter()
            .map(|subnet| NetworkSubnet {
                subnet: subnet.subnet,
                gateway: subnet.gateway,
            })
            .collect(),
        internal: raw.internal,
        ipv6: raw.ipv6_enabled,
        dns: raw.dns_enabled,
        containers: raw
            .containers
            .unwrap_or_default()
            .into_iter()
            .map(|(id, value)| {
                value
                    .get("name")
                    .and_then(|name| name.as_str())
                    .map(str::to_string)
                    .unwrap_or_else(|| short_id(&id))
            })
            .collect(),
    })
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub(super) struct RawPod {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    created: String,
    #[serde(default)]
    containers: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub(super) struct RawPodInspect {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    created: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    num_containers: u64,
    #[serde(default)]
    exit_policy: String,
    #[serde(default)]
    hostname: String,
    #[serde(default)]
    create_infra: bool,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub(super) struct RawVolume {
    name: String,
    #[serde(default)]
    driver: String,
    #[serde(default)]
    mountpoint: String,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    scope: String,
    #[serde(default)]
    anonymous: bool,
    #[serde(default)]
    mount_count: u64,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct RawNetwork {
    #[serde(default)]
    name: String,
    #[serde(default)]
    id: String,
    #[serde(default)]
    driver: String,
    #[serde(default)]
    network_interface: String,
    #[serde(default)]
    created: String,
    #[serde(default)]
    subnets: Option<Vec<RawSubnet>>,
    #[serde(default)]
    ipv6_enabled: bool,
    #[serde(default)]
    internal: bool,
    #[serde(default)]
    dns_enabled: bool,
    #[serde(default)]
    containers: Option<BTreeMap<String, serde_json::Value>>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct RawSubnet {
    #[serde(default)]
    subnet: String,
    #[serde(default)]
    gateway: String,
}
