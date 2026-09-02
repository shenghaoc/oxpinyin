//! KMM semantic parity: a committed canonical golden over a non-trivial
//! fixture, plus an env-gated oracle differential against the pin binaries.
//!
//! The canonical semantic representation is the **export text** — it carries
//! every stored field (magic `count`/`N`/`total_freq`; per-row `count`/`freq`;
//! per-pair `count`/`T`/`N_n_0`/`n_1`/`Mr`). The committed goldens are the
//! native output over a five-token, two-document fixture that exercises:
//! multi-occurrence pairs (`Mr`), rare pairs (`n_1`), cross-document
//! accumulation (`N_n_0`, `N`), candidate scoring, a selective prune, the
//! interpolation projection, and the **token2-only rule** (a token that never
//! begins a pair gets no `\1-gram` header under the pin's Tkrzw backend — 乙
//! and 丁 below).
//!
//! Two behaviours were pinned by running the live oracle (libpinyin 2.11.91,
//! Tkrzw backend, model20 data), and the native side matches both:
//!
//! * **token2-only**: `set_array_header` no-ops on a token without a
//!   single_gram (`flexible_ngram_tkrzwdb.h:411`), so a W2-only token gets no
//!   array header — its freq counts in `total_freq` only. `oxpinyin-kmm`
//!   reproduces this (`generate.rs`).
//! * **ordering**: the pin serialises the export in Tkrzw hash-iteration order
//!   (its `get_all_items` is unordered); the native canonicalises to
//!   token-ascending. The KMM `.db` is an unordered DBM, so the record *set*
//!   is what carries meaning — the live gate compares sorted item sets, not
//!   bytes.
//!
//! The `rust_kmm_matches_pin_*` gates run the actual pin tools where they and
//! the built system data are present (`PINYIN_*` env), and skip cleanly
//! otherwise.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use oxpinyin_kmm::{
    DEFAULT_PRUNE_K, GenerateParams, KMixtureModel, estimate, export, import,
    kmm_text_to_interpolation, merge_into, prune, validate,
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
// 甲5 + 乙5 + 丙1 + 丁1 = 12; magic count = Σ row header count = 6+5+1 = 12.
//
// 乙 and 丁 are W2-only (they never begin a pair), so under the pin's Tkrzw
// backend they get NO `\1-gram` array header — their freq counts toward
// total_freq (12) only. This is the token2-only rule, oracle-verified below.
const A_EXPORT: &str = "\
\\data model \"k mixture model\" count 12 N 1 total_freq 12
\\1-gram
\\item 1 <start> count 6 freq 0
\\item 10 甲 count 5 freq 5
\\item 30 丙 count 1 freq 1
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
\\item 30 丙 count 1 freq 1
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
\\item 30 丙 count 2 freq 2
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
\\2-gram
\\item 1 <start> 10 甲 count 8 T 8 N_n_0 2 n_1 0 Mr 5
\\item 10 甲 20 乙 count 8 T 8 N_n_0 2 n_1 0 Mr 5
\\end
";

// Interpolation from the pruned model: <start> and zero-freq unigrams drop;
// the 1-gram count is the KMM freq, the 2-gram count is the pair WC. 乙 is
// W2-only so it has no 1-gram header to project (token2-only rule).
const INTERPOLATION: &str = "\
\\data model interpolation
\\1-gram
\\item 10 甲 count 8
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
fn candidate_scores_are_deterministic() {
    let a = candidate_a();
    let b = candidate_b();
    let score_a = estimate(&a, &b).expect("score A vs B").average;
    let score_b = estimate(&b, &a).expect("score B vs A").average;
    // Both are well-formed λ in the unit interval and deterministic. A and B
    // share the same structure (甲乙 repeated + one 丙丁, differing only in the
    // 甲乙 repeat count, which normalises out of the deleted-interpolation EM),
    // so they score identically — the `estimate.py` sort key is stable and a
    // tie keeps gather order (a stable sort). The sort/top-N of distinct
    // scores is unit-tested in `candidate.rs`.
    assert!((0.0..=1.0).contains(&score_a), "score A {score_a}");
    assert!((0.0..=1.0).contains(&score_b), "score B {score_b}");
    // Pin at six decimals — the pin's `%f` precision (the sort key).
    assert_eq!(format!("{score_a:.6}"), "0.999783");
    assert_eq!(format!("{score_b:.6}"), "0.999783");
    // Deterministic across repeated runs.
    assert_eq!(estimate(&a, &b).expect("re-score").average, score_a);
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

/// A fresh working directory seeded with the built system model, in which one
/// pin command is run (`gen_k_mixture_model` loads `SYSTEM_PHRASE_INDEX` and
/// `SYSTEM_TABLE_INFO` from the cwd, so the built `phrase_index.bin` /
/// `pinyin_index.bin` / `table.conf` must be present). `PINYIN_GEN_NGRAM_DATA`
/// points at the built data dir (`libpinyin/data` after `gen_binary_files` +
/// `import_interpolation`; see `docs/testing/oracle-environment.md`).
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
                // The built binary model the pin utils load from the cwd, plus
                // the raw tables/config (harmless to copy).
                if name == "table.conf"
                    || name == "phrase_index.bin"
                    || name == "pinyin_index.bin"
                    || name.ends_with(".table")
                    || name.ends_with(".bin")
                {
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

/// The `\item` lines of an export, sorted — the order-insensitive canonical
/// form. The pin serialises in Tkrzw hash-iteration order (its `get_all_items`
/// is unordered); the native walks token-ascending. The *set* of records is
/// what carries meaning (the KMM `.db` is an unordered DBM), so parity is a
/// set comparison, not a byte comparison — exactly the "canonical semantic
/// representation where byte comparison is inappropriate" the plan calls for.
fn sorted_items(export_text: &str) -> Vec<String> {
    let mut items: Vec<String> = export_text
        .lines()
        .filter(|l| l.starts_with("\\data ") || l.starts_with("\\item"))
        .map(str::to_owned)
        .collect();
    items.sort();
    items
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

    // Compare the record *set* (tokens + counts), order-insensitive: the pin's
    // Tkrzw `get_all_items` order is unordered, the native's is token-ascending,
    // and the KMM `.db` is an unordered DBM — so the set is what carries meaning.
    assert_eq!(
        sorted_items(&native_export_tokens_only(&segmented)),
        sorted_items(&strip_text(&pin_export)),
        "native gen+export must match the pin's record set field-for-field"
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

/// A *complete* segmented corpus for the prune differential. Every real
/// system token appears as a W1 (no W2-only tokens → the pin's prune
/// post-processing does not abort), and — crucially — pruning must not empty
/// any row, or the pin's export aborts reading a header-only entry
/// (`flexible_ngram_tkrzwdb.h:388`). So a high-count backbone
/// 中国→你好→世界→中国 (repeated, survives CDF 0.5) carries the model, and one
/// rare 中国→世界 (pruned) rides on 中国, whose backbone pair 中国→你好 keeps its
/// row non-empty. Real system tokens (from `spseg`): 中国 16817937, 你好
/// 16802309, 世界 16808451.
fn complete_corpus() -> String {
    let backbone = "16817937 中国\n16802309 你好\n16808451 世界\n16817937 中国\n0 \n";
    let rare = "16817937 中国\n16808451 世界\n0 \n";
    format!("{}{rare}", backbone.repeat(4))
}

/// Two candidate corpora: the real fixture split in half.
fn corpus_halves() -> (String, String) {
    let segmented = std::fs::read_to_string(real_segmented_fixture()).expect("fixture");
    let lines: Vec<&str> = segmented.lines().collect();
    let half = lines.len() / 2;
    let mut a = lines[..half].join("\n");
    a.push('\n');
    let mut b = lines[half..].join("\n");
    b.push('\n');
    (a, b)
}

#[test]
fn rust_kmm_matches_pin_merge() {
    let (Some(gen_bin), Some(export_bin), Some(merge_bin), Some(data)) = (
        locate_bin("PINYIN_GEN_KMM"),
        locate_bin("PINYIN_EXPORT_KMM"),
        locate_bin("PINYIN_MERGE_KMM"),
        locate_data(),
    ) else {
        eprintln!(
            "skipping live merge differential: set PINYIN_GEN_KMM, PINYIN_EXPORT_KMM, \
             PINYIN_MERGE_KMM, PINYIN_GEN_NGRAM_DATA"
        );
        return;
    };
    let (corpus_a, corpus_b) = corpus_halves();
    let pin = PinDir::fresh(&data, "merge").expect("temp data dir");
    let gen_one = |seg: &str, name: &str| {
        let seg_path = pin.path(&format!("{name}.seg"));
        std::fs::write(&seg_path, seg).expect("write seg");
        let db = pin.path(&format!("{name}.db"));
        pin.run(
            &gen_bin,
            &[
                "--k-mixture-model-file",
                db.to_str().unwrap(),
                seg_path.to_str().unwrap(),
            ],
        )
        .expect("pin gen");
        let export_text = String::from_utf8(
            pin.run(
                &export_bin,
                &["--k-mixture-model-file", db.to_str().unwrap()],
            )
            .expect("pin export"),
        )
        .expect("utf8");
        (db, export_text)
    };
    let (db_a, export_a) = gen_one(&corpus_a, "a");
    let (db_b, export_b) = gen_one(&corpus_b, "b");

    // Pin merge a.db + b.db → merged.db, export it.
    let merged_db = pin.path("merged.db");
    pin.run(
        &merge_bin,
        &[
            "--result-file",
            merged_db.to_str().unwrap(),
            db_a.to_str().unwrap(),
            db_b.to_str().unwrap(),
        ],
    )
    .expect("pin merge");
    let pin_merged = String::from_utf8(
        pin.run(
            &export_bin,
            &["--k-mixture-model-file", merged_db.to_str().unwrap()],
        )
        .expect("pin export merged"),
    )
    .expect("utf8");

    // Native: import both pin candidate exports, merge, export.
    let mut merged = import(&export_a).expect("import a");
    merge_into(&mut merged, &import(&export_b).expect("import b")).expect("merge");

    assert_eq!(
        sorted_items(&strip_text(&export(&merged))),
        sorted_items(&strip_text(&pin_merged)),
        "native merge must match the pin's merged record set"
    );
    eprintln!("live parity: native merge matches the pin");
}

#[test]
fn rust_kmm_matches_pin_prune() {
    let (Some(gen_bin), Some(export_bin), Some(prune_bin), Some(data)) = (
        locate_bin("PINYIN_GEN_KMM"),
        locate_bin("PINYIN_EXPORT_KMM"),
        locate_bin("PINYIN_PRUNE_KMM"),
        locate_data(),
    ) else {
        eprintln!(
            "skipping live prune differential: set PINYIN_GEN_KMM, PINYIN_EXPORT_KMM, \
             PINYIN_PRUNE_KMM, PINYIN_GEN_NGRAM_DATA"
        );
        return;
    };
    // A *complete* corpus (every token appears as a W1) — required because the
    // pin's prune post-processing asserts every pruned pair's W2 has an array
    // header (`prune_k_mixture_model.cpp:165`), which W2-only tokens lack: on a
    // model with W2-only tokens the pin **aborts** (SIGABRT), while the native
    // prune completes gracefully (its unigram-reduce no-ops on a missing row).
    // That is a class-(c) availability divergence — the native must not abort
    // on caller input — and it is exactly why the prune stage needs a complete
    // corpus to compare byte-for-byte. The chain 中国→你好→世界→中国 keeps every
    // token a W1; repeats give the pairs enough mass that CDF 0.5 keeps them.
    let segmented = complete_corpus();
    let pin = PinDir::fresh(&data, "prune").expect("temp data dir");
    let seg_path = pin.path("corpus.seg");
    std::fs::write(&seg_path, &segmented).expect("write seg");
    let db = pin.path("model.db");
    pin.run(
        &gen_bin,
        &[
            "--k-mixture-model-file",
            db.to_str().unwrap(),
            seg_path.to_str().unwrap(),
        ],
    )
    .expect("pin gen");
    // Native starts from the pin's candidate export (identical stored state).
    let candidate_export = String::from_utf8(
        pin.run(
            &export_bin,
            &["--k-mixture-model-file", db.to_str().unwrap()],
        )
        .expect("pin export"),
    )
    .expect("utf8");

    // Pin prune the .db in place (-k 3 --CDF 0.5), then export.
    pin.run(
        &prune_bin,
        &["-k", "3", "--CDF", "0.5", db.to_str().unwrap()],
    )
    .expect("pin prune");
    let pin_pruned = String::from_utf8(
        pin.run(
            &export_bin,
            &["--k-mixture-model-file", db.to_str().unwrap()],
        )
        .expect("pin export pruned"),
    )
    .expect("utf8");

    // Native prune the imported candidate with the same parameters.
    let mut model = import(&candidate_export).expect("import candidate");
    prune(&mut model, 3, 0.5).expect("native prune");

    assert_eq!(
        sorted_items(&strip_text(&export(&model))),
        sorted_items(&strip_text(&pin_pruned)),
        "native prune must match the pin's pruned record set"
    );
    eprintln!("live parity: native prune matches the pin");
}

#[test]
fn rust_kmm_matches_pin_validate() {
    let (Some(gen_bin), Some(export_bin), Some(validate_bin), Some(data)) = (
        locate_bin("PINYIN_GEN_KMM"),
        locate_bin("PINYIN_EXPORT_KMM"),
        locate_bin("PINYIN_VALIDATE_KMM"),
        locate_data(),
    ) else {
        eprintln!(
            "skipping live validate differential: set PINYIN_GEN_KMM, PINYIN_EXPORT_KMM, \
             PINYIN_VALIDATE_KMM, PINYIN_GEN_NGRAM_DATA"
        );
        return;
    };
    let segmented = std::fs::read_to_string(real_segmented_fixture()).expect("fixture");
    let pin = PinDir::fresh(&data, "validate").expect("temp data dir");
    let seg_path = pin.path("corpus.seg");
    std::fs::write(&seg_path, &segmented).expect("write seg");
    let db = pin.path("model.db");
    pin.run(
        &gen_bin,
        &[
            "--k-mixture-model-file",
            db.to_str().unwrap(),
            seg_path.to_str().unwrap(),
        ],
    )
    .expect("pin gen");
    let export_text = String::from_utf8(
        pin.run(
            &export_bin,
            &["--k-mixture-model-file", db.to_str().unwrap()],
        )
        .expect("pin export"),
    )
    .expect("utf8");

    // The pin validate on this small corpus (has W2-only tokens) rejects it.
    let pin_ok = Command::new(&validate_bin)
        .current_dir(&pin.dir)
        .arg(db.to_str().unwrap())
        .output()
        .expect("run validate")
        .status
        .success();
    let native_ok = validate(&import(&export_text).expect("import")).is_ok();
    assert_eq!(
        native_ok, pin_ok,
        "native validate verdict must match the pin's (both reject the W2-only model)"
    );
    eprintln!("live parity: native validate verdict matches the pin ({pin_ok})");
}
