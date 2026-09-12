//! C ABI of libpinyin's zhuyin facade — the 52-symbol `libzhuyin.so.15`
//! surface, the `--enable-libzhuyin` counterpart to `libpinyin.so.15`.
//!
//! Upstream builds this as a SEPARATE shared object from `$(pinyin_SOURCES)
//! zhuyin.cpp` with its own version script (`src/Makefile.am:108-125`,
//! `configure.ac:138-144` at 0c5e80e1 — 2.11.91 — unchanged at the 074a2219 pin): `libzhuyin.so.15`, not
//! additional symbols in `libpinyin.so.15`. This crate mirrors that cut for
//! the Rust world: a new workspace member producing `libzhuyin.so.15`, with
//! no change to `oxpinyin-capi` (which keeps building `libpinyin.so.15`).
//!
//! ## Export boundary
//!
//! The authoritative export list is the checked-in `libzhuyin.ver` (copied
//! verbatim from upstream): 52 `zhuyin_*` symbols. `zhuyin_get_raw_user_input`
//! appears in `zhuyin.h` inside `#if 0` and is NOT in the `.ver` — it is not
//! exported.
//!
//! A Rust `cdylib` cannot apply a named version script at link time (rustc
//! merges its own anonymous script, and GNU ld rejects the pair — the same
//! reason `oxpinyin-capi`'s `build.rs` enforces scope in source, not by a
//! linker script). So the boundary is enforced by source construction: every
//! symbol in the `.ver` is `#[unsafe(no_mangle)] pub extern "C"`, and nothing
//! else is. The built `libzhuyin.so.15` is verified to export exactly the 52.
//! The `.ver` ships verbatim as the record and for the packaging step.
//!
//! ## Panic discipline
//!
//! Nothing here may panic on any input: the library crates on this
//! facade's path (`oxpinyin-core`, `-data`, `-store`, `-engine`, `-facade`,
//! `-runtime`, `-user` and this crate) deny `clippy::unwrap_used`/
//! `expect_used`/`panic`/`panic_in_result_fn` outside tests —
//! `oxpinyin-chewing` and the macro-only `oxpinyin-capi-marshal` carry no
//! such lint and are review-covered — so the entry-point bodies are
//! panic-free by construction. Rust (since 1.81) aborts the process when
//! a panic reaches an `extern "C"` boundary, so if a bug ever produced a
//! panic the failure would be a loud abort, not undefined behaviour.
//! There is deliberately no panic-catching wrapper: with the lints green
//! one is operationally inert, and abort-at-ABI makes the outcome the
//! same under either panic strategy (the release profile is `unwind`;
//! `abort` was tried and reverted for its keystroke-cycle cost, see
//! docs/perf/perf-baseline-kc-2026-09.md).
//!
//! Opaque handles cross as `*mut T` via `Box::into_raw` / `Box::from_raw`;
//! every incoming pointer is null-checked. `// SAFETY:` documents each
//! `unsafe` block.
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::panic))]
#![cfg_attr(not(test), deny(clippy::panic_in_result_fn))]
#![expect(
    unsafe_code,
    reason = "the C ABI crate; every block carries a SAFETY comment (constitution §5)"
)]
#![allow(
    clippy::not_unsafe_ptr_arg_deref,
    reason = "pointer-by-contract C ABI; fires only when the fuzz-api facade is compiled in, so an expectation would fail the default build"
)]
#![warn(missing_docs)]

mod candidates;
mod config;
mod context;
mod cursor;
mod dict;
mod ffi;
mod instance;
mod iterators;
mod keys;
mod parse;
mod phrase;
mod sentence;
mod state;
mod types;

// The pure-MIRROR and SHARED-CHEWING symbols are the public entry points;
// the four enum-touching symbols + zhuyin_init live where the zhuyin-local
// state is. Re-export for in-tree tooling.
pub use context::{zhuyin_fini, zhuyin_init, zhuyin_save};
pub use iterators::{zhuyin_begin_add_phrases, zhuyin_end_add_phrases, zhuyin_iterator_add_phrase};

// ── candidate / sentence re-exports for the harness ───────────────
pub use candidates::{
    zhuyin_choose_candidate, zhuyin_clear_constraint, zhuyin_get_candidate,
    zhuyin_get_candidate_string, zhuyin_get_candidate_type, zhuyin_get_n_candidate, zhuyin_train,
};
pub use sentence::{
    zhuyin_get_sentence, zhuyin_guess_candidates_after_cursor,
    zhuyin_guess_candidates_before_cursor, zhuyin_guess_sentence,
    zhuyin_guess_sentence_with_prefix,
};

// The behaviour battery: the training/import/dictionary entry points
// driven through the C symbols themselves, the zhuyin twin of
// oxpinyin-capi's e2e suite, over the shared test fixtures.
#[cfg(test)]
mod e2e_tests;
#[cfg(test)]
mod test_support;

#[cfg(test)]
mod tests {
    use std::ffi::c_char;
    use std::ptr;

    use super::candidates::{
        zhuyin_choose_candidate, zhuyin_clear_constraint, zhuyin_get_candidate,
        zhuyin_get_n_candidate,
    };
    use super::context::zhuyin_fini;
    use super::instance::zhuyin_free_instance;
    use super::parse::zhuyin_parse_more_chewings;
    use super::sentence::{
        zhuyin_get_character_offset, zhuyin_get_sentence, zhuyin_guess_candidates_before_cursor,
        zhuyin_guess_sentence,
    };
    use super::state::instance_mut;
    use super::test_support::{candidate_text, cstr, open};
    use super::types::{LookupCandidate, lookup_candidate_type_t};

    /// The Phase-1 correction, pinned: the zhuyin 4-value enum's exact
    /// discriminants. The zhuyin header (`zhuyin.h:41-45`) defines four
    /// enumerators, and they collide with the pinyin eight at 3 and 4 — so
    /// the enum must never be aliased to the pinyin one.
    #[test]
    fn zhuyin_candidate_type_discriminants_match_zhuyin_h() {
        assert_eq!(lookup_candidate_type_t::BEST_MATCH_CANDIDATE as i32, 1);
        assert_eq!(
            lookup_candidate_type_t::NORMAL_CANDIDATE_AFTER_CURSOR as i32,
            2
        );
        assert_eq!(
            lookup_candidate_type_t::NORMAL_CANDIDATE_BEFORE_CURSOR as i32,
            3
        );
        assert_eq!(lookup_candidate_type_t::ZOMBIE_CANDIDATE as i32, 4);
    }

    /// The zhuyin init seed is `USE_TONE | FORCE_TONE` (the pin's
    /// `zhuyin.cpp:273`), unlike `pinyin_init`'s `PINYIN_INCOMPLETE`. The
    /// constant is the wire value the context stores.
    #[test]
    fn zhuyin_default_options_is_use_tone_or_force_tone() {
        assert_ne!(
            super::state::ZHUYIN_DEFAULT_OPTIONS & oxpinyin_core::USE_TONE,
            0
        );
        assert_ne!(
            super::state::ZHUYIN_DEFAULT_OPTIONS & oxpinyin_core::FORCE_TONE,
            0
        );
        // No ZHUYIN_INCOMPLETE is seeded.
        assert_eq!(
            super::state::ZHUYIN_DEFAULT_OPTIONS & oxpinyin_core::PINYIN_INCOMPLETE,
            0
        );
    }

    /// The packed `ChewingKey` word is 2 bytes and the `ChewingKeyRest`
    /// span is 4 bytes, matching upstream's `_ChewingKey`/`_ChewingKeyRest`
    /// (`chewing_key.h`).
    #[test]
    fn opaque_handles_layout() {
        assert_eq!(std::mem::size_of::<super::types::ChewingKey>(), 2);
        assert_eq!(std::mem::size_of::<super::types::ChewingKeyRest>(), 4);
    }

    /// The zhuyin twin of the pinyin crate's
    /// `choosing_from_a_reanchored_window_uses_the_anchored_span`
    /// (`oxpinyin-capi/src/e2e_tests.rs`): a
    /// `zhuyin_guess_candidates_before_cursor` window is re-anchored, so
    /// the following `zhuyin_choose_candidate` must resolve the row's
    /// index against THAT window.
    ///
    /// `su3cl3` is `ni3'hao3` in the session's `'`-joined buffer, and the
    /// two lists provably differ at row 1: the composition-anchored cached
    /// list offers 你 (the first key's span), the `before(6)` window offers
    /// 好 (the second key's span, ending at the cursor). Resolving row 1
    /// through the cached list committed 你 and answered cursor 3 — a row
    /// the caller never displayed.
    #[test]
    fn choosing_from_a_before_cursor_window_uses_that_window() {
        let (context, instance) = open();
        let input = cstr("su3cl3");
        assert_eq!(
            zhuyin_parse_more_chewings(instance, input.as_ptr()),
            "su3cl3".len(),
            "the whole keystroke run parses"
        );

        assert!(zhuyin_guess_candidates_before_cursor(instance, 6));
        let mut count = 0;
        assert!(zhuyin_get_n_candidate(instance, &raw mut count));
        assert!(count > 1, "the before-cursor window carries several rows");

        // Row 0 is the prepended BEST_MATCH sentence row (你好); row 1 is
        // the first phrase row of the spans ending at the cursor.
        let displayed = candidate_text(instance, 1);
        assert_eq!(displayed, "好", "the fixture's before(6) row 1");
        let mut cand: *mut LookupCandidate = ptr::null_mut();
        assert!(zhuyin_get_candidate(instance, 1, &raw mut cand));
        assert!(!cand.is_null());

        // The chosen span is `[3, 6)` — the second key alone. Upstream
        // writes the constraint on `[m_begin, m_end)` and, for a
        // before-cursor row, answers `m_begin` as the new cursor
        // (`zhuyin.cpp:1656-1660` at the pin): 3, the span's start — not
        // the span's end. (The cached list's row 1 also ends at 3; the
        // committed text below is what tells the two rows apart.)
        assert_eq!(zhuyin_choose_candidate(instance, 6, cand), 3);
        // The session's own composition offset advanced to the span's end,
        // which is the whole joined buffer here.
        // SAFETY: the instance is live and no other reference is held.
        let session = unsafe { &instance_mut(instance).core.session };
        assert_eq!(
            session.composition_offset(),
            session.raw_input().len(),
            "the composition consumed through the chosen span's end"
        );

        // The forcing is the row's own span `[3, 6)`, not `[0, 6)`: the
        // first key's cell is free, the second key's is forced.
        // (`clear_constraint` answers false for a free cell and true for a
        // hit anywhere inside a forced run; probing 0 first cannot disturb
        // the run at 3.)
        assert!(
            !zhuyin_clear_constraint(instance, 0),
            "no forcing covers the first key"
        );

        // The pin's constrain-and-re-decode: a re-guess keeps the leading
        // key's conversion and the forced 好 (register, measured
        // 2026-09-05 on the pin-built oracle: 你好 after the choose).
        assert!(zhuyin_guess_sentence(instance));
        let mut sentence: *mut c_char = ptr::null_mut();
        assert!(zhuyin_get_sentence(instance, &raw mut sentence));
        assert!(!sentence.is_null());
        // SAFETY: `sentence` is a NUL-terminated string this facade just
        // malloc'd for the caller; it is read, then released with the
        // allocator that produced it.
        let decoded = unsafe { std::ffi::CStr::from_ptr(sentence) }
            .to_str()
            .expect("UTF-8")
            .to_owned();
        // SAFETY: `sentence` came from `malloc` in `owned_cstr` and is not
        // used after this call.
        unsafe { super::ffi::free(sentence.cast()) };
        assert_eq!(decoded, "你好", "the re-decode keeps 你 and forces 好");
        assert!(
            zhuyin_clear_constraint(instance, 3),
            "the forcing starts where 好 starts"
        );
        zhuyin_free_instance(instance);
        zhuyin_fini(context);
    }

    /// A before-cursor row whose span starts at the first key answers
    /// cursor 0 — upstream's `m_begin` (`zhuyin.cpp:1660`), which the end
    /// mapper would have turned into the first key's end.
    #[test]
    fn choosing_a_first_key_before_cursor_row_answers_zero() {
        let (context, instance) = open();
        let input = cstr("su3cl3");
        assert_eq!(zhuyin_parse_more_chewings(instance, input.as_ptr()), 6);
        assert!(zhuyin_guess_candidates_before_cursor(instance, 3));
        let mut count = 0;
        assert!(zhuyin_get_n_candidate(instance, &raw mut count));
        // Row 0 is the prepended BEST_MATCH sentence row; row 1 is the
        // first phrase row of the spans ending at 3, all of which start
        // at 0 (nothing precedes the first key).
        assert!(count > 1);
        let mut cand: *mut LookupCandidate = ptr::null_mut();
        assert!(zhuyin_get_candidate(instance, 1, &raw mut cand));
        // SAFETY: the instance is live; the borrow ends with the read.
        let kind = unsafe { instance_mut(instance) }.candidates[1].candidate_type;
        assert_eq!(
            kind,
            lookup_candidate_type_t::NORMAL_CANDIDATE_BEFORE_CURSOR
        );
        assert_eq!(
            zhuyin_choose_candidate(instance, 3, cand),
            0,
            "m_begin of a first-key span is 0, not the key's end"
        );
        assert!(
            zhuyin_clear_constraint(instance, 0),
            "the forcing starts at the first key"
        );
        zhuyin_free_instance(instance);
        zhuyin_fini(context);
    }

    /// The zhuyin twin of issue #356: `zhuyin_get_character_offset` runs
    /// the same phrase-table search and matrix walk (`zhuyin.cpp:2148`
    /// at the pin). `su3cl3` is 你好 on the standard keyboard: the
    /// keystroke string as the phrase answers `false`, the sentence
    /// counts one character per key at or before the offset.
    #[test]
    fn character_offset_searches_the_phrase_and_walks_the_keys() {
        let (context, instance) = open();
        let input = cstr("su3cl3");
        assert_eq!(zhuyin_parse_more_chewings(instance, input.as_ptr()), 6);

        let phrase = cstr("su3cl3");
        for offset in [0, 3, 6] {
            let mut length = usize::MAX;
            assert!(!zhuyin_get_character_offset(
                instance,
                phrase.as_ptr(),
                offset,
                &raw mut length
            ));
            assert_eq!(length, usize::MAX, "offset {offset}");
        }
        let phrase = cstr("你好");
        for (offset, expected) in [(0, 0), (3, 1), (6, 2)] {
            let mut length = usize::MAX;
            assert!(zhuyin_get_character_offset(
                instance,
                phrase.as_ptr(),
                offset,
                &raw mut length
            ));
            assert_eq!(length, expected, "offset {offset}");
        }

        zhuyin_free_instance(instance);
        zhuyin_fini(context);
    }
}
