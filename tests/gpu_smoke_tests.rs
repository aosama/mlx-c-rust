//! GPU smoke contracts: one real computation through the default GPU stream,
//! asserting the round trip so a regression fails the lane instead of only
//! printing. Continuous integration runs these on a macOS runner whose
//! virtual machine exposes the Metal device (verified by the metal_probe
//! example); locally they require an MLX-C library (see the README).

use mlx_c_rust::{MlxBindingsContext, install_non_terminating_error_handler};

#[test]
fn should_execute_an_elementwise_addition_on_the_gpu_stream() {
    install_non_terminating_error_handler();
    let context = MlxBindingsContext::new().expect("the bindings context should initialize");

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

    assert_eq!(sum_values, vec![11.0, 22.0, 33.0, 44.0]);
}

#[test]
fn should_report_the_linked_mlx_version() {
    install_non_terminating_error_handler();
    let context = MlxBindingsContext::new().expect("the bindings context should initialize");
    assert!(
        !context.version().is_empty(),
        "the linked MLX version string must not be empty"
    );
}
