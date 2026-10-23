//! Safe wrapper over the MLX AOT metallib path override.

use std::ffi::{CStr, c_char};

use crate::{MlxCError, error::check_status, raw};

/// Overrides the metallib MLX loads.
///
/// The policy owner resolves and verifies WHICH metallib to load; this wrapper
/// only performs the official override call.
pub fn set_metallib_path(path: &CStr) -> Result<(), MlxCError> {
    let nul_terminated_path: *const c_char = path.as_ptr();
    // SAFETY: The official C API copies the non-null NUL-terminated path and
    // does not retain the borrowed pointer after returning.
    let status = unsafe { raw::mlx_metal_set_metallib_path(nul_terminated_path) };
    check_status(status, "set the MLX AOT metallib path")
}
