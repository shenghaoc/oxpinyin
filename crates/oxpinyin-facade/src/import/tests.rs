//! The import iterator's length gate and add calls, shared by both C-ABI
//! facades.

use std::path::Path;

use oxpinyin_core::{PhraseText, SyllableKey};
use oxpinyin_user::{PinyinKey, USER_DICTIONARY, UserConfLaw, toned_key};

use crate::{ContextCore, ImportPlan, PINYIN_DEFAULT_OPTION_WORD};

/// The GBK system library of the committed mini tables.
const GBK: u8 = 2;

/// A context over the committed mini tables of this build's backend, with
/// a user directory of its own.
fn open(user: &Path) -> ContextCore {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/w3");
    ContextCore::open(
        &root.join(oxpinyin_data::DEFAULT_STORE_EXT),
        Some(user),
        PINYIN_DEFAULT_OPTION_WORD,
        UserConfLaw::Pinyin,
    )
    .expect("committed backend fixtures")
}

/// `count` keys of the syllable `a`, tone 1.
fn keys(count: usize) -> Vec<PinyinKey> {
    let syllable = SyllableKey::from_text("a").expect("syllable").index();
    vec![toned_key(syllable, 1).expect("key"); count]
}

#[rstest::rstest]
#[case::empty("", 0)]
#[case::fewer_keys("阿阿", 1)]
#[case::more_keys("阿", 2)]
#[case::sixteen_characters("阿阿阿阿阿阿阿阿阿阿阿阿阿阿阿阿", 16)]
fn the_length_gate_refuses_before_the_phrase_table(#[case] phrase: &str, #[case] count: usize) {
    let user = tempfile::tempdir().expect("user dir");
    let context = open(user.path());
    assert!(matches!(
        context.plan_import(USER_DICTIONARY, PhraseText::from(phrase), keys(count)),
        ImportPlan::Refuse
    ));
}

#[test]
fn a_committed_user_add_is_the_same_library_token_of_the_next_plan() {
    let user = tempfile::tempdir().expect("user dir");
    let mut context = open(user.path());
    let phrase = PhraseText::from("阿阿");
    let ImportPlan::Add {
        phrase: planned,
        keys: planned_keys,
        original: None,
    } = context.plan_import(USER_DICTIONARY, phrase.clone(), keys(2))
    else {
        panic!("a new user phrase is added with no same-library token");
    };
    assert!(context.commit_import(USER_DICTIONARY, &planned, &planned_keys, None, None));
    let ImportPlan::Add { original, .. } = context.plan_import(USER_DICTIONARY, phrase, keys(2))
    else {
        panic!("the phrase adds again");
    };
    let token = original.expect("the phrase table holds the committed add");
    assert_eq!(
        oxpinyin_user::phrase_index_library_index(token),
        USER_DICTIONARY
    );
}

#[test]
fn an_add_into_an_unloaded_system_library_is_refused() {
    let user = tempfile::tempdir().expect("user dir");
    let mut context = open(user.path());
    assert!(context.unload_phrase_library(GBK));
    let phrase = PhraseText::from("阿阿");
    assert!(!context.commit_import(GBK, &phrase, &keys(2), None, None));
}
