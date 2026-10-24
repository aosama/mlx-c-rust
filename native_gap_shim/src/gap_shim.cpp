/* The mlx-c-rust gap shim implementation. See gap_shim.h for the contract
 * and the retirement policy. The handle casts below inline what MLX-C's
 * private headers do: the public C struct carries the C++ object as a
 * void* ctx (that layout is the published C ABI), so casting ctx is both
 * self-contained — this file compiles against any installed MLX-C, no
 * private headers — and exactly what the vendored MLX-C itself does.
 */

#include "mlxrs/gap_shim.h"

#include <optional>
#include <utility>
#include <vector>

#include "mlx/c/error.h"
#include "mlx/mlx.h"
#include "mlx/ops.h"

namespace {

inline mlx::core::array& shim_array_get(const mlx_array handle) {
  if (!handle.ctx) {
    throw std::runtime_error("expected a non-empty mlx_array");
  }
  return *static_cast<mlx::core::array*>(handle.ctx);
}

inline void shim_array_set(mlx_array& destination, mlx::core::array&& value) {
  if (destination.ctx) {
    *static_cast<mlx::core::array*>(destination.ctx) = std::move(value);
  } else {
    destination.ctx = new mlx::core::array(std::move(value));
  }
}

inline std::vector<mlx::core::array>& shim_vector_array_get(
    const mlx_vector_array handle) {
  if (!handle.ctx) {
    throw std::runtime_error("expected a non-empty mlx_vector_array");
  }
  return *static_cast<std::vector<mlx::core::array>*>(handle.ctx);
}

inline void shim_vector_array_set(
    mlx_vector_array& destination,
    std::vector<mlx::core::array>&& value) {
  if (destination.ctx) {
    *static_cast<std::vector<mlx::core::array>*>(destination.ctx) =
        std::move(value);
  } else {
    destination.ctx = new std::vector<mlx::core::array>(std::move(value));
  }
}

inline mlx::core::Stream& shim_stream_get(const mlx_stream handle) {
  if (!handle.ctx) {
    throw std::runtime_error("expected a non-empty mlx_stream");
  }
  return *static_cast<mlx::core::Stream*>(handle.ctx);
}

} // namespace

extern "C" int mlxrs_minmax_axes(
    mlx_array* res_0,
    mlx_array* res_1,
    const mlx_array a,
    const int* axes,
    size_t axes_num,
    bool keepdims,
    const mlx_stream s) {
  try {
    auto [minima, maxima] = mlx::core::minmax(
        shim_array_get(a),
        std::vector<int>(axes, axes + axes_num),
        keepdims,
        shim_stream_get(s));
    shim_array_set(*res_0, std::move(minima));
    shim_array_set(*res_1, std::move(maxima));
  } catch (std::exception& error) {
    mlx_error("%s", error.what());
    return 1;
  }
  return 0;
}

extern "C" int mlxrs_minmax_axis(
    mlx_array* res_0,
    mlx_array* res_1,
    const mlx_array a,
    int axis,
    bool keepdims,
    const mlx_stream s) {
  try {
    auto [minima, maxima] =
        mlx::core::minmax(shim_array_get(a), axis, keepdims, shim_stream_get(s));
    shim_array_set(*res_0, std::move(minima));
    shim_array_set(*res_1, std::move(maxima));
  } catch (std::exception& error) {
    mlx_error("%s", error.what());
    return 1;
  }
  return 0;
}

extern "C" int mlxrs_matrix_transpose(
    mlx_array* res,
    const mlx_array a,
    const mlx_stream s) {
  try {
    shim_array_set(
        *res,
        mlx::core::matrix_transpose(shim_array_get(a), shim_stream_get(s)));
  } catch (std::exception& error) {
    mlx_error("%s", error.what());
    return 1;
  }
  return 0;
}

extern "C" int mlxrs_unique(
    mlx_vector_array* res,
    const mlx_array a,
    int size,
    bool return_index,
    bool return_inverse,
    bool return_counts,
    const mlx_array fill_value,
    const mlx_stream s) {
  try {
    std::optional<mlx::core::array> fill;
    if (fill_value.ctx) {
      fill = shim_array_get(fill_value);
    }
    shim_vector_array_set(
        *res,
        mlx::core::unique(
            shim_array_get(a),
            size,
            return_index,
            return_inverse,
            return_counts,
            fill,
            shim_stream_get(s)));
  } catch (std::exception& error) {
    mlx_error("%s", error.what());
    return 1;
  }
  return 0;
}
