//! The token getters: phrases, readings, pronunciation counts, caller
//! arrays and phrase texts.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::support::{
    TempUserDir, import_batch, lookup, pinyin_choose_candidate, pinyin_get_sentence,
    pinyin_guess_candidates, pinyin_guess_sentence, pinyin_save, pinyin_set_options,
    pinyin_token_get_n_pronunciation, system_dir, write_user_conf, *,
};

use std::ffi::c_char;

use std::path::Path;

use std::ptr;

std::thread_local! {
    static CLEARED: std::cell::RefCell<Vec<u32>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Records the four-byte element that GLib clears.
///
/// # Safety
///
/// GLib calls it as the array's clear function: `element` points to one live four-byte element of
/// that array, valid for reads for the call.
unsafe extern "C" fn record_cleared(element: glib_sys::gpointer) {
    // SAFETY: GLib hands the clear function one live four-byte element.
    let value = unsafe { element.cast::<u32>().read_unaligned() };
    CLEARED.with(|cleared| cleared.borrow_mut().push(value));
}

fn take_cleared() -> Vec<u32> {
    CLEARED.with(|cleared| std::mem::take(&mut *cleared.borrow_mut()))
}

/// The elements of a zero-terminated GArray of `u32`, its terminator
/// checked.
///
/// # Safety
///
/// `array` points to a live `GArray` of four-byte elements whose data is followed by a zero
/// element.
unsafe fn terminated_u32(array: *mut glib_sys::GArray) -> Vec<u32> {
    // SAFETY: a live zero-terminated GArray of four-byte elements.
    unsafe {
        let view = &*array;
        let data = view.data.cast::<u32>();
        assert_eq!(
            data.add(view.len as usize).read_unaligned(),
            0,
            "the terminator"
        );
        (0..view.len as usize)
            .map(|at| data.add(at).read_unaligned())
            .collect()
    }
}

/// Hex of the NUL-terminated `text`, freed with `g_free`.
///
/// # Safety
///
/// `text` is a NUL-terminated string, not NULL, that the library returned to the caller, who owns
/// it. The call frees it with `g_free`, so the caller does not use it again.
unsafe fn take_hex(text: *mut c_char) -> String {
    assert!(!text.is_null());
    // SAFETY: a NUL-terminated GLib string the caller owns.
    let bytes = unsafe { std::ffi::CStr::from_ptr(text) }
        .to_bytes()
        .to_vec();
    // SAFETY: freed once.
    unsafe { glib_sys::g_free(text.cast()) };
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// A private mini system whose library 1 holds 甲`word`乙 over
/// da1'da1'da1 and 丙 over ba1, each at 100.
fn phrase_text_system(parent: &Path, word: u32) -> std::path::PathBuf {
    crate::support::items_system(
        &system_dir(),
        parent,
        &[
            (&[0x7532, word, 0x4e59], &[0x1084; 3], 100),
            (&[0x4e19], &[0x1081], 100),
        ],
        200,
    )
}

/// `token_get_n_pronunciation` of a token in a library with no sub-index
/// answers false and writes 0.
#[test]
fn n_pronunciation_without_a_sub_index_writes_zero() {
    let user = TempUserDir::new("count-no-sub-index");
    let (context, instance) = open_at(&user.path);
    let mut count = UNTOUCHED;
    assert!(!unsafe { pinyin_token_get_n_pronunciation(instance, 0x0f00_0001, &raw mut count) });
    assert_eq!(count, 0);
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// Register entry "Token getters on a phrase longer than sixteen characters
/// (policy row 124, class (b))": at 074a2219 `_token_get_phrase` and
/// `pinyin_token_get_nth_pronunciation` copy a 17-word item into 16-entry
/// stack buffers (`pinyin.cpp:1633,1639,2808,2815`), so the pin's result is
/// undefined and nothing runs on it. oxpinyin's prescribed result: both
/// answer true with the whole stored item.
#[test]
fn token_getters_return_a_phrase_longer_than_sixteen_whole() {
    use oxpinyin::data::chunk_format::build_memory_chunk;
    use oxpinyin::data::chunk_write::{ChunkItem, build_chunk};

    let user = TempUserDir::new("user-file-long-phrase");
    write_user_conf(&user.path, &system_dir());
    let text = "一二三四五六七八九十一二三四五六七";
    let keys = vec![0x302b_u16; 17];
    let item = ChunkItem {
        phrase: text.chars().map(u32::from).collect(),
        unigram: 17,
        prons: vec![(keys.clone(), 5)],
    };
    let chunk = build_chunk(&[(1, item)]).expect("one-item user chunk");
    std::fs::write(
        user.path.join("user.bin"),
        build_memory_chunk(&chunk[8..]).expect("frame"),
    )
    .expect("write user.bin");

    let (context, instance) = open_at(&user.path);
    let token = 0x0700_0001;
    let (mut length, mut phrase) = (UNTOUCHED, ptr::null_mut());
    assert!(unsafe {
        crate::support::pinyin_token_get_phrase(instance, token, &raw mut length, &raw mut phrase)
    });
    assert_eq!(
        (
            length,
            // SAFETY: `phrase` is the string the library just returned; this code owns it and frees
            // it only here.
            unsafe { crate::support::take_owned_cstr(phrase) }.as_str()
        ),
        (17, text)
    );
    let expected: Vec<u8> = keys.iter().flat_map(|key| key.to_le_bytes()).collect();
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { reading_bytes(instance, token, 0) }, expected);
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// A stored user key word is read back as it is stored: 0x8000.
#[test]
fn a_stored_user_key_word_is_read_back_verbatim() {
    let user = TempUserDir::new("stored-key-8000");
    let (context, instance) = open_at(&user.path);
    assert_eq!(
        // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
        unsafe { import_batch(context, 7, &[("你", "ni3", 5)]) },
        [true]
    );
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let token = unsafe { lookup(instance, "你") }
        .into_iter()
        .find(|token| token >> 24 == 7)
        .expect("a user token");
    assert!(unsafe { pinyin_save(context) });
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
    let path = user.path.join("user.bin");
    let mut payload = std::fs::read(&path).expect("user.bin")[8..].to_vec();
    let text = 0x4f60_u32.to_le_bytes();
    let at = payload
        .windows(4)
        .position(|window| window == text)
        .expect("the imported item")
        + 4;
    payload[at..at + 2].copy_from_slice(&0x8000_u16.to_le_bytes());
    std::fs::write(
        &path,
        oxpinyin::data::chunk_format::build_memory_chunk(&payload).expect("frame"),
    )
    .expect("write user.bin");
    let (context, instance) = open_at(&user.path);
    let (mut length, mut phrase) = (UNTOUCHED, ptr::null_mut());
    assert!(unsafe {
        crate::support::pinyin_token_get_phrase(instance, token, &raw mut length, &raw mut phrase)
    });
    assert_eq!(
        (
            length,
            // SAFETY: `phrase` is the string the library just returned; this code owns it and frees
            // it only here.
            unsafe { crate::support::take_owned_cstr(phrase) }.as_str()
        ),
        (1, "你")
    );
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { reading_bytes(instance, token, 0) }, [0x00, 0x80]);
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// A stored word at or above 0x80000000 makes `g_ucs4_to_utf8` answer NULL,
/// but `_token_get_phrase` still writes the stored length and answers true
/// (`pinyin.cpp:1635-1645`).
#[test]
fn a_phrase_glib_cannot_convert_answers_its_length_and_null() {
    let scratch = tempfile::tempdir().expect("scratch");
    let system = prefix_system(scratch.path(), 0x8000_0000);
    let user = TempUserDir::new("token-phrase-null");
    let context = init_at(&system, &user.path);
    assert!(!context.is_null());
    // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
    let instance = unsafe { alloc(context) };
    let (mut length, mut phrase) = (UNTOUCHED, UNTOUCHED as usize as *mut c_char);
    assert!(unsafe {
        crate::support::pinyin_token_get_phrase(
            instance,
            0x0100_0001,
            &raw mut length,
            &raw mut phrase,
        )
    });
    assert_eq!((length, phrase), (3, ptr::null_mut()));
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// `lookup_tokens` clears the caller's GArray through its clear function
/// before it appends, and on a refusal leaves it cleared and empty.
#[test]
fn lookup_tokens_resets_the_callers_array() {
    let user = TempUserDir::new("garray-lifecycle");
    let (context, instance) = open_at(&user.path);
    take_cleared();
    // SAFETY: a fresh zero-terminated, cleared GArray of four-byte elements
    // with a clear function that reads one element; freed below.
    let array = unsafe {
        let array = glib_sys::g_array_new(1, 1, 4);
        glib_sys::g_array_set_clear_func(array, Some(record_cleared));
        let seed: u32 = 0x1234;
        glib_sys::g_array_append_vals(array, (&raw const seed).cast(), 1);
        array
    };
    assert!(unsafe {
        crate::support::pinyin_lookup_tokens(instance, c"你好".as_ptr(), array.cast())
    });
    assert_eq!(take_cleared(), [0x1234]);
    // SAFETY: `array` is the live `GArray` made above, of the element width `terminated_u32` reads.
    let found = unsafe { terminated_u32(array) };
    assert_eq!(found, [0x0100_6205]);
    assert!(!unsafe {
        crate::support::pinyin_lookup_tokens(instance, c"not-a-phrase".as_ptr(), array.cast())
    });
    assert_eq!(take_cleared(), found);
    // SAFETY: `array` is the live `GArray` made above, of the element width `terminated_u32` reads.
    assert!(unsafe { terminated_u32(array) }.is_empty());
    // SAFETY: the array is freed once, with its data.
    unsafe { glib_sys::g_array_free(array, 1) };
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// `token_get_nth_pronunciation` into a NULL array logs the GArray
/// criticals the pin's calls make: set_size and append_vals on a hit,
/// set_size alone on an unknown token.
#[test]
fn nth_pronunciation_into_a_null_array_logs_the_glib_criticals() {
    let user = TempUserDir::new("garray-null-pronunciation");
    let user_token = saved_user_phrase(&user.path);
    let (context, instance) = open_at(&user.path);
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let token = unsafe { lookup(instance, "你好") }[0];
    let logs = crate::support::LogCapture::new();
    let glib = (Some("GLib".to_owned()), CRITICAL);
    for token in [token, user_token] {
        assert!(unsafe {
            crate::support::pinyin_token_get_nth_pronunciation(instance, token, 0, ptr::null_mut())
        });
        let records = logs.take_with_messages();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].0, glib);
        assert!(records[0].1.contains("g_array_set_size"));
        assert_eq!(records[1].0, glib);
        assert!(records[1].1.contains("g_array_append_vals"));
    }
    assert!(!unsafe {
        crate::support::pinyin_token_get_nth_pronunciation(
            instance,
            0xFFFF_FFFF,
            0,
            ptr::null_mut(),
        )
    });
    let records = logs.take_with_messages();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].0, glib);
    assert!(records[0].1.contains("g_array_set_size"));
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// A stored phrase word above Unicode reaches the sentence, the training
/// and the export through `phrase_text`.
#[test]
fn a_system_phrase_above_unicode_is_chosen_and_trained() {
    let scratch = tempfile::tempdir().expect("scratch");
    let system = phrase_text_system(scratch.path(), 0x11_0000);
    let user = TempUserDir::new("phrase-text-above-unicode");
    let context = init_at(&system, &user.path);
    assert!(!context.is_null());
    // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
    let instance = unsafe { alloc(context) };
    assert!(unsafe { pinyin_set_options(context, 1 << 5) });
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { parse(instance, "da1da1da1ba1") }, 12);
    assert!(unsafe { pinyin_guess_candidates(instance, 0, crate::support::DEFAULT_SORT) });
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let row = unsafe { row_of(instance, NORMAL) };
    // SAFETY: the row belongs to this live instance and no call has invalidated its snapshot.
    assert_eq!(unsafe { pinyin_choose_candidate(instance, 0, row) }, 9);
    assert!(unsafe { pinyin_guess_sentence(instance) });
    let mut sentence: *mut c_char = ptr::null_mut();
    assert!(unsafe { pinyin_get_sentence(instance, 0, &raw mut sentence) });
    // SAFETY: `sentence` is the string the library just returned; this code owns it and frees it
    // only here.
    assert_eq!(unsafe { take_hex(sentence) }, "e794b2f4908080e4b999e4b899");
    assert!(unsafe { crate::support::pinyin_train(instance, 0) });
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { unigram(instance, 0x0100_0001) }, (true, 583));
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { unigram(instance, 0x0100_0002) }, (true, 583));
    let iter = unsafe { crate::support::pinyin_begin_get_phrases(context, 1) };
    let mut rows = Vec::new();
    while unsafe { crate::support::pinyin_iterator_has_next_phrase(iter) } {
        let (mut phrase, mut reading, mut count) = (ptr::null_mut(), ptr::null_mut(), 0);
        assert!(unsafe {
            crate::support::pinyin_iterator_get_next_phrase(
                iter,
                &raw mut phrase,
                &raw mut reading,
                &raw mut count,
            )
        });
        rows.push((
            // SAFETY: `phrase` is the string the library just returned; this code owns it and frees
            // it only here.
            unsafe { take_hex(phrase) },
            // SAFETY: `reading` is the string the library just returned; this code owns it and
            // frees it only here.
            unsafe { take_hex(reading) },
            count,
        ));
    }
    unsafe { crate::support::pinyin_end_get_phrases(iter) };
    let row = |text: &str, reading: &str| (text.to_owned(), reading.to_owned(), 79);
    assert_eq!(
        rows,
        [
            row("e794b2f4908080e4b999", "6461312764613127646131"),
            row("e4b899", "626131"),
        ]
    );
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}
