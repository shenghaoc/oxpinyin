//! The token getters: phrases, readings, pronunciation counts and caller
//! arrays.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::support::{
    TempUserDir, ZhuyinInstance, close, cstr, import, open, open_with_user, system_dir,
    take_sentence, tokens_of, ucs4_key, write_user_conf, zhuyin_save,
    zhuyin_token_get_n_pronunciation, zhuyin_token_get_nth_pronunciation, zhuyin_token_get_phrase,
    *,
};

use std::os::raw::c_char;

use std::ptr;

/// # Safety
///
/// `instance` is NULL or a live instance: `zhuyin_alloc_instance` returned it and
/// `zhuyin_free_instance` has not freed it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
unsafe fn n_pronunciation(instance: *mut ZhuyinInstance, token: u32) -> (bool, u32) {
    let mut count = UNTOUCHED;
    // SAFETY: `instance` meets this function's `# Safety` contract, and `count` is a local `u32`,
    // valid and aligned for the write the getter makes.
    let ret = unsafe { zhuyin_token_get_n_pronunciation(instance, token, &raw mut count) };
    (ret, count)
}

/// The values a GLib clear callback saw.
static CLEARED: std::sync::Mutex<Vec<u32>> = std::sync::Mutex::new(Vec::new());

/// Records the low two bytes of the element that GLib clears.
///
/// # Safety
///
/// GLib calls it as the array's clear function: `element` points to one live element of that
/// array, two or four bytes wide, valid for reads for the call.
unsafe extern "C" fn clear_element(element: glib_sys::gpointer) {
    // SAFETY: GLib hands the callback one live element; the arrays here hold
    // four-byte or two-byte elements, and both start with their low bytes.
    let low = unsafe { element.cast::<u16>().read_unaligned() };
    CLEARED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(u32::from(low));
}

fn take_cleared() -> Vec<u32> {
    std::mem::take(
        &mut *CLEARED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    )
}

/// A stored word at or above 0x80000000 makes `g_ucs4_to_utf8` answer NULL,
/// but `_token_get_phrase` still writes the stored length and answers true
/// (`zhuyin.cpp:1104-1113`).
#[test]
fn a_phrase_glib_cannot_convert_answers_its_length_and_null() {
    use oxpinyin::data::phrase_library::{ChunkItem, build_chunk};

    let user = TempUserDir::new("user-file-null-text");
    write_user_conf(&user.path, &system_dir());
    let item = ChunkItem {
        phrase: vec![0x4f60, 0x8000_0000],
        unigram: 17,
        prons: vec![(vec![0x302b, 0x30a7], 5)],
    };
    std::fs::write(
        user.path.join("user.bin"),
        build_chunk(&[(1, &item)]).expect("one-item user chunk"),
    )
    .expect("write user.bin");

    let (context, instance) = open_with_user(&user.path);
    let mut length = UNTOUCHED;
    let mut text = SEEDED as *mut c_char;
    assert!(unsafe {
        zhuyin_token_get_phrase(instance, 0x0700_0001, &raw mut length, &raw mut text)
    });
    assert_eq!((length, text), (2, ptr::null_mut()));
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// A stored key word 0x8000 reads back verbatim (`phrase_index.cpp:33-50`).
#[test]
fn a_stored_key_word_reads_back_verbatim() {
    let user_dir = TempUserDir::new("key-8000");
    let (context, instance) = open_with_user(&user_dir.path);
    // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
    assert!(unsafe { import(context, 7, "你", "ㄋㄧˇ".as_bytes(), 5) });
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let token = unsafe { user_tokens(instance, "你") }[0];
    assert_eq!(
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        unsafe { nth_pronunciation(instance, token, 0) },
        (true, 1, "2b30".to_owned())
    );
    assert!(unsafe { zhuyin_save(context) });
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
    let path = user_dir.path.join("user.bin");
    let mut payload = std::fs::read(&path).expect("user.bin")[8..].to_vec();
    let text = ucs4_key("你");
    let at = (0..=payload.len() - text.len())
        .find(|&at| payload[at..at + text.len()] == text[..])
        .expect("the imported item")
        + text.len();
    assert_eq!(payload[at..at + 2], [0x2b, 0x30]);
    payload[at..at + 2].copy_from_slice(&0x8000_u16.to_le_bytes());
    std::fs::write(
        &path,
        oxpinyin::data::chunk_format::build_memory_chunk(&payload).expect("frame"),
    )
    .expect("write user.bin");
    let (context, instance) = open_with_user(&user_dir.path);
    assert_eq!(
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        unsafe { token_phrase(instance, token) },
        (true, 1, Some("你".to_owned()))
    );
    assert_eq!(
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        unsafe { nth_pronunciation(instance, token, 0) },
        (true, 1, "0080".to_owned())
    );
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// A caller's zero-terminated, cleared GArray keeps its metadata: the
/// facade's fill runs the clear callback on the seed, and a refusal clears
/// the array through it (`garray.c`, #924).
#[test]
fn caller_arrays_keep_their_metadata_and_clear_through_the_callback() {
    let (context, instance) = open();
    let mut token = 0;
    for width in [4_u32, 2] {
        // SAFETY: a fresh zero-terminated, cleared GArray of `width`-byte elements.
        let array = unsafe { glib_sys::g_array_new(1, 1, width) };
        // SAFETY: the callback reads the low two bytes of one element.
        unsafe { glib_sys::g_array_set_clear_func(array, Some(clear_element)) };
        let seed: u32 = 0x1234;
        // SAFETY: one element of `width` bytes from `seed`.
        unsafe { glib_sys::g_array_append_vals(array, (&raw const seed).cast(), 1) };
        take_cleared();
        let ret = if width == 4 {
            unsafe {
                crate::support::zhuyin_lookup_tokens(instance, cstr("你好").as_ptr(), array)
            }
        } else {
            unsafe { zhuyin_token_get_nth_pronunciation(instance, token, 0, array) }
        };
        assert!(ret);
        assert_eq!(take_cleared(), [0x1234]);
        // SAFETY: `array` is the live `GArray` made above, of the element width `elements` reads.
        let filled = unsafe { elements(array, width as usize) };
        assert!(!filled.is_empty());
        if width == 4 {
            token = filled[0];
        }
        let refused = if width == 4 {
            unsafe {
                crate::support::zhuyin_lookup_tokens(instance, cstr("not-a-phrase").as_ptr(), array)
            }
        } else {
            unsafe { zhuyin_token_get_nth_pronunciation(instance, 0xFFFF_FFFF, 0, array) }
        };
        assert!(!refused);
        // SAFETY: `array` is the live `GArray` made above, of the element width `elements` reads.
        assert!(unsafe { elements(array, width as usize) }.is_empty());
        let low: Vec<u32> = filled.iter().map(|value| value & 0xFFFF).collect();
        assert_eq!(take_cleared(), low);
        // SAFETY: the array is freed once, with its data.
        unsafe { glib_sys::g_array_free(array, 1) };
    }
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// The token getters (`zhuyin.cpp:1793-1826`): the nth pronunciation answers
/// true for any nth of a known token, and false with an emptied array for an
/// unknown one; the phrase getter fills each non-NULL output on a hit and
/// leaves both on a miss; the count is 0 on a miss.
#[test]
fn token_getters_answer_the_pins_ranges_and_outputs() {
    let (context, instance) = open();
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let token = unsafe { tokens_of(instance, "你好") }[0];
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let (_, count) = unsafe { n_pronunciation(instance, token) };
    assert_eq!(count, 1);
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let first = unsafe { nth_pronunciation(instance, token, 0) };
    assert_eq!((first.0, first.1), (true, 2));
    // Past the last reading the pin ignores the failed read and appends
    // `phrase_length` keys all the same (`zhuyin.cpp:1806-1809`).
    for nth in [count, count + 1, u32::MAX] {
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        let past = unsafe { nth_pronunciation(instance, token, nth) };
        assert_eq!((past.0, past.1), (true, 2), "nth {nth}");
    }
    // SAFETY: a fresh GArray of two-byte elements, seeded with one key.
    let keys = unsafe { glib_sys::g_array_new(0, 0, 2) };
    let seed: u16 = 0x1234;
    // SAFETY: one two-byte element from a live local.
    unsafe { glib_sys::g_array_append_vals(keys, (&raw const seed).cast(), 1) };
    assert!(!unsafe { zhuyin_token_get_nth_pronunciation(instance, 0xFFFF_FFFF, 0, keys) });
    // SAFETY: the array is live; it is freed once.
    unsafe {
        assert_eq!((*keys).len, 0);
        glib_sys::g_array_free(keys, 1);
    }

    assert_eq!(
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        unsafe { token_phrase(instance, token) },
        (true, 2, Some("你好".to_owned()))
    );
    let mut length = UNTOUCHED;
    assert!(unsafe { zhuyin_token_get_phrase(instance, token, &raw mut length, ptr::null_mut()) });
    assert_eq!(length, 2);
    let mut text: *mut c_char = ptr::null_mut();
    assert!(unsafe { zhuyin_token_get_phrase(instance, token, ptr::null_mut(), &raw mut text) });
    // SAFETY: `text` is the string the library just returned; this code owns it and frees it only
    // here.
    assert_eq!(unsafe { take_sentence(text) }, "你好");
    assert!(unsafe { zhuyin_token_get_phrase(instance, token, ptr::null_mut(), ptr::null_mut()) });
    for miss in [0xFFFF_FFFF, 0x0DEA_DBEE, 0x0100_0000] {
        assert_eq!(
            // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into
            // it.
            unsafe { token_phrase(instance, miss) },
            (false, UNTOUCHED, None),
            "{miss:#x}"
        );
        assert!(!unsafe {
            zhuyin_token_get_phrase(instance, miss, ptr::null_mut(), ptr::null_mut())
        });
        assert_eq!(
            // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into
            // it.
            unsafe { n_pronunciation(instance, miss) },
            (false, 0),
            "{miss:#x}"
        );
    }
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}
