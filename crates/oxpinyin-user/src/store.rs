//! The libpinyin session store: the user phrase index, the user bigram and
//! the live system items of one `pinyin_init`.
//!
//! A user library's items live in an owned arena
//! ([`crate::user_library::UserLibrary`], the pin's `SubPhraseIndex`), the
//! user pinyin index and phrase table in their own tables, and a system
//! library's changes in its live buffer ([`crate::system_items`]). The user
//! bigram is held once, as the pin's own `SingleGram` values
//! (`docs/findings/user-store.md` §5), in the pin's container
//! ([`StoreInner::bigram_db`]), and every bigram operation runs the pin's
//! `SingleGram` code over those bytes ([`oxpinyin_core::single_gram`]).
//! The counts the decoder merges come back as a
//! [`oxpinyin_core::UserCountDelta`]; the save cycle is the §4 `m_modified`
//! gate and the persistence point behind `pinyin_save`.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use oxpinyin_core::{ChewingKey, Completeness, SyllableKey, UserCountDelta, single_gram};
use oxpinyin_store::{DefaultUserBigramDb, StoreError, UserBigramDb};

use crate::lock_result_or_abort;
use crate::persistence::ChunkHeaderField;
use crate::phrase::{
    self, ADD_PHRASE_UNIGRAM_FACTOR, ADDON_DICTIONARY, DEFAULT_PHRASE_COUNT,
    PHRASE_INDEX_LIBRARY_MASK, PHRASE_MASK, PinyinKey, USER_DICTIONARY, UserPhrase,
    is_user_file_library, phrase_index_library_index,
};
use crate::phrase_table;
use crate::replay::UnigramAdd;
use crate::seed;
use crate::store_libpinyin::FiniGuard;

#[path = "store_arena.rs"]
mod arena;

/// Token type — libpinyin's 32-bit `phrase_token_t`.
pub type Token = u32;

/// What a save left undone, in the pin's own terms.
///
/// The pin's `pinyin_save` and `zhuyin_save` never stop at a failure: a
/// write that fails leaves a missing `.tmp`, the rename of it fails and is
/// printed (`rename %s to %s failed.`, `pinyin.cpp:1061`…`:1123`), the
/// marker write fails and is printed (`write %s failed.`,
/// `table_info.cpp:382`), and the save answers `true`
/// (`pinyin.cpp:1132-1147`). [`UserStore::save_reporting`] prints
/// each line where the pin does, as the failure happens, and also returns
/// the failures.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SaveReport {
    /// `(tmp, final)` of every rename that failed, in the pin's order: the
    /// libraries by index, then the pinyin index, the phrase index and the
    /// bigram.
    pub renames_failed: Vec<(PathBuf, PathBuf)>,
    /// The `user.conf` whose write failed, if it did; `UserTableInfo::save`
    /// has written its `write %s failed.` line, after every rename line.
    pub user_conf_write_failed: Option<PathBuf>,
}

/// One §9 export row for a user phrase: the phrase text, one pronunciation's
/// `'`-joined pinyin spelling, and that pronunciation's stored count.
///
/// This is the `(phrase, pinyin, count)` triple
/// `pinyin_iterator_get_next_phrase` yields upstream
/// (`docs/findings/user-store.md` §9).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExportedPhrase {
    /// Exact stored phrase words.
    pub text: Vec<u32>,
    /// One pronunciation, syllables joined by `'` (e.g. `ni'hao`).
    pub pinyin: String,
    /// The pronunciation's stored count.
    pub count: u64,
}

/// `sentence_start` sentinel: the predecessor of the first phrase in a
/// sentence (`docs/findings/user-store.md` §2; `novel_types.h:122`).
pub const SENTENCE_START: Token = 1;

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
    /// The libpinyin user-dir persistence failed (I/O, container, or a
    /// byte stream that does not parse).
    Persistence(String),
    /// `user.conf` names a database format upstream's mapper does not
    /// know — the point `to_table_database_format_type` `abort()`s on
    /// (`table_info.cpp:122-133`). Returned before anything is cleaned or
    /// written; the C adapters abort.
    UnknownDatabaseFormat,
    /// A library chunk's `MemoryChunk::save` header write failed
    /// (`memory_chunk.h:543`/`:547`) — the pin `assert`s and dies of
    /// SIGABRT. The save stops there; the C adapters abort.
    ChunkHeaderWrite(ChunkHeaderField),
    /// A loaded system library's file, mapped again where the save's
    /// library loop reaches it, is a directory whose header read the pin's
    /// `MemoryChunk::mmap` asserts on (`pinyin.cpp:955`, `zhuyin.cpp:588`,
    /// `memory_chunk.h:493`; register row 96). The save stopped there with
    /// the earlier libraries' files written; the C adapters abort.
    LibraryChunkHeaderRead,
    /// `MemoryChunk::mmap` refused a loaded system library's file where the
    /// save's library loop maps it again (`pinyin.cpp:956`, `zhuyin.cpp:589`;
    /// register row 76). The save stopped at that library: the earlier
    /// libraries' files stand as written, nothing later was written or
    /// renamed, no marker was written, and the session stays dirty.
    SaveRemapFailed,
    /// A bulk copy of the save reaches the pin's unsafe size
    /// (`memory_chunk.h:95-98`; register row 112, #1514). The save stops at
    /// that copy: the earlier files stay as written (libpinyin's staged
    /// `.tmp` files, libzhuyin's completed renames), nothing later happens,
    /// no marker is written, the session stays dirty and a later save retries.
    SaveTooLarge,
    /// The unigram add of an accepted predicted candidate overflowed its
    /// library's `guint32` total: `FacadePhraseIndex::add_unigram_frequency`
    /// answers `ERROR_INTEGER_OVERFLOW` and `pinyin_choose_predicted_candidate`
    /// returns `false` before it trains the bigram (`pinyin.cpp:2609-2612`).
    UnigramTotalOverflow,
    /// `pinyin_mask_out` / `zhuyin_mask_out` met a `user_pinyin_index.bin`
    /// key past `MAX_PHRASE_LENGTH` syllables, or with no whole syllable
    /// (an empty or one-byte key): the pin's user
    /// `ChewingLargeTable2::mask_out` walks every record and its
    /// `switch`'s `default: abort()` fires on the key
    /// (`chewing_large_table2_bdb.cpp:529`). Returned before the mask
    /// changes anything; the C adapters abort.
    OverlongIndexKey,
    /// A `user_bigram.db` key is not a four-byte `phrase_token_t`:
    /// `Bigram::get_all_items` asserts `key.size == sizeof(phrase_token_t)`
    /// (`ngram_bdb.cpp:199`), and the pin's `mask_out` and
    /// `pinyin_begin_get_bigram_phrases` both drive that walk. Returned
    /// where the pin's walk meets the key; the C adapters abort.
    NonTokenUserBigramKey,
    /// A `user_bigram.db` value is shorter than a `guint32` `total_freq`:
    /// the pin's `get_total_freq` asserts on it (`memory_chunk.h:390`,
    /// through `ngram.cpp:49`; register row 85). Returned where the pin's
    /// walk reads that total; the C adapters abort on it.
    ShortUserBigramValue,
    /// A `user_bigram.db` value has an incomplete eight-byte item after
    /// its total: the pin's `cur != end` walks read past it
    /// (`ngram.cpp:76-86`, `:190`, `:241`, `:261`; register rows 98 and
    /// 100, class (b)). The mask, removal and predicted-training facades
    /// refuse it with one warning.
    PartialUserBigramTail,
    /// A `user_bigram.db` gram's `total_freq` is not covered by its items
    /// and a mask removes every item, leaving a residual total:
    /// `Bigram::mask_out` then asks `SingleGram::get_length`, whose assert
    /// dies (`ngram.cpp:70`, `ngram_bdb.cpp:243`). Returned before the
    /// mask changes anything; the C adapters abort.
    ResidualUserBigramGram,
    /// A system library's user `.dbin` replay reached a pin assertion
    /// ([`crate::JournalFault`]): at init's and a reload's `merge`, or at
    /// `mask_out`'s `merge_with_mask`. The pin dies there; the C adapters
    /// abort on it.
    Journal(crate::JournalFault),
    /// Init's library loop died at a row ([`crate::LibraryLoopStop`]).
    LibraryLoop(crate::LibraryLoopStop),
    /// A training step's `set_freq` misses the item its `insert_freq` just
    /// added: on an unsorted gram the second `lower_bound` can land past
    /// it, and the pin's `check_result(user->set_freq(token, freq + seed))`
    /// dies (`phonetic_lookup.h:903`, `ngram.cpp:252-271`; register row
    /// 85). Returned before anything is stored; the C adapters abort.
    TrainingSetFreqMiss,
    /// A user reading cannot be removed from both pinyin index projections;
    /// the pin asserts on the failed removal (`pinyin.cpp:3759`).
    UserReadingRemoval,
    /// The open could not build the session once the system libraries
    /// were handed to it: a loaded item's encoding or its placement, or
    /// the user bigram's container. The
    /// pin's counterparts die: an allocation that throws `std::bad_alloc`,
    /// or the asserts of its in-memory loads (`ngram_bdb.cpp:51-52`,
    /// `bdb_utils.h:61`, `tkrzwdb_utils.h:45`). The C adapters abort.
    SessionSeed(String),
    /// A training step would store its gram in the failed Berkeley DB user
    /// bigram container ([`oxpinyin_store::UserBigramDb::is_failed`]): its
    /// `store` answers false and the pin's
    /// `check_result(m_user_bigram->store(last_token, user))` dies
    /// (`phonetic_lookup.h:904`). Returned before anything is stored; the
    /// C adapters abort.
    FailedUserBigramStore,
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
            Self::Persistence(message) => {
                write!(f, "libpinyin user-dir persistence: {message}")
            }
            Self::UnknownDatabaseFormat => write!(
                f,
                "user.conf: unknown database format (upstream aborts, table_info.cpp:122-133)"
            ),
            Self::ChunkHeaderWrite(field) => write!(
                f,
                "user chunk header write failed ({}; upstream asserts, \
                 memory_chunk.h:543/547)",
                field.assert_expression()
            ),
            Self::LibraryChunkHeaderRead => write!(
                f,
                "a system library is a directory whose chunk header cannot be read \
                 (upstream asserts, memory_chunk.h:493)"
            ),
            Self::SaveRemapFailed => write!(
                f,
                "a system library's file cannot be mapped again for the save (upstream: mmap failed!)"
            ),
            Self::SaveTooLarge => write!(
                f,
                "a save copy reaches the pin's unsafe size (upstream memory_chunk.h:95-98)"
            ),
            Self::UnigramTotalOverflow => write!(
                f,
                "unigram total overflow (upstream ERROR_INTEGER_OVERFLOW, pinyin.cpp:2609-2612)"
            ),
            Self::OverlongIndexKey => write!(
                f,
                "user pinyin index key past MAX_PHRASE_LENGTH syllables or without a whole \
                 syllable (upstream aborts, chewing_large_table2_bdb.cpp:529)"
            ),
            Self::NonTokenUserBigramKey => write!(
                f,
                "user bigram key is not a phrase_token_t (upstream aborts, ngram_bdb.cpp:199)"
            ),
            Self::ShortUserBigramValue => write!(
                f,
                "user bigram value shorter than a guint32 total_freq (upstream asserts, \
                 memory_chunk.h:390)"
            ),
            Self::PartialUserBigramTail => write!(
                f,
                "user bigram value with a partial item tail (upstream reads past it, \
                 ngram.cpp:76-86)"
            ),
            Self::ResidualUserBigramGram => write!(
                f,
                "masking a user bigram leaves a residual total_freq (upstream aborts, ngram.cpp:70)"
            ),
            Self::Journal(fault) => write!(f, "journal replay: {fault}"),
            Self::LibraryLoop(stop) => write!(f, "library loop: {stop}"),
            Self::TrainingSetFreqMiss => write!(
                f,
                "a training step's set_freq misses the item insert_freq added \
                 (upstream asserts, phonetic_lookup.h:903)"
            ),
            Self::UserReadingRemoval => write!(
                f,
                "user reading removal failed (upstream asserts, pinyin.cpp:3759)"
            ),
            Self::SessionSeed(cause) => write!(
                f,
                "the session could not be built after the system libraries were handed \
                 over (upstream dies): {cause}"
            ),
            Self::FailedUserBigramStore => write!(
                f,
                "a training step stores into the failed user bigram container \
                 (upstream asserts, phonetic_lookup.h:904)"
            ),
        }
    }
}

impl std::error::Error for UserStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Store(e) => Some(e),
            Self::Decode
            | Self::InvalidPhrase
            | Self::Persistence(_)
            | Self::UnknownDatabaseFormat
            | Self::ChunkHeaderWrite(_)
            | Self::LibraryChunkHeaderRead
            | Self::SaveRemapFailed
            | Self::SaveTooLarge
            | Self::UnigramTotalOverflow
            | Self::OverlongIndexKey
            | Self::NonTokenUserBigramKey
            | Self::ShortUserBigramValue
            | Self::PartialUserBigramTail
            | Self::ResidualUserBigramGram
            | Self::TrainingSetFreqMiss
            | Self::UserReadingRemoval
            | Self::SessionSeed(_)
            | Self::FailedUserBigramStore => None,
            Self::Journal(fault) => Some(fault),
            Self::LibraryLoop(stop) => Some(stop),
        }
    }
}

impl UserStoreError {
    /// Whether this is the checked-read failure, the one read failure the
    /// pin's `Bigram::load` does not answer with a NULL gram: Kyoto
    /// Cabinet's body read after a good size check asserts
    /// (`ngram_kyotodb.cpp:138` at 074a2219).
    #[must_use]
    pub const fn is_read_after_check(&self) -> bool {
        matches!(self, Self::Store(StoreError::ReadAfterCheck))
    }
}

impl From<StoreError> for UserStoreError {
    fn from(e: StoreError) -> Self {
        match e {
            StoreError::Io(io) => Self::Io(io),
            other @ (StoreError::Backend(_)
            | StoreError::ReadOnly
            | StoreError::InvalidInput(_)
            | StoreError::ReadAfterCheck
            | StoreError::ShortArrayHeader) => Self::Store(other),
        }
    }
}

/// The user pinyin index projection of `syllables` — the DB key
/// `add_index` would write for this reading: tone-zeroed complete keys, or
/// initial-only keys when any syllable is incomplete
/// (`compute_chewing_index` / `compute_incomplete_chewing_index`,
/// `src/storage/pinyin_phrase3.h:160-178` at the pin). Always `Some`: every
/// `SyllableKey` has a `ChewingKey`; the `Option` keeps the callers'
/// signature.
fn index_projection(syllables: &[SyllableKey]) -> Option<Vec<u16>> {
    let incomplete = syllables
        .iter()
        .any(|key| key.completeness() == Completeness::Partial);
    Some(
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
            .collect(),
    )
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
    item: &oxpinyin_data::chunk_write::ChunkItem,
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
    item: &mut oxpinyin_data::chunk_write::ChunkItem,
    packed: &[u16],
    delta: u32,
) -> bool {
    let reading = SystemReading { packed, delta };
    let change = plan_system_reading(item, &reading);
    apply_system_reading(item, &reading, change);
    matches!(change, SystemMerge::Wrap)
}

fn apply_system_reading(
    item: &mut oxpinyin_data::chunk_write::ChunkItem,
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

/// Check the encoder's shape conditions without allocating a serialized
/// copy. Malformed internal items still take the encoder's original error
/// path, including its error text. Wrapping deliberately discards old rows.
fn validate_system_merge(
    item: &oxpinyin_data::chunk_write::ChunkItem,
    reading: &SystemReading<'_>,
    change: SystemMerge,
) -> Result<(), StoreError> {
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
        let mut candidate = item.clone();
        apply_system_reading(&mut candidate, reading, change);
        oxpinyin_data::chunk_write::encode_phrase_item(&candidate)
            .map_err(|error| StoreError::Backend(error.to_string().into()))?;
    }
    Ok(())
}

// ── the user bigram's grams ───────────────────────────────────────

/// The four operations `Bigram` runs on its container (`ngram_bdb.cpp`
/// and its Kyoto Cabinet and tkrzw twins): `get_all_items`, `load`,
/// `store`, `remove`, over the pin's own in-memory user bigram container,
/// keyed by the four native-endian token bytes.
pub(crate) struct ContainerGrams<'a>(pub(crate) &'a oxpinyin_store::DefaultUserBigramDb);

impl ContainerGrams<'_> {
    /// `Bigram::get_all_items`: every key in the container's walk order;
    /// `None` for a key that is not a four-byte `phrase_token_t`.
    fn keys(&self) -> Result<Vec<Option<Token>>, StoreError> {
        Ok(self
            .0
            .keys()?
            .into_iter()
            .map(|key| key.map(Token::from_le_bytes))
            .collect())
    }

    /// `Bigram::load`: the raw gram under `prev`.
    fn get(&self, prev: Token) -> Result<Option<Vec<u8>>, StoreError> {
        container_gram(self.0, prev)
    }

    /// `Bigram::store`.
    fn store(&self, prev: Token, gram: &[u8]) -> Result<(), StoreError> {
        self.0.store(&prev.to_le_bytes(), gram)
    }

    /// `Bigram::remove`.
    fn remove(&self, prev: Token) -> Result<(), StoreError> {
        self.0.remove(&prev.to_le_bytes())
    }
}

/// `Bigram::load` on the pin's container: the gram under `prev`, owned.
fn container_gram(
    container: &oxpinyin_store::DefaultUserBigramDb,
    prev: Token,
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
    keys: Vec<Option<Token>>,
    mask: Token,
    value: Token,
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
    keys: &[Option<Token>],
    mask: Token,
    value: Token,
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
/// store — `None` when the overflow guard skips the store.
fn trained_gram(
    row: Option<Vec<u8>>,
    cur: Token,
) -> Result<(u64, Option<Vec<u8>>), UserStoreError> {
    let mut gram = row.unwrap_or_else(single_gram::new_gram);
    let total = single_gram::get_total_freq(&gram).ok_or(UserStoreError::ShortUserBigramValue)?;
    let (freq, seed) = match single_gram::get_freq(&gram, cur) {
        Some(freq) => (freq, seed::training_seed(Some(u64::from(freq)))),
        None => {
            // A miss walks the same items `insert_freq` walks, so it never
            // meets an equal token (`ngram.cpp:190-200`).
            if !single_gram::insert_freq(&mut gram, cur, 0) {
                return Err(UserStoreError::Decode);
            }
            (0, seed::INITIAL_SEED)
        }
    };
    let seed32 = seed as u32;
    if seed32 > 0 && total > total.wrapping_add(seed32) {
        return Ok((seed, None));
    }
    single_gram::set_total_freq(&mut gram, total.wrapping_add(seed32));
    if !single_gram::set_freq(&mut gram, cur, freq.wrapping_add(seed32)) {
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
fn predicted_gram(row: Option<Vec<u8>>, cur: Token) -> Result<Vec<u8>, UserStoreError> {
    let seed = seed::predicted_seed() as u32;
    let mut gram = row.unwrap_or_else(single_gram::new_gram);
    let total = single_gram::get_total_freq(&gram).ok_or(UserStoreError::ShortUserBigramValue)?;
    let stored = match single_gram::get_freq(&gram, cur) {
        None => single_gram::insert_freq(&mut gram, cur, seed),
        Some(freq) => single_gram::set_freq(&mut gram, cur, freq.wrapping_add(seed)),
    };
    if !stored {
        return Err(UserStoreError::Decode);
    }
    single_gram::set_total_freq(&mut gram, total.wrapping_add(seed));
    Ok(gram)
}

/// The words a phrase writes: the stored form when the phrase carries one,
/// else its outward words.
fn phrase_words(phrase: &oxpinyin_core::PhraseText) -> Vec<u32> {
    phrase
        .stored_words()
        .map_or_else(|| phrase.outward_words().collect(), <[u32]>::to_vec)
}

/// `_add_phrase`'s gate: a non-empty phrase shorter than the maximum with
/// one key per word (074a2219 pinyin.cpp:620-627).
fn phrase_fits_keys(words: &[u32], keys: &[PinyinKey]) -> bool {
    !words.is_empty() && words.len() < crate::MAX_PHRASE_LENGTH && words.len() == keys.len()
}

// ── UserStore ───────────────────────────────────────────────────

/// Decode-time memo of the user bigram's raw grams, valid for one write
/// generation: a gram is loaded from the container the first time its
/// predecessor is scored and served from memory afterwards, and the whole
/// cache is dropped the moment `write_generation` moves. Grams are
/// memoised on demand, so a write costs no rebuild scan and the cache
/// never grows past the predecessors decode asked about between two
/// writes.
#[derive(Debug)]
pub struct CountCache {
    /// The `write_generation` this cache was built for; a mismatch
    /// retires it wholesale.
    pub(crate) generation: u64,
    /// The raw gram under each predecessor scored since the last write
    /// that has one. Counts and totals are read from the bytes as the pin
    /// reads them (`get_freq`, `get_total_freq`).
    pub(crate) bigram_rows: HashMap<Token, Vec<u8>>,
}

/// Per library, how far the total the pin's sub-index holds lies from the
/// one its items make, a wrapping `guint32` keyed by nibble: the
/// difference a stored header or a `.dbin` replay left, until the next
/// `compact` rebuilds the library's total from its items. An entry exists
/// only for a non-zero difference.
pub(crate) type Offsets = BTreeMap<u8, u32>;

/// One session's state, owned by its [`UserStore`].
pub struct StoreInner {
    pub(crate) has_system_items: AtomicBool,
    // When nested, acquire the session phrase-arena mutex before
    // `system_items`; never reverse this order.
    pub(crate) system_items: Mutex<crate::system_items::SystemItems>,
    /// Whether a journal REMOVE took out an item of a loaded system library:
    /// `get_phrase_item` fails for it from then on (074a2219
    /// `phrase_index.cpp:180-206`, `:219-236`), though the shipped chunk still
    /// holds the item and the phrase table the token. The library's live
    /// buffer is the record; this only gates the per-token check.
    pub(crate) has_removed_system_items: AtomicBool,
    /// Decode-time count memo. Declared before `offsets` so it is dropped
    /// first (struct fields drop in declaration order).
    pub(crate) count_cache: Mutex<Option<CountCache>>,
    /// The user libraries' stored-total differences ([`Offsets`]). Its
    /// mutex serializes every write to the session, so a reader that holds
    /// it sees one side of a commit.
    pub(crate) offsets: Mutex<Offsets>,
    pub(crate) dirty: AtomicBool,
    /// Bumped after every committed user-data write, which
    /// retires any `count_cache` built against an older generation.
    ///
    /// Future raw-write paths, including legacy migration import, must call
    /// `UserStore::mark_committed_write` after commit rather than touching
    /// this counter directly. Bumping the generation alone refreshes the
    /// cache but leaves `has_user_data` as it was, and that flag is
    /// consulted *first* — an import that only bumped the generation would
    /// leave the store reporting `UserCountDelta::ZERO` for every candidate,
    /// silently and permanently, however much data it wrote.
    pub(crate) write_generation: AtomicU64,
    /// Exact text-to-token rows changed. Pronunciation-only merges leave
    /// this stamp alone, so repeated text queries can retain their row.
    pub(crate) phrase_table_generation: AtomicU64,
    /// First gate on the decode read path: `false` answers `count_delta` and
    /// `unigram_delta` with zero from this one atomic, taking no mutex.
    /// Recomputed at `open` and maintained only
    /// by `UserStore::mark_committed_write`.
    pub(crate) has_user_data: AtomicBool,
    /// Whether any library still carries its own share of that difference
    /// in [`Self::offsets`]: from the load until the next
    /// `compact` (a save, the end of an import, a mask_out), which
    /// rebuilds each library's total from its items.
    pub(crate) has_library_offsets: AtomicBool,
    /// The libpinyin session metadata (optional directory, system originals).
    /// Transient sessions retain originals without a directory; others use the
    /// pin's own file shapes (`open_libpinyin`): `save()` exports the
    /// session and writes the file set.
    pub(crate) libpinyin: std::sync::Arc<crate::store_libpinyin::Target>,
    /// The pin's own in-memory user bigram container (`Bigram::m_db`
    /// after `load_db`): the session's only copy of the user bigram, the
    /// raw `SingleGram` values every bigram operation reads and writes, so
    /// its walk order — the export's order and the saved file's layout —
    /// is the pin's. A writer takes the offsets guard first, then this one,
    /// and bumps `write_generation` before releasing the offsets guard.
    pub(crate) bigram_db: Mutex<oxpinyin_store::DefaultUserBigramDb>,
    /// Whether [`Self::bigram_db`] is the failed Berkeley DB container
    /// ([`oxpinyin_store::UserBigramDb::is_failed`]), recorded when it is
    /// created: it never changes, so a training step reads it without the
    /// container's lock.
    pub(crate) bigram_failed: bool,
}

/// The libpinyin session store: one per open, over the arena, the pin's
/// user bigram container and the live system items. The user files are
/// read and written with the compiled-in backend (tkrzw, Kyoto Cabinet or
/// Berkeley DB, one per binary; see `oxpinyin-store`).
///
/// The context owns the store and lends it to each operation. System chunk
/// originals belong to this owner; sessions and cursors cannot retain them.
/// The offsets mutex serializes every write to the session, while public
/// mutations require an exclusive borrow of this store.
pub struct UserStore {
    /// The facade fini's `user.conf` write: armed only on the handle a
    /// libpinyin open returned ([`FiniGuard`]).
    _fini: FiniGuard,
    inner: StoreInner,
    pub(crate) originals: crate::persistence::SystemOriginals,
    /// This handle's current training call's step checks.
    training: TrainingCheck,
}

/// Which pin abort or read-past a training step's store check met, with
/// the predecessor whose gram the step loads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrainingFault {
    /// The gram is too short for its total (`memory_chunk.h:390`).
    ShortValue(Token),
    /// The gram has a partial item tail (register row 100).
    PartialTail(Token),
    /// `set_freq` misses the item `insert_freq` added (`phonetic_lookup.h:903`).
    SetFreqMiss(Token),
    /// The step's `store` meets the failed user bigram container
    /// (`phonetic_lookup.h:904`).
    FailedUserBigramStore(Token),
}

/// One training call's replay: the grams its checked steps left, and the
/// fault a failing check met.
#[derive(Debug, Default)]
pub struct TrainingCheck {
    grams: HashMap<Token, Option<Vec<u8>>>,
    fault: Option<TrainingFault>,
}

impl TrainingCheck {
    /// The failure reached by this operation's checked steps.
    #[must_use]
    pub const fn fault(&self) -> Option<TrainingFault> {
        self.fault
    }
}

impl UserStore {
    /// Run the context's fini marker write once, while retaining its live data
    /// for handles that outlive the public context (compatibility row 42).
    pub fn finish(&mut self) {
        self._fini.finish();
    }

    /// Crate-visible handle assembly for the libpinyin constructor
    /// ([`crate::store_libpinyin`]), which runs in memory and arms
    /// the fini guard of the open that raised the counter.
    pub(crate) fn from_parts(
        inner: StoreInner,
        fini: FiniGuard,
        originals: crate::persistence::SystemOriginals,
    ) -> Self {
        Self {
            _fini: fini,
            originals,
            inner,
            training: TrainingCheck::default(),
        }
    }
}

impl fmt::Debug for UserStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UserStore").finish_non_exhaustive()
    }
}

#[cfg(test)]
thread_local! {
    /// Runs once on this thread right after the next mask's container key
    /// walk: a test's window between that walk and the mask.
    pub(crate) static AFTER_CONTAINER_KEYS: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
        const { std::cell::RefCell::new(None) };
}

impl UserStore {
    /// Borrow the uniquely owned init baselines for a read operation.
    pub const fn originals(&self) -> &crate::persistence::SystemOriginals {
        &self.originals
    }

    /// Read system library `nibble`'s saved log once into `logs` and return
    /// the header fault `mask_out`'s reload would reach in it (rows 91 and
    /// 92), found without changing any state; `None` when it reaches none.
    /// [`Self::reload_system_libraries_prefetched`] merges the bytes read here.
    pub fn read_mask_log(
        &self,
        logs: &mut crate::persistence::MaskLogs,
        nibble: u8,
        mask: Token,
        value: Token,
    ) -> Option<crate::JournalFault> {
        let dir = self.inner.libpinyin.dir.as_deref()?;
        crate::persistence::read_mask_log(dir, self.originals.layout(), logs, nibble, mask, value)
    }

    /// Borrow the init baselines exclusively for a library transition.
    pub fn originals_mut(&mut self) -> &mut crate::persistence::SystemOriginals {
        &mut self.originals
    }

    /// Locks the session's offsets, the guard every write takes first.
    /// See the [poisoned locks convention](https://github.com/shenghaoc/oxpinyin/blob/main/docs/rust-conventions.md#poisoned-locks).
    pub(crate) fn offsets(&self) -> MutexGuard<'_, Offsets> {
        lock_result_or_abort(self.inner.offsets.lock())
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

    /// The pin's in-memory user bigram container, locked. Taken after the
    /// offsets guard when both are held, never before it.
    fn container(&self) -> MutexGuard<'_, DefaultUserBigramDb> {
        lock_result_or_abort(self.inner.bigram_db.lock())
    }

    /// Whether the session's user bigram is the failed Berkeley DB container
    /// ([`oxpinyin_store::UserBigramDb::is_failed`]), as recorded when it was
    /// created: no lock is taken, and on a backend whose container cannot
    /// fail this is the constant `false`.
    #[inline]
    fn user_bigram_failed(&self) -> bool {
        <DefaultUserBigramDb as oxpinyin_store::UserBigramDb>::CAN_FAIL && self.inner.bigram_failed
    }

    /// `Bigram::load(prev)`: the raw gram stored under `prev` — one lock
    /// and one point read of the pin's container. `None` when `prev` has
    /// no gram.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the read fails.
    pub fn bigram_row(&self, prev: Token) -> Result<Option<Vec<u8>>, UserStoreError> {
        // Every gram write sets the flag and a mask recomputes it from the
        // grams it leaves, so a store without user data holds no gram.
        if !self.has_user_data() {
            return Ok(None);
        }
        self.gram_in(prev)
    }

    /// [`Self::bigram_row`] without the user-data gate, for a caller that
    /// holds the offsets guard.
    fn gram_in(&self, prev: Token) -> Result<Option<Vec<u8>>, UserStoreError> {
        Ok(container_gram(&self.container(), prev)?)
    }

    /// `Bigram::get_all_items`: every key in the container's walk order,
    /// with `None` for a key that is not a four-byte `phrase_token_t`.
    fn gram_keys(&self) -> Result<Vec<Option<Token>>, UserStoreError> {
        Ok(ContainerGrams(&self.container()).keys()?)
    }

    /// Store `value` raw under `token` in the session's container, as a
    /// value the writer never produces, for the tests.
    #[cfg(test)]
    pub(crate) fn plant_raw_bigram(&self, token: Token, value: &[u8]) {
        self.container()
            .store(&token.to_le_bytes(), value)
            .expect("plant raw value");
        // A stored gram is user data, as after any gram write.
        self.inner.has_user_data.store(true, Ordering::Release);
    }

    /// Whether the gram stored under `token` is shorter than a `guint32`
    /// `total_freq` (`memory_chunk.h:390`). The predicted-candidate walk
    /// loads only the resolved prefixes' own grams (`pinyin.cpp:2322-2330`),
    /// training only its predecessors', while the export walk loads every
    /// predecessor (`pinyin.cpp:889`); callers scope this live check to the
    /// grams their pinned operation reaches.
    #[must_use]
    pub fn has_short_bigram_value_for(&self, token: Token) -> bool {
        self.bigram_row(token)
            .ok()
            .flatten()
            .is_some_and(|value| value.len() < single_gram::TOTAL_LEN)
    }

    /// Whether a `user_bigram.db` key is not a four-byte `phrase_token_t`.
    /// The pin's `mask_out` and `pinyin_begin_get_bigram_phrases` both walk
    /// the whole container through `get_all_items`, so any such key aborts
    /// them (`ngram_bdb.cpp:199`). Read from the live container's keys.
    #[must_use]
    pub fn has_non_token_bigram_key(&self) -> bool {
        self.gram_keys()
            .is_ok_and(|keys| keys.iter().any(Option::is_none))
    }

    /// Whether `token`'s stored user-bigram value has a partial item
    /// tail (register rows 98 and 100). The pin's `retrieve_all`
    /// (`ngram.cpp:133-143`), `SingleGram::mask_out` (`ngram.cpp:86`) and
    /// the training walks (`ngram.cpp:190`, `:241`, `:261`) all walk with
    /// `cur != end` and read past that tail. Read from the live value, so
    /// an in-session store or removal counts at once.
    #[must_use]
    pub fn has_oob_bigram_value_for(&self, token: Token) -> bool {
        self.bigram_row(token)
            .ok()
            .flatten()
            .is_some_and(|bytes| single_gram::has_partial_tail(&bytes))
    }

    /// The session container's `Bigram::get_all_items` walk
    /// (`ngram_tkrzwdb.cpp:161-172` and its Kyoto Cabinet and Berkeley DB
    /// twins at 074a2219). A mask takes it once and hands it to both its
    /// fault check and its walk. The caller holds the offsets guard from
    /// before this walk until the mask has run, so no other handle's
    /// `set_bigram_count` adds a gram the walk missed.
    fn container_keys(&self) -> Result<Vec<Option<Token>>, StoreError> {
        let keys = self.collect_container_keys();
        #[cfg(test)]
        AFTER_CONTAINER_KEYS.with(|hook| {
            if let Some(hook) = hook.borrow_mut().take() {
                hook();
            }
        });
        keys
    }

    fn collect_container_keys(&self) -> Result<Vec<Option<Token>>, StoreError> {
        ContainerGrams(&self.container()).keys()
    }

    /// [`Self::bigram_mask_fault`] over the container keys
    /// [`Self::container_keys`] collected, with the offsets guard held by
    /// the caller.
    fn bigram_mask_fault_over(
        &self,
        keys: &Result<Vec<Option<Token>>, StoreError>,
        mask: Token,
        value: Token,
    ) -> Option<UserStoreError> {
        let Ok(keys) = keys else {
            return None;
        };
        gram_mask_fault_over(&ContainerGrams(&self.container()), keys, mask, value).unwrap_or(None)
    }

    /// Starts one training call's step checks ([`Self::check_training_step`]):
    /// forgets the grams replayed and the fault recorded by the last one.
    pub fn begin_training_steps(&mut self) {
        self.training = TrainingCheck::default();
    }

    /// One training step's store checks, in the pin's order inside the
    /// step (`phonetic_lookup.h:876-909` at 074a2219): a gram too short for
    /// its total (`get_total_freq`, :884; `memory_chunk.h:390`), a partial
    /// item tail the `get_freq` walk reads past (:888, register row 100),
    /// and a `set_freq` that misses the item `insert_freq` just added
    /// (:903). The step runs on the gram the earlier steps of this call
    /// left, replayed on copies: nothing is stored. The fault is recorded
    /// for [`Self::check_training_step`].
    ///
    /// # Errors
    ///
    /// [`UserStoreError::ShortUserBigramValue`] for a short value,
    /// [`UserStoreError::PartialUserBigramTail`] for a partial tail,
    /// [`UserStoreError::TrainingSetFreqMiss`] for the miss,
    /// or the store's read error.
    pub fn check_training_step(&mut self, prev: Token, cur: Token) -> Result<(), UserStoreError> {
        let mut check = std::mem::take(&mut self.training);
        let result = self.check_training_step_with(&mut check, prev, cur);
        self.training = check;
        result
    }

    /// Check a step using operation-owned bookkeeping while phrase storage is borrowed.
    ///
    /// # Errors
    /// The same store and malformed-gram failures as [`Self::check_training_step`].
    pub fn check_training_step_with(
        &self,
        check: &mut TrainingCheck,
        prev: Token,
        cur: Token,
    ) -> Result<(), UserStoreError> {
        // The failed container's `load` misses, so the step trains a fresh
        // gram with no overflow, and its `store` answers false: the pin's
        // `check_result` dies at this first trained step
        // (`phonetic_lookup.h:876-904`).
        if self.user_bigram_failed() {
            check.fault = Some(TrainingFault::FailedUserBigramStore(prev));
            return Err(UserStoreError::FailedUserBigramStore);
        }
        let row = match check.grams.get(&prev) {
            Some(row) => row.clone(),
            None => self.bigram_row(prev)?,
        };
        if let Some(gram) = row.as_deref() {
            if gram.len() < single_gram::TOTAL_LEN {
                check.fault = Some(TrainingFault::ShortValue(prev));
                return Err(UserStoreError::ShortUserBigramValue);
            }
            if single_gram::has_partial_tail(gram) {
                check.fault = Some(TrainingFault::PartialTail(prev));
                return Err(UserStoreError::PartialUserBigramTail);
            }
        }
        match trained_gram(row.clone(), cur) {
            Ok((_, gram)) => {
                check.grams.insert(prev, gram.or(row));
                Ok(())
            }
            Err(UserStoreError::TrainingSetFreqMiss) => {
                check.fault = Some(TrainingFault::SetFreqMiss(prev));
                Err(UserStoreError::TrainingSetFreqMiss)
            }
            Err(error) => Err(error),
        }
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
    pub fn overlong_extension_gate(&self, syllables: &[SyllableKey]) -> bool {
        let Some(projection) = index_projection(syllables) else {
            return false;
        };
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

    fn count_cache(&self) -> MutexGuard<'_, Option<CountCache>> {
        lock_result_or_abort(self.inner.count_cache.lock())
    }

    fn write_generation(&self) -> u64 {
        self.inner.write_generation.load(Ordering::Acquire)
    }

    fn has_user_data(&self) -> bool {
        self.inner.has_user_data.load(Ordering::Acquire)
    }

    /// The gate in front of the count reads, as the session holds it.
    #[cfg(test)]
    pub(crate) fn gate_for_test(&self) -> bool {
        self.has_user_data()
    }

    /// Sets the gate and retires the count cache, as a committed write does.
    #[cfg(test)]
    pub(crate) fn force_gate_for_test(&self, open: bool) {
        self.inner.has_user_data.store(open, Ordering::Release);
        self.inner.write_generation.fetch_add(1, Ordering::AcqRel);
        *self.count_cache() = None;
    }

    /// Runs `read` against the count cache for the current write
    /// generation, discarding a cache built against an older one first.
    ///
    /// The offsets guard is taken for the whole call, in the lock order
    /// every write path uses — cache first, then offsets, then the
    /// container.
    /// `write_generation` is read under that guard, so cache validation
    /// is ordered against the same critical section every commit bumps
    /// in: the observed state and generation always belong to one side of
    /// a commit, never straddle it. Holding the guard across the call is
    /// also what makes a multi-value read atomic without MVCC: every write
    /// path takes the same guard, so no commit can land between the values
    /// `count_delta` reads.
    fn with_count_cache<T>(
        &self,
        read: impl FnOnce(&mut CountCache) -> Result<T, UserStoreError>,
    ) -> Result<T, UserStoreError> {
        let mut cache = self.count_cache();
        let offsets = self.offsets();
        let generation = self.write_generation();
        let mut current = match cache.take() {
            Some(cached) if cached.generation == generation => cached,
            _ => CountCache::new(generation),
        };
        let out = read(&mut current);
        drop(offsets);
        *cache = Some(current);
        out
    }

    /// Invalidate cached reads after a committed user-data write.
    ///
    /// The generation bump lands while the offsets guard is still held:
    /// paired with [`Self::with_count_cache`] reading the generation under
    /// the same guard, no reader can observe post-commit rows against a
    /// pre-commit generation (or vice versa).
    fn mark_committed_write(&self, offsets: MutexGuard<'_, Offsets>, has_user_data: bool) {
        self.inner
            .has_user_data
            .store(has_user_data, Ordering::Release);
        self.inner.write_generation.fetch_add(1, Ordering::AcqRel);
        drop(offsets);
        *self.count_cache() = None;
    }

    /// A phrase-table edit invalidates exact-text rows. Count-only writes
    /// and pronunciation merges, which keep the phrase table's
    /// text-to-token rows (074a2219 pinyin.cpp:563-582), take
    /// [`Self::mark_committed_write`].
    fn mark_committed_phrase_write(&self, offsets: MutexGuard<'_, Offsets>, has_user_data: bool) {
        self.inner
            .phrase_table_generation
            .fetch_add(1, Ordering::AcqRel);
        self.mark_committed_write(offsets, has_user_data);
    }

    // Keep the import memo's publication separate from whether this write
    // changed the exact-text row; reading-only merges retain queried rows.
    fn mark_committed_import_write(&self, offsets: MutexGuard<'_, Offsets>, added_token: bool) {
        if added_token {
            self.mark_committed_phrase_write(offsets, true);
        } else {
            self.mark_committed_write(offsets, true);
        }
    }

    /// A search of the user pinyin table for the query `keys` makes
    /// (`ChewingLargeTable2::search`): a hit on a non-empty record loads it
    /// into the length's table entry, which the next `mask_out` walk starts
    /// from (#785). Read that record under the canonical table's guard.
    pub fn note_pinyin_search(&self, keys: impl FnOnce() -> Option<Vec<ChewingKey>>) {
        if !crate::user_index::ENTRY_PER_LENGTH {
            return;
        }
        if let Some(keys) = keys() {
            self.inner.libpinyin.phrases().index.note_search(&keys);
        }
    }

    /// A `search_suggestion` of the user pinyin table for the prefix `keys`
    /// makes (#785): every longer key it visits empties its length's entry.
    pub fn note_pinyin_suggestion(&self, keys: impl FnOnce() -> Option<Vec<ChewingKey>>) {
        if !crate::user_index::ENTRY_PER_LENGTH {
            return;
        }
        if let Some(keys) = keys() {
            self.inner.libpinyin.phrases().index.note_suggestion(&keys);
        }
    }

    /// `SingleGram::get_freq(cur)` on `prev`'s raw gram — the first match
    /// walking from `lower_bound` (`ngram.cpp:231-250`); `0` if absent.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the store cannot be read.
    pub fn bigram_count(&self, prev: Token, cur: Token) -> Result<u64, UserStoreError> {
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
    pub fn bigram_total(&self, prev: Token) -> Result<u64, UserStoreError> {
        Ok(self
            .bigram_row(prev)?
            .and_then(|gram| single_gram::get_total_freq(&gram))
            .map_or(0, u64::from))
    }

    /// Overwrite the raw `(prev -> cur)` user-bigram count, a test hook
    /// with no pin counterpart: the first match walking from `lower_bound`
    /// is set, or the item inserted; the total moves by the difference,
    /// `min(total − old + new, u32::MAX)`; the count saturates at
    /// `u32::MAX`; a value too short for a total is replaced by a fresh
    /// gram.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the gram cannot be written.
    pub fn set_bigram_count(
        &mut self,
        prev: Token,
        cur: Token,
        count: u64,
    ) -> Result<(), UserStoreError> {
        let offsets = self.offsets();
        let mut gram = self
            .gram_in(prev)?
            .filter(|gram| gram.len() >= single_gram::TOTAL_LEN)
            .unwrap_or_else(single_gram::new_gram);
        let count32 = u32::try_from(count).unwrap_or(u32::MAX);
        let total = single_gram::get_total_freq(&gram).unwrap_or(0);
        let old = single_gram::get_freq(&gram, cur);
        let stored = match old {
            Some(_) => single_gram::set_freq(&mut gram, cur, count32),
            None => single_gram::insert_freq(&mut gram, cur, count32),
        };
        if !stored {
            return Err(UserStoreError::Decode);
        }
        let new_total = u64::from(total)
            .saturating_sub(u64::from(old.unwrap_or(0)))
            .saturating_add(u64::from(count32))
            .min(u64::from(u32::MAX));
        single_gram::set_total_freq(&mut gram, new_total as u32);
        self.store_gram(prev, &gram)?;
        self.mark_committed_write(offsets, true);
        Ok(())
    }

    /// `Bigram::store(prev, gram)` on the pin's container, with the
    /// offsets guard held by the caller.
    fn store_gram(&self, prev: Token, gram: &[u8]) -> Result<(), UserStoreError> {
        self.container().store(&prev.to_le_bytes(), gram)?;
        Ok(())
    }

    /// Accumulated phrase-index unigram delta for `token`; `0` if none: a
    /// user library item's count, and `0` for a system item, whose count is
    /// its live field ([`Self::system_unigram`]), which the model prices;
    /// the session keeps no shipped count to take a difference from.
    ///
    /// # Errors
    ///
    /// None: the `Result` is the callers' signature.
    pub fn unigram_delta(&self, token: Token) -> Result<u64, UserStoreError> {
        Ok(self
            .with_user_item(token, |item| u64::from(item.unigram()))
            .unwrap_or(0))
    }

    /// A user item's unigram frequency, or `None` when the item is absent. A
    /// present zero count stays `Some(0)`. The existence check and the count
    /// read happen under one arena guard, as the pin reads one item
    /// (074a2219 `pinyin.cpp:2821-2833`, `zhuyin.cpp:1813-1825`). The caller
    /// applies library visibility.
    ///
    /// # Errors
    /// None: the `Result` is the callers' signature.
    pub fn user_unigram_frequency(&self, token: Token) -> Result<Option<u64>, UserStoreError> {
        Ok(self.with_user_item(token, |item| u64::from(item.unigram())))
    }

    /// The live `guint32` unigram of the system item behind `token`
    /// (`PhraseItem::get_unigram_frequency`), where the session holds the
    /// library's state: an arena item's, a decoded item's, the library's live
    /// buffer's, or the original's before any write. `None` for a removed item,
    /// a library with no original or an item nothing holds.
    #[must_use]
    pub fn system_unigram(&self, token: Token) -> Option<u32> {
        let original = self.originals.get(&phrase_index_library_index(token))?;
        if self.system_item_removed(token) {
            return None;
        }
        if let Some(count) = self.with_user_item(token, |item| item.unigram()) {
            return Some(count);
        }
        lock_result_or_abort(self.inner.system_items.lock()).unigram(token, Some(original))
    }

    /// Whether the session layout names a library as a system chunk.
    #[must_use]
    pub fn is_system_library(&self, library: u8) -> bool {
        self.originals.get(&library).is_some()
            || self
                .originals
                .layout()
                .system_logs()
                .iter()
                .any(|(index, _)| *index == library)
    }

    /// `SubPhraseIndex::m_total_freq` of system library `library`, where the
    /// session holds its state; `None` for a library with no original.
    #[must_use]
    pub fn system_library_total(&self, library: u8) -> Option<u32> {
        let original = self.originals.get(&library)?;
        lock_result_or_abort(self.inner.system_items.lock()).library_total(library, Some(original))
    }

    /// Apply the exported unigram increment to a loaded system library, as
    /// `FacadePhraseIndex::add_unigram_frequency` does (074a2219
    /// `phrase_index.h:628-635`, `phrase_index.cpp:150-176`): the facade total
    /// takes `delta` first, whatever the sub-index then answers, and the
    /// sub-index takes it only when it holds the item and its `guint32` total
    /// can. The caller checks library visibility. This does not mark the
    /// context dirty (`pinyin.cpp:2836-2843`, `zhuyin.cpp:1828-1835`).
    ///
    /// # Errors
    /// None: the `Result` is the callers' signature.
    pub fn add_system_unigram_frequency(
        &mut self,
        token: Token,
        delta: u32,
    ) -> Result<bool, UserStoreError> {
        Ok(self.add_unigram_outcome(token, delta)?.is_accepted())
    }

    /// Apply a unigram increment to the default phrase index. A loaded
    /// sub-index receives the update in its live item, and an absent
    /// sub-index leaves the facade total alone (074a2219
    /// `phrase_index.h:628-635`). Only [`UnigramAdd::Refused`] is
    /// `ERROR_INTEGER_OVERFLOW` (`pinyin.cpp:2528`).
    ///
    /// # Errors
    /// None: the `Result` is the callers' signature.
    pub fn add_unigram_outcome(
        &mut self,
        token: Token,
        delta: u32,
    ) -> Result<UnigramAdd, UserStoreError> {
        let library = phrase_index_library_index(token);
        if self.originals.get(&library).is_none() && !self.has_user_library(library) {
            return Ok(UnigramAdd::Missing);
        }
        Ok(self.arena_add_unigram_outcome(token, delta))
    }

    /// What the user side adds to the facade's unigram total: everything of
    /// `FacadePhraseIndex::m_total_freq` beyond the system libraries'
    /// sub-index totals (the user libraries' totals and every delta a
    /// sub-index did not take). `0` for an empty store.
    ///
    /// # Errors
    ///
    /// None: the `Result` is the callers' signature.
    pub fn unigram_total(&self) -> Result<u64, UserStoreError> {
        Ok(lock_result_or_abort(self.inner.system_items.lock()).facade_extra())
    }

    /// What a session's facade total holds beyond the stored totals of the
    /// visible system libraries: [`Self::unigram_total`] plus how far each
    /// visible library's live sub-index total lies from its original's, as a
    /// wrapping `guint32` the model adds to those stored totals.
    #[must_use]
    pub fn unigram_total_over(&self, visible: impl Fn(u8) -> bool) -> u64 {
        let items = lock_result_or_abort(self.inner.system_items.lock());
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

    /// The `guint32` total of a user library: its items' sum plus the stored
    /// total's difference from it, until the next compaction.
    ///
    /// # Errors
    ///
    /// None: the `Result` is the callers' signature.
    pub fn unigram_library_total(&self, library: u8) -> Result<u32, UserStoreError> {
        let offset = self.offsets().get(&library).copied().unwrap_or(0);
        Ok(self
            .inner
            .libpinyin
            .phrases()
            .libraries
            .get(&library)
            .map_or(offset, crate::user_library::UserLibrary::total))
    }

    /// How far `library`'s stored total lies from the sum of its items
    /// (a wrapping `guint32`), until the next [`Self::compact_libraries`];
    /// `0` once compacted or when the two agree.
    ///
    /// # Errors
    /// None: the `Result` is the callers' signature.
    pub fn library_offset(&self, library: u8) -> Result<u32, UserStoreError> {
        if !self.inner.has_library_offsets.load(Ordering::SeqCst) {
            return Ok(0);
        }
        Ok(self.offsets().get(&library).copied().unwrap_or(0))
    }

    /// `FacadePhraseIndex::compact` (074a2219 `phrase_index.cpp:648-675`):
    /// every library's total is rebuilt from its items, so none carries a
    /// stored total's difference any more. The facade's total keeps it.
    /// The pin compacts in `pinyin_save` once it has something to write,
    /// at the end of an import and at the end of a mask_out
    /// (`pinyin.cpp:1139`, `:657`, `:1294`; `zhuyin.cpp:554`, `:542`,
    /// `:829`).
    pub fn compact_libraries(&mut self) {
        // Flag invariant: `has_library_offsets` is never false while an
        // offset entry exists (true with no entries is harmless: the read
        // gives 0). `library_offset` reads the flag without the lock, so the
        // flag is cleared only after the entries are gone, under the offsets
        // lock that every offset writer also holds.
        let target = Arc::clone(&self.inner.libpinyin);
        // Each system library's total becomes the sum of its live readable
        // items; the facade total keeps what the libraries held, so the
        // difference moves to the facade's extra.
        {
            let mut items = lock_result_or_abort(self.inner.system_items.lock());
            for (&library, original) in self.originals.iter() {
                // An unloaded library has no sub-index to compact: the pin's
                // loop skips it (`phrase_index.cpp:651-652`) and its total
                // stays as the unload left it.
                if items.lacks_sub_index(library, original) {
                    continue;
                }
                let old = items
                    .library_total(library, Some(original))
                    .unwrap_or(original.total);
                let new = items.compacted_total(library, original);
                items.set_library_total(library, new);
                items.add_facade_extra(u64::from(old.wrapping_sub(new)));
            }
        }
        let mut offsets = self.offsets();
        offsets.clear();
        for library in target.phrases().libraries.values_mut() {
            library.compact();
        }
        self.inner
            .has_library_offsets
            .store(false, Ordering::SeqCst);
    }

    /// The §5 overlay for scoring `token` after `prev`, read under one guard.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the store cannot be read.
    pub fn count_delta(
        &self,
        prev: Option<Token>,
        token: Token,
    ) -> Result<UserCountDelta, UserStoreError> {
        // The pin merges the user gram whenever one is stored, item-less
        // ones included (`merge_single_gram`, `ngram.cpp:298-306`).
        let (bigram_count, bigram_total) = match prev {
            Some(prev) if self.has_user_data() => self.with_count_cache(|cached| {
                Ok(cached
                    .bigram_row(prev, || self.gram_in(prev))?
                    .map_or((0, 0), |gram| {
                        (
                            single_gram::get_freq(gram, token).map_or(0, u64::from),
                            single_gram::get_total_freq(gram).map_or(0, u64::from),
                        )
                    }))
            })?,
            _ => (0, 0),
        };
        // The session holds no unigram rows: a user item's count is its
        // arena field, a system item's frequency its live field, and the
        // facade total's user side one scalar.
        Ok(UserCountDelta {
            bigram_count,
            bigram_total,
            unigram_delta: self.unigram_delta(token)?,
            unigram_total_delta: self.unigram_total()?,
            unigram_live: self.system_unigram(token),
        })
    }

    /// Record a training selection of `cur` after `last` (the `pinyin_train`
    /// path, §2). Returns the seed applied.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the observation cannot be committed,
    /// [`UserStoreError::ShortUserBigramValue`] when `last`'s stored
    /// value is too short for its total (register row 85, the pin's abort),
    /// or [`UserStoreError::PartialUserBigramTail`] when it has a partial
    /// item tail (register row 100); nothing changes then.
    pub fn observe_selection(&mut self, last: Token, cur: Token) -> Result<u64, UserStoreError> {
        let seed = self.update(last, cur, SeedPolicy::Training)?;
        self.inner.dirty.store(true, Ordering::Relaxed);
        Ok(seed)
    }

    /// Pronunciation half of train_result3 (074a2219 phonetic_lookup.h:925-927).
    pub(crate) fn train_pronunciations(
        &mut self,
        token: Token,
        readings: impl Iterator<Item = Vec<oxpinyin_core::ChewingKey>>,
        seed: u64,
    ) -> Result<(), UserStoreError> {
        self.arena_train(token, readings, seed as u32)
    }

    /// Record an accepted *predicted* candidate `cur` after `last` (the
    /// `pinyin_choose_predicted_candidate` path, §2). Returns the seed
    /// applied.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the observation cannot be committed.
    pub fn observe_predicted(&mut self, last: Token, cur: Token) -> Result<u64, UserStoreError> {
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
    pub fn observe_predicted_prefix(&mut self, cur: Token) -> Result<u64, UserStoreError> {
        let seed = seed::predicted_seed();
        let delta = seed::unigram_delta(seed) as u32;
        let offsets = self.offsets();
        let accepted = self.arena_add_unigram(cur, delta);
        self.mark_committed_write(offsets, true);
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
        last: Token,
        cur: Token,
        policy: SeedPolicy,
    ) -> Result<u64, UserStoreError> {
        let offsets = self.offsets();
        let row = self.gram_in(last)?;
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
                self.mark_committed_write(offsets, true);
                if !accepted {
                    return Err(UserStoreError::UnigramTotalOverflow);
                }
            }
            return Err(UserStoreError::ShortUserBigramValue);
        }
        let (seed, gram) = match policy {
            SeedPolicy::Training => trained_gram(row, cur)?,
            SeedPolicy::Predicted => (seed::predicted_seed(), Some(predicted_gram(row, cur)?)),
        };
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
        let stored = if accepted && let Some(gram) = &gram {
            self.store_gram(last, gram)
        } else {
            Ok(())
        };
        self.mark_committed_write(offsets, true);
        match stored {
            Ok(()) => {}
            // The failed container's `store` answers false.
            // `pinyin_choose_predicted_candidate` ignores that answer
            // (`pinyin.cpp:2638`); training's `check_result` dies on it
            // (`phonetic_lookup.h:904`), which its step check reports first.
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

    /// Add a user phrase under [`crate::USER_DICTIONARY`] (`_add_phrase`, §3.2).
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the phrase cannot be added.
    pub fn add_phrase(
        &mut self,
        phrase: &oxpinyin_core::PhraseText,
        keys: &[PinyinKey],
        count: Option<u64>,
    ) -> Result<Token, UserStoreError> {
        self.add_phrase_in(USER_DICTIONARY, phrase, keys, count)
    }

    /// Whether `table.conf` gives sub-index `library` a `USER_FILE` row
    /// with a file: a `NOT_USED` row is not a library the pin loaded, and
    /// nothing can be added to it. A store without a system `table.conf`
    /// behind it keeps the stock layout.
    #[must_use]
    pub fn has_user_library(&self, library: u8) -> bool {
        self.originals.layout().has_user_library(library)
    }

    /// Borrows the session's canonical tables.
    #[must_use]
    pub fn lookup(&self) -> crate::UserLookup<'_> {
        crate::UserLookup::session(&self.inner.libpinyin.phrases, &self.inner.system_items)
    }

    /// Add a phrase under `library` (`USER_DICTIONARY` or `NETWORK_DICTIONARY`).
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the phrase cannot be added.
    pub fn add_phrase_in(
        &mut self,
        library: u8,
        phrase: &oxpinyin_core::PhraseText,
        keys: &[PinyinKey],
        count: Option<u64>,
    ) -> Result<Token, UserStoreError> {
        let words = phrase_words(phrase);
        if !is_user_file_library(library)
            || !self.has_user_library(library)
            || !phrase_fits_keys(&words, keys)
        {
            return Err(UserStoreError::InvalidPhrase);
        }
        // `_add_phrase`'s `count` reaches the item as a `guint32` delta
        // (`add_pronunciation(keys, count)`, `pinyin.cpp:579`, `:603`): the
        // C ABI hands its `gint`'s bit pattern over; a wider caller value
        // saturates.
        let delta = u32::try_from(count.unwrap_or(DEFAULT_PHRASE_COUNT)).unwrap_or(u32::MAX);
        self.arena_add_phrase(library, &words, keys, delta, None, 1)
    }

    /// Promote a chosen addon phrase into the default-facade
    /// [`ADDON_DICTIONARY`] (nibble 5) sub-index.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the promotion cannot be written.
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

        self.arena_promote(phrase, &valid, unigram as u32)
    }

    /// Read the stored count without decoding text or keys.
    #[must_use]
    pub fn user_pronunciation_count(&self, token: Token) -> Option<u32> {
        self.inner
            .libpinyin
            .phrases()
            .libraries
            .get(&phrase_index_library_index(token))
            .and_then(|library| library.pronunciation_count(token & PHRASE_MASK))
            .or_else(|| self.with_appended_item(token, |item| item.n_pronunciations() as u32))
    }

    /// The item's text, without decoding or collecting its readings.
    ///
    /// # Errors
    /// None: the `Result` is the callers' signature.
    pub fn phrase_text(&self, token: Token) -> Result<Option<Vec<u32>>, UserStoreError> {
        Ok(self.with_user_item(token, |item| {
            item.as_bytes()[6..6 + 4 * item.phrase_length()]
                .as_chunks::<4>()
                .0
                .iter()
                .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                .collect()
        }))
    }

    /// Whether this store owns the phrase, without materializing its fields.
    ///
    /// # Errors
    /// None: the `Result` is the callers' signature.
    pub fn phrase_exists(&self, token: Token) -> Result<bool, UserStoreError> {
        Ok(self.with_user_item(token, |_| ()).is_some())
    }

    /// The stored word count without copying or decoding the phrase text.
    ///
    /// # Errors
    /// None: the `Result` is the callers' signature.
    pub fn phrase_length(&self, token: Token) -> Result<Option<usize>, UserStoreError> {
        Ok(self.with_user_item(token, |item| item.phrase_length()))
    }

    /// One reading's stored key words, as `get_nth_pronunciation` copies them
    /// (074a2219 phrase_index.cpp:33-50): a session item hands out its packed
    /// words verbatim, whether or not they name a syllable. Missing readings
    /// answer `None`.
    ///
    /// # Errors
    /// None: the `Result` is the callers' signature.
    pub fn nth_pronunciation_words(
        &self,
        token: Token,
        nth: usize,
    ) -> Result<Option<Vec<u16>>, UserStoreError> {
        Ok(self
            .with_user_item(token, |item| {
                let reading = item.pronunciation(nth)?;
                Some(
                    reading
                        .keys
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
                        .collect(),
                )
            })
            .flatten())
    }

    /// One reading's stored key bytes, borrowed from a session-arena item.
    ///
    /// `read` runs while the item view is live and receives the item's stored
    /// length and the reading's packed little-endian words, or `None` when
    /// the item has no such reading. `None` is returned when no session item
    /// backs `token`; callers then use [`Self::nth_pronunciation_words`],
    /// which copies. This optional borrowed path keeps the
    /// token getters' copy into the caller's array single for an
    /// arena-resident reading, as the pin's `get_nth_pronunciation` copies
    /// once (`phrase_index.cpp:33-50`).
    pub fn with_nth_pronunciation_words<R>(
        &self,
        token: Token,
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
    ///
    /// # Errors
    ///
    /// None: the `Result` is the callers' signature.
    pub fn phrase(&self, token: Token) -> Result<Option<UserPhrase>, UserStoreError> {
        let phrases = self.inner.libpinyin.phrases();
        Ok(match phrases.item(token) {
            Some(item) => Some(phrases.phrase(token, item)),
            None => self.with_appended_item(token, |item| phrases.phrase(token, item)),
        })
    }

    /// Token already allocated for `phrase` in the user sub-index, if any.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the store cannot be read.
    pub fn token_for_phrase(&self, phrase: &str) -> Result<Option<Token>, UserStoreError> {
        self.token_for_phrase_in(USER_DICTIONARY, phrase)
    }

    /// Token already allocated for `phrase` in `library`, if any.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the store cannot be read.
    pub fn token_for_phrase_in(
        &self,
        library: u8,
        phrase: &str,
    ) -> Result<Option<Token>, UserStoreError> {
        Ok(self
            .phrase_table_tokens_for_text(phrase)?
            .into_iter()
            .find(|&token| phrase_index_library_index(token) == library))
    }

    /// The pin's user phrase-table search, preserving stored token multiplicity.
    ///
    /// # Errors
    /// Returns an error when the record cannot be read.
    pub fn phrase_table_tokens_for_text(&self, text: &str) -> Result<Vec<Token>, UserStoreError> {
        self.inner
            .libpinyin
            .phrases()
            .phrase_table
            .tokens(&phrase_table::key(text))
            .map_err(|error| UserStoreError::Persistence(error.to_string()))
    }

    /// The pin's user phrase-table search over raw UCS-4 words, preserving
    /// stored token multiplicity. No `char` or `str` is built from a
    /// non-scalar word.
    ///
    /// # Errors
    /// Returns an error when the record cannot be read.
    pub fn phrase_table_tokens_for_ucs4(
        &self,
        words: &[u32],
    ) -> Result<Vec<Token>, UserStoreError> {
        self.inner
            .libpinyin
            .phrases()
            .phrase_table
            .tokens(&phrase_table::key_ucs4(words))
            .map_err(|error| UserStoreError::Persistence(error.to_string()))
    }

    /// Whether the user phrase table holds this exact key, including an
    /// empty prefix marker (`SEARCH_CONTINUED` in the pin).
    ///
    /// # Errors
    /// Returns a backend or malformed-record error.
    pub fn phrase_table_contains_ucs4(&self, words: &[u32]) -> Result<bool, UserStoreError> {
        Ok(self
            .inner
            .libpinyin
            .phrases()
            .phrase_table
            .contains_key(&phrase_table::key_ucs4(words)))
    }

    /// Current write generation: moves on every committed write, counts
    /// included.
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.write_generation()
    }

    /// Current exact-text row generation. New tokens, removals, masks and
    /// library replacements move it. Pronunciation-only writes do not.
    #[must_use]
    pub fn phrase_table_generation(&self) -> u64 {
        self.inner.phrase_table_generation.load(Ordering::Acquire)
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

    /// Export rows for one `USER_FILE` nibble.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the store cannot be read.
    pub fn export_phrases_in(&self, library: u8) -> Result<Vec<ExportedPhrase>, UserStoreError> {
        let phrases = self.inner.libpinyin.phrases();
        let mut out = Vec::new();
        if let Some(items) = phrases.libraries.get(&library) {
            for (_, item) in items.items() {
                for reading in item.pronunciations() {
                    let pinyin = reading.keys.as_chunks::<2>().0.iter().enumerate().try_fold(
                        String::new(),
                        |mut text, (position, bytes)| {
                            let key =
                                ChewingKey::from_packed(u16::from_le_bytes([bytes[0], bytes[1]]));
                            let syllable = oxpinyin_core::SyllableKey::from_chewing_key(key)?;
                            crate::toned_key(syllable.index(), key.tone)?;
                            if position != 0 {
                                text.push('\'');
                            }
                            text.push_str(syllable.text());
                            if key.tone != 0 {
                                text.push(char::from(b'0' + key.tone));
                            }
                            Some(text)
                        },
                    );
                    if let Some(pinyin) = pinyin {
                        let text = item.as_bytes()[6..6 + 4 * item.phrase_length()]
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                            .collect();
                        out.push(ExportedPhrase {
                            text,
                            pinyin,
                            count: u64::from(reading.freq),
                        });
                    }
                }
            }
        }
        Ok(out)
    }

    /// Every stored phrase (user and network) with pronunciations.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the store cannot be read.
    pub fn phrases(&self) -> Result<Vec<UserPhrase>, UserStoreError> {
        let phrases = self.inner.libpinyin.phrases();
        Ok(phrases
            .libraries
            .iter()
            .flat_map(|(&nibble, library)| {
                let phrases = &*phrases;
                library
                    .items()
                    .map(move |(slot, item)| phrases.phrase((u32::from(nibble) << 24) | slot, item))
            })
            .collect())
    }

    /// `SingleGram::retrieve_all` on `prev`'s raw gram: its whole items as
    /// `(token, count)` pairs in stored order, duplicates included
    /// (`ngram.cpp:133-151`). Empty when `prev` has no gram.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the store cannot be read.
    pub fn bigram_successors(&self, prev: Token) -> Result<Vec<(Token, u64)>, UserStoreError> {
        Ok(self.bigram_row(prev)?.map_or_else(Vec::new, |gram| {
            single_gram::retrieve_all(&gram)
                .map(|(token, count)| (token, u64::from(count)))
                .collect()
        }))
    }

    /// Whether this session has a user-directory argument, including "".
    /// A transient NULL-user session has mutable state but no directory.
    #[must_use]
    pub fn has_user_directory(&self) -> bool {
        self.inner.libpinyin.dir.is_some()
    }

    /// The save the C facades use: the file set is written and renamed and
    /// the marker rewritten whatever fails on the way, and the failures come
    /// back in the [`SaveReport`] instead of ending the save. A store with no
    /// libpinyin target (no directory to write) reports nothing. A completed
    /// save leaves the store clean; a guard return leaves its state
    /// unchanged.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the session cannot be exported or
    /// encoded — failures the pin has no counterpart for. A filesystem
    /// failure is never one.
    pub fn save_reporting(&mut self) -> Result<SaveReport, UserStoreError> {
        self.save_reporting_compacted(&[])
    }

    /// [`Self::save_reporting`] with the nibbles whose live empty sub-index
    /// the pin's `compact` rebuilt from no items: their saved log records a
    /// new header total of zero even though the system chunk's old total is
    /// the one the failed load read (`phrase_index.cpp:648-675`).
    pub fn save_reporting_compacted(
        &mut self,
        compacted: &[u8],
    ) -> Result<SaveReport, UserStoreError> {
        self.save_reporting_inner(
            compacted,
            None::<fn(u8, &crate::SystemOriginals) -> crate::persistence::SaveBaseline>,
        )
    }

    /// [`Self::save_reporting_compacted`] with the current-on-disk baseline
    /// hook. The store calls it once per selected system library, in the
    /// pin's `_write_files` order, immediately before that library's log is
    /// staged; the hook maps the current chunk and emits any layout
    /// critical at that point (`pinyin.cpp:956`, `zhuyin.cpp:589`).
    pub fn save_reporting_with_baseline(
        &mut self,
        compacted: &[u8],
        baseline: impl FnMut(u8, &crate::SystemOriginals) -> crate::persistence::SaveBaseline,
    ) -> Result<SaveReport, UserStoreError> {
        self.save_reporting_inner(compacted, Some(baseline))
    }

    fn save_reporting_inner<
        B: FnMut(u8, &crate::SystemOriginals) -> crate::persistence::SaveBaseline,
    >(
        &mut self,
        compacted: &[u8],
        mut baseline: Option<B>,
    ) -> Result<SaveReport, UserStoreError> {
        let mut report = SaveReport::default();
        if !self.is_modified() {
            return Ok(report);
        }
        let target = Arc::clone(&self.inner.libpinyin);
        let Some(dir) = target.dir.as_deref() else {
            return Ok(report);
        };
        self.compact_libraries();
        let originals = &self.originals;
        let state = crate::store_libpinyin::export_for_save(self)?;
        let bigram_db = self.container();
        let phrases = target.phrases();
        let live = lock_result_or_abort(self.inner.system_items.lock());
        {
            let mut reporting = crate::persistence::SaveReporting {
                versions: &target.versions,
                open_counter: target.open_counter,
                bigram_db: Some(&*bigram_db),
                interleaved: target.law == crate::persistence::UserConfLaw::Zhuyin,
                baseline: baseline
                    .as_mut()
                    .map(|baseline| |nibble| baseline(nibble, originals)),
            };
            report = crate::persistence::save_reporting(
                dir,
                crate::persistence::SaveSource {
                    state: &state,
                    libraries: Some(&phrases),
                    index: &phrases.index,
                    phrase_table: Some(&phrases.phrase_table),
                    compacted,
                    live: Some(&live),
                },
                originals,
                &mut reporting,
            )?;
        }
        drop(bigram_db);
        *self.count_cache() = None;
        self.inner.dirty.store(false, Ordering::Relaxed);
        Ok(report)
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
    pub fn bigram_predecessors(&self) -> Result<Vec<Token>, UserStoreError> {
        Ok(self.gram_keys()?.into_iter().flatten().collect())
    }

    /// `Bigram::mask_out(mask, value)` on the pin's container over the keys
    /// [`Self::container_keys`] collected before the mask's fault check,
    /// with the offsets guard held ([`mask_grams_over`]). Answers whether a
    /// gram remains.
    fn mask_container_over(
        &self,
        keys: Result<Vec<Option<Token>>, StoreError>,
        mask: Token,
        value: Token,
    ) -> Result<bool, UserStoreError> {
        Ok(mask_grams_over(
            &ContainerGrams(&self.container()),
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
    /// The [`JournalFault`](crate::JournalFault) of the first library whose log reaches a pin
    /// assertion, with nothing replaced.
    pub fn reload_system_libraries(
        &mut self,
        mut originals: crate::persistence::SystemOriginals,
        mask: Token,
        value: Token,
    ) -> Result<(), crate::JournalFault> {
        self.reload_system_libraries_retaining(&mut originals, mask, value)
    }

    /// Replay a remap without losing its mappings when the journal refuses it.
    ///
    /// # Errors
    /// The same journal fault as [`Self::reload_system_libraries`].
    pub fn reload_system_libraries_retaining(
        &mut self,
        originals: &mut crate::persistence::SystemOriginals,
        mask: Token,
        value: Token,
    ) -> Result<(), crate::JournalFault> {
        self.reload_system_libraries_prefetched(
            originals,
            mask,
            value,
            crate::persistence::MaskLogs::default(),
        )
    }

    /// [`Self::reload_system_libraries_retaining`] merging the logs `logs`
    /// already holds ([`Self::read_mask_log`]) instead of reading them again;
    /// a library not in `logs` is read when the replay reaches it.
    ///
    /// # Errors
    /// The same journal fault as [`Self::reload_system_libraries`].
    pub fn reload_system_libraries_prefetched(
        &mut self,
        originals: &mut crate::persistence::SystemOriginals,
        mask: Token,
        value: Token,
        mut logs: crate::persistence::MaskLogs,
    ) -> Result<(), crate::JournalFault> {
        let target = &self.inner.libpinyin;
        let (replayed, library_totals) = match target.dir.clone() {
            Some(dir) => {
                crate::persistence::load_masked_logs_live(&dir, originals, mask, value, &mut logs)?
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
    /// The [`JournalFault`](crate::JournalFault) where the merge reaches a pin assertion (rows 91
    /// and 108), with nothing replaced.
    pub fn load_system_libraries(
        &mut self,
        mut originals: crate::persistence::SystemOriginals,
    ) -> Result<(), crate::JournalFault> {
        self.load_system_libraries_retaining(&mut originals)
    }

    /// Load a remap, retaining it for dictionary publication if replay reaches
    /// a journal assertion. A transient session has no log to merge.
    ///
    /// # Errors
    /// The same journal fault as [`Self::load_system_libraries`].
    pub fn load_system_libraries_retaining(
        &mut self,
        originals: &mut crate::persistence::SystemOriginals,
    ) -> Result<(), crate::JournalFault> {
        let (replayed, library_totals) = match &self.inner.libpinyin.dir {
            Some(dir) => crate::persistence::load_library_logs_live(dir, originals)?,
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
    pub fn unload_system_libraries(&mut self, originals: crate::persistence::SystemOriginals) {
        let unloaded: Vec<_> = originals.keys().copied().collect();
        self.replace_system_libraries(
            originals,
            std::collections::BTreeMap::new(),
            &std::collections::BTreeMap::new(),
        );
        self.inner.libpinyin.phrases().unloaded.extend(unloaded);
    }

    /// `_load_phrase_library`'s `USER_FILE` branch for `library`: its
    /// sub-index comes back from the saved chunk, or empty when there is
    /// none or it cannot be read (`create_sub_phrase`), so everything the
    /// session changed since the last save goes (074a2219
    /// `pinyin.cpp:303-321`, `zhuyin.cpp:247-265`). The phrase table and the
    /// user pinyin index keep their entries, separate from the sub-index: a
    /// reading the index carried stays indexed on the item the chunk brings
    /// back. In memory, with no failure of its own.
    pub fn load_user_library(&mut self, library: u8) {
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
            .and_then(|dir| crate::persistence::load_user_library(dir, &layout, library));
        self.replace_user_arena(library, Some(loaded.unwrap_or_default()));
    }

    /// `FacadePhraseIndex::unload` for the `USER_FILE` `library`: the
    /// sub-index goes with everything in it and with its share of the
    /// facade total, saved or not (074a2219 `phrase_index.cpp:261-268`).
    /// The phrase table and the user pinyin index keep their entries. In
    /// memory, with no failure of its own; a save skips the library while
    /// it is unloaded (`pinyin.cpp:928-933`).
    pub fn unload_user_library(&mut self, library: u8) {
        if !self.originals.layout().has_user_library(library) {
            return;
        }
        self.replace_user_arena(library, None);
    }

    /// The session side of a system library transition: each reloaded
    /// library's live buffer and total are replaced by the one its replay
    /// left in `replayed` (none when its log left the chunk as it is). The
    /// session's state is process memory, so this cannot fail, as
    /// `FacadePhraseIndex::load`/`unload` cannot (policy row 103).
    fn replace_system_libraries(
        &mut self,
        originals: crate::persistence::SystemOriginals,
        replayed: std::collections::BTreeMap<u8, crate::replay::ReplayedLibrary>,
        library_totals: &std::collections::BTreeMap<u8, u32>,
    ) {
        self.apply_system_libraries(&originals, replayed, library_totals);
        {
            let mut phrases = self.inner.libpinyin.phrases();
            for nibble in originals.keys() {
                phrases.unloaded.remove(nibble);
            }
        }
        self.originals.append(originals);
    }

    /// Synchronize a system library's mutation guard with runtime visibility.
    /// This does not replace its items or reset its training state. USER_FILE
    /// markers belong exclusively to the store's own item load/unload.
    pub fn set_library_loaded(&mut self, nibble: u8, loaded: bool) {
        if nibble >= 16 || !self.is_system_library(nibble) {
            return;
        }
        let mut phrases = self.inner.libpinyin.phrases();
        if loaded {
            phrases.unloaded.remove(&nibble);
        } else {
            phrases.unloaded.insert(nibble);
        }
    }

    /// Reset one loaded baseline's replay and training state at unload.
    pub fn reset_system_library(&mut self, nibble: u8) {
        if nibble >= 16 {
            return;
        }
        self.apply_system_libraries(
            &self.originals.selected(1 << nibble),
            std::collections::BTreeMap::new(),
            &Default::default(),
        );
        self.inner.libpinyin.phrases().unloaded.insert(nibble);
    }

    fn apply_system_libraries(
        &self,
        originals: &impl crate::persistence::OriginalLibraries,
        mut replayed: std::collections::BTreeMap<u8, crate::replay::ReplayedLibrary>,
        library_totals: &std::collections::BTreeMap<u8, u32>,
    ) {
        let target = Arc::clone(&self.inner.libpinyin);
        let mut offsets = self.offsets();
        // Flag invariant (see `compact_libraries`): set before the entries
        // can exist, and cleared below only once none is left, still under
        // the offsets lock.
        self.inner.has_library_offsets.store(true, Ordering::SeqCst);
        // A reloaded library has no stored total difference any more: its
        // total is the one its replay left, held in the live scalar below.
        for nibble in originals.keys() {
            offsets.remove(nibble);
        }
        let offsets_left = !offsets.is_empty();
        {
            // A library with an original keeps its items in its live buffer;
            // the arenas of the reloaded libraries hold nothing.
            let mut phrases = target.phrases();
            phrases
                .libraries
                .retain(|nibble, _| !originals.contains_key(nibble));
        }
        self.inner
            .has_library_offsets
            .store(offsets_left, Ordering::SeqCst);
        let buffer_live;
        {
            let phrases = target.phrases();
            let arena_has_system_items = phrases
                .libraries
                .iter()
                .any(|(library, items)| (1..=4).contains(library) && !items.is_empty());
            let mut items = lock_result_or_abort(self.inner.system_items.lock());
            // `FacadePhraseIndex::load` and `merge_with_mask` swap the old
            // sub-index for the new one (`phrase_index.cpp:237-248`,
            // `:301-332`): the library's live buffer and total go, and the
            // replay's buffer and total come in. The facade's total follows
            // the sub-index totals, and keeps what no sub-index took.
            for &nibble in originals.keys() {
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
            buffer_live = items.has_added();
            self.inner.has_system_items.store(
                !items.is_empty() || arena_has_system_items,
                Ordering::Relaxed,
            );
            self.inner
                .has_removed_system_items
                .store(items.has_removed(), Ordering::Relaxed);
        }
        let arena_live = {
            let phrases = target.phrases();
            phrases
                .libraries
                .values()
                .any(|library| !library.is_empty())
        } || buffer_live;
        // A session's grams are user data too.
        let has_grams = self.container().is_empty().is_ok_and(|empty| !empty);
        self.mark_committed_phrase_write(offsets, has_grams || arena_live);
    }

    /// The faults [`Self::mask_out`] reaches before it changes anything, in
    /// the pin's order (`pinyin.cpp:1228-1230`): an overlong user pinyin index
    /// key (rows 84 and 94), a phrase-table record the mask would read past
    /// (row 101), then the user bigram's non-token key, short value, partial
    /// tail and residual gram (rows 86, 85, 98, 87). It changes no state, so a
    /// caller can learn that the user tables fault before it opens any system
    /// library or journal. The bigram keys are walked here and again by
    /// [`Self::mask_out`]: time linear in the user bigram, nothing retained.
    ///
    /// # Errors
    ///
    /// The [`UserStoreError`] [`Self::mask_out`] would return.
    pub fn check_mask_out(&self, mask: Token, value: Token) -> Result<(), UserStoreError> {
        if self.has_overlong_index_key() {
            return Err(UserStoreError::OverlongIndexKey);
        }
        self.inner
            .libpinyin
            .phrases()
            .phrase_table
            .check_mask()
            .map_err(|error| UserStoreError::Persistence(error.to_string()))?;
        let _offsets = self.offsets();
        let container_keys = self.collect_container_keys();
        match self.bigram_mask_fault_over(&container_keys, mask, value) {
            Some(error) => Err(error),
            None => Ok(()),
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
    pub fn mask_out(&mut self, mask: Token, value: Token) -> Result<(), UserStoreError> {
        // The same checks [`Self::check_mask_out`] makes, under the guards
        // the mask below keeps.
        if self.has_overlong_index_key() {
            return Err(UserStoreError::OverlongIndexKey);
        }
        // `pinyin_mask_out` masks the user phrase table next
        // (`pinyin.cpp:1229` at 074a2219); a record whose entry walk it
        // would read past (register row 101) is refused before any change.
        let target = Arc::clone(&self.inner.libpinyin);
        target
            .phrases()
            .phrase_table
            .check_mask()
            .map_err(|error| UserStoreError::Persistence(error.to_string()))?;
        // The bigram walk's faults, in the pin's order: a non-token key
        // (`get_all_items`), then a short value (`get_total_freq`), then a
        // mask residual (`get_length`). The one `get_all_items` walk serves
        // the check and the mask below, under one offsets guard.
        let offsets = self.offsets();
        let container_keys = self.container_keys();
        if let Some(error) = self.bigram_mask_fault_over(&container_keys, mask, value) {
            return Err(error);
        }
        // The pin's mask walk edits every record of the user pinyin index,
        // arena or overlay; the next save writes the table as it stands.
        {
            let mut phrases = target.phrases();
            phrases.index.mask(mask, value);
            phrases
                .phrase_table
                .mask(mask, value)
                .map_err(UserStoreError::from)?;
        }
        // Commit succeeded: discard matched in-memory system payloads too.
        // Dropping an original's override restores it; dropping an appended
        // item removes its only payload. Do this before publishing the epoch.
        self.mask_arena(mask, value)?;
        // `m_user_bigram->mask_out(mask, value)` (`pinyin.cpp:1230`,
        // `zhuyin.cpp:765`).
        let grams_left = self.mask_container_over(container_keys, mask, value)?;
        self.mark_committed_phrase_write(offsets, grams_left || self.arena_has_items());
        Ok(())
    }

    /// `pinyin_remove_user_candidate`'s store side (§3.4).
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the phrase cannot be removed.
    pub fn remove_user_phrase(&mut self, token: Token) -> Result<bool, UserStoreError> {
        self.arena_remove(token)
    }
}

/// Entry cap per count-memo map. Bounds memo memory to O(cap) no matter
/// how many distinct keys a session scores against; the value sits far
/// above any trained store's working set. On overflow the map resets —
/// entries are pure speed hints, so wholesale eviction changes nothing
/// but repeat-read cost.
const COUNT_MEMO_MAX_ENTRIES: usize = 8192;

/// Inserts into a count-memo map, resetting it first once at capacity.
fn memo_insert<K: std::hash::Hash + Eq, V>(map: &mut HashMap<K, V>, key: K, value: V) {
    if map.len() >= COUNT_MEMO_MAX_ENTRIES {
        map.clear();
    }
    map.insert(key, value);
}

impl CountCache {
    /// An empty memo bound to `generation`.
    pub(crate) fn new(generation: u64) -> Self {
        Self {
            generation,
            bigram_rows: HashMap::new(),
        }
    }

    /// `prev`'s raw gram, memoised like [`Self::unigram`]: a present gram
    /// is kept until the generation moves; an absent one reads as `None`
    /// without entering the memo.
    fn bigram_row(
        &mut self,
        prev: Token,
        load: impl FnOnce() -> Result<Option<Vec<u8>>, UserStoreError>,
    ) -> Result<Option<&[u8]>, UserStoreError> {
        if !self.bigram_rows.contains_key(&prev) {
            let Some(row) = load()? else {
                return Ok(None);
            };
            memo_insert(&mut self.bigram_rows, prev, row);
        }
        Ok(self.bigram_rows.get(&prev).map(Vec::as_slice))
    }
}

impl UserStore {
    /// The system items the live buffers hold that differ from the originals.
    #[cfg(test)]
    pub(crate) fn live_system_changes(
        &self,
        originals: &impl crate::persistence::OriginalLibraries,
    ) -> std::collections::BTreeMap<Token, Option<oxpinyin_data::chunk_write::ChunkItem>> {
        self.inner
            .system_items
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .changed_items(originals)
    }

    /// Whether this session carries system-item payloads from imports or logs.
    #[must_use]
    pub(crate) fn has_system_items(&self) -> bool {
        self.inner.has_system_items.load(Ordering::Relaxed)
    }

    /// Whether a journal REMOVE took out a system item of this session
    /// ([`Self::system_item_removed`]): the one atomic that gates the
    /// per-token check.
    #[must_use]
    pub(crate) fn has_removed_system_items(&self) -> bool {
        self.inner.has_removed_system_items.load(Ordering::Relaxed)
    }

    /// Whether a journal REMOVE took the item behind system `token` out of
    /// its sub-index: `get_phrase_item` fails for it (074a2219
    /// `phrase_index.cpp:219-236`), while the shipped chunk and the phrase
    /// table still hold it. The library's live buffer answers no item where
    /// the chunk holds one.
    #[must_use]
    pub(crate) fn system_item_removed(&self, token: Token) -> bool {
        self.has_removed_system_items()
            && lock_result_or_abort(self.inner.system_items.lock()).is_removed(
                token,
                self.originals.get(&phrase_index_library_index(token)),
            )
    }

    /// Whether system library `nibble`'s live buffer records any slot's
    /// first-write provenance.
    #[cfg(test)]
    pub(crate) fn system_provenance_recorded(&self, nibble: u8) -> bool {
        self.inner
            .system_items
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .has_provenance(nibble)
    }

    #[cfg(test)]
    pub(crate) fn test_forget_removal(&self, _token: Token) {
        self.inner
            .has_removed_system_items
            .store(false, Ordering::Relaxed);
    }

    #[cfg(test)]
    /// The imported/replayed system item, with its current unigram field.
    ///
    /// # Errors
    /// Returns an error if the scratch payload or count cannot be read.
    pub(crate) fn system_item_override(
        &self,
        token: Token,
    ) -> Result<Option<oxpinyin_data::chunk_write::ChunkItem>, UserStoreError> {
        if !(1..=4).contains(&(token >> 24)) {
            return Ok(None);
        }
        let Some(mut item) = self.with_effective_system_item(token, Clone::clone) else {
            return Ok(None);
        };
        if let Some(unigram) = self.system_unigram(token) {
            item.unigram = unigram;
        }
        Ok(Some(item))
    }

    /// The stored pronunciations of the item this store holds behind
    /// `token`, as `(keys, frequency)` rows in stored order (074a2219
    /// `phrase_index.h` `get_nth_pronunciation`): an imported or trained
    /// system item, or a user-file library's phrase.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when a user phrase cannot be read.
    pub fn item_pronunciations(
        &self,
        token: Token,
    ) -> Result<Option<oxpinyin_core::PronunciationRows>, UserStoreError> {
        use crate::store_libpinyin::pinyin_keys_to_packed;
        if let Some(readings) = self.with_user_item(token, |item| {
            item.pronunciations()
                .map(|reading| {
                    (
                        reading
                            .keys
                            .as_chunks::<2>()
                            .0
                            .iter()
                            .map(|b| ChewingKey::from_packed(u16::from_le_bytes([b[0], b[1]])))
                            .collect(),
                        reading.freq,
                    )
                })
                .collect()
        }) {
            return Ok(Some(readings));
        }
        if self.is_system_library(phrase_index_library_index(token)) {
            return Ok(self.with_effective_system_item(token, |item| {
                item.prons
                    .iter()
                    .map(|(packed, frequency)| {
                        let keys = packed
                            .iter()
                            .map(|&bits| oxpinyin_core::ChewingKey::from_packed(bits))
                            .collect();
                        (keys, *frequency)
                    })
                    .collect()
            }));
        }
        match token >> 24 {
            5..=7 => Ok(self.phrase(token)?.map(|phrase| {
                phrase
                    .pronunciations()
                    .iter()
                    .filter_map(|pronunciation| {
                        let keys = pinyin_keys_to_packed(pronunciation.keys())?
                            .into_iter()
                            .map(oxpinyin_core::ChewingKey::from_packed)
                            .collect();
                        let frequency = u32::try_from(pronunciation.count()).unwrap_or(u32::MAX);
                        Some((keys, frequency))
                    })
                    .collect()
            })),
            _ => Ok(None),
        }
    }

    /// Exclusive token end of imported system items in one library, without
    /// cloning payloads for the streaming export's range probe.
    #[must_use]
    pub(crate) fn system_item_end(&self, library: u8) -> Option<Token> {
        if !self.is_system_library(library) {
            return None;
        }
        let arena_end = self
            .inner
            .libpinyin
            .phrases()
            .libraries
            .get(&library)
            .filter(|items| !items.is_empty())
            .map(|items| (u32::from(library) << 24) | items.token_end());
        lock_result_or_abort(self.inner.system_items.lock())
            .end(library)
            .into_iter()
            .chain(arena_end)
            .max()
    }

    /// The effective live system item shared by export, n-best and token
    /// introspection (074a2219 pinyin.cpp:707-724; phonetic_lookup.h:643-693).
    /// Original counts plus exact-reading deltas are resolved once when
    /// training commits, with the pin's prefix-total overflow guard, rather
    /// than rereading the store at every pronunciation probability. Imports
    /// and replay publish their absolute payloads into the same view.
    /// Untouched items stay mapped: `None` tells the caller to read the
    /// original. A borrowed item is stable for this call and copied nowhere.
    /// Only its unigram field is stale; [`Self::system_item_override`]
    /// supplies the live unigram when a caller needs that field too.
    pub(crate) fn with_effective_system_item<R>(
        &self,
        token: Token,
        read: impl FnOnce(&oxpinyin_data::chunk_write::ChunkItem) -> R,
    ) -> Option<R> {
        if !self.is_system_library(phrase_index_library_index(token)) {
            return None;
        }
        if let Some(item) = self
            .with_user_item(token, |view| {
                oxpinyin_data::chunk_write::decode_phrase_item(view.as_bytes()).ok()
            })
            .flatten()
        {
            return Some(read(&item));
        }
        if !self.has_system_items() {
            return None;
        }
        let items = lock_result_or_abort(self.inner.system_items.lock());
        items.get(&token).map(|item| read(&item))
    }

    #[cfg(test)]
    pub(crate) fn system_items_locked(&self) -> bool {
        self.inner.system_items.try_lock().is_err()
    }

    /// Reads the item a system library's live buffer holds under `token`.
    /// Nothing is decoded or kept.
    pub(crate) fn with_live_item<R>(
        &self,
        token: Token,
        read: impl FnOnce(&[u8]) -> R,
    ) -> Option<R> {
        if !self.has_system_items() {
            return None;
        }
        lock_result_or_abort(self.inner.system_items.lock()).with_live(token, read)
    }

    /// Count the pristine items held by one transferred library.
    pub fn pristine_system_item_count(&self, nibble: u8) -> Option<u64> {
        lock_result_or_abort(self.inner.system_items.lock()).pristine_item_count(nibble)
    }

    pub(crate) fn live_pronunciation_count(&self, token: Token) -> Option<u32> {
        lock_result_or_abort(self.inner.system_items.lock()).live_pronunciation_count(token)
    }

    pub(crate) fn live_item_past_buffer(&self, token: Token) -> bool {
        lock_result_or_abort(self.inner.system_items.lock()).live_item_past_buffer(token)
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
    /// `InvalidPhrase` for a library that is not a `USER_FILE` one.
    pub fn user_library_token_end(&self, library: u8) -> Result<Token, UserStoreError> {
        if !is_user_file_library(library) {
            return Err(UserStoreError::InvalidPhrase);
        }
        Ok(phrase::phrase_index_make_token(
            library,
            self.inner
                .libpinyin
                .phrases()
                .libraries
                .get(&library)
                .map_or(1, crate::user_library::UserLibrary::token_end),
        ))
    }

    /// `_add_phrase` for a system library (074a2219, pinyin.cpp:515-611).
    /// `original_token` comes from the combined system/user phrase-table
    /// search, restricted to this library, including prior imported items.
    /// Packed original readings are retained verbatim, including their order.
    ///
    /// # Errors
    /// Returns an error for invalid input, unavailable originals, or store failure.
    pub fn add_system_phrase_in(
        &mut self,
        library: u8,
        original_token: Option<Token>,
        text: &oxpinyin_core::PhraseText,
        keys: &[PinyinKey],
        count: Option<u64>,
    ) -> Result<Token, UserStoreError> {
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
        if original_token.is_none()
            || original_token.is_some_and(|token| self.with_appended_item(token, |_| ()).is_some())
        {
            return self.arena_add_phrase(
                library,
                &words,
                keys,
                delta,
                Some(original_token),
                range_end,
            );
        }
        let token = original_token.ok_or(UserStoreError::InvalidPhrase)?;
        let packed = crate::store_libpinyin::pinyin_keys_to_packed(keys)
            .ok_or(UserStoreError::InvalidPhrase)?;
        let reading = SystemReading {
            packed: &packed,
            delta,
        };
        let offsets = self.offsets();
        let mut items = lock_result_or_abort(self.inner.system_items.lock());
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
        let bytes = oxpinyin_data::chunk_write::encode_phrase_item(&item)
            .map_err(|error| UserStoreError::Persistence(error.to_string()))?;
        let removed = items.remove_item(original, token).is_some();
        let _ = items.add_item(original, token, &bytes);
        if !removed {
            let library = phrase_index_library_index(token);
            let total = items
                .library_total(library, Some(original))
                .unwrap_or(original.total);
            items.set_library_total(
                library,
                total.wrapping_sub(crate::replay::raw_unigram(&bytes)),
            );
        }
        drop(items);
        self.inner.has_system_items.store(true, Ordering::Relaxed);
        self.mark_committed_import_write(offsets, false);
        Ok(token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poisoned_mutex_aborts() {
        const DEATH_TEST: &str = "OXPINYIN_USER_DEATH_TEST";
        const NAME: &str = "store::tests::poisoned_mutex_aborts";

        if std::env::var_os(DEATH_TEST).is_some_and(|value| value == NAME) {
            let mutex = std::sync::Mutex::new(());
            let panic = std::panic::catch_unwind(|| {
                let _guard = mutex.lock().expect("unpoisoned mutex");
                panic!("poison the mutex");
            });
            assert!(panic.is_err());
            let result = mutex.lock();
            assert!(result.is_err(), "the mutex must be poisoned");
            drop(lock_result_or_abort(result));
            std::process::exit(0);
        }

        let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
            .args(["--exact", NAME, "--test-threads=1"])
            .env(DEATH_TEST, NAME)
            .output()
            .expect("re-run the test binary");
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt as _;
            assert_eq!(
                output.status.signal(),
                Some(6),
                "expected SIGABRT: {output:?}"
            );
        }
        #[cfg(windows)]
        assert_eq!(
            output.status.code(),
            Some(i32::from_ne_bytes(0xC000_0409_u32.to_ne_bytes())),
            "expected STATUS_STACK_BUFFER_OVERRUN: {output:?}"
        );
    }

    fn next_user_token(store: &UserStore) -> Token {
        let end = store
            .inner
            .libpinyin
            .phrases()
            .libraries
            .get(&USER_DICTIONARY)
            .map_or(1, crate::user_library::UserLibrary::token_end);
        crate::phrase::add_phrase_token(USER_DICTIONARY, end)
    }
    use std::collections::BTreeMap;

    /// 074a2219 phrase_index.h:634 increments the facade before a refused
    /// sub-index add; phrase_index.cpp:243-248 reload adjusts only sub-index totals.
    #[test]
    fn reload_preserves_the_delta_no_subindex_accepted() {
        use crate::persistence::{SystemLibrary, SystemOriginals, UserConfLaw};
        let originals = || {
            SystemOriginals::from(BTreeMap::from([(
                1,
                SystemLibrary {
                    mapped: None,
                    total: u32::MAX,
                    range_end: 2,
                    items: BTreeMap::from([(
                        1,
                        oxpinyin_data::chunk_write::ChunkItem {
                            phrase: vec![0x4f60],
                            unigram: u32::MAX,
                            prons: Vec::new(),
                        },
                    )]),
                },
            )]))
        };
        let mut store = UserStore::open_transient(
            originals(),
            oxpinyin_data::user_files::SystemVersions::for_this_build(7, 14),
            UserConfLaw::Pinyin,
        )
        .unwrap();
        assert_eq!(
            store.add_unigram_outcome(0x0100_0001, 7).unwrap(),
            UnigramAdd::Refused
        );
        assert_eq!(store.unigram_total().unwrap(), 7);
        store.load_system_libraries(originals()).unwrap();
        assert_eq!(store.system_library_total(1), Some(u32::MAX));
        assert_eq!(store.unigram_total().unwrap(), 7);
    }

    #[test]
    fn system_import_changes_the_first_match_and_guards_its_prefix() {
        let mut item = oxpinyin_data::chunk_write::ChunkItem {
            phrase: vec![0x4f60],
            unigram: 0,
            prons: vec![(vec![1], u32::MAX - 10), (vec![2], 3), (vec![2], 5)],
        };
        let mut reading = SystemReading {
            packed: &[2],
            delta: 7,
        };
        let change = plan_system_reading(&item, &reading);
        validate_system_merge(&item, &reading, change).expect("valid item");
        apply_system_reading(&mut item, &reading, change);
        assert_eq!(
            item.prons[1].1, 10,
            "the prefix fits even though the whole total does not"
        );
        assert_eq!(item.prons[2].1, 5, "the later exact match stays untouched");
        reading.delta = 1;
        let change = plan_system_reading(&item, &reading);
        apply_system_reading(&mut item, &reading, change);
        assert_eq!(item.prons[1].1, 10, "overflow refuses the increment");
    }

    // ── The libpinyin session: the constructors the C libraries reach ──

    /// The store the shipped libraries open: `open_transient` is the pin's
    /// NULL-user-directory session, `open_libpinyin` a session over a user
    /// directory. Every value operation runs on the arena, the pin's user
    /// bigram container and the live system items; nothing here reads a
    /// DBM row. A unigram belongs to an item, as the pin's does
    /// (074a2219 `phrase_index.cpp:150-161`: `add_unigram_frequency` of a
    /// token without an item is `ERROR_NO_ITEM`), so a test that trains a
    /// count allocates its token first.
    mod session {
        use super::super::*;
        use super::next_user_token;
        use crate::persistence::{SystemLibrary, SystemOriginals, UserConfLaw};
        use crate::{
            ADDON_DICTIONARY, FIRST_USER_TOKEN, NETWORK_DICTIONARY, PHRASE_INDEX_LIBRARY_MASK,
            PHRASE_MASK, USER_DICTIONARY, phrase_index_make_token,
        };
        use oxpinyin_core::SyllableKey;
        use oxpinyin_data::chunk_write::{ChunkItem, encode_phrase_item};
        use oxpinyin_data::user_files::{SystemVersions, UserFileLayout};
        use std::collections::BTreeMap;

        fn versions() -> SystemVersions {
            SystemVersions::for_this_build(7, 14)
        }

        /// No system library: the stock layout names the user, network
        /// and addon `USER_FILE` libraries and nothing else.
        fn no_originals() -> SystemOriginals {
            SystemOriginals::new(Default::default(), UserFileLayout::stock())
        }

        /// The pin's NULL-user-directory session.
        fn transient() -> UserStore {
            UserStore::open_transient(no_originals(), versions(), UserConfLaw::Pinyin).unwrap()
        }

        /// A session over a user directory, for the tests that save: a
        /// transient session has nowhere to write, so its `save` answers
        /// `false` and leaves the dirty flag.
        fn in_dir(originals: SystemOriginals) -> (tempfile::TempDir, UserStore) {
            let dir = tempfile::tempdir().unwrap();
            let store =
                UserStore::open_libpinyin(dir.path(), originals, versions(), UserConfLaw::Pinyin)
                    .unwrap();
            (dir, store)
        }

        /// A user item with a zero unigram and one reading of count zero:
        /// the token the pinned seed arithmetic is read on, bare.
        fn bare_token(store: &mut UserStore, text: &str, keys: &[PinyinKey]) -> Token {
            store.add_phrase(&text.into(), keys, Some(0)).unwrap()
        }

        fn key(text: &str) -> PinyinKey {
            toned(text, 0)
        }

        fn toned(text: &str, tone: u8) -> PinyinKey {
            crate::toned_key(
                SyllableKey::from_text(text)
                    .expect("frozen syllable")
                    .index(),
                tone,
            )
            .expect("frozen syllable inventory fits u16")
        }

        /// One item at `slot` of `library`'s arena, so its `get_range` end
        /// is one past it. A slot far past the content takes the sparse
        /// offset map, not the pin's dense offset array.
        fn plant_item(store: &UserStore, library: u8, slot: u32) {
            let bytes = encode_phrase_item(&ChunkItem {
                phrase: vec![0x7532],
                unigram: 0,
                prons: Vec::new(),
            })
            .unwrap();
            store
                .session_target()
                .phrases()
                .libraries
                .entry(library)
                .or_default()
                .replace(slot, &bytes)
                .unwrap();
        }

        #[test]
        fn open_creates_empty_store() {
            let store = transient();
            assert!(!store.has_user_data());
            assert_eq!(store.write_generation(), 0);
            assert_eq!(store.bigram_count(1, 2).unwrap(), 0);
            assert_eq!(store.bigram_total(1).unwrap(), 0);
            assert_eq!(store.unigram_delta(2).unwrap(), 0);
            assert_eq!(store.unigram_total().unwrap(), 0);
            assert_eq!(store.count_delta(Some(1), 2).unwrap(), UserCountDelta::ZERO);
            assert_eq!(store.count_delta(None, 2).unwrap(), UserCountDelta::ZERO);
            assert!(
                store.count_cache().is_none(),
                "empty count_delta must not build a count memo"
            );
        }

        #[test]
        fn cached_count_delta_refreshes_after_committed_writes() {
            let mut store = transient();
            let cur = bare_token(&mut store, "你", &[key("ni")]);
            assert_eq!(
                store.count_delta(Some(1), cur).unwrap(),
                UserCountDelta::ZERO
            );
            let opened = store.write_generation();

            store.observe_selection(1, cur).unwrap();
            let first_generation = store.write_generation();
            assert!(first_generation > opened);
            assert!(store.has_user_data());
            assert_eq!(
                store.count_delta(Some(1), cur).unwrap(),
                UserCountDelta {
                    bigram_count: 69,
                    bigram_total: 69,
                    unigram_delta: 483,
                    unigram_total_delta: 483,
                    unigram_live: None,
                }
            );

            store.observe_selection(1, cur).unwrap();
            assert!(
                store.write_generation() > first_generation,
                "a committed write invalidates the cached count reads"
            );
            assert_eq!(
                store.count_delta(Some(1), cur).unwrap(),
                UserCountDelta {
                    bigram_count: 207,
                    bigram_total: 207,
                    unigram_delta: 483 + 966,
                    unigram_total_delta: 483 + 966,
                    unigram_live: None,
                }
            );
        }

        #[test]
        fn mask_out_all_marks_store_empty_again() {
            let mut store = transient();
            let cur = bare_token(&mut store, "你", &[key("ni")]);
            store.observe_selection(1, cur).unwrap();
            assert!(store.has_user_data());
            assert_ne!(
                store.count_delta(Some(1), cur).unwrap(),
                UserCountDelta::ZERO
            );

            store.mask_out(0, 0).unwrap();
            assert!(!store.has_user_data());
            assert_eq!(
                store.count_delta(Some(1), cur).unwrap(),
                UserCountDelta::ZERO
            );
            assert!(
                store.count_cache().is_none(),
                "an emptied store must not keep a count memo"
            );
        }

        /// `Bigram::mask_out` (`ngram_tkrzwdb.cpp:195-198` at 074a2219)
        /// stores a gram back only when the mask removed an item. An
        /// item-less gram has nothing to remove, so it stays as it is,
        /// a zero total included.
        #[test]
        fn mask_out_leaves_an_itemless_gram_untouched() {
            let mut store = transient();
            let user = phrase_index_make_token(USER_DICTIONARY, 1);
            store.plant_raw_bigram(1, &17_u32.to_le_bytes());
            store.plant_raw_bigram(2, &0_u32.to_le_bytes());
            let emptied = [9_u32.to_le_bytes(), user.to_le_bytes(), 9_u32.to_le_bytes()].concat();
            store.plant_raw_bigram(3, &emptied);

            store
                .mask_out(
                    PHRASE_INDEX_LIBRARY_MASK,
                    phrase_index_make_token(USER_DICTIONARY, 0),
                )
                .unwrap();

            assert_eq!(
                store.bigram_row(1).unwrap(),
                Some(17_u32.to_le_bytes().to_vec())
            );
            assert_eq!(
                store.bigram_row(2).unwrap(),
                Some(0_u32.to_le_bytes().to_vec())
            );
            // A gram the mask empties goes.
            assert_eq!(store.bigram_row(3).unwrap(), None);
        }

        #[test]
        fn save_compacts_after_a_cached_read() {
            let (_dir, mut store) = in_dir(no_originals());
            store.observe_selection(1, 100).unwrap();
            assert_eq!(store.count_delta(Some(1), 100).unwrap().bigram_count, 69);
            store.save_reporting().unwrap();
            assert_eq!(store.count_delta(Some(1), 100).unwrap().bigram_count, 69);
        }

        #[test]
        fn observe_applies_pinned_seed_sequence() {
            let mut store = transient();
            let cur = bare_token(&mut store, "你", &[key("ni")]);

            assert_eq!(store.observe_selection(1, cur).unwrap(), 69);
            assert_eq!(store.bigram_count(1, cur).unwrap(), 69);
            assert_eq!(store.bigram_total(1).unwrap(), 69);
            assert_eq!(store.unigram_delta(cur).unwrap(), 483);
            assert_eq!(store.unigram_total().unwrap(), 483);

            assert_eq!(store.observe_selection(1, cur).unwrap(), 138);
            assert_eq!(store.bigram_count(1, cur).unwrap(), 207);
            assert_eq!(store.bigram_total(1).unwrap(), 207);
            assert_eq!(store.unigram_delta(cur).unwrap(), 483 + 966);
            assert_eq!(store.unigram_total().unwrap(), 483 + 966);
            assert_eq!(
                store.count_delta(Some(1), cur).unwrap(),
                UserCountDelta {
                    bigram_count: 207,
                    bigram_total: 207,
                    unigram_delta: 483 + 966,
                    unigram_total_delta: 483 + 966,
                    unigram_live: None,
                }
            );
            assert_eq!(
                store.count_delta(None, cur).unwrap(),
                UserCountDelta {
                    bigram_count: 0,
                    bigram_total: 0,
                    unigram_delta: 483 + 966,
                    unigram_total_delta: 483 + 966,
                    unigram_live: None,
                }
            );
        }

        #[test]
        fn totals_accumulate_per_predecessor() {
            let mut store = transient();
            assert_eq!(store.observe_selection(5, 10).unwrap(), 69);
            assert_eq!(store.observe_selection(5, 11).unwrap(), 69);
            assert_eq!(store.bigram_count(5, 10).unwrap(), 69);
            assert_eq!(store.bigram_count(5, 11).unwrap(), 69);
            assert_eq!(store.bigram_total(5).unwrap(), 138);
        }

        #[test]
        fn set_bigram_count_plants_filter_edge() {
            let mut store = transient();
            store.set_bigram_count(1, 10, 9).unwrap();
            store.set_bigram_count(1, 11, 10).unwrap();
            assert_eq!(store.bigram_count(1, 10).unwrap(), 9);
            assert_eq!(store.bigram_count(1, 11).unwrap(), 10);
            assert_eq!(store.bigram_total(1).unwrap(), 19);
        }

        #[test]
        fn predicted_path_is_flat_69() {
            let mut store = transient();
            let cur = bare_token(&mut store, "你", &[key("ni")]);
            assert_eq!(store.observe_predicted(1, cur).unwrap(), 69);
            assert_eq!(store.observe_predicted(1, cur).unwrap(), 69);
            assert_eq!(store.bigram_count(1, cur).unwrap(), 138);
            assert_eq!(store.bigram_total(1).unwrap(), 138);
            assert_eq!(store.unigram_delta(cur).unwrap(), 483 * 2);
        }

        #[test]
        fn predicted_prefix_invalidates_counts_without_training_a_bigram() {
            let mut store = transient();
            let cur = bare_token(&mut store, "你", &[key("ni")]);
            store.set_bigram_count(1, cur, 10).unwrap();
            assert_eq!(store.count_delta(Some(1), cur).unwrap().unigram_delta, 0);
            let generation = store.generation();
            for _ in 0..2 {
                assert_eq!(store.observe_predicted_prefix(cur).unwrap(), 69);
            }
            assert!(store.generation() > generation);
            assert_eq!(store.count_delta(Some(1), cur).unwrap().unigram_delta, 966);
            assert_eq!(store.unigram_total().unwrap(), 966);
            assert_eq!(store.bigram_count(1, cur).unwrap(), 10);
            assert_eq!(store.bigram_total(1).unwrap(), 10);
            assert!(!store.is_modified());
        }

        #[test]
        fn predicted_prefix_overflow_keeps_only_the_facade_total_increment() {
            let mut store = transient();
            let token = store
                .add_phrase(
                    &"你好".into(),
                    &[key("ni"), key("hao")],
                    Some(1_431_655_631),
                )
                .unwrap();
            let before = store.unigram_delta(token).unwrap();
            let total = store.unigram_total().unwrap();
            let error = store.observe_predicted_prefix(token).unwrap_err();
            assert!(matches!(error, UserStoreError::UnigramTotalOverflow));
            assert_eq!(store.unigram_delta(token).unwrap(), before);
            assert_eq!(store.unigram_total().unwrap(), total + 483);
            assert!(store.bigram_predecessors().unwrap().is_empty());
        }

        #[test]
        fn dirty_gate_matches_m_modified_semantics() {
            let (_dir, mut store) = in_dir(no_originals());

            assert!(!store.is_modified());
            assert_eq!(store.save_reporting().unwrap(), Default::default());
            assert_eq!(store.save_reporting().unwrap(), Default::default());

            store.observe_selection(SENTENCE_START, 10).unwrap();
            assert!(store.is_modified());
            store.save_reporting().unwrap();
            assert!(!store.is_modified());
            assert_eq!(store.save_reporting().unwrap(), Default::default());

            store.observe_predicted(10, 20).unwrap();
            assert!(!store.is_modified());
            assert_eq!(store.save_reporting().unwrap(), Default::default());
            store
                .add_phrase(&"你好".into(), &[key("ni"), key("hao")], None)
                .unwrap();
            assert!(!store.is_modified());
            assert_eq!(store.save_reporting().unwrap(), Default::default());

            store.observe_selection(SENTENCE_START, 10).unwrap();
            assert!(store.is_modified());
            store.save_reporting().unwrap();
            assert!(!store.is_modified());
        }

        #[test]
        fn first_allocation_is_first_user_token() {
            let mut store = transient();
            assert_eq!(next_user_token(&store), FIRST_USER_TOKEN);
            let token = store
                .add_phrase(&"你好".into(), &[key("ni"), key("hao")], None)
                .unwrap();
            assert_eq!(token, FIRST_USER_TOKEN);
            assert_eq!(token, 0x0700_0001);
            assert!(phrase::is_user_token(token));
            assert_eq!(next_user_token(&store), FIRST_USER_TOKEN + 1);
        }

        #[test]
        fn allocation_increments_by_one_without_gap() {
            let mut store = transient();
            let a = store.add_phrase(&"甲".into(), &[key("jia")], None).unwrap();
            let b = store.add_phrase(&"乙".into(), &[key("yi")], None).unwrap();
            let c = store
                .add_phrase(&"丙".into(), &[key("bing")], None)
                .unwrap();
            assert_eq!(a, FIRST_USER_TOKEN);
            assert_eq!(b, a + 1);
            assert_eq!(c, a + 2);
            assert_eq!(next_user_token(&store), a + 3);
        }

        #[test]
        fn user_token_is_distinguishable_from_system_token() {
            const SYSTEM: Token = 0x0100_0001;
            let mut store = transient();
            let user = store.add_phrase(&"词".into(), &[key("ci")], None).unwrap();
            assert!(phrase::is_user_token(user));
            assert!(!phrase::is_user_token(SYSTEM));
        }

        #[test]
        fn network_and_user_can_share_phrase_text() {
            let mut store = transient();
            let user = store
                .add_phrase_in(USER_DICTIONARY, &"词".into(), &[key("ci")], Some(5))
                .unwrap();
            let net = store
                .add_phrase_in(NETWORK_DICTIONARY, &"词".into(), &[key("ci")], Some(5))
                .unwrap();
            assert_ne!(user, net);
            assert_eq!(phrase::phrase_index_library_index(user), 7);
            assert_eq!(phrase::phrase_index_library_index(net), 6);
            assert_eq!(store.export_phrases_in(USER_DICTIONARY).unwrap().len(), 1);
            assert_eq!(store.export_phrases_in(6).unwrap().len(), 1);
            assert_eq!(next_user_token(&store), user + 1);
        }

        #[test]
        fn add_phrase_seeds_unigram_with_count_times_three() {
            let mut store = transient();
            let token = store
                .add_phrase(&"你好".into(), &[key("ni"), key("hao")], None)
                .unwrap();
            assert_eq!(store.unigram_delta(token).unwrap(), 15);
            assert_eq!(store.bigram_count(SENTENCE_START, token).unwrap(), 0);

            let token2 = store
                .add_phrase(&"世界".into(), &[key("shi"), key("jie")], Some(10))
                .unwrap();
            assert_eq!(store.unigram_delta(token2).unwrap(), 30);
        }

        /// `_add_phrase` adds `count * 3` as a `guint32` product: past
        /// `u32::MAX` it wraps (1431655766 * 3 = 2^32 + 2).
        #[test]
        fn add_phrase_count_times_three_wraps_past_u32() {
            let mut store = transient();
            let token = store
                .add_phrase(
                    &"你好".into(),
                    &[key("ni"), key("hao")],
                    Some(1_431_655_766),
                )
                .unwrap();
            assert_eq!(store.unigram_delta(token).unwrap(), 2);
        }

        /// The sub-index refuses a unigram add that would overflow its
        /// `guint32` total (`phrase_index.cpp:169-171`): training goes on
        /// without the unigram, and an accepted predicted candidate
        /// answers `ERROR_INTEGER_OVERFLOW` before its bigram
        /// (`pinyin.cpp:2609-2612`).
        #[test]
        fn unigram_adds_stop_at_the_library_total() {
            let mut store = transient();
            // 402 below `u32::MAX`: room for no 483 delta.
            let token = store
                .add_phrase(
                    &"你好".into(),
                    &[key("ni"), key("hao")],
                    Some(1_431_655_631),
                )
                .unwrap();
            let full = store.unigram_delta(token).unwrap();
            assert_eq!(full, 4_294_966_893);

            let err = store.observe_predicted(SENTENCE_START, token).unwrap_err();
            assert!(matches!(err, UserStoreError::UnigramTotalOverflow));
            assert_eq!(store.bigram_count(SENTENCE_START, token).unwrap(), 0);
            assert_eq!(store.unigram_delta(token).unwrap(), full);

            // Training keeps its bigram and loses only the unigram.
            store.observe_selection(SENTENCE_START, token).unwrap();
            assert_eq!(store.bigram_count(SENTENCE_START, token).unwrap(), 69);
            assert_eq!(store.unigram_delta(token).unwrap(), full);
        }

        #[test]
        fn phrase_table_generation_keeps_readings_but_tracks_shared_token_edits() {
            let mut store = transient();
            assert_eq!(store.phrase_table_generation(), 0);
            let token = store
                .add_phrase(&"你好".into(), &[key("ni"), key("hao")], None)
                .unwrap();
            assert_eq!(store.phrase_table_generation(), 1);
            let write_before = store.generation();
            assert_eq!(
                store
                    .add_phrase(&"你好".into(), &[key("ni"), toned("hao", 3)], None)
                    .unwrap(),
                token
            );
            assert_eq!(store.generation(), write_before + 1);
            assert_eq!(store.phrase_table_generation(), 1);
            store.observe_selection(SENTENCE_START, token).unwrap();
            assert_eq!(store.phrase_table_generation(), 1);

            let second = store
                .add_phrase(&"中国".into(), &[key("zhong"), key("guo")], None)
                .unwrap();
            assert_eq!(store.phrase_table_generation(), 2);
            assert!(store.remove_user_phrase(second).unwrap());
            assert_eq!(store.phrase_table_generation(), 3);
        }

        #[test]
        fn existing_phrase_merges_a_new_reading() {
            let mut store = transient();
            let first = store
                .add_phrase(&"你好".into(), &[key("ni"), key("hao")], None)
                .unwrap();
            let again = store
                .add_phrase(&"你好".into(), &[toned("ni", 3), key("hao")], Some(8))
                .unwrap();
            assert_eq!(first, again);
            assert_eq!(next_user_token(&store), FIRST_USER_TOKEN + 1);
            // The merge is a remove and an add of the same item
            // (`pinyin.cpp:576-580`; `phrase_index.h:669`, `:691`): no
            // unigram moves.
            assert_eq!(store.unigram_delta(first).unwrap(), 15);

            let got = store.phrase(first).unwrap().unwrap();
            assert_eq!(got.phrase_text().unwrap(), "你好");
            assert_eq!(got.pronunciations().len(), 2);
            assert_eq!(got.pronunciations()[0].keys(), &[key("ni"), key("hao")]);
            assert_eq!(got.pronunciations()[0].count(), 5);
            assert_eq!(
                got.pronunciations()[1].keys(),
                &[toned("ni", 3), key("hao")]
            );
            assert_eq!(got.pronunciations()[1].count(), 8);
        }

        #[test]
        fn same_reading_accumulates_pronunciation_count() {
            let mut store = transient();
            let token = store
                .add_phrase(&"词".into(), &[key("ci")], Some(5))
                .unwrap();
            let again = store
                .add_phrase(&"词".into(), &[key("ci")], Some(5))
                .unwrap();
            assert_eq!(token, again);
            assert_eq!(store.unigram_delta(token).unwrap(), 15);
            let got = store.phrase(token).unwrap().unwrap();
            assert_eq!(got.pronunciations().len(), 1);
            assert_eq!(got.pronunciations()[0].count(), 10);
        }

        #[test]
        fn invalid_phrase_is_rejected_without_allocation() {
            let mut store = transient();
            assert!(matches!(
                store.add_phrase(&"".into(), &[], None),
                Err(UserStoreError::InvalidPhrase)
            ));
            assert!(matches!(
                store.add_phrase(&"你好".into(), &[key("ni")], None),
                Err(UserStoreError::InvalidPhrase)
            ));
            assert!(matches!(
                store.add_phrase(
                    &oxpinyin_core::PhraseText::from(&"啊".repeat(16)),
                    &[key("a"); 16],
                    None
                ),
                Err(UserStoreError::InvalidPhrase)
            ));
            assert_eq!(next_user_token(&store), FIRST_USER_TOKEN);
            assert!(store.token_for_phrase("你好").unwrap().is_none());
        }

        #[test]
        fn lookup_of_unknown_token_is_none() {
            let store = transient();
            assert!(store.phrase(FIRST_USER_TOKEN).unwrap().is_none());
            assert!(store.phrase(0x0100_0001).unwrap().is_none());
        }

        #[test]
        fn export_phrases_render_the_pinned_triples() {
            let mut store = transient();
            store
                .add_phrase(&"你好".into(), &[key("ni"), key("hao")], None)
                .unwrap();
            store
                .add_phrase(&"你好".into(), &[key("ni"), key("hao")], Some(7))
                .unwrap();
            store
                .add_phrase(&"世界".into(), &[key("shi"), key("jie")], Some(3))
                .unwrap();

            assert_eq!(
                store.export_phrases_in(USER_DICTIONARY).unwrap(),
                vec![
                    ExportedPhrase {
                        text: "你好".chars().map(u32::from).collect(),
                        pinyin: "ni'hao".to_owned(),
                        count: 12,
                    },
                    ExportedPhrase {
                        text: "世界".chars().map(u32::from).collect(),
                        pinyin: "shi'jie".to_owned(),
                        count: 3,
                    },
                ]
            );
        }

        const SYSTEM_A: Token = 0x0100_0001;
        const SYSTEM_B: Token = 0x0200_0001;

        /// Libraries 1 and 2, each with the one item the mixed store trains.
        fn two_system_libraries() -> SystemOriginals {
            let library = |text: u32| SystemLibrary {
                total: 100,
                range_end: 2,
                mapped: None,
                items: BTreeMap::from([(
                    1,
                    ChunkItem {
                        phrase: vec![text],
                        unigram: 100,
                        prons: Vec::new(),
                    },
                )]),
            };
            SystemOriginals::new(
                BTreeMap::from([(1, library(0x4f60)), (2, library(0x597d))]),
                UserFileLayout::stock(),
            )
        }

        /// Two user phrases and four selections over them and two system
        /// items, in a session over a user directory.
        fn mixed_store() -> (tempfile::TempDir, UserStore, Token) {
            let (dir, mut store) = in_dir(two_system_libraries());
            let user_a = store
                .add_phrase(&"你好".into(), &[key("ni"), key("hao")], None)
                .unwrap();
            let user_b = store
                .add_phrase(&"世界".into(), &[key("shi"), key("jie")], None)
                .unwrap();
            store.observe_selection(SYSTEM_A, SYSTEM_B).unwrap();
            store.observe_selection(SYSTEM_A, user_a).unwrap();
            store.observe_selection(user_a, SYSTEM_B).unwrap();
            store.observe_selection(user_a, user_b).unwrap();
            (dir, store, user_a)
        }

        #[test]
        fn mask_out_user_clear_deletes_user_entries_and_keeps_system() {
            let (_dir, mut store, user_a) = mixed_store();
            assert!(store.is_modified());
            store.save_reporting().unwrap();

            store
                .mask_out(
                    PHRASE_INDEX_LIBRARY_MASK,
                    phrase_index_make_token(USER_DICTIONARY, 0),
                )
                .unwrap();

            assert!(store.phrase(user_a).unwrap().is_none());
            assert!(store.token_for_phrase("你好").unwrap().is_none());
            assert_eq!(store.bigram_count(SYSTEM_A, SYSTEM_B).unwrap(), 69);
            assert_eq!(store.bigram_total(SYSTEM_A).unwrap(), 69);
            assert_eq!(store.bigram_count(user_a, SYSTEM_B).unwrap(), 0);
            assert_eq!(store.bigram_count(SYSTEM_A, user_a).unwrap(), 0);
            assert_eq!(store.bigram_count(user_a, user_a + 1).unwrap(), 0);
            // A system item's training lands in the item itself
            // (`phrase_index.h:628-635`, `phrase_index.cpp:150-176`): the
            // session keeps no delta for it, and the mask of the user
            // library leaves it.
            assert_eq!(store.unigram_delta(SYSTEM_B).unwrap(), 0);
            assert_eq!(store.system_unigram(SYSTEM_B), Some(100 + 966));
            assert_eq!(store.unigram_delta(user_a).unwrap(), 0);
            // `FacadePhraseIndex::mask_out` takes the user sub-index's old
            // total out and its new one in (`phrase_index.cpp:713-715`):
            // with every user item gone, nothing is left beyond the system
            // libraries' totals.
            assert_eq!(store.unigram_total().unwrap(), 0);
            // The user sub-index is empty again, so `get_range` ends at 1
            // (`phrase_index.cpp:624-646`) and the next import reuses it.
            assert_eq!(next_user_token(&store), FIRST_USER_TOKEN);
            assert!(!store.is_modified());
            assert_eq!(store.save_reporting().unwrap(), Default::default());

            store.mask_out(0x0, 0x0).unwrap();
            assert_eq!(store.bigram_count(SYSTEM_A, SYSTEM_B).unwrap(), 0);
            assert_eq!(store.bigram_total(SYSTEM_A).unwrap(), 0);
            assert_eq!(store.unigram_total().unwrap(), 0);
            assert!(!store.is_modified());
        }

        /// `_add_phrase` takes `range.m_range_end` (`pinyin.cpp:587-595`) and
        /// `get_range` drops trailing zero offsets (`phrase_index.cpp:624-646`):
        /// the next token is one past the highest live token.
        #[test]
        fn next_user_token_follows_the_highest_live_token() {
            let mut store = transient();
            let a = store
                .add_phrase(&"你好".into(), &[key("ni"), key("hao")], None)
                .unwrap();
            let b = store
                .add_phrase(&"世界".into(), &[key("shi"), key("jie")], None)
                .unwrap();
            let c = store
                .add_phrase(&"再见".into(), &[key("zai"), key("jian")], None)
                .unwrap();
            assert_eq!((b, c), (a + 1, a + 2));

            // A hole below the top is not reused.
            store.mask_out(0xFFFF_FFFF, b).unwrap();
            assert_eq!(next_user_token(&store), c + 1);

            // Masking the top frees it and every empty slot beneath it.
            store.mask_out(0xFFFF_FFFF, c).unwrap();
            assert_eq!(next_user_token(&store), a + 1);
            assert_eq!(
                store
                    .add_phrase(&"好的".into(), &[key("hao"), key("de")], None)
                    .unwrap(),
                a + 1
            );

            // A single removal frees the top the same way.
            assert!(store.remove_user_phrase(a + 1).unwrap());
            assert_eq!(next_user_token(&store), a + 1);
            assert_eq!(
                store
                    .add_phrase(&"再来".into(), &[key("zai"), key("lai")], None)
                    .unwrap(),
                a + 1
            );
        }

        #[test]
        fn remove_user_phrase_deletes_everywhere_and_rejects_others() {
            let (_dir, mut store, user_a) = mixed_store();
            store.save_reporting().unwrap();

            assert!(store.remove_user_phrase(user_a).unwrap());
            assert!(store.phrase(user_a).unwrap().is_none());
            assert!(store.token_for_phrase("你好").unwrap().is_none());
            assert_eq!(store.bigram_count(user_a, SYSTEM_B).unwrap(), 0);
            assert_eq!(store.bigram_count(SYSTEM_A, user_a).unwrap(), 0);
            assert_eq!(store.bigram_count(user_a, user_a + 1).unwrap(), 0);
            assert_eq!(store.bigram_count(SYSTEM_A, SYSTEM_B).unwrap(), 69);
            assert_eq!(store.bigram_total(SYSTEM_A).unwrap(), 69);
            assert_eq!(store.bigram_total(user_a).unwrap(), 0);
            assert_eq!(store.unigram_delta(user_a).unwrap(), 0);
            // `remove_phrase_item` takes the removed item's unigram out of
            // the facade total (`phrase_index.h:682-692`); the other user
            // item keeps its `count * 3` and its one selection.
            assert_eq!(store.unigram_total().unwrap(), 15 + 483);
            // `pinyin_remove_user_candidate` (`pinyin.cpp:3726-3768`) sets
            // no `m_modified`.
            assert!(!store.is_modified());
            assert!(!store.remove_user_phrase(user_a).unwrap());
            assert!(!store.remove_user_phrase(SYSTEM_A).unwrap());
        }

        #[test]
        fn bigram_removals_subtract_stored_totals_with_u32_wrap() {
            for remove_phrase in [false, true] {
                for (total, counts, removed, expected) in [
                    (1_u32, [u32::MAX, 1], 1_u32, 0_u32),
                    (0, [2, 3], 1, u32::MAX),
                    (17, [2, 3], 4, 13),
                    (17, [2, 3], 0, 17),
                    (1, [0, 0], 1, 0),
                    (1, [2, 3], u32::MAX, 2),
                    (9, [2, 3], 4, 5),
                ] {
                    let mut store = transient();
                    let token = store
                        .add_phrase(&"你好".into(), &[key("ni"), key("hao")], None)
                        .unwrap();
                    let prev = SYSTEM_A;
                    let kept = [0x0100_0002, 0x0100_0003];
                    let gram = |total: u32, items: &[(Token, u32)]| {
                        let mut bytes = total.to_le_bytes().to_vec();
                        for (cur, count) in items {
                            bytes.extend_from_slice(&cur.to_le_bytes());
                            bytes.extend_from_slice(&count.to_le_bytes());
                        }
                        bytes
                    };
                    let planted = [
                        (
                            prev,
                            gram(
                                total,
                                &[(kept[0], counts[0]), (kept[1], counts[1]), (token, removed)],
                            ),
                        ),
                        // An independent untouched total, plus a selected
                        // predecessor (also with reserved token bits) and
                        // a gram whose last zero-frequency item is removed.
                        (2, gram(17, &[(kept[0], 2)])),
                        (token, gram(4, &[(kept[0], 4)])),
                        (token | 0x8000_0000, gram(4, &[(kept[0], 4)])),
                        (3, gram(0, &[(token, 0)])),
                    ];
                    for (p, value) in &planted {
                        store.plant_raw_bigram(*p, value);
                    }
                    if remove_phrase {
                        assert!(store.remove_user_phrase(token).unwrap());
                    } else {
                        store
                            .mask_out(PHRASE_INDEX_LIBRARY_MASK | PHRASE_MASK, token)
                            .unwrap();
                    }
                    assert_eq!(store.bigram_total(prev).unwrap(), u64::from(expected));
                    assert_eq!(
                        store.bigram_successors(prev).unwrap(),
                        kept.into_iter()
                            .zip(counts.map(u64::from))
                            .collect::<Vec<_>>()
                    );
                    // The raw gram is the masked bytes, total wrapped.
                    assert_eq!(
                        store.bigram_row(prev).unwrap(),
                        Some(gram(
                            expected,
                            &[(kept[0], counts[0]), (kept[1], counts[1])]
                        ))
                    );
                    assert_eq!(store.bigram_total(2).unwrap(), 17);
                    for removed_prev in [token, token | 0x8000_0000, 3] {
                        assert!(store.bigram_row(removed_prev).unwrap().is_none());
                    }
                }
            }
        }

        /// `_add_phrase` takes `get_range`'s end as the new token with no
        /// check (`pinyin.cpp:589-594` at 074a2219), so the last slot is
        /// allocated like any other. The end past it, `PHRASE_MASK + 1`, is
        /// masked into the nibble by `FacadePhraseIndex::get_range`
        /// (`phrase_index.cpp:620`, `novel_types.h:45-46`) and bumped past
        /// the reserved id (`pinyin.cpp:592-594`): every later add lands on
        /// slot 1, where `add_phrase_item` overwrites the offset word and
        /// adds the new unigram to a total that keeps the overwritten one
        /// (`phrase_index.cpp:208-217`). Register row 119. The planted item
        /// takes the arena's sparse offset map, not the pin's 64 MiB array.
        #[test]
        fn add_phrase_fills_the_last_slot_then_wraps_to_slot_1() {
            let mut store = transient();
            plant_item(&store, USER_DICTIONARY, PHRASE_MASK - 1);
            let last = phrase_index_make_token(USER_DICTIONARY, PHRASE_MASK);
            assert_eq!(next_user_token(&store), last);

            let token = store
                .add_phrase(&"你好".into(), &[key("ni"), key("hao")], None)
                .unwrap();
            assert_eq!(token, last, "the last slot is allocated like any other");
            assert_eq!(store.phrases().unwrap().len(), 2);
            assert_eq!(
                store.user_library_token_end(USER_DICTIONARY).unwrap(),
                phrase_index_make_token(USER_DICTIONARY, 0),
                "the facade range end is masked into the nibble"
            );
            assert_eq!(next_user_token(&store), FIRST_USER_TOKEN);

            let wrapped = store
                .add_phrase(&"世界".into(), &[key("shi"), key("jie")], None)
                .unwrap();
            assert_eq!(
                wrapped, FIRST_USER_TOKEN,
                "the add past the last slot wraps"
            );
            assert_eq!(store.phrases().unwrap().len(), 3);
            assert_eq!(store.unigram_library_total(USER_DICTIONARY).unwrap(), 30);

            // The range end stays past the last slot, so the next add lands
            // on slot 1 again: the offset word moves to the new item and the
            // overwritten item's unigram stays in `m_total_freq`.
            let again = store
                .add_phrase(&"再见".into(), &[key("zai"), key("jian")], None)
                .unwrap();
            assert_eq!(again, FIRST_USER_TOKEN);
            assert_eq!(store.phrases().unwrap().len(), 3);
            assert_eq!(
                store
                    .phrase(FIRST_USER_TOKEN)
                    .unwrap()
                    .unwrap()
                    .phrase_text()
                    .unwrap(),
                "再见"
            );
            assert_eq!(store.unigram_delta(FIRST_USER_TOKEN).unwrap(), 15);
            assert_eq!(store.unigram_library_total(USER_DICTIONARY).unwrap(), 45);
            assert_eq!(store.unigram_total().unwrap(), 45);
        }

        /// The promotion takes `ADDON_DICTIONARY`'s `get_range` end as is,
        /// "assume not over flow here" (`pinyin.cpp:2541-2545`): the last
        /// slot is allocated, and the end past it is masked to slot 0
        /// (`phrase_index.cpp:620`) with no reserved-id bump, so every later
        /// promotion lands on slot 0 and `add_phrase_item` overwrites it
        /// (`phrase_index.cpp:208-217`). Register row 119.
        #[test]
        fn promote_addon_phrase_fills_the_last_slot_then_wraps_to_slot_0() {
            let mut store = transient();
            plant_item(&store, ADDON_DICTIONARY, PHRASE_MASK - 1);
            let readings = [(vec![key("er"), key("huang")], 100)];
            let last = store.promote_addon_phrase("二簧", &readings, 100).unwrap();
            assert_eq!(last, phrase_index_make_token(ADDON_DICTIONARY, PHRASE_MASK));
            assert_eq!(
                store.user_library_token_end(ADDON_DICTIONARY).unwrap(),
                phrase_index_make_token(ADDON_DICTIONARY, 0)
            );

            let wrapped = store.promote_addon_phrase("二簧", &readings, 100).unwrap();
            assert_eq!(
                wrapped,
                phrase_index_make_token(ADDON_DICTIONARY, 0),
                "no `token++` on this path"
            );
            assert_eq!(store.unigram_delta(wrapped).unwrap(), 100);
            assert_eq!(store.unigram_library_total(ADDON_DICTIONARY).unwrap(), 200);

            let again = store.promote_addon_phrase("二簧", &readings, 100).unwrap();
            assert_eq!(again, wrapped);
            assert_eq!(store.unigram_delta(again).unwrap(), 100);
            assert_eq!(
                store.unigram_library_total(ADDON_DICTIONARY).unwrap(),
                300,
                "the overwritten unigram stays in the total"
            );
            assert_eq!(store.unigram_total().unwrap(), 300);
        }

        #[test]
        fn promote_addon_phrase_allocates_nibble_5_and_copies_frequency() {
            let mut store = transient();
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
            assert_eq!(phrase.phrase_text().unwrap(), "二簧");
            assert_eq!(phrase.pronunciations().len(), 1);
            assert_eq!(phrase.pronunciations()[0].keys(), keys);
            assert_eq!(phrase.pronunciations()[0].count(), 100);

            // A second promotion searches nothing: it copies the item to
            // the range end again (`pinyin.cpp:2543-2558`), so the facade
            // total takes its unigram a second time and the first item
            // keeps its reading count.
            let again = store
                .promote_addon_phrase("二簧", &[(keys.to_vec(), 100)], 100)
                .unwrap();
            assert_eq!(again, token + 1, "re-promotion takes the next range end");
            assert_eq!(store.unigram_delta(token).unwrap(), 100);
            assert_eq!(store.unigram_delta(again).unwrap(), 100);
            assert_eq!(store.unigram_total().unwrap(), 200);
            assert_eq!(
                store.phrase(token).unwrap().unwrap().pronunciations()[0].count(),
                100
            );
            assert_eq!(
                store.phrase(again).unwrap().unwrap().pronunciations()[0].count(),
                100
            );
        }

        #[test]
        fn promote_addon_phrase_rejects_a_reading_of_the_wrong_length() {
            let mut store = transient();
            let err = store
                .promote_addon_phrase("二簧", &[(vec![key("er")], 100)], 100)
                .unwrap_err();
            assert!(matches!(err, UserStoreError::InvalidPhrase));
            assert!(!store.has_user_data());
        }
    }

    // ── Cross-backend equivalence (one peer per build, all peers in CI) ──

    #[test]
    fn reporting_save_guards_preserve_unigram_overflow() {
        use crate::persistence::{SystemLibrary, SystemOriginals, UserConfLaw};
        use oxpinyin_data::user_files::SystemVersions;
        fn originals() -> SystemOriginals {
            BTreeMap::from([(
                1,
                SystemLibrary {
                    total: 100,
                    range_end: 2,
                    mapped: None,
                    items: BTreeMap::from([(
                        1,
                        oxpinyin_data::chunk_write::ChunkItem {
                            phrase: vec![0x4f60],
                            unigram: 100,
                            prons: Vec::new(),
                        },
                    )]),
                },
            )])
            .into()
        }
        fn versions() -> SystemVersions {
            SystemVersions::for_this_build(7, 14)
        }
        for has_dir in [false, true] {
            for entry in 0..3 {
                for law in [UserConfLaw::Pinyin, UserConfLaw::Zhuyin] {
                    let scratch = tempfile::tempdir().expect("scratch");
                    let dir = scratch.path();
                    let mut store = if has_dir {
                        UserStore::open_libpinyin(dir, originals(), versions(), law)
                    } else {
                        UserStore::open_transient(originals(), versions(), law)
                    }
                    .expect("open");
                    // A sub-index total of MAX-4, independent of its one
                    // item's frequency 100. Compact must not erase the
                    // difference on either rejected save path.
                    store
                        .inner
                        .system_items
                        .lock()
                        .unwrap()
                        .set_library_total(1, u32::MAX - 4);
                    if !has_dir {
                        store.mark_modified();
                    }
                    let token = 0x0100_0001;
                    assert!(
                        store
                            .add_system_unigram_frequency(token, 2)
                            .expect("near bound")
                    );
                    let before = (
                        store.system_unigram(token),
                        store.unigram_total().unwrap(),
                        store.system_library_total(1).unwrap(),
                        store.is_modified(),
                    );
                    let report = match entry {
                        0 => store.save_reporting(),
                        1 => store.save_reporting_compacted(&[]),
                        _ => store
                            .save_reporting_with_baseline(&[], |_, _| panic!("guarded baseline")),
                    }
                    .expect("guarded save");
                    assert_eq!(report, crate::SaveReport::default());
                    assert_eq!(
                        (
                            store.system_unigram(token),
                            store.unigram_total().unwrap(),
                            store.system_library_total(1).unwrap(),
                            store.is_modified()
                        ),
                        before
                    );
                    assert!(
                        !store
                            .add_system_unigram_frequency(token, 4)
                            .expect("refusal")
                    );
                    assert_eq!(store.system_unigram(token), Some(102));
                    assert_eq!(
                        (store.unigram_total().unwrap() as u32)
                            .wrapping_add(store.system_library_total(1).unwrap()),
                        1
                    );
                    drop(store);
                }
            }
        }
    }

    #[test]
    fn repeated_increments_move_the_live_unigram_and_total() {
        use crate::persistence::{SystemLibrary, SystemOriginals, UserConfLaw};
        use oxpinyin_data::user_files::SystemVersions;
        let item = oxpinyin_data::chunk_write::ChunkItem {
            phrase: vec![u32::from('你')],
            unigram: 0,
            prons: Vec::new(),
        };
        let originals = || -> SystemOriginals {
            BTreeMap::from([(
                1,
                SystemLibrary {
                    total: 0,
                    range_end: 2,
                    mapped: None,
                    items: BTreeMap::from([(1, item.clone())]),
                },
            )])
            .into()
        };
        let mut store = UserStore::open_transient(
            originals(),
            SystemVersions::for_this_build(7, 14),
            UserConfLaw::Pinyin,
        )
        .unwrap();
        let token = 0x0100_0001;
        let mut last = 0;
        for _ in 0..2000 {
            assert!(store.add_system_unigram_frequency(token, 1).unwrap());
            let live = u64::from(
                store
                    .count_delta(None, token)
                    .unwrap()
                    .unigram_live
                    .unwrap(),
            );
            assert_eq!(live, last + 1);
            last = live;
        }
        assert_eq!(store.system_unigram(token), Some(2000));
        assert_eq!(store.system_library_total(1), Some(2000));
        assert_eq!(store.unigram_total().unwrap(), 0);
        // Readers of one store share it by `&`; the writer holds `&mut`, so the
        // two never overlap. Several readers see the same live state.
        let shared = &store;
        std::thread::scope(|scope| {
            for _ in 0..4 {
                scope.spawn(move || {
                    for _ in 0..500 {
                        let counts = shared.count_delta(None, token).unwrap();
                        assert_eq!(counts.unigram_live, Some(2000));
                        assert_eq!(shared.system_library_total(1), Some(2000));
                    }
                });
            }
        });
    }

    /// Only the checked-read failure is the pin's assert; every other
    /// store or I/O failure is an ordinary failed read.
    #[test]
    fn only_a_read_after_check_is_the_checked_read_failure() {
        assert!(UserStoreError::Store(StoreError::ReadAfterCheck).is_read_after_check());
        assert!(!UserStoreError::Store(StoreError::Backend("x".into())).is_read_after_check());
        assert!(!UserStoreError::Io(std::io::Error::other("x")).is_read_after_check());
        assert!(!UserStoreError::Decode.is_read_after_check());
    }

    /// The gram memo, the one a session's `count_delta` runs on: a present
    /// gram is kept until the generation moves, an absent one reads as
    /// `None` without entering the memo, and the map resets at the cap.
    #[test]
    fn bigram_memo_resets_at_the_capacity_bound() {
        let mut cache = CountCache::new(0);
        let gram = |prev: Token| {
            let mut bytes = prev.to_le_bytes().to_vec();
            bytes.extend_from_slice(&[0; 4]);
            bytes
        };
        assert_eq!(cache.bigram_row(7, || Ok(None)).unwrap(), None);
        assert!(
            !cache.bigram_rows.contains_key(&7),
            "an absent gram must not occupy memo space"
        );
        for prev in 1..=u32::try_from(COUNT_MEMO_MAX_ENTRIES + 10).unwrap() {
            assert_eq!(
                cache.bigram_row(prev, || Ok(Some(gram(prev)))).unwrap(),
                Some(gram(prev).as_slice())
            );
        }
        assert!(
            cache.bigram_rows.len() <= COUNT_MEMO_MAX_ENTRIES,
            "memo must not exceed the cap"
        );
        // A memoised gram is served without a load; a reset one loads again.
        let served = cache
            .bigram_row(u32::try_from(COUNT_MEMO_MAX_ENTRIES + 10).unwrap(), || {
                panic!("the freshest gram is memoised")
            })
            .unwrap()
            .map(<[u8]>::to_vec);
        assert_eq!(
            served,
            Some(gram(u32::try_from(COUNT_MEMO_MAX_ENTRIES + 10).unwrap()))
        );
        assert_eq!(
            cache.bigram_row(1, || Ok(Some(gram(1)))).unwrap(),
            Some(gram(1).as_slice())
        );
    }
}
