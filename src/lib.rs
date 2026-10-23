//! Safe ownership boundary around the official MLX C API.
//!
//! This crate owns exactly one responsibility: the MLX-C C ABI translated
//! into idiomatic Rust. It holds the bindgen-generated raw declarations, the
//! captured-error machinery, the owned handle types (`MlxArray`, `MlxDtype`,
//! `MlxArrayVector`, `MlxStream`), the per-worker bindings context
//! (`MlxBindingsContext`: GPU stream and linked version), and every operation
//! wrapper family: creation, shape, padding, elementwise math, activation,
//! normalization, convolution, random, rope, quantized operations,
//! quantization construction, NVFP4, attention, the compiled-graph family,
//! the Metal kernels and capture, and the general operation set. It contains
//! no embedding policy: memory limits, the metallib path selection and
//! verification, safetensors vtable policies, and process I/O accounting
//! belong to the embedding application, which links this crate against an
//! MLX-C library and converts `MlxCError` values into its own typed runtime
//! errors at its boundary.
//!
//! # API-surface completeness
//!
//! The bridge is complete, not selective: every public function and type the
//! vendored MLX-C headers declare is carried here. The build script
//! translates the vendored tree in full and emits a bridged-symbol inventory
//! alongside the declarations, so any upstream pin bump that adds, removes,
//! or renames surface shows up as an inventory change the coverage contract
//! can fail on. The recorded state for the current pin lives in this crate's
//! `COVERAGE_INVENTORY.md`.
//!
//! Source-reference audits must never drive removals here: a wrapper with no
//! caller yet is the expected steady state of a complete bridge — the crate
//! exists precisely so that every MLX-C capability is reachable without
//! waiting for a wrapper to be written on demand.
//!
//! # Attribution
//!
//! The fallible-closure error channel — a payload trampoline parking its
//! typed failure in a thread-local slot that the caller's status check
//! prefers over MLX-C's own status error — adopts the pattern used by
//! mlx-rs (MIT OR Apache-2.0). The pattern is adopted; no mlx-rs code is
//! included.
//!
//! # Test placement
//!
//! Tests that never touch MLX-C symbols (the error-channel machinery) live
//! in this crate's test tree. Anything exercising the bindings themselves
//! needs an MLX-C library at link time, discovered through the
//! `MLX_C_RUST_MLX_C_PREFIX` environment variable or `pkg-config mlx-c`
//! (see the build script and README); without one, library compilation and
//! documentation builds still succeed and only test binaries fail to link.

mod bindings;
mod compile;
mod metal;
mod operations;
mod runtime;
mod types;

pub mod error;
pub mod raw;

pub use bindings::context::MlxBindingsContext;
pub use compile::elementwise_graphs::MlxCompiledElementwiseGraphs;
pub use compile::gated_delta_rate_activation::MlxCompiledGatedDeltaRateActivation;
pub use compile::graph::{
    MlxGraphBuilder, array_from_vector, graph_output_array, set_graph_output,
    set_graph_output_vector,
};
pub use compile::multi_output_graph::MlxCompiledMultiOutputGraph;
pub use compile::norm_fusion::MlxCompiledDualHeadNorm;
pub use compile::swiglu::MlxCompiledSwiGlu;
pub use compile::transforms::{
    MlxCompileCache, disable_compile, enable_compile, jvp_of_closure, set_compile_mode,
    value_and_grad_of_closure, vjp_of_closure, vmap_replace, vmap_trace,
};
pub use error::{
    MlxCError, check_status, clear_captured_mlx_error, install_non_terminating_error_handler,
    take_captured_mlx_error,
};
pub use metal::cuda_kernel::{MlxCudaKernel, MlxCudaKernelConfig};
pub use metal::kernel::{
    MlxMetalKernel, MlxMetalKernelConfig, MlxMetalKernelOutput, MlxMetalKernelTemplateArgument,
    apply_metal_kernel_in_graph,
};
pub use metal::metallib::set_metallib_path;
pub use runtime::export::{
    GraphOutputFile, MlxFunctionExporter, MlxImportedFunction, MlxNodeNamer, export_function,
    export_function_with_keywords, export_graph_to_dot, print_graph,
};
pub use runtime::io::{
    MlxIoGguf, MlxIoReader, MlxIoWriter, load_array_from_reader, load_safetensors,
    load_safetensors_from_reader, save_array_to_writer, save_safetensors,
    save_safetensors_to_writer,
};
pub use runtime::memory::{
    active_memory_bytes, cache_memory_bytes, clear_allocator_cache, memory_limit_bytes,
    peak_memory_bytes, reset_peak_memory, set_cache_limit, set_memory_limit, synchronize,
};
pub use runtime::platform::{
    MlxDistributedGroup, cuda_is_available, distributed_is_available, initialize_distributed,
    metal_is_available, metallib_path, set_wired_limit,
};
pub use runtime::stream_defaults::{
    MlxStreamThreadLocal, clear_streams, set_default_stream, synchronize_all_defaults,
};
pub use types::array::{MlxArray, MlxDtype};
pub use types::array_vector::MlxArrayVector;
pub use types::closure_custom::{MlxClosureCustom, MlxClosureCustomJvp, MlxClosureCustomVmap};
pub use types::closures::{MlxClosure, MlxClosureKwargs, MlxClosureValueAndGrad};
pub use types::device::{MlxDeviceHandle, MlxDeviceInfo};
pub use types::maps::{
    MlxMapStringToArray, MlxMapStringToArrayIterator, MlxMapStringToString,
    MlxMapStringToStringIterator,
};
pub use types::stream::MlxStream;
pub use types::string::MlxString;
pub use types::vectors::{MlxVectorInt, MlxVectorStream, MlxVectorString, MlxVectorVectorArray};
