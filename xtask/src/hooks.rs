//! `cargo xtask install-hooks` — points git at the repository's
//! `.githooks` directory so the pre-commit and pre-push gates become
//! active for this checkout. Typed Rust issues the one `git config` call
//! instead of a shell script, and the hooks themselves stay thin shims
//! that delegate every real decision to `cargo xtask` commands.

use std::process::{Command, ExitCode};

use super::repository::repository_root;

/// The gate hooks this repository ships. Printed in the confirmation so
/// the human sees exactly which gates just became active.
const GATE_HOOK_NAMES: [&str; 2] = ["pre-commit", "pre-push"];

pub fn install() -> ExitCode {
    let hooks_directory = repository_root().join(".githooks");
    for gate_hook_name in GATE_HOOK_NAMES {
        let gate_hook_path = hooks_directory.join(gate_hook_name);
        if !gate_hook_path.is_file() {
            eprintln!(
                "[install-hooks] status=failed reason=missing-hook path={}",
                gate_hook_path.display()
            );
            return ExitCode::FAILURE;
        }
    }
    let config_status = Command::new("git")
        .args(["config", "core.hooksPath", ".githooks"])
        .current_dir(repository_root())
        .status();
    match config_status {
        Ok(status) if status.success() => {
            println!(
                "[install-hooks] status=success hooks={} note=run once per-clone; re-run after adding a hook",
                GATE_HOOK_NAMES.join(",")
            );
            ExitCode::SUCCESS
        }
        Ok(status) => {
            eprintln!(
                "[install-hooks] status=failed exit={}",
                status.code().unwrap_or(-1)
            );
            ExitCode::FAILURE
        }
        Err(launch_error) => {
            eprintln!("[install-hooks] status=failed error={launch_error}");
            ExitCode::FAILURE
        }
    }
}
