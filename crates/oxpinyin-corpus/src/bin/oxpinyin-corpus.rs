//! Corpus cleaner CLI: zhwiki XML dump → raw Han text for
//! `oxpinyin-segment`.
//!
//! Usage: oxpinyin-corpus [OPTIONS] [inputfile]
//!
//! The input is a decompressed MediaWiki XML dump (`Special:Export` or a
//! `dumps.wikimedia.org` pages-articles slice after `bzip2 -dc`); `-`
//! or no argument reads stdin. Output is one sentence per line, `\n`
//! only — the exact input format `oxpinyin-segment` reads.

#![allow(missing_docs)]

use std::fs::File;
use std::io::{self, BufReader};
use std::path::PathBuf;
use std::process::ExitCode;

use oxpinyin_corpus::{Converter, Options, clean};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut max_pages: Option<usize> = None;
    let mut output: Option<PathBuf> = None;
    let mut input: Option<PathBuf> = None;
    let mut opencc: Option<PathBuf> = None;
    let mut config = oxpinyin_corpus::DEFAULT_CONFIG.to_owned();
    let mut no_convert = false;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print_help();
                return Ok(());
            }
            "--pages" => {
                let raw = args.next().ok_or("missing --pages argument")?;
                max_pages = Some(raw.parse().map_err(|_| "invalid --pages")?);
            }
            "-o" | "--outputfile" => {
                output = Some(PathBuf::from(args.next().ok_or("missing -o argument")?));
            }
            "--opencc" => {
                opencc = Some(PathBuf::from(
                    args.next().ok_or("missing --opencc argument")?,
                ));
            }
            "--opencc-config" => {
                config = args.next().ok_or("missing --opencc-config argument")?;
            }
            "--no-convert" => no_convert = true,
            flag if flag.starts_with('-') => {
                return Err(format!("unknown option: {flag}").into());
            }
            path => {
                if input.is_some() {
                    return Err("too many arguments".into());
                }
                input = Some(PathBuf::from(path));
            }
        }
    }

    let options = Options {
        max_pages,
        converter: if no_convert {
            None
        } else {
            Some(
                Converter::new(opencc.unwrap_or_else(|| PathBuf::from("opencc")))
                    .with_config(config),
            )
        },
    };

    let stats = match (&input, &output) {
        (Some(path), Some(out_path)) if path.as_os_str() != "-" => {
            let file = File::open(path)?;
            let out = File::create(out_path)?;
            clean(BufReader::new(file), out, &options)?
        }
        (Some(path), None) if path.as_os_str() != "-" => {
            let file = File::open(path)?;
            clean(BufReader::new(file), io::stdout().lock(), &options)?
        }
        (_, Some(out_path)) => {
            let out = File::create(out_path)?;
            clean(BufReader::new(io::stdin().lock()), out, &options)?
        }
        _ => clean(
            BufReader::new(io::stdin().lock()),
            io::stdout().lock(),
            &options,
        )?,
    };

    eprintln!(
        "pages {}/{} kept, {} sentences, {} bytes",
        stats.pages_read - stats.pages_skipped,
        stats.pages_read,
        stats.sentences,
        stats.bytes_written
    );
    Ok(())
}

fn print_help() {
    print!(
        "Usage: oxpinyin-corpus [OPTIONS] [inputfile]\n\
         \n\
         Cleans a zhwiki XML dump into line-oriented raw Han text for\n\
         oxpinyin-segment. Input must be decompressed XML; '-' or no argument\n\
         reads stdin.\n\
         \n\
           --pages N          stop after N pages (bounded sample)\n\
           --opencc BIN       trad->simp converter binary (default: opencc)\n\
           --opencc-config C  opencc config name (default: t2s)\n\
           --no-convert       skip trad->simp conversion\n\
           -o, --outputfile FILE\n"
    );
}
