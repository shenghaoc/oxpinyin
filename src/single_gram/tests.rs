use super::{
    GramFault, checked_get_freq, checked_search, get_freq, get_length, get_total_freq, insert_freq,
    lower_bound, mask_out, merge_single_gram, new_gram, retrieve_all, set_freq,
};

fn gram(total: u32, items: &[(u32, u32)]) -> Vec<u8> {
    super::encode_single_gram(total, items)
}

fn items(bytes: &[u8]) -> Vec<(u32, u32)> {
    retrieve_all(bytes).collect()
}

const SORTED_ROW: &[(u32, u32)] = &[(3, 4), (7, 2), (7, 9), (11, 5)];

#[rstest::rstest]
#[case::empty(&[], 0, 1, &[], 0, &[], None)]
#[case::below_all(SORTED_ROW, 0, u32::MAX, &[], 0, SORTED_ROW, None)]
#[case::equal_first(SORTED_ROW, 3, 7, &[], 0, &[(3, 4)], None)]
#[case::between_tokens(SORTED_ROW, 4, 8, &[], 1, &[(7, 2), (7, 9)], None)]
#[case::equal_duplicate(SORTED_ROW, 7, 11, &[], 1, &[(7, 2), (7, 9)], None)]
#[case::above_all(SORTED_ROW, 12, 13, &[], 4, &[], None)]
#[case::maximum(&[(0, 12), (u32::MAX, u32::MAX)], u32::MAX, u32::MAX, &[], 1, &[], None)]
#[case::inverted_range(SORTED_ROW, 7, 3, &[], 1, &[], None)]
#[case::stops_before_tail(SORTED_ROW, 3, 7, &[4], 0, &[(3, 4)], None)]
#[case::reaches_tail(SORTED_ROW, 0, u32::MAX, &[4], 0, SORTED_ROW, Some(GramFault::OverRead))]
fn partitioned_records_find_the_first_not_less(
    #[case] row: &[(u32, u32)],
    #[case] begin: u32,
    #[case] end: u32,
    #[case] tail: &[u8],
    #[case] first: usize,
    #[case] expected: &[(u32, u32)],
    #[case] fault: Option<GramFault>,
) {
    // Pin 074a2219: ngram.cpp:153–176; compatibility row 98 for a reached partial tail.
    let mut bytes = gram(u32::MAX, row);
    bytes.extend_from_slice(tail);
    assert_eq!(lower_bound(&bytes, begin), first);
    let mut out = vec![(u32::MAX, 17)];
    assert_eq!(
        checked_search(&bytes, begin, end, &mut out),
        fault.map_or(Ok(()), Err)
    );
    assert_eq!(&out[1..], expected);
    assert_eq!(out[0], (u32::MAX, 17));
}

#[test]
fn unpartitioned_records_use_std_without_cleaning() {
    // Rust lower_bound contract and the no-ported-internals ruling: no position is promised on unpartitioned input.
    let row = [(9, 4), (3, 6), (7, 2)];
    let bytes = gram(19, &row);
    let before = bytes.clone();
    assert!(lower_bound(&bytes, 9) <= row.len());
    assert_eq!(items(&bytes), row);
    assert_eq!(bytes, before);
}

#[test]
fn checked_get_freq_faults_only_where_the_pins_walk_leaves_the_value() {
    // Pin 074a2219: ngram.cpp:231–250; compatibility row 98 for a reached partial tail.
    let mut tailed = gram(9, &[(2, 4), (5, 5)]);
    tailed.extend_from_slice(&[1, 2, 3]);
    let whole = gram(9, &[(2, 4)]);
    for (bytes, token, expected) in [
        (&tailed, 2, Ok(Some(4))),
        (&tailed, 3, Ok(None)),
        (&tailed, 6, Err(GramFault::OverRead)),
        (&whole, 6, Ok(None)),
    ] {
        assert_eq!(checked_get_freq(bytes, token), expected);
    }
}

#[test]
fn empty_and_short_records_keep_their_refusal_contracts() {
    // Pin 074a2219: ngram.cpp:36–39,165–173,239–248; compatibility row 85 for a short total.
    let short = [0xa5; 3];
    let mut out = vec![(7, 9)];
    assert_eq!(lower_bound(&short, 0), 0);
    assert_eq!(get_freq(&short, 0), None);
    assert_eq!(checked_get_freq(&short, 0), Err(GramFault::OverRead));
    assert_eq!(
        checked_search(&short, 0, 1, &mut out),
        Err(GramFault::Short)
    );
    assert_eq!(out, [(7, 9)]);
    let empty = new_gram();
    assert_eq!(lower_bound(&empty, u32::MAX), 0);
    assert_eq!(checked_get_freq(&empty, 0), Ok(None));
    assert_eq!(checked_search(&empty, 0, 1, &mut out), Ok(()));
    assert_eq!(out, [(7, 9)]);
}

#[test]
fn a_partitioned_search_preserves_the_unsorted_suffix() {
    // Pin 074a2219: ngram.cpp:161–173; a partitioned lower-bound predicate does not sort the physical walk.
    let bytes = gram(1, &[(0, 1), (3, 2), (2, 7)]);
    let mut out = Vec::new();
    assert_eq!(checked_search(&bytes, 2, 4, &mut out), Ok(()));
    assert_eq!(out, [(3, 2), (2, 7)]);
}

#[test]
fn unpartitioned_mutations_start_from_the_std_partition() {
    // Rust lower_bound contract; pin 074a2219 ngram.cpp:231–271 keeps the get/set walk and stored total.
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

#[test]
fn duplicates_are_kept_and_masked_together() {
    // Pin 074a2219: ngram.cpp:76–103,178–205,231–271.
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
    // Pin 074a2219: tests/storage/test_ngram.cpp:12–49; src/storage/ngram.cpp:178–205,252–271.
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

#[test]
fn mask_out_subtracts_from_the_stored_total_with_wrap() {
    // Pin 074a2219: ngram.cpp:76–103.
    let mut bytes = gram(1, &[(1, 2), (2, 3)]);
    assert_eq!(mask_out(&mut bytes, u32::MAX, 2), 1);
    assert_eq!(get_total_freq(&bytes), Some(1_u32.wrapping_sub(3)));
    let before = bytes.clone();
    assert_eq!(mask_out(&mut bytes, u32::MAX, 7), 0);
    assert_eq!(bytes, before);
}

#[test]
fn walks_stop_at_the_last_whole_item() {
    // Pin 074a2219: ngram.cpp:133–151; compatibility row 98 retains whole records only.
    let mut bytes = gram(5, &[(1, 5)]);
    bytes.extend_from_slice(&[0xff; 3]);
    assert_eq!(get_length(&bytes), 1);
    assert_eq!(items(&bytes), [(1, 5)]);
}

#[test]
fn merge_copies_one_side_and_sums_both() {
    // Pin 074a2219: ngram.cpp:277–352.
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

#[test]
fn dense_mask_subtracts_removed_frequencies_and_keeps_partial_tail() {
    let mut bytes = gram(3, &[(0, 2), (1, 9), (2, 5), (3, 4), (4, 7), (1, 8)]);
    bytes.extend([0xaa, 0xbb]);
    assert_eq!(mask_out(&mut bytes, 1, 0), 3);
    let mut expected = gram(3_u32.wrapping_sub(14), &[(1, 9), (3, 4), (1, 8)]);
    expected.extend([0xaa, 0xbb]);
    assert_eq!(bytes, expected);
}
