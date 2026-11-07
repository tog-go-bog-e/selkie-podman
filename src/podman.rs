//! Minimal client for the local Podman CLI.
//!
//! The app shells out to `podman` instead of talking to the libpod REST API so
//! that it works with every Podman version that supports `--format json`.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, OnceLock, RwLock};
use std::time::{Duration, Instant};

use serde::Deserialize;

/// Everything the UI needs to render one frame.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub version: String,
    pub images: Vec<Image>,
    pub containers: Vec<Container>,
    pub pods: Vec<Pod>,
    pub volumes: Vec<Volume>,
    pub networks: Vec<Network>,
    pub compose: Vec<ComposeProject>,
}

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

#[derive(Debug, Clone, Default)]
pub struct Image {
    pub id: String,
    pub name: String,
    pub digest: String,
    pub size: u64,
    pub containers: u64,
    pub created: String,
}

impl Image {
    pub fn id_short(&self) -> String {
        short_id(&self.id)
    }

    /// `sha256:abc…` shortened to `abc…`, or an empty string when unknown.
    pub fn digest_short(&self) -> String {
        let digest = self.digest.strip_prefix("sha256:").unwrap_or(&self.digest);
        if digest.is_empty() {
            String::new()
        } else {
            format!("digest {}", short_id(digest))
        }
    }

    pub fn size_label(&self) -> String {
        human_size(self.size)
    }
}

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

/// Compose metadata attached to a container by `docker compose` or
/// `podman-compose` (both label prefixes are understood).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposeMembership {
    pub project: String,
    pub service: Option<String>,
    /// Comma-separated paths, as recorded by compose.
    pub config_files: Option<String>,
    pub working_dir: Option<String>,
}

/// One service (a group of replicas) inside a compose project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposeService {
    pub name: String,
    pub containers: usize,
    pub running: usize,
}

/// A compose project, derived from the labels of its containers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposeProject {
    pub name: String,
    pub config_files: Vec<String>,
    pub working_dir: Option<String>,
    pub services: Vec<ComposeService>,
    pub running: usize,
    pub total: usize,
}

impl ComposeProject {
    pub fn status_label(&self) -> String {
        format!("{}/{} running", self.running, self.total)
    }

    /// File name of the first compose file, for compact table cells.
    pub fn config_label(&self) -> String {
        self.config_files
            .first()
            .map(|file| {
                std::path::Path::new(file)
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| file.clone())
            })
            .unwrap_or_else(|| self.working_dir.clone().unwrap_or_else(|| "-".into()))
    }
}

/// A project-level compose action, executed by shelling out to `podman compose`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComposeAction {
    Up,
    Down,
    Restart,
    Build,
}

impl ComposeAction {
    pub fn verb(self) -> &'static str {
        match self {
            ComposeAction::Up => "up",
            ComposeAction::Down => "down",
            ComposeAction::Restart => "restart",
            ComposeAction::Build => "build",
        }
    }

    pub fn done(self) -> &'static str {
        match self {
            ComposeAction::Up => "started",
            ComposeAction::Down => "stopped and removed",
            ComposeAction::Restart => "restarted",
            ComposeAction::Build => "built",
        }
    }
}

impl Container {
    pub fn id_short(&self) -> String {
        short_id(&self.id)
    }
}

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

/// Fetch the engine version, all images and all containers.
pub fn snapshot() -> Result<Snapshot, String> {
    let version = query(&["--version"])?.trim().to_string();
    let images_raw = query(&["images", "--format", "json"])?;
    let containers_raw = query(&["ps", "-a", "--format", "json"])?;
    let pods_raw = query(&["pod", "ls", "--format", "json"])?;
    let volumes_raw = query(&["volume", "ls", "--format", "json"])?;
    let networks_raw = query(&["network", "ls", "--format", "json"])?;

    let containers = parse_containers(&containers_raw)?;
    let compose = compose_projects(&containers);

    Ok(Snapshot {
        version,
        images: parse_images(&images_raw)?,
        containers,
        pods: parse_pods(&pods_raw)?,
        volumes: parse_volumes(&volumes_raw)?,
        networks: parse_networks(&networks_raw)?,
        compose,
    })
}

/// Upper bound for read-only queries (lists, inspect, info, stats).
///
/// An unreachable remote connection can leave `podman` waiting indefinitely;
/// without a bound the refresh never completes and the UI stays "loading".
/// Actions (start, pull, machine init, ...) are not bounded: they can
/// legitimately take minutes.
pub const QUERY_TIMEOUT: Duration = Duration::from_secs(45);

/// An action against the selected engine. Not time-bounded.
fn run(args: &[&str]) -> Result<String, String> {
    run_command(base_command(), args, None)
}

/// An action against the local Podman installation. Not time-bounded.
fn run_local(args: &[&str]) -> Result<String, String> {
    run_command(Command::new("podman"), args, None)
}

/// A read-only query against the selected engine, bounded by [`QUERY_TIMEOUT`].
fn query(args: &[&str]) -> Result<String, String> {
    run_command(base_command(), args, Some(QUERY_TIMEOUT))
}

/// A read-only local query, bounded by [`QUERY_TIMEOUT`].
fn query_local(args: &[&str]) -> Result<String, String> {
    run_command(Command::new("podman"), args, Some(QUERY_TIMEOUT))
}

fn run_command(
    mut command: Command,
    args: &[&str],
    timeout: Option<Duration>,
) -> Result<String, String> {
    command.args(args);
    execute_with(command, &args.join(" "), timeout)
}

/// Run a fully built command. `display` is only used in the error message.
fn execute(command: Command, display: &str) -> Result<String, String> {
    execute_with(command, display, None)
}

fn launch_error(err: std::io::Error) -> String {
    if err.kind() == std::io::ErrorKind::NotFound {
        "podman was not found in PATH".to_string()
    } else {
        format!("failed to launch podman: {err}")
    }
}

/// Like [`Command::output`], but kills the child when `timeout` elapses.
fn output_with_timeout(
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

fn execute_with(
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
                    "`podman {display}` did not answer within {} s; is the engine reachable?",
                    timeout.as_secs()
                )
            })?,
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            format!("`podman {display}` exited with {}", output.status)
        } else {
            stderr
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub fn parse_images(json: &str) -> Result<Vec<Image>, String> {
    let raw: Vec<RawImage> = parse_json(json, "podman images")?;
    // `podman images --format json` emits one entry per repo tag for images
    // with several names, so deduplicate by image ID.
    let mut seen = HashSet::new();
    Ok(raw
        .into_iter()
        .filter(|raw| !raw.id.is_empty() && seen.insert(raw.id.clone()))
        .map(|raw| {
            let name = first_non_empty(raw.names.as_deref())
                .or_else(|| first_non_empty(raw.repo_tags.as_deref()))
                .unwrap_or_else(|| "<none>".to_string());
            Image {
                id: raw.id,
                name,
                digest: raw.digest.unwrap_or_default(),
                size: raw.size.unwrap_or_default(),
                containers: raw.containers.unwrap_or_default(),
                created: format_timestamp(raw.created_at.as_deref()),
            }
        })
        .collect())
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

/// Group containers carrying compose labels into projects.
///
/// Projects are sorted by name; services within a project by name as well.
pub fn compose_projects(containers: &[Container]) -> Vec<ComposeProject> {
    #[derive(Default)]
    struct Builder {
        config_files: Vec<String>,
        working_dir: Option<String>,
        services: BTreeMap<String, (usize, usize)>,
        running: usize,
        total: usize,
    }

    let mut projects: BTreeMap<String, Builder> = BTreeMap::new();
    for container in containers {
        let Some(membership) = &container.compose else {
            continue;
        };
        let builder = projects.entry(membership.project.clone()).or_default();

        if let Some(files) = &membership.config_files {
            for file in files.split(',') {
                let file = file.trim();
                if !file.is_empty() && !builder.config_files.iter().any(|known| known == file) {
                    builder.config_files.push(file.to_string());
                }
            }
        }
        if builder.working_dir.is_none() {
            builder.working_dir = membership.working_dir.clone();
        }

        let service = membership
            .service
            .clone()
            .unwrap_or_else(|| container.name.clone());
        let counts = builder.services.entry(service).or_default();
        counts.0 += 1;
        builder.total += 1;
        if container.state == "running" {
            counts.1 += 1;
            builder.running += 1;
        }
    }

    projects
        .into_iter()
        .map(|(name, builder)| ComposeProject {
            name,
            config_files: builder.config_files,
            working_dir: builder.working_dir,
            services: builder
                .services
                .into_iter()
                .map(|(name, (containers, running))| ComposeService {
                    name,
                    containers,
                    running,
                })
                .collect(),
            running: builder.running,
            total: builder.total,
        })
        .collect()
}

/// Build the `podman compose` command for a project action.
///
/// A recorded compose file wins; otherwise the working directory is used so the
/// provider can discover `compose.yaml` itself. The project name is always
/// passed explicitly so actions target the project the labels describe.
pub fn compose_command(project: &ComposeProject, action: ComposeAction) -> Result<Command, String> {
    let mut command = compose_base(project)?;
    match action {
        ComposeAction::Up => {
            command.arg("up").arg("-d");
        }
        ComposeAction::Down => {
            command.arg("down");
        }
        ComposeAction::Restart => {
            command.arg("restart");
        }
        ComposeAction::Build => {
            command.arg("build");
        }
    }
    Ok(command)
}

/// `podman compose logs -f` for a whole project or one service.
///
/// `--tail` is used instead of `--since` because both the Docker Compose v2
/// provider and `podman-compose` accept it, while `--since` support varies.
pub fn compose_logs_command(
    project: &ComposeProject,
    service: Option<&str>,
    tail: usize,
) -> Result<Command, String> {
    let mut command = compose_base(project)?;
    command.args(["logs", "-f", "--tail", &tail.to_string()]);
    if let Some(service) = service {
        command.arg(validate_name(service, "service")?);
    }
    Ok(command)
}

/// `podman compose [-f file] -p <project>` with the recorded working directory.
fn compose_base(project: &ComposeProject) -> Result<Command, String> {
    let mut command = base_command();
    // Podman prints a banner about the external compose provider on every call;
    // in the log view and output panel it is just noise.
    command.env("PODMAN_COMPOSE_WARNING_LOGS", "false");
    command.arg("compose");

    if let Some(file) = project.config_files.first() {
        command.arg("-f").arg(file);
    } else if project.working_dir.is_none() {
        return Err(format!(
            "project {} has no recorded compose file or working directory",
            project.name
        ));
    }

    command.arg("-p").arg(&project.name);

    if let Some(dir) = &project.working_dir {
        if std::path::Path::new(dir).is_dir() {
            command.current_dir(dir);
        }
    }

    Ok(command)
}

/// `podman logs -f --since <window> --timestamps` for a container.
pub fn container_logs_command(id: &str, window: &str) -> Command {
    let mut command = base_command();
    command.args(["logs", "-f", "--since", window, "--timestamps", id]);
    command
}

fn compose_membership(labels: Option<&HashMap<String, String>>) -> Option<ComposeMembership> {
    let labels = labels?;
    let project = compose_label(labels, "project")?;
    Some(ComposeMembership {
        project,
        service: compose_label(labels, "service"),
        config_files: compose_label(labels, "project.config_files"),
        working_dir: compose_label(labels, "project.working_dir"),
    })
}

/// `docker compose` uses `com.docker.compose.*`, `podman-compose` uses
/// `io.podman.compose.*`.
fn compose_label(labels: &HashMap<String, String>, suffix: &str) -> Option<String> {
    ["com.docker.compose.", "io.podman.compose."]
        .iter()
        .find_map(|prefix| {
            labels
                .get(&format!("{prefix}{suffix}"))
                .filter(|value| !value.is_empty())
                .cloned()
        })
}

pub fn container_action(id: &str, action: ContainerAction) -> Result<String, String> {
    run(&[action.verb(), id])
}

pub fn remove_container(id: &str) -> Result<String, String> {
    run(&["rm", "-f", id])
}

pub fn rename_container(id: &str, name: &str) -> Result<String, String> {
    let name = validate_container_name(name)?;
    run(&["rename", id, name])
}

/// Names must be a valid Podman container name: no whitespace, no leading `-`
/// (which would be parsed as a flag), and only characters Podman accepts.
pub fn validate_container_name(value: &str) -> Result<&str, String> {
    validate_name(value, "container")
}

pub fn remove_image(id: &str) -> Result<String, String> {
    run(&["rmi", "-f", id])
}

// ---------------------------------------------------------------------------
// Image history and file browser
// ---------------------------------------------------------------------------

/// One layer in `podman history`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageHistoryEntry {
    pub id: String,
    pub created: String,
    pub created_by: String,
    pub size: u64,
    pub comment: String,
}

impl ImageHistoryEntry {
    /// The base layer has no image ID (`<missing>` in the CLI output).
    pub fn id_label(&self) -> String {
        if self.id.is_empty() || self.id == "<missing>" {
            "-".to_string()
        } else {
            short_id(&self.id)
        }
    }

    pub fn size_label(&self) -> String {
        if self.size == 0 {
            "-".to_string()
        } else {
            human_size(self.size)
        }
    }
}

pub fn image_history(image: &str) -> Result<Vec<ImageHistoryEntry>, String> {
    let json = query(&["history", "--format", "json", image])?;
    parse_image_history(&json)
}

fn parse_image_history(json: &str) -> Result<Vec<ImageHistoryEntry>, String> {
    let raw: Vec<RawHistory> = parse_json(json, "podman history")?;
    Ok(raw
        .into_iter()
        .map(|row| ImageHistoryEntry {
            id: row.id,
            created: format_timestamp(Some(row.created.as_str())),
            created_by: row.created_by.trim().to_string(),
            size: row.size,
            comment: row.comment,
        })
        .collect())
}

#[derive(Deserialize)]
struct RawHistory {
    #[serde(default)]
    id: String,
    #[serde(default)]
    created: String,
    #[serde(default, rename = "CreatedBy")]
    created_by: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    comment: String,
}

/// How much of a file the browser reads before treating it as binary.
pub const IMAGE_FILE_LIMIT: usize = 128 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFileKind {
    File,
    Directory,
    Link,
    Other,
}

/// One path inside an image's filesystem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageFile {
    pub path: String,
    pub kind: ImageFileKind,
    pub size: u64,
    /// Symlink target, when `kind` is [`ImageFileKind::Link`].
    pub target: Option<String>,
}

impl ImageFile {
    pub fn name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }

    /// Empty for entries directly below the root.
    pub fn parent(&self) -> &str {
        self.path.rsplit_once('/').map_or("", |(parent, _)| parent)
    }
}

/// List an image's filesystem.
///
/// `podman image mount` needs root or `podman unshare`, so the browser creates a
/// throwaway container and streams `podman export` through `tar`. Nothing is
/// written to disk, and the container is removed when the listing finishes.
pub fn image_files(image: &str) -> Result<Vec<ImageFile>, String> {
    let output = export_tar(image, &["-tvf", "-"], None)?;
    Ok(parse_tar_listing(&String::from_utf8_lossy(&output)))
}

/// Read one file out of the image. The returned bytes stop at `limit`.
pub fn image_file_content(image: &str, path: &str, limit: usize) -> Result<Vec<u8>, String> {
    let path = validate_archive_path(path)?;
    export_tar(image, &["-xOf", "-", "--", path], Some(limit))
}

fn validate_archive_path(path: &str) -> Result<&str, String> {
    let path = path.trim();
    if path.is_empty() {
        return Err("path cannot be empty".to_string());
    }
    if path.starts_with('-') {
        return Err("path cannot start with '-'".to_string());
    }
    if path.split('/').any(|part| part == "..") {
        return Err("path cannot contain '..'".to_string());
    }
    Ok(path)
}

/// Read at most `limit` bytes from `reader`.
///
/// Returns the bytes and whether the limit was hit before EOF, in which case
/// the caller stops the producer instead of buffering the rest.
fn read_bounded(reader: &mut impl Read, limit: usize) -> std::io::Result<(Vec<u8>, bool)> {
    let mut bytes = Vec::new();
    // One extra byte tells "exactly `limit`" apart from "more than `limit`".
    reader.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    let truncated = bytes.len() > limit;
    bytes.truncate(limit);
    Ok((bytes, truncated))
}

/// Pipe `podman export <temp container>` into `tar` with `tar_args`.
///
/// With `limit`, tar's output is read only up to that many bytes; once it is
/// reached both processes are stopped, so previewing a large file neither
/// buffers it in memory nor streams the rest of the image.
fn export_tar(image: &str, tar_args: &[&str], limit: Option<usize>) -> Result<Vec<u8>, String> {
    let container = TempContainer::create(image)?;

    let mut export = base_command();
    export
        .args(["export", &container.name])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut export_child = export
        .spawn()
        .map_err(|error| format!("could not run podman export: {error}"))?;
    let stdout = export_child
        .stdout
        .take()
        .ok_or_else(|| "podman export produced no output".to_string())?;
    // Read stderr concurrently so a noisy export cannot block on a full pipe.
    let export_stderr = export_child.stderr.take().map(|mut pipe| {
        std::thread::spawn(move || {
            let mut text = String::new();
            let _ = pipe.read_to_string(&mut text);
            text
        })
    });

    let mut tar = Command::new("tar");
    tar.args(tar_args)
        .env("LC_ALL", "C")
        .stdin(Stdio::from(stdout))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut tar_child = match tar.spawn() {
        Ok(child) => child,
        Err(error) => {
            // Do not leave `podman export` blocked on a closed pipe.
            let _ = export_child.kill();
            let _ = export_child.wait();
            return Err(if error.kind() == std::io::ErrorKind::NotFound {
                "`tar` was not found in PATH (needed for the image file browser)".to_string()
            } else {
                format!("could not run tar: {error}")
            });
        }
    };
    let tar_stderr = tar_child.stderr.take().map(|mut pipe| {
        std::thread::spawn(move || {
            let mut text = String::new();
            let _ = pipe.read_to_string(&mut text);
            text
        })
    });

    let mut tar_stdout = tar_child
        .stdout
        .take()
        .ok_or_else(|| "tar produced no output".to_string())?;
    let read = match limit {
        Some(limit) => read_bounded(&mut tar_stdout, limit),
        None => {
            let mut bytes = Vec::new();
            tar_stdout.read_to_end(&mut bytes).map(|_| (bytes, false))
        }
    };
    drop(tar_stdout);
    let (bytes, truncated) = match read {
        Ok(result) => result,
        Err(error) => {
            let _ = tar_child.kill();
            let _ = export_child.kill();
            let _ = tar_child.wait();
            let _ = export_child.wait();
            return Err(format!("could not read tar output: {error}"));
        }
    };

    if truncated {
        // We have all we will show; the exit statuses of the stopped
        // processes say nothing about the data.
        let _ = tar_child.kill();
        let _ = export_child.kill();
        let _ = tar_child.wait();
        let _ = export_child.wait();
        return Ok(bytes);
    }

    let tar_status = tar_child
        .wait()
        .map_err(|error| format!("tar failed: {error}"))?;
    if !tar_status.success() {
        // tar stopped early (bad arguments, unreadable archive): kill the writer
        // instead of waiting for it forever.
        let _ = export_child.kill();
    }

    let export_status = export_child
        .wait()
        .map_err(|error| format!("podman export failed: {error}"))?;
    if !export_status.success() && tar_status.success() {
        let stderr = export_stderr
            .and_then(|handle| handle.join().ok())
            .unwrap_or_default();
        let stderr = stderr.trim();
        return Err(if stderr.is_empty() {
            format!("`podman export {image}` exited with {export_status}")
        } else {
            stderr.to_string()
        });
    }

    if !tar_status.success() {
        let stderr = tar_stderr
            .and_then(|handle| handle.join().ok())
            .unwrap_or_default();
        let stderr = stderr.trim();
        return Err(if stderr.is_empty() {
            format!("`tar {}` exited with {}", tar_args.join(" "), tar_status)
        } else {
            stderr.to_string()
        });
    }

    Ok(bytes)
}

/// A container that only exists so `podman export` has something to read.
struct TempContainer {
    name: String,
}

impl TempContainer {
    fn create(image: &str) -> Result<Self, String> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let name = format!(
            "pd-browse-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );

        let mut command = base_command();
        // The command is never run; it only satisfies `podman create`.
        command.args(["create", "--name", &name, image, "/bin/true"]);
        let output = command
            .output()
            .map_err(|error| format!("could not run podman create: {error}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(if stderr.is_empty() {
                format!("`podman create {image}` exited with {}", output.status)
            } else {
                stderr
            });
        }
        Ok(Self { name })
    }
}

impl Drop for TempContainer {
    fn drop(&mut self) {
        let mut command = base_command();
        command.args(["rm", "-f", &self.name]);
        let _ = command.output();
    }
}

/// Parse `tar -tv` output into entries.
///
/// GNU tar's verbose format is `mode owner size date time name`, with ` -> ` for
/// symlinks. Names may contain spaces, so only the first five whitespace-separated
/// fields are split off and the remainder is the name.
pub fn parse_tar_listing(text: &str) -> Vec<ImageFile> {
    let mut files = Vec::new();
    let mut directories: HashSet<String> = HashSet::new();

    for line in text.lines().take(200_000) {
        let Some([mode, _owner, size, _date, _time, rest]) = tar_line_fields(line) else {
            continue;
        };
        let kind = match mode.chars().next().unwrap_or('-') {
            'd' => ImageFileKind::Directory,
            'l' | 'h' => ImageFileKind::Link,
            '-' => ImageFileKind::File,
            _ => ImageFileKind::Other,
        };
        let (name, target) = match rest.split_once(" -> ") {
            Some((name, target)) => (name, Some(target.trim().to_string())),
            None => (rest, None),
        };
        let cleaned = name.trim_start_matches("./").trim_end_matches('/');
        if cleaned.is_empty() || cleaned == "." {
            continue;
        }
        let kind = if name.ends_with('/') {
            ImageFileKind::Directory
        } else {
            kind
        };
        if kind == ImageFileKind::Directory {
            directories.insert(cleaned.to_string());
        }
        files.push(ImageFile {
            path: cleaned.to_string(),
            kind,
            size: size.parse().unwrap_or(0),
            target,
        });
    }

    // Make sure every path is reachable even when the archive omits a directory
    // entry for an implied parent.
    let mut synthesized = Vec::new();
    for file in &files {
        let mut parent = file.parent();
        while !parent.is_empty() {
            if directories.insert(parent.to_string()) {
                synthesized.push(parent.to_string());
            }
            parent = parent.rsplit_once('/').map_or("", |(rest, _)| rest);
        }
    }
    files.extend(synthesized.into_iter().map(|path| ImageFile {
        path,
        kind: ImageFileKind::Directory,
        size: 0,
        target: None,
    }));

    files
}

fn tar_line_fields(line: &str) -> Option<[&str; 6]> {
    let mut rest = line;
    let mut fields = [""; 6];
    for slot in fields.iter_mut().take(5) {
        rest = rest.trim_start();
        if rest.is_empty() {
            return None;
        }
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        *slot = &rest[..end];
        rest = &rest[end..];
    }
    fields[5] = rest.trim_start();
    if fields[5].is_empty() {
        return None;
    }
    Some(fields)
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

fn validate_name<'a>(value: &'a str, what: &str) -> Result<&'a str, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(format!("{what} name cannot be empty"));
    }
    if value.starts_with('-') {
        return Err(format!("{what} name cannot start with '-'"));
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return Err(format!(
            "{what} names may only contain letters, digits, '-', '_' and '.'"
        ));
    }
    Ok(value)
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

const MACHINE_CPUS: (u64, u64) = (1, 128);
/// MiB.
const MACHINE_MEMORY: (u64, u64) = (512, 262_144);
/// GiB.
const MACHINE_DISK: (u64, u64) = (1, 4096);

fn parse_machine_number(
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

pub fn validate_machine_name(value: &str) -> Result<&str, String> {
    validate_name(value, "machine")
}

/// Sample resource usage for one container.
///
/// Returns `None` when Podman reports no statistics for the container. A
/// stopped container yields a zeroed sample rather than an error.
pub fn stats(id: &str) -> Result<Option<StatsSample>, String> {
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

fn parse_machines(json: &str) -> Result<Vec<Machine>, String> {
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

fn parse_connections(json: &str) -> Result<Vec<Connection>, String> {
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

/// Host and store details from `podman info`, for the Dashboard.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SystemInfo {
    pub version: String,
    pub os_arch: String,
    pub hostname: String,
    pub kernel: String,
    pub cpus: u64,
    pub memory_total: u64,
    pub memory_free: u64,
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
        format!(
            "{} free of {} ({}% used)",
            human_size(self.memory_free),
            human_size(self.memory_total),
            self.memory_used_percent()
        )
    }

    /// Share of host memory in use, 0-100.
    pub fn memory_used_percent(&self) -> u64 {
        if self.memory_total == 0 {
            return 0;
        }
        self.memory_total
            .saturating_sub(self.memory_free)
            .saturating_mul(100)
            / self.memory_total
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
    Ok(Dashboard {
        info: system_info()?,
        disk: disk_usage()?,
    })
}

fn system_info() -> Result<SystemInfo, String> {
    let json = query(&["info", "--format", "json"])?;
    parse_system_info(&json)
}

fn parse_system_info(json: &str) -> Result<SystemInfo, String> {
    let raw: RawInfo = serde_json::from_str(json.trim())
        .map_err(|err| format!("could not parse `podman info` output: {err}"))?;
    Ok(raw.into_system_info())
}

fn disk_usage() -> Result<Vec<DiskUsage>, String> {
    let json = query(&["system", "df", "--format", "json"])?;
    parse_disk_usage(&json)
}

fn parse_disk_usage(json: &str) -> Result<Vec<DiskUsage>, String> {
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
struct RawInfo {
    #[serde(default)]
    host: RawInfoHost,
    #[serde(default)]
    store: RawInfoStore,
    #[serde(default)]
    version: RawInfoVersion,
}

#[derive(Deserialize, Default)]
struct RawInfoHost {
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
struct RawInfoSecurity {
    #[serde(default)]
    rootless: bool,
}

#[derive(Deserialize, Default)]
struct RawInfoRuntime {
    #[serde(default)]
    name: String,
    #[serde(default)]
    version: String,
}

#[derive(Deserialize, Default)]
struct RawInfoSocket {
    #[serde(default)]
    path: String,
    #[serde(default)]
    exists: bool,
}

#[derive(Deserialize, Default)]
struct RawInfoStore {
    #[serde(default, rename = "graphDriverName")]
    storage_driver: String,
    #[serde(default, rename = "graphRoot")]
    graph_root: String,
}

#[derive(Deserialize, Default)]
struct RawInfoVersion {
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
            memory_free: self.host.memory_free,
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
fn short_runtime(name: &str, version: &str) -> String {
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
struct RawDiskUsage {
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

/// A `podman` command that targets the selected connection, if any.
pub fn base_command() -> Command {
    let mut command = Command::new("podman");
    apply_connection(&mut command, connection().as_deref());
    command
}

fn apply_connection(command: &mut Command, name: Option<&str>) {
    if let Some(name) = name {
        command.arg("--connection").arg(name);
    }
}

fn connection_slot() -> &'static RwLock<Option<String>> {
    static CONNECTION: OnceLock<RwLock<Option<String>>> = OnceLock::new();
    CONNECTION.get_or_init(|| RwLock::new(None))
}

pub fn remove_network(name: &str) -> Result<String, String> {
    run(&["network", "rm", "-f", name])
}

pub fn pull_command(reference: &str) -> Result<Command, String> {
    let reference = validate_image_reference(reference)?;
    let mut command = base_command();
    command.args(["pull", reference]);
    Ok(command)
}

pub fn tag_image(id: &str, target: &str) -> Result<String, String> {
    let target = validate_image_reference(target)?;
    run(&["tag", id, target])
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
    command.arg(image);
    command.args(&options.command);
    Ok(command)
}

/// Run a container and return Podman's stdout (the container ID).
pub fn run_container(options: &RunOptions) -> Result<String, String> {
    let command = container_run_command(options)?;
    execute(command, &format!("run {}", options.image))
}

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

fn parse_accounts(contents: &str) -> Vec<Account> {
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
fn base64_decode(input: &str) -> Option<Vec<u8>> {
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
fn is_loopback_registry(reference: &str) -> bool {
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

fn parse_tokens(
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
#[derive(Debug, Clone, Default)]
pub struct BuildOptions {
    pub containerfile: String,
    pub tag: String,
    /// Empty/None uses the Containerfile's directory.
    pub context: Option<String>,
    /// Empty/None uses the engine default.
    pub platform: Option<String>,
    /// Already parsed `KEY=VALUE` (or `KEY`) tokens.
    pub args: Vec<String>,
}

/// Build an image from a Containerfile, using its directory as the default
/// build context.
pub fn build_command(options: &BuildOptions) -> Result<Command, String> {
    let containerfile = options.containerfile.trim();
    if containerfile.is_empty() {
        return Err("Containerfile path cannot be empty".to_string());
    }
    let tag = validate_image_reference(&options.tag)?;

    let context = match options
        .context
        .as_deref()
        .map(str::trim)
        .filter(|context| !context.is_empty())
    {
        Some(context) => validate_build_context(context)?.to_string(),
        None => default_build_context(containerfile),
    };
    let platform = options
        .platform
        .as_deref()
        .map(str::trim)
        .filter(|platform| !platform.is_empty());
    if let Some(platform) = platform {
        validate_platform(platform)?;
    }
    for arg in &options.args {
        validate_build_arg(arg)?;
    }

    let mut command = base_command();
    command.args(["build", "-f", containerfile, "-t", tag]);
    if let Some(platform) = platform {
        command.args(["--platform", platform]);
    }
    for arg in &options.args {
        command.args(["--build-arg", arg]);
    }
    command.arg(&context);
    Ok(command)
}

fn default_build_context(containerfile: &str) -> String {
    std::path::Path::new(containerfile)
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(|parent| parent.to_string_lossy().into_owned())
        .unwrap_or_else(|| ".".to_string())
}

pub fn validate_build_context(value: &str) -> Result<&str, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("build context cannot be empty".to_string());
    }
    if !std::path::Path::new(value).is_dir() {
        return Err(format!("build context is not a directory: {value}"));
    }
    Ok(value)
}

/// Accept `os/arch` or `os/arch/variant` with the characters Podman allows.
pub fn validate_platform(value: &str) -> Result<&str, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("platform cannot be empty".to_string());
    }
    if value.starts_with('-') {
        return Err("platform cannot start with '-'".to_string());
    }
    if !value.contains('/') {
        return Err("platform must include an architecture (e.g. linux/amd64)".to_string());
    }
    let valid = value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '-' | '_' | '.'));
    if !valid {
        return Err(format!("invalid platform: {value}"));
    }
    Ok(value)
}

/// Split whitespace-separated `KEY=VALUE` build args and validate each token.
pub fn parse_build_args(value: &str) -> Result<Vec<String>, String> {
    let mut args = Vec::new();
    for token in value.split_whitespace() {
        validate_build_arg(token)?;
        args.push(token.to_string());
    }
    Ok(args)
}

fn validate_build_arg(token: &str) -> Result<(), String> {
    if token.starts_with('-') {
        return Err(format!("invalid build arg: {token}"));
    }
    let key = token.split('=').next().unwrap_or_default();
    let valid_key = !key.is_empty()
        && key
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-'));
    if !valid_key {
        return Err(format!("invalid build arg: {token} (expected KEY=VALUE)"));
    }
    Ok(())
}

pub fn validate_image_reference(value: &str) -> Result<&str, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("image reference cannot be empty".to_string());
    }
    if value.starts_with('-') {
        return Err("image reference cannot start with '-'".to_string());
    }
    if value.chars().any(char::is_whitespace) {
        return Err("image reference cannot contain whitespace".to_string());
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
    let json = query(&["inspect", id])?;
    parse_inspect(json.trim()).map_err(|err| format!("could not inspect {id}: {err}"))
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
    fn args(self, key: &str) -> Vec<String> {
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

fn pretty_json(raw: &str) -> String {
    serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .and_then(|value| serde_json::to_string_pretty(&value).ok())
        .unwrap_or_else(|| raw.trim().to_string())
}

fn parse_inspect(json: &str) -> Result<ContainerDetails, String> {
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

fn parse_pod_inspect(json: &str) -> Result<PodDetails, String> {
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

fn parse_volume_inspect(json: &str) -> Result<VolumeDetails, String> {
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

fn parse_network_inspect(json: &str) -> Result<NetworkDetails, String> {
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

fn display_created(raw: &str) -> String {
    if raw.is_empty() {
        "-".to_string()
    } else {
        format_timestamp(Some(raw))
    }
}

fn parse_json<T: for<'de> Deserialize<'de>>(json: &str, command: &str) -> Result<Vec<T>, String> {
    let trimmed = json.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(trimmed)
        .map_err(|err| format!("could not parse `{command}` output: {err}"))
}

fn first_non_empty(values: Option<&[String]>) -> Option<String> {
    values?
        .iter()
        .find(|value| !value.trim().is_empty())
        .cloned()
}

fn short_id(id: &str) -> String {
    id.chars().take(12).collect()
}

fn format_timestamp(raw: Option<&str>) -> String {
    match raw {
        None => "-".to_string(),
        Some(raw) if raw.len() >= 16 => raw[..16].replace('T', " "),
        Some(raw) => raw.to_string(),
    }
}

fn ports_label(ports: &[RawPort]) -> String {
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

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "kB", "MB", "GB", "TB"];
    if bytes < 1000 {
        return format!("{bytes} B");
    }

    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

// ---------------------------------------------------------------------------
// Raw CLI output. Fields are optional because their presence and nullability
// differ between Podman versions.
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawImage {
    id: String,
    #[serde(default)]
    names: Option<Vec<String>>,
    #[serde(default)]
    repo_tags: Option<Vec<String>>,
    #[serde(default)]
    digest: Option<String>,
    #[serde(default)]
    size: Option<u64>,
    #[serde(default)]
    containers: Option<u64>,
    #[serde(default)]
    created_at: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct RawStats {
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
    fn into_sample(self) -> StatsSample {
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

fn parse_percent(value: &str) -> f64 {
    value
        .trim()
        .trim_end_matches('%')
        .parse()
        .unwrap_or_default()
}

/// Parse Podman's `"<used> / <limit>"` strings.
fn parse_io(value: &str) -> (u64, u64) {
    let mut parts = value.split('/');
    let used = parts.next().map(parse_size).unwrap_or_default();
    let limit = parts.next().map(parse_size).unwrap_or_default();
    (used, limit)
}

/// Parse values such as `1.712MB`, `556.2kB`, `16.38GB` or `0B`.
fn parse_size(value: &str) -> u64 {
    let value = value.trim();
    let split = value
        .find(|character: char| {
            !(character.is_ascii_digit()
                || character == '.'
                || character == '+'
                || character == '-')
        })
        .unwrap_or(value.len());
    let (number, unit) = value.split_at(split);
    let number: f64 = number.parse().unwrap_or_default();
    let unit = unit.trim();
    let multiplier = match unit {
        "" | "B" => 1.0,
        "kB" | "KB" => 1e3,
        "MB" => 1e6,
        "GB" => 1e9,
        "TB" => 1e12,
        "PB" => 1e15,
        "KiB" => 1024.0,
        "MiB" => 1_048_576.0,
        "GiB" => 1_073_741_824.0,
        "TiB" => 1_099_511_627_776.0,
        _ => 1.0,
    };
    (number * multiplier).round() as u64
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
struct RawMachine {
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

/// Podman reports some numeric fields as JSON strings on some versions
/// (`"Memory": "2147483648"`, `"DiskSize": "107374182400"`) and as numbers on
/// others, so accept both.
fn lenient_u64<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum NumberOrText {
        Number(u64),
        Text(String),
    }

    Ok(match Option::<NumberOrText>::deserialize(deserializer)? {
        None => None,
        Some(NumberOrText::Number(value)) => Some(value),
        Some(NumberOrText::Text(text)) => text.trim().parse().ok(),
    })
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
struct RawConnection {
    #[serde(default)]
    name: String,
    #[serde(default, rename = "URI")]
    uri: String,
    #[serde(default)]
    identity: String,
    #[serde(default)]
    default: bool,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
struct RawContainer {
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

#[derive(Debug, Deserialize, Default)]
struct RawPort {
    #[serde(default)]
    host_ip: String,
    #[serde(default)]
    host_port: u16,
    #[serde(default)]
    container_port: u16,
    #[serde(default)]
    protocol: String,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
struct RawInspect {
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
struct RawState {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    pid: Option<i64>,
    #[serde(default)]
    exit_code: Option<i64>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
struct RawConfig {
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
struct RawMount {
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    destination: Option<String>,
    #[serde(default, rename = "Type")]
    kind: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
struct RawNetworkSettings {
    #[serde(default)]
    networks: Option<BTreeMap<String, serde_json::Value>>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
struct RawPod {
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
struct RawPodInspect {
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
struct RawVolume {
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
struct RawNetwork {
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
struct RawSubnet {
    #[serde(default)]
    subnet: String,
    #[serde(default)]
    gateway: String,
}

#[cfg(test)]
mod tests;
