//! Stateful session fuzzer over the C ABI — the libchewing `fuzzer.rs`
//! shape (`docs/testing/upstream-test-coverage.md`, "Fuzz harnesses").
//! An input is a sequence of commands; return values drive subsequent
//! state, so the harness walks real session lifecycles.
//!
//! # Input format
//!
//! The first byte of an input selects its user directory: 255 is an empty
//! directory, any other byte a copy of the template directory (see
//! "Independent inputs"). The commands follow.
//!
//! Each command is one selector byte, reduced modulo 11, then its own
//! arguments. A command consumes its arguments, so no argument byte is
//! also read as a selector. A number is one byte. A string is one length
//! byte, reduced modulo 65, then that many bytes; the C string ends at
//! its first NUL. An argument past the end of the input is 0 or the empty
//! string.
//!
//! | Selector | Command | Arguments |
//! |---|---|---|
//! | 0 | `pinyin_reset` | none |
//! | 1 | `pinyin_parse_more_full_pinyins` | string |
//! | 2 | `pinyin_guess_sentence`, then `pinyin_get_sentence` | none |
//! | 3 | `pinyin_guess_candidates`, a walk of the rows, `pinyin_choose_candidate` | offset, sort option, row |
//! | 4 | `pinyin_get_parsed_input_length` | none |
//! | 5 | `pinyin_train` | none |
//! | 6 | `pinyin_set_double_pinyin_scheme` | scheme |
//! | 7 | import one phrase, then export the same library | library, count, phrase string, pinyin string |
//! | 8 | export one library | library |
//! | 9 | `pinyin_clear_constraint` | offset |
//! | 10 | `pinyin_parse_more_double_pinyins` | string |
//!
//! The import library is the byte modulo 16. Count byte 255 is the default
//! count (-1); any other byte is that count, 0 included.
//!
//! # Seeds
//!
//! `fuzz/seeds/capi-commands/` holds one command stream per operation that
//! a mutation of an empty corpus does not reach soon. `just fuzz
//! capi-commands`, which the nightly also runs, copies them into
//! `corpus/capi-commands`.
//! Write the files again when the format above changes. Bytes are in
//! hexadecimal, strings in quotes; the first byte is the user directory:
//!
//! - `choose`: `00`, `01 05 "nihao"`, `02`, `03 00 00 00` (the sentence
//!   row), `03 00 00 03` (a phrase row), `01 05 "nihao"`, `02`, `05`. The
//!   parse, guess, choose, reparse and train sequence of
//!   `capi/tests/it/choose.rs`.
//! - `double-pinyin`: `00`, `06 01` (ZRM), `0A 02 "aa"`, `02`,
//!   `03 00 00 00`, `06 02` (Microsoft), `0A 04 "nihk"`, `02`,
//!   `03 00 00 03`. The inputs of `capi/tests/it/contract.rs` and
//!   `choose.rs`.
//! - `import-export`: `00`, `07 05 07 06 "你好" 06 "ni'hao"` (library 5,
//!   count 7), `08 05`, `01 05 "nihao"`, `02`, `03 00 00 03`, `02`, `05`.
//!   The import of `capi/tests/it/import_export.rs`. The second `02`
//!   guesses the sentence under the chosen row, so the train is not the
//!   pin's stale-forcing assert.
//! - `first-run`: `FF`, `01 05 "nihao"`, `02`, `05`. A parse, a guess and
//!   a train on an empty user directory, the pin's first run.
//! - `stale-sentence`: `00`, `06 01` (ZRM), `0A 02 "aa"`, `02`, `0A 00`
//!   (no key), `02`, `01 03 "hao"`, `03 96 8C 68`. The second guess empties
//!   the constraint array and keeps the results of "aa", so the list holds
//!   sentence rows longer than the array. The row byte selects one whose
//!   choose is the pin's assert, the case of `capi/tests/it/sentence.rs`,
//!   and the harness skips it.
//!
//! # Findings and the pin's asserts
//!
//! Per the postconditions defined in the document above: no per-call
//! assertions on return values — valid-sequence contracts are pinned by
//! `capi/tests/it/contract.rs`; here a finding is any abort, panic, or
//! sanitizer report (`just fuzz` builds with AddressSanitizer and runs
//! it with LeakSanitizer). Mispaired iterator lifecycles are UB
//! by contract, so every begin reaches its end within the same command:
//! the bug classes hunted are caller-visible mispairings, not contract
//! violations.
//!
//! Where the pin asserts or aborts, the facade aborts the process too
//! (`docs/findings/upstream-divergences.md#pin-aborts-reproduced`), so the
//! harness keeps every call off those preconditions. Pin cites are to
//! 074a2219.
//!
//! - Scheme 30 (`DOUBLE_PINYIN_CUSTOMIZED`) is not set: the pin's
//!   `abort()` (`pinyin_parser2.cpp:611`).
//! - The guess offset is at most the parsed length. `_check_offset` reads
//!   column `offset - 1` (`pinyin.cpp:2163-2171`, from `:2226`), and a
//!   filled matrix has the parsed length plus one columns
//!   (`phonetic_key_matrix.cpp:47-48`).
//! - The candidate list is guessed in the command that chooses from it.
//!   `pinyin_get_n_candidate` answers only the current list's length
//!   (`pinyin.cpp:2845-2849`).
//! - The choose offset is 0: the pin asserts that under
//!   `SORT_WITHOUT_SENTENCE_CANDIDATE` (`:2566`) and otherwise only adds
//!   the offset to its answer (`:2588`).
//! - Conditions that depend on library state ask the library:
//!   `oxpinyin_choose_aborts`, `oxpinyin_train_aborts` and
//!   `oxpinyin_import_aborts`. The choose asserts on a predicted row
//!   (`:2507`), on a sentence row whose rank the results no longer hold
//!   (`:2517-2518`), and on a sentence row with a differing phrase that
//!   ends past the constraint array (`phonetic_lookup.cpp:64-65`, `:203`).
//!   The array follows the matrix only at a sentence guess and at a normal
//!   choose (`pinyin.cpp:1366`, `:2579`), and a sentence guess on an empty
//!   matrix keeps the old results (`phonetic_lookup.h:743-748`), so a list
//!   can hold a sentence row longer than the array.
//!
//! The other converted sites are not reachable from this alphabet: no
//! options are set (a toned initial needs `PINYIN_INCOMPLETE`), the
//! sentence is read at index 0 only, and the export walk is gated by
//! `has_next`.
//!
//! # Independent inputs
//!
//! Each input opens its own context over the committed mini tables and a new
//! user directory, and its own instance; it closes them when it ends. So
//! imported phrases, trained counts, the double pinyin scheme and the
//! instance state of one input do not reach the next, and an input
//! reproduces alone.
//!
//! The harness writes no file of its own into a user directory. At process
//! start, one `pinyin_init` and `pinyin_fini` run over a template user
//! directory, so the library writes what a first session leaves there
//! (`check_format` and `mark_version`, `pinyin.cpp:170-234`: `user.conf`
//! with the system versions, the database format and the open counter).
//! An input writes the template's files into its own directory, as a later
//! session of that profile finds it. Directory byte 255 starts from an empty
//! directory: the pin's first run, which writes one `open %s failed.` line
//! (`table_info.cpp:332`).
//!
//! `main` builds the template before the input: it runs the `pinyin_init`
//! and `pinyin_fini`, reads the files into memory and removes the template
//! directory. The input's own user directory is removed when its session
//! closes.
//!
//! The target runs one input per child (`LOOP_COUNT`): in a persistent
//! child, the same input covered different edges depending on the input
//! before it, although each input opens a new context, instance and user
//! directory. LeakSanitizer checks at exit.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]
// Scoped exception to this workspace's unsafe_code deny: an FFI-driven
// harness is the documented reason the fuzz crate uses deny, not forbid.
#![allow(unsafe_code)]

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::OnceLock;

use oxpinyin_capi::candidates::{oxpinyin_choose_aborts, oxpinyin_train_aborts};
use oxpinyin_capi::candidates::{
    pinyin_choose_candidate, pinyin_clear_constraint, pinyin_get_candidate,
    pinyin_get_candidate_string, pinyin_get_n_candidate, pinyin_train,
};
use oxpinyin_capi::config::pinyin_set_double_pinyin_scheme;
use oxpinyin_capi::context::{pinyin_fini, pinyin_init};
use oxpinyin_capi::instance::{pinyin_alloc_instance, pinyin_free_instance, pinyin_reset};
use oxpinyin_capi::iterators::oxpinyin_import_aborts;
use oxpinyin_capi::iterators::{
    pinyin_begin_add_phrases, pinyin_begin_get_phrases, pinyin_end_add_phrases,
    pinyin_end_get_phrases, pinyin_iterator_add_phrase, pinyin_iterator_get_next_phrase,
    pinyin_iterator_has_next_phrase,
};
use oxpinyin_capi::parse::{
    pinyin_get_parsed_input_length, pinyin_parse_more_double_pinyins,
    pinyin_parse_more_full_pinyins,
};
use oxpinyin_capi::sentence::{
    pinyin_get_sentence, pinyin_guess_candidates, pinyin_guess_sentence,
};
use oxpinyin_capi::types::{
    ExportIterator, GChar, GUint, ImportIterator, LookupCandidate, PinyinContext, PinyinInstance,
};
use tempfile::TempDir;

// SAFETY: re-declaring libc `free` to release strings the ABI hands to the
// caller (g_free and free are equivalent on glibc, as documented in capi's
// ffi.rs); signature matches the C standard.
unsafe extern "C" {
    fn free(ptr: *mut c_void);
}

/// The longest string argument, in bytes: it keeps the fuzzer on parse
/// states, not on multi-KB copies.
const MAX_STRING: usize = 64;

/// The bits `sort_option_t` defines (`pinyin.h:54-66`).
const SORT_BITS: u8 = 0x1F;

/// `DOUBLE_PINYIN_CUSTOMIZED` (`pinyin_custom2.h:108-117`).
const DOUBLE_PINYIN_CUSTOMIZED: c_int = 30;

/// The library slots of the pin's phrase index (`phrase_index.h:630`).
const LIBRARY_SLOTS: u8 = 16;

/// The directory byte of an input that starts from an empty user
/// directory.
const FIRST_RUN: u8 = u8::MAX;

/// What every session of this process opens over.
struct Fixture {
    /// The system directory, as `pinyin_init` takes it.
    system_dir: CString,
    /// The files and directories that one `pinyin_init` and `pinyin_fini`
    /// leave in a new user directory, by relative path, parents first: the
    /// state of a profile that holds no user data. `None` is a directory.
    template: Vec<(PathBuf, Option<Vec<u8>>)>,
}

/// A new directory under the system temporary directory.
fn new_dir(prefix: &str) -> TempDir {
    tempfile::Builder::new()
        .prefix(prefix)
        .tempdir()
        .expect("a new temporary directory")
}

/// `path` as the C string `pinyin_init` takes.
fn c_path(path: &Path) -> CString {
    CString::new(path.to_str().expect("UTF-8 path")).expect("no NUL")
}

/// The files and directories under `root`, below `relative`, parents first.
fn snapshot(root: &Path, relative: &Path, into: &mut Vec<(PathBuf, Option<Vec<u8>>)>) {
    let mut entries: Vec<_> = std::fs::read_dir(root.join(relative))
        .expect("read the template directory")
        .map(|entry| entry.expect("a template entry"))
        .collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = relative.join(entry.file_name());
        if entry.file_type().expect("a template entry type").is_dir() {
            into.push((path.clone(), None));
            snapshot(root, &path, into);
        } else {
            let bytes = std::fs::read(entry.path()).expect("read a template file");
            into.push((path, Some(bytes)));
        }
    }
}

/// Writes the template into the empty directory `to`.
fn write_template(template: &[(PathBuf, Option<Vec<u8>>)], to: &Path) {
    for (path, bytes) in template {
        let target = to.join(path);
        match bytes {
            Some(bytes) => std::fs::write(&target, bytes).expect("write a template file"),
            None => std::fs::create_dir(&target).expect("create a template subdirectory"),
        }
    }
}

/// The compiled-in backend's own fixture directory — the same contract as
/// the capi suites' `system_dir()` twins (capi/src/test_support.rs,
/// tests/it/support.rs): fixtures/w3 carries one complete DBM set per
/// backend under its `DEFAULT_STORE_EXT` subdirectory, and the runtime
/// opens only that backend's files. Passing the fixtures/w3 root asks for
/// DBMs that live one level down, so init answers NULL.
///
/// `main` calls this before the first input.
fn fixture() -> &'static Fixture {
    static FIXTURE: OnceLock<Fixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("fixtures")
            .join("w3")
            .join(oxpinyin::__internal::DEFAULT_STORE_EXT);
        let system_dir = c_path(&dir);
        let template_dir = new_dir("oxpinyin-fuzz-capi-template-");
        let user = c_path(template_dir.path());
        // SAFETY: both paths are live C strings; the context is closed
        // before either directory goes away.
        let context = unsafe { pinyin_init(system_dir.as_ptr(), user.as_ptr()) };
        assert!(
            !context.is_null(),
            "fixture context must initialize (system dir: {})",
            dir.display()
        );
        // SAFETY: the live context above, with no instance or iterator.
        unsafe { pinyin_fini(context) };
        let mut template = Vec::new();
        snapshot(template_dir.path(), Path::new(""), &mut template);
        drop(template_dir);
        Fixture {
            system_dir,
            template,
        }
    })
}

/// One input's context and instance, over a user directory no other input
/// has written. The drop closes both, then removes the directory.
struct Session {
    context: *mut PinyinContext,
    instance: *mut PinyinInstance,
    _user_dir: TempDir,
}

impl Session {
    /// Opens a session over a new user directory: empty for a first run,
    /// else holding the template's files.
    fn open(first_run: bool) -> Self {
        let fixture = fixture();
        let user_dir = new_dir("oxpinyin-fuzz-capi-");
        if !first_run {
            write_template(&fixture.template, user_dir.path());
        }
        let user = c_path(user_dir.path());
        // SAFETY: both paths are live C strings; the committed mini tables
        // and the new user directory outlive the context.
        let context = unsafe { pinyin_init(fixture.system_dir.as_ptr(), user.as_ptr()) };
        assert!(
            !context.is_null(),
            "fixture context must initialize (system dir: {})",
            fixture.system_dir.to_string_lossy()
        );
        // SAFETY: `context` is the live context above.
        let instance = unsafe { pinyin_alloc_instance(context) };
        assert!(!instance.is_null(), "instance must allocate");
        Session {
            context,
            instance,
            _user_dir: user_dir,
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // SAFETY: both handles are live and this is their one release; no
        // iterator outlives its command, and the inputs run one at a time on
        // one thread.
        unsafe {
            pinyin_free_instance(self.instance);
            pinyin_fini(self.context);
        }
    }
}

/// The bytes of one input the commands have not consumed.
struct Input<'a> {
    rest: &'a [u8],
}

impl Input<'_> {
    /// The next byte, or `None` at the end of the input.
    fn selector(&mut self) -> Option<u8> {
        let (&byte, rest) = self.rest.split_first()?;
        self.rest = rest;
        Some(byte)
    }

    /// A number argument: the next byte, or 0 past the end.
    fn number(&mut self) -> u8 {
        self.selector().unwrap_or(0)
    }

    /// A string argument: a length byte, then at most [`MAX_STRING`]
    /// bytes. The C string ends at its first NUL; the command still
    /// consumes the whole argument.
    fn string(&mut self) -> CString {
        let len = (usize::from(self.number()) % (MAX_STRING + 1)).min(self.rest.len());
        let (bytes, rest) = self.rest.split_at(len);
        self.rest = rest;
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(len);
        CString::new(&bytes[..end]).expect("cut at the first NUL")
    }
}

/// Walks the export iterator of one library — begin, rows, end.
fn export_library(context: *mut PinyinContext, index: GUint) {
    // SAFETY: live context.
    let iter: *mut ExportIterator = unsafe { pinyin_begin_get_phrases(context, index) };
    if iter.is_null() {
        return;
    }
    // SAFETY: `iter` was just returned by its begin call.
    while unsafe { pinyin_iterator_has_next_phrase(iter) } {
        let mut phrase: *mut c_char = null_mut();
        let mut pinyin: *mut c_char = null_mut();
        let mut freq: c_int = 0;
        // SAFETY: same live iterator; all out-params are locals.
        if unsafe { pinyin_iterator_get_next_phrase(iter, &mut phrase, &mut pinyin, &mut freq) } {
            // SAFETY: malloc'd by the ABI, released once here.
            unsafe {
                if !phrase.is_null() {
                    free(phrase.cast());
                }
                if !pinyin.is_null() {
                    free(pinyin.cast());
                }
            }
        }
    }
    // SAFETY: transfers ownership back exactly once.
    unsafe { pinyin_end_get_phrases(iter) };
}

/// Reads the rows of the current candidate list (borrowed strings — never
/// freed), then chooses the row `pick` selects.
fn walk_and_choose(instance: *mut PinyinInstance, pick: u8) {
    let mut count = 0_u32;
    // SAFETY: live instance; `count` is an out-param.
    if !unsafe { pinyin_get_n_candidate(instance, &mut count) } || count == 0 {
        return;
    }
    for index in 0..count.min(8) {
        let mut candidate: *mut LookupCandidate = null_mut();
        // SAFETY: live instance; in-range index.
        if unsafe { pinyin_get_candidate(instance, index, &mut candidate) } && !candidate.is_null()
        {
            let mut text: *const GChar = null();
            // SAFETY: `candidate` was just returned by the ABI.
            unsafe { pinyin_get_candidate_string(instance, candidate, &mut text) };
            if !text.is_null() {
                // SAFETY: borrowed interior pointer from the instance
                // snapshot; read-only, never freed.
                let _ = unsafe { CStr::from_ptr(text) }.to_bytes().len();
            }
        }
    }
    let mut candidate: *mut LookupCandidate = null_mut();
    // SAFETY: live instance; in-range index.
    if !unsafe { pinyin_get_candidate(instance, u32::from(pick) % count, &mut candidate) }
        || candidate.is_null()
    {
        return;
    }
    // Offset 0: the pin asserts it under SORT_WITHOUT_SENTENCE_CANDIDATE
    // (`pinyin.cpp:2566`). The library reports the pin's other choose
    // asserts for this row.
    // SAFETY: live instance; `candidate` is a row of the list the caller
    // guessed in this command.
    if !unsafe { oxpinyin_choose_aborts(instance, 0, candidate) } {
        // SAFETY: as above.
        unsafe { pinyin_choose_candidate(instance, 0, candidate) };
    }
}

/// One input per child: in a persistent child, the same input covered
/// different edges depending on the input before it, although each input
/// opens a new session and user directory. LeakSanitizer's check at exit then
/// names that input. `AFL_FUZZER_LOOPCOUNT` overrides it.
const LOOP_COUNT: usize = 1;

fn main() {
    fixture();
    oxpinyin_fuzz::fuzz(LOOP_COUNT, &[], run);
}

/// Runs one input.
fn run(data: &[u8]) {
    let mut input = Input { rest: data };
    let session = Session::open(input.number() == FIRST_RUN);
    let (context, instance) = (session.context, session.instance);
    while let Some(command) = input.selector() {
        match command % 11 {
            // reset the session state
            0 => {
                // SAFETY: live instance of this input's session.
                unsafe { pinyin_reset(instance) };
            }
            // feed a full pinyin string
            1 => {
                let pinyins = input.string();
                // SAFETY: live instance; `pinyins` is a valid CString.
                unsafe { pinyin_parse_more_full_pinyins(instance, pinyins.as_ptr()) };
            }
            // guess + fetch the sentence (owned string → free)
            2 => {
                // SAFETY: live instance.
                if unsafe { pinyin_guess_sentence(instance) } {
                    let mut sentence: *mut c_char = null_mut();
                    // SAFETY: live instance; `sentence` is an out-param.
                    if unsafe { pinyin_get_sentence(instance, 0, &mut sentence) }
                        && !sentence.is_null()
                    {
                        // SAFETY: malloc'd by the ABI, released once here.
                        unsafe { free(sentence.cast()) };
                    }
                }
            }
            // guess the candidates at an offset, walk the rows, choose one
            3 => {
                // SAFETY: live instance.
                let parsed = unsafe { pinyin_get_parsed_input_length(instance) };
                let offset = usize::from(input.number()) % (parsed + 1);
                let sort = GUint::from(input.number() & SORT_BITS);
                let pick = input.number();
                // SAFETY: live instance; the offset is inside the matrix.
                unsafe { pinyin_guess_candidates(instance, offset, sort) };
                walk_and_choose(instance, pick);
            }
            // parsed-length accounting
            4 => {
                // SAFETY: live instance.
                let _ = unsafe { pinyin_get_parsed_input_length(instance) };
            }
            // train the current selection, unless the library reports the
            // pin's training assert for it (stale forcing or span)
            5 => {
                // SAFETY: live instance.
                if !unsafe { oxpinyin_train_aborts(instance, 0) } {
                    // SAFETY: live instance.
                    unsafe { pinyin_train(instance, 0) };
                }
            }
            // config setter incl. out-of-enum discriminants (the pin's
            // half-mutation); CUSTOMIZED (30) is the pin's `abort()`
            6 => {
                let scheme = c_int::from(input.number());
                if scheme != DOUBLE_PINYIN_CUSTOMIZED {
                    // SAFETY: live context of this input's session.
                    unsafe { pinyin_set_double_pinyin_scheme(context, scheme) };
                }
            }
            // import iterator trio — begin/add/end paired within the
            // command — then the export of the same library, which reads
            // an added row back. Library 0 is RESERVED and 8 to 15 hold no
            // sub-index: the pin's `get_range` refuses them
            // (`pinyin.cpp:589-591`). 1 to 7 take the phrase.
            7 => {
                let library = input.number() % LIBRARY_SLOTS;
                let count = match input.number() {
                    u8::MAX => -1,
                    count => c_int::from(count),
                };
                let phrase = input.string();
                let pinyin = input.string();
                // SAFETY: live context.
                let iter: *mut ImportIterator =
                    unsafe { pinyin_begin_add_phrases(context, library) };
                if !iter.is_null() {
                    // The library reports the pin's import asserts.
                    if !unsafe { oxpinyin_import_aborts(iter, phrase.as_ptr(), pinyin.as_ptr()) } {
                        // SAFETY: `iter` was just returned by its begin call.
                        unsafe {
                            pinyin_iterator_add_phrase(
                                iter,
                                phrase.as_ptr(),
                                pinyin.as_ptr(),
                                count,
                            )
                        };
                    }
                    // SAFETY: transfers ownership back exactly once.
                    unsafe { pinyin_end_add_phrases(iter) };
                }
                export_library(context, GUint::from(library));
            }
            // export iterator trio — begin/walk/end paired within the command
            8 => {
                export_library(context, GUint::from(input.number()));
            }
            // constraint clearing at an arbitrary offset
            9 => {
                let offset = usize::from(input.number());
                // SAFETY: live instance.
                unsafe { pinyin_clear_constraint(instance, offset) };
            }
            // feed a double pinyin string, under the scheme command 6 set
            _ => {
                let pinyins = input.string();
                // SAFETY: live instance; `pinyins` is a valid CString.
                unsafe { pinyin_parse_more_double_pinyins(instance, pinyins.as_ptr()) };
            }
        }
    }
}
