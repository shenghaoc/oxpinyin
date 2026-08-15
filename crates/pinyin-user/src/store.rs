//! redb-backed integer store for user-learning counts.
//!
//! Models the *values* libpinyin records on candidate selection — the user
//! bigram counts (and per-predecessor totals) and the phrase-index unigram
//! deltas — not its MemoryChunk / DBM byte layout
//! (`docs/findings/user-store.md` §4, §10: the same value-not-format bypass as
//! the system store). All counts are `u64` integers.
//!
//! T1 scope: the schema, the seed-driven update, and value read-back. The save
//! cycle (T5), the user phrase index and token allocation (T2), Session/capi
//! wiring (T3) and the decode-time merge (T4) are out of scope.

use std::fmt;
use std::path::Path;

use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};

use crate::seed;

/// Token type — libpinyin's 32-bit `phrase_token_t`.
pub type Token = u32;

/// `sentence_start` sentinel: the predecessor of the first phrase in a
/// sentence (`docs/findings/user-store.md` §2; `novel_types.h:122`).
pub const SENTENCE_START: Token = 1;

/// User bigram counts: `(prev, cur) -> count`.
const BIGRAM: TableDefinition<(Token, Token), u64> = TableDefinition::new("user_bigram");

/// Per-predecessor bigram totals: `prev -> total`.
const BIGRAM_TOTAL: TableDefinition<Token, u64> = TableDefinition::new("user_bigram_total");

/// Phrase-index unigram deltas: `token -> delta`.
const UNIGRAM: TableDefinition<Token, u64> = TableDefinition::new("user_unigram");

/// Which seed rule an update applies.
#[derive(Clone, Copy)]
enum SeedPolicy {
    /// Reselection-expansion rule (`pinyin_train`, §2).
    Training,
    /// Flat `INITIAL_SEED` (`pinyin_choose_predicted_candidate`, §2).
    Predicted,
}

/// Errors from opening or updating the user store.
#[derive(Debug)]
pub enum UserStoreError {
    /// The redb file could not be opened (I/O error).
    Io(std::io::Error),
    /// redb reported a database-level error.
    Db(redb::DatabaseError),
    /// redb reported a table-level error.
    Table(redb::TableError),
    /// redb reported a transaction-level error.
    Transaction(redb::TransactionError),
    /// redb reported a commit error.
    Commit(redb::CommitError),
    /// redb reported a storage-level error.
    Storage(redb::StorageError),
}

impl fmt::Display for UserStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Db(e) => write!(f, "database error: {e}"),
            Self::Table(e) => write!(f, "table error: {e}"),
            Self::Transaction(e) => write!(f, "transaction error: {e}"),
            Self::Commit(e) => write!(f, "commit error: {e}"),
            Self::Storage(e) => write!(f, "storage error: {e}"),
        }
    }
}

impl std::error::Error for UserStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Db(e) => Some(e),
            Self::Table(e) => Some(e),
            Self::Transaction(e) => Some(e),
            Self::Commit(e) => Some(e),
            Self::Storage(e) => Some(e),
        }
    }
}

impl From<redb::DatabaseError> for UserStoreError {
    fn from(e: redb::DatabaseError) -> Self {
        Self::Db(e)
    }
}

impl From<redb::TableError> for UserStoreError {
    fn from(e: redb::TableError) -> Self {
        Self::Table(e)
    }
}

impl From<redb::TransactionError> for UserStoreError {
    fn from(e: redb::TransactionError) -> Self {
        Self::Transaction(e)
    }
}

impl From<redb::CommitError> for UserStoreError {
    fn from(e: redb::CommitError) -> Self {
        Self::Commit(e)
    }
}

impl From<redb::StorageError> for UserStoreError {
    fn from(e: redb::StorageError) -> Self {
        Self::Storage(e)
    }
}

/// A redb-backed store of user-learning counts.
pub struct UserStore {
    db: Database,
}

impl fmt::Debug for UserStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UserStore").finish_non_exhaustive()
    }
}

impl UserStore {
    /// Open the user store at `path`, creating an empty database if absent.
    ///
    /// The three tables are created eagerly so that reads issued before any
    /// write succeed with zero counts rather than a "table does not exist"
    /// error.
    pub fn open(path: &Path) -> Result<Self, UserStoreError> {
        let db = Database::create(path).map_err(|e| match e {
            redb::DatabaseError::Storage(redb::StorageError::Io(io)) => UserStoreError::Io(io),
            other => UserStoreError::Db(other),
        })?;
        let txn = db.begin_write()?;
        {
            txn.open_table(BIGRAM)?;
            txn.open_table(BIGRAM_TOTAL)?;
            txn.open_table(UNIGRAM)?;
        }
        txn.commit()?;
        Ok(Self { db })
    }

    /// Stored bigram count for `(prev, cur)`; `0` if unrecorded.
    pub fn bigram_count(&self, prev: Token, cur: Token) -> Result<u64, UserStoreError> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(BIGRAM)?;
        Ok(table.get((prev, cur))?.map_or(0, |g| g.value()))
    }

    /// Total bigram mass recorded after `prev`; `0` if none.
    pub fn bigram_total(&self, prev: Token) -> Result<u64, UserStoreError> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(BIGRAM_TOTAL)?;
        Ok(table.get(prev)?.map_or(0, |g| g.value()))
    }

    /// Accumulated phrase-index unigram delta for `token`; `0` if none.
    pub fn unigram_delta(&self, token: Token) -> Result<u64, UserStoreError> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(UNIGRAM)?;
        Ok(table.get(token)?.map_or(0, |g| g.value()))
    }

    /// Record a training selection of `cur` after `last` (the `pinyin_train`
    /// path, §2). Returns the seed applied.
    pub fn observe_selection(&mut self, last: Token, cur: Token) -> Result<u64, UserStoreError> {
        self.update(last, cur, SeedPolicy::Training)
    }

    /// Record an accepted *predicted* candidate `cur` after `last` (the
    /// `pinyin_choose_predicted_candidate` path, §2). Flat `+69` seed. Returns
    /// the seed applied.
    pub fn observe_predicted(&mut self, last: Token, cur: Token) -> Result<u64, UserStoreError> {
        self.update(last, cur, SeedPolicy::Predicted)
    }

    /// Single atomic update: compute the seed under `policy`, then raise the
    /// bigram count for `(last, cur)` and `last`'s total by the seed, and
    /// `cur`'s unigram delta by `seed * 7`. Additions saturate so no input can
    /// panic (constitution §4).
    fn update(
        &mut self,
        last: Token,
        cur: Token,
        policy: SeedPolicy,
    ) -> Result<u64, UserStoreError> {
        let txn = self.db.begin_write()?;
        let seed = {
            let mut bigram = txn.open_table(BIGRAM)?;
            let prev = bigram.get((last, cur))?.map_or(0, |g| g.value());
            let seed = match policy {
                SeedPolicy::Training => seed::training_seed((prev != 0).then_some(prev)),
                SeedPolicy::Predicted => seed::predicted_seed(),
            };
            bigram.insert((last, cur), prev.saturating_add(seed))?;

            let mut total = txn.open_table(BIGRAM_TOTAL)?;
            let prev_total = total.get(last)?.map_or(0, |g| g.value());
            total.insert(last, prev_total.saturating_add(seed))?;

            let mut unigram = txn.open_table(UNIGRAM)?;
            let prev_unigram = unigram.get(cur)?.map_or(0, |g| g.value());
            unigram.insert(cur, prev_unigram.saturating_add(seed::unigram_delta(seed)))?;

            seed
        };
        txn.commit()?;
        Ok(seed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(tag: &str) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("pinyin-user-{tag}-{}.redb", std::process::id()));
        let _ = std::fs::remove_file(&path);
        path
    }

    #[test]
    fn open_creates_empty_store() {
        let path = temp_path("empty");
        let store = UserStore::open(&path).unwrap();
        assert_eq!(store.bigram_count(1, 2).unwrap(), 0);
        assert_eq!(store.bigram_total(1).unwrap(), 0);
        assert_eq!(store.unigram_delta(2).unwrap(), 0);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn observe_applies_pinned_seed_sequence() {
        let path = temp_path("seq");
        let mut store = UserStore::open(&path).unwrap();

        // First selection: seed 69.
        assert_eq!(store.observe_selection(1, 100).unwrap(), 69);
        assert_eq!(store.bigram_count(1, 100).unwrap(), 69);
        assert_eq!(store.bigram_total(1).unwrap(), 69);
        assert_eq!(store.unigram_delta(100).unwrap(), 483); // 69 * 7

        // Second selection of the same pair: seed 138, count 69 + 138 = 207.
        assert_eq!(store.observe_selection(1, 100).unwrap(), 138);
        assert_eq!(store.bigram_count(1, 100).unwrap(), 207);
        assert_eq!(store.bigram_total(1).unwrap(), 207);
        assert_eq!(store.unigram_delta(100).unwrap(), 483 + 966); // + 138 * 7

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn totals_accumulate_per_predecessor() {
        let path = temp_path("totals");
        let mut store = UserStore::open(&path).unwrap();
        // Two distinct successors of the same predecessor: total is the sum of
        // both first-selection seeds.
        assert_eq!(store.observe_selection(5, 10).unwrap(), 69);
        assert_eq!(store.observe_selection(5, 11).unwrap(), 69);
        assert_eq!(store.bigram_count(5, 10).unwrap(), 69);
        assert_eq!(store.bigram_count(5, 11).unwrap(), 69);
        assert_eq!(store.bigram_total(5).unwrap(), 138);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn predicted_path_is_flat_69() {
        let path = temp_path("pred");
        let mut store = UserStore::open(&path).unwrap();
        assert_eq!(store.observe_predicted(1, 200).unwrap(), 69);
        assert_eq!(store.observe_predicted(1, 200).unwrap(), 69); // still flat
        assert_eq!(store.bigram_count(1, 200).unwrap(), 138); // 69 + 69
        assert_eq!(store.bigram_total(1).unwrap(), 138);
        assert_eq!(store.unigram_delta(200).unwrap(), 483 * 2);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn roundtrip_reopen_reads_identical() {
        let path = temp_path("roundtrip");
        {
            let mut store = UserStore::open(&path).unwrap();
            store.observe_selection(SENTENCE_START, 10).unwrap();
            store.observe_selection(10, 20).unwrap();
            store.observe_selection(10, 20).unwrap();
        } // dropping the store closes the database

        let store = UserStore::open(&path).unwrap();
        assert_eq!(store.bigram_count(SENTENCE_START, 10).unwrap(), 69);
        assert_eq!(store.bigram_count(10, 20).unwrap(), 207); // 69 + 138
        assert_eq!(store.bigram_total(SENTENCE_START).unwrap(), 69);
        assert_eq!(store.bigram_total(10).unwrap(), 207);
        assert_eq!(store.unigram_delta(10).unwrap(), 483);
        assert_eq!(store.unigram_delta(20).unwrap(), 1449); // 483 + 966
        let _ = std::fs::remove_file(&path);
    }
}
