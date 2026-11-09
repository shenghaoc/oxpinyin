//! The format of the committed candidate structure capture.

use std::path::PathBuf;

#[path = "common/candidate_fixture.rs"]
mod candidate_fixture;

use candidate_fixture::{CandidateFixture, OracleCandidateType, STRUCTURE_FIXTURE};

/// The pin the capture was taken from.
const EXPECTED_PIN_REF: &str = concat!(
    "libpinyin-2.11.92-074a2219c90feaf962d0d24f034514033ece5f99",
    "+model20-59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155",
    "+dbm-tkrzw"
);

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn structure_fixture_has_valid_format_and_candidate_types() {
    let raw = std::fs::read_to_string(repo_root().join(STRUCTURE_FIXTURE))
        .expect("committed structure fixture");
    let fixture = CandidateFixture::parse(&raw).expect("well-formed structure fixture");
    assert_eq!(fixture.pin_ref, EXPECTED_PIN_REF);
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
