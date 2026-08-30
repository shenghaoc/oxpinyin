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
//! Commits use `hard = 0` and then flush: durable against a process
//! crash, not against power loss.
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

use crate::{ReadStore, StoreError, Visitor, WriteStore, WriteTxn, validate_table_name};

use ffi::{Db, DbType};

/// The framing separator between a table name and a key.
///
/// Table names are validated NUL-free, so no framed prefix is a prefix of
/// another: every table is a contiguous run of the tree whose internal
/// order is the caller's key order.
const SEPARATOR: u8 = 0;

/// `table || 0x00 || key`.
fn frame(table: &str, key: &[u8]) -> Vec<u8> {
    let mut framed = Vec::with_capacity(table.len() + 1 + key.len());
    framed.extend_from_slice(table.as_bytes());
    framed.push(SEPARATOR);
    framed.extend_from_slice(key);
    framed
}

/// `table || 0x00` — the prefix every one of `table`'s rows carries.
fn prefix(table: &str) -> Vec<u8> {
    frame(table, &[])
}

/// The caller's key inside a framed one, or `None` if the framed key
/// belongs to another table.
fn unframe<'a>(prefix: &[u8], framed: &'a [u8]) -> Option<&'a [u8]> {
    framed.strip_prefix(prefix)
}

/// Whether `key` falls inside `[lo, hi]`.
fn in_bounds(key: &[u8], lo: Bound<&[u8]>, hi: Bound<&[u8]>) -> bool {
    let above = match lo {
        Bound::Unbounded => true,
        Bound::Included(bound) => key >= bound,
        Bound::Excluded(bound) => key > bound,
    };
    let below = match hi {
        Bound::Unbounded => true,
        Bound::Included(bound) => key <= bound,
        Bound::Excluded(bound) => key < bound,
    };
    above && below
}

/// A Kyoto Cabinet `TreeDB` store implementing both capability tiers.
pub struct KcStore {
    db: Db,
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

impl WriteStore for KcStore {
    fn create(path: &Path) -> Result<Self, StoreError> {
        Ok(Self {
            db: Db::open(path, DbType::Tree, false, true)?,
        })
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
                // Push the commit to the operating system, so a reader —
                // including the user's own libpinyin — sees it.
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
        // durable is the honest implementation.
        self.db.sync(false)
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
