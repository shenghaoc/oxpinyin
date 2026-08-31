use super::*;

/// 你's `gb_char` token in the pinned model.
const NI: u32 = 0x0100_1225;
/// 的's `gb_char` token in the pinned model.
const DE: u32 = 0x0100_05db;

fn fixtures_dir() -> std::path::PathBuf {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    std::path::PathBuf::from(manifest)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("fixtures")
        .join("w3")
}

fn model() -> BigramLanguageModel {
    BigramLanguageModel::open(&fixtures_dir().join(crate::default_store_file("bigram"))).unwrap()
}

#[test]
fn mini_fixture_opens() {
    assert!(model().entry_count().unwrap() > 0);
}

#[test]
fn observed_transition_is_cheaper_than_novel() {
    let model = model();
    let history = [PhraseToken::new(NI)];
    let observed = model
        .score(&history, &PhraseToken::new(DE), 0)
        .expect("你 → 的 scores");
    let novel = model
        .score(&history, &PhraseToken::new(0x0100_0001), 0)
        .expect("你 → rare scores");
    assert!(
        observed < novel,
        "你 → 的 ({observed}) must undercut a novel transition ({novel})"
    );
}

#[test]
fn empty_history_returns_edge_cost_without_unigrams() {
    let cost = model().score(&[], &PhraseToken::new(DE), 1234).unwrap();
    assert_eq!(cost, 1234);
}

#[test]
fn empty_history_uses_scaled_unigram_when_installed() {
    let mut model = model();
    let mut unigrams = BTreeMap::new();
    unigrams.insert(DE, 100);
    unigrams.insert(NI, 10);
    model.set_unigrams(unigrams, 110);

    let de = model.score(&[], &PhraseToken::new(DE), 0).unwrap();
    let ni = model.score(&[], &PhraseToken::new(NI), 0).unwrap();
    assert!(
        de < ni,
        "higher unigram count must cost less: de={de} ni={ni}"
    );
    // Scaled, not the raw surprisal floor for a known token.
    let raw = surprisal(100, 110);
    assert_eq!(de, raw / UNIGRAM_TIEBREAK_SCALE);
    assert_eq!(
        model.score(&[], &PhraseToken::new(0x0100_0001), 0).unwrap(),
        UNKNOWN_COST
    );
}

#[test]
fn interpolation_prefers_observed_bigram() {
    let mut model = model();
    let mut unigrams = BTreeMap::new();
    // Equal unigrams so only the bigram term can separate them.
    unigrams.insert(DE, 50);
    unigrams.insert(0x0100_0001, 50);
    model.set_unigrams(unigrams, 100);

    let history = [PhraseToken::new(NI)];
    let observed = model
        .score(&history, &PhraseToken::new(DE), 0)
        .expect("你 → 的");
    let novel = model
        .score(&history, &PhraseToken::new(0x0100_0001), 0)
        .expect("你 → rare");
    assert!(
        observed < novel,
        "interpolated 你 → 的 ({observed}) must undercut a novel pair ({novel})"
    );
}

#[test]
fn a_no_entry_history_floors_instead_of_discounting() {
    // Regression: when the previous token has no bigram entry at all, the
    // transition is *unseen*, not merely rare. It must floor at
    // UNKNOWN_COST — the same floor a count-0 next-token gets — never a
    // discounted unigram, which used to rank an unseen transition below a
    // rare but observed one.
    const NO_ENTRY_PREV: u32 = 0xFFFF_FFFF;
    let mut model = model();
    let mut unigrams = BTreeMap::new();
    unigrams.insert(DE, 100);
    model.set_unigrams(unigrams, 110);

    assert!(
        matches!(model.transition(NO_ENTRY_PREV, DE), Ok(None)),
        "precondition: the previous token must be absent from the bigram"
    );

    let unseen = model
        .score(&[PhraseToken::new(NO_ENTRY_PREV)], &PhraseToken::new(DE), 0)
        .expect("scoring an unseen transition");
    assert_eq!(
        unseen, UNKNOWN_COST,
        "a no-entry history must floor at UNKNOWN_COST, not discount"
    );
}

#[test]
fn invariant_holds_for_every_fixture_entry() {
    let model = model();
    for (key, row) in &model.bigram {
        let prev = key.token();
        let sum: u64 = model.records[row.records.clone()]
            .iter()
            .map(|(_, count)| u64::from(*count))
            .sum();
        assert_eq!(
            u64::from(row.total),
            sum,
            "total == Σ count for prev {prev:#010x}"
        );
    }
}

#[test]
fn default_lambda_is_the_pinned_config_value() {
    // Not the old authored 1/2: the LM defaults to the pinned 0.312699.
    assert_eq!(model().lambda(), Lambda::PINNED);
}

#[test]
fn set_lambda_from_table_conf_reads_the_config() {
    let dir = std::env::temp_dir().join(format!("pinyin-lm-tableconf-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("table.conf");
    std::fs::write(
        &path,
        "binary format version:7\nlambda parameter:0.312699\n",
    )
    .unwrap();

    let mut model = model();
    assert!(
        model.set_lambda_from_table_conf(&path),
        "a present config is read"
    );
    assert_eq!(model.lambda(), Lambda::PINNED);

    // Absent config: returns false and leaves λ unchanged (the default
    // stands for the fetched cache, which ships no table.conf).
    assert!(!model.set_lambda_from_table_conf(&dir.join("does-not-exist.conf")));
    assert_eq!(model.lambda(), Lambda::PINNED);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn interpolate_ratio_floors_instead_of_overflowing() {
    // A pathological λ denominator whose products exceed u128 returns
    // None (the caller floors at UNKNOWN_COST) rather than panicking.
    assert_eq!(
        interpolate_ratio(1, 2, u128::MAX, u128::MAX, 1, u128::MAX),
        None
    );
    // Ordinary values interpolate fine.
    assert_eq!(
        interpolate_ratio(312_699, 1_000_000, 3, 10, 4, 100),
        Some((312_699 * 3 * 100 + 687_301 * 4 * 10, 1_000_000 * 10 * 100))
    );
}

#[test]
fn lambda_is_live_in_the_interpolated_cost() {
    // The bigram term is weighted by λ, so a different λ yields a
    // different interpolated cost for an observed transition — proof the
    // config value reaches the hot path rather than a stale constant.
    let mut model = model();
    let mut unigrams = BTreeMap::new();
    unigrams.insert(DE, 50);
    unigrams.insert(NI, 50);
    model.set_unigrams(unigrams, 100);
    let history = [PhraseToken::new(NI)];

    model.set_lambda(Lambda::PINNED);
    let at_pinned = model.score(&history, &PhraseToken::new(DE), 0).unwrap();

    // λ = 1/2, the old hardcoded value, via a synthetic table.conf.
    assert!(model.set_lambda_from_table_conf(&write_temp_conf("0.5")));
    let at_half = model.score(&history, &PhraseToken::new(DE), 0).unwrap();

    assert_ne!(
        at_pinned, at_half,
        "λ must change the interpolated bigram cost (0.312699 vs 0.5)"
    );
}

fn write_temp_conf(value: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "pinyin-lm-conf-{}-{value}.conf",
        std::process::id()
    ));
    std::fs::write(&path, format!("lambda parameter:{value}\n")).unwrap();
    path
}

#[test]
fn merge_counts_is_saturating_addition() {
    assert_eq!(merge_counts(10, 0), 10);
    assert_eq!(merge_counts(0, 10), 10);
    assert_eq!(merge_counts(10, 20), 30);
    assert_eq!(merge_counts(u64::MAX, 1), u64::MAX);
    assert_eq!(merge_counts(1, u64::MAX), u64::MAX);
}

#[test]
fn merge_bigram_empty_user_is_the_system_gram() {
    assert_eq!(merge_bigram(Some((3, 10)), 0, 0), Some((3, 10)));
    assert_eq!(merge_bigram(None, 0, 0), None);
    assert_eq!(merge_bigram(Some((0, 0)), 0, 0), None);
}

#[test]
fn merge_bigram_user_only_is_the_user_gram() {
    // merge_single_gram: system NULL + user present → user unchanged.
    assert_eq!(merge_bigram(None, 69, 69), Some((69, 69)));
    assert_eq!(merge_bigram(None, 0, 69), Some((0, 69)));
}

#[test]
fn merge_bigram_adds_both_sides() {
    assert_eq!(merge_bigram(Some((10, 100)), 69, 69), Some((79, 169)));
    assert_eq!(
        merge_bigram(Some((u32::MAX, u32::MAX)), 1, 1),
        Some((u64::from(u32::MAX) + 1, u64::from(u32::MAX) + 1))
    );
}

#[test]
fn zero_overlay_is_bit_identical_to_system_score() {
    let mut model = model();
    let mut unigrams = BTreeMap::new();
    unigrams.insert(DE, 50);
    unigrams.insert(NI, 50);
    model.set_unigrams(unigrams, 100);
    let history = [PhraseToken::new(NI)];
    let token = PhraseToken::new(DE);

    let system = model.score(&history, &token, 0).unwrap();
    let merged = model
        .score_with_user_delta(&history, &token, 0, UserCountDelta::ZERO)
        .unwrap();
    assert_eq!(system, merged, "a zero overlay must be identity");

    let empty_history_system = model.score(&[], &token, 7).unwrap();
    let empty_history_merged = model
        .score_with_user_delta(&[], &token, 7, UserCountDelta::ZERO)
        .unwrap();
    assert_eq!(empty_history_system, empty_history_merged);
}

#[test]
fn populated_overlay_lowers_the_interpolated_cost() {
    // Populated-store pin (separate from the empty-store contract).
    // Training sequence this models: one first-seen selection of
    // DE after NI — seed 69 on the bigram and 69*7 = 483 on the
    // unigram (`docs/findings/user-store.md` §2.1).
    let mut model = model();
    let mut unigrams = BTreeMap::new();
    unigrams.insert(DE, 50);
    unigrams.insert(NI, 50);
    model.set_unigrams(unigrams, 100);
    let history = [PhraseToken::new(NI)];
    let token = PhraseToken::new(DE);

    let empty = model.score(&history, &token, 0).unwrap();
    let trained = UserCountDelta {
        bigram_count: 69,
        bigram_total: 69,
        unigram_delta: 483,
        unigram_total_delta: 483,
    };
    let populated = model
        .score_with_user_delta(&history, &token, 0, trained)
        .unwrap();
    assert!(
        populated < empty,
        "raising merged counts must cheapen the cost: empty={empty} populated={populated}"
    );

    // Empty-history ranking uses the merged unigram only.
    let empty_first = model.score(&[], &token, 0).unwrap();
    let populated_first = model
        .score_with_user_delta(&[], &token, 0, trained)
        .unwrap();
    assert!(
        populated_first < empty_first,
        "a user unigram delta must cheapen empty-history cost: \
         empty={empty_first} populated={populated_first}"
    );
}

#[test]
fn a_user_only_transition_does_not_floor() {
    // System has no row for this prev; the user gram is the merged gram.
    const NO_ENTRY_PREV: u32 = 0xFFFF_FFFF;
    let mut model = model();
    let mut unigrams = BTreeMap::new();
    unigrams.insert(DE, 100);
    model.set_unigrams(unigrams, 110);

    assert!(
        matches!(model.transition(NO_ENTRY_PREV, DE), Ok(None)),
        "precondition: the previous token must be absent from the system bigram"
    );

    let empty = model
        .score(&[PhraseToken::new(NO_ENTRY_PREV)], &PhraseToken::new(DE), 0)
        .unwrap();
    assert_eq!(empty, UNKNOWN_COST);

    let user_only = UserCountDelta {
        bigram_count: 69,
        bigram_total: 69,
        unigram_delta: 0,
        unigram_total_delta: 0,
    };
    let populated = model
        .score_with_user_delta(
            &[PhraseToken::new(NO_ENTRY_PREV)],
            &PhraseToken::new(DE),
            0,
            user_only,
        )
        .unwrap();
    assert!(
        populated < UNKNOWN_COST,
        "a user-only gram must interpolate, not floor: {populated}"
    );
}

#[test]
fn nbest_zero_delta_is_bit_identical_to_trait_impl() {
    // Every shape the trait method answers: an observed successor, a
    // count-0 non-successor in an existing row, a prev with no row, and
    // the no-unigram-table default.
    const NO_ENTRY_PREV: u32 = 0xFFFF_FFFF;
    let mut model = model();
    let mut unigrams = BTreeMap::new();
    unigrams.insert(DE, 50);
    unigrams.insert(NI, 50);
    unigrams.insert(0x0100_0001, 50);
    model.set_unigrams(unigrams, 100);

    for (prev, token) in [
        (NI, DE),
        (NI, 0x0100_0001),
        (NO_ENTRY_PREV, DE),
        (NI, 0x0200_0002),
    ] {
        let system = model
            .nbest_step_costs(&PhraseToken::new(prev), &PhraseToken::new(token))
            .unwrap();
        let merged = model
            .nbest_step_costs_with_user_delta(
                &PhraseToken::new(prev),
                &PhraseToken::new(token),
                UserCountDelta::ZERO,
            )
            .unwrap();
        assert_eq!(
            system, merged,
            "a zero overlay must be identity for {prev:08x} → {token:08x}"
        );
    }

    // No unigram table installed: both answers are the empty default.
    let bare = BigramLanguageModel::open(&fixtures_dir().join(crate::default_store_file("bigram")))
        .unwrap();
    let system = bare
        .nbest_step_costs(&PhraseToken::new(NI), &PhraseToken::new(DE))
        .unwrap();
    let merged = bare
        .nbest_step_costs_with_user_delta(
            &PhraseToken::new(NI),
            &PhraseToken::new(DE),
            UserCountDelta::ZERO,
        )
        .unwrap();
    assert_eq!(
        (system, merged),
        (Default::default(), Default::default()),
        "no unigram table answers the empty default on both paths"
    );
}

#[test]
fn nbest_augmented_delta_merges_into_both_branches() {
    // System carries the row (你 → 的); training adds the seed 69 on the
    // bigram and 483 on the unigram. The blended cost must be the blend
    // over the MERGED counts — numerator and denominator both.
    let mut model = model();
    let mut unigrams = BTreeMap::new();
    unigrams.insert(DE, 50);
    unigrams.insert(NI, 50);
    model.set_unigrams(unigrams, 100);
    let prev = PhraseToken::new(NI);
    let token = PhraseToken::new(DE);

    let (system_count, system_total) = model.transition(NI, DE).unwrap().expect("row exists");
    let trained = UserCountDelta {
        bigram_count: 69,
        bigram_total: 69,
        unigram_delta: 483,
        unigram_total_delta: 483,
    };

    let system = model.nbest_step_costs(&prev, &token).unwrap();
    let populated = model
        .nbest_step_costs_with_user_delta(&prev, &token, trained)
        .unwrap();

    let expected_blended = interpolate_ratio(
        model.lambda().numerator(),
        model.lambda().denominator(),
        u128::from(system_count + 69),
        u128::from(system_total) + 69,
        50 + 483,
        100 + 483,
    )
    .and_then(ratio_cost);
    assert_eq!(populated.blended, expected_blended);
    assert_ne!(
        populated.blended, system.blended,
        "an augmented pair must shift the blended cost"
    );
    assert!(
        populated.blended.unwrap() < system.blended.unwrap(),
        "raising merged counts must cheapen the blended step"
    );

    // The unigram branch merges too: (1 − λ) · (50+483)/(100+483).
    let expected_unigram = {
        let one_minus_lambda = model.lambda().denominator() - model.lambda().numerator();
        ratio_cost((
            one_minus_lambda * (50 + 483),
            model.lambda().denominator() * (100 + 483),
        ))
    };
    assert_eq!(populated.unigram, expected_unigram);
    assert_ne!(populated.unigram, system.unigram);
}

#[test]
fn nbest_user_only_pair_produces_a_blended_step() {
    // The prev has no system row; training creates the gram. Merged
    // BEFORE the presence gate, the blended branch appears where the
    // system-only answer is unigram-only — and its denominator is the
    // user total, not a system one.
    const NO_ENTRY_PREV: u32 = 0xFFFF_FFFF;
    let mut model = model();
    let mut unigrams = BTreeMap::new();
    unigrams.insert(DE, 100);
    model.set_unigrams(unigrams, 110);
    assert!(matches!(model.transition(NO_ENTRY_PREV, DE), Ok(None)));
    let prev = PhraseToken::new(NO_ENTRY_PREV);
    let token = PhraseToken::new(DE);

    let system = model.nbest_step_costs(&prev, &token).unwrap();
    assert!(
        system.blended.is_none(),
        "precondition: system-only answer has no blended step"
    );

    let user_only = UserCountDelta {
        bigram_count: 69,
        bigram_total: 69,
        unigram_delta: 0,
        unigram_total_delta: 0,
    };
    let populated = model
        .nbest_step_costs_with_user_delta(&prev, &token, user_only)
        .unwrap();
    let expected_blended = interpolate_ratio(
        model.lambda().numerator(),
        model.lambda().denominator(),
        69,
        69,
        100,
        110,
    )
    .and_then(ratio_cost);
    assert_eq!(
        populated.blended, expected_blended,
        "a user-only gram must blend over the user total (merged denominator)"
    );
    assert!(
        populated
            .step()
            .is_some_and(|step| step < populated.unigram.unwrap()),
        "the blended step must undercut the unigram-only branch"
    );
}

#[test]
fn nbest_user_successor_on_existing_row_blends_over_merged_total() {
    // 你 has a system row; 0x0100_0001 is a count-0 non-successor in it —
    // the 你→浩 shape. Merged before the gate, the blended denominator is
    // system_row_total + user_total, not the user total alone (that case
    // is `nbest_user_only_pair_produces_a_blended_step`).
    const NOVEL: u32 = 0x0100_0001;
    let mut model = model();
    let mut unigrams = BTreeMap::new();
    unigrams.insert(NOVEL, 50);
    unigrams.insert(NI, 50);
    model.set_unigrams(unigrams, 100);

    let (system_count, system_total) = model
        .transition(NI, NOVEL)
        .unwrap()
        .expect("你 has a system row");
    assert_eq!(
        system_count, 0,
        "precondition: successor is count-0 in 你's row"
    );
    assert!(system_total > 0);

    let prev = PhraseToken::new(NI);
    let token = PhraseToken::new(NOVEL);
    let system = model.nbest_step_costs(&prev, &token).unwrap();
    assert!(
        system.blended.is_none(),
        "precondition: system-only answer has no blended step"
    );

    let trained = UserCountDelta {
        bigram_count: 69,
        bigram_total: 69,
        unigram_delta: 0,
        unigram_total_delta: 0,
    };
    let populated = model
        .nbest_step_costs_with_user_delta(&prev, &token, trained)
        .unwrap();
    let expected_blended = interpolate_ratio(
        model.lambda().numerator(),
        model.lambda().denominator(),
        69,
        u128::from(system_total) + 69,
        50,
        100,
    )
    .and_then(ratio_cost);
    assert_eq!(
        populated.blended, expected_blended,
        "the blended denominator must include the system row total"
    );
    assert!(
        populated
            .step()
            .is_some_and(|step| step < populated.unigram.unwrap()),
        "the blended step must undercut the unigram-only branch"
    );
}
