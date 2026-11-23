use super::*;

#[test]
fn round_trips_and_matches_the_layout() {
    // Pin 074a2219: ngram.cpp:36–74,133–151; Rust encode_single_gram preserves supplied record order.
    let value = encode_single_gram(22080, &[(1, 69), (0x0100_0002, 138)]);
    assert_eq!(value.len(), 4 + 2 * 8);
    assert_eq!(&value[..4], &22080_u32.to_le_bytes());
    assert_eq!(&value[4..8], &1_u32.to_le_bytes());
    assert_eq!(&value[8..12], &69_u32.to_le_bytes());
    assert_eq!(get_total_freq(&value), Some(22080));
    assert_eq!(
        retrieve_all(&value).collect::<Vec<_>>(),
        [(1, 69), (0x0100_0002, 138)]
    );
}

#[rstest::rstest]
#[case::short(&[0; 3], false)]
#[case::empty_gram(&[7, 0, 0, 0], true)]
#[case::partial_record(&[0; 5], false)]
fn rejects_short_and_unaligned_blobs(#[case] value: &[u8], #[case] complete: bool) {
    // Rust layout contract: a four-byte total followed by whole eight-byte records.
    assert_eq!(is_complete(value), complete);
}
