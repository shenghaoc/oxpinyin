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

    // The three strictnesses of §12. 1-best 500, distinct-set 500, ordered 500.
    assert_eq!(report.row0_match, 500, "1-best agreement moved (§12: 500)");
    assert_eq!(
        report.distinct_set_match(),
        500,
        "n-best distinct-set agreement moved (§12: 500)"
    );
    assert_eq!(
        report.list_ordered_match, 500,
        "n-best ordered-list agreement moved (§12: 500)"
    );
    assert_eq!(
        report.rows_match, 500,
        "first-6 candidate-row agreement moved (§12: 500, coincides with ordered)"
    );

    // Measured invariant: no residual list is merely reordered.
    // This observation does not attribute its cause.
    assert_eq!(
        report.list_order_only, 0,
        "an order-only sentence divergence appeared; re-measure §12"
    );

    // The 500 − 500 = 0 duplicate-path ranks (the distinct-same rows).
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

// The frozen gate's module documentation refers to the six named §12
// shared-comparison residuals. That existing table is the membership source;
// this supplementary gate must not introduce another list of names.
fn declared_sentence_residuals(root: &std::path::Path) -> std::collections::BTreeSet<String> {
    let doc = std::fs::read_to_string(root.join("docs/findings/sentence-surface.md"))
        .expect("the frozen sentence-surface declaration must be present");
    let section = doc
        .split_once("\n## 12. ")
        .expect("the frozen §12 must be present")
        .1
        .split("\n## 13.")
        .next()
        .expect("§12 must have a body");
    let declaration = section
        .split_once("Only the following six shared-comparison rows are class (a), as ruled.")
        .expect("the frozen shared-comparison residual definition must be present")
        .1;
    let table = declaration
        .split_once("| Input | Pin deciding f32 bits | Subject native costs |\n")
        .expect("the declared residual table must be present")
        .1;
    let mut lines = table.lines();
    assert!(lines.next().expect("table separator").starts_with("|---"));
    let mut names = std::collections::BTreeSet::new();
    for line in lines.take_while(|line| line.starts_with('|')) {
        let columns: Vec<_> = line.trim_matches('|').split('|').collect();
        assert_eq!(columns.len(), 3, "residual declaration format changed");
        let name = columns[0].trim().trim_matches('`');
        assert!(!name.is_empty() && names.insert(name.to_owned()));
    }
    assert_eq!(names.len(), 6, "declared residual membership count moved");
    names
}

#[derive(Debug, PartialEq, Eq)]
struct TieSurface {
    indexed: Vec<String>,
    // Every row has NBEST type 1; keep its original tail index and text.
    rows: Vec<(u8, String)>,
}

struct TieCase {
    input: String,
    occurrences: usize,
    guessed: bool,
    pin: TieSurface,
    residual: Option<TieSurface>,
}

fn decode_tie_surface(pool: &[&str], indexed: &str, rows: &str) -> TieSurface {
    let text = |id: &str| {
        pool.get(id.parse::<usize>().expect("numeric pool ID"))
            .expect("pool ID must be in range")
            .to_string()
    };
    let indexed: Vec<_> = if indexed.is_empty() {
        Vec::new()
    } else {
        indexed.split(',').map(text).collect()
    };
    assert!(indexed.len() <= 3, "pinyin n-best cap moved");
    let rows: Vec<_> = if rows.is_empty() {
        Vec::new()
    } else {
        rows.split(',')
            .map(|row| {
                let (rank, id) = row.split_once(':').expect("rank:pool-ID row");
                (rank.parse::<u8>().expect("numeric tail rank"), text(id))
            })
            .collect()
    };
    let mut seen = std::collections::BTreeSet::new();
    let complete: Vec<_> = indexed
        .iter()
        .enumerate()
        .filter(|(_, value)| seen.insert((*value).clone()))
        .map(|(rank, value)| (u8::try_from(rank).expect("n-best rank fits"), value.clone()))
        .collect();
    assert_eq!(
        rows, complete,
        "fixture must retain the complete NBEST surface"
    );
    TieSurface { indexed, rows }
}

fn load_tie_cases(root: &std::path::Path) -> Vec<TieCase> {
    let raw = std::fs::read_to_string(root.join("fixtures/w4/oracle-sentence-tie-order.txt"))
        .expect("the supplementary tie fixture must be present");
    assert!(
        raw.lines()
            .any(|line| line == "# format=sentence-tie-order-v1")
    );
    let declared = declared_sentence_residuals(root);
    let mut names = std::collections::BTreeSet::new();
    let mut recorded_residuals = std::collections::BTreeSet::new();
    let mut cases = Vec::new();
    for line in raw.lines().filter(|line| !line.starts_with('#')) {
        let columns: Vec<_> = line.split('\t').collect();
        assert_eq!(columns.len(), 9, "compact fixture column count moved");
        let input = columns[0].to_owned();
        assert!(names.insert(input.clone()), "duplicate fixture input");
        let occurrences = columns[1].parse().expect("numeric corpus occurrence count");
        assert!(occurrences > 0);
        let costs: Vec<_> = columns[2]
            .split(',')
            .map(|cost| cost.parse::<i64>().expect("numeric native selection cost"))
            .collect();
        assert!((2..=3).contains(&costs.len()));
        let guessed = match columns[3] {
            "0" => false,
            "1" => true,
            _ => panic!("fixture guess status must be 0 or 1"),
        };
        let pool: Vec<_> = columns[4].split('\u{1}').collect();
        let pin = decode_tie_surface(&pool, columns[5], columns[6]);
        let residual = if declared.contains(&input) {
            assert!(!columns[7].is_empty() && !columns[8].is_empty());
            assert!(recorded_residuals.insert(input.clone()));
            let surface = decode_tie_surface(&pool, columns[7], columns[8]);
            assert_ne!(
                surface, pin,
                "a declared residual silently became pin-equal"
            );
            Some(surface)
        } else {
            assert!(
                columns[7].is_empty() && columns[8].is_empty(),
                "an undeclared subject expectation was added for {input}"
            );
            None
        };
        cases.push(TieCase {
            input,
            occurrences,
            guessed,
            pin,
            residual,
        });
    }
    assert_eq!(
        recorded_residuals, declared,
        "declared inputs must all be checked"
    );
    assert_eq!(cases.len(), 8592, "selected distinct-input count moved");
    assert_eq!(
        cases.iter().map(|case| case.occurrences).sum::<usize>(),
        8744
    );
    assert_eq!(
        cases.iter().map(|case| case.pin.rows.len()).sum::<usize>(),
        22038
    );
    cases
}

/// Check fixture completeness and bind its subject expectations to the
/// existing frozen declaration even when CI has no real tables.
#[test]
fn sentence_tie_fixture_uses_only_the_declared_residuals() {
    let _ = load_tie_cases(&sentence_tail::repo_root());
}

/// Supplementary #747 coverage, reported separately from the frozen 500.
/// Every ordinary input matches the pin row for row. The six existing §12
/// residuals instead match their exact recorded subject surfaces; changes
/// within those cases fail too. No new exception membership lives here.
#[test]
#[ignore = "needs the matching system-table export (PINYIN_EXPORT_DIR); run with --include-ignored"]
fn sentence_surface_matches_supplementary_tie_order() {
    let cases = load_tie_cases(&sentence_tail::repo_root());
    let mut session = sentence_tail::open_session_from_env()
        .expect("system-table export must open")
        .expect("system-table export must be present");
    session
        .set_options(oxpinyin_core::OptionBits::from_bits(0x18a))
        .expect("capture options must apply");
    session
        .set_sort_options(0x1e)
        .expect("capture sort must apply");
    let (mut pin_equal, mut residuals) = (0, 0);
    for (ordinal, case) in cases.iter().enumerate() {
        session.reset();
        // The Hanyu C-ABI parse_full_more seam uses replace_raw, preserving
        // literal junk and physical separators exactly as the corpus walk.
        session.replace_raw(&case.input).expect("input must parse");
        assert_eq!(
            session.guess_sentence().expect("guess must run"),
            case.guessed
        );
        let actual = TieSurface {
            indexed: (0..3)
                .filter_map(|rank| session.sentence_text(rank).map(str::to_owned))
                .collect(),
            rows: session
                .candidates()
                .iter()
                .filter(|candidate| candidate.kind() == oxpinyin_engine::CandidateKind::Sentence)
                .map(|candidate| {
                    (
                        candidate.nbest_row().expect("NBEST rank"),
                        candidate.text().to_owned(),
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
        assert_eq!(
            &actual, expected,
            "supplementary tie surface moved for {:?}",
            case.input
        );
        if (ordinal + 1) % 1000 == 0 {
            eprintln!("supplementary tie progress: {} inputs checked", ordinal + 1);
        }
    }
    eprintln!(
        "supplementary tie order: pin-equal={pin_equal}, exact residuals={residuals}, total={}",
        cases.len()
    );
    assert_eq!(residuals, 6, "residual inputs checked count moved");
    assert_eq!(pin_equal, 8586, "pin-equal inputs checked count moved");
}
