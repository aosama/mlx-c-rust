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

Nothing to configure: `cargo add mlx-c-rust`, then `cargo build`. With no environment set, the build script's typed native engine downloads the pinned MLX and MLX-C source releases (SHA-256 verified), compiles them once with CMake into a **machine-shared, identity-keyed prefix** (`~/.cache/mlx-c-rust/native/<identity>/`), and links against the result — so the pair compiles once per machine per pin, and every project using the crate reuses it. Requirements: an Apple Silicon Mac, the Rust toolchain pinned in `rust-toolchain.toml`, Clang for bindgen, and `curl`, `tar`, and `cmake` on PATH (a standard developer machine; `brew install cmake` covers the one that is usually missing).

Working in this repository, the same engine is available as typed tooling: `cargo xtask build-native` (add `--force` to recompile) builds the pair up front with live progress, so later builds reuse it in milliseconds.

To link against an MLX-C you provide yourself instead, in first-match order:

1. `MLX_C_RUST_MLX_C_PREFIX` — a prefix whose `lib/` holds the libraries (an embedded rpath means no run-time environment variables).
2. The standard Homebrew prefixes, after `brew install mlx-c`.
3. `pkg-config mlx-c`.

## Versioning

The crate version names the **MLX release it is verified against** (the scheme mlx-rs established for user-facing crates): crate `0.32.3.x` is verified against MLX `v0.32.3`. The bound MLX-C release rides in the README headline and `COVERAGE_INVENTORY.md`. Both upstreams are pre-1.0, so minor releases may carry breaking changes when upstream does; our own fixes and same-MLX MLX-C surface changes ride as patch bumps.

## Attribution

The error handling follows the usage pattern MLX-C itself documents — installing a non-terminating handler that records the message before the nonzero status returns, extended with a thread-local slot so payload closures can park their typed failures across the C boundary. MLX and MLX-C are © Apple Inc., MIT licensed.

## License

Dual licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at your option, matching the widest Rust ecosystem norms. Contribution happens under the same terms.
