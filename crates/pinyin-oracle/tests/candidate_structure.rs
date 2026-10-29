//! W2-CAND: format, candidate types and live freshness of the single W4 capture.

use pinyin_oracle::OracleCandidateType;
use pinyin_oracle::candidate_fixture::{CandidateFixture, STRUCTURE_FIXTURE};
use std::path::PathBuf;

#[cfg(feature = "oracle-ffi")]
const CAPTURE_DEPTH: usize = 10;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn structure_fixture_has_valid_format_and_candidate_types() {
    let raw = std::fs::read_to_string(repo_root().join(STRUCTURE_FIXTURE))
        .expect("committed structure fixture");
    let fixture = CandidateFixture::parse(&raw).expect("well-formed structure fixture");
    assert_eq!(fixture.pin_ref, pinyin_oracle::EXPECTED_PIN_REF);
    let header_triples: usize = raw
        .lines()
        .find_map(|line| line.strip_prefix("# total_triples="))
        .and_then(|rest| rest.split_whitespace().next())
        .expect("total_triples header")
        .parse()
        .expect("numeric total_triples");
    assert_eq!(fixture.rows.len(), header_triples);
    let mut previous_input = None;
    let mut expected_rank = 1;
    for row in &fixture.rows {
        if previous_input != Some(row.input.as_str()) {
            assert!(
                previous_input.is_none_or(|previous| previous < row.input.as_str()),
                "inputs sorted bytewise"
            );
            expected_rank = 1;
        }
        assert_eq!(
            row.rank, expected_rank,
            "consecutive ranks for {:?}",
            row.input
        );
        assert!(row.rank <= 10, "capture depth is ten");
        // The 0x1e guess-candidates path excludes LONGER and cannot emit predictions.
        assert_ne!(row.candidate_type, OracleCandidateType::Longer);
        assert!(!row.candidate_type.as_wire().starts_with("PREDICTED_"));
        assert_eq!(
            row.nbest_index.is_some(),
            row.candidate_type == OracleCandidateType::NbestMatch
        );
        previous_input = Some(row.input.as_str());
        expected_rank += 1;
    }
}

/// Live-oracle freshness check for the structure fixture.
///
/// ```text
/// cargo test -p pinyin-oracle --features oracle-ffi \
///     --test candidate_structure structure_fixture_is_fresh -- --ignored --nocapture
/// ```
#[cfg(feature = "oracle-ffi")]
#[test]
#[ignore = "full-corpus live oracle run; regenerate with bin oracle-candidate-structure"]
fn structure_fixture_is_fresh() {
    use std::collections::BTreeMap;

    use pinyin_oracle::corpus;
    use pinyin_oracle::{Oracle, OracleFlags, OraclePrefix};

    let structure =
        CandidateFixture::read(repo_root().join(STRUCTURE_FIXTURE)).expect("structure fixture");
    assert_eq!(
        structure.pin_ref,
        pinyin_oracle::EXPECTED_PIN_REF,
        "structure fixture pin_ref does not match the frozen pin"
    );

    // Committed payload, verbatim, in file order.
    let committed: Vec<String> = structure
        .rows
        .iter()
        .map(|row| {
            let nbest = row
                .nbest_index
                .map_or_else(|| "-".to_owned(), |index| index.to_string());
            format!(
                "{}\t{}\t{}\t{}\t{nbest}",
                row.input,
                row.rank,
                row.text,
                row.candidate_type.as_wire()
            )
        })
        .collect();

    let prefix = OraclePrefix::locate().expect("oracle prefix");
    let mut oracle = Oracle::open_with_temp_user_dir(prefix).expect("oracle opens");
    let mut session = oracle
        .session(OracleFlags::DEFAULT)
        .expect("oracle session");

    // Distinct corpus inputs, sorted bytewise, exactly as the capture dedups.
    let mut distinct: BTreeMap<String, ()> = BTreeMap::new();
    for stratum in corpus::generate() {
        for input in stratum.inputs {
            distinct.entry(input).or_insert(());
        }
    }

    let mut regenerated: Vec<String> = Vec::new();
    for input in distinct.keys() {
        let infos = session
            .observe_candidate_infos(input.as_bytes())
            .unwrap_or_else(|e| panic!("observe {input:?}: {e}"));
        for (idx, info) in infos.iter().take(CAPTURE_DEPTH).enumerate() {
            let nbest = info
                .nbest_index
                .map_or_else(|| "-".to_owned(), |index| index.to_string());
            regenerated.push(format!(
                "{input}\t{}\t{}\t{}\t{nbest}",
                idx + 1,
                info.text,
                info.candidate_type.as_wire()
            ));
        }
    }

    assert_eq!(
        committed,
        regenerated.iter().map(String::as_str).collect::<Vec<_>>(),
        "structure fixture payload differs from a fresh capture; regenerate it"
    );
}
