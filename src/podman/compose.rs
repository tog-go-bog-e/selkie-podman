//! Compose projects, grouped from container labels, and `podman compose` commands.

use super::*;

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
pub(super) fn compose_base(project: &ComposeProject) -> Result<Command, String> {
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

pub(super) fn compose_membership(
    labels: Option<&HashMap<String, String>>,
) -> Option<ComposeMembership> {
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
pub(super) fn compose_label(labels: &HashMap<String, String>, suffix: &str) -> Option<String> {
    ["com.docker.compose.", "io.podman.compose."]
        .iter()
        .find_map(|prefix| {
            labels
                .get(&format!("{prefix}{suffix}"))
                .filter(|value| !value.is_empty())
                .cloned()
        })
}
