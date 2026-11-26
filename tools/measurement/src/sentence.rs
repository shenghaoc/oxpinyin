//! Sentence-only and accumulated-prefix observations outside collection anchors.
use crate::{Result, Usage, abi, output, platform, require};
use std::ffi::CStr;
use std::io::Write;

#[derive(Default)]
pub(crate) struct Step {
    parsed: usize,
    sentence: bool,
    candidates: bool,
}

#[inline(never)]
#[unsafe(no_mangle)]
fn bisect_sentence_unit(sentence: &abi::Sentence, instance: abi::Handle) -> bool {
    let _collection = platform::Collection::new();
    (sentence.guess)(instance)
}
#[inline(never)]
#[unsafe(no_mangle)]
fn bisect_typing_unit(
    typing: &abi::Typing,
    guess: &abi::Guess,
    sentence: &abi::Sentence,
    instance: abi::Handle,
    input: &CStr,
) -> Step {
    let _collection = platform::Collection::new();
    Step {
        parsed: (typing.parse)(instance, input.as_ptr()),
        sentence: (sentence.guess)(instance),
        candidates: guess.call(instance),
    }
}

pub(crate) fn run(args: &[String]) -> Result<()> {
    // #lizard forgives(cyclomatic_complexity)
    // One ordered replay keeps preparation, collection and observation in their required order.
    if args.len() != 6 {
        return Err(Usage(
            "Usage: bisect --perf-sentence|--perf-typing-sequence SO DATA ASCII-PINYIN ITERATIONS"
                .into(),
        )
        .into());
    }
    let input = &args[4];
    let iterations = args[5]
        .parse::<usize>()
        .ok()
        .filter(|n| (1..=100000).contains(n));
    if input.is_empty()
        || input.len() > 255
        || !input.bytes().all(|b| b.is_ascii_lowercase() || b == b'\'')
        || args[5].is_empty()
        || !args[5].bytes().all(|b| b.is_ascii_digit())
        || iterations.is_none()
    {
        return Err(Usage(String::new()).into());
    }
    let iterations = iterations.unwrap_or(0);
    let is_typing = args[1] == "--perf-typing-sequence";
    let user = abi::UserDir::temporary("bisect-sentence-")?;
    let library = abi::Library::open(&args[2])?;
    let core = abi::Core::load(&library, "pinyin")?;
    let typing = abi::Typing::load(&library, "pinyin")?;
    let guess = abi::Guess::load(&library, "pinyin")?;
    let sentence = abi::Sentence::load(&library, "pinyin")?;
    let release = library.release()?;
    let paths = abi::ContextPaths::new(&args[3], user.path())?;
    let ctx = abi::Context::open(&core, &paths)?;
    require(
        (typing.options)(ctx.handle, 0x18a),
        "sentence workload failed",
    )?;
    let instance = abi::Instance::open(&typing, ctx.handle)?;
    let mut out = std::io::stdout().lock();
    write!(
        out,
        "{{\"pid\":{},\"workload\":\"{}\",\"iterations\":{iterations},\"observations\":[",
        std::process::id(),
        if is_typing {
            "typing-sequence"
        } else {
            "sentence"
        }
    )?;
    let mut elapsed = 0;
    // `PERF_HW` counts user-space instructions around the same anchor, natively. A refused
    // counter leaves its errno for the report and the run goes on.
    let mut hw_errno = 0;
    let mut counter = std::env::var_os("PERF_HW").and_then(|_| {
        platform::Counter::open()
            .map_err(|error| {
                hw_errno = error
                    .downcast_ref::<std::io::Error>()
                    .and_then(|e| e.raw_os_error())
                    .unwrap_or(0);
            })
            .ok()
    });
    // `None` once any sample is missing: a partial total is not reported.
    let mut hw_instructions = Some(0u64);
    for repeat in 0..iterations {
        require((typing.reset)(instance.handle), "sentence workload failed")?;
        if repeat != 0 {
            out.write_all(b",")?;
        }
        out.write_all(b"[")?;
        let first = if is_typing { 1 } else { input.len() };
        for n in first..=input.len() {
            let mut prefix = [0u8; 256];
            prefix[..n].copy_from_slice(&input.as_bytes()[..n]);
            let prefix = CStr::from_bytes_with_nul(&prefix[..=n])?;
            let mut step = Step::default();
            if !is_typing {
                step.parsed = (typing.parse)(instance.handle, prefix.as_ptr());
            }
            if let Some(c) = &mut counter {
                c.enable();
            }
            let start = platform::now_ns();
            if is_typing {
                step = bisect_typing_unit(&typing, &guess, &sentence, instance.handle, prefix);
            } else {
                step.sentence = bisect_sentence_unit(&sentence, instance.handle);
            }
            let end = platform::now_ns();
            if let Some(c) = &mut counter {
                hw_instructions = hw_instructions
                    .zip(c.sample())
                    .map(|(total, count)| total + count);
            }
            require(
                start != 0
                    && end >= start
                    && step.parsed != 0
                    && step.sentence
                    && (!is_typing || step.candidates),
                "sentence workload failed",
            )?;
            elapsed += end - start;
            if n != first {
                out.write_all(b",")?;
            }
            observe(
                &mut out,
                &typing,
                &sentence,
                release,
                instance.handle,
                prefix.to_str()?,
                &step,
                is_typing,
            )?;
        }
        out.write_all(b"]")?;
    }
    write!(out, "],\"elapsed_ns\":{elapsed}")?;
    match (counter.is_some(), hw_instructions) {
        (true, Some(total)) => write!(out, ",\"hw_instructions\":{total}")?,
        _ if hw_errno != 0 || std::env::var_os("PERF_HW").is_some() => {
            write!(out, ",\"hw_errno\":{hw_errno}")?;
        }
        _ => {}
    }
    // `PERF_MODE=rss-diag` reads smaps_rollup once, after the last iteration, with the
    // library still open.
    if std::env::var("PERF_MODE").is_ok_and(|mode| mode == "rss-diag") {
        platform::Diagnostic::read().write(&mut out, "after")?;
    }
    writeln!(out, "}}")?;
    Ok(())
}

#[expect(
    clippy::too_many_arguments,
    reason = "Observation consumes one ABI snapshot and writes it outside collection"
)]
fn observe(
    out: &mut impl Write,
    typing: &abi::Typing,
    sentence: &abi::Sentence,
    release: extern "C" fn(abi::Handle),
    instance: abi::Handle,
    input: &str,
    step: &Step,
    is_typing: bool,
) -> Result<()> {
    let mut value = std::ptr::null_mut();
    let ok = (sentence.get)(instance, 0, &mut value);
    let value = abi::StringValue::new(value, release);
    out.write_all(b"{\"input\":")?;
    output::string(out, input)?;
    write!(
        out,
        ",\"parsed\":{},\"guess_sentence\":{},\"get_sentence\":{ok},\"sentence\":",
        step.parsed, step.sentence
    )?;
    output::nullable(out, value.text()?)?;
    require(ok, "sentence workload failed")?;
    if is_typing {
        candidates(out, typing, sentence, instance, step)?;
    }
    out.write_all(b"}")?;
    Ok(())
}

fn candidates(
    out: &mut impl Write,
    typing: &abi::Typing,
    sentence: &abi::Sentence,
    instance: abi::Handle,
    step: &Step,
) -> Result<()> {
    let mut count = 0;
    require(
        (typing.count)(instance, &mut count),
        "sentence workload failed",
    )?;
    write!(
        out,
        ",\"guess_candidates\":{},\"candidates\":[",
        step.candidates
    )?;
    for i in 0..count {
        if i != 0 {
            out.write_all(b",")?;
        }
        candidate(out, sentence, instance, i)?;
    }
    out.write_all(b"]")?;
    Ok(())
}

pub(crate) fn candidate(
    out: &mut impl Write,
    api: &abi::Sentence,
    instance: abi::Handle,
    index: u32,
) -> Result<()> {
    let (candidate, kind, rank) = candidate_info(api, instance, index)?;
    let mut text = std::ptr::null();
    require(
        (api.text)(instance, candidate, &mut text),
        "sentence workload failed",
    )?;
    write!(out, "{{\"type\":{kind},\"nbest\":")?;
    if kind == 1 {
        write!(out, "{rank}")?;
    } else {
        out.write_all(b"null")?;
    }
    out.write_all(b",\"text\":")?;
    output::nullable(out, abi::text(text)?)?;
    out.write_all(b"}")?;
    Ok(())
}
pub(crate) fn candidate_info(
    api: &abi::Sentence,
    instance: abi::Handle,
    index: u32,
) -> Result<(abi::Handle, i32, u8)> {
    let mut candidate = std::ptr::null_mut();
    let mut kind = 2;
    let mut rank = 0;
    require(
        (api.candidate)(instance, index, &mut candidate)
            && !candidate.is_null()
            && (api.kind)(instance, candidate, &mut kind),
        "candidate getter failed",
    )?;
    if kind == 1 {
        require(
            (api.rank)(instance, candidate, &mut rank),
            "candidate rank failed",
        )?;
    }
    Ok((candidate, kind, rank))
}
