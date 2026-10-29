//! Integration: live-oracle freshness checks for the frozen candidate and
//! sentence-surface fixtures (`fixtures/w4/`).
//!
//! These tests are `#[ignore]`d by default — they re-query the pin-built
//! oracle over the whole W2 corpus and assert the frozen fixture matches.
//! Re-run with:
//!
//! ```text
//! cargo test -p pinyin-oracle --features oracle-ffi \
//!     --test real_tables_integration -- --ignored --nocapture
//! ```

#[cfg(feature = "oracle-ffi")]
use std::collections::BTreeMap;
#[cfg(feature = "oracle-ffi")]
use std::path::PathBuf;

#[cfg(feature = "oracle-ffi")]
use pinyin_oracle::candidate_fixture::{CandidateFixture, STRUCTURE_FIXTURE};
#[cfg(feature = "oracle-ffi")]
use pinyin_oracle::corpus;

#[cfg(feature = "oracle-ffi")]
const CAPTURE_DEPTH: usize = 10;

#[cfg(feature = "oracle-ffi")]
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

#[cfg(feature = "oracle-ffi")]
fn load_w2_corpus() -> Option<Vec<String>> {
    let corpus_dir = repo_root().join(corpus::CORPUS_DIR);
    if !corpus_dir.is_dir() {
        eprintln!(
            "W2 parity corpus not found at {}; skipping real_tables integration",
            corpus_dir.display()
        );
        return None;
    }

    let mut all_inputs: Vec<String> = Vec::new();
    for stratum in corpus::generate() {
        let path = corpus_dir.join(stratum.file_name);
        match std::fs::read(&path) {
            Ok(bytes) => all_inputs.extend(corpus::Stratum::parse_file_bytes(&bytes)),
            Err(error) => {
                eprintln!(
                    "cannot read corpus stratum {}: {error}; skipping real_tables integration",
                    path.display()
                );
                return None;
            }
        }
    }

    if all_inputs.is_empty() {
        eprintln!(
            "W2 parity corpus at {} is empty; skipping real_tables integration",
            corpus_dir.display()
        );
        return None;
    }

    Some(all_inputs)
}

/// Live-oracle freshness check for the candidate-only view of `fixtures/w4/oracle-candidate-structure.txt`.
///
/// Re-queries the pin-built oracle over the whole W2 corpus (~3 hours) and
/// asserts the frozen file matches. Ignored by default; re-run with:
///
/// ```text
/// cargo test -p pinyin-oracle --features oracle-ffi \
///     --test real_tables_integration \
///     oracle_candidates_fixture_is_fresh -- --ignored --nocapture
/// ```
///
/// When the pin changes, regenerate first:
///
/// ```text
/// cargo run -p pinyin-oracle --features oracle-ffi --bin oracle-candidate-structure
/// ```
#[cfg(feature = "oracle-ffi")]
#[test]
#[ignore = "full-corpus live oracle run (~3h); regenerate with bin oracle-candidate-structure"]
fn oracle_candidates_fixture_is_fresh() {
    use pinyin_oracle::{Oracle, OracleFlags, OraclePrefix};

    let fixture = CandidateFixture::read(repo_root().join(STRUCTURE_FIXTURE))
        .expect("candidate structure fixture");

    assert_eq!(
        fixture.pin_ref,
        pinyin_oracle::EXPECTED_PIN_REF,
        "fixture pin_ref {} does not match expected pin {}",
        fixture.pin_ref,
        pinyin_oracle::EXPECTED_PIN_REF
    );

    let by_input = fixture.candidate_lists();

    // Header total_triples must match the parsed payload.
    let path = repo_root().join(STRUCTURE_FIXTURE);
    let raw = std::fs::read_to_string(&path).expect("fixture readable");
    let mut header_triples: Option<usize> = None;
    for line in raw.lines() {
        if let Some(rest) = line.strip_prefix("# total_triples=") {
            let num: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            header_triples = num.parse().ok();
        }
    }
    let actual_triples: usize = by_input.values().map(Vec::len).sum();
    if let Some(expected) = header_triples {
        assert_eq!(
            expected, actual_triples,
            "header total_triples {expected} does not match actual {actual_triples}"
        );
    }

    // Re-query the live oracle for every distinct corpus input and compare.
    let prefix = OraclePrefix::locate().expect("oracle prefix");
    let mut oracle = Oracle::open_with_temp_user_dir(prefix).expect("oracle opens");
    let mut oracle_session = oracle
        .session(OracleFlags::DEFAULT)
        .expect("oracle session");

    let all_inputs = load_w2_corpus().expect("W2 corpus present for freshness check");
    let mut distinct: BTreeMap<String, ()> = BTreeMap::new();
    for input in all_inputs {
        distinct.entry(input).or_insert(());
    }

    let mut sample_mismatches: Vec<String> = Vec::new();
    let mut mismatch_count = 0_usize;
    let mut checked = 0_usize;
    let mut live_triples = 0_usize;
    let mut live_map: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for input in distinct.keys() {
        let obs = oracle_session
            .observe(input.as_bytes())
            .unwrap_or_else(|e| panic!("oracle observe failed for {input:?}: {e}"));
        let live: Vec<String> = obs.candidates.into_iter().take(CAPTURE_DEPTH).collect();
        live_triples += live.len();
        let frozen_list: &[String] = by_input.get(input).map(Vec::as_slice).unwrap_or(&[]);

        if live != frozen_list {
            mismatch_count += 1;
            if sample_mismatches.len() < 20 {
                sample_mismatches.push(format!(
                    "input {input:?}: live {live:?} vs frozen {frozen_list:?}"
                ));
            }
        }
        if !live.is_empty() {
            live_map.insert(input.clone(), live);
        }
        checked += 1;
    }

    eprintln!("freshness check — {checked} distinct inputs, {live_triples} live triples");
    eprintln!("  fixture pin_ref {}", fixture.pin_ref);
    eprintln!("  fixture triples {actual_triples}");

    assert!(
        mismatch_count == 0,
        "candidate fixture is stale ({mismatch_count} mismatches, showing {}): {}\n\
         re-run: cargo run -p pinyin-oracle --features oracle-ffi --bin oracle-candidate-structure -- {}",
        sample_mismatches.len(),
        sample_mismatches.join("\n"),
        path.display()
    );

    // Payload-only regeneration check from the live map already collected above
    // (no second oracle pass). Header drift alone is not fatal once map equality
    // has passed.
    let mut regenerated_payload: Vec<String> = Vec::new();
    for (input, cands) in &live_map {
        for (idx, cand) in cands.iter().enumerate() {
            regenerated_payload.push(format!("{input}\t{}\t{cand}", idx + 1));
        }
    }
    let committed_payload: Vec<String> = fixture
        .candidates()
        .map(|(input, rank, text)| format!("{input}\t{rank}\t{text}"))
        .collect();
    assert_eq!(
        committed_payload,
        regenerated_payload
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        "candidate payload differs; fixture must be regenerated"
    );
}

#[cfg(feature = "oracle-ffi")]
struct SentenceLine {
    /// Whether `pinyin_guess_sentence` reported success.
    guessed: bool,
    /// Decoded sentences the candidate list proves (`-` for none).
    sentences: Vec<String>,
    /// First candidates as `(type-letter, nbest, text)`; the letter is
    /// `n` for NBEST rows, `N` for normal, `a` for addon.
    rows: Vec<(char, Option<u8>, String)>,
}

#[cfg(feature = "oracle-ffi")]
struct SentenceFixture {
    pin_ref: String,
    by_input: BTreeMap<String, SentenceLine>,
}

#[cfg(feature = "oracle-ffi")]
fn load_sentence_fixture() -> Option<SentenceFixture> {
    const PATH: &str = "fixtures/w4/oracle-sentence-surface.txt";
    let raw = match std::fs::read_to_string(repo_root().join(PATH)) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!(
                "sentence fixture not found at {PATH}; skipping \
                 (run cargo run -p pinyin-oracle --features oracle-ffi --bin oracle_sentence_surface)"
            );
            return None;
        }
        Err(error) => panic!("sentence fixture at {PATH} is unreadable: {error}"),
    };

    let mut pin_ref = String::new();
    let mut by_input = BTreeMap::new();
    for line in raw.lines() {
        if let Some(value) = line.strip_prefix("# pin_ref=") {
            pin_ref = value.to_owned();
            continue;
        }
        if line.starts_with("# sample=") || line.is_empty() {
            continue;
        }
        let mut parts = line.split('\t');
        let input = parts.next().unwrap_or("").to_owned();
        let guessed = parts.next().unwrap_or("false") == "true";
        let sentences_field = parts.next().unwrap_or("");
        let sentences: Vec<String> = if sentences_field.is_empty() {
            Vec::new()
        } else {
            sentences_field.split('\u{1}').map(str::to_owned).collect()
        };
        let rows = parts
            .next()
            .unwrap_or("")
            .split('\u{1}')
            .filter(|row| !row.is_empty())
            .map(|row| {
                let mut fields = row.splitn(3, '/');
                let kind = fields.next().unwrap_or("?").chars().next().unwrap_or('?');
                let nbest = fields.next().and_then(|value| value.parse::<u8>().ok());
                let text = fields.next().unwrap_or("").to_owned();
                (kind, nbest, text)
            })
            .collect();
        assert!(
            parts.next().is_none(),
            "malformed sentence fixture line: {line:?}"
        );
        by_input.insert(
            input,
            SentenceLine {
                guessed,
                sentences,
                rows,
            },
        );
    }
    assert!(
        !pin_ref.is_empty(),
        "sentence fixture has no # pin_ref header"
    );
    Some(SentenceFixture { pin_ref, by_input })
}

/// Live-oracle freshness check for `fixtures/w4/oracle-sentence-surface.txt`.
///
/// Re-captures the same deterministic sample from the pin-built oracle and
/// asserts the frozen fixture matches exactly. Ignored by default; re-run
/// with:
///
/// ```text
/// cargo test -p pinyin-oracle --features oracle-ffi \
///     --test real_tables_integration \
///     sentence_surface_fixture_is_fresh -- --ignored --nocapture
/// ```
#[cfg(feature = "oracle-ffi")]
#[test]
#[ignore = "live oracle run (~1 min); regenerate with bin oracle_sentence_surface"]
fn sentence_surface_fixture_is_fresh() {
    use pinyin_oracle::{Oracle, OracleFlags, OraclePrefix};

    let fixture =
        load_sentence_fixture().expect("sentence fixture missing; cannot check freshness");
    assert_eq!(
        fixture.pin_ref,
        pinyin_oracle::EXPECTED_PIN_REF,
        "fixture pin_ref {} is off-pin",
        fixture.pin_ref
    );

    let prefix = OraclePrefix::locate().expect("oracle prefix");
    let mut oracle = Oracle::open_with_temp_user_dir(prefix).expect("oracle opens");
    let mut session = oracle.session(OracleFlags::DEFAULT).expect("session");

    let mut mismatches: Vec<String> = Vec::new();
    for (input, line) in &fixture.by_input {
        let surface = session
            .observe_sentence_surface(input.as_bytes(), 2)
            .unwrap_or_else(|error| panic!("oracle observe failed for {input:?}: {error}"));
        if surface.guessed != line.guessed {
            mismatches.push(format!("input {input:?}: guessed {}", surface.guessed));
            continue;
        }
        let sentences: Vec<String> = surface
            .sentences
            .iter()
            .map(|sentence| sentence.as_deref().unwrap_or("-").to_owned())
            .collect();
        if sentences != line.sentences {
            mismatches.push(format!(
                "input {input:?}: sentences {sentences:?} vs frozen {:?}",
                line.sentences
            ));
        }
        let rows: Vec<(char, Option<u8>, String)> = surface
            .candidates
            .iter()
            .take(6)
            .map(|info| {
                let kind = match info.candidate_type {
                    pinyin_oracle::OracleCandidateType::NbestMatch => 'n',
                    pinyin_oracle::OracleCandidateType::Normal => 'N',
                    pinyin_oracle::OracleCandidateType::Addon => 'a',
                    _ => '?',
                };
                (kind, info.nbest_index, info.text.clone())
            })
            .collect();
        if rows != line.rows {
            mismatches.push(format!(
                "input {input:?}: rows {rows:?} vs frozen {:?}",
                line.rows
            ));
        }
    }

    assert!(
        mismatches.is_empty(),
        "sentence fixture is stale ({} mismatches): {}",
        mismatches.len(),
        mismatches.join("\n  ")
    );
}
