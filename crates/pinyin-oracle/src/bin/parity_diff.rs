//! Runs the parity corpus against the live pinned oracle and writes both logs.
//!
//! W2-T4 is the human triage pass over the divergence log this produces.
//!
//! ```bash
//! bash tools/oracle/build-oracle.sh --prefix "$HOME/.local/opt/pinyin-oracle" --jobs "$(nproc)"
//!
//! PKG_CONFIG_PATH=$HOME/.local/opt/pinyin-oracle/lib/pkgconfig \
//! cargo run -p pinyin-oracle --features oracle-ffi --bin parity-diff -- target/parity
//! ```

use std::process::ExitCode;

fn main() -> ExitCode {
    #[cfg(not(feature = "oracle-ffi"))]
    {
        eprintln!("parity-diff needs the `oracle-ffi` feature and a pin-built oracle.");
        eprintln!(
            "  bash tools/oracle/build-oracle.sh --prefix \"$HOME/.local/opt/pinyin-oracle\""
        );
        eprintln!("  cargo run -p pinyin-oracle --features oracle-ffi --bin parity-diff");
        ExitCode::from(2)
    }

    #[cfg(feature = "oracle-ffi")]
    run()
}

#[cfg(feature = "oracle-ffi")]
fn run() -> ExitCode {
    use std::path::PathBuf;

    use pinyin_oracle::corpus;
    use pinyin_oracle::differential::{self, Case};
    use pinyin_oracle::live::LiveSource;
    use pinyin_oracle::{Oracle, OracleFlags, OraclePrefix, Taxonomy, taxonomy};

    let mut args = std::env::args_os().skip(1);
    let out_dir = args
        .next()
        .map_or_else(|| PathBuf::from("target/parity"), PathBuf::from);

    if args.next().is_some() {
        eprintln!("usage: parity-diff [OUTPUT_DIR]");
        return ExitCode::from(2);
    }

    let prefix = match OraclePrefix::locate() {
        Ok(prefix) => prefix,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };
    eprintln!("oracle  {}", prefix.pin().pin_ref());

    let mut oracle = match Oracle::open_with_temp_user_dir(prefix) {
        Ok(oracle) => oracle,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };

    let cases: Vec<Case> = corpus::generate()
        .into_iter()
        .flat_map(|stratum| {
            stratum
                .inputs
                .into_iter()
                .enumerate()
                .map(move |(index, input)| Case {
                    corpus: stratum.file_name.to_owned(),
                    case: format!("{}:{index}", stratum.file_name),
                    input: input.into_bytes(),
                })
        })
        .collect();
    eprintln!("corpus  {} inputs", cases.len());

    let session = match oracle.session(OracleFlags::DEFAULT) {
        Ok(session) => session,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };
    let mut source = LiveSource::new(session);
    let classifier = Taxonomy::new(OracleFlags::DEFAULT);
    let output = differential::run(&mut source, &cases, &classifier);
    let verdict = taxonomy::budget(output.report.total, &output.report.classes);

    if let Err(error) = std::fs::create_dir_all(&out_dir) {
        eprintln!("cannot create {}: {error}", out_dir.display());
        return ExitCode::FAILURE;
    }
    let summary = format!("{}\n{}", output.report.to_summary(), verdict.to_summary());
    for (name, text) in [
        ("comparisons.tsv", output.comparison_log_text()),
        ("divergences.tsv", output.divergence_log_text()),
        ("summary.txt", summary.clone()),
    ] {
        let path = out_dir.join(name);
        if let Err(error) = std::fs::write(&path, text) {
            eprintln!("cannot write {}: {error}", path.display());
            return ExitCode::FAILURE;
        }
        eprintln!("wrote   {}", path.display());
    }

    print!("{summary}");
    ExitCode::SUCCESS
}
