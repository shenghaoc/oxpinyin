//! oxpinyin's candidate lists against the frozen pin capture, over every distinct
//! input of the W2 parity corpus.
//!
//! `fixtures/w4/oracle-candidate-structure.txt` holds the pin's first ten candidates
//! for each input. The system data is the pinned libpinyin-data package, which
//! `just fetch-contract-data` extracts and which is in tkrzw format, so this target
//! requires that backend. A missing package is a failure, not a skip.

use std::collections::BTreeSet;
use std::path::PathBuf;

use oxpinyin_data::{BigramLanguageModel, SystemDbm, SystemDictionary};
use oxpinyin_engine::{Candidate, CandidateKind, Session, SessionSettings};

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

/// Every distinct input of the committed corpus, in bytewise order.
fn corpus_inputs() -> BTreeSet<String> {
    let mut inputs = BTreeSet::new();
    let dir = repo_root().join("tests/parity/corpus/inputs");
    for entry in std::fs::read_dir(dir).expect("the corpus is committed") {
        let bytes = std::fs::read(entry.expect("corpus entry").path()).expect("stratum");
        for line in bytes.split_inclusive(|byte| *byte == b'\n') {
            let line = line.strip_suffix(b"\n").unwrap_or(line);
            inputs.insert(String::from_utf8_lossy(line).into_owned());
        }
    }
    inputs
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
fn candidates_match_the_frozen_capture_over_the_corpus() {
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
    let dictionary = SystemDictionary::open(&dir).expect("the system dictionary opens");
    let mut model = BigramLanguageModel::open(
        &dir.join(SystemDbm::Bigram.file_name()),
        dictionary.libraries(),
    )
    .expect("the bigram opens");
    model.set_lambda_from_table_conf(&dir.join("table.conf"));
    let mut session =
        Session::new(SessionSettings::default(), &dictionary, model).expect("the session opens");

    let inputs = corpus_inputs();
    let mut rows = 0_usize;
    let mut compared = 0_usize;
    let mut moved = Vec::new();
    for input in &inputs {
        // The capture records nothing for an input the pin parses none of: its
        // protocol stops before asking for candidates, so no rows says nothing here.
        let Some(theirs) = expected.get(input.as_str()) else {
            continue;
        };
        session.reset();
        let _ = session.type_pinyin(input);
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
        "candidate surface: {compared} of {} corpus inputs compared, {rows} rows, {} moved",
        inputs.len(),
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
    assert_eq!(inputs.len(), 10_312, "distinct corpus inputs");
    assert_eq!(compared, 10_037, "inputs with captured candidates");
    assert_eq!(
        compared,
        expected.len(),
        "captured inputs outside the corpus"
    );
    assert_eq!(rows, 97_442, "captured rows");
}
