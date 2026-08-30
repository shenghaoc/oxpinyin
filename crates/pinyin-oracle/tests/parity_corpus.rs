//! W2-T2 acceptance: the committed corpus is exactly what the generator emits.
//!
//! Runs on the portable tier with no oracle present, so corpus drift fails CI
//! rather than surfacing later as unexplained W2-T3 divergence.
//!
//! Regenerate with:
//!
//! ```bash
//! cargo run -p pinyin-oracle --bin parity-corpus -- tests/parity/corpus/inputs
//! git diff --exit-code tests/parity/corpus/inputs
//! ```

use std::path::PathBuf;

use pinyin_oracle::corpus::{self, Stratum};

/// Repository root, derived from this crate's manifest directory.
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn corpus_dir() -> PathBuf {
    repo_root().join(corpus::CORPUS_DIR)
}

#[test]
fn committed_corpus_matches_the_generator_byte_for_byte() {
    let dir = corpus_dir();

    for stratum in corpus::generate() {
        let path = dir.join(stratum.file_name);
        let committed = std::fs::read(&path).unwrap_or_else(|error| {
            panic!(
                "cannot read {}: {error}\n\
                 regenerate with: cargo run -p pinyin-oracle --bin parity-corpus -- {}",
                path.display(),
                corpus::CORPUS_DIR
            )
        });

        let expected = stratum.to_file_bytes();
        assert!(
            committed == expected,
            "{} differs from the generator ({} committed inputs vs {} generated); \
             regenerate with: cargo run -p pinyin-oracle --bin parity-corpus -- {}",
            stratum.file_name,
            Stratum::parse_file_bytes(&committed).len(),
            stratum.inputs.len(),
            corpus::CORPUS_DIR
        );
    }
}

#[test]
fn corpus_directory_holds_exactly_the_generated_strata() {
    let dir = corpus_dir();
    let mut found: Vec<String> = std::fs::read_dir(&dir)
        .expect("corpus directory exists")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    found.sort();

    let mut expected: Vec<String> = corpus::generate()
        .iter()
        .map(|stratum| stratum.file_name.to_owned())
        .collect();
    expected.sort();

    assert_eq!(
        found,
        expected,
        "stray or missing files in {}",
        corpus::CORPUS_DIR
    );
}

#[test]
fn committed_corpus_meets_the_documented_size_and_counts() {
    let dir = corpus_dir();
    let mut total = 0_usize;

    // Counts frozen in docs/testing/parity-corpus.md.
    let documented: &[(&str, usize)] = &[
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

    for (file_name, expected_count) in documented {
        let bytes = std::fs::read(dir.join(file_name)).expect("stratum is committed");
        let inputs = Stratum::parse_file_bytes(&bytes);
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

    for stratum in corpus::generate() {
        let bytes = std::fs::read(dir.join(stratum.file_name)).expect("stratum is committed");
        assert!(
            bytes.ends_with(b"\n"),
            "{} must end with LF",
            stratum.file_name
        );
        assert!(
            !bytes.windows(2).any(|pair| pair == b"\n\n") || stratum.file_name == "09-edge.txt",
            "{} has a blank line outside the edge stratum",
            stratum.file_name
        );

        for input in Stratum::parse_file_bytes(&bytes) {
            assert!(
                !input.starts_with(' ') && !input.ends_with(' '),
                "{}: {input:?} carries edge whitespace",
                stratum.file_name
            );
            assert!(
                !input.bytes().any(|byte| byte == b'\r' || byte == b'\t'),
                "{}: {input:?} carries a control byte",
                stratum.file_name
            );
        }
    }
}
