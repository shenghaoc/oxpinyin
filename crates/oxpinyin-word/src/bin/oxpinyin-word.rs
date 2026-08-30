//! `oxpinyin-word` — the word-recognition pipeline.
//!
//! Usage: oxpinyin-word recognize --words words.txt --oldwords oldwords.txt
//!                                [-o recognized.txt] {SEGMENTED}+
//!
//! Runs populate → partialword → newword → markpinyin over the segmented
//! documents and emits `recognized.txt` (`word\tpinyin\tfreq`).

#![forbid(unsafe_code)]
#![allow(missing_docs)]

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use oxpinyin_word::{parse_word_list, recognize};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let result = match command.as_str() {
        "-h" | "--help" | "help" => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        "recognize" => run_recognize(&args[1..]),
        other => Err(format!("unknown subcommand: {other}\n\n{USAGE}").into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("oxpinyin-word: {error}");
            ExitCode::from(1)
        }
    }
}

const USAGE: &str = "\
Usage: oxpinyin-word recognize --words words.txt --oldwords oldwords.txt \
[-o recognized.txt] {SEGMENTED}+

  words.txt     dictionary word list (one word per line)
  oldwords.txt  atomic pinyin list (word pinyin freq)
  SEGMENTED     segmented corpus documents
";

type Cli = Result<(), Box<dyn std::error::Error>>;

fn run_recognize(args: &[String]) -> Cli {
    let mut words: Option<PathBuf> = None;
    let mut oldwords: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut inputs: Vec<PathBuf> = Vec::new();

    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--words" => words = Some(PathBuf::from(iter.next().ok_or("missing --words")?)),
            "--oldwords" => {
                oldwords = Some(PathBuf::from(iter.next().ok_or("missing --oldwords")?));
            }
            "-o" | "--output" => {
                output = Some(PathBuf::from(iter.next().ok_or("missing -o")?));
            }
            other => inputs.push(PathBuf::from(other)),
        }
    }

    let words = words.ok_or("--words is required")?;
    let oldwords = oldwords.ok_or("--oldwords is required")?;
    if inputs.is_empty() {
        return Err("no segmented input files".into());
    }

    let dict_words = parse_word_list(&read(&words)?);
    let oldwords_text = read(&oldwords)?;
    let documents: Vec<String> = inputs.iter().map(|p| read(p)).collect::<Result<_, _>>()?;

    let recognized = recognize(&documents, &dict_words, &oldwords_text)?;

    match output {
        Some(path) => fs::write(&path, recognized.as_bytes())
            .map_err(|source| format!("cannot write {path:?}: {source}"))?,
        None => io::stdout().lock().write_all(recognized.as_bytes())?,
    }
    Ok(())
}

fn read(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    fs::read_to_string(path).map_err(|source| format!("cannot read {path:?}: {source}").into())
}
