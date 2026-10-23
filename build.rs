//! Standalone bindgen generation for the public MLX-C API, from the headers
//! vendored under `vendor/mlx-c/`, plus library discovery for the pinned
//! native pair.
//!
//! This build script translates the vendored headers of the pinned MLX-C
//! release into Rust declarations and emits the bridged-symbol inventory that
//! the coverage contract compares against those same headers.
//!
//! Linking a binary that uses this crate needs an MLX-C library. Discovery
//! order, first match wins: the `MLX_C_RUST_MLX_C_PREFIX` environment
//! variable, the machine-shared identity-keyed prefix that `cargo xtask
//! build-native` (or a previous build of any consumer) compiled, the
//! standard Homebrew prefixes, `pkg-config mlx-c`, and — the zero-
//! configuration default — the typed native-build engine in `native_build.rs`
//! (this crate's own code, no shell), which downloads the checksum-verified
//! pinned sources and compiles them once into that same shared prefix. The
//! pair therefore compiles once per machine per pin, and every later
//! consumer reuses it.

#[path = "native_build.rs"]
mod native_build;

use std::{
    env,
    error::Error,
    path::{Path, PathBuf},
};

use pkg_config::Config as PkgConfig;

const VENDORED_HEADERS_DIRECTORY: &str = "vendor/mlx-c";
const MLX_C_PREFIX_VARIABLE: &str = "MLX_C_RUST_MLX_C_PREFIX";

const BINDGEN_FUNCTION_ALLOWLIST: &str = "mlx_.*";
const BINDGEN_TYPE_ALLOWLIST: &str = "mlx_.*";

/// The generated inventory module consumed by `raw.rs`; it exposes the exact
/// bridged symbol sets the coverage contract compares against the vendored
/// headers.
const BRIDGED_INVENTORY_FILE_NAME: &str = "bridged_inventory.rs";

/// Clang arguments that make bindgen's vendored-header parse independent of
/// whichever libclang won the loading search on a Linux host. The evidence
/// from hosted Linux runners — docs.rs behind them — is that the loaded
/// libclang can carry a default target triple that does not match the
/// 64-bit host, and one misload surfaces as two different fatalities.
/// First: the distribution's multiarch include directory
/// (`/usr/include/x86_64-linux-gnu`) never enters the search path, so the
/// host `<stdint.h>` dies on `bits/libc-header-start.h`. Supplying that
/// directory exposes the second surface: with the host architecture macro
/// undefined, glibc's `gnu/stubs.h` selects the 32-bit stubs and dies on
/// `gnu/stubs-32.h`. Both trace to the same root, so the fix names the
/// parse explicitly: `--target=<host-triplet>` restores the architecture
/// macros (and clang's own Linux path logic where implemented), while the
/// explicit `-I` guarantees the multiarch directory regardless. The
/// directory existence check doubles as a Debian-layout detector; hosts
/// without it — macOS and musl systems — keep the default parse, where it
/// is already correct.
fn linux_host_clang_arguments() -> Vec<String> {
    let host_multiarch_include_directory =
        format!("/usr/include/{}-linux-gnu", std::env::consts::ARCH);
    if !Path::new(&host_multiarch_include_directory).is_dir() {
        return Vec::new();
    }
    vec![
        format!("--target={}-unknown-linux-gnu", std::env::consts::ARCH),
        format!("-I{host_multiarch_include_directory}"),
    ]
}

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
        .clang_args(linux_host_clang_arguments())
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
/// match wins: the prefix variable, the machine-shared identity-keyed prefix
/// (building it if absent — the zero-configuration default), Homebrew, and
/// pkg-config. When nothing resolves, a warning is printed and no link flags
/// are emitted: library compilation and documentation builds still work, and
/// the failure surfaces only when a consumer links a binary.
fn emit_link_instructions() {
    if !cfg!(target_os = "macos") {
        // The pinned pair compiles MLX, which needs Apple's Metal stack: a
        // non-mac host can never provide it. Documentation and check lanes
        // never link, so the ladder stops here with a warning instead of
        // burning build minutes (docs.rs builds are time-boxed) on a doomed
        // CMake attempt. Linking consumers are on Apple silicon by
        // definition.
        println!(
            "cargo:warning=no MLX-C library can be built on this host operating system; \
             the crate compiles and documents, but linking a binary that uses it needs \
             an Apple silicon Mac"
        );
        return;
    }
    if let Some(prefix) = prefix_from_environment() {
        emit_prefix_link_flags(&prefix);
        return;
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

    // Nothing located: the zero-configuration cold path. The typed engine in
    // `native_build.rs` compiles the pinned pair into the machine-shared
    // prefix — the same directory `cargo xtask build-native` maintains — so
    // this compilation is shared with every other consumer on the machine.
    match native_build::ensure_pinned_pair() {
        native_build::NativeBuildOutcome::Built | native_build::NativeBuildOutcome::Reused => {
            if let Some(prefix) = native_build::shared_prefix()
                .filter(|candidate| native_build::prefix_carries_mlxc_library(candidate))
            {
                emit_prefix_link_flags(&prefix);
                return;
            }
            println!(
                "cargo:warning=the native build reported success but the shared prefix \
                 holds no MLX-C library; continuing without link flags"
            );
        }
        native_build::NativeBuildOutcome::Failed => println!(
            "cargo:warning=no MLX-C library could be located or built; the crate compiles, \
             but linking a binary that uses it will report undefined MLX symbols. Set \
             {MLX_C_PREFIX_VARIABLE}, run `cargo xtask build-native`, install mlx-c \
             through Homebrew, or install CMake (brew install cmake) so the pinned \
             sources can be built automatically."
        ),
    }
}

/// The explicit prefix override, validated before use.
fn prefix_from_environment() -> Option<PathBuf> {
    let prefix = PathBuf::from(env::var_os(MLX_C_PREFIX_VARIABLE)?);
    if native_build::prefix_carries_mlxc_library(&prefix) {
        return Some(prefix);
    }
    println!(
        "cargo:warning=MLX_C_RUST_MLX_C_PREFIX is set but {} holds no MLX-C library; \
         continuing down the discovery ladder",
        prefix.join("lib").display()
    );
    None
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
        .find(|prefix| native_build::prefix_carries_mlxc_library(prefix))
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
