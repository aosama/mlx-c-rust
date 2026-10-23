//! Standalone bindgen generation for the public MLX-C API, from the headers
//! vendored under `vendor/mlx-c/`.
//!
//! This build script translates the vendored headers of the pinned MLX-C
//! release into Rust declarations and emits the bridged-symbol inventory that
//! the coverage contract compares against those same headers. It never builds
//! native code and never touches the network: the vendor directory is the
//! single source of truth for the bound surface, so a pin bump is one vendored
//! tree swap away and drift shows up as a compile or contract failure.
//!
//! Linking a binary that uses this crate requires an MLX-C library. Discovery
//! order: the `MLX_C_RUST_MLX_C_PREFIX` environment variable (a prefix with
//! `lib/`, optionally `lib/pkgconfig/`), then `pkg-config mlx-c`. When neither
//! resolves, the crate still checks and builds documentation — only linking a
//! consumer binary reports undefined symbols, with a warning printed here.

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

/// Emits the linker instructions for an MLX-C library found through the
/// prefix variable or pkg-config. When nothing is found, a warning is printed
/// and no link flags are emitted: library compilation and documentation builds
/// still work, and the failure surfaces only when a consumer links a binary.
fn emit_link_instructions() {
    if let Some(prefix) = env::var_os(MLX_C_PREFIX_VARIABLE) {
        let prefix = PathBuf::from(prefix);
        // Route the prefix through pkg-config so the upstream mlx-c.pc — and
        // its transitive MLX and framework flags — drives the exact link line.
        for pkgconfig_directory in ["lib/pkgconfig", "lib"] {
            let candidate = prefix.join(pkgconfig_directory);
            if candidate.join("mlx-c.pc").is_file() {
                // SAFETY: single-threaded build script; extending a process
                // environment variable before any probe reads it is race-free.
                unsafe {
                    env::set_var(
                        "PKG_CONFIG_PATH",
                        env::join_paths(
                            env::split_paths(&env::var_os("PKG_CONFIG_PATH").unwrap_or_default())
                                .chain(std::iter::once(candidate)),
                        )
                        .expect("PKG_CONFIG_PATH pieces must be joinable"),
                    );
                }
                break;
            }
        }
    }
    match PkgConfig::new().atleast_version("0.7.0").probe("mlx-c") {
        Ok(mlx_c) => {
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
        }
        Err(probe_error) => {
            println!(
                "cargo:warning=MLX-C was not found via {MLX_C_PREFIX_VARIABLE} or pkg-config \
                 ({probe_error}); the crate compiles, but linking a binary that uses it \
                 requires an MLX-C library. The vendored headers in {VENDORED_HEADERS_DIRECTORY} \
                 document the bound API version."
            );
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
