//! Which container engine the app talks to.
//!
//! Podman keeps using its CLI (every Podman version that prints `--format json`
//! works) and is the default. Two API engines reach their daemon over HTTP with
//! [`bollard`]: Docker, and Podman's Docker-compatible API on its own socket.
//! The CLI is still used for the interactive surfaces that stream a child
//! process (logs, exec, compose, build, pull/push).
//!
//! Docker lacks several Podman concepts (pods, standalone secrets, machines and
//! Podman's named connections); Podman's Docker-compatible API lacks them too.
//! The UI hides those views for either API engine; the same flags are available
//! through [`EngineKind`] so the rest of the code can guard calls that would
//! otherwise reach the wrong backend.

use super::*;

use serde::{Deserialize, Serialize};

/// The engine backing the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum EngineKind {
    /// The local (or `--connection`-selected) Podman engine, via its CLI.
    #[default]
    Podman,
    /// Podman's Docker-compatible HTTP API, via its local socket.
    PodmanApi,
    /// Docker Engine, via its HTTP API.
    Docker,
}

impl EngineKind {
    pub const ALL: [EngineKind; 3] = [
        EngineKind::Podman,
        EngineKind::PodmanApi,
        EngineKind::Docker,
    ];

    pub fn label(self) -> &'static str {
        match self {
            EngineKind::Podman => "Podman",
            EngineKind::PodmanApi => "Podman (API)",
            EngineKind::Docker => "Docker",
        }
    }

    /// The CLI used for streaming and interactive commands.
    pub fn binary(self) -> &'static str {
        match self {
            EngineKind::Podman | EngineKind::PodmanApi => "podman",
            EngineKind::Docker => "docker",
        }
    }

    /// Whether resource data and actions go through the HTTP API (bollard).
    pub fn uses_api(self) -> bool {
        matches!(self, EngineKind::PodmanApi | EngineKind::Docker)
    }

    pub fn has_pods(self) -> bool {
        self == EngineKind::Podman
    }

    pub fn has_secrets(self) -> bool {
        self == EngineKind::Podman
    }

    pub fn has_machines(self) -> bool {
        self == EngineKind::Podman
    }

    pub fn has_connections(self) -> bool {
        self == EngineKind::Podman
    }
}

/// The engine every command targets.
pub fn engine() -> EngineKind {
    engine_slot().read().map(|slot| *slot).unwrap_or_default()
}

pub fn set_engine(kind: EngineKind) {
    if let Ok(mut slot) = engine_slot().write() {
        *slot = kind;
    }
}

pub fn is_docker() -> bool {
    engine() == EngineKind::Docker
}

/// Whether the current engine reads and acts through the HTTP API.
pub fn uses_api() -> bool {
    engine().uses_api()
}

/// Capabilities of the current engine (the UI uses `EngineKind` directly).
pub fn has_secrets() -> bool {
    engine().has_secrets()
}

pub fn has_machines() -> bool {
    engine().has_machines()
}

/// Fails with a clear message for features the current engine does not have
/// (`what` is plural, e.g. "pods").
pub(super) fn podman_only(what: &str) -> Result<(), String> {
    if engine() != EngineKind::Podman {
        return Err(format!(
            "{what} are not available with the {} engine",
            engine().label()
        ));
    }
    Ok(())
}

fn engine_slot() -> &'static RwLock<EngineKind> {
    static ENGINE: OnceLock<RwLock<EngineKind>> = OnceLock::new();
    ENGINE.get_or_init(|| RwLock::new(EngineKind::Podman))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn podman_is_the_default() {
        assert_eq!(EngineKind::default(), EngineKind::Podman);
        assert_eq!(EngineKind::Podman.binary(), "podman");
        assert_eq!(EngineKind::PodmanApi.binary(), "podman");
        assert_eq!(EngineKind::Docker.binary(), "docker");
        assert!(!EngineKind::Podman.uses_api());
        assert!(EngineKind::PodmanApi.uses_api());
        assert!(EngineKind::Docker.uses_api());
    }

    #[test]
    fn api_engines_lack_podman_only_features() {
        for has in [
            EngineKind::Podman.has_pods(),
            EngineKind::Podman.has_secrets(),
            EngineKind::Podman.has_machines(),
            EngineKind::Podman.has_connections(),
        ] {
            assert!(has);
        }
        for engine in [EngineKind::PodmanApi, EngineKind::Docker] {
            for has in [
                engine.has_pods(),
                engine.has_secrets(),
                engine.has_machines(),
                engine.has_connections(),
            ] {
                assert!(!has, "{} should not have this feature", engine.label());
            }
        }
    }
}
