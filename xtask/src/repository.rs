//! Shared location helper for the xtask commands, so every command
//! resolves the repository root through one contract instead of a copy
//! per module.

use std::path::PathBuf;

/// The repository root, resolved from the compiled-in xtask location so
/// every command is faithful no matter which directory invoked it. The
/// xtask package always lives exactly one level below the root, which is
/// what makes the compiled-in location a reliable anchor.
pub fn repository_root() -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(1)
        .expect("the xtask package always lives one level below the root")
        .to_path_buf()
}
