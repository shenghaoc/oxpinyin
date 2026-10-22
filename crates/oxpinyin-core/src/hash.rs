//! A fixed-seed hasher for the small maps on the guess path.
//!
//! The standard library's `RandomState` seeds SipHash-1-3 per process. On
//! the guess path that cost about a tenth of a keystroke in hashing tokens
//! and candidate strings, for maps whose iteration order never reaches the
//! output.

use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};

/// The FxHash multiplier.
const SEED: u64 = 0x517c_c1b7_2722_0a95;

/// FxHash-style multiply-rotate hasher with a fixed seed.
///
/// The trade-off is deliberate: it is deterministic across processes and
/// platforms and costs a few instructions per word, but it does not resist
/// hash flooding. An input chosen to collide degrades a map to a linear
/// scan. That is accepted for a local input method, whose keys are the
/// user's own tokens and candidate strings (maintainer approval,
/// 2026-10-22, #1031). Do not use it for keys an untrusted peer chooses.
#[derive(Clone, Copy, Debug, Default)]
pub struct FixedHasher(u64);

impl FixedHasher {
    fn add(&mut self, word: u64) {
        self.0 = (self.0.rotate_left(5) ^ word).wrapping_mul(SEED);
    }
}

impl Hasher for FixedHasher {
    fn write(&mut self, bytes: &[u8]) {
        let mut chunks = bytes.chunks_exact(8);
        for chunk in &mut chunks {
            let mut word = [0; 8];
            word.copy_from_slice(chunk);
            self.add(u64::from_le_bytes(word));
        }
        let rest = chunks.remainder();
        if !rest.is_empty() {
            let mut word = [0; 8];
            word[..rest.len()].copy_from_slice(rest);
            // The length keeps "a" and "a\0" apart.
            self.add(u64::from_le_bytes(word) ^ ((rest.len() as u64) << 59));
        }
    }

    fn write_u8(&mut self, value: u8) {
        self.add(u64::from(value));
    }

    fn write_u16(&mut self, value: u16) {
        self.add(u64::from(value));
    }

    fn write_u32(&mut self, value: u32) {
        self.add(u64::from(value));
    }

    fn write_u64(&mut self, value: u64) {
        self.add(value);
    }

    fn write_usize(&mut self, value: usize) {
        // As a u64, so a key hashes the same on 32- and 64-bit targets.
        self.add(value as u64);
    }

    fn finish(&self) -> u64 {
        // The multiply leaves its best-mixed bits at the top; the table
        // indexes buckets by the low bits.
        self.0.rotate_left(26)
    }
}

/// The [`std::hash::BuildHasher`] of [`FixedHasher`].
pub type FixedState = BuildHasherDefault<FixedHasher>;

/// A [`HashMap`] hashed with [`FixedHasher`].
pub type FixedHashMap<K, V> = HashMap<K, V, FixedState>;

/// A [`HashSet`] hashed with [`FixedHasher`].
pub type FixedHashSet<T> = HashSet<T, FixedState>;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::hash::{BuildHasher, Hash};

    fn hash_of(value: impl Hash) -> u64 {
        FixedState::default().hash_one(value)
    }

    #[test]
    fn hashes_are_fixed_across_builders() {
        assert_eq!(hash_of(0x0100_0001_u32), hash_of(0x0100_0001_u32));
        assert_eq!(hash_of("你好"), hash_of("你好"));
        assert_ne!(hash_of(1_u32), hash_of(2_u32));
    }

    #[test]
    fn a_short_tail_is_not_its_zero_padding() {
        assert_ne!(hash_of("a"), hash_of("a\0"));
        assert_ne!(hash_of(""), hash_of("\0"));
        assert_ne!(hash_of("abcdefgh"), hash_of("abcdefgh\0"));
    }

    #[test]
    fn maps_and_sets_work_as_the_std_ones() {
        let mut map: FixedHashMap<u32, u64> = FixedHashMap::default();
        let mut set: FixedHashSet<&str> = FixedHashSet::default();
        for token in 0..10_000_u32 {
            map.insert(token | 0x0100_0000, u64::from(token));
            assert!(set.insert(if token % 2 == 0 { "even" } else { "odd" }) || token > 1);
        }
        assert_eq!(map.len(), 10_000);
        assert_eq!(map.get(&0x0100_0042), Some(&0x42));
        assert_eq!(set.len(), 2);
    }
}
