//! The typed native-build engine for the pinned MLX and MLX-C pair: one
//! implementation, two entry points. `build.rs` includes this module so a
//! cold `cargo add mlx-c-rust && cargo build` compiles the pair with no
//! configuration, and the repository's `xtask build-native` includes the
//! same module so humans and continuous integration run the identical
//! orchestration with live output. Both entries compile into the same
//! machine-shared, content-addressed prefix, so the pair is compiled once
//! per machine per pin and every later consumer reuses it.
//!
//! The prefix identity hashes the pinned tags and their tarball checksums,
//! in the spirit of a production serving stack's native-build store: a pin
//! or patch edit changes the identity, which orphans the old prefix and
//! triggers exactly one fresh compilation. Phase completion markers make
//! re-runs safe: every step checks for its output before doing work.

use std::{
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    time::Instant,
};

use sha2::{Digest, Sha256};

/// The native pair this crate version is verified against. The crate version
/// names the MLX side; bump these together with the version. MLX-C is the
/// carrier for the C bridge, not the pace-setter: when no MLX-C release
/// pairs with the pinned MLX yet, the forward edit set below adapts the
/// pinned MLX-C sources to it until upstream catches up.
pub const PINNED_MLX_TAG: &str = "v0.32.4";
pub const PINNED_MLX_C_TAG: &str = "v0.7.0";
pub const PINNED_MLX_TARBALL_SHA256: &str =
    "0276db8d7bf1af8db6d4ae25c7c550abea4d5b3b6a16207610a5d4a0c19fe1a0";
pub const PINNED_MLX_C_TARBALL_SHA256: &str =
    "ee726bb38e191bb3c516a6bae47dc8abad9e5f273873385839019ce46bfceab5";
const PINNED_TARBALL_BASE_URL: &str = "https://github.com";

/// Version of the build recipe itself, folded into the prefix identity.
/// Recipe 3 is the first to carry the forward edit set (see below) and to
/// compile MLX-C against THIS engine's pinned MLX through MLX_C_USE_SYSTEM_
/// MLX; recipe 2 added system-MLX linkage; recipe 1 left the option unset,
/// so MLX-C's CMake took its FetchContent fallback — cloning an older MLX
/// and compiling it a second time inside the MLX-C build tree, doubling
/// every cold journey and version-skewing the pair. Bump this when the
/// recipe's build semantics change, so machines holding a prefix built the
/// old way orphan it exactly like a pin bump would.
const PINNED_PAIR_BUILD_RECIPE: &str = "recipe-3-forward-edits";

/// One reviewed forward adaptation of the pinned MLX-C sources to the
/// pinned MLX, carried until an MLX-C release pairs with that MLX (see
/// `drop_with`). C++-side only: the C surface this crate binds is
/// untouched, and COVERAGE_INVENTORY.md's pairing note mirrors this set.
struct ForwardSourceEdit {
    /// Path of the edited file, relative to the extracted MLX-C tree.
    path: &'static str,
    /// The exact text to find. It must occur exactly once — or, on a retry
    /// over already-edited sources, exactly zero times with the replacement
    /// present once — so a drifted upstream fails the build loudly instead
    /// of editing the wrong site.
    find: &'static str,
    /// The replacement text.
    replacement: &'static str,
    /// The MLX-C release that makes this edit droppable; each MLX-C release
    /// reconciles this set.
    drop_with: &'static str,
}

const MLX_C_FORWARD_SOURCE_EDITS: &[ForwardSourceEdit] = &[ForwardSourceEdit {
    path: "mlx/c/ops.cpp",
    find: r#"        mlx::core::gather_qmm(
            mlx_array_get_(x),
            mlx_array_get_(w),
            mlx_array_get_(scales),
            (biases.ctx ? std::make_optional(mlx_array_get_(biases))
                        : std::nullopt),
            (lhs_indices.ctx ? std::make_optional(mlx_array_get_(lhs_indices))
                             : std::nullopt),
            (rhs_indices.ctx ? std::make_optional(mlx_array_get_(rhs_indices))
                             : std::nullopt),
            transpose,
            (group_size.has_value ? std::make_optional<int>(group_size.value)
                                  : std::nullopt),
            (bits.has_value ? std::make_optional<int>(bits.value)
                            : std::nullopt),
            std::string(mode),"#,
    replacement: r#"        mlx::core::gather_qmm(
            mlx_array_get_(x),
            mlx_array_get_(w),
            mlx_array_get_(scales),
            (biases.ctx ? std::make_optional(mlx_array_get_(biases))
                        : std::nullopt),
            (lhs_indices.ctx ? std::make_optional(mlx_array_get_(lhs_indices))
                             : std::nullopt),
            (rhs_indices.ctx ? std::make_optional(mlx_array_get_(rhs_indices))
                             : std::nullopt),
            transpose,
            (group_size.has_value ? std::make_optional<int>(group_size.value)
                                  : std::nullopt),
            (bits.has_value ? std::make_optional<int>(bits.value)
                            : std::nullopt),
            std::string(mode),
            std::nullopt,"#,
    drop_with: "the MLX-C release pairing with MLX v0.32.4, whose \
                    gather_qmm call gains the global_scale parameter this \
                    edit bridges over",
}];

/// Applies the forward edit set to the extracted MLX-C tree. Idempotent: a
/// retry over sources edited by a previous attempt recognizes the
/// replacement and moves on, so a failed later step never wedges the
/// working directory.
fn apply_forward_source_edits(mlx_c_directory: &Path) -> Option<()> {
    for forward_edit in MLX_C_FORWARD_SOURCE_EDITS {
        let edited_file = mlx_c_directory.join(forward_edit.path);
        let source_content = match std::fs::read_to_string(&edited_file) {
            Ok(read_content) => read_content,
            Err(read_error) => {
                println!(
                    "cargo:warning=forward edit could not read {}: {read_error}",
                    edited_file.display()
                );
                return None;
            }
        };
        let find_occurrences = source_content.matches(forward_edit.find).count();
        if find_occurrences == 1 {
            let patched_content =
                source_content.replacen(forward_edit.find, forward_edit.replacement, 1);
            if let Err(write_error) = std::fs::write(&edited_file, patched_content) {
                println!(
                    "cargo:warning=forward edit could not write {}: {write_error}",
                    edited_file.display()
                );
                return None;
            }
        } else {
            let replacement_occurrences = source_content.matches(forward_edit.replacement).count();
            if find_occurrences == 0 && replacement_occurrences == 1 {
                // Already applied by a previous attempt over the same
                // extracted tree; the edit set is idempotent by design.
                continue;
            }
            println!(
                "cargo:warning=forward edit for {} found its target {} times \
                 (already-applied form {} times); the pinned MLX-C sources \
                 drifted — reconcile the forward edit set (droppable with \
                 {})",
                forward_edit.path,
                find_occurrences,
                replacement_occurrences,
                forward_edit.drop_with
            );
            return None;
        }
    }
    Some(())
}

/// The progress-file environment variable: when it names an absolute file,
/// every structured progress line is appended there in addition to stdout.
/// Cargo captures build-script stdout, so under `build.rs` a `tail -f` on
/// this file streams the multi-minute phases into any log as they happen;
/// under `cargo xtask build-native` stdout is already live and the variable
/// is simply unset.
pub const NATIVE_BUILD_PROGRESS_FILE_VARIABLE: &str = "MLX_C_RUST_NATIVE_PROGRESS_FILE";

const MLX_C_SHARED_CACHE_DIRECTORY: &str = ".cache/mlx-c-rust/native";
const COMPLETION_MARKER_DIRECTORY: &str = "complete";

/// The content-addressed identity of the pinned pair: any change to the tags,
/// their verified checksums, or the build recipe below changes the directory,
/// orphaning stale work.
pub fn pinned_pair_identity() -> String {
    let mut hasher = Sha256::new();
    hasher.update(PINNED_MLX_TAG);
    hasher.update(PINNED_MLX_TARBALL_SHA256);
    hasher.update(PINNED_MLX_C_TAG);
    hasher.update(PINNED_MLX_C_TARBALL_SHA256);
    hasher.update(PINNED_PAIR_BUILD_RECIPE);
    format!("{:x}", hasher.finalize())[..16].to_owned()
}

/// The machine-shared prefix for the pinned pair: every consumer on this
/// machine — every project, every build script, every xtask invocation —
/// reads and writes the same directory, so the pair compiles once per
/// machine per pin.
pub fn shared_prefix() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let home = PathBuf::from(home);
    Some(
        home.join(MLX_C_SHARED_CACHE_DIRECTORY)
            .join(pinned_pair_identity())
            .join("prefix"),
    )
}

/// Reports whether `prefix/lib` holds an MLX-C library, shared or static.
pub fn prefix_carries_mlxc_library(prefix: &Path) -> bool {
    let library_directory = prefix.join("lib");
    library_directory.join("libmlxc.dylib").is_file()
        || library_directory.join("libmlxc.a").is_file()
}

/// One outcome of a build attempt, rendered for callers that must decide
/// between proceeding with the prefix or warning and continuing without.
pub enum NativeBuildOutcome {
    /// The prefix was already complete; nothing ran.
    Reused,
    /// The pair was downloaded, verified, and compiled.
    Built,
    /// The build could not proceed; the warning lines name every failure.
    Failed,
}

/// Compiles the pinned pair into the shared prefix if it is not already
/// there. Progress goes to stdout as structured lines (live when run through
/// xtask; captured by cargo under build.rs, where the progress-file variable
/// streams the same events to a tailed file) and every phase is skipped when
/// its completion marker exists, so interrupted builds resume instead of
/// restarting.
pub fn ensure_pinned_pair() -> NativeBuildOutcome {
    let progress_file_path = std::env::var_os(NATIVE_BUILD_PROGRESS_FILE_VARIABLE)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute());
    if let Some(raw_path) = std::env::var_os(NATIVE_BUILD_PROGRESS_FILE_VARIABLE)
        .filter(|path| !Path::new(path).is_absolute())
    {
        println!(
            "cargo:warning={NATIVE_BUILD_PROGRESS_FILE_VARIABLE} must be an absolute path: {}",
            raw_path.to_string_lossy()
        );
    }
    let started_at = Instant::now();
    emit(
        progress_file_path.as_deref(),
        &format!(
            "status=start identity={} mlx={} mlx_c={}",
            pinned_pair_identity(),
            PINNED_MLX_TAG,
            PINNED_MLX_C_TAG
        ),
    );
    let Some(prefix) = shared_prefix() else {
        emit(
            progress_file_path.as_deref(),
            "status=failed reason=no-home",
        );
        return NativeBuildOutcome::Failed;
    };
    if prefix_carries_mlxc_library(&prefix) {
        // The warm path most builds take: report the reuse so callers see
        // why this engine returned in milliseconds.
        emit(
            progress_file_path.as_deref(),
            &format!(
                "status=complete outcome=reused prefix={} elapsed_seconds={:.3}",
                prefix.display(),
                started_at.elapsed().as_secs_f64()
            ),
        );
        return NativeBuildOutcome::Reused;
    }
    let working_directory = prefix
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let marker_directory = working_directory.join(COMPLETION_MARKER_DIRECTORY);
    let _ = std::fs::create_dir_all(&marker_directory);

    for required_tool in ["curl", "tar", "cmake"] {
        if Command::new(required_tool)
            .arg("--version")
            .output()
            .is_err()
        {
            println!(
                "cargo:warning=automatic MLX build needs `{required_tool}` on PATH \
                 (brew install {required_tool}); no MLX-C library will be linked"
            );
            emit(
                progress_file_path.as_deref(),
                &format!("status=failed reason=missing-tool tool={required_tool}"),
            );
            return NativeBuildOutcome::Failed;
        }
    }

    let download_operation = begin(progress_file_path.as_deref(), "download-and-verify");
    let Some(mlx_source) = download_verified_tarball(
        &marker_directory,
        &working_directory,
        "mlx.tar.gz",
        &format!(
            "{PINNED_TARBALL_BASE_URL}/ml-explore/mlx/archive/refs/tags/{PINNED_MLX_TAG}.tar.gz"
        ),
        PINNED_MLX_TARBALL_SHA256,
    ) else {
        download_operation.complete("failed");
        return NativeBuildOutcome::Failed;
    };
    let Some(mlx_c_source) = download_verified_tarball(
        &marker_directory,
        &working_directory,
        "mlx-c.tar.gz",
        &format!(
            "{PINNED_TARBALL_BASE_URL}/ml-explore/mlx-c/archive/refs/tags/{PINNED_MLX_C_TAG}.tar.gz"
        ),
        PINNED_MLX_C_TARBALL_SHA256,
    ) else {
        download_operation.complete("failed");
        return NativeBuildOutcome::Failed;
    };
    download_operation.complete("ok");

    let mlx_directory = working_directory.join("mlx");
    let mlx_c_directory = working_directory.join("mlx-c");
    let extract_operation = begin(progress_file_path.as_deref(), "extract");
    if !mlx_directory.is_dir() {
        if run_step(
            &working_directory,
            Command::new("tar").arg("xzf").arg(&mlx_source),
        )
        .is_none()
        {
            extract_operation.complete("failed");
            return NativeBuildOutcome::Failed;
        }
        let _ = std::fs::rename(
            working_directory.join(format!("mlx-{}", PINNED_MLX_TAG.trim_start_matches('v'))),
            &mlx_directory,
        );
    }
    if !mlx_c_directory.is_dir() {
        if run_step(
            &working_directory,
            Command::new("tar").arg("xzf").arg(&mlx_c_source),
        )
        .is_none()
        {
            extract_operation.complete("failed");
            return NativeBuildOutcome::Failed;
        }
        let _ = std::fs::rename(
            working_directory.join(format!(
                "mlx-c-{}",
                PINNED_MLX_C_TAG.trim_start_matches('v')
            )),
            &mlx_c_directory,
        );
    }
    extract_operation.complete("ok");

    // Before any configure: bring the pinned MLX-C sources up to the pinned
    // MLX. Skipping this on a pairing gap is not an option — the compile
    // would fail (or, historically, silently take the FetchContent fallback
    // and build a second, older MLX).
    let forward_edit_operation = begin(progress_file_path.as_deref(), "forward-edits");
    if apply_forward_source_edits(&mlx_c_directory).is_none() {
        forward_edit_operation.complete("failed");
        return NativeBuildOutcome::Failed;
    }
    forward_edit_operation.complete("ok");

    let parallelism = std::env::var("NUM_JOBS").unwrap_or_else(|_| "4".to_owned());
    let mlx_build = working_directory.join("mlx-build");
    let mlx_marker = marker_directory.join("mlx");
    if !mlx_marker.exists() {
        let operation = begin(progress_file_path.as_deref(), "compile-mlx");
        let configured = run_step(
            &working_directory,
            Command::new("cmake")
                .arg("-S")
                .arg(&mlx_directory)
                .arg("-B")
                .arg(&mlx_build)
                .arg("-DCMAKE_BUILD_TYPE=Release")
                .arg("-DBUILD_SHARED_LIBS=ON")
                .arg("-DMLX_METAL_JIT=ON")
                .arg(format!("-DCMAKE_INSTALL_PREFIX={}", prefix.display())),
        )
        .and_then(|()| {
            run_step(
                &working_directory,
                Command::new("cmake")
                    .arg("--build")
                    .arg(&mlx_build)
                    .arg("--parallel")
                    .arg(&parallelism),
            )
        })
        .and_then(|()| {
            run_step(
                &working_directory,
                Command::new("cmake").arg("--install").arg(&mlx_build),
            )
        });
        if configured.is_none() {
            operation.complete("failed");
            // A failed CMake configure leaves a CMakeCache.txt that pins the
            // poisoned attempt — a stale MLX_DIR, an old option set — which a
            // retry, including one through a changed recipe, must not
            // inherit; find_package consults the cached value first, ahead of
            // any prefix path. Start the next attempt from a clean cache.
            let _ = std::fs::remove_dir_all(&mlx_build);
            return NativeBuildOutcome::Failed;
        }
        operation.complete("ok");
        let _ = std::fs::write(&mlx_marker, b"complete");
    }

    let mlx_c_build = working_directory.join("mlx-c-build");
    let mlx_c_marker = marker_directory.join("mlx-c");
    if !mlx_c_marker.exists() {
        let operation = begin(progress_file_path.as_deref(), "compile-mlx-c");
        let configured = run_step(
            &working_directory,
            Command::new("cmake")
                .arg("-S")
                .arg(&mlx_c_directory)
                .arg("-B")
                .arg(&mlx_c_build)
                .arg("-DCMAKE_BUILD_TYPE=Release")
                .arg("-DBUILD_SHARED_LIBS=ON")
                .arg("-DMLX_BUILD_EXAMPLES=OFF")
                // THE switch that makes MLX-C compile against this engine's
                // pinned MLX. MLX-C's CMake chooses between find_package(MLX)
                // and a FetchContent fallback through MLX_C_USE_SYSTEM_MLX —
                // leaving the option unset (the default) made the fallback
                // clone an older MLX and compile it a second time inside
                // mlx-c-build: every cold journey took twice the time and the
                // pair was version-skewed against the documented pin. The
                // hint is the INSTALLED config under the prefix (already
                // placed by the MLX install step above): the build tree's own
                // MLXConfig.cmake references install-time files that do not
                // exist and resolves to broken paths, so CMAKE_PREFIX_PATH —
                // not the build directory — is what find_package needs.
                .arg("-DMLX_C_USE_SYSTEM_MLX=ON")
                .arg(format!("-DCMAKE_PREFIX_PATH={}", prefix.display()))
                .arg(format!("-DCMAKE_INSTALL_PREFIX={}", prefix.display())),
        )
        .and_then(|()| {
            run_step(
                &working_directory,
                Command::new("cmake")
                    .arg("--build")
                    .arg(&mlx_c_build)
                    .arg("--parallel")
                    .arg(&parallelism),
            )
        })
        .and_then(|()| {
            run_step(
                &working_directory,
                Command::new("cmake").arg("--install").arg(&mlx_c_build),
            )
        });
        if configured.is_none() {
            operation.complete("failed");
            // Same cache-poisoning rationale as the MLX side: a failed
            // configure must not pin its decisions into the retry.
            let _ = std::fs::remove_dir_all(&mlx_c_build);
            return NativeBuildOutcome::Failed;
        }
        operation.complete("ok");
        let _ = std::fs::write(&mlx_c_marker, b"complete");
    }

    if prefix_carries_mlxc_library(&prefix) {
        emit(
            progress_file_path.as_deref(),
            &format!(
                "status=complete outcome=built prefix={} elapsed_seconds={:.3}",
                prefix.display(),
                started_at.elapsed().as_secs_f64()
            ),
        );
        NativeBuildOutcome::Built
    } else {
        emit(
            progress_file_path.as_deref(),
            "status=failed reason=library-missing-after-build",
        );
        NativeBuildOutcome::Failed
    }
}

fn begin(progress_file_path: Option<&Path>, operation_name: &str) -> OperationTimer {
    emit(
        progress_file_path,
        &format!("operation={operation_name} status=start"),
    );
    OperationTimer {
        started_at: Instant::now(),
        progress_file_path: progress_file_path.map(Path::to_path_buf),
        operation_name: operation_name.to_owned(),
    }
}

struct OperationTimer {
    started_at: Instant,
    progress_file_path: Option<PathBuf>,
    operation_name: String,
}

impl OperationTimer {
    fn complete(self, outcome: &str) {
        emit(
            self.progress_file_path.as_deref(),
            &format!(
                "operation={} status={outcome} elapsed_seconds={:.3}",
                self.operation_name,
                self.started_at.elapsed().as_secs_f64()
            ),
        );
    }
}

fn emit(progress_file_path: Option<&Path>, line_body: &str) {
    println!("[mlx-c-rust-native-build] {line_body}");
    if let Some(progress_file_path) = progress_file_path {
        // A per-write append-mode open keeps reporting lock-free, and
        // ignoring write errors keeps telemetry best-effort: reporting can
        // never fail the build it describes.
        if let Ok(mut progress_file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(progress_file_path)
        {
            let _ = writeln!(progress_file, "[mlx-c-rust-native-build] {line_body}");
        }
    }
}

/// Downloads `tarball_name` from `tarball_url` unless the completion marker
/// records it as already verified, and checks its SHA-256 against
/// `expected_sha256` before any use.
fn download_verified_tarball(
    marker_directory: &Path,
    working_directory: &Path,
    tarball_name: &str,
    tarball_url: &str,
    expected_sha256: &str,
) -> Option<PathBuf> {
    let tarball_path = working_directory.join(tarball_name);
    let marker = marker_directory.join(format!("{tarball_name}.verified"));
    if !marker.exists() {
        if !tarball_path.exists()
            && run_step(
                working_directory,
                Command::new("curl")
                    .arg("-sSL")
                    .arg("-o")
                    .arg(&tarball_path)
                    .arg(tarball_url),
            )
            .is_none()
        {
            return None;
        }
        let tarball_bytes = std::fs::read(&tarball_path).ok()?;
        let mut hasher = Sha256::new();
        hasher.update(&tarball_bytes);
        let actual_sha256 = format!("{:x}", hasher.finalize());
        if actual_sha256 != expected_sha256 {
            println!(
                "cargo:warning={} has SHA-256 {actual_sha256}, expected {expected_sha256}; \
                 refusing to build from an unverified source",
                tarball_path.display()
            );
            return None;
        }
        let _ = std::fs::write(&marker, b"verified");
    }
    Some(tarball_path)
}

/// Runs one step, naming the failing phase in a warning line.
fn run_step(working_directory: &Path, command: &mut Command) -> Option<()> {
    let command_status = command
        .current_dir(working_directory)
        .status()
        .map_err(|launch_error| {
            println!(
                "cargo:warning=failed to run {}: {launch_error}",
                command.get_program().to_string_lossy()
            );
        })
        .ok()?;
    if command_status.success() {
        Some(())
    } else {
        println!(
            "cargo:warning=native build step failed: {}",
            command.get_program().to_string_lossy()
        );
        None
    }
}
