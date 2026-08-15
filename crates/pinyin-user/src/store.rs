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
//! session and the C ABI (in `pinyin-engine` / `pinyin-capi`). T4 exposes
//! the counts as a [`pinyin_core::UserCountDelta`] for the decode-time
//! additive merge. T5 adds the save cycle: the §4 `m_modified` gate and the
//! redb-backed persistence point behind `pinyin_save`.

use std::fmt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use pinyin_core::{SyllableKey, UserCountDelta};
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};

use crate::phrase::{
    self, ADD_PHRASE_UNIGRAM_FACTOR, DEFAULT_PHRASE_COUNT, FIRST_USER_TOKEN, PinyinKey, UserPhrase,
    UserPronunciation,
};
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
/// not itself `Clone` (redb 4.1.0), so it lives behind an `Arc`; the `Mutex`
/// serializes the handle because [`Database::compact`] (the `pinyin_save`
/// write side) demands `&mut self`.
///
/// The §4 `m_modified` flag is also shared, as an [`AtomicBool`]: clones
/// record dirtiness through their own `&mut self` updates and the context's
/// `pinyin_save` observes it. The C ABI contract is main-thread-only, so the
/// flag uses relaxed ordering.
pub struct UserStore {
    db: Arc<Mutex<Database>>,
    dirty: Arc<AtomicBool>,
}

impl Clone for UserStore {
    fn clone(&self) -> Self {
        Self {
            db: Arc::clone(&self.db),
            dirty: Arc::clone(&self.dirty),
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
        self.db
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Open the user store at `path`, creating an empty database if absent.
    ///
    /// Count tables and phrase-index tables are created eagerly so that reads
    /// issued before any write succeed with zero / `None` rather than a
    /// "table does not exist" error. A missing allocation cursor is
    /// initialised to [`FIRST_USER_TOKEN`]. A freshly opened store is clean:
    /// [`Self::save`] is a no-op until a training update records a change.
    pub fn open(path: &Path) -> Result<Self, UserStoreError> {
        let db = Database::create(path).map_err(|e| match e {
            redb::DatabaseError::Storage(redb::StorageError::Io(io)) => UserStoreError::Io(io),
            other => UserStoreError::Db(other),
        })?;
        let txn = db.begin_write()?;
        {
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
            txn.open_table(PRONUNCIATION)?;
            let mut alloc = txn.open_table(ALLOC)?;
            if alloc.get(ALLOC_CURSOR)?.is_none() {
                alloc.insert(ALLOC_CURSOR, FIRST_USER_TOKEN)?;
            }
        }
        txn.commit()?;
        Ok(Self {
            db: Arc::new(Mutex::new(db)),
            dirty: Arc::new(AtomicBool::new(false)),
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
    pub fn unigram_delta(&self, token: Token) -> Result<u64, UserStoreError> {
        let db = self.database();
        let txn = db.begin_read()?;
        let table = txn.open_table(UNIGRAM)?;
        Ok(table.get(token)?.map_or(0, |g| g.value()))
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
        let db = self.database();
        let txn = db.begin_read()?;
        let unigrams = txn.open_table(UNIGRAM)?;
        let uni_total = txn.open_table(UNIGRAM_TOTAL)?;
        let unigram_delta = unigrams.get(token)?.map_or(0, |g| g.value());
        let unigram_total_delta = uni_total.get(UNIGRAM_TOTAL_KEY)?.map_or(0, |g| g.value());
        let (bigram_count, bigram_total) = if let Some(prev) = prev {
            let bigram = txn.open_table(BIGRAM)?;
            let totals = txn.open_table(BIGRAM_TOTAL)?;
            (
                bigram.get((prev, token))?.map_or(0, |g| g.value()),
                totals.get(prev)?.map_or(0, |g| g.value()),
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
        self.dirty.store(true, Ordering::Relaxed);
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
        if !phrase::phrase_and_keys_valid(phrase, keys) {
            return Err(UserStoreError::InvalidPhrase);
        }
        let count = count.unwrap_or(DEFAULT_PHRASE_COUNT);
        let key_bytes = phrase::encode_keys(keys);

        let db = self.database();
        let txn = db.begin_write()?;
        let token = {
            let mut by_text = txn.open_table(PHRASE_BY_TEXT)?;
            if let Some(existing) = by_text.get(phrase)? {
                let token = existing.value();
                drop(existing);
                let mut prons = txn.open_table(PRONUNCIATION)?;
                let prev = prons
                    .get((token, key_bytes.as_slice()))?
                    .map_or(0, |g| g.value());
                prons.insert((token, key_bytes.as_slice()), prev.saturating_add(count))?;
                token
            } else {
                let mut alloc = txn.open_table(ALLOC)?;
                let raw = alloc
                    .get(ALLOC_CURSOR)?
                    .map_or(FIRST_USER_TOKEN, |g| g.value());
                let token = phrase::canonicalize_user_token(raw)
                    .ok_or(UserStoreError::TokenSpaceExhausted)?;
                let next = phrase::next_user_token_after(token)
                    .ok_or(UserStoreError::TokenSpaceExhausted)?;
                alloc.insert(ALLOC_CURSOR, next)?;
                drop(alloc);

                let mut phrases = txn.open_table(PHRASE)?;
                phrases.insert(token, phrase)?;
                by_text.insert(phrase, token)?;

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
        let db = self.database();
        let txn = db.begin_read()?;
        let by_text = txn.open_table(PHRASE_BY_TEXT)?;
        Ok(by_text.get(phrase)?.map(|g| g.value()))
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
        self.dirty.load(Ordering::Relaxed)
    }

    /// Every user phrase as §9 export rows, in token order then stored
    /// pronunciation order: one row per (phrase, pronunciation), the pinyin
    /// rendered as `'`-joined syllable spellings and the count the stored
    /// pronunciation count — the same shape `pinyin_iterator_get_next_phrase`
    /// yields upstream.
    ///
    /// Keys were written from [`pinyin_core::SyllableKey`] ids (T3), so every
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
            for pronunciation in collect_pronunciations(&prons, token.value())? {
                let mut parts = Vec::with_capacity(pronunciation.keys().len());
                let mut renderable = true;
                for key in pronunciation.keys() {
                    match SyllableKey::from_index(usize::from(*key)) {
                        Some(syllable) => parts.push(syllable.text()),
                        None => {
                            renderable = false;
                            break;
                        }
                    }
                }
                if !renderable {
                    continue;
                }
                rows.push(ExportedPhrase {
                    text: text.value().to_owned(),
                    pinyin: parts.join("'"),
                    count: pronunciation.count(),
                });
            }
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
        let mut db = self.database();
        // `performed` is false when nothing further could be compacted; the
        // save still succeeded (upstream returns the write+rename result,
        // which is success even when the phrase index had nothing to move).
        let _performed = db.compact()?;
        drop(db);
        self.dirty.store(false, Ordering::Relaxed);
        Ok(true)
    }
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
        assert_eq!(store.unigram_total().unwrap(), 0);
        assert_eq!(store.count_delta(Some(1), 2).unwrap(), UserCountDelta::ZERO);
        assert_eq!(store.count_delta(None, 2).unwrap(), UserCountDelta::ZERO);
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
        assert_ne!(
            phrase::phrase_index_library_index(user),
            phrase::phrase_index_library_index(SYSTEM)
        );
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
}
