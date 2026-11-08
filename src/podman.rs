//! Client for the selected container engine.
//!
//! The **Podman** engine shells out to the `podman` CLI so it works with every
//! version that supports `--format json`; the **Podman (API)** and **Docker**
//! engines talk to their daemon over HTTP through [`bollard`] (see `api.rs`).
//! Both paths produce the same snapshot and action types, so the UI is unaware
//! of which one is in use.
//!
//! The `Raw*` structs mirror the CLI's JSON output. Their fields are optional
//! because presence and nullability differ between Podman versions.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, OnceLock, RwLock};
use std::time::{Duration, Instant};

use serde::Deserialize;

// Each module covers one area of the CLI. Everything is re-exported here, so
// callers keep using `podman::…`; the modules share these imports and each
// other's items through `use super::*`. The general-purpose helpers live in
// `crate::util`.
mod api;
mod compose;
mod containers;
mod engine;
mod exec;
mod images;
mod machines;
mod registry;
mod resources;
mod secrets;
mod system;

pub use crate::util::*;
pub use compose::*;
pub use containers::*;
pub use engine::*;
pub use exec::*;
pub use images::*;
pub use machines::*;
pub use registry::*;
pub use resources::*;
pub use secrets::*;
pub use system::*;

#[cfg(test)]
mod tests;
