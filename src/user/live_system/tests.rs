use super::*;
use crate::data::chunk_write::{ChunkItem, build_chunk};

/// Policy row 107: phrase_index.cpp:180-206 at 074a2219 can hand out
/// an item extending past its buffer. The approved safe outcome is no
/// item, never resurrection of the valid original in a separate mapping.
#[test]
fn an_overlong_live_item_never_falls_back_to_a_shipped_copy() {
    use crate::data::{
        phrase_library::PhraseLibrary,
        user_files::{LogRecord, SystemVersions, UserTableInfo, encode_log_records},
    };
    use crate::user::persistence::{SystemLibrary, SystemOriginals, UserConfLaw};
    use std::collections::BTreeMap;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("system.bin");
    let item = ChunkItem {
        phrase: vec![0x4f60],
        unigram: 1,
        prons: Vec::new(),
    };
    std::fs::write(&path, build_chunk(&[(1, item.clone())]).unwrap()).unwrap();
    let mut shipped = PhraseLibraries::empty();
    shipped.load(1, &path).unwrap();
    let original = PhraseLibrary::open(&path).unwrap();
    let originals = SystemOriginals::from(crate::user::LibraryTable::from([(
        1,
        SystemLibrary {
            total: original.total_freq(),
            range_end: original.token_range().end,
            mapped: Some(original),
            items: BTreeMap::new(),
        },
    )]));
    let old = crate::data::chunk_write::encode_phrase_item(&item).unwrap();
    let mut over = old.clone();
    over[0] = 255;
    over[1] = 255;
    let payload = encode_log_records(&[LogRecord::Modify {
        token: 1,
        old_item: old,
        new_item: over,
    }])
    .unwrap();
    let versions = SystemVersions::for_this_build(7, 14);
    let user = dir.path().join("user");
    std::fs::create_dir(&user).unwrap();
    std::fs::write(
        user.join("user.conf"),
        UserTableInfo::conform_to(&versions).to_text(),
    )
    .unwrap();
    std::fs::write(
        user.join("gb_char.dbin"),
        crate::data::chunk_format::build_memory_chunk(&payload).unwrap(),
    )
    .unwrap();
    let store =
        crate::user::test_session::open(&user, originals, versions, UserConfLaw::Pinyin).unwrap();
    assert!(store.live_item_past_buffer(0x0100_0001));
    let unloaded = Cell::new(0);
    let live = LiveSystemItems::new(&shipped, &store, &unloaded);
    assert_eq!(
        live.read(0x0100_0001, |item| item.unigram()),
        Lookup::Absent
    );
    assert_eq!(shipped.unigram_count(0x0100_0001), Some(1));
}

/// A session whose `table.conf` makes library `nibble`, named `row`, a
/// `SYSTEM_FILE` holding one item at slot 1 with unigram 1.
fn session_over_system_library(
    dir: &std::path::Path,
    nibble: u8,
    row: &str,
) -> (PhraseLibraries, UserStore) {
    use crate::data::{
        phrase_library::PhraseLibrary,
        user_files::{SystemVersions, UserTableInfo},
    };
    use crate::user::persistence::{SystemLibrary, SystemOriginals, UserConfLaw};
    use std::collections::BTreeMap;
    let path = dir.join("six.bin");
    let item = ChunkItem {
        phrase: vec![0x4f60],
        unigram: 1,
        prons: Vec::new(),
    };
    std::fs::write(&path, build_chunk(&[(1, item)]).unwrap()).unwrap();
    let mut libraries = PhraseLibraries::empty();
    assert!(libraries.load(nibble, &path).unwrap());
    let original = PhraseLibrary::open(&path).unwrap();
    // A table.conf that makes the library a `SYSTEM_FILE`.
    let conf_path = dir.join("table.conf");
    std::fs::write(
        &conf_path,
        format!(
            "binary format version:7\nmodel data version:14\n\
                 lambda parameter:0.312699\n\nsource table format:pinyin\n\
                 database format:BerkeleyDB\n\n\
                 default {row} six.table six.bin six.dbin SYSTEM_FILE\n"
        ),
    )
    .unwrap();
    let conf = crate::data::table_info::TableConf::load_path(&conf_path).unwrap();
    let originals = SystemOriginals::new(
        crate::user::LibraryTable::from([(
            nibble,
            SystemLibrary {
                total: original.total_freq(),
                range_end: original.token_range().end,
                mapped: Some(original),
                items: BTreeMap::new(),
            },
        )]),
        crate::data::user_files::UserFileLayout::from_conf(&conf),
    );
    let versions = SystemVersions::for_this_build(7, 14);
    let user = dir.join("user");
    std::fs::create_dir(&user).unwrap();
    std::fs::write(
        user.join("user.conf"),
        UserTableInfo::conform_to(&versions).to_text(),
    )
    .unwrap();
    let store =
        crate::user::test_session::open(&user, originals, versions, UserConfLaw::Pinyin).unwrap();
    (libraries, store)
}

/// 074a2219 pinyin.cpp:337-367 loads the libraries named by table.conf;
/// the configured library index is not restricted to the stock 1..=4.
#[test]
fn a_nonstock_loaded_library_is_a_system_library() {
    let dir = tempfile::tempdir().unwrap();
    let (libraries, store) = session_over_system_library(dir.path(), 6, "NETWORK_DICTIONARY");
    let unloaded = Cell::new(0);
    let live = LiveSystemItems::new(&libraries, &store, &unloaded);
    assert!(live.is_system_library(6));
    assert!(!live.is_system_library(5));
    assert_eq!(
        live.read(0x0600_0001, |item| item.unigram()),
        Lookup::Found(1)
    );
}

/// With library 5 a `SYSTEM_FILE`, the promotion's token is the end of
/// that sub-index's range (`pinyin.cpp:2541-2544` at the pin
/// `074a2219`) and `add_phrase_item` puts the item in that sub-index,
/// whose total takes the unigram (`pinyin.cpp:2558`,
/// `phrase_index.h:663-671`, `phrase_index.cpp:208-217`). No second
/// library 5 exists beside it.
#[test]
fn a_promotion_into_a_system_library_5_writes_that_sub_index() {
    let dir = tempfile::tempdir().unwrap();
    let (libraries, mut store) = session_over_system_library(dir.path(), 5, "ADDON_DICTIONARY");
    let keys = ["er", "huang"].map(|pinyin| {
        crate::user::toned_key(crate::SyllableKey::from_text(pinyin).unwrap().index(), 0).unwrap()
    });
    let token =
        crate::user::test_session::promote(&mut store, "二簧", &[(keys.to_vec(), 100)], 100)
            .unwrap();
    assert_eq!(token, 0x0500_0002, "the chunk's range ends past slot 1");
    assert!(!store.has_user_library(5), "no arena beside the sub-index");
    // `SubPhraseIndex::get_phrase_index_total_freq` answers the
    // sub-index's `m_total_freq` (`phrase_index.cpp:146-148`), which
    // `add_phrase_item` raised by the item's unigram (`:215`).
    assert_eq!(crate::user::test_session::library_total(&store, 5), 101);
    let unloaded = Cell::new(0);
    let live = LiveSystemItems::new(&libraries, &store, &unloaded);
    assert_eq!(live.read(token, |item| item.unigram()), Lookup::Found(100));
    assert_eq!(
        live.read(0x0500_0001, |item| item.unigram()),
        Lookup::Found(1),
        "the chunk's own item stays"
    );
}

#[test]
fn live_items_distinguish_found_absent_and_unloaded() {
    use crate::ChewingKey;
    use crate::data::chunk_write::encode_phrase_item;
    use crate::data::phrase_library::PhraseLibrary;
    use crate::data::user_files::{LogRecord, SystemVersions, UserTableInfo, encode_log_records};
    use crate::user::persistence::{SystemLibrary, SystemOriginals, UserConfLaw};
    use crate::user::{ItemOrigin, SystemItem};
    use std::collections::BTreeMap;

    let ni = ChewingKey::new(23, 0, 1, 0).to_packed();
    let item = |word: u32, unigram: u32, frequency: u32| ChunkItem {
        phrase: vec![word],
        unigram,
        prons: vec![(vec![ni], frequency)],
    };
    let shipped = [
        (1, item(0x4f60, 100, 100)),
        (2, item(0x597d, 50, 50)),
        (3, item(0x4e16, 30, 30)),
    ];
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gb_char.bin");
    std::fs::write(&path, build_chunk(&shipped).expect("chunk")).expect("write chunk");
    let library = PhraseLibrary::open(&path).expect("open");
    let total = library.total_freq();
    let originals: SystemOriginals = crate::user::LibraryTable::from([(
        1_u8,
        SystemLibrary {
            total,
            range_end: library.token_range().end,
            items: BTreeMap::new(),
            mapped: Some(library),
        },
    )])
    .into();

    let mut modified = shipped[0].1.clone();
    modified.prons[0].1 += 7;
    let records = [
        LogRecord::ModifyHeader {
            old_total: total,
            new_total: total,
        },
        LogRecord::Modify {
            token: 1,
            old_item: encode_phrase_item(&shipped[0].1).expect("old"),
            new_item: encode_phrase_item(&modified).expect("new"),
        },
        LogRecord::Remove {
            token: 2,
            old_item: encode_phrase_item(&shipped[1].1).expect("removed"),
        },
    ];
    let payload = encode_log_records(&records).expect("log");
    let versions = SystemVersions::for_this_build(7, 14);
    let user = dir.path().join("user");
    std::fs::create_dir(&user).expect("user dir");
    std::fs::write(
        user.join("gb_char.dbin"),
        crate::data::chunk_format::build_memory_chunk(&payload).expect("chunk"),
    )
    .expect("write log");
    std::fs::write(
        user.join("user.conf"),
        UserTableInfo::conform_to(&versions).to_text(),
    )
    .expect("write marker");
    let store = crate::user::test_session::open(&user, originals, versions, UserConfLaw::Pinyin)
        .expect("open");

    let unloaded = Cell::new(0);
    let live = LiveSystemItems::new(store.originals().phrase_libraries(), &store, &unloaded);
    let (replayed, removed, untouched, absent) = (
        crate::phrase_index_make_token(1, 1),
        crate::phrase_index_make_token(1, 2),
        crate::phrase_index_make_token(1, 3),
        crate::phrase_index_make_token(1, 9),
    );
    let summary = |item: SystemItem<'_>| {
        let reading = item.pronunciation(0).expect("reading");
        (
            item.origin(),
            item.phrase_length(),
            item.n_pronunciations(),
            reading.frequency,
            reading.keys.iter().collect::<Vec<_>>(),
        )
    };
    // Every item of a library with a live buffer is read from it: the
    // replayed one as the replay wrote it, an untouched one as the chunk
    // stored it.
    assert_eq!(
        live.read(replayed, summary),
        Lookup::Found((ItemOrigin::Overlay, 1, 1, 107, vec![ni]))
    );
    assert_eq!(
        live.read(untouched, summary),
        Lookup::Found((ItemOrigin::Overlay, 1, 1, 30, vec![ni]))
    );
    assert_eq!(
        live.read(untouched, |item| item.phrase_text())
            .found()
            .flatten(),
        PhraseText::from_words([0x4e16])
    );
    assert_eq!(live.read(removed, summary), Lookup::Absent);
    assert_eq!(live.read(absent, summary), Lookup::Absent);
    assert_eq!(
        live.read(crate::phrase_index_make_token(5, 1), summary),
        Lookup::Absent
    );
    assert!(live.is_visible(untouched) && !live.is_visible(removed));

    // The header-only count agrees with the view for every state.
    assert_eq!(live.pronunciation_count(replayed), Lookup::Found(1));
    assert_eq!(live.pronunciation_count(untouched), Lookup::Found(1));
    assert_eq!(live.pronunciation_count(removed), Lookup::Absent);
    assert_eq!(live.pronunciation_count(absent), Lookup::Absent);

    // The stored field and the live count are separate reads: the stored
    // one is the chunk's; the live one adds the store's delta once.
    assert_eq!(live.effective_unigram(untouched), Some(30));
    assert_eq!(live.effective_unigram(absent), None);
    assert_eq!(live.effective_unigram(untouched), Some(30));
    assert_eq!(
        live.effective_unigram(replayed),
        Some(u64::from(100_u32) + store.unigram_delta(replayed))
    );
    assert_eq!(live.effective_unigram(removed), None);
    assert_eq!(live.effective_unigram(absent), None);

    // The store's own accessors answer the same.
    assert_eq!(
        live.read(replayed, |item| item.pronunciation(0).map(|r| r.frequency))
            .found()
            .flatten(),
        store.with_effective_system_item(replayed, |item| item.prons[0].1)
    );
    assert!(store.system_item_removed(removed));

    // An unloaded library hides every token, whatever it holds.
    let unloaded = Cell::new(1 << 1);
    let hidden = LiveSystemItems::new(store.originals().phrase_libraries(), &store, &unloaded);
    for token in [replayed, removed, untouched, absent] {
        assert_eq!(hidden.read(token, summary), Lookup::Unloaded);
        assert_eq!(hidden.pronunciation_count(token), Lookup::Unloaded);
        assert!(!hidden.is_visible(token));
        assert_eq!(hidden.effective_unigram(token), None);
    }
}
