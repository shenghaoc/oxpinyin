// Each peer backend can produce its own use-line tests
// — the imports below are gated to whichever peer is compiled. Under
// the exactly-one-backend invariant, at most one peer is enabled per
// build, so at most one branch of each `cfg` fires.
#[cfg(feature = "tkrzw")]
use std::ops::Bound;

#[cfg(feature = "tkrzw")]
use super::RawReadOwned;
#[cfg(feature = "tkrzw")]
use super::RawReadStore;
#[cfg(feature = "tkrzw")]
use super::StoreError;
#[cfg(feature = "tkrzw")]
use super::TkrzwStore;
#[cfg(feature = "tkrzw")]
use super::WriteStore;
#[cfg(feature = "tkrzw")]
use super::WriteTxn;

/// Emits the temp-path plumbing every tier group needs.
///
/// Invoked from inside each group's generated module, so the three
/// groups do not have to triplicate it.
macro_rules! store_test_paths {
    ($mod:ident, $ext:literal) => {
        /// Owns a temporary store path and removes the data file,
        /// and any stale `<file>-lock` (no current backend creates
        /// one), when it drops — including when a test panics, so a
        /// failed assertion leaves no state behind in
        /// `std::env::temp_dir()`. Derefs to `Path`, so `&guard`
        /// passes anywhere a `&Path` is wanted.
        struct TempPath(std::path::PathBuf);

        impl std::ops::Deref for TempPath {
            type Target = std::path::Path;
            fn deref(&self) -> &std::path::Path {
                &self.0
            }
        }

        impl Drop for TempPath {
            fn drop(&mut self) {
                cleanup(&self.0);
            }
        }

        fn temp_path(tag: &str) -> TempPath {
            let path = std::env::temp_dir().join(format!(
                "oxpinyin-core-{}-{tag}-{}.{}",
                stringify!($mod),
                std::process::id(),
                $ext,
            ));
            cleanup(&path);
            TempPath(path)
        }

        fn cleanup(path: &std::path::Path) {
            let _ = std::fs::remove_file(path);
            let lock = format!("{}-lock", path.display());
            let _ = std::fs::remove_file(&lock);
        }
    };
}

/// The read tier: everything a bare [`RawReadStore`] must answer.
///
/// `$store` is exercised through [`RawReadStore`] alone. `$writer` is a
/// [`WriteStore`] used only to lay down the fixture file that `$store`
/// then opens read-only — a read-only backend supplies whichever
/// writer produces its file format, and runs this group unchanged
/// without gaining a write capability.
macro_rules! store_read_tests {
    ($mod:ident, $store:ty, $writer:ty, $ext:literal) => {
        mod $mod {
            use super::super::*;

            store_test_paths!($mod, $ext);

            /// Lays down a fixture with `$writer`, then returns a
            /// read-only `$store` handle over the same file.
            fn seeded(
                path: &std::path::Path,
                build: impl FnOnce(&mut <$writer as WriteStore>::Txn<'_>) -> Result<(), StoreError>,
            ) -> $store {
                let writer = <$writer>::create(path).unwrap();
                writer.write(build).unwrap();
                drop(writer);
                <$store>::open_read_only(path).unwrap()
            }

            #[test]
            fn range_included_included() {
                let path = temp_path("range-ii");
                let store = seeded(&path, |txn| {
                    txn.put_raw(b"a", b"1")?;
                    txn.put_raw(b"b", b"2")?;
                    txn.put_raw(b"c", b"3")?;
                    txn.put_raw(b"d", b"4")?;
                    Ok(())
                });
                let mut rows = Vec::new();
                store
                    .range_raw(
                        Bound::Included(b"b".as_slice()),
                        Bound::Included(b"c".as_slice()),
                        |k, v| {
                            rows.push((k.to_vec(), v.to_vec()));
                            Ok(())
                        },
                    )
                    .unwrap();
                assert_eq!(rows.len(), 2);
                assert_eq!(rows[0].0, b"b");
                assert_eq!(rows[0].1, b"2");
                assert_eq!(rows[1].0, b"c");
                assert_eq!(rows[1].1, b"3");
                drop(store);
                cleanup(&path);
            }

            #[test]
            fn range_included_unbounded() {
                let path = temp_path("range-iu");
                let store = seeded(&path, |txn| {
                    txn.put_raw(b"a", b"1")?;
                    txn.put_raw(b"b", b"2")?;
                    txn.put_raw(b"c", b"3")?;
                    Ok(())
                });
                let mut keys = Vec::new();
                store
                    .range_raw(Bound::Included(b"b".as_slice()), Bound::Unbounded, |k, _v| {
                        keys.push(k.to_vec());
                        Ok(())
                    })
                    .unwrap();
                assert_eq!(keys, vec![b"b".to_vec(), b"c".to_vec()]);
                drop(store);
                cleanup(&path);
            }

            /// A visitor `Break` ends the scan right there, with `Ok(())`
            /// and no further row visited; a visitor `Err` ends it and
            /// comes back as itself.
            #[test]
            fn scan_break_stops_and_error_propagates() {
                let path = temp_path("scan-break");
                let store = seeded(&path, |txn| {
                    for key in [b"a", b"b", b"c", b"d"] {
                        txn.put_raw(key, b"v")?;
                    }
                    Ok(())
                });
                for stop in 1..=4_usize {
                    let mut seen = Vec::new();
                    store
                        .scan_raw(Bound::Unbounded, Bound::Unbounded, |k, _v| {
                            seen.push(k.to_vec());
                            Ok(if seen.len() == stop {
                                ControlFlow::Break(())
                            } else {
                                ControlFlow::Continue(())
                            })
                        })
                        .unwrap();
                    let all = [b"a".to_vec(), b"b".to_vec(), b"c".to_vec(), b"d".to_vec()];
                    assert_eq!(seen, all[..stop], "break after {stop} rows");
                }
                let mut visited = 0;
                let outcome = store.scan_raw(Bound::Unbounded, Bound::Unbounded, |_k, _v| {
                    visited += 1;
                    if visited == 2 {
                        Err(StoreError::Backend("visitor failed".into()))
                    } else {
                        Ok(ControlFlow::Continue(()))
                    }
                });
                assert!(matches!(
                    outcome,
                    Err(StoreError::Backend(ref e)) if e.message == "visitor failed"
                ));
                assert_eq!(visited, 2, "the failing row is the last one visited");
                drop(store);
                cleanup(&path);
            }

            #[test]
            fn excluded_empty_lower_bound_is_safe() {
                let path = temp_path("excluded-empty");
                let store = seeded(&path, |txn| {
                    txn.put_raw(b"a", b"1")?;
                    Ok(())
                });
                let mut rows = Vec::new();
                store
                    .range_raw(Bound::Excluded(&[]), Bound::Unbounded, |key, _| {
                        rows.push(key.to_vec());
                        Ok(())
                    })
                    .unwrap();
                assert_eq!(rows, vec![b"a".to_vec()]);
                drop(store);
                cleanup(&path);
            }

            #[test]
            fn empty_bounds_never_match_or_error() {
                let path = temp_path("empty-bounds");
                let store = seeded(&path, |txn| {
                    txn.put_raw(b"a", b"1")?;
                    txn.put_raw(b"b", b"2")?;
                    Ok(())
                });

                let mut keys = Vec::new();
                store
                    .range_raw(Bound::Included(&[]), Bound::Unbounded, |k, _| {
                        keys.push(k.to_vec());
                        Ok(())
                    })
                    .unwrap();
                assert_eq!(keys, vec![b"a".to_vec(), b"b".to_vec()]);

                for hi in [Bound::<&[u8]>::Excluded(&[]), Bound::Included(&[])] {
                    let mut upper_empty_keys = Vec::new();
                    store
                        .range_raw(Bound::Unbounded, hi, |k, _| {
                            upper_empty_keys.push(k.to_vec());
                            Ok(())
                        })
                        .unwrap();
                    assert!(upper_empty_keys.is_empty());
                }
                drop(store);
                cleanup(&path);
            }
        }
    };
}

/// The write tier: creation, raw writes, compaction, and the
/// read-only handle's refusal to mutate.
macro_rules! store_write_tests {
    ($mod:ident, $store:ty, $ext:literal) => {
        mod $mod {
            use super::super::*;

            store_test_paths!($mod, $ext);

            /// The user-bigram seam round trips on every backend, and
            /// is its own pair rather than a plain hash open: libpinyin
            /// keeps its user bigram in a *different* container than
            /// its system bigram on Kyoto Cabinet (an in-memory stash
            /// dumped as a `KCSS` snapshot, `ngram_kyotodb.cpp:54-101`,
            /// vs the system's `HashDB` file).
            ///
            /// What this law does and does not catch: it pins that one
            /// backend's `write_user_bigram` and `open_user_bigram`
            /// agree with each other — so a regression that splits them
            /// (one half a snapshot, the other a hash file) fails here.
            /// It does **not** prove the format matches the pin's: the
            /// original defect was hash-on-both-halves, which is
            /// self-consistent and passes this law. Only the oracle
            /// round trip — a real libpinyin writes the profile,
            /// oxpinyin reads it — can catch that; the frozen captures
            /// that carry it are described in `docs/runbooks/oracle.md`.
            #[test]
            fn user_bigram_round_trips_through_its_own_seam() {
                let path = temp_path("user-bigram");
                let rows: Vec<(Vec<u8>, Vec<u8>)> = [1_u32, 2, 0x0100_0003]
                    .iter()
                    .map(|&tok| {
                        (
                            tok.to_le_bytes().to_vec(),
                            // A `SingleGram` blob: total then one record.
                            [&42_u32.to_le_bytes()[..], &tok.to_le_bytes()[..], &7_u32.to_le_bytes()[..]]
                                .concat(),
                        )
                    })
                    .collect();
                <$store>::write_user_bigram(&path, &rows).unwrap();

                let store = <$store>::open_user_bigram(&path).unwrap();
                for (key, value) in &rows {
                    assert_eq!(
                        store.read_raw(key).unwrap().as_deref(),
                        Some(&value[..]),
                        "gram {key:?} must read back through the user-bigram seam"
                    );
                }
                // The loader also walks the whole container.
                let mut walked = 0;
                store
                    .range_raw(
                        std::ops::Bound::Unbounded,
                        std::ops::Bound::Unbounded,
                        |_k, _v| {
                            walked += 1;
                            Ok(())
                        },
                    )
                    .unwrap();
                assert_eq!(walked, rows.len(), "the walk must see every gram");
                drop(store);
                cleanup(&path);
            }

            #[test]
            fn atomic_rollback() {
                let path = temp_path("rollback");
                let store = <$store>::create(&path).unwrap();
                let result: Result<(), _> = store.write(|txn| {
                    txn.put_raw(b"a", b"1")?;
                    txn.put_raw(b"b", b"2")?;
                    txn.put_raw(b"c", b"3")?;
                    Err(StoreError::Backend("deliberate rollback".into()))
                });
                assert!(result.is_err());
                assert!(matches!(
                    result,
                    Err(StoreError::Backend(ref e)) if e.to_string() == "deliberate rollback"
                ));
                assert_eq!(store.read_raw(b"a").unwrap(), None);
                assert_eq!(store.read_raw(b"b").unwrap(), None);
                assert_eq!(store.read_raw(b"c").unwrap(), None);
                drop(store);
                cleanup(&path);
            }

            /// Every point read clears the caller's buffer and refills
            /// it: a longer value than the buffer holds (past Berkeley
            /// DB's first `DB_DBT_USERMEM` size), then a shorter one,
            /// then a miss, through one buffer.
            #[test]
            fn point_reads_refill_one_buffer() {
                let path = temp_path("buffer-reuse");
                let store = <$store>::create(&path).unwrap();
                store
                    .write(|txn| {
                        txn.put_raw(b"long", &[7; 5000])?;
                        txn.put_raw(b"short", b"ab")?;
                        Ok(())
                    })
                    .unwrap();
                let mut value = Vec::new();
                assert!(store.get_raw(b"long", &mut value).unwrap());
                assert_eq!(value, [7; 5000]);
                assert!(store.get_raw(b"short", &mut value).unwrap());
                assert_eq!(value, b"ab");
                value.extend_from_slice(b"stale");
                assert!(!store.get_raw(b"none", &mut value).unwrap());
                assert!(value.is_empty());
                drop(store);
                cleanup(&path);
            }

            /// An empty value is a stored record, not a delete.
            ///
            /// libpinyin's index DBMs are full of zero-length
            /// continuation markers, so the raw writer depends on this
            /// on every backend. The values are built from `Vec::new()`
            /// on purpose: a `Vec<u8>` with no allocation hands its
            /// `as_ptr()` over as the dangling address `1`, which Kyoto
            /// Cabinet reads as its `Visitor::REMOVE` sentinel (see
            /// `bytes` in `kyotocabinet/adapter.cc`); a `b""` literal has a real
            /// address and never trips it.
            #[test]
            fn empty_value_is_a_record() {
                let path = temp_path("empty-value");
                let store = <$store>::create(&path).unwrap();
                let empty: Vec<u8> = Vec::new();
                store
                    .write(|txn| {
                        txn.put_raw(b"marker", &empty)?;
                        txn.put_raw(b"full", b"v")?;
                        Ok(())
                    })
                    .unwrap();
                assert_eq!(store.read_raw(b"marker").unwrap(), Some(Vec::new()));
                // The existence query counts an empty value; a miss is
                // no key.
                assert!(store.contains_raw(b"marker").unwrap());
                assert!(!store.contains_raw(b"absent").unwrap());
                // Overwriting a full record with an empty value keeps
                // the record; only `remove_raw` deletes.
                store.write(|txn| txn.put_raw(b"full", &empty)).unwrap();
                assert_eq!(store.read_raw(b"full").unwrap(), Some(Vec::new()));
                let mut rows = Vec::new();
                store
                    .range_raw(Bound::Unbounded, Bound::Unbounded, |key, value| {
                        rows.push((key.to_vec(), value.to_vec()));
                        Ok(())
                    })
                    .unwrap();
                assert_eq!(
                    rows,
                    vec![(b"full".to_vec(), Vec::new()), (b"marker".to_vec(), Vec::new())]
                );
                drop(store);
                cleanup(&path);
            }

            #[test]
            fn remove_in_write() {
                let path = temp_path("remove");
                let store = <$store>::create(&path).unwrap();
                store
                    .write(|txn| {
                        txn.put_raw(b"k", b"v")?;
                        txn.remove_raw(b"k")?;
                        Ok(())
                    })
                    .unwrap();
                assert_eq!(store.read_raw(b"k").unwrap(), None);
                drop(store);
                cleanup(&path);
            }

            #[test]
            fn read_only_reopen_preserves_reads_and_rejects_mutation() {
                let path = temp_path("read-only");
                let store = <$store>::create(&path).unwrap();
                store.write(|txn| txn.put_raw(b"key", b"value")).unwrap();
                drop(store);

                let mut readonly = <$store>::open_read_only(&path).unwrap();
                assert_eq!(readonly.read_raw(b"key").unwrap(), Some(b"value".to_vec()));
                assert!(matches!(
                    readonly.write(|_| Ok(())),
                    Err(StoreError::ReadOnly)
                ));
                assert!(matches!(readonly.compact(), Err(StoreError::ReadOnly)));
                drop(readonly);
                cleanup(&path);
            }

            #[test]
            fn compact_preserves_data() {
                let path = temp_path("compact");
                let mut store = <$store>::create(&path).unwrap();
                store.write(|txn| txn.put_raw(b"k", b"v")).unwrap();
                store.compact().unwrap();
                assert_eq!(store.read_raw(b"k").unwrap(), Some(b"v".to_vec()));
                drop(store);
                cleanup(&path);
            }

            /// The in-memory session store (the libpinyin session's
            /// container, #546/#531): every write-tier operation
            /// holds on it — ordered walks, rollback, compaction —
            /// and nothing reaches the file system.
            #[test]
            fn in_memory_store_behaves_as_a_tree_store() {
                let mut store = <$store>::create_in_memory().unwrap();
                assert_eq!(store.count_raw().unwrap(), 0);
                store
                    .write(|txn| {
                        txn.put_raw(b"b", b"2")?;
                        txn.put_raw(b"a", b"1")?;
                        txn.put_raw(b"c", b"3")?;
                        Ok(())
                    })
                    .unwrap();
                assert_eq!(store.read_raw(b"a").unwrap(), Some(b"1".to_vec()));
                let mut walked = Vec::new();
                store
                    .range_raw(Bound::Unbounded, Bound::Unbounded, |k, v| {
                        walked.push((k.to_vec(), v.to_vec()));
                        Ok(())
                    })
                    .unwrap();
                assert_eq!(
                    walked,
                    vec![
                        (b"a".to_vec(), b"1".to_vec()),
                        (b"b".to_vec(), b"2".to_vec()),
                        (b"c".to_vec(), b"3".to_vec()),
                    ]
                );
                let mut ranged = Vec::new();
                store
                    .range_raw(Bound::Included(&b"b"[..]), Bound::Unbounded, |k, _| {
                        ranged.push(k.to_vec());
                        Ok(())
                    })
                    .unwrap();
                assert_eq!(ranged, vec![b"b".to_vec(), b"c".to_vec()]);
                // An `Err` rolls the transaction back.
                let rolled: Result<(), StoreError> = store.write(|txn| {
                    txn.put_raw(b"d", b"4")?;
                    txn.remove_raw(b"a")?;
                    Err(StoreError::Backend("deliberate rollback".into()))
                });
                assert!(rolled.is_err());
                assert_eq!(store.read_raw(b"d").unwrap(), None);
                assert_eq!(store.read_raw(b"a").unwrap(), Some(b"1".to_vec()));
                store.compact().unwrap();
                assert_eq!(store.read_raw(b"c").unwrap(), Some(b"3".to_vec()));
                store.write(|txn| txn.remove_raw(b"c")).unwrap();
                assert_eq!(store.read_raw(b"c").unwrap(), None);
            }
        }
    };
}

// Every backend offers both tiers, so each runs both groups and is
// its own fixture writer. A read-only backend would invoke only
// `store_read_tests!`. Each group is gated by the peer's feature —
// the exactly-one-backend guards refuse combined builds, so at most
// one of these groups is ever compiled.
#[cfg(feature = "tkrzw")]
store_read_tests!(tkrzw_read, TkrzwStore, TkrzwStore, "tkrzw");
#[cfg(feature = "tkrzw")]
store_write_tests!(tkrzw_write, TkrzwStore, "tkrzw");

#[cfg(feature = "tkrzw")]
#[test]
fn tkrzw_rejects_nul_path() {
    let path = std::path::PathBuf::from("oxpinyin-core\0invalid.tkrzw");
    assert!(matches!(
        TkrzwStore::create(&path),
        Err(StoreError::InvalidInput("path contains NUL"))
    ));
    assert!(matches!(
        TkrzwStore::open_read_only(&path),
        Err(StoreError::InvalidInput("path contains NUL"))
    ));
}

#[cfg(feature = "kyotocabinet")]
store_read_tests!(kc_read, KcStore, KcStore, "kc");
#[cfg(feature = "kyotocabinet")]
store_write_tests!(kc_write, KcStore, "kc");

#[cfg(feature = "bdb")]
store_read_tests!(bdb_read, BdbStore, BdbStore, "db");
#[cfg(feature = "bdb")]
store_write_tests!(bdb_write, BdbStore, "db");

/// Removes a tkrzw store file, and any stale `<file>-lock` (tkrzw
/// creates none: it locks the data file itself with `fcntl`), on
/// drop, so a panicking test leaves nothing behind in
/// `std::env::temp_dir()`.
#[cfg(feature = "tkrzw")]
struct RemoveTkrzw(std::path::PathBuf);

#[cfg(feature = "tkrzw")]
impl Drop for RemoveTkrzw {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
        let mut lock = self.0.clone().into_os_string();
        lock.push("-lock");
        let _ = std::fs::remove_file(std::path::PathBuf::from(lock));
    }
}

#[cfg(feature = "tkrzw")]
#[test]
fn tkrzw_scan_keeps_every_borrowed_record_intact() {
    // The zero-copy walk hands the visitor a pointer into tkrzw's
    // record buffer that is valid only for that record's callback. A
    // use-after-free of that borrow corrupts LATER records, not the
    // first — the iterator invalidates or reuses the memory a prior
    // callback read. Walk many records and verify each key and value
    // in place as it is visited, so a stale read fails the assertion
    // rather than passing silently.
    fn expected_value(i: u32) -> [u8; 32] {
        let mut value = [0u8; 32];
        for (j, byte) in value.iter_mut().enumerate() {
            *byte =
                u8::try_from(i.wrapping_add(u32::try_from(j).expect("loop index fits u32")) & 0xFF)
                    .expect("masked byte fits u8");
        }
        value
    }

    const RECORDS: u32 = 10_000;
    let path = std::env::temp_dir().join(format!(
        "oxpinyin-core-tkrzw-scan-many-{}.tkrzw",
        std::process::id(),
    ));
    let _ = std::fs::remove_file(&path);
    let _cleanup = RemoveTkrzw(path.clone());
    let store = TkrzwStore::create(&path).unwrap();

    store
        .write(|txn| {
            for i in 0..RECORDS {
                txn.put_raw(&i.to_be_bytes(), &expected_value(i))?;
            }
            Ok(())
        })
        .unwrap();

    let mut seen = 0u32;
    store
        .range_raw(Bound::Unbounded, Bound::Unbounded, |key, value| {
            let i = u32::from_be_bytes(key.try_into().unwrap());
            assert_eq!(i, seen, "scan must visit keys in ascending order");
            assert_eq!(
                value,
                &expected_value(i),
                "record {i} came back corrupt — borrowed buffer invalidated early"
            );
            seen += 1;
            Ok(())
        })
        .unwrap();
    assert_eq!(seen, RECORDS, "every record must be visited exactly once");
    drop(store);
}

/// tkrzw's record processors answer "keep this record" and "delete
/// it" with two sentinels that libtkrzw recognises by the *address*
/// of their backing bytes, and every `remove` this backend issues
/// ends in that comparison inside the library. A libtkrzw holding
/// more than one copy of those bytes compares against the wrong one
/// — the known cause is an LTO build of tkrzw older than 1.0.34.
/// Removing an absent record then stores one and removing a stored
/// record leaves it in place, each carrying the sentinel's five
/// bytes as its value and each write reporting success; and on the
/// two file classes the rebuild behind `compact` answers
/// `CANCELED_ERROR` (`docs/findings/tkrzw-distro-compat.md`).
///
/// The shared suites trip over the same faults; this test names
/// them, on each of the four DBM classes the backend opens: the two
/// file classes, the libpinyin session's in-memory `BabyDBM`, and
/// the user bigram's `TinyDBM`.
#[cfg(feature = "tkrzw")]
#[test]
fn tkrzw_library_honours_its_sentinel_protocol() {
    use super::{GramReadOwned, TkrzwUserBigramDb, UserBigramDb};

    type Create = fn(&std::path::Path) -> Result<TkrzwStore, StoreError>;

    const CAUSE: &str = "the linked libtkrzw does not recognise its own NOOP/REMOVE \
             sentinels (known cause: an LTO build of tkrzw older than 1.0.34); see \
             docs/findings/tkrzw-distro-compat.md";

    fn check(class: &str, mut store: TkrzwStore) {
        store
            .write(|txn| {
                txn.put_raw(b"kept", b"v")?;
                txn.put_raw(b"removed", b"v")
            })
            .unwrap();

        // NOOP: the callback's answer for a record that is not there.
        store.write(|txn| txn.remove_raw(b"absent")).unwrap();
        assert_eq!(
            store.read_raw(b"absent").unwrap(),
            None,
            "{class}: removing an absent record stored one: {CAUSE}"
        );

        // REMOVE: its answer for a record that is. A separate
        // transaction from the put, so the record is really stored
        // when the removal reaches the library.
        store.write(|txn| txn.remove_raw(b"removed")).unwrap();
        assert_eq!(
            store.read_raw(b"removed").unwrap(),
            None,
            "{class}: a removed record is still stored: {CAUSE}"
        );

        // On the file classes the rebuild replays every record
        // through a processor that answers NOOP and takes any other
        // answer as a cancellation; `BabyDBM`'s rebuild does nothing.
        if let Err(error) = store.compact() {
            panic!("{class}: compact failed ({error}): {CAUSE}");
        }
        assert_eq!(store.read_raw(b"kept").unwrap(), Some(b"v".to_vec()));
    }

    let file_classes: [(&str, Create); 2] = [
        ("TreeDBM", TkrzwStore::create),
        ("HashDBM", TkrzwStore::create_hash),
    ];
    for (class, create) in file_classes {
        let path = std::env::temp_dir().join(format!(
            "oxpinyin-core-tkrzw-sentinel-{class}-{}.tkrzw",
            std::process::id(),
        ));
        let _ = std::fs::remove_file(&path);
        let _cleanup = RemoveTkrzw(path.clone());
        check(class, create(&path).unwrap());
    }

    // The container a libpinyin session runs on.
    check("BabyDBM", TkrzwStore::create_in_memory().unwrap());

    // The user bigram's in-memory TinyDBM takes the same two answers.
    let bigram = TkrzwUserBigramDb::empty().unwrap();
    bigram.store(b"kept", b"v").unwrap();
    bigram.store(b"removed", b"v").unwrap();
    bigram.remove(b"absent").unwrap();
    assert_eq!(
        bigram.read_gram(b"absent").unwrap(),
        None,
        "TinyDBM: removing an absent record stored one: {CAUSE}"
    );
    bigram.remove(b"removed").unwrap();
    assert_eq!(
        bigram.read_gram(b"removed").unwrap(),
        None,
        "TinyDBM: a removed record is still stored: {CAUSE}"
    );
    assert_eq!(bigram.read_gram(b"kept").unwrap(), Some(b"v".to_vec()));
}

// ── per-peer key-ordering conformance ─────────────────────────
//
// The byte-order contract must hold *identically* across every
// peer backend, and — the property a small-id fixture cannot see —
// byte order must diverge from integer order across the 256
// boundary. Under the exactly-one-backend invariant the tests
// cannot cross-compare two peers in one process, so each build
// runs them against its own `DefaultStore`. Running every peer
// build (KC / Tkrzw / BDB) through CI gives the same
// coverage the earlier in-process cross-peer check gave: each peer
// independently satisfies the byte-order contract, and the
// expected walk order is computed mathematically (sort the keys)
// rather than borrowed from a reference peer's output.
mod key_ordering {
    use std::ops::Bound;
    use std::path::Path;

    use super::super::{DefaultStore, WriteStore, WriteTxn};

    /// Tokens that cross the 256 boundary in the low byte and in
    /// higher bytes, so their little-endian byte order differs from
    /// their integer order. Deliberately unsorted: the store imposes
    /// the walk order, not the insertion order.
    const BOUNDARY_TOKENS: &[u32] = &[
        0x0000_0200,
        0x0000_00FF,
        0x0700_0100,
        0x0000_0100,
        0x0000_0001,
        0x0000_FFFF,
        0x0000_01FF,
        0x0700_0001,
        0x0001_0000,
        0x0700_00FF,
        0x00FF_0000,
        0x0100_0000,
    ];

    type Rows = Vec<(Vec<u8>, Vec<u8>)>;

    /// Owns a temp store path; removes the data file and any stale
    /// `<file>-lock` (no current backend creates one) on drop, so a
    /// panicking test leaves nothing behind.
    struct TempPath(std::path::PathBuf);

    impl TempPath {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "oxpinyin-core-keyorder-{tag}-{}.db",
                std::process::id(),
            ));
            let this = Self(path);
            this.cleanup();
            this
        }

        fn cleanup(&self) {
            let _ = std::fs::remove_file(&self.0);
            let mut lock = self.0.clone().into_os_string();
            lock.push("-lock");
            let _ = std::fs::remove_file(std::path::PathBuf::from(lock));
        }
    }

    impl Drop for TempPath {
        fn drop(&mut self) {
            self.cleanup();
        }
    }

    /// Insert `keys` (each mapped to a 1-byte value) and return the
    /// raw walk as owned rows.
    fn insert_and_walk<S: WriteStore>(path: &Path, keys: &[Vec<u8>]) -> Rows {
        let store = S::create(path).expect("create store");
        store
            .write(|txn| {
                for key in keys {
                    txn.put_raw(key, b"v")?;
                }
                Ok(())
            })
            .expect("bulk insert");
        let mut rows = Rows::new();
        store
            .range_raw(Bound::Unbounded, Bound::Unbounded, |key, value| {
                rows.push((key.to_vec(), value.to_vec()));
                Ok(())
            })
            .expect("raw walk");
        drop(store);
        rows
    }

    /// Insert `keys` and return the raw range walk over `[lo, hi)`.
    fn insert_and_range<S: WriteStore>(
        path: &Path,
        keys: &[Vec<u8>],
        lo: &[u8],
        hi: &[u8],
    ) -> Rows {
        let store = S::create(path).expect("create store");
        store
            .write(|txn| {
                for key in keys {
                    txn.put_raw(key, b"v")?;
                }
                Ok(())
            })
            .expect("bulk insert");
        let mut rows = Rows::new();
        store
            .range_raw(Bound::Included(lo), Bound::Excluded(hi), |key, value| {
                rows.push((key.to_vec(), value.to_vec()));
                Ok(())
            })
            .expect("range walk");
        drop(store);
        rows
    }

    fn le_keys() -> Vec<Vec<u8>> {
        BOUNDARY_TOKENS
            .iter()
            .map(|t| t.to_le_bytes().to_vec())
            .collect()
    }

    // ── the store contract: byte order, not integer order ──────

    /// Ascending sort of `keys` by raw byte order — the store's one
    /// walk-order rule (`docs/findings/store-key-ordering.md`).
    fn sorted_by_bytes(keys: &[Vec<u8>]) -> Vec<Vec<u8>> {
        let mut sorted: Vec<Vec<u8>> = keys.to_vec();
        sorted.sort();
        sorted
    }

    #[test]
    fn walks_raw_keys_in_byte_order_and_that_is_not_integer_order() {
        let path = TempPath::new("byteorder");
        let rows = insert_and_walk::<DefaultStore>(&path.0, &le_keys());

        let keys: Vec<Vec<u8>> = rows.iter().map(|(k, _)| k.clone()).collect();
        assert_eq!(
            keys,
            sorted_by_bytes(&le_keys()),
            "store walk must be strictly ascending by raw byte order",
        );

        // Non-vacuity: the SAME walk, decoded to integers, is NOT
        // ascending — byte order and integer order genuinely differ
        // on this 256-crossing set.
        let tokens: Vec<u32> = rows
            .iter()
            .map(|(k, _)| u32::from_le_bytes([k[0], k[1], k[2], k[3]]))
            .collect();
        assert!(
            !tokens.is_sorted(),
            "the 256-boundary set must make byte order differ from integer order",
        );
    }

    #[test]
    fn range_walks_byte_order_across_256() {
        let path = TempPath::new("range");
        let keys = le_keys();
        // A window straddling 256 in byte order: LE(0x0000_0100)
        // (`00 01 00 00`) up to LE(0x0000_00FF) (`FF 00 00 00`).
        let lo = 0x0000_0100_u32.to_le_bytes();
        let hi = 0x0000_00FF_u32.to_le_bytes();
        let rows = insert_and_range::<DefaultStore>(&path.0, &keys, &lo, &hi);
        assert!(!rows.is_empty(), "the range must match rows");
        let out: Vec<Vec<u8>> = rows.iter().map(|(k, _)| k.clone()).collect();
        let expected: Vec<Vec<u8>> = sorted_by_bytes(&keys)
            .into_iter()
            .filter(|k| k.as_slice() >= &lo[..] && k.as_slice() < &hi[..])
            .collect();
        assert_eq!(
            out, expected,
            "range walk must return the ascending byte-ordered slice \
                 inside its bounds",
        );
    }
}

#[cfg(all(
    test,
    any(feature = "bdb", feature = "kyotocabinet", feature = "tkrzw")
))]
mod user_bigram_db_tests {
    use crate::store::{DefaultUserBigramDb, GramReadOwned, UserBigramDb};

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "oxpinyin-user-bigram-db-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn key(token: u32) -> [u8; 4] {
        token.to_le_bytes()
    }

    #[test]
    fn a_missing_file_loads_an_empty_container() {
        let dir = temp_dir("missing");
        let db = DefaultUserBigramDb::load_db(&dir.join("user_bigram.db")).expect("load");
        assert!(db.keys().expect("keys").is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn store_get_remove_and_the_walk_see_the_same_records() {
        let db = DefaultUserBigramDb::empty().expect("empty");
        for token in 1..=40_u32 {
            db.store(&key(token), &token.to_le_bytes().repeat(3))
                .expect("store");
        }
        db.remove(&key(7)).expect("remove");
        db.remove(&key(999)).expect("absent remove is not an error");
        assert_eq!(db.read_gram(&key(7)).expect("get"), None);
        assert_eq!(
            db.read_gram(&key(8)).expect("get"),
            Some(8_u32.to_le_bytes().repeat(3))
        );
        let mut keys = db.keys().expect("keys");
        assert_eq!(keys.len(), 39);
        keys.sort();
        let mut expected: Vec<Option<[u8; 4]>> = (1..=40_u32)
            .filter(|&token| token != 7)
            .map(|token| Some(key(token)))
            .collect();
        expected.sort();
        assert_eq!(keys, expected);
    }

    #[test]
    fn keys_mark_a_key_that_is_not_four_bytes_in_its_place() {
        let db = DefaultUserBigramDb::empty().expect("empty");
        db.store(&key(3), b"gram").expect("store");
        db.store(b"abc", b"gram").expect("store");
        db.store(b"abcde", b"gram").expect("store");
        let mut keys = db.keys().expect("keys");
        keys.sort();
        assert_eq!(keys, vec![None, None, Some(key(3))]);
    }

    #[test]
    fn one_buffer_serves_successive_reads() {
        let db = DefaultUserBigramDb::empty().expect("empty");
        db.store(&key(1), &[1; 300]).expect("store");
        db.store(&key(2), b"").expect("store");
        db.store(&key(3), &[3; 2]).expect("store");
        let mut value = Vec::new();
        assert!(db.get(&key(1), &mut value).expect("get"));
        assert_eq!(value, [1; 300]);
        // A shorter value, an empty one and a miss each replace what the
        // buffer held.
        assert!(db.get(&key(3), &mut value).expect("get"));
        assert_eq!(value, [3; 2]);
        assert!(db.get(&key(2), &mut value).expect("get"));
        assert!(value.is_empty());
        value.extend_from_slice(b"stale");
        assert!(!db.get(&key(9), &mut value).expect("get"));
        assert!(value.is_empty());
    }

    /// tests/storage/test_ngram.cpp:39-46 and :69-79 at 074a2219: attach,
    /// the two stored rows, `get_all_items`, `save_db` and `load_db`.
    #[test]
    fn a_native_hash_snapshot_keeps_both_predecessor_rows() {
        use crate::store::{DefaultStore, RawReadStore, WriteStore, WriteTxn};

        let dir = temp_dir("attached-snapshot");
        let path = dir.join("test.db");
        // The fixed program also covers an existing empty input file.
        std::fs::write(&path, []).unwrap();
        let bigram = DefaultStore::create_hash(&path).expect("attach empty file");
        let rows = [
            [16_u32, 1, 16, 2, 1, 3, 32, 4, 4, 6, 2],
            [32_u32, 2, 1, 3, 32, 4, 4, 5, 8, 6, 2],
        ];
        let rows = rows.map(|row| {
            row.into_iter()
                .flat_map(u32::to_ne_bytes)
                .collect::<Vec<_>>()
        });
        for (token, row) in [1_u32, 2].into_iter().zip(&rows) {
            bigram
                .write(|txn| txn.put_raw(&token.to_ne_bytes(), row))
                .unwrap();
        }
        let keys = bigram.native_keys().unwrap();
        let order: [u32; 2] = if cfg!(feature = "bdb") {
            [2, 1]
        } else {
            [1, 2]
        };
        assert_eq!(keys, order.map(|token| token.to_ne_bytes().to_vec()));

        let snapshot = dir.join("snapshot.db");
        bigram.save_bigram_db(&snapshot).expect("snapshot");
        let blocked = dir.join("directory");
        std::fs::create_dir(&blocked).unwrap();
        std::fs::write(blocked.join("keep"), b"kept").unwrap();
        assert!(bigram.save_bigram_db(&blocked).is_err());
        assert!(DefaultStore::create_hash(&blocked).is_err());
        assert_eq!(std::fs::read(blocked.join("keep")).unwrap(), b"kept");
        drop(bigram);

        let copy = DefaultUserBigramDb::load_db(&snapshot).expect("load snapshot");
        for (token, row) in [1_u32, 2].into_iter().zip(rows) {
            assert_eq!(copy.read_gram(&token.to_ne_bytes()).unwrap(), Some(row));
        }
        drop(copy);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn save_then_load_keeps_every_record() {
        let dir = temp_dir("round-trip");
        let path = dir.join("user_bigram.db");
        let db = DefaultUserBigramDb::empty().expect("empty");
        for token in [1_u32, 0x0100_0001, 0x0700_0001, 0x0100_05db] {
            db.store(&key(token), &[token.to_le_bytes(), [9, 0, 0, 0]].concat())
                .expect("store");
        }
        db.save_db(&path).expect("save");
        let reloaded = DefaultUserBigramDb::load_db(&path).expect("load");
        let mut before = db.keys().expect("keys");
        let mut after = reloaded.keys().expect("keys");
        before.sort();
        after.sort();
        assert_eq!(before, after);
        for token in [1_u32, 0x0100_0001, 0x0700_0001, 0x0100_05db] {
            assert_eq!(
                reloaded.read_gram(&key(token)).expect("get"),
                db.read_gram(&key(token)).expect("get")
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

mod backend_files {
    //! Black-box trait laws over the public `Store` surface that no backend
    //! unit test holds: the literal `#` in a store path, the 511/512-byte key
    //! boundary, the removal of an absent key, and the raw bytes of a user
    //! bigram row that ends in a partial item.
    //!
    //! Exactly one backend is compiled per build (the crate's
    //! `compile_error!` guards), so `DefaultStore` resolves this file's
    //! backend the same way every consumer's does, and the
    //! `store-backends.yml` matrix runs this file once per peer by running
    //! the whole workspace sweep under each backend feature.
    //!
    //! No backend leaves a file beside the store: tkrzw locks the data file
    //! itself with `fcntl` (`tkrzw_file_mmap.cc:124-133` at 1.0.34), Berkeley
    //! DB opens without an environment, and Kyoto Cabinet's only companion is
    //! its `<file>.wal`. The guard below removes the store's own path on drop.

    use crate::store::{DEFAULT_STORE_EXT, DefaultStore, RawReadStore, WriteStore, WriteTxn};

    /// Owns a temporary store path and removes the data file or directory
    /// when it drops — including on panic, so a failed law leaves nothing
    /// behind in the temp dir.
    struct TempPath(std::path::PathBuf);

    impl std::ops::Deref for TempPath {
        type Target = std::path::Path;
        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }

    impl Drop for TempPath {
        fn drop(&mut self) {
            cleanup(&self.0);
        }
    }

    /// A point read as an owned value: `None` on a miss.
    fn read(store: &DefaultStore, key: &[u8]) -> Option<Vec<u8>> {
        let mut value = Vec::new();
        store.get_raw(key, &mut value).unwrap().then_some(value)
    }

    fn cleanup(path: &std::path::Path) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_dir_all(path);
    }

    fn temp_path(tag: &str) -> TempPath {
        let path = std::env::temp_dir().join(format!(
            "oxpinyin-core-trait-laws-{tag}-{}.{DEFAULT_STORE_EXT}",
            std::process::id()
        ));
        cleanup(&path);
        TempPath(path)
    }

    /// A literal '#' belongs to either the parent directory or the DBM
    /// filename. In particular, `#type=` must never select a different class.
    /// This covers both persistent KC classes; the C facade fixes their
    /// basenames, so its contract cases cannot rename these DBM files.
    #[test]
    fn hash_paths_create_and_reopen_tree_and_hash_files() {
        let root = temp_path("hash-path");
        std::fs::create_dir(&*root).unwrap();
        for (directory, filename) in [
            ("parent#type=kch", "store.db"),
            ("parent", "store#type=kch.db"),
        ] {
            let parent = root.join(directory);
            std::fs::create_dir(&parent).unwrap();
            for hash in [false, true] {
                let path = parent.join(format!("{hash}-{filename}"));
                let store = if hash {
                    DefaultStore::create_hash(&path).unwrap()
                } else {
                    DefaultStore::create(&path).unwrap()
                };
                store.write(|txn| txn.put_raw(b"key", b"value")).unwrap();
                drop(store);
                assert!(path.is_file(), "the literal filename must be created");
                let store = if hash {
                    DefaultStore::open_hash_read_only(&path).unwrap()
                } else {
                    DefaultStore::open_read_only(&path).unwrap()
                };
                let mut value = Vec::new();
                assert!(store.get_raw(b"key", &mut value).unwrap());
                assert_eq!(value, b"value");
            }
        }
    }

    /// The 511/512-byte key boundary: every peer accepts both.
    #[test]
    fn key_length_edges_follow_the_documented_contract() {
        let path = temp_path("key-length");
        let store = DefaultStore::create(&path).unwrap();

        let key_511 = vec![b'k'; 511];
        store
            .write(|txn| txn.put_raw(&key_511, b"edge-511"))
            .expect("a 511-byte key is legal on every peer");
        assert_eq!(read(&store, &key_511), Some(b"edge-511".to_vec()));

        let key_512 = vec![b'k'; 512];
        store
            .write(|txn| txn.put_raw(&key_512, b"edge-512"))
            .expect("a 512-byte key is legal on every peer");
        assert_eq!(read(&store, &key_512), Some(b"edge-512".to_vec()));
    }

    /// Removing an absent key is a no-op, not an error.
    #[test]
    fn removing_an_absent_key_succeeds() {
        let path = temp_path("absent-remove");
        let store = DefaultStore::create(&path).unwrap();
        store.write(|txn| txn.put_raw(b"kept", b"1")).unwrap();
        store
            .write(|txn| txn.remove_raw(b"never-there"))
            .expect("removing an absent key must succeed");
        assert_eq!(read(&store, b"kept"), Some(b"1".to_vec()));
    }

    /// A user bigram row whose items end in a partial record loads as raw
    /// bytes: the load decodes nothing, so the row, partial bytes and all, is
    /// the value every later read takes apart the pin's way.
    #[test]
    fn partial_user_bigram_items_keep_defined_reads_and_raw_bytes() {
        let root = temp_path("partial-bigram-items");
        std::fs::create_dir(&*root).unwrap();
        let path = root.join("user_bigram.db");
        let key = 0x0700_0001_u32.to_le_bytes();
        let record = [9_u32.to_le_bytes(), 7_u32.to_le_bytes()].concat();
        for tail_len in 1..8 {
            for complete_items in 0..=1 {
                let mut value = 7_u32.to_le_bytes().to_vec();
                if complete_items == 1 {
                    value.extend_from_slice(&record);
                }
                value.extend(std::iter::repeat_n(0xa5, tail_len));
                DefaultStore::write_user_bigram(&path, &[(key.to_vec(), value.clone())])
                    .expect("write user bigram");

                let raw = DefaultStore::open_user_bigram(&path).expect("open raw");
                let mut rows = Vec::new();
                raw.copy_raw(|key, value| rows.push((key.to_vec(), value.to_vec())))
                    .expect("walk");
                assert_eq!(rows, [(key.to_vec(), value.clone())]);

                assert_eq!(read(&raw, &key), Some(value));
            }
        }
    }
}

/// `write_deferred`, the trait contract every backend implements.
mod deferred_writes {
    use crate::store::{DefaultStore, RawReadStore, StoreError, WriteStore, WriteTxn};

    #[test]
    fn the_deferred_removal_flag_aggregates_every_removal() {
        let scratch = tempfile::tempdir().expect("scratch directory");
        let db = DefaultStore::create_hash(&scratch.path().join("raw.db")).expect("hash create");
        let put = |key: &'static [u8]| {
            db.write_deferred(|txn| txn.put_raw(key, b"v"))
                .expect("put");
        };
        let remove_two = |first: &'static [u8], second: &'static [u8]| {
            db.write_deferred(|txn| {
                txn.remove_raw(first)?;
                txn.remove_raw(second)
            })
            .expect("remove")
            .1
        };
        put(b"a");
        put(b"b");
        assert!(remove_two(b"a", b"b"), "both records found");
        put(b"b");
        // The buffer applies in key order: the missing `a` first, the found
        // `b` last, so a last-answer-wins flag would answer true here.
        assert!(!remove_two(b"a", b"b"), "one of two missing");
        assert!(!remove_two(b"y", b"z"), "both missing");
        let ((), found) = db
            .write_deferred(|txn| txn.put_raw(b"c", b"v"))
            .expect("put");
        assert!(found, "no removal at all");
    }

    #[test]
    fn a_failed_deferred_closure_applies_nothing() {
        let scratch = tempfile::tempdir().expect("scratch directory");
        let path = scratch.path().join("raw.db");
        let db = DefaultStore::create_hash(&path).expect("hash create");
        let result = db.write_deferred(|txn| {
            txn.put_raw(b"k", b"v")?;
            Err::<(), _>(StoreError::InvalidInput("closure failed"))
        });
        assert!(result.is_err());
        let mut value = Vec::new();
        assert!(
            !db.get_raw(b"k", &mut value).expect("read"),
            "no earlier write stays"
        );
    }
}
