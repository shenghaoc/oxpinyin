//! Phrase prediction: prefixes → user-bigram successors → prefix suggestions,
//! then punctuation prepended from the Option A punct table.
//!
//! Reproduces `pinyin_guess_predicted_candidates` (`pinyin.cpp:2411-2451`)
//! and the punctuation prefix of
//! `pinyin_guess_predicted_candidates_with_punctuations` (`:2454-2498`).

use std::collections::HashSet;
use std::ffi::CString;
use std::os::raw::c_char;

use oxpinyin_engine::CandidateKind;
use oxpinyin_user::UserStore;

use crate::ffi::cstr_to_strict;
use crate::state::{CapiCandidate, CapiInstance, SharedDict, SharedLm, instance_mut};
use crate::types::{PinyinInstance, lookup_candidate_type_t};

/// Minimum user-bigram count for a predicted successor.
///
/// Copied from `_compute_predicted_bigram_candidates`:
/// `const guint32 filter = 10` (`pinyin.cpp:2311`) and
/// `if (phrase_item->m_count < filter) continue` (`pinyin.cpp:2349-2350`).
/// Not a design choice — public `pinyin_train` first-seeds 69, so the
/// 9-vs-10 edge is planted in `run-union-diff.sh`, not trained.
const BIGRAM_FILTER: u64 = 10;

/// One predicted item before sort/dedup.
struct Predicted {
    text: String,
    token: u32,
    candidate_type: lookup_candidate_type_t,
    frequency: u64,
}

/// Fills `inst.candidates` with predicted phrases for `prefix`.
///
/// Returns `false` when the prefix matches no phrase-table suffix (upstream
/// returns false when `m_prefixes` stays empty).
pub fn guess_predicted(inst: &mut CapiInstance, prefix: &str) -> bool {
    guess_predicted_result(inst, prefix).unwrap_or(false)
}

// Err marks the scorer's assert site, distinct from a missing prefix. The
// punctuation wrapper ignores a missing prefix but must refuse an assert.
fn guess_predicted_result(inst: &mut CapiInstance, prefix: &str) -> Result<bool, ()> {
    inst.candidates.clear();
    let prefixes =
        oxpinyin_facade::compute_prefixes(&inst.core.dict, inst.core.user.as_ref(), prefix);
    // `m_prefixes` is emptied and refilled before the empty check
    // (`pinyin.cpp:2423-2426`).
    inst.prefixes.clone_from(&prefixes);
    if prefixes.is_empty() {
        return Ok(false);
    }

    let mut items = Vec::new();
    append_predicted_bigrams(
        &inst.core.dict,
        &inst.core.lm,
        inst.predicted_lambda,
        inst.core.user.as_ref(),
        &prefixes,
        &mut items,
    );
    // pinyin.cpp:1859 asserts only for the general scorer branch, so
    // prefix-only rows do not reach it. Class (c), policy row 61: refuse
    // with one warning and leave the candidate list empty in both APIs.
    if !items.is_empty() && predicted_total(&inst.core.dict, &inst.core.lm) == 0 {
        crate::ffi::log_warning(
            "pinyin_guess_predicted_candidates: assertion '0 < total_freq' failed",
        );
        return Err(());
    }
    append_predicted_prefix(
        &inst.core.dict,
        &inst.core.lm,
        inst.predicted_lambda,
        inst.core.user.as_ref(),
        prefix,
        &mut items,
    );

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
        let Ok(text) = CString::new(item.text) else {
            continue;
        };
        inst.candidates.push(CapiCandidate {
            text,
            kind: CandidateKind::Phrase,
            candidate_type: item.candidate_type,
            nbest_index: 0,
            consumed_bytes: 0,
            token: Some(oxpinyin_core::PhraseToken::new(item.token)),
            // Predicted candidates are chosen via
            // `pinyin_choose_predicted_candidate`, never the anchored-select
            // path, so the window index is unused here; record the snapshot
            // position for determinism.
            source_index: inst.candidates.len(),
        });
    }
    Ok(true)
}

/// Reproduce GLib's stable half-split merge order, including comparisons
/// whose guint32 difference converts to a negative gint. A wrapping score
/// comparator need not be transitive, so Rust's total-order sort is unsuitable.
/// GLib 2.90.0 glib/gqsort.c:64-183 halves n/2 and takes the left item on <= 0.
/// Sort indices and permute in place: O(n log n) time and two usize arrays.
fn sort_predicted(items: &mut [Predicted]) {
    fn merge(items: &[Predicted], order: &mut [usize], scratch: &mut [usize]) {
        if order.len() <= 1 {
            return;
        }
        let middle = order.len() / 2;
        let (left, right) = order.split_at_mut(middle);
        let (left_scratch, right_scratch) = scratch.split_at_mut(middle);
        merge(items, left, left_scratch);
        merge(items, right, right_scratch);
        let (mut lhs, mut rhs) = (0, middle);
        for target in scratch.iter_mut() {
            let take_left = if rhs == order.len() {
                true
            } else if lhs == middle {
                false
            } else {
                let left = &items[order[lhs]];
                let right = &items[order[rhs]];
                let left_len = left.text.chars().count();
                let right_len = right.text.chars().count();
                if left_len != right_len {
                    left_len > right_len
                } else {
                    // pinyin.cpp:1701-1705: -(freq_lhs - freq_rhs),
                    // guint32 arithmetic followed by the gint conversion.
                    (right.frequency as u32).wrapping_sub(left.frequency as u32) as i32 <= 0
                }
            };
            if take_left {
                *target = order[lhs];
                lhs += 1;
            } else {
                *target = order[rhs];
                rhs += 1;
            }
        }
        order.copy_from_slice(scratch);
    }

    let mut order: Vec<usize> = (0..items.len()).collect();
    let mut scratch = vec![0; items.len()];
    merge(items, &mut order, &mut scratch);
    // Reuse scratch for old-index -> new-index destinations. Cycle swaps
    // move Strings without cloning them or using unchecked accesses.
    for (new, old) in order.into_iter().enumerate() {
        scratch[old] = new;
    }
    for index in 0..items.len() {
        while scratch[index] != index {
            let target = scratch[index];
            items.swap(index, target);
            scratch.swap(index, target);
        }
    }
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
    let inst = unsafe { instance_mut(instance) };
    // Reject invalid UTF-8 without touching `inst.candidates` —
    // mirrors upstream's `g_return_val_if_fail(prefix, FALSE)` at
    // `pinyin.cpp:1450-1452` (see `ffi::cstr_to_strict`).
    let Some(prefix) = cstr_to_strict(prefix) else {
        return false;
    };
    guess_predicted(inst, &prefix)
}

///
/// Upstream returns `true` after the prepend, even when the prefix matched
/// no phrase-table suffix. The scorer's zero-total assert is refused before
/// prepending, with a warning and `false` under policy class (c).
pub fn guess_predicted_with_punctuations(inst: &mut CapiInstance, prefix: &str) -> bool {
    let prefixes =
        oxpinyin_facade::compute_prefixes(&inst.core.dict, inst.core.user.as_ref(), prefix);
    if guess_predicted_result(inst, prefix).is_err() {
        return false;
    }
    prepend_punctuations(inst, &prefixes);
    true
}

fn prepend_punctuations(inst: &mut CapiInstance, prefixes: &[u32]) {
    let mut puncts: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for token in prefixes {
        for punct in inst.core.dict.punctuations(*token) {
            if seen.insert(punct.clone()) {
                puncts.push(punct);
            }
        }
    }
    if puncts.is_empty() {
        return;
    }
    let rest = std::mem::take(&mut inst.candidates);
    for text in puncts {
        let Ok(text) = CString::new(text) else {
            continue;
        };
        inst.candidates.push(CapiCandidate {
            text,
            kind: CandidateKind::Phrase,
            candidate_type: lookup_candidate_type_t::PREDICTED_PUNCTUATION_CANDIDATE,
            nbest_index: 0,
            consumed_bytes: 0,
            token: None,
            source_index: inst.candidates.len(),
        });
    }
    inst.candidates.extend(rest);
}

fn append_predicted_bigrams(
    dict: &SharedDict,
    lm: &SharedLm,
    lambda: f32,
    user: Option<&UserStore>,
    prefixes: &[u32],
    into: &mut Vec<Predicted>,
) {
    let Some(store) = user else {
        return;
    };
    let total = predicted_total(dict, lm);
    let mut successors = Vec::new();
    for prev in prefixes.iter().rev().copied() {
        let Ok(rows) = store.bigram_successors(prev) else {
            continue;
        };
        if rows.is_empty() {
            continue;
        }
        successors = rows;
        break;
    }
    for length in [2_usize, 1] {
        for (token, count) in &successors {
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
            if text.chars().count() != length {
                continue;
            }
            into.push(Predicted {
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

fn append_predicted_prefix(
    dict: &SharedDict,
    lm: &SharedLm,
    lambda: f32,
    user: Option<&UserStore>,
    prefix: &str,
    into: &mut Vec<Predicted>,
) {
    let prefix_len = prefix.chars().count();
    if prefix_len == 0 {
        return;
    }
    let limit = prefix_len.saturating_mul(2).saturating_add(1);
    // The facade merge reproduces reduce_tokens: library groups first,
    // then each DBM's UCS-4 byte cursor order. The stable length/score sort
    // preserves this collection order on ties, including user/system ties.
    let suggestions = oxpinyin_facade::merged_suggestions(dict, user, prefix);
    let total = predicted_total(dict, lm);
    for (token, text) in suggestions {
        // The length gate stays on the FULL phrase: the pin checks
        // `get_phrase_length()` against `prefix_len * 2 + 1` before any
        // slicing (`pinyin.cpp:2392-2395`).
        if text.chars().count() > limit {
            continue;
        }
        // The prefix subtraction the pin applies twice (`pinyin.cpp:1976-1980`):
        // the display string is sliced from `m_begin` (`:2018-2023`) and the
        // phrase-length sort key subtracts it. Storing the sliced text here
        // drives both — the sort counts `text.chars()`, the dedup and the
        // emitted candidate reuse the same string, matching upstream's
        // `_remove_duplicated_items_by_phrase_string` on the final string.
        let display: String = text.chars().skip(prefix_len).collect();
        // The sort key is the amplified law, not the raw count: the pin's
        // PREDICTED_PREFIX branch computes `(1−λ)·unigram/total·2²⁴`
        // truncated (`pinyin.cpp:1811-1824`), the same law the normal
        // candidate path pins.
        into.push(Predicted {
            frequency: predicted_frequency(dict, user, token, total, lambda),
            text: display,
            token,
            candidate_type: lookup_candidate_type_t::PREDICTED_PREFIX_CANDIDATE,
        });
    }
}

/// The live facade total is guint32 and wraps, including both training
/// deltas and ABI add overlays (`phrase_index.h:633`, `pinyin.cpp:1814-1815`).
fn predicted_total(dict: &SharedDict, lm: &SharedLm) -> u64 {
    lm.amplified_total()
        .wrapping_add(u64::from(dict.unigram_total_delta()))
        & u64::from(u32::MAX)
}

/// Both predicted row kinds read the live item counter. User dictionary
/// imports and training live in the store; ABI unigram adds live in the dict.
fn predicted_frequency(
    dict: &SharedDict,
    user: Option<&UserStore>,
    token: u32,
    total: u64,
    lambda: f32,
) -> u64 {
    let unigram = dict
        .system_unigram_count(token)
        .unwrap_or(0)
        .wrapping_add(
            user.and_then(|store| store.unigram_delta(token).ok())
                .unwrap_or(0),
        )
        .wrapping_add(dict.unigram_delta(token).unwrap_or(0))
        & u64::from(u32::MAX);
    amplified_frequency(unigram, total, lambda)
}

/// The pin's candidate `m_freq` for predicted rows: the unigram possibility
/// `(1−λ)·unigram/total` computed and amplified by 2²⁴ in C `float`
/// arithmetic, then truncated like the `guint32` assignment
/// (`pinyin.cpp:1811-1824`, the `PREDICTED_PREFIX` branch).
///
/// Matches the engine's ordinary in-range probe values. Its copy is private
/// and widening it would change the crate's public surface, so this copy is
/// bound to those values by
/// [`tests::amplified_law_mirrors_the_session_pinning_values`] asserting the
/// same probe values `amplified_frequency_pins_the_class_a_probe_values`
/// pins there.
fn amplified_frequency(unigram: u64, total: u64, lambda: f32) -> u64 {
    if total == 0 {
        return 0;
    }
    let possibility = (1.0_f32 - lambda) * unigram as f32 / total as f32;
    let score = possibility * 256.0 * 256.0 * 256.0;
    // The pinned x86-64 scorer converts with cvttss2si to a 64-bit integer
    // then stores eax (pinyin.cpp:1821-1824, 1862-1866). Preserve its low
    // 32 bits for finite scores beyond guint32 instead of Rust saturation.
    // An invalid 64-bit conversion yields INT64_MIN, whose low bits are 0.
    if !score.is_finite() || score >= i64::MAX as f32 || score < i64::MIN as f32 {
        return 0;
    }
    u64::from(score as i64 as u32)
}

pub(crate) fn phrase_text(dict: &SharedDict, store: &UserStore, token: u32) -> Option<String> {
    if let Ok(Some(phrase)) = store.phrase(token) {
        return Some(phrase.text().to_owned());
    }
    dict.system().phrase_text(token)
}

#[cfg(test)]
mod tests {
    fn amplified_frequency(unigram: u64, total: u64) -> u64 {
        super::amplified_frequency(unigram, total, 0.312_699)
    }

    #[test]
    fn amplified_law_mirrors_the_session_pinning_values() {
        // The same probe values `amplified_frequency_pins_the_class_a_probe_values`
        // pins for the engine's private copy (`session.rs`): binds this
        // mirror to the pinned law so the two cannot drift.
        const PIN_TOTAL: u64 = 51_051_831;
        assert_eq!(amplified_frequency(1, PIN_TOTAL), 0);
        assert_eq!(amplified_frequency(3, PIN_TOTAL), 0);
        assert_eq!(amplified_frequency(14, PIN_TOTAL), 3);
        assert_eq!(amplified_frequency(16, PIN_TOTAL), 3);
        assert_eq!(amplified_frequency(18, PIN_TOTAL), 4);
        assert_eq!(amplified_frequency(20, PIN_TOTAL), 4);
        assert_eq!(amplified_frequency(21, PIN_TOTAL), 4);
        assert_eq!(amplified_frequency(77, PIN_TOTAL), 17);
        assert_eq!(amplified_frequency(78, PIN_TOTAL), 17);
        assert_eq!(amplified_frequency(87, PIN_TOTAL), 19);
        assert_eq!(amplified_frequency(0, PIN_TOTAL), 0);
    }

    #[test]
    fn amplified_law_zero_total_is_zero() {
        assert_eq!(amplified_frequency(100, 0), 0);
    }
}
