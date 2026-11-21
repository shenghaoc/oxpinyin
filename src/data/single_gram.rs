//! Encodes and decodes the raw bigram value for one previous token. A four-byte total
//! precedes the successor token and count pairs. The codecs preserve the supplied total
//! and record order. Callers that search by token must supply sorted records.

// SingleGram, ngram.cpp:133–205. The native writer maintains token order with
// lower_bound; a fresh gram has a zero total.

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
/// Returns `None` for a short total or a partial 8-byte record.
#[must_use]
pub fn decode_single_gram(bytes: &[u8]) -> Option<(u32, Vec<SingleGramRecord>)> {
    let (total, rest) = bytes.split_first_chunk::<4>()?;
    if !rest.len().is_multiple_of(8) {
        return None;
    }
    let mut records = Vec::with_capacity(rest.len() / 8);
    for [next, count] in rest.as_chunks::<4>().0.as_chunks::<2>().0 {
        records.push((u32::from_le_bytes(*next), u32::from_le_bytes(*count)));
    }
    Some((u32::from_le_bytes(*total), records))
}

#[cfg(test)]
mod tests;
