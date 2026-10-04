//! The empty user path is the working directory, as the pin's `""` is.
//!
//! `pinyin_init` keeps `g_strdup(userdir)` and builds every user file's
//! path with `g_build_filename`, which drops an empty element
//! (`pinyin.cpp:332`, `:176-177`, `:220-232` at 074a2219): the profile of
//! a `""` user dir is bare file names, resolved against the directory
//! that is current when each file is opened, not the one current at init
//! (#619). The differential that holds this against the pin is
//! `tools/bisection/run-locale-diff.sh`; this executable holds the
//! same law where no oracle is built.
//!
//! The working directory is process state, so everything here is one
//! test in an executable of its own: nothing else can run beside it and
//! see the directory move.

use std::path::{Path, PathBuf};

use oxpinyin_core::SyllableKey;
use oxpinyin_runtime::Runtime;
use oxpinyin_user::PinyinKey;

fn w3_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("fixtures")
        .join("w3")
        .join(oxpinyin_data::DEFAULT_STORE_EXT)
}

/// A fresh directory for this process.
fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "oxpinyin-runtime-empty-user-dir-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("readable dir")
        .map(|entry| {
            entry
                .expect("dir entry")
                .file_name()
                .into_string()
                .expect("UTF-8 file name")
        })
        .collect();
    names.sort();
    names
}

/// `user.conf`'s `open counter:` value in `dir`.
fn open_counter(dir: &Path) -> i32 {
    let text = std::fs::read_to_string(dir.join("user.conf")).expect("user.conf");
    text.lines()
        .find_map(|line| line.strip_prefix("open counter:"))
        .expect("an open counter line")
        .parse()
        .expect("a decimal counter")
}

#[test]
fn empty_user_path_is_the_working_directory_at_each_use() {
    let first = scratch("first");
    let second = scratch("second");
    std::env::set_current_dir(&first).expect("enter the first directory");

    // Init: the store opens on the working directory, and libpinyin's
    // check_format writes the raised marker there.
    let runtime = Runtime::open(&w3_dir(), Some(Path::new(""))).expect("open");
    let mut store = runtime
        .user_store()
        .expect("an empty user path is a user dir, not the absence of one");
    assert_eq!(names(&first), ["user.conf"]);
    assert_eq!(open_counter(&first), 1);

    // The consumer moves. A dirty save lands where it is now, marker
    // included, and the directory init wrote into is not touched again.
    std::env::set_current_dir(&second).expect("enter the second directory");
    let keys: Vec<PinyinKey> = ["ni", "hao"]
        .iter()
        .map(|s| SyllableKey::from_text(s).expect("fixture key"))
        .map(|key| PinyinKey::try_from(key.index()).expect("frozen syllable inventory fits u16"))
        .collect();
    let phrase = "你鎄";
    store.add_phrase(phrase, &keys, None).expect("learn it");
    store.mark_modified();
    assert!(store.save().expect("save"), "a dirty save answers true");
    let saved = names(&second);
    for name in ["user.conf", "user.bin", "user_bigram.db"] {
        assert!(
            saved.iter().any(|n| n == name),
            "{name} missing from {saved:?}"
        );
    }
    assert_eq!(open_counter(&second), 1);
    assert_eq!(names(&first), ["user.conf"]);

    // Fini is the runtime's own handle dropping: it lowers the counter in
    // the directory that is current now. The first directory keeps the
    // raised one, as the pin's does.
    drop(store);
    drop(runtime);
    assert_eq!(open_counter(&second), 0);
    assert_eq!(open_counter(&first), 1);

    // The profile reads back from the working directory, and a stale DBM
    // sidecar beside one of its files is removed on the way, as it is in
    // a named directory.
    for sidecar in ["user_bigram.db-lock", "user_bigram.db-shm"] {
        std::fs::write(sidecar, b"stale").expect("plant a sidecar");
    }
    let runtime = Runtime::open(&w3_dir(), Some(Path::new(""))).expect("reopen");
    let store = runtime.user_store().expect("the profile reopens");
    assert!(
        store
            .token_for_phrase(phrase)
            .expect("phrase lookup")
            .is_some(),
        "the saved phrase is read back"
    );
    assert_eq!(names(&second), saved, "exactly the saved names remain");
    drop(store);
    drop(runtime);

    std::env::set_current_dir(std::env::temp_dir()).expect("leave the scratch dirs");
    let _ = std::fs::remove_dir_all(&first);
    let _ = std::fs::remove_dir_all(&second);
}
