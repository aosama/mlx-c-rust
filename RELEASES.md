# Release discipline

A release is the act of certifying that one commit, against one pinned
MLX and MLX-C pair, passed the adopter's exact journey. The maintainer's
machine never proves a release: it carries a warm machine-shared native
prefix and warm caches. The proof runs on fresh runners with nothing.

## Classify the change — this decides the version

The crate version names the MLX release it is verified against; our own
fixes ride as patch bumps (see the Versioning section of the README).

| Change | Version | Extra obligation |
|---|---|---|
| Defect fix in the bridge (safety, wrong semantics, leak) | Patch (`0.32.3` → `0.32.4`) | Release notes state whether the unsafe boundary was touched |
| New bindings for symbols already in the vendored headers | Patch | `COVERAGE_INVENTORY.md` updated |
| MLX-C header pin bump within the same MLX release | Patch | Inventory note; checksums enforced by `native_build.rs` |
| Verification against a new MLX release | Version rides MLX (`0.33.0-alpha.1` …) | The native cache key changes with the pin, so the cold journey proves itself exactly then |
| Breaking Rust API change | Rides the next MLX-release alpha window | While pre-stable, an urgent one may ship as an alpha of the current series with explicit breaking-change notes |

**Breaking-change policy (decided):** while the version tracks the MLX
release number, there is no natural slot for a breaking Rust change —
cargo reads `0.32.4 → 0.33.0` as breaking, but `0.33.0` *means*
"verified against MLX 0.33". Breaking changes therefore wait for the
next MLX-release bump and ride its alpha window; once the crate declares
stability, breaking changes only ever ride an MLX release bump.

## The release pull request

Releases flow through the same protected pipeline as every change — one
pull request, linked to its issue (`Fixes #N`), containing:

1. The version bump in `Cargo.toml`, with `Cargo.lock` refreshed.
2. The release notes body — the text the GitHub Release will carry.
3. `README.md` and `COVERAGE_INVENTORY.md` refreshed if the verified
   pair or the surface moved.
4. The release line's version link updated to the new release page.

## Verification gates

1. **The standard lanes** — lint/document and native build/test, green
   on the release pull request.
2. **The cold native journey** — once per release, run the native lane
   with the compiled-pair restore skipped, proving the checksummed
   sources download and compile from nothing on a fresh machine: the
   zero-configuration adopter's exact path.
3. **Packaging** — `cargo package` from the tag, proving the packaged
   crate carries the vendored headers and excludes `xtask`, `.github`,
   and `.cargo`.

## Cut and publish, in order

1. Tag the squash-merge commit: annotated tag `v<version>`.
2. Publish from the tag checkout, never the working tree:
   `cargo publish`.
3. Create the GitHub Release from the tag with the release notes.

**Changelog of record (decided):** GitHub Releases. Every pull request
already links its issue and merges through one pipeline, so the release
notes aggregate the linked issues — no parallel `CHANGELOG.md` to
maintain, no merge conflicts in release pull requests.

## After publishing

- The crates.io page shows the version.
- docs.rs built the documentation (the document lane mirrors its
  target; confirm the real build).
- A scratch consumer project: `cargo add mlx-c-rust@<version>`, build,
  run one operation — the adopter's first five minutes, proven.

## Defect response after a release

- A defect in the latest release: fix through the normal pipeline, then
  a patch release through the same gates. Never patch a tag; history is
  linear and immutable.
- **Yank criteria (decided in advance):** yank only for unsoundness, a
  security problem, or a package that cannot compile at all. Everything
  else deserves a patch release. Yanking is loud and public.
