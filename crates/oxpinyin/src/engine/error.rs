//! Engine failures.

use core::fmt;

use crate::graph::GraphError;
use crate::scoring::ScoringError;

/// Which `increase_pronunciation_possibility` precondition a stale
/// training span violates (`storage/phonetic_key_matrix.cpp` at the pin).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrainingSpanFault {
    /// `assert(end < matrix->size())` — the span's end sits past the
    /// current matrix (`phonetic_key_matrix.cpp:661`).
    PastMatrixEnd,
    /// `assert(matrix->get_column_size(start) > 0)` — the span starts on
    /// a column the current matrix leaves empty (`:663`). An internal
    /// separator between keys is filled with a zero key (not empty), so it
    /// is accepted; only a genuine gap (a leading separator run) trips this.
    EmptyStartColumn,
    /// `assert(matrix->get_column_size(end) > 0)` — the span ends on a
    /// column the current matrix leaves empty (`:664`). The internal
    /// apostrophe can leave the start column non-empty (a zero key) while
    /// the stale end still lands on an empty column.
    EndEmptyColumn,
}

/// Anything a session can fail at.
///
/// Backend failures arrive as text because the frozen `Dictionary` and
/// `LanguageModel` seams leave their `Error` types unbounded: the engine
/// renders them at the boundary rather than leaking an associated type into
/// its public surface.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum EngineError {
    /// A candidate index was out of range, most likely a stale one.
    #[error("candidate index {index} is out of range 0..{len}")]
    CandidateIndexOutOfRange {
        /// The index the caller asked for.
        index: usize,
        /// How many candidates the list actually holds.
        len: usize,
    },
    /// A lookup offset sat one position past a leading separator run,
    /// which the zero-start walk cannot normalize away — `_check_offset`'s
    /// refusal at libpinyin@dbff264, answered as an error instead of the
    /// upstream abort.
    #[error(
        "lookup offset {offset} sits one past a leading separator \
         run (normalized to {normalized})"
    )]
    LookupOffsetPastSeparator {
        /// The offset the caller asked to look up at.
        offset: usize,
        /// Where the zero-start walk stopped.
        normalized: usize,
    },
    /// A lookup offset lay beyond the raw input's one-past-end position —
    /// upstream reads its matrix out of bounds there, so the range has no
    /// pinned behaviour and the engine refuses it.
    #[error("lookup offset {offset} is out of range 0..={len}")]
    LookupOffsetOutOfRange {
        /// The offset the caller asked to look up at.
        offset: usize,
        /// The raw input length the offset may at most equal.
        len: usize,
    },
    /// A lookup offset fell inside a multi-byte character of the raw
    /// input — no window exists under a mid-character slice, so the
    /// anchor is refused rather than rounded (rounding would silently
    /// answer a neighbouring offset's window).
    #[error(
        "lookup offset {offset} falls inside a multi-byte character \
         of the {len}-byte raw input"
    )]
    LookupOffsetInsideCharacter {
        /// The offset the caller asked to look up at.
        offset: usize,
        /// The raw input length the offset was range-checked against.
        len: usize,
    },
    /// A selection was requested from a window anchored before the
    /// composition offset — a stale cursor behind the selection, whose span
    /// would regress the consumed boundary. Rejected rather than
    /// reconciled; no frontend drives a backward selection.
    ///
    /// Amended 2026-09-27 (maintainer ruling, register row 37): a frontend does
    /// drive it — ibus-libpinyin's `moveCursorLeft` looks up at 0 behind a
    /// choose — and the pin forces the chosen span over the overlapped
    /// forcings (`pinyin.cpp:2578-2590`, `phonetic_lookup.cpp:61-86`). A
    /// token-bearing row is now selected that way and the record moves
    /// back to it; this error remains only for a row with no token to
    /// force (the engine's raw-input `Fallback` row).
    #[error("selection anchor {anchor} precedes the composition offset {composition}")]
    SelectionAnchorBeforeComposition {
        /// The window anchor the caller supplied.
        anchor: usize,
        /// The composition offset (the selected boundary) it precedes.
        composition: usize,
    },
    /// A cursor normalization or word move examined an offset one past a
    /// lone zero-key matrix column — the shape the pin's `_check_offset`
    /// aborts on (`assert(zero_key != key)`, `pinyin.cpp:2175` at the pin)
    /// and post-`95e3af7` libpinyin answers with a `false` every caller
    /// discards (the function then completes with the computed value).
    /// The engine answers with an error; the left and right moves abort on
    /// it in the C adapters (register row 14).
    #[error("offset {offset} sits one past a lone zero-key column")]
    ZeroKeyOffsetCheck {
        /// The examined offset whose preceding column holds the lone zero key.
        offset: usize,
    },
    /// The character-offset walk reached a matrix column the pin asserts
    /// on: an empty column at a stepped-to position (`assert(size > 0)`,
    /// `pinyin.cpp:3152` at the pin) or a zero key sharing a column with
    /// real keys (`assert(1 == size)`, `pinyin.cpp:3166`). Answered as an
    /// error, on which the C adapters abort (register row 19).
    #[error("matrix column {offset} has no key or mixes a zero key with real keys")]
    MatrixColumnAssert {
        /// The column position the walk stepped to.
        offset: usize,
    },
    /// A training walk met a user-forced phrase whose token differs from the
    /// token the decoded result holds there — the constraints changed after
    /// the last sentence lookup. The pin's `train_result3` asserts it
    /// (`assert(token == constraint->m_token)`, `phonetic_lookup.h:868` at
    /// the pin); the engine answers an error, before observing anything.
    #[error("the forced phrase at {position} differs from the decoded result's")]
    StaleTrainingConstraint {
        /// The matrix position of the forced phrase.
        position: usize,
    },
    /// A training step would load a predecessor whose stored user-bigram
    /// value has a partial item tail. The pin's `insert_freq`
    /// (`ngram.cpp:178`, loop `:190`), `get_freq` (`:231`, loop `:241`) and `set_freq`
    /// (`:252`, loop `:261`) walk
    /// with `cur_item != end` and read past it (register row 100); the
    /// engine answers an error, before observing anything.
    #[error("the predecessor {token:#010x} has a partial user-bigram item")]
    UnsafeTrainingPredecessor {
        /// The predecessor token whose gram has the partial tail.
        token: u32,
    },
    /// A training step would load a predecessor whose stored user-bigram
    /// value is shorter than its `guint32` total: the pin's
    /// `get_total_freq` asserts on it (`phonetic_lookup.h:884` through
    /// `ngram.cpp:49`, `memory_chunk.h:390`; register row 85). The engine
    /// answers an error, before observing anything.
    #[error("the predecessor {token:#010x} has a user-bigram value shorter than its total")]
    ShortTrainingPredecessor {
        /// The predecessor token whose gram is too short for a total.
        token: u32,
    },
    /// A lookup merged or read a predecessor's bigram where the pin
    /// asserts: a stored user-bigram value shorter than its `guint32`
    /// total, whose total `merge_single_gram` (`ngram.cpp:304`),
    /// `SingleGram::search` (`:165`) or the `DYNAMIC_ADJUST` term
    /// (`pinyin.cpp:1848`, `zhuyin.cpp:1250`) reads through
    /// `memory_chunk.h:390` (register row 85). The engine answers an error
    /// instead of the abort.
    #[error("the predecessor {token:#010x} has a user-bigram value shorter than its total")]
    ShortLookupPredecessor {
        /// The predecessor token whose gram is too short for a total.
        token: u32,
    },
    /// A lookup would read past a predecessor's bigram value: a partial
    /// item the merge consumes (`ngram.cpp:318-349`), or a `get_freq` or
    /// `search` walk (`:241-248`, `:167-173`) that leaves the value
    /// (register row 98). The engine answers an error instead.
    #[error("reading the predecessor {token:#010x}'s bigram leaves its value")]
    UnsafeLookupPredecessor {
        /// The predecessor token whose gram the read leaves.
        token: u32,
    },
    /// A checked backend read found a record, then failed to read its
    /// value or read a different size. Kyoto Cabinet's `Bigram::load`
    /// asserts at that point (`ngram_kyotodb.cpp:131-139` at 074a2219);
    /// the engine answers an error instead.
    #[error("the predecessor {token:#010x}'s bigram read failed after its size check")]
    ReadAfterCheck {
        /// The predecessor token whose bigram read failed.
        token: u32,
    },
    /// A training step's `set_freq` would miss the item its `insert_freq`
    /// just added to the predecessor's unsorted gram: the pin's
    /// `check_result(user->set_freq(token, freq + seed))` asserts
    /// (`phonetic_lookup.h:903`). The engine answers an error, before
    /// observing anything.
    #[error("training the predecessor {token:#010x}'s gram misses its inserted item")]
    TrainingSetFreqMiss {
        /// The predecessor token whose gram the step trains.
        token: u32,
    },
    /// A training step would store its gram in the failed Berkeley DB user
    /// bigram container, whose `store` answers false: the pin's
    /// `check_result(m_user_bigram->store(last_token, user))` asserts
    /// (`phonetic_lookup.h:904`). The engine answers an error, before
    /// observing anything.
    #[error(
        "training the predecessor {token:#010x}'s gram stores into the failed \
         user bigram container"
    )]
    FailedUserBigramStore {
        /// The predecessor token whose gram the step trains.
        token: u32,
    },
    /// A training span met a matrix position the pin's
    /// `increase_pronunciation_possibility` asserts on: the decoded
    /// result no longer fits the current matrix because a re-parse
    /// shortened the input after the last sentence lookup. The pin aborts
    /// (`assert(end < matrix->size())`, `storage/phonetic_key_matrix.cpp:661`;
    /// `assert(matrix->get_column_size(start) > 0)`, `:663`;
    /// `assert(matrix->get_column_size(end) > 0)`, `:664` at the pin);
    /// the engine answers an error instead, before observing anything.
    #[error("the training span at {position} leaves the current matrix ({fault:?})")]
    StaleTrainingSpan {
        /// The matrix position of the offending phrase.
        position: usize,
        /// Which precondition the span violates.
        fault: TrainingSpanFault,
    },
    /// The longer-candidate walk met a pinyin index key (the user's or the
    /// system's) past `MAX_PHRASE_LENGTH` syllables that extends the
    /// current prefix — the pin's `ChewingLargeTable2::search_suggestion`,
    /// over either table, reaches the key
    /// through its `DB_SET`/`DB_NEXT` walk and its `switch`'s
    /// `default: abort()` dies (`storage/chewing_large_table2_bdb.cpp:282`
    /// at the pin); the engine answers an error, on which the C adapters
    /// abort (register row 84).
    #[error(
        "a pinyin index key past MAX_PHRASE_LENGTH syllables extends the \
         {syllables}-syllable prefix"
    )]
    OverlongUserIndexKey {
        /// The prefix path length the walk was on when it reached the key.
        syllables: usize,
    },
    /// A phrase-table search over a key path whose first initial-only key
    /// carries a tone: `contains_incomplete_pinyin` asserts
    /// `CHEWING_ZERO_TONE == key.m_tone` on the first initial-only key
    /// (`storage/pinyin_phrase3.h:146-156` at 074a2219), at the top of every
    /// table search and suggestion search that holds it.
    #[error("a searched key path's first initial-only key carries a tone")]
    TonedIncompleteKey,
    /// A character's one-character phrase-table search held more than four
    /// tokens. `_pre_compute_tokens` reduces it through `get_first_token`
    /// (`pinyin.cpp:3117`, `zhuyin.cpp:2080`), whose validating
    /// `reduce_tokens` asserts `0 <= num && num <= 4`
    /// (`phrase_large_table3.h:95`, `:106`).
    #[error("a phrase-table search held {count} tokens, more than four")]
    ReduceTokensCount {
        /// The tokens the search held.
        count: usize,
    },
    /// The user-model backend failed (the learning/observation seam).
    #[error("user model error: {0}")]
    UserModel(String),
    /// The input could not be represented as a segment graph.
    #[error("graph error: {0}")]
    Graph(#[from] GraphError),
    /// Scoring failed.
    #[error("scoring error: {0}")]
    Scoring(#[from] ScoringError),
}

impl EngineError {
    /// [`Self::TonedIncompleteKey`] where the pin's table search over
    /// `keys` (with their typed `tones`) asserts: the first initial-only
    /// key, if any, carries a tone (`contains_incomplete_pinyin` returns at
    /// the first one, `pinyin_phrase3.h:148-154`).
    ///
    /// # Errors
    ///
    /// [`Self::TonedIncompleteKey`] on that path.
    pub(crate) fn check_toned_incomplete(
        keys: &[crate::SyllableKey],
        tones: &[u8],
    ) -> Result<(), Self> {
        let first = keys
            .iter()
            .position(|key| key.completeness() == crate::Completeness::Partial);
        match first {
            Some(index) if tones.get(index).is_some_and(|tone| *tone != 0) => {
                Err(Self::TonedIncompleteKey)
            }
            _ => Ok(()),
        }
    }

    /// The lookup refusal for a [`crate::GramFault`] met on
    /// `token`'s merged row.
    #[must_use]
    pub const fn lookup_gram(token: u32, fault: crate::GramFault) -> Self {
        match fault {
            crate::GramFault::Short => Self::ShortLookupPredecessor { token },
            crate::GramFault::OverRead => Self::UnsafeLookupPredecessor { token },
        }
    }

    /// A model failure while reading `token`'s bigram row:
    /// [`Self::ReadAfterCheck`] where the model reports the read that failed
    /// after its size check (Kyoto Cabinet's `Bigram::load` assert,
    /// `ngram_kyotodb.cpp:136-137` at 074a2219), a scoring error otherwise.
    pub(crate) fn row_read<L>(model: &L, token: u32, error: &L::Error) -> Self
    where
        L: crate::model::LanguageModel + ?Sized,
        L::Error: fmt::Display,
    {
        if model.is_read_after_check(error) {
            Self::ReadAfterCheck { token }
        } else {
            Self::Scoring(ScoringError::LanguageModel(error.to_string()))
        }
    }
}

#[cfg(test)]
mod tests;
