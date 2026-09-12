//! The per-library phrase-index chunk **writer** — the write half of the
//! `MemoryChunk` + `SubPhraseIndex` format `chunk_format` and
//! `phrase_library` read.
//!
//! This is the byte-level output of upstream's `SubPhraseIndex::store`
//! (`src/storage/phrase_index.cpp`), reached after `compact()` rebuilds a
//! sub-index in ascending token order. `oxpinyin-datagen` writes the
//! system libraries' files with it; the user store writes `user.bin`,
//! `addon.bin` and `network.bin` with it — the `USER_FILE` sub-indexes,
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

use crate::chunk_format::SEPARATOR;

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
        let offset = u32::try_from(content.len()).map_err(|_| {
            ChunkWriteError(format!("chunk slot {slot:#010x} offset overflows u32"))
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
    // The single framing implementation; the error type narrows here.
    crate::chunk_format::build_memory_chunk(&payload).map_err(|e| ChunkWriteError(e.to_string()))
}

/// Serialises one item into the `PhraseItem` wire form.
///
/// The entry-area encoding above and the `PhraseIndexLogger`'s record
/// payloads, which carry whole items (`append_record`'s
/// `oldone`/`newone` chunks).
///
/// The shape contract is [`build_chunk`]'s, checked here too because a
/// logger record with a short pronunciation run would decode wrong:
/// `decode_phrase_item` reads `2 * phrase_length` key bytes per
/// pronunciation, so a missing key byte shifts the frequency field into
/// key data — silently wrong on replay, not an error.
///
/// # Errors
///
/// Fails on an empty or over-`u8::MAX` phrase, more than `u8::MAX`
/// pronunciations, or any pronunciation whose key run length differs
/// from the phrase length.
pub fn encode_phrase_item(item: &ChunkItem) -> Result<Vec<u8>, ChunkWriteError> {
    let phrase_length = item.phrase.len();
    if phrase_length == 0 || phrase_length > usize::from(u8::MAX) {
        return Err(ChunkWriteError(format!(
            "phrase item: phrase length {phrase_length} out of range"
        )));
    }
    if item.prons.len() > usize::from(u8::MAX) {
        return Err(ChunkWriteError(format!(
            "phrase item: {} pronunciations",
            item.prons.len()
        )));
    }
    for (keys, _) in &item.prons {
        if keys.len() != phrase_length {
            return Err(ChunkWriteError(format!(
                "phrase item: pronunciation has {} keys for a {phrase_length}-character phrase",
                keys.len()
            )));
        }
    }
    let mut bytes = Vec::new();
    bytes.push(u8::try_from(phrase_length).unwrap_or(u8::MAX));
    bytes.push(u8::try_from(item.prons.len()).unwrap_or(u8::MAX));
    bytes.extend_from_slice(&item.unigram.to_le_bytes());
    for &code in &item.phrase {
        bytes.extend_from_slice(&code.to_le_bytes());
    }
    for (keys, freq) in &item.prons {
        for key in keys {
            bytes.extend_from_slice(&key.to_le_bytes());
        }
        bytes.extend_from_slice(&freq.to_le_bytes());
    }
    Ok(bytes)
}

/// Decodes one `PhraseItem` wire form — the inverse of
/// [`encode_phrase_item`], for the logger payloads' replay.
///
/// # Errors
///
/// Fails when the header claims a shape that does not fit the bytes
/// (length, pronunciation count, or run lengths).
pub fn decode_phrase_item(bytes: &[u8]) -> Result<ChunkItem, ChunkWriteError> {
    let phrase_length = *bytes
        .first()
        .ok_or_else(|| ChunkWriteError("phrase item: no phrase length byte".to_owned()))?
        as usize;
    let n_prons = *bytes
        .get(1)
        .ok_or_else(|| ChunkWriteError("phrase item: no pronunciation count byte".to_owned()))?
        as usize;
    let mut offset = 2;
    let unigram = bytes
        .get(offset..offset + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| ChunkWriteError("phrase item: no unigram".to_owned()))?;
    offset += 4;

    let phrase_end = offset + 4 * phrase_length;
    let phrase_area = bytes
        .get(offset..phrase_end)
        .ok_or_else(|| ChunkWriteError("phrase item: phrase text truncated".to_owned()))?;
    let phrase: Vec<u32> = phrase_area
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    offset = phrase_end;

    let mut prons = Vec::with_capacity(n_prons);
    for _ in 0..n_prons {
        let keys_end = offset + 2 * phrase_length;
        let keys_area = bytes.get(offset..keys_end).ok_or_else(|| {
            ChunkWriteError("phrase item: pronunciation keys truncated".to_owned())
        })?;
        let keys: Vec<u16> = keys_area
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        offset = keys_end;
        let freq = bytes
            .get(offset..offset + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .ok_or_else(|| {
                ChunkWriteError("phrase item: pronunciation freq truncated".to_owned())
            })?;
        offset += 4;
        prons.push((keys, freq));
    }

    Ok(ChunkItem {
        phrase,
        unigram,
        prons,
    })
}

/// Decodes a `SubPhraseIndex` payload — the reader half of
/// [`build_chunk`]'s layout, at the writer's home so both stay beside
/// the format they share.
///
/// Returns the library's `total_freq` and its
/// items in ascending slot order.
///
/// This is the shape inside every per-library chunk file (`gb_char.bin`
/// and friends, `user.bin`, `addon.bin`), after the `MemoryChunk` frame
/// ([`crate::user_files::read_chunk_payload`]); the runtime's
/// mmap-backed reader (`phrase_library`) validates the same layout.
///
/// # Errors
///
/// Fails when the header, separators or offset bounds do not hold, or
/// an item's stored shape overruns the entry area.
pub fn decode_sub_phrase_index(
    payload: &[u8],
) -> Result<(u32, Vec<(u32, ChunkItem)>), ChunkWriteError> {
    let header = payload
        .get(..16)
        .ok_or_else(|| ChunkWriteError("sub phrase index: short header".to_owned()))?;
    let u32_at = |offset: usize| -> u32 {
        u32::from_le_bytes([
            header[offset],
            header[offset + 1],
            header[offset + 2],
            header[offset + 3],
        ])
    };
    let total = u32_at(0);
    let index_one = u32_at(4) as usize;
    let index_two = u32_at(8) as usize;
    let index_three = u32_at(12) as usize;

    if payload.get(16) != Some(&SEPARATOR) {
        return Err(ChunkWriteError(
            "sub phrase index: no lead separator".to_owned(),
        ));
    }
    if index_one != 17 {
        return Err(ChunkWriteError(format!(
            "sub phrase index: index_one {index_one} is not the header end"
        )));
    }
    let _ = index_two
        .checked_sub(1)
        .and_then(|at| payload.get(at))
        .filter(|&&byte| byte == SEPARATOR)
        .ok_or_else(|| ChunkWriteError("sub phrase index: no offset separator".to_owned()))?;
    let _ = index_three
        .checked_sub(1)
        .and_then(|at| payload.get(at))
        .filter(|&&byte| byte == SEPARATOR)
        .ok_or_else(|| ChunkWriteError("sub phrase index: no entry separator".to_owned()))?;
    if index_three > payload.len() {
        return Err(ChunkWriteError(
            "sub phrase index: past the payload end".to_owned(),
        ));
    }

    let offset_array = payload
        .get(index_one..index_two - 1)
        .ok_or_else(|| ChunkWriteError("sub phrase index: no offset array".to_owned()))?;
    let entry_area = payload
        .get(index_two..index_three - 1)
        .ok_or_else(|| ChunkWriteError("sub phrase index: no entry area".to_owned()))?;

    let mut items = Vec::new();
    for (slot, window) in offset_array.chunks_exact(4).enumerate() {
        let offset = u32::from_le_bytes([window[0], window[1], window[2], window[3]]) as usize;
        if offset == 0 {
            continue;
        }
        let bytes = entry_area.get(offset..).ok_or_else(|| {
            ChunkWriteError(format!("sub phrase index: slot {slot} offset out of range"))
        })?;
        let item = decode_phrase_item(bytes)
            .map_err(|error| ChunkWriteError(format!("sub phrase index: slot {slot}: {error}")))?;
        items.push((u32::try_from(slot).unwrap_or(u32::MAX), item));
    }
    Ok((total, items))
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
        assert_eq!(crate::chunk_format::chunk_checksum(payload), csum);

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

    #[test]
    fn sub_phrase_index_round_trips_through_build_chunk() {
        let items = vec![
            (
                1,
                ChunkItem {
                    phrase: vec![0x4f60],
                    unigram: 3,
                    prons: vec![(vec![0x1234], 7)],
                },
            ),
            (
                3,
                ChunkItem {
                    phrase: vec![0x597d],
                    unigram: 5,
                    prons: vec![(vec![0x5678], 11)],
                },
            ),
        ];
        let file = build_chunk(&items).expect("build");
        let payload = read_frame(&file);
        let (total, decoded) = decode_sub_phrase_index(payload).expect("decode");
        assert_eq!(total, 8);
        assert_eq!(decoded, items);

        // The pin's own empty-library bytes (19-byte payload) decode to
        // an empty library.
        let empty = build_chunk(&[]).expect("build");
        let (total, decoded) = decode_sub_phrase_index(read_frame(&empty)).expect("decode");
        assert_eq!(total, 0);
        assert!(decoded.is_empty());

        // Hostile payloads answer typed errors: a broken separator and a
        // truncated payload.
        let mut broken = payload.to_vec();
        broken[16] = b'!';
        assert!(decode_sub_phrase_index(&broken).is_err());
        assert!(decode_sub_phrase_index(&payload[..payload.len() - 1]).is_err());
    }

    fn read_frame(file: &[u8]) -> &[u8] {
        crate::user_files::read_chunk_payload(file).expect("frame")
    }

    #[test]
    fn phrase_item_codec_round_trips_and_matches_the_entry_area() {
        let item = ChunkItem {
            phrase: vec![0x4f60, 0x597d],
            unigram: 483,
            prons: vec![(vec![0x1234, 0x5678], 7), (vec![0x1235, 0x5679], 2)],
        };
        let bytes = encode_phrase_item(&item).expect("item encodes");
        // {u8 len, u8 npron, u32 unigram, 2×u32 text, 2×(2×u16 keys, u32 freq)}
        assert_eq!(bytes.len(), 6 + 8 + 2 * (4 + 4));
        assert_eq!(&bytes[..6], &[2, 2, 0xe3, 0x01, 0x00, 0x00]);
        assert_eq!(decode_phrase_item(&bytes).expect("decode"), item);

        // The entry area a built chunk stores for the item decodes the
        // same way — one wire form, two containers.
        let file = build_chunk(&[(1, item.clone())]).expect("build");
        let body = &file[crate::chunk_format::CHUNK_HEADER_SIZE..];
        let index_two =
            usize::try_from(u32::from_le_bytes(body[8..12].try_into().unwrap())).unwrap();
        let entry_area = &body[index_two..];
        assert_eq!(&entry_area[8..8 + bytes.len()], &bytes[..]);
    }

    #[test]
    fn phrase_item_decode_rejects_hostile_bytes() {
        assert!(decode_phrase_item(&[]).is_err());
        assert!(decode_phrase_item(&[2, 0, 1, 0, 0, 0]).is_err()); // no text
        // A pron run that overruns.
        assert!(decode_phrase_item(&[1, 1, 1, 0, 0, 0, 1, 0, 0, 0, 0x34]).is_err());
    }
}
