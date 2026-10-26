//! Later native save boundaries, derived from libpinyin 074a2219:
//! src/pinyin.cpp:922-1146,1194-1200; src/zhuyin.cpp:547-698,741-757.
//! GLib 2.89.4 glib/gstdio.h:60-82 maps Unix g_rename/g_fsync directly
//! to rename/fsync. These tests cover process interruption, not power loss
//! or C-facade execution. The missing chunk sync remains #1221.
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

use oxpinyin_core::SyllableKey;
use oxpinyin_data::chunk_write::ChunkItem;
use oxpinyin_data::table_info::TableConf;
use oxpinyin_data::user_files::{SystemVersions, UserFileLayout, UserTableInfo};
use oxpinyin_user::persistence::{SystemLibrary, SystemOriginals, UserConfLaw};
use oxpinyin_user::{PinyinKey, UserStore};

const CHILD: &str = "OXPINYIN_929_LATE_DIR";
const CHUNKS: [&str; 2] = ["addon.bin", "network.bin"];
const INDEXES: [&str; 3] = [
    "user_pinyin_index.bin",
    "user_phrase_index.bin",
    "user_bigram.db",
];

fn originals() -> SystemOriginals {
    // A valid late system row supplies the existing remap callback after
    // the two user libraries. The pin walks table indices in ascending
    // order (src/pinyin.cpp:927-938; src/zhuyin.cpp:560-571).
    // Row names and independent file types: src/storage/table_info.cpp:145-156,266-286.
    let conf = format!(
        "binary format version:7\nmodel data version:14\nlambda parameter:0.312699\n\
         source table format:pinyin\ndatabase format:{}\n\
         default ADDON_DICTIONARY NULL NULL addon.bin USER_FILE\n\
         default NETWORK_DICTIONARY NULL NULL network.bin USER_FILE\n\
         default USER_DICTIONARY late.table late.bin late.dbin SYSTEM_FILE\n",
        SystemVersions::for_this_build(7, 14).database_format
    );
    let layout = UserFileLayout::from_conf(&TableConf::parse(conf.as_bytes()).unwrap());
    let library = SystemLibrary {
        total: 100,
        range_end: 2,
        mapped: None,
        items: BTreeMap::from([(
            1,
            ChunkItem {
                phrase: vec![0x4f60],
                unigram: 100,
                prons: Vec::new(),
            },
        )]),
    };
    SystemOriginals::new(BTreeMap::from([(7, library)]), layout)
}

fn open(dir: &Path, law: UserConfLaw) -> UserStore {
    UserStore::open_libpinyin(dir, originals(), SystemVersions::for_this_build(7, 14), law).unwrap()
}

fn change(store: &mut UserStore) {
    let key = PinyinKey::try_from(SyllableKey::from_text("ni").unwrap().index()).unwrap();
    for library in 5..=6_u8 {
        store
            .add_phrase_in(library, &"你".into(), &[key], Some(17))
            .unwrap();
    }
    store
        .set_bigram_count(0x0700_0001, 0x0600_0001, 37)
        .unwrap();
    store.mark_modified();
}

fn seed(dir: &Path, law: UserConfLaw) {
    let mut store = open(dir, law);
    store.mark_modified();
    assert_eq!(store.save_reporting().unwrap(), Default::default());
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
    UserTableInfo::parse(&std::fs::read(dir.join("user.conf")).unwrap())
        .unwrap()
        .open_counter
}

fn run_child(dir: &Path, law: UserConfLaw, test: &str, boundary: &str) {
    let status = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test])
        .env(CHILD, dir)
        .env(
            "OXPINYIN_929_LATE_LAW",
            if law == UserConfLaw::Pinyin {
                "pinyin"
            } else {
                "zhuyin"
            },
        )
        .env("OXPINYIN_929_LATE_BOUNDARY", boundary)
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(73), "child must exit without unwinding");
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

#[test]
fn interruption_after_user_chunks_keeps_indexes_and_bigram_at_previous_save() {
    if let Some(mut store) = child_store() {
        store
            .save_reporting_with_baseline(&[], |index| {
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
            "interruption_after_user_chunks_keeps_indexes_and_bigram_at_previous_save",
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
                    .phrase((u32::from(library) << 24) | 1)
                    .unwrap()
                    .is_some(),
                law == UserConfLaw::Zhuyin
            );
        }
    }
}

#[test]
fn interruption_after_save_or_fini_retains_every_saved_data_byte() {
    if let Some(mut store) = child_store() {
        assert_eq!(store.save_reporting().unwrap(), Default::default());
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
                "interruption_after_save_or_fini_retains_every_saved_data_byte",
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
            for library in 5..=6_u32 {
                assert_eq!(
                    reopened
                        .phrase((library << 24) | 1)
                        .unwrap()
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
