//! Integration tests for `BigramLanguageModel` over `bigram.db` and the
//! chunk-file unigrams.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use oxpinyin_core::{LanguageModel, PhraseToken, UserCountDelta};
use oxpinyin_data::{BigramLanguageModel, PhraseLibraries, SYSTEM_LIBRARY_FILES, SystemDbm};
use oxpinyin_store::{DefaultStore, WriteStore};

mod support;
use support::ChunkBuilder;
use tempfile::TempDir;

const NI: u32 = 0x0100_0010;
const HAO: u32 = 0x0100_0011;
const NIHAO: u32 = 0x0100_0099;
const GBK: u32 = 0x0200_0001;

struct Fixture {
    dir: TempDir,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::Builder::new()
            .prefix("oxpinyin-lm-test-")
            .tempdir()
            .unwrap();
        // gb_char: 你 count 9 (+1 = 10), 好 count 4 (+1 = 5), 你好 never
        // seen (+1 = 1) → total 16, items 3.
        let mut gb = ChunkBuilder::new(16);
        gb.add(0x10, 10, "你", vec![(vec![0, 0], 1)]);
        gb.add(0x11, 5, "好", vec![(vec![0, 0], 1)]);
        gb.add(0x99, 1, "你好", vec![(vec![0, 0, 0, 0], 1)]);
        std::fs::write(dir.path().join("gb_char.bin"), gb.build()).unwrap();
        // gbk_char: one item, count 2 (+1 = 3).
        let mut gbk = ChunkBuilder::new(3);
        gbk.add(0x01, 3, "鎄", vec![(vec![0, 0], 1)]);
        std::fs::write(dir.path().join("gbk_char.bin"), gbk.build()).unwrap();
        // bigram.db: 你 → 好 ×7 (row total 7).
        let store =
            DefaultStore::create_hash(&dir.path().join(SystemDbm::Bigram.file_name())).unwrap();
        store
            .write(|txn| {
                let mut value = 7_u32.to_le_bytes().to_vec();
                value.extend_from_slice(&HAO.to_le_bytes());
                value.extend_from_slice(&7_u32.to_le_bytes());
                txn.put_raw(&NI.to_le_bytes(), &value)
            })
            .unwrap();
        Self { dir }
    }

    fn open(&self) -> (BigramLanguageModel, Arc<AtomicU32>) {
        let libraries =
            Arc::new(PhraseLibraries::open(self.dir.path(), SYSTEM_LIBRARY_FILES).unwrap());
        let mask = Arc::new(AtomicU32::new(0));
        let lm = BigramLanguageModel::open_with_mask(
            &self.dir.path().join(SystemDbm::Bigram.file_name()),
            libraries,
            Arc::clone(&mask),
        )
        .unwrap();
        (lm, mask)
    }
}

#[test]
fn unigrams_are_the_chunk_item_fields() {
    let fix = Fixture::new();
    let (lm, _) = fix.open();
    assert_eq!(
        lm.unigram_count(NI),
        Some(10),
        "\\1-gram 9 + gen_unigram's 1"
    );
    assert_eq!(
        lm.unigram_count(NIHAO),
        Some(1),
        "never seen: still 1, never 0"
    );
    assert_eq!(lm.unigram_count(GBK), Some(3));
    assert_eq!(lm.unigram_count(0x0300_0001), None, "no such library");
    assert_eq!(lm.unigram_total(), 16 + 3, "Σ item over both chunks");
    assert!(lm.has_unigrams());
    assert!(lm.has_real_unigrams());
    assert_eq!(
        LanguageModel::unigram_freq(&lm, &PhraseToken::new(NI)).unwrap(),
        Some(10)
    );
    assert_eq!(LanguageModel::unigram_total(&lm).unwrap(), Some(19));
}

#[test]
fn an_unloaded_library_leaves_the_model() {
    let fix = Fixture::new();
    let (lm, mask) = fix.open();
    mask.store(1 << 2, Ordering::SeqCst);
    assert_eq!(lm.unigram_count(GBK), None);
    assert_eq!(lm.unigram_total(), 16, "gbk's total leaves");
    assert_eq!(lm.unigram_count(NI), Some(10));
}

#[test]
fn bigram_rows_are_point_reads() {
    let fix = Fixture::new();
    let (lm, _) = fix.open();
    let row = lm.load_successors(NI).unwrap().expect("你 has a row");
    assert_eq!(row.total, 7);
    assert_eq!(row.records, vec![(HAO, 7)]);
    assert!(lm.load_successors(HAO).unwrap().is_none());
}

#[test]
fn scoring_blends_bigram_and_unigram_over_the_corpus_counts() {
    let fix = Fixture::new();
    let (lm, _) = fix.open();
    let ni = PhraseToken::new(NI);
    let hao = PhraseToken::new(HAO);
    let nihao = PhraseToken::new(NIHAO);
    // 好 after 你: a seen transition costs less than the unigram alone.
    let seen = lm.score(&[ni], &hao, 0).unwrap();
    let unseen = lm.score(&[hao], &ni, 0).unwrap();
    assert!(seen < unseen, "seen {seen} vs unseen {unseen}");
    // A never-seen phrase carries gen_unigram's 1: finite, never the floor.
    let rare = lm.score(&[], &nihao, 0).unwrap();
    assert!(rare < oxpinyin_core::cost::UNKNOWN_COST);
    assert!(rare > lm.score(&[], &ni, 0).unwrap(), "rarer than 你");
    // A token with no item at all does floor.
    let absent = lm.score(&[], &PhraseToken::new(0x0300_0001), 0).unwrap();
    assert_eq!(absent, oxpinyin_core::cost::UNKNOWN_COST);
    // The user overlay raises the rare one.
    let raised = lm
        .score_with_user_delta(
            &[],
            &nihao,
            0,
            UserCountDelta {
                unigram_delta: 3,
                unigram_total_delta: 3,
                ..UserCountDelta::ZERO
            },
        )
        .unwrap();
    assert!(raised < rare);
    // n-best step costs carry both branches for the seen transition.
    let step = lm.nbest_step_costs(&ni, &hao).unwrap();
    assert!(step.blended.is_some());
    assert!(step.unigram.is_some());
}

#[test]
fn wrapped_bigram_fields_keep_record_presence_and_nan_costs() {
    use oxpinyin_core::cost::{NAN_COST, NEG_INF_COST};

    let fix = Fixture::new();
    let (lm, _) = fix.open();
    let ni = PhraseToken::new(NI);
    let hao = PhraseToken::new(HAO);
    // Both stored system fields are 7: adding MAX - 6 wraps both to zero.
    let delta = UserCountDelta {
        bigram_count: u64::from(u32::MAX - 6),
        bigram_total: u64::from(u32::MAX - 6),
        ..UserCountDelta::ZERO
    };
    for edge in [0, 17] {
        assert_eq!(
            lm.score_with_user_delta(&[ni], &hao, edge, delta).unwrap(),
            NAN_COST
        );
    }
    assert_eq!(
        lm.nbest_step_costs_with_user_delta(&ni, &hao, delta)
            .unwrap()
            .blended,
        Some(NAN_COST)
    );
    // A user-only positive successor survives a wrapped total of zero.
    let delta = UserCountDelta {
        bigram_count: u64::from(u32::MAX),
        bigram_total: 0,
        ..UserCountDelta::ZERO
    };
    assert_eq!(
        lm.score_with_user_delta(&[hao], &ni, 0, delta).unwrap(),
        NEG_INF_COST
    );
    assert_eq!(
        lm.nbest_step_costs_with_user_delta(&hao, &ni, delta)
            .unwrap()
            .blended,
        Some(NEG_INF_COST)
    );
    // The same predecessor total does not invent a missing successor.
    let absent = UserCountDelta {
        bigram_count: 0,
        ..delta
    };
    assert_eq!(
        lm.nbest_step_costs_with_user_delta(
            &ni,
            &PhraseToken::new(NIHAO),
            UserCountDelta {
                bigram_total: u64::from(u32::MAX - 6),
                ..absent
            }
        )
        .unwrap()
        .blended,
        None
    );
}

/// The n-best walk's form of one step: the predecessor's user gram merged
/// with its system gram, read with `get_freq` and `get_total_freq`, then
/// priced from that item.
fn merged_row_costs(
    lm: &BigramLanguageModel,
    prev: u32,
    token: u32,
    user: UserCountDelta,
    user_gram: (u64, Vec<(u32, u64)>),
) -> oxpinyin_core::NbestStepCosts {
    use oxpinyin_core::{MergedGram, NbestStepRow};
    let bytes = MergedGram::new(user_gram.0, user_gram.1)
        .as_bytes()
        .to_vec();
    let mut row = NbestStepRow::default();
    let gram = row
        .get_or_try_init(|| lm.load_merged_row(prev, Some(bytes)))
        .unwrap()
        .expect("the user gram is present");
    let item = gram
        .get_freq(token)
        .unwrap()
        .map(|freq| (freq, gram.total().unwrap()));
    lm.nbest_item_costs(&PhraseToken::new(token), user, false, item)
        .unwrap()
}

/// #791: the pin takes `log` of the blend with no cap
/// (`phonetic_lookup.h:692-693`), and a merged `SingleGram` total that wrapped
/// below a stored count (`ngram.cpp:305`) puts it above one, so the cost is
/// negative — not the zero a clamp would give.
#[test]
fn a_blend_above_one_from_a_wrapped_total_is_a_negative_cost() {
    let fix = Fixture::new();
    let (lm, _) = fix.open();
    let ni = PhraseToken::new(NI);
    let hao = PhraseToken::new(HAO);
    // System 你 → 好 is (7, 7). A user total of 2^32 - 6 wraps the merged
    // total to exactly 1 while the merged count is 7 + 3 = 10: the planted
    // sentence-start state of #791 in miniature.
    let user_total = u64::from(u32::MAX) - 5;
    let delta = UserCountDelta {
        bigram_count: 3,
        bigram_total: user_total,
        ..UserCountDelta::ZERO
    };
    // Independently evaluated with IEEE f32 rounding (Python struct.pack/unpack):
    // f32(λ * f32(10 / 1)) = 3.1269898414611816;
    // f32(1 - f64(λ)) * (5 / 19) = 0.1808686946567736 in f64.
    // The f64 sum is 3.3078585361179553: -1725.89753753063 millibits.
    let expected = Some(-1726);
    let pair = lm
        .nbest_step_costs_with_user_delta(&ni, &hao, delta)
        .unwrap();
    assert_eq!(pair.blended, expected);
    // The merged-row form the n-best walk takes agrees, as does the unigram
    // branch, which no wrap touches.
    let raw = merged_row_costs(
        &lm,
        NI,
        HAO,
        UserCountDelta::ZERO,
        (user_total, vec![(HAO, 3)]),
    );
    assert_eq!(raw.blended, expected);
    assert_eq!(raw.unigram, pair.unigram);
    assert!(pair.unigram.unwrap() > 0, "the unigram branch is a cost");
}

/// Codex P2 on #796: an above-one blend at a millibit rounding boundary.
#[test]
fn an_above_one_blend_keeps_the_pins_mixed_precision_rounding() {
    let fix = Fixture::new();
    let (lm, _) = fix.open();
    let ni = PhraseToken::new(NI);
    let hao = PhraseToken::new(HAO);
    // Final counts: b=2134351507, bt=602365351, u=21950320,
    // ut=4294967295, with the default lambda. The unigram total is the
    // facade's guint32, so it stays at or under u32::MAX. Subtract the
    // fixture's system counts (7, 7, 5, 19) so the merged fields equal those
    // values.
    let delta = UserCountDelta {
        bigram_count: 2_134_351_507 - 7,
        bigram_total: 602_365_351 - 7,
        unigram_delta: 21_950_320 - 5,
        unigram_total_delta: 4_294_967_295 - 19,
    };
    // Independently evaluated with Python struct.pack/unpack for each f32
    // operation, following 074a2219 phonetic_lookup.h:605-608, :692-693:
    // bigram_poss = 3.543283700942993; λ * poss = 1.1079812049865723;
    // the f64 unigram term is 0.0035125943276022526, giving the f64 sum
    // 1.1114937993141745 and -152.49990003054108 millibits, rounded to -152.
    // Logging the exact rational instead gives -153 (-152.50010327593435).
    let pair = lm
        .nbest_step_costs_with_user_delta(&ni, &hao, delta)
        .unwrap();
    assert_eq!(pair.blended, Some(-152));

    let raw = merged_row_costs(
        &lm,
        NI,
        HAO,
        delta,
        (delta.bigram_total, vec![(HAO, delta.bigram_count)]),
    );
    assert_eq!(raw.blended, Some(-152));
    assert_eq!(raw.unigram, pair.unigram);
}

/// #798: the n-best denominator is the facade's `guint32` total
/// (074a2219 `phrase_index.h:440`), which the pin adds to with no overflow
/// check, so an extra of exactly 2^32 prices every step as no extra does.
#[test]
fn the_nbest_denominator_wraps_as_the_facade_total() {
    let fix = Fixture::new();
    let (lm, _) = fix.open();
    let ni = PhraseToken::new(NI);
    let hao = PhraseToken::new(HAO);
    assert_eq!(lm.facade_total(0), 19);
    assert_eq!(lm.facade_total(u64::from(u32::MAX)), 18);
    let wrapped = UserCountDelta {
        unigram_total_delta: 1 << 32,
        ..UserCountDelta::ZERO
    };
    assert_eq!(
        lm.nbest_step_costs_with_user_delta(&ni, &hao, wrapped)
            .unwrap(),
        lm.nbest_step_costs_with_user_delta(&ni, &hao, UserCountDelta::ZERO)
            .unwrap()
    );
}

/// #801 review: whether there is a unigram table to price with is decided
/// by the loaded libraries' items, not by their stored totals, which can
/// wrap to zero as a `guint32` while every item stands.
#[test]
fn unigram_availability_survives_a_total_that_wraps_to_zero() {
    let fix = Fixture::new();
    let mut gb = ChunkBuilder::new(u32::MAX - 2);
    gb.add(0x10, 10, "你", vec![(vec![0, 0], 1)]);
    gb.add(0x11, 5, "好", vec![(vec![0, 0], 1)]);
    gb.add(0x99, 1, "你好", vec![(vec![0, 0, 0, 0], 1)]);
    std::fs::write(fix.dir.path().join("gb_char.bin"), gb.build()).unwrap();
    let (lm, mask) = fix.open();
    assert_eq!(
        lm.unigram_total(),
        0,
        "gb_char's total plus gbk_char's 3 wraps"
    );
    assert!(lm.has_unigrams());
    mask.store((1 << 1) | (1 << 2), Ordering::SeqCst);
    assert!(!lm.has_unigrams(), "no visible library holds an item");
}

/// The §12 presence gate (revert-plan.md §12, register #35): the pin's
/// `unigram_gen_next_step` prices any *loaded* sub-index's item
/// (`get_phrase_item`), and the three `USER_FILE` sub-indexes (nibbles
/// 5/6/7 — promoted addon, network, user) live in the default facade
/// with their counts in the user directory, so their tokens are priced
/// from the user delta alone — while a masked library's token, a loaded
/// library's missing item and a non-existent library keep the default
/// answer (`ERROR_NO_SUB_PHRASE_INDEX` / `ERROR_NO_ITEM`).
#[test]
fn user_file_tokens_are_priced_from_their_user_delta_alone() {
    let fix = Fixture::new();
    let (lm, _) = fix.open();
    let start = PhraseToken::new(1);

    // A zero delta answers the default: no phantom rows for a user-file
    // token the store does not carry.
    let zero = lm
        .nbest_step_costs_with_user_delta(
            &start,
            &PhraseToken::new(0x0700_0002),
            UserCountDelta::ZERO,
        )
        .unwrap();
    assert_eq!(zero, oxpinyin_core::NbestStepCosts::default());

    // A carried delta prices the step — the same additive merge a system
    // item takes, over `0 + delta`.
    let delta = UserCountDelta {
        unigram_delta: 27,
        unigram_total_delta: 27,
        ..UserCountDelta::ZERO
    };
    let priced = lm
        .nbest_step_costs_with_user_delta(&start, &PhraseToken::new(0x0700_0002), delta)
        .unwrap();
    assert!(priced.unigram.is_some(), "a carried delta must price");
    assert!(priced.blended.is_none(), "no bigram row involves it");
    // A larger delta is cheaper (more frequent), and the three user-file
    // libraries price identically — the promoted addon nibble is not
    // special.
    let bigger = lm
        .nbest_step_costs_with_user_delta(
            &start,
            &PhraseToken::new(0x0700_0002),
            UserCountDelta {
                unigram_delta: 270,
                unigram_total_delta: 270,
                ..UserCountDelta::ZERO
            },
        )
        .unwrap();
    assert!(bigger.unigram.unwrap() < priced.unigram.unwrap());
    for token in [0x0500_0001_u32, 0x0600_0001, 0x0700_0002] {
        let step = lm
            .nbest_step_costs_with_user_delta(&start, &PhraseToken::new(token), delta)
            .unwrap();
        assert_eq!(step, priced, "nibble {} prices like nibble 7", token >> 24);
    }

    // A user bigram takes the blended branch over the merged counts — the
    // merge-before-gate invariant extended to user-file tokens.
    let with_bigram = lm
        .nbest_step_costs_with_user_delta(
            &PhraseToken::new(NI),
            &PhraseToken::new(0x0700_0002),
            UserCountDelta {
                bigram_count: 5,
                bigram_total: 5,
                unigram_delta: 27,
                unigram_total_delta: 27,
            },
        )
        .unwrap();
    assert!(with_bigram.blended.is_some());

    // The gate keeps its refusals: a masked user library, a loaded
    // library's missing item and a non-existent library answer the
    // default however large the delta — the counterfactual's
    // `unwrap_or(0)` would have priced all three (register #35).
    let (lm2, mask) = fix.open();
    mask.store(1 << 7, Ordering::SeqCst);
    let masked = lm2
        .nbest_step_costs_with_user_delta(&start, &PhraseToken::new(0x0700_0002), delta)
        .unwrap();
    assert_eq!(masked, oxpinyin_core::NbestStepCosts::default());
    let (lm3, _) = fix.open();
    for token in [0x0100_0050_u32, 0x0300_0001, 0x0400_0001, 0x0800_0001] {
        let step = lm3
            .nbest_step_costs_with_user_delta(&start, &PhraseToken::new(token), delta)
            .unwrap();
        assert_eq!(
            step,
            oxpinyin_core::NbestStepCosts::default(),
            "nibble {} without an item keeps the default answer",
            token >> 24
        );
    }
}

/// The merged row prices each item as the per-pair overlay does wherever
/// that overlay can express the row: one load and merge per predecessor,
/// `get_freq` per successor. A stored zero-count user item is the one
/// thing the pair overlay cannot express (its presence gate is a positive
/// count); the merged row keeps it, as the pin's merge does, and it takes
/// the blended branch.
#[test]
fn merged_row_items_price_as_the_pair_overlay_at_wrap_presence_and_lambda_edges() {
    use oxpinyin_core::{MergedGram, NbestStepRow};
    let fix = Fixture::new();
    let (mut lm, _) = fix.open();
    let user_rows = [
        (NI, u64::from(u32::MAX)),
        (HAO, u64::from(u32::MAX - 6)),
        (NIHAO, 0),
    ];
    for lambda in ["0.312699", "0.5", "0", "1e-1", "nan", "inf", "-1"] {
        let conf = oxpinyin_data::table_info::TableConf::parse(format!(
            "binary format version:7\nmodel data version:12\nlambda parameter:{lambda}\nsource table format:pinyin\ndatabase format:BerkeleyDB\n").as_bytes()).unwrap();
        lm.set_lambda_from_conf(&conf);
        for prev in [NI, HAO] {
            for total in [0, u64::from(u32::MAX - 6), u64::from(u32::MAX), 17] {
                let user_gram = MergedGram::new(total, user_rows.to_vec())
                    .as_bytes()
                    .to_vec();
                let mut row = NbestStepRow::default();
                let mut loads = 0;
                for (token, bigram_count) in user_rows.into_iter().chain([(GBK, 0)]) {
                    let user = UserCountDelta {
                        bigram_count,
                        bigram_total: total,
                        unigram_delta: 3,
                        unigram_total_delta: 9,
                    };
                    let expected = lm
                        .nbest_step_costs_with_user_delta(
                            &PhraseToken::new(prev),
                            &PhraseToken::new(token),
                            user,
                        )
                        .unwrap();
                    let gram = row
                        .get_or_try_init(|| {
                            loads += 1;
                            lm.load_merged_row(prev, Some(user_gram.clone()))
                        })
                        .unwrap()
                        .expect("the user gram is present");
                    let item = gram
                        .get_freq(token)
                        .unwrap()
                        .map(|freq| (freq, gram.total().unwrap()));
                    let actual = lm
                        .nbest_item_costs(&PhraseToken::new(token), user, false, item)
                        .unwrap();
                    if token == NIHAO {
                        assert_eq!(item.map(|(freq, _)| freq), Some(0), "the stored zero stays");
                        assert_eq!(actual.unigram, expected.unigram);
                        assert!(actual.blended.is_some(), "prev={prev} total={total}");
                    } else {
                        assert_eq!(actual, expected, "prev={prev} token={token} total={total}");
                    }
                }
                assert_eq!(loads, 1, "one load/merge for all successors");
            }
        }
    }
}

#[test]
fn reusable_row_keeps_visibility_live_and_propagates_empty_value() {
    use oxpinyin_core::NbestStepRow;
    let fix = Fixture::new();
    let store =
        DefaultStore::create_hash(&fix.dir.path().join(SystemDbm::Bigram.file_name())).unwrap();
    store
        .write(|txn| txn.put_raw(&NI.to_le_bytes(), &[]))
        .unwrap();
    drop(store);
    let (lm, mask) = fix.open();
    let mut row = NbestStepRow::default();
    let prev = PhraseToken::new(NI);
    mask.store(1 << 2, Ordering::SeqCst);
    assert_eq!(
        lm.nbest_item_costs_in_row(&prev, &PhraseToken::new(GBK), Some((1, 1)), &mut row)
            .unwrap(),
        oxpinyin_core::NbestStepCosts::default()
    );
    let Err(error) = lm.nbest_successors_in_row(&prev, &mut row) else {
        panic!("a present empty value is corruption");
    };
    assert!(
        matches!(error, oxpinyin_data::lm::LmError::Parse(ref text) if text.contains("empty value"))
    );
    assert_eq!(
        error.to_string(),
        lm.nbest_step_costs(&prev, &PhraseToken::new(HAO))
            .unwrap_err()
            .to_string()
    );
}

/// A stored system row enters the merge as stored, and both the merged
/// row and the pair path read it with the pin's `get_freq`: `lower_bound`
/// over the unsorted items, then the first match walking on. On
/// `[(NIHAO, 0), (HAO, 7), (NI, 4), (HAO, 99)]` `lower_bound(HAO)` lands
/// on index 3, so `HAO` answers 99, not the first record's 7.
#[test]
fn row_and_pair_read_a_stored_unsorted_row_through_the_pins_lower_bound() {
    use oxpinyin_core::NbestSuccessors;
    let fix = Fixture::new();
    let store =
        DefaultStore::create_hash(&fix.dir.path().join(SystemDbm::Bigram.file_name())).unwrap();
    let value = oxpinyin_data::row_format::bigram::encode_value(&oxpinyin_data::BigramRow {
        total: 11,
        records: vec![(NIHAO, 0), (HAO, 7), (NI, 4), (HAO, 99)],
    });
    store
        .write(|txn| txn.put_raw(&NI.to_le_bytes(), &value))
        .unwrap();
    drop(store);
    let (lm, _) = fix.open();
    let mut row = oxpinyin_core::NbestStepRow::default();
    let prev = PhraseToken::new(NI);
    for (token, freq) in [(HAO, Some(99)), (NIHAO, None), (NI, None), (GBK, None)] {
        let item = match lm.nbest_successors_in_row(&prev, &mut row).unwrap() {
            Some(NbestSuccessors::Row(gram)) => {
                assert_eq!(gram.as_bytes(), value.as_slice(), "stored, not sorted");
                assert_eq!(gram.get_freq(token).unwrap(), freq, "token {token:#x}");
                gram.get_freq(token)
                    .unwrap()
                    .map(|freq| (freq, gram.total().unwrap()))
            }
            _ => panic!("the system row is present"),
        };
        let token = PhraseToken::new(token);
        assert_eq!(
            lm.nbest_item_costs_in_row(&prev, &token, item, &mut row)
                .unwrap(),
            lm.nbest_step_costs(&prev, &token).unwrap()
        );
    }
}
