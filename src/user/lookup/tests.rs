use super::*;
use crate::user::store::UserStore;
use crate::{Completeness, NETWORK_DICTIONARY, USER_DICTIONARY};

fn key(text: &str) -> SyllableKey {
    SyllableKey::from_text(text).expect("frozen syllable")
}

/// The pin's NULL-user-directory session over the stock layout: the
/// user and network libraries, and no system library.
fn transient() -> UserStore {
    UserStore::open_transient(
        crate::user::persistence::SystemOriginals::new(
            Default::default(),
            crate::data::user_files::UserFileLayout::stock(),
        ),
        crate::data::user_files::SystemVersions::for_this_build(7, 14),
        crate::user::persistence::UserConfLaw::Pinyin,
    )
    .unwrap()
}

/// `_token_get_phrase` reads the item `get_phrase_item` finds
/// (`pinyin.cpp:1627` at the pin `074a2219`), which selects the library
/// with `PHRASE_INDEX_LIBRARY_INDEX` and the slot with `PHRASE_MASK`
/// (`phrase_index.h:647`, `phrase_index.cpp:186`). An index token with
/// high bits set reads the same item and keeps its own value.
#[test]
fn session_lookup_reads_a_tagged_token_through_its_masked_library() {
    let mut phrases = SessionPhrases::default();
    let token = crate::user::FIRST_USER_TOKEN;
    let bytes =
        crate::data::chunk_write::encode_phrase_item(&crate::data::chunk_write::ChunkItem {
            phrase: vec![u32::from('你')],
            unigram: 1,
            prons: Vec::new(),
        })
        .unwrap();
    phrases
        .libraries
        .get_or_default(USER_DICTIONARY)
        .unwrap()
        .replace(token & crate::PHRASE_MASK, &bytes)
        .unwrap();
    let keys = [crate::ChewingKey::from_pinyin("ni").unwrap()];
    phrases.index.add(&keys, token);
    phrases.index.add(&keys, token | 0x8000_0000);
    let phrases = RefCell::new(phrases);
    let items = RefCell::new(crate::user::system_items::SystemItems::default());
    let lookup = UserLookup::session(&phrases, &items);
    assert_eq!(lookup.search_toned(&keys), [token, token | 0x8000_0000]);
    let entries = lookup.lookup(&[key("ni")]);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].token(), token);
    assert_eq!(entries[0].text(), "你");
    assert_eq!(entries[1].token(), token | 0x8000_0000);
    assert_eq!(entries[1].text(), "你");
    assert_eq!(
        lookup.phrase_text(token | 0x8000_0000).unwrap().as_str(),
        Some("你")
    );
}

#[test]
fn lookup_surfaces_user_then_network_by_token() {
    let mut store = transient();
    let ni = u16::try_from(SyllableKey::from_text("ni").unwrap().index())
        .expect("frozen syllable inventory fits u16");
    store
        .add_phrase_in(USER_DICTIONARY, &"你".into(), &[ni], Some(5))
        .unwrap();
    store
        .add_phrase_in(NETWORK_DICTIONARY, &"拟".into(), &[ni], Some(5))
        .unwrap();
    let lookup = store.lookup();
    let entries = lookup.lookup(&[key("ni")]);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].text(), "拟");
    assert_eq!(entries[1].text(), "你");
    assert!(lookup.phrase_prefix_exists(&[key("ni")]));
}

/// `_add_phrase` takes `ChewingKey`s (`pinyin.cpp:514-519`), each of
/// which names a syllable; a dense id that names none has no packed
/// form, so the session refuses the phrase before it stores or indexes
/// anything, and the readings that do convert index as before.
#[test]
fn a_reading_with_an_unconvertible_syllable_is_refused_before_it_is_stored() {
    let mut store = transient();
    let ni = u16::try_from(SyllableKey::from_text("ni").unwrap().index())
        .expect("frozen syllable inventory fits u16");
    let hao = u16::try_from(SyllableKey::from_text("hao").unwrap().index())
        .expect("frozen syllable inventory fits u16");
    let unknown = crate::user::phrase::KEY_SYLLABLE_MASK;
    assert!(SyllableKey::from_index(usize::from(unknown)).is_none());
    assert!(matches!(
        store.add_phrase_in(USER_DICTIONARY, &"你好".into(), &[ni, unknown], Some(5)),
        Err(crate::user::UserStoreError::InvalidPhrase)
    ));
    store
        .add_phrase_in(USER_DICTIONARY, &"好好".into(), &[hao, hao], Some(5))
        .unwrap();
    let lookup = store.lookup();
    assert!(lookup.lookup(&[key("ni")]).is_empty());
    assert!(!lookup.phrase_prefix_exists(&[key("ni")]));
    assert_eq!(lookup.lookup(&[key("hao"), key("hao")]).len(), 1);
    assert_eq!(store.phrases().len(), 1);
}

/// `lookup` with an incomplete syllable at the head of the query must
/// find every user entry whose stored first-syllable initial matches
/// the query, without callers enumerating completions — the
/// incomplete-projected index mirrors upstream's
/// incomplete keyspace at `chewing_large_table2.cpp:184-197`. Regresses
/// shenghaoc/oxpinyin#403's fix.
#[test]
fn lookup_answers_a_single_syllable_incomplete_query_from_by_initial() {
    let mut store = transient();
    let ni = u16::try_from(SyllableKey::from_text("ni").unwrap().index())
        .expect("frozen syllable inventory fits u16");
    let na = u16::try_from(SyllableKey::from_text("na").unwrap().index())
        .expect("frozen syllable inventory fits u16");
    let hao = u16::try_from(SyllableKey::from_text("hao").unwrap().index())
        .expect("frozen syllable inventory fits u16");
    store
        .add_phrase_in(USER_DICTIONARY, &"你".into(), &[ni], Some(5))
        .unwrap();
    store
        .add_phrase_in(USER_DICTIONARY, &"那".into(), &[na], Some(5))
        .unwrap();
    store
        .add_phrase_in(USER_DICTIONARY, &"号".into(), &[hao], Some(5))
        .unwrap();
    let lookup = store.lookup();

    let n_partial =
        SyllableKey::from_text("n").expect("initial-only 'n' is in the incomplete inventory");
    assert_eq!(
        n_partial.completeness(),
        Completeness::Partial,
        "'n' resolves to the initial-only key"
    );
    let entries = lookup.lookup(&[n_partial]);
    let texts: Vec<&str> = entries
        .iter()
        .filter_map(crate::PhraseEntry::text_str)
        .collect();
    assert!(texts.contains(&"你"), "expected 你 in {texts:?}");
    assert!(texts.contains(&"那"), "expected 那 in {texts:?}");
    assert!(
        !texts.contains(&"号"),
        "hao does not start with n: {texts:?}"
    );
}

/// A two-syllable query where the first key is incomplete and the
/// second is complete must still filter by the second syllable
/// (`keys_match`'s complete-only rule at
/// `src/data/chewing_table.rs:431-451`). Regresses
/// shenghaoc/oxpinyin#403's fix.
#[test]
fn lookup_filters_mixed_incomplete_and_complete_syllables() {
    let mut store = transient();
    let ni = u16::try_from(SyllableKey::from_text("ni").unwrap().index())
        .expect("frozen syllable inventory fits u16");
    let na = u16::try_from(SyllableKey::from_text("na").unwrap().index())
        .expect("frozen syllable inventory fits u16");
    let hao = u16::try_from(SyllableKey::from_text("hao").unwrap().index())
        .expect("frozen syllable inventory fits u16");
    let li = u16::try_from(SyllableKey::from_text("li").unwrap().index())
        .expect("frozen syllable inventory fits u16");
    // Two matching phrases: (ni, hao) and (na, hao); one non-matching
    // (li, hao) — starts with 'l' not 'n'.
    store
        .add_phrase_in(USER_DICTIONARY, &"你好".into(), &[ni, hao], Some(5))
        .unwrap();
    store
        .add_phrase_in(USER_DICTIONARY, &"那号".into(), &[na, hao], Some(5))
        .unwrap();
    store
        .add_phrase_in(USER_DICTIONARY, &"礼号".into(), &[li, hao], Some(5))
        .unwrap();
    let lookup = store.lookup();

    let n_partial =
        SyllableKey::from_text("n").expect("initial-only 'n' is in the incomplete inventory");
    assert_eq!(
        n_partial.completeness(),
        Completeness::Partial,
        "'n' resolves to the initial-only key"
    );
    let entries = lookup.lookup(&[n_partial, key("hao")]);
    let texts: Vec<&str> = entries
        .iter()
        .filter_map(crate::PhraseEntry::text_str)
        .collect();
    assert!(texts.contains(&"你好"), "expected 你好 in {texts:?}");
    assert!(texts.contains(&"那号"), "expected 那号 in {texts:?}");
    assert!(
        !texts.contains(&"礼号"),
        "礼 starts with l, must not match n-partial: {texts:?}"
    );
}
