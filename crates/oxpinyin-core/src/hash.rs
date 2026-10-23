//! Fixed-seed hashing for the local IME's decode-time lookup tables.

use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};

/// A deterministic, fast multiply-rotate hasher with a fixed zero seed.
///
/// This does not resist hash flooding. The maintainer accepts that trade-off
/// for a local IME (#1031); use it only for the audited guess-path tables.
/// Table iteration must never decide candidate order.
#[derive(Clone, Copy, Debug, Default)]
pub struct FixedHasher(u64);

impl FixedHasher {
    #[inline]
    fn mix(&mut self, word: u64) {
        self.0 = (self.0.rotate_left(5) ^ word).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
}

impl Hasher for FixedHasher {
    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }

    #[inline]
    fn write(&mut self, mut bytes: &[u8]) {
        while let Some((word, rest)) = bytes.split_first_chunk::<8>() {
            self.mix(u64::from_le_bytes(*word));
            bytes = rest;
        }
        if let Some((word, rest)) = bytes.split_first_chunk::<4>() {
            self.mix(u64::from(u32::from_le_bytes(*word)));
            bytes = rest;
        }
        if let Some((word, rest)) = bytes.split_first_chunk::<2>() {
            self.mix(u64::from(u16::from_le_bytes(*word)));
            bytes = rest;
        }
        if let Some(&byte) = bytes.first() {
            self.mix(u64::from(byte));
        }
    }

    #[inline]
    fn write_u8(&mut self, value: u8) {
        self.mix(u64::from(value));
    }

    #[inline]
    fn write_u16(&mut self, value: u16) {
        self.mix(u64::from(value));
    }

    #[inline]
    fn write_u32(&mut self, value: u32) {
        self.mix(u64::from(value));
    }

    #[inline]
    fn write_u64(&mut self, value: u64) {
        self.mix(value);
    }

    #[inline]
    fn write_usize(&mut self, value: usize) {
        self.mix(value as u64);
    }
}

/// The fixed-seed builder shared by the audited decode-time tables.
pub type FixedBuildHasher = BuildHasherDefault<FixedHasher>;

/// A lookup table using [`FixedHasher`]. Output order must live elsewhere.
pub type FixedHashMap<K, V> = HashMap<K, V, FixedBuildHasher>;

/// A membership set using [`FixedHasher`]. Output order must live elsewhere.
pub type FixedHashSet<K> = HashSet<K, FixedBuildHasher>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::hash::BuildHasher;

    #[test]
    fn token_hash_keeps_the_trellis_multiply_rotate_law() {
        let build = FixedBuildHasher::default();
        for token in [0, 1, 255, 0x0100_0001, u32::MAX] {
            assert_eq!(
                build.hash_one(token),
                u64::from(token).wrapping_mul(0x517c_c1b7_2722_0a95)
            );
        }
    }

    #[test]
    fn byte_hash_covers_each_word_and_tail() {
        for length in 0..40 {
            let bytes = vec![0xa5; length];
            let mut first = FixedHasher::default();
            first.write(&bytes);
            let mut second = FixedHasher::default();
            second.write(&bytes);
            assert_eq!(first.finish(), second.finish());
            for index in 0..length {
                let mut changed = bytes.clone();
                changed[index] ^= 1;
                let mut other = FixedHasher::default();
                other.write(&changed);
                assert_ne!(first.finish(), other.finish());
            }
        }
    }

    #[test]
    fn owned_and_borrowed_strings_use_the_same_lookup() {
        let mut table = FixedHashMap::default();
        for text in ["", "nihao", "你好", "123456789abcdef"] {
            table.insert(text.to_owned(), text.len());
            assert_eq!(table.get(text), Some(&text.len()));
        }
        let mut membership = FixedHashSet::default();
        assert!(membership.insert("nihao".to_owned()));
        assert!(membership.contains("nihao"));
        assert!(!membership.insert("nihao".to_owned()));
    }
}
