//! Ordered-store-backed integer store for user-learning counts and the user
//! phrase index.
//!
//! Models the *values* libpinyin records for phrase-index unigram deltas
//! and user-phrase text/pronunciations — not their `MemoryChunk` / DBM byte
//! layout (`docs/findings/user-store.md` §4, §10); those counts are `u64`
//! integers. The user bigram is the exception: it is held once, as the
//! pin's own `SingleGram` values (`docs/findings/user-store.md` §5), in the
//! pin's container on a libpinyin session and in the ordered
//! [`BIGRAM_GRAM`] table otherwise, and every bigram operation runs the
//! pin's `SingleGram` code over those bytes
//! ([`oxpinyin_core::single_gram`]).
//!
//! T1: count schema and seed-driven update. T2: user phrase-index tables and
//! `USER_DICTIONARY` token allocation. T3 wires the store into the engine
//! session and the C ABI (in `oxpinyin-engine` / `oxpinyin-capi`). T4 exposes
//! the counts as a [`oxpinyin_core::UserCountDelta`] for the decode-time
//! additive merge. T5 adds the save cycle: the §4 `m_modified` gate and the
//! persistence point behind `pinyin_save`.

use std::collections::HashMap;
use std::fmt;
use std::ops::Bound;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use oxpinyin_core::hash::FixedHashMap;
use oxpinyin_core::{ChewingKey, Completeness, SyllableKey, UserCountDelta, single_gram};
use oxpinyin_data::user_files::{LogRecord, decode_log_records_partial, read_chunk_payload};
use oxpinyin_store::{
    DefaultStore, DefaultUserBigramDb, ReadStore, StoreError, UserBigramDb, WriteStore, WriteTxn,
};

use crate::codec;
use crate::persistence::ChunkHeaderField;
use crate::phrase::{
    self, ADD_PHRASE_UNIGRAM_FACTOR, ADDON_DICTIONARY, DEFAULT_PHRASE_COUNT, FIRST_USER_TOKEN,
    NETWORK_DICTIONARY, PHRASE_INDEX_LIBRARY_MASK, PHRASE_MASK, PinyinKey, USER_DICTIONARY,
    UserPhrase, UserPronunciation, first_library_token, is_user_file_library,
    phrase_index_library_index,
};
use crate::phrase_table;
use crate::registry::{self, CountCache, RegistryLease, StandaloneLease, StoreInner};
use crate::seed;
use crate::session_values::SessionValues;
use crate::store_libpinyin::{FiniGuard, IndexRows};

/// Token type — libpinyin's 32-bit `phrase_token_t`.
pub type Token = u32;

/// What a save left undone, in the pin's own terms.
///
/// The pin's `pinyin_save` and `zhuyin_save` never stop at a failure: a
/// write that fails leaves a missing `.tmp`, the rename of it fails and is
/// printed (`rename %s to %s failed.`, `pinyin.cpp:1061`…`:1123`), the
/// marker write fails and is printed (`write %s failed.`,
/// `table_info.cpp:382`), and the save answers `true`
/// (`pinyin.cpp:1132-1147`). [`GenericUserStore::save_reporting`] returns
/// those failures instead of printing them, so the caller can.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SaveReport {
    /// `(tmp, final)` of every rename that failed, in the pin's order: the
    /// libraries by index, then the pinyin index, the phrase index and the
    /// bigram.
    pub renames_failed: Vec<(PathBuf, PathBuf)>,
    /// The `user.conf` whose write failed, if it did.
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

/// The user bigram of a plain or standalone store: the raw `SingleGram`
/// value of each predecessor, keyed by its big-endian token so the table
/// walks predecessors in ascending order. A libpinyin session keeps its
/// grams in the pin's own container instead ([`StoreInner::bigram_db`]),
/// keyed and walked the pin's way, and never writes this table.
pub const BIGRAM_GRAM: &str = "user_bigram_gram";
pub(crate) const SYSTEM_BASE: &str = "system_base";
/// Per library, how far the total the pin's sub-index holds lies from the
/// one the seeded rows make, a wrapping `guint32` keyed by nibble: the
/// difference a stored header or a `.dbin` replay left, until the next
/// `compact` rebuilds the library's total from its items. A row exists
/// only for a non-zero difference.
pub(crate) const LIBRARY_OFFSET: &str = "library_offset";
pub const UNIGRAM: &str = "user_unigram";
pub const UNIGRAM_TOTAL: &str = "user_unigram_total";
pub const PHRASE: &str = "user_phrase";
pub const PHRASE_BY_TEXT: &str = "user_phrase_by_text";
pub const PHRASE_BY_LIB_TEXT: &str = "user_phrase_by_lib_text";
pub const PRONUNCIATION: &str = "user_pronunciation";
pub const ALLOC: &str = "user_phrase_alloc";

/// Sole key in the `user_unigram_total` table.
pub const UNIGRAM_TOTAL_KEY: u8 = 0;

/// Sole key in the `user_phrase_alloc` table.
pub const ALLOC_CURSOR: u8 = 0;

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
    /// A present phrase's stored pronunciation row could not be decoded
    /// (corrupt or incompatible): the item exists, one of its readings does
    /// not read back.
    DecodePronunciation,
    /// A standalone store at this path is already live in this process.
    AlreadyOpen,
    /// Phrase text is empty, too long, or its key count does not match its
    /// Unicode scalar length (`docs/findings/user-store.md` §3.1–3.2).
    InvalidPhrase,
    /// No remaining token in the [`crate::USER_DICTIONARY`] 24-bit id space.
    TokenSpaceExhausted,
    /// The libpinyin user-dir persistence failed (I/O, container, or a
    /// byte stream that does not parse).
    Persistence(String),
    /// `user.conf` names a database format upstream's mapper does not
    /// know — the point `to_table_database_format_type` `abort()`s on
    /// (`table_info.cpp:122-133`). The class-(c) answer: the store does
    /// not open, and nothing is cleaned or written.
    UnknownDatabaseFormat,
    /// A library chunk's `MemoryChunk::save` header write failed
    /// (`memory_chunk.h:543`/`:547`) — the pin `assert`s and dies of
    /// SIGABRT. The class-(c) answer: the save fails, and the facade logs
    /// the point in its own domain.
    ChunkHeaderWrite(ChunkHeaderField),
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
    /// (`chewing_large_table2_bdb.cpp:529`). The class-(c) answer: the
    /// mask fails, and the facade logs the point in its own domain.
    OverlongIndexKey,
    /// A `user_bigram.db` key is not a four-byte `phrase_token_t`:
    /// `Bigram::get_all_items` asserts `key.size == sizeof(phrase_token_t)`
    /// (`ngram_bdb.cpp:199`), and the pin's `mask_out` and
    /// `pinyin_begin_get_bigram_phrases` both drive that walk. The
    /// class-(c) answer: the walk fails.
    NonTokenUserBigramKey,
    /// A `user_bigram.db` value is shorter than a `guint32`, or has an
    /// incomplete eight-byte item after that total. The pin asserts on
    /// the missing total (`memory_chunk.h:390`) and reads past a partial
    /// item in `mask_out` (`ngram.cpp:76-86`). The mask and removal
    /// facades report either cause through one malformed-value warning.
    ShortUserBigramValue,
    /// A `user_bigram.db` row decodes to a gram with no items and a
    /// nonzero `total_freq`: `SingleGram::get_length` asserts
    /// `0 == total_freq` (`ngram.cpp:70`), reached by
    /// `pinyin_guess_predicted_candidates` through
    /// `_compute_predicted_bigram_candidates` (`pinyin.cpp:2332`).
    EmptyUserBigramGram,
    /// A `user_bigram.db` gram's `total_freq` is not covered by its items
    /// and a mask removes every item, leaving a residual total:
    /// `Bigram::mask_out` then asks `SingleGram::get_length`, whose assert
    /// dies (`ngram.cpp:70`, `ngram_bdb.cpp:243`). The class-(c) answer:
    /// the mask fails.
    ResidualUserBigramGram,
    /// A `SYSTEM_FILE`/`DICTIONARY` library's user `.dbin` holds a
    /// `MODIFY_HEADER` record whose token is not `null_token`:
    /// `PhraseIndexLogger::next_record` asserts `token == null_token`
    /// (`phrase_index_logger.h:202`). Reached at init (the library load's
    /// `merge`) and at `mask_out` (`merge_with_mask`). The class-(c)
    /// answer: the load/mask fails, and the facade logs one warning in its
    /// own domain.
    SystemLogHeaderToken,
    /// A `SYSTEM_FILE`/`DICTIONARY` library's user `.dbin` holds more than
    /// one `MODIFY_HEADER` record: `_peek_header` asserts
    /// `1 >= header_count` (`phrase_index.cpp:745`), reached only by
    /// `mask_out`'s `merge_with_mask` (init's `merge` never peeks). The
    /// class-(c) answer: the mask fails.
    SystemLogMultipleHeaders,
    /// A surviving ADD record in a masked system log is re-appended with
    /// a non-null oldchunk pointer, violating `NULL == oldone`
    /// (074a2219 phrase_index_logger.h:245, phrase_index.cpp:839).
    SystemLogSurvivingAdd,
    /// A training step's `set_freq` misses the item its `insert_freq` just
    /// added: on an unsorted gram the second `lower_bound` can land past
    /// it, and the pin's `check_result(user->set_freq(token, freq + seed))`
    /// dies (`phonetic_lookup.h:903`, `ngram.cpp:252-271`). The class-(c)
    /// answer: the training refuses before anything is stored.
    TrainingSetFreqMiss,
}

impl fmt::Display for UserStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Store(e) => write!(f, "store error: {e}"),
            Self::Decode => write!(f, "stored value could not be decoded"),
            Self::DecodePronunciation => {
                write!(f, "stored pronunciation could not be decoded")
            }
            Self::AlreadyOpen => write!(f, "standalone user store is already open"),
            Self::InvalidPhrase => {
                write!(f, "invalid phrase (empty, too long, or key count mismatch)")
            }
            Self::TokenSpaceExhausted => {
                write!(f, "USER_DICTIONARY token space exhausted")
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
                "malformed user bigram value (upstream asserts on a missing total or reads \
                 past a partial item; memory_chunk.h:390, ngram.cpp:76-86)"
            ),
            Self::EmptyUserBigramGram => write!(
                f,
                "user bigram gram has no items but a nonzero total_freq (upstream aborts, \
                 ngram.cpp:70)"
            ),
            Self::ResidualUserBigramGram => write!(
                f,
                "masking a user bigram leaves a residual total_freq (upstream aborts, ngram.cpp:70)"
            ),
            Self::SystemLogHeaderToken => write!(
                f,
                "a system library's user .dbin carries a non-null-token MODIFY_HEADER \
                 (upstream asserts, phrase_index_logger.h:202)"
            ),
            Self::SystemLogMultipleHeaders => write!(
                f,
                "a system library's user .dbin carries more than one MODIFY_HEADER \
                 (upstream asserts, phrase_index.cpp:745)"
            ),
            Self::SystemLogSurvivingAdd => write!(
                f,
                "a surviving system log ADD violates NULL == oldone \
                 (upstream asserts, phrase_index_logger.h:245)"
            ),
            Self::TrainingSetFreqMiss => write!(
                f,
                "a training step's set_freq misses the item insert_freq added \
                 (upstream asserts, phonetic_lookup.h:903)"
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
            | Self::DecodePronunciation
            | Self::AlreadyOpen
            | Self::InvalidPhrase
            | Self::TokenSpaceExhausted
            | Self::Persistence(_)
            | Self::UnknownDatabaseFormat
            | Self::ChunkHeaderWrite(_)
            | Self::UnigramTotalOverflow
            | Self::OverlongIndexKey
            | Self::NonTokenUserBigramKey
            | Self::ShortUserBigramValue
            | Self::EmptyUserBigramGram
            | Self::ResidualUserBigramGram
            | Self::SystemLogHeaderToken
            | Self::SystemLogMultipleHeaders
            | Self::SystemLogSurvivingAdd
            | Self::TrainingSetFreqMiss => None,
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

/// The user pinyin index projection of `syllables` — the DB key
/// `add_index` would write for this reading: tone-zeroed complete keys, or
/// initial-only keys when any syllable is incomplete
/// (`compute_chewing_index` / `compute_incomplete_chewing_index`,
/// `src/storage/pinyin_phrase3.h:160-178` at the pin). `None` when a
/// syllable names no `ChewingKey`, which is never a key upstream writes.
fn index_projection(syllables: &[SyllableKey]) -> Option<Vec<u16>> {
    let incomplete = syllables
        .iter()
        .any(|key| key.completeness() == Completeness::Partial);
    syllables
        .iter()
        .map(|key| {
            let full = ChewingKey::from_pinyin(key.text())?;
            if incomplete {
                // `compute_incomplete_chewing_index` sets only `m_initial`,
                // so a vowel-initial syllable and `ng` project to the zero
                // initial (`ChewingKey::new(0, 0, 0, 0)`), not to a missing
                // key. Building the packed word directly keeps those zeros
                // representable.
                Some(ChewingKey::new(full.initial, 0, 0, 0).to_packed())
            } else {
                Some(full.with_tone(0).to_packed())
            }
        })
        .collect()
}

// ── codec helpers ─────────────────────────────────────────────────

fn get_u64(store: &impl ReadStore, table: &str, key: &[u8]) -> Result<Option<u64>, UserStoreError> {
    store.get(table, key)?.map_or_else(
        || Ok(None),
        |bytes| {
            codec::decode_u64(&bytes)
                .map(Some)
                .map_err(|_| UserStoreError::Decode)
        },
    )
}

fn get_u64_or(
    store: &impl ReadStore,
    table: &str,
    key: &[u8],
    default: u64,
) -> Result<u64, UserStoreError> {
    Ok(get_u64(store, table, key)?.unwrap_or(default))
}

fn txn_get_u64(txn: &impl WriteTxn, table: &str, key: &[u8]) -> Result<Option<u64>, StoreError> {
    txn.get(table, key)?.map_or_else(
        || Ok(None),
        |bytes| {
            codec::decode_u64(&bytes)
                .map(Some)
                .map_err(|_| StoreError::Backend("corrupt u64 value".into()))
        },
    )
}

pub fn txn_get_u64_or(
    txn: &impl WriteTxn,
    table: &str,
    key: &[u8],
    default: u64,
) -> Result<u64, StoreError> {
    Ok(txn_get_u64(txn, table, key)?.unwrap_or(default))
}

fn bump_unigram_total(txn: &mut impl WriteTxn, delta: u64) -> Result<(), StoreError> {
    let key = codec::encode_u8(UNIGRAM_TOTAL_KEY);
    let prev = txn_get_u64_or(txn, UNIGRAM_TOTAL, &key, 0)?;
    txn.put(
        UNIGRAM_TOTAL,
        &key,
        &codec::encode_u64(prev.saturating_add(delta)),
    )?;
    Ok(())
}

/// A `PRONUNCIATION` row's value: the reading's count, its insertion
/// sequence inside the phrase, and whether the user pinyin index carries
/// it (`count` in [`codec::encode_u64`]'s layout, then `seq: u32 BE`,
/// then `indexed: u8`).
///
/// libpinyin keeps a phrase's pronunciations in the order they were
/// appended (`PhraseItem::add_pronunciation`, `phrase_index.cpp:86-91`)
/// and indexes only the reading a phrase was created with
/// (`pinyin.cpp:569-600`); the row key `(token, keys)` carries neither, so
/// the value does. A bare 8-byte count — a row written before this
/// layout — reads as indexed, sequenced after every sequenced row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PronValue {
    pub(crate) count: u64,
    pub(crate) seq: u32,
    pub(crate) indexed: bool,
}

impl PronValue {
    const LEN: usize = 13;

    pub(crate) fn encode(self) -> [u8; Self::LEN] {
        let mut out = [0_u8; Self::LEN];
        out[..8].copy_from_slice(&codec::encode_u64(self.count));
        out[8..12].copy_from_slice(&self.seq.to_be_bytes());
        out[12] = u8::from(self.indexed);
        out
    }

    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, StoreError> {
        let corrupt = || StoreError::Backend("corrupt pronunciation value".into());
        match bytes.len() {
            8 => Ok(Self {
                count: codec::decode_u64(bytes).map_err(|_| corrupt())?,
                seq: u32::MAX,
                indexed: true,
            }),
            Self::LEN => {
                let count = codec::decode_u64(&bytes[..8]).map_err(|_| corrupt())?;
                let mut seq = [0_u8; 4];
                seq.copy_from_slice(&bytes[8..12]);
                Ok(Self {
                    count,
                    seq: u32::from_be_bytes(seq),
                    indexed: bytes[12] != 0,
                })
            }
            _ => Err(corrupt()),
        }
    }
}

/// One stored reading of a token, as the write paths walk them.
pub(crate) struct PronRow {
    pub(crate) key_bytes: Vec<u8>,
    pub(crate) value: PronValue,
}

/// One import handle's last item, in the pin's stored reading order. The
/// database remains authoritative: every write still commits there, and any
/// write through another handle invalidates this bounded, one-item memo.
struct ImportReadings {
    token: Token,
    generation: u64,
    rows: Vec<PronRow>,
}

#[inline]
fn discard_stale_import_readings(cache: &mut Option<ImportReadings>, generation: u64) {
    if cache
        .as_ref()
        .is_some_and(|cached| cached.generation != generation)
    {
        *cache = None;
    }
}

#[inline]
fn note_import_commit(cache: &mut Option<ImportReadings>, generation: u64) {
    if let Some(cached) = cache.as_mut() {
        cached.generation = generation.wrapping_add(1);
    }
}

/// Puts `rows` into insertion order: ascending sequence, the table's key
/// order among equal sequences (the stable sort keeps it).
fn sort_by_insertion<T>(rows: &mut [T], seq: impl Fn(&T) -> u32) {
    rows.sort_by_key(|row| seq(row));
}

/// [`ADD_PHRASE_UNIGRAM_FACTOR`] as `_add_phrase`'s `guint32`
/// `unigram_factor` (`pinyin.cpp:522`).
const ADD_PHRASE_UNIGRAM_FACTOR_U32: u32 = 3;
const _: () = assert!(ADD_PHRASE_UNIGRAM_FACTOR_U32 as u64 == ADD_PHRASE_UNIGRAM_FACTOR);

/// One imported reading: its dense keys, the same as packed `ChewingKey`
/// words, and the count it adds.
struct SystemReading<'a> {
    keys: &'a [PinyinKey],
    packed: &'a [u16],
    delta: u32,
}

/// The token `_add_phrase` allocates for a phrase new to a system library
/// (`get_range`'s end, pinyin.cpp:587-595; seeded from the shipped range and
/// replayed ADD records, never from another library), with its text rows, its
/// first reading and its unigram.
fn append_system_token<S: WriteStore>(
    db: &S,
    library: u8,
    range_end: u32,
    text: &str,
    reading: &SystemReading<'_>,
) -> Result<Token, StoreError> {
    let lib_key = codec::encode_u8_str(library, text);
    db.write(|txn| {
        let token = match txn.get(ALLOC, &codec::encode_u8(library))? {
            Some(bytes) => codec::decode_token(&bytes)
                .map_err(|_| StoreError::Backend("corrupt system cursor".into()))?,
            None => (u32::from(library) << 24) | range_end.max(1),
        };
        let next = phrase::next_library_token_after(library, token)
            .ok_or_else(|| StoreError::Backend("system token space exhausted".into()))?;
        txn.put(
            ALLOC,
            &codec::encode_u8(library),
            &codec::encode_token(next),
        )?;
        txn.put(PHRASE, &codec::encode_token(token), codec::encode_str(text))?;
        txn.put(PHRASE_BY_LIB_TEXT, &lib_key, &codec::encode_token(token))?;
        phrase_table::add_index(txn, text, token)?;
        add_pronunciation(
            txn,
            token,
            &phrase::encode_keys(reading.keys),
            reading.delta,
            true,
        )?;
        // The two facade add_index calls write the user tables. Only
        // this first reading gets an index row (pinyin.cpp:598-607).
        add_unigram_frequency(
            txn,
            library,
            token,
            reading.delta.wrapping_mul(ADD_PHRASE_UNIGRAM_FACTOR_U32),
        )?;
        Ok(token)
    })
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

/// Keeps a system token's persistence rows in step with its live `item`. An
/// original with no delta row still uses its imported payload; an existing
/// row must include this import or the next train/save would erase it. A
/// wrapped item drops every row.
fn sync_system_rows<S: WriteStore>(
    db: &S,
    original: &crate::persistence::SystemLibrary,
    token: Token,
    current: u32,
    reading: &SystemReading<'_>,
    (untouched, wrapped): (bool, bool),
    (generation, cache): (u64, &mut Option<ImportReadings>),
) -> Result<(), StoreError> {
    if wrapped {
        return db.write(|txn| {
            let mut rows = match cache.take() {
                Some(cached) if cached.token == token && cached.generation == generation => {
                    cached.rows
                }
                _ => collect_pronunciations_from_txn(txn, token)?,
            };
            clear_pronunciations(txn, &rows, token)?;
            rows.clear();
            *cache = Some(ImportReadings {
                token,
                generation,
                rows,
            });
            Ok(())
        });
    }
    if untouched {
        return Ok(());
    }
    let has_original = original.contains_item(token & PHRASE_MASK);
    let row_keys = if has_original {
        reading.packed
    } else {
        reading.keys
    };
    let key_bytes = phrase::encode_keys(row_keys);
    let row_key = codec::encode_token_bytes(token, &key_bytes);
    if let Some(bytes) = db.get(PRONUNCIATION, &row_key)? {
        let mut value = PronValue::decode(&bytes)?;
        let base = original
            .item(token & PHRASE_MASK)
            .and_then(|item| {
                item.prons
                    .into_iter()
                    .find(|(stored, _)| stored.as_slice() == reading.packed)
            })
            .map_or(0, |(_, count)| count);
        let count = u64::from(current.wrapping_sub(base));
        if value.count != count {
            value.count = count;
            db.write(|txn| txn.put(PRONUNCIATION, &row_key, &value.encode()))?;
        }
        if let Some(cached) = cache.as_mut() {
            if let Some(row) = cached
                .rows
                .iter_mut()
                .find(|row| row.key_bytes == key_bytes)
            {
                row.value = value;
            } else {
                *cache = None;
            }
        }
    } else if !has_original {
        db.write(|txn| {
            add_import_pronunciation(
                txn,
                token,
                &phrase::encode_keys(reading.keys),
                reading.delta,
                generation,
                cache,
            )
            .map(|_| ())
        })?;
    }
    Ok(())
}

/// Drops every stored reading of `token`: the item a wrapped count leaves.
fn clear_pronunciations(
    txn: &mut impl WriteTxn,
    rows: &[PronRow],
    token: Token,
) -> Result<(), StoreError> {
    for row in rows {
        txn.remove(
            PRONUNCIATION,
            &codec::encode_token_bytes(token, &row.key_bytes),
        )?;
    }
    Ok(())
}

/// A reading's keys as the `ChewingKey`s the pin's tables address.
#[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
fn chewing_keys(keys: &[PinyinKey]) -> Option<Vec<ChewingKey>> {
    let packed = crate::store_libpinyin::pinyin_keys_to_packed(keys)?;
    Some(packed.into_iter().map(ChewingKey::from_packed).collect())
}

/// `PhraseItem::add_pronunciation(keys, delta)` (`phrase_index.cpp:56-91`)
/// on `token`'s stored readings: an exact-key match (tone included,
/// `pinyin_exact_compare2`) gains `delta` unless the running `guint32`
/// total of the readings up to and including it would overflow — then
/// nothing changes (the pin's `return false`, which `_add_phrase`
/// discards); any other key is appended with `delta` as its count.
///
/// The pin's count is a `guint8` (`set_n_pronunciation`, `:28`), so an
/// unmatched key on an item that already holds 255 readings stores a count
/// of 0 (`:88`) and the call still succeeds. Every read goes through
/// `SubPhraseIndex::get_phrase_item`, which sizes the item from that count
/// (`:203-204`), and `remove_phrase_item` copies only that view (`:228`):
/// the appended bytes are unreachable, so the item is one with no readings.
/// The unigram, the phrase rows and the next add are untouched by the wrap.
fn add_pronunciation(
    txn: &mut impl WriteTxn,
    token: Token,
    key_bytes: &[u8],
    delta: u32,
    indexed: bool,
) -> Result<usize, StoreError> {
    let mut rows = collect_pronunciations_from_txn(txn, token)?;
    add_pronunciation_to_rows(txn, token, key_bytes, delta, indexed, &mut rows)
}

fn add_import_pronunciation(
    txn: &mut impl WriteTxn,
    token: Token,
    key_bytes: &[u8],
    delta: u32,
    generation: u64,
    cache: &mut Option<ImportReadings>,
) -> Result<usize, StoreError> {
    let mut rows = match cache.take() {
        Some(cached) if cached.token == token && cached.generation == generation => cached.rows,
        _ => collect_pronunciations_from_txn(txn, token)?,
    };
    let count = add_pronunciation_to_rows(txn, token, key_bytes, delta, false, &mut rows)?;
    *cache = Some(ImportReadings {
        token,
        generation,
        rows,
    });
    Ok(count)
}

fn add_pronunciation_to_rows(
    txn: &mut impl WriteTxn,
    token: Token,
    key_bytes: &[u8],
    delta: u32,
    indexed: bool,
    rows: &mut Vec<PronRow>,
) -> Result<usize, StoreError> {
    let mut total_freq: u32 = 0;
    let count = rows.len();
    for row in rows.iter_mut() {
        // Stored counts are `guint32` fields upstream.
        let freq = u32::try_from(row.value.count).unwrap_or(u32::MAX);
        total_freq = total_freq.wrapping_add(freq);
        if row.key_bytes == key_bytes {
            if delta > 0 && total_freq.checked_add(delta).is_none() {
                return Ok(count);
            }
            let value = PronValue {
                count: u64::from(freq.wrapping_add(delta)),
                ..row.value
            };
            txn.put(
                PRONUNCIATION,
                &codec::encode_token_bytes(token, key_bytes),
                &value.encode(),
            )?;
            row.value = value;
            return Ok(count);
        }
    }
    if rows.len() == usize::from(u8::MAX) {
        clear_pronunciations(txn, rows, token)?;
        rows.clear();
        return Ok(0);
    }
    let compacted = rows.last().is_some_and(|row| row.value.seq == u32::MAX);
    let value = PronValue {
        count: u64::from(delta),
        seq: next_pron_seq(txn, token, rows)?,
        indexed,
    };
    txn.put(
        PRONUNCIATION,
        &codec::encode_token_bytes(token, key_bytes),
        &value.encode(),
    )?;
    if compacted {
        for (seq, row) in rows.iter_mut().enumerate() {
            row.value.seq = seq as u32;
        }
    }
    rows.push(PronRow {
        key_bytes: key_bytes.to_vec(),
        value,
    });
    Ok(rows.len())
}

/// `FacadePhraseIndex::add_unigram_frequency` (`phrase_index.h:628-635`)
/// for a `USER_FILE` token: the facade total always takes `delta`; the
/// sub-index refuses it — the item's unigram unchanged — when its own
/// `guint32` total would overflow (`SubPhraseIndex::add_unigram_frequency`,
/// `phrase_index.cpp:150-178`).
///
/// Answers whether the sub-index accepted the delta: `false` is
/// `ERROR_INTEGER_OVERFLOW`, which `pinyin_choose_predicted_candidate`
/// turns into `false` (`pinyin.cpp:2609-2612`) and the training walk
/// ignores (`phonetic_lookup.h:928-929`).
fn add_unigram_frequency(
    txn: &mut impl WriteTxn,
    library: u8,
    token: Token,
    delta: u32,
) -> Result<bool, StoreError> {
    bump_unigram_total(txn, u64::from(delta))?;
    if library_total_overflows(txn, library, delta)? {
        return Ok(false);
    }
    let uni_key = codec::encode_token(token);
    let prev = txn_get_u64_or(txn, UNIGRAM, &uni_key, 0)?;
    txn.put(
        UNIGRAM,
        &uni_key,
        &codec::encode_u64(prev.saturating_add(u64::from(delta))),
    )?;
    Ok(true)
}

/// Whether `library`'s `guint32` unigram total plus `delta` overflows —
/// the sub-index's `m_total_freq > m_total_freq + delta` guard. Absent an
/// offset the facade total bounds every library's sum from above, so the
/// library's rows are summed only when that bound itself could overflow.
fn library_total_overflows(
    txn: &impl WriteTxn,
    library: u8,
    delta: u32,
) -> Result<bool, StoreError> {
    if delta == 0 {
        return Ok(false);
    }
    let base = txn_get_u64_or(txn, SYSTEM_BASE, &codec::encode_u8(library), 0)?;
    // Until the next compact the sub-index still holds the stored total, not
    // its items' sum (`phrase_index.cpp:170-171`): the difference
    // [`LIBRARY_OFFSET`] keeps is part of the guarded sum, and it breaks the
    // item-sum bound below.
    let offset = txn_get_u64_or(txn, LIBRARY_OFFSET, &codec::encode_u8(library), 0)? as u32;
    let bound = txn_get_u64_or(txn, UNIGRAM_TOTAL, &codec::encode_u8(UNIGRAM_TOTAL_KEY), 0)?
        .saturating_add(base);
    if offset == 0 && bound <= u64::from(u32::MAX - delta) {
        return Ok(false);
    }
    let lo = codec::encode_token(phrase::phrase_index_make_token(library, 0));
    let hi = u32::from(library)
        .checked_add(1)
        .map(|next| codec::encode_token(next << 24));
    let mut total = (base as u32).wrapping_add(offset);
    txn.range(
        UNIGRAM,
        Bound::Included(lo.as_slice()),
        hi.as_ref()
            .map_or(Bound::Unbounded, |hi| Bound::Excluded(hi.as_slice())),
        |_key, value| {
            let unigram = codec::decode_u64(value)
                .map_err(|_| StoreError::Backend("corrupt unigram".into()))?;
            total = total.wrapping_add(unigram as u32);
            Ok(())
        },
    )?;
    Ok(total.checked_add(delta).is_none())
}

/// A reading on its way through one training call: row key, packed keys,
/// running count, the stored row, and for a live item the entry's place in it
/// with the original count of its keys.
type TrainRow = (Vec<u8>, Vec<u16>, u32, PronValue, Option<(usize, u32)>);

/// Pronunciation-range bounds for `token`: every key whose 4-byte
/// big-endian prefix is `token`, so `[token, token + 1)` over the prefix
/// alone. The bytes are exactly what `encode_token_bytes(token, &[])`
/// yields, held in fixed-width arrays rather than heap buffers.
fn pronunciation_range(token: Token) -> (Bound<[u8; 4]>, Bound<[u8; 4]>) {
    let lo = Bound::Included(codec::encode_token(token));
    let hi = token.checked_add(1).map_or(Bound::Unbounded, |next| {
        Bound::Excluded(codec::encode_token(next))
    });
    (lo, hi)
}

/// `token`'s readings in insertion order. A row that does not decode is
/// [`UserStoreError::DecodePronunciation`], told apart from the backend's
/// own failure so a reader can treat the item as present but malformed.
fn collect_pronunciations_from_store(
    store: &impl ReadStore,
    token: Token,
) -> Result<Vec<UserPronunciation>, UserStoreError> {
    let (lo, hi) = pronunciation_range(token);
    let mut out = Vec::new();
    let mut undecodable = false;
    store.range(
        PRONUNCIATION,
        lo.as_ref().map(<[u8; 4]>::as_slice),
        hi.as_ref().map(<[u8; 4]>::as_slice),
        |key, value| {
            let (Ok((_, key_bytes)), Ok(value)) =
                (codec::decode_token_bytes(key), PronValue::decode(value))
            else {
                undecodable = true;
                return Ok(());
            };
            out.push((
                value.seq,
                UserPronunciation::new(phrase::decode_keys(key_bytes), value.count, value.indexed),
            ));
            Ok(())
        },
    )?;
    if undecodable {
        return Err(UserStoreError::DecodePronunciation);
    }
    sort_by_insertion(&mut out, |row| row.0);
    Ok(out
        .into_iter()
        .map(|(_, pronunciation)| pronunciation)
        .collect())
}

/// `token`'s readings inside `txn`, in insertion order.
pub(crate) fn collect_pronunciations_from_txn(
    txn: &impl WriteTxn,
    token: Token,
) -> Result<Vec<PronRow>, StoreError> {
    let (lo, hi) = pronunciation_range(token);
    let mut out = Vec::new();
    txn.range(
        PRONUNCIATION,
        lo.as_ref().map(<[u8; 4]>::as_slice),
        hi.as_ref().map(<[u8; 4]>::as_slice),
        |key, value| {
            let (_, key_bytes) = codec::decode_token_bytes(key)
                .map_err(|_| StoreError::Backend("corrupt pronunciation key".into()))?;
            out.push(PronRow {
                key_bytes: key_bytes.to_vec(),
                value: PronValue::decode(value)?,
            });
            Ok(())
        },
    )?;
    sort_by_insertion(&mut out, |row| row.value.seq);
    Ok(out)
}

/// The sequence the next reading appended to a phrase with `rows` takes.
fn next_pron_seq(
    txn: &mut impl WriteTxn,
    token: Token,
    rows: &[PronRow],
) -> Result<u32, StoreError> {
    let next = rows
        .iter()
        .map(|row| row.value.seq)
        .max()
        .map_or(Some(0), |seq| seq.checked_add(1));
    if let Some(next) = next {
        return Ok(next);
    }
    // Legacy rows use MAX as their stable, table-key-ordered tail. Compact
    // that order in the same transaction before appending a new reading.
    let next = u32::try_from(rows.len())
        .map_err(|_| StoreError::Backend("pronunciation sequence exhausted".into()))?;
    for (seq, row) in rows.iter().enumerate() {
        let value = PronValue {
            seq: u32::try_from(seq)
                .map_err(|_| StoreError::Backend("pronunciation sequence exhausted".into()))?,
            ..row.value
        };
        txn.put(
            PRONUNCIATION,
            &codec::encode_token_bytes(token, &row.key_bytes),
            &value.encode(),
        )?;
    }
    Ok(next)
}

fn remove_pronunciations(txn: &mut impl WriteTxn, token: Token) -> Result<(), StoreError> {
    let rows = collect_pronunciations_from_txn(txn, token)?;
    for row in rows {
        txn.remove(
            PRONUNCIATION,
            &codec::encode_token_bytes(token, &row.key_bytes),
        )?;
    }
    Ok(())
}

/// Every pronunciation of `tokens`, grouped by token, from **one** ordered
/// walk of the pronunciation table.
///
/// The table's keys are a big-endian token prefix plus a key tail, so a
/// single ascending walk yields each token's rows contiguously, in the
/// same per-token order the per-token [`pronunciation_range`] scan
/// produced. Multi-token readers use this instead of one `range` scan —
/// each with its own read transaction and cursor — per token.
fn collect_pronunciations_for_tokens(
    store: &impl ReadStore,
    tokens: &std::collections::HashSet<Token>,
) -> Result<std::collections::BTreeMap<Token, Vec<UserPronunciation>>, UserStoreError> {
    let mut out: std::collections::BTreeMap<Token, Vec<(u32, UserPronunciation)>> =
        std::collections::BTreeMap::new();
    if tokens.is_empty() {
        return Ok(std::collections::BTreeMap::new());
    }
    store.for_each(PRONUNCIATION, |key, value| {
        let (token, key_bytes) = codec::decode_token_bytes(key)
            .map_err(|_| StoreError::Backend("corrupt pronunciation key".into()))?;
        if !tokens.contains(&token) {
            return Ok(());
        }
        let value = PronValue::decode(value)?;
        out.entry(token).or_default().push((
            value.seq,
            UserPronunciation::new(phrase::decode_keys(key_bytes), value.count, value.indexed),
        ));
        Ok(())
    })?;
    Ok(out
        .into_iter()
        .map(|(token, mut rows)| {
            sort_by_insertion(&mut rows, |row| row.0);
            (token, rows.into_iter().map(|(_, row)| row).collect())
        })
        .collect())
}

/// The pronunciation key tails owned by `tokens`, from **one** ordered
/// walk of the pronunciation table inside `txn` — the write-side twin of
/// [`collect_pronunciations_for_tokens`], for callers that remove the
/// rows afterwards. Returns `(token, key tail)` pairs so each removal can
/// rebuild its full key with [`codec::encode_token_bytes`].
fn collect_pronunciation_keys_from_txn(
    txn: &impl WriteTxn,
    tokens: &std::collections::HashSet<Token>,
) -> Result<Vec<(Token, Vec<u8>)>, StoreError> {
    let mut out = Vec::new();
    if tokens.is_empty() {
        return Ok(out);
    }
    txn.for_each(PRONUNCIATION, |key, _value| {
        let (token, key_bytes) = codec::decode_token_bytes(key)
            .map_err(|_| StoreError::Backend("corrupt pronunciation key".into()))?;
        if tokens.contains(&token) {
            out.push((token, key_bytes.to_vec()));
        }
        Ok(())
    })?;
    Ok(out)
}

// ── the user bigram's grams ───────────────────────────────────────

/// The four operations `Bigram` runs on its container (`ngram_bdb.cpp`
/// and its Kyoto Cabinet and tkrzw twins): `get_all_items`, `load`,
/// `store`, `remove`. A libpinyin session runs them on the pin's own
/// container; a plain or standalone store on its [`BIGRAM_GRAM`] rows.
pub(crate) trait GramOps {
    /// `Bigram::get_all_items`: every key in the container's walk order;
    /// `None` for a key that is not a four-byte `phrase_token_t`.
    fn keys(&mut self) -> Result<Vec<Option<Token>>, StoreError>;
    /// `Bigram::load`: the raw gram under `prev`.
    fn get(&mut self, prev: Token) -> Result<Option<Vec<u8>>, StoreError>;
    /// `Bigram::store`.
    fn store(&mut self, prev: Token, gram: &[u8]) -> Result<(), StoreError>;
    /// `Bigram::remove`.
    fn remove(&mut self, prev: Token) -> Result<(), StoreError>;
}

/// The pin's own in-memory user bigram container, keyed by the four
/// native-endian token bytes.
pub(crate) struct ContainerGrams<'a>(pub(crate) &'a oxpinyin_store::DefaultUserBigramDb);

impl GramOps for ContainerGrams<'_> {
    fn keys(&mut self) -> Result<Vec<Option<Token>>, StoreError> {
        Ok(self
            .0
            .keys()?
            .into_iter()
            .map(|key| {
                <[u8; 4]>::try_from(key.as_slice())
                    .ok()
                    .map(Token::from_le_bytes)
            })
            .collect())
    }

    fn get(&mut self, prev: Token) -> Result<Option<Vec<u8>>, StoreError> {
        self.0.get(&prev.to_le_bytes())
    }

    fn store(&mut self, prev: Token, gram: &[u8]) -> Result<(), StoreError> {
        self.0.store(&prev.to_le_bytes(), gram)
    }

    fn remove(&mut self, prev: Token) -> Result<(), StoreError> {
        self.0.remove(&prev.to_le_bytes())
    }
}

/// A plain store's [`BIGRAM_GRAM`] rows inside one write transaction.
pub(crate) struct TableGrams<'a, T>(pub(crate) &'a mut T);

impl<T: WriteTxn> GramOps for TableGrams<'_, T> {
    fn keys(&mut self) -> Result<Vec<Option<Token>>, StoreError> {
        let mut keys = Vec::new();
        self.0.for_each(BIGRAM_GRAM, |key, _| {
            keys.push(<[u8; 4]>::try_from(key).ok().map(Token::from_be_bytes));
            Ok(())
        })?;
        Ok(keys)
    }

    fn get(&mut self, prev: Token) -> Result<Option<Vec<u8>>, StoreError> {
        self.0.get(BIGRAM_GRAM, &prev.to_be_bytes())
    }

    fn store(&mut self, prev: Token, gram: &[u8]) -> Result<(), StoreError> {
        self.0.put(BIGRAM_GRAM, &prev.to_be_bytes(), gram)
    }

    fn remove(&mut self, prev: Token) -> Result<(), StoreError> {
        self.0.remove(BIGRAM_GRAM, &prev.to_be_bytes())
    }
}

/// A snapshot of a plain store's [`BIGRAM_GRAM`] rows, for the fault walk,
/// which only reads.
struct ReadGrams(Vec<(Vec<u8>, Vec<u8>)>);

impl GramOps for ReadGrams {
    fn keys(&mut self) -> Result<Vec<Option<Token>>, StoreError> {
        Ok(self
            .0
            .iter()
            .map(|(key, _)| {
                <[u8; 4]>::try_from(key.as_slice())
                    .ok()
                    .map(Token::from_be_bytes)
            })
            .collect())
    }

    fn get(&mut self, prev: Token) -> Result<Option<Vec<u8>>, StoreError> {
        let key = prev.to_be_bytes();
        Ok(self
            .0
            .iter()
            .find(|(stored, _)| stored.as_slice() == key)
            .map(|(_, gram)| gram.clone()))
    }

    fn store(&mut self, _prev: Token, _gram: &[u8]) -> Result<(), StoreError> {
        Err(StoreError::InvalidInput("read-only gram snapshot"))
    }

    fn remove(&mut self, _prev: Token) -> Result<(), StoreError> {
        Err(StoreError::InvalidInput("read-only gram snapshot"))
    }
}

/// Whether a raw gram's partial item tail is one the pin's `cur != end`
/// walks read past (register rows 98 and 100).
fn has_partial_tail(gram: &[u8]) -> bool {
    gram.len() >= single_gram::TOTAL_LEN
        && !(gram.len() - single_gram::TOTAL_LEN).is_multiple_of(single_gram::ITEM_LEN)
}

/// `Bigram::mask_out(mask, value)` (`ngram_bdb.cpp:217-254`, synced with
/// `ngram_kyotodb.cpp:198-235` and `ngram_tkrzwdb.cpp:174-211` at the pin
/// `074a2219`): over the keys in the container's walk order, a selected
/// key is removed; any other gram is loaded and masked
/// ([`single_gram::mask_out`]); one with nothing removed is left untouched,
/// one left with no items is removed and any other is stored back.
/// Answers whether a key remains. The faults the pin dies on are the
/// caller's to refuse first ([`gram_mask_fault`]).
fn mask_grams(grams: &mut impl GramOps, mask: Token, value: Token) -> Result<bool, StoreError> {
    let mut remaining = false;
    for key in grams.keys()? {
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

/// The fault [`mask_grams`] would die on at the pin, in the pin's own
/// order, or `None` when the walk completes. `get_all_items` asserts on a
/// key that is not four bytes before any gram is loaded
/// (`ngram_bdb.cpp:199`); then, per key the mask leaves to be loaded,
/// `SingleGram::mask_out` asserts on a missing total
/// (`memory_chunk.h:390`) and reads past a partial item
/// (`ngram.cpp:86`), and a gram the mask empties with a total left over
/// asserts in `get_length` (`ngram.cpp:70`, reached at `ngram_bdb.cpp:243`).
/// The last is found by running the mask on a copy.
fn gram_mask_fault(
    grams: &mut impl GramOps,
    mask: Token,
    value: Token,
) -> Result<Option<UserStoreError>, StoreError> {
    let keys = grams.keys()?;
    if keys.iter().any(Option::is_none) {
        return Ok(Some(UserStoreError::NonTokenUserBigramKey));
    }
    for prev in keys.into_iter().flatten() {
        if prev & mask == value {
            continue;
        }
        let Some(mut gram) = grams.get(prev)? else {
            continue;
        };
        if gram.len() < single_gram::TOTAL_LEN || has_partial_tail(&gram) {
            return Ok(Some(UserStoreError::ShortUserBigramValue));
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

fn mask_phrase_rows(txn: &mut impl WriteTxn, mask: Token, value: Token) -> Result<(), StoreError> {
    // User phrases: text, reverse lookup, and pronunciations.
    let mut matched: Vec<(Token, String)> = Vec::new();
    txn.for_each(PHRASE, |k, v| {
        let token = codec::decode_token(k)
            .map_err(|_| StoreError::Backend("corrupt phrase token".into()))?;
        if (token & mask) == value {
            let text = codec::decode_str(v)
                .map_err(|_| StoreError::Backend("corrupt phrase text".into()))?
                .to_owned();
            matched.push((token, text));
        }
        Ok(())
    })?;
    // One ordered walk collects every matched token's
    // pronunciation keys, instead of a per-token range scan
    // (and cursor) inside the remove loop below.
    let matched_tokens: std::collections::HashSet<Token> =
        matched.iter().map(|(token, _)| *token).collect();
    let pron_keys = collect_pronunciation_keys_from_txn(txn, &matched_tokens)?;
    for (token, text) in matched {
        txn.remove(PHRASE, &codec::encode_token(token))?;
        if phrase_index_library_index(token) == USER_DICTIONARY {
            txn.remove(PHRASE_BY_TEXT, codec::encode_str(&text))?;
        }
        txn.remove(
            PHRASE_BY_LIB_TEXT,
            &codec::encode_u8_str(phrase_index_library_index(token), &text),
        )?;
    }
    for (token, key_bytes) in pron_keys {
        txn.remove(PRONUNCIATION, &codec::encode_token_bytes(token, &key_bytes))?;
    }
    phrase_table::mask_out(txn, mask, value)?;
    Ok(())
}

/// Re-derive the `ADDON_DICTIONARY`, `NETWORK_DICTIONARY` and
/// `USER_DICTIONARY` allocation cursors after items left those sub-indexes.
///
/// The pin gives a new phrase `range.m_range_end` for any index
/// (`_add_phrase`, `pinyin.cpp:586-592`; addon promotion, `pinyin.cpp:2541-2544`),
/// and `SubPhraseIndex::get_range` drops trailing zero offsets
/// (`phrase_index.cpp:624-650`): the next token is one past the highest token
/// still holding an item, so a mask or removal that empties the top of the
/// range hands those tokens out again. `add_phrase_in` and
/// `promote_addon_phrase` keep the cursor at that value on every add; this
/// restores it after a removal, in the same walk-the-table cost the removal
/// already paid.
fn rederive_alloc_cursors(txn: &mut impl WriteTxn, only: &[u8]) -> Result<(), StoreError> {
    const LIBRARIES: [u8; 3] = [ADDON_DICTIONARY, NETWORK_DICTIONARY, USER_DICTIONARY];
    let mut highest: [Option<Token>; 3] = [None; 3];
    txn.for_each(PHRASE, |key, _| {
        let token = codec::decode_token(key)
            .map_err(|_| StoreError::Backend("corrupt phrase token".into()))?;
        let library = phrase_index_library_index(token);
        if let Some(slot) = LIBRARIES.iter().position(|&candidate| candidate == library) {
            highest[slot] = highest[slot].max(Some(token));
        }
        Ok(())
    })?;
    for (library, top) in LIBRARIES.into_iter().zip(highest) {
        if !only.is_empty() && !only.contains(&library) {
            continue;
        }
        let next = match top {
            Some(top) => phrase::next_library_token_after(library, top),
            None => Some(phrase::first_library_token(library)),
        };
        // A full sub-index has no cursor to move: the add path reports the
        // exhaustion from the row it already holds.
        let Some(next) = next else { continue };
        txn.put(
            ALLOC,
            &codec::encode_u8(library),
            &codec::encode_token(next),
        )?;
        if library == USER_DICTIONARY {
            txn.put(
                ALLOC,
                &codec::encode_u8(ALLOC_CURSOR),
                &codec::encode_token(next),
            )?;
        }
    }
    Ok(())
}

/// What a library replacement does: the system libraries a mask or reload
/// maps again, or one `USER_FILE` library's unload or load.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LibraryChange {
    System,
    UserUnload(u8),
    UserLoad(u8),
}

impl LibraryChange {
    const fn user_library(self) -> Option<u8> {
        match self {
            Self::System => None,
            Self::UserUnload(library) | Self::UserLoad(library) => Some(library),
        }
    }
}

/// Removes the unigram, phrase and pronunciation rows of the reloaded
/// libraries. Returns the readings of `user_library` that the user pinyin
/// index carried, as `(token, packed keys)`.
fn remove_reloaded_rows(
    txn: &mut impl WriteTxn,
    reloaded: impl Fn(Token) -> bool,
    user_library: Option<u8>,
) -> Result<std::collections::BTreeSet<(Token, Vec<u16>)>, StoreError> {
    let mut unigrams = Vec::new();
    txn.for_each(UNIGRAM, |key, _| {
        let token = codec::decode_token(key)
            .map_err(|_| StoreError::Backend("corrupt unigram key".into()))?;
        if reloaded(token) {
            unigrams.push(key.to_vec());
        }
        Ok(())
    })?;
    for key in unigrams {
        txn.remove(UNIGRAM, &key)?;
    }
    // PHRASE represents a live sub-index item, unlike the raw phrase table.
    // Discard the old library's items before seeding the filtered log.
    let mut phrases = Vec::new();
    txn.for_each(PHRASE, |key, value| {
        let token = codec::decode_token(key)
            .map_err(|_| StoreError::Backend("corrupt phrase token".into()))?;
        if reloaded(token) {
            let text = codec::decode_str(value)
                .map_err(|_| StoreError::Backend("corrupt phrase text".into()))?;
            phrases.push((token, text.to_owned()));
        }
        Ok(())
    })?;
    for (token, text) in phrases {
        txn.remove(PHRASE, &codec::encode_token(token))?;
        txn.remove(
            PHRASE_BY_LIB_TEXT,
            &codec::encode_u8_str(phrase_index_library_index(token), &text),
        )?;
        if phrase_index_library_index(token) == USER_DICTIONARY {
            txn.remove(PHRASE_BY_TEXT, codec::encode_str(&text))?;
        }
    }
    let mut deltas = Vec::new();
    let mut indexed = std::collections::BTreeSet::new();
    txn.for_each(PRONUNCIATION, |key, value| {
        let (token, key_bytes) = codec::decode_token_bytes(key)
            .map_err(|_| StoreError::Backend("corrupt pronunciation key".into()))?;
        // These are item readings/deltas, not phrase-table membership.
        if !reloaded(token) {
            return Ok(());
        }
        deltas.push(key.to_vec());
        if user_library == Some(phrase_index_library_index(token))
            && PronValue::decode(value)?.indexed
            && let Some(packed) =
                crate::store_libpinyin::pinyin_keys_to_packed(&phrase::decode_keys(key_bytes))
        {
            indexed.insert((token, packed));
        }
        Ok(())
    })?;
    for key in deltas {
        txn.remove(PRONUNCIATION, &key)?;
    }
    Ok(indexed)
}

/// What a replacement installs in [`Target::kept_index`] once its
/// transaction has committed: the library and its entries.
type PendingKept = Option<(u8, std::collections::BTreeSet<(Token, Vec<u16>)>)>;

/// The user pinyin index outlives a `USER_FILE` library's sub-index: its
/// entries stay in it, found on the items the chunk brings back
/// (`state.indexed` for the seed) and on whatever item is later given a
/// dropped token again ([`Target::kept_index`] keeps what no reading backs).
/// `indexed` is what the removed rows carried. Nothing is changed here: the
/// entries to keep come back for the caller to install after the store
/// write took (policy row 103), so a refused write leaves `kept_index` as it
/// was.
fn kept_index_state<'a>(
    target: &crate::store_libpinyin::Target,
    originals: &crate::persistence::SystemOriginals,
    change: LibraryChange,
    indexed: std::collections::BTreeSet<(Token, Vec<u16>)>,
    state: &'a crate::persistence::UserState,
) -> (
    std::borrow::Cow<'a, crate::persistence::UserState>,
    PendingKept,
) {
    let Some(library) = change.user_library() else {
        return (std::borrow::Cow::Borrowed(state), None);
    };
    let mut entries = target
        .kept_index
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&library)
        .cloned()
        .unwrap_or_default();
    entries.extend(indexed);
    if change != LibraryChange::UserLoad(library) {
        return (std::borrow::Cow::Borrowed(state), Some((library, entries)));
    }
    let mut state = state.clone();
    state.indexed.extend(entries.iter().cloned());
    let backed = crate::store_libpinyin::backed_readings(&state, originals);
    entries.retain(|entry| !backed.contains(entry));
    (std::borrow::Cow::Owned(state), Some((library, entries)))
}

/// The facade total's unigram sum, stored after a replacement.
fn store_unigram_total(txn: &mut impl WriteTxn) -> Result<(), StoreError> {
    let mut total = 0_u64;
    txn.for_each(UNIGRAM, |_, value| {
        total = total.saturating_add(
            codec::decode_u64(value)
                .map_err(|_| StoreError::Backend("corrupt unigram value".into()))?,
        );
        Ok(())
    })?;
    txn.put(
        UNIGRAM_TOTAL,
        &codec::encode_u8(UNIGRAM_TOTAL_KEY),
        &codec::encode_u64(total),
    )
}

/// Whether the store holds any user data, evaluated inside `txn`.
///
/// `||` short-circuits, so each table is probed with a first-row
/// `WriteTxn::is_empty` check and full-table scans are avoided.
fn has_user_data_in_write_txn(txn: &impl WriteTxn) -> Result<bool, StoreError> {
    Ok(!txn.is_empty(BIGRAM_GRAM)?
        || !txn.is_empty(UNIGRAM)?
        || !txn.is_empty(PHRASE)?
        || !txn.is_empty(PRONUNCIATION)?)
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
pub struct GenericUserStore<S: WriteStore> {
    /// The facade fini's `user.conf` write: armed only on the handle a
    /// libpinyin open returned, never on a clone ([`FiniGuard`]).
    _fini: FiniGuard,
    inner: Arc<StoreInner<S>>,
    /// Keeps a standalone path reservation alive until its last clone drops.
    _standalone_lease: Option<Arc<StandaloneLease>>,
    /// This handle's current training call's step checks.
    training: TrainingCheck,
    /// Reused only by libpinyin sessions, whose raw rows have no outside writer.
    import_readings: Option<ImportReadings>,
    /// Last field so [`RegistryLease`] drains after this handle's `Arc` dies.
    _lease: RegistryLease,
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
}

/// One training call's replay: the grams its checked steps left, and the
/// fault a failing check met.
#[derive(Debug, Default)]
struct TrainingCheck {
    grams: HashMap<Token, Option<Vec<u8>>>,
    fault: Option<TrainingFault>,
}

/// Default user store backed by [`DefaultStore`] — whichever peer
/// backend (Kyoto Cabinet, tkrzw, Berkeley DB) the build was
/// compiled against.
///
/// Berkeley DB is the default selection under the workspace's default
/// feature set (since 2026-09-20); the others are selected with
/// `--no-default-features --features {kyotocabinet|tkrzw}`.
pub type UserStore = GenericUserStore<DefaultStore>;

impl<S: WriteStore> GenericUserStore<S> {
    /// Whether a system journal's ADD payload was shorter than its
    /// unigram, which stops that library's replay there (register row 108).
    #[must_use]
    pub fn short_add_payload(&self) -> bool {
        self.inner
            .libpinyin
            .as_ref()
            .is_some_and(|target| target.short_add_payload)
    }

    /// Crate-visible handle assembly for the libpinyin constructor
    /// ([`crate::store_libpinyin`]), which runs in memory and arms
    /// the fini guard of the open that raised the counter.
    pub(crate) fn from_parts(
        inner: Arc<StoreInner<S>>,
        lease: Option<Arc<StandaloneLease>>,
        fini: FiniGuard,
    ) -> Self {
        Self {
            _fini: fini,
            inner,
            _standalone_lease: lease,
            training: TrainingCheck::default(),
            import_readings: None,
            _lease: RegistryLease,
        }
    }
}

impl<S: WriteStore> Clone for GenericUserStore<S> {
    fn clone(&self) -> Self {
        Self {
            // A clone shares the session, not the open: only the handle
            // the open returned finishes it.
            _fini: FiniGuard::disarmed(),
            inner: Arc::clone(&self.inner),
            _standalone_lease: self._standalone_lease.clone(),
            training: TrainingCheck::default(),
            import_readings: None,
            _lease: RegistryLease,
        }
    }
}

impl<S: WriteStore> fmt::Debug for GenericUserStore<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GenericUserStore").finish_non_exhaustive()
    }
}

impl<S: WriteStore> GenericUserStore<S> {
    /// Locks the shared store handle, recovering from a poisoned lock
    /// (constitution §4: nothing here panics, so a poisoned mutex must not
    /// brick the store either).
    pub(crate) fn database(&self) -> MutexGuard<'_, SessionValues<S>> {
        self.inner
            .db
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The user pinyin index's readings past `MAX_PHRASE_LENGTH` syllables,
    /// as packed `ChewingKey` words — the pin's user `ChewingLargeTable2`
    /// carries one DB key per reading, and any key past the table's
    /// 16-syllable instantiation drives its `switch`'s `default: abort()`
    /// sites. Empty for a plain or standalone store, which has no
    /// `user_pinyin_index.bin`.
    #[must_use]
    pub(crate) fn overlong_index_readings(&self) -> &[Vec<u16>] {
        self.inner
            .libpinyin
            .as_ref()
            .map_or(&[][..], |target| target.overlong_index_keys.as_slice())
    }

    /// Whether `user_pinyin_index.bin` carried a key past
    /// `MAX_PHRASE_LENGTH` syllables, or one with no whole syllable.
    /// `pinyin_mask_out` / `zhuyin_mask_out` walk every record, so any such
    /// key aborts the whole mask (`chewing_large_table2_bdb.cpp:529`).
    #[must_use]
    pub fn has_overlong_index_key(&self) -> bool {
        !self.overlong_index_readings().is_empty()
    }

    /// The pin's in-memory user bigram container, locked — `None` on a
    /// plain or standalone store. Taken after the database guard when both
    /// are held, never before it.
    fn container(&self) -> Option<MutexGuard<'_, DefaultUserBigramDb>> {
        self.inner.bigram_db.as_ref().map(|container| {
            container
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        })
    }

    /// `Bigram::load(prev)`: the raw gram stored under `prev` — one lock
    /// and one point read, of the pin's container on a libpinyin session
    /// and of the [`BIGRAM_GRAM`] row on any other store. `None` when
    /// `prev` has no gram.
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
        if let Some(container) = self.container() {
            return Ok(container.get(&prev.to_le_bytes())?);
        }
        let db = self.database();
        Ok(db.get(BIGRAM_GRAM, &prev.to_be_bytes())?)
    }

    /// [`Self::bigram_row`] with the database guard already held.
    fn gram_in(
        &self,
        db: &SessionValues<S>,
        prev: Token,
    ) -> Result<Option<Vec<u8>>, UserStoreError> {
        if let Some(container) = self.container() {
            return Ok(container.get(&prev.to_le_bytes())?);
        }
        Ok(db.get(BIGRAM_GRAM, &prev.to_be_bytes())?)
    }

    /// `Bigram::get_all_items`: every key in the user bigram's walk order —
    /// the pin's container's on a libpinyin session, ascending otherwise —
    /// with `None` for a key that is not a four-byte `phrase_token_t`.
    fn gram_keys(&self) -> Result<Vec<Option<Token>>, UserStoreError> {
        if let Some(container) = self.container() {
            return Ok(ContainerGrams(&container).keys()?);
        }
        let db = self.database();
        let mut keys = Vec::new();
        db.for_each(BIGRAM_GRAM, |key, _| {
            keys.push(<[u8; 4]>::try_from(key).ok().map(Token::from_be_bytes));
            Ok(())
        })?;
        Ok(keys)
    }

    /// Store `value` raw under `token` in the session's container, as a
    /// value the writer never produces, for the tests.
    #[cfg(test)]
    pub(crate) fn plant_raw_bigram(&self, token: Token, value: &[u8]) {
        self.container()
            .expect("libpinyin container")
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

    /// Whether the gram stored under `token` has no whole item and a
    /// nonzero `total_freq` — the pin's `SingleGram::get_length` assert
    /// (`ngram.cpp:70`). `_compute_predicted_bigram_candidates` merges only
    /// the resolved prefixes' own grams (`pinyin.cpp:2322-2336`), so an
    /// item-less row under any other token never reaches `get_length`.
    #[must_use]
    pub fn has_empty_bigram_gram_for(&self, token: Token) -> bool {
        self.bigram_row(token).ok().flatten().is_some_and(|bytes| {
            single_gram::get_length(&bytes) == 0
                && single_gram::get_total_freq(&bytes).is_some_and(|total| total != 0)
        })
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
            .is_some_and(|bytes| has_partial_tail(&bytes))
    }

    /// Whether `mask_out(mask, value)` makes the pin abort in
    /// `get_length`: a gram whose key the mask leaves alone, from which the
    /// mask removes every item while the stored total is not used up
    /// (`ngram.cpp:70`, reached at `ngram_bdb.cpp:243`). Found by running
    /// the ported mask on a copy of each such gram.
    #[must_use]
    pub fn has_residual_bigram_gram(&self, mask: Token, value: Token) -> bool {
        matches!(
            self.bigram_mask_fault(mask, value),
            Some(UserStoreError::ResidualUserBigramGram)
        )
    }

    /// The fault the pin's `Bigram::mask_out(mask, value)` dies on over the
    /// live user bigram, or `None` when the walk completes
    /// ([`gram_mask_fault`]). `pinyin_mask_out` / `zhuyin_mask_out` and
    /// `pinyin_remove_user_candidate` (`user_bigram->mask_out`,
    /// `pinyin.cpp:1230`, `zhuyin.cpp:765`, `pinyin.cpp:3766`) all reach
    /// the same walk.
    fn bigram_mask_fault(&self, mask: Token, value: Token) -> Option<UserStoreError> {
        let fault = if let Some(container) = self.container() {
            gram_mask_fault(&mut ContainerGrams(&container), mask, value)
        } else {
            let db = self.database();
            let mut keys = Vec::new();
            let _ = db.for_each(BIGRAM_GRAM, |key, gram| {
                keys.push((key.to_vec(), gram.to_vec()));
                Ok(())
            });
            gram_mask_fault(&mut ReadGrams(keys), mask, value)
        };
        fault.unwrap_or(None)
    }

    /// Starts one training call's step checks ([`Self::check_training_step`]):
    /// forgets the grams replayed and the fault recorded by the last one.
    pub fn begin_training_steps(&mut self) {
        self.training = TrainingCheck::default();
    }

    /// The fault the last failing [`Self::check_training_step`] met, once.
    pub fn take_training_fault(&mut self) -> Option<TrainingFault> {
        self.training.fault.take()
    }

    /// One training step's store checks, in the pin's order inside the
    /// step (`phonetic_lookup.h:876-909` at 074a2219): a gram too short for
    /// its total (`get_total_freq`, :884; `memory_chunk.h:390`), a partial
    /// item tail the `get_freq` walk reads past (:888, register row 100),
    /// and a `set_freq` that misses the item `insert_freq` just added
    /// (:903). The step runs on the gram the earlier steps of this call
    /// left, replayed on copies: nothing is stored. The fault is recorded
    /// for [`Self::take_training_fault`].
    ///
    /// # Errors
    ///
    /// [`UserStoreError::ShortUserBigramValue`] for a short value or a
    /// partial tail, [`UserStoreError::TrainingSetFreqMiss`] for the miss,
    /// or the store's read error.
    pub fn check_training_step(&mut self, prev: Token, cur: Token) -> Result<(), UserStoreError> {
        let row = match self.training.grams.get(&prev) {
            Some(row) => row.clone(),
            None => self.bigram_row(prev)?,
        };
        if let Some(gram) = row.as_deref() {
            if gram.len() < single_gram::TOTAL_LEN {
                self.training.fault = Some(TrainingFault::ShortValue(prev));
                return Err(UserStoreError::ShortUserBigramValue);
            }
            if has_partial_tail(gram) {
                self.training.fault = Some(TrainingFault::PartialTail(prev));
                return Err(UserStoreError::ShortUserBigramValue);
            }
        }
        match trained_gram(row.clone(), cur) {
            Ok((_, gram)) => {
                self.training.grams.insert(prev, gram.or(row));
                Ok(())
            }
            Err(UserStoreError::TrainingSetFreqMiss) => {
                self.training.fault = Some(TrainingFault::SetFreqMiss(prev));
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
        let Some(target) = self.inner.libpinyin.as_ref() else {
            return false;
        };
        if !target.index_keys.contains(&projection) {
            return false;
        }
        target
            .overlong_index_keys
            .iter()
            .any(|reading| reading.len() > projection.len() && reading.starts_with(&projection))
    }

    fn count_cache(&self) -> MutexGuard<'_, Option<CountCache>> {
        self.inner
            .count_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
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

    /// The count memo's entry for `token`'s unigram row, if it holds one.
    #[cfg(test)]
    pub(crate) fn memoised_unigram_for_test(&self, token: Token) -> Option<u64> {
        self.count_cache()
            .as_ref()
            .and_then(|cache| cache.unigram.get(&token).copied())
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
    /// The database guard is taken for the whole call and handed to
    /// `read`, which needs it only to fill a memo miss; the lock order is
    /// the same one the snapshot cache used — cache first, then db.
    /// `write_generation` is read under that guard, so cache validation
    /// is ordered against the same critical section every commit bumps
    /// in: the observed database state and generation always belong to
    /// one side of a commit, never straddle it.  Holding the guard across
    /// the call is also what makes a multi-key read atomic without MVCC:
    /// every write path takes the same guard, so no commit can land
    /// between the two or four rows `count_delta` reads.
    fn with_count_cache<T>(
        &self,
        read: impl FnOnce(&mut CountCache, &SessionValues<S>) -> Result<T, UserStoreError>,
    ) -> Result<T, UserStoreError> {
        let mut cache = self.count_cache();
        let db = self.database();
        let generation = self.write_generation();
        let mut current = match cache.take() {
            Some(cached) if cached.generation == generation => cached,
            _ => CountCache::new(generation),
        };
        let out = read(&mut current, &db);
        drop(db);
        *cache = Some(current);
        out
    }

    /// Invalidate cached reads after a committed user-data write.
    ///
    /// The generation bump lands while the database guard is still held:
    /// paired with [`Self::with_count_cache`] reading the generation under
    /// the same guard, no reader can observe post-commit rows against a
    /// pre-commit generation (or vice versa).
    fn mark_committed_write(&self, db: MutexGuard<'_, SessionValues<S>>, has_user_data: bool) {
        self.inner
            .has_user_data
            .store(has_user_data, Ordering::Release);
        self.inner.write_generation.fetch_add(1, Ordering::AcqRel);
        drop(db);
        *self.count_cache() = None;
    }

    /// A phrase-table edit invalidates exact-text rows and phonetic
    /// lookups. Count-only writes take [`Self::mark_committed_write`].
    fn mark_committed_phrase_write(
        &self,
        db: MutexGuard<'_, SessionValues<S>>,
        has_user_data: bool,
    ) {
        self.inner
            .phrase_table_generation
            .fetch_add(1, Ordering::AcqRel);
        self.mark_committed_reading_write(db, has_user_data);
    }

    // Keep the import memo's publication separate from whether this write
    // changed the exact-text row; reading-only merges retain queried rows.
    fn mark_committed_import_write(&self, db: MutexGuard<'_, SessionValues<S>>, added_token: bool) {
        if added_token {
            self.mark_committed_phrase_write(db, true);
        } else {
            self.mark_committed_reading_write(db, true);
        }
    }

    /// A pronunciation merge changes the phonetic lookup, but keeps the
    /// phrase table's text-to-token rows (074a2219 pinyin.cpp:563-582).
    fn mark_committed_reading_write(
        &self,
        db: MutexGuard<'_, SessionValues<S>>,
        has_user_data: bool,
    ) {
        self.inner.phrase_generation.fetch_add(1, Ordering::AcqRel);
        self.mark_committed_write(db, has_user_data);
    }

    /// Keeps the keys of pinyin index rows a committed mask or removal took out
    /// of the store, so the session's saves write them back empty, as the
    /// pin's tables keep them ([`crate::persistence::IndexKeys`]). A store
    /// with no libpinyin session writes no index file and keeps nothing.
    fn keep_index_rows(&self, rows: &IndexRows) {
        if let Some(target) = self.inner.libpinyin.as_ref() {
            rows.keep_in(&mut target.index_file_keys());
        }
    }

    /// A search of the user pinyin table for the query `keys` makes
    /// (`ChewingLargeTable2::search`): a hit on a non-empty record loads it
    /// into the length's table entry, which the next `mask_out` walk starts
    /// from (#785). `natural` answers whether the readings put an item
    /// under the probed key; neither closure runs while the table is empty.
    #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
    pub fn note_pinyin_search(
        &self,
        keys: impl FnOnce() -> Option<Vec<ChewingKey>>,
        natural: impl FnOnce() -> bool,
    ) {
        if let Some(target) = self.inner.libpinyin.as_ref() {
            let mut chain = target.index_file_keys();
            if !chain.is_idle()
                && let Some(keys) = keys()
            {
                chain.note_search(&keys, natural);
            }
        }
    }

    /// A `search_suggestion` of the user pinyin table for the prefix `keys`
    /// makes (#785): every longer key it visits empties its length's entry.
    #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
    pub fn note_pinyin_suggestion(&self, keys: impl FnOnce() -> Option<Vec<ChewingKey>>) {
        if let Some(target) = self.inner.libpinyin.as_ref() {
            let mut chain = target.index_file_keys();
            if !chain.is_idle()
                && let Some(keys) = keys()
            {
                chain.note_suggestion(&keys);
            }
        }
    }

    /// The user pinyin table's records as the mask walk finds them and as
    /// the mask leaves the readings (#785): the walk's input and the
    /// records the held readings derive afterwards. Every key the table
    /// holds now stays in it.
    #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
    fn pinyin_mask_records(
        &self,
        mask: Token,
        value: Token,
    ) -> Result<
        Option<(
            oxpinyin_data::table_entries::Entries,
            oxpinyin_data::table_entries::Entries,
        )>,
        UserStoreError,
    > {
        use oxpinyin_data::table_entries::pinyin_index_entries;
        let Some(target) = self.inner.libpinyin.as_ref() else {
            return Ok(None);
        };
        let rows = {
            let originals = target.originals();
            let state = crate::store_libpinyin::export_state(self, &originals)?;
            crate::persistence::chewing_rows(&state, &originals)
        };
        let mut kept = target.index_file_keys();
        let before = kept.with_tails(crate::persistence::with_kept_keys(
            pinyin_index_entries(&rows),
            &kept.pinyin,
        ));
        kept.pinyin
            .extend(before.iter().map(|(key, _)| key.clone()));
        let survivors: Vec<_> = rows
            .into_iter()
            .filter(|row| (row.token & mask) != value)
            .collect();
        let after =
            crate::persistence::with_kept_keys(pinyin_index_entries(&survivors), &kept.pinyin);
        Ok(Some((before, after)))
    }

    /// `ChewingLargeTable2::add_index` on the session's user pinyin table:
    /// leaves the entry state the Kyoto Cabinet and tkrzw mask walk starts
    /// from (#785). `exists` is whether the table already carried the item.
    #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
    fn note_index_add(&self, keys: &[PinyinKey], token: Token, exists: bool) {
        if let Some(target) = self.inner.libpinyin.as_ref()
            && let Some(keys) = chewing_keys(keys)
        {
            target.index_file_keys().note_add(&keys, token, exists);
        }
    }

    /// `ChewingLargeTable2::remove_index` on the session's user pinyin
    /// table (#785).
    #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
    fn note_index_remove(&self, keys: &[ChewingKey], token: Token) {
        if let Some(target) = self.inner.libpinyin.as_ref() {
            target.index_file_keys().note_remove(keys, token);
        }
    }

    fn init_and_wrap(db: S) -> Result<Arc<StoreInner<S>>, UserStoreError> {
        let has_user_data = db.write(|txn| {
            let total_key = codec::encode_u8(UNIGRAM_TOTAL_KEY);
            if txn.get(UNIGRAM_TOTAL, &total_key)?.is_none() {
                let mut sum = 0_u64;
                txn.for_each(UNIGRAM, |_k, v| {
                    let delta = codec::decode_u64(v)
                        .map_err(|_| StoreError::Backend("corrupt unigram value".into()))?;
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
            system_bumps_live: std::sync::atomic::AtomicBool::new(false),
            system_bumps: Mutex::new(crate::registry::SystemUnigramBumps::default()),
            has_system_items: AtomicBool::new(false),
            system_items: std::sync::Mutex::new(crate::system_items::SystemItems::default()),
            has_removed_system_items: AtomicBool::new(false),
            removed_system_items: std::sync::Mutex::new(std::collections::BTreeSet::new()),
            count_cache: Mutex::new(None),
            db: Mutex::new(SessionValues::Native(db)),
            dirty: AtomicBool::new(false),
            write_generation: AtomicU64::new(0),
            phrase_generation: AtomicU64::new(0),
            phrase_table_generation: AtomicU64::new(0),
            #[cfg(test)]
            fail_replace: AtomicBool::new(false),
            has_user_data: AtomicBool::new(has_user_data),
            facade_offset: std::sync::atomic::AtomicU32::new(0),
            has_library_offsets: AtomicBool::new(false),
            libpinyin: None,
            bigram_db: None,
        }))
    }

    /// Open or create the one standalone store at `path` for this process.
    ///
    /// This bypasses [`UserStore::open`]'s shared-handle registry, but a
    /// second live `create_standalone` call for the same path returns
    /// [`UserStoreError::AlreadyOpen`]. Clones share the first handle.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the store cannot be created at `path`.
    pub fn create_standalone(path: &Path) -> Result<Self, UserStoreError> {
        let standalone_lease =
            Arc::new(registry::acquire_standalone(path).ok_or(UserStoreError::AlreadyOpen)?);
        let db = S::create(path)?;
        let inner = Self::init_and_wrap(db)?;
        Ok(Self {
            _fini: FiniGuard::disarmed(),
            inner,
            _standalone_lease: Some(standalone_lease),
            training: TrainingCheck::default(),
            import_readings: None,
            _lease: RegistryLease,
        })
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
        let db = self.database();
        let mut gram = self
            .gram_in(&db, prev)?
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
        self.store_gram(&db, prev, &gram)?;
        self.mark_committed_write(db, true);
        Ok(())
    }

    /// `Bigram::store(prev, gram)` with the database guard held: the pin's
    /// container on a libpinyin session, the [`BIGRAM_GRAM`] row otherwise.
    fn store_gram(
        &self,
        db: &SessionValues<S>,
        prev: Token,
        gram: &[u8],
    ) -> Result<(), UserStoreError> {
        if let Some(container) = self.container() {
            container.store(&prev.to_le_bytes(), gram)?;
            return Ok(());
        }
        db.write(|txn| TableGrams(txn).store(prev, gram))?;
        Ok(())
    }

    /// Accumulated phrase-index unigram delta for `token`; `0` if none.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the store cannot be read.
    pub fn unigram_delta(&self, token: Token) -> Result<u64, UserStoreError> {
        let bumps = self.system_unigrams_for_read();
        let stored = if self.has_user_data() {
            self.with_count_cache(|cached, db| cached.unigram_delta(db, token))?
        } else {
            0
        };
        Ok(stored.wrapping_add(
            bumps
                .as_ref()
                .and_then(|b| b.tokens.get(&token))
                .copied()
                .unwrap_or(0),
        ))
    }

    /// Apply the exported unigram increment to a loaded system library.
    /// The caller checks library visibility. The facade total changes even
    /// for a missing item or overflowing sub-index; accepted increments stay
    /// in memory until another count writer or the save diff needs the rows. This does not mark the context dirty
    /// (074a2219 `pinyin.cpp:2836-2843`, `zhuyin.cpp:1828-1835`).
    ///
    /// # Errors
    /// Returns the store error if the in-memory transaction fails.
    pub fn add_system_unigram_frequency(
        &self,
        token: Token,
        delta: u32,
    ) -> Result<bool, UserStoreError> {
        let library = phrase_index_library_index(token);
        let mut bumps = self
            .inner
            .system_bumps
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let exists = !self.system_item_removed(token)
            && (self.with_effective_system_item(token, |_| ()).is_some()
                || self.inner.libpinyin.as_ref().is_some_and(|target| {
                    target.originals().get(&library).is_some_and(|original| {
                        original.unigram(token & phrase::PHRASE_MASK).is_some()
                    })
                }));
        if !exists {
            self.inner.facade_offset.fetch_add(delta, Ordering::SeqCst);
            return Ok(false);
        }
        let total = if let Some(total) = bumps.libraries.get(&library) {
            *total
        } else {
            let db = self.database();
            let base = get_u64_or(&*db, SYSTEM_BASE, &codec::encode_u8(library), 0)? as u32;
            let offset = get_u64_or(&*db, LIBRARY_OFFSET, &codec::encode_u8(library), 0)? as u32;
            drop(db);
            base.wrapping_add(offset)
                .wrapping_add(self.stored_unigram_library_total(library)?)
        };
        let Some(total) = total.checked_add(delta) else {
            self.inner.facade_offset.fetch_add(delta, Ordering::SeqCst);
            return Ok(false);
        };
        bumps.libraries.insert(library, total);
        let count = bumps.tokens.entry(token).or_default();
        *count = count.wrapping_add(u64::from(delta));
        bumps.total = bumps.total.wrapping_add(u64::from(delta));
        self.inner.system_bumps_live.store(true, Ordering::Release);
        Ok(true)
    }

    fn system_unigrams_for_read(
        &self,
    ) -> Option<MutexGuard<'_, crate::registry::SystemUnigramBumps>> {
        self.inner
            .system_bumps_live
            .load(Ordering::Acquire)
            .then(|| {
                self.inner
                    .system_bumps
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
            })
    }

    /// Commit live item counts before another count writer or the save diff.
    /// This does not change the modified gate. Lock order is bumps, then DB;
    /// readers take the same bumps-first order so a flush cannot split a
    /// read between old rows and an already cleared overlay.
    pub(crate) fn flush_system_unigrams(&self) -> Result<(), UserStoreError> {
        if !self.inner.system_bumps_live.load(Ordering::Acquire) {
            return Ok(());
        }
        let mut bumps = self
            .inner
            .system_bumps
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let db = self.database();
        db.write(|txn| {
            for (&token, &delta) in &bumps.tokens {
                let key = codec::encode_token(token);
                let old = txn_get_u64_or(txn, UNIGRAM, &key, 0)?;
                txn.put(UNIGRAM, &key, &codec::encode_u64(old.wrapping_add(delta)))?;
            }
            bump_unigram_total(txn, bumps.total)
        })?;
        bumps.tokens.clear();
        bumps.libraries.clear();
        bumps.total = 0;
        self.inner.system_bumps_live.store(false, Ordering::Release);
        self.mark_committed_write(db, true);
        Ok(())
    }

    /// What the user side adds to the facade's unigram total: the sum of
    /// every stored unigram delta, plus the session's facade offset (what
    /// the loaded libraries' stored totals hold beyond their items).
    /// `0` for an empty store.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the store cannot be read.
    pub fn unigram_total(&self) -> Result<u64, UserStoreError> {
        let bumps = self.system_unigrams_for_read();
        let db = self.database();
        let total = get_u64_or(&*db, UNIGRAM_TOTAL, &codec::encode_u8(UNIGRAM_TOTAL_KEY), 0)?;
        drop(db);
        Ok(total
            .wrapping_add(u64::from(self.facade_offset()))
            .wrapping_add(bumps.as_ref().map_or(0, |b| b.total)))
    }

    /// The pinned sub-index's wrapping u32 total of stored deltas in one library.
    ///
    /// # Errors
    /// Returns the store error if a unigram row cannot be read or decoded.
    pub fn unigram_library_total(&self, library: u8) -> Result<u32, UserStoreError> {
        let bumps = self.system_unigrams_for_read();
        let stored = self.stored_unigram_library_total(library)?;
        Ok(bumps.as_ref().map_or(stored, |bumps| {
            bumps
                .tokens
                .iter()
                .filter(|(token, _)| phrase_index_library_index(**token) == library)
                .fold(stored, |total, (_, delta)| {
                    total.wrapping_add(*delta as u32)
                })
        }))
    }

    fn stored_unigram_library_total(&self, library: u8) -> Result<u32, UserStoreError> {
        let db = self.database();
        let lo = codec::encode_token(u32::from(library) << 24);
        let hi = u32::from(library)
            .checked_add(1)
            .and_then(|next| next.checked_mul(1 << 24))
            .map(codec::encode_token);
        let mut total = 0_u32;
        db.range(
            UNIGRAM,
            Bound::Included(lo.as_slice()),
            hi.as_ref()
                .map_or(Bound::Unbounded, |hi| Bound::Excluded(hi.as_slice())),
            |_, value| {
                let count = codec::decode_u64(value)
                    .map_err(|_| StoreError::Backend("corrupt unigram".into()))?;
                total = total.wrapping_add(count as u32);
                Ok(())
            },
        )?;
        Ok(total)
    }

    /// How far `library`'s stored total lies from the sum of its items
    /// (a wrapping `guint32`), until the next [`Self::compact_libraries`];
    /// `0` once compacted or when the two agree.
    ///
    /// # Errors
    /// Returns the store error if the row cannot be read.
    pub fn library_offset(&self, library: u8) -> Result<u32, UserStoreError> {
        if !self.inner.has_library_offsets.load(Ordering::SeqCst) {
            return Ok(0);
        }
        let db = self.database();
        Ok(get_u64_or(&*db, LIBRARY_OFFSET, &codec::encode_u8(library), 0)? as u32)
    }

    fn facade_offset(&self) -> u32 {
        self.inner.facade_offset.load(Ordering::SeqCst)
    }

    /// `FacadePhraseIndex::compact` (074a2219 `phrase_index.cpp:648-675`):
    /// every library's total is rebuilt from its items, so none carries a
    /// stored total's difference any more. The facade's total keeps it.
    /// The pin compacts in `pinyin_save` once it has something to write,
    /// at the end of an import and at the end of a mask_out
    /// (`pinyin.cpp:1139`, `:657`, `:1294`; `zhuyin.cpp:554`, `:542`,
    /// `:829`).
    pub fn compact_libraries(&self) {
        if self.flush_system_unigrams().is_err() {
            return;
        }
        // Flag invariant: `has_library_offsets` is never false while a
        // LIBRARY_OFFSET row exists (true with no rows is harmless: the row
        // read gives 0). `library_offset` reads the flag without the lock,
        // so the flag is cleared only after the rows are gone, under the
        // database lock that every offset writer also holds, and stays set
        // if the write fails.
        let originals = self
            .inner
            .libpinyin
            .as_ref()
            .map(|target| target.originals());
        if originals.is_none() && !self.inner.has_library_offsets.load(Ordering::SeqCst) {
            return;
        }
        let db = self.database();
        let cleared = db.write(|txn| {
            if let Some(originals) = originals.as_ref() {
                for (&library, original) in originals.iter() {
                    txn.put(
                        SYSTEM_BASE,
                        &codec::encode_u8(library),
                        &codec::encode_u64(u64::from(original.item_total())),
                    )?;
                }
            }

            let mut keys = Vec::new();
            txn.for_each(LIBRARY_OFFSET, |key, _| {
                keys.push(key.to_vec());
                Ok(())
            })?;
            for key in keys {
                txn.remove(LIBRARY_OFFSET, &key)?;
            }
            Ok(())
        });
        if cleared.is_ok() {
            self.inner
                .has_library_offsets
                .store(false, Ordering::SeqCst);
        }
    }

    /// One-transaction §5 overlay for scoring `token` after `prev`.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the store cannot be read.
    pub fn count_delta(
        &self,
        prev: Option<Token>,
        token: Token,
    ) -> Result<UserCountDelta, UserStoreError> {
        let bumps = self.system_unigrams_for_read();
        let mut delta = if self.has_user_data() {
            self.with_count_cache(|cached, db| {
                let prev = prev.map(|prev| (prev, move || self.gram_in(db, prev)));
                cached.count_delta(db, prev, token)
            })?
        } else {
            UserCountDelta::ZERO
        };
        let (item, total) = bumps.as_ref().map_or((0, 0), |bumps| {
            (bumps.tokens.get(&token).copied().unwrap_or(0), bumps.total)
        });
        delta.unigram_delta = delta.unigram_delta.wrapping_add(item);
        delta.unigram_total_delta = delta
            .unigram_total_delta
            .wrapping_add(u64::from(self.facade_offset()))
            .wrapping_add(total);
        Ok(delta)
    }

    /// Record a training selection of `cur` after `last` (the `pinyin_train`
    /// path, §2). Returns the seed applied.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the observation cannot be committed,
    /// or [`UserStoreError::ShortUserBigramValue`] when `last`'s stored
    /// value is too short for its total (register row 85) or has a partial
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
        self.flush_system_unigrams()?;
        use crate::store_libpinyin::{pinyin_keys_to_packed, pronunciation_matches};
        if self.system_item_removed(token) {
            // `get_phrase_item` fails: there is no item to train (074a2219
            // `phrase_index.cpp:180-206`).
            return Ok(());
        }
        let original = self.inner.libpinyin.as_ref().and_then(|target| {
            target
                .originals()
                .get(&phrase_index_library_index(token))
                .and_then(|library| library.item(token & phrase::PHRASE_MASK))
        });
        let db = self.database();
        let mut items = self
            .inner
            .system_items
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut live = items.get(&token).cloned().or_else(|| original.clone());
        db.write(|txn| {
            // (row key, packed reading, running count, stored row value, and
            // for a live item the entry's place in it with its original count).
            // The item is a list: each entry keeps its own count
            // (`increase_pronunciation_possibility` walks every entry,
            // 074a2219 `phrase_index.cpp:114-143`), whatever its keys, so the
            // rows are matched to the entries by place and not by keys.
            let mut rows: Vec<TrainRow> = Vec::new();
            if let Some(item) = &live {
                for (place, (keys, frequency)) in item.prons.iter().enumerate() {
                    // Original system items store exact packed-key deltas.
                    // Appended items store absolute counts under dense keys.
                    let row_keys = if original.is_some() {
                        keys.clone()
                    } else {
                        let Some(keys) = crate::store_libpinyin::packed_to_pinyin_keys(keys) else {
                            continue;
                        };
                        keys
                    };
                    let key = codec::encode_token_bytes(token, &phrase::encode_keys(&row_keys));
                    let base = original.as_ref().map_or(0, |item| {
                        item.prons
                            .iter()
                            .find(|(reading, _)| reading == keys)
                            .map_or(0, |(_, count)| *count)
                    });
                    // The entry's count is the live one. A row that is there
                    // only gives the sequence and the index flag.
                    let mut stored = txn
                        .get(PRONUNCIATION, &key)?
                        .map(|bytes| PronValue::decode(&bytes))
                        .transpose()?
                        .unwrap_or(PronValue {
                            count: 0,
                            seq: u32::MAX,
                            indexed: false,
                        });
                    stored.count = u64::from(frequency.wrapping_sub(base));
                    rows.push((key, keys.clone(), *frequency, stored, Some((place, base))));
                }
            } else {
                for row in collect_pronunciations_from_txn(txn, token)? {
                    let Some(packed) = pinyin_keys_to_packed(&phrase::decode_keys(&row.key_bytes))
                    else {
                        continue;
                    };
                    let key = codec::encode_token_bytes(token, &row.key_bytes);
                    rows.push((key, packed, row.value.count as u32, row.value, None));
                }
            }
            for reading in readings {
                let mut total = 0_u32;
                for (key, packed, frequency, stored, entry) in &mut rows {
                    // 074a2219 phrase_index.cpp:122-139: accumulate each
                    // current count before matching; retain partial updates
                    // if the running total would overflow on this addition.
                    total = total.wrapping_add(*frequency);
                    if !pronunciation_matches(&reading, packed) {
                        continue;
                    }
                    let delta = seed as u32;
                    if delta > 0 && total > total.wrapping_add(delta) {
                        break;
                    }
                    *frequency = frequency.wrapping_add(delta);
                    total = total.wrapping_add(delta);
                    stored.count = match entry {
                        // The delta over the original, in `guint32` as the
                        // count is.
                        Some((_, base)) => u64::from(frequency.wrapping_sub(*base)),
                        None => stored.count.wrapping_add(seed),
                    };
                    txn.put(PRONUNCIATION, key, &stored.encode())?;
                }
            }
            if let Some(item) = &mut live {
                for (_, _, current, _, entry) in &rows {
                    if let Some((place, _)) = entry {
                        item.prons[*place].1 = *current;
                    }
                }
            }
            Ok(())
        })?;
        // Publish the same live item every reader borrows, only after the
        // delta transaction commits. Replayed/imported items already live
        // here; a first training materializes only the one touched item.
        if let Some(item) = live {
            items.insert(token, item);
            self.inner.has_system_items.store(true, Ordering::Relaxed);
        }
        drop(items);
        self.mark_committed_write(db, true);
        Ok(())
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
        self.flush_system_unigrams()?;
        let seed = seed::predicted_seed();
        let delta = seed::unigram_delta(seed) as u32;
        let db = self.database();
        let accepted = db
            .write(|txn| add_unigram_frequency(txn, phrase_index_library_index(cur), cur, delta))?;
        self.mark_committed_write(db, true);
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
        self.flush_system_unigrams()?;
        let db = self.database();
        let row = self.gram_in(&db, last)?;
        // Refuse before anything changes: the pin asserts on a total it
        // cannot read (`memory_chunk.h:390`, register row 85) and its
        // `get_freq`/`insert_freq`/`set_freq` walks read past a partial
        // item (`ngram.cpp:190`, `:241`, `:261`, register row 100).
        if row
            .as_deref()
            .is_some_and(|gram| gram.len() < single_gram::TOTAL_LEN || has_partial_tail(gram))
        {
            return Err(UserStoreError::ShortUserBigramValue);
        }
        let (seed, gram) = match policy {
            SeedPolicy::Training => trained_gram(row, cur)?,
            SeedPolicy::Predicted => (seed::predicted_seed(), Some(predicted_gram(row, cur)?)),
        };
        let session = self.inner.bigram_db.is_some();
        let accepted = db.write(|txn| {
            // `seed * unigram_factor` in `guint32`; the seeds are small.
            let delta = u32::try_from(seed::unigram_delta(seed)).unwrap_or(u32::MAX);
            let library = phrase_index_library_index(cur);
            if matches!(policy, SeedPolicy::Predicted)
                && !add_unigram_frequency(txn, library, cur, delta)?
            {
                // The unigram add comes first and its overflow ends the
                // call before any bigram is trained (`pinyin.cpp:2609-2612`);
                // the facade total has already taken the delta.
                return Ok(false);
            }
            if !session && let Some(gram) = &gram {
                TableGrams(txn).store(last, gram)?;
            }
            if matches!(policy, SeedPolicy::Training) {
                // `train_result3` ignores the sub-index's refusal
                // (`phonetic_lookup.h:928-929`): the item stops growing
                // once its library's `guint32` total would overflow.
                add_unigram_frequency(txn, library, cur, delta)?;
            }
            Ok(true)
        })?;
        // `m_user_bigram->store(last_token, user)` (`phonetic_lookup.h:904`,
        // `pinyin.cpp:2638`): one store of the predecessor's gram, skipped
        // by training's overflow guard.
        if accepted
            && session
            && let Some(gram) = &gram
        {
            self.store_gram(&db, last, gram)?;
        }
        self.mark_committed_write(db, true);
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
        phrase: &str,
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
        self.inner
            .libpinyin
            .as_ref()
            .is_none_or(|target| target.originals().layout().has_user_library(library))
    }

    /// [`Self::phrases`] as the user pinyin index sees them: the index also
    /// keeps entries no reading of the item behind them backs (a dropped
    /// item's, 074a2219 `phrase_index.cpp:261-268`, and a profile's), so a
    /// phrase that holds such a token now is found under them, as the pin's
    /// is once an import hands the token out again (`zhuyin.cpp:472-478`).
    /// Each such reading is added to the phrase indexed, with no count.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the store cannot be read.
    pub fn index_phrases(&self) -> Result<Vec<UserPhrase>, UserStoreError> {
        let mut phrases = self.phrases()?;
        let Some(target) = self.inner.libpinyin.as_ref() else {
            return Ok(phrases);
        };
        let unloaded = target
            .kept_index
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if unloaded.values().all(std::collections::BTreeSet::is_empty) {
            return Ok(phrases);
        }
        let by_token: std::collections::HashMap<Token, usize> = phrases
            .iter()
            .enumerate()
            .map(|(at, phrase)| (phrase.token(), at))
            .collect();
        for (token, packed) in unloaded.values().flatten() {
            let (Some(&at), Some(keys)) = (
                by_token.get(token),
                crate::store_libpinyin::packed_to_pinyin_keys(packed),
            ) else {
                continue;
            };
            phrases[at].index_reading(keys);
        }
        Ok(phrases)
    }

    /// Test only: the next library replacement's write is refused.
    #[cfg(test)]
    pub(crate) fn fail_next_replace(&self) {
        self.inner.fail_replace.store(true, Ordering::SeqCst);
    }

    /// The user pinyin index entries no item reading backs
    /// ([`Target::kept_index`]), for the save to write.
    pub(crate) fn kept_index_entries(&self) -> Vec<(Token, Vec<u16>)> {
        let Some(target) = self.inner.libpinyin.as_ref() else {
            return Vec::new();
        };
        let kept = target
            .kept_index
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        kept.values().flatten().cloned().collect()
    }

    /// The pin's `mask_out` takes the entries whose token is in the mask's
    /// range out of the user pinyin index, backed or not.
    fn clear_masked_kept_index(&self, mask: Token, value: Token) {
        let Some(target) = self.inner.libpinyin.as_ref() else {
            return;
        };
        let mut kept = target
            .kept_index
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for entries in kept.values_mut() {
            entries.retain(|(token, _)| (token & mask) != value);
        }
    }

    /// Add a phrase under `library` (`USER_DICTIONARY` or `NETWORK_DICTIONARY`).
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the phrase cannot be added.
    pub fn add_phrase_in(
        &mut self,
        library: u8,
        phrase: &str,
        keys: &[PinyinKey],
        count: Option<u64>,
    ) -> Result<Token, UserStoreError> {
        self.flush_system_unigrams()?;
        if !is_user_file_library(library)
            || !self.has_user_library(library)
            || !phrase::phrase_and_keys_valid(phrase, keys)
        {
            return Err(UserStoreError::InvalidPhrase);
        }
        // `_add_phrase`'s `count` reaches the item as a `guint32` delta
        // (`add_pronunciation(keys, count)`, `pinyin.cpp:579`, `:603`): the
        // C ABI hands its `gint`'s bit pattern over; a wider caller value
        // saturates.
        let delta = u32::try_from(count.unwrap_or(DEFAULT_PHRASE_COUNT)).unwrap_or(u32::MAX);
        let key_bytes = phrase::encode_keys(keys);

        let cache_enabled = self.inner.libpinyin.is_some();
        let mut import_readings = self.import_readings.take();
        let db = self.database();
        let generation = self.write_generation();
        discard_stale_import_readings(&mut import_readings, generation);
        let indexed_new = std::cell::Cell::new(false);
        let (token, count) = db.write(|txn| {
            let lib_key = codec::encode_u8_str(library, phrase);
            // _add_phrase selects existing items from the phrase table,
            // not the live item's reverse index (074a2219 pinyin.cpp:533-562).
            let mut tokens = Vec::new();
            if let Some(bytes) = txn.get(phrase_table::TABLE, &phrase_table::key(phrase))? {
                phrase_table::PhraseTableEntry::from_bytes(bytes)?.search(&mut tokens, |token| {
                    phrase_index_library_index(token) == library
                });
            }
            if tokens.len() > 1 {
                return Ok(Err(UserStoreError::InvalidPhrase));
            }
            let existing = tokens.first().copied();
            if let Some(token) = existing
                && txn.get(PHRASE, &codec::encode_token(token))?.as_deref()
                    != Some(codec::encode_str(phrase))
            {
                return Ok(Err(UserStoreError::InvalidPhrase));
            }

            if let Some(token) = existing {
                // The existing-item path (`pinyin.cpp:566-583`): the
                // reading is merged into the item — no unigram, and no
                // `add_index`, so a reading this adds stays out of the
                // user pinyin index.
                let count = if cache_enabled {
                    add_import_pronunciation(
                        txn,
                        token,
                        &key_bytes,
                        delta,
                        generation,
                        &mut import_readings,
                    )?
                } else {
                    add_pronunciation(txn, token, &key_bytes, delta, false)?
                };
                Ok(Ok((token, count)))
            } else {
                let alloc_key = codec::encode_u8(ALLOC_CURSOR);
                let lib_alloc_key = codec::encode_u8(library);
                let raw = if library == USER_DICTIONARY {
                    match txn.get(ALLOC, &lib_alloc_key)? {
                        Some(bytes) => codec::decode_token(&bytes)
                            .map_err(|_| StoreError::Backend("corrupt alloc cursor".into()))?,
                        None => match txn.get(ALLOC, &alloc_key)? {
                            Some(bytes) => codec::decode_token(&bytes)
                                .map_err(|_| StoreError::Backend("corrupt alloc cursor".into()))?,
                            None => FIRST_USER_TOKEN,
                        },
                    }
                } else {
                    match txn.get(ALLOC, &lib_alloc_key)? {
                        Some(bytes) => codec::decode_token(&bytes)
                            .map_err(|_| StoreError::Backend("corrupt alloc cursor".into()))?,
                        None => first_library_token(library),
                    }
                };
                let Some(token) = phrase::canonicalize_library_token(library, raw) else {
                    return Ok(Err(UserStoreError::TokenSpaceExhausted));
                };
                let Some(next) = phrase::next_library_token_after(library, token) else {
                    return Ok(Err(UserStoreError::TokenSpaceExhausted));
                };
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
                // pinyin.cpp:596 adds every new item to the user phrase table.
                phrase_table::add_index(txn, phrase, token)?;

                // The new-item path (`pinyin.cpp:585-607`): indexed
                // under its reading, then `add_unigram_frequency(token,
                // count * unigram_factor)` in `guint32` arithmetic.
                let pron_key = codec::encode_token_bytes(token, &key_bytes);
                let value = PronValue {
                    count: u64::from(delta),
                    seq: 0,
                    indexed: true,
                };
                txn.put(PRONUNCIATION, &pron_key, &value.encode())?;
                indexed_new.set(true);
                import_readings = cache_enabled.then(|| ImportReadings {
                    token,
                    generation,
                    rows: vec![PronRow {
                        key_bytes: key_bytes.clone(),
                        value,
                    }],
                });

                let unigram = delta.wrapping_mul(ADD_PHRASE_UNIGRAM_FACTOR_U32);
                add_unigram_frequency(txn, library, token, unigram)?;
                Ok(Ok((token, 1)))
            }
        })??;
        #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
        if indexed_new.get() {
            self.note_index_add(keys, token, false);
        }
        self.set_user_pronunciation_count(token, count);
        note_import_commit(&mut import_readings, generation);
        self.mark_committed_import_write(db, indexed_new.get());
        self.import_readings = import_readings;
        Ok(token)
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

        let db = self.database();
        let indexed = std::cell::RefCell::new(Vec::new());
        let inserted = std::cell::Cell::new(false);
        let (token, count) = db.write(|txn| {
            let lib_key = codec::encode_u8_str(ADDON_DICTIONARY, phrase);
            let existing = match txn.get(PHRASE_BY_LIB_TEXT, &lib_key)? {
                Some(bytes) => Some(
                    codec::decode_token(&bytes)
                        .map_err(|_| StoreError::Backend("corrupt lib-text token".into()))?,
                ),
                None => None,
            };
            let token = if let Some(token) = existing {
                token
            } else {
                let lib_alloc_key = codec::encode_u8(ADDON_DICTIONARY);
                let raw = match txn.get(ALLOC, &lib_alloc_key)? {
                    Some(bytes) => codec::decode_token(&bytes)
                        .map_err(|_| StoreError::Backend("corrupt alloc cursor".into()))?,
                    None => first_library_token(ADDON_DICTIONARY),
                };
                // `pinyin.cpp:2541-2544` takes `range.m_range_end` as is: unlike
                // `_add_phrase` (`:590-591`) there is no `token++` over a zero
                // low word, so the token is only checked to stay in the library.
                let token = raw;
                if phrase_index_library_index(token) != ADDON_DICTIONARY {
                    return Ok(Err(UserStoreError::TokenSpaceExhausted));
                }
                let Some(next) = phrase::next_library_token_after(ADDON_DICTIONARY, token) else {
                    return Ok(Err(UserStoreError::TokenSpaceExhausted));
                };
                txn.put(ALLOC, &lib_alloc_key, &codec::encode_token(next))?;

                txn.put(
                    PHRASE,
                    &codec::encode_token(token),
                    codec::encode_str(phrase),
                )?;
                txn.put(PHRASE_BY_LIB_TEXT, &lib_key, &codec::encode_token(token))?;
                phrase_table::add_index(txn, phrase, token)?;
                inserted.set(true);

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

            let mut n_prons = 0;
            for (keys, count) in valid {
                let key_bytes = phrase::encode_keys(keys);
                let pron_key = codec::encode_token_bytes(token, &key_bytes);
                let rows = collect_pronunciations_from_txn(txn, token)?;
                indexed.borrow_mut().push((
                    keys.as_slice(),
                    rows.iter()
                        .any(|row| row.key_bytes == key_bytes && row.value.indexed),
                ));
                n_prons =
                    rows.len() + usize::from(!rows.iter().any(|row| row.key_bytes == key_bytes));
                let value = match rows.iter().find(|row| row.key_bytes == key_bytes) {
                    Some(row) => PronValue {
                        count: row.value.count.saturating_add(*count),
                        ..row.value
                    },
                    None => PronValue {
                        count: *count,
                        seq: next_pron_seq(txn, token, &rows)?,
                        indexed: true,
                    },
                };
                txn.put(PRONUNCIATION, &pron_key, &value.encode())?;
            }
            Ok(Ok((token, n_prons)))
        })??;
        #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
        for (keys, exists) in indexed.into_inner() {
            self.note_index_add(keys, token, exists);
        }
        self.set_user_pronunciation_count(token, count);
        if inserted.get() {
            self.mark_committed_phrase_write(db, true);
        } else {
            self.mark_committed_reading_write(db, true);
        }
        Ok(token)
    }

    fn set_user_pronunciation_count(&self, token: Token, count: usize) {
        if let Some(target) = self.inner.libpinyin.as_ref() {
            target
                .pronunciation_counts
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(token, u32::try_from(count).unwrap_or(u32::MAX));
        }
    }

    /// Read the stored count of a live USER_FILE item in this session.
    /// This path skips phrase text and pronunciation records.
    #[must_use]
    pub fn user_pronunciation_count(&self, token: Token) -> Option<u32> {
        self.inner
            .libpinyin
            .as_ref()?
            .pronunciation_counts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&token)
            .copied()
    }

    /// The item's text, without decoding or collecting its readings.
    ///
    /// # Errors
    /// Returns a backend error or [`UserStoreError::Decode`] for non-UTF-8 text.
    pub fn phrase_text(&self, token: Token) -> Result<Option<String>, UserStoreError> {
        self.database()
            .get(PHRASE, &codec::encode_token(token))?
            .map(|bytes| String::from_utf8(bytes).map_err(|_| UserStoreError::Decode))
            .transpose()
    }

    /// Phrase text and pronunciations for `token`, if this store owns it.
    ///
    /// # Errors
    ///
    /// [`UserStoreError::Decode`] when the stored text is not UTF-8,
    /// [`UserStoreError::DecodePronunciation`] when one of its pronunciation
    /// rows does not decode, otherwise the backend's own error: the two
    /// decode failures name a present item that is malformed, the rest a
    /// store that cannot be read.
    pub fn phrase(&self, token: Token) -> Result<Option<UserPhrase>, UserStoreError> {
        let db = self.database();
        let token_key = codec::encode_token(token);
        let Some(text_bytes) = db.get(PHRASE, &token_key)? else {
            return Ok(None);
        };
        // The get already handed over an owned buffer: validate it as UTF-8
        // in place instead of decoding to `&str` and copying a second time.
        let text = String::from_utf8(text_bytes).map_err(|_| UserStoreError::Decode)?;
        let pronunciations = collect_pronunciations_from_store(&*db, token)?;
        Ok(Some(UserPhrase::new(token, text, pronunciations)))
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
        let db = self.database();
        let lib_key = codec::encode_u8_str(library, phrase);
        if let Some(bytes) = db.get(PHRASE_BY_LIB_TEXT, &lib_key)? {
            return codec::decode_token(&bytes)
                .map(Some)
                .map_err(|_| UserStoreError::Decode);
        }
        if library == USER_DICTIONARY {
            let text_key = codec::encode_str(phrase);
            if let Some(bytes) = db.get(PHRASE_BY_TEXT, text_key)? {
                return codec::decode_token(&bytes)
                    .map(Some)
                    .map_err(|_| UserStoreError::Decode);
            }
        }
        Ok(None)
    }

    /// The pin's user phrase-table search, preserving stored token multiplicity.
    ///
    /// # Errors
    /// Returns an error when the record cannot be read.
    pub fn phrase_table_tokens_for_text(&self, text: &str) -> Result<Vec<Token>, UserStoreError> {
        Ok(phrase_table::search(&*self.database(), text)?.tokens)
    }

    pub(crate) fn phrase_table_records(
        &self,
    ) -> Result<std::collections::BTreeMap<Vec<u8>, Vec<u8>>, UserStoreError> {
        let mut records = std::collections::BTreeMap::new();
        self.database()
            .for_each(phrase_table::TABLE, |key, value| {
                records.insert(key.to_vec(), value.to_vec());
                Ok(())
            })?;
        Ok(records)
    }

    /// Current write generation: moves on every committed write, counts
    /// included.
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.write_generation()
    }

    /// Current phrase generation: moves only when a phrase or
    /// pronunciation row was added, imported, or removed. [`UserLookup`]
    /// rebuilds when this changes and on nothing else.
    #[must_use]
    pub fn phrase_generation(&self) -> u64 {
        self.inner.phrase_generation.load(Ordering::Acquire)
    }

    /// Current exact-text row generation. New tokens, removals, masks and
    /// library replacements move it. Pronunciation-only writes do not.
    #[must_use]
    pub fn phrase_table_generation(&self) -> u64 {
        self.inner.phrase_table_generation.load(Ordering::Acquire)
    }

    /// Next token the store will allocate.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the store cannot be read.
    pub fn next_user_token(&self) -> Result<Token, UserStoreError> {
        let db = self.database();
        let alloc_key = codec::encode_u8(ALLOC_CURSOR);
        db.get(ALLOC, &alloc_key)?.map_or_else(
            || Ok(FIRST_USER_TOKEN),
            |bytes| codec::decode_token(&bytes).map_err(|_| UserStoreError::Decode),
        )
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
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the store cannot be read.
    pub fn export_phrases(&self) -> Result<Vec<ExportedPhrase>, UserStoreError> {
        self.export_phrases_in(USER_DICTIONARY)
    }

    /// Export rows for one `USER_FILE` nibble.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the store cannot be read.
    pub fn export_phrases_in(&self, library: u8) -> Result<Vec<ExportedPhrase>, UserStoreError> {
        let db = self.database();
        let mut rows = Vec::new();
        db.for_each(PHRASE, |k, v| {
            let token = codec::decode_token(k)
                .map_err(|_| StoreError::Backend("corrupt phrase token".into()))?;
            if phrase_index_library_index(token) != library {
                return Ok(());
            }
            let text = codec::decode_str(v)
                .map_err(|_| StoreError::Backend("corrupt phrase text".into()))?;
            // Buffer rows so pronunciations are collected after iteration,
            // when the store is no longer borrowed by this callback.
            rows.push((token, text.to_owned()));
            Ok(())
        })?;
        let tokens: std::collections::HashSet<Token> =
            rows.iter().map(|(token, _)| *token).collect();
        let pronunciations = collect_pronunciations_for_tokens(&*db, &tokens)?;
        let mut out = Vec::new();
        for (token, text) in rows {
            for pronunciation in pronunciations.get(&token).into_iter().flatten() {
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
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the store cannot be read.
    pub fn phrases(&self) -> Result<Vec<UserPhrase>, UserStoreError> {
        let db = self.database();
        let mut tokens_and_texts = Vec::new();
        db.for_each(PHRASE, |k, v| {
            let token = codec::decode_token(k)
                .map_err(|_| StoreError::Backend("corrupt phrase token".into()))?;
            let text = codec::decode_str(v)
                .map_err(|_| StoreError::Backend("corrupt phrase text".into()))?
                .to_owned();
            tokens_and_texts.push((token, text));
            Ok(())
        })?;
        let tokens: std::collections::HashSet<Token> =
            tokens_and_texts.iter().map(|(token, _)| *token).collect();
        let pronunciations = collect_pronunciations_for_tokens(&*db, &tokens)?;
        let mut out = Vec::new();
        for (token, text) in tokens_and_texts {
            let pronunciations = pronunciations.get(&token).cloned().unwrap_or_default();
            out.push(UserPhrase::new(token, text, pronunciations));
        }
        Ok(out)
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

    /// Every stored user-bigram item as `(prev, cur, count)`: the
    /// predecessors in the user bigram's walk order ([`Self::bigram_predecessors`]),
    /// each gram's items in stored order.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the store cannot be read.
    pub fn export_bigrams(&self) -> Result<Vec<(Token, Token, u64)>, UserStoreError> {
        let mut rows = Vec::new();
        for prev in self.bigram_predecessors()? {
            for (cur, count) in self.bigram_successors(prev)? {
                rows.push((prev, cur, count));
            }
        }
        Ok(rows)
    }

    /// Whether this session has a user-directory argument, including "".
    /// A transient NULL-user session has mutable state but no directory.
    #[must_use]
    pub fn has_user_directory(&self) -> bool {
        self.inner
            .libpinyin
            .as_ref()
            .is_some_and(|target| target.dir.is_some())
    }

    /// The `pinyin_save` write side (§4).
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the store cannot be flushed to disk.
    pub fn save(&mut self) -> Result<bool, UserStoreError> {
        if !self.is_modified() {
            return Ok(false);
        }
        // The libpinyin branch: export the session values and write the
        // pin's whole file set (`.tmp` + rename). The session store's
        // compaction below still runs — it keeps the in-memory session
        // tidy — but the durable write is the file set, not the session.
        if let Some(target) = self.inner.libpinyin.clone() {
            let Some(dir) = target.dir.as_deref() else {
                return Ok(false);
            };
            self.compact_libraries();
            // Arc clone: the originals hold every system item (~138k
            // ChunkItems) and a dirty save must not copy them — both
            // halves take the Arc by reference.
            let originals = target.originals();
            let state = crate::store_libpinyin::export_state(self, &originals)?;
            let bigram_db = self
                .inner
                .bigram_db
                .as_ref()
                .map(|db| db.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
            let kept = target.index_file_keys();
            crate::persistence::save_session(
                dir,
                crate::persistence::SaveSource {
                    state: &state,
                    kept: &kept,
                    compacted: &[],
                },
                &originals,
                &target.versions,
                target.open_counter,
                bigram_db.as_deref(),
            )?;
            drop(bigram_db);
        }
        // Dropping the cache is no longer forced by the backend — no read
        // view outlives a call, so nothing pins pages against compaction —
        // but it is kept so `save` leaves exactly the state it always did.
        let mut cache = self.count_cache();
        *cache = None;
        let mut db = self.database();
        db.compact()?;
        drop(db);
        drop(cache);
        self.inner.dirty.store(false, Ordering::Relaxed);
        Ok(true)
    }

    /// [`Self::save`] the way the pin saves, with the same directory and
    /// modified guards: the file set is written and
    /// renamed and the marker rewritten whatever fails on the way, and the
    /// failures come back in the [`SaveReport`] instead of ending the save.
    /// A store with no libpinyin target (no directory to write) reports
    /// nothing. A completed save leaves the store clean, as after
    /// [`Self::save`]; a guard return leaves its state unchanged.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the session values cannot be
    /// exported or encoded, or the in-memory session store cannot be
    /// compacted — failures the pin has no counterpart for. A filesystem
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
        self.save_reporting_inner(compacted, None::<crate::persistence::NoBaseline>)
    }

    /// [`Self::save_reporting_compacted`] with the current-on-disk baseline
    /// hook. The store calls it once per selected system library, in the
    /// pin's `_write_files` order, immediately before that library's log is
    /// staged; the hook maps the current chunk and emits any layout
    /// critical at that point (`pinyin.cpp:956`, `zhuyin.cpp:589`).
    pub fn save_reporting_with_baseline(
        &mut self,
        compacted: &[u8],
        baseline: impl FnMut(u8) -> crate::persistence::SaveBaseline,
    ) -> Result<SaveReport, UserStoreError> {
        self.save_reporting_inner(compacted, Some(baseline))
    }

    fn save_reporting_inner<B: FnMut(u8) -> crate::persistence::SaveBaseline>(
        &mut self,
        compacted: &[u8],
        baseline: Option<B>,
    ) -> Result<SaveReport, UserStoreError> {
        let mut report = SaveReport::default();
        if !self.is_modified() {
            return Ok(report);
        }
        if let Some(target) = self.inner.libpinyin.clone() {
            let Some(dir) = target.dir.as_deref() else {
                return Ok(report);
            };
            self.compact_libraries();
            let originals = target.originals();
            let state = crate::store_libpinyin::export_state(self, &originals)?;
            let bigram_db = self
                .inner
                .bigram_db
                .as_ref()
                .map(|db| db.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
            let kept = target.index_file_keys();
            {
                let mut reporting = crate::persistence::SaveReporting {
                    versions: &target.versions,
                    open_counter: target.open_counter,
                    bigram_db: bigram_db.as_deref(),
                    interleaved: target.law == crate::persistence::UserConfLaw::Zhuyin,
                    lenient: true,
                    baseline,
                };
                report = crate::persistence::save_reporting(
                    dir,
                    crate::persistence::SaveSource {
                        state: &state,
                        kept: &kept,
                        compacted,
                    },
                    &originals,
                    &mut reporting,
                )?;
            }
            drop(bigram_db);
        }
        let mut cache = self.count_cache();
        *cache = None;
        let mut db = self.database();
        db.compact()?;
        drop(db);
        drop(cache);
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

    /// `Bigram::mask_out(mask, value)` on the user bigram with the
    /// database guard held: on the pin's container for a libpinyin session
    /// ([`mask_grams`]); a plain store masks its [`BIGRAM_GRAM`] rows inside
    /// its own write transaction instead. Answers whether a gram remains.
    fn mask_container(&self, mask: Token, value: Token) -> Result<bool, UserStoreError> {
        let Some(container) = self.container() else {
            return Ok(false);
        };
        Ok(mask_grams(&mut ContainerGrams(&container), mask, value)?)
    }

    /// Reads and validates every loaded `SYSTEM_FILE`/`DICTIONARY`
    /// library's user `.dbin` from the user directory, before a mask
    /// touches any state. Returns the decoded snapshot replay consumes,
    /// without a second file read. `visible` names the libraries the runtime still
    /// holds: the pin's `mask_out` walks `index` 1..`PHRASE_INDEX_LIBRARY_
    /// COUNT` and `continue`s when `get_range` answers
    /// `ERROR_NO_SUB_PHRASE_INDEX` (`pinyin.cpp:1235-1241`,
    /// `zhuyin.cpp:770-776`), so an unloaded library's `.dbin` is never
    /// merged and must not refuse the mask.
    /// A library that fails `merge_with_mask`'s library-bit check
    /// (`phrase_index.cpp:309-313`) also never reads its log.
    ///
    /// The pin's `merge_with_mask` (`pinyin.cpp:1273-1285`,
    /// `zhuyin.cpp:808-820`) `_peek_header`s each merged log, whose
    /// `assert(1 >= header_count)` (`phrase_index.cpp:745`) and
    /// `assert(token == null_token)` (`phrase_index_logger.h:202`) are the
    /// pin's abort sites. Reading the file here, rather than a load-time
    /// snapshot, also covers a `.dbin` corrupted after a successful init.
    ///
    /// # Errors
    ///
    /// [`UserStoreError::SystemLogHeaderToken`] for a non-null-token
    /// `MODIFY_HEADER` and [`UserStoreError::SystemLogMultipleHeaders`] for
    /// more than one `MODIFY_HEADER` record, or
    /// [`UserStoreError::SystemLogSurvivingAdd`] for an ADD record that
    /// survives a header-bearing masked log (`NULL == oldone`,
    /// `phrase_index_logger.h:245`). A missing, unreadable, or
    /// otherwise malformed `.dbin` is the pin's graceful empty-log path and
    /// is not an error.
    pub fn validate_system_logs(
        &self,
        visible: impl Fn(u8) -> bool,
        mask: Token,
        value: Token,
    ) -> Result<crate::persistence::SystemLogSnapshot, UserStoreError> {
        let mut snapshot = crate::persistence::SystemLogSnapshot::default();
        let Some(target) = self.inner.libpinyin.as_ref() else {
            return Ok(snapshot);
        };
        let Some(dir) = target.dir.as_deref() else {
            return Ok(snapshot);
        };
        let originals = target.originals();
        for (nibble, file) in originals.layout().system_logs() {
            if !visible(*nibble)
                || (*nibble & phrase_index_library_index(mask)) != phrase_index_library_index(value)
            {
                continue;
            }
            let path = file.under(dir);
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            let Ok(payload) = read_chunk_payload(&bytes) else {
                continue;
            };
            // Count the headers decoded before any malformed tail: the
            // pin's `_peek_header` accumulates them until `next_record`
            // answers false and then asserts on the total
            // (`phrase_index.cpp:721-746`), so two valid headers followed
            // by a bad record still abort there.
            let (records, error) = decode_log_records_partial(payload);
            let headers = records
                .iter()
                .filter(|record| matches!(record, LogRecord::ModifyHeader { .. }))
                .count();
            // The non-null-token assert fires while parsing the record,
            // before the post-loop header-count assert.
            if error.is_some_and(|error| error.non_null_header_token().is_some()) {
                return Err(UserStoreError::SystemLogHeaderToken);
            }
            if headers > 1 {
                return Err(UserStoreError::SystemLogMultipleHeaders);
            }
            // Without a header, _peek_header returns an empty logger.
            // Otherwise _mask_out_records passes &oldchunk even for ADD,
            // whose writer asserts NULL == oldone (074a2219 :245).
            if headers == 1
                && records.iter().any(|record| {
                    matches!(record, LogRecord::Add { token, .. }
                    if (token & (mask & PHRASE_MASK)) != (value & PHRASE_MASK))
                })
            {
                return Err(UserStoreError::SystemLogSurvivingAdd);
            }
            snapshot.records.insert(*nibble, records);
        }
        Ok(snapshot)
    }

    /// Replaces the loaded system sub-index overlays with the validated
    /// disk-log snapshot over the freshly mapped originals. Live item and
    /// pronunciation rows are rebuilt; independent phrase-table membership
    /// remains in place, separate from the sub-index
    /// (`pinyin.cpp:1253-1285`, `zhuyin.cpp:788-820` at 074a2219).
    ///
    /// # Errors
    /// Returns [`UserStoreError`] when the session values cannot be written.
    pub fn reload_system_libraries(
        &mut self,
        originals: crate::persistence::SystemOriginals,
        mask: Token,
        value: Token,
        logs: crate::persistence::SystemLogSnapshot,
    ) -> Result<(), UserStoreError> {
        if self.inner.libpinyin.is_none() {
            return Ok(());
        }
        let (state, library_totals) =
            crate::persistence::load_masked_logs(&originals, mask, value, logs);
        self.replace_system_libraries(originals, LibraryChange::System, &state, &library_totals)
    }

    /// `_load_phrase_library`'s system branch for the libraries in
    /// `originals`: each is mapped again and only its saved `.dbin` is
    /// merged, so everything the session changed since the last save
    /// (trained unigrams, imported readings and items, removals) goes with
    /// the sub-index `unload` freed (074a2219 `pinyin.cpp:264-277`,
    /// `phrase_index.cpp:261-268`; `zhuyin.cpp:298-312`). The independent
    /// phrase-table membership stays in place, separate from the
    /// sub-index.
    ///
    /// # Errors
    /// Returns [`UserStoreError`] when the session values cannot be written.
    pub fn load_system_libraries(
        &mut self,
        originals: crate::persistence::SystemOriginals,
    ) -> Result<(), UserStoreError> {
        let Some(dir) = self
            .inner
            .libpinyin
            .as_ref()
            .and_then(|target| target.dir.clone())
        else {
            return Ok(());
        };
        let (state, library_totals) = crate::persistence::load_library_logs(&dir, &originals);
        self.replace_system_libraries(originals, LibraryChange::System, &state, &library_totals)
    }

    /// `FacadePhraseIndex::unload` for the libraries in `originals`: the
    /// sub-index goes with everything the session changed in it and with
    /// its share of the facade total, whatever was saved (074a2219
    /// `phrase_index.cpp:261-268`). The user index tables keep their
    /// entries.
    ///
    /// # Errors
    /// Returns [`UserStoreError`] when the session values cannot be written.
    pub fn unload_system_libraries(
        &mut self,
        originals: crate::persistence::SystemOriginals,
    ) -> Result<(), UserStoreError> {
        self.replace_system_libraries(
            originals,
            LibraryChange::System,
            &crate::persistence::UserState::default(),
            &std::collections::BTreeMap::new(),
        )
    }

    /// `_load_phrase_library`'s `USER_FILE` branch for `library`: its
    /// sub-index comes back from the saved chunk, or empty when there is
    /// none, so everything the session changed since the last save goes
    /// (074a2219 `pinyin.cpp:303-321`, `zhuyin.cpp:247-265`). The phrase
    /// table and the user pinyin index keep their entries, separate from
    /// the sub-index: a reading the index carried stays indexed on the item
    /// the chunk brings back.
    ///
    /// # Errors
    /// Returns [`UserStoreError`] when the session values cannot be written.
    pub fn load_user_library(&mut self, library: u8) -> Result<(), UserStoreError> {
        let Some((dir, layout)) = self.inner.libpinyin.as_ref().and_then(|target| {
            target
                .dir
                .clone()
                .map(|dir| (dir, target.originals().layout().clone()))
        }) else {
            return Ok(());
        };
        if !layout.has_user_library(library) {
            return Ok(());
        }
        let (state, library_totals) = crate::persistence::load_user_library(&dir, &layout, library);
        self.replace_system_libraries(
            crate::persistence::SystemOriginals::new(Default::default(), layout),
            LibraryChange::UserLoad(library),
            &state,
            &library_totals,
        )
    }

    /// `FacadePhraseIndex::unload` for the `USER_FILE` `library`: the
    /// sub-index goes with everything in it and with its share of the
    /// facade total, saved or not (074a2219 `phrase_index.cpp:261-268`).
    /// The phrase table and the user pinyin index keep their entries.
    ///
    /// # Errors
    /// Returns [`UserStoreError`] when the session values cannot be written.
    pub fn unload_user_library(&mut self, library: u8) -> Result<(), UserStoreError> {
        let Some(layout) = self
            .inner
            .libpinyin
            .as_ref()
            .map(|target| target.originals().layout().clone())
        else {
            return Ok(());
        };
        if !layout.has_user_library(library) {
            return Ok(());
        }
        self.replace_system_libraries(
            crate::persistence::SystemOriginals::new(Default::default(), layout),
            LibraryChange::UserUnload(library),
            &crate::persistence::UserState::default(),
            &std::collections::BTreeMap::new(),
        )
    }

    fn replace_system_libraries(
        &mut self,
        originals: crate::persistence::SystemOriginals,
        change: LibraryChange,
        state: &crate::persistence::UserState,
        library_totals: &std::collections::BTreeMap<u8, u32>,
    ) -> Result<(), UserStoreError> {
        self.flush_system_unigrams()?;
        let Some(target) = self.inner.libpinyin.clone() else {
            return Ok(());
        };
        let user_library = change.user_library();
        let reloaded = |token| {
            let library = phrase_index_library_index(token);
            originals.contains_key(&library) || user_library == Some(library)
        };
        let mut target_originals = target
            .originals
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let db = self.database();
        // Flag invariant (see `compact_libraries`): set before the rows can
        // exist, and cleared below only once the transaction leaves none,
        // still under the database lock.
        self.inner.has_library_offsets.store(true, Ordering::SeqCst);
        let (has_user_data, moved, offsets_left, pending) = db.write(|txn| {
            // `FacadePhraseIndex::load` and `merge_with_mask` take the old
            // sub-index's total out of the facade and put the new one's in
            // (`phrase_index.cpp:237-248`, `:301-332`): each reloaded
            // library's difference leaves with it, and the one its masked
            // replay leaves comes in.
            let mut moved = 0_u32;
            for nibble in originals.keys().chain(user_library.as_ref()) {
                let key = codec::encode_u8(*nibble);
                let old = txn_get_u64_or(txn, LIBRARY_OFFSET, &key, 0)?;
                moved = moved.wrapping_sub(old as u32);
                txn.remove(LIBRARY_OFFSET, &key)?;
            }
            let indexed = remove_reloaded_rows(txn, reloaded, user_library)?;
            let (state, pending) = kept_index_state(&target, &originals, change, indexed, state);
            let reseeded =
                crate::store_libpinyin::seed_txn(txn, &state, &originals, library_totals)?;
            #[cfg(test)]
            if self.inner.fail_replace.swap(false, Ordering::SeqCst) {
                return Err(StoreError::Backend("injected write failure".into()));
            }
            // The sub-index starts again from its items: the next token is
            // one past the highest the chunk brought back, or the first.
            if let Some(library) = user_library {
                rederive_alloc_cursors(txn, &[library])?;
            }
            moved = reseeded
                .values()
                .fold(moved, |moved, &offset| moved.wrapping_add(offset));
            let offsets_left = !txn.is_empty(LIBRARY_OFFSET)?;
            store_unigram_total(txn)?;
            Ok((
                has_user_data_in_write_txn(txn)?,
                moved,
                offsets_left,
                pending,
            ))
        })?;
        if let Some(library) = user_library {
            let mut counts = target
                .pronunciation_counts
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            counts.retain(|&token, _| phrase_index_library_index(token) != library);
            if matches!(change, LibraryChange::UserLoad(_)) {
                counts.extend(crate::store_libpinyin::loaded_pronunciation_counts(
                    state,
                    target.dir.as_deref(),
                    originals.layout(),
                    Some(library),
                ));
            }
        }
        // The write took: now the index entries it set aside are kept.
        if let Some((library, entries)) = pending {
            target
                .kept_index
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(library, entries);
        }
        self.inner.facade_offset.fetch_add(moved, Ordering::SeqCst);
        self.inner
            .has_library_offsets
            .store(offsets_left, Ordering::SeqCst);
        {
            let mut items = self
                .inner
                .system_items
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let mut removed = self.removed_system_items_guard();
            items.retain(|&token, _| !reloaded(token));
            removed.retain(|&token| !reloaded(token));
            for (&nibble, overrides) in &state.system_overrides {
                for (&slot, item) in overrides {
                    let token = (u32::from(nibble) << 24) | slot;
                    if let Some(item) = item {
                        items.insert(token, item.clone());
                    } else {
                        removed.insert(token);
                    }
                }
            }
            self.inner
                .has_system_items
                .store(!items.is_empty() || !removed.is_empty(), Ordering::Relaxed);
            self.inner
                .has_removed_system_items
                .store(!removed.is_empty(), Ordering::Relaxed);
        }
        target_originals.extend(
            originals
                .iter()
                .map(|(&nibble, library)| (nibble, library.clone())),
        );
        drop(target_originals);
        // A session's grams live in the container, not in the tables the
        // transaction probed: they are user data too.
        let has_grams = self
            .container()
            .is_some_and(|container| container.keys().is_ok_and(|keys| !keys.is_empty()));
        self.mark_committed_phrase_write(db, has_user_data || has_grams);
        Ok(())
    }

    fn clear_masked_system_items(&self, mask: Token, value: Token) {
        let mut items = self
            .inner
            .system_items
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        items.retain(|&token, _| (token & mask) != value);
        let mut removed = self.removed_system_items_guard();
        removed.retain(|&token| (token & mask) != value);
        self.inner
            .has_system_items
            .store(!items.is_empty() || !removed.is_empty(), Ordering::Relaxed);
        self.inner
            .has_removed_system_items
            .store(!removed.is_empty(), Ordering::Relaxed);
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
        self.flush_system_unigrams()?;
        if self.has_overlong_index_key() {
            return Err(UserStoreError::OverlongIndexKey);
        }
        // The bigram walk's faults, in the pin's order: a non-token key
        // (`get_all_items`), then a short value (`get_total_freq`), then a
        // mask residual (`get_length`).
        if let Some(error) = self.bigram_mask_fault(mask, value) {
            return Err(error);
        }
        #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
        let walk = self.pinyin_mask_records(mask, value)?;
        let db = self.database();
        let session = self.inner.libpinyin.is_some();
        let container = self.inner.bigram_db.is_some();
        let (has_user_data, index_rows) = db.write(|txn| {
            // The pin's user index tables keep every key the mask empties
            // (`IndexKeys`). Every row the store holds now is one the
            // tables hold, and the mask and the system reload after it
            // only take rows away, so keep the keys of all of them.
            let index_rows = if session {
                IndexRows::read(txn, |_| true)?
            } else {
                IndexRows::default()
            };
            // `m_user_bigram->mask_out(mask, value)` (`pinyin.cpp:1230`,
            // `zhuyin.cpp:765`); a session's grams are masked below.
            if !container {
                mask_grams(&mut TableGrams(txn), mask, value)?;
            }

            // Unigram deltas and their running total.
            let mut unigram_rows: Vec<(Token, u64)> = Vec::new();
            txn.for_each(UNIGRAM, |k, v| {
                let token = codec::decode_token(k)
                    .map_err(|_| StoreError::Backend("corrupt unigram key".into()))?;
                let delta = codec::decode_u64(v)
                    .map_err(|_| StoreError::Backend("corrupt unigram value".into()))?;
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

            mask_phrase_rows(txn, mask, value)?;
            rederive_alloc_cursors(txn, &[])?;
            Ok((has_user_data_in_write_txn(txn)?, index_rows))
        })?;
        self.keep_index_rows(&index_rows);
        #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
        if let (Some((before, after)), Some(target)) = (walk, self.inner.libpinyin.as_ref()) {
            target
                .index_file_keys()
                .mask_walk(&before, &after, mask, value);
        }
        // Commit succeeded: discard matched in-memory system payloads too.
        // Dropping an original's override restores it; dropping an appended
        // item removes its only payload. Do this before publishing the epoch.
        if let Some(target) = self.inner.libpinyin.as_ref() {
            target
                .pronunciation_counts
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .retain(|&token, _| (token & mask) != value);
        }
        self.clear_masked_system_items(mask, value);
        self.clear_masked_kept_index(mask, value);
        let grams_left = self.mask_container(mask, value)?;
        self.mark_committed_phrase_write(db, has_user_data || grams_left);
        Ok(())
    }

    /// `pinyin_remove_user_candidate`'s store side (§3.4).
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the phrase cannot be removed.
    pub fn remove_user_phrase(&mut self, token: Token) -> Result<bool, UserStoreError> {
        // `pinyin_remove_user_candidate` reaches `user_bigram->mask_out`
        // (`pinyin.cpp:3766`) after its phrase-index removals. Those are
        // in-memory and die with the pin's abort, so a failed call must
        // leave the store unchanged: refuse the recorded bigram faults
        // before any removal.
        if let Some(error) = self.bigram_mask_fault(PHRASE_INDEX_LIBRARY_MASK | PHRASE_MASK, token)
        {
            return Err(error);
        }
        let db = self.database();
        let container = self.inner.bigram_db.is_some();
        let result: Result<Option<(bool, IndexRows)>, UserStoreError> = db
            .write(|txn| {
                let token_key = codec::encode_token(token);
                // `phrase_index->remove_phrase_item` (`pinyin.cpp:3743`):
                // no item under the token, so the pin asserts.
                let Some(text_bytes) = txn.get(PHRASE, &token_key)? else {
                    return Ok(None);
                };
                // As in `phrase`: the get hands over an owned buffer, so
                // validate it in place rather than copying it a second time.
                let text = String::from_utf8(text_bytes)
                    .map_err(|_| StoreError::Backend("corrupt phrase text".into()))?;

                // `pinyin_table->remove_index` for every reading
                // (`pinyin.cpp:3759`): a reading merged into an existing
                // phrase was never indexed (`pinyin.cpp:569-582`), so the
                // pin asserts when it cannot drop one.
                let readings = collect_pronunciations_from_txn(txn, token)?;
                if readings.iter().any(|row| !row.value.indexed) {
                    return Ok(None);
                }
                // Validate the known abort case before publishing any table
                // mutation, preserving this API's failed-call behavior.
                // pinyin.cpp:3750 removes only the first matching stored token.
                if phrase_table::remove_index(txn, &text, token)? != phrase_table::ERROR_OK {
                    return Ok(None);
                }

                // Both `remove_index` calls store the emptied entry back
                // (`IndexKeys`): the phrase's keys stay in the tables.
                let index_rows = IndexRows::of_phrase(token, &readings);

                txn.remove(PHRASE, &token_key)?;
                if phrase_index_library_index(token) == USER_DICTIONARY {
                    txn.remove(PHRASE_BY_TEXT, codec::encode_str(&text))?;
                }
                txn.remove(
                    PHRASE_BY_LIB_TEXT,
                    &codec::encode_u8_str(phrase_index_library_index(token), &text),
                )?;
                remove_pronunciations(txn, token)?;

                // `user_bigram->mask_out(PHRASE_INDEX_LIBRARY_MASK |
                // PHRASE_MASK, token)` (`pinyin.cpp:3764-3766`); a
                // session's grams are masked below.
                if !container {
                    mask_grams(
                        &mut TableGrams(txn),
                        PHRASE_INDEX_LIBRARY_MASK | PHRASE_MASK,
                        token,
                    )?;
                }

                // Unigram: recompute total excluding the removed token.
                let mut kept_sum = 0_u64;
                txn.for_each(UNIGRAM, |k, v| {
                    let candidate = codec::decode_token(k)
                        .map_err(|_| StoreError::Backend("corrupt unigram key".into()))?;
                    if candidate != token {
                        let delta = codec::decode_u64(v)
                            .map_err(|_| StoreError::Backend("corrupt unigram value".into()))?;
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

                rederive_alloc_cursors(txn, &[])?;
                let has = has_user_data_in_write_txn(txn)?;
                Ok(Some((has, index_rows)))
            })
            .map_err(UserStoreError::from);
        result?.map_or_else(
            || Ok(false),
            |(has_user_data, index_rows)| {
                self.keep_index_rows(&index_rows);
                #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
                for keys in index_rows.readings() {
                    self.note_index_remove(keys, token);
                }
                if let Some(target) = self.inner.libpinyin.as_ref() {
                    target
                        .pronunciation_counts
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .remove(&token);
                }
                let grams_left =
                    self.mask_container(PHRASE_INDEX_LIBRARY_MASK | PHRASE_MASK, token)?;
                self.mark_committed_phrase_write(db, has_user_data || grams_left);
                Ok(true)
            },
        )
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
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the store cannot be opened at `path`.
    pub fn open(path: &Path) -> Result<Self, UserStoreError> {
        let key = registry::registry_key(path);
        let mut reg = registry::lock_registry();
        if let Some(inner) = reg.get(&key).and_then(std::sync::Weak::upgrade) {
            return Ok(Self {
                _fini: FiniGuard::disarmed(),
                inner,
                _standalone_lease: None,
                training: TrainingCheck::default(),
                import_readings: None,
                _lease: RegistryLease,
            });
        }

        let db = DefaultStore::create(path)?;
        let inner = Self::init_and_wrap(db)?;
        reg.insert(key, Arc::downgrade(&inner));
        Ok(Self {
            _fini: FiniGuard::disarmed(),
            inner,
            _standalone_lease: None,
            training: TrainingCheck::default(),
            import_readings: None,
            _lease: RegistryLease,
        })
    }
}

/// Entry cap per count-memo map. Bounds memo memory to O(cap) no matter
/// how many distinct keys a session scores against; the value sits far
/// above any trained store's working set. On overflow the map resets —
/// entries are pure speed hints, so wholesale eviction changes nothing
/// but repeat-read cost.
const COUNT_MEMO_MAX_ENTRIES: usize = 8192;

/// Inserts into a count-memo map, resetting it first once at capacity.
fn memo_insert<K: std::hash::Hash + Eq, V>(map: &mut FixedHashMap<K, V>, key: K, value: V) {
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
            unigram: FixedHashMap::default(),
            unigram_total: None,
            bigram_rows: FixedHashMap::default(),
        }
    }

    /// `UNIGRAM[token]`, memoised. A present row is memoised whatever its
    /// value, including an explicit `0`; an absent row memoises as `0`.
    fn unigram(&mut self, db: &impl ReadStore, token: Token) -> Result<u64, UserStoreError> {
        if let Some(&hit) = self.unigram.get(&token) {
            return Ok(hit);
        }
        // An absent row memoises as `0`, as the type's doc says: every
        // committed write retires the whole cache, so the memo cannot
        // outlive a row that a later write creates.
        let value = get_u64(db, UNIGRAM, &codec::encode_token(token))?.unwrap_or(0);
        memo_insert(&mut self.unigram, token, value);
        Ok(value)
    }

    /// `UNIGRAM_TOTAL`'s single row, memoised. Absent reads as `0`.
    fn unigram_total(&mut self, db: &impl ReadStore) -> Result<u64, UserStoreError> {
        if let Some(hit) = self.unigram_total {
            return Ok(hit);
        }
        let value = get_u64_or(db, UNIGRAM_TOTAL, &codec::encode_u8(UNIGRAM_TOTAL_KEY), 0)?;
        self.unigram_total = Some(value);
        Ok(value)
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

    fn unigram_delta(&mut self, db: &impl ReadStore, token: Token) -> Result<u64, UserStoreError> {
        self.unigram(db, token)
    }

    fn count_delta(
        &mut self,
        db: &impl ReadStore,
        prev: Option<(
            Token,
            impl FnOnce() -> Result<Option<Vec<u8>>, UserStoreError>,
        )>,
        token: Token,
    ) -> Result<UserCountDelta, UserStoreError> {
        let unigram_delta = self.unigram(db, token)?;
        let unigram_total_delta = self.unigram_total(db)?;
        // The pin merges the user gram whenever one is stored, item-less
        // ones included (`merge_single_gram`, `ngram.cpp:298-306`).
        let (bigram_count, bigram_total) = match prev {
            Some((prev, load)) => self.bigram_row(prev, load)?.map_or((0, 0), |gram| {
                (
                    single_gram::get_freq(gram, token).map_or(0, u64::from),
                    single_gram::get_total_freq(gram).map_or(0, u64::from),
                )
            }),
            None => (0, 0),
        };
        Ok(UserCountDelta {
            bigram_count,
            bigram_total,
            unigram_delta,
            unigram_total_delta,
        })
    }
}

impl<S: WriteStore> GenericUserStore<S> {
    pub(crate) fn stored_system_items(
        &self,
    ) -> std::collections::BTreeMap<Token, oxpinyin_data::chunk_write::ChunkItem> {
        self.inner
            .system_items
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .snapshot()
    }

    /// Whether this session carries system-item payloads from imports or logs.
    #[must_use]
    pub fn has_system_items(&self) -> bool {
        self.inner.has_system_items.load(Ordering::Relaxed)
    }

    fn removed_system_items_guard(
        &self,
    ) -> std::sync::MutexGuard<'_, std::collections::BTreeSet<Token>> {
        self.inner
            .removed_system_items
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Whether this session holds a system token whose item a journal REMOVE
    /// took out ([`Self::system_item_removed`]).
    #[must_use]
    pub fn has_removed_system_items(&self) -> bool {
        self.inner.has_removed_system_items.load(Ordering::Relaxed)
    }

    /// Whether a journal REMOVE took the item behind system `token` out of
    /// its sub-index: `get_phrase_item` fails for it (074a2219
    /// `phrase_index.cpp:219-236`), while the shipped chunk and the phrase
    /// table still hold it.
    #[must_use]
    pub fn system_item_removed(&self, token: Token) -> bool {
        self.has_removed_system_items() && self.removed_system_items_guard().contains(&token)
    }

    pub(crate) fn stored_removed_system_items(&self) -> std::collections::BTreeSet<Token> {
        self.removed_system_items_guard().clone()
    }

    /// The token's item is live again: an add put it back (`add_phrase_item`
    /// stores at the token whatever the index held).
    fn revive_system_item(&self, token: Token) {
        if !self.has_removed_system_items() {
            return;
        }
        let mut removed = self.removed_system_items_guard();
        removed.remove(&token);
        self.inner
            .has_removed_system_items
            .store(!removed.is_empty(), Ordering::Relaxed);
    }

    /// The imported/replayed system item, with its current unigram field.
    ///
    /// # Errors
    /// Returns an error if the scratch payload or count cannot be read.
    pub fn system_item_override(
        &self,
        token: Token,
    ) -> Result<Option<oxpinyin_data::chunk_write::ChunkItem>, UserStoreError> {
        if !self.has_system_items() || !(1..=4).contains(&(token >> 24)) {
            return Ok(None);
        }
        let Some(mut item) = self.with_effective_system_item(token, Clone::clone) else {
            return Ok(None);
        };
        let base = self
            .inner
            .libpinyin
            .as_ref()
            .and_then(|target| {
                target
                    .originals()
                    .get(&phrase_index_library_index(token))
                    .and_then(|library| library.unigram(token & PHRASE_MASK))
            })
            .unwrap_or(0);
        item.unigram = base.wrapping_add(self.unigram_delta(token)? as u32);
        Ok(Some(item))
    }

    /// The stored pronunciations of the item this store holds behind
    /// `token`, as `(keys, frequency)` rows in stored order (074a2219
    /// `phrase_index.h` `get_nth_pronunciation`): an imported or trained
    /// system item (libraries 1..=4, `None` while the shipped item is
    /// untouched), or a user-file library's phrase (libraries 5..=7).
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when a user phrase cannot be read.
    pub fn item_pronunciations(
        &self,
        token: Token,
    ) -> Result<Option<oxpinyin_core::PronunciationRows>, UserStoreError> {
        use crate::store_libpinyin::pinyin_keys_to_packed;
        match token >> 24 {
            1..=4 => Ok(self.with_effective_system_item(token, |item| {
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
            })),
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
    pub fn system_item_end(&self, library: u8) -> Option<Token> {
        if !(1..=4).contains(&library) || !self.has_system_items() {
            return None;
        }
        self.inner
            .system_items
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .end(library)
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
    pub fn with_effective_system_item<R>(
        &self,
        token: Token,
        read: impl FnOnce(&oxpinyin_data::chunk_write::ChunkItem) -> R,
    ) -> Option<R> {
        if !self.has_system_items() || !(1..=4).contains(&(token >> 24)) {
            return None;
        }
        let mut items = self
            .inner
            .system_items
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        items.get(&token).map(read)
    }

    /// One past the highest token a `USER_FILE` library holds: the pin's
    /// `SubPhraseIndex::get_range` end (`phrase_index.cpp:624-646` at
    /// 074a2219, the offset array less its trailing zeros). It is the
    /// library's allocation cursor, which every add moves past the token it
    /// took and every mask or removal re-derives from the surviving items;
    /// an empty library ends at its first token, the pin's `1..1`.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the cursor cannot be read, or
    /// `InvalidPhrase` for a library that is not a `USER_FILE` one.
    pub fn user_library_token_end(&self, library: u8) -> Result<Token, UserStoreError> {
        if !is_user_file_library(library) {
            return Err(UserStoreError::InvalidPhrase);
        }
        let db = self.database();
        let keys = if library == USER_DICTIONARY {
            // `add_phrase_in`'s order: the library's own cursor, then the
            // legacy one a standalone store seeds for the user dictionary.
            vec![codec::encode_u8(library), codec::encode_u8(ALLOC_CURSOR)]
        } else {
            vec![codec::encode_u8(library)]
        };
        for key in keys {
            if let Some(bytes) = db.get(ALLOC, &key)? {
                return codec::decode_token(&bytes).map_err(|_| UserStoreError::Decode);
            }
        }
        Ok(first_library_token(library))
    }

    /// System overrides in token order for a bulk export; original items stay lazy.
    ///
    /// # Errors
    /// Returns an error when a scratch payload cannot be decoded.
    pub fn system_item_overrides(
        &self,
        library: u8,
    ) -> Result<
        std::collections::BTreeMap<Token, oxpinyin_data::chunk_write::ChunkItem>,
        UserStoreError,
    > {
        let mut items = std::collections::BTreeMap::new();
        if !self.has_system_items() {
            return Ok(items);
        }
        let tokens: Vec<_> = self
            .inner
            .system_items
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .keys()
            .copied()
            .filter(|&token| phrase_index_library_index(token) == library)
            .collect();
        for token in tokens {
            if let Some(item) = self.system_item_override(token)? {
                items.insert(token, item);
            }
        }
        Ok(items)
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
        text: &str,
        keys: &[PinyinKey],
        count: Option<u64>,
    ) -> Result<Token, UserStoreError> {
        self.flush_system_unigrams()?;
        use oxpinyin_data::chunk_write::ChunkItem;
        if !(1..=4).contains(&library) || !phrase::phrase_and_keys_valid(text, keys) {
            return Err(UserStoreError::InvalidPhrase);
        }
        let mut import_readings = self.import_readings.take();
        let target = self
            .inner
            .libpinyin
            .as_ref()
            .ok_or(UserStoreError::InvalidPhrase)?;
        let originals = target.originals();
        let original = originals
            .get(&library)
            .ok_or(UserStoreError::InvalidPhrase)?;
        let packed = crate::store_libpinyin::pinyin_keys_to_packed(keys)
            .ok_or(UserStoreError::InvalidPhrase)?;
        let delta = u32::try_from(count.unwrap_or(DEFAULT_PHRASE_COUNT)).unwrap_or(u32::MAX);
        let db = self.database();
        let generation = self.write_generation();
        if import_readings.as_ref().is_some_and(|cached| {
            cached.generation != generation || Some(cached.token) != original_token
        }) {
            import_readings = None;
        }
        let mut system_items = self
            .inner
            .system_items
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let reading = SystemReading {
            keys,
            packed: &packed,
            delta,
        };
        // The facade already selected this token from both raw phrase
        // tables; a live reverse-index row is not phrase-table membership.
        let existing = original_token;
        // A delta row for an original token exists only if training or an
        // earlier import moved one of its readings: a replayed item carries
        // its counts in its live item and writes no row at open. An untouched
        // original therefore has no row to reconcile, and the lookup below
        // can be skipped.
        let (token, pending_item, was_live) = if let Some(token) = existing {
            let was_live = system_items.contains_key(&token);
            let pending = if was_live {
                None
            } else {
                Some(
                    original
                        .item(token & PHRASE_MASK)
                        .ok_or(UserStoreError::InvalidPhrase)?,
                )
            };
            (token, pending, was_live)
        } else {
            let token = append_system_token(&*db, library, original.range_end, text, &reading)?;
            #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
            self.note_index_add(keys, token, false);
            (
                token,
                Some(ChunkItem {
                    phrase: text.chars().map(u32::from).collect(),
                    unigram: 0,
                    prons: Vec::new(),
                }),
                false,
            )
        };
        let (change, current) = {
            let item = system_items
                .get(&token)
                .or(pending_item.as_ref())
                .ok_or(UserStoreError::InvalidPhrase)?;
            let change = plan_system_reading(item, &reading);
            validate_system_merge(item, &reading, change)?;
            let current = match change {
                SystemMerge::Matched { frequency, .. } => frequency,
                SystemMerge::Append => reading.delta,
                SystemMerge::Wrap => 0,
            };
            (change, current)
        };
        let wrapped = matches!(change, SystemMerge::Wrap);
        let untouched = !was_live && existing.is_some();
        sync_system_rows(
            &*db,
            original,
            token,
            current,
            &reading,
            (untouched, wrapped),
            (generation, &mut import_readings),
        )?;
        // No fallible work remains: publish only after the rows committed.
        if let Some(mut item) = pending_item {
            apply_system_reading(&mut item, &reading, change);
            system_items.insert(token, item);
        } else if let Some(item) = system_items.get_mut(&token) {
            apply_system_reading(item, &reading, change);
        }
        drop(system_items);
        self.revive_system_item(token);
        self.inner.has_system_items.store(true, Ordering::Relaxed);
        note_import_commit(&mut import_readings, generation);
        self.mark_committed_import_write(db, existing.is_none());
        self.import_readings = import_readings;
        Ok(token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn system_import_changes_the_first_match_and_guards_its_prefix() {
        let mut item = oxpinyin_data::chunk_write::ChunkItem {
            phrase: vec![0x4f60],
            unigram: 0,
            prons: vec![(vec![1], u32::MAX - 10), (vec![2], 3), (vec![2], 5)],
        };
        let mut reading = SystemReading {
            keys: &[2],
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

    macro_rules! user_store_tests {
        ($mod:ident, $backend:ty, $ext:literal) => {
            mod $mod {
                use super::super::*;
                use crate::{
                    PHRASE_INDEX_LIBRARY_MASK, PHRASE_MASK, USER_DICTIONARY,
                    phrase_index_make_token,
                };
                use oxpinyin_core::SyllableKey;

                type Store = GenericUserStore<$backend>;

                fn temp_path(tag: &str) -> std::path::PathBuf {
                    let path = std::env::temp_dir().join(format!(
                        "oxpinyin-user-{}-{tag}-{}.{}",
                        $ext,
                        std::process::id(),
                        $ext,
                    ));
                    cleanup(&path);
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
                    let store = Store::create_standalone(&path).unwrap();
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
                        "empty count_delta must not open a read transaction"
                    );
                    cleanup(&path);
                }

                #[test]
                fn create_standalone_rejects_second_live_handle() {
                    let path = temp_path("second-live-handle");
                    let first = Store::create_standalone(&path).unwrap();
                    assert!(matches!(
                        Store::create_standalone(&path),
                        Err(UserStoreError::AlreadyOpen)
                    ));
                    drop(first);
                    cleanup(&path);
                }

                #[test]
                fn cached_count_delta_refreshes_after_committed_writes() {
                    let path = temp_path("cache-refresh");
                    let mut store = Store::create_standalone(&path).unwrap();

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
                        "a committed write invalidates the cached count reads"
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
                    let mut store = Store::create_standalone(&path).unwrap();
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
                        store.count_cache().is_none(),
                        "an emptied store must not keep a cached read transaction"
                    );
                    cleanup(&path);
                }

                #[test]
                fn reopen_of_populated_store_sets_has_user_data() {
                    let path = temp_path("reopen-populated");
                    {
                        let mut store = Store::create_standalone(&path).unwrap();
                        store.observe_selection(1, 100).unwrap();
                    }
                    let store = Store::create_standalone(&path).unwrap();
                    assert!(store.has_user_data());
                    assert_eq!(store.count_delta(Some(1), 100).unwrap().bigram_count, 69);
                    cleanup(&path);
                }

                #[test]
                fn save_compacts_after_a_cached_read() {
                    let path = temp_path("save-after-cache");
                    let mut store = Store::create_standalone(&path).unwrap();
                    store.observe_selection(1, 100).unwrap();
                    assert_eq!(store.count_delta(Some(1), 100).unwrap().bigram_count, 69);
                    assert!(store.save().unwrap());
                    assert_eq!(store.count_delta(Some(1), 100).unwrap().bigram_count, 69);
                    cleanup(&path);
                }

                #[test]
                fn observe_applies_pinned_seed_sequence() {
                    let path = temp_path("seq");
                    let mut store = Store::create_standalone(&path).unwrap();

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
                    let mut store = Store::create_standalone(&path).unwrap();
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
                    let mut store = Store::create_standalone(&path).unwrap();
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
                    let mut store = Store::create_standalone(&path).unwrap();
                    assert_eq!(store.observe_predicted(1, 200).unwrap(), 69);
                    assert_eq!(store.observe_predicted(1, 200).unwrap(), 69);
                    assert_eq!(store.bigram_count(1, 200).unwrap(), 138);
                    assert_eq!(store.bigram_total(1).unwrap(), 138);
                    assert_eq!(store.unigram_delta(200).unwrap(), 483 * 2);
                    cleanup(&path);
                }

                #[test]
                fn predicted_prefix_invalidates_counts_without_training_a_bigram() {
                    let path = temp_path("pred-prefix");
                    let mut store = Store::create_standalone(&path).unwrap();
                    store.set_bigram_count(1, 200, 10).unwrap();
                    assert_eq!(store.count_delta(Some(1), 200).unwrap().unigram_delta, 0);
                    let generation = store.generation();
                    for _ in 0..2 {
                        assert_eq!(store.observe_predicted_prefix(200).unwrap(), 69);
                    }
                    assert!(store.generation() > generation);
                    assert_eq!(store.count_delta(Some(1), 200).unwrap().unigram_delta, 966);
                    assert_eq!(store.unigram_total().unwrap(), 966);
                    assert_eq!(store.bigram_count(1, 200).unwrap(), 10);
                    assert_eq!(store.bigram_total(1).unwrap(), 10);
                    assert!(!store.is_modified());
                    cleanup(&path);
                }

                #[test]
                fn predicted_prefix_overflow_keeps_only_the_facade_total_increment() {
                    let path = temp_path("pred-prefix-overflow");
                    let mut store = Store::create_standalone(&path).unwrap();
                    let token = store
                        .add_phrase("你好", &[10, 20], Some(1_431_655_631))
                        .unwrap();
                    let before = store.unigram_delta(token).unwrap();
                    let total = store.unigram_total().unwrap();
                    let error = store.observe_predicted_prefix(token).unwrap_err();
                    assert!(matches!(error, UserStoreError::UnigramTotalOverflow));
                    assert_eq!(store.unigram_delta(token).unwrap(), before);
                    assert_eq!(store.unigram_total().unwrap(), total + 483);
                    assert!(store.export_bigrams().unwrap().is_empty());
                    cleanup(&path);
                }

                #[test]
                fn roundtrip_reopen_reads_identical() {
                    let path = temp_path("roundtrip");
                    {
                        let mut store = Store::create_standalone(&path).unwrap();
                        store.observe_selection(SENTENCE_START, 10).unwrap();
                        store.observe_selection(10, 20).unwrap();
                        store.observe_selection(10, 20).unwrap();
                    }

                    let store = Store::create_standalone(&path).unwrap();
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
                    let mut store = Store::create_standalone(&path).unwrap();

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
                        let mut store = Store::create_standalone(&path).unwrap();
                        store.observe_selection(SENTENCE_START, 10).unwrap();
                        store.observe_selection(10, 20).unwrap();
                        store.observe_selection(10, 20).unwrap();
                        let token = store.add_phrase("你好", &[10, 20], None).unwrap();
                        assert_eq!(token, FIRST_USER_TOKEN);
                        assert!(store.is_modified());
                        assert!(store.save().unwrap());
                        assert!(!store.is_modified());
                    }

                    let mut store = Store::create_standalone(&path).unwrap();
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
                    let mut store = Store::create_standalone(&path).unwrap();
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
                    let mut store = Store::create_standalone(&path).unwrap();
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
                    const SYSTEM: Token = 0x0100_0001;
                    let path = temp_path("nibble");
                    let mut store = Store::create_standalone(&path).unwrap();
                    let user = store.add_phrase("词", &[7], None).unwrap();
                    assert!(phrase::is_user_token(user));
                    assert!(!phrase::is_user_token(SYSTEM));
                    cleanup(&path);
                }

                #[test]
                fn network_and_user_can_share_phrase_text() {
                    let path = temp_path("two-nibbles");
                    let mut store = Store::create_standalone(&path).unwrap();
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
                    let mut store = Store::create_standalone(&path).unwrap();
                    let token = store.add_phrase("你好", &[10, 20], None).unwrap();
                    assert_eq!(store.unigram_delta(token).unwrap(), 15);
                    assert_eq!(store.bigram_count(SENTENCE_START, token).unwrap(), 0);

                    let token2 = store.add_phrase("世界", &[30, 40], Some(10)).unwrap();
                    assert_eq!(store.unigram_delta(token2).unwrap(), 30);
                    cleanup(&path);
                }

                /// The sub-index refuses a unigram add that would overflow its
                /// `guint32` total (`phrase_index.cpp:169-171`): training goes on
                /// without the unigram, and an accepted predicted candidate
                /// answers `ERROR_INTEGER_OVERFLOW` before its bigram
                /// (`pinyin.cpp:2609-2612`).
                #[test]
                fn unigram_adds_stop_at_the_library_total() {
                    let path = temp_path("uni-total");
                    let mut store = Store::create_standalone(&path).unwrap();
                    // 402 below `u32::MAX`: room for no 483 delta.
                    let token = store
                        .add_phrase("你好", &[10, 20], Some(1_431_655_631))
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
                    cleanup(&path);
                }

                #[test]
                fn phrase_generation_moves_only_on_phrase_writes() {
                    let path = temp_path("phrase-gen");
                    let mut store = Store::create_standalone(&path).unwrap();
                    assert_eq!(store.phrase_generation(), 0);
                    let token = store.add_phrase("你好", &[10, 20], None).unwrap();
                    assert_eq!(store.phrase_generation(), 1, "add_phrase is a phrase write");
                    let before = store.phrase_generation();
                    let write_before = store.generation();
                    store.observe_selection(SENTENCE_START, token).unwrap();
                    store.set_bigram_count(SENTENCE_START, token, 7).unwrap();
                    assert!(
                        store.generation() > write_before,
                        "counts moved the write generation"
                    );
                    assert_eq!(
                        store.phrase_generation(),
                        before,
                        "training and bigram writes must not move the phrase generation"
                    );
                    assert!(store.remove_user_phrase(token).unwrap());
                    assert_eq!(
                        store.phrase_generation(),
                        before + 1,
                        "removal is a phrase write"
                    );
                    cleanup(&path);
                }

                #[test]
                fn phrase_table_generation_keeps_readings_but_tracks_shared_token_edits() {
                    let path = temp_path("phrase-table-gen");
                    let mut store = Store::create_standalone(&path).unwrap();
                    assert_eq!(store.phrase_table_generation(), 0);
                    let token = store.add_phrase("你好", &[10, 20], None).unwrap();
                    assert_eq!(store.phrase_table_generation(), 1);
                    let phrase_before = store.phrase_generation();
                    assert_eq!(store.add_phrase("你好", &[11, 20], None).unwrap(), token);
                    assert_eq!(store.phrase_generation(), phrase_before + 1);
                    assert_eq!(store.phrase_table_generation(), 1);
                    store.observe_selection(SENTENCE_START, token).unwrap();
                    assert_eq!(store.phrase_table_generation(), 1);

                    let mut writer = store.clone();
                    let second = writer.add_phrase("中国", &[30, 40], None).unwrap();
                    assert_eq!(store.phrase_table_generation(), 2);
                    assert!(writer.remove_user_phrase(second).unwrap());
                    assert_eq!(store.phrase_table_generation(), 3);
                    drop(writer);
                    drop(store);
                    cleanup(&path);
                }

                #[test]
                fn existing_phrase_merges_a_new_reading() {
                    let path = temp_path("merge");
                    let mut store = Store::create_standalone(&path).unwrap();
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
                fn phrase_table_membership_keeps_each_token_text_pair() {
                    // The pin's `m_phrase_table` lists a token under every
                    // text it indexes, and `remove_index` matches the exact
                    // `(token, text)` pair (`pinyin.cpp:3750`). A later row
                    // for the same token must not evict the phrase's own
                    // membership, or the phrase would stop being removable.
                    let path = temp_path("pair-set");
                    let mut store = Store::create_standalone(&path).unwrap();
                    let token = store.add_phrase("甲", &[7], None).unwrap();
                    // A later-walked phrase-index entry: same token, new text.
                    store
                        .database()
                        .write(|txn| phrase_table::add_index(txn, "乙", token))
                        .unwrap();

                    // The own pair is still present, so removal succeeds and
                    // drops only that pair; the unrelated membership stays.
                    assert!(store.remove_user_phrase(token).unwrap());
                    assert!(store.phrase(token).unwrap().is_none());
                    assert!(
                        store
                            .database()
                            .get(phrase_table::TABLE, &phrase_table::key("乙"))
                            .unwrap()
                            .is_some(),
                        "removing the phrase drops only its own pair"
                    );
                    cleanup(&path);
                }

                #[test]
                fn reopening_a_standalone_store_preserves_missing_phrase_table() {
                    // Standalone schema changes have no migration. Reopening
                    // must not synthesize membership absent from the table.
                    let path = temp_path("backfill");
                    let token = {
                        let mut store = Store::create_standalone(&path).unwrap();
                        let token = store.add_phrase("你好", &[10, 20], None).unwrap();
                        // Parent-written shape: drop the membership row.
                        store
                            .database()
                            .write(|txn| {
                                txn.remove(phrase_table::TABLE, &phrase_table::key("你好"))
                            })
                            .unwrap();
                        assert!(
                            store
                                .database()
                                .get(phrase_table::TABLE, &phrase_table::key("你好"))
                                .unwrap()
                                .is_none()
                        );
                        token
                    };

                    let mut reopened = Store::create_standalone(&path).unwrap();
                    assert!(reopened.phrase(token).unwrap().is_some());
                    assert!(
                        !reopened.remove_user_phrase(token).unwrap(),
                        "the reopen must not backfill the phrase table"
                    );
                    assert!(reopened.phrase(token).unwrap().is_some());
                    cleanup(&path);
                }

                #[test]
                fn new_reading_appends_after_legacy_and_exhausted_sequences() {
                    for legacy in [true, false] {
                        let path = temp_path("legacy-pron-order");
                        let mut store = Store::create_standalone(&path).unwrap();
                        let token = store.add_phrase("你", &[20], Some(5)).unwrap();
                        store
                            .database()
                            .write(|txn| {
                                let value = if legacy {
                                    codec::encode_u64(5).to_vec()
                                } else {
                                    super::super::PronValue {
                                        count: 5,
                                        seq: u32::MAX,
                                        indexed: false,
                                    }
                                    .encode()
                                    .to_vec()
                                };
                                txn.put(
                                    PRONUNCIATION,
                                    &codec::encode_token_bytes(token, &phrase::encode_keys(&[20])),
                                    &value,
                                )
                            })
                            .unwrap();
                        store.add_phrase("你", &[10], Some(7)).unwrap();
                        store.add_phrase("你", &[15], Some(9)).unwrap();
                        let item = store.phrase(token).unwrap().unwrap();
                        assert_eq!(
                            item.pronunciations()
                                .iter()
                                .map(|p| p.keys().to_vec())
                                .collect::<Vec<_>>(),
                            vec![vec![20], vec![10], vec![15]]
                        );

                        let encoded = store
                            .database()
                            .get(
                                PRONUNCIATION,
                                &codec::encode_token_bytes(token, &phrase::encode_keys(&[20])),
                            )
                            .unwrap()
                            .unwrap();
                        assert_eq!(PronValue::decode(&encoded).unwrap().indexed, legacy);
                        drop(store);
                        let reopened = Store::create_standalone(&path).unwrap();
                        assert_eq!(reopened.phrase(token).unwrap().unwrap(), item);
                        drop(reopened);
                        cleanup(&path);
                    }
                }

                #[test]
                fn same_reading_accumulates_pronunciation_count() {
                    let path = temp_path("same-read");
                    let mut store = Store::create_standalone(&path).unwrap();
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
                        let mut store = Store::create_standalone(&path).unwrap();
                        let t1 = store.add_phrase("你好", &[10, 20], None).unwrap();
                        let t2 = store.add_phrase("世界", &[30, 40], Some(9)).unwrap();
                        store.add_phrase("你好", &[11, 20], Some(2)).unwrap();
                        (t1, t2, store.next_user_token().unwrap())
                    };

                    let store = Store::create_standalone(&path).unwrap();
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
                    let mut store = Store::create_standalone(&path).unwrap();
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
                    let store = Store::create_standalone(&path).unwrap();
                    assert!(store.phrase(FIRST_USER_TOKEN).unwrap().is_none());
                    assert!(store.phrase(0x0100_0001).unwrap().is_none());
                    cleanup(&path);
                }

                #[test]
                fn export_phrases_render_the_pinned_triples() {
                    let path = temp_path("export-phrases");
                    let mut store = Store::create_standalone(&path).unwrap();
                    let ni =
                        u16::try_from(SyllableKey::from_text("ni").expect("frozen key").index())
                            .expect("frozen syllable inventory fits u16");
                    let hao =
                        u16::try_from(SyllableKey::from_text("hao").expect("frozen key").index())
                            .expect("frozen syllable inventory fits u16");
                    let shi =
                        u16::try_from(SyllableKey::from_text("shi").expect("frozen key").index())
                            .expect("frozen syllable inventory fits u16");
                    let jie =
                        u16::try_from(SyllableKey::from_text("jie").expect("frozen key").index())
                            .expect("frozen syllable inventory fits u16");

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
                    let mut store = Store::create_standalone(&path).unwrap();
                    store.observe_selection(SENTENCE_START, 10).unwrap();
                    store.observe_selection(10, 20).unwrap();
                    store.observe_selection(10, 20).unwrap();

                    let mut rows = store.export_bigrams().unwrap();
                    rows.sort();
                    assert_eq!(rows, vec![(SENTENCE_START, 10, 69), (10, 20, 207)]);
                    cleanup(&path);
                }

                #[test]
                fn bigram_successors_complete_across_256_boundary() {
                    let path = temp_path("succ-256");
                    let mut store = Store::create_standalone(&path).unwrap();

                    // Successors of `prev` spanning below and above 256 (and
                    // above it in a higher byte), so integer order and byte
                    // order genuinely differ. `prev` itself is 256.
                    let prev: Token = 0x0000_0100;
                    let succ_counts: [(Token, u64); 6] = [
                        (0x0000_0001, 11),
                        (0x0000_0002, 12),
                        (0x0000_00FF, 13),
                        (0x0000_0100, 14),
                        (0x0000_0101, 15),
                        (0x0001_0000, 16),
                    ];
                    for &(cur, count) in &succ_counts {
                        store.set_bigram_count(prev, cur, count).unwrap();
                    }

                    // Neighbouring prevs that bracket `prev` in byte order
                    // must NOT leak into the scan: prev-1, prev+1, a prev whose
                    // low byte crosses 256, and a prev in a higher byte.
                    store.set_bigram_count(prev - 1, 0x0000_0100, 99).unwrap();
                    store.set_bigram_count(prev + 1, 0x0000_00FF, 99).unwrap();
                    store
                        .set_bigram_count(0x0000_00FF, 0x0000_0100, 99)
                        .unwrap();
                    store
                        .set_bigram_count(0x0001_0000, 0x0000_0001, 99)
                        .unwrap();

                    // Complete and correctly ordered: exactly prev's
                    // successors, ascending by integer cur (the big-endian key
                    // property), with no neighbour rows.
                    let got = store.bigram_successors(prev).unwrap();
                    let mut expected = succ_counts.to_vec();
                    expected.sort_by_key(|&(cur, _)| cur);
                    assert_eq!(
                        got, expected,
                        "successor scan must be complete and integer-ordered"
                    );

                    // Non-vacuity: the successor set crosses 256, so a
                    // little-endian pair encoding would order these
                    // differently — this fixture would catch that drift.
                    let mut le_order: Vec<Token> =
                        succ_counts.iter().map(|&(cur, _)| cur).collect();
                    le_order.sort_by_key(|cur| cur.to_le_bytes());
                    let got_tokens: Vec<Token> = got.iter().map(|&(cur, _)| cur).collect();
                    assert_ne!(
                        le_order, got_tokens,
                        "fixture must cross 256 so LE and BE successor orders differ"
                    );

                    cleanup(&path);
                }

                fn mixed_store(path: &std::path::Path) -> (Store, Token) {
                    const SYSTEM_A: Token = 0x0100_0001;
                    const SYSTEM_B: Token = 0x0200_0001;
                    let mut store = Store::create_standalone(path).unwrap();
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
                    // The user sub-index is empty again, so `get_range` ends at 1
                    // (`phrase_index.cpp:640-650`) and the next import reuses it.
                    assert_eq!(store.next_user_token().unwrap(), FIRST_USER_TOKEN);
                    assert!(!store.is_modified());
                    assert!(!store.save().unwrap());

                    store.mask_out(0x0, 0x0).unwrap();
                    assert_eq!(store.bigram_count(0x0100_0001, 0x0200_0001).unwrap(), 0);
                    assert_eq!(store.bigram_total(0x0100_0001).unwrap(), 0);
                    assert_eq!(store.unigram_total().unwrap(), 0);
                    assert!(!store.is_modified());
                    cleanup(&path);
                }

                /// `_add_phrase` takes `range.m_range_end` (`pinyin.cpp:587-595`) and
                /// `get_range` drops trailing zero offsets (`phrase_index.cpp:624-650`):
                /// the next token is one past the highest live token.
                #[test]
                fn next_user_token_follows_the_highest_live_token() {
                    let path = temp_path("next-token");
                    let mut store = Store::create_standalone(&path).unwrap();
                    let a = store.add_phrase("你好", &[10, 20], None).unwrap();
                    let b = store.add_phrase("世界", &[30, 40], None).unwrap();
                    let c = store.add_phrase("再见", &[50, 60], None).unwrap();
                    assert_eq!((b, c), (a + 1, a + 2));

                    // A hole below the top is not reused.
                    store.mask_out(0xFFFF_FFFF, b).unwrap();
                    assert_eq!(store.next_user_token().unwrap(), c + 1);

                    // Masking the top frees it and every empty slot beneath it.
                    store.mask_out(0xFFFF_FFFF, c).unwrap();
                    assert_eq!(store.next_user_token().unwrap(), a + 1);
                    assert_eq!(store.add_phrase("好的", &[70, 80], None).unwrap(), a + 1);

                    // A single removal frees the top the same way.
                    assert!(store.remove_user_phrase(a + 1).unwrap());
                    assert_eq!(store.next_user_token().unwrap(), a + 1);
                    assert_eq!(store.add_phrase("再来", &[90, 100], None).unwrap(), a + 1);
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
                            let path = temp_path("remove-bigram-total");
                            let mut store = Store::create_standalone(&path).unwrap();
                            let token = store.add_phrase("你好", &[10, 20], None).unwrap();
                            let prev = 0x0100_0001;
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
                                        &[
                                            (kept[0], counts[0]),
                                            (kept[1], counts[1]),
                                            (token, removed),
                                        ],
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
                            store
                                .database()
                                .write(|txn| {
                                    for (p, value) in &planted {
                                        txn.put(BIGRAM_GRAM, &p.to_be_bytes(), value)?;
                                    }
                                    Ok(())
                                })
                                .unwrap();
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
                            drop(store);
                            let reopened = Store::create_standalone(&path).unwrap();
                            assert_eq!(reopened.bigram_total(prev).unwrap(), u64::from(expected));
                            assert_eq!(reopened.bigram_total(2).unwrap(), 17);
                            drop(reopened);
                            cleanup(&path);
                        }
                    }
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

                    let store = Store::create_standalone(&path).unwrap();
                    assert!(store.token_for_phrase("你好").unwrap().is_none());
                    assert!(store.token_for_phrase("中国").unwrap().is_none());
                    assert_eq!(store.bigram_count(0x0100_0001, 0x0200_0001).unwrap(), 69);
                    assert_eq!(store.unigram_delta(user_a).unwrap(), 0);
                    assert_eq!(store.unigram_total().unwrap(), 966);
                    cleanup(&path);
                }

                fn key(text: &str) -> PinyinKey {
                    PinyinKey::try_from(
                        SyllableKey::from_text(text)
                            .expect("frozen syllable")
                            .index(),
                    )
                    .expect("frozen syllable inventory fits u16")
                }

                #[test]
                fn add_phrase_reports_typed_token_space_exhaustion() {
                    let path = temp_path("add-phrase-exhausted");
                    let mut store = Store::create_standalone(&path).unwrap();
                    let last = phrase_index_make_token(USER_DICTIONARY, PHRASE_MASK);
                    {
                        let db = store.database();
                        db.write(|txn| {
                            txn.put(
                                ALLOC,
                                &codec::encode_u8(USER_DICTIONARY),
                                &codec::encode_token(last),
                            )
                        })
                        .unwrap();
                    }
                    let err = store.add_phrase("你好", &[1, 2], None).unwrap_err();
                    assert!(matches!(err, UserStoreError::TokenSpaceExhausted));
                    cleanup(&path);
                }

                #[test]
                fn promote_addon_phrase_reports_typed_token_space_exhaustion() {
                    let path = temp_path("promote-addon-exhausted");
                    let mut store = Store::create_standalone(&path).unwrap();
                    let last = phrase_index_make_token(ADDON_DICTIONARY, PHRASE_MASK);
                    {
                        let db = store.database();
                        db.write(|txn| {
                            txn.put(
                                ALLOC,
                                &codec::encode_u8(ADDON_DICTIONARY),
                                &codec::encode_token(last),
                            )
                        })
                        .unwrap();
                    }
                    let err = store
                        .promote_addon_phrase("二簧", &[(vec![1, 2], 100)], 100)
                        .unwrap_err();
                    assert!(matches!(err, UserStoreError::TokenSpaceExhausted));
                    cleanup(&path);
                }

                /// Reads `token`'s absent unigram row into the count memo.
                /// The write that follows must retire the memo for the next
                /// read to see the row it creates.
                fn memoise_absent(store: &Store, token: Token) {
                    assert!(store.has_user_data(), "the read must go through the memo");
                    assert_eq!(store.unigram_delta(token).unwrap(), 0);
                    assert_eq!(
                        store.memoised_unigram_for_test(token),
                        Some(0),
                        "the absent row is memoised"
                    );
                }

                /// A store with user data, so its count reads use the memo.
                fn with_user_data(tag: &str) -> (std::path::PathBuf, Store) {
                    let path = temp_path(tag);
                    let mut store = Store::create_standalone(&path).unwrap();
                    store.observe_selection(1, 100).unwrap();
                    (path, store)
                }

                #[test]
                fn training_retires_a_memoised_absent_unigram() {
                    let (path, mut store) = with_user_data("absent-train");
                    memoise_absent(&store, 200);
                    store.observe_selection(1, 200).unwrap();
                    assert_eq!(store.unigram_delta(200).unwrap(), 483);
                    cleanup(&path);
                }

                #[test]
                fn a_predicted_prefix_retires_a_memoised_absent_unigram() {
                    let (path, mut store) = with_user_data("absent-predicted");
                    memoise_absent(&store, 200);
                    assert_eq!(store.observe_predicted_prefix(200).unwrap(), 69);
                    assert_eq!(store.unigram_delta(200).unwrap(), 483);
                    cleanup(&path);
                }

                #[test]
                fn an_import_retires_a_memoised_absent_unigram() {
                    let (path, mut store) = with_user_data("absent-import");
                    let token = store.next_user_token().unwrap();
                    memoise_absent(&store, token);
                    assert_eq!(
                        store.add_phrase("世界", &[30, 40], Some(10)).unwrap(),
                        token
                    );
                    assert_eq!(store.unigram_delta(token).unwrap(), 30);
                    cleanup(&path);
                }

                #[test]
                fn a_promotion_retires_a_memoised_absent_unigram() {
                    let (path, mut store) = with_user_data("absent-promote");
                    let token = phrase_index_make_token(ADDON_DICTIONARY, 1);
                    memoise_absent(&store, token);
                    let keys = [key("er"), key("huang")];
                    assert_eq!(
                        store
                            .promote_addon_phrase("二簧", &[(keys.to_vec(), 100)], 100)
                            .unwrap(),
                        token
                    );
                    assert_eq!(store.unigram_delta(token).unwrap(), 100);
                    cleanup(&path);
                }

                #[test]
                fn a_system_unigram_flush_retires_a_memoised_absent_unigram() {
                    let (path, store) = with_user_data("absent-flush");
                    let token = 0x0100_0001;
                    store.inner.system_items.lock().unwrap().insert(
                        token,
                        oxpinyin_data::chunk_write::ChunkItem {
                            phrase: vec![u32::from('你')],
                            unigram: 0,
                            prons: Vec::new(),
                        },
                    );
                    store.inner.has_system_items.store(true, Ordering::Release);
                    memoise_absent(&store, token);
                    assert!(store.add_system_unigram_frequency(token, 7).unwrap());
                    store.flush_system_unigrams().unwrap();
                    assert_eq!(store.unigram_delta(token).unwrap(), 7);
                    cleanup(&path);
                }

                #[test]
                fn removals_retire_a_memoised_present_unigram() {
                    let (path, mut store) = with_user_data("present-removal");
                    let removed = store.add_phrase("你好", &[10, 20], None).unwrap();
                    let masked = store.add_phrase("世界", &[30, 40], None).unwrap();
                    assert_eq!(store.unigram_delta(removed).unwrap(), 15);
                    assert_eq!(store.unigram_delta(masked).unwrap(), 15);
                    assert!(store.remove_user_phrase(removed).unwrap());
                    assert_eq!(store.unigram_delta(removed).unwrap(), 0);
                    assert_eq!(store.unigram_delta(masked).unwrap(), 15);
                    store.mask_out(0xFFFF_FFFF, masked).unwrap();
                    assert_eq!(store.unigram_delta(masked).unwrap(), 0);
                    cleanup(&path);
                }

                #[test]
                fn promote_addon_phrase_allocates_nibble_5_and_copies_frequency() {
                    let path = temp_path("promote-addon");
                    let mut store = Store::create_standalone(&path).unwrap();
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
                    let mut store = Store::create_standalone(&path).unwrap();
                    let err = store
                        .promote_addon_phrase("二簧", &[(vec![key("er")], 100)], 100)
                        .unwrap_err();
                    assert!(matches!(err, UserStoreError::InvalidPhrase));
                    assert!(!store.has_user_data());
                    cleanup(&path);
                }

                /// A present item that is malformed reads back as the decode
                /// failure that names the malformed part, never as a backend
                /// failure: the export cursor refuses the item at the pin's
                /// assert site for the one and propagates the other.
                #[test]
                fn phrase_tells_an_undecodable_text_and_pronunciation_apart() {
                    let path = temp_path("decode-apart");
                    let mut store = Store::create_standalone(&path).unwrap();
                    let token = store.add_phrase("甲", &[key("jia")], Some(3)).unwrap();
                    let text_key = codec::encode_token(token);
                    // Text bytes that are not UTF-8.
                    store
                        .database()
                        .write(|txn| txn.put(PHRASE, &text_key, &[0xff, 0xfe]))
                        .unwrap();
                    assert!(matches!(store.phrase(token), Err(UserStoreError::Decode)));
                    assert!(matches!(
                        store.phrase_text(token),
                        Err(UserStoreError::Decode)
                    ));
                    // The text restored; a pronunciation value of a length the
                    // codec does not know.
                    store
                        .database()
                        .write(|txn| txn.put(PHRASE, &text_key, codec::encode_str("甲")))
                        .unwrap();
                    let pron_key =
                        codec::encode_token_bytes(token, &phrase::encode_keys(&[key("jia")]));
                    store
                        .database()
                        .write(|txn| txn.put(PRONUNCIATION, &pron_key, &[1, 2, 3]))
                        .unwrap();
                    assert!(matches!(
                        store.phrase(token),
                        Err(UserStoreError::DecodePronunciation)
                    ));
                    assert_eq!(store.phrase_text(token).unwrap().as_deref(), Some("甲"));
                    cleanup(&path);
                }
            }
        };
    }

    // Exactly-one-backend: the store's compile-time guards refuse
    // combined builds, so at most one of these peer test suites
    // is compiled per build; each peer's own suite exercises the
    // generic user store's contract over that peer.
    #[cfg(feature = "tkrzw")]
    user_store_tests!(tkrzw, oxpinyin_store::TkrzwStore, "tkrzw");
    #[cfg(feature = "kyotocabinet")]
    user_store_tests!(kc, oxpinyin_store::KcStore, "kc");
    #[cfg(feature = "bdb")]
    user_store_tests!(bdb, oxpinyin_store::BdbStore, "db");

    // ── Cross-backend equivalence (one peer per build, all peers in CI) ──

    /// Under exactly-one-backend, cross-peer comparisons cannot happen
    /// in-process. Instead each build proves the *current* peer, driven
    /// through the generic user store, produces bigram walks and
    /// successor scans in ascending (prev, cur) integer order — the
    /// big-endian key property. Running every peer build (KC
    /// / Tkrzw / BDB) through CI gives the same equivalence
    /// coverage the earlier in-process check gave.
    #[test]
    fn bigram_walks_and_successors_follow_be_integer_order() {
        const PREVS: &[Token] = &[
            0x0000_00FF,
            0x0000_0100,
            0x0000_0101,
            0x0001_0000,
            SENTENCE_START,
        ];
        let rows: &[(Token, Token, u64)] = &[
            (0x0000_00FF, 0x0000_0100, 1),
            (0x0000_0100, 0x0000_00FF, 2),
            (0x0000_0100, 0x0001_0000, 3),
            (0x0000_0100, 0x0000_0001, 4),
            (0x0000_0101, 0x0000_0100, 5),
            (0x0001_0000, 0x0000_00FF, 6),
            (SENTENCE_START, 0x0000_0100, 7),
        ];

        let path =
            std::env::temp_dir().join(format!("oxpinyin-user-xback-{}.db", std::process::id(),));
        let _ = std::fs::remove_file(&path);
        let mut store = UserStore::create_standalone(&path).unwrap();
        for &(prev, cur, count) in rows {
            store.set_bigram_count(prev, cur, count).unwrap();
        }

        let walk = store.export_bigrams().unwrap();
        let pairs: Vec<(Token, Token)> = walk.iter().map(|&(p, c, _)| (p, c)).collect();
        assert!(
            pairs.is_sorted(),
            "big-endian bigram keys must walk in integer order",
        );

        // The set the peer holds must equal the set written — no
        // duplicates dropped, no rows manufactured.
        let mut expected: Vec<(Token, Token, u64)> = rows.to_vec();
        expected.sort_by_key(|&(p, c, _)| (p, c));
        assert_eq!(walk, expected, "the peer's walk must be the sorted rows");

        // Each `prev`'s successors are the peer's own iteration order
        // for that group, and must be a strictly-ascending prefix of
        // the full walk (by `cur`).
        for &prev in PREVS {
            let succ = store.bigram_successors(prev).unwrap();
            let curs: Vec<Token> = succ.iter().map(|&(c, _)| c).collect();
            assert!(
                curs.is_sorted(),
                "successors of {prev:#x} must be ascending by cur",
            );
        }

        drop(store);
        let mut lock = path.as_os_str().to_os_string();
        lock.push("-lock");
        let _ = std::fs::remove_file(std::path::Path::new(&lock));
        let _ = std::fs::remove_file(&path);
    }

    // ── Registry-specific tests (DefaultStore, whichever backend) ───────

    fn temp_path(tag: &str) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("oxpinyin-user-{tag}-{}.store", std::process::id()));
        let _ = std::fs::remove_file(&path);
        path
    }

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
                    // A stored sub-index total of MAX-4, independent of its
                    // one item's frequency 100. Compact must not erase this
                    // offset on either rejected save path.
                    let offset = u32::MAX - 104;
                    store
                        .database()
                        .write(|txn| {
                            txn.put(
                                LIBRARY_OFFSET,
                                &codec::encode_u8(1),
                                &codec::encode_u64(u64::from(offset)),
                            )
                        })
                        .expect("offset");
                    store
                        .inner
                        .has_library_offsets
                        .store(true, std::sync::atomic::Ordering::Release);
                    store
                        .inner
                        .facade_offset
                        .store(offset, std::sync::atomic::Ordering::Release);
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
                        store.unigram_delta(token).unwrap(),
                        store.unigram_total().unwrap(),
                        store.library_offset(1).unwrap(),
                        store.is_modified(),
                    );
                    let report = match entry {
                        0 => store.save_reporting(),
                        1 => store.save_reporting_compacted(&[]),
                        _ => store
                            .save_reporting_with_baseline(&[], &mut |_| panic!("guarded baseline")),
                    }
                    .expect("guarded save");
                    assert_eq!(report, crate::SaveReport::default());
                    assert_eq!(
                        (
                            store.unigram_delta(token).unwrap(),
                            store.unigram_total().unwrap(),
                            store.library_offset(1).unwrap(),
                            store.is_modified()
                        ),
                        before
                    );
                    assert!(
                        !store
                            .add_system_unigram_frequency(token, 4)
                            .expect("refusal")
                    );
                    assert_eq!(store.unigram_delta(token).unwrap(), 2);
                    assert_eq!((store.unigram_total().unwrap() as u32).wrapping_add(100), 1);
                    drop(store);
                }
            }
        }
    }

    #[test]
    fn concurrent_readers_keep_live_unigrams_across_flushes() {
        let path = temp_path("live-unigram-flush");
        let store = UserStore::open(&path).unwrap();
        let token = 0x0100_0001;
        store.inner.system_items.lock().unwrap().insert(
            token,
            oxpinyin_data::chunk_write::ChunkItem {
                phrase: vec![u32::from('你')],
                unigram: 0,
                prons: Vec::new(),
            },
        );
        store.inner.has_system_items.store(true, Ordering::Release);
        let writer = store.clone();
        let done = AtomicBool::new(false);
        let start = std::sync::Barrier::new(2);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                start.wait();
                for _ in 0..2000 {
                    assert!(writer.add_system_unigram_frequency(token, 1).unwrap());
                    writer.flush_system_unigrams().unwrap();
                }
                done.store(true, Ordering::Release);
            });
            start.wait();
            let mut last = 0;
            while !done.load(Ordering::Acquire) {
                let counts = store.count_delta(None, token).unwrap();
                assert_eq!(counts.unigram_delta, counts.unigram_total_delta);
                assert!(counts.unigram_delta >= last);
                last = counts.unigram_delta;
            }
        });
        assert_eq!(store.unigram_delta(token).unwrap(), 2000);
        assert_eq!(store.unigram_total().unwrap(), 2000);
        drop(writer);
        drop(store);
        let _ = std::fs::remove_file(path);
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
            reader.count_cache().is_none(),
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

    #[cfg(unix)]
    #[test]
    fn standalone_rejects_second_open_through_a_symlink_alias() {
        let real = temp_path("alias-real");
        let link = temp_path("alias-link");
        let first = UserStore::create_standalone(&real).unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();

        assert!(
            matches!(
                UserStore::create_standalone(&link),
                Err(UserStoreError::AlreadyOpen)
            ),
            "the symlink resolves to the same leased file"
        );

        drop(first);
        let reopened = UserStore::create_standalone(&link).unwrap();
        assert!(matches!(
            UserStore::create_standalone(&real),
            Err(UserStoreError::AlreadyOpen)
        ));
        drop(reopened);

        for stale in [&real, &link] {
            let mut lock = stale.as_os_str().to_os_string();
            lock.push("-lock");
            let _ = std::fs::remove_file(std::path::Path::new(&lock));
            let _ = std::fs::remove_file(stale);
        }
    }

    /// An in-memory [`ReadStore`] over 8-byte-encoded counts, so memo
    /// policy is testable without a backend file.
    struct MemoDb(BTreeMap<Vec<u8>, u64>);

    impl ReadStore for MemoDb {
        fn open_read_only(_path: &Path) -> Result<Self, StoreError> {
            Err(StoreError::Backend("stub opens nothing".into()))
        }

        fn get(&self, table: &str, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError> {
            if table != UNIGRAM {
                return Ok(None);
            }
            Ok(self.0.get(key).map(|v| codec::encode_u64(*v).to_vec()))
        }

        fn scan(
            &self,
            _table: &str,
            _lo: std::ops::Bound<&[u8]>,
            _hi: std::ops::Bound<&[u8]>,
            _visit: impl FnMut(&[u8], &[u8]) -> Result<oxpinyin_store::Flow, StoreError>,
        ) -> Result<(), StoreError> {
            Err(StoreError::Backend("stub scans nothing".into()))
        }

        fn is_empty(&self, table: &str) -> Result<bool, StoreError> {
            if table != UNIGRAM {
                return Ok(true);
            }
            Ok(self.0.is_empty())
        }
    }

    #[test]
    fn absent_rows_memoise_as_zero() {
        let db = MemoDb(BTreeMap::from([(codec::encode_token(1).to_vec(), 5_u64)]));
        let mut cache = CountCache::new(0);
        assert_eq!(cache.unigram(&db, 2).unwrap(), 0);
        assert_eq!(
            cache.unigram.get(&2),
            Some(&0),
            "a miss is served from the memo until a write retires it"
        );
        assert_eq!(cache.unigram(&db, 2).unwrap(), 0);
        assert_eq!(cache.unigram(&db, 1).unwrap(), 5);
        assert_eq!(cache.unigram.get(&1), Some(&5), "present rows stay cached");
    }

    #[test]
    fn explicitly_stored_zero_rows_are_memoised() {
        let db = MemoDb(BTreeMap::from([(codec::encode_token(3).to_vec(), 0_u64)]));
        let mut cache = CountCache::new(0);
        assert_eq!(cache.unigram(&db, 3).unwrap(), 0);
        assert_eq!(
            cache.unigram.get(&3),
            Some(&0),
            "a stored zero is a present row and stays cached"
        );
    }

    #[test]
    fn memo_maps_reset_at_the_capacity_bound() {
        let db = MemoDb(
            (1..=u64::try_from(COUNT_MEMO_MAX_ENTRIES + 10).unwrap())
                .map(|t| (codec::encode_token(u32::try_from(t).unwrap()).to_vec(), t))
                .collect(),
        );
        let mut cache = CountCache::new(0);
        for token in 1..=COUNT_MEMO_MAX_ENTRIES + 10 {
            let expected = u64::try_from(token).unwrap();
            assert_eq!(
                cache.unigram(&db, u32::try_from(token).unwrap()).unwrap(),
                expected
            );
        }
        assert!(
            cache.unigram.len() <= COUNT_MEMO_MAX_ENTRIES,
            "memo must not exceed the cap"
        );
        // Post-reset the map still answers correctly from the database.
        assert_eq!(cache.unigram(&db, 1).unwrap(), 1);
    }
}
