//! Which container engine the app talks to.
//!
//! Podman keeps using its CLI (every Podman version that prints `--format json`
//! works). Docker is reached through its HTTP API with [`bollard`], so the
//! Docker CLI is only needed for the interactive surfaces that stream a child
//! process (logs, exec, compose, build, pull/push).
//!
//! Docker lacks several Podman concepts (pods, standalone secrets, machines and
//! Podman's named connections). The UI hides those views when Docker is
//! selected; the same flags are available through [`EngineKind`] so the rest of
//! the code can guard calls that would otherwise reach the wrong backend.

use super::*;

use serde::{Deserialize, Serialize};

/// The engine backing the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum EngineKind {
    /// The local (or `--connection`-selected) Podman engine, via its CLI.
    #[default]
    Podman,
    /// Docker Engine, via its HTTP API.
    Docker,
}

impl EngineKind {
    pub const ALL: [EngineKind; 2] = [EngineKind::Podman, EngineKind::Docker];

    pub fn label(self) -> &'static str {
        match self {
            EngineKind::Podman => "Podman",
            EngineKind::Docker => "Docker",
        }
    }

    /// The CLI used for streaming and interactive commands.
    pub fn binary(self) -> &'static str {
        match self {
            EngineKind::Podman => "podman",
            EngineKind::Docker => "docker",
        }
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

/// Fails with a clear message when Docker is selected, for features only
/// Podman has (`what` is plural, e.g. "pods").
pub(super) fn podman_only(what: &str) -> Result<(), String> {
    if is_docker() {
        return Err(format!("{what} are not available with the Docker engine"));
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
        assert_eq!(EngineKind::Docker.binary(), "docker");
    }

    #[test]
    fn docker_lacks_podman_only_features() {
        for has in [
            EngineKind::Podman.has_pods(),
            EngineKind::Podman.has_secrets(),
            EngineKind::Podman.has_machines(),
            EngineKind::Podman.has_connections(),
        ] {
            assert!(has);
        }
        for has in [
            EngineKind::Docker.has_pods(),
            EngineKind::Docker.has_secrets(),
            EngineKind::Docker.has_machines(),
            EngineKind::Docker.has_connections(),
        ] {
            assert!(!has);
        }
    }
}
