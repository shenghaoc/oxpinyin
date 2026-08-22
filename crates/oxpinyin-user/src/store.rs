//! Ordered-store-backed integer store for user-learning counts and the user
//! phrase index.
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
use std::ops::Bound;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use oxpinyin_core::UserCountDelta;
use oxpinyin_store::{DefaultStore, OrderedStore, ReadSnapshot, StoreError, WriteTxn};

use crate::codec;
use crate::phrase::{
    self, ADD_PHRASE_UNIGRAM_FACTOR, ADDON_DICTIONARY, DEFAULT_PHRASE_COUNT, FIRST_USER_TOKEN,
    PinyinKey, USER_DICTIONARY, UserPhrase, UserPronunciation, first_library_token,
    is_user_file_library, phrase_index_library_index,
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

// ── table names ───────────────────────────────────────────────────

const BIGRAM: &str = "user_bigram";
const BIGRAM_TOTAL: &str = "user_bigram_total";
const UNIGRAM: &str = "user_unigram";
const UNIGRAM_TOTAL: &str = "user_unigram_total";
const PHRASE: &str = "user_phrase";
const PHRASE_BY_TEXT: &str = "user_phrase_by_text";
const PHRASE_BY_LIB_TEXT: &str = "user_phrase_by_lib_text";
const PRONUNCIATION: &str = "user_pronunciation";
const ALLOC: &str = "user_phrase_alloc";

/// Sole key in the `user_unigram_total` table.
const UNIGRAM_TOTAL_KEY: u8 = 0;

/// Sole key in the `user_phrase_alloc` table.
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
    /// The store file could not be opened (I/O error).
    Io(std::io::Error),
    /// The storage backend reported an error.
    Store(StoreError),
    /// A stored value could not be decoded (corrupt or incompatible).
    Decode,
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
            Self::Store(e) => write!(f, "store error: {e}"),
            Self::Decode => write!(f, "stored value could not be decoded"),
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
            Self::Store(e) => Some(e),
            Self::Decode | Self::InvalidPhrase | Self::TokenSpaceExhausted => None,
        }
    }
}

impl From<StoreError> for UserStoreError {
    fn from(e: StoreError) -> Self {
        match e {
            StoreError::Io(io) => Self::Io(io),
            other => Self::Store(other),
        }
    }
}

// ── codec helpers ─────────────────────────────────────────────────

fn get_u64(
    store: &impl OrderedStore,
    table: &str,
    key: &[u8],
) -> Result<Option<u64>, UserStoreError> {
    match store.get(table, key)? {
        None => Ok(None),
        Some(bytes) => codec::decode_u64(&bytes)
            .map(Some)
            .ok_or(UserStoreError::Decode),
    }
}

fn get_u64_or(
    store: &impl OrderedStore,
    table: &str,
    key: &[u8],
    default: u64,
) -> Result<u64, UserStoreError> {
    Ok(get_u64(store, table, key)?.unwrap_or(default))
}

fn txn_get_u64(txn: &dyn WriteTxn, table: &str, key: &[u8]) -> Result<Option<u64>, StoreError> {
    match txn.get(table, key)? {
        None => Ok(None),
        Some(bytes) => codec::decode_u64(&bytes)
            .map(Some)
            .ok_or_else(|| StoreError::Backend("corrupt u64 value".into())),
    }
}

fn txn_get_u64_or(
    txn: &dyn WriteTxn,
    table: &str,
    key: &[u8],
    default: u64,
) -> Result<u64, StoreError> {
    Ok(txn_get_u64(txn, table, key)?.unwrap_or(default))
}

fn snap_get_u64(
    snap: &impl ReadSnapshot,
    table: &str,
    key: &[u8],
) -> Result<Option<u64>, UserStoreError> {
    match snap.get(table, key)? {
        None => Ok(None),
        Some(bytes) => codec::decode_u64(&bytes)
            .map(Some)
            .ok_or(UserStoreError::Decode),
    }
}

fn snap_get_u64_or(
    snap: &impl ReadSnapshot,
    table: &str,
    key: &[u8],
    default: u64,
) -> Result<u64, UserStoreError> {
    Ok(snap_get_u64(snap, table, key)?.unwrap_or(default))
}

fn bump_unigram_total(txn: &mut dyn WriteTxn, delta: u64) -> Result<(), StoreError> {
    let key = codec::encode_u8(UNIGRAM_TOTAL_KEY);
    let prev = txn_get_u64_or(txn, UNIGRAM_TOTAL, &key, 0)?;
    txn.put(
        UNIGRAM_TOTAL,
        &key,
        &codec::encode_u64(prev.saturating_add(delta)),
    )?;
    Ok(())
}

/// Pronunciation-range bounds for `token`.
fn pronunciation_range(token: Token) -> (Bound<Vec<u8>>, Bound<Vec<u8>>) {
    let lo = Bound::Included(codec::encode_token_bytes(token, &[]).to_vec());
    let hi = match token.checked_add(1) {
        Some(next) => Bound::Excluded(codec::encode_token_bytes(next, &[]).to_vec()),
        None => Bound::Unbounded,
    };
    (lo, hi)
}

fn collect_pronunciations_from_store(
    store: &impl OrderedStore,
    token: Token,
) -> Result<Vec<UserPronunciation>, UserStoreError> {
    let (lo, hi) = pronunciation_range(token);
    let mut out = Vec::new();
    store.range(
        PRONUNCIATION,
        lo.as_ref().map(|v| v.as_slice()),
        hi.as_ref().map(|v| v.as_slice()),
        &mut |key, value| {
            let (_, key_bytes) = codec::decode_token_bytes(key)
                .ok_or(StoreError::Backend("corrupt pronunciation key".into()))?;
            let count = codec::decode_u64(value)
                .ok_or(StoreError::Backend("corrupt pronunciation count".into()))?;
            out.push(UserPronunciation::new(
                phrase::decode_keys(key_bytes),
                count,
            ));
            Ok(())
        },
    )?;
    Ok(out)
}

fn collect_pronunciations_from_txn(
    txn: &dyn WriteTxn,
    token: Token,
) -> Result<Vec<(Vec<u8>, u64)>, StoreError> {
    let (lo, hi) = pronunciation_range(token);
    let mut out = Vec::new();
    txn.range(
        PRONUNCIATION,
        lo.as_ref().map(|v| v.as_slice()),
        hi.as_ref().map(|v| v.as_slice()),
        &mut |key, value| {
            let (_, key_bytes) = codec::decode_token_bytes(key)
                .ok_or(StoreError::Backend("corrupt pronunciation key".into()))?;
            let count = codec::decode_u64(value)
                .ok_or(StoreError::Backend("corrupt pronunciation count".into()))?;
            out.push((key_bytes.to_vec(), count));
            Ok(())
        },
    )?;
    Ok(out)
}

fn remove_pronunciations(txn: &mut dyn WriteTxn, token: Token) -> Result<(), StoreError> {
    let rows = collect_pronunciations_from_txn(txn, token)?;
    for (key_bytes, _) in rows {
        txn.remove(PRONUNCIATION, &codec::encode_token_bytes(token, &key_bytes))?;
    }
    Ok(())
}

/// Whether the store holds any user data, evaluated inside `txn`.
fn has_user_data_in_write_txn(txn: &dyn WriteTxn) -> Result<bool, StoreError> {
    let mut bigram_empty = true;
    txn.for_each(BIGRAM, &mut |_, _| {
        bigram_empty = false;
        Ok(())
    })?;
    if !bigram_empty {
        return Ok(true);
    }

    let mut unigram_empty = true;
    txn.for_each(UNIGRAM, &mut |_, _| {
        unigram_empty = false;
        Ok(())
    })?;
    if !unigram_empty {
        return Ok(true);
    }

    let mut phrase_empty = true;
    txn.for_each(PHRASE, &mut |_, _| {
        phrase_empty = false;
        Ok(())
    })?;
    if !phrase_empty {
        return Ok(true);
    }

    let mut pron_empty = true;
    txn.for_each(PRONUNCIATION, &mut |_, _| {
        pron_empty = false;
        Ok(())
    })?;
    Ok(!pron_empty)
}

// ── GenericUserStore ─────────────────────────────────────────────

/// An ordered-store-backed store of user-learning counts.
///
/// `Clone` shares the underlying database handle (cheap): the C ABI context
/// keeps the canonical store and hands each instance a clone, exactly like
/// the dictionary and language model handles. The handle and the §4
/// `m_modified` flag live on one `Arc`; the `Mutex` serializes the handle
/// because compaction (the `pinyin_save` write side) demands `&mut self`.
/// Clones record dirtiness through their own `&mut self` updates and the
/// context's `pinyin_save` observes it. The C ABI contract is
/// main-thread-only, so the flag uses relaxed ordering.
pub struct GenericUserStore<S: OrderedStore> {
    inner: Arc<StoreInner<S>>,
    /// Last field so [`RegistryLease`] drains after this handle's `Arc` dies.
    _lease: RegistryLease,
}

/// Default user store backed by [`DefaultStore`] (redb).
pub type UserStore = GenericUserStore<DefaultStore>;

impl<S: OrderedStore> Clone for GenericUserStore<S> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
            _lease: RegistryLease,
        }
    }
}

impl<S: OrderedStore> fmt::Debug for GenericUserStore<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GenericUserStore").finish_non_exhaustive()
    }
}

impl<S: OrderedStore> GenericUserStore<S> {
    /// Locks the shared store handle, recovering from a poisoned lock
    /// (constitution §4: nothing here panics, so a poisoned mutex must not
    /// brick the store either).
    fn database(&self) -> MutexGuard<'_, S> {
        self.inner
            .db
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn count_snapshot(&self) -> MutexGuard<'_, Option<CountSnapshot<S>>> {
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

    /// Opens one read snapshot for the count tables.
    ///
    /// Takes `database()` for the call and releases it before returning; the
    /// caller already holds the snapshot lock, which is the outer half of the
    /// one legal order (snapshot, then db).
    fn build_count_snapshot(&self, generation: u64) -> Result<CountSnapshot<S>, UserStoreError> {
        let db = self.database();
        let snap = db.snapshot()?;
        drop(db);
        Ok(CountSnapshot { generation, snap })
    }

    /// Runs `read` against the snapshot for the current write generation,
    /// rebuilding it first when a write has landed since it was cached.
    fn with_count_snapshot<T>(
        &self,
        read: impl FnOnce(&CountSnapshot<S>) -> Result<T, UserStoreError>,
    ) -> Result<T, UserStoreError> {
        let mut snapshot = self.count_snapshot();
        let generation = self.write_generation();
        let cached = match snapshot.take() {
            Some(cached) if cached.generation == generation => cached,
            _ => self.build_count_snapshot(generation)?,
        };
        let out = read(&cached);
        *snapshot = Some(cached);
        out
    }

    /// Invalidate cached reads after a committed user-data write.
    fn mark_committed_write(&self, db: MutexGuard<'_, S>, has_user_data: bool) {
        drop(db);
        *self.count_snapshot() = None;
        self.inner
            .has_user_data
            .store(has_user_data, Ordering::Release);
        self.inner.write_generation.fetch_add(1, Ordering::AcqRel);
    }

    fn init_and_wrap(db: S) -> Result<Arc<StoreInner<S>>, UserStoreError> {
        let has_user_data = db.write(|txn| {
            let total_key = codec::encode_u8(UNIGRAM_TOTAL_KEY);
            if txn.get(UNIGRAM_TOTAL, &total_key)?.is_none() {
                let mut sum = 0_u64;
                txn.for_each(UNIGRAM, &mut |_k, v| {
                    let delta = codec::decode_u64(v)
                        .ok_or(StoreError::Backend("corrupt unigram value".into()))?;
                    sum = sum.saturating_add(delta);
                    Ok(())
                })?;
                txn.put(UNIGRAM_TOTAL, &total_key, &codec::encode_u64(sum))?;
            }

            let alloc_key = codec::encode_u8(ALLOC_CURSOR);
            if txn.get(ALLOC, &alloc_key)?.is_none() {
                txn.put(ALLOC, &alloc_key, &codec::encode_token(FIRST_USER_TOKEN))?;
            }
            has_user_data_in_write_txn(txn)
        })?;

        Ok(Arc::new(StoreInner {
            count_snapshot: Mutex::new(None),
            db: Mutex::new(db),
            dirty: AtomicBool::new(false),
            write_generation: AtomicU64::new(0),
            has_user_data: AtomicBool::new(has_user_data),
        }))
    }

    /// Open a standalone store at `path` (no process-global dedup).
    #[cfg(test)]
    pub(crate) fn create(path: &Path) -> Result<Self, UserStoreError> {
        let db = S::create(path)?;
        let inner = Self::init_and_wrap(db)?;
        Ok(Self {
            inner,
            _lease: RegistryLease,
        })
    }

    /// Stored bigram count for `(prev, cur)`; `0` if unrecorded.
    pub fn bigram_count(&self, prev: Token, cur: Token) -> Result<u64, UserStoreError> {
        let db = self.database();
        get_u64_or(&*db, BIGRAM, &codec::encode_token_pair(prev, cur), 0)
    }

    /// Total bigram mass recorded after `prev`; `0` if none.
    pub fn bigram_total(&self, prev: Token) -> Result<u64, UserStoreError> {
        let db = self.database();
        get_u64_or(&*db, BIGRAM_TOTAL, &codec::encode_token(prev), 0)
    }

    /// Overwrite the raw `(prev -> cur)` user-bigram count.
    pub fn set_bigram_count(
        &mut self,
        prev: Token,
        cur: Token,
        count: u64,
    ) -> Result<(), UserStoreError> {
        let db = self.database();
        db.write(|txn| {
            let pair_key = codec::encode_token_pair(prev, cur);
            let prev_count = txn_get_u64_or(txn, BIGRAM, &pair_key, 0)?;
            txn.put(BIGRAM, &pair_key, &codec::encode_u64(count))?;

            let total_key = codec::encode_token(prev);
            let prev_total = txn_get_u64_or(txn, BIGRAM_TOTAL, &total_key, 0)?;
            let new_total = prev_total.saturating_sub(prev_count).saturating_add(count);
            txn.put(BIGRAM_TOTAL, &total_key, &codec::encode_u64(new_total))?;
            Ok(())
        })?;
        self.mark_committed_write(db, true);
        Ok(())
    }

    /// Accumulated phrase-index unigram delta for `token`; `0` if none.
    pub fn unigram_delta(&self, token: Token) -> Result<u64, UserStoreError> {
        if !self.has_user_data() {
            return Ok(0);
        }
        self.with_count_snapshot(|cached| cached.unigram_delta(token))
    }

    /// Sum of every stored unigram delta; `0` if the store is empty.
    pub fn unigram_total(&self) -> Result<u64, UserStoreError> {
        let db = self.database();
        get_u64_or(&*db, UNIGRAM_TOTAL, &codec::encode_u8(UNIGRAM_TOTAL_KEY), 0)
    }

    /// One-transaction §5 overlay for scoring `token` after `prev`.
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
    pub fn observe_selection(&mut self, last: Token, cur: Token) -> Result<u64, UserStoreError> {
        let seed = self.update(last, cur, SeedPolicy::Training)?;
        self.inner.dirty.store(true, Ordering::Relaxed);
        Ok(seed)
    }

    /// Record an accepted *predicted* candidate `cur` after `last` (the
    /// `pinyin_choose_predicted_candidate` path, §2). Returns the seed
    /// applied.
    pub fn observe_predicted(&mut self, last: Token, cur: Token) -> Result<u64, UserStoreError> {
        self.update(last, cur, SeedPolicy::Predicted)
    }

    /// Single atomic update: compute the seed under `policy`, then raise the
    /// bigram count for `(last, cur)` and `last`'s total by the seed, and
    /// `cur`'s unigram delta by `seed * 7`.
    fn update(
        &mut self,
        last: Token,
        cur: Token,
        policy: SeedPolicy,
    ) -> Result<u64, UserStoreError> {
        let db = self.database();
        let seed = db.write(|txn| {
            let pair_key = codec::encode_token_pair(last, cur);
            let prev = txn_get_u64_or(txn, BIGRAM, &pair_key, 0)?;
            let seed = match policy {
                SeedPolicy::Training => seed::training_seed((prev != 0).then_some(prev)),
                SeedPolicy::Predicted => seed::predicted_seed(),
            };
            txn.put(
                BIGRAM,
                &pair_key,
                &codec::encode_u64(prev.saturating_add(seed)),
            )?;

            let total_key = codec::encode_token(last);
            let prev_total = txn_get_u64_or(txn, BIGRAM_TOTAL, &total_key, 0)?;
            txn.put(
                BIGRAM_TOTAL,
                &total_key,
                &codec::encode_u64(prev_total.saturating_add(seed)),
            )?;

            let delta = seed::unigram_delta(seed);
            let uni_key = codec::encode_token(cur);
            let prev_unigram = txn_get_u64_or(txn, UNIGRAM, &uni_key, 0)?;
            txn.put(
                UNIGRAM,
                &uni_key,
                &codec::encode_u64(prev_unigram.saturating_add(delta)),
            )?;
            bump_unigram_total(txn, delta)?;

            Ok(seed)
        })?;
        self.mark_committed_write(db, true);
        Ok(seed)
    }

    /// Add a user phrase under [`crate::USER_DICTIONARY`] (`_add_phrase`, §3.2).
    pub fn add_phrase(
        &mut self,
        phrase: &str,
        keys: &[PinyinKey],
        count: Option<u64>,
    ) -> Result<Token, UserStoreError> {
        self.add_phrase_in(USER_DICTIONARY, phrase, keys, count)
    }

    /// Add a phrase under `library` (`USER_DICTIONARY` or `NETWORK_DICTIONARY`).
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
        let token = db.write(|txn| {
            let lib_key = codec::encode_u8_str(library, phrase);
            let existing = if let Some(bytes) = txn.get(PHRASE_BY_LIB_TEXT, &lib_key)? {
                Some(
                    codec::decode_token(&bytes)
                        .ok_or(StoreError::Backend("corrupt lib-text token".into()))?,
                )
            } else if library == USER_DICTIONARY {
                let text_key = codec::encode_str(phrase);
                match txn.get(PHRASE_BY_TEXT, text_key)? {
                    Some(bytes) => Some(
                        codec::decode_token(&bytes)
                            .ok_or(StoreError::Backend("corrupt text token".into()))?,
                    ),
                    None => None,
                }
            } else {
                None
            };

            if let Some(token) = existing {
                let pron_key = codec::encode_token_bytes(token, &key_bytes);
                let prev = txn_get_u64_or(txn, PRONUNCIATION, &pron_key, 0)?;
                txn.put(
                    PRONUNCIATION,
                    &pron_key,
                    &codec::encode_u64(prev.saturating_add(count)),
                )?;
                Ok(token)
            } else {
                let alloc_key = codec::encode_u8(ALLOC_CURSOR);
                let lib_alloc_key = codec::encode_u8(library);
                let raw = if library == USER_DICTIONARY {
                    match txn.get(ALLOC, &lib_alloc_key)? {
                        Some(bytes) => codec::decode_token(&bytes)
                            .ok_or(StoreError::Backend("corrupt alloc cursor".into()))?,
                        None => match txn.get(ALLOC, &alloc_key)? {
                            Some(bytes) => codec::decode_token(&bytes)
                                .ok_or(StoreError::Backend("corrupt alloc cursor".into()))?,
                            None => FIRST_USER_TOKEN,
                        },
                    }
                } else {
                    match txn.get(ALLOC, &lib_alloc_key)? {
                        Some(bytes) => codec::decode_token(&bytes)
                            .ok_or(StoreError::Backend("corrupt alloc cursor".into()))?,
                        None => first_library_token(library),
                    }
                };
                let token = phrase::canonicalize_library_token(library, raw)
                    .ok_or(StoreError::Backend("token space exhausted".into()))?;
                let next = phrase::next_library_token_after(library, token)
                    .ok_or(StoreError::Backend("token space exhausted".into()))?;
                txn.put(ALLOC, &lib_alloc_key, &codec::encode_token(next))?;
                if library == USER_DICTIONARY {
                    txn.put(ALLOC, &alloc_key, &codec::encode_token(next))?;
                }

                txn.put(
                    PHRASE,
                    &codec::encode_token(token),
                    codec::encode_str(phrase),
                )?;
                txn.put(PHRASE_BY_LIB_TEXT, &lib_key, &codec::encode_token(token))?;
                if library == USER_DICTIONARY {
                    txn.put(
                        PHRASE_BY_TEXT,
                        codec::encode_str(phrase),
                        &codec::encode_token(token),
                    )?;
                }

                let pron_key = codec::encode_token_bytes(token, &key_bytes);
                txn.put(PRONUNCIATION, &pron_key, &codec::encode_u64(count))?;

                let delta = count.saturating_mul(ADD_PHRASE_UNIGRAM_FACTOR);
                let uni_key = codec::encode_token(token);
                let prev = txn_get_u64_or(txn, UNIGRAM, &uni_key, 0)?;
                txn.put(
                    UNIGRAM,
                    &uni_key,
                    &codec::encode_u64(prev.saturating_add(delta)),
                )?;
                bump_unigram_total(txn, delta)?;
                Ok(token)
            }
        })?;
        self.mark_committed_write(db, true);
        Ok(token)
    }

    /// Promote a chosen addon phrase into the default-facade
    /// [`ADDON_DICTIONARY`] (nibble 5) sub-index.
    pub fn promote_addon_phrase(
        &mut self,
        phrase: &str,
        readings: &[(Vec<PinyinKey>, u64)],
        unigram: u64,
    ) -> Result<Token, UserStoreError> {
        let valid: Vec<&(Vec<PinyinKey>, u64)> = readings
            .iter()
            .filter(|(keys, _)| phrase::phrase_and_keys_valid(phrase, keys))
            .collect();
        if valid.is_empty() {
            return Err(UserStoreError::InvalidPhrase);
        }

        let db = self.database();
        let token = db.write(|txn| {
            let lib_key = codec::encode_u8_str(ADDON_DICTIONARY, phrase);
            let existing = match txn.get(PHRASE_BY_LIB_TEXT, &lib_key)? {
                Some(bytes) => Some(
                    codec::decode_token(&bytes)
                        .ok_or(StoreError::Backend("corrupt lib-text token".into()))?,
                ),
                None => None,
            };
            let token = if let Some(token) = existing {
                token
            } else {
                let lib_alloc_key = codec::encode_u8(ADDON_DICTIONARY);
                let raw = match txn.get(ALLOC, &lib_alloc_key)? {
                    Some(bytes) => codec::decode_token(&bytes)
                        .ok_or(StoreError::Backend("corrupt alloc cursor".into()))?,
                    None => first_library_token(ADDON_DICTIONARY),
                };
                let token = phrase::canonicalize_library_token(ADDON_DICTIONARY, raw)
                    .ok_or(StoreError::Backend("token space exhausted".into()))?;
                let next = phrase::next_library_token_after(ADDON_DICTIONARY, token)
                    .ok_or(StoreError::Backend("token space exhausted".into()))?;
                txn.put(ALLOC, &lib_alloc_key, &codec::encode_token(next))?;

                txn.put(
                    PHRASE,
                    &codec::encode_token(token),
                    codec::encode_str(phrase),
                )?;
                txn.put(PHRASE_BY_LIB_TEXT, &lib_key, &codec::encode_token(token))?;

                let uni_key = codec::encode_token(token);
                let prev = txn_get_u64_or(txn, UNIGRAM, &uni_key, 0)?;
                txn.put(
                    UNIGRAM,
                    &uni_key,
                    &codec::encode_u64(prev.saturating_add(unigram)),
                )?;
                bump_unigram_total(txn, unigram)?;
                token
            };

            for (keys, count) in valid {
                let key_bytes = phrase::encode_keys(keys);
                let pron_key = codec::encode_token_bytes(token, &key_bytes);
                let prev = txn_get_u64_or(txn, PRONUNCIATION, &pron_key, 0)?;
                txn.put(
                    PRONUNCIATION,
                    &pron_key,
                    &codec::encode_u64(prev.saturating_add(*count)),
                )?;
            }
            Ok(token)
        })?;
        self.mark_committed_write(db, true);
        Ok(token)
    }

    /// Phrase text and pronunciations for `token`, if this store owns it.
    pub fn phrase(&self, token: Token) -> Result<Option<UserPhrase>, UserStoreError> {
        let db = self.database();
        let token_key = codec::encode_token(token);
        let Some(text_bytes) = db.get(PHRASE, &token_key)? else {
            return Ok(None);
        };
        let text = codec::decode_str(&text_bytes)
            .ok_or(UserStoreError::Decode)?
            .to_owned();
        let pronunciations = collect_pronunciations_from_store(&*db, token)?;
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
        let lib_key = codec::encode_u8_str(library, phrase);
        if let Some(bytes) = db.get(PHRASE_BY_LIB_TEXT, &lib_key)? {
            return codec::decode_token(&bytes)
                .map(Some)
                .ok_or(UserStoreError::Decode);
        }
        if library == USER_DICTIONARY {
            let text_key = codec::encode_str(phrase);
            if let Some(bytes) = db.get(PHRASE_BY_TEXT, text_key)? {
                return codec::decode_token(&bytes)
                    .map(Some)
                    .ok_or(UserStoreError::Decode);
            }
        }
        Ok(None)
    }

    /// Current write generation; [`UserLookup`] rebuilds when this changes.
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.write_generation()
    }

    /// Next token the store will allocate.
    pub fn next_user_token(&self) -> Result<Token, UserStoreError> {
        let db = self.database();
        let alloc_key = codec::encode_u8(ALLOC_CURSOR);
        match db.get(ALLOC, &alloc_key)? {
            Some(bytes) => codec::decode_token(&bytes).ok_or(UserStoreError::Decode),
            None => Ok(FIRST_USER_TOKEN),
        }
    }

    /// `m_modified` (§4).
    #[must_use]
    pub fn is_modified(&self) -> bool {
        self.inner.dirty.load(Ordering::Relaxed)
    }

    /// Arm `m_modified` without a data write.
    pub fn mark_modified(&mut self) {
        self.inner.dirty.store(true, Ordering::Relaxed);
    }

    /// Every user phrase as §9 export rows.
    pub fn export_phrases(&self) -> Result<Vec<ExportedPhrase>, UserStoreError> {
        self.export_phrases_in(USER_DICTIONARY)
    }

    /// Export rows for one `USER_FILE` nibble.
    pub fn export_phrases_in(&self, library: u8) -> Result<Vec<ExportedPhrase>, UserStoreError> {
        let db = self.database();
        let mut rows = Vec::new();
        db.for_each(PHRASE, &mut |k, v| {
            let token =
                codec::decode_token(k).ok_or(StoreError::Backend("corrupt phrase token".into()))?;
            if phrase_index_library_index(token) != library {
                return Ok(());
            }
            let text =
                codec::decode_str(v).ok_or(StoreError::Backend("corrupt phrase text".into()))?;
            // Collect pronunciations inline (we cannot call
            // collect_pronunciations_from_store here because we don't have a
            // &DefaultStore — we have a &dyn FnMut, but we need the store).
            // Instead, store (token, text) and collect after.
            rows.push((token, text.to_owned()));
            Ok(())
        })?;
        let mut out = Vec::new();
        for (token, text) in rows {
            for pronunciation in collect_pronunciations_from_store(&*db, token)? {
                let Some(pinyin) = pronunciation.render_pinyin() else {
                    continue;
                };
                out.push(ExportedPhrase {
                    text: text.clone(),
                    pinyin,
                    count: pronunciation.count(),
                });
            }
        }
        Ok(out)
    }

    /// Every stored phrase (user and network) with pronunciations.
    pub fn phrases(&self) -> Result<Vec<UserPhrase>, UserStoreError> {
        let db = self.database();
        let mut tokens_and_texts = Vec::new();
        db.for_each(PHRASE, &mut |k, v| {
            let token =
                codec::decode_token(k).ok_or(StoreError::Backend("corrupt phrase token".into()))?;
            let text = codec::decode_str(v)
                .ok_or(StoreError::Backend("corrupt phrase text".into()))?
                .to_owned();
            tokens_and_texts.push((token, text));
            Ok(())
        })?;
        let mut out = Vec::new();
        for (token, text) in tokens_and_texts {
            let pronunciations = collect_pronunciations_from_store(&*db, token)?;
            out.push(UserPhrase::new(token, text, pronunciations));
        }
        Ok(out)
    }

    /// User-bigram successors of `prev` as `(token, count)` pairs.
    pub fn bigram_successors(&self, prev: Token) -> Result<Vec<(Token, u64)>, UserStoreError> {
        let db = self.database();
        let lo = codec::encode_token_pair(prev, Token::MIN);
        let hi = codec::encode_token_pair(prev, Token::MAX);
        let mut rows = Vec::new();
        db.range(
            BIGRAM,
            Bound::Included(lo.as_slice()),
            Bound::Included(hi.as_slice()),
            &mut |k, v| {
                let (_, cur) = codec::decode_token_pair(k)
                    .ok_or(StoreError::Backend("corrupt bigram key".into()))?;
                let count = codec::decode_u64(v)
                    .ok_or(StoreError::Backend("corrupt bigram count".into()))?;
                rows.push((cur, count));
                Ok(())
            },
        )?;
        Ok(rows)
    }

    /// Every stored user-bigram row as `(prev, cur, count)`, raw.
    pub fn export_bigrams(&self) -> Result<Vec<(Token, Token, u64)>, UserStoreError> {
        let db = self.database();
        let mut rows = Vec::new();
        db.for_each(BIGRAM, &mut |k, v| {
            let (prev, cur) = codec::decode_token_pair(k)
                .ok_or(StoreError::Backend("corrupt bigram key".into()))?;
            let count =
                codec::decode_u64(v).ok_or(StoreError::Backend("corrupt bigram count".into()))?;
            rows.push((prev, cur, count));
            Ok(())
        })?;
        Ok(rows)
    }

    /// The `pinyin_save` write side (§4).
    pub fn save(&mut self) -> Result<bool, UserStoreError> {
        if !self.is_modified() {
            return Ok(false);
        }
        let mut snapshot = self.count_snapshot();
        *snapshot = None;
        let mut db = self.database();
        db.compact()?;
        drop(db);
        drop(snapshot);
        self.inner.dirty.store(false, Ordering::Relaxed);
        Ok(true)
    }

    /// `pinyin_mask_out`'s store side.
    pub fn mask_out(&mut self, mask: Token, value: Token) -> Result<(), UserStoreError> {
        let db = self.database();
        let has_user_data = db.write(|txn| {
            // Bigram: collect all rows, then remove matching and rewrite totals.
            let mut bigram_rows: Vec<((Token, Token), u64)> = Vec::new();
            txn.for_each(BIGRAM, &mut |k, v| {
                let (prev, cur) = codec::decode_token_pair(k)
                    .ok_or(StoreError::Backend("corrupt bigram key".into()))?;
                let count = codec::decode_u64(v)
                    .ok_or(StoreError::Backend("corrupt bigram count".into()))?;
                bigram_rows.push(((prev, cur), count));
                Ok(())
            })?;

            let mut survivors: std::collections::BTreeMap<Token, u64> =
                std::collections::BTreeMap::new();
            for ((prev, cur), count) in &bigram_rows {
                if (prev & mask) == value || (cur & mask) == value {
                    txn.remove(BIGRAM, &codec::encode_token_pair(*prev, *cur))?;
                } else {
                    let slot = survivors.entry(*prev).or_default();
                    *slot = slot.saturating_add(*count);
                }
            }

            let mut old_totals: Vec<Token> = Vec::new();
            txn.for_each(BIGRAM_TOTAL, &mut |k, _v| {
                let prev = codec::decode_token(k)
                    .ok_or(StoreError::Backend("corrupt bigram_total key".into()))?;
                old_totals.push(prev);
                Ok(())
            })?;
            for prev in old_totals {
                txn.remove(BIGRAM_TOTAL, &codec::encode_token(prev))?;
            }
            for (prev, total) in survivors {
                if total > 0 {
                    txn.put(
                        BIGRAM_TOTAL,
                        &codec::encode_token(prev),
                        &codec::encode_u64(total),
                    )?;
                }
            }

            // Unigram deltas and their running total.
            let mut unigram_rows: Vec<(Token, u64)> = Vec::new();
            txn.for_each(UNIGRAM, &mut |k, v| {
                let token = codec::decode_token(k)
                    .ok_or(StoreError::Backend("corrupt unigram key".into()))?;
                let delta = codec::decode_u64(v)
                    .ok_or(StoreError::Backend("corrupt unigram value".into()))?;
                unigram_rows.push((token, delta));
                Ok(())
            })?;
            let mut kept_sum = 0_u64;
            for (token, delta) in &unigram_rows {
                if (token & mask) == value {
                    txn.remove(UNIGRAM, &codec::encode_token(*token))?;
                } else {
                    kept_sum = kept_sum.saturating_add(*delta);
                }
            }
            txn.put(
                UNIGRAM_TOTAL,
                &codec::encode_u8(UNIGRAM_TOTAL_KEY),
                &codec::encode_u64(kept_sum),
            )?;

            // User phrases: text, reverse lookup, and pronunciations.
            let mut matched: Vec<(Token, String)> = Vec::new();
            txn.for_each(PHRASE, &mut |k, v| {
                let token = codec::decode_token(k)
                    .ok_or(StoreError::Backend("corrupt phrase token".into()))?;
                if (token & mask) == value {
                    let text = codec::decode_str(v)
                        .ok_or(StoreError::Backend("corrupt phrase text".into()))?
                        .to_owned();
                    matched.push((token, text));
                }
                Ok(())
            })?;
            for (token, text) in matched {
                txn.remove(PHRASE, &codec::encode_token(token))?;
                txn.remove(PHRASE_BY_TEXT, codec::encode_str(&text))?;
                txn.remove(
                    PHRASE_BY_LIB_TEXT,
                    &codec::encode_u8_str(phrase_index_library_index(token), &text),
                )?;
                remove_pronunciations(txn, token)?;
            }
            has_user_data_in_write_txn(txn)
        })?;
        self.mark_committed_write(db, has_user_data);
        Ok(())
    }

    /// `pinyin_remove_user_candidate`'s store side (§3.4).
    pub fn remove_user_phrase(&mut self, token: Token) -> Result<bool, UserStoreError> {
        let db = self.database();
        let result: Result<Option<bool>, UserStoreError> = db
            .write(|txn| {
                let token_key = codec::encode_token(token);
                let Some(text_bytes) = txn.get(PHRASE, &token_key)? else {
                    return Ok(None);
                };
                let text = codec::decode_str(&text_bytes)
                    .ok_or(StoreError::Backend("corrupt phrase text".into()))?
                    .to_owned();

                txn.remove(PHRASE, &token_key)?;
                txn.remove(PHRASE_BY_TEXT, codec::encode_str(&text))?;
                txn.remove(
                    PHRASE_BY_LIB_TEXT,
                    &codec::encode_u8_str(phrase_index_library_index(token), &text),
                )?;
                remove_pronunciations(txn, token)?;

                // Bigram: collect, remove matching, rewrite totals.
                let mut bigram_rows: Vec<((Token, Token), u64)> = Vec::new();
                txn.for_each(BIGRAM, &mut |k, v| {
                    let (prev, cur) = codec::decode_token_pair(k)
                        .ok_or(StoreError::Backend("corrupt bigram key".into()))?;
                    let count = codec::decode_u64(v)
                        .ok_or(StoreError::Backend("corrupt bigram count".into()))?;
                    bigram_rows.push(((prev, cur), count));
                    Ok(())
                })?;

                let mut survivors: std::collections::BTreeMap<Token, u64> =
                    std::collections::BTreeMap::new();
                for ((prev, cur), count) in &bigram_rows {
                    if *prev == token || *cur == token {
                        txn.remove(BIGRAM, &codec::encode_token_pair(*prev, *cur))?;
                    } else {
                        let slot = survivors.entry(*prev).or_default();
                        *slot = slot.saturating_add(*count);
                    }
                }

                let mut old_totals: Vec<Token> = Vec::new();
                txn.for_each(BIGRAM_TOTAL, &mut |k, _v| {
                    let prev = codec::decode_token(k)
                        .ok_or(StoreError::Backend("corrupt bigram_total key".into()))?;
                    old_totals.push(prev);
                    Ok(())
                })?;
                for prev in old_totals {
                    txn.remove(BIGRAM_TOTAL, &codec::encode_token(prev))?;
                }
                for (prev, total) in survivors {
                    if total > 0 {
                        txn.put(
                            BIGRAM_TOTAL,
                            &codec::encode_token(prev),
                            &codec::encode_u64(total),
                        )?;
                    }
                }

                // Unigram: recompute total excluding the removed token.
                let mut kept_sum = 0_u64;
                txn.for_each(UNIGRAM, &mut |k, v| {
                    let candidate = codec::decode_token(k)
                        .ok_or(StoreError::Backend("corrupt unigram key".into()))?;
                    if candidate != token {
                        let delta = codec::decode_u64(v)
                            .ok_or(StoreError::Backend("corrupt unigram value".into()))?;
                        kept_sum = kept_sum.saturating_add(delta);
                    }
                    Ok(())
                })?;
                txn.remove(UNIGRAM, &codec::encode_token(token))?;
                txn.put(
                    UNIGRAM_TOTAL,
                    &codec::encode_u8(UNIGRAM_TOTAL_KEY),
                    &codec::encode_u64(kept_sum),
                )?;

                let has = has_user_data_in_write_txn(txn)?;
                Ok(Some(has))
            })
            .map_err(UserStoreError::from);
        match result? {
            None => Ok(false),
            Some(has_user_data) => {
                self.mark_committed_write(db, has_user_data);
                Ok(true)
            }
        }
    }
}

impl GenericUserStore<DefaultStore> {
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
    /// second database handle.
    pub fn open(path: &Path) -> Result<Self, UserStoreError> {
        let key = registry::registry_key(path);
        let mut reg = registry::lock_registry();
        if let Some(inner) = reg.get(&key).and_then(|handle| handle.upgrade()) {
            return Ok(Self {
                inner,
                _lease: RegistryLease,
            });
        }

        let db = DefaultStore::create(path)?;
        let inner = Self::init_and_wrap(db)?;
        reg.insert(key, Arc::downgrade(&inner));
        Ok(Self {
            inner,
            _lease: RegistryLease,
        })
    }
}

impl<S: OrderedStore> CountSnapshot<S> {
    fn unigram_delta(&self, token: Token) -> Result<u64, UserStoreError> {
        snap_get_u64_or(&self.snap, UNIGRAM, &codec::encode_token(token), 0)
    }

    fn count_delta(
        &self,
        prev: Option<Token>,
        token: Token,
    ) -> Result<UserCountDelta, UserStoreError> {
        let unigram_delta = snap_get_u64_or(&self.snap, UNIGRAM, &codec::encode_token(token), 0)?;
        let unigram_total_delta = snap_get_u64_or(
            &self.snap,
            UNIGRAM_TOTAL,
            &codec::encode_u8(UNIGRAM_TOTAL_KEY),
            0,
        )?;
        let (bigram_count, bigram_total) = if let Some(prev) = prev {
            (
                snap_get_u64_or(
                    &self.snap,
                    BIGRAM,
                    &codec::encode_token_pair(prev, token),
                    0,
                )?,
                snap_get_u64_or(&self.snap, BIGRAM_TOTAL, &codec::encode_token(prev), 0)?,
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

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! user_store_tests {
        ($mod:ident, $backend:ty, $ext:literal) => {
            mod $mod {
                use super::super::*;
                use crate::{PHRASE_INDEX_LIBRARY_MASK, USER_DICTIONARY, phrase_index_make_token};
                use oxpinyin_core::SyllableKey;

                type Store = GenericUserStore<$backend>;

                fn temp_path(tag: &str) -> std::path::PathBuf {
                    let path = std::env::temp_dir().join(format!(
                        "oxpinyin-user-{}-{tag}-{}.{}",
                        $ext,
                        std::process::id(),
                        $ext,
                    ));
                    let _ = std::fs::remove_file(&path);
                    path
                }

                fn cleanup(path: &std::path::Path) {
                    let _ = std::fs::remove_file(path);
                    let mut lock = path.as_os_str().to_os_string();
                    lock.push("-lock");
                    let _ = std::fs::remove_file(std::path::Path::new(&lock));
                }

                #[test]
                fn open_creates_empty_store() {
                    let path = temp_path("empty");
                    let store = Store::create(&path).unwrap();
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
                        "empty count_delta must not open a read transaction"
                    );
                    cleanup(&path);
                }

                #[test]
                fn cached_count_delta_refreshes_after_committed_writes() {
                    let path = temp_path("cache-refresh");
                    let mut store = Store::create(&path).unwrap();

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
                    cleanup(&path);
                }

                #[test]
                fn mask_out_all_marks_store_empty_again() {
                    let path = temp_path("empty-again");
                    let mut store = Store::create(&path).unwrap();
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
                    cleanup(&path);
                }

                #[test]
                fn reopen_of_populated_store_sets_has_user_data() {
                    let path = temp_path("reopen-populated");
                    {
                        let mut store = Store::create(&path).unwrap();
                        store.observe_selection(1, 100).unwrap();
                    }
                    let store = Store::create(&path).unwrap();
                    assert!(store.has_user_data());
                    assert_eq!(store.count_delta(Some(1), 100).unwrap().bigram_count, 69);
                    cleanup(&path);
                }

                #[test]
                fn save_compacts_after_a_cached_read() {
                    let path = temp_path("save-after-cache");
                    let mut store = Store::create(&path).unwrap();
                    store.observe_selection(1, 100).unwrap();
                    assert_eq!(store.count_delta(Some(1), 100).unwrap().bigram_count, 69);
                    assert!(store.save().unwrap());
                    assert_eq!(store.count_delta(Some(1), 100).unwrap().bigram_count, 69);
                    cleanup(&path);
                }

                #[test]
                fn observe_applies_pinned_seed_sequence() {
                    let path = temp_path("seq");
                    let mut store = Store::create(&path).unwrap();

                    assert_eq!(store.observe_selection(1, 100).unwrap(), 69);
                    assert_eq!(store.bigram_count(1, 100).unwrap(), 69);
                    assert_eq!(store.bigram_total(1).unwrap(), 69);
                    assert_eq!(store.unigram_delta(100).unwrap(), 483);
                    assert_eq!(store.unigram_total().unwrap(), 483);

                    assert_eq!(store.observe_selection(1, 100).unwrap(), 138);
                    assert_eq!(store.bigram_count(1, 100).unwrap(), 207);
                    assert_eq!(store.bigram_total(1).unwrap(), 207);
                    assert_eq!(store.unigram_delta(100).unwrap(), 483 + 966);
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
                    assert_eq!(
                        store.count_delta(None, 100).unwrap(),
                        UserCountDelta {
                            bigram_count: 0,
                            bigram_total: 0,
                            unigram_delta: 483 + 966,
                            unigram_total_delta: 483 + 966,
                        }
                    );

                    cleanup(&path);
                }

                #[test]
                fn totals_accumulate_per_predecessor() {
                    let path = temp_path("totals");
                    let mut store = Store::create(&path).unwrap();
                    assert_eq!(store.observe_selection(5, 10).unwrap(), 69);
                    assert_eq!(store.observe_selection(5, 11).unwrap(), 69);
                    assert_eq!(store.bigram_count(5, 10).unwrap(), 69);
                    assert_eq!(store.bigram_count(5, 11).unwrap(), 69);
                    assert_eq!(store.bigram_total(5).unwrap(), 138);
                    cleanup(&path);
                }

                #[test]
                fn set_bigram_count_plants_filter_edge() {
                    let path = temp_path("plant-edge");
                    let mut store = Store::create(&path).unwrap();
                    store.set_bigram_count(1, 10, 9).unwrap();
                    store.set_bigram_count(1, 11, 10).unwrap();
                    assert_eq!(store.bigram_count(1, 10).unwrap(), 9);
                    assert_eq!(store.bigram_count(1, 11).unwrap(), 10);
                    assert_eq!(store.bigram_total(1).unwrap(), 19);
                    cleanup(&path);
                }

                #[test]
                fn predicted_path_is_flat_69() {
                    let path = temp_path("pred");
                    let mut store = Store::create(&path).unwrap();
                    assert_eq!(store.observe_predicted(1, 200).unwrap(), 69);
                    assert_eq!(store.observe_predicted(1, 200).unwrap(), 69);
                    assert_eq!(store.bigram_count(1, 200).unwrap(), 138);
                    assert_eq!(store.bigram_total(1).unwrap(), 138);
                    assert_eq!(store.unigram_delta(200).unwrap(), 483 * 2);
                    cleanup(&path);
                }

                #[test]
                fn roundtrip_reopen_reads_identical() {
                    let path = temp_path("roundtrip");
                    {
                        let mut store = Store::create(&path).unwrap();
                        store.observe_selection(SENTENCE_START, 10).unwrap();
                        store.observe_selection(10, 20).unwrap();
                        store.observe_selection(10, 20).unwrap();
                    }

                    let store = Store::create(&path).unwrap();
                    assert_eq!(store.bigram_count(SENTENCE_START, 10).unwrap(), 69);
                    assert_eq!(store.bigram_count(10, 20).unwrap(), 207);
                    assert_eq!(store.bigram_total(SENTENCE_START).unwrap(), 69);
                    assert_eq!(store.bigram_total(10).unwrap(), 207);
                    assert_eq!(store.unigram_delta(10).unwrap(), 483);
                    assert_eq!(store.unigram_delta(20).unwrap(), 1449);
                    cleanup(&path);
                }

                #[test]
                fn dirty_gate_matches_m_modified_semantics() {
                    let path = temp_path("dirty");
                    let mut store = Store::create(&path).unwrap();

                    assert!(!store.is_modified());
                    assert!(!store.save().unwrap());
                    assert!(!store.save().unwrap());

                    store.observe_selection(SENTENCE_START, 10).unwrap();
                    assert!(store.is_modified());
                    assert!(store.save().unwrap());
                    assert!(!store.is_modified());
                    assert!(!store.save().unwrap());

                    store.observe_predicted(10, 20).unwrap();
                    assert!(!store.is_modified());
                    assert!(!store.save().unwrap());
                    store.add_phrase("你好", &[1, 2], None).unwrap();
                    assert!(!store.is_modified());
                    assert!(!store.save().unwrap());

                    store.observe_selection(SENTENCE_START, 10).unwrap();
                    assert!(store.is_modified());
                    assert!(store.save().unwrap());
                    assert!(!store.is_modified());

                    cleanup(&path);
                }

                #[test]
                fn save_reopen_roundtrip_preserves_counts_cursor_and_total() {
                    let path = temp_path("save-rt");
                    {
                        let mut store = Store::create(&path).unwrap();
                        store.observe_selection(SENTENCE_START, 10).unwrap();
                        store.observe_selection(10, 20).unwrap();
                        store.observe_selection(10, 20).unwrap();
                        let token = store.add_phrase("你好", &[10, 20], None).unwrap();
                        assert_eq!(token, FIRST_USER_TOKEN);
                        assert!(store.is_modified());
                        assert!(store.save().unwrap());
                        assert!(!store.is_modified());
                    }

                    let mut store = Store::create(&path).unwrap();
                    assert!(!store.is_modified());
                    assert!(!store.save().unwrap(), "a reopen starts clean");

                    assert_eq!(store.bigram_count(SENTENCE_START, 10).unwrap(), 69);
                    assert_eq!(store.bigram_count(10, 20).unwrap(), 207);
                    assert_eq!(store.bigram_total(SENTENCE_START).unwrap(), 69);
                    assert_eq!(store.bigram_total(10).unwrap(), 207);
                    assert_eq!(store.unigram_delta(10).unwrap(), 483);
                    assert_eq!(store.unigram_delta(20).unwrap(), 1449);
                    assert_eq!(store.unigram_total().unwrap(), 483 + 1449 + 15);
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
                    cleanup(&path);
                }

                #[test]
                fn first_allocation_is_first_user_token() {
                    let path = temp_path("first-tok");
                    let mut store = Store::create(&path).unwrap();
                    assert_eq!(store.next_user_token().unwrap(), FIRST_USER_TOKEN);
                    let token = store.add_phrase("你好", &[10, 20], None).unwrap();
                    assert_eq!(token, FIRST_USER_TOKEN);
                    assert_eq!(token, 0x0700_0001);
                    assert!(phrase::is_user_token(token));
                    assert_eq!(store.next_user_token().unwrap(), FIRST_USER_TOKEN + 1);
                    cleanup(&path);
                }

                #[test]
                fn allocation_increments_by_one_without_gap() {
                    let path = temp_path("incr");
                    let mut store = Store::create(&path).unwrap();
                    let a = store.add_phrase("甲", &[1], None).unwrap();
                    let b = store.add_phrase("乙", &[2], None).unwrap();
                    let c = store.add_phrase("丙", &[3], None).unwrap();
                    assert_eq!(a, FIRST_USER_TOKEN);
                    assert_eq!(b, a + 1);
                    assert_eq!(c, a + 2);
                    assert_eq!(store.next_user_token().unwrap(), a + 3);
                    cleanup(&path);
                }

                #[test]
                fn user_token_is_distinguishable_from_system_token() {
                    let path = temp_path("nibble");
                    let mut store = Store::create(&path).unwrap();
                    let user = store.add_phrase("词", &[7], None).unwrap();
                    const SYSTEM: Token = 0x0100_0001;
                    assert!(phrase::is_user_token(user));
                    assert!(!phrase::is_user_token(SYSTEM));
                    cleanup(&path);
                }

                #[test]
                fn network_and_user_can_share_phrase_text() {
                    let path = temp_path("two-nibbles");
                    let mut store = Store::create(&path).unwrap();
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
                    cleanup(&path);
                }

                #[test]
                fn add_phrase_seeds_unigram_with_count_times_three() {
                    let path = temp_path("uni");
                    let mut store = Store::create(&path).unwrap();
                    let token = store.add_phrase("你好", &[10, 20], None).unwrap();
                    assert_eq!(store.unigram_delta(token).unwrap(), 15);
                    assert_eq!(store.bigram_count(SENTENCE_START, token).unwrap(), 0);

                    let token2 = store.add_phrase("世界", &[30, 40], Some(10)).unwrap();
                    assert_eq!(store.unigram_delta(token2).unwrap(), 30);
                    cleanup(&path);
                }

                #[test]
                fn existing_phrase_merges_a_new_reading() {
                    let path = temp_path("merge");
                    let mut store = Store::create(&path).unwrap();
                    let first = store.add_phrase("你好", &[10, 20], None).unwrap();
                    let again = store.add_phrase("你好", &[11, 20], Some(8)).unwrap();
                    assert_eq!(first, again);
                    assert_eq!(store.next_user_token().unwrap(), FIRST_USER_TOKEN + 1);
                    assert_eq!(store.unigram_delta(first).unwrap(), 15);

                    let got = store.phrase(first).unwrap().unwrap();
                    assert_eq!(got.text(), "你好");
                    assert_eq!(got.pronunciations().len(), 2);
                    assert_eq!(got.pronunciations()[0].keys(), &[10, 20]);
                    assert_eq!(got.pronunciations()[0].count(), 5);
                    assert_eq!(got.pronunciations()[1].keys(), &[11, 20]);
                    assert_eq!(got.pronunciations()[1].count(), 8);
                    cleanup(&path);
                }

                #[test]
                fn same_reading_accumulates_pronunciation_count() {
                    let path = temp_path("same-read");
                    let mut store = Store::create(&path).unwrap();
                    let token = store.add_phrase("词", &[7], Some(5)).unwrap();
                    let again = store.add_phrase("词", &[7], Some(5)).unwrap();
                    assert_eq!(token, again);
                    assert_eq!(store.unigram_delta(token).unwrap(), 15);
                    let got = store.phrase(token).unwrap().unwrap();
                    assert_eq!(got.pronunciations().len(), 1);
                    assert_eq!(got.pronunciations()[0].count(), 10);
                    cleanup(&path);
                }

                #[test]
                fn phrase_roundtrip_reopen_preserves_cursor() {
                    let path = temp_path("phrase-rt");
                    let (t1, t2, next) = {
                        let mut store = Store::create(&path).unwrap();
                        let t1 = store.add_phrase("你好", &[10, 20], None).unwrap();
                        let t2 = store.add_phrase("世界", &[30, 40], Some(9)).unwrap();
                        store.add_phrase("你好", &[11, 20], Some(2)).unwrap();
                        (t1, t2, store.next_user_token().unwrap())
                    };

                    let store = Store::create(&path).unwrap();
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

                    let mut store = store;
                    let t3 = store.add_phrase("中国", &[50, 60], None).unwrap();
                    assert_eq!(t3, t2 + 1);
                    assert_eq!(t3, FIRST_USER_TOKEN + 2);
                    cleanup(&path);
                }

                #[test]
                fn invalid_phrase_is_rejected_without_allocation() {
                    let path = temp_path("invalid");
                    let mut store = Store::create(&path).unwrap();
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
                    cleanup(&path);
                }

                #[test]
                fn lookup_of_unknown_token_is_none() {
                    let path = temp_path("miss");
                    let store = Store::create(&path).unwrap();
                    assert!(store.phrase(FIRST_USER_TOKEN).unwrap().is_none());
                    assert!(store.phrase(0x0100_0001).unwrap().is_none());
                    cleanup(&path);
                }

                #[test]
                fn export_phrases_render_the_pinned_triples() {
                    let path = temp_path("export-phrases");
                    let mut store = Store::create(&path).unwrap();
                    let ni = SyllableKey::from_text("ni").expect("frozen key").index() as u16;
                    let hao = SyllableKey::from_text("hao").expect("frozen key").index() as u16;
                    let shi = SyllableKey::from_text("shi").expect("frozen key").index() as u16;
                    let jie = SyllableKey::from_text("jie").expect("frozen key").index() as u16;

                    store.add_phrase("你好", &[ni, hao], None).unwrap();
                    store.add_phrase("你好", &[ni, hao], Some(7)).unwrap();
                    store.add_phrase("世界", &[shi, jie], Some(3)).unwrap();

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
                    cleanup(&path);
                }

                #[test]
                fn export_bigrams_lists_every_stored_row_raw() {
                    let path = temp_path("export-bigrams");
                    let mut store = Store::create(&path).unwrap();
                    store.observe_selection(SENTENCE_START, 10).unwrap();
                    store.observe_selection(10, 20).unwrap();
                    store.observe_selection(10, 20).unwrap();

                    let mut rows = store.export_bigrams().unwrap();
                    rows.sort();
                    assert_eq!(rows, vec![(SENTENCE_START, 10, 69), (10, 20, 207)]);
                    cleanup(&path);
                }

                fn mixed_store(path: &std::path::Path) -> (Store, Token) {
                    const SYSTEM_A: Token = 0x0100_0001;
                    const SYSTEM_B: Token = 0x0200_0001;
                    let mut store = Store::create(path).unwrap();
                    let user_a = store.add_phrase("你好", &[10, 20], None).unwrap();
                    let user_b = store.add_phrase("世界", &[30, 40], None).unwrap();
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
                    assert!(store.is_modified());
                    assert!(store.save().unwrap());

                    store
                        .mask_out(
                            PHRASE_INDEX_LIBRARY_MASK,
                            phrase_index_make_token(USER_DICTIONARY, 0),
                        )
                        .unwrap();

                    assert!(store.phrase(user_a).unwrap().is_none());
                    assert!(store.token_for_phrase("你好").unwrap().is_none());
                    assert_eq!(store.bigram_count(0x0100_0001, 0x0200_0001).unwrap(), 69);
                    assert_eq!(store.bigram_total(0x0100_0001).unwrap(), 69);
                    assert_eq!(store.bigram_count(user_a, 0x0200_0001).unwrap(), 0);
                    assert_eq!(store.bigram_count(0x0100_0001, user_a).unwrap(), 0);
                    assert_eq!(store.bigram_count(user_a, user_a + 1).unwrap(), 0);
                    assert_eq!(store.unigram_delta(0x0200_0001).unwrap(), 966);
                    assert_eq!(store.unigram_delta(user_a).unwrap(), 0);
                    assert_eq!(store.unigram_total().unwrap(), 966);
                    assert_eq!(store.next_user_token().unwrap(), user_a + 2);
                    assert!(!store.is_modified());
                    assert!(!store.save().unwrap());

                    store.mask_out(0x0, 0x0).unwrap();
                    assert_eq!(store.bigram_count(0x0100_0001, 0x0200_0001).unwrap(), 0);
                    assert_eq!(store.bigram_total(0x0100_0001).unwrap(), 0);
                    assert_eq!(store.unigram_total().unwrap(), 0);
                    assert!(!store.is_modified());
                    cleanup(&path);
                }

                #[test]
                fn remove_user_phrase_deletes_everywhere_and_rejects_others() {
                    let path = temp_path("remove");
                    let (mut store, user_a) = mixed_store(&path);
                    assert!(store.save().unwrap());

                    assert!(store.remove_user_phrase(user_a).unwrap());
                    assert!(store.phrase(user_a).unwrap().is_none());
                    assert!(store.token_for_phrase("你好").unwrap().is_none());
                    assert_eq!(store.bigram_count(user_a, 0x0200_0001).unwrap(), 0);
                    assert_eq!(store.bigram_count(0x0100_0001, user_a).unwrap(), 0);
                    assert_eq!(store.bigram_count(user_a, user_a + 1).unwrap(), 0);
                    assert_eq!(store.bigram_count(0x0100_0001, 0x0200_0001).unwrap(), 69);
                    assert_eq!(store.bigram_total(0x0100_0001).unwrap(), 69);
                    assert_eq!(store.bigram_total(user_a).unwrap(), 0);
                    assert_eq!(store.unigram_delta(user_a).unwrap(), 0);
                    assert_eq!(store.unigram_total().unwrap(), 966 + 498);
                    assert!(!store.is_modified());
                    assert!(!store.remove_user_phrase(user_a).unwrap());
                    assert!(!store.remove_user_phrase(0x0100_0001).unwrap());
                    cleanup(&path);
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

                    let store = Store::create(&path).unwrap();
                    assert!(store.token_for_phrase("你好").unwrap().is_none());
                    assert!(store.token_for_phrase("中国").unwrap().is_none());
                    assert_eq!(store.bigram_count(0x0100_0001, 0x0200_0001).unwrap(), 69);
                    assert_eq!(store.unigram_delta(user_a).unwrap(), 0);
                    assert_eq!(store.unigram_total().unwrap(), 966);
                    cleanup(&path);
                }

                fn key(text: &str) -> PinyinKey {
                    SyllableKey::from_text(text)
                        .expect("frozen syllable")
                        .index() as PinyinKey
                }

                #[test]
                fn promote_addon_phrase_allocates_nibble_5_and_copies_frequency() {
                    let path = temp_path("promote-addon");
                    let mut store = Store::create(&path).unwrap();
                    let keys = [key("er"), key("huang")];

                    let token = store
                        .promote_addon_phrase("二簧", &[(keys.to_vec(), 100)], 100)
                        .unwrap();
                    assert_eq!(
                        phrase_index_library_index(token),
                        ADDON_DICTIONARY,
                        "promotion lands in default nibble 5"
                    );
                    assert_eq!(token, phrase_index_make_token(ADDON_DICTIONARY, 1));
                    assert_eq!(store.unigram_delta(token).unwrap(), 100);
                    assert_eq!(store.unigram_total().unwrap(), 100);
                    assert_eq!(
                        store.token_for_phrase_in(ADDON_DICTIONARY, "二簧").unwrap(),
                        Some(token)
                    );
                    let phrase = store.phrase(token).unwrap().unwrap();
                    assert_eq!(phrase.text(), "二簧");
                    assert_eq!(phrase.pronunciations().len(), 1);
                    assert_eq!(phrase.pronunciations()[0].keys(), keys);
                    assert_eq!(phrase.pronunciations()[0].count(), 100);

                    let again = store
                        .promote_addon_phrase("二簧", &[(keys.to_vec(), 100)], 100)
                        .unwrap();
                    assert_eq!(again, token, "re-promotion reuses the nibble-5 token");
                    assert_eq!(store.unigram_delta(token).unwrap(), 100);
                    assert_eq!(store.unigram_total().unwrap(), 100);
                    assert_eq!(
                        store.phrase(token).unwrap().unwrap().pronunciations()[0].count(),
                        200
                    );

                    cleanup(&path);
                }

                #[test]
                fn promote_addon_phrase_rejects_a_reading_of_the_wrong_length() {
                    let path = temp_path("promote-addon-invalid");
                    let mut store = Store::create(&path).unwrap();
                    let err = store
                        .promote_addon_phrase("二簧", &[(vec![key("er")], 100)], 100)
                        .unwrap_err();
                    assert!(matches!(err, UserStoreError::InvalidPhrase));
                    assert!(!store.has_user_data());
                    cleanup(&path);
                }
            }
        };
    }

    user_store_tests!(redb, oxpinyin_store::RedbStore, "redb");
    #[cfg(feature = "lmdb")]
    user_store_tests!(lmdb, oxpinyin_store::LmdbStore, "lmdb");

    // ── Registry-specific tests (DefaultStore / redb only) ───────

    fn temp_path(tag: &str) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("oxpinyin-user-{tag}-{}.redb", std::process::id()));
        let _ = std::fs::remove_file(&path);
        path
    }

    #[test]
    fn a_write_through_one_handle_invalidates_another_handles_cache() {
        let path = temp_path("clone-cache");
        let mut writer = UserStore::open(&path).unwrap();
        let reader = UserStore::open(&path).unwrap();

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

        writer.observe_selection(1, 100).unwrap();
        assert_eq!(reader.count_delta(Some(1), 100).unwrap().bigram_count, 207);

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
    fn second_open_of_same_path_shares_the_handle() {
        let path = temp_path("shared-handle");
        let mut first = UserStore::open(&path).unwrap();
        let mut second = UserStore::open(&path).unwrap();

        assert_eq!(first.observe_selection(1, 100).unwrap(), 69);
        assert_eq!(second.bigram_count(1, 100).unwrap(), 69);

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
}
