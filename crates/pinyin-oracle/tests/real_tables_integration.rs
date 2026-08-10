//! Integration: the exported tables through the real Session, over the
//! W2 parity corpus.
//!
//! Constructs `Session<SystemDictionary, BigramLanguageModel>` from the
//! tables `pinyin-migrate export` writes to `/tmp/pinyin-rs-export` and
//! compares candidates with the oracle's **frozen** candidate lists at
//! `fixtures/w4/oracle-candidates.txt`.
//!
//! Two tiers:
//!
//! - **Portable** (`real_tables_session_reports_parity`): no `oracle-ffi`,
//!   no Linux requirement. Loads `oracle-candidates.txt` (pin-stamped,
//!   depth 10, sorted) and reports top-1, top-5-set, prefix-10 and absent.
//!   This is the fast parity check every scoring change re-runs.
//!
//! - **Freshness** (`oracle_candidates_fixture_is_fresh`, `oracle-ffi`
//!   only): re-queries the live pin-built oracle over the whole corpus and
//!   asserts the frozen file matches exactly. Re-run
//!   `cargo run -p pinyin-oracle --features oracle-ffi --bin oracle_candidates`
//!   when the pin changes.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use pinyin_data::{BigramLanguageModel, SystemDictionary};
use pinyin_engine::{EmptyConfigSource, Session, StoragePaths};
use pinyin_oracle::corpus;

/// Depth the capture protocol records candidates to.
const CAPTURE_DEPTH: usize = 10;

/// Relative path from the repository root to the frozen candidate fixture.
const CANDIDATES_FIXTURE: &str = "fixtures/w4/oracle-candidates.txt";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn export_dir() -> Option<PathBuf> {
    let dir = Path::new("/tmp/pinyin-rs-export").to_path_buf();
    if ["pinyin_index.redb", "phrase_index.redb", "bigram.redb"]
        .iter()
        .all(|name| dir.join(name).exists())
    {
        Some(dir)
    } else {
        eprintln!(
            "exported tables not found at /tmp/pinyin-rs-export; skipping \
             (run pinyin-migrate export first)"
        );
        None
    }
}

/// Load every input from the committed W2 parity corpus strata.
///
/// Returns `None` when the corpus directory is missing or no stratum file
/// can be read — the caller skips rather than inventing a stand-in set.
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

/// Parsed contents of `oracle-candidates.txt`.
struct CandidateFixture {
    /// Pin reference stamped in the header (`# pin_ref=...`).
    pin_ref: String,
    /// `input -> candidates` in rank order (1..n, n ≤ 10), deduplicated and sorted.
    by_input: BTreeMap<String, Vec<String>>,
    /// Raw file text (for freshness byte-for-byte check if desired).
    _raw: String,
}

fn load_candidate_fixture() -> Option<CandidateFixture> {
    let path = repo_root().join(CANDIDATES_FIXTURE);
    let raw = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        // A genuinely absent fixture is a skip: a checkout without the W4
        // fixtures still builds, and the freshness test regenerates it.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!(
                "candidate fixture not found at {}; skipping \
                 (run cargo run -p pinyin-oracle --features oracle-ffi --bin oracle_candidates)",
                path.display()
            );
            return None;
        }
        // Present but unreadable is a hard failure, not a silent skip.
        Err(error) => panic!(
            "candidate fixture at {} is present but unreadable: {error}",
            path.display()
        ),
    };

    let mut pin_ref = String::new();
    let mut by_input: BTreeMap<String, Vec<String>> = BTreeMap::new();

    // The fixture is a committed ~97k-line file. Once it is present it must
    // parse: a corruption that returned None would silently pass CI as a
    // skipped test, so every malformed-content case below panics.
    for line in raw.lines() {
        if let Some(value) = line.strip_prefix("# pin_ref=") {
            pin_ref = value.to_owned();
            continue;
        }
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        // input<TAB>rank<TAB>candidate_text ; input may be empty -> leading TAB
        let mut parts = line.split('\t');
        let input = parts.next().unwrap_or("").to_owned();
        let Some(rank_str) = parts.next() else {
            panic!("malformed fixture line (missing rank): {line:?}");
        };
        let Some(candidate) = parts.next() else {
            panic!("malformed fixture line (missing candidate): {line:?}");
        };
        let candidate = candidate.to_owned();
        assert!(
            parts.next().is_none(),
            "malformed fixture line (extra tab): {line:?}"
        );
        let Ok(rank) = rank_str.parse::<usize>() else {
            panic!("malformed rank {rank_str:?} in {line:?}");
        };
        let entry = by_input.entry(input.clone()).or_default();
        // File is sorted by input then rank ascending, so each push is next rank.
        assert_eq!(
            rank,
            entry.len() + 1,
            "fixture not sorted or rank gap for input {input:?} in {line:?}"
        );
        entry.push(candidate);
    }

    assert!(
        !pin_ref.is_empty(),
        "candidate fixture at {} is present but has no # pin_ref header",
        path.display()
    );

    Some(CandidateFixture {
        pin_ref,
        by_input,
        _raw: raw,
    })
}

fn real_candidates(session: &Session<SystemDictionary, BigramLanguageModel>) -> Vec<String> {
    session
        .candidates()
        .iter()
        .map(|c| c.text().to_owned())
        .collect()
}

fn type_input(session: &mut Session<SystemDictionary, BigramLanguageModel>, input: &str) {
    session.reset();
    // One refresh for the whole string: final candidates match per-keystroke
    // typing when nothing is selected mid-composition, and the full corpus
    // finishes in seconds rather than tens of minutes.
    let _ = session.type_pinyin(input);
}

#[test]
fn real_tables_session_reports_parity() {
    let Some(dir) = export_dir() else { return };
    let Some(all_inputs) = load_w2_corpus() else {
        return;
    };
    let Some(fixture) = load_candidate_fixture() else {
        return;
    };

    // Pin sanity — the fixture is off-pin if this trips, but don't fail the
    // parity suite hard; the freshness test will catch it and the parity
    // numbers would be meaningless anyway.
    if fixture.pin_ref != pinyin_oracle::EXPECTED_PIN_REF {
        eprintln!(
            "candidate fixture pin_ref {} does not match expected {}; \
             re-run oracle_candidates with the pinned oracle",
            fixture.pin_ref,
            pinyin_oracle::EXPECTED_PIN_REF
        );
    }

    eprintln!(
        "running real session over the W2 corpus ({} inputs) with {} against {}",
        all_inputs.len(),
        dir.display(),
        Path::new(CANDIDATES_FIXTURE).display()
    );

    let dict = SystemDictionary::open(
        &dir.join("pinyin_index.redb"),
        &dir.join("phrase_index.redb"),
    )
    .expect("SystemDictionary opens from the export");
    let mut lm =
        BigramLanguageModel::open(&dir.join("bigram.redb")).expect("BigramLanguageModel opens");
    // Wire dictionary frequencies into the LM so score() can interpolate
    // unigram + bigram per docs/findings/scoring-spec.md.
    lm.set_unigrams_from_dict(&dict);

    let mut session = Session::new(&EmptyConfigSource, StoragePaths::new("user"), dict, lm)
        .expect("Session::new with exported tables");

    // Candidate metrics (docs/findings/decode-differential.md):
    //   top-1      — our first candidate is the pin's first candidate
    //   top-5-set  — the pin's first candidate appears in our first five
    //   prefix-10  — share of the pin's first ten that appear in our first ten
    //   absent     — we produced candidates, but the pin's first is not among them
    let mut total = 0_usize;
    let mut top1 = 0_usize;
    let mut top5 = 0_usize;
    let mut prefix_overlap = 0_usize;
    let mut prefix_depth = 0_usize;
    let mut absent = 0_usize;
    let started = std::time::Instant::now();

    for input in &all_inputs {
        let oracle_cands = match fixture.by_input.get(input) {
            Some(list) if !list.is_empty() => list,
            _ => continue,
        };
        total += 1;

        type_input(&mut session, input);
        let real_cands = real_candidates(&session);
        let oracle_top = &oracle_cands[0];

        if real_cands.first() == Some(oracle_top) {
            top1 += 1;
        }
        if real_cands.iter().take(5).any(|text| text == oracle_top) {
            top5 += 1;
        }

        let real_prefix: BTreeSet<&String> = real_cands.iter().take(CAPTURE_DEPTH).collect();
        for cand in oracle_cands.iter().take(CAPTURE_DEPTH) {
            prefix_depth += 1;
            if real_prefix.contains(cand) {
                prefix_overlap += 1;
            }
        }

        if !real_cands.is_empty() && !real_cands.iter().any(|text| text == oracle_top) {
            absent += 1;
        }

        if total.is_multiple_of(1_000) {
            eprintln!(
                "  … {total} compared in {:?} (top-1 so far {}%)",
                started.elapsed(),
                (top1 * 100).checked_div(total).unwrap_or(0)
            );
        }
    }

    let top1_pct = (top1 * 100).checked_div(total).unwrap_or(0);
    let top5_pct = (top5 * 100).checked_div(total).unwrap_or(0);
    let prefix_pct = (prefix_overlap * 100)
        .checked_div(prefix_depth)
        .unwrap_or(0);

    // Print rates before any assertion (required for the parity report).
    eprintln!("real tables parity — candidates, W2 parity corpus (fixture)");
    eprintln!("  fixture pin_ref         {}", fixture.pin_ref);
    eprintln!("  compared                {total}");
    eprintln!("  top-1                   {top1:>6}  {top1_pct}%");
    eprintln!("  top-5-set               {top5:>6}  {top5_pct}%");
    eprintln!("  prefix-10 overlap       {prefix_overlap:>6} of {prefix_depth}  {prefix_pct}%");
    eprintln!("  absent                  {absent:>6}");

    assert!(
        total > 0,
        "fixture produced no candidates over the W2 corpus; cannot report parity"
    );
    // Regression floors under the measured Stage-1 baseline for this pin
    // (2026-08-10, full corpus): top-1 39%, top-5-set 63%, prefix-10 44%,
    // absent 388 of 10,190. The floors sit below the measurement so noise
    // cannot flake the suite, while a ranking or data regression trips them.
    assert!(
        top1 * 100 >= total * 35,
        "top-1 fell to {top1_pct}% ({top1}/{total}); expected >= 35%"
    );
    assert!(
        top5 * 100 >= total * 55,
        "top-5-set fell to {top5_pct}% ({top5}/{total}); expected >= 55%"
    );
    assert!(
        absent * 100 <= total * 6,
        "absent rose to {absent}/{total}; expected <= 6%"
    );
}

/// Live-oracle freshness check for `fixtures/w4/oracle-candidates.txt`.
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
/// cargo run -p pinyin-oracle --features oracle-ffi --bin oracle-candidates
/// ```
#[cfg(feature = "oracle-ffi")]
#[test]
#[ignore = "full-corpus live oracle run (~3h); regenerate with bin oracle-candidates"]
fn oracle_candidates_fixture_is_fresh() {
    use pinyin_oracle::{Oracle, OracleFlags, OraclePrefix};

    let fixture =
        load_candidate_fixture().expect("candidate fixture missing; cannot check freshness");

    assert_eq!(
        fixture.pin_ref,
        pinyin_oracle::EXPECTED_PIN_REF,
        "fixture pin_ref {} does not match expected pin {}",
        fixture.pin_ref,
        pinyin_oracle::EXPECTED_PIN_REF
    );

    // Header total_triples must match the parsed payload.
    let path = repo_root().join(CANDIDATES_FIXTURE);
    let raw = std::fs::read_to_string(&path).expect("fixture readable");
    let mut header_triples: Option<usize> = None;
    for line in raw.lines() {
        if let Some(rest) = line.strip_prefix("# total_triples=") {
            // header is "# total_triples=97442 (distinct ...)"
            let num: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            header_triples = num.parse().ok();
        }
    }
    let actual_triples: usize = fixture.by_input.values().map(Vec::len).sum();
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
        let frozen_list: &[String] = fixture
            .by_input
            .get(input)
            .map(Vec::as_slice)
            .unwrap_or(&[]);

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
         re-run: cargo run -p pinyin-oracle --features oracle-ffi --bin oracle-candidates -- {}",
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
    let committed_payload: Vec<&str> = raw
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
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
