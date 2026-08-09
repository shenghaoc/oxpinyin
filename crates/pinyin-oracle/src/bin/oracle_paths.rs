//! Recovers the oracle's selected segmentation for every parity-corpus input.
//!
//! The W2-T3 live run already measured all 10,465 inputs; it wrote the answer
//! in two halves. `divergences.tsv` spells the oracle's path out verbatim for
//! the 485 inputs whose path we did not enumerate. `comparisons.tsv` records,
//! for the other 9,974, the `rank` of the oracle's path inside our ordered
//! path set — and our parser is frozen and deterministic, so path `rank` *is*
//! the oracle's path.
//!
//! Putting the two halves together gives a portable fixture: the pin's answer
//! for the whole corpus, with no oracle build and no Linux requirement. That
//! is what lets W4-T1 and W4-T4 check themselves against the pin in ordinary
//! CI.
//!
//! ```bash
//! cargo run -p pinyin-oracle --bin oracle-paths -- target/parity fixtures/w4/oracle-paths.txt
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use pinyin_core::{Completeness, FullPinyinParser, InputParser, ParseResult};
use pinyin_oracle::capture::{escape, unescape};
use pinyin_oracle::{EXPECTED_PIN_REF, OracleFlags};

/// Schema tag of the recovered-path fixture.
const SCHEMA: &str = "pinyin-oracle-path-v1";

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let run_dir = args
        .next()
        .map_or_else(|| PathBuf::from("target/parity"), PathBuf::from);
    let out = args.next().map_or_else(
        || PathBuf::from("fixtures/w4/oracle-paths.txt"),
        PathBuf::from,
    );

    match run(&run_dir, &out) {
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

fn run(run_dir: &Path, out: &Path) -> Result<String, String> {
    let comparisons = read(run_dir.join("comparisons.tsv"))?;
    let divergences = read(run_dir.join("divergences.tsv"))?;

    // input -> the oracle's path, spelled out by the divergence log.
    let mut spelled: BTreeMap<Vec<u8>, String> = BTreeMap::new();
    for line in divergences.lines() {
        let record = fields(line);
        let (Some(input), Some(path)) = (record.get("input"), record.get("theirs_path")) else {
            continue;
        };
        check_pin(&record)?;
        if record.get("reason").copied() == Some("oracle-sentinel") {
            continue;
        }
        spelled.insert(unescape(input), (*path).to_owned());
    }

    let mut lines = vec![
        format!("# {SCHEMA}"),
        format!("# pin_ref={EXPECTED_PIN_REF}"),
        format!("# flags={}", OracleFlags::DEFAULT),
        "# Recovered from a W2-T3 live parity run: theirs_path from".to_owned(),
        "# divergences.tsv, and our frozen parser's path at the recorded rank".to_owned(),
        "# from comparisons.tsv.  See docs/findings/segment-graph.md.".to_owned(),
        "# Fields: input, consumed, path (pinyin-capture-v1 segment notation).".to_owned(),
    ];

    let mut from_rank = 0_usize;
    let mut from_log = 0_usize;
    let mut sentinels = 0_usize;
    let mut total = 0_usize;

    for line in comparisons.lines() {
        let record = fields(line);
        let Some(raw_input) = record.get("input") else {
            continue;
        };
        check_pin(&record)?;
        total += 1;

        let input = unescape(raw_input);
        let consumed = record
            .get("theirs_consumed")
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or_else(|| format!("record for {raw_input:?} has no theirs_consumed"))?;

        let path = if let Some(path) = spelled.get(&input) {
            from_log += 1;
            path.clone()
        } else if let Some(rank) = record
            .get("rank")
            .and_then(|value| value.parse::<usize>().ok())
        {
            from_rank += 1;
            let paths = FullPinyinParser
                .parse(&input)
                .map_err(|error| format!("our parser refused {raw_input:?}: {error}"))?;
            let path = paths
                .get(rank)
                .ok_or_else(|| format!("{raw_input:?} has no path at rank {rank}"))?;
            render(path)
        } else {
            // A divergence with no usable path: the oracle-sentinel shape.
            sentinels += 1;
            continue;
        };

        lines.push(format!("{}\t{consumed}\t{path}", escape(&input)));
    }

    let text = format!("{}\n", lines.join("\n"));
    std::fs::write(out, text)
        .map_err(|error| format!("cannot write {}: {error}", out.display()))?;

    Ok(format!(
        "compared    {total}\nfrom rank   {from_rank}\nfrom log    {from_log}\nsentinels   {sentinels}\nwrote       {}\n",
        out.display()
    ))
}

fn read(path: PathBuf) -> Result<String, String> {
    std::fs::read_to_string(&path).map_err(|error| {
        format!(
            "cannot read {}: {error}\nrun parity-diff first (see the module docs)",
            path.display()
        )
    })
}

fn fields(line: &str) -> BTreeMap<&str, &str> {
    line.split('\t')
        .filter_map(|field| field.split_once('='))
        .collect()
}

fn check_pin(record: &BTreeMap<&str, &str>) -> Result<(), String> {
    match record.get("pin_ref") {
        Some(pin) if *pin == EXPECTED_PIN_REF => Ok(()),
        Some(pin) => Err(format!("record is off-pin: {pin}")),
        None => Err("record has no pin_ref".to_owned()),
    }
}

/// Renders one of our parser's paths in `pinyin-capture-v1` segment notation.
fn render(path: &ParseResult) -> String {
    if path.syllables.is_empty() {
        return "-".to_owned();
    }
    path.syllables
        .iter()
        .map(|segment| {
            format!(
                "{}@{}:{}:{}",
                escape(segment.syllable.as_bytes()),
                segment.start,
                segment.end,
                match segment.completeness {
                    Completeness::Complete => "complete",
                    Completeness::Partial => "partial",
                }
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}
