//! The engine snapshot and the Dashboard's `podman info` / `system df` data.

use super::*;

/// Everything the UI needs to render one frame.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub version: String,
    pub images: Vec<Image>,
    pub containers: Vec<Container>,
    pub pods: Vec<Pod>,
    pub volumes: Vec<Volume>,
    pub networks: Vec<Network>,
    pub secrets: Vec<Secret>,
    pub compose: Vec<ComposeProject>,
}

/// Fetch the engine version, all images and all containers.
pub fn snapshot() -> Result<Snapshot, String> {
    if engine::is_docker() {
        return docker::snapshot();
    }

    let version = query(&["--version"])?.trim().to_string();
    let images_raw = query(&["images", "--format", "json"])?;
    let containers_raw = query(&["ps", "-a", "--format", "json"])?;
    let pods_raw = query(&["pod", "ls", "--format", "json"])?;
    let volumes_raw = query(&["volume", "ls", "--format", "json"])?;
    let networks_raw = query(&["network", "ls", "--format", "json"])?;

    // Secrets need Podman 3.1+; an engine without them still gets a snapshot.
    let secrets = secret_list().unwrap_or_default();

    let containers = parse_containers(&containers_raw)?;
    let compose = compose_projects(&containers);

    Ok(Snapshot {
        version,
        images: parse_images(&images_raw)?,
        containers,
        pods: parse_pods(&pods_raw)?,
        volumes: parse_volumes(&volumes_raw)?,
        networks: parse_networks(&networks_raw)?,
        secrets,
        compose,
    })
}

/// Host and store details from `podman info`, for the Dashboard.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SystemInfo {
    pub version: String,
    pub os_arch: String,
    pub hostname: String,
    pub kernel: String,
    pub cpus: u64,
    pub memory_total: u64,
    /// `None` when the engine does not report it (Docker's API has no free-memory
    /// figure, and the local host's would be wrong for a VM or remote daemon).
    pub memory_free: Option<u64>,
    pub rootless: bool,
    pub remote: bool,
    pub storage_driver: String,
    pub graph_root: String,
    pub event_logger: String,
    pub runtime: String,
    pub network_backend: String,
    pub socket_path: String,
    pub socket_present: bool,
}

impl SystemInfo {
    pub fn mode_label(&self) -> &'static str {
        if self.rootless {
            "rootless"
        } else {
            "rootful"
        }
    }

    pub fn memory_label(&self) -> String {
        match self.memory_free {
            Some(free) => format!(
                "{} free of {} ({}% used)",
                human_size(free),
                human_size(self.memory_total),
                self.memory_used_percent().unwrap_or(0)
            ),
            None => format!("{} total", human_size(self.memory_total)),
        }
    }

    /// Share of host memory in use, 0-100, when the engine reports free memory.
    pub fn memory_used_percent(&self) -> Option<u64> {
        let free = self.memory_free?;
        if self.memory_total == 0 {
            return None;
        }
        Some(self.memory_total.saturating_sub(free).saturating_mul(100) / self.memory_total)
    }
}

/// One row of `podman system df`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiskUsage {
    pub kind: String,
    pub total: u64,
    pub active: u64,
    pub size: u64,
    pub reclaimable: u64,
}

impl DiskUsage {
    pub fn size_label(&self) -> String {
        human_size(self.size)
    }

    pub fn reclaimable_label(&self) -> String {
        if self.size == 0 {
            "nothing to reclaim".to_string()
        } else {
            format!(
                "{} reclaimable ({}%)",
                human_size(self.reclaimable),
                self.reclaimable_percent()
            )
        }
    }

    /// Share of the used space that is reclaimable, 0-100.
    pub fn reclaimable_percent(&self) -> u64 {
        if self.size == 0 {
            return 0;
        }
        (self.reclaimable.saturating_mul(100) / self.size).min(100)
    }
}

/// Everything the Dashboard needs: engine details plus disk usage.
#[derive(Debug, Clone, Default)]
pub struct Dashboard {
    pub info: SystemInfo,
    pub disk: Vec<DiskUsage>,
}

pub fn dashboard() -> Result<Dashboard, String> {
    if engine::is_docker() {
        return docker::dashboard();
    }

    Ok(Dashboard {
        info: system_info()?,
        disk: disk_usage()?,
    })
}

pub(super) fn system_info() -> Result<SystemInfo, String> {
    let json = query(&["info", "--format", "json"])?;
    parse_system_info(&json)
}

pub(super) fn parse_system_info(json: &str) -> Result<SystemInfo, String> {
    let raw: RawInfo = serde_json::from_str(json.trim())
        .map_err(|err| format!("could not parse `podman info` output: {err}"))?;
    Ok(raw.into_system_info())
}

pub(super) fn disk_usage() -> Result<Vec<DiskUsage>, String> {
    let json = query(&["system", "df", "--format", "json"])?;
    parse_disk_usage(&json)
}

pub(super) fn parse_disk_usage(json: &str) -> Result<Vec<DiskUsage>, String> {
    let raw: Vec<RawDiskUsage> = parse_json(json, "podman system df")?;
    Ok(raw
        .into_iter()
        .map(|row| DiskUsage {
            kind: match row.kind.as_str() {
                "Local Volumes" => "Volumes".to_string(),
                other => other.to_string(),
            },
            total: row.total,
            active: row.active,
            size: row.size,
            reclaimable: row.reclaimable,
        })
        .collect())
}

#[derive(Deserialize, Default)]
pub(super) struct RawInfo {
    #[serde(default)]
    host: RawInfoHost,
    #[serde(default)]
    store: RawInfoStore,
    #[serde(default)]
    version: RawInfoVersion,
}

#[derive(Deserialize, Default)]
pub(super) struct RawInfoHost {
    #[serde(default)]
    os: String,
    #[serde(default)]
    arch: String,
    #[serde(default)]
    hostname: String,
    #[serde(default)]
    kernel: String,
    #[serde(default)]
    cpus: u64,
    #[serde(default, rename = "memFree")]
    memory_free: u64,
    #[serde(default, rename = "memTotal")]
    memory_total: u64,
    #[serde(default, rename = "eventLogger")]
    event_logger: String,
    #[serde(default, rename = "networkBackend")]
    network_backend: String,
    #[serde(default, rename = "serviceIsRemote")]
    service_is_remote: bool,
    #[serde(default)]
    security: RawInfoSecurity,
    #[serde(default, rename = "ociRuntime")]
    oci_runtime: RawInfoRuntime,
    #[serde(default, rename = "remoteSocket")]
    remote_socket: Option<RawInfoSocket>,
}

#[derive(Deserialize, Default)]
pub(super) struct RawInfoSecurity {
    #[serde(default)]
    rootless: bool,
}

#[derive(Deserialize, Default)]
pub(super) struct RawInfoRuntime {
    #[serde(default)]
    name: String,
    #[serde(default)]
    version: String,
}

#[derive(Deserialize, Default)]
pub(super) struct RawInfoSocket {
    #[serde(default)]
    path: String,
    #[serde(default)]
    exists: bool,
}

#[derive(Deserialize, Default)]
pub(super) struct RawInfoStore {
    #[serde(default, rename = "graphDriverName")]
    storage_driver: String,
    #[serde(default, rename = "graphRoot")]
    graph_root: String,
}

#[derive(Deserialize, Default)]
pub(super) struct RawInfoVersion {
    #[serde(default, rename = "Version")]
    version: String,
    #[serde(default, rename = "OsArch")]
    os_arch: String,
}

impl RawInfo {
    fn into_system_info(self) -> SystemInfo {
        let os_arch = if !self.version.os_arch.is_empty() {
            self.version.os_arch
        } else if !self.host.os.is_empty() || !self.host.arch.is_empty() {
            format!("{}/{}", self.host.os, self.host.arch)
        } else {
            String::new()
        };
        let (socket_path, socket_present) = match self.host.remote_socket {
            Some(socket) => (socket.path, socket.exists),
            None => (String::new(), false),
        };
        SystemInfo {
            version: self.version.version,
            os_arch,
            hostname: self.host.hostname,
            kernel: self.host.kernel,
            cpus: self.host.cpus,
            memory_total: self.host.memory_total,
            memory_free: Some(self.host.memory_free),
            rootless: self.host.security.rootless,
            remote: self.host.service_is_remote,
            storage_driver: self.store.storage_driver,
            graph_root: self.store.graph_root,
            event_logger: self.host.event_logger,
            runtime: short_runtime(&self.host.oci_runtime.name, &self.host.oci_runtime.version),
            network_backend: self.host.network_backend,
            socket_path,
            socket_present,
        }
    }
}

/// `crun version 1.21\ncommit: …` → `crun 1.21`.
pub(super) fn short_runtime(name: &str, version: &str) -> String {
    let first = version
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .trim_end_matches(',');
    let without_name = first.strip_prefix(name).unwrap_or(first).trim();
    let number = without_name.strip_prefix("version").unwrap_or(without_name);
    let number = number.trim();
    if number.is_empty() {
        name.to_string()
    } else {
        format!("{name} {number}")
    }
}

#[derive(Deserialize)]
pub(super) struct RawDiskUsage {
    #[serde(default, rename = "Type")]
    kind: String,
    #[serde(default, rename = "Total")]
    total: u64,
    #[serde(default, rename = "Active")]
    active: u64,
    #[serde(default, rename = "RawSize")]
    size: u64,
    #[serde(default, rename = "RawReclaimable")]
    reclaimable: u64,
}
