//! The per-library phrase-index chunk **writer** — the write half of the
//! `MemoryChunk` + `SubPhraseIndex` format `chunk_format` and
//! `phrase_library` read.
//!
//! This is the byte-level output of upstream's `SubPhraseIndex::store`
//! (`src/storage/phrase_index.cpp`), reached after `compact()` rebuilds a
//! sub-index in ascending token order. `oxpinyin-datagen` writes the
//! system libraries' files with it; the user store writes `user.bin`,
//! `addon.bin` and `network.bin` with it — the USER_FILE sub-indexes,
//! whose whole-store files the pin's `_write_files` produces through the
//! same `store` path.
//!
//! ```text
//! file    = { length: u32, checksum: u32 } payload          (MemoryChunk)
//! payload = [ total_freq: u32 ]
//!           [ index_one, index_two, index_three: u32×3 ]
//!           '#' offset-array '#' entry-area '#'
//! ```
//!
//! * `index_one == 17` (header 16 + separator) — the offset array holds
//!   one `u32` per `token & PHRASE_MASK` slot up to the highest occupied
//!   slot; `0` is the no-item sentinel (`add_phrase_item` never stores an
//!   offset below 8).
//! * The entry area's first 8 bytes stay zero (`add_phrase_item` bumps a
//!   zero content size to 8 before the first write) — **once there is a
//!   first write**: a library with no items has an empty entry area, so
//!   its whole payload is the 16-byte header and three separators
//!   (19 bytes; measured on the pin's own `gen_binary_files` output for
//!   an empty `.table`). Item offsets in the offset array are relative to
//!   the entry-area start.
//! * Each item is `{ u8 phrase_length, u8 n_pronunciations, u32 unigram,
//!   ucs4_t phrase[L], { ChewingKey u16[L], u32 freq } × n_pronunciations }`
//!   (`phrase_item_header`, `phrase_index.h:56`; `sizeof(ChewingKey) == 2`).
//! * Items appear in ascending slot order — the order `compact()`'s token
//!   walk feeds `add_phrase_item`, which appends to the entry area.
//!
//! The checksum is `MemoryChunk::get_check_sum`: the XOR of the payload's
//! little-endian `u32` words with tail bytes folded in shifted by
//! position (`chunk_format` — the one written copy both halves take).

use crate::chunk_format::{CHUNK_HEADER_SIZE, SEPARATOR, chunk_checksum};

/// `PHRASE_MASK` (`novel_types.h:41`): the library-local token bits a
/// phrase-index slot is addressed by. Re-exported from the shared format
/// module for the writers above this crate.
pub use crate::chunk_format::PHRASE_MASK;

/// Header `total_freq` + three offsets, then the first separator: where
/// the offset array starts (`SubPhraseIndex::store`).
const INDEX_ONE: u32 = 17;
/// `add_phrase_item` reserves the first 8 entry-area bytes by bumping a
/// zero content size to 8 on the first item; the first real item lives at
/// offset 8, and a library with no items reserves nothing.
const FIRST_ITEM_OFFSET: u32 = 8;

/// A malformed [`ChunkItem`] or slot sequence.
#[derive(Debug)]
pub struct ChunkWriteError(String);

impl std::fmt::Display for ChunkWriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "chunk write error: {}", self.0)
    }
}

impl std::error::Error for ChunkWriteError {}

/// One phrase entry of a library chunk.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChunkItem {
    /// The phrase text as UCS-4 code points (`ucs4_t` array).
    pub phrase: Vec<u32>,
    /// The item's unigram frequency — the `\1-gram` count plus the one
    /// `gen_unigram` adds for every library token (the system libraries;
    /// a user item carries whatever training accumulated).
    pub unigram: u32,
    /// Pronunciations in insertion order: packed `ChewingKey` sequence
    /// (`to_packed`, little-endian on disk) and the accumulated frequency
    /// (`PhraseItem::add_pronunciation` sums duplicate exact key
    /// sequences).
    pub prons: Vec<(Vec<u16>, u32)>,
}

/// Serialises one library's phrase items into the complete chunk file.
///
/// `items` must be in ascending slot order (slot = `token & PHRASE_MASK`)
/// — the order `FacadePhraseIndex::compact()` produces — and slots must
/// not repeat.
///
/// # Errors
///
/// Fails on a repeated or out-of-order slot, an empty phrase, a
/// pronunciation whose key run length differs from the phrase length, an
/// item longer than the format allows (phrase length or pronunciation
/// count above `u8::MAX`), or a total frequency above `u32::MAX`.
pub fn build_chunk(items: &[(u32, ChunkItem)]) -> Result<Vec<u8>, ChunkWriteError> {
    // ---- validate and serialise the entry area -------------------------
    // Empty until the first item: `add_phrase_item`'s `if (0 == offset)
    // offset = 8` only fires when an item is written.
    let mut content: Vec<u8> = Vec::new();
    let mut offsets: Vec<u32> = Vec::new();
    let mut total_freq: u64 = 0;
    let mut last_slot: Option<u32> = None;

    for &(slot, ref item) in items {
        if slot > PHRASE_MASK {
            return Err(ChunkWriteError(format!(
                "chunk slot {slot:#010x} exceeds PHRASE_MASK"
            )));
        }
        if let Some(prev) = last_slot.filter(|&p| slot <= p) {
            return Err(ChunkWriteError(format!(
                "chunk slots not ascending: {slot:#010x} after {prev:#010x}"
            )));
        }
        last_slot = Some(slot);

        let phrase_len = item.phrase.len();
        if phrase_len == 0 || phrase_len > usize::from(u8::MAX) {
            return Err(ChunkWriteError(format!(
                "chunk slot {slot:#010x} phrase length {} out of range",
                item.phrase.len()
            )));
        }
        if item.prons.len() > usize::from(u8::MAX) {
            return Err(ChunkWriteError(format!(
                "chunk slot {slot:#010x} has {} pronunciations",
                item.prons.len()
            )));
        }

        if content.is_empty() {
            content.resize(usize::try_from(FIRST_ITEM_OFFSET).unwrap_or(0), 0);
        }
        let offset = u32::try_from(content.len())
            .map_err(|_| ChunkWriteError(format!("chunk slot {slot:#010x} offset overflows u32")))?;
        offsets.resize(usize::try_from(slot).unwrap_or(0) + 1, 0);
        offsets[usize::try_from(slot).unwrap_or(0)] = offset;

        content.push(u8::try_from(phrase_len).unwrap_or(u8::MAX));
        content.push(u8::try_from(item.prons.len()).unwrap_or(u8::MAX));
        content.extend_from_slice(&item.unigram.to_le_bytes());
        for &code in &item.phrase {
            content.extend_from_slice(&code.to_le_bytes());
        }
        for (keys, freq) in &item.prons {
            if keys.len() != phrase_len {
                return Err(ChunkWriteError(format!(
                    "chunk slot {slot:#010x} pronunciation has {} keys for a {}-character phrase",
                    keys.len(),
                    phrase_len
                )));
            }
            for key in keys {
                content.extend_from_slice(&key.to_le_bytes());
            }
            content.extend_from_slice(&freq.to_le_bytes());
        }
        total_freq += u64::from(item.unigram);
    }

    let total_freq = u32::try_from(total_freq)
        .map_err(|_| ChunkWriteError("chunk total_freq overflows u32".to_owned()))?;

    // ---- assemble payload: header, offset array, entry area ------------
    let slot_count = offsets.len();
    let index_one = INDEX_ONE;
    let index_two = index_one + u32::try_from(slot_count * 4).unwrap_or(u32::MAX) + 1;
    let index_three = index_two + u32::try_from(content.len()).unwrap_or(u32::MAX) + 1;

    let mut payload = Vec::with_capacity(usize::try_from(index_three).unwrap_or(0));
    payload.extend_from_slice(&total_freq.to_le_bytes());
    payload.extend_from_slice(&index_one.to_le_bytes());
    payload.extend_from_slice(&index_two.to_le_bytes());
    payload.extend_from_slice(&index_three.to_le_bytes());
    payload.push(SEPARATOR);
    for offset in &offsets {
        payload.extend_from_slice(&offset.to_le_bytes());
    }
    payload.push(SEPARATOR);
    payload.extend_from_slice(&content);
    payload.push(SEPARATOR);

    // ---- MemoryChunk header --------------------------------------------
    let length = u32::try_from(payload.len())
        .map_err(|_| ChunkWriteError("chunk payload overflows u32".to_owned()))?;
    let mut file = Vec::with_capacity(CHUNK_HEADER_SIZE + payload.len());
    file.extend_from_slice(&length.to_le_bytes());
    file.extend_from_slice(&chunk_checksum(&payload).to_le_bytes());
    file.extend_from_slice(&payload);
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A hand-built two-item chunk: verifies header, checksum, slot
    /// addressing, the 8-byte entry-area reservation, and item encoding.
    #[test]
    fn build_chunk_matches_hand_layout() {
        let items = vec![
            (
                1,
                ChunkItem {
                    phrase: vec![0x4f60], // 你
                    unigram: 3,
                    prons: vec![(vec![0x1234], 7)],
                },
            ),
            (
                3,
                ChunkItem {
                    phrase: vec![0x597d], // 好
                    unigram: 5,
                    prons: vec![(vec![0x5678], 11), (vec![0x5679], 2)],
                },
            ),
        ];
        let file = build_chunk(&items).expect("build");
        let (length, csum) = (
            u32::from_le_bytes(file[0..4].try_into().unwrap()),
            u32::from_le_bytes(file[4..8].try_into().unwrap()),
        );
        let payload = &file[8..];
        assert_eq!(usize::try_from(length).unwrap(), payload.len());
        assert_eq!(chunk_checksum(payload), csum);

        let total = u32::from_le_bytes(payload[0..4].try_into().unwrap());
        assert_eq!(total, 8); // 3 + 5
        let i1 = u32::from_le_bytes(payload[4..8].try_into().unwrap());
        let i2 = u32::from_le_bytes(payload[8..12].try_into().unwrap());
        let i3 = u32::from_le_bytes(payload[12..16].try_into().unwrap());
        assert_eq!(i1, 17);
        assert_eq!(payload[16], b'#');
        assert_eq!(payload[usize::try_from(i2).unwrap() - 1], b'#');
        assert_eq!(payload[usize::try_from(i3).unwrap() - 1], b'#');
        assert_eq!(i3 as usize, payload.len());

        // Offset array: 4 slots (0..3); slot 1 → 8, slot 3 → 8 + item0 size.
        let item0_len = 6 + 4 + (2 + 4);
        let offs: Vec<u32> = (0..4)
            .map(|s| {
                let p = usize::try_from(i1).unwrap() + s * 4;
                u32::from_le_bytes(payload[p..p + 4].try_into().unwrap())
            })
            .collect();
        assert_eq!(offs, vec![0, 8, 0, 8 + u32::try_from(item0_len).unwrap()]);

        // Item 0 at entry-area offset 8: header {1, 1, 3}, text, one pron.
        let e = usize::try_from(i2).unwrap();
        assert_eq!(payload[e..e + 8], [0; 8]);
        let p = e + 8;
        assert_eq!(&payload[p..p + 6], &[1, 1, 3, 0, 0, 0]);
        assert_eq!(
            u32::from_le_bytes(payload[p + 6..p + 10].try_into().unwrap()),
            0x4f60
        );
        assert_eq!(
            u16::from_le_bytes(payload[p + 10..p + 12].try_into().unwrap()),
            0x1234
        );
        assert_eq!(
            u32::from_le_bytes(payload[p + 12..p + 16].try_into().unwrap()),
            7
        );
    }

    /// The reader half of this crate must accept what this writer emits.
    #[test]
    fn build_chunk_reads_back_through_phrase_library() {
        let items = vec![(
            1,
            ChunkItem {
                phrase: vec![0x4f60, 0x597d],
                unigram: 9,
                prons: vec![(vec![0x0011, 0x0022], 9)],
            },
        )];
        let file = build_chunk(&items).expect("build");
        let dir = std::env::temp_dir().join(format!("oxpinyin-chunks-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("tmpdir");
        let path = dir.join("test.bin");
        std::fs::write(&path, &file).expect("write");
        let lib = crate::phrase_library::PhraseLibrary::open(&path).expect("open");
        assert_eq!(lib.total_freq(), 9);
        let item = lib.item(0x0000_0001).expect("item");
        assert_eq!(item.phrase_text().as_deref(), Some("你好"));
        assert_eq!(item.unigram(), 9);
        let pron = item.pronunciation(0).expect("pronunciation");
        assert_eq!(pron.keys, &[0x11_u8, 0x00, 0x22, 0x00]);
        assert_eq!(pron.freq, 9);
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    /// An empty library is the 16-byte header plus three separators — no
    /// 8-byte entry-area reservation, because `add_phrase_item` never ran.
    /// The bytes are the pin's own `gen_binary_files` output for an empty
    /// `.table` (the toned mini model's `culture.table`).
    #[test]
    fn build_chunk_of_no_items_matches_the_pin() {
        let file = build_chunk(&[]).expect("build");
        assert_eq!(
            file,
            [
                19, 0, 0, 0, // payload length
                51, 35, 35, 0, // checksum
                0, 0, 0, 0, // total_freq
                17, 0, 0, 0, // index_one
                18, 0, 0, 0, // index_two: no offset array
                19, 0, 0, 0, // index_three: no entry area
                b'#', b'#', b'#'
            ]
        );
        let dir =
            std::env::temp_dir().join(format!("oxpinyin-chunks-empty-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("tmpdir");
        let path = dir.join("empty.bin");
        std::fs::write(&path, &file).expect("write");
        let lib = crate::phrase_library::PhraseLibrary::open(&path).expect("open");
        assert_eq!(lib.total_freq(), 0);
        assert!(lib.item(0x0000_0001).is_none());
        assert_eq!(lib.items().count(), 0);
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn build_chunk_rejects_disorder_and_shape_errors() {
        let item = ChunkItem {
            phrase: vec![1],
            unigram: 1,
            prons: vec![(vec![0], 1)],
        };
        assert!(build_chunk(&[(2, item.clone()), (1, item.clone())]).is_err());
        let bad_keys = ChunkItem {
            prons: vec![(vec![0, 0], 1)],
            ..item.clone()
        };
        assert!(build_chunk(&[(1, bad_keys)]).is_err());
        let empty = ChunkItem {
            phrase: Vec::new(),
            ..item
        };
        assert!(build_chunk(&[(1, empty)]).is_err());
    }
}
