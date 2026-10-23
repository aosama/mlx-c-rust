//! `cargo xtask source-size [--staged]` — enforces the repository's
//! per-file line ceiling that AGENTS.md sets (code files stay under 700
//! lines) on every code file the repository owns. `--staged` inspects
//! only the files staged for the next commit and measures their index
//! content — what the commit will actually contain, not the working
//! tree — which is the pre-commit hook's view; without it the whole
//! repository is scanned, which is the continuous-integration view. Both
//! views share one ceiling and one line-counting rule so they can never
//! disagree.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use crate::repository::repository_root;

/// The ceiling AGENTS.md sets for code files. A file above it is a
/// separation-of-concerns defect long before it is a tooling problem: it
/// almost always carries two jobs that belong in two modules.
const MAX_LINES_PER_CODE_FILE: usize = 700;

/// Extensions that count as code files under the ceiling. C-family headers
/// are included because the vendored tree's public surface ships there;
/// the vendored tree itself is excluded as third-party code this
/// repository does not own.
const CODE_FILE_EXTENSIONS: [&str; 9] = ["rs", "c", "h", "cpp", "hpp", "cc", "cxx", "m", "mm"];

/// Directory names whose contents are not ours to measure: vendored
/// third-party code, cargo build output, and git metadata.
const EXCLUDED_DIRECTORY_NAMES: [&str; 3] = ["vendor", "target", ".git"];

/// Whether a repository-relative path is a code file this repository owns
/// and therefore must keep under the ceiling.
fn is_owned_code_file(relative_path: &Path) -> bool {
    let crosses_excluded_directory = relative_path.components().any(|component| {
        component
            .as_os_str()
            .to_str()
            .is_some_and(|component_name| EXCLUDED_DIRECTORY_NAMES.contains(&component_name))
    });
    if crosses_excluded_directory {
        return false;
    }
    relative_path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| CODE_FILE_EXTENSIONS.contains(&extension))
}

/// Counts lines with `wc -l` semantics — the number of line feeds — so the
/// ceiling matches how the repository was measured when the rule was
/// written. Bytes, not characters, because code files may carry any
/// encoding and only the line structure matters here.
fn count_line_feeds(file_bytes: &[u8]) -> usize {
    file_bytes.iter().filter(|byte| **byte == b'\n').count()
}

/// The staged view's file list: files added, copied, modified, or renamed
/// in the index. Deleted files cannot violate a size ceiling, so they
/// never appear. Any git failure is an error, not an empty list: a gate
/// that silently sees nothing green-lights everything.
fn staged_owned_code_file_paths() -> Result<Vec<String>, String> {
    let git_output = Command::new("git")
        .args([
            "diff",
            "--cached",
            "--name-only",
            "--diff-filter=ACMR",
            "-z",
        ])
        .current_dir(repository_root())
        .output()
        .map_err(|launch_error| format!("git failed to start: {launch_error}"))?;
    if !git_output.status.success() {
        return Err(format!(
            "git diff --cached failed with {}: {}",
            git_output.status,
            String::from_utf8_lossy(&git_output.stderr).trim()
        ));
    }
    Ok(git_output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path_bytes| !path_bytes.is_empty())
        .map(|path_bytes| String::from_utf8_lossy(path_bytes).into_owned())
        .filter(|relative_path| is_owned_code_file(Path::new(relative_path)))
        .collect())
}

/// The index content of one staged file, read through `git show :<path>`
/// with the repository root as the working directory. Judging the index
/// blob — not the working-tree file — means a file staged at 750 lines
/// but trimmed in the working copy still fails, and a partially staged
/// fix that leaves oversized content staged still fails.
fn staged_file_index_contents(relative_path: &str) -> Result<Vec<u8>, String> {
    let index_path_specification = format!(":{relative_path}");
    let git_output = Command::new("git")
        .args(["show", &index_path_specification])
        .current_dir(repository_root())
        .output()
        .map_err(|launch_error| format!("git failed to start: {launch_error}"))?;
    if !git_output.status.success() {
        return Err(format!(
            "git show {index_path_specification} failed with {}: {}",
            git_output.status,
            String::from_utf8_lossy(&git_output.stderr).trim()
        ));
    }
    Ok(git_output.stdout)
}

/// The repository-wide view: every owned code file under the root, as
/// absolute paths straight from the directory walk.
fn all_owned_code_files() -> Vec<PathBuf> {
    let mut collected_code_files = Vec::new();
    collect_owned_code_files(&repository_root(), &mut collected_code_files);
    collected_code_files
}

fn collect_owned_code_files(directory: &Path, collected_code_files: &mut Vec<PathBuf>) {
    let Ok(directory_entries) = fs::read_dir(directory) else {
        return;
    };
    for directory_entry in directory_entries.flatten() {
        let entry_path = directory_entry.path();
        let entry_name = directory_entry.file_name();
        let Some(entry_name) = entry_name.to_str() else {
            continue;
        };
        if entry_path.is_dir() {
            if !EXCLUDED_DIRECTORY_NAMES.contains(&entry_name) {
                collect_owned_code_files(&entry_path, collected_code_files);
            }
        } else if is_owned_code_file(Path::new(entry_name)) {
            collected_code_files.push(entry_path);
        }
    }
}

/// The command entry point. Prints one structured line per violating file
/// and a summary; a violation fails the command so hooks and continuous
/// integration both block on it.
pub fn run(arguments: &[String]) -> ExitCode {
    let staged_only = arguments.iter().any(|argument| argument == "--staged");
    let started_at = std::time::Instant::now();

    let mut oversized_files: Vec<(PathBuf, usize)> = Vec::new();
    let scanned_code_file_count: usize;

    if staged_only {
        let staged_paths = match staged_owned_code_file_paths() {
            Ok(staged_paths) => staged_paths,
            Err(listing_error) => {
                eprintln!(
                    "[source-size] status=failed reason=staged-listing-failed error={listing_error}"
                );
                return ExitCode::FAILURE;
            }
        };
        scanned_code_file_count = staged_paths.len();
        for relative_path in &staged_paths {
            match staged_file_index_contents(relative_path) {
                Ok(index_bytes) => {
                    let line_feeds = count_line_feeds(&index_bytes);
                    if line_feeds > MAX_LINES_PER_CODE_FILE {
                        oversized_files.push((PathBuf::from(relative_path), line_feeds));
                    }
                }
                Err(content_error) => {
                    eprintln!(
                        "[source-size] status=failed reason=staged-content-unreadable path={relative_path} error={content_error}"
                    );
                    return ExitCode::FAILURE;
                }
            }
        }
    } else {
        let scanned_code_files = all_owned_code_files();
        scanned_code_file_count = scanned_code_files.len();
        for code_file in &scanned_code_files {
            let Ok(file_bytes) = fs::read(code_file) else {
                eprintln!(
                    "[source-size] status=unreadable path={}",
                    code_file.display()
                );
                continue;
            };
            let line_feeds = count_line_feeds(&file_bytes);
            if line_feeds > MAX_LINES_PER_CODE_FILE {
                oversized_files.push((code_file.clone(), line_feeds));
            }
        }
    }

    let view_name = if staged_only { "staged" } else { "repository" };
    println!(
        "[source-size] view={view_name} files_scanned={scanned_code_file_count} ceiling_lines={MAX_LINES_PER_CODE_FILE} violations={} elapsed_seconds={:.1}",
        oversized_files.len(),
        started_at.elapsed().as_secs_f64()
    );
    if oversized_files.is_empty() {
        return ExitCode::SUCCESS;
    }
    for (oversized_file, line_count) in &oversized_files {
        eprintln!(
            "[source-size] violation path={} lines={line_count} ceiling={MAX_LINES_PER_CODE_FILE}",
            oversized_file.display()
        );
    }
    eprintln!(
        "[source-size] hint=split the file along its concerns so each module does one kind of job"
    );
    ExitCode::FAILURE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_accept_owned_rust_source() {
        assert!(is_owned_code_file(Path::new("src/lib.rs")));
        assert!(is_owned_code_file(Path::new("build.rs")));
        assert!(is_owned_code_file(Path::new("xtask/src/main.rs")));
    }

    #[test]
    fn should_reject_vendored_code() {
        assert!(!is_owned_code_file(Path::new("vendor/mlx-c/mlx/c/ops.h")));
        assert!(!is_owned_code_file(Path::new("vendor/mlx-c/mlx/c/array.h")));
    }

    #[test]
    fn should_reject_build_output_and_git_metadata() {
        assert!(!is_owned_code_file(Path::new("target/debug/build/x.rs")));
        assert!(!is_owned_code_file(Path::new(".git/hooks/x.rs")));
    }

    #[test]
    fn should_reject_non_code_files() {
        assert!(!is_owned_code_file(Path::new("README.md")));
        assert!(!is_owned_code_file(Path::new("Cargo.toml")));
        assert!(!is_owned_code_file(Path::new("ci.yml")));
    }

    #[test]
    fn should_count_line_feeds_with_wc_semantics() {
        assert_eq!(count_line_feeds(b"fn main() {}\n"), 1);
        assert_eq!(count_line_feeds(b"one\ntwo\nthree"), 2);
        assert_eq!(count_line_feeds(b""), 0);
        assert_eq!(count_line_feeds(b"\n\n\n"), 3);
    }
}
