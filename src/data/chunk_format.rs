//! The `MemoryChunk` file format, written once for its two halves.
//!
//! libpinyin's per-library phrase-index files (`gb_char.bin`,
//! `gbk_char.bin`, `opengram.bin`, `merged.bin`, the addon `*.bin`
//! libraries) are one `MemoryChunk` each (`src/include/memory_chunk.h`):
//! an 8-byte header `{length: u32, checksum: u32}` then the payload,
//! with the payload's inner `SubPhraseIndex` layout owned by
//! [`crate::data::phrase_library`]. This module owns the frame and its checksum.
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

/// The `MemoryChunk` file header: `{length, checksum}`.
pub const CHUNK_HEADER_SIZE: usize = 8;

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
    for word in payload[..aligned].as_chunks::<4>().0.iter() {
        checksum ^= u32::from_le_bytes(*word);
    }
    let mut shift = 0_u32;
    for &byte in &payload[aligned..] {
        checksum ^= u32::from(byte) << shift;
        shift += 8;
    }
    checksum
}

/// Frames a payload into a complete `MemoryChunk` file — the 8-byte
/// `{length, checksum}` header over the payload, `MemoryChunk::save`'s
/// output.
///
/// The inverse of the header check every reader performs, and
/// shared framing policy for borrowed payloads. The owned variant below
/// applies the same header and overflow policy without a second buffer.
///
/// The payload length must fit in `u32`.
#[must_use]
pub fn build_memory_chunk(payload: &[u8]) -> Option<Vec<u8>> {
    let length = u32::try_from(payload.len()).ok()?;
    let mut file = Vec::with_capacity(CHUNK_HEADER_SIZE + payload.len());
    file.extend_from_slice(&length.to_le_bytes());
    file.extend_from_slice(&chunk_checksum(payload).to_le_bytes());
    file.extend_from_slice(payload);
    Some(file)
}

/// Frames an owned payload in place. Reserve eight additional bytes when
/// constructing the payload to avoid a second allocation for the header.
///
/// The payload length must fit in `u32`.
pub(crate) fn build_memory_chunk_owned(mut payload: Vec<u8>) -> Option<Vec<u8>> {
    let length = u32::try_from(payload.len()).ok()?;
    let checksum = chunk_checksum(&payload);
    let end = payload.len();
    payload.resize(end + CHUNK_HEADER_SIZE, 0);
    payload.copy_within(..end, CHUNK_HEADER_SIZE);
    payload[..4].copy_from_slice(&length.to_le_bytes());
    payload[4..CHUNK_HEADER_SIZE].copy_from_slice(&checksum.to_le_bytes());
    Some(payload)
}

#[cfg(test)]
mod tests;
