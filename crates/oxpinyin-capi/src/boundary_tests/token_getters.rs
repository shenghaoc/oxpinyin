//! The token getters: phrases, readings, pronunciation counts, caller
//! arrays and phrase texts.

use super::*;

/// `token_get_n_pronunciation` of a token in a library with no sub-index
/// answers false and writes 0.
#[test]
fn n_pronunciation_without_a_sub_index_writes_zero() {
    let user = TempUserDir::new("count-no-sub-index");
    let (context, instance) = open_at(&user.path);
    let mut count = UNTOUCHED;
    assert!(!pinyin_token_get_n_pronunciation(
        instance,
        0x0f00_0001,
        &raw mut count
    ));
    assert_eq!(count, 0);
    close(context, instance);
}

/// At 074a2219 the USER_FILE load installs the whole offset and content
/// sections (phrase_index.cpp:335-362). A bad offset fails only the item
/// read (phrase_index.cpp:180-205); neighbouring items remain readable.
#[test]
fn a_bad_user_item_offset_leaves_neighbouring_items_readable() {
    use oxpinyin_data::chunk_format::build_memory_chunk;
    use oxpinyin_data::chunk_write::{ChunkItem, build_chunk};

    let user = TempUserDir::new("user-file-bad-item");
    write_user_conf(&user.path, &system_dir());
    let rows = [(1, "你", 0x302b_u16), (2, "好", 0x30a7), (3, "吗", 0x5083)];
    let items = rows.map(|(slot, phrase, key)| {
        (
            slot,
            ChunkItem {
                phrase: phrase.chars().map(u32::from).collect(),
                unigram: 17,
                prons: vec![(vec![key], 5)],
            },
        )
    });
    let chunk = build_chunk(&items).expect("three-item user chunk");
    let mut payload = chunk[8..].to_vec();
    let offsets_start = u32::from_le_bytes(payload[4..8].try_into().expect("offset header"));
    let slot_two = offsets_start as usize + 2 * 4;
    payload[slot_two..slot_two + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    std::fs::write(
        user.path.join("user.bin"),
        build_memory_chunk(&payload).expect("frame"),
    )
    .expect("write user.bin");

    let (context, instance) = open_at(&user.path);
    for (slot, expected, key) in [rows[0], rows[2]] {
        let token = 0x0700_0000 | slot;
        let (mut length, mut phrase) = (UNTOUCHED, ptr::null_mut());
        assert!(crate::dict::pinyin_token_get_phrase(
            instance,
            token,
            &raw mut length,
            &raw mut phrase
        ));
        assert_eq!(
            (length, crate::ffi::take_owned_cstr(phrase).as_str()),
            (1, expected)
        );
        let mut count = UNTOUCHED;
        assert!(pinyin_token_get_n_pronunciation(
            instance,
            token,
            &raw mut count
        ));
        assert_eq!(count, 1);
        assert_eq!(reading_bytes(instance, token, 0), key.to_le_bytes());
    }
    let (mut length, mut phrase) = (UNTOUCHED, ptr::null_mut());
    assert!(!crate::dict::pinyin_token_get_phrase(
        instance,
        0x0700_0002,
        &raw mut length,
        &raw mut phrase
    ));
    assert_eq!((length, phrase), (UNTOUCHED, ptr::null_mut()));
    let mut count = UNTOUCHED;
    assert!(!pinyin_token_get_n_pronunciation(
        instance,
        0x0700_0002,
        &raw mut count
    ));
    assert_eq!(count, 0);
    close(context, instance);
}

/// Register entry "Token getters on a phrase longer than sixteen characters
/// (policy row 124, class (b))": at 074a2219 `_token_get_phrase` and
/// `pinyin_token_get_nth_pronunciation` copy a 17-word item into 16-entry
/// stack buffers (`pinyin.cpp:1633,1639,2808,2815`), so the pin's result is
/// undefined and nothing runs on it. oxpinyin's prescribed result: both
/// answer true with the whole stored item.
#[test]
fn token_getters_return_a_phrase_longer_than_sixteen_whole() {
    use oxpinyin_data::chunk_format::build_memory_chunk;
    use oxpinyin_data::chunk_write::{ChunkItem, build_chunk};

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
    assert!(crate::dict::pinyin_token_get_phrase(
        instance,
        token,
        &raw mut length,
        &raw mut phrase
    ));
    assert_eq!(
        (length, crate::ffi::take_owned_cstr(phrase).as_str()),
        (17, text)
    );
    let expected: Vec<u8> = keys.iter().flat_map(|key| key.to_le_bytes()).collect();
    assert_eq!(reading_bytes(instance, token, 0), expected);
    close(context, instance);
}

/// A stored user key word is read back as it is stored: 0x8000.
#[test]
fn a_stored_user_key_word_is_read_back_verbatim() {
    let user = TempUserDir::new("stored-key-8000");
    let (context, instance) = open_at(&user.path);
    assert_eq!(import_batch(context, 7, &[("你", "ni3", 5)]), [true]);
    let token = lookup(instance, "你")
        .into_iter()
        .find(|token| token >> 24 == 7)
        .expect("a user token");
    assert!(pinyin_save(context));
    close(context, instance);
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
        oxpinyin_data::chunk_format::build_memory_chunk(&payload).expect("frame"),
    )
    .expect("write user.bin");
    let (context, instance) = open_at(&user.path);
    let (mut length, mut phrase) = (UNTOUCHED, ptr::null_mut());
    assert!(crate::dict::pinyin_token_get_phrase(
        instance,
        token,
        &raw mut length,
        &raw mut phrase
    ));
    assert_eq!(
        (length, crate::ffi::take_owned_cstr(phrase).as_str()),
        (1, "你")
    );
    assert_eq!(reading_bytes(instance, token, 0), [0x00, 0x80]);
    close(context, instance);
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
    let instance = alloc(context);
    let (mut length, mut phrase) = (UNTOUCHED, UNTOUCHED as usize as *mut c_char);
    assert!(crate::dict::pinyin_token_get_phrase(
        instance,
        0x0100_0001,
        &raw mut length,
        &raw mut phrase
    ));
    assert_eq!((length, phrase), (3, ptr::null_mut()));
    close(context, instance);
}

/// `get_phrase_item` checks only the item's offset and two header bytes
/// (`phrase_index.cpp:180-205`), so a shipped item whose second reading
/// passes the content end still answers its unigram, text and first
/// reading. The second reading is undefined at the pin; oxpinyin answers no
/// reading (compatibility policy row 130).
#[test]
fn a_shipped_item_with_a_reading_past_the_content_end_keeps_its_fields() {
    let scratch = tempfile::tempdir().expect("scratch");
    let system = crate::test_support::items_system(
        &system_dir(),
        scratch.path(),
        &[(&[0x4e19], &[0x1081], 100)],
        2000,
    );
    let gb_char = system.join("gb_char.bin");
    let mut payload = std::fs::read(&gb_char).expect("gb_char.bin")[8..].to_vec();
    let content = u32::from_le_bytes(payload[8..12].try_into().expect("index two")) as usize;
    // The item at content offset 8 now declares two readings.
    payload[content + 8 + 1] = 2;
    std::fs::remove_file(&gb_char).expect("unlink gb_char.bin");
    std::fs::write(
        &gb_char,
        oxpinyin_data::chunk_format::build_memory_chunk(&payload).expect("frame"),
    )
    .expect("gb_char.bin");
    let user = TempUserDir::new("token-overrun");
    let context = init_at(&system, &user.path);
    assert!(!context.is_null());
    let instance = alloc(context);
    let token = 0x0100_0001;
    assert_eq!(unigram(instance, token), (true, 100));
    assert_eq!(phrase_of(instance, token).as_deref(), Some("丙"));
    assert_eq!(reading_bytes(instance, token, 0), 0x1081_u16.to_le_bytes());
    // SAFETY: a fresh GArray of two-byte elements, freed below.
    let keys = unsafe { glib_sys::g_array_new(0, 0, 2) };
    assert!(!crate::dict::pinyin_token_get_nth_pronunciation(
        instance,
        token,
        1,
        keys.cast()
    ));
    // SAFETY: allocated above, freed once.
    unsafe { glib_sys::g_array_free(keys, 1) };
    close(context, instance);
}

std::thread_local! {
    static CLEARED: std::cell::RefCell<Vec<u32>> = const { std::cell::RefCell::new(Vec::new()) };
}

extern "C" fn record_cleared(element: glib_sys::gpointer) {
    // SAFETY: GLib hands the clear function one live four-byte element.
    let value = unsafe { element.cast::<u32>().read_unaligned() };
    CLEARED.with(|cleared| cleared.borrow_mut().push(value));
}

fn take_cleared() -> Vec<u32> {
    CLEARED.with(|cleared| std::mem::take(&mut *cleared.borrow_mut()))
}

/// The elements of a zero-terminated GArray of `u32`, its terminator
/// checked.
fn terminated_u32(array: *mut glib_sys::GArray) -> Vec<u32> {
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
    assert!(crate::dict::pinyin_lookup_tokens(
        instance,
        c"你好".as_ptr(),
        array.cast()
    ));
    assert_eq!(take_cleared(), [0x1234]);
    let found = terminated_u32(array);
    assert_eq!(found, [0x0100_6205]);
    assert!(!crate::dict::pinyin_lookup_tokens(
        instance,
        c"not-a-phrase".as_ptr(),
        array.cast()
    ));
    assert_eq!(take_cleared(), found);
    assert!(terminated_u32(array).is_empty());
    // SAFETY: the array is freed once, with its data.
    unsafe { glib_sys::g_array_free(array, 1) };
    close(context, instance);
}

/// `token_get_nth_pronunciation` into a NULL array logs the GArray
/// criticals the pin's calls make: set_size and append_vals on a hit,
/// set_size alone on an unknown token.
#[test]
fn nth_pronunciation_into_a_null_array_logs_the_glib_criticals() {
    let user = TempUserDir::new("garray-null-pronunciation");
    let user_token = saved_user_phrase(&user.path);
    let (context, instance) = open_at(&user.path);
    let token = lookup(instance, "你好")[0];
    let logs = crate::test_support::LogCapture::new();
    let glib = (Some("GLib".to_owned()), CRITICAL);
    for token in [token, user_token] {
        assert!(crate::dict::pinyin_token_get_nth_pronunciation(
            instance,
            token,
            0,
            ptr::null_mut()
        ));
        let records = logs.take_with_messages();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].0, glib);
        assert!(records[0].1.contains("g_array_set_size"));
        assert_eq!(records[1].0, glib);
        assert!(records[1].1.contains("g_array_append_vals"));
    }
    assert!(!crate::dict::pinyin_token_get_nth_pronunciation(
        instance,
        0xFFFF_FFFF,
        0,
        ptr::null_mut()
    ));
    let records = logs.take_with_messages();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].0, glib);
    assert!(records[0].1.contains("g_array_set_size"));
    close(context, instance);
}

/// Hex of the NUL-terminated `text`, freed with `g_free`.
fn take_hex(text: *mut c_char) -> String {
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
    crate::test_support::items_system(
        &system_dir(),
        parent,
        &[
            (&[0x7532, word, 0x4e59], &[0x1084; 3], 100),
            (&[0x4e19], &[0x1081], 100),
        ],
        200,
    )
}

/// A library-1 journal ADD whose text is replaced with a surrogate word is
/// loaded as stored: the token's text keeps it, the unigram is the import's
/// count, and a save writes the journal as loaded.
#[test]
fn a_journal_word_no_import_writes_is_kept() {
    let scratch = tempfile::tempdir().expect("scratch");
    let system = phrase_text_system(scratch.path(), 0x4e01);
    let user = TempUserDir::new("stored-words-surrogate");
    let context = init_at(&system, &user.path);
    assert!(!context.is_null());
    let instance = alloc(context);
    assert_eq!(
        import_batch(context, 1, &[("甲戊乙", "da1'da1'da1", 1000)]),
        [true]
    );
    let token = lookup(instance, "甲戊乙")
        .into_iter()
        .find(|token| token >> 24 == 1)
        .expect("the imported token");
    assert!(pinyin_save(context));
    close(context, instance);
    let path = user.path.join("gb_char.dbin");
    let original: Vec<u8> = [0x7532_u32, 0x620a, 0x4e59]
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect();
    let replacement: Vec<u8> = [0x7532_u32, 0xd800, 0x4e59]
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect();
    let mut payload = std::fs::read(&path).expect("gb_char.dbin")[8..].to_vec();
    let at = payload
        .windows(original.len())
        .position(|window| window == original.as_slice())
        .expect("the imported item");
    payload.splice(at..at + original.len(), replacement);
    write_log(&user.path, "gb_char.dbin", &payload);
    let context = init_at(&system, &user.path);
    assert!(!context.is_null());
    let instance = alloc(context);
    let (mut length, mut text) = (UNTOUCHED, ptr::null_mut());
    assert!(crate::dict::pinyin_token_get_phrase(
        instance,
        token,
        &raw mut length,
        &raw mut text
    ));
    assert_eq!((length, take_hex(text).as_str()), (3, "e794b2eda080e4b999"));
    assert_eq!(unigram(instance, token), (true, 3000));
    // Ending an import batch arms the save without changing an item.
    assert!(import_batch(context, 7, &[]).is_empty());
    assert!(pinyin_save(context));
    let native: String = std::fs::read(&path)
        .expect("gb_char.dbin")
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(
        native,
        "380000006f1285e404000000000000000400c8000000800c000001000000030000001c000301b80b0000\
         3275000000d80000594e0000841084108410e8030000"
    );
    close(context, instance);
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
    let instance = alloc(context);
    assert!(pinyin_set_options(context, 1 << 5));
    assert_eq!(parse(instance, "da1da1da1ba1"), 12);
    assert!(pinyin_guess_candidates(
        instance,
        0,
        crate::test_support::DEFAULT_SORT
    ));
    assert_eq!(
        pinyin_choose_candidate(instance, 0, row_of(instance, NORMAL)),
        9
    );
    assert!(pinyin_guess_sentence(instance));
    let mut sentence: *mut c_char = ptr::null_mut();
    assert!(pinyin_get_sentence(instance, 0, &raw mut sentence));
    assert_eq!(take_hex(sentence), "e794b2f4908080e4b999e4b899");
    assert!(crate::candidates::pinyin_train(instance, 0));
    assert_eq!(unigram(instance, 0x0100_0001), (true, 583));
    assert_eq!(unigram(instance, 0x0100_0002), (true, 583));
    let iter = crate::iterators::pinyin_begin_get_phrases(context, 1);
    let mut rows = Vec::new();
    while crate::iterators::pinyin_iterator_has_next_phrase(iter) {
        let (mut phrase, mut reading, mut count) = (ptr::null_mut(), ptr::null_mut(), 0);
        assert!(crate::iterators::pinyin_iterator_get_next_phrase(
            iter,
            &raw mut phrase,
            &raw mut reading,
            &raw mut count
        ));
        rows.push((take_hex(phrase), take_hex(reading), count));
    }
    crate::iterators::pinyin_end_get_phrases(iter);
    let row = |text: &str, reading: &str| (text.to_owned(), reading.to_owned(), 79);
    assert_eq!(
        rows,
        [
            row("e794b2f4908080e4b999", "6461312764613127646131"),
            row("e4b899", "626131"),
        ]
    );
    close(context, instance);
}

/// A row whose phrase is stored words keeps them through a choose of its
/// published handle, after a reparse of the same input: the selection
/// record takes the row's text with its stored length and words. The packed
/// string the handle points at cannot give them back — it stops at a zero
/// word and drops the stored length — so the snapshot keeps that text.
#[rstest::rstest]
#[case::zero_word(0, &[0x7532])]
#[case::above_unicode(0x11_0000, &[0x7532, 0x11_0000, 0x4e59])]
fn a_stored_words_row_is_chosen_through_its_handle_with_its_stored_text(
    #[case] word: u32,
    #[case] stored: &[u32],
) {
    let scratch = tempfile::tempdir().expect("scratch");
    let system = phrase_text_system(scratch.path(), word);
    let user = TempUserDir::new("stored-words-handle");
    let context = init_at(&system, &user.path);
    assert!(!context.is_null());
    let instance = alloc(context);
    assert_eq!(parse(instance, "da1da1da1ba1"), 12);
    assert!(pinyin_guess_candidates(
        instance,
        0,
        crate::test_support::DEFAULT_SORT
    ));
    let row = row_of(instance, NORMAL);
    assert_eq!(parse(instance, "da1da1da1ba1"), 12);
    assert_eq!(pinyin_choose_candidate(instance, 0, row), 9);
    // SAFETY: the live instance is read between serialized ABI calls.
    let inst = unsafe { crate::state::instance_ref(instance) };
    let selected = inst.core.session.selected_text();
    assert_eq!(
        (selected.stored_words(), selected.phrase_length()),
        (Some(stored), 3)
    );
    close(context, instance);
}
