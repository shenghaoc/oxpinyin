use proptest::prelude::*;

use super::{
    GramFault, checked_get_freq, checked_search, get_freq, get_length, get_total_freq, insert_freq,
    item_freq, item_offset, item_token, lower_bound, mask_out, merge_single_gram, new_gram,
    retrieve_all, set_freq,
};

fn gram(total: u32, items: &[(u32, u32)]) -> Vec<u8> {
    let mut bytes = total.to_le_bytes().to_vec();
    for (token, freq) in items {
        bytes.extend_from_slice(&token.to_le_bytes());
        bytes.extend_from_slice(&freq.to_le_bytes());
    }
    bytes
}

fn items(bytes: &[u8]) -> Vec<(u32, u32)> {
    retrieve_all(bytes).collect()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn partitioned_records_find_the_first_not_less(
        total in any::<u32>(),
        mut row in prop::collection::vec(
            (prop_oneof![0_u32..16, any::<u32>()], any::<u32>()), 0..128),
        begin in any::<u32>(),
        end in any::<u32>(),
        tail in any::<[u8; 7]>(),
    ) {
        row.sort_by_key(|&(token, _)| token);
        let first = row.iter().position(|&(token, _)| token >= begin).unwrap_or(row.len());
        for tail_len in 0..=tail.len() {
            let mut bytes = gram(total, &row);
            bytes.extend_from_slice(&tail[..tail_len]);
            prop_assert_eq!(lower_bound(&bytes, begin), first);
            let expected: Vec<_> = row[first..].iter().copied()
                .take_while(|&(token, _)| token < end).collect();
            let mut out = vec![(u32::MAX, 17)];
            let status = checked_search(&bytes, begin, end, &mut out);
            let reached_tail = first + expected.len() == row.len() && tail_len != 0;
            prop_assert_eq!(status, if reached_tail { Err(GramFault::OverRead) } else { Ok(()) });
            prop_assert_eq!(&out[1..], &expected);
            prop_assert_eq!(out[0], (u32::MAX, 17));
        }
    }

    #[test]
    fn unpartitioned_records_use_std_without_cleaning(
        total in any::<u32>(),
        row in prop::collection::vec((any::<u32>(), any::<u32>()), 0..128),
        probe in any::<u32>(),
        tail in any::<[u8; 7]>(),
    ) {
        let mut bytes = gram(total, &row);
        bytes.extend_from_slice(&tail);
        let before = bytes.clone();
        // Unpartitioned input has no promised search position. It must
        // still answer an in-bounds position without changing storage.
        let first = lower_bound(&bytes, probe);
        prop_assert!(first <= row.len());
        prop_assert_eq!(retrieve_all(&bytes).collect::<Vec<_>>(), row);
        prop_assert_eq!(bytes, before);
    }
}

/// N2 refusals 4 and 6 (register row 98): the pin's `get_freq` walk
/// (`ngram.cpp:241-248`) answers before the last whole item, or reads
/// past a partial tail, or past a value shorter than its total.
#[test]
fn checked_get_freq_faults_only_where_the_pins_walk_leaves_the_value() {
    let mut tailed = gram(9, &[(2, 4), (5, 5)]);
    tailed.extend_from_slice(&[1, 2, 3]);
    // A hit or a larger item stops the walk inside the whole items.
    assert_eq!(checked_get_freq(&tailed, 2), Ok(Some(4)));
    assert_eq!(checked_get_freq(&tailed, 3), Ok(None));
    // Past the last whole item the walk meets the tail.
    assert_eq!(checked_get_freq(&tailed, 6), Err(GramFault::OverRead));
    // An item-less value with a tail: `lower_bound` is the begin.
    let mut itemless = gram(0, &[]);
    itemless.push(7);
    assert_eq!(checked_get_freq(&itemless, 1), Err(GramFault::OverRead));
    // Shorter than the total: `begin` already lies past `end`.
    assert_eq!(checked_get_freq(&[1, 0], 1), Err(GramFault::OverRead));
    assert_eq!(checked_get_freq(&[], 1), Err(GramFault::OverRead));
    // A whole value walks off its end as `get_freq` does.
    assert_eq!(checked_get_freq(&gram(9, &[(2, 4)]), 6), Ok(None));
}

/// N2 refusals 2 and 6: `search` reads the total (`ngram.cpp:165`),
/// which asserts on a short value, and then walks until an item at or
/// past the range end (`:167-173`), reading past a partial tail.
#[test]
fn checked_search_asserts_short_and_faults_past_a_partial_tail() {
    let mut out = Vec::new();
    assert_eq!(
        checked_search(&[1, 0, 0], 1, 2, &mut out),
        Err(GramFault::Short)
    );
    let mut tailed = gram(9, &[(2, 4), (5, 5)]);
    tailed.extend_from_slice(&[1]);
    out.clear();
    assert_eq!(checked_search(&tailed, 2, 5, &mut out), Ok(()));
    assert_eq!(out, [(2, 4)]);
    out.clear();
    assert_eq!(
        checked_search(&tailed, 2, 6, &mut out),
        Err(GramFault::OverRead)
    );
}

#[test]
fn empty_and_short_records_keep_their_refusal_contracts() {
    for len in 0..super::TOTAL_LEN {
        let mut bytes = vec![0xa5; len];
        let before = bytes.clone();
        let mut out = vec![(7, 9)];
        assert_eq!(lower_bound(&bytes, 0), 0);
        assert_eq!(get_freq(&bytes, 0), None);
        assert_eq!(checked_get_freq(&bytes, 0), Err(GramFault::OverRead));
        assert_eq!(
            checked_search(&bytes, 0, 1, &mut out),
            Err(GramFault::Short)
        );
        assert_eq!(out, [(7, 9)]);
        assert!(!set_freq(&mut bytes, 0, 1));
        assert_eq!(bytes, before);
    }
    let mut empty = new_gram();
    let mut out = vec![(7, 9)];
    assert_eq!(lower_bound(&empty, u32::MAX), 0);
    assert_eq!(checked_get_freq(&empty, 0), Ok(None));
    assert_eq!(checked_search(&empty, 0, 1, &mut out), Ok(()));
    assert_eq!(out, [(7, 9)]);
    assert!(insert_freq(&mut empty, 3, 4));
    assert!(!insert_freq(&mut empty, 3, 5));
    assert_eq!(items(&empty), [(3, 4)]);
}

/// A partitioned predicate need not imply a globally sorted row.
/// The physical walk still yields later smaller items.
#[test]
fn a_partitioned_search_preserves_the_unsorted_suffix() {
    let bytes = gram(1, &[(0, 1), (3, 2), (2, 7)]);
    let mut out = Vec::new();
    assert_eq!(checked_search(&bytes, 2, 4, &mut out), Ok(()));
    assert_eq!(out, [(3, 2), (2, 7)]);
}

#[test]
fn unpartitioned_mutations_start_from_the_std_partition() {
    let row = [(9, 4), (3, 6), (7, 2)];
    let mut bytes = gram(12, &row);
    let first = lower_bound(&bytes, 3);
    let hit = row[first..]
        .iter()
        .take_while(|&&(token, _)| token <= 3)
        .position(|&(token, _)| token == 3)
        .map(|at| first + at);
    assert_eq!(get_freq(&bytes, 3), hit.map(|at| row[at].1));
    assert_eq!(set_freq(&mut bytes, 3, 69), hit.is_some());
    let mut expected = row.to_vec();
    if let Some(at) = hit {
        expected[at].1 = 69;
    }
    assert_eq!(items(&bytes), expected);
    assert_eq!(get_total_freq(&bytes), Some(12));
}

/// Q2: get/set act on the first match; mask removes every match and
/// subtracts each frequency; retrieve_all keeps stored order.
#[test]
fn duplicates_are_kept_and_masked_together() {
    let mut bytes = gram(20, &[(5, 3), (5, 7), (6, 10)]);
    assert_eq!(get_freq(&bytes, 5), Some(3));
    assert!(set_freq(&mut bytes, 5, 4));
    assert_eq!(items(&bytes), [(5, 4), (5, 7), (6, 10)]);
    assert!(!insert_freq(&mut bytes, 5, 1));
    assert_eq!(mask_out(&mut bytes, u32::MAX, 5), 2);
    assert_eq!(items(&bytes), [(6, 10)]);
    assert_eq!(get_total_freq(&bytes), Some(9));
}

#[test]
fn row_edits_keep_the_stored_total_and_half_open_order() {
    // tests/storage/test_ngram.cpp:12-49 at 074a2219.
    let mut row = gram(16, &[]);
    for (token, count) in [(2, 1), (6, 2), (4, 4), (3, 8), (1, 16)] {
        assert!(insert_freq(&mut row, token, count));
    }
    assert!(set_freq(&mut row, 3, 32));
    assert!(insert_freq(&mut row, 5, 8));
    assert_eq!(get_total_freq(&row), Some(16));
    assert_eq!(
        items(&row),
        [(1, 16), (2, 1), (3, 32), (4, 4), (5, 8), (6, 2)]
    );
}

/// Removal subtracts from the stored total with wrap; a mask that
/// matches nothing leaves the bytes as they were.
#[test]
fn mask_out_subtracts_from_the_stored_total_with_wrap() {
    let mut bytes = gram(1, &[(1, 2), (2, 3)]);
    assert_eq!(mask_out(&mut bytes, u32::MAX, 2), 1);
    assert_eq!(get_total_freq(&bytes), Some(1_u32.wrapping_sub(3)));
    let before = bytes.clone();
    assert_eq!(mask_out(&mut bytes, u32::MAX, 7), 0);
    assert_eq!(bytes, before);
}

/// Walks stop at the last whole item; a partial tail is never read, and
/// a short value removes nothing.
#[test]
fn walks_stop_at_the_last_whole_item() {
    let mut bytes = gram(5, &[(1, 5)]);
    bytes.extend_from_slice(&[0xff; 3]);
    assert_eq!(get_length(&bytes), 1);
    assert_eq!(items(&bytes), [(1, 5)]);
    let mut short = vec![1_u8, 2];
    assert_eq!(mask_out(&mut short, 0, 0), 0);
    assert_eq!(short, [1, 2]);
    assert_eq!(get_total_freq(&short), None);
}

/// Constitution §4: no index panics. One that overflows the offset
/// arithmetic, or lies past the last whole item, answers `None`.
#[test]
fn a_huge_item_index_answers_none_without_panicking() {
    let bytes = gram(7, &[(3, 7)]);
    assert_eq!(item_offset(usize::MAX), None);
    assert_eq!(item_offset(usize::MAX / 8 + 1), None);
    for index in [1, usize::MAX / 8, usize::MAX - 1, usize::MAX] {
        assert_eq!(item_token(&bytes, index), None, "token {index}");
        assert_eq!(item_freq(&bytes, index), None, "freq {index}");
    }
    assert_eq!(item_token(&bytes, 0), Some(3));
    assert_eq!(item_freq(&bytes, 0), Some(7));
}

#[test]
fn insert_appends_past_the_largest() {
    let mut bytes = gram(0, &[(2, 1), (4, 1)]);
    assert!(insert_freq(&mut bytes, 9, 2));
    assert!(insert_freq(&mut bytes, 3, 2));
    assert_eq!(items(&bytes), [(2, 1), (3, 2), (4, 1), (9, 2)]);
}

#[test]
fn merge_copies_one_side_and_sums_both() {
    let mut merged = gram(9, &[(1, 1)]);
    assert!(!merge_single_gram(&mut merged, None, None));
    assert_eq!(items(&merged), [(1, 1)]);
    let user = gram(3, &[(2, 3)]);
    assert!(merge_single_gram(&mut merged, None, Some(&user)));
    assert_eq!(merged, user);
    let system = gram(u32::MAX, &[(1, 4), (2, 5)]);
    assert!(merge_single_gram(&mut merged, Some(&system), Some(&user)));
    assert_eq!(get_total_freq(&merged), Some(2));
    assert_eq!(items(&merged), [(1, 4), (2, 8)]);
}
