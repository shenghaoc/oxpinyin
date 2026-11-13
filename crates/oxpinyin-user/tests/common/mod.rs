//! Opens a session the way an init does: `check_format`, then the library
//! loop over a system directory's `table.conf` rows.

use std::path::Path;

use oxpinyin_data::chunk_write::{ChunkItem, build_chunk};
use oxpinyin_data::table_info::TableConf;
use oxpinyin_data::user_files::{SystemVersions, UserFileLayout};
use oxpinyin_user::persistence::{SystemOriginals, UserConfLaw};
use oxpinyin_user::{LibraryLoopRows, UserStore};

thread_local! {
    /// The system directories of this thread's sessions. A save maps each
    /// system chunk again by path, so a directory outlives its store.
    static SYSTEM_DIRS: std::cell::RefCell<Vec<tempfile::TempDir>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// The versions every test profile conforms to.
pub fn versions() -> SystemVersions {
    SystemVersions::for_this_build(7, 14)
}

/// A system directory that holds `chunks` by file name, each built from its
/// items, kept until the thread ends.
pub fn system_dir(chunks: &[(&str, Vec<(u32, ChunkItem)>)]) -> std::path::PathBuf {
    let dir = tempfile::tempdir().expect("system directory");
    for (name, items) in chunks {
        std::fs::write(dir.path().join(name), build_chunk(items).expect("chunk")).expect("write");
    }
    let path = dir.path().to_path_buf();
    SYSTEM_DIRS.with_borrow_mut(|dirs| dirs.push(dir));
    path
}

/// `pinyin_init`'s or `zhuyin_init`'s user-data half over `conf`'s default
/// rows: the profile is judged, then each row is mapped, merged or read.
pub fn open(user_dir: &Path, system_dir: &Path, conf: &TableConf, law: UserConfLaw) -> UserStore {
    let layout = UserFileLayout::from_conf(conf);
    let check = UserStore::check_profile(user_dir, &versions(), &layout, law).expect("profile");
    let mut originals = SystemOriginals::new(Default::default(), layout);
    let rows = LibraryLoopRows::new(system_dir, conf, Some(user_dir));
    UserStore::open_libpinyin_checked(user_dir, &mut originals, &rows, versions(), law, check)
        .expect("open profile")
}
