//! Data-table reads that fail or return a partial row.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use super::*;

/// A pinyin-table read that fails is a miss on every backend: the search
/// answers `SEARCH_NONE` for that key and the lookup goes on
/// (`chewing_large_table2_tkrzwdb.cpp:146-149`,
/// `chewing_large_table2_bdb.cpp:182-184`,
/// `chewing_large_table2_kyotodb.cpp:160-163` at 074a2219). With the read
/// of the `ni'hao` row failing, the guess still answers true and builds the
/// sentence from the rows of `ni` and `hao`.
#[test]
fn a_failed_pinyin_table_read_is_a_miss() {
    crate::test_support::run_in_child(
        "boundary_tests::table_reads::a_failed_pinyin_table_read_is_a_miss",
        |user| {
            let (context, instance) = open_at(user);
            let key = |text: &str| {
                let mut key = 0_u16;
                let text = cstr(text);
                assert!(unsafe {
                    pinyin_parse_full_pinyin(instance, text.as_ptr(), (&raw mut key).cast())
                });
                key.to_le_bytes()
            };
            // The complete index key: the packed keys, tones zero.
            let row = [key("ni"), key("hao")].concat();
            assert_eq!(parse(instance, "nihao"), 5);
            oxpinyin_store::read_faults::fail_point_read_of(&row, 0);
            assert!(unsafe { pinyin_guess_sentence(instance) });
            assert_eq!(sentence_out(instance, 0), (true, Some("你好".to_owned())));
            close(context, instance);
        },
    );
}

/// Kyoto Cabinet sizes a pinyin-table row with `check` and reads its body
/// under `check_result(vsiz == m_db->get(...))`, an assert
/// (`chewing_large_table2_kyotodb.cpp:159-176`, `pinyin_utils.h:27-31` at
/// 074a2219). With the check of the `ni` row good and its body read
/// failing, the sentence guess aborts.
#[cfg(feature = "kyotocabinet")]
#[test]
fn a_pinyin_table_body_read_failure_after_the_check_aborts() {
    assert_aborts(
        "boundary_tests::table_reads::a_pinyin_table_body_read_failure_after_the_check_aborts",
        || {
            let user = TempUserDir::new("kc-pinyin-body-read");
            let (context, instance) = open_at(&user.path);
            let mut key = 0_u16;
            let text = cstr("ni");
            assert!(unsafe {
                pinyin_parse_full_pinyin(instance, text.as_ptr(), (&raw mut key).cast())
            });
            assert_eq!(parse(instance, "nihao"), 5);
            // One read of the row passes: its size check.
            oxpinyin_store::read_faults::fail_point_read_of(&key.to_le_bytes(), 1);
            unsafe { pinyin_guess_sentence(instance) };
            close(context, instance);
        },
    );
}

/// A phrase-table read that fails is a miss: `PhraseLargeTable3::search`
/// answers `SEARCH_NONE` (`phrase_large_table3_tkrzwdb.cpp:136-139`,
/// `phrase_large_table3_bdb.cpp:196-198`,
/// `phrase_large_table3_kyotodb.cpp:174-177` at 074a2219). With the read
/// of the row of 测测 failing, `_add_phrase` finds no token for the phrase
/// and adds it (`pinyin.cpp:536-542`, `:588-608`).
#[test]
fn a_failed_phrase_table_read_is_a_miss() {
    crate::test_support::run_in_child(
        "boundary_tests::table_reads::a_failed_phrase_table_read_is_a_miss",
        |user| {
            let (context, instance) = open_at(user);
            let row: Vec<u8> = "测测"
                .chars()
                .flat_map(|word| u32::from(word).to_le_bytes())
                .collect();
            oxpinyin_store::read_faults::fail_point_read_of(&row, 0);
            assert_eq!(import_batch(context, 7, &[("测测", "ce4'ce4", 5)]), [true]);
            close(context, instance);
        },
    );
}

/// Kyoto Cabinet checks the phrase-table row size, answers a failed check
/// as a miss, and asserts on a failed body read after a successful check
/// (`phrase_large_table3_kyotodb.cpp:174-187`, `pinyin_utils.h:27-31` at
/// 074a2219). The existing 你好 row passes its size check; importing that
/// phrase must abort when its body read fails.
#[cfg(feature = "kyotocabinet")]
#[test]
fn a_phrase_table_body_read_failure_after_the_check_aborts() {
    assert_aborts(
        "boundary_tests::table_reads::a_phrase_table_body_read_failure_after_the_check_aborts",
        || {
            let user = TempUserDir::new("kc-phrase-body-read");
            let (context, instance) = open_at(&user.path);
            let row: Vec<u8> = "你好"
                .chars()
                .flat_map(|word| u32::from(word).to_le_bytes())
                .collect();
            oxpinyin_store::read_faults::fail_point_read_of(&row, 1);
            import_batch(context, 7, &[("你好", "ni3'hao3", 5)]);
            close(context, instance);
        },
    );
}

/// A failed existence read of a pinyin-table key is a miss too: the search
/// for that key answers `SEARCH_NONE` and the candidate lookup goes on
/// (`chewing_large_table2_tkrzwdb.cpp:146-149`, `:511` at 074a2219). With
/// every read of the `ni` row failing, the candidate guess still answers
/// true.
#[test]
fn a_failed_pinyin_table_existence_read_is_a_miss() {
    crate::test_support::run_in_child(
        "boundary_tests::table_reads::a_failed_pinyin_table_existence_read_is_a_miss",
        |user| {
            let (context, instance) = open_at(user);
            let mut key = 0_u16;
            let text = cstr("ni");
            assert!(unsafe {
                pinyin_parse_full_pinyin(instance, text.as_ptr(), (&raw mut key).cast())
            });
            assert_eq!(parse(instance, "nihao"), 5);
            oxpinyin_store::read_faults::fail_point_read_of(&key.to_le_bytes(), 0);
            assert!(unsafe {
                pinyin_guess_candidates(instance, 0, crate::test_support::DEFAULT_SORT)
            });
            close(context, instance);
        },
    );
}

/// A cursor read that fails ends the suggestion walk with the rows read
/// before it: the pin leaves its loop when the iterator's `Get` is not OK
/// (`chewing_large_table2_tkrzwdb.cpp:516-541`,
/// `phrase_large_table3_tkrzwdb.cpp:171-186` at 074a2219). With the read
/// of the `ni'hao` row failing in the walk from `ni`, the candidate guess
/// still answers true.
#[test]
fn a_failed_pinyin_table_cursor_read_ends_the_walk() {
    crate::test_support::run_in_child(
        "boundary_tests::table_reads::a_failed_pinyin_table_cursor_read_ends_the_walk",
        |user| {
            let (context, instance) = open_at(user);
            let key = |text: &str| {
                let mut key = 0_u16;
                let text = cstr(text);
                assert!(unsafe {
                    pinyin_parse_full_pinyin(instance, text.as_ptr(), (&raw mut key).cast())
                });
                key.to_le_bytes()
            };
            let row = [key("ni"), key("hao")].concat();
            assert_eq!(parse(instance, "ni"), 2);
            oxpinyin_store::read_faults::fail_point_read_of(&row, 0);
            // Sort word 0 keeps the longer-candidate row, which walks the
            // rows that extend `ni`.
            assert!(unsafe { pinyin_guess_candidates(instance, 0, 0) });
            close(context, instance);
        },
    );
}

/// The phrase table's suggestion walk ends at a cursor read that fails and
/// keeps the rows read before it (`phrase_large_table3_tkrzwdb.cpp:171-186`
/// at 074a2219). The walk from 你 reads its extensions in key order; with
/// the read of the second one failing, the first is still predicted.
#[test]
fn a_failed_phrase_table_cursor_read_keeps_the_rows_before_it() {
    crate::test_support::run_in_child(
        "boundary_tests::table_reads::a_failed_phrase_table_cursor_read_keeps_the_rows_before_it",
        |user| {
            let prefix: Vec<u8> = u32::from('你').to_le_bytes().to_vec();
            // The system phrase table's rows that extend the prefix, in
            // key order, read as the library attaches the file.
            let mut extensions: Vec<Vec<u8>> = Vec::new();
            {
                use oxpinyin_store::RawReadStore as _;
                let table = oxpinyin_store::DefaultStore::open_read_only(
                    &system_dir().join("phrase_index.bin"),
                )
                .expect("open the phrase table");
                table
                    .range_raw(
                        std::ops::Bound::Unbounded,
                        std::ops::Bound::Unbounded,
                        |key, value| {
                            if key.len() > prefix.len()
                                && key.starts_with(&prefix)
                                && !value.is_empty()
                            {
                                extensions.push(key.to_vec());
                            }
                            Ok(())
                        },
                    )
                    .expect("read the phrase table");
            }
            extensions.sort();
            assert!(extensions.len() >= 2, "two phrases extend the prefix");
            let text = |key: &[u8]| {
                key.as_chunks::<4>()
                    .0
                    .iter()
                    .map(|word| char::from_u32(u32::from_le_bytes(*word)).expect("a scalar"))
                    .collect::<String>()
            };
            let (context, instance) = open_at(user);
            oxpinyin_store::read_faults::fail_point_read_of(&extensions[1], 0);
            assert!(unsafe { pinyin_guess_predicted_candidates(instance, cstr("你").as_ptr()) });
            let rows: Vec<String> = capture_rows(instance)
                .into_iter()
                .map(|(_, text, _)| text)
                .collect();
            let first = text(&extensions[0]);
            let first = first.strip_prefix('你').expect("the prefix");
            assert!(rows.iter().any(|row| row == first), "{rows:?}");
            close(context, instance);
        },
    );
}

/// A pinyin-table row with a trailing partial record keeps its whole
/// records. The pin counts the `IndexItem`s between the row's begin and end
/// (`chewing_large_table2.h:145-150` at 074a2219), so a guess over a row of
/// one 8-byte record and 4 more bytes lists the record's 丙 and answers true
/// (compatibility policy row 128).
#[test]
fn a_misaligned_pinyin_table_row_keeps_its_whole_records() {
    let scratch = tempfile::tempdir().expect("scratch");
    let system = crate::test_support::items_system(
        &system_dir(),
        scratch.path(),
        &[(&[0x4e19], &[0x1081], 100)],
        100,
    );
    let mut row = 0x0100_0001_u32.to_le_bytes().to_vec();
    row.extend(0x1081_u16.to_le_bytes());
    row.extend([0; 2 + 4]);
    craft_tree(
        &system.join("pinyin_index.bin"),
        &[((0x1081_u16 & 4095).to_le_bytes().to_vec(), row)],
    );
    let user = TempUserDir::new("misaligned-pinyin-row");
    let context = init_at(&system, &user.path);
    assert!(!context.is_null());
    let instance = alloc(context);
    assert_eq!(parse(instance, "ba"), 2);
    assert!(unsafe { pinyin_guess_candidates(instance, 0, 0) });
    assert!(
        capture_rows(instance)
            .iter()
            .any(|(kind, text, _)| *kind == NORMAL && text == "丙")
    );
    close(context, instance);
}

/// A failed point read contributes neither rows nor continuation
/// (`chewing_large_table2_tkrzwdb.cpp:146-152`, `_bdb.cpp:182-187`,
/// `_kyotodb.cpp:160-166` at 074a2219). The candidate scan widens on
/// `SEARCH_NONE` (`pinyin.cpp:2231-2243`), so a failed `ni` search
/// drops 你 but the following `ni'hao` search still lists 你好.
#[test]
fn a_failed_candidate_table_read_keeps_searching_longer_spans() {
    crate::test_support::run_in_child(
        "boundary_tests::table_reads::a_failed_candidate_table_read_keeps_searching_longer_spans",
        |user| {
            let (context, instance) = open_at(user);
            let key = oxpinyin_core::ChewingKey::from_pinyin("ni")
                .expect("ni")
                .to_packed();
            assert_eq!(parse(instance, "nihao"), 5);
            let normal_rows = || {
                assert!(unsafe {
                    pinyin_guess_candidates(instance, 0, crate::test_support::DEFAULT_SORT)
                });
                capture_rows(instance)
                    .into_iter()
                    .filter_map(|(kind, text, _)| (kind == NORMAL).then_some(text))
                    .collect::<Vec<_>>()
            };
            let before = normal_rows();
            assert!(before.iter().any(|text| text == "你"));
            assert!(before.iter().any(|text| text == "你好"));
            oxpinyin_store::read_faults::fail_point_read_of(&key.to_le_bytes(), 0);
            let after = normal_rows();
            assert!(!after.iter().any(|text| text == "你"));
            assert!(after.iter().any(|text| text == "你好"));
            close(context, instance);
        },
    );
}

/// The pin ORs the default and addon searches before deciding to widen
/// (`pinyin.cpp:2231-2256`, `phonetic_key_matrix.cpp:359-373` at
/// 074a2219). The default `ba` read is a miss. The addon read finds 甲
/// and sets `SEARCH_CONTINUED`, so the scan reaches 乙丙 at `ba'ma`.
/// An extra read of `ba` would fail and lose that continuation bit.
#[test]
fn addon_candidate_continuation_uses_its_row_read() {
    use crate::test_support::{AddonItem, addon_system};

    crate::test_support::run_in_child(
        "boundary_tests::table_reads::addon_candidate_continuation_uses_its_row_read",
        |user| {
            let packed = |text| {
                oxpinyin_core::ChewingKey::from_pinyin(text)
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

            let context = init_at(&system, user);
            assert!(!context.is_null());
            assert!(unsafe { crate::config::pinyin_load_addon_phrase_library(context, 4) });
            let instance = alloc(context);
            assert_eq!(parse(instance, "bama"), 4);
            // One default get/check misses. The addon row needs one get,
            // or a check and a body read on Kyoto Cabinet.
            let reads = if cfg!(feature = "kyotocabinet") { 3 } else { 2 };
            oxpinyin_store::read_faults::fail_point_read_of(&short[0].to_le_bytes(), reads);
            assert!(unsafe {
                pinyin_guess_candidates(instance, 0, crate::test_support::DEFAULT_SORT)
            });
            let rows = capture_rows(instance);
            for expected in ["甲", "乙丙"] {
                assert!(
                    rows.iter().any(|(kind, text, _)| {
                        *kind == lookup_candidate_type_t::ADDON_CANDIDATE && text == expected
                    }),
                    "missing addon candidate {expected}: {rows:?}"
                );
            }
            close(context, instance);
        },
    );
}
