//! Stateful session fuzzer over the C ABI — the libchewing `fuzzer.rs`
//! shape (`docs/testing/upstream-test-strategies.md`). Each input byte
//! selects one command from the alphabet below; return values drive
//! subsequent state, so the harness walks real session lifecycles.
//!
//! Per the postconditions defined there: no per-call assertions on
//! return values — valid-sequence contracts are pinned by
//! contract_tests.rs; here a finding is any abort, panic, or sanitizer
//! report (cargo-fuzz's default `-s address` instrumentation includes
//! LeakSanitizer on Linux). Mispaired iterator lifecycles are UB by
//! contract (audit F-6), so every begin reaches its end within the same
//! command: the bug classes hunted are caller-visible mispairings, not
//! contract violations.

#![no_main]
// Scoped exception to this workspace's unsafe_code deny: an FFI-driven
// harness is the documented reason the fuzz crate uses deny, not forbid.
#![allow(unsafe_code)]

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::ptr::{null, null_mut};
use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use pinyin_capi::fuzz_api::{
    oxpinyin_init_for_fixtures, pinyin_alloc_instance, pinyin_begin_add_phrases,
    pinyin_begin_get_phrases, pinyin_choose_candidate, pinyin_clear_constraint,
    pinyin_end_add_phrases, pinyin_end_get_phrases, pinyin_get_candidate,
    pinyin_get_candidate_string, pinyin_get_n_candidate, pinyin_get_parsed_input_length,
    pinyin_get_sentence, pinyin_guess_sentence, pinyin_iterator_add_phrase,
    pinyin_iterator_get_next_phrase, pinyin_iterator_has_next_phrase,
    pinyin_parse_more_full_pinyins, pinyin_reset, pinyin_set_double_pinyin_scheme, pinyin_train,
    ExportIterator, GChar, ImportIterator, LookupCandidate, PinyinContext, PinyinInstance,
};

use std::os::raw::c_void;

// SAFETY: re-declaring libc `free` to release strings the ABI hands to the
// caller (g_free and free are equivalent on glibc, as documented in capi's
// ffi.rs); signature matches the C standard.
unsafe extern "C" {
    fn free(ptr: *mut c_void);
}

struct Session {
    context: *mut PinyinContext,
    instance: *mut PinyinInstance,
}

// SAFETY: the handles are only ever used from libFuzzer's single fuzzing
// thread; no other code touches this static, so the Send/Sync proofs only
// formalize that single-threaded reality.
unsafe impl Send for Session {}
unsafe impl Sync for Session {}

static SESSION: OnceLock<Session> = OnceLock::new();

fn session() -> &'static Session {
    SESSION.get_or_init(|| {
        let system = CString::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../fixtures/w3"))
            .expect("static path");
        let user =
            std::env::temp_dir().join(format!("oxpinyin-fuzz-capi-{}.d", std::process::id()));
        let _ = std::fs::remove_dir_all(&user);
        std::fs::create_dir_all(&user).expect("temp user dir");
        let user = CString::new(user.to_str().expect("UTF-8 temp path")).expect("no NUL");
        // SAFETY: fresh fixture context over the committed w3 tables and a
        // per-process temp user dir; both outlive the process.
        let context = oxpinyin_init_for_fixtures(system.as_ptr(), user.as_ptr());
        assert!(!context.is_null(), "fixture context must initialize");
        // SAFETY: `context` is the live context above.
        let instance = pinyin_alloc_instance(context);
        assert!(!instance.is_null(), "instance must allocate");
        Session { context, instance }
    })
}

/// Maps a byte slice to a short, NUL-free C string (caps length so the
/// fuzzer explores parse states, not multi-KB memcpys).
fn cstring_arg(bytes: &[u8]) -> CString {
    let filtered: Vec<u8> = bytes
        .iter()
        .take(64)
        .copied()
        .take_while(|&b| b != 0)
        .collect();
    CString::new(filtered).unwrap_or_default()
}

fuzz_target!(|data: &[u8]| {
    let session = session();
    let (context, instance) = (session.context, session.instance);
    // Each fuzz input starts from a clean instance; state still evolves
    // across the commands within one input.
    pinyin_reset(instance);
    let mut cursor = 0;
    while cursor < data.len() {
        let command = data[cursor];
        cursor += 1;
        let rest = &data[cursor..];
        match command % 11 {
            // reset the session state
            0 => {
                // SAFETY: live instance from the static session.
                pinyin_reset(instance);
            }
            // feed a pinyin string
            1 => {
                let pinyins = cstring_arg(rest);
                // SAFETY: live instance; `pinyins` is a valid CString.
                pinyin_parse_more_full_pinyins(instance, pinyins.as_ptr());
            }
            // guess + fetch the sentence (owned string → free)
            2 => {
                // SAFETY: live instance.
                if pinyin_guess_sentence(instance) {
                    let mut sentence: *mut c_char = null_mut();
                    // SAFETY: live instance; `sentence` is an out-param.
                    if pinyin_get_sentence(instance, 0, &mut sentence) && !sentence.is_null() {
                        // SAFETY: malloc'd by the ABI, released once here.
                        unsafe { free(sentence.cast()) };
                    }
                }
            }
            // candidate walk (borrowed strings — never freed)
            3 => {
                let mut count = 0_u32;
                // SAFETY: live instance; `count` is an out-param.
                if pinyin_get_n_candidate(instance, &mut count) {
                    for index in 0..count.min(8) {
                        let mut candidate: *mut LookupCandidate = null_mut();
                        // SAFETY: live instance; in-range index.
                        if pinyin_get_candidate(instance, index, &mut candidate)
                            && !candidate.is_null()
                        {
                            let mut text: *const GChar = null();
                            // SAFETY: `candidate` was just returned by the ABI.
                            pinyin_get_candidate_string(instance, candidate, &mut text);
                            if !text.is_null() {
                                // SAFETY: borrowed interior pointer from the
                                // instance snapshot; read-only, never freed.
                                let _ = unsafe { CStr::from_ptr(text) }.to_bytes().len();
                            }
                            // choose exercises the selection path
                            // SAFETY: live instance; `candidate` from the ABI.
                            pinyin_choose_candidate(instance, 0, candidate);
                        }
                    }
                }
            }
            // parsed-length accounting
            4 => {
                // SAFETY: live instance.
                let _ = pinyin_get_parsed_input_length(instance);
            }
            // train the current selection
            5 => {
                // SAFETY: live instance.
                pinyin_train(instance, 0);
            }
            // config setter incl. invalid discriminants (validated in the ABI)
            6 => {
                let scheme = rest.first().copied().unwrap_or(0) as c_int;
                // SAFETY: live context from the static session.
                pinyin_set_double_pinyin_scheme(context, scheme);
            }
            // import iterator trio — begin/add/end paired within the command
            7 => {
                // Two disjoint halves of the remaining bytes so both
                // inputs are non-empty whenever the input allows it.
                let phrase = cstring_arg(&rest[..rest.len() / 2]);
                let pinyin = cstring_arg(&rest[rest.len() / 2..]);
                // SAFETY: live context.
                let iter: *mut ImportIterator = pinyin_begin_add_phrases(context, 0);
                if !iter.is_null() {
                    // SAFETY: `iter` was just returned by its begin call.
                    pinyin_iterator_add_phrase(iter, phrase.as_ptr(), pinyin.as_ptr(), 1);
                    // SAFETY: transfers ownership back exactly once.
                    pinyin_end_add_phrases(iter);
                }
            }
            // export iterator trio — begin/walk/end paired within the command
            8 => {
                let index = rest.first().copied().unwrap_or(0) as u32;
                // SAFETY: live context.
                let iter: *mut ExportIterator = pinyin_begin_get_phrases(context, index);
                if !iter.is_null() {
                    // SAFETY: `iter` was just returned by its begin call.
                    while pinyin_iterator_has_next_phrase(iter) {
                        let mut phrase: *mut c_char = null_mut();
                        let mut pinyin: *mut c_char = null_mut();
                        let mut freq: c_int = 0;
                        // SAFETY: same live iterator; all out-params are locals.
                        if pinyin_iterator_get_next_phrase(
                            iter,
                            &mut phrase,
                            &mut pinyin,
                            &mut freq,
                        ) {
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
                    pinyin_end_get_phrases(iter);
                }
            }
            // constraint clearing at an arbitrary offset
            9 => {
                let offset = rest.first().copied().unwrap_or(0) as usize;
                // SAFETY: live instance.
                pinyin_clear_constraint(instance, offset);
            }
            // second parse flavor feeding the same session
            _ => {
                let pinyins = cstring_arg(rest);
                // SAFETY: live instance; `pinyins` is a valid CString.
                pinyin_parse_more_full_pinyins(instance, pinyins.as_ptr());
            }
        }
    }
});
