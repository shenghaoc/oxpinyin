//! Backend-selection contract for `oxpinyin-datagen`.
//!
//! Three invariants:
//!   1. `Backend::DEFAULT` — the constant `Options::default()` pulls from —
//!      is the compiled-in peer, matching `oxpinyin_store::DefaultStore`
//!      under the workspace's default feature set (tkrzw there).
//!   2. The file names the producer writes are the names the runtime
//!      reader (`oxpinyin_data::SystemDbm`) opens for the same backend:
//!      libpinyin's own on the drop-in backends, `<stem>.<ext>` elsewhere.
//!   3. Each of the four peer backends knows its own extension and can be
//!      selected explicitly on the command line.

use oxpinyin_datagen::write::{Backend, DbmFile};

/// The datagen default is the workspace's default store.
#[test]
fn default_backend_matches_store_default() {
    let compiled_in = [
        (cfg!(feature = "kyotocabinet"), Backend::KyotoCabinet),
        (cfg!(feature = "tkrzw"), Backend::Tkrzw),
        (cfg!(feature = "lmdb"), Backend::Lmdb),
        (cfg!(feature = "redb"), Backend::Redb),
    ]
    .into_iter()
    .find_map(|(on, backend)| on.then_some(backend))
    .expect("one backend is compiled in");
    assert_eq!(Backend::DEFAULT, compiled_in);
    assert_eq!(
        Backend::DEFAULT.extension(),
        oxpinyin_store::DEFAULT_STORE_EXT
    );
}

/// The compiled backend's file names are exactly what the runtime reader
/// opens.
#[test]
fn compiled_backend_names_match_the_runtime_reader() {
    let backend = [
        (cfg!(feature = "kyotocabinet"), Backend::KyotoCabinet),
        (cfg!(feature = "tkrzw"), Backend::Tkrzw),
        (cfg!(feature = "lmdb"), Backend::Lmdb),
        (cfg!(feature = "redb"), Backend::Redb),
    ]
    .into_iter()
    .find_map(|(on, backend)| on.then_some(backend))
    .expect("one backend is compiled in");
    assert_eq!(
        backend.is_libpinyin_dbm(),
        oxpinyin_store::DEFAULT_STORE_IS_LIBPINYIN_DBM
    );
    for (dbm, runtime) in [
        (DbmFile::PinyinIndex, oxpinyin_data::SystemDbm::PinyinIndex),
        (DbmFile::PhraseIndex, oxpinyin_data::SystemDbm::PhraseIndex),
        (DbmFile::Bigram, oxpinyin_data::SystemDbm::Bigram),
        (DbmFile::Punct, oxpinyin_data::SystemDbm::Punct),
        (
            DbmFile::AddonPinyinIndex,
            oxpinyin_data::SystemDbm::AddonPinyinIndex,
        ),
        (
            DbmFile::AddonPhraseIndex,
            oxpinyin_data::SystemDbm::AddonPhraseIndex,
        ),
    ] {
        assert_eq!(backend.dbm_file_name(dbm), runtime.file_name(), "{dbm:?}");
    }
}

/// The drop-in backends write libpinyin's names; the others their own.
#[test]
fn drop_in_backends_use_libpinyin_file_names() {
    assert_eq!(
        Backend::KyotoCabinet.dbm_file_name(DbmFile::Bigram),
        "bigram.db"
    );
    assert_eq!(
        Backend::Tkrzw.dbm_file_name(DbmFile::PinyinIndex),
        "pinyin_index.bin"
    );
    assert_eq!(
        Backend::Redb.dbm_file_name(DbmFile::PinyinIndex),
        "pinyin_index.redb"
    );
    assert_eq!(Backend::Lmdb.dbm_file_name(DbmFile::Punct), "punct.lmdb");
    assert_eq!(
        Backend::KyotoCabinet.database_format_token(),
        "KyotoCabinet"
    );
    assert_eq!(Backend::Tkrzw.database_format_token(), "Tkrzw");
}

/// Every peer backend the CLI can name reports its extension.
#[test]
fn peer_backends_report_their_expected_extensions() {
    assert_eq!(Backend::KyotoCabinet.extension(), "kct");
    assert_eq!(Backend::Redb.extension(), "redb");
    assert_eq!(Backend::Lmdb.extension(), "lmdb");
    assert_eq!(Backend::Tkrzw.extension(), "tkt");
}

/// The `--backend` argument parser accepts each of the four peer names
/// spelled the way the CLI's usage line advertises.
#[test]
fn parse_accepts_each_peer_backend_name() {
    assert_eq!(
        Backend::parse("kyotocabinet").expect("kyotocabinet parses"),
        Backend::KyotoCabinet
    );
    assert_eq!(Backend::parse("redb").expect("redb parses"), Backend::Redb);
    assert_eq!(Backend::parse("lmdb").expect("lmdb parses"), Backend::Lmdb);
    assert_eq!(
        Backend::parse("tkrzw").expect("tkrzw parses"),
        Backend::Tkrzw
    );
    // A non-peer name is rejected, not silently mapped to the default.
    assert!(Backend::parse("bogus").is_err());
}
