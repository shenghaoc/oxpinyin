//! The measured sentence-surface gate recorded in
//! `docs/findings/sentence-surface.md` §12. Passing this gate does not classify
//! the remaining rows or freeze unattributed defects permanently. Only the
//! six named shared-comparison rows have the approved math classification.
//!
//! Uses the same `pinyin_oracle::sentence_tail::measure` as `sentence-tail`.
//! This ignored real-table test fails if its required inputs are absent.
//! Provision `PINYIN_EXPORT_DIR` with exported tables and `PINYIN_MODEL_DIR`
//! with the complete extracted model20 directory (all 18 files), then run
//! with `--include-ignored`.
//!
//! Approved repairs update these measured numbers and §12 in the same PR.
//! A pin move also requires regenerating the oracle fixture with
//! `cargo run -p pinyin-oracle --features oracle-ffi --bin
//! oracle_sentence_surface` and re-measuring.

use pinyin_oracle::sentence_tail;

/// The §12 measured residual, over the 500 comparable inputs of the frozen
/// W2 sample. Re-measure and obtain approval before updating these numbers
/// and §12 together; do not silently change the expected surface.
#[test]
#[ignore = "needs the system-table export and the model20 cache (PINYIN_EXPORT_DIR, PINYIN_MODEL_DIR); run with --include-ignored"]
fn sentence_surface_matches_the_declared_residual() {
    let mut session = match sentence_tail::open_session_from_env() {
        Ok(Some(session)) => session,
        Ok(None) => {
            panic!(
                "exported tables or model cache absent sentence-surface parity \
                 (set PINYIN_EXPORT_DIR + PINYIN_MODEL_DIR to run it)"
            )
        }
        Err(error) => panic!("cannot open port session: {error}"),
    };

    let report = sentence_tail::measure(&mut session, &sentence_tail::repo_root())
        .expect("the sentence-surface measurement runs");

    assert_eq!(report.comparable, 500, "comparable-input count drifted");
    assert_eq!(
        report.guessed_disagree, 0,
        "guess_sentence retval must agree on every comparable input"
    );

    // The three strictnesses of §12. 1-best 499, distinct-set 499, ordered 499.
    assert_eq!(report.row0_match, 499, "1-best agreement moved (§12: 499)");
    assert_eq!(
        report.distinct_set_match(),
        499,
        "n-best distinct-set agreement moved (§12: 499)"
    );
    assert_eq!(
        report.list_ordered_match, 499,
        "n-best ordered-list agreement moved (§12: 499)"
    );
    assert_eq!(
        report.rows_match, 499,
        "first-6 candidate-row agreement moved (§12: 499, coincides with ordered)"
    );

    // Measured invariant: no residual list is merely reordered.
    // This observation does not attribute its cause.
    assert_eq!(
        report.list_order_only, 0,
        "an order-only sentence divergence appeared; re-measure §12"
    );

    // The 499 − 499 = 0 duplicate-path ranks (the distinct-same rows).
    assert_eq!(
        report.list_distinct_extra, 0,
        "the distinct-set minus ordered gap moved from 0 (§12)"
    );
}

/// G-m1 adds one native cost unit at every trellis step. Surface parity
/// alone can miss it after selection is ported; assert the native cost at
/// each of #574's four recovered corpus inputs as well as their fixture rows.
#[test]
#[ignore = "needs the system-table export and the model20 cache (PINYIN_EXPORT_DIR, PINYIN_MODEL_DIR); run with --include-ignored"]
fn sentence_surface_detects_per_step_cost_shift() {
    let mut session = sentence_tail::open_session_from_env()
        .expect("system-table export and model20 must open")
        .expect("system-table export and model20 must be present");
    for (input, expected) in [
        ("yaomeichong", 32746),
        ("xiehenshuaitong", 46792),
        ("nuanmanqianzhaofang", 59752),
        ("nic", 17763),
    ] {
        session.reset();
        session.type_pinyin(input).expect("input must parse");
        assert!(session.guess_sentence().expect("sentence guess must run"));
        let first = session
            .candidates()
            .iter()
            .find(|candidate| candidate.kind() == oxpinyin_engine::CandidateKind::Sentence)
            .expect("input must have a sentence candidate");
        assert_eq!(
            first.cost(),
            expected,
            "native trellis cost moved for {input}"
        );
    }
}
