//! Sample-based sweep of [`oxpinyin_core::scoring::ScoringConfig`] constants
//! against `fixtures/w4/oracle-candidates.txt`.
//!
//! ```bash
//! cargo run -p pinyin-oracle --release --bin parity-sweep
//! ```
//!
//! Requires `/tmp/oxpinyin-export` tables. Prints top-1 for each trial on a
//! fixed 2,000-input sample, then optionally re-scores the winner on the
//! full corpus when `PARITY_SWEEP_FULL=1`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use oxpinyin_core::scoring::ScoringConfig;
use oxpinyin_data::{BigramLanguageModel, SystemDictionary, default_store_file};
use oxpinyin_engine::{EmptyConfigSource, Session, StoragePaths};
use pinyin_oracle::corpus;

const SAMPLE: usize = 2_000;
const CANDIDATES_FIXTURE: &str = "fixtures/w4/oracle-candidates.txt";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn load_fixture() -> BTreeMap<String, Vec<String>> {
    let raw = std::fs::read_to_string(repo_root().join(CANDIDATES_FIXTURE))
        .expect("oracle-candidates fixture");
    let mut by_input: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for line in raw.lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let mut parts = line.split('\t');
        let input = parts.next().unwrap_or("").to_owned();
        let _rank = parts.next().unwrap_or("1");
        let cand = parts.next().unwrap_or("").to_owned();
        by_input.entry(input).or_default().push(cand);
    }
    by_input
}

fn load_corpus() -> Vec<String> {
    let dir = repo_root().join(corpus::CORPUS_DIR);
    let mut all = Vec::new();
    for stratum in corpus::generate() {
        let bytes = std::fs::read(dir.join(stratum.file_name)).expect("stratum");
        all.extend(corpus::Stratum::parse_file_bytes(&bytes));
    }
    all
}

struct Rates {
    total: usize,
    top1: usize,
    top5: usize,
    absent: usize,
}

fn measure(
    session: &mut Session<SystemDictionary, BigramLanguageModel>,
    inputs: &[String],
    fixture: &BTreeMap<String, Vec<String>>,
) -> Rates {
    let mut total = 0_usize;
    let mut top1 = 0_usize;
    let mut top5 = 0_usize;
    let mut absent = 0_usize;
    for input in inputs {
        let Some(oracle) = fixture.get(input).filter(|c| !c.is_empty()) else {
            continue;
        };
        total += 1;
        session.reset();
        let _ = session.type_pinyin(input);
        let real: Vec<String> = session
            .candidates()
            .iter()
            .map(|c| c.text().to_owned())
            .collect();
        let oracle_top = &oracle[0];
        if real.first() == Some(oracle_top) {
            top1 += 1;
        }
        if real.iter().take(5).any(|t| t == oracle_top) {
            top5 += 1;
        }
        if !real.is_empty() && !real.iter().any(|t| t == oracle_top) {
            absent += 1;
        }
    }
    Rates {
        total,
        top1,
        top5,
        absent,
    }
}

fn pct(n: usize, d: usize) -> usize {
    (n * 100).checked_div(d).unwrap_or(0)
}

fn main() -> ExitCode {
    let dir = Path::new("/tmp/oxpinyin-export");
    let missing: Vec<String> = ["pinyin_index", "phrase_index", "bigram"]
        .into_iter()
        .map(default_store_file)
        .filter(|table| !dir.join(table).is_file())
        .collect();
    if !missing.is_empty() {
        eprintln!(
            "missing {missing:?} in {dir:?}; copy or symlink the required \
             tables from the committed fixtures/w3/"
        );
        return ExitCode::from(2);
    }

    let dict = SystemDictionary::open(
        &dir.join(default_store_file("pinyin_index")),
        &dir.join(default_store_file("phrase_index")),
    )
    .unwrap_or_else(|error| {
        eprintln!("cannot open system dictionary from {dir:?}: {error}");
        std::process::exit(2);
    });
    let mut lm = BigramLanguageModel::open(&dir.join(default_store_file("bigram"))).unwrap_or_else(
        |error| {
            eprintln!("cannot open bigram model from {dir:?}: {error}");
            std::process::exit(2);
        },
    );
    lm.set_unigrams_from_dict(&dict);
    let mut session =
        Session::new(&EmptyConfigSource, StoragePaths::new("user"), dict, lm).expect("session");

    let fixture = load_fixture();
    let all_inputs = load_corpus();
    // Stratified sample: every k-th input with candidates, so short and long
    // strata both appear (the first N of the corpus is alphabetically short).
    let with_cands: Vec<String> = all_inputs
        .iter()
        .filter(|i| fixture.get(*i).is_some_and(|c| !c.is_empty()))
        .cloned()
        .collect();
    let step = (with_cands.len() / SAMPLE).max(1);
    let sample: Vec<String> = with_cands
        .iter()
        .step_by(step)
        .take(SAMPLE)
        .cloned()
        .collect();
    eprintln!(
        "sample size {} (step {step} over {} with candidates)",
        sample.len(),
        with_cands.len()
    );

    // Focused grid over the swept region. The full ±50% first pass and its
    // rankings are recorded in docs/findings/scoring-constant-sweep.md; the
    // winner it confirmed is the now-frozen default seg=750 inc=999 bonus=1000,
    // so the `base` baseline trial below is that frozen default rather than the
    // pre-freeze provisional.
    let base = ScoringConfig::default();
    let trials: Vec<(&str, ScoringConfig)> = {
        let mut v = Vec::new();
        v.push(("baseline", base));
        for &(seg, inc, bonus) in &[
            (500, 1000, 1500),
            (500, 1500, 2000),
            (750, 1000, 1500),
            (750, 1500, 2000),
            (750, 1000, 1200),
            (500, 1000, 1200),
            (500, 1100, 1500),
            (500, 1400, 1500),
            (625, 1000, 1500),
            (625, 1250, 1500),
            (750, 1250, 1500),
            (750, 1400, 1500),
            (500, 999, 1000),
            (750, 999, 1000),
            (1000, 1000, 1500),
            (1000, 1500, 2000),
        ] {
            let mut c = base;
            c.segmentation_penalty = seg;
            c.incomplete_penalty = inc;
            c.phrase_key_bonus = bonus;
            if c.incomplete_penalty >= c.phrase_key_bonus {
                continue;
            }
            v.push(("grid", c));
        }
        v
    };

    let mut best_top1 = 0_usize;
    let mut best_cfg = base;
    let mut best_label = "baseline";

    eprintln!(
        "{:<8} {:>6} {:>6} {:>6} {:>6}  {:>5} {:>5} {:>5}  detail",
        "kind", "seg", "inc", "bonus", "lm", "top1%", "top5%", "abs%"
    );
    for (label, cfg) in &trials {
        session.set_scoring_config(*cfg);
        let start = Instant::now();
        let rates = measure(&mut session, &sample, &fixture);
        let t1 = pct(rates.top1, rates.total);
        let t5 = pct(rates.top5, rates.total);
        let ab = pct(rates.absent, rates.total);
        eprintln!(
            "{:<8} {:>6} {:>6} {:>6} {:>6}  {:>5} {:>5} {:>5}  {}/{} in {:?}",
            label,
            cfg.segmentation_penalty,
            cfg.incomplete_penalty,
            cfg.phrase_key_bonus,
            cfg.lm_weight,
            t1,
            t5,
            ab,
            rates.top1,
            rates.total,
            start.elapsed()
        );
        if rates.top1 > best_top1 {
            best_top1 = rates.top1;
            best_cfg = *cfg;
            best_label = label;
        }
    }

    eprintln!(
        "best sample: {best_label} seg={} inc={} bonus={} top1={best_top1}/{}",
        best_cfg.segmentation_penalty,
        best_cfg.incomplete_penalty,
        best_cfg.phrase_key_bonus,
        sample.len()
    );

    if std::env::var_os("PARITY_SWEEP_FULL").is_some() {
        session.set_scoring_config(best_cfg);
        let inputs: Vec<String> = all_inputs
            .into_iter()
            .filter(|i| fixture.get(i).is_some_and(|c| !c.is_empty()))
            .collect();
        let start = Instant::now();
        let rates = measure(&mut session, &inputs, &fixture);
        eprintln!(
            "full corpus with best: top-1 {}% ({}/{}) top-5 {}% absent {} in {:?}",
            pct(rates.top1, rates.total),
            rates.top1,
            rates.total,
            pct(rates.top5, rates.total),
            rates.absent,
            start.elapsed()
        );
    }

    ExitCode::SUCCESS
}
