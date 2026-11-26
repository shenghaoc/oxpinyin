use super::*;

// 074a2219 memory_chunk.h:137-163 folds whole words and the final partial word.
#[test]
fn checksum_matches_the_pinned_word_xor() {
    for (payload, checksum) in [
        (&[][..], 0),
        (&[1, 2, 3], 0x0003_0201),
        (&[1, 0, 0, 0], 1),
        (&[0, 0, 0, 0, 9], 9),
        (&[1, 0, 0, 0, 2, 0, 0, 0], 3),
    ] {
        assert_eq!(chunk_checksum(payload), checksum);
    }
}

// Rust build_memory_chunk rejects payload lengths outside u32.
#[test]
fn framing_refuses_a_payload_beyond_the_length_field() {
    let huge = vec![0_u8; u32::MAX as usize + 1];
    assert!(build_memory_chunk(&huge).is_none());
}

// 074a2219 memory_chunk.h:536-557 writes length, checksum and payload.
#[test]
fn build_memory_chunk_frames_the_pin_layout() {
    let payload = [1_u8, 2, 3];
    let file = build_memory_chunk(&payload).expect("frame");
    assert_eq!(file.len(), 8 + 3);
    assert_eq!(&file[..4], &3_u32.to_le_bytes());
    assert_eq!(&file[4..8], &0x0003_0201_u32.to_le_bytes());
    assert_eq!(&file[8..], &payload);
    let mut owned = Vec::with_capacity(payload.len() + CHUNK_HEADER_SIZE);
    owned.resize(CHUNK_HEADER_SIZE, 0);
    owned.extend_from_slice(&payload);
    let framed = finish_memory_chunk(owned).expect("finish frame");
    assert_eq!(framed, file);
}

// The 16-byte block fold gives the word-by-word XOR of memory_chunk.h:137-163
// for every length and start, so every block, word and tail path is reached.
#[test]
fn checksum_blocks_equal_the_word_by_word_xor() {
    fn word_by_word(payload: &[u8]) -> u32 {
        let aligned = payload.len() & !0x3;
        let mut checksum = payload[..aligned]
            .as_chunks::<4>()
            .0
            .iter()
            .fold(0, |checksum, word| checksum ^ u32::from_le_bytes(*word));
        for (index, &byte) in payload[aligned..].iter().enumerate() {
            checksum ^= u32::from(byte) << (8 * index);
        }
        checksum
    }
    let bytes: Vec<u8> = (0_u32..300)
        .map(|n| (n.wrapping_mul(97) ^ (n >> 3)) as u8)
        .collect();
    for start in 0..4 {
        for end in start..bytes.len() {
            let payload = &bytes[start..end];
            assert_eq!(
                chunk_checksum(payload),
                word_by_word(payload),
                "{start}..{end}"
            );
        }
    }
}
