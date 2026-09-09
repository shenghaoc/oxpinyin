//! The user-dir round trip with the pin-built libpinyin (drop-in task 9's
//! differential): a profile the pin trained must export identically
//! through oxpinyin's C ABI, and a profile oxpinyin trained and saved
//! must export identically through the pin.
//!
//! Driven by `tools/oracle/user-dir-round-trip.sh`, which builds the
//! pin-side C driver, runs both phases and diffs the pin's dumps. The
//! environment hands over the pin's user dir and dump, the dirs to use,
//! and the inputs; every input must be present — a missing one panics
//! rather than skipping (the house rule for tests that need inputs CI
//! never has).
//!
//! The count scaling matches on both sides by construction: the pin's
//! bigram export renders `m_count × 2` and so does ours
//! (`crates/oxpinyin-capi/src/iterators.rs`).
#![cfg(target_os = "linux")]

use std::ffi::{CStr, c_int};
use std::fs;
use std::panic;

use pinyin_capi::{
    PinyinContext, pinyin_alloc_instance, pinyin_begin_get_bigram_phrases,
    pinyin_begin_get_phrases, pinyin_bigram_iterator_get_next_phrase,
    pinyin_bigram_iterator_has_next_phrase, pinyin_end_get_bigram_phrases, pinyin_end_get_phrases,
    pinyin_fini, pinyin_free_instance, pinyin_guess_sentence, pinyin_init,
    pinyin_iterator_get_next_phrase, pinyin_iterator_has_next_phrase,
    pinyin_parse_more_full_pinyins, pinyin_save, pinyin_train,
};

// The matching deallocator for the export iterators' buffers
// (`ffi::owned_cstr` allocates with libc `malloc`, which `free` releases).
unsafe extern "C" {
    fn free(ptr: *mut std::ffi::c_void);
}

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("user-dir round trip: ${name} is required"))
}

fn cstr(text: &str) -> std::ffi::CString {
    std::ffi::CString::new(text).expect("NUL-free text")
}

/// One `P\t…` / `B\t…` line per exported row, sorted: the dump format
/// the C driver and this test share.
fn dump_through_the_abi(context: *mut PinyinContext) -> Vec<String> {
    let mut lines = Vec::new();

    let iter = pinyin_begin_get_phrases(context, 7);
    assert!(!iter.is_null(), "phrase export begins");
    while pinyin_iterator_has_next_phrase(iter) {
        let mut phrase: *mut std::ffi::c_char = std::ptr::null_mut();
        let mut pinyin: *mut std::ffi::c_char = std::ptr::null_mut();
        let mut count: c_int = -1;
        assert!(pinyin_iterator_get_next_phrase(
            iter,
            &raw mut phrase,
            &raw mut pinyin,
            &raw mut count
        ));
        // SAFETY: both pointers are non-null, NUL-terminated and owned by
        // this call until the `free` below.
        unsafe {
            lines.push(format!(
                "P\t{}\t{}\t{count}",
                CStr::from_ptr(phrase).to_string_lossy(),
                CStr::from_ptr(pinyin).to_string_lossy()
            ));
            free(phrase.cast());
            free(pinyin.cast());
        }
    }
    pinyin_end_get_phrases(iter);

    let iter = pinyin_begin_get_bigram_phrases(context);
    assert!(!iter.is_null(), "bigram export begins");
    while pinyin_bigram_iterator_has_next_phrase(iter) {
        let mut phrase: *mut std::ffi::c_char = std::ptr::null_mut();
        let mut pinyin: *mut std::ffi::c_char = std::ptr::null_mut();
        let mut count: c_int = -1;
        assert!(pinyin_bigram_iterator_get_next_phrase(
            iter,
            &raw mut phrase,
            &raw mut pinyin,
            &raw mut count
        ));
        // SAFETY: as above.
        unsafe {
            lines.push(format!(
                "B\t{}\t{}\t{count}",
                CStr::from_ptr(phrase).to_string_lossy(),
                CStr::from_ptr(pinyin).to_string_lossy()
            ));
            free(phrase.cast());
            free(pinyin.cast());
        }
    }
    pinyin_end_get_bigram_phrases(iter);

    lines.sort();
    lines
}

/// Drives one train pass: parse, guess, train the sentence n-best's
/// index 0 — the exact sequence the C driver performs.
fn train_through_the_abi(context: *mut PinyinContext, inputs: &[String]) {
    let instance = pinyin_alloc_instance(context);
    assert!(!instance.is_null(), "instance allocates");
    for input in inputs {
        let text = cstr(input);
        assert_eq!(
            pinyin_parse_more_full_pinyins(instance, text.as_ptr()),
            input.len(),
            "full input parses: {input}"
        );
        assert!(pinyin_guess_sentence(instance), "guess: {input}");
        assert!(pinyin_train(instance, 0), "train: {input}");
    }
    pinyin_free_instance(instance);
}

#[test]
#[ignore = "needs the pin-built oracle; run via tools/oracle/user-dir-round-trip.sh"]
fn the_user_dir_round_trips_with_the_pin() {
    let system = env("OX_SYSTEM_DIR");
    let pin_trained = env("OX_PIN_TRAINED_DIR");
    let pin_dump_path = env("OX_PIN_DUMP");
    let ox_trained = env("OX_OX_TRAINED_DIR");
    let ox_dump_path = env("OX_OX_DUMP");
    let inputs: Vec<String> = env("OX_INPUTS")
        .split_whitespace()
        .map(str::to_owned)
        .collect();

    let system_c = cstr(&system);

    // ---- Phase A: the pin's profile, read and exported by oxpinyin ----
    let pin_trained_c = cstr(&pin_trained);
    let context = pinyin_init(system_c.as_ptr(), pin_trained_c.as_ptr());
    assert!(!context.is_null(), "oxpinyin opens the pin-trained dir");
    let ours = dump_through_the_abi(context);
    pinyin_fini(context);

    let pin_dump = fs::read_to_string(&pin_dump_path)
        .unwrap_or_else(|e| panic!("read the pin dump {}: {e}", pin_dump_path));
    let mut pin_lines: Vec<&str> = pin_dump.lines().filter(|l| !l.is_empty()).collect();
    pin_lines.sort_unstable();
    assert_eq!(
        ours, pin_lines,
        "oxpinyin's export of the pin-trained profile differs from the pin's"
    );

    // ---- Phase B: oxpinyin trains and saves; the pin will read it ----
    fs::create_dir_all(&ox_trained).unwrap_or_else(|e| panic!("mkdir {}: {e}", ox_trained));
    let ox_trained_c = cstr(&ox_trained);
    let context = pinyin_init(system_c.as_ptr(), ox_trained_c.as_ptr());
    assert!(!context.is_null(), "oxpinyin opens the fresh dir");
    train_through_the_abi(context, &inputs);
    assert!(pinyin_save(context), "the dirty save writes the profile");
    let ours = dump_through_the_abi(context);
    pinyin_fini(context);

    // The values must equal the pin's own training of the same inputs:
    // the script diffs the pin's re-dump of this dir against the pin's
    // dump; here we assert the same equality on our side, so a failure
    // names the side that diverged.
    assert_eq!(
        ours, pin_lines,
        "oxpinyin's own trained profile exports differently than the pin's"
    );
    fs::write(&ox_dump_path, ours.join("\n") + "\n")
        .unwrap_or_else(|e| panic!("write {}: {e}", ox_dump_path));
}
