use super::*;

/// Basis: `074a2219 novel_types.h:44-46` (`PHRASE_INDEX_MAKE_TOKEN`, `PHRASE_INDEX_LIBRARY_INDEX`).
#[test]
fn make_token_and_library_index_match_macros() {
    assert_eq!(phrase_index_make_token(6, 1), 0x0600_0001);
    assert_eq!(phrase_index_make_token(7, 1), 0x0700_0001);
    assert_eq!(phrase_index_make_token(7, 0), 0x0700_0000);
    assert_eq!(phrase_index_library_index(0x0700_0001), 7);
    assert_eq!(phrase_index_library_index(0x0100_0001), 1);
    assert_eq!(phrase_index_library_index(1), 0);
}

/// Basis: `pinyin.cpp:3716-3719` (`pinyin_is_user_candidate`).
#[test]
fn user_tokens_are_distinguished_by_library_nibble() {
    assert!(is_user_token(0x0700_0001));
    assert!(!is_user_token(0x0500_0001));
}

/// Basis: `pinyin.cpp:589-594`, `phrase_index.cpp:620` (masked range end, reserved-id bump).
#[test]
fn a_full_library_wraps_its_allocation_as_the_pin_does() {
    assert_eq!(add_phrase_token(USER_DICTIONARY, 1), FIRST_USER_TOKEN);
    assert_eq!(add_phrase_token(USER_DICTIONARY, 0), FIRST_USER_TOKEN);
    assert_eq!(
        add_phrase_token(USER_DICTIONARY, PHRASE_MASK),
        phrase_index_make_token(USER_DICTIONARY, PHRASE_MASK)
    );
    assert_eq!(
        add_phrase_token(USER_DICTIONARY, PHRASE_MASK + 1),
        FIRST_USER_TOKEN
    );
}
