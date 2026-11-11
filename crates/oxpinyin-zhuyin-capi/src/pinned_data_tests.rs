//! C-boundary decisions of the zhuyin facade on the pinned libpinyin-data
//! package (`tools/oracle/libpinyin-data.json`) in tkrzw format: the pin's
//! observations at 074a2219 whose deciding values need the full tables or a
//! pin-written profile, which the w3 mini fixture cannot hold.
//! `just fetch-contract-data` provides the package; a missing one is a
//! failure, not a skip.

use std::os::raw::c_char;
use std::path::{Path, PathBuf};
use std::ptr;

use crate::candidates::{
    zhuyin_choose_candidate, zhuyin_get_candidate, zhuyin_get_candidate_string,
    zhuyin_get_candidate_type, zhuyin_get_n_candidate, zhuyin_train,
};
use crate::config::zhuyin_set_options;
use crate::context::{zhuyin_fini, zhuyin_save};
use crate::dict::zhuyin_token_get_unigram_frequency;
use crate::instance::zhuyin_alloc_instance;
use crate::parse::{zhuyin_parse_more_chewings, zhuyin_parse_more_full_pinyins};
use crate::sentence::{
    zhuyin_get_sentence, zhuyin_guess_candidates_after_cursor, zhuyin_guess_sentence,
    zhuyin_guess_sentence_with_prefix,
};
use crate::test_support::{
    LogCapture, TempUserDir, assert_aborts, close, craft_tree, cstr, import, init_on, link_file,
    tokens_of, write_pin_profile, write_user_conf,
};
use crate::types::{LookupCandidate, ZhuyinContext, ZhuyinInstance, lookup_candidate_type_t};

#[path = "../../oxpinyin-runtime/tests/common/contract_data.rs"]
mod contract_data;
use contract_data::contract_data_dir;

/// A context and instance on `system` with `user_dir`, which must open.
fn open_on(system: &Path, user_dir: &Path) -> (*mut ZhuyinContext, *mut ZhuyinInstance) {
    let context = init_on(system, Some(user_dir));
    assert!(!context.is_null(), "zhuyin_init opens the pinned data");
    let instance = zhuyin_alloc_instance(context);
    assert!(!instance.is_null());
    (context, instance)
}

/// Every record of a user container as sorted `(key hex, value hex)` pairs;
/// `hash` reads the user bigram's hash container.
fn container_rows(path: &Path, hash: bool) -> Vec<(String, String)> {
    use oxpinyin_store::RawReadStore as _;
    use std::ops::Bound;
    let store = if hash {
        oxpinyin_store::DefaultStore::open_user_bigram(path)
    } else {
        oxpinyin_store::DefaultStore::open_read_only(path)
    }
    .unwrap_or_else(|error| panic!("open {}: {error:?}", path.display()));
    let mut rows = Vec::new();
    store
        .range_raw(Bound::Unbounded, Bound::Unbounded, |key, value| {
            rows.push((hex(key), hex(value)));
            Ok(())
        })
        .expect("read the container");
    rows.sort();
    rows
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// `(key hex, value hex)` rows from string pairs.
fn rows(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}

/// The three user containers' rows of `user_dir`.
fn user_rows(user_dir: &Path) -> [Vec<(String, String)>; 3] {
    [
        container_rows(&user_dir.join("user_bigram.db"), true),
        container_rows(&user_dir.join("user_phrase_index.bin"), false),
        container_rows(&user_dir.join("user_pinyin_index.bin"), false),
    ]
}

/// A pin-written profile (its own donor profile, a crafted bigram row, two
/// saved imports and an unsaved one) opens with its logical records: the
/// rows and tokens below. A count-zero re-import and a save change no
/// record (#929). The expected rows are the pinyin facade's too.
#[test]
fn a_pin_written_profile_opens_and_saves_with_its_logical_records() {
    let user_dir = TempUserDir::new("handoff-929");
    write_pin_profile("save-929-logical-cross-open-zhuyin", &user_dir.path);
    let expected = [
        rows(&[("01000007", "250000000200000725000000")]),
        rows(&[
            ("969f0000", ""),
            ("969f0000969f0000", "02000007"),
            ("989f0000", ""),
            ("989f0000989f0000", "01000007"),
        ]),
        rows(&[
            ("0400", ""),
            ("04000400", "0100000784208420"),
            ("1200", ""),
            ("12001200", "0200000792409240"),
            ("8400", ""),
            ("84008400", "0100000784208420"),
            ("9200", ""),
            ("92009200", "0200000792409240"),
        ]),
    ];
    assert_eq!(user_rows(&user_dir.path), expected);
    let logs = LogCapture::new();
    let (context, instance) = open_on(&contract_data_dir(), &user_dir.path);
    let tokens: Vec<Vec<u32>> = ["龘龘", "龖龖", "靐靐"]
        .iter()
        .map(|text| tokens_of(instance, text))
        .collect();
    assert_eq!(tokens, [vec![0x0700_0001], vec![0x0700_0002], vec![]]);
    assert!(import(context, 7, "龘龘", "ㄉㄚˊ ㄉㄚˊ".as_bytes(), 0));
    assert!(zhuyin_save(context));
    close(context, instance);
    assert_eq!(logs.take(), []);
    assert_eq!(user_rows(&user_dir.path), expected);
}

/// Replaces the hash DBM at `path` with one that holds exactly `rows`.
fn craft_hash(path: &Path, rows: &[(Vec<u8>, Vec<u8>)]) {
    use oxpinyin_store::{WriteStore as _, WriteTxn as _};
    if path.exists() {
        std::fs::remove_file(path).expect("replace the DBM");
    }
    let store = oxpinyin_store::DefaultStore::create_hash(path).expect("create the DBM");
    store
        .write(|txn| {
            for (key, value) in rows {
                txn.put_raw(key, value)?;
            }
            Ok(())
        })
        .expect("write the DBM rows");
}

/// Three user grams whose keys share TinyDBM bucket 74121 (#1114): degree
/// 1, 6 and 100, each item count 69.
fn collision_rows() -> Vec<(Vec<u8>, Vec<u8>)> {
    [
        (0x0100_0d46_u32, 1_u32),
        (0x0100_4404, 6),
        (0x0100_4dd8, 100),
    ]
    .into_iter()
    .map(|(token, degree)| {
        let mut value = (69 * degree).to_le_bytes().to_vec();
        for item in 0x0100_0001..0x0100_0001 + degree {
            value.extend(item.to_le_bytes());
            value.extend(69_u32.to_le_bytes());
        }
        (token.to_le_bytes().to_vec(), value)
    })
    .collect()
}

/// A saved tkrzw HashDBM with only its timestamp (bytes 48..56) zeroed.
fn masked(raw: &[u8]) -> Vec<u8> {
    let mut masked = raw.to_vec();
    masked[48..56].fill(0);
    masked
}

/// Two cycles of import, save and reopen on `user_dir`. Each save keeps the
/// colliding rows and writes the same 4199312 bytes, timestamp masked.
fn collision_saves(system: &Path, user_dir: &Path) {
    use sha2::Digest as _;
    let expected: Vec<(String, String)> = {
        let mut rows: Vec<_> = collision_rows()
            .iter()
            .map(|(key, value)| (hex(key), hex(value)))
            .collect();
        rows.sort();
        rows
    };
    let path = user_dir.join("user_bigram.db");
    for (text, reading) in [("达达", "ㄉㄚˊ ㄉㄚˊ"), ("你你", "ㄋㄧˇ ㄋㄧˇ")] {
        let (context, instance) = open_on(system, user_dir);
        assert!(import(context, 7, text, reading.as_bytes(), 100));
        assert!(zhuyin_save(context));
        close(context, instance);
        let raw = std::fs::read(&path).expect("the saved bigram");
        assert_eq!(raw.len(), 4_199_312);
        assert_eq!(
            hex(&sha2::Sha256::digest(masked(&raw))),
            "44042431c088b786e90ee01b493aa01661912dda345a8cc5f758a41c682f7c22"
        );
        assert_eq!(container_rows(&path, true), expected);
    }
}

/// Colliding user-bigram keys survive two load and save cycles, in this
/// side's profile and in the pin's: both write the pin's bytes, timestamp
/// masked, and the pin's profile opens with no GLib record (#1114). The
/// expected bytes are the pinyin facade's too.
#[test]
fn colliding_bigram_keys_save_the_pins_bytes() {
    let system = contract_data_dir();
    let own = TempUserDir::new("collision-own");
    let context = init_on(&system, Some(&own.path));
    assert!(!context.is_null());
    zhuyin_fini(context);
    craft_hash(&own.path.join("user_bigram.db"), &collision_rows());
    write_user_conf(&own.path, &system);
    collision_saves(&system, &own.path);

    let pin = TempUserDir::new("collision-pin");
    write_pin_profile("issue1114-collision-load-save-zhuyin", &pin.path);
    let read = |dir: &Path| masked(&std::fs::read(dir.join("user_bigram.db")).expect("a bigram"));
    assert!(read(&pin.path) == read(&own.path), "the saved bytes differ");
    let logs = LogCapture::new();
    collision_saves(&system, &pin.path);
    assert_eq!(logs.take(), []);
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

/// With five tokens for 你好, an import into the unused library 8 asserts
/// in the phrase-table search (`phrase_large_table3.h:95`).
#[test]
fn an_import_into_an_unused_library_after_five_aborts() {
    assert_aborts(
        "pinned_data_tests::an_import_into_an_unused_library_after_five_aborts",
        || {
            let user_dir = TempUserDir::new("unused-index");
            let (context, _instance) = five_libraries(&user_dir.path);
            import(context, 8, "你好", "ㄋㄧˇ ㄏㄠˇ".as_bytes(), 3);
        },
    );
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
        "pinned_data_tests::a_prefix_guess_over_five_library_tokens_aborts",
        || {
            let user_dir = TempUserDir::new("prefix-five");
            let (context, _instance) = five_libraries(&user_dir.path);
            let instance = zhuyin_alloc_instance(context);
            zhuyin_guess_sentence_with_prefix(instance, cstr("你好").as_ptr());
        },
    );
}

/// Training a forced choice that the sentence no longer holds asserts
/// (register row 66).
#[test]
fn training_a_stale_forced_choice_aborts() {
    assert_aborts(
        "pinned_data_tests::training_a_stale_forced_choice_aborts",
        || {
            let user_dir = TempUserDir::new("stale-forcing");
            let (context, _instance) = open_on(&contract_data_dir(), &user_dir.path);
            let instance = zhuyin_alloc_instance(context);
            assert_eq!(
                zhuyin_parse_more_chewings(instance, cstr("su3cl3").as_ptr()),
                6
            );
            assert!(zhuyin_guess_sentence(instance));
            assert!(zhuyin_guess_candidates_after_cursor(instance, 0));
            let (chosen, _, _) = candidate_rows(instance)
                .into_iter()
                .find(|(_, _, text)| text.as_slice() == "泥".as_bytes())
                .expect("a 泥 row");
            zhuyin_choose_candidate(instance, 0, chosen);
            zhuyin_train(instance);
        },
    );
}

/// The raw bytes of a NUL-terminated string.
fn bytes_at(text: *const c_char) -> Vec<u8> {
    assert!(!text.is_null());
    // SAFETY: the facade answered a live NUL-terminated string.
    unsafe { std::ffi::CStr::from_ptr(text) }
        .to_bytes()
        .to_vec()
}

/// The candidate rows of the current list: type and text bytes.
fn candidate_rows(
    instance: *mut ZhuyinInstance,
) -> Vec<(*mut LookupCandidate, lookup_candidate_type_t, Vec<u8>)> {
    let mut count = 0;
    assert!(zhuyin_get_n_candidate(instance, &raw mut count));
    (0..count)
        .map(|index| {
            let mut candidate: *mut LookupCandidate = ptr::null_mut();
            assert!(zhuyin_get_candidate(instance, index, &raw mut candidate));
            let mut kind = lookup_candidate_type_t::ZOMBIE_CANDIDATE;
            assert!(zhuyin_get_candidate_type(
                instance,
                candidate,
                &raw mut kind
            ));
            let mut text: *const c_char = ptr::null();
            assert!(zhuyin_get_candidate_string(
                instance,
                candidate,
                &raw mut text
            ));
            (candidate, kind, bytes_at(text))
        })
        .collect()
}

/// The resplit table under the separator options: the window at offset 0 of
/// `xian` holds the single syllables, led by 县, 见 and 线, and no two-syllable
/// 西安 split.
#[test]
fn the_resplit_xian_window_lists_single_syllables_only() {
    let user_dir = TempUserDir::new("resplit-xian");
    let (context, instance) = open_on(&contract_data_dir(), &user_dir.path);
    assert!(zhuyin_set_options(context, 0x18a));
    assert_eq!(
        zhuyin_parse_more_full_pinyins(instance, cstr("xian").as_ptr()),
        4
    );
    assert!(zhuyin_guess_candidates_after_cursor(instance, 0));
    let texts: Vec<String> = candidate_rows(instance)
        .into_iter()
        .map(|(_, _, text)| String::from_utf8(text).expect("UTF-8"))
        .collect();
    assert_eq!(texts[..3], ["县", "见", "线"]);
    for split in ["西安", "西岸", "锡安"] {
        assert!(!texts.iter().any(|text| text == split), "{split}");
    }
    close(context, instance);
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
        oxpinyin_data::chunk_format::build_memory_chunk(&payload).expect("frame"),
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
        zhuyin_parse_more_full_pinyins(instance, cstr("da1da1da1ba1").as_ptr()),
        12
    );
    assert!(zhuyin_guess_candidates_after_cursor(instance, 0));
    let (candidate, _, chosen) = candidate_rows(instance)
        .into_iter()
        .find(|(_, kind, _)| *kind == lookup_candidate_type_t::NORMAL_CANDIDATE_AFTER_CURSOR)
        .expect("the stored phrase row");
    assert_eq!(hex(&chosen), "e794b2f4908080e4b999");
    assert_eq!(zhuyin_choose_candidate(instance, 0, candidate), 9);
    assert!(zhuyin_guess_sentence(instance));
    let mut sentence: *mut c_char = ptr::null_mut();
    assert!(zhuyin_get_sentence(instance, &raw mut sentence));
    assert_eq!(hex(&bytes_at(sentence)), "e794b2f4908080e4b999e4b899");
    // SAFETY: the sentence is the caller's GLib string, freed once.
    unsafe { glib_sys::g_free(sentence.cast()) };
    assert!(zhuyin_train(instance));
    for token in [0x0100_0001, 0x0100_0002] {
        let mut freq = 0;
        assert!(zhuyin_token_get_unigram_frequency(
            instance,
            token,
            &raw mut freq
        ));
        assert_eq!(freq, 583, "{token:#x}");
    }
    close(context, instance);
}
