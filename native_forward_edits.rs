//! The forward edit set: reviewed adaptations of the pinned MLX-C sources
//! to the pinned MLX, applied by the native build engine between extraction
//! and configure. This module is deliberately self-contained — std only —
//! so the shared engine (`native_build.rs`), the build script, xtask, and
//! the test tree can all include it directly without dragging engine
//! dependencies along.

/// One reviewed forward adaptation of the pinned MLX-C sources to the
/// pinned MLX, carried until an MLX-C release pairs with that MLX (see
/// `drop_with`). C++-side only: the C surface this crate binds is
/// untouched, and COVERAGE_INVENTORY.md's pairing note mirrors this set.
/// Changing an entry's find or replacement requires bumping
/// PINNED_PAIR_BUILD_RECIPE in the engine: extracted trees already carrying
/// the old form must orphan through a new identity rather than wedge on
/// drift.
pub struct ForwardSourceEdit {
    /// Path of the edited file, relative to the extracted MLX-C tree.
    pub path: &'static str,
    /// The exact text to find, anchored so it occurs exactly once in the
    /// upstream file. On a retry over already-edited sources the counted
    /// state must equal the APPLIED state — which the rule computes from
    /// whether the replacement contains the find text: containing edits
    /// still match find once (nested inside the replacement), non-
    /// containing edits match it zero times. Anything else means upstream
    /// drifted, and the build fails loudly instead of editing a moved
    /// target.
    pub find: &'static str,
    /// The replacement text.
    pub replacement: &'static str,
    /// The MLX-C release that makes this edit droppable; each MLX-C release
    /// reconciles this set.
    pub drop_with: &'static str,
}

pub const MLX_C_FORWARD_SOURCE_EDITS: &[ForwardSourceEdit] = &[
    ForwardSourceEdit {
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
    },
    ForwardSourceEdit {
        path: "CMakeLists.txt",
        find: r#"if(MLX_C_USE_SYSTEM_MLX)
  find_package(MLX REQUIRED)"#,
        replacement: r#"if(MLX_C_USE_SYSTEM_MLX)
  # The installed static MLX package's targets reference jaccl::jaccl in
  # their link interface without loading the jaccl package themselves, so
  # the consumer must resolve it first or find_package(MLX) dies at
  # set_target_properties on a missing target.
  find_package(jaccl REQUIRED)
  find_package(MLX REQUIRED)"#,
        drop_with: "an MLX or MLX-C release whose system-MLX packaging \
                    resolves jaccl's imported target itself",
    },
];

/// Applies the forward edit set to the extracted MLX-C tree. Idempotent: a
/// retry over sources edited by a previous attempt recognizes the
/// replacement and moves on, so a failed later step never wedges the
/// working directory.
pub fn apply_forward_source_edits(mlx_c_directory: &std::path::Path) -> Option<()> {
    apply_edit_set(MLX_C_FORWARD_SOURCE_EDITS, mlx_c_directory)
}

/// The counting rule over any edit set and any root directory; the engine
/// entry point delegates over the real set, and the contract tests drive
/// synthetic sets through the same rule.
pub fn apply_edit_set(
    edit_set: &[ForwardSourceEdit],
    root_directory: &std::path::Path,
) -> Option<()> {
    for forward_edit in edit_set {
        let edited_file = root_directory.join(forward_edit.path);
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
        let replacement_occurrences = source_content.matches(forward_edit.replacement).count();
        // The idempotency rule is shape-aware, computed rather than
        // configured: when a replacement CONTAINS its find text (the find
        // block plus inserted lines), an already-edited tree still matches
        // find exactly once — nested inside the replacement; when it does
        // not, the applied state shows find zero times. Anything else means
        // the pinned sources drifted, and the build says so loudly instead
        // of editing a moved target or double-applying into a nested site.
        let applied_find_occurrences =
            usize::from(forward_edit.replacement.contains(forward_edit.find));
        if replacement_occurrences == 1 && find_occurrences == applied_find_occurrences {
            // Already applied by a previous attempt over the same extracted
            // tree; the edit set is idempotent by design.
            continue;
        }
        if replacement_occurrences == 0 && find_occurrences == 1 {
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
