//! End-to-end tests: the training entry points, driven through the C ABI
//! functions themselves.
//!
//! The unit tests call the `#[unsafe(no_mangle)]` symbols directly (they are
//! ordinary crate functions when the crate is compiled for testing) and read
//! the counts back through the instance's own store handle — the committed
//! state, exactly as a T7 export iterator would observe it. (The store
//! backend refuses a second in-process write handle on the same file, so a
//! reopen-and-read would be the wrong shape anyway.) The system tables are
//! the committed mini fixture (`fixtures/w3`); no model bytes are added by
//! these tests.
//!
//! These are the §2 sequences through the wired path: the training seed
//! arithmetic (69, 138, 414, …), `sentence_start` as the first predecessor,
//! the predicted flat `+69`, remember-as-index-only, and
//! `InvalidPhrase` → `false`.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use oxpinyin_core::Dictionary as _;

use std::ptr;

use oxpinyin_user::USER_DICTIONARY;

use oxpinyin_user::{FIRST_USER_TOKEN, UserStore};

use crate::candidates::{
    pinyin_choose_candidate, pinyin_clear_constraint, pinyin_get_candidate,
    pinyin_is_user_candidate, pinyin_remove_user_candidate, pinyin_train,
};

use crate::config::{pinyin_mask_out, pinyin_set_options};

use crate::context::{pinyin_init, pinyin_save};

use crate::instance::{pinyin_alloc_instance, pinyin_reset};

use crate::iterators::{
    pinyin_begin_add_phrases, pinyin_begin_get_phrases, pinyin_end_add_phrases,
    pinyin_end_get_phrases, pinyin_iterator_add_phrase,
};

use crate::parse::pinyin_parse_more_full_pinyins;

use crate::sentence::{pinyin_get_sentence, pinyin_guess_candidates, pinyin_guess_sentence};

use crate::state::instance_ref;

use crate::test_support::{DEFAULT_SORT, TempUserDir, cstr, open, system_dir};

use crate::types::{LookupCandidate, PinyinInstance, lookup_candidate_type_t};

use crate::user_data::pinyin_remember_user_input;

/// Read the live store for one assertion; no storage reference escapes.
pub(crate) fn with_store<R>(
    instance: *mut PinyinInstance,
    read: impl FnOnce(&UserStore) -> R,
) -> R {
    // SAFETY: tests call this only with a live instance, between serialized
    // C operations. The closure cannot return a borrow into the store.
    let inst = unsafe { instance_ref(instance) };
    read(inst.core.user)
}

/// The token snapshotted on the candidate pointer.
fn token_of(instance: *mut PinyinInstance, cand: *mut LookupCandidate) -> u32 {
    // SAFETY: `cand` was produced by `pinyin_get_candidate` on `instance`
    // and no later guess invalidated the snapshot.
    let inst = unsafe { instance_ref(instance) };
    inst.state
        .candidates
        .iter()
        .find(|c| std::ptr::eq(*c, cand.cast::<crate::state::CapiCandidate>()))
        .and_then(|c| c.token.get())
        .expect("a phrase candidate carries its token")
        .value()
}

/// The first snapshot row of `kind`, with its token.
pub(crate) fn predicted_row(
    instance: *mut PinyinInstance,
    kind: lookup_candidate_type_t,
) -> (*mut LookupCandidate, u32) {
    // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
    let inst = unsafe { instance_ref(instance) };
    let index = inst
        .candidates
        .iter()
        .position(|c| c.candidate_type() == kind && c.token.get().is_some())
        .expect("the prediction holds a row of this kind");
    let mut cand: *mut LookupCandidate = ptr::null_mut();
    assert!(unsafe {
        pinyin_get_candidate(
            instance,
            u32::try_from(index).expect("small candidate index"),
            &raw mut cand,
        )
    });
    (cand, token_of(instance, cand))
}

/// Forces a phrase after the last sentence lookup that the decoded result
/// does not hold: the stale-forcing shape of `pinyin_train`.
fn force_a_stale_phrase(instance: *mut PinyinInstance) -> u32 {
    let input = cstr("zhongguo");
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, input.as_ptr()) },
        "zhongguo".len()
    );
    assert!(unsafe { pinyin_guess_sentence(instance) });
    assert!(unsafe { pinyin_guess_candidates(instance, 0, crate::test_support::DEFAULT_SORT) });
    // The last word row: not the phrase the sentence decoded at 0.
    let index = {
        // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
        let inst = unsafe { instance_ref(instance) };
        inst.state
            .candidates
            .iter()
            .rposition(|c| {
                c.candidate_type() == lookup_candidate_type_t::NORMAL_CANDIDATE
                    && c.token.get().is_some()
            })
            .expect("a word candidate")
    };
    let mut other: *mut LookupCandidate = ptr::null_mut();
    assert!(unsafe {
        pinyin_get_candidate(
            instance,
            u32::try_from(index).expect("small index"),
            &raw mut other,
        )
    });
    let forced = token_of(instance, other);
    assert!(unsafe { pinyin_choose_candidate(instance, 0, other) } > 0);
    forced
}

/// Looking the sentence up again after a stale forcing brings the two back
/// in step, and training goes ahead.
#[test]
fn a_new_sentence_lookup_trains_a_forcing() {
    let user_dir = TempUserDir::new("train-restep");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    force_a_stale_phrase(instance);
    assert!(unsafe { pinyin_guess_sentence(instance) });
    assert!(
        unsafe { pinyin_train(instance, 0) },
        "the lookup is back in step"
    );

    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };
}

// The deallocator for buffers the export iterators hand out (GLib and the
// conversion encoder allocate with libc `malloc`, which `g_free` and `free`
// both release on Linux).
unsafe extern "C" {
    fn free(ptr: *mut std::ffi::c_void);
}

/// Frees a caller-owned buffer the export iterators hand out.
fn free_exported(ptr: *mut std::ffi::c_char) {
    if !ptr.is_null() {
        // SAFETY: the iterator's buffers come from libc `malloc`, directly
        // or through GLib; `free` releases them.
        unsafe {
            free(ptr.cast());
        }
    }
}

/// Reads a caller-owned, NUL-terminated export string and frees it.
fn take_exported(ptr: *mut std::ffi::c_char) -> String {
    // SAFETY: the iterator hands out NUL-terminated, valid UTF-8 text.
    let text = unsafe { std::ffi::CStr::from_ptr(ptr) }
        .to_str()
        .expect("UTF-8 export string")
        .to_owned();
    free_exported(ptr);
    text
}

/// Drains an export iterator into owned `(phrase, pinyin, count)` triples,
/// freeing each caller-owned buffer as it goes.
fn drain_phrases(iter: *mut crate::types::ExportIterator) -> Vec<(String, String, i32)> {
    let mut rows = Vec::new();
    while unsafe { crate::iterators::pinyin_iterator_has_next_phrase(iter) } {
        let mut phrase: *mut crate::types::GChar = ptr::null_mut();
        let mut pinyin: *mut crate::types::GChar = ptr::null_mut();
        let mut count: std::os::raw::c_int = 0;
        assert!(unsafe {
            crate::iterators::pinyin_iterator_get_next_phrase(
                iter,
                &raw mut phrase,
                &raw mut pinyin,
                &raw mut count,
            )
        });
        rows.push((take_exported(phrase), take_exported(pinyin), count));
    }
    rows
}

/// The other half of the parse rule, after the R5 revert (register #8): a
/// composition a SELECTION consumed still continues when the buffer
/// evolved from it — upstream's parse path never touches the constraint
/// store (`pinyin.cpp:1497-1517`) and only `pinyin_reset` clears it
/// (`pinyin.cpp:2693-2704`), so the pre-revert fresh start (the frontend's
/// reset-between-compositions contract the #141 cursor flows pinned)
/// dropped forcings upstream keeps. The window stays anchored at the
/// stale composition offset, and dropping the surviving forcing re-opens
/// the walk at 0. A buffer that shrank to the cursor never was that
/// shape (the open-composition rule above).
#[test]
fn a_selection_committed_composition_reparses_with_its_store() {
    let user_dir = TempUserDir::new("constraint-committed");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    let input = cstr("nihao");
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, input.as_ptr()) },
        5
    );
    // No `pinyin_guess_sentence` here: it seeds the W14 n-best rows and
    // the \u{4f60}\u{597d} the list then offers is their row-0 mirror — a
    // row-0 choose constrains nothing (exactly upstream), and the
    // survival probe below would be vacuous. The plain guess offers the
    // \u{4f60}\u{597d} PHRASE, whose choose writes the forcing.
    assert!(unsafe { pinyin_guess_candidates(instance, 0, DEFAULT_SORT) });
    let full_ptr = {
        // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
        let inst = unsafe { instance_ref(instance) };
        let index = inst
            .candidates
            .iter()
            .position(|c| c.text_bytes() == "\u{4f60}\u{597d}".as_bytes())
            .expect("\u{4f60}\u{597d} is offered");
        let mut cand: *mut LookupCandidate = ptr::null_mut();
        assert!(unsafe {
            pinyin_get_candidate(
                instance,
                u32::try_from(index).expect("small candidate index"),
                &raw mut cand,
            )
        });
        cand
    };
    // Consumes the whole buffer: the commit branch.
    assert_eq!(unsafe { pinyin_choose_candidate(instance, 0, full_ptr) }, 5);

    // The next parse CONTINUES the committed composition: the forcing
    // survives into the new buffer, where the next guess's validate would
    // drop it if it stopped spelling. It still spells over 0..5, so the
    // run is live — the probe answers true exactly because the store
    // survived (the pre-revert rule answered false here).
    let next = cstr("nihaoshijie");
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, next.as_ptr()) },
        11
    );
    assert!(
        unsafe { pinyin_clear_constraint(instance, 0) },
        "the \u{4f60}\u{597d} forcing survived the committed re-parse"
    );
    assert!(
        !unsafe { pinyin_clear_constraint(instance, 0) },
        "the run is free now"
    );

    // Dropping the last forcing re-opens the composition at 0: an evolved
    // re-send keeps it, and the head window answers \u{4f60} again — the
    // old fresh-start expectation, now reached through the store's death.
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, next.as_ptr()) },
        11
    );
    assert!(unsafe { pinyin_guess_candidates(instance, 0, DEFAULT_SORT) });
    {
        // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
        let inst = unsafe { instance_ref(instance) };
        assert!(
            inst.state
                .candidates
                .iter()
                .any(|c| c.text_bytes() == "\u{4f60}".as_bytes()),
            "the re-opened composition offers \u{4f60} at offset 0"
        );
    }

    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };
}

/// The pin sorts the predicted rows with the stable `g_array_sort_with_data`
/// and `compare_item_with_sort_option` under `SORT_BY_PHRASE_LENGTH |
/// SORT_BY_FREQUENCY` (`pinyin.cpp:2443-2447` at `074a2219`; GLib's sort
/// is stable since 2.32), so tied rows keep their arrival order:
/// library by library (`reduce_tokens`, `phrase_large_table3.h:77-97`),
/// and inside one library the phrase table's key order over the
/// little-endian UCS-4 keys of the stored phrase text.
#[test]
fn predicted_tie_groups_keep_library_then_key_order_including_user_rows() {
    use crate::sentence::pinyin_guess_predicted_candidates_with_punctuations;
    use crate::types::lookup_candidate_type_t;

    /// Mirror of the engine's `amplified_frequency` at the pinned λ, so
    /// the expected order is computed without the production code.
    fn amplified(baked: u64, total: u64) -> u64 {
        if total == 0 {
            return 0;
        }
        let possibility = (1.0_f32 - 0.312_699_f32) * baked as f32 / total as f32;
        u64::from((possibility * 256.0 * 256.0 * 256.0) as u32)
    }

    /// The phrase table's key for `text`: its UCS-4 words, little-endian.
    fn ucs4_le_key(text: &str) -> Vec<u8> {
        text.chars()
            .flat_map(|ch| u32::from(ch).to_le_bytes())
            .collect()
    }

    let user_dir = TempUserDir::new("pred-defined-order");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    // Populate the USER seam: the prefix phrase 中 (a fixture phrase too,
    // so the system side also contributes), then two pairs of user
    // phrases under it with equal counts, so each pair ties.
    // - 中年 is added before 中华, so token order would put 年 first; key
    //   order (and text order) put 华 first.
    // - 中型 (型 U+578B, low byte 8B) and 中文 (文 U+6587, low byte 87):
    //   text order puts 型 first, little-endian key order puts 文 first.
    //   (The fixture already holds 中国 as a system phrase, so importing it
    //   would train that system item rather than add a user row.)
    //   This pair tells the pin's key order apart from text order.
    let iter = unsafe { pinyin_begin_add_phrases(context, 7) };
    assert!(!iter.is_null());
    for (phrase, pinyin, count) in [
        ("\u{4e2d}", "zhong", 9),
        ("\u{4e2d}\u{5e74}", "zhongnian", 7),
        ("\u{4e2d}\u{534e}", "zhonghua", 7),
        ("\u{4e2d}\u{578b}", "zhongxing", 7),
        ("\u{4e2d}\u{6587}", "zhongwen", 7),
    ] {
        assert!(unsafe {
            pinyin_iterator_add_phrase(iter, cstr(phrase).as_ptr(), cstr(pinyin).as_ptr(), count)
        });
    }
    unsafe { pinyin_end_add_phrases(iter) };

    let prefix = "\u{4e2d}";
    let prefix_c = cstr(prefix);
    assert!(unsafe {
        pinyin_guess_predicted_candidates_with_punctuations(instance, prefix_c.as_ptr())
    });

    // SAFETY: live instance immediately after the guess.
    let inst = unsafe { instance_ref(instance) };
    let total = inst.core.lm.amplified_total();
    // (text, length, amplified frequency, library, stored-text key)
    let rows: Vec<(String, usize, u64, usize, Vec<u8>)> = inst
        .candidates
        .iter()
        .filter(|c| c.candidate_type() == lookup_candidate_type_t::PREDICTED_PREFIX_CANDIDATE)
        .map(|c| {
            let token = c.token.get().expect("a predicted row has a token");
            let baked = inst
                .core
                .dict
                .live_items()
                .effective_unigram(token.value())
                .unwrap_or(0)
                + inst.core.user.unigram_delta(token.value());
            let text = c.text_str().expect("candidate text is UTF-8").to_owned();
            // A predicted row shows the phrase after the prefix; the
            // phrase table keys the whole stored phrase.
            let key = ucs4_le_key(&format!("{prefix}{text}"));
            let length = text.chars().count();
            (
                text,
                length,
                amplified(baked, total),
                oxpinyin_core::phrase_index_library(token.value()),
                key,
            )
        })
        .collect();

    // The user rows survived the prefix slice and reached the list.
    let user_position = |text: &str| {
        rows.iter()
            .position(|row| row.0 == text && row.3 == usize::from(USER_DICTIONARY))
            .unwrap_or_else(|| panic!("user row {text:?} missing from {rows:?}"))
    };
    let hua = user_position("\u{534e}");
    let nian = user_position("\u{5e74}");
    let xing = user_position("\u{578b}");
    let wen = user_position("\u{6587}");

    // THE PIN'S ORDER: the list is sorted by the comparator's keys (length
    // desc, amplified frequency desc), and inside every tie group the rows
    // keep their arrival order: library ascending, then the little-endian
    // UCS-4 key of the stored phrase text.
    for (i, (text_a, len_a, freq_a, library_a, key_a)) in rows.iter().enumerate() {
        for (text_b, len_b, freq_b, library_b, key_b) in rows.iter().skip(i + 1) {
            if len_a == len_b && freq_a == freq_b {
                assert!(
                    (library_a, key_a) <= (library_b, key_b),
                    "tie group out of the pin's order: {text_a:?} before {text_b:?} \
                     in {rows:?}"
                );
            }
        }
    }
    // Both pairs tie inside the user library.
    assert_eq!((rows[hua].1, rows[hua].2), (rows[nian].1, rows[nian].2));
    assert_eq!((rows[xing].1, rows[xing].2), (rows[wen].1, rows[wen].2));
    assert!(hua < nian, "key order, not token order: 华 before 年");
    assert!(
        wen < xing,
        "little-endian key order, not text order: 文 before 型"
    );
    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };
}

/// A lookup strictly inside a double-pinyin key answers the sentence rows
/// alone: the pin's matrix holds the key only on the column its key rest
/// begins on, so a mid-key `pinyin_guess_candidates` searches an empty
/// column (`pinyin.cpp:2224-2262`; the zhuyin facade's #577 law). Offset 0
/// and offset 2 are key starts and list phrases.
#[test]
fn a_mid_key_lookup_under_double_pinyin_lists_only_the_sentence_rows() {
    let user_dir = TempUserDir::new("double-mid-key");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    assert!(unsafe { crate::config::pinyin_set_double_pinyin_scheme(context, 2) });
    let keys = cstr("nihk");
    assert_eq!(
        unsafe { crate::parse::pinyin_parse_more_double_pinyins(instance, keys.as_ptr()) },
        4
    );
    assert!(unsafe { pinyin_guess_sentence(instance) });
    let kinds = |instance: *mut PinyinInstance| -> Vec<lookup_candidate_type_t> {
        // SAFETY: live instance immediately after a guess.
        let inst = unsafe { instance_ref(instance) };
        inst.state
            .candidates
            .iter()
            .map(|cd| cd.candidate_type())
            .collect()
    };
    for offset in [0, 2] {
        assert!(unsafe { pinyin_guess_candidates(instance, offset, DEFAULT_SORT) });
        assert!(
            kinds(instance).contains(&lookup_candidate_type_t::NORMAL_CANDIDATE),
            "offset {offset} is a key start and lists phrases"
        );
    }
    for offset in [1, 3] {
        assert!(unsafe { pinyin_guess_candidates(instance, offset, DEFAULT_SORT) });
        let kinds = kinds(instance);
        assert!(!kinds.is_empty(), "the sentence rows are listed");
        assert!(
            kinds
                .iter()
                .all(|kind| *kind == lookup_candidate_type_t::NBEST_MATCH_CANDIDATE),
            "offset {offset} is mid-key: sentence rows only"
        );
    }
    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };
}

/// The CR-flagged regression on PR #234: `RuntimeDict::phrase_prefix_exists`
/// used to short-circuit on the plain system probe, ignoring the
/// library-visibility mask. That leaked GBK-only continuations into
/// the n-best widen probe after `pinyin_unload_phrase_library(2)`,
/// and — as a side effect of the shared unload plumbing — kept GBK
/// candidate rows on the surface after a fresh scan too. This test
/// pins the surface: the candidate list rescanned after an unload must
/// carry no GBK-nibble tokens, and the reload must restore them. The
/// widen-probe path also has to survive the mask armed without
/// regressing the mixed rows the fixture happens to have.
#[test]
fn unloading_gbk_hides_its_tokens_from_the_candidate_surface() {
    use crate::config::{pinyin_load_phrase_library, pinyin_unload_phrase_library};

    let user_dir = TempUserDir::new("gbk-unload-hides");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    // A GBK-carrying prefix — `ni` — has GBK entries mixed with the
    // system ones on the fixture's `ni` row. The initial guess must
    // include at least one GBK candidate.
    let gbk_before = collect_gbk_candidate_tokens(instance, "ni");
    assert!(
        !gbk_before.is_empty(),
        "the mini fixture must carry at least one GBK candidate under ni"
    );

    // Unload GBK. A fresh parse re-scans through the visibility mask
    // (`RuntimeDict::lookup_into` retains only visible tokens); the
    // rescanned list must carry no GBK-nibble candidate.
    assert!(unsafe { pinyin_unload_phrase_library(context, 2) });
    assert!(unsafe { pinyin_reset(instance) });
    let gbk_after = collect_gbk_candidate_tokens(instance, "ni");
    assert!(
        gbk_after.is_empty(),
        "GBK tokens leaked into candidates after unload: {gbk_after:?}"
    );

    // The sentence-decode path drives `phrase_prefix_exists` through
    // the runtime dict. With the mask armed, the widen probe must
    // still terminate and the fixture's mixed rows must still
    // support a non-empty sentence — the survival guarantee the fix
    // preserves.
    assert!(unsafe { pinyin_guess_sentence(instance) });
    let mut sentence: *mut std::os::raw::c_char = ptr::null_mut();
    assert!(unsafe { pinyin_get_sentence(instance, 0, &raw mut sentence) });
    assert!(
        !sentence.is_null(),
        "sentence decode must yield an n-best row"
    );
    let text = crate::ffi::take_owned_cstr(sentence);
    assert!(!text.is_empty(), "sentence text is not empty");

    // Reload GBK: the entries return to the surface.
    assert!(unsafe { pinyin_load_phrase_library(context, 2) });
    assert!(unsafe { pinyin_reset(instance) });
    let gbk_reloaded = collect_gbk_candidate_tokens(instance, "ni");
    assert_eq!(
        gbk_reloaded, gbk_before,
        "GBK tokens must return in the same order after reload"
    );

    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };
}

/// Reset the instance, re-parse `input`, guess a fresh candidate list,
/// and return the GBK-nibble (nibble 2) tokens that surface.
fn collect_gbk_candidate_tokens(instance: *mut PinyinInstance, input: &str) -> Vec<u32> {
    let text = cstr(input);
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, text.as_ptr()) },
        input.len(),
        "full input parses"
    );
    assert!(unsafe { pinyin_guess_candidates(instance, 0, DEFAULT_SORT) });
    // SAFETY: live instance immediately after the guess.
    let inst = unsafe { instance_ref(instance) };
    inst.state
        .candidates
        .iter()
        .filter_map(|c| c.token.get().map(oxpinyin_core::PhraseToken::value))
        .filter(|token| (token >> 24) == 2)
        .collect()
}

/// The chewing batch seam's option forwarding — row 30 of the
/// compatibility policy: `pinyin_parse_more_chewings` passes the whole
/// option word through after the `ZHUYIN_CORRECT_ALL` strip
/// (`pinyin.cpp:1582-1609` at the pin), so a caller-set `FORCE_TONE`
/// reaches the zhuyin parser's per-keyboard law.
#[cfg(test)]
mod chewing_batch_force_tone {
    use crate::config::pinyin_set_options;

    use crate::parse::pinyin_parse_more_chewings;

    use crate::test_support::{TempUserDir, cstr, open};

    const PARITY: u32 = 0x18a;

    const USE_TONE: u32 = 0x20;

    const FORCE_TONE: u32 = 0x40;

    fn chewing_len(
        context: *mut crate::types::PinyinContext,
        instance: *mut crate::types::PinyinInstance,
        word: u32,
        input: &str,
    ) -> usize {
        assert!(unsafe { pinyin_set_options(context, word) });
        let text = cstr(input);
        unsafe { pinyin_parse_more_chewings(instance, text.as_ptr()) }
    }

    #[test]
    fn the_seam_strips_only_the_zhuyin_corrections() {
        // Without FORCE_TONE the forwarded word parses exactly as the
        // three-argument law did — the strip changes nothing a caller
        // can observe on the STANDARD keyboard (its corrections are the
        // parser's own, re-or'd inside `parse_detailed`), and
        // FORCE_TONE without USE_TONE stays inert (the nested check).
        let user_dir = TempUserDir::new("row30-strip");
        let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

        assert_eq!(chewing_len(context, instance, PARITY | USE_TONE, "su"), 2);
        assert_eq!(chewing_len(context, instance, PARITY | USE_TONE, "su3"), 3);
        assert_eq!(
            chewing_len(context, instance, PARITY | FORCE_TONE, "su"),
            2,
            "FORCE_TONE is nested inside USE_TONE"
        );

        unsafe { crate::instance::pinyin_free_instance(instance) };
        unsafe { crate::context::pinyin_fini(context) };
    }
}

#[test]
fn masking_system_imports_restores_originals_and_removes_appended_items() {
    let user_dir = TempUserDir::new("mask-system-imports");
    let dir = user_dir.path.to_str().expect("UTF-8 path");
    let (context, instance) = open(dir);
    let export = unsafe { pinyin_begin_get_phrases(context, 1) };
    let original = drain_phrases(export);
    unsafe { pinyin_end_get_phrases(export) };
    assert!(!original.is_empty());
    let import = unsafe { pinyin_begin_add_phrases(context, 1) };
    assert!(unsafe {
        pinyin_iterator_add_phrase(
            import,
            cstr(&original[0].0).as_ptr(),
            cstr(&original[0].1).as_ptr(),
            5,
        )
    });
    assert!(unsafe {
        pinyin_iterator_add_phrase(
            import,
            cstr("你你你").as_ptr(),
            cstr("ni'ni'ni").as_ptr(),
            5,
        )
    });
    unsafe { pinyin_end_add_phrases(import) };
    let export = unsafe { pinyin_begin_get_phrases(context, 1) };
    let modified = drain_phrases(export);
    unsafe { pinyin_end_get_phrases(export) };
    assert_ne!(modified, original);
    assert!(modified.iter().any(|row| row.0 == "你你你"));
    assert!(unsafe { crate::config::pinyin_mask_out(context, 0, 0) });
    let export = unsafe { pinyin_begin_get_phrases(context, 1) };
    assert_eq!(drain_phrases(export), original);
    unsafe { pinyin_end_get_phrases(export) };
    assert!(unsafe { pinyin_save(context) });
    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };
    let (context, instance) = open(dir);
    let export = unsafe { pinyin_begin_get_phrases(context, 1) };
    assert_eq!(drain_phrases(export), original);
    unsafe { pinyin_end_get_phrases(export) };
    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };
}

/// Opens `user_dir` over the fixture, answering the raw handle.
fn init_at(user_dir: &std::path::Path) -> *mut crate::types::PinyinContext {
    let system = cstr(system_dir().to_str().expect("UTF-8 path"));
    let user = cstr(user_dir.to_str().expect("UTF-8 path"));
    unsafe { pinyin_init(system.as_ptr(), user.as_ptr()) }
}

/// Register row 91's control: `merge` returns at a mismatched MODIFY before
/// `next_record` reads the bad header, and init completes.
#[test]
fn init_completes_when_the_merge_stops_before_a_bad_header() {
    let user_dir = TempUserDir::new("init-stop-before-header");
    let payload = [
        crate::test_support::logs::mismatched_modify(),
        crate::test_support::logs::header_token(),
    ]
    .concat();
    crate::test_support::plant_log(&user_dir.path, "gb_char.dbin", &payload);
    let context = init_at(&user_dir.path);
    assert!(!context.is_null());
    unsafe { crate::context::pinyin_fini(context) };
}

/// Row 85's control: a key the mask selects is removed without loading its
/// value (`ngram_bdb.cpp:231-238`), and the mask completes.
#[test]
fn mask_out_of_a_selected_short_key_completes() {
    let user_dir = TempUserDir::new("mask-short-gram-selected");
    crate::test_support::plant_bigram(
        &user_dir.path,
        vec![(1_u32.to_le_bytes().to_vec(), vec![7, 0, 0])],
    );
    let context = init_at(&user_dir.path);
    assert!(!context.is_null());
    assert!(unsafe { pinyin_mask_out(context, 0xffff_ffff, 1) });
    unsafe { crate::context::pinyin_fini(context) };
}

/// As above, for `:568`: the lone library-7 token names a stored phrase of
/// another length (`你好` -> the three-character `你好嗷`).
#[test]
fn remember_user_input_aborts_on_an_item_length_mismatch() {
    crate::test_support::assert_aborts(
        "e2e_tests::remember_user_input_aborts_on_an_item_length_mismatch",
        || {
            let user_dir = TempUserDir::new("remember-length");
            remember_against_a_redirected_row(&user_dir, "你好嗷", "ni'hao'ao");
        },
    );
}

/// As above, for `:571`: the lone library-7 token names another phrase of the
/// same length (`你好` -> `你們`).
#[test]
fn remember_user_input_aborts_on_an_item_text_mismatch() {
    crate::test_support::assert_aborts(
        "e2e_tests::remember_user_input_aborts_on_an_item_text_mismatch",
        || {
            let user_dir = TempUserDir::new("remember-text");
            remember_against_a_redirected_row(&user_dir, "你們", "ni'men");
        },
    );
}

/// The preflight runs only where `_remember_phrase_recur` reaches
/// `_add_phrase`. With the crafted 你好 row and only `ni` parsed, the one
/// path has one key for a two-character phrase and is refused at
/// `pinyin.cpp:3600-3601`, so the call answers `false`.
#[test]
fn remember_user_input_refuses_a_short_path_before_the_preflight() {
    let user_dir = TempUserDir::new("remember-short-path");
    crate::test_support::set_phrase_table_row(&user_dir.path, "你好", &[0x0700_0001, 0x0700_0002]);
    let (_context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, cstr("ni").as_ptr()) },
        2
    );
    assert!(!unsafe { pinyin_remember_user_input(instance, cstr("你好").as_ptr(), -1) });
}

/// A key the next character's item does not pronounce ends its branch
/// (`pinyin.cpp:3640-3645`): 好 does not pronounce `ni`, so 好你 reaches no
/// path under `nihao`, whatever its crafted row holds.
#[test]
fn remember_user_input_refuses_a_path_the_item_does_not_pronounce() {
    let user_dir = TempUserDir::new("remember-unpronounced");
    crate::test_support::set_phrase_table_row(&user_dir.path, "好你", &[0x0700_0001, 0x0700_0002]);
    let (_context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, cstr("nihao").as_ptr()) },
        5
    );
    assert!(!unsafe { pinyin_remember_user_input(instance, cstr("好你").as_ptr(), -1) });
}

/// Imports `phrase` into library 7 once per reading and returns its token.
fn import_readings(
    context: *mut crate::types::PinyinContext,
    instance: *mut PinyinInstance,
    phrase: &str,
    readings: &[&str],
) -> u32 {
    let import = unsafe { pinyin_begin_add_phrases(context, 7) };
    for reading in readings {
        assert!(unsafe {
            pinyin_iterator_add_phrase(import, cstr(phrase).as_ptr(), cstr(reading).as_ptr(), 1)
        });
    }
    unsafe { pinyin_end_add_phrases(import) };
    with_store(instance, |store| store.token_for_phrase(phrase))
        .expect("readable store")
        .expect("the import gave the phrase a token")
}

/// The stored readings of `token` as `(packed first key, frequency)`.
fn stored_readings(instance: *mut PinyinInstance, token: u32) -> Vec<(u16, u32)> {
    with_store(instance, |store| store.item_pronunciations(token))
        .expect("readable store")
        .expect("an item")
        .iter()
        .map(|(keys, freq)| (keys[0].to_packed(), *freq))
        .collect()
}

/// 弄 with its `nong` and `long` readings, parsed as `ns` in Microsoft
/// double pinyin under `L_N`. Returns the readings before any remember.
fn parsed_nong_under_l_n(
    user_dir: &TempUserDir,
) -> (
    *mut crate::types::PinyinContext,
    *mut PinyinInstance,
    u32,
    Vec<(u16, u32)>,
) {
    use crate::config::pinyin_set_double_pinyin_scheme;
    use crate::parse::pinyin_parse_more_double_pinyins;

    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    let token = import_readings(context, instance, "弄", &["nong", "long"]);
    let before = stored_readings(instance, token);
    assert_eq!(before.len(), 2);
    assert!(unsafe { pinyin_set_options(context, 0x8 | (1 << 15)) });
    assert!(unsafe { pinyin_set_double_pinyin_scheme(context, 2) });
    assert_eq!(
        unsafe { pinyin_parse_more_double_pinyins(instance, cstr("ns").as_ptr()) },
        2
    );
    (context, instance, token, before)
}

/// Both readings' frequencies rose by the count of 3.
fn assert_both_readings_rose(instance: *mut PinyinInstance, token: u32, before: &[(u16, u32)]) {
    let after = stored_readings(instance, token);
    assert_eq!(after.len(), 2);
    for ((_, was), (_, now)) in before.iter().zip(&after) {
        assert_eq!(*now, *was + 3);
    }
}

/// The matrix comes from the parse, not from the options now: turning `L_N`
/// off after the parse leaves `long` in it.
#[test]
fn remember_user_input_keeps_the_parse_time_fuzzy_alternatives() {
    let user_dir = TempUserDir::new("remember-parse-time-fuzzy");
    let (context, instance, token, before) = parsed_nong_under_l_n(&user_dir);
    assert!(unsafe { pinyin_set_options(context, 0x8) });
    assert!(unsafe { pinyin_remember_user_input(instance, cstr("弄").as_ptr(), 3) });
    assert_both_readings_rose(instance, token, &before);
}

/// Persists `other` in library 7, points 你好's phrase-table row at its
/// token, reopens, and remembers 你好 under the composition `nihao`.
fn remember_against_a_redirected_row(user_dir: &TempUserDir, other: &str, reading: &str) {
    let path = user_dir.path.to_str().expect("UTF-8 path").to_owned();
    let (context, instance) = open(&path);
    let it = unsafe { pinyin_begin_add_phrases(context, 7) };
    assert!(unsafe {
        pinyin_iterator_add_phrase(it, cstr(other).as_ptr(), cstr(reading).as_ptr(), 1)
    });
    unsafe { pinyin_end_add_phrases(it) };
    let token = with_store(instance, |store| store.token_for_phrase(other))
        .expect("readable store")
        .expect("the setup phrase has a token");
    assert!(unsafe { pinyin_save(context) });
    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };
    crate::test_support::set_phrase_table_row(&user_dir.path, "你好", &[token]);
    let (_context, instance) = open(&path);
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, cstr("nihao").as_ptr()) },
        5
    );
    let _ = unsafe { pinyin_remember_user_input(instance, cstr("你好").as_ptr(), -1) };
}

/// GLib log records the child's default handler collected:
/// `(domain, level, message)`.
static GLIB_LOG: std::sync::Mutex<Vec<(String, u32, String)>> = std::sync::Mutex::new(Vec::new());

/// Collects every GLib log record instead of printing it, so the child's
/// stderr holds only the library's raw `fprintf` lines.
extern "C" fn collect_glib_log(
    domain: *const std::ffi::c_char,
    level: glib_sys::GLogLevelFlags,
    message: *const std::ffi::c_char,
    _data: glib_sys::gpointer,
) {
    let text = |ptr: *const std::ffi::c_char| {
        if ptr.is_null() {
            String::new()
        } else {
            // SAFETY: GLib hands NUL-terminated strings that outlive the call.
            unsafe { std::ffi::CStr::from_ptr(ptr) }
                .to_string_lossy()
                .into_owned()
        }
    };
    GLIB_LOG
        .lock()
        .expect("log lock")
        .push((text(domain), level, text(message)));
}

/// `#919`, `table-conf-919-short-row-merged`: the five-word row
/// `default GB_DICTIONARY gbk_char.table gbk_char.bin gbk_char.dbin` plus
/// the next line's first word `SYSTEM_FILE` is one valid six-word row
/// (`fscanf("%255s ... %255s\n")` skips white space across newlines and
/// takes whole words, `table_info.cpp:254-258`), so GB_DICTIONARY is
/// re-pointed at the GBK files; the trailing `default RESERVED NULL NULL
/// NULL` is five words, `6 != num` drops it (`:260-261`). The library and
/// table then disagree and the pin reads a NULL item (SIGSEGV, class (b)),
/// so only oxpinyin's answer is defined. The session is the case's:
/// look up, learn a sentence, import a phrase, save, finalise.
///
/// The save writes every library's log to `<user file>.tmp` first and
/// renames afterwards (`pinyin.cpp:925-1090`); libraries 1 and 2 now share
/// `gbk_char.dbin`, so the second rename finds no tmp file and prints
/// `rename ... failed.` (`pinyin.cpp:1059-1062`), the one raw stderr line
/// beside the init's missing-marker line (`table_info.cpp:332`).
#[test]
fn table_conf_919_short_row_merged() {
    let run =
        crate::test_support::run_in_child("e2e_tests::table_conf_919_short_row_merged", |user| {
            let system = crate::test_support::private_system(user.parent().expect("scratch"));
            let conf = system.join("table.conf");
            let stock = std::fs::read_to_string(&conf).expect("stock table.conf");
            std::fs::remove_file(&conf).expect("unlink the link");
            std::fs::write(
                &conf,
                stock
                    + "default GB_DICTIONARY gbk_char.table gbk_char.bin gbk_char.dbin\n"
                    + "SYSTEM_FILE default RESERVED NULL NULL NULL\n",
            )
            .expect("crafted table.conf");
            // SAFETY: the handler is a plain function and `user_data` is unused.
            unsafe {
                glib_sys::g_log_set_default_handler(Some(collect_glib_log), ptr::null_mut());
            }
            let system_c = cstr(system.to_str().expect("UTF-8 path"));
            let user_c = cstr(user.to_str().expect("UTF-8 path"));
            let context =
                unsafe { crate::context::pinyin_init(system_c.as_ptr(), user_c.as_ptr()) };
            assert!(!context.is_null());
            for text in ["nihao", "yishu"] {
                let inst = unsafe { pinyin_alloc_instance(context) };
                unsafe { pinyin_parse_more_full_pinyins(inst, cstr(text).as_ptr()) };
                unsafe { pinyin_guess_sentence(inst) };
                let mut sentence: *mut std::ffi::c_char = ptr::null_mut();
                unsafe { pinyin_get_sentence(inst, 0, &raw mut sentence) };
                free_exported(sentence);
                assert!(unsafe { pinyin_guess_candidates(inst, 0, 0) }, "{text}");
                // The original GB table still points to 你 at slot 4645.
                // GB_DICTIONARY now reads that slot from gbk_char.bin.
                // SAFETY: inst is live and the guess just populated its snapshot.
                let snapshot = unsafe { instance_ref(inst) };
                assert!(
                    snapshot.state.candidates.iter().all(|candidate| {
                        candidate.token.get().map(|token| token.value()) != Some(0x0100_1225)
                    }),
                    "{text}"
                );
            }
            let inst = unsafe { pinyin_alloc_instance(context) };
            unsafe { pinyin_parse_more_full_pinyins(inst, cstr("nihao").as_ptr()) };
            unsafe { pinyin_guess_sentence(inst) };
            let train = unsafe { pinyin_train(inst, 0) };
            let it = unsafe { pinyin_begin_add_phrases(context, 7) };
            let add = unsafe {
                pinyin_iterator_add_phrase(
                    it,
                    cstr("测试词").as_ptr(),
                    cstr("ce'shi'ci").as_ptr(),
                    1,
                )
            };
            unsafe { pinyin_end_add_phrases(it) };
            let save = unsafe { pinyin_save(context) };
            unsafe { crate::context::pinyin_fini(context) };
            assert!(train && add && save, "train {train} add {add} save {save}");
            // The user files: GB_DICTIONARY's `gb_char.dbin` is gone, the
            // GBK file name serves libraries 1 and 2 (each library's log is a
            // 26-byte empty `MemoryChunk` here), the addon and network files
            // are 27 bytes and the imported phrase makes `user.bin` 71.
            let mut sizes = std::collections::BTreeMap::new();
            let mut marker = String::new();
            for entry in std::fs::read_dir(user).expect("user dir") {
                let entry = entry.expect("entry");
                let name = entry.file_name().to_string_lossy().into_owned();
                let bytes = std::fs::read(entry.path()).expect("read");
                if name == "user.conf" {
                    marker = String::from_utf8_lossy(&bytes).into_owned();
                } else {
                    sizes.insert(name, bytes.len());
                }
            }
            // The index and bigram stores are written in the backend's own
            // container format, so only their presence is fixed.
            let present: Vec<&str> = sizes.keys().map(String::as_str).collect();
            assert_eq!(
                present,
                [
                    "addon.bin",
                    "gbk_char.dbin",
                    "merged.dbin",
                    "network.bin",
                    "opengram.dbin",
                    "user.bin",
                    "user_bigram.db",
                    "user_phrase_index.bin",
                    "user_pinyin_index.bin",
                ]
            );
            for (name, size) in [
                ("addon.bin", 27),
                ("gbk_char.dbin", 26),
                ("merged.dbin", 26),
                ("network.bin", 27),
                ("opengram.dbin", 26),
                ("user.bin", 71),
            ] {
                assert_eq!(sizes[name], size, "{name}");
            }
            // The open counter the init raised, the fini lowered.
            // user.conf is written in text mode: CRLF on Windows.
            let newline = oxpinyin_input::TEXT_NEWLINE;
            assert!(
                marker.starts_with(&format!(
                    "binary format version:7{newline}model data version:14{newline}"
                )) && marker.ends_with(&format!("{newline}open counter:0{newline}")),
                "{marker}"
            );
            // Row 106 applies to a live item lookup failure here too.
            assert!(GLIB_LOG.lock().expect("log lock").is_empty());
        });
    let user = run.user_dir.display();
    // `g_build_filename` joins with the host's separator, and the UCRT's
    // text-mode stderr writes `\n` as `\r\n`.
    let sep = std::path::MAIN_SEPARATOR;
    let newline = oxpinyin_input::TEXT_NEWLINE;
    assert_eq!(
        String::from_utf8_lossy(&run.stderr),
        format!(
            "open {user}{sep}user.conf failed.\
             rename {user}{sep}gbk_char.dbin.tmp to {user}{sep}gbk_char.dbin failed.{newline}"
        )
    );
}

/// One step of the bigram export walk: `has_next` answered false, or a
/// row `get_next` handed out (`get_next`'s answer, phrase, pinyin, count).
#[derive(Debug, PartialEq)]
enum ExportStep {
    NoNext,
    Row(bool, String, String, i32),
}

/// Plants the user-gram layout of the `stored_user_gram` contract cases
/// (`user_gram_layout`) and walks the bigram export at most sixteen steps,
/// stopping after two consecutive `has_next` answers of false.
///
/// Session one adds the user phrase 龘龘 (`da2'da2`, count 5) to library 7
/// and saves. The gram under 你 is then replaced, `total` first and the
/// `(token, count)` items verbatim in the stored order the layout names,
/// and a second session walks the export. Every count passes the export
/// threshold (68); the walk doubles each count (register row 1).
fn export_walk(tag: &str, items: impl FnOnce(u32, u32, u32) -> Vec<(u32, u32)>) -> Vec<ExportStep> {
    let user_dir = TempUserDir::new(tag);
    let path = user_dir.path.to_str().expect("UTF-8 path").to_owned();
    let (context, instance) = open(&path);
    let it = unsafe { pinyin_begin_add_phrases(context, 7) };
    assert!(unsafe {
        pinyin_iterator_add_phrase(it, cstr("龘龘").as_ptr(), cstr("da2'da2").as_ptr(), 5)
    });
    unsafe { pinyin_end_add_phrases(it) };
    let (ni, hao, hao_alt, dalong) = {
        // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
        let inst = unsafe { instance_ref(instance) };
        let system = |text: &str| {
            inst.core
                .dict
                .tokens_for_text(text)
                .first()
                .expect("the fixture holds the character")
                .value()
        };
        let dalong = inst
            .core
            .user
            .token_for_phrase("龘龘")
            .expect("readable store")
            .expect("the imported phrase has a token");
        (system("你"), system("好"), system("号"), dalong)
    };
    assert!(unsafe { pinyin_save(context) });
    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };

    lay_gram(&user_dir.path, ni, &items(hao, hao_alt, dalong));
    walk_bigram_export(&user_dir.path)
}

/// Lays a planted `user_bigram.db` holding PREDECESSOR's gram of ITEMS, `(token, count)` in the
/// stored order with their total first, over the profile at USER_DIR.
fn lay_gram(user_dir: &std::path::Path, predecessor: u32, items: &[(u32, u32)]) {
    let total: u32 = items.iter().map(|&(_, count)| count).sum();
    let mut gram = total.to_le_bytes().to_vec();
    for &(token, count) in items {
        gram.extend_from_slice(&token.to_le_bytes());
        gram.extend_from_slice(&count.to_le_bytes());
    }
    // `plant_bigram` writes a whole profile; only its `user_bigram.db` is
    // wanted, laid over the session-one profile that holds the phrase.
    let planted = TempUserDir::new("export-gram-plant");
    crate::test_support::plant_bigram(
        &planted.path,
        vec![(predecessor.to_le_bytes().to_vec(), gram)],
    );
    std::fs::copy(
        planted.path.join("user_bigram.db"),
        user_dir.join("user_bigram.db"),
    )
    .expect("lay the planted bigram over the profile");
}

/// Opens USER_DIR and walks the bigram export at most sixteen steps, stopping after two
/// consecutive `has_next` answers of false.
fn walk_bigram_export(user_dir: &std::path::Path) -> Vec<ExportStep> {
    let context = init_at(user_dir);
    assert!(!context.is_null());
    let iter = unsafe { crate::iterators::pinyin_begin_get_bigram_phrases(context) };
    let mut steps = Vec::new();
    let mut falses = 0;
    while falses < 2 && steps.len() < 16 {
        if !unsafe { crate::iterators::pinyin_bigram_iterator_has_next_phrase(iter) } {
            falses += 1;
            steps.push(ExportStep::NoNext);
            continue;
        }
        falses = 0;
        let mut phrase: *mut crate::types::GChar = ptr::null_mut();
        let mut pinyin: *mut crate::types::GChar = ptr::null_mut();
        let mut count: std::os::raw::c_int = 0;
        let ret = unsafe {
            crate::iterators::pinyin_bigram_iterator_get_next_phrase(
                iter,
                &raw mut phrase,
                &raw mut pinyin,
                &raw mut count,
            )
        };
        steps.push(ExportStep::Row(
            ret,
            take_exported(phrase),
            take_exported(pinyin),
            count,
        ));
    }
    unsafe { crate::iterators::pinyin_end_get_bigram_phrases(iter) };
    unsafe { crate::context::pinyin_fini(context) };
    steps
}

/// `user-gram-unsorted-export`: 你's stored gram holds `[龘龘 90, 号 80,
/// 好 70]`, descending by token, not the ascending order the pin writes.
/// The pin's export reads `retrieve_all`, which appends the raw items in
/// stored order (`ngram.cpp:133-151` at 074a2219), and the walk hands the
/// array out in that order; the `std::lower_bound` over the raw array
/// (`:86-99`) is a lookup and does not re-sort it. The walk's first
/// `has_next` answers false (its do/while loads the last predecessor's gram
/// on a false answer, register row 36); the last row's `get_next` answer is
/// the walk's, `false` at the end of the gram.
#[test]
fn user_gram_unsorted_export_follows_the_stored_order() {
    let steps = export_walk("export-unsorted", |hao, hao_alt, dalong| {
        let mut items = vec![(dalong, 90), (hao_alt, 80), (hao, 70)];
        items.sort_by(|a, b| b.cmp(a));
        items
    });
    let row = |ok: bool, phrase: &str, pinyin: &str, count: i32| {
        ExportStep::Row(ok, phrase.to_owned(), pinyin.to_owned(), count)
    };
    assert_eq!(
        steps,
        [
            ExportStep::NoNext,
            row(true, "你龘龘", "ni'da2'da2", 180),
            row(true, "你号", "ni'hao", 160),
            row(false, "你好", "ni'hao", 140),
            ExportStep::NoNext,
            ExportStep::NoNext,
        ]
    );
}

/// `user-gram-duplicate-export`: 你's stored gram holds 好 and 龘龘 twice,
/// sorted: `[好 70, 好 80, 龘龘 90, 龘龘 100]`. `retrieve_all` appends
/// every stored item, duplicates included (`ngram.cpp:133-151` at
/// 074a2219), so the export lists each twice, in stored order.
#[test]
fn user_gram_duplicate_export_lists_every_stored_item() {
    let steps = export_walk("export-duplicate", |hao, _, dalong| {
        vec![(hao, 70), (hao, 80), (dalong, 90), (dalong, 100)]
    });
    let row = |ok: bool, phrase: &str, pinyin: &str, count: i32| {
        ExportStep::Row(ok, phrase.to_owned(), pinyin.to_owned(), count)
    };
    assert_eq!(
        steps,
        [
            ExportStep::NoNext,
            row(true, "你好", "ni'hao", 140),
            row(true, "你好", "ni'hao", 160),
            row(true, "你龘龘", "ni'da2'da2", 180),
            row(false, "你龘龘", "ni'da2'da2", 200),
            ExportStep::NoNext,
            ExportStep::NoNext,
        ]
    );
}

/// `_remember_phrase_recur` calls `_add_phrase` for every complete path and
/// ORs the answers (`pinyin.cpp:3654-3659`). With `L_N` the composition
/// `nong` holds two keys over one span, `nong` and `long`, and a character
/// whose item pronounces both is added along both paths: each reading's
/// frequency rises by the count.
#[test]
fn remember_user_input_adds_along_every_complete_path() {
    let user_dir = TempUserDir::new("remember-two-paths");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    let import = unsafe { pinyin_begin_add_phrases(context, 7) };
    for reading in ["nong", "long"] {
        assert!(unsafe {
            pinyin_iterator_add_phrase(import, cstr("弄").as_ptr(), cstr(reading).as_ptr(), 1)
        });
    }
    unsafe { pinyin_end_add_phrases(import) };
    let token = with_store(instance, |store| store.token_for_phrase("弄"))
        .expect("readable store")
        .expect("the import gave 弄 a token");
    let readings = |instance| {
        with_store(instance, |store| store.item_pronunciations(token))
            .expect("readable store")
            .expect("an item")
            .iter()
            .map(|(keys, freq)| (keys.len(), *freq))
            .collect::<Vec<_>>()
    };
    let before = readings(instance);
    assert_eq!(before.len(), 2, "two imported readings");

    assert!(unsafe { pinyin_set_options(context, 0x8 | (1 << 15)) });
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, cstr("nong").as_ptr()) },
        4
    );
    assert!(unsafe { pinyin_remember_user_input(instance, cstr("弄").as_ptr(), 3) });
    let after = readings(instance);
    assert_eq!(after.len(), before.len());
    for ((_, was), (_, now)) in before.iter().zip(&after) {
        assert_eq!(*now, *was + 3, "each path adds its reading once");
    }
}

// ── Pin capture of 2026-11-07 (tools/oracle/pin_capture.py, runs 1 and 2 identical) ──
//
// Each test below names its observation. The values are the pin's on the pinned
// libpinyin-data; every one asserted here depends only on the test's own imports, parse
// and options, so the w3 mini fixture holds it too.

/// Every row of the current candidate list as (type, string, n-best index of an NBEST row).
pub(crate) fn capture_rows(
    instance: *mut PinyinInstance,
) -> Vec<(lookup_candidate_type_t, String, Option<u8>)> {
    let mut count = 0;
    assert!(unsafe { crate::candidates::pinyin_get_n_candidate(instance, &raw mut count) });
    (0..count)
        .map(|index| {
            let mut cand: *mut LookupCandidate = ptr::null_mut();
            assert!(unsafe { pinyin_get_candidate(instance, index, &raw mut cand) });
            let mut kind = lookup_candidate_type_t::ZOMBIE_CANDIDATE;
            assert!(unsafe {
                crate::candidates::pinyin_get_candidate_type(instance, cand, &raw mut kind)
            });
            let mut text: *const crate::types::GChar = ptr::null();
            assert!(unsafe {
                crate::candidates::pinyin_get_candidate_string(instance, cand, &raw mut text)
            });
            // SAFETY: a true answer points `text` at the row's borrowed string.
            let text = unsafe { std::ffi::CStr::from_ptr(text) }
                .to_string_lossy()
                .into_owned();
            let nbest = (kind == lookup_candidate_type_t::NBEST_MATCH_CANDIDATE).then(|| {
                let mut index = 0_u8;
                assert!(unsafe {
                    crate::candidates::pinyin_get_candidate_nbest_index(
                        instance,
                        cand,
                        &raw mut index,
                    )
                });
                index
            });
            (kind, text, nbest)
        })
        .collect()
}

/// Imports ROWS into LIBRARY in one batch; answers each add's flag.
pub(crate) fn import_batch(
    context: *mut crate::types::PinyinContext,
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
pub(crate) fn export_of(
    context: *mut crate::types::PinyinContext,
    index: u32,
) -> Vec<(String, String, i32)> {
    let iter = unsafe { pinyin_begin_get_phrases(context, index) };
    assert!(
        !iter.is_null(),
        "begin never answers NULL on a live context (pinyin.cpp:662-687)"
    );
    let rows = drain_phrases(iter);
    unsafe { pinyin_end_get_phrases(iter) };
    rows
}

/// Saves, then reopens the context on the same user directory.
fn save_and_reopen(
    context: *mut crate::types::PinyinContext,
    instance: *mut PinyinInstance,
    user_dir: &std::path::Path,
) -> (*mut crate::types::PinyinContext, *mut PinyinInstance) {
    assert!(unsafe { pinyin_save(context) });
    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };
    open(user_dir.to_str().expect("UTF-8 path"))
}

/// Removal leaves m_modified alone (074a2219 pinyin.cpp:3726-3768),
/// so the clean save gate (pinyin.cpp:1132-1146) keeps the saved phrase.
#[test]
fn removing_the_last_saved_phrase_from_a_clean_context_does_not_persist() {
    let user_dir = TempUserDir::new("clean-last-removal");
    let path = user_dir.path.to_str().expect("UTF-8 path");
    let (context, instance) = open(path);
    let phrase = "测侧";
    assert_eq!(import_batch(context, 7, &[(phrase, "ce'ce", 7)]), [true]);
    let saved = export_of(context, 7);
    assert_eq!(saved.len(), 1);
    let (context, instance) = save_and_reopen(context, instance, &user_dir.path);

    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, cstr("cece").as_ptr()) },
        4
    );
    assert!(unsafe { pinyin_guess_candidates(instance, 0, DEFAULT_SORT) });
    let mut count = 0;
    assert!(unsafe { crate::candidates::pinyin_get_n_candidate(instance, &raw mut count) });
    let candidate = (0..count)
        .find_map(|index| {
            let mut candidate = ptr::null_mut();
            assert!(unsafe { pinyin_get_candidate(instance, index, &raw mut candidate) });
            unsafe { pinyin_is_user_candidate(instance, candidate) }.then_some(candidate)
        })
        .expect("the only user phrase is a candidate");
    assert!(unsafe { pinyin_remove_user_candidate(instance, candidate) });
    assert!(export_of(context, 7).is_empty());
    assert!(!unsafe { pinyin_save(context) });
    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };

    let (context, instance) = open(path);
    assert_eq!(export_of(context, 7), saved);
    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };
}

/// D1: an import into a slot with no sub-index answers false: `get_range` fails
/// (`pinyin.cpp:589`; `phrase_index.cpp:610-613`) and `result` stays false (`pinyin.cpp:608`).
/// Library 7 stays empty. Observations `import/library/0` and `import/library/15`.
///
/// D2: libraries 16 and 255 read past the 16-slot array (`phrase_index.cpp:611`,
/// `phrase_index.h:441`), undefined behaviour whose pinyin outcome is not stable (compatibility
/// register row 43: the audit's D-12 recorded 255 accepted). oxpinyin refuses them; no capture.
#[test]
fn an_import_into_a_slot_without_a_sub_index_is_refused() {
    let user_dir = TempUserDir::new("import-no-sub-index");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    for library in [0, 15, 16, 255] {
        assert_eq!(
            import_batch(context, library, &[("你好", "nihao", 1)]),
            [false],
            "library {library}"
        );
    }
    assert_eq!(export_of(context, 7), []);
    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };
}

/// D3: a second reading merged into an item is appended and never indexed (`pinyin.cpp:578`,
/// `phrase_index.cpp:88-90`), and lowers the item's pronunciation possibility, the matched
/// frequency over the total (`phrase_index.h:136-163`): 测侧 loses the sentence to 侧测 once its
/// `xi'xi` reading carries 9,000,000 of 10,000,000. Observation `import/dilution/cece`.
#[test]
fn an_unindexed_second_reading_dilutes_the_sentence_score() {
    let user_dir = TempUserDir::new("import-dilution");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    let sentence = || {
        let input = cstr("cece");
        assert_eq!(
            unsafe { pinyin_parse_more_full_pinyins(instance, input.as_ptr()) },
            4
        );
        assert!(unsafe { pinyin_guess_sentence(instance) });
        let mut text: *mut std::os::raw::c_char = ptr::null_mut();
        assert!(unsafe { pinyin_get_sentence(instance, 0, &raw mut text) });
        assert!(unsafe { pinyin_reset(instance) });
        crate::ffi::take_owned_cstr(text)
    };
    assert_eq!(
        import_batch(
            context,
            1,
            &[("测侧", "ce'ce", 1_000_000), ("侧测", "ce'ce", 600_000)]
        ),
        [true, true]
    );
    assert_eq!(sentence(), "测侧");
    assert_eq!(
        import_batch(context, 1, &[("测侧", "xi'xi", 9_000_000)]),
        [true]
    );
    assert_eq!(sentence(), "侧测");
    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };
}

/// D4: a keyed reparse leaves the n-best results in place (`pinyin.cpp:1497-1524`; only reset
/// clears them, `:2700`): the sentence reads the stale result (`:1464-1474`), and the candidate
/// list prepends it as its NBEST row (`:1934-1951`) until a new guess replaces it. Observation
/// `assembly/stale-nbest/nihao-zhongguo`; its row count is the pinned data's and is not asserted.
#[test]
fn a_reparse_keeps_the_stale_nbest_result_until_the_next_guess() {
    let user_dir = TempUserDir::new("stale-nbest");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    let sentence = || {
        let mut text: *mut std::os::raw::c_char = ptr::null_mut();
        assert!(unsafe { pinyin_get_sentence(instance, 0, &raw mut text) });
        crate::ffi::take_owned_cstr(text)
    };
    let nihao = cstr("nihao");
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, nihao.as_ptr()) },
        5
    );
    assert!(unsafe { pinyin_guess_sentence(instance) });
    assert_eq!(sentence(), "你好");
    let zhongguo = cstr("zhongguo");
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, zhongguo.as_ptr()) },
        8
    );
    assert_eq!(sentence(), "你好", "the stale result");
    assert!(unsafe { pinyin_guess_candidates(instance, 0, DEFAULT_SORT) });
    assert_eq!(
        capture_rows(instance)[0],
        (
            lookup_candidate_type_t::NBEST_MATCH_CANDIDATE,
            "你好".to_owned(),
            Some(0)
        )
    );
    assert!(unsafe { pinyin_guess_sentence(instance) });
    assert_eq!(sentence(), "中国");
    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };
}

/// D15: one-character user phrases under readings that differ only in the stored key, listed
/// for the incomplete key `n` (0x18a): their order is the user pinyin index's key order, the
/// ranges `convert` compresses (`storage/chewing_large_table2.h:91-133`). On the pinned data they
/// are rows 788-796 of 797, in this order; the system rows around them are the data's and are
/// not asserted. Observation `tie/user-n`.
#[test]
fn user_rows_for_an_incomplete_key_follow_the_stored_key_order() {
    let user_dir = TempUserDir::new("tie-user-n");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    // 𦰡 takes two readings, the second appended; the candidate order is `pinyin.cpp:2184-2308` at 074a2219.
    let phrases = [
        ("𦰡", "na"),
        ("𫐐", "ni"),
        ("𫠜", "ni"),
        ("𨺙", "ni"),
        ("𫔶", "nie"),
        ("𬪩", "nong"),
        ("𦰡", "nuo"),
        ("𫟅", "nen"),
        ("𫵷", "nao"),
        ("𬇕", "nu"),
    ];
    let rows: Vec<(&str, &str, i32)> = phrases
        .iter()
        .map(|&(phrase, reading)| (phrase, reading, 1))
        .collect();
    assert!(import_batch(context, 7, &rows).iter().all(|&added| added));
    assert!(unsafe { pinyin_set_options(context, 0x18a) });
    let n = cstr("n");
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, n.as_ptr()) },
        1
    );
    assert!(unsafe { pinyin_guess_sentence(instance) });
    assert!(unsafe { pinyin_guess_candidates(instance, 0, DEFAULT_SORT) });
    let user: Vec<String> = capture_rows(instance)
        .into_iter()
        .filter(|(_, text, _)| phrases.iter().any(|&(phrase, _)| phrase == text))
        .map(|(kind, text, _)| {
            assert_eq!(kind, lookup_candidate_type_t::NORMAL_CANDIDATE, "{text}");
            text
        })
        .collect();
    assert_eq!(user, ["𦰡", "𫵷", "𫟅", "𬪩", "𫐐", "𫠜", "𨺙", "𫔶", "𬇕"]);
    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };
}

/// D6: the bigram export lists an item only when its count is above
/// `threshold = initial_seed - 1`, 68 (`pinyin.cpp:791-792`, `:809` at 074a2219), and doubles
/// it (`:906`): 你's gram `[好 68, 号 69]` exports only 你号, at 138. Source-derived, with no
/// capture, per the 2026-11-01 ruling: the unpatched pin joins the pinyin strings with
/// `g_strjoinv` over an unterminated array (`:849`, `:862`; compatibility register row 1).
#[cfg(unix)]
#[test]
fn the_bigram_export_skips_a_count_at_the_threshold() {
    let steps = export_walk("export-threshold", |hao, hao_alt, _| {
        vec![(hao, 68), (hao_alt, 69)]
    });
    assert_eq!(
        steps,
        [
            ExportStep::NoNext,
            ExportStep::Row(false, "你号".to_owned(), "ni'hao".to_owned(), 138),
            ExportStep::NoNext,
            ExportStep::NoNext,
        ]
    );
}

/// D7: the export lists every reading of the predecessor against every reading of the
/// successor, the predecessor's outer (`pinyin.cpp:839-869` at 074a2219). 龘龘 holds two
/// readings, `da2'da2` and the appended `da4'da4` (`phrase_index.cpp:88-90`), and its own gram
/// holds `[龘龘 100]`: four rows, each at 200 (`:906`). `get_next` answers `has_next` after each
/// row (`:910`), so false only on the last. Source-derived, as D6 is (register row 1).
#[cfg(unix)]
#[test]
fn the_bigram_export_crosses_both_items_readings() {
    let user_dir = TempUserDir::new("export-cross-product");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    assert_eq!(
        import_batch(
            context,
            7,
            &[("龘龘", "da2'da2", 5), ("龘龘", "da4'da4", 5)]
        ),
        [true, true]
    );
    let dalong = {
        // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
        let inst = unsafe { instance_ref(instance) };
        inst.core
            .user
            .token_for_phrase("龘龘")
            .expect("readable store")
            .expect("the imported phrase has a token")
    };
    assert!(unsafe { pinyin_save(context) });
    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };
    lay_gram(&user_dir.path, dalong, &[(dalong, 100)]);
    let row = |more: bool, pinyin: &str| {
        ExportStep::Row(more, "龘龘龘龘".to_owned(), pinyin.to_owned(), 200)
    };
    assert_eq!(
        walk_bigram_export(&user_dir.path),
        [
            ExportStep::NoNext,
            row(true, "da2'da2'da2'da2"),
            row(true, "da2'da2'da4'da4"),
            row(true, "da4'da4'da2'da2"),
            row(false, "da4'da4'da4'da4"),
            ExportStep::NoNext,
            ExportStep::NoNext,
        ]
    );
}

fn unigram_item_count(instance: *mut crate::types::PinyinInstance, token: u32) -> (bool, u32) {
    let mut count = u32::MAX;
    let found =
        unsafe { crate::dict::pinyin_token_get_unigram_frequency(instance, token, &raw mut count) };
    (found, count)
}

/// U1: the add changes the item serialized by a later armed save
/// (074a2219 `phrase_index.cpp:173-175`, `pinyin.cpp:978-981,1132-1145`).
#[test]
fn user_unigram_increment_survives_a_later_save() {
    let user = TempUserDir::new("user-unigram-save");
    let path = user.path.to_str().unwrap();
    let (context, instance) = open(path);
    assert_eq!(import_batch(context, 7, &[("龘龘", "da2'da2", 5)]), [true]);
    let token = with_store(instance, |store| {
        store.token_for_phrase("龘龘").unwrap().unwrap()
    });
    let (found, base) = unigram_item_count(instance, token);
    assert!(found);
    assert!(unsafe { pinyin_save(context) });
    assert!(unsafe { crate::dict::pinyin_token_add_unigram_frequency(instance, token, 9) });
    assert!(
        !unsafe { pinyin_save(context) },
        "the unigram add alone leaves save unarmed"
    );
    assert!(import_batch(context, 7, &[]).is_empty());
    assert!(unsafe { pinyin_save(context) });
    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };

    let (context, instance) = open(path);
    assert_eq!(unigram_item_count(instance, token), (true, base + 9));
    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };
}

/// U4: the library and slot masks ignore the high nibble
/// (074a2219 `novel_types.h:41-44`, `phrase_index.cpp:154,186`).
#[test]
fn unigram_get_and_add_ignore_the_token_tag() {
    let user = TempUserDir::new("tagged-unigram");
    let (context, instance) = open(user.path.to_str().unwrap());
    assert_eq!(import_batch(context, 7, &[("龘龘", "da2'da2", 5)]), [true]);
    let tokens = {
        // SAFETY: the instance is live and this read ends before mutations.
        let inst = unsafe { instance_ref(instance) };
        [
            inst.core.dict.tokens_for_text("你")[0].value(),
            inst.core.user.token_for_phrase("龘龘").unwrap().unwrap(),
        ]
    };
    for token in tokens {
        let (found, base) = unigram_item_count(instance, token);
        assert!(found);
        let tagged = token | 0xf000_0000;
        assert_eq!(unigram_item_count(instance, tagged), (true, base));
        assert!(unsafe { crate::dict::pinyin_token_add_unigram_frequency(instance, tagged, 1) });
        assert_eq!(unigram_item_count(instance, token), (true, base + 1));
    }
    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };
}

/// U5: mask-out removes the live item and subtracts its whole unigram
/// from the facade total (074a2219 `phrase_index.cpp:233,713-715`).
#[test]
fn removing_a_user_item_takes_its_unigram_increment_out_of_the_total() {
    let user = TempUserDir::new("user-unigram-remove");
    let (context, instance) = open(user.path.to_str().unwrap());
    let total = || {
        // SAFETY: the instance is live for each call and no borrow escapes.
        unsafe { instance_ref(instance) }.core.lm.amplified_total()
    };
    let before = total();
    assert_eq!(import_batch(context, 7, &[("龘龘", "da2'da2", 0)]), [true]);
    let token = with_store(instance, |store| {
        store.token_for_phrase("龘龘").unwrap().unwrap()
    });
    assert!(unsafe { crate::dict::pinyin_token_add_unigram_frequency(instance, token, 9) });
    assert_eq!(total(), before + 9);
    assert!(unsafe { pinyin_mask_out(context, u32::MAX, token) });
    assert_eq!(unigram_item_count(instance, token), (false, 0));
    assert_eq!(total(), before);
    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };
}

/// U6: the persisted sub-index total is independent of item sums. The
/// pin guards that total and wraps the item's `guint32` (074a2219
/// `phrase_index.cpp:169-175,345`).
#[test]
fn user_unigram_item_wraps_when_its_sub_index_total_accepts_the_add() {
    use oxpinyin_data::chunk_format::{CHUNK_HEADER_SIZE, build_memory_chunk};
    use oxpinyin_data::chunk_write::{ChunkItem, build_chunk};

    let user = TempUserDir::new("user-unigram-wrap");
    let path = user.path.to_str().unwrap();
    let context = init_at(&user.path);
    assert!(!context.is_null());
    unsafe { crate::context::pinyin_fini(context) };
    let chunk = build_chunk(&[(
        1,
        ChunkItem {
            phrase: vec![u32::from('词')],
            unigram: u32::MAX - 2,
            prons: vec![],
        },
    )])
    .unwrap();
    let mut payload = chunk[CHUNK_HEADER_SIZE..].to_vec();
    payload[..4].copy_from_slice(&3_u32.to_le_bytes());
    std::fs::write(
        user.path.join("user.bin"),
        build_memory_chunk(&payload).unwrap(),
    )
    .unwrap();

    let (context, instance) = open(path);
    let token = FIRST_USER_TOKEN;
    assert_eq!(unigram_item_count(instance, token), (true, u32::MAX - 2));
    assert!(unsafe { crate::dict::pinyin_token_add_unigram_frequency(instance, token, 4) });
    assert_eq!(unigram_item_count(instance, token), (true, 1));
    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };
}

/// Oversized journal record payloads (compatibility policy row 123).
#[test]
fn oversized_journal_record_payloads_refuse_save_and_keep_modified() {
    use oxpinyin_data::chunk_write::{ChunkItem, build_chunk};

    let system = crate::test_support::TempSystemDir::new("oversized-journal-record");
    let user = TempUserDir::new("oversized-journal-record");
    let ni = oxpinyin_core::ChewingKey::from_pinyin("ni")
        .unwrap()
        .to_packed();
    // A 66,818-byte item fits the library format but exceeds a journal
    // record's length field. Direct unigram access avoids rendering it.
    let chunk = build_chunk(&[(
        1,
        ChunkItem {
            phrase: vec![u32::from('你'); 128],
            unigram: 100,
            prons: vec![(vec![ni; 128], 1); 255],
        },
    )])
    .unwrap();
    std::fs::write(system.path.join("gb_char.bin"), chunk).unwrap();
    let system_path = cstr(system.path.to_str().unwrap());
    let user_path = cstr(user.path.to_str().unwrap());
    let context = unsafe { pinyin_init(system_path.as_ptr(), user_path.as_ptr()) };
    assert!(!context.is_null());
    let instance = unsafe { pinyin_alloc_instance(context) };
    assert!(!instance.is_null());

    let token = 0x0100_0001;
    assert_eq!(unigram_item_count(instance, token), (true, 100));
    assert!(unsafe { crate::dict::pinyin_token_add_unigram_frequency(instance, token, 1) });
    assert_eq!(unigram_item_count(instance, token), (true, 101));
    // The direct unigram change does not arm save. Closing the empty
    // import sets the modified flag without adding a journal item.
    unsafe { pinyin_end_add_phrases(pinyin_begin_add_phrases(context, USER_DICTIONARY)) };
    assert!(with_store(instance, UserStore::is_modified));

    assert!(!unsafe { pinyin_save(context) });
    assert!(with_store(instance, UserStore::is_modified));
    assert!(!user.path.join("gb_char.dbin.tmp").exists());
    assert!(!user.path.join("gb_char.dbin").exists());

    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };
}

/// At 074a2219, `pinyin.cpp:812-815` reads both live items each time a pair
/// loads. `:904-910` copies the buffered row before loading the next pair.
#[test]
fn bigram_export_reads_a_new_pronunciation_when_the_next_pair_loads() {
    for live_successor in [false, true] {
        use crate::iterators::{
            pinyin_begin_get_bigram_phrases, pinyin_bigram_iterator_get_next_phrase,
            pinyin_bigram_iterator_has_next_phrase, pinyin_end_get_bigram_phrases,
        };

        let user_dir = TempUserDir::new("export-live-item");
        let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
        assert_eq!(import_batch(context, 7, &[("龘龘", "da2'da2", 5)]), [true]);
        let (user_token, system_token) = {
            // SAFETY: `instance` was just allocated and has not been freed.
            let inst = unsafe { instance_ref(instance) };
            let predecessor = inst.core.user.token_for_phrase("龘龘").unwrap().unwrap();
            (predecessor, inst.core.dict.tokens_for_text("好")[0].value())
        };
        let (predecessor, successor) = if live_successor {
            (system_token, user_token)
        } else {
            (user_token, system_token)
        };
        assert!(unsafe { pinyin_save(context) });
        unsafe { crate::instance::pinyin_free_instance(instance) };
        unsafe { crate::context::pinyin_fini(context) };
        lay_gram(
            &user_dir.path,
            predecessor,
            &[(successor, 69), (successor, 70)],
        );

        let context = init_at(&user_dir.path);
        assert!(!context.is_null());
        let iter = unsafe { pinyin_begin_get_bigram_phrases(context) };
        assert!(!unsafe { pinyin_bigram_iterator_has_next_phrase(iter) });
        assert!(unsafe { pinyin_bigram_iterator_has_next_phrase(iter) });
        assert_eq!(import_batch(context, 7, &[("龘龘", "da4'da4", 5)]), [true]);

        let row = || {
            let (mut phrase, mut pinyin, mut count) = (ptr::null_mut(), ptr::null_mut(), 0);
            let more = unsafe {
                pinyin_bigram_iterator_get_next_phrase(
                    iter,
                    &raw mut phrase,
                    &raw mut pinyin,
                    &raw mut count,
                )
            };
            ExportStep::Row(more, take_exported(phrase), take_exported(pinyin), count)
        };
        let expected = |more, reading: &str, count| {
            let (phrase, pinyin) = if live_successor {
                ("好龘龘", format!("hao'{reading}"))
            } else {
                ("龘龘好", format!("{reading}'hao"))
            };
            ExportStep::Row(more, phrase.to_owned(), pinyin, count)
        };
        assert_eq!(row(), expected(true, "da2'da2", 138));
        assert_eq!(row(), expected(true, "da2'da2", 140));
        assert_eq!(row(), expected(false, "da4'da4", 140));
        unsafe { pinyin_end_get_bigram_phrases(iter) };
        unsafe { crate::context::pinyin_fini(context) };
    }
}

/// Every complete path calls _add_phrase, including repeated keys
/// (pinyin.cpp:3594-3606). Repeated paths therefore repeat the count write.
#[test]
fn remember_user_input_writes_again_for_an_identical_complete_path() {
    let user = TempUserDir::new("remember-identical-paths");
    let (context, instance, token, before) = parsed_nong_under_l_n(&user);
    // This fixture adds an identical column entry to the parsed matrix,
    // as PhoneticTable::append does (phonetic_key_matrix.h:92-100).
    let duplicate = {
        // SAFETY: the live instance is exclusively borrowed before remember.
        let mut inst = unsafe { crate::state::instance_mut(instance) };
        let first = inst.core.parse_matrix[0];
        inst.core.parse_matrix.push(first);
        first.key().to_packed()
    };
    assert!(unsafe { pinyin_remember_user_input(instance, cstr("弄").as_ptr(), 3) });
    let after = stored_readings(instance, token);
    for ((key, was), (after_key, now)) in before.iter().zip(&after) {
        assert_eq!(key, after_key);
        assert_eq!(*now, *was + if *key == duplicate { 6 } else { 3 });
    }
    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };
}
