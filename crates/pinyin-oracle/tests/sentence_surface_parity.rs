//! The gate for the sentence-surface residual `docs/findings/sentence-
//! surface.md` §12 records: the port's `guess_sentence` surface against the
//! pinned oracle fixture, at the three named strictnesses. The residual is
//! FROZEN as a permanent Stage-1 divergence (maintainer ruling 2026-09-02),
//! so this gate asserts a defined residual, not a parity target: it holds the
//! frozen numbers and the mechanism invariants, and any move is a deliberate
//! re-freeze of §12.
//!
//! Runs the same `pinyin_oracle::sentence_tail::measure` the `sentence-tail`
//! binary prints, so the asserted numbers and the reported numbers are one
//! implementation. **Self-skips** when the exported model20 tables or the
//! model cache are absent — the same self-skip the rest of the real-tables
//! tier uses — so `cargo test --workspace` stays green on a runner without
//! them; it asserts on any runner (or maintainer) that has them.
//!
//! Provisioning: `PINYIN_EXPORT_DIR` → the exported redb tables,
//! `PINYIN_MODEL_DIR` → a **complete** extracted model20 directory (all 18
//! files; the partial 4-file `~/.cache/oxpinyin-data` is rejected).
//!
//! When the pin moves, regenerate the fixture
//! (`cargo run -p pinyin-oracle --features oracle-ffi --bin
//! oracle-sentence-surface`) and re-measure — a change here is a deliberate
//! re-freeze of the §12 residual, not a silent drift.

use pinyin_oracle::sentence_tail;

/// The §12 measured residual, over the 496 comparable inputs of the frozen
/// W2 sample. A change to any of these is a pin move: re-measure and update
/// §12 (a re-freeze the maintainer signs off), do not just edit the number.
#[test]
fn sentence_surface_matches_the_declared_residual() {
    let mut session = match sentence_tail::open_session_from_env() {
        Ok(Some(session)) => session,
        Ok(None) => {
            eprintln!(
                "exported tables or model cache absent; skipping sentence-surface parity \
                 (set PINYIN_EXPORT_DIR + PINYIN_MODEL_DIR to run it)"
            );
            return;
        }
        Err(error) => panic!("cannot open port session: {error}"),
    };

    let report = sentence_tail::measure(&mut session, &sentence_tail::repo_root())
        .expect("the sentence-surface measurement runs");

    assert_eq!(report.comparable, 496, "comparable-input count drifted");
    assert_eq!(
        report.guessed_disagree, 0,
        "guess_sentence retval must agree on every comparable input"
    );

    // The three strictnesses of §12. 1-best 491, distinct-set 396, ordered 390.
    assert_eq!(report.row0_match, 491, "1-best agreement moved (§12: 491)");
    assert_eq!(
        report.distinct_set_match(),
        396,
        "n-best distinct-set agreement moved (§12: 396)"
    );
    assert_eq!(
        report.list_ordered_match, 390,
        "n-best ordered-list agreement moved (§12: 390)"
    );
    assert_eq!(
        report.rows_match, 390,
        "first-6 candidate-row agreement moved (§12: 390, coincides with ordered)"
    );

    // The mechanism invariant: the residual is hypothesis selection, not
    // display order. No case is one list merely reordered.
    assert_eq!(
        report.list_order_only, 0,
        "an order-only sentence divergence appeared — the residual is no longer \
         pure hypothesis selection; revisit §12's mechanism claim"
    );

    // The 396 − 390 = 6 duplicate-path ranks (the distinct-same rows).
    assert_eq!(
        report.list_distinct_extra, 6,
        "the distinct-set minus ordered gap moved from 6 (§12)"
    );
}
