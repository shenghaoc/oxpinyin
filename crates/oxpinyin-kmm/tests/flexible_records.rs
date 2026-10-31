//! The KMM record types on the flexible-bigram container: every compiled
//! backend round-trips the pin's raw layout, the header/item operations keep
//! their pin semantics, and the native walk is never sorted.

use std::path::Path;

use oxpinyin_kmm::{
    Database, FlexibleGramSpec, KMixtureModelArrayHeader, KMixtureModelArrayItem,
    KMixtureModelMagicHeader, KmmSpec,
};
use oxpinyin_store::FixedRecord;

fn c_path(path: &Path) -> std::ffi::CString {
    oxpinyin_input::c_path(path).expect("path has no NUL")
}

fn open(path: &Path) -> Database {
    Database::open_read_write(Some(&c_path(path)))
}

fn item(wc: u32, n_n_0: u32, n_1: u32, mr: u32) -> KMixtureModelArrayItem {
    KMixtureModelArrayItem {
        m_wc: wc,
        m_n_n_0: n_n_0,
        m_n_1: n_1,
        m_mr: mr,
    }
}

#[test]
fn a_fresh_container_carries_the_magic_number_and_no_magic_header() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fresh.db");
    let db = open(&path);
    assert!(db.is_open());
    assert!(
        db.get_magic_header().is_none(),
        "a fresh container is 4 bytes"
    );
    assert!(db.get_all_items().is_empty());
    drop(db);

    let read = Database::open_read_only(Some(&c_path(&path)));
    assert!(read.is_open());
    assert!(read.get_magic_header().is_none());
}

#[test]
fn magic_array_and_item_records_round_trip_in_the_pins_layout() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("records.db");
    let magic = KMixtureModelMagicHeader {
        m_wc: 0x0102_0304,
        m_n: 5,
        m_total_freq: 6,
    };
    let header = KMixtureModelArrayHeader { m_wc: 7, m_freq: 8 };
    {
        let db = open(&path);
        db.set_magic_header_checked(&magic);
        let mut gram = oxpinyin_kmm::KmmGram::new();
        gram.set_header(header);
        assert!(gram.insert_array_item(0x0100_0001, item(9, 10, 11, 12)));
        assert!(gram.insert_array_item(2, item(1, 2, 3, 4)));
        db.store_checked(0x0100_0001, &gram);
        assert_eq!(db.get_magic_header(), Some(magic));
        assert_eq!(db.array_header_checked(0x0100_0001), header);
        let loaded = db.load_checked(0x0100_0001);
        assert_eq!(loaded.header(), &header);
        // `retrieve_all` is token-ascending.
        let tokens: Vec<u32> = loaded.items().iter().map(|entry| entry.token).collect();
        assert_eq!(tokens, [2, 0x0100_0001]);
        assert_eq!(loaded.get_array_item(2), Some(item(1, 2, 3, 4)));
    }
    let read = Database::open_read_only(Some(&c_path(&path)));
    assert_eq!(read.get_magic_header(), Some(magic));
    assert_eq!(read.array_header_checked(0x0100_0001), header);
    assert_eq!(read.load_checked(0x0100_0001).items().len(), 2);
}

#[test]
fn set_array_header_leaves_the_items_alone_and_creates_a_header_only_row() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("headers.db");
    let db = open(&path);
    let mut gram = oxpinyin_kmm::KmmGram::new();
    assert!(gram.insert_array_item(9, item(1, 1, 1, 1)));
    db.store_checked(4, &gram);

    db.set_array_header_checked(4, &KMixtureModelArrayHeader { m_wc: 1, m_freq: 2 });
    let loaded = db.load_checked(4);
    assert_eq!(loaded.header().m_freq, 2);
    assert_eq!(loaded.items().len(), 1);

    // A missing token becomes a header-only record, as the pin's 074a2219
    // Tkrzw `set_array_header` does.
    db.set_array_header_checked(5, &KMixtureModelArrayHeader { m_wc: 0, m_freq: 3 });
    assert_eq!(
        db.array_header_checked(5),
        KMixtureModelArrayHeader { m_wc: 0, m_freq: 3 }
    );
    assert!(db.load_checked(5).items().is_empty());
}

#[test]
fn remove_answers_the_pins_semantics() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("remove.db");
    let db = open(&path);
    let gram = oxpinyin_kmm::KmmGram::new();
    db.store_checked(11, &gram);
    db.remove_checked(11);
    assert!(db.load(11).is_none());
    // A second remove has nothing to answer; the raw layer reports false and
    // the pin's checker would abort, so use the unchecked form here.
    db.remove(11);
    assert!(db.load(11).is_none());
}

#[test]
fn the_native_walk_is_the_backends_order_not_a_sort() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("walk.db");
    let db = open(&path);
    for token in [0x0100_0003, 2, 0x0200_0001, 9] {
        db.store_checked(token, &oxpinyin_kmm::KmmGram::new());
    }
    let mut walked = db.get_all_items();
    assert_eq!(walked.len(), 4);
    walked.sort_unstable();
    assert_eq!(walked, [2, 9, 0x0100_0003, 0x0200_0001]);
}

#[test]
fn the_record_lengths_are_the_pins() {
    assert_eq!(KMixtureModelMagicHeader::LEN, 12);
    assert_eq!(KMixtureModelArrayHeader::LEN, 8);
    assert_eq!(KMixtureModelArrayItem::LEN, 16);
    assert_eq!(KmmSpec::MAGIC_NUMBER, *b"KMMP");
}

#[test]
fn removing_every_row_leaves_a_readable_container() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("remove_all.db");
    {
        let db = open(&path);
        db.set_magic_header_checked(&KMixtureModelMagicHeader::default());
        for token in [1_u32, 2, 3, 16802309] {
            db.store_checked(token, &oxpinyin_kmm::KmmGram::new());
        }
        for token in [1_u32, 2, 3, 16802309] {
            db.remove_checked(token);
        }
    }
    let read = Database::open_read_only(Some(&c_path(&path)));
    assert!(read.is_open(), "the container reopens");
    assert!(read.get_all_items().is_empty());
    assert!(read.load(1).is_none());
}
