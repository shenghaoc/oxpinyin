use super::*;
use crate::parser::Completeness;
use crate::user::store::UserStore;
use crate::vocab::{NETWORK_DICTIONARY, USER_DICTIONARY};

fn key(text: &str) -> SyllableKey {
    SyllableKey::from_text(text).expect("frozen syllable")
}

/// A NULL-user-directory session over the stock layout.
fn transient() -> UserStore {
    UserStore::open_transient(
        crate::user::SystemOriginals::new(
            Default::default(),
            crate::data::user_files::UserFileLayout::stock(),
        ),
        crate::data::user_files::SystemVersions::for_this_build(7, 14),
        crate::user::UserConfLaw::Pinyin,
    )
    .unwrap()
}

/// Basis: `074a2219 pinyin.cpp:1627`, `phrase_index.h:647`, `phrase_index.cpp:186` (library nibble and slot mask).
#[test]
fn session_lookup_reads_a_tagged_token_through_its_masked_library() {
    let mut phrases = SessionPhrases::default();
    let token = crate::user::FIRST_USER_TOKEN;
    let bytes =
        crate::data::phrase_library::encode_phrase_item(&crate::data::phrase_library::ChunkItem {
            phrase: vec![u32::from('你')],
            unigram: 1,
            prons: Vec::new(),
        })
        .unwrap();
    phrases
        .libraries
        .get_or_default(USER_DICTIONARY)
        .unwrap()
        .replace(token & crate::vocab::PHRASE_MASK, &bytes)
        .unwrap();
    let keys = [crate::parser::ChewingKey::from_pinyin("ni").unwrap()];
    phrases.index.add(&keys, token);
    phrases.index.add(&keys, token | 0x8000_0000);
    let phrases = RefCell::new(phrases);
    let items = crate::user::system_items::SystemItems::default();
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

/// Basis: `chewing_large_table2.cpp:161-172` (stored token order); `phrase_index.h:647` (library by nibble).
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

/// Basis: `chewing_large_table2.cpp:184-197` (incomplete keyspace), `pinyin_phrase3.h:111-142` (with-tones compare).
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

    let n_partial = SyllableKey::from_text("n").expect("initial-only key");
    assert_eq!(n_partial.completeness(), Completeness::Partial);
    let entries = lookup.lookup(&[n_partial, key("hao")]);
    let texts: Vec<&str> = entries
        .iter()
        .filter_map(crate::vocab::PhraseEntry::text_str)
        .collect();
    assert!(texts.contains(&"你好"), "{texts:?}");
    assert!(texts.contains(&"那号"), "{texts:?}");
    assert!(!texts.contains(&"礼号"), "{texts:?}");
}
