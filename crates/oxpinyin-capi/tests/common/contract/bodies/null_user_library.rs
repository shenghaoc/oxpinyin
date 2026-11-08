//! The null_user_library family's case bodies: each answers the result document the old case returned.
//!
//! Null user and library operations (inventory row 33, #642, #1175).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::facade;
use crate::readers::{c, lookup};
use crate::support::{J, Kit, P, UNTOUCHED, borrowed_text, take_text};

/// One file's mode bits and contents.
type Entry = (u32, Vec<u8>);

/// `transient_files`: every file under `root` by relative path, with its mode
/// bits and bytes (symlinks followed).
fn transient_files(root: &Path) -> BTreeMap<String, Entry> {
    let mut files = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).expect("read a canary directory") {
            let path = entry.expect("canary entry").path();
            if path.is_dir() {
                pending.push(path);
            } else if path.is_file() {
                let mode = file_mode(&path);
                let relative = path.strip_prefix(root).expect("under the root");
                let bytes = std::fs::read(&path).expect("read a canary file");
                files.insert(relative.to_string_lossy().into_owned(), (mode, bytes));
            }
        }
    }
    files
}

/// A file's mode bits (`st_mode`).
#[cfg(unix)]
fn file_mode(path: &Path) -> u32 {
    std::os::unix::fs::MetadataExt::mode(&std::fs::metadata(path).expect("stat a canary file"))
}

/// A file's read-only bit, where there are no mode bits.
#[cfg(not(unix))]
fn file_mode(path: &Path) -> u32 {
    u32::from(
        std::fs::metadata(path)
            .expect("stat a canary file")
            .permissions()
            .readonly(),
    )
}

/// The canary directories and their baseline snapshots.
struct Canaries {
    roots: [PathBuf; 2],
    baseline: [BTreeMap<String, Entry>; 2],
}

impl Canaries {
    /// `checkpoint`: per directory, the sorted names whose entry changed.
    fn checkpoint(&self, out: &mut J, label: &str) {
        let changed: Vec<Vec<String>> = self
            .roots
            .iter()
            .zip(&self.baseline)
            .map(|(root, before)| {
                let after = transient_files(root);
                let names: std::collections::BTreeSet<&String> =
                    before.keys().chain(after.keys()).collect();
                names
                    .into_iter()
                    .filter(|name| before.get(*name) != after.get(*name))
                    .cloned()
                    .collect()
            })
            .collect();
        out.set(format!("files-{label}"), changed);
    }
}

/// Creates the cwd and TMPDIR canaries, enters cwd and points TMPDIR at tmp.
fn canaries(k: &Kit) -> Canaries {
    let roots = [k.scratch.join("cwd-X"), k.scratch.join("tmp-X")];
    for directory in &roots {
        std::fs::create_dir(directory).expect("a canary directory");
        std::fs::write(directory.join("canary"), b"unchanged").expect("write the canary");
    }
    std::env::set_current_dir(&roots[0]).expect("enter cwd");
    // SAFETY: the case child runs one test on one thread; nothing reads the
    // environment concurrently.
    unsafe { std::env::set_var("TMPDIR", &roots[1]) };
    let baseline = [transient_files(&roots[0]), transient_files(&roots[1])];
    Canaries { roots, baseline }
}

/// The facade's train of n-best result 0.
fn train(inst: P) -> bool {
    facade::train_top(inst)
}

/// The pinyin facade's indexed `train`, named in a body both facades compile:
/// `zhuyin_train` takes no index, and only pinyin cases reach this. The marker
/// picks the implementation from the symbol's own prototype.
trait IndexedTrain<Marker> {
    /// Trains n-best result `index`.
    fn train_at(self, inst: P, index: u8) -> bool;
}

impl<F: Fn(P, u8) -> bool> IndexedTrain<(P, u8)> for F {
    fn train_at(self, inst: P, index: u8) -> bool {
        self(inst, index)
    }
}

impl<F: Fn(P) -> bool> IndexedTrain<(P,)> for F {
    fn train_at(self, _inst: P, _index: u8) -> bool {
        unreachable!("the zhuyin facade's train takes no index")
    }
}

/// `unigram_of` on `inst`.
fn unigram_of(inst: P, token: u32) -> J {
    let mut freq = UNTOUCHED;
    let ret = facade::token_get_unigram_frequency(inst, token, &raw mut freq);
    J::from((ret, freq))
}

/// Library 7's tokens of `phrase` on `inst`.
fn user_tokens(inst: P, phrase: &str) -> Vec<u32> {
    lookup(inst, phrase.as_bytes())
        .into_iter()
        .filter(|token| token >> 24 == 7)
        .collect()
}

/// Every export row of `library`: `[ret, phrase, reading, count]`.
fn export_rows(ctx: P, library: u32, only: Option<&str>) -> Vec<J> {
    let it = facade::begin_get_phrases(ctx, library);
    let mut rows = Vec::new();
    while facade::iterator_has_next_phrase(it) {
        let (mut text, mut reading, mut count) =
            (std::ptr::null_mut(), std::ptr::null_mut(), 0_i32);
        let ret =
            facade::iterator_get_next_phrase(it, &raw mut text, &raw mut reading, &raw mut count);
        let (text, reading) = (take_text(text), take_text(reading));
        if only.is_none_or(|phrase| text.as_deref() == Some(phrase)) {
            rows.push(J::from((ret, text, reading, count)));
        }
    }
    facade::end_get_phrases(it);
    rows
}

/// The phrase and its doubled reading.
fn doubled_reading() -> String {
    let (reading, separator) = if facade::ZHUYIN {
        ("ㄋㄧˇ ㄏㄠˇ", " ")
    } else {
        ("ni3'hao3", "'")
    };
    format!("{reading}{separator}{reading}")
}

/// The session's import of `phrase` into library 7.
fn import(ctx: P, phrase: &str, out: &mut J) {
    let it = facade::begin_add_phrases(ctx, 7);
    let added = facade::iterator_add_phrase(
        it,
        c(phrase).as_ptr(),
        c(doubled_reading()).as_ptr(),
        100_000,
    );
    facade::end_add_phrases(it);
    out.set("added", vec![added]);
}

/// The session's parse, guess, lookup and candidate guess.
fn parse_and_lookup(inst: P, phrase: &str, out: &mut J) {
    let parsed = if facade::ZHUYIN {
        facade::parse_more_chewings(inst, c("su3cl3su3cl3").as_ptr())
    } else {
        facade::parse_more_full_pinyins(inst, c("ni3hao3ni3hao3").as_ptr())
    };
    out.set("parse", parsed);
    out.set("guess", facade::guess_sentence(inst));
    out.set("tokens", lookup(inst, phrase.as_bytes()));
    if facade::ZHUYIN {
        facade::guess_candidates_after_cursor(inst, 0);
    } else {
        facade::guess_candidates(inst, 0, 0x1f);
    }
}

/// The first candidate's flag, text and (pinyin) user flag.
fn first_candidate(inst: P, out: &mut J) {
    let mut cand: P = std::ptr::null_mut();
    out.set(
        "candidate0-ok",
        facade::get_candidate(inst, 0, &raw mut cand),
    );
    if !cand.is_null() {
        let mut text: *const std::ffi::c_char = std::ptr::null();
        facade::get_candidate_string(inst, cand, &raw mut text);
        out.set("candidate0", borrowed_text(text));
        if !facade::ZHUYIN {
            out.set("candidate0-user", facade::is_user_candidate(inst, cand));
        }
    }
}

/// The session's unigram, remember and export mutations.
fn frequencies(ctx: P, inst: P, phrase: &str, out: &mut J) {
    let token = lookup(inst, "你".as_bytes())[0];
    out.set("frequency-before", unigram_of(inst, token));
    out.set(
        "frequency-add",
        facade::token_add_unigram_frequency(inst, token, 7),
    );
    out.set("frequency-after", unigram_of(inst, token));
    if !facade::ZHUYIN {
        out.set(
            "remember",
            facade::remember_user_input(inst, c(phrase).as_ptr(), 3),
        );
        out.set("export", export_rows(ctx, 7, None));
    }
}

/// A second NULL-user context's library 7 tokens, then the first's mask.
fn other_and_mask(k: &Kit, ctx: P, inst: P, phrase: &str, out: &mut J) {
    let other = k.init(Some(&k.data), None);
    let other_inst = facade::alloc_instance(other);
    out.set("other-user-tokens", user_tokens(other_inst, phrase));
    facade::free_instance(other_inst);
    facade::fini(other);
    out.set("mask", facade::mask_out(ctx, 0x0f00_0000, 0x0700_0000));
    out.set("tokens-after-mask", user_tokens(inst, phrase));
}

/// `null_user_session`: a literal NULL user directory, cwd and TMPDIR checked for changes.
fn null_user_session(k: &mut Kit) -> J {
    let canaries = canaries(k);
    let mut out = J::map();
    let ctx = k.init(Some(&k.data), None);
    out.set("init", !ctx.is_null());
    canaries.checkpoint(&mut out, "init");
    if ctx.is_null() {
        return out;
    }
    let inst = facade::alloc_instance(ctx);
    out.set("train-fresh", train(inst));
    canaries.checkpoint(&mut out, "train-fresh");
    out.set("save-fresh", facade::save(ctx));
    canaries.checkpoint(&mut out, "save-fresh");
    let phrase = "你好你好";
    import(ctx, phrase, &mut out);
    canaries.checkpoint(&mut out, "import");
    parse_and_lookup(inst, phrase, &mut out);
    first_candidate(inst, &mut out);
    canaries.checkpoint(&mut out, "lookup");
    out.set("train", train(inst));
    if !facade::ZHUYIN {
        out.set(
            "train-invalid-index",
            IndexedTrain::train_at(facade::train, inst, 255),
        );
    }
    canaries.checkpoint(&mut out, "train");
    out.set("save", facade::save(ctx));
    canaries.checkpoint(&mut out, "save");
    frequencies(ctx, inst, phrase, &mut out);
    other_and_mask(k, ctx, inst, phrase, &mut out);
    canaries.checkpoint(&mut out, "mutations");
    facade::free_instance(inst);
    facade::fini(ctx);
    canaries.checkpoint(&mut out, "fini");
    std::env::set_current_dir(&k.scratch).expect("return to scratch");
    out
}

/// `transient_library`: imports 你好 into `library` with a NULL or ordinary user path.
fn transient_library(k: &mut Kit, library: u8, null: bool) -> J {
    std::env::set_current_dir(&k.scratch).expect("enter scratch");
    let ctx = k.init(Some(&k.data), (!null).then_some(k.user.as_path()));
    let mut out = obj! { "init" => !ctx.is_null() };
    let it = facade::begin_add_phrases(ctx, library);
    out.set("begin", !it.is_null());
    let reading = if facade::ZHUYIN {
        "ㄋㄧˇ ㄏㄠˇ"
    } else {
        "ni3'hao3"
    };
    out.set(
        "add",
        facade::iterator_add_phrase(it, c("你好").as_ptr(), c(reading).as_ptr(), 100_000),
    );
    facade::end_add_phrases(it);
    let inst = facade::alloc_instance(ctx);
    let tokens: Vec<u32> = lookup(inst, "你好".as_bytes())
        .into_iter()
        .filter(|token| token >> 24 == u32::from(library))
        .collect();
    out.set("tokens", tokens);
    let export = if facade::ZHUYIN {
        J::from("NO EXPORT API")
    } else {
        J::from(export_rows(ctx, u32::from(library), Some("你好")))
    };
    out.set("export", export);
    facade::free_instance(inst);
    facade::fini(ctx);
    out
}

/// The case `id`'s result document.
pub fn case(k: &mut Kit, id: &str) -> J {
    if id.starts_with("null-user-session-") {
        return null_user_session(k);
    }
    let rest = id
        .strip_prefix("user-library-")
        .expect("a null_user_library case");
    let (library, rest) = rest.split_once('-').expect("a library number");
    let library: u8 = library.parse().expect("a library number");
    let null = match rest.split_once('-').expect("a placement").0 {
        "null" => true,
        "ordinary" => false,
        other => panic!("unknown placement {other}"),
    };
    transient_library(k, library, null)
}
