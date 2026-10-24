//! The gap-shim build phase: compiles the repository's mlxrs_* C ABI as
//! the third component of the native prefix, after MLX and MLX-C. Split
//! from the engine's pair orchestration so each module carries one build
//! concern; the engine still owns download, verification, forward edits,
//! and the pair phases.

use std::path::Path;
use std::process::Command;

use super::{begin, run_step};

/// Configures, builds, and installs the gap shim from the repository
/// sources into the prefix, resumable through its completion marker.
/// Returns None on failure, with the failure named in cargo warnings and
/// the structured progress stream.
pub(crate) fn build_gap_shim(
    progress_file_path: Option<&Path>,
    working_directory: &Path,
    marker_directory: &Path,
    prefix: &Path,
    parallelism: &str,
    gap_shim_source: &Path,
) -> Option<()> {
    let gap_shim_build = working_directory.join("gap-shim-build");
    let gap_shim_marker = marker_directory.join("gap-shim");
    if gap_shim_marker.exists() {
        return Some(());
    }
    let operation = begin(progress_file_path, "compile-gap-shim");
    let configured = run_step(
        working_directory,
        Command::new("cmake")
            .arg("-S")
            .arg(gap_shim_source)
            .arg("-B")
            .arg(&gap_shim_build)
            .arg("-DCMAKE_BUILD_TYPE=Release")
            .arg("-DBUILD_SHARED_LIBS=OFF")
            .arg(format!("-DCMAKE_PREFIX_PATH={}", prefix.display()))
            .arg(format!("-DCMAKE_INSTALL_PREFIX={}", prefix.display())),
    );
    if configured.is_none() {
        operation.complete("failed");
        // Same configure-poisoning rationale as the pair phases: a failed
        // configure leaves a CMakeCache.txt a retry must not inherit.
        purge_build_tree(&gap_shim_build);
        return None;
    }
    let built_and_installed = configured
        .and_then(|()| {
            run_step(
                working_directory,
                Command::new("cmake")
                    .arg("--build")
                    .arg(&gap_shim_build)
                    .arg("--parallel")
                    .arg(parallelism),
            )
        })
        .and_then(|()| {
            run_step(
                working_directory,
                Command::new("cmake").arg("--install").arg(&gap_shim_build),
            )
        });
    if built_and_installed.is_none() {
        // Build or install failure after a sound configure keeps the tree,
        // exactly as in the pair phases: a retry resumes incrementally.
        operation.complete("failed");
        return None;
    }
    operation.complete("ok");
    let _ = std::fs::write(&gap_shim_marker, b"complete");
    Some(())
}

/// Purges a poisoned configure tree, loud when even the purge fails.
fn purge_build_tree(gap_shim_build: &Path) {
    if let Err(purge_error) = std::fs::remove_dir_all(gap_shim_build) {
        println!(
            "cargo:warning=failed to purge the poisoned CMake build tree at {}: \
             {purge_error}",
            gap_shim_build.display()
        );
    }
}
