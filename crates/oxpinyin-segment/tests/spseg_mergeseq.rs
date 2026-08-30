//! Differential parity for the fewest-words `spseg` and the phrase-merge
//! `mergeseq`, over the committed `fixtures/w3` phrase_index.
//!
//! Unlike the `ngseg` differential (which needs the full system-table
//! export and so skips on CI), `spseg` and `mergeseq` consult only the
//! phrase table, which is committed at `fixtures/w3/phrase_index.<backend>`.
//! So the golden compare runs **unconditionally** on CI against that table.
//! The goldens are therefore over the *partial* W3 dictionary — a real,
//! committed system table, but a subset — which is why some single Han
//! characters segment as unknown (`0 …`) runs.
//!
//! When a full pin-built export and the `spseg`/`mergeseq` binaries are
//! present (`PINYIN_SPSEG` / `PINYIN_MERGESEQ` + oracle data), the live
//! cross-checks additionally run Rust-vs-upstream on the same fixtures.

use std::path::{Path, PathBuf};
use std::process::Command;

use oxpinyin_segment::{PhraseLexicon, default_store_file, mergeseq, spseg};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn w3_lexicon() -> PhraseLexicon {
    let path = repo_root()
        .join("fixtures/w3")
        .join(default_store_file("phrase_index"));
    PhraseLexicon::from_phrase_index(&path)
        .unwrap_or_else(|error| panic!("committed w3 phrase_index must open: {error}"))
}

fn first_divergence(left: &str, right: &str) -> Option<String> {
    if left == right {
        return None;
    }
    for (index, (a, b)) in left.lines().zip(right.lines()).enumerate() {
        if a != b {
            return Some(format!(
                "line {} diverges\n  rust:  {a:?}\n  other: {b:?}",
                index + 1
            ));
        }
    }
    Some(format!(
        "length diverges: rust {} lines, other {} lines",
        left.lines().count(),
        right.lines().count()
    ))
}

/// Every golden line parses as an ngseg-grammar record (`0 `, `0 {raw}`,
/// or `{token} {text}`). Always on — no table needed.
fn assert_well_formed(golden: &str) {
    assert!(!golden.is_empty(), "golden must not be empty");
    for (index, line) in golden.lines().enumerate() {
        if line == "0 " {
            continue;
        }
        let Some((head, rest)) = line.split_once(' ') else {
            panic!("record {} has no space: {line:?}", index + 1);
        };
        let well_formed = !rest.is_empty() && (head == "0" || head.parse::<u32>().is_ok());
        assert!(well_formed, "record {} is malformed: {line:?}", index + 1);
    }
}

#[test]
fn rust_spseg_matches_w3_golden() {
    let lexicon = w3_lexicon();
    let input =
        std::fs::read(repo_root().join("fixtures/w9/segmenter-han.txt")).expect("han input");
    let rust = spseg::segment_bytes(&lexicon, &input, false);
    let golden = std::fs::read_to_string(repo_root().join("fixtures/w9/segmenter-spseg-w3.txt"))
        .expect("spseg golden");
    if let Some(diff) = first_divergence(&rust, &golden) {
        panic!("Rust spseg diverges from the committed w3 golden: {diff}");
    }
}

#[test]
fn rust_mergeseq_matches_w3_golden() {
    let lexicon = w3_lexicon();
    let input =
        std::fs::read(repo_root().join("fixtures/w9/mergeseq-input.txt")).expect("mergeseq input");
    let rust = lexicon_mergeseq(&lexicon, &input);
    let golden = std::fs::read_to_string(repo_root().join("fixtures/w9/segmenter-mergeseq-w3.txt"))
        .expect("mergeseq golden");
    if let Some(diff) = first_divergence(&rust, &golden) {
        panic!("Rust mergeseq diverges from the committed w3 golden: {diff}");
    }
}

fn lexicon_mergeseq(lexicon: &PhraseLexicon, input: &[u8]) -> String {
    mergeseq::merge_bytes(lexicon, input).expect("mergeseq over the committed w3 fixture")
}

#[test]
fn committed_goldens_are_well_formed() {
    for name in ["segmenter-spseg-w3.txt", "segmenter-mergeseq-w3.txt"] {
        let golden = std::fs::read_to_string(repo_root().join("fixtures/w9").join(name))
            .unwrap_or_else(|_| panic!("{name} must be committed"));
        assert_well_formed(&golden);
    }
}

// --- env-gated live oracle cross-checks (skip on CI) ------------------------

fn locate_binary(var: &str) -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var_os(var)?);
    path.is_file().then_some(path)
}

fn locate_oracle_data() -> Option<PathBuf> {
    if let Some(raw) = std::env::var_os("PINYIN_NGSEG_DATA") {
        let path = PathBuf::from(raw);
        return (path.join("table.conf").is_file()).then_some(path);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    let candidate = home.join(".local/opt/pinyin-oracle/lib/libpinyin/data");
    candidate.join("table.conf").is_file().then_some(candidate)
}

fn run_binary(binary: &Path, data: &Path, input: &Path) -> Result<String, String> {
    let output = Command::new(binary)
        .current_dir(data)
        .arg(input)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "{} exited {}: {}",
            binary.display(),
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    String::from_utf8(output.stdout).map_err(|error| error.to_string())
}

#[test]
fn rust_matches_live_spseg_when_present() {
    let (Some(binary), Some(data)) = (locate_binary("PINYIN_SPSEG"), locate_oracle_data()) else {
        eprintln!("skipping live spseg: set PINYIN_SPSEG and the oracle data dir");
        return;
    };
    // Live spseg needs the full pin data dir; the Rust side must load the
    // same phrase table via the export dir. This path only runs with the
    // full oracle present, so we reuse the oracle prefix's export if set.
    let Some(export) = oxpinyin_segment::locate_export_dir() else {
        eprintln!("skipping live spseg: no full export for the Rust side");
        return;
    };
    let lexicon =
        PhraseLexicon::from_phrase_index(&export.join(default_store_file("phrase_index")))
            .expect("export phrase_index opens");
    let input_path = repo_root().join("fixtures/w9/segmenter-han.txt");
    let input = std::fs::read(&input_path).expect("han input");
    let rust = spseg::segment_bytes(&lexicon, &input, false);
    let live = run_binary(&binary, &data, &input_path).expect("spseg runs");
    if let Some(diff) = first_divergence(&rust, &live) {
        panic!("Rust spseg diverges from live spseg: {diff}");
    }
}

#[test]
fn rust_matches_live_mergeseq_when_present() {
    let (Some(binary), Some(data)) = (locate_binary("PINYIN_MERGESEQ"), locate_oracle_data())
    else {
        eprintln!("skipping live mergeseq: set PINYIN_MERGESEQ and the oracle data dir");
        return;
    };
    let Some(export) = oxpinyin_segment::locate_export_dir() else {
        eprintln!("skipping live mergeseq: no full export for the Rust side");
        return;
    };
    let lexicon =
        PhraseLexicon::from_phrase_index(&export.join(default_store_file("phrase_index")))
            .expect("export phrase_index opens");
    // mergeseq consumes a segmented stream: use the committed full-dict
    // ngseg golden as the shared input.
    let input_path = repo_root().join("fixtures/w9/segmenter-ngseg.txt");
    let input = std::fs::read(&input_path).expect("ngseg golden");
    let rust = mergeseq::merge_bytes(&lexicon, &input).expect("mergeseq runs");
    let live = run_binary(&binary, &data, &input_path).expect("mergeseq runs");
    if let Some(diff) = first_divergence(&rust, &live) {
        panic!("Rust mergeseq diverges from live mergeseq: {diff}");
    }
}
