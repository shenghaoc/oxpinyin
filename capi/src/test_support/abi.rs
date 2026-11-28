//! Pinyin C-ABI helpers that the crate's module tests and its verification
//! tests share. `test_support.rs` declares this module, and
//! `tests/it/support.rs` includes it by `#[path]`. The exported functions
//! and types are named through `oxpinyin_capi`, which the crate root also
//! binds in a test build.

#![allow(dead_code)]
#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use super::*;

// The verification tests' `support.rs` already brings these in through
// `super`; the module tests' `test_support.rs` does not.
#[allow(
    unused_imports,
    reason = "used by the module tests, redundant in the verification tests"
)]
use oxpinyin_capi::{
    candidates::{
        pinyin_get_candidate, pinyin_get_candidate_nbest_index, pinyin_get_candidate_string,
        pinyin_get_candidate_type, pinyin_get_n_candidate, pinyin_is_user_candidate, pinyin_train,
    },
    context::{pinyin_fini, pinyin_init, pinyin_save},
    dict::{pinyin_lookup_tokens, pinyin_token_get_unigram_frequency},
    instance::{pinyin_alloc_instance, pinyin_free_instance},
    iterators::{
        pinyin_begin_add_phrases, pinyin_begin_get_phrases, pinyin_end_add_phrases,
        pinyin_end_get_phrases, pinyin_iterator_add_phrase, pinyin_iterator_get_next_phrase,
        pinyin_iterator_has_next_phrase,
    },
    parse::pinyin_parse_more_full_pinyins,
    predict::pinyin_guess_predicted_candidates,
    sentence::{pinyin_guess_candidates, pinyin_guess_sentence},
    types::{
        ChewingKey, ExportIterator, GArray, GChar, LookupCandidate, PinyinContext, PinyinInstance,
        lookup_candidate_type_t,
    },
};

use std::ffi::{CString, c_uint};

use std::path::Path;

use std::ptr;

/// `SORT_BY_PHRASE_LENGTH | SORT_BY_PINYIN_LENGTH | SORT_BY_FREQUENCY`.
pub(crate) const DEFAULT_SORT: c_uint = 0x1e;

/// Parses `text`, guesses candidates, and returns the pointer to candidate
/// `index` (borrowed into the instance's snapshot until the next guess).
///
/// # Safety
///
/// `instance` is NULL or a live instance: `pinyin_alloc_instance` returned it and
/// `pinyin_free_instance` has not freed it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
pub(crate) unsafe fn candidate(
    instance: *mut PinyinInstance,
    text: &str,
    index: c_uint,
) -> *mut LookupCandidate {
    let input = cstr(text);
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, input.as_ptr()) },
        text.len(),
        "full input parses"
    );
    assert!(unsafe { pinyin_guess_candidates(instance, 0, DEFAULT_SORT) });
    let mut cand: *mut LookupCandidate = std::ptr::null_mut();
    assert!(
        unsafe { pinyin_get_candidate(instance, index, &raw mut cand) },
        "candidate {index} exists"
    );
    assert!(!cand.is_null());
    cand
}

pub(crate) fn open(user_dir: &str) -> (*mut PinyinContext, *mut PinyinInstance) {
    let system = cstr(system_dir().to_str().expect("UTF-8 path"));
    let user = cstr(user_dir);
    let context = unsafe { pinyin_init(system.as_ptr(), user.as_ptr()) };
    assert!(
        !context.is_null(),
        "fixture init must open the mini fixture"
    );
    let instance = unsafe { pinyin_alloc_instance(context) };
    assert!(!instance.is_null());
    (context, instance)
}

/// The environment variable that names the death test a re-executed test
/// binary runs.
pub(crate) const DEATH_TEST: &str = "OXPINYIN_CAPI_DEATH_TEST";

/// The environment variable that names the user directory a
/// [`run_in_child`] child works in.
pub(crate) const CHILD_DIR: &str = "OXPINYIN_CAPI_CHILD_DIR";

/// The environment variable that names the file [`assert_aborts_sharing`]
/// maps in the child.
// Unix only: the shared page is an `mmap(MAP_SHARED)` of a file.
#[cfg(unix)]
pub(crate) const DEATH_TEST_PAGE: &str = "OXPINYIN_CAPI_DEATH_TEST_PAGE";

/// The bytes [`assert_aborts_sharing`] shares with the child.
#[cfg(unix)]
pub(crate) const SHARED_PAGE: usize = 4096;

/// [`assert_aborts`], with one page the child writes through and this
/// process reads after the abort: a file holding `init` (zero-padded to
/// [`SHARED_PAGE`] bytes) that the child maps `MAP_SHARED`, so what the
/// facade wrote through pointers into the page before it aborted reaches
/// the file. `body` gets the page's address. Answers the page as the child
/// left it.
// Unix only: the shared page is an `mmap(MAP_SHARED)` of a file.
#[cfg(unix)]
pub(crate) fn assert_aborts_sharing(
    name: &str,
    init: &[u8],
    body: impl FnOnce(*mut u8),
) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt as _;
    if std::env::var_os(DEATH_TEST).is_some_and(|value| value == name) {
        let path = std::env::var_os(DEATH_TEST_PAGE).expect("the shared page's file");
        let path = CString::new(path.as_bytes()).expect("a path without NUL");
        // SAFETY: `path` is NUL-terminated; the descriptor stays open for
        // the mapping's life (the process aborts in `body`).
        let fd = unsafe { libc::open(path.as_ptr(), libc::O_RDWR) };
        assert!(fd >= 0, "open the shared page");
        // SAFETY: a fresh shared mapping of the whole page-sized file.
        let page = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                SHARED_PAGE,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            )
        };
        assert!(page != libc::MAP_FAILED, "map the shared page");
        exit_on_panic();
        body(page.cast());
        // Reached only when `body` did not abort.
        std::process::exit(0);
    }
    let scratch = tempfile::tempdir().expect("death-test scratch");
    let page = scratch.path().join("shared-page");
    let mut bytes = init.to_vec();
    bytes.resize(SHARED_PAGE, 0);
    std::fs::write(&page, &bytes).expect("write the shared page");
    let mut command = std::process::Command::new(std::env::current_exe().expect("test binary"));
    command
        .args(["--exact", name, "--test-threads=1"])
        .env(DEATH_TEST, name)
        .env(DEATH_TEST_PAGE, &page)
        .env("TMPDIR", scratch.path());
    wait_for_abort(name, command);
    std::fs::read(&page).expect("read the shared page")
}

/// The tokens `pinyin_lookup_tokens` writes for `text`.
///
/// # Safety
///
/// `instance` is NULL or a live instance: `pinyin_alloc_instance` returned it and
/// `pinyin_free_instance` has not freed it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
pub(crate) unsafe fn lookup(instance: *mut PinyinInstance, text: impl AsRef<[u8]>) -> Vec<u32> {
    let text = CString::new(text.as_ref()).expect("no interior NUL");
    // SAFETY: a fresh GArray of four-byte elements, freed below.
    let array = unsafe { glib_sys::g_array_new(0, 0, 4) };
    unsafe { pinyin_lookup_tokens(instance, text.as_ptr(), array.cast()) };
    // SAFETY: a live GArray of four-byte elements: `data` holds `len` of them.
    let tokens = unsafe {
        let view = &*array;
        if view.len == 0 {
            Vec::new()
        } else {
            std::slice::from_raw_parts(view.data.cast::<u32>(), view.len as usize).to_vec()
        }
    };
    // SAFETY: allocated above and freed once.
    unsafe { glib_sys::g_array_free(array, 1) };
    tokens
}

/// The text `pinyin_token_get_phrase` writes for `token`.
///
/// # Safety
///
/// `instance` is NULL or a live instance: `pinyin_alloc_instance` returned it and
/// `pinyin_free_instance` has not freed it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
pub(crate) unsafe fn token_text(instance: *mut PinyinInstance, token: u32) -> String {
    let mut len = 0;
    let mut text = ptr::null_mut();
    assert!(unsafe {
        oxpinyin_capi::dict::pinyin_token_get_phrase(instance, token, &raw mut len, &raw mut text)
    });
    assert!(
        !text.is_null(),
        "pinyin_token_get_phrase answered true but wrote a NULL phrase for token {token:#x}"
    );
    // SAFETY: the getter answered true and `text` is not NULL (checked above), so `text` is
    // its NUL-terminated GLib string.
    let owned = unsafe { std::ffi::CStr::from_ptr(text) }
        .to_str()
        .expect("UTF-8 phrase")
        .to_owned();
    // SAFETY: the caller owns the string and frees it once.
    unsafe { glib_sys::g_free(text.cast()) };
    owned
}

/// The token `pinyin_lookup_tokens` writes for `text` in the user library.
///
/// # Safety
///
/// As [`lookup`].
pub(crate) unsafe fn user_token(instance: *mut PinyinInstance, text: &str) -> Option<u32> {
    // SAFETY: `instance` meets this function's `# Safety` contract; only this thread calls into it.
    unsafe { lookup(instance, text) }
        .into_iter()
        .find(|&token| token >> 24 == u32::from(oxpinyin::facade::USER_DICTIONARY))
}

/// The stored user-bigram count of `word` after `prev` in the context of `instance`, read
/// through the bigram export. A row's phrase is the text of the pair and its count is two
/// times the stored count (`pinyin.cpp:896-911`). No row is a count of 0. The export skips
/// the sentence-start predecessor.
///
/// # Safety
///
/// As [`lookup`].
pub(crate) unsafe fn exported_bigram_count(
    instance: *mut PinyinInstance,
    prev: u32,
    word: u32,
) -> i32 {
    use oxpinyin_capi::iterators::{
        pinyin_begin_get_bigram_phrases, pinyin_bigram_iterator_get_next_phrase,
        pinyin_bigram_iterator_has_next_phrase, pinyin_end_get_bigram_phrases,
    };
    // SAFETY: `instance` meets this function's `# Safety` contract; only this thread calls into it.
    let pair = unsafe { token_text(instance, prev) + &token_text(instance, word) };
    let context = unsafe { oxpinyin_capi::instance::pinyin_get_context(instance) };
    let iter = unsafe { pinyin_begin_get_bigram_phrases(context) };
    assert!(!iter.is_null());
    let mut counts = Vec::new();
    // The walk scans the last predecessor's gram only in a further `has_next` after one that
    // answered `false` (`pinyin.cpp:790-893`), so one `false` does not end the read.
    while unsafe {
        pinyin_bigram_iterator_has_next_phrase(iter) || pinyin_bigram_iterator_has_next_phrase(iter)
    } {
        let mut phrase = ptr::null_mut();
        let mut pinyin = ptr::null_mut();
        let mut count = 0;
        unsafe {
            pinyin_bigram_iterator_get_next_phrase(
                iter,
                &raw mut phrase,
                &raw mut pinyin,
                &raw mut count,
            )
        };
        // SAFETY: the iterator wrote a NUL-terminated GLib string, or NULL for a pair with
        // no text.
        if !phrase.is_null()
            && unsafe { std::ffi::CStr::from_ptr(phrase) }.to_bytes() == pair.as_bytes()
        {
            counts.push(count);
        }
        // SAFETY: the caller owns both strings and frees each once; `g_free` accepts NULL.
        unsafe {
            glib_sys::g_free(phrase.cast());
            glib_sys::g_free(pinyin.cast());
        }
    }
    unsafe { pinyin_end_get_bigram_phrases(iter) };
    // Each reading pair of the two items is one row with the same count.
    counts.dedup();
    match counts[..] {
        [] => 0,
        [count] => {
            assert_eq!(
                count % 2,
                0,
                "the export count is two times the stored count"
            );
            count / 2
        }
        _ => panic!("the rows of {pair} differ: {counts:?}"),
    }
}

/// One GLib log record: the domain (`None` for the default domain) and
/// the level.
pub(crate) type LogRecord = (Option<String>, glib_sys::GLogLevelFlags);

std::thread_local! {
    static CAPTURED: std::cell::RefCell<Option<Vec<(LogRecord, String)>>> =
        const { std::cell::RefCell::new(None) };
}

pub(crate) unsafe extern "C" fn capture_log(
    domain: *const std::ffi::c_char,
    level: glib_sys::GLogLevelFlags,
    message: *const std::ffi::c_char,
    data: glib_sys::gpointer,
) {
    let captured = CAPTURED.with(|logs| {
        let mut logs = logs.borrow_mut();
        let Some(logs) = logs.as_mut() else {
            return false;
        };
        let domain = (!domain.is_null()).then(|| {
            // SAFETY: GLib hands a live NUL-terminated domain.
            unsafe { std::ffi::CStr::from_ptr(domain) }
                .to_string_lossy()
                .into_owned()
        });
        // SAFETY: GLib supplies the live, NUL-terminated diagnostic message.
        let message = unsafe { std::ffi::CStr::from_ptr(message) }
            .to_string_lossy()
            .into_owned();
        logs.push(((domain, level), message));
        true
    });
    if !captured {
        // SAFETY: GLib's unchanged arguments, for a thread with no capture.
        unsafe { glib_sys::g_log_default_handler(domain, level, message, data) };
    }
}

/// Collects the GLib log records of this thread (the `libpinyin`, `GLib`
/// and default domains) while it lives.
pub(crate) struct LogCapture {
    pub(crate) handlers: Vec<(Option<&'static std::ffi::CStr>, u32)>,
}

impl LogCapture {
    pub fn new() -> Self {
        CAPTURED.with(|logs| *logs.borrow_mut() = Some(Vec::new()));
        let handlers = [Some(c"libpinyin"), Some(c"GLib"), None]
            .into_iter()
            .map(|domain| {
                // SAFETY: the callback lives for the process and takes no
                // user data; Drop removes it.
                let id = unsafe {
                    glib_sys::g_log_set_handler(
                        domain.map_or(std::ptr::null(), std::ffi::CStr::as_ptr),
                        glib_sys::G_LOG_LEVEL_MASK,
                        Some(capture_log),
                        std::ptr::null_mut(),
                    )
                };
                (domain, id)
            })
            .collect();
        Self { handlers }
    }

    /// The records since the capture began or since the last call.
    pub fn take(&self) -> Vec<LogRecord> {
        self.take_with_messages()
            .into_iter()
            .map(|(record, _)| record)
            .collect()
    }

    /// Takes the records and their messages in emission order.
    #[expect(
        clippy::unused_self,
        reason = "The capture guard must stay live while its thread-local records are read"
    )]
    pub fn take_with_messages(&self) -> Vec<(LogRecord, String)> {
        CAPTURED.with(|logs| std::mem::take(logs.borrow_mut().as_mut().expect("active capture")))
    }
}

impl Drop for LogCapture {
    fn drop(&mut self) {
        for (domain, id) in &self.handlers {
            // SAFETY: the handler ids installed in `new`.
            unsafe {
                glib_sys::g_log_remove_handler(
                    domain.map_or(std::ptr::null(), std::ffi::CStr::as_ptr),
                    *id,
                );
            }
        }
        CAPTURED.with(|logs| *logs.borrow_mut() = None);
    }
}

pub(crate) const NORMAL: lookup_candidate_type_t = lookup_candidate_type_t::NORMAL_CANDIDATE;

pub(crate) const PREDICTED_BIGRAM: lookup_candidate_type_t =
    lookup_candidate_type_t::PREDICTED_BIGRAM_CANDIDATE;

pub(crate) const PREDICTED_PREFIX: lookup_candidate_type_t =
    lookup_candidate_type_t::PREDICTED_PREFIX_CANDIDATE;

pub(crate) const PREDICTED_PUNCTUATION: lookup_candidate_type_t =
    lookup_candidate_type_t::PREDICTED_PUNCTUATION_CANDIDATE;

/// A context on `system` and `user`; NULL when init refuses.
pub(crate) fn init_at(system: &Path, user: &Path) -> *mut PinyinContext {
    let system = cstr(system.to_str().expect("UTF-8 path"));
    let user = cstr(user.to_str().expect("UTF-8 path"));
    unsafe { pinyin_init(system.as_ptr(), user.as_ptr()) }
}

/// A context and instance on the mini fixture and `user`.
pub(crate) fn open_at(user: &Path) -> (*mut PinyinContext, *mut PinyinInstance) {
    open(user.to_str().expect("UTF-8 path"))
}

/// # Safety
///
/// `context` is NULL or a live context: `pinyin_init` returned it and `pinyin_fini` has not taken
/// it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
pub(crate) unsafe fn alloc(context: *mut PinyinContext) -> *mut PinyinInstance {
    let instance = unsafe { pinyin_alloc_instance(context) };
    assert!(!instance.is_null());
    instance
}

/// # Safety
///
/// `context` is NULL or a live context: `pinyin_init` returned it and `pinyin_fini` has not taken
/// it.
///
/// `instance` is NULL or a live instance: `pinyin_alloc_instance` returned it and
/// `pinyin_free_instance` has not freed it.
///
/// `instance` is NULL or belongs to `context`. The call frees both, so the caller does not use
/// either again.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
pub(crate) unsafe fn close(context: *mut PinyinContext, instance: *mut PinyinInstance) {
    unsafe { pinyin_free_instance(instance) };
    unsafe { pinyin_fini(context) };
}

/// # Safety
///
/// `instance` is NULL or a live instance: `pinyin_alloc_instance` returned it and
/// `pinyin_free_instance` has not freed it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
pub(crate) unsafe fn parse(instance: *mut PinyinInstance, text: impl AsRef<[u8]>) -> usize {
    let text = std::ffi::CString::new(text.as_ref()).expect("no interior NUL");
    unsafe { pinyin_parse_more_full_pinyins(instance, text.as_ptr()) }
}

/// The first row of `kind` whose text is `text`.
///
/// # Safety
///
/// `instance` is NULL or a live instance: `pinyin_alloc_instance` returned it and
/// `pinyin_free_instance` has not freed it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
pub(crate) unsafe fn row_with(
    instance: *mut PinyinInstance,
    kind: lookup_candidate_type_t,
    text: &str,
) -> *mut LookupCandidate {
    // SAFETY: `instance` meets this function's `# Safety` contract; only this thread calls into it.
    let index = unsafe { capture_rows(instance) }
        .iter()
        .position(|(found, row, _)| *found == kind && row == text)
        .unwrap_or_else(|| panic!("a {kind:?} row {text} is listed"));
    // SAFETY: `instance` meets this function's `# Safety` contract; only this thread calls into it.
    unsafe { candidate_at(instance, index) }
}

/// # Safety
///
/// `instance` is NULL or a live instance: `pinyin_alloc_instance` returned it and
/// `pinyin_free_instance` has not freed it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
pub(crate) unsafe fn candidate_at(
    instance: *mut PinyinInstance,
    index: usize,
) -> *mut LookupCandidate {
    let mut cand: *mut LookupCandidate = ptr::null_mut();
    assert!(unsafe {
        pinyin_get_candidate(
            instance,
            u32::try_from(index).expect("small index"),
            &raw mut cand,
        )
    });
    cand
}

/// # Safety
///
/// `instance` is NULL or a live instance: `pinyin_alloc_instance` returned it and
/// `pinyin_free_instance` has not freed it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
pub(crate) unsafe fn n_candidate(instance: *mut PinyinInstance) -> u32 {
    let mut count = UNTOUCHED;
    assert!(unsafe { pinyin_get_n_candidate(instance, &raw mut count) });
    count
}

/// # Safety
///
/// `instance` is NULL or a live instance: `pinyin_alloc_instance` returned it and
/// `pinyin_free_instance` has not freed it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
pub(crate) unsafe fn unigram(instance: *mut PinyinInstance, token: u32) -> (bool, u32) {
    let mut freq = UNTOUCHED;
    let ret = unsafe { pinyin_token_get_unigram_frequency(instance, token, &raw mut freq) };
    (ret, freq)
}

/// The user bigram of a profile on the mini fixture replaced by `rows`
/// under a conforming `user.conf`; the rest of the profile stays.
pub(crate) fn replace_user_bigram(user: &Path, rows: &[(Vec<u8>, Vec<u8>)]) {
    craft_user_bigram(&user.join("user_bigram.db"), rows);
    write_user_conf(user, &system_dir());
}

/// Imports and saves 龘龘 (da2'da2) in library 7, then closes the context;
/// answers its token.
pub(crate) fn saved_user_phrase(user: &Path) -> u32 {
    let (context, instance) = open_at(user);
    assert_eq!(
        // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
        unsafe { import_batch(context, 7, &[("龘龘", "da2'da2", 5)]) },
        [true]
    );
    assert!(unsafe { pinyin_save(context) });
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let token = unsafe { lookup(instance, "龘龘") }[0];
    assert_eq!(token >> 24, 7);
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
    token
}

/// The first NORMAL user row of `dada` on a new context over `user`.
pub(crate) fn user_row_of_dada(user: &Path) -> (*mut PinyinInstance, *mut LookupCandidate) {
    let (_context, instance) = open_at(user);
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { parse(instance, "dada") }, 4);
    assert!(unsafe { pinyin_guess_candidates(instance, 0, 0) });
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let rows = unsafe { capture_rows(instance) };
    let index = (0..rows.len())
        .find(|&index| {
            // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into
            // it.
            let cand = unsafe { candidate_at(instance, index) };
            rows[index].0 == NORMAL && unsafe { pinyin_is_user_candidate(instance, cand) }
        })
        .expect("a NORMAL user row");
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    (instance, unsafe { candidate_at(instance, index) })
}

/// Every row of the current candidate list as (type, string, n-best index of an NBEST row).
///
/// # Safety
///
/// `instance` is NULL or a live instance: `pinyin_alloc_instance` returned it and
/// `pinyin_free_instance` has not freed it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
pub(crate) unsafe fn capture_rows(
    instance: *mut PinyinInstance,
) -> Vec<(lookup_candidate_type_t, String, Option<u8>)> {
    let mut count = 0;
    assert!(unsafe { pinyin_get_n_candidate(instance, &raw mut count) });
    (0..count)
        .map(|index| {
            let mut cand: *mut LookupCandidate = ptr::null_mut();
            assert!(unsafe { pinyin_get_candidate(instance, index, &raw mut cand) });
            let mut kind = lookup_candidate_type_t::ZOMBIE_CANDIDATE;
            assert!(unsafe { pinyin_get_candidate_type(instance, cand, &raw mut kind) });
            let mut text: *const GChar = ptr::null();
            assert!(unsafe { pinyin_get_candidate_string(instance, cand, &raw mut text) });
            assert!(!text.is_null(), "expected a candidate with display text");
            // SAFETY: the current row owns the non-NULL, NUL-terminated display string.
            let text = unsafe { std::ffi::CStr::from_ptr(text) }
                .to_string_lossy()
                .into_owned();
            let nbest = (kind == lookup_candidate_type_t::NBEST_MATCH_CANDIDATE).then(|| {
                let mut index = 0_u8;
                assert!(unsafe {
                    pinyin_get_candidate_nbest_index(instance, cand, &raw mut index)
                });
                index
            });
            (kind, text, nbest)
        })
        .collect()
}

/// Imports ROWS into LIBRARY in one batch; answers each add's flag.
///
/// # Safety
///
/// `context` is NULL or a live context: `pinyin_init` returned it and `pinyin_fini` has not taken
/// it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
pub(crate) unsafe fn import_batch(
    context: *mut PinyinContext,
    library: u8,
    rows: &[(&str, &str, i32)],
) -> Vec<bool> {
    let iter = unsafe { pinyin_begin_add_phrases(context, library) };
    assert!(!iter.is_null());
    let added = rows
        .iter()
        .map(|&(phrase, reading, count)| unsafe {
            pinyin_iterator_add_phrase(iter, cstr(phrase).as_ptr(), cstr(reading).as_ptr(), count)
        })
        .collect();
    unsafe { pinyin_end_add_phrases(iter) };
    added
}

/// Library INDEX's export rows, through `begin`/`has_next`/`get_next`.
///
/// # Safety
///
/// `context` is NULL or a live context: `pinyin_init` returned it and `pinyin_fini` has not taken
/// it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
pub(crate) unsafe fn export_of(
    context: *mut PinyinContext,
    index: u32,
) -> Vec<(String, String, i32)> {
    let iter = unsafe { pinyin_begin_get_phrases(context, index) };
    assert!(
        !iter.is_null(),
        "begin never answers NULL on a live context (pinyin.cpp:662-687)"
    );
    // SAFETY: `iter` is a live iterator from its `begin` call, not yet ended; only this thread
    // calls into it.
    let rows = unsafe { drain_phrases(iter) };
    unsafe { pinyin_end_get_phrases(iter) };
    rows
}

/// Drains an export iterator into owned `(phrase, pinyin, count)` triples,
/// freeing each caller-owned buffer as it goes.
///
/// # Safety
///
/// `iter` is NULL or a live export iterator: `pinyin_begin_get_phrases` returned it and
/// `pinyin_end_get_phrases` has not freed it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
pub(crate) unsafe fn drain_phrases(iter: *mut ExportIterator) -> Vec<(String, String, i32)> {
    let mut rows = Vec::new();
    while unsafe { pinyin_iterator_has_next_phrase(iter) } {
        let mut phrase: *mut GChar = ptr::null_mut();
        let mut pinyin: *mut GChar = ptr::null_mut();
        let mut count: std::os::raw::c_int = 0;
        assert!(unsafe {
            pinyin_iterator_get_next_phrase(iter, &raw mut phrase, &raw mut pinyin, &raw mut count)
        });
        rows.push(
            // SAFETY: both slots hold the iterator outputs, owned by this caller.
            unsafe { take_exported_row(phrase, pinyin, count) },
        );
    }
    rows
}

/// A private system whose addon library 4 holds 二簧 with two toned
/// readings, an item whose first word is the surrogate 0xD800, and 三次 indexed under `san ci` with no reading in its body.
pub(crate) fn promotion_system(parent: &std::path::Path) -> std::path::PathBuf {
    use {AddonItem, addon_system, system_dir};
    let readings: [(&[u16], u32); 2] = [
        (&[packed_key("er", 2), packed_key("huang", 2)], 7),
        (&[packed_key("er", 4), packed_key("huang", 1)], 3),
    ];
    let system = addon_system(
        &system_dir(),
        parent,
        &[
            AddonItem {
                text: &[0x4e8c, 0x7c27],
                readings: &readings,
                indexed_as: &[packed_key("er", 0), packed_key("huang", 0)],
                unigram: 11,
            },
            AddonItem {
                text: &[0xD800, 0x4e8c],
                readings: &[(&[packed_key("ba", 1), packed_key("ma", 3)], 5)],
                indexed_as: &[packed_key("ba", 0), packed_key("ma", 0)],
                unigram: 5,
            },
            AddonItem {
                text: &[0x4e09, 0x6b21],
                readings: &[],
                indexed_as: &[packed_key("san", 0), packed_key("ci", 0)],
                unigram: 9,
            },
        ],
    );
    std::fs::write(
        system.join("interpolation2.text"),
        "\\data model interpolation\n\\1-gram\n\\item 1 ok count 1\n",
    )
    .expect("interpolation2.text");
    system
}

/// Reads a caller-owned, NUL-terminated export string and frees it.
///
/// # Safety
///
/// `ptr` is a NUL-terminated UTF-8 string, not NULL, that the library returned to the caller, who
/// owns it. The call frees it with `g_free`, so the caller does not use it again.
pub(crate) unsafe fn take_exported(ptr: *mut std::ffi::c_char) -> String {
    // SAFETY: the iterator hands out NUL-terminated, valid UTF-8 text.
    let text = unsafe { std::ffi::CStr::from_ptr(ptr) }
        .to_str()
        .expect("UTF-8 export string")
        .to_owned();
    // SAFETY: `ptr` meets this function's `# Safety` contract.
    unsafe { free_exported(ptr) };
    text
}

/// Takes ownership of both string outputs from an export step.
///
/// # Safety
///
/// Each pointer is NULL or a distinct caller-owned export string.
pub(crate) unsafe fn take_exported_row(
    phrase: *mut GChar,
    pinyin: *mut GChar,
    count: i32,
) -> (String, String, i32) {
    if phrase.is_null() || pinyin.is_null() {
        // SAFETY: both slots are NULL or hold separate owned export strings.
        unsafe {
            free_exported(phrase);
            free_exported(pinyin);
        }
        panic!("expected both export strings");
    }
    // SAFETY: both outputs are non-NULL, owned, NUL-terminated UTF-8 strings.
    unsafe { (take_exported(phrase), take_exported(pinyin), count) }
}

/// The packed key of `spelling` at `tone`, as an addon item stores it.
pub(crate) fn packed_key(spelling: &str, tone: u8) -> u16 {
    oxpinyin::facade::ChewingKey::from_pinyin(spelling)
        .expect("frozen syllable")
        .with_tone(tone)
        .to_packed()
}

/// Frees a caller-owned buffer the export iterators hand out.
///
/// # Safety
///
/// `ptr` is NULL or a NUL-terminated string that the library returned to the caller, who owns it.
/// The call frees it with `g_free`, so the caller does not use it again.
pub(crate) unsafe fn free_exported(ptr: *mut std::ffi::c_char) {
    // SAFETY: the export ABI specifies g_free for these strings and permits NULL.
    unsafe { glib_sys::g_free(ptr.cast()) };
}

pub(crate) const PARITY: u32 = 0x18a;

/// A context over a private system directory, with its user dir.
#[cfg(unix)]
pub(crate) fn init_private(scratch: &std::path::Path) -> (std::path::PathBuf, *mut PinyinContext) {
    let system = private_system(scratch);
    let user = scratch.join("user");
    std::fs::create_dir_all(&user).expect("user dir");
    let system_c = cstr(system.to_str().expect("UTF-8 path"));
    let user_c = cstr(user.to_str().expect("UTF-8 path"));
    (system, unsafe {
        pinyin_init(system_c.as_ptr(), user_c.as_ptr())
    })
}

/// A context, its instance and the user directory they sit in, released in
/// that order when the fixture drops. A death-test child aborts without
/// dropping it; its scratch root removes the directory.
pub(crate) struct Fixture {
    context: *mut PinyinContext,
    instance: *mut PinyinInstance,
    _user_dir: TempUserDir,
}

impl Fixture {
    /// The fixture's context. Only the fixture frees it, when it drops.
    pub(crate) fn context(&self) -> *mut PinyinContext {
        self.context
    }

    /// The fixture's instance. Only the fixture frees it, when it drops.
    pub(crate) fn instance(&self) -> *mut PinyinInstance {
        self.instance
    }
}

impl Fixture {
    /// A fresh instance with nothing parsed.
    pub(crate) fn new(tag: &str) -> Self {
        let user_dir = TempUserDir::new(tag);
        let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
        Self {
            context,
            instance,
            _user_dir: user_dir,
        }
    }

    /// An instance over `text`, which parses to `parsed` bytes.
    pub(crate) fn parsed(tag: &str, text: &str, parsed: usize) -> Self {
        let fixture = Self::new(tag);
        assert_eq!(
            unsafe { pinyin_parse_more_full_pinyins(fixture.instance, cstr(text).as_ptr()) },
            parsed
        );
        fixture
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        unsafe { pinyin_free_instance(self.instance) };
        unsafe { pinyin_fini(self.context) };
    }
}

/// ibus preset 1 (the GSettings default): LONGER rows on (bit 0x2 clear).
pub(crate) const IBUS_DEFAULT_SORT: c_uint = 0x1c;

/// A producing and a receiving instance, on one context or on two.
pub(crate) struct Pair {
    contexts: Vec<*mut PinyinContext>,
    producer: *mut PinyinInstance,
    receiver: *mut PinyinInstance,
    _dirs: Vec<TempUserDir>,
}

impl Pair {
    /// The pair's contexts: one, or the producer's and then the receiver's.
    /// Only the pair frees them, when it drops.
    pub(crate) fn contexts(&self) -> &[*mut PinyinContext] {
        &self.contexts
    }

    /// The producing instance. Only the pair frees it, when it drops.
    pub(crate) fn producer(&self) -> *mut PinyinInstance {
        self.producer
    }

    /// The receiving instance. Only the pair frees it, when it drops.
    pub(crate) fn receiver(&self) -> *mut PinyinInstance {
        self.receiver
    }
}

impl Pair {
    pub(crate) fn open(same_context: bool) -> Self {
        let first = TempUserDir::new("foreign-producer");
        let (context, producer) = open(first.path.to_str().expect("UTF-8 path"));
        if same_context {
            let receiver = unsafe { pinyin_alloc_instance(context) };
            assert!(!receiver.is_null());
            return Self {
                contexts: vec![context],
                producer,
                receiver,
                _dirs: vec![first],
            };
        }
        let second = TempUserDir::new("foreign-receiver");
        let (other, receiver) = open(second.path.to_str().expect("UTF-8 path"));
        Self {
            contexts: vec![context, other],
            producer,
            receiver,
            _dirs: vec![first, second],
        }
    }

    /// The receiving instance's context.
    pub(crate) fn receiver_context(&self) -> *mut PinyinContext {
        *self.contexts.last().expect("a context")
    }
}

impl Drop for Pair {
    fn drop(&mut self) {
        unsafe { pinyin_free_instance(self.producer) };
        unsafe { pinyin_free_instance(self.receiver) };
        for context in &self.contexts {
            unsafe { pinyin_fini(*context) };
        }
    }
}

/// The texts of the rows `pinyin_guess_predicted_candidates` lists
/// after `prefix`.
///
/// # Safety
///
/// `instance` is NULL or a live instance: `pinyin_alloc_instance` returned it and
/// `pinyin_free_instance` has not freed it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
pub(crate) unsafe fn predicted_texts(
    instance: *mut PinyinInstance,
    prefix: &std::ffi::CStr,
) -> Vec<String> {
    assert!(unsafe { pinyin_guess_predicted_candidates(instance, prefix.as_ptr()) });
    // SAFETY: `instance` meets this function's `# Safety` contract; only this thread calls into it.
    (unsafe { capture_rows(instance) })
        .into_iter()
        .map(|(_, text, _)| text)
        .collect()
}

#[cfg(any(unix, feature = "tkrzw", feature = "bdb"))]
/// Trains the top sentence of `nihao` once on a new instance.
///
/// # Safety
///
/// `context` is NULL or a live context: `pinyin_init` returned it and `pinyin_fini` has not taken
/// it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
pub(crate) unsafe fn train_once(context: *mut PinyinContext) {
    // SAFETY: `context` meets this function's `# Safety` contract; only this thread calls into it.
    let instance = unsafe { alloc(context) };
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { parse(instance, "nihao") }, 5);
    assert!(unsafe { pinyin_guess_sentence(instance) });
    assert!(unsafe { pinyin_train(instance, 0) });
    unsafe { pinyin_free_instance(instance) };
}
