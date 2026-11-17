/// A panic that reaches this non-unwinding function aborts the process,
/// as a facade panic inside an entry point would.
extern "C" fn panic_in_an_entry_point() {
    panic!("an unrelated panic");
}

/// The witness: a death test whose operation panics at a C boundary
/// fails with its own message, not as the pin's abort.
#[test]
fn a_panic_at_the_c_boundary_is_not_the_pins_abort() {
    const NAME: &str = "support_tests::a_panic_at_the_c_boundary_is_not_the_pins_abort";
    let parent = std::panic::catch_unwind(|| {
        let _ = crate::support::assert_aborts(NAME, || panic_in_an_entry_point());
    });
    let message = parent.expect_err("a panic must not pass as an abort");
    let message = message
        .downcast_ref::<String>()
        .expect("the helper's assertion message");
    assert!(message.contains("OperationPanicked"), "{message}");
    assert!(message.contains("an unrelated panic"), "{message}");
}
