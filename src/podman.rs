//! Minimal client for the local Podman CLI.
//!
//! The app shells out to `podman` instead of talking to the libpod REST API so
//! that it works with every Podman version that supports `--format json`.
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
// other's items through `use super::*`.
mod compose;
mod containers;
mod exec;
mod images;
mod machines;
mod registry;
mod resources;
mod system;
mod util;

pub use compose::*;
pub use containers::*;
pub use exec::*;
pub use images::*;
pub use machines::*;
pub use registry::*;
pub use resources::*;
pub use system::*;
pub use util::*;

#[cfg(test)]
mod tests;
