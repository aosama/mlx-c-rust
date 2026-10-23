//! `cargo xtask verify` — the local mirror of the continuous-integration
//! lint lane: the same checks, in the same configuration, the check job
//! runs. The pre-push hook calls this command so a locally catchable
//! failure surfaces before the push spends a paid GitHub Actions round
//! trip. Running the lanes from typed Rust (instead of a shell script)
//! keeps the sequencing, timing, and failure reporting unit-testable.

use std::process::{Command, ExitCode};

use super::repository::repository_root;
use super::source_size;

/// One lane of verification: the cargo arguments that run it and the name
/// reported in the structured output.
struct VerificationLane {
    lane_name: &'static str,
    cargo_arguments: &'static [&'static str],
}

/// The lanes, in the continuous-integration lane's order. Clippy and doc
/// target aarch64-apple-darwin — the one target this crate ships and
/// documents — so the local mirror cannot drift from what the runner
/// proves. The bindings-only lane compiles with the `link` feature off,
/// the mode consumers who provide their own native MLX build take.
const VERIFICATION_LANES: [VerificationLane; 4] = [
    VerificationLane {
        lane_name: "formatting",
        cargo_arguments: &["fmt", "--all", "--", "--check"],
    },
    VerificationLane {
        lane_name: "clippy",
        cargo_arguments: &[
            "clippy",
            "--all-targets",
            "--target",
            "aarch64-apple-darwin",
            "--",
            "-D",
            "warnings",
        ],
    },
    VerificationLane {
        lane_name: "documentation",
        cargo_arguments: &["doc", "--no-deps", "--target", "aarch64-apple-darwin"],
    },
    VerificationLane {
        lane_name: "bindings-only",
        cargo_arguments: &[
            "check",
            "--no-default-features",
            "--target",
            "aarch64-apple-darwin",
        ],
    },
];

/// The cargo binary to invoke. Cargo exports `CARGO` when it runs the
/// xtask binary, so a non-standard cargo on `PATH` cannot diverge the
/// verification from the build that invoked it.
fn cargo_executable_path() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned())
}

/// Runs every lane plus the repository-wide size ceiling, stopping at the
/// first failure so the report names the earliest defect, exactly like
/// separate continuous-integration steps would.
pub fn run() -> ExitCode {
    let started_at = std::time::Instant::now();
    println!("[verify] status=start");

    // The size ceiling is checked first because it is the cheapest lane
    // and its violations shape how any fix should be structured.
    let source_size_arguments: Vec<String> = Vec::new();
    if source_size::run(&source_size_arguments) == ExitCode::FAILURE {
        eprintln!(
            "[verify] lane=source-size status=fail hint=fix the size violations above, then verify again"
        );
        return ExitCode::FAILURE;
    }
    println!(
        "[verify] lane=source-size status=pass elapsed_seconds={:.1}",
        started_at.elapsed().as_secs_f64()
    );

    for verification_lane in &VERIFICATION_LANES {
        let lane_started_at = std::time::Instant::now();
        let lane_status = Command::new(cargo_executable_path())
            .args(verification_lane.cargo_arguments)
            .current_dir(repository_root())
            .status();
        let lane_elapsed_seconds = lane_started_at.elapsed().as_secs_f64();
        match lane_status {
            Ok(status) if status.success() => {
                println!(
                    "[verify] lane={} status=pass elapsed_seconds={lane_elapsed_seconds:.1}",
                    verification_lane.lane_name
                );
            }
            Ok(status) => {
                eprintln!(
                    "[verify] lane={} status=fail exit={} elapsed_seconds={lane_elapsed_seconds:.1}",
                    verification_lane.lane_name,
                    status.code().unwrap_or(-1)
                );
                eprintln!(
                    "[verify] hint=fix the failures above locally before spending a GitHub Actions run on them"
                );
                return ExitCode::FAILURE;
            }
            Err(launch_error) => {
                eprintln!(
                    "[verify] lane={} status=not-launched error={launch_error}",
                    verification_lane.lane_name
                );
                return ExitCode::FAILURE;
            }
        }
    }

    println!(
        "[verify] status=success elapsed_seconds={:.1}",
        started_at.elapsed().as_secs_f64()
    );
    ExitCode::SUCCESS
}
