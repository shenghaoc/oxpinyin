//! The `SingleGram` blob — one `user_bigram.db` / `bigram.db` value.
//!
//! `ngram.cpp`'s `SingleGram` chunk: a native-endian `u32`
//! `total_freq`, then `{ phrase_token_t m_token, guint32 m_freq }`
//! records with no padding, kept sorted ascending by `m_token`
//! (`insert_freq`'s `lower_bound`). Both bigrams — the system's and the
//! user's — store this layout under the raw four bytes of the previous
//! token as the key; the readers in `lm`/`bigram_table` decode the system
//! half, and this module carries the one written copy both halves of the
//! user bigram share (load and save).
//!
//! A fresh gram upstream is a 4-byte chunk with `total_freq == 0`, and
//! `get_length` asserts a zero-item gram has a zero total; the encode
//! side therefore never emits records without a total that covers them
//! — callers keep `total == Σ freq` (training raises both by the seed).

/// A malformed `SingleGram` blob.
#[derive(Debug)]
pub struct SingleGramError(String);

impl std::fmt::Display for SingleGramError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "single gram error: {}", self.0)
    }
}

impl std::error::Error for SingleGramError {}

/// One successor record: `(next token, count)`.
pub type SingleGramRecord = (u32, u32);

/// Serialises `total` and `records` into the `SingleGram` layout.
///
/// `records` are emitted in the given order — callers sort ascending by
/// token first (upstream's `insert_freq` keeps the array sorted, so a
/// byte-faithful save must too).
#[must_use]
pub fn encode_single_gram(total: u32, records: &[SingleGramRecord]) -> Vec<u8> {
    let mut value = Vec::with_capacity(4 + records.len() * 8);
    value.extend_from_slice(&total.to_le_bytes());
    for (next, count) in records {
        value.extend_from_slice(&next.to_le_bytes());
        value.extend_from_slice(&count.to_le_bytes());
    }
    value
}

/// Decodes a `SingleGram` blob into `(total_freq, records)`.
///
/// # Errors
///
/// Fails on a blob shorter than the 4-byte total, or whose remainder is
/// not a whole number of 8-byte records.
pub fn decode_single_gram(bytes: &[u8]) -> Result<(u32, Vec<SingleGramRecord>), SingleGramError> {
    if bytes.len() < 4 {
        return Err(SingleGramError(format!(
            "blob of {} bytes has no total_freq",
            bytes.len()
        )));
    }
    let total = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let rest = &bytes[4..];
    if !rest.len().is_multiple_of(8) {
        return Err(SingleGramError(format!(
            "record area of {} bytes is not a whole number of records",
            rest.len()
        )));
    }
    let mut records = Vec::with_capacity(rest.len() / 8);
    for record in rest.chunks_exact(8) {
        let next = u32::from_le_bytes([record[0], record[1], record[2], record[3]]);
        let count = u32::from_le_bytes([record[4], record[5], record[6], record[7]]);
        records.push((next, count));
    }
    Ok((total, records))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_matches_the_layout() {
        let value = encode_single_gram(22080, &[(1, 69), (0x0100_0002, 138)]);
        assert_eq!(value.len(), 4 + 2 * 8);
        assert_eq!(&value[..4], &22080_u32.to_le_bytes());
        assert_eq!(&value[4..8], &1_u32.to_le_bytes());
        assert_eq!(&value[8..12], &69_u32.to_le_bytes());
        let (total, records) = decode_single_gram(&value).expect("decode");
        assert_eq!(total, 22080);
        assert_eq!(records, vec![(1, 69), (0x0100_0002, 138)]);
    }

    #[test]
    fn rejects_short_and_unaligned_blobs() {
        assert!(decode_single_gram(&[]).is_err());
        assert!(decode_single_gram(&[0; 3]).is_err());
        // A 5-byte blob: total present, one stray byte.
        assert!(decode_single_gram(&[0, 0, 0, 0, 0]).is_err());
        // The empty gram — a bare total — is valid.
        assert_eq!(
            decode_single_gram(&[7, 0, 0, 0]).expect("decode"),
            (7, vec![])
        );
    }
}
