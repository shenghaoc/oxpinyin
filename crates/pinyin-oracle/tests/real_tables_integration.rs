//! Integration: SystemDictionary + BigramLanguageModel from the full oracle tables
//! through the real Session, run over the W2 parity corpus.
//!
//! This is the first real parity run against real data, not fixtures.
//! It constructs `Session<SystemDictionary, BigramLanguageModel>` from the
//! full redb tables at `/tmp/*_full.redb` (or the prefix's data dir) and
//! reports top-1, top-5-set, prefix-10 over the corpus.
//!
//! Requires `oracle-ffi` and the pin-built oracle on Linux with the full
//! redb files generated via `pinyin-migrate`.
//! The test is `#[ignore]`-like: if the full redb files or the W2 corpus are
//! not present, it succeeds without asserting, so `cargo test --workspace`
//! without oracle still passes.

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

fn find_full_redb(name: &str) -> Option<PathBuf> {
    // Try /tmp first (where we generated via pinyin-migrate), then the prefix's data dir
    let candidates = [
        Path::new("/tmp").join(name),
        Path::new(&format!(
            "/tmp/{}_full.redb",
            name.trim_end_matches(".redb")
        ))
        .to_path_buf(),
    ];
    for p in candidates {
        if p.exists() {
            return Some(p);
        }
    }
    // Try via OraclePrefix
    if let Ok(prefix) = OraclePrefix::locate() {
        let data = prefix.data_dir();
        // The redb files are not in the prefix's data dir (they are .bin), but we can try
        // to find a converted redb near the prefix
        let alt = data.join(name);
        if alt.exists() {
            return Some(alt);
        }
    }
    None
}

/// Load every input from the committed W2 parity corpus strata.
///
/// Returns `None` when the corpus directory is missing or no stratum file can
/// be read — the caller skips rather than inventing a tiny stand-in set.
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
    // Locate full redb files
    let pinyin_index = find_full_redb("pinyin_index.redb")
        .or_else(|| find_full_redb("pinyin_index_full.redb"))
        .or_else(|| {
            let p = Path::new("/tmp/pinyin_index_full.redb");
            if p.exists() {
                Some(p.to_path_buf())
            } else {
                None
            }
        });
    let phrase_index = find_full_redb("phrase_index.redb").or_else(|| {
        let p = Path::new("/tmp/phrase_index_full.redb");
        if p.exists() {
            Some(p.to_path_buf())
        } else {
            None
        }
    });
    let bigram = find_full_redb("bigram.redb").or_else(|| {
        let p = Path::new("/tmp/bigram_full.redb");
        if p.exists() {
            Some(p.to_path_buf())
        } else {
            None
        }
    });

    let (pinyin_path, phrase_path, bigram_path) = match (pinyin_index, phrase_index, bigram) {
        (Some(a), Some(b), Some(c)) => (a, b, c),
        _ => {
            eprintln!(
                "full redb files not found at /tmp/*_full.redb; skipping real_tables integration (run pinyin-migrate first)"
            );
            return;
        }
    };

    let Some(all_inputs) = load_w2_corpus() else {
        return;
    };

    eprintln!("Using pinyin_index {:?}", pinyin_path);
    eprintln!("Using phrase_index {:?}", phrase_path);
    eprintln!("Using bigram {:?}", bigram_path);
    eprintln!(
        "Running real session over full W2 corpus ({} inputs)",
        all_inputs.len()
    );

    let dict = SystemDictionary::open(&pinyin_path, &phrase_path)
        .expect("SystemDictionary opens from full redb");
    let lm = BigramLanguageModel::open(&bigram_path).expect("BigramLanguageModel opens");

    let mut session = Session::new(&EmptyConfigSource, StoragePaths::new("user"), dict, lm)
        .expect("Session::new with real tables");

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
                eprintln!("oracle observe failed for {input:?}: {error}; counting as unobservable");
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
    assert!(
        top1 * 100 >= total * 80,
        "top-1 fell to {top1_pct}% ({top1}/{total}); expected >= 80%"
    );
}
