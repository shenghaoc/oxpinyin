use std::path::Path;

use super::SystemAttachment;

#[test]
fn attach_bigram_answers_a_failed_open_apart_from_an_earlier_failure() {
    // `Bigram::attach`: a failed native open is an unopened attachment,
    // whose object Kyoto Cabinet and tkrzw keep; a failure before the open
    // is an error on every backend.
    let attach = |path: &Path, read_only| SystemAttachment::attach_bigram(path, read_only);
    let scratch = tempfile::tempdir().expect("scratch directory");
    let missing = scratch.path().join("missing.db");
    assert!(matches!(
        attach(&missing, true).expect("attach"),
        SystemAttachment::Unopened(_)
    ));
    assert!(!missing.exists());
    let created = scratch.path().join("bigram.db");
    for read_only in [false, true] {
        assert!(
            matches!(
                attach(&created, read_only).expect("attach"),
                SystemAttachment::Open(_)
            ),
            "read only: {read_only}"
        );
    }
    let nul = Path::new(std::ffi::OsStr::new("bad\0name.db"));
    assert!(attach(nul, true).is_err(), "a rejected path");
}

#[cfg(any(feature = "bdb", feature = "kyotocabinet"))]
mod finished_failed_attachment {
    //! A failed bigram attachment after the owner's `reset()`, on the two
    //! backends that keep the failed object (Berkeley DB closes it at once; Kyoto
    //! Cabinet keeps it). Expectations are read from the pin's source at
    //! 074a2219; nothing here was produced by running the pin.

    use crate::store::SystemAttachment;

    #[test]
    fn a_finished_failed_bigram_attachment_refuses_store_and_walk() {
        // After the owner's `reset()` the pin's `m_db` is NULL, so `Bigram::store`
        // answers false (ngram_bdb.cpp:148-150 at 074a2219) and
        // `get_all_items` answers false (:181-182) — the same on Kyoto Cabinet
        // (ngram_kyotodb.cpp:145-147, :186-190). Before `reset()` the failed
        // object refuses the write on every backend (BDB `EINVAL`, ngram_bdb.cpp:
        // 161; KC `set` returns false, ngram_kyotodb.cpp:152); the walk answers
        // false on BDB (:190, no cursor) and true on KC (:193-195).
        let dir = tempfile::tempdir().expect("scratch directory");
        let missing = dir.path().join("no-such-dir").join("bigram.db");
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
}
