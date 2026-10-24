//! Gap-shim wrappers: the Rust path to MLX functions the vendored MLX-C
//! pin has not wrapped.
//!
//! Each wrapper calls this repository's own `mlxrs_*` C ABI (the
//! `native_gap_shim` component compiled by the native build engine) with
//! the same owned-handle and captured-error semantics as every MLX-C
//! family. The shim exists so an MLX capability is reachable the day the
//! pin moves; when an MLX-C release wraps a function, the shim entry
//! retires and the wrapper moves to the matching MLX-C family — the
//! audit-upstream-surface tool flags both the retireable entry and the
//! still-missing wrapper.

use crate::raw;
use crate::{MlxArray, MlxArrayVector, MlxBindingsContext, MlxCError};

impl MlxBindingsContext {
    /// Applies the gap shim `mlxrs_minmax_axes` (MLX core `minmax`, the
    /// axes overload) on the runtime GPU stream, producing the minima and
    /// the maxima.
    pub fn minmax_axes(
        &self,
        a: &MlxArray,
        axes: &[i32],
        keepdims: bool,
    ) -> Result<(MlxArray, MlxArray), MlxCError> {
        self.output_array_pair(
            "apply the gap shim minmax over axes",
            |minima, maxima, stream| {
                // SAFETY: Inputs and stream are live and both outputs are
                // uniquely writable.
                unsafe {
                    raw::mlxrs_minmax_axes(
                        minima,
                        maxima,
                        a.raw(),
                        axes.as_ptr(),
                        axes.len(),
                        keepdims,
                        stream,
                    )
                }
            },
        )
    }

    /// Applies the gap shim `mlxrs_minmax_axis` (MLX core `minmax`, the
    /// axis overload) on the runtime GPU stream, producing the minima and
    /// the maxima.
    pub fn minmax_axis(
        &self,
        a: &MlxArray,
        axis: i32,
        keepdims: bool,
    ) -> Result<(MlxArray, MlxArray), MlxCError> {
        self.output_array_pair(
            "apply the gap shim minmax along one axis",
            |minima, maxima, stream| {
                // SAFETY: Inputs and stream are live and both outputs are
                // uniquely writable.
                unsafe { raw::mlxrs_minmax_axis(minima, maxima, a.raw(), axis, keepdims, stream) }
            },
        )
    }

    /// Applies the gap shim `mlxrs_matrix_transpose` (MLX core
    /// `matrix_transpose`) on the runtime GPU stream.
    pub fn matrix_transpose(&self, a: &MlxArray) -> Result<MlxArray, MlxCError> {
        self.output_array("apply the gap shim matrix_transpose", |output, stream| {
            // SAFETY: Inputs and stream are live and output is uniquely writable.
            unsafe { raw::mlxrs_matrix_transpose(output, a.raw(), stream) }
        })
    }

    /// Applies the gap shim `mlxrs_unique` (MLX core `unique`) on the
    /// runtime GPU stream. `size` is the exact unique count the result
    /// carries: fewer actual uniques get padded (with `fill_value` when
    /// supplied), so callers either know the count a priori or pass the
    /// array size and trim. The returned vector carries the unique values
    /// first, then the `return_index`, `return_inverse`, and
    /// `return_counts` arrays in that order when requested.
    pub fn unique(
        &self,
        a: &MlxArray,
        size: i32,
        return_index: bool,
        return_inverse: bool,
        return_counts: bool,
        fill_value: Option<&MlxArray>,
    ) -> Result<MlxArrayVector, MlxCError> {
        // The nullable-handle convention from the shim's C ABI: an empty
        // owned handle carries a null ctx, which the shim reads as
        // std::nullopt on the C++ side.
        let no_fill_value = MlxArray::empty();
        let fill_value_argument = fill_value
            .map(MlxArray::raw)
            .unwrap_or_else(|| no_fill_value.raw());
        self.output_vector_array("apply the gap shim unique", |output, stream| {
            // SAFETY: Inputs and stream are live and output is uniquely
            // writable; the empty handle passed for no fill value is a
            // plain non-owning value copy.
            unsafe {
                raw::mlxrs_unique(
                    output,
                    a.raw(),
                    size,
                    return_index,
                    return_inverse,
                    return_counts,
                    fill_value_argument,
                    stream,
                )
            }
        })
    }
}
