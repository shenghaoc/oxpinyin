//! Kyoto Cabinet backend — the libpinyin compatibility route for
//! distributions that build libpinyin `--with-dbm=KyotoCabinet`.
//!
//! Enabled by the `kyotocabinet` cargo feature. Two things live here, and
//! keeping them apart matters:
//!
//! * [`KcStore`] — a `TreeDB` implementation of the store's two capability
//!   tiers, addressed by `(table, key)` like every other backend.
//! * [`BigramDb`] — raw `HashDB` access to libpinyin's own `bigram.db`,
//!   keyed by the four native-endian bytes of a `phrase_token_t` and
//!   valued by a [`SingleGram`] chunk. Deliberately **not** a
//!   [`ReadStore`]: a hash database has no ordering, so it cannot honour
//!   `range` or an ordered `for_each`.
//!
//! Which class libpinyin uses for what is read from its source, not
//! assumed: `ngram_kyotodb.cpp:115` is `m_db = new HashDB`, while
//! `phrase_large_table3_kyotodb.cpp:102` and
//! `chewing_large_table2_kyotodb.cpp:87` are `new TreeDB`.
//!
//! # The filename problem, and why every open carries `#type=`
//!
//! libpinyin's file names are compile-time constants that do **not** vary
//! with the DBM backend: `SYSTEM_BIGRAM "bigram.db"`, `USER_BIGRAM
//! "user_bigram.db"` (`src/pinyin_internal.h:56-58`). A
//! Kyoto-Cabinet-built libpinyin therefore ships no `.kch` or `.kct` file
//! anywhere — the extensions are Kyoto Cabinet's own convention, not
//! libpinyin's.
//!
//! That collides with the C API, which is `PolyDB` and picks the database
//! class from the path suffix, failing outright on an unrecognised one
//! (`kclangc.h:312-320`). Measured on 1.2.80: `kcdbopen(db, "bigram.db",
//! KCOWRITER|KCOCREATE)` fails with `invalid operation`, and the same
//! path with `#type=kch` appended succeeds. `PolyDB`'s tuning parameters
//! (`kcpolydb.h:496-515`) are the escape hatch, and [`ffi::Db::open`]
//! always uses them. Detection by extension would find nothing; detection
//! is by file magic instead (`oxpinyin_data::layout`).
//!
//! # Key ordering
//!
//! `TreeDB` with no `rcomp` tuning parameter uses Kyoto Cabinet's default
//! record comparator, `LEXICALCOMP` — byte-wise, shorter key first on a
//! shared prefix. libpinyin sets no comparator
//! (`phrase_large_table3_kyotodb.cpp`, `chewing_large_table2_kyotodb.cpp`
//! contain no `rcomp`), so that default applies, and it is the store's one
//! rule exactly. The cross-backend conformance tests in `super` assert
//! this backend walks identically to redb, LMDB and the others over keys
//! that cross 256 in the first and in a later element — where byte order
//! and integer order genuinely differ.
//!
//! # Atomicity
//!
//! Stronger than the tkrzw and Berkeley DB backends, and worth stating
//! because it is a real difference rather than a matter of style. Kyoto
//! Cabinet gives a standalone handle real transactions
//! (`kcdbbegintran`/`kcdbendtran`), so [`WriteStore::write`] is the
//! library's own transaction rather than a buffered imitation: the
//! closure's writes go straight to the database inside the transaction,
//! reads see them, and an `Err` rolls the whole thing back. The Berkeley
//! DB backend has to buffer precisely because libpinyin's
//! environment-less opens leave it no transaction to use.
//!
//! Commits use `hard = 0` and then flush: durable against a process
//! crash, not against power loss.
//!
//! # Threading
//!
//! [`KcStore`] is `Send + Sync`: Kyoto Cabinet's `PolyDB` carries its own
//! locking (every access method takes the database's rwlock), and the
//! `unsafe impl`s on the FFI handle record exactly that contract — see the
//! SAFETY comment in `ffi.rs`. This is required, not optional, for a
//! default backend: the user-store registry holds `DefaultStore` behind a
//! `static Mutex`, and the runtime compile-asserts its handles
//! `Send + Sync`.
#![allow(unsafe_code)]

mod ffi;
/// Re-export: the chunk format is backend-independent (`ngram.cpp` is
/// unconditional upstream), so it lives at the crate root and the tkrzw
/// compat reader shares it; this path is kept for existing consumers.
pub use crate::single_gram;

use std::collections::BTreeSet;
use std::ops::Bound;
use std::path::Path;

use crate::{ReadStore, StoreError, Visitor, WriteStore, WriteTxn, validate_table_name};

pub use crate::single_gram::SingleGram;

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

// ── libpinyin's own bigram.db ──────────────────────────────────────

/// libpinyin's `bigram.db` as a Kyoto-Cabinet-built libpinyin writes it:
/// a `HashDB` keyed by the four native-endian bytes of a
/// `phrase_token_t`, valued by a whole [`SingleGram`] chunk.
///
/// The key encoding is read from the pin, not assumed:
/// `ngram_kyotodb.cpp:128` is `const char * kbuf = (char *) &index;` with
/// `sizeof(phrase_token_t)` — byte for byte what the Berkeley DB backend
/// does at `ngram_bdb.cpp`. The *logical* format is backend-independent;
/// only the physical container differs.
///
/// Blob-per-previous-token, which is upstream's model: every successor of
/// `prev` lives in one value and every access is a point operation. There
/// is no ordering contract — a hash database has no order — which is why
/// this type is not a [`ReadStore`] and offers no `range`.
///
/// The same format serves the system `bigram.db` and the user's
/// `user_bigram.db`; only the open mode differs.
pub struct BigramDb {
    db: Db,
}

impl BigramDb {
    /// Opens a `bigram.db` as `HashDB`, as `ngram_kyotodb.cpp:115` does.
    ///
    /// A read-only open never creates; a writable open creates when the
    /// file is absent, which is how a user's first training run makes one.
    ///
    /// # Errors
    ///
    /// [`StoreError`] when Kyoto Cabinet refuses the open — a missing file
    /// for a read-only open, a file that is not a hash database, or an
    /// unsupported library version.
    pub fn open(path: &Path, read_only: bool) -> Result<Self, StoreError> {
        Ok(Self {
            db: Db::open(path, DbType::Hash, read_only, !read_only)?,
        })
    }

    /// The four raw bytes libpinyin uses as a key.
    fn key(prev: u32) -> [u8; 4] {
        prev.to_ne_bytes()
    }

    /// The gram stored for `prev`, or `None`.
    ///
    /// # Errors
    ///
    /// [`StoreError`] from Kyoto Cabinet, or from a chunk that does not
    /// satisfy the layout invariants ([`SingleGram::decode`]).
    pub fn get(&self, prev: u32) -> Result<Option<SingleGram>, StoreError> {
        match self.db.get(&Self::key(prev))? {
            None => Ok(None),
            Some(bytes) => SingleGram::decode(&bytes).map(Some),
        }
    }

    /// The chunk stored for `prev`, undecoded.
    ///
    /// The compatibility gate compares what [`SingleGram::encode`]
    /// produces against the bytes already in the file; ordinary callers
    /// want [`Self::get`].
    ///
    /// # Errors
    ///
    /// [`StoreError`] from Kyoto Cabinet.
    pub fn raw(&self, prev: u32) -> Result<Option<Vec<u8>>, StoreError> {
        Ok(self.db.get(&Self::key(prev))?.map(|buf| buf.to_vec()))
    }

    /// Stores `gram` under `prev`, in libpinyin's byte layout.
    ///
    /// # Errors
    ///
    /// [`StoreError`] from Kyoto Cabinet, or [`StoreError::ReadOnly`].
    pub fn put(&self, prev: u32, gram: &SingleGram) -> Result<(), StoreError> {
        self.db.set(&Self::key(prev), &gram.encode())
    }

    /// Removes `prev`'s gram; absent is not an error.
    ///
    /// # Errors
    ///
    /// [`StoreError`] from Kyoto Cabinet, or [`StoreError::ReadOnly`].
    pub fn remove(&self, prev: u32) -> Result<(), StoreError> {
        self.db.remove(&Self::key(prev))
    }

    /// Number of records, which Kyoto Cabinet tracks rather than counts.
    ///
    /// # Errors
    ///
    /// [`StoreError`] from Kyoto Cabinet.
    pub fn len(&self) -> Result<u64, StoreError> {
        self.db.count()
    }

    /// Whether the database holds no records.
    ///
    /// # Errors
    ///
    /// [`StoreError`] from Kyoto Cabinet.
    pub fn is_empty(&self) -> Result<bool, StoreError> {
        Ok(self.len()? == 0)
    }

    /// Flushes to the operating system.
    ///
    /// # Errors
    ///
    /// [`StoreError`] from Kyoto Cabinet.
    pub fn sync(&self) -> Result<(), StoreError> {
        self.db.sync(false)
    }

    /// Visits every `(prev, gram)` record.
    ///
    /// The order is the hash database's, which is not key order and
    /// carries no contract — callers needing an order impose their own.
    ///
    /// # Errors
    ///
    /// [`StoreError`] from Kyoto Cabinet, from a malformed key, from a
    /// chunk that fails [`SingleGram::decode`], or whatever `visit`
    /// returns.
    pub fn for_each(
        &self,
        visit: &mut dyn FnMut(u32, &SingleGram) -> Result<(), StoreError>,
    ) -> Result<(), StoreError> {
        let mut cursor = self.db.cursor()?;
        if !cursor.jump()? {
            return Ok(());
        }
        while let Some(record) = cursor.next()? {
            let key: [u8; 4] = record.key().try_into().map_err(|_| {
                StoreError::Backend(
                    format!(
                        "corrupt bigram key: {} bytes, expected the 4 of a phrase_token_t",
                        record.key().len()
                    )
                    .into(),
                )
            })?;
            let gram = SingleGram::decode(record.value())?;
            visit(u32::from_ne_bytes(key), &gram)?;
        }
        Ok(())
    }

    /// Every `prev` token in the database, ascending.
    ///
    /// The hash database has no order of its own, so this imposes one —
    /// which is what makes a Kyoto Cabinet file and a Berkeley DB file
    /// comparable record for record.
    ///
    /// # Errors
    ///
    /// As [`Self::for_each`].
    pub fn tokens(&self) -> Result<Vec<u32>, StoreError> {
        let mut tokens = BTreeSet::new();
        self.for_each(&mut |prev, _| {
            tokens.insert(prev);
            Ok(())
        })?;
        Ok(tokens.into_iter().collect())
    }
}
