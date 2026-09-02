//! `oxpinyin-kmm` — the K-mixture-model toolchain, one binary with the
//! subcommands the trainer pipeline drives (generate → estimate → merge →
//! validate → prune → export/import → to-interpolation).
//!
//! In this Rust re-expression the on-disk model file is the KMM text format
//! (upstream's `export_k_mixture_model` output), so the `.db` files of the
//! upstream pipeline are these text files. See the crate docs and
//! `docs/findings/trainer-parity-audit.md` §6.

#![forbid(unsafe_code)]
#![allow(missing_docs)]

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use oxpinyin_kmm::{
    DEFAULT_CDF, DEFAULT_PRUNE_K, GenerateParams, KMixtureModel, canonicalize, estimate, export,
    import, kmm_text_to_interpolation, merge_into, prune, validate,
};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let rest = &args[1..];
    let result = match command.as_str() {
        "-h" | "--help" | "help" => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        "generate" => run_generate(rest),
        "estimate" => run_estimate(rest),
        "merge" => run_merge(rest),
        "validate" => run_validate(rest),
        "prune" => run_prune(rest),
        "export" => run_export(rest),
        "import" => run_import(rest),
        "to-interpolation" => run_to_interpolation(rest),
        other => Err(format!("unknown subcommand: {other}\n\n{USAGE}").into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("oxpinyin-kmm: {error}");
            ExitCode::from(1)
        }
    }
}

const USAGE: &str = "\
Usage: oxpinyin-kmm <subcommand> [options]

Subcommands (model files are the KMM text format):
  generate [--maximum-occurs-allowed N] [--maximum-increase-rates-allowed F]
           [--skip-pi-gram-training] --k-mixture-model-file FILE {SEGMENTED}+
  estimate --bigram-file FILE --deleted-bigram-file FILE
  merge    --result-file FILE {SOURCE}+
  validate FILE
  prune    [-k N] [--CDF F] FILE
  export   --k-mixture-model-file FILE          (canonicalize to stdout)
  import   [--k-mixture-model-file FILE] [INPUT] (canonicalize; stdout if no file)
  to-interpolation [INPUT]                       (KMM text -> interpolation, stdout)
";

/// The CLI error monad: `Cli` is the unit form, `Cli<T>` returns a value.
type Cli<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn read_text(path: &Path) -> Cli<String> {
    fs::read_to_string(path).map_err(|source| format!("cannot read {path:?}: {source}").into())
}

fn read_stdin() -> Cli<String> {
    let mut buffer = String::new();
    io::stdin()
        .read_to_string(&mut buffer)
        .map_err(|source| format!("cannot read stdin: {source}"))?;
    Ok(buffer)
}

fn write_output(path: Option<&Path>, text: &str) -> Cli {
    match path {
        Some(path) => {
            fs::write(path, text.as_bytes())
                .map_err(|source| format!("cannot write {path:?}: {source}"))?;
        }
        None => io::stdout().lock().write_all(text.as_bytes())?,
    }
    Ok(())
}

fn load_model(path: &Path) -> Cli<KMixtureModel> {
    let text = read_text(path)?;
    import(&text).map_err(Into::into)
}

fn run_generate(args: &[String]) -> Cli {
    let mut params = GenerateParams::default();
    let mut model_file: Option<PathBuf> = None;
    let mut inputs: Vec<PathBuf> = Vec::new();

    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--maximum-occurs-allowed" => {
                params.max_occurs = next(&mut iter, "--maximum-occurs-allowed")?
                    .parse()
                    .map_err(|_| "invalid --maximum-occurs-allowed")?;
            }
            "--maximum-increase-rates-allowed" => {
                params.max_increase_rate = next(&mut iter, "--maximum-increase-rates-allowed")?
                    .parse()
                    .map_err(|_| "invalid --maximum-increase-rates-allowed")?;
            }
            "--skip-pi-gram-training" => params.train_pi_gram = false,
            "--k-mixture-model-file" => {
                model_file = Some(PathBuf::from(next(&mut iter, "--k-mixture-model-file")?));
            }
            other => inputs.push(PathBuf::from(other)),
        }
    }

    let model_file = model_file.ok_or("--k-mixture-model-file is required")?;
    if inputs.is_empty() {
        return Err("no segmented input files".into());
    }

    // Accumulate into the existing model file (upstream attaches
    // READWRITE|CREATE), one document per input file.
    let mut model = if model_file.is_file() {
        load_model(&model_file)?
    } else {
        KMixtureModel::new()
    };
    for input in &inputs {
        let text = read_text(input)?;
        model.add_document(&text, params)?;
    }
    write_output(Some(&model_file), &export(&model))
}

fn run_estimate(args: &[String]) -> Cli {
    let mut bigram: Option<PathBuf> = None;
    let mut deleted: Option<PathBuf> = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--bigram-file" => bigram = Some(PathBuf::from(next(&mut iter, "--bigram-file")?)),
            "--deleted-bigram-file" => {
                deleted = Some(PathBuf::from(next(&mut iter, "--deleted-bigram-file")?));
            }
            other => return Err(format!("unexpected argument: {other}").into()),
        }
    }
    let candidate = load_model(&bigram.ok_or("--bigram-file is required")?)?;
    let deleted = load_model(&deleted.ok_or("--deleted-bigram-file is required")?)?;
    let result = estimate(&candidate, &deleted)?;

    let mut out = String::new();
    for (token, lambda) in &result.per_token {
        out.push_str(&format!("token:{token} lambda:{}\n", printf_f(*lambda)));
    }
    out.push_str(&format!("average lambda:{}\n", printf_f(result.average)));
    io::stdout().lock().write_all(out.as_bytes())?;
    Ok(())
}

/// C `printf("%f")` rendering: six fixed decimals for a finite value, and
/// glibc's `nan`/`-nan` (sign bit honoured) for the NaN that
/// `estimate_k_mixture_model` prints when the deleted model has no scorable
/// context — Rust's own `{:.6}` would print `NaN`.
fn printf_f(value: f64) -> String {
    if value.is_nan() {
        if value.is_sign_negative() {
            "-nan"
        } else {
            "nan"
        }
        .to_owned()
    } else if value.is_infinite() {
        if value.is_sign_negative() {
            "-inf"
        } else {
            "inf"
        }
        .to_owned()
    } else {
        format!("{value:.6}")
    }
}

fn run_merge(args: &[String]) -> Cli {
    let mut result_file: Option<PathBuf> = None;
    let mut sources: Vec<PathBuf> = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--result-file" => result_file = Some(PathBuf::from(next(&mut iter, "--result-file")?)),
            other => sources.push(PathBuf::from(other)),
        }
    }
    let result_file = result_file.ok_or("--result-file is required")?;
    if sources.is_empty() {
        return Err("no source models to merge".into());
    }
    let mut target = if result_file.is_file() {
        load_model(&result_file)?
    } else {
        KMixtureModel::new()
    };
    for source in &sources {
        let new_one = load_model(source)?;
        merge_into(&mut target, &new_one)?;
    }
    write_output(Some(&result_file), &export(&target))
}

fn run_validate(args: &[String]) -> Cli {
    let [file] = args else {
        return Err("validate requires exactly one model file".into());
    };
    let model = load_model(Path::new(file))?;
    validate(&model)?;
    Ok(())
}

fn run_prune(args: &[String]) -> Cli {
    let mut prune_k = DEFAULT_PRUNE_K;
    let mut cdf = DEFAULT_CDF;
    let mut file: Option<PathBuf> = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-k" | "--pruneK" => {
                prune_k = next(&mut iter, "-k")?.parse().map_err(|_| "invalid -k")?;
            }
            "--CDF" => {
                cdf = next(&mut iter, "--CDF")?
                    .parse()
                    .map_err(|_| "invalid --CDF")?
            }
            other => {
                if file.replace(PathBuf::from(other)).is_some() {
                    return Err(format!("unexpected extra argument: {other}").into());
                }
            }
        }
    }
    let file = file.ok_or("prune requires a model file")?;
    let mut model = load_model(&file)?;
    prune(&mut model, prune_k, cdf)?;
    write_output(Some(&file), &export(&model))
}

fn run_export(args: &[String]) -> Cli {
    let mut file: Option<PathBuf> = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--k-mixture-model-file" => {
                file = Some(PathBuf::from(next(&mut iter, "--k-mixture-model-file")?));
            }
            other => return Err(format!("unexpected argument: {other}").into()),
        }
    }
    let text = read_text(&file.ok_or("--k-mixture-model-file is required")?)?;
    // canonicalize == import + export (byte-identical for export output).
    write_output(None, &canonicalize(&text)?)
}

fn run_import(args: &[String]) -> Cli {
    let mut model_file: Option<PathBuf> = None;
    let mut input: Option<PathBuf> = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--k-mixture-model-file" => {
                model_file = Some(PathBuf::from(next(&mut iter, "--k-mixture-model-file")?));
            }
            other => {
                if input.replace(PathBuf::from(other)).is_some() {
                    return Err(format!("unexpected extra argument: {other}").into());
                }
            }
        }
    }
    let text = match input {
        Some(path) => read_text(&path)?,
        None => read_stdin()?,
    };
    write_output(model_file.as_deref(), &canonicalize(&text)?)
}

fn run_to_interpolation(args: &[String]) -> Cli {
    let text = match args {
        [] => read_stdin()?,
        [path] => read_text(Path::new(path))?,
        [_, extra, ..] => return Err(format!("unexpected extra argument: {extra}").into()),
    };
    write_output(None, &kmm_text_to_interpolation(&text)?)
}

fn next<'a>(iter: &mut std::slice::Iter<'a, String>, flag: &str) -> Cli<&'a str> {
    iter.next()
        .map(String::as_str)
        .ok_or_else(|| format!("missing value for {flag}").into())
}

#[cfg(test)]
mod tests {
    use super::printf_f;

    #[test]
    fn printf_f_matches_glibc_for_nan_and_finite_values() {
        assert_eq!(printf_f(0.5), "0.500000");
        assert_eq!(printf_f(f64::NAN.copysign(1.0)), "nan");
        assert_eq!(printf_f(f64::NAN.copysign(-1.0)), "-nan");
        // The upstream `lambda_sum / lambda_count` with both zero.
        let zero: f64 = "0".parse().expect("zero");
        assert_eq!(printf_f(zero / zero).trim_start_matches('-'), "nan");
    }
}
