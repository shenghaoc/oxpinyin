//! Native import planning (074a2219 pinyin.cpp:536-608, zhuyin.cpp:422-492), and the import
//! iterator's length gate and add calls, shared by both C-ABI facades.
// Read faults need nextest's separate process for each test.
use super::*;
use crate::facade::test_fixture::{native_fixture::*, *};
use crate::user::{PinyinKey, UserConfLaw, toned_key};
use crate::vocab::{PhraseText, SyllableKey, USER_DICTIONARY};

/// The GBK system library of the committed mini tables.
const GBK: u8 = 2;

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
    let context = native_context(&system_dir(), user.path(), UserConfLaw::Pinyin);
    assert!(matches!(
        context.plan_import(USER_DICTIONARY, PhraseText::from(phrase), keys(count)),
        ImportPlan::Refuse
    ));
}

#[test]
fn a_committed_user_add_is_the_same_library_token_of_the_next_plan() {
    let user = tempfile::tempdir().expect("user dir");
    let mut context = native_context(&system_dir(), user.path(), UserConfLaw::Pinyin);
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
        crate::vocab::phrase_index_library_index(token),
        USER_DICTIONARY
    );
}

#[test]
fn an_add_into_an_unloaded_system_library_is_refused() {
    let user = tempfile::tempdir().expect("user dir");
    let mut context = native_context(&system_dir(), user.path(), UserConfLaw::Pinyin);
    assert!(context.unload_phrase_library(GBK));
    let phrase = PhraseText::from("阿阿");
    assert!(!context.commit_import(GBK, &phrase, &keys(2), None, None));
}

#[test]
fn an_import_finds_the_token_of_its_library_under_high_bits() {
    let user = TempUserDir::new("tagged-import");
    let token = saved_user_phrase(&user.path);
    set_phrase_table_row(&user.path, "龘龘", &[token | 0x8000_0000]);
    let mut context = native_context(&system_dir(), &user.path, UserConfLaw::Pinyin);
    assert_eq!(tokens(&context, "龘龘"), [token | 0x8000_0000]);
    assert!(
        matches!(plan(&context, 7, "龘龘", "da4'da4"), ImportPlan::Add {original: Some(found), ..} if found == token | 0x8000_0000)
    );
    assert_eq!(
        import_native(&mut context, 7, &[("龘龘", "da4'da4", 5)]),
        [true]
    );
    assert_eq!(tokens(&context, "龘龘"), [token | 0x8000_0000]);
    assert_eq!(
        context.user_store().user_pronunciation_count(token),
        Some(2)
    );
}

#[test]
fn import_refuses_two_tokens_of_one_library_under_high_bits() {
    let user = TempUserDir::new("tagged-import-duplicate");
    native_context(&system_dir(), &user.path, UserConfLaw::Pinyin).finish();
    set_phrase_table_row(&user.path, "你好", &[0x0700_0001, 0x8700_0002]);
    let context = native_context(&system_dir(), &user.path, UserConfLaw::Pinyin);
    assert!(matches!(
        plan(&context, 7, "你好", "ni'hao"),
        ImportPlan::Abort
    ));
}

#[test]
fn an_import_refuses_five_tokens_before_the_library_refusal() {
    let user = TempUserDir::new("combined-unused-index");
    let mut context = native_context(&system_dir(), &user.path, UserConfLaw::Pinyin);
    for library in 1..=5 {
        assert_eq!(
            import_native(&mut context, library, &[("你好", "ni3'hao3", 100_000)]),
            [true]
        );
    }
    assert!(matches!(
        plan(&context, 8, "你好", "ni3'hao3"),
        ImportPlan::Abort
    ));
}

#[rstest::rstest]
#[case::pinyin(UserConfLaw::Pinyin, "测测", "ce4'ce4")]
#[case::zhuyin(UserConfLaw::Zhuyin, "你你", "ㄋㄧˇ ㄋㄧˇ")]
fn a_failed_phrase_table_read_is_a_miss(
    #[case] law: UserConfLaw,
    #[case] text: &str,
    #[case] reading: &str,
) {
    let user = TempUserDir::new("failed-phrase-read");
    let mut context = native_context(&system_dir(), &user.path, law);
    let key: Vec<u8> = text
        .chars()
        .flat_map(|word| u32::from(word).to_le_bytes())
        .collect();
    crate::store::fail_point_read_of(&key, 0);
    assert!(matches!(
        plan(&context, 7, text, reading),
        ImportPlan::Add { original: None, .. }
    ));
    assert_eq!(
        import_native(&mut context, 7, &[(text, reading, 5)]),
        [true]
    );
}

#[test]
fn adding_a_user_phrase_again_after_a_reload_refuses_the_missing_item() {
    let user = TempUserDir::new("reload-reuse");
    let mut context = native_context(&system_dir(), &user.path, UserConfLaw::Zhuyin);
    assert_eq!(
        import_native(&mut context, 7, &[("好你", "ㄏㄠˇ ㄋㄧˇ", 50)]),
        [true]
    );
    assert!(context.unload_phrase_library(7));
    assert_eq!(context.load_phrase_library(7), Ok(true));
    assert!(matches!(
        plan(&context, 7, "好你", "ㄏㄠˇ ㄋㄧˇ"),
        ImportPlan::Abort
    ));
}

#[rstest::rstest]
#[case::pinyin_length(UserConfLaw::Pinyin, "你好嗷", "ni'hao'ao", "nihao")]
#[case::pinyin_text(UserConfLaw::Pinyin, "你們", "ni'men", "nihao")]
#[case::zhuyin_text(UserConfLaw::Zhuyin, "你們", "ㄋㄧˇ ㄇㄣˊ", "ㄋㄧˇ ㄏㄠˇ")]
fn an_index_token_with_another_item_refuses_import(
    #[case] law: UserConfLaw,
    #[case] other: &str,
    #[case] reading: &str,
    #[case] imported: &str,
) {
    let user = TempUserDir::new("redirected-import");
    let mut context = native_context(&system_dir(), &user.path, law);
    assert_eq!(
        import_native(&mut context, 7, &[(other, reading, 1)]),
        [true]
    );
    let token = context
        .user_store()
        .token_for_phrase(other)
        .unwrap()
        .unwrap();
    assert_eq!(context.save_user(), crate::facade::SaveOutcome::Saved);
    context.finish();
    drop(context);
    set_phrase_table_row(&user.path, "你好", &[token]);
    let context = native_context(&system_dir(), &user.path, law);
    assert!(matches!(
        plan(&context, 7, "你好", imported),
        ImportPlan::Abort
    ));
}

#[test]
fn an_import_into_a_slot_without_a_sub_index_is_refused() {
    let user = TempUserDir::new("import-no-sub-index");
    let mut context = native_context(&system_dir(), &user.path, UserConfLaw::Pinyin);
    for library in [0, 15, 16, 255] {
        assert!(matches!(
            plan(&context, library, "你好", "nihao"),
            ImportPlan::Refuse
        ));
        assert_eq!(
            import_native(&mut context, library, &[("你好", "nihao", 1)]),
            [false]
        );
    }
    assert_eq!(export_native(&context, 7), []);
}

#[test]
fn a_user_file_row_at_a_system_index_is_read_and_imported_into() {
    use crate::data::phrase_library::{ChunkItem, build_chunk};
    let system = TempSystemDir::new("gb-user-file");
    let conf = std::fs::read_to_string(system.path.join("table.conf")).unwrap();
    let row = "default GB_DICTIONARY gb_char.table gb_char.bin gb_char.dbin SYSTEM_FILE";
    assert!(conf.contains(row));
    system.write(
        "table.conf",
        &conf.replace(row, "default GB_DICTIONARY NULL NULL gb_user.bin USER_FILE"),
    );
    let user = TempUserDir::new("gb-user-file-user");
    write_user_conf(&user.path, &system.path);
    std::fs::write(
        user.path.join("gb_user.bin"),
        build_chunk(&[(
            1,
            ChunkItem {
                phrase: vec![u32::from('你')],
                unigram: 17,
                prons: vec![(vec![0x302b], 5)],
            },
        )])
        .unwrap(),
    )
    .unwrap();
    let mut context = native_context(&system.path, &user.path, UserConfLaw::Pinyin);
    assert_eq!(unigram(&context, 0x0100_0001), Some(17));
    assert_eq!(
        context.runtime.dict().token_text(0x0100_0001),
        Some("你".into())
    );
    assert_eq!(unigram(&context, 0x0100_0002), None);
    assert_eq!(
        import_native(&mut context, 1, &[("测测", "ce4'ce4", 5)]),
        [true]
    );
    assert!(unigram(&context, 0x0100_0002).is_some());
}

#[cfg(feature = "tkrzw")]
#[test]
fn an_import_into_an_unused_library_after_five_refuses() {
    let user = TempUserDir::new("unused-index");
    let mut context = native_context(
        &contract_data::contract_data_dir(),
        &user.path,
        UserConfLaw::Zhuyin,
    );
    for library in 1..=5 {
        assert_eq!(
            import_native(&mut context, library, &[("你好", "ㄋㄧˇ ㄏㄠˇ", 100_000)]),
            [true]
        );
    }
    assert!(matches!(
        plan(&context, 8, "你好", "ㄋㄧˇ ㄏㄠˇ"),
        ImportPlan::Abort
    ));
}
