use super::*;

#[test]
fn round_trips_and_matches_the_layout() {
    let value = encode_single_gram(22080, &[(1, 69), (0x0100_0002, 138)]);
    assert_eq!(value.len(), 4 + 2 * 8);
    assert_eq!(&value[..4], &22080_u32.to_le_bytes());
    assert_eq!(&value[4..8], &1_u32.to_le_bytes());
    assert_eq!(&value[8..12], &69_u32.to_le_bytes());
    let (total, records) = decode_single_gram(&value).expect("decode");
    assert_eq!(total, 22080);
    assert_eq!(records, vec![(1, 69), (0x0100_0002, 138)]);
}

#[test]
fn rejects_short_and_unaligned_blobs() {
    assert!(decode_single_gram(&[]).is_none());
    assert!(decode_single_gram(&[0; 3]).is_none());
    // A 5-byte blob: total present, one stray byte.
    assert!(decode_single_gram(&[0, 0, 0, 0, 0]).is_none());
    // The empty gram — a bare total — is valid.
    assert_eq!(
        decode_single_gram(&[7, 0, 0, 0]).expect("decode"),
        (7, vec![])
    );
}
