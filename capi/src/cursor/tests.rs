#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::parse::pinyin_parse_more_full_pinyins;

use crate::test_support::{TempUserDir, cstr, key_slot_guard, open};

use crate::types::{ChewingKey, ChewingKeyRest};

use std::ptr;

/// The pin's key and key-rest slots are function-local `static`s
/// (`pinyin.cpp:2936`, `:2960`): one slot for the whole process, so a
/// second instance's call overwrites what the first was handed, and the
/// pointer survives the instance. The key-rest length is a `guint16`
/// difference, which wraps on a reversed rest.
#[test]
fn the_key_slots_are_process_wide_and_the_rest_length_wraps() {
    let _slots = key_slot_guard();
    let user_dir = TempUserDir::new("preedit-static-slots");
    let (context, first) = open(user_dir.path.to_str().expect("UTF-8 path"));
    let second = unsafe { crate::instance::pinyin_alloc_instance(context) };
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(first, cstr("ni").as_ptr()) },
        2
    );
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(second, cstr("hao").as_ptr()) },
        3
    );

    let (mut a, mut b): (*mut ChewingKey, *mut ChewingKey) = (ptr::null_mut(), ptr::null_mut());
    assert!(unsafe { super::pinyin_get_pinyin_key(first, 0, &raw mut a) });
    // SAFETY: `a` is the slot the call above handed out.
    let ni = unsafe { *a };
    assert!(unsafe { super::pinyin_get_pinyin_key(second, 0, &raw mut b) });
    assert_eq!(a, b, "one slot for every instance");
    // SAFETY: as above.
    assert_ne!(unsafe { *a }, ni, "the second call overwrote the first");

    let (mut ra, mut rb): (*mut ChewingKeyRest, *mut ChewingKeyRest) =
        (ptr::null_mut(), ptr::null_mut());
    assert!(unsafe { super::pinyin_get_pinyin_key_rest(first, 0, &raw mut ra) });
    assert!(unsafe { super::pinyin_get_pinyin_key_rest(second, 0, &raw mut rb) });
    assert_eq!(ra, rb);

    let mut length = 0_u16;
    let mut reversed = ChewingKeyRest { begin: 5, end: 2 };
    assert!(unsafe {
        super::pinyin_get_pinyin_key_rest_length(first, &raw mut reversed, &raw mut length)
    });
    assert_eq!(length, 65533, "end - begin in guint16");

    unsafe { crate::instance::pinyin_free_instance(first) };
    unsafe { crate::instance::pinyin_free_instance(second) };
    // SAFETY: the slot is a `static`: the pointer outlives the instances.
    assert_ne!(unsafe { *a }, ni);
    unsafe { crate::context::pinyin_fini(context) };
}
