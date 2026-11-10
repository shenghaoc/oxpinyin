//! Phrase prediction: prefixes → user-bigram successors → prefix suggestions,
//! then punctuation prepended from the Option A punct table.
//!
//! Reproduces `pinyin_guess_predicted_candidates` (`pinyin.cpp:2411-2451`)
//! and the punctuation prefix of
//! `pinyin_guess_predicted_candidates_with_punctuations` (`:2454-2498`).

use oxpinyin_capi_marshal::phrase_text::phrase_text_bytes;
use std::collections::HashSet;
use std::os::raw::c_char;

use oxpinyin_core::{PhraseText, single_gram};
use oxpinyin_engine::CandidateKind;
use oxpinyin_user::UserStore;

use crate::state::{CapiCandidate, SharedDict, SharedLm, instance_mut};
use crate::types::{PinyinInstance, lookup_candidate_type_t};

/// Minimum user-bigram count for a predicted successor.
///
/// Copied from `_compute_predicted_bigram_candidates`:
/// `const guint32 filter = 10` (`pinyin.cpp:2311`) and
/// `if (phrase_item->m_count < filter) continue` (`pinyin.cpp:2349-2350`).
/// Not a design choice — public `pinyin_train` first-seeds 69, so the
/// 9-vs-10 edge is planted in the union data, not trained.
const BIGRAM_FILTER: u32 = 10;

/// One predicted item before sort/dedup.
struct Predicted {
    text: PhraseText,
    phrase_length: usize,
    token: u32,
    candidate_type: lookup_candidate_type_t,
    frequency: u32,
}

/// Fills `inst.state.candidates` with predicted phrases for `prefix`.
///
/// Returns `false` when the prefix matches no phrase-table suffix (upstream
/// returns false when `m_prefixes` stays empty).
pub fn guess_predicted(
    inst: &mut crate::state::CapiInstanceMut<'_>,
    prefix: Option<&PhraseText>,
) -> bool {
    guess_predicted_result(inst, prefix).unwrap_or(false)
}

/// `_compute_predicted_bigram_candidates`'s gram walk (`pinyin.cpp:2321-2341`
/// at the pin `074a2219`): for the prefixes from last to first, `load` the
/// prefix's user gram and `merge_single_gram(merged, NULL, user)` — a copy
/// of it, or, when the prefix has no gram, the previous copy kept — and
/// stop at the first whose `get_length` is not zero; then `retrieve_all`.
/// Answers the items in stored order, duplicates included, or the one
/// warning a call refuses with where the pin reads past a partial item
/// tail after a whole item (`retrieve_all`, `ngram.cpp:143`, register row
/// 98, class (b)). Where the pin asserts, the process aborts at the same
/// point of the walk: a value too short for its total (`get_length` reads
/// it, `memory_chunk.h:390`, register row 85) and a gram with no whole item
/// and a total left over (`ngram.cpp:70` via `pinyin.cpp:2332`, row 88).
/// Only the prefixes the walk loads are read; a fault under any other token
/// is never reached.
fn predicted_bigram_items(
    user: Option<&UserStore>,
    prefixes: &[u32],
) -> Result<Vec<(u32, u32)>, &'static str> {
    let Some(store) = user else {
        return Ok(Vec::new());
    };
    let mut merged = single_gram::new_gram();
    for &prev in prefixes.iter().rev() {
        if let Ok(Some(row)) = store.bigram_row(prev) {
            single_gram::merge_single_gram(&mut merged, None, Some(&row));
        }
        if single_gram::get_length(&merged) != 0 {
            break;
        }
        match single_gram::get_total_freq(&merged) {
            Some(0) => {}
            None | Some(_) => std::process::abort(),
        }
    }
    if single_gram::get_length(&merged) == 0 {
        return Ok(Vec::new());
    }
    if !(merged.len() - single_gram::TOTAL_LEN).is_multiple_of(single_gram::ITEM_LEN) {
        return Err(
            "pinyin_guess_predicted_candidates: a user bigram value has a partial item tail \
             (upstream reads past it, ngram.cpp:143)",
        );
    }
    Ok(single_gram::retrieve_all(&merged).collect())
}

// Err marks a refused gram walk (row 98), distinct from a missing
// prefix. The punctuation wrapper ignores a missing prefix but must refuse
// the walk.
fn guess_predicted_result(
    inst: &mut crate::state::CapiInstanceMut<'_>,
    prefix: Option<&PhraseText>,
) -> Result<bool, ()> {
    inst.state.candidates.clear();
    // `_compute_prefixes`' validating `reduce_tokens` (`pinyin.cpp:2424`,
    // `:1415`; `phrase_large_table3.h:95`) asserts on a tail substring with
    // more than four tokens, before the empty check.
    let Ok(prefixes) = oxpinyin_facade::compute_prefixes(&inst.core.dict, inst.core.user, prefix)
    else {
        std::process::abort();
    };
    // `m_prefixes` is emptied and refilled before the empty check
    // (`pinyin.cpp:2423-2426`).
    inst.state.prefixes.clone_from(&prefixes);
    inst.core.session.set_candidate_prefixes(
        prefixes
            .iter()
            .copied()
            .map(oxpinyin_core::PhraseToken::new),
    );
    let Some(prefix) = prefix.filter(|_| !prefixes.is_empty()) else {
        return Ok(false);
    };

    // `_compute_predicted_bigram_candidates` (`pinyin.cpp:2310-2341`). A
    // gram the pin reads past in its walk is refused here: one warning and
    // `false`, with the candidate list left empty in both APIs (the short
    // and the residual-total grams abort inside the walk).
    let successors = match predicted_bigram_items(inst.core.user, &prefixes) {
        Ok(successors) => successors,
        Err(message) => {
            crate::ffi::log_warning(message);
            return Err(());
        }
    };

    let mut items = Vec::new();
    append_predicted_bigrams(
        &inst.core.dict,
        &inst.core.lm,
        inst.state.predicted_lambda,
        inst.core.user,
        &successors,
        &mut items,
    );
    // pinyin.cpp:1859 asserts only for the general scorer branch, so
    // prefix-only rows do not reach it; a predicted bigram row does, and the
    // process aborts there (register row 61).
    if !items.is_empty() && predicted_total(&inst.core.lm) == 0 {
        std::process::abort();
    }
    let missing = append_predicted_prefix(
        &inst.core.dict,
        &inst.core.lm,
        inst.state.predicted_lambda,
        inst.core.user,
        prefix,
        &mut items,
    );

    if let Some(token) = missing {
        // Only one pre-dedup row: neither strcmp runs. Its opaque score and
        // length are unobservable; preserve the NULL getter and choose token.
        inst.state.candidates.push(CapiCandidate {
            text: oxpinyin_facade::TextSpan::NULL,
            text_ptr: std::ptr::null(),
            kind: CandidateKind::Phrase,
            type_tag: lookup_candidate_type_t::PREDICTED_PREFIX_CANDIDATE.tag(),
            nbest_index: 0,
            token: Some(oxpinyin_core::PhraseToken::new(token)),
            source_index: 0,
        });
        return Ok(true);
    }

    sort_predicted(&mut items);

    // Dedup retains the numerically higher score even if the pin's signed
    // comparator put it later (`pinyin.cpp:2128-2136`). Equal scores keep
    // the first row. The winner remains at its original sorted position.
    let mut winners = std::collections::HashMap::new();
    for (index, item) in items.iter().enumerate() {
        let winner = winners.entry(item.text.clone()).or_insert(index);
        if item.frequency > items[*winner].frequency {
            *winner = index;
        }
    }
    for (index, item) in items.into_iter().enumerate() {
        if winners.get(&item.text) != Some(&index) {
            continue;
        }
        let bytes = phrase_text_bytes(&item.text);
        let Some(text) = inst.state.candidates.intern(&bytes) else {
            continue;
        };
        inst.state.candidates.push(CapiCandidate {
            text,
            text_ptr: std::ptr::null(),
            kind: CandidateKind::Phrase,
            type_tag: item.candidate_type.tag(),
            nbest_index: 0,
            token: Some(oxpinyin_core::PhraseToken::new(item.token)),
            // Predicted candidates are chosen via
            // `pinyin_choose_predicted_candidate`, never the anchored-select
            // path, so the window index is unused here; record the snapshot
            // position for determinism.
            source_index: u32::try_from(inst.state.candidates.len()).unwrap_or(u32::MAX),
        });
    }
    Ok(true)
}

/// Stably rank predicted rows by phrase length and guarded wrapping frequency
/// (`pinyin.cpp:2440-2445`). Prediction has no pinyin-span key.
fn sort_predicted(items: &mut [Predicted]) {
    oxpinyin_engine::rank_by_frequency(items, |row| (row.phrase_length, 0, row.frequency));
}

/// Whether `prefix` is one of the instance's own candidate strings; if so,
/// empties the candidates and prediction prefixes without reading it.
///
/// At 074a2219, `pinyin_guess_predicted_candidates` frees that string in
/// `_free_candidates` and empties `m_prefixes` before `_compute_prefixes`
/// reads it (`pinyin.cpp:1335-1344`, `:2420-2424`). #542's ruling keeps
/// the defined part and the stable no-prefix outcome, by pointer identity;
/// the punctuation entry reaches the same call first (`:2459`), register
/// entry "Aliased prediction prefix use-after-free".
pub fn clear_borrowed_prefix(
    inst: &mut crate::state::CapiInstanceMut<'_>,
    prefix: *const c_char,
) -> bool {
    let borrowed = !prefix.is_null()
        && inst
            .candidates
            .iter()
            .any(|candidate| std::ptr::eq(candidate.text_ptr, prefix));
    if borrowed {
        inst.state.candidates.clear();
        inst.state.prefixes.clear();
        inst.core
            .session
            .set_candidate_prefixes(core::iter::empty());
    }
    borrowed
}

/// Guess predicted candidates for a prefix (plain variant).
///
/// # C signature
/// ```c
/// bool pinyin_guess_predicted_candidates(pinyin_instance_t * instance,
///                                        const char * prefix);
/// ```
///
/// The same pipeline `_with_punctuations` wraps, without the punctuation
/// prepend — and with the real retval: `false` when the prefix matches
/// no phrase-table suffix (`pinyin.cpp:2411-2452`; the `_with_punctuations`
/// entry discards this retval and always answers `true`).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_guess_predicted_candidates(
    instance: *mut PinyinInstance,
    prefix: *const c_char,
) -> bool {
    if instance.is_null() {
        return false;
    }

    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`.
    let mut inst = unsafe { instance_mut(instance) };
    if clear_borrowed_prefix(&mut inst, prefix) {
        return false;
    }
    let prefix =
        oxpinyin_capi_marshal::ucs4::prefix_text(oxpinyin_capi_marshal::borrow_c_string!(prefix));
    guess_predicted(&mut inst, prefix.as_ref())
}

///
/// Upstream returns `true` after the prepend, even when the prefix matched
/// no phrase-table suffix. The scorer's zero-total assert aborts before
/// prepending, as the pin's does (register row 61).
pub fn guess_predicted_with_punctuations(
    inst: &mut crate::state::CapiInstanceMut<'_>,
    prefix: Option<&PhraseText>,
) -> bool {
    if guess_predicted_result(inst, prefix).is_err() {
        return false;
    }
    // The pin walks `m_prefixes` as the prediction left it (`pinyin.cpp:2463`).
    let prefixes = inst.state.prefixes.clone();
    prepend_punctuations(inst, &prefixes);
    true
}

/// Prepends the punctuations of each prefix token, in prefix order then
/// table order, keeping the first of equal strings (`pinyin.cpp:2468-2494`
/// at 074a2219). The pin's `g_strv_contains` compares C strings with
/// `g_str_equal`; `phrase_text_bytes` is that string, so equal bytes are
/// the test. Existing candidates take no part, and the reverse
/// `g_array_prepend_val` leaves the punctuations first in this order.
fn prepend_punctuations(inst: &mut crate::state::CapiInstanceMut<'_>, prefixes: &[u32]) {
    let mut puncts: Vec<Vec<u8>> = Vec::new();
    let mut seen: HashSet<Vec<u8>> = HashSet::new();
    for token in prefixes {
        for punct in inst.core.dict.punctuations(*token) {
            let bytes = phrase_text_bytes(&punct).into_owned();
            if seen.insert(bytes.clone()) {
                puncts.push(bytes);
            }
        }
    }
    if puncts.is_empty() {
        return;
    }
    let rest = inst.state.candidates.take();
    for text in puncts {
        let Some(text) = inst.state.candidates.intern(&text) else {
            continue;
        };
        inst.state.candidates.push(CapiCandidate {
            text,
            text_ptr: std::ptr::null(),
            kind: CandidateKind::Phrase,
            type_tag: lookup_candidate_type_t::PREDICTED_PUNCTUATION_CANDIDATE.tag(),
            nbest_index: 0,
            token: None,
            source_index: u32::try_from(inst.state.candidates.len()).unwrap_or(u32::MAX),
        });
    }
    inst.state.candidates.append(rest);
}

fn append_predicted_bigrams(
    dict: &SharedDict,
    lm: &SharedLm,
    lambda: f32,
    user: Option<&UserStore>,
    successors: &[(u32, u32)],
    into: &mut Vec<Predicted>,
) {
    let Some(store) = user else {
        return;
    };
    let total = predicted_total(lm);
    for length in [2_usize, 1] {
        for (token, count) in successors {
            // pinyin.cpp:2349-2350: skip when `m_count < filter` (10).
            if *count < BIGRAM_FILTER {
                continue;
            }
            // Same library-mask gate as `compute_prefixes`: an unloaded
            // library's stored rows must not resolve into rendered
            // successor text.
            if !dict.library_visible_token(*token) {
                continue;
            }
            let Some(text) = phrase_text(dict, store, *token) else {
                continue;
            };
            let phrase_length = text.phrase_length();
            if phrase_length != length {
                continue;
            }
            into.push(Predicted {
                phrase_length,
                // The caller passes null_token to the scorer, so the bigram
                // term is zero even with DYNAMIC_ADJUST (`pinyin.cpp:2418,2438`).
                frequency: predicted_frequency(dict, Some(store), *token, total, lambda),
                text,
                token: *token,
                candidate_type: lookup_candidate_type_t::PREDICTED_BIGRAM_CANDIDATE,
            });
        }
    }
}

/// The pin's stale-length gate and missing-item cardinality
/// (`pinyin.cpp:2389-2405`, `:2443-2451`).
///
/// `last_read_length` is the loop-wide `PhraseItem`'s length: it starts at
/// the default header's zero and every successful item read replaces it
/// *before* the `2 * prefix_len + 1` check, including a read the check then
/// skips. A failed read leaves it unchanged, so a missing token can pass or
/// be skipped on the previous success's length.
///
/// `existing_rows` counts the pre-dedup rows already collected (the bigram
/// candidates). A sole missing row with no other row survives to the C
/// surface; with any other row the pin's dedup reaches the NULL string, so
/// class (b) takes row 106's answer and the missing token adds no candidate.
fn select_prefix_rows(
    suggestions: Vec<(u32, Option<PhraseText>)>,
    limit: usize,
    existing_rows: usize,
) -> (Vec<(u32, PhraseText)>, Option<u32>) {
    let mut last_read_length = 0;
    let mut kept = Vec::new();
    let mut missing = Vec::new();
    for (token, text) in suggestions {
        if let Some(text) = &text {
            last_read_length = text.phrase_length();
        }
        if last_read_length > limit {
            continue;
        }
        match text {
            Some(text) => kept.push((token, text)),
            None => missing.push(token),
        }
    }
    let sole_missing = if missing.len() == 1 && existing_rows + kept.len() == 0 {
        missing.pop()
    } else {
        None
    };
    (kept, sole_missing)
}

fn append_predicted_prefix(
    dict: &SharedDict,
    lm: &SharedLm,
    lambda: f32,
    user: Option<&UserStore>,
    prefix: &PhraseText,
    into: &mut Vec<Predicted>,
) -> Option<u32> {
    // `m_prefix_len`, the converted word count (`pinyin.cpp:2395`, `:2401`).
    let prefix_len = prefix.outward_words().count();
    if prefix_len == 0 {
        return None;
    }
    let limit = prefix_len.saturating_mul(2).saturating_add(1);
    // The facade merge reproduces reduce_tokens: library groups first,
    // then each DBM's UCS-4 byte cursor order. The stable length/score sort
    // preserves this collection order on ties, including user/system ties.
    let suggestions = oxpinyin_facade::merged_suggestions(dict, user, prefix);
    let total = predicted_total(lm);
    let (kept, sole_missing) = select_prefix_rows(suggestions, limit, into.len());
    for (token, text) in kept {
        let phrase_length = text.phrase_length();
        // The prefix subtraction the pin applies twice (`pinyin.cpp:1976-1980`):
        // the display string is sliced from `m_begin` (`:2018-2023`) and the
        // phrase-length sort key subtracts it. Storing the sliced text here
        // drives both — the sort counts `text.chars()`, the dedup and the
        // emitted candidate reuse the same string, matching upstream's
        // `_remove_duplicated_items_by_phrase_string` on the final string.
        let display = text.skip_chars(prefix_len);
        let display_length = display
            .char_count()
            .unwrap_or_else(|| phrase_length.saturating_sub(prefix_len));
        // The sort key is the amplified law, not the raw count: the pin's
        // PREDICTED_PREFIX branch computes `(1−λ)·unigram/total·2²⁴`
        // truncated (`pinyin.cpp:1811-1824`), the same law the normal
        // candidate path pins.
        into.push(Predicted {
            frequency: predicted_frequency(dict, user, token, total, lambda),
            text: display,
            phrase_length: display_length,
            token,
            candidate_type: lookup_candidate_type_t::PREDICTED_PREFIX_CANDIDATE,
        });
    }
    sole_missing
}

/// The live facade total is guint32 and wraps, including both training
/// deltas and ABI add overlays (`phrase_index.h:633`, `pinyin.cpp:1814-1815`).
fn predicted_total(lm: &SharedLm) -> u64 {
    lm.amplified_total()
}

/// Both predicted row kinds read the live item counter. Imports, training
/// and ABI increments share the same live phrase items.
fn predicted_frequency(
    dict: &SharedDict,
    user: Option<&UserStore>,
    token: u32,
    total: u64,
    lambda: f32,
) -> u32 {
    let store_count = || {
        user.and_then(|store| store.unigram_delta(token).ok())
            .unwrap_or(0)
    };
    // Read the system item or the user arena's count.
    let unigram = dict
        .live_items()
        .effective_unigram(token)
        .unwrap_or_else(store_count);
    // The pin's candidate `m_freq` for predicted rows is the same
    // expression as the lookup candidates' (`pinyin.cpp:1811-1824`, the
    // `PREDICTED_PREFIX` branch), so the engine's function computes it.
    oxpinyin_engine::amplified_frequency(unigram, total, lambda)
}

pub(crate) fn phrase_text(dict: &SharedDict, store: &UserStore, token: u32) -> Option<PhraseText> {
    if let Ok(Some(phrase)) = store.phrase(token) {
        return phrase.phrase_text();
    }
    dict.token_text(token)
}

#[cfg(test)]
mod tests {
    #[test]
    fn predicted_sort_is_stable_and_uses_wrapping_frequency_after_length() {
        use super::{Predicted, sort_predicted};
        use crate::types::lookup_candidate_type_t;
        use oxpinyin_core::PhraseText;
        let mut items: Vec<_> = [(1, 2, 1), (2, 3, 0), (3, 2, u32::MAX), (4, 2, 1), (5, 2, 0)]
            .into_iter()
            .map(|(token, phrase_length, frequency)| Predicted {
                text: PhraseText::from("test"),
                phrase_length,
                token,
                candidate_type: lookup_candidate_type_t::PREDICTED_PREFIX_CANDIDATE,
                frequency,
            })
            .collect();
        sort_predicted(&mut items);
        assert_eq!(
            items.iter().map(|i| i.token).collect::<Vec<_>>(),
            [2, 1, 4, 5, 3]
        );
    }
    use super::select_prefix_rows;
    use oxpinyin_core::PhraseText;

    #[test]
    fn a_sole_missing_suggestion_survives_the_length_gate() {
        let (kept, sole) = select_prefix_rows(vec![(7, None)], 5, 0);
        assert!(kept.is_empty());
        assert_eq!(sole, Some(7));
    }

    #[test]
    fn a_missing_suggestion_with_any_other_row_adds_no_candidate() {
        // A live row and the missing row: two pre-dedup rows, class (b).
        let (kept, sole) =
            select_prefix_rows(vec![(1, Some(PhraseText::from("abc"))), (2, None)], 5, 0);
        assert_eq!(kept.len(), 1);
        assert_eq!(sole, None);
        // A preexisting bigram row is enough even without a live prefix row.
        let (kept, sole) = select_prefix_rows(vec![(2, None)], 5, 1);
        assert!(kept.is_empty());
        assert_eq!(sole, None);
        // Two missing rows also reach the NULL comparison.
        let (kept, sole) = select_prefix_rows(vec![(2, None), (3, None)], 5, 0);
        assert!(kept.is_empty());
        assert_eq!(sole, None);
    }

    #[test]
    fn the_stale_length_skips_a_missing_suggestion_after_a_skipped_read() {
        // The long live row is itself skipped, but its successful read still
        // updates the loop-wide length, so the following missing row is
        // skipped too rather than becoming a sole NULL candidate.
        let (kept, sole) =
            select_prefix_rows(vec![(1, Some(PhraseText::from("abcdef"))), (2, None)], 5, 0);
        assert!(kept.is_empty());
        assert_eq!(sole, None);
    }

    #[test]
    fn a_short_live_row_after_the_stale_skip_still_replaces_the_gate() {
        // The skipped long row leaves its own length; the missing row is
        // skipped on it. A later short successful read replaces the gate and
        // is appended normally.
        let (kept, sole) = select_prefix_rows(
            vec![
                (1, Some(PhraseText::from("abcdef"))),
                (2, None),
                (3, Some(PhraseText::from("ab"))),
            ],
            5,
            0,
        );
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].0, 3);
        assert_eq!(sole, None);
    }

    // ── The predicted ranking at the C boundary (mini fixture) ──

    use crate::config::pinyin_set_options;
    use crate::e2e_tests::{capture_rows, import_batch};
    use crate::test_support::{TempUserDir, lookup, open};
    use crate::types::lookup_candidate_type_t;
    use crate::types::{PinyinContext, PinyinInstance};

    /// `PINYIN_INCOMPLETE | USE_DIVIDED_TABLE | USE_RESPLIT_TABLE | IS_PINYIN`.
    const OPTIONS: u32 = 0x18a;
    /// `DYNAMIC_ADJUST`.
    const DYNAMIC_ADJUST: u32 = 1 << 9;

    fn close(context: *mut PinyinContext, instance: *mut PinyinInstance) {
        crate::instance::pinyin_free_instance(instance);
        crate::context::pinyin_fini(context);
    }

    /// The texts of the rows `pinyin_guess_predicted_candidates` lists
    /// after `prefix`.
    fn predicted_texts(instance: *mut PinyinInstance, prefix: &std::ffi::CStr) -> Vec<String> {
        assert!(super::pinyin_guess_predicted_candidates(
            instance,
            prefix.as_ptr()
        ));
        capture_rows(instance)
            .into_iter()
            .map(|(_, text, _)| text)
            .collect()
    }

    /// The predicted scorer passes no predecessor, so neither DYNAMIC_ADJUST
    /// nor the bigram counts move a row (`pinyin.cpp:2418`, `:2438`): two
    /// successors of 你 rank the same whichever has the larger count.
    #[test]
    fn predicted_rows_ignore_dynamic_adjust_and_the_bigram_counts() {
        let user = TempUserDir::new("predict-bigram-terms");
        let (context, instance) = open(user.path.to_str().expect("UTF-8 path"));
        let [ni, hao, zhong] = ["你", "好", "中"].map(|text| lookup(instance, text)[0]);
        let mut lists = Vec::new();
        for (hao_count, zhong_count) in [(1000, 20), (20, 1000)] {
            // SAFETY: the live context has no reader retained across this write.
            let ctx = unsafe { crate::state::context_mut(context) };
            let store = ctx.core.user_store_mut().expect("a user store");
            store.set_bigram_count(ni, hao, hao_count).expect("set");
            store.set_bigram_count(ni, zhong, zhong_count).expect("set");
            for dynamic in [0, DYNAMIC_ADJUST] {
                assert!(pinyin_set_options(context, OPTIONS | dynamic));
                lists.push(predicted_texts(instance, c"你"));
            }
        }
        assert!(
            capture_rows(instance).iter().any(|(kind, text, _)| {
                *kind == lookup_candidate_type_t::PREDICTED_BIGRAM_CANDIDATE && text == "中"
            }),
            "the bigram rows are listed"
        );
        assert!(lists.iter().all(|list| *list == lists[0]), "{lists:?}");
        close(context, instance);
    }

    /// The predicted ranking uses the table.conf lambda: at λ = 1 every
    /// unigram term is zero, so a large count does not move its row ahead
    /// of the collection order; at the fixture's λ it does.
    #[test]
    fn predicted_rows_take_the_table_conf_lambda() {
        let order = |lambda: Option<&str>| {
            let scratch = tempfile::tempdir().expect("scratch");
            let system = crate::test_support::private_system(scratch.path());
            if let Some(lambda) = lambda {
                let path = system.join("table.conf");
                let text = std::fs::read_to_string(&path).expect("table.conf");
                std::fs::remove_file(&path).expect("unlink the linked table.conf");
                let text = text.replace(
                    "lambda parameter:0.312699",
                    &format!("lambda parameter:{lambda}"),
                );
                std::fs::write(&path, text).expect("write table.conf");
            }
            let user = TempUserDir::new("predict-lambda");
            let system_c = crate::test_support::cstr(system.to_str().expect("UTF-8 path"));
            let user_c = crate::test_support::cstr(user.path.to_str().expect("UTF-8 path"));
            let context = crate::context::pinyin_init(system_c.as_ptr(), user_c.as_ptr());
            assert!(!context.is_null());
            let instance = crate::instance::pinyin_alloc_instance(context);
            let before = predicted_texts(instance, c"你");
            assert_eq!(before.len(), 2);
            let last = lookup(instance, format!("你{}", before[1]))[0];
            assert!(crate::dict::pinyin_token_add_unigram_frequency(
                instance, last, 50_000_000
            ));
            let after = predicted_texts(instance, c"你");
            close(context, instance);
            (before, after)
        };
        let (before, after) = order(Some("1"));
        assert_eq!(after, before, "λ = 1 ranks by collection order");
        let (before, after) = order(None);
        assert_eq!(after, [before[1].clone(), before[0].clone()]);
    }

    /// Prefix rows alone never reach the zero-total assert, which gates the
    /// bigram rows only: on a facade total wrapped to zero the prediction
    /// answers true with its rows.
    #[test]
    fn prefix_rows_rank_on_a_zero_facade_total() {
        let user = TempUserDir::new("predict-prefix-zero-total");
        let (context, instance) = open(user.path.to_str().expect("UTF-8 path"));
        let before = predicted_texts(instance, c"你");
        // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
        let total = unsafe { crate::state::instance_ref(instance) }
            .core
            .lm
            .amplified_total();
        let total = u32::try_from(total).expect("a guint32 total");
        assert!(crate::dict::pinyin_token_add_unigram_frequency(
            instance,
            0x0100_0001,
            total.wrapping_neg()
        ));
        let mut after = predicted_texts(instance, c"你");
        after.sort();
        let mut expected = before;
        expected.sort();
        assert_eq!(after, expected);
        close(context, instance);
    }

    /// A system-library import ranks by its live item count, runtime
    /// additions included, against a user-library row.
    #[test]
    fn a_system_library_import_ranks_by_its_live_count() {
        let user = TempUserDir::new("predict-system-import");
        let (context, instance) = open(user.path.to_str().expect("UTF-8 path"));
        assert_eq!(
            import_batch(context, 1, &[("你甲甲", "ni3'jia3'jia3", 100_000)]),
            [true]
        );
        assert_eq!(
            import_batch(context, 7, &[("你乙乙", "ni3'yi3'yi3", 110_000)]),
            [true]
        );
        let system = lookup(instance, "你甲甲")[0];
        assert_eq!(system >> 24, 1);
        let rank = |instance| {
            predicted_texts(instance, c"你")
                .into_iter()
                .filter(|text| text == "甲甲" || text == "乙乙")
                .collect::<Vec<_>>()
        };
        assert_eq!(rank(instance), ["乙乙", "甲甲"]);
        assert!(crate::dict::pinyin_token_add_unigram_frequency(
            instance, system, 100_000
        ));
        assert_eq!(rank(instance), ["甲甲", "乙乙"]);
        close(context, instance);
    }
}
