//! Per-library phrase-index chunk files (`MemoryChunk` + `SubPhraseIndex`),
//! the `gb_char.bin` / `gbk_char.bin` / `opengram.bin` / `merged.bin` and
//! addon `*.bin` files a libpinyin installation ships.
//!
//! This is the Rust equivalent of the pinned upstream writer pair
//! (`utils/storage/gen_binary_files` + `src/storage/phrase_index.cpp`):
//! `FacadePhraseIndex::load_text` builds one `SubPhraseIndex` per library,
//! `compact()` rebuilds it in ascending token order, and
//! `SubPhraseIndex::store` serialises the result. The byte layout this
//! module emits reproduces `store` exactly:
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
//!   zero content size to 8 before the first write); item offsets in the
//!   offset array are relative to the entry-area start.
//! * Each item is `{ u8 phrase_length, u8 n_pronunciations, u32 unigram,
//!   ucs4_t phrase[L], { ChewingKey u16[L], u32 freq } × n_pronunciations }`
//!   (`phrase_item_header`, `phrase_index.h:56`; `sizeof(ChewingKey) == 2`).
//! * Items appear in ascending slot order — the order `compact()`'s token
//!   walk feeds `add_phrase_item`, which appends to the entry area.
//!
//! The checksum is `MemoryChunk::get_check_sum`: the XOR of the payload's
//! little-endian `u32` words with tail bytes folded in shifted by position.
//! It is recomputed by the runtime reader (`oxpinyin-data`'s
//! `phrase_library`), so a wrong byte here makes the file unloadable.

use crate::DatagenError;

/// `PHRASE_MASK` (`novel_types.h:41`): the library-local token bits a
/// chunk slot is addressed by.
pub const PHRASE_MASK: u32 = 0x00FF_FFFF;
/// `c_separate` (`novel_types.h:126`).
const SEPARATOR: u8 = b'#';
/// Header `total_freq` + three offsets, then the first separator: where
/// the offset array starts (`SubPhraseIndex::store`).
const INDEX_ONE: u32 = 17;
/// `add_phrase_item` reserves the first 8 entry-area bytes by bumping a
/// zero content size to 8; the first real item lives at offset 8.
const FIRST_ITEM_OFFSET: u32 = 8;

/// One phrase entry of a library chunk.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChunkItem {
    /// The phrase text as UCS-4 code points (`ucs4_t` array).
    pub phrase: Vec<u32>,
    /// The item's unigram frequency — the `\1-gram` count plus the one
    /// `gen_unigram` adds for every library token.
    pub unigram: u32,
    /// Pronunciations in `.table` row order: packed `ChewingKey` sequence
    /// (`to_packed`, little-endian on disk) and the summed row frequency
    /// (`PhraseItem::add_pronunciation` sums duplicate exact key
    /// sequences).
    pub prons: Vec<(Vec<u16>, u32)>,
}

impl ChunkItem {
    /// The serialised item size: header + UCS-4 text + per-pronunciation
    /// key runs and frequency (`get_phrase_item`'s length arithmetic).
    #[must_use]
    pub fn byte_len(&self) -> usize {
        let len = self.phrase.len();
        6 + 4 * len
            + self
                .prons
                .iter()
                .map(|(keys, _)| 2 * keys.len() + 4)
                .sum::<usize>()
    }
}

/// `MemoryChunk::get_check_sum` (`memory_chunk.h:131-159`).
fn checksum(payload: &[u8]) -> u32 {
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
pub fn build_chunk(items: &[(u32, ChunkItem)]) -> Result<Vec<u8>, DatagenError> {
    // ---- validate and serialise the entry area -------------------------
    let mut content: Vec<u8> = vec![0; usize::try_from(FIRST_ITEM_OFFSET).unwrap_or(0)];
    let mut offsets: Vec<u32> = Vec::new();
    let mut total_freq: u64 = 0;
    let mut last_slot: Option<u32> = None;

    for &(slot, ref item) in items {
        if slot > PHRASE_MASK {
            return Err(DatagenError::Consistency(format!(
                "chunk slot {slot:#010x} exceeds PHRASE_MASK"
            )));
        }
        if let Some(prev) = last_slot.filter(|&p| slot <= p) {
            return Err(DatagenError::Consistency(format!(
                "chunk slots not ascending: {slot:#010x} after {prev:#010x}"
            )));
        }
        last_slot = Some(slot);

        let phrase_len = item.phrase.len();
        if phrase_len == 0 || phrase_len > usize::from(u8::MAX) {
            return Err(DatagenError::Consistency(format!(
                "chunk slot {slot:#010x} phrase length {} out of range",
                item.phrase.len()
            )));
        }
        if item.prons.len() > usize::from(u8::MAX) {
            return Err(DatagenError::Consistency(format!(
                "chunk slot {slot:#010x} has {} pronunciations",
                item.prons.len()
            )));
        }

        let offset = u32::try_from(content.len()).map_err(|_| {
            DatagenError::Consistency(format!("chunk slot {slot:#010x} offset overflows u32"))
        })?;
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
                return Err(DatagenError::Consistency(format!(
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
        .map_err(|_| DatagenError::Consistency("chunk total_freq overflows u32".to_owned()))?;

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
        .map_err(|_| DatagenError::Consistency("chunk payload overflows u32".to_owned()))?;
    let mut file = Vec::with_capacity(8 + payload.len());
    file.extend_from_slice(&length.to_le_bytes());
    file.extend_from_slice(&checksum(&payload).to_le_bytes());
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
        assert_eq!(checksum(payload), csum);

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

    /// The reader in `oxpinyin-data` must accept what this writer emits.
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
        let lib = oxpinyin_data::phrase_library::PhraseLibrary::open(&path).expect("open");
        assert_eq!(lib.total_freq(), 9);
        let item = lib.item(0x0000_0001).expect("item");
        assert_eq!(item.phrase_text().as_deref(), Some("你好"));
        assert_eq!(item.unigram(), 9);
        let pron = item.pronunciation(0).expect("pronunciation");
        assert_eq!(pron.keys, &[0x11_u8, 0x00, 0x22, 0x00]);
        assert_eq!(pron.freq, 9);
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
