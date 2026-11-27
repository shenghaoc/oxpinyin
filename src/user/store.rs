//! The libpinyin session store: the user phrase index, the user bigram and
//! the live system items of one `pinyin_init`.
//!
//! A user library's items live in an owned arena
//! ([`crate::user::user_library::UserLibrary`], the pin's `SubPhraseIndex`), the
//! user pinyin index and phrase table in their own tables, and a system
//! library's changes in its live buffer ([`crate::user::system_items`]). The user
//! bigram is held once, as the pin's own `SingleGram` values
//! (`docs/findings/user-store.md` §5), in the pin's container
//! ([`StoreInner::bigram_db`]), and every bigram operation runs the pin's
//! `SingleGram` code over those bytes ([`crate::single_gram`]).
//! The counts the decoder merges come back as a
//! [`crate::model::UserCountDelta`]; the save cycle is the §4 `m_modified`
//! gate and the persistence point behind `pinyin_save`.

use crate::phrase_item::PhraseItemView;
use std::fmt;
use std::rc::Rc;

use crate::parser::{ChewingKey, Completeness};
use crate::store::{DefaultUserBigramDb, StoreError, UserBigramDb};
use crate::vocab::SyllableKey;
use crate::{model::UserCountDelta, single_gram};

use crate::user::phrase_table;
use crate::user::replay::UnigramAdd;
use crate::user::seed;
use crate::user::store_libpinyin::FiniGuard;
use crate::user::{ADD_PHRASE_UNIGRAM_FACTOR, DEFAULT_PHRASE_COUNT, PinyinKey, UserPhrase};
use crate::vocab::{
    ADDON_DICTIONARY, PHRASE_INDEX_LIBRARY_MASK, PHRASE_MASK, PhraseToken, USER_DICTIONARY,
    phrase_index_library_index,
};

#[path = "store_arena.rs"]
mod arena;

/// Which seed rule an update applies.
#[derive(Clone, Copy)]
enum SeedPolicy {
    /// Reselection-expansion rule (`pinyin_train`, §2).
    Training,
    /// Flat `INITIAL_SEED` (`pinyin_choose_predicted_candidate`, §2).
    Predicted,
}

/// Errors from opening or updating the user store.
#[derive(Debug, thiserror::Error)]
pub enum UserStoreError {
    /// The store file could not be opened (I/O error).
    #[error("I/O error: {0}")]
    Io(#[source] std::io::Error),
    /// The storage backend reported an error.
    #[error("store error: {0}")]
    Store(#[source] StoreError),
    /// Phrase text is empty, too long, or its key count does not match its
    /// Unicode scalar length (`docs/findings/user-store.md` §3.1–3.2).
    #[error("invalid phrase (empty, too long, or key count mismatch)")]
    InvalidPhrase,
    /// The libpinyin user-dir persistence failed (I/O, container, or a
    /// byte stream that does not parse).
    #[error("libpinyin user-dir persistence failed")]
    Persistence,
    /// `MemoryChunk::mmap` refused a loaded system library's file where the
    /// save's library loop maps it again (`pinyin.cpp:956`, `zhuyin.cpp:589`). The save stopped at that library: the earlier
    /// libraries' files stand as written, nothing later was written or
    /// renamed, no marker was written, and the session stays dirty.
    #[error("a system library's file cannot be mapped again for the save (upstream: mmap failed!)")]
    SaveRemapFailed,
    /// A bulk copy of the save reaches the pin's unsafe size
    /// (`memory_chunk.h:95-98`). The save stops at
    /// that copy: the earlier files stay as written (libpinyin's staged
    /// `.tmp` files, libzhuyin's completed renames), nothing later happens,
    /// no marker is written, the session stays dirty and a later save retries.
    #[error("a save copy reaches the pin's unsafe size (upstream memory_chunk.h:95-98)")]
    SaveTooLarge,
    /// The unigram add of an accepted predicted candidate overflowed its
    /// library's `guint32` total: `FacadePhraseIndex::add_unigram_frequency`
    /// answers `ERROR_INTEGER_OVERFLOW` and `pinyin_choose_predicted_candidate`
    /// returns `false` before it trains the bigram (`pinyin.cpp:2609-2612`).
    #[error("unigram total overflow (upstream ERROR_INTEGER_OVERFLOW, pinyin.cpp:2609-2612)")]
    UnigramTotalOverflow,
    /// `pinyin_mask_out` / `zhuyin_mask_out` met a `user_pinyin_index.bin`
    /// key past `MAX_PHRASE_LENGTH` syllables, or with no whole syllable
    /// (an empty or one-byte key): the pin's user
    /// `ChewingLargeTable2::mask_out` walks every record and its
    /// `switch`'s `default: abort()` fires on the key
    /// (`chewing_large_table2_bdb.cpp:529`). Returned before the mask
    /// changes anything; the C adapters abort.
    #[error(
        "user pinyin index key past MAX_PHRASE_LENGTH syllables or without a whole \
                 syllable (upstream aborts, chewing_large_table2_bdb.cpp:529)"
    )]
    OverlongIndexKey,
    /// A `user_bigram.db` key is not a four-byte `phrase_token_t`:
    /// `Bigram::get_all_items` asserts `key.size == sizeof(phrase_token_t)`
    /// (`ngram_bdb.cpp:199`), and the pin's `mask_out` and
    /// `pinyin_begin_get_bigram_phrases` both drive that walk. Returned
    /// where the pin's walk meets the key; the C adapters abort.
    #[error("user bigram key is not a phrase_token_t (upstream aborts, ngram_bdb.cpp:199)")]
    NonTokenUserBigramKey,
    /// A `user_bigram.db` value is shorter than a `guint32` `total_freq`:
    /// the pin's `get_total_freq` asserts on it (`memory_chunk.h:390`,
    /// through `ngram.cpp:49`). Returned where the pin's
    /// walk reads that total; the C adapters abort on it.
    #[error(
        "user bigram value shorter than a guint32 total_freq (upstream asserts, \
                 memory_chunk.h:390)"
    )]
    ShortUserBigramValue,
    /// A `user_bigram.db` value has an incomplete eight-byte item after
    /// its total: the pin's `cur != end` walks read past it
    /// (`ngram.cpp:76-86`, `:190`, `:241`, `:261`). The mask, removal and predicted-training facades
    /// refuse it with one warning.
    #[error(
        "user bigram value with a partial item tail (upstream reads past it, \
                 ngram.cpp:76-86)"
    )]
    PartialUserBigramTail,
    /// A `user_bigram.db` gram's `total_freq` is not covered by its items
    /// and a mask removes every item, leaving a residual total:
    /// `Bigram::mask_out` then asks `SingleGram::get_length`, whose assert
    /// dies (`ngram.cpp:70`, `ngram_bdb.cpp:243`). Returned before the
    /// mask changes anything; the C adapters abort.
    #[error("masking a user bigram leaves a residual total_freq (upstream aborts, ngram.cpp:70)")]
    ResidualUserBigramGram,
    /// Init's library loop died at a row ([`crate::user::LibraryLoopStop`]).
    #[error("library loop: {0}")]
    LibraryLoop(#[source] crate::user::LibraryLoopStop),
    /// A training step's `set_freq` misses the item its `insert_freq` just
    /// added: on an unsorted gram the second `lower_bound` can land past
    /// it, and the pin's `check_result(user->set_freq(token, freq + seed))`
    /// dies (`phonetic_lookup.h:903`, `ngram.cpp:252-271`). Returned before anything is stored; the C adapters abort.
    #[error(
        "a training step's set_freq misses the item insert_freq added \
                 (upstream asserts, phonetic_lookup.h:903)"
    )]
    TrainingSetFreqMiss,
    /// A user reading cannot be removed from both pinyin index projections;
    /// the pin asserts on the failed removal (`pinyin.cpp:3759`).
    #[error("user reading removal failed (upstream asserts, pinyin.cpp:3759)")]
    UserReadingRemoval,
    /// The session could not load the user bigram container after taking
    /// ownership of the system libraries. The pin's in-memory loads assert
    /// (`ngram_bdb.cpp:51-52`, `bdb_utils.h:61`, `tkrzwdb_utils.h:45`).
    /// The C adapters abort.
    #[error(
        "the session could not be built after the system libraries were handed \
                 over (upstream dies)"
    )]
    SessionSeed,
    /// A training step would store its gram in the failed Berkeley DB user
    /// bigram container (`crate::store::UserBigramDb::is_failed`): its
    /// `store` answers false and the pin's
    /// `check_result(m_user_bigram->store(last_token, user))` dies
    /// (`phonetic_lookup.h:904`). Returned before anything is stored; the
    /// C adapters abort.
    #[error(
        "a training step stores into the failed user bigram container \
                 (upstream asserts, phonetic_lookup.h:904)"
    )]
    FailedUserBigramStore,
}

impl From<StoreError> for UserStoreError {
    fn from(e: StoreError) -> Self {
        match e {
            StoreError::Io(io) => Self::Io(io),
            other @ (StoreError::Backend(_)
            | StoreError::ReadOnly
            | StoreError::InvalidInput(_)
            | StoreError::ReadAfterCheck) => Self::Store(other),
        }
    }
}

/// The user pinyin index projection of `syllables` — the DB key
/// `add_index` would write for this reading: tone-zeroed complete keys, or
/// initial-only keys when any syllable is incomplete
/// (`compute_chewing_index` / `compute_incomplete_chewing_index`,
/// `src/storage/pinyin_phrase3.h:160-178` at the pin).
fn index_projection(syllables: &[SyllableKey]) -> Vec<u16> {
    let incomplete = syllables
        .iter()
        .any(|key| key.completeness() == Completeness::Partial);
    syllables
        .iter()
        .map(|key| {
            let full = key.chewing_key();
            if incomplete {
                // `compute_incomplete_chewing_index` sets only
                // `m_initial`, so a vowel-initial syllable and `ng`
                // project to the zero initial
                // (`ChewingKey::new(0, 0, 0, 0)`), not to a missing
                // key. Building the packed word directly keeps those
                // zeros representable.
                ChewingKey::new(full.initial, 0, 0, 0).to_packed()
            } else {
                full.with_tone(0).to_packed()
            }
        })
        .collect()
}

/// [`ADD_PHRASE_UNIGRAM_FACTOR`] as `_add_phrase`'s `guint32`
/// `unigram_factor` (`pinyin.cpp:522`).
const ADD_PHRASE_UNIGRAM_FACTOR_U32: u32 = 3;
const _: () = assert!(ADD_PHRASE_UNIGRAM_FACTOR_U32 as u64 == ADD_PHRASE_UNIGRAM_FACTOR);

/// One imported reading: its dense keys, the same as packed `ChewingKey`
/// words, and the count it adds.
struct SystemReading<'a> {
    packed: &'a [u16],
    delta: u32,
}

/// `PhraseItem::add_pronunciation` (phrase_index.cpp:56-91) on a live system
/// item: the overflow test uses the prefix total through the matching row, and
/// `_add_phrase` ignores a refused increment and still returns true. An
/// unmatched reading on an item holding 255 wraps the pin's `guint8` count to
/// 0 (`:88`). Plan the change without touching the live item: a refused
/// persistence write must leave it unchanged.
#[derive(Clone, Copy)]
enum SystemMerge {
    Matched { position: usize, frequency: u32 },
    Append,
    Wrap,
}

fn plan_system_reading(
    item: &crate::data::phrase_library::ChunkItem,
    reading: &SystemReading<'_>,
) -> SystemMerge {
    let mut total = 0_u32;
    for (position, (stored, frequency)) in item.prons.iter().enumerate() {
        total = total.wrapping_add(*frequency);
        if stored.as_slice() == reading.packed {
            let frequency = if reading.delta == 0 || total <= total.wrapping_add(reading.delta) {
                frequency.wrapping_add(reading.delta)
            } else {
                *frequency
            };
            return SystemMerge::Matched {
                position,
                frequency,
            };
        }
    }
    if item.prons.len() == usize::from(u8::MAX) {
        SystemMerge::Wrap
    } else {
        SystemMerge::Append
    }
}

/// Applies one `PhraseItem::add_pronunciation` reading to a stored item,
/// with the pin's `guint8` pronunciation-count wrap. The storage tools'
/// text importer uses the same planning and application as the system
/// import path; `true` means the count wrapped and the stored rows are the
/// empty view.
pub fn add_system_reading(
    item: &mut crate::data::phrase_library::ChunkItem,
    packed: &[u16],
    delta: u32,
) -> bool {
    let reading = SystemReading { packed, delta };
    let change = plan_system_reading(item, &reading);
    apply_system_reading(item, &reading, change);
    matches!(change, SystemMerge::Wrap)
}

fn apply_system_reading(
    item: &mut crate::data::phrase_library::ChunkItem,
    reading: &SystemReading<'_>,
    change: SystemMerge,
) {
    match change {
        SystemMerge::Matched {
            position,
            frequency,
        } => {
            if let Some((_, current)) = item.prons.get_mut(position) {
                *current = frequency;
            }
        }
        SystemMerge::Append => item.prons.push((reading.packed.to_vec(), reading.delta)),
        SystemMerge::Wrap => item.prons.clear(),
    }
}

/// Check the encoder's shape conditions before changing the item.
/// Wrapping discards every old pronunciation.
fn validate_system_merge(
    item: &crate::data::phrase_library::ChunkItem,
    reading: &SystemReading<'_>,
    change: SystemMerge,
) -> Result<(), UserStoreError> {
    let length = item.phrase.len();
    let wrapped = matches!(change, SystemMerge::Wrap);
    let appended = matches!(change, SystemMerge::Append);
    if length == 0
        || length > usize::from(u8::MAX)
        || (!wrapped
            && (item.prons.len() + usize::from(appended) > usize::from(u8::MAX)
                || item.prons.iter().any(|(keys, _)| keys.len() != length)
                || (appended && reading.packed.len() != length)))
    {
        return Err(UserStoreError::Persistence);
    }
    Ok(())
}

// ── the user bigram's grams ───────────────────────────────────────

/// The four operations `Bigram` runs on its container (`ngram_bdb.cpp`
/// and its Kyoto Cabinet and tkrzw twins): `get_all_items`, `load`,
/// `store`, `remove`, over the pin's own in-memory user bigram container,
/// keyed by the four native-endian token bytes.
pub(crate) struct ContainerGrams<'a>(pub(crate) &'a crate::store::DefaultUserBigramDb);

impl ContainerGrams<'_> {
    /// `Bigram::get_all_items`: every key in the container's walk order;
    /// `None` for a key that is not a four-byte `phrase_token_t`.
    fn keys(&self) -> Result<Vec<Option<PhraseToken>>, StoreError> {
        Ok(self
            .0
            .keys()?
            .into_iter()
            .map(|key| key.map(PhraseToken::from_le_bytes))
            .collect())
    }

    /// `Bigram::load`: the raw gram under `prev`.
    fn get(&self, prev: PhraseToken) -> Result<Option<Vec<u8>>, StoreError> {
        container_gram(self.0, prev)
    }

    /// `Bigram::store`.
    fn store(&self, prev: PhraseToken, gram: &[u8]) -> Result<(), StoreError> {
        self.0.store(&prev.to_le_bytes(), gram)
    }

    /// `Bigram::remove`.
    fn remove(&self, prev: PhraseToken) -> Result<(), StoreError> {
        self.0.remove(&prev.to_le_bytes())
    }
}

/// `Bigram::load` on the pin's container: the gram under `prev`, owned.
fn container_gram(
    container: &crate::store::DefaultUserBigramDb,
    prev: PhraseToken,
) -> Result<Option<Vec<u8>>, StoreError> {
    let mut gram = Vec::new();
    Ok(container
        .get(&prev.to_le_bytes(), &mut gram)?
        .then_some(gram))
}

/// `Bigram::mask_out(mask, value)` (`ngram_bdb.cpp:217-254`, synced with
/// `ngram_kyotodb.cpp:198-235` and `ngram_tkrzwdb.cpp:174-211` at the pin
/// `074a2219`): over the keys `get_all_items` already collected, so that a
/// mask and its fault check share the pin's one container walk, a selected
/// key is removed; any other gram is loaded and masked
/// ([`single_gram::mask_out`]); one with nothing removed is left untouched,
/// one left with no items is removed and any other is stored back.
/// Answers whether a key remains. The faults the pin dies on are the
/// caller's to refuse first ([`gram_mask_fault_over`]).
fn mask_grams_over(
    grams: &ContainerGrams<'_>,
    keys: Vec<Option<PhraseToken>>,
    mask: PhraseToken,
    value: PhraseToken,
) -> Result<bool, StoreError> {
    let mut remaining = false;
    for key in keys {
        let Some(prev) = key else {
            remaining = true;
            continue;
        };
        if prev & mask == value {
            grams.remove(prev)?;
            continue;
        }
        let Some(mut gram) = grams.get(prev)? else {
            continue;
        };
        if single_gram::mask_out(&mut gram, mask, value) == 0 {
            remaining = true;
            continue;
        }
        if single_gram::get_length(&gram) == 0 {
            grams.remove(prev)?;
        } else {
            grams.store(prev, &gram)?;
            remaining = true;
        }
    }
    Ok(remaining)
}

/// The fault [`mask_grams_over`] would die on at the pin, in the pin's own
/// order, or `None` when the walk completes, over keys `get_all_items`
/// already collected: the faults are met in the same key order, after the
/// same one walk, that the pin's `Bigram::mask_out` meets them.
/// `get_all_items` asserts on a key that is not four bytes before any gram
/// is loaded (`ngram_bdb.cpp:199`); then, per key the mask leaves to be
/// loaded, `SingleGram::mask_out` asserts on a missing total
/// (`memory_chunk.h:390`) and reads past a partial item (`ngram.cpp:86`),
/// and a gram the mask empties with a total left over asserts in
/// `get_length` (`ngram.cpp:70`, reached at `ngram_bdb.cpp:243`). The last
/// is found by running the mask on a copy.
fn gram_mask_fault_over(
    grams: &ContainerGrams<'_>,
    keys: &[Option<PhraseToken>],
    mask: PhraseToken,
    value: PhraseToken,
) -> Result<Option<UserStoreError>, StoreError> {
    if keys.iter().any(Option::is_none) {
        return Ok(Some(UserStoreError::NonTokenUserBigramKey));
    }
    for &prev in keys.iter().flatten() {
        if prev & mask == value {
            continue;
        }
        let Some(mut gram) = grams.get(prev)? else {
            continue;
        };
        if gram.len() < single_gram::TOTAL_LEN {
            return Ok(Some(UserStoreError::ShortUserBigramValue));
        }
        if single_gram::has_partial_tail(&gram) {
            return Ok(Some(UserStoreError::PartialUserBigramTail));
        }
        if single_gram::mask_out(&mut gram, mask, value) > 0
            && single_gram::get_length(&gram) == 0
            && single_gram::get_total_freq(&gram) != Some(0)
        {
            return Ok(Some(UserStoreError::ResidualUserBigramGram));
        }
    }
    Ok(None)
}

/// A training step's gram (`phonetic_lookup.h:876-909` at the pin
/// `074a2219`): `load`, or a fresh gram; `get_total_freq`; `get_freq`,
/// with `insert_freq(token, 0)` and the initial seed on a miss and the
/// expanded seed on a hit; then, unless the total would overflow,
/// `set_total_freq` and `set_freq`. Answers the seed and the gram to
/// store — `None` when the overflow guard leaves the gram unchanged.
fn trained_gram(
    gram: &mut Vec<u8>,
    cur: PhraseToken,
) -> Result<(u64, Option<&[u8]>), UserStoreError> {
    let total = single_gram::get_total_freq(gram).ok_or(UserStoreError::ShortUserBigramValue)?;
    let previous = single_gram::get_freq(gram, cur);
    let (freq, seed) = match previous {
        Some(freq) => (freq, seed::training_seed(Some(u64::from(freq)))),
        None => (0, seed::INITIAL_SEED),
    };
    let seed32 = seed as u32;
    if seed32 > 0 && total > total.wrapping_add(seed32) {
        return Ok((seed, None));
    }
    if previous.is_none() {
        // The pin inserts before its overflow guard, but skips the store
        // on overflow. Leave the cached row unchanged in that case.
        single_gram::insert_freq(gram, cur, 0);
    }
    single_gram::set_total_freq(gram, total.wrapping_add(seed32));
    if !single_gram::set_freq(gram, cur, freq.wrapping_add(seed32)) {
        // On an unsorted gram the second `lower_bound` can land past the
        // item `insert_freq` just added; the pin's `check_result` dies
        // (`phonetic_lookup.h:903`).
        return Err(UserStoreError::TrainingSetFreqMiss);
    }
    Ok((seed, Some(gram)))
}

/// An accepted predicted candidate's gram (`pinyin.cpp:2622-2638` at the
/// pin `074a2219`): `load`, or a fresh gram; `get_total_freq`; `get_freq`;
/// `insert_freq(token, 69)` on a miss or `set_freq(freq + 69)` on a hit;
/// `set_total_freq(total + 69)`. No overflow guard: both fields wrap.
fn predicted_gram(row: Option<Vec<u8>>, cur: PhraseToken) -> Result<Vec<u8>, UserStoreError> {
    let seed = seed::predicted_seed() as u32;
    let mut gram = row.unwrap_or_else(single_gram::new_gram);
    let total = single_gram::get_total_freq(&gram).ok_or(UserStoreError::ShortUserBigramValue)?;
    match single_gram::get_freq(&gram, cur) {
        None => single_gram::insert_freq(&mut gram, cur, seed),
        Some(freq) => single_gram::set_freq(&mut gram, cur, freq.wrapping_add(seed)),
    };
    single_gram::set_total_freq(&mut gram, total.wrapping_add(seed));
    Ok(gram)
}

/// The words a phrase writes: the stored form when the phrase carries one,
/// else its outward words.
fn phrase_words(phrase: &crate::vocab::PhraseText) -> Vec<u32> {
    phrase
        .stored_words()
        .map_or_else(|| phrase.outward_words().collect(), <[u32]>::to_vec)
}

/// `_add_phrase`'s gate: a non-empty phrase shorter than the maximum with
/// one key per word (074a2219 pinyin.cpp:620-627).
fn phrase_fits_keys(words: &[u32], keys: &[PinyinKey]) -> bool {
    !words.is_empty() && words.len() < crate::vocab::MAX_PHRASE_LENGTH && words.len() == keys.len()
}

// ── UserStore ───────────────────────────────────────────────────

/// Per library, how far the total the pin's sub-index holds lies from the
/// one its items make, a wrapping `guint32` keyed by nibble: the
/// difference a stored header or a `.dbin` replay left, until the next
/// `compact` rebuilds the library's total from its items. An entry exists
/// only for a non-zero difference.
pub(crate) type Offsets = crate::user::LibraryTable<u32>;

/// One session's state, owned by its [`UserStore`].
#[expect(
    clippy::struct_excessive_bools,
    reason = "The system-item gate, the dirty mark, the user-data gate and the failed-container record are independent session properties"
)]
pub struct StoreInner {
    pub(crate) has_system_items: bool,
    pub(crate) system_items: crate::user::system_items::SystemItems,
    /// The user libraries' stored-total differences ([`Offsets`]).
    pub(crate) offsets: Offsets,
    pub(crate) dirty: bool,
    /// Revision observed by the phrase export iterator.
    pub(crate) write_generation: u64,
    /// The gate in front of the user-bigram read: `false` means the
    /// container holds no gram. Recomputed at `open` and maintained only by
    /// `UserStore::mark_committed_write`.
    pub(crate) has_user_data: bool,
    /// The libpinyin session metadata (optional directory, system originals).
    /// Transient sessions retain originals without a directory; others use the
    /// pin's own file shapes (`open_libpinyin_checked`): `save()` exports the
    /// session and writes the file set.
    pub(crate) libpinyin: Rc<crate::user::store_libpinyin::Target>,
    /// The pin's own in-memory user bigram container (`Bigram::m_db`
    /// after `load_db`): the session's only copy of the user bigram, the
    /// raw `SingleGram` values every bigram operation reads and writes, so
    /// its walk order — the export's order and the saved file's layout —
    /// is the pin's.
    pub(crate) bigram_db: crate::store::DefaultUserBigramDb,
    /// Whether [`Self::bigram_db`] is the failed Berkeley DB container
    /// (`crate::store::UserBigramDb::is_failed`), recorded when it is
    /// created: it never changes.
    pub(crate) bigram_failed: bool,
}

/// The libpinyin session store: one per open, over the arena, the pin's
/// user bigram container and the live system items. The user files are
/// read and written with the compiled-in backend (tkrzw, Kyoto Cabinet or
/// Berkeley DB, one per binary; see `oxpinyin`).
///
/// The context owns the store and lends it to each operation. System chunk
/// originals belong to this owner; sessions and cursors cannot retain them.
/// Public mutations require an exclusive borrow of this store.
pub struct UserStore {
    /// The facade fini's `user.conf` write: armed only on the handle a
    /// libpinyin open returned ([`FiniGuard`]).
    _fini: FiniGuard,
    inner: StoreInner,
    pub(crate) originals: crate::user::SystemOriginals,
}

/// What `UserStore::check_mask_out` found fit to mask: the user bigram's
/// keys from the one `get_all_items` walk, for `UserStore::apply_mask_out`.
pub struct MaskCheck {
    container_keys: Result<Vec<Option<PhraseToken>>, StoreError>,
}

impl UserStore {
    /// Run the context's fini marker write once, while retaining its live data
    /// for handles that outlive the public context.
    pub fn finish(&mut self) {
        self._fini.finish();
    }

    /// Crate-visible handle assembly for the libpinyin constructor
    /// ([`crate::user::store_libpinyin`]), which runs in memory and arms
    /// the fini guard of the open that raised the counter.
    pub(crate) fn from_parts(
        inner: StoreInner,
        fini: FiniGuard,
        originals: crate::user::SystemOriginals,
    ) -> Self {
        Self {
            _fini: fini,
            originals,
            inner,
        }
    }
}

impl fmt::Debug for UserStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UserStore").finish_non_exhaustive()
    }
}

impl UserStore {
    /// Borrow the uniquely owned init baselines for a read operation.
    pub const fn originals(&self) -> &crate::user::SystemOriginals {
        &self.originals
    }

    /// Read system library `nibble`'s saved log once into `logs` and return
    /// the chunk-read or journal fault the walk reached.
    /// [`Self::reload_system_libraries_prefetched`] merges the bytes read here.
    pub(crate) fn read_mask_log(
        &self,
        logs: &mut crate::user::MaskLogs,
        nibble: u8,
        mask: PhraseToken,
        value: PhraseToken,
    ) -> Option<crate::user::MaskStop> {
        let dir = self.inner.libpinyin.dir.as_deref()?;
        crate::user::read_mask_log(dir, self.originals.layout(), logs, nibble, mask, value)
    }

    /// Borrow the init baselines exclusively for a library transition.
    pub(crate) fn originals_mut(&mut self) -> &mut crate::user::SystemOriginals {
        &mut self.originals
    }

    /// The user pinyin index's readings past `MAX_PHRASE_LENGTH` syllables,
    /// as packed `ChewingKey` words — the pin's user `ChewingLargeTable2`
    /// carries one DB key per reading, and any key past the table's
    /// 16-syllable instantiation drives its `switch`'s `default: abort()`
    /// sites.
    #[must_use]
    pub(crate) fn overlong_index_readings(&self) -> &[Vec<u16>] {
        self.inner.libpinyin.overlong_index_keys.as_slice()
    }

    /// Whether `user_pinyin_index.bin` carried a key past
    /// `MAX_PHRASE_LENGTH` syllables, or one with no whole syllable.
    /// `pinyin_mask_out` / `zhuyin_mask_out` walk every record, so any such
    /// key aborts the whole mask (`chewing_large_table2_bdb.cpp:529`).
    #[must_use]
    pub fn has_overlong_index_key(&self) -> bool {
        !self.overlong_index_readings().is_empty()
    }

    /// The pin's in-memory user bigram container.
    const fn container(&self) -> &DefaultUserBigramDb {
        &self.inner.bigram_db
    }

    /// Whether the session's user bigram is the failed Berkeley DB container
    /// (`crate::store::UserBigramDb::is_failed`), as recorded when it was
    /// created. On a backend whose container cannot fail this is the
    /// constant `false`.
    #[inline]
    fn user_bigram_failed(&self) -> bool {
        <DefaultUserBigramDb as crate::store::UserBigramDb>::CAN_FAIL && self.inner.bigram_failed
    }

    /// `Bigram::load(prev)`: the raw gram stored under `prev` — one
    /// point read of the pin's container. `None` when `prev` has
    /// no gram.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the read fails.
    pub fn bigram_row(&self, prev: PhraseToken) -> Result<Option<Vec<u8>>, UserStoreError> {
        // Every gram write sets the flag and a mask recomputes it from the
        // grams it leaves, so a store without user data holds no gram.
        if !self.has_user_data() {
            return Ok(None);
        }
        self.gram_in(prev)
    }

    /// `train_result3`'s `m_user_bigram->load(last_token, user)` and the
    /// fresh gram it falls back to (074a2219 `phonetic_lookup.h:876-881`):
    /// a failed lookup is no gram ([`crate::store::UserBigramDb::load`]).
    ///
    /// # Errors
    ///
    /// None in practice: [`crate::store::UserBigramDb::load`] answers `Ok`.
    fn training_gram(&self, prev: PhraseToken) -> Result<Option<Vec<u8>>, UserStoreError> {
        if !self.has_user_data() {
            return Ok(None);
        }
        let mut gram = Vec::new();
        Ok(<DefaultUserBigramDb as crate::store::UserBigramDb>::load(
            self.container(),
            &prev.to_le_bytes(),
            &mut gram,
        )?
        .then_some(gram))
    }

    /// [`Self::bigram_row`] without the user-data gate.
    fn gram_in(&self, prev: PhraseToken) -> Result<Option<Vec<u8>>, UserStoreError> {
        Ok(container_gram(self.container(), prev)?)
    }

    /// `Bigram::get_all_items`: every key in the container's walk order,
    /// with `None` for a key that is not a four-byte `phrase_token_t`.
    fn gram_keys(&self) -> Result<Vec<Option<PhraseToken>>, UserStoreError> {
        Ok(ContainerGrams(self.container()).keys()?)
    }

    /// Whether the gram stored under `token` is shorter than a `guint32`
    /// `total_freq` (`memory_chunk.h:390`). The predicted-candidate walk
    /// loads only the resolved prefixes' own grams (`pinyin.cpp:2322-2330`),
    /// training only its predecessors', while the export walk loads every
    /// predecessor (`pinyin.cpp:889`); callers scope this live check to the
    /// grams their pinned operation reaches.
    #[must_use]
    pub(crate) fn has_short_bigram_value_for(&self, token: PhraseToken) -> bool {
        self.bigram_row(token)
            .ok()
            .flatten()
            .is_some_and(|value| value.len() < single_gram::TOTAL_LEN)
    }

    /// `pinyin_begin_get_bigram_phrases`'s one `Bigram::get_all_items`
    /// walk (`pinyin.cpp:779`): the predecessors in the container's walk
    /// order, or `None` when a key is not a four-byte `phrase_token_t`,
    /// where the walk's assert dies (`ngram_bdb.cpp:199`; Kyoto Cabinet
    /// `:173`, tkrzw `:150`). Read from the live container's keys.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the keys cannot be read.
    pub(crate) fn bigram_export_predecessors(
        &self,
    ) -> Result<Option<Vec<PhraseToken>>, UserStoreError> {
        Ok(self.gram_keys()?.into_iter().collect())
    }

    /// Whether `token`'s stored user-bigram value has a partial item
    /// tail. The pin's `retrieve_all`
    /// (`ngram.cpp:133-143`), `SingleGram::mask_out` (`ngram.cpp:86`) and
    /// the training walks (`ngram.cpp:190`, `:241`, `:261`) all walk with
    /// `cur != end` and read past that tail. Read from the live value, so
    /// an in-session store or removal counts at once.
    #[must_use]
    pub(crate) fn has_oob_bigram_value_for(&self, token: PhraseToken) -> bool {
        self.bigram_row(token)
            .ok()
            .flatten()
            .is_some_and(|bytes| single_gram::has_partial_tail(&bytes))
    }

    /// The session container's `Bigram::get_all_items` walk
    /// (`ngram_tkrzwdb.cpp:161-172` and its Kyoto Cabinet and Berkeley DB
    /// twins at 074a2219). A mask takes it once and hands it to both its
    /// fault check and its walk. The mask takes `&mut self`, so no other
    /// handle adds a gram the walk missed.
    fn container_keys(&self) -> Result<Vec<Option<PhraseToken>>, StoreError> {
        ContainerGrams(self.container()).keys()
    }

    /// The mask's bigram fault check over the container keys
    /// [`Self::container_keys`] collected.
    fn bigram_mask_fault_over(
        &self,
        keys: &Result<Vec<Option<PhraseToken>>, StoreError>,
        mask: PhraseToken,
        value: PhraseToken,
    ) -> Option<UserStoreError> {
        let Ok(keys) = keys else {
            return None;
        };
        gram_mask_fault_over(&ContainerGrams(self.container()), keys, mask, value).unwrap_or(None)
    }

    /// The pin's user-table `search_suggestion` gate
    /// (`chewing_large_table2_bdb.cpp:282`): `true` when some reading past
    /// `MAX_PHRASE_LENGTH` syllables strictly extends `syllables` in the
    /// keyspace `add_index` writes. The pin's `DB_SET` probe first requires
    /// the exact query key to exist (`:576-582`); only then does its
    /// `DB_NEXT` walk reach an over-long extension and its `switch`'s
    /// `default: abort()` die. A crafted index that carries the over-long
    /// key but omits the exact prefix row walks nothing, so the gate must
    /// consult the raw key set too. The engine surfaces the pin's abort as
    /// an error instead.
    #[must_use]
    pub(crate) fn overlong_extension_gate(&self, syllables: &[SyllableKey]) -> bool {
        let projection = index_projection(syllables);
        let target = &self.inner.libpinyin;
        let projection_bytes: Vec<u8> = projection
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect();
        if !target.phrases().index.contains_raw_key(&projection_bytes) {
            return false;
        }
        target
            .overlong_index_keys
            .iter()
            .any(|reading| reading.len() > projection.len() && reading.starts_with(&projection))
    }

    fn write_generation(&self) -> u64 {
        self.inner.write_generation
    }

    fn has_user_data(&self) -> bool {
        self.inner.has_user_data
    }

    /// The gate in front of the count reads, as the session holds it.
    #[cfg(test)]
    pub(crate) fn gate_for_test(&self) -> bool {
        self.has_user_data()
    }

    /// Sets the decode gate.
    #[cfg(test)]
    pub(crate) fn force_gate_for_test(&mut self, open: bool) {
        self.inner.has_user_data = open;
    }

    /// Invalidate cached reads after a committed user-data write.
    ///
    /// Every caller's entry point takes `&mut self`, so no reader observes
    /// post-commit rows against a pre-commit generation, or the reverse.
    fn mark_committed_write(&mut self, has_user_data: bool) {
        self.inner.has_user_data = has_user_data;
        self.advance_write_generation();
    }

    fn advance_write_generation(&mut self) {
        self.inner.write_generation = self.inner.write_generation.wrapping_add(1);
    }

    /// A search of the user pinyin table for the query `keys` makes
    /// (`ChewingLargeTable2::search`): a hit on a non-empty record loads it
    /// into the length's table entry, which the next `mask_out` walk starts
    /// from (#785). Read that record from the canonical table.
    pub(crate) fn note_pinyin_search(&self, keys: impl FnOnce() -> Vec<ChewingKey>) {
        if !crate::user::user_index::ENTRY_PER_LENGTH {
            return;
        }
        let keys = keys();
        self.note_pinyin_search_keys(&keys);
    }

    /// Record an exact search using keys already built for lookup.
    pub(crate) fn note_pinyin_search_keys(&self, keys: &[ChewingKey]) {
        if crate::user::user_index::ENTRY_PER_LENGTH {
            self.inner.libpinyin.phrases_mut().index.note_search(keys);
        }
    }

    /// A `search_suggestion` of the user pinyin table for the prefix `keys`
    /// makes (#785): every longer key it visits empties its length's entry.
    pub(crate) fn note_pinyin_suggestion(&self, keys: impl FnOnce() -> Vec<ChewingKey>) {
        if !crate::user::user_index::ENTRY_PER_LENGTH {
            return;
        }
        let keys = keys();
        self.inner
            .libpinyin
            .phrases_mut()
            .index
            .note_suggestion(&keys);
    }

    /// `SingleGram::get_freq(cur)` on `prev`'s raw gram — the first match
    /// walking from `lower_bound` (`ngram.cpp:231-250`); `0` if absent.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the store cannot be read.
    #[cfg(any(test, feature = "test-hooks"))]
    pub fn bigram_count(&self, prev: PhraseToken, cur: PhraseToken) -> Result<u64, UserStoreError> {
        Ok(self
            .bigram_row(prev)?
            .and_then(|gram| single_gram::get_freq(&gram, cur))
            .map_or(0, u64::from))
    }

    /// `SingleGram::get_total_freq` on `prev`'s raw gram; `0` if none.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the store cannot be read.
    #[cfg(any(test, feature = "test-hooks"))]
    pub fn bigram_total(&self, prev: PhraseToken) -> Result<u64, UserStoreError> {
        Ok(self
            .bigram_row(prev)?
            .and_then(|gram| single_gram::get_total_freq(&gram))
            .map_or(0, u64::from))
    }

    /// Stores `gram` under `prev` as it is, as a crafted container record
    /// holds it, without marking the context modified.
    #[cfg(test)]
    pub(crate) fn plant_gram(&mut self, prev: PhraseToken, gram: &[u8]) {
        self.store_gram(prev, gram).expect("store the gram");
        self.mark_committed_write(true);
    }

    /// `Bigram::store(prev, gram)` on the pin's container.
    fn store_gram(&self, prev: PhraseToken, gram: &[u8]) -> Result<(), UserStoreError> {
        self.container().store(&prev.to_le_bytes(), gram)?;
        Ok(())
    }

    /// Accumulated phrase-index unigram delta for `token`; `0` if none: a
    /// user library item's count, and `0` for a system item, whose count is
    /// its live field ([`Self::system_unigram`]), which the model prices;
    /// the session keeps no shipped count to take a difference from.
    #[must_use]
    pub fn unigram_delta(&self, token: PhraseToken) -> u64 {
        self.with_user_item(token, |item| u64::from(item.unigram()))
            .unwrap_or(0)
    }

    /// A user item's unigram frequency, or `None` when the item is absent. A
    /// present zero count stays `Some(0)`. The existence check and the count
    /// read happen under one arena borrow, as the pin reads one item
    /// (074a2219 `pinyin.cpp:2821-2833`, `zhuyin.cpp:1813-1825`). The caller
    /// applies library visibility.
    #[must_use]
    pub(crate) fn user_unigram_frequency(&self, token: PhraseToken) -> Option<u64> {
        self.with_user_item(token, |item| u64::from(item.unigram()))
    }

    /// The live `guint32` unigram of the system item behind `token`
    /// (`PhraseItem::get_unigram_frequency`), where the session holds the
    /// library's state: an arena item's, a decoded item's, the library's live
    /// buffer's, or the original's before any write. `None` for a removed item,
    /// a library with no original or an item nothing holds.
    #[must_use]
    pub fn system_unigram(&self, token: PhraseToken) -> Option<u32> {
        use crate::user::system_items::LiveItemFields;
        let original = self.originals.get(&phrase_index_library_index(token))?;
        // One slot read answers both whether a journal REMOVE took the item
        // out and its live unigram, as `get_phrase_item` reads it once
        // (074a2219 `phrase_index.cpp:180-206`).
        let live = self.read_live_item(token, crate::user::replay::raw_unigram);
        if let LiveItemFields::NoReadableItem = live {
            return None;
        }
        if let Some(count) = self.with_user_item(token, |item| item.unigram()) {
            return Some(count);
        }
        match live {
            LiveItemFields::Read(unigram) => Some(unigram),
            LiveItemFields::NoLiveLibrary | LiveItemFields::NoReadableItem => {
                original.unigram(token & PHRASE_MASK)
            }
        }
    }

    /// The unigram field of the item the pin's `get_phrase_item` reads for
    /// `token`: the live system item's, or a `USER_FILE` library's own user
    /// item (`Some(0)` for a readable zero), and `None` when the read fails
    /// (`ERROR_NO_ITEM`).
    fn item_unigram(&self, token: PhraseToken) -> Option<u32> {
        self.system_unigram(token).or_else(|| {
            (!self.is_system_library(phrase_index_library_index(token)))
                .then(|| self.with_user_item(token, |item| item.unigram()))
                .flatten()
        })
    }

    /// Whether the session layout names a library as a system chunk.
    #[must_use]
    pub(crate) fn is_system_library(&self, library: u8) -> bool {
        self.originals.get(&library).is_some()
            || self
                .originals
                .layout()
                .system_logs()
                .iter()
                .any(|(index, _)| *index == library)
    }

    /// Apply the exported unigram increment to a loaded system library, as
    /// `FacadePhraseIndex::add_unigram_frequency` does (074a2219
    /// `phrase_index.h:628-635`, `phrase_index.cpp:150-176`): the facade total
    /// takes `delta` first, whatever the sub-index then answers, and the
    /// sub-index takes it only when it holds the item and its `guint32` total
    /// can. The caller checks library visibility. This does not mark the
    /// context dirty (`pinyin.cpp:2836-2843`, `zhuyin.cpp:1828-1835`).
    #[cfg(test)]
    pub fn add_system_unigram_frequency(&mut self, token: PhraseToken, delta: u32) -> bool {
        self.add_unigram_outcome(token, delta).is_accepted()
    }

    /// Apply a unigram increment to the default phrase index. A loaded
    /// sub-index receives the update in its live item, and an absent
    /// sub-index leaves the facade total alone (074a2219
    /// `phrase_index.h:628-635`). Only [`UnigramAdd::Refused`] is
    /// `ERROR_INTEGER_OVERFLOW` (`pinyin.cpp:2528`).
    #[must_use]
    pub fn add_unigram_outcome(&mut self, token: PhraseToken, delta: u32) -> UnigramAdd {
        let library = phrase_index_library_index(token);
        if self.originals.get(&library).is_none() && !self.has_user_library(library) {
            return UnigramAdd::Missing;
        }
        let (outcome, has_sub_index) = self.arena_add_unigram_outcome(token, delta);
        // Even a missing item or refused increment can change the facade total.
        if has_sub_index && delta != 0 {
            self.advance_write_generation();
        }
        outcome
    }

    /// What the user side adds to the facade's unigram total: everything of
    /// `FacadePhraseIndex::m_total_freq` beyond the system libraries'
    /// sub-index totals (the user libraries' totals and every delta a
    /// sub-index did not take). `0` for an empty store.
    #[must_use]
    pub fn unigram_total(&self) -> u64 {
        self.inner.system_items.facade_extra()
    }

    /// What a session's facade total holds beyond the stored totals of the
    /// visible system libraries: [`Self::unigram_total`] plus how far each
    /// visible library's live sub-index total lies from its original's, as a
    /// wrapping `guint32` the model adds to those stored totals.
    #[must_use]
    pub(crate) fn unigram_total_over(&self, visible: impl Fn(u8) -> bool) -> u64 {
        let items = &self.inner.system_items;
        let mut extra = items.facade_extra();
        for (&library, original) in self.originals.iter() {
            if !visible(library) {
                continue;
            }
            let live = items
                .library_total(library, Some(original))
                .unwrap_or(original.total);
            extra = extra.wrapping_add(u64::from(live.wrapping_sub(original.total)));
        }
        extra
    }

    /// `FacadePhraseIndex::compact` (074a2219 `phrase_index.cpp:648-675`):
    /// every library's total is rebuilt from its items, so none carries a
    /// stored total's difference any more. The facade's total keeps it.
    /// The pin compacts in `pinyin_save` once it has something to write,
    /// at the end of an import and at the end of a mask_out
    /// (`pinyin.cpp:1139`, `:657`, `:1294`; `zhuyin.cpp:554`, `:542`,
    /// `:829`).
    pub fn compact_libraries(&mut self) {
        let target = &self.inner.libpinyin;
        // Each system library's total becomes the sum of its live readable
        // items; the facade total keeps what the libraries held, so the
        // difference moves to the facade's extra.
        let mut rebased = Vec::new();
        {
            let items = &mut self.inner.system_items;
            for (&library, original) in self.originals.iter() {
                // An unloaded library has no sub-index to compact: the pin's
                // loop skips it (`phrase_index.cpp:651-652`) and its total
                // stays as the unload left it.
                if items.lacks_sub_index(library, original) {
                    continue;
                }
                // The library adds its live total less its original total
                // to the facade. A library whose layout `SubPhraseIndex::load`
                // refused has a local total the facade never took (074a2219
                // `phrase_index.cpp:242-246`), and `compact_layout_failed`,
                // which every caller runs first (`pinyin.cpp:657`, `:1139`,
                // `:1294`), sets its mapped total to zero. Its original total
                // follows that mapped total, so an unload takes out the
                // compacted local total (`phrase_index.cpp:261-269`). The
                // pin's compact leaves the facade total alone
                // (`phrase_index.cpp:648-675`), so the extra takes the change
                // of the library's share.
                let current = original.mapped.map_or(
                    original.total,
                    crate::data::phrase_library::PhraseLibrary::total_freq,
                );
                let old = items
                    .library_total(library, Some(original))
                    .unwrap_or(original.total);
                let new = items
                    .compact(library)
                    .unwrap_or_else(|| items.compacted_total(library, original));
                items.set_library_total(library, new);
                let share = old.wrapping_sub(original.total);
                items.add_facade_extra(u64::from(share.wrapping_sub(new.wrapping_sub(current))));
                if current != original.total {
                    rebased.push((library, current));
                }
            }
        }
        for (library, total) in rebased {
            self.originals.set_total(library, total);
        }
        let offsets = &mut self.inner.offsets;
        offsets.clear();
        for library in target.phrases_mut().libraries.values_mut() {
            library.compact();
        }
    }

    /// The §5 overlay for scoring `token` after `prev`, read between two commits.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the store cannot be read.
    pub(crate) fn count_delta(
        &self,
        prev: Option<PhraseToken>,
        token: PhraseToken,
    ) -> Result<UserCountDelta, UserStoreError> {
        // Load the user gram for this step (pinyin_lookup2.cpp:369).
        // The pin merges the user gram whenever one is stored, item-less
        // ones included (`merge_single_gram`, `ngram.cpp:298-306`).
        let (bigram_count, bigram_total) = match prev {
            Some(prev) => self.gram_in(prev)?.as_deref().map_or((0, 0), |gram| {
                (
                    single_gram::get_freq(gram, token).map_or(0, u64::from),
                    single_gram::get_total_freq(gram).map_or(0, u64::from),
                )
            }),
            _ => (0, 0),
        };
        // The session holds no unigram rows: a user item's count is its
        // arena field, a system item's frequency its live field, and the
        // facade total's user side one scalar.
        Ok(UserCountDelta {
            bigram_count,
            bigram_total,
            unigram_delta: self.unigram_delta(token),
            unigram_total_delta: self.unigram_total(),
            unigram_live: self.item_unigram(token),
        })
    }

    /// Record a training selection of `cur` after `last` (the `pinyin_train`
    /// path, §2). Returns the seed applied.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the observation cannot be committed,
    /// [`UserStoreError::ShortUserBigramValue`] when `last`'s stored
    /// value is too short for its total (the pin's abort),
    /// or [`UserStoreError::PartialUserBigramTail`] when it has a partial
    /// item tail; nothing changes then.
    pub fn observe_selection(
        &mut self,
        last: PhraseToken,
        cur: PhraseToken,
    ) -> Result<u64, UserStoreError> {
        let seed = self.update(last, cur, SeedPolicy::Training)?;
        self.inner.dirty = true;
        Ok(seed)
    }

    /// Pronunciation half of train_result3 (074a2219 phonetic_lookup.h:925-927).
    pub(crate) fn train_pronunciations(
        &mut self,
        token: PhraseToken,
        readings: &mut impl crate::model::ReadingWalk,
        seed: u64,
    ) {
        self.arena_train(token, readings, seed as u32);
    }

    /// Record an accepted *predicted* candidate `cur` after `last` (the
    /// `pinyin_choose_predicted_candidate` path, §2). Returns the seed
    /// applied.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the observation cannot be committed.
    pub fn observe_predicted(
        &mut self,
        last: PhraseToken,
        cur: PhraseToken,
    ) -> Result<u64, UserStoreError> {
        self.update(last, cur, SeedPolicy::Predicted)
    }

    /// Record an accepted predicted prefix candidate, training only its
    /// unigram (`074a2219 pinyin.cpp:2607-2616`).
    ///
    /// Returns the flat predicted seed (69); the unigram increment is that
    /// seed times seven (483). The C facade uses only whether this succeeds.
    /// No bigram is written and the modified/save gate is left unchanged.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError::UnigramTotalOverflow`] if the library total
    /// cannot accept 483. The facade-total increment is retained even on
    /// refusal, as at the pin. Other store failures return [`UserStoreError`].
    pub fn observe_predicted_prefix(&mut self, cur: PhraseToken) -> Result<u64, UserStoreError> {
        let seed = seed::predicted_seed();
        let delta = seed::unigram_delta(seed) as u32;
        let accepted = self.arena_add_unigram(cur, delta);
        self.mark_committed_write(true);
        if !accepted {
            return Err(UserStoreError::UnigramTotalOverflow);
        }
        Ok(seed)
    }

    /// One training or predicted-selection step on `last`'s gram and
    /// `cur`'s unigram: the pin's sequences over the raw gram
    /// ([`trained_gram`], [`predicted_gram`]), the unigram by `seed * 7`.
    fn update(
        &mut self,
        last: PhraseToken,
        cur: PhraseToken,
        policy: SeedPolicy,
    ) -> Result<u64, UserStoreError> {
        // #lizard forgives(cyclomatic_complexity): the training and
        // predicted-selection policies meet at four shared steps with a
        // different failure sequence at each; a split by policy duplicates
        // the steps, and a split by step hides which policy fails where.
        let mut row = match policy {
            SeedPolicy::Training => self.training_gram(last)?,
            SeedPolicy::Predicted => self.gram_in(last)?,
        };
        // Class (b), register row 100: the pin's `get_freq`/`insert_freq`/
        // `set_freq` walks read past a partial item (`ngram.cpp:190`,
        // `:241`, `:261`); refused before anything changes.
        if row.as_deref().is_some_and(single_gram::has_partial_tail) {
            return Err(UserStoreError::PartialUserBigramTail);
        }
        // Register row 85: the pin asserts on a total it cannot read
        // (`memory_chunk.h:390`). Training reads it first
        // (`phonetic_lookup.h:884`); an accepted predicted candidate reads it
        // after its unigram add, whose overflow returns `false` before the
        // bigram is loaded (`pinyin.cpp:2609-2630`). The add is in memory
        // and dies with the pin.
        if row
            .as_deref()
            .is_some_and(|gram| gram.len() < single_gram::TOTAL_LEN)
        {
            if matches!(policy, SeedPolicy::Predicted) {
                let delta =
                    u32::try_from(seed::unigram_delta(seed::predicted_seed())).unwrap_or(u32::MAX);
                let accepted = self.arena_add_unigram(cur, delta);
                self.mark_committed_write(true);
                if !accepted {
                    return Err(UserStoreError::UnigramTotalOverflow);
                }
            }
            return Err(UserStoreError::ShortUserBigramValue);
        }
        let (seed, gram) = match policy {
            SeedPolicy::Training => {
                trained_gram(row.get_or_insert_with(single_gram::new_gram), cur)?
            }
            SeedPolicy::Predicted => {
                row = Some(predicted_gram(row, cur)?);
                (seed::predicted_seed(), row.as_deref())
            }
        };
        self.commit_step(last, cur, policy, seed, gram)
    }

    /// The writes of one [`Self::update`] step after its gram is trained:
    /// `cur`'s unigram by `seed * 7`, then `last`'s gram when there is one to
    /// store.
    fn commit_step(
        &mut self,
        last: PhraseToken,
        cur: PhraseToken,
        policy: SeedPolicy,
        seed: u64,
        gram: Option<&[u8]>,
    ) -> Result<u64, UserStoreError> {
        // `seed * unigram_factor` in `guint32`; the seeds are small.
        let delta = u32::try_from(seed::unigram_delta(seed)).unwrap_or(u32::MAX);
        let accepted = match policy {
            // The unigram add comes first and its overflow ends the call
            // before any bigram is trained (`pinyin.cpp:2609-2612`); the
            // facade total has already taken the delta.
            SeedPolicy::Predicted => self.arena_add_unigram(cur, delta),
            // `train_result3` ignores the sub-index's refusal
            // (`phonetic_lookup.h:928-929`): the item stops growing once its
            // library's `guint32` total would overflow.
            SeedPolicy::Training => {
                self.arena_add_unigram(cur, delta);
                true
            }
        };
        // `m_user_bigram->store(last_token, user)` (`phonetic_lookup.h:904`,
        // `pinyin.cpp:2638`): one store of the predecessor's gram, skipped
        // by training's overflow guard.
        let stored = if accepted && let Some(gram) = gram {
            self.store_gram(last, gram)
        } else {
            Ok(())
        };
        self.mark_committed_write(true);
        match stored {
            Ok(()) => {}
            // The failed container's `store` answers false.
            // `pinyin_choose_predicted_candidate` ignores that answer
            // (`pinyin.cpp:2638`); training's `check_result` dies on it
            // (`phonetic_lookup.h:904`), and training aborts on that answer.
            Err(_) if self.user_bigram_failed() => {
                if matches!(policy, SeedPolicy::Training) {
                    return Err(UserStoreError::FailedUserBigramStore);
                }
            }
            Err(error) => return Err(error),
        }
        if !accepted {
            return Err(UserStoreError::UnigramTotalOverflow);
        }
        Ok(seed)
    }

    /// Adds a user phrase under [`crate::vocab::USER_DICTIONARY`] with the count rules
    /// of [`Self::add_phrase_in`].
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the phrase cannot be added.
    pub fn add_phrase(
        &mut self,
        phrase: &crate::vocab::PhraseText,
        keys: &[PinyinKey],
        count: Option<u64>,
    ) -> Result<PhraseToken, UserStoreError> {
        self.add_phrase_in(USER_DICTIONARY, phrase, keys, count)
    }

    /// Whether `table.conf` gives sub-index `library` a `USER_FILE` row
    /// with a file. Addon promotion can create a live sub-index without a
    /// file. A store without a system `table.conf` keeps the stock layout.
    #[must_use]
    pub(crate) fn has_user_library(&self, library: u8) -> bool {
        self.originals.layout().has_user_library(library)
    }

    /// Borrows the session's canonical tables.
    #[must_use]
    pub fn lookup(&self) -> crate::user::UserLookup<'_> {
        crate::user::UserLookup::session(&self.inner.libpinyin.phrases, &self.inner.system_items)
    }

    /// Adds a phrase under `library` (`USER_DICTIONARY` or `NETWORK_DICTIONARY`).
    /// An absent count uses `DEFAULT_PHRASE_COUNT`. Counts above `u32::MAX`
    /// use `u32::MAX`.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the phrase cannot be added.
    pub fn add_phrase_in(
        &mut self,
        library: u8,
        phrase: &crate::vocab::PhraseText,
        keys: &[PinyinKey],
        count: Option<u64>,
    ) -> Result<PhraseToken, UserStoreError> {
        let words = phrase_words(phrase);
        if !self.has_user_library(library) || !phrase_fits_keys(&words, keys) {
            return Err(UserStoreError::InvalidPhrase);
        }
        // `_add_phrase`'s `count` reaches the item as a `guint32` delta
        // (`add_pronunciation(keys, count)`, `pinyin.cpp:579`, `:603`): the
        // C ABI hands its `gint`'s bit pattern over; a wider caller value
        // saturates.
        let delta = u32::try_from(count.unwrap_or(DEFAULT_PHRASE_COUNT)).unwrap_or(u32::MAX);
        self.arena_add_phrase(
            library,
            &words,
            keys,
            delta,
            arena::PhraseSelection::SearchTable,
            1,
        )
    }

    /// `FacadePhraseIndex::get_range(ADDON_DICTIONARY, range)`'s
    /// `m_range_end` (`pinyin.cpp:2541-2542`, `phrase_index.cpp:608-622` at
    /// 074a2219): the end of sub-index 5's range, masked into the library.
    /// `None` is `ERROR_NO_SUB_PHRASE_INDEX`: `table.conf` gives library 5
    /// no row, so the pin has no sub-index there and writes nothing to the
    /// range. An empty sub-index answers slot 1 (`phrase_index.cpp:628-633`).
    #[must_use]
    pub fn addon_promotion_range_end(&self) -> Option<PhraseToken> {
        if let Some(original) = self.originals.get(&ADDON_DICTIONARY) {
            let items = &self.inner.system_items;
            return Some(crate::vocab::phrase_index_make_token(
                ADDON_DICTIONARY,
                items.range_end(ADDON_DICTIONARY, original),
            ));
        }
        let phrases = self.inner.libpinyin.phrases();
        let arena = phrases.libraries.get(&ADDON_DICTIONARY);
        if arena.is_none()
            && !self.has_user_library(ADDON_DICTIONARY)
            && !self.is_system_library(ADDON_DICTIONARY)
        {
            return None;
        }
        let end = arena.map_or(1, crate::user::user_library::UserLibrary::token_end);
        Some(crate::vocab::phrase_index_make_token(ADDON_DICTIONARY, end))
    }

    /// Copy a chosen addon phrase item into the default facade under `token`
    /// (`pinyin.cpp:2547-2558` at 074a2219): the pinyin index gets one record
    /// per stored reading with its packed keys as stored, tones included, the
    /// phrase table gets the stored words, and `add_phrase_item` puts the
    /// item's bytes in the sub-index the token's own library bits name, which
    /// it makes when it is missing (`phrase_index.h:663-671`). The caller
    /// read `token` from the range, [`Self::addon_promotion_range_end`].
    ///
    /// `item` is the addon item's wire bytes (`phrase_index.cpp:33-43`). An
    /// item with no reading skips the index loop and is still written.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError::InvalidPhrase`] when `item` is not a whole
    /// item or its length is past `MAX_PHRASE_LENGTH`: the pin copies words
    /// and readings into arrays of that size (`pinyin.cpp:2548`, `:2555`),
    /// so a longer item is undefined there. Returns another
    /// [`UserStoreError`] when the promotion cannot be written.
    pub fn promote_addon_phrase(
        &mut self,
        token: PhraseToken,
        item: &[u8],
    ) -> Result<(), UserStoreError> {
        let view = crate::data::phrase_library::PhraseItemView::from_bytes(item)
            .ok_or(UserStoreError::InvalidPhrase)?;
        if view.phrase_length() > crate::vocab::MAX_PHRASE_LENGTH {
            return Err(UserStoreError::InvalidPhrase);
        }
        self.arena_promote(token, &view)
    }

    /// Read the stored count without decoding text or keys.
    #[must_use]
    pub(crate) fn user_pronunciation_count(&self, token: PhraseToken) -> Option<u32> {
        self.inner
            .libpinyin
            .phrases()
            .libraries
            .get(&phrase_index_library_index(token))
            .and_then(|library| library.pronunciation_count(token & PHRASE_MASK))
            .or_else(|| self.with_appended_item(token, |item| item.n_pronunciations() as u32))
    }

    /// Whether this store owns the phrase, without materializing its fields.
    #[must_use]
    pub(crate) fn phrase_exists(&self, token: PhraseToken) -> bool {
        self.with_user_item(token, |_| ()).is_some()
    }

    /// The stored word count without copying or decoding the phrase text.
    #[must_use]
    pub fn phrase_length(&self, token: PhraseToken) -> Option<usize> {
        self.with_user_item(token, |item| item.phrase_length())
    }

    /// One reading's stored key bytes, borrowed from a session-arena item.
    ///
    /// `read` runs while the item view is live and receives the item's stored
    /// length and the reading's packed little-endian words, or `None` when
    /// the item has no such reading. `None` is returned when no session item
    /// backs `token`. A session item hands out its packed words verbatim,
    /// whether or not they name a syllable, and the token getters copy them
    /// once into the caller's array, as the pin's `get_nth_pronunciation`
    /// does (074a2219 `phrase_index.cpp:33-50`).
    pub(crate) fn with_nth_pronunciation_words<R>(
        &self,
        token: PhraseToken,
        nth: usize,
        read: impl FnOnce(usize, Option<&[u8]>) -> R,
    ) -> Option<R> {
        self.with_user_item(token, |item| {
            read(
                item.phrase_length(),
                item.pronunciation(nth).map(|reading| reading.keys),
            )
        })
    }

    /// Phrase text and pronunciations for `token`, if this store owns it.
    #[must_use]
    pub fn phrase(&self, token: PhraseToken) -> Option<UserPhrase> {
        let phrases = self.inner.libpinyin.phrases();
        match phrases.item(token) {
            Some(item) => Some(phrases.phrase(token, item)),
            None => self.with_appended_item(token, |item| phrases.phrase(token, item)),
        }
    }

    /// Token already allocated for `phrase` in the user sub-index, if any.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the store cannot be read.
    #[cfg(any(test, feature = "test-hooks"))]
    pub fn token_for_phrase(&self, phrase: &str) -> Result<Option<PhraseToken>, UserStoreError> {
        self.token_for_phrase_in(USER_DICTIONARY, phrase)
    }

    /// Token already allocated for `phrase` in `library`, if any.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the store cannot be read.
    #[cfg(any(test, feature = "test-hooks"))]
    pub fn token_for_phrase_in(
        &self,
        library: u8,
        phrase: &str,
    ) -> Result<Option<PhraseToken>, UserStoreError> {
        Ok(self
            .phrase_table_tokens_for_text(phrase)?
            .into_iter()
            .find(|&token| phrase_index_library_index(token) == library))
    }

    /// The pin's user phrase-table search, preserving stored token multiplicity.
    ///
    /// # Errors
    /// Returns an error when the record cannot be read.
    #[cfg(any(test, feature = "test-hooks"))]
    pub fn phrase_table_tokens_for_text(
        &self,
        text: &str,
    ) -> Result<Vec<PhraseToken>, UserStoreError> {
        self.phrase_table_tokens_for_ucs4(&text.chars().map(u32::from).collect::<Vec<_>>())
    }

    /// The pin's user phrase-table search over raw UCS-4 words, preserving
    /// stored token multiplicity. No `char` or `str` is built from a
    /// non-scalar word.
    ///
    /// # Errors
    /// Returns an error when the record cannot be read.
    pub(crate) fn phrase_table_tokens_for_ucs4(
        &self,
        words: &[u32],
    ) -> Result<Vec<PhraseToken>, UserStoreError> {
        self.inner
            .libpinyin
            .phrases()
            .phrase_table
            .tokens(&phrase_table::key_ucs4(words))
            .map_err(|_| UserStoreError::Persistence)
    }

    /// Whether the user phrase table holds this exact key, including an
    /// empty prefix marker (`SEARCH_CONTINUED` in the pin).
    #[must_use]
    pub(crate) fn phrase_table_contains_ucs4(&self, words: &[u32]) -> bool {
        self.inner
            .libpinyin
            .phrases()
            .phrase_table
            .contains_key(&phrase_table::key_ucs4(words))
    }

    /// Current write generation: moves on every committed write, counts
    /// included.
    #[must_use]
    pub(crate) fn generation(&self) -> u64 {
        self.write_generation()
    }

    /// `m_modified` (§4).
    #[must_use]
    pub fn is_modified(&self) -> bool {
        self.inner.dirty
    }

    /// Arm `m_modified` without a data write.
    pub fn mark_modified(&mut self) {
        self.inner.dirty = true;
    }

    /// Every stored phrase (user and network) with pronunciations.
    #[cfg(any(test, feature = "test-hooks"))]
    #[must_use]
    pub fn phrases(&self) -> Vec<UserPhrase> {
        let phrases = self.inner.libpinyin.phrases();
        phrases
            .libraries
            .iter()
            .flat_map(|(&nibble, library)| {
                let phrases = &*phrases;
                library
                    .items()
                    .map(move |(slot, item)| phrases.phrase((u32::from(nibble) << 24) | slot, item))
            })
            .collect()
    }

    /// `SingleGram::retrieve_all` on `prev`'s raw gram: its whole items as
    /// `(token, count)` pairs in stored order, duplicates included
    /// (`ngram.cpp:133-151`). Empty when `prev` has no gram.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the store cannot be read.
    pub fn bigram_successors(
        &self,
        prev: PhraseToken,
    ) -> Result<Vec<(PhraseToken, u64)>, UserStoreError> {
        Ok(self.bigram_row(prev)?.map_or_else(Vec::new, |gram| {
            single_gram::retrieve_all(&gram)
                .map(|(token, count)| (token, u64::from(count)))
                .collect()
        }))
    }

    /// Whether this session has a user-directory argument, including "".
    /// A transient NULL-user session has mutable state but no directory.
    #[must_use]
    pub(crate) fn has_user_directory(&self) -> bool {
        self.inner.libpinyin.dir.is_some()
    }

    // pinyin.cpp:933–1130 and zhuyin.cpp:585–698 define the distinct save sequences.
    /// Saves the profile and reports soft write and rename failures on standard error.
    /// Missing user storage or a clean store requires no write. Only a completed save clears
    /// the dirty flag.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] if exporting or encoding fails or a hard filesystem
    /// failure stops the save. Files written before that point may remain.
    ///
    /// # Aborts
    ///
    /// A failed chunk length or checksum write, or a fatal record-copy failure,
    /// aborts at the failed operation.
    #[cfg(any(test, feature = "test-hooks"))]
    pub fn save_reporting(&mut self) -> Result<(), UserStoreError> {
        self.save_reporting_compacted(&[])
    }

    // FacadePhraseIndex::compact, phrase_index.cpp:648–675.
    /// Saves the profile with the supplied library indices marked as compacted. Their empty
    /// sub-indexes contribute a zero total to the saved log.
    ///
    /// # Errors
    ///
    /// Returns the same [`UserStoreError`] as [`Self::save_reporting`]. Earlier file changes
    /// can survive a hard error.
    ///
    /// # Aborts
    ///
    /// As [`Self::save_reporting`].
    #[cfg(any(test, feature = "test-hooks"))]
    pub(crate) fn save_reporting_compacted(
        &mut self,
        compacted: &[u8],
    ) -> Result<(), UserStoreError> {
        self.save_reporting_with_baseline(compacted, |nibble, originals| {
            match crate::user::read_save_baseline(originals, 0, nibble) {
                Ok(baseline) => baseline,
                Err(crate::user::SaveRemapFault::Unmappable(_)) => {
                    crate::user::SaveBaseline::Unmappable
                }
            }
        })
    }

    /// Saves with the current-on-disk baseline
    /// hook. The store calls it once per selected system library, in the
    /// pin's `_write_files` order, immediately before that library's log is
    /// staged; the hook maps the current chunk and emits any layout
    /// critical at that point (`pinyin.cpp:956`, `zhuyin.cpp:589`).
    pub(crate) fn save_reporting_with_baseline(
        &mut self,
        compacted: &[u8],
        mut baseline: impl FnMut(u8, &crate::user::SystemOriginals) -> crate::user::SaveBaseline,
    ) -> Result<(), UserStoreError> {
        if !self.is_modified() {
            return Ok(());
        }
        let target = Rc::clone(&self.inner.libpinyin);
        let Some(dir) = target.dir.as_deref() else {
            return Ok(());
        };
        self.compact_libraries();
        let originals = &self.originals;
        let bigram_db = self.container();
        let phrases = target.phrases();
        let live = &self.inner.system_items;
        {
            let mut reporting = crate::user::SaveReporting {
                versions: &target.versions,
                open_counter: target.open_counter,
                bigram_db,
                interleaved: target.law == crate::user::UserConfLaw::Zhuyin,
                baseline: |nibble| baseline(nibble, originals),
            };
            crate::user::save_reporting(
                dir,
                crate::user::SaveSource {
                    libraries: &phrases,
                    compacted,
                    live,
                },
                originals,
                &mut reporting,
            )?;
        }
        self.inner.dirty = false;
        Ok(())
    }

    /// `Bigram::get_all_items` (`pinyin.cpp:779`): every predecessor with
    /// a stored gram, in the walk order of the pin's own in-memory
    /// container on a libpinyin session; ascending on a store without
    /// one. A key that is not a four-byte token is left out.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the store or the container cannot
    /// be read.
    pub(crate) fn bigram_predecessors(&self) -> Result<Vec<PhraseToken>, UserStoreError> {
        Ok(self.gram_keys()?.into_iter().flatten().collect())
    }

    /// `Bigram::mask_out(mask, value)` on the pin's container over the keys
    /// [`Self::container_keys`] collected before the mask's fault check
    /// ([`mask_grams_over`]). Answers whether a gram remains.
    fn mask_container_over(
        &self,
        keys: Result<Vec<Option<PhraseToken>>, StoreError>,
        mask: PhraseToken,
        value: PhraseToken,
    ) -> Result<bool, UserStoreError> {
        Ok(mask_grams_over(
            &ContainerGrams(self.container()),
            keys?,
            mask,
            value,
        )?)
    }

    /// `mask_out`'s reload: each loaded system library in `originals`,
    /// freshly mapped, takes its user `.dbin` read now and filtered by the
    /// mask (`merge_with_mask`), library by library in the pin's order. Live
    /// item and pronunciation rows are rebuilt; independent phrase-table
    /// membership remains in place, separate from the sub-index
    /// (`pinyin.cpp:1253-1285`, `zhuyin.cpp:788-820` at 074a2219).
    ///
    /// # Errors
    /// The [`JournalFault`](crate::user::JournalFault) of the first library whose log reaches a pin
    /// assertion, with nothing replaced.
    #[cfg(test)]
    pub(crate) fn reload_system_libraries(
        &mut self,
        mut originals: crate::user::SystemOriginals,
        mask: PhraseToken,
        value: PhraseToken,
    ) -> Result<(), crate::user::JournalFault> {
        self.reload_system_libraries_retaining(&mut originals, mask, value)
    }

    /// Replay a remap without losing its mappings when the journal refuses it.
    ///
    /// # Errors
    /// The same journal fault as `Self::reload_system_libraries`.
    #[cfg(test)]
    pub fn reload_system_libraries_retaining(
        &mut self,
        originals: &mut crate::user::SystemOriginals,
        mask: PhraseToken,
        value: PhraseToken,
    ) -> Result<(), crate::user::JournalFault> {
        self.reload_system_libraries_prefetched(
            originals,
            mask,
            value,
            crate::user::MaskLogs::default(),
        )
    }

    /// [`Self::reload_system_libraries_retaining`] merging the logs `logs`
    /// already holds ([`Self::read_mask_log`]) instead of reading them again;
    /// a library not in `logs` is read when the replay reaches it.
    ///
    /// # Errors
    /// The same journal fault as `Self::reload_system_libraries`.
    pub(crate) fn reload_system_libraries_prefetched(
        &mut self,
        originals: &mut crate::user::SystemOriginals,
        mask: PhraseToken,
        value: PhraseToken,
        mut logs: crate::user::MaskLogs,
    ) -> Result<(), crate::user::JournalFault> {
        let target = &self.inner.libpinyin;
        let (replayed, library_totals) = match target.dir.clone() {
            Some(dir) => {
                crate::user::load_masked_logs_live(&dir, originals, mask, value, &mut logs)?
            }
            None => (originals.take_live_libraries(), Default::default()),
        };
        self.replace_system_libraries(originals.take(), replayed, &library_totals);
        Ok(())
    }

    /// `_load_phrase_library`'s system branch for the libraries in
    /// `originals` (074a2219 `pinyin.cpp:246-277`, `zhuyin.cpp:190-221`):
    /// each is mapped again and only its saved `.dbin` is merged, so
    /// everything the session changed since the last save (trained
    /// unigrams, imported readings and items, removals) goes with the
    /// sub-index `unload` freed (`phrase_index.cpp:261-268`). A `.dbin`
    /// that cannot be read is an empty log (`MemoryChunk::load` fails and
    /// leaves the chunk empty), so the library comes back as its system
    /// chunk alone. The independent phrase-table membership stays in place,
    /// separate from the sub-index. Like the pin's, the transition is in
    /// memory and has no failure of its own: nothing is written to disk.
    ///
    /// # Errors
    /// The [`JournalFault`](crate::user::JournalFault) where the merge reaches a pin assertion, with nothing replaced.
    #[cfg(test)]
    pub fn load_system_libraries(
        &mut self,
        mut originals: crate::user::SystemOriginals,
    ) -> Result<(), crate::user::JournalFault> {
        self.load_system_libraries_retaining(&mut originals)
    }

    /// Load a remap, retaining it for dictionary publication if replay reaches
    /// a journal assertion. A transient session has no log to merge.
    ///
    /// # Errors
    /// The same journal fault as [`Self::load_system_libraries`].
    pub(crate) fn load_system_libraries_retaining(
        &mut self,
        originals: &mut crate::user::SystemOriginals,
    ) -> Result<(), crate::user::JournalFault> {
        let (replayed, library_totals) = match &self.inner.libpinyin.dir {
            Some(dir) => crate::user::load_library_logs_live(dir, originals)?,
            // No user directory means no journal; the freshly mapped system
            // library still replaces the unloaded one (phrase_index.cpp:261,
            // pinyin.cpp:246-276 at 074a2219).
            None => (originals.take_live_libraries(), Default::default()),
        };
        self.replace_system_libraries(originals.take(), replayed, &library_totals);
        Ok(())
    }

    /// `FacadePhraseIndex::unload` for the libraries in `originals`: the
    /// sub-index goes with everything the session changed in it and with
    /// its share of the facade total, whatever was saved (074a2219
    /// `phrase_index.cpp:261-268`). The user index tables keep their
    /// entries. In memory, as the pin's: nothing is written to disk, and a
    /// save skips the library while it is unloaded (`pinyin.cpp:928-933`).
    #[cfg(test)]
    pub fn unload_system_libraries(&mut self, originals: crate::user::SystemOriginals) {
        let unloaded: Vec<_> = originals.keys().copied().collect();
        self.replace_system_libraries(
            originals,
            crate::user::LibraryTable::new(),
            &crate::user::LibraryTable::new(),
        );
        self.inner.libpinyin.phrases_mut().unloaded.extend(unloaded);
    }

    /// `_load_phrase_library`'s `USER_FILE` branch for `library`: its
    /// sub-index comes back from the saved chunk, or empty when there is
    /// none or it cannot be read (`create_sub_phrase`), so everything the
    /// session changed since the last save goes (074a2219
    /// `pinyin.cpp:303-321`, `zhuyin.cpp:247-265`). The phrase table and the
    /// user pinyin index keep their entries, separate from the sub-index: a
    /// reading the index carried stays indexed on the item the chunk brings
    /// back. In memory, with no failure of its own.
    pub(crate) fn load_user_library(&mut self, library: u8) {
        let layout = self.originals.layout().clone();
        if !layout.has_user_library(library) {
            return;
        }
        // No user directory, no chunk, or one that does not load:
        // `create_sub_phrase`, an empty sub-index.
        let loaded = self
            .inner
            .libpinyin
            .dir
            .as_deref()
            .and_then(|dir| crate::user::load_user_library(dir, &layout, library));
        self.replace_user_arena(library, Some(loaded.unwrap_or_default()));
    }

    /// `FacadePhraseIndex::unload` for the `USER_FILE` `library`: the
    /// sub-index goes with everything in it and with its share of the
    /// facade total, saved or not (074a2219 `phrase_index.cpp:261-268`).
    /// The phrase table and the user pinyin index keep their entries. In
    /// memory, with no failure of its own; a save skips the library while
    /// it is unloaded (`pinyin.cpp:928-933`).
    pub(crate) fn unload_user_library(&mut self, library: u8) {
        if !self.originals.layout().has_user_library(library) {
            return;
        }
        self.replace_user_arena(library, None);
    }

    /// The session side of a system library transition: each reloaded
    /// library's live buffer and total are replaced by the one its replay
    /// left in `replayed` (none when its log left the chunk as it is). The
    /// session's state is process memory, so this cannot fail, as
    /// `FacadePhraseIndex::load`/`unload` cannot.
    fn replace_system_libraries(
        &mut self,
        originals: crate::user::SystemOriginals,
        replayed: crate::user::LibraryTable<crate::user::replay::ReplayedLibrary>,
        library_totals: &crate::user::LibraryTable<u32>,
    ) {
        let libraries: Vec<u8> = originals.keys().copied().collect();
        let has_user_data = self.apply_system_libraries(&libraries, replayed, library_totals);
        self.mark_committed_write(has_user_data);
        {
            let mut phrases = self.inner.libpinyin.phrases_mut();
            for nibble in originals.keys() {
                phrases.unloaded.remove(nibble);
            }
        }
        self.originals.append(originals);
    }

    /// Synchronize a system library's mutation guard with runtime visibility.
    /// This does not replace its items or reset its training state. USER_FILE
    /// markers belong exclusively to the store's own item load/unload.
    pub(crate) fn set_library_loaded(&mut self, nibble: u8, loaded: bool) {
        if nibble >= 16 || !self.is_system_library(nibble) {
            return;
        }
        let mut phrases = self.inner.libpinyin.phrases_mut();
        if loaded {
            phrases.unloaded.remove(&nibble);
        } else {
            phrases.unloaded.insert(nibble);
        }
    }

    /// Reset one loaded baseline's replay and training state at unload.
    pub(crate) fn reset_system_library(&mut self, nibble: u8) {
        if nibble >= 16 {
            return;
        }
        let loaded = self.originals.contains_key(&nibble).then_some(nibble);
        let has_user_data = self.apply_system_libraries(
            loaded.as_slice(),
            crate::user::LibraryTable::new(),
            &Default::default(),
        );
        self.mark_committed_write(has_user_data);
        self.inner.libpinyin.phrases_mut().unloaded.insert(nibble);
    }

    /// Replaces the live state of each library in `libraries`, in nibble
    /// order, with its replay or its total.
    fn apply_system_libraries(
        &mut self,
        libraries: &[u8],
        mut replayed: crate::user::LibraryTable<crate::user::replay::ReplayedLibrary>,
        library_totals: &crate::user::LibraryTable<u32>,
    ) -> bool {
        let target = &self.inner.libpinyin;
        let offsets = &mut self.inner.offsets;
        // A reloaded library has no stored total difference any more: its
        // total is the one its replay left, held in the live scalar below.
        for nibble in libraries {
            offsets.remove(nibble);
        }
        {
            // A library with an original keeps its items in its live buffer;
            // the arenas of the reloaded libraries hold nothing.
            let mut phrases = target.phrases_mut();
            phrases
                .libraries
                .retain(|nibble, _| !libraries.contains(nibble));
        }
        {
            let phrases = target.phrases();
            let arena_has_system_items = phrases
                .libraries
                .iter()
                .any(|(library, items)| (1..=4).contains(library) && !items.is_empty());
            let items = &mut self.inner.system_items;
            // `FacadePhraseIndex::load` and `merge_with_mask` swap the old
            // sub-index for the new one (`phrase_index.cpp:237-248`,
            // `:301-332`): the library's live buffer and total go, and the
            // replay's buffer and total come in. The facade's total follows
            // the sub-index totals, and keeps what no sub-index took.
            for &nibble in libraries {
                items.forget_library(nibble);
                match replayed.remove(&nibble) {
                    Some(library) => items.insert_library(nibble, library),
                    None => {
                        if let Some(&total) = library_totals.get(&nibble) {
                            items.set_library_total(nibble, total);
                        }
                    }
                }
            }
            self.inner.has_system_items = !items.is_empty() || arena_has_system_items;
        }
        let arena_live = target.phrases().has_items();
        // A session's grams are user data too.
        let has_grams = self.container().is_empty().is_ok_and(|empty| !empty);
        has_grams || arena_live
    }

    /// The faults [`Self::mask_out`] reaches before it changes anything, in
    /// the pin's order (`pinyin.cpp:1228-1230`): an overlong user pinyin index
    /// key, a phrase-table record the mask would read past, then the user bigram's non-token key, short value, partial
    /// tail and residual gram. It changes no state, so a
    /// caller can learn that the user tables fault before it opens any system
    /// library or journal. The answer carries the one walk of the bigram
    /// keys to [`Self::apply_mask_out`], which checks nothing again.
    ///
    /// # Errors
    ///
    /// The [`UserStoreError`] [`Self::mask_out`] would return.
    pub(crate) fn check_mask_out(
        &self,
        mask: PhraseToken,
        value: PhraseToken,
    ) -> Result<MaskCheck, UserStoreError> {
        if self.has_overlong_index_key() {
            return Err(UserStoreError::OverlongIndexKey);
        }
        self.inner
            .libpinyin
            .phrases()
            .phrase_table
            .check_mask()
            .map_err(|_| UserStoreError::Persistence)?;
        let container_keys = self.container_keys();
        match self.bigram_mask_fault_over(&container_keys, mask, value) {
            Some(error) => Err(error),
            None => Ok(MaskCheck { container_keys }),
        }
    }

    /// `pinyin_mask_out`'s store side.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError::OverlongIndexKey`] when the user pinyin
    /// index carried a key past `MAX_PHRASE_LENGTH` syllables — the pin's
    /// user `ChewingLargeTable2::mask_out` walks every record and aborts on
    /// it (`chewing_large_table2_bdb.cpp:529`) — or [`UserStoreError`] when
    /// the mask cannot otherwise be written.
    #[cfg(any(test, feature = "test-hooks"))]
    pub fn mask_out(
        &mut self,
        mask: PhraseToken,
        value: PhraseToken,
    ) -> Result<(), UserStoreError> {
        let checked = self.check_mask_out(mask, value)?;
        self.apply_mask_out(checked, mask, value)
    }

    /// [`Self::mask_out`] after [`Self::check_mask_out`] passed for the same
    /// `mask` and `value`, with no write to the user tables in between: the
    /// index, the phrase table, the arenas and the bigram take the mask, and
    /// nothing is checked again. The bigram mask walks the keys the check
    /// collected.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the mask cannot be written.
    pub(crate) fn apply_mask_out(
        &mut self,
        checked: MaskCheck,
        mask: PhraseToken,
        value: PhraseToken,
    ) -> Result<(), UserStoreError> {
        let MaskCheck { container_keys } = checked;
        let target = &self.inner.libpinyin;
        // The pin's mask walk edits every record of the user pinyin index,
        // in its native container.
        {
            let mut phrases = target.phrases_mut();
            phrases.index.mask(mask, value);
            phrases
                .phrase_table
                .mask(mask, value)
                .map_err(UserStoreError::from)?;
        }
        // Commit succeeded: discard matched in-memory system payloads too.
        // Dropping an original's override restores it; dropping an appended
        // item removes its only payload. Do this before publishing the epoch.
        self.mask_arena(mask, value);
        // `m_user_bigram->mask_out(mask, value)` (`pinyin.cpp:1230`,
        // `zhuyin.cpp:765`).
        let grams_left = self.mask_container_over(container_keys, mask, value)?;
        self.mark_committed_write(grams_left || self.inner.libpinyin.phrases().has_items());
        Ok(())
    }

    /// `pinyin_remove_user_candidate`'s store side (§3.4).
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the phrase cannot be removed.
    pub fn remove_user_phrase(&mut self, token: PhraseToken) -> Result<bool, UserStoreError> {
        self.arena_remove(token)
    }
}

impl UserStore {
    /// The system items the live buffers hold that differ from the originals.
    #[cfg(test)]
    pub(crate) fn live_system_changes(
        &self,
        originals: &crate::user::SystemOriginals,
    ) -> std::collections::BTreeMap<PhraseToken, Option<crate::data::phrase_library::ChunkItem>>
    {
        self.inner.system_items.changed_items(originals)
    }

    /// Whether this session carries system-item payloads from imports or logs.
    #[must_use]
    pub(crate) fn has_system_items(&self) -> bool {
        self.inner.has_system_items
    }

    /// Whether a journal REMOVE took the item behind system `token` out of
    /// its sub-index: `get_phrase_item` fails for it (074a2219
    /// `phrase_index.cpp:219-236`), while the shipped chunk and the phrase
    /// table still hold it. The library's live buffer answers no item where
    /// the chunk holds one.
    #[must_use]
    pub(crate) fn system_item_removed(&self, token: PhraseToken) -> bool {
        self.inner.system_items.is_removed(token)
    }

    #[cfg(test)]
    /// The imported/replayed system item, with its current unigram field.
    pub(crate) fn system_item_override(
        &self,
        token: PhraseToken,
    ) -> Option<crate::data::phrase_library::ChunkItem> {
        if !(1..=4).contains(&(token >> 24)) {
            return None;
        }
        let mut item = self.with_effective_system_item(token, Clone::clone)?;
        if let Some(unigram) = self.system_unigram(token) {
            item.unigram = unigram;
        }
        Some(item)
    }

    /// Lends the item this store holds behind `token` to `read`, in place:
    /// a session arena's or an added item, else a system library's live
    /// item (`get_phrase_item`, 074a2219 `phrase_index.h:646-657`). `None`
    /// where the store holds none, so the caller reads the loaded library.
    pub(crate) fn with_effective_item<R>(
        &self,
        token: PhraseToken,
        read: impl FnOnce(PhraseItemView<'_>) -> R,
    ) -> Option<R> {
        let mut read = Some(read);
        if let Some(found) = self
            .with_user_item(token, |item| read.take().map(|read| read(item)))
            .flatten()
        {
            return Some(found);
        }
        if !self.is_system_library(phrase_index_library_index(token)) || !self.has_system_items() {
            return None;
        }
        let read = read?;
        self.inner.system_items.read_complete(token, read)
    }

    /// The live sub-index's range end for the export probe. A promotion can
    /// create a sub-index with no original (074a2219 phrase_index.h:663-670).
    pub(crate) fn system_range_end(&self, library: u8) -> Option<PhraseToken> {
        let end = if let Some(original) = self.originals.get(&library) {
            self.inner.system_items.range_end(library, original)
        } else {
            let phrases = self.inner.libpinyin.phrases();
            phrases
                .libraries
                .get(&library)
                .map(crate::user::user_library::UserLibrary::token_end)
                .or_else(|| self.has_user_library(library).then_some(1))?
        };
        Some(crate::vocab::phrase_index_make_token(library, end))
    }

    /// The effective live system item shared by export, n-best and token
    /// introspection (074a2219 pinyin.cpp:707-724; phonetic_lookup.h:643-693).
    /// Original counts plus exact-reading deltas are resolved once when
    /// training commits, with the pin's prefix-total overflow guard, rather
    /// than rereading the store at every pronunciation probability. Imports
    /// and replay publish their absolute payloads into the same view.
    /// Untouched items stay mapped: `None` tells the caller to read the
    /// original. A borrowed item is stable for this call and copied nowhere.
    /// Only its unigram field is stale; the test-only
    /// `Self::system_item_override` supplies the live unigram.
    #[cfg(test)]
    pub(crate) fn with_effective_system_item<R>(
        &self,
        token: PhraseToken,
        read: impl FnOnce(&crate::data::phrase_library::ChunkItem) -> R,
    ) -> Option<R> {
        if !self.is_system_library(phrase_index_library_index(token)) {
            return None;
        }
        if let Some(item) = self
            .with_user_item(token, |view| {
                crate::data::phrase_library::decode_phrase_item(view.as_bytes())
            })
            .flatten()
        {
            return Some(read(&item));
        }
        if !self.has_system_items() {
            return None;
        }
        let items = &self.inner.system_items;
        items.get(&token).map(|item| read(&item))
    }

    /// Reads the item a system library's live buffer holds under `token`.
    /// Nothing is decoded or kept.
    pub(crate) fn with_live_item<R>(
        &self,
        token: PhraseToken,
        read: impl FnOnce(&[u8]) -> R,
    ) -> Option<R> {
        if !self.has_system_items() {
            return None;
        }
        self.inner.system_items.with_live(token, read)
    }

    /// Reads the complete item a system library's live buffer holds under
    /// `token`, or says that the library has no live buffer or that the
    /// buffer holds no item there ([`Self::system_item_removed`]). Unlike
    /// [`Self::with_live_item`], the answer does not depend on
    /// [`Self::has_system_items`].
    pub(crate) fn read_live_item<R>(
        &self,
        token: PhraseToken,
        read: impl FnOnce(&[u8]) -> R,
    ) -> crate::user::system_items::LiveItemFields<R> {
        self.inner.system_items.read_live(token, read)
    }

    /// [`crate::user::system_items::SystemItems::live_words`] while the
    /// session holds system items; a store without them has no live
    /// buffer to read.
    pub(crate) fn live_words(
        &self,
        token: PhraseToken,
    ) -> crate::user::system_items::LiveItemFields<
        Option<smallvec::SmallVec<[u32; crate::vocab::MAX_PHRASE_LENGTH]>>,
    > {
        if !self.has_system_items() {
            return crate::user::system_items::LiveItemFields::NoLiveLibrary;
        }
        self.inner.system_items.live_words(token)
    }

    pub(crate) fn with_live_item_fields<R>(
        &self,
        token: PhraseToken,
        read: impl FnOnce(&[u8]) -> R,
    ) -> crate::user::system_items::LiveItemFields<R> {
        if !self.has_system_items() {
            return crate::user::system_items::LiveItemFields::NoLiveLibrary;
        }
        self.inner.system_items.with_live_fields(token, read)
    }

    /// The stored reading count of the item fields a system library's live
    /// buffer holds under `token`, read once: the library has no live
    /// buffer, the buffer holds no item fields there, or the header's count.
    pub(crate) fn live_pronunciation_count_read(
        &self,
        token: PhraseToken,
    ) -> crate::user::system_items::LiveItemFields<Option<u32>> {
        self.inner
            .system_items
            .with_live_fields(token, |bytes| bytes.get(1).copied().map(u32::from))
    }

    /// One past the highest token a `USER_FILE` library holds: the pin's
    /// `FacadePhraseIndex::get_range` end (`phrase_index.cpp:610-646` at
    /// 074a2219, the offset array less its trailing zeros, masked into the
    /// library nibble, so a library whose last slot is live ends at its
    /// slot 0). It is the library's allocation cursor, which every add
    /// moves past the token it took and every mask or removal re-derives
    /// from the surviving items; an empty library ends at its first token,
    /// the pin's `1..1`.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the cursor cannot be read, or
    /// `InvalidPhrase` for a system library.
    #[cfg(test)]
    pub fn user_library_token_end(&self, library: u8) -> Result<PhraseToken, UserStoreError> {
        if self.is_system_library(library) {
            return Err(UserStoreError::InvalidPhrase);
        }
        Ok(crate::vocab::phrase_index_make_token(
            library,
            self.inner
                .libpinyin
                .phrases()
                .libraries
                .get(&library)
                .map_or(1, crate::user::user_library::UserLibrary::token_end),
        ))
    }

    /// `_add_phrase` for a system library (074a2219, pinyin.cpp:515-611).
    /// `original_token` comes from the combined system/user phrase-table
    /// search, restricted to this library, including prior imported items.
    /// Packed original readings are retained verbatim, including their order.
    /// An absent count uses `DEFAULT_PHRASE_COUNT`. Counts above `u32::MAX`
    /// use `u32::MAX`.
    ///
    /// # Errors
    /// Returns an error for invalid input, unavailable originals, or store failure.
    pub(crate) fn add_system_phrase_in(
        &mut self,
        library: u8,
        original_token: Option<PhraseToken>,
        text: &crate::vocab::PhraseText,
        keys: &[PinyinKey],
        count: Option<u64>,
    ) -> Result<PhraseToken, UserStoreError> {
        let words = phrase_words(text);
        if !self.is_system_library(library) || !phrase_fits_keys(&words, keys) {
            return Err(UserStoreError::InvalidPhrase);
        }
        let originals = &self.originals;
        let original = originals
            .get(&library)
            .ok_or(UserStoreError::InvalidPhrase)?;
        let range_end = original.range_end;
        let delta = u32::try_from(count.unwrap_or(DEFAULT_PHRASE_COUNT)).unwrap_or(u32::MAX);
        if original_token.is_none_or(|token| self.with_appended_item(token, |_| ()).is_some()) {
            return self.arena_add_phrase(
                library,
                &words,
                keys,
                delta,
                match original_token {
                    None => arena::PhraseSelection::NewPhrase,
                    Some(token) => arena::PhraseSelection::UseToken(token),
                },
                range_end,
            );
        }
        let token = original_token.ok_or(UserStoreError::InvalidPhrase)?;
        let packed = crate::user::store_libpinyin::pinyin_keys_to_packed(keys)
            .ok_or(UserStoreError::InvalidPhrase)?;
        let reading = SystemReading {
            packed: &packed,
            delta,
        };
        let items = &mut self.inner.system_items;
        let mut item = items
            .get(&token)
            .or_else(|| original.item(token & PHRASE_MASK))
            .ok_or(UserStoreError::InvalidPhrase)?;
        let change = plan_system_reading(&item, &reading);
        validate_system_merge(&item, &reading, change)?;
        apply_system_reading(&mut item, &reading, change);
        // `remove_phrase_item`, `add_pronunciation`, `add_phrase_item` over
        // the buffer (`pinyin.cpp:559-569`). A slot the journal removed has
        // nothing to remove, so its total is left as the removal left it.
        let bytes = crate::data::phrase_library::encode_phrase_item(&item)
            .ok_or(UserStoreError::Persistence)?;
        let removed = items.remove_item(original, token);
        items.add_item(original, token, &bytes);
        if !removed {
            let library = phrase_index_library_index(token);
            let total = items
                .library_total(library, Some(original))
                .unwrap_or(original.total);
            items.set_library_total(
                library,
                total.wrapping_sub(crate::user::replay::raw_unigram(&bytes)),
            );
        }
        self.inner.has_system_items = true;
        self.mark_committed_write(true);
        Ok(token)
    }
}

/// Test support: a session filled from a value state.
#[cfg(test)]
impl UserStore {
    /// Puts `state`'s values into this session's own structures: the user
    /// bigram container, the user library arenas, the two user tables and
    /// the live system buffers.
    pub(crate) fn seed_from_state(&mut self, state: &crate::user::UserState) {
        use crate::data::phrase_library::encode_phrase_item;
        use crate::store::UserBigramDb as _;
        for (key, value) in &state.bigram {
            self.container().store(key, value).expect("store a gram");
        }
        let target = &self.inner.libpinyin;
        let mut phrases = target.phrases_mut();
        for (&nibble, items) in &state.libraries {
            for (&slot, item) in items {
                let bytes = encode_phrase_item(item).expect("item");
                phrases
                    .libraries
                    .get_or_default(nibble)
                    .expect("a library slot")
                    .replace(slot, &bytes)
                    .expect("place an item");
            }
        }
        phrases.index = crate::user::user_index::UserIndex::from_entries(
            &crate::data::pinyin_index_entries(&crate::user::chewing_rows(state, &self.originals)),
        );
        for (key, value) in &state.phrase_table {
            phrases.phrase_table.push_record(key, value).unwrap();
        }
        for (&nibble, overrides) in &state.system_overrides {
            for (&slot, item) in overrides {
                let token = (u32::from(nibble) << 24) | slot;
                let bytes = item
                    .as_ref()
                    .map(|item| encode_phrase_item(item).expect("item"));
                match self.originals.get(&nibble) {
                    Some(original) => {
                        let items = &mut self.inner.system_items;
                        items.remove_item(original, token);
                        if let Some(bytes) = &bytes {
                            items.add_item(original, token, bytes);
                        }
                    }
                    None => {
                        if let Some(bytes) = &bytes {
                            phrases
                                .libraries
                                .get_or_default(nibble)
                                .expect("a library slot")
                                .replace(slot, bytes)
                                .expect("place an item");
                        }
                    }
                }
            }
        }
        drop(phrases);
        self.inner.has_system_items = true;
        self.mark_committed_write(true);
    }
}

#[cfg(test)]
pub(super) mod tests;
