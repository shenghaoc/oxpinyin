//! Shared structure reader and its lossless candidate projection.

use pinyin_oracle::OracleCandidateType;
use pinyin_oracle::candidate_fixture::CandidateFixture;

const HEADER: &str = "# pinyin-oracle-candidate-structure-v1\n# pin_ref=test-pin\n";

#[test]
fn projection_preserves_input_bytes_order_duplicates_and_pin() {
    let raw = format!(
        "{HEADER} a'\t1\t甲\tNORMAL_CANDIDATE\t-\n a'\t2\t甲\tNBEST_MATCH_CANDIDATE\t0\na\t1\t乙\tNORMAL_CANDIDATE\t-\n"
    );
    let fixture = CandidateFixture::parse(&raw).expect("valid structure");
    assert_eq!(fixture.pin_ref, "test-pin");
    assert_eq!(
        fixture.candidates().collect::<Vec<_>>(),
        vec![(" a'", 1, "甲"), (" a'", 2, "甲"), ("a", 1, "乙")]
    );
    assert_eq!(
        fixture.rows[1].candidate_type,
        OracleCandidateType::NbestMatch
    );
    assert_eq!(fixture.rows[1].nbest_index, Some(0));
    assert_eq!(fixture.candidate_lists()[" a'"], ["甲", "甲"]);
}

#[test]
fn malformed_structure_is_rejected() {
    for row in [
        "a\t1\t甲\tNORMAL_CANDIDATE",
        "a\t1\t甲\tNORMAL_CANDIDATE\t-\textra",
        "a\tbad\t甲\tNORMAL_CANDIDATE\t-",
        "a\t0\t甲\tNORMAL_CANDIDATE\t-",
        "a\t1\t甲\tUNKNOWN\t-",
        "a\t1\t甲\tNBEST_MATCH_CANDIDATE\t256",
        "a\t1\t甲\tNBEST_MATCH_CANDIDATE\t-",
        "a\t1\t甲\tNORMAL_CANDIDATE\t0",
    ] {
        assert!(
            CandidateFixture::parse(&format!("{HEADER}{row}\n")).is_err(),
            "accepted {row:?}"
        );
    }
    assert!(CandidateFixture::parse("a\t1\t甲\tNORMAL_CANDIDATE\t-\n").is_err());
    assert!(CandidateFixture::parse("# pinyin-oracle-candidate-structure-v1\n").is_err());
    assert!(CandidateFixture::parse("# wrong-schema\n# pin_ref=test-pin\n").is_err());
}
