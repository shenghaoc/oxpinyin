//! The token getters: phrases, readings, pronunciation counts, caller
//! arrays and phrase texts.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use super::*;

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
        assert!(unsafe {
            crate::dict::pinyin_token_get_phrase(instance, token, &raw mut length, &raw mut phrase)
        });
        assert_eq!(
            (length, crate::ffi::take_owned_cstr(phrase).as_str()),
            (1, expected)
        );
        let mut count = UNTOUCHED;
        assert!(unsafe { pinyin_token_get_n_pronunciation(instance, token, &raw mut count) });
        assert_eq!(count, 1);
        assert_eq!(reading_bytes(instance, token, 0), key.to_le_bytes());
    }
    let (mut length, mut phrase) = (UNTOUCHED, ptr::null_mut());
    assert!(!unsafe {
        crate::dict::pinyin_token_get_phrase(
            instance,
            0x0700_0002,
            &raw mut length,
            &raw mut phrase,
        )
    });
    assert_eq!((length, phrase), (UNTOUCHED, ptr::null_mut()));
    let mut count = UNTOUCHED;
    assert!(!unsafe { pinyin_token_get_n_pronunciation(instance, 0x0700_0002, &raw mut count) });
    assert_eq!(count, 0);
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
    assert!(!unsafe {
        crate::dict::pinyin_token_get_nth_pronunciation(instance, token, 1, keys.cast())
    });
    // SAFETY: allocated above, freed once.
    unsafe { glib_sys::g_array_free(keys, 1) };
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
    assert!(unsafe { pinyin_save(context) });
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
    assert!(unsafe {
        crate::dict::pinyin_token_get_phrase(instance, token, &raw mut length, &raw mut text)
    });
    assert_eq!((length, take_hex(text).as_str()), (3, "e794b2eda080e4b999"));
    assert_eq!(unigram(instance, token), (true, 3000));
    // Ending an import batch arms the save without changing an item.
    assert!(import_batch(context, 7, &[]).is_empty());
    assert!(unsafe { pinyin_save(context) });
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
