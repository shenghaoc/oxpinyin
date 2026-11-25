use super::*;
use crate::user::PinyinKey;

// Basis: phonetic_lookup.h:643-656 at 074a2219.
#[test]
fn a_step_prices_system_tokens_the_user_table_indexes() {
    let mut runtime = Runtime::open(&system_dir(), None).unwrap();
    let ni = SyllableKey::from_text("ni").unwrap();
    let shipped = runtime.dict().system().tokens_for_text("你").unwrap()[0];
    let keys = [crate::user::toned_key(ni.index(), 3).unwrap()];
    let appended = runtime
        .user_store_mut()
        .add_system_phrase_in(1, None, &"龘".into(), &keys, Some(5))
        .unwrap();
    let listed = runtime.dict().lookup(&[ni]).unwrap();
    for token in [shipped, appended] {
        assert!(listed.iter().any(|entry| entry.token() == token));
        let item = runtime
            .dict()
            .trellis_item(&[ni], &[], token)
            .expect("the step finds the item");
        assert!(item.pronunciation.is_some(), "token {token:#x} is priced");
    }
    assert!(
        runtime
            .dict()
            .trellis_records(&[ni], &[3])
            .unwrap()
            .iter()
            .any(|entry| entry.token() == shipped && entry.pronunciation_possibility().is_some())
    );
}

// Basis: phrase_index.cpp:150-205 at 074a2219.
#[test]
fn unigram_freq_reads_increments_from_the_user_item() {
    let mut runtime = Runtime::open(&system_dir(), None).unwrap();
    let ni = SyllableKey::from_text("ni").unwrap();
    let keys = [crate::user::toned_key(ni.index(), 3).unwrap()];
    for (text, count, expected) in [("鎄", 5, 15), ("龘", 0, 0)] {
        let token = runtime
            .user_store_mut()
            .add_phrase_in(5, &text.into(), &keys, Some(count))
            .unwrap();
        assert_eq!(token >> 24, 5);
        assert_eq!(runtime.user_store().system_unigram(token), None);
        assert_eq!(runtime.lm().unigram_freq(&token).unwrap(), Some(expected));
        assert!(runtime.add_unigram_delta(token, 7));
        assert_eq!(
            runtime.lm().unigram_freq(&token).unwrap(),
            Some(expected + 7)
        );
    }
}

// Basis: phrase_index.cpp:150-205 at 074a2219.
#[test]
fn unigram_freq_preserves_removed_and_missing_item_fallbacks() {
    let mut runtime = Runtime::open(&system_dir(), None).unwrap();
    let ni = SyllableKey::from_text("ni").unwrap();
    let keys = [crate::user::toned_key(ni.index(), 3).unwrap()];
    let token = runtime
        .user_store_mut()
        .add_system_phrase_in(1, None, &"龘".into(), &keys, Some(0))
        .unwrap();
    assert_eq!(runtime.user_store().system_unigram(token), Some(0));
    assert_eq!(runtime.lm().unigram_freq(&token).unwrap(), Some(0));
    let dir = tempfile::tempdir().unwrap();
    let token = runtime.dict().system().tokens_for_text("你").unwrap()[0];
    assert_eq!(token >> 24, 1);
    let old_item = runtime
        .dict()
        .live_items()
        .read(token, |item| match item {
            crate::user::SystemItem::Shipped(view) | crate::user::SystemItem::Replayed(view) => {
                view.as_bytes().to_vec()
            }
        })
        .found()
        .unwrap();
    let payload = crate::data::user_files::encode_log_records(&[
        crate::data::user_files::LogRecord::Remove {
            token: token & crate::vocab::PHRASE_MASK,
            old_item,
        },
    ])
    .unwrap();
    std::fs::write(
        dir.path().join("gb_char.dbin"),
        crate::data::chunk_format::build_memory_chunk(&payload).unwrap(),
    )
    .unwrap();
    assert!(
        crate::data::user_files::UserTableInfo::conform_to(&SystemVersions::from_conf(
            runtime.table_conf(),
        ))
        .save(dir.path().join("user.conf").as_path())
    );
    drop(runtime);
    let runtime = Runtime::open(&system_dir(), Some(dir.path())).unwrap();
    assert_eq!(runtime.user_store().system_unigram(token), None);
    assert_eq!(
        runtime.dict().live_items().read(token, |_| ()),
        crate::user::Lookup::Absent
    );

    let entries = runtime
        .dict()
        .lookup(&[SyllableKey::from_text("ni").unwrap()])
        .unwrap();
    assert!(entries.iter().all(|entry| entry.token() != token));
    assert_eq!(runtime.lm().unigram_freq(&token).unwrap(), Some(0));
    assert_eq!(runtime.lm().unigram_freq(&u32::MAX).unwrap(), Some(0));
}

// Basis: pinyin.cpp:416-440 and zhuyin.cpp:270-358 at 074a2219.
#[test]
fn zhuyin_open_holds_no_addon_or_punct_handles() {
    let system = tempfile::tempdir().unwrap();
    for entry in std::fs::read_dir(system_dir()).unwrap() {
        let path = entry.unwrap().path();
        std::fs::copy(&path, system.path().join(path.file_name().unwrap())).unwrap();
    }
    let conf_path = system.path().join("table.conf");
    let conf = std::fs::read_to_string(&conf_path).unwrap();
    let edited = conf.replace(
        "addon 4 art.table art.bin NULL DICTIONARY",
        "addon 4 art.table art-local.bin NULL DICTIONARY",
    );
    assert_ne!(conf, edited);
    std::fs::write(&conf_path, edited).unwrap();

    for (law, attached) in [(UserConfLaw::Pinyin, true), (UserConfLaw::Zhuyin, false)] {
        let runtime = Runtime::open_with_law(system.path(), None, law).unwrap();
        let addons = runtime.dict().addons.borrow();
        assert_eq!(addons.dict.has_open_indexes(), attached);
        assert_eq!(runtime.dict().punct.is_open(), attached);
        let row = addons.dict.row(4).unwrap();
        assert_eq!(
            row.system,
            Some(crate::data::FileName::from_bytes(b"art-local.bin"))
        );
        assert_eq!(Some(row), runtime.table_conf().addon_row(4));
    }
}

// Basis: Rust contract: exact-text lookup reads the current system and user rows.
#[test]
fn tokens_for_text_reads_live_rows_without_rebuilding_user_lookup() {
    let mut runtime = Runtime::open(&system_dir(), None).unwrap();
    let words = [u32::from('你'), u32::from('鎄')];
    let keys = native_keys("ni'hao");
    assert!(runtime.dict().tokens_for_ucs4(&words).is_empty());
    let token = runtime
        .user_store_mut()
        .add_phrase(&"你鎄".into(), &keys, None)
        .unwrap();
    assert_eq!(runtime.dict().tokens_for_ucs4(&words), [token]);
    assert!(runtime.user_store_mut().remove_user_phrase(token).unwrap());
    assert!(runtime.dict().tokens_for_ucs4(&words).is_empty());
}

// Basis: pinyin.cpp:563-582 at 074a2219.
#[test]
fn tokens_for_text_reads_the_row_after_a_system_reading_merge() {
    let dir = tempfile::tempdir().unwrap();
    let mut runtime = Runtime::open(&system_dir(), Some(dir.path())).unwrap();

    let tokens = runtime
        .dict()
        .tokens_for_ucs4(&"你好".chars().map(u32::from).collect::<Vec<_>>());
    let token = tokens[0];
    let library = u8::try_from(token >> 24).unwrap();
    let keys: Vec<_> = ["ni", "hao"]
        .into_iter()
        .map(|text| PinyinKey::try_from(SyllableKey::from_text(text).unwrap().index()).unwrap())
        .collect();

    runtime
        .user_store_mut()
        .add_system_phrase_in(library, Some(token), &"你好".into(), &keys, Some(5))
        .unwrap();
    assert_eq!(
        runtime
            .dict()
            .tokens_for_ucs4(&"你好".chars().map(u32::from).collect::<Vec<_>>()),
        tokens
    );
    let added = runtime
        .user_store_mut()
        .add_system_phrase_in(library, None, &"你鎄".into(), &keys, Some(5))
        .unwrap();
    assert_eq!(
        runtime
            .dict()
            .tokens_for_ucs4(&"你鎄".chars().map(u32::from).collect::<Vec<_>>()),
        [added]
    );
}

#[path = "../../tests/common/candidate_fixture.rs"]
mod candidate_fixture;
#[cfg(feature = "tkrzw")]
use crate::user::contract_data;
#[cfg(feature = "tkrzw")]
#[path = "../../tests/common/sentence_fixture.rs"]
mod sentence_fixture;
#[cfg(feature = "tkrzw")]
#[path = "../../tests/common/sentence_tail.rs"]
mod sentence_tail;

use std::path::PathBuf;

use candidate_fixture::{CandidateFixture, OracleCandidateType, STRUCTURE_FIXTURE};

const EXPECTED_PIN_REF: &str = concat!(
    "libpinyin-2.11.92-074a2219c90feaf962d0d24f034514033ece5f99",
    "+model20-59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155",
    "+dbm-tkrzw"
);

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

// Basis: Frozen candidate structure capture, fixtures/w4/oracle-candidate-structure.txt.
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
    use crate::engine::SessionSettings;
    use crate::runtime::{OpenError, Runtime, TableConfError};
    use crate::user::{FIRST_USER_TOKEN, PinyinKey};
    use crate::vocab::SENTENCE_START;
    use crate::{
        model::{Dictionary, LanguageModel},
        vocab::SyllableKey,
    };
    use std::path::Path;

    fn scratch_dir(prefix: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(prefix)
            .tempdir()
            .expect("temp dir")
    }

    use super::system_dir;

    // Basis: pinyin.cpp:2776-2818 at 074a2219.
    #[test]
    fn token_text_reads_live_items() {
        let mut runtime = Runtime::open(&system_dir(), None).expect("runtime");
        let system = runtime
            .dict()
            .system()
            .tokens_for_text("你")
            .expect("lookup")[0];
        let ni = SyllableKey::from_text("ni").expect("ni");
        let keys = [crate::user::toned_key(ni.index(), 3).expect("toned key")];
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

        assert_eq!(runtime.dict().token_text(0), None);
        for words in [
            vec![0x7532, 0, 0x8000_0000],
            vec![0xd800, 0x110000],
            vec![0x8000_0000],
        ] {
            let text = crate::vocab::PhraseText::from_stored_parts(words.clone(), words.len());
            let token = runtime
                .user_store_mut()
                .add_phrase(&text, &vec![keys[0]; words.len()], Some(0))
                .expect("stored user words");
            let actual = runtime.dict().token_text(token);
            let expected = crate::vocab::PhraseText::from_words(words);
            assert_eq!(actual, expected);
            if let (Some(actual), Some(expected)) = (actual, expected) {
                assert_eq!(actual.phrase_length(), expected.phrase_length());
                assert_eq!(actual.stored_words(), expected.stored_words());
            }
        }
    }

    // Basis: phrase_index.cpp:150-205 at 074a2219.
    #[test]
    fn narrow_token_reads_preserve_item_order() {
        let mut runtime = Runtime::open(&system_dir(), None).expect("runtime");
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
        let keys = [crate::user::toned_key(ni.index(), 3).unwrap()];
        runtime
            .user_store_mut()
            .add_system_phrase_in(1, Some(system), &"你".into(), &keys, Some(5))
            .expect("override");
        assert_eq!(packed(&runtime, system, 0).as_ref(), Some(&original));

        let user = runtime
            .user_store_mut()
            .add_phrase(&"词".into(), &keys, Some(5))
            .unwrap();
        let second = [crate::user::toned_key(ni.index(), 1).unwrap()];
        runtime
            .user_store_mut()
            .add_phrase(&"词".into(), &second, Some(9))
            .unwrap();

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
                    crate::parser::ChewingKey::from_pinyin("ni")
                        .unwrap()
                        .with_tone(tone)
                        .to_packed()
                ])
            );
        }
        assert_eq!(runtime.dict().token_pronunciation_count(user), Some(2));
        assert_eq!(user_packed(&runtime, 2), None);
        for token in [system, user] {
            assert!(runtime.dict().token_exists(token));
            assert_eq!(runtime.dict().phrase_length_for_token(token), Some(1));
        }
        for token in [0x01ff_ffff, 0x07ff_ffff] {
            assert!(!runtime.dict().token_exists(token));
            assert_eq!(runtime.dict().phrase_length_for_token(token), None);
            assert!(runtime.dict().token_nth_pronunciation(token, 0).is_none());
        }
    }

    // Basis: pinyin.cpp:335-340 at 074a2219.
    #[test]
    fn open_refuses_a_directory_without_table_conf() {
        let tmp = scratch_dir("oxpinyin-runtime-no-conf-");
        for entry in std::fs::read_dir(system_dir()).expect("w3 fixture") {
            let entry = entry.expect("w3 entry");
            if entry.file_name() != "table.conf" {
                std::fs::copy(entry.path(), tmp.path().join(entry.file_name())).expect("copy");
            }
        }
        assert!(
            tmp.path()
                .join(crate::data::SystemDbm::PinyinIndex.file_name())
                .exists()
        );
        match Runtime::open(tmp.path(), None) {
            Err(OpenError::TableConf(TableConfError::Unopenable)) => {}
            Err(other) => panic!("expected an unopenable table.conf, got {other}"),
            Ok(_) => panic!("a directory without table.conf must not open"),
        }
    }

    // Basis: Rust contract: a profile feeds live entries and unigram totals to the same runtime.
    #[test]
    fn usable_user_dir_feeds_the_merged_lookup_and_overlay() {
        let tmp = scratch_dir("oxpinyin-runtime-user-");
        let dir = tmp.path().to_path_buf();

        let mut runtime = Runtime::open(&system_dir(), Some(&dir)).expect("open");
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

        let phrase = "你鎄";
        let merged = |runtime: &Runtime| runtime.dict().lookup(&syllables).expect("merged lookup");
        assert!(
            !merged(&runtime).iter().any(|entry| entry.text() == phrase),
            "fixture already carries the probe phrase"
        );

        let token = runtime
            .user_store_mut()
            .add_phrase(&crate::vocab::PhraseText::from(phrase), &keys, None)
            .expect("learn it");

        let entries = merged(&runtime);
        assert!(
            entries.iter().any(|entry| entry.text() == phrase),
            "learned phrase missing from merged lookup: {:?}",
            entries
                .iter()
                .map(|e| e.text().to_owned())
                .collect::<Vec<_>>()
        );

        let store_before = runtime.user_store_mut().unigram_total();

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

        let freq = LanguageModel::unigram_freq(&runtime.lm(), &token).expect("overlay query");
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

    // Basis: pinyin.cpp:176-187,332 at 074a2219.
    #[rstest::rstest]
    #[case::absent(false)]
    #[case::regular_file(true)]
    fn user_path_presence_selects_profile_storage(#[case] supplied: bool) {
        let file = tempfile::NamedTempFile::new().unwrap().into_temp_path();

        let runtime = Runtime::open(&system_dir(), supplied.then_some(file.as_ref())).unwrap();
        assert_eq!(runtime.user_store().has_user_directory(), supplied);
    }

    // Basis: Rust contract: the addon LM exposes totals only after an addon library loads.
    #[test]
    fn addon_unigram_totals_stay_none_until_a_library_loads() {
        let mut runtime = Runtime::open(&system_dir(), None).expect("open");
        assert_eq!(
            LanguageModel::addon_unigram_total(&runtime.lm()).expect("infallible query"),
            None,
            "no addon library is loaded"
        );

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

    // Basis: phrase_index.h:628-635 and phrase_index.cpp:150-176 at 074a2219.
    #[test]
    fn unigram_add_distinguishes_a_missing_sub_index_from_a_missing_slot() {
        let mut runtime = Runtime::open(&system_dir(), None).unwrap();
        let before = runtime.lm().amplified_total();
        assert!(!runtime.add_unigram_delta(0x0800_0001, 9));
        assert_eq!(runtime.lm().amplified_total(), before);
        assert!(!runtime.add_unigram_delta(FIRST_USER_TOKEN, 9));
        assert_eq!(runtime.lm().amplified_total(), before + 9);
    }

    // Basis: pinyin.cpp:2836-2843 at 074a2219.
    #[test]
    fn unigram_add_rejects_a_token_found_only_in_the_addon_index() {
        use crate::data::phrase_library::PhraseLibrary;

        let system = scratch_dir("oxpinyin-addon-only-");
        for entry in std::fs::read_dir(system_dir()).unwrap() {
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
        assert!(runtime.lm().addon_unigram_freq(&token).unwrap().unwrap() > 0);
        assert!(!runtime.dict().token_exists(token));
        let before = runtime.lm().amplified_total();
        assert!(!runtime.add_unigram_delta(token, 9));
        assert_eq!(runtime.lm().amplified_total(), before);
    }

    // Basis: phrase_index.h:628-635 and phrase_index.cpp:150-176 at 074a2219.
    #[test]
    fn rejected_unigram_add_keeps_item_and_library_counts_but_moves_facade_total() {
        let mut runtime = Runtime::open(&system_dir(), None).expect("open");
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

    // Basis: phonetic_lookup.h:588-599 at 074a2219.
    #[test]
    fn a_fresh_nbest_row_observes_a_reborrowed_stores_committed_write() {
        use crate::model::{NbestStepRow, NbestSuccessors};
        let tmp = scratch_dir("oxpinyin-nbest-snapshot-");
        let dir = tmp.path().to_path_buf();
        let mut runtime = Runtime::open(&system_dir(), Some(&dir)).unwrap();
        let keys: Vec<_> = ["ni", "hao"]
            .into_iter()
            .map(|text| SyllableKey::from_text(text).unwrap().index() as PinyinKey)
            .collect();
        let token = runtime
            .user_store_mut()
            .add_phrase(&"你鎄".into(), &keys, Some(5))
            .unwrap();
        let other = runtime
            .user_store_mut()
            .add_phrase(&"你鎬".into(), &keys, Some(5))
            .unwrap();
        crate::user::seed_bigram(runtime.user_store_mut(), SENTENCE_START, &[(other, 1000)]);
        let prev = SENTENCE_START;
        let mut row = NbestStepRow::default();

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
                    .get_freq(token)
                    .unwrap()
                    .map(|freq| (freq, gram.total().unwrap())),
                NbestSuccessors::Miss => None,
            };
            runtime
                .lm()
                .nbest_token_costs(&token)
                .unwrap()
                .costs(item.map(|(freq, total)| (u64::from(freq), u64::from(total), true)))
        };
        let before = costs(&runtime, &mut row);
        assert_eq!(
            before,
            runtime.lm().nbest_step_costs(&prev, &token).unwrap()
        );
        assert!(carries(&runtime, &mut row, other));

        runtime
            .user_store_mut()
            .observe_selection(SENTENCE_START, token)
            .unwrap();
        let trained = runtime.lm().nbest_step_costs(&prev, &token).unwrap();
        assert_ne!(trained, before);
        let mut row = NbestStepRow::default();
        assert!(carries(&runtime, &mut row, token));
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

    // Basis: facade_chewing_table2.h:109-129 at 074a2219.
    #[test]
    fn trellis_ranges_continued_matches_the_ranges_and_prefix_pair() {
        let mut runtime = Runtime::open(&system_dir(), None).unwrap();
        let zuo = SyllableKey::from_text("zuo").unwrap();
        let key = crate::user::toned_key(zuo.index(), 3).unwrap();
        runtime
            .user_store_mut()
            .add_phrase(&"坐坐坐".into(), &[key; 3], Some(5))
            .unwrap();
        let mut scratch = Vec::new();
        for (spellings, tones, masked, expected_continuation) in [
            (&["ni"][..], &[][..], false, true),
            (&["zuo", "zuo"][..], &[][..], false, true),
            (&["ni", "hao", "ma"][..], &[0, 3, 0][..], false, false),
            (&["ni"][..], &[3][..], true, true),
        ] {
            if masked {
                assert!(runtime.unload_library(2));
            }
            let keys: Vec<_> = spellings
                .iter()
                .map(|s| SyllableKey::from_text(s).unwrap())
                .collect();
            let dict = runtime.dict();
            let mut pair = crate::model::PhraseIndexRanges::new();
            dict.trellis_ranges(&keys, tones, &mut pair).unwrap();
            let mut fused = crate::model::PhraseIndexRanges::new();
            let continued = dict
                .trellis_ranges_continued(&keys, tones, &mut fused, &mut scratch)
                .unwrap();
            assert!(pair.iter().eq(fused.iter()));
            assert_eq!(continued, expected_continuation, "{spellings:?}");
            assert_eq!(continued, dict.phrase_prefix_exists(&keys));
        }
    }

    // Basis: Rust contract: each guess reads the current user rows.
    #[test]
    fn a_store_write_between_guesses_reaches_the_second_guess() {
        let tmp = scratch_dir("oxpinyin-guess-memo-");
        let dir = tmp.path().to_path_buf();
        let mut runtime = Runtime::open(&system_dir(), Some(&dir)).unwrap();
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
        crate::user::seed_bigram(runtime.user_store_mut(), SENTENCE_START, &[(token, 1000)]);
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

    // Basis: Register row 79; pinyin.cpp:253-256,377-392 at 074a2219.
    #[test]
    fn a_default_dictionary_row_reports_only_the_libraries_before_it() {
        let tmp = scratch_dir("oxpinyin-runtime-row-79-");
        let dir = tmp.path().to_path_buf();
        let conf = std::fs::read_to_string(system_dir().join("table.conf")).expect("table.conf");
        let conf = conf.replace(
            "default OPENGRAM_DICTIONARY opengram.table opengram.bin opengram.dbin SYSTEM_FILE",
            "default OPENGRAM_DICTIONARY opengram.table opengram.bin NULL DICTIONARY",
        );
        std::fs::write(dir.join("table.conf"), conf).expect("write table.conf");
        let mut reported = Vec::new();
        let opened = Runtime::open_with_law_reporting(
            &dir,
            None,
            crate::user::UserConfLaw::Pinyin,
            &mut |path| reported.push(path.to_path_buf()),
        );
        assert!(matches!(
            opened,
            Err(OpenError::TableConf(TableConfError::Abort(
                crate::runtime::TableConfSite::DefaultDictionary
            )))
        ));
        assert_eq!(
            reported,
            [dir.join("gb_char.bin"), dir.join("gbk_char.bin")]
        );
    }

    // Basis: Register rows 91 and 108; pinyin.cpp:253-276 at 074a2219.
    #[test]
    fn a_journal_fault_reports_only_the_libraries_through_its_own() {
        let system_tmp = scratch_dir("oxpinyin-runtime-journal-system-");
        let system = system_tmp.path().to_path_buf();
        for entry in std::fs::read_dir(system_dir()).expect("fixture") {
            let entry = entry.expect("entry");
            let name = entry.file_name();
            if name != "gbk_char.bin" && name != "merged.bin" {
                std::fs::copy(entry.path(), system.join(&name)).expect("copy");
            }
        }
        let user_tmp = scratch_dir("oxpinyin-runtime-journal-user-");
        let user = user_tmp.path().to_path_buf();
        let conf = crate::data::TableConf::load_from_dir(&system).expect("conf");
        let versions = crate::data::user_files::SystemVersions::from_conf(&conf);
        assert!(
            crate::data::user_files::UserTableInfo::conform_to(&versions)
                .save(user.join("user.conf").as_path())
        );

        let mut header = 4_u32.to_le_bytes().to_vec();
        header.extend_from_slice(&0x0300_0001_u32.to_le_bytes());
        header.extend_from_slice(&4_u16.to_le_bytes());
        header.extend_from_slice(&[0; 8]);
        std::fs::write(
            user.join("opengram.dbin"),
            crate::data::chunk_format::build_memory_chunk(&header).expect("frame"),
        )
        .expect("opengram.dbin");
        let mut reported = Vec::new();
        let opened = Runtime::open_with_law_reporting(
            &system,
            Some(&user),
            crate::user::UserConfLaw::Pinyin,
            &mut |path| reported.push(path.to_path_buf()),
        );
        assert!(matches!(opened, Err(OpenError::Journal(fault)) if fault.library() == 3));
        assert_eq!(reported, [system.join("gbk_char.bin")]);
    }

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

    fn open_counter(dir: &Path) -> i32 {
        let text = std::fs::read_to_string(dir.join("user.conf")).expect("user.conf");
        text.lines()
            .find_map(|line| line.strip_prefix("open counter:"))
            .expect("an open counter line")
            .parse()
            .expect("a decimal counter")
    }

    // Basis: pinyin.cpp:176-187,332 at 074a2219.
    #[test]
    fn empty_user_path_is_the_working_directory_at_each_use() {
        let first_dir = scratch("first");
        let second_dir = scratch("second");
        let first = first_dir.path().to_path_buf();
        let second = second_dir.path().to_path_buf();
        std::env::set_current_dir(&first).expect("enter the first directory");

        let mut runtime = Runtime::open(&system_dir(), Some(Path::new(""))).expect("open");
        let store = runtime.user_store_mut();
        assert_eq!(names(&first), ["user.conf"]);
        assert_eq!(open_counter(&first), 1);

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
            .add_phrase(&crate::vocab::PhraseText::from(phrase), &keys, None)
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

        drop(runtime);
        assert_eq!(open_counter(&second), 0);
        assert_eq!(open_counter(&first), 1);

        let strays = ["user_bigram.db-lock", "user_bigram.db-shm"];
        for stray in strays {
            std::fs::write(stray, b"stale").expect("plant a stray file");
        }
        let mut runtime = Runtime::open(&system_dir(), Some(Path::new(""))).expect("reopen");
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
    use super::contract_data;

    use crate::{
        model::{Dictionary, LanguageModel},
        vocab::SyllableKey,
    };

    use crate::engine::SessionSettings;

    use crate::runtime::Runtime;

    fn syllables(text: &str) -> Vec<SyllableKey> {
        text.split('\'')
            .map(|s| SyllableKey::from_text(s).expect("frozen syllable"))
            .collect()
    }

    // Basis: Rust contract: the runtime opens and decodes the unmodified pinned data package.
    #[test]
    fn an_unmodified_libpinyin_data_directory_opens_and_decodes() {
        let dir = contract_data::contract_data_dir();

        let conf = std::fs::read_to_string(dir.join("table.conf")).expect("table.conf");
        let format = format!("database format:{}", crate::data::DEFAULT_STORE_DB_FORMAT);
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

        let total = LanguageModel::unigram_total(&lm)
            .expect("query")
            .expect("real unigrams");
        assert_eq!(
            total, 51_051_831,
            "Σ item unigram of the pinned model: 50,913,735 \\1-gram counts + one per item"
        );

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
            .trellis_item(&syllables("ni'hao"), &[], entry.token())
            .expect("你好 is ranged on ni'hao");
        assert!(
            item.pronunciation.is_some_and(|(m, t)| m > 0 && m <= t),
            "possibility from the chunk pronunciations"
        );
        assert!(dict.phrase_prefix_exists(&syllables("zhong")));

        let lone = SyllableKey::from_text("beng").expect("frozen syllable");
        let _ = dict.lookup(&[lone]).expect("lookup");

        let tokens = dict.tokens_for_ucs4(&"中国".chars().map(u32::from).collect::<Vec<_>>());
        assert!(!tokens.is_empty());
        for token in &tokens {
            assert_eq!(
                dict.phrase_text_for_token(*token)
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

        let de = dict.tokens_for_ucs4(&"的".chars().map(u32::from).collect::<Vec<_>>());
        assert!(de.iter().any(|token| !dict.punctuations(*token).is_empty()));

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
    }
}

#[cfg(feature = "tkrzw")]
mod candidate_surface {
    use super::{candidate_fixture, contract_data, repo_root};

    use crate::engine::{Candidate, CandidateKind, Session, SessionSettings};

    use crate::runtime::Runtime;

    use candidate_fixture::{CandidateFixture, OracleCandidateType, STRUCTURE_FIXTURE};

    const CAPTURE_DEPTH: usize = 10;

    fn candidate_type(candidate: &Candidate) -> Option<OracleCandidateType> {
        match candidate.kind() {
            CandidateKind::Phrase => Some(OracleCandidateType::Normal),
            CandidateKind::Addon => Some(OracleCandidateType::Addon),
            CandidateKind::Sentence => Some(OracleCandidateType::NbestMatch),
            CandidateKind::Fallback => None,
        }
    }

    // Basis: Frozen candidate structure capture, fixtures/w4/oracle-candidate-structure.txt.
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
    use super::{contract_data, sentence_fixture, sentence_tail};

    use sentence_tail::PortSession;

    fn open_session() -> PortSession {
        PortSession::open(&contract_data::contract_data_dir()).expect("the package opens")
    }

    // Basis: Frozen sentence-surface residuals, docs/findings/sentence-surface.md §12.
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

        assert_eq!(
            report.list_order_only, 0,
            "an order-only sentence divergence appeared; re-measure §12"
        );

        assert_eq!(
            report.list_distinct_extra, 1,
            "the distinct-set minus ordered gap moved from 1 (§12)"
        );
    }

    // Basis: Frozen sentence-surface residuals, docs/findings/sentence-surface.md §12.
    #[test]
    fn sentence_surface_matches_supplementary_tie_order() {
        let cases = sentence_fixture::load_tie_cases(&sentence_fixture::repo_root());
        let mut owner = open_session();
        let mut session = owner.session();
        session
            .set_options(crate::OptionBits::from_bits_retain(0x18a))
            .expect("capture options must apply");
        session
            .set_sort_options(0x1e)
            .expect("capture sort must apply");
        let (mut pin_equal, mut residuals) = (0, 0);
        let mut moved = Vec::new();
        for (ordinal, case) in cases.iter().enumerate() {
            session.reset();

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
                    .filter(|candidate| candidate.kind() == crate::engine::CandidateKind::Sentence)
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

use crate::OptionBits;
use crate::user::native_fixture;
use native_fixture::{
    TempUserDir, craft_hash, craft_tree, gram_bytes, set_phrase_table_row, system_dir,
};

fn native_session(runtime: &Runtime, options: OptionBits) -> crate::engine::SessionState {
    let mut state = runtime.new_session(SessionSettings::default());
    let mut session = runtime.session(&mut state);
    session.set_defer_list_builds(true);
    session.set_options(options).unwrap();
    state
}
fn native_keys(reading: &str) -> Vec<crate::user::PinyinKey> {
    if !reading.is_ascii() {
        return crate::parser::parse_zhuyin_direct(reading, OptionBits::USE_TONE)
            .into_iter()
            .map(|(key, tone)| crate::user::toned_key(key.index(), tone).unwrap())
            .collect();
    }
    let graph =
        crate::graph::SegmentGraph::build_with_options(reading.as_bytes(), OptionBits::USE_TONE)
            .unwrap();
    assert_eq!(graph.consumed(), reading.len());
    graph
        .fewest_keys(false)
        .iter()
        .map(|edge| crate::user::toned_key(edge.key().index(), edge.tone()).unwrap())
        .collect()
}
fn import_rows(runtime: &mut Runtime, library: u8, rows: &[(&str, &str, u64)]) {
    for &(text, reading, count) in rows {
        let original = runtime
            .dict()
            .tokens_for_ucs4(&text.chars().map(u32::from).collect::<Vec<_>>())
            .into_iter()
            .find(|token| crate::vocab::phrase_index_library_index(*token) == library);
        let keys = native_keys(reading);
        let store = runtime.user_store_mut();
        if store.is_system_library(library) {
            store
                .add_system_phrase_in(library, original, &text.into(), &keys, Some(count))
                .unwrap();
        } else {
            store
                .add_phrase_in(library, &text.into(), &keys, Some(count))
                .unwrap();
        }
    }
    runtime.user_store_mut().compact_libraries();
    runtime.user_store_mut().mark_modified();
}
fn normal_texts(runtime: &Runtime, state: &mut crate::engine::SessionState) -> Vec<String> {
    runtime
        .session(state)
        .candidates_at_for_abi(0, false)
        .unwrap()
        .iter()
        .filter(|candidate| candidate.kind() != crate::engine::CandidateKind::Sentence)
        .map(|candidate| candidate.text().as_str().unwrap().to_owned())
        .collect()
}

// Basis: pinyin.cpp:1415-1421 and phonetic_lookup.h:244-265 at 074a2219.
#[test]
fn repeated_prefix_tokens_keep_duplicate_sentence_rows() {
    let scratch = tempfile::tempdir().expect("scratch");
    let system = native_fixture::items_system(
        &system_dir(),
        scratch.path(),
        &[
            (&[0x7532], &[0x1081], 1),
            (&[0x4e59], &[0x1081], 1),
            (&[0x4e19], &[0x1084], 999_998),
        ],
        1_000_000,
    );
    let config = system.join("table.conf");
    let config_text = std::fs::read_to_string(&config).expect("configuration");
    std::fs::remove_file(&config).expect("unlink fixture configuration");
    std::fs::write(
        &config,
        config_text
            .lines()
            .map(|line| {
                if line.starts_with("lambda parameter:") {
                    "lambda parameter:0.5"
                } else {
                    line
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .expect("configuration");
    craft_hash(
        &system.join("bigram.db"),
        &[
            (
                1_u32.to_le_bytes().to_vec(),
                gram_bytes(1000, &[(0x0100_0002, 10), (0x0100_0003, 990)]),
            ),
            (
                0x0100_0003_u32.to_le_bytes().to_vec(),
                gram_bytes(1000, &[(0x0100_0001, 1000)]),
            ),
        ],
    );
    let user = TempUserDir::new("repeated-prefix");

    set_phrase_table_row(&user.path, "丙", &[0x0100_0003]);

    let runtime = Runtime::open(&system, Some(&user.path)).unwrap();
    let mut state = native_session(&runtime, OptionBits::USE_TONE);
    runtime.session(&mut state).replace_raw("ba1").unwrap();
    let prefixes = runtime
        .dict()
        .tokens_for_ucs4(&"丙".chars().map(u32::from).collect::<Vec<_>>());
    assert_eq!(prefixes.to_vec(), [0x0100_0003, 0x0100_0003]);
    assert!(
        runtime
            .session(&mut state)
            .guess_sentence_with_prefix(&prefixes)
            .unwrap()
    );
    for (index, text) in [(0, "甲"), (1, "甲"), (2, "乙")] {
        assert_eq!(
            runtime
                .session(&mut state)
                .sentence_text(index)
                .and_then(|text| text.as_str()),
            Some(text)
        );
    }
}

// Basis: phonetic_lookup.h:552-554,776-788 at 074a2219.
#[rstest::rstest]
#[case::system(1)]
#[case::user(6)]
fn an_unranged_onestep_token_still_produces_a_sentence(#[case] library: u8) {
    let user = TempUserDir::new("unranged-onestep");
    let mut runtime = Runtime::open(&system_dir(), Some(&user.path)).unwrap();
    import_rows(
        &mut runtime,
        library,
        &[("甲", "ba4", 3), ("甲", "da4", 9), ("乙", "da4", 2)],
    );
    let mut state = native_session(&runtime, OptionBits::USE_TONE);
    runtime.session(&mut state).replace_raw("ba4").unwrap();
    let candidate = runtime
        .session(&mut state)
        .candidates_at_for_abi(0, false)
        .unwrap()
        .iter()
        .find(|row| row.text() == "甲")
        .unwrap()
        .clone();
    assert_eq!(candidate.consumed_bytes(), 3);
    runtime
        .session(&mut state)
        .select_target(&crate::engine::SelectionTarget::of_candidate(
            &candidate, 0, None,
        ))
        .unwrap();
    runtime.session(&mut state).replace_raw("da4").unwrap();
    let rows = normal_texts(&runtime, &mut state);
    assert!(rows.iter().any(|row| row == "乙"));
    assert!(!rows.iter().any(|row| row == "甲"));
    assert!(runtime.session(&mut state).guess_sentence().unwrap());
    assert_eq!(runtime.session(&mut state).sentence_text(0).unwrap(), "甲");
}

// Basis: phonetic_lookup.h:588-599 at 074a2219.
#[test]
fn a_later_span_reads_the_sentence_start_gram_again() {
    let scratch = tempfile::tempdir().expect("scratch");
    let system = native_fixture::items_system(
        &system_dir(),
        scratch.path(),
        &[
            (&[0x7532], &[0x1081], 1),
            (&[0x4e59], &[0x1084], 1000),
            (&[0x4e19, 0x4e01], &[0x1081, 0x1084], 1),
        ],
        2000,
    );
    let sentence_start = 1_u32.to_le_bytes();
    craft_hash(
        &system.join("bigram.db"),
        &[(
            sentence_start.to_vec(),
            gram_bytes(1000, &[(0x0100_0001, 500), (0x0100_0003, 500)]),
        )],
    );
    let user = TempUserDir::new("start-gram-reread");
    let runtime = Runtime::open(&system, Some(&user.path)).unwrap();
    let mut state = native_session(&runtime, OptionBits::USE_TONE);
    runtime.session(&mut state).replace_raw("ba1da1").unwrap();
    assert!(runtime.session(&mut state).guess_sentence().unwrap());
    assert_eq!(
        runtime.session(&mut state).sentence_text(0).unwrap(),
        "丙丁"
    );
    crate::store::fail_point_read_of(&sentence_start, 1);
    assert!(runtime.session(&mut state).guess_sentence().unwrap());
    assert_eq!(
        runtime.session(&mut state).sentence_text(0).unwrap(),
        "甲乙"
    );
}

// Basis: phrase_lookup.cpp:288-344 at 074a2219.
#[rstest::rstest]
#[case::zero_weight("1", false)]
#[case::nan_weight("nan", false)]
#[case::infinite_bigram("1", true)]
fn nonfinite_segment_scores_keep_the_token_path(#[case] lambda: &str, #[case] infinite: bool) {
    use native_fixture::{TempUserDir, craft_hash, gram_bytes, private_system};

    const NI: u32 = 0x0100_1225;
    const HAO: u32 = 16_779_429;
    let scratch = tempfile::tempdir().expect("scratch");
    let system = private_system(scratch.path());
    let conf = system.join("table.conf");
    let text = std::fs::read_to_string(&conf).expect("fixture table.conf");
    std::fs::remove_file(&conf).expect("unlink fixture config");
    std::fs::write(
        &conf,
        text.replace(
            "lambda parameter:0.312699",
            &format!("lambda parameter:{lambda}"),
        ),
    )
    .expect("private table.conf");
    let rows = if infinite {
        vec![
            (1_u32.to_le_bytes().to_vec(), gram_bytes(0, &[(NI, 1)])),
            (NI.to_le_bytes().to_vec(), gram_bytes(0, &[(HAO, 1)])),
        ]
    } else {
        Vec::new()
    };
    craft_hash(&system.join("bigram.db"), &rows);

    let user = TempUserDir::new("segment-ieee");
    let runtime = Runtime::open(&system, Some(&user.path)).unwrap();
    let mut state = native_session(&runtime, OptionBits::USE_TONE);
    let tokens = |text: &str| {
        runtime
            .dict()
            .tokens_for_ucs4(&text.chars().map(u32::from).collect::<Vec<_>>())
            .to_vec()
    };
    assert_eq!(tokens("你"), [NI]);
    assert!(tokens("你你").is_empty());

    let (sentence, expected) = if infinite {
        assert_eq!(tokens("好"), [HAO]);
        assert!(!tokens("你好").is_empty());
        ("你好", [NI, HAO])
    } else {
        ("你你", [NI, NI])
    };

    let (matched, tokens) = runtime
        .session(&mut state)
        .phrase_segment(sentence)
        .unwrap();
    assert!(matched);
    assert_eq!(tokens.to_vec(), expected);
}

// Basis: Register row 154, stored-gram partitions in phrase segmentation.
#[cfg(feature = "tkrzw")]
#[test]
fn the_segmenter_reads_an_unsorted_user_gram_as_stored() {
    let user = TempUserDir::new("unsorted-segment");
    drop(Runtime::open(&contract_data::contract_data_dir(), Some(&user.path)).unwrap());
    let gram = native_fixture::gram_bytes(
        (1 << 31) + 2,
        &[(16_800_245, 1 << 31), (16_800_252, 1), (16_800_244, 1)],
    );
    native_fixture::craft_hash(
        &user.path.join("user_bigram.db"),
        &[(1_u32.to_le_bytes().to_vec(), gram)],
    );
    native_fixture::write_user_conf(&user.path, &contract_data::contract_data_dir());
    let runtime = Runtime::open(&contract_data::contract_data_dir(), Some(&user.path)).unwrap();
    let mut state = native_session(&runtime, OptionBits::USE_TONE);
    let (matched, path) = runtime
        .session(&mut state)
        .phrase_segment("今天天气很好")
        .unwrap();
    assert!(matched);
    let tokens: Vec<_> = path.to_vec();
    assert_eq!(
        tokens,
        [16_800_245, 0, 16_810_538, 0, 16_779_492, 16_779_429]
    );
}

// Basis: phrase_index.h:136-164 and pinyin.cpp:563-582 at 074a2219.
#[rstest::rstest]
#[case::system(1, "ba4", "ba3", [2, 98, 1], None)]
#[case::user(6, "ba", "da", [3, 9, 2], Some("da"))]
fn a_step_prices_readings_with_the_query_tone(
    #[case] library: u8,
    #[case] query: &str,
    #[case] second: &str,
    #[case] counts: [u64; 3],
    #[case] unindexed: Option<&str>,
) {
    let user = TempUserDir::new("step-price");
    let mut runtime = Runtime::open(&system_dir(), Some(&user.path)).unwrap();
    import_rows(
        &mut runtime,
        library,
        &[
            ("甲", query, counts[0]),
            ("甲", second, counts[1]),
            ("乙", query, counts[2]),
        ],
    );
    let mut state = native_session(&runtime, OptionBits::USE_TONE);
    runtime.session(&mut state).replace_raw(query).unwrap();
    assert!(runtime.session(&mut state).guess_sentence().unwrap());
    assert_eq!(runtime.session(&mut state).sentence_text(0).unwrap(), "乙");
    if let Some(second) = unindexed {
        runtime.session(&mut state).replace_raw(second).unwrap();
        assert!(
            !normal_texts(&runtime, &mut state)
                .iter()
                .any(|text| text == "甲")
        );
    }
}

// Basis: pinyin.cpp:2231-2256 at 074a2219.
#[test]
fn a_missing_candidate_table_row_keeps_searching_longer_spans() {
    let user = TempUserDir::new("candidate-read-miss");
    use crate::store::RawReadStore as _;
    let scratch = tempfile::tempdir().unwrap();
    let system = native_fixture::private_system(scratch.path());
    let key = crate::parser::ChewingKey::from_pinyin("ni")
        .unwrap()
        .to_packed();
    let runtime = Runtime::open(&system_dir(), Some(&user.path)).unwrap();
    let mut state = native_session(&runtime, OptionBits::USE_TONE);
    runtime.session(&mut state).replace_raw("nihao").unwrap();
    let before = normal_texts(&runtime, &mut state);
    assert!(before.iter().any(|text| text == "你"));
    assert!(before.iter().any(|text| text == "你好"));
    let index = system.join("pinyin_index.bin");
    let store = crate::store::DefaultStore::open_read_only(&index).unwrap();
    let mut rows = Vec::new();
    store
        .range_raw(
            std::ops::Bound::Unbounded,
            std::ops::Bound::Unbounded,
            |row, value| {
                if row != key.to_le_bytes() {
                    rows.push((row.to_vec(), value.to_vec()));
                }
                Ok(())
            },
        )
        .unwrap();
    drop(store);
    native_fixture::craft_tree(&index, &rows);
    let runtime = Runtime::open(&system, Some(&user.path)).unwrap();
    let mut state = native_session(&runtime, OptionBits::USE_TONE);
    runtime.session(&mut state).replace_raw("nihao").unwrap();
    let after = normal_texts(&runtime, &mut state);
    assert!(!after.iter().any(|text| text == "你"));
    assert!(after.iter().any(|text| text == "你好"));
}

// Basis: pinyin.cpp:2231-2256 at 074a2219.
#[test]
fn addon_candidate_continuation_uses_its_row_read() {
    use native_fixture::{AddonItem, addon_system};
    let packed = |text| {
        crate::parser::ChewingKey::from_pinyin(text)
            .expect("complete key")
            .to_packed()
    };
    let short = [packed("ba")];
    let long = [short[0], packed("ma")];
    let scratch = tempfile::tempdir().expect("scratch");
    let system = addon_system(
        &system_dir(),
        scratch.path(),
        &[
            AddonItem {
                text: &[u32::from('甲')],
                readings: &[(&short, 5)],
                indexed_as: &short,
                unigram: 15,
            },
            AddonItem {
                text: &[u32::from('乙'), u32::from('丙')],
                readings: &[(&long, 5)],
                indexed_as: &long,
                unigram: 15,
            },
        ],
    );
    let index = system.join("pinyin_index.bin");
    std::fs::remove_file(&index).expect("unlink the default index");
    craft_tree(&index, &[]);

    let user = TempUserDir::new("addon-read-continuation");
    let mut runtime = Runtime::open(&system, Some(&user.path)).unwrap();
    assert!(runtime.load_system_addon_reporting(4).unwrap());
    let mut state = native_session(&runtime, OptionBits::USE_TONE);
    runtime.session(&mut state).replace_raw("bama").unwrap();
    let reads = if cfg!(feature = "kyotocabinet") { 3 } else { 2 };
    crate::store::fail_point_read_of(&short[0].to_le_bytes(), reads);
    let rows = runtime
        .session(&mut state)
        .candidates_at_for_abi(0, false)
        .unwrap();
    for expected in ["甲", "乙丙"] {
        assert!(
            rows.iter()
                .any(|row| row.kind() == crate::engine::CandidateKind::Addon
                    && row.text() == expected)
        );
    }
}

fn native_chewing(runtime: &Runtime, state: &mut crate::engine::SessionState, input: &str) {
    let options = OptionBits::USE_TONE | OptionBits::FORCE_TONE;
    let parsed = crate::parser::ZhuyinParser::with_scheme(crate::parser::ZhuyinScheme::Standard)
        .parse_with_options(input.as_bytes(), options);
    assert_eq!(parsed.consumed(), input.len());
    let segments: Vec<_> = parsed
        .keys()
        .iter()
        .map(|key| crate::graph::ExactSegment::new(key.start(), key.end(), key.key(), key.tone()))
        .collect();
    runtime
        .session(state)
        .set_nbest_shape(crate::engine::NbestShape::ZHUYIN);
    runtime
        .session(state)
        .replace_raw_exact(input, &segments)
        .unwrap();
}

fn gbk_tokens(runtime: &Runtime, state: &mut crate::engine::SessionState) -> Vec<u32> {
    runtime
        .session(state)
        .candidates_at_for_abi(0, false)
        .unwrap()
        .iter()
        .filter_map(|row| row.token())
        .filter(|token| token >> 24 == 2)
        .collect()
}

// Basis: Register row 54; pinyin.cpp:464-475 at 074a2219.
#[test]
fn an_unload_hides_its_rows_without_a_reparse() {
    let user = TempUserDir::new("row54-unload");
    let mut runtime = Runtime::open(&system_dir(), Some(&user.path)).unwrap();
    let mut state = native_session(&runtime, OptionBits::USE_TONE);
    runtime.session(&mut state).replace_raw("ni").unwrap();
    assert!(!gbk_tokens(&runtime, &mut state).is_empty());
    assert!(runtime.unload_library(2));
    assert!(gbk_tokens(&runtime, &mut state).is_empty());
    assert!(!normal_texts(&runtime, &mut state).is_empty());
}

// Basis: Frozen import/dilution/cece observation.
#[test]
fn an_unindexed_second_reading_dilutes_the_sentence_score() {
    let user = TempUserDir::new("import-dilution");
    let mut runtime = Runtime::open(&system_dir(), Some(&user.path)).unwrap();
    import_rows(
        &mut runtime,
        1,
        &[("测侧", "ce'ce", 1_000_000), ("侧测", "ce'ce", 600_000)],
    );
    let mut state = native_session(&runtime, OptionBits::USE_TONE);
    let sentence = |runtime: &Runtime, state: &mut crate::engine::SessionState| {
        runtime.session(state).replace_raw("cece").unwrap();
        assert!(runtime.session(state).guess_sentence().unwrap());
        let text = runtime.session(state).sentence_text(0).unwrap().clone();
        runtime.session(state).reset();
        text
    };
    assert_eq!(sentence(&runtime, &mut state), "测侧");
    import_rows(&mut runtime, 1, &[("测侧", "xi'xi", 9_000_000)]);
    assert_eq!(sentence(&runtime, &mut state), "侧测");
}

// Basis: Frozen tie/user-n observation.
#[test]
fn user_rows_for_an_incomplete_key_follow_the_stored_key_order() {
    let user = TempUserDir::new("tie-user-n");
    let mut runtime = Runtime::open(&system_dir(), Some(&user.path)).unwrap();

    let phrases = [
        ("𦰡", "na"),
        ("𫐐", "ni"),
        ("𫠜", "ni"),
        ("𨺙", "ni"),
        ("𫔶", "nie"),
        ("𬪩", "nong"),
        ("𦰡", "nuo"),
        ("𫟅", "nen"),
        ("𫵷", "nao"),
        ("𬇕", "nu"),
    ];

    let rows: Vec<_> = phrases
        .iter()
        .map(|&(text, reading)| (text, reading, 1))
        .collect();
    import_rows(&mut runtime, 7, &rows);
    let mut state = native_session(&runtime, OptionBits::from_bits_retain(0x18a));
    runtime.session(&mut state).replace_raw("n").unwrap();
    assert!(runtime.session(&mut state).guess_sentence().unwrap());
    let user: Vec<_> = runtime
        .session(&mut state)
        .candidates_at_for_abi(0, false)
        .unwrap()
        .iter()
        .filter(|row| phrases.iter().any(|&(phrase, _)| row.text() == phrase))
        .map(|row| {
            assert_eq!(row.kind(), crate::engine::CandidateKind::Phrase);
            row.text().as_str().unwrap().to_owned()
        })
        .collect();
    assert_eq!(user, ["𦰡", "𫵷", "𫟅", "𬪩", "𫐐", "𫠜", "𨺙", "𫔶", "𬇕"]);
}

// Basis: novel_types.h:41-44 and phrase_index.cpp:154,186 at 074a2219.
#[test]
fn unigram_get_and_add_ignore_the_token_tag() {
    let user = TempUserDir::new("tagged-unigram");
    let mut runtime = Runtime::open(&system_dir(), Some(&user.path)).unwrap();
    import_rows(&mut runtime, 7, &[("龘龘", "da2'da2", 5)]);
    let tokens = [
        runtime
            .dict()
            .tokens_for_ucs4(&"你".chars().map(u32::from).collect::<Vec<_>>())[0],
        runtime
            .user_store()
            .token_for_phrase("龘龘")
            .unwrap()
            .unwrap(),
    ];
    for token in tokens {
        let base = runtime.lm().unigram_freq(&token).unwrap().unwrap();
        let tagged = token | 0xf000_0000;
        assert_eq!(runtime.lm().unigram_freq(&tagged).unwrap(), Some(base));
        assert!(runtime.add_unigram_delta(tagged, 1));
        assert_eq!(runtime.lm().unigram_freq(&token).unwrap(), Some(base + 1));
    }
}

// Basis: phrase_lookup.cpp:310,329 and phonetic_lookup.h:646,674 at 074a2219.
#[test]
fn a_user_file_token_without_its_item_is_skipped_and_a_zero_item_is_not() {
    let law = UserConfLaw::Pinyin;
    let user = TempUserDir::new("stale-user-file-token");
    let mut runtime = Runtime::open_with_law(&system_dir(), Some(&user.path), law).unwrap();
    import_rows(&mut runtime, 7, &[("龘", "ni", 0), ("龘", "ni", 1)]);
    let token = runtime
        .dict()
        .tokens_for_ucs4(&"龘".chars().map(u32::from).collect::<Vec<_>>())
        .iter()
        .find(|token| **token >> 24 == 7)
        .copied()
        .unwrap();
    runtime.user_store_mut().save_reporting().unwrap();
    drop(runtime);
    let file = user.path.join("user.bin");
    let original = std::fs::read(&file).unwrap();
    let gram = gram_bytes(100, &[(token, 100)]);
    let empty = crate::data::phrase_library::build_chunk(&[]).unwrap();
    for present in [false, true] {
        std::fs::write(&file, if present { &original } else { &empty }).unwrap();
        native_fixture::plant_bigram(&user.path, &[(1_u32.to_le_bytes().to_vec(), gram.clone())]);
        let runtime = Runtime::open_with_law(&system_dir(), Some(&user.path), law).unwrap();
        assert_eq!(
            runtime
                .dict()
                .tokens_for_ucs4(&"龘".chars().map(u32::from).collect::<Vec<_>>())
                .to_vec(),
            [token]
        );
        let mut state = native_session(&runtime, OptionBits::USE_TONE);
        let (matched, tokens) = runtime.session(&mut state).phrase_segment("龘").unwrap();
        assert_eq!(
            (matched, tokens.to_vec()),
            (present, vec![if present { token } else { 0 }])
        );
        if present {
            assert_eq!(runtime.lm().unigram_freq(&token).unwrap(), Some(0));
        }
        runtime.session(&mut state).replace_raw("ni").unwrap();
        assert!(runtime.session(&mut state).guess_sentence().unwrap());
        assert_eq!(
            runtime.session(&mut state).sentence_text(0).unwrap(),
            if present { "龘" } else { "你" }
        );
    }
}

// Basis: novel_types.h:41-44 and phrase_index.cpp:154,186 at 074a2219.
#[test]
fn the_phrase_and_reading_getters_ignore_the_high_bits_of_a_token() {
    let user = TempUserDir::new("tagged-getters");
    let mut runtime = Runtime::open(&system_dir(), Some(&user.path)).unwrap();
    import_rows(&mut runtime, 7, &[("龘龘", "da2'da2", 5)]);
    runtime.user_store_mut().save_reporting().unwrap();
    let dada = runtime
        .dict()
        .tokens_for_ucs4(&"龘龘".chars().map(u32::from).collect::<Vec<_>>())[0];
    drop(runtime);
    let runtime = Runtime::open(&system_dir(), Some(&user.path)).unwrap();
    let dict = runtime.dict();
    for (token, text) in [
        (
            dict.tokens_for_ucs4(&"你".chars().map(u32::from).collect::<Vec<_>>())[0],
            "你",
        ),
        (dada, "龘龘"),
    ] {
        let count = dict.token_pronunciation_count(token).unwrap();
        assert_eq!(
            dict.token_pronunciation_count(token | 0x8000_0000),
            Some(count)
        );
        assert_eq!(dict.token_phrase(token).unwrap().text.unwrap(), text);
        assert_eq!(
            dict.token_phrase(token | 0x8000_0000)
                .unwrap()
                .text
                .unwrap(),
            text
        );
        let reading = |token| {
            dict.with_user_nth_pronunciation_words(token, 0, |length, words| {
                (length, words.unwrap().to_vec())
            })
            .or_else(|| {
                dict.token_nth_pronunciation(token, 0).map(|reading| {
                    (
                        reading.phrase_length,
                        reading
                            .keys
                            .unwrap()
                            .into_iter()
                            .flat_map(u16::to_le_bytes)
                            .collect(),
                    )
                })
            })
            .unwrap()
        };
        assert_eq!(reading(token | 0x8000_0000), reading(token));
    }
}

// Basis: phonetic_lookup.h:856-930 and novel_types.h:41-44 at 074a2219.
#[test]
fn a_candidate_under_high_bits_is_listed_and_trains_its_item() {
    const TAG: u32 = 0x8000_0000;
    let user = TempUserDir::new("tagged-candidate");
    let mut runtime = Runtime::open(&system_dir(), Some(&user.path)).unwrap();
    import_rows(&mut runtime, 7, &[("龘龘", "da2'da2", 5)]);
    runtime.user_store_mut().save_reporting().unwrap();
    let token = runtime
        .dict()
        .tokens_for_ucs4(&"龘龘".chars().map(u32::from).collect::<Vec<_>>())[0];
    drop(runtime);
    let path = user.path.join("user_pinyin_index.bin");
    let mut rows = native_fixture::store_rows(&path, false);
    let mut retagged = 0;
    for (_, value) in &mut rows {
        for at in 0..value.len().saturating_sub(3) {
            if value[at..at + 4] == token.to_le_bytes() {
                value[at..at + 4].copy_from_slice(&(token | TAG).to_le_bytes());
                retagged += 1;
            }
        }
    }
    assert!(retagged > 0, "the index holds the token");
    native_fixture::craft_user_index(&path, &rows);

    let mut runtime = Runtime::open(&system_dir(), Some(&user.path)).unwrap();
    let base = runtime.lm().unigram_freq(&token).unwrap().unwrap();
    let mut state = native_session(&runtime, OptionBits::USE_TONE);
    runtime.session(&mut state).replace_raw("dada").unwrap();
    let candidate = runtime
        .session(&mut state)
        .candidates_at_for_abi(0, false)
        .unwrap()
        .iter()
        .find(|row| {
            row.kind() == crate::engine::CandidateKind::Phrase && row.token() == Some(token | TAG)
        })
        .unwrap()
        .clone();
    runtime
        .session(&mut state)
        .select_target(&crate::engine::SelectionTarget::of_candidate(
            &candidate, 0, None,
        ))
        .unwrap();
    assert!(runtime.session(&mut state).guess_sentence().unwrap());
    assert!(runtime.train_session(&state, 0).unwrap());
    assert_eq!(runtime.lm().unigram_freq(&token).unwrap(), Some(base + 483));
    assert_eq!(
        runtime.user_store().bigram_successors(1).unwrap(),
        [(token | TAG, 69)]
    );
}

// Basis: pinyin.cpp:2684 at 074a2219.
#[test]
fn train_session_aborts_past_the_nonempty_results() {
    crate::store::death_test::assert_aborts(
        "runtime::tests::train_session_aborts_past_the_nonempty_results",
        || {
            let user = TempUserDir::new("train-abort-index");
            let mut runtime = Runtime::open(&system_dir(), Some(&user.path)).unwrap();
            let mut state = native_session(&runtime, OptionBits::USE_TONE);
            runtime.session(&mut state).replace_raw("nihao").unwrap();
            assert!(runtime.session(&mut state).guess_sentence().unwrap());
            let _ = runtime.train_session(&state, 9);
        },
    );
}

#[cfg(feature = "tkrzw")]
fn system_items(name: &str) -> (u32, std::collections::BTreeMap<u32, Vec<u8>>) {
    let bytes =
        std::fs::read(contract_data::contract_data_dir().join(name)).expect("a system library");
    let payload = &bytes[8..];
    let word = |at: usize| u32::from_le_bytes(payload[at..at + 4].try_into().expect("a word"));
    let (total, index, content) = (word(0), word(4) as usize, word(8) as usize);
    let mut items = std::collections::BTreeMap::new();
    for slot in 0..(content - index - 1) / 4 {
        let offset = word(index + slot * 4);
        if offset != 0 {
            let start = content + offset as usize;
            let (length, prons) = (usize::from(payload[start]), usize::from(payload[start + 1]));
            let size = 6 + 4 * length + prons * (2 * length + 4);
            let end = (start + size).min(payload.len());
            items.insert(
                u32::try_from(slot).expect("a slot"),
                payload[start..end].to_vec(),
            );
        }
    }
    (total, items)
}

#[cfg(feature = "tkrzw")]
fn item_readings(item: &[u8]) -> Vec<&[u8]> {
    let (length, prons) = (usize::from(item[0]), usize::from(item[1]));
    let start = 6 + 4 * length;
    let stride = 2 * length + 4;
    (0..prons)
        .map(|nth| {
            let at = (start + stride * nth).min(item.len());
            &item[at..(at + 2 * length).min(item.len())]
        })
        .collect()
}

#[cfg(feature = "tkrzw")]
// Basis: phrase_index.cpp:150-205 at 074a2219.
#[test]
fn a_journal_removed_item_adds_no_candidate() {
    let (total, items) = system_items("merged.bin");
    let mut longer = std::collections::BTreeSet::new();
    for name in ["merged.bin", "opengram.bin", "gb_char.bin", "gbk_char.bin"] {
        for item in system_items(name).1.values() {
            if item[0] > 4 {
                longer.extend(
                    item_readings(item)
                        .iter()
                        .map(|reading| reading[..8.min(reading.len())].to_vec()),
                );
            }
        }
    }
    let (slot, item) = items
        .iter()
        .find(|(_, item)| item[0] == 4 && item[1] == 1 && !longer.contains(item_readings(item)[0]))
        .expect("a four-character item no longer phrase extends");
    let token = 0x0400_0000 | slot;
    let user = TempUserDir::new("removed-candidate");

    let runtime = Runtime::open(&contract_data::contract_data_dir(), Some(&user.path)).unwrap();
    let phrase = runtime.dict().token_phrase(token).unwrap().text.unwrap();
    let reading: String = item_readings(item)[0]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|bytes| {
            crate::parser::ChewingKey::from_packed(u16::from_le_bytes(*bytes)).pinyin_string()
        })
        .collect();
    drop(runtime);
    native_fixture::write_user_conf(&user.path, &contract_data::contract_data_dir());
    let mut remove = 2_u32.to_le_bytes().to_vec();
    remove.extend(token.to_le_bytes());
    remove.extend(u16::try_from(item.len()).expect("a length").to_le_bytes());
    remove.extend(item);
    let log = [native_fixture::modify_header(0, total, total), remove].concat();
    std::fs::write(
        user.path.join("merged.dbin"),
        crate::data::chunk_format::build_memory_chunk(&log).expect("frame"),
    )
    .expect("write the journal");

    let runtime = Runtime::open(&contract_data::contract_data_dir(), Some(&user.path)).unwrap();
    assert!(runtime.dict().token_phrase(token).is_none());
    let mut state = native_session(&runtime, OptionBits::USE_TONE);
    runtime.session(&mut state).replace_raw(&reading).unwrap();
    assert_eq!(runtime.session(&mut state).full_parsed_len(), reading.len());
    assert!(runtime.session(&mut state).guess_sentence().unwrap());
    assert!(
        !normal_texts(&runtime, &mut state)
            .iter()
            .any(|text| phrase == text.as_str())
    );
}

#[cfg(feature = "tkrzw")]
fn native_rows(
    runtime: &Runtime,
    state: &mut crate::engine::SessionState,
    offset: usize,
) -> Vec<(crate::engine::CandidateKind, String, Option<u8>)> {
    runtime.session(state).set_sort_options(0x1e).unwrap();
    runtime
        .session(state)
        .candidates_at_for_abi(offset, false)
        .unwrap()
        .iter()
        .map(|row| {
            (
                row.kind(),
                row.text().as_str().unwrap().to_owned(),
                row.nbest_row(),
            )
        })
        .collect()
}
#[cfg(feature = "tkrzw")]
fn choose_native(
    runtime: &Runtime,
    state: &mut crate::engine::SessionState,
    offset: usize,
    index: usize,
) -> usize {
    runtime.session(state).set_sort_options(0x1e).unwrap();
    let row = runtime
        .session(state)
        .candidates_at_for_abi(offset, false)
        .unwrap()
        .iter()
        .nth(index)
        .unwrap()
        .clone();
    let target = crate::engine::SelectionTarget::of_candidate(&row, offset, None);
    let end = target.span_end;
    assert_ne!(
        runtime.session(state).select_target(&target).unwrap(),
        crate::engine::Selection::Refused
    );
    end
}

#[cfg(feature = "tkrzw")]
// Basis: pinyin.cpp:1711-1758,2200-2215 at 074a2219.
#[test]
fn dynamic_adjust_ranks_the_window_after_a_chosen_token() {
    use crate::engine::CandidateKind::{Phrase as NORMAL, Sentence as NBEST};
    let row = |kind, text: &str, rank| (kind, text.to_owned(), rank);
    for (options, expected) in [
        (
            0x18a,
            [
                "矣", "移", "蛇", "遗", "毅", "疑", "佚", "驿", "姨", "贻", "轶", "薏", "翌", "饴",
            ],
        ),
        (
            0x38a,
            [
                "遗", "矣", "移", "蛇", "疑", "毅", "驿", "佚", "贻", "姨", "薏", "轶", "饴", "翌",
            ],
        ),
    ] {
        let user = TempUserDir::new("dynamic-chosen-de");
        let runtime = Runtime::open(&contract_data::contract_data_dir(), Some(&user.path)).unwrap();
        let mut state = native_session(&runtime, OptionBits::from_bits_retain(options));
        runtime.session(&mut state).replace_raw("deyi").unwrap();
        assert_eq!(runtime.session(&mut state).full_parsed_len(), 4);
        assert!(runtime.session(&mut state).guess_sentence().unwrap());
        let initial = native_rows(&runtime, &mut state, 0);
        assert_eq!(initial.len(), 33);
        assert_eq!(initial[6], row(NORMAL, "的", None));
        assert_eq!(choose_native(&runtime, &mut state, 0, 6), 2);
        assert!(runtime.session(&mut state).guess_sentence().unwrap());
        let rows = native_rows(&runtime, &mut state, 2);
        assert_eq!(rows.len(), 599);
        assert_eq!(
            &rows[..3],
            &[
                row(NBEST, "的一", Some(0)),
                row(NBEST, "的以", Some(2)),
                row(NORMAL, "一", None)
            ]
        );
        for (index, text) in [23, 24, 25, 26, 32, 33, 38, 39, 52, 53, 70, 71, 75, 76]
            .into_iter()
            .zip(expected)
        {
            assert_eq!(
                rows[index],
                row(NORMAL, text, None),
                "options {options:#x}, row {index}"
            );
        }
    }
}

#[cfg(feature = "tkrzw")]
// Basis: Frozen nbest-train/ordinary observation.
#[test]
fn ordinary_training_reorders_the_nbest_rows() {
    use crate::engine::CandidateKind::{Phrase as NORMAL, Sentence as NBEST};
    let row = |kind, text: &str, rank| (kind, text.to_owned(), rank);
    let user = TempUserDir::new("pinned-nbest-train");
    let mut runtime = Runtime::open(&contract_data::contract_data_dir(), Some(&user.path)).unwrap();
    let mut state = native_session(&runtime, OptionBits::USE_TONE);
    let surface = |runtime: &Runtime, state: &mut crate::engine::SessionState| {
        runtime.session(state).replace_raw("nihao").unwrap();
        assert!(runtime.session(state).guess_sentence().unwrap());
        let rows = native_rows(runtime, state, 0);
        let mut indexes: Vec<_> = rows.iter().filter_map(|row| row.2).collect();
        indexes.sort_unstable();
        let sentences: Vec<_> = indexes
            .iter()
            .take(3)
            .map(|&index| {
                runtime
                    .session(state)
                    .sentence_text(index)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect();
        runtime.session(state).reset();
        (
            rows.len(),
            rows.into_iter().take(8).collect::<Vec<_>>(),
            sentences,
        )
    };
    let train = |runtime: &mut Runtime,
                 state: &mut crate::engine::SessionState,
                 first: &str,
                 second: &str| {
        runtime.session(state).replace_raw("nihao").unwrap();
        assert!(runtime.session(state).guess_sentence().unwrap());
        for (offset, text, end) in [(0, first, 2), (2, second, 5)] {
            let index = native_rows(runtime, state, offset)
                .iter()
                .position(|row| row.1 == text)
                .unwrap();
            assert_eq!(choose_native(runtime, state, offset, index), end);
            assert!(runtime.session(state).guess_sentence().unwrap());
        }
        assert!(runtime.train_session(state, 0).unwrap());
        runtime.session(state).reset();
    };
    let normal = |texts: &[&str]| {
        texts
            .iter()
            .map(|&text| row(NORMAL, text, None))
            .collect::<Vec<_>>()
    };
    let mut baseline = vec![row(NBEST, "你好", Some(0))];
    baseline.extend(normal(&["你", "尼", "呢", "泥", "妮", "拟", "逆"]));
    assert_eq!(
        surface(&runtime, &mut state),
        (126, baseline, vec!["你好".to_owned()])
    );
    for _ in 0..3 {
        train(&mut runtime, &mut state, "你", "浩");
    }
    let mut user_only = vec![row(NBEST, "你浩", Some(0)), row(NBEST, "你好", Some(1))];
    user_only.extend(normal(&["你", "尼", "呢", "泥", "妮", "拟"]));
    assert_eq!(
        surface(&runtime, &mut state),
        (127, user_only, vec!["你浩".to_owned(), "你好".to_owned()])
    );
    for _ in 0..3 {
        train(&mut runtime, &mut state, "你", "好");
    }
    let mut augmented = vec![row(NBEST, "你好", Some(0)), row(NBEST, "你浩", Some(1))];
    augmented.extend(normal(&["你", "尼", "呢", "泥", "妮", "拟"]));
    let trained = (127, augmented, vec!["你好".to_owned(), "你浩".to_owned()]);
    assert_eq!(surface(&runtime, &mut state), trained);

    assert!(
        runtime
            .user_store()
            .phrases()
            .iter()
            .all(|phrase| crate::vocab::phrase_index_library_index(phrase.token()) != 7)
    );
    runtime.user_store_mut().save_reporting().unwrap();
    drop(runtime);
    let runtime = Runtime::open(&contract_data::contract_data_dir(), Some(&user.path)).unwrap();
    let mut state = native_session(&runtime, OptionBits::USE_TONE);
    assert_eq!(
        surface(&runtime, &mut state),
        trained,
        "after a save and reopen"
    );
}

use crate::data::support as chunk_fixture;
#[path = "../../tests/support/unreadable_fixture.rs"]
mod unreadable_fixture;

// Basis: Register row 80, unreadable candidate item.
#[test]
fn a_user_table_fallback_reports_its_unreadable_row() {
    let (system, user) = unreadable_fixture::dirs(unreadable_fixture::Case::Fallback);
    let runtime =
        Runtime::open_with_law(system.path(), Some(user.path()), UserConfLaw::Zhuyin).unwrap();
    let mut state = native_session(&runtime, OptionBits::USE_TONE | OptionBits::FORCE_TONE);
    native_chewing(&runtime, &mut state, "su3");
    runtime
        .session(&mut state)
        .set_candidate_ranking(crate::engine::CandidateRanking::LengthThenFrequency);
    let rows = runtime
        .session(&mut state)
        .candidates_at_for_abi(0, false)
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows.had_unreadable_item());
    assert!(rows.iter().all(|row| row.token().is_none()));
    assert!(runtime.dict().token_phrase(0x0100_0010).is_none());
}

// Basis: Rust contract: extension lookup reaches only visible library rows.
#[test]
fn a_library_mask_does_not_read_extension_rows() {
    for (unload, expected) in [(false, 1), (true, 0)] {
        let (system, user) = unreadable_fixture::dirs(unreadable_fixture::Case::MaskedExtension);
        let mut runtime =
            Runtime::open_with_law(system.path(), Some(user.path()), UserConfLaw::Zhuyin).unwrap();
        if unload {
            assert!(runtime.unload_library(2));
        }
        let mut state = native_session(&runtime, OptionBits::USE_TONE | OptionBits::FORCE_TONE);
        native_chewing(&runtime, &mut state, "su3");
        runtime
            .session(&mut state)
            .set_candidate_ranking(crate::engine::CandidateRanking::LengthThenFrequency);
        let rows = runtime
            .session(&mut state)
            .candidates_at_for_abi(0, false)
            .unwrap();
        assert_eq!(
            usize::from(rows.had_unreadable_item()),
            expected,
            "unload {unload}"
        );
        assert!(rows.iter().all(|row| row.token().is_none()));
    }
}

fn ucs4_bytes(text: &str) -> Vec<u8> {
    text.chars()
        .flat_map(|word| u32::from(word).to_le_bytes())
        .collect()
}

// Basis: pinyin.cpp:2371-2405 at 074a2219.
#[test]
fn a_raw_user_key_predicts_its_live_item() {
    let user = TempUserDir::new("raw-key-prediction");
    let mut runtime = Runtime::open(&system_dir(), Some(&user.path)).unwrap();
    import_rows(&mut runtime, 7, &[("龘龘", "da2'da2", 5)]);
    runtime.user_store_mut().save_reporting().unwrap();
    let token = runtime
        .dict()
        .tokens_for_ucs4(&"龘龘".chars().map(u32::from).collect::<Vec<_>>())[0];
    drop(runtime);
    native_fixture::craft_user_index(
        &user.path.join("user_phrase_index.bin"),
        &[
            (ucs4_bytes("你"), Vec::new()),
            (
                [ucs4_bytes("你"), 0x0011_0000_u32.to_le_bytes().to_vec()].concat(),
                token.to_le_bytes().to_vec(),
            ),
        ],
    );
    native_fixture::write_user_conf(&user.path, &system_dir());
    let runtime = Runtime::open(&system_dir(), Some(&user.path)).unwrap();
    let user_rows = runtime
        .user_store()
        .lookup()
        .suggest_after(&[u32::from('你')]);
    let system = runtime
        .dict()
        .system()
        .suggestion_tokens(&[u32::from('你')])
        .unwrap();
    assert!(merge_suggestion_rows(&system, &user_rows).contains(&token));
    assert_eq!(
        runtime.dict().token_phrase(token).unwrap().text.unwrap(),
        "龘龘"
    );
}

// Basis: phrase_index.h:762-772 at 074a2219.
#[test]
fn rows_of_a_never_loaded_library_are_filtered() {
    let user = TempUserDir::new("never-loaded-library");
    let token = crate::vocab::phrase_index_make_token(15, 1);
    let value = token.to_le_bytes().to_vec();
    native_fixture::craft_user_index(
        &user.path.join("user_phrase_index.bin"),
        &[
            (ucs4_bytes("你"), Vec::new()),
            ([ucs4_bytes("你"), ucs4_bytes("龘")].concat(), value.clone()),
            (ucs4_bytes("龘龘"), value),
        ],
    );
    native_fixture::write_user_conf(&user.path, &system_dir());
    let runtime = Runtime::open(&system_dir(), Some(&user.path)).unwrap();
    assert!(
        runtime
            .dict()
            .tokens_for_ucs4(&"龘龘".chars().map(u32::from).collect::<Vec<_>>())
            .is_empty()
    );
    assert!(!runtime.dict().phrase_table_token_enabled(token));
    let suggestions = runtime
        .user_store()
        .lookup()
        .suggest_after(&[u32::from('你')]);
    assert_eq!(suggestions.len(), 1);
    assert!(
        suggestions
            .iter()
            .flat_map(|(_, tokens)| tokens)
            .all(|token| !runtime.dict().phrase_table_token_enabled(*token))
    );
}

// Basis: pinyin.cpp:2371-2405 at 074a2219.
#[test]
fn predicted_tie_groups_keep_library_then_key_order_including_user_rows() {
    let user = TempUserDir::new("pred-defined-order");
    let mut runtime = Runtime::open(&system_dir(), Some(&user.path)).unwrap();
    import_rows(
        &mut runtime,
        7,
        &[
            ("中", "zhong", 9),
            ("中年", "zhongnian", 7),
            ("中华", "zhonghua", 7),
            ("中型", "zhongxing", 7),
            ("中文", "zhongwen", 7),
        ],
    );
    let prefix = [u32::from('中')];
    let system = runtime.dict().system().suggestion_tokens(&prefix).unwrap();
    let user_rows = runtime.user_store().lookup().suggest_after(&prefix);
    let tokens = merge_suggestion_rows(&system, &user_rows);
    let dict = runtime.dict();
    let rows: Vec<_> = tokens
        .iter()
        .map(|&token| {
            let text = dict.token_text(token).unwrap();
            (
                crate::vocab::phrase_index_library_index(token),
                ucs4_bytes(text.as_str().unwrap()),
                text,
            )
        })
        .collect();
    for pair in rows.windows(2) {
        assert!((&pair[0].0, &pair[0].1) <= (&pair[1].0, &pair[1].1));
    }
    let user_texts: Vec<_> = rows
        .iter()
        .filter(|row| row.0 == 7)
        .map(|row| row.2.as_str().unwrap())
        .collect();
    assert_eq!(user_texts, ["中华", "中年", "中文", "中型"]);
}

/// A trellis step's item read: the ordinary live item in place, every other
/// origin through the full resolution, both pricing alike.
mod step_item {
    use super::*;

    fn fixture() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/w3")
            .join(crate::__internal::DEFAULT_STORE_EXT)
    }
    fn opened() -> Runtime {
        Runtime::open(&fixture(), None).unwrap()
    }
    fn token(runtime: &Runtime) -> u32 {
        runtime.dict().system().tokens_for_text("你").unwrap()[0]
    }
    fn path(runtime: &Runtime, spelling: &str) -> PathPricing {
        runtime
            .dict()
            .path_pricing(&[SyllableKey::from_text(spelling).unwrap()], &[])
    }
    /// The step's read and the full resolution price `token` alike on the
    /// path `spelling`; the step's read answers the unigram it found the
    /// item with. The trellis step's own read, through a path memo, answers
    /// the item the text read does.
    fn price(runtime: &Runtime, spelling: &str, token: u32) -> Option<u32> {
        let path = path(runtime, spelling);
        let old = runtime.dict().priced_on_path(&path, token, true);
        let new = runtime.dict().priced(&path, token, true);
        assert_eq!(
            old.as_ref()
                .map(|p| (p.item.chars, p.item.pronunciation, p.text.clone())),
            new.as_ref()
                .map(|p| (p.item.chars, p.item.pronunciation, p.text.clone()))
        );
        let step = runtime
            .dict()
            .trellis_step_item(
                &[SyllableKey::from_text(spelling).unwrap()],
                &[],
                token,
                &crate::model::TrellisPathMemo::default(),
            )
            .unwrap();
        assert_eq!(step, new.as_ref().map(|p| p.item));
        new.and_then(|p| p.item.unigram)
    }
    fn score(runtime: &Runtime, token: u32, unigram: Option<u32>) {
        let lm = runtime.lm();
        let old = lm.nbest_token_costs(&token).unwrap();
        let total = Cell::new(None);
        let new = lm.nbest_token_costs_read(&token, unigram, &total).unwrap();
        assert_eq!(total.get().is_some(), unigram.is_some());
        let bits = |costs: crate::model::NbestTokenCosts| {
            (
                costs.elem.map(f64::to_bits),
                costs.lambda.to_bits(),
                costs.unigram_lambda.to_bits(),
            )
        };
        assert_eq!(bits(old), bits(new));
        for item in [None, Some((3_u32, 7_u32)), Some((5, 7))] {
            let transition = item.map(|(freq, total)| (u64::from(freq), u64::from(total), true));
            let old_step = old.costs(transition);
            let new_step = new.costs(transition);
            assert_eq!(
                (
                    old_step.blended.map(f64::to_bits),
                    old_step.unigram.map(f64::to_bits),
                    old_step.unigram_lambda.to_bits()
                ),
                (
                    new_step.blended.map(f64::to_bits),
                    new_step.unigram.map(f64::to_bits),
                    new_step.unigram_lambda.to_bits()
                )
            );
            // The n-best step reads each branch from the token costs.
            assert_eq!(
                item.and_then(|(freq, total)| new.blended(u64::from(freq), u64::from(total)))
                    .map(f64::to_bits),
                new_step.blended.map(f64::to_bits)
            );
            assert_eq!(
                new.unigram().map(f64::to_bits),
                new_step.unigram.map(f64::to_bits)
            );
        }
    }

    // Basis: phonetic_lookup.h:643-697 at 074a2219 reads one item while each path supplies its pronunciation.
    #[test]
    fn a_step_reads_the_live_item_in_place_on_each_path() {
        let runtime = opened();
        let token = token(&runtime);
        for spelling in ["ni", "hao"] {
            let unigram = price(&runtime, spelling, token);
            assert!(unigram.is_some(), "the ordinary live item is read in place");
            score(&runtime, token, unigram);
        }
        let ni = runtime
            .dict()
            .priced(&path(&runtime, "ni"), token, false)
            .unwrap();
        let hao = runtime
            .dict()
            .priced(&path(&runtime, "hao"), token, false)
            .unwrap();
        assert_ne!(ni.item.pronunciation, hao.item.pronunciation);
    }

    // Basis: each sentence invocation reads the committed training counts (Rust contract).
    #[test]
    fn next_guess_reads_committed_training() {
        let mut runtime = opened();
        let token = token(&runtime);
        let before = price(&runtime, "ni", token).unwrap();
        runtime
            .user_store_mut()
            .observe_selection(1, token)
            .unwrap();
        let after = price(&runtime, "ni", token).unwrap();
        assert!(after > before);
        score(&runtime, token, Some(after));
    }

    fn fallback(runtime: &Runtime, token: u32) {
        assert!(
            runtime
                .dict()
                .read_in_place(&path(runtime, "ni"), token, |_| ())
                .is_none(),
            "the full resolution answers"
        );
        let unigram = price(runtime, "ni", token);
        assert!(unigram.is_none());
        score(runtime, token, unigram);
    }

    fn replayed(
        change: impl FnOnce(u32, Vec<u8>) -> crate::data::user_files::LogRecord,
    ) -> (tempfile::TempDir, Runtime, u32) {
        let runtime = opened();
        let token = token(&runtime);
        let old = runtime
            .dict()
            .live_items()
            .read(token, |item| item.as_bytes().to_vec())
            .found()
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let payload = crate::data::user_files::encode_log_records(&[change(
            token & crate::vocab::PHRASE_MASK,
            old,
        )])
        .unwrap();
        std::fs::write(
            dir.path().join("gb_char.dbin"),
            crate::data::chunk_format::build_memory_chunk(&payload).unwrap(),
        )
        .unwrap();
        assert!(
            crate::data::user_files::UserTableInfo::conform_to(&SystemVersions::from_conf(
                runtime.table_conf(),
            ))
            .save(dir.path().join("user.conf").as_path())
        );
        drop(runtime);
        let runtime = Runtime::open(&fixture(), Some(dir.path())).unwrap();
        (dir, runtime, token)
    }
    #[derive(Clone, Copy)]
    enum Fallback {
        User,
        Imported,
        Missing,
        Removed,
        Unreadable,
        Unloaded,
    }

    // Basis: the step reads ordinary live items in place; a user item comes first, and other origins, failed reads and an unloaded library keep the full resolution (Rust contract; phrase_index.h:646-650 at 074a2219).
    #[rstest::rstest]
    #[case::user(Fallback::User)]
    #[case::imported(Fallback::Imported)]
    #[case::missing(Fallback::Missing)]
    #[case::removed(Fallback::Removed)]
    #[case::unreadable(Fallback::Unreadable)]
    #[case::unloaded(Fallback::Unloaded)]
    fn other_origins_keep_the_full_resolution(#[case] case: Fallback) {
        match case {
            Fallback::Unloaded => {
                let mut runtime = opened();
                let token = token(&runtime);
                assert!(
                    runtime
                        .unload_library(u32::from(crate::vocab::phrase_index_library_index(token)))
                );
                fallback(&runtime, token);
            }
            Fallback::User | Fallback::Imported => {
                let mut runtime = opened();
                let ni = SyllableKey::from_text("ni").unwrap();
                let keys = [crate::user::toned_key(ni.index(), 0).unwrap()];
                let store = runtime.user_store_mut();
                let token = if matches!(case, Fallback::User) {
                    store.add_phrase_in(7, &"词".into(), &keys, Some(5))
                } else {
                    store.add_system_phrase_in(1, None, &"龘".into(), &keys, Some(0))
                }
                .unwrap();
                fallback(&runtime, token);
            }
            Fallback::Missing => fallback(&opened(), 0x01ff_fffe),
            Fallback::Removed | Fallback::Unreadable => {
                use crate::data::user_files::LogRecord;
                let (_dir, runtime, token) = replayed(|token, old_item| {
                    if matches!(case, Fallback::Removed) {
                        return LogRecord::Remove { token, old_item };
                    }
                    let mut new_item = old_item.clone();
                    new_item[1] = 255;
                    new_item.push(0);
                    LogRecord::Modify {
                        token,
                        old_item,
                        new_item,
                    }
                });
                fallback(&runtime, token);
            }
        }
    }
}
