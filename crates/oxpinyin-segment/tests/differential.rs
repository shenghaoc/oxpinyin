//! Differential parity: Rust segmenter vs pin-built `ngseg`.
//!
//! Skips when the migrate export or the model20 cache is absent, matching
//! the oracle integration tests. When both are present, the fixture must
//! match the committed `ngseg` golden. When `PINYIN_NGSEG` (or a known
//! pin-build path) is present, the golden is also checked against a live
//! `ngseg` run so it cannot go stale silently.

use std::path::{Path, PathBuf};
use std::process::Command;

use oxpinyin_segment::{
    PINNED_LAMBDA, Segmenter, SegmenterPaths, locate_export_dir, locate_model_dir,
};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn fixture_input() -> PathBuf {
    repo_root().join("fixtures/w9/segmenter-han.txt")
}

fn fixture_golden() -> PathBuf {
    repo_root().join("fixtures/w9/segmenter-ngseg.txt")
}

fn open_segmenter() -> Option<Segmenter> {
    let export = locate_export_dir()?;
    let model = locate_model_dir()?;
    let paths = SegmenterPaths::from_dirs(&export, &model);
    match Segmenter::open(&paths, PINNED_LAMBDA) {
        Ok(segmenter) => Some(segmenter),
        Err(error) => {
            eprintln!("segmenter open failed: {error}");
            None
        }
    }
}

fn locate_ngseg() -> Option<PathBuf> {
    let raw = std::env::var_os("PINYIN_NGSEG")?;
    let path = PathBuf::from(raw);
    path.is_file().then_some(path)
}

fn locate_ngseg_data() -> Option<PathBuf> {
    if let Some(raw) = std::env::var_os("PINYIN_NGSEG_DATA") {
        let path = PathBuf::from(raw);
        if path.join("table.conf").is_file() && path.join("bigram.db").is_file() {
            return Some(path);
        }
        return None;
    }
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    let candidate = home.join(".local/opt/pinyin-oracle/lib/libpinyin/data");
    candidate.join("table.conf").is_file().then_some(candidate)
}

fn run_ngseg(ngseg: &Path, data: &Path, input: &Path) -> Result<String, String> {
    let output = Command::new(ngseg)
        .current_dir(data)
        .arg(input)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "ngseg exited {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    String::from_utf8(output.stdout).map_err(|error| error.to_string())
}

fn first_divergence(left: &str, right: &str) -> Option<String> {
    if left == right {
        return None;
    }
    let left_lines: Vec<&str> = left.split_inclusive('\n').collect();
    let right_lines: Vec<&str> = right.split_inclusive('\n').collect();
    for (index, (a, b)) in left_lines.iter().zip(right_lines.iter()).enumerate() {
        if a != b {
            return Some(format!(
                "line {} diverges\n  rust: {a:?}\n  ngseg: {b:?}",
                index + 1
            ));
        }
    }
    Some(format!(
        "length diverges: rust {} lines, other {} lines",
        left_lines.len(),
        right_lines.len()
    ))
}

/// Not env-gated: the differential tests in this file all skip without the
/// export dir or model20 cache, so this smoke test keeps the committed golden
/// on CI's critical path. It parses every record against the `ngseg` output
/// grammar (`0 \n`, `0 {raw}\n`, `{token} {text}\n`) without needing a model.
#[test]
fn committed_golden_is_well_formed() {
    let golden_path = fixture_golden();
    assert!(
        golden_path.is_file(),
        "{} must be committed",
        golden_path.display()
    );
    let golden = std::fs::read_to_string(&golden_path).expect("golden is utf-8");
    assert!(!golden.is_empty(), "committed golden must not be empty");
    let mut records = 0_usize;
    for (index, line) in golden.split_inclusive('\n').enumerate() {
        let line = line.strip_suffix('\n').unwrap_or(line);
        records += 1;
        if line == "0 " {
            continue;
        }
        let Some((head, rest)) = line.split_once(' ') else {
            panic!("record {} has no space: {line:?}", index + 1);
        };
        let well_formed = !rest.is_empty() && (head == "0" || head.parse::<u32>().is_ok());
        assert!(
            well_formed,
            "record {} is not an ngseg record: {line:?}",
            index + 1
        );
    }
    eprintln!("golden smoke: {records} records parse as ngseg output");
}

#[test]
fn rust_matches_committed_ngseg_golden() {
    let Some(segmenter) = open_segmenter() else {
        eprintln!(
            "skipping: migrate export or model20 cache not found \
             (PINYIN_EXPORT_DIR / PINYIN_MODEL_DIR)"
        );
        return;
    };
    let input = std::fs::read(fixture_input()).expect("fixture input");
    let rust = segmenter
        .segment_bytes(&input, false)
        .expect("segmenter cannot fail on the fixture");
    let golden_path = fixture_golden();
    if !golden_path.is_file() {
        eprintln!(
            "skipping golden compare: {} is not committed yet",
            golden_path.display()
        );
        eprintln!("--- rust output ({} bytes) ---", rust.len());
        eprint!("{rust}");
        return;
    }
    let golden = std::fs::read_to_string(&golden_path).expect("golden");
    if let Some(diff) = first_divergence(&rust, &golden) {
        panic!("Rust segmenter diverges from committed ngseg golden: {diff}");
    }
    let input_lines = std::str::from_utf8(&input)
        .expect("fixture is utf-8")
        .split_inclusive('\n')
        .count();
    let token_lines = rust.lines().count();
    eprintln!(
        "parity: {input_lines} input lines → {token_lines} output lines, bit-identical to golden"
    );
}

#[test]
fn committed_golden_matches_live_ngseg() {
    let Some(ngseg) = locate_ngseg() else {
        eprintln!("skipping live ngseg: PINYIN_NGSEG not set and pin-build ngseg not found");
        return;
    };
    let Some(data) = locate_ngseg_data() else {
        eprintln!("skipping live ngseg: PINYIN_NGSEG_DATA / oracle prefix data not found");
        return;
    };
    let golden_path = fixture_golden();
    if !golden_path.is_file() {
        eprintln!("skipping live ngseg: golden not committed");
        return;
    }
    let live = run_ngseg(&ngseg, &data, &fixture_input()).expect("ngseg runs");
    let golden = std::fs::read_to_string(&golden_path).expect("golden");
    if let Some(diff) = first_divergence(&live, &golden) {
        panic!("committed golden is stale versus live ngseg: {diff}");
    }
}

#[test]
fn rust_matches_live_ngseg_when_both_exist() {
    let Some(segmenter) = open_segmenter() else {
        eprintln!("skipping live rust-vs-ngseg: tables not found");
        return;
    };
    let Some(ngseg) = locate_ngseg() else {
        eprintln!("skipping live rust-vs-ngseg: ngseg not found");
        return;
    };
    let Some(data) = locate_ngseg_data() else {
        eprintln!("skipping live rust-vs-ngseg: ngseg data not found");
        return;
    };
    let input_path = fixture_input();
    let input = std::fs::read(&input_path).expect("fixture input");
    let rust = segmenter
        .segment_bytes(&input, false)
        .expect("segmenter cannot fail");
    let live = run_ngseg(&ngseg, &data, &input_path).expect("ngseg runs");
    if let Some(diff) = first_divergence(&rust, &live) {
        panic!("Rust segmenter diverges from live ngseg: {diff}");
    }
    let input_lines = std::str::from_utf8(&input).expect("utf-8").lines().count();
    eprintln!("live parity: {input_lines} fixture lines, token sequences bit-identical to ngseg");
}
