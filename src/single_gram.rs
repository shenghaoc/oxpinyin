//! Raw bigram operations on a predecessor's row. A row is a four-byte little-endian
//! total, then eight-byte records. Each record is a four-byte little-endian token
//! followed by a four-byte little-endian frequency. Searches that start at
//! [`lower_bound`] need whole records sorted by token. Updates keep the stored order.

// SingleGram, src/storage/ngram.cpp. The pin asserts on a short total at
// memory_chunk.h:390 and can read past a partial record (compatibility rows 85, 98 and
// 100).

/// Bytes of the `guint32` `total_freq` header.
pub(crate) const TOTAL_LEN: usize = 4;

/// Bytes of one `SingleGramItem`.
pub(crate) const ITEM_LEN: usize = 8;

/// `SingleGram::get_length`: the number of whole items after the total.
#[must_use]
pub(crate) const fn get_length(gram: &[u8]) -> usize {
    gram.len().saturating_sub(TOTAL_LEN) / ITEM_LEN
}

fn word(gram: &[u8], offset: usize) -> u32 {
    let mut bytes = [0_u8; 4];
    if let Some(slice) = gram.get(offset..offset.saturating_add(4)) {
        bytes.copy_from_slice(slice);
    }
    u32::from_le_bytes(bytes)
}

fn put_word(gram: &mut [u8], offset: usize, value: u32) {
    if let Some(slice) = gram.get_mut(offset..offset.saturating_add(4)) {
        slice.copy_from_slice(&value.to_le_bytes());
    }
}

/// Borrow whole records without decoding the row or discarding a partial tail.
fn records(gram: &[u8]) -> Option<(&[[u8; ITEM_LEN]], &[u8])> {
    Some(gram.get(TOTAL_LEN..)?.as_chunks())
}

/// The whole records, none for a value shorter than its total.
fn whole_records(gram: &[u8]) -> &[[u8; ITEM_LEN]] {
    records(gram).map_or(&[], |(records, _)| records)
}

fn record_token(record: &[u8; ITEM_LEN]) -> u32 {
    u32::from_le_bytes([record[0], record[1], record[2], record[3]])
}

fn record_freq(record: &[u8; ITEM_LEN]) -> u32 {
    u32::from_le_bytes([record[4], record[5], record[6], record[7]])
}

/// `SingleGram::get_total_freq`; `None` for a value shorter than the
/// total, where the pin asserts (`memory_chunk.h:390`).
#[must_use]
pub fn get_total_freq(gram: &[u8]) -> Option<u32> {
    (gram.len() >= TOTAL_LEN).then(|| word(gram, 0))
}

/// `SingleGram::set_total_freq`: `MemoryChunk::set_content` at offset 0,
/// which grows a shorter chunk to hold the total.
pub(crate) fn set_total_freq(gram: &mut Vec<u8>, total: u32) {
    if gram.len() < TOTAL_LEN {
        gram.resize(TOTAL_LEN, 0);
    }
    put_word(gram, 0, total);
}

/// Partition the stored items by `item.token < token`, returning the item
/// index. On unpartitioned input the result is unspecified, as for
/// [`slice::partition_point`]; the stored bytes are never reordered.
#[must_use]
pub(crate) fn lower_bound(gram: &[u8], token: u32) -> usize {
    let Some((records, _)) = records(gram) else {
        return 0;
    };
    lower_bound_records(records, token)
}

fn lower_bound_records(records: &[[u8; ITEM_LEN]], token: u32) -> usize {
    records.partition_point(|record| record_token(record) < token)
}

/// The walk `get_freq` and `set_freq` share: from
/// [`lower_bound`], stop at the first item above `token` (a miss) or the
/// first equal one (a hit).
fn find(gram: &[u8], token: u32) -> Option<usize> {
    let records = whole_records(gram);
    let start = lower_bound_records(records, token);
    let index = start
        + records[start..]
            .iter()
            .position(|record| record_token(record) >= token)?;
    (record_token(&records[index]) == token).then_some(index)
}

/// `SingleGram::get_freq` (`ngram.cpp:231-250`): the first match walking
/// from [`lower_bound`].
#[must_use]
pub(crate) fn get_freq(gram: &[u8], token: u32) -> Option<u32> {
    find(gram, token).map(|index| record_freq(&whole_records(gram)[index]))
}

/// Where the pin's own read of a malformed value stops being defined.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GramFault {
    /// `get_total_freq` on a value shorter than its total: the pin asserts
    /// (`memory_chunk.h:390`); the C adapters abort.
    Short,
    /// A walk that leaves the value: a partial item after the last whole
    /// one, or a value shorter than its total, where `cur_item != end`
    /// never meets `end`.
    OverRead,
}

/// Whether a value with a whole total ends in a partial item.
#[must_use]
pub(crate) const fn has_partial_tail(gram: &[u8]) -> bool {
    gram.len() >= TOTAL_LEN && !(gram.len() - TOTAL_LEN).is_multiple_of(ITEM_LEN)
}

/// `get_freq` where the pin's walk can leave the value
/// (`ngram.cpp:231-250`): `lower_bound` over the whole items, then
/// `cur_item != end`, which an `end` off the item grid never meets. A
/// walk that returns before the last whole item is the pin's answer; one
/// that runs past it, or starts past a value shorter than its total
/// (`begin` four bytes in, beyond `end`), is `GramFault::OverRead`.
///
/// # Errors
/// `GramFault::OverRead` where the pin reads past the value.
pub fn checked_get_freq(gram: &[u8], token: u32) -> Result<Option<u32>, GramFault> {
    let Some((records, tail)) = records(gram) else {
        return Err(GramFault::OverRead);
    };
    for record in &records[lower_bound_records(records, token)..] {
        let item = record_token(record);
        if item > token {
            return Ok(None);
        }
        if item == token {
            return Ok(Some(record_freq(record)));
        }
    }
    if !tail.is_empty() {
        return Err(GramFault::OverRead);
    }
    Ok(None)
}

// SingleGram::search, ngram.cpp:153–176. lower_bound precedes get_total_freq and the
// range walk.
/// Appends records with tokens in `begin..end` to `out`, including duplicate tokens.
/// Supply a total header followed by whole records sorted by token. Existing entries in
/// `out` are retained.
///
/// # Errors
///
/// A header read or a record read can fail with [`GramFault`]. Entries already appended
/// remain in `out` on error.
pub(crate) fn checked_search(
    gram: &[u8],
    begin: u32,
    end: u32,
    out: &mut Vec<(u32, u32)>,
) -> Result<(), GramFault> {
    let Some((records, tail)) = records(gram) else {
        return Err(GramFault::Short);
    };
    for record in records.iter().skip(lower_bound_records(records, begin)) {
        let item = record_token(record);
        let freq = record_freq(record);
        if item >= end {
            return Ok(());
        }
        out.push((item, freq));
    }
    // A partial tail faults only when the physical walk reaches it. An
    // earlier range-end stop still succeeds, including on unsorted rows.
    if !tail.is_empty() {
        return Err(GramFault::OverRead);
    }
    Ok(())
}

// SingleGram::set_freq, ngram.cpp:252–271.
/// Replaces the first matching token's frequency and leaves the row total unchanged.
/// Returns `false` when the sorted row has no such token.
pub(crate) fn set_freq(gram: &mut [u8], token: u32, freq: u32) -> bool {
    let Some(index) = find(gram, token) else {
        return false;
    };
    put_word(gram, TOTAL_LEN + index * ITEM_LEN + 4, freq);
    true
}

// SingleGram::insert_freq, ngram.cpp:178–205.
/// Inserts a token and frequency into a sorted row and leaves the row total unchanged. An
/// existing token is refused.
pub(crate) fn insert_freq(gram: &mut Vec<u8>, token: u32, freq: u32) -> bool {
    let mut record = [0_u8; ITEM_LEN];
    record[..4].copy_from_slice(&token.to_le_bytes());
    record[4..].copy_from_slice(&freq.to_le_bytes());
    let records = whole_records(gram);
    let start = lower_bound_records(records, token);
    match records[start..]
        .iter()
        .position(|stored| record_token(stored) >= token)
    {
        Some(found) if record_token(&records[start + found]) == token => false,
        Some(found) => {
            let offset = TOTAL_LEN + (start + found) * ITEM_LEN;
            gram.splice(offset..offset, record);
            true
        }
        None => {
            // `insert_content(m_chunk.size(), …)`, after any partial tail.
            gram.extend_from_slice(&record);
            true
        }
    }
}

// SingleGram::mask_out, ngram.cpp:76–103.
/// Removes records for which `(token & mask) == value` and returns the number removed.
/// Each removed frequency is subtracted from the stored total using 32-bit wrapping arithmetic.
pub(crate) fn mask_out(gram: &mut Vec<u8>, mask: u32, value: u32) -> u32 {
    let Some(mut total) = get_total_freq(gram) else {
        return 0;
    };
    let mut removed = 0_u32;
    crate::packed_records::retain_records(gram, TOTAL_LEN, ITEM_LEN, |record| {
        if word(record, 0) & mask != value {
            return true;
        }
        total = total.wrapping_sub(word(record, 4));
        removed = removed.wrapping_add(1);
        false
    });
    set_total_freq(gram, total);
    removed
}

/// `SingleGram::retrieve_all` (`ngram.cpp:133-151`): every whole item in
/// stored order, duplicates included, as `(token, freq)`.
pub fn retrieve_all(gram: &[u8]) -> impl Iterator<Item = (u32, u32)> + '_ {
    whole_records(gram)
        .iter()
        .map(|record| (record_token(record), record_freq(record)))
}

/// A fresh `SingleGram`: the four zero bytes of an empty total
/// (`ngram.cpp:36-39`).
#[must_use]
pub(crate) fn new_gram() -> Vec<u8> {
    vec![0; TOTAL_LEN]
}

/// Checks the reads of a two-sided merge. The pin reads both totals
/// before walking the records (`ngram.cpp:303-349`). A short total gives
/// [`GramFault::Short`]. A partial record gives [`GramFault::OverRead`].
pub(crate) fn merge_fault(system: &[u8], user: &[u8]) -> Option<GramFault> {
    let short = |gram: &[u8]| get_total_freq(gram).is_none();
    if short(system) || short(user) {
        Some(GramFault::Short)
    } else if has_partial_tail(system) || has_partial_tail(user) {
        Some(GramFault::OverRead)
    } else {
        None
    }
}

/// The chunk `merge_single_gram` (`ngram.cpp:277-352`) builds from two
/// present, readable grams ([`merge_fault`] answered `None`): the wrapped
/// sum of the totals, then the two item arrays merged linearly, equal
/// tokens summed under the system token.
pub(crate) fn merged_chunk(system: &[u8], user: &[u8]) -> Vec<u8> {
    let (system_records, user_records) = (whole_records(system), whole_records(user));
    let total = get_total_freq(system)
        .unwrap_or(0)
        .wrapping_add(get_total_freq(user).unwrap_or(0));
    let mut chunk =
        Vec::with_capacity(TOTAL_LEN + ITEM_LEN * (system_records.len() + user_records.len()));
    chunk.extend_from_slice(&total.to_le_bytes());
    let (mut left, mut right) = (0, 0);
    while let (Some(system_record), Some(user_record)) =
        (system_records.get(left), user_records.get(right))
    {
        let (system_token, user_token) = (record_token(system_record), record_token(user_record));
        match system_token.cmp(&user_token) {
            std::cmp::Ordering::Less => {
                chunk.extend_from_slice(system_record);
                left += 1;
            }
            std::cmp::Ordering::Greater => {
                chunk.extend_from_slice(user_record);
                right += 1;
            }
            std::cmp::Ordering::Equal => {
                let freq = record_freq(system_record).wrapping_add(record_freq(user_record));
                chunk.extend_from_slice(&system_token.to_le_bytes());
                chunk.extend_from_slice(&freq.to_le_bytes());
                left += 1;
                right += 1;
            }
        }
    }
    chunk.extend_from_slice(system_records[left..].as_flattened());
    chunk.extend_from_slice(user_records[right..].as_flattened());
    chunk
}

/// One successor record: `(next token, count)`.
pub type SingleGramRecord = (u32, u32);

/// Serialises `total` and `records` into the `SingleGram` layout.
///
/// Emits `records` in the supplied order. Callers constructing a searchable
/// row supply records in ascending token order.
#[must_use]
pub fn encode_single_gram(total: u32, records: &[SingleGramRecord]) -> Vec<u8> {
    let mut value = Vec::with_capacity(TOTAL_LEN + records.len() * ITEM_LEN);
    value.extend_from_slice(&total.to_le_bytes());
    for (next, count) in records {
        value.extend_from_slice(&next.to_le_bytes());
        value.extend_from_slice(&count.to_le_bytes());
    }
    value
}

pub(crate) use layout::is_complete;

pub(crate) mod layout {
    use super::{ITEM_LEN, TOTAL_LEN};

    /// Whether the value holds a total and only whole records.
    #[must_use]
    pub const fn is_complete(gram: &[u8]) -> bool {
        gram.len() >= TOTAL_LEN && (gram.len() - TOTAL_LEN).is_multiple_of(ITEM_LEN)
    }
}

#[cfg(test)]
mod codec_tests;

#[cfg(test)]
mod tests;
