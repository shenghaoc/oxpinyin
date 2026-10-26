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
const BIGRAM_FILTER: u32 = 10;

/// One predicted item before sort/dedup.
struct Predicted {
    text: PhraseText,
    phrase_length: usize,
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

/// `_compute_predicted_bigram_candidates`'s gram walk (`pinyin.cpp:2321-2341`
/// at the pin `074a2219`): for the prefixes from last to first, `load` the
/// prefix's user gram and `merge_single_gram(merged, NULL, user)` — a copy
/// of it, or, when the prefix has no gram, the previous copy kept — and
/// stop at the first whose `get_length` is not zero; then `retrieve_all`.
/// Answers the items in stored order, duplicates included, or the one
/// warning a call refuses with where the pin dies: a value too short for
/// its total (`get_length` reads it, `memory_chunk.h:390`, register row
/// 85), a gram with no whole item and a total left over (`ngram.cpp:70`,
/// row 88), or a partial item tail after a whole item, which `retrieve_all`
/// reads past (`ngram.cpp:143`, row 98). Only the prefixes the walk loads
/// are read; a fault under any other token is never reached.
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
            None => {
                return Err(
                    "pinyin_guess_predicted_candidates: a user bigram value is shorter than a \
                     guint32 total_freq (upstream aborts, memory_chunk.h:390)",
                );
            }
            Some(0) => {}
            Some(_) => {
                return Err(
                    "pinyin_guess_predicted_candidates: assertion '0 == total_freq' failed",
                );
            }
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

    // `_compute_predicted_bigram_candidates` (`pinyin.cpp:2310-2341`). A
    // gram the pin dies on in its walk is refused here: one warning and
    // `false`, with the candidate list left empty in both APIs.
    let successors = match predicted_bigram_items(inst.core.user.as_ref(), &prefixes) {
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
        inst.predicted_lambda,
        inst.core.user.as_ref(),
        &successors,
        &mut items,
    );
    // pinyin.cpp:1859 asserts only for the general scorer branch, so
    // prefix-only rows do not reach it. Class (c), policy row 61: refuse
    // with one warning and leave the candidate list empty in both APIs.
    if !items.is_empty() && predicted_total(&inst.core.lm) == 0 {
        crate::ffi::log_warning(
            "pinyin_guess_predicted_candidates: assertion '0 < total_freq' failed",
        );
        return Err(());
    }
    let missing = append_predicted_prefix(
        &inst.core.dict,
        &inst.core.lm,
        inst.predicted_lambda,
        inst.core.user.as_ref(),
        prefix,
        &mut items,
    );

    if let Some(token) = missing {
        // Only one pre-dedup row: neither strcmp runs. Its opaque score and
        // length are unobservable; preserve the NULL getter and choose token.
        inst.candidates.push(CapiCandidate {
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
        let Some(text) = inst.candidates.intern(&bytes) else {
            continue;
        };
        inst.candidates.push(CapiCandidate {
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
            source_index: u32::try_from(inst.candidates.len()).unwrap_or(u32::MAX),
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
                let left_len = left.phrase_length;
                let right_len = right.phrase_length;
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
    // At 074a2219, `_free_candidates` frees this borrowed string before
    // `_compute_prefixes` reads it (pinyin.cpp:1340, :2420-2427). #542's
    // ruling preserves its stable false by identity, without reading freed
    // storage. The pin still empties the candidates and prediction prefixes.
    if inst
        .candidates
        .iter()
        .any(|candidate| std::ptr::eq(candidate.text_ptr, prefix))
    {
        inst.candidates.clear();
        inst.prefixes.clear();
        return false;
    }
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
    if guess_predicted_result(inst, prefix).is_err() {
        return false;
    }
    // The pin walks `m_prefixes` as the prediction left it (`pinyin.cpp:2463`).
    let prefixes = inst.prefixes.clone();
    prepend_punctuations(inst, &prefixes);
    true
}

/// Prepends the punctuations of each prefix token, in prefix order then
/// table order, keeping the first of equal strings (`pinyin.cpp:2468-2494`
/// at 074a2219). The pin's `g_strv_contains` compares C strings with
/// `g_str_equal`; `phrase_text_bytes` is that string, so equal bytes are
/// the test. Existing candidates take no part, and the reverse
/// `g_array_prepend_val` leaves the punctuations first in this order.
fn prepend_punctuations(inst: &mut CapiInstance, prefixes: &[u32]) {
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
    let rest = inst.candidates.take();
    for text in puncts {
        let Some(text) = inst.candidates.intern(&text) else {
            continue;
        };
        inst.candidates.push(CapiCandidate {
            text,
            text_ptr: std::ptr::null(),
            kind: CandidateKind::Phrase,
            type_tag: lookup_candidate_type_t::PREDICTED_PUNCTUATION_CANDIDATE.tag(),
            nbest_index: 0,
            token: None,
            source_index: u32::try_from(inst.candidates.len()).unwrap_or(u32::MAX),
        });
    }
    inst.candidates.append(rest);
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
    prefix: &str,
    into: &mut Vec<Predicted>,
) -> Option<u32> {
    let prefix_len = prefix.chars().count();
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
/// and system ABI increments share store rows; remaining transient increments
/// live in the dictionary overlay.
fn predicted_frequency(
    dict: &SharedDict,
    user: Option<&UserStore>,
    token: u32,
    total: u64,
    lambda: f32,
) -> u64 {
    let store_count = || {
        user.and_then(|store| store.unigram_delta(token).ok())
            .unwrap_or(0)
    };
    // An imported system-library token missing from the immutable dictionary
    // has a complete live count in system_item_override. Its UNIGRAM value
    // must not be added twice. Existing immutable items need base + delta.
    let item_count = match dict.system().unigram_count(token) {
        Some(base) => base.wrapping_add(store_count()),
        None => dict.system_unigram_count(token).unwrap_or_else(store_count),
    };
    let unigram =
        item_count.wrapping_add(dict.unigram_delta(token).unwrap_or(0)) & u64::from(u32::MAX);
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

pub(crate) fn phrase_text(dict: &SharedDict, store: &UserStore, token: u32) -> Option<PhraseText> {
    if let Ok(Some(phrase)) = store.phrase(token) {
        return phrase.phrase_text();
    }
    dict.system().phrase_text(token)
}

#[cfg(test)]
mod tests {
    use super::select_prefix_rows;
    use oxpinyin_core::PhraseText;

    fn amplified_frequency(unigram: u64, total: u64) -> u64 {
        super::amplified_frequency(unigram, total, 0.312_699)
    }

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
