//! Raw bigram operations on a predecessor’s row. A row is a four-byte little-endian
//! total, then eight-byte records. Each record is a four-byte little-endian token
//! followed by a four-byte little-endian frequency. Searches that start at
//! [`lower_bound`] need whole records sorted by token. Updates keep the stored order.

// SingleGram, src/storage/ngram.cpp. The pin asserts on a short total at
// memory_chunk.h:390 and can read past a partial record (compatibility rows 85, 98 and
// 100).

/// Bytes of the `guint32` `total_freq` header.
pub const TOTAL_LEN: usize = 4;

/// Bytes of one `SingleGramItem`.
pub const ITEM_LEN: usize = 8;

/// `SingleGram::get_length`: the number of whole items after the total.
#[must_use]
pub const fn get_length(gram: &[u8]) -> usize {
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

/// The byte offset of item `index`; `None` when it overflows.
const fn item_offset(index: usize) -> Option<usize> {
    match index.checked_mul(ITEM_LEN) {
        Some(bytes) => bytes.checked_add(TOTAL_LEN),
        None => None,
    }
}

/// The token of item `index`; `None` past the last whole item.
fn item_token(gram: &[u8], index: usize) -> Option<u32> {
    (index < get_length(gram)).then(|| item_offset(index).map(|offset| word(gram, offset)))?
}

/// The frequency of item `index`; `None` past the last whole item.
fn item_freq(gram: &[u8], index: usize) -> Option<u32> {
    let offset = item_offset(index)?.checked_add(4)?;
    (index < get_length(gram)).then(|| word(gram, offset))
}

/// Borrow whole records without decoding the row or discarding a partial tail.
fn records(gram: &[u8]) -> Option<(&[[u8; ITEM_LEN]], &[u8])> {
    Some(gram.get(TOTAL_LEN..)?.as_chunks())
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
pub fn lower_bound(gram: &[u8], token: u32) -> usize {
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
    for index in lower_bound(gram, token)..get_length(gram) {
        let item = item_token(gram, index)?;
        if item > token {
            return None;
        }
        if item == token {
            return Some(index);
        }
    }
    None
}

/// `SingleGram::get_freq` (`ngram.cpp:231-250`): the first match walking
/// from [`lower_bound`].
#[must_use]
pub fn get_freq(gram: &[u8], token: u32) -> Option<u32> {
    find(gram, token).and_then(|index| item_freq(gram, index))
}

/// Where the pin's own read of a malformed value stops being defined.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GramFault {
    /// `get_total_freq` on a value shorter than its total: the pin asserts
    /// (`memory_chunk.h:390`; register row 85); the C adapters abort.
    Short,
    /// A walk that leaves the value: a partial item after the last whole
    /// one, or a value shorter than its total, where `cur_item != end`
    /// never meets `end` (register row 98, class (b)).
    OverRead,
}

/// Whether a value with a whole total ends in a partial item.
#[must_use]
pub(crate) const fn has_partial_tail(gram: &[u8]) -> bool {
    gram.len() >= TOTAL_LEN && !(gram.len() - TOTAL_LEN).is_multiple_of(ITEM_LEN)
}

/// [`get_freq`] where the pin's walk can leave the value
/// (`ngram.cpp:231-250`): `lower_bound` over the whole items, then
/// `cur_item != end`, which an `end` off the item grid never meets. A
/// walk that returns before the last whole item is the pin's answer; one
/// that runs past it, or starts past a value shorter than its total
/// (`begin` four bytes in, beyond `end`), is [`GramFault::OverRead`].
///
/// # Errors
/// [`GramFault::OverRead`] where the pin reads past the value.
pub fn checked_get_freq(gram: &[u8], token: u32) -> Result<Option<u32>, GramFault> {
    if gram.len() < TOTAL_LEN {
        return Err(GramFault::OverRead);
    }
    for index in lower_bound(gram, token)..get_length(gram) {
        let (Some(item), Some(freq)) = (item_token(gram, index), item_freq(gram, index)) else {
            return Err(GramFault::OverRead);
        };
        if item > token {
            return Ok(None);
        }
        if item == token {
            return Ok(Some(freq));
        }
    }
    if has_partial_tail(gram) {
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
pub fn checked_search(
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
/// Replaces the first matching token’s frequency and leaves the row total unchanged.
/// Returns `false` when the sorted row has no such token.
pub fn set_freq(gram: &mut [u8], token: u32, freq: u32) -> bool {
    let Some(offset) = find(gram, token)
        .and_then(item_offset)
        .and_then(|offset| offset.checked_add(4))
    else {
        return false;
    };
    put_word(gram, offset, freq);
    true
}

// SingleGram::insert_freq, ngram.cpp:178–205.
/// Inserts a token and frequency into a sorted row and leaves the row total unchanged. An
/// existing token is refused.
pub fn insert_freq(gram: &mut Vec<u8>, token: u32, freq: u32) -> bool {
    let mut record = [0_u8; ITEM_LEN];
    record[..4].copy_from_slice(&token.to_le_bytes());
    record[4..].copy_from_slice(&freq.to_le_bytes());
    for index in lower_bound(gram, token)..get_length(gram) {
        let Some(item) = item_token(gram, index) else {
            return false;
        };
        if item > token {
            let Some(offset) = item_offset(index) else {
                return false;
            };
            gram.splice(offset..offset, record);
            return true;
        }
        if item == token {
            return false;
        }
    }
    // `insert_content(m_chunk.size(), …)`.
    gram.extend_from_slice(&record);
    true
}

// SingleGram::mask_out, ngram.cpp:76–103.
/// Removes records for which `(token & mask) == value` and returns the number removed.
/// Each removed frequency is subtracted from the stored total using 32-bit wrapping arithmetic.
pub fn mask_out(gram: &mut Vec<u8>, mask: u32, value: u32) -> u32 {
    let Some(mut total) = get_total_freq(gram) else {
        return 0;
    };
    let mut removed = 0_u32;
    let mut index = 0;
    while index < get_length(gram) {
        let (Some(item), Some(freq), Some(offset)) = (
            item_token(gram, index),
            item_freq(gram, index),
            item_offset(index),
        ) else {
            break;
        };
        if item & mask != value {
            index += 1;
            continue;
        }
        total = total.wrapping_sub(freq);
        gram.drain(offset..offset.saturating_add(ITEM_LEN));
        removed = removed.wrapping_add(1);
    }
    set_total_freq(gram, total);
    removed
}

/// `SingleGram::retrieve_all` (`ngram.cpp:133-151`): every whole item in
/// stored order, duplicates included, as `(token, freq)`.
pub fn retrieve_all(gram: &[u8]) -> impl Iterator<Item = (u32, u32)> + '_ {
    (0..get_length(gram))
        .map_while(|index| Some((item_token(gram, index)?, item_freq(gram, index)?)))
}

/// `merge_single_gram` (`ngram.cpp:277-352`) into `merged`: `false` and
/// `merged` untouched when both grams are absent; a copy of the present
/// one when the other is absent; otherwise the wrapped sum of the totals
/// and the two item arrays merged linearly, equal tokens summed.
pub fn merge_single_gram(merged: &mut Vec<u8>, system: Option<&[u8]>, user: Option<&[u8]>) -> bool {
    let (system, user) = match (system, user) {
        (None, None) => return false,
        (None, Some(only)) | (Some(only), None) => {
            merged.clear();
            merged.extend_from_slice(only);
            return true;
        }
        (Some(system), Some(user)) => (system, user),
    };
    let total = get_total_freq(system)
        .unwrap_or(0)
        .wrapping_add(get_total_freq(user).unwrap_or(0));
    merged.clear();
    merged.extend_from_slice(&total.to_le_bytes());
    let (system_len, user_len) = (get_length(system), get_length(user));
    let (mut left, mut right) = (0, 0);
    fn append(merged: &mut Vec<u8>, token: u32, freq: u32) {
        merged.extend_from_slice(&token.to_le_bytes());
        merged.extend_from_slice(&freq.to_le_bytes());
    }
    let item = |gram: &[u8], index: usize| -> (u32, u32) {
        (
            item_token(gram, index).unwrap_or(0),
            item_freq(gram, index).unwrap_or(0),
        )
    };
    while left < system_len && right < user_len {
        let ((system_token, system_freq), (user_token, user_freq)) =
            (item(system, left), item(user, right));
        if system_token < user_token {
            append(merged, system_token, system_freq);
            left += 1;
        } else if system_token > user_token {
            append(merged, user_token, user_freq);
            right += 1;
        } else {
            append(merged, system_token, system_freq.wrapping_add(user_freq));
            left += 1;
            right += 1;
        }
    }
    for index in left..system_len {
        let (token, freq) = item(system, index);
        append(merged, token, freq);
    }
    for index in right..user_len {
        let (token, freq) = item(user, index);
        append(merged, token, freq);
    }
    true
}

/// A fresh `SingleGram`: the four zero bytes of an empty total
/// (`ngram.cpp:36-39`).
#[must_use]
pub fn new_gram() -> Vec<u8> {
    vec![0; TOTAL_LEN]
}

#[cfg(test)]
mod tests;
