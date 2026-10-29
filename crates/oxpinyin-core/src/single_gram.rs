//! libpinyin's `SingleGram` operations over one raw gram value
//! (`src/storage/ngram.cpp` at the pin `074a2219`).
//!
//! A gram is the value a bigram container stores under a predecessor: a
//! little-endian `guint32` `total_freq`, then packed eight-byte
//! `SingleGramItem { m_token, m_freq }` records. The pin never checks that
//! the records are sorted or unique. [`slice::partition_point`] finds
//! the first-not-less item on partitioned records. On
//! unpartitioned records std's answer is unspecified; stored order is
//! preserved rather than sorted or cleaned (2026-10-29 ruling, inventory C).
//!
//! The item count is `(len − 4) / 8`, the pin's `end − begin` over whole
//! items; every walk here stops at the last whole item. A value shorter
//! than the total, or with a partial item after it, is the caller's to
//! refuse first: the pin asserts on the first (`memory_chunk.h:390`) and
//! reads past the second (register rows 85, 98 and 100). Nothing here
//! panics on either.

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
pub fn set_total_freq(gram: &mut Vec<u8>, total: u32) {
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

/// The walk `get_freq`, `set_freq` and `remove_freq` share: from
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
    /// (`memory_chunk.h:390`; register row 85, class (c)).
    Short,
    /// A walk that leaves the value: a partial item after the last whole
    /// one, or a value shorter than its total, where `cur_item != end`
    /// never meets `end` (register row 98, class (b)).
    OverRead,
}

/// Whether a value with a whole total ends in a partial item.
#[must_use]
pub const fn has_partial_tail(gram: &[u8]) -> bool {
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

/// [`search`] into `out` where the pin's walk can leave the value
/// (`ngram.cpp:153-176`): `lower_bound(begin)` (`:163`, which reads
/// nothing on a value shorter than its total), then `get_total_freq`
/// (`:165`), then every item until the first at or past `end`
/// (`:167-173`), duplicates included.
///
/// # Errors
/// [`GramFault::Short`] where the total read asserts;
/// [`GramFault::OverRead`] where the walk runs past the last whole item
/// of a value with a partial tail.
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

/// `SingleGram::set_freq` (`ngram.cpp:252-271`): sets the first match
/// walking from [`lower_bound`]; `false` on a miss.
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

/// `SingleGram::insert_freq` (`ngram.cpp:178-205`): from [`lower_bound`],
/// inserts before the first item above `token`, refuses when an equal one
/// comes first, and otherwise appends at the chunk's end.
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

/// `SingleGram::remove_freq` (`ngram.cpp:207-229`): removes the first
/// match walking from [`lower_bound`] and answers its frequency.
pub fn remove_freq(gram: &mut Vec<u8>, token: u32) -> Option<u32> {
    let index = find(gram, token)?;
    let freq = item_freq(gram, index)?;
    let offset = item_offset(index)?;
    gram.drain(offset..offset.checked_add(ITEM_LEN)?);
    Some(freq)
}

/// `SingleGram::mask_out` (`ngram.cpp:76-103`): removes every item with
/// `(token & mask) == value`, subtracting each one's frequency from the
/// stored total with `guint32` wrap, and answers how many it removed. A
/// value shorter than the total removes nothing.
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

/// `SingleGram::search` (`ngram.cpp:153-176`): from [`lower_bound`] of
/// `begin`, every item until the first at or past `end`.
pub fn search(gram: &[u8], begin: u32, end: u32) -> impl Iterator<Item = (u32, u32)> + '_ {
    (lower_bound(gram, begin)..get_length(gram))
        .map_while(|index| Some((item_token(gram, index)?, item_freq(gram, index)?)))
        .take_while(move |&(token, _)| token < end)
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
mod tests {
    use proptest::prelude::*;

    use super::{
        GramFault, checked_get_freq, checked_search, get_freq, get_length, get_total_freq,
        insert_freq, item_freq, item_offset, item_token, lower_bound, mask_out, merge_single_gram,
        new_gram, remove_freq, retrieve_all, search, set_freq,
    };

    fn gram(total: u32, items: &[(u32, u32)]) -> Vec<u8> {
        let mut bytes = total.to_le_bytes().to_vec();
        for (token, freq) in items {
            bytes.extend_from_slice(&token.to_le_bytes());
            bytes.extend_from_slice(&freq.to_le_bytes());
        }
        bytes
    }

    fn items(bytes: &[u8]) -> Vec<(u32, u32)> {
        retrieve_all(bytes).collect()
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn partitioned_records_find_the_first_not_less(
            total in any::<u32>(),
            mut row in prop::collection::vec(
                (prop_oneof![0_u32..16, any::<u32>()], any::<u32>()), 0..128),
            begin in any::<u32>(),
            end in any::<u32>(),
            tail in any::<[u8; 7]>(),
        ) {
            row.sort_by_key(|&(token, _)| token);
            let first = row.iter().position(|&(token, _)| token >= begin).unwrap_or(row.len());
            for tail_len in 0..=tail.len() {
                let mut bytes = gram(total, &row);
                bytes.extend_from_slice(&tail[..tail_len]);
                prop_assert_eq!(lower_bound(&bytes, begin), first);
                let expected: Vec<_> = row[first..].iter().copied()
                    .take_while(|&(token, _)| token < end).collect();
                prop_assert_eq!(search(&bytes, begin, end).collect::<Vec<_>>(), expected.clone());
                let mut out = vec![(u32::MAX, 17)];
                let status = checked_search(&bytes, begin, end, &mut out);
                let reached_tail = first + expected.len() == row.len() && tail_len != 0;
                prop_assert_eq!(status, if reached_tail { Err(GramFault::OverRead) } else { Ok(()) });
                prop_assert_eq!(&out[1..], &expected);
                prop_assert_eq!(out[0], (u32::MAX, 17));
            }
        }

        #[test]
        fn unpartitioned_records_use_std_without_cleaning(
            total in any::<u32>(),
            row in prop::collection::vec((any::<u32>(), any::<u32>()), 0..128),
            probe in any::<u32>(),
            tail in any::<[u8; 7]>(),
        ) {
            let mut bytes = gram(total, &row);
            bytes.extend_from_slice(&tail);
            let before = bytes.clone();
            // Unpartitioned input has no promised search position. It must
            // still return an in-bounds suffix without changing storage.
            let first = lower_bound(&bytes, probe);
            prop_assert!(first <= row.len());
            let expected: Vec<_> = row[first..].iter().copied()
                .take_while(|&(token, _)| token < u32::MAX).collect();
            prop_assert_eq!(search(&bytes, probe, u32::MAX).collect::<Vec<_>>(), expected);
            prop_assert_eq!(retrieve_all(&bytes).collect::<Vec<_>>(), row);
            prop_assert_eq!(bytes, before);
        }
    }

    /// N2 refusals 4 and 6 (register row 98): the pin's `get_freq` walk
    /// (`ngram.cpp:241-248`) answers before the last whole item, or reads
    /// past a partial tail, or past a value shorter than its total.
    #[test]
    fn checked_get_freq_faults_only_where_the_pins_walk_leaves_the_value() {
        let mut tailed = gram(9, &[(2, 4), (5, 5)]);
        tailed.extend_from_slice(&[1, 2, 3]);
        // A hit or a larger item stops the walk inside the whole items.
        assert_eq!(checked_get_freq(&tailed, 2), Ok(Some(4)));
        assert_eq!(checked_get_freq(&tailed, 3), Ok(None));
        // Past the last whole item the walk meets the tail.
        assert_eq!(checked_get_freq(&tailed, 6), Err(GramFault::OverRead));
        // An item-less value with a tail: `lower_bound` is the begin.
        let mut itemless = gram(0, &[]);
        itemless.push(7);
        assert_eq!(checked_get_freq(&itemless, 1), Err(GramFault::OverRead));
        // Shorter than the total: `begin` already lies past `end`.
        assert_eq!(checked_get_freq(&[1, 0], 1), Err(GramFault::OverRead));
        assert_eq!(checked_get_freq(&[], 1), Err(GramFault::OverRead));
        // A whole value walks off its end as `get_freq` does.
        assert_eq!(checked_get_freq(&gram(9, &[(2, 4)]), 6), Ok(None));
    }

    /// N2 refusals 2 and 6: `search` reads the total (`ngram.cpp:165`),
    /// which asserts on a short value, and then walks until an item at or
    /// past the range end (`:167-173`), reading past a partial tail.
    #[test]
    fn checked_search_asserts_short_and_faults_past_a_partial_tail() {
        let mut out = Vec::new();
        assert_eq!(
            checked_search(&[1, 0, 0], 1, 2, &mut out),
            Err(GramFault::Short)
        );
        let mut tailed = gram(9, &[(2, 4), (5, 5)]);
        tailed.extend_from_slice(&[1]);
        out.clear();
        assert_eq!(checked_search(&tailed, 2, 5, &mut out), Ok(()));
        assert_eq!(out, [(2, 4)]);
        out.clear();
        assert_eq!(
            checked_search(&tailed, 2, 6, &mut out),
            Err(GramFault::OverRead)
        );
    }

    /// On sorted records the lower bound is the first item not less than
    /// the probe, including the first of several equal items.
    #[test]
    fn lower_bound_is_the_first_not_less_on_a_sorted_array() {
        let bytes = gram(0, &[(2, 0), (4, 0), (4, 0), (8, 0)]);
        let probes = [(1, 0), (2, 0), (3, 1), (4, 1), (5, 3), (8, 3), (9, 4)];
        for (token, expected) in probes {
            assert_eq!(lower_bound(&bytes, token), expected, "probe {token}");
        }
        assert_eq!(lower_bound(&new_gram(), 7), 0);
    }

    #[test]
    fn empty_and_short_records_keep_their_refusal_contracts() {
        for len in 0..super::TOTAL_LEN {
            let mut bytes = vec![0xa5; len];
            let before = bytes.clone();
            let mut out = vec![(7, 9)];
            assert_eq!(lower_bound(&bytes, 0), 0);
            assert_eq!(get_freq(&bytes, 0), None);
            assert_eq!(checked_get_freq(&bytes, 0), Err(GramFault::OverRead));
            assert_eq!(
                checked_search(&bytes, 0, 1, &mut out),
                Err(GramFault::Short)
            );
            assert_eq!(out, [(7, 9)]);
            assert!(!set_freq(&mut bytes, 0, 1));
            assert_eq!(remove_freq(&mut bytes, 0), None);
            assert_eq!(bytes, before);
        }
        let mut empty = new_gram();
        let mut out = vec![(7, 9)];
        assert_eq!(lower_bound(&empty, u32::MAX), 0);
        assert_eq!(checked_get_freq(&empty, 0), Ok(None));
        assert_eq!(checked_search(&empty, 0, 1, &mut out), Ok(()));
        assert_eq!(out, [(7, 9)]);
        assert!(insert_freq(&mut empty, 3, 4));
        assert!(!insert_freq(&mut empty, 3, 5));
        assert_eq!(items(&empty), [(3, 4)]);
    }

    /// A partitioned predicate need not imply a globally sorted row.
    /// The physical walk still yields later smaller items.
    #[test]
    fn a_partitioned_search_preserves_the_unsorted_suffix() {
        let bytes = gram(1, &[(0, 1), (3, 2), (2, 7)]);
        let mut out = Vec::new();
        assert_eq!(checked_search(&bytes, 2, 4, &mut out), Ok(()));
        assert_eq!(out, [(3, 2), (2, 7)]);
    }

    #[test]
    fn unpartitioned_mutations_start_from_the_std_partition() {
        let row = [(9, 4), (3, 6), (7, 2)];
        let mut bytes = gram(12, &row);
        let first = lower_bound(&bytes, 3);
        let hit = row[first..]
            .iter()
            .take_while(|&&(token, _)| token <= 3)
            .position(|&(token, _)| token == 3)
            .map(|at| first + at);
        assert_eq!(get_freq(&bytes, 3), hit.map(|at| row[at].1));
        assert_eq!(set_freq(&mut bytes, 3, 69), hit.is_some());
        let mut expected = row.to_vec();
        if let Some(at) = hit {
            expected[at].1 = 69;
        }
        assert_eq!(items(&bytes), expected);
        assert_eq!(get_total_freq(&bytes), Some(12));
        assert_eq!(remove_freq(&mut bytes, 3), hit.map(|_| 69));
        if let Some(at) = hit {
            expected.remove(at);
        }
        assert_eq!(items(&bytes), expected);
    }

    /// Q2: get/set act on the first match; mask removes every match and
    /// subtracts each frequency; retrieve_all keeps stored order.
    #[test]
    fn duplicates_are_kept_and_masked_together() {
        let mut bytes = gram(20, &[(5, 3), (5, 7), (6, 10)]);
        assert_eq!(get_freq(&bytes, 5), Some(3));
        assert!(set_freq(&mut bytes, 5, 4));
        assert_eq!(items(&bytes), [(5, 4), (5, 7), (6, 10)]);
        assert!(!insert_freq(&mut bytes, 5, 1));
        assert_eq!(mask_out(&mut bytes, u32::MAX, 5), 2);
        assert_eq!(items(&bytes), [(6, 10)]);
        assert_eq!(get_total_freq(&bytes), Some(9));
        assert_eq!(remove_freq(&mut bytes, 6), Some(10));
        assert_eq!(get_length(&bytes), 0);
    }

    /// Removal subtracts from the stored total with wrap; a mask that
    /// matches nothing leaves the bytes as they were.
    #[test]
    fn mask_out_subtracts_from_the_stored_total_with_wrap() {
        let mut bytes = gram(1, &[(1, 2), (2, 3)]);
        assert_eq!(mask_out(&mut bytes, u32::MAX, 2), 1);
        assert_eq!(get_total_freq(&bytes), Some(1_u32.wrapping_sub(3)));
        let before = bytes.clone();
        assert_eq!(mask_out(&mut bytes, u32::MAX, 7), 0);
        assert_eq!(bytes, before);
    }

    /// Walks stop at the last whole item; a partial tail is never read, and
    /// a short value removes nothing.
    #[test]
    fn walks_stop_at_the_last_whole_item() {
        let mut bytes = gram(5, &[(1, 5)]);
        bytes.extend_from_slice(&[0xff; 3]);
        assert_eq!(get_length(&bytes), 1);
        assert_eq!(items(&bytes), [(1, 5)]);
        let mut short = vec![1_u8, 2];
        assert_eq!(mask_out(&mut short, 0, 0), 0);
        assert_eq!(short, [1, 2]);
        assert_eq!(get_total_freq(&short), None);
    }

    /// Constitution §4: no index panics. One that overflows the offset
    /// arithmetic, or lies past the last whole item, answers `None`.
    #[test]
    fn a_huge_item_index_answers_none_without_panicking() {
        let bytes = gram(7, &[(3, 7)]);
        assert_eq!(item_offset(usize::MAX), None);
        assert_eq!(item_offset(usize::MAX / 8 + 1), None);
        for index in [1, usize::MAX / 8, usize::MAX - 1, usize::MAX] {
            assert_eq!(item_token(&bytes, index), None, "token {index}");
            assert_eq!(item_freq(&bytes, index), None, "freq {index}");
        }
        assert_eq!(item_token(&bytes, 0), Some(3));
        assert_eq!(item_freq(&bytes, 0), Some(7));
    }

    #[test]
    fn insert_appends_past_the_largest_and_search_stops_at_the_end() {
        let mut bytes = gram(0, &[(2, 1), (4, 1)]);
        assert!(insert_freq(&mut bytes, 9, 2));
        assert!(insert_freq(&mut bytes, 3, 2));
        assert_eq!(items(&bytes), [(2, 1), (3, 2), (4, 1), (9, 2)]);
        let found: Vec<_> = search(&bytes, 3, 5).collect();
        assert_eq!(found, [(3, 2), (4, 1)]);
    }

    #[test]
    fn merge_copies_one_side_and_sums_both() {
        let mut merged = gram(9, &[(1, 1)]);
        assert!(!merge_single_gram(&mut merged, None, None));
        assert_eq!(items(&merged), [(1, 1)]);
        let user = gram(3, &[(2, 3)]);
        assert!(merge_single_gram(&mut merged, None, Some(&user)));
        assert_eq!(merged, user);
        let system = gram(u32::MAX, &[(1, 4), (2, 5)]);
        assert!(merge_single_gram(&mut merged, Some(&system), Some(&user)));
        assert_eq!(get_total_freq(&merged), Some(2));
        assert_eq!(items(&merged), [(1, 4), (2, 8)]);
    }
}
