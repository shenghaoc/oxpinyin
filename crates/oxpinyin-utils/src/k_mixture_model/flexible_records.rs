//! The KMM database on the flexible-bigram container: a container whose
//! every row is removed reopens and reads as empty on every compiled
//! backend.

use std::path::Path;

use super::database::Database;
use super::records::{KMixtureModelMagicHeader, KmmGram};

fn c_path(path: &Path) -> std::ffi::CString {
    oxpinyin_input::c_path(path).expect("path has no NUL")
}

fn open(path: &Path) -> Database {
    Database::open_read_write(Some(&c_path(path)))
}

#[test]
fn removing_every_row_leaves_a_readable_container() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("remove_all.db");
    {
        let db = open(&path);
        db.set_magic_header_checked(&KMixtureModelMagicHeader::default());
        for token in [1_u32, 2, 3, 16802309] {
            db.store_checked(token, &KmmGram::new());
        }
        for token in [1_u32, 2, 3, 16802309] {
            db.remove_checked(token);
        }
    }
    let read = Database::open_read_only(Some(&c_path(&path)));
    assert!(read.attached(), "the container reopens");
    assert_eq!(
        read.get_magic_header(),
        Some(KMixtureModelMagicHeader::default())
    );
    assert!(read.get_all_items().is_empty());
    assert!(read.load(1).is_none());
}
