//! Phrase prediction: prefixes → user-bigram successors → prefix suggestions,
//! then punctuation prepended from the Option A `punct.redb`.
//!
//! Reproduces `pinyin_guess_predicted_candidates` (`pinyin.cpp:2411-2451`)
//! and the punctuation prefix of
//! `pinyin_guess_predicted_candidates_with_punctuations` (`:2454-2498`).

use std::collections::HashSet;
use std::ffi::CString;
use std::os::raw::c_char;

use oxpinyin_engine::CandidateKind;
use oxpinyin_user::UserStore;

use crate::ffi::{cstr_to_strict, ffi_catch};
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
pub(crate) fn guess_predicted(inst: &mut CapiInstance, prefix: &str) -> bool {
    inst.candidates.clear();
    let prefixes = compute_prefixes(&inst.dict, inst.user.as_ref(), prefix);
    if prefixes.is_empty() {
        return false;
    }

    let mut items = Vec::new();
    append_predicted_bigrams(&inst.dict, inst.user.as_ref(), &prefixes, &mut items);
    append_predicted_prefix(&inst.dict, &inst.lm, inst.user.as_ref(), prefix, &mut items);

    items.sort_by(|left, right| {
        right
            .text
            .chars()
            .count()
            .cmp(&left.text.chars().count())
            .then(right.frequency.cmp(&left.frequency))
    });

    let mut seen = std::collections::HashSet::new();
    for item in items {
        if !seen.insert(item.text.clone()) {
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
    true
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
    ffi_catch(false, || {
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
    })
}

///
/// Upstream always returns `true` after the prepend, even when the prefix
/// matched no phrase-table suffix.
pub(crate) fn guess_predicted_with_punctuations(inst: &mut CapiInstance, prefix: &str) -> bool {
    let prefixes = compute_prefixes(&inst.dict, inst.user.as_ref(), prefix);
    let _ = guess_predicted(inst, prefix);
    prepend_punctuations(inst, &prefixes);
    true
}

fn prepend_punctuations(inst: &mut CapiInstance, prefixes: &[u32]) {
    let mut puncts = Vec::new();
    let mut seen = HashSet::new();
    for token in prefixes {
        for punct in inst.dict.punctuations(*token) {
            if seen.insert(punct.as_str()) {
                puncts.push(punct.clone());
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

pub(crate) fn compute_prefixes(
    dict: &SharedDict,
    user: Option<&UserStore>,
    prefix: &str,
) -> Vec<u32> {
    let chars: Vec<char> = prefix.chars().collect();
    if chars.is_empty() {
        return Vec::new();
    }
    let user_lookup = user.and_then(|store| oxpinyin_user::UserLookup::from_store(store).ok());
    let max = chars.len().min(oxpinyin_user::MAX_PHRASE_LENGTH);
    let mut tokens = Vec::new();
    for length in 1..=max {
        let suffix: String = chars[chars.len() - length..].iter().collect();
        // System tokens ride the loaded-library mask: an unloaded library
        // must not contribute prefix or successor tokens (upstream's
        // `_get_phrase_item_from_token` refuses them at the item lookup;
        // filtering here is the closest we get to that gate on the
        // prefix path).
        tokens.extend(
            dict.system()
                .tokens_for_text(&suffix)
                .iter()
                .copied()
                .filter(|token| dict.library_visible_token(*token)),
        );
        if let Some(lookup) = user_lookup.as_ref() {
            tokens.extend(lookup.tokens_for_text(&suffix).iter().copied());
        }
    }
    tokens
}

fn append_predicted_bigrams(
    dict: &SharedDict,
    user: Option<&UserStore>,
    prefixes: &[u32],
    into: &mut Vec<Predicted>,
) {
    let Some(store) = user else {
        return;
    };
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
                frequency: *count,
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
    user: Option<&UserStore>,
    prefix: &str,
    into: &mut Vec<Predicted>,
) {
    let prefix_len = prefix.chars().count();
    if prefix_len == 0 {
        return;
    }
    let limit = prefix_len.saturating_mul(2).saturating_add(1);
    // Merge the system and user seams by TEXT before ranking: both
    // `suggest_after` results are already text-ascending (the reverse-map
    // walk, token-ascending within one text), and the defined prediction
    // order is text-ascending across BOTH seams — a plain concatenation
    // would put every system row before a user row regardless of text. That
    // is only text-safe today because the two seams settle in different
    // (length, frequency) tie groups (system baked > 0, user rows always
    // baked 0 — user tokens never appear in the system unigram map); merge
    // instead so a future cross-seam tie group stays text-ascending, with
    // the system row first when a text is shared. The stable sort below
    // keeps both inside their (length, frequency) tie groups
    // (`upstream-divergences.md`, "Predicted-candidate tie order").
    let system: Vec<(u32, String)> = dict
        .system()
        .suggest_after(prefix)
        .into_iter()
        .filter(|(token, _)| dict.library_visible_token(*token))
        .collect();
    let user_rows = if let Some(store) = user
        && let Ok(lookup) = oxpinyin_user::UserLookup::from_store(store)
    {
        lookup.suggest_after(prefix)
    } else {
        Vec::new()
    };
    let suggestions = merge_suggestions(system, user_rows);
    // The pin divides by the phrase-index total, live per call
    // (`pinyin.cpp:1813-1814`); `amplified_total` is the same construction
    // the pinned normal path uses (`session.rs:1409-1416`). The item count
    // leg is `SystemDictionary`'s `phrase_index_item_count`
    // (`dict.rs:302-304`).
    // The visible item count (Tier C's library mask can shrink it) plus
    // the add_unigram_frequency overlay total — upstream's facade
    // total_freq shifts by exactly these (`phrase_index.h:632`,
    // `phrase_index.cpp:264`).
    let total = lm
        .amplified_total(dict.visible_item_count())
        .saturating_add(dict.unigram_total_delta());
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
        // The live item count: the baked count plus whatever
        // `pinyin_token_add_unigram_frequency` overlaid (upstream's
        // `_compute_frequency_of_items` reads the item through
        // `get_phrase_item`, which sees the add's write,
        // `phrase_index.h:632`).
        let baked = dict.system().unigram_count(token).unwrap_or(0)
            + dict.unigram_delta(token).unwrap_or(0);
        into.push(Predicted {
            frequency: amplified_frequency(baked, total),
            text: display,
            token,
            candidate_type: lookup_candidate_type_t::PREDICTED_PREFIX_CANDIDATE,
        });
    }
}

/// The pinned interpolation λ (`PIN_LAMBDA_F32` in `session.rs`).
const PIN_LAMBDA_F32: f32 = 0.312_699;

/// The pin's candidate `m_freq` for predicted rows: the unigram possibility
/// `(1−λ)·unigram/total` computed and amplified by 2²⁴ in C `float`
/// arithmetic, then truncated like the `guint32` assignment
/// (`pinyin.cpp:1811-1824`, the PREDICTED_PREFIX branch).
///
/// Mirrors `amplified_frequency` (`session.rs:1835`) exactly — the engine's
/// copy is private and widening it would change the crate's public surface,
/// so this copy is bound to the pinned law by
/// [`tests::amplified_law_mirrors_the_session_pinning_values`] asserting the
/// same probe values `amplified_frequency_pins_the_class_a_probe_values`
/// pins there.
fn amplified_frequency(unigram: u64, total: u64) -> u64 {
    if total == 0 {
        return 0;
    }
    let possibility = (1.0_f32 - PIN_LAMBDA_F32) * unigram as f32 / total as f32;
    u64::from((possibility * 256.0 * 256.0 * 256.0) as u32)
}

fn phrase_text(dict: &SharedDict, store: &UserStore, token: u32) -> Option<String> {
    if let Ok(Some(phrase)) = store.phrase(token) {
        return Some(phrase.text().to_owned());
    }
    dict.system().phrase_text(token).ok().flatten()
}

/// Merges the system and user `suggest_after` results by TEXT, preserving
/// the global text-ascending order across both seams, with the system row
/// first when the two seams share a text.
///
/// Both inputs are already text-ascending (the reverse-map walk); the
/// merge is the standard two-way merge of two ascending-by-text sequences,
/// stable so equal-text system rows keep their (token) order and precede the
/// user row. This is what makes the defined prediction order hold across the
/// system/user boundary rather than only within each seam
/// (`upstream-divergences.md`, "Predicted-candidate tie order").
fn merge_suggestions(
    system: Vec<(u32, String)>,
    user_rows: Vec<(u32, String)>,
) -> Vec<(u32, String)> {
    let mut merged = Vec::with_capacity(system.len() + user_rows.len());
    let (mut system_index, mut user_index) = (0, 0);
    while system_index < system.len() && user_index < user_rows.len() {
        if system[system_index].1 <= user_rows[user_index].1 {
            merged.push(system[system_index].clone());
            system_index += 1;
        } else {
            merged.push(user_rows[user_index].clone());
            user_index += 1;
        }
    }
    merged.extend(system[system_index..].iter().cloned());
    merged.extend(user_rows[user_index..].iter().cloned());
    merged
}

#[cfg(test)]
mod tests {
    use super::{amplified_frequency, merge_suggestions};

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
    #[test]
    fn merge_suggestions_interleaves_across_seams_and_keeps_system_first_on_ties() {
        // The defined order is text-ascending across BOTH seams. A plain
        // concatenation would put every system row before every user row,
        // which only survives a (length, frequency) tie group because the
        // two seams' frequencies never collide today. This exercises the
        // merge directly: texts from both seams interleave, and a shared
        // text keeps the system row first.
        let system = vec![
            (10, "中华".to_owned()), // zhonghua (华)
            (20, "中年".to_owned()), // zhongnian (年)
        ];
        let user_rows = vec![
            (901, "中号".to_owned()), // zhonghao (号) — sorts between the two
            (902, "中年".to_owned()), // same text as the system row 20
        ];
        let merged = merge_suggestions(system, user_rows);
        let texts: Vec<&str> = merged.iter().map(|(_, text)| text.as_str()).collect();
        assert_eq!(
            texts,
            vec![
                "中华", // 华
                "中号", // 号 (user) interleaves between the two system rows
                "中年", // 年 — the system row (token 20) first,
                "中年", // then the user row (token 902) on the same text,
            ],
            "global text-ascending across both seams"
        );
        // System-first on the shared text: the lower system token precedes the
        // user token on that identical text.
        assert_eq!(merged[2], (20, "中年".to_owned()));
        assert_eq!(merged[3], (902, "中年".to_owned()));
    }
}
