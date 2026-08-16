//! redb-backed integer store for user-learning counts and the user phrase
//! index.
//!
//! Models the *values* libpinyin records — user bigram counts, phrase-index
//! unigram deltas, and user-phrase text/pronunciations — not its MemoryChunk
//! / DBM byte layout (`docs/findings/user-store.md` §4, §10). All counts are
//! `u64` integers.
//!
//! T1: count schema and seed-driven update. T2: user phrase-index tables and
//! `USER_DICTIONARY` token allocation. T3 wires the store into the engine
//! session and the C ABI (in `oxpinyin-engine` / `oxpinyin-capi`). T4 exposes
//! the counts as a [`oxpinyin_core::UserCountDelta`] for the decode-time
//! additive merge. T5 adds the save cycle: the §4 `m_modified` gate and the
//! redb-backed persistence point behind `pinyin_save`.

use std::fmt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use oxpinyin_core::UserCountDelta;
use redb::{Database, ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition};

use crate::phrase::{
    self, ADD_PHRASE_UNIGRAM_FACTOR, DEFAULT_PHRASE_COUNT, FIRST_USER_TOKEN, PinyinKey,
    USER_DICTIONARY, UserPhrase, UserPronunciation, first_library_token, is_user_file_library,
    phrase_index_library_index,
};
use crate::registry::{self, CountSnapshot, RegistryLease, StoreInner};
use crate::seed;

/// Token type — libpinyin's 32-bit `phrase_token_t`.
pub type Token = u32;

/// One §9 export row for a user phrase: the phrase text, one pronunciation's
/// `'`-joined pinyin spelling, and that pronunciation's stored count — the
/// `(phrase, pinyin, count)` triple `pinyin_iterator_get_next_phrase` yields
/// upstream (`docs/findings/user-store.md` §9).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExportedPhrase {
    /// Phrase text (UTF-8).
    pub text: String,
    /// One pronunciation, syllables joined by `'` (e.g. `ni'hao`).
    pub pinyin: String,
    /// The pronunciation's stored count.
    pub count: u64,
}

/// `sentence_start` sentinel: the predecessor of the first phrase in a
/// sentence (`docs/findings/user-store.md` §2; `novel_types.h:122`).
pub const SENTENCE_START: Token = 1;

/// User bigram counts: `(prev, cur) -> count`.
const BIGRAM: TableDefinition<(Token, Token), u64> = TableDefinition::new("user_bigram");

/// Per-predecessor bigram totals: `prev -> total`.
const BIGRAM_TOTAL: TableDefinition<Token, u64> = TableDefinition::new("user_bigram_total");

/// Phrase-index unigram deltas: `token -> delta`.
const UNIGRAM: TableDefinition<Token, u64> = TableDefinition::new("user_unigram");

/// Running sum of every unigram delta: singleton key [`UNIGRAM_TOTAL_KEY`].
const UNIGRAM_TOTAL: TableDefinition<u8, u64> = TableDefinition::new("user_unigram_total");

/// Sole key in [`UNIGRAM_TOTAL`].
const UNIGRAM_TOTAL_KEY: u8 = 0;

/// User phrase text: `token -> phrase`.
const PHRASE: TableDefinition<Token, &str> = TableDefinition::new("user_phrase");

/// Reverse lookup used by the §3.2 "already in this sub-index" merge.
const PHRASE_BY_TEXT: TableDefinition<&str, Token> = TableDefinition::new("user_phrase_by_text");

/// Per-library reverse lookup so network (6) and user (7) can share a text.
const PHRASE_BY_LIB_TEXT: TableDefinition<(u8, &str), Token> =
    TableDefinition::new("user_phrase_by_lib_text");

/// Pronunciations: `(token, encoded key sequence) -> count`.
const PRONUNCIATION: TableDefinition<(Token, &[u8]), u64> =
    TableDefinition::new("user_pronunciation");

/// Persistent allocation cursor: singleton key [`ALLOC_CURSOR`] → next token.
const ALLOC: TableDefinition<u8, Token> = TableDefinition::new("user_phrase_alloc");

/// Sole key in [`ALLOC`].
const ALLOC_CURSOR: u8 = 0;

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
    /// redb reported a compaction error.
    Compaction(redb::CompactionError),
    /// Phrase text is empty, too long, or its key count does not match its
    /// Unicode scalar length (`docs/findings/user-store.md` §3.1–3.2).
    InvalidPhrase,
    /// No remaining token in the [`crate::USER_DICTIONARY`] 24-bit id space.
    TokenSpaceExhausted,
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
            Self::Compaction(e) => write!(f, "compaction error: {e}"),
            Self::InvalidPhrase => {
                write!(f, "invalid phrase (empty, too long, or key count mismatch)")
            }
            Self::TokenSpaceExhausted => {
                write!(f, "USER_DICTIONARY token space exhausted")
            }
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
            Self::Compaction(e) => Some(e),
            Self::InvalidPhrase | Self::TokenSpaceExhausted => None,
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

impl From<redb::CompactionError> for UserStoreError {
    fn from(e: redb::CompactionError) -> Self {
        Self::Compaction(e)
    }
}

/// A redb-backed store of user-learning counts.
///
/// `Clone` shares the underlying database handle (cheap): the C ABI context
/// keeps the canonical store and hands each instance a clone, exactly like
/// the dictionary and language model handles. `redb`'s `Database` handle is
/// not itself `Clone` (redb 4.1.0), so the handle and the §4 `m_modified`
/// flag live on one `Arc`; the `Mutex` serializes the handle because
/// [`Database::compact`] (the `pinyin_save` write side) demands `&mut self`.
/// Clones record dirtiness through their own `&mut self` updates and the
/// context's `pinyin_save` observes it. The C ABI contract is
/// main-thread-only, so the flag uses relaxed ordering.
pub struct UserStore {
    inner: Arc<StoreInner>,
    /// Last field so [`RegistryLease`] drains after this handle's `Arc` dies.
    _lease: RegistryLease,
}

impl Clone for UserStore {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
            _lease: RegistryLease,
        }
    }
}

impl fmt::Debug for UserStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UserStore").finish_non_exhaustive()
    }
}

impl UserStore {
    /// Locks the shared database handle, recovering from a poisoned lock
    /// (constitution §4: nothing here panics, so a poisoned mutex must not
    /// brick the store either).
    fn database(&self) -> MutexGuard<'_, Database> {
        self.inner
            .db
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn count_snapshot(&self) -> MutexGuard<'_, Option<CountSnapshot>> {
        self.inner
            .count_snapshot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn write_generation(&self) -> u64 {
        self.inner.write_generation.load(Ordering::Acquire)
    }

    fn has_user_data(&self) -> bool {
        self.inner.has_user_data.load(Ordering::Acquire)
    }

    /// Opens one read transaction plus the four owned count tables.
    ///
    /// Takes `database()` for the call and releases it before returning; the
    /// caller already holds the snapshot lock, which is the outer half of the
    /// one legal order (snapshot, then db).
    fn build_count_snapshot(&self, generation: u64) -> Result<CountSnapshot, UserStoreError> {
        let db = self.database();
        let txn = db.begin_read()?;
        let unigram = txn.open_table(UNIGRAM)?;
        let unigram_total = txn.open_table(UNIGRAM_TOTAL)?;
        let bigram = txn.open_table(BIGRAM)?;
        let bigram_total = txn.open_table(BIGRAM_TOTAL)?;
        drop(db);
        Ok(CountSnapshot {
            generation,
            unigram,
            unigram_total,
            bigram,
            bigram_total,
            _txn: txn,
        })
    }

    /// Runs `read` against the snapshot for the current write generation,
    /// rebuilding it first when a write has landed since it was cached.
    ///
    /// The snapshot is moved out of the mutex for the call and put back after,
    /// so `read` always receives a live [`CountSnapshot`]. There is
    /// deliberately no "snapshot absent" arm: an arm like that would have to
    /// invent a reading, and the only plausible one — report zero — is exactly
    /// what a populated store must never say.
    fn with_count_snapshot<T>(
        &self,
        read: impl FnOnce(&CountSnapshot) -> Result<T, UserStoreError>,
    ) -> Result<T, UserStoreError> {
        let mut snapshot = self.count_snapshot();
        let generation = self.write_generation();
        let cached = match snapshot.take() {
            Some(cached) if cached.generation == generation => cached,
            // A rebuild that fails leaves the slot empty: the next read
            // retries rather than serving counts from a superseded txn.
            _ => self.build_count_snapshot(generation)?,
        };
        let out = read(&cached);
        *snapshot = Some(cached);
        out
    }

    /// Invalidate cached reads after a committed user-data write.
    ///
    /// Consumes the caller's [`Self::database`] guard and drops it before
    /// touching the snapshot lock. Writers therefore hold db alone and then
    /// snapshot alone, while readers and [`Self::save`] hold snapshot-then-db;
    /// the two orders cannot interleave into a cycle. Taking the guard by
    /// value makes that mechanical instead of a rule each call site has to
    /// remember — forgetting a bare `drop(db)` here would deadlock against a
    /// concurrent `save`.
    fn mark_committed_write(&self, db: MutexGuard<'_, Database>, has_user_data: bool) {
        drop(db);
        *self.count_snapshot() = None;
        self.inner
            .has_user_data
            .store(has_user_data, Ordering::Release);
        self.inner.write_generation.fetch_add(1, Ordering::AcqRel);
    }

    /// Open the user store at `path`, creating an empty database if absent.
    ///
    /// Count tables and phrase-index tables are created eagerly so that reads
    /// issued before any write succeed with zero / `None` rather than a
    /// "table does not exist" error. A missing allocation cursor is
    /// initialised to [`FIRST_USER_TOKEN`]. A freshly opened store is clean:
    /// [`Self::save`] is a no-op until a training update records a change.
    ///
    /// Opening a path that is already open in this process returns a clone of
    /// the live handle (shared counts and shared §4 dirty flag) rather than a
    /// second database handle, which redb refuses.
    pub fn open(path: &Path) -> Result<Self, UserStoreError> {
        // Reuse a live handle for a path already open in this process (redb
        // refuses a second write handle). The registry lock is held across
        // create + insert so two racing opens of a fresh path cannot both
        // create.
        let key = registry::registry_key(path);
        let mut reg = registry::lock_registry();
        if let Some(inner) = reg.get(&key).and_then(|handle| handle.upgrade()) {
            return Ok(Self {
                inner,
                _lease: RegistryLease,
            });
        }

        let db = Database::create(path).map_err(|e| match e {
            redb::DatabaseError::Storage(redb::StorageError::Io(io)) => UserStoreError::Io(io),
            other => UserStoreError::Db(other),
        })?;
        let txn = db.begin_write()?;
        let has_user_data = {
            txn.open_table(BIGRAM)?;
            txn.open_table(BIGRAM_TOTAL)?;
            {
                // Backfill the running total so a T1–T3 store reopened
                // after T4 still reports the sum of its unigram deltas.
                let unigrams = txn.open_table(UNIGRAM)?;
                let mut uni_total = txn.open_table(UNIGRAM_TOTAL)?;
                if uni_total.get(UNIGRAM_TOTAL_KEY)?.is_none() {
                    let mut sum = 0_u64;
                    for item in unigrams.iter()? {
                        let (_, value) = item?;
                        sum = sum.saturating_add(value.value());
                    }
                    uni_total.insert(UNIGRAM_TOTAL_KEY, sum)?;
                }
            }
            txn.open_table(PHRASE)?;
            txn.open_table(PHRASE_BY_TEXT)?;
            txn.open_table(PHRASE_BY_LIB_TEXT)?;
            txn.open_table(PRONUNCIATION)?;
            let mut alloc = txn.open_table(ALLOC)?;
            if alloc.get(ALLOC_CURSOR)?.is_none() {
                alloc.insert(ALLOC_CURSOR, FIRST_USER_TOKEN)?;
            }
            has_user_data_in_write_txn(&txn)?
        };
        txn.commit()?;
        let inner = Arc::new(StoreInner {
            count_snapshot: Mutex::new(None),
            db: Mutex::new(db),
            dirty: AtomicBool::new(false),
            write_generation: AtomicU64::new(0),
            has_user_data: AtomicBool::new(has_user_data),
        });
        reg.insert(key, Arc::downgrade(&inner));
        Ok(Self {
            inner,
            _lease: RegistryLease,
        })
    }

    /// Stored bigram count for `(prev, cur)`; `0` if unrecorded.
    pub fn bigram_count(&self, prev: Token, cur: Token) -> Result<u64, UserStoreError> {
        let db = self.database();
        let txn = db.begin_read()?;
        let table = txn.open_table(BIGRAM)?;
        Ok(table.get((prev, cur))?.map_or(0, |g| g.value()))
    }

    /// Total bigram mass recorded after `prev`; `0` if none.
    pub fn bigram_total(&self, prev: Token) -> Result<u64, UserStoreError> {
        let db = self.database();
        let txn = db.begin_read()?;
        let table = txn.open_table(BIGRAM_TOTAL)?;
        Ok(table.get(prev)?.map_or(0, |g| g.value()))
    }

    /// Accumulated phrase-index unigram delta for `token`; `0` if none.
    ///
    /// On the decode hot path, so it takes the cached-snapshot route: an
    /// empty store answers from one atomic load. The uncached siblings
    /// ([`Self::bigram_count`], [`Self::bigram_total`],
    /// [`Self::unigram_total`]) are diagnostic and open their own read
    /// transaction per call.
    pub fn unigram_delta(&self, token: Token) -> Result<u64, UserStoreError> {
        if !self.has_user_data() {
            return Ok(0);
        }
        self.with_count_snapshot(|cached| cached.unigram_delta(token))
    }

    /// Sum of every stored unigram delta; `0` if the store is empty.
    pub fn unigram_total(&self) -> Result<u64, UserStoreError> {
        let db = self.database();
        let txn = db.begin_read()?;
        let table = txn.open_table(UNIGRAM_TOTAL)?;
        Ok(table.get(UNIGRAM_TOTAL_KEY)?.map_or(0, |g| g.value()))
    }

    /// One-transaction §5 overlay for scoring `token` after `prev`.
    ///
    /// `prev` of `None` is the empty-history (unigram-only) case: bigram
    /// fields stay zero. An empty store returns [`UserCountDelta::ZERO`].
    pub fn count_delta(
        &self,
        prev: Option<Token>,
        token: Token,
    ) -> Result<UserCountDelta, UserStoreError> {
        if !self.has_user_data() {
            return Ok(UserCountDelta::ZERO);
        }
        self.with_count_snapshot(|cached| cached.count_delta(prev, token))
    }

    /// Record a training selection of `cur` after `last` (the `pinyin_train`
    /// path, §2). Returns the seed applied.
    ///
    /// This is the one mutation that sets the §4 `m_modified` flag — the
    /// upstream set-sites are `pinyin_train` (`pinyin.cpp:2679`) and
    /// `pinyin_end_add_phrases` (`:658`); the predicted path and
    /// `pinyin_remember_user_input` deliberately do not, so a save after
    /// only those stays an upstream-faithful no-op.
    pub fn observe_selection(&mut self, last: Token, cur: Token) -> Result<u64, UserStoreError> {
        let seed = self.update(last, cur, SeedPolicy::Training)?;
        self.inner.dirty.store(true, Ordering::Relaxed);
        Ok(seed)
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
        let db = self.database();
        let txn = db.begin_write()?;
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

            let delta = seed::unigram_delta(seed);
            let mut unigram = txn.open_table(UNIGRAM)?;
            let prev_unigram = unigram.get(cur)?.map_or(0, |g| g.value());
            unigram.insert(cur, prev_unigram.saturating_add(delta))?;
            bump_unigram_total(&txn, delta)?;

            seed
        };
        txn.commit()?;
        self.mark_committed_write(db, true);
        Ok(seed)
    }

    /// Add a user phrase under [`crate::USER_DICTIONARY`] (`_add_phrase`, §3.2).
    ///
    /// `count` of `None` means [`DEFAULT_PHRASE_COUNT`] (the C ABI's `-1`).
    /// If `phrase` is already in the user sub-index, a new reading is merged
    /// onto the existing token and the unigram is left unchanged. A new
    /// phrase allocates `max token + 1`, seeds the unigram with
    /// `count * 3`, and advances the allocation cursor — all in one write
    /// transaction.
    ///
    /// # Errors
    ///
    /// [`UserStoreError::InvalidPhrase`] when the text is empty, too long, or
    /// the key count does not match the Unicode scalar length.
    /// [`UserStoreError::TokenSpaceExhausted`] when the 24-bit user id space
    /// is full.
    pub fn add_phrase(
        &mut self,
        phrase: &str,
        keys: &[PinyinKey],
        count: Option<u64>,
    ) -> Result<Token, UserStoreError> {
        self.add_phrase_in(USER_DICTIONARY, phrase, keys, count)
    }

    /// Add a phrase under `library` (`USER_DICTIONARY` or `NETWORK_DICTIONARY`).
    ///
    /// Same `_add_phrase` path as [`Self::add_phrase`]; only the token nibble
    /// and the per-library reverse lookup change.
    ///
    /// # Errors
    ///
    /// [`UserStoreError::InvalidPhrase`] when the text is empty, too long, the
    /// key count does not match, or `library` is not a `USER_FILE` nibble.
    /// [`UserStoreError::TokenSpaceExhausted`] when that nibble's 24-bit id
    /// space is full.
    pub fn add_phrase_in(
        &mut self,
        library: u8,
        phrase: &str,
        keys: &[PinyinKey],
        count: Option<u64>,
    ) -> Result<Token, UserStoreError> {
        if !is_user_file_library(library) || !phrase::phrase_and_keys_valid(phrase, keys) {
            return Err(UserStoreError::InvalidPhrase);
        }
        let count = count.unwrap_or(DEFAULT_PHRASE_COUNT);
        let key_bytes = phrase::encode_keys(keys);

        let db = self.database();
        let txn = db.begin_write()?;
        let token = {
            let mut by_lib = txn.open_table(PHRASE_BY_LIB_TEXT)?;
            let mut by_text = txn.open_table(PHRASE_BY_TEXT)?;
            let existing = if let Some(found) = by_lib.get((library, phrase))? {
                Some(found.value())
            } else if library == USER_DICTIONARY {
                by_text.get(phrase)?.map(|g| g.value())
            } else {
                None
            };
            if let Some(token) = existing {
                let mut prons = txn.open_table(PRONUNCIATION)?;
                let prev = prons
                    .get((token, key_bytes.as_slice()))?
                    .map_or(0, |g| g.value());
                prons.insert((token, key_bytes.as_slice()), prev.saturating_add(count))?;
                token
            } else {
                let mut alloc = txn.open_table(ALLOC)?;
                let raw = if library == USER_DICTIONARY {
                    alloc
                        .get(library)?
                        .or(alloc.get(ALLOC_CURSOR)?)
                        .map_or(FIRST_USER_TOKEN, |g| g.value())
                } else {
                    alloc
                        .get(library)?
                        .map_or(first_library_token(library), |g| g.value())
                };
                let token = phrase::canonicalize_library_token(library, raw)
                    .ok_or(UserStoreError::TokenSpaceExhausted)?;
                let next = phrase::next_library_token_after(library, token)
                    .ok_or(UserStoreError::TokenSpaceExhausted)?;
                alloc.insert(library, next)?;
                if library == USER_DICTIONARY {
                    alloc.insert(ALLOC_CURSOR, next)?;
                }
                drop(alloc);

                let mut phrases = txn.open_table(PHRASE)?;
                phrases.insert(token, phrase)?;
                by_lib.insert((library, phrase), token)?;
                if library == USER_DICTIONARY {
                    by_text.insert(phrase, token)?;
                }

                let mut prons = txn.open_table(PRONUNCIATION)?;
                prons.insert((token, key_bytes.as_slice()), count)?;

                let delta = count.saturating_mul(ADD_PHRASE_UNIGRAM_FACTOR);
                let mut unigram = txn.open_table(UNIGRAM)?;
                let prev = unigram.get(token)?.map_or(0, |g| g.value());
                unigram.insert(token, prev.saturating_add(delta))?;
                bump_unigram_total(&txn, delta)?;
                token
            }
        };
        txn.commit()?;
        self.mark_committed_write(db, true);
        Ok(token)
    }

    /// Phrase text and pronunciations for `token`, if this store owns it.
    pub fn phrase(&self, token: Token) -> Result<Option<UserPhrase>, UserStoreError> {
        let db = self.database();
        let txn = db.begin_read()?;
        let phrases = txn.open_table(PHRASE)?;
        let Some(text) = phrases.get(token)?.map(|g| g.value().to_owned()) else {
            return Ok(None);
        };
        let prons = txn.open_table(PRONUNCIATION)?;
        let pronunciations = collect_pronunciations(&prons, token)?;
        Ok(Some(UserPhrase::new(token, text, pronunciations)))
    }

    /// Token already allocated for `phrase` in the user sub-index, if any.
    pub fn token_for_phrase(&self, phrase: &str) -> Result<Option<Token>, UserStoreError> {
        self.token_for_phrase_in(USER_DICTIONARY, phrase)
    }

    /// Token already allocated for `phrase` in `library`, if any.
    pub fn token_for_phrase_in(
        &self,
        library: u8,
        phrase: &str,
    ) -> Result<Option<Token>, UserStoreError> {
        let db = self.database();
        let txn = db.begin_read()?;
        let by_lib = txn.open_table(PHRASE_BY_LIB_TEXT)?;
        if let Some(token) = by_lib.get((library, phrase))?.map(|g| g.value()) {
            return Ok(Some(token));
        }
        if library == USER_DICTIONARY {
            let by_text = txn.open_table(PHRASE_BY_TEXT)?;
            return Ok(by_text.get(phrase)?.map(|g| g.value()));
        }
        Ok(None)
    }

    /// Current write generation; [`UserLookup`] rebuilds when this changes.
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.write_generation()
    }

    /// Next token the store will allocate. Persisted; a reopen continues
    /// from this value (no reuse, no gap).
    pub fn next_user_token(&self) -> Result<Token, UserStoreError> {
        let db = self.database();
        let txn = db.begin_read()?;
        let alloc = txn.open_table(ALLOC)?;
        Ok(alloc
            .get(ALLOC_CURSOR)?
            .map_or(FIRST_USER_TOKEN, |g| g.value()))
    }

    /// `m_modified` (§4): a training update has been recorded since the
    /// last successful [`Self::save`]. Shared with every clone, so the
    /// context's `pinyin_save` sees dirtiness recorded through instances.
    #[must_use]
    pub fn is_modified(&self) -> bool {
        self.inner.dirty.load(Ordering::Relaxed)
    }

    /// Arm `m_modified` without a data write — `pinyin_end_add_phrases`
    /// (`pinyin.cpp:658`), the import trio's set-site. The phrase adds
    /// themselves were already durable per-add commits; this flag makes the
    /// next [`Self::save`] perform its compaction and clear cycle.
    pub fn mark_modified(&mut self) {
        self.inner.dirty.store(true, Ordering::Relaxed);
    }

    /// Every user phrase as §9 export rows, in token order then stored
    /// pronunciation order: one row per (phrase, pronunciation), the pinyin
    /// rendered as `'`-joined syllable spellings and the count the stored
    /// pronunciation count — the same shape `pinyin_iterator_get_next_phrase`
    /// yields upstream.
    ///
    /// Keys were written from [`oxpinyin_core::SyllableKey`] ids (T3), so every
    /// stored key renders; a row whose keys do not is skipped rather than
    /// fabricated.
    pub fn export_phrases(&self) -> Result<Vec<ExportedPhrase>, UserStoreError> {
        let db = self.database();
        let txn = db.begin_read()?;
        let phrases = txn.open_table(PHRASE)?;
        let prons = txn.open_table(PRONUNCIATION)?;
        let mut rows = Vec::new();
        for item in phrases.iter()? {
            let (token, text) = item?;
            if phrase_index_library_index(token.value()) != USER_DICTIONARY {
                continue;
            }
            for pronunciation in collect_pronunciations(&prons, token.value())? {
                let Some(pinyin) = pronunciation.render_pinyin() else {
                    continue;
                };
                rows.push(ExportedPhrase {
                    text: text.value().to_owned(),
                    pinyin,
                    count: pronunciation.count(),
                });
            }
        }
        Ok(rows)
    }

    /// Export rows for one `USER_FILE` nibble, in token then pronunciation
    /// order.
    pub fn export_phrases_in(&self, library: u8) -> Result<Vec<ExportedPhrase>, UserStoreError> {
        let db = self.database();
        let txn = db.begin_read()?;
        let phrases = txn.open_table(PHRASE)?;
        let prons = txn.open_table(PRONUNCIATION)?;
        let mut rows = Vec::new();
        for item in phrases.iter()? {
            let (token, text) = item?;
            if phrase_index_library_index(token.value()) != library {
                continue;
            }
            for pronunciation in collect_pronunciations(&prons, token.value())? {
                let Some(pinyin) = pronunciation.render_pinyin() else {
                    continue;
                };
                rows.push(ExportedPhrase {
                    text: text.value().to_owned(),
                    pinyin,
                    count: pronunciation.count(),
                });
            }
        }
        Ok(rows)
    }

    /// Every stored phrase (user and network) with pronunciations.
    pub fn phrases(&self) -> Result<Vec<UserPhrase>, UserStoreError> {
        let db = self.database();
        let txn = db.begin_read()?;
        let phrases = txn.open_table(PHRASE)?;
        let prons = txn.open_table(PRONUNCIATION)?;
        let mut out = Vec::new();
        for item in phrases.iter()? {
            let (token, text) = item?;
            let token = token.value();
            let pronunciations = collect_pronunciations(&prons, token)?;
            out.push(UserPhrase::new(
                token,
                text.value().to_owned(),
                pronunciations,
            ));
        }
        Ok(out)
    }

    /// User-bigram successors of `prev` as `(token, count)` pairs.
    pub fn bigram_successors(&self, prev: Token) -> Result<Vec<(Token, u64)>, UserStoreError> {
        let db = self.database();
        let txn = db.begin_read()?;
        let bigrams = txn.open_table(BIGRAM)?;
        let mut rows = Vec::new();
        for item in bigrams.range((prev, Token::MIN)..=(prev, Token::MAX))? {
            let (key, count) = item?;
            let (_prev, cur) = key.value();
            rows.push((cur, count.value()));
        }
        Ok(rows)
    }

    /// Every stored user-bigram row as `(prev, cur, count)`, raw — the
    /// §9 bigram export filters and renders these (upstream skips
    /// `sentence_start` predecessors and counts below the first-seed
    /// threshold, and resolves phrase text through the system phrase
    /// index, which this crate does not hold).
    pub fn export_bigrams(&self) -> Result<Vec<(Token, Token, u64)>, UserStoreError> {
        let db = self.database();
        let txn = db.begin_read()?;
        let bigrams = txn.open_table(BIGRAM)?;
        let mut rows = Vec::new();
        for item in bigrams.iter()? {
            let (key, count) = item?;
            let (prev, cur) = key.value();
            rows.push((prev, cur, count.value()));
        }
        Ok(rows)
    }

    /// The `pinyin_save` write side (§4).
    ///
    /// `Ok(false)` is the unmodified deliberate no-op — upstream's
    /// `pinyin_save` returns `false` when `m_modified` is clear
    /// (`pinyin.cpp:1136`). A dirty save compacts the database
    /// ([`Database::compact`], upstream's `m_phrase_index->compact()` at
    /// `:1139`): every training update was already committed atomically and
    /// durably (redb's Immediate durability fsyncs before `commit` returns),
    /// so there is no serialization step to write — compaction plus the
    /// flag clear is the whole save.
    ///
    /// The flag clears only on success. Upstream clears it unconditionally
    /// (`pinyin.cpp:1145`), which drops data after a failed write; the
    /// deviation keeps a failed save retryable and is noted in §4's
    /// reproduction notes.
    pub fn save(&mut self) -> Result<bool, UserStoreError> {
        if !self.is_modified() {
            return Ok(false);
        }
        // Hold the snapshot lock across compact so a decode cannot install a
        // new read transaction while redb refuses live readers.
        let mut snapshot = self.count_snapshot();
        *snapshot = None;
        let mut db = self.database();
        // `performed` is false when nothing further could be compacted; the
        // save still succeeded (upstream returns the write+rename result,
        // which is success even when the phrase index had nothing to move).
        let _performed = db.compact()?;
        drop(db);
        drop(snapshot);
        self.inner.dirty.store(false, Ordering::Relaxed);
        Ok(true)
    }

    /// `pinyin_mask_out`'s store side: delete every entry whose token
    /// matches `(token & mask) == value` — the exact predicate upstream
    /// applies to the bigram (`Bigram::mask_out`, `ngram_kyotodb.cpp:199`:
    /// matching index tokens drop the whole gram, matching items are
    /// removed and empty grams deleted), the phrase index
    /// (`SubPhraseIndex::mask_out`, `phrase_index.cpp:689`), and the user
    /// phrase/pinyin tables (`facade_phrase_table3.h:203`).
    ///
    /// One write transaction. The allocation cursor is untouched — a
    /// monotonic counter, where upstream's `range_end` can shrink and reuse
    /// ids; invisible in the exported value surface. Does **not** arm
    /// `m_modified`: upstream's set-sites are `pinyin_train` and
    /// `pinyin_end_add_phrases` only (`pinyin.cpp:2679`, `:658`), and the
    /// deletions are committed durably regardless.
    pub fn mask_out(&mut self, mask: Token, value: Token) -> Result<(), UserStoreError> {
        let db = self.database();
        let txn = db.begin_write()?;
        {
            // Bigram: drop rows whose predecessor or successor matches, then
            // rewrite every predecessor's total from the survivors.
            let mut bigram = txn.open_table(BIGRAM)?;
            let mut totals = txn.open_table(BIGRAM_TOTAL)?;
            let mut survivors: std::collections::BTreeMap<Token, u64> =
                std::collections::BTreeMap::new();
            let mut rows: Vec<((Token, Token), u64)> = Vec::new();
            for item in bigram.iter()? {
                let (key, count) = item?;
                let (prev, cur) = key.value();
                rows.push(((prev, cur), count.value()));
            }
            for ((prev, cur), count) in rows {
                if (prev & mask) == value || (cur & mask) == value {
                    bigram.remove((prev, cur))?;
                } else {
                    let slot = survivors.entry(prev).or_default();
                    *slot = slot.saturating_add(count);
                }
            }
            let mut old_totals: Vec<Token> = Vec::new();
            for item in totals.iter()? {
                let (prev, _) = item?;
                old_totals.push(prev.value());
            }
            for prev in old_totals {
                totals.remove(prev)?;
            }
            for (prev, total) in survivors {
                if total > 0 {
                    totals.insert(prev, total)?;
                }
            }

            // Unigram deltas and their running total.
            let mut unigram = txn.open_table(UNIGRAM)?;
            let mut uni_total = txn.open_table(UNIGRAM_TOTAL)?;
            let mut removed: Vec<Token> = Vec::new();
            let mut kept_sum = 0_u64;
            for item in unigram.iter()? {
                let (token, delta) = item?;
                let token = token.value();
                if (token & mask) == value {
                    removed.push(token);
                } else {
                    kept_sum = kept_sum.saturating_add(delta.value());
                }
            }
            for token in removed {
                unigram.remove(token)?;
            }
            uni_total.insert(UNIGRAM_TOTAL_KEY, kept_sum)?;

            // User phrases: text, reverse lookup, and pronunciations.
            let mut phrases = txn.open_table(PHRASE)?;
            let mut by_text = txn.open_table(PHRASE_BY_TEXT)?;
            let mut by_lib = txn.open_table(PHRASE_BY_LIB_TEXT)?;
            let mut prons = txn.open_table(PRONUNCIATION)?;
            let mut matched: Vec<(Token, String)> = Vec::new();
            for item in phrases.iter()? {
                let (token, text) = item?;
                let token = token.value();
                if (token & mask) == value {
                    matched.push((token, text.value().to_owned()));
                }
            }
            for (token, text) in matched {
                phrases.remove(token)?;
                by_text.remove(text.as_str())?;
                by_lib.remove((phrase_index_library_index(token), text.as_str()))?;
                remove_pronunciations(&mut prons, token)?;
            }
        }
        let has_user_data = has_user_data_in_write_txn(&txn)?;
        txn.commit()?;
        self.mark_committed_write(db, has_user_data);
        Ok(())
    }

    /// `pinyin_remove_user_candidate`'s store side (§3.4): fully removes a
    /// user phrase — the phrase index entry, its pronunciations, its bigram
    /// rows (as predecessor and successor), and its unigram delta.
    ///
    /// `Ok(false)` when the store does not own `token` as a user phrase
    /// (upstream asserts `USER_DICTIONARY` ownership; oxpinyin reports it).
    /// Does **not** arm `m_modified`, like upstream.
    pub fn remove_user_phrase(&mut self, token: Token) -> Result<bool, UserStoreError> {
        let db = self.database();
        let txn = db.begin_write()?;
        {
            // The full-token predicate on every table that names `token`,
            // plus the phrase rows themselves. An absent phrase is the
            // `Ok(false)` "store does not own this token" report.
            let mut phrases = txn.open_table(PHRASE)?;
            let Some(text) = phrases.get(token)?.map(|g| g.value().to_owned()) else {
                return Ok(false);
            };
            phrases.remove(token)?;
            let mut by_text = txn.open_table(PHRASE_BY_TEXT)?;
            by_text.remove(text.as_str())?;
            let mut by_lib = txn.open_table(PHRASE_BY_LIB_TEXT)?;
            by_lib.remove((phrase_index_library_index(token), text.as_str()))?;
            let mut prons = txn.open_table(PRONUNCIATION)?;
            remove_pronunciations(&mut prons, token)?;

            let mut bigram = txn.open_table(BIGRAM)?;
            let mut totals = txn.open_table(BIGRAM_TOTAL)?;
            let mut survivors: std::collections::BTreeMap<Token, u64> =
                std::collections::BTreeMap::new();
            let mut rows: Vec<((Token, Token), u64)> = Vec::new();
            for item in bigram.iter()? {
                let (key, count) = item?;
                let (prev, cur) = key.value();
                rows.push(((prev, cur), count.value()));
            }
            for ((prev, cur), count) in rows {
                if prev == token || cur == token {
                    bigram.remove((prev, cur))?;
                } else {
                    let slot = survivors.entry(prev).or_default();
                    *slot = slot.saturating_add(count);
                }
            }
            let mut old_totals: Vec<Token> = Vec::new();
            for item in totals.iter()? {
                let (prev, _) = item?;
                old_totals.push(prev.value());
            }
            for prev in old_totals {
                totals.remove(prev)?;
            }
            for (prev, total) in survivors {
                if total > 0 {
                    totals.insert(prev, total)?;
                }
            }

            let mut unigram = txn.open_table(UNIGRAM)?;
            let mut uni_total = txn.open_table(UNIGRAM_TOTAL)?;
            let mut kept_sum = 0_u64;
            for item in unigram.iter()? {
                let (candidate, delta) = item?;
                let candidate = candidate.value();
                if candidate != token {
                    kept_sum = kept_sum.saturating_add(delta.value());
                }
            }
            unigram.remove(token)?;
            uni_total.insert(UNIGRAM_TOTAL_KEY, kept_sum)?;
        }
        let has_user_data = has_user_data_in_write_txn(&txn)?;
        txn.commit()?;
        self.mark_committed_write(db, has_user_data);
        Ok(true)
    }
}

impl CountSnapshot {
    fn unigram_delta(&self, token: Token) -> Result<u64, UserStoreError> {
        Ok(self.unigram.get(token)?.map_or(0, |g| g.value()))
    }

    fn count_delta(
        &self,
        prev: Option<Token>,
        token: Token,
    ) -> Result<UserCountDelta, UserStoreError> {
        let unigram_delta = self.unigram.get(token)?.map_or(0, |g| g.value());
        let unigram_total_delta = self
            .unigram_total
            .get(UNIGRAM_TOTAL_KEY)?
            .map_or(0, |g| g.value());
        let (bigram_count, bigram_total) = if let Some(prev) = prev {
            (
                self.bigram.get((prev, token))?.map_or(0, |g| g.value()),
                self.bigram_total.get(prev)?.map_or(0, |g| g.value()),
            )
        } else {
            (0, 0)
        };
        Ok(UserCountDelta {
            bigram_count,
            bigram_total,
            unigram_delta,
            unigram_total_delta,
        })
    }
}

/// Whether the store holds any user data at all, evaluated inside `txn`.
///
/// Backs the [`UserStore::has_user_data`] fast path, so a wrong `false` here
/// is a silent scoring failure, not a slow one.
///
/// `BIGRAM_TOTAL` and `UNIGRAM_TOTAL` are deliberately absent: they are
/// derived sums that every writer keeps trivial whenever their source table
/// is empty. `mask_out` and `remove_user_phrase` rewrite the bigram totals
/// from the surviving rows and insert only positive ones, and set the unigram
/// total to the surviving sum; `open`'s backfill sums an empty `UNIGRAM` to
/// `0`. Reading a derived total when its source is empty therefore always
/// yields the same `UserCountDelta::ZERO` this predicate authorises. A future
/// writer able to leave a non-trivial total behind an empty source table must
/// add that table here.
fn has_user_data_in_write_txn(txn: &redb::WriteTransaction) -> Result<bool, UserStoreError> {
    let bigram = txn.open_table(BIGRAM)?;
    if !bigram.is_empty()? {
        return Ok(true);
    }
    drop(bigram);

    let unigram = txn.open_table(UNIGRAM)?;
    if !unigram.is_empty()? {
        return Ok(true);
    }
    drop(unigram);

    let phrases = txn.open_table(PHRASE)?;
    if !phrases.is_empty()? {
        return Ok(true);
    }
    drop(phrases);

    let pronunciations = txn.open_table(PRONUNCIATION)?;
    Ok(!pronunciations.is_empty()?)
}

/// Removes every pronunciation row of `token`.
fn remove_pronunciations(
    prons: &mut redb::Table<(Token, &[u8]), u64>,
    token: Token,
) -> Result<(), UserStoreError> {
    // Guard `token + 1` against overflow at `Token::MAX` (constitution §4:
    // no panic on any input), mirroring `collect_pronunciations`.
    let start: (Token, &[u8]) = (token, &[]);
    let range = match token.checked_add(1) {
        Some(next) => {
            let end: (Token, &[u8]) = (next, &[]);
            prons.range(start..end)?
        }
        None => prons.range(start..)?,
    };
    let mut keys: Vec<Vec<u8>> = Vec::new();
    for item in range {
        let (key, _) = item?;
        let (_, key_bytes) = key.value();
        keys.push(key_bytes.to_vec());
    }
    for key in keys {
        prons.remove((token, key.as_slice()))?;
    }
    Ok(())
}

fn bump_unigram_total(txn: &redb::WriteTransaction, delta: u64) -> Result<(), UserStoreError> {
    let mut total = txn.open_table(UNIGRAM_TOTAL)?;
    let prev = total.get(UNIGRAM_TOTAL_KEY)?.map_or(0, |g| g.value());
    total.insert(UNIGRAM_TOTAL_KEY, prev.saturating_add(delta))?;
    Ok(())
}

fn collect_pronunciations(
    prons: &redb::ReadOnlyTable<(Token, &[u8]), u64>,
    token: Token,
) -> Result<Vec<UserPronunciation>, UserStoreError> {
    let start: (Token, &[u8]) = (token, &[]);
    let mut out = Vec::new();
    if token < Token::MAX {
        let end: (Token, &[u8]) = (token + 1, &[]);
        for item in prons.range(start..end)? {
            let (key, value) = item?;
            let (_tok, key_bytes) = key.value();
            out.push(UserPronunciation::new(
                phrase::decode_keys(key_bytes),
                value.value(),
            ));
        }
    } else {
        for item in prons.range(start..)? {
            let (key, value) = item?;
            let (_tok, key_bytes) = key.value();
            out.push(UserPronunciation::new(
                phrase::decode_keys(key_bytes),
                value.value(),
            ));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use oxpinyin_core::SyllableKey;

    use super::*;
    use crate::{PHRASE_INDEX_LIBRARY_MASK, USER_DICTIONARY, phrase_index_make_token};

    fn temp_path(tag: &str) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("oxpinyin-user-{tag}-{}.redb", std::process::id()));
        let _ = std::fs::remove_file(&path);
        path
    }

    #[test]
    fn open_creates_empty_store() {
        let path = temp_path("empty");
        let store = UserStore::open(&path).unwrap();
        assert!(!store.has_user_data());
        assert_eq!(store.write_generation(), 0);
        assert_eq!(store.bigram_count(1, 2).unwrap(), 0);
        assert_eq!(store.bigram_total(1).unwrap(), 0);
        assert_eq!(store.unigram_delta(2).unwrap(), 0);
        assert_eq!(store.unigram_total().unwrap(), 0);
        assert_eq!(store.count_delta(Some(1), 2).unwrap(), UserCountDelta::ZERO);
        assert_eq!(store.count_delta(None, 2).unwrap(), UserCountDelta::ZERO);
        assert!(
            store.count_snapshot().is_none(),
            "empty count_delta must not open a redb read transaction"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn cached_count_delta_refreshes_after_committed_writes() {
        let path = temp_path("cache-refresh");
        let mut store = UserStore::open(&path).unwrap();

        assert_eq!(
            store.count_delta(Some(1), 100).unwrap(),
            UserCountDelta::ZERO
        );
        assert_eq!(store.write_generation(), 0);

        store.observe_selection(1, 100).unwrap();
        let first_generation = store.write_generation();
        assert!(store.has_user_data());
        assert_eq!(
            store.count_delta(Some(1), 100).unwrap(),
            UserCountDelta {
                bigram_count: 69,
                bigram_total: 69,
                unigram_delta: 483,
                unigram_total_delta: 483,
            }
        );

        store.observe_selection(1, 100).unwrap();
        assert!(
            store.write_generation() > first_generation,
            "a committed write invalidates the cached read snapshot"
        );
        assert_eq!(
            store.count_delta(Some(1), 100).unwrap(),
            UserCountDelta {
                bigram_count: 207,
                bigram_total: 207,
                unigram_delta: 483 + 966,
                unigram_total_delta: 483 + 966,
            }
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn mask_out_all_marks_store_empty_again() {
        let path = temp_path("empty-again");
        let mut store = UserStore::open(&path).unwrap();
        store.observe_selection(1, 100).unwrap();
        assert!(store.has_user_data());
        assert_ne!(
            store.count_delta(Some(1), 100).unwrap(),
            UserCountDelta::ZERO
        );

        store.mask_out(0, 0).unwrap();
        assert!(!store.has_user_data());
        assert_eq!(
            store.count_delta(Some(1), 100).unwrap(),
            UserCountDelta::ZERO
        );
        assert!(
            store.count_snapshot().is_none(),
            "an emptied store must not keep a cached read transaction"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_write_through_one_handle_invalidates_another_handles_cache() {
        // The C ABI topology: the context's canonical store writes while
        // `SharedLm`'s clone reads `count_delta` once per candidate. Both are
        // clones over one `StoreInner`, so the generation counter, the
        // `has_user_data` flag, and the snapshot mutex are shared.
        // `second_open_of_same_path_shares_the_handle` checks cross-handle
        // visibility through `bigram_count`, which bypasses the cache
        // entirely — this is the same question asked of the cached path.
        let path = temp_path("clone-cache");
        let mut writer = UserStore::open(&path).unwrap();
        let reader = UserStore::open(&path).unwrap();

        // Prime the reader's fast path while the store is still empty.
        assert_eq!(
            reader.count_delta(Some(1), 100).unwrap(),
            UserCountDelta::ZERO
        );

        writer.observe_selection(1, 100).unwrap();
        assert!(
            reader.has_user_data(),
            "the flag lives on the shared inner, not the writing handle"
        );
        assert_eq!(reader.count_delta(Some(1), 100).unwrap().bigram_count, 69);

        // A second write must retire the snapshot the reader just cached.
        writer.observe_selection(1, 100).unwrap();
        assert_eq!(reader.count_delta(Some(1), 100).unwrap().bigram_count, 207);

        // Emptying through the writer restores the reader's zero-cost path
        // rather than stranding it on a stale snapshot.
        writer.mask_out(0, 0).unwrap();
        assert!(!reader.has_user_data());
        assert_eq!(
            reader.count_delta(Some(1), 100).unwrap(),
            UserCountDelta::ZERO
        );
        assert_eq!(reader.unigram_delta(100).unwrap(), 0);
        assert!(
            reader.count_snapshot().is_none(),
            "an emptied store must not keep a cached read transaction"
        );

        drop(writer);
        drop(reader);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn reopen_of_populated_store_sets_has_user_data() {
        let path = temp_path("reopen-populated");
        {
            let mut store = UserStore::open(&path).unwrap();
            store.observe_selection(1, 100).unwrap();
        }
        let store = UserStore::open(&path).unwrap();
        assert!(store.has_user_data());
        assert_eq!(store.count_delta(Some(1), 100).unwrap().bigram_count, 69);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn save_compacts_after_a_cached_read() {
        let path = temp_path("save-after-cache");
        let mut store = UserStore::open(&path).unwrap();
        store.observe_selection(1, 100).unwrap();
        assert_eq!(store.count_delta(Some(1), 100).unwrap().bigram_count, 69);
        assert!(store.save().unwrap());
        assert_eq!(store.count_delta(Some(1), 100).unwrap().bigram_count, 69);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn second_open_of_same_path_shares_the_handle() {
        // redb refuses a second write handle to an open file; `open` must hand
        // back a clone of the live one instead, so a second context on the same
        // user dir keeps learning (rather than silently degrading to `None`).
        let path = temp_path("shared-handle");
        let mut first = UserStore::open(&path).unwrap();
        let mut second = UserStore::open(&path).unwrap();

        // A write through one handle is visible through the other: one db.
        assert_eq!(first.observe_selection(1, 100).unwrap(), 69);
        assert_eq!(second.bigram_count(1, 100).unwrap(), 69);

        // The §4 dirty flag is shared: the write through `first` arms `second`'s
        // save gate, so `second` performs the (single, shared) save and both
        // handles are clean afterward.
        assert!(second.save().unwrap());
        assert!(!first.save().unwrap());

        drop(first);
        drop(second);
        assert!(
            !registry::contains_key(&path),
            "last drop must empty the registry entry"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn last_drop_removes_registry_entry_and_allows_reopen() {
        let path = temp_path("drain-reopen");

        let first = UserStore::open(&path).unwrap();
        let second = UserStore::open(&path).unwrap();
        assert!(registry::contains_key(&path));

        drop(first);
        assert!(registry::contains_key(&path), "second clone is still live");
        drop(second);
        assert!(!registry::contains_key(&path));

        let reopened = UserStore::open(&path).unwrap();
        assert_eq!(reopened.bigram_count(1, 100).unwrap(), 0);
        drop(reopened);
        assert!(!registry::contains_key(&path));
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
        assert_eq!(store.unigram_total().unwrap(), 483);

        // Second selection of the same pair: seed 138, count 69 + 138 = 207.
        assert_eq!(store.observe_selection(1, 100).unwrap(), 138);
        assert_eq!(store.bigram_count(1, 100).unwrap(), 207);
        assert_eq!(store.bigram_total(1).unwrap(), 207);
        assert_eq!(store.unigram_delta(100).unwrap(), 483 + 966); // + 138 * 7
        assert_eq!(store.unigram_total().unwrap(), 483 + 966);
        assert_eq!(
            store.count_delta(Some(1), 100).unwrap(),
            UserCountDelta {
                bigram_count: 207,
                bigram_total: 207,
                unigram_delta: 483 + 966,
                unigram_total_delta: 483 + 966,
            }
        );
        // Empty history is unigram-only: bigram fields stay zero.
        assert_eq!(
            store.count_delta(None, 100).unwrap(),
            UserCountDelta {
                bigram_count: 0,
                bigram_total: 0,
                unigram_delta: 483 + 966,
                unigram_total_delta: 483 + 966,
            }
        );

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

    #[test]
    fn dirty_gate_matches_m_modified_semantics() {
        let path = temp_path("dirty");
        let mut store = UserStore::open(&path).unwrap();

        // Fresh open is clean: save is the §4 unmodified no-op (upstream
        // pinyin_save returns false when m_modified is clear, :1136).
        assert!(!store.is_modified());
        assert!(!store.save().unwrap());
        assert!(!store.save().unwrap());

        // pinyin_train (observe_selection) is an m_modified set-site
        // (upstream pinyin.cpp:2679): save now writes and clears.
        store.observe_selection(SENTENCE_START, 10).unwrap();
        assert!(store.is_modified());
        assert!(store.save().unwrap());
        assert!(!store.is_modified());
        assert!(!store.save().unwrap());

        // The predicted path and add_phrase deliberately do NOT set
        // m_modified (upstream's set-sites are train and end_add_phrases
        // only): their data is committed durably regardless, but a save
        // after only those stays an upstream-faithful no-op.
        store.observe_predicted(10, 20).unwrap();
        assert!(!store.is_modified());
        assert!(!store.save().unwrap());
        store.add_phrase("你好", &[1, 2], None).unwrap();
        assert!(!store.is_modified());
        assert!(!store.save().unwrap());

        // The next training selection re-arms the gate.
        store.observe_selection(SENTENCE_START, 10).unwrap();
        assert!(store.is_modified());
        assert!(store.save().unwrap());
        assert!(!store.is_modified());

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn save_reopen_roundtrip_preserves_counts_cursor_and_total() {
        let path = temp_path("save-rt");
        {
            let mut store = UserStore::open(&path).unwrap();
            store.observe_selection(SENTENCE_START, 10).unwrap();
            store.observe_selection(10, 20).unwrap();
            store.observe_selection(10, 20).unwrap();
            let token = store.add_phrase("你好", &[10, 20], None).unwrap();
            assert_eq!(token, FIRST_USER_TOKEN);
            // The dirty save writes (compacts) and clears the flag.
            assert!(store.is_modified());
            assert!(store.save().unwrap());
            assert!(!store.is_modified());
        } // drop closes the database

        let mut store = UserStore::open(&path).unwrap();
        assert!(!store.is_modified());
        assert!(!store.save().unwrap(), "a reopen starts clean");

        // Counts, allocation cursor and running unigram total all survive.
        assert_eq!(store.bigram_count(SENTENCE_START, 10).unwrap(), 69);
        assert_eq!(store.bigram_count(10, 20).unwrap(), 207); // 69 + 138
        assert_eq!(store.bigram_total(SENTENCE_START).unwrap(), 69);
        assert_eq!(store.bigram_total(10).unwrap(), 207);
        assert_eq!(store.unigram_delta(10).unwrap(), 483);
        assert_eq!(store.unigram_delta(20).unwrap(), 1449); // 483 + 966
        assert_eq!(store.unigram_total().unwrap(), 483 + 1449 + 15); // + phrase 5*3
        assert_eq!(store.next_user_token().unwrap(), FIRST_USER_TOKEN + 1);
        assert_eq!(
            store.token_for_phrase("你好").unwrap(),
            Some(FIRST_USER_TOKEN)
        );
        let phrase = store
            .phrase(FIRST_USER_TOKEN)
            .unwrap()
            .expect("phrase stored");
        assert_eq!(phrase.text(), "你好");
        assert_eq!(phrase.pronunciations()[0].keys(), &[10, 20]);
        assert_eq!(phrase.pronunciations()[0].count(), 5);

        // The §5 overlay reads the persisted counts.
        assert_eq!(
            store.count_delta(Some(10), 20).unwrap(),
            UserCountDelta {
                bigram_count: 207,
                bigram_total: 207,
                unigram_delta: 1449,
                unigram_total_delta: 483 + 1449 + 15,
            }
        );
        assert_eq!(store.count_delta(None, 20).unwrap().bigram_count, 0);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn first_allocation_is_first_user_token() {
        let path = temp_path("first-tok");
        let mut store = UserStore::open(&path).unwrap();
        assert_eq!(store.next_user_token().unwrap(), FIRST_USER_TOKEN);
        let token = store.add_phrase("你好", &[10, 20], None).unwrap();
        assert_eq!(token, FIRST_USER_TOKEN);
        assert_eq!(token, 0x0700_0001);
        assert!(phrase::is_user_token(token));
        assert_eq!(store.next_user_token().unwrap(), FIRST_USER_TOKEN + 1);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn allocation_increments_by_one_without_gap() {
        let path = temp_path("incr");
        let mut store = UserStore::open(&path).unwrap();
        let a = store.add_phrase("甲", &[1], None).unwrap();
        let b = store.add_phrase("乙", &[2], None).unwrap();
        let c = store.add_phrase("丙", &[3], None).unwrap();
        assert_eq!(a, FIRST_USER_TOKEN);
        assert_eq!(b, a + 1);
        assert_eq!(c, a + 2);
        assert_eq!(store.next_user_token().unwrap(), a + 3);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn user_token_is_distinguishable_from_system_token() {
        let path = temp_path("nibble");
        let mut store = UserStore::open(&path).unwrap();
        let user = store.add_phrase("词", &[7], None).unwrap();
        // GB_DICTIONARY = 1; a typical system token is not a user token.
        const SYSTEM: Token = 0x0100_0001;
        assert!(phrase::is_user_token(user));
        assert!(!phrase::is_user_token(SYSTEM));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn network_and_user_can_share_phrase_text() {
        let path = temp_path("two-nibbles");
        let mut store = UserStore::open(&path).unwrap();
        let user = store
            .add_phrase_in(phrase::USER_DICTIONARY, "词", &[7], Some(5))
            .unwrap();
        let net = store
            .add_phrase_in(phrase::NETWORK_DICTIONARY, "词", &[7], Some(5))
            .unwrap();
        assert_ne!(user, net);
        assert_eq!(phrase::phrase_index_library_index(user), 7);
        assert_eq!(phrase::phrase_index_library_index(net), 6);
        assert_eq!(store.export_phrases().unwrap().len(), 1);
        assert_eq!(store.export_phrases_in(6).unwrap().len(), 1);
        assert_eq!(store.next_user_token().unwrap(), user + 1);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn add_phrase_seeds_unigram_with_count_times_three() {
        let path = temp_path("uni");
        let mut store = UserStore::open(&path).unwrap();
        let token = store.add_phrase("你好", &[10, 20], None).unwrap();
        // default_count 5 * add-phrase unigram_factor 3.
        assert_eq!(store.unigram_delta(token).unwrap(), 15);
        assert_eq!(store.bigram_count(SENTENCE_START, token).unwrap(), 0);

        let token2 = store.add_phrase("世界", &[30, 40], Some(10)).unwrap();
        assert_eq!(store.unigram_delta(token2).unwrap(), 30);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn existing_phrase_merges_a_new_reading() {
        let path = temp_path("merge");
        let mut store = UserStore::open(&path).unwrap();
        let first = store.add_phrase("你好", &[10, 20], None).unwrap();
        let again = store.add_phrase("你好", &[11, 20], Some(8)).unwrap();
        assert_eq!(first, again);
        // Merge does not allocate and does not raise the unigram (§3.2).
        assert_eq!(store.next_user_token().unwrap(), FIRST_USER_TOKEN + 1);
        assert_eq!(store.unigram_delta(first).unwrap(), 15);

        let got = store.phrase(first).unwrap().unwrap();
        assert_eq!(got.text(), "你好");
        assert_eq!(got.pronunciations().len(), 2);
        assert_eq!(got.pronunciations()[0].keys(), &[10, 20]);
        assert_eq!(got.pronunciations()[0].count(), 5);
        assert_eq!(got.pronunciations()[1].keys(), &[11, 20]);
        assert_eq!(got.pronunciations()[1].count(), 8);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn same_reading_accumulates_pronunciation_count() {
        let path = temp_path("same-read");
        let mut store = UserStore::open(&path).unwrap();
        let token = store.add_phrase("词", &[7], Some(5)).unwrap();
        let again = store.add_phrase("词", &[7], Some(5)).unwrap();
        assert_eq!(token, again);
        assert_eq!(store.unigram_delta(token).unwrap(), 15); // seeded once
        let got = store.phrase(token).unwrap().unwrap();
        assert_eq!(got.pronunciations().len(), 1);
        assert_eq!(got.pronunciations()[0].count(), 10);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn phrase_roundtrip_reopen_preserves_cursor() {
        let path = temp_path("phrase-rt");
        let (t1, t2, next) = {
            let mut store = UserStore::open(&path).unwrap();
            let t1 = store.add_phrase("你好", &[10, 20], None).unwrap();
            let t2 = store.add_phrase("世界", &[30, 40], Some(9)).unwrap();
            store.add_phrase("你好", &[11, 20], Some(2)).unwrap();
            (t1, t2, store.next_user_token().unwrap())
        };

        let store = UserStore::open(&path).unwrap();
        assert_eq!(store.next_user_token().unwrap(), next);
        assert_eq!(next, FIRST_USER_TOKEN + 2);

        let p1 = store.phrase(t1).unwrap().unwrap();
        assert_eq!(p1.text(), "你好");
        assert_eq!(p1.pronunciations().len(), 2);
        assert_eq!(p1.pronunciations()[0].keys(), &[10, 20]);
        assert_eq!(p1.pronunciations()[0].count(), 5);
        assert_eq!(p1.pronunciations()[1].keys(), &[11, 20]);
        assert_eq!(p1.pronunciations()[1].count(), 2);
        assert_eq!(store.unigram_delta(t1).unwrap(), 15);
        assert_eq!(store.unigram_total().unwrap(), 15 + 27);

        let p2 = store.phrase(t2).unwrap().unwrap();
        assert_eq!(p2.text(), "世界");
        assert_eq!(p2.pronunciations()[0].keys(), &[30, 40]);
        assert_eq!(p2.pronunciations()[0].count(), 9);
        assert_eq!(store.unigram_delta(t2).unwrap(), 27);

        // Reopened cursor allocates the next id — no reuse, no gap.
        let mut store = store;
        let t3 = store.add_phrase("中国", &[50, 60], None).unwrap();
        assert_eq!(t3, t2 + 1);
        assert_eq!(t3, FIRST_USER_TOKEN + 2);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn invalid_phrase_is_rejected_without_allocation() {
        let path = temp_path("invalid");
        let mut store = UserStore::open(&path).unwrap();
        assert!(matches!(
            store.add_phrase("", &[], None),
            Err(UserStoreError::InvalidPhrase)
        ));
        assert!(matches!(
            store.add_phrase("你好", &[10], None),
            Err(UserStoreError::InvalidPhrase)
        ));
        assert!(matches!(
            store.add_phrase(&"啊".repeat(16), &[0; 16], None),
            Err(UserStoreError::InvalidPhrase)
        ));
        assert_eq!(store.next_user_token().unwrap(), FIRST_USER_TOKEN);
        assert!(store.token_for_phrase("你好").unwrap().is_none());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn lookup_of_unknown_token_is_none() {
        let path = temp_path("miss");
        let store = UserStore::open(&path).unwrap();
        assert!(store.phrase(FIRST_USER_TOKEN).unwrap().is_none());
        assert!(store.phrase(0x0100_0001).unwrap().is_none());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn export_phrases_render_the_pinned_triples() {
        let path = temp_path("export-phrases");
        let mut store = UserStore::open(&path).unwrap();
        let ni = SyllableKey::from_text("ni").expect("frozen key").index() as u16;
        let hao = SyllableKey::from_text("hao").expect("frozen key").index() as u16;
        let shi = SyllableKey::from_text("shi").expect("frozen key").index() as u16;
        let jie = SyllableKey::from_text("jie").expect("frozen key").index() as u16;

        // Two remembers of the same reading merge into one pronunciation
        // row (upstream's add_pronunciation merges exact-match keys).
        store.add_phrase("你好", &[ni, hao], None).unwrap();
        store.add_phrase("你好", &[ni, hao], Some(7)).unwrap();
        store.add_phrase("世界", &[shi, jie], Some(3)).unwrap();

        // Token order, then pronunciation order; pinyin is `'`-joined (§9).
        assert_eq!(
            store.export_phrases().unwrap(),
            vec![
                ExportedPhrase {
                    text: "你好".to_owned(),
                    pinyin: "ni'hao".to_owned(),
                    count: 12,
                },
                ExportedPhrase {
                    text: "世界".to_owned(),
                    pinyin: "shi'jie".to_owned(),
                    count: 3,
                },
            ]
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn export_bigrams_lists_every_stored_row_raw() {
        let path = temp_path("export-bigrams");
        let mut store = UserStore::open(&path).unwrap();
        store.observe_selection(SENTENCE_START, 10).unwrap();
        store.observe_selection(10, 20).unwrap();
        store.observe_selection(10, 20).unwrap();

        let mut rows = store.export_bigrams().unwrap();
        rows.sort();
        // Raw rows: sentence_start rows included (the §9 filters live at the
        // C ABI layer, mirroring upstream's iterator).
        assert_eq!(rows, vec![(SENTENCE_START, 10, 69), (10, 20, 207)]);
        let _ = std::fs::remove_file(&path);
    }

    /// A mixed store: system and user tokens across every table, so the
    /// mask predicates can be checked per class of entry.
    fn mixed_store(path: &std::path::Path) -> (UserStore, Token) {
        const SYSTEM_A: Token = 0x0100_0001;
        const SYSTEM_B: Token = 0x0200_0001;
        let mut store = UserStore::open(path).unwrap();
        let user_a = store.add_phrase("你好", &[10, 20], None).unwrap();
        let user_b = store.add_phrase("世界", &[30, 40], None).unwrap();
        // bigram: system→system, system→user, user→system, user→user.
        store.observe_selection(SYSTEM_A, SYSTEM_B).unwrap();
        store.observe_selection(SYSTEM_A, user_a).unwrap();
        store.observe_selection(user_a, SYSTEM_B).unwrap();
        store.observe_selection(user_a, user_b).unwrap();
        (store, user_a)
    }

    #[test]
    fn mask_out_user_clear_deletes_user_entries_and_keeps_system() {
        let path = temp_path("mask-user");
        let (mut store, user_a) = mixed_store(&path);
        // The setup training armed m_modified; a save consumes it, so the
        // mask below runs on a clean flag.
        assert!(store.is_modified());
        assert!(store.save().unwrap());

        // The frontend's "user" clear: PHRASE_INDEX_LIBRARY_MASK against
        // MAKE_TOKEN(USER_DICTIONARY, 0).
        store
            .mask_out(
                PHRASE_INDEX_LIBRARY_MASK,
                phrase_index_make_token(USER_DICTIONARY, 0),
            )
            .unwrap();

        // User phrases are gone; the system-only bigram survives with its
        // total recomputed.
        assert!(store.phrase(user_a).unwrap().is_none());
        assert!(store.token_for_phrase("你好").unwrap().is_none());
        assert_eq!(store.bigram_count(0x0100_0001, 0x0200_0001).unwrap(), 69);
        assert_eq!(store.bigram_total(0x0100_0001).unwrap(), 69);
        assert_eq!(store.bigram_count(user_a, 0x0200_0001).unwrap(), 0);
        assert_eq!(store.bigram_count(0x0100_0001, user_a).unwrap(), 0);
        assert_eq!(store.bigram_count(user_a, user_a + 1).unwrap(), 0);
        // The user tokens' unigram deltas are gone; the system token's
        // deltas (two observations of 0x0200_0001) survive; the running
        // total is recomputed.
        assert_eq!(store.unigram_delta(0x0200_0001).unwrap(), 966);
        assert_eq!(store.unigram_delta(user_a).unwrap(), 0);
        assert_eq!(store.unigram_total().unwrap(), 966);
        // The allocation cursor is monotonic — never rolled back.
        assert_eq!(store.next_user_token().unwrap(), user_a + 2);
        // Upstream set-sites: masking does not arm m_modified — the flag
        // stays clear, and a save is the §4 no-op.
        assert!(!store.is_modified());
        assert!(!store.save().unwrap());

        // The frontend's "all" clear: mask 0x0 against 0x0 removes what
        // remains.
        store.mask_out(0x0, 0x0).unwrap();
        assert_eq!(store.bigram_count(0x0100_0001, 0x0200_0001).unwrap(), 0);
        assert_eq!(store.bigram_total(0x0100_0001).unwrap(), 0);
        assert_eq!(store.unigram_total().unwrap(), 0);
        assert!(!store.is_modified());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn remove_user_phrase_deletes_everywhere_and_rejects_others() {
        let path = temp_path("remove");
        let (mut store, user_a) = mixed_store(&path);
        // Consume the setup training's m_modified so the removal runs on a
        // clean flag.
        assert!(store.save().unwrap());

        assert!(store.remove_user_phrase(user_a).unwrap());
        assert!(store.phrase(user_a).unwrap().is_none());
        assert!(store.token_for_phrase("你好").unwrap().is_none());
        // Bigram rows naming the phrase as predecessor or successor are
        // gone, with totals recomputed; unrelated rows survive.
        assert_eq!(store.bigram_count(user_a, 0x0200_0001).unwrap(), 0);
        assert_eq!(store.bigram_count(0x0100_0001, user_a).unwrap(), 0);
        assert_eq!(store.bigram_count(user_a, user_a + 1).unwrap(), 0);
        assert_eq!(store.bigram_count(0x0100_0001, 0x0200_0001).unwrap(), 69);
        assert_eq!(store.bigram_total(0x0100_0001).unwrap(), 69);
        assert_eq!(store.bigram_total(user_a).unwrap(), 0);
        // Unigram delta removed, total recomputed.
        assert_eq!(store.unigram_delta(user_a).unwrap(), 0);
        // SYSTEM_B's 966 plus 世界's seeding 15 and its trained 483.
        assert_eq!(store.unigram_total().unwrap(), 966 + 498);
        // Not armed, and unknown/system tokens report false.
        assert!(!store.is_modified());
        assert!(!store.remove_user_phrase(user_a).unwrap());
        assert!(!store.remove_user_phrase(0x0100_0001).unwrap());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn mask_and_remove_survive_a_reopen() {
        let path = temp_path("mask-rt");
        let (mut store, user_a) = mixed_store(&path);
        store
            .mask_out(
                PHRASE_INDEX_LIBRARY_MASK,
                phrase_index_make_token(USER_DICTIONARY, 0),
            )
            .unwrap();
        let other = store.add_phrase("中国", &[50, 60], None).unwrap();
        store.remove_user_phrase(other).unwrap();
        drop(store);

        let store = UserStore::open(&path).unwrap();
        assert!(store.token_for_phrase("你好").unwrap().is_none());
        assert!(store.token_for_phrase("中国").unwrap().is_none());
        assert_eq!(store.bigram_count(0x0100_0001, 0x0200_0001).unwrap(), 69);
        assert_eq!(store.unigram_delta(user_a).unwrap(), 0);
        assert_eq!(store.unigram_total().unwrap(), 966);
        let _ = std::fs::remove_file(&path);
    }
}
