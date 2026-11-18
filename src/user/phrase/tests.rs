use super::*;
use crate::{ADDON_DICTIONARY, NETWORK_DICTIONARY};

#[test]
fn user_file_libraries_are_nibbles_5_to_7() {
    // The three USER_FILE sub-indexes of the default facade
    // (addon.bin / network.bin / user.bin, phrase-union.md §3.2).
    assert_eq!(ADDON_DICTIONARY, 5);
    assert_eq!(NETWORK_DICTIONARY, 6);
    assert_eq!(USER_DICTIONARY, 7);
}

#[test]
fn make_token_and_library_index_match_macros() {
    assert_eq!(phrase_index_make_token(6, 1), 0x0600_0001);
    assert_eq!(phrase_index_make_token(7, 1), 0x0700_0001);
    assert_eq!(phrase_index_make_token(7, 0), 0x0700_0000);
    assert_eq!(phrase_index_library_index(0x0700_0001), 7);
    assert_eq!(phrase_index_library_index(0x0100_0001), 1);
    assert_eq!(phrase_index_library_index(1), 0);
}

#[test]
fn user_tokens_are_distinguished_by_library_nibble() {
    // pinyin_is_user_candidate: PHRASE_INDEX_LIBRARY_INDEX == USER_DICTIONARY.
    assert!(is_user_token(0x0700_0001));
    assert!(is_user_token(0x0700_0002));
    assert!(!is_user_token(0x0100_0001)); // GB_DICTIONARY
    assert!(!is_user_token(0x0200_0001)); // GBK_DICTIONARY
    assert!(!is_user_token(0x0500_0001)); // ADDON_DICTIONARY
    assert!(!is_user_token(1)); // sentence_start
    assert!(!is_user_token(0));
}

/// The session's allocation has no upper boundary (register row 119):
/// the pin masks the range end into the nibble and bumps a zero low
/// word, so a full library wraps to slot 1, and the promotion's bare
/// mask wraps to slot 0.
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
    assert_eq!(
        phrase_index_make_token(ADDON_DICTIONARY, PHRASE_MASK + 1),
        phrase_index_make_token(ADDON_DICTIONARY, 0)
    );
}
