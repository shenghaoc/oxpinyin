use super::*;
use crate::data::phrase_library::PhraseLibrary;
use std::collections::BTreeMap;

fn item(phrase: &[u32], unigram: u32, prons: &[(Vec<u16>, u32)]) -> ChunkItem {
    ChunkItem {
        phrase: phrase.to_vec(),
        unigram,
        prons: prons.to_vec(),
    }
}

/// Basis: Rust contract (the `add_candidates` speed gate equals the plain slot walk).
#[test]
fn add_candidates_preserve_sparse_additions() {
    let original = library();
    let sparse_additions = |merge: &mut Merge| {
        for record in [
            LogRecord::Add {
                token: 1024,
                new_item: one(),
            },
            LogRecord::Add {
                token: 2048,
                new_item: one(),
            },
            LogRecord::Remove {
                token: 2048,
                old_item: one(),
            },
        ] {
            assert!(merge.apply_fixture(&record).is_ok());
        }
    };
    let modified = || {
        let mut merge = Merge::new(Some(original.view()));
        assert!(
            merge
                .apply_fixture(&modify(1, one(), with_freq(one(), 77)))
                .is_ok()
        );
        merge
    };
    let mut cases: Vec<(&str, Merge, &[u32])> = Vec::new();
    cases.push(("modify only", modified(), &[]));
    let mut replaced = Merge::new(Some(original.view()));
    assert!(
        replaced
            .apply_fixture(&LogRecord::Add {
                token: 1,
                new_item: with_freq(one(), 77),
            })
            .is_ok()
    );
    cases.push(("original-slot add", replaced, &[]));
    let mut sparse = modified();
    sparse_additions(&mut sparse);
    cases.push(("sparse", sparse, &[1024]));
    for (name, merge, added) in cases {
        let (_, _, view) = merge.finish_views(&mut None, &"test");
        let view = view.expect("retained originals");
        let expected: Vec<_> = (1..view.range_end())
            .filter(|&slot| !original.contains_item(slot) && view.item(slot).is_some())
            .collect();
        let actual: Vec<_> = view
            .add_candidates()
            .filter(|&slot| !original.contains_item(slot))
            .collect();
        assert_eq!(expected, added, "{name}");
        assert_eq!(actual, expected, "{name}");
        for &slot in added {
            assert_eq!(view.item(slot), decode_phrase_item(&one()), "{name}");
        }
    }
}

/// Slot 1 holds a one-reading item (unigram 100), slot 2 a two-reading one
/// (unigram 50): the library's total is their sum.
fn library() -> SystemLibrary {
    SystemLibrary {
        total: 150,
        range_end: 3,
        items: BTreeMap::from([
            (1, item(&[0x4f60], 100, &[(vec![1], 100)])),
            (2, item(&[0x597d], 50, &[(vec![2], 30), (vec![3], 20)])),
        ]),
        mapped: None,
    }
}

fn one() -> Vec<u8> {
    journal_item_bytes(&library().items[&1])
}

fn two() -> Vec<u8> {
    journal_item_bytes(&library().items[&2])
}

fn with_last(mut item: Vec<u8>, byte: u8) -> Vec<u8> {
    *item.last_mut().expect("item") = byte;
    item
}

/// A six-byte item: the two header bytes and a unigram.
fn bytes_with(header: &[u8; 2], unigram: u32) -> Vec<u8> {
    let mut bytes = header.to_vec();
    bytes.extend_from_slice(&unigram.to_le_bytes());
    bytes
}

fn with_freq(mut item: Vec<u8>, frequency: u32) -> Vec<u8> {
    let end = item.len();
    item[end - 4..].copy_from_slice(&frequency.to_le_bytes());
    item
}

/// What a merge leaves: how it ended, the total and the overrides.
type Merged = (
    Result<(), Stop>,
    Option<u32>,
    BTreeMap<u32, Option<ChunkItem>>,
);

fn merge_of(library: &SystemLibrary, records: Vec<LogRecord>) -> Merged {
    let mut merge = Merge::new(Some(library.view()));
    let mut outcome = Ok(());
    for record in records {
        if let Err(stop) = merge.apply_fixture(&record) {
            outcome = Err(stop);
            break;
        }
    }
    let (total, gone, retained) = merge.finish_views(&mut None, &"test");
    let mut overrides: BTreeMap<_, _> = gone.into_iter().map(|slot| (slot, None)).collect();
    if let Some(retained) = retained {
        overrides.extend(changed_items(
            &ReplayedLibrary::pristine(library.view()),
            &retained,
        ));
    }
    (outcome, total, overrides)
}

/// The slots whose item `after` reads differently from `before`.
fn changed_items(
    before: &ReplayedLibrary,
    after: &ReplayedLibrary,
) -> BTreeMap<u32, Option<ChunkItem>> {
    (0..before.range_end().max(after.range_end()))
        .filter_map(|slot| {
            let now = after.item(slot);
            (now != before.item(slot)).then_some((slot, now))
        })
        .collect()
}

fn modify(token: u32, old_item: Vec<u8>, new_item: Vec<u8>) -> LogRecord {
    LogRecord::Modify {
        token,
        old_item,
        new_item,
    }
}

/// Basis: `074a2219 phrase_index.cpp:503-512` (each header compares the running total).
#[test]
fn chained_modify_headers_both_apply_to_the_running_total() {
    let header = |old_total, new_total| LogRecord::ModifyHeader {
        old_total,
        new_total,
    };
    let first = 1_000_000_150;
    let (outcome, total, _) = merge_of(&library(), vec![header(150, first), header(first, 7)]);
    assert!(outcome.is_ok());
    assert_eq!(total, Some(7));
}

/// Basis: `074a2219 phrase_index.cpp:460-470` (the removal happens before the comparison fails).
#[test]
fn a_failed_remove_keeps_the_removal_and_stops() {
    let records = vec![
        LogRecord::Remove {
            token: 1,
            old_item: with_last(one(), 0xff),
        },
        LogRecord::Add {
            token: 5,
            new_item: one(),
        },
    ];
    let (outcome, total, overrides) = merge_of(&library(), records);
    assert!(matches!(outcome, Err(Stop::Mismatch("REMOVE", 1))));
    assert_eq!(total, Some(50));
    assert_eq!(overrides, BTreeMap::from([(1, None)]));
}

/// Basis: `074a2219 phrase_index.cpp:477-484` (MODIFY compares first and changes nothing on a mismatch).
#[test]
fn a_trailing_byte_makes_an_old_chunk_differ() {
    let mut trailing = one();
    trailing.push(0);
    let (outcome, total, overrides) =
        merge_of(&library(), vec![modify(1, trailing, with_last(one(), 9))]);
    assert!(matches!(outcome, Err(Stop::Mismatch("MODIFY", 1))));
    assert_eq!((total, overrides), (Some(150), BTreeMap::new()));
}

/// Basis: policy row 107 (a stored item is its declared extent; the next record compares that).
#[test]
fn a_replacement_ignores_bytes_after_its_declared_item() {
    let mut grown = with_freq(one(), 5);
    grown.extend_from_slice(&[1, 2, 3]);
    let visible = grown[..grown.len() - 3].to_vec();
    let (outcome, total, overrides) = merge_of(
        &library(),
        vec![
            modify(1, one(), grown.clone()),
            modify(1, visible.clone(), with_freq(visible.clone(), 7)),
        ],
    );
    assert!(outcome.is_ok());
    assert_eq!(total, Some(150));
    assert_eq!(
        overrides[&1].as_ref().expect("item").prons,
        vec![(vec![1], 7)]
    );
    let (outcome, _, _) = merge_of(
        &library(),
        vec![
            modify(1, one(), grown.clone()),
            modify(1, grown, with_freq(visible, 7)),
        ],
    );
    assert!(matches!(outcome, Err(Stop::Mismatch("MODIFY", 1))));
}

/// Basis: `074a2219 phrase_index.cpp:486-500` (a replacement no larger than the item is edited in place).
#[test]
fn a_shorter_replacement_is_what_the_next_record_sees() {
    let mut shorter = two()[..two().len() - 6].to_vec();
    shorter[1] = 1;
    let mut chunk = shorter.clone();
    chunk.extend_from_slice(&[9, 9, 9, 9]);
    assert!(chunk.len() <= two().len());
    let (outcome, total, overrides) = merge_of(
        &library(),
        vec![
            modify(2, two(), chunk),
            modify(2, shorter.clone(), with_freq(shorter, 8)),
        ],
    );
    assert!(outcome.is_ok());
    assert_eq!(total, Some(150));
    assert_eq!(
        overrides[&2].as_ref().expect("item").prons,
        vec![(vec![2], 8)]
    );
}

/// Basis: policy row 107 (an absent MODIFY stops at that record).
#[test]
fn an_absent_modify_stops_with_the_previous_item_intact() {
    let first = with_freq(two(), 21);
    let (outcome, total, overrides) = merge_of(
        &library(),
        vec![
            modify(2, two(), first.clone()),
            modify(9, first, with_freq(two(), 22)),
            LogRecord::Add {
                token: 7,
                new_item: one(),
            },
        ],
    );
    assert!(matches!(outcome, Err(Stop::Mismatch("MODIFY", 9))));
    assert_eq!(total, Some(150));
    assert_eq!(
        overrides[&2].as_ref().expect("item").prons,
        vec![(vec![2], 30), (vec![3], 21)]
    );
    assert!(!overrides.contains_key(&9));
    assert!(!overrides.contains_key(&7));
}

/// Basis: `074a2219 phrase_index.cpp:486-500` (a payload no larger than the item is written
/// over it in place); `:180-206` (the read takes the header's extent, across the old bytes).
#[test]
fn an_incomplete_modify_completes_from_the_bytes_it_overwrites() {
    let short = bytes_with(&[1, 1], 200);
    let (outcome, total, overrides) = merge_of(
        &library(),
        vec![
            modify(1, one(), short),
            LogRecord::Add {
                token: 5,
                new_item: one(),
            },
        ],
    );
    assert!(outcome.is_ok());
    assert_eq!(total, Some(250));
    assert_eq!(
        overrides.get(&1),
        Some(&Some(item(&[0x4f60], 200, &[(vec![1], 100)])))
    );
    assert!(!overrides.contains_key(&2));
    assert!(overrides[&5].is_some());
}

/// Basis: policy row 107 (an item read past the buffer answers no item, and a later MODIFY of it stops).
#[test]
fn a_stored_header_past_the_buffer_stops_a_later_read() {
    let overlong = bytes_with(&[255, 255], 200);
    let (outcome, total, overrides) =
        merge_of(&library(), vec![modify(1, one(), overlong.clone())]);
    assert!(outcome.is_ok());
    assert_eq!(total, Some(150));
    assert_eq!(overrides.get(&1), Some(&None));
    let (outcome, _, _) = merge_of(
        &library(),
        vec![
            modify(1, one(), overlong),
            modify(1, one(), with_freq(one(), 9)),
        ],
    );
    assert!(matches!(outcome, Err(Stop::Mismatch("MODIFY", 1))));
}

/// Basis: `074a2219 phrase_index.cpp:453-457` (ADD stores the payload unread); policy row 107 (the later read stops).
#[test]
fn an_add_payload_shorter_than_its_header_is_stored_and_stops_a_later_read() {
    let payload = bytes_with(&[1, 1], 7);
    let (outcome, total, overrides) = merge_of(
        &library(),
        vec![LogRecord::Add {
            token: 5,
            new_item: payload.clone(),
        }],
    );
    assert!(outcome.is_ok());
    assert_eq!(total, Some(157));
    assert_eq!(overrides.get(&5), Some(&None));
    let (outcome, _, _) = merge_of(
        &library(),
        vec![
            LogRecord::Add {
                token: 5,
                new_item: payload,
            },
            LogRecord::Remove {
                token: 5,
                old_item: one(),
            },
        ],
    );
    assert!(matches!(outcome, Err(Stop::Mismatch("REMOVE", 5))));
}

/// Basis: policy row 108 (`memory_chunk.h:390` aborts on the short ADD after the records before it).
#[test]
fn a_short_add_is_reported_after_the_records_before_it() {
    let records = vec![
        modify(1, one(), with_freq(one(), 77)),
        LogRecord::Add {
            token: 5,
            new_item: vec![1],
        },
        LogRecord::Add {
            token: 5,
            new_item: one(),
        },
    ];
    let (outcome, total, overrides) = merge_of(&library(), records);
    assert!(matches!(outcome, Err(Stop::ShortAdd(5))));
    assert_eq!(total, Some(150));
    assert_eq!(overrides[&1].as_ref().expect("item").prons[0].1, 77);
    assert!(!overrides.contains_key(&5));
}

/// Basis: `phrase_index.cpp:180-205` (the item extent comes from its header counts).
#[test]
fn an_empty_phrase_is_an_item_of_its_header_length() {
    let empty = vec![0, 0, 3, 0, 0, 0];
    let (outcome, total, overrides) = merge_of(
        &library(),
        vec![
            modify(1, one(), empty.clone()),
            modify(1, empty, vec![0, 0, 4, 0, 0, 0]),
        ],
    );
    assert!(outcome.is_ok());
    assert_eq!(total, Some(150));
    assert_eq!(overrides[&1], Some(item(&[], 4, &[])));
}

fn assert_rollback_restores(original: &SystemLibrary, records: Vec<LogRecord>) {
    let mut merge = Merge::new(Some(original.view()));
    let lengths = (merge.live.offsets().len(), merge.live.content().len());
    for record in records {
        assert!(merge.apply_fixture(&record).is_ok());
    }
    assert!(logged(&merge));
    let rolled = merge.rollback();
    let before = ReplayedLibrary::pristine(original.view());
    assert_restored(&rolled, &before);
    assert_eq!((rolled.offsets().len(), rolled.content().len()), lengths);
}

fn logged(merge: &Merge) -> bool {
    merge
        .live
        .journal
        .as_ref()
        .is_some_and(|journal| !journal.undo.is_empty())
}

/// Every slot reads, allocates and answers as before the replay.
fn assert_restored(rolled: &ReplayedLibrary, before: &ReplayedLibrary) {
    assert_eq!(rolled.total(), before.total());
    assert_eq!(rolled.range_end(), before.range_end());
    for slot in 0..before.range_end() {
        assert_eq!(rolled.item_bytes(slot), before.item_bytes(slot));
        assert_eq!(
            rolled.ordinary_item(slot).map(|item| item.as_bytes()),
            before.ordinary_item(slot).map(|item| item.as_bytes())
        );
        assert_eq!(rolled.is_added_slot(slot), before.is_added_slot(slot));
        assert!(!rolled.item_is_incomplete(slot));
    }
    assert!(changed_items(before, rolled).is_empty());
}

fn rollback_cases() -> Vec<Vec<LogRecord>> {
    vec![
        // A larger replacement changes the running total.
        vec![modify(1, one(), two())],
        // REMOVE, then ADD at the same slot.
        vec![
            LogRecord::Remove {
                token: 1,
                old_item: one(),
            },
            LogRecord::Add {
                token: 1,
                new_item: with_freq(one(), 5),
            },
        ],
        // A distant ADD between two MODIFY records.
        vec![
            modify(1, one(), with_freq(one(), 77)),
            LogRecord::Add {
                token: 5000,
                new_item: one(),
            },
            modify(2, two(), with_freq(two(), 88)),
        ],
        // A distant ADD, then a REMOVE and an incomplete MODIFY.
        vec![
            LogRecord::Add {
                token: 5000,
                new_item: one(),
            },
            LogRecord::Remove {
                token: 2,
                old_item: two(),
            },
            modify(1, one(), bytes_with(&[1, 1], 3)),
        ],
    ]
}

/// Basis: policy row 107 (a truncated journal restores the operation's starting state).
#[test]
fn rollback_restores_the_buffer_for_every_kind_of_mutation() {
    let original = library();
    for records in rollback_cases() {
        assert_rollback_restores(&original, records);
    }
}

/// Basis: policy row 107, over a mapped baseline.
#[test]
fn rollback_restores_a_mapped_buffer() {
    let items = [
        (1, &item(&[0x4f60], 100, &[(vec![1], 100)])),
        (2, &item(&[0x597d], 50, &[(vec![2], 30), (vec![3], 20)])),
    ];
    let chunk = crate::data::phrase_library::build_chunk(&items).expect("chunk");
    let dir = crate::user::test_session::tempdir("rollback");
    let path = dir.join("rollback.bin");
    std::fs::write(&path, chunk).expect("write");
    for records in rollback_cases() {
        let mapped = crate::data::phrase_library::PhraseLibrary::open(&path).expect("open");
        let mut library = SystemLibrary {
            total: mapped.total_freq(),
            range_end: mapped.token_range().end,
            items: BTreeMap::new(),
            mapped: Some(mapped),
        };
        let before = ReplayedLibrary::pristine(library.view());
        let content = library
            .mapped
            .as_mut()
            .expect("mapped")
            .take_live_content()
            .expect("owner");
        let live = ReplayedLibrary::from_mapping(content, library.total);
        let mut merge = Merge::from_live(Some(library.view()), live);
        assert!(merge.live.mapping.is_some());
        for record in records {
            assert!(merge.apply_fixture(&record).is_ok());
        }
        assert!(logged(&merge));
        let rolled = merge.rollback();
        // The offset words written in the mapping are back.
        assert_restored(&rolled, &before);
    }
    std::fs::remove_dir_all(&dir).expect("cleanup");
}

/// Basis: `074a2219 tests/storage/test_phrase_index_logger.cpp:32-63`.
#[test]
fn sparse_unigram_edits_emit_only_changed_items_and_replay_the_native_journal() {
    use crate::data::user_files::{encode_log_records, log_records};
    use crate::user::{LiveSubIndex, encode_live_diff};

    for total in [7, u32::MAX] {
        let old = SystemLibrary {
            total,
            range_end: 4,
            items: BTreeMap::from([(1, item(&[97], 2, &[])), (3, item(&[99], 5, &[]))]),
            mapped: None,
        };
        let mut live = ReplayedLibrary::pristine(old.view());
        let mut expected = vec![LogRecord::ModifyHeader {
            old_total: total,
            new_total: if total == 7 { 11 } else { total },
        }];
        for (slot, before) in &old.items {
            let outcome = live.add_unigram(*slot, 1);
            if total == 7 {
                assert!(matches!(outcome, UnigramAdd::Accepted));
                // The first edit copied the record to the content end; the
                // second writes that copy in place.
                let content = live.content().len();
                assert!(matches!(live.add_unigram(*slot, 1), UnigramAdd::Accepted));
                assert_eq!(live.content().len(), content);
                let mut after = before.clone();
                after.unigram += 2;
                expected.push(modify(
                    *slot,
                    journal_item_bytes(before),
                    journal_item_bytes(&after),
                ));
            } else {
                assert!(matches!(outcome, UnigramAdd::Refused));
            }
        }
        assert!(matches!(live.add_unigram(2, 1), UnigramAdd::Missing));
        let mut payload = Vec::new();
        encode_live_diff(
            LiveSubIndex::Buffer(&live),
            old.view(),
            live.total(),
            &mut payload,
        )
        .unwrap();
        assert_eq!(payload, encode_log_records(&expected).unwrap());
        let file = crate::data::chunk_format::build_memory_chunk(&payload).unwrap();
        let decoded = log_records(crate::data::user_files::read_chunk_payload(&file).unwrap())
            .map(|record| record.map(LogRecord::into_owned))
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(decoded, expected);
        let (result, new_total, changed) = merge_of(&old, decoded);
        assert!(result.is_ok());
        assert_eq!(new_total, Some(live.total()));
        assert!(!changed.contains_key(&2));
        for (slot, before) in &old.items {
            assert_eq!(
                changed.get(slot).and_then(Option::as_ref).unwrap_or(before),
                &live.item(*slot).unwrap()
            );
        }
    }
}

/// Basis: `074a2219 tests/storage/test_phrase_index.cpp:10-71` (0.75 at `:54`, 0.5 at `:62` and `:70`).
#[test]
fn a_loaded_item_keeps_reading_order_when_training_matches_the_first() {
    use crate::data::PhraseLibraries;

    fn possibility(item: &ChunkItem, key: crate::parser::ChewingKey) -> (u64, u64) {
        let frame = crate::data::phrase_library::build_chunk(&[(1, item)]).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gb_char.bin");
        crate::input::chunk::save(&path, &frame).unwrap();
        let mut libraries = PhraseLibraries::empty();
        assert!(libraries.load(0, &path).unwrap());
        libraries.pronunciation_possibility(1, &[key]).unwrap()
    }

    let cheng = crate::parser::ChewingKey::from_pinyin("cheng").unwrap();
    let shang = crate::parser::ChewingKey::from_pinyin("shang").unwrap();
    let stored = item(
        &[2],
        0,
        &[
            (vec![cheng.to_packed()], 100),
            (vec![shang.to_packed()], 300),
        ],
    );
    assert_eq!(possibility(&stored, shang), (300, 400));
    let frame = crate::data::phrase_library::build_chunk(&[(1, &stored)]).unwrap();
    let end = frame.len();
    let library = PhraseLibrary::from_bytes(
        crate::data::phrase_library::LibraryBytes::from(frame),
        8..end,
        None,
    )
    .unwrap();
    let baseline = SystemLibrary::from_library(library);
    let mut live = ReplayedLibrary::pristine(baseline.view());
    assert_eq!(live.item(1), Some(stored));
    assert!(live.train(1, [vec![cheng]].into_iter(), 200));
    let trained = live.item(1).unwrap();
    assert_eq!(
        trained,
        item(
            &[2],
            0,
            &[
                (vec![cheng.to_packed()], 300),
                (vec![shang.to_packed()], 300)
            ]
        )
    );
    assert_eq!(possibility(&trained, cheng), (300, 600));
}

/// Basis: `phonetic_lookup.h:868-929` (`train_result3`: a running `guint32` total per reading walk).
#[test]
fn training_in_the_buffer_follows_the_running_total() {
    use crate::parser::ChewingKey;
    let ni = ChewingKey::new(23, 0, 1, 0);
    let hao = ChewingKey::new(11, 3, 2, 0);
    let (ni_bits, hao_bits) = (ni.to_packed(), hao.to_packed());
    let max = u32::MAX;
    type Prons = Vec<(Vec<u16>, u32)>;
    let cases: [(Prons, Vec<Vec<ChewingKey>>, u32, Prons); 4] = [
        // The overflow: 100 + 483 takes the delta; the total then reaches
        // max - 367 and cannot take 483, so the last reading keeps 50.
        (
            vec![
                (vec![ni_bits], 100),
                (vec![hao_bits], max - 1000),
                (vec![ni_bits], 50),
            ],
            vec![vec![ni]],
            483,
            vec![
                (vec![ni_bits], 583),
                (vec![hao_bits], max - 1000),
                (vec![ni_bits], 50),
            ],
        ),
        // A repeated key, trained twice: each pass gives both entries 7.
        (
            vec![(vec![ni_bits], 100), (vec![ni_bits], 7)],
            vec![vec![ni], vec![ni]],
            7,
            vec![(vec![ni_bits], 114), (vec![ni_bits], 21)],
        ),
        // No entry matches: nothing changes.
        (
            vec![(vec![ni_bits], 100)],
            vec![vec![hao]],
            7,
            vec![(vec![ni_bits], 100)],
        ),
        // The first increment takes the total to 1. The next count wraps
        // the total to 0, so its delta fits and its stored count wraps too.
        (
            vec![(vec![ni_bits], 0), (vec![ni_bits], max), (vec![ni_bits], 1)],
            vec![vec![ni]],
            1,
            vec![(vec![ni_bits], 1), (vec![ni_bits], 0), (vec![ni_bits], 2)],
        ),
    ];
    for (prons, readings, delta, trained) in cases {
        let library = SystemLibrary {
            total: 9,
            range_end: 2,
            items: BTreeMap::from([(1, item(&[0x4f60], 9, &prons))]),
            mapped: None,
        };
        let mut buffer = ReplayedLibrary::pristine(library.view());
        assert!(buffer.train(1, readings.into_iter(), delta));
        assert_eq!(buffer.item(1), Some(item(&[0x4f60], 9, &trained)));
    }
}

/// Basis: `phonetic_lookup.h:868-929` (no item, no reading walk).
#[test]
fn training_leaves_an_absent_slot_alone_and_does_not_consume_the_readings() {
    use crate::parser::ChewingKey;
    let library = library();
    let mut buffer = ReplayedLibrary::pristine(library.view());
    let mut consumed = false;
    let readings = std::iter::from_fn(|| {
        consumed = true;
        None::<Vec<ChewingKey>>
    });
    let end = buffer.range_end();
    assert!(!buffer.train(9, readings, 5));
    assert!(!consumed);
    assert!(buffer.item(9).is_none());
    assert_eq!(buffer.range_end(), end);
}

/// Basis: `074a2219 phrase_index.cpp:624-646` (`get_range` scans back past
/// zero offsets) and `:648-675` (`compact`); policy row 130 (an item past the
/// content end reads as no item and keeps its slot).
#[test]
fn range_end_gives_back_trailing_removals_and_keeps_incomplete_slots() {
    let mut live = ReplayedLibrary::pristine(library().view());
    assert_eq!(live.range_end(), 3);
    // An item past the table grows it.
    live.add_item(7, &one());
    assert_eq!(live.range_end(), 8);
    // A trailing removal gives its slot back, with the zero offsets below it.
    assert!(live.remove_item(7).is_some());
    assert_eq!(live.range_end(), 3);
    assert!(live.remove_item(2).is_some());
    assert_eq!(live.range_end(), 2);
    // A payload stored whole whose header passes the content end reads as
    // no item and still occupies its slot.
    live.add_item(5, &bytes_with(&[1, 1], 7));
    assert!(live.item(5).is_none());
    assert!(live.item_is_incomplete(5));
    assert_eq!(live.range_end(), 6);
    // An offset past the content reads as no item and occupies its slot.
    live.put_word(4, live.content().len() as u32 + 100);
    assert!(live.item_field_bytes(4).is_none());
    // The rebuild keeps every read and every slot.
    let before: Vec<_> = (0..6)
        .map(|slot| live.item_field_bytes(slot).map(Cow::into_owned))
        .collect();
    live.compact();
    let after: Vec<_> = (0..6)
        .map(|slot| live.item_field_bytes(slot).map(Cow::into_owned))
        .collect();
    assert_eq!(after, before);
    assert_eq!(live.range_end(), 6);
    assert!(live.remove_item(1).is_some());
    assert_eq!(live.range_end(), 6);
}
