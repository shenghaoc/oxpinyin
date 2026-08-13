//! Writes `fixtures/w4/oracle-candidate-structure.txt` — the sibling of
//! `oracle-candidates.txt` that additionally records, per candidate, the two
//! fields the pinned public API exports: `pinyin_get_candidate_type` and
//! `pinyin_get_candidate_nbest_index`. This is W2-CAND
//! (`docs/findings/candidate-construction.md` §1.6).
//!
//! The `(input, rank, candidate_text)` columns are byte-identical to the sister
//! fixture by construction — both come from the same `collect_candidates` under
//! the same `0x1e` sort — so the two files are line-comparable. Nothing else
//! about a candidate (segmentation, tokens, offsets, score) is reachable from
//! the public header and none is recorded.
//!
//! ```bash
//! cargo run -p pinyin-oracle --features oracle-ffi --bin oracle-candidate-structure -- fixtures/w4/oracle-candidate-structure.txt
//! ```

use std::process::ExitCode;

fn main() -> ExitCode {
    #[cfg(not(feature = "oracle-ffi"))]
    {
        eprintln!(
            "oracle-candidate-structure needs the `oracle-ffi` feature and a pin-built oracle."
        );
        eprintln!(
            "  bash tools/oracle/build-oracle.sh --prefix \"$HOME/.local/opt/pinyin-oracle\""
        );
        eprintln!(
            "  cargo run -p pinyin-oracle --features oracle-ffi --bin oracle-candidate-structure"
        );
        ExitCode::from(2)
    }
    #[cfg(feature = "oracle-ffi")]
    {
        use std::path::PathBuf;

        let mut args = std::env::args_os().skip(1);
        let out = args.next().map_or_else(
            || PathBuf::from("fixtures/w4/oracle-candidate-structure.txt"),
            PathBuf::from,
        );

        if args.next().is_some() {
            eprintln!("usage: oracle-candidate-structure [OUTPUT]");
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
    use std::time::Instant;

    use pinyin_oracle::corpus;
    use pinyin_oracle::{CandidateInfo, Oracle, OracleFlags, OraclePrefix};

    const DEPTH: usize = 10;
    const SCHEMA: &str = "pinyin-oracle-candidate-structure-v1";

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

    // Collect distinct inputs -> candidate infos, deduplicating cross-strata
    // repeats exactly as `oracle_candidates.rs` does, so the two fixtures share
    // the same input set and ordering.
    let mut by_input: BTreeMap<String, Vec<CandidateInfo>> = BTreeMap::new();

    let mut observed_inputs = 0_usize;
    let mut inputs_with_cands = 0_usize;
    let started = Instant::now();

    for stratum in &strata {
        for input in &stratum.inputs {
            observed_inputs += 1;
            if by_input.contains_key(input) {
                continue;
            }
            let infos = session
                .observe_candidate_infos(input.as_bytes())
                .map_err(|e| format!("observe {input:?}: {e}"))?;
            if !infos.is_empty() {
                inputs_with_cands += 1;
            }
            let infos = infos.into_iter().take(DEPTH).collect::<Vec<_>>();
            by_input.insert(input.clone(), infos);

            if by_input.len().is_multiple_of(500) {
                eprintln!(
                    "  {} distinct observed ({} of {} corpus entries seen) in {:?}",
                    by_input.len(),
                    observed_inputs,
                    total_inputs,
                    started.elapsed()
                );
            }
        }
    }

    let mut total_triples = 0_usize;
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
    lines.push("# format: input<TAB>rank<TAB>candidate_text<TAB>type<TAB>nbest_index".to_owned());
    lines.push("# type is the public lookup_candidate_type_t name".to_owned());
    lines.push(
        "# nbest_index is the guint8 index for NBEST_MATCH_CANDIDATE, else - (accessor asserts that type)"
            .to_owned(),
    );
    lines.push("# sorted by input (bytewise) then rank (ascending)".to_owned());
    lines.push("# one line per (input,rank,candidate) triple at depth 10".to_owned());
    lines.push(
        "# deduplicated by input: corpus cross-strata duplicates share the same oracle output"
            .to_owned(),
    );
    lines.push(
        "# (input,rank,candidate_text) columns match fixtures/w4/oracle-candidates.txt".to_owned(),
    );

    for (input, infos) in &by_input {
        for (idx, info) in infos.iter().enumerate() {
            let rank = idx + 1;
            let nbest = info
                .nbest_index
                .map_or_else(|| "-".to_owned(), |index| index.to_string());
            // input and text carry no TAB or LF (corpus domain / candidate text).
            lines.push(format!(
                "{input}\t{rank}\t{}\t{}\t{nbest}",
                info.text,
                info.candidate_type.as_wire()
            ));
            total_triples += 1;
        }
    }
    lines[triples_header_index] = format!(
        "# total_triples={total_triples} (distinct inputs with candidates {distinct_with_cands})"
    );

    let text = lines.join("\n") + "\n";

    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    std::fs::write(out, text).map_err(|e| format!("write {}: {e}", out.display()))?;

    Ok(format!(
        "wrote {total_triples} triples for {observed_inputs} inputs ({inputs_with_cands} with candidates) to {} in {:?}\n",
        out.display(),
        started.elapsed()
    ))
}
