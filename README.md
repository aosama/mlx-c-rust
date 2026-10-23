# mlx-c-rust

A complete, low-level, policy-free Rust bridge to the public [MLX-C](https://github.com/ml-explore/mlx-c) C API of Apple's [MLX](https://github.com/ml-explore/mlx) array framework for Apple silicon.

**Current release: `0.32.3-alpha.1` — verified against MLX `v0.32.3`, binding the complete MLX-C `v0.7.0` surface.** The crate version always names the MLX release it is verified against — the number you search for and install — while the bound MLX-C release is recorded here and in `COVERAGE_INVENTORY.md`.

## The goal

MLX-C exposes the full power of MLX through a stable C Application Programming Interface. This crate binds **every public symbol that API declares** — not a demand-driven subset — so any capability Apple ships is callable from Rust the day the pin moves. Completeness is enforced, not aspirational: the vendored headers are bindgen-translated in full, and the bridged-symbol inventory they produce feeds a coverage contract that fails when a new MLX-C release adds or removes surface.

The design keeps the crate at one job:

- **Complete**: every function, type, and enum variant in the vendored MLX-C headers has a Rust path in.
- **Low-level**: thin, predictable wrappers over the C calls; no neural-network modules, optimizers, or model loading.
- **Policy-free**: no memory limits, no Metal library path selection, no scheduling opinions. Bring your own policy on top.
- **Exact ownership**: array and stream handles are move-only with deterministic release — when a handle drops, its C resource is released, with no hidden reference counting to surprise memory-sensitive code.
- **Errors, not exits**: MLX-C's non-terminating error handler is captured thread-locally and surfaced as `MlxCError` values carrying the attempted operation and MLX's reported description.

What this crate is *not*: a high-level framework. For neural-network layers, optimizers, and model utilities, look at [mlx-rs](https://github.com/oxiglade/mlx-rs). The two projects occupy different layers and can be used side by side.

## Status

Early public extraction, versioned to mirror the vendored MLX-C release (crate `0.7.0` binds MLX-C `v0.7.0`, MLX `v0.32.3`). The bridge was hardened inside a production local-model serving stack before extraction. The completeness guarantee covers symbol *presence*; per-wrapper behavioral test coverage is growing and tracked in `COVERAGE_INVENTORY.md`.

## Building

Requirements: a Rust toolchain (pinned in `rust-toolchain.toml`), Clang for bindgen, and an MLX-C library to link against.

Bindgen always runs against the headers vendored under `vendor/mlx-c/` — no network, no system headers. For linking a binary that uses the crate, the build script looks for MLX-C in this order:

1. `MLX_C_RUST_MLX_C_PREFIX` — a prefix with `lib/` containing the library and, if needed, `include/`.
2. `pkg-config mlx-c`.

If neither is found the crate still checks and compiles (documentation builds work); linking a test or binary then reports undefined symbols until MLX-C is provided. Building MLX and MLX-C from source is a CMake build of the upstream repositories; see their documentation.

## Versioning

The crate version names the **MLX release it is verified against** (the scheme mlx-rs established for user-facing crates): crate `0.32.3.x` is verified against MLX `v0.32.3`. The bound MLX-C release rides in the README headline and `COVERAGE_INVENTORY.md`. Both upstreams are pre-1.0, so minor releases may carry breaking changes when upstream does; our own fixes and same-MLX MLX-C surface changes ride as patch bumps.

## Attribution

The fallible-closure error channel — a payload trampoline parking its typed failure in a thread-local slot that the caller's status check prefers over MLX-C's own status error — adopts the pattern used by [mlx-rs](https://github.com/oxiglade/mlx-rs) (MIT OR Apache-2.0). The pattern is adopted; no mlx-rs code is included. MLX and MLX-C are © Apple Inc., MIT licensed.

## License

Dual licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at your option, matching the widest Rust ecosystem norms. Contribution happens under the same terms.
