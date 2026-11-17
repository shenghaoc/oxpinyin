//! White-box tests needing private field access; the public assembly
//! seam is covered by the `backend_files` module below.

use super::*;
use oxpinyin_user::PinyinKey;

fn w3_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("fixtures")
        .join("w3")
        .join(oxpinyin_data::DEFAULT_STORE_EXT)
}

#[test]
fn direct_library_visibility_also_guards_session_mutations() {
    for law in [UserConfLaw::Pinyin, UserConfLaw::Zhuyin] {
        let mut runtime = Runtime::open_with_law(&w3_dir(), None, law).unwrap();
        let token = runtime.dict().system().tokens_for_text("你").unwrap()[0];
        let library = token >> 24;
        assert!(runtime.unload_library(library));
        assert!(runtime.unload_library(library));
        let store = runtime.user_store_mut();
        let total = store.unigram_total();
        assert!(!store.add_system_unigram_frequency(token, 7));
        assert_eq!(store.add_unigram_outcome(token, 7), UnigramAdd::Missing);
        assert_eq!(store.unigram_total(), total);
        assert!(runtime.load_library(library));
        assert!(!runtime.load_library(library));
        assert!(
            runtime
                .user_store_mut()
                .add_system_unigram_frequency(token, 7)
        );
    }
}

#[test]
fn runtime_visibility_preserves_user_library_save_state() {
    for law in [UserConfLaw::Pinyin, UserConfLaw::Zhuyin] {
        let dir = tempfile::tempdir().unwrap();
        let mut runtime = Runtime::open_with_law(&w3_dir(), Some(dir.path()), law).unwrap();
        let library = oxpinyin_user::USER_DICTIONARY;
        let chunk = dir.path().join("user.bin");
        let ni = SyllableKey::from_text("ni").unwrap();
        let keys = [oxpinyin_user::toned_key(ni.index(), 3).unwrap()];
        let store = runtime.user_store_mut();
        assert!(!store.is_system_library(library));
        let saved = store
            .add_phrase_in(library, &"龘".into(), &keys, Some(20))
            .unwrap();
        store.mark_modified();
        store.save_reporting().unwrap();
        let original = std::fs::read(&chunk).unwrap();
        // Only the store's own unload drops the items and arms save's
        // skip. A runtime unload/load pair must not clear that marker
        // and overwrite the saved chunk with an empty loaded library.
        runtime.user_store_mut().unload_user_library(library);
        assert!(runtime.unload_library(u32::from(library)));
        assert!(runtime.load_library(u32::from(library)));
        let store = runtime.user_store_mut();
        assert!(store.phrase(saved).is_none());
        store.mark_modified();
        store.save_reporting().unwrap();
        assert_eq!(std::fs::read(&chunk).unwrap(), original);
        assert!(!dir.path().join("user.bin.tmp").exists());

        store.load_user_library(library);
        assert!(store.phrase(saved).is_some());
        let added = store
            .add_phrase_in(library, &"鱻".into(), &keys, Some(30))
            .unwrap();

        // Runtime visibility alone must not insert the store's unloaded
        // marker: the store still owns its items and must save them.
        assert!(runtime.unload_library(u32::from(library)));
        let store = runtime.user_store_mut();
        assert!(store.phrase(added).is_some());
        store.mark_modified();
        store.save_reporting().unwrap();
        let updated = std::fs::read(&chunk).unwrap();
        assert_ne!(updated, original, "the loaded store saves its new item");
        assert!(runtime.load_library(u32::from(library)));
        let store = runtime.user_store_mut();
        store.mark_modified();
        store.save_reporting().unwrap();
        assert_eq!(std::fs::read(&chunk).unwrap(), updated);
    }
}

#[test]
fn unloading_an_absent_system_chunk_still_guards_the_session() {
    let system = tempfile::tempdir().unwrap();
    for entry in std::fs::read_dir(w3_dir()).unwrap() {
        let path = entry.unwrap().path();
        std::fs::copy(&path, system.path().join(path.file_name().unwrap())).unwrap();
    }
    std::fs::remove_file(system.path().join("gb_char.bin")).unwrap();
    for law in [UserConfLaw::Pinyin, UserConfLaw::Zhuyin] {
        let mut runtime = Runtime::open_with_law(system.path(), None, law).unwrap();
        assert!(!runtime.libraries().is_loaded(1));
        assert!(runtime.unload_library(1));
        let store = runtime.user_store_mut();
        let total = store.unigram_total();
        assert_eq!(
            store.add_unigram_outcome(0x0100_0001, 7),
            UnigramAdd::Missing
        );
        assert_eq!(store.unigram_total(), total);
    }
}

/// The save's library loop never visits the reserved row 0: a
/// library loaded there is not mapped again, so a directory in its
/// file's place does not fault. Library 1 at the same path does.
#[cfg(unix)]
#[test]
fn the_save_remap_skips_the_reserved_library() {
    let system = tempfile::tempdir().unwrap();
    for entry in std::fs::read_dir(w3_dir()).unwrap() {
        let path = entry.unwrap().path();
        std::fs::copy(&path, system.path().join(path.file_name().unwrap())).unwrap();
    }
    let mut runtime = Runtime::open_with_law(system.path(), None, UserConfLaw::Pinyin).unwrap();
    let gb = system.path().join("gb_char.bin");
    assert!(
        runtime
            .originals_mut()
            .phrase_libraries_mut()
            .load(0, &gb)
            .unwrap()
    );
    std::fs::remove_file(&gb).unwrap();
    std::fs::create_dir(&gb).unwrap();
    std::fs::write(gb.join("e".repeat(25)), b"x").unwrap();
    let originals = runtime.user.originals();
    assert!(matches!(
        Runtime::read_save_baseline(originals, 0, 0),
        Ok(SaveBaseline::Keep)
    ));
    assert!(Runtime::read_save_baseline(originals, 0, 1).is_err());
}

/// `PHRASE_INDEX_LIBRARY_INDEX` ignores the high four bits
/// (`novel_types.h:42-44` at 074a2219): a tagged token reads the
/// item's own pronunciation count.
#[test]
fn pronunciation_count_ignores_the_token_tag_nibble() {
    let runtime = Runtime::open_with_law(&w3_dir(), None, UserConfLaw::Pinyin).unwrap();
    let token = runtime.dict().system().tokens_for_text("你").unwrap()[0];
    let count = runtime.dict().token_pronunciation_count(token);
    assert!(count.is_some());
    for tag in [0x1000_0000, 0xf000_0000] {
        assert_eq!(runtime.dict().token_pronunciation_count(tag | token), count);
    }
}

#[test]
fn unigram_freq_uses_the_live_count_while_the_library_is_visible() {
    let mut runtime = Runtime::open(&w3_dir(), None).unwrap();
    let token = runtime.dict().system().tokens_for_text("你").unwrap()[0];
    let library = runtime.libraries().library(token).unwrap();
    assert!(library.is_detached());
    assert_eq!(runtime.user_store().unigram_delta(token), 0);
    let initial = runtime.user_store().system_unigram(token).unwrap();
    assert!(initial > 0);

    for (increment, delta) in [(0, 0), (9, 9)] {
        if increment != 0 {
            assert!(runtime.add_unigram_delta(token, increment));
        }
        // A session keeps no delta for a system item: the increment is in
        // the live field.
        assert_eq!(runtime.user_store().unigram_delta(token), 0);
        assert_eq!(
            runtime.lm().unigram_freq(&PhraseToken::new(token)).unwrap(),
            Some(u64::from(initial) + delta),
            "visible live count wins, including when the original is detached"
        );

        // Keep the live buffer while masking the library.
        runtime.library_mask.set(1 << (token >> 24));
        runtime
            .user_store_mut()
            .set_library_loaded((token >> 24) as u8, false);
        assert_eq!(
            runtime.user_store().system_unigram(token),
            Some(initial + delta as u32)
        );
        assert_eq!(
            runtime.lm().unigram_freq(&PhraseToken::new(token)).unwrap(),
            Some(0),
            "the masked token cannot use the live system count"
        );
        runtime.library_mask.set(0);
        runtime
            .user_store_mut()
            .set_library_loaded((token >> 24) as u8, true);
    }
}

/// A step prices the token it ranges from the token's item
/// (`get_phrase_item` then `compute_pronunciation_possibility`,
/// `phonetic_lookup.h:646-656` at 074a2219): a system token appended
/// to a system library and indexed by the user table, as well as a
/// shipped one, whatever the lookup that listed it carries.
#[test]
fn a_step_prices_system_tokens_the_user_table_indexes() {
    let mut runtime = Runtime::open(&w3_dir(), None).unwrap();
    let ni = SyllableKey::from_text("ni").unwrap();
    let shipped = runtime.dict().system().tokens_for_text("你").unwrap()[0];
    let keys = [oxpinyin_user::toned_key(ni.index(), 3).unwrap()];
    let appended = runtime
        .user_store_mut()
        .add_system_phrase_in(1, None, &"龘".into(), &keys, Some(5))
        .unwrap();
    let listed = runtime.dict().lookup(&[ni]).unwrap();
    for token in [shipped, appended] {
        assert!(listed.iter().any(|entry| entry.token().value() == token));
        let item = runtime
            .dict()
            .trellis_item(&[ni], &[], token)
            .unwrap()
            .expect("the step finds the item");
        assert!(item.pronunciation.is_some(), "token {token:#x} is priced");
    }
    assert!(
        runtime
            .dict()
            .trellis_records(&[ni], &[3])
            .unwrap()
            .iter()
            .any(|entry| entry.token().value() == shipped
                && entry.pronunciation_possibility().is_some())
    );
}

#[test]
fn unigram_freq_reads_increments_from_the_user_item() {
    let mut runtime = Runtime::open(&w3_dir(), None).unwrap();
    let ni = SyllableKey::from_text("ni").unwrap();
    let keys = [oxpinyin_user::toned_key(ni.index(), 3).unwrap()];
    for (text, count, expected) in [("鎄", 5, 15), ("龘", 0, 0)] {
        let token = runtime
            .user_store_mut()
            .add_phrase_in(5, &text.into(), &keys, Some(count))
            .unwrap();
        assert_eq!(token >> 24, 5);
        assert_eq!(runtime.user_store().system_unigram(token), None);
        assert_eq!(
            runtime.lm().unigram_freq(&PhraseToken::new(token)).unwrap(),
            Some(expected)
        );
        assert!(runtime.add_unigram_delta(token, 7));
        assert_eq!(
            runtime.lm().unigram_freq(&PhraseToken::new(token)).unwrap(),
            Some(expected + 7)
        );
    }
}

#[test]
fn unigram_freq_reads_a_user_items_count_live() {
    let mut store = UserStore::open_transient(
        oxpinyin_user::SystemOriginals::new(Default::default(), UserFileLayout::stock()),
        SystemVersions::for_this_build(7, 14),
        UserConfLaw::Pinyin,
    )
    .unwrap();
    let ci = SyllableKey::from_text("ci").unwrap();
    let keys = [oxpinyin_user::toned_key(ci.index(), 0).unwrap()];
    let token = store.add_phrase(&"词".into(), &keys, Some(5)).unwrap();
    assert_eq!(store.system_unigram(token), None);
    let mut runtime = Runtime::open(&w3_dir(), None).unwrap();
    runtime.user = store;
    for _ in 0..2 {
        assert_eq!(
            runtime.lm().unigram_freq(&PhraseToken::new(token)).unwrap(),
            Some(15)
        );
    }
    runtime
        .user_store_mut()
        .observe_selection(1, token)
        .unwrap();
    assert_eq!(
        runtime.lm().unigram_freq(&PhraseToken::new(token)).unwrap(),
        Some(15 + 483),
        "a committed write invalidates the cached count"
    );
}

#[test]
fn unigram_freq_preserves_removed_and_missing_item_fallbacks() {
    let mut runtime = Runtime::open(&w3_dir(), None).unwrap();
    let ni = SyllableKey::from_text("ni").unwrap();
    let keys = [oxpinyin_user::toned_key(ni.index(), 3).unwrap()];
    let token = runtime
        .user_store_mut()
        .add_system_phrase_in(1, None, &"龘".into(), &keys, Some(0))
        .unwrap();
    assert_eq!(runtime.user_store().system_unigram(token), Some(0));
    assert_eq!(
        runtime.lm().unigram_freq(&PhraseToken::new(token)).unwrap(),
        Some(0)
    );
    let dir = tempfile::tempdir().unwrap();
    let token = runtime.dict().system().tokens_for_text("你").unwrap()[0];
    assert_eq!(token >> 24, 1);
    let old_item = runtime
        .dict()
        .live_items()
        .read(token, |item| match item {
            oxpinyin_user::SystemItem::Shipped(view)
            | oxpinyin_user::SystemItem::Replayed(view) => view.as_bytes().to_vec(),
        })
        .found()
        .unwrap();
    let payload = oxpinyin_data::user_files::encode_log_records(&[
        oxpinyin_data::user_files::LogRecord::Remove {
            token: token & oxpinyin_user::PHRASE_MASK,
            old_item,
        },
    ])
    .unwrap();
    std::fs::write(
        dir.path().join("gb_char.dbin"),
        oxpinyin_data::chunk_format::build_memory_chunk(&payload).unwrap(),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("user.conf"),
        oxpinyin_data::user_files::UserTableInfo::conform_to(&SystemVersions::from_conf(
            runtime.table_conf(),
        ))
        .to_text(),
    )
    .unwrap();
    drop(runtime);
    let runtime = Runtime::open(&w3_dir(), Some(dir.path())).unwrap();
    assert_eq!(runtime.user_store().system_unigram(token), None);
    assert_eq!(
        runtime.dict().live_items().read(token, |_| ()),
        oxpinyin_user::Lookup::Absent
    );
    // The removed item leaves no entry.
    let entries = runtime
        .dict()
        .lookup(&[SyllableKey::from_text("ni").unwrap()])
        .unwrap();
    assert!(entries.iter().all(|entry| entry.token().value() != token));
    assert_eq!(
        runtime.lm().unigram_freq(&PhraseToken::new(token)).unwrap(),
        Some(0)
    );
    assert_eq!(
        runtime
            .lm()
            .unigram_freq(&PhraseToken::new(u32::MAX))
            .unwrap(),
        Some(0)
    );
}

#[test]
fn zhuyin_open_holds_no_addon_or_punct_handles() {
    let system = w3_dir();
    let pinyin = Runtime::open(&system, None).expect("pinyin fixture opens");
    assert!(
        pinyin.dict().addons.borrow().dict.has_open_indexes(),
        "vacuity guard: pinyin eagerly opens the fixture's addon indexes"
    );
    assert!(
        pinyin.dict().punct.is_open(),
        "vacuity guard: pinyin eagerly opens the fixture's punctuation table"
    );

    let user = tempfile::tempdir().unwrap();
    for user_dir in [None, Some(user.path())] {
        let mut zhuyin = Runtime::open_with_law(&system, user_dir, UserConfLaw::Zhuyin)
            .expect("zhuyin fixture opens");
        // An empty loaded-library set would also pass before the fix;
        // inspect the actual index handles, not just visible items.
        assert!(!zhuyin.dict().addons.borrow().dict.has_open_indexes());
        assert!(!zhuyin.dict().punct.is_open());

        // Default user library 5 (addon.bin) is separate from the
        // addon DBM pair and remains available to zhuyin imports.

        let ni = SyllableKey::from_text("ni").unwrap();
        let hao = SyllableKey::from_text("hao").unwrap();
        let keys = [
            oxpinyin_user::toned_key(ni.index(), 3).unwrap(),
            oxpinyin_user::toned_key(hao.index(), 3).unwrap(),
        ];
        let token = zhuyin
            .user_store_mut()
            .add_phrase_in(5, &"你鎄".into(), &keys, Some(5))
            .unwrap();
        assert_eq!(token >> 24, 5);
        assert!(
            zhuyin
                .dict()
                .tokens_for_text("你鎄")
                .contains(&PhraseToken::new(token))
        );
    }

    // A table.conf whose addon 4 names a non-stock chunk: the stock
    // rows would say `art.bin`, so a facade built from them would
    // lose the configuration.
    let system = tempfile::tempdir().unwrap();
    for entry in std::fs::read_dir(w3_dir()).unwrap() {
        let path = entry.unwrap().path();
        std::fs::copy(&path, system.path().join(path.file_name().unwrap())).unwrap();
    }
    let conf_path = system.path().join("table.conf");
    let conf = std::fs::read_to_string(&conf_path).unwrap();
    let edited = conf.replace(
        "addon 4 art.table art.bin NULL DICTIONARY",
        "addon 4 art.table art-local.bin NULL DICTIONARY",
    );
    assert_ne!(conf, edited, "vacuity guard: the fixture names art.bin");
    std::fs::write(&conf_path, edited).unwrap();

    for user_dir in [None, Some(user.path())] {
        let zhuyin = Runtime::open_with_law(system.path(), user_dir, UserConfLaw::Zhuyin)
            .expect("edited fixture opens");
        let addons = zhuyin.dict().addons.borrow();
        assert!(
            !addons.dict.has_open_indexes(),
            "no addon index handle is open"
        );
        let row = addons.dict.row(4).expect("sub-index 4 is in range");
        assert_eq!(
            row.system,
            Some(oxpinyin_data::table_info::FileName::from_bytes(
                b"art-local.bin"
            )),
            "the configured addon row is kept, not the stock one"
        );
        assert_eq!(Some(row), zhuyin.table_conf().addon_row(4));
        assert!(!zhuyin.dict().punct.is_open());
    }
}

#[test]
fn tokens_for_text_reads_live_rows_without_rebuilding_user_lookup() {
    let dir = tempfile::tempdir().unwrap();
    let mut runtime = Runtime::open(&w3_dir(), Some(dir.path())).unwrap();

    let phrase = "你鎄";
    assert!(runtime.dict().tokens_for_text(phrase).is_empty());

    // Each operation re-borrows the same storage owner. Exact-text
    // queries still read their live rows after writes.

    let keys: Vec<_> = ["ni", "hao"]
        .into_iter()
        .map(|text| PinyinKey::try_from(SyllableKey::from_text(text).unwrap().index()).unwrap())
        .collect();

    let token = runtime
        .user_store_mut()
        .add_phrase(&oxpinyin_core::PhraseText::from(phrase), &keys, None)
        .unwrap();
    assert_eq!(
        runtime.dict().tokens_for_text(phrase),
        [PhraseToken::new(token)]
    );
    let syllables = [
        SyllableKey::from_text("ni").unwrap(),
        SyllableKey::from_text("hao").unwrap(),
    ];
    assert!(
        runtime
            .dict()
            .user_lookup()
            .lookup(&syllables)
            .iter()
            .any(|entry| entry.token().value() == token && entry.text() == phrase)
    );
    assert_eq!(
        runtime.dict().user_lookup().phrase_text(token).unwrap(),
        phrase
    );
    assert_eq!(
        runtime
            .user_store_mut()
            .add_phrase(&oxpinyin_core::PhraseText::from(phrase), &keys, Some(5))
            .unwrap(),
        token
    );
    assert_eq!(
        runtime.dict().tokens_for_text(phrase),
        [PhraseToken::new(token)]
    );
    assert!(runtime.user_store_mut().remove_user_phrase(token).unwrap());
    assert!(runtime.dict().tokens_for_text(phrase).is_empty());

    // A later operation re-borrows the owner without rebuilding query state.

    let imported = runtime
        .user_store_mut()
        .add_phrase_in(7, &oxpinyin_core::PhraseText::from(phrase), &keys, Some(5))
        .unwrap();
    assert_eq!(
        runtime.dict().tokens_for_text(phrase),
        [PhraseToken::new(imported)]
    );
    runtime
        .user_store_mut()
        .mask_out(u32::MAX, imported)
        .unwrap();
    assert!(runtime.dict().tokens_for_text(phrase).is_empty());
    assert!(
        runtime
            .dict()
            .user_lookup()
            .tokens_for_ucs4(&phrase.chars().map(u32::from).collect::<Vec<_>>())
            .is_empty()
    );
}

#[test]
fn tokens_for_text_reads_the_row_after_a_system_reading_merge() {
    let dir = tempfile::tempdir().unwrap();
    let mut runtime = Runtime::open(&w3_dir(), Some(dir.path())).unwrap();

    let tokens = runtime.dict().tokens_for_text("你好");
    let token = tokens[0].value();
    let library = u8::try_from(token >> 24).unwrap();
    let keys: Vec<_> = ["ni", "hao"]
        .into_iter()
        .map(|text| PinyinKey::try_from(SyllableKey::from_text(text).unwrap().index()).unwrap())
        .collect();

    runtime
        .user_store_mut()
        .add_system_phrase_in(library, Some(token), &"你好".into(), &keys, Some(5))
        .unwrap();
    assert_eq!(runtime.dict().tokens_for_text("你好"), tokens);
    let added = runtime
        .user_store_mut()
        .add_system_phrase_in(library, None, &"你鎄".into(), &keys, Some(5))
        .unwrap();
    assert_eq!(
        runtime.dict().tokens_for_text("你鎄"),
        [PhraseToken::new(added)]
    );
}

#[test]
fn suggestion_rows_group_by_library_then_walk_the_ucs4_keys() {
    // The pin's list: system library groups first, the user library
    // last, each in the DBM's byte-lexical UCS-4 order — not text
    // (UTF-8 / code point) order. U+4E50 sorts before U+4F2D by code
    // point but after it by little-endian bytes (0x50 > 0x2D).
    let system = vec![0x0200_0001, 0x0100_0010];
    let user_rows = vec![
        (
            0x0700_0001,
            oxpinyin_data::row_format::phrase_index::encode_ucs4_key("中乐"),
        ), // U+4E50
        (
            0x0700_0002,
            oxpinyin_data::row_format::phrase_index::encode_ucs4_key("中伭"),
        ), // U+4F2D
    ];
    let merged = merge_suggestion_rows(&system, &user_rows);
    assert_eq!(merged, [0x0100_0010, 0x0200_0001, 0x0700_0002, 0x0700_0001]);
}

#[test]
fn suggestion_merge_retains_missing_tokens_and_duplicate_occurrences() {
    let system = [0x0100_0001];
    let user_rows = [(0x0100_0001, vec![0xff; 4]), (0x0700_0002, vec![0; 4])];
    assert_eq!(
        merge_suggestion_rows(&system, &user_rows),
        [0x0100_0001, 0x0100_0001, 0x0700_0002]
    );
}

use super::{UserStoreError, user_gram_or_none};

/// `m_user_bigram->load` answers any failed read with no gram; a row
/// that reads passes through.
#[test]
fn a_failed_user_read_is_no_gram() {
    let io = UserStoreError::Io(std::io::Error::other("injected"));
    assert_eq!(user_gram_or_none(Err(io)).unwrap(), None);
    assert_eq!(
        user_gram_or_none(Err(UserStoreError::Decode)).unwrap(),
        None
    );
    assert_eq!(
        user_gram_or_none(Ok(Some(vec![1, 2]))).unwrap(),
        Some(vec![1, 2])
    );
    assert_eq!(user_gram_or_none(Ok(None)).unwrap(), None);
}

#[path = "../tests/common/candidate_fixture.rs"]
mod candidate_fixture;
#[cfg(feature = "tkrzw")]
#[path = "../tests/common/contract_data.rs"]
mod contract_data;
#[cfg(feature = "tkrzw")]
#[path = "../tests/common/sentence_fixture.rs"]
mod sentence_fixture;
#[cfg(feature = "tkrzw")]
#[path = "../tests/common/sentence_tail.rs"]
mod sentence_tail;

// The format of the committed candidate structure capture.
use std::path::PathBuf;

use candidate_fixture::{CandidateFixture, OracleCandidateType, STRUCTURE_FIXTURE};

/// The pin the capture was taken from.
const EXPECTED_PIN_REF: &str = concat!(
    "libpinyin-2.11.92-074a2219c90feaf962d0d24f034514033ece5f99",
    "+model20-59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155",
    "+dbm-tkrzw"
);

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn structure_fixture_has_valid_format_and_candidate_types() {
    let raw = std::fs::read_to_string(repo_root().join(STRUCTURE_FIXTURE))
        .expect("committed structure fixture");
    let fixture = CandidateFixture::parse(&raw).expect("well-formed structure fixture");
    assert_eq!(fixture.pin_ref, EXPECTED_PIN_REF);
    assert_eq!(
        fixture.rows.len(),
        fixture.total_triples,
        "header total_triples"
    );
    let distinct_inputs = fixture
        .rows
        .iter()
        .map(|row| row.input.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    assert_eq!(distinct_inputs, fixture.total_inputs, "header total_inputs");
    let mut previous_input = None;
    let mut expected_rank = 1;
    for row in &fixture.rows {
        if previous_input != Some(row.input.as_str()) {
            assert!(
                previous_input.is_none_or(|previous| previous < row.input.as_str()),
                "inputs sorted bytewise"
            );
            expected_rank = 1;
        }
        assert_eq!(
            row.rank, expected_rank,
            "consecutive ranks for {:?}",
            row.input
        );
        assert!(row.rank <= 10, "capture depth is ten");
        // The 0x1e guess-candidates path excludes LONGER and cannot emit predictions.
        assert_ne!(row.candidate_type, OracleCandidateType::Longer);
        assert!(!row.candidate_type.as_wire().starts_with("PREDICTED_"));
        assert_eq!(
            row.nbest_index.is_some(),
            row.candidate_type == OracleCandidateType::NbestMatch
        );
        previous_input = Some(row.input.as_str());
        expected_rank += 1;
    }
}

mod backend_files {
    //! Assembly-level tests for the shared runtime.
    //!
    //! Moved out of `src/lib.rs`'s inline tests: every one drives the public
    //! seam — `Runtime::open`, session surfaces, store accessors — against
    //! the committed `fixtures/w3/<backend>` data directory (the `--mini`
    //! compile of the pinned model, in the compiled-in backend's container),
    //! which is integration territory (components together, real files, no
    //! private access).

    use crate::{OpenError, Runtime, TableConfError};
    use oxpinyin_core::{Dictionary, LanguageModel, PhraseToken, SyllableKey};
    use oxpinyin_engine::{Selection, SelectionTarget, SessionSettings};
    use oxpinyin_user::{FIRST_USER_TOKEN, PinyinKey, SENTENCE_START};
    use std::path::{Path, PathBuf};

    /// A fresh, uniquely named scratch dir, removed on drop (including unwind).
    /// Declared before the runtime that opens it, so it drops after every
    /// handle into it.
    fn scratch_dir(prefix: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(prefix)
            .tempdir()
            .expect("temp dir")
    }

    fn w3_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("fixtures")
            .join("w3")
            .join(oxpinyin_data::DEFAULT_STORE_EXT)
    }

    #[test]
    fn token_text_reads_live_items_and_obeys_library_visibility() {
        let mut runtime = Runtime::open(&w3_dir(), None).expect("runtime");
        let system = runtime
            .dict()
            .system()
            .tokens_for_text("你")
            .expect("lookup")[0];
        let ni = SyllableKey::from_text("ni").expect("ni");
        let keys = [oxpinyin_user::toned_key(ni.index(), 3).expect("toned key")];
        assert_eq!(
            runtime
                .dict()
                .token_text(system)
                .as_ref()
                .and_then(|text| text.as_str()),
            Some("你")
        );
        runtime
            .user_store_mut()
            .add_system_phrase_in(1, Some(system), &"你".into(), &keys, Some(5))
            .expect("live system item");
        assert_eq!(
            runtime
                .dict()
                .token_text(system)
                .as_ref()
                .and_then(|text| text.as_str()),
            Some("你")
        );
        let user = runtime
            .user_store_mut()
            .add_phrase(&"词".into(), &keys, Some(5))
            .expect("user item");
        assert_eq!(
            runtime
                .dict()
                .token_text(user)
                .as_ref()
                .and_then(|text| text.as_str()),
            Some("词")
        );
        for (library, token, text) in [(1, system, "你"), (7, user, "词")] {
            assert!(runtime.unload_library(library));
            assert_eq!(runtime.dict().token_text(token), None);
            assert!(runtime.load_library(library));
            assert_eq!(
                runtime
                    .dict()
                    .token_text(token)
                    .as_ref()
                    .and_then(|text| text.as_str()),
                Some(text)
            );
        }
        assert_eq!(runtime.dict().token_text(0), None);
        for words in [
            vec![0x7532, 0, 0x8000_0000],
            vec![0xd800, 0x110000],
            vec![0x8000_0000],
        ] {
            let text = oxpinyin_core::PhraseText::from_stored_parts(words.clone(), words.len());
            let token = runtime
                .user_store_mut()
                .add_phrase(&text, &vec![keys[0]; words.len()], Some(0))
                .expect("stored user words");
            let actual = runtime.dict().token_text(token);
            let expected = oxpinyin_core::PhraseText::from_words(words);
            assert_eq!(actual, expected);
            if let (Some(actual), Some(expected)) = (actual, expected) {
                assert_eq!(actual.phrase_length(), expected.phrase_length());
                assert_eq!(actual.stored_words(), expected.stored_words());
            }
        }
    }

    #[test]
    fn narrow_token_reads_preserve_item_order_and_visibility() {
        let mut runtime = Runtime::open(&w3_dir(), None).expect("runtime");
        let system = runtime
            .dict()
            .system()
            .tokens_for_text("你")
            .expect("lookup")[0];
        let packed = |runtime: &Runtime, token, nth| {
            let reading = runtime
                .dict()
                .token_nth_pronunciation(token, nth)
                .expect("item");
            assert_eq!(reading.phrase_length, 1);
            reading.keys
        };
        let original = packed(&runtime, system, 0).expect("system reading");
        assert!(runtime.dict().token_exists(system));
        assert_eq!(packed(&runtime, system, usize::MAX), None);
        let ni = SyllableKey::from_text("ni").unwrap();
        let keys = [oxpinyin_user::toned_key(ni.index(), 3).unwrap()];
        runtime
            .user_store_mut()
            .add_system_phrase_in(1, Some(system), &"你".into(), &keys, Some(5))
            .expect("override");
        assert_eq!(packed(&runtime, system, 0).as_ref(), Some(&original));
        // Descending tones make insertion order differ from table-key order.
        let user = runtime
            .user_store_mut()
            .add_phrase(&"词".into(), &keys, Some(5))
            .unwrap();
        let second = [oxpinyin_user::toned_key(ni.index(), 1).unwrap()];
        runtime
            .user_store_mut()
            .add_phrase(&"词".into(), &second, Some(9))
            .unwrap();
        // A `USER_FILE` reading is read in place from the session arena.
        let user_packed = |runtime: &Runtime, nth| {
            runtime
                .dict()
                .with_user_nth_pronunciation_words(user, nth, |length, words| {
                    assert_eq!(length, 1);
                    words.map(|words| {
                        words
                            .as_chunks::<2>()
                            .0
                            .iter()
                            .map(|bytes| u16::from_le_bytes(*bytes))
                            .collect::<Vec<u16>>()
                    })
                })
                .expect("item")
        };
        assert!(runtime.dict().token_nth_pronunciation(user, 0).is_none());
        for (nth, tone) in [(0, 3), (1, 1)] {
            assert_eq!(
                user_packed(&runtime, nth),
                Some(vec![
                    oxpinyin_core::ChewingKey::from_pinyin("ni")
                        .unwrap()
                        .with_tone(tone)
                        .to_packed()
                ])
            );
        }
        assert_eq!(runtime.dict().token_pronunciation_count(user), Some(2));
        assert_eq!(user_packed(&runtime, 2), None);
        for (library, token) in [(1, system), (7, user)] {
            assert!(runtime.dict().token_exists(token));
            assert_eq!(runtime.dict().phrase_length_for_token(token), Some(1));
            assert!(runtime.unload_library(library));
            assert!(!runtime.dict().token_exists(token));
            assert_eq!(runtime.dict().phrase_length_for_token(token), None);
            assert!(runtime.dict().token_nth_pronunciation(token, 0).is_none());
            assert_eq!(runtime.dict().token_pronunciation_count(token), None);
            assert!(runtime.load_library(library));
            assert!(runtime.dict().token_exists(token));
            assert_eq!(runtime.dict().phrase_length_for_token(token), Some(1));
        }
        for token in [0, 0x01ff_ffff, 0x07ff_ffff, u32::MAX] {
            assert!(!runtime.dict().token_exists(token));
            assert_eq!(runtime.dict().phrase_length_for_token(token), None);
            assert!(runtime.dict().token_nth_pronunciation(token, 0).is_none());
        }
    }

    #[test]
    fn trellis_records_filter_user_tones_and_refresh_after_import() {
        let tmp = scratch_dir("oxpinyin-trellis-records-");
        let dir = tmp.path().to_path_buf();
        let mut runtime = Runtime::open(&w3_dir(), Some(&dir)).expect("runtime");
        let syllables = [
            SyllableKey::from_text("ni").unwrap(),
            SyllableKey::from_text("hao").unwrap(),
        ];
        assert!(
            !runtime
                .dict()
                .trellis_records(&syllables, &[3, 3])
                .unwrap()
                .iter()
                .any(|entry| entry.text() == "你鎄")
        );
        let keys: Vec<_> = syllables
            .iter()
            .map(|key| oxpinyin_user::toned_key(key.index(), 3).unwrap())
            .collect();
        let token = runtime
            .user_store_mut()
            .add_phrase(&"你鎄".into(), &keys, Some(5))
            .unwrap();
        let records = runtime.dict().trellis_records(&syllables, &[3, 3]).unwrap();
        assert_eq!(
            records
                .iter()
                .filter(|entry| entry.token().value() == token)
                .count(),
            1
        );
        assert!(
            runtime
                .dict()
                .trellis_records(&syllables, &[1, 3])
                .unwrap()
                .iter()
                .all(|entry| entry.token().value() != token)
        );
        assert_eq!(
            runtime
                .dict()
                .lookup(&syllables)
                .unwrap()
                .iter()
                .filter(|entry| entry.token().value() == token)
                .count(),
            1
        );
        drop(runtime);
        tmp.close().expect("cleanup");
    }

    #[test]
    fn opens_the_w3_fixture_and_decodes_nihao() {
        let runtime = Runtime::open(&w3_dir(), None).expect("the fixture dir opens");
        let mut session = runtime.new_session(SessionSettings::default());
        runtime
            .session(&mut session)
            .replace_raw("nihao")
            .expect("batch typing");
        let first = runtime
            .session(&mut session)
            .candidates()
            .iter()
            .next()
            .expect("nihao has candidates in the fixture")
            .text()
            .to_owned();
        assert_eq!(first, "你好");

        runtime
            .session(&mut session)
            .guess_sentence()
            .expect("sentence guess");
        let read = runtime.session(&mut session);
        let best = read.sentence_text(0).expect("n-best row 0");
        assert_eq!(best, "你好");
    }

    /// `SystemTableInfo2::load` is the pin's first step, and a `table.conf` it
    /// cannot open fails the init however complete the rest of the directory
    /// is (maintainer ruling 2026-10-28 UTC, D3).
    #[test]
    fn open_refuses_a_directory_without_table_conf() {
        let tmp = scratch_dir("oxpinyin-runtime-no-conf-");
        for entry in std::fs::read_dir(w3_dir()).expect("w3 fixture") {
            let entry = entry.expect("w3 entry");
            if entry.file_name() != "table.conf" {
                std::fs::copy(entry.path(), tmp.path().join(entry.file_name())).expect("copy");
            }
        }
        assert!(
            tmp.path()
                .join(oxpinyin_data::SystemDbm::PinyinIndex.file_name())
                .exists()
        );
        match Runtime::open(tmp.path(), None) {
            Err(OpenError::TableConf(TableConfError::Unopenable)) => {}
            Err(other) => panic!("expected an unopenable table.conf, got {other}"),
            Ok(_) => panic!("a directory without table.conf must not open"),
        }
    }

    #[test]
    fn missing_pinyin_index_follows_backend_attach() {
        let tmp = scratch_dir("oxpinyin-runtime-empty-");
        let dir = tmp.path().to_path_buf();
        // Keep the chunks valid: absent chunks reach an independent pin
        // assertion after the ignored DBM attach (#764).
        for entry in std::fs::read_dir(w3_dir()).expect("w3 fixture") {
            let entry = entry.expect("w3 entry");
            if entry.file_name().as_os_str()
                != std::ffi::OsStr::new(&oxpinyin_data::SystemDbm::PinyinIndex.file_name())
            {
                std::fs::copy(entry.path(), dir.join(entry.file_name())).expect("copy");
            }
        }
        // Every backend keeps the failed native object and initializes
        // (pinyin.cpp:357 at 074a2219 ignores the load's answer).
        assert!(Runtime::open(&dir, None).is_ok());
    }

    #[test]
    fn open_reads_no_records_beyond_the_handles() {
        // The fixture opens with real counts straight from the chunk files:
        // no interpolation text, no fixture mode.
        let runtime = Runtime::open(&w3_dir(), None).expect("open");
        let total = LanguageModel::unigram_total(&runtime.lm())
            .expect("query")
            .expect("real unigrams");
        assert!(total > 0, "the mini set carries \\1-gram counts");
    }

    #[test]
    fn selection_advances_and_reset_clears_the_chosen_text() {
        let runtime = Runtime::open(&w3_dir(), None).expect("open");
        let mut session = runtime.new_session(SessionSettings::default());
        runtime
            .session(&mut session)
            .replace_raw("nihao")
            .expect("typing");
        let advanced = {
            let mut view = runtime.session(&mut session);
            let candidate = view
                .candidates()
                .iter()
                .next()
                .cloned()
                .expect("a first candidate");
            let anchor = view.composition_offset();
            view.select_target(&SelectionTarget::of_candidate(&candidate, anchor, None))
                .expect("first candidate selects")
        };
        assert_eq!(advanced, Selection::Completed);
        assert_eq!(runtime.session(&mut session).selected_text(), "你好");
        assert_eq!(
            runtime.session(&mut session).composition_offset(),
            "nihao".len()
        );
        runtime.session(&mut session).reset();
        assert!(!runtime.session(&mut session).is_composing());
        assert!(runtime.session(&mut session).selected_text().is_empty());
    }

    #[test]
    fn usable_user_dir_feeds_the_merged_lookup_and_overlay() {
        // The one case the other tests never reach: a user store that
        // actually opens. Learning must surface through BOTH moved seams —
        // the dictionary's user-phrase merge and the language model's
        // unigram overlay — via nothing but public accessors.
        let tmp = scratch_dir("oxpinyin-runtime-user-");
        let dir = tmp.path().to_path_buf();

        let mut runtime = Runtime::open(&w3_dir(), Some(&dir)).expect("open");
        let syllables: Vec<SyllableKey> = ["ni", "hao"]
            .iter()
            .map(|s| SyllableKey::from_text(s).expect("fixture key"))
            .collect();
        let keys: Vec<PinyinKey> = syllables
            .iter()
            .map(|key| {
                PinyinKey::try_from(key.index()).expect("frozen syllable inventory fits u16")
            })
            .collect();

        // Two scalars for two keys; the pre-add probe proves this phrase
        // arrives from the user layer, not the fixture tables.
        let phrase = "你鎄";
        let merged = |runtime: &Runtime| runtime.dict().lookup(&syllables).expect("merged lookup");
        assert!(
            !merged(&runtime).iter().any(|entry| entry.text() == phrase),
            "fixture already carries the probe phrase"
        );

        let token = runtime
            .user_store_mut()
            .add_phrase(&oxpinyin_core::PhraseText::from(phrase), &keys, None)
            .expect("learn it");

        // Dictionary seam: the merged lookup returns the learned phrase.
        let entries = merged(&runtime);
        assert!(
            entries.iter().any(|entry| entry.text() == phrase),
            "learned phrase missing from merged lookup: {:?}",
            entries
                .iter()
                .map(|e| e.text().to_owned())
                .collect::<Vec<_>>()
        );

        // Language-model seam: user counts must reach the overlay — the
        // merged answer and the LM total must move by exactly the store's
        // own delta.
        let store_before = runtime.user_store_mut().unigram_total();
        // Capture the overlay total *before* the observation so the branch
        // below measures the delta the observation causes.
        let lm_before = LanguageModel::unigram_total(&runtime.lm()).expect("total query");
        runtime
            .user_store_mut()
            .observe_selection(SENTENCE_START, FIRST_USER_TOKEN)
            .expect("observe a selection");
        let store_after = runtime.user_store_mut().unigram_total();
        assert!(
            store_after > store_before,
            "observing must grow the store's unigram total: {store_before} -> {store_after}"
        );

        let freq = LanguageModel::unigram_freq(&runtime.lm(), &PhraseToken::new(token))
            .expect("overlay query");
        assert!(freq.is_some(), "overlay must answer with real unigrams");
        let lm_before = lm_before.expect("real unigrams before observation");
        let lm_after = LanguageModel::unigram_total(&runtime.lm())
            .expect("total query")
            .expect("real unigrams");
        assert_eq!(
            lm_after - lm_before,
            store_after - store_before,
            "LM total must absorb exactly the store's delta"
        );
    }

    /// A regular file as the user dir cannot hold `user.conf`: the pin's
    /// `check_format` writes its `write %s failed.` line, ignores the save's
    /// answer (`pinyin.cpp:187`; maintainer ruling 2026-10-28 UTC, D4) and the
    /// context keeps its user state, which no save can write.
    #[test]
    fn regular_file_as_user_dir_keeps_user_state() {
        let mut file = tempfile::Builder::new()
            .prefix("oxpinyin-runtime-file-user-")
            .suffix(".tmp")
            .tempfile()
            .expect("temp file");
        std::io::Write::write_all(&mut file, b"not a directory").expect("temp file");
        // Only the path is kept: no handle on the file stays open across the
        // open below.
        let marker = file.into_temp_path();
        let runtime = Runtime::open(&w3_dir(), Some(&*marker));
        // A non-directory user path must not fail init (capi contract).
        if let Err(error) = &runtime {
            panic!("expected open to succeed over a file user dir: {error}");
        }
        let mut runtime = runtime.expect("open");
        assert!(runtime.user_store_mut().has_user_directory());
        drop(runtime);
        marker.close().expect("cleanup temp file");
    }

    #[test]
    fn no_user_path_means_transient_user_state() {
        // capi contract: a NULL user dir disables learning (the pin's NULL
        // `m_user_dir`). The empty path is a different argument — the
        // working directory, as the pin's `""` is — and writes there; its
        // test is `empty_user_path_is_the_working_directory_at_each_use`.
        let mut runtime = Runtime::open(&w3_dir(), None).expect("open");
        assert!(!runtime.user_store_mut().has_user_directory());
    }

    #[test]
    fn addon_unigram_totals_stay_none_until_a_library_loads() {
        let mut runtime = Runtime::open(&w3_dir(), None).expect("open");
        assert_eq!(
            LanguageModel::addon_unigram_total(&runtime.lm()).expect("infallible query"),
            None,
            "no addon library is loaded"
        );
        // The committed w3 carries the addon DBM pair and art.bin; loading is
        // idempotent.
        let index = 4_u8;
        assert!(matches!(
            runtime.load_system_addon_reporting(index),
            Ok(true)
        ));
        assert!(matches!(
            runtime.load_system_addon_reporting(index),
            Ok(false)
        ));
        assert!(
            LanguageModel::addon_unigram_total(&runtime.lm())
                .expect("infallible query")
                .is_some(),
            "the loaded addon library owns items"
        );
    }

    /// U2: absent sub-indices return before the facade total moves, while a
    /// missing slot in a loaded sub-index still moves it (`phrase_index.h:628-635`
    /// and `phrase_index.cpp:150-161` at 074a2219).
    #[test]
    fn unigram_add_distinguishes_a_missing_sub_index_from_a_missing_slot() {
        let mut runtime = Runtime::open(&w3_dir(), None).unwrap();
        let before = runtime.lm().amplified_total();
        assert!(!runtime.add_unigram_delta(0x0800_0001, 9));
        assert_eq!(runtime.lm().amplified_total(), before);
        assert!(!runtime.add_unigram_delta(FIRST_USER_TOKEN, 9));
        assert_eq!(runtime.lm().amplified_total(), before + 9);
    }

    /// U3: the exported add consults only the default phrase index
    /// (`pinyin.cpp:2836-2843`, `phrase_index.h:628-635` at 074a2219).
    #[test]
    fn unigram_add_rejects_a_token_found_only_in_the_addon_index() {
        use oxpinyin_data::phrase_library::PhraseLibrary;

        let system = scratch_dir("oxpinyin-addon-only-");
        for entry in std::fs::read_dir(w3_dir()).unwrap() {
            let entry = entry.unwrap();
            std::fs::copy(entry.path(), system.path().join(entry.file_name())).unwrap();
        }
        let conf = system.path().join("table.conf");
        let stock = std::fs::read_to_string(&conf).unwrap();
        let edited = stock.replace(
            "default MERGED_DICTIONARY merged.table merged.bin merged.dbin SYSTEM_FILE",
            "default MERGED_DICTIONARY NULL NULL NULL NOT_USED",
        );
        assert_ne!(stock, edited);
        std::fs::write(conf, edited).unwrap();
        let addon = PhraseLibrary::open(&system.path().join("art.bin")).unwrap();
        let token = 0x0400_0000 | addon.items().next().unwrap().0;
        let mut runtime = Runtime::open(system.path(), None).unwrap();
        assert!(matches!(runtime.load_system_addon_reporting(4), Ok(true)));
        assert!(
            runtime
                .lm()
                .addon_unigram_freq(&PhraseToken::new(token))
                .unwrap()
                .unwrap()
                > 0
        );
        assert!(!runtime.dict().token_exists(token));
        let before = runtime.lm().amplified_total();
        assert!(!runtime.add_unigram_delta(token, 9));
        assert_eq!(runtime.lm().amplified_total(), before);
    }

    #[test]
    fn rejected_unigram_add_keeps_item_and_library_counts_but_moves_facade_total() {
        let mut runtime = Runtime::open(&w3_dir(), None).expect("open");
        let before = runtime.lm().amplified_total() as u32;
        let read = runtime.dict();
        let library = read
            .system()
            .libraries()
            .library(0x0100_0000)
            .expect("library 1");
        let tokens: Vec<_> = library
            .token_range()
            .map(|local| 0x0100_0000 | local)
            .filter(|&token| {
                runtime
                    .dict()
                    .live_items()
                    .read(token, |_| ())
                    .found()
                    .is_some()
            })
            .take(2)
            .collect();
        assert_eq!(tokens.len(), 2);
        let remaining = u32::MAX - library.total_freq();
        let live = |runtime: &Runtime, token: u32| {
            runtime
                .user_store()
                .system_unigram(token)
                .expect("live item")
        };
        let shipped = [live(&runtime, tokens[0]), live(&runtime, tokens[1])];
        assert!(runtime.add_unigram_delta(tokens[0], u64::from(remaining)));
        assert_eq!(
            live(&runtime, tokens[0]),
            shipped[0].wrapping_add(remaining)
        );
        assert!(!runtime.add_unigram_delta(tokens[1], 1));
        assert_eq!(live(&runtime, tokens[1]), shipped[1]);
        assert!(!runtime.add_unigram_delta(tokens[0], 1));
        assert_eq!(
            live(&runtime, tokens[0]),
            shipped[0].wrapping_add(remaining)
        );
        assert_eq!(
            (runtime.lm().amplified_total() as u32).wrapping_sub(before),
            remaining + 2
        );
        assert!(runtime.add_unigram_delta(tokens[1], 0));
    }

    #[test]
    fn fresh_nbest_step_observes_training_store_writes_and_mask_out() {
        use oxpinyin_core::NbestStepRow;
        let tmp = scratch_dir("oxpinyin-nbest-step-");
        let dir = tmp.path().to_path_buf();
        let mut runtime = Runtime::open(&w3_dir(), Some(&dir)).unwrap();
        let keys: Vec<_> = ["ni", "hao"]
            .into_iter()
            .map(|text| SyllableKey::from_text(text).unwrap().index() as PinyinKey)
            .collect();
        let token = PhraseToken::new(
            runtime
                .user_store_mut()
                .add_phrase(&"你鎄".into(), &keys, Some(5))
                .unwrap(),
        );
        // A second successor keeps count/total from remaining exactly 1 when
        // the tested count changes, so a stale row cannot pass by normalization.
        let other = runtime
            .user_store_mut()
            .add_phrase(&"你鎬".into(), &keys, Some(5))
            .unwrap();
        runtime
            .user_store_mut()
            .set_bigram_count(SENTENCE_START, other, 1000)
            .unwrap();
        let prev = PhraseToken::new(SENTENCE_START);
        let fresh = |runtime: &Runtime| {
            let actual = runtime
                .lm()
                .nbest_step_costs_in_row(&prev, &token, &mut NbestStepRow::default())
                .unwrap();
            assert_eq!(
                actual,
                runtime.lm().nbest_step_costs(&prev, &token).unwrap()
            );
            actual
        };
        let before = fresh(&runtime);
        runtime
            .user_store_mut()
            .observe_selection(SENTENCE_START, token.value())
            .unwrap();
        let trained = fresh(&runtime);
        assert_ne!(trained, before);
        runtime
            .user_store_mut()
            .set_bigram_count(SENTENCE_START, token.value(), 1000)
            .unwrap();
        let written = fresh(&runtime);
        assert_ne!(written, trained);
        runtime
            .user_store_mut()
            .mask_out(u32::MAX, token.value())
            .unwrap();
        let masked = fresh(&runtime);
        assert_ne!(masked, written);
        drop(runtime);
        tmp.close().unwrap();
    }

    #[test]
    fn a_predecessor_missing_from_the_system_table_merges_the_user_gram_alone() {
        use oxpinyin_core::LanguageModel;
        let tmp = scratch_dir("oxpinyin-user-gram-alone-");
        let dir = tmp.path().to_path_buf();
        let mut runtime = Runtime::open(&w3_dir(), Some(&dir)).unwrap();
        let keys: Vec<_> = ["ni", "hao"]
            .into_iter()
            .map(|text| SyllableKey::from_text(text).unwrap().index() as PinyinKey)
            .collect();
        let user = runtime.user_store_mut();
        let prev = user.add_phrase(&"你鎄".into(), &keys, Some(5)).unwrap();
        let next = user.add_phrase(&"你鎬".into(), &keys, Some(5)).unwrap();
        user.set_bigram_count(prev, next, 7).unwrap();
        // A user phrase has no row in the system table: `Bigram::load` leaves
        // the system gram NULL, and `merge_single_gram` takes the user's alone
        // (`ngram.cpp:286-290` at 074a2219).
        let stored = user.bigram_row(prev).unwrap().expect("the user gram");
        let merged = runtime
            .lm()
            .merged_successors(&PhraseToken::new(prev))
            .unwrap()
            .expect("the user gram stands alone");
        assert_eq!(merged.to_bytes(), stored);
        assert_eq!(merged.get_freq(next), Ok(Some(7)));
        drop(runtime);
        tmp.close().unwrap();
    }

    #[test]
    fn a_fresh_nbest_row_observes_a_reborrowed_stores_committed_write() {
        use oxpinyin_core::{NbestStepRow, NbestSuccessors};
        let tmp = scratch_dir("oxpinyin-nbest-snapshot-");
        let dir = tmp.path().to_path_buf();
        let mut runtime = Runtime::open(&w3_dir(), Some(&dir)).unwrap();
        let keys: Vec<_> = ["ni", "hao"]
            .into_iter()
            .map(|text| SyllableKey::from_text(text).unwrap().index() as PinyinKey)
            .collect();
        let token = PhraseToken::new(
            runtime
                .user_store_mut()
                .add_phrase(&"你鎄".into(), &keys, Some(5))
                .unwrap(),
        );
        let other = runtime
            .user_store_mut()
            .add_phrase(&"你鎬".into(), &keys, Some(5))
            .unwrap();
        runtime
            .user_store_mut()
            .set_bigram_count(SENTENCE_START, other, 1000)
            .unwrap();
        let prev = PhraseToken::new(SENTENCE_START);
        let mut row = NbestStepRow::default();
        // One span load's merged row: whether it carries `successor`, and the
        // item costs of `token` read from it.
        let carries = |runtime: &Runtime, row: &mut NbestStepRow, successor: u32| match runtime
            .lm()
            .nbest_successors_in_row(&prev, row)
            .unwrap()
            .unwrap()
        {
            NbestSuccessors::Row(gram) => {
                let mut items = Vec::new();
                gram.search_into(successor, successor, &mut items).unwrap();
                !items.is_empty()
            }
            NbestSuccessors::Miss => false,
        };
        let costs = |runtime: &Runtime, row: &mut NbestStepRow| {
            let item = match runtime
                .lm()
                .nbest_successors_in_row(&prev, row)
                .unwrap()
                .unwrap()
            {
                NbestSuccessors::Row(gram) => gram
                    .get_freq(token.value())
                    .unwrap()
                    .map(|freq| (freq, gram.total().unwrap())),
                NbestSuccessors::Miss => None,
            };
            runtime
                .lm()
                .nbest_item_costs_in_row(&prev, &token, item, row)
                .unwrap()
        };
        let before = costs(&runtime, &mut row);
        assert_eq!(
            before,
            runtime.lm().nbest_step_costs(&prev, &token).unwrap()
        );
        assert!(carries(&runtime, &mut row, other));
        // A row is one load: the next load, after an exclusive write, reads
        // the grams again. No dictionary/model reader is retained while the
        // owner changes.
        runtime
            .user_store_mut()
            .observe_selection(SENTENCE_START, token.value())
            .unwrap();
        let trained = runtime.lm().nbest_step_costs(&prev, &token).unwrap();
        assert_ne!(trained, before);
        let mut row = NbestStepRow::default();
        assert!(carries(&runtime, &mut row, token.value()));
        assert_eq!(costs(&runtime, &mut row), trained);
        runtime.user_store_mut().mask_out(u32::MAX, other).unwrap();
        let forgotten = runtime.lm().nbest_step_costs(&prev, &token).unwrap();
        assert_ne!(forgotten, trained);
        let mut row = NbestStepRow::default();
        assert!(!carries(&runtime, &mut row, other));
        assert_eq!(costs(&runtime, &mut row), forgotten);
        drop(runtime);
        tmp.close().unwrap();
    }

    /// The fused `trellis_ranges_continued` answers exactly as the two calls
    /// it replaces: ranges and probe, over system rows, an empty-marker or
    /// absent key, a user-table-only prefix, toned and incomplete spellings,
    /// and a live library mask.
    #[test]
    fn trellis_ranges_continued_matches_the_ranges_and_prefix_pair() {
        let tmp = scratch_dir("oxpinyin-trellis-fused-");
        let dir = tmp.path().to_path_buf();
        let mut runtime = Runtime::open(&w3_dir(), Some(&dir)).expect("runtime");
        let texts: [&[&str]; 9] = [
            &["ni"],
            &["ni", "hao"],
            &["hao"],
            &["n"],
            &["n", "h"],
            &["ni", "hao", "ma"],
            &["zuo", "zuo", "zuo"],
            &["zuo", "zuo"],
            &["xyz"],
        ];
        let tones: [&[u8]; 5] = [&[], &[3], &[3, 3], &[1, 3], &[0, 3, 0]];
        let paths: Vec<Vec<SyllableKey>> = texts
            .iter()
            .filter_map(|path| {
                path.iter()
                    .map(|text| SyllableKey::from_text(text))
                    .collect::<Option<Vec<_>>>()
            })
            .collect();
        assert!(paths.len() >= 8, "the key paths parse: {}", paths.len());
        let seen = std::cell::Cell::new((0_u32, 0_u32, 0_u32));
        let compare = |runtime: &Runtime, phase: &str| {
            // One value buffer for every fused search, as the decode loop
            // passes it.
            let mut scratch = Vec::new();
            for path in &paths {
                for tones in tones {
                    let mut pair = oxpinyin_core::PhraseIndexRanges::new();
                    runtime
                        .dict()
                        .trellis_ranges(path, tones, &mut pair)
                        .unwrap();
                    let continued = runtime.dict().phrase_prefix_exists(path).unwrap();
                    let mut fused = oxpinyin_core::PhraseIndexRanges::new();
                    let fused_continued = runtime
                        .dict()
                        .trellis_ranges_continued(path, tones, &mut fused, &mut scratch)
                        .unwrap();
                    assert_eq!(
                        format!("{pair:?}"),
                        format!("{fused:?}"),
                        "{phase}: ranges of {path:?} {tones:?}"
                    );
                    assert_eq!(
                        continued, fused_continued,
                        "{phase}: probe of {path:?} {tones:?}"
                    );
                    let (ranged, yes, no) = seen.get();
                    seen.set((
                        ranged + u32::from(fused.iter().next().is_some()),
                        yes + u32::from(fused_continued),
                        no + u32::from(!fused_continued),
                    ));
                }
            }
        };
        compare(&runtime, "system only");
        let user = |texts: &[&str]| -> Vec<_> {
            texts
                .iter()
                .map(|text| {
                    oxpinyin_user::toned_key(SyllableKey::from_text(text).unwrap().index(), 3)
                        .unwrap()
                })
                .collect()
        };
        runtime
            .user_store_mut()
            .add_phrase(&"你鎄".into(), &user(&["ni", "hao"]), Some(5))
            .unwrap();
        runtime
            .user_store_mut()
            .add_phrase(&"坐坐坐".into(), &user(&["zuo", "zuo", "zuo"]), Some(5))
            .unwrap();
        compare(&runtime, "with user phrases");
        assert!(runtime.unload_library(2));
        compare(&runtime, "library 2 unloaded");
        let (ranged, yes, no) = seen.get();
        assert!(ranged > 0 && yes > 0 && no > 0, "{ranged} {yes} {no}");
        drop(runtime);
        tmp.close().expect("cleanup");
    }

    // The n-best keeps what it reads of a key path's user lookup only for the
    // span search that found the path. A write to the store between two
    // guesses on one session must reach the second guess, both ways.
    #[test]
    fn a_store_write_between_guesses_reaches_the_second_guess() {
        let tmp = scratch_dir("oxpinyin-guess-memo-");
        let dir = tmp.path().to_path_buf();
        let mut runtime = Runtime::open(&w3_dir(), Some(&dir)).unwrap();
        let mut session = runtime.new_session(SessionSettings::default());
        runtime.session(&mut session).replace_raw("nihao").unwrap();
        let mut guess = |runtime: &Runtime| {
            runtime.session(&mut session).guess_sentence().unwrap();
            runtime
                .session(&mut session)
                .sentence_text(0)
                .unwrap()
                .to_owned()
        };
        assert_eq!(guess(&runtime), "你好");

        let keys: Vec<_> = ["ni", "hao"]
            .into_iter()
            .map(|text| SyllableKey::from_text(text).unwrap().index() as PinyinKey)
            .collect();
        let token = runtime
            .user_store_mut()
            .add_phrase(&"你鎄".into(), &keys, Some(5))
            .unwrap();
        runtime
            .user_store_mut()
            .set_bigram_count(SENTENCE_START, token, 1000)
            .unwrap();
        assert_eq!(
            guess(&runtime),
            "你鎄",
            "the second guess prices the added phrase"
        );

        runtime.user_store_mut().mask_out(u32::MAX, token).unwrap();
        assert_eq!(
            guess(&runtime),
            "你好",
            "the third guess no longer lists it"
        );
        drop(session);
        drop(runtime);
        tmp.close().unwrap();
    }

    // Pricing a path's tokens through one shared memo answers exactly what
    // `trellis_item` answers for each, the memo being filled by the first.
    #[test]
    fn a_path_memo_prices_as_trellis_item_does() {
        let runtime = Runtime::open(&w3_dir(), None).unwrap();
        let syllables = [
            SyllableKey::from_text("ni").unwrap(),
            SyllableKey::from_text("hao").unwrap(),
        ];
        let tones = [0, 0];
        let mut ranges = oxpinyin_core::PhraseIndexRanges::new();
        runtime
            .dict()
            .trellis_ranges(&syllables, &tones, &mut ranges)
            .unwrap();
        let tokens: Vec<u32> = ranges.walk_tokens().map(|(token, ())| token).collect();
        assert!(!tokens.is_empty(), "vacuity guard: nihao ranges tokens");

        let own = <crate::RuntimeDict as Dictionary>::PathMemo::default();
        for token in tokens.iter().copied().chain([0, u32::MAX]) {
            let expected = runtime
                .dict()
                .trellis_item(&syllables, &tones, token)
                .unwrap();
            assert_eq!(
                runtime
                    .dict()
                    .trellis_item_on_path(&syllables, &tones, token, &own)
                    .unwrap(),
                expected,
                "own memo, token {token:#x}"
            );
        }
        assert!(
            tokens.iter().any(|&token| {
                runtime
                    .dict()
                    .trellis_item(&syllables, &tones, token)
                    .unwrap()
                    .is_some()
            }),
            "vacuity guard: some token prices"
        );
    }

    /// Register row 79: the init loop maps the default libraries in index order
    /// and writes `mmap %s failed!` for each absent one (`pinyin.cpp:253-256`)
    /// before it reaches a `DICTIONARY` row and asserts (`:388`), so the open
    /// reports the libraries before that row, and only those.
    #[test]
    fn a_default_dictionary_row_reports_only_the_libraries_before_it() {
        let tmp = scratch_dir("oxpinyin-runtime-row-79-");
        let dir = tmp.path().to_path_buf();
        let conf = std::fs::read_to_string(w3_dir().join("table.conf")).expect("table.conf");
        let conf = conf.replace(
            "default OPENGRAM_DICTIONARY opengram.table opengram.bin opengram.dbin SYSTEM_FILE",
            "default OPENGRAM_DICTIONARY opengram.table opengram.bin NULL DICTIONARY",
        );
        std::fs::write(dir.join("table.conf"), conf).expect("write table.conf");
        let mut reported = Vec::new();
        let opened = Runtime::open_with_law_reporting(
            &dir,
            None,
            oxpinyin_user::UserConfLaw::Pinyin,
            &mut |path| reported.push(path.to_path_buf()),
        );
        assert!(matches!(
            opened,
            Err(OpenError::TableConf(TableConfError::Abort(
                crate::TableConfSite::DefaultDictionary
            )))
        ));
        assert_eq!(
            reported,
            [dir.join("gb_char.bin"), dir.join("gbk_char.bin")]
        );
    }

    /// Rows 91 and 108: the init loop maps library `i`, then merges its log and
    /// dies there (`pinyin.cpp:253-256`, `:276`), so the open reports the absent
    /// libraries through `i` and none after it.
    #[test]
    fn a_journal_fault_reports_only_the_libraries_through_its_own() {
        let system_tmp = scratch_dir("oxpinyin-runtime-journal-system-");
        let system = system_tmp.path().to_path_buf();
        for entry in std::fs::read_dir(w3_dir()).expect("fixture") {
            let entry = entry.expect("entry");
            let name = entry.file_name();
            if name != "gbk_char.bin" && name != "merged.bin" {
                std::fs::copy(entry.path(), system.join(&name)).expect("copy");
            }
        }
        let user_tmp = scratch_dir("oxpinyin-runtime-journal-user-");
        let user = user_tmp.path().to_path_buf();
        let conf = oxpinyin_data::table_info::TableConf::load_from_dir(&system).expect("conf");
        let versions = oxpinyin_data::user_files::SystemVersions::from_conf(&conf);
        std::fs::write(
            user.join("user.conf"),
            oxpinyin_data::user_files::UserTableInfo::conform_to(&versions).to_text(),
        )
        .expect("user.conf");
        // A MODIFY_HEADER whose token is not `null_token` (`phrase_index_logger.h:202`).
        let mut header = 4_u32.to_le_bytes().to_vec();
        header.extend_from_slice(&0x0300_0001_u32.to_le_bytes());
        header.extend_from_slice(&4_u16.to_le_bytes());
        header.extend_from_slice(&[0; 8]);
        std::fs::write(
            user.join("opengram.dbin"),
            oxpinyin_data::chunk_format::build_memory_chunk(&header).expect("frame"),
        )
        .expect("opengram.dbin");
        let mut reported = Vec::new();
        let opened = Runtime::open_with_law_reporting(
            &system,
            Some(&user),
            oxpinyin_user::UserConfLaw::Pinyin,
            &mut |path| reported.push(path.to_path_buf()),
        );
        assert!(matches!(opened, Err(OpenError::Journal(fault)) if fault.library() == 3));
        assert_eq!(reported, [system.join("gbk_char.bin")]);
    }

    // The empty user path is the working directory, as the pin's `""` is.
    //
    // `pinyin_init` keeps `g_strdup(userdir)` and builds every user file's
    // path with `g_build_filename`, which drops an empty element
    // (`pinyin.cpp:332`, `:176-177`, `:220-232` at 074a2219): the profile of
    // a `""` user dir is bare file names, resolved against the directory
    // that is current when each file is opened, not the one current at init
    // (#619). This test holds that law where no oracle is built.
    //
    // The working directory is process state. nextest runs each test in a
    // process of its own, so no other test sees the directory move, and no
    // other test of this crate reads the working directory.
    /// A fresh directory, removed when it drops.
    fn scratch(tag: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(&format!("oxpinyin-runtime-empty-user-dir-{tag}-"))
            .tempdir()
            .expect("scratch dir")
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .expect("readable dir")
            .map(|entry| {
                entry
                    .expect("dir entry")
                    .file_name()
                    .into_string()
                    .expect("UTF-8 file name")
            })
            .collect();
        names.sort();
        names
    }

    /// `user.conf`'s `open counter:` value in `dir`.
    fn open_counter(dir: &Path) -> i32 {
        let text = std::fs::read_to_string(dir.join("user.conf")).expect("user.conf");
        text.lines()
            .find_map(|line| line.strip_prefix("open counter:"))
            .expect("an open counter line")
            .parse()
            .expect("a decimal counter")
    }

    #[test]
    fn empty_user_path_is_the_working_directory_at_each_use() {
        let first_dir = scratch("first");
        let second_dir = scratch("second");
        let first = first_dir.path().to_path_buf();
        let second = second_dir.path().to_path_buf();
        std::env::set_current_dir(&first).expect("enter the first directory");

        // Init: the store opens on the working directory, and libpinyin's
        // check_format writes the raised marker there.
        let mut runtime = Runtime::open(&w3_dir(), Some(Path::new(""))).expect("open");
        let store = runtime.user_store_mut();
        assert_eq!(names(&first), ["user.conf"]);
        assert_eq!(open_counter(&first), 1);

        // The consumer moves. A dirty save lands where it is now, marker
        // included, and the directory init wrote into is not touched again.
        std::env::set_current_dir(&second).expect("enter the second directory");
        let keys: Vec<PinyinKey> = ["ni", "hao"]
            .iter()
            .map(|s| SyllableKey::from_text(s).expect("fixture key"))
            .map(|key| {
                PinyinKey::try_from(key.index()).expect("frozen syllable inventory fits u16")
            })
            .collect();
        let phrase = "你鎄";
        store
            .add_phrase(&oxpinyin_core::PhraseText::from(phrase), &keys, None)
            .expect("learn it");
        store.mark_modified();
        store.save_reporting().expect("save");
        let saved = names(&second);
        for name in ["user.conf", "user.bin", "user_bigram.db"] {
            assert!(
                saved.iter().any(|n| n == name),
                "{name} missing from {saved:?}"
            );
        }
        assert_eq!(open_counter(&second), 1);
        assert_eq!(names(&first), ["user.conf"]);

        // Fini is the runtime's own handle dropping: it lowers the counter in
        // the directory that is current now. The first directory keeps the
        // raised one, as the pin's does.
        drop(runtime);
        assert_eq!(open_counter(&second), 0);
        assert_eq!(open_counter(&first), 1);

        // The profile reads back from the working directory. A file beside
        // one of its DBM files is not the profile's, so the reopen leaves it:
        // the pin's init unlinks only the profile's own files, and only when
        // `check_format` finds an old format (`pinyin.cpp:200-215`).
        let strays = ["user_bigram.db-lock", "user_bigram.db-shm"];
        for stray in strays {
            std::fs::write(stray, b"stale").expect("plant a stray file");
        }
        let mut runtime = Runtime::open(&w3_dir(), Some(Path::new(""))).expect("reopen");
        let store = runtime.user_store_mut();
        assert!(
            store
                .token_for_phrase(phrase)
                .expect("phrase lookup")
                .is_some(),
            "the saved phrase is read back"
        );
        let mut expected = saved.clone();
        expected.extend(strays.map(String::from));
        expected.sort();
        assert_eq!(
            names(&second),
            expected,
            "the saved names and the stray files remain"
        );
        drop(runtime);

        std::env::set_current_dir(std::env::temp_dir()).expect("leave the scratch dirs");
    }
}

#[cfg(feature = "tkrzw")]
mod pin_data {
    //! The drop-in invariant at the Rust runtime seam: an **unmodified
    //! libpinyin data directory** opens through `Runtime::open` and decodes.
    //!
    //! The directory is the pinned libpinyin-data package
    //! (`just fetch-contract-data`), in tkrzw format, so this module requires
    //! that backend. Nothing here is converted, imported or copied: the directory
    //! is the test input.
    //!
    //! The C-ABI half of the invariant is the facade crates' pinned-data tests
    //! over the same package ([contract references](../../../docs/runbooks/contract-references.md)).

    use super::contract_data;

    use oxpinyin_core::{Dictionary, LanguageModel, PhraseToken, SyllableKey};

    use oxpinyin_engine::SessionSettings;

    use crate::Runtime;

    fn syllables(text: &str) -> Vec<SyllableKey> {
        text.split('\'')
            .map(|s| SyllableKey::from_text(s).expect("frozen syllable"))
            .collect()
    }

    #[test]
    fn an_unmodified_libpinyin_data_directory_opens_and_decodes() {
        let dir = contract_data::contract_data_dir();
        // The directory is libpinyin's own: its table.conf names the DBM the
        // install was built with, which must be this build's, and every file
        // keeps libpinyin's name.
        let conf = std::fs::read_to_string(dir.join("table.conf")).expect("table.conf");
        let format = format!("database format:{}", oxpinyin_data::DEFAULT_STORE_DB_FORMAT);
        assert!(
            conf.contains(&format),
            "not a libpinyin data dir for this build's backend ({format}): {conf}"
        );
        for name in [
            "pinyin_index.bin",
            "phrase_index.bin",
            "bigram.db",
            "punct.bin",
            "gb_char.bin",
            "addon_pinyin_index.bin",
            "art.bin",
        ] {
            assert!(dir.join(name).is_file(), "{name} missing");
        }

        let mut runtime = Runtime::open(&dir, None).expect("libpinyin's data dir opens as is");
        let dict = runtime.dict();
        let lm = runtime.lm();

        // The facade total of the model libpinyin 074a2219 builds: real corpus
        // counts from the chunk items.
        let total = LanguageModel::unigram_total(&lm)
            .expect("query")
            .expect("real unigrams");
        assert_eq!(
            total, 51_051_831,
            "Σ item unigram of the pinned model: 50,913,735 \\1-gram counts + one per item"
        );

        // Dictionary lookups through the pinyin DBM + chunk files.
        let nihao = dict.lookup(&syllables("ni'hao")).expect("lookup");
        assert!(
            nihao.iter().any(|entry| entry.text() == "你好"),
            "{nihao:?}"
        );
        let entry = nihao
            .iter()
            .find(|entry| entry.text() == "你好")
            .expect("你好");
        let item = dict
            .trellis_item(&syllables("ni'hao"), &[], entry.token().value())
            .expect("trellis item")
            .expect("你好 is ranged on ni'hao");
        assert!(
            item.pronunciation.is_some_and(|(m, t)| m > 0 && m <= t),
            "possibility from the chunk pronunciations"
        );
        assert!(
            dict.phrase_prefix_exists(&syllables("zhong"))
                .expect("probe")
        );
        // A valid syllable no phrase in the fixture is keyed on: an empty
        // lookup, no panic.
        let lone = SyllableKey::from_text("beng").expect("frozen syllable");
        let _ = dict.lookup(&[lone]).expect("lookup");

        // Phrase DBM: text → tokens, and token → text back through the chunks.
        let tokens = dict.tokens_for_text("中国");
        assert!(!tokens.is_empty());
        for token in &tokens {
            assert_eq!(
                dict.phrase_text_for_token(token.value())
                    .as_ref()
                    .and_then(|text| text.as_str()),
                Some("中国")
            );
            assert!(
                LanguageModel::unigram_freq(&lm, token)
                    .expect("query")
                    .is_some(),
                "every chunk item carries a count"
            );
        }

        // Prediction through the phrase DBM's raw suggestion walk; the text
        // comes from the live item, as the facade's prediction path resolves it.
        let suggestions = dict
            .system()
            .suggestion_tokens(&[u32::from('中')])
            .expect("suggest");
        assert!(
            suggestions.iter().any(|&token| {
                dict.phrase_text_for_token(token)
                    .is_some_and(|text| text == "中国")
            }),
            "中 suggests 中国: {} rows",
            suggestions.len()
        );

        // Punctuation through punct.bin.
        let de = dict.tokens_for_text("的");
        assert!(
            de.iter()
                .any(|token| !dict.punctuations(token.value()).is_empty())
        );

        // The addon facade: addon_pinyin_index.bin + art.bin on demand.
        assert!(
            matches!(runtime.load_system_addon_reporting(4), Ok(true)),
            "art loads from the same dir"
        );
        assert!(
            matches!(runtime.load_system_addon_reporting(4), Ok(false)),
            "second load is false"
        );
        let dict = runtime.dict();
        let art = dict
            .lookup_addon(&syllables("er'huang"))
            .expect("addon lookup");
        assert!(art.iter().any(|entry| entry.text() == "二簧"), "{art:?}");

        // A session decodes over the same handles.
        let mut session = runtime.new_session(SessionSettings::default());
        let mut session = runtime.session(&mut session);
        session.replace_raw("nihao").expect("typing");
        let first = session
            .candidates()
            .iter()
            .next()
            .expect("candidates")
            .text()
            .to_owned();
        assert_eq!(first, "你好");
        session.guess_sentence().expect("sentence");
        assert_eq!(session.sentence_text(0).expect("row 0"), "你好");
        let _ = PhraseToken::new(0);
    }
}

#[cfg(feature = "tkrzw")]
mod candidate_surface {
    //! oxpinyin's candidate lists against the frozen pin capture, over every input
    //! the capture holds.
    //!
    //! `fixtures/w4/oracle-candidate-structure.txt` holds the pin's first ten candidates
    //! for each retained input. The system data is the pinned libpinyin-data package,
    //! which `just fetch-contract-data` extracts and which is in tkrzw format, so this
    //! target requires that backend. A missing package is a failure, not a skip.

    use super::{candidate_fixture, contract_data};

    use std::path::PathBuf;

    use oxpinyin_engine::{Candidate, CandidateKind, Session, SessionSettings};

    use crate::Runtime;

    use candidate_fixture::{CandidateFixture, OracleCandidateType, STRUCTURE_FIXTURE};

    /// How many candidates the capture records per input.
    const CAPTURE_DEPTH: usize = 10;

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn candidate_type(candidate: &Candidate) -> Option<OracleCandidateType> {
        match candidate.kind() {
            CandidateKind::Phrase => Some(OracleCandidateType::Normal),
            CandidateKind::Addon => Some(OracleCandidateType::Addon),
            CandidateKind::Sentence => Some(OracleCandidateType::NbestMatch),
            CandidateKind::Fallback => None,
        }
    }

    #[test]
    fn candidates_match_the_frozen_capture() {
        let fixture = CandidateFixture::read(repo_root().join(STRUCTURE_FIXTURE))
            .expect("the structure capture is committed");
        let mut expected = std::collections::BTreeMap::<&str, Vec<_>>::new();
        for row in &fixture.rows {
            expected.entry(row.input.as_str()).or_default().push((
                row.text.as_str(),
                Some(row.candidate_type),
                row.nbest_index,
            ));
        }

        let dir = contract_data::contract_data_dir();
        let runtime = Runtime::open(&dir, None).expect("the pinned data directory opens");
        let mut session = Session::new(SessionSettings::default(), runtime.dict(), runtime.lm());

        let mut rows = 0_usize;
        let mut compared = 0_usize;
        let mut moved = Vec::new();
        for (input, theirs) in &expected {
            session.reset();
            let _ = session.replace_raw(input);
            let ours: Vec<_> = session
                .candidates()
                .iter()
                .take(CAPTURE_DEPTH)
                .map(|candidate| {
                    (
                        candidate.text().as_str().unwrap_or_default().to_owned(),
                        candidate_type(candidate),
                        candidate.nbest_row(),
                    )
                })
                .collect();
            rows += theirs.len();
            compared += 1;
            let same = ours.len() == theirs.len()
                && ours
                    .iter()
                    .zip(theirs)
                    .all(|(ours, theirs)| (ours.0.as_str(), ours.1, ours.2) == *theirs);
            if !same {
                moved.push(format!("{input:?}: ours {ours:?}, pin {theirs:?}"));
            }
        }

        eprintln!(
            "candidate surface: {compared} captured inputs compared, {rows} rows, {} moved",
            moved.len()
        );
        for line in moved.iter().take(20) {
            eprintln!("{line}");
        }
        assert!(
            moved.is_empty(),
            "{} inputs moved from the capture",
            moved.len()
        );
        assert_eq!(compared, expected.len(), "every captured input compared");
        assert_eq!(compared, fixture.total_inputs, "header total_inputs");
        assert_eq!(rows, fixture.total_triples, "header total_triples");
        assert_eq!(compared, 750, "retained captured inputs");
        assert_eq!(rows, 4_572, "retained candidate rows");
    }
}

#[cfg(feature = "tkrzw")]
mod sentence_surface_parity {
    //! The measured sentence-surface gate recorded in
    //! `docs/findings/sentence-surface.md` §12, over the pinned libpinyin-data
    //! package (`just fetch-contract-data`), in tkrzw format. Passing this gate does
    //! not classify the remaining rows or freeze unattributed defects permanently.
    //! Only the current exact selection residuals have the registered S1/S2/S3
    //! disposition.
    //!
    //! Approved repairs update these measured numbers and §12 in the same PR.
    //! A pin move also requires regenerating the oracle fixture and re-measuring
    //! (`docs/runbooks/goldens-and-pins.md`).

    use super::{contract_data, sentence_fixture, sentence_tail};

    use sentence_tail::PortSession;

    /// The port over the pinned package.
    fn open_session() -> PortSession {
        PortSession::open(&contract_data::contract_data_dir()).expect("the package opens")
    }

    /// The §12 measured residual, over the 500 comparable inputs of the frozen
    /// W2 sample. Re-measure and obtain approval before updating these numbers
    /// and §12 together; do not silently change the expected surface.
    #[test]
    fn sentence_surface_matches_the_declared_residual() {
        let mut session = open_session();

        let report = sentence_tail::measure(&mut session, &sentence_fixture::repo_root())
            .expect("the sentence-surface measurement runs");
        eprintln!("sentence-surface measurement: {report:#?}");

        assert!(
            report.unexpected_surfaces.is_empty(),
            "sentence surfaces moved from their exact fixture bindings: {:?}",
            report.unexpected_surfaces
        );
        assert_eq!(report.declared_residuals, 9, "residual membership moved");
        assert_eq!(report.comparable, 500, "comparable-input count drifted");
        assert_eq!(
            report.guessed_disagree, 0,
            "guess_sentence retval must agree on every comparable input"
        );

        // Re-measured after #1378: 1-best 496, distinct-set 492, ordered 491.
        assert_eq!(report.row0_match, 496, "1-best agreement moved (§12: 496)");
        assert_eq!(
            report.distinct_set_match(),
            492,
            "n-best distinct-set agreement moved (§12: 492)"
        );
        assert_eq!(
            report.list_ordered_match, 491,
            "n-best ordered-list agreement moved (§12: 491)"
        );
        assert_eq!(
            report.rows_match, 491,
            "first-6 candidate-row agreement moved (§12: 491, coincides with ordered)"
        );

        // Measured invariant: no residual list is merely reordered.
        // This observation does not attribute its cause.
        assert_eq!(
            report.list_order_only, 0,
            "an order-only sentence divergence appeared; re-measure §12"
        );

        // The 492 − 491 = 1 duplicate-path rank (the distinct-same row).
        assert_eq!(
            report.list_distinct_extra, 1,
            "the distinct-set minus ordered gap moved from 1 (§12)"
        );
    }

    /// Supplementary #747 coverage, reported separately from the frozen 500.
    /// Every ordinary input matches the pin row for row. The current §12
    /// selection residuals match their exact recorded subject surfaces; changes
    /// within those cases fail too. No new exception membership lives here.
    #[test]
    fn sentence_surface_matches_supplementary_tie_order() {
        let cases = sentence_fixture::load_tie_cases(&sentence_fixture::repo_root());
        let mut owner = open_session();
        let mut session = owner.session();
        session
            .set_options(oxpinyin_core::OptionBits::from_bits_retain(0x18a))
            .expect("capture options must apply");
        session
            .set_sort_options(0x1e)
            .expect("capture sort must apply");
        let (mut pin_equal, mut residuals) = (0, 0);
        let mut moved = Vec::new();
        for (ordinal, case) in cases.iter().enumerate() {
            session.reset();
            // The Hanyu C-ABI parse_full_more seam uses replace_raw, preserving
            // literal junk and physical separators exactly as the corpus walk.
            session.replace_raw(&case.input).expect("input must parse");
            assert_eq!(
                session.guess_sentence().expect("guess must run"),
                case.guessed
            );
            let actual = sentence_fixture::TieSurface {
                indexed: (0..3)
                    .filter_map(|rank| {
                        session
                            .sentence_text(rank)
                            .and_then(|text| text.as_str())
                            .map(str::to_owned)
                    })
                    .collect(),
                rows: session
                    .candidates()
                    .iter()
                    .filter(|candidate| {
                        candidate.kind() == oxpinyin_engine::CandidateKind::Sentence
                    })
                    .map(|candidate| {
                        (
                            candidate.nbest_row().expect("NBEST rank"),
                            candidate.text().as_str().unwrap_or_default().to_owned(),
                        )
                    })
                    .collect(),
            };
            let expected = if let Some(residual) = &case.residual {
                residuals += 1;
                residual
            } else {
                pin_equal += 1;
                &case.pin
            };
            if &actual != expected {
                eprintln!(
                    "tie surface moved for {:?}: actual={actual:?}; expected={expected:?}",
                    case.input
                );
                moved.push(case.input.clone());
            }
            if (ordinal + 1) % 1000 == 0 {
                eprintln!("supplementary tie progress: {} inputs checked", ordinal + 1);
            }
        }
        eprintln!(
            "supplementary tie order: pin-equal={pin_equal}, exact residuals={residuals}, total={}",
            cases.len()
        );
        assert!(
            moved.is_empty(),
            "supplementary tie surfaces moved: {moved:?}"
        );
        assert_eq!(residuals, 156, "residual inputs checked count moved");
        assert_eq!(pin_equal, 62, "pin-equal inputs checked count moved");
    }
}
