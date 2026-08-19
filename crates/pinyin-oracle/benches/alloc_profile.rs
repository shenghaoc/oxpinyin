//! One-shot profiles used by `docs/findings/perf-exploration.md`.
//!
//! `cargo bench -p pinyin-oracle --bench alloc_profile -- --dhat` writes
//! `/tmp/dhat-heap-parity.json` for a full W2 parity `type_pinyin` run. The
//! profiler wraps only the decode loop: tables and inputs are loaded before
//! it starts, so setup allocations do not land in the dump. Without
//! `--dhat` the same loop prints probe counts, table sizes, and load
//! timings.
//!
//! This is a custom bench harness, not a Criterion group.

#![allow(missing_docs)]

use std::env;
use std::time::Instant;

use oxpinyin_data::parse_interpolation2;

#[path = "support/mod.rs"]
mod harness;

use harness::{
    CYCLE_INPUTS, CountingDict, ProbeStats, counting_session, interpolation2_path,
    load_prefix_tables, load_real_tables, load_w2_inputs, prefix_probe, real_session,
    string_table_bytes, type_batch, type_keystrokes,
};

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.iter().any(|arg| arg == "--dhat") {
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
            let inputs = load_w2_inputs();
            for input in &inputs {
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
        return;
    }
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
    if args.iter().any(|arg| arg == "--dump-keys") {
        dump_prefix_keys();
        return;
    }
    report_tables_and_load();
    report_probe_counts();
    timed_parity();
}

fn dump_prefix_keys() {
    let (pinyin_keys, initial_keys) = load_prefix_tables();
    std::fs::write("/tmp/pinyin_keys.txt", pinyin_keys.join("\n") + "\n").expect("write");
    std::fs::write("/tmp/initial_keys.txt", initial_keys.join("\n") + "\n").expect("write");
    println!(
        "wrote {} pinyin keys and {} initial keys",
        pinyin_keys.len(),
        initial_keys.len()
    );
}

fn report_tables_and_load() {
    let (pinyin_keys, initial_keys) = load_prefix_tables();
    let pinyin_bytes = string_table_bytes(&pinyin_keys);
    let initial_bytes = string_table_bytes(&initial_keys);
    println!("prefix_tables");
    println!("  pinyin_keys.len        {}", pinyin_keys.len());
    println!("  initial_keys.len       {}", initial_keys.len());
    println!("  pinyin_keys.heap_bytes {pinyin_bytes}");
    println!("  initial_keys.heap_bytes {initial_bytes}");
    println!("  both.heap_bytes        {}", pinyin_bytes + initial_bytes);

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

    // Isolated prefix_probe throughput, to compare with the FST prototype.
    let (pinyin_keys, initial_keys) = load_prefix_tables();
    let needles: Vec<&str> = CYCLE_INPUTS.to_vec();
    let started = Instant::now();
    let mut hits = 0_u64;
    for _ in 0..10_000 {
        for needle in &needles {
            if prefix_probe(&pinyin_keys, needle) {
                hits += 1;
            }
            if prefix_probe(&initial_keys, needle) {
                hits += 1;
            }
        }
    }
    let elapsed = started.elapsed();
    let ops = 10_000_u64 * needles.len() as u64 * 2;
    println!("isolated_prefix_probe");
    println!("  ops                    {ops}");
    println!(
        "  wall_ms                {:.3}",
        elapsed.as_secs_f64() * 1_000.0
    );
    println!(
        "  ns_per_probe           {:.2}",
        elapsed.as_nanos() as f64 / ops as f64
    );
    println!("  hits                   {hits}");
}

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
