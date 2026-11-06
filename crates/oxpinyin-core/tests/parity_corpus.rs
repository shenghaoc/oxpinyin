//! The committed parity corpus: frozen input files, their documented sizes, and
//! their line discipline.
//!
//! The corpus holds inputs only and is not regenerated. Its bytes are also pinned
//! by the digests in `tools/bisection/axis-c-replacement.json`.

use std::path::PathBuf;

/// The committed strata and their counts, frozen in `docs/testing/parity-corpus.md`.
const STRATA: &[(&str, usize)] = &[
    ("01-single-syllable.txt", 405),
    ("02-syllable-pairs.txt", 2_000),
    ("03-short-utterances.txt", 2_500),
    ("04-long-utterances.txt", 1_500),
    ("05-apostrophe.txt", 1_200),
    ("06-partial-tails.txt", 1_200),
    ("07-ambiguity.txt", 800),
    ("08-junk.txt", 800),
    ("09-edge.txt", 60),
];

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/parity/corpus/inputs")
}

/// One input per line. Empty lines are kept, because the empty input is a corpus member.
fn parse_file_bytes(bytes: &[u8]) -> Vec<String> {
    std::str::from_utf8(bytes)
        .expect("corpus inputs must be valid UTF-8")
        .split_inclusive('\n')
        .map(|line| line.strip_suffix('\n').unwrap_or(line).to_owned())
        .collect()
}

#[test]
fn corpus_directory_holds_exactly_the_committed_strata() {
    let dir = corpus_dir();
    let mut found: Vec<String> = std::fs::read_dir(&dir)
        .expect("corpus directory exists")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    found.sort();
    let expected: Vec<&str> = STRATA.iter().map(|(name, _)| *name).collect();
    assert_eq!(
        found,
        expected,
        "stray or missing files in {}",
        dir.display()
    );
}

#[test]
fn committed_corpus_meets_the_documented_size_and_counts() {
    let dir = corpus_dir();
    let mut total = 0_usize;

    for (file_name, expected_count) in STRATA {
        let bytes = std::fs::read(dir.join(file_name)).expect("stratum is committed");
        let inputs = parse_file_bytes(&bytes);
        assert_eq!(
            inputs.len(),
            *expected_count,
            "{file_name} holds {} inputs, finding says {expected_count}",
            inputs.len()
        );
        total += inputs.len();
    }

    assert_eq!(total, 10_465, "corpus total drifted from the finding");
    assert!(total >= 10_000, "corpus is below the W2-T2 floor");
}

#[test]
fn committed_lines_carry_no_stray_whitespace_or_control_bytes() {
    let dir = corpus_dir();

    for (file_name, _) in STRATA {
        let bytes = std::fs::read(dir.join(file_name)).expect("stratum is committed");
        assert!(bytes.ends_with(b"\n"), "{file_name} must end with LF");
        assert!(
            bytes
                .iter()
                .all(|byte| *byte == b'\n' || (b' '..=b'~').contains(byte)),
            "{file_name} must contain only printable ASCII inputs and LF separators"
        );
        let inputs = parse_file_bytes(&bytes);
        assert_eq!(
            inputs.iter().filter(|input| input.is_empty()).count(),
            usize::from(*file_name == "09-edge.txt"),
            "{file_name} must contain exactly its documented empty inputs"
        );
        for input in inputs {
            assert!(
                !input.starts_with(' ') && !input.ends_with(' '),
                "{file_name}: {input:?} carries edge whitespace"
            );
        }
    }
}
