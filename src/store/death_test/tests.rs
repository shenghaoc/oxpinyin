extern "C" fn panic_in_a_c_boundary() {
    panic!("an unrelated panic");
}

// assert_aborts must reject a panic even at a non-unwinding C boundary.
#[test]
fn a_panic_at_the_c_boundary_is_not_the_pins_abort() {
    const NAME: &str = "store::death_test::tests::a_panic_at_the_c_boundary_is_not_the_pins_abort";
    let parent = std::panic::catch_unwind(|| {
        super::assert_aborts(NAME, || panic_in_a_c_boundary());
    });
    let message = parent.expect_err("a panic must not pass as an abort");
    let message = message
        .downcast_ref::<String>()
        .expect("the helper's assertion message");
    assert!(message.contains("the operation panicked"), "{message}");
    assert!(message.contains("an unrelated panic"), "{message}");
}
