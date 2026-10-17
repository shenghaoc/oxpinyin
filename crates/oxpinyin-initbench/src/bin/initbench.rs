//! `initbench`: how long libpinyin and libzhuyin take to become usable.
//!
//! Subcommands (see `docs/runbooks/initbench.md`):
//!
//! - `run`: verify equivalence, then time, then write the result stream.
//! - `report`: turn a result stream into the Markdown report.
//! - `profiles`: build the user-profile fixtures with the pin and prove each was
//!   loaded.
//! - `instrument`: per-phase Callgrind instruction counts or DHAT allocation
//!   totals (Linux and Valgrind only; elsewhere it refuses).
//! - `child`, `verify-child`, `probe-child`, `persist-child`: one process of one
//!   kind; started by `run`, usable by hand under a profiler.
//!
//! Libraries are loaded by path with `libloading` (module `ffi`); nothing here
//! links libpinyin. Measurement only: no library's behaviour is changed.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Duration;

use oxpinyin_initbench::BenchError;
use oxpinyin_initbench::child::{self, ChildOptions};
use oxpinyin_initbench::ffi::Native;
use oxpinyin_initbench::fixtures::{
    self, FIXTURE_FREQUENCY, Level, REGISTERED_PHRASE_INPUT_SHA256,
};
use oxpinyin_initbench::instrument::{parse_callgrind_ir, parse_dhat_summary};
use oxpinyin_initbench::memory::host_probe;
use oxpinyin_initbench::observe::Observation;
use oxpinyin_initbench::orchestrate::{
    CacheState, Plan, ProbeLines, Record, SubjectSpec, run, run_process,
};
use oxpinyin_initbench::profile::{
    Fixture, MANIFEST_FILE, Manifest, USER_SUBDIR, WITNESS_FILE, copy_tree,
};
use oxpinyin_initbench::report::{RatioSpec, ReportSpec, render};
use oxpinyin_initbench::sample::{SampleKind, StopPoint};
use oxpinyin_initbench::workload::{Bind, Facade};

const USAGE: &str = "\
usage: initbench <subcommand> [options]

  run        --subject LABEL=PINYIN_SO (repeat; the first is the reference)
             [--zhuyin-subject LABEL=ZHUYIN_SO (repeat)] --system-dir DIR
             --profile DIR (repeat) --out FILE [--facade pinyin|zhuyin|both]
             [--rounds N (0 = verification only)] [--warmup N]
             [--kind timing|memory] [--cache warm|cold] [--cpu N]
             [--bind now|lazy] [--work-dir DIR] [--max-skew-ms N]
             [--max-load X] [--timeout-s N]
  report     --input FILE --reference LABEL [--ratio NUM/DEN] (repeat)
  profiles   --out DIR --pin-lib SO --system-dir DIR [--work-dir DIR]
  instrument --lib SO --facade pinyin|zhuyin --system-dir DIR --profile DIR
             --tool callgrind|dhat [--cpu N] [--work-dir DIR]
  child|verify-child|probe-child|persist-child
             --facade F --lib SO --system-dir DIR --user-dir DIR [--bind now|lazy]
             [--kind timing|memory] [--stop-after POINT] [--profile NAME]
             [--witness-file FILE] [--input TEXT]

POINT is one of: start lib_load init config alloc first_candidate end
";

struct Args {
    values: BTreeMap<String, Vec<String>>,
}

impl Args {
    fn parse(raw: &[String]) -> Result<Self, BenchError> {
        let mut values: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut iter = raw.iter();
        while let Some(arg) = iter.next() {
            let Some(name) = arg.strip_prefix("--") else {
                return Err(BenchError::invalid(format!("unexpected argument {arg:?}")));
            };
            let value = iter
                .next()
                .ok_or_else(|| BenchError::invalid(format!("--{name} needs a value")))?;
            values
                .entry(name.to_owned())
                .or_default()
                .push(value.clone());
        }
        Ok(Self { values })
    }

    fn one(&self, name: &str) -> Result<Option<&str>, BenchError> {
        match self.values.get(name).map(Vec::as_slice) {
            None => Ok(None),
            Some([v]) => Ok(Some(v)),
            Some(_) => Err(BenchError::invalid(format!(
                "--{name} was given more than once"
            ))),
        }
    }

    fn require(&self, name: &str) -> Result<&str, BenchError> {
        self.one(name)?
            .ok_or_else(|| BenchError::invalid(format!("--{name} is required")))
    }

    fn many(&self, name: &str) -> &[String] {
        self.values.get(name).map_or(&[], Vec::as_slice)
    }

    fn number<T: std::str::FromStr>(&self, name: &str, default: T) -> Result<T, BenchError> {
        match self.one(name)? {
            None => Ok(default),
            Some(text) => text
                .parse()
                .map_err(|_| BenchError::invalid(format!("--{name} {text:?} is not a number"))),
        }
    }

    fn reject_unknown(&self, allowed: &[&str]) -> Result<(), BenchError> {
        for name in self.values.keys() {
            if !allowed.contains(&name.as_str()) {
                return Err(BenchError::invalid(format!("unknown option --{name}")));
            }
        }
        Ok(())
    }
}

fn facade_of(args: &Args) -> Result<Facade, BenchError> {
    let text = args.require("facade")?;
    Facade::parse(text).ok_or_else(|| {
        BenchError::invalid(format!("--facade takes pinyin or zhuyin, not {text:?}"))
    })
}

fn bind_of(args: &Args) -> Result<Bind, BenchError> {
    match args.one("bind")? {
        None | Some("now") => Ok(Bind::Now),
        Some("lazy") => Ok(Bind::Lazy),
        Some(other) => Err(BenchError::invalid(format!(
            "--bind takes now or lazy, not {other:?}"
        ))),
    }
}

fn kind_of(args: &Args) -> Result<SampleKind, BenchError> {
    match args.one("kind")? {
        None | Some("timing") => Ok(SampleKind::Timing),
        Some("memory") => Ok(SampleKind::Memory),
        Some(other) => Err(BenchError::invalid(format!(
            "--kind takes timing or memory, not {other:?}"
        ))),
    }
}

fn read_inputs(path: &Path) -> Result<Vec<String>, BenchError> {
    let text = fs::read_to_string(path)
        .map_err(|e| BenchError::io(format!("reading {}", path.display()), e))?;
    Ok(text
        .lines()
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect())
}

fn child_options(args: &Args) -> Result<(ChildOptions, PathBuf), BenchError> {
    let stop_after = match args.one("stop-after")? {
        None => StopPoint::End,
        Some(text) => StopPoint::parse(text)
            .ok_or_else(|| BenchError::invalid(format!("unknown stop point {text:?}")))?,
    };
    Ok((
        ChildOptions {
            facade: facade_of(args)?,
            system_dir: PathBuf::from(args.require("system-dir")?),
            user_dir: PathBuf::from(args.require("user-dir")?),
            kind: kind_of(args)?,
            stop_after,
            bind: bind_of(args)?,
        },
        PathBuf::from(args.require("lib")?),
    ))
}

fn print_json<T: serde::Serialize>(value: &T) -> Result<(), BenchError> {
    let line = serde_json::to_string(value)?;
    let mut out = std::io::stdout().lock();
    writeln!(out, "{line}").map_err(|e| BenchError::io("writing to stdout", e))
}

const CHILD_OPTIONS: [&str; 10] = [
    "facade",
    "lib",
    "system-dir",
    "user-dir",
    "bind",
    "kind",
    "stop-after",
    "profile",
    "witness-file",
    "input",
];

fn cmd_child(sub: &str, args: &Args) -> Result<(), BenchError> {
    args.reject_unknown(&CHILD_OPTIONS)?;
    let (options, lib) = child_options(args)?;
    let probe = host_probe()?;
    let (facade, bind) = (options.facade, options.bind);
    let open = move || Native::open(&lib, facade, bind);
    match sub {
        "child" => print_json(&child::timed(open, &options, probe.as_ref())?),
        "verify-child" => {
            let witness = match args.one("witness-file")? {
                Some(path) => read_inputs(Path::new(path))?,
                None => Vec::new(),
            };
            let profile = args.one("profile")?.unwrap_or("unnamed");
            print_json(&child::verify(
                open,
                &options,
                profile,
                &witness,
                probe.as_ref(),
            )?)
        }
        "probe-child" => {
            let input = args.require("input")?;
            let lines = child::probe_input(open, &options, input, probe.as_ref())?;
            print_json(&ProbeLines { lines })
        }
        _ => print_json(&child::persist(open, &options)?),
    }
}

fn probe_exe() -> Result<PathBuf, BenchError> {
    std::env::current_exe().map_err(|e| BenchError::io("locating the benchmark binary", e))
}

fn subject_specs(args: &Args) -> Result<Vec<SubjectSpec>, BenchError> {
    let split = |spec: &String| -> Result<(String, PathBuf), BenchError> {
        let (label, path) = spec
            .split_once('=')
            .ok_or_else(|| BenchError::invalid(format!("expected LABEL=PATH, got {spec:?}")))?;
        Ok((label.to_owned(), PathBuf::from(path)))
    };
    let mut specs: Vec<SubjectSpec> = Vec::new();
    for spec in args.many("subject") {
        let (label, path) = split(spec)?;
        specs.push(SubjectSpec {
            label,
            pinyin: Some(path),
            zhuyin: None,
        });
    }
    for spec in args.many("zhuyin-subject") {
        let (label, path) = split(spec)?;
        match specs.iter_mut().find(|s| s.label == label) {
            Some(s) => s.zhuyin = Some(path),
            None => {
                return Err(BenchError::invalid(format!(
                    "--zhuyin-subject {label} has no --subject"
                )));
            }
        }
    }
    Ok(specs)
}

fn cmd_run(args: &Args) -> Result<(), BenchError> {
    args.reject_unknown(&[
        "subject",
        "zhuyin-subject",
        "system-dir",
        "profile",
        "out",
        "facade",
        "rounds",
        "warmup",
        "kind",
        "cache",
        "cpu",
        "bind",
        "work-dir",
        "max-skew-ms",
        "max-load",
        "timeout-s",
    ])?;
    let subjects = subject_specs(args)?;
    let facades = match args.one("facade")? {
        Some("pinyin") => vec![Facade::Pinyin],
        Some("zhuyin") => vec![Facade::Zhuyin],
        Some("both") => vec![Facade::Pinyin, Facade::Zhuyin],
        None => {
            if subjects.iter().all(|s| s.zhuyin.is_some()) && !subjects.is_empty() {
                vec![Facade::Pinyin, Facade::Zhuyin]
            } else {
                vec![Facade::Pinyin]
            }
        }
        Some(other) => {
            return Err(BenchError::invalid(format!(
                "--facade takes pinyin, zhuyin or both, not {other:?}"
            )));
        }
    };
    let cache = match args.one("cache")? {
        None | Some("warm") => CacheState::Warm,
        Some("cold") => CacheState::ColdDropCaches,
        Some(other) => {
            return Err(BenchError::invalid(format!(
                "--cache takes warm or cold, not {other:?}"
            )));
        }
    };
    let plan = Plan {
        probe: probe_exe()?,
        subjects,
        facades,
        system_dir: PathBuf::from(args.require("system-dir")?),
        profiles: args.many("profile").iter().map(PathBuf::from).collect(),
        rounds: args.number("rounds", 30_usize)?,
        warmup: args.number("warmup", 2_usize)?,
        kind: kind_of(args)?,
        cache,
        cpu: args
            .one("cpu")?
            .map(|_| args.number("cpu", 0_u32))
            .transpose()?,
        bind: bind_of(args)?,
        work_dir: args
            .one("work-dir")?
            .map_or_else(std::env::temp_dir, PathBuf::from),
        max_skew: Duration::from_millis(args.number("max-skew-ms", 50_u64)?),
        max_load: args
            .one("max-load")?
            .map(|_| args.number("max-load", 0.0_f64))
            .transpose()?,
        timeout: Duration::from_secs(args.number("timeout-s", 120_u64)?),
    };
    let out_path = args.require("out")?;
    let mut out = BufWriter::new(
        File::create(out_path).map_err(|e| BenchError::io(format!("creating {out_path}"), e))?,
    );
    let mut sink = |record: &Record| -> Result<(), BenchError> {
        let line = serde_json::to_string(record)?;
        writeln!(out, "{line}")
            .and_then(|()| out.flush())
            .map_err(|e| BenchError::io("writing the result stream", e))
    };
    let summary = run(&plan, &mut sink)?;
    eprintln!(
        "{} accepted measured samples, {} rejected, {} NOT COMPARABLE cell(s); results in {out_path}",
        summary.accepted, summary.rejected, summary.not_comparable
    );
    Ok(())
}

fn cmd_report(args: &Args) -> Result<(), BenchError> {
    args.reject_unknown(&["input", "reference", "ratio"])?;
    let path = args.require("input")?;
    let file = File::open(path).map_err(|e| BenchError::io(format!("opening {path}"), e))?;
    let mut records = Vec::new();
    for (number, line) in BufReader::new(file).lines().enumerate() {
        let line = line.map_err(|e| BenchError::io(format!("reading {path}"), e))?;
        if line.trim().is_empty() {
            continue;
        }
        records.push(
            serde_json::from_str::<Record>(&line)
                .map_err(|e| BenchError::invalid(format!("{path} line {}: {e}", number + 1)))?,
        );
    }
    let ratios = args
        .many("ratio")
        .iter()
        .map(|text| {
            let (n, d) = text.split_once('/').ok_or_else(|| {
                BenchError::invalid(format!("--ratio wants NUM/DEN, got {text:?}"))
            })?;
            Ok(RatioSpec {
                numerator: n.to_owned(),
                denominator: d.to_owned(),
            })
        })
        .collect::<Result<Vec<_>, BenchError>>()?;
    let text = render(
        &records,
        &ReportSpec {
            reference: args.require("reference")?.to_owned(),
            ratios,
        },
    )?;
    print!("{text}");
    Ok(())
}

/// Builds the four fixtures with the pin and proves each non-empty one loaded.
fn cmd_profiles(args: &Args) -> Result<(), BenchError> {
    args.reject_unknown(&["out", "pin-lib", "system-dir", "work-dir"])?;
    let out = PathBuf::from(args.require("out")?);
    let system = PathBuf::from(args.require("system-dir")?);
    let lib = PathBuf::from(args.require("pin-lib")?);
    // The work directory may be one the caller owns: build in a subdirectory
    // this process creates and remove only that.
    let parent = args
        .one("work-dir")?
        .map_or_else(|| out.join(".work"), PathBuf::from);
    let work = parent.join(format!("initbench-profiles-{}", std::process::id()));
    fs::create_dir_all(&work).map_err(|e| BenchError::io("creating the work dir", e))?;
    let result = build_profiles(&out, &lib, &system, &work);
    let _ = fs::remove_dir_all(&work);
    result
}

fn build_profiles(out: &Path, lib: &Path, system: &Path, work: &Path) -> Result<(), BenchError> {
    let mut pin = Native::open(lib, Facade::Pinyin, Bind::Now)?;
    let scratch = work.join("export-user");
    fs::create_dir_all(&scratch).map_err(|e| BenchError::io("creating a scratch dir", e))?;
    let export = fixtures::export_system(&mut pin, system, &scratch)?;
    let registered = fixtures::select_sequential(&export, 5)?;
    let digest = fixtures::phrase_input_digest(&registered);
    println!(
        "phrase_input_registered\trows={}\tsha256={digest}\tmatches_runbook={}",
        registered.len(),
        digest == REGISTERED_PHRASE_INPUT_SHA256
    );
    let exe = probe_exe()?;
    let mut empty_user: Option<PathBuf> = None;
    for level in Level::ALL {
        let root = out.join(level.name());
        if root.exists() {
            return Err(BenchError::invalid(format!(
                "{} exists; remove it first (fixtures are never overwritten in place)",
                root.display()
            )));
        }
        let user = root.join(USER_SUBDIR);
        fs::create_dir_all(&user).map_err(|e| BenchError::io("creating a fixture", e))?;
        let rows = match level {
            Level::Empty => Vec::new(),
            Level::Modest => fixtures::modest_rows(),
            Level::Phrases => fixtures::select_sequential(&export, FIXTURE_FREQUENCY)?,
            Level::Journals => fixtures::select_balanced(&export, FIXTURE_FREQUENCY)?,
        };
        if !rows.is_empty() {
            fixtures::populate(&mut pin, system, &user, &rows)?;
        }
        let manifest = Manifest::of_directory(&user)?;
        let witness_path = root.join(WITNESS_FILE);
        let note = if level == Level::Empty {
            if !manifest.files.is_empty() {
                return Err(BenchError::invalid("the empty fixture is not empty"));
            }
            fs::write(&witness_path, "").map_err(|e| BenchError::io("writing the witness", e))?;
            empty_user = Some(user.clone());
            "none".to_owned()
        } else {
            if manifest.files.is_empty() {
                return Err(BenchError::invalid(format!(
                    "{}: nothing was saved",
                    level.name()
                )));
            }
            let reference = empty_user
                .as_deref()
                .ok_or_else(|| BenchError::invalid("the empty fixture must come first"))?;
            let candidates = fixtures::candidate_inputs(&rows, 400);
            let candidate_file = work.join(format!("candidates-{}.txt", level.name()));
            fs::write(
                &candidate_file,
                candidates
                    .iter()
                    .map(|c| format!("{c}\n"))
                    .collect::<String>(),
            )
            .map_err(|e| BenchError::io("writing the candidates", e))?;
            let a = verify_transcript(&exe, lib, system, reference, &candidate_file, work)?;
            let b = verify_transcript(&exe, lib, system, &user, &candidate_file, work)?;
            let wanted: std::collections::BTreeSet<&str> =
                candidates.iter().map(String::as_str).collect();
            let chosen: Vec<String> = fixtures::discriminating(&a.lines, &b.lines)
                .into_iter()
                .filter_map(|tag| {
                    let input = tag
                        .split_once('"')
                        .and_then(|(_, rest)| rest.rsplit_once('"'))
                        .map(|(s, _)| s.to_owned())?;
                    wanted.contains(input.as_str()).then_some(input)
                })
                .take(40)
                .collect();
            if chosen.is_empty() {
                return Err(BenchError::invalid(format!(
                    "{}: no candidate input is answered differently by the reopened profile; \
                     the saved data was not shown to be loaded",
                    level.name()
                )));
            }
            fs::write(
                &witness_path,
                chosen.iter().map(|c| format!("{c}\n")).collect::<String>(),
            )
            .map_err(|e| BenchError::io("writing the witness", e))?;
            format!(
                "{} discriminating inputs of {} tried",
                chosen.len(),
                candidates.len()
            )
        };
        fs::write(root.join(MANIFEST_FILE), manifest.to_text())
            .map_err(|e| BenchError::io("writing the manifest", e))?;
        println!(
            "{}\tfiles={}\tbytes={}\tidentity={}\twitness={note}",
            level.name(),
            manifest.files.len(),
            manifest.total_bytes(),
            manifest.identity()
        );
    }
    Ok(())
}

fn verify_transcript(
    exe: &Path,
    lib: &Path,
    system: &Path,
    user: &Path,
    witness: &Path,
    work: &Path,
) -> Result<Observation, BenchError> {
    let copy = work.join("verify-copy");
    let _ = fs::remove_dir_all(&copy);
    copy_tree(user, &copy)?;
    let mut command = Command::new(exe);
    command
        .args(["verify-child", "--facade", "pinyin", "--lib"])
        .arg(lib)
        .arg("--system-dir")
        .arg(system)
        .arg("--user-dir")
        .arg(&copy)
        .args(["--profile", "witness-search", "--witness-file"])
        .arg(witness);
    let out = run_process(command, Duration::from_secs(300));
    let _ = fs::remove_dir_all(&copy);
    let out = out?;
    let line = out
        .stdout
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("");
    Ok(serde_json::from_str(line)?)
}

fn cmd_instrument(args: &Args) -> Result<(), BenchError> {
    args.reject_unknown(&[
        "lib",
        "facade",
        "system-dir",
        "profile",
        "tool",
        "cpu",
        "work-dir",
    ])?;
    if !cfg!(target_os = "linux") {
        return Err(BenchError::Unsupported(
            "the instrument subcommand drives Valgrind, which is Linux-only here; on macOS use \
             `footprint`, `vmmap -summary` and `xcrun xctrace` by hand"
                .to_owned(),
        ));
    }
    let tool = args.require("tool")?;
    if tool != "callgrind" && tool != "dhat" {
        return Err(BenchError::invalid("--tool takes callgrind or dhat"));
    }
    let facade = facade_of(args)?;
    let fixture = Fixture::open(Path::new(args.require("profile")?))?;
    let work_dir = args
        .one("work-dir")?
        .map_or_else(std::env::temp_dir, PathBuf::from);
    let exe = probe_exe()?;
    let mut rows: Vec<(StopPoint, Vec<u64>)> = Vec::new();
    for stop in StopPoint::ALL {
        let copy = fixture.working_copy(&work_dir)?;
        let child_args: Vec<String> = vec![
            "child".into(),
            "--facade".into(),
            facade.prefix().into(),
            "--lib".into(),
            args.require("lib")?.into(),
            "--system-dir".into(),
            args.require("system-dir")?.into(),
            "--user-dir".into(),
            copy.path().display().to_string(),
            "--bind".into(),
            "now".into(),
            "--stop-after".into(),
            stop.name().into(),
        ];
        let cg_out = work_dir.join(format!(
            "callgrind-{}-{}.out",
            std::process::id(),
            stop.name()
        ));
        let mut command = if let Some(cpu) = args.one("cpu")? {
            let mut c = Command::new("taskset");
            c.args(["-c", cpu, "valgrind"]);
            c
        } else {
            Command::new("valgrind")
        };
        if tool == "callgrind" {
            command.args(["--tool=callgrind", "--cache-sim=no", "--branch-sim=no"]);
            command.arg(format!("--callgrind-out-file={}", cg_out.display()));
        } else {
            command.args(["--tool=dhat", "--dhat-out-file=/dev/null"]);
        }
        command.arg(&exe).args(&child_args);
        let output = run_process(command, Duration::from_secs(1800))?;
        let figures = if tool == "callgrind" {
            let text = fs::read_to_string(&cg_out)
                .map_err(|e| BenchError::io("reading the callgrind output", e))?;
            let _ = fs::remove_file(&cg_out);
            vec![parse_callgrind_ir(&text)?]
        } else {
            let h = parse_dhat_summary(&output.stderr)?;
            vec![h.total_bytes, h.total_blocks, h.peak_bytes, h.end_bytes]
        };
        rows.push((stop, figures));
    }
    fixture.verify()?;
    let version = Command::new("valgrind")
        .arg("--version")
        .output()
        .map_err(|e| BenchError::io("running valgrind --version", e))?;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "valgrind: {}\n\nWhole-process figures ({}); the delta column is this stop minus the previous one.\n",
        String::from_utf8_lossy(&version.stdout).trim(),
        facade.prefix()
    );
    if tool == "callgrind" {
        let _ = writeln!(
            out,
            "| stop after | Ir (cumulative) | Ir (delta) |\n| --- | ---: | ---: |"
        );
        let mut previous = 0_u64;
        for (stop, f) in &rows {
            let _ = writeln!(
                out,
                "| {} | {} | {} |",
                stop.name(),
                f[0],
                f[0].saturating_sub(previous)
            );
            previous = f[0];
        }
    } else {
        let _ = writeln!(
            out,
            "| stop after | bytes allocated | allocation calls | peak live bytes | live at exit |\n| --- | ---: | ---: | ---: | ---: |"
        );
        for (stop, f) in &rows {
            let _ = writeln!(
                out,
                "| {} | {} | {} | {} | {} |",
                stop.name(),
                f[0],
                f[1],
                f[2],
                f[3]
            );
        }
    }
    print!("{out}");
    Ok(())
}

fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let Some((command, rest)) = raw.split_first() else {
        eprint!("{USAGE}");
        return ExitCode::from(2);
    };
    let result = Args::parse(rest).and_then(|args| match command.as_str() {
        sub @ ("child" | "verify-child" | "probe-child" | "persist-child") => cmd_child(sub, &args),
        "run" => cmd_run(&args),
        "report" => cmd_report(&args),
        "profiles" => cmd_profiles(&args),
        "instrument" => cmd_instrument(&args),
        other => Err(BenchError::invalid(format!(
            "unknown subcommand {other:?}\n{USAGE}"
        ))),
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("initbench: {error}");
            ExitCode::FAILURE
        }
    }
}
