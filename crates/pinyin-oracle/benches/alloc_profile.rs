//! One-shot profiles used by `docs/perf/perf-exploration.md`.
//!
//! Heap attribution needs the `dhat-heap` feature, which installs `dhat`'s
//! global allocator for the whole executable:
//!
//! ```text
//! cargo bench -p pinyin-oracle --features dhat-heap --bench alloc_profile -- --dhat
//! ```
//!
//! writes `/tmp/dhat-heap-parity.json` for a full W2 parity `type_pinyin`
//! run (`--dhat --keystroke-only` profiles the keystroke cycle instead,
//! into `/tmp/dhat-heap-keystroke.json`). The profiler wraps only the decode
//! loop: tables and inputs are loaded before it starts, so setup
//! allocations do not land in the dump.
//!
//! Without the feature the executable runs on the system allocator and
//! prints probe counts, table sizes, load timings and the `--parity-timed`
//! and `--keystroke-only [--repeats N]` timings; `--dhat` then exits with
//! the command above. With the feature, every mode except `--dhat` exits
//! instead of printing timings: the allocator wrapper stays installed even
//! when the profiler is not running, so those timings would not be
//! allocator-uninstrumented.
//!
//! Timings printed by this bench before the feature existed came from an
//! executable that always had `dhat::Alloc` installed. They are kept as
//! recorded; the overhead of the dormant wrapper is workload- and
//! build-specific, so they are not rescaled.
//!
//! This is a custom bench harness, not a Criterion group.

#![allow(missing_docs)]

use std::env;
use std::time::Instant;

#[cfg(not(feature = "dhat-heap"))]
use oxpinyin_data::parse_interpolation2;

#[path = "support/mod.rs"]
mod harness;

use harness::{
    CYCLE_INPUTS, load_real_tables, load_w2_inputs, real_session, type_batch, type_keystrokes,
};
#[cfg(not(feature = "dhat-heap"))]
use harness::{CountingDict, ProbeStats, counting_session, interpolation2_path};

#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

fn main() {
    let args: Vec<String> = env::args().collect();
    let want_dhat = args.iter().any(|arg| arg == "--dhat");
    #[cfg(feature = "dhat-heap")]
    {
        if !want_dhat {
            eprintln!(
                "built with dhat-heap: dhat's allocator is installed, so timing modes would be mislabelled; rebuild without the feature, or pass --dhat"
            );
            std::process::exit(2);
        }
        if let Some(ignored) = ["--parity-timed", "--repeats"]
            .into_iter()
            .find(|flag| args.iter().any(|arg| arg == flag))
        {
            eprintln!(
                "--dhat cannot be combined with {ignored}: it profiles the W2 batch, or the keystroke cycle with --keystroke-only, and would ignore it"
            );
            std::process::exit(2);
        }
        dhat_profile(&args);
    }
    #[cfg(not(feature = "dhat-heap"))]
    {
        if want_dhat {
            eprintln!(
                "--dhat needs the dhat-heap feature: cargo bench -p pinyin-oracle --features dhat-heap --bench alloc_profile -- --dhat"
            );
            std::process::exit(2);
        }
        plain_modes(&args);
    }
}

#[cfg(not(feature = "dhat-heap"))]
fn plain_modes(args: &[String]) {
    if args.iter().any(|arg| arg == "--parity-timed") {
        timed_parity();
        return;
    }
    if args.iter().any(|arg| arg == "--keystroke-only") {
        let repeats = args
            .windows(2)
            .find(|pair| pair[0] == "--repeats")
            .and_then(|pair| pair[1].parse().ok())
            .unwrap_or(1_u32);
        let (dict, lm) = load_real_tables();
        let mut session = real_session(&dict, &lm);
        for _ in 0..repeats {
            for input in CYCLE_INPUTS {
                type_keystrokes(&mut session, input);
            }
        }
        return;
    }
    report_tables_and_load();
    report_probe_counts();
    timed_parity();
}

#[cfg(feature = "dhat-heap")]
fn dhat_profile(args: &[String]) {
    // Load everything the profile should not attribute to the decoder
    // before the profiler starts: table slurp, prefix-table build,
    // interpolation2 parse, corpus file reading.
    let (dict, lm) = load_real_tables();
    let mut session = real_session(&dict, &lm);
    let keystroke = args.iter().any(|arg| arg == "--keystroke-only");
    let dump = if keystroke {
        "/tmp/dhat-heap-keystroke.json"
    } else {
        "/tmp/dhat-heap-parity.json"
    };
    // Corpus files and fixture parsing belong with table load: the
    // profiler must wrap only type_batch / parse+guess.
    let w2_inputs = if keystroke {
        Vec::new()
    } else {
        load_w2_inputs()
    };
    // Non-testing mode writes the JSON on Drop (`testing()` skips finish).
    let _profiler = dhat::Profiler::builder()
        .file_name(dump)
        .trim_backtraces(Some(16))
        .build();
    let started = Instant::now();
    if keystroke {
        for input in CYCLE_INPUTS {
            type_keystrokes(&mut session, input);
        }
    } else {
        for input in &w2_inputs {
            type_batch(&mut session, input);
        }
    }
    let elapsed_ms = started.elapsed().as_secs_f64() * 1_000.0;
    // 123 parse+guess steps: sum of CYCLE_INPUTS character counts.
    let ops = if keystroke { 123_u64 } else { 0 };
    println!(
        "dhat_scope               {}",
        if keystroke {
            "keystroke_cycle"
        } else {
            "w2_batch"
        }
    );
    println!("dhat_dump                {dump}");
    println!("wall_ms                  {elapsed_ms:.3}");
    if ops > 0 {
        println!("ops                      {ops}");
        println!("cycle_ms                 {elapsed_ms:.3}");
    }
}

#[cfg(not(feature = "dhat-heap"))]
fn report_tables_and_load() {
    let path = interpolation2_path();
    let meta = std::fs::metadata(&path).expect("interpolation2 metadata");
    println!("interpolation2");
    println!("  path                   {}", path.display());
    println!("  file_bytes             {}", meta.len());

    // Cold: drop page cache is not available without privileges. We still
    // distinguish first-open from a subsequent parse of an already-warm file.
    let cold = Instant::now();
    let table = parse_interpolation2(&path).expect("cold parse");
    let cold_ms = cold.elapsed().as_secs_f64() * 1_000.0;
    let warm = Instant::now();
    let table2 = parse_interpolation2(&path).expect("warm parse");
    let warm_ms = warm.elapsed().as_secs_f64() * 1_000.0;
    println!("  records                {}", table.len());
    println!("  cold_parse_ms          {cold_ms:.3}");
    println!("  warm_parse_ms          {warm_ms:.3}");
    assert_eq!(table.len(), table2.len());

    let load = Instant::now();
    let _tables = load_real_tables();
    println!(
        "  full_table_load_ms     {:.3}",
        load.elapsed().as_secs_f64() * 1_000.0
    );
}

#[cfg(not(feature = "dhat-heap"))]
fn report_probe_counts() {
    let (dict, lm) = load_real_tables();
    let stats = ProbeStats::default();
    let counting = CountingDict {
        inner: &dict,
        stats: &stats,
    };
    let mut session = counting_session(counting, &lm);
    let inputs = load_w2_inputs();
    let started = Instant::now();
    for input in &inputs {
        type_batch(&mut session, input);
    }
    let total_ms = started.elapsed().as_secs_f64() * 1_000.0;
    let (probes, probe_ns, lookups, entries) = stats.snapshot();
    println!("parity_batch_w2");
    println!("  inputs                 {}", inputs.len());
    println!("  wall_ms                {total_ms:.3}");
    println!("  prefix_probe_calls     {probes}");
    println!(
        "  prefix_probe_ms        {:.3}",
        probe_ns as f64 / 1_000_000.0
    );
    println!(
        "  prefix_probe_share     {:.3}%",
        if total_ms > 0.0 {
            (probe_ns as f64 / 1_000_000.0) / total_ms * 100.0
        } else {
            0.0
        }
    );
    println!("  lookup_calls           {lookups}");
    println!("  lookup_entries         {entries}");
    println!(
        "  probes_per_input       {:.2}",
        probes as f64 / inputs.len() as f64
    );
}

#[cfg(not(feature = "dhat-heap"))]
fn timed_parity() {
    let (dict, lm) = load_real_tables();
    let mut session = real_session(&dict, &lm);
    let inputs = load_w2_inputs();

    let started = Instant::now();
    for input in &inputs {
        type_batch(&mut session, input);
    }
    println!(
        "parity_uninstrumented_ms  {:.3}",
        started.elapsed().as_secs_f64() * 1_000.0
    );

    let started = Instant::now();
    for input in CYCLE_INPUTS {
        type_keystrokes(&mut session, input);
    }
    println!(
        "keystroke_20_ms           {:.3}",
        started.elapsed().as_secs_f64() * 1_000.0
    );
}
