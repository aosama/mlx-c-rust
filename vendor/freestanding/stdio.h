/* Freestanding FILE for the self-contained bindgen parse.
 *
 * The vendored MLX-C headers use FILE solely as an opaque parameter type
 * (the two graph-printing functions in graph_utils.h) and never touch a
 * FILE's internals. Defining it as an undeclared struct keeps the parse
 * host-free AND stops bindgen from pulling the host's real FILE layout
 * into the generated bindings — previously a full 152-byte macOS SDK
 * __sFILE struct on Macs and glibc's _IO_FILE on Linux, differing between
 * hosts while nothing in the crate ever used it. The struct name
 * deliberately avoids the mlx_ prefix so it cannot join the allowlisted
 * bridged-symbol inventory.
 */

#ifndef MLX_C_RUST_FREESTANDING_STDIO_H
#define MLX_C_RUST_FREESTANDING_STDIO_H

typedef struct freestanding_file FILE;

#endif
