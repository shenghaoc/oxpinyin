//! A failed bigram attachment after the owner's `reset()`, on the two
//! backends that keep the failed object (Berkeley DB closes it at once; Kyoto
//! Cabinet keeps it). Expectations are read from the pin's source at
//! 074a2219; nothing here was produced by running the pin.
#![cfg(any(feature = "bdb", feature = "kyotocabinet"))]

use std::path::PathBuf;

use oxpinyin_store::SystemAttachment;

/// A scratch directory removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "oxpinyin-store-finished-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_finished_failed_bigram_attachment_refuses_store_and_walk() {
    // After the owner's `reset()` the pin's `m_db` is NULL, so `Bigram::store`
    // answers false (ngram_bdb.cpp:148-150 at 074a2219) and
    // `get_all_items` answers false (:181-182) — the same on Kyoto Cabinet
    // (ngram_kyotodb.cpp:145-147, :186-190). Before `reset()` the failed
    // object refuses the write on every backend (BDB `EINVAL`, ngram_bdb.cpp:
    // 161; KC `set` returns false, ngram_kyotodb.cpp:152); the walk answers
    // false on BDB (:190, no cursor) and true on KC (:193-195).
    let dir = Scratch::new("finished");
    let missing = dir.0.join("no-such-dir").join("bigram.db");
    let SystemAttachment::Unopened(db) =
        SystemAttachment::attach_bigram(&missing, false).expect("attach")
    else {
        panic!("a path in a missing directory opened");
    };
    assert!(db.write(&[1, 0, 0, 0], b"gram").is_err());
    let walk = db.walk();
    assert_eq!(walk.is_ok(), !cfg!(feature = "bdb"), "{walk:?}");
    assert!(!db.is_finished());

    db.finish();
    assert!(db.is_finished());
    let write = db
        .write(&[1, 0, 0, 0], b"gram")
        .expect_err("finished write");
    assert!(write.to_string().contains("finished"), "{write}");
    let walk = db.walk().expect_err("finished walk");
    assert!(walk.to_string().contains("finished"), "{walk}");
    // A second finish and the inert reads change nothing.
    db.finish();
    db.read(&[1, 0, 0, 0]);
    db.suggest(&[1, 0, 0, 0]);
    assert!(db.is_finished());
}
