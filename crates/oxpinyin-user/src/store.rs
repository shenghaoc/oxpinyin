//! Ordered-store-backed integer store for user-learning counts and the user
//! phrase index.
//!
//! Models the *values* libpinyin records — user bigram counts, phrase-index
//! unigram deltas, and user-phrase text/pronunciations — not its `MemoryChunk`
//! / DBM byte layout (`docs/findings/user-store.md` §4, §10). All counts are
//! `u64` integers.
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

use oxpinyin_core::{ChewingKey, Completeness, SyllableKey, UserCountDelta};
use oxpinyin_data::single_gram::encode_single_gram;
use oxpinyin_store::{DefaultStore, ReadStore, StoreError, UserBigramDb, WriteStore, WriteTxn};

use crate::codec;
use crate::persistence::ChunkHeaderField;
use crate::phrase::{
    self, ADD_PHRASE_UNIGRAM_FACTOR, ADDON_DICTIONARY, DEFAULT_PHRASE_COUNT, FIRST_USER_TOKEN,
    PHRASE_INDEX_LIBRARY_MASK, PHRASE_MASK, PinyinKey, USER_DICTIONARY, UserPhrase,
    UserPronunciation, first_library_token, is_user_file_library, phrase_index_library_index,
};
use crate::registry::{self, CountCache, RegistryLease, StandaloneLease, StoreInner};
use crate::seed;
use crate::store_libpinyin::FiniGuard;

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

pub const BIGRAM: &str = "user_bigram";
pub const BIGRAM_TOTAL: &str = "user_bigram_total";
pub(crate) const SYSTEM_BASE: &str = "system_base";
pub const UNIGRAM: &str = "user_unigram";
pub const UNIGRAM_TOTAL: &str = "user_unigram_total";
pub const PHRASE: &str = "user_phrase";
pub const PHRASE_BY_TEXT: &str = "user_phrase_by_text";
pub const PHRASE_BY_LIB_TEXT: &str = "user_phrase_by_lib_text";
/// The pin's `m_phrase_table` membership loaded from
/// `user_phrase_index.bin` — `phrase_table->remove_index`'s table
/// (`pinyin.cpp:3750`), as exact `(token, phrase text)` pairs. The
/// subject's text lookups use [`PHRASE_BY_LIB_TEXT`], derived from the
/// `USER_FILE` items; this row set exists only so `remove_user_phrase`
/// can reproduce the pin's phrase-table miss. A token may sit under more
/// than one text, so the row key is the whole pair (see
/// [`phrase_table_key`]), not the token alone.
pub const PHRASE_TABLE: &str = "user_phrase_table";
pub const PRONUNCIATION: &str = "user_pronunciation";
pub const ALLOC: &str = "user_phrase_alloc";

/// Sole key in the `user_unigram_total` table.
pub const UNIGRAM_TOTAL_KEY: u8 = 0;

/// Sole key in the `user_phrase_alloc` table.
pub const ALLOC_CURSOR: u8 = 0;

/// The row key of a [`PHRASE_TABLE`] membership: the exact `(token, phrase
/// text)` pair as 4 big-endian token bytes followed by the UTF-8 text. Keying
/// the pair — rather than the token with the text as the value — keeps every
/// membership when a token is listed under more than one text, so
/// `remove_user_phrase` succeeds whenever the exact pair is present, as
/// `phrase_table->remove_index` (`pinyin.cpp:3750`) does.
#[must_use]
pub(crate) fn phrase_table_key(token: Token, text: &str) -> Vec<u8> {
    codec::encode_token_bytes(token, text.as_bytes())
}

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
    /// key past `MAX_PHRASE_LENGTH` syllables: the pin's user
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
    /// A `user_bigram.db` value is shorter than a `guint32`:
    /// `SingleGram::get_total_freq` reads four bytes through
    /// `MemoryChunk::get_content<guint32>`, whose assert dies
    /// (`memory_chunk.h:390`). `mask_out` (`ngram.cpp:80`) and
    /// `pinyin_guess_predicted_candidates` (`ngram.cpp:69`) both read it.
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
}

impl fmt::Display for UserStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Store(e) => write!(f, "store error: {e}"),
            Self::Decode => write!(f, "stored value could not be decoded"),
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
                "user pinyin index key past MAX_PHRASE_LENGTH syllables (upstream aborts, \
                 chewing_large_table2_bdb.cpp:529)"
            ),
            Self::NonTokenUserBigramKey => write!(
                f,
                "user bigram key is not a phrase_token_t (upstream aborts, ngram_bdb.cpp:199)"
            ),
            Self::ShortUserBigramValue => write!(
                f,
                "user bigram value is shorter than a guint32 total_freq (upstream aborts, \
                 memory_chunk.h:390)"
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
        }
    }
}

impl std::error::Error for UserStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Store(e) => Some(e),
            Self::Decode
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
            | Self::ResidualUserBigramGram => None,
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

fn txn_get_u64(txn: &dyn WriteTxn, table: &str, key: &[u8]) -> Result<Option<u64>, StoreError> {
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
    txn: &dyn WriteTxn,
    table: &str,
    key: &[u8],
    default: u64,
) -> Result<u64, StoreError> {
    Ok(txn_get_u64(txn, table, key)?.unwrap_or(default))
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

/// Puts `rows` into insertion order: ascending sequence, the table's key
/// order among equal sequences (the stable sort keeps it).
fn sort_by_insertion<T>(rows: &mut [T], seq: impl Fn(&T) -> u32) {
    rows.sort_by_key(|row| seq(row));
}

/// [`ADD_PHRASE_UNIGRAM_FACTOR`] as `_add_phrase`'s `guint32`
/// `unigram_factor` (`pinyin.cpp:522`).
const ADD_PHRASE_UNIGRAM_FACTOR_U32: u32 = 3;
const _: () = assert!(ADD_PHRASE_UNIGRAM_FACTOR_U32 as u64 == ADD_PHRASE_UNIGRAM_FACTOR);

/// `PhraseItem::add_pronunciation(keys, delta)` (`phrase_index.cpp:56-91`)
/// on `token`'s stored readings: an exact-key match (tone included,
/// `pinyin_exact_compare2`) gains `delta` unless the running `guint32`
/// total of the readings up to and including it would overflow — then
/// nothing changes (the pin's `return false`, which `_add_phrase`
/// discards); any other key is appended with `delta` as its count.
fn add_pronunciation(
    txn: &mut dyn WriteTxn,
    token: Token,
    key_bytes: &[u8],
    delta: u32,
    indexed: bool,
) -> Result<(), StoreError> {
    let rows = collect_pronunciations_from_txn(txn, token)?;
    let mut total_freq: u32 = 0;
    for row in &rows {
        // Stored counts are `guint32` fields upstream.
        let freq = u32::try_from(row.value.count).unwrap_or(u32::MAX);
        total_freq = total_freq.wrapping_add(freq);
        if row.key_bytes == key_bytes {
            if delta > 0 && total_freq.checked_add(delta).is_none() {
                return Ok(());
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
            return Ok(());
        }
    }
    let value = PronValue {
        count: u64::from(delta),
        seq: next_pron_seq(txn, token, &rows)?,
        indexed,
    };
    txn.put(
        PRONUNCIATION,
        &codec::encode_token_bytes(token, key_bytes),
        &value.encode(),
    )
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
    txn: &mut dyn WriteTxn,
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
/// the sub-index's `m_total_freq > m_total_freq + delta` guard. The
/// facade total bounds every library's sum from above, so the library's
/// rows are summed only when that bound itself could overflow.
fn library_total_overflows(
    txn: &dyn WriteTxn,
    library: u8,
    delta: u32,
) -> Result<bool, StoreError> {
    if delta == 0 {
        return Ok(false);
    }
    let base = txn_get_u64_or(txn, SYSTEM_BASE, &codec::encode_u8(library), 0)?;
    let bound = txn_get_u64_or(txn, UNIGRAM_TOTAL, &codec::encode_u8(UNIGRAM_TOTAL_KEY), 0)?
        .saturating_add(base);
    if bound <= u64::from(u32::MAX - delta) {
        return Ok(false);
    }
    let lo = codec::encode_token(phrase::phrase_index_make_token(library, 0));
    let hi = u32::from(library)
        .checked_add(1)
        .map(|next| codec::encode_token(next << 24));
    let mut total = base as u32;
    txn.range(
        UNIGRAM,
        Bound::Included(lo.as_slice()),
        hi.as_ref()
            .map_or(Bound::Unbounded, |hi| Bound::Excluded(hi.as_slice())),
        &mut |_key, value| {
            let unigram = codec::decode_u64(value)
                .map_err(|_| StoreError::Backend("corrupt unigram".into()))?;
            total = total.wrapping_add(u32::try_from(unigram).unwrap_or(u32::MAX));
            Ok(())
        },
    )?;
    Ok(total.checked_add(delta).is_none())
}

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

fn collect_pronunciations_from_store(
    store: &impl ReadStore,
    token: Token,
) -> Result<Vec<UserPronunciation>, UserStoreError> {
    let (lo, hi) = pronunciation_range(token);
    let mut out = Vec::new();
    store.range(
        PRONUNCIATION,
        lo.as_ref().map(<[u8; 4]>::as_slice),
        hi.as_ref().map(<[u8; 4]>::as_slice),
        &mut |key, value| {
            let (_, key_bytes) = codec::decode_token_bytes(key)
                .map_err(|_| StoreError::Backend("corrupt pronunciation key".into()))?;
            let value = PronValue::decode(value)?;
            out.push((
                value.seq,
                UserPronunciation::new(phrase::decode_keys(key_bytes), value.count, value.indexed),
            ));
            Ok(())
        },
    )?;
    sort_by_insertion(&mut out, |row| row.0);
    Ok(out
        .into_iter()
        .map(|(_, pronunciation)| pronunciation)
        .collect())
}

/// `token`'s readings inside `txn`, in insertion order.
pub(crate) fn collect_pronunciations_from_txn(
    txn: &dyn WriteTxn,
    token: Token,
) -> Result<Vec<PronRow>, StoreError> {
    let (lo, hi) = pronunciation_range(token);
    let mut out = Vec::new();
    txn.range(
        PRONUNCIATION,
        lo.as_ref().map(<[u8; 4]>::as_slice),
        hi.as_ref().map(<[u8; 4]>::as_slice),
        &mut |key, value| {
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
    txn: &mut dyn WriteTxn,
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

fn remove_pronunciations(txn: &mut dyn WriteTxn, token: Token) -> Result<(), StoreError> {
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
    store.for_each(PRONUNCIATION, &mut |key, value| {
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
    txn: &dyn WriteTxn,
    tokens: &std::collections::HashSet<Token>,
) -> Result<Vec<(Token, Vec<u8>)>, StoreError> {
    let mut out = Vec::new();
    if tokens.is_empty() {
        return Ok(out);
    }
    txn.for_each(PRONUNCIATION, &mut |key, _value| {
        let (token, key_bytes) = codec::decode_token_bytes(key)
            .map_err(|_| StoreError::Backend("corrupt pronunciation key".into()))?;
        if tokens.contains(&token) {
            out.push((token, key_bytes.to_vec()));
        }
        Ok(())
    })?;
    Ok(out)
}

/// Whether the store holds any user data, evaluated inside `txn`.
///
/// `||` short-circuits, so each table is probed with a first-row
/// `WriteTxn::is_empty` check and full-table scans are avoided.
fn has_user_data_in_write_txn(txn: &dyn WriteTxn) -> Result<bool, StoreError> {
    Ok(!txn.is_empty(BIGRAM)?
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
    /// Last field so [`RegistryLease`] drains after this handle's `Arc` dies.
    _lease: RegistryLease,
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
    /// Crate-visible handle assembly for the libpinyin constructor
    /// ([`crate::store_libpinyin`]), which runs in memory and arms
    /// the fini guard of the open that raised the counter.
    pub(crate) const fn from_parts(
        inner: Arc<StoreInner<S>>,
        lease: Option<Arc<StandaloneLease>>,
        fini: FiniGuard,
    ) -> Self {
        Self {
            _fini: fini,
            inner,
            _standalone_lease: lease,
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
    pub(crate) fn database(&self) -> MutexGuard<'_, S> {
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
    /// `MAX_PHRASE_LENGTH` syllables. `pinyin_mask_out` / `zhuyin_mask_out`
    /// walk every record, so any such key aborts the whole mask
    /// (`chewing_large_table2_bdb.cpp:529`).
    #[must_use]
    pub fn has_overlong_index_key(&self) -> bool {
        !self.overlong_index_readings().is_empty()
    }

    /// The `user_bigram.db` keys whose value is shorter than a `guint32`
    /// `total_freq` — the pin's `MemoryChunk::get_content<guint32>` assert
    /// (`memory_chunk.h:390`). Empty for a store with no bigram rows.
    fn short_bigram_values(&self) -> &[Token] {
        self.inner
            .libpinyin
            .as_ref()
            .map_or(&[][..], |target| target.bigram_short_values.as_slice())
    }

    /// Whether a `user_bigram.db` value is too short to hold a `guint32`
    /// `total_freq`. `pinyin_mask_out` / `zhuyin_mask_out` and
    /// `pinyin_guess_predicted_candidates` read the total directly
    /// (`ngram.cpp:80`, `:69`), so any such value aborts them
    /// (`memory_chunk.h:390`).
    #[must_use]
    pub fn has_short_bigram_value(&self) -> bool {
        !self.short_bigram_values().is_empty()
    }

    /// The `user_bigram.db` keys that are not four bytes — the pin's
    /// `Bigram::get_all_items` assert (`ngram_bdb.cpp:199`). Empty for a
    /// store with no bigram rows.
    fn non_token_bigram_keys(&self) -> &[Vec<u8>] {
        self.inner
            .libpinyin
            .as_ref()
            .map_or(&[][..], |target| target.bigram_non_token_keys.as_slice())
    }

    /// Whether a `user_bigram.db` key is not a four-byte `phrase_token_t`.
    /// The pin's `mask_out` and `pinyin_begin_get_bigram_phrases` both walk
    /// the whole container through `get_all_items`, so any such key aborts
    /// them (`ngram_bdb.cpp:199`).
    #[must_use]
    pub fn has_non_token_bigram_key(&self) -> bool {
        !self.non_token_bigram_keys().is_empty()
    }

    /// The `user_bigram.db` rows with no items and a residual
    /// `total_freq` — the pin's `SingleGram::get_length` assert
    /// (`ngram.cpp:70`). Empty for a store with no bigram rows.
    fn empty_bigram_grams(&self) -> &[Token] {
        self.inner
            .libpinyin
            .as_ref()
            .map_or(&[][..], |target| target.bigram_empty_with_total.as_slice())
    }

    /// Whether a `user_bigram.db` row decodes to a gram with no items and a
    /// nonzero `total_freq`. `pinyin_guess_predicted_candidates` reaches
    /// `get_length` on it (`pinyin.cpp:2332`), and `get_length` asserts
    /// (`ngram.cpp:70`).
    #[must_use]
    pub fn has_empty_bigram_gram(&self) -> bool {
        !self.empty_bigram_grams().is_empty()
    }

    /// The `user_bigram.db` rows whose `total_freq` is not covered by their
    /// items, with their item tokens — the pin's `Bigram::mask_out` reaches
    /// `SingleGram::get_length` on them (`ngram.cpp:70`). Empty for a store
    /// with no bigram rows.
    fn residual_bigram_grams(&self) -> &[(Token, Vec<Token>)] {
        self.inner
            .libpinyin
            .as_ref()
            .map_or(&[][..], |target| target.bigram_residual_grams.as_slice())
    }

    /// Whether `mask_out(mask, value)` makes the pin abort: a recorded gram
    /// whose key the mask leaves alone (`(key & mask) != value`) but every
    /// item of which the mask selects is reduced to no items, leaving the
    /// `total_freq` the mask could not subtract. `Bigram::mask_out` then
    /// asks `SingleGram::get_length`, whose assert dies (`ngram.cpp:70`,
    /// `ngram_bdb.cpp:243`). A gram already covered by its items cannot be
    /// left with such a residual, so only inconsistent totals are recorded.
    #[must_use]
    pub fn has_residual_bigram_gram(&self, mask: Token, value: Token) -> bool {
        self.residual_bigram_grams().iter().any(|(key, items)| {
            (key & mask) != value && items.iter().all(|token| (token & mask) == value)
        })
    }

    /// The fault the pin's `Bigram::mask_out(mask, value)` hits on the
    /// recorded `user_bigram.db` faults, in the pin's own order, or `None`
    /// when the walk completes. `pinyin_mask_out` / `zhuyin_mask_out` and
    /// `pinyin_remove_user_candidate` (`user_bigram->mask_out`,
    /// `pinyin.cpp:1230`, `zhuyin.cpp:765`, `pinyin.cpp:3766`) all reach the
    /// same walk.
    ///
    /// `Bigram::mask_out` walks the whole container with `get_all_items`
    /// first — a non-token key aborts before any gram (`ngram_bdb.cpp:199`)
    /// — then, per gram whose key the mask does not remove wholesale,
    /// loads it and calls `SingleGram::mask_out`, whose `get_total_freq`
    /// aborts on a short value (`memory_chunk.h:390`), and asks
    /// `get_length` for the remainder, which aborts on a residual
    /// (`ngram.cpp:70`, `ngram_bdb.cpp:243`). A gram the mask removes
    /// wholesale (`(key & mask) == value`) is never loaded, so the two
    /// value faults are checked only for keys the mask leaves alone.
    fn bigram_mask_fault(&self, mask: Token, value: Token) -> Option<UserStoreError> {
        if self.has_non_token_bigram_key() {
            return Some(UserStoreError::NonTokenUserBigramKey);
        }
        if self
            .short_bigram_values()
            .iter()
            .any(|key| (key & mask) != value)
        {
            return Some(UserStoreError::ShortUserBigramValue);
        }
        if self.has_residual_bigram_gram(mask, value) {
            return Some(UserStoreError::ResidualUserBigramGram);
        }
        None
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
        read: impl FnOnce(&mut CountCache, &S) -> Result<T, UserStoreError>,
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
    fn mark_committed_write(&self, db: MutexGuard<'_, S>, has_user_data: bool) {
        self.inner
            .has_user_data
            .store(has_user_data, Ordering::Release);
        self.inner.write_generation.fetch_add(1, Ordering::AcqRel);
        drop(db);
        *self.count_cache() = None;
    }

    /// [`Self::mark_committed_write`] for a write that changed the phrase
    /// or pronunciation tables: also moves the phrase generation, which
    /// is what [`UserLookup`] watches. Count-only writes (training,
    /// bigram seeding) take the plain form, so a keystroke that trains
    /// never pays for an index rebuild.
    fn mark_committed_phrase_write(&self, db: MutexGuard<'_, S>, has_user_data: bool) {
        self.inner.phrase_generation.fetch_add(1, Ordering::AcqRel);
        self.mark_committed_write(db, has_user_data);
    }

    fn init_and_wrap(db: S) -> Result<Arc<StoreInner<S>>, UserStoreError> {
        let has_user_data = db.write(|txn| {
            let total_key = codec::encode_u8(UNIGRAM_TOTAL_KEY);
            if txn.get(UNIGRAM_TOTAL, &total_key)?.is_none() {
                let mut sum = 0_u64;
                txn.for_each(UNIGRAM, &mut |_k, v| {
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

            // Backfill `PHRASE_TABLE` for a store written before the table
            // existed: derive each phrase's exact `(token, text)` pair from
            // its `PHRASE` row, so a pre-upgrade phrase stays removable.
            // Only standalone stores run `init_and_wrap`; a libpinyin session
            // seeds the table from `user_phrase_index.bin` instead, where a
            // missing row is intentional state `pinyin_remove_user_candidate`
            // reproduces at `pinyin.cpp:3750` (see `remove_user_phrase`), so
            // that path must not be backfilled here.
            if txn.is_empty(PHRASE_TABLE)? {
                let mut memberships: Vec<(Token, String)> = Vec::new();
                txn.for_each(PHRASE, &mut |k, v| {
                    let token = codec::decode_token(k)
                        .map_err(|_| StoreError::Backend("corrupt phrase token".into()))?;
                    let text = codec::decode_str(v)
                        .map_err(|_| StoreError::Backend("corrupt phrase text".into()))?
                        .to_owned();
                    memberships.push((token, text));
                    Ok(())
                })?;
                for (token, text) in memberships {
                    txn.put(PHRASE_TABLE, &phrase_table_key(token, &text), &[])?;
                }
            }
            has_user_data_in_write_txn(txn)
        })?;

        Ok(Arc::new(StoreInner {
            has_system_items: AtomicBool::new(false),
            system_items: std::sync::Mutex::new(std::collections::BTreeMap::new()),
            count_cache: Mutex::new(None),
            db: Mutex::new(db),
            dirty: AtomicBool::new(false),
            write_generation: AtomicU64::new(0),
            phrase_generation: AtomicU64::new(0),
            has_user_data: AtomicBool::new(has_user_data),
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
            _lease: RegistryLease,
        })
    }

    /// Stored bigram count for `(prev, cur)`; `0` if unrecorded.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the store cannot be read.
    pub fn bigram_count(&self, prev: Token, cur: Token) -> Result<u64, UserStoreError> {
        let db = self.database();
        get_u64_or(&*db, BIGRAM, &codec::encode_token_pair(prev, cur), 0)
    }

    /// Total bigram mass recorded after `prev`; `0` if none.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the store cannot be read.
    pub fn bigram_total(&self, prev: Token) -> Result<u64, UserStoreError> {
        let db = self.database();
        get_u64_or(&*db, BIGRAM_TOTAL, &codec::encode_token(prev), 0)
    }

    /// Overwrite the raw `(prev -> cur)` user-bigram count.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the counters cannot be written.
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
        self.mirror_store(prev)?;
        Ok(())
    }

    /// Accumulated phrase-index unigram delta for `token`; `0` if none.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the store cannot be read.
    pub fn unigram_delta(&self, token: Token) -> Result<u64, UserStoreError> {
        if !self.has_user_data() {
            return Ok(0);
        }
        self.with_count_cache(|cached, db| cached.unigram_delta(db, token))
    }

    /// Sum of every stored unigram delta; `0` if the store is empty.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the store cannot be read.
    pub fn unigram_total(&self) -> Result<u64, UserStoreError> {
        let db = self.database();
        get_u64_or(&*db, UNIGRAM_TOTAL, &codec::encode_u8(UNIGRAM_TOTAL_KEY), 0)
    }

    /// The pinned sub-index's wrapping u32 total of stored deltas in one library.
    ///
    /// # Errors
    /// Returns the store error if a unigram row cannot be read or decoded.
    pub fn unigram_library_total(&self, library: u8) -> Result<u32, UserStoreError> {
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
            &mut |_, value| {
                let count = codec::decode_u64(value)
                    .map_err(|_| StoreError::Backend("corrupt unigram".into()))?;
                total = total.wrapping_add(u32::try_from(count).unwrap_or(u32::MAX));
                Ok(())
            },
        )?;
        Ok(total)
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
        if !self.has_user_data() {
            return Ok(UserCountDelta::ZERO);
        }
        self.with_count_cache(|cached, db| cached.count_delta(db, prev, token))
    }

    /// Record a training selection of `cur` after `last` (the `pinyin_train`
    /// path, §2). Returns the seed applied.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the observation cannot be committed.
    pub fn observe_selection(&mut self, last: Token, cur: Token) -> Result<u64, UserStoreError> {
        let seed = self.update(last, cur, SeedPolicy::Training)?;
        self.inner.dirty.store(true, Ordering::Relaxed);
        Ok(seed)
    }

    /// Pronunciation half of train_result3 (074a2219 phonetic_lookup.h:925-927).
    pub(crate) fn train_pronunciations(
        &mut self,
        token: Token,
        readings: &mut dyn Iterator<Item = Vec<oxpinyin_core::ChewingKey>>,
        seed: u64,
    ) -> Result<(), UserStoreError> {
        use crate::store_libpinyin::{pinyin_keys_to_packed, pronunciation_matches};
        let original = self
            .inner
            .libpinyin
            .as_ref()
            .and_then(|target| target.originals.get(&phrase_index_library_index(token)))
            .and_then(|library| library.item(token & phrase::PHRASE_MASK));
        let db = self.database();
        db.write(|txn| {
            // (row key, packed reading, running count, stored row value).
            let mut rows: Vec<(Vec<u8>, Vec<u16>, u32, PronValue)> = Vec::new();
            if let Some(item) = &original {
                // A system item: the stored value is the delta over the
                // original count, and an untouched reading has no row yet.
                for (keys, frequency) in &item.prons {
                    let key = codec::encode_token_bytes(token, &phrase::encode_keys(keys));
                    let stored = match txn.get(PRONUNCIATION, &key)? {
                        Some(bytes) => PronValue::decode(&bytes)?,
                        None => PronValue {
                            count: 0,
                            seq: u32::MAX,
                            indexed: false,
                        },
                    };
                    rows.push((
                        key,
                        keys.clone(),
                        frequency.wrapping_add(stored.count as u32),
                        stored,
                    ));
                }
            } else {
                for row in collect_pronunciations_from_txn(txn, token)? {
                    let Some(packed) = pinyin_keys_to_packed(&phrase::decode_keys(&row.key_bytes))
                    else {
                        continue;
                    };
                    let key = codec::encode_token_bytes(token, &row.key_bytes);
                    rows.push((key, packed, row.value.count as u32, row.value));
                }
            }
            for reading in readings {
                let mut total = 0_u32;
                for (key, packed, frequency, stored) in &mut rows {
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
                    stored.count = stored.count.wrapping_add(seed);
                    txn.put(PRONUNCIATION, key, &stored.encode())?;
                }
            }
            Ok(())
        })?;
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
            // `seed * unigram_factor` in `guint32`; the seeds are small.
            let delta = u32::try_from(seed::unigram_delta(seed)).unwrap_or(u32::MAX);
            let library = phrase_index_library_index(cur);
            if matches!(policy, SeedPolicy::Predicted)
                && !add_unigram_frequency(txn, library, cur, delta)?
            {
                // The unigram add comes first and its overflow ends the
                // call before any bigram is trained (`pinyin.cpp:2609-2612`);
                // the facade total has already taken the delta.
                return Ok(None);
            }
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

            if matches!(policy, SeedPolicy::Training) {
                // `train_result3` ignores the sub-index's refusal
                // (`phonetic_lookup.h:928-929`): the item stops growing
                // once its library's `guint32` total would overflow.
                add_unigram_frequency(txn, library, cur, delta)?;
            }

            Ok(Some(seed))
        })?;
        self.mark_committed_write(db, true);
        let Some(seed) = seed else {
            return Err(UserStoreError::UnigramTotalOverflow);
        };
        // `m_user_bigram->store(last_token, user)` (`pinyin_lookup2.cpp:626`,
        // `pinyin.cpp:2638`): one store of the predecessor's gram.
        self.mirror_store(last)?;
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
            .is_none_or(|target| target.originals.layout().has_user_library(library))
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

        let db = self.database();
        let token = db.write(|txn| {
            let lib_key = codec::encode_u8_str(library, phrase);
            let existing = if let Some(bytes) = txn.get(PHRASE_BY_LIB_TEXT, &lib_key)? {
                Some(
                    codec::decode_token(&bytes)
                        .map_err(|_| StoreError::Backend("corrupt lib-text token".into()))?,
                )
            } else if library == USER_DICTIONARY {
                let text_key = codec::encode_str(phrase);
                match txn.get(PHRASE_BY_TEXT, text_key)? {
                    Some(bytes) => Some(
                        codec::decode_token(&bytes)
                            .map_err(|_| StoreError::Backend("corrupt text token".into()))?,
                    ),
                    None => None,
                }
            } else {
                None
            };

            if let Some(token) = existing {
                // The existing-item path (`pinyin.cpp:566-583`): the
                // reading is merged into the item — no unigram, and no
                // `add_index`, so a reading this adds stays out of the
                // user pinyin index.
                add_pronunciation(txn, token, &key_bytes, delta, false)?;
                Ok(Ok(token))
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
                    // `_add_phrase`'s new-item path calls
                    // `phrase_table->add_index` (`pinyin.cpp:596`), so the
                    // pin's phrase table gains the `(token, text)` pair and
                    // a later `remove_index` finds it.
                    txn.put(PHRASE_TABLE, &phrase_table_key(token, phrase), &[])?;
                }

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

                let unigram = delta.wrapping_mul(ADD_PHRASE_UNIGRAM_FACTOR_U32);
                add_unigram_frequency(txn, library, token, unigram)?;
                Ok(Ok(token))
            }
        })??;
        self.mark_committed_phrase_write(db, true);
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
        let token = db.write(|txn| {
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
                let Some(token) = phrase::canonicalize_library_token(ADDON_DICTIONARY, raw) else {
                    return Ok(Err(UserStoreError::TokenSpaceExhausted));
                };
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
                let rows = collect_pronunciations_from_txn(txn, token)?;
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
            Ok(Ok(token))
        })??;
        self.mark_committed_phrase_write(db, true);
        Ok(token)
    }

    /// Phrase text and pronunciations for `token`, if this store owns it.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the store cannot be read.
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
        db.for_each(PHRASE, &mut |k, v| {
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
        db.for_each(PHRASE, &mut |k, v| {
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

    /// User-bigram successors of `prev` as `(token, count)` pairs.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the store cannot be read.
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
                    .map_err(|_| StoreError::Backend("corrupt bigram key".into()))?;
                let count = codec::decode_u64(v)
                    .map_err(|_| StoreError::Backend("corrupt bigram count".into()))?;
                rows.push((cur, count));
                Ok(())
            },
        )?;
        Ok(rows)
    }

    /// Every stored user-bigram row as `(prev, cur, count)`, raw.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the store cannot be read.
    pub fn export_bigrams(&self) -> Result<Vec<(Token, Token, u64)>, UserStoreError> {
        let db = self.database();
        let mut rows = Vec::new();
        db.for_each(BIGRAM, &mut |k, v| {
            let (prev, cur) = codec::decode_token_pair(k)
                .map_err(|_| StoreError::Backend("corrupt bigram key".into()))?;
            let count = codec::decode_u64(v)
                .map_err(|_| StoreError::Backend("corrupt bigram count".into()))?;
            rows.push((prev, cur, count));
            Ok(())
        })?;
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
            // Arc clone: the originals hold every system item (~138k
            // ChunkItems) and a dirty save must not copy them — both
            // halves take the Arc by reference.
            let state = crate::store_libpinyin::export_state(self, &target.originals)?;
            let bigram_db = self
                .inner
                .bigram_db
                .as_ref()
                .map(|db| db.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
            crate::persistence::save_with_bigram(
                dir,
                &state,
                &target.originals,
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

    /// [`Self::save`] the way the pin saves, without its `m_modified` gate
    /// (the caller's: [`Self::is_modified`]): the file set is written and
    /// renamed and the marker rewritten whatever fails on the way, and the
    /// failures come back in the [`SaveReport`] instead of ending the save.
    /// A store with no libpinyin target (no directory to write) reports
    /// nothing. The store is clean afterwards, as after [`Self::save`].
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the session values cannot be
    /// exported or encoded, or the in-memory session store cannot be
    /// compacted — failures the pin has no counterpart for. A filesystem
    /// failure is never one.
    pub fn save_reporting(&mut self) -> Result<SaveReport, UserStoreError> {
        let mut report = SaveReport::default();
        if let Some(target) = self.inner.libpinyin.clone() {
            let Some(dir) = target.dir.as_deref() else {
                return Ok(report);
            };
            let state = crate::store_libpinyin::export_state(self, &target.originals)?;
            let bigram_db = self
                .inner
                .bigram_db
                .as_ref()
                .map(|db| db.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
            report = crate::persistence::save_with_bigram_reporting(
                dir,
                &state,
                &target.originals,
                &target.versions,
                target.open_counter,
                bigram_db.as_deref(),
                target.law == crate::persistence::UserConfLaw::Zhuyin,
            )?;
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
    /// one.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the store or the container cannot
    /// be read.
    pub fn bigram_predecessors(&self) -> Result<Vec<Token>, UserStoreError> {
        if let Some(mirror) = self.inner.bigram_db.as_ref() {
            let mirror = mirror
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            return Ok(mirror
                .keys()?
                .into_iter()
                .filter_map(|key| <[u8; 4]>::try_from(key.as_slice()).ok())
                .map(Token::from_le_bytes)
                .collect());
        }
        let mut prevs: Vec<Token> = self
            .export_bigrams()?
            .into_iter()
            .map(|(prev, _, _)| prev)
            .collect();
        prevs.dedup();
        Ok(prevs)
    }

    /// The `SingleGram` chunk `prev`'s stored rows make (`total_freq`, then
    /// the successors in token order); `None` when `prev` has no row.
    fn gram_value(&self, prev: Token) -> Result<Option<Vec<u8>>, UserStoreError> {
        let items = self.bigram_successors(prev)?;
        if items.is_empty() {
            return Ok(None);
        }
        let total = u32::try_from(self.bigram_total(prev)?).unwrap_or(u32::MAX);
        let records: Vec<(u32, u32)> = items
            .into_iter()
            .map(|(cur, count)| (cur, u32::try_from(count).unwrap_or(u32::MAX)))
            .collect();
        Ok(Some(encode_single_gram(total, &records)))
    }

    /// `Bigram::store(prev, gram)` on the pin's container — or its
    /// removal, when `prev` no longer has a row.
    fn mirror_store(&self, prev: Token) -> Result<(), UserStoreError> {
        let Some(mirror) = self.inner.bigram_db.as_ref() else {
            return Ok(());
        };
        let value = self.gram_value(prev)?;
        let mirror = mirror
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let key = prev.to_le_bytes();
        match value {
            Some(value) => mirror.store(&key, &value)?,
            None => mirror.remove(&key)?,
        }
        Ok(())
    }

    /// `Bigram::mask_out(mask, value)`'s walk (`ngram_*.cpp`, "sync
    /// mask_out code"): over the container's keys in its own order, a key
    /// that matches is removed; any other whose gram changed is stored
    /// again, or removed once empty. The values are the store's rows
    /// after its own mask, so the container follows the pin's sequence of
    /// operations on the store's data.
    fn mirror_mask_out(&self, mask: Token, value: Token) -> Result<(), UserStoreError> {
        let Some(mirror) = self.inner.bigram_db.as_ref() else {
            return Ok(());
        };
        let mirror = mirror
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for key in mirror.keys()? {
            let Ok(bytes) = <[u8; 4]>::try_from(key.as_slice()) else {
                continue;
            };
            let prev = Token::from_le_bytes(bytes);
            if prev & mask == value {
                mirror.remove(&key)?;
                continue;
            }
            match self.gram_value(prev)? {
                None => mirror.remove(&key)?,
                Some(gram) => {
                    if mirror.get(&key)?.as_deref() != Some(gram.as_slice()) {
                        mirror.store(&key, &gram)?;
                    }
                }
            }
        }
        Ok(())
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
        if self.has_overlong_index_key() {
            return Err(UserStoreError::OverlongIndexKey);
        }
        // The bigram walk's faults, in the pin's order: a non-token key
        // (`get_all_items`), then a short value (`get_total_freq`), then a
        // mask residual (`get_length`).
        if let Some(error) = self.bigram_mask_fault(mask, value) {
            return Err(error);
        }
        let db = self.database();
        let has_user_data = db.write(|txn| {
            // Bigram: collect all rows, then remove matching and rewrite totals.
            let mut bigram_rows: Vec<((Token, Token), u64)> = Vec::new();
            txn.for_each(BIGRAM, &mut |k, v| {
                let (prev, cur) = codec::decode_token_pair(k)
                    .map_err(|_| StoreError::Backend("corrupt bigram key".into()))?;
                let count = codec::decode_u64(v)
                    .map_err(|_| StoreError::Backend("corrupt bigram count".into()))?;
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
                    .map_err(|_| StoreError::Backend("corrupt bigram_total key".into()))?;
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

            // User phrases: text, reverse lookup, and pronunciations.
            let mut matched: Vec<(Token, String)> = Vec::new();
            txn.for_each(PHRASE, &mut |k, v| {
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
                txn.remove(PHRASE_TABLE, &phrase_table_key(token, &text))?;
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
            has_user_data_in_write_txn(txn)
        })?;
        // Commit succeeded: discard matched in-memory system payloads too.
        // Dropping an original's override restores it; dropping an appended
        // item removes its only payload. Do this before publishing the epoch.
        {
            let mut items = self
                .inner
                .system_items
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            items.retain(|&token, _| (token & mask) != value);
            self.inner
                .has_system_items
                .store(!items.is_empty(), Ordering::Relaxed);
        }
        self.mark_committed_phrase_write(db, has_user_data);
        // `m_user_bigram->mask_out(mask, value)` (`pinyin.cpp:1230`).
        self.mirror_mask_out(mask, value)?;
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
        let result: Result<Option<bool>, UserStoreError> = db
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

                // `phrase_table->remove_index` (`pinyin.cpp:3750`): the
                // phrase table loaded from `user_phrase_index.bin` must
                // hold this exact `(token, text)` pair, or the pin asserts.
                // Keying the whole pair keeps every membership when a token
                // is listed under more than one text. Nothing else reads
                // these rows (the subject's text lookups derive from
                // `user.bin`; see `docs/findings/user-store.md` §11).
                let table_key = phrase_table_key(token, &text);
                if txn.get(PHRASE_TABLE, &table_key)?.is_none() {
                    return Ok(None);
                }

                // `pinyin_table->remove_index` for every reading
                // (`pinyin.cpp:3759`): a reading merged into an existing
                // phrase was never indexed (`pinyin.cpp:569-582`), so the
                // pin asserts when it cannot drop one.
                let readings = collect_pronunciations_from_txn(txn, token)?;
                if readings.iter().any(|row| !row.value.indexed) {
                    return Ok(None);
                }

                txn.remove(PHRASE, &token_key)?;
                txn.remove(PHRASE_TABLE, &table_key)?;
                if phrase_index_library_index(token) == USER_DICTIONARY {
                    txn.remove(PHRASE_BY_TEXT, codec::encode_str(&text))?;
                }
                txn.remove(
                    PHRASE_BY_LIB_TEXT,
                    &codec::encode_u8_str(phrase_index_library_index(token), &text),
                )?;
                remove_pronunciations(txn, token)?;

                // Bigram: collect, remove matching, rewrite totals.
                let mut bigram_rows: Vec<((Token, Token), u64)> = Vec::new();
                txn.for_each(BIGRAM, &mut |k, v| {
                    let (prev, cur) = codec::decode_token_pair(k)
                        .map_err(|_| StoreError::Backend("corrupt bigram key".into()))?;
                    let count = codec::decode_u64(v)
                        .map_err(|_| StoreError::Backend("corrupt bigram count".into()))?;
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
                        .map_err(|_| StoreError::Backend("corrupt bigram_total key".into()))?;
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

                let has = has_user_data_in_write_txn(txn)?;
                Ok(Some(has))
            })
            .map_err(UserStoreError::from);
        result?.map_or_else(
            || Ok(false),
            |has_user_data| {
                self.mark_committed_phrase_write(db, has_user_data);
                // `user_bigram->mask_out(PHRASE_INDEX_LIBRARY_MASK |
                // PHRASE_MASK, token)` (`pinyin.cpp:3764-3766`).
                self.mirror_mask_out(PHRASE_INDEX_LIBRARY_MASK | PHRASE_MASK, token)?;
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
            unigram: HashMap::new(),
            unigram_total: None,
            bigram: HashMap::new(),
            bigram_total: HashMap::new(),
        }
    }

    /// `UNIGRAM[token]`, memoised. A present row is memoised whatever its
    /// value, including an explicit `0`; an absent row reads as `0`
    /// without entering the memo.
    fn unigram(&mut self, db: &impl ReadStore, token: Token) -> Result<u64, UserStoreError> {
        if let Some(&hit) = self.unigram.get(&token) {
            return Ok(hit);
        }
        match get_u64(db, UNIGRAM, &codec::encode_token(token))? {
            Some(value) => {
                memo_insert(&mut self.unigram, token, value);
                Ok(value)
            }
            None => Ok(0),
        }
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

    /// `BIGRAM[(prev, cur)]`, memoised like [`Self::unigram`]: present
    /// rows enter the memo, absent ones do not.
    fn bigram(
        &mut self,
        db: &impl ReadStore,
        prev: Token,
        cur: Token,
    ) -> Result<u64, UserStoreError> {
        if let Some(&hit) = self.bigram.get(&(prev, cur)) {
            return Ok(hit);
        }
        match get_u64(db, BIGRAM, &codec::encode_token_pair(prev, cur))? {
            Some(value) => {
                memo_insert(&mut self.bigram, (prev, cur), value);
                Ok(value)
            }
            None => Ok(0),
        }
    }

    /// `BIGRAM_TOTAL[prev]`, memoised like [`Self::unigram`]: present rows
    /// enter the memo, absent ones do not.
    fn bigram_total(&mut self, db: &impl ReadStore, prev: Token) -> Result<u64, UserStoreError> {
        if let Some(&hit) = self.bigram_total.get(&prev) {
            return Ok(hit);
        }
        match get_u64(db, BIGRAM_TOTAL, &codec::encode_token(prev))? {
            Some(value) => {
                memo_insert(&mut self.bigram_total, prev, value);
                Ok(value)
            }
            None => Ok(0),
        }
    }

    fn unigram_delta(&mut self, db: &impl ReadStore, token: Token) -> Result<u64, UserStoreError> {
        self.unigram(db, token)
    }

    fn count_delta(
        &mut self,
        db: &impl ReadStore,
        prev: Option<Token>,
        token: Token,
    ) -> Result<UserCountDelta, UserStoreError> {
        let unigram_delta = self.unigram(db, token)?;
        let unigram_total_delta = self.unigram_total(db)?;
        let (bigram_count, bigram_total) = if let Some(prev) = prev {
            (self.bigram(db, prev, token)?, self.bigram_total(db, prev)?)
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

impl<S: WriteStore> GenericUserStore<S> {
    pub(crate) fn stored_system_items(
        &self,
    ) -> std::collections::BTreeMap<Token, oxpinyin_data::chunk_write::ChunkItem> {
        self.inner
            .system_items
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Whether this session carries system-item payloads from imports or logs.
    #[must_use]
    pub fn has_system_items(&self) -> bool {
        self.inner.has_system_items.load(Ordering::Relaxed)
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
        let db = self.database();
        let Some(mut item) = self
            .inner
            .system_items
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&token)
            .cloned()
        else {
            return Ok(None);
        };
        let base = self
            .inner
            .libpinyin
            .as_ref()
            .and_then(|target| target.originals.get(&phrase_index_library_index(token)))
            .and_then(|library| library.unigram(token & PHRASE_MASK))
            .unwrap_or(0);
        item.unigram =
            base.wrapping_add(get_u64_or(&*db, UNIGRAM, &codec::encode_token(token), 0)? as u32);
        Ok(Some(item))
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
            .range((u32::from(library) << 24)..(u32::from(library + 1) << 24))
            .next_back()
            .and_then(|(&token, _)| token.checked_add(1))
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
    /// `original_token` comes from the system phrase-table search, restricted
    /// to this library; prior imports are resolved by the scratch text index.
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
        use oxpinyin_data::chunk_write::{ChunkItem, encode_phrase_item};
        if !(1..=4).contains(&library) || !phrase::phrase_and_keys_valid(text, keys) {
            return Err(UserStoreError::InvalidPhrase);
        }
        let target = self
            .inner
            .libpinyin
            .as_ref()
            .ok_or(UserStoreError::InvalidPhrase)?;
        let original = target
            .originals
            .get(&library)
            .ok_or(UserStoreError::InvalidPhrase)?;
        let packed = crate::store_libpinyin::pinyin_keys_to_packed(keys)
            .ok_or(UserStoreError::InvalidPhrase)?;
        let delta = u32::try_from(count.unwrap_or(DEFAULT_PHRASE_COUNT)).unwrap_or(u32::MAX);
        let db = self.database();
        let mut system_items = self
            .inner
            .system_items
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let lib_key = codec::encode_u8_str(library, text);
        let existing = match db.get(PHRASE_BY_LIB_TEXT, &lib_key)? {
            Some(bytes) => Some(codec::decode_token(&bytes).map_err(|_| UserStoreError::Decode)?),
            None => original_token,
        };
        let (token, mut item) = if let Some(token) = existing {
            let item = system_items
                .get(&token)
                .cloned()
                .or_else(|| original.item(token & PHRASE_MASK))
                .ok_or(UserStoreError::InvalidPhrase)?;
            (token, item)
        } else {
            let token = db.write(|txn| {
                // get_range's end, pinyin.cpp:587-595; seeded from the shipped
                // range and replayed ADD records, never from another library.
                let token = match txn.get(ALLOC, &codec::encode_u8(library))? {
                    Some(bytes) => codec::decode_token(&bytes)
                        .map_err(|_| StoreError::Backend("corrupt system cursor".into()))?,
                    None => (u32::from(library) << 24) | original.range_end.max(1),
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
                add_pronunciation(txn, token, &phrase::encode_keys(keys), delta, true)?;
                // The two facade add_index calls write the user tables. Only
                // this first reading gets an index row (pinyin.cpp:598-607).
                add_unigram_frequency(
                    txn,
                    library,
                    token,
                    delta.wrapping_mul(ADD_PHRASE_UNIGRAM_FACTOR_U32),
                )?;
                Ok(token)
            })?;
            (
                token,
                ChunkItem {
                    phrase: text.chars().map(u32::from).collect(),
                    unigram: 0,
                    prons: Vec::new(),
                },
            )
        };
        // PhraseItem::add_pronunciation, phrase_index.cpp:56-91: the
        // overflow test uses the prefix total through the matching row.
        // _add_phrase ignores a refused increment and still returns true.
        let mut total = 0_u32;
        let mut matched = false;
        for (reading, frequency) in &mut item.prons {
            total = total.wrapping_add(*frequency);
            if *reading == packed {
                if delta == 0 || total <= total.wrapping_add(delta) {
                    *frequency = frequency.wrapping_add(delta);
                }
                matched = true;
                break;
            }
        }
        if !matched {
            item.prons.push((packed.clone(), delta));
        }
        // Validate the eventual packed payload before publishing it.
        encode_phrase_item(&item).map_err(|error| StoreError::Backend(error.to_string().into()))?;
        system_items.insert(token, item);
        drop(system_items);
        self.inner.has_system_items.store(true, Ordering::Relaxed);
        self.mark_committed_phrase_write(db, true);
        Ok(token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxpinyin_store::Visitor;
    use std::collections::BTreeMap;

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
                        .write(|txn| txn.put(PHRASE_TABLE, &phrase_table_key(token, "乙"), &[]))
                        .unwrap();

                    // The own pair is still present, so removal succeeds and
                    // drops only that pair; the unrelated membership stays.
                    assert!(store.remove_user_phrase(token).unwrap());
                    assert!(store.phrase(token).unwrap().is_none());
                    assert!(
                        store
                            .database()
                            .get(PHRASE_TABLE, &phrase_table_key(token, "乙"))
                            .unwrap()
                            .is_some(),
                        "removing the phrase drops only its own pair"
                    );
                    cleanup(&path);
                }

                #[test]
                fn reopening_a_standalone_store_backfills_phrase_table() {
                    // A store written before `PHRASE_TABLE` existed has
                    // `PHRASE` rows but no membership rows. Opening it
                    // standalone must derive the pairs from `PHRASE`, or
                    // every pre-upgrade phrase would refuse removal.
                    let path = temp_path("backfill");
                    let token = {
                        let mut store = Store::create_standalone(&path).unwrap();
                        let token = store.add_phrase("你好", &[10, 20], None).unwrap();
                        // Parent-written shape: drop the membership row.
                        store
                            .database()
                            .write(|txn| txn.remove(PHRASE_TABLE, &phrase_table_key(token, "你好")))
                            .unwrap();
                        assert!(
                            store
                                .database()
                                .get(PHRASE_TABLE, &phrase_table_key(token, "你好"))
                                .unwrap()
                                .is_none()
                        );
                        token
                    };

                    let mut reopened = Store::create_standalone(&path).unwrap();
                    assert!(reopened.phrase(token).unwrap().is_some());
                    assert!(
                        reopened.remove_user_phrase(token).unwrap(),
                        "the reopen backfilled the phrase-table membership"
                    );
                    assert!(reopened.phrase(token).unwrap().is_none());
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

        fn range(
            &self,
            _table: &str,
            _lo: std::ops::Bound<&[u8]>,
            _hi: std::ops::Bound<&[u8]>,
            _visit: &mut Visitor<'_>,
        ) -> Result<(), StoreError> {
            Err(StoreError::Backend("stub ranges nothing".into()))
        }

        fn for_each(&self, _table: &str, _visit: &mut Visitor<'_>) -> Result<(), StoreError> {
            Err(StoreError::Backend("stub walks nothing".into()))
        }

        fn is_empty(&self, table: &str) -> Result<bool, StoreError> {
            if table != UNIGRAM {
                return Ok(true);
            }
            Ok(self.0.is_empty())
        }
    }

    #[test]
    fn absent_rows_read_zero_without_being_memoised() {
        let db = MemoDb(BTreeMap::from([(codec::encode_token(1).to_vec(), 5_u64)]));
        let mut cache = CountCache::new(0);
        assert_eq!(cache.unigram(&db, 2).unwrap(), 0);
        assert_eq!(cache.unigram(&db, 2).unwrap(), 0);
        assert!(
            !cache.unigram.contains_key(&2),
            "a zero miss must not occupy memo space"
        );
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
