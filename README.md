# mlx-c-rust

[![Crates.io](https://img.shields.io/crates/v/mlx-c-rust.svg)](https://crates.io/crates/mlx-c-rust)
[![Documentation](https://docs.rs/mlx-c-rust/badge.svg)](https://docs.rs/mlx-c-rust)

A complete, low-level, policy-free Rust bridge to the public [MLX-C](https://github.com/ml-explore/mlx-c) C API of Apple's [MLX](https://github.com/ml-explore/mlx) array framework for Apple silicon.

**Current release: [`0.32.4-alpha.3`](https://crates.io/crates/mlx-c-rust/0.32.4-alpha.3) — verified against MLX `v0.32.4`, binding the complete MLX-C `v0.7.0` surface and carrying it forward to that MLX with this repository's reviewed bridge adaptations, plus the gap shim for the MLX functions that MLX-C pin has not wrapped.** The crate version always names the MLX release it is verified against — the number you search for and install — while the bound MLX-C release and the forward adaptations are recorded here and in `COVERAGE_INVENTORY.md`. Published on [crates.io](https://crates.io/crates/mlx-c-rust); documentation builds on [docs.rs](https://docs.rs/mlx-c-rust).

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

Early public extraction, versioned to name the verified MLX release (crate `0.32.4` is verified against MLX `v0.32.4`, binding MLX-C `v0.7.0` carried forward by this repository's reviewed bridge adaptations). The bridge was hardened inside a production local-model serving stack before extraction. The completeness guarantee covers symbol *presence*; per-wrapper behavioral test coverage is growing and tracked in `COVERAGE_INVENTORY.md`.

## Building

Nothing to configure: `cargo add mlx-c-rust`, then `cargo build`. With no environment set, the build script's typed native engine downloads the pinned MLX and MLX-C source releases (SHA-256 verified), compiles them once with CMake into a **machine-shared, identity-keyed prefix** (`~/.cache/mlx-c-rust/native/<identity>/`) — statically, so consumer binaries embed the pair and load no MLX dylibs at runtime; Metal kernels are read from the metallib inside the same prefix (the path is baked into the compiled code), so the prefix is self-sufficient and the build tree is disposable. The pair compiles once per machine per pin, and every project using the crate reuses it. Requirements: an Apple Silicon Mac, the Rust toolchain pinned in `rust-toolchain.toml`, Clang for bindgen, and `curl`, `tar`, and `cmake` on PATH (a standard developer machine; `brew install cmake` covers the one that is usually missing).

Working in this repository, the same engine is available as typed tooling: `cargo xtask build-native` (add `--force` to recompile) builds the pair up front with live progress, so later builds reuse it in milliseconds.

To link against an MLX-C you provide yourself instead, in first-match order:

1. `MLX_C_RUST_MLX_C_PREFIX` — a prefix whose `lib/` holds the libraries (an embedded rpath means no run-time environment variables).
2. The standard Homebrew prefixes, after `brew install mlx-c`.
3. `pkg-config mlx-c`.

**Bindings-only mode** — consumers whose native image is deliberately not any library this crate can discover (for example a pinned, patched MLX fork from their own native orchestration) take the bridge alone: `mlx-c-rust = { version = "…", default-features = false }`. The build script then emits zero link directives — no ladder, no pair build, no side effects — and the consumer's own build supplies and links its native libraries. Everything else (declarations, captured-error translation, handle types, operation families) is identical.

## Versioning

The crate version names the **MLX release it is verified against** (the scheme mlx-rs established for user-facing crates): crate `0.32.4.x` is verified against MLX `v0.32.4`. The bound MLX-C release rides in the README headline and `COVERAGE_INVENTORY.md`. MLX-C is the carrier for the C bridge, not the pace-setter: when no MLX-C release pairs with the newest MLX, the native build engine applies this repository's reviewed forward adaptations to the pinned MLX-C sources, and drops them when an MLX-C release catches up — so the crate tracks MLX releases even ahead of MLX-C. Both upstreams are pre-1.0, so minor releases may carry breaking changes when upstream does; our own fixes and same-MLX MLX-C surface changes ride as patch bumps.

**The gap shim** — a pinned MLX-C release can lag the pinned MLX, and where it does this repository carries its own C ABI instead of waiting: the gap shim (`native_gap_shim/`, compiled by the native build engine as the third prefix component) wraps the missing MLX functions behind `mlxrs_*` entry points with MLX-C's exact conventions, and the bridge exposes them through the same owned-handle, captured-error wrappers as every MLX-C family. When an MLX-C release wraps a function, the shim entry retires — `cargo xtask audit-upstream-surface` (upstream leg with `--mlx-source <mlx checkout>`, record legs always, also run by `cargo xtask verify`) flags both missing wrappers and entries whose MLX-C equivalent has arrived.

## Attribution

The error handling follows the usage pattern MLX-C itself documents — installing a non-terminating handler that records the message before the nonzero status returns, extended with a thread-local slot so payload closures can park their typed failures across the C boundary. MLX and MLX-C are © Apple Inc., MIT licensed.

## License

Dual licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at your option, matching the widest Rust ecosystem norms. Contribution happens under the same terms.
