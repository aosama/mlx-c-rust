//! Standalone bindgen generation for the public MLX-C API, from the headers
//! vendored under `vendor/mlx-c/`.
//!
//! This build script translates the vendored headers of the pinned MLX-C
//! release into Rust declarations and emits the bridged-symbol inventory that
//! the coverage contract compares against those same headers.
//!
//! Linking a binary that uses this crate needs an MLX-C library, and the
//! default is zero configuration: with nothing set, the script downloads the
//! pinned MLX and MLX-C source releases (checksum-verified), compiles them
//! with CMake once, and links against the result. Explicit overrides win in
//! this order: the `MLX_C_RUST_MLX_C_PREFIX` environment variable (a prefix
//! whose `lib/` holds the libraries), then `pkg-config mlx-c`, then the
//! standard Homebrew prefixes. The built prefix caches inside the build
//! directory and survives incremental rebuilds until `cargo clean`.

use std::{
    env,
    error::Error,
    path::{Path, PathBuf},
    process::Command,
};

use pkg_config::Config as PkgConfig;
use sha2::{Digest, Sha256};

const VENDORED_HEADERS_DIRECTORY: &str = "vendor/mlx-c";
const MLX_C_PREFIX_VARIABLE: &str = "MLX_C_RUST_MLX_C_PREFIX";

/// The native pair this crate version is verified against. The crate version
/// names the MLX side; bump both together with the version.
const PINNED_MLX_TAG: &str = "v0.32.3";
const PINNED_MLX_C_TAG: &str = "v0.7.0";
const PINNED_MLX_TARBALL_SHA256: &str =
    "4129039ddcb36cb860982b616c975a5b9b8e6d8fa97efa86335b4427c29c4b4b";
const PINNED_MLX_C_TARBALL_SHA256: &str =
    "ee726bb38e191bb3c516a6bae47dc8abad9e5f273873385839019ce46bfceab5";
const PINNED_TARBALL_BASE_URL: &str = "https://github.com";

const BINDGEN_FUNCTION_ALLOWLIST: &str = "mlx_.*";
const BINDGEN_TYPE_ALLOWLIST: &str = "mlx_.*";

/// The generated inventory module consumed by `raw.rs`; it exposes the exact
/// bridged symbol sets the coverage contract compares against the vendored
/// headers.
const BRIDGED_INVENTORY_FILE_NAME: &str = "bridged_inventory.rs";

fn main() -> Result<(), Box<dyn Error>> {
    let manifest_directory =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").ok_or("CARGO_MANIFEST_DIR is unset")?);
    let output_directory = PathBuf::from(env::var_os("OUT_DIR").ok_or("OUT_DIR is unset")?);
    let include_directory = manifest_directory.join(VENDORED_HEADERS_DIRECTORY);
    let mlx_c_header = include_directory.join("mlx/c/mlx.h");
    if !mlx_c_header.is_file() {
        return Err(format!(
            "missing vendored MLX C umbrella header at {}",
            mlx_c_header.display()
        )
        .into());
    }

    println!("cargo:rerun-if-changed={}", include_directory.display());
    println!("cargo:rerun-if-env-changed={MLX_C_PREFIX_VARIABLE}");

    let bindings = bindgen::Builder::default()
        .header(mlx_c_header.to_string_lossy())
        .clang_arg(format!("-I{}", include_directory.display()))
        .allowlist_function(BINDGEN_FUNCTION_ALLOWLIST)
        .allowlist_type(BINDGEN_TYPE_ALLOWLIST)
        .generate_comments(false)
        .generate()?;
    let bindings_content = bindings.to_string();
    write_bridged_inventory(&bindings_content, &output_directory)?;
    bindings.write_to_file(output_directory.join("mlx_c_bindings.rs"))?;

    emit_link_instructions();
    Ok(())
}

/// Emits the linker instructions for an MLX-C library. The ladder, first
/// match wins: the `MLX_C_RUST_MLX_C_PREFIX` environment variable, the
/// standard Homebrew prefixes, `pkg-config mlx-c`, and — the zero-config
/// default — downloading and compiling the pinned MLX and MLX-C sources.
fn emit_link_instructions() {
    if let Some(prefix) = env::var_os(MLX_C_PREFIX_VARIABLE) {
        let prefix = PathBuf::from(prefix);
        if prefix_carries_mlxc_library(&prefix) {
            emit_prefix_link_flags(&prefix);
            return;
        }
        println!(
            "cargo:warning=MLX_C_RUST_MLX_C_PREFIX is set but {} holds no MLX-C library; continuing down the discovery ladder",
            prefix.join("lib").display()
        );
    }
    if let Some(prefix) = homebrew_prefix_with_mlxc() {
        emit_prefix_link_flags(&prefix);
        return;
    }
    if let Ok(mlx_c) = PkgConfig::new().atleast_version("0.7.0").probe("mlx-c") {
        for library_name in &mlx_c.libs {
            println!("cargo:rustc-link-lib={library_name}");
        }
        for search_path in &mlx_c.link_paths {
            println!("cargo:rustc-link-search=native={}", search_path.display());
        }
        for framework in &mlx_c.frameworks {
            println!("cargo:rustc-link-lib=framework={framework}");
        }
        for framework_path in &mlx_c.framework_paths {
            println!(
                "cargo:rustc-link-search=framework={}",
                framework_path.display()
            );
        }
        return;
    }
    if let Some(prefix) = fetch_and_build_pinned_pair() {
        emit_prefix_link_flags(&prefix);
        return;
    }
    println!(
        "cargo:warning=no MLX-C library could be located or built; the crate compiles, \
         but linking a binary that uses it will report undefined MLX symbols. Set \
         {MLX_C_PREFIX_VARIABLE}, install mlx-c through Homebrew, or install CMake \
         (brew install cmake) so the pinned sources can be built automatically."
    );
}

/// Reports whether `prefix/lib` holds an MLX-C library, shared or static.
fn prefix_carries_mlxc_library(prefix: &Path) -> bool {
    let library_directory = prefix.join("lib");
    library_directory.join("libmlxc.dylib").is_file()
        || library_directory.join("libmlxc.a").is_file()
}

/// Finds a Homebrew installation whose `lib` already carries MLX-C (the
/// bottled `mlx-c` formula links here after `brew install mlx-c`).
fn homebrew_prefix_with_mlxc() -> Option<PathBuf> {
    let mut candidates = vec![PathBuf::from("/opt/homebrew")];
    if let Some(home) = env::var_os("HOME") {
        candidates.push(PathBuf::from(home).join("homebrew"));
    }
    candidates
        .into_iter()
        .find(|prefix| prefix_carries_mlxc_library(prefix))
}

/// Emits the link flags for a prefix whose `lib` holds the libraries: the
/// two dylibs plus an embedded rpath (their install names are `@rpath/...`
/// and they carry their own framework dependencies), or the static pair with
/// the frameworks MLX requires.
fn emit_prefix_link_flags(prefix: &Path) {
    let library_directory = prefix.join("lib");
    println!(
        "cargo:rustc-link-search=native={}",
        library_directory.display()
    );
    if library_directory.join("libmlxc.dylib").is_file() {
        println!("cargo:rustc-link-lib=dylib=mlxc");
        println!("cargo:rustc-link-lib=dylib=mlx");
        println!(
            "cargo:rustc-link-arg=-Wl,-rpath,{}",
            library_directory.display()
        );
    } else {
        println!("cargo:rustc-link-lib=static=mlxc");
        println!("cargo:rustc-link-lib=static=mlx");
        for framework in ["Metal", "Foundation", "QuartzCore", "Accelerate"] {
            println!("cargo:rustc-link-lib=framework={framework}");
        }
    }
}

/// The zero-configuration path: download the pinned MLX and MLX-C source
/// releases, verify their checksums, compile both with CMake, and install
/// into a prefix inside this build directory. The prefix persists across
/// incremental rebuilds (Cargo keeps the build directory until `cargo
/// clean`), so the native compilation happens once per profile.
fn fetch_and_build_pinned_pair() -> Option<PathBuf> {
    let output_directory =
        PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set for build scripts"));
    let native_directory = output_directory.join("native");
    let prefix = native_directory.join("prefix");
    if prefix_carries_mlxc_library(&prefix) {
        return Some(prefix);
    }
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
            return None;
        }
    }
    let _ = std::fs::create_dir_all(&native_directory);
    let mlx_source = download_verified_tarball(
        native_directory.join("mlx.tar.gz"),
        &format!(
            "{PINNED_TARBALL_BASE_URL}/ml-explore/mlx/archive/refs/tags/{PINNED_MLX_TAG}.tar.gz"
        ),
        PINNED_MLX_TARBALL_SHA256,
    )?;
    let mlx_c_source = download_verified_tarball(
        native_directory.join("mlx-c.tar.gz"),
        &format!(
            "{PINNED_TARBALL_BASE_URL}/ml-explore/mlx-c/archive/refs/tags/{PINNED_MLX_C_TAG}.tar.gz"
        ),
        PINNED_MLX_C_TARBALL_SHA256,
    )?;
    let mlx_directory = native_directory.join("mlx");
    let mlx_c_directory = native_directory.join("mlx-c");
    if !mlx_directory.is_dir() {
        run_step(
            "extract the pinned MLX source",
            &native_directory,
            Command::new("tar").arg("xzf").arg(&mlx_source),
        )?;
        let _ = std::fs::rename(
            native_directory.join(format!("mlx-{}", PINNED_MLX_TAG.trim_start_matches('v'))),
            &mlx_directory,
        );
    }
    if !mlx_c_directory.is_dir() {
        run_step(
            "extract the pinned MLX-C source",
            &native_directory,
            Command::new("tar").arg("xzf").arg(&mlx_c_source),
        )?;
        let _ = std::fs::rename(
            native_directory.join(format!(
                "mlx-c-{}",
                PINNED_MLX_C_TAG.trim_start_matches('v')
            )),
            &mlx_c_directory,
        );
    }
    let parallelism = env::var("NUM_JOBS").unwrap_or_else(|_| "4".to_owned());
    let mlx_build = native_directory.join("mlx-build");
    if !prefix.join("lib").join("libmlx.dylib").is_file() {
        run_step(
            "configure the pinned MLX build",
            &native_directory,
            Command::new("cmake")
                .arg("-S")
                .arg(&mlx_directory)
                .arg("-B")
                .arg(&mlx_build)
                .arg("-DCMAKE_BUILD_TYPE=Release")
                .arg("-DBUILD_SHARED_LIBS=ON")
                .arg("-DMLX_METAL_JIT=ON")
                .arg(format!("-DCMAKE_INSTALL_PREFIX={}", prefix.display())),
        )?;
        run_step(
            "compile the pinned MLX",
            &native_directory,
            Command::new("cmake")
                .arg("--build")
                .arg(&mlx_build)
                .arg("--parallel")
                .arg(&parallelism),
        )?;
        run_step(
            "install the pinned MLX",
            &native_directory,
            Command::new("cmake").arg("--install").arg(&mlx_build),
        )?;
    }
    let mlx_c_build = native_directory.join("mlx-c-build");
    if !prefix_carries_mlxc_library(&prefix) {
        run_step(
            "configure the pinned MLX-C build",
            &native_directory,
            Command::new("cmake")
                .arg("-S")
                .arg(&mlx_c_directory)
                .arg("-B")
                .arg(&mlx_c_build)
                .arg("-DCMAKE_BUILD_TYPE=Release")
                .arg("-DBUILD_SHARED_LIBS=ON")
                .arg("-DMLX_BUILD_EXAMPLES=OFF")
                .arg(format!("-DMLX_DIR={}", mlx_build.display()))
                .arg(format!("-DCMAKE_INSTALL_PREFIX={}", prefix.display())),
        )?;
        run_step(
            "compile the pinned MLX-C",
            &native_directory,
            Command::new("cmake")
                .arg("--build")
                .arg(&mlx_c_build)
                .arg("--parallel")
                .arg(&parallelism),
        )?;
        run_step(
            "install the pinned MLX-C",
            &native_directory,
            Command::new("cmake").arg("--install").arg(&mlx_c_build),
        )?;
    }
    if prefix_carries_mlxc_library(&prefix) {
        Some(prefix)
    } else {
        println!(
            "cargo:warning=the automatic MLX build finished but {} holds no MLX-C library",
            prefix.join("lib").display()
        );
        None
    }
}

/// Downloads `tarball_path` from `tarball_url` unless it is already present,
/// and verifies its SHA-256 against `expected_sha256` before use.
fn download_verified_tarball(
    tarball_path: PathBuf,
    tarball_url: &str,
    expected_sha256: &str,
) -> Option<PathBuf> {
    if !tarball_path.is_file() {
        run_step(
            "download the pinned MLX source release",
            tarball_path.parent()?,
            Command::new("curl")
                .arg("-sSL")
                .arg("-o")
                .arg(&tarball_path)
                .arg(tarball_url),
        )?;
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
    Some(tarball_path)
}

/// Runs one build step, mapping a failure to a warning that names the step.
fn run_step(step_description: &str, working_directory: &Path, command: &mut Command) -> Option<()> {
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
    if !command_status.success() {
        println!("cargo:warning=automatic MLX build failed to {step_description}");
        return None;
    }
    Some(())
}

/// Extracts the bridged `mlx_*` functions and types from the generated
/// bindings and emits them as public constant slices so the coverage contract
/// can compare the bridge against the vendored headers without re-running
/// bindgen.
fn write_bridged_inventory(
    bindings_content: &str,
    output_directory: &Path,
) -> Result<(), Box<dyn Error>> {
    let identifier_end = |segment: &str| {
        segment
            .find(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
            .unwrap_or(segment.len())
    };
    let mut bridged_functions: Vec<String> = bindings_content
        .split("pub fn ")
        .skip(1)
        .map(|segment| &segment[..identifier_end(segment)])
        .filter(|name| name.starts_with("mlx_"))
        .map(str::to_owned)
        .collect();
    bridged_functions.sort();
    bridged_functions.dedup();

    // bindgen 0.73 skips C variadic functions; the hand-written declaration
    // in `raw.rs` restores the one variadic MLX-C entry point.
    bridged_functions.push("mlx_error".to_owned());

    let mut bridged_types: Vec<String> = ["pub struct ", "pub enum ", "pub union ", "pub type "]
        .iter()
        .flat_map(|marker| {
            bindings_content
                .split(marker)
                .skip(1)
                .map(|segment| &segment[..identifier_end(segment)])
                .filter(|name| name.starts_with("mlx_"))
                .map(str::to_owned)
                .collect::<Vec<String>>()
        })
        .collect();
    // bindgen aliases constified C enums through `pub use self::mlx_x_ as
    // mlx_x;`; the alias is the name the headers expose, so it belongs in the
    // inventory alongside the tag.
    for alias_segment in bindings_content.split("pub use self::").skip(1) {
        let Some((_tag, alias_with_rest)) = alias_segment.split_once(" as ") else {
            continue;
        };
        let alias = &alias_with_rest[..identifier_end(alias_with_rest)];
        if alias.starts_with("mlx_") {
            bridged_types.push(alias.to_owned());
        }
    }
    bridged_types.sort();
    bridged_types.dedup();

    let inventory_source = format!(
        "// Generated by this crate's build script from the bindgen output.\n\
         // Every listed symbol exists in the compiled bridge; the coverage\n\
         // contract compares this inventory against the vendored headers.\n\n\
         /// Every MLX-C function bridged by the generated raw declarations.\n\
         pub const BRIDGED_FUNCTIONS: [&str; {}] = {:?};\n\n\
         /// Every MLX-C type bridged by the generated raw declarations.\n\
         pub const BRIDGED_TYPES: [&str; {}] = {:?};\n",
        bridged_functions.len(),
        bridged_functions,
        bridged_types.len(),
        bridged_types,
    );
    std::fs::write(
        output_directory.join(BRIDGED_INVENTORY_FILE_NAME),
        inventory_source,
    )?;
    Ok(())
}
