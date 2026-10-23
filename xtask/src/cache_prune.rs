//! `cargo xtask prune-caches` — prunes surplus GitHub Actions caches so
//! the repository stays under the 10 GB cache storage limit. The policy is
//! adapted from the astronomical repository's proven policy: a pure,
//! unit-tested deletion plan beside this source, with the `gh` list and
//! delete calls kept as thin edges.
//!
//! Why prune at all: the workflow saves new cache generations as
//! `Cargo.lock` and the native pin move, and GitHub evicts caches by least
//! recent use once the repository exceeds its budget — silently discarding
//! the precious native-pair cache and forcing the next run into a
//! multi-minute native rebuild. The prune keeps every cache family at its
//! keep-count and deletes oversize sccache generations so eviction never
//! has to choose.

use std::process::{Command, ExitCode};

/// The prefix every cache family this repository saves starts with. Keys
/// outside it (none today) are left alone.
const CACHE_KEY_PREFIX: &str = "mlx-c-rust-v1-";
/// native-pair keeps two generations because a pin bump creates the new
/// generation while pull requests based on the old pin still restore it;
/// deleting the old one mid-flight forces their runners into cold native
/// builds.
const NATIVE_PAIR_KEEP_COUNT: usize = 2;
const DEFAULT_KEEP_COUNT: usize = 1;
/// Size ceilings are family-scoped because only sccache generations ratchet
/// upward; 0 disables the ceiling for families that own bounded artifacts.
/// The sccache cap (`SCCACHE_CACHE_SIZE=400M`, 0.419 decimal GB of tracked
/// entries) plus cache-archive overhead must land under this decimal
/// ceiling, so a generation that slipped past the write-time cap can never
/// restore within budget and is deleted even when newest.
const SCCACHE_MAX_ENTRY_BYTES: u64 = 450_000_000;
const DEFAULT_MAX_ENTRY_BYTES: u64 = 0;
/// sccache saves are default-branch-only, so a non-default-ref sccache
/// entry is never superseded and only burns budget; every other family
/// keeps one entry across non-default refs so an in-flight pull request
/// stays warm.
const SCCACHE_OTHER_REF_KEEP_COUNT: usize = 0;
const DEFAULT_OTHER_REF_KEEP_COUNT: usize = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheEntry {
    pub id: u64,
    pub cache_key: String,
    pub ref_name: String,
    pub created_at: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeletionReason {
    Oversize,
    Surplus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deletion {
    pub id: u64,
    pub reason: DeletionReason,
    pub size_bytes: u64,
}

/// The family is the key with the optional `mlx-c-rust-v1-` prefix and
/// the trailing hash segment removed, so it carries the platform
/// (`sccache-macOS-arm64`): keep-counts then apply per platform, and the
/// day a second runner platform joins, its prune cannot delete this
/// platform's generations. Family names themselves contain hyphens, which
/// is why classification strips the trailing segment instead of guessing
/// by splitting; legacy keys without the platform segment (the original
/// `native-pair-<hash>` form) keep their bare family name. Unknown
/// families are never deleted: the prune must not destroy artifacts it
/// cannot name.
const LEGACY_SCCACHE_KEY_PREFIX: &str = "sccache/";
const LEGACY_SCCACHE_FAMILY_NAME: &str = "sccache-legacy-backend";

pub fn family_of_key(cache_key: &str) -> &str {
    if cache_key.starts_with(LEGACY_SCCACHE_KEY_PREFIX) {
        return LEGACY_SCCACHE_FAMILY_NAME;
    }
    let stripped_key = cache_key
        .strip_prefix(CACHE_KEY_PREFIX)
        .unwrap_or(cache_key);
    match stripped_key.rsplit_once('-') {
        Some((family_with_platform, _hash_segment)) => family_with_platform,
        None => stripped_key,
    }
}

fn keep_count_for_family(family_with_platform: &str) -> usize {
    if family_with_platform == LEGACY_SCCACHE_FAMILY_NAME {
        0
    } else if family_with_platform.starts_with("native-pair") {
        NATIVE_PAIR_KEEP_COUNT
    } else if family_with_platform.starts_with("sccache")
        || family_with_platform.starts_with("cargo-downloads")
    {
        DEFAULT_KEEP_COUNT
    } else {
        usize::MAX
    }
}

fn max_entry_bytes_for_family(family_with_platform: &str) -> u64 {
    if family_with_platform.starts_with("sccache") {
        SCCACHE_MAX_ENTRY_BYTES
    } else {
        DEFAULT_MAX_ENTRY_BYTES
    }
}

fn other_ref_keep_count_for_family(family_with_platform: &str) -> usize {
    if family_with_platform == LEGACY_SCCACHE_FAMILY_NAME {
        0
    } else if family_with_platform.starts_with("sccache") {
        SCCACHE_OTHER_REF_KEEP_COUNT
    } else if family_with_platform.starts_with("native-pair")
        || family_with_platform.starts_with("cargo-downloads")
    {
        DEFAULT_OTHER_REF_KEEP_COUNT
    } else {
        usize::MAX
    }
}

/// The pure deletion plan: entries above the family's size ceiling are
/// deleted first even when newest (they can never restore within budget),
/// then the newest `keep_count` entries on the default branch and, per
/// non-default ref, the newest `other_ref_keep_count` entries are kept —
/// per ref, not pooled, so two open pull requests each keep their warm
/// cache. The remainder is deleted. `created_at` is ISO 8601 UTC, so
/// lexicographic ordering is chronological.
pub fn deletion_plan(entries: &[CacheEntry], default_branch_ref: &str) -> Vec<Deletion> {
    let mut family_names: Vec<&str> = entries
        .iter()
        .map(|cache_entry| family_of_key(&cache_entry.cache_key))
        .collect();
    family_names.sort_unstable();
    family_names.dedup();

    let mut deletions = Vec::new();
    for family_with_platform in family_names {
        let mut family_entries: Vec<&CacheEntry> = entries
            .iter()
            .filter(|cache_entry| family_of_key(&cache_entry.cache_key) == family_with_platform)
            .collect();
        family_entries.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        let mut kept_on_default_branch = 0usize;
        let mut kept_per_other_ref: std::collections::HashMap<&str, usize> =
            std::collections::HashMap::new();
        for cache_entry in family_entries {
            let size_ceiling = max_entry_bytes_for_family(family_with_platform);
            if size_ceiling > 0 && cache_entry.size_bytes > size_ceiling {
                deletions.push(Deletion {
                    id: cache_entry.id,
                    reason: DeletionReason::Oversize,
                    size_bytes: cache_entry.size_bytes,
                });
                continue;
            }
            if cache_entry.ref_name == default_branch_ref {
                kept_on_default_branch += 1;
                if kept_on_default_branch <= keep_count_for_family(family_with_platform) {
                    continue;
                }
            } else {
                let kept_count_on_this_ref = kept_per_other_ref
                    .entry(cache_entry.ref_name.as_str())
                    .or_insert(0);
                *kept_count_on_this_ref += 1;
                if *kept_count_on_this_ref <= other_ref_keep_count_for_family(family_with_platform)
                {
                    continue;
                }
            }
            deletions.push(Deletion {
                id: cache_entry.id,
                reason: DeletionReason::Surplus,
                size_bytes: cache_entry.size_bytes,
            });
        }
    }
    deletions
}

/// Parses the `id \t key \t ref \t created_at \t size` TSV the gh jq filter
/// emits. Malformed lines are skipped rather than fatal: one bad row must
/// not blind the prune to the rest of the inventory.
pub fn parse_cache_list_tsv(tsv_text: &str) -> Vec<CacheEntry> {
    tsv_text
        .lines()
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            if fields.len() < 5 {
                return None;
            }
            Some(CacheEntry {
                id: fields[0].parse().ok()?,
                cache_key: fields[1].to_owned(),
                ref_name: fields[2].to_owned(),
                created_at: fields[3].to_owned(),
                size_bytes: fields[4].parse().ok()?,
            })
        })
        .collect()
}

/// The command entry point. `--dry-run` prints the plan without deleting;
/// `--repo owner/name` overrides `GITHUB_REPOSITORY` so a human can run
/// the same plan locally that continuous integration runs in the workflow.
pub fn run(arguments: &[String]) -> ExitCode {
    let started_at = std::time::Instant::now();
    let mut dry_run = false;
    let mut repository = std::env::var("GITHUB_REPOSITORY").ok();
    let mut argument_index = 0;
    while argument_index < arguments.len() {
        match arguments[argument_index].as_str() {
            "--dry-run" => dry_run = true,
            "--repo" => {
                argument_index += 1;
                match arguments.get(argument_index) {
                    Some(repo_argument) => repository = Some(repo_argument.clone()),
                    None => {
                        eprintln!("[cache-prune] status=failed reason=--repo-requires-owner-name");
                        return ExitCode::FAILURE;
                    }
                }
            }
            other => {
                eprintln!(
                    "[cache-prune] status=failed reason=unexpected-argument argument={other}"
                );
                return ExitCode::FAILURE;
            }
        }
        argument_index += 1;
    }
    let Some(repository) = repository else {
        eprintln!(
            "[cache-prune] status=failed reason=no-repository-pass---repo-or-set-GITHUB_REPOSITORY"
        );
        return ExitCode::FAILURE;
    };
    eprintln!("[cache-prune] status=start repository={repository} dry_run={dry_run}");

    let (default_branch, cache_list_text) = match list_repository_caches(&repository) {
        Ok(listed) => listed,
        Err(listing_error) => {
            eprintln!("[cache-prune] status=failed reason=listing-failed error={listing_error}");
            return ExitCode::FAILURE;
        }
    };
    let cache_entries = parse_cache_list_tsv(&cache_list_text);
    if cache_entries.is_empty() {
        eprintln!("[cache-prune] status=complete outcome=no-caches-found");
        return ExitCode::SUCCESS;
    }
    eprintln!(
        "[cache-prune] default_branch={default_branch} cache_entries={}",
        cache_entries.len()
    );

    let default_branch_ref = format!("refs/heads/{default_branch}");
    let deletions = deletion_plan(&cache_entries, &default_branch_ref);
    let mut total_deleted_count = 0usize;
    let mut total_freed_bytes = 0u64;
    for deletion in &deletions {
        let reason_text = match deletion.reason {
            DeletionReason::Oversize => "oversize",
            DeletionReason::Surplus => "surplus",
        };
        if dry_run {
            eprintln!(
                "[cache-prune] action=would-delete reason={reason_text} cache_id={} size_bytes={}",
                deletion.id, deletion.size_bytes
            );
        } else {
            let delete_status = Command::new("gh")
                .args([
                    "cache",
                    "delete",
                    &deletion.id.to_string(),
                    "-R",
                    &repository,
                ])
                .status();
            match delete_status {
                Ok(status) if status.success() => {}
                // A vanished cache (404) must not fail the prune; the next
                // run relists and retries.
                _ => {
                    eprintln!(
                        "[cache-prune] action=delete-failed cache_id={}",
                        deletion.id
                    );
                    continue;
                }
            }
            eprintln!(
                "[cache-prune] action=deleted reason={reason_text} cache_id={} size_bytes={}",
                deletion.id, deletion.size_bytes
            );
        }
        total_deleted_count += 1;
        total_freed_bytes += deletion.size_bytes;
    }
    let outcome_text = if dry_run { "dry-run" } else { "pruned" };
    eprintln!(
        "[cache-prune] status=complete outcome={outcome_text} deleted={total_deleted_count} freed_bytes={total_freed_bytes} elapsed_seconds={:.1}",
        started_at.elapsed().as_secs_f64()
    );
    ExitCode::SUCCESS
}

/// Lists the repository's caches through the GitHub CLI: the default
/// branch name plus one TSV row per cache entry.
fn list_repository_caches(repository: &str) -> Result<(String, String), String> {
    let default_branch = gh_output(&[
        "api",
        &format!("repos/{repository}"),
        "--jq",
        ".default_branch",
    ])?;
    let cache_list_text = gh_output(&[
        "api",
        "--paginate",
        &format!("repos/{repository}/actions/caches"),
        "--jq",
        ".actions_caches[] | [.id, .key, .ref, .created_at, .size_in_bytes] | @tsv",
    ])?;
    Ok((default_branch.trim().to_owned(), cache_list_text))
}

fn gh_output(gh_arguments: &[&str]) -> Result<String, String> {
    let gh_output = Command::new("gh")
        .args(gh_arguments)
        .output()
        .map_err(|launch_error| format!("gh failed to start: {launch_error}"))?;
    if !gh_output.status.success() {
        return Err(format!(
            "gh api failed with {}: {}",
            gh_output.status,
            String::from_utf8_lossy(&gh_output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&gh_output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEFAULT_BRANCH_REF: &str = "refs/heads/main";

    fn cache_entry(
        id: u64,
        cache_key: &str,
        ref_name: &str,
        created_at: &str,
        size_bytes: u64,
    ) -> CacheEntry {
        CacheEntry {
            id,
            cache_key: cache_key.to_owned(),
            ref_name: ref_name.to_owned(),
            created_at: created_at.to_owned(),
            size_bytes,
        }
    }

    #[test]
    fn should_classify_families_with_their_platform_segment() {
        assert_eq!(
            family_of_key("mlx-c-rust-v1-sccache-macOS-arm64-abc123"),
            "sccache-macOS-arm64"
        );
        assert_eq!(
            family_of_key("mlx-c-rust-v1-cargo-downloads-macOS-arm64-abc123"),
            "cargo-downloads-macOS-arm64"
        );
        assert_eq!(family_of_key("native-pair-def456"), "native-pair");
        assert_eq!(
            family_of_key("mlx-c-rust-v1-native-pair-macOS-arm64-def456"),
            "native-pair-macOS-arm64"
        );
        assert_eq!(
            family_of_key("sccache/f/a/9/fa9f81cd"),
            "sccache-legacy-backend"
        );
    }

    #[test]
    fn should_keep_each_platforms_generation_independently() {
        let cross_platform_entries = vec![
            cache_entry(
                70,
                "mlx-c-rust-v1-sccache-macOS-arm64-hash",
                DEFAULT_BRANCH_REF,
                "2026-10-03T00:00:00Z",
                1,
            ),
            cache_entry(
                71,
                "mlx-c-rust-v1-sccache-Linux-x64-hash",
                DEFAULT_BRANCH_REF,
                "2026-10-03T00:00:00Z",
                1,
            ),
        ];
        assert!(deletion_plan(&cross_platform_entries, DEFAULT_BRANCH_REF).is_empty());
    }

    #[test]
    fn should_keep_one_entry_per_non_default_ref_so_parallel_pulls_stay_warm() {
        let parallel_pull_entries = vec![
            cache_entry(
                80,
                "mlx-c-rust-v1-cargo-downloads-macOS-arm64-a",
                "refs/heads/first-feature",
                "2026-10-03T00:00:00Z",
                1,
            ),
            cache_entry(
                81,
                "mlx-c-rust-v1-cargo-downloads-macOS-arm64-b",
                "refs/heads/second-feature",
                "2026-10-02T00:00:00Z",
                1,
            ),
        ];
        assert!(deletion_plan(&parallel_pull_entries, DEFAULT_BRANCH_REF).is_empty());
    }

    #[test]
    fn should_delete_oversize_sccache_entries_on_other_refs_too() {
        let oversize_entries = vec![cache_entry(
            90,
            "mlx-c-rust-v1-sccache-macOS-arm64-pr",
            "refs/heads/feature",
            "2026-10-03T00:00:00Z",
            SCCACHE_MAX_ENTRY_BYTES + 1,
        )];
        let deletions = deletion_plan(&oversize_entries, DEFAULT_BRANCH_REF);
        assert_eq!(
            deletions,
            vec![Deletion {
                id: 90,
                reason: DeletionReason::Oversize,
                size_bytes: SCCACHE_MAX_ENTRY_BYTES + 1
            }]
        );
    }

    #[test]
    fn should_delete_every_legacy_backend_entry_even_the_newest() {
        let legacy_entries = vec![
            cache_entry(
                60,
                "sccache/f/a/9/fa9f81cd",
                DEFAULT_BRANCH_REF,
                "2026-10-22T00:00:00Z",
                400_000,
            ),
            cache_entry(
                61,
                "sccache/e/b/f/ebff00e8",
                DEFAULT_BRANCH_REF,
                "2026-10-01T00:00:00Z",
                400_000,
            ),
        ];
        let deletions = deletion_plan(&legacy_entries, DEFAULT_BRANCH_REF);
        assert_eq!(deletions.len(), 2);
        assert!(
            deletions
                .iter()
                .all(|deletion| deletion.reason == DeletionReason::Surplus)
        );
    }

    #[test]
    fn should_keep_two_native_pair_generations_and_delete_the_oldest() {
        let native_pair_entries = vec![
            cache_entry(
                1,
                "native-pair-old",
                DEFAULT_BRANCH_REF,
                "2026-10-01T00:00:00Z",
                1,
            ),
            cache_entry(
                2,
                "native-pair-mid",
                DEFAULT_BRANCH_REF,
                "2026-10-02T00:00:00Z",
                1,
            ),
            cache_entry(
                3,
                "native-pair-new",
                DEFAULT_BRANCH_REF,
                "2026-10-03T00:00:00Z",
                1,
            ),
        ];
        let deletions = deletion_plan(&native_pair_entries, DEFAULT_BRANCH_REF);
        assert_eq!(
            deletions,
            vec![Deletion {
                id: 1,
                reason: DeletionReason::Surplus,
                size_bytes: 1
            }]
        );
    }

    #[test]
    fn should_delete_oversize_sccache_entries_even_when_newest() {
        let sccache_entries = vec![
            cache_entry(
                10,
                "mlx-c-rust-v1-sccache-macOS-arm64-new",
                DEFAULT_BRANCH_REF,
                "2026-10-03T00:00:00Z",
                SCCACHE_MAX_ENTRY_BYTES + 1,
            ),
            cache_entry(
                11,
                "mlx-c-rust-v1-sccache-macOS-arm64-old",
                DEFAULT_BRANCH_REF,
                "2026-10-01T00:00:00Z",
                1,
            ),
        ];
        let deletions = deletion_plan(&sccache_entries, DEFAULT_BRANCH_REF);
        assert_eq!(
            deletions,
            vec![Deletion {
                id: 10,
                reason: DeletionReason::Oversize,
                size_bytes: SCCACHE_MAX_ENTRY_BYTES + 1
            }]
        );
    }

    #[test]
    fn should_delete_all_sccache_entries_on_other_refs() {
        let sccache_entries = vec![cache_entry(
            20,
            "mlx-c-rust-v1-sccache-macOS-arm64-pr",
            "refs/heads/feature",
            "2026-10-02T00:00:00Z",
            1,
        )];
        let deletions = deletion_plan(&sccache_entries, DEFAULT_BRANCH_REF);
        assert_eq!(
            deletions,
            vec![Deletion {
                id: 20,
                reason: DeletionReason::Surplus,
                size_bytes: 1
            }]
        );
    }

    #[test]
    fn should_keep_one_other_ref_entry_for_bounded_families() {
        let cargo_download_entries = vec![
            cache_entry(
                30,
                "mlx-c-rust-v1-cargo-downloads-macOS-arm64-a",
                "refs/heads/feature",
                "2026-10-02T00:00:00Z",
                1,
            ),
            cache_entry(
                31,
                "mlx-c-rust-v1-cargo-downloads-macOS-arm64-b",
                "refs/heads/feature",
                "2026-10-01T00:00:00Z",
                1,
            ),
        ];
        let deletions = deletion_plan(&cargo_download_entries, DEFAULT_BRANCH_REF);
        assert_eq!(
            deletions,
            vec![Deletion {
                id: 31,
                reason: DeletionReason::Surplus,
                size_bytes: 1
            }]
        );
    }

    #[test]
    fn should_keep_newest_default_branch_entry_for_bounded_families() {
        let cargo_download_entries = vec![
            cache_entry(
                40,
                "mlx-c-rust-v1-cargo-downloads-macOS-arm64-a",
                DEFAULT_BRANCH_REF,
                "2026-10-02T00:00:00Z",
                1,
            ),
            cache_entry(
                41,
                "mlx-c-rust-v1-cargo-downloads-macOS-arm64-b",
                DEFAULT_BRANCH_REF,
                "2026-10-01T00:00:00Z",
                1,
            ),
        ];
        let deletions = deletion_plan(&cargo_download_entries, DEFAULT_BRANCH_REF);
        assert_eq!(
            deletions,
            vec![Deletion {
                id: 41,
                reason: DeletionReason::Surplus,
                size_bytes: 1
            }]
        );
    }

    #[test]
    fn should_plan_nothing_for_an_empty_inventory() {
        assert!(deletion_plan(&[], DEFAULT_BRANCH_REF).is_empty());
    }

    #[test]
    fn should_leave_unknown_cache_families_alone() {
        let unknown_entries = vec![
            cache_entry(
                50,
                "someone-elses-cache",
                DEFAULT_BRANCH_REF,
                "2026-10-03T00:00:00Z",
                1,
            ),
            cache_entry(
                51,
                "mlx-c-rust-v1-unknown-family-macOS-arm64-x",
                DEFAULT_BRANCH_REF,
                "2026-10-03T00:00:00Z",
                1,
            ),
        ];
        assert!(deletion_plan(&unknown_entries, DEFAULT_BRANCH_REF).is_empty());
    }

    #[test]
    fn should_parse_well_formed_tsv_rows() {
        let tsv_text = "1\tnative-pair-abc\trefs/heads/main\t2026-10-02T00:00:00Z\t1024\n";
        let parsed_entries = parse_cache_list_tsv(tsv_text);
        assert_eq!(parsed_entries.len(), 1);
        assert_eq!(parsed_entries[0].cache_key, "native-pair-abc");
        assert_eq!(parsed_entries[0].size_bytes, 1024);
    }

    #[test]
    fn should_skip_malformed_tsv_rows_instead_of_failing() {
        let tsv_text = "not-a-number\tkey\tref\t2026-10-02T00:00:00Z\t10\n1\ttwo-fields\n\n";
        let parsed_entries = parse_cache_list_tsv(tsv_text);
        assert!(parsed_entries.is_empty());
    }
}
