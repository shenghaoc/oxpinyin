//! KMM semantic parity: a committed canonical golden over a non-trivial
//! fixture, plus an env-gated oracle differential against the pin binaries.
//!
//! The canonical semantic representation is the **export text** — it carries
//! every stored field (magic `count`/`N`/`total_freq`; per-row `count`/`freq`;
//! per-pair `count`/`T`/`N_n_0`/`n_1`/`Mr`), so a byte comparison of the
//! export is a field-by-field semantic comparison. The committed golden below
//! is hand-derived from the KMM arithmetic (see `docs/findings/
//! kmm-arithmetic-audit.md`) over a five-token, two-document fixture that
//! exercises: multi-occurrence pairs (`Mr`), rare pairs (`n_1`), cross-document
//! accumulation (`N_n_0`, `N`), candidate scoring and ordering, a selective
//! prune, and the interpolation projection. The `rust_kmm_matches_pin_*`
//! gates run the actual pin tools where they and the system data are present,
//! comparing the same canonical export byte-for-byte.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use oxpinyin_kmm::{
    DEFAULT_PRUNE_K, GenerateParams, KMixtureModel, estimate, export, import,
    kmm_text_to_interpolation, merge_into, prune,
};

// ---- the fixture ----------------------------------------------------------
//
// Candidate A: one document, five `甲 乙` sentences then one `丙 丁`.
// Candidate B: one document, three `甲 乙` sentences then one `丙 丁`.
// Tokens: 10 甲, 20 乙, 30 丙, 40 丁 (1 <start>). No token collides with
// sentence_start (1) or null_token (0).

const JIAYI: &str = "10 甲\n20 乙\n0 \n";

fn candidate_a() -> KMixtureModel {
    built(&format!(
        "{JIAYI}{JIAYI}{JIAYI}{JIAYI}{JIAYI}30 丙\n40 丁\n"
    ))
}

fn candidate_b() -> KMixtureModel {
    built(&format!("{JIAYI}{JIAYI}{JIAYI}30 丙\n40 丁\n"))
}

fn built(doc: &str) -> KMixtureModel {
    let mut model = KMixtureModel::new();
    model
        .add_document(doc, GenerateParams::default())
        .expect("generate");
    model
}

// ---- committed canonical goldens (hand-derived) ---------------------------

// A: 甲→乙 five times in one document → wc 5, n_1 0 (5≠1), Mr 5, N_n_0 1;
// <start>→甲 five times; the single 丙→丁 → wc 1, n_1 1, Mr 1. total_freq =
// 甲5 + 乙5 + 丙1 + 丁1 = 12; magic count = Σ row header count = 6+5+0+1+0 = 12.
const A_EXPORT: &str = "\
\\data model \"k mixture model\" count 12 N 1 total_freq 12
\\1-gram
\\item 1 <start> count 6 freq 0
\\item 10 甲 count 5 freq 5
\\item 20 乙 count 0 freq 5
\\item 30 丙 count 1 freq 1
\\item 40 丁 count 0 freq 1
\\2-gram
\\item 1 <start> 10 甲 count 5 T 5 N_n_0 1 n_1 0 Mr 5
\\item 1 <start> 30 丙 count 1 T 1 N_n_0 1 n_1 1 Mr 1
\\item 10 甲 20 乙 count 5 T 5 N_n_0 1 n_1 0 Mr 5
\\item 30 丙 40 丁 count 1 T 1 N_n_0 1 n_1 1 Mr 1
\\end
";

const B_EXPORT: &str = "\
\\data model \"k mixture model\" count 8 N 1 total_freq 8
\\1-gram
\\item 1 <start> count 4 freq 0
\\item 10 甲 count 3 freq 3
\\item 20 乙 count 0 freq 3
\\item 30 丙 count 1 freq 1
\\item 40 丁 count 0 freq 1
\\2-gram
\\item 1 <start> 10 甲 count 3 T 3 N_n_0 1 n_1 0 Mr 3
\\item 1 <start> 30 丙 count 1 T 1 N_n_0 1 n_1 1 Mr 1
\\item 10 甲 20 乙 count 3 T 3 N_n_0 1 n_1 0 Mr 3
\\item 30 丙 40 丁 count 1 T 1 N_n_0 1 n_1 1 Mr 1
\\end
";

// Merge A ← B: fields add, Mr maxes, N adds. 甲→乙 wc 5+3=8, N_n_0 1+1=2,
// n_1 0, Mr max(5,3)=5; 丙→丁 wc 1+1=2, N_n_0 2, n_1 1+1=2, Mr 1; N 2.
const MERGED_EXPORT: &str = "\
\\data model \"k mixture model\" count 20 N 2 total_freq 20
\\1-gram
\\item 1 <start> count 10 freq 0
\\item 10 甲 count 8 freq 8
\\item 20 乙 count 0 freq 8
\\item 30 丙 count 2 freq 2
\\item 40 丁 count 0 freq 2
\\2-gram
\\item 1 <start> 10 甲 count 8 T 8 N_n_0 2 n_1 0 Mr 5
\\item 1 <start> 30 丙 count 2 T 2 N_n_0 2 n_1 2 Mr 1
\\item 10 甲 20 乙 count 8 T 8 N_n_0 2 n_1 0 Mr 5
\\item 30 丙 40 丁 count 2 T 2 N_n_0 2 n_1 2 Mr 1
\\end
";

// Prune (k 3, CDF 0.5). 甲→乙 / <start>→甲 (count 8, N 2, n_0 0, n_1 0):
// B=8/2=4, remained = 1 − Pr(0)+Pr(1)+Pr(2) = 1 − (0 + 0 + 1/3) = 0.667 ≥ 0.5
// → KEEP. 丙→丁 / <start>→丙 (count 2, N 2, n_1 2): B special-case 2,
// Pr(1)=α(1−γ)=1·(1−0)=1 → remained 0 < 0.5 → PRUNE. The emptied 丙/丁 rows
// clean up.
const PRUNED_EXPORT: &str = "\
\\data model \"k mixture model\" count 16 N 2 total_freq 16
\\1-gram
\\item 1 <start> count 8 freq 0
\\item 10 甲 count 8 freq 8
\\item 20 乙 count 0 freq 8
\\2-gram
\\item 1 <start> 10 甲 count 8 T 8 N_n_0 2 n_1 0 Mr 5
\\item 10 甲 20 乙 count 8 T 8 N_n_0 2 n_1 0 Mr 5
\\end
";

// Interpolation from the pruned model: <start> and zero-freq unigrams drop;
// the 1-gram count is the KMM freq, the 2-gram count is the pair WC.
const INTERPOLATION: &str = "\
\\data model interpolation
\\1-gram
\\item 10 甲 count 8
\\item 20 乙 count 8
\\2-gram
\\item 1 <start> 10 甲 count 8
\\item 10 甲 20 乙 count 8
\\end
";

#[test]
fn canonical_export_headers_and_per_pair_fields_match_the_golden() {
    // Byte parity of the export == field parity of headers, document count,
    // word count, per-token freqs, and per-pair count/T/N_n_0/n_1/Mr.
    assert_eq!(export(&candidate_a()), A_EXPORT);
    assert_eq!(export(&candidate_b()), B_EXPORT);
}

#[test]
fn candidate_scores_and_ordering_are_stable() {
    let a = candidate_a();
    let b = candidate_b();
    let score_a = estimate(&a, &b).expect("score A vs B").average;
    let score_b = estimate(&b, &a).expect("score B vs A").average;
    // Both are well-formed λ in the unit interval; B scores above A, so the
    // sorted (descending) candidate order is [B, A].
    assert!((0.0..=1.0).contains(&score_a), "score A {score_a}");
    assert!((0.0..=1.0).contains(&score_b), "score B {score_b}");
    assert!(
        score_b > score_a,
        "candidate ordering: B ({score_b}) must rank above A ({score_a})"
    );
    // Pin at six decimals — the pin's `%f` precision (the estimate.py sort key).
    assert_eq!(format!("{score_a:.6}"), "0.999571");
    assert_eq!(format!("{score_b:.6}"), "0.999689");
}

#[test]
fn merge_prune_interpolation_match_the_golden() {
    let mut merged = KMixtureModel::new();
    merge_into(&mut merged, &candidate_a()).expect("merge A");
    merge_into(&mut merged, &candidate_b()).expect("merge B");
    assert_eq!(export(&merged), MERGED_EXPORT, "merge result");

    let mut pruned = merged.clone();
    prune(&mut pruned, DEFAULT_PRUNE_K, 0.5).expect("prune");
    assert_eq!(export(&pruned), PRUNED_EXPORT, "prune result");

    let interp = kmm_text_to_interpolation(&export(&pruned)).expect("to interpolation");
    assert_eq!(interp, INTERPOLATION, "interpolation output");
}

#[test]
fn export_import_is_an_exact_round_trip_on_the_fixture() {
    // The canonical form is a fixed point of import∘export, so the semantic
    // representation is lossless.
    for model in [candidate_a(), candidate_b()] {
        let text = export(&model);
        assert_eq!(import(&text).expect("import"), model);
        assert_eq!(export(&import(&text).expect("import")), text);
    }
}

// ---- env-gated oracle differential ----------------------------------------

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn real_segmented_fixture() -> PathBuf {
    repo_root().join("fixtures/w9/segmenter-spseg-w3.txt")
}

fn locate_bin(name: &str) -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var_os(name)?);
    path.is_file().then_some(path)
}

fn locate_data() -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var_os("PINYIN_GEN_NGRAM_DATA")?);
    path.join("table.conf").is_file().then_some(path)
}

/// A fresh working directory seeded with the raw `.table` + `table.conf`, in
/// which one pin command is run (never the stale `.db` outputs). Mirrors the
/// λ differential's `PinDir`.
struct PinDir {
    dir: PathBuf,
}

impl PinDir {
    fn fresh(data: &Path, tag: &str) -> Result<Self, String> {
        let dir = std::env::temp_dir().join(format!("oxpinyin-kmm-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        for entry in data.read_dir().map_err(|e| e.to_string())?.flatten() {
            if entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name == "table.conf" || name.ends_with(".table") {
                    std::fs::copy(entry.path(), dir.join(entry.file_name()))
                        .map_err(|e| e.to_string())?;
                }
            }
        }
        Ok(Self { dir })
    }

    fn run(&self, bin: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
        let output = Command::new(bin)
            .current_dir(&self.dir)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(format!(
                "{} exited {}: {}",
                bin.display(),
                output.status,
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        Ok(output.stdout)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }
}

impl Drop for PinDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The native model built from the same segmented corpus the pin consumes,
/// with its text column dropped so the comparison is on tokens + counts (the
/// pin resolves text from the phrase index; the fixture's inline text matches,
/// but normalising avoids any incidental text divergence).
fn native_export_tokens_only(segmented: &str) -> String {
    let mut model = KMixtureModel::new();
    model
        .add_document(segmented, GenerateParams::default())
        .expect("generate");
    strip_text(&export(&model))
}

/// Rewrites every phrase word to a placeholder so two exports compare on
/// tokens and counts alone. Words never contain spaces, so the field indices
/// are fixed.
fn strip_text(export_text: &str) -> String {
    let mut out = String::new();
    for line in export_text.lines() {
        if let Some(rest) = line.strip_prefix("\\item ") {
            let mut fields: Vec<&str> = rest.split(' ').collect();
            // 1-gram: TOKEN WORD count .. ; 2-gram: T1 W1 T2 W2 count ..
            if fields.get(2) == Some(&"count") {
                fields[1] = "_";
            } else {
                fields[1] = "_";
                fields[3] = "_";
            }
            out.push_str("\\item ");
            out.push_str(&fields.join(" "));
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

#[test]
fn rust_kmm_matches_pin_gen_and_export() {
    let (Some(gen_bin), Some(export_bin), Some(data)) = (
        locate_bin("PINYIN_GEN_KMM"),
        locate_bin("PINYIN_EXPORT_KMM"),
        locate_data(),
    ) else {
        eprintln!(
            "skipping live KMM gen/export differential: set PINYIN_GEN_KMM, \
             PINYIN_EXPORT_KMM, PINYIN_GEN_NGRAM_DATA"
        );
        return;
    };

    let segmented = std::fs::read_to_string(real_segmented_fixture()).expect("fixture");

    // Pin: gen the model, export it.
    let pin = PinDir::fresh(&data, "gen").expect("temp data dir");
    let seg_path = pin.path("corpus.segmented");
    std::fs::write(&seg_path, &segmented).expect("write segmented");
    let model_path = pin.path("model.db");
    pin.run(
        &gen_bin,
        &[
            "--k-mixture-model-file",
            model_path.to_str().unwrap(),
            seg_path.to_str().unwrap(),
        ],
    )
    .expect("pin gen");
    let pin_export = String::from_utf8(
        pin.run(
            &export_bin,
            &["--k-mixture-model-file", model_path.to_str().unwrap()],
        )
        .expect("pin export"),
    )
    .expect("utf8");

    // Compare on tokens + counts (the canonical semantic form, text-normalised).
    assert_eq!(
        native_export_tokens_only(&segmented),
        strip_text(&pin_export),
        "native gen+export must match the pin field-for-field"
    );
    eprintln!("live parity: native gen+export matches the pin on the real corpus");
}

#[test]
fn rust_kmm_matches_pin_to_interpolation() {
    let (Some(to_interp), Some(data)) = (
        locate_bin("PINYIN_KMM_TO_INTERP"),
        Some(()).and(locate_data()),
    ) else {
        eprintln!(
            "skipping live to-interpolation differential: set PINYIN_KMM_TO_INTERP, \
             PINYIN_GEN_NGRAM_DATA"
        );
        return;
    };

    // The pin `k_mixture_model_to_interpolation` reads KMM text on stdin. Feed
    // it our canonical merged golden and compare to the native transform.
    let pin = PinDir::fresh(&data, "interp").expect("temp data dir");
    let input = pin.path("kmm.text");
    std::fs::write(&input, MERGED_EXPORT).expect("write kmm");

    let output = Command::new(&to_interp)
        .current_dir(&pin.dir)
        .stdin(std::fs::File::open(&input).expect("open"))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("run to-interpolation");
    assert!(output.status.success(), "pin to-interpolation failed");
    let pin_interp = String::from_utf8(output.stdout).expect("utf8");

    let native = kmm_text_to_interpolation(MERGED_EXPORT).expect("native");
    assert_eq!(
        native, pin_interp,
        "native to-interpolation matches the pin"
    );
    eprintln!("live parity: native to-interpolation matches the pin");
}
