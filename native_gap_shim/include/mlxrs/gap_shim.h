#ifndef MLXRS_GAP_SHIM_H
#define MLXRS_GAP_SHIM_H

/* The mlx-c-rust gap shim: this repository's own C ABI over the pinned
 * MLX C++ surface, carrying exactly the functions the vendored MLX-C pin
 * has not wrapped yet. It exists so an MLX capability becomes reachable
 * from Rust the day the MLX pin moves, instead of the day MLX-C happens
 * to catch up; when an MLX-C release wraps a function, the shim entry
 * retires (the audit-upstream-surface tool flags redundant entries).
 *
 * Conventions are MLX-C's own, so the bridge treats shim entry points
 * exactly like MLX-C ones: status-returning functions that report through
 * MLX's installed error handler, MLX-C handle types in and out, and
 * multi-output results as res_0/res_1 out parameters (the eig/qr
 * precedent). The prefix is mlxrs_ so shim symbols can never collide with
 * a future MLX-C symbol.
 *
 * Consumers who own their native image (default-features = false, or a
 * prefix of their own) compile this one-file shim themselves: it is
 * self-contained — public headers only, no MLX-C private headers — and
 * links the same mlx and mlxc libraries any consumer already links.
 */

#include <stdbool.h>
#include <stddef.h>

#include "mlx/c/mlx.h"

#ifdef __cplusplus
extern "C" {
#endif

/* The element-wise minimum and maximum of `a` reduced over `axes`
 * (MLX core minmax, the axes overload). res_0 receives the minima,
 * res_1 the maxima. */
int mlxrs_minmax_axes(
    mlx_array* res_0,
    mlx_array* res_1,
    const mlx_array a,
    const int* axes,
    size_t axes_num,
    bool keepdims,
    const mlx_stream s);

/* The minimum and maximum of `a` along one `axis` (MLX core minmax, the
 * axis overload). res_0 receives the minima, res_1 the maxima. */
int mlxrs_minmax_axis(
    mlx_array* res_0,
    mlx_array* res_1,
    const mlx_array a,
    int axis,
    bool keepdims,
    const mlx_stream s);

/* Transposes the last two dimensions of `a` (MLX core matrix_transpose). */
int mlxrs_matrix_transpose(mlx_array* res, const mlx_array a, const mlx_stream s);

/* The unique elements of `a` (MLX core unique). `size` is the exact
 * unique count the result carries: when the array holds fewer uniques
 * the result is padded (with `fill_value` when supplied).
 * `return_index`, `return_inverse`, and `return_counts` append their
 * arrays to the returned vector, exactly as the C++ function appends
 * them. */
int mlxrs_unique(
    mlx_vector_array* res,
    const mlx_array a,
    int size,
    bool return_index,
    bool return_inverse,
    bool return_counts,
    const mlx_array fill_value /* may be null */,
    const mlx_stream s);

#ifdef __cplusplus
}
#endif

#endif /* MLXRS_GAP_SHIM_H */
