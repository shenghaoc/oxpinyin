//! Enumerate the W2 corpus tail — the residual inputs the pinned
//! `real_tables_session_reports_parity` measurement counts against pin
//! `0c5e80e`.
//!
//! The parity test reports the top-1, top-5, absent, and prefix-10 pins
//! but does not name the inputs that drive them. This binary reproduces
//! the same scoring path (real unigrams from `interpolation2.text`,
//! shared read-only tables) and prints every residual input by category:
//!
//! - `top1-miss`   — oracle #1 is not our #1;
//! - `top5-miss`   — oracle #1 is not in our top-5;
//! - `absent`      — oracle #1 does not appear in our candidate list at all;
//! - `order-only`  — same depth-10 set, different order (the tie-swap class).
//!
//! Read-only, single-threaded, no oracle FFI — the fixture already holds
//! the pin's answer for every corpus input, so no fresh oracle call is
//! made. Prints to stdout for capture into `docs/testing/corpus-tail.md`.
//!
//! ```bash
//! PINYIN_EXPORT_DIR=/tmp/oxpinyin-export \
//! PINYIN_MODEL_DIR=<extracted-model20> \
//! cargo run -p pinyin-oracle --release --bin corpus-tail
//! ```
//!
//! `--all-off-tails` enumerates instead the six W12 option-sweep TEXT-set
//! tails under the ALL-BITS-OFF (`0x0`) profile
//! (`docs/findings/all-off-tails.md`), against the same tables, fixture,
//! and scoring path as the corpus pass.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use oxpinyin_core::{OptionBits, PINYIN_CORRECT_GN_NG, PINYIN_CORRECT_MG_NG, PINYIN_CORRECT_UE_VE};
use oxpinyin_data::{BigramLanguageModel, SystemDictionary, default_store_file};
use oxpinyin_engine::{EmptyConfigSource, Session, StoragePaths};
use pinyin_oracle::corpus;

const CANDIDATES_FIXTURE: &str = "fixtures/w4/oracle-candidates.txt";
const CAPTURE_DEPTH: usize = 10;

/// The six all-off TEXT-set tail inputs of `docs/findings/option-bits.md`
/// "TEXT-set STOP triage", plus `ang` — the canonical key the `agn`/`amg`
/// correction aliases resolve to, carried so the alias rows can be diffed
/// against their canonical's all-off list in the same run.
const ALL_OFF_TAIL_INPUTS: [&str; 7] = ["agn", "amg", "ang", "cang", "lue", "lve", "sang"];

/// `(alias, correction bit, canonical)` for the correction-alias half of
/// the table: under the bit, the alias parses to the canonical's
/// `content_table` key, so the alias's cross-engine tail *is* the
/// canonical's all-off tail.
const CORRECTION_ALIASES: [(&str, u32, &str); 3] = [
    ("lue", PINYIN_CORRECT_UE_VE, "lve"),
    ("agn", PINYIN_CORRECT_GN_NG, "ang"),
    ("amg", PINYIN_CORRECT_MG_NG, "ang"),
];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn export_dir() -> Result<PathBuf, String> {
    let dir = std::env::var_os("PINYIN_EXPORT_DIR").map_or_else(
        || Path::new("/tmp/oxpinyin-export").to_path_buf(),
        PathBuf::from,
    );
    if ["pinyin_index", "phrase_index", "bigram"]
        .iter()
        .all(|stem| dir.join(default_store_file(stem)).exists())
    {
        Ok(dir)
    } else {
        Err(format!("exported tables not found at {}", dir.display()))
    }
}

fn load_fixture(path: &Path) -> Result<BTreeMap<String, Vec<String>>, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read fixture {}: {error}", path.display()))?;
    let mut by_input: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for line in raw.lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let mut parts = line.split('\t');
        let input = parts.next().unwrap_or("").to_owned();
        let _rank = parts.next();
        let cand = parts.next().unwrap_or("").to_owned();
        by_input.entry(input).or_default().push(cand);
    }
    Ok(by_input)
}

/// The shared read-only environment of both passes: exported tables,
/// real unigrams, a fresh session, and the frozen oracle fixture.
struct TailEnv {
    session: Session<SystemDictionary, BigramLanguageModel>,
    fixture: BTreeMap<String, Vec<String>>,
}

fn open_env() -> Result<TailEnv, String> {
    let dir = export_dir()?;
    let Ok(Some(model_dir)) = pinyin_oracle::model_cache::locate_model_dir() else {
        return Err(
            "model cache absent; set PINYIN_MODEL_DIR to an extracted model20 dir".to_owned(),
        );
    };

    let dict = SystemDictionary::open(
        &dir.join(default_store_file("pinyin_index")),
        &dir.join(default_store_file("phrase_index")),
    )
    .map_err(|error| format!("cannot open SystemDictionary: {error}"))?;
    let mut lm = BigramLanguageModel::open(&dir.join(default_store_file("bigram")))
        .map_err(|error| format!("cannot open BigramLanguageModel: {error}"))?;
    lm.set_unigrams_from_interpolation2(&model_dir.join("interpolation2.text"))
        .map_err(|error| format!("cannot parse interpolation2: {error}"))?;
    let session = Session::new(&EmptyConfigSource, StoragePaths::new("user"), dict, lm)
        .map_err(|error| format!("cannot create Session: {error}"))?;
    let fixture = load_fixture(&repo_root().join(CANDIDATES_FIXTURE))?;
    Ok(TailEnv { session, fixture })
}

/// One input's all-off observation: the selected parse, its filtered
/// length, and the full candidate list in rank order.
struct Snapshot {
    parse: String,
    parsed: usize,
    texts: Vec<String>,
}

fn snapshot(env: &mut TailEnv, input: &str, options: OptionBits) -> Result<Snapshot, String> {
    env.session.reset();
    env.session
        .set_options(options)
        .map_err(|error| format!("set_options on {input:?}: {error:?}"))?;
    env.session
        .type_pinyin(input)
        .map_err(|error| format!("type_pinyin {input:?}: {error:?}"))?;
    let keys = env
        .session
        .composition_keys()
        .map_err(|error| format!("composition_keys on {input:?}: {error:?}"))?;
    let parse = keys
        .iter()
        .map(|key| key.text().to_owned())
        .collect::<Vec<_>>()
        .join("'");
    let texts = env
        .session
        .candidates()
        .iter()
        .map(|candidate| candidate.text().to_owned())
        .collect();
    Ok(Snapshot {
        parse,
        parsed: env.session.parsed_prefix_len(),
        texts,
    })
}

/// The all-off (`0x0`) profile's answer for an input, per the frozen
/// oracle fixture: the fixture was captured at the parity word `0x18a`,
/// whose only extra parse-relevant bit is `PINYIN_INCOMPLETE`; for the
/// unflagged single-syllable entries these inputs parse to, the two words
/// admit the same key (`docs/findings/all-off-tails.md` §"Why the 0x18a
/// fixture row is the 0x0 answer").
fn fixture_top10<'a>(
    fixture: &'a BTreeMap<String, Vec<String>>,
    input: &str,
) -> Option<&'a [String]> {
    fixture
        .get(input)
        .filter(|list| !list.is_empty())
        .map(|list| &list[..list.len().min(CAPTURE_DEPTH)])
}

fn prefix_slice(texts: &[String]) -> &[String] {
    &texts[..texts.len().min(CAPTURE_DEPTH)]
}

/// Enumerates the six W12 all-off TEXT-set tails plus `ang` under the
/// `0x0` profile, diffs the fixture-backed rows against the pin fixture,
/// and checks the correction-alias and option-word-invariance claims the
/// triage in `docs/findings/option-bits.md` froze from the live-oracle
/// W10 control — this time with no oracle FFI at all.
fn run_all_off_tails(env: &mut TailEnv) -> Result<(), String> {
    let all_off = OptionBits::default();

    println!("== W12 all-off TEXT-set tails (option word 0x0) ==");
    let mut all_off_snapshots = BTreeMap::new();
    for input in ALL_OFF_TAIL_INPUTS {
        let snap = snapshot(env, input, all_off)?;
        println!(
            "input={input}\tparse={}\tparsed={}\tn={}",
            snap.parse,
            snap.parsed,
            snap.texts.len()
        );
        println!("  ours[:10]={:?}", prefix_slice(&snap.texts));
        match fixture_top10(&env.fixture, input) {
            Some(fixture) => {
                let ours = prefix_slice(&snap.texts);
                let shared = ours
                    .iter()
                    .zip(fixture.iter())
                    .take_while(|(ours, pin)| ours == pin)
                    .count();
                let pin_not_ours: Vec<&String> =
                    fixture.iter().filter(|t| !ours.contains(t)).collect();
                let ours_not_pin: Vec<&String> =
                    ours.iter().filter(|t| !fixture.contains(t)).collect();
                println!(
                    "  pin[:10]={fixture:?}\n  top1 ours={:?} pin={:?}\n  shared_prefix={shared}  pin_not_ours={}  ours_not_pin={}",
                    ours.first(),
                    fixture.first(),
                    pin_not_ours.len(),
                    ours_not_pin.len(),
                );
                if !pin_not_ours.is_empty() {
                    println!("  pin_not_ours={pin_not_ours:?}");
                }
                if !ours_not_pin.is_empty() {
                    println!("  ours_not_pin={ours_not_pin:?}");
                }
            }
            None => println!(
                "  fixture: no row ({input:?} is not a W2 corpus input); all-off cross-engine verdict is IDENTICAL per the frozen W10 control"
            ),
        }
        all_off_snapshots.insert(input.to_owned(), snap);
    }

    println!("\n== correction-alias equivalence (same engine, bit set) ==");
    for (alias, bit, canonical) in CORRECTION_ALIASES {
        let corrected = snapshot(env, alias, OptionBits::from_bits(bit))?;
        let native = all_off_snapshots
            .get(canonical)
            .ok_or_else(|| format!("canonical {canonical:?} missing from the all-off pass"))?;
        let verdict = if corrected.texts == native.texts {
            "EQUAL"
        } else {
            "DIFF"
        };
        println!(
            "{alias}+bit -> {canonical}\t{verdict}\tcorrected n={}  native n={}",
            corrected.texts.len(),
            native.texts.len()
        );
    }

    println!("\n== option-word invariance (ours, all-off vs parity word) ==");
    let parity_word = OptionBits::default().with(oxpinyin_core::PINYIN_INCOMPLETE, true);
    for input in ALL_OFF_TAIL_INPUTS {
        // Only the fixture-backed rows need this: the invariance is the
        // engine-side half of the argument that the 0x18a fixture row is
        // comparable to an all-off run. `agn`/`amg`/`lue` have no row, and
        // `agn`/`amg` legitimately widen under the parity word's
        // initial-only keys.
        if fixture_top10(&env.fixture, input).is_none() {
            continue;
        }
        let parity = snapshot(env, input, parity_word)?;
        let off = &all_off_snapshots
            .get(input)
            .ok_or_else(|| format!("{input:?} missing from the all-off pass"))?;
        let verdict = if parity.texts == off.texts {
            "INVARIANT"
        } else {
            "DIFFERS"
        };
        println!("{input}\t{verdict}\tn={}", parity.texts.len());
    }

    Ok(())
}

fn run(env: &mut TailEnv) -> Result<(), String> {
    let corpus_dir = repo_root().join(corpus::CORPUS_DIR);
    let mut inputs = Vec::new();
    for stratum in corpus::generate() {
        let path = corpus_dir.join(stratum.file_name);
        let bytes = std::fs::read(&path)
            .map_err(|error| format!("cannot read corpus stratum {}: {error}", path.display()))?;
        inputs.extend(corpus::Stratum::parse_file_bytes(&bytes));
    }

    let mut top1_miss = Vec::new();
    let mut top5_miss = Vec::new();
    let mut absent = Vec::new();
    let mut order_only_count = 0_usize;
    let mut prefix_gap_sum = 0_usize;
    let mut prefix_gap_denom = 0_usize;
    let mut total = 0_usize;

    for input in &inputs {
        let Some(oracle_cands) = env.fixture.get(input).filter(|l| !l.is_empty()) else {
            continue;
        };
        total += 1;
        env.session.reset();
        let _ = env.session.type_pinyin(input);
        let ours: Vec<String> = env
            .session
            .candidates()
            .iter()
            .map(|c| c.text().to_owned())
            .collect();
        let oracle_top = &oracle_cands[0];

        let our_prefix = &ours[..ours.len().min(CAPTURE_DEPTH)];
        let oracle_prefix = &oracle_cands[..oracle_cands.len().min(CAPTURE_DEPTH)];

        for cand in oracle_prefix {
            prefix_gap_denom += 1;
            if !our_prefix.contains(cand) {
                prefix_gap_sum += 1;
            }
        }

        let our_set: std::collections::HashSet<&str> =
            our_prefix.iter().map(String::as_str).collect();
        let oracle_set: std::collections::HashSet<&str> =
            oracle_prefix.iter().map(String::as_str).collect();
        if our_set == oracle_set && our_prefix != oracle_prefix {
            order_only_count += 1;
        }

        let our_rank = ours.iter().position(|t| t == oracle_top);
        if our_rank == Some(0) {
            continue;
        }

        let rank_display = our_rank.map_or("absent".to_owned(), |i| format!("#{}", i + 1));
        let our_top = ours.first().cloned().unwrap_or_else(|| "(none)".to_owned());
        let row = format!(
            "input={input:?}\toracle_top={oracle_top:?}\tour_top={our_top:?}\tour_rank={rank_display}\n\
             \tours[:5]={:?}\n\toracle[:5]={:?}",
            &our_prefix[..our_prefix.len().min(5)],
            &oracle_prefix[..oracle_prefix.len().min(5)],
        );

        top1_miss.push(row.clone());
        if !ours.iter().take(5).any(|t| t == oracle_top) {
            top5_miss.push(row.clone());
        }
        if !ours.iter().any(|t| t == oracle_top) {
            absent.push(row);
        }
    }

    println!("== W2 corpus tail residuals ==");
    println!("compared            {total}");
    println!("top-1 misses        {}", top1_miss.len());
    println!("top-5 misses        {}", top5_miss.len());
    println!("absent              {}", absent.len());
    println!("order-only          {order_only_count}");
    println!(
        "prefix-10 gap       {prefix_gap_sum} of {prefix_gap_denom} (positions not in our top-10)"
    );

    println!("\n== top-1 misses ({}) ==", top1_miss.len());
    for row in &top1_miss {
        println!("{row}");
    }

    println!("\n== top-5 misses ({}) ==", top5_miss.len());
    for row in &top5_miss {
        println!("{row}");
    }

    println!("\n== absent ({}) ==", absent.len());
    for row in &absent {
        println!("{row}");
    }

    Ok(())
}

fn main() -> ExitCode {
    let mut all_off_tails = false;
    for arg in std::env::args_os().skip(1) {
        match arg.to_str() {
            Some("--all-off-tails") => all_off_tails = true,
            Some(other) => {
                eprintln!("unknown argument {other:?}; expected --all-off-tails or nothing");
                return ExitCode::FAILURE;
            }
            None => {
                eprintln!("argument is not valid UTF-8; expected --all-off-tails or nothing");
                return ExitCode::FAILURE;
            }
        }
    }
    let outcome = match open_env() {
        Ok(mut env) => {
            if all_off_tails {
                run_all_off_tails(&mut env)
            } else {
                run(&mut env)
            }
        }
        Err(message) => Err(message),
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}
