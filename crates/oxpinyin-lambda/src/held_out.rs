//! Held-out bigram counting reproducing libpinyin `gen_deleted_ngram`
//! (`utils/training/gen_deleted_ngram.cpp:76-121`).
//!
//! `gen_deleted_ngram` is `gen_ngram` with exactly two deletions
//! (verified by reading both files against each other):
//!
//! 1. it does **not** call `phrase_index.add_unigram_frequency(cur_token, 1)`
//!    (`gen_ngram.cpp:96-97` has no counterpart in `gen_deleted_ngram.cpp`),
//!    so it never touches the unigram phrase index; and
//! 2. it writes `DELETED_BIGRAM` (`deleted_bigram.db`) instead of
//!    `SYSTEM_BIGRAM`, and never saves the phrase index
//!    (`gen_deleted_ngram.cpp` has no `save_phrase_index`).
//!
//! The **bigram counting loop itself is byte-identical** to `gen_ngram`'s
//! (`gen_ngram.cpp:88-124` vs `gen_deleted_ngram.cpp:88-121`): same
//! `last`/`cur` tracking, same `null_token` second-word skip, same
//! `sentence_start` boundary substitution, same `+1` increment. So the
//! held-out bigram **values** equal what `oxpinyin_counter::Counter` produces
//! for its bigram half over the same input; [`count_deleted`] reimplements
//! that loop directly (citing the deleted source) and a test asserts the
//! two agree, pinning the SHOWN "identical counting" claim mechanically.
//!
//! No floor, no unigram: the held-out counts feed the EM's *deleted* term
//! only (`estimate_interpolation.cpp:44-51`), which reads bigram counts and
//! per-context totals. Values are integers; no float appears here.

use std::collections::BTreeMap;

use oxpinyin_counter::parse_ngseg;
use oxpinyin_segment::{NULL_TOKEN, SENTENCE_START};

use crate::error::LambdaError;

/// Raw held-out bigram counts — the `DELETED_BIGRAM` the EM consumes.
///
/// `(prev, cur) → count`, `prev` substituted with `sentence_start` at
/// sentence boundaries. Integer counts only: no unigram section (the pin's
/// `gen_deleted_ngram` never increments unigrams) and no freq-1 floor.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DeletedCounts {
    /// `(prev, cur) → count`.
    pub bigrams: BTreeMap<(u32, u32), u64>,
}

impl DeletedCounts {
    /// Distinct `prev` contexts, ascending — the outer keys the EM averages
    /// over (`estimate_interpolation.cpp:119`, one λ per `prev`).
    ///
    /// The pin walks these via `Bigram::get_all_items` (tkrzw
    /// `ProcessEach`, hash order); the per-context λ is keyed by `prev`, so
    /// this ordering affects only the outer sum's rounding, never which
    /// contexts exist. See `docs/findings/lambda-port.md` §float-boundary.
    pub fn contexts(&self) -> impl Iterator<Item = u32> + '_ {
        // Deduplicate ascending `prev` from the sorted bigram keys.
        let mut last: Option<u32> = None;
        self.bigrams.keys().filter_map(move |&(prev, _)| {
            if last == Some(prev) {
                None
            } else {
                last = Some(prev);
                Some(prev)
            }
        })
    }

    /// The `cur → count` `SingleGram` for one `prev` context, ascending by
    /// `cur` — the exact order `SingleGram::retrieve_all` yields (items are
    /// kept token-sorted by `insert_freq`'s `lower_bound`, `ngram.cpp:185`),
    /// which the inner EM loop iterates (`estimate_interpolation.cpp:47`).
    #[must_use]
    pub fn context(&self, prev: u32) -> BTreeMap<u32, u64> {
        self.bigrams
            .range((prev, u32::MIN)..=(prev, u32::MAX))
            .map(|(&(_, cur), &count)| (cur, count))
            .collect()
    }
}

/// Counts held-out bigrams over an ngseg token stream, reproducing
/// `gen_deleted_ngram.cpp:88-121`. `train_pi_gram` mirrors
/// `--skip-pi-gram-training` (default `true`, boundary bigrams trained
/// against `sentence_start`).
#[must_use]
pub fn count_deleted_tokens(tokens: &[u32], train_pi_gram: bool) -> DeletedCounts {
    let mut bigrams: BTreeMap<(u32, u32), u64> = BTreeMap::new();

    // `phrase_token_t last_token, cur_token = last_token = 0;` — both
    // start as `null_token`.
    let mut cur_token = NULL_TOKEN;

    for &token in tokens {
        // `last_token = cur_token; cur_token = token;`
        let last_token = std::mem::replace(&mut cur_token, token);

        // `if ( null_token == cur_token ) continue;`  — skip null second word.
        // (No `add_unigram_frequency` here: gen_deleted_ngram omits it.)
        if NULL_TOKEN == cur_token {
            continue;
        }

        // `if ( null_token == last_token ) { if (!train_pi_gram) continue;
        //  last_token = sentence_start; }`
        let last_token = if NULL_TOKEN == last_token {
            if !train_pi_gram {
                continue;
            }
            SENTENCE_START
        } else {
            last_token
        };

        // `get_freq → set_freq(+1)` else `insert_freq(1)`; total_freq += 1.
        *bigrams.entry((last_token, cur_token)).or_insert(0) += 1;
    }

    DeletedCounts { bigrams }
}

/// Parses an ngseg held-out stream and counts its deleted bigrams.
///
/// # Errors
///
/// Returns [`LambdaError::Count`] when a line is not `token phrase`
/// (`TAGLIB_PARSE_SEGMENTED_LINE` aborts on that shape).
pub fn count_deleted(text: &str, train_pi_gram: bool) -> Result<DeletedCounts, LambdaError> {
    let tokens = parse_ngseg(text)?;
    Ok(count_deleted_tokens(&tokens, train_pi_gram))
}

#[cfg(test)]
mod tests {
    use super::{DeletedCounts, count_deleted_tokens};
    use std::collections::BTreeMap;

    /// The held-out bigram values equal `Counter`'s bigram half over the
    /// same input — the SHOWN "identical counting loop" claim, checked
    /// mechanically against T2's validated counter.
    #[test]
    fn matches_counter_bigrams() {
        let tokens = [
            16_817_937u32,
            16_782_711,
            0,
            16_802_309,
            16_808_451,
            16_802_309,
        ];
        let deleted = count_deleted_tokens(&tokens, true);
        let counter = oxpinyin_counter::Counter::new(true, std::iter::empty()).count(&tokens);
        assert_eq!(deleted.bigrams, counter.bigrams);
    }

    #[test]
    fn no_unigram_section_exists() {
        // Structural: DeletedCounts carries bigrams only. Contrast the
        // counter, which also emits a floored unigram map.
        let deleted = count_deleted_tokens(&[10_u32, 20], true);
        assert_eq!(
            deleted.bigrams,
            BTreeMap::from([((1, 10), 1), ((10, 20), 1)])
        );
    }

    #[test]
    fn sentence_start_substituted_at_boundary() {
        // [A, null, B] → (start, B); the (A, ...) boundary is dropped and
        // A never pairs across the null.
        let deleted = count_deleted_tokens(&[10_u32, 0, 20], true);
        assert_eq!(deleted.bigrams.get(&(1, 20)), Some(&1));
        assert!(!deleted.bigrams.contains_key(&(10, 20)));
    }

    #[test]
    fn skip_pi_gram_drops_boundary_bigrams() {
        let deleted = count_deleted_tokens(&[10_u32, 0, 20], false);
        assert!(deleted.bigrams.is_empty());
    }

    #[test]
    fn contexts_and_context_slices_are_ascending() {
        let deleted = DeletedCounts {
            bigrams: BTreeMap::from([((20, 5), 2), ((10, 30), 1), ((10, 7), 4), ((20, 1), 3)]),
        };
        assert_eq!(deleted.contexts().collect::<Vec<_>>(), vec![10, 20]);
        assert_eq!(
            deleted.context(10).into_iter().collect::<Vec<_>>(),
            vec![(7, 4), (30, 1)]
        );
        assert_eq!(
            deleted.context(20).into_iter().collect::<Vec<_>>(),
            vec![(1, 3), (5, 2)]
        );
    }
}
