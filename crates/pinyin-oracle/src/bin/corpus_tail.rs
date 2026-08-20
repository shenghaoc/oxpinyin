//! Enumerate the W2 corpus tail — the residual inputs the pinned
//! `real_tables_session_reports_parity` measurement counts against pin
//! `0c5e80e`.
//!
//! The parity test reports the top-1, top-5, absent, and prefix-10 pins
//! but does not name the inputs that drive them. This binary reproduces
//! the same scoring path (real unigrams from `interpolation2.text`,
//! shared read-only tables) and prints every residual input by category:
//!
//! - `top1-miss`   — oracle #1 is not our #1;
//! - `top5-miss`   — oracle #1 is not in our top-5;
//! - `absent`      — oracle #1 does not appear in our candidate list at all;
//! - `order-only`  — same depth-10 set, different order (the tie-swap class).
//!
//! Read-only, single-threaded, no oracle FFI — the fixture already holds
//! the pin's answer for every corpus input, so no fresh oracle call is
//! made. Prints to stdout for capture into `docs/findings/corpus-tail.md`.
//!
//! ```bash
//! PINYIN_EXPORT_DIR=/tmp/oxpinyin-export \
//! PINYIN_MODEL_DIR=<extracted-model20> \
//! cargo run -p pinyin-oracle --release --bin corpus-tail
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use oxpinyin_data::{BigramLanguageModel, SystemDictionary};
use oxpinyin_engine::{EmptyConfigSource, Session, StoragePaths};
use pinyin_oracle::corpus;

const CANDIDATES_FIXTURE: &str = "fixtures/w4/oracle-candidates.txt";
const CAPTURE_DEPTH: usize = 10;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn export_dir() -> Result<PathBuf, String> {
    let dir = std::env::var_os("PINYIN_EXPORT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new("/tmp/oxpinyin-export").to_path_buf());
    if ["pinyin_index.redb", "phrase_index.redb", "bigram.redb"]
        .iter()
        .all(|name| dir.join(name).exists())
    {
        Ok(dir)
    } else {
        Err(format!("exported tables not found at {}", dir.display()))
    }
}

fn load_fixture(path: &Path) -> Result<BTreeMap<String, Vec<String>>, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read fixture {}: {error}", path.display()))?;
    let mut by_input: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for line in raw.lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let mut parts = line.split('\t');
        let input = parts.next().unwrap_or("").to_owned();
        let _rank = parts.next();
        let cand = parts.next().unwrap_or("").to_owned();
        by_input.entry(input).or_default().push(cand);
    }
    Ok(by_input)
}

fn run() -> Result<(), String> {
    let dir = export_dir()?;
    let Ok(Some(model_dir)) = pinyin_oracle::model_cache::locate_model_dir() else {
        return Err(
            "model cache absent; set PINYIN_MODEL_DIR to an extracted model20 dir".to_owned(),
        );
    };

    let dict = SystemDictionary::open(
        &dir.join("pinyin_index.redb"),
        &dir.join("phrase_index.redb"),
    )
    .map_err(|error| format!("cannot open SystemDictionary: {error}"))?;
    let mut lm = BigramLanguageModel::open(&dir.join("bigram.redb"))
        .map_err(|error| format!("cannot open BigramLanguageModel: {error}"))?;
    lm.set_unigrams_from_interpolation2(&model_dir.join("interpolation2.text"))
        .map_err(|error| format!("cannot parse interpolation2: {error}"))?;
    let mut session = Session::new(&EmptyConfigSource, StoragePaths::new("user"), &dict, &lm)
        .map_err(|error| format!("cannot create Session: {error}"))?;

    let fixture = load_fixture(&repo_root().join(CANDIDATES_FIXTURE))?;

    let corpus_dir = repo_root().join(corpus::CORPUS_DIR);
    let mut inputs = Vec::new();
    for stratum in corpus::generate() {
        let path = corpus_dir.join(stratum.file_name);
        let bytes = std::fs::read(&path)
            .map_err(|error| format!("cannot read corpus stratum {}: {error}", path.display()))?;
        inputs.extend(corpus::Stratum::parse_file_bytes(&bytes));
    }

    let mut top1_miss = Vec::new();
    let mut top5_miss = Vec::new();
    let mut absent = Vec::new();
    let mut order_only_count = 0_usize;
    let mut prefix_gap_sum = 0_usize;
    let mut prefix_gap_denom = 0_usize;
    let mut total = 0_usize;

    for input in &inputs {
        let Some(oracle_cands) = fixture.get(input).filter(|l| !l.is_empty()) else {
            continue;
        };
        total += 1;
        session.reset();
        let _ = session.type_pinyin(input);
        let ours: Vec<String> = session
            .candidates()
            .iter()
            .map(|c| c.text().to_owned())
            .collect();
        let oracle_top = &oracle_cands[0];

        let our_prefix = &ours[..ours.len().min(CAPTURE_DEPTH)];
        let oracle_prefix = &oracle_cands[..oracle_cands.len().min(CAPTURE_DEPTH)];

        for cand in oracle_prefix {
            prefix_gap_denom += 1;
            if !our_prefix.contains(cand) {
                prefix_gap_sum += 1;
            }
        }

        let our_set: std::collections::HashSet<&str> =
            our_prefix.iter().map(String::as_str).collect();
        let oracle_set: std::collections::HashSet<&str> =
            oracle_prefix.iter().map(String::as_str).collect();
        if our_set == oracle_set && our_prefix != oracle_prefix {
            order_only_count += 1;
        }

        let our_rank = ours.iter().position(|t| t == oracle_top);
        if our_rank == Some(0) {
            continue;
        }

        let rank_display = our_rank.map_or("absent".to_owned(), |i| format!("#{}", i + 1));
        let our_top = ours.first().cloned().unwrap_or_else(|| "(none)".to_owned());
        let row = format!(
            "input={input:?}\toracle_top={oracle_top:?}\tour_top={our_top:?}\tour_rank={rank_display}\n\
             \tours[:5]={:?}\n\toracle[:5]={:?}",
            &our_prefix[..our_prefix.len().min(5)],
            &oracle_prefix[..oracle_prefix.len().min(5)],
        );

        top1_miss.push(row.clone());
        if !ours.iter().take(5).any(|t| t == oracle_top) {
            top5_miss.push(row.clone());
        }
        if !ours.iter().any(|t| t == oracle_top) {
            absent.push(row);
        }
    }

    println!("== W2 corpus tail residuals ==");
    println!("compared            {total}");
    println!("top-1 misses        {}", top1_miss.len());
    println!("top-5 misses        {}", top5_miss.len());
    println!("absent              {}", absent.len());
    println!("order-only          {order_only_count}");
    println!(
        "prefix-10 gap       {prefix_gap_sum} of {prefix_gap_denom} (positions not in our top-10)"
    );

    println!("\n== top-1 misses ({}) ==", top1_miss.len());
    for row in &top1_miss {
        println!("{row}");
    }

    println!("\n== top-5 misses ({}) ==", top5_miss.len());
    for row in &top5_miss {
        println!("{row}");
    }

    println!("\n== absent ({}) ==", absent.len());
    for row in &absent {
        println!("{row}");
    }

    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}
