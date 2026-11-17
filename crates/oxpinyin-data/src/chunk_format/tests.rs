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
fn build_memory_chunk_frames_the_pin_layout() {
    let payload = [1_u8, 2, 3];
    let file = build_memory_chunk(&payload).expect("frame");
    assert_eq!(file.len(), 8 + 3);
    assert_eq!(&file[..4], &3_u32.to_le_bytes());
    assert_eq!(&file[4..8], &0x0003_0201_u32.to_le_bytes());
    assert_eq!(&file[8..], &payload);
    let mut owned = Vec::with_capacity(payload.len() + CHUNK_HEADER_SIZE);
    owned.extend_from_slice(&payload);
    let framed = build_memory_chunk_owned(owned).expect("frame owned");
    assert_eq!(framed, file);
}
