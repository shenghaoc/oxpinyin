//! Writes `fixtures/w4/oracle-candidates.txt` — the frozen candidate lists
//! for the full W2 parity corpus (10,465 inputs) at depth 10.
//!
//! This is the one-time oracle run the parity-climb plan refers to: the
//! live oracle is queried once, the result is committed, and every
//! subsequent `cargo test` loads the fixture instead of the oracle.
//!
//! ```bash
//! cargo run -p pinyin-oracle --features oracle-ffi --bin oracle-candidates -- fixtures/w4/oracle-candidates.txt
//! ```

use std::process::ExitCode;

fn main() -> ExitCode {
    #[cfg(not(feature = "oracle-ffi"))]
    {
        eprintln!("oracle-candidates needs the `oracle-ffi` feature and a pin-built oracle.");
        eprintln!(
            "  bash tools/oracle/build-oracle.sh --prefix \"$HOME/.local/opt/pinyin-oracle\""
        );
        eprintln!("  cargo run -p pinyin-oracle --features oracle-ffi --bin oracle-candidates");
        ExitCode::from(2)
    }
    #[cfg(feature = "oracle-ffi")]
    {
        use std::path::PathBuf;

        let mut args = std::env::args_os().skip(1);
        let out = args.next().map_or_else(
            || PathBuf::from("fixtures/w4/oracle-candidates.txt"),
            PathBuf::from,
        );

        if args.next().is_some() {
            eprintln!("usage: oracle-candidates [OUTPUT]");
            return ExitCode::from(2);
        }

        match run(&out) {
            Ok(summary) => {
                eprint!("{summary}");
                ExitCode::SUCCESS
            }
            Err(message) => {
                eprintln!("error: {message}");
                ExitCode::FAILURE
            }
        }
    }
}

#[cfg(feature = "oracle-ffi")]
fn run(out: &std::path::Path) -> Result<String, String> {
    use std::collections::BTreeMap;

    use pinyin_oracle::corpus;
    use pinyin_oracle::{Oracle, OracleFlags, OraclePrefix};

    const DEPTH: usize = 10;
    const SCHEMA: &str = "pinyin-oracle-candidates-v1";

    let prefix = OraclePrefix::locate().map_err(|e| format!("oracle prefix: {e}"))?;
    let pin_ref = prefix.pin().pin_ref().to_owned();
    eprintln!("oracle pin_ref={pin_ref}");

    let mut oracle =
        Oracle::open_with_temp_user_dir(prefix).map_err(|e| format!("oracle open: {e}"))?;
    let mut session = oracle
        .session(OracleFlags::DEFAULT)
        .map_err(|e| format!("session: {e}"))?;

    let strata = corpus::generate();
    let total_inputs: usize = strata.iter().map(|s| s.inputs.len()).sum();
    eprintln!("corpus total_inputs={total_inputs}");

    // Collect distinct inputs -> candidates, deduplicating corpus cross-strata
    // repeats. The W2 corpus has 10,465 entries but only 10,312 distinct strings;
    // duplicates are deterministic (oracle is pure), so one entry per distinct
    // input is sufficient and makes the fixture uniquely sorted by (input, rank).
    let mut by_input: BTreeMap<String, Vec<String>> = BTreeMap::new();

    let mut observed_inputs = 0_usize;
    let mut inputs_with_cands = 0_usize;

    for stratum in &strata {
        for input in &stratum.inputs {
            observed_inputs += 1;
            if by_input.contains_key(input) {
                continue;
            }
            let obs = session
                .observe(input.as_bytes())
                .map_err(|e| format!("observe {input:?}: {e}"))?;
            if !obs.candidates.is_empty() {
                inputs_with_cands += 1;
            }
            let cands = obs.candidates.into_iter().take(DEPTH).collect::<Vec<_>>();
            by_input.insert(input.clone(), cands);
        }
    }

    // Build sorted triples from the deduplicated map (BTreeMap is already sorted).
    let mut total_cands = 0_usize;
    let mut lines: Vec<String> = Vec::new();
    let total_distinct = by_input.len();
    let distinct_with_cands = by_input.values().filter(|v| !v.is_empty()).count();

    lines.push(format!("# {SCHEMA}"));
    lines.push(format!("# pin_ref={pin_ref}"));
    lines.push(format!("# corpus={}", corpus::CORPUS_DIR));
    lines.push(format!(
        "# total_inputs={observed_inputs} (distinct {total_distinct})"
    ));
    lines.push(format!("# depth={DEPTH}"));
    // total_triples is filled after the data pass.
    let triples_header_index = lines.len();
    lines.push(String::new());
    lines.push("# format: input<TAB>rank<TAB>candidate_text".to_owned());
    lines.push("# sorted by input (bytewise) then rank (ascending)".to_owned());
    lines.push("# one line per (input,rank,candidate) triple at depth 10".to_owned());
    lines.push(
        "# deduplicated by input: corpus cross-strata duplicates share the same oracle output"
            .to_owned(),
    );

    for (input, cands) in &by_input {
        for (idx, cand) in cands.iter().enumerate() {
            let rank = idx + 1;
            // input and cand contain no TAB or LF (corpus domain / candidate text).
            lines.push(format!("{input}\t{rank}\t{cand}"));
            total_cands += 1;
        }
    }
    lines[triples_header_index] = format!(
        "# total_triples={total_cands} (distinct inputs with candidates {distinct_with_cands})"
    );

    let text = lines.join("\n") + "\n";

    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    std::fs::write(out, text).map_err(|e| format!("write {}: {e}", out.display()))?;

    Ok(format!(
        "wrote {total_cands} triples for {observed_inputs} inputs ({inputs_with_cands} with candidates) to {}\n",
        out.display()
    ))
}
