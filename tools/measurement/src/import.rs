//! Import/save phases, full fixture modes and retained unigram stops.
use crate::{Result, Usage, abi, platform, require, sentence};
use std::ffi::CString;
use std::io::Write;
use std::path::Path;

struct FixtureRow {
    input: CString,
}
struct PhraseRow {
    library: u8,
    phrase: CString,
    pinyin: CString,
    count: i32,
}
/// What the arguments select, with its input files parsed. None of it needs the loaded library.
enum Input {
    Import(u32),
    Cold,
    Export,
    Sentence(Vec<FixtureRow>, bool),
    Phrases(Vec<PhraseRow>),
    Bigrams(Vec<FixtureRow>),
}
enum Mode {
    Import(u32, abi::Import),
    Cold(abi::Typing),
    Sentence(
        Vec<FixtureRow>,
        bool,
        abi::Typing,
        abi::Guess,
        abi::Sentence,
        abi::Training,
        abi::Import,
    ),
    Phrases(Vec<PhraseRow>, abi::Import, abi::Export),
    Export(abi::Export),
    Bigrams(
        Vec<FixtureRow>,
        abi::Typing,
        abi::Guess,
        abi::Sentence,
        abi::Training,
        abi::Bigram,
    ),
}

pub(crate) fn run(args: &[String]) -> Result<()> {
    // #lizard forgives(cyclomatic_complexity)
    // The exhaustive mode dispatch keeps the shared open and close boundaries in one place.
    if !(4..=6).contains(&args.len()) {
        return Err(Usage(String::new()).into());
    }
    if args.len() == 6 && matches!(args[4].as_str(), "--unigram-pinyin" | "--unigram-zhuyin") {
        return unigram(args);
    }
    let user = Path::new(&args[3]);
    platform::fresh_user(user)?;
    // Everything that needs no loaded library comes first: the input files, stdout and the
    // context paths. The base C driver read its files only after `init`, so none of this
    // belongs to the open phase.
    let input = parse(args)?;
    let mut out = std::io::stdout().lock();
    let paths = abi::ContextPaths::new(&args[2], user)?;
    // Callgrind starts with collection on. Discard what ran before the library load, the
    // Rust runtime and the driver's start-up, and collect the open phase from the load
    // through `pinyin_init`: relocation, symbol lookup and context initialization.
    platform::profiling_reset();
    let open = platform::Collection::new();
    let library = abi::Library::open(&args[1])?;
    let core = abi::Core::load(&library, "pinyin")?;
    let mode = prepare(&library, input)?;
    let release = match &mode {
        Mode::Import(..) | Mode::Cold(..) => None,
        _ => Some(library.release()?),
    };
    let ctx = abi::Context::open(&core, &paths)?;
    // The open collection ends here; each later phase scopes its own.
    drop(open);
    match &mode {
        Mode::Import(iterations, api) => {
            platform::dump(c"opened");
            {
                let _collection = platform::Collection::new();
                for lib in 1..=4 {
                    let iterator = (api.begin)(ctx.handle, lib);
                    for _ in 0..*iterations {
                        require(
                            (api.add)(iterator, c"测试".as_ptr(), c"ce4'shi4".as_ptr(), 5)
                                && (api.add)(iterator, c"测侧".as_ptr(), c"ce4'ce4".as_ptr(), 5),
                            "import failed",
                        )?;
                    }
                    (api.end)(iterator);
                }
            }
            platform::dump(c"imported");
            {
                let _collection = platform::Collection::new();
                require((api.save)(ctx.handle), "save failed")?;
            }
            platform::dump(c"saved");
        }
        Mode::Cold(typing) => {
            let _collection = platform::Collection::new();
            require((typing.options)(ctx.handle, 0x18a), "set_options failed")?;
            let instance = abi::Instance::open(typing, ctx.handle)?;
            drop(instance);
        }
        Mode::Sentence(rows, training, typing, guess, api, train, import) => {
            fixture(
                &mut out,
                ctx.handle,
                rows,
                *training,
                typing,
                guess,
                api,
                train,
                import,
                release.ok_or("missing string allocator")?,
            )?;
        }
        Mode::Phrases(rows, api, export) => phrase_file(
            &mut out,
            ctx.handle,
            rows,
            api,
            export,
            release.ok_or("missing string allocator")?,
        )?,
        Mode::Export(api) => {
            for lib in 1..=4 {
                collected(|| (api.load)(ctx.handle, lib));
                export_walk(
                    &mut out,
                    ctx.handle,
                    api,
                    lib as u32,
                    release.ok_or("missing string allocator")?,
                )?;
            }
        }
        Mode::Bigrams(rows, typing, guess, api, train, export) => {
            choose_and_train(&mut out, ctx.handle, rows, typing, guess, api, train)?;
            bigrams(
                &mut out,
                ctx.handle,
                export,
                release.ok_or("missing string allocator")?,
            )?;
        }
    }
    {
        let _collection = platform::Collection::new();
        drop(ctx);
        drop(library);
    }
    Ok(())
}

/// Run one library call with collection on. The driver's own work around it stays outside.
fn collected<T>(call: impl FnOnce() -> T) -> T {
    let _collection = platform::Collection::new();
    call()
}

/// Select the mode and read its input files. The library is not loaded yet.
fn parse(args: &[String]) -> Result<Input> {
    let mode = args.get(4).map(String::as_str);
    match mode {
        Some("--cold-open") if args.len() == 5 => Ok(Input::Cold),
        Some("--export-system") if args.len() == 5 => Ok(Input::Export),
        Some("--sentence-fixture" | "--train-fixture") if args.len() == 6 => {
            sentence_input(&args[5], mode == Some("--train-fixture"))
        }
        Some("--phrase-file") if args.len() == 6 => Ok(Input::Phrases(phrase_rows(&args[5])?)),
        Some("--train-export-bigrams") if args.len() == 6 => {
            Ok(Input::Bigrams(fixture_rows(&args[5], Some(200))?))
        }
        _ if args.len() < 6 => Ok(Input::Import(import_iterations(mode))),
        _ => Err(Usage(String::new()).into()),
    }
}

fn sentence_input(path: &str, training: bool) -> Result<Input> {
    let limit = if training { Some(200) } else { None };
    Ok(Input::Sentence(fixture_rows(path, limit)?, training))
}

/// Resolve the symbols that the selected mode calls. This is the only work that needs the library.
fn prepare(library: &abi::Library, input: Input) -> Result<Mode> {
    match input {
        Input::Import(iterations) => import_mode(library, iterations),
        Input::Cold => cold_mode(library),
        Input::Export => export_mode(library),
        Input::Sentence(rows, training) => sentence_mode(library, rows, training),
        Input::Phrases(rows) => phrase_mode(library, rows),
        Input::Bigrams(rows) => bigram_mode(library, rows),
    }
}

fn cold_mode(library: &abi::Library) -> Result<Mode> {
    Ok(Mode::Cold(abi::Typing::load(library, "pinyin")?))
}

fn export_mode(library: &abi::Library) -> Result<Mode> {
    Ok(Mode::Export(abi::Export::load(library, "pinyin")?))
}

fn import_mode(library: &abi::Library, iterations: u32) -> Result<Mode> {
    Ok(Mode::Import(
        iterations,
        abi::Import::load(library, "pinyin")?,
    ))
}

fn sentence_mode(library: &abi::Library, rows: Vec<FixtureRow>, training: bool) -> Result<Mode> {
    Ok(Mode::Sentence(
        rows,
        training,
        abi::Typing::load(library, "pinyin")?,
        abi::Guess::load(library, "pinyin")?,
        abi::Sentence::load(library, "pinyin")?,
        abi::Training::load(library, "pinyin")?,
        abi::Import::load(library, "pinyin")?,
    ))
}

fn phrase_mode(library: &abi::Library, rows: Vec<PhraseRow>) -> Result<Mode> {
    Ok(Mode::Phrases(
        rows,
        abi::Import::load(library, "pinyin")?,
        abi::Export::load(library, "pinyin")?,
    ))
}

fn bigram_mode(library: &abi::Library, rows: Vec<FixtureRow>) -> Result<Mode> {
    Ok(Mode::Bigrams(
        rows,
        abi::Typing::load(library, "pinyin")?,
        abi::Guess::load(library, "pinyin")?,
        abi::Sentence::load(library, "pinyin")?,
        abi::Training::load(library, "pinyin")?,
        abi::Bigram::load(library, "pinyin")?,
    ))
}

/// strtoul's historical numeric prefix and unsigned narrowing.
fn import_iterations(mode: Option<&str>) -> u32 {
    mode.map(|s| {
        let s = s.trim_start();
        let s = s.strip_prefix('+').unwrap_or(s);
        let digits = s.bytes().take_while(u8::is_ascii_digit).count();
        s[..digits].parse::<u64>().unwrap_or(0) as u32
    })
    .unwrap_or(1000)
}

fn fixture_rows(path: &str, limit: Option<usize>) -> Result<Vec<FixtureRow>> {
    let content = std::fs::read(path)?;
    let mut rows = Vec::new();
    for line in content.split_inclusive(|b| *b == b'\n') {
        if line.starts_with(b"#") || line.starts_with(b"\n") {
            continue;
        }
        let mut fields = line.split(|b| *b == b'\t');
        let input = fields.next().ok_or("missing fixture input")?;
        let expected = fields.next();
        let surface = fields.next();
        require(
            expected.is_some() && surface.is_some(),
            "missing fixture fields",
        )?;
        if surface.is_none_or(|s| s.is_empty()) {
            continue;
        }
        if limit == Some(rows.len()) {
            break;
        }
        rows.push(FixtureRow {
            input: CString::new(input)?,
        });
    }
    Ok(rows)
}

fn phrase_rows(path: &str) -> Result<Vec<PhraseRow>> {
    let content = std::fs::read_to_string(path)?;
    let mut rows = Vec::new();
    for line in content.split_inclusive('\n') {
        require(!line.contains('\0'), "phrase row contains NUL")?;
        let mut fields = line.splitn(4, '\t');
        let library = fields.next().ok_or("missing library")?;
        require(
            !library.is_empty() && library.bytes().all(|b| b.is_ascii_digit()),
            "invalid library",
        )?;
        let library = library.parse::<u8>()?;
        require((1..=4).contains(&library), "invalid library")?;
        let phrase = CString::new(fields.next().ok_or("missing phrase")?)?;
        let pinyin = CString::new(fields.next().ok_or("missing pinyin")?)?;
        let raw = fields.next().ok_or("missing frequency")?;
        let raw = raw
            .strip_suffix("\r\n")
            .or_else(|| raw.strip_suffix('\n'))
            .unwrap_or(raw);
        let count = raw.trim_start().parse::<i32>()?;
        rows.push(PhraseRow {
            library,
            phrase,
            pinyin,
            count,
        });
    }
    Ok(rows)
}

fn export_walk(
    out: &mut impl Write,
    ctx: abi::Handle,
    api: &abi::Export,
    library: u32,
    release: extern "C" fn(abi::Handle),
) -> Result<u32> {
    let iterator = collected(|| (api.begin)(ctx, library));
    require(!iterator.is_null(), "export begin failed")?;
    let mut count = 0;
    loop {
        // The advance and the read are library work; releasing and printing the row are not.
        let collection = platform::Collection::new();
        if !(api.next)(iterator) {
            break;
        }
        let mut phrase = std::ptr::null_mut();
        let mut pinyin = std::ptr::null_mut();
        let mut frequency = -1;
        let ok = (api.get)(iterator, &mut phrase, &mut pinyin, &mut frequency);
        drop(collection);
        let phrase = abi::StringValue::new(phrase, release);
        let pinyin = abi::StringValue::new(pinyin, release);
        require(ok, "export get failed")?;
        writeln!(
            out,
            "{library}\t{}\t{}\t{frequency}",
            phrase.text()?.ok_or("export get failed")?,
            pinyin.text()?.ok_or("export get failed")?
        )?;
        // The library allocated both strings; its allocator frees them.
        collected(|| (drop(phrase), drop(pinyin)));
        count += 1;
    }
    collected(|| (api.end)(iterator));
    Ok(count)
}

fn phrase_file(
    out: &mut impl Write,
    ctx: abi::Handle,
    rows: &[PhraseRow],
    api: &abi::Import,
    export: &abi::Export,
    release: extern "C" fn(abi::Handle),
) -> Result<()> {
    let mut touched = [false; 5];
    {
        // The imports and the save are library calls end to end, so one scope holds them.
        let _collection = platform::Collection::new();
        let mut iterator: abi::Handle = std::ptr::null_mut();
        let mut active = 0;
        for row in rows {
            if active != row.library {
                if !iterator.is_null() {
                    (api.end)(iterator);
                }
                (export.load)(ctx, row.library);
                iterator = (api.begin)(ctx, row.library);
                active = row.library;
                touched[row.library as usize] = true;
                require(!iterator.is_null(), "import begin failed")?;
            }
            require(
                (api.add)(
                    iterator,
                    row.phrase.as_ptr(),
                    row.pinyin.as_ptr(),
                    row.count,
                ),
                "import failed",
            )?;
        }
        if !iterator.is_null() {
            (api.end)(iterator);
        }
        require(
            rows.len() == 10000 && (api.save)(ctx),
            "phrase-file workload failed",
        )?;
    }
    let mut exported = 0;
    for lib in 1..=4 {
        if touched[lib as usize] {
            exported += export_walk(out, ctx, export, lib, release)?;
        }
    }
    writeln!(out, "operations\t{}\t{exported}", rows.len())?;
    Ok(())
}

#[expect(
    clippy::too_many_arguments,
    reason = "One fixture replay shares preloaded ABI tables without allocations"
)]
fn fixture(
    out: &mut impl Write,
    ctx: abi::Handle,
    rows: &[FixtureRow],
    training: bool,
    typing: &abi::Typing,
    guess: &abi::Guess,
    api: &abi::Sentence,
    train: &abi::Training,
    import: &abi::Import,
    release: extern "C" fn(abi::Handle),
) -> Result<()> {
    require(
        collected(|| (typing.options)(ctx, 0x18a)),
        "set_options failed",
    )?;
    let instance = collected(|| abi::Instance::open(typing, ctx))?;
    let replay = Replay {
        instance: instance.handle,
        typing,
        guess,
        api,
        train,
        release,
    };
    let mut trained = 0;
    for (index, row) in rows.iter().enumerate() {
        let collection = platform::Collection::new();
        let (parsed, length) = replay.parse(row)?;
        if training {
            replay.train_row(out, collection, index, row, (parsed, length))?;
            trained += 1;
        } else {
            replay.sentence_row(out, collection, row, length)?;
        }
    }
    require(
        rows.len() == if training { 200 } else { 500 },
        "wrong fixture operation count",
    )?;
    if training {
        require(collected(|| (import.save)(ctx)), "save failed")?;
    }
    collected(|| drop(instance));
    writeln!(out, "operations\t{}\t{trained}", rows.len())?;
    Ok(())
}

/// The ABI tables and instance one fixture replay shares across its rows.
struct Replay<'a> {
    instance: abi::Handle,
    typing: &'a abi::Typing,
    guess: &'a abi::Guess,
    api: &'a abi::Sentence,
    train: &'a abi::Training,
    release: extern "C" fn(abi::Handle),
}
impl Replay<'_> {
    /// Reset and parse one row: the parse result and the consumed length.
    fn parse(&self, row: &FixtureRow) -> Result<(usize, usize)> {
        require((self.typing.reset)(self.instance), "reset failed")?;
        let parsed = (self.typing.parse)(self.instance, row.input.as_ptr());
        Ok((parsed, (self.train.parsed)(self.instance)))
    }

    fn train_row(
        &self,
        out: &mut impl Write,
        collection: platform::Collection,
        index: usize,
        row: &FixtureRow,
        (parsed, length): (usize, usize),
    ) -> Result<()> {
        if length != row.input.as_bytes().len() {
            return Err(format!(
                "train_top parse precondition failed row={} input={} parsed={length} expected={}",
                index + 1,
                row.input.to_str()?,
                row.input.as_bytes().len()
            )
            .into());
        }
        let guessed = (self.api.guess)(self.instance);
        if !guessed || !(self.train.train)(self.instance, 0) {
            return Err(format!(
                "train_top guess/train failed row={} input={} guess={}",
                index + 1,
                row.input.to_str()?,
                u8::from(guessed)
            )
            .into());
        }
        drop(collection);
        let value = get_sentence(self.api, self.instance, 0, self.release)?;
        writeln!(
            out,
            "train\t{}\t{parsed}\t{length}\t1\t{}",
            row.input.to_str()?,
            value.text()?.ok_or("missing sentence")?
        )?;
        collected(|| drop(value));
        Ok(())
    }

    fn sentence_row(
        &self,
        out: &mut impl Write,
        collection: platform::Collection,
        row: &FixtureRow,
        length: usize,
    ) -> Result<()> {
        require(length != 0, "empty fixture parse")?;
        let guessed = (self.api.guess)(self.instance);
        require(self.guess.call(self.instance), "guess candidates failed")?;
        drop(collection);
        sentence_row(
            out,
            self.typing,
            self.api,
            self.instance,
            &row.input,
            guessed,
            self.release,
        )
    }
}
fn get_sentence(
    api: &abi::Sentence,
    instance: abi::Handle,
    rank: u8,
    release: extern "C" fn(abi::Handle),
) -> Result<abi::StringValue> {
    let mut value = std::ptr::null_mut();
    let ok = collected(|| (api.get)(instance, rank, &mut value));
    let value = abi::StringValue::new(value, release);
    require(ok, "get sentence failed")?;
    Ok(value)
}
fn sentence_row(
    out: &mut impl Write,
    typing: &abi::Typing,
    api: &abi::Sentence,
    instance: abi::Handle,
    input: &std::ffi::CStr,
    guessed: bool,
    release: extern "C" fn(abi::Handle),
) -> Result<()> {
    let mut count = 0;
    require(
        collected(|| (typing.count)(instance, &mut count)),
        "get candidate count failed",
    )?;
    let proven = proven_rank(api, instance, count)?;
    write!(out, "{}\t{guessed}\t", input.to_str()?)?;
    sentence_cells(out, api, instance, proven, release)?;
    out.write_all(b"\t")?;
    candidate_cells(out, api, instance, count)?;
    out.write_all(b"\n")?;
    Ok(())
}

fn proven_rank(api: &abi::Sentence, instance: abi::Handle, count: u32) -> Result<u8> {
    let mut proven = 0;
    for i in 0..count {
        let (_, kind, rank) = collected(|| sentence::candidate_info(api, instance, i))?;
        if kind == 1 {
            proven = proven.max(rank);
        }
    }
    Ok(proven)
}

fn sentence_cells(
    out: &mut impl Write,
    api: &abi::Sentence,
    instance: abi::Handle,
    proven: u8,
    release: extern "C" fn(abi::Handle),
) -> Result<()> {
    for i in 0..=proven.min(2) {
        let mut raw = std::ptr::null_mut();
        let ok = collected(|| (api.get)(instance, i, &mut raw));
        let value = abi::StringValue::new(raw, release);
        if i != 0 {
            out.write_all(&[1])?;
        }
        write!(
            out,
            "{}",
            if ok {
                value.text()?.unwrap_or("-")
            } else {
                "-"
            }
        )?;
        collected(|| drop(value));
    }
    Ok(())
}

fn candidate_cells(
    out: &mut impl Write,
    api: &abi::Sentence,
    instance: abi::Handle,
    count: u32,
) -> Result<()> {
    for i in 0..count.min(6) {
        let (candidate, kind, rank) = collected(|| sentence::candidate_info(api, instance, i))?;
        let mut text = std::ptr::null();
        require(
            collected(|| (api.text)(instance, candidate, &mut text)),
            "candidate text failed",
        )?;
        if i != 0 {
            out.write_all(&[1])?;
        }
        let text = abi::text(text)?.ok_or("candidate text is NULL")?;
        if kind == 1 {
            write!(out, "n/{rank}/{text}")?;
        } else {
            write!(out, "{}/-/{text}", kind_tag(kind))?;
        }
    }
    Ok(())
}

fn kind_tag(kind: i32) -> char {
    match kind {
        2 => 'N',
        6 => 'a',
        _ => '?',
    }
}

fn choose_and_train(
    out: &mut impl Write,
    ctx: abi::Handle,
    rows: &[FixtureRow],
    typing: &abi::Typing,
    guess: &abi::Guess,
    api: &abi::Sentence,
    train: &abi::Training,
) -> Result<()> {
    require(
        collected(|| (typing.options)(ctx, 0x18a)),
        "set_options failed",
    )?;
    let instance = collected(|| abi::Instance::open(typing, ctx))?;
    let mut trained = 0;
    for row in rows {
        let _collection = platform::Collection::new();
        if !(typing.reset)(instance.handle)
            || (typing.parse)(instance.handle, row.input.as_ptr()) == 0
            || !(api.guess)(instance.handle)
            || !guess.call(instance.handle)
        {
            continue;
        }
        let mut count = 0;
        require(
            (typing.count)(instance.handle, &mut count),
            "candidate count failed",
        )?;
        for i in 0..count {
            let (candidate, kind, _) = sentence::candidate_info(api, instance.handle, i)?;
            if kind == 2 {
                (train.choose)(instance.handle, 0, candidate);
                if (api.guess)(instance.handle) && (train.train)(instance.handle, 0) {
                    trained += 1;
                }
                break;
            }
        }
    }
    collected(|| drop(instance));
    writeln!(out, "chosen-and-trained\t{}\t{trained}", rows.len())?;
    require(trained != 0, "no trained rows")
}
fn bigrams(
    out: &mut impl Write,
    ctx: abi::Handle,
    api: &abi::Bigram,
    release: extern "C" fn(abi::Handle),
) -> Result<()> {
    let iterator = collected(|| (api.begin)(ctx));
    require(!iterator.is_null(), "bigram export begin failed")?;
    let mut rows = 0;
    loop {
        // The advance and the read are library work; releasing and printing the row are not.
        let collection = platform::Collection::new();
        if !(api.next)(iterator) {
            break;
        }
        let mut phrase = std::ptr::null_mut();
        let mut pinyin = std::ptr::null_mut();
        let mut count = 0;
        (api.get)(iterator, &mut phrase, &mut pinyin, &mut count);
        drop(collection);
        let phrase = abi::StringValue::new(phrase, release);
        let pinyin = abi::StringValue::new(pinyin, release);
        writeln!(
            out,
            "bigram\t{}\t{}\t{count}",
            phrase.text()?.unwrap_or("-"),
            pinyin.text()?.unwrap_or("-")
        )?;
        collected(|| (drop(phrase), drop(pinyin)));
        rows += 1;
    }
    collected(|| (api.end)(iterator));
    writeln!(out, "bigram-rows\t{rows}")?;
    require(rows != 0, "empty bigram export")
}

const STOPS: [&str; 8] = [
    "lib-load",
    "init",
    "ready",
    "increments",
    "dirty-zero",
    "dirty-increments",
    "save-zero",
    "save-increments",
];

fn unigram(args: &[String]) -> Result<()> {
    let stop = args[5].as_str();
    if !STOPS.contains(&stop) {
        return Err(Usage(String::new()).into());
    }
    let facade = if args[4] == "--unigram-pinyin" {
        "pinyin"
    } else {
        "zhuyin"
    };
    let library = abi::Library::open(&args[1])?;
    let core = abi::Core::load(&library, facade)?;
    let typing = abi::Typing::load(&library, facade)?;
    let unigram = abi::Unigram::load(&library, facade)?;
    let import = abi::Import::load(&library, facade)?;
    let mut out = std::io::stdout().lock();
    if stop == "lib-load" {
        return retain(&mut out, format_args!("loaded"));
    }
    let paths = abi::ContextPaths::new(&args[2], Path::new(&args[3]))?;
    let ctx = abi::Context::open(&core, &paths)?;
    if stop == "init" {
        return retain(&mut out, format_args!("initialized"));
    }
    let instance = abi::Instance::open(&typing, ctx.handle)?;
    let totals = unigram_stop(
        &unigram,
        &import,
        (ctx.handle, instance.handle),
        facade,
        stop,
    )?;
    retain(
        &mut out,
        format_args!(
            "increments={} before={} after={} saved={}",
            totals.count,
            totals.before,
            totals.after,
            u8::from(totals.saved)
        ),
    )
}

/// Write one line, flush it and exit at once, so every live object stays allocated.
fn retain(out: &mut impl Write, line: std::fmt::Arguments<'_>) -> Result<()> {
    writeln!(out, "{line}")?;
    out.flush()?;
    platform::immediate_exit()
}

struct Totals {
    count: u32,
    before: u32,
    after: u32,
    saved: bool,
}

fn unigram_stop(
    unigram: &abi::Unigram,
    import: &abi::Import,
    (ctx, instance): (abi::Handle, abi::Handle),
    facade: &str,
    stop: &str,
) -> Result<Totals> {
    let token = 0x01000001;
    let mut before = 0;
    let mut after = 0;
    require(
        (unigram.get)(instance, token, &mut before),
        "unigram get failed",
    )?;
    let count = if stop.contains("increments") { 1000 } else { 0 };
    for _ in 0..count {
        require(
            (unigram.increment)(instance, token, 1),
            "unigram increment failed",
        )?;
    }
    if stop.starts_with("dirty-") || stop.starts_with("save-") {
        dirty_import(import, ctx, facade)?;
    }
    let saved = stop.starts_with("save-");
    require(
        (!saved || (import.save)(ctx))
            && (unigram.get)(instance, token, &mut after)
            && after == before.wrapping_add(count),
        "unigram stop failed",
    )?;
    Ok(Totals {
        count,
        before,
        after,
        saved,
    })
}

fn dirty_import(import: &abi::Import, ctx: abi::Handle, facade: &str) -> Result<()> {
    let iterator = (import.begin)(ctx, 7);
    let pinyin = if facade == "pinyin" {
        c"ni3'hao3"
    } else {
        c"ㄋㄧˇ ㄏㄠˇ"
    };
    require(
        !iterator.is_null() && (import.add)(iterator, c"你好".as_ptr(), pinyin.as_ptr(), 1),
        "dirty import failed",
    )?;
    (import.end)(iterator);
    Ok(())
}
