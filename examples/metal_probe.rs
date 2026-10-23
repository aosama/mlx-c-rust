//! Probes this machine's MLX setup: Metal availability, the linked MLX
//! version, and one real computation on the default GPU stream.
//!
//! Run with `cargo run --example metal_probe` (an MLX-C library must be
//! discoverable; see the README). On continuous integration this doubles as
//! the evidence step for whether the runner's virtual machine exposes the
//! Metal device to guest workloads.

use mlx_c_rust::{MlxBindingsContext, install_non_terminating_error_handler, metal_is_available};

fn main() {
    install_non_terminating_error_handler();
    match metal_is_available() {
        Ok(is_available) => println!("metal available: {is_available}"),
        Err(probe_error) => println!("metal availability probe failed: {probe_error}"),
    }

    let context = MlxBindingsContext::new().expect("the MLX bindings context should initialize");
    println!("linked MLX version: {}", context.version());

    let left_addend = context
        .array_from_f32(&[1.0, 2.0, 3.0, 4.0], &[4])
        .expect("the left addend array should be valid");
    let right_addend = context
        .array_from_f32(&[10.0, 20.0, 30.0, 40.0], &[4])
        .expect("the right addend array should be valid");
    let elementwise_sum = context
        .add(&left_addend, &right_addend)
        .expect("the elementwise addition should build on the GPU stream");
    elementwise_sum
        .evaluate()
        .expect("the sum should evaluate on the GPU stream");
    let sum_values = elementwise_sum
        .to_vec_f32()
        .expect("the evaluated sum should copy back to host memory");
    println!("gpu stream elementwise sum: {sum_values:?}");
}
