//! Sample the worst-ranked parity failures for analysis.
//!
//! ```bash
//! cargo run -p pinyin-oracle --release --bin parity-worst
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use oxpinyin_data::{BigramLanguageModel, SystemDbm, SystemDictionary};
use oxpinyin_engine::{EmptyConfigSource, Session, StoragePaths};
use pinyin_oracle::corpus;

const CANDIDATES_FIXTURE: &str = "fixtures/w4/oracle-candidates.txt";
const WORST_N: usize = 50;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

#[derive(Clone)]
struct Row {
    input: String,
    our_rank: Option<usize>, // 1-based; None = absent
    oracle_top: String,
    our_top: Option<String>,
    our_list: Vec<String>,
    oracle_list: Vec<String>,
}

fn main() -> ExitCode {
    let dir = Path::new("/tmp/oxpinyin-export");
    if !dir.join(SystemDbm::PinyinIndex.file_name()).exists() {
        eprintln!("missing export tables");
        return ExitCode::from(2);
    }
    let dict = SystemDictionary::open(dir).expect("dict");
    let mut lm = BigramLanguageModel::open(
        &dir.join(SystemDbm::Bigram.file_name()),
        std::sync::Arc::clone(dict.libraries()),
    )
    .expect("lm");
    lm.set_lambda_from_table_conf(&dir.join("table.conf"));
    let mut session =
        Session::new(&EmptyConfigSource, StoragePaths::new("user"), dict, lm).expect("session");

    let raw = std::fs::read_to_string(repo_root().join(CANDIDATES_FIXTURE)).expect("fixture");
    let mut fixture: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for line in raw.lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let mut p = line.split('\t');
        let input = p.next().unwrap_or("").to_owned();
        let _rank = p.next();
        let cand = p.next().unwrap_or("").to_owned();
        fixture.entry(input).or_default().push(cand);
    }

    let corpus_dir = repo_root().join(corpus::CORPUS_DIR);
    let mut inputs = Vec::new();
    for stratum in corpus::generate() {
        let bytes = std::fs::read(corpus_dir.join(stratum.file_name)).expect("stratum");
        inputs.extend(corpus::Stratum::parse_file_bytes(&bytes));
    }

    let mut failures: Vec<Row> = Vec::new();
    let mut n = 0_usize;
    for input in &inputs {
        let Some(oracle) = fixture.get(input).filter(|c| !c.is_empty()) else {
            continue;
        };
        n += 1;
        session.reset();
        let _ = session.type_pinyin(input);
        let ours: Vec<String> = session
            .candidates()
            .iter()
            .map(|c| c.text().to_owned())
            .collect();
        let oracle_top = oracle[0].clone();
        if ours.first() == Some(&oracle_top) {
            continue;
        }
        let our_rank = ours.iter().position(|t| t == &oracle_top).map(|i| i + 1);
        failures.push(Row {
            input: input.clone(),
            our_rank,
            oracle_top,
            our_top: ours.first().cloned(),
            our_list: ours,
            oracle_list: oracle.clone(),
        });
    }

    // Sort: absent last-rank first, then rank descending (worst first).
    failures.sort_by(|a, b| {
        let ar = a.our_rank.unwrap_or(10_000);
        let br = b.our_rank.unwrap_or(10_000);
        br.cmp(&ar).then_with(|| a.input.cmp(&b.input))
    });

    eprintln!("compared {n}; top-1 misses {}", failures.len());
    eprintln!("--- worst {WORST_N} (oracle #1 is our #10+ or absent) ---");

    let mut cat_wrong_scoring = 0_usize;
    let mut cat_missing = 0_usize;
    let mut cat_prefix_only = 0_usize;
    let mut cat_seg = 0_usize;
    let mut cat_other = 0_usize;

    for (i, row) in failures.iter().take(WORST_N).enumerate() {
        let rank_s = row
            .our_rank
            .map(|r| format!("#{r}"))
            .unwrap_or_else(|| "absent".to_owned());
        let our_top = row.our_top.as_deref().unwrap_or("(none)");
        // Categorise
        let category = if row.our_rank.is_none() {
            // Is oracle top reachable as a dict phrase for some segmentation?
            // Heuristic: if any of our candidates share a prefix of oracle_top, incomplete gap;
            // if our list is empty-ish fallback, missing.
            if row.our_list.is_empty()
                || row.our_list.iter().any(|t| t == &row.input || t.is_ascii())
            {
                cat_missing += 1;
                "missing-candidate / fallback"
            } else if row
                .our_list
                .iter()
                .any(|t| row.oracle_top.starts_with(t.as_str()) && t != &row.oracle_top)
            {
                cat_prefix_only += 1;
                "prefix-only (longer phrase missing or outranked)"
            } else {
                cat_missing += 1;
                "missing-candidate"
            }
        } else if row.our_rank.unwrap_or(0) >= 6 {
            // present but badly ranked
            let our_set: BTreeSet<&str> = row.our_list.iter().map(String::as_str).collect();
            let oracle_set: BTreeSet<&str> = row.oracle_list.iter().map(String::as_str).collect();
            let overlap = our_set.intersection(&oracle_set).count();
            if overlap >= 3 {
                cat_wrong_scoring += 1;
                "wrong-scoring (candidate present, shared set large)"
            } else if row
                .our_top
                .as_ref()
                .is_some_and(|t| t.chars().count() != row.oracle_top.chars().count())
            {
                cat_seg += 1;
                "wrong-segmentation / length preference"
            } else {
                cat_wrong_scoring += 1;
                "wrong-scoring"
            }
        } else {
            cat_other += 1;
            "near-miss (rank 2-5)"
        };

        eprintln!(
            "{:>2}. input={:?}  oracle_top={:?}  our_top={:?}  our_rank={}  cat={}",
            i + 1,
            row.input,
            row.oracle_top,
            our_top,
            rank_s,
            category
        );
        eprintln!(
            "    ours[:5]={:?}  oracle[:5]={:?}",
            &row.our_list[..row.our_list.len().min(5)],
            &row.oracle_list[..row.oracle_list.len().min(5)]
        );
    }

    // Full-miss taxonomy over all top-1 misses
    let mut all_absent = 0_usize;
    let mut all_rank_ge10 = 0_usize;
    let mut all_rank_6_9 = 0_usize;
    let mut all_rank_2_5 = 0_usize;
    for row in &failures {
        match row.our_rank {
            None => all_absent += 1,
            Some(r) if r >= 10 => all_rank_ge10 += 1,
            Some(r) if r >= 6 => all_rank_6_9 += 1,
            Some(_) => all_rank_2_5 += 1,
        }
    }
    eprintln!("--- all top-1 misses ({}) ---", failures.len());
    eprintln!("  rank 2-5     {all_rank_2_5}");
    eprintln!("  rank 6-9     {all_rank_6_9}");
    eprintln!("  rank 10+     {all_rank_ge10}");
    eprintln!("  absent       {all_absent}");
    eprintln!("--- worst-{WORST_N} category counts ---");
    eprintln!("  wrong-scoring              {cat_wrong_scoring}");
    eprintln!("  missing-candidate          {cat_missing}");
    eprintln!("  prefix-only                {cat_prefix_only}");
    eprintln!("  wrong-segmentation/length  {cat_seg}");
    eprintln!("  near-miss/other            {cat_other}");

    ExitCode::SUCCESS
}
