//! Helpers more than one verification topic uses: the fixture handles, the
//! scratch directories, and the abort and child-process helpers. The
//! exported C functions and types come from the rlib's crate root.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use std::ffi::CStr;

pub use oxpinyin_capi::*;

#[path = "../../src/test_support/abi.rs"]
mod abi;
pub(crate) use abi::*;

#[path = "../../../tests/support/c_harness.rs"]
mod c_harness;
pub(crate) use c_harness::*;

#[path = "../../../tests/support/native_fixture.rs"]
mod native_fixture;
pub(crate) use native_fixture::*;

use std::ffi::c_char;

use std::path::Path;

use std::ptr;

/// Runs `body` in a child process that exits 0 when it returns, the way
/// [`assert_aborts`] runs one that must abort, and answers the child's
/// stderr. `body` gets a fresh, empty user directory the parent created, so
/// the parent can name it in the expected text. `name` is the calling
/// test's full path as libtest prints it (`module::test`). A child that
/// panics or dies fails the parent's assertion on its exit status.
pub(crate) fn run_in_child(name: &str, body: impl FnOnce(&std::path::Path)) -> ChildRun {
    if std::env::var_os(DEATH_TEST).is_some_and(|value| value == name) {
        let dir = std::env::var_os(CHILD_DIR).expect("the child's user directory");
        body(std::path::Path::new(&dir));
        std::process::exit(0);
    }
    let scratch = tempfile::tempdir().expect("child scratch");
    let user_dir = scratch.path().join("user");
    std::fs::create_dir(&user_dir).expect("child user directory");
    let output = crate::test_child::run_exact(
        name,
        [
            (DEATH_TEST, std::ffi::OsStr::new(name)),
            (CHILD_DIR, user_dir.as_os_str()),
            ("TMPDIR", scratch.path().as_os_str()),
        ],
    );
    // libtest reports a child's panic on stdout.
    assert!(
        output.status.success(),
        "{name} child failed ({:?}): {}{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    ChildRun {
        stderr: output.stderr,
        user_dir,
        _scratch: scratch,
    }
}

/// A private system whose table.conf has `from` replaced by `to`.
pub(crate) fn conf_system(parent: &Path, from: &str, to: &str) -> std::path::PathBuf {
    let system = private_system(parent);
    let path = system.join("table.conf");
    let text = std::fs::read_to_string(system_dir().join("table.conf")).expect("table.conf");
    assert!(text.contains(from), "table.conf holds {from}");
    std::fs::remove_file(&path).expect("unlink the linked table.conf");
    std::fs::write(&path, text.replace(from, to)).expect("write table.conf");
    system
}

pub(crate) const NBEST: lookup_candidate_type_t = lookup_candidate_type_t::NBEST_MATCH_CANDIDATE;

/// The first row of `kind` in the current list.
///
/// # Safety
///
/// `instance` is NULL or a live instance: `pinyin_alloc_instance` returned it and
/// `pinyin_free_instance` has not freed it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
pub(crate) unsafe fn row_of(
    instance: *mut PinyinInstance,
    kind: lookup_candidate_type_t,
) -> *mut LookupCandidate {
    // SAFETY: `instance` meets this function's `# Safety` contract; only this thread calls into it.
    let index = unsafe { capture_rows(instance) }
        .iter()
        .position(|(found, _, _)| *found == kind)
        .unwrap_or_else(|| panic!("a {kind:?} row is listed"));
    // SAFETY: `instance` meets this function's `# Safety` contract; only this thread calls into it.
    unsafe { candidate_at(instance, index) }
}

/// `get_sentence(index)` with a seeded out-pointer: the answer, and the
/// text or `None` when the pointer was left alone.
///
/// # Safety
///
/// `instance` is NULL or a live instance: `pinyin_alloc_instance` returned it and
/// `pinyin_free_instance` has not freed it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
pub(crate) unsafe fn sentence_out(
    instance: *mut PinyinInstance,
    index: u8,
) -> (bool, Option<String>) {
    let seed = UNTOUCHED as usize as *mut c_char;
    let mut out = seed;
    let ret = unsafe { pinyin_get_sentence(instance, index, &raw mut out) };
    if out == seed {
        (ret, None)
    } else {
        // SAFETY: `out` is the string the library just returned; this code owns it and frees it
        // only here.
        (ret, Some(unsafe { crate::support::take_owned_cstr(out) }))
    }
}

/// The newline the C stdio of the host writes for `\n` on stderr: the
/// UCRT's text mode writes `\r\n`.
pub(crate) const NL: &str = if cfg!(windows) { "\r\n" } else { "\n" };

pub(crate) fn write_log(user: &Path, name: &str, payload: &[u8]) {
    std::fs::write(
        user.join(name),
        oxpinyin::data::chunk_format::build_memory_chunk(payload).expect("frame"),
    )
    .expect("write the log");
}

/// The keys of reading `nth` of `token` as little-endian bytes.
///
/// # Safety
///
/// `instance` is NULL or a live instance: `pinyin_alloc_instance` returned it and
/// `pinyin_free_instance` has not freed it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
pub(crate) unsafe fn reading_bytes(instance: *mut PinyinInstance, token: u32, nth: u32) -> Vec<u8> {
    // SAFETY: a fresh GArray of two-byte elements, freed below.
    let keys = unsafe { glib_sys::g_array_new(0, 0, 2) };
    assert!(unsafe {
        crate::support::pinyin_token_get_nth_pronunciation(instance, token, nth, keys.cast())
    });
    // SAFETY: a live GArray of two-byte elements.
    let bytes = unsafe {
        let view = &*keys;
        if view.len == 0 {
            Vec::new()
        } else {
            std::slice::from_raw_parts(view.data.cast::<u8>(), view.len as usize * 2).to_vec()
        }
    };
    // SAFETY: allocated above, freed once.
    unsafe { glib_sys::g_array_free(keys, 1) };
    bytes
}

/// Library 1's export of `phrase`: each row's reading and count.
///
/// # Safety
///
/// `context` is NULL or a live context: `pinyin_init` returned it and `pinyin_fini` has not taken
/// it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
pub(crate) unsafe fn export_rows_of(
    context: *mut PinyinContext,
    library: u32,
    phrase: &str,
) -> Vec<(String, i32)> {
    // SAFETY: `context` meets this function's `# Safety` contract; only this thread calls into it.
    (unsafe { crate::support::export_of(context, library) })
        .into_iter()
        .filter(|(text, _, _)| text == phrase)
        .map(|(_, reading, count)| (reading, count))
        .collect()
}

/// A private system whose library 1 holds 甲?乙 (the middle word `word`)
/// at 100, 丙 at 1000, 戌 at 10 and 甲?乙丙; its bigram ranks 戌 after
/// 甲?乙.
pub(crate) fn prefix_system(parent: &Path, word: u32) -> std::path::PathBuf {
    let prefix = [0x7532, word, 0x4e59];
    let phrase = [0x7532, word, 0x4e59, 0x4e19];
    let system = crate::support::items_system(
        &system_dir(),
        parent,
        &[
            (&prefix, &[0x1084; 3], 100),
            (&[0x4e19], &[0x1081], 1000),
            (&[0x620c], &[0x1081], 10),
            (&phrase, &[0x1084, 0x1084, 0x1084, 0x1081], 100),
        ],
        2000,
    );
    craft_hash(
        &system.join("bigram.db"),
        &[(
            0x0100_0001_u32.to_le_bytes().to_vec(),
            gram_bytes(1000, &[(0x0100_0003, 1000)]),
        )],
    );
    system
}

/// GLib's `G_LOG_LEVEL_CRITICAL`.
pub(crate) const CRITICAL: glib_sys::GLogLevelFlags = glib_sys::G_LOG_LEVEL_CRITICAL;

// ── Pin capture of 2026-11-07 (tools/oracle/pin_capture.py, runs 1 and 2 identical) ──
//
// Each test below names its observation. The values are the pin's on the pinned
// libpinyin-data; every one asserted here depends only on the test's own imports, parse
// and options, so the w3 mini fixture holds it too.

/// Copies a string the C API handed out and frees it with `g_free`.
///
/// # Safety
///
/// `ptr` is NULL or a NUL-terminated string that the library returned to the caller, who owns it.
/// The call frees it with `g_free`, so the caller does not use it again.
pub(crate) unsafe fn take_owned_cstr(ptr: *mut c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    // SAFETY: The callers pass NUL-terminated buffers this library
    // returned.
    let text = unsafe { std::ffi::CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned();
    // SAFETY: `ptr` is a returned string the caller owns; `g_free` is its
    // deallocator.
    unsafe { glib_sys::g_free(ptr.cast()) };
    text
}

/// A gram with one item to `successor` at count 100: above the export
/// threshold, so the walk hands out its row at count 200.
pub(crate) fn whole_gram(successor: u32) -> Vec<u8> {
    [
        100_u32.to_le_bytes(),
        successor.to_le_bytes(),
        100_u32.to_le_bytes(),
    ]
    .concat()
}

/// Opens `user_dir` over the fixture, answering the raw handle.
pub(crate) fn init_fixture_at(user_dir: &std::path::Path) -> *mut crate::support::PinyinContext {
    let system = cstr(system_dir().to_str().expect("UTF-8 path"));
    let user = cstr(user_dir.to_str().expect("UTF-8 path"));
    unsafe { pinyin_init(system.as_ptr(), user.as_ptr()) }
}

/// The predecessor tokens of the bigram export fixture, in the order the
/// walk loads them (the backend's key order), and the successor both name.
pub(crate) fn export_order_fixture() -> (u32, u32, u32) {
    let scratch = TempUserDir::new("export-order-probe");
    let (context, instance) = open(scratch.path.to_str().expect("UTF-8 path"));
    // The first three characters the fixture's dictionary holds.
    let found: Vec<(&str, u32)> = ["你", "好", "我", "的", "是", "中", "国", "人"]
        .into_iter()
        .filter_map(|text| {
            // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into
            // it.
            unsafe { lookup(instance, text) }
                .first()
                .map(|&token| (text, token))
        })
        .take(3)
        .collect();
    assert_eq!(found.len(), 3, "the fixture holds three of the characters");
    let ((ni_text, ni), (hao_text, hao), (_, wo)) = (found[0], found[1], found[2]);
    unsafe { crate::support::pinyin_fini(context) };
    let probe = TempUserDir::new("export-order");
    crate::support::plant_bigram(
        &probe.path,
        vec![
            (ni.to_le_bytes().to_vec(), whole_gram(hao)),
            (wo.to_le_bytes().to_vec(), whole_gram(hao)),
        ],
    );
    let context = init_fixture_at(&probe.path);
    let iter = unsafe { crate::support::pinyin_begin_get_bigram_phrases(context) };
    let mut rows = Vec::new();
    while unsafe { crate::support::pinyin_bigram_iterator_has_next_phrase(iter) } {
        let mut phrase: *mut crate::support::GChar = ptr::null_mut();
        let mut pinyin: *mut crate::support::GChar = ptr::null_mut();
        let mut count: std::os::raw::c_int = 0;
        unsafe {
            crate::support::pinyin_bigram_iterator_get_next_phrase(
                iter,
                &raw mut phrase,
                &raw mut pinyin,
                &raw mut count,
            )
        };
        // SAFETY: `phrase` and `pinyin` are this step's outputs, each NULL or a separate string
        // the library returned to this code, which owns them and frees them only here.
        // `take_exported_row` fails the test if either is NULL.
        rows.push(unsafe { take_exported_row(phrase, pinyin, count) });
    }
    unsafe { crate::support::pinyin_end_get_bigram_phrases(iter) };
    unsafe { crate::support::pinyin_fini(context) };
    assert_eq!(rows.len(), 2, "{rows:?}");
    if rows[0].0 == format!("{ni_text}{hao_text}") {
        (ni, wo, hao)
    } else {
        (wo, ni, hao)
    }
}

// ---------------------------------------------------------------------
// NULL arguments the pin never reads (row 41's non-crashing probes). Each
// test hands valid objects and the one NULL argument the pin ignores; the
// expectations are the pin's answer at 074a2219 (cited beside each), not a
// pin run.
// ---------------------------------------------------------------------

/// A parsed pinyin key and key rest at offset 0 of `text`, borrowed from the
/// process-wide key slots (hold `key_slot_guard`).
///
/// # Safety
///
/// `instance` is NULL or a live instance: `pinyin_alloc_instance` returned it and
/// `pinyin_free_instance` has not freed it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
pub(crate) unsafe fn parsed_key_and_rest(
    instance: *mut PinyinInstance,
    text: &str,
) -> (
    *mut crate::support::ChewingKey,
    *mut crate::support::ChewingKeyRest,
) {
    let input = cstr(text);
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, input.as_ptr()) },
        text.len()
    );
    let mut key = ptr::null_mut();
    assert!(unsafe { crate::support::pinyin_get_pinyin_key(instance, 0, &raw mut key) });
    let mut rest = ptr::null_mut();
    assert!(unsafe { crate::support::pinyin_get_pinyin_key_rest(instance, 0, &raw mut rest) });
    (key, rest)
}

/// A partial item tail (`4 + 8k + 1` bytes): row 98's value, which the pin's
/// `retrieve_all` reads past (class (b)).
pub(crate) fn partial_gram(successor: u32) -> Vec<u8> {
    let mut gram = whole_gram(successor);
    gram.push(1);
    gram
}

/// One call that maps a system library again over a private system
/// directory whose library is a directory (register row 96): the save and
/// the mask-out map `merged.bin` (`pinyin.cpp:955`, `:1264`), the reload of
/// the unloaded GBK library maps `gbk_char.bin` (`:256`).
#[cfg(unix)]
pub(crate) fn remap_a_directory_library(site: &str) -> bool {
    let scratch = tempfile::tempdir().expect("scratch");
    let (system, context) = init_private(scratch.path());
    assert!(!context.is_null());
    match site {
        "mask" => {
            crate::support::directory_library(&system, "merged.bin");
            unsafe { pinyin_mask_out(context, 0xff00_0000, 0x0400_0000) }
        }
        "addon" => {
            crate::support::directory_library(&system, "art.bin");
            unsafe { crate::support::pinyin_load_addon_phrase_library(context, 4) }
        }
        _ => {
            assert!(unsafe { crate::support::pinyin_unload_phrase_library(context, 2) });
            crate::support::directory_library(&system, "gbk_char.bin");
            unsafe { crate::support::pinyin_load_phrase_library(context, 2) }
        }
    }
}

/// Reads a caller-owned rendered string (`g_free`-releasable) and frees it.
///
/// # Safety
///
/// `rendered` is NULL or a NUL-terminated string that the library returned to the caller, who owns
/// it. The call frees it with `g_free`, so the caller does not use it again.
pub(crate) unsafe fn take(rendered: *mut oxpinyin_capi::GChar) -> Option<String> {
    if rendered.is_null() {
        return None;
    }
    // SAFETY: The getters return a NUL-terminated UTF-8 buffer or null.
    let text = Some(
        unsafe { CStr::from_ptr(rendered) }
            .to_str()
            .expect("UTF-8 render")
            .to_owned(),
    );
    // SAFETY: the rendering ABI gives the caller an owned buffer to release with g_free.
    unsafe {
        glib_sys::g_free(rendered.cast());
    }
    text
}
