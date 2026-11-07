//! The measured sentence-surface gate recorded in
//! `docs/findings/sentence-surface.md` §12, over the pinned libpinyin-data
//! package (`just fetch-contract-data`), in tkrzw format. Passing this gate does
//! not classify the remaining rows or freeze unattributed defects permanently.
//! Only the current exact selection residuals have the registered S1/S2/S3
//! disposition.
//!
//! `sentence_fixtures.rs` checks the fixtures themselves on every backend.
//!
//! Approved repairs update these measured numbers and §12 in the same PR.
//! A pin move also requires regenerating the oracle fixture and re-measuring
//! (`docs/runbooks/goldens-and-pins.md`).

#[path = "common/contract_data.rs"]
mod contract_data;
#[path = "common/sentence_fixture.rs"]
mod sentence_fixture;
#[path = "common/sentence_tail.rs"]
mod sentence_tail;

use sentence_tail::PortSession;

/// The port over the pinned package.
fn open_session() -> PortSession {
    PortSession::open(&contract_data::contract_data_dir()).expect("the package opens")
}

/// The §12 measured residual, over the 500 comparable inputs of the frozen
/// W2 sample. Re-measure and obtain approval before updating these numbers
/// and §12 together; do not silently change the expected surface.
#[test]
fn sentence_surface_matches_the_declared_residual() {
    let mut session = open_session();

    let report = sentence_tail::measure(&mut session, &sentence_fixture::repo_root())
        .expect("the sentence-surface measurement runs");
    eprintln!("sentence-surface measurement: {report:#?}");

    assert!(
        report.unexpected_surfaces.is_empty(),
        "sentence surfaces moved from their exact fixture bindings: {:?}",
        report.unexpected_surfaces
    );
    assert_eq!(report.declared_residuals, 9, "residual membership moved");
    assert_eq!(report.comparable, 500, "comparable-input count drifted");
    assert_eq!(
        report.guessed_disagree, 0,
        "guess_sentence retval must agree on every comparable input"
    );

    // Re-measured after #1378: 1-best 496, distinct-set 492, ordered 491.
    assert_eq!(report.row0_match, 496, "1-best agreement moved (§12: 496)");
    assert_eq!(
        report.distinct_set_match(),
        492,
        "n-best distinct-set agreement moved (§12: 492)"
    );
    assert_eq!(
        report.list_ordered_match, 491,
        "n-best ordered-list agreement moved (§12: 491)"
    );
    assert_eq!(
        report.rows_match, 491,
        "first-6 candidate-row agreement moved (§12: 491, coincides with ordered)"
    );

    // Measured invariant: no residual list is merely reordered.
    // This observation does not attribute its cause.
    assert_eq!(
        report.list_order_only, 0,
        "an order-only sentence divergence appeared; re-measure §12"
    );

    // The 492 − 491 = 1 duplicate-path rank (the distinct-same row).
    assert_eq!(
        report.list_distinct_extra, 1,
        "the distinct-set minus ordered gap moved from 1 (§12)"
    );
}

/// A per-step score mutation can leave the visible surface unchanged.
/// Keep exact f32 accumulation checks for #574's recovered corpus inputs:
/// the `gfloat m_poss` of the pin's `PhoneticLookup` (libpinyin 074a2219
/// `src/lookup/phonetic_lookup.h`).
#[test]
fn sentence_surface_detects_per_step_score_shift() {
    let mut owner = open_session();
    let mut session = owner.session();
    let actual: Vec<_> = [
        "yaomeichong",
        "xiehenshuaitong",
        "nuanmanqianzhaofang",
        "nic",
    ]
    .into_iter()
    .map(|input| {
        session.reset();
        session.type_pinyin(input).expect("input must parse");
        assert!(session.guess_sentence().expect("sentence guess must run"));
        (
            input,
            session
                .sentence_log_possibility(0)
                .expect("real-table score")
                .to_bits(),
        )
    })
    .collect();
    assert_eq!(
        actual,
        [
            ("yaomeichong", 0xc1b595f0),
            ("xiehenshuaitong", 0xc201bc36),
            ("nuanmanqianzhaofang", 0xc225aaba),
            ("nic", 0xc144ff96)
        ],
        "f32 accumulation moved"
    );
}

/// Supplementary #747 coverage, reported separately from the frozen 500.
/// Every ordinary input matches the pin row for row. The current §12
/// selection residuals match their exact recorded subject surfaces; changes
/// within those cases fail too. No new exception membership lives here.
#[test]
fn sentence_surface_matches_supplementary_tie_order() {
    let cases = sentence_fixture::load_tie_cases(&sentence_fixture::repo_root());
    let mut owner = open_session();
    let mut session = owner.session();
    session
        .set_options(oxpinyin_core::OptionBits::from_bits(0x18a))
        .expect("capture options must apply");
    session
        .set_sort_options(0x1e)
        .expect("capture sort must apply");
    let (mut pin_equal, mut residuals) = (0, 0);
    let mut moved = Vec::new();
    for (ordinal, case) in cases.iter().enumerate() {
        session.reset();
        // The Hanyu C-ABI parse_full_more seam uses replace_raw, preserving
        // literal junk and physical separators exactly as the corpus walk.
        session.replace_raw(&case.input).expect("input must parse");
        assert_eq!(
            session.guess_sentence().expect("guess must run"),
            case.guessed
        );
        let actual = sentence_fixture::TieSurface {
            indexed: (0..3)
                .filter_map(|rank| {
                    session
                        .sentence_text(rank)
                        .and_then(|text| text.as_str())
                        .map(str::to_owned)
                })
                .collect(),
            rows: session
                .candidates()
                .iter()
                .filter(|candidate| candidate.kind() == oxpinyin_engine::CandidateKind::Sentence)
                .map(|candidate| {
                    (
                        candidate.nbest_row().expect("NBEST rank"),
                        candidate.text().as_str().unwrap_or_default().to_owned(),
                    )
                })
                .collect(),
        };
        let expected = if let Some(residual) = &case.residual {
            residuals += 1;
            residual
        } else {
            pin_equal += 1;
            &case.pin
        };
        if &actual != expected {
            eprintln!(
                "tie surface moved for {:?}: actual={actual:?}; expected={expected:?}",
                case.input
            );
            moved.push(case.input.clone());
        }
        if (ordinal + 1) % 1000 == 0 {
            eprintln!("supplementary tie progress: {} inputs checked", ordinal + 1);
        }
    }
    eprintln!(
        "supplementary tie order: pin-equal={pin_equal}, exact residuals={residuals}, total={}",
        cases.len()
    );
    assert!(
        moved.is_empty(),
        "supplementary tie surfaces moved: {moved:?}"
    );
    assert_eq!(residuals, 156, "residual inputs checked count moved");
    assert_eq!(pin_equal, 8436, "pin-equal inputs checked count moved");
}
