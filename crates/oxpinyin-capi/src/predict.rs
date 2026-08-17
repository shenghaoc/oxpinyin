//! Phrase prediction: prefixes → user-bigram successors → prefix suggestions,
//! then punctuation prepended from the Option A `punct.redb`.
//!
//! Reproduces `pinyin_guess_predicted_candidates` (`pinyin.cpp:2411-2451`)
//! and the punctuation prefix of
//! `pinyin_guess_predicted_candidates_with_punctuations` (`:2454-2498`).

use std::collections::HashSet;
use std::ffi::CString;

use oxpinyin_engine::CandidateKind;
use oxpinyin_user::UserStore;

use crate::state::{CapiCandidate, CapiInstance, SharedDict};
use crate::types::lookup_candidate_type_t;

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
    append_predicted_prefix(&inst.dict, inst.user.as_ref(), prefix, &mut items);

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
        });
    }
    true
}

/// Phrase prediction plus the punctuation prefix (`pinyin.cpp:2454-2498`).
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
        });
    }
    inst.candidates.extend(rest);
}

fn compute_prefixes(dict: &SharedDict, user: Option<&UserStore>, prefix: &str) -> Vec<u32> {
    let chars: Vec<char> = prefix.chars().collect();
    if chars.is_empty() {
        return Vec::new();
    }
    let user_lookup = user.and_then(|store| oxpinyin_user::UserLookup::from_store(store).ok());
    let max = chars.len().min(oxpinyin_user::MAX_PHRASE_LENGTH);
    let mut tokens = Vec::new();
    for length in 1..=max {
        let suffix: String = chars[chars.len() - length..].iter().collect();
        tokens.extend(dict.system().tokens_for_text(&suffix).iter().copied());
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
    user: Option<&UserStore>,
    prefix: &str,
    into: &mut Vec<Predicted>,
) {
    let prefix_len = prefix.chars().count();
    if prefix_len == 0 {
        return;
    }
    let limit = prefix_len.saturating_mul(2).saturating_add(1);
    let mut suggestions = dict.system().suggest_after(prefix);
    if let Some(store) = user
        && let Ok(lookup) = oxpinyin_user::UserLookup::from_store(store)
    {
        suggestions.extend(lookup.suggest_after(prefix));
    }
    suggestions.sort_by_key(|(token, _)| *token);
    for (token, text) in suggestions {
        if text.chars().count() > limit {
            continue;
        }
        let frequency = dict.system().unigram_count(token).unwrap_or(0);
        into.push(Predicted {
            frequency,
            text,
            token,
            candidate_type: lookup_candidate_type_t::PREDICTED_PREFIX_CANDIDATE,
        });
    }
}

fn phrase_text(dict: &SharedDict, store: &UserStore, token: u32) -> Option<String> {
    if let Ok(Some(phrase)) = store.phrase(token) {
        return Some(phrase.text().to_owned());
    }
    dict.system().phrase_text(token).ok().flatten()
}
