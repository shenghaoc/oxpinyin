//! The import and export iterators and the compaction at the end of an
//! import.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use super::*;

/// A user gram for 你 and for 中, each `[0xFFFFFFFF, (好, 0xFFFFFFFF)]`:
/// the bigram export count wraps to -2.
#[test]
fn the_bigram_export_count_wraps() {
    let user = TempUserDir::new("d18-export-ffffffff");
    let (context, instance) = open_at(&user.path);
    let [ni, hao, zhong] = ["你", "好", "中"].map(|text| lookup(instance, text)[0]);
    close(context, instance);
    let gram = gram_bytes(0xFFFF_FFFF, &[(hao, 0xFFFF_FFFF)]);
    plant_bigram(
        &user.path,
        vec![
            (ni.to_le_bytes().to_vec(), gram.clone()),
            (zhong.to_le_bytes().to_vec(), gram),
            (1_u32.to_le_bytes().to_vec(), vec![0; 4]),
        ],
    );
    let (context, instance) = open_at(&user.path);
    let iter = unsafe { pinyin_begin_get_bigram_phrases(context) };
    let mut rows = Vec::new();
    while unsafe { pinyin_bigram_iterator_has_next_phrase(iter) } {
        let (mut phrase, mut pinyin, mut count) = (ptr::null_mut(), ptr::null_mut(), 0);
        unsafe {
            pinyin_bigram_iterator_get_next_phrase(
                iter,
                &raw mut phrase,
                &raw mut pinyin,
                &raw mut count,
            )
        };
        rows.push((
            crate::ffi::take_owned_cstr(phrase),
            crate::ffi::take_owned_cstr(pinyin),
            count,
        ));
    }
    unsafe { crate::iterators::pinyin_end_get_bigram_phrases(iter) };
    rows.sort();
    assert_eq!(
        rows,
        [
            ("中好".to_owned(), "zhong'hao".to_owned(), -2),
            ("你好".to_owned(), "ni'hao".to_owned(), -2),
        ]
    );
    close(context, instance);
}

/// A system library whose only item is 你 with the one packed reading `key`,
/// and the export of that library.
fn export_a_system_reading(key: u16) {
    let scratch = tempfile::tempdir().expect("scratch");
    let system = crate::test_support::items_system(
        &system_dir(),
        scratch.path(),
        &[(&[0x4f60], &[key], 100)],
        1000,
    );
    let user = TempUserDir::new("export-system-key");
    let context = init_at(&system, &user.path);
    assert!(!context.is_null());
    export_rows_of(context, 1, "你");
}

/// `pinyin_iterator_get_next_phrase` renders every key of the row with
/// `get_pinyin_string`, which asserts the tone, for a system item as for a
/// user item (`pinyin.cpp:721-733`, `chewing_key.cpp:47-57` at 074a2219).
/// A stored tone 6 on a system item's reading aborts the export.
#[test]
fn export_aborts_on_a_system_reading_with_a_tone_past_the_tones() {
    assert_aborts(
        "boundary_tests::import_export::export_aborts_on_a_system_reading_with_a_tone_past_the_tones",
        // `ba` with tone 6: the tone is bits 12 to 14 of the packed key.
        || export_a_system_reading(0x0081 | (6 << 12)),
    );
}

/// After the tone, `get_pinyin_string` calls `get_table_index`, which
/// asserts the initial, middle and final against the table dimensions
/// (`chewing_key.cpp:29-36`, from `:49`). Initial 31 is outside the 24
/// initials, so the export aborts.
#[test]
fn export_aborts_on_a_system_reading_with_a_field_outside_the_table() {
    assert_aborts(
        "boundary_tests::import_export::export_aborts_on_a_system_reading_with_a_field_outside_the_table",
        || export_a_system_reading(0x009f | (1 << 12)),
    );
}

/// The compaction at the end of an import copies the tokens of the range
/// `[1, end)` into a fresh sub-index, so an item at the reserved slot 0 is
/// dropped (`pinyin.cpp:655-660`, `phrase_index.cpp:624-675` at 074a2219).
/// Before the compaction the item is readable: `get_phrase_item` has no
/// slot-0 exclusion (`phrase_index.cpp:180-206`).
#[test]
fn import_end_compaction_drops_the_reserved_slot() {
    use oxpinyin_data::chunk_format::build_memory_chunk;
    use oxpinyin_data::chunk_write::{ChunkItem, build_chunk};

    let user = TempUserDir::new("user-file-slot-zero");
    write_user_conf(&user.path, &system_dir());
    let item = ChunkItem {
        phrase: vec![u32::from('你')],
        unigram: 17,
        prons: vec![(vec![0x302b], 5)],
    };
    let chunk = build_chunk(&[(1, item)]).expect("one-item user chunk");
    // Move the item's offset word from slot 1 to slot 0.
    let mut payload = chunk[8..].to_vec();
    let offsets_start = u32::from_le_bytes(payload[4..8].try_into().expect("offset header"));
    let slot_zero = offsets_start as usize;
    payload.copy_within(slot_zero + 4..slot_zero + 8, slot_zero);
    payload[slot_zero + 4..slot_zero + 8].fill(0);
    std::fs::write(
        user.path.join("user.bin"),
        build_memory_chunk(&payload).expect("frame"),
    )
    .expect("write user.bin");

    let (context, instance) = open_at(&user.path);
    assert_eq!(unigram(instance, 0x0700_0000), (true, 17));
    assert_eq!(import_batch(context, 7, &[]), [false; 0]);
    assert_eq!(unigram(instance, 0x0700_0000), (false, 0));
    close(context, instance);
}

/// An import of a reading its phrase already holds removes the item, edits
/// a copy and adds the copy back at the content's end
/// (`pinyin.cpp:563-582`, `phrase_index.cpp:208-235` at 074a2219), so the
/// bytes the slot named before are not written. Slots 1 and 2 of `user.bin`
/// name one body and the phrase table holds token 1 alone: the import
/// raises token 1's count and token 2 keeps the old one.
#[test]
fn an_imported_reading_leaves_an_aliased_slot_its_old_count() {
    use oxpinyin_data::chunk_format::build_memory_chunk;
    use oxpinyin_data::chunk_write::{ChunkItem, build_chunk};

    let user = TempUserDir::new("user-file-aliased-slots");
    write_user_conf(&user.path, &system_dir());
    let ce4 = oxpinyin_core::ChewingKey::from_pinyin("ce")
        .expect("ce")
        .with_tone(4)
        .to_packed();
    let item = || ChunkItem {
        phrase: "测测".chars().map(u32::from).collect(),
        unigram: 15,
        prons: vec![(vec![ce4, ce4], 5)],
    };
    let chunk = build_chunk(&[(1, item()), (2, item())]).expect("two-item user chunk");
    // Point slot 2 at slot 1's body.
    let mut payload = chunk[8..].to_vec();
    let offsets_start = u32::from_le_bytes(payload[4..8].try_into().expect("offset header"));
    let slot_one = offsets_start as usize + 4;
    payload.copy_within(slot_one..slot_one + 4, slot_one + 4);
    std::fs::write(
        user.path.join("user.bin"),
        build_memory_chunk(&payload).expect("frame"),
    )
    .expect("write user.bin");
    set_phrase_table_row(&user.path, "测测", &[0x0700_0001]);

    let (context, instance) = open_at(&user.path);
    let iter = unsafe { pinyin_begin_add_phrases(context, 7) };
    assert!(unsafe {
        crate::iterators::pinyin_iterator_add_phrase(
            iter,
            cstr("测测").as_ptr(),
            cstr("ce4'ce4").as_ptr(),
            7,
        )
    });
    assert_eq!(
        export_rows_of(context, 7, "测测"),
        [("ce4'ce4".to_owned(), 12), ("ce4'ce4".to_owned(), 5)]
    );
    unsafe { pinyin_end_add_phrases(iter) };
    close(context, instance);
}

/// An import reading parses past long separator runs, between its keys and
/// after them, to its two keys.
#[test]
fn an_import_reading_parses_past_long_separator_runs() {
    let user = TempUserDir::new("import-separator-runs");
    let (context, instance) = open_at(&user.path);
    let iter = unsafe { crate::iterators::pinyin_begin_add_phrases(context, 7) };
    for reading in [
        [&b"ni3"[..], &[b'\''; 40_000], b"hao3"].concat(),
        [&b"ni3'hao3"[..], &[b'\''; 70_000]].concat(),
    ] {
        let reading = std::ffi::CString::new(reading).expect("no NUL");
        assert!(unsafe {
            crate::iterators::pinyin_iterator_add_phrase(
                iter,
                c"你好".as_ptr(),
                reading.as_ptr(),
                1,
            )
        });
    }
    unsafe { crate::iterators::pinyin_end_add_phrases(iter) };
    assert_eq!(
        crate::e2e_tests::export_of(context, 7),
        [("你好".to_owned(), "ni3'hao3".to_owned(), 2)]
    );
    close(context, instance);
}

/// `FacadePhraseIndex::compact` copies every readable item of a system
/// sub-index into a fresh one, and `add_phrase_item` stores each at its own
/// offset (`phrase_index.cpp:208-217`, `:648-675` at 074a2219). Slots 1 and
/// 2 of `gb_char.bin` name one body. After the compaction at the end of an
/// import (`pinyin.cpp:655-660`), an increment of token 1's unigram
/// (`phrase_index.cpp:150-178`) leaves token 2's as it was.
#[test]
fn import_end_compaction_gives_aliased_system_slots_their_own_body() {
    use oxpinyin_data::chunk_format::build_memory_chunk;

    let scratch = tempfile::tempdir().expect("scratch");
    let system = crate::test_support::items_system(
        &system_dir(),
        scratch.path(),
        &[(&[0x7532], &[0x1081], 100), (&[0x4e59], &[0x1084], 100)],
        1000,
    );
    // Point slot 2 at slot 1's body.
    let chunk = system.join("gb_char.bin");
    let mut payload = std::fs::read(&chunk).expect("gb_char.bin")[8..].to_vec();
    let offsets_start = u32::from_le_bytes(payload[4..8].try_into().expect("offset header"));
    let slot_one = offsets_start as usize + 4;
    payload.copy_within(slot_one..slot_one + 4, slot_one + 4);
    std::fs::write(&chunk, build_memory_chunk(&payload).expect("frame")).expect("gb_char.bin");

    let user = TempUserDir::new("system-aliased-slots");
    let context = init_at(&system, &user.path);
    assert!(!context.is_null());
    let instance = alloc(context);
    assert_eq!(unigram(instance, 0x0100_0002), (true, 100));
    assert_eq!(import_batch(context, 7, &[]), [false; 0]);
    assert!(unsafe { pinyin_token_add_unigram_frequency(instance, 0x0100_0001, 5) });
    assert_eq!(unigram(instance, 0x0100_0001), (true, 105));
    assert_eq!(unigram(instance, 0x0100_0002), (true, 100));
    close(context, instance);
}

/// Compaction copies each readable item independently (`phrase_index.cpp:648-675`
/// at 074a2219), including bodies at distinct offsets that overlap. Adding 256
/// to the first unigram changes the second's low byte only before compaction.
#[test]
fn import_end_compaction_gives_overlapping_system_slots_their_own_body() {
    use oxpinyin_data::chunk_format::build_memory_chunk;

    for write_before_compaction in [false, true] {
        let scratch = tempfile::tempdir().expect("scratch");
        let system = crate::test_support::items_system(
            &system_dir(),
            scratch.path(),
            &[(&[0x7532], &[0x1081], 100), (&[0x4e59], &[0x1084], 100)],
            1000,
        );
        let chunk = system.join("gb_char.bin");
        let mut payload = std::fs::read(&chunk).expect("gb_char.bin")[8..].to_vec();
        let offsets_start =
            u32::from_le_bytes(payload[4..8].try_into().expect("offset header")) as usize;
        let content_start =
            u32::from_le_bytes(payload[8..12].try_into().expect("content header")) as usize;
        let slot_one = offsets_start + 4;
        let offset =
            u32::from_le_bytes(payload[slot_one..slot_one + 4].try_into().expect("slot 1"));
        payload[slot_one + 4..slot_one + 8].copy_from_slice(&(offset + 1).to_le_bytes());
        let body = content_start + offset as usize;
        // The first item has one character and one reading (16 bytes).
        // The second starts one byte later, with one character and no readings
        // (10 bytes). Both unigrams start at zero and both bodies are readable.
        payload[body..body + 16].fill(0);
        payload[body..body + 2].copy_from_slice(&[1, 1]);
        std::fs::write(&chunk, build_memory_chunk(&payload).expect("frame")).expect("gb_char.bin");

        let user = TempUserDir::new("system-overlapping-slots");
        let context = init_at(&system, &user.path);
        assert!(!context.is_null());
        let instance = alloc(context);
        assert_eq!(unigram(instance, 0x0100_0001), (true, 0));
        assert_eq!(unigram(instance, 0x0100_0002), (true, 0));
        let (first, second) = if write_before_compaction {
            assert!(unsafe { pinyin_token_add_unigram_frequency(instance, 0x0100_0001, 256) });
            assert_eq!(unigram(instance, 0x0100_0002), (true, 1));
            (256, 1)
        } else {
            (0, 0)
        };
        assert_eq!(import_batch(context, 7, &[]), [false; 0]);
        assert!(unsafe { pinyin_token_add_unigram_frequency(instance, 0x0100_0001, 256) });
        assert_eq!(unigram(instance, 0x0100_0001), (true, first + 256));
        assert_eq!(unigram(instance, 0x0100_0002), (true, second));
        close(context, instance);
    }
}
