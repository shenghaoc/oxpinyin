#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use std::os::raw::c_char;

use std::path::{Path, PathBuf};

use std::ptr;

use crate::support::{
    TempUserDir, ZhuyinContext, ZhuyinInstance, assert_aborts, bytes_at, candidate_rows, close,
    craft_tree, cstr, import, init_on, link_file, lookup_candidate_type_t, write_user_conf,
    zhuyin_alloc_instance, zhuyin_choose_candidate, zhuyin_get_sentence,
    zhuyin_guess_candidates_after_cursor, zhuyin_guess_sentence, zhuyin_guess_sentence_with_prefix,
    zhuyin_parse_more_chewings, zhuyin_parse_more_full_pinyins, zhuyin_token_get_unigram_frequency,
    zhuyin_train,
};

use contract_data::contract_data_dir;

#[path = "../../../oxpinyin/tests/common/contract_data.rs"]
mod contract_data;

/// A context and instance on `system` with `user_dir`, which must open.
fn open_on(system: &Path, user_dir: &Path) -> (*mut ZhuyinContext, *mut ZhuyinInstance) {
    let context = init_on(system, Some(user_dir));
    assert!(!context.is_null(), "zhuyin_init opens the pinned data");
    let instance = unsafe { zhuyin_alloc_instance(context) };
    assert!(!instance.is_null());
    (context, instance)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// 你好 imported into libraries 1 to 5 of the pinned data, each with count
/// 100000, then a fresh instance.
fn five_libraries(user_dir: &Path) -> (*mut ZhuyinContext, *mut ZhuyinInstance) {
    let (context, instance) = open_on(&contract_data_dir(), user_dir);
    for library in 1..=5 {
        assert!(
            import(context, library, "你好", "ㄋㄧˇ ㄏㄠˇ".as_bytes(), 100_000),
            "{library}"
        );
    }
    (context, instance)
}

/// A private system under `parent`: the pinned files linked in, library 1
/// replaced by `items` (text words, key words, unigram) with a matching
/// pinyin and phrase index, and a total of `total`.
fn items_system(parent: &Path, items: &[(&[u32], &[u16], u32)], total: u32) -> PathBuf {
    use std::collections::BTreeMap;
    let source = contract_data_dir();
    let system = parent.join("system");
    std::fs::create_dir(&system).expect("private system");
    for entry in std::fs::read_dir(&source).expect("pinned data") {
        let entry = entry.expect("pinned entry");
        link_file(&entry.path(), &system.join(entry.file_name()));
    }
    let mut content = vec![0_u8; 8];
    let mut offsets = vec![0_u32];
    let mut phonetic: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
    let mut phrases: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
    for (slot, (text, reading, unigram)) in items.iter().enumerate() {
        let token = 0x0100_0000 | u32::try_from(slot + 1).expect("slot");
        let raw_text: Vec<u8> = text.iter().flat_map(|word| word.to_le_bytes()).collect();
        let raw_keys: Vec<u8> = reading.iter().flat_map(|word| word.to_le_bytes()).collect();
        offsets.push(u32::try_from(content.len()).expect("offset"));
        content.extend([u8::try_from(text.len()).expect("length"), 1]);
        content.extend(unigram.to_le_bytes());
        content.extend(&raw_text);
        content.extend(&raw_keys);
        content.extend(10_u32.to_le_bytes());
        let index: Vec<u8> = reading
            .iter()
            .flat_map(|word| (word & 4095).to_le_bytes())
            .collect();
        for end in (2..index.len()).step_by(2) {
            phonetic.entry(index[..end].to_vec()).or_default();
        }
        let mut item = token.to_le_bytes().to_vec();
        item.extend(&raw_keys);
        item.resize(item.len().div_ceil(4) * 4, 0);
        phonetic.entry(index).or_default().extend(item);
        for end in (4..raw_text.len()).step_by(4) {
            phrases.entry(raw_text[..end].to_vec()).or_default();
        }
        phrases
            .entry(raw_text)
            .or_default()
            .extend(token.to_le_bytes());
    }
    let count = u32::try_from(offsets.len()).expect("offsets");
    let index_two = 17 + count * 4 + 1;
    let content_len = u32::try_from(content.len()).expect("content");
    let mut payload: Vec<u8> = [total, 17, index_two, index_two + content_len + 1]
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    payload.push(b'#');
    payload.extend(offsets.iter().flat_map(|value| value.to_le_bytes()));
    payload.push(b'#');
    payload.extend(&content);
    payload.push(b'#');
    let gb_char = system.join("gb_char.bin");
    std::fs::remove_file(&gb_char).expect("unlink gb_char.bin");
    std::fs::write(
        &gb_char,
        oxpinyin::user::data::chunk_format::build_memory_chunk(&payload).expect("frame"),
    )
    .expect("gb_char.bin");
    for (name, rows) in [
        ("pinyin_index.bin", phonetic),
        ("phrase_index.bin", phrases),
    ] {
        let path = system.join(name);
        std::fs::remove_file(&path).expect("unlink an index");
        craft_tree(&path, &rows.into_iter().collect::<Vec<_>>());
    }
    system
}

/// A reading with fewer keys than the phrase has characters refuses the
/// import before the search.
#[test]
fn a_short_reading_refuses_the_import_after_five_libraries() {
    let user_dir = TempUserDir::new("invalid-reading");
    let (context, instance) = five_libraries(&user_dir.path);
    assert!(!import(context, 6, "你好", "ㄋㄧˇ".as_bytes(), 3));
    close(context, instance);
}

/// The prefix of a guess is reduced with the same assert on five tokens
/// (`zhuyin.cpp:933`).
#[test]
fn a_prefix_guess_over_five_library_tokens_aborts() {
    assert_aborts(
        "pinned_data::a_prefix_guess_over_five_library_tokens_aborts",
        || {
            let user_dir = TempUserDir::new("prefix-five");
            let (context, _instance) = five_libraries(&user_dir.path);
            let instance = unsafe { zhuyin_alloc_instance(context) };
            unsafe { zhuyin_guess_sentence_with_prefix(instance, cstr("你好").as_ptr()) };
        },
    );
}

/// Training a forced choice that the sentence no longer holds asserts
/// (register row 66).
#[test]
fn training_a_stale_forced_choice_aborts() {
    assert_aborts("pinned_data::training_a_stale_forced_choice_aborts", || {
        let user_dir = TempUserDir::new("stale-forcing");
        let (context, _instance) = open_on(&contract_data_dir(), &user_dir.path);
        let instance = unsafe { zhuyin_alloc_instance(context) };
        assert_eq!(
            unsafe { zhuyin_parse_more_chewings(instance, cstr("su3cl3").as_ptr()) },
            6
        );
        assert!(unsafe { zhuyin_guess_sentence(instance) });
        assert!(unsafe { zhuyin_guess_candidates_after_cursor(instance, 0) });
        let (chosen, _, _) = candidate_rows(instance)
            .into_iter()
            .find(|(_, _, text)| text.as_slice() == "泥".as_bytes())
            .expect("a 泥 row");
        unsafe { zhuyin_choose_candidate(instance, 0, chosen) };
        unsafe { zhuyin_train(instance) };
    });
}

/// A stored phrase that holds a word above Unicode is chosen, read in the
/// sentence and trained byte for byte (`zhuyin.cpp:895-910`, `:1646-1666`,
/// #894).
#[test]
fn a_stored_phrase_above_unicode_is_chosen_and_trained_byte_for_byte() {
    let user_dir = TempUserDir::new("above-unicode");
    let scratch = tempfile::tempdir().expect("system scratch");
    let text = [0x7532, 0x0011_0000, 0x4e59];
    let system = items_system(
        scratch.path(),
        &[(&text, &[0x1084; 3], 100), (&[0x4e19], &[0x1081], 100)],
        200,
    );
    write_user_conf(&user_dir.path, &system);
    let (context, instance) = open_on(&system, &user_dir.path);
    assert_eq!(
        unsafe { zhuyin_parse_more_full_pinyins(instance, cstr("da1da1da1ba1").as_ptr()) },
        12
    );
    assert!(unsafe { zhuyin_guess_candidates_after_cursor(instance, 0) });
    let (candidate, _, chosen) = candidate_rows(instance)
        .into_iter()
        .find(|(_, kind, _)| *kind == lookup_candidate_type_t::NORMAL_CANDIDATE_AFTER_CURSOR)
        .expect("the stored phrase row");
    assert_eq!(hex(&chosen), "e794b2f4908080e4b999");
    assert_eq!(
        unsafe { zhuyin_choose_candidate(instance, 0, candidate) },
        9
    );
    assert!(unsafe { zhuyin_guess_sentence(instance) });
    let mut sentence: *mut c_char = ptr::null_mut();
    assert!(unsafe { zhuyin_get_sentence(instance, &raw mut sentence) });
    assert_eq!(hex(&bytes_at(sentence)), "e794b2f4908080e4b999e4b899");
    // SAFETY: the sentence is the caller's GLib string, freed once.
    unsafe { glib_sys::g_free(sentence.cast()) };
    assert!(unsafe { zhuyin_train(instance) });
    for token in [0x0100_0001, 0x0100_0002] {
        let mut freq = 0;
        assert!(unsafe { zhuyin_token_get_unigram_frequency(instance, token, &raw mut freq) });
        assert_eq!(freq, 583, "{token:#x}");
    }
    close(context, instance);
}
