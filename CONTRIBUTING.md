# Contributing to mlx-c-rust

Every change to this repository flows through a pull request that links a
real issue and passes the required checks — for maintainers too. This
guide is the shortest path from idea to merged change.

## Before you open a pull request

1. **Create or pick an issue.** Every pull request must reference at least
   one real issue of this repository; the compliance check fails the pull
   request until it does.

2. **Activate the local gates** (once per clone):

   ```
   cargo xtask install-hooks
   ```

   This points git at `.githooks`: the pre-commit hook enforces the
   per-file size ceiling on what you stage, and the pre-push hook runs
   `cargo xtask verify` — the same lanes continuous integration runs —
   so failures surface before the push spends a GitHub Actions run.
   `MLX_C_RUST_SKIP_PRE_PUSH=1` escapes deliberately when the failures
   are the subject under test.

3. **Verify locally** what the runners will verify:

   ```
   cargo xtask verify
   cargo test -- --test-threads=1
   ```

   The first checks the size ceiling, formatting, clippy, and
   documentation for the shipped `aarch64-apple-darwin` target; the
   second runs the test suite against the machine-shared native prefix
   (built automatically on first use, or through
   `cargo xtask build-native`).

## Writing the pull request body

The template's `## Linked issue` section is scanned by the compliance
check ([the policy script](.github/scripts/pull-request-issue-compliance.js)):

- **Closing keywords** close the issue at merge time: `Fixes #N`,
  `Closes #N`, `Resolves #N`.
- **Non-closing references** keep the issue open: `Refs #N`.
- **Context phrases** satisfy the contract for relationships that are
  real but not closing: `part of #N`, `tracked by #N`, `umbrella #N`,
  `epic #N`, `see also #N`.
- **Groups** share one keyword: `Fixes #N, #M, and #K`.
- The same-repository issue URL is accepted wherever `#N` is.

Rejected: a bare `#N` with no keyword, references whose number is a pull
request, and numbers that do not exist as issues in this repository.

## What the required checks prove

- **Lint and document on the shipped target** — formatting, clippy with
  warnings as errors, documentation, the per-file size ceiling, and the
  pull-request policy contracts.
- **Adopt, build, and test — the one macOS journey** — the pinned MLX
  and MLX-C pair compiled from scratch and the test suite against it:
  exactly the journey an adopter's machine performs.
- **PR issue compliance** — the pull request documents its issue.

Branch protection requires all three, an up-to-date branch, and keeps
history linear: merges are squash merges.

## The safety checklist

The template's safety section is the review contract for this crate:
keep the unsafe boundary encapsulated (owned handles, checked statuses,
no raw pointers crossing the public API), reflect any MLX-C surface
change in `COVERAGE_INVENTORY.md` and the vendored header pin, and
include no credentials, personal paths, or machine-specific assumptions.

## Releases

Releases follow a written discipline — what decides a version bump,
which gates a release must pass (including the cold native journey),
the publish order, and the yank criteria. It lives in
[RELEASES.md](RELEASES.md).
