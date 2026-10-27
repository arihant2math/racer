//! Locates the `data/` directory (cars, tracks) at run time, so built
//! binaries do not depend on where the source checkout was when they were
//! compiled.
//!
//! The first of these wins:
//! 1. `RACER_DATA_DIR`, if set (used as given, even if it does not exist, so
//!    a typo is reported against the path the user asked for);
//! 2. `data/` next to the executable, as in a release archive;
//! 3. the repository's `data/` at compile time, for `cargo run` and
//!    `cargo test` in the checkout.

use std::path::{Path, PathBuf};

/// Environment variable that overrides the data directory.
pub const DATA_DIR_VAR: &str = "RACER_DATA_DIR";

/// The data directory (see the crate docs for how it is found).
pub fn dir() -> PathBuf {
    if let Some(dir) = std::env::var_os(DATA_DIR_VAR).filter(|v| !v.is_empty()) {
        return PathBuf::from(dir);
    }
    let beside_exe = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join("data")));
    match beside_exe {
        Some(dir) if dir.is_dir() => dir,
        _ => Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data"),
    }
}
