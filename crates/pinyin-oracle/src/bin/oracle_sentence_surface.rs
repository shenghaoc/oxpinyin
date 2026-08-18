//! Capture the pinned oracle's sentence surface over a deterministic W2
//! corpus sample — the W14 fixture generator.
//!
//! Writes `fixtures/w4/oracle-sentence-surface.txt`: one line per input,
//! the ibus call order (`pinyin_guess_sentence` → `pinyin_guess_candidates`
//! → `pinyin_get_sentence`), with the decoded sentences the candidate list
//! proves and the first candidates' `(type, nbest, text)` rows. Regenerate
//! with the pinned oracle when the pin changes:
//!
//! ```text
//! cargo run -p pinyin-oracle --features oracle-ffi --bin oracle-sentence-surface
//! ```

// Only the feature-on build formats strings; ungated, the import would be
// unused and CI's `-D warnings` would fail the default build.
#[cfg(feature = "oracle-ffi")]
use std::fmt::Write as _;

fn main() {
    #[cfg(not(feature = "oracle-ffi"))]
    {
        eprintln!("rebuild with --features oracle-ffi to capture the oracle fixture");
        std::process::exit(2);
    }
    #[cfg(feature = "oracle-ffi")]
    {
        match real_main() {
            Ok(summary) => {
                eprint!("{summary}");
            }
            Err(message) => {
                eprintln!("error: {message}");
                std::process::exit(1);
            }
        }
    }
}

/// Sample size: every `stride`-th corpus input, deterministic.
#[cfg(feature = "oracle-ffi")]
const SAMPLE: usize = 500;

/// Anchor inputs the differential asserts exactly (the #97 evidence).
#[cfg(feature = "oracle-ffi")]
const ANCHORS: [&str; 4] = ["nihao", "nihaoshijie", "cecenihao", "zhongguoren"];

#[cfg(feature = "oracle-ffi")]
fn real_main() -> Result<String, String> {
    use pinyin_oracle::{EXPECTED_PIN_REF, Oracle, OracleFlags, OraclePrefix, corpus};

    let corpus_dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/parity/corpus/inputs"
    );
    let mut inputs: Vec<String> = Vec::new();
    for stratum in corpus::generate() {
        let path = format!("{corpus_dir}/{}", stratum.file_name);
        let bytes = std::fs::read(&path).map_err(|error| format!("cannot read {path}: {error}"))?;
        inputs.extend(corpus::Stratum::parse_file_bytes(&bytes));
    }

    let stride = (inputs.len() / SAMPLE).max(1);
    let mut sample: Vec<String> = inputs
        .iter()
        .step_by(stride)
        .take(SAMPLE)
        .cloned()
        .chain(anchors())
        .collect();
    sample.dedup();
    let sample_len = sample.len();

    let prefix = OraclePrefix::locate().map_err(|error| error.to_string())?;
    let mut oracle = Oracle::open_with_temp_user_dir(prefix).map_err(|error| error.to_string())?;
    let mut session = oracle
        .session(OracleFlags::DEFAULT)
        .map_err(|error| error.to_string())?;

    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/w4/oracle-sentence-surface.txt"
    );
    let mut out = String::new();
    let _ = writeln!(out, "# pin_ref={EXPECTED_PIN_REF}");
    let _ = writeln!(out, "# sample={sample_len}");

    for input in &sample {
        let surface = session
            .observe_sentence_surface(input.as_bytes(), 2)
            .map_err(|error| format!("oracle observe failed for {input:?}: {error}"))?;
        let sentences: Vec<String> = surface
            .sentences
            .iter()
            .map(|sentence| sentence.as_deref().unwrap_or("-").to_owned())
            .collect();
        let rows: Vec<String> = surface.candidates.iter().take(6).map(render_row).collect();
        let _ = writeln!(
            out,
            "{}\t{}\t{}\t{}",
            input,
            surface.guessed,
            sentences.join("\u{1}"),
            rows.join("\u{1}")
        );
    }

    std::fs::write(path, out).map_err(|error| format!("cannot write {path}: {error}"))?;
    Ok(format!("wrote {path}: {sample_len} inputs\n"))
}

/// One candidate rendered in the fixture's row format
/// `type-letter/nbest/text`.
#[cfg(feature = "oracle-ffi")]
fn render_row(info: &pinyin_oracle::CandidateInfo) -> String {
    use pinyin_oracle::observation::OracleCandidateType;

    let nbest = info
        .nbest_index
        .map_or_else(|| "-".to_owned(), |value| value.to_string());
    let kind = match info.candidate_type {
        OracleCandidateType::NbestMatch => "n",
        OracleCandidateType::Normal => "N",
        OracleCandidateType::Addon => "a",
        _ => "?",
    };
    format!("{kind}/{nbest}/{}", info.text)
}

/// The anchors as an iterator, so `chain` stays lazy.
#[cfg(feature = "oracle-ffi")]
fn anchors() -> impl Iterator<Item = String> {
    ANCHORS.iter().map(|anchor| (*anchor).to_owned())
}
