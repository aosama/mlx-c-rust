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
    /// The exact text to find. It must occur exactly once — or, on a retry
    /// over already-edited sources, exactly zero times with the replacement
    /// present once — so a drifted upstream fails the build loudly instead
    /// of editing the wrong site.
    pub find: &'static str,
    /// The replacement text.
    pub replacement: &'static str,
    /// The MLX-C release that makes this edit droppable; each MLX-C release
    /// reconciles this set.
    pub drop_with: &'static str,
}

pub const MLX_C_FORWARD_SOURCE_EDITS: &[ForwardSourceEdit] = &[ForwardSourceEdit {
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
pub fn apply_forward_source_edits(mlx_c_directory: &std::path::Path) -> Option<()> {
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
        let replacement_occurrences = source_content.matches(forward_edit.replacement).count();
        // Replacement-first, because this set's replacements CONTAIN their
        // find text (the find block plus an inserted line): over an
        // already-edited tree the find text still matches once, nested
        // inside the replacement, and a find-first check would re-apply the
        // edit into that nested site on every retry, corrupting the call a
        // little more each time.
        if replacement_occurrences == 1 && find_occurrences == 1 {
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
