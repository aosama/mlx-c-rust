/* Freestanding stdlib for the self-contained bindgen parse.
 *
 * The vendored MLX-C headers use exactly one thing from stdlib: size_t
 * (the byte-and-element count returns in array.h). size_t belongs to
 * stddef, which clang's builtin headers provide for every target, so this
 * stub simply re-exports it.
 */

#ifndef MLX_C_RUST_FREESTANDING_STDLIB_H
#define MLX_C_RUST_FREESTANDING_STDLIB_H

#include <stddef.h>

#endif
