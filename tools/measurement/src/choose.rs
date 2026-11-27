//! The selection path in three anchored windows: a partial choose with the redraw after it,
//! the final choose with training, and the save.
//!
//! The calls are ibus-libpinyin's (libpinyin/ibus-libpinyin 4278882):
//! `LibPinyinCandidates::selectCandidate` (src/PYPLibPinyinCandidates.cc:146-172), then
//! `PhoneticEditor::update` (src/PYPPhoneticEditor.cc:346-361) with its candidate list
//! (src/PYPLibPinyinCandidates.cc:37-79), preedit (src/PYPPinyinEditor.cc:245-296) and
//! auxiliary text (src/PYPFullPinyinEditor.cc:109-135) in the default traditional style.
use crate::{Result, Usage, abi, output, platform, require, sentence};
use std::ffi::CString;
use std::hint::black_box;
use std::io::Write;
use std::path::Path;

/// ibus-libpinyin's default sort: phrase length, pinyin length, frequency (src/PYPConfig.cc:313).
const SORT: u32 = 0x1c;
const NBEST: i32 = 1;
const NORMAL: i32 = 2;
const ADDON: i32 = 6;
const LONGER: i32 = 7;
const WINDOWS: [&str; 3] = ["choose", "train", "save"];

struct Api {
    typing: abi::Typing,
    guess: abi::Guess,
    sentence: abi::Sentence,
    training: abi::Training,
    redraw: abi::Redraw,
    import: abi::Import,
    release: extern "C" fn(abi::Handle),
}

/// The return values of the calls in a window, oldest call in the highest bit.
#[derive(Default)]
struct Returns {
    bits: u32,
    count: u32,
}
impl Returns {
    fn push(&mut self, ok: bool) -> bool {
        self.bits = self.bits << 1 | u32::from(ok);
        self.count += 1;
        ok
    }
    fn write(&self, out: &mut impl Write) -> std::io::Result<()> {
        out.write_all(b"\"")?;
        for i in (0..self.count).rev() {
            out.write_all(if self.bits >> i & 1 == 1 { b"1" } else { b"0" })?;
        }
        out.write_all(b"\"")
    }
}

/// What a choose window returned: the new cursor, the parsed length after it, the lookup
/// cursor the redraw used and every other call's return value.
struct Chosen {
    cursor: i32,
    parsed: usize,
    begin: u16,
    returns: Returns,
}

#[inline(never)]
#[unsafe(no_mangle)]
fn bisect_choose_unit(
    api: &Api,
    instance: abi::Handle,
    candidate: abi::Handle,
    cursor: usize,
) -> Chosen {
    let _collection = platform::Collection::new();
    let mut returns = Returns::default();
    let chosen = (api.training.choose)(instance, 0, candidate);
    returns.push((api.sentence.guess)(instance));
    let parsed = (api.training.parsed)(instance);
    let mut begin = 0;
    if let Ok(at) = usize::try_from(chosen)
        && at != parsed
    {
        let mut rest = std::ptr::null_mut();
        returns.push((api.redraw.key_rest)(instance, at, &mut rest));
        returns.push((api.redraw.rest_positions)(
            instance,
            rest,
            &mut begin,
            std::ptr::null_mut(),
        ));
    }
    returns.push(api.guess.at(instance, usize::from(begin), SORT));
    list(api, instance, &mut returns);
    preedit(api, instance, cursor, &mut returns);
    let mut text = std::ptr::null_mut();
    returns.push((api.redraw.auxiliary)(instance, cursor, &mut text));
    drop(abi::StringValue::new(text, api.release));
    Chosen {
        cursor: chosen,
        parsed,
        begin,
        returns,
    }
}

/// `LibPinyinCandidates::processCandidates`: each candidate's type and text, and the user
/// flag of the kinds that have one.
fn list(api: &Api, instance: abi::Handle, returns: &mut Returns) {
    let mut count = 0;
    if !returns.push((api.typing.count)(instance, &mut count)) {
        return;
    }
    let mut ok = true;
    for index in 0..count {
        let mut candidate = std::ptr::null_mut();
        let mut kind = 0;
        let mut text = std::ptr::null();
        ok &= (api.sentence.candidate)(instance, index, &mut candidate)
            && (api.sentence.kind)(instance, candidate, &mut kind)
            && (api.sentence.text)(instance, candidate, &mut text);
        if matches!(kind, LONGER | NORMAL | ADDON) {
            black_box((api.redraw.user)(instance, candidate));
        }
        black_box(text);
    }
    returns.push(ok);
}

/// `PinyinEditor::updatePreeditText`: the rank-0 sentence when the first candidate is the
/// N-best match, and the character offset of the cursor in it.
fn preedit(api: &Api, instance: abi::Handle, cursor: usize, returns: &mut Returns) {
    let mut count = 0;
    if !returns.push((api.typing.count)(instance, &mut count)) || count == 0 {
        return;
    }
    let mut candidate = std::ptr::null_mut();
    let mut kind = 0;
    returns.push(
        (api.sentence.candidate)(instance, 0, &mut candidate)
            && (api.sentence.kind)(instance, candidate, &mut kind),
    );
    let mut text = std::ptr::null_mut();
    if kind == NBEST {
        returns.push((api.sentence.get)(instance, 0, &mut text));
    }
    let text = abi::StringValue::new(text, api.release);
    let mut pinyin_cursor = 0;
    returns.push((api.redraw.pinyin_offset)(
        instance,
        cursor,
        &mut pinyin_cursor,
    ));
    let mut offset = 0;
    returns.push((api.redraw.character_offset)(
        instance,
        text.pointer(),
        pinyin_cursor,
        &mut offset,
    ));
    black_box(offset);
}

#[inline(never)]
#[unsafe(no_mangle)]
fn bisect_train_unit(
    api: &Api,
    instance: abi::Handle,
    offset: usize,
    candidate: abi::Handle,
) -> Chosen {
    let _collection = platform::Collection::new();
    let mut returns = Returns::default();
    let chosen = (api.training.choose)(instance, offset, candidate);
    returns.push((api.sentence.guess)(instance));
    let parsed = (api.training.parsed)(instance);
    if usize::try_from(chosen).is_ok_and(|at| at == parsed) {
        let mut text = std::ptr::null_mut();
        returns.push((api.sentence.get)(instance, 0, &mut text));
        let text = abi::StringValue::new(text, api.release);
        returns.push((api.training.train)(instance, 0));
        drop(text);
    }
    Chosen {
        cursor: chosen,
        parsed,
        begin: 0,
        returns,
    }
}

#[inline(never)]
#[unsafe(no_mangle)]
fn bisect_save_unit(import: &abi::Import, context: abi::Handle) -> bool {
    let _collection = platform::Collection::new();
    (import.save)(context)
}

struct Args {
    input: CString,
    first: String,
    rest: String,
    iterations: usize,
}
fn arguments(args: &[String]) -> Result<Args> {
    let usage =
        || Usage("Usage: bisect --perf-choose SO DATA ASCII-PINYIN FIRST REST ITERATIONS".into());
    let [_, _, _, _, input, first, rest, iterations] = args else {
        return Err(usage().into());
    };
    let iterations = iterations
        .bytes()
        .all(|b| b.is_ascii_digit())
        .then(|| iterations.parse::<usize>().ok())
        .flatten()
        .filter(|n| (1..=100000).contains(n));
    let valid = !input.is_empty()
        && input.len() <= 255
        && input.bytes().all(|b| b.is_ascii_lowercase() || b == b'\'')
        && !first.is_empty()
        && !rest.is_empty();
    match iterations {
        Some(iterations) if valid => Ok(Args {
            input: CString::new(input.as_str())?,
            first: first.clone(),
            rest: rest.clone(),
            iterations,
        }),
        _ => Err(usage().into()),
    }
}

/// Per-window wall time and user-space instructions, one sample per iteration. A missing
/// counter sample drops that window's instruction samples: a partial total is not reported.
struct Clock {
    counter: Option<platform::Counter>,
    hw_errno: i32,
    elapsed: [Vec<u64>; 3],
    instructions: [Option<Vec<u64>>; 3],
}
impl Clock {
    fn new() -> Self {
        let mut hw_errno = 0;
        let counter = std::env::var_os("PERF_HW").and_then(|_| {
            platform::Counter::open()
                .map_err(|error| {
                    hw_errno = error
                        .downcast_ref::<std::io::Error>()
                        .and_then(|e| e.raw_os_error())
                        .unwrap_or(0);
                })
                .ok()
        });
        Self {
            counter,
            hw_errno,
            elapsed: Default::default(),
            instructions: [Some(Vec::new()), Some(Vec::new()), Some(Vec::new())],
        }
    }
    fn timed<T>(&mut self, window: usize, unit: impl FnOnce() -> T) -> T {
        if let Some(c) = &mut self.counter {
            c.enable();
        }
        let start = platform::now_ns();
        let value = unit();
        let end = platform::now_ns();
        let sample = self.counter.as_mut().and_then(platform::Counter::sample);
        self.elapsed[window].push(end.saturating_sub(start));
        self.instructions[window] =
            self.instructions[window]
                .take()
                .zip(sample)
                .map(|(mut samples, count)| {
                    samples.push(count);
                    samples
                });
        value
    }
    fn write(&self, out: &mut impl Write) -> std::io::Result<()> {
        out.write_all(b",\"windows\":{")?;
        for (i, name) in WINDOWS.iter().enumerate() {
            if i != 0 {
                out.write_all(b",")?;
            }
            let total: u64 = self.elapsed[i].iter().sum();
            write!(out, "\"{name}\":{{\"elapsed_ns\":{total},\"samples_ns\":")?;
            output::numbers(out, &self.elapsed[i])?;
            match (&self.counter, &self.instructions[i]) {
                (Some(_), Some(samples)) => {
                    let total: u64 = samples.iter().sum();
                    write!(out, ",\"hw_instructions\":{total},\"samples_hw\":")?;
                    output::numbers(out, samples)?;
                }
                _ if std::env::var_os("PERF_HW").is_some() => {
                    write!(out, ",\"hw_errno\":{}", self.hw_errno)?;
                }
                _ => {}
            }
            out.write_all(b"}")?;
        }
        out.write_all(b"}")
    }
}

/// The first normal candidate whose text is `phrase`. Content, not position, selects it,
/// so that every library chooses the same phrase.
fn find(api: &Api, instance: abi::Handle, phrase: &str) -> Result<abi::Handle> {
    let mut count = 0;
    require(
        (api.typing.count)(instance, &mut count),
        "candidate count failed",
    )?;
    for index in 0..count {
        let (candidate, kind, _) = sentence::candidate_info(&api.sentence, instance, index)?;
        let mut text = std::ptr::null();
        require(
            (api.sentence.text)(instance, candidate, &mut text),
            "candidate text failed",
        )?;
        if kind == NORMAL && abi::text(text)? == Some(phrase) {
            return Ok(candidate);
        }
    }
    Err(format!("no normal candidate {phrase}").into())
}

/// A fresh user directory: empty, or a copy of the plain files of `PERF_USER_DIR`.
fn profile(user: &Path) -> Result<()> {
    let Some(source) = std::env::var_os("PERF_USER_DIR").filter(|path| !path.is_empty()) else {
        return Ok(());
    };
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        require(
            entry.file_type()?.is_file(),
            "PERF_USER_DIR must hold only plain files",
        )?;
        std::fs::copy(entry.path(), user.join(entry.file_name()))?;
    }
    Ok(())
}

impl Api {
    fn load(library: &abi::Library) -> Result<Self> {
        Ok(Self {
            typing: abi::Typing::load(library, "pinyin")?,
            guess: abi::Guess::load(library, "pinyin")?,
            sentence: abi::Sentence::load(library, "pinyin")?,
            training: abi::Training::load(library, "pinyin")?,
            redraw: abi::Redraw::load(library, "pinyin")?,
            import: abi::Import::load(library, "pinyin")?,
            release: library.release()?,
        })
    }
}

/// One iteration's subject state. The fields drop in order: instance, context, profile.
struct Session<'a> {
    instance: abi::Instance<'a>,
    context: abi::Context<'a>,
    _user: abi::UserDir,
}
impl<'a> Session<'a> {
    /// Open a fresh profile, then type and guess the whole input, all outside the windows.
    fn open(api: &'a Api, core: &'a abi::Core, data: &str, input: &CString) -> Result<Self> {
        let user = abi::UserDir::temporary("bisect-choose-")?;
        profile(user.path())?;
        let paths = abi::ContextPaths::new(data, user.path())?;
        let context = abi::Context::open(core, &paths)?;
        require(
            (api.typing.options)(context.handle, 0x18a),
            "set_options failed",
        )?;
        let instance = abi::Instance::open(&api.typing, context.handle)?;
        let handle = instance.handle;
        require(
            (api.typing.reset)(handle)
                && (api.typing.parse)(handle, input.as_ptr()) != 0
                && (api.training.parsed)(handle) == input.as_bytes().len()
                && (api.sentence.guess)(handle)
                && api.guess.at(handle, 0, SORT),
            "the input does not parse and guess in full",
        )?;
        Ok(Self {
            instance,
            context,
            _user: user,
        })
    }
}

/// Window 1, then its observation and the candidate list it left. Returns the lookup cursor.
fn partial(
    out: &mut impl Write,
    api: &Api,
    handle: abi::Handle,
    args: &Args,
    clock: &mut Clock,
) -> Result<u16> {
    let first = find(api, handle, &args.first)?;
    let length = args.input.as_bytes().len();
    let chosen = clock.timed(0, || bisect_choose_unit(api, handle, first, length));
    require(
        usize::try_from(chosen.cursor).is_ok_and(|at| at > 0 && at < chosen.parsed),
        "the partial choose did not stop inside the input",
    )?;
    out.write_all(b"{\"first\":")?;
    observe(out, &args.first, &chosen)?;
    write!(out, ",\"begin\":{},\"candidates\":[", chosen.begin)?;
    let mut count = 0;
    require(
        (api.typing.count)(handle, &mut count),
        "candidate count failed",
    )?;
    for i in 0..count {
        if i != 0 {
            out.write_all(b",")?;
        }
        sentence::candidate(out, &api.sentence, handle, i)?;
    }
    out.write_all(b"]")?;
    Ok(chosen.begin)
}

/// Window 2 at the lookup cursor, then its observation and the rank-0 sentence.
fn finish(
    out: &mut impl Write,
    api: &Api,
    handle: abi::Handle,
    args: &Args,
    clock: &mut Clock,
    begin: u16,
) -> Result<()> {
    let second = find(api, handle, &args.rest)?;
    let chosen = clock.timed(1, || {
        bisect_train_unit(api, handle, usize::from(begin), second)
    });
    require(
        usize::try_from(chosen.cursor).is_ok_and(|at| at == chosen.parsed),
        "the final choose did not reach the end of the input",
    )?;
    out.write_all(b",\"rest\":")?;
    observe(out, &args.rest, &chosen)?;
    let mut text = std::ptr::null_mut();
    let got = (api.sentence.get)(handle, 0, &mut text);
    let text = abi::StringValue::new(text, api.release);
    out.write_all(b",\"sentence\":")?;
    output::nullable(out, if got { text.text()? } else { None })?;
    Ok(())
}

/// One iteration from a fresh profile: the three windows, each checked and observed after
/// it ends.
fn iteration(
    out: &mut impl Write,
    api: &Api,
    core: &abi::Core,
    data: &str,
    args: &Args,
    clock: &mut Clock,
) -> Result<()> {
    let session = Session::open(api, core, data, &args.input)?;
    let handle = session.instance.handle;
    let begin = partial(out, api, handle, args, clock)?;
    finish(out, api, handle, args, clock, begin)?;
    let saved = clock.timed(2, || bisect_save_unit(&api.import, session.context.handle));
    require(saved, "pinyin_save failed")?;
    out.write_all(b",\"saved\":true}")?;
    Ok(())
}

fn observe(out: &mut impl Write, phrase: &str, chosen: &Chosen) -> Result<()> {
    out.write_all(b"{\"text\":")?;
    output::string(out, phrase)?;
    write!(
        out,
        ",\"cursor\":{},\"parsed\":{},\"returns\":",
        chosen.cursor, chosen.parsed
    )?;
    chosen.returns.write(out)?;
    out.write_all(b"}")?;
    Ok(())
}

pub(crate) fn run(args: &[String]) -> Result<()> {
    let parsed = arguments(args)?;
    let library = abi::Library::open(&args[2])?;
    let core = abi::Core::load(&library, "pinyin")?;
    let api = Api::load(&library)?;
    let mut clock = Clock::new();
    let mut out = std::io::stdout().lock();
    write!(
        out,
        "{{\"pid\":{},\"workload\":\"choose\",\"input\":",
        std::process::id()
    )?;
    output::string(&mut out, args[4].as_str())?;
    write!(
        out,
        ",\"iterations\":{},\"observations\":[",
        parsed.iterations
    )?;
    for i in 0..parsed.iterations {
        if i != 0 {
            out.write_all(b",")?;
        }
        iteration(&mut out, &api, &core, &args[3], &parsed, &mut clock)?;
    }
    out.write_all(b"]")?;
    clock.write(&mut out)?;
    writeln!(out, "}}")?;
    Ok(())
}
