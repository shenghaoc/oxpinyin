use super::{MergedGram, NbestStepRow, PhraseIndexRanges, TrellisPathMemo, sort_tokens_by_library};
use crate::single_gram::GramFault;
use crate::vocab::phrase_index_library_index;

#[test]
fn tokens_sort_by_their_masked_library_and_keep_their_order_within_it() {
    // Pin 074a2219: phrase_large_table3.cpp:42, phrase_large_table3.h:83–91, novel_types.h:44.
    let mut tokens = [
        0x0700_0002,
        0x8100_0001,
        0x0100_0009,
        0x0700_0001,
        0x0100_0003,
        0x1100_0000,
    ];
    sort_tokens_by_library(&mut tokens);
    assert_eq!(
        tokens,
        [
            0x8100_0001,
            0x0100_0009,
            0x0100_0003,
            0x1100_0000,
            0x0700_0002,
            0x0700_0001
        ]
    );
}

#[test]
fn trellis_path_memo_keeps_the_first_value_and_nothing_on_failure() {
    // Rust contract: TrellisPathMemo retries failed initialization and retains its first value.
    let memo = TrellisPathMemo::<u8>::default();
    assert_eq!(memo.get_or_try_init(|| Err::<u8, _>("boom")), Err("boom"));
    assert_eq!(memo.get_or_try_init(|| Ok::<_, ()>(7)), Ok(&7));
    assert_eq!(
        memo.get_or_try_init::<()>(|| panic!("init runs on a filled memo")),
        Ok(&7)
    );
}

fn spans(ranges: &PhraseIndexRanges<u32>) -> Vec<(u8, u32, u32)> {
    ranges
        .iter()
        .map(|range| {
            (
                phrase_index_library_index(range.first()),
                range.first(),
                range.last(),
            )
        })
        .collect()
}

fn call(tokens: &[u32]) -> impl Iterator<Item = (u32, Option<u32>)> + '_ {
    tokens.iter().map(|&token| (token, Some(token)))
}

#[rstest::rstest]
#[case::item_order(&[5, 6, 7, 9, 8, 8], &[(0, 5, 7), (0, 9, 9), (0, 8, 8), (0, 8, 8)], &[5, 6, 7, 9, 8, 8])]
#[case::library_boundary(
    &[0x0200_0001, 0x00ff_ffff, 0x0100_0000, 0x0200_0002],
    &[(0, 0x00ff_ffff, 0x00ff_ffff), (1, 0x0100_0000, 0x0100_0000),
      (2, 0x0200_0001, 0x0200_0001), (2, 0x0200_0002, 0x0200_0002)],
    &[0x00ff_ffff, 0x0100_0000, 0x0200_0001, 0x0200_0002]
)]
fn convert_merges_consecutive_runs_in_item_order(
    #[case] tokens: &[u32],
    #[case] expected_ranges: &[(u8, u32, u32)],
    #[case] expected_walk: &[u32],
) {
    // Pin 074a2219: chewing_large_table2.h:91–133; phonetic_lookup.h:560–572,615–632.
    let mut ranges = PhraseIndexRanges::new();
    ranges.convert(call(tokens));
    assert_eq!(spans(&ranges), expected_ranges);
    assert_eq!(
        ranges
            .walk_tokens()
            .map(|(_, &entry)| entry)
            .collect::<Vec<_>>(),
        expected_walk
    );
}

#[test]
fn convert_builds_no_run_across_calls() {
    // Pin 074a2219: chewing_large_table2.h:128–132; Rust append_from maps and drains in range order.
    let mut ranges = PhraseIndexRanges::new();
    ranges.convert(call(&[5, 6]));
    ranges.convert(call(&[7, 4]));
    assert_eq!(spans(&ranges), [(0, 5, 6), (0, 7, 7), (0, 4, 4)]);
    let mut merged = PhraseIndexRanges::new();
    merged.convert(call(&[1]));
    merged.append_from(&mut ranges, |token| token * 10);
    assert_eq!(spans(&merged), [(0, 1, 1), (0, 5, 6), (0, 7, 7), (0, 4, 4)]);
    assert_eq!(
        merged
            .walk_tokens()
            .map(|(_, &entry)| entry)
            .collect::<Vec<_>>(),
        [1, 50, 60, 70, 40]
    );
    assert!(ranges.iter().next().is_none());
}

#[test]
fn step_row_reuses_hits_and_misses_but_retries_errors() {
    // Rust contract: NbestStepRow caches successful hits and misses, but propagates and retries errors.
    let mut row = NbestStepRow::default();
    assert_eq!(
        row.get_or_try_init::<&str>(|| Err("empty value")).err(),
        Some("empty value")
    );
    let first = row
        .get_or_try_init::<&str>(|| Ok(Some(MergedGram::new(0, vec![(9, 0), (3, 7)]))))
        .unwrap()
        .unwrap() as *const MergedGram;
    let cached = row
        .get_or_try_init::<&str>(|| panic!("hit reloaded"))
        .unwrap()
        .unwrap() as *const MergedGram;
    assert_eq!(cached, first);
    let mut miss = NbestStepRow::default();
    assert!(miss.get_or_try_init::<&str>(|| Ok(None)).unwrap().is_none());
    assert!(
        miss.get_or_try_init::<&str>(|| panic!("miss reloaded"))
            .unwrap()
            .is_none()
    );
}

#[test]
fn a_range_ending_at_the_last_token_searches_nothing_as_the_pin_does() {
    // Pin 074a2219: chewing_large_table2.h:115,120; ngram.cpp:167–173.
    let gram = MergedGram::new(2, vec![(u32::MAX - 1, 1), (u32::MAX, 1)]);
    for (first, last, expected) in [
        (u32::MAX, u32::MAX, &[][..]),
        (u32::MAX - 1, u32::MAX, &[]),
        (u32::MAX - 1, u32::MAX - 1, &[(u32::MAX - 1, 1)][..]),
    ] {
        let mut items = Vec::new();
        gram.search_into(first, last, &mut items).unwrap();
        assert_eq!(items, expected);
    }
}

#[test]
fn a_merge_the_pin_cannot_read_answers_its_fault() {
    // Pin 074a2219: ngram.cpp:303–349; compatibility rows 85 and 98.
    let system = gram(4, &[(2, 4)]);
    for (value, fault, alone_total, alone_search) in [
        (
            vec![1, 2],
            GramFault::Short,
            Err(GramFault::Short),
            GramFault::Short,
        ),
        (
            vec![0, 0, 0, 0, 9],
            GramFault::OverRead,
            Ok(0),
            GramFault::OverRead,
        ),
    ] {
        let merged = MergedGram::merge(Some(system.clone()), Some(value.clone())).unwrap();
        assert_eq!(merged.fault(), Some(fault));
        assert_eq!(merged.total(), Err(fault));
        assert_eq!(merged.get_freq(2), Err(fault));
        assert_eq!(merged.search_into(0, 9, &mut Vec::new()), Err(fault));
        let alone = MergedGram::merge(None, Some(value.clone())).unwrap();
        assert_eq!(alone.fault(), None);
        assert_eq!(alone.to_bytes(), value);
        assert_eq!(alone.total(), alone_total);
        assert_eq!(alone.get_freq(2), Err(GramFault::OverRead));
        assert_eq!(alone.search_into(0, 9, &mut Vec::new()), Err(alone_search));
    }
    assert_eq!(
        MergedGram::merge(Some(system.clone()), None)
            .unwrap()
            .to_bytes(),
        system
    );
    assert!(MergedGram::merge(None, None).is_none());
}

fn gram(total: u32, records: &[(u32, u32)]) -> Vec<u8> {
    crate::single_gram::encode_single_gram(total, records)
}

#[rstest::rstest]
#[case::equal_and_wrapping(
    (0x8000_0003, &[(3, u32::MAX), (3, 3), (3, 0), (3, 1)][..]),
    (9, &[(3, u32::MAX), (3, 3), (3, 0), (3, 1)][..]),
    (0x8000_000c, &[(3, u32::MAX - 1), (3, 6), (3, 0), (3, 2)][..])
)]
#[case::user_remainder(
    (0x8000_0017, &[(2, u32::MAX), (3, 23), (3, 0), (3, 1)][..]),
    (69, &[(5, u32::MAX), (3, 23), (2, 0), (3, 1)][..]),
    (0x8000_005c, &[(2, u32::MAX), (3, 23), (3, 0), (3, 1),
        (5, u32::MAX), (3, 23), (2, 0), (3, 1)][..])
)]
#[case::system_remainder(
    (0, &[(3, 0), (1, 4), (3, 9)][..]),
    (0, &[(2, 7), (3, u32::MAX)][..]),
    (0, &[(2, 7), (3, u32::MAX), (1, 4), (3, 9)][..])
)]
fn row_merge_is_the_pins_two_pointer_interleave_for_arbitrary_rows(
    #[case] system: (u32, &[(u32, u32)]),
    #[case] user: (u32, &[(u32, u32)]),
    #[case] expected: (u32, &[(u32, u32)]),
) {
    // Pin 074a2219: ngram.cpp:303–349; these merge branches lack a SingleGram-local witness.
    let merged =
        MergedGram::merge(Some(gram(system.0, system.1)), Some(gram(user.0, user.1))).unwrap();
    assert_eq!(merged.to_bytes(), gram(expected.0, expected.1));
}
