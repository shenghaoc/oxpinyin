//! Keystroke timing, hardware counts and resident/allocator diagnostics.
use crate::{Result, abi, output, platform, require, workload};
use std::io::Write;

pub(crate) fn run(args: &[String]) -> Result<()> {
    // #lizard forgives(cyclomatic_complexity)
    // One ordered measurement lifecycle keeps the sampling and emission boundaries visible.
    let mode = std::env::var("PERF_MODE").unwrap_or_else(|_| "speed".into());
    if args.len() < 4 || !matches!(mode.as_str(), "speed" | "rss-diag") {
        return Err(format!("Usage: {} --perf <path-to-so> <systemdir>", args[0]).into());
    }
    let facade = std::env::var("PERF_FACADE").unwrap_or_else(|_| "pinyin".into());
    require(
        matches!(facade.as_str(), "pinyin" | "zhuyin"),
        "fatal: PERF_FACADE must be pinyin or zhuyin",
    )?;
    let backend = std::env::var("PERF_BACKEND").unwrap_or_else(|_| args[2].clone());
    let cycles = workload::count("PERF_CYCLES", 8);
    let repeats = workload::count("PERF_REPEATS", 1);
    let checksum = std::env::var_os("PERF_WORKLOAD_CHECKSUM").is_some_and(|s| !s.is_empty());
    let diag_dir = std::env::var("RSS_DIAG_DIR").ok();
    let tag = std::env::var("RSS_DIAG_TAG")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "run".into());
    let diag = mode == "rss-diag";
    let hardware = std::env::var_os("PERF_HW").is_some();
    let user = abi::UserDir::supplied_or_temporary("bisect-perf-")?;
    let library = abi::Library::open(&args[2])?;
    let core = abi::Core::load(&library, &facade)?;
    let typing = abi::Typing::load(&library, &facade)?;
    let guess = abi::Guess::load(&library, &facade)?;
    let paths = abi::ContextPaths::new(&args[3], user.path())?;
    let mut cycle_ns = vec![0; cycles];
    let mut cpu_ns = vec![0; cycles];
    let mut out = std::io::stdout().lock();
    let mut counter = None;
    let mut hw_errno = 0;
    if hardware {
        match platform::Counter::open() {
            Ok(value) => counter = Some(value),
            Err(error) => {
                hw_errno = error
                    .downcast_ref::<std::io::Error>()
                    .and_then(|e| e.raw_os_error())
                    .unwrap_or(0);
                eprintln!(
                    "perf_event_open: {error} (errno {hw_errno}) — this run carries no hardware instruction counts"
                );
            }
        }
    }
    let init_start = platform::now_ns();
    let ctx = abi::Context::open(&core, &paths)?;
    let init_ns = platform::now_ns() - init_start;
    require(
        (typing.options)(ctx.handle, 0x18a),
        "fatal: set_options failed",
    )?;
    let alloc_start = platform::now_ns();
    let instance = abi::Instance::open(&typing, ctx.handle)?;
    let alloc_ns = platform::now_ns() - alloc_start;
    let after_init = platform::Memory::read();
    let initial = diag.then(platform::Diagnostic::read);
    if let (Some(state), Some(dir)) = (&initial, &diag_dir) {
        state.dump(dir, &tag, "init");
    }
    let mut cycle = workload::Cycle {
        typing: &typing,
        guess: &guess,
        instance: instance.handle,
        operations: 0,
        failed: false,
    };
    let mut hw_cycles = 0;
    let mut hw_instructions = 0u64;
    let mut workload_checksum = workload::FNV_OFFSET;
    let mut after_first = after_init;
    for i in 0..cycles {
        if i != 0
            && let Some(c) = &mut counter
        {
            c.enable();
        }
        let cpu_start = platform::cpu_ns();
        let start = platform::now_ns();
        let value = if i == 0 {
            workload::bisect_perf_cold_unit(&mut cycle, repeats)
        } else {
            workload::bisect_perf_steady_unit(&mut cycle, repeats)
        };
        let end = platform::now_ns();
        let cpu_end = platform::cpu_ns();
        workload_checksum ^= value;
        workload_checksum = workload_checksum.wrapping_mul(workload::FNV_PRIME);
        if i != 0
            && let Some(c) = &mut counter
            && let Some(value) = c.sample()
        {
            hw_instructions += value;
            hw_cycles += 1;
        }
        require(
            !cycle.failed
                && cycle.operations == workload::operations() * repeats as u64 * (i + 1) as u64,
            "fatal: incomplete perf workload",
        )?;
        cycle_ns[i] = end - start;
        cpu_ns[i] = cpu_end - cpu_start;
        if i == 0 {
            after_first = platform::Memory::read();
        }
    }
    let after_last = platform::Memory::read();
    write!(out, "{{\"pid\":{},\"backend\":", std::process::id())?;
    output::string(&mut out, &backend)?;
    out.write_all(b",\"so\":")?;
    output::string(&mut out, &args[2])?;
    out.write_all(b",\"systemdir\":")?;
    output::string(&mut out, &args[3])?;
    out.write_all(b",\"mode\":")?;
    output::string(&mut out, &mode)?;
    write!(
        out,
        ",\"repeats\":{repeats},\"init_ns\":{init_ns},\"alloc_ns\":{alloc_ns},\"after_init\":{after_init}"
    )?;
    if let Some(state) = &initial {
        state.write(&mut out, "init")?;
    }
    if checksum {
        write!(out, ",\"workload_checksum\":\"{workload_checksum:016x}\"")?;
    }
    let after_cycle = diag.then(platform::Diagnostic::read);
    if let (Some(state), Some(dir)) = (&after_cycle, &diag_dir) {
        state.dump(dir, &tag, "cycle");
    }
    let released = if diag { platform::trim() } else { "null" };
    let after_trim = platform::Memory::read();
    let trimmed = diag.then(platform::Diagnostic::read);
    if let (Some(state), Some(dir)) = (&trimmed, &diag_dir) {
        state.dump(dir, &tag, "trim");
    }
    out.write_all(b",\"cycles_ns\":")?;
    output::numbers(&mut out, &cycle_ns)?;
    out.write_all(b",\"cycles_cpu_ns\":")?;
    output::numbers(&mut out, &cpu_ns)?;
    write!(
        out,
        ",\"after_first\":{after_first},\"after_last\":{after_last}"
    )?;
    if let (Some(state), Some(trimmed)) = (&after_cycle, &trimmed) {
        state.write(&mut out, "cycle")?;
        write!(
            out,
            ",\"trim_released\":{released},\"after_trim\":{after_trim}"
        )?;
        trimmed.write(&mut out, "trim")?;
    }
    if counter.is_some() {
        write!(
            out,
            ",\"steady_hw_cycles\":{hw_cycles},\"steady_hw_instructions\":{hw_instructions}"
        )?;
    } else if hardware {
        write!(out, ",\"hw_errno\":{hw_errno}")?;
    }
    writeln!(out, "}}")?;
    Ok(())
}
