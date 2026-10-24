//! The upstream surface audit: prove which MLX functions the vendored
//! MLX-C tree actually carries, so a capability MLX ships cannot silently
//! lack a Rust path into this crate.
//!
//! The audit has two legs:
//!
//! 1. The upstream leg (needs `--mlx-source <checkout>`): parse the public
//!    C++ free functions out of the pinned MLX checkout's umbrella header
//!    (`mlx/mlx.h` — the reviewed definition of MLX's public surface), map
//!    each C++ name onto the C naming convention MLX-C follows, and report
//!    every MLX function with no C wrapper. Those functions are the only
//!    place coverage can be lost upstream of this repository: the build
//!    script's bindgen translation is mechanical and full-tree, so
//!    anything MLX-C declares is necessarily bridged here.
//!
//! 2. The inventory leg (always runs): re-extract the vendored MLX-C C
//!    function surface from the vendored umbrella header (following local
//!    includes transitively, the same closure bindgen parses) and compare
//!    it with the function rows recorded in `COVERAGE_INVENTORY.md`, so a
//!    pin bump or vendored-header edit that forgets to refresh the record
//!    fails loudly. Types stay the build script's inventory domain: they
//!    are emitted per compile from the bindgen output, where the type
//!    extraction is exact rather than regex-approximate.
//!
//! Matching is deliberately layered: an exact family-prefixed name first
//! (`mlx::core::linalg::norm` → `mlx_linalg_norm`), then the bare prefix
//! (`fft::fftn` also tries `mlx_fftn`), then a suffix match for names the
//! C side composes differently (`cuda_kernel` family under `mlx_fast_`).
//! Everything still unmatched lands in the report against the reviewed
//! allowlist below, which records why each surviving gap is accepted.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::inventory_records::{audit_coverage_inventory_record, audit_gap_shim_record};
use crate::repository;
use crate::surface_parsing::{
    cpp_public_free_function_names, function_names_in_c_source, function_names_with_prefix,
    quoted_includes,
};

/// MLX public functions that MLX-C verifiably does not wrap, kept reviewed
/// so the audit fails only on NEW gaps. Every entry pairs the C++ function
/// name with the reason the gap is accepted. Verified against the pinned
/// pair MLX v0.32.4 + MLX-C v0.7.0 (the vendored header tree).
const REVIEWED_UNWRAPPED_MLX_FUNCTIONS: &[(&str, &str)] = &[
    (
        "broadcast_shapes",
        "shape-algebra helper on std::vector<int>; no wrapper in the MLX-C v0.7.0 pin",
    ),
    (
        "clear_backends",
        "distributed backend teardown; no wrapper in the MLX-C v0.7.0 pin",
    ),
    (
        "einsum_path",
        "einsum path-search API; no wrapper in the MLX-C v0.7.0 pin",
    ),
    (
        "gated_delta_update",
        "fast gated-delta op; no wrapper in the MLX-C v0.7.0 pin",
    ),
    (
        "get_array_buffer_size",
        "memory introspection; no wrapper in the MLX-C v0.7.0 pin",
    ),
    (
        "get_global_formatter",
        "iostream printing configuration; the C API carries no printing surface",
    ),
    (
        "exporter",
        "served by the mlx_function_exporter_* family under the exporter-suffix naming",
    ),
    (
        "import_function",
        "served by the mlx_imported_function_* family under the imported naming",
    ),
    (
        "istft",
        "short-time Fourier transform; no wrapper in the MLX-C v0.7.0 pin",
    ),
    (
        "peek_default_stream",
        "default-stream accessor; no wrapper in the MLX-C v0.7.0 pin",
    ),
    (
        "precompiled_cuda_kernel",
        "precompiled cuda-kernel construction; no wrapper in the MLX-C v0.7.0 pin",
    ),
    (
        "result_type",
        "type-promotion helper; no wrapper in the MLX-C v0.7.0 pin",
    ),
    (
        "normalize_axis_index",
        "axis-normalization helper; no wrapper in the MLX-C v0.7.0 pin",
    ),
    (
        "set_printoptions",
        "iostream printing configuration; the C API carries no printing surface",
    ),
    (
        "stft",
        "short-time Fourier transform; no wrapper in the MLX-C v0.7.0 pin",
    ),
    (
        "to_stream",
        "converts the C++ StreamOrDevice variant, which the C API does not model",
    ),
    (
        "vmap",
        "no direct C wrapper in the MLX-C v0.7.0 pin: the transform is reachable \
         only through the detail machinery (mlx_detail_vmap_trace/_replace) and the \
         optional fun_vmap closure of mlx_custom_function; revisit when an MLX-C \
         release pairs with this MLX",
    ),
];

/// The C-name prefixes tried for each MLX header, in order. The header file
/// stem names the MLX family; most families mirror their namespace in the
/// C prefix, and the bare `mlx_` fallback catches the members the C side
/// hoisted into the top namespace (for example `mlx_async_eval`).
fn candidate_prefixes_for_header(header_relative_path: &str) -> &'static [&'static str] {
    // MLX-side paths carry the mlx/ prefix ("mlx/distributed/ops.h");
    // routing cares about the path inside the mlx namespace.
    let header_relative_path = header_relative_path.trim_start_matches("mlx/");
    let header_file_stem = header_relative_path
        .rsplit('/')
        .next()
        .unwrap_or(header_relative_path)
        .trim_end_matches(".h");
    match header_file_stem {
        "linalg" => &["mlx_linalg_", "mlx_"],
        "fft" => &["mlx_fft_", "mlx_"],
        "fast" => &["mlx_fast_", "mlx_"],
        "random" => &["mlx_random_", "mlx_"],
        // The backend headers carry their C families under their own
        // prefix; the fast fallback exists because MLX-C's fast.h also
        // exposes cuda-kernel entry points as mlx_fast_cuda_kernel_*.
        "metal" => &["mlx_metal_", "mlx_"],
        "cuda" => &["mlx_cuda_", "mlx_fast_", "mlx_"],
        "distributed" | "ops" if header_relative_path.starts_with("distributed/") => {
            &["mlx_distributed_", "mlx_"]
        }
        _ => &["mlx_"],
    }
}

/// One parsed MLX public function: the C++ name and the vendored-relative
/// header it was declared in (the header drives the C-name candidates).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct MlxPublicFunction {
    header_relative_path: String,
    function_name: String,
}

pub fn run(arguments: &[String]) -> ExitCode {
    let mut mlx_source_path: Option<PathBuf> = None;
    let mut argument_index = 0;
    while argument_index < arguments.len() {
        let argument = &arguments[argument_index];
        if argument == "--mlx-source" && argument_index + 1 < arguments.len() {
            mlx_source_path = Some(PathBuf::from(&arguments[argument_index + 1]));
            argument_index += 2;
        } else if argument == "--mlx-source" {
            eprintln!("audit-upstream-surface: --mlx-source needs a path to an MLX checkout");
            print_usage();
            return ExitCode::FAILURE;
        } else if let Some(path_tail) = argument.strip_prefix("--mlx-source=") {
            mlx_source_path = Some(PathBuf::from(path_tail));
            argument_index += 1;
        } else {
            eprintln!("audit-upstream-surface: unknown argument {argument:?}");
            print_usage();
            return ExitCode::FAILURE;
        }
    }

    let repository_root = repository::repository_root();
    let vendored_c_functions = extract_vendored_c_functions(&repository_root);
    if vendored_c_functions.is_empty() {
        eprintln!("audit-upstream-surface: no vendored MLX-C functions found under vendor/");
        return ExitCode::FAILURE;
    }
    println!(
        "[audit] vendored MLX-C surface: {} functions",
        vendored_c_functions.len()
    );

    let inventory_in_sync =
        audit_coverage_inventory_record(&repository_root, &vendored_c_functions);
    let gap_shim_functions = extract_gap_shim_functions(&repository_root);
    if gap_shim_functions.is_empty() {
        eprintln!("audit-upstream-surface: no gap shim functions found under native_gap_shim/");
        return ExitCode::FAILURE;
    }
    println!(
        "[audit] gap shim surface: {} functions",
        gap_shim_functions.len()
    );
    let shim_record_in_sync = audit_gap_shim_record(&repository_root, &gap_shim_functions);

    let upstream_audit_passed = match mlx_source_path {
        Some(checkout_path) => {
            audit_upstream_surface(&checkout_path, &vendored_c_functions, &gap_shim_functions)
        }
        None => {
            println!("[audit] upstream leg skipped: pass --mlx-source <mlx checkout> to run it");
            true
        }
    };

    if inventory_in_sync && shim_record_in_sync && upstream_audit_passed {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn print_usage() {
    eprintln!(
        "usage: cargo xtask audit-upstream-surface [--mlx-source <path-to-mlx-checkout>]\n\n\
         Audits that no MLX public function lacks a C wrapper in the vendored\n\
         MLX-C tree (the only layer where coverage can be lost upstream), and\n\
         that COVERAGE_INVENTORY.md's function record matches the vendored\n\
         headers. The upstream leg needs a checkout of the pinned MLX tag\n\
         (PINNED_MLX_TAG in native_build.rs); the inventory leg always runs."
    );
}

/// Extract every `mlx_*` function the vendored umbrella header tree
/// declares. Follows quoted includes transitively from
/// `vendor/mlx-c/mlx/c/mlx.h` — the same closure the build script hands to
/// bindgen — so the extraction is comparable with the recorded inventory.
fn extract_vendored_c_functions(repository_root: &Path) -> BTreeSet<String> {
    let include_root = repository_root.join("vendor/mlx-c");
    let umbrella_path = include_root.join("mlx/c/mlx.h");
    let mut visited_headers: BTreeSet<PathBuf> = BTreeSet::new();
    let mut declared_functions: BTreeSet<String> = BTreeSet::new();
    visit_vendored_header(
        &umbrella_path,
        &include_root,
        &mut visited_headers,
        &mut declared_functions,
    );
    declared_functions
}

fn visit_vendored_header(
    header_path: &Path,
    include_root: &Path,
    visited_headers: &mut BTreeSet<PathBuf>,
    declared_functions: &mut BTreeSet<String>,
) {
    if !visited_headers.insert(header_path.to_path_buf()) {
        return;
    }
    let header_source = match std::fs::read_to_string(header_path) {
        Ok(source) => source,
        Err(read_error) => {
            eprintln!(
                "[audit] cannot read vendored header {}: {read_error}",
                header_path.display()
            );
            return;
        }
    };
    for function_name in function_names_in_c_source(&header_source) {
        declared_functions.insert(function_name);
    }
    for included_relative_path in quoted_includes(&header_source) {
        // The vendored tree's quoted includes are include-root relative
        // (they all read "mlx/c/…", matching the build script's single -I
        // on vendor/mlx-c); the including file's own directory is kept as
        // a secondary resolution so the walker survives a tree that ever
        // mixes both styles.
        let root_resolved = include_root.join(&included_relative_path);
        let included_path = if root_resolved.is_file() {
            root_resolved
        } else {
            header_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(&included_relative_path)
        };
        visit_vendored_header(
            &included_path,
            include_root,
            visited_headers,
            declared_functions,
        );
    }
}

/// Parse the MLX checkout's public C++ free functions, starting from the
/// umbrella header `mlx/mlx.h` (the reviewed definition of the public
/// surface — the one header MLX keeps as the complete public include set).
fn extract_mlx_public_functions(mlx_checkout_root: &Path) -> BTreeSet<MlxPublicFunction> {
    let umbrella_path = mlx_checkout_root.join("mlx/mlx.h");
    let Some(umbrella_source) = std::fs::read_to_string(&umbrella_path).ok() else {
        return BTreeSet::new();
    };
    let mut public_functions = BTreeSet::new();
    for included_relative_path in quoted_includes(&umbrella_source) {
        let header_path = mlx_checkout_root.join(&included_relative_path);
        let Some(header_source) = std::fs::read_to_string(&header_path).ok() else {
            continue;
        };
        for function_name in cpp_public_free_function_names(&header_source) {
            public_functions.insert(MlxPublicFunction {
                header_relative_path: included_relative_path.clone(),
                function_name,
            });
        }
    }
    public_functions
}

/// The composition layer: MLX-C sometimes re-composes a C++ name's words
/// in a different order (`new_stream` → `mlx_stream_new`,
/// `device_info` → `mlx_device_info_new`). A multi-word C++ name counts
/// as wrapped when some C symbol's underscore segments contain every
/// segment of the C++ name. Single-word names are deliberately excluded:
/// a lone word (`eval`) must never fuzzy-match a composed C name
/// (`mlx_async_eval`), which would silently mask a real gap.
fn is_wrapped_by_composition(function_name: &str, vendored_c_functions: &BTreeSet<String>) -> bool {
    let required_segments: Vec<&str> = function_name.split('_').collect();
    if required_segments.len() < 2 {
        return false;
    }
    vendored_c_functions.iter().any(|c_function_name| {
        let c_segments: std::collections::BTreeSet<&str> = c_function_name.split('_').collect();
        required_segments
            .iter()
            .all(|required_segment| c_segments.contains(required_segment))
    })
}

/// The repository's gap-shim declarations: the mlxrs_* functions the
/// native engine compiles as the third component, read straight from the
/// shim's own header (the single source of truth for its surface).
fn extract_gap_shim_functions(repository_root: &Path) -> BTreeSet<String> {
    let shim_header_path = repository_root.join("native_gap_shim/include/mlxrs/gap_shim.h");
    let Ok(shim_header_source) = std::fs::read_to_string(&shim_header_path) else {
        return BTreeSet::new();
    };
    function_names_with_prefix(&shim_header_source, "mlxrs_")
        .into_iter()
        .collect()
}

/// The shim entry serving one MLX function, if any: an exact
/// `mlxrs_<name>` match or a shim entry the repository named by extending
/// the C++ name (the C++ `minmax` overloads carry the axes/axis suffix the
/// C ABI needs as separate entry points).
fn gap_shim_entry_for(
    function_name: &str,
    gap_shim_functions: &BTreeSet<String>,
) -> Option<String> {
    let exact_shim_symbol = format!("mlxrs_{function_name}");
    if gap_shim_functions.contains(&exact_shim_symbol) {
        return Some(exact_shim_symbol);
    }
    let extended_prefix = format!("mlxrs_{function_name}_");
    gap_shim_functions
        .iter()
        .find(|shim_symbol| shim_symbol.starts_with(&extended_prefix))
        .cloned()
}

/// The upstream leg: map every MLX public function onto the vendored C
/// surface plus the repository's gap shim, and report the ones with no
/// wrapper anywhere.
fn audit_upstream_surface(
    mlx_checkout_root: &Path,
    vendored_c_functions: &BTreeSet<String>,
    gap_shim_functions: &BTreeSet<String>,
) -> bool {
    let public_functions = extract_mlx_public_functions(mlx_checkout_root);
    if public_functions.is_empty() {
        eprintln!(
            "[audit] no MLX public functions parsed from {} — is this a \
             checkout of the MLX source tree?",
            mlx_checkout_root.display()
        );
        return false;
    }

    let mut unwrapped: BTreeMap<String, Vec<&MlxPublicFunction>> = BTreeMap::new();
    let mut exact_match_count = 0usize;
    let mut composition_match_count = 0usize;
    let mut shim_wrap_count = 0usize;
    let mut redundant_shim_count = 0usize;
    for public_function in &public_functions {
        let candidate_prefixes =
            candidate_prefixes_for_header(&public_function.header_relative_path);
        let exact_candidate = candidate_prefixes
            .iter()
            .map(|prefix| format!("{prefix}{}", public_function.function_name))
            .find(|candidate| vendored_c_functions.contains(candidate.as_str()));
        let wrapped_by_mlxc_composition =
            is_wrapped_by_composition(&public_function.function_name, vendored_c_functions);
        let shim_entry = gap_shim_entry_for(&public_function.function_name, gap_shim_functions);
        if exact_candidate.is_some() || wrapped_by_mlxc_composition {
            if exact_candidate.is_some() {
                exact_match_count += 1;
            } else {
                composition_match_count += 1;
            }
            // The retirement governor: MLX-C now carries this function, so
            // a shim entry for it is redundant surface to retire.
            if let Some(redundant_shim_symbol) = shim_entry {
                redundant_shim_count += 1;
                eprintln!(
                    "[audit]   REDUNDANT SHIM: {redundant_shim_symbol} — MLX-C now \
                     wraps {}; retire the shim entry and move the wrapper to \
                     the MLX-C family",
                    public_function.function_name
                );
            }
            continue;
        }
        if shim_entry.is_some() {
            shim_wrap_count += 1;
            continue;
        }
        unwrapped
            .entry(public_function.function_name.clone())
            .or_default()
            .push(public_function);
    }

    println!(
        "[audit] MLX public free functions: {} across {} headers",
        public_functions.len(),
        public_functions
            .iter()
            .map(|function| function.header_relative_path.as_str())
            .collect::<BTreeSet<_>>()
            .len()
    );
    println!(
        "[audit] wrapped: {} by MLX-C ({} exact, {} composition), {} by the gap shim",
        exact_match_count + composition_match_count,
        exact_match_count,
        composition_match_count,
        shim_wrap_count
    );

    let mut new_gap_count = 0usize;
    let mut reviewed_gap_count = 0usize;
    let mut sorted_unwrapped: Vec<(&String, &Vec<&MlxPublicFunction>)> = unwrapped.iter().collect();
    sorted_unwrapped.sort_by(|left, right| left.0.cmp(right.0));
    for (function_name, declarations) in &sorted_unwrapped {
        match REVIEWED_UNWRAPPED_MLX_FUNCTIONS
            .iter()
            .find(|(reviewed_name, _)| reviewed_name == function_name)
        {
            Some(_) => reviewed_gap_count += 1,
            None => new_gap_count += declarations.len(),
        }
    }
    println!(
        "[audit] unwrapped upstream functions: {} ({} reviewed, {} NEW)",
        sorted_unwrapped.len(),
        reviewed_gap_count,
        new_gap_count
    );
    for (function_name, declarations) in &sorted_unwrapped {
        match REVIEWED_UNWRAPPED_MLX_FUNCTIONS
            .iter()
            .find(|(reviewed_name, _)| reviewed_name == function_name)
        {
            Some((_, rationale)) => {
                for declaration in declarations.iter() {
                    println!(
                        "[audit]   reviewed: {function_name} ({}) — {rationale}",
                        declaration.header_relative_path
                    );
                }
            }
            None => {
                for declaration in declarations.iter() {
                    println!(
                        "[audit]   NEW GAP: {function_name} ({})",
                        declaration.header_relative_path
                    );
                }
            }
        }
    }
    if new_gap_count == 0 && redundant_shim_count == 0 {
        println!(
            "[audit] upstream leg: PASS (every MLX public function is wrapped \
             by MLX-C, wrapped by the gap shim, or reviewed)"
        );
        true
    } else {
        if new_gap_count > 0 {
            eprintln!(
                "[audit] upstream leg: FAIL — {new_gap_count} new unwrapped MLX \
                 functions; wrap them upstream in MLX-C, add a gap shim entry, \
                 or add a reviewed allowlist entry"
            );
        }
        if redundant_shim_count > 0 {
            eprintln!(
                "[audit] upstream leg: FAIL — {redundant_shim_count} redundant \
                 shim entries; retire them now that MLX-C wraps their functions"
            );
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_match_shim_entries_exactly_and_by_extension() {
        let shim_surface: BTreeSet<String> = [
            "mlxrs_matrix_transpose".to_owned(),
            "mlxrs_unique".to_owned(),
            "mlxrs_minmax_axes".to_owned(),
            "mlxrs_minmax_axis".to_owned(),
        ]
        .into_iter()
        .collect();
        // Exact shim names match their C++ function...
        assert_eq!(
            gap_shim_entry_for("matrix_transpose", &shim_surface),
            Some("mlxrs_matrix_transpose".to_owned())
        );
        // ...and a C++ overload whose C entry points extend the name (the
        // minmax axes/axis variants) matches through the extension rule.
        assert!(
            gap_shim_entry_for("minmax", &shim_surface)
                .is_some_and(|shim_symbol| shim_symbol.starts_with("mlxrs_minmax_"))
        );
        assert_eq!(gap_shim_entry_for("vmap", &shim_surface), None);
    }

    #[test]
    fn should_apply_family_prefixes_and_composition_fallback() {
        let vendored_surface: BTreeSet<String> = [
            "mlx_linalg_norm",
            "mlx_fft_fftn",
            "mlx_abs",
            "mlx_stream_new",
            "mlx_device_info_new",
            "mlx_fast_metal_kernel_new",
            "mlx_async_eval",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        assert_eq!(
            candidate_prefixes_for_header("mlx/linalg.h"),
            &["mlx_linalg_", "mlx_"] as &[&str]
        );
        // Exact family names win first.
        assert!(vendored_surface.contains("mlx_linalg_norm"));
        // Composition matches reordered multi-word names...
        assert!(is_wrapped_by_composition("new_stream", &vendored_surface));
        assert!(is_wrapped_by_composition("device_info", &vendored_surface));
        assert!(is_wrapped_by_composition("metal_kernel", &vendored_surface));
        // ...but a single-word name must never fuzzy-match: eval would
        // otherwise silently hide behind mlx_async_eval.
        assert!(!is_wrapped_by_composition("eval", &vendored_surface));
        assert!(!is_wrapped_by_composition("unique", &vendored_surface));
    }
}
