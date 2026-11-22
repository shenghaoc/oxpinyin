use super::*;

fn key(initial: u8) -> ChewingKey {
    ChewingKey::new(initial, 1, 1, 0)
}

fn pair(a: u8, b: u8) -> Vec<ChewingKey> {
    vec![key(a), key(b)]
}

fn item(token: u32, keys: &[ChewingKey]) -> Vec<u8> {
    encode_item(token, keys)
}

fn record(items: &[(u32, &[ChewingKey])]) -> Vec<u8> {
    items
        .iter()
        .flat_map(|(token, keys)| item(*token, keys))
        .collect()
}

#[test]
fn entry_ops_keep_the_pins_order_and_report_presence() {
    let (a, b) = (pair(2, 2), pair(3, 3));
    let mut bytes = Vec::new();
    let mut entry = Entry::new(&mut bytes, 2);
    assert!(entry.add_index(&b, 9));
    assert!(entry.add_index(&a, 7));
    assert!(entry.add_index(&a, 5));
    assert!(!entry.add_index(&a, 5));
    assert_eq!(*entry.bytes, record(&[(5, &a), (7, &a), (9, &b)]));
    assert!(!entry.remove_index(&a, 9));
    assert!(entry.remove_index(&a, 7));
    entry.mask_out(0xffff_ffff, 5);
    assert_eq!(*entry.bytes, record(&[(9, &b)]));
}

fn index(rows: &[(Vec<u8>, Vec<u8>)]) -> UserIndex {
    let mut index = UserIndex::new();
    for (key, value) in rows {
        index.push_record(key, value).unwrap();
    }
    index
}

#[test]
fn a_loaded_record_answers_membership_without_a_side_set() {
    let keys = pair(2, 2);
    let complete = encode_complete_key(&keys);
    let incomplete = encode_incomplete_key(&keys);
    let mut index = index(&[
        (incomplete, record(&[(7, &keys)])),
        (complete, record(&[(7, &keys)])),
    ]);
    let packed: Vec<u16> = keys.iter().map(|key| key.to_packed()).collect();
    assert!(index.contains_reading(7, &packed));
    assert!(!index.contains_reading(8, &packed));
    assert!(index.add(&pair(3, 3), 9));
    assert!(index.contains_reading(
        9,
        &pair(3, 3).iter().map(|k| k.to_packed()).collect::<Vec<_>>()
    ));
}

#[test]
fn add_and_remove_keep_prefix_markers_and_record_bytes() {
    let keys = pair(2, 3);
    let mut index = UserIndex::new();
    assert!(index.add(&keys, 7));
    let complete = encode_complete_key(&keys);
    let incomplete = encode_incomplete_key(&keys);
    assert_eq!(
        index.record(&complete).as_deref(),
        Some(record(&[(7, &keys)]).as_slice())
    );
    assert_eq!(
        index.record(&incomplete).as_deref(),
        Some(record(&[(7, &keys)]).as_slice())
    );
    assert_eq!(index.record(&complete[..2]).as_deref(), Some(&[][..]));
    assert!(!index.add(&keys, 7));
    assert!(index.remove(&keys, 7));
    assert!(
        index
            .record(&complete)
            .as_deref()
            .is_some_and(|bytes| bytes.is_empty())
    );
    assert!(!index.remove(&keys, 7));
}

#[test]
fn has_prefix_probes_existing_and_new_markers() {
    let full = pair(1, 2);
    let mut index = index(&[(encode_complete_key(&full), record(&[(7, &full)]))]);
    assert!(!index.has_prefix(&full[..1]));
    assert!(index.suggest(&full[..1]).is_empty());
    assert!(index.has_prefix(&full));
    assert!(!index.has_prefix(&pair(3, 4)[..1]));
    // Adding a longer key creates its empty prefix marker, which counts
    // as a stored key.
    assert!(index.add(&pair(3, 4), 8));
    assert!(
        index
            .record(&encode_complete_key(&pair(3, 4)[..1]))
            .is_some_and(|bytes| bytes.is_empty())
    );
    assert!(index.has_prefix(&pair(3, 4)[..1]));
    assert!(!index.has_prefix(&pair(5, 6)[..1]));
}

fn toned(initial: u8, tone: u8) -> ChewingKey {
    ChewingKey::new(initial, 1, 1, tone)
}

#[test]
fn query_key_builds_the_encoders_bytes_on_the_stack() {
    let complete = [toned(2, 3), toned(4, 0)];
    assert_eq!(*query_key(&complete), *encode_complete_key(&complete));
    let incomplete = [toned(2, 3), ChewingKey::new(4, 0, 0, 0)];
    assert_eq!(*query_key(&incomplete), *encode_incomplete_key(&incomplete));
    let longest = [toned(5, 1); MAX_PHRASE_LENGTH];
    assert_eq!(*query_key(&longest), *encode_complete_key(&longest));
}

#[test]
fn search_filters_a_mixed_tone_row_in_stored_order() {
    let (one, two, none) = (
        [toned(2, 1), toned(3, 4)],
        [toned(2, 2), toned(3, 4)],
        [toned(2, 0), toned(3, 0)],
    );
    // Unsorted, with a duplicate: stored as found.
    let row = record(&[(9, &two), (7, &one), (8, &none), (7, &one), (6, &two)]);
    let index = index(&[(encode_complete_key(&one), row)]);
    assert_eq!(index.search(&one), [7, 8, 7]);
    assert_eq!(index.search(&two), [9, 8, 6]);
    assert_eq!(index.search(&[toned(2, 3), toned(3, 4)]), [8]);
    assert_eq!(index.search(&[toned(2, 1), toned(3, 5)]), [8]);
}

#[test]
fn a_zero_tone_query_matches_every_toned_record() {
    let records = [
        [toned(2, 1), toned(3, 4)],
        [toned(2, 2), toned(3, 1)],
        [toned(2, 5), toned(3, 0)],
    ];
    let row = record(&[(1, &records[0]), (2, &records[1]), (3, &records[2])]);
    let index = index(&[(encode_complete_key(&records[0]), row)]);
    assert_eq!(index.search(&[toned(2, 0), toned(3, 0)]), [1, 2, 3]);
    assert_eq!(index.search(&[toned(2, 0), toned(3, 1)]), [2, 3]);
}

#[test]
fn a_misaligned_row_answers_its_whole_records() {
    let keys = pair(2, 3);
    let mut row = record(&[(7, &keys), (8, &keys)]);
    row.push(0);
    let longer = [key(2), key(3), key(4)];
    let mut longer_row = record(&[(9, &longer)]);
    longer_row.pop();
    let mut index = index(&[
        (encode_complete_key(&keys), row),
        (encode_complete_key(&longer), longer_row),
    ]);
    assert!(index.has_record(&keys));
    // The trailing byte is dropped; the two whole records stay.
    assert_eq!(index.search(&keys), [7, 8]);
    // One byte short of its only record: nothing whole to read.
    assert!(index.suggest(&keys).is_empty());

    let suggestion = [key(2), key(3), key(5)];
    let mut suggestion_row = record(&[(10, &suggestion)]);
    suggestion_row.push(0);
    index
        .push_record(&encode_complete_key(&suggestion), &suggestion_row)
        .unwrap();
    assert_eq!(index.suggest(&keys), [10]);
    assert!(index.contains_reading(10, &suggestion.map(ChewingKey::to_packed)));
}

#[test]
fn suggest_filters_longer_records_with_tones() {
    let prefix = [toned(2, 1)];
    let (one, two) = ([toned(2, 1), toned(3, 2)], [toned(2, 2), toned(3, 2)]);
    let index = index(&[
        (encode_complete_key(&prefix), Vec::new()),
        (
            encode_complete_key(&one),
            record(&[(5, &two), (4, &one), (4, &one)]),
        ),
    ]);
    assert_eq!(index.suggest(&prefix), [4, 4]);
    assert_eq!(index.suggest(&[toned(2, 0)]), [5, 4, 4]);
}

/// The pin's `pinyin_compare_with_tones` (`pinyin_phrase3.h:111-142` at
/// 074a2219) transcribed pass for pass, over decoded keys.
fn pin_compare_with_tones(lhs: &[ChewingKey], rhs: &[ChewingKey]) -> i32 {
    for (lhs, rhs) in lhs.iter().zip(rhs) {
        let result = i32::from(lhs.initial) - i32::from(rhs.initial);
        if result != 0 {
            return result;
        }
    }
    for (lhs, rhs) in lhs.iter().zip(rhs) {
        if (lhs.middle == rhs.middle && lhs.final_ == rhs.final_)
            || (lhs.middle == 0 && lhs.final_ == 0)
            || (rhs.middle == 0 && rhs.final_ == 0)
        {
            continue;
        }
        let middle = i32::from(lhs.middle) - i32::from(rhs.middle);
        if middle != 0 {
            return middle;
        }
        return i32::from(lhs.final_) - i32::from(rhs.final_);
    }
    for (lhs, rhs) in lhs.iter().zip(rhs) {
        if lhs.tone == rhs.tone || lhs.tone == 0 || rhs.tone == 0 {
            continue;
        }
        return i32::from(lhs.tone) - i32::from(rhs.tone);
    }
    0
}

#[test]
fn the_in_place_compare_agrees_with_decoding_for_every_packed_word() {
    let mut queries = Vec::new();
    for initial in [0, 1, 23, 31, 32, 40] {
        for (middle, final_) in [(0, 0), (0, 3), (1, 0), (2, 17), (3, 31), (4, 0), (0, 32)] {
            for tone in [0, 1, 5, 7, 8] {
                queries.push(ChewingKey::new(initial, middle, final_, tone));
            }
        }
    }
    let mut record = [0u8; 6];
    for word in 0..=u16::MAX {
        record[4..].copy_from_slice(&word.to_le_bytes());
        let stored = [ChewingKey::from_packed(word)];
        for query in &queries {
            assert_eq!(
                record_matches_with_tones(std::slice::from_ref(query), &record),
                pin_compare_with_tones(std::slice::from_ref(query), &stored) == 0,
                "word {word:#06x} query {query:?}",
            );
        }
    }
}

#[test]
fn mask_keeps_berkeley_db_records_independent() {
    let (a, b) = (pair(1, 1), pair(2, 2));
    let mut index = index(&[
        (encode_complete_key(&a), record(&[(1, &a), (5, &a)])),
        (encode_complete_key(&b), record(&[(2, &b)])),
    ]);
    index.mask(0xffff_ffff, 5);
    assert_eq!(
        index.record(&encode_complete_key(&a)).as_deref(),
        Some(record(&[(1, &a)]).as_slice())
    );
    assert_eq!(
        index.record(&encode_complete_key(&b)).as_deref(),
        Some(record(&[(2, &b)]).as_slice())
    );
}

/// Two length-2 keys, `short` sorting first with one item and `long`
/// after it with two, as the Kyoto Cabinet and tkrzw mask walk visits
/// them (ascending key order).
#[cfg(not(feature = "bdb"))]
fn short_then_long() -> (Vec<ChewingKey>, Vec<ChewingKey>) {
    let (x, y) = (pair(1, 1), pair(2, 2));
    if encode_complete_key(&x) < encode_complete_key(&y) {
        (x, y)
    } else {
        (y, x)
    }
}

#[cfg(not(feature = "bdb"))]
fn long_record(long: &[ChewingKey]) -> Vec<u8> {
    record(&[(2, long), (3, long)])
}

/// The short record after a mask that removes nothing, with `tail`
/// appended: the previous record's items beyond its length that a
/// reused chunk keeps.
#[cfg(not(feature = "bdb"))]
fn masked_short(short: &[ChewingKey], tail: &[u8]) -> Vec<u8> {
    let mut expected = record(&[(1, short)]);
    expected.extend_from_slice(tail);
    expected
}

#[cfg(feature = "bdb")]
#[test]
fn a_mask_pass_starts_every_record_from_its_own_bytes() {
    // `ChewingLargeTable2::mask_out` points the entry at each record's
    // own data with `set_chunk(db_data.data, db_data.size, NULL)` and
    // puts the entry back with `DB_CURRENT` (chewing_large_table2_bdb.cpp:
    // 498-506 at 074a2219), so a longer record never leaves a tail behind
    // a shorter one, in either walk order and over repeated passes.
    let (x, y) = (pair(1, 1), pair(2, 2));
    let (short, long) = if encode_complete_key(&x) < encode_complete_key(&y) {
        (x, y)
    } else {
        (y, x)
    };
    let long_items = record(&[(2, &long), (3, &long)]);
    let mut index = index(&[
        (encode_complete_key(&short), record(&[(1, &short)])),
        (encode_complete_key(&long), long_items.clone()),
    ]);
    for _ in 0..2 {
        index.mask(0xffff_ffff, 99);
        assert_eq!(
            index.record(&encode_complete_key(&short)).as_deref(),
            Some(record(&[(1, &short)]).as_slice())
        );
        assert_eq!(
            index.record(&encode_complete_key(&long)).as_deref(),
            Some(long_items.as_slice())
        );
    }
    // The same walk also removes what the mask names, record by record.
    index.mask(0xffff_ffff, 3);
    assert_eq!(
        index.record(&encode_complete_key(&long)).as_deref(),
        Some(record(&[(2, &long)]).as_slice())
    );
    assert_eq!(
        index.record(&encode_complete_key(&short)).as_deref(),
        Some(record(&[(1, &short)]).as_slice())
    );
}

#[cfg(not(feature = "bdb"))]
#[test]
fn a_search_hit_leaves_its_record_in_the_entry_the_next_mask_starts_from() {
    // `search_internal` loads a non-empty record into the length's
    // entry (chewing_large_table2_kyotodb.cpp:160-174 at 074a2219).
    // `MaskOutVisitor2::visit_full` then writes each record over the
    // start of that entry with `set_content(0, ...)` and returns the
    // whole entry (:471-478); `set_content` keeps the larger of the old
    // size and the new length (memory_chunk.h:290-296). The first record
    // the walk visits is shorter than the searched one, so it leaves
    // with the searched record's second item behind it.
    let (short, long) = short_then_long();
    let rows = [
        (encode_complete_key(&short), record(&[(1, &short)])),
        (encode_complete_key(&long), long_record(&long)),
    ];

    let mut searched = index(&rows);
    searched.note_search(&long);
    searched.mask(0xffff_ffff, 99);
    let item_len = record(&[(2, &long)]).len();
    assert_eq!(
        searched.record(&encode_complete_key(&short)).as_deref(),
        Some(masked_short(&short, &long_record(&long)[item_len..]).as_slice()),
        "the searched record's tail follows the first record"
    );
    assert_eq!(
        searched.record(&encode_complete_key(&long)).as_deref(),
        Some(long_record(&long).as_slice()),
        "the next record overwrites the whole entry"
    );

    // Without a search the entry starts empty and nothing is carried.
    let mut fresh = index(&rows);
    fresh.mask(0xffff_ffff, 99);
    assert_eq!(
        fresh.record(&encode_complete_key(&short)).as_deref(),
        Some(record(&[(1, &short)]).as_slice())
    );
}

#[cfg(not(feature = "bdb"))]
#[test]
fn a_second_mask_starts_from_the_chunks_the_first_left_in_the_entries() {
    // `m_entries` outlives each `mask_out` call: the entry of a length
    // keeps the last record's chunk when the walk ends
    // (chewing_large_table2_kyotodb.cpp:459-479 at 074a2219 never
    // resets it). The first pass ends on the long record, so the second
    // pass's first (short) record is written over the start of that
    // chunk and keeps its second item behind it (memory_chunk.h:290-296).
    let (short, long) = short_then_long();
    let mut index = index(&[
        (encode_complete_key(&short), record(&[(1, &short)])),
        (encode_complete_key(&long), long_record(&long)),
    ]);
    index.mask(0xffff_ffff, 99);
    assert_eq!(
        index.record(&encode_complete_key(&short)).as_deref(),
        Some(record(&[(1, &short)]).as_slice()),
        "the first pass starts from an empty entry"
    );
    index.mask(0xffff_ffff, 99);
    let item_len = record(&[(2, &long)]).len();
    assert_eq!(
        index.record(&encode_complete_key(&short)).as_deref(),
        Some(masked_short(&short, &long_record(&long)[item_len..]).as_slice()),
        "the second pass starts from the long record's chunk"
    );
    assert_eq!(
        index.record(&encode_complete_key(&long)).as_deref(),
        Some(long_record(&long).as_slice())
    );
}

#[cfg(not(feature = "bdb"))]
#[test]
fn a_suggestion_empties_the_entries_of_the_lengths_it_visits() {
    // `search_suggestion` checks the prefix key, jumps to it, and steps
    // through every longer key that extends it
    // (chewing_large_table2_kyotodb.cpp:541-582, `kyotodb_chewing_
    // continue_search` :34-50 at 074a2219). Each visit borrows the
    // record into that length's entry and ends with `set_size(0)`
    // (:229-233), so the entry the earlier search loaded is empty.
    let (short, long) = short_then_long();
    let prefix = &long[..1];
    let mut index = index(&[
        (encode_complete_key(prefix), Vec::new()),
        (encode_complete_key(&short), record(&[(1, &short)])),
        (encode_complete_key(&long), long_record(&long)),
    ]);
    index.note_search(&long);
    index.note_suggestion(prefix);
    index.mask(0xffff_ffff, 99);
    assert_eq!(
        index.record(&encode_complete_key(&short)).as_deref(),
        Some(record(&[(1, &short)]).as_slice()),
        "the visited length's entry was emptied"
    );
    assert_eq!(
        index.record(&encode_complete_key(prefix)).as_deref(),
        Some(&[][..])
    );
}

#[cfg(not(feature = "bdb"))]
#[test]
fn a_suggestion_on_an_absent_prefix_key_leaves_the_entries_alone() {
    // The pin's first call is `m_db->check(prefix)`; `-1` returns before
    // any cursor is made (chewing_large_table2_kyotodb.cpp:541-544,
    // chewing_large_table2_bdb.cpp:579-583 and
    // chewing_large_table2_tkrzwdb.cpp:511-512 at 074a2219), so no
    // longer key is visited and no entry is emptied, even though a
    // longer key exists.
    let (short, long) = short_then_long();
    let mut index = index(&[
        (encode_complete_key(&short), record(&[(1, &short)])),
        (encode_complete_key(&long), long_record(&long)),
    ]);
    index.note_search(&long);
    index.note_suggestion(&long[..1]);
    index.mask(0xffff_ffff, 99);
    let item_len = record(&[(2, &long)]).len();
    assert_eq!(
        index.record(&encode_complete_key(&short)).as_deref(),
        Some(masked_short(&short, &long_record(&long)[item_len..]).as_slice()),
    );
}

/// Without the prefix record, suggestion never walks the 17-key extension.
#[test]
fn an_overlong_index_key_without_its_prefix_is_not_walked() {
    let ni = crate::parser::ChewingKey::from_pinyin("ni").unwrap();
    let index =
        super::UserIndex::from_entries(vec![(ni.to_packed().to_le_bytes().repeat(17), Vec::new())]);
    assert!(index.suggest(&[ni]).is_empty());
}

#[test]
fn dense_mask_keeps_record_padding_and_partial_tail() {
    // One key per record: four token bytes, two key bytes, two padding bytes.
    let record = |token: u8| [token, 0, 0, 0, token, 1, 0xff, token];
    let mut bytes: Vec<_> = (0..8).flat_map(record).collect();
    bytes.extend([0xaa, 0xbb]);
    Entry::new(&mut bytes, 1).mask_out(1, 0);
    let mut expected = [record(1), record(3), record(5), record(7)].concat();
    expected.extend([0xaa, 0xbb]);
    assert_eq!(bytes, expected);
}
