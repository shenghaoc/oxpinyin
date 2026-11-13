//! Source-derived save laws at 074a2219: pinyin.cpp:922-1146,
//! zhuyin.cpp:547-698 and include/memory_chunk.h:536-557.
mod common;

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use oxpinyin_data::chunk_write::ChunkItem;
use oxpinyin_data::table_info::TableConf;
use oxpinyin_data::user_files::UserFileLayout;
use oxpinyin_user::UserStore;
use oxpinyin_user::persistence::{SaveBaseline, UserConfLaw};

/// A system directory with the stock layout's four system libraries, one
/// item each.
fn system() -> (std::path::PathBuf, TableConf) {
    let conf = TableConf::stock();
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
    let chunks: Vec<(&str, Vec<_>)> = names.iter().map(|name| (name.as_str(), item())).collect();
    (common::system_dir(&chunks), conf)
}

fn open(dir: &Path, law: UserConfLaw) -> UserStore {
    let (system, conf) = system();
    common::open(dir, &system, &conf, law)
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
        .map(|(_, name)| oxpinyin_input::path::build_path(dir, name.as_ref()))
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

    for law in [UserConfLaw::Pinyin, UserConfLaw::Zhuyin] {
        for missing in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let mut child = Command::new(std::env::current_exe().unwrap());
            child
                .args([
                    "--exact",
                    "failed_renames_report_exact_order_and_preserve_destinations",
                ])
                .env(CHILD, dir.path());
            if missing {
                child.env(MISSING, "1");
            }
            if law == UserConfLaw::Zhuyin {
                child.env(ZHUYIN, "1");
            }
            let output = child.output().unwrap();
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
                    // Some earlier open diagnostics have no newline.
                    line.find("rename ")
                        .or_else(|| line.find(&marker_failure))
                        .map(|start| &line[start..])
                })
                .collect();
            assert_eq!(actual, expected, "{law:?}, missing={missing}");
        }
    }
}

// Both facades may rewrite user.conf during lifecycle operations; the data
// files must stay byte-for-byte intact when a conforming profile is reopened.
fn data_bytes(dir: &Path) -> BTreeMap<std::ffi::OsString, Vec<u8>> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap())
        .filter(|entry| entry.file_name() != "user.conf")
        .map(|entry| (entry.file_name(), std::fs::read(entry.path()).unwrap()))
        .collect()
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
        let saved_bytes = data_bytes(dir.path());
        store
            .set_bigram_count(0x0100_0001, 0x0200_0001, 91)
            .unwrap();
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

        let reading = [oxpinyin_core::SyllableKey::from_text("ni").unwrap().index() as u16];
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
        let phrases = store.export_phrases_in(7).unwrap();
        let system_unigram = store.system_unigram(SYSTEM).expect("live system item");
        let unigram = store.user_unigram_frequency(USER).unwrap();
        let gram = store.bigram_row(SYSTEM).unwrap();
        drop(store);
        drop(uninterrupted);

        // What a new session reads from each profile.
        let load = |path| {
            let session = open(path, law);
            let grams: Vec<_> = session
                .bigram_predecessors()
                .unwrap()
                .into_iter()
                .map(|prev| (prev, session.bigram_row(prev).unwrap()))
                .collect();
            (
                session.phrases().unwrap(),
                [5, 6, 7].map(|library| session.export_phrases_in(library).unwrap()),
                grams,
                session.unigram_total().unwrap(),
                session
                    .system_unigram(SYSTEM)
                    .expect("reopened system item"),
                session.user_unigram_frequency(USER).unwrap(),
            )
        };
        assert_eq!(load(dir.path()), load(control.path()), "{law:?}");
        assert!(
            save_destinations(dir.path())
                .iter()
                .all(|path| { path.is_file() && !staged_path(path).exists() })
        );
        let reopened = open(dir.path(), law);
        assert_eq!(reopened.export_phrases_in(7).unwrap(), phrases);
        assert_eq!(reopened.system_unigram(SYSTEM), Some(system_unigram));
        assert_eq!(reopened.user_unigram_frequency(USER).unwrap(), unigram);
        assert_eq!(reopened.bigram_row(SYSTEM).unwrap(), gram);
        assert_eq!(reopened.token_for_phrase("你").unwrap(), Some(USER));
    }
}

// Linux only: /dev/null accepts writes and rejects fsync.
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
        store.save_reporting().unwrap();
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
        let stop_before: u8 = std::env::var("OXPINYIN_929_STOP_BEFORE")
            .unwrap()
            .parse()
            .unwrap();
        let mut store = open(Path::new(&dir), law);
        for index in 1..=4_u32 {
            store
                .add_system_unigram_frequency((index << 24) | 1, 17)
                .unwrap();
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
    // Default file order is data/table.conf.in:9-12 at 074a2219.
    const LOGS: [&str; 4] = [
        "gb_char.dbin",
        "gbk_char.dbin",
        "opengram.dbin",
        "merged.dbin",
    ];
    for law in [UserConfLaw::Pinyin, UserConfLaw::Zhuyin] {
        for stop_before in 2..=4_usize {
            let dir = tempfile::tempdir().unwrap();
            let seeded = seed(dir.path(), law);
            let shipped: Vec<u32> = (1..=4_u32)
                .map(|index| seeded.system_unigram((index << 24) | 1).expect("item"))
                .collect();
            drop(seeded);
            let old = data_bytes(dir.path());
            let mut child = Command::new(std::env::current_exe().unwrap());
            child
                .args([
                    "--exact",
                    "interrupted_save_preserves_the_facade_specific_committed_prefix",
                ])
                .env(CHILD, dir.path())
                .env("OXPINYIN_929_STOP_BEFORE", stop_before.to_string());
            if law == UserConfLaw::Zhuyin {
                child.env("OXPINYIN_929_ZHUYIN", "1");
            }
            assert_eq!(child.status().unwrap().code(), Some(73));
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
            for index in 1..=4_u32 {
                assert_eq!(
                    reopened.system_unigram((index << 24) | 1),
                    Some(
                        shipped[index as usize - 1]
                            + if law == UserConfLaw::Zhuyin && index < stop_before as u32 {
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
