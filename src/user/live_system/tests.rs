use super::*;
use crate::data::phrase_library::{ChunkItem, build_chunk};

/// Basis: policy row 107 (`074a2219 phrase_index.cpp:180-206`: an over-long item answers no item).
#[test]
fn an_overlong_live_item_never_falls_back_to_a_shipped_copy() {
    use crate::data::{
        phrase_library::PhraseLibrary,
        user_files::{LogRecord, SystemVersions, UserTableInfo, encode_log_records},
    };
    use crate::user::{SystemLibrary, SystemOriginals, UserConfLaw};
    use std::collections::BTreeMap;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("system.bin");
    let item = ChunkItem {
        phrase: vec![0x4f60],
        unigram: 1,
        prons: Vec::new(),
    };
    std::fs::write(&path, build_chunk(&[(1, &item)]).unwrap()).unwrap();
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
    let old = crate::data::phrase_library::encode_phrase_item(&item).unwrap();
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
    assert!(UserTableInfo::conform_to(&versions).save(user.join("user.conf").as_path()));
    std::fs::write(
        user.join("gb_char.dbin"),
        crate::data::chunk_format::build_memory_chunk(&payload).unwrap(),
    )
    .unwrap();
    let store =
        crate::user::test_session::open(&user, originals, versions, UserConfLaw::Pinyin).unwrap();
    let unloaded = Cell::new(0);
    let live = LiveSystemItems::new(&shipped, &store, &unloaded);
    assert_eq!(live.pronunciation_count(0x0100_0001).found(), None);
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
    use crate::user::{SystemLibrary, SystemOriginals, UserConfLaw};
    use std::collections::BTreeMap;
    let path = dir.join("six.bin");
    let item = ChunkItem {
        phrase: vec![0x4f60],
        unigram: 1,
        prons: Vec::new(),
    };
    std::fs::write(&path, build_chunk(&[(1, &item)]).unwrap()).unwrap();
    let mut libraries = PhraseLibraries::empty();
    assert!(libraries.load(nibble, &path).unwrap());
    let original = PhraseLibrary::open(&path).unwrap();
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
    let conf = crate::data::TableConf::load_path(&conf_path).unwrap();
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
    assert!(UserTableInfo::conform_to(&versions).save(user.join("user.conf").as_path()));
    let store =
        crate::user::test_session::open(&user, originals, versions, UserConfLaw::Pinyin).unwrap();
    (libraries, store)
}

/// Basis: `074a2219 pinyin.cpp:337-367` (any `table.conf` row is a system library).
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

/// Basis: `074a2219 pinyin.cpp:2541-2558`, `phrase_index.h:663-671`, `phrase_index.cpp:146-148,208-217`.
#[test]
fn a_promotion_into_a_system_library_5_writes_that_sub_index() {
    let dir = tempfile::tempdir().unwrap();
    let (libraries, mut store) = session_over_system_library(dir.path(), 5, "ADDON_DICTIONARY");
    let keys = ["er", "huang"].map(|pinyin| {
        crate::user::toned_key(
            crate::vocab::SyllableKey::from_text(pinyin)
                .unwrap()
                .index(),
            0,
        )
        .unwrap()
    });
    let token =
        crate::user::test_session::promote(&mut store, "二簧", &[(keys.to_vec(), 100)], 100)
            .unwrap();
    assert_eq!(token, 0x0500_0002);
    assert!(!store.has_user_library(5));
    assert_eq!(crate::user::test_session::library_total(&store, 5), 101);
    let unloaded = Cell::new(0);
    let live = LiveSystemItems::new(&libraries, &store, &unloaded);
    assert_eq!(live.read(token, |item| item.unigram()), Lookup::Found(100));
    assert_eq!(
        live.read(0x0500_0001, |item| item.unigram()),
        Lookup::Found(1)
    );
}

/// Basis: `phrase_index.cpp:180-206,477-501` (live buffer reads), Rust contract of `Lookup`.
#[test]
fn live_items_distinguish_found_absent_and_unloaded() {
    use crate::data::phrase_library::PhraseLibrary;
    use crate::data::phrase_library::encode_phrase_item;
    use crate::data::user_files::{LogRecord, SystemVersions, UserTableInfo, encode_log_records};
    use crate::parser::ChewingKey;
    use crate::user::{ItemOrigin, SystemItem};
    use crate::user::{SystemLibrary, SystemOriginals, UserConfLaw};
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
    std::fs::write(
        &path,
        build_chunk(&shipped.each_ref().map(|(slot, item)| (*slot, item))).expect("chunk"),
    )
    .expect("write chunk");
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
    assert!(UserTableInfo::conform_to(&versions).save(user.join("user.conf").as_path()));
    let store = crate::user::test_session::open(&user, originals, versions, UserConfLaw::Pinyin)
        .expect("open");

    let unloaded = Cell::new(0);
    let live = LiveSystemItems::new(store.originals().phrase_libraries(), &store, &unloaded);
    let (replayed, removed, untouched, absent) = (
        crate::vocab::phrase_index_make_token(1, 1),
        crate::vocab::phrase_index_make_token(1, 2),
        crate::vocab::phrase_index_make_token(1, 3),
        crate::vocab::phrase_index_make_token(1, 9),
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
        live.read(crate::vocab::phrase_index_make_token(5, 1), summary),
        Lookup::Absent
    );
    assert!(live.is_visible(untouched) && !live.is_visible(removed));

    assert_eq!(live.pronunciation_count(replayed), Lookup::Found(1));
    assert_eq!(live.pronunciation_count(untouched), Lookup::Found(1));
    assert_eq!(live.pronunciation_count(removed), Lookup::Absent);
    assert_eq!(live.pronunciation_count(absent), Lookup::Absent);

    assert_eq!(live.effective_unigram(untouched), Some(30));
    assert_eq!(
        live.effective_unigram(replayed),
        Some(u64::from(100_u32) + store.unigram_delta(replayed))
    );
    assert_eq!(live.effective_unigram(removed), None);
    assert_eq!(live.effective_unigram(absent), None);

    assert_eq!(
        live.read(replayed, |item| item.pronunciation(0).map(|r| r.frequency))
            .found()
            .flatten(),
        store.with_effective_system_item(replayed, |item| item.prons[0].1)
    );
    assert!(store.system_item_removed(removed));

    let unloaded = Cell::new(1 << 1);
    let hidden = LiveSystemItems::new(store.originals().phrase_libraries(), &store, &unloaded);
    assert_eq!(hidden.read(untouched, summary), Lookup::Unloaded);
    assert_eq!(hidden.pronunciation_count(untouched), Lookup::Unloaded);
    assert!(!hidden.is_visible(untouched));
    assert_eq!(hidden.effective_unigram(untouched), None);
}
