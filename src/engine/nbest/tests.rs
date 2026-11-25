use super::{NbestRow, NbestShape, Trellis, Value, nbest_sentences};
use crate::engine::session::ScanKey;
use crate::engine::session::matrix::ScanMatrix;
use crate::graph::SegmentGraph;
use crate::vocab::PhraseEntry;
use crate::{
    OptionBits,
    model::{LanguageModel, NbestTokenCosts, PhraseList},
    vocab::{PhraseText, PhraseToken, SyllableKey},
};
use std::cell::RefCell;

use crate::engine::error::EngineError;
use crate::engine::session::build_scan_matrix;
use crate::fixture_doubles::FixtureDictionary;

/// A slot whose pricing is already known, so no dictionary is read.
fn priced_slot(token: u32) -> super::LazySlot {
    let slot = super::LazySlot::new(0);
    slot.priced
        .set(Some(super::SpanEntry::new(token, 0, 1, 1, 1.0)))
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

// Basis: Rust span pricing contract and phonetic_key_matrix.cpp:534–600 at 074a2219.
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

// Basis: Divergence register rows 146 and 152.
#[test]
fn complete_selection_keys_obey_ordering_laws() {
    let mut entries = Vec::new();
    for (poss, length) in [
        (f32::NAN, 0),
        (f32::INFINITY, 1),
        (f32::NEG_INFINITY, 1),
        (0.0, 1),
        (-0.0, 1),
        (-1.0, 0),
        (-1.0, 1),
    ] {
        entries.push(super::BeamEntry {
            value: scored(poss, length),
            slot: 1,
            batch_position: entries.len(),
        });
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

// Basis: phonetic_lookup_heap.h:56–81,89–122 at 074a2219 and the Rust physical slot contract.
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

// Basis: Divergence register rows 146 and 152.
#[test]
fn selection_keeps_the_complete_key_at_each_limit() {
    use super::NBEAM;

    for (size, limit) in [
        (0, 1),
        (1, 0),
        (NBEAM - 1, NBEAM),
        (NBEAM, NBEAM),
        (NBEAM + 1, NBEAM),
    ] {
        let entries = selection_batch(size, true);
        let selected = super::select_top(entries, limit, NbestShape::PINYIN.nstore());
        assert_eq!(
            selected
                .iter()
                .map(|entry| (entry.batch_position, entry.slot))
                .collect::<Vec<_>>(),
            (0..size.min(limit))
                .map(|index| (index, (index % 2) as u32))
                .collect::<Vec<_>>()
        );
    }

    let entries = selection_batch(NBEAM + 1, false);
    let ids = |selected: Vec<super::BeamEntry>| {
        selected
            .into_iter()
            .map(|entry| (entry.batch_position, entry.slot))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        ids(super::select_top(
            entries.clone(),
            NBEAM,
            NbestShape::PINYIN.nstore()
        )),
        ids(binary_heap_top::<2>(entries.clone(), NBEAM))
    );
    assert_eq!(
        ids(super::select_top(
            entries.clone(),
            NBEAM,
            NbestShape::ZHUYIN.nstore()
        )),
        ids(binary_heap_top::<1>(entries, NBEAM))
    );
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

// Basis: Rust trellis storage layout contract.
#[test]
fn float_trellis_storage_shrinks() {
    assert_eq!(std::mem::size_of::<Value>(), 28);
    assert_eq!(std::mem::size_of::<super::Node>(), 60);
    #[cfg(target_pointer_width = "64")]
    assert_eq!(std::mem::size_of::<super::BeamEntry>(), 40);
}

// Basis: Divergence register row 147.
#[test]
fn ieee_order_and_stable_raw_tails() {
    let mut tails = [
        f32::NAN,
        -0.0,
        f32::NEG_INFINITY,
        f32::INFINITY,
        0.0,
        -1.0,
        -0.25,
        f32::from_bits(0xffc00001),
    ]
    .map(|p| scored(p, 1));
    for (index, tail) in tails.iter_mut().enumerate() {
        tail.token = index as u32;
    }
    super::tail_merge(&mut tails);
    assert_eq!(tails.map(|v| v.token), [3, 1, 4, 6, 5, 2, 0, 7]);
}

// Basis: Divergence register row 146.
#[test]
fn adjusted_score_rounds_product_then_difference_and_ties() {
    let p = 1.2_f32.ln();
    for advantage in [p.next_down(), p, p.next_up()] {
        let short = scored(0.0, 0);
        let long = scored(advantage, 1);
        assert_eq!(loses_to(&short, &long), advantage > p);
        assert_eq!(loses_to(&long, &short), advantage < p);
    }
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

// Basis: Divergence register row 146.
#[test]
fn float_adjusted_comparison_is_a_strict_weak_order() {
    let values = [
        scored(f32::NAN, 0),
        scored(f32::INFINITY, 1),
        scored(f32::NEG_INFINITY, 1),
        scored(0.0, 0),
        scored(-0.0, 0),
        scored(-1.0, 1),
        scored(-1.0_f32.next_up(), 1),
    ];
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

// Basis: phonetic_lookup.h:329–339 at 074a2219.
#[test]
fn tails_require_the_final_step_even_when_underfilled() {
    let mut trellis = Trellis::with_seeds(2, &[], NbestShape::PINYIN);
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

// Basis: Divergence register row 152 and phonetic_lookup.h:66–91 at 074a2219.
#[test]
fn selection_uses_batch_ties_and_zhuyin_length_first_nan_last() {
    {
        let mut trellis = Trellis::with_seeds(1, &[], NbestShape::ZHUYIN);
        for token in 1..=33 {
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
    assert!(!super::node_loses_to(
        &scored(f32::NAN, 1),
        &scored(1.0, 1),
        1
    ));
}

// Basis: phonetic_lookup_heap.h:56–81,89–122 at 074a2219 and the Rust physical slot contract.
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
        &super::SpanEntry::new(22, 0, 1, 1, 1.0),
        super::StepCtx {
            paths: &paths,
            span: 2..3,
            model: &model,
            facts: &RefCell::default(),
        },
        predecessor,
        &mut trellis,
        super::Branch::Unigram,
    )
    .unwrap();
    let tails = trellis.tails();
    assert_eq!(tails[0].sub, 1);
    assert_eq!(trellis.extract(&tails[0]), [(0, 8), (1, 11), (2, 22)]);
}

/// Every fixture token, each once, at `freq / total` of 1: a predecessor
/// row through which every ranged token takes a bigram step.
fn dense_row() -> crate::model::MergedGram {
    crate::model::MergedGram::new(
        1,
        (1..=16)
            .chain(0x0100_0001..=0x0100_0010)
            .map(|token| (token, 1))
            .collect(),
    )
}

/// The predecessor's merged row `row`, loaded once into `slot`.
fn row_in(
    slot: &mut crate::model::NbestStepRow,
    row: impl FnOnce() -> Option<crate::model::MergedGram>,
) -> Result<Option<crate::model::NbestSuccessors<'_>>, EngineError> {
    Ok(Some(
        slot.get_or_try_init::<EngineError>(|| Ok(row()))?.map_or(
            crate::model::NbestSuccessors::Miss,
            crate::model::NbestSuccessors::Row,
        ),
    ))
}

/// A model answering fixed step probabilities, so the tests measure the trellis
/// and not the blend. With `blended`, every predecessor's row holds every
/// token at `freq / total` of 1, and the f32 weight `blended - unigram`
/// gives the bigram branch `blended`; both are exact at the values used.
struct Fixed {
    blended: Option<f64>,
    unigram: Option<f64>,
}

impl LanguageModel for Fixed {
    fn amplification_lambda(&self) -> f32 {
        0.312_699
    }

    type Error = EngineError;

    fn nbest_successors_in_row<'a>(
        &self,
        _: &PhraseToken,
        row: &'a mut crate::model::NbestStepRow,
    ) -> Result<Option<crate::model::NbestSuccessors<'a>>, EngineError> {
        row_in(row, || self.blended.map(|_| dense_row()))
    }

    fn nbest_token_costs(&self, _: &PhraseToken) -> Result<NbestTokenCosts, EngineError> {
        if self.blended.is_none() && self.unigram.is_none() {
            return Ok(NbestTokenCosts::ABSENT);
        }
        let elem = self.unigram.unwrap_or(0.0);
        Ok(NbestTokenCosts {
            elem: Some(elem),
            lambda: self.blended.map_or(0.0, |blended| (blended - elem) as f32),
            unigram_lambda: 1.0,
        })
    }
}

// Basis: Divergence register row 11 and phonetic_lookup.h:643–697 at 074a2219.
#[test]
fn unigram_step_accumulates_log_possibility() {
    for (probability, expected) in [(0.5, 0xc02c_5c86), (13.0 / 32.0, 0xc039_a67d)] {
        let mut trellis = Trellis::with_seeds(1, &[], NbestShape::PINYIN);
        let mut paths = super::SpanPaths::default();
        let entry = super::SpanEntry::new(
            2,
            paths.push(&[SyllableKey::from_text("ni").expect("ni is a key")], &[0]),
            1,
            1,
            1.0,
        );
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

// Basis: Divergence register row 11 and phonetic_lookup.h:643–697 at 074a2219.
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

// Basis: phonetic_lookup.h:643–713 at 074a2219 and the Rust step-cost contract.
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

// Basis: phonetic_lookup.h:643–713 at 074a2219 and the Rust step-cost contract.
#[test]
fn a_zero_unigram_weight_still_gives_a_final_step_sentence() {
    struct UnigramOnly;
    impl LanguageModel for UnigramOnly {
        fn amplification_lambda(&self) -> f32 {
            0.312_699
        }

        type Error = EngineError;
        fn nbest_token_costs(&self, _: &PhraseToken) -> Result<NbestTokenCosts, EngineError> {
            Ok(NbestTokenCosts {
                elem: Some(0.5),
                lambda: 1.0,
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

// Basis: phonetic_lookup.h:595–596,670–693 at 074a2219.
#[test]
fn row_step_costs_follow_each_predecessors_row() {
    use crate::model::{MergedGram, NbestStepRow};
    use std::cell::Cell;

    fn records(prev: u32, flipped: bool) -> Vec<(u32, u64)> {
        match (prev, flipped) {
            (1, false) | (2, true) => vec![(1, 3), (2, 1)],
            _ => vec![(1, 1), (2, 3)],
        }
    }
    /// Each predecessor's row, read once per load: the bigram branch is
    /// `freq / 4 + 0.125`, the unigram branch `0.125`.
    struct RowCached<'a>(&'a Cell<bool>);
    impl LanguageModel for RowCached<'_> {
        fn amplification_lambda(&self) -> f32 {
            0.312_699
        }

        type Error = EngineError;
        fn nbest_successors_in_row<'a>(
            &self,
            prev: &PhraseToken,
            row: &'a mut NbestStepRow,
        ) -> Result<Option<crate::model::NbestSuccessors<'a>>, EngineError> {
            row_in(row, || {
                Some(MergedGram::new(4, records(*prev, self.0.get())))
            })
        }
        fn nbest_token_costs(&self, _: &PhraseToken) -> Result<NbestTokenCosts, EngineError> {
            Ok(NbestTokenCosts {
                elem: Some(0.25),
                lambda: 1.0,
                unigram_lambda: 0.5,
            })
        }
    }
    let dict = FixtureDictionary::parse(
        "token=1\tkeys=ni\ttext=你\tunigram=1000\ntoken=2\tkeys=ni\ttext=尼\tunigram=900\n",
    )
    .expect("authored fixture");
    let (matrix, bound) = one_column_matrix("nini");
    let flipped = Cell::new(false);
    // From the seed 1, the row that counts a successor 3 of 4 prices it at
    // 0.875 and the other at 0.375. Unflipped: 1 → 1 → 1 at 0.875², then
    // two rows at 0.875 × 0.375, through 1 (slot 0, earlier batch
    // position) and through 2. Flipped, the same with the first token
    // swapped.
    let row = |text: &str, tokens: [u32; 2]| (Some(text.to_owned()), tokens.to_vec());
    for (flip, expected) in [
        (
            false,
            [
                row("你你", [1, 1]),
                row("你尼", [1, 2]),
                row("尼尼", [2, 2]),
            ],
        ),
        (
            true,
            [
                row("尼你", [2, 1]),
                row("尼尼", [2, 2]),
                row("你尼", [1, 2]),
            ],
        ),
    ] {
        flipped.set(flip);
        let rows = nbest_sentences(
            &matrix,
            bound,
            &dict,
            &RowCached(&flipped),
            &[1],
            None,
            NbestShape::PINYIN,
        )
        .unwrap();
        let decoded = rows
            .iter()
            .map(|row| {
                (
                    row.text
                        .as_ref()
                        .and_then(PhraseText::as_str)
                        .map(str::to_owned),
                    row.spans.iter().map(|span| span.token).collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(decoded, expected, "flipped = {flip}");
    }
}

// Basis: phonetic_lookup.h:594–632 and ngram.cpp:167–173 at 074a2219.
#[test]
fn sparse_bigram_walk_skips_absent_tokens_and_keeps_span_order() {
    use crate::model::{MergedGram, NbestStepRow};

    /// The bigram branch is `freq / 4 + 0.25`, the unigram branch `0.25`.
    /// Token 4 is stored with a zero count, a present successor.
    struct Sparse;
    impl LanguageModel for Sparse {
        fn amplification_lambda(&self) -> f32 {
            0.312_699
        }

        type Error = EngineError;

        fn nbest_successors_in_row<'a>(
            &self,
            _: &PhraseToken,
            row: &'a mut NbestStepRow,
        ) -> Result<Option<crate::model::NbestSuccessors<'a>>, EngineError> {
            row_in(row, || {
                Some(MergedGram::new(4, vec![(1, 2), (3, 1), (4, 0)]))
            })
        }

        fn nbest_token_costs(&self, _: &PhraseToken) -> Result<NbestTokenCosts, EngineError> {
            Ok(NbestTokenCosts {
                elem: Some(0.25),
                lambda: 1.0,
                unigram_lambda: 1.0,
            })
        }
    }

    let mut entries = crate::model::PhraseIndexRanges::new();
    entries.convert([3, 2, 1, 3, 4].map(|token| (token, Some(priced_slot(token)))));
    let mut trellis = super::Trellis::with_seeds(1, &[11], NbestShape::PINYIN);
    let beam = trellis.beam(0);
    super::expand_entries(&pricing(&entries), &beam, 0..1, &Sparse, &mut trellis, None).unwrap();
    // The ranges are [3], [2], [1], [3, 4]. Bigram steps 3, 1, 3, 4 in span
    // order: 2 is absent from the row, and 4's stored zero is kept, at
    // 0 / 4 + 0.25. Then unigram steps 3, 2, 1, 3, 4 from the head. Nodes
    // appear in step order, so node 4 precedes node 2 and holds both of its
    // steps; each node keeps its first two values, and a tie never evicts.
    let ln = |probability: f64| probability.ln() as f32;
    assert_eq!(
        trellis
            .candidates(1)
            .iter()
            .map(|entry| (entry.value.token, entry.value.poss.to_bits()))
            .collect::<Vec<_>>(),
        [
            (3, ln(0.5)),
            (3, ln(0.5)),
            (1, ln(0.75)),
            (1, ln(0.25)),
            (4, ln(0.25)),
            (4, ln(0.25)),
            (2, ln(0.25)),
        ]
        .map(|(token, poss)| (token, poss.to_bits()))
    );
}

/// A model whose every predecessor's merged row is the given gram
/// (`None`: both grams absent), with flat step costs.
struct FaultRows(Option<crate::model::MergedGram>);

impl LanguageModel for FaultRows {
    fn amplification_lambda(&self) -> f32 {
        0.312_699
    }

    type Error = EngineError;

    fn nbest_successors_in_row<'a>(
        &self,
        _: &PhraseToken,
        row: &'a mut crate::model::NbestStepRow,
    ) -> Result<Option<crate::model::NbestSuccessors<'a>>, EngineError> {
        row_in(row, || self.0.clone())
    }

    fn nbest_token_costs(&self, _: &PhraseToken) -> Result<NbestTokenCosts, EngineError> {
        Ok(NbestTokenCosts {
            elem: Some(0.25),
            lambda: 1.0,
            unigram_lambda: 1.0,
        })
    }
}

/// A stored gram value from its total, whole `(token, freq)` items and a
/// trailing partial tail.
fn gram_value(total: u32, items: &[(u32, u32)], tail: &[u8]) -> Vec<u8> {
    let mut value = total.to_le_bytes().to_vec();
    for (token, freq) in items {
        value.extend_from_slice(&token.to_le_bytes());
        value.extend_from_slice(&freq.to_le_bytes());
    }
    value.extend_from_slice(tail);
    value
}

/// One n-best step over predecessor 11's `row`, priced for tokens 1 and 2.
fn expand_with_row(
    row: Option<crate::model::MergedGram>,
    one_step: Option<u32>,
) -> Result<(), EngineError> {
    let mut entries = crate::model::PhraseIndexRanges::new();
    entries.convert([1, 2].map(|token| (token, Some(priced_slot(token)))));
    let mut trellis = super::Trellis::with_seeds(1, &[11], NbestShape::PINYIN);
    let beam = trellis.beam(0);
    super::expand_entries(
        &pricing(&entries),
        &beam,
        0..1,
        &FaultRows(row),
        &mut trellis,
        one_step,
    )
}

// Basis: phonetic_lookup.h:594–632 and ngram.cpp:165–173,231–248,303–349 at 074a2219.
#[test]
fn a_predecessor_row_the_pin_cannot_read_refuses_the_step() {
    use crate::model::MergedGram;

    let unsafe_read = EngineError::UnsafeLookupPredecessor;
    let alone = || MergedGram::merge(None, Some(vec![0, 0]));
    assert_eq!(expand_with_row(alone(), Some(1)), Err(unsafe_read));
    let tailed = || MergedGram::merge(None, Some(gram_value(3, &[(1, 3)], &[9])));
    assert_eq!(expand_with_row(tailed(), None), Err(unsafe_read));
    assert_eq!(expand_with_row(tailed(), Some(1)), Ok(()));
    assert_eq!(
        expand_with_row(None, None),
        Ok(()),
        "both grams absent: no bigram step"
    );
}

// Basis: phonetic_lookup.h:594–632 and ngram.cpp:165–173,231–248,303–349 at 074a2219.
#[test]
fn a_short_merge_aborts_the_step() {
    crate::store::death_test::assert_aborts(
        "engine::nbest::tests::a_short_merge_aborts_the_step",
        || {
            let row = crate::model::MergedGram::merge(
                Some(gram_value(1, &[(1, 1)], &[])),
                Some(vec![0, 0]),
            );
            let _ = expand_with_row(row, None);
        },
    );
}

// Basis: phonetic_lookup.h:594–632 and ngram.cpp:165–173,231–248,303–349 at 074a2219.
#[test]
fn a_short_lone_row_aborts_the_ranged_step() {
    crate::store::death_test::assert_aborts(
        "engine::nbest::tests::a_short_lone_row_aborts_the_ranged_step",
        || {
            let row = crate::model::MergedGram::merge(None, Some(vec![0, 0]));
            let _ = expand_with_row(row, None);
        },
    );
}

// Basis: phonetic_lookup.h:594–632 and ngram.cpp:167–173 at 074a2219.
#[test]
fn a_token_the_walk_yields_below_its_range_still_steps() {
    use crate::model::{MergedGram, NbestStepRow};

    /// The bigram branch is the item's f32 `freq / total`; the unigram
    /// branch never steps.
    struct Walked;
    impl LanguageModel for Walked {
        fn amplification_lambda(&self) -> f32 {
            0.312_699
        }

        type Error = EngineError;

        fn nbest_successors_in_row<'a>(
            &self,
            _: &PhraseToken,
            row: &'a mut NbestStepRow,
        ) -> Result<Option<crate::model::NbestSuccessors<'a>>, EngineError> {
            row_in(row, || {
                Some(MergedGram::new(15, vec![(1, 1), (3, 5), (2, 7)]))
            })
        }

        fn nbest_token_costs(&self, _: &PhraseToken) -> Result<NbestTokenCosts, EngineError> {
            Ok(NbestTokenCosts {
                elem: Some(0.0),
                lambda: 1.0,
                unigram_lambda: 1.0,
            })
        }
    }

    let mut entries = crate::model::PhraseIndexRanges::new();
    entries.convert([1, 3, 2].map(|token| (token, Some(priced_slot(token)))));
    let mut trellis = super::Trellis::with_seeds(1, &[11], NbestShape::PINYIN);
    let beam = trellis.beam(0);
    super::expand_entries(&pricing(&entries), &beam, 0..1, &Walked, &mut trellis, None).unwrap();
    // Range 3's walk yields 3 and then 2, below it; each steps with its own
    // frequency, in walk order.
    let ln = |freq: u32| f64::from(freq as f32 / 15.0_f32).ln() as f32;
    assert_eq!(
        trellis
            .candidates(1)
            .iter()
            .map(|entry| (entry.value.token, entry.value.poss.to_bits()))
            .collect::<Vec<_>>(),
        [(1, ln(1)), (3, ln(5)), (2, ln(7))].map(|(token, poss)| (token, poss.to_bits()))
    );
}

// Basis: phonetic_lookup.h:329–339 at 074a2219.
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

// Basis: pinyin.cpp:406–409 and zhuyin.cpp:348–351 at 074a2219, with register row 152 for ties.
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
