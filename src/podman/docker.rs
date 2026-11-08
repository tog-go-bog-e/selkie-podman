//! Docker Engine backend, implemented with the Docker HTTP API through
//! [`bollard`].
//!
//! Everything this module exposes is synchronous: the UI already calls the
//! engine client from GPUI's background executor, so a small dedicated Tokio
//! runtime bridges bollard's futures. That keeps the rest of the app unchanged
//! and means the Docker socket is used directly, without a `docker` process.

use std::future::Future;

use bollard::models as model;
use bollard::query_parameters as query;
use bollard::Docker;
use futures_util::StreamExt;

use super::*;

// ---------------------------------------------------------------------------
// Runtime and connection
// ---------------------------------------------------------------------------

fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("selkie-docker")
            .build()
            .expect("failed to start the Docker runtime")
    })
}

/// How long a resolved Docker host is trusted before `docker context` is
/// consulted again (the user may run `docker context use` meanwhile).
const HOST_TTL: Duration = Duration::from_secs(15);

struct CachedClient {
    host: String,
    docker: Docker,
    resolved_at: Instant,
}

fn client_slot() -> &'static std::sync::Mutex<Option<CachedClient>> {
    static CLIENT: OnceLock<std::sync::Mutex<Option<CachedClient>>> = OnceLock::new();
    CLIENT.get_or_init(|| std::sync::Mutex::new(None))
}

/// The API client for the daemon the `docker` CLI targets, so the tables and
/// the CLI-driven actions (logs, exec, compose, build, run) see the same engine.
///
/// The client, and the API version negotiated with the daemon, are reused
/// until the resolved host changes; the host is re-resolved every
/// [`HOST_TTL`].
fn client() -> Result<Docker, String> {
    let mut slot = client_slot()
        .lock()
        .map_err(|_| "the Docker client cache is poisoned".to_string())?;
    if let Some(cached) = slot.as_mut() {
        if cached.resolved_at.elapsed() < HOST_TTL {
            return Ok(cached.docker.clone());
        }
        let host = docker_host();
        if host == cached.host {
            cached.resolved_at = Instant::now();
            return Ok(cached.docker.clone());
        }
    }

    let host = docker_host();
    let docker = connect(&host)?;
    *slot = Some(CachedClient {
        host,
        docker: docker.clone(),
        resolved_at: Instant::now(),
    });
    Ok(docker)
}

fn connect(host: &str) -> Result<Docker, String> {
    let docker = if host.starts_with("unix://") {
        Docker::connect_with_unix(host, 120, bollard::API_DEFAULT_VERSION)
    } else if host.starts_with("ssh://") {
        return Err(format!(
            "the Docker host {host} uses SSH, which Selkie's Docker support does not handle yet; \
             expose the remote daemon over a local socket (ssh -L) or use a local context"
        ));
    } else {
        Docker::connect_with_host(host)
    }
    .map_err(|error| format!("could not connect to the Docker engine at {host}: {error}"))?;
    // bollard defaults to the newest API version; ask the daemon which version
    // it supports so older Docker installations are not rejected.
    runtime()
        .block_on(docker.negotiate_version())
        .map_err(|error| format!("could not negotiate the Docker API version with {host}: {error}"))
}

/// The daemon address the `docker` CLI would use: `DOCKER_HOST`, else the
/// endpoint of the active context (`DOCKER_CONTEXT` or `docker context use`,
/// e.g. Docker Desktop's `desktop-linux`), else the usual local sockets.
fn docker_host() -> String {
    if let Some(host) = std::env::var("DOCKER_HOST")
        .ok()
        .map(|host| host.trim().to_string())
        .filter(|host| !host.is_empty())
    {
        return host;
    }
    let context = run_command(
        Command::new("docker"),
        &[
            "context",
            "inspect",
            "--format",
            "{{.Endpoints.docker.Host}}",
        ],
        Some(QUERY_TIMEOUT),
    );
    if let Ok(host) = context {
        let host = host.trim();
        if !host.is_empty() {
            return host.to_string();
        }
    }
    format!("unix://{}", local_socket_path())
}

/// The local daemon socket when no context says otherwise.
fn local_socket_path() -> String {
    let default = "/var/run/docker.sock";
    if std::path::Path::new(default).exists() {
        return default.to_string();
    }
    // Rootless Docker uses the per-user runtime directory instead.
    if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") {
        let socket = std::path::Path::new(&runtime).join("docker.sock");
        if socket.exists() {
            return socket.display().to_string();
        }
    }
    default.to_string()
}

/// The host currently in use, without resolving it again.
fn current_host() -> String {
    client_slot()
        .lock()
        .ok()
        .and_then(|slot| slot.as_ref().map(|cached| cached.host.clone()))
        .unwrap_or_else(docker_host)
}

/// Everything except a Unix socket (or a Windows named pipe) reaches the
/// daemon over the network.
fn is_remote_host(host: &str) -> bool {
    !(host.starts_with("unix://") || host.starts_with("npipe://"))
}

fn call<T>(future: impl Future<Output = Result<T, bollard::errors::Error>>) -> Result<T, String> {
    runtime()
        .block_on(future)
        .map_err(|error| format!("Docker engine error: {error}"))
}

// ---------------------------------------------------------------------------
// Snapshot
// ---------------------------------------------------------------------------

pub(super) fn snapshot() -> Result<Snapshot, String> {
    let docker = client()?;
    let version = call(docker.version())?;
    let version = match version.version.filter(|value| !value.is_empty()) {
        Some(version) => format!("Docker {version}"),
        None => "Docker".to_string(),
    };

    let containers = list_containers(&docker)?;
    let compose = compose_projects(&containers);

    Ok(Snapshot {
        version,
        images: list_images(&docker)?,
        containers,
        pods: Vec::new(),
        volumes: list_volumes(&docker)?,
        networks: list_networks(&docker)?,
        // Docker secrets belong to Swarm services, not standalone containers.
        secrets: Vec::new(),
        compose,
    })
}

fn list_images(docker: &Docker) -> Result<Vec<Image>, String> {
    // Like `docker images`: intermediate build layers are not listed.
    let options = query::ListImagesOptionsBuilder::default()
        .all(false)
        .build();
    let images = call(docker.list_images(Some(options)))?;
    let mut seen = HashSet::new();
    Ok(images
        .into_iter()
        .filter(|image| !image.id.is_empty() && seen.insert(image.id.clone()))
        .map(|image| {
            let name = image
                .repo_tags
                .iter()
                .find(|tag| !tag.is_empty())
                .cloned()
                .unwrap_or_else(|| "<none>".to_string());
            let digest = image
                .repo_digests
                .iter()
                .find(|digest| !digest.is_empty())
                .map(|digest| digest.split('@').next_back().unwrap_or(digest).to_string())
                .unwrap_or_default();
            Image {
                id: image.id,
                name,
                digest,
                size: image.size.max(0) as u64,
                containers: image.containers.max(0) as u64,
                created: format_epoch(image.created),
            }
        })
        .collect())
}

fn list_containers(docker: &Docker) -> Result<Vec<Container>, String> {
    let options = query::ListContainersOptionsBuilder::default()
        .all(true)
        .build();
    let containers = call(docker.list_containers(Some(options)))?;
    Ok(containers
        .into_iter()
        .map(|container| {
            let id = container.id.unwrap_or_default();
            let name = container
                .names
                .unwrap_or_default()
                .into_iter()
                .find_map(|name| {
                    let name = name.trim_start_matches('/').to_string();
                    (!name.is_empty()).then_some(name)
                })
                .unwrap_or_else(|| short_id(&id));
            let ports = container.ports.unwrap_or_default();
            Container {
                id,
                name,
                image: container.image.unwrap_or_else(|| "<none>".to_string()),
                state: container
                    .state
                    .map(|state| state.to_string())
                    .unwrap_or_else(|| "unknown".to_string()),
                status: container.status.unwrap_or_default(),
                ports: docker_ports_label(&ports),
                compose: super::compose::compose_membership(container.labels.as_ref()),
            }
        })
        .collect())
}

fn list_volumes(docker: &Docker) -> Result<Vec<Volume>, String> {
    let response = call(docker.list_volumes(None::<query::ListVolumesOptions>))?;
    Ok(response
        .volumes
        .unwrap_or_default()
        .into_iter()
        .map(|volume| Volume {
            name: volume.name,
            driver: volume.driver,
            mountpoint: volume.mountpoint,
            created: volume
                .created_at
                .map(|created| display_created(&created.to_string()))
                .unwrap_or_else(|| "-".to_string()),
            scope: volume
                .scope
                .map(|scope| scope.to_string())
                .unwrap_or_else(|| "local".to_string()),
            mounts: volume
                .usage_data
                .map(|usage| usage.ref_count.max(0) as u64)
                .unwrap_or_default(),
        })
        .collect())
}

fn list_networks(docker: &Docker) -> Result<Vec<Network>, String> {
    let networks = call(docker.list_networks(None::<query::ListNetworksOptions>))?;
    Ok(networks
        .into_iter()
        .map(|network| {
            let name = network.name.unwrap_or_default();
            let subnets = network
                .ipam
                .and_then(|ipam| ipam.config)
                .unwrap_or_default()
                .into_iter()
                .map(|config| NetworkSubnet {
                    subnet: config.subnet.unwrap_or_default(),
                    gateway: config.gateway.unwrap_or_default(),
                })
                .collect();
            let dns = !matches!(name.as_str(), "bridge" | "host" | "none");
            Network {
                id: network.id.unwrap_or_default(),
                name,
                driver: network.driver.unwrap_or_default(),
                interface: String::new(),
                subnets,
                dns,
            }
        })
        .collect())
}

fn docker_ports_label(ports: &[model::PortSummary]) -> String {
    if ports.is_empty() {
        return "-".to_string();
    }

    ports
        .iter()
        .map(|port| {
            let protocol = port
                .typ
                .map(|kind| kind.to_string())
                .filter(|kind| !kind.is_empty())
                .unwrap_or_else(|| "tcp".to_string());
            match port.public_port {
                Some(host_port) => {
                    let host_ip = port.ip.as_deref().unwrap_or_default();
                    if host_ip.is_empty() || host_ip == "0.0.0.0" || host_ip == "::" {
                        format!("{host_port}->{}/{protocol}", port.private_port)
                    } else {
                        format!("{host_ip}:{host_port}->{}/{protocol}", port.private_port)
                    }
                }
                None => format!("{}/{protocol}", port.private_port),
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

// ---------------------------------------------------------------------------
// Dashboard
// ---------------------------------------------------------------------------

pub(super) fn dashboard() -> Result<Dashboard, String> {
    let docker = client()?;
    let info = call(docker.info())?;
    let disk = disk_usage(&docker)?;
    Ok(Dashboard {
        info: system_info(info),
        disk,
    })
}

fn system_info(info: model::SystemInfo) -> SystemInfo {
    let os_arch = match (info.os_type, info.architecture) {
        (Some(os), Some(arch)) if !os.is_empty() || !arch.is_empty() => format!("{os}/{arch}"),
        _ => String::new(),
    };
    let host = current_host();
    let remote = is_remote_host(&host);
    let socket_path = host.strip_prefix("unix://").unwrap_or(&host).to_string();
    // Only a local socket is a file to look for; the dashboard renders only
    // once the engine answered, so a network address counts as present.
    let socket_present = remote || std::path::Path::new(&socket_path).exists();
    let rootless = info
        .security_options
        .unwrap_or_default()
        .iter()
        .any(|option| option.contains("rootless"));

    SystemInfo {
        version: info
            .server_version
            .map(|version| format!("Docker {version}"))
            .unwrap_or_else(|| "Docker".to_string()),
        os_arch,
        // The daemon's own host name: for Docker Desktop's VM or a remote
        // daemon this is not the machine Selkie runs on.
        hostname: info.name.unwrap_or_default(),
        kernel: info.kernel_version.unwrap_or_default(),
        cpus: info.ncpu.unwrap_or_default().max(0) as u64,
        memory_total: info.mem_total.unwrap_or_default().max(0) as u64,
        // The API reports total memory only.
        memory_free: None,
        rootless,
        remote,
        storage_driver: info.driver.unwrap_or_default(),
        graph_root: info.docker_root_dir.unwrap_or_default(),
        event_logger: info.logging_driver.unwrap_or_default(),
        runtime: info.default_runtime.unwrap_or_default(),
        network_backend: String::new(),
        socket_path,
        socket_present,
    }
}

fn disk_usage(docker: &Docker) -> Result<Vec<DiskUsage>, String> {
    let usage = call(docker.df(None::<query::DataUsageOptions>))?;
    let mut rows = Vec::new();
    if let Some(images) = usage.image_usage {
        rows.push(usage_row(
            "Images",
            images.active_count,
            images.total_count,
            images.total_size,
            images.reclaimable,
        ));
    }
    if let Some(containers) = usage.container_usage {
        rows.push(usage_row(
            "Containers",
            containers.active_count,
            containers.total_count,
            containers.total_size,
            containers.reclaimable,
        ));
    }
    if let Some(volumes) = usage.volume_usage {
        rows.push(usage_row(
            "Volumes",
            volumes.active_count,
            volumes.total_count,
            volumes.total_size,
            volumes.reclaimable,
        ));
    }
    if rows.is_empty() {
        // Older daemons answer `/system/df` with the legacy resource arrays
        // instead of the aggregate `*Usage` objects; derive the same rows from
        // the endpoints we already understand.
        rows = legacy_disk_usage(docker)?;
    }
    Ok(rows)
}

fn legacy_disk_usage(docker: &Docker) -> Result<Vec<DiskUsage>, String> {
    let images = list_images(docker)?;
    let image_size: u64 = images.iter().map(|image| image.size).sum();
    let image_active = images.iter().filter(|image| image.containers > 0).count() as u64;
    let image_reclaimable: u64 = images
        .iter()
        .filter(|image| image.containers == 0)
        .map(|image| image.size)
        .sum();

    let containers = list_containers(docker)?;
    let container_active = containers
        .iter()
        .filter(|container| matches!(container.state.as_str(), "running" | "paused"))
        .count() as u64;

    let volumes = call(docker.list_volumes(None::<query::ListVolumesOptions>))?
        .volumes
        .unwrap_or_default();
    let volume_size: u64 = volumes
        .iter()
        .filter_map(|volume| volume.usage_data.as_ref())
        .map(|usage| usage.size.max(0) as u64)
        .sum();
    let volume_reclaimable: u64 = volumes
        .iter()
        .filter(|volume| {
            volume
                .usage_data
                .as_ref()
                .is_some_and(|usage| usage.ref_count == 0)
        })
        .filter_map(|volume| volume.usage_data.as_ref())
        .map(|usage| usage.size.max(0) as u64)
        .sum();
    let volume_active = volumes
        .iter()
        .filter(|volume| {
            volume
                .usage_data
                .as_ref()
                .is_some_and(|usage| usage.ref_count > 0)
        })
        .count() as u64;

    Ok(vec![
        DiskUsage {
            kind: "Images".to_string(),
            total: images.len() as u64,
            active: image_active,
            size: image_size,
            reclaimable: image_reclaimable,
        },
        DiskUsage {
            kind: "Containers".to_string(),
            total: containers.len() as u64,
            active: container_active,
            size: 0,
            reclaimable: 0,
        },
        DiskUsage {
            kind: "Volumes".to_string(),
            total: volumes.len() as u64,
            active: volume_active,
            size: volume_size,
            reclaimable: volume_reclaimable,
        },
    ])
}

fn usage_row(
    kind: &str,
    active: Option<i64>,
    total: Option<i64>,
    size: Option<i64>,
    reclaimable: Option<i64>,
) -> DiskUsage {
    DiskUsage {
        kind: kind.to_string(),
        total: non_negative(total),
        active: non_negative(active),
        size: non_negative(size),
        reclaimable: non_negative(reclaimable),
    }
}

fn non_negative(value: Option<i64>) -> u64 {
    value.unwrap_or_default().max(0) as u64
}

// ---------------------------------------------------------------------------
// Containers
// ---------------------------------------------------------------------------

pub(super) fn container_action(id: &str, action: ContainerAction) -> Result<String, String> {
    let docker = client()?;
    match action {
        ContainerAction::Start => call(docker.start_container(id, None))?,
        ContainerAction::Stop => call(docker.stop_container(id, None))?,
        ContainerAction::Restart => call(docker.restart_container(id, None))?,
        ContainerAction::Pause => call(docker.pause_container(id))?,
        ContainerAction::Unpause => call(docker.unpause_container(id))?,
    }
    Ok(String::new())
}

pub(super) fn remove_container(id: &str) -> Result<String, String> {
    let docker = client()?;
    let options = query::RemoveContainerOptionsBuilder::default()
        .force(true)
        .build();
    call(docker.remove_container(id, Some(options)))?;
    Ok(String::new())
}

pub(super) fn rename_container(id: &str, name: &str) -> Result<String, String> {
    let docker = client()?;
    let options = query::RenameContainerOptions {
        name: name.to_string(),
    };
    call(docker.rename_container(id, options))?;
    Ok(String::new())
}

pub(super) fn stats(id: &str) -> Result<Option<StatsSample>, String> {
    let docker = client()?;
    // Without `one_shot`, the daemon waits for a second sample and fills
    // `precpu_stats`, so the CPU figure is current usage rather than the
    // average since the container started.
    let options = query::StatsOptionsBuilder::default()
        .stream(false)
        .one_shot(false)
        .build();
    let mut stream = docker.stats(id, Some(options));
    let sample = runtime().block_on(async { stream.next().await });
    match sample {
        None => Ok(None),
        // A stopped container has no live statistics; Podman's `stats` returns
        // a zeroed sample there, so do the same instead of surfacing a 409.
        Some(Err(bollard::errors::Error::DockerResponseServerError {
            status_code: 409, ..
        })) => Ok(Some(empty_stats_sample())),
        Some(Err(error)) => Err(format!("Docker engine error: {error}")),
        Some(Ok(sample)) => Ok(Some(stats_sample(sample))),
    }
}

fn empty_stats_sample() -> StatsSample {
    StatsSample {
        cpu_percent: 0.0,
        mem_usage: 0,
        mem_limit: 0,
        mem_percent: 0.0,
        net_in: 0,
        net_out: 0,
        block_in: 0,
        block_out: 0,
        pids: 0,
        at: Instant::now(),
    }
}

fn stats_sample(sample: model::ContainerStatsResponse) -> StatsSample {
    let cpu_stats = sample.cpu_stats.unwrap_or_default();
    let pre_cpu = sample.precpu_stats.unwrap_or_default();
    let cpu_usage = cpu_stats.cpu_usage.clone().unwrap_or_default();
    let pre_cpu_usage = pre_cpu.cpu_usage.clone().unwrap_or_default();
    let cpu_delta = cpu_usage
        .total_usage
        .unwrap_or_default()
        .saturating_sub(pre_cpu_usage.total_usage.unwrap_or_default());
    let system_delta = cpu_stats
        .system_cpu_usage
        .unwrap_or_default()
        .saturating_sub(pre_cpu.system_cpu_usage.unwrap_or_default());
    let online_cpus = cpu_stats.online_cpus.unwrap_or(1).max(1) as f64;
    let cpu_percent = if system_delta > 0 && cpu_delta > 0 {
        cpu_delta as f64 / system_delta as f64 * online_cpus * 100.0
    } else {
        0.0
    };

    let memory = sample.memory_stats.unwrap_or_default();
    let mem_usage = memory.usage.unwrap_or_default();
    let mem_limit = memory.limit.unwrap_or_default();
    let mem_percent = if mem_limit > 0 {
        mem_usage as f64 / mem_limit as f64 * 100.0
    } else {
        0.0
    };

    let mut net_in = 0;
    let mut net_out = 0;
    for interface in sample.networks.unwrap_or_default().into_values() {
        net_in += interface.rx_bytes.unwrap_or_default();
        net_out += interface.tx_bytes.unwrap_or_default();
    }

    let mut block_in = 0;
    let mut block_out = 0;
    for entry in sample
        .blkio_stats
        .and_then(|stats| stats.io_service_bytes_recursive)
        .unwrap_or_default()
    {
        match entry.op.as_deref() {
            Some(op) if op.eq_ignore_ascii_case("read") => {
                block_in += entry.value.unwrap_or_default();
            }
            Some(op) if op.eq_ignore_ascii_case("write") => {
                block_out += entry.value.unwrap_or_default();
            }
            _ => {}
        }
    }

    StatsSample {
        cpu_percent,
        mem_usage,
        mem_limit,
        mem_percent,
        net_in,
        net_out,
        block_in,
        block_out,
        pids: sample
            .pids_stats
            .and_then(|stats| stats.current)
            .unwrap_or_default(),
        at: Instant::now(),
    }
}

pub(super) fn inspect_container(id: &str) -> Result<ContainerDetails, String> {
    let docker = client()?;
    let info = call(docker.inspect_container(id, None))?;
    let config = info.config.unwrap_or_default();
    let state = info.state.unwrap_or_default();
    let compose = super::compose::compose_membership(config.labels.as_ref());

    let mut command = config.entrypoint.unwrap_or_default();
    command.extend(config.cmd.unwrap_or_default());
    let image = config
        .image
        .or(info.image)
        .unwrap_or_else(|| "<none>".to_string());
    let created = info
        .created
        .map(|created| display_created(&created.to_string()))
        .unwrap_or_else(|| "-".to_string());

    Ok(ContainerDetails {
        id: info.id.unwrap_or_default(),
        name: info
            .name
            .unwrap_or_default()
            .trim_start_matches('/')
            .to_string(),
        image,
        command: command.join(" "),
        created,
        status: state
            .status
            .map(|status| status.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        pid: state.pid.unwrap_or_default(),
        exit_code: state.exit_code.unwrap_or_default(),
        env: config.env.unwrap_or_default(),
        mounts: info
            .mounts
            .unwrap_or_default()
            .into_iter()
            .map(|mount| MountDetails {
                kind: mount.typ.map(|kind| kind.to_string()).unwrap_or_default(),
                source: mount.source.unwrap_or_default(),
                destination: mount.destination.unwrap_or_default(),
            })
            .collect(),
        networks: info
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

// ---------------------------------------------------------------------------
// Images
// ---------------------------------------------------------------------------

pub(super) fn remove_image(id: &str) -> Result<String, String> {
    let docker = client()?;
    let options = query::RemoveImageOptionsBuilder::default()
        .force(true)
        .build();
    call(docker.remove_image(id, Some(options), None))?;
    Ok(String::new())
}

pub(super) fn image_history(image: &str) -> Result<Vec<ImageHistoryEntry>, String> {
    let docker = client()?;
    let history = call(docker.image_history(image))?;
    Ok(history
        .into_iter()
        .map(|row| ImageHistoryEntry {
            id: row.id,
            created: format_epoch(row.created),
            created_by: row.created_by.trim().to_string(),
            size: row.size.max(0) as u64,
            comment: row.comment,
        })
        .collect())
}

// ---------------------------------------------------------------------------
// Volumes and networks
// ---------------------------------------------------------------------------

pub(super) fn create_volume(name: &str, driver: Option<&str>) -> Result<String, String> {
    let docker = client()?;
    let config = model::VolumeCreateRequest {
        name: Some(name.to_string()),
        driver: driver.map(str::to_string),
        ..Default::default()
    };
    call(docker.create_volume(config))?;
    Ok(String::new())
}

pub(super) fn remove_volume(name: &str) -> Result<String, String> {
    let docker = client()?;
    let options = query::RemoveVolumeOptionsBuilder::default()
        .force(true)
        .build();
    call(docker.remove_volume(name, Some(options)))?;
    Ok(String::new())
}

pub(super) fn prune_volumes() -> Result<String, String> {
    let docker = client()?;
    let response = call(docker.prune_volumes(None::<query::PruneVolumesOptions>))?;
    let deleted = response.volumes_deleted.unwrap_or_default().len();
    Ok(format!(
        "Deleted {deleted} volume(s), reclaimed {}",
        human_size(non_negative(response.space_reclaimed))
    ))
}

pub(super) fn inspect_volume(name: &str) -> Result<VolumeDetails, String> {
    let docker = client()?;
    let volume = call(docker.inspect_volume(name))?;
    Ok(VolumeDetails {
        name: volume.name,
        driver: volume.driver,
        mountpoint: volume.mountpoint,
        created: volume
            .created_at
            .map(|created| display_created(&created.to_string()))
            .unwrap_or_else(|| "-".to_string()),
        scope: volume
            .scope
            .map(|scope| scope.to_string())
            .unwrap_or_else(|| "local".to_string()),
        anonymous: false,
        mounts: volume
            .usage_data
            .map(|usage| usage.ref_count.max(0) as u64)
            .unwrap_or_default(),
    })
}

pub(super) fn create_network(
    name: &str,
    driver: Option<&str>,
    subnet: Option<&str>,
    gateway: Option<&str>,
) -> Result<String, String> {
    let docker = client()?;
    let ipam = match (subnet, gateway) {
        (None, None) => None,
        (subnet, gateway) => Some(model::Ipam {
            driver: None,
            config: Some(vec![model::IpamConfig {
                subnet: subnet.map(str::to_string),
                gateway: gateway.map(str::to_string),
                ..Default::default()
            }]),
            options: None,
        }),
    };
    let config = model::NetworkCreateRequest {
        name: name.to_string(),
        driver: driver.map(str::to_string),
        ipam,
        ..Default::default()
    };
    call(docker.create_network(config))?;
    Ok(String::new())
}

pub(super) fn remove_network(name: &str) -> Result<String, String> {
    let docker = client()?;
    call(docker.remove_network(name))?;
    Ok(String::new())
}

pub(super) fn prune_networks() -> Result<String, String> {
    let docker = client()?;
    let response = call(docker.prune_networks(None::<query::PruneNetworksOptions>))?;
    let deleted = response.networks_deleted.unwrap_or_default().len();
    Ok(format!("Deleted {deleted} network(s)"))
}

pub(super) fn inspect_network(name: &str) -> Result<NetworkDetails, String> {
    let docker = client()?;
    let network = call(docker.inspect_network(name, None))?;
    let name = network.name.unwrap_or_else(|| name.to_string());
    let subnets = network
        .ipam
        .and_then(|ipam| ipam.config)
        .unwrap_or_default()
        .into_iter()
        .map(|config| NetworkSubnet {
            subnet: config.subnet.unwrap_or_default(),
            gateway: config.gateway.unwrap_or_default(),
        })
        .collect();
    let containers = network
        .containers
        .unwrap_or_default()
        .into_iter()
        .map(|(id, container)| {
            container
                .name
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| short_id(&id))
        })
        .collect();

    Ok(NetworkDetails {
        id: network.id.unwrap_or_default(),
        name,
        driver: network.driver.unwrap_or_default(),
        interface: String::new(),
        created: network
            .created
            .map(|created| display_created(&created.to_string()))
            .unwrap_or_else(|| "-".to_string()),
        subnets,
        internal: network.internal.unwrap_or_default(),
        ipv6: network.enable_ipv6.unwrap_or_default(),
        dns: true,
        containers,
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Format a Unix timestamp (seconds, UTC) the way the Podman CLI output is
/// shortened for the tables.
fn format_epoch(seconds: i64) -> String {
    if seconds <= 0 {
        return "-".to_string();
    }
    let days = seconds.div_euclid(86_400);
    let seconds_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}")
}

/// Howard Hinnant's `civil_from_days`, with 1970-01-01 as day 0.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    if month <= 2 {
        year += 1;
    }
    (year, month as u32, day as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_sockets_and_pipes_are_local() {
        assert!(!is_remote_host("unix:///var/run/docker.sock"));
        assert!(!is_remote_host("npipe:////./pipe/docker_engine"));
        assert!(is_remote_host("tcp://10.0.0.5:2376"));
        assert!(is_remote_host("ssh://me@server"));
    }

    #[test]
    fn ssh_hosts_get_a_clear_error() {
        let error = connect("ssh://me@server").unwrap_err();
        assert!(error.contains("SSH"), "{error}");
    }

    #[test]
    fn formats_unix_timestamps() {
        assert_eq!(format_epoch(0), "-");
        assert_eq!(format_epoch(1_700_000_000), "2023-11-14 22:13");
        assert_eq!(format_epoch(951_782_400), "2000-02-29 00:00");
    }

    #[test]
    fn formats_docker_ports() {
        let ports = vec![
            model::PortSummary {
                ip: Some("0.0.0.0".to_string()),
                private_port: 80,
                public_port: Some(8080),
                typ: Some(model::PortSummaryTypeEnum::TCP),
            },
            model::PortSummary {
                ip: None,
                private_port: 443,
                public_port: None,
                typ: None,
            },
        ];
        assert_eq!(docker_ports_label(&ports), "8080->80/tcp, 443/tcp");
    }
}
