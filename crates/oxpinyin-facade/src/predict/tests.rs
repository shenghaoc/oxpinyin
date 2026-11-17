use super::*;
use crate::{ContextCore, PINYIN_DEFAULT_OPTION_WORD};
use oxpinyin_core::SyllableKey;
use oxpinyin_user::{UserConfLaw, toned_key};

/// 074a2219 pinyin.cpp:1413-1415 and :2389-2395 search the current
/// phrase table on every prefix/prediction call, including after import.
#[test]
fn prediction_reads_imports_after_its_first_query() {
    let user = tempfile::tempdir().unwrap();
    let system = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/w3")
        .join(oxpinyin_data::DEFAULT_STORE_EXT);
    let mut ctx = ContextCore::open(
        &system,
        Some(user.path()),
        PINYIN_DEFAULT_OPTION_WORD,
        UserConfLaw::Pinyin,
    )
    .unwrap();
    let prefix = PhraseText::from("龘");
    let dict = ctx.runtime.dict();
    assert!(merged_suggestions(&dict, ctx.user_store(), &prefix).is_empty());
    let key = toned_key(SyllableKey::from_text("da").unwrap().index(), 2).unwrap();
    let token = ctx
        .user_store_mut()
        .add_phrase(&PhraseText::from("龘龘"), &[key, key], Some(7))
        .unwrap();
    let rows = merged_suggestions(&ctx.runtime.dict(), ctx.user_store(), &prefix);
    assert!(rows.iter().any(|(found, _)| *found == token), "{rows:?}");
}
