//! Operation-local token facts shared by sentence pricing and scoring.

/// Token map for keyed lookup only; it does not decide output order.
pub type TokenMap<V> = rustc_hash::FxHashMap<u32, V>;

/// A readable ordinary overlay item. Other origins use the existing readers.
#[derive(Clone, Copy, Debug)]
pub struct TokenFacts {
    /// Length in characters.
    pub chars: u32,
    /// Present live unigram, including zero.
    pub unigram: u32,
    /// Number of stored pronunciations.
    pub pronunciations: u32,
    /// Classification: ordinary overlay, visible, not user or imported.
    pub flags: u32,
    /// Offset into the guess's owned item bytes.
    pub offset: usize,
    /// Validated item extent.
    pub len: usize,
}

/// A single sentence invocation's memo, never retained in composition state.
#[derive(Default)]
pub struct SentenceFacts {
    generation: Option<u64>,
    disabled: bool,
    /// None records a classification requiring the existing fallback.
    pub tokens: TokenMap<Option<TokenFacts>>,
    /// Owned item bodies; offsets survive growth of this buffer.
    pub bytes: Vec<u8>,
    /// Lazy, generation-bound facade total adjustment.
    pub total: Option<u64>,
}

impl SentenceFacts {
    /// Validate before and after reads. A mismatch permanently retires this
    /// guess's memo, without restarting or repricing its existing trellis.
    pub fn validate(&mut self, generation: u64) -> bool {
        if self.disabled {
            return false;
        }
        if generation == u64::MAX || self.generation.is_some_and(|seen| seen != generation) {
            self.tokens.clear();
            self.bytes.clear();
            self.total = None;
            self.disabled = true;
            return false;
        }
        self.generation = Some(generation);
        true
    }
}
