//! Operation-local token facts shared by sentence pricing and scoring.
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

/// A multiplicative hasher for the trellis's `u32` token keys.
///
/// These maps are only probed (`get`, `entry`), never iterated: the order of
/// a position's nodes is its position's node vector, so the hash function
/// cannot reach the output. SipHash's per-lookup cost dominated the step expansion.
#[derive(Default, Clone, Copy)]
pub struct TokenHasher(u64);

impl Hasher for TokenHasher {
    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.write_u64(u64::from(byte));
        }
    }

    fn write_u32(&mut self, value: u32) {
        self.write_u64(u64::from(value));
    }

    fn write_u64(&mut self, value: u64) {
        self.0 = (self.0.rotate_left(5) ^ value).wrapping_mul(0x517c_c1b7_2722_0a95);
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

/// Probe-only token map; iteration must never determine output order.
pub type TokenMap<V> = HashMap<u32, V, BuildHasherDefault<TokenHasher>>;

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
