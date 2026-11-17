use super::{PhraseIndex, SystemPhraseIndex};
use oxpinyin_user::data::PhraseLibraries;

#[test]
fn sentence_start_is_the_special_table() {
    let phrases = SystemPhraseIndex {
        libraries: PhraseLibraries::empty(),
    };
    assert_eq!(phrases.token_to_string(1), Some(b"<start>".to_vec()));
    assert_eq!(phrases.token_to_string(2), None);
    assert!(phrases.validate_token_with_string(1, b"<start>"));
    assert!(!phrases.validate_token_with_string(2, b"x"));
}
