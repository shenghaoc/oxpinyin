//! Source-derived save laws at 074a2219: pinyin.cpp:922-1146,
//! zhuyin.cpp:547-698 and include/memory_chunk.h:536-557.
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use oxpinyin_data::chunk_write::ChunkItem;
use oxpinyin_data::user_files::SystemVersions;
use oxpinyin_user::UserStore;
use oxpinyin_user::persistence::{SaveBaseline, SystemLibrary, SystemOriginals, UserConfLaw};

fn originals() -> SystemOriginals {
    [1, 2]
        .into_iter()
        .map(|index| {
            (
                index,
                SystemLibrary {
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
                },
            )
        })
        .collect::<BTreeMap<_, _>>()
        .into()
}

fn open(dir: &Path, law: UserConfLaw) -> UserStore {
    UserStore::open_libpinyin(dir, originals(), SystemVersions::for_this_build(7, 14), law)
        .expect("open profile")
}

fn seed(dir: &Path, law: UserConfLaw) -> UserStore {
    let mut store = open(dir, law);
    store.mark_modified();
    assert_eq!(
        store.save_reporting().expect("seed save"),
        Default::default()
    );
    store
}

#[test]
fn failed_renames_report_exact_order_and_preserve_destinations() {
    for law in [UserConfLaw::Pinyin, UserConfLaw::Zhuyin] {
        let dir = tempfile::tempdir().unwrap();
        let mut store = seed(dir.path(), law);
        let layout = originals();
        let mut libraries: Vec<_> = layout
            .layout()
            .system_logs()
            .iter()
            .chain(layout.layout().user_libraries())
            .collect();
        libraries.sort_by_key(|(index, _)| *index);
        let mut names: Vec<_> = libraries
            .into_iter()
            .map(|(_, name)| name.under(dir.path()))
            .collect();
        names.extend(
            [
                "user_pinyin_index.bin",
                "user_phrase_index.bin",
                "user_bigram.db",
            ]
            .map(|name| dir.path().join(name)),
        );
        for path in &names {
            std::fs::remove_file(path).unwrap();
            std::fs::create_dir(path).unwrap();
            std::fs::write(path.join("keep"), b"previous data").unwrap();
        }
        store.mark_modified();
        let report = store.save_reporting().expect("rename failures are soft");
        assert_eq!(
            report
                .renames_failed
                .iter()
                .map(|(_, path)| path)
                .collect::<Vec<_>>(),
            names.iter().collect::<Vec<_>>()
        );
        assert!(
            !store.is_modified(),
            "the pin clears dirty even after failed renames"
        );
        assert!(report.user_conf_write_failed.is_none());
        for (tmp, final_path) in report.renames_failed {
            assert!(tmp.is_file(), "failed rename keeps the staged file");
            assert_eq!(
                std::fs::read(final_path.join("keep")).unwrap(),
                b"previous data"
            );
        }
        assert_eq!(
            store.save_reporting().unwrap(),
            Default::default(),
            "clean save does not retry"
        );
    }
}

#[test]
fn save_reload_keeps_bigrams_and_unsaved_changes_do_not_replace_them() {
    for law in [UserConfLaw::Pinyin, UserConfLaw::Zhuyin] {
        let dir = tempfile::tempdir().unwrap();
        let mut store = seed(dir.path(), law);
        store
            .set_bigram_count(0x0100_0001, 0x0200_0001, 37)
            .unwrap();
        store.mark_modified();
        store.save_reporting().unwrap();
        store
            .set_bigram_count(0x0100_0001, 0x0200_0001, 91)
            .unwrap();
        drop(store); // fini is not an implicit save (both facades).
        let mut reopened = open(dir.path(), law);
        assert_eq!(reopened.bigram_count(0x0100_0001, 0x0200_0001).unwrap(), 37);
        reopened.mark_modified();
        reopened.save_reporting().unwrap();
        drop(reopened);
        assert_eq!(
            open(dir.path(), law)
                .bigram_count(0x0100_0001, 0x0200_0001)
                .unwrap(),
            37
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn sync_rejecting_chunk_target_does_not_stop_later_files() {
    // /dev/null accepts writes but rejects fsync. The pin ignores that result
    // at memory_chunk.h:555. This verifies the outward error path only: it
    // does NOT prove that the subject actually issues its missing sync.
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
        assert_eq!(store.save_reporting().unwrap(), Default::default());
        assert_eq!(
            std::fs::read_link(dir.path().join("gb_char.dbin")).unwrap(),
            Path::new("/dev/null")
        );
        assert!(dir.path().join("user_bigram.db").is_file());
        assert!(!store.is_modified());
    }
}

#[test]
fn interrupted_save_preserves_the_facade_specific_committed_prefix() {
    const CHILD: &str = "OXPINYIN_929_INTERRUPT_DIR";
    if let Some(dir) = std::env::var_os(CHILD) {
        let law = if std::env::var_os("OXPINYIN_929_ZHUYIN").is_some() {
            UserConfLaw::Zhuyin
        } else {
            UserConfLaw::Pinyin
        };
        let mut store = open(Path::new(&dir), law);
        store.add_system_unigram_frequency(0x0100_0001, 17).unwrap();
        store.mark_modified();
        store
            .save_reporting_with_baseline(&[], |index| {
                if index == 2 {
                    std::process::exit(73);
                }
                SaveBaseline::Unchanged
            })
            .unwrap();
        panic!("the second library boundary must be reached");
    }
    for law in [UserConfLaw::Pinyin, UserConfLaw::Zhuyin] {
        let dir = tempfile::tempdir().unwrap();
        drop(seed(dir.path(), law));
        let first = dir.path().join("gb_char.dbin");
        let second = dir.path().join("gbk_char.dbin");
        let old_first = std::fs::read(&first).unwrap();
        let old_second = std::fs::read(&second).unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap());
        child
            .args([
                "--exact",
                "interrupted_save_preserves_the_facade_specific_committed_prefix",
            ])
            .env(CHILD, dir.path());
        if law == UserConfLaw::Zhuyin {
            child.env("OXPINYIN_929_ZHUYIN", "1");
        }
        assert_eq!(child.status().unwrap().code(), Some(73));
        assert_eq!(std::fs::read(second).unwrap(), old_second);
        let staged = dir.path().join("gb_char.dbin.tmp");
        if law == UserConfLaw::Pinyin {
            assert_eq!(std::fs::read(first).unwrap(), old_first);
            assert_ne!(std::fs::read(staged).unwrap(), old_first);
        } else {
            assert_ne!(std::fs::read(first).unwrap(), old_first);
            assert!(!staged.exists());
        }
        let reopened = open(dir.path(), law);
        assert_eq!(
            reopened.unigram_delta(0x0100_0001).unwrap(),
            if law == UserConfLaw::Pinyin { 0 } else { 17 }
        );
    }
}
