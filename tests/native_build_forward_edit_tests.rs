//! Contracts for the forward-edit machinery that carries the pinned MLX-C
//! sources up to the pinned MLX — the linchpin of the track-MLX charter.
//! The engine is a source module shared by the build script and xtask, so
//! these tests include it directly instead of going through the crate's
//! library (the module's own tests never run under `cargo test`, since
//! build scripts are compiled for the host but never executed as tests).
//!
//! Every case seeds a synthetic extracted tree with text taken from the
//! real edit set, so the tests drift with the edits they protect: if an
//! entry's literals change, these contracts exercise the new form.

#[path = "../native_forward_edits.rs"]
mod native_forward_edits;

use native_forward_edits::{MLX_C_FORWARD_SOURCE_EDITS, apply_forward_source_edits};
use std::fs;
use std::path::{Path, PathBuf};

/// A fresh temporary tree shaped like the extracted MLX-C sources. Every
/// edit in the set gets its file seeded with fresh find text (so a whole-
/// set apply proceeds past the entries the test is not exercising), and
/// the first edit's file carries the given body — the one each contract
/// varies.
fn extracted_tree_with_target_body(target_body: &str) -> PathBuf {
    let mlx_c_directory = std::env::temp_dir().join(format!(
        "mlx-c-rust-forward-edit-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the system clock should be past the Unix epoch")
            .as_nanos()
    ));
    for forward_edit in MLX_C_FORWARD_SOURCE_EDITS {
        let seeded_file = mlx_c_directory.join(forward_edit.path);
        fs::create_dir_all(
            seeded_file
                .parent()
                .expect("the edit path names a file in directories"),
        )
        .expect("the synthetic extracted tree should create");
        fs::write(&seeded_file, forward_edit.find).expect("the seeded file should write");
    }
    let target_file = mlx_c_directory.join(MLX_C_FORWARD_SOURCE_EDITS[0].path);
    fs::write(&target_file, target_body).expect("the synthetic target file should write");
    mlx_c_directory
}

fn target_file_path(mlx_c_directory: &Path) -> PathBuf {
    mlx_c_directory.join(MLX_C_FORWARD_SOURCE_EDITS[0].path)
}

#[test]
fn should_apply_the_edit_to_fresh_upstream_sources() {
    let edit = &MLX_C_FORWARD_SOURCE_EDITS[0];
    let upstream_body = format!(
        "preceding context\n{find_text}\ntrailing context\n",
        find_text = edit.find
    );
    let mlx_c_directory = extracted_tree_with_target_body(&upstream_body);

    let apply_outcome = apply_forward_source_edits(&mlx_c_directory);

    assert!(
        apply_outcome.is_some(),
        "a fresh tree carrying the exact find text must edit cleanly"
    );
    let edited_body = fs::read_to_string(target_file_path(&mlx_c_directory))
        .expect("the edited file should still read");
    // Exact equality, not substring checks: this set's replacement CONTAINS
    // its find text, so "find no longer present" is not a valid expectation
    // — the edited file must be precisely the seeded body with the edit
    // applied and nothing else touched.
    assert_eq!(
        edited_body,
        format!(
            "preceding context\n{replacement_text}\ntrailing context\n",
            replacement_text = edit.replacement
        ),
        "the applied file must be exactly the seeded body with the edit applied"
    );
    let _ = fs::remove_dir_all(&mlx_c_directory);
}

#[test]
fn should_skip_idempotently_when_the_replacement_is_already_present() {
    let edit = &MLX_C_FORWARD_SOURCE_EDITS[0];
    let already_edited_body = format!(
        "preceding context\n{replacement_text}\ntrailing context\n",
        replacement_text = edit.replacement
    );
    let mlx_c_directory = extracted_tree_with_target_body(&already_edited_body);

    let apply_outcome = apply_forward_source_edits(&mlx_c_directory);

    assert!(
        apply_outcome.is_some(),
        "a retry over an already-edited tree must succeed without touching the file"
    );
    let body_after_retry =
        fs::read_to_string(target_file_path(&mlx_c_directory)).expect("the file should still read");
    assert_eq!(
        body_after_retry, already_edited_body,
        "the idempotent skip must leave the file byte-identical"
    );
    let _ = fs::remove_dir_all(&mlx_c_directory);
}

#[test]
fn should_fail_loudly_when_upstream_drifts_away_from_the_find_text() {
    let drifted_body = "the upstream call was rewritten upstream and no longer \
         carries the text this edit anchors on\n";
    let mlx_c_directory = extracted_tree_with_target_body(drifted_body);

    let apply_outcome = apply_forward_source_edits(&mlx_c_directory);

    assert!(
        apply_outcome.is_none(),
        "a drifted target must fail the build loudly instead of silently skipping"
    );
    let _ = fs::remove_dir_all(&mlx_c_directory);
}

#[test]
fn should_fail_loudly_when_the_find_text_occurs_more_than_once() {
    let edit = &MLX_C_FORWARD_SOURCE_EDITS[0];
    let duplicated_body = format!(
        "{find_text}\nsome separation\n{find_text}\n",
        find_text = edit.find
    );
    let mlx_c_directory = extracted_tree_with_target_body(&duplicated_body);

    let apply_outcome = apply_forward_source_edits(&mlx_c_directory);

    assert!(
        apply_outcome.is_none(),
        "an ambiguous target must fail the build loudly instead of editing one arbitrary site"
    );
    let _ = fs::remove_dir_all(&mlx_c_directory);
}

#[test]
fn should_handle_edits_whose_replacement_does_not_contain_the_find_text() {
    // A second shape exists in the set: edits whose replacement interrupts
    // the find text, so an applied tree shows find ZERO times rather than
    // nested inside the replacement. The real CMakeLists edit carries this
    // shape; a synthetic set drives the same counting rule through apply,
    // skip, and drift.
    use native_forward_edits::{ForwardSourceEdit, apply_edit_set};
    let synthetic_edit = ForwardSourceEdit {
        path: "synthetic/CMakeLists.txt",
        find: "if(SYSTEM_MODE)\n  find_package(SYSTEM REQUIRED)",
        replacement: "if(SYSTEM_MODE)\n  # injected dependency resolution\n  \
                      find_package(dep REQUIRED)\n  find_package(SYSTEM REQUIRED)",
        drop_with: "nothing; this edit exists only in tests",
    };
    let synthetic_set = [synthetic_edit];
    let mlx_c_directory = std::env::temp_dir().join(format!(
        "mlx-c-rust-forward-edit-shape-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the system clock should be past the Unix epoch")
            .as_nanos()
    ));
    let target_file = mlx_c_directory.join(synthetic_set[0].path);
    fs::create_dir_all(target_file.parent().expect("the path names directories"))
        .expect("the synthetic tree should create");

    // Fresh tree: applies exactly once.
    fs::write(&target_file, format!("{}\n", synthetic_set[0].find))
        .expect("the fresh target should write");
    assert!(
        apply_edit_set(&synthetic_set, &mlx_c_directory).is_some(),
        "a fresh tree carrying the exact find text must edit cleanly"
    );
    assert_eq!(
        fs::read_to_string(&target_file).expect("the edited file should read"),
        format!("{}\n", synthetic_set[0].replacement),
        "the non-containing shape must apply the full replacement"
    );

    // Applied tree: replacement once, find absent — the state that
    // previously misclassified as drift — must skip byte-identically.
    assert!(
        apply_edit_set(&synthetic_set, &mlx_c_directory).is_some(),
        "a retry over the applied non-containing form must succeed"
    );
    assert_eq!(
        fs::read_to_string(&target_file).expect("the file should still read"),
        format!("{}\n", synthetic_set[0].replacement),
        "the idempotent skip must leave the file byte-identical"
    );

    // Drifted tree: neither form present — loud failure.
    fs::write(&target_file, "upstream rewrote the block entirely\n")
        .expect("the drifted target should write");
    assert!(
        apply_edit_set(&synthetic_set, &mlx_c_directory).is_none(),
        "a drifted target must fail the build loudly"
    );
    let _ = fs::remove_dir_all(&mlx_c_directory);
}
