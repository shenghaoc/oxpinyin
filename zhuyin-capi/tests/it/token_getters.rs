//! The token getters: phrases, readings, pronunciation counts and caller
//! arrays.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::support::{
    LogCapture, TempUserDir, ZhuyinInstance, close, cstr, import, open, open_with_user, system_dir,
    take_sentence, tokens_of, ucs4_key, write_user_conf, zhuyin_begin_add_phrases,
    zhuyin_end_add_phrases, zhuyin_iterator_add_phrase, zhuyin_save,
    zhuyin_token_get_n_pronunciation, zhuyin_token_get_nth_pronunciation, zhuyin_token_get_phrase,
    *,
};

use std::os::raw::c_char;

use std::ptr;

fn n_pronunciation(instance: *mut ZhuyinInstance, token: u32) -> (bool, u32) {
    let mut count = UNTOUCHED;
    let ret = unsafe { zhuyin_token_get_n_pronunciation(instance, token, &raw mut count) };
    (ret, count)
}

/// The values a GLib clear callback saw.
static CLEARED: std::sync::Mutex<Vec<u32>> = std::sync::Mutex::new(Vec::new());

extern "C" fn clear_element(element: glib_sys::gpointer) {
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

/// A token of a library with no sub-index answers false and writes 0 to the
/// count (`dict.rs`, `phrase_index.cpp:180-205`).
#[test]
fn pronunciation_count_without_a_sub_index_writes_zero() {
    let (context, instance) = open();
    assert_eq!(n_pronunciation(instance, 0x0f00_0001), (false, 0));
    close(context, instance);
}

/// Register entry "Token getters on a phrase longer than sixteen characters
/// (policy row 124, class (b))": at 074a2219 `_token_get_phrase` and
/// `zhuyin_token_get_nth_pronunciation` copy a 17-word item into 16-entry
/// stack buffers (`zhuyin.cpp:1101,1107,1800,1807`), so the pin's result is
/// undefined and nothing runs on it. oxpinyin's prescribed result: both
/// answer true with the whole stored item.
#[test]
fn token_getters_return_a_phrase_longer_than_sixteen_whole() {
    use oxpinyin::data::chunk_format::build_memory_chunk;
    use oxpinyin::data::chunk_write::{ChunkItem, build_chunk};

    let user = TempUserDir::new("user-file-long-phrase");
    write_user_conf(&user.path, &system_dir());
    let text = "一二三四五六七八九十一二三四五六七";
    let item = ChunkItem {
        phrase: text.chars().map(u32::from).collect(),
        unigram: 17,
        prons: vec![(vec![0x302b_u16; 17], 5)],
    };
    let chunk = build_chunk(&[(1, item)]).expect("one-item user chunk");
    std::fs::write(
        user.path.join("user.bin"),
        build_memory_chunk(&chunk[8..]).expect("frame"),
    )
    .expect("write user.bin");

    let (context, instance) = open_with_user(&user.path);
    let token = 0x0700_0001;
    assert_eq!(
        token_phrase(instance, token),
        (true, 17, Some(text.to_owned()))
    );
    assert_eq!(
        nth_pronunciation(instance, token, 0),
        (true, 17, "2b30".repeat(17))
    );
    close(context, instance);
}

/// A stored word at or above 0x80000000 makes `g_ucs4_to_utf8` answer NULL,
/// but `_token_get_phrase` still writes the stored length and answers true
/// (`zhuyin.cpp:1104-1113`).
#[test]
fn a_phrase_glib_cannot_convert_answers_its_length_and_null() {
    use oxpinyin::data::chunk_write::{ChunkItem, build_chunk};

    let user = TempUserDir::new("user-file-null-text");
    write_user_conf(&user.path, &system_dir());
    let item = ChunkItem {
        phrase: vec![0x4f60, 0x8000_0000],
        unigram: 17,
        prons: vec![(vec![0x302b, 0x30a7], 5)],
    };
    std::fs::write(
        user.path.join("user.bin"),
        build_chunk(&[(1, item)]).expect("one-item user chunk"),
    )
    .expect("write user.bin");

    let (context, instance) = open_with_user(&user.path);
    let mut length = UNTOUCHED;
    let mut text = SEEDED as *mut c_char;
    assert!(unsafe {
        zhuyin_token_get_phrase(instance, 0x0700_0001, &raw mut length, &raw mut text)
    });
    assert_eq!((length, text), (2, ptr::null_mut()));
    close(context, instance);
}

/// A stored key word 0x8000 reads back verbatim (`phrase_index.cpp:33-50`).
#[test]
fn a_stored_key_word_reads_back_verbatim() {
    let user_dir = TempUserDir::new("key-8000");
    let (context, instance) = open_with_user(&user_dir.path);
    assert!(import(context, 7, "你", "ㄋㄧˇ".as_bytes(), 5));
    let token = user_tokens(instance, "你")[0];
    assert_eq!(
        nth_pronunciation(instance, token, 0),
        (true, 1, "2b30".to_owned())
    );
    assert!(unsafe { zhuyin_save(context) });
    close(context, instance);
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
        token_phrase(instance, token),
        (true, 1, Some("你".to_owned()))
    );
    assert_eq!(
        nth_pronunciation(instance, token, 0),
        (true, 1, "0080".to_owned())
    );
    close(context, instance);
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
        let filled = elements(array, width as usize);
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
        assert!(elements(array, width as usize).is_empty());
        let low: Vec<u32> = filled.iter().map(|value| value & 0xFFFF).collect();
        assert_eq!(take_cleared(), low);
        // SAFETY: the array is freed once, with its data.
        unsafe { glib_sys::g_array_free(array, 1) };
    }
    close(context, instance);
}

/// A NULL key array makes GLib log a critical for each array call: two for
/// a known token (`g_array_set_size`, `g_array_append_vals`), one for an
/// unknown token.
#[test]
fn a_null_pronunciation_array_logs_the_glib_criticals() {
    let user = TempUserDir::new("garray-null-pronunciation");
    let (context, instance) = open_with_user(&user.path);
    let token = tokens_of(instance, "你好")[0];
    assert!(import(context, 7, "你好", "ㄋㄧˇ ㄏㄠˇ".as_bytes(), 1));
    let user_token = tokens_of(instance, "你好")
        .into_iter()
        .find(|token| token >> 24 == 7)
        .expect("the imported USER_FILE item");
    let logs = LogCapture::new();
    let glib = (Some("GLib".to_owned()), CRITICAL);
    for token in [token, user_token] {
        assert!(unsafe { zhuyin_token_get_nth_pronunciation(instance, token, 0, ptr::null_mut()) });
        let records = logs.take_with_messages();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].0, glib);
        assert!(records[0].1.contains("g_array_set_size"));
        assert_eq!(records[1].0, glib);
        assert!(records[1].1.contains("g_array_append_vals"));
    }
    assert!(!unsafe {
        zhuyin_token_get_nth_pronunciation(instance, 0xFFFF_FFFF, 0, ptr::null_mut())
    });
    let records = logs.take_with_messages();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].0, glib);
    assert!(records[0].1.contains("g_array_set_size"));
    drop(logs);
    close(context, instance);
}

/// The token getters (`zhuyin.cpp:1793-1826`): the nth pronunciation answers
/// true for any nth of a known token, and false with an emptied array for an
/// unknown one; the phrase getter fills each non-NULL output on a hit and
/// leaves both on a miss; the count is 0 on a miss.
#[test]
fn token_getters_answer_the_pins_ranges_and_outputs() {
    let (context, instance) = open();
    let token = tokens_of(instance, "你好")[0];
    let (_, count) = n_pronunciation(instance, token);
    assert_eq!(count, 1);
    let first = nth_pronunciation(instance, token, 0);
    assert_eq!((first.0, first.1), (true, 2));
    for nth in [count, count + 1, u32::MAX] {
        assert!(nth_pronunciation(instance, token, nth).0, "nth {nth}");
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
        token_phrase(instance, token),
        (true, 2, Some("你好".to_owned()))
    );
    let mut length = UNTOUCHED;
    assert!(unsafe { zhuyin_token_get_phrase(instance, token, &raw mut length, ptr::null_mut()) });
    assert_eq!(length, 2);
    let mut text: *mut c_char = ptr::null_mut();
    assert!(unsafe { zhuyin_token_get_phrase(instance, token, ptr::null_mut(), &raw mut text) });
    assert_eq!(take_sentence(text), "你好");
    assert!(unsafe { zhuyin_token_get_phrase(instance, token, ptr::null_mut(), ptr::null_mut()) });
    for miss in [0xFFFF_FFFF, 0x0DEA_DBEE, 0x0100_0000] {
        assert_eq!(
            token_phrase(instance, miss),
            (false, UNTOUCHED, None),
            "{miss:#x}"
        );
        assert!(!unsafe {
            zhuyin_token_get_phrase(instance, miss, ptr::null_mut(), ptr::null_mut())
        });
        assert_eq!(n_pronunciation(instance, miss), (false, 0), "{miss:#x}");
    }
    close(context, instance);
}

/// Imports 你好 into libraries `1..=libraries` of the fixture, which loads
/// libraries 1 to 7, so the phrase-table search holds `libraries` tokens.
fn hold_nihao_in_libraries(context: *mut crate::support::ZhuyinContext, libraries: u8) {
    for library in 1..=libraries {
        let iter = unsafe { zhuyin_begin_add_phrases(context, library) };
        assert!(unsafe {
            zhuyin_iterator_add_phrase(iter, cstr("你好").as_ptr(), cstr("ㄋㄧˇ ㄏㄠˇ").as_ptr(), 1)
        });
        unsafe { zhuyin_end_add_phrases(iter) };
    }
}

/// Four tokens pass `reduce_tokens`' count, so the NULL-array lookup
/// answers instead of aborting.
#[test]
fn lookup_tokens_into_a_null_array_answers_with_four_tokens() {
    let user_dir = TempUserDir::new("lookup-null-four");
    let (context, instance) = open_with_user(&user_dir.path);
    hold_nihao_in_libraries(context, 4);
    assert!(unsafe {
        crate::support::zhuyin_lookup_tokens(instance, cstr("你好").as_ptr(), ptr::null_mut())
    });
    close(context, instance);
}
