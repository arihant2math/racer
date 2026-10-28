//! Locates the `data/` directory (cars, tracks) and the built `assets/`
//! directory (car models) at run time, so built binaries do not depend on
//! where the source checkout was when they were compiled.
//!
//! For each directory the first of these wins:
//! 1. its environment variable (`RACER_DATA_DIR`, `RACER_ASSETS_DIR`), if
//!    set (used as given, even if it does not exist, so a typo is reported
//!    against the path the user asked for);
//! 2. `data/` (`assets/`) next to the executable, as in a release archive;
//! 3. the repository's `data/` (`assets/`) at compile time, for `cargo run`
//!    and `cargo test` in the checkout.

use std::path::{Path, PathBuf};

/// Environment variable that overrides the data directory.
pub const DATA_DIR_VAR: &str = "RACER_DATA_DIR";

/// Environment variable that overrides the assets directory.
pub const ASSETS_DIR_VAR: &str = "RACER_ASSETS_DIR";

/// The data directory (see the crate docs for how it is found).
pub fn dir() -> PathBuf {
    locate(DATA_DIR_VAR, "data")
}

/// The assets directory (see the crate docs for how it is found).
pub fn assets_dir() -> PathBuf {
    locate(ASSETS_DIR_VAR, "assets")
}

fn locate(var: &str, name: &str) -> PathBuf {
    if let Some(dir) = std::env::var_os(var).filter(|v| !v.is_empty()) {
        return PathBuf::from(dir);
    }
    let beside_exe = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join(name)));
    match beside_exe {
        Some(dir) if dir.is_dir() => dir,
        _ => Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(name),
    }
}
