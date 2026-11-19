//! Encodes and decodes the raw bigram value for one previous token. A four-byte total
//! precedes the successor token and count pairs. The codecs preserve the supplied total
//! and record order. Callers that search by token must supply sorted records.

// SingleGram, ngram.cpp:133–205. The native writer maintains token order with
// lower_bound; a fresh gram has a zero total.

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
    for record in rest.as_chunks::<8>().0.iter() {
        let next = u32::from_le_bytes([record[0], record[1], record[2], record[3]]);
        let count = u32::from_le_bytes([record[4], record[5], record[6], record[7]]);
        records.push((next, count));
    }
    Ok((total, records))
}

#[cfg(test)]
mod tests;
