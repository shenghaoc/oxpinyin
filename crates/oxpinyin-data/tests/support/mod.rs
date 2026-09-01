//! Shared fixture builders for the lazy-reader integration tests.
//!
//! `ChunkBuilder` is a faithful `SubPhraseIndex::store` counterpart (the
//! seed of the P5 native emitter): chunk header with upstream's checksum,
//! the `'#'`-separated sections, and items written at `add_phrase_item`'s
//! offsets (first item at 8, `0` = no item). The DBM helpers write bare
//! keyspace rows (`put_raw`), the layout libpinyin's own DBMs and the P5
//! writers produce.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::Path;

use oxpinyin_store::WriteStore;

/// Builds one phrase-library chunk file from slot → item entries.
/// One item's pieces: `(unigram, text, pronunciations as (packed
/// keys, freq))`.
pub type ItemParts = (u32, String, Vec<(Vec<u8>, u32)>);

pub struct ChunkBuilder {
    total_freq: u32,
    /// slot → item parts.
    pub items: BTreeMap<usize, ItemParts>,
}

pub fn checksum(payload: &[u8]) -> u32 {
    let mut sum: u32 = 0;
    let aligned = payload.len() & !0x3;
    for word in payload[..aligned].chunks_exact(4) {
        sum ^= u32::from_le_bytes([word[0], word[1], word[2], word[3]]);
    }
    let mut shift = 0_u32;
    for &byte in &payload[aligned..] {
        sum ^= u32::from(byte) << shift;
        shift += 8;
    }
    sum
}

impl ChunkBuilder {
    pub fn new(total_freq: u32) -> Self {
        Self {
            total_freq,
            items: BTreeMap::new(),
        }
    }

    pub fn add(
        &mut self,
        slot: usize,
        unigram: u32,
        text: &str,
        pronunciations: Vec<(Vec<u8>, u32)>,
    ) {
        self.items
            .insert(slot, (unigram, text.to_owned(), pronunciations));
    }

    /// Serializes exactly what `SubPhraseIndex::store` writes.
    pub fn build(&self) -> Vec<u8> {
        let max_slot = self.items.keys().copied().max().unwrap_or(0);
        let slots = if self.items.is_empty() {
            0
        } else {
            max_slot + 1
        };

        // Entry area: bytes 0..8 reserved, then items in slot order.
        let mut content: Vec<u8> = vec![0; 8];
        let mut offsets = vec![0_u32; slots];
        for (&slot, (unigram, text, pronunciations)) in &self.items {
            offsets[slot] = content.len() as u32;
            content.push(text.chars().count() as u8);
            content.push(pronunciations.len() as u8);
            content.extend_from_slice(&unigram.to_le_bytes());
            for ch in text.chars() {
                content.extend_from_slice(&(ch as u32).to_le_bytes());
            }
            for (keys, freq) in pronunciations {
                content.extend_from_slice(keys);
                content.extend_from_slice(&freq.to_le_bytes());
            }
        }

        let index_one = 17_usize; // 4 words + separator, per store()
        let mut offset_array: Vec<u8> = Vec::new();
        for offset in &offsets {
            offset_array.extend_from_slice(&offset.to_le_bytes());
        }
        let index_two = index_one + offset_array.len() + 1; // + separator
        let index_three = index_two + content.len() + 1;

        let mut payload = Vec::new();
        payload.extend_from_slice(&self.total_freq.to_le_bytes());
        payload.extend_from_slice(&(index_one as u32).to_le_bytes());
        payload.extend_from_slice(&(index_two as u32).to_le_bytes());
        payload.extend_from_slice(&(index_three as u32).to_le_bytes());
        payload.push(b'#');
        payload.extend_from_slice(&vec![0; index_one - payload.len()]);
        payload.extend_from_slice(&offset_array);
        payload.push(b'#');
        payload.extend_from_slice(&content);
        payload.push(b'#');
        assert_eq!(payload.len(), index_three);

        let mut file = Vec::new();
        file.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        file.extend_from_slice(&checksum(&payload).to_le_bytes());
        file.extend_from_slice(&payload);
        file
    }
}

/// Writes `bytes` to a unique temp file named `name` under `dir`.
pub fn write_temp(dir: &Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let unique = SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = dir.join(format!("{name}-{}-{unique}.bin", std::process::id()));
    std::fs::write(&path, bytes).expect("write temp chunk");
    path
}

/// Packs `(initial, middle, final, tone)` element values as an upstream
/// `ChewingKey` u16 (LE bytes appended to `buf`).
pub fn push_packed_key(buf: &mut Vec<u8>, initial: u8, middle: u8, final_: u8, tone: u8) {
    let bits = (initial & 0x1f) as u16
        | ((middle & 0x3) as u16) << 5
        | ((final_ & 0x1f) as u16) << 7
        | ((tone & 0x7) as u16) << 12;
    buf.extend_from_slice(&bits.to_le_bytes());
}

/// Writes bare-keyspace rows into a freshly created store file — the
/// layout libpinyin's DBMs use (no table-name framing).
pub fn write_raw_rows<S: WriteStore>(path: &Path, rows: &[(Vec<u8>, Vec<u8>)]) {
    let store = S::create(path).expect("create store");
    store
        .write(|txn| {
            for (key, value) in rows {
                txn.put_raw(key, value)?;
            }
            Ok(())
        })
        .expect("bulk put_raw");
}
