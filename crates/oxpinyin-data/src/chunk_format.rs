//! The `MemoryChunk` file format, written once for its two halves.
//!
//! libpinyin's per-library phrase-index files (`gb_char.bin`,
//! `gbk_char.bin`, `opengram.bin`, `merged.bin`, the addon `*.bin`
//! libraries) are one `MemoryChunk` each (`src/include/memory_chunk.h`):
//! an 8-byte header `{length: u32, checksum: u32}` then the payload,
//! with the payload's inner `SubPhraseIndex` layout owned by
//! [`crate::phrase_library`] (reader) and `oxpinyin-datagen::chunks`
//! (writer). This module holds the constants and the checksum both
//! halves share, so the reader's validation and the writer's emission
//! cannot drift apart — a wrong byte here makes a file the other half
//! rejects.
//!
//! Everything is little-endian on the supported targets (the fields are
//! host-endian upstream; every target oxpinyin and the pin share is
//! little-endian).

#[cfg(not(target_endian = "little"))]
compile_error!(
    "chunk_format: the MemoryChunk format uses host-endian fields; this \
     module encodes and decodes them as little-endian. Big-endian targets \
     are not supported."
);

/// `PHRASE_MASK` (`novel_types.h:41`): the library-local token bits a
/// phrase-index slot is addressed by.
pub const PHRASE_MASK: u32 = 0x00FF_FFFF;

/// `c_separate` (`novel_types.h:126`).
pub const SEPARATOR: u8 = b'#';

/// The `MemoryChunk` file header: `{length, checksum}`.
pub const CHUNK_HEADER_SIZE: usize = 8;

/// `SubPhraseIndex::store`'s `index_one`: the payload's `total_freq` plus
/// the three section offsets (`u32×4`) and the separator that closes
/// them, so the offset array starts at byte 17.
///
/// Constant in every
/// upstream file, and no alignment requirement rides on it — the loader
/// reads its `u32`s at unaligned offsets.
pub const INDEX_ONE: u32 = 17;

/// The entry area's first item offset.
///
/// `SubPhraseIndex::add_phrase_item`
/// bumps a zero content size to 8 on the first item, reserving bytes
/// `0..8` so that `0` stays the offset array's "no item" sentinel. A
/// library with no items reserves nothing and has an empty entry area
/// (measured on the pin's own `gen_binary_files` output for an empty
/// `.table`).
pub const FIRST_ITEM_OFFSET: u32 = 8;

/// `phrase_item_header` (`phrase_index.h:56`): `{u8 phrase_length,
/// u8 n_pronunciations, u32 unigram}` ahead of every item's UCS-4 text.
pub const ITEM_HEADER_SIZE: usize = 6;

/// `sizeof(ChewingKey)` — a 16-bit bitfield (`chewing_key.h:41`), the
/// width one pronunciation key occupies inside an item.
pub const CHEWING_KEY_SIZE: usize = 2;

/// `MemoryChunk::get_check_sum` (`memory_chunk.h:131-159`): the XOR of
/// the payload's little-endian `u32` words, with any tail bytes folded
/// in shifted by their position.
///
/// Reproduced exactly — the header's
/// checksum is what upstream verifies at `mmap` time, what the writer
/// stamps, and what the reader recomputes.
#[must_use]
pub fn chunk_checksum(payload: &[u8]) -> u32 {
    let mut checksum: u32 = 0;
    let aligned = payload.len() & !0x3;
    for word in payload[..aligned].chunks_exact(4) {
        checksum ^= u32::from_le_bytes([word[0], word[1], word[2], word[3]]);
    }
    let mut shift = 0_u32;
    for &byte in &payload[aligned..] {
        checksum ^= u32::from(byte) << shift;
        shift += 8;
    }
    checksum
}

/// A payload that does not fit the `MemoryChunk` header's `u32` length
/// field. The one framing policy this module owns: refuse rather than
/// clamp — a clamped length names a file the reader would misparse.
#[derive(Debug)]
pub struct ChunkFrameError(usize);

impl std::fmt::Display for ChunkFrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "payload of {} bytes exceeds the u32 length field",
            self.0
        )
    }
}

impl std::error::Error for ChunkFrameError {}

/// Frames a payload into a complete `MemoryChunk` file — the 8-byte
/// `{length, checksum}` header over the payload, `MemoryChunk::save`'s
/// output.
///
/// The inverse of the header check every reader performs, and
/// the **only** framing implementation: every chunk writer goes through
/// here, so the overflow policy is one policy.
///
/// # Errors
///
/// Fails when the payload exceeds the header's `u32` length field.
pub fn build_memory_chunk(payload: &[u8]) -> Result<Vec<u8>, ChunkFrameError> {
    let length = u32::try_from(payload.len()).map_err(|_| ChunkFrameError(payload.len()))?;
    let mut file = Vec::with_capacity(CHUNK_HEADER_SIZE + payload.len());
    file.extend_from_slice(&length.to_le_bytes());
    file.extend_from_slice(&chunk_checksum(payload).to_le_bytes());
    file.extend_from_slice(payload);
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `memory_chunk.h`'s own example shape: whole words XOR, tail bytes
    /// folded in little-endian position order. Pinned vectors so a change
    /// to the shared implementation breaks both halves' tests at once.
    #[test]
    fn checksum_matches_the_pinned_word_xor() {
        assert_eq!(chunk_checksum(&[]), 0);
        assert_eq!(chunk_checksum(&[1, 0, 0, 0]), 1);
        assert_eq!(chunk_checksum(&[1, 0, 0, 0, 2, 0, 0, 0]), 3);
        // Tail bytes shift up by 8 per position: 0x030201.
        assert_eq!(chunk_checksum(&[1, 2, 3]), 0x0003_0201);
        // A leading zero word contributes nothing; the tail byte is the
        // low byte of the checksum.
        assert_eq!(chunk_checksum(&[0, 0, 0, 0, 9]), 9);
    }

    #[test]
    fn framing_refuses_a_payload_beyond_the_length_field() {
        // A 2^32-byte payload cannot be named by the header's field;
        // the single framing refuses rather than clamping.
        let huge = vec![0_u8; u32::MAX as usize + 1];
        assert!(build_memory_chunk(&huge).is_err());
    }

    #[test]
    fn header_and_mask_are_the_upstream_values() {
        assert_eq!(CHUNK_HEADER_SIZE, 8);
        assert_eq!(SEPARATOR, b'#');
        assert_eq!(PHRASE_MASK, 0x00FF_FFFF);
    }

    /// `index_one` is the four header words plus their separator, and the
    /// item header is the two `u8` fields plus the `u32` unigram — the
    /// arithmetic the writer emits and the reader walks.
    #[test]
    fn sub_phrase_index_constants_follow_their_field_sums() {
        assert_eq!(INDEX_ONE as usize, 4 * 4 + 1);
        assert_eq!(FIRST_ITEM_OFFSET, 8);
        assert_eq!(ITEM_HEADER_SIZE, 1 + 1 + 4);
        assert_eq!(CHEWING_KEY_SIZE, 2);
    }

    #[test]
    fn build_memory_chunk_frames_the_pin_layout() {
        let payload = [1_u8, 2, 3];
        let file = build_memory_chunk(&payload).expect("frame");
        assert_eq!(file.len(), 8 + 3);
        assert_eq!(&file[..4], &3_u32.to_le_bytes());
        assert_eq!(&file[4..8], &0x0003_0201_u32.to_le_bytes());
        assert_eq!(&file[8..], &payload);
    }
}
