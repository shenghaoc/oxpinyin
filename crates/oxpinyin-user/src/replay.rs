//! The pin's `SubPhraseIndex::merge` over one system library's journal
//! (`phrase_index.cpp:439-517` at 074a2219), kept in the pin's own terms:
//! records are compared as bytes, each stored payload is kept whole, and the
//! item a lookup sees is the header-derived prefix of it.
//!
//! * `PhraseItem::operator==` is equal chunk sizes, then `memcmp`
//!   (`phrase_index.h:235-240`).
//! * `get_phrase_item` takes the item's length from its header,
//!   `6 + 4L + n(2L + 4)`, not from what was stored (`:180-206`);
//!   `add_phrase_item` stores the whole payload, trailing bytes included
//!   (`:208-217`).
//! * REMOVE calls `remove_phrase_item` (the item leaves the index and its
//!   unigram leaves the total) before it compares, so a mismatch returns
//!   false with the removal kept (`:460-475`, `:219-236`).
//! * MODIFY compares against the visible item first. A new chunk longer than
//!   it is a remove plus an add of the whole payload; otherwise it is
//!   `memmove`d over the stored bytes (`:477-501`).
//! * `item`, the `PhraseItem` MODIFY fetches into, outlives its call: when
//!   `get_phrase_item` fails for a later record the compare runs against what
//!   it still points at (`:478-483`).
//! * The first failure ends the merge and the records before it stay applied
//!   (`pinyin.cpp:276` ignores the answer).
//!
//! Typed [`ChunkItem`]s are made once, at [`Merge::finish`].

use std::borrow::Cow;
use std::collections::BTreeMap;

use oxpinyin_data::chunk_write::{ChunkItem, PHRASE_MASK, decode_phrase_item};
use oxpinyin_data::user_files::LogRecord;

use crate::persistence::SystemLibrary;

/// Why the merge ended at a record.
pub(crate) enum Stop {
    /// The record's payload is not an item (`decode_phrase_item`'s error).
    Malformed(String),
    /// `merge`'s `return false` at a REMOVE, MODIFY or header record.
    Mismatch(&'static str, u32),
}

/// A stored payload: where `m_phrase_content` keeps it, and its bytes.
struct Region {
    id: u64,
    bytes: Vec<u8>,
}

enum Held {
    /// `remove_phrase_item` zeroed the offset; the bytes stay where they were.
    Removed(Option<Region>),
    Stored(Region),
}

/// The `PhraseItem` MODIFY last fetched into: the storage it points at and the
/// size it was given.
struct Stale {
    slot: u32,
    id: u64,
    size: usize,
    /// `add_phrase_item` calls before the fetch, for a region that can move.
    appends: u64,
    /// The bytes, once their slot no longer holds the storage.
    detached: Option<Vec<u8>>,
}

/// Original (mapped) storage never moves; the id carries the slot.
const ORIGINAL: u64 = 1 << 40;

/// The length `get_phrase_item` gives an item from its two header bytes, when
/// `bytes` holds that much.
fn visible_len(bytes: &[u8]) -> Option<usize> {
    let (&length, &prons) = (bytes.first()?, bytes.get(1)?);
    let (length, prons) = (usize::from(length), usize::from(prons));
    let len = 6 + 4 * length + prons * (2 * length + 4);
    (len <= bytes.len()).then_some(len)
}

/// `PhraseItem::get_unigram_frequency` read from an item's bytes: the `guint32`
/// after the two length bytes (`phrase_index.h:124-126`).
pub(crate) fn raw_unigram(item: &[u8]) -> u32 {
    item.get(2..6)
        .and_then(|bytes| bytes.try_into().ok())
        .map_or(0, u32::from_le_bytes)
}

pub(crate) struct Merge<'a> {
    original: Option<&'a SystemLibrary>,
    held: BTreeMap<u32, Held>,
    /// `SubPhraseIndex::m_total_freq`, a wrapping `guint32`; `None` for a
    /// library with no original.
    total: Option<u32>,
    stale: Option<Stale>,
    appends: u64,
    next_id: u64,
}

impl<'a> Merge<'a> {
    pub(crate) fn new(original: Option<&'a SystemLibrary>) -> Self {
        Self {
            original,
            held: BTreeMap::new(),
            total: original.map(|library| library.total),
            stale: None,
            appends: 0,
            next_id: 0,
        }
    }

    /// The item `get_phrase_item` finds at `slot`: its header-length bytes.
    fn visible(&self, slot: u32) -> Option<Cow<'_, [u8]>> {
        match self.held.get(&slot) {
            Some(Held::Stored(region)) => {
                let len = visible_len(&region.bytes)?;
                Some(Cow::Borrowed(&region.bytes[..len]))
            }
            Some(Held::Removed(_)) => None,
            None => self.original?.visible_bytes(slot),
        }
    }

    fn fresh_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    /// `old` was what `slot` held and no longer is (an add over it, a grow): a
    /// `Stale` on that storage keeps its bytes.
    fn detach_from(&mut self, slot: u32, old: Option<Held>) {
        let Some(stale) = self.stale.as_mut() else {
            return;
        };
        if stale.slot != slot || stale.detached.is_some() {
            return;
        }
        stale.detached = match old {
            Some(Held::Stored(region) | Held::Removed(Some(region))) if region.id == stale.id => {
                Some(region.bytes)
            }
            None if stale.id == ORIGINAL | u64::from(slot) => self
                .original
                .and_then(|library| library.visible_bytes(slot))
                .map(Cow::into_owned),
            _ => None,
        };
    }

    /// The bytes the stale `item` still points at, when they can be told.
    fn stale_bytes(&self) -> Option<&[u8]> {
        let stale = self.stale.as_ref()?;
        // A region the pin allocated can move when `m_phrase_content` grows;
        // the mapped chunk cannot.
        if stale.id & ORIGINAL == 0 && stale.appends != self.appends {
            return None;
        }
        let bytes = match (&stale.detached, self.held.get(&stale.slot)) {
            (Some(bytes), _) => bytes.as_slice(),
            (None, Some(Held::Stored(region) | Held::Removed(Some(region))))
                if region.id == stale.id =>
            {
                region.bytes.as_slice()
            }
            _ => return None,
        };
        bytes.get(..stale.size)
    }

    pub(crate) fn apply(&mut self, record: LogRecord) -> Result<(), Stop> {
        match record {
            LogRecord::Add { token, new_item } => self.add(token & PHRASE_MASK, new_item),
            LogRecord::Remove { token, old_item } => self.remove(token, &old_item),
            LogRecord::Modify {
                token,
                old_item,
                new_item,
            } => self.modify(token, &old_item, new_item),
            LogRecord::ModifyHeader {
                old_total,
                new_total,
            } => {
                // The header must land on the current total, as the merge has
                // moved it so far; a mismatch is merge's failure return
                // (`phrase_index.cpp:503-514`).
                let now = self.total.unwrap_or_else(|| self.items_total());
                if now != old_total {
                    return Err(Stop::Mismatch("header", 0));
                }
                self.total = self.total.map(|_| new_total);
                Ok(())
            }
        }
    }

    /// The unigrams of the items held, for a library with no original.
    fn items_total(&self) -> u32 {
        self.held.values().fold(0, |total, held| match held {
            Held::Stored(region) => total.saturating_add(raw_unigram(&region.bytes)),
            Held::Removed(_) => total,
        })
    }

    fn add(&mut self, slot: u32, payload: Vec<u8>) -> Result<(), Stop> {
        if visible_len(&payload).is_none() {
            return Err(Stop::Malformed(
                decode_phrase_item(&payload)
                    .err()
                    .map_or_else(String::new, |error| error.to_string()),
            ));
        }
        self.total = self
            .total
            .map(|total| total.wrapping_add(raw_unigram(&payload)));
        let id = self.fresh_id();
        self.appends += 1;
        let old = self
            .held
            .insert(slot, Held::Stored(Region { id, bytes: payload }));
        self.detach_from(slot, old);
        Ok(())
    }

    /// The region `slot` holds, taken out of the index: its bytes stay.
    fn take_region(&mut self, slot: u32, visible: Vec<u8>) -> Region {
        match self.held.remove(&slot) {
            Some(Held::Stored(region)) => region,
            _ => Region {
                id: ORIGINAL | u64::from(slot),
                bytes: visible,
            },
        }
    }

    fn remove(&mut self, token: u32, old_item: &[u8]) -> Result<(), Stop> {
        let slot = token & PHRASE_MASK;
        let Some(visible) = self.visible(slot).map(Cow::into_owned) else {
            // `get_phrase_item` fails and `merge` dereferences the NULL item
            // (`phrase_index.cpp:468`, row 107): the removal of an item that is
            // not there is a no-op here, and an old chunk that is no item at
            // all is a mismatch.
            if visible_len(old_item).is_none() {
                return Err(Stop::Mismatch("REMOVE", token));
            }
            self.held.entry(slot).or_insert(Held::Removed(None));
            return Ok(());
        };
        // `remove_phrase_item` first: the item and its unigram leave, then the
        // chunks are compared and a mismatch keeps the removal.
        let unigram = raw_unigram(&visible);
        let equal = visible.as_slice() == old_item;
        let region = self.take_region(slot, visible);
        self.held.insert(slot, Held::Removed(Some(region)));
        self.total = self.total.map(|total| total.wrapping_sub(unigram));
        if equal {
            Ok(())
        } else {
            Err(Stop::Mismatch("REMOVE", token))
        }
    }

    fn modify(&mut self, token: u32, old_item: &[u8], new_item: Vec<u8>) -> Result<(), Stop> {
        let slot = token & PHRASE_MASK;
        let mismatch = || Stop::Mismatch("MODIFY", token);
        if visible_len(&new_item).is_none() {
            return Err(mismatch());
        }
        match self.held.get_mut(&slot) {
            Some(Held::Stored(region)) => {
                let len = visible_len(&region.bytes).ok_or_else(mismatch)?;
                // `get_phrase_item` succeeded: `item` now points at this storage.
                self.stale = Some(Stale {
                    slot,
                    id: region.id,
                    size: len,
                    appends: self.appends,
                    detached: None,
                });
                if region.bytes[..len] != *old_item {
                    return Err(mismatch());
                }
                if new_item.len() > len {
                    // A remove and an add of the whole new payload.
                    let unigram = raw_unigram(&region.bytes);
                    self.total = self.total.map(|total| {
                        total
                            .wrapping_sub(unigram)
                            .wrapping_add(raw_unigram(&new_item))
                    });
                    self.next_id += 1;
                    self.appends += 1;
                    let old = std::mem::replace(
                        region,
                        Region {
                            id: self.next_id,
                            bytes: new_item,
                        },
                    );
                    if let Some(stale) = self.stale.as_mut() {
                        stale.detached = Some(old.bytes);
                    }
                } else if new_item.len() == region.bytes.len() {
                    // In place, over all of it.
                    region.bytes = new_item;
                } else {
                    // In place: `memmove` over the stored bytes, which keep their length.
                    region.bytes[..new_item.len()].copy_from_slice(&new_item);
                }
                Ok(())
            }
            Some(Held::Removed(_)) => self.modify_stale(old_item, &new_item).ok_or_else(mismatch),
            None => {
                let Some(visible) = self
                    .original
                    .and_then(|library| library.visible_bytes(slot))
                else {
                    return self.modify_stale(old_item, &new_item).ok_or_else(mismatch);
                };
                let len = visible.len();
                self.stale = Some(Stale {
                    slot,
                    id: ORIGINAL | u64::from(slot),
                    size: len,
                    appends: self.appends,
                    detached: None,
                });
                if visible.as_ref() != old_item {
                    return Err(mismatch());
                }
                let (id, bytes) = if new_item.len() > len {
                    let unigram = raw_unigram(&visible);
                    self.total = self.total.map(|total| {
                        total
                            .wrapping_sub(unigram)
                            .wrapping_add(raw_unigram(&new_item))
                    });
                    if let Some(stale) = self.stale.as_mut() {
                        stale.detached = Some(visible.into_owned());
                    }
                    self.next_id += 1;
                    self.appends += 1;
                    (self.next_id, new_item)
                } else if new_item.len() == len {
                    // The shipped chunk's storage, written over all of it.
                    (ORIGINAL | u64::from(slot), new_item)
                } else {
                    // The shipped chunk's storage, `memmove`d over its start.
                    let mut bytes = visible.into_owned();
                    bytes[..new_item.len()].copy_from_slice(&new_item);
                    (ORIGINAL | u64::from(slot), bytes)
                };
                self.held.insert(slot, Held::Stored(Region { id, bytes }));
                Ok(())
            }
        }
    }

    /// MODIFY of a token `get_phrase_item` does not find: the compare runs
    /// against the stale `item` (`phrase_index.cpp:478-483`). Equal bytes of the
    /// same size go on to `memmove` over that storage; a longer new chunk calls
    /// `remove_phrase_item` for the missing token and `assert(olditem ==
    /// *tmpitem)` dereferences NULL (`:486-491`, row 107), which stops here.
    fn modify_stale(&mut self, old_item: &[u8], new_item: &[u8]) -> Option<()> {
        let Some(stale) = self.stale.as_ref() else {
            // The default `PhraseItem` is empty: a non-empty old chunk differs;
            // an empty one is the NULL dereference above.
            return None;
        };
        let (stale_slot, id, size) = (stale.slot, stale.id, stale.size);
        let bytes = self.stale_bytes()?;
        if bytes != old_item || new_item.len() > size {
            return None;
        }
        // Write through to wherever the storage lives.
        let stale = self.stale.as_mut()?;
        if let Some(detached) = stale.detached.as_mut() {
            detached[..new_item.len()].copy_from_slice(new_item);
            return Some(());
        }
        match self.held.get_mut(&stale_slot) {
            Some(Held::Stored(region) | Held::Removed(Some(region))) if region.id == id => {
                region.bytes[..new_item.len()].copy_from_slice(new_item);
                Some(())
            }
            _ => None,
        }
    }

    /// The library's total and its overrides as typed items; `None` removes.
    pub(crate) fn finish(
        self,
        skipped: &mut Vec<String>,
        name: &impl std::fmt::Display,
    ) -> (Option<u32>, BTreeMap<u32, Option<ChunkItem>>) {
        let mut typed = Vec::with_capacity(self.held.len());
        for (slot, held) in self.held {
            match held {
                Held::Removed(_) => typed.push((slot, None)),
                Held::Stored(region) => {
                    let len = visible_len(&region.bytes).unwrap_or(0);
                    match decode_phrase_item(&region.bytes[..len]) {
                        Ok(item) => typed.push((slot, Some(item))),
                        Err(error) => skipped.push(format!("{name}: {error}")),
                    }
                }
            }
        }
        // Ascending slots: the map is built in one pass.
        let overrides = typed.into_iter().collect();
        (self.total, overrides)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::journal_item_bytes;

    fn item(phrase: &[u32], unigram: u32, prons: &[(Vec<u16>, u32)]) -> ChunkItem {
        ChunkItem {
            phrase: phrase.to_vec(),
            unigram,
            prons: prons.to_vec(),
        }
    }

    /// Slot 1 holds a one-reading item (unigram 100), slot 2 a two-reading one
    /// (unigram 50): the library's total is their sum.
    fn library() -> SystemLibrary {
        SystemLibrary {
            total: 150,
            range_end: 3,
            items: BTreeMap::from([
                (1, item(&[0x4f60], 100, &[(vec![1], 100)])),
                (2, item(&[0x597d], 50, &[(vec![2], 30), (vec![3], 20)])),
            ]),
            mapped: None,
        }
    }

    fn one() -> Vec<u8> {
        journal_item_bytes(&library().items[&1])
    }

    fn two() -> Vec<u8> {
        journal_item_bytes(&library().items[&2])
    }

    fn with_last(mut item: Vec<u8>, byte: u8) -> Vec<u8> {
        *item.last_mut().expect("item") = byte;
        item
    }

    fn with_freq(mut item: Vec<u8>, frequency: u32) -> Vec<u8> {
        let end = item.len();
        item[end - 4..].copy_from_slice(&frequency.to_le_bytes());
        item
    }

    /// What a merge leaves: how it ended, the total and the overrides.
    type Merged = (
        Result<(), Stop>,
        Option<u32>,
        BTreeMap<u32, Option<ChunkItem>>,
    );

    fn merge_of(library: &SystemLibrary, records: Vec<LogRecord>) -> Merged {
        let mut merge = Merge::new(Some(library));
        let mut outcome = Ok(());
        for record in records {
            if let Err(stop) = merge.apply(record) {
                outcome = Err(stop);
                break;
            }
        }
        let (total, overrides) = merge.finish(&mut Vec::new(), &"test");
        (outcome, total, overrides)
    }

    fn modify(token: u32, old_item: Vec<u8>, new_item: Vec<u8>) -> LogRecord {
        LogRecord::Modify {
            token,
            old_item,
            new_item,
        }
    }

    #[test]
    fn a_failed_remove_keeps_the_removal_and_stops() {
        // Same size, other bytes: the item and its unigram are gone already.
        let records = vec![
            LogRecord::Remove {
                token: 1,
                old_item: with_last(one(), 0xff),
            },
            LogRecord::Add {
                token: 5,
                new_item: one(),
            },
        ];
        let (outcome, total, overrides) = merge_of(&library(), records);
        assert!(matches!(outcome, Err(Stop::Mismatch("REMOVE", 1))));
        assert_eq!(total, Some(50));
        assert_eq!(overrides, BTreeMap::from([(1, None)]));
    }

    #[test]
    fn a_trailing_byte_makes_an_old_chunk_differ() {
        let mut trailing = one();
        trailing.push(0);
        let (outcome, total, overrides) = merge_of(
            &library(),
            vec![LogRecord::Remove {
                token: 1,
                old_item: trailing.clone(),
            }],
        );
        assert!(matches!(outcome, Err(Stop::Mismatch("REMOVE", 1))));
        assert_eq!((total, overrides), (Some(50), BTreeMap::from([(1, None)])));
        // MODIFY compares first: nothing is changed.
        let (outcome, total, overrides) =
            merge_of(&library(), vec![modify(1, trailing, with_last(one(), 9))]);
        assert!(matches!(outcome, Err(Stop::Mismatch("MODIFY", 1))));
        assert_eq!((total, overrides), (Some(150), BTreeMap::new()));
    }

    #[test]
    fn a_grown_item_is_stored_whole_but_is_as_long_as_its_header() {
        let mut grown = with_freq(one(), 5);
        grown.extend_from_slice(&[1, 2, 3]);
        let visible = grown[..grown.len() - 3].to_vec();
        // The next record's old chunk is the header-length bytes.
        let (outcome, total, overrides) = merge_of(
            &library(),
            vec![
                modify(1, one(), grown.clone()),
                modify(1, visible.clone(), with_freq(visible.clone(), 7)),
            ],
        );
        assert!(outcome.is_ok());
        assert_eq!(total, Some(150));
        assert_eq!(
            overrides[&1].as_ref().expect("item").prons,
            vec![(vec![1], 7)]
        );
        // With the stored trailing bytes it is another size: a mismatch.
        let (outcome, _, _) = merge_of(
            &library(),
            vec![
                modify(1, one(), grown.clone()),
                modify(1, grown, with_freq(visible, 7)),
            ],
        );
        assert!(matches!(outcome, Err(Stop::Mismatch("MODIFY", 1))));
    }

    #[test]
    fn an_in_place_chunk_with_a_shorter_header_is_what_the_next_record_sees() {
        // Slot 2 has two readings; the new chunk has one and a tail that fits.
        let mut shorter = two()[..two().len() - 6].to_vec();
        shorter[1] = 1;
        let mut chunk = shorter.clone();
        chunk.extend_from_slice(&[9, 9, 9, 9]);
        assert!(chunk.len() <= two().len());
        let (outcome, total, overrides) = merge_of(
            &library(),
            vec![
                modify(2, two(), chunk),
                modify(2, shorter.clone(), with_freq(shorter, 8)),
            ],
        );
        assert!(outcome.is_ok());
        // In place leaves the total alone.
        assert_eq!(total, Some(150));
        assert_eq!(
            overrides[&2].as_ref().expect("item").prons,
            vec![(vec![2], 8)]
        );
    }

    #[test]
    fn a_stale_item_takes_a_same_size_modify_of_an_absent_token() {
        // MODIFY of slot 2 leaves `item` on its storage; a MODIFY of a token
        // the library does not hold, whose old chunk equals it, is written
        // over that storage and the merge goes on.
        let first = with_freq(two(), 21);
        let (outcome, total, overrides) = merge_of(
            &library(),
            vec![
                modify(2, two(), first.clone()),
                modify(9, first.clone(), with_freq(two(), 22)),
                LogRecord::Add {
                    token: 7,
                    new_item: one(),
                },
            ],
        );
        assert!(outcome.is_ok());
        assert_eq!(total, Some(250));
        assert_eq!(
            overrides[&2].as_ref().expect("item").prons,
            vec![(vec![2], 30), (vec![3], 22)]
        );
        assert!(!overrides.contains_key(&9));
        assert!(overrides.contains_key(&7));
        // A longer new chunk would be the pin's NULL dereference: the merge ends.
        let mut longer = with_freq(two(), 22);
        longer.push(0);
        let (outcome, _, _) = merge_of(
            &library(),
            vec![modify(2, two(), first.clone()), modify(9, first, longer)],
        );
        assert!(matches!(outcome, Err(Stop::Mismatch("MODIFY", 9))));
        // With no earlier fetch the stale item is empty and differs.
        let (outcome, _, _) = merge_of(&library(), vec![modify(9, two(), two())]);
        assert!(matches!(outcome, Err(Stop::Mismatch("MODIFY", 9))));
    }

    #[test]
    fn an_empty_phrase_is_an_item_of_its_header_length() {
        let empty = vec![0, 0, 3, 0, 0, 0];
        let (outcome, total, overrides) = merge_of(
            &library(),
            vec![
                modify(1, one(), empty.clone()),
                modify(1, empty, vec![0, 0, 4, 0, 0, 0]),
            ],
        );
        assert!(outcome.is_ok());
        assert_eq!(total, Some(150));
        assert_eq!(overrides[&1], Some(item(&[], 4, &[])));
    }
}
