//! The COVERAGE_INVENTORY.md record legs: re-extract the recorded
//! surfaces (the vendored MLX-C function rows and the gap shim rows) and
//! compare them with the header trees they describe, so a pin bump, a
//! vendored-header edit, or a shim edit that forgets to refresh the
//! record fails loudly. Split from the upstream audit so each module
//! carries one comparison concern.

use std::collections::BTreeSet;
use std::path::Path;

use crate::surface_parsing::markdown_section;

/// The gap-shim record leg: the shim rows recorded in the Gap shim
/// functions section of COVERAGE_INVENTORY.md must exactly equal the shim
/// header's surface.
pub(crate) fn audit_gap_shim_record(
    repository_root: &Path,
    gap_shim_functions: &BTreeSet<String>,
) -> bool {
    let inventory_path = repository_root.join("COVERAGE_INVENTORY.md");
    let Ok(inventory_markdown) = std::fs::read_to_string(&inventory_path) else {
        eprintln!("[audit] cannot read COVERAGE_INVENTORY.md");
        return false;
    };
    let Some(shim_section) = markdown_section(&inventory_markdown, "## Gap shim functions") else {
        eprintln!("[audit] COVERAGE_INVENTORY.md has no ## Gap shim functions section");
        return false;
    };
    let recorded_shim_functions: BTreeSet<String> = shim_section
        .lines()
        .filter_map(|line| {
            let without_list_marker = line.trim_start().strip_prefix("| `")?;
            let shim_symbol = without_list_marker
                .split('`')
                .next()
                .unwrap_or(without_list_marker);
            shim_symbol
                .starts_with("mlxrs_")
                .then(|| shim_symbol.to_owned())
        })
        .collect();
    let missing_from_record: Vec<&String> = gap_shim_functions
        .difference(&recorded_shim_functions)
        .collect();
    let stale_in_record: Vec<&String> = recorded_shim_functions
        .difference(gap_shim_functions)
        .collect();
    println!(
        "[audit] COVERAGE_INVENTORY.md gap shim rows: {} (shim header surface: {})",
        recorded_shim_functions.len(),
        gap_shim_functions.len()
    );
    for shim_symbol in &missing_from_record {
        eprintln!(
            "[audit]   declared in the shim header but missing from the record: {shim_symbol}"
        );
    }
    for shim_symbol in &stale_in_record {
        eprintln!("[audit]   recorded but absent from the shim header: {shim_symbol}");
    }
    if missing_from_record.is_empty() && stale_in_record.is_empty() {
        println!("[audit] gap shim record leg: PASS (shim record matches the shim header)");
        true
    } else {
        eprintln!("[audit] gap shim record leg: FAIL — refresh COVERAGE_INVENTORY.md");
        false
    }
}

/// The inventory leg: the function rows recorded in the Functions section
/// of COVERAGE_INVENTORY.md must exactly equal the vendored header
/// surface. The Types section shares the row shape, so the slice runs from
/// the Functions heading to the next heading.
pub(crate) fn audit_coverage_inventory_record(
    repository_root: &Path,
    vendored_c_functions: &BTreeSet<String>,
) -> bool {
    let inventory_path = repository_root.join("COVERAGE_INVENTORY.md");
    let Ok(inventory_markdown) = std::fs::read_to_string(&inventory_path) else {
        eprintln!("[audit] cannot read COVERAGE_INVENTORY.md");
        return false;
    };
    let functions_section = markdown_section(&inventory_markdown, "## Functions");
    if functions_section.is_none() {
        eprintln!("[audit] COVERAGE_INVENTORY.md has no ## Functions section");
        return false;
    }
    let recorded_functions: BTreeSet<String> = functions_section
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let without_list_marker = line.trim_start().strip_prefix("| `")?;
            let function_name = without_list_marker
                .split('`')
                .next()
                .unwrap_or(without_list_marker);
            function_name
                .starts_with("mlx_")
                .then(|| function_name.to_owned())
        })
        .collect();
    let missing_from_record: Vec<&String> = vendored_c_functions
        .difference(&recorded_functions)
        .collect();
    let stale_in_record: Vec<&String> = recorded_functions
        .difference(vendored_c_functions)
        .collect();
    println!(
        "[audit] COVERAGE_INVENTORY.md function rows: {} (vendored surface: {})",
        recorded_functions.len(),
        vendored_c_functions.len()
    );
    for function_name in &missing_from_record {
        eprintln!(
            "[audit]   declared in vendored headers but missing from the record: {function_name}"
        );
    }
    for function_name in &stale_in_record {
        eprintln!("[audit]   recorded but absent from the vendored headers: {function_name}");
    }
    if missing_from_record.is_empty() && stale_in_record.is_empty() {
        println!("[audit] inventory leg: PASS (function record matches the vendored surface)");
        true
    } else {
        eprintln!("[audit] inventory leg: FAIL — refresh COVERAGE_INVENTORY.md");
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_flag_inventory_drift_in_both_directions() {
        let vendored_surface: BTreeSet<String> = [
            "mlx_present_everywhere".to_owned(),
            "mlx_only_in_headers".to_owned(),
        ]
        .into_iter()
        .collect();
        let recorded_markdown = "\
# record

## Functions

| upstream symbol | bridge state |
|---|---|
| `mlx_present_everywhere` | bridged |
| `mlx_only_in_record` | bridged |

## Types

| upstream symbol | bridge state |
|---|---|
| `mlx_array` | bridged |
";
        let functions_section =
            markdown_section(recorded_markdown, "## Functions").expect("section exists");
        let recorded_functions: BTreeSet<String> = functions_section
            .lines()
            .filter_map(|line| {
                let without_marker = line.trim_start().strip_prefix("| `")?;
                let name = without_marker.split('`').next()?;
                name.starts_with("mlx_").then(|| name.to_owned())
            })
            .collect();
        // The Types row `mlx_array` must not leak into the function record.
        assert!(!recorded_functions.contains("mlx_array"));
        assert_eq!(
            vendored_surface
                .difference(&recorded_functions)
                .collect::<Vec<_>>(),
            [&"mlx_only_in_headers".to_owned()]
        );
        assert_eq!(
            recorded_functions
                .difference(&vendored_surface)
                .collect::<Vec<_>>(),
            [&"mlx_only_in_record".to_owned()]
        );
    }
}
