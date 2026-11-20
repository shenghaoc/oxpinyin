//! Phrase prediction: prefixes → user-bigram successors → prefix suggestions,
//! then punctuation prepended from the Option A punct table.
//!
//! Reproduces `pinyin_guess_predicted_candidates` (`pinyin.cpp:2411-2451`)
//! and the punctuation prefix of
//! `pinyin_guess_predicted_candidates_with_punctuations` (`:2454-2498`).

use oxpinyin::marshal::phrase_text::phrase_text_bytes;
use rustc_hash::{FxHashMap, FxHashSet};
use std::cell::Cell;
use std::os::raw::c_char;

use oxpinyin::user::UserStore;
use oxpinyin::{PhraseText, PhraseToken, single_gram};

use crate::ffi::borrow_or_return;
use crate::state::{CapiCandidate, SharedDict, instance_mut};
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
    user: &UserStore,
    prefixes: &[PhraseToken],
) -> Result<Vec<(u32, u32)>, &'static str> {
    let mut merged = single_gram::new_gram();
    for prev in prefixes.iter().rev() {
        if let Ok(Some(row)) = user.bigram_row(*prev) {
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

/// Fill the candidate snapshot, returning false for a missing prefix.
///
/// A refused gram walk (row 98) returns a warning for the entry point to
/// emit after releasing the instance view.
fn guess_predicted_result(
    inst: &mut crate::state::CapiInstanceMut<'_>,
    prefix: Option<&PhraseText>,
) -> Result<bool, &'static str> {
    // Prediction frees the previous list first (074a2219 pinyin.cpp:2420).
    // Its rows carry the pin's default span, `m_begin = m_end = 0`
    // (`pinyin.cpp:114`).
    inst.state.candidates.clear();
    // `_compute_prefixes`' validating `reduce_tokens` (`pinyin.cpp:2424`,
    // `:1415`; `phrase_large_table3.h:95`) asserts on a tail substring with
    // more than four tokens, before the empty check.
    let Ok(prefixes) = oxpinyin::facade::compute_prefixes(&inst.core.dict, inst.core.user, prefix)
    else {
        std::process::abort();
    };
    // `m_prefixes` is emptied and refilled before the empty check
    // (`pinyin.cpp:2423-2426`).
    inst.core.session.set_candidate_prefixes(prefixes);
    let prefixes = inst.core.session.candidate_prefixes();
    let Some(prefix) = prefix.filter(|_| !prefixes.is_empty()) else {
        return Ok(false);
    };

    // `_compute_predicted_bigram_candidates` (`pinyin.cpp:2310-2341`). A
    // gram the pin reads past in its walk is refused here: one warning and
    // `false`, with the candidate list left empty in both APIs (the short
    // and the residual-total grams abort inside the walk).
    let successors = predicted_bigram_items(inst.core.user, prefixes)?;

    let mut items = Vec::new();
    append_predicted_bigrams(&inst.core.dict, &successors, &mut items);
    append_predicted_prefix(&inst.core.dict, inst.core.user, prefix, &mut items);
    score_predictions(
        &mut items,
        &inst.core.dict,
        inst.core.user,
        inst.core.lm.amplified_total(),
        inst.state.predicted_lambda,
    );
    sort_predicted(&mut items);

    // Dedup retains the numerically higher score even if the pin's signed
    // comparator put it later (`pinyin.cpp:2128-2136`). Equal scores keep
    // the first row. The winner remains at its original sorted position.
    let mut winners = FxHashMap::default();
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
            type_tag: Cell::new(item.candidate_type.tag()),
            nbest_index: 0,
            token: Cell::new(Some(item.token)),
            begin: 0,
            end: 0,
        });
    }
    Ok(true)
}

/// Stably rank predicted rows by phrase length and guarded wrapping frequency
/// (`pinyin.cpp:2440-2445`). Prediction has no pinyin-span key.
fn sort_predicted(items: &mut [Predicted]) {
    oxpinyin::engine::rank_by_frequency(items, |row| (row.phrase_length, 0, row.frequency));
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
        inst.core
            .session
            .set_candidate_prefixes(core::iter::empty());
    }
    borrowed
}

// pinyin.cpp:2411-2452.
/// Guess predicted candidates for a prefix (plain variant).
///
/// # C signature
/// ```c
/// bool pinyin_guess_predicted_candidates(pinyin_instance_t * instance,
///                                        const char * prefix);
/// ```
///
/// Replace the candidate list using phrase-table suffixes of the prefix. Returns false if no
/// suffix matches. A null instance or refused user-bigram lookup returns false. Supply a prefix
/// that remains valid when the old candidate list is cleared (`pinyin.cpp:2420-2426,2459`).
///
/// # Aborts
///
/// A suffix of the prefix that resolves to more than four tokens aborts (`phrase_large_table3.h:95`).
/// Scoring a predicted bigram candidate aborts if the live phrase-index total frequency is zero
/// (`pinyin.cpp:1855-1859`).
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle and `prefix` is a
/// string. The serial-calls rule applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_guess_predicted_candidates(
    instance: *mut PinyinInstance,
    prefix: *const c_char,
) -> bool {
    let mut inst = borrow_or_return!(instance, instance_mut, false);
    if clear_borrowed_prefix(&mut inst, prefix) {
        return false;
    }
    let prefix = oxpinyin::marshal::ucs4::prefix_text(
        // SAFETY: Strings guarantees valid, unchanged storage.
        // The borrow ends in this statement.
        unsafe { crate::ffi::borrow_c_string(prefix) },
    );
    let outcome = guess_predicted_result(&mut inst, prefix.as_ref());
    prediction_answer(outcome)
}

///
/// Upstream returns `true` after the prepend, even when the prefix matched
/// no phrase-table suffix. The scorer's zero-total assert aborts before
/// prepending, as the pin's does (register row 61).
pub(crate) fn guess_predicted_with_punctuations(
    inst: &mut crate::state::CapiInstanceMut<'_>,
    prefix: Option<&PhraseText>,
) -> Result<bool, &'static str> {
    guess_predicted_result(inst, prefix)?;
    // The pin walks `m_prefixes` as the prediction left it (`pinyin.cpp:2463`).
    prepend_punctuations(inst);
    Ok(true)
}

/// Finish prediction after the instance view's last use.
pub(crate) fn prediction_answer(outcome: Result<bool, &'static str>) -> bool {
    match outcome {
        Ok(answer) => answer,
        Err(warning) => {
            crate::ffi::log_warning(warning);
            false
        }
    }
}

/// Prepends the punctuations of each prefix token, in prefix order then
/// table order, keeping the first of equal strings (`pinyin.cpp:2468-2494`
/// at 074a2219). The pin's `g_strv_contains` compares C strings with
/// `g_str_equal`; `phrase_text_bytes` is that string, so equal bytes are
/// the test. Existing candidates take no part, and the reverse
/// `g_array_prepend_val` leaves the punctuations first in this order.
fn prepend_punctuations(inst: &mut crate::state::CapiInstanceMut<'_>) {
    let mut puncts: Vec<Vec<u8>> = Vec::new();
    let mut seen: FxHashSet<Vec<u8>> = FxHashSet::default();
    for token in inst.core.session.candidate_prefixes() {
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
            type_tag: Cell::new(lookup_candidate_type_t::PREDICTED_PUNCTUATION_CANDIDATE.tag()),
            nbest_index: 0,
            token: Cell::new(None),
            begin: 0,
            end: 0,
        });
    }
    inst.state.candidates.append(rest);
}

/// Appends readable one- and two-character successors in gram order.
fn append_predicted_bigrams(
    dict: &SharedDict,
    successors: &[(u32, u32)],
    into: &mut Vec<Predicted>,
) {
    for &(token, count) in successors {
        if count < BIGRAM_FILTER || !dict.phrase_table_token_enabled(token) {
            continue;
        }
        let Some(text) = dict.token_text(token) else {
            continue;
        };
        let phrase_length = text.phrase_length();
        if !(1..=2).contains(&phrase_length) {
            continue;
        }
        into.push(Predicted {
            phrase_length,
            frequency: 0,
            text,
            token,
            candidate_type: lookup_candidate_type_t::PREDICTED_BIGRAM_CANDIDATE,
        });
    }
}

fn append_predicted_prefix(
    dict: &SharedDict,
    user: &UserStore,
    prefix: &PhraseText,
    into: &mut Vec<Predicted>,
) {
    // `m_prefix_len`, the converted word count (`pinyin.cpp:2395`, `:2401`).
    let prefix_len = prefix.outward_words().count();
    if prefix_len == 0 {
        return;
    }
    let limit = prefix_len.saturating_mul(2).saturating_add(1);
    // The facade merge reproduces reduce_tokens: library groups first,
    // then each DBM's UCS-4 byte cursor order. The stable length/score sort
    // preserves this collection order on ties, including user/system ties.
    for (token, text) in oxpinyin::facade::merged_suggestions(dict, user, prefix) {
        let phrase_length = text.phrase_length();
        if phrase_length > limit {
            continue;
        }
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
        into.push(Predicted {
            frequency: 0,
            text: display,
            phrase_length: display_length,
            token,
            candidate_type: lookup_candidate_type_t::PREDICTED_PREFIX_CANDIDATE,
        });
    }
}

/// Scores after both lookups. Only bigram rows reach the ranking assertion
/// (`pinyin.cpp:1812–1825, 1855–1859, 2432–2438`). The caller passes
/// `null_token` as the predecessor, leaving the bigram term at zero.
fn score_predictions(
    items: &mut [Predicted],
    dict: &SharedDict,
    user: &UserStore,
    total: u64,
    lambda: f32,
) {
    for item in items {
        let unigram = dict
            .live_items()
            .effective_unigram(item.token)
            .unwrap_or_else(|| user.unigram_delta(item.token));
        if total == 0 && item.candidate_type == lookup_candidate_type_t::PREDICTED_BIGRAM_CANDIDATE
        {
            std::process::abort();
        }
        item.frequency = oxpinyin::engine::amplified_frequency(unigram, total, lambda);
    }
}

#[cfg(test)]
mod tests;
