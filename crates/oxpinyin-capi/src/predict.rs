//! Phrase prediction: prefixes → user-bigram successors → prefix suggestions,
//! then punctuation prepended from the Option A punct table.
//!
//! Reproduces `pinyin_guess_predicted_candidates` (`pinyin.cpp:2411-2451`)
//! and the punctuation prefix of
//! `pinyin_guess_predicted_candidates_with_punctuations` (`:2454-2498`).

use oxpinyin_capi_marshal::phrase_text::phrase_text_bytes;
use std::cell::Cell;
use std::collections::HashSet;
use std::os::raw::c_char;

use oxpinyin_core::{PhraseText, PhraseToken, single_gram};
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
        if let Ok(Some(row)) = user.bigram_row(prev.value()) {
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
    let Ok(prefixes) = oxpinyin_facade::compute_prefixes(&inst.core.dict, inst.core.user, prefix)
    else {
        std::process::abort();
    };
    // `m_prefixes` is emptied and refilled before the empty check
    // (`pinyin.cpp:2423-2426`).
    inst.core
        .session
        .set_candidate_prefixes(prefixes.into_iter().map(PhraseToken::new));
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
    // The bigram rows of a token with no item: they count as rows until the
    // duplicate removal, where register row 106 drops them.
    let rows_without_item = append_predicted_bigrams(
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
        rows_without_item,
        &mut items,
    );

    if let Some(token) = missing {
        // Only one pre-dedup row: neither strcmp runs. Its opaque score and
        // length are unobservable; preserve the NULL getter and choose token.
        inst.state.candidates.push(CapiCandidate {
            text: oxpinyin_facade::TextSpan::NULL,
            text_ptr: std::ptr::null(),
            type_tag: Cell::new(lookup_candidate_type_t::PREDICTED_PREFIX_CANDIDATE.tag()),
            nbest_index: 0,
            token: Cell::new(Some(oxpinyin_core::PhraseToken::new(token))),
            selection: oxpinyin_facade::SelectionSpan::default(),
            span_len: 0,
        });
        return Ok(true);
    }

    sort_predicted(&mut items);

    // A bigram row of a token with no item has a NULL phrase string here,
    // and the pin's `strcmp` reads it (`pinyin.cpp:2053`): register row 106.
    // Its registered result is that the token adds no candidate, so
    // `rows_without_item` rows end here.
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
            type_tag: Cell::new(item.candidate_type.tag()),
            nbest_index: 0,
            token: Cell::new(Some(oxpinyin_core::PhraseToken::new(item.token))),
            selection: oxpinyin_facade::SelectionSpan::default(),
            span_len: 0,
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
    let mut seen: HashSet<Vec<u8>> = HashSet::new();
    for token in inst.core.session.candidate_prefixes() {
        for punct in inst.core.dict.punctuations(token.value()) {
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
            selection: oxpinyin_facade::SelectionSpan::default(),
            span_len: 0,
        });
    }
    inst.state.candidates.append(rest);
}

/// What `FacadePhraseIndex::get_phrase_item` answers for a gram item's
/// token (`phrase_index.h:646-651`, `phrase_index.cpp:180-206` at the pin
/// `074a2219`).
#[derive(Clone, Debug, Eq, PartialEq)]
enum ItemRead {
    /// `ERROR_NO_SUB_PHRASE_INDEX`: the token's library has no sub-index.
    NoSubIndex,
    /// `ERROR_NO_ITEM`: the sub-index is there and the slot holds no item.
    NoItem,
    /// The item, by its phrase text.
    Item(PhraseText),
}

/// The rows `_compute_predicted_bigram_candidates` appends
/// (`pinyin.cpp:2342-2365` at the pin `074a2219`), over the gram items that
/// passed the count filter (`:2349-2350`). One `PhraseItem` serves the whole
/// walk (`:2342`): it starts empty, with length zero, and every read that
/// finds an item replaces it. A token whose library has no sub-index is
/// skipped (`:2352-2355`). A token with no item leaves the cached item as
/// the previous read left it, so its row is appended when that item's
/// length is the pass's length (`:2357-2363`). The longer pass runs first,
/// and the cached item carries over from it.
///
/// A row is its token and its phrase text, `None` for a token with no item.
fn select_bigram_rows(reads: &[(u32, ItemRead)]) -> Vec<(u32, Option<&PhraseText>)> {
    let mut cached_length = 0;
    let mut rows = Vec::new();
    for length in [2_usize, 1] {
        for (token, read) in reads {
            let text = match read {
                ItemRead::NoSubIndex => continue,
                ItemRead::NoItem => None,
                ItemRead::Item(text) => {
                    cached_length = text.phrase_length();
                    Some(text)
                }
            };
            if cached_length == length {
                rows.push((*token, text));
            }
        }
    }
    rows
}

/// Appends the predicted bigram rows that have an item, and answers how
/// many rows the pin appends for a token with no item.
///
/// Such a row has no phrase string: `_token_get_phrase` fails and leaves
/// `m_phrase_string` NULL (`pinyin.cpp:1635-1637`, `:2013-2016`). It never
/// stands alone, because the cached item that let it through belongs to a
/// row of the same pass. The duplicate removal therefore sorts at least two
/// rows and its `strcmp` reads the NULL string (`:2053`, `:2069-2070`):
/// register row 106, whose result is that the token adds no candidate. The
/// caller applies it there.
fn append_predicted_bigrams(
    dict: &SharedDict,
    lm: &SharedLm,
    lambda: f32,
    user: &UserStore,
    successors: &[(u32, u32)],
    into: &mut Vec<Predicted>,
) -> usize {
    let total = predicted_total(lm);
    let reads: Vec<(u32, ItemRead)> = successors
        .iter()
        // pinyin.cpp:2349-2350: skip when `m_count < filter` (10).
        .filter(|(_, count)| *count >= BIGRAM_FILTER)
        .map(|(token, _)| {
            // `prepare_tokens` and `get_phrase_item` see the same
            // sub-indices: a library with none answers
            // `ERROR_NO_SUB_PHRASE_INDEX`.
            let read = if !dict.phrase_table_token_enabled(*token) {
                ItemRead::NoSubIndex
            } else if !dict.library_visible_token(*token) {
                // A journal REMOVE took the item out of its sub-index.
                ItemRead::NoItem
            } else {
                phrase_text(dict, user, *token).map_or(ItemRead::NoItem, ItemRead::Item)
            };
            (*token, read)
        })
        .collect();
    let mut without_item = 0;
    for (token, text) in select_bigram_rows(&reads) {
        let Some(text) = text else {
            without_item += 1;
            continue;
        };
        into.push(Predicted {
            phrase_length: text.phrase_length(),
            // The caller passes null_token to the scorer, so the bigram
            // term is zero even with DYNAMIC_ADJUST (`pinyin.cpp:2418,2438`).
            frequency: predicted_frequency(dict, user, token, total, lambda),
            text: text.clone(),
            token,
            candidate_type: lookup_candidate_type_t::PREDICTED_BIGRAM_CANDIDATE,
        });
    }
    without_item
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
    user: &UserStore,
    prefix: &PhraseText,
    earlier_rows_without_item: usize,
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
    let (kept, sole_missing) =
        select_prefix_rows(suggestions, limit, into.len() + earlier_rows_without_item);
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
    user: &UserStore,
    token: u32,
    total: u64,
    lambda: f32,
) -> u32 {
    let store_count = || user.unigram_delta(token).unwrap_or(0);
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

fn phrase_text(dict: &SharedDict, store: &UserStore, token: u32) -> Option<PhraseText> {
    if let Ok(Some(phrase)) = store.phrase(token) {
        return phrase.phrase_text();
    }
    dict.token_text(token)
}

#[cfg(test)]
mod tests {
    use crate::test_support::{DEFAULT_SORT, cstr};

    fn text(phrase: &str) -> oxpinyin_core::PhraseText {
        oxpinyin_core::PhraseText::from(phrase)
    }

    /// `_compute_predicted_bigram_candidates` keeps one `PhraseItem` for
    /// the whole walk (`pinyin.cpp:2342` at the pin `074a2219`). It skips a
    /// token only on `ERROR_NO_SUB_PHRASE_INDEX` (`:2352-2355`); a token
    /// with no item is compared by the length the previous read left
    /// (`:2357-2358`) and appended with its own token (`:2360-2363`). The
    /// item starts empty and carries over from the longer pass to the
    /// shorter one (`:2344`).
    #[test]
    fn a_bigram_token_with_no_item_rides_on_the_previous_items_length() {
        use super::{ItemRead, select_bigram_rows};
        let (one, two) = (text("好"), text("你好"));
        let rows = |reads: &[(u32, ItemRead)]| -> Vec<(u32, bool)> {
            select_bigram_rows(reads)
                .into_iter()
                .map(|(token, text)| (token, text.is_some()))
                .collect()
        };
        // Before any read the cached item has length zero: no row.
        assert_eq!(rows(&[(9, ItemRead::NoItem)]), []);
        // After a one-character item, the token with no item is a row of
        // the one-character pass.
        assert_eq!(
            rows(&[(1, ItemRead::Item(one.clone())), (9, ItemRead::NoItem)]),
            [(1, true), (9, false)]
        );
        // The longer pass ends on the one-character item, so the shorter
        // pass starts with it cached: the token with no item comes first.
        assert_eq!(
            rows(&[(9, ItemRead::NoItem), (1, ItemRead::Item(one.clone()))]),
            [(9, false), (1, true)]
        );
        // A two-character item before it puts it in the longer pass; the
        // shorter pass then reads the same item again and skips both.
        assert_eq!(
            rows(&[(2, ItemRead::Item(two.clone())), (9, ItemRead::NoItem)]),
            [(2, true), (9, false)]
        );
        // A library with no sub-index is skipped and leaves the cached item.
        assert_eq!(
            rows(&[
                (1, ItemRead::Item(one)),
                (8, ItemRead::NoSubIndex),
                (9, ItemRead::NoItem),
                (2, ItemRead::Item(two)),
            ]),
            [(2, true), (1, true), (9, false)]
        );
    }

    #[rstest::rstest]
    #[case::plain("你", false, false, true)]
    #[case::missing("🦀", false, false, false)]
    #[case::punctuated("你", true, false, true)]
    #[case::borrowed("", false, true, false)]
    #[case::borrowed_punctuated("", true, true, true)]
    fn prediction_replaces_every_row_with_a_spanless_one(
        #[case] prefix: &str,
        #[case] punctuated: bool,
        #[case] borrowed: bool,
        #[case] accepted: bool,
    ) {
        let user = TempUserDir::new("prediction-window");
        let (context, instance) = open(user.path.to_str().expect("UTF-8 path"));
        assert_eq!(
            crate::parse::pinyin_parse_more_full_pinyins(instance, cstr("nihao").as_ptr()),
            5
        );
        assert!(crate::sentence::pinyin_guess_candidates(
            instance,
            2,
            DEFAULT_SORT
        ));
        // SAFETY: the live instance is read between serialized ABI calls.
        let prior_spans = unsafe { crate::state::instance_ref(instance) }
            .state
            .candidates
            .iter()
            .any(|row| row.selection != oxpinyin_facade::SelectionSpan::default());
        assert!(prior_spans);
        let owned = cstr(prefix);
        let prefix = if borrowed {
            let mut candidate = std::ptr::null_mut();
            let mut text = std::ptr::null();
            assert!(crate::candidates::pinyin_get_candidate(
                instance,
                0,
                &raw mut candidate
            ));
            assert!(crate::candidates::pinyin_get_candidate_string(
                instance,
                candidate,
                &raw mut text
            ));
            assert!(!text.is_null());
            text
        } else {
            owned.as_ptr()
        };
        let answer = if punctuated {
            crate::sentence::pinyin_guess_predicted_candidates_with_punctuations(instance, prefix)
        } else {
            super::pinyin_guess_predicted_candidates(instance, prefix)
        };
        assert_eq!(answer, accepted);
        // The pin frees its former candidate list before reading prefixes
        // (pinyin.cpp:2420-2427, 2459), and every row that replaces it has
        // the default `m_begin = m_end = 0` (`:114`), even when the
        // snapshot is left empty.
        // SAFETY: the live instance is read between serialized ABI calls.
        let spanless = unsafe { crate::state::instance_ref(instance) }
            .state
            .candidates
            .iter()
            .all(|row| row.selection == oxpinyin_facade::SelectionSpan::default());
        assert!(spanless);
        crate::instance::pinyin_free_instance(instance);
        crate::context::pinyin_fini(context);
    }

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
            let store = ctx.core.user_store_mut();
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
