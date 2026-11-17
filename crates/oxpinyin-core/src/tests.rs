use std::collections::BTreeMap;

use super::{PhraseIndexRanges, TrellisPathMemo, phrase_index_library, sort_tokens_by_library};
use crate::fixture_doubles::{FixtureDictionary, FixtureError, FixtureLanguageModel};
use crate::{Dictionary, SyllableKey};

/// `add_index` files a token under `PHRASE_INDEX_LIBRARY_INDEX(token)`
/// and keeps its whole value (`phrase_large_table3.cpp:42`,
/// `novel_types.h:44` at the pin `074a2219`); `reduce_tokens` appends
/// the arrays in library order (`phrase_large_table3.h:83-91`). A token
/// with high bits set files with its masked library, in the given order.
#[test]
fn tokens_sort_by_their_masked_library_and_keep_their_order_within_it() {
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
            0x0700_0001,
        ]
    );
}

/// A memo runs `init` once and keeps what it made; a failed `init`
/// keeps nothing, so the next call tries again.
#[test]
fn trellis_path_memo_keeps_the_first_value_and_nothing_on_failure() {
    let memo = TrellisPathMemo::<u8>::default();
    assert_eq!(memo.get_or_try_init(|| Err::<u8, _>("boom")), Err("boom"));
    assert_eq!(memo.get_or_try_init(|| Ok::<_, ()>(7)), Ok(&7));
    assert_eq!(
        memo.get_or_try_init::<()>(|| panic!("init runs on a filled memo")),
        Ok(&7)
    );
}

fn spans(ranges: &PhraseIndexRanges<u32>, library: usize) -> Vec<(u32, u32)> {
    ranges
        .ranges(library)
        .iter()
        .map(|range| (range.first(), range.last()))
        .collect()
}

fn call(tokens: &[u32]) -> Vec<(u32, Option<u32>)> {
    tokens.iter().map(|&token| (token, Some(token))).collect()
}

/// `convert` merges a token one past the open range and opens a new
/// range otherwise — including for a repeated or descending token.
#[test]
fn convert_merges_consecutive_runs_in_item_order() {
    let mut ranges = PhraseIndexRanges::new();
    ranges.convert(call(&[5, 6, 7, 9, 8, 8]));
    assert_eq!(spans(&ranges, 0), [(5, 7), (9, 9), (8, 8), (8, 8)]);
    let walked: Vec<u32> = ranges.walk().copied().collect();
    assert_eq!(walked, [5, 6, 7, 9, 8, 8]);
}

/// A run never crosses a library nibble, and each range lands in its
/// own library's array; the walk visits libraries in ascending order.
#[test]
fn convert_splits_runs_at_library_boundaries() {
    let top = 0x00ff_ffff;
    let mut ranges = PhraseIndexRanges::new();
    ranges.convert(call(&[0x0200_0001, top, top + 1, 0x0200_0002]));
    assert_eq!(phrase_index_library(top + 1), 1);
    assert_eq!(spans(&ranges, 0), [(top, top)]);
    assert_eq!(spans(&ranges, 1), [(top + 1, top + 1)]);
    assert_eq!(
        spans(&ranges, 2),
        [(0x0200_0001, 0x0200_0001), (0x0200_0002, 0x0200_0002)]
    );
    let walked: Vec<u32> = ranges.walk().copied().collect();
    assert_eq!(walked, [top, top + 1, 0x0200_0001, 0x0200_0002]);
}

/// Each call closes its last range: `[5,6]` and `[7]` from two calls
/// stay two ranges, in append order.
#[test]
fn convert_builds_no_run_across_calls() {
    let mut ranges = PhraseIndexRanges::new();
    ranges.convert(call(&[5, 6]));
    ranges.convert(call(&[7, 4]));
    assert_eq!(spans(&ranges, 0), [(5, 6), (7, 7), (4, 4)]);
    let mut merged = PhraseIndexRanges::new();
    merged.convert(call(&[1]));
    merged.append_from(&mut ranges, |token| token * 10);
    assert_eq!(spans(&merged, 0), [(1, 1), (5, 6), (7, 7), (4, 4)]);
    let walked: Vec<u32> = merged.walk().copied().collect();
    assert_eq!(walked, [1, 50, 60, 70, 40]);
    assert!(ranges.iter().next().is_none());
}

/// `trellis_records`' contract is the walk order, not item order: a
/// row that interleaves libraries comes back library by library, each
/// library's ranges in append order.
#[test]
fn walk_entries_follow_libraries_then_ranges_on_an_interleaved_row() {
    let mut ranges = PhraseIndexRanges::new();
    ranges.convert(call(&[0x0200_0005, 0x0100_0007, 0x0200_0006, 0x0100_0003]));
    ranges.convert(call(&[0x0200_0001, 0x0100_0008]));
    assert_eq!(
        ranges.walk().copied().collect::<Vec<_>>(),
        [
            0x0100_0007,
            0x0100_0003,
            0x0100_0008,
            0x0200_0005,
            0x0200_0006,
            0x0200_0001,
        ]
    );
}

/// An unresolved item keeps its token in the range, so `entry` finds
/// its neighbours at the right offsets.
#[test]
fn unresolved_items_keep_their_place_in_the_range() {
    let mut ranges = PhraseIndexRanges::new();
    ranges.convert([(3, Some(30)), (4, None), (5, Some(50))]);
    let [range] = ranges.ranges(0) else {
        panic!("one range: {:?}", ranges.ranges(0));
    };
    assert_eq!((range.first(), range.last()), (3, 5));
    assert_eq!(ranges.entry(range, 3), Some(&30));
    assert_eq!(ranges.entry(range, 4), None);
    assert_eq!(ranges.entry(range, 5), Some(&50));
    assert_eq!(ranges.entry(range, 6), None);
    assert_eq!(ranges.walk().copied().collect::<Vec<_>>(), [30, 50]);
}

// Merged-row and step-row behaviour against the shared fixture doubles.

#[test]
fn step_row_reuses_hits_and_misses_but_retries_errors() {
    use crate::{MergedGram, NbestStepRow};
    let mut row = NbestStepRow::default();
    assert_eq!(
        row.get_or_try_init::<&str>(|| Err("empty value")).err(),
        Some("empty value")
    );
    // The row keeps the order given: `lower_bound(9)` over `[9, 3]` lands
    // past the end and `lower_bound(3)` on the 9, so the pin's `get_freq`
    // misses both (`ngram.cpp:231-250`).
    let gram = row
        .get_or_try_init::<&str>(|| Ok(Some(MergedGram::new(0, vec![(9, 0), (3, 7)]))))
        .unwrap()
        .unwrap();
    assert_eq!(gram.get_freq(9), Ok(None));
    assert_eq!(gram.get_freq(4), Ok(None));
    assert_eq!(
        row.get_or_try_init::<&str>(|| panic!("hit reloaded"))
            .unwrap()
            .unwrap()
            .get_freq(3),
        Ok(None)
    );
    let mut miss = NbestStepRow::default();
    assert!(miss.get_or_try_init::<&str>(|| Ok(None)).unwrap().is_none());
    assert!(
        miss.get_or_try_init::<&str>(|| panic!("miss reloaded"))
            .unwrap()
            .is_none()
    );
}

/// `merge_single_gram` over an unsorted system row and a user row
/// (`ngram.cpp:308-349`): `3 > 2` appends the user's 2, the equal 3s sum,
/// and the system's remainder follows, giving `[2, 3, 1, 3]`. The pin's
/// `get_freq` then misses 1 and 2 and finds the second 3; `search` over
/// `1..=3` yields every item, duplicates included.
#[test]
fn a_merged_row_answers_as_the_pins_merged_chunk() {
    use crate::{MergedGram, NbestStepRow, NbestSuccessors};
    let chunk = |total: u32, items: &[(u32, u32)]| {
        let mut value = total.to_le_bytes().to_vec();
        for (token, count) in items {
            value.extend_from_slice(&token.to_le_bytes());
            value.extend_from_slice(&count.to_le_bytes());
        }
        value
    };
    let system = chunk(0, &[(3, 0), (1, 4), (3, 9)]);
    let user = chunk(0, &[(2, 7), (3, u32::MAX)]);
    let mut row = NbestStepRow::default();
    let gram = row
        .get_or_try_init::<&str>(|| Ok(MergedGram::merge(Some(system), Some(user))))
        .unwrap()
        .unwrap();
    assert_eq!(
        gram.to_bytes(),
        chunk(0, &[(2, 7), (3, u32::MAX), (1, 4), (3, 9)])
    );
    assert_eq!(gram.total(), Ok(0));
    assert_eq!(gram.get_freq(1), Ok(None));
    assert_eq!(gram.get_freq(2), Ok(None));
    assert_eq!(gram.get_freq(3), Ok(Some(9)));
    assert_eq!(gram.get_freq(4), Ok(None));
    let mut items = Vec::new();
    gram.search_into(1, 3, &mut items).unwrap();
    assert_eq!(items, [(2, 7), (3, u32::MAX), (1, 4), (3, 9)]);
    assert!(matches!(
        NbestSuccessors::Row(gram),
        NbestSuccessors::Row(row) if row.fault().is_none()
    ));
    assert!(MergedGram::merge(None, None).is_none());
}

/// A range ending at `u32::MAX`: the pin's exclusive end is the `guint32`
/// `token + 1` (`chewing_large_table2.h:115`, `:120` at the pin
/// `074a2219`), zero, and `search` stops at the first item at or past it
/// (`ngram.cpp:168`) — every item — so the walk yields nothing.
#[test]
fn a_range_ending_at_the_last_token_searches_nothing_as_the_pin_does() {
    use crate::MergedGram;
    let gram = MergedGram::new(2, vec![(u32::MAX - 1, 1), (u32::MAX, 1)]);
    let mut items = Vec::new();
    gram.search_into(u32::MAX, u32::MAX, &mut items).unwrap();
    gram.search_into(u32::MAX - 1, u32::MAX, &mut items)
        .unwrap();
    assert!(items.is_empty());
    gram.search_into(u32::MAX - 1, u32::MAX - 1, &mut items)
        .unwrap();
    assert_eq!(items, [(u32::MAX - 1, 1)]);
}

/// N2 refusals 1 and 5 at the merge (`ngram.cpp:303-349`): both grams
/// present, a user value shorter than its total asserts and a partial
/// tail is consumed past its end; every read of the row then answers the
/// fault. With no system gram the user value is the row as stored, and
/// the fault surfaces at the first read that meets it.
#[test]
fn a_merge_the_pin_cannot_read_answers_its_fault() {
    use crate::{GramFault, MergedGram};
    let system = {
        let mut value = 4_u32.to_le_bytes().to_vec();
        value.extend_from_slice(&2_u32.to_le_bytes());
        value.extend_from_slice(&4_u32.to_le_bytes());
        value
    };
    let short = MergedGram::merge(Some(system.clone()), Some(vec![1, 2])).unwrap();
    assert_eq!(short.fault(), Some(GramFault::Short));
    assert_eq!(short.total(), Err(GramFault::Short));
    assert_eq!(short.get_freq(2), Err(GramFault::Short));
    let mut tailed = 0_u32.to_le_bytes().to_vec();
    tailed.push(9);
    let over = MergedGram::merge(Some(system.clone()), Some(tailed.clone())).unwrap();
    assert_eq!(over.fault(), Some(GramFault::OverRead));
    let mut items = Vec::new();
    assert_eq!(over.search_into(0, 9, &mut items), Err(GramFault::OverRead));
    // No system gram: the user value itself, read as the pin reads it.
    let alone = MergedGram::merge(None, Some(vec![1, 2])).unwrap();
    assert_eq!(alone.fault(), None);
    assert_eq!(alone.to_bytes(), [1, 2]);
    assert_eq!(alone.total(), Err(GramFault::Short));
    assert_eq!(alone.get_freq(2), Err(GramFault::OverRead));
    assert_eq!(alone.search_into(0, 9, &mut items), Err(GramFault::Short));
    let alone = MergedGram::merge(None, Some(tailed)).unwrap();
    assert_eq!(alone.total(), Ok(0));
    assert_eq!(alone.get_freq(2), Err(GramFault::OverRead));
    // The system gram alone is its own bytes.
    assert_eq!(
        MergedGram::merge(Some(system.clone()), None)
            .unwrap()
            .to_bytes(),
        system
    );
}

/// A stored row: the `guint32` total, then `(token, count)` records.
fn gram(total: u32, records: &[(u32, u32)]) -> Vec<u8> {
    let mut bytes = total.to_le_bytes().to_vec();
    for (token, count) in records {
        bytes.extend_from_slice(&token.to_le_bytes());
        bytes.extend_from_slice(&count.to_le_bytes());
    }
    bytes
}

fn merged(system: Option<Vec<u8>>, user: Option<Vec<u8>>) -> Option<Vec<u8>> {
    crate::MergedGram::merge(system, user).map(|row| row.to_bytes())
}

/// `merge_single_gram`'s loop (`ngram.cpp:308-349` at `074a2219`),
/// written out item by item: no sort, no dedup, equal heads summed,
/// remainders appended.
fn pin_merge(system: (u32, &[(u32, u32)]), user: (u32, &[(u32, u32)])) -> Vec<u8> {
    let mut items = Vec::new();
    let (mut left, mut right) = (0, 0);
    while left < system.1.len() && right < user.1.len() {
        let (s, u) = (system.1[left], user.1[right]);
        if s.0 < u.0 {
            items.push(s);
            left += 1;
        } else if s.0 > u.0 {
            items.push(u);
            right += 1;
        } else {
            items.push((s.0, s.1.wrapping_add(u.1)));
            left += 1;
            right += 1;
        }
    }
    items.extend_from_slice(&system.1[left..]);
    items.extend_from_slice(&user.1[right..]);
    gram(system.0.wrapping_add(user.0), &items)
}

#[test]
fn row_merge_is_the_pins_two_pointer_interleave_for_arbitrary_rows() {
    for seed in 0..128_u32 {
        let system = [(seed % 7, u32::MAX), (3, seed), (seed % 5, 0), (3, 1)];
        let user = [(seed % 6, u32::MAX), (3, seed), (seed % 7, 0), (3, 1)];
        let (system_total, user_total) = (seed.wrapping_mul(0x8000_0001), seed.wrapping_mul(3));
        assert_eq!(
            merged(
                Some(gram(system_total, &system)),
                Some(gram(user_total, &user))
            ),
            Some(pin_merge((system_total, &system), (user_total, &user))),
            "seed {seed}"
        );
    }
}

// The fixture doubles' own contract: parsing and lookup.

const VOCAB: &str = include_str!("../../../fixtures/w4/mini-vocab.txt");

fn keys(text: &str) -> Vec<SyllableKey> {
    text.split(',')
        .map(|key| SyllableKey::from_text(key).expect("frozen key"))
        .collect()
}

fn dictionary() -> FixtureDictionary {
    FixtureDictionary::parse(VOCAB).expect("the committed fixture parses")
}

#[test]
fn the_committed_vocabulary_loads() {
    let dictionary = dictionary();
    assert_eq!(dictionary.len(), 90);
    assert!(!dictionary.is_empty());
    assert_eq!(dictionary.max_keys(), 3);
}

#[test]
fn lookup_preserves_the_captured_candidate_order() {
    let dictionary = dictionary();
    let entries = dictionary.lookup(&keys("ni")).expect("lookup cannot fail");
    let texts: Vec<&str> = entries
        .iter()
        .filter_map(crate::PhraseEntry::text_str)
        .collect();
    assert_eq!(
        texts,
        ["你", "尼", "呢", "泥", "妮", "拟", "逆", "倪", "腻", "溺"]
    );

    let phrase = dictionary.lookup(&keys("zhong,guo,ren")).expect("lookup");
    assert_eq!(phrase.len(), 1);
    assert_eq!(phrase[0].text(), "中国人");
}

#[test]
fn an_absent_key_sequence_is_an_empty_lookup() {
    let dictionary = dictionary();
    assert!(
        dictionary
            .lookup(&keys("zhuang"))
            .expect("lookup cannot fail")
            .is_empty()
    );
    assert!(
        dictionary
            .lookup(&[])
            .expect("lookup cannot fail")
            .is_empty()
    );
}

#[test]
fn lookups_are_deterministic() {
    let dictionary = dictionary();
    for sequence in ["ni", "ni,hao", "zhong,guo", "xi,an", "fang,an"] {
        let first = dictionary.lookup(&keys(sequence)).expect("lookup");
        let second = dictionary.lookup(&keys(sequence)).expect("lookup");
        assert_eq!(first, second, "sequence: {sequence}");
    }
}

#[test]
fn malformed_records_are_reported_with_their_line() {
    let failure = |text: &str| {
        FixtureDictionary::parse(text).expect_err("the record is malformed on purpose")
    };

    assert_eq!(
        failure("token=1\tkeys=ni"),
        FixtureError::MissingField {
            line: 1,
            field: "text"
        }
    );
    assert_eq!(
        failure("token=1\tkeys=qqq\ttext=x\tunigram=1"),
        FixtureError::UnknownKey {
            line: 1,
            key: "qqq".to_owned()
        }
    );
    assert_eq!(
        failure("token=1\tkeys=ni\ttext=x\tunigram=1\ntoken=1\tkeys=ni\ttext=y\tunigram=1"),
        FixtureError::DuplicateToken { line: 2, token: 1 }
    );
    assert_eq!(
        failure("token=1\tkeys=ni\ttext=x\tunigram=nope"),
        FixtureError::InvalidNumber {
            line: 1,
            field: "unigram",
            value: "nope".to_owned()
        }
    );
    assert_eq!(
        FixtureLanguageModel::parse(
            "token=1\tkeys=ni\ttext=x\tunigram=1",
            "prev=1\tnext=2\tcount=1"
        )
        .expect_err("the bigram names an undefined token"),
        FixtureError::UnknownToken { line: 1, token: 2 }
    );

    for message in [
        FixtureError::MissingField {
            line: 1,
            field: "text",
        }
        .to_string(),
        FixtureError::UnknownKey {
            line: 2,
            key: "qqq".to_owned(),
        }
        .to_string(),
        FixtureError::DuplicateToken { line: 3, token: 1 }.to_string(),
        FixtureError::InvalidNumber {
            line: 4,
            field: "unigram",
            value: "nope".to_owned(),
        }
        .to_string(),
        FixtureError::UnknownToken { line: 5, token: 2 }.to_string(),
    ] {
        assert!(message.starts_with("line "), "message: {message}");
    }
}

#[test]
fn comments_and_blank_lines_are_skipped() {
    let dictionary = FixtureDictionary::parse(
        "# header\n\n  \ntoken=1\tkeys=ni\ttext=x\tunigram=1\t# trailing\n",
    )
    .expect("comments parse");
    assert_eq!(dictionary.len(), 1);
}

// The W4 fixture vocabulary is derived from the frozen captures.
//
// `docs/testing/fixture-adapters.md` states the rule; this test is what
// makes it checkable rather than asserted. Every phrase in
// `fixtures/w4/mini-vocab.txt` must be text the pinned oracle actually
// emitted in `fixtures/foundation/f-a.txt` or `f-c.txt`, and every unigram
// weight must be exactly `1000 - 100 × best captured rank`.
//
// Key sequences are the authored part and are deliberately not checked here:
// the capture ranks candidates across every segmentation of an input, so it
// cannot say which keys a candidate covers. That is recorded in the finding.

const F_A: &str = include_str!("../../../fixtures/foundation/f-a.txt");

const F_C: &str = include_str!("../../../fixtures/foundation/f-c.txt");

const BASE_WEIGHT: u64 = 1_000;

const RANK_STEP: u64 = 100;

/// Best (lowest) candidate rank at which each phrase was captured.
fn captured_ranks() -> BTreeMap<String, usize> {
    let mut ranks: BTreeMap<String, usize> = BTreeMap::new();

    for line in F_A.lines().chain(F_C.lines()) {
        let Some(encoded) = field(line, "candidates_hex") else {
            continue;
        };
        if encoded == "-" {
            continue;
        }

        for (rank, item) in encoded.split(',').enumerate() {
            let phrase = decode_hex(item);
            ranks
                .entry(phrase)
                .and_modify(|best| *best = (*best).min(rank))
                .or_insert(rank);
        }
    }

    ranks
}

fn field<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    line.split('\t')
        .filter_map(|field| field.split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value)
}

fn decode_hex(text: &str) -> String {
    let bytes: Vec<u8> = (0..text.len() / 2)
        .map(|index| {
            u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).expect("capture hex is valid")
        })
        .collect();
    String::from_utf8(bytes).expect("capture candidates are UTF-8")
}

/// The vocabulary fixture as `(token, keys, text, unigram)` in file order.
fn vocab_records() -> Vec<(u32, String, String, u64)> {
    VOCAB
        .lines()
        .filter(|line| !line.trim_start().starts_with('#') && !line.trim().is_empty())
        .map(|line| {
            (
                field(line, "token").expect("token").parse().expect("token"),
                field(line, "keys").expect("keys").to_owned(),
                field(line, "text").expect("text").to_owned(),
                field(line, "unigram")
                    .expect("unigram")
                    .parse()
                    .expect("unigram"),
            )
        })
        .collect()
}

/// `docs/testing/capture-fixtures.md` freezes F-A at 15 records and F-C at 46, all
/// from one pin.
#[test]
fn the_captures_hold_their_frozen_records_from_one_pin() {
    const PIN_REF: &str = concat!(
        "libpinyin-2.11.92-074a2219c90feaf962d0d24f034514033ece5f99",
        "+model20-59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155",
        "+dbm-tkrzw"
    );
    for (name, text, count) in [("F-A", F_A, 15), ("F-C", F_C, 46)] {
        assert_eq!(text.lines().count(), count, "{name} record count");
        for line in text.lines() {
            assert_eq!(field(line, "schema"), Some("pinyin-capture-v1"), "{name}");
            assert_eq!(field(line, "family"), Some(name), "{name}");
            assert_eq!(field(line, "pin_ref"), Some(PIN_REF), "{name}");
        }
    }
}

#[test]
fn every_fixture_phrase_is_captured_oracle_output() {
    let ranks = captured_ranks();
    let records = vocab_records();
    assert!(!records.is_empty(), "the fixture must not be empty");

    for (_, keys, text, _) in &records {
        assert!(
            ranks.contains_key(text),
            "{text:?} (keys {keys}) is not in any captured candidate list"
        );
    }
}

#[test]
fn every_unigram_weight_follows_the_documented_rank_rule() {
    let ranks = captured_ranks();

    for (token, keys, text, unigram) in vocab_records() {
        let rank = u64::try_from(ranks[&text]).expect("captured ranks are small");
        let expected = BASE_WEIGHT - RANK_STEP * rank;
        assert_eq!(
            unigram, expected,
            "token {token} ({text:?}, keys {keys}) is at captured rank {rank}"
        );
    }
}

#[test]
fn tokens_are_dense_and_ordered() {
    for (index, (token, ..)) in vocab_records().into_iter().enumerate() {
        let expected = u32::try_from(index + 1).expect("the fixture is small");
        assert_eq!(token, expected, "tokens number 1..=n in file order");
    }
}

#[test]
fn entries_sharing_a_key_sequence_keep_the_captured_order() {
    let mut previous: Option<(String, u64)> = None;

    for (_, keys, text, unigram) in vocab_records() {
        if let Some((last_keys, last_weight)) = &previous
            && *last_keys == keys
        {
            assert!(
                *last_weight >= unigram,
                "{text:?} outranks an earlier entry for keys {keys}"
            );
        }
        previous = Some((keys, unigram));
    }
}
