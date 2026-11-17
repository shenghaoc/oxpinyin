use super::*;

fn item(phrase: &[u32], unigram: u32, prons: &[(Vec<u16>, u32)]) -> ChunkItem {
    ChunkItem {
        phrase: phrase.to_vec(),
        unigram,
        prons: prons.to_vec(),
    }
}

#[test]
fn add_candidates_preserve_mixed_sparse_and_overreach_views() {
    // The `has_added_items` speed gate against the uncached answer: every
    // touched slot the original library does not hold.
    let original = library();
    let sparse_additions = |merge: &mut Merge<'_, '_>| {
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
            assert!(merge.apply_owned(record).is_ok());
        }
    };
    let modified = || {
        let mut merge = Merge::new(Some(original.view()));
        assert!(
            merge
                .apply_owned(modify(1, one(), with_freq(one(), 77)))
                .is_ok()
        );
        merge
    };
    let mut cases: Vec<(&str, Merge<'_, '_>, &[u32])> = Vec::new();
    // A modify-only replay and an ADD over an original slot.
    cases.push(("modify only", modified(), &[]));
    let mut replaced = Merge::new(Some(original.view()));
    assert!(
        replaced
            .apply_owned(LogRecord::Add {
                token: 1,
                new_item: with_freq(one(), 77),
            })
            .is_ok()
    );
    cases.push(("original-slot add", replaced, &[]));
    // Sparse additions over a modified original and over an overreach.
    let mut sparse = modified();
    sparse_additions(&mut sparse);
    cases.push(("sparse", sparse, &[1024]));
    let mut sparse_overreach = over_length_merge(&original);
    sparse_additions(&mut sparse_overreach);
    cases.push(("sparse overreach", sparse_overreach, &[1024]));
    // An overreach alone and with a rejected addition.
    cases.push(("overreach", over_length_merge(&original), &[]));
    let mut rejected = over_length_merge(&original);
    assert!(
        rejected
            .apply_owned(LogRecord::Add {
                token: 1024,
                new_item: bytes_with(&[255, 255], 1),
            })
            .is_ok()
    );
    cases.push(("rejected add", rejected, &[]));
    for (name, merge, added) in cases {
        let (_, _, view) = merge.finish_views(&mut None, &"test");
        let view = view.expect("retained originals");
        let expected: Vec<_> = view
            .slots()
            .filter(|&slot| !original.contains_item(slot))
            .collect();
        let actual: Vec<_> = view
            .add_candidates()
            .filter(|&slot| !original.contains_item(slot))
            .collect();
        assert_eq!(expected, added, "{name}");
        assert_eq!(actual, expected, "{name}");
        for &slot in added {
            assert_eq!(view.item(slot), decode_phrase_item(&one()).ok(), "{name}");
        }
    }
}

/// The over-length item's raw pronunciation words, built once: A's
/// second character is A's first pronunciation key plus its frequency's
/// low half, and both readings borrow B's bytes.
fn over_length_merge(original: &SystemLibrary) -> Merge<'_, '_> {
    let mut merge = Merge::new(Some(original.view()));
    assert!(
        merge
            .apply_owned(modify(1, one(), bytes_with(&[2, 2], 200)))
            .is_ok()
    );
    let offset = merge.slot_offset(1) as usize;
    let Found::Item { len, .. } = merge.content.header(offset) else {
        panic!("expanded item");
    };
    let old = merge
        .content
        .item(offset, len)
        .expect("cross-item bytes")
        .into_owned();
    let mut new = old.clone();
    let at = merge.slot_offset(2) as usize - offset + 2;
    new[at..at + 4].copy_from_slice(&170_u32.to_le_bytes());
    assert!(merge.apply_owned(modify(1, old, new)).is_ok());
    merge
}

#[test]
fn an_unrecorded_overlapped_slot_is_visible_and_seeded() {
    let original = library();
    let merge = over_length_merge(&original);
    let (_, _, view) = merge.finish_views(&mut None, &"test");
    let view = view.expect("retained originals");
    assert_eq!(view.slots().collect::<Vec<_>>(), [1, 2]);
    assert!(view.contains(2));
    assert_eq!(view.item(1).expect("over-length item").unigram, 200);
    assert_eq!(view.item(2).expect("overlapped item").unigram, 170);
}

#[test]
fn an_over_length_item_reports_the_borrowed_keys() {
    let original = library();
    let merge = over_length_merge(&original);
    let (_, _, view) = merge.finish_views(&mut None, &"test");
    let view = view.expect("retained originals");
    let item = view.item(1).expect("over-length item");
    assert_eq!(item.prons[0].0, [0x0000, 0x0201]);
    assert_eq!(item.prons[1].0, [0x597d, 0x0000]);
    assert_eq!(item.prons[0].1, 170);
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

#[test]
fn an_overlong_header_past_the_area_keeps_the_last_changed_item() {
    let original = library();
    let mut merge = Merge::new(Some(original.view()));
    assert!(
        merge
            .apply_owned(modify(1, one(), bytes_with(&[2, 3], 200)))
            .is_ok()
    );
    let _ = merge.content.append(&[0; 8]);
    let mut changed = two();
    changed[2..6].copy_from_slice(&170_u32.to_le_bytes());
    assert!(merge.apply_owned(modify(2, two(), changed)).is_ok());
    let mut skipped = Vec::new();
    let (_, _, retained) = merge.finish_views(&mut Some(&mut skipped), &"test");
    let retained = retained.expect("the over-length item stays");
    assert!(retained.contains(2));
    assert_eq!(retained.item(2).expect("two").unigram, 170);
}

/// #1514 ruling 1 in the replay: an ADD after the content has reached the
/// modulus lands at the size modulo it, over the first item, and the
/// content does not grow. Slot 1 keeps its offset and reads the new item.
#[test]
fn an_add_past_the_boundary_overwrites_the_first_item() {
    let original = library();
    // 8 + 16 + 22 = 46 bytes of content.
    let _limits = crate::user_library::TestLimits::set(46, 1 << 31);
    let mut merge = Merge::new(Some(original.view()));
    let size = merge.content.data_len();
    assert_eq!(size, 46);
    let added = journal_item_bytes(&item(&[0x4e00], 7, &[(vec![9], 8)]));
    assert_eq!(added.len(), 16);
    assert!(
        merge
            .apply_owned(LogRecord::Add {
                token: 3,
                new_item: added,
            })
            .is_ok()
    );
    assert_eq!(merge.content.data_len(), 46, "the size does not grow");
    assert_eq!(merge.slot_offset(3), 8);
    assert_eq!(merge.slot_offset(1), 8, "slot 1 is aliased, not moved");
    assert!(merge.overreach);
    let mut skipped = Vec::new();
    let (total, _, retained) = merge.finish_views(&mut Some(&mut skipped), &"test");
    assert_eq!(total, Some(157), "the total takes only the added unigram");
    let retained = retained.expect("the overwritten buffer is retained");
    assert_eq!(retained.item(1).expect("one").unigram, 7);
    assert_eq!(retained.item(3).expect("three").unigram, 7);
}

/// A live-buffer import whose offset has wrapped writes over earlier
/// bytes. It can make another slot's header unreadable, so the library
/// must keep the per-token removal check active (`train` does the same).
#[test]
fn a_wrapped_import_that_corrupts_a_header_raises_the_removal_flag() {
    let original = library();
    let mut merge = Merge::new(Some(original.view()));
    let added = journal_item_bytes(&item(&[0x4e00], 7, &[(vec![9], 8)]));
    assert!(
        merge
            .apply_owned(LogRecord::Add {
                token: 3,
                new_item: added,
            })
            .is_ok()
    );
    let mut skipped = Vec::new();
    let (_, _, retained) = merge.finish_views(&mut Some(&mut skipped), &"test");
    let mut retained = retained.expect("the added item is retained");
    // The original slots' visibility as the store's removal filter answers
    // it: with the gate off every slot is visible unchecked; with it on,
    // each slot is visible only where the buffer reads an item.
    let visibility = |library: &ReplayedLibrary, gate: bool| {
        original
            .items
            .keys()
            .map(|&slot| !gate || library.item_bytes(slot).is_some())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        visibility(&retained, retained.has_removed()),
        visibility(&retained, true)
    );
    // 8 + 16 + 22 + 16 = 62 bytes of content: the next import wraps to 8,
    // over slot 1's item, whose header becomes 255 characters.
    let _limits = crate::user_library::TestLimits::set(62, 1 << 31);
    let mut corrupting = bytes_with(&[255, 255], 4);
    corrupting.resize(16, 0);
    assert!(retained.add_item(4, &corrupting));
    assert_eq!(
        visibility(&retained, retained.has_removed()),
        visibility(&retained, true)
    );
    assert!(
        !visibility(&retained, true)[0],
        "slot 1 no longer reads as an item"
    );
}

#[test]
fn overreach_fast_path_matches_the_forced_overreach_finish() {
    // The `overreach` fast path against `finish_overreach` forced on:
    // the finished total, overrides, items and skips agree.
    let original = library();
    let finished = |new: &[u8], forced: bool| {
        let mut merge = Merge::new(Some(original.view()));
        let old = one();
        assert!(merge.modify(1, &old, new).is_ok());
        if forced {
            merge.overreach = true;
        }
        let mut skipped = Vec::new();
        let (total, overrides, view) = merge.finish_views(&mut Some(&mut skipped), &"test");
        let items = view.map(|view| {
            (1..view.range_end())
                .map(|slot| view.item(slot))
                .collect::<Vec<_>>()
        });
        (total, overrides, items, skipped)
    };
    for (name, new) in [
        ("same", with_last(one(), 0x7f)),
        ("grown", bytes_with(&[3, 1], 200)),
        ("shorter", bytes_with(&[0, 0], 0)),
    ] {
        assert_eq!(finished(&new, false), finished(&new, true), "{name}");
    }
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
        if let Err(stop) = merge.apply_owned(record) {
            outcome = Err(stop);
            break;
        }
    }
    let (total, gone, retained) = merge.finish_views(&mut None, &"test");
    let mut overrides: BTreeMap<_, _> = gone.into_iter().map(|slot| (slot, None)).collect();
    if let Some(retained) = retained {
        overrides.extend(retained.slots().map(|slot| (slot, retained.item(slot))));
    }
    (outcome, total, overrides)
}

fn modify(token: u32, old_item: Vec<u8>, new_item: Vec<u8>) -> LogRecord {
    LogRecord::Modify {
        token,
        old_item,
        new_item,
    }
}

/// Each MODIFY_HEADER must start from the running total, so a second
/// header chained to the first one's new total also applies
/// (`phrase_index.cpp:503-512` at 074a2219).
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

#[test]
fn a_failed_remove_keeps_the_removal_and_stops() {
    // Same size, other bytes: the item and its unigram are gone already.
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

#[test]
fn a_trailing_byte_makes_an_old_chunk_differ() {
    let mut trailing = one();
    trailing.push(0);
    let (outcome, total, overrides) = merge_of(
        &library(),
        vec![LogRecord::Remove {
            token: 1,
            old_item: trailing.clone(),
        }],
    );
    assert!(matches!(outcome, Err(Stop::Mismatch("REMOVE", 1))));
    assert_eq!((total, overrides), (Some(50), BTreeMap::from([(1, None)])));
    // MODIFY compares first: nothing is changed.
    let (outcome, total, overrides) =
        merge_of(&library(), vec![modify(1, trailing, with_last(one(), 9))]);
    assert!(matches!(outcome, Err(Stop::Mismatch("MODIFY", 1))));
    assert_eq!((total, overrides), (Some(150), BTreeMap::new()));
}

#[test]
fn a_grown_item_is_stored_whole_but_is_as_long_as_its_header() {
    let mut grown = with_freq(one(), 5);
    grown.extend_from_slice(&[1, 2, 3]);
    let visible = grown[..grown.len() - 3].to_vec();
    // The next record's old chunk is the header-length bytes.
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
    // With the stored trailing bytes it is another size: a mismatch.
    let (outcome, _, _) = merge_of(
        &library(),
        vec![
            modify(1, one(), grown.clone()),
            modify(1, grown, with_freq(visible, 7)),
        ],
    );
    assert!(matches!(outcome, Err(Stop::Mismatch("MODIFY", 1))));
}

#[test]
fn an_in_place_chunk_with_a_shorter_header_is_what_the_next_record_sees() {
    // Slot 2 has two readings; the new chunk has one and a tail that fits.
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
    // In place leaves the total alone.
    assert_eq!(total, Some(150));
    assert_eq!(
        overrides[&2].as_ref().expect("item").prons,
        vec![(vec![2], 8)]
    );
}

#[test]
fn a_stale_item_takes_a_same_size_modify_of_an_absent_token() {
    // MODIFY of slot 2 leaves `item` on its storage; a MODIFY of a token
    // the library does not hold, whose old chunk equals it, is written
    // over that storage and the merge goes on.
    let first = with_freq(two(), 21);
    let (outcome, total, overrides) = merge_of(
        &library(),
        vec![
            modify(2, two(), first.clone()),
            modify(9, first.clone(), with_freq(two(), 22)),
            LogRecord::Add {
                token: 7,
                new_item: one(),
            },
        ],
    );
    assert!(outcome.is_ok());
    assert_eq!(total, Some(250));
    assert_eq!(
        overrides[&2].as_ref().expect("item").prons,
        vec![(vec![2], 30), (vec![3], 22)]
    );
    assert!(!overrides.contains_key(&9));
    assert!(overrides.contains_key(&7));
    // A longer new chunk would be the pin's NULL dereference: the merge ends.
    let mut longer = with_freq(two(), 22);
    longer.push(0);
    let (outcome, _, _) = merge_of(
        &library(),
        vec![modify(2, two(), first.clone()), modify(9, first, longer)],
    );
    assert!(matches!(outcome, Err(Stop::Mismatch("MODIFY", 9))));
    // With no earlier fetch the stale item is the six zero bytes of the
    // pin's default `PhraseItem`: a two-character old chunk differs.
    let (outcome, _, _) = merge_of(&library(), vec![modify(9, two(), two())]);
    assert!(matches!(outcome, Err(Stop::Mismatch("MODIFY", 9))));
}

#[test]
fn the_default_stale_item_is_six_zero_bytes() {
    // An absent token twice: the first MODIFY compares against the six
    // zero bytes, writes its six-byte new chunk into that buffer and
    // creates no item; the second compares against what the first wrote.
    let first = bytes_with(&[0, 0], 50);
    let second = bytes_with(&[0, 0], 60);
    let (outcome, total, overrides) = merge_of(
        &library(),
        vec![
            modify(9, vec![0; 6], first.clone()),
            modify(9, first, second),
            LogRecord::Add {
                token: 7,
                new_item: one(),
            },
        ],
    );
    assert!(outcome.is_ok());
    assert_eq!(total, Some(250));
    assert!(!overrides.contains_key(&9), "no item is created");
    assert!(overrides.contains_key(&7), "the merge went on");
    // A non-zero old chunk differs from the default.
    let (outcome, _, overrides) = merge_of(
        &library(),
        vec![modify(9, vec![1; 6], bytes_with(&[0, 0], 50))],
    );
    assert!(matches!(outcome, Err(Stop::Mismatch("MODIFY", 9))));
    assert!(overrides.is_empty());
}

#[test]
fn a_short_new_chunk_is_written_in_place_without_its_own_header_check() {
    // The compare is on the old chunk: a six-byte new chunk for a
    // sixteen-byte item is `memmove`d over its start, the other ten bytes
    // and the total stay.
    let short = bytes_with(&[1, 1], 200);
    assert_eq!(short.len(), 6);
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
    let item = overrides[&1].as_ref().expect("item");
    assert_eq!(item.unigram, 200);
    assert_eq!(item.phrase, vec![0x4f60]);
    assert_eq!(item.prons, vec![(vec![1], 100)]);
}

#[test]
fn a_small_overlong_header_reads_the_following_payload() {
    // Six bytes whose header names twenty-two: the in-place write keeps
    // the item's stored sixteen bytes and changes its header. The next
    // read takes the six bytes that follow, inside the content buffer.
    let overlong = bytes_with(&[2, 1], 200);
    let visible = {
        let mut visible = overlong.clone();
        visible.extend_from_slice(&one()[6..]);
        visible.extend_from_slice(&two()[..6]);
        visible
    };
    assert_eq!(visible.len(), 22);
    let (outcome, total, overrides) = merge_of(&library(), vec![modify(1, one(), overlong)]);
    assert!(outcome.is_ok());
    assert_eq!(total, Some(150));
    let item = overrides[&1].as_ref().expect("item");
    assert_eq!(item.phrase.len(), 2);
    assert_eq!(item.unigram, 200);
    // The header-length bytes are what a later compare sees.
    let (outcome, _, overrides) = merge_of(
        &library(),
        vec![
            modify(1, one(), bytes_with(&[2, 1], 200)),
            modify(1, visible.clone(), with_freq(visible, 7)),
        ],
    );
    assert!(outcome.is_ok());
    assert_eq!(overrides[&1].as_ref().expect("item").prons[0].1, 7);
    // The stored sixteen bytes are no longer the item.
    let (outcome, _, _) = merge_of(
        &library(),
        vec![
            modify(1, one(), bytes_with(&[2, 1], 200)),
            modify(1, one(), with_freq(one(), 9)),
        ],
    );
    assert!(matches!(outcome, Err(Stop::Mismatch("MODIFY", 1))));
}

#[test]
fn a_stored_header_past_the_buffer_stops_a_later_read() {
    // A header naming 132,096 bytes ends past the content buffer; the pin
    // reads past its allocation (row 107): the item answers no item.
    let overlong = bytes_with(&[255, 255], 200);
    let (outcome, total, overrides) =
        merge_of(&library(), vec![modify(1, one(), overlong.clone())]);
    assert!(outcome.is_ok());
    assert_eq!(total, Some(150));
    assert_eq!(
        overrides.get(&1),
        Some(&None),
        "the over-read item answers no item"
    );
    let (outcome, _, _) = merge_of(
        &library(),
        vec![
            modify(1, one(), overlong.clone()),
            modify(1, one(), with_freq(one(), 9)),
        ],
    );
    assert!(matches!(outcome, Err(Stop::Mismatch("MODIFY", 1))));
    let (outcome, _, _) = merge_of(
        &library(),
        vec![
            modify(1, one(), overlong),
            LogRecord::Remove {
                token: 1,
                old_item: one(),
            },
        ],
    );
    assert!(matches!(outcome, Err(Stop::Mismatch("REMOVE", 1))));
}

#[test]
fn an_add_payload_shorter_than_its_header_is_stored_and_stops_a_later_read() {
    // `add_phrase_item` stores the payload without reading its header.
    // A later read of it ends past the buffer (row 107) and stops.
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
    assert_eq!(
        overrides.get(&5),
        Some(&None),
        "the over-read item answers no item"
    );
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

#[test]
fn a_short_add_is_reported_after_the_records_before_it() {
    // A payload shorter than the unigram the ADD reads aborts the pin
    // (`memory_chunk.h:390`, row 108). The merge reports the record by
    // its raw token after applying the earlier MODIFY; the caller returns
    // the fault, so nothing after it applies.
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

/// The buffer a fresh merge over `library` starts from.
fn pristine(library: &SystemLibrary) -> Content {
    Merge::new(Some(library.view())).content
}

/// Whether `a` holds exactly the bytes, sizes and capacities `b` does, with
/// no grown copy of either chunk.
fn same_buffer(a: &Content, b: &Content) -> bool {
    a.items.owned.is_none()
        && a.slots.owned.is_none()
        && a.items.data_end == b.items.data_end
        && a.items.capacity == b.items.capacity
        && a.slots.data_end == b.slots.data_end
        && a.slots.capacity == b.slots.capacity
        && a.backing.bytes(Area::Content) == b.backing.bytes(Area::Content)
        && a.backing.bytes(Area::Offsets) == b.backing.bytes(Area::Offsets)
}

/// Applies `records`, checks the buffer changed, rolls back and checks it
/// is the pristine buffer again, bytes and offsets.
fn assert_rollback_restores(library: &SystemLibrary, records: Vec<LogRecord>) {
    let mut merge = Merge::new(Some(library.view()));
    for record in records {
        assert!(merge.apply_owned(record).is_ok());
    }
    assert!(
        !same_buffer(&merge.content, &pristine(library)),
        "the replay changed nothing, so the check below proves nothing"
    );
    let rolled = merge.rollback();
    assert!(same_buffer(&rolled.content, &pristine(library)));
    assert_eq!(rolled.slots().count(), 0);
}

fn rollback_cases() -> Vec<Vec<LogRecord>> {
    vec![
        // Repeated MODIFY of one token.
        vec![
            modify(1, one(), with_freq(one(), 77)),
            modify(1, with_freq(one(), 77), with_freq(one(), 66)),
        ],
        // A longer MODIFY: a remove and an append, so the offset word is
        // overwritten and the content grows.
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
        // A stale-item write: the absent token compares against the item
        // the previous MODIFY read and writes over it.
        vec![
            modify(1, one(), with_freq(one(), 77)),
            modify(9, with_freq(one(), 77), with_freq(one(), 66)),
        ],
        // Overlapping writes: the second is a prefix of the first.
        vec![
            modify(1, one(), bytes_with(&[0, 0], 0)),
            modify(1, bytes_with(&[0, 0], 0), bytes_with(&[0, 0], 9)),
        ],
        // An in-place write, then growth of the offset array and the
        // content (an ADD far past both), then a write into the grown copy.
        vec![
            modify(1, one(), with_freq(one(), 77)),
            LogRecord::Add {
                token: 5000,
                new_item: one(),
            },
            modify(2, two(), with_freq(two(), 88)),
        ],
    ]
}

#[test]
fn rollback_restores_the_buffer_for_every_kind_of_mutation() {
    let original = library();
    for records in rollback_cases() {
        assert_rollback_restores(&original, records);
    }
}

#[test]
fn rollback_restores_a_mapped_buffer() {
    let items = vec![
        (1, item(&[0x4f60], 100, &[(vec![1], 100)])),
        (2, item(&[0x597d], 50, &[(vec![2], 30), (vec![3], 20)])),
    ];
    let chunk = crate::data::chunk_write::build_chunk(&items).expect("chunk");
    let dir = crate::test_session::tempdir("rollback");
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
        let before = pristine(&library);
        let content = library
            .mapped
            .as_mut()
            .expect("mapped")
            .take_live_content()
            .expect("owner");
        let live = ReplayedLibrary::from_mapping(content, library.total);
        let mut merge = Merge::from_live(Some(library.view()), live);
        assert!(matches!(merge.content.backing, Backing::Mapped(_)));
        for record in records {
            assert!(merge.apply_owned(record).is_ok());
        }
        assert!(!same_buffer(&merge.content, &before));
        let rolled = merge.rollback();
        assert!(same_buffer(&rolled.content, &before));
        assert_eq!(rolled.slots().count(), 0);
    }
    std::fs::remove_dir_all(&dir).expect("cleanup");
}

#[test]
fn valid_records_before_and_after_an_over_read_item_stay() {
    // Row 107, option 1: the item answers no item and the rest stay.
    let original = library();
    let overlong = bytes_with(&[255, 255], 200);
    let (outcome, _, overrides) = merge_of(
        &original,
        vec![
            modify(2, two(), with_freq(two(), 88)),
            modify(1, one(), overlong),
            LogRecord::Add {
                token: 6,
                new_item: with_freq(one(), 5),
            },
        ],
    );
    assert!(outcome.is_ok());
    assert_eq!(overrides.get(&1), Some(&None));
    assert_eq!(overrides[&2].as_ref().expect("two").prons[1].1, 88);
    assert!(overrides[&6].is_some());
}

/// tests/storage/test_phrase_index_logger.cpp:32-63 at 074a2219: one
/// unigram add per slot of the range, then diff, merge and the total.
#[test]
fn sparse_unigram_edits_emit_only_changed_items_and_replay_the_native_journal() {
    use crate::data::user_files::{encode_log_records, log_records};
    use crate::persistence::{LiveSubIndex, diff_live_records};

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
            new_total: if total == 7 { 9 } else { total },
        }];
        for (slot, before) in &old.items {
            let outcome = live.add_unigram(*slot, 1);
            if total == 7 {
                assert!(matches!(outcome, UnigramAdd::Accepted));
                let mut after = before.clone();
                after.unigram += 1;
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
        let records = diff_live_records(LiveSubIndex::Buffer(&live), old.view(), live.total());
        assert_eq!(records, expected);
        let payload = encode_log_records(&records).unwrap();
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

/// tests/storage/test_phrase_index.cpp:10-71 at 074a2219: two readings,
/// the store/load round trip, the 0.75 possibility (`:54`) and 0.5 after
/// the increase, on the item (`:62`) and on a fresh read (`:70`).
#[test]
fn a_heap_loaded_item_keeps_reading_order_when_training_matches_the_first() {
    use crate::data::PhraseLibraries;

    fn possibility(item: &ChunkItem, key: oxpinyin_core::ChewingKey) -> (u64, u64) {
        let frame = crate::data::chunk_write::build_chunk(&[(1, item.clone())]).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gb_char.bin");
        oxpinyin_core::input::chunk::save(&path, &frame).unwrap();
        let mut libraries = PhraseLibraries::empty();
        assert!(libraries.load_heap(0, &path).unwrap());
        libraries.pronunciation_possibility(1, &[key]).unwrap()
    }

    let cheng = oxpinyin_core::ChewingKey::from_pinyin("cheng").unwrap();
    let shang = oxpinyin_core::ChewingKey::from_pinyin("shang").unwrap();
    let mut stored = item(&[2], 0, &[]);
    crate::add_system_reading(&mut stored, &[cheng.to_packed()], 100);
    crate::add_system_reading(&mut stored, &[shang.to_packed()], 300);
    // `:54`: 300 of 400 is the pin's 0.75.
    assert_eq!(possibility(&stored, shang), (300, 400));
    let frame = crate::data::chunk_write::build_chunk(&[(1, stored.clone())]).unwrap();
    let library = PhraseLibrary::from_payload(frame[8..].to_vec()).unwrap();
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
    // `:62`, `:70`: 300 of 600 is the pin's 0.5.
    assert_eq!(possibility(&trained, cheng), (300, 600));
}

/// Each reading walks the item's readings with a running `guint32`
/// total: a matching reading takes `delta` unless the total cannot, and
/// that overflow ends the walk for the reading (`train_result3`,
/// `phonetic_lookup.h:868-929`).
#[test]
fn training_in_the_buffer_follows_the_running_total() {
    use oxpinyin_core::ChewingKey;
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
        // Both additions wrap: the first entry takes the total to 1, the
        // second's count wraps the total to 0 so the delta fits, and that
        // count wraps to 0; the third entry becomes 2.
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

#[test]
fn training_leaves_an_absent_slot_alone_and_does_not_consume_the_readings() {
    use oxpinyin_core::ChewingKey;
    let library = library();
    let mut buffer = ReplayedLibrary::pristine(library.view());
    let mut consumed = false;
    let readings = std::iter::from_fn(|| {
        consumed = true;
        None::<Vec<ChewingKey>>
    });
    assert!(!buffer.train(9, readings, 5));
    assert!(!consumed);
    assert!(!buffer.contains(9));
}
