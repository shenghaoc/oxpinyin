use super::{NbestRow, NbestShape, Trellis, Value, nbest_sentences};
use crate::PhraseEntry;
use crate::engine::session::ScanKey;
use crate::engine::session::matrix::ScanMatrix;
use crate::graph::SegmentGraph;
use crate::{
    OptionBits, PhraseText, PhraseToken, SyllableKey,
    model::{LanguageModel, NbestStepCosts, PhraseList},
};
use std::cell::RefCell;

use crate::engine::SessionSettings;
use crate::engine::error::EngineError;
use crate::engine::session::build_scan_matrix;
use crate::fixture_doubles::{FixtureDictionary, FixtureLanguageModel};

struct TextDictionary;

impl PhraseList for TextDictionary {
    type Syllable = SyllableKey;
    type Error = std::convert::Infallible;

    fn lookup(&self, _syllables: &[Self::Syllable]) -> Result<Vec<PhraseEntry>, Self::Error> {
        Ok(vec![PhraseEntry::new(1, "你好")])
    }
}

#[test]
fn an_ordinary_sentence_text_is_built() {
    let mut trellis = Trellis::with_seeds(0, &[], NbestShape::PINYIN);
    trellis.texts.insert(1, (0, 1));
    trellis
        .text_keys
        .push(SyllableKey::from_text("ni").expect("ni is a key"));
    trellis.text_tones.push(0);
    let mut cache = super::TokenMap::default();
    let text = trellis
        .text(&TextDictionary, &mut cache, &[(0, 1)], &RefCell::default())
        .expect("trellis text")
        .expect("present");
    assert_eq!(text.as_str(), Some("你好"));
}

/// A slot whose pricing is already known, so no dictionary is read.
fn priced_slot(token: u32) -> super::LazySlot {
    let slot = super::LazySlot::new(0);
    slot.priced
        .set(Some(super::SpanEntry {
            token,
            path: 0,
            keys: 1,
            chars: 1,
            pronunciation: 1.0,
        }))
        .ok();
    slot
}

fn pricing(
    ranges: &crate::model::PhraseIndexRanges<super::LazySlot>,
) -> super::Pricing<'_, FixtureDictionary> {
    static DICTIONARY: std::sync::OnceLock<FixtureDictionary> = std::sync::OnceLock::new();
    // The paths are leaked rather than held in a static.
    super::Pricing {
        facts: Box::leak(Box::default()),
        ranges,
        dictionary: DICTIONARY.get_or_init(FixtureDictionary::default),
        paths: Box::leak(Box::default()),
        memos: &[],
        generation: Box::leak(Box::new(super::GenerationView {
            columns: Box::leak(Box::default()),
            zeros: &[],
        })),
        span: 0..0,
        sums: Box::leak(Box::default()),
    }
}

#[test]
fn span_pricing_reuses_the_sum_and_preserves_each_slots_path_and_key_count() {
    use std::cell::Cell;

    struct SpanDictionary {
        short_item_key: SyllableKey,
        reads: Cell<usize>,
    }

    impl PhraseList for SpanDictionary {
        type Syllable = SyllableKey;
        type Error = std::convert::Infallible;

        fn lookup(&self, syllables: &[SyllableKey]) -> Result<Vec<PhraseEntry>, Self::Error> {
            self.reads.set(self.reads.get() + 1);
            // Give one path a different item length for the same token
            // to exercise the length check on a populated cache.
            let (text, total) = match syllables {
                [_] => ("西安", 5),
                [key, _] if *key == self.short_item_key => ("先", 7),
                [_, _] => ("西安", 3),
                _ => return Ok(Vec::new()),
            };
            Ok(vec![
                PhraseEntry::new(1, text).with_pronunciation_possibility(1, total),
            ])
        }
    }

    let edge = |text, from, to| ScanKey {
        key: SyllableKey::from_text(text).unwrap(),
        from,
        to,
        syllable_start: from,
        crosses_separator: false,
        tone: 0,
    };
    // The paths in walk order are xian, xi + an, and xia + n.
    let scan = ScanMatrix::from_columns(vec![
        vec![edge("xian", 0, 4), edge("xi", 0, 2), edge("xia", 0, 3)],
        vec![],
        vec![edge("an", 2, 4)],
        vec![edge("n", 3, 4)],
    ]);
    let columns = super::ParsedMatrix::from_scan(&scan, 4, false);
    let generation = super::GenerationView {
        columns: &columns,
        zeros: &columns.zeros,
    };
    let dictionary = SpanDictionary {
        short_item_key: SyllableKey::from_text("xia").unwrap(),
        reads: Cell::new(0),
    };
    let mut found = super::SpanRanges::default();
    generation
        .search(
            0,
            4,
            &dictionary,
            &mut super::GenerationPath::default(),
            &mut found,
        )
        .unwrap();
    let pricing = super::Pricing {
        ranges: &found.ranges,
        paths: &found.paths,
        memos: &found.memos,
        sums: &found.sums,
        facts: &RefCell::default(),
        dictionary: &dictionary,
        generation: &generation,
        span: 0..4,
    };
    // The uncached walk for length two sums the two split paths in
    // matrix order. The walk for length one prices only the direct path.
    let split_sum = 1.0_f32 / 3.0 + 1.0_f32 / 7.0;
    let expected = [
        (0, 1, 2, split_sum),
        (1, 2, 2, split_sum),
        (2, 2, 1, 1.0 / 5.0),
    ];
    let mut slots = pricing.ranges.walk_tokens();
    for (path, keys, chars, sum) in expected {
        let (token, slot) = slots.next().unwrap();
        assert_eq!(token, 1);
        dictionary.reads.set(0);
        let entry = pricing.price(slot, token).unwrap().unwrap();
        assert_eq!((entry.path, entry.keys, entry.chars), (path, keys, chars));
        assert_eq!(entry.pronunciation.to_bits(), sum.to_bits(), "path {path}");
        if path == 1 {
            // The cache hit reads only this slot's item.
            assert_eq!(dictionary.reads.get(), 1);
        }
    }
    assert!(slots.next().is_none());
}

fn loses_to(left: &Value, right: &Value) -> bool {
    super::node_loses_to(left, right, 2)
}

fn scored(poss: f32, length: u32) -> Value {
    Value {
        token: 1,
        prev_token: 0,
        from: super::SEED_STEP,
        sub: 0,
        length,
        keys: length,
        poss,
    }
}

#[test]
fn complete_selection_keys_obey_ordering_laws() {
    let mut entries = Vec::new();
    for length in [0, 1, u32::MAX] {
        for poss in [
            f32::NAN,
            f32::from_bits(0xffc00001),
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::MAX,
            -f32::MAX,
            0.0,
            -0.0,
            -1.0,
        ] {
            entries.push(super::BeamEntry {
                value: scored(poss, length),
                slot: 1,
                batch_position: entries.len(),
            });
        }
    }
    for shape in [NbestShape::PINYIN, NbestShape::ZHUYIN] {
        let less = |a, b| super::selection_compare(a, b, shape.nstore()).is_gt();
        for a in &entries {
            assert!(!less(a, a));
            for b in &entries {
                assert_eq!(
                    less(a, b),
                    !less(b, a) && a.batch_position != b.batch_position
                );
                for c in &entries {
                    if less(a, b) && less(b, c) {
                        assert!(less(a, c));
                    }
                }
            }
        }
    }
}

#[test]
fn node_transitions_preserve_physical_slots() {
    let mut trellis = Trellis::with_seeds(1, &[], NbestShape::PINYIN);
    let insert = |trellis: &mut Trellis, poss, identity| {
        trellis.insert(
            1,
            Value {
                prev_token: identity,
                ..scored(poss, 1)
            },
        );
    };
    let slots = |trellis: &Trellis| {
        trellis
            .candidates(1)
            .iter()
            .map(|entry| (entry.slot, entry.value.prev_token))
            .collect::<Vec<_>>()
    };
    assert!(slots(&trellis).is_empty());
    insert(&mut trellis, -10.0, 1);
    assert_eq!(slots(&trellis), [(0, 1)]);
    insert(&mut trellis, -20.0, 2);
    assert_eq!(slots(&trellis), [(0, 1), (1, 2)]);
    insert(&mut trellis, -15.0, 3); // Beats the back only: reject.
    assert_eq!(slots(&trellis), [(0, 1), (1, 2)]);
    insert(&mut trellis, -10.0, 4); // Ties the front: reject.
    assert_eq!(slots(&trellis), [(0, 1), (1, 2)]);
    insert(&mut trellis, -5.0, 5); // Evicts the front, keeps slot 1.
    assert_eq!(slots(&trellis), [(0, 5), (1, 2)]);

    for (poss, expected) in [(-5.0, [2, 1]), (-10.0, [1, 2]), (-20.0, [1, 2])] {
        let mut trellis = Trellis::with_seeds(1, &[], NbestShape::PINYIN);
        insert(&mut trellis, -10.0, 1);
        insert(&mut trellis, poss, 2);
        assert_eq!(slots(&trellis), [(0, expected[0]), (1, expected[1])]);
    }
}

#[test]
fn selection_keeps_the_complete_key_at_each_limit() {
    for shape in [NbestShape::PINYIN, NbestShape::ZHUYIN] {
        for limit in [0, 1, 3, super::NBEAM] {
            for size in [0, limit.saturating_sub(1), limit, limit + 1] {
                for tied in [false, true] {
                    let entries: Vec<_> = (0..size)
                        .rev()
                        .map(|position| super::BeamEntry {
                            value: scored(if tied { -1.0 } else { -(position as f32) }, 1),
                            slot: (position % 2) as u32,
                            batch_position: position,
                        })
                        .collect();
                    let selected = super::select_top(entries, limit, shape.nstore());
                    assert_eq!(
                        selected
                            .iter()
                            .map(|entry| entry.batch_position)
                            .collect::<Vec<_>>(),
                        (0..size.min(limit)).collect::<Vec<_>>()
                    );
                    assert!(
                        selected
                            .iter()
                            .all(|entry| entry.slot as usize == entry.batch_position % 2)
                    );
                }
            }
        }
    }
}

/// The standard-heap alternative uses exactly the production key. A
/// max heap needs the reverse of the best-first selection comparator.
#[derive(Clone, Copy, Debug)]
struct HeapEntry<const S: usize>(super::BeamEntry);

impl<const S: usize> Ord for HeapEntry<S> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        super::selection_compare(&other.0, &self.0, S)
    }
}

impl<const S: usize> PartialOrd for HeapEntry<S> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<const S: usize> PartialEq for HeapEntry<S> {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}

impl<const S: usize> Eq for HeapEntry<S> {}

fn binary_heap_top<const S: usize>(
    values: Vec<super::BeamEntry>,
    count: usize,
) -> Vec<super::BeamEntry> {
    let mut heap = std::collections::BinaryHeap::from(
        values.into_iter().map(HeapEntry::<S>).collect::<Vec<_>>(),
    );
    (0..count)
        .map_while(|_| heap.pop().map(|entry| entry.0))
        .collect()
}

fn selection_batch(size: usize, tied: bool) -> Vec<super::BeamEntry> {
    (0..size)
        .rev()
        .map(|position| super::BeamEntry {
            value: scored(
                if tied {
                    -1.0
                } else {
                    [
                        f32::NAN,
                        f32::INFINITY,
                        f32::NEG_INFINITY,
                        0.0,
                        -0.0,
                        -1.0,
                        -20.0,
                    ][position % 7]
                },
                if tied { 1 } else { (position % 3) as u32 },
            ),
            slot: (position % 2) as u32,
            batch_position: position,
        })
        .collect()
}

#[test]
fn selection_agrees_with_std_heap_on_mixed_keys() {
    for size in [0, 1, 2, 3, 31, 32, 33, 128] {
        for count in [0, 1, 3, super::NBEAM, 129] {
            for tied in [false, true] {
                let entries = selection_batch(size, tied);
                let ids = |entries: Vec<super::BeamEntry>| {
                    entries
                        .into_iter()
                        .map(|entry| (entry.batch_position, entry.slot))
                        .collect::<Vec<_>>()
                };
                assert_eq!(
                    ids(super::select_top(entries.clone(), count, 1)),
                    ids(binary_heap_top::<1>(entries.clone(), count))
                );
                assert_eq!(
                    ids(super::select_top(entries.clone(), count, 2)),
                    ids(binary_heap_top::<2>(entries, count))
                );
            }
        }
    }
}

#[test]
fn float_trellis_storage_shrinks() {
    assert_eq!(std::mem::size_of::<Value>(), 28);
    assert_eq!(std::mem::size_of::<super::Node>(), 60);
    #[cfg(target_pointer_width = "64")]
    assert_eq!(std::mem::size_of::<super::BeamEntry>(), 40);
}

#[test]
fn ieee_order_and_stable_raw_tails() {
    let mut tails = [
        f32::NAN,
        -0.0,
        f32::NEG_INFINITY,
        f32::INFINITY,
        0.0,
        -1.0,
        f32::from_bits(0xffc00001),
    ]
    .map(|p| scored(p, 1));
    for (index, tail) in tails.iter_mut().enumerate() {
        tail.token = index as u32;
    }
    super::tail_merge(&mut tails);
    assert_eq!(tails.map(|v| v.token), [3, 1, 4, 5, 2, 0, 6]);
    for (index, left) in tails.iter().enumerate() {
        for right in &tails[index..] {
            assert!(!super::ordered_poss_compare(left.poss, right.poss).is_gt());
        }
    }
}

#[test]
fn raw_tails_order_subunit_score_differences() {
    let mut tails = [scored(-1.0, 1), scored(-0.25, 1)];
    tails[1].token = 2;
    super::tail_merge(&mut tails);
    assert_eq!(tails.map(|tail| tail.token), [2, 1]);
}

#[test]
fn adjusted_score_rounds_product_then_difference_and_ties() {
    let p = 1.2_f32.ln();
    for advantage in [p.next_down(), p, p.next_up()] {
        let short = scored(0.0, 0);
        let long = scored(advantage, 1);
        assert_eq!(loses_to(&short, &long), advantage > p);
        assert_eq!(loses_to(&long, &short), advantage < p);
    }
    // Distinct exact adjusted scores collapse on the f32 grid.
    let short = scored(-16777216.0, 0);
    let long = scored(-16777216.0, 1);
    assert_ne!(f64::from(short.poss), f64::from(long.poss) - f64::from(p));
    assert_eq!(super::adjusted_poss(&short), super::adjusted_poss(&long));
    assert!(!loses_to(&short, &long));
    assert!(!loses_to(&long, &short));
    for special in [f32::INFINITY, f32::NEG_INFINITY, f32::NAN] {
        assert!(!loses_to(&scored(special, 0), &scored(special, u32::MAX)));
    }
}

#[test]
fn float_adjusted_comparison_is_a_strict_weak_order() {
    let mut values = Vec::new();
    for length in [0, 1, 3, u32::MAX] {
        for poss in [
            f32::NAN,
            f32::from_bits(0xffc00001),
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::MAX,
            -f32::MAX,
            0.0,
            -0.0,
            -1.0,
            -1.0_f32.next_up(),
        ] {
            values.push(scored(poss, length));
        }
    }
    for a in &values {
        assert!(!loses_to(a, a));
        for b in &values {
            assert!(!(loses_to(a, b) && loses_to(b, a)));
            for c in &values {
                if loses_to(a, b) && loses_to(b, c) {
                    assert!(loses_to(a, c));
                }
                if !loses_to(a, b) && !loses_to(b, a) && !loses_to(b, c) && !loses_to(c, b) {
                    assert!(!loses_to(a, c) && !loses_to(c, a));
                }
            }
        }
    }
}

#[test]
fn tails_require_the_final_step_even_when_underfilled() {
    for shape in [NbestShape::PINYIN, NbestShape::ZHUYIN] {
        let mut trellis = Trellis::with_seeds(2, &[], shape);
        for token in 1..=3 {
            trellis.insert(
                1,
                Value {
                    token,
                    ..scored(-1.0, 1)
                },
            );
        }
        assert!(trellis.tails().is_empty());
        trellis.insert(
            2,
            Value {
                token: 4,
                ..scored(-2.0, 2)
            },
        );
        assert_eq!(
            trellis.tails().iter().map(|v| v.token).collect::<Vec<_>>(),
            [4]
        );
    }
}

#[test]
fn full_node_preserves_front_eviction_and_rejects_ties() {
    let mut trellis = Trellis::with_seeds(1, &[], NbestShape::PINYIN);
    for poss in [-10.0, -20.0, -5.0] {
        trellis.insert(1, scored(poss, 1));
    }
    let residents = trellis.candidates(1);
    assert_eq!(
        residents.iter().map(|v| v.value.poss).collect::<Vec<_>>(),
        [-5.0, -20.0]
    );
    trellis.insert(
        1,
        Value {
            keys: 999,
            ..scored(-5.0, 1)
        },
    );
    assert!(trellis.candidates(1).iter().all(|v| v.value.keys != 999));
}

#[test]
fn selection_uses_batch_ties_and_zhuyin_length_first_nan_last() {
    for shape in [NbestShape::PINYIN, NbestShape::ZHUYIN] {
        let mut trellis = Trellis::with_seeds(1, &[], shape);
        for token in 1..=40 {
            trellis.insert(
                1,
                Value {
                    token,
                    ..scored(-1.0, 1)
                },
            );
        }
        assert_eq!(
            trellis
                .beam(1)
                .iter()
                .map(|v| v.value.token)
                .collect::<Vec<_>>(),
            (1..=32).collect::<Vec<_>>()
        );
    }
    let entry = |poss, length, batch_position| super::BeamEntry {
        value: scored(poss, length),
        slot: 0,
        batch_position,
    };
    assert!(
        super::selection_compare(&entry(f32::INFINITY, 2, 0), &entry(f32::NAN, 1, 1), 1).is_gt()
    );
    assert!(
        super::selection_compare(&entry(f32::NAN, 1, 0), &entry(f32::NEG_INFINITY, 1, 1), 1)
            .is_gt()
    );
    assert!(super::selection_compare(&entry(-0.0, 1, 1), &entry(0.0, 1, 0), 1).is_gt());
    // Zhuyin's single-slot admission remains raw IEEE <, including NaN.
    assert!(!super::node_loses_to(
        &scored(f32::NAN, 1),
        &scored(1.0, 1),
        1
    ));
}

#[test]
fn backtracking_uses_the_physical_slot_after_batch_selection() {
    let mut trellis = Trellis::with_seeds(3, &[0], NbestShape::PINYIN);
    for token in [7, 8] {
        trellis.insert(
            1,
            Value {
                token,
                from: 0,
                ..scored(0.0, 1)
            },
        );
    }
    trellis.insert(
        2,
        Value {
            token: 99,
            ..scored(0.0, 2)
        },
    );
    for (poss, prev_token) in [(-0.1, 7), (-0.2, 8)] {
        trellis.insert(
            2,
            Value {
                token: 11,
                prev_token,
                from: 1,
                ..scored(poss, 2)
            },
        );
    }
    let beam = trellis.beam(2);
    let predecessor = beam
        .iter()
        .find(|entry| entry.value.token == 11 && entry.slot == 1)
        .unwrap();
    assert_eq!(predecessor.batch_position, 2);
    let paths = super::SpanPaths::default();
    let model = Fixed {
        blended: Some(1.0),
        unigram: Some(1.0),
    };
    super::expand_one(
        &super::SpanEntry {
            token: 22,
            path: 0,
            keys: 1,
            chars: 1,
            pronunciation: 1.0,
        },
        super::StepCtx {
            paths: &paths,
            span: 2..3,
            model: &model,
            facts: &RefCell::default(),
        },
        predecessor,
        &mut crate::model::NbestStepRow::default(),
        &mut trellis,
        super::Branch::Unigram,
    )
    .unwrap();
    let tails = trellis.tails();
    assert_eq!(tails[0].sub, 1);
    assert_eq!(trellis.extract(&tails[0]), [(0, 8), (1, 11), (2, 22)]);
}

/// A model answering fixed step probabilities, so the tests measure the trellis
/// and not the blend.
struct Fixed {
    blended: Option<f64>,
    unigram: Option<f64>,
}

impl LanguageModel for Fixed {
    type Error = EngineError;

    fn nbest_step_costs(
        &self,
        _prev: &PhraseToken,
        _token: &PhraseToken,
    ) -> Result<NbestStepCosts, EngineError> {
        Ok(NbestStepCosts {
            blended: self.blended,
            unigram: self.unigram,

            unigram_lambda: 1.0,
        })
    }
}

#[test]
fn unigram_step_accumulates_log_possibility() {
    // phonetic_lookup.h:45,650-665 stores predecessor + log(p) in a gfloat.
    // At 13/32, rounding log(p) before addition gives 0xc039_a67c.
    for (probability, expected) in [(0.5, 0xc02c_5c86), (13.0 / 32.0, 0xc039_a67d)] {
        let mut trellis = Trellis::with_seeds(1, &[], NbestShape::PINYIN);
        let mut paths = super::SpanPaths::default();
        let entry = super::SpanEntry {
            token: 2,
            path: paths.push(&[SyllableKey::from_text("ni").expect("ni is a key")], &[0]),
            keys: 1,
            chars: 1,
            pronunciation: 1.0,
        };
        super::expand_one(
            &entry,
            super::StepCtx {
                facts: &RefCell::default(),
                paths: &paths,
                span: 0..1,
                model: &Fixed {
                    blended: None,
                    unigram: Some(probability),
                },
            },
            &super::BeamEntry {
                value: scored(-2.0, 1),
                slot: 0,
                batch_position: 0,
            },
            &mut Default::default(),
            &mut trellis,
            super::Branch::Unigram,
        )
        .expect("unigram expansion");
        assert_eq!(
            trellis.tails()[0].poss.to_bits(),
            expected,
            "p = {probability}"
        );
    }
}

fn one_column_matrix(text: &str) -> (ScanMatrix, usize) {
    let graph = SegmentGraph::build_with_options(text.as_bytes(), OptionBits::default())
        .expect("short input builds");
    let matrix = ScanMatrix::from_columns(build_scan_matrix(&graph, OptionBits::default(), true));
    let bound = graph.consumed();
    (matrix, bound)
}

/// The pin sums all paths in `phonetic_key_matrix.cpp:534-600` at
/// 074a2219, then both step branches compare the sum with FLT_EPSILON
/// (`phonetic_lookup.h:654-658,683-688`). Two shares of 1/2^24
/// reach that boundary even though neither path reaches it alone.
#[test]
fn two_sub_epsilon_paths_admit_the_span_in_both_step_branches() {
    struct TinyReadings;
    impl PhraseList for TinyReadings {
        type Syllable = SyllableKey;
        type Error = std::convert::Infallible;

        fn lookup(&self, keys: &[SyllableKey]) -> Result<Vec<PhraseEntry>, Self::Error> {
            Ok(if keys.len() == 1 {
                vec![PhraseEntry::new(1, "你").with_pronunciation_possibility(1, 1 << 24)]
            } else {
                vec![]
            })
        }
    }
    let edge = |text| ScanKey {
        key: SyllableKey::from_text(text).unwrap(),
        from: 0,
        to: 1,
        syllable_start: 0,
        crosses_separator: false,
        tone: 0,
    };
    for (blended, unigram) in [(Some(1.0), None), (None, Some(1.0))] {
        for count in [1, 2] {
            let matrix = super::ScanMatrix::from_columns(vec![
                vec![edge("ni"), edge("mi")][..count].to_vec(),
                vec![],
            ]);
            let rows = nbest_sentences(
                &matrix,
                1,
                &TinyReadings,
                &Fixed { blended, unigram },
                &[],
                None,
                NbestShape::PINYIN,
            )
            .unwrap();
            if count == 1 {
                assert!(rows.is_empty(), "the whole-span sum is below epsilon");
            } else {
                assert!(!rows.is_empty(), "the whole-span sum equals epsilon");
                for row in rows {
                    assert_eq!(row.text.as_ref().and_then(PhraseText::as_str), Some("你"));
                }
            }
        }
    }
}

#[test]
fn a_model_without_step_costs_yields_no_rows() {
    let (matrix, bound) = one_column_matrix("nihao");
    let dict = FixtureDictionary::default();
    let model = Fixed {
        blended: None,
        unigram: None,
    };
    let rows = nbest_sentences(&matrix, bound, &dict, &model, &[], None, NbestShape::PINYIN)
        .expect("the trellis cannot fail here");
    assert!(rows.is_empty());
}

/// λ = 1 makes `unigram_lambda(1. - lambda)` zero
/// (`phonetic_lookup.h:712-713` at 074a2219), so a unigram step adds
/// `log(0)`. The step is kept: the sentence still reaches the final
/// step, at -inf.
#[test]
fn a_zero_unigram_weight_still_gives_a_final_step_sentence() {
    struct UnigramOnly;
    impl LanguageModel for UnigramOnly {
        type Error = EngineError;
        fn nbest_step_costs(
            &self,
            _: &PhraseToken,
            _: &PhraseToken,
        ) -> Result<NbestStepCosts, EngineError> {
            Ok(NbestStepCosts {
                blended: None,
                unigram: Some(0.5),
                unigram_lambda: 0.0,
            })
        }
    }
    let dict = FixtureDictionary::parse("token=1\tkeys=ni\ttext=你\tunigram=1000\n")
        .expect("authored fixture");
    let (matrix, bound) = one_column_matrix("ni");
    let rows = nbest_sentences(
        &matrix,
        bound,
        &dict,
        &UnigramOnly,
        &[],
        None,
        NbestShape::PINYIN,
    )
    .expect("the trellis cannot fail here");
    assert_eq!(
        rows[0].text.as_ref().and_then(PhraseText::as_str),
        Some("你")
    );
}

/// A predecessor's row lives for one span load: the pin reloads both
/// bigrams for every beam value and span (`phonetic_lookup.h:595-596`
/// at 074a2219). A model that loads into the row must decode the same
/// rows as the pair-wise default `nbest_step_costs_in_row`, including
/// after the gram changes between guesses, so no row outlives its span
/// and no predecessor reads another's row.
#[test]
fn row_cached_step_costs_decode_as_the_pairwise_path() {
    use crate::model::{MergedGram, NbestStepRow};
    use std::cell::Cell;

    fn records(prev: u32, flipped: bool) -> Vec<(u32, u64)> {
        match (prev, flipped) {
            (1, false) | (2, true) => vec![(1, 3), (2, 1)],
            _ => vec![(1, 1), (2, 3)],
        }
    }
    fn costs(freq: u64) -> NbestStepCosts {
        NbestStepCosts {
            blended: Some(freq as f64 / 4.0),
            unigram: Some(0.25),
            unigram_lambda: 0.5,
        }
    }
    fn freq_of(records: &[(u32, u64)], token: u32) -> u64 {
        records
            .iter()
            .find(|(next, _)| *next == token)
            .map_or(0, |(_, freq)| *freq)
    }
    struct Pairwise<'a>(&'a Cell<bool>);
    impl LanguageModel for Pairwise<'_> {
        type Error = EngineError;
        fn nbest_step_costs(
            &self,
            prev: &PhraseToken,
            token: &PhraseToken,
        ) -> Result<NbestStepCosts, EngineError> {
            let records = records(*prev, self.0.get());
            Ok(costs(freq_of(&records, *token)))
        }
    }
    struct RowCached<'a>(&'a Cell<bool>);
    impl LanguageModel for RowCached<'_> {
        type Error = EngineError;
        fn nbest_step_costs_in_row(
            &self,
            prev: &PhraseToken,
            token: &PhraseToken,
            row: &mut NbestStepRow,
        ) -> Result<NbestStepCosts, EngineError> {
            let gram = row
                .get_or_try_init::<EngineError>(|| {
                    Ok(Some(MergedGram::new(4, records(*prev, self.0.get()))))
                })?
                .expect("loaded row");
            let freq = gram.get_freq(*token).expect("whole gram");
            Ok(costs(u64::from(freq.unwrap_or(0))))
        }
    }
    fn decoded(rows: &[NbestRow]) -> Vec<(Option<String>, Vec<u32>)> {
        rows.iter()
            .map(|row| {
                (
                    row.text
                        .as_ref()
                        .and_then(PhraseText::as_str)
                        .map(str::to_owned),
                    row.tokens.to_vec(),
                )
            })
            .collect()
    }
    let dict = FixtureDictionary::parse(
        "token=1\tkeys=ni\ttext=你\tunigram=1000\ntoken=2\tkeys=ni\ttext=尼\tunigram=900\n",
    )
    .expect("authored fixture");
    let (matrix, bound) = one_column_matrix("nini");
    let flipped = Cell::new(false);
    let mut seen = Vec::new();
    for flip in [false, true] {
        flipped.set(flip);
        let history = [1];
        let cached = nbest_sentences(
            &matrix,
            bound,
            &dict,
            &RowCached(&flipped),
            &history,
            None,
            NbestShape::PINYIN,
        )
        .unwrap();
        let pairwise = nbest_sentences(
            &matrix,
            bound,
            &dict,
            &Pairwise(&flipped),
            &history,
            None,
            NbestShape::PINYIN,
        )
        .unwrap();
        assert!(!cached.is_empty());
        assert_eq!(decoded(&cached), decoded(&pairwise), "flipped = {flip}");
        seen.push(decoded(&cached));
    }
    assert_ne!(seen[0], seen[1], "the gram change must move the rows");
}

#[test]
fn sparse_bigram_walk_skips_absent_tokens_and_keeps_span_order() {
    use crate::model::{MergedGram, NbestStepRow};
    use std::cell::RefCell;

    struct Sparse(RefCell<Vec<u32>>);
    impl LanguageModel for Sparse {
        type Error = EngineError;

        fn nbest_successors_in_row<'a>(
            &self,
            _: &PhraseToken,
            row: &'a mut NbestStepRow,
        ) -> Result<Option<crate::model::NbestSuccessors<'a>>, EngineError> {
            Ok(Some(
                row.get_or_try_init::<EngineError>(|| {
                    Ok(Some(MergedGram::new(2, vec![(1, 2), (3, 0)])))
                })?
                .map_or(
                    crate::model::NbestSuccessors::Miss,
                    crate::model::NbestSuccessors::Row,
                ),
            ))
        }

        fn nbest_step_costs_in_row(
            &self,
            _: &PhraseToken,
            token: &PhraseToken,
            _: &mut NbestStepRow,
        ) -> Result<NbestStepCosts, EngineError> {
            self.0.borrow_mut().push(*token);
            Ok(NbestStepCosts {
                blended: Some(0.5),
                unigram: Some(0.25),

                unigram_lambda: 1.0,
            })
        }
    }

    let mut entries = crate::model::PhraseIndexRanges::new();
    entries.convert([3, 2, 1, 3].map(|token| (token, Some(priced_slot(token)))));
    let model = Sparse(RefCell::new(Vec::new()));
    let mut trellis = super::Trellis::with_seeds(1, &[11], NbestShape::PINYIN);
    let beam = trellis.beam(0);
    super::expand_entries(&pricing(&entries), &beam, 0..1, &model, &mut trellis, None).unwrap();
    // Bigram visits present 3,1,3 in the original range/item order;
    // the zero-count record 3 still participates. The independent head
    // unigram pass then visits every candidate, including absent 2.
    assert_eq!(*model.0.borrow(), [3, 1, 3, 3, 2, 1, 3]);
}

/// N2 refusals 1, 2, 4 and 6 in the n-best step
/// (`phonetic_lookup.h:594-632`): the predecessor's merge asserts on a
/// short user value beside a system gram; with no system gram,
/// `search` asserts on its total, a one-step constraint's `get_freq`
/// reads past it, and a walk past the last whole item reads past a
/// partial tail. A walk that stops before the tail steps normally.
#[test]
fn a_predecessor_row_the_pin_cannot_read_refuses_the_step() {
    use crate::model::{MergedGram, NbestStepRow, NbestSuccessors};

    struct Rows(Option<MergedGram>);
    impl LanguageModel for Rows {
        type Error = EngineError;

        fn nbest_successors_in_row<'a>(
            &self,
            _: &PhraseToken,
            row: &'a mut NbestStepRow,
        ) -> Result<Option<NbestSuccessors<'a>>, EngineError> {
            Ok(Some(
                row.get_or_try_init::<EngineError>(|| Ok(self.0.clone()))?
                    .map_or(NbestSuccessors::Miss, NbestSuccessors::Row),
            ))
        }

        fn nbest_item_costs_in_row(
            &self,
            _: &PhraseToken,
            _: &PhraseToken,
            item: Option<(u32, u32)>,
            _: &mut NbestStepRow,
        ) -> Result<NbestStepCosts, EngineError> {
            Ok(NbestStepCosts {
                blended: item.map(|(freq, total)| f64::from(freq) / f64::from(total)),
                unigram: Some(0.25),

                unigram_lambda: 1.0,
            })
        }
    }

    let gram = |total: u32, items: &[(u32, u32)], tail: &[u8]| {
        let mut value = total.to_le_bytes().to_vec();
        for (token, freq) in items {
            value.extend_from_slice(&token.to_le_bytes());
            value.extend_from_slice(&freq.to_le_bytes());
        }
        value.extend_from_slice(tail);
        value
    };
    let mut entries = crate::model::PhraseIndexRanges::new();
    entries.convert([1, 2].map(|token| (token, Some(priced_slot(token)))));
    let run = |row: Option<MergedGram>, one_step: Option<u32>| {
        let mut trellis = super::Trellis::with_seeds(1, &[11], NbestShape::PINYIN);
        let beam = trellis.beam(0);
        super::expand_entries(
            &pricing(&entries),
            &beam,
            0..1,
            &Rows(row),
            &mut trellis,
            one_step,
        )
    };
    let short = EngineError::ShortLookupPredecessor { token: 11 };
    let unsafe_read = EngineError::UnsafeLookupPredecessor { token: 11 };
    let merged = MergedGram::merge(Some(gram(1, &[(1, 1)], &[])), Some(vec![0, 0]));
    assert_eq!(run(merged, None), Err(short.clone()));
    let alone = || MergedGram::merge(None, Some(vec![0, 0]));
    assert_eq!(run(alone(), None), Err(short));
    assert_eq!(run(alone(), Some(1)), Err(unsafe_read.clone()));
    let tailed = || MergedGram::merge(None, Some(gram(3, &[(1, 3)], &[9])));
    assert_eq!(run(tailed(), None), Err(unsafe_read));
    // `get_freq(1)` returns at the whole item before the tail.
    assert_eq!(run(tailed(), Some(1)), Ok(()));
    assert_eq!(run(None, None), Ok(()), "both grams absent: no bigram step");
}

/// `SingleGram::search` bounds its walk by the range end only
/// (`ngram.cpp:167-173`). On the row `[1, 3, 2]` the walk over range
/// `[3]` starts at 3 and yields 2 too, below that range; the pin steps
/// every yielded token whose phrase spells the span, with that item's
/// own count, so 2 steps there with 7.
#[test]
fn a_token_the_walk_yields_below_its_range_still_steps() {
    use crate::model::{MergedGram, NbestStepRow, NbestSuccessors};
    use std::cell::RefCell;

    /// Each bigram step's token and the item it was priced from.
    type Steps = Vec<(u32, Option<(u32, u32)>)>;
    struct Walked(RefCell<Steps>);
    impl LanguageModel for Walked {
        type Error = EngineError;

        fn nbest_successors_in_row<'a>(
            &self,
            _: &PhraseToken,
            row: &'a mut NbestStepRow,
        ) -> Result<Option<NbestSuccessors<'a>>, EngineError> {
            Ok(Some(
                row.get_or_try_init::<EngineError>(|| {
                    Ok(Some(MergedGram::new(15, vec![(1, 1), (3, 5), (2, 7)])))
                })?
                .map_or(NbestSuccessors::Miss, NbestSuccessors::Row),
            ))
        }

        fn nbest_item_costs_in_row(
            &self,
            _: &PhraseToken,
            token: &PhraseToken,
            item: Option<(u32, u32)>,
            _: &mut NbestStepRow,
        ) -> Result<NbestStepCosts, EngineError> {
            if item.is_some() {
                self.0.borrow_mut().push((*token, item));
            }
            Ok(NbestStepCosts {
                blended: Some(0.5),
                unigram: Some(0.25),

                unigram_lambda: 1.0,
            })
        }
    }

    let mut entries = crate::model::PhraseIndexRanges::new();
    entries.convert([1, 3, 2].map(|token| (token, Some(priced_slot(token)))));
    let model = Walked(RefCell::new(Vec::new()));
    let mut trellis = super::Trellis::with_seeds(1, &[11], NbestShape::PINYIN);
    let beam = trellis.beam(0);
    super::expand_entries(&pricing(&entries), &beam, 0..1, &model, &mut trellis, None).unwrap();
    // Range [1] yields 1; range [3] yields 3 and then 2; range [2]
    // starts past 2 and yields nothing.
    assert_eq!(
        *model.0.borrow(),
        [(1, Some((1, 15))), (3, Some((5, 15))), (2, Some((7, 15)))]
    );
}

/// The mini vocabulary's `ni`+`hao` chain with equal step costs decodes
/// the composition span.
#[test]
fn the_trellis_decodes_the_composition_span() {
    const VOCAB: &str =
        "token=1\tkeys=ni\ttext=你\tunigram=1000\ntoken=2\tkeys=hao\ttext=好\tunigram=900\n";
    let dict = FixtureDictionary::parse(VOCAB).expect("authored fixture");
    let model = Fixed {
        blended: Some(0.5),
        unigram: Some(0.25),
    };
    let (matrix, bound) = one_column_matrix("nihao");
    let rows: Vec<NbestRow> =
        nbest_sentences(&matrix, bound, &dict, &model, &[], None, NbestShape::PINYIN)
            .expect("the trellis cannot fail here");
    assert!(!rows.is_empty());
    assert_eq!(
        rows[0].text.as_ref().and_then(PhraseText::as_str),
        Some("你好")
    );
    assert_eq!(rows[0].span, bound);
    assert_eq!(rows[0].keys, 2);
}

/// libzhuyin's `PhoneticLookup<1, 1>` keeps one value per node and
/// extracts one tail, where libpinyin's `<2, 3>` extracts up to three:
/// two readings of `ni` give the pinyin shape two rows and the zhuyin
/// shape exactly one. Selection ties use earlier batch position.
#[test]
fn the_zhuyin_shape_keeps_one_tail_with_batch_ties() {
    const VOCAB: &str = "token=1\tkeys=ni\ttext=你\tunigram=1000\n\
                             token=2\tkeys=ni\ttext=尼\tunigram=900\n\
                             token=3\tkeys=hao\ttext=好\tunigram=900\n";
    let dict = FixtureDictionary::parse(VOCAB).expect("authored fixture");
    let model = Fixed {
        blended: Some(0.5),
        unigram: Some(0.25),
    };
    let (matrix, bound) = one_column_matrix("nihao");
    let pinyin = nbest_sentences(&matrix, bound, &dict, &model, &[], None, NbestShape::PINYIN)
        .expect("the trellis cannot fail here");
    let zhuyin = nbest_sentences(&matrix, bound, &dict, &model, &[], None, NbestShape::ZHUYIN)
        .expect("the trellis cannot fail here");
    assert!(
        pinyin.len() >= 2,
        "libpinyin's shape keeps both readings: {pinyin:?}"
    );
    assert_eq!(
        zhuyin.len(),
        1,
        "libzhuyin's shape extracts one tail: {zhuyin:?}"
    );
    assert_eq!(pinyin[0].text, Some("你好".into()));
    assert_eq!(
        zhuyin[0].text,
        Some("你好".into()),
        "earlier batch position wins ties"
    );
    assert_eq!(NbestShape::default(), NbestShape::PINYIN);
}

/// Session-level smoke: the public types compose.
#[test]
fn session_types_compose() {
    let session = crate::engine::Session::<FixtureDictionary, FixtureLanguageModel>::new(
        SessionSettings::default(),
        FixtureDictionary::default(),
        FixtureLanguageModel::default(),
    );
    assert!(!session.is_composing());
}
