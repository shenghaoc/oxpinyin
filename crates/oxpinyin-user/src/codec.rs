//! Byte codec for user-store key/value types.
//!
//! Every type that appears in a user-store table definition has an `encode_*`
//! / `decode_*` pair here.  Integers are big-endian so that `memcmp` on the
//! encoded bytes reproduces redb's `Key::compare` (which decodes and compares
//! values, not stored LE bytes).  Composite keys concatenate their
//! fixed-width prefix with the variable tail — no length prefix needed
//! because the fixed half exactly delimits the split point.

use crate::store::Token;

/// Error returned when stored codec bytes cannot be decoded.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum DecodeError {
    /// The input did not contain the required fixed-width prefix.
    Truncated {
        /// Required byte count.
        expected: usize,
        /// Actual byte count.
        actual: usize,
    },
    /// A string payload was not valid UTF-8.
    InvalidUtf8,
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Truncated { expected, actual } => {
                write!(
                    f,
                    "truncated codec value: expected {expected} bytes, got {actual}"
                )
            }
            Self::InvalidUtf8 => f.write_str("codec string is not valid UTF-8"),
        }
    }
}

impl std::error::Error for DecodeError {}

fn fixed_prefix<const N: usize>(bytes: &[u8]) -> Result<[u8; N], DecodeError> {
    let actual = bytes.len();
    bytes
        .get(..N)
        .and_then(|prefix| prefix.try_into().ok())
        .ok_or(DecodeError::Truncated {
            expected: N,
            actual,
        })
}

// ── scalars ────────────────────────────────────────────────────────

/// Encode a `u8` value.
pub fn encode_u8(v: u8) -> [u8; 1] {
    [v]
}

/// Decode a `u8` value.
pub fn decode_u8(bytes: &[u8]) -> Result<u8, DecodeError> {
    Ok(fixed_prefix::<1>(bytes)?[0])
}

/// Encode a `Token` (`u32`) as 4 big-endian bytes.
pub fn encode_token(token: Token) -> [u8; 4] {
    token.to_be_bytes()
}

/// Decode a `Token` (`u32`) from 4 big-endian bytes.
pub fn decode_token(bytes: &[u8]) -> Result<Token, DecodeError> {
    Ok(Token::from_be_bytes(fixed_prefix::<4>(bytes)?))
}

/// Encode a `u64` value as 8 big-endian bytes.
pub fn encode_u64(v: u64) -> [u8; 8] {
    v.to_be_bytes()
}

/// Decode a `u64` value from 8 big-endian bytes.
pub fn decode_u64(bytes: &[u8]) -> Result<u64, DecodeError> {
    Ok(u64::from_be_bytes(fixed_prefix::<8>(bytes)?))
}

// ── identity types ─────────────────────────────────────────────────

/// Encode a `&str` as raw UTF-8 bytes (identity).
pub fn encode_str(s: &str) -> &[u8] {
    s.as_bytes()
}

/// Decode a `&str` from raw UTF-8 bytes (identity).
pub fn decode_str(bytes: &[u8]) -> Result<&str, DecodeError> {
    std::str::from_utf8(bytes).map_err(|_| DecodeError::InvalidUtf8)
}

/// Encode `&[u8]` (identity — returns the same slice).
pub fn encode_bytes(b: &[u8]) -> &[u8] {
    b
}

/// Decode `&[u8]` (identity — returns the same slice).
pub fn decode_bytes(b: &[u8]) -> &[u8] {
    b
}

// ── composite keys ─────────────────────────────────────────────────

/// Encode a `(Token, Token)` key as 8 big-endian bytes.
pub fn encode_token_pair(a: Token, b: Token) -> [u8; 8] {
    let mut out = [0u8; 8];
    out[..4].copy_from_slice(&a.to_be_bytes());
    out[4..].copy_from_slice(&b.to_be_bytes());
    out
}

/// Decode a `(Token, Token)` key from 8 big-endian bytes.
pub fn decode_token_pair(bytes: &[u8]) -> Result<(Token, Token), DecodeError> {
    let [a0, a1, a2, a3, b0, b1, b2, b3] = fixed_prefix::<8>(bytes)?;
    Ok((
        Token::from_be_bytes([a0, a1, a2, a3]),
        Token::from_be_bytes([b0, b1, b2, b3]),
    ))
}

/// Encode a `(u8, &str)` key: one byte prefix, then raw UTF-8.
pub fn encode_u8_str(v: u8, s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + s.len());
    out.push(v);
    out.extend_from_slice(s.as_bytes());
    out
}

/// Decode a `(u8, &str)` key: one byte prefix, then raw UTF-8.
pub fn decode_u8_str(bytes: &[u8]) -> Result<(u8, &str), DecodeError> {
    let (&v, tail) = bytes.split_first().ok_or(DecodeError::Truncated {
        expected: 1,
        actual: 0,
    })?;
    let s = std::str::from_utf8(tail).map_err(|_| DecodeError::InvalidUtf8)?;
    Ok((v, s))
}

/// Encode a `(Token, &[u8])` key: 4 BE bytes, then raw tail.
pub fn encode_token_bytes(token: Token, tail: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + tail.len());
    out.extend_from_slice(&token.to_be_bytes());
    out.extend_from_slice(tail);
    out
}

/// Decode a `(Token, &[u8])` key: 4 BE bytes, then raw tail.
pub fn decode_token_bytes(bytes: &[u8]) -> Result<(Token, &[u8]), DecodeError> {
    let token = Token::from_be_bytes(fixed_prefix::<4>(bytes)?);
    Ok((token, &bytes[4..]))
}

// ── tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── fixed-width unit tests ─────────────────────────────────────

    #[test]
    fn u8_roundtrip() {
        for v in [0, 1, 127, 255] {
            assert_eq!(decode_u8(&encode_u8(v)).unwrap(), v);
        }
    }

    #[test]
    fn token_roundtrip_fixed() {
        for v in [0, 1, 0x0700_0001, Token::MAX] {
            assert_eq!(decode_token(&encode_token(v)).unwrap(), v);
        }
    }

    #[test]
    fn token_encoding_is_big_endian() {
        assert_eq!(encode_token(0x0102_0304), [0x01, 0x02, 0x03, 0x04]);
    }

    #[test]
    fn u64_roundtrip_fixed() {
        for v in [0, 1, 69, u64::MAX] {
            assert_eq!(decode_u64(&encode_u64(v)).unwrap(), v);
        }
    }

    #[test]
    fn u64_encoding_is_big_endian() {
        assert_eq!(
            encode_u64(0x0102_0304_0506_0708),
            [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08]
        );
    }

    #[test]
    fn str_roundtrip_fixed() {
        for s in ["", "hello", "你好", "ni'hao"] {
            assert_eq!(decode_str(encode_str(s)).unwrap(), s);
        }
    }

    #[test]
    fn bytes_roundtrip_fixed() {
        let cases: &[&[u8]] = &[&[], &[0], &[1, 2, 3], &[0xff; 16]];
        for b in cases {
            assert_eq!(decode_bytes(encode_bytes(b)), *b);
        }
    }

    #[test]
    fn token_pair_roundtrip_fixed() {
        let cases = [(0, 0), (1, 2), (Token::MAX, 0), (0, Token::MAX)];
        for (a, b) in cases {
            assert_eq!(decode_token_pair(&encode_token_pair(a, b)).unwrap(), (a, b));
        }
    }

    #[test]
    fn token_pair_encoding_layout() {
        let buf = encode_token_pair(0x0000_0001, 0x0000_0002);
        assert_eq!(buf, [0, 0, 0, 1, 0, 0, 0, 2]);
    }

    #[test]
    fn u8_str_roundtrip_fixed() {
        let cases = [(0u8, ""), (7, "你好"), (255, "ni'hao")];
        for (v, s) in cases {
            assert_eq!(decode_u8_str(&encode_u8_str(v, s)).unwrap(), (v, s));
        }
    }

    #[test]
    fn token_bytes_roundtrip_fixed() {
        let cases: &[(Token, &[u8])] = &[(0, &[]), (1, &[0xff]), (Token::MAX, &[1, 2, 3])];
        for &(token, tail) in cases {
            assert_eq!(
                decode_token_bytes(&encode_token_bytes(token, tail)).unwrap(),
                (token, tail)
            );
        }
    }

    // ── malformed-input validation ─────────────────────────────────

    #[test]
    fn decode_u8_empty() {
        assert!(matches!(
            decode_u8(&[]),
            Err(DecodeError::Truncated {
                expected: 1,
                actual: 0
            })
        ));
    }

    #[test]
    fn decode_token_too_short() {
        for bytes in [&[][..], &[0, 1], &[0, 1, 2]] {
            assert!(matches!(
                decode_token(bytes),
                Err(DecodeError::Truncated { expected: 4, .. })
            ));
        }
    }

    #[test]
    fn decode_u64_too_short() {
        for bytes in [&[][..], &[0; 7]] {
            assert!(matches!(
                decode_u64(bytes),
                Err(DecodeError::Truncated { expected: 8, .. })
            ));
        }
    }

    #[test]
    fn decode_str_invalid_utf8() {
        for bytes in [&[0xff][..], &[0xc0, 0x80]] {
            assert_eq!(decode_str(bytes), Err(DecodeError::InvalidUtf8));
        }
    }

    #[test]
    fn decode_token_pair_too_short() {
        for bytes in [&[][..], &[0; 4], &[0; 7]] {
            assert!(matches!(
                decode_token_pair(bytes),
                Err(DecodeError::Truncated { expected: 8, .. })
            ));
        }
    }

    #[test]
    fn decode_u8_str_rejects_empty_and_invalid_utf8() {
        assert!(matches!(
            decode_u8_str(&[]),
            Err(DecodeError::Truncated {
                expected: 1,
                actual: 0
            })
        ));
        assert_eq!(decode_u8_str(&[7, 0xff]), Err(DecodeError::InvalidUtf8));
    }

    #[test]
    fn decode_token_bytes_too_short() {
        for bytes in [&[][..], &[0, 1, 2]] {
            assert!(matches!(
                decode_token_bytes(bytes),
                Err(DecodeError::Truncated { expected: 4, .. })
            ));
        }
    }

    // ── proptest round-trips ───────────────────────────────────────

    mod prop {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn u8_roundtrip(v: u8) {
                prop_assert_eq!(decode_u8(&encode_u8(v)).unwrap(), v);
            }

            #[test]
            fn token_roundtrip(v: u32) {
                prop_assert_eq!(decode_token(&encode_token(v)).unwrap(), v);
            }

            #[test]
            fn u64_roundtrip(v: u64) {
                prop_assert_eq!(decode_u64(&encode_u64(v)).unwrap(), v);
            }

            #[test]
            fn str_roundtrip(s in "\\PC{0,64}") {
                prop_assert_eq!(decode_str(encode_str(&s)).unwrap(), s.as_str());
            }

            #[test]
            fn bytes_roundtrip(b in proptest::collection::vec(any::<u8>(), 0..64)) {
                prop_assert_eq!(decode_bytes(encode_bytes(&b)), b.as_slice());
            }

            #[test]
            fn token_pair_roundtrip(a: u32, b: u32) {
                prop_assert_eq!(decode_token_pair(&encode_token_pair(a, b)).unwrap(), (a, b));
            }

            #[test]
            fn u8_str_roundtrip(v: u8, s in "\\PC{0,64}") {
                let enc = encode_u8_str(v, &s);
                let (dv, ds) = decode_u8_str(&enc).unwrap();
                prop_assert_eq!(dv, v);
                prop_assert_eq!(ds, s.as_str());
            }

            #[test]
            fn token_bytes_roundtrip(
                token: u32,
                tail in proptest::collection::vec(any::<u8>(), 0..64),
            ) {
                let enc = encode_token_bytes(token, &tail);
                let (dec_t, dec_b) = decode_token_bytes(&enc).unwrap();
                prop_assert_eq!(dec_t, token);
                prop_assert_eq!(dec_b, tail.as_slice());
            }
        }
    }

    // ── order-equivalence tests ────────────────────────────────────

    mod order {
        use super::*;
        use proptest::prelude::*;
        use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};

        /// Insert `keys` into a redb typed table, read them back in walk
        /// order, encode each with `enc`, and verify that the encoded
        /// sequence is sorted by `[u8]::cmp` (memcmp).
        fn assert_order_matches_redb<K, E>(
            table_name: &str,
            keys: &[K],
            insert: impl Fn(&Database, &str, &[K]),
            walk: impl Fn(&Database, &str) -> Vec<K>,
            enc: impl Fn(&K) -> E,
        ) where
            E: AsRef<[u8]>,
        {
            let path = std::env::temp_dir().join(format!(
                "oxpinyin-codec-order-{table_name}-{}.redb",
                std::process::id(),
            ));
            let _ = std::fs::remove_file(&path);
            let db = Database::create(&path).unwrap();
            insert(&db, table_name, keys);
            let walked = walk(&db, table_name);
            drop(db);
            let _ = std::fs::remove_file(&path);

            let encoded: Vec<Vec<u8>> = walked.iter().map(|k| enc(k).as_ref().to_vec()).collect();
            for pair in encoded.windows(2) {
                assert!(
                    pair[0] <= pair[1],
                    "encoded order violated: {:?} > {:?}",
                    pair[0],
                    pair[1],
                );
            }
        }

        // ── (Token, Token) ────────────────────────────────────────

        fn insert_token_pairs(db: &Database, name: &str, keys: &[(Token, Token)]) {
            let def: TableDefinition<(Token, Token), u64> = TableDefinition::new(name);
            let txn = db.begin_write().unwrap();
            {
                let mut table = txn.open_table(def).unwrap();
                for &(a, b) in keys {
                    table.insert((a, b), 0u64).unwrap();
                }
            }
            txn.commit().unwrap();
        }

        fn walk_token_pairs(db: &Database, name: &str) -> Vec<(Token, Token)> {
            let def: TableDefinition<(Token, Token), u64> = TableDefinition::new(name);
            let txn = db.begin_read().unwrap();
            let table = txn.open_table(def).unwrap();
            table
                .iter()
                .unwrap()
                .map(|item| {
                    let (k, _) = item.unwrap();
                    k.value()
                })
                .collect()
        }

        proptest! {
            #[test]
            fn token_pair_order(
                keys in proptest::collection::vec((any::<u32>(), any::<u32>()), 2..32)
            ) {
                assert_order_matches_redb(
                    "test_bigram",
                    &keys,
                    insert_token_pairs,
                    walk_token_pairs,
                    |&(a, b)| encode_token_pair(a, b),
                );
            }
        }

        // ── (Token, &[u8]) ────────────────────────────────────────

        fn insert_token_bytes_keys(db: &Database, name: &str, keys: &[(Token, Vec<u8>)]) {
            let def: TableDefinition<(Token, &[u8]), u64> = TableDefinition::new(name);
            let txn = db.begin_write().unwrap();
            {
                let mut table = txn.open_table(def).unwrap();
                for (token, tail) in keys {
                    table.insert((*token, tail.as_slice()), 0u64).unwrap();
                }
            }
            txn.commit().unwrap();
        }

        fn walk_token_bytes_keys(db: &Database, name: &str) -> Vec<(Token, Vec<u8>)> {
            let def: TableDefinition<(Token, &[u8]), u64> = TableDefinition::new(name);
            let txn = db.begin_read().unwrap();
            let table = txn.open_table(def).unwrap();
            table
                .iter()
                .unwrap()
                .map(|item| {
                    let (k, _) = item.unwrap();
                    let (token, bytes) = k.value();
                    (token, bytes.to_vec())
                })
                .collect()
        }

        proptest! {
            #[test]
            fn token_bytes_order(
                keys in proptest::collection::vec(
                    (any::<u32>(), proptest::collection::vec(any::<u8>(), 0..16)),
                    2..32,
                )
            ) {
                assert_order_matches_redb(
                    "test_pronunciation",
                    &keys,
                    insert_token_bytes_keys,
                    walk_token_bytes_keys,
                    |(token, tail)| encode_token_bytes(*token, tail),
                );
            }
        }
    }
}
