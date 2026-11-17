use super::c_string;

#[test]
fn c_strings_stop_at_the_first_nul() {
    assert_eq!(
        c_string(b"abc\0def").map(|text| text.into_bytes()),
        Some(b"abc".to_vec())
    );
    assert_eq!(
        c_string(b"abc").map(|text| text.into_bytes()),
        Some(b"abc".to_vec())
    );
}
