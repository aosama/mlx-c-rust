//! Typed repository tooling for mlx-c-rust, following the xtask pattern:
//! repository tooling is typed, unit-testable Rust invoked as
//! `cargo xtask <command>` instead of shell. The alias that wires the
//! invocation is documented in `.cargo/config.toml`.

#[path = "../../native_build.rs"]
mod native_build;

mod cache_prune;
mod hooks;
mod repository;
mod source_size;
mod verify;

use std::process::ExitCode;

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match arguments.first().map(String::as_str) {
        Some("build-native") => build_native(arguments.get(1..).unwrap_or(&[])),
        Some("source-size") => source_size::run(arguments.get(1..).unwrap_or(&[])),
        Some("verify") => verify::run(),
        Some("prune-caches") => cache_prune::run(arguments.get(1..).unwrap_or(&[])),
        Some("install-hooks") => hooks::install(),
        Some("--version") | Some("version") => {
            println!("xtask {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!(
                "usage: cargo xtask <command>\n\ncommands:\n  \
                 build-native [--force]  Compile the pinned MLX and MLX-C pair into the \
                 machine-shared prefix (reuses an existing build; --force recompiles).\n  \
                 source-size [--staged] Enforce the per-file line ceiling on owned code \
                 files (--staged inspects only the index; default scans the repository).\n  \
                 verify                 Run the continuous-integration lint lane locally: \
                 size ceiling, formatting, clippy, documentation.\n  \
                 prune-caches [--dry-run] [--repo owner/name]\n \
                 \x20                       Delete surplus GitHub Actions cache generations \
                 so the repository stays under its cache budget.\n  \
                 install-hooks           Point git at .githooks so the pre-commit and \
                 pre-push gates are active for this clone."
            );
            ExitCode::FAILURE
        }
    }
}

/// The pinned-pair build command: the same engine the crate's build script
/// runs, but with live structured output on stdout for humans and
/// continuous integration, and a `--force` switch that clears the shared
/// identity directory first.
fn build_native(arguments: &[String]) -> ExitCode {
    let force = arguments.iter().any(|argument| argument == "--force");
    let Some(prefix) = native_build::shared_prefix() else {
        eprintln!("build-native: HOME must be set to locate the shared native prefix");
        return ExitCode::FAILURE;
    };
    if force && let Some(identity_directory) = prefix.parent() {
        println!(
            "[xtask] clearing the shared native directory {}",
            identity_directory.display()
        );
        let _ = std::fs::remove_dir_all(identity_directory);
    }
    let started_at = std::time::Instant::now();
    let outcome = native_build::ensure_pinned_pair();
    match outcome {
        native_build::NativeBuildOutcome::Built => {
            println!(
                "[xtask] built the pinned pair in {:.1}s into {}",
                started_at.elapsed().as_secs_f64(),
                prefix.display()
            );
            ExitCode::SUCCESS
        }
        native_build::NativeBuildOutcome::Reused => {
            println!(
                "[xtask] reused the existing pinned pair at {} ({:.1}s)",
                prefix.display(),
                started_at.elapsed().as_secs_f64()
            );
            ExitCode::SUCCESS
        }
        native_build::NativeBuildOutcome::Failed => {
            eprintln!("[xtask] the native build failed; see the warnings above");
            ExitCode::FAILURE
        }
    }
}
