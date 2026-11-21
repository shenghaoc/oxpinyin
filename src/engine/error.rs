//! Engine failures.

/// Why the shared training preparation stopped
/// ([`crate::engine::Session::prepare_training`]).
///
/// The preparation is read-only and shared by production training and the
/// fuzz probe, so it answers the pin's own training aborts as a typed
/// signal: production terminates on it, the probe answers `true`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TrainingPrepFault {
    /// The pin's own training abort: the index past nonempty results
    /// (`pinyin.cpp:2684` at 074a2219), a forced phrase whose token
    /// differs from the decoded result's (`phonetic_lookup.h:868`), or a
    /// span outside the current matrix — past its end, or starting or
    /// ending on an empty column (`storage/phonetic_key_matrix.cpp:661`,
    /// `:663`, `:664`).
    PinAbort,
    /// The per-step store check's or a backend's failure. Production
    /// propagates it; the probe answers the C facade's classification of
    /// it — `true` only for a store fault the facade terminates on.
    Engine(EngineError),
}

impl From<EngineError> for TrainingPrepFault {
    fn from(error: EngineError) -> Self {
        Self::Engine(error)
    }
}

/// Anything a session can fail at.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum EngineError {
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
    /// A cursor normalization or word move examined an offset one past a
    /// lone zero-key matrix column — the shape the pin's `_check_offset`
    /// aborts on (`assert(zero_key != key)`, `pinyin.cpp:2175` at the pin)
    /// The engine answers with an error; the left and right moves abort on
    /// it in the C adapters.
    #[error("an offset sits one past a lone zero-key column")]
    ZeroKeyOffsetCheck,
    /// The character-offset walk reached a matrix column the pin asserts
    /// on: an empty column at a stepped-to position (`assert(size > 0)`,
    /// `pinyin.cpp:3152` at the pin) or a zero key sharing a column with
    /// real keys (`assert(1 == size)`, `pinyin.cpp:3166`). Answered as an
    /// error, on which the C adapters abort. The same
    /// answer carries `reduce_tokens`' assert on more than four tokens
    /// (`phrase_large_table3.h:95`, `:106`) in character-offset and prefix lookup.
    #[error("a matrix column or phrase-token count failed the pin's check")]
    PinAssert,
    /// A training step would load a predecessor whose stored user-bigram
    /// value has a partial item tail. The pin's `insert_freq`
    /// (`ngram.cpp:178`, loop `:190`), `get_freq` (`:231`, loop `:241`) and `set_freq`
    /// (`:252`, loop `:261`) walk
    /// with `cur_item != end` and read past it; the
    /// engine answers an error, before observing anything.
    #[error("a predecessor has a partial user-bigram item")]
    UnsafeTrainingPredecessor,
    /// A training step would load a predecessor whose stored user-bigram
    /// value is shorter than its `guint32` total: the pin's
    /// `get_total_freq` asserts on it (`phonetic_lookup.h:884` through
    /// `ngram.cpp:49`, `memory_chunk.h:390`). The engine
    /// answers an error, before observing anything. The same answer carries
    /// the pin's `set_freq` and user-bigram `store` checks
    /// (`phonetic_lookup.h:903-904`).
    #[error("a training store check failed at a pin assertion")]
    TrainingAbort,
    /// A lookup would read past a predecessor's bigram value: a partial
    /// item the merge consumes (`ngram.cpp:318-349`), or a `get_freq` or
    /// `search` walk (`:241-248`, `:167-173`) that leaves the value. The engine answers an error instead.
    #[error("reading a predecessor's bigram leaves its value")]
    UnsafeLookupPredecessor,
    /// A checked backend read found a record, then failed to read its
    /// value or read a different size. Kyoto Cabinet's `Bigram::load`
    /// asserts at that point (`ngram_kyotodb.cpp:131-139` at 074a2219);
    /// the engine answers an error instead.
    #[error("a predecessor's bigram read failed after its size check")]
    ReadAfterCheck,
    /// The user-model backend failed (the learning/observation seam).
    #[error("user model error")]
    UserModel,
    /// An ordinary graph, dictionary, model or selection failure.
    #[error("operation failed")]
    Failure,
}

/// The pin's table-search assert over `keys` (with their typed `tones`):
/// the first initial-only key, if any, must carry no tone
/// (`contains_incomplete_pinyin` returns at the first one,
/// `pinyin_phrase3.h:148-154`; the search asserts
/// `CHEWING_ZERO_TONE == key.m_tone`, `:152` at 074a2219).
///
/// # Aborts
///
/// Terminates where the pin's assert fires: the first initial-only key
/// carries a tone.
pub(crate) fn abort_toned_incomplete(keys: &[crate::SyllableKey], tones: &[u8]) {
    let first = keys
        .iter()
        .position(|key| key.completeness() == crate::Completeness::Partial);
    if let Some(index) = first
        && tones.get(index).is_some_and(|tone| *tone != 0)
    {
        std::process::abort();
    }
}

impl EngineError {
    /// The lookup refusal for a [`crate::GramFault`] met on a merged row.
    ///
    /// # Aborts
    ///
    /// Terminates on [`crate::GramFault::Short`] — a stored system or user
    /// bigram value shorter than its `guint32` total, whose total
    /// `merge_single_gram` (`ngram.cpp:303-304`), `SingleGram::search`
    /// (`:165`), `get_total_freq` (`:49`) or the `DYNAMIC_ADJUST` term
    /// (`pinyin.cpp:1848`, `zhuyin.cpp:1250`) reads through
    /// `memory_chunk.h:390`.
    #[must_use]
    pub fn lookup_gram(fault: crate::GramFault) -> Self {
        match fault {
            crate::GramFault::Short => std::process::abort(),
            crate::GramFault::OverRead => Self::UnsafeLookupPredecessor,
        }
    }

    /// A model failure while reading a bigram row:
    /// [`Self::ReadAfterCheck`] where the model reports the read that failed
    /// after its size check (Kyoto Cabinet's `Bigram::load` assert,
    /// `ngram_kyotodb.cpp:138-139` at 074a2219), a scoring error otherwise.
    pub(crate) fn row_read<L>(model: &L, error: &L::Error) -> Self
    where
        L: crate::model::LanguageModel + ?Sized,
    {
        if model.is_read_after_check(error) {
            Self::ReadAfterCheck
        } else {
            Self::Failure
        }
    }
}

#[cfg(test)]
mod tests;
