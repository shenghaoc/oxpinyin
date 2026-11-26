#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::support::{
    TempUserDir, assert_aborts, cstr, open_with_user, zhuyin_parse_more_full_pinyins,
};

use std::ptr;

// With no keys, fill_matrix leaves the matrix empty (phonetic_key_matrix.cpp:30-38).
// The wrapped size-minus-one check reaches the column assertion
// (zhuyin.cpp:1886-1895; phonetic_key_matrix.h:102-103 at 074a2219).
macro_rules! full_pinyin_keyless_query_aborts {
    ($name:ident, $input:literal, $length:expr) => {
        #[test]
        fn $name() {
            assert_aborts(concat!("matrix_bounds::", stringify!($name)), || {
                let user = TempUserDir::new(stringify!($name));
                let (_context, instance) = open_with_user(&user.path);
                assert_eq!(
                    unsafe { zhuyin_parse_more_full_pinyins(instance, cstr($input).as_ptr()) },
                    $length
                );
                assert_eq!(
                    unsafe { crate::support::zhuyin_get_parsed_input_length(instance) },
                    $length
                );
                let mut key = ptr::null_mut();
                let _slots = crate::support::key_slot_guard();
                unsafe { crate::support::zhuyin_get_zhuyin_key(instance, 0, &raw mut key) };
            });
        }
    };
}

full_pinyin_keyless_query_aborts!(full_pinyin_ni_separator_key_aborts, "ni'", 0);
