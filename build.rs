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
//! consumer reuses it. Non-mac hosts never enter the ladder at all: the
//! link line it emits is Apple-framework-shaped end to end, so those hosts
//! get a warning instead and compile or document without linking.

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

/// The freestanding stubs that replace host libc in the self-contained
/// parse; see `self_contained_parse_arguments`.
const FREESTANDING_HEADERS_DIRECTORY: &str = "vendor/freestanding";

/// Clang arguments giving bindgen ONE parse identity on every host: the
/// aarch64 Apple-darwin view of the ABI this crate binds, in a
/// self-contained freestanding environment. Three distinct fatalities
/// surfaced while proving the Linux lane — each a different symptom of
/// letting host state describe the parse. First, the host libclang missed
/// the Debian/Ubuntu multiarch include directory and the host `<stdint.h>`
/// died on `bits/libc-header-start.h`. Second, the default triple did not
/// match the 64-bit host, so glibc's `gnu/stubs.h` selected the 32-bit
/// stubs. Third — the deepest, and invisible until the first two were
/// fixed — the vendored `half.h` gates the float16 and bfloat16 slice of
/// the ABI on `__aarch64__`, so any x86_64 parse silently DROPS symbols
/// like `mlx_array_data_bfloat16` that the bridge promises to carry; and
/// conversely an aarch64 parse of an x86_64 host's glibc is incoherent,
/// because that glibc's `stubs.h` speaks only the x86 family. The
/// conclusion is structural: no host header set can serve this parse.
/// `--target` names the Apple silicon ABI view; `-nostdsysteminc` drops
/// every host system include path while KEEPING clang's builtin headers
/// (`-nostdinc` would drop those too — float.h, stddef.h, and the ARM
/// intrinsic headers the vendored gates include); the freestanding stubs
/// vendored beside the headers supply the few libc-owned pieces they
/// touch (fixed-width integers, an opaque FILE, size_t via stddef, an
/// empty complex). docs.rs, the Ubuntu lint lane, and every Mac therefore
/// generate byte-identical declarations, and the bindings stop embedding
/// host FILE layouts nothing ever used.
fn self_contained_parse_arguments(freestanding_directory: &Path) -> Vec<String> {
    vec![
        "--target=aarch64-apple-darwin".to_owned(),
        // A frontend-only flag, so it rides behind -Xclang; -nostdinc would
        // also drop clang's builtin headers (float.h, stddef.h, the ARM
        // intrinsic headers), which the parse very much keeps.
        "-Xclang".to_owned(),
        "-nostdsysteminc".to_owned(),
        format!("-isystem{}", freestanding_directory.display()),
    ]
}

fn main() -> Result<(), Box<dyn Error>> {
    let manifest_directory =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").ok_or("CARGO_MANIFEST_DIR is unset")?);
    let output_directory = PathBuf::from(env::var_os("OUT_DIR").ok_or("OUT_DIR is unset")?);
    let include_directory = manifest_directory.join(VENDORED_HEADERS_DIRECTORY);
    let freestanding_directory = manifest_directory.join(FREESTANDING_HEADERS_DIRECTORY);
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
        .clang_args(self_contained_parse_arguments(&freestanding_directory))
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

/// Emits the linker instructions for an MLX-C library. Non-mac hosts stop
/// at a warning before any rung: the link line this ladder produces — a
/// `libmlxc.dylib` plus Apple frameworks — could never work there, so even
/// an explicit prefix variable would only yield unusable flags. On macOS,
/// the ladder, first match wins: the prefix variable, the machine-shared
/// identity-keyed prefix (building it if absent — the zero-configuration
/// default), Homebrew, and pkg-config. When nothing resolves, a warning is
/// printed and no link flags are emitted: library compilation and
/// documentation builds still work, and the failure surfaces only when a
/// consumer links a binary.
fn emit_link_instructions() {
    if !cfg!(target_os = "macos") {
        // The pinned pair compiles MLX, which needs Apple's Metal stack: a
        // non-mac host can never provide it, and the dylib-plus-frameworks
        // link line below has no meaning there — including for an explicit
        // prefix variable. Documentation and check lanes never link, so the
        // ladder stops here with a warning instead of burning build minutes
        // (docs.rs builds are time-boxed) on a doomed CMake attempt.
        // Linking consumers are on Apple silicon by definition.
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
