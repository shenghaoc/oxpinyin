//! Integration: the exported tables through the real Session, over the
//! W2 parity corpus.
//!
//! Constructs `Session<SystemDictionary, BigramLanguageModel>` from the
//! tables `pinyin-migrate export` writes to `/tmp/pinyin-rs-export` and
//! compares candidates with the live oracle over the whole corpus,
//! reporting top-1, top-5-set, prefix-10 overlap and the absent count.
//!
//! Requires `oracle-ffi`, the pin-built oracle, and a prior export run;
//! missing tables or corpus skip the test so the portable workspace run
//! is unaffected.

#![cfg(feature = "oracle-ffi")]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use pinyin_data::{BigramLanguageModel, SystemDictionary};
use pinyin_engine::{EmptyConfigSource, KeyInput, Session, StoragePaths};
use pinyin_oracle::{Oracle, OracleFlags, OraclePrefix, corpus};

/// Depth the capture protocol records candidates to.
const CAPTURE_DEPTH: usize = 10;

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

fn real_candidates(session: &Session<SystemDictionary, BigramLanguageModel>) -> Vec<String> {
    session
        .candidates()
        .iter()
        .map(|c| c.text().to_owned())
        .collect()
}

fn type_input(session: &mut Session<SystemDictionary, BigramLanguageModel>, input: &str) {
    session.reset();
    for ch in input.chars() {
        // Junk / edge corpus members may not map to keys; keep going so the
        // parity denominator is the full corpus rather than a filtered subset.
        let _ = session.process_key(&KeyInput::character(ch));
    }
}

#[test]
fn real_tables_session_reports_parity() {
    let Some(dir) = export_dir() else { return };
    let Some(all_inputs) = load_w2_corpus() else {
        return;
    };

    eprintln!(
        "running real session over the W2 corpus ({} inputs) with {}",
        all_inputs.len(),
        dir.display()
    );

    let dict = SystemDictionary::open(
        &dir.join("pinyin_index.redb"),
        &dir.join("phrase_index.redb"),
    )
    .expect("SystemDictionary opens from the export");
    let lm =
        BigramLanguageModel::open(&dir.join("bigram.redb")).expect("BigramLanguageModel opens");

    let mut session = Session::new(&EmptyConfigSource, StoragePaths::new("user"), dict, lm)
        .expect("Session::new with exported tables");

    let prefix = OraclePrefix::locate().expect("oracle prefix");
    let mut oracle = Oracle::open_with_temp_user_dir(prefix).expect("oracle opens");
    let mut oracle_session = oracle
        .session(OracleFlags::DEFAULT)
        .expect("oracle session");

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

    for input in &all_inputs {
        let obs = match oracle_session.observe(input.as_bytes()) {
            Ok(obs) => obs,
            Err(error) => {
                eprintln!("oracle observe failed for {input:?}: {error}; skipping input");
                continue;
            }
        };
        if obs.candidates.is_empty() {
            continue;
        }
        total += 1;

        type_input(&mut session, input);
        let real_cands = real_candidates(&session);
        let oracle_top = &obs.candidates[0];

        if real_cands.first() == Some(oracle_top) {
            top1 += 1;
        }
        if real_cands.iter().take(5).any(|text| text == oracle_top) {
            top5 += 1;
        }

        let real_prefix: BTreeSet<&String> = real_cands.iter().take(CAPTURE_DEPTH).collect();
        for cand in obs.candidates.iter().take(CAPTURE_DEPTH) {
            prefix_depth += 1;
            if real_prefix.contains(cand) {
                prefix_overlap += 1;
            }
        }

        if !real_cands.is_empty() && !real_cands.iter().any(|text| text == oracle_top) {
            absent += 1;
        }
    }

    let top1_pct = if total == 0 { 0 } else { top1 * 100 / total };
    let top5_pct = if total == 0 { 0 } else { top5 * 100 / total };
    let prefix_pct = if prefix_depth == 0 {
        0
    } else {
        prefix_overlap * 100 / prefix_depth
    };

    // Print rates before any assertion (required for the parity report).
    eprintln!("real tables parity — candidates, W2 parity corpus");
    eprintln!("  compared                {total}");
    eprintln!("  top-1                   {top1:>6}  {top1_pct}%");
    eprintln!("  top-5-set               {top5:>6}  {top5_pct}%");
    eprintln!("  prefix-10 overlap       {prefix_overlap:>6} of {prefix_depth}  {prefix_pct}%");
    eprintln!("  absent                  {absent:>6}");

    assert!(
        total > 0,
        "oracle produced no candidates over the W2 corpus; cannot report parity"
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
