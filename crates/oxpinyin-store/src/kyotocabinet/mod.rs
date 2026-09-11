//! Kyoto Cabinet backend for the store's peer set.
//!
//! Enabled by the `kyotocabinet` cargo feature and — as the workspace's
//! default selected backend — compiled in on a plain `cargo build`.
//! [`KcStore`] is a `TreeDB` implementation of the store's two capability
//! tiers, addressed by `(table, key)` like every other backend.
//!
//! # Why every open carries `#type=`
//!
//! The C API is `PolyDB` and picks the database class from the path
//! suffix, failing outright on an unrecognised one (`kclangc.h:312-320`).
//! oxpinyin names its native tables `.kct` (the Kyoto Cabinet TreeDB
//! convention), so the suffix already tells `PolyDB` what to open — but
//! [`ffi::Db::open`] passes `#type=` explicitly anyway so the class is
//! never guessed from a filename that came in from a caller.
//!
//! # Key ordering
//!
//! `TreeDB` with no `rcomp` tuning parameter uses Kyoto Cabinet's default
//! record comparator, `LEXICALCOMP` — byte-wise, shorter key first on a
//! shared prefix. The cross-backend conformance tests in `super` assert
//! this backend walks identically to redb, LMDB and the others over keys
//! that cross 256 in the first and in a later element — where byte order
//! and integer order genuinely differ.
//!
//! # Atomicity
//!
//! Kyoto Cabinet gives a standalone handle real transactions
//! (`kcdbbegintran`/`kcdbendtran`), so [`WriteStore::write`] is the
//! library's own transaction rather than a buffered imitation: the
//! closure's writes go straight to the database inside the transaction,
//! reads see them, and an `Err` rolls the whole thing back.
//!
//! Commits end with `kcdbsync(hard = 0)`: the commit is pushed to the
//! operating system, so another process — or another handle in this one
//! — observes it and it survives a process crash, but it is not yet on
//! the device. [`WriteStore::compact`] is the hard `kcdbsync(hard = 1)`,
//! and `UserStore::save` (the `pinyin_save` path) calls it, so the
//! user's data reaches stable storage where the consumer asks for it.
//! The tkrzw backend documents the same split and the measurement
//! behind it (`docs/findings/perf-train-commit-fsync-2026-09-09.md`).
//!
//! A power cut *during* the commit itself can still tear the
//! transaction — TreeDB writes through no write-ahead log — the same
//! residual the tkrzw backend documents; redb and LMDB alone roll a
//! torn commit back on the next open.
//!
//! # Threading
//!
//! [`KcStore`] is `Send + Sync`: Kyoto Cabinet's `PolyDB` carries its own
//! locking (every access method takes the database's rwlock), and the
//! `unsafe impl`s on the FFI handle record exactly that contract — see the
//! SAFETY comment in `ffi.rs`. This is required for a peer backend that
//! also serves as the default selection: the user-store registry holds
//! `DefaultStore` behind a `static Mutex`, and the runtime compile-asserts
//! its handles `Send + Sync`.
#![allow(unsafe_code)]

mod ffi;

use std::ops::Bound;
use std::path::Path;

use crate::common::{frame, in_bounds, prefix, unframe};
use crate::{ReadStore, StoreError, Visitor, WriteStore, WriteTxn, validate_table_name};

use ffi::{Db, DbType};

/// A Kyoto Cabinet `TreeDB` store implementing both capability tiers.
pub struct KcStore {
    db: Db,
}

/// Hidden bench entry points.
///
/// Reserved for measurement work in this crate's `benches/` directory;
/// the `bench-internal` feature is off by default and no shipping
/// profile enables it. The tuning knob these open up is what
/// shenghaoc/oxpinyin#402's time-side measurement bench uses to
/// characterise a Kyoto Cabinet default for an upstream report; the
/// shipping open path itself matches libpinyin's untuned open
/// byte-for-byte and stays that way (see
/// `docs/findings/compatibility-policy.md`).
///
/// Not stable, not documented for external use, not part of any
/// contract this backend keeps. Names live only under this feature so
/// nothing else in the crate — tests, doc builds, the shipping default
/// selection — can even see them.
#[cfg(feature = "bench-internal")]
impl KcStore {
    /// Opens `path` read-only, appending `extra_tuning` after `#type=kct`.
    ///
    /// `extra_tuning` must start with `#` (`#bnum=4096`, `#bnum=4096#pccap=1m`,
    /// …) or be empty. Empty reproduces [`KcStore::open_read_only`]
    /// byte-for-byte.
    #[doc(hidden)]
    pub fn open_read_only_tuned(path: &Path, extra_tuning: &str) -> Result<Self, StoreError> {
        Ok(Self {
            db: Db::open_with_tuning(path, DbType::Tree, true, false, extra_tuning)?,
        })
    }

    /// Same as [`Self::open_read_only_tuned`] but for `HashDB`.
    #[doc(hidden)]
    pub fn open_hash_read_only_tuned(path: &Path, extra_tuning: &str) -> Result<Self, StoreError> {
        Ok(Self {
            db: Db::open_with_tuning(path, DbType::Hash, true, false, extra_tuning)?,
        })
    }

    /// Walks the raw keyspace (no table framing), calling `visit` with each
    /// key. Stops on the first `Err` from `visit`. Used only to sample a
    /// stable set of keys for the `kyotocabinet_bnum` bench; production
    /// walks go through [`crate::RawReadStore::range_raw`].
    #[doc(hidden)]
    pub fn walk_raw_keys(
        &self,
        mut visit: impl FnMut(&[u8]) -> Result<(), StoreError>,
    ) -> Result<(), StoreError> {
        let mut cursor = self.db.cursor()?;
        if !cursor.jump_to(&[])? {
            return Ok(());
        }
        while let Some(record) = cursor.next()? {
            visit(record.key())?;
        }
        Ok(())
    }
}

impl KcStore {
    /// Walks `table`'s rows in ascending key order within `[lo, hi]`.
    ///
    /// One cursor, positioned once with `kccurjumpkey` and advanced by
    /// `kccurget`'s own step, stopping at the first key outside the
    /// table's framed prefix — so a scan costs the rows it returns and one
    /// more.
    fn walk(
        &self,
        table: &str,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        visit: &mut Visitor<'_>,
    ) -> Result<(), StoreError> {
        validate_table_name(table)?;
        let table_prefix = prefix(table);
        let start = match lo {
            Bound::Unbounded => table_prefix.clone(),
            Bound::Included(key) | Bound::Excluded(key) => frame(table, key),
        };
        let mut cursor = self.db.cursor()?;
        if !cursor.jump_to(&start)? {
            return Ok(());
        }
        while let Some(record) = cursor.next()? {
            let Some(key) = unframe(&table_prefix, record.key()) else {
                // Past the last row of this table.
                return Ok(());
            };
            if matches!(lo, Bound::Excluded(bound) if key == bound) {
                continue;
            }
            if !in_bounds(key, Bound::Unbounded, hi) {
                return Ok(());
            }
            visit(key, record.value())?;
        }
        Ok(())
    }
}

impl ReadStore for KcStore {
    fn open_read_only(path: &Path) -> Result<Self, StoreError> {
        Ok(Self {
            db: Db::open(path, DbType::Tree, true, false)?,
        })
    }

    fn get(&self, table: &str, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError> {
        validate_table_name(table)?;
        Ok(self.db.get(&frame(table, key))?.map(|buf| buf.to_vec()))
    }

    fn range(
        &self,
        table: &str,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        visit: &mut Visitor<'_>,
    ) -> Result<(), StoreError> {
        self.walk(table, lo, hi, visit)
    }

    fn for_each(&self, table: &str, visit: &mut Visitor<'_>) -> Result<(), StoreError> {
        self.walk(table, Bound::Unbounded, Bound::Unbounded, visit)
    }

    fn is_empty(&self, table: &str) -> Result<bool, StoreError> {
        validate_table_name(table)?;
        let table_prefix = prefix(table);
        let mut cursor = self.db.cursor()?;
        if !cursor.jump_to(&table_prefix)? {
            return Ok(true);
        }
        // Stops at the first record rather than scanning the table.
        Ok(match cursor.next()? {
            None => true,
            Some(record) => unframe(&table_prefix, record.key()).is_none(),
        })
    }
}

impl crate::RawReadStore for KcStore {
    fn get_raw(&self, key: &[u8]) -> Result<Option<Vec<u8>>, crate::StoreError> {
        Ok(self.db.get(key)?.map(|buf| buf.to_vec()))
    }

    fn range_raw(
        &self,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        visit: &mut Visitor<'_>,
    ) -> Result<(), crate::StoreError> {
        // The contract is ascending key order, but a cursor over the
        // unordered containers (HashDB, and the StashDB the user bigram
        // loads into) walks in bucket order — jump positions at a hash
        // slot, not at the first key at or above a lower bound. Collect
        // every row, sort by key, and apply the bounds here: the only
        // walk that is correct on all three container classes. TreeDB
        // rows arrive sorted, so the sort is a no-op re-sort there.
        let mut cursor = self.db.cursor()?;
        if !cursor.jump_first()? {
            return Ok(());
        }
        let mut rows: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        while let Some(record) = cursor.next()? {
            rows.push((record.key().to_vec(), record.value().to_vec()));
        }
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        for (key, value) in rows {
            if matches!(lo, Bound::Excluded(bound) if key == *bound) {
                continue;
            }
            if matches!(lo, Bound::Included(bound) if key[..] < *bound) {
                continue;
            }
            if !in_bounds(&key, Bound::Unbounded, hi) {
                return Ok(());
            }
            visit(&key, &value)?;
        }
        Ok(())
    }

    /// The library's own O(1) record count — over the whole file, which
    /// on this backend is exactly the raw keyspace.
    fn count_raw(&self) -> Result<u64, crate::StoreError> {
        self.db.count()
    }

    fn open_hash_read_only(path: &Path) -> Result<Self, crate::StoreError> {
        Ok(Self {
            db: Db::open(path, DbType::Hash, true, false)?,
        })
    }

    fn open_user_bigram(path: &Path) -> Result<Self, crate::StoreError> {
        // Kyoto Cabinet's user bigram is a snapshot stream, not a hash
        // file (`ngram_kyotodb.cpp:54-64`): open an in-memory stash and
        // load the snapshot into it. The system bigram stays a HashDB file
        // through `open_hash_read_only` above — the two are different
        // containers and must not share one open.
        let db = Db::open_stash()?;
        db.load_snapshot(path)?;
        // The seam is read-only even though the stash class opens
        // writer: seal it so a stray `write` cannot mutate memory that
        // no dump would ever persist.
        Ok(Self {
            db: db.into_read_only(),
        })
    }
}

impl WriteStore for KcStore {
    fn create(path: &Path) -> Result<Self, StoreError> {
        Ok(Self {
            db: Db::open(path, DbType::Tree, false, true)?,
        })
    }

    fn create_hash(path: &Path) -> Result<Self, StoreError> {
        Ok(Self {
            db: Db::open(path, DbType::Hash, false, true)?,
        })
    }

    fn write_user_bigram(path: &Path, rows: &[(Vec<u8>, Vec<u8>)]) -> Result<(), StoreError> {
        // The inverse of `open_user_bigram`, mirroring the pin's
        // `Bigram::save_db` (`ngram_kyotodb.cpp:82-101`) — unlink then
        // dump — with the unlink deferred into an atomic rename: the
        // snapshot is dumped to a sibling temporary first, so a failure
        // anywhere before the rename leaves the previous profile intact
        // instead of destroyed.
        let tmp = crate::sibling_temp(path);
        let db = Db::open_stash()?;
        for (key, value) in rows {
            db.set(key, value)?;
        }
        db.dump_snapshot(&tmp)?;
        std::fs::rename(&tmp, path).map_err(StoreError::Io)?;
        Ok(())
    }

    fn write<R>(
        &self,
        f: impl FnOnce(&mut dyn WriteTxn) -> Result<R, StoreError>,
    ) -> Result<R, StoreError> {
        // A read-only store refuses the whole transaction, not merely the
        // writes inside it: the tier contract is that `write` on a
        // read-only handle is `ReadOnly` even when the closure writes
        // nothing.
        if self.db.is_read_only() {
            return Err(StoreError::ReadOnly);
        }
        // Atomicity here is committer-side only: Kyoto Cabinet does not
        // isolate readers from this transaction. `begin_transaction`
        // releases its writer lock once the transaction is under way, and
        // the writes land in the live database before `end_transaction`
        // commits or rolls them back, so a concurrent reader can observe
        // intermediate values — including ones a rollback then removes.
        self.db.begin_transaction()?;
        let mut txn = KcTxn { store: self };
        match f(&mut txn) {
            Ok(out) => {
                self.db.end_transaction(true)?;
                // Push the commit to the operating system (`hard = 0`),
                // so another process — or another handle in this one —
                // reading the database observes it, and a process crash
                // cannot lose it. `compact` is the hard sync; see the
                // module docs for why the device-level sync sits there.
                self.db.sync(false)?;
                Ok(out)
            }
            Err(error) => {
                // The rollback's own failure must not mask the reason the
                // closure failed, which is what the caller needs to see.
                let _ = self.db.end_transaction(false);
                Err(error)
            }
        }
    }

    fn compact(&mut self) -> Result<(), StoreError> {
        if self.db.is_read_only() {
            return Err(StoreError::ReadOnly);
        }
        // Kyoto Cabinet reuses freed regions in place through its own free
        // block pool; there is no in-place rewrite that does not go
        // through a copy of the whole file, which is not what the other
        // backends' `compact` does either (LMDB's successful `compact`
        // also does not shrink the file). Making the current state
        // durable on the device is the honest implementation — and it is
        // the store's one stable-storage point, which `UserStore::save`
        // reaches through `pinyin_save`.
        self.db.sync(true)
    }
}

/// A write transaction — the library's own, not a buffer.
///
/// Every method writes straight through to the database inside Kyoto
/// Cabinet's transaction, so read-your-writes is the database's semantics
/// rather than something this type has to emulate, and rollback is the
/// library's.
struct KcTxn<'store> {
    store: &'store KcStore,
}

impl WriteTxn for KcTxn<'_> {
    fn get(&self, table: &str, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError> {
        self.store.get(table, key)
    }

    fn put(&mut self, table: &str, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        validate_table_name(table)?;
        self.store.db.set(&frame(table, key), value)
    }

    fn put_raw(&mut self, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        // The file's bare keyspace, no table-name framing — what
        // `get_raw`/`range_raw` read back on this backend.
        self.store.db.set(key, value)
    }

    fn remove(&mut self, table: &str, key: &[u8]) -> Result<(), StoreError> {
        validate_table_name(table)?;
        self.store.db.remove(&frame(table, key))
    }

    fn range(
        &self,
        table: &str,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        visit: &mut Visitor<'_>,
    ) -> Result<(), StoreError> {
        // A cursor inside the transaction sees the transaction's writes,
        // so the walk needs no merge pass. It does need the rows
        // collected first: the visitor may itself call back into the
        // store, and holding a cursor across that is what the borrow of
        // `self.store.db` would otherwise allow.
        let mut rows: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        self.store.walk(table, lo, hi, &mut |key, value| {
            rows.push((key.to_vec(), value.to_vec()));
            Ok(())
        })?;
        for (key, value) in &rows {
            visit(key, value)?;
        }
        Ok(())
    }

    fn for_each(&self, table: &str, visit: &mut Visitor<'_>) -> Result<(), StoreError> {
        self.range(table, Bound::Unbounded, Bound::Unbounded, visit)
    }

    fn is_empty(&self, table: &str) -> Result<bool, StoreError> {
        self.store.is_empty(table)
    }
}
