// Nextest isolates the process-wide copy-fault hooks by running each test in its own process.
use super::*;
use crate::data::phrase_library::encode_phrase_item;
use crate::data::user_files::OPEN_COUNTER_LIMIT;
use crate::user::test_session::borrowed_items;
use crate::user::test_session::tempdir;
use crate::user::test_session::{
    load, load_checked, load_profile, save_state as save_state_reporting,
};

/// A changed chunk with one item; at 128 readings the item exceeds the journal's u16 length.
fn changed_baseline(readings: usize) -> SaveBaseline {
    SaveBaseline::Changed(SystemLibrary {
        items: BTreeMap::from([(
            1,
            item(&[0x4f60; 255], 1, &vec![(vec![1; 255], 1); readings]),
        )]),
        total: 1,
        range_end: 2,
        mapped: None,
    })
}

/// Basis: `pinyin.cpp:933-1020,1141-1144` (stage, then rename), `zhuyin.cpp:585-617` (rename each file).
#[rstest::rstest]
#[case::remap(SaveBaseline::Unmappable, None, UserConfLaw::Pinyin)]
#[case::journal_length(changed_baseline(128), None, UserConfLaw::Zhuyin)]
#[case::copy_size(changed_baseline(1), Some(512), UserConfLaw::Pinyin)]
fn hard_save_stops_after_the_earlier_library(
    #[case] stop: SaveBaseline,
    #[case] limit: Option<u64>,
    #[case] law: UserConfLaw,
) {
    let dir = tempfile::tempdir().unwrap();
    let libraries = crate::user::LibraryTable::from([
        (1, SystemLibrary::default()),
        (2, SystemLibrary::default()),
    ])
    .into();
    let mut store =
        crate::user::test_session::open(dir.path(), libraries, versions(), law).unwrap();
    store.mark_modified();
    std::fs::write(dir.path().join(USER_CONF), b"previous marker").unwrap();
    let _limit = limit.map(crate::user::user_library::TestLimits::set);
    let remap = matches!(stop, SaveBaseline::Unmappable);
    let mut stop = Some(stop);
    let mut visited = Vec::new();
    let result = store.save_reporting_with_baseline(&[], |library, _| {
        visited.push(library);
        if library == 2 {
            stop.take().unwrap()
        } else {
            SaveBaseline::Keep
        }
    });
    let error = result.unwrap_err();
    assert_eq!(
        matches!(error, crate::user::UserStoreError::SaveRemapFailed),
        remap
    );
    assert_eq!(
        matches!(error, crate::user::UserStoreError::SaveTooLarge),
        limit.is_some()
    );
    assert_eq!(visited, [1, 2]);
    assert!(store.is_modified());
    let first = match law {
        UserConfLaw::Pinyin => "gb_char.dbin.tmp",
        UserConfLaw::Zhuyin => "gb_char.dbin",
    };
    let header = LogRecord::ModifyHeader {
        old_total: 0,
        new_total: 0,
    };
    let journal = build_memory_chunk(&encode_log_records(&[header]).unwrap()).unwrap();
    assert_eq!(
        dir_snapshot(dir.path()),
        [
            (first.to_owned(), journal),
            (USER_CONF.to_owned(), b"previous marker".to_vec())
        ]
    );
}

/// Basis: `pinyin.cpp:963-1020,1141-1144` (a failed open or index save leaves the later files).
#[rstest::rstest]
fn soft_writes_allow_the_rest_of_the_save(
    #[values(UserConfLaw::Pinyin, UserConfLaw::Zhuyin)] law: UserConfLaw,
) {
    let dir = tempfile::tempdir().unwrap();
    let dir = dir.path();
    let mut store = crate::user::test_session::open(dir, originals(), versions(), law).unwrap();
    let blocked = ["gb_char.dbin", "user_pinyin_index.bin"];
    for name in blocked {
        std::fs::write(dir.join(name), b"previous file").unwrap();
        std::fs::create_dir(dir.join(format!("{name}.tmp"))).unwrap();
    }
    std::fs::write(dir.join(USER_CONF), b"previous marker").unwrap();
    store.mark_modified();
    store.save_reporting().unwrap();
    assert!(!store.is_modified());
    for name in blocked {
        assert_eq!(std::fs::read(dir.join(name)).unwrap(), b"previous file");
        assert!(dir.join(format!("{name}.tmp")).is_dir());
    }
    for name in ["user.bin", "user_phrase_index.bin", "user_bigram.db"] {
        assert!(dir.join(name).is_file(), "{name}");
        assert!(!dir.join(format!("{name}.tmp")).exists(), "{name}");
    }
    assert!(
        UserTableInfo::load_path(&dir.join(USER_CONF))
            .unwrap()
            .is_conform(&versions())
    );
}

fn item(phrase: &[u32], unigram: u32, prons: &[(Vec<u16>, u32)]) -> ChunkItem {
    ChunkItem {
        phrase: phrase.to_vec(),
        unigram,
        prons: prons.to_vec(),
    }
}

fn originals() -> SystemOriginals {
    let mut merged = BTreeMap::new();
    merged.insert(
        1,
        item(
            &[0x4f60],
            100,
            &[(vec![ChewingKey::new(23, 0, 1, 0).to_packed()], 100)],
        ),
    );
    merged.insert(
        2,
        item(
            &[0x597d],
            50,
            &[(vec![ChewingKey::new(7, 0, 13, 0).to_packed()], 50)],
        ),
    );
    crate::user::LibraryTable::from([(
        1_u8,
        SystemLibrary {
            mapped: None,
            total: 150,
            range_end: 3,
            items: merged,
        },
    )])
    .into()
}

fn state() -> UserState {
    let bigram = vec![(
        1_u32.to_le_bytes().to_vec(),
        crate::single_gram::encode_single_gram(
            207,
            &[(1, 69), (0x0100_0001, 138), (0x0700_0001, 69)],
        ),
    )];

    let user_item = item(
        &[0x4f60, 0x597d],
        15,
        &[(
            vec![
                ChewingKey::new(23, 0, 1, 0).to_packed(),
                ChewingKey::new(7, 0, 13, 0).to_packed(),
            ],
            15,
        )],
    );

    UserState {
        bigram,
        libraries: crate::user::LibraryTable::from([(7_u8, BTreeMap::from([(1_u32, user_item)]))]),
        system_overrides: crate::user::LibraryTable::from([(
            1_u8,
            BTreeMap::from([(
                1_u32,
                Some(item(
                    &[0x4f60],
                    100 + 69 * 7,
                    &[(vec![ChewingKey::new(23, 0, 1, 0).to_packed()], 100 + 69)],
                )),
            )]),
        )]),
        phrase_table: BTreeMap::new(),
        indexed: std::collections::BTreeSet::new(),
    }
    .index_every_reading()
}

fn versions() -> SystemVersions {
    SystemVersions::for_this_build(7, 14)
}

/// Basis: frozen pin observation #784 (a key no row writes is kept with an empty value).
#[test]
fn a_kept_index_key_is_saved_empty_and_loaded_back() {
    let dir = tempdir("kept-index-keys");
    let originals = originals();
    let gone: Vec<u8> = [0x9f96_u32, 0x9f96]
        .iter()
        .flat_map(|c| c.to_le_bytes())
        .collect();
    let gone_reading = vec![0x12, 0x00, 0x12, 0x00];
    let mut index = UserIndex::new();
    index.push_record(&gone_reading, &[]).unwrap();
    let mut saved = state();
    saved.phrase_table.insert(gone.clone(), Vec::new());
    let mut store =
        crate::user::test_session::seeded(&dir, &saved, &originals, &versions()).expect("session");
    store
        .session_target()
        .phrases_mut()
        .index
        .merge_missing(&index);
    store.save_reporting().expect("save");
    std::mem::forget(store);

    let rows = |dbm: UserDbm| {
        let store = DefaultStore::open_user_index(&dir.join(dbm.file_name())).expect("opens");
        let mut rows = BTreeMap::new();
        store
            .range_raw(
                std::ops::Bound::Unbounded,
                std::ops::Bound::Unbounded,
                |k, v| {
                    rows.insert(k.to_vec(), v.to_vec());
                    Ok(())
                },
            )
            .expect("walk");
        rows
    };
    let phrase = rows(UserDbm::PhraseIndex);
    assert_eq!(phrase.get(&gone), Some(&Vec::new()));
    assert_eq!(phrase.len(), 3, "你, 你好 and the kept key");
    assert_eq!(
        rows(UserDbm::PinyinIndex).get(&gone_reading),
        Some(&Vec::new())
    );

    let loaded = load_profile(&dir, &originals, &versions(), UserConfLaw::Pinyin);
    assert!(loaded.phrase_table.contains_key(&gone));
    assert!(loaded.index.contains_key(&gone_reading));
    let actual = load(&dir, &originals, &versions(), UserConfLaw::Pinyin)
        .expect("load")
        .state;
    assert_eq!(actual, saved, "an empty key names no row");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Basis: `pinyin.cpp:922-1146` (the save writes exactly the pin's eleven names).
#[test]
fn save_then_load_round_trips_the_state() {
    let dir = tempdir("round-trip");
    let originals = originals();
    let state = state();

    save_state_reporting(&dir, &state, &originals, &versions(), 1).expect("save");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .expect("readdir")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    let mut expected = vec![
        UserDbm::Bigram.file_name().to_owned(),
        UserDbm::PinyinIndex.file_name().to_owned(),
        UserDbm::PhraseIndex.file_name().to_owned(),
        "user.conf".to_owned(),
    ];
    let layout = UserFileLayout::stock();
    expected.extend(
        layout
            .user_libraries()
            .iter()
            .map(|(_, n)| n.display().into_owned()),
    );
    expected.extend(
        layout
            .system_logs()
            .iter()
            .map(|(_, n)| n.display().into_owned()),
    );
    expected.sort();
    assert_eq!(names, expected, "the save left an unexpected file");

    let loaded = load(&dir, &originals, &versions(), UserConfLaw::Pinyin).expect("load");
    assert!(loaded.skipped.is_empty(), "{:?}", loaded.skipped);
    assert!(!loaded.wiped);
    assert_eq!(loaded.open_counter, 2); // check_format's raise
    assert_eq!(loaded.state, state);

    std::fs::remove_dir_all(&dir).expect("cleanup");
}

/// Basis: Rust contract (a mapped original saves and replays as the inline one does).
#[test]
fn mapped_originals_save_and_replay_exact_logs_without_expanding_items() {
    let dir = tempdir("mapped-originals");
    let mut inline = originals();
    let base = inline.get_mut(&1).expect("fixture library");
    let mut last = base.items.remove(&2).expect("fixture item");
    last.prons.push((last.prons[0].0.clone(), 0));
    base.items.insert(3, last);
    base.range_end = 4;
    let pairs: Vec<_> = base
        .items
        .iter()
        .map(|(&slot, item)| (slot, item))
        .collect();
    let chunk = dir.join("system.bin");
    std::fs::write(&chunk, build_chunk(&pairs).expect("fixture chunk")).expect("write fixture");
    let mut libraries = PhraseLibraries::empty();
    assert!(libraries.load(1, &chunk).expect("map fixture"));
    let mapped = system_originals(libraries, inline.layout().clone());

    assert_eq!(
        mapped.get(&1).expect("mapped original").total,
        inline[&1].total
    );
    assert_eq!(
        mapped.get(&1).expect("mapped original").range_end,
        inline[&1].range_end
    );
    assert!(mapped.get(&1).expect("mapped original").mapped.is_some());
    assert!(
        mapped.get(&1).expect("mapped original").items.is_empty(),
        "open eagerly decoded items"
    );
    for slot in 0..=4 {
        assert_eq!(
            mapped.get(&1).expect("mapped original").contains_item(slot),
            inline[&1].contains_item(slot)
        );
        assert_eq!(
            mapped.get(&1).expect("mapped original").unigram(slot),
            inline[&1].unigram(slot)
        );
        assert_eq!(
            mapped.get(&1).expect("mapped original").item(slot),
            inline[&1].item(slot)
        );
    }

    let mut changed = inline[&1].item(1).expect("original item");
    changed.unigram += 69;
    changed.prons[0].1 += 69;
    let mut edited = UserState::default();
    edited.system_overrides.insert(
        1,
        BTreeMap::from([(1, Some(changed.clone())), (2, Some(changed)), (3, None)]),
    );
    {
        let (name, state) = ("edited", edited);
        let inline_dir = dir.join(format!("{name}-inline"));
        let mapped_dir = dir.join(format!("{name}-mapped"));
        for profile in [&inline_dir, &mapped_dir] {
            std::fs::create_dir(profile).expect("profile dir");
        }
        save_state_reporting(&inline_dir, &state, &inline, &versions(), 1).expect("inline save");
        save_state_reporting(&mapped_dir, &state, &mapped, &versions(), 1).expect("mapped save");
        for (_, file) in inline.layout().system_logs() {
            assert_eq!(
                std::fs::read(build_path(&mapped_dir, file.as_ref())).expect("mapped log"),
                std::fs::read(build_path(&inline_dir, file.as_ref())).expect("inline log"),
                "{name}: {}",
                file.display()
            );
        }
        for profile in [&inline_dir, &mapped_dir] {
            let loaded =
                load(profile, &mapped, &versions(), UserConfLaw::Pinyin).expect("mapped replay");
            assert!(loaded.skipped.is_empty(), "{:?}", loaded.skipped);
            assert_eq!(loaded.state, state);
        }
        assert!(
            mapped.get(&1).expect("mapped original").items.is_empty(),
            "save/replay expanded originals"
        );
    }
    drop(mapped);
    std::fs::remove_dir_all(&dir).expect("cleanup");
}

/// Basis: `pinyin.cpp:185-215` (no marker is non-conform), `phrase_index.cpp:395-397` (header-only log).
#[test]
fn fresh_dir_saves_and_loads_empty_everywhere() {
    let dir = tempdir("fresh");
    let originals = originals();

    let loaded = load(&dir, &originals, &versions(), UserConfLaw::Pinyin).expect("load");
    assert!(loaded.wiped);
    assert_eq!(loaded.open_counter, 1);
    assert_eq!(loaded.state, UserState::default());

    save_state_reporting(&dir, &UserState::default(), &originals, &versions(), 1).expect("save");
    let reloaded = load(&dir, &originals, &versions(), UserConfLaw::Pinyin).expect("load");
    assert!(!reloaded.wiped);
    assert!(reloaded.skipped.is_empty(), "{:?}", reloaded.skipped);
    assert_eq!(reloaded.state, UserState::default());
    let log = std::fs::read(dir.join("merged.dbin")).expect("absent lib log");
    let records = log_records(read_chunk_payload(&log).expect("frame"))
        .collect::<Result<Vec<_>, _>>()
        .expect("records");
    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0],
        LogRecord::ModifyHeader {
            old_total: 0,
            new_total: 0
        }
    );

    std::fs::remove_dir_all(&dir).expect("cleanup");
}

/// The user dir as a sorted (name, bytes) list.
fn dir_snapshot(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut rows: Vec<(String, Vec<u8>)> = std::fs::read_dir(dir)
        .expect("readdir")
        .map(|entry| {
            let entry = entry.expect("entry");
            let name = entry.file_name().to_string_lossy().into_owned();
            (name, std::fs::read(entry.path()).expect("read"))
        })
        .collect();
    rows.sort();
    rows
}

/// Basis: `table_info.cpp:338-348` (an unknown database format aborts before any wipe).
#[test]
fn an_unknown_database_format_aborts_the_load() {
    let before = std::cell::RefCell::new(Vec::new());
    crate::store::death_test::assert_aborts_in_tempdir(
        "user::persistence::tests::an_unknown_database_format_aborts_the_load",
        |dir| {
            let originals = originals();
            save_state_reporting(dir, &state(), &originals, &versions(), 1).expect("save");
            std::fs::write(dir.join(USER_CONF),
                "binary format version:7\nmodel data version:14\ndatabase format:NotADbmLibrary\nopen counter:5\n"
            ).expect("marker");
            *before.borrow_mut() = dir_snapshot(dir);
        },
        |dir| {
            let _ = load(dir, &originals(), &versions(), UserConfLaw::Pinyin);
        },
        |dir| assert_eq!(dir_snapshot(dir), *before.borrow(), "no wipe or rewrite"),
    );
}

/// Basis: `table_info.cpp:338-348`, `pinyin.cpp:185-215` (a marker the load cannot complete wipes).
#[test]
fn a_marker_the_load_cannot_complete_still_wipes() {
    let dir = tempdir("marker-incomplete");
    let originals = originals();
    for body in ["", "binary format version:x\nmodel data version:14\n"] {
        save_state_reporting(&dir, &state(), &originals, &versions(), 1).expect("save");
        assert!(dir.join("user.bin").exists());
        std::fs::write(dir.join(USER_CONF), body).expect("write");

        let loaded = load(&dir, &originals, &versions(), UserConfLaw::Pinyin).expect("load");
        assert!(loaded.wiped, "{body:?}");
        assert_eq!(loaded.open_counter, 1, "{body:?}"); // 0 + the pin's raise
        assert_eq!(dir_snapshot(&dir).len(), 1, "{body:?}: only the marker");
        assert!(!dir.join("user.bin").exists(), "{body:?}");
    }

    std::fs::remove_dir_all(&dir).expect("cleanup");
}

/// Basis: `g_build_filename(dir, "user.conf", NULL)` (repeated trailing separators collapse).
#[test]
fn user_conf_path_is_the_join_the_pin_writes() {
    for (dir, expected) in [
        ("a", "a/user.conf"),
        ("a/", "a/user.conf"),
        ("a///", "a/user.conf"),
        ("/", "/user.conf"),
        ("//", "//user.conf"),
        ("", "user.conf"),
        ("a/./", "a/./user.conf"),
    ] {
        assert_eq!(
            build_path(Path::new(dir), OsStr::new(USER_CONF)),
            Path::new(expected),
            "{dir:?}"
        );
    }
}

/// Basis: `pinyin.cpp:185-215` (a foreign marker wipes; the marker is rewritten conform).
#[test]
fn non_conform_profile_wipes_and_resets() {
    let dir = tempdir("wipe");
    let originals = originals();
    save_state_reporting(&dir, &state(), &originals, &versions(), 3).expect("save");
    assert!(dir.join("user.bin").exists());

    let foreign_token = match crate::store::DEFAULT_STORE_DB_FORMAT {
        "BerkeleyDB" => "KyotoCabinet",
        _ => "BerkeleyDB",
    };
    let foreign = UserTableInfo {
        binary_format_version: 7,
        model_data_version: 14,
        database_format: Some(foreign_token.to_owned()),
        open_counter: 0,
    };
    assert!(foreign.save(dir.join(USER_CONF).as_path()));

    let loaded = load(&dir, &originals, &versions(), UserConfLaw::Pinyin).expect("load");
    assert!(loaded.wiped);
    assert_eq!(loaded.state, UserState::default());
    assert_eq!(loaded.open_counter, 1); // get_open_counter caps, +1
    assert!(
        !dir.join("user.bin").exists(),
        "profile file survived the wipe"
    );
    assert!(!dir.join("gb_char.dbin").exists());
    let marker = UserTableInfo::load_path(&dir.join(USER_CONF)).expect("parse");
    assert!(marker.is_conform(&versions()));

    std::fs::remove_dir_all(&dir).expect("cleanup");
}

/// Basis: `pinyin.cpp:194-215`, `zhuyin.cpp:141-159` (libzhuyin cleans no addon rows).
#[test]
fn cleanup_walks_default_rows_then_libpinyins_addons_then_the_fixed_names() {
    let dir = tempdir("cleanup");
    let conf_dir = tempfile::tempdir().expect("conf dir");
    let conf_path = conf_dir.path().join("table.conf");
    std::fs::write(
        &conf_path,
        "binary format version:7\nmodel data version:14\n\
             lambda parameter:0.312699\n\nsource table format:pinyin\n\
             database format:BerkeleyDB\n\n\
             default RESERVED NULL NULL NULL NOT_USED\n\
             default USER_DICTIONARY NULL NULL default-user.bin USER_FILE\n\n\
             addon 4 art.table art.bin addon-user.bin DICTIONARY\n",
    )
    .expect("table.conf");
    let conf = crate::data::TableConf::load_path(&conf_path).expect("parse");
    let layout = UserFileLayout::from_conf(&conf);

    let names = [
        "default-user.bin",
        "addon-user.bin",
        "user_pinyin_index.bin",
        "user_phrase_index.bin",
        "user_bigram.db",
    ];
    for name in names {
        std::fs::write(build_path(&dir, OsStr::new(name)), b"x").expect("file");
    }
    clean_user_files(&dir, &layout, UserConfLaw::Pinyin);
    for name in names {
        assert!(
            !build_path(&dir, OsStr::new(name)).exists(),
            "{name} survived the pinyin wipe"
        );
    }

    std::fs::write(build_path(&dir, OsStr::new("addon-user.bin")), b"x").expect("file");
    std::fs::write(build_path(&dir, OsStr::new("default-user.bin")), b"x").expect("file");
    clean_user_files(&dir, &layout, UserConfLaw::Zhuyin);
    assert!(!build_path(&dir, OsStr::new("default-user.bin")).exists());
    assert!(
        build_path(&dir, OsStr::new("addon-user.bin")).exists(),
        "libzhuyin does not clean addon rows (zhuyin.cpp:141-159)"
    );

    std::fs::remove_dir_all(&dir).expect("cleanup");
}

/// Basis: Rust contract (`check_format` then the profile read equals `load`).
#[test]
fn check_format_then_the_profile_read_is_load() {
    for law in [UserConfLaw::Pinyin, UserConfLaw::Zhuyin] {
        let whole = tempdir("split-whole");
        let split = tempdir("split-parts");
        let originals = originals();
        for dir in [&whole, &split] {
            save_state_reporting(dir, &state(), &originals, &versions(), 1).expect("save");
        }
        let loaded = load(&whole, &originals, &versions(), law).expect("load");
        let check = check_format(&split, &versions(), &UserFileLayout::stock(), law);
        assert_eq!(recorded_counter(&split), recorded_counter(&whole));
        let parts = load_checked(&split, &originals, &check).expect("load the split profile");
        assert_eq!(loaded.state.bigram, state().bigram);
        assert_eq!(parts.state, loaded.state);
        assert_eq!(parts.open_counter, loaded.open_counter);
        assert_eq!(parts.wiped, loaded.wiped);
        std::fs::remove_dir_all(&whole).expect("cleanup");
        std::fs::remove_dir_all(&split).expect("cleanup");
    }
}

/// Basis: `phrase_large_table3.cpp` stores its records verbatim (duplicates, markers, partial tails).
#[test]
fn phrase_index_records_round_trip_verbatim() {
    use crate::data::row_format::phrase_index::{
        encode_ucs4_key, test_encode_tokens as encode_tokens,
    };

    let dir = tempdir("phrase-index-records");
    let originals = originals();
    let mut saved = state();
    let token = 0x0400_0001_u32;
    let mut partial = encode_tokens(&[token, token]);
    partial.extend_from_slice(&[0xa5, 0x5a]);
    saved.phrase_table = BTreeMap::from([
        (encode_ucs4_key("甲"), partial),
        (encode_ucs4_key("乙"), encode_tokens(&[token])),
        (encode_ucs4_key("丙"), Vec::new()),
        (vec![0xff], vec![0x12]),
    ]);
    save_state_reporting(&dir, &saved, &originals, &versions(), 1).expect("save");
    let loaded = load(&dir, &originals, &versions(), UserConfLaw::Pinyin).expect("load");
    assert_eq!(loaded.state.phrase_table, saved.phrase_table);
    save_state_reporting(&dir, &loaded.state, &originals, &versions(), 2).expect("save again");
    let store = DefaultStore::open_user_index(&dir.join(UserDbm::PhraseIndex.file_name()))
        .expect("open index");
    let mut rows = BTreeMap::new();
    store
        .range_raw(
            std::ops::Bound::Unbounded,
            std::ops::Bound::Unbounded,
            |key, value| {
                rows.insert(key.to_vec(), value.to_vec());
                Ok(())
            },
        )
        .expect("walk");
    assert_eq!(rows, saved.phrase_table);
    drop(store);
    std::fs::remove_dir_all(&dir).expect("cleanup");
}

/// The counter `user.conf` holds, read back as the next init reads it.
fn recorded_counter(dir: &Path) -> Option<i32> {
    Some(
        UserTableInfo::load_path(&dir.join(USER_CONF))
            .ok()?
            .open_counter,
    )
}

/// Basis: `pinyin.cpp:1196-1197` (`counter > 1 ? counter - 1 : 0` over `get_open_counter`).
#[test]
fn the_lowering_is_pinyin_finis_arithmetic() {
    assert_eq!(lowered_open_counter(0), 0);
    assert_eq!(lowered_open_counter(1), 0);
    assert_eq!(lowered_open_counter(2), 1);
    assert_eq!(
        lowered_open_counter(OPEN_COUNTER_LIMIT),
        OPEN_COUNTER_LIMIT - 1
    );
    assert_eq!(lowered_open_counter(OPEN_COUNTER_LIMIT + 1), 0);
    assert_eq!(lowered_open_counter(-2), 0);
}

/// Basis: `pinyin.cpp:185-187,1143,1194-1200` (init raises, save writes back, fini lowers; #523).
#[test]
fn clean_pinyin_cycles_return_the_counter_and_keep_the_profile() {
    let dir = tempdir("pinyin-cycles");
    let originals = originals();
    for launch in 1..=10 {
        let loaded = load(&dir, &originals, &versions(), UserConfLaw::Pinyin).expect("load");
        assert_eq!(
            loaded.wiped,
            launch == 1,
            "launch {launch}: only the fresh dir wipes"
        );
        assert_eq!(loaded.open_counter, 1, "launch {launch}: raised from 0");
        assert_eq!(
            recorded_counter(&dir),
            Some(1),
            "launch {launch}: the init writes"
        );
        if launch > 1 {
            assert_eq!(
                loaded.state,
                state(),
                "launch {launch}: the profile survived"
            );
        }
        save_state_reporting(&dir, &state(), &originals, &versions(), loaded.open_counter)
            .expect("save");
        assert_eq!(
            recorded_counter(&dir),
            Some(1),
            "launch {launch}: the save writes back"
        );
        fini(
            Some(&dir),
            &versions(),
            UserConfLaw::Pinyin,
            loaded.open_counter,
        );
        assert_eq!(
            recorded_counter(&dir),
            Some(0),
            "launch {launch}: the fini lowers"
        );
    }
    std::fs::remove_dir_all(&dir).expect("cleanup");
}

/// Basis: `pinyin.cpp:1194-1200` (`pinyin_fini` lowers the counter without a save).
#[test]
fn a_pinyin_fini_without_a_save_still_lowers_the_counter() {
    let dir = tempdir("pinyin-fini-unsaved");
    let originals = originals();
    save_state_reporting(&dir, &state(), &originals, &versions(), 0).expect("seed");
    let before = std::fs::read(dir.join("user.bin")).expect("user.bin");
    let loaded = load(&dir, &originals, &versions(), UserConfLaw::Pinyin).expect("load");
    assert_eq!(recorded_counter(&dir), Some(1));
    fini(
        Some(&dir),
        &versions(),
        UserConfLaw::Pinyin,
        loaded.open_counter,
    );
    assert_eq!(recorded_counter(&dir), Some(0));
    assert_eq!(
        std::fs::read(dir.join("user.bin")).expect("user.bin"),
        before
    );
    std::fs::remove_dir_all(&dir).expect("cleanup");
}

/// Basis: `table_info.cpp:409-410` (a counter above the limit wipes on the eighth killed launch).
#[test]
fn pinyin_launches_that_die_before_fini_wipe_on_the_eighth() {
    let dir = tempdir("pinyin-crashes");
    let originals = originals();
    save_state_reporting(&dir, &state(), &originals, &versions(), 0).expect("seed");
    for launch in 1..=7 {
        let loaded = load(&dir, &originals, &versions(), UserConfLaw::Pinyin).expect("load");
        assert!(!loaded.wiped, "killed launch {launch} wiped early");
        assert_eq!(loaded.state, state());
        assert_eq!(recorded_counter(&dir), Some(launch));
    }
    let eighth = load(&dir, &originals, &versions(), UserConfLaw::Pinyin).expect("load");
    assert!(eighth.wiped);
    assert_eq!(eighth.state, UserState::default());
    assert_eq!(recorded_counter(&dir), Some(1));
    assert!(
        !dir.join("user.bin").exists(),
        "profile file survived the wipe"
    );
    std::fs::remove_dir_all(&dir).expect("cleanup");
}

/// Basis: `zhuyin.cpp:126-176,741-757` (init only reads the marker; fini writes nothing; save writes 0).
#[test]
fn zhuyin_init_and_fini_never_write_the_marker() {
    let dir = tempdir("zhuyin-marker");
    let originals = originals();
    let fresh = load(&dir, &originals, &versions(), UserConfLaw::Zhuyin).expect("load");
    assert!(fresh.wiped);
    assert_eq!(fresh.open_counter, 0);
    assert!(
        !dir.join(USER_CONF).exists(),
        "a zhuyin init wrote user.conf"
    );
    fini(
        Some(&dir),
        &versions(),
        UserConfLaw::Zhuyin,
        fresh.open_counter,
    );
    assert!(
        !dir.join(USER_CONF).exists(),
        "a zhuyin fini wrote user.conf"
    );

    save_state_reporting(&dir, &state(), &originals, &versions(), fresh.open_counter)
        .expect("save");
    let marker = std::fs::read(dir.join(USER_CONF)).expect("marker");
    assert_eq!(recorded_counter(&dir), Some(0));
    for launch in 1..=10 {
        let loaded = load(&dir, &originals, &versions(), UserConfLaw::Zhuyin).expect("load");
        assert!(!loaded.wiped, "zhuyin launch {launch} wiped");
        assert_eq!(loaded.state, state());
        fini(
            Some(&dir),
            &versions(),
            UserConfLaw::Zhuyin,
            loaded.open_counter,
        );
        assert_eq!(std::fs::read(dir.join(USER_CONF)).expect("marker"), marker);
    }
    std::fs::remove_dir_all(&dir).expect("cleanup");
}

/// Basis: `table_info.cpp:338-348`, `pinyin.cpp:191-215`, `zhuyin.cpp:138-161`; UCRT's 0xFF pushback
/// (`external-io-boundary.md`).
#[test]
fn stray_identity_bytes_follow_the_native_scanner() {
    for (byte, law) in [
        (0xFE, UserConfLaw::Pinyin),
        (0xFE, UserConfLaw::Zhuyin),
        (0xFF, UserConfLaw::Pinyin),
    ] {
        let case = format!("{law:?} byte {byte:#04x}");
        let dir = tempdir("stray-byte");
        let originals = originals();
        save_state_reporting(&dir, &state(), &originals, &versions(), 0).expect("seed");
        let identity = versions();
        let mut marker = format!(
            "binary format version:{}\nmodel data version:{}\ndatabase format:{}\nopen counter:0\n",
            identity.binary_format_version, identity.model_data_version, identity.database_format,
        )
        .into_bytes();
        let after_version = marker
            .iter()
            .position(|&byte| byte == b'\n')
            .expect("a line end");
        marker.insert(after_version, byte);
        std::fs::write(dir.join(USER_CONF), &marker).expect("corrupt marker");

        let conform = UserTableInfo::load_path(&dir.join(USER_CONF))
            .is_ok_and(|info| info.is_conform(&versions()));
        let expected_conform = byte == 0xFF && cfg!(all(windows, target_env = "msvc"));
        assert_eq!(conform, expected_conform, "{case}: native scanner");
        let loaded = load(&dir, &originals, &versions(), law).expect("load");
        assert_eq!(loaded.wiped, !expected_conform, "{case}");
        assert_eq!(
            loaded.state,
            if expected_conform {
                state()
            } else {
                UserState::default()
            },
            "{case}"
        );
        assert_eq!(dir.join("user.bin").exists(), expected_conform, "{case}");
        match law {
            UserConfLaw::Pinyin => assert_eq!(recorded_counter(&dir), Some(1)),
            UserConfLaw::Zhuyin => assert_eq!(
                std::fs::read(dir.join(USER_CONF)).expect("marker"),
                marker,
                "{case}: zhuyin rewrote the marker"
            ),
        }
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }
}

/// Basis: `table_info.cpp:356-359,409-410` (`fscanf("open counter:%d\n")`, 0 when nothing is assigned),
/// `pinyin.cpp:185-187,1194-1200`.
#[test]
fn a_hand_written_counter_is_the_scanners_int() {
    let originals = originals();
    let identity = versions();
    let head = format!(
        "binary format version:{}\nmodel data version:{}\ndatabase format:{}\n",
        identity.binary_format_version, identity.model_data_version, identity.database_format,
    );
    for (line, raised, lowered, wiped) in [
        (&b"open counter:-3\n"[..], -2, 0, false),
        (b"open counter:+3\n", 4, 3, false),
        (b"open counter:+7\n", 1, 0, true),
        (b"open counter:3x\n", 4, 3, false),
        (b"open counter:7x\n", 1, 0, true),
        (b"open counter:3\xff\n", 4, 3, false),
        (b"opencounter:3\xff\n", 4, 3, false),
        (b"open \n counter:3\xff\n", 4, 3, false),
        (b"open counter:\xff\n", 1, 0, false),
        (b"open counter:\x0b5\n", 6, 5, false),
        (b"open counter:\n5\n", 6, 5, false),
        (b"  open counter:5\n", 6, 5, false),
        (b"opencounter:5\n", 6, 5, false),
        (b"open counter:\n", 1, 0, false),
        (b"open counter:-\n", 1, 0, false),
        (b"", 1, 0, false),
    ] {
        // libzhuyin writes no counter, so one row holds its law.
        let laws: &[UserConfLaw] = if line == b"open counter:+3\n" {
            &[UserConfLaw::Pinyin, UserConfLaw::Zhuyin]
        } else {
            &[UserConfLaw::Pinyin]
        };
        for &law in laws {
            let case = format!("{law:?} {:?}", String::from_utf8_lossy(line));
            let dir = tempdir("hand-written-counter");
            save_state_reporting(&dir, &state(), &originals, &versions(), 0).expect("seed");
            let mut marker = head.as_bytes().to_vec();
            marker.extend_from_slice(line);
            std::fs::write(dir.join(USER_CONF), &marker).expect("hand-written marker");

            let loaded = load(&dir, &originals, &versions(), law).expect("load");
            assert_eq!(loaded.wiped, wiped, "{case}");
            let expected_state = if wiped { UserState::default() } else { state() };
            assert_eq!(loaded.state, expected_state, "{case}");
            match law {
                UserConfLaw::Pinyin => {
                    assert_eq!(loaded.open_counter, raised, "{case}");
                    assert_eq!(recorded_counter(&dir), Some(raised), "{case}");
                    fini(Some(&dir), &versions(), law, loaded.open_counter);
                    assert_eq!(recorded_counter(&dir), Some(lowered), "{case}");
                }
                UserConfLaw::Zhuyin => {
                    fini(Some(&dir), &versions(), law, loaded.open_counter);
                    assert_eq!(
                        std::fs::read(dir.join(USER_CONF)).expect("marker"),
                        marker,
                        "{case}"
                    );
                }
            }
            std::fs::remove_dir_all(&dir).expect("cleanup");
        }
    }
}

/// Basis: `pinyin.cpp:187,194-215` (the marker is saved before the wipe).
#[test]
fn the_marker_is_written_before_the_wipe() {
    let dir = tempdir("marker-before-wipe");
    let conf_dir = tempdir("marker-before-wipe-conf");
    std::fs::write(
        conf_dir.join("table.conf"),
        format!(
            "binary format version:7\nmodel data version:14\nlambda parameter:0.312699\n\
                 source table format:pinyin\ndatabase format:{}\n\
                 default USER_DICTIONARY NULL NULL user.conf USER_FILE\n",
            versions().database_format
        ),
    )
    .expect("table.conf");
    let layout = UserFileLayout::from_conf(
        &crate::data::TableConf::load_from_dir(&conf_dir).expect("table.conf"),
    );
    let check = check_format(&dir, &versions(), &layout, UserConfLaw::Pinyin);
    assert!(!check.conform);
    assert!(!dir.join(USER_CONF).exists(), "the wipe follows the save");
    std::fs::remove_dir_all(&dir).expect("cleanup");
    std::fs::remove_dir_all(&conf_dir).expect("cleanup");
}

/// Basis: `zhuyin.cpp:141-159` (the wipe unlinks no `user.conf`).
#[test]
fn a_zhuyin_wipe_keeps_the_non_conform_marker() {
    let dir = tempdir("zhuyin-tired");
    let originals = originals();
    save_state_reporting(
        &dir,
        &state(),
        &originals,
        &versions(),
        OPEN_COUNTER_LIMIT + 1,
    )
    .expect("seed");
    let marker = std::fs::read(dir.join(USER_CONF)).expect("marker");
    let loaded = load(&dir, &originals, &versions(), UserConfLaw::Zhuyin).expect("load");
    assert!(loaded.wiped);
    assert_eq!(loaded.state, UserState::default());
    assert!(
        !dir.join("user.bin").exists(),
        "profile file survived the wipe"
    );
    assert_eq!(std::fs::read(dir.join(USER_CONF)).expect("marker"), marker);
    assert!(
        load(&dir, &originals, &versions(), UserConfLaw::Zhuyin)
            .expect("load")
            .wiped
    );
    save_state_reporting(
        &dir,
        &UserState::default(),
        &originals,
        &versions(),
        loaded.open_counter,
    )
    .expect("save");
    assert_eq!(recorded_counter(&dir), Some(0));
    assert!(
        !load(&dir, &originals, &versions(), UserConfLaw::Zhuyin)
            .expect("load")
            .wiped
    );
    std::fs::remove_dir_all(&dir).expect("cleanup");
}

/// Basis: `memory_chunk.h:536-557` (the pin asserts on the refused header write).
#[test]
#[cfg(target_os = "linux")]
fn reporting_save_aborts_at_a_chunk_header_write() {
    crate::store::death_test::assert_aborts_in_tempdir(
        "user::persistence::tests::reporting_save_aborts_at_a_chunk_header_write",
        |dir| {
            let system = crate::user::native_fixture::system_dir();
            let conf = crate::data::TableConf::load_from_dir(&system).expect("table.conf");
            let marker = UserTableInfo::conform_to(&SystemVersions::from_conf(&conf));
            assert!(marker.save(dir.join(USER_CONF).as_path()));
            std::os::unix::fs::symlink("/dev/full", dir.join("gb_char.dbin.tmp")).expect("symlink");
        },
        |dir| {
            let mut store = crate::user::test_session::open_w3(dir);
            store.mark_modified();
            let _ = store.save_reporting();
        },
        |dir| {
            assert_eq!(recorded_counter(dir), Some(1));
            assert_eq!(
                std::fs::read_link(dir.join("gb_char.dbin.tmp")).unwrap(),
                Path::new("/dev/full")
            );
            assert!(!dir.join("gb_char.dbin").exists());
            assert!(!dir.join("gbk_char.dbin.tmp").exists());
            assert!(!dir.join("user_bigram.db.tmp").exists());
        },
    );
}

/// The process umask, from a probe created with 0777.
#[cfg(unix)]
fn current_umask(dir: &Path) -> u32 {
    use std::os::unix::fs::OpenOptionsExt;
    let probe = dir.join("umask-probe");
    let _ = std::fs::remove_file(&probe);
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o777)
        .open(&probe)
        .expect("probe");
    let granted = mode_of(&probe);
    std::fs::remove_file(&probe).expect("probe");
    !granted & 0o777
}

#[cfg(unix)]
fn mode_of(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .permissions()
        .mode()
        & 0o777
}

/// Basis: `memory_chunk.h:536-537` (0644), each backend's `save_db` mode, `fopen` 0666 in place (#544).
#[test]
// Unix only: file modes and the umask.
#[cfg(unix)]
fn user_files_take_the_pins_creation_modes() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempdir("modes");
    let originals = originals();
    let umask = current_umask(&dir);
    save_state_reporting(&dir, &state(), &originals, &versions(), 0).expect("save");

    let dbm_request = match crate::store::DEFAULT_STORE_DB_FORMAT {
        "BerkeleyDB" => 0o600,
        "KyotoCabinet" => 0o666,
        _ => 0o644,
    };
    let mut expected: Vec<(String, u32)> = vec![(USER_CONF.to_owned(), 0o666)];
    for dbm in [UserDbm::Bigram, UserDbm::PinyinIndex, UserDbm::PhraseIndex] {
        expected.push((dbm.file_name().to_owned(), dbm_request));
    }
    let layout = UserFileLayout::stock();
    for (_, name) in layout.user_libraries().iter().chain(layout.system_logs()) {
        expected.push((name.display().into_owned(), 0o644));
    }
    for (name, request) in &expected {
        assert_eq!(mode_of(&dir.join(name)), request & !umask, "{name}");
    }

    for name in [USER_CONF, "user.bin"] {
        std::fs::set_permissions(dir.join(name), std::fs::Permissions::from_mode(0o640))
            .expect("chmod");
    }
    save_state_reporting(&dir, &state(), &originals, &versions(), 0).expect("save again");
    assert_eq!(
        mode_of(&dir.join(USER_CONF)),
        0o640,
        "user.conf is rewritten in place"
    );
    assert_eq!(
        mode_of(&dir.join("user.bin")),
        0o644 & !umask,
        "user.bin is replaced"
    );

    std::fs::remove_dir_all(&dir).expect("cleanup");
}

/// Basis: `phrase_index.cpp:395-429` (the diff of a replayed state is the same journal).
#[test]
fn the_diff_log_is_stable_across_save_cycles() {
    let dir = tempdir("stable");
    let originals = originals();
    save_state_reporting(&dir, &state(), &originals, &versions(), 1).expect("save");
    let first = std::fs::read(dir.join("gb_char.dbin")).expect("log");

    let loaded = load(&dir, &originals, &versions(), UserConfLaw::Pinyin).expect("load");
    save_state_reporting(&dir, &loaded.state, &originals, &versions(), 2).expect("save 2");
    let second = std::fs::read(dir.join("gb_char.dbin")).expect("log 2");
    assert_eq!(first, second);

    std::fs::remove_dir_all(&dir).expect("cleanup");
}

/// Basis: `074a2219 phrase_index.h:100-114`, `phrase_index.cpp:180-205,411-429`, `pinyin.cpp:949-963`.
#[test]
fn save_journals_empty_and_max_count_items_from_the_changed_baseline() {
    let dir = tempfile::tempdir().unwrap();
    let originals = originals();
    let mut baseline = originals[&1].fixture_copy();
    baseline.total = 81;
    baseline.range_end = 5;
    baseline
        .items
        .insert(1, item(&[], 7, &[(vec![], 5), (vec![], 6)]));
    baseline
        .items
        .insert(3, item(&[7; 255], 11, &[(vec![1; 255], 2)]));
    let readings: Vec<_> = (0..255_u16)
        .map(|key| (vec![key], u32::from(key) + 1))
        .collect();
    baseline.items.insert(4, item(&[0x7532], 13, &readings));

    let mut store =
        crate::user::test_session::open(dir.path(), originals, versions(), UserConfLaw::Pinyin)
            .unwrap();
    store.mark_modified();
    let mut baseline = Some(baseline);
    store
        .save_reporting_with_baseline(&[], |library, _| {
            if library == 1 {
                SaveBaseline::Changed(baseline.take().expect("save remaps library 1 once"))
            } else {
                SaveBaseline::Keep
            }
        })
        .unwrap();

    let empty = [0, 2, 7, 0, 0, 0, 5, 0, 0, 0, 6, 0, 0, 0];
    let mut max_words = vec![255, 1, 11, 0, 0, 0];
    max_words.extend([7, 0, 0, 0].repeat(255));
    max_words.extend([1, 0].repeat(255));
    max_words.extend([2, 0, 0, 0]);
    let mut max_readings = vec![1, 255, 13, 0, 0, 0, 0x32, 0x75, 0, 0];
    for key in 0..255_u16 {
        max_readings.extend(key.to_le_bytes());
        max_readings.extend((u32::from(key) + 1).to_le_bytes());
    }
    let mut live_item = vec![1, 1, 100, 0, 0, 0, 0x60, 0x4f, 0, 0];
    live_item.extend(ChewingKey::new(23, 0, 1, 0).to_packed().to_le_bytes());
    live_item.extend([100, 0, 0, 0]);

    let journal = std::fs::read(dir.path().join("gb_char.dbin")).unwrap();
    let records = log_records(read_chunk_payload(&journal).unwrap())
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        records,
        [
            LogRecord::ModifyHeader {
                old_total: 81,
                new_total: 150
            },
            LogRecord::Modify {
                token: 1,
                old_item: empty.as_slice(),
                new_item: &live_item
            },
            LogRecord::Remove {
                token: 3,
                old_item: &max_words
            },
            LogRecord::Remove {
                token: 4,
                old_item: &max_readings
            },
        ]
    );
}

/// A system library with mapped items in slots 1 to 4 and an inline item in slot 9.
fn library_for_equivalence(dir: &Path) -> SystemLibrary {
    let key = |n: u16| vec![n, n + 1];
    let items = vec![
        (1, item(&[0x4f60, 0x597d], 9, &[(key(1), 5), (key(3), 7)])),
        (2, item(&[0x4f60], 4, &[])),
        (3, item(&[0xd800], 4, &[(vec![1], 2)])),
        (4, item(&[0x4f60, 0x597d], 9, &[(key(1), 5), (key(1), 8)])),
    ];
    let chunk = crate::data::phrase_library::build_chunk(&borrowed_items(&items)).expect("chunk");
    let path = dir.join("equiv.bin");
    std::fs::write(&path, chunk).expect("write");
    let mapped = crate::data::phrase_library::PhraseLibrary::open(&path).expect("open");
    SystemLibrary {
        total: 0,
        range_end: 0,
        items: BTreeMap::from([(9, item(&[0x1234], 3, &[(vec![7], 11), (vec![7], 12)]))]),
        mapped: Some(mapped),
    }
}

/// Basis: Rust contract (in-place item reads equal the materialized item).
#[test]
fn in_place_item_reads_agree_with_the_materialized_item() {
    let dir = tempdir("equivalence");
    let library = library_for_equivalence(&dir);
    let probes = [
        item(&[0x4f60, 0x597d], 9, &[(vec![1, 2], 5), (vec![3, 4], 7)]),
        item(&[0x4f60, 0x597d], 10, &[(vec![1, 2], 5), (vec![3, 4], 7)]),
        item(&[0x4f60, 0x597e], 9, &[(vec![1, 2], 5), (vec![3, 4], 7)]),
        item(&[0x4f60], 9, &[(vec![1, 2], 5), (vec![3, 4], 7)]),
        item(&[0x4f60, 0x597d], 9, &[(vec![1, 2], 6), (vec![3, 4], 7)]),
        item(&[0x4f60, 0x597d], 9, &[(vec![1, 2], 5)]),
        item(
            &[0x4f60, 0x597d],
            9,
            &[(vec![1, 2], 5), (vec![3, 4], 7), (vec![5, 6], 1)],
        ),
        item(&[0x4f60, 0x597d], 9, &[(vec![3, 4], 7), (vec![1, 2], 5)]),
        item(&[0x4f60], 4, &[]),
        item(&[0xd800], 4, &[(vec![1], 2)]),
        item(&[0x1234], 3, &[(vec![7], 11), (vec![7], 12)]),
        item(&[0x1234], 3, &[(vec![7], 12), (vec![7], 11)]),
        item(&[], 0, &[]),
    ];
    for slot in [0_u32, 1, 2, 3, 4, 5, 9, 1000] {
        let materialized = library.item(slot);
        assert_eq!(
            library.unigram(slot).filter(|_| materialized.is_some()),
            materialized.as_ref().map(|current| current.unigram),
            "slot {slot}"
        );
        if let Some(current) = &materialized {
            let bytes = library
                .visible_bytes(slot)
                .map(std::borrow::Cow::into_owned);
            assert_eq!(bytes, Some(journal_item_bytes(current)), "slot {slot}");
            for probe in &probes {
                assert_eq!(
                    bytes.as_deref() == Some(journal_item_bytes(probe).as_slice()),
                    current == probe,
                    "slot {slot} against {probe:?}"
                );
            }
        }
    }
    drop(library);
    std::fs::remove_dir_all(&dir).expect("cleanup");
}

/// Basis: Rust contract (replay over mapped originals equals replay over inline ones).
#[test]
fn replay_over_mapped_originals_matches_replay_over_inline_ones() {
    let key = |n: u16| vec![n];
    let items = vec![
        (1_u32, item(&[0x4f60], 100, &[(key(1), 100)])),
        (2, item(&[0x597d], 50, &[(key(2), 30), (key(3), 20)])),
        (3, item(&[0x4e16], 7, &[(key(4), 7)])),
        (4, item(&[0x754c], 9, &[(key(5), 9)])),
    ];
    let total = 100 + 50 + 7 + 9;
    let chunk = build_chunk(&borrowed_items(&items)).expect("chunk");
    let dir = tempdir("mapped-vs-inline");
    let path = dir.join("lib.bin");
    std::fs::write(&path, chunk).expect("write");
    let originals_of = |inline: bool| -> SystemOriginals {
        let library = SystemLibrary {
            total,
            range_end: 5,
            items: if inline {
                items.iter().cloned().collect()
            } else {
                BTreeMap::new()
            },
            mapped: (!inline)
                .then(|| crate::data::phrase_library::PhraseLibrary::open(&path).expect("open")),
        };
        crate::user::LibraryTable::from([(1_u8, library)]).into()
    };
    let grown = |base: &ChunkItem, extra: u32| {
        let mut next = base.clone();
        next.prons[0].1 += extra;
        next.unigram += extra;
        next
    };
    let longer = {
        let mut next = items[1].1.clone();
        next.prons.push((key(9), 4));
        next
    };
    let wire = |item: &ChunkItem| encode_phrase_item(item).expect("wire");
    let scenarios: Vec<Vec<LogRecord>> = vec![
        // An in-place modify, a growing modify, a matching remove.
        vec![
            LogRecord::ModifyHeader {
                old_total: total,
                new_total: total,
            },
            LogRecord::Modify {
                token: 1,
                old_item: wire(&items[0].1),
                new_item: wire(&grown(&items[0].1, 5)),
            },
            LogRecord::Modify {
                token: 2,
                old_item: wire(&items[1].1),
                new_item: wire(&longer),
            },
            LogRecord::Remove {
                token: 3,
                old_item: wire(&items[2].1),
            },
        ],
        // A record over a slot an earlier record already changed.
        vec![
            LogRecord::Modify {
                token: 1,
                old_item: wire(&items[0].1),
                new_item: wire(&grown(&items[0].1, 5)),
            },
            LogRecord::Modify {
                token: 1,
                old_item: wire(&grown(&items[0].1, 5)),
                new_item: wire(&grown(&items[0].1, 9)),
            },
            LogRecord::Remove {
                token: 1,
                old_item: wire(&grown(&items[0].1, 9)),
            },
            LogRecord::Remove {
                token: 1,
                old_item: wire(&grown(&items[0].1, 9)),
            },
        ],
        // Each way a record disagrees with the original stops the replay.
        vec![LogRecord::Modify {
            token: 4,
            old_item: wire(&grown(&items[3].1, 1)),
            new_item: wire(&items[3].1),
        }],
        vec![LogRecord::Remove {
            token: 4,
            old_item: wire(&grown(&items[3].1, 1)),
        }],
        vec![LogRecord::Remove {
            token: 4,
            old_item: wire(&item(&[0x754c, 0x754c], 9, &[(vec![5, 5], 9)])),
        }],
        // A slot with no original item.
        vec![LogRecord::Remove {
            token: 40,
            old_item: wire(&items[3].1),
        }],
        vec![LogRecord::Modify {
            token: 40,
            old_item: wire(&items[3].1),
            new_item: wire(&items[3].1),
        }],
        // A payload that does not decode.
        vec![LogRecord::Modify {
            token: 4,
            old_item: vec![1],
            new_item: wire(&items[3].1),
        }],
        // A header that misses the running total.
        vec![LogRecord::ModifyHeader {
            old_total: total + 1,
            new_total: 0,
        }],
        // An add, then a modify of the added item.
        vec![
            LogRecord::Add {
                token: 7,
                new_item: wire(&item(&[0x1234], 3, &[(key(8), 3)])),
            },
            LogRecord::Modify {
                token: 7,
                old_item: wire(&item(&[0x1234], 3, &[(key(8), 3)])),
                new_item: wire(&item(&[0x1234], 4, &[(key(8), 4)])),
            },
        ],
    ];
    for (index, records) in scenarios.iter().enumerate() {
        let outcome = |inline: bool| {
            let run_dir = tempdir(&format!("mapped-vs-inline-{index}-{inline}"));
            std::fs::write(
                run_dir.join("gb_char.dbin"),
                build_memory_chunk(&encode_log_records(records).expect("log")).expect("chunk"),
            )
            .expect("write log");
            assert!(
                UserTableInfo::conform_to(&versions()).save(run_dir.join("user.conf").as_path())
            );
            let loaded = load(
                &run_dir,
                &originals_of(inline),
                &versions(),
                UserConfLaw::Pinyin,
            )
            .expect("load");
            std::fs::remove_dir_all(run_dir).expect("cleanup");
            (
                loaded.state.system_overrides.clone(),
                loaded.library_totals.clone(),
                loaded.skipped.clone(),
            )
        };
        assert_eq!(outcome(false), outcome(true), "scenario {index}");
    }
    std::fs::remove_dir_all(dir).expect("cleanup");
}

/// Basis: `phrase_index.h:647`, `phrase_index.cpp:186` (the slot is the token's low 24 bits).
#[test]
fn replay_masks_high_token_bits_like_the_subindex() {
    let dir = tempdir("local-slot-mask");
    let originals = originals();
    let old = originals[&1].item(1).expect("original");
    let mut new = old.clone();
    new.prons[0].1 += 7;
    let records = vec![
        LogRecord::ModifyHeader {
            old_total: 150,
            new_total: 150,
        },
        LogRecord::Modify {
            token: 0xab00_0001,
            old_item: encode_phrase_item(&old).expect("old payload"),
            new_item: encode_phrase_item(&new).expect("new payload"),
        },
    ];
    let payload = encode_log_records(&records).expect("log");
    std::fs::write(
        dir.join("gb_char.dbin"),
        build_memory_chunk(&payload).expect("chunk"),
    )
    .expect("write");
    assert!(UserTableInfo::conform_to(&versions()).save(dir.join("user.conf").as_path()));
    let loaded = load(&dir, &originals, &versions(), UserConfLaw::Pinyin).expect("load");
    assert!(loaded.skipped.is_empty());
    assert_eq!(loaded.state.system_overrides[&1][&1], Some(new));
    std::fs::remove_dir_all(dir).expect("cleanup");
}

/// Loads a conforming profile whose `gb_char.dbin` holds `payload`.
fn load_with_log(name: &str, payload: &[u8]) -> Loaded {
    let dir = tempdir(name);
    std::fs::write(
        dir.join("gb_char.dbin"),
        build_memory_chunk(payload).expect("frame"),
    )
    .expect("write");
    assert!(UserTableInfo::conform_to(&versions()).save(dir.join(USER_CONF).as_path()));
    let loaded = load(&dir, &originals(), &versions(), UserConfLaw::Pinyin).expect("load");
    std::fs::remove_dir_all(&dir).expect("cleanup");
    loaded
}

/// A `MODIFY_HEADER` record carrying `token`, as no writer emits it.
fn header_with_token(token: u32) -> Vec<u8> {
    let mut record = 4_u32.to_le_bytes().to_vec();
    record.extend_from_slice(&token.to_le_bytes());
    record.extend_from_slice(&4_u16.to_le_bytes());
    record.extend_from_slice(&[0; 8]);
    record
}

/// The header and a MODIFY of slot 1 that the originals accept.
fn accepted_modify() -> (Vec<LogRecord>, ChunkItem) {
    let old = originals()[&1].item(1).expect("original");
    let mut new = old.clone();
    new.prons[0].1 += 7;
    (
        vec![
            LogRecord::ModifyHeader {
                old_total: 150,
                new_total: 150,
            },
            LogRecord::Modify {
                token: 1,
                old_item: encode_phrase_item(&old).expect("old payload"),
                new_item: encode_phrase_item(&new).expect("new payload"),
            },
        ],
        new,
    )
}

/// Basis: Rust contract (the mask-log cache keeps each first load, failed or not).
#[test]
fn mask_log_cache_keeps_validated_payloads_and_failed_loads() {
    let dir = tempdir("mask-log-cache");
    let originals = originals();
    let path = dir.join("gb_char.dbin");
    let mut cached = MaskLogs::default();
    let payload = header_with_token(0x0100_0001);
    std::fs::write(&path, build_memory_chunk(&payload).expect("frame")).expect("write");
    assert_eq!(
        read_mask_log(
            &dir,
            originals.layout(),
            &mut cached,
            1,
            0x0f00_0000,
            0x0200_0000
        ),
        None
    );
    assert_eq!(
        cached.0.remove(&1).expect("cached").expect("loaded"),
        payload
    );

    std::fs::remove_file(&path).expect("remove");
    assert_eq!(
        read_mask_log(
            &dir,
            originals.layout(),
            &mut cached,
            1,
            0x0f00_0000,
            0x0100_0000
        ),
        None
    );
    std::fs::write(&path, build_memory_chunk(&payload).expect("frame")).expect("write");
    let mut loaded = Loaded::default();
    assert_eq!(
        replay_logs(
            &dir,
            &originals,
            &mut loaded,
            Some((0x0f00_0000, 0x0100_0000)),
            false,
            &mut crate::user::LibraryTable::new(),
            Some(&mut cached)
        ),
        Ok(())
    );
    assert_eq!(
        replay_logs(
            &dir,
            &originals,
            &mut loaded,
            Some((0x0f00_0000, 0x0100_0000)),
            false,
            &mut crate::user::LibraryTable::new(),
            None
        ),
        Err(JournalFault::HeaderToken {
            library: 1,
            token: 0x0100_0001
        })
    );
    std::fs::remove_dir_all(&dir).expect("cleanup");
}

/// Basis: policy row 159 (a bounded short header stops its walk; later journal faults stay reachable).
#[test]
fn bounded_short_header_runs_keep_later_journal_faults_reachable() {
    let full = encode_log_records(&[LogRecord::<Vec<u8>>::ModifyHeader {
        old_total: 150,
        new_total: 0,
    }])
    .expect("header");
    let kept_add = encode_log_records(&[LogRecord::Add {
        token: 5,
        new_item: vec![0; 6],
    }])
    .expect("add");
    for width in [0_u16, 3] {
        let mut short = 4_u32.to_le_bytes().to_vec();
        short.extend_from_slice(&0_u32.to_le_bytes());
        short.extend_from_slice(&width.to_le_bytes());
        short.extend_from_slice(&vec![0xff; 2 * usize::from(width)]);
        assert_eq!(masked_log(&short, 0x0FFF_FFFF, 0x0100_0009, 1), Ok(None));
        assert_eq!(
            masked_log(
                &[short.as_slice(), kept_add.as_slice()].concat(),
                0x0FFF_FFFF,
                0x0100_0009,
                1,
            )
            .err(),
            Some(JournalFault::SurvivingRecord {
                library: 1,
                token: 5
            }),
        );

        let non_null = header_with_token(0x0100_0001);
        let short_then_fault = [short.as_slice(), non_null.as_slice()].concat();
        let loaded = load_with_log("short-header-stops-nonzero-total", &short_then_fault);
        assert_eq!(loaded.journal_fault, None, "width {width}");
        assert_eq!(
            masked_log(&short_then_fault, 0x0FFF_FFFF, 0x0100_0009, 1).err(),
            Some(JournalFault::HeaderToken {
                library: 1,
                token: 0x0100_0001,
            }),
            "width {width}"
        );

        let mut payload = [full.as_slice(), short.as_slice()].concat();
        assert_eq!(
            masked_log(&payload, 0x0FFF_FFFF, 0x0100_0009, 1).err(),
            Some(JournalFault::MultipleHeaders { library: 1 }),
            "width {width}"
        );
        payload.extend_from_slice(&non_null);
        let loaded = load_with_log("short-header-before-fault", &payload);
        assert_eq!(
            loaded.journal_fault,
            Some(JournalFault::HeaderToken {
                library: 1,
                token: 0x0100_0001,
            }),
            "width {width}"
        );
    }
}

/// Basis: `074a2219 phrase_index.cpp:468-470` (merge returns before the bad header is read).
#[test]
fn a_header_token_after_a_mismatch_is_never_read() {
    let (mut records, new) = accepted_modify();
    records.push(LogRecord::Remove {
        token: 2,
        old_item: vec![0; 6],
    });
    let mut payload = encode_log_records(&records).expect("log");
    payload.extend_from_slice(&header_with_token(0x0100_0001));
    let loaded = load_with_log("header-token-after-stop", &payload);
    assert_eq!(loaded.journal_fault, None);
    assert_eq!(loaded.state.system_overrides[&1][&1], Some(new));
}

/// An ADD for library 1 whose declared payload runs past the log's end.
fn over_read_add() -> Vec<u8> {
    let mut record = 1_u32.to_le_bytes().to_vec();
    record.extend_from_slice(&0x0100_0006_u32.to_le_bytes());
    record.extend_from_slice(&6_u16.to_le_bytes());
    record.extend_from_slice(&[1, 0, 0]);
    record
}

/// `payload` then an over-read, at init and at reload: the two replays
/// that share the unmasked walk.
fn init_and_reload_with_over_read(
    name: &str,
    records: &[LogRecord],
) -> [Result<Loaded, JournalFault>; 2] {
    let mut payload = encode_log_records(records).expect("log");
    payload.extend_from_slice(&over_read_add());
    let init = load_with_log(&format!("{name}-init"), &payload);
    let init = match init.journal_fault {
        Some(fault) => Err(fault),
        None => Ok(init),
    };
    let dir = tempdir(&format!("{name}-reload"));
    std::fs::write(
        dir.join("gb_char.dbin"),
        build_memory_chunk(&payload).expect("frame"),
    )
    .expect("write");
    let reload = load_library_logs(&dir, &originals()).map(|(replayed, library_totals)| {
        let mut state = UserState::default();
        for (token, item) in
            crate::user::system_items::SystemItems::new(replayed).changed_items(&originals())
        {
            state
                .system_overrides
                .get_or_default(crate::vocab::phrase_index_library_index(token))
                .unwrap()
                .insert(token & crate::vocab::PHRASE_MASK, item);
        }
        Loaded {
            state,
            library_totals,
            ..Loaded::default()
        }
    });
    std::fs::remove_dir_all(&dir).expect("cleanup");
    [init, reload]
}

/// Basis: policy row 108 (`memory_chunk.h:390` comes before the over-read).
#[test]
fn a_short_add_before_an_over_read_is_the_fault() {
    let (mut records, _) = accepted_modify();
    records.push(LogRecord::Add {
        token: 0x0100_0005,
        new_item: vec![1, 0, 0, 0, 0],
    });
    for replay in init_and_reload_with_over_read("short-add-over-read", &records) {
        assert_eq!(
            replay.err(),
            Some(JournalFault::ShortAdd {
                library: 1,
                token: 0x0100_0005,
            })
        );
    }
}

/// Basis: `phrase_index_logger.h:202` (the header-token assert comes before the over-read).
#[test]
fn a_header_token_before_an_over_read_is_the_fault() {
    let (records, _) = accepted_modify();
    let mut payload = encode_log_records(&records).expect("log");
    payload.extend_from_slice(&header_with_token(0x0100_0001));
    payload.extend_from_slice(&over_read_add());
    let loaded = load_with_log("header-token-over-read", &payload);
    assert_eq!(
        loaded.journal_fault,
        Some(JournalFault::HeaderToken {
            library: 1,
            token: 0x0100_0001,
        })
    );
}

/// Basis: `074a2219 phrase_index.cpp:468-470` (merge returns before the over-read).
#[test]
fn a_mismatch_before_an_over_read_keeps_the_records_before_the_stop() {
    let (mut records, new) = accepted_modify();
    records.push(LogRecord::Remove {
        token: 2,
        old_item: vec![0; 6],
    });
    let without = load_with_log(
        "mismatch-no-over-read",
        &encode_log_records(&records).expect("log"),
    );
    for replay in init_and_reload_with_over_read("mismatch-over-read", &records) {
        let loaded = replay.expect("the merge stops before the over-read");
        assert_eq!(loaded.state.system_overrides[&1][&1], Some(new.clone()));
        assert_eq!(
            loaded.library_totals.get(&1),
            without.library_totals.get(&1)
        );
    }
}

/// Basis: class (b), policy row 107 (an over-read skips the whole log at init and reload).
#[test]
fn an_over_read_the_walk_reaches_skips_the_whole_log_at_reload_too() {
    let (records, _) = accepted_modify();
    let [init, reload] = init_and_reload_with_over_read("over-read-only", &records);
    let init = init.expect("class (b): no fault");
    assert_eq!(init.skipped.len(), 1, "{:?}", init.skipped);
    for loaded in [init, reload.expect("class (b): no fault")] {
        assert!(
            loaded
                .state
                .system_overrides
                .get(&1)
                .is_none_or(std::collections::BTreeMap::is_empty)
        );
        assert!(!loaded.library_totals.contains_key(&1));
    }
}

/// Basis: `phrase_index.cpp:745,851-869` (type 9 sets `m_error`; later passes read nothing).
#[test]
fn an_unknown_record_type_ends_every_masked_walk() {
    let header = LogRecord::ModifyHeader {
        old_total: 150,
        new_total: 150,
    };
    let add = LogRecord::Add {
        token: 0x0100_0005,
        new_item: vec![1, 0, 7, 0, 0, 0, 0, 0],
    };
    let mut payload = encode_log_records(&[header, add]).expect("log");
    payload.extend_from_slice(&[9, 0, 0, 0, 1, 0, 0, 1]);
    let dir = tempdir("mask-unknown-type");
    std::fs::write(
        dir.join("gb_char.dbin"),
        build_memory_chunk(&payload).expect("frame"),
    )
    .expect("write");
    let result = load_masked_logs(&dir, &originals(), 0x0FFF_FFFF, 0x0100_0009);
    std::fs::remove_dir_all(&dir).expect("cleanup");
    let (_, totals) = result.expect("the mask completes");
    assert_eq!(totals.get(&1), Some(&150));
    let header = LogRecord::ModifyHeader {
        old_total: 150,
        new_total: 150,
    };
    let mut payload = encode_log_records(&[header.clone(), header]).expect("log");
    payload.extend_from_slice(&[9, 0, 0, 0, 1, 0, 0, 1]);
    let dir = tempdir("mask-unknown-type-headers");
    std::fs::write(
        dir.join("gb_char.dbin"),
        build_memory_chunk(&payload).expect("frame"),
    )
    .expect("write");
    let result = load_masked_logs(&dir, &originals(), 0x0FFF_FFFF, 0x0100_0009);
    std::fs::remove_dir_all(&dir).expect("cleanup");
    assert_eq!(
        result.err(),
        Some(JournalFault::MultipleHeaders { library: 1 })
    );
}

/// Basis: `phrase_index.cpp:745`, `phrase_index_logger.h:245` (the mask faults come before the over-read).
#[test]
fn mask_out_faults_before_an_over_read_come_first() {
    let header = LogRecord::ModifyHeader {
        old_total: 150,
        new_total: 150,
    };
    let add = LogRecord::Add {
        token: 0x0100_0005,
        new_item: vec![1, 0, 7, 0, 0, 0, 0, 0],
    };
    for (name, records, fault) in [
        (
            "mask-headers-over-read",
            vec![header.clone(), header.clone()],
            JournalFault::MultipleHeaders { library: 1 },
        ),
        (
            "mask-surviving-over-read",
            vec![header, add],
            JournalFault::SurvivingRecord {
                library: 1,
                token: 0x0100_0005,
            },
        ),
    ] {
        let mut payload = encode_log_records(&records).expect("log");
        payload.extend_from_slice(&over_read_add());
        let dir = tempdir(name);
        std::fs::write(
            dir.join("gb_char.dbin"),
            build_memory_chunk(&payload).expect("frame"),
        )
        .expect("write");
        let result = load_masked_logs(&dir, &originals(), 0x0FFF_FFFF, 0x0100_0009);
        std::fs::remove_dir_all(&dir).expect("cleanup");
        assert_eq!(result.err(), Some(fault), "{name}");
    }
}

/// Basis: `phrase_index_logger.h:214-216` (`next_record` answers false; earlier records stay).
#[test]
fn an_unknown_record_type_keeps_the_records_before_it() {
    let (records, new) = accepted_modify();
    let mut payload = encode_log_records(&records).expect("log");
    payload.extend_from_slice(&[9, 0, 0, 0, 1, 0, 0, 0]);
    let loaded = load_with_log("unknown-type", &payload);
    assert_eq!(loaded.journal_fault, None);
    assert_eq!(loaded.state.system_overrides[&1][&1], Some(new));
}

/// Basis: Rust contract (an unreadable profile file is skipped alone).
#[test]
fn hostile_profile_files_degrade_per_file() {
    let dir = tempdir("hostile");
    let originals = originals();
    save_state_reporting(&dir, &state(), &originals, &versions(), 1).expect("save");

    let mut chunk = std::fs::read(dir.join("user.bin")).expect("read");
    let last = chunk.len() - 1;
    chunk[last] ^= 0xFF;
    std::fs::write(dir.join("user.bin"), chunk).expect("write");
    std::fs::write(dir.join(UserDbm::Bigram.file_name()), b"not a dbm").expect("write");

    let loaded = load(&dir, &originals, &versions(), UserConfLaw::Pinyin).expect("load");
    assert!(!loaded.wiped, "user.conf is intact; only files skip");
    // Bigram::load_db leaves an empty container after a failed file load;
    // init ignores its false answer (074a2219 pinyin.cpp:399-401).
    assert_eq!(loaded.skipped, ["user.bin: invalid data"]);
    assert!(loaded.state.bigram.is_empty());
    assert!(!loaded.state.libraries.contains_key(&7));
    assert!(loaded.state.system_overrides.contains_key(&1));

    std::fs::remove_dir_all(&dir).expect("cleanup");
}

/// The frozen record golden, the same on Kyoto Cabinet, tkrzw and Berkeley DB.
const RECORDS_GOLDEN: &str = r#"## user_bigram (2 rows)
  01000000 = cf0000000100000045000000020000018a0000000100000745000000
  01000007 = 450000000100000545000000
## user_pinyin_index (12 rows)
  0200 = 
  02000f00 = 0100000582008f03
  0300 = 
  03000b00 = 0200000783028b01
  1700 = 
  17000700 = 0100000797008706
  8200 = 
  82008f03 = 0100000582008f03
  8302 = 
  83028b01 = 0200000783028b01
  9700 = 
  97008706 = 0100000797008706
## user_phrase_index (6 rows)
  17530000 = 
  17530000ac4e0000 = 01000005
  4b6d0000 = 
  4b6d0000d58b0000 = 02000007
  604f0000 = 
  604f00007d590000 = 01000007
## user.bin (83 payload bytes)
  2d000000110000001e000000530000002300000000080000001e00000023000000000000000002010f000000604f00007d590000970087060f00000002011e0000004b6d0000d58b000083028b011e00000023
## addon.bin (57 payload bytes)
  09000000110000001a0000003900000023000000000800000023000000000000000002010900000017530000ac4e000082008f030900000023
## network.bin (19 payload bytes)
  00000000110000001200000013000000232323
## gb_char.dbin (62 payload bytes)
  040000000000000004006400000047020000030000000100000010001000010164000000604f0000970064000000010147020000604f00009700a9000000
## gbk_char.dbin (18 payload bytes)
  040000000000000004000000000000000000
## opengram.dbin (18 payload bytes)
  040000000000000004000000000000000000
## merged.dbin (18 payload bytes)
  040000000000000004000000000000000000"#;

/// Basis: frozen golden (each backend build saves the same records; CI runs every backend).
#[test]
fn the_saved_records_are_backend_independent() {
    let dir = tempdir("records");
    let (state, originals) = cross_backend_state();
    save_state_reporting(&dir, &state, &originals, &versions(), 1).expect("save");
    let got = dump_user_dir(&dir);
    assert_eq!(
        got.trim_end(),
        RECORDS_GOLDEN,
        "the backend's records drifted from the frozen golden"
    );
    std::fs::remove_dir_all(&dir).expect("cleanup");
}

/// A fixed user state in which every table of the file set is non-empty.
fn cross_backend_state() -> (UserState, SystemOriginals) {
    let k = |i: u8, m: u8, f: u8| ChewingKey::new(i, m, f, 0).to_packed();
    let mut libraries = crate::user::LibraryTable::new();
    libraries.insert(
        7_u8,
        BTreeMap::from([
            (
                1_u32,
                ChunkItem {
                    phrase: vec![0x4f60, 0x597d],
                    unigram: 15,
                    prons: vec![(vec![k(23, 0, 1), k(7, 0, 13)], 15)],
                },
            ),
            (
                2_u32,
                ChunkItem {
                    phrase: vec![0x6d4b, 0x8bd5],
                    unigram: 30,
                    prons: vec![(vec![k(3, 0, 5), k(11, 0, 3)], 30)],
                },
            ),
        ]),
    );
    libraries.insert(
        5_u8,
        BTreeMap::from([(
            1_u32,
            ChunkItem {
                phrase: vec![0x5317, 0x4eac],
                unigram: 9,
                prons: vec![(vec![k(2, 0, 1), k(15, 0, 7)], 9)],
            },
        )]),
    );
    let gram = crate::single_gram::encode_single_gram;
    let bigram = vec![
        (
            1_u32.to_le_bytes().to_vec(),
            gram(207, &[(1, 69), (0x0100_0002, 138), (0x0700_0001, 69)]),
        ),
        (
            0x0700_0001_u32.to_le_bytes().to_vec(),
            gram(69, &[(0x0500_0001, 69)]),
        ),
    ];
    let mut system_overrides = crate::user::LibraryTable::new();
    system_overrides.insert(
        1_u8,
        BTreeMap::from([(
            1_u32,
            Some(ChunkItem {
                phrase: vec![0x4f60],
                unigram: 583,
                prons: vec![(vec![k(23, 0, 1)], 169)],
            }),
        )]),
    );
    let originals = crate::user::LibraryTable::from([(
        1_u8,
        SystemLibrary {
            mapped: None,
            total: 100,
            range_end: 2,
            items: BTreeMap::from([(
                1_u32,
                ChunkItem {
                    phrase: vec![0x4f60],
                    unigram: 100,
                    prons: vec![(vec![k(23, 0, 1)], 100)],
                },
            )]),
        },
    )]);
    (
        UserState {
            bigram,
            libraries,
            system_overrides,
            phrase_table: BTreeMap::new(),
            indexed: std::collections::BTreeSet::new(),
        }
        .index_every_reading(),
        originals.into(),
    )
}

/// Every DBM row and chunk payload in `dir`, as the golden's hex dump.
fn dump_user_dir(dir: &Path) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    for dbm in [UserDbm::Bigram, UserDbm::PinyinIndex, UserDbm::PhraseIndex] {
        let path = dir.join(dbm.file_name());
        let mut rows = crate::user::native_fixture::store_rows(&path, dbm.is_hash());
        rows.sort();
        let _ = writeln!(out, "## {} ({} rows)", dbm.stem(), rows.len());
        for (k, v) in rows {
            let _ = writeln!(out, "  {} = {}", hex(&k), hex(&v));
        }
    }
    for name in [
        "user.bin",
        "addon.bin",
        "network.bin",
        "gb_char.dbin",
        "gbk_char.dbin",
        "opengram.dbin",
        "merged.dbin",
    ] {
        let bytes = std::fs::read(dir.join(name)).expect("chunk file");
        let payload = read_chunk_payload(&bytes).expect("frame");
        let _ = writeln!(
            out,
            "## {name} ({} payload bytes)\n  {}",
            payload.len(),
            hex(payload)
        );
    }
    out
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

mod save_semantics_929 {
    //! Basis: `074a2219 pinyin.cpp:922-1146`, `zhuyin.cpp:547-698`, `memory_chunk.h:536-557`.

    use std::collections::BTreeMap;

    use std::path::Path;

    use crate::data::phrase_library::ChunkItem;

    use crate::data::TableConf;

    use crate::data::user_files::UserFileLayout;

    use crate::user::UserStore;

    use crate::user::{SaveBaseline, UserConfLaw};

    /// The stock layout's four system libraries, one item each.
    fn system() -> (std::path::PathBuf, TableConf) {
        let conf = TableConf::stock(0.312_699);
        let item = || {
            vec![(
                1,
                ChunkItem {
                    phrase: vec![0x4f60],
                    unigram: 100,
                    prons: Vec::new(),
                },
            )]
        };
        let names: Vec<String> = conf
            .system_libraries()
            .filter_map(|(_, row)| Some(row.system.as_ref()?.display().into_owned()))
            .collect();
        assert_eq!(names.len(), 4);
        let chunks: Vec<(&str, Vec<_>)> =
            names.iter().map(|name| (name.as_str(), item())).collect();
        (crate::user::test_session::system_dir(&chunks), conf)
    }

    fn open(dir: &Path, law: UserConfLaw) -> UserStore {
        let (system, conf) = system();
        crate::user::test_session::open_with_conf(dir, &system, &conf, law)
    }

    fn seed(dir: &Path, law: UserConfLaw) -> UserStore {
        let mut store = open(dir, law);
        store.mark_modified();
        store.save_reporting().expect("seed save");
        store
    }

    fn save_destinations(dir: &Path) -> Vec<std::path::PathBuf> {
        let layout = UserFileLayout::stock();
        let mut libraries: Vec<_> = layout
            .system_logs()
            .iter()
            .chain(layout.user_libraries())
            .collect();
        libraries.sort_by_key(|(index, _)| *index);
        let mut names: Vec<_> = libraries
            .into_iter()
            .map(|(_, name)| crate::input::build_path(dir, name.as_ref()))
            .collect();
        names.extend(
            [
                "user_pinyin_index.bin",
                "user_phrase_index.bin",
                "user_bigram.db",
            ]
            .map(|name| dir.join(name)),
        );
        names
    }

    fn block_destination(path: &Path) {
        std::fs::remove_file(path).unwrap();
        std::fs::create_dir(path).unwrap();
        std::fs::write(path.join("keep"), b"previous data").unwrap();
    }

    fn staged_path(path: &Path) -> std::path::PathBuf {
        let mut staged = path.as_os_str().to_owned();
        staged.push(".tmp");
        staged.into()
    }

    /// Basis: `pinyin.cpp:1032-1146`, `zhuyin.cpp:585-617` (soft rename and marker failures, in order).
    #[test]
    fn failed_renames_report_exact_order_and_preserve_destinations() {
        const CHILD: &str = "OXPINYIN_929_RENAME_CHILD";
        const MISSING: &str = "OXPINYIN_929_RENAME_MISSING";
        const ZHUYIN: &str = "OXPINYIN_929_RENAME_ZHUYIN";
        if let Some(dir) = std::env::var_os(CHILD) {
            let dir = Path::new(&dir);
            let law = if std::env::var_os(ZHUYIN).is_some() {
                UserConfLaw::Zhuyin
            } else {
                UserConfLaw::Pinyin
            };
            let mut store = seed(dir, law);
            let missing = std::env::var_os(MISSING).is_some();
            if missing {
                std::fs::remove_dir_all(dir).unwrap();
            } else {
                for path in save_destinations(dir) {
                    block_destination(&path);
                }
            }
            store.mark_modified();
            store
                .save_reporting()
                .expect("rename and marker failures are soft");
            assert!(
                !store.is_modified(),
                "the pin clears dirty even after failed renames"
            );
            store.save_reporting().expect("clean save does not retry");
            if missing {
                // Allow the separate fini marker write after the failed save.
                std::fs::create_dir(dir).unwrap();
            }
            return;
        }

        for (law, missing) in [(UserConfLaw::Pinyin, false), (UserConfLaw::Zhuyin, true)] {
            let dir = tempfile::tempdir().unwrap();
            let mut env = vec![(CHILD, dir.path().as_os_str().to_owned())];
            if missing {
                env.push((MISSING, "1".into()));
            }
            if law == UserConfLaw::Zhuyin {
                env.push((ZHUYIN, "1".into()));
            }
            let output = crate::user::test_child::run_exact(
                "user::persistence::tests::save_semantics_929::failed_renames_report_exact_order_and_preserve_destinations",
                env,
            );
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(output.status.success(), "{law:?}: {stderr}");
            let mut expected = Vec::new();
            for path in save_destinations(dir.path()) {
                let tmp = staged_path(&path);
                expected.push(format!(
                    "rename {} to {} failed.",
                    tmp.display(),
                    path.display()
                ));
                if !missing {
                    assert!(tmp.is_file(), "failed rename keeps the staged file");
                    assert_eq!(std::fs::read(path.join("keep")).unwrap(), b"previous data");
                }
            }
            let marker_failure =
                format!("write {} failed.", dir.path().join("user.conf").display());
            if missing {
                expected.push(marker_failure.clone());
            }
            let actual: Vec<_> = stderr
                .lines()
                .filter_map(|line| {
                    line.find("rename ")
                        .or_else(|| line.find(&marker_failure))
                        .map(|start| &line[start..])
                })
                .collect();
            assert_eq!(actual, expected, "{law:?}, missing={missing}");
        }
    }

    /// The profile's files other than `user.conf`.
    fn data_bytes(dir: &Path) -> BTreeMap<std::ffi::OsString, Vec<u8>> {
        std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap())
            .filter(|entry| entry.file_name() != "user.conf")
            .map(|entry| (entry.file_name(), std::fs::read(entry.path()).unwrap()))
            .collect()
    }

    /// Basis: `pinyin.cpp:1194-1200`, `zhuyin.cpp:741-757` (fini is no save; a conforming reopen writes no data file).
    #[test]
    fn save_reload_keeps_bigrams_and_unsaved_changes_do_not_replace_them() {
        for law in [UserConfLaw::Pinyin, UserConfLaw::Zhuyin] {
            let dir = tempfile::tempdir().unwrap();
            let mut store = seed(dir.path(), law);
            crate::user::seed_bigram(&mut store, 0x0100_0001, &[(0x0200_0001, 37)]);
            store.mark_modified();
            store.save_reporting().unwrap();
            let saved_bytes = data_bytes(dir.path());
            crate::user::seed_bigram(&mut store, 0x0100_0001, &[(0x0200_0001, 91)]);
            drop(store); // fini is not an implicit save (both facades).
            let mut reopened = open(dir.path(), law);
            assert_eq!(
                data_bytes(dir.path()),
                saved_bytes,
                "fini and conforming reload must not rewrite any saved data file"
            );
            assert_eq!(reopened.bigram_count(0x0100_0001, 0x0200_0001).unwrap(), 37);
            reopened.mark_modified();
            reopened.save_reporting().unwrap();
            let resaved_bytes = data_bytes(dir.path());
            drop(reopened);
            let reopened = open(dir.path(), law);
            assert_eq!(reopened.bigram_count(0x0100_0001, 0x0200_0001).unwrap(), 37);
            assert_eq!(data_bytes(dir.path()), resaved_bytes);
        }
    }

    /// Basis: `pinyin.cpp:1032-1146` (a soft rename failure clears `m_modified`; the next save writes all).
    #[test]
    fn training_after_a_soft_rename_failure_saves_the_full_profile() {
        const SYSTEM: u32 = 0x0100_0001;
        const USER: u32 = 0x0700_0001;
        for law in [UserConfLaw::Pinyin, UserConfLaw::Zhuyin] {
            let dir = tempfile::tempdir().unwrap();
            let control = tempfile::tempdir().unwrap();
            let mut store = seed(dir.path(), law);
            let mut uninterrupted = seed(control.path(), law);
            let blocked = dir.path().join("user_phrase_index.bin");
            block_destination(&blocked);

            let reading = [crate::vocab::SyllableKey::from_text("ni").unwrap().index() as u16];
            for session in [&mut store, &mut uninterrupted] {
                assert_eq!(
                    session
                        .add_phrase(&"你".into(), &reading[..], Some(7))
                        .unwrap(),
                    USER
                );
                session.mark_modified();
            }
            store.save_reporting().expect("soft failure still succeeds");
            assert!(!store.is_modified());
            assert_eq!(
                std::fs::read(blocked.join("keep")).unwrap(),
                b"previous data"
            );
            assert!(staged_path(&blocked).is_file());
            std::fs::remove_dir_all(&blocked).unwrap();

            for session in [&mut store, &mut uninterrupted] {
                session.observe_selection(SYSTEM, USER).unwrap();
                assert!(session.is_modified());
                session.save_reporting().unwrap();
                assert!(!session.is_modified());
            }
            let phrases = crate::user::test_session::stored_phrase_rows(&store, 7);
            let system_unigram = store.system_unigram(SYSTEM).expect("live system item");
            let unigram = store.user_unigram_frequency(USER);
            let gram = store.bigram_row(SYSTEM).unwrap();
            drop(store);
            drop(uninterrupted);

            let load = |path| {
                let session = open(path, law);
                let grams: Vec<_> = session
                    .bigram_predecessors()
                    .unwrap()
                    .into_iter()
                    .map(|prev| (prev, session.bigram_row(prev).unwrap()))
                    .collect();
                (
                    session.phrases(),
                    [5, 6, 7].map(|library| {
                        crate::user::test_session::stored_phrase_rows(&session, library)
                    }),
                    grams,
                    session.unigram_total(),
                    session
                        .system_unigram(SYSTEM)
                        .expect("reopened system item"),
                    session.user_unigram_frequency(USER),
                )
            };
            assert_eq!(load(dir.path()), load(control.path()), "{law:?}");
            assert!(
                save_destinations(dir.path())
                    .iter()
                    .all(|path| { path.is_file() && !staged_path(path).exists() })
            );
            let reopened = open(dir.path(), law);
            assert_eq!(
                crate::user::test_session::stored_phrase_rows(&reopened, 7),
                phrases
            );
            assert_eq!(reopened.system_unigram(SYSTEM), Some(system_unigram));
            assert_eq!(reopened.user_unigram_frequency(USER), unigram);
            assert_eq!(reopened.bigram_row(SYSTEM).unwrap(), gram);
            assert_eq!(reopened.token_for_phrase("你").unwrap(), Some(USER));
        }
    }

    /// Basis: `memory_chunk.h:555` (the pin ignores the fsync result); `/dev/null` rejects fsync.
    #[cfg(target_os = "linux")]
    #[test]
    fn sync_rejecting_chunk_target_does_not_stop_later_files() {
        assert!(
            std::fs::OpenOptions::new()
                .write(true)
                .open("/dev/null")
                .unwrap()
                .sync_all()
                .is_err()
        );
        for law in [UserConfLaw::Pinyin, UserConfLaw::Zhuyin] {
            let dir = tempfile::tempdir().unwrap();
            let mut store = seed(dir.path(), law);
            std::os::unix::fs::symlink("/dev/null", dir.path().join("gb_char.dbin.tmp")).unwrap();
            store.mark_modified();
            store.save_reporting().unwrap();
            assert_eq!(
                std::fs::read_link(dir.path().join("gb_char.dbin")).unwrap(),
                Path::new("/dev/null")
            );
            assert!(dir.path().join("user_bigram.db").is_file());
            assert!(!store.is_modified());
        }
    }

    /// Basis: `pinyin.cpp:933-1020` (staged), `zhuyin.cpp:585-617` (renamed at once), `table.conf.in:9-12` order.
    #[test]
    fn interrupted_save_preserves_the_facade_specific_committed_prefix() {
        const CHILD: &str = "OXPINYIN_929_INTERRUPT_DIR";
        if let Some(dir) = std::env::var_os(CHILD) {
            let law = if std::env::var_os("OXPINYIN_929_ZHUYIN").is_some() {
                UserConfLaw::Zhuyin
            } else {
                UserConfLaw::Pinyin
            };
            let stop_before: u8 = std::env::var("OXPINYIN_929_STOP_BEFORE")
                .unwrap()
                .parse()
                .unwrap();
            let mut store = open(Path::new(&dir), law);
            for index in 1..=4_u8 {
                store.add_system_unigram_frequency(
                    crate::vocab::phrase_index_make_token(index, 1),
                    17,
                );
            }
            store.mark_modified();
            store
                .save_reporting_with_baseline(&[], |index, _| {
                    if index == stop_before {
                        std::process::exit(73);
                    }
                    SaveBaseline::Keep
                })
                .unwrap();
            panic!("the selected library boundary must be reached");
        }
        const LOGS: [&str; 4] = [
            "gb_char.dbin",
            "gbk_char.dbin",
            "opengram.dbin",
            "merged.dbin",
        ];
        for law in [UserConfLaw::Pinyin, UserConfLaw::Zhuyin] {
            for stop_before in [2_usize, 4] {
                let dir = tempfile::tempdir().unwrap();
                let seeded = seed(dir.path(), law);
                let shipped: Vec<u32> = (1..=4_u8)
                    .map(|index| {
                        seeded
                            .system_unigram(crate::vocab::phrase_index_make_token(index, 1))
                            .expect("item")
                    })
                    .collect();
                drop(seeded);
                let old = data_bytes(dir.path());
                let mut env = vec![
                    (CHILD, dir.path().as_os_str().to_owned()),
                    ("OXPINYIN_929_STOP_BEFORE", stop_before.to_string().into()),
                ];
                if law == UserConfLaw::Zhuyin {
                    env.push(("OXPINYIN_929_ZHUYIN", "1".into()));
                }
                let output = crate::user::test_child::run_exact(
                    "user::persistence::tests::save_semantics_929::interrupted_save_preserves_the_facade_specific_committed_prefix",
                    env,
                );
                assert_eq!(output.status.code(), Some(73));
                for (index, name) in LOGS.iter().enumerate() {
                    let final_bytes = std::fs::read(dir.path().join(name)).unwrap();
                    let staged = dir.path().join(format!("{name}.tmp"));
                    let old_bytes = &old[std::ffi::OsStr::new(name)];
                    if index + 1 < stop_before && law == UserConfLaw::Pinyin {
                        assert!(staged.is_file(), "each earlier log must be staged");
                        assert_eq!(&final_bytes, old_bytes);
                        assert_ne!(&std::fs::read(staged).unwrap(), old_bytes);
                    } else {
                        assert!(
                            !staged.exists(),
                            "no later staging or retained zhuyin staging"
                        );
                        if index + 1 < stop_before {
                            assert_ne!(&final_bytes, old_bytes);
                        } else {
                            assert_eq!(&final_bytes, old_bytes);
                        }
                    }
                }
                let reopened = open(dir.path(), law);
                for index in 1..=4_u8 {
                    assert_eq!(
                        reopened.system_unigram(crate::vocab::phrase_index_make_token(index, 1)),
                        Some(
                            shipped[usize::from(index) - 1]
                                + if law == UserConfLaw::Zhuyin && usize::from(index) < stop_before
                                {
                                    17
                                } else {
                                    0
                                }
                        )
                    );
                }
            }
        }
    }
}

mod save_boundaries_929 {
    //! Basis: `074a2219 pinyin.cpp:922-1146,1194-1200`, `zhuyin.cpp:547-698,741-757`;
    //! GLib 2.89.4 `gstdio.h:60-82`. Process interruption only; the missing chunk sync is #1221.

    use std::collections::BTreeMap;

    use std::ffi::OsString;

    use std::path::Path;

    use crate::vocab::SyllableKey;

    use crate::data::phrase_library::ChunkItem;

    use crate::data::TableConf;

    use crate::data::user_files::UserTableInfo;

    use crate::user::UserConfLaw;

    use crate::user::{PinyinKey, UserStore};

    const CHILD: &str = "OXPINYIN_929_LATE_DIR";

    const CHUNKS: [&str; 2] = ["addon.bin", "network.bin"];

    const INDEXES: [&str; 3] = [
        "user_pinyin_index.bin",
        "user_phrase_index.bin",
        "user_bigram.db",
    ];

    /// A `table.conf` whose late system row follows the two user libraries
    /// (`pinyin.cpp:927-938`, `zhuyin.cpp:560-571`, `table_info.cpp:145-156,266-286`).
    fn system() -> (std::path::PathBuf, TableConf) {
        let dir = crate::user::test_session::system_dir(&[(
            "late.bin",
            vec![(
                1,
                ChunkItem {
                    phrase: vec![0x4f60],
                    unigram: 100,
                    prons: Vec::new(),
                },
            )],
        )]);
        let conf = format!(
            "binary format version:7\nmodel data version:14\nlambda parameter:0.312699\n\
         source table format:pinyin\ndatabase format:{}\n\
         default ADDON_DICTIONARY NULL NULL addon.bin USER_FILE\n\
         default NETWORK_DICTIONARY NULL NULL network.bin USER_FILE\n\
         default USER_DICTIONARY late.table late.bin late.dbin SYSTEM_FILE\n",
            crate::user::test_session::versions().database_format
        );
        std::fs::write(dir.join("table.conf"), conf).unwrap();
        let conf = TableConf::load_from_dir(&dir).unwrap();
        (dir, conf)
    }

    fn open(dir: &Path, law: UserConfLaw) -> UserStore {
        let (system, conf) = system();
        crate::user::test_session::open_with_conf(dir, &system, &conf, law)
    }

    fn change(store: &mut UserStore) {
        let key = PinyinKey::try_from(SyllableKey::from_text("ni").unwrap().index()).unwrap();
        for library in 5..=6_u8 {
            store
                .add_phrase_in(library, &"你".into(), &[key], Some(17))
                .unwrap();
        }
        crate::user::seed_bigram(store, 0x0700_0001, &[(0x0600_0001, 37)]);
        store.mark_modified();
    }

    fn seed(dir: &Path, law: UserConfLaw) {
        let mut store = open(dir, law);
        store.mark_modified();
        store.save_reporting().unwrap();
    }

    fn bytes(dir: &Path) -> BTreeMap<OsString, Vec<u8>> {
        std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), std::fs::read(entry.path()).unwrap())
            })
            .collect()
    }

    fn counter(dir: &Path) -> i32 {
        UserTableInfo::load_path(&dir.join("user.conf"))
            .unwrap()
            .open_counter
    }

    fn run_child(dir: &Path, law: UserConfLaw, test: &str, boundary: &str) {
        let law = if law == UserConfLaw::Pinyin {
            "pinyin"
        } else {
            "zhuyin"
        };
        let output = crate::user::test_child::run_exact(
            test,
            [
                (CHILD, dir.as_os_str()),
                ("OXPINYIN_929_LATE_LAW", std::ffi::OsStr::new(law)),
                ("OXPINYIN_929_LATE_BOUNDARY", std::ffi::OsStr::new(boundary)),
            ],
        );
        assert_eq!(
            output.status.code(),
            Some(73),
            "child must exit without unwinding"
        );
    }

    fn child_store() -> Option<UserStore> {
        let dir = std::env::var_os(CHILD)?;
        let law = if std::env::var("OXPINYIN_929_LATE_LAW").unwrap() == "pinyin" {
            UserConfLaw::Pinyin
        } else {
            UserConfLaw::Zhuyin
        };
        let mut store = open(Path::new(&dir), law);
        change(&mut store);
        Some(store)
    }

    /// Basis: `pinyin.cpp:933-1020` (chunks staged, indexes later), `zhuyin.cpp:585-617`.
    #[test]
    fn interruption_after_user_chunks_keeps_indexes_and_bigram_at_previous_save() {
        if let Some(mut store) = child_store() {
            store
                .save_reporting_with_baseline(&[], |index, _| {
                    assert_eq!(index, 7);
                    std::process::exit(73);
                })
                .unwrap();
            panic!("late system remap boundary must be reached");
        }
        for law in [UserConfLaw::Pinyin, UserConfLaw::Zhuyin] {
            let dir = tempfile::tempdir().unwrap();
            seed(dir.path(), law);
            let before = bytes(dir.path());
            run_child(
                dir.path(),
                law,
                "user::persistence::tests::save_boundaries_929::interruption_after_user_chunks_keeps_indexes_and_bigram_at_previous_save",
                "chunks",
            );
            for name in CHUNKS {
                let final_bytes = std::fs::read(dir.path().join(name)).unwrap();
                let staged = dir.path().join(format!("{name}.tmp"));
                if law == UserConfLaw::Pinyin {
                    assert_eq!(final_bytes, before[std::ffi::OsStr::new(name)]);
                    assert_ne!(std::fs::read(staged).unwrap(), final_bytes);
                } else {
                    assert_ne!(final_bytes, before[std::ffi::OsStr::new(name)]);
                    assert!(!staged.exists());
                }
            }
            for name in INDEXES.into_iter().chain(["late.dbin"]) {
                assert_eq!(
                    std::fs::read(dir.path().join(name)).unwrap(),
                    before[std::ffi::OsStr::new(name)]
                );
                assert!(!dir.path().join(format!("{name}.tmp")).exists());
            }
            assert_eq!(counter(dir.path()), i32::from(law == UserConfLaw::Pinyin));
            let reopened = open(dir.path(), law);
            assert_eq!(reopened.bigram_count(0x0700_0001, 0x0600_0001).unwrap(), 0);
            for library in 5..=6_u8 {
                assert_eq!(
                    reopened
                        .phrase(crate::vocab::phrase_index_make_token(library, 1))
                        .is_some(),
                    law == UserConfLaw::Zhuyin
                );
            }
        }
    }

    /// Basis: `pinyin.cpp:1141-1146,1194-1200`, `zhuyin.cpp:695,741-757` (the counter each boundary leaves).
    #[test]
    fn interruption_after_save_or_fini_retains_every_saved_data_byte() {
        if let Some(mut store) = child_store() {
            store.save_reporting().unwrap();
            assert!(!store.is_modified());
            if std::env::var("OXPINYIN_929_LATE_BOUNDARY").unwrap() == "fini" {
                drop(store);
            }
            std::process::exit(73);
        }
        for law in [UserConfLaw::Pinyin, UserConfLaw::Zhuyin] {
            for boundary in ["save", "fini"] {
                let dir = tempfile::tempdir().unwrap();
                seed(dir.path(), law);
                run_child(
                    dir.path(),
                    law,
                    "user::persistence::tests::save_boundaries_929::interruption_after_save_or_fini_retains_every_saved_data_byte",
                    boundary,
                );
                assert_eq!(
                    counter(dir.path()),
                    i32::from(law == UserConfLaw::Pinyin && boundary == "save")
                );
                let mut saved = bytes(dir.path());
                saved.remove(std::ffi::OsStr::new("user.conf"));
                assert!(
                    saved
                        .keys()
                        .all(|name| !name.to_string_lossy().ends_with(".tmp"))
                );
                let reopened = open(dir.path(), law);
                assert_eq!(reopened.bigram_count(0x0700_0001, 0x0600_0001).unwrap(), 37);
                for library in 5..=6_u8 {
                    assert_eq!(
                        reopened
                            .phrase(crate::vocab::phrase_index_make_token(library, 1))
                            .unwrap()
                            .phrase_text()
                            .unwrap(),
                        "你"
                    );
                }
                drop(reopened);
                let mut after = bytes(dir.path());
                after.remove(std::ffi::OsStr::new("user.conf"));
                assert_eq!(after, saved);
            }
        }
    }
}

/// Basis: `074a2219 pinyin.cpp:1132-1147` (each save writes its session's whole profile).
#[test]
fn when_two_sessions_save_the_later_save_is_the_profile() {
    use crate::user::test_session::{native_exports, native_keys, open_w3};
    {
        let user = tempfile::tempdir().unwrap();
        let mut first = open_w3(user.path());
        let mut second = open_w3(user.path());
        first
            .add_phrase_in(7, &"你好".into(), &native_keys("nihao"), Some(5))
            .unwrap();
        second
            .add_phrase_in(7, &"世界".into(), &native_keys("shijie"), Some(3))
            .unwrap();
        first.compact_libraries();
        second.compact_libraries();
        first.mark_modified();
        second.mark_modified();
        let exported = |store: &crate::user::UserStore| {
            native_exports(store, 7)
                .into_iter()
                .map(|(phrase, _, count)| (phrase, count))
                .collect::<Vec<_>>()
        };
        assert_eq!(exported(&first), [("你好".to_owned(), 5)]);
        assert_eq!(exported(&second), [("世界".to_owned(), 3)]);
        second.save_reporting().unwrap();
        first.save_reporting().unwrap();
        let expected = exported(&first);
        drop(first);
        drop(second);
        assert_eq!(exported(&open_w3(user.path())), expected);
    }
}

/// Hex of each `(key, value)` row.
fn hex_rows(rows: &[(Vec<u8>, Vec<u8>)]) -> Vec<(String, String)> {
    let hex = |bytes: &[u8]| bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    rows.iter()
        .map(|(key, value)| (hex(key), hex(value)))
        .collect()
}

/// Basis: frozen pin observation of the saved index records (reading and phrase keys with prefix markers).
#[test]
fn saved_user_indexes_hold_the_pins_records() {
    use crate::user::native_fixture::TempUserDir;
    let user = TempUserDir::new("saved-index-format");
    let mut store = crate::user::test_session::open_w3(&user.path);
    for (text, reading, count) in [
        ("龘龘", "da2'da2", 100),
        ("龘龘", "da4'da4", 100),
        ("靐靐", "bing4'bing4", 100),
    ] {
        store
            .add_phrase_in(
                7,
                &text.into(),
                &crate::user::test_session::native_keys(reading),
                Some(count),
            )
            .unwrap();
    }
    store.compact_libraries();
    store.mark_modified();
    store.save_reporting().unwrap();
    drop(store);
    let rows = |name: &str| crate::user::native_fixture::store_rows(&user.path.join(name), false);
    let pairs = |rows: &[(&str, &str)]| -> Vec<(String, String)> {
        rows.iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect()
    };
    assert_eq!(
        hex_rows(&rows("user_pinyin_index.bin")),
        pairs(&[
            ("0100", ""),
            ("01000100", "0200000781488148"),
            ("0400", ""),
            ("04000400", "0100000784208420"),
            ("8108", ""),
            ("81088108", "0200000781488148"),
            ("8400", ""),
            ("84008400", "0100000784208420"),
        ])
    );
    assert_eq!(
        hex_rows(&rows("user_phrase_index.bin")),
        pairs(&[
            ("50970000", ""),
            ("5097000050970000", "02000007"),
            ("989f0000", ""),
            ("989f0000989f0000", "01000007"),
        ])
    );
    let store = crate::user::test_session::open_w3(&user.path);
    assert_eq!(
        store.phrase_table_tokens_for_text("龘龘").unwrap(),
        [0x0700_0001]
    );
    assert_eq!(
        store.phrase_table_tokens_for_text("靐靐").unwrap(),
        [0x0700_0002]
    );
}

/// Basis: `g_build_filename(user, "/…/nest/u.bin")` is `<user>/…/nest/u.bin`.
// Unix only: the table.conf row spells a POSIX absolute path.
#[cfg(unix)]
#[test]
fn an_absolute_user_file_lands_under_the_user_directory() {
    use crate::user::native_fixture::{private_system, system_dir};
    let scratch = tempfile::tempdir().expect("scratch");
    let user = scratch.path().join("user");
    let nested = format!("{}/nest", user.display());
    let beneath = format!("{}{nested}", user.display());
    std::fs::create_dir_all(&nested).expect("the nested directory");
    std::fs::create_dir_all(&beneath).expect("the user-rooted directory");
    let system = private_system(scratch.path());
    let path = system.join("table.conf");
    let text = std::fs::read_to_string(system_dir().join("table.conf")).unwrap();
    assert!(text.contains("user.bin USER_FILE"));
    std::fs::remove_file(&path).unwrap();
    std::fs::write(
        path,
        text.replace("user.bin USER_FILE", &format!("{nested}/u.bin USER_FILE")),
    )
    .unwrap();
    let mut store = crate::user::test_session::open_native(&user, &system, UserConfLaw::Pinyin);
    store
        .add_phrase_in(
            7,
            &"你好".into(),
            &crate::user::test_session::native_keys("ni3'hao3"),
            Some(1),
        )
        .unwrap();
    store.compact_libraries();
    store.mark_modified();
    store.save_reporting().unwrap();
    drop(store);
    assert!(Path::new(&beneath).join("u.bin").is_file());
    assert!(!Path::new(&nested).join("u.bin").exists());
}

/// Basis: `ngram_tkrzwdb.cpp:55-56`, `ngram_bdb.cpp:67-70` (an unopenable user bigram is a soft `false`).
#[cfg(any(feature = "tkrzw", feature = "bdb"))]
#[test]
fn init_survives_a_user_bigram_that_does_not_open() {
    let scratch = tempfile::tempdir().unwrap();
    let user = scratch.path();
    crate::user::native_fixture::plant_bigram(
        user,
        &[
            (0x0100_0001_u32.to_le_bytes().to_vec(), vec![0; 4]),
            (0x0100_0002_u32.to_le_bytes().to_vec(), vec![0; 4]),
        ],
    );
    let bigram = user.join("user_bigram.db");
    std::fs::remove_file(&bigram).expect("unlink the user bigram");
    std::fs::create_dir(&bigram).expect("a directory in its place");
    crate::store::fail_source_read_after(0);
    let _store = crate::user::test_session::open_w3(user);
}

/// Basis: `tkrzwdb_utils.h:52-54`, `bdb_utils.h:49-52`, `ngram_tkrzwdb.cpp:58` (the copy's `false` is ignored).
#[cfg(any(feature = "tkrzw", feature = "bdb"))]
#[test]
fn init_survives_a_copy_whose_source_iterator_is_not_made() {
    let scratch = tempfile::tempdir().unwrap();
    let user = scratch.path();
    crate::user::native_fixture::plant_bigram(
        user,
        &[
            (0x0100_0001_u32.to_le_bytes().to_vec(), vec![0; 4]),
            (0x0100_0002_u32.to_le_bytes().to_vec(), vec![0; 4]),
        ],
    );
    crate::store::fail_iterator_creation_after(0);
    crate::store::fail_source_read_after(0);
    let _store = crate::user::test_session::open_w3(user);
}

/// Basis: `pinyin.cpp:1032-1146` (`g_rename` over an absent or, on Unix, empty directory).
#[test]
fn a_stale_index_tmp_directory_is_renamed_unless_a_live_file_holds_the_name() {
    const NAME: &str = "user::persistence::tests::a_stale_index_tmp_directory_is_renamed_unless_a_live_file_holds_the_name";
    const ENV: &str = "OXPINYIN_STALE_INDEX_CHILD";
    if let Some(root) = std::env::var_os(ENV) {
        let root = std::path::PathBuf::from(root);
        let marker: &[u8] = b"unchanged temporary directory";
        let system = crate::user::native_fixture::system_dir();
        // The first su3cl3 candidate is 你好, whose training arms each save.
        let token = crate::user::test_session::native_tokens(&system, "你好")[0];
        for (round, (destination, moved)) in [
            ("absent", true),
            ("empty-directory", cfg!(not(windows))),
            ("live-file", false),
        ]
        .into_iter()
        .enumerate()
        {
            let user = root.join(format!("round-{round}"));
            std::fs::create_dir(&user).unwrap();
            let mut store =
                crate::user::test_session::open_native(&user, &system, UserConfLaw::Zhuyin);
            store.observe_selection(1, token).unwrap();
            let live = user.join("user_phrase_index.bin");
            let mut before = None;
            if destination == "live-file" {
                store.save_reporting().unwrap();
                before = Some(std::fs::read(&live).unwrap());
                store.observe_selection(1, token).unwrap();
            } else if destination == "empty-directory" {
                std::fs::create_dir(&live).unwrap();
            }
            let tmp = user.join("user_phrase_index.bin.tmp");
            std::fs::create_dir(&tmp).unwrap();
            std::fs::write(tmp.join("stale-marker"), marker).unwrap();
            store.save_reporting().unwrap();
            assert_eq!(tmp.exists(), !moved, "{destination}");
            assert_eq!(
                std::fs::read(live.join("stale-marker")).ok().as_deref(),
                moved.then_some(marker),
                "{destination}"
            );
            if let Some(before) = before {
                assert_eq!(std::fs::read(&live).unwrap(), before);
            }
        }
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let output = crate::user::test_child::run_exact(NAME, [(ENV, root.path().as_os_str())]);
    assert!(output.status.success(), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        stderr.matches("user_phrase_index.bin failed.").count(),
        if cfg!(windows) { 2 } else { 1 },
        "{stderr}"
    );
}

/// Basis: policy row 107 (a journal ADD keeps a surrogate word as the pin-derived fixture holds it).
#[test]
fn a_journal_word_no_import_writes_is_kept() {
    use crate::user::native_fixture::TempUserDir;
    let scratch = tempfile::tempdir().unwrap();
    let system = crate::user::native_fixture::items_system(
        &crate::user::native_fixture::system_dir(),
        scratch.path(),
        &[
            (&[0x7532, 0x4e01, 0x4e59], &[0x1084; 3], 100),
            (&[0x4e19], &[0x1081], 100),
        ],
        200,
    );
    let user = TempUserDir::new("stored-words-surrogate");
    let mut store =
        crate::user::test_session::open_native(&user.path, &system, UserConfLaw::Pinyin);
    let token = store
        .add_system_phrase_in(
            1,
            None,
            &"甲戊乙".into(),
            &crate::user::test_session::native_keys("da1'da1'da1"),
            Some(1000),
        )
        .unwrap();
    store.compact_libraries();
    store.mark_modified();
    store.save_reporting().unwrap();
    drop(store);
    let path = user.path.join("gb_char.dbin");
    let original: Vec<u8> = [0x7532_u32, 0x620a, 0x4e59]
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect();
    let replacement: Vec<u8> = [0x7532_u32, 0xd800, 0x4e59]
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect();
    let mut payload = std::fs::read(&path).expect("gb_char.dbin")[8..].to_vec();
    let at = payload
        .windows(original.len())
        .position(|window| window == original.as_slice())
        .expect("the imported item");
    payload.splice(at..at + original.len(), replacement);
    std::fs::write(
        &path,
        crate::data::chunk_format::build_memory_chunk(&payload).unwrap(),
    )
    .unwrap();

    let mut store =
        crate::user::test_session::open_native(&user.path, &system, UserConfLaw::Pinyin);
    let crate::user::system_items::LiveItemFields::Read(words) =
        store.with_live_item_fields(token, |item| {
            crate::data::phrase_library::decode_phrase_item(item)
                .unwrap()
                .phrase
        })
    else {
        panic!("expected a live item");
    };
    assert_eq!(words, [0x7532, 0xd800, 0x4e59]);
    assert_eq!(store.system_unigram(token), Some(3000));
    store.compact_libraries();
    store.mark_modified();
    store.save_reporting().unwrap();
    let native: String = std::fs::read(&path)
        .expect("gb_char.dbin")
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(
        native,
        "380000006f1285e404000000000000000400c8000000800c000001000000030000001c000301b80b0000\
         3275000000d80000594e0000841084108410e8030000"
    );
}

#[cfg(feature = "tkrzw")]
mod pinned_profiles;
