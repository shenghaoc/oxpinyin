//! oxpinyin's candidate lists against the frozen pin capture, over every input
//! the capture holds.
//!
//! `fixtures/w4/oracle-candidate-structure.txt` holds the pin's first ten candidates
//! for each retained input. The system data is the pinned libpinyin-data package,
//! which `just fetch-contract-data` extracts and which is in tkrzw format, so this
//! target requires that backend. A missing package is a failure, not a skip.

use std::path::PathBuf;

use oxpinyin_engine::{Candidate, CandidateKind, Session, SessionSettings};
use oxpinyin_runtime::Runtime;

#[path = "common/candidate_fixture.rs"]
mod candidate_fixture;
#[path = "common/contract_data.rs"]
mod contract_data;

use candidate_fixture::{CandidateFixture, OracleCandidateType, STRUCTURE_FIXTURE};

/// How many candidates the capture records per input.
const CAPTURE_DEPTH: usize = 10;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn candidate_type(candidate: &Candidate) -> Option<OracleCandidateType> {
    match candidate.kind() {
        CandidateKind::Phrase => Some(OracleCandidateType::Normal),
        CandidateKind::Addon => Some(OracleCandidateType::Addon),
        CandidateKind::Sentence => Some(OracleCandidateType::NbestMatch),
        CandidateKind::Fallback => None,
    }
}

#[test]
fn candidates_match_the_frozen_capture() {
    let fixture = CandidateFixture::read(repo_root().join(STRUCTURE_FIXTURE))
        .expect("the structure capture is committed");
    let mut expected = std::collections::BTreeMap::<&str, Vec<_>>::new();
    for row in &fixture.rows {
        expected.entry(row.input.as_str()).or_default().push((
            row.text.as_str(),
            Some(row.candidate_type),
            row.nbest_index,
        ));
    }

    let dir = contract_data::contract_data_dir();
    let runtime = Runtime::open(&dir, None).expect("the pinned data directory opens");
    let mut session = Session::new(SessionSettings::default(), runtime.dict(), runtime.lm());

    let mut rows = 0_usize;
    let mut compared = 0_usize;
    let mut moved = Vec::new();
    for (input, theirs) in &expected {
        session.reset();
        let _ = session.replace_raw(input);
        let ours: Vec<_> = session
            .candidates()
            .iter()
            .take(CAPTURE_DEPTH)
            .map(|candidate| {
                (
                    candidate.text().as_str().unwrap_or_default().to_owned(),
                    candidate_type(candidate),
                    candidate.nbest_row(),
                )
            })
            .collect();
        rows += theirs.len();
        compared += 1;
        let same = ours.len() == theirs.len()
            && ours
                .iter()
                .zip(theirs)
                .all(|(ours, theirs)| (ours.0.as_str(), ours.1, ours.2) == *theirs);
        if !same {
            moved.push(format!("{input:?}: ours {ours:?}, pin {theirs:?}"));
        }
    }

    eprintln!(
        "candidate surface: {compared} captured inputs compared, {rows} rows, {} moved",
        moved.len()
    );
    for line in moved.iter().take(20) {
        eprintln!("{line}");
    }
    assert!(
        moved.is_empty(),
        "{} inputs moved from the capture",
        moved.len()
    );
    assert_eq!(compared, expected.len(), "every captured input compared");
    assert_eq!(compared, fixture.total_inputs, "header total_inputs");
    assert_eq!(rows, fixture.total_triples, "header total_triples");
    assert_eq!(compared, 750, "retained captured inputs");
    assert_eq!(rows, 4_572, "retained candidate rows");
}
