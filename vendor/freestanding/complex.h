/* Freestanding complex for the self-contained bindgen parse.
 *
 * Under clang the complex types are built into the compiler: array.h's
 * own MSVC-versus-everyone-else conditional selects `float _Complex` and
 * `double _Complex` directly, and the vendored headers use no complex
 * macros or functions. This stub exists only so the #include resolves
 * without any host libc.
 */

#ifndef MLX_C_RUST_FREESTANDING_COMPLEX_H
#define MLX_C_RUST_FREESTANDING_COMPLEX_H

#endif
