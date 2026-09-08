//! Black-box trait laws over the public `Store` surface — G2 of the
//! testing-strategy assessment (`testing-strategy.md` §2): the crate had
//! 47 unit tests and the backend matrix, but no single test body run
//! identically through every backend's public traits.
//!
//! Exactly one backend is compiled per build (the crate's
//! `compile_error!` guards), so `DefaultStore` resolves this file's
//! backend the same way every consumer's does, and the
//! `store-backends.yml` matrix runs this file once per peer by running
//! the whole workspace sweep under each backend feature. The laws:
//!
//! - **ordered iteration**: `for_each` visits keys in ascending byte
//!   order regardless of insertion order;
//! - **prefix-scan boundaries**: a half-open `range` yields exactly the
//!   keys whose bytes fall inside it — the plan's `[b"a", b"a\xff"]`
//!   under `b"a"..b"b"` law;
//! - **empty key**: LMDB refuses it with `InvalidInput` (its 1..=511
//!   contract); every other peer stores it, reads it back, and sorts it
//!   before all other keys;
//! - **max key length**: a 511-byte key is legal everywhere; a 512-byte
//!   key is LMDB's typed `InvalidInput` and round-trips elsewhere;
//! - **reopen equality**: rows written through one handle read back
//!   identically through a fresh `open_read_only` handle;
//! - **transaction semantics**: read-your-writes inside `write`, and a
//!   closure returning `Err` rolls the whole transaction back.
//!
//! The `-lock` sidecar note from the crate's own unit tests applies
//! here too: opening a store dirties LMDB's sidecar, so the guard below
//! removes both shapes on drop and the test never asserts on sidecars.

use std::ops::Bound;

use oxpinyin_store::{DEFAULT_STORE_EXT, DefaultStore, ReadStore, StoreError, WriteStore};

/// Owns a temporary store path and removes the data file or directory
/// and the `-lock` sidecar (either shape) when it drops — including on
/// panic, so a failed law leaves nothing behind in the temp dir.
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

fn cleanup(path: &std::path::Path) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_dir_all(path);
    let lock = format!("{}-lock", path.display());
    let _ = std::fs::remove_file(&lock);
    let _ = std::fs::remove_dir_all(&lock);
}

fn temp_path(tag: &str) -> TempPath {
    let path = std::env::temp_dir().join(format!(
        "oxpinyin-store-trait-laws-{tag}-{}.{DEFAULT_STORE_EXT}",
        std::process::id()
    ));
    cleanup(&path);
    TempPath(path)
}

/// Whether the compiled backend accepts the empty key. LMDB's contract
/// is 1..=511 bytes; the other three peers have no such floor.
const EMPTY_KEY_LEGAL: bool = !cfg!(feature = "lmdb");
/// Whether the compiled backend accepts a key longer than LMDB's
/// 511-byte ceiling.
const LONG_KEY_LEGAL: bool = !cfg!(feature = "lmdb");

/// The assessment's example row set, minus the empty key on peers that
/// refuse it.
fn seeded_keys(store: &DefaultStore) {
    store
        .write(|txn| {
            for (key, value) in [
                (&b"a\xff"[..], &b"v-high"[..]),
                (b"".as_slice(), b"v-empty".as_slice()),
                (b"b".as_slice(), b"v-b".as_slice()),
                (b"a".as_slice(), b"v-a".as_slice()),
            ] {
                if key.is_empty() && !EMPTY_KEY_LEGAL {
                    continue;
                }
                txn.put("laws", key, value)?;
            }
            Ok(())
        })
        .unwrap();
}

/// `for_each` visits keys in ascending byte order whatever order they
/// were inserted in, and the half-open `a..b` scan returns exactly the
/// keys whose bytes fall inside it.
#[test]
fn iteration_is_ordered_and_prefix_scans_respect_boundaries() {
    let path = temp_path("ordered");
    let store = DefaultStore::create(&path).unwrap();
    seeded_keys(&store);

    let mut seen = Vec::new();
    store
        .for_each("laws", &mut |key, _| {
            seen.push(key.to_vec());
            Ok(())
        })
        .unwrap();
    let mut expected = vec![b"a".to_vec(), b"a\xff".to_vec(), b"b".to_vec()];
    if EMPTY_KEY_LEGAL {
        expected.insert(0, Vec::new());
    }
    assert_eq!(seen, expected, "iteration must be ascending byte order");

    // The plan's law: exactly the two `a…` keys, `b` excluded by the
    // exclusive bound and everything below `a` by the low bound.
    let mut in_range = Vec::new();
    store
        .range(
            "laws",
            Bound::Included(b"a".as_slice()),
            Bound::Excluded(b"b".as_slice()),
            &mut |key, _| {
                in_range.push(key.to_vec());
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(in_range, vec![b"a".to_vec(), b"a\xff".to_vec()]);

    // Inclusive upper bound flips `b` in.
    let mut through_b = Vec::new();
    store
        .range(
            "laws",
            Bound::Included(b"a".as_slice()),
            Bound::Included(b"b".as_slice()),
            &mut |key, _| {
                through_b.push(key.to_vec());
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(
        through_b,
        vec![b"a".to_vec(), b"a\xff".to_vec(), b"b".to_vec()]
    );
}

/// Point reads return the last written value, unharmed by embedded NULs
/// and non-UTF-8 bytes on either side of the pair.
#[test]
fn point_reads_round_trip_arbitrary_bytes() {
    let path = temp_path("roundtrip");
    let store = DefaultStore::create(&path).unwrap();
    store
        .write(|txn| {
            txn.put("laws", b"k\x00\x01\xff", &[0u8, 255, 0, 1])?;
            Ok(())
        })
        .unwrap();
    assert_eq!(
        store.get("laws", b"k\x00\x01\xff").unwrap(),
        Some(vec![0u8, 255, 0, 1])
    );
    assert_eq!(store.get("laws", b"absent").unwrap(), None);
}

/// The empty key and the 511/512-byte boundary: LMDB's typed refusals
/// against the other peers' acceptance, at the documented edges.
#[test]
fn key_length_edges_follow_the_documented_contract() {
    let path = temp_path("key-length");
    let store = DefaultStore::create(&path).unwrap();

    // The closure's own Result is the transaction result: a `put` error
    // (e.g. LMDB's typed refusals) surfaces as `write`'s Err directly.
    let empty = store.write(|txn| txn.put("laws", b"", b"v"));
    if EMPTY_KEY_LEGAL {
        assert!(empty.is_ok(), "the empty key must be storable");
        assert_eq!(store.get("laws", b"").unwrap(), Some(b"v".to_vec()));
    } else {
        assert!(
            matches!(empty, Err(StoreError::InvalidInput(_))),
            "LMDB refuses the empty key with InvalidInput"
        );
    }

    let key_511 = vec![b'k'; 511];
    store
        .write(|txn| txn.put("laws", &key_511, b"edge-511"))
        .expect("a 511-byte key is legal on every peer");
    assert_eq!(
        store.get("laws", &key_511).unwrap(),
        Some(b"edge-511".to_vec())
    );

    let key_512 = vec![b'k'; 512];
    let over = store.write(|txn| txn.put("laws", &key_512, b"edge-512"));
    if LONG_KEY_LEGAL {
        assert!(over.is_ok(), "a 512-byte key is legal outside LMDB");
        assert_eq!(
            store.get("laws", &key_512).unwrap(),
            Some(b"edge-512".to_vec())
        );
    } else {
        assert!(
            matches!(over, Err(StoreError::InvalidInput(_))),
            "LMDB refuses a 512-byte key with InvalidInput"
        );
    }
}

/// Rows written through one handle read back identically through a
/// fresh read-only handle over the same file — the drop-in consumer's
/// exact shape (datagen writes, the runtime reads).
#[test]
fn rows_survive_reopen_through_a_read_only_handle() {
    let path = temp_path("reopen");
    {
        let store = DefaultStore::create(&path).unwrap();
        seeded_keys(&store);
    }
    let reader = DefaultStore::open_read_only(&path).unwrap();
    let mut seen = Vec::new();
    reader
        .for_each("laws", &mut |key, value| {
            seen.push((key.to_vec(), value.to_vec()));
            Ok(())
        })
        .unwrap();
    let mut expected = vec![
        (b"a".to_vec(), b"v-a".to_vec()),
        (b"a\xff".to_vec(), b"v-high".to_vec()),
        (b"b".to_vec(), b"v-b".to_vec()),
    ];
    if EMPTY_KEY_LEGAL {
        expected.insert(0, (Vec::new(), b"v-empty".to_vec()));
    }
    assert_eq!(seen, expected);
    assert_eq!(
        reader.get("laws", b"a\xff").unwrap(),
        Some(b"v-high".to_vec())
    );
}

/// A transaction sees its own writes, and an `Err` out of the closure
/// rolls the whole transaction back — `WriteTxn`'s atomicity contract.
#[test]
fn transactions_see_their_writes_and_roll_back_on_err() {
    let path = temp_path("txn");
    let store = DefaultStore::create(&path).unwrap();

    // Read-your-writes inside the closure.
    store
        .write(|txn| {
            txn.put("laws", b"ryw", b"1")?;
            assert_eq!(txn.get("laws", b"ryw")?, Some(b"1".to_vec()));
            Ok(())
        })
        .unwrap();

    // The whole transaction unwinds when the closure fails, including
    // writes that preceded the failure.
    let blown: Result<(), StoreError> = store.write(|txn| {
        txn.put("laws", b"doomed", b"1")?;
        txn.put("laws", b"also-doomed", b"2")?;
        Err(StoreError::InvalidInput("deliberate failure"))
    });
    assert!(blown.is_err());
    assert_eq!(store.get("laws", b"ryw").unwrap(), Some(b"1".to_vec()));
    assert_eq!(store.get("laws", b"doomed").unwrap(), None);
    assert_eq!(store.get("laws", b"also-doomed").unwrap(), None);

    // Removing an absent key is a no-op, not an error.
    store
        .write(|txn| txn.remove("laws", b"never-there"))
        .expect("removing an absent key must succeed");
}

/// A table no transaction ever touched reads as empty everywhere —
/// absent is a legal steady state, not an error.
#[test]
fn an_untouched_table_reads_empty() {
    let path = temp_path("absent-table");
    let store = DefaultStore::create(&path).unwrap();
    assert_eq!(store.get("never-touched", b"k").unwrap(), None);
    assert!(store.is_empty("never-touched").unwrap());
    let mut rows = 0;
    store
        .for_each("never-touched", &mut |_, _| {
            rows += 1;
            Ok(())
        })
        .unwrap();
    assert_eq!(rows, 0);
}
