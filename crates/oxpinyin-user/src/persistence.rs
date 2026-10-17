//! The user directory in libpinyin's own file shapes — drop-in task 9's
//! persistence half.
//!
//! The store's durable representation is the pin's file set
//! (`docs/findings/user-store.md` §4 for the save cycle's semantics,
//! `oxpinyin_data::user_files` for the inventory and codecs):
//!
//! * load — `check_format` (user.conf conformance, the open counter,
//!   `_clean_user_files` on a non-conform profile), then the bigram
//!   hash, the `USER_FILE` chunk stores (`user.bin`, `addon.bin`,
//!   `network.bin`), and the `SYSTEM_FILE` `.dbin` logs replayed onto the
//!   original system chunks, and the user pinyin index's records.
//!   `user_phrase_index.bin` records are loaded and saved verbatim
//!   ([`UserState::phrase_table`]); only phrase-table operations edit them.
//!   `user_pinyin_index.bin` is
//!   not a derivative — a reading merged into an existing phrase is
//!   never indexed
//!   (`pinyin.cpp:569-582`) — so its records are loaded
//!   ([`UserState::indexed`]) and saved back.
//! * save — the pin's `_write_files` + `_rename_files`: every file is
//!   written whole to a `.tmp` sibling, then all are renamed over their
//!   finals, so a crash mid-save leaves the previous profile intact.
//! * fini — `pinyin_fini`'s `user.conf` write: the open counter the load
//!   raised, lowered again.
//!
//! The two facades share the files and the `user.conf` codec but not its
//! lifecycle: [`UserConfLaw`] names which one a profile follows.
//!
//! Nothing here holds a container open: the DBM files are created,
//! written and closed during save, and opened read-only during load —
//! the pin's own lifecycle (`Bigram::load_db` copies into memory,
//! `save_db` writes a fresh file).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use oxpinyin_core::ChewingKey;
use oxpinyin_data::chunk_format::{CHUNK_HEADER_SIZE, build_memory_chunk};
use oxpinyin_data::chunk_write::{
    ChunkItem, PHRASE_MASK, build_chunk, decode_phrase_item, decode_sub_phrase_index,
    encode_phrase_item,
};
use oxpinyin_data::phrase_libraries::PhraseLibraries;
use oxpinyin_data::pin_stderr;
use oxpinyin_data::row_format::pinyin_index::PinyinIndexItem;
use oxpinyin_data::table_entries::pinyin_index_entries;
use oxpinyin_data::table_info::FileName;
use oxpinyin_data::user_files::{
    LogRecord, SystemVersions, UserConfError, UserDbm, UserFileLayout, UserTableInfo,
    decode_log_records, encode_log_records, get_open_counter, read_chunk_payload,
};
use oxpinyin_store::{
    DefaultStore, DefaultUserBigramDb, RawReadStore, StoreError, UserBigramDb, WriteStore,
};

use crate::store::SaveReport;

/// `USER_TABLE_INFO` (`pinyin_internal.h:56`).
const USER_CONF: &str = "user.conf";

/// Reads `user.conf`'s bytes, splitting upstream's two failure kinds the
/// way `check_format` meets them.
///
/// `Ok(None)` is a marker upstream's `load` cannot complete: the file is
/// absent, or a version directive did not convert (`table_info.cpp:339-342`,
/// `:345-348`). Upstream then runs its conformance check against the
/// reset defaults, so the profile reads as non-conforming and is wiped —
/// [`load`] does the same.
///
/// `Err` is the class-(c) site: the `database format:` directive reached
/// `to_table_database_format_type`, which `abort()`s upstream
/// (`:122-133`, called at `:353-354`). No profile is judged and nothing
/// is written or cleaned — the open fails, as `check_format` never gets
/// to run.
fn parse_user_conf(bytes: &[u8]) -> Result<Option<UserTableInfo>, PersistenceError> {
    match UserTableInfo::parse(bytes) {
        Ok(info) => Ok(Some(info)),
        Err(UserConfError::Line(_)) => Ok(None),
        Err(UserConfError::UnknownDatabaseFormat) => Err(PersistenceError::UnknownDatabaseFormat),
    }
}

/// Which facade's `user.conf` lifecycle a profile follows. libpinyin and
/// libzhuyin read and write the same marker through the same
/// `UserTableInfo`, but at different points and with different counters,
/// so the facade that opens a profile names its law and every write below
/// follows it ([`load`], [`save`]'s counter, [`fini`]).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UserConfLaw {
    /// libpinyin: every `pinyin_init` raises the open counter and writes
    /// the marker (`check_format`, `pinyin.cpp:185-187`); `pinyin_save`
    /// writes the context's counter back (`mark_version`, `:1143`,
    /// `:220-232`); `pinyin_fini` lowers it and writes again, with or
    /// without a save before it (`:1194-1200`). A process that dies
    /// between init and fini leaves the counter raised, and the init that
    /// reads it past `OPEN_COUNTER_LIMIT` wipes the profile.
    Pinyin,
    /// libzhuyin: `zhuyin_init` only reads the marker (`check_format`,
    /// `zhuyin.cpp:126-162`: conformance and the wipe, no counter step and
    /// no write); `zhuyin_save` writes a fresh `UserTableInfo`, so its
    /// counter is 0 (`mark_version`, `:164-176`, called at `:695`);
    /// `zhuyin_fini` writes nothing (`:741-757`).
    Zhuyin,
}

/// One system library's original state — the shipped chunk as loaded,
/// the diff base of its `.dbin` log.
#[derive(Clone, Debug, Default)]
pub struct SystemLibrary {
    /// The chunk's `total_freq`.
    pub total: u32,
    /// The pin's trimmed allocation range end (phrase_index.cpp:624-646).
    pub range_end: u32,
    /// Items by slot (`token & PHRASE_MASK`).
    pub items: BTreeMap<u32, ChunkItem>,
    /// Shared immutable runtime mapping; inline originals serve fixture stores.
    pub mapped: Option<oxpinyin_data::phrase_library::PhraseLibrary>,
}

impl SystemLibrary {
    pub(crate) fn contains_item(&self, slot: u32) -> bool {
        self.items.contains_key(&slot)
            || self
                .mapped
                .as_ref()
                .is_some_and(|library| library.item(slot).is_some())
    }

    pub(crate) fn unigram(&self, slot: u32) -> Option<u32> {
        self.items
            .get(&slot)
            .map(|item| item.unigram)
            .or_else(|| self.mapped.as_ref()?.item(slot).map(|view| view.unigram()))
    }

    pub(crate) fn item(&self, slot: u32) -> Option<ChunkItem> {
        if let Some(item) = self.items.get(&slot) {
            return Some(item.clone());
        }
        let view = self.mapped.as_ref()?.item(slot)?;
        Some(ChunkItem {
            phrase: view.phrase_text()?.chars().map(u32::from).collect(),
            unigram: view.unigram(),
            prons: view
                .pronunciations()
                .map(|pron| {
                    let keys = pron
                        .keys
                        .chunks_exact(2)
                        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
                        .collect();
                    (keys, pron.freq)
                })
                .collect(),
        })
    }

    /// Whether [`Self::item`] would answer `Some` for `slot`, without building
    /// the item: an inline item, or a mapped one whose phrase text decodes.
    pub(crate) fn is_readable(&self, slot: u32) -> bool {
        self.items.contains_key(&slot)
            || self.mapped.as_ref().is_some_and(|library| {
                library
                    .item(slot)
                    .is_some_and(|view| view.phrase_chars().all(|scalar| scalar.is_some()))
            })
    }

    /// The frequency `keys` carries in the item [`Self::item`] would build for
    /// `slot` — the first pronunciation with exactly those keys — or `0` when
    /// the item has none (or no item is readable). Read in place from the
    /// mapping; nothing is allocated.
    pub(crate) fn pronunciation_frequency(&self, slot: u32, keys: &[u16]) -> u32 {
        if let Some(item) = self.items.get(&slot) {
            return item
                .prons
                .iter()
                .find(|(base, _)| base.as_slice() == keys)
                .map_or(0, |(_, frequency)| *frequency);
        }
        let Some(view) = self.mapped.as_ref().and_then(|library| library.item(slot)) else {
            return 0;
        };
        if !view.phrase_chars().all(|scalar| scalar.is_some()) {
            return 0; // `item` is `None` for an item whose text does not decode
        }
        view.pronunciations()
            .find(|pron| {
                pron.keys
                    .chunks_exact(2)
                    .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
                    .eq(keys.iter().copied())
            })
            .map_or(0, |pron| pron.freq)
    }

    /// `self.item(slot).map(|current| current == decoded(other))`, decided in
    /// place: `None` when no item is readable at `slot`, otherwise whether it
    /// equals `other`. A mapped item is compared against the mapping without
    /// being copied out; its text is still scanned to the end first, so an item
    /// whose phrase does not decode is `None` whatever else differs.
    pub(crate) fn item_matches(&self, slot: u32, other: &WireItem<'_>) -> Option<bool> {
        if let Some(item) = self.items.get(&slot) {
            return Some(other.equals(item));
        }
        let view = self.mapped.as_ref()?.item(slot)?;
        let mut theirs = other.phrase();
        let mut equal = view.phrase_length() == other.phrase_length;
        for scalar in view.phrase_chars() {
            equal &= theirs.next() == Some(u32::from(scalar?));
        }
        equal &= view.unigram() == other.unigram;
        equal &= view.n_pronunciations() == other.n_prons;
        if equal {
            // Both key runs are little-endian `u16`s of even length, so equal
            // runs are equal bytes.
            equal = view
                .pronunciations()
                .zip(other.pronunciations())
                .all(|(pron, (keys, frequency))| pron.freq == frequency && pron.keys == keys);
        }
        Some(equal)
    }
}

/// A journal record's `PhraseItem` bytes, validated as [`decode_phrase_item`]
/// validates them and read in place: what a replay needs of an *old* item (to
/// compare it with the held one, and its unigram and stored length) without
/// building the owned [`ChunkItem`] it would discard.
///
/// [`WireItem::parse`] accepts exactly the payloads `decode_phrase_item`
/// accepts: the header, the phrase's `ucs4_t` run and every pronunciation must
/// fit, and bytes after the last pronunciation are ignored. No scalar check
/// happens there either; the phrase stays raw `u32`s.
pub(crate) struct WireItem<'a> {
    phrase_length: usize,
    n_prons: usize,
    unigram: u32,
    /// The phrase's `4 * phrase_length` bytes.
    phrase: &'a [u8],
    /// The pronunciations' `n_prons * (2 * phrase_length + 4)` bytes.
    prons: &'a [u8],
}

impl<'a> WireItem<'a> {
    /// `None` exactly when `decode_phrase_item(bytes)` fails.
    pub(crate) fn parse(bytes: &'a [u8]) -> Option<Self> {
        let phrase_length = usize::from(*bytes.first()?);
        let n_prons = usize::from(*bytes.get(1)?);
        let unigram = u32::from_le_bytes(bytes.get(2..6)?.try_into().ok()?);
        let phrase_end = 6 + 4 * phrase_length;
        let phrase = bytes.get(6..phrase_end)?;
        let prons = bytes.get(phrase_end..phrase_end + n_prons * (2 * phrase_length + 4))?;
        Some(Self {
            phrase_length,
            n_prons,
            unigram,
            phrase,
            prons,
        })
    }

    /// The unigram the decoded item carries.
    pub(crate) const fn unigram(&self) -> u32 {
        self.unigram
    }

    /// `phrase_item_wire_len(&decoded)`: `encode_phrase_item`'s length for the
    /// decoded item, or its refusal. The decoder already guarantees the
    /// pronunciation count and key runs, so only an empty phrase is refused.
    pub(crate) const fn wire_len(&self) -> Option<usize> {
        if self.phrase_length == 0 {
            return None;
        }
        Some(6 + self.phrase.len() + self.prons.len())
    }

    fn phrase(&self) -> impl Iterator<Item = u32> + '_ {
        self.phrase
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
    }

    /// Each pronunciation's packed key bytes and frequency, in stored order.
    fn pronunciations(&self) -> impl Iterator<Item = (&[u8], u32)> + '_ {
        self.prons
            .chunks_exact(2 * self.phrase_length + 4)
            .map(|run| {
                let (keys, freq) = run.split_at(2 * self.phrase_length);
                (
                    keys,
                    u32::from_le_bytes([freq[0], freq[1], freq[2], freq[3]]),
                )
            })
    }

    /// `*item == decode_phrase_item(bytes)`.
    fn equals(&self, item: &ChunkItem) -> bool {
        self.unigram == item.unigram
            && self.n_prons == item.prons.len()
            && self.phrase().eq(item.phrase.iter().copied())
            && self.pronunciations().zip(&item.prons).all(
                |((packed, frequency), (keys, expected))| {
                    frequency == *expected
                        && packed
                            .chunks_exact(2)
                            .map(|c| u16::from_le_bytes([c[0], c[1]]))
                            .eq(keys.iter().copied())
                },
            )
    }
}

/// The `SYSTEM_FILE` libraries' originals together with the user dir's
/// file layout (`table.conf`'s user file names): everything a profile's
/// load and save need to know about the system side.
#[derive(Clone, Debug)]
pub struct SystemOriginals {
    libraries: BTreeMap<u8, SystemLibrary>,
    layout: UserFileLayout,
}

impl SystemOriginals {
    /// `libraries` under `layout`.
    #[must_use]
    pub const fn new(libraries: BTreeMap<u8, SystemLibrary>, layout: UserFileLayout) -> Self {
        Self { libraries, layout }
    }

    /// The user dir's file layout.
    #[must_use]
    pub const fn layout(&self) -> &UserFileLayout {
        &self.layout
    }
}

impl std::ops::Deref for SystemOriginals {
    type Target = BTreeMap<u8, SystemLibrary>;

    fn deref(&self) -> &Self::Target {
        &self.libraries
    }
}

impl std::ops::DerefMut for SystemOriginals {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.libraries
    }
}

/// The libraries under the stock layout.
impl From<BTreeMap<u8, SystemLibrary>> for SystemOriginals {
    fn from(libraries: BTreeMap<u8, SystemLibrary>) -> Self {
        Self::new(libraries, UserFileLayout::stock())
    }
}

/// Builds the `SYSTEM_FILE` libraries' originals from the runtime's
/// opened chunks — the `.dbin` diff base, and the conformance source
/// for replaying a profile's logs.
///
/// Items whose UCS-4 text does not
/// decode are skipped (a malformed entry, never a panic).
/// Only library metadata and shared mappings are retained here: decoding
/// phrases and pronunciations is deferred to a touched slot's [`SystemLibrary`].
/// A fresh profile therefore pays no full-item pass (#588).
#[must_use]
pub fn system_originals(libraries: &PhraseLibraries, layout: UserFileLayout) -> SystemOriginals {
    let mut out = BTreeMap::new();
    for &(nibble, _) in layout.system_logs() {
        let Some(library) = libraries.library((u32::from(nibble) << 24) | 1) else {
            continue;
        };
        out.insert(
            nibble,
            SystemLibrary {
                total: library.total_freq(),
                range_end: library.token_range().end,
                items: BTreeMap::new(),
                mapped: Some(library.clone()),
            },
        );
    }
    SystemOriginals::new(out, layout)
}

/// The user state, in the value shapes the file set carries.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct UserState {
    /// The user bigram's rows verbatim, `(key, SingleGram value)` in the
    /// file's walk order: what [`load`] read and what a [`save`] without a
    /// container writes. A session never fills it — its grams live in the
    /// pin's own container, which its saves write instead.
    pub bigram: Vec<(Vec<u8>, Vec<u8>)>,
    /// The user phrase table's raw UCS-4 keys and token-array values.
    /// Loaded and saved verbatim, including prefix markers, duplicate tokens,
    /// and partial bytes, as `PhraseLargeTable3::load_db` / `save_db` copy
    /// the records (074a2219, `phrase_large_table3_bdb.cpp:128-174`).
    /// Phrase-table operations alone edit these records; library items do
    /// not reconstruct their contents.
    pub phrase_table: BTreeMap<Vec<u8>, Vec<u8>>,
    /// The `USER_FILE` sub-indexes (5, 6, 7) by nibble, items by slot.
    pub libraries: BTreeMap<u8, BTreeMap<u32, ChunkItem>>,
    /// The `SYSTEM_FILE` libraries' touched items by nibble and slot:
    /// `Some(item)` modifies the original, `None` removes it. A slot
    /// absent here still answers its original item.
    pub system_overrides: BTreeMap<u8, BTreeMap<u32, Option<ChunkItem>>>,
    /// The `USER_FILE` readings the user pinyin index carries, as
    /// `(token, packed ChewingKey words)`. `_add_phrase` indexes the
    /// reading a phrase is created with and never one merged into an
    /// existing phrase (`pinyin.cpp:569-600`), so the index is not a
    /// derivative of the items: it is loaded from `user_pinyin_index.bin`
    /// and saved from this set.
    pub indexed: std::collections::BTreeSet<(u32, Vec<u16>)>,
}

impl UserState {
    /// Fixture state whose `USER_FILE` phrases and readings were all indexed
    /// when created. Production state never reconstructs phrase-table rows.
    #[cfg(test)]
    pub(crate) fn index_every_reading(mut self) -> Self {
        let mut phrases = Vec::new();
        for (&nibble, items) in &self.libraries {
            for (&slot, item) in items {
                phrases.push((item.phrase.clone(), token_of(nibble, slot)));
                for (packed, _) in &item.prons {
                    self.indexed
                        .insert((token_of(nibble, slot), packed.clone()));
                }
            }
        }
        self.phrase_table = oxpinyin_data::table_entries::phrase_index_entries(&phrases)
            .into_iter()
            .collect();
        self
    }
}

/// Which of `MemoryChunk::save`'s two header writes the filesystem
/// refused (`src/include/memory_chunk.h`): the `length` word (`:543`,
/// `assert(ret_len == sizeof(length))`) or the `checksum` word (`:547`,
/// `assert(ret_len == sizeof(checksum))`).
///
/// Both sit under `pinyin_save` / `zhuyin_save`, which write every
/// library chunk (`pinyin.cpp:988`, `zhuyin.cpp:645`), and the pin is
/// built with asserts live, so a full filesystem or an `RLIMIT_FSIZE`
/// that cuts the header kills the process. The class-(c) answer keeps
/// only the failure; the save stops there, as the abort would have.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChunkHeaderField {
    /// `write(fd, &length, sizeof(guint32))` — `memory_chunk.h:543`.
    Length,
    /// `write(fd, &checksum, sizeof(guint32))` — `memory_chunk.h:547`.
    Checksum,
}

impl ChunkHeaderField {
    /// The pin's assert expression at this write, for the facade's
    /// class-(c) warning.
    #[must_use]
    pub const fn assert_expression(self) -> &'static str {
        match self {
            Self::Length => "ret_len == sizeof(length)",
            Self::Checksum => "ret_len == sizeof(checksum)",
        }
    }
}

/// A persistence failure: I/O, a container, or a byte stream that does
/// not parse.
#[derive(Debug)]
pub enum PersistenceError {
    /// Filesystem failure.
    Io(std::io::Error),
    /// A store container failed to open, write or read.
    Store(StoreError),
    /// A byte stream did not parse under its frozen format.
    Codec(String),
    /// A library chunk's `MemoryChunk::save` header write failed
    /// (`memory_chunk.h:543`/`:547`): the pin `assert`s and dies of
    /// SIGABRT. Carried apart from [`Self::Io`] because a short *payload*
    /// write is the pin's own soft failure (`save` answers `false` and
    /// every caller ignores it), not an abort.
    ChunkHeaderWrite {
        /// Which header word the write refused.
        field: ChunkHeaderField,
        /// The underlying write error.
        source: std::io::Error,
    },
    /// `user.conf` names a database format upstream's
    /// `to_table_database_format_type` does not know, where that function
    /// `abort()`s (`table_info.cpp:122-133`). The class-(c) answer: the
    /// open fails instead of the process dying, and nothing is cleaned or
    /// written.
    UnknownDatabaseFormat,
}

impl std::fmt::Display for PersistenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "user file io: {error}"),
            Self::Store(error) => write!(f, "user file store: {error}"),
            Self::Codec(message) => write!(f, "user file codec: {message}"),
            Self::ChunkHeaderWrite { field, source } => write!(
                f,
                "user chunk header ({}): {source}",
                field.assert_expression()
            ),
            Self::UnknownDatabaseFormat => write!(
                f,
                "user.conf: unknown database format (upstream aborts, \
                 table_info.cpp:122-133, so the open is refused)"
            ),
        }
    }
}

impl std::error::Error for PersistenceError {}

impl From<std::io::Error> for PersistenceError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<StoreError> for PersistenceError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

impl From<oxpinyin_data::chunk_format::ChunkFrameError> for PersistenceError {
    fn from(error: oxpinyin_data::chunk_format::ChunkFrameError) -> Self {
        PersistenceError::Codec(error.to_string())
    }
}

impl From<oxpinyin_data::chunk_write::ChunkWriteError> for PersistenceError {
    fn from(error: oxpinyin_data::chunk_write::ChunkWriteError) -> Self {
        Self::Codec(error.to_string())
    }
}

/// The result of [`load`].
#[derive(Clone, Debug, Default)]
pub struct Loaded {
    /// The decoded state (empty on a fresh or wiped profile).
    pub state: UserState,
    /// The open counter every later `user.conf` write of this session
    /// starts from: under [`UserConfLaw::Pinyin`] the value the load
    /// raised and wrote, which [`save`] writes back and [`fini`] lowers;
    /// under [`UserConfLaw::Zhuyin`] 0, the counter of the fresh
    /// `UserTableInfo` libzhuyin's `mark_version` saves. Upstream's `int`
    /// (`table_info.h:102`), so a negative marker raises to a negative
    /// value.
    pub open_counter: i32,
    /// A non-conform profile was found and its files removed —
    /// `check_format`'s `_clean_user_files`, the ecosystem's own
    /// mechanism for "backend or model change discards user data".
    /// Read by the tests here; the runtime wiring logs it.
    #[cfg_attr(not(test), allow(dead_code))]
    pub wiped: bool,
    /// Files whose bytes failed to parse; the profile continues without
    /// them (upstream's own degrade — `chunk->load` failure leaves an
    /// empty library, never a failed init).
    pub skipped: Vec<String>,
    /// A `SYSTEM_FILE`/`DICTIONARY` `.dbin` whose `MODIFY_HEADER` record
    /// carried a non-null token. Upstream `assert(token == null_token)`s
    /// there (`phrase_index_logger.h:202`), so this is a class (c) abort
    /// site, not the graceful [`Self::skipped`] list: the session open
    /// fails and the facade logs one warning in its own domain. Recorded
    /// as `(file name, token)`.
    pub strict_log_fault: Option<(String, u32)>,
    /// The total each library the profile filled holds once loaded, as the
    /// pin keeps it: a `USER_FILE` library's is the total stored in its
    /// file (074a2219 `phrase_index.cpp:345`), not a sum of its items; a
    /// system library's is its original's after the `.dbin` replay, which
    /// sets it from each `MODIFY_HEADER` and moves it with every item added
    /// or removed (`phrase_index.cpp:439-520`). The facade adds each to its
    /// own total (`phrase_index.cpp:237-248`), so the session prices with
    /// these and not with the items' sum.
    pub(crate) library_totals: BTreeMap<u8, u32>,
}

/// The private result of a profile parse: the public [`Loaded`] plus
/// the user pinyin index's key metadata needed by session wiring.
#[derive(Clone, Debug, Default)]
pub(crate) struct LoadedProfile {
    /// The public load result.
    pub(crate) loaded: Loaded,
    /// The user pinyin index's readings longer than `MAX_PHRASE_LENGTH`
    /// syllables, as packed `ChewingKey` words — the pin's user
    /// `ChewingLargeTable2` carries one DB key per reading, and a key
    /// past the table's 16-syllable instantiation drives
    /// `chewing_large_table2_bdb.cpp:282`/`:529` into `switch`'s
    /// `default: abort()`. The value model cannot hold such a reading
    /// (its `(token, keys)` item exists only for a `user.bin` reading of
    /// at most `MAX_PHRASE_LENGTH`), so the raw keys ride here as a
    /// sibling of [`UserState`] for the two abort sites — recorded from
    /// the DB key's length alone, independent of the value decoding. A key
    /// with no whole word (empty, or one byte) is recorded as an empty
    /// reading: the same `switch` has no case for a word count of 0, so
    /// `mask_out` aborts on it (`:529`).
    pub(crate) overlong_index_keys: Vec<Vec<u16>>,
    /// Every raw key `user_pinyin_index.bin` carries, as packed
    /// `ChewingKey` words — the pin's user `ChewingLargeTable2` btree rows
    /// (readings and `add_index`'s prefix markers alike). The
    /// longer-candidate gate consults the exact query key's existence
    /// (the pin's `DB_SET` probe, `chewing_large_table2_bdb.cpp:576`)
    /// before it walks extensions, and a crafted index can hold an
    /// over-long key whose prefix row is absent — the pin then never
    /// starts the walk.
    pub(crate) index_keys: BTreeSet<Vec<u16>>,
    /// The user pinyin index's retained keys ([`IndexKeys`]).
    pub(crate) index_file_keys: IndexKeys,
}

/// Every retained key of the user pinyin index. The chewing table's
/// `mask_out` and `remove_index` retain empty records, so saves write these
/// keys even when no remaining reading reconstructs them. Phrase-table
/// records, including empty values, live directly in [`UserState::phrase_table`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct IndexKeys {
    /// `user_pinyin_index.bin` keys.
    pub(crate) pinyin: BTreeSet<Vec<u8>>,
    /// The records of `user_pinyin_index.bin` whose bytes differ from what
    /// the held readings derive (#785): the tails a Kyoto Cabinet or tkrzw
    /// mask walk leaves in them.
    #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
    pub(crate) tails: BTreeMap<Vec<u8>, Vec<u8>>,
    /// The keys the session's adds wrote, each standing for itself and its
    /// prefixes; with [`Self::pinyin`], every key the table holds (#785).
    #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
    pub(crate) added: BTreeSet<crate::pinyin_chain::ChainKey>,
    /// What each key length's table entry holds (#785).
    #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
    pub(crate) slots: [crate::pinyin_chain::Slot; crate::phrase::MAX_PHRASE_LENGTH + 1],
}

impl IndexKeys {
    /// Keeps every key `add_index` wrote for these rows — each row's key
    /// and its prefix markers, in both pinyin key spaces — before the rows
    /// leave the store.
    pub(crate) fn keep_rows(&mut self, pinyin: &[PinyinIndexItem]) {
        self.pinyin
            .extend(pinyin_index_entries(pinyin).into_iter().map(|(key, _)| key));
    }
}

/// The readings `add_index` saw, as the pinyin index's rows: those the
/// libraries carry and `state.indexed` names, and the indexed readings of
/// items added to a system library.
pub(crate) fn chewing_rows(state: &UserState, originals: &SystemOriginals) -> Vec<PinyinIndexItem> {
    // The pinyin index holds the readings `add_index` saw. The phrase
    // index is independently stored and copied verbatim below.
    let mut chewing_rows: Vec<PinyinIndexItem> = Vec::new();
    for (&nibble, items) in &state.libraries {
        for (&slot, item) in items {
            let token = token_of(nibble, slot);
            for (packed, _freq) in &item.prons {
                if !state.indexed.contains(&(token, packed.clone())) {
                    continue;
                }
                chewing_rows.push(PinyinIndexItem {
                    token,
                    keys: packed
                        .iter()
                        .map(|&bits| ChewingKey::from_packed(bits))
                        .collect(),
                });
            }
        }
    }
    // Both add_index calls target the user trees even for a system token
    // (074a2219, facade_phrase_table3.h:166-172 / facade_chewing_table2.h:167-172).
    for (&nibble, overrides) in &state.system_overrides {
        for (&slot, item) in overrides {
            if originals
                .get(&nibble)
                .is_some_and(|base| base.contains_item(slot))
            {
                continue;
            }
            let Some(item) = item else {
                continue;
            };
            let token = token_of(nibble, slot);
            for (packed, _) in &item.prons {
                if state.indexed.contains(&(token, packed.clone())) {
                    chewing_rows.push(PinyinIndexItem {
                        token,
                        keys: packed
                            .iter()
                            .map(|&bits| ChewingKey::from_packed(bits))
                            .collect(),
                    });
                }
            }
        }
    }
    // The index also holds entries no item reading backs: a dropped item's,
    // kept by the pin's index tables (074a2219 `phrase_index.cpp:261-268`),
    // and a profile's own. They are written as they stand.
    let written: BTreeSet<(u32, Vec<u16>)> = chewing_rows
        .iter()
        .map(|row| {
            (
                row.token,
                row.keys.iter().map(|key| key.to_packed()).collect(),
            )
        })
        .collect();
    for (token, packed) in state.indexed.difference(&written) {
        if packed.is_empty() || packed.len() > crate::phrase::MAX_PHRASE_LENGTH {
            continue;
        }
        chewing_rows.push(PinyinIndexItem {
            token: *token,
            keys: packed
                .iter()
                .map(|&bits| ChewingKey::from_packed(bits))
                .collect(),
        });
    }
    chewing_rows
}

/// `rows` with the records a mask walk left tails in (#785).
#[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
fn with_tails(kept: &IndexKeys, rows: Vec<(Vec<u8>, Vec<u8>)>) -> Vec<(Vec<u8>, Vec<u8>)> {
    kept.with_tails(rows)
}

/// Berkeley DB keeps no tails.
#[cfg(not(any(feature = "kyotocabinet", feature = "tkrzw")))]
const fn with_tails(_kept: &IndexKeys, rows: Vec<(Vec<u8>, Vec<u8>)>) -> Vec<(Vec<u8>, Vec<u8>)> {
    rows
}

/// `rows` with every kept key that no row writes added as an empty value,
/// in ascending key order.
pub(crate) fn with_kept_keys(
    rows: Vec<(Vec<u8>, Vec<u8>)>,
    kept: &BTreeSet<Vec<u8>>,
) -> Vec<(Vec<u8>, Vec<u8>)> {
    if kept.is_empty() {
        return rows;
    }
    let mut merged: BTreeMap<Vec<u8>, Vec<u8>> = rows.into_iter().collect();
    for key in kept {
        merged.entry(key.clone()).or_default();
    }
    merged.into_iter().collect()
}

/// The full token of a system-library item.
const fn token_of(nibble: u8, slot: u32) -> u32 {
    ((nibble as u32) << 24) | slot
}

/// Reads the user dir: `check_format` first, then the profile.
///
/// This reproduces the facade's init-time user-dir half under `law`.
/// libpinyin raises the open counter and writes `user.conf` at every
/// init, conform or not (`pinyin.cpp:185-187`); libzhuyin reads the
/// marker and writes nothing (`zhuyin.cpp:126-162`). Both wipe a
/// non-conform profile — a counter past the limit included — and keep
/// the marker file itself, which neither pin's wipe removes.
///
/// # Errors
///
/// Returns [`PersistenceError::UnknownDatabaseFormat`] for the
/// `database format:` abort point — the class-(c) refusal, raised before
/// any judgement, wipe or write (see [`parse_user_conf`]) — and otherwise
/// only when the `user.conf` write itself fails; unparsable profile files
/// degrade per-file (see [`Loaded::skipped`]), as upstream's do.
pub fn load(
    dir: &Path,
    originals: &SystemOriginals,
    versions: &SystemVersions,
    law: UserConfLaw,
) -> Result<Loaded, PersistenceError> {
    let check = check_format(dir, versions, originals.layout(), law)?;
    let mut loaded = load_checked(dir, originals, &check).loaded;
    if check.conform {
        load_bigram_rows(dir, &mut loaded);
    }
    Ok(loaded)
}

/// [`load`] with the crate-private result: the judgement, the profile read
/// and the pinyin index key metadata the session wiring needs.
///
/// # Errors
///
/// As [`load`].
pub(crate) fn load_profile(
    dir: &Path,
    originals: &SystemOriginals,
    versions: &SystemVersions,
    law: UserConfLaw,
) -> Result<LoadedProfile, PersistenceError> {
    let check = check_format(dir, versions, originals.layout(), law)?;
    Ok(load_checked(dir, originals, &check))
}

/// What `check_format` decided about a profile: the open counter this
/// session starts from and whether the files are kept.
#[derive(Clone, Copy, Debug)]
pub struct ProfileCheck {
    open_counter: i32,
    conform: bool,
}

/// `check_format`'s half of [`load`]: judge the `user.conf` marker, raise
/// the counter, wipe a non-conform profile and write the marker. The pin
/// runs it first in `pinyin_init`, before any library is loaded
/// (`pinyin.cpp:337-346`), so a facade whose init can fail later runs it
/// ahead of those loads, then reads the profile with `load_checked`.
///
/// # Errors
///
/// As [`load`].
pub(crate) fn check_format(
    dir: &Path,
    versions: &SystemVersions,
    layout: &UserFileLayout,
    law: UserConfLaw,
) -> Result<ProfileCheck, PersistenceError> {
    let conf_path = dir.join(USER_CONF);
    let existing = match std::fs::read(&conf_path) {
        Ok(bytes) => parse_user_conf(&bytes)?,
        Err(_) => {
            // `UserTableInfo::load`'s raw `fprintf` (`table_info.cpp:332`):
            // no newline, as the pin's has none, and the same line when the
            // marker is absent as when it cannot be opened.
            pin_stderr::emit(&[b"open ", pin_stderr::path_bytes(&conf_path), b" failed."]);
            None
        }
    };

    let conform = existing
        .as_ref()
        .is_some_and(|info| info.is_conform(versions));

    // libpinyin's raise, exactly `check_format`'s arithmetic
    // (`pinyin.cpp:185-186`): `get_open_counter() + 1`, where a missing
    // or unparsable marker reads 0 (`UserTableInfo::load` resets before
    // it parses, `table_info.cpp:325-326`). A conform profile raised to 7
    // fails `is_conform` at the next init, which wipes and writes 1.
    // libzhuyin keeps no counter of its own: its writes carry the fresh
    // `UserTableInfo`'s 0.
    let open_counter = match law {
        UserConfLaw::Pinyin => {
            get_open_counter(existing.as_ref().map_or(0, |info| info.open_counter)) + 1
        }
        UserConfLaw::Zhuyin => 0,
    };

    if !conform {
        clean_user_files(dir, layout);
    }
    if law == UserConfLaw::Pinyin {
        write_marker(dir, versions, open_counter)?;
    }
    Ok(ProfileCheck {
        open_counter,
        conform,
    })
}

/// The profile half of [`load`], over a profile `check_format` judged:
/// nothing to read when it was wiped. Returns the crate-private
/// [`LoadedProfile`] so the pinyin index key metadata survives to the
/// session wiring.
#[must_use]
pub(crate) fn load_checked(
    dir: &Path,
    originals: &SystemOriginals,
    check: &ProfileCheck,
) -> LoadedProfile {
    let mut profile = LoadedProfile {
        loaded: Loaded {
            open_counter: check.open_counter,
            wiped: !check.conform,
            ..Loaded::default()
        },
        overlong_index_keys: Vec::new(),
        index_keys: BTreeSet::new(),
        index_file_keys: IndexKeys::default(),
    };
    if check.conform {
        load_libraries(dir, originals.layout(), &mut profile.loaded, None);
        #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
        let mut records = Vec::new();
        load_user_pinyin_index(
            dir,
            &mut profile.loaded,
            &mut profile.overlong_index_keys,
            &mut profile.index_keys,
            &mut profile.index_file_keys.pinyin,
            #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
            &mut records,
        );
        load_user_phrase_index(
            dir,
            &mut profile.loaded.skipped,
            &mut profile.loaded.state.phrase_table,
        );
        load_logs(dir, originals, &mut profile.loaded);
        // The file's records are the table's: one that the readings do not
        // derive keeps its bytes (#785).
        #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
        {
            profile.index_file_keys.tails = crate::pinyin_chain::file_tails(
                &profile.loaded.state,
                originals,
                &profile.index_file_keys.pinyin,
                records,
            );
        }
    }
    profile
}

/// `pinyin_fini`'s arithmetic (`pinyin.cpp:1196-1197`): the counter the
/// init raised, read through `get_open_counter`, lowered by one and
/// floored at 0 — `counter > 1 ? counter - 1 : 0`. A raised 7 therefore
/// reads 0 and lowers to 0, not to 6; a negative counter lowers to 0.
#[must_use]
pub const fn lowered_open_counter(open_counter: i32) -> i32 {
    let counter = get_open_counter(open_counter);
    if counter > 1 { counter - 1 } else { 0 }
}

/// The facade's fini-time `user.conf` write under `law`: libpinyin
/// lowers the counter the load raised and writes the conform marker
/// (`pinyin_fini`, `pinyin.cpp:1195-1200`, whose `mark_version` runs
/// whether or not a save came first); libzhuyin writes nothing
/// (`zhuyin_fini`, `zhuyin.cpp:741-757`). Nothing else of the profile
/// is touched.
///
/// A process that never reaches its fini leaves the load's raised value
/// on disk, as upstream's does.
///
/// # Errors
///
/// Returns [`PersistenceError`] when the marker cannot be written.
pub fn fini(
    dir: &Path,
    versions: &SystemVersions,
    law: UserConfLaw,
    open_counter: i32,
) -> Result<(), PersistenceError> {
    match law {
        UserConfLaw::Pinyin => write_marker(dir, versions, lowered_open_counter(open_counter)),
        UserConfLaw::Zhuyin => Ok(()),
    }
}

/// `mark_version`'s write: the conform marker (`make_conform`) with
/// `open_counter`, straight over `user.conf` as `UserTableInfo::save`'s
/// `fopen("w")` does (`table_info.cpp:377-397`).
fn write_marker(
    dir: &Path,
    versions: &SystemVersions,
    open_counter: i32,
) -> Result<(), PersistenceError> {
    let mut marker = UserTableInfo::conform_to(versions);
    marker.open_counter = open_counter;
    std::fs::write(dir.join(USER_CONF), marker.to_text())?;
    Ok(())
}

/// `_clean_user_files` + the fixed names: every data file of the profile
/// is removed; absence is not an error. `user.conf` is not among them —
/// neither pin's `check_format` unlinks it (`pinyin.cpp:194-215`,
/// `zhuyin.cpp:141-159`): libpinyin has already rewritten it by then, and
/// libzhuyin leaves the non-conform marker in place until a save.
fn clean_user_files(dir: &Path, layout: &UserFileLayout) {
    let mut names: Vec<FileName> = [
        UserDbm::Bigram.file_name(),
        UserDbm::PinyinIndex.file_name(),
        UserDbm::PhraseIndex.file_name(),
    ]
    .into_iter()
    .map(|name| FileName::from_bytes(name.as_bytes()))
    .collect();
    names.extend(layout.user_libraries().iter().map(|(_, name)| name.clone()));
    names.extend(layout.system_logs().iter().map(|(_, name)| name.clone()));
    for name in names {
        let _ = std::fs::remove_file(name.under(dir));
    }
}

/// The user bigram's rows for a caller with no container of its own
/// ([`load`]): every `(key, value)` of `user_bigram.db` verbatim, in the
/// file's walk order. The container is the backend's *user*-bigram form —
/// a Kyoto Cabinet snapshot stream, a tkrzw hash file
/// (`RawReadStore::open_user_bigram`); the system `bigram.db` is a
/// different container on Kyoto Cabinet and must not share an open path.
/// Nothing is decoded: the grams are the pin's `SingleGram` bytes, read by
/// the pin's own operations wherever they are used. A session reads the
/// file once, into its container ([`load_user_bigram_db`]), not here.
fn load_bigram_rows(dir: &Path, loaded: &mut Loaded) {
    let path = dir.join(UserDbm::Bigram.file_name());
    if !path.exists() {
        return;
    }
    // A backend that locks can leave a lock sidecar beside the *final*
    // file even on a read-only open; it is stale once the handle drops,
    // and the user dir must hold exactly the pin's names. Removed at the
    // end of this function, after the walk.
    let store = match DefaultStore::open_user_bigram(&path) {
        Ok(store) => store,
        Err(error) => {
            loaded
                .skipped
                .push(format!("{}: {error}", UserDbm::Bigram.file_name()));
            // The failed open can have created a lock sidecar (a
            // locking backend writes one even when it then rejects the
            // file): the inventory rule applies on this path too.
            remove_dbm_sidecars(dir, &path);
            return;
        }
    };
    let mut visit = |key: &[u8], value: &[u8]| -> Result<(), StoreError> {
        loaded.state.bigram.push((key.to_vec(), value.to_vec()));
        Ok(())
    };
    let walked = store.range_raw(
        std::ops::Bound::Unbounded,
        std::ops::Bound::Unbounded,
        &mut visit,
    );
    if let Err(error) = walked {
        loaded
            .skipped
            .push(format!("{}: {error}", UserDbm::Bigram.file_name()));
    }
    drop(store);
    remove_dbm_sidecars(dir, &path);
}

/// The user pinyin index's `(token, keys)` records — which `USER_FILE`
/// readings lookup finds — plus the raw key set, the over-long readings
/// and the keys a save writes back ([`IndexKeys::pinyin`]). Both keyspaces
/// carry every record, so the set absorbs the duplicate. An absent or
/// unreadable file indexes nothing, as an empty `ChewingLargeTable2` does.
fn load_user_pinyin_index(
    dir: &Path,
    loaded: &mut Loaded,
    overlong: &mut Vec<Vec<u16>>,
    index_keys: &mut BTreeSet<Vec<u16>>,
    file_keys: &mut BTreeSet<Vec<u8>>,
    #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))] records: &mut Vec<(Vec<u8>, Vec<u8>)>,
) {
    let path = dir.join(UserDbm::PinyinIndex.file_name());
    if !path.exists() {
        return;
    }
    let store = match DefaultStore::open_user_index(&path) {
        Ok(store) => store,
        Err(error) => {
            loaded
                .skipped
                .push(format!("{}: {error}", UserDbm::PinyinIndex.file_name()));
            remove_dbm_sidecars(dir, &path);
            return;
        }
    };
    let indexed = &mut loaded.state.indexed;
    // The over-long readings arrive from directly crafted or corrupt DBMs
    // and their count is unbounded, so deduplicate through a set rather
    // than rescanning the accumulated vector (which would be quadratic in
    // the number of rows). The ordered set preserves the walk's order in
    // `overlong` for the callers that read it as a slice.
    let mut seen: BTreeSet<Vec<u16>> = BTreeSet::new();
    let mut visit = |key: &[u8], value: &[u8]| -> Result<(), StoreError> {
        if key.len() < 2 {
            // No whole word: the pin's word count `key.size /
            // sizeof(ChewingKey)` is 0, which `mask_out`'s `switch` has no
            // case for (`chewing_large_table2_bdb.cpp:529`; kc `:499`,
            // tkrzw `:466`). Record the empty reading beside the over-long
            // ones so the same mask refusal covers it. Nothing else reads
            // it: it extends no query and is not an exact key.
            if seen.insert(Vec::new()) {
                overlong.push(Vec::new());
            }
            return Ok(()); // not a key upstream writes
        }
        let words: Vec<u16> = key
            .chunks_exact(2)
            .map(|word| u16::from_le_bytes([word[0], word[1]]))
            .collect();
        // The pin's `m_db->Get`/`DB_SET` probe (`chewing_large_table2_bdb.cpp:576`)
        // consults an exact query key, so record every even-length row —
        // reading or prefix marker — for [`GenericUserStore::overlong_extension_gate`]
        // to test. A malformed odd-length row is not an exact key and is
        // left out.
        if key.len().is_multiple_of(2) {
            index_keys.insert(words.clone());
        }
        // The pin's user `ChewingLargeTable2` instantiates its entries only
        // for lengths 1..=16, and both the longer-candidate walk
        // (`chewing_large_table2_bdb.cpp:282`) and `mask_out` (`:529`)
        // compute the word count by integer division (`key.size /
        // sizeof(ChewingKey)`). A 35-byte key therefore reads as 17 words
        // and reaches `switch`'s `default: abort()` even though the key is
        // not one upstream writes, so classify it before rejecting the
        // malformed trailing byte. Record the complete-word prefix as
        // packed `ChewingKey` words; the pin aborts on the key, before
        // touching the value.
        if key.len() / 2 > crate::phrase::MAX_PHRASE_LENGTH && seen.insert(words.clone()) {
            overlong.push(words);
        }
        if !key.len().is_multiple_of(2) {
            return Ok(()); // not a key upstream writes
        }
        if key.len() / 2 <= crate::phrase::MAX_PHRASE_LENGTH {
            file_keys.insert(key.to_vec());
            #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
            records.push((key.to_vec(), value.to_vec()));
        }
        if value.is_empty() {
            return Ok(()); // a prefix marker names no reading
        }
        let Ok(items) = oxpinyin_data::row_format::pinyin_index::decode_items(value, key.len() / 2)
        else {
            return Ok(());
        };
        for item in items {
            indexed.insert((
                item.token,
                item.keys.iter().map(|key| key.to_packed()).collect(),
            ));
        }
        Ok(())
    };
    if let Err(error) = store.range_raw(
        std::ops::Bound::Unbounded,
        std::ops::Bound::Unbounded,
        &mut visit,
    ) {
        loaded
            .skipped
            .push(format!("{}: {error}", UserDbm::PinyinIndex.file_name()));
    }
    drop(store);
    remove_dbm_sidecars(dir, &path);
}

/// Copy `user_phrase_index.bin` records without interpreting either key or value,
/// as the pin's `PhraseLargeTable3::load_db` does. Missing or unreadable files
/// leave the table empty.
fn load_user_phrase_index(
    dir: &Path,
    skipped: &mut Vec<String>,
    table: &mut BTreeMap<Vec<u8>, Vec<u8>>,
) {
    let path = dir.join(UserDbm::PhraseIndex.file_name());
    if !path.exists() {
        return;
    }
    let store = match DefaultStore::open_user_index(&path) {
        Ok(store) => store,
        Err(error) => {
            skipped.push(format!("{}: {error}", UserDbm::PhraseIndex.file_name()));
            remove_dbm_sidecars(dir, &path);
            return;
        }
    };
    let mut visit = |key: &[u8], value: &[u8]| -> Result<(), StoreError> {
        table.insert(key.to_vec(), value.to_vec());
        Ok(())
    };
    if let Err(error) = store.range_raw(
        std::ops::Bound::Unbounded,
        std::ops::Bound::Unbounded,
        &mut visit,
    ) {
        skipped.push(format!("{}: {error}", UserDbm::PhraseIndex.file_name()));
    }
    drop(store);
    remove_dbm_sidecars(dir, &path);
}

/// `pinyin_init`'s `m_user_bigram->load_db(user_bigram.db)`
/// (`pinyin.cpp:399-401`): the pin's own in-memory container, filled from
/// the file in the file's walk order. Opened after [`load`], so a profile
/// `check_format` wiped loads empty, as the pin's does.
///
/// # Errors
///
/// Returns [`PersistenceError`] when the in-memory container cannot be
/// created.
pub(crate) fn load_user_bigram_db(dir: &Path) -> Result<DefaultUserBigramDb, PersistenceError> {
    let path = dir.join(UserDbm::Bigram.file_name());
    let db = DefaultUserBigramDb::load_db(&path)?;
    // As in `load_bigram`: a locking backend's sidecar beside the final
    // file is stale once the read handle is gone.
    remove_dbm_sidecars(dir, &path);
    Ok(db)
}

/// The `USER_FILE` chunk stores, or only library `only`'s.
fn load_libraries(dir: &Path, layout: &UserFileLayout, loaded: &mut Loaded, only: Option<u8>) {
    for (nibble, file) in layout.user_libraries() {
        let nibble = *nibble;
        if only.is_some_and(|only| only != nibble) {
            continue;
        }
        let name = file.display();
        let path = file.under(dir);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            // Absent is the normal fresh-profile case. Any other error
            // is an unreadable *existing* file: loading as if it were
            // absent would let the next save rebuild it from empty and
            // destroy the user's data with no record, so it lands in
            // `skipped` for the caller to see.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                loaded.skipped.push(format!("{name}: {e}"));
                continue;
            }
        };
        let payload = match read_chunk_payload(&bytes) {
            Ok(payload) => payload,
            Err(error) => {
                loaded.skipped.push(format!("{name}: {error}"));
                continue;
            }
        };
        match decode_sub_phrase_index(payload) {
            Ok((total, items)) => {
                loaded.library_totals.insert(nibble, total);
                // An absent library and an empty one are the same state;
                // keep the map absent so save/load are byte- and
                // value-stable round trips.
                if !items.is_empty() {
                    loaded
                        .state
                        .libraries
                        .insert(nibble, items.into_iter().collect());
                }
            }
            Err(error) => loaded.skipped.push(format!("{name}: {error}")),
        }
    }
}

/// The `.dbin` logs, replayed onto the original system chunks —
/// `FacadePhraseIndex::merge`. A record whose old payload does not
/// match the current item stops that library's replay (upstream's
/// `merge` returns false mid-log and `_load_phrase_library` keeps the
/// partially-merged index).
fn load_logs(dir: &Path, originals: &SystemOriginals, loaded: &mut Loaded) {
    load_logs_with_mask(
        SystemLogSource::Directory(dir),
        originals,
        loaded,
        None,
        false,
    );
}

/// One reloaded library's saved state: the `.dbin` log in the user
/// directory replayed onto the freshly mapped system original —
/// `_load_phrase_library`'s `load` then `merge` (074a2219
/// `pinyin.cpp:264-277`, `zhuyin.cpp:298-312`). Only the libraries in
/// `originals` are read; unsaved session state never enters it.
pub(crate) fn load_library_logs(
    dir: &Path,
    originals: &SystemOriginals,
) -> (UserState, BTreeMap<u8, u32>) {
    let mut loaded = Loaded::default();
    load_logs_with_mask(
        SystemLogSource::Directory(dir),
        originals,
        &mut loaded,
        None,
        true,
    );
    (loaded.state, loaded.library_totals)
}

/// One reloaded `USER_FILE` library's saved state: its chunk in the user
/// directory, or nothing when the file is absent or unreadable —
/// `_load_phrase_library`'s `USER_FILE` branch, which loads the chunk or
/// creates an empty sub-index (074a2219 `pinyin.cpp:303-321`,
/// `zhuyin.cpp:247-265`). Unsaved session state never enters it.
pub(crate) fn load_user_library(
    dir: &Path,
    layout: &UserFileLayout,
    nibble: u8,
) -> (UserState, BTreeMap<u8, u32>) {
    let mut loaded = Loaded::default();
    load_libraries(dir, layout, &mut loaded, Some(nibble));
    (loaded.state, loaded.library_totals)
}

/// Decoded system logs read and validated before mask_out mutates state.
/// Replay consumes these same records without reopening any .dbin, matching
/// the pin's single MemoryChunk read (074a2219 pinyin.cpp:1278-1285,
/// zhuyin.cpp:813-820).
#[derive(Debug, Default)]
pub struct SystemLogSnapshot {
    pub(crate) records: BTreeMap<u8, Vec<LogRecord>>,
}

/// The pin's masked logger: check the library bits, require a header,
/// keep only unselected slot records, then replay the new logger over
/// the freshly mapped system original (074a2219 phrase_index.cpp:301-330,
/// :850-875). The caller has already validated the log's abort sites.
pub(crate) fn load_masked_logs(
    originals: &SystemOriginals,
    mask: u32,
    value: u32,
    snapshot: SystemLogSnapshot,
) -> (UserState, BTreeMap<u8, u32>) {
    let mut loaded = Loaded::default();
    load_logs_with_mask(
        SystemLogSource::Snapshot(snapshot),
        originals,
        &mut loaded,
        Some((mask, value)),
        false,
    );
    (loaded.state, loaded.library_totals)
}

enum SystemLogSource<'a> {
    Directory(&'a Path),
    Snapshot(SystemLogSnapshot),
}

fn load_logs_with_mask(
    mut source: SystemLogSource<'_>,
    originals: &SystemOriginals,
    loaded: &mut Loaded,
    mask: Option<(u32, u32)>,
    only_originals: bool,
) {
    for (nibble, file) in originals.layout().system_logs() {
        let nibble = *nibble;
        if only_originals && !originals.contains_key(&nibble) {
            continue;
        }
        if mask.is_some_and(|(mask, value)| {
            !originals.contains_key(&nibble)
                || (nibble & crate::phrase::phrase_index_library_index(mask))
                    != crate::phrase::phrase_index_library_index(value)
        }) {
            continue;
        }
        let name = file.display();
        let mut records = match &mut source {
            SystemLogSource::Snapshot(snapshot) => {
                let Some(records) = snapshot.records.remove(&nibble) else {
                    continue;
                };
                records
            }
            SystemLogSource::Directory(dir) => {
                let path = file.under(dir);
                let bytes = match std::fs::read(&path) {
                    Ok(bytes) => bytes,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(e) => {
                        loaded.skipped.push(format!("{name}: {e}"));
                        continue;
                    }
                };
                let payload = match read_chunk_payload(&bytes) {
                    Ok(payload) => payload,
                    Err(error) => {
                        loaded.skipped.push(format!("{name}: {error}"));
                        continue;
                    }
                };
                match decode_log_records(payload) {
                    Ok(records) => records,
                    Err(error) => {
                        // A non-null-`MODIFY_HEADER` token is the pin's
                        // `assert(token == null_token)` (`phrase_index_logger.h:202`):
                        // a class (c) abort site, recorded for the session open to
                        // fail on. Every other malformed stream is the pin's
                        // graceful `next_record` false and stays a skip.
                        if let Some(token) = error.non_null_header_token() {
                            loaded.strict_log_fault = Some((name.to_string(), token));
                        } else {
                            loaded.skipped.push(format!("{name}: {error}"));
                        }
                        continue;
                    }
                }
            }
        };
        if let Some((mask, value)) = mask {
            let Some(old_total) = records.iter().find_map(|record| match record {
                LogRecord::ModifyHeader { old_total, .. } => Some(*old_total),
                _ => None,
            }) else {
                continue; // _peek_header's false: the new logger is empty
            };
            records.retain(|record| match record {
                LogRecord::Add { token, .. }
                | LogRecord::Remove { token, .. }
                | LogRecord::Modify { token, .. } => {
                    (token & (mask & PHRASE_MASK)) != (value & PHRASE_MASK)
                }
                LogRecord::ModifyHeader { .. } => false,
            });
            // The generated logger writes its header first, regardless of
            // where the old logger held it. Its new total is the old one
            // moved by every record kept, read from the records' own
            // payloads (`_compute_new_header`, 074a2219
            // `phrase_index.cpp:750-804`).
            let new_total = records
                .iter()
                .fold(old_total, |total, record| match record {
                    LogRecord::Add { new_item, .. } => total.wrapping_add(raw_unigram(new_item)),
                    LogRecord::Remove { old_item, .. } => total.wrapping_sub(raw_unigram(old_item)),
                    LogRecord::Modify {
                        old_item, new_item, ..
                    } => total
                        .wrapping_sub(raw_unigram(old_item))
                        .wrapping_add(raw_unigram(new_item)),
                    LogRecord::ModifyHeader { .. } => total,
                });
            records.insert(
                0,
                LogRecord::ModifyHeader {
                    old_total,
                    new_total,
                },
            );
        }

        let original = originals.get(&nibble);
        let mut overrides: BTreeMap<u32, Option<ChunkItem>> = BTreeMap::new();
        // `SubPhraseIndex::m_total_freq` through `merge`, a wrapping
        // `guint32`: a header sets it, an added item adds its unigram, a
        // removed one takes the current item's away, and a modified one
        // moves it only when the new item is longer and is removed and
        // added back; an in-place modify leaves it (074a2219
        // `phrase_index.cpp:439-520`, `:208-235`).
        let mut total = original.map(|library| library.total);
        // Whether the item the replay holds at `slot` — an override, else
        // the original's — equals `probe`; `None` when there is none.
        let current_matches = |overrides: &BTreeMap<u32, Option<ChunkItem>>,
                               slot: u32,
                               probe: &WireItem<'_>|
         -> Option<bool> {
            match overrides.get(&slot) {
                Some(Some(item)) => Some(probe.equals(item)),
                Some(None) => None,
                None => original.and_then(|library| library.item_matches(slot, probe)),
            }
        };

        // The pin emits local slots, but its reader masks high bits even
        // in hand-written records (074a2219 phrase_index.cpp:180-215, 219-232).
        for record in records {
            match record {
                LogRecord::Add { token, new_item } => match decode_phrase_item(&new_item) {
                    Ok(item) => {
                        total = total.map(|total| total.wrapping_add(item.unigram));
                        overrides.insert(token & PHRASE_MASK, Some(item));
                    }
                    Err(error) => {
                        loaded.skipped.push(format!("{name}: {error}"));
                        break;
                    }
                },
                LogRecord::Remove { token, old_item } => {
                    let slot = token & PHRASE_MASK;
                    let matched = WireItem::parse(&old_item)
                        .map(|old| (current_matches(&overrides, slot, &old), old));
                    match matched {
                        // An equal item has the record's own unigram.
                        Some((Some(true), old)) => {
                            total = total.map(|total| total.wrapping_sub(old.unigram()));
                            overrides.insert(slot, None);
                        }
                        Some((None, _)) => {
                            overrides.insert(slot, None); // already gone; a no-op removal
                        }
                        _ => {
                            // payload mismatch: merge's failure return,
                            // recorded so a clean load is distinguishable
                            // from a truncated replay.
                            loaded.skipped.push(format!(
                                "{name}: replay stopped at a mismatched REMOVE for {token:#010x}"
                            ));
                            break;
                        }
                    }
                }
                LogRecord::Modify {
                    token,
                    old_item,
                    new_item,
                } => {
                    let slot = token & PHRASE_MASK;
                    // Only the new item is kept, so only it is built.
                    let old = WireItem::parse(&old_item)
                        .filter(|old| current_matches(&overrides, slot, old) == Some(true));
                    match (old, decode_phrase_item(&new_item)) {
                        (Some(old), Ok(new)) => {
                            // The held item equals `old`, so its stored
                            // form and unigram are `old`'s.
                            let grows =
                                old.wire_len().is_some_and(|stored| new_item.len() > stored);
                            if grows {
                                total = total.map(|total| {
                                    total.wrapping_sub(old.unigram()).wrapping_add(new.unigram)
                                });
                            }
                            overrides.insert(slot, Some(new));
                        }
                        _ => {
                            // payload mismatch: merge's failure return,
                            // recorded (see the REMOVE note above).
                            loaded.skipped.push(format!(
                                "{name}: replay stopped at a mismatched MODIFY for {token:#010x}"
                            ));
                            break;
                        }
                    }
                }
                LogRecord::ModifyHeader {
                    old_total,
                    new_total,
                } => {
                    // The header must land on the current total, as the
                    // merge has moved it so far; a mismatch is merge's
                    // failure return. A header that lands sets the total,
                    // which is the old one moved by `new_total - old_total`
                    // (`phrase_index.cpp:503-514`).
                    let fallback = SystemLibrary::default();
                    let now = total.unwrap_or_else(|| {
                        system_new_total(original.unwrap_or(&fallback), &overrides)
                    });
                    if now != old_total {
                        loaded
                            .skipped
                            .push(format!("{name}: replay stopped at a mismatched header"));
                        break;
                    }
                    total = total.map(|_| new_total);
                }
            }
        }
        if let Some(total) = total {
            loaded.library_totals.insert(nibble, total);
        }
        // An empty override set and an absent one are the same state.
        if !overrides.is_empty() {
            loaded.state.system_overrides.insert(nibble, overrides);
        }
    }
}

/// `PhraseItem::get_unigram_frequency` read from a record's raw item: the
/// `guint32` after the two length bytes (074a2219 `phrase_index.h:124-126`),
/// `0` when the payload is too short to hold one.
fn raw_unigram(item: &[u8]) -> u32 {
    item.get(2..6)
        .and_then(|bytes| bytes.try_into().ok())
        .map_or(0, u32::from_le_bytes)
}

/// The library's new `total_freq` — the original plus the overrides'
/// deltas.
///
/// `m_total_freq`'s incremental arithmetic recomputed deterministically
/// (saturating; upstream drops a delta that would overflow instead, an
/// edge no training run reaches).
#[must_use]
pub fn system_new_total(
    original: &SystemLibrary,
    overrides: &BTreeMap<u32, Option<ChunkItem>>,
) -> u32 {
    let mut total = original.total;
    for (slot, override_item) in overrides {
        match (original.item(*slot), override_item) {
            (Some(old), Some(new)) => {
                total = total.saturating_sub(old.unigram);
                total = total.saturating_add(new.unigram);
            }
            (Some(old), None) => {
                total = total.saturating_sub(old.unigram);
            }
            (None, Some(new)) => {
                total = total.saturating_add(new.unigram);
            }
            (None, None) => {}
        }
    }
    total
}

/// Writes the whole profile: `_write_files` (every file to its `.tmp`
/// sibling) then `_rename_files` (all renames), so a crash between the
/// passes leaves the previous profile intact.
///
/// `user.conf` is `mark_version`'s marker with `open_counter` — the
/// session's [`Loaded::open_counter`]: libpinyin's save writes back the
/// value its init raised (`pinyin.cpp:1143`), libzhuyin's a fresh 0
/// (`zhuyin.cpp:695`). Neither save moves the counter. It is written
/// after the renames, in place, not staged.
///
/// # Errors
///
/// Returns [`PersistenceError`] on the first write or rename failure;
/// the `.tmp` files of a failed save are removed, leaving the previous
/// profile untouched.
pub fn save(
    dir: &Path,
    state: &UserState,
    originals: &SystemOriginals,
    versions: &SystemVersions,
    open_counter: i32,
) -> Result<(), PersistenceError> {
    save_with_bigram(dir, state, originals, versions, open_counter, None)
}

/// [`save`], with the session's in-memory user bigram container: when
/// given, `user_bigram.db` is that container written the pin's way —
/// `m_user_bigram->save_db(user_bigram.db.tmp)` (`pinyin.cpp:1014-1018`),
/// renamed with the rest of the file set — so the file's layout, and
/// the walk order of the next session's load, are the pin's. Without
/// one, `state.bigram` is written by key.
///
/// # Errors
///
/// As [`save`].
pub fn save_with_bigram(
    dir: &Path,
    state: &UserState,
    originals: &SystemOriginals,
    versions: &SystemVersions,
    open_counter: i32,
    bigram_db: Option<&DefaultUserBigramDb>,
) -> Result<(), PersistenceError> {
    let kept = IndexKeys::default();
    save_session(
        dir,
        SaveSource { state, kept: &kept },
        originals,
        versions,
        open_counter,
        bigram_db,
    )
}

/// What a save writes: the session's values, and the keys its user index
/// tables hold ([`IndexKeys`]), written empty where no row of `state`
/// writes them.
#[derive(Clone, Copy)]
pub(crate) struct SaveSource<'a> {
    pub(crate) state: &'a UserState,
    pub(crate) kept: &'a IndexKeys,
}

/// [`save_with_bigram`] for a session, whose `source` carries the index
/// keys its tables hold.
///
/// # Errors
///
/// As [`save`].
pub(crate) fn save_session(
    dir: &Path,
    source: SaveSource<'_>,
    originals: &SystemOriginals,
    versions: &SystemVersions,
    open_counter: i32,
    bigram_db: Option<&DefaultUserBigramDb>,
) -> Result<(), PersistenceError> {
    let mut staged = Staged::default();
    let result = stage_all(dir, source, originals, bigram_db, false, &mut staged);
    finish_save(dir, versions, open_counter, &staged.pending, result)
}

/// The files of a save in the pin's rename order (`_rename_files`,
/// `pinyin.cpp:1025-1130`): the libraries by index, then the two indices
/// and the bigram.
fn pin_rename_order(layout: &UserFileLayout) -> Vec<FileName> {
    let mut order: Vec<FileName> = libraries_in_pin_order(layout)
        .into_iter()
        .map(|(_, name, _)| name.clone())
        .collect();
    for dbm in [UserDbm::PinyinIndex, UserDbm::PhraseIndex, UserDbm::Bigram] {
        order.push(FileName::from_bytes(dbm.file_name().as_bytes()));
    }
    order
}

/// Which kind of library file a save writes.
#[derive(Clone, Copy)]
enum LibraryFile {
    /// A `SYSTEM_FILE` library's `.dbin` diff log.
    Log,
    /// A `USER_FILE` library's chunk.
    User,
}

/// The library files in the pin's loop order: one pass over the sub-indices
/// (`_write_files`, `_rename_files`, `pinyin.cpp:933-1090`), a stable sort
/// keeping the log before the chunk where a layout gives one index both.
fn libraries_in_pin_order(layout: &UserFileLayout) -> Vec<(u8, &FileName, LibraryFile)> {
    let mut libraries: Vec<(u8, &FileName, LibraryFile)> = layout
        .system_logs()
        .iter()
        .map(|(nibble, name)| (*nibble, name, LibraryFile::Log))
        .chain(
            layout
                .user_libraries()
                .iter()
                .map(|(nibble, name)| (*nibble, name, LibraryFile::User)),
        )
        .collect();
    libraries.sort_by_key(|&(nibble, _, _)| nibble);
    libraries
}

/// [`save_with_bigram`] the way the pin saves: no failure stops it. Each file
/// is written on its own; a file that cannot be written is a file that
/// cannot be renamed, a rename that fails is reported and the next one runs,
/// the marker is written last whatever happened, and the save answers as
/// having saved (`pinyin_save`'s `_write_files(context) &&
/// _rename_files(context)` is `true` for both, `pinyin.cpp:1132-1147`). One
/// failing file therefore leaves the others updated, as the pin does.
///
/// # Errors
///
/// Returns [`PersistenceError::Codec`] when a record cannot be encoded: an
/// internal failure the pin has no counterpart for. Returns
/// [`PersistenceError::ChunkHeaderWrite`] when a chunk file's header write
/// fails: the pin `assert`s and dies right there, so this is the one
/// failure the save does not carry past — no rename pass and no marker
/// follow it, and the caller logs the point.
pub(crate) fn save_with_bigram_reporting(
    dir: &Path,
    source: SaveSource<'_>,
    originals: &SystemOriginals,
    versions: &SystemVersions,
    open_counter: i32,
    bigram_db: Option<&DefaultUserBigramDb>,
    interleaved: bool,
) -> Result<SaveReport, PersistenceError> {
    // `_write_files` writes every file whatever the one before did
    // (`pinyin.cpp:940-1020`): a write that fails leaves no `.tmp` and the
    // next file is written anyway.
    let mut staged = Staged {
        immediate: interleaved,
        ..Staged::default()
    };
    stage_all(dir, source, originals, bigram_db, true, &mut staged)?;
    // `_rename_files` then renames every file of the set in its own order and
    // prints the ones that fail (`:1025-1130`), a file whose write failed
    // among them: the unaffected files are updated and the profile can mix
    // two saves.
    let mut outcome = SaveReport::default();
    if interleaved {
        // `zhuyin_save` renames each file as soon as it has written it.
        outcome.renames_failed = staged.failed;
    } else {
        for name in pin_rename_order(originals.layout()) {
            let tmp = name.with_suffix(".tmp").under(dir);
            let final_path = name.under(dir);
            if std::fs::rename(&tmp, &final_path).is_err() {
                outcome.renames_failed.push((tmp, final_path));
            }
        }
    }
    if write_marker(dir, versions, open_counter).is_err() {
        outcome.user_conf_write_failed = Some(dir.join(USER_CONF));
    }
    Ok(outcome)
}

/// The files a save has written to `.tmp` siblings. libpinyin writes them
/// all (`_write_files`) and then renames them all (`_rename_files`); libzhuyin
/// renames each as soon as it is written (`zhuyin_save`), so a name two roles
/// share ends with the other role's contents.
#[derive(Default)]
struct Staged {
    /// Waiting for the rename pass.
    pending: Vec<(PathBuf, PathBuf)>,
    /// Rename on the spot (libzhuyin).
    immediate: bool,
    /// The immediate renames that failed.
    failed: Vec<(PathBuf, PathBuf)>,
}

impl Staged {
    /// libzhuyin attempts the rename even after a soft write failure
    /// (074a2219 zhuyin.cpp:612-617, :645-650, :669-691). A header failure
    /// aborts before the rename; the reporting caller refuses it instead.
    fn finish(
        &mut self,
        paths: (PathBuf, PathBuf),
        written: Result<(), PersistenceError>,
    ) -> Result<(), PersistenceError> {
        if written.is_ok()
            || (self.immediate
                && matches!(
                    &written,
                    Err(PersistenceError::Io(_) | PersistenceError::Store(_))
                ))
        {
            self.push(paths);
        }
        written
    }

    fn push(&mut self, (tmp, final_path): (PathBuf, PathBuf)) {
        if !self.immediate {
            self.pending.push((tmp, final_path));
        } else if std::fs::rename(&tmp, &final_path).is_err() {
            self.failed.push((tmp, final_path));
        }
    }
}

/// [`save_with_bigram`]'s staging half, shared with the reporting save.
fn stage_all(
    dir: &Path,
    source: SaveSource<'_>,
    originals: &SystemOriginals,
    bigram_db: Option<&DefaultUserBigramDb>,
    lenient: bool,
    staged: &mut Staged,
) -> Result<(), PersistenceError> {
    // `_write_files` writes the libraries by sub-index, then the user pinyin
    // index, the user phrase index and the bigram (`pinyin.cpp:933-1020`):
    // where a `table.conf` gives two roles one name, the last writer's
    // contents are what the first rename moves into place.
    stage_rest(dir, source, originals, lenient, staged)?;
    let state = source.state;
    if let Some(db) = bigram_db {
        let final_path = dir.join(UserDbm::Bigram.file_name());
        let tmp = dir.join(format!("{}.tmp", UserDbm::Bigram.file_name()));
        remove_dbm_sidecars(dir, &tmp);
        let written = db.save_db(&tmp).map_err(PersistenceError::from);
        if written.is_ok() {
            remove_dbm_sidecars(dir, &tmp);
        }
        tolerate(lenient, staged.finish((tmp, final_path), written))?;
        return Ok(());
    }
    tolerate(lenient, stage_user_bigram(dir, &state.bigram))?;
    Ok(())
}

/// The reporting save carries on past a file it could not write, as the pin's
/// `_write_files` does; the strict save stops at the first. A chunk header
/// write the pin `assert`s on and an encoding failure stop both. `Ok(true)`
/// when the write succeeded.
fn tolerate<T>(
    lenient: bool,
    result: Result<T, PersistenceError>,
) -> Result<bool, PersistenceError> {
    match result {
        Ok(_) => Ok(true),
        Err(PersistenceError::Io(_) | PersistenceError::Store(_)) if lenient => Ok(false),
        Err(error) => Err(error),
    }
}

/// Every file of the set but the user bigram, staged as `.tmp` siblings.
fn stage_rest(
    dir: &Path,
    source: SaveSource<'_>,
    originals: &SystemOriginals,
    lenient: bool,
    staged: &mut Staged,
) -> Result<(), PersistenceError> {
    let SaveSource { state, kept } = source;
    {
        // ---- the library files, by sub-index -----------------------------
        let empty_library = SystemLibrary::default();
        for (nibble, name, kind) in libraries_in_pin_order(originals.layout()) {
            let bytes = match kind {
                LibraryFile::User => {
                    let pairs: Vec<(u32, ChunkItem)> =
                        state.libraries.get(&nibble).map_or(Vec::new(), |slots| {
                            slots
                                .iter()
                                .map(|(&slot, item)| (slot, item.clone()))
                                .collect()
                        });
                    build_chunk(&pairs)?
                }
                LibraryFile::Log => {
                    let original = originals.get(&nibble).unwrap_or(&empty_library);
                    let overrides = state
                        .system_overrides
                        .get(&nibble)
                        .cloned()
                        .unwrap_or_default();
                    let records = diff_records(nibble, original, &overrides)?;
                    let payload = encode_log_records(&records)
                        .map_err(|e| PersistenceError::Codec(e.to_string()))?;
                    build_memory_chunk(&payload).map_err(PersistenceError::from)?
                }
            };
            tolerate(lenient, stage_chunk(dir, name, &bytes, staged))?;
        }

        let chewing_rows = chewing_rows(state, originals);
        tolerate(
            lenient,
            stage_dbm(
                dir,
                UserDbm::PinyinIndex,
                &with_tails(
                    kept,
                    with_kept_keys(pinyin_index_entries(&chewing_rows), &kept.pinyin),
                ),
                staged,
            ),
        )?;
        tolerate(
            lenient,
            stage_dbm(
                dir,
                UserDbm::PhraseIndex,
                &state
                    .phrase_table
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect::<Vec<_>>(),
                staged,
            ),
        )?;

        Ok(())
    }
}

/// The save's commit: on success every staged `.tmp` renamed over its
/// final, then `user.conf`; on failure every staged `.tmp` removed.
fn finish_save(
    dir: &Path,
    versions: &SystemVersions,
    open_counter: i32,
    staged: &[(PathBuf, PathBuf)],
    result: Result<(), PersistenceError>,
) -> Result<(), PersistenceError> {
    match result {
        Ok(()) => {
            for (index, (tmp, final_path)) in staged.iter().enumerate() {
                if let Err(error) = std::fs::rename(tmp, final_path) {
                    // The rename pass is half-done: this `.tmp` and every
                    // one still staged behind it are orphans the profile
                    // does not own. Remove them so the user dir keeps
                    // exactly the pin's inventory — the already-renamed
                    // files are complete files and stay.
                    for (tmp, _) in &staged[index..] {
                        let _ = std::fs::remove_file(tmp);
                    }
                    return Err(error.into());
                }
            }
            // `user.conf` last, and in place: `mark_version` runs after
            // the renames and `fopen`s the marker over the existing file
            // (`pinyin.cpp:1141-1143`, `zhuyin.cpp:695`), so the file
            // keeps whatever mode it already had.
            write_marker(dir, versions, open_counter)
        }
        Err(error) => {
            for (tmp, _) in staged {
                let _ = std::fs::remove_file(tmp);
            }
            Err(error)
        }
    }
}

/// The minimal log a save leaves for one system library — the header
/// record plus a record per changed slot, in ascending token walk
/// (`SubPhraseIndex::diff`'s order).
///
/// # Errors
///
/// Returns the store's error when a diff walk or record append fails.
pub fn diff_records(
    _nibble: u8,
    original: &SystemLibrary,
    overrides: &BTreeMap<u32, Option<ChunkItem>>,
) -> Result<Vec<LogRecord>, PersistenceError> {
    let mut records = vec![LogRecord::ModifyHeader {
        old_total: original.total,
        new_total: system_new_total(original, overrides),
    }];

    // 074a2219 phrase_index.cpp:394-442 emits local SubPhraseIndex slots.
    // Only overridden slots can differ; walking this ordered sparse map
    // emits the same records without scanning every immutable original.
    for &slot in overrides.keys() {
        // An override entry always wins — including `Some(None)`, an
        // explicit removal: flattening it into the original item here
        // would drop the Remove record and resurrect the phrase on
        // reopen.
        let old = original.item(slot);
        let current = overrides.get(&slot).and_then(|item| item.as_ref());
        let token = slot;
        match (old.as_ref(), current) {
            (Some(old), Some(new)) => {
                if old != new {
                    records.push(LogRecord::Modify {
                        token,
                        old_item: encode_phrase_item(old).map_err(PersistenceError::from)?,
                        new_item: encode_phrase_item(new).map_err(PersistenceError::from)?,
                    });
                }
            }
            (Some(old), None) => {
                records.push(LogRecord::Remove {
                    token,
                    old_item: encode_phrase_item(old).map_err(PersistenceError::from)?,
                });
            }
            (None, Some(new)) => {
                records.push(LogRecord::Add {
                    token,
                    new_item: encode_phrase_item(new).map_err(PersistenceError::from)?,
                });
            }
            (None, None) => {}
        }
    }
    Ok(records)
}

/// Writes the user bigram to its `.tmp` sibling and registers the rename.
///
/// Routed through [`WriteStore::write_user_bigram`] rather than
/// [`stage_dbm`]'s container create, because the user bigram is not a
/// hash file on every backend: Kyoto Cabinet writes a snapshot stream of
/// an in-memory stash (`ngram_kyotodb.cpp:82-101`) where tkrzw
/// write a container at the path.
fn stage_user_bigram(dir: &Path, rows: &[(Vec<u8>, Vec<u8>)]) -> Result<(), PersistenceError> {
    // The seam stages its own sibling temporary and renames atomically
    // (S2's review fix), so this is a direct call on the final path —
    // no outer `.tmp`, and any lock sidecar of the *previous* file is
    // the seam's to handle.
    DefaultStore::write_user_bigram(&dir.join(UserDbm::Bigram.file_name()), rows)?;
    Ok(())
}

/// Writes one of the two index DBMs' rows to its `.tmp` sibling (a tree
/// container on every backend) and registers the rename. The user bigram
/// takes the separate [`stage_user_bigram`] route, since its container is
/// backend-specific.
fn stage_dbm(
    dir: &Path,
    dbm: UserDbm,
    rows: &[(Vec<u8>, Vec<u8>)],
    staged: &mut Staged,
) -> Result<(), PersistenceError> {
    debug_assert!(!dbm.is_hash(), "stage_dbm serves the two index trees");
    let final_path = dir.join(dbm.file_name());
    let tmp = dir.join(format!("{}.tmp", dbm.file_name()));
    // A crashed save can leave this `.tmp` behind with rows the coming
    // write would not overwrite — `create` opens-or-creates, so stale
    // index rows would survive into the renamed file.
    let written = (|| {
        match std::fs::remove_file(&tmp) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        remove_dbm_sidecars(dir, &tmp);
        // A user table, created with the mode the backend's own `save_db`
        // uses for one (Berkeley DB's 0600, not `attach`'s 0644).
        DefaultStore::write_user_index(&tmp, rows)?;
        // Remove a backend's stale lock sidecar before the data-file rename.
        remove_dbm_sidecars(dir, &tmp);
        Ok(())
    })();
    staged.finish((tmp, final_path), written)
}

/// The sidecar suffixes a DBM backend may keep beside a data file.
/// An explicit allowlist, not a prefix match: a prefix match would
/// delete any future or unrelated profile file that happens to share
/// the data file's stem.
const DBM_SIDECAR_SUFFIXES: [&str; 2] = ["-lock", "-shm"];

/// Removes the sidecar files a DBM backend created beside `dbm_path`.
/// Absence is not an error: most backends create no sidecar at all.
///
/// The allowlisted names are unlinked where they would be, not looked
/// for: `dir` may be empty — the pin's `""` user dir, the working
/// directory (#619) — and an empty path joins to the bare names, where
/// a walk of it has nothing to open.
fn remove_dbm_sidecars(dir: &Path, dbm_path: &Path) {
    let Some(file_name) = dbm_path.file_name().and_then(|s| s.to_str()) else {
        return;
    };
    for suffix in DBM_SIDECAR_SUFFIXES {
        let _ = std::fs::remove_file(dir.join(format!("{file_name}{suffix}")));
    }
}

/// Writes one chunk file to its `.tmp` sibling and registers the rename.
fn stage_chunk(
    dir: &Path,
    name: &FileName,
    bytes: &[u8],
    staged: &mut Staged,
) -> Result<(), PersistenceError> {
    let final_path = name.under(dir);
    let tmp = name.with_suffix(".tmp").under(dir);
    let written = write_chunk_file(&tmp, bytes);
    staged.finish((tmp, final_path), written)
}

/// `MemoryChunk::save` (`memory_chunk.h:536-537`): `open(O_CREAT |
/// O_WRONLY | O_TRUNC, 0644)` over `path`, then the bytes. The process
/// umask applies to 0644, and a file that already exists keeps its own
/// mode, as upstream's does — `std::fs::write` would ask for 0666.
///
/// The header is two `guint32` writes in the pin (`memory_chunk.h:542`
/// and `:546`), each a single `write` whose own return value is `assert`ed;
/// here they are two single writes too ([`write_header_word`]), so a
/// filesystem that refuses the header names the word it refused and a
/// partial word is a failure, not a retry. The payload after them is the
/// pin's soft failure (`MemoryChunk::save` answers `false` and every caller
/// ignores it) and stays an ordinary [`PersistenceError::Io`].
fn write_chunk_file(path: &Path, bytes: &[u8]) -> Result<(), PersistenceError> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o644);
    let mut file = options.open(path).map_err(PersistenceError::Io)?;
    let split = bytes.len().min(CHUNK_HEADER_SIZE);
    let (header, payload) = bytes.split_at(split);
    let (length, checksum) = header.split_at(header.len().min(4));
    write_header_word(&mut file, length, ChunkHeaderField::Length)?;
    write_header_word(&mut file, checksum, ChunkHeaderField::Checksum)?;
    std::io::Write::write_all(&mut file, payload).map_err(PersistenceError::Io)
}

/// One of `MemoryChunk::save`'s two header words (`memory_chunk.h:542-549`).
///
/// The pin makes a single `write` per word and `assert`s that call's own
/// return value is the word's size; a short write is a failure there too.
/// So this is one write, not `write_all`: a retry would finish the word and
/// hide the short write the pin's `assert` would have caught.
fn write_header_word(
    writer: &mut impl std::io::Write,
    word: &[u8],
    field: ChunkHeaderField,
) -> Result<(), PersistenceError> {
    let written = writer
        .write(word)
        .map_err(|source| PersistenceError::ChunkHeaderWrite { field, source })?;
    if written == word.len() {
        return Ok(());
    }
    Err(PersistenceError::ChunkHeaderWrite {
        field,
        source: std::io::Error::new(
            std::io::ErrorKind::WriteZero,
            format!("short write: {written} of {} header bytes", word.len()),
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxpinyin_data::user_files::OPEN_COUNTER_LIMIT;

    fn item(phrase: &[u32], unigram: u32, prons: &[(Vec<u16>, u32)]) -> ChunkItem {
        ChunkItem {
            phrase: phrase.to_vec(),
            unigram,
            prons: prons.to_vec(),
        }
    }

    fn originals() -> SystemOriginals {
        let mut merged = BTreeMap::new();
        merged.insert(
            1,
            item(
                &[0x4f60],
                100,
                &[(vec![ChewingKey::new(23, 0, 1, 0).to_packed()], 100)],
            ),
        );
        merged.insert(
            2,
            item(
                &[0x597d],
                50,
                &[(vec![ChewingKey::new(7, 0, 13, 0).to_packed()], 50)],
            ),
        );
        BTreeMap::from([(
            1_u8,
            SystemLibrary {
                mapped: None,
                total: 150,
                range_end: 3,
                items: merged,
            },
        )])
        .into()
    }

    fn state() -> UserState {
        let bigram = vec![(
            1_u32.to_le_bytes().to_vec(),
            oxpinyin_data::single_gram::encode_single_gram(
                207,
                &[(1, 69), (0x0100_0001, 138), (0x0700_0001, 69)],
            ),
        )];

        let user_item = item(
            &[0x4f60, 0x597d],
            15,
            &[(
                vec![
                    ChewingKey::new(23, 0, 1, 0).to_packed(),
                    ChewingKey::new(7, 0, 13, 0).to_packed(),
                ],
                15,
            )],
        );

        UserState {
            bigram,
            libraries: BTreeMap::from([(7_u8, BTreeMap::from([(1_u32, user_item)]))]),
            // 你 trained twice: 100 → 100 + 69*7
            system_overrides: BTreeMap::from([(
                1_u8,
                BTreeMap::from([(
                    1_u32,
                    Some(item(
                        &[0x4f60],
                        100 + 69 * 7,
                        &[(vec![ChewingKey::new(23, 0, 1, 0).to_packed()], 100 + 69)],
                    )),
                )]),
            )]),
            phrase_table: BTreeMap::new(),
            indexed: std::collections::BTreeSet::new(),
        }
        .index_every_reading()
    }

    fn tempdir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "oxpinyin-user-persist-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("tmpdir");
        dir
    }

    fn versions() -> SystemVersions {
        SystemVersions::for_this_build(7, 14)
    }

    /// A key no remaining row writes is saved with an empty value, as the
    /// pin's tables keep it (#784), and the next load keeps it again.
    #[test]
    fn a_kept_index_key_is_saved_empty_and_loaded_back() {
        let dir = tempdir("kept-index-keys");
        let originals = originals();
        let gone: Vec<u8> = [0x9f96_u32, 0x9f96]
            .iter()
            .flat_map(|c| c.to_le_bytes())
            .collect();
        let gone_reading = vec![0x12, 0x00, 0x12, 0x00];
        let mut kept = IndexKeys::default();
        kept.pinyin.insert(gone_reading.clone());
        let mut saved = state();
        saved.phrase_table.insert(gone.clone(), Vec::new());
        let source = SaveSource {
            state: &saved,
            kept: &kept,
        };
        save_session(&dir, source, &originals, &versions(), 1, None).expect("save");

        let rows = |dbm: UserDbm| {
            let store = DefaultStore::open_user_index(&dir.join(dbm.file_name())).expect("opens");
            let mut rows = BTreeMap::new();
            store
                .range_raw(
                    std::ops::Bound::Unbounded,
                    std::ops::Bound::Unbounded,
                    &mut |k, v| {
                        rows.insert(k.to_vec(), v.to_vec());
                        Ok(())
                    },
                )
                .expect("walk");
            rows
        };
        let phrase = rows(UserDbm::PhraseIndex);
        assert_eq!(phrase.get(&gone), Some(&Vec::new()));
        // The item's own rows are written as before.
        assert_eq!(phrase.len(), 3, "你, 你好 and the kept key");
        assert_eq!(
            rows(UserDbm::PinyinIndex).get(&gone_reading),
            Some(&Vec::new())
        );

        let loaded =
            load_profile(&dir, &originals, &versions(), UserConfLaw::Pinyin).expect("load");
        assert!(loaded.loaded.state.phrase_table.contains_key(&gone));
        assert!(loaded.index_file_keys.pinyin.contains(&gone_reading));
        // The session's profile read leaves the user bigram to its
        // container (`load_user_bigram_db`).
        let saved = UserState {
            bigram: Vec::new(),
            ..saved
        };
        assert_eq!(loaded.loaded.state, saved, "an empty key names no row");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_then_load_round_trips_the_state() {
        let dir = tempdir("round-trip");
        let originals = originals();
        let state = state();

        save(&dir, &state, &originals, &versions(), 1).expect("save");
        // The user dir holds exactly the pin's eleven names — the three
        // DBMs under the backend's own names, the three USER_FILE chunks,
        // the four `.dbin` logs, and `user.conf` — and nothing else. This
        // is the assertion that catches an orphaned backend sidecar (a
        // locking backend writes a `-lock` file beside each DBM; the
        // rename pass moves
        // only the data file), which a literal `.tmp`-suffix check misses.
        let mut names: Vec<String> = std::fs::read_dir(&dir)
            .expect("readdir")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        let mut expected = vec![
            UserDbm::Bigram.file_name(),
            UserDbm::PinyinIndex.file_name(),
            UserDbm::PhraseIndex.file_name(),
            "user.conf".to_owned(),
        ];
        let layout = UserFileLayout::stock();
        expected.extend(
            layout
                .user_libraries()
                .iter()
                .map(|(_, n)| n.display().into_owned()),
        );
        expected.extend(
            layout
                .system_logs()
                .iter()
                .map(|(_, n)| n.display().into_owned()),
        );
        expected.sort();
        assert_eq!(names, expected, "the save left an unexpected file");

        let loaded = load(&dir, &originals, &versions(), UserConfLaw::Pinyin).expect("load");
        assert!(loaded.skipped.is_empty(), "{:?}", loaded.skipped);
        assert!(!loaded.wiped);
        assert_eq!(loaded.open_counter, 2); // check_format's raise
        assert_eq!(loaded.state, state);

        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn diff_records_emits_remove_for_an_explicit_removal() {
        // A `Some(None)` override is a removal, not "fall back to the
        // original item": collapsing the two lookups drops the Remove
        // record (review on #447).
        let originals = originals();
        let original = &originals[&1];
        let overrides: BTreeMap<u32, Option<ChunkItem>> = BTreeMap::from([(1_u32, None)]);

        let records = diff_records(1, original, &overrides).expect("diff");
        assert!(matches!(
            records.as_slice(),
            [
                LogRecord::ModifyHeader {
                    old_total: 150,
                    new_total: 50
                },
                LogRecord::Remove { token: 1, .. }
            ]
        ));
    }

    #[test]
    fn mapped_originals_save_and_replay_exact_logs_without_expanding_items() {
        let dir = tempdir("mapped-originals");
        let mut inline = originals();
        let base = inline.get_mut(&1).expect("fixture library");
        let mut last = base.items.remove(&2).expect("fixture item");
        // A hole, multiple ordered readings and a removed last slot exercise
        // the mapped diff base beyond the single-reading inline fixtures.
        last.prons.push((last.prons[0].0.clone(), 0));
        base.items.insert(3, last);
        base.range_end = 4;
        let pairs: Vec<_> = base
            .items
            .iter()
            .map(|(&slot, item)| (slot, item.clone()))
            .collect();
        let chunk = dir.join("system.bin");
        std::fs::write(&chunk, build_chunk(&pairs).expect("fixture chunk")).expect("write fixture");
        let mut libraries = PhraseLibraries::empty();
        assert!(libraries.load(1, &chunk).expect("map fixture"));
        let mapped = system_originals(&libraries, inline.layout().clone());
        drop(libraries); // Originals must keep their own shared mapping alive.

        assert_eq!(mapped[&1].total, inline[&1].total);
        assert_eq!(mapped[&1].range_end, inline[&1].range_end);
        assert!(mapped[&1].mapped.is_some());
        assert!(mapped[&1].items.is_empty(), "open eagerly decoded items");
        for slot in 0..=4 {
            assert_eq!(
                mapped[&1].contains_item(slot),
                inline[&1].contains_item(slot)
            );
            assert_eq!(mapped[&1].unigram(slot), inline[&1].unigram(slot));
            assert_eq!(mapped[&1].item(slot), inline[&1].item(slot));
        }

        let mut changed = inline[&1].item(1).expect("original item");
        changed.unigram += 69;
        changed.prons[0].1 += 69;
        let mut edited = UserState::default();
        edited.system_overrides.insert(
            1,
            BTreeMap::from([(1, Some(changed.clone())), (2, Some(changed)), (3, None)]),
        );
        for (name, state) in [("empty", UserState::default()), ("edited", edited)] {
            let inline_dir = dir.join(format!("{name}-inline"));
            let mapped_dir = dir.join(format!("{name}-mapped"));
            for profile in [&inline_dir, &mapped_dir] {
                std::fs::create_dir(profile).expect("profile dir");
            }
            save(&inline_dir, &state, &inline, &versions(), 1).expect("inline save");
            save(&mapped_dir, &state, &mapped, &versions(), 1).expect("mapped save");
            for (_, file) in inline.layout().system_logs() {
                assert_eq!(
                    std::fs::read(file.under(&mapped_dir)).expect("mapped log"),
                    std::fs::read(file.under(&inline_dir)).expect("inline log"),
                    "{name}: {}",
                    file.display()
                );
            }
            for profile in [&inline_dir, &mapped_dir] {
                let loaded = load(profile, &mapped, &versions(), UserConfLaw::Pinyin)
                    .expect("mapped replay");
                assert!(loaded.skipped.is_empty(), "{:?}", loaded.skipped);
                assert_eq!(loaded.state, state);
            }
            assert!(
                mapped[&1].items.is_empty(),
                "save/replay expanded originals"
            );
        }
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn removed_system_phrase_stays_removed_across_save_and_reopen() {
        let dir = tempdir("removal-round-trip");
        let originals = originals();
        let mut state = state();
        state
            .system_overrides
            .get_mut(&1)
            .expect("library")
            .insert(2_u32, None);

        save(&dir, &state, &originals, &versions(), 1).expect("save");
        let loaded = load(&dir, &originals, &versions(), UserConfLaw::Pinyin).expect("load");
        assert!(loaded.skipped.is_empty(), "{:?}", loaded.skipped);
        assert_eq!(
            loaded.state.system_overrides[&1].get(&2),
            Some(&None),
            "the removal did not survive the save-then-reopen"
        );

        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn fresh_dir_saves_and_loads_empty_everywhere() {
        let dir = tempdir("fresh");
        let originals = originals();

        // A fresh load: no user.conf → non-conform → empty, counter 1.
        let loaded = load(&dir, &originals, &versions(), UserConfLaw::Pinyin).expect("load");
        assert!(loaded.wiped);
        assert_eq!(loaded.open_counter, 1);
        assert_eq!(loaded.state, UserState::default());

        // Saving the empty state writes the whole inventory, and it
        // loads back empty.
        save(&dir, &UserState::default(), &originals, &versions(), 1).expect("save");
        let reloaded = load(&dir, &originals, &versions(), UserConfLaw::Pinyin).expect("load");
        assert!(!reloaded.wiped);
        assert!(reloaded.skipped.is_empty(), "{:?}", reloaded.skipped);
        assert_eq!(reloaded.state, UserState::default());
        // The header-only diff log: the frame decodes to one header
        // record with old == new totals.
        let log = std::fs::read(dir.join("merged.dbin")).expect("absent lib log");
        let records =
            decode_log_records(read_chunk_payload(&log).expect("frame")).expect("records");
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0],
            LogRecord::ModifyHeader {
                old_total: 0,
                new_total: 0
            }
        );

        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    /// The user dir as a sorted (name, bytes) list: the differential's
    /// "byte-identical to the seeded dir, no wipe" check, at unit scale.
    fn dir_snapshot(dir: &Path) -> Vec<(String, Vec<u8>)> {
        let mut rows: Vec<(String, Vec<u8>)> = std::fs::read_dir(dir)
            .expect("readdir")
            .map(|entry| {
                let entry = entry.expect("entry");
                let name = entry.file_name().to_string_lossy().into_owned();
                (name, std::fs::read(entry.path()).expect("read"))
            })
            .collect();
        rows.sort();
        rows
    }

    /// The class-(c) marker: a `database format:` field upstream's
    /// `to_table_database_format_type` does not know, which `abort()`s the
    /// pin's process (`table_info.cpp:122-133`, reached from `:353-354`).
    /// `load` answers `Err` and leaves the user dir byte-identical — no
    /// conformance judgement, no wipe, no marker write, no counter step.
    #[test]
    fn an_unknown_database_format_refuses_the_load_untouched() {
        let dir = tempdir("marker-abort");
        let originals = originals();
        save(&dir, &state(), &originals, &versions(), 1).expect("save");

        let long = format!(
            "binary format version:7\nmodel data version:14\ndatabase format:{}\nopen counter:5\n",
            "A".repeat(300)
        );
        let trunc = format!(
            "binary format version:7\nmodel data version:14\ndatabase format:{}open counter:5\n",
            "A".repeat(255)
        );
        for body in [
            // A token no upstream build knows.
            "binary format version:7\nmodel data version:14\ndatabase format:NotADbmLibrary\nopen counter:5\n".to_owned(),
            // 300 bytes: `%255s` takes 255 of them.
            long,
            // 255 bytes, counter on the same line.
            trunc,
            // No format directive at all: the literal mismatches and
            // upstream maps the `str` no conversion wrote.
            "binary format version:7\nmodel data version:14\nopen counter:5\n".to_owned(),
            // Junk on the model-data line: the abort is what the *third*
            // directive meets.
            "binary format version:7\nmodel data version:14x\ndatabase format:Tkrzw\nopen counter:5\n".to_owned(),
        ] {
            std::fs::write(dir.join(USER_CONF), &body).expect("write");
            let before = dir_snapshot(&dir);
            for law in [UserConfLaw::Pinyin, UserConfLaw::Zhuyin] {
                assert!(
                    matches!(
                        load(&dir, &originals, &versions(), law),
                        Err(PersistenceError::UnknownDatabaseFormat)
                    ),
                    "{body:?} under {law:?}"
                );
            }
            assert_eq!(dir_snapshot(&dir), before, "the dir moved under {body:?}");
        }

        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    /// A marker the load cannot complete at all — a version directive that
    /// does not convert, in the directives before the format one — is not
    /// the refusal: upstream's `load` returns `false`, `check_format`
    /// judges the reset marker non-conforming, and the profile is wiped
    /// and (pinyin) rewritten, exactly as before.
    #[test]
    fn a_marker_the_load_cannot_complete_still_wipes() {
        let dir = tempdir("marker-incomplete");
        let originals = originals();
        for body in [
            "",
            "binary format version:x\nmodel data version:14\n",
            "model data version:14\nbinary format version:7\ndatabase format:Tkrzw\nopen counter:5\n",
            "binary format version:7x\nmodel data version:14\ndatabase format:Tkrzw\nopen counter:5\n",
        ] {
            save(&dir, &state(), &originals, &versions(), 1).expect("save");
            assert!(dir.join("user.bin").exists());
            std::fs::write(dir.join(USER_CONF), body).expect("write");

            let loaded = load(&dir, &originals, &versions(), UserConfLaw::Pinyin).expect("load");
            assert!(loaded.wiped, "{body:?}");
            assert_eq!(loaded.open_counter, 1, "{body:?}"); // 0 + the pin's raise
            assert_eq!(dir_snapshot(&dir).len(), 1, "{body:?}: only the marker");
            assert!(!dir.join("user.bin").exists(), "{body:?}");
        }

        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn non_conform_profile_wipes_and_resets() {
        let dir = tempdir("wipe");
        let originals = originals();
        save(&dir, &state(), &originals, &versions(), 3).expect("save");
        assert!(dir.join("user.bin").exists());

        // A cross-backend marker never conforms. `BerkeleyDB` was the
        // obvious foreign token while no backend carried it; the bdb
        // build IS BerkeleyDB, so its foreign marker is a different real
        // upstream token (any of the three that is not this build's
        // `DEFAULT_STORE_DB_FORMAT` would do).
        let foreign_token = match oxpinyin_store::DEFAULT_STORE_DB_FORMAT {
            "BerkeleyDB" => "KyotoCabinet",
            _ => "BerkeleyDB",
        };
        let foreign = UserTableInfo {
            binary_format_version: 7,
            model_data_version: 14,
            database_format: Some(foreign_token.to_owned()),
            open_counter: 0,
        };
        std::fs::write(dir.join(USER_CONF), foreign.to_text()).expect("write");

        let loaded = load(&dir, &originals, &versions(), UserConfLaw::Pinyin).expect("load");
        assert!(loaded.wiped);
        assert_eq!(loaded.state, UserState::default());
        assert_eq!(loaded.open_counter, 1); // get_open_counter caps, +1
        assert!(
            !dir.join("user.bin").exists(),
            "profile file survived the wipe"
        );
        assert!(!dir.join("gb_char.dbin").exists());
        // The marker is rewritten conform.
        let marker = UserTableInfo::parse(&std::fs::read(dir.join(USER_CONF)).expect("read"))
            .expect("parse");
        assert!(marker.is_conform(&versions()));

        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn the_tired_counter_wipes_on_the_next_open() {
        let dir = tempdir("tired");
        let originals = originals();
        save(&dir, &state(), &originals, &versions(), OPEN_COUNTER_LIMIT).expect("save");
        // The limit itself still conforms; one more open crosses it.
        let loaded = load(&dir, &originals, &versions(), UserConfLaw::Pinyin).expect("load");
        assert!(!loaded.wiped);
        assert_eq!(loaded.open_counter, OPEN_COUNTER_LIMIT + 1);
        let next = load(&dir, &originals, &versions(), UserConfLaw::Pinyin).expect("load");
        assert!(next.wiped);
        assert_eq!(next.state, UserState::default());

        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn check_format_then_load_checked_is_load() {
        for law in [UserConfLaw::Pinyin, UserConfLaw::Zhuyin] {
            let whole = tempdir("split-whole");
            let split = tempdir("split-parts");
            let originals = originals();
            for dir in [&whole, &split] {
                save(dir, &state(), &originals, &versions(), 1).expect("save");
            }
            let loaded = load(&whole, &originals, &versions(), law).expect("load");
            // The judgement alone already raised the counter and wrote the
            // marker, as it does ahead of the system libraries.
            let check =
                check_format(&split, &versions(), &UserFileLayout::stock(), law).expect("check");
            assert_eq!(recorded_counter(&split), recorded_counter(&whole));
            let parts = load_checked(&split, &originals, &check);
            // `load` alone reads the user bigram's raw rows; the checked
            // profile read is the session's, whose container reads them.
            assert_eq!(loaded.state.bigram, state().bigram);
            let without_bigram = UserState {
                bigram: Vec::new(),
                ..loaded.state.clone()
            };
            assert_eq!(parts.loaded.state, without_bigram);
            assert_eq!(parts.loaded.open_counter, loaded.open_counter);
            assert_eq!(parts.loaded.wiped, loaded.wiped);
            std::fs::remove_dir_all(&whole).expect("cleanup");
            std::fs::remove_dir_all(&split).expect("cleanup");
        }
    }

    /// Records are independent of library items: duplicate tokens, shared
    /// tokens across texts, empty markers, and undecodable tails survive
    /// both load and save byte-for-byte.
    #[test]
    fn phrase_index_records_round_trip_verbatim() {
        use oxpinyin_data::row_format::phrase_index::{encode_tokens, encode_ucs4_key};

        let dir = tempdir("phrase-index-records");
        let originals = originals();
        let mut saved = state();
        let token = 0x0400_0001_u32;
        let mut partial = encode_tokens(&[token, token]);
        partial.extend_from_slice(&[0xa5, 0x5a]);
        saved.phrase_table = BTreeMap::from([
            (encode_ucs4_key("甲"), partial),
            (encode_ucs4_key("乙"), encode_tokens(&[token])),
            (encode_ucs4_key("丙"), Vec::new()),
            (vec![0xff], vec![0x12]),
        ]);
        save(&dir, &saved, &originals, &versions(), 1).expect("save");
        let loaded = load(&dir, &originals, &versions(), UserConfLaw::Pinyin).expect("load");
        assert_eq!(loaded.state.phrase_table, saved.phrase_table);
        save(&dir, &loaded.state, &originals, &versions(), 2).expect("save again");
        let store = DefaultStore::open_user_index(&dir.join(UserDbm::PhraseIndex.file_name()))
            .expect("open index");
        let mut rows = BTreeMap::new();
        store
            .range_raw(
                std::ops::Bound::Unbounded,
                std::ops::Bound::Unbounded,
                &mut |key, value| {
                    rows.insert(key.to_vec(), value.to_vec());
                    Ok(())
                },
            )
            .expect("walk");
        assert_eq!(rows, saved.phrase_table);
        drop(store);
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    /// The counter `user.conf` holds, read back as the next init reads it.
    fn recorded_counter(dir: &Path) -> Option<i32> {
        let bytes = std::fs::read(dir.join(USER_CONF)).ok()?;
        Some(UserTableInfo::parse(&bytes).ok()?.open_counter)
    }

    #[test]
    fn the_lowering_is_pinyin_finis_arithmetic() {
        // `counter > 1 ? counter - 1 : 0` over `get_open_counter`
        // (pinyin.cpp:1196-1197): a raised 7 reads 0, so it lowers to 0.
        assert_eq!(lowered_open_counter(0), 0);
        assert_eq!(lowered_open_counter(1), 0);
        assert_eq!(lowered_open_counter(2), 1);
        assert_eq!(
            lowered_open_counter(OPEN_COUNTER_LIMIT),
            OPEN_COUNTER_LIMIT - 1
        );
        assert_eq!(lowered_open_counter(OPEN_COUNTER_LIMIT + 1), 0);
        assert_eq!(lowered_open_counter(i32::MAX), 0);
        assert_eq!(lowered_open_counter(-2), 0);
        assert_eq!(lowered_open_counter(i32::MIN), 0);
    }

    #[test]
    fn clean_pinyin_cycles_return_the_counter_and_keep_the_profile() {
        // #523: init raises, save writes the raised value back, fini
        // lowers — so a profile that is saved and finished every launch
        // never reaches the limit, however many launches it sees.
        let dir = tempdir("pinyin-cycles");
        let originals = originals();
        for launch in 1..=10 {
            let loaded = load(&dir, &originals, &versions(), UserConfLaw::Pinyin).expect("load");
            assert_eq!(
                loaded.wiped,
                launch == 1,
                "launch {launch}: only the fresh dir wipes"
            );
            assert_eq!(loaded.open_counter, 1, "launch {launch}: raised from 0");
            assert_eq!(
                recorded_counter(&dir),
                Some(1),
                "launch {launch}: the init writes"
            );
            if launch > 1 {
                assert_eq!(
                    loaded.state,
                    state(),
                    "launch {launch}: the profile survived"
                );
            }
            save(&dir, &state(), &originals, &versions(), loaded.open_counter).expect("save");
            assert_eq!(
                recorded_counter(&dir),
                Some(1),
                "launch {launch}: the save writes back"
            );
            fini(&dir, &versions(), UserConfLaw::Pinyin, loaded.open_counter).expect("fini");
            assert_eq!(
                recorded_counter(&dir),
                Some(0),
                "launch {launch}: the fini lowers"
            );
        }
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn a_pinyin_fini_without_a_save_still_lowers_the_counter() {
        // pinyin_fini's mark_version runs whether or not a save came
        // first (pinyin.cpp:1194-1200).
        let dir = tempdir("pinyin-fini-unsaved");
        let originals = originals();
        save(&dir, &state(), &originals, &versions(), 0).expect("seed");
        let before = std::fs::read(dir.join("user.bin")).expect("user.bin");
        let loaded = load(&dir, &originals, &versions(), UserConfLaw::Pinyin).expect("load");
        assert_eq!(recorded_counter(&dir), Some(1));
        fini(&dir, &versions(), UserConfLaw::Pinyin, loaded.open_counter).expect("fini");
        assert_eq!(recorded_counter(&dir), Some(0));
        // user.conf is the only file a fini writes.
        assert_eq!(
            std::fs::read(dir.join("user.bin")).expect("user.bin"),
            before
        );
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn pinyin_launches_that_die_before_fini_wipe_on_the_eighth() {
        // The crash path: every init raises and writes, nothing lowers.
        // From a counter of 0 the seventh killed launch leaves 7, and the
        // eighth init reads it past the limit, wipes and writes 1 — where
        // the pin wipes (table_info.cpp:409-410).
        let dir = tempdir("pinyin-crashes");
        let originals = originals();
        save(&dir, &state(), &originals, &versions(), 0).expect("seed");
        for launch in 1..=7 {
            let loaded = load(&dir, &originals, &versions(), UserConfLaw::Pinyin).expect("load");
            assert!(!loaded.wiped, "killed launch {launch} wiped early");
            assert_eq!(loaded.state, state());
            assert_eq!(recorded_counter(&dir), Some(launch));
        }
        let eighth = load(&dir, &originals, &versions(), UserConfLaw::Pinyin).expect("load");
        assert!(eighth.wiped);
        assert_eq!(eighth.state, UserState::default());
        assert_eq!(recorded_counter(&dir), Some(1));
        assert!(
            !dir.join("user.bin").exists(),
            "profile file survived the wipe"
        );
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn a_raised_seven_that_finishes_restarts_from_zero() {
        // get_open_counter on both ends: an init that raises 6 to 7 and
        // then finishes writes 0 (a 7 reads as 0), not 6.
        let dir = tempdir("pinyin-seven");
        let originals = originals();
        save(&dir, &state(), &originals, &versions(), OPEN_COUNTER_LIMIT).expect("seed");
        let loaded = load(&dir, &originals, &versions(), UserConfLaw::Pinyin).expect("load");
        assert!(!loaded.wiped);
        assert_eq!(recorded_counter(&dir), Some(OPEN_COUNTER_LIMIT + 1));
        fini(&dir, &versions(), UserConfLaw::Pinyin, loaded.open_counter).expect("fini");
        assert_eq!(recorded_counter(&dir), Some(0));
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn zhuyin_init_and_fini_never_write_the_marker() {
        // libzhuyin's check_format only reads user.conf and its fini writes
        // nothing (zhuyin.cpp:126-162, :741-757): a fresh dir has no marker
        // until the first save, which writes a counter of 0 (:164-176).
        let dir = tempdir("zhuyin-marker");
        let originals = originals();
        let fresh = load(&dir, &originals, &versions(), UserConfLaw::Zhuyin).expect("load");
        assert!(fresh.wiped);
        assert_eq!(fresh.open_counter, 0);
        assert!(
            !dir.join(USER_CONF).exists(),
            "a zhuyin init wrote user.conf"
        );
        fini(&dir, &versions(), UserConfLaw::Zhuyin, fresh.open_counter).expect("fini");
        assert!(
            !dir.join(USER_CONF).exists(),
            "a zhuyin fini wrote user.conf"
        );

        save(&dir, &state(), &originals, &versions(), fresh.open_counter).expect("save");
        let marker = std::fs::read(dir.join(USER_CONF)).expect("marker");
        assert_eq!(recorded_counter(&dir), Some(0));
        // Launches that die before fini, and clean ones, leave it alone:
        // no counter moves, so no wipe ever comes of them.
        for launch in 1..=10 {
            let loaded = load(&dir, &originals, &versions(), UserConfLaw::Zhuyin).expect("load");
            assert!(!loaded.wiped, "zhuyin launch {launch} wiped");
            assert_eq!(loaded.state, state());
            fini(&dir, &versions(), UserConfLaw::Zhuyin, loaded.open_counter).expect("fini");
            assert_eq!(std::fs::read(dir.join(USER_CONF)).expect("marker"), marker);
        }
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn a_stray_byte_before_the_counter_line_wipes() {
        // #584 review: invalid bytes amid the identity fields must not
        // be hidden by lossy decoding, even though arbitrary trailing
        // bytes in the counter value are accepted. The stray byte stops
        // the next `fscanf`'s literal, `load` fails on the reset fields
        // (table_info.cpp:318-321, :338-351) and `check_format` wipes
        // (pinyin.cpp:191-215, zhuyin.cpp:138-161). libpinyin's rewrite
        // then records a conform marker with counter 1
        // (`pinyin.cpp:185-187`); libzhuyin wrote nothing and leaves the
        // non-conform marker as it was (`zhuyin.cpp:126-162`).
        for law in [UserConfLaw::Pinyin, UserConfLaw::Zhuyin] {
            let dir = tempdir("stray-byte");
            let originals = originals();
            save(&dir, &state(), &originals, &versions(), 0).expect("seed");
            let mut marker = UserTableInfo::conform_to(&versions())
                .to_text()
                .into_bytes();
            let after_version = marker
                .iter()
                .position(|&byte| byte == b'\n')
                .expect("a line end");
            marker.splice(after_version..after_version, *b"\xFF");
            std::fs::write(dir.join(USER_CONF), &marker).expect("corrupt marker");

            let loaded = load(&dir, &originals, &versions(), law).expect("load");
            assert!(loaded.wiped, "{law:?} kept a profile past a stray byte");
            assert_eq!(loaded.state, UserState::default());
            assert!(
                !dir.join("user.bin").exists(),
                "{law:?}: profile file survived the wipe"
            );
            match law {
                UserConfLaw::Pinyin => assert_eq!(recorded_counter(&dir), Some(1)),
                UserConfLaw::Zhuyin => assert_eq!(
                    std::fs::read(dir.join(USER_CONF)).expect("marker"),
                    marker,
                    "zhuyin rewrote the non-conform marker"
                ),
            }
            std::fs::remove_dir_all(&dir).expect("cleanup");
        }
    }

    #[test]
    fn a_hand_written_counter_reads_as_the_pins_percent_d() {
        // #583: the marker's counter is whatever `fscanf`'s `%d` makes of it
        // (table_info.cpp:356-359), an `int`. Under either law the value
        // decides the wipe (above 6, :409-410); libpinyin's init writes it
        // raised through get_open_counter (pinyin.cpp:185-187) and its fini
        // the lowered value (:1194-1200); libzhuyin writes neither.
        let originals = originals();
        let conform = UserTableInfo::conform_to(&versions()).to_text();
        let head = conform
            .strip_suffix("open counter:0\n")
            .expect("the marker ends with its counter");
        for (line, raised, lowered, wiped) in [
            (&b"open counter:-3\n"[..], -2, 0, false),
            (b"open counter:+3\n", 4, 3, false),
            (b"open counter:+7\n", 1, 0, true),
            (b"open counter:3x\n", 4, 3, false),
            (b"open counter:7x\n", 1, 0, true),
            (b"open counter:3\xff\n", 4, 3, false),
            (b"opencounter:3\xff\n", 4, 3, false),
            (b"open \n counter:3\xff\n", 4, 3, false),
            (b"open counter:\xff\n", 1, 0, false),
            (b"open counter:\x0b5\n", 6, 5, false),
            (b"open counter:\n5\n", 6, 5, false),
            (b"  open counter:5\n", 6, 5, false),
            (b"opencounter:5\n", 6, 5, false),
            (b"open counter:2147483648\n", i32::MIN + 1, 0, false),
            (b"open counter:4294967303\n", 1, 0, true),
            (b"open counter:-2147483649\n", 1, 0, true),
            (b"open counter:99999999999999999999\n", 0, 0, false),
            (b"open counter:\n", 1, 0, false),
            (b"open counter:-\n", 1, 0, false),
            (b"", 1, 0, false),
        ] {
            for law in [UserConfLaw::Pinyin, UserConfLaw::Zhuyin] {
                let case = format!("{law:?} {:?}", String::from_utf8_lossy(line));
                let dir = tempdir("hand-written-counter");
                save(&dir, &state(), &originals, &versions(), 0).expect("seed");
                let mut marker = head.as_bytes().to_vec();
                marker.extend_from_slice(line);
                std::fs::write(dir.join(USER_CONF), &marker).expect("hand-written marker");

                let loaded = load(&dir, &originals, &versions(), law).expect("load");
                assert_eq!(loaded.wiped, wiped, "{case}");
                let expected_state = if wiped { UserState::default() } else { state() };
                assert_eq!(loaded.state, expected_state, "{case}");
                match law {
                    UserConfLaw::Pinyin => {
                        assert_eq!(loaded.open_counter, raised, "{case}");
                        assert_eq!(recorded_counter(&dir), Some(raised), "{case}");
                        fini(&dir, &versions(), law, loaded.open_counter).expect("fini");
                        assert_eq!(recorded_counter(&dir), Some(lowered), "{case}");
                    }
                    UserConfLaw::Zhuyin => {
                        fini(&dir, &versions(), law, loaded.open_counter).expect("fini");
                        assert_eq!(
                            std::fs::read(dir.join(USER_CONF)).expect("marker"),
                            marker,
                            "{case}"
                        );
                    }
                }
                std::fs::remove_dir_all(&dir).expect("cleanup");
            }
        }
    }

    #[test]
    fn a_zhuyin_wipe_keeps_the_non_conform_marker() {
        // A counter past the limit is non-conform under either law; the
        // zhuyin wipe removes the data files and leaves the marker as it
        // was (zhuyin.cpp:141-159 unlinks no user.conf), so every later
        // init wipes again until a save rewrites it.
        let dir = tempdir("zhuyin-tired");
        let originals = originals();
        save(
            &dir,
            &state(),
            &originals,
            &versions(),
            OPEN_COUNTER_LIMIT + 1,
        )
        .expect("seed");
        let marker = std::fs::read(dir.join(USER_CONF)).expect("marker");
        let loaded = load(&dir, &originals, &versions(), UserConfLaw::Zhuyin).expect("load");
        assert!(loaded.wiped);
        assert_eq!(loaded.state, UserState::default());
        assert!(
            !dir.join("user.bin").exists(),
            "profile file survived the wipe"
        );
        assert_eq!(std::fs::read(dir.join(USER_CONF)).expect("marker"), marker);
        assert!(
            load(&dir, &originals, &versions(), UserConfLaw::Zhuyin)
                .expect("load")
                .wiped
        );
        save(
            &dir,
            &UserState::default(),
            &originals,
            &versions(),
            loaded.open_counter,
        )
        .expect("save");
        assert_eq!(recorded_counter(&dir), Some(0));
        assert!(
            !load(&dir, &originals, &versions(), UserConfLaw::Zhuyin)
                .expect("load")
                .wiped
        );
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    /// `memory_chunk.h:543`/`:547`: the pin `assert`s on a chunk header
    /// write the filesystem refuses, and `/dev/full` refuses every write.
    /// The reporting save is the one place the pin's "carry on" stops, so
    /// the failure has to come back typed, naming the header word, for the
    /// facade to turn into its class-(c) warning.
    #[test]
    #[cfg(target_os = "linux")]
    fn reporting_save_stops_at_a_chunk_header_write() {
        let dir = tempdir("chunk-header");
        let originals = originals();
        std::os::unix::fs::symlink("/dev/full", dir.join("user.bin.tmp")).expect("symlink");
        let state = state();
        let source = SaveSource {
            state: &state,
            kept: &IndexKeys::default(),
        };
        let error =
            save_with_bigram_reporting(&dir, source, &originals, &versions(), 0, None, false)
                .expect_err("the header write must fail");
        match error {
            PersistenceError::ChunkHeaderWrite {
                field: ChunkHeaderField::Length,
                ..
            } => {}
            other => panic!("expected the length write to fail: {other:?}"),
        }
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    /// `memory_chunk.h:543`/`:547`: the pin reads the `write` call's own
    /// return value, so a *short* header write — a partial word the
    /// filesystem accepted without error — must fail here too, where
    /// `write_all` would retry the tail and report success. A real short
    /// write to a regular file is not portable to trigger, so a writer that
    /// returns a short count from its first `write` stands in for it.
    #[test]
    fn a_short_header_write_is_not_retried() {
        struct ShortWriter {
            count: usize,
        }
        impl std::io::Write for ShortWriter {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                Ok(self.count.min(buf.len()))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let mut short = ShortWriter { count: 2 };
        let error = write_header_word(&mut short, &[0_u8; 4], ChunkHeaderField::Length)
            .expect_err("a short header write must fail");
        match error {
            PersistenceError::ChunkHeaderWrite { field, source } => {
                assert_eq!(field, ChunkHeaderField::Length);
                assert_eq!(source.kind(), std::io::ErrorKind::WriteZero);
            }
            other => panic!("expected the length write to fail: {other:?}"),
        }

        let mut whole = ShortWriter { count: 4 };
        write_header_word(&mut whole, &[0_u8; 4], ChunkHeaderField::Checksum)
            .expect("a whole header write must succeed");
    }

    /// The process umask, read without `unsafe`: a file created asking
    /// for 0777 keeps exactly the bits the umask lets through.
    #[cfg(unix)]
    fn current_umask(dir: &Path) -> u32 {
        use std::os::unix::fs::OpenOptionsExt;
        let probe = dir.join("umask-probe");
        let _ = std::fs::remove_file(&probe);
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o777)
            .open(&probe)
            .expect("probe");
        let granted = mode_of(&probe);
        std::fs::remove_file(&probe).expect("probe");
        !granted & 0o777
    }

    #[cfg(unix)]
    fn mode_of(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
            .permissions()
            .mode()
            & 0o777
    }

    #[test]
    #[cfg(unix)]
    fn user_files_take_the_pins_creation_modes() {
        // #544: each file's mode is what upstream asks for, less the
        // umask — MemoryChunk::save's 0644 for the chunks
        // (memory_chunk.h:536-537), the backend's own save_db request for
        // the three DBMs (Berkeley DB 0600, Kyoto Cabinet's ofstream 0666,
        // tkrzw's 0644), fopen's 0666 for user.conf — and user.conf is
        // written in place, so a mode the user gave it survives a save
        // while the renamed files come back fresh.
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir("modes");
        let originals = originals();
        let umask = current_umask(&dir);
        save(&dir, &state(), &originals, &versions(), 0).expect("save");

        let dbm_request = match oxpinyin_store::DEFAULT_STORE_DB_FORMAT {
            "BerkeleyDB" => 0o600,
            "KyotoCabinet" => 0o666,
            _ => 0o644,
        };
        let mut expected: Vec<(String, u32)> = vec![(USER_CONF.to_owned(), 0o666)];
        for dbm in [UserDbm::Bigram, UserDbm::PinyinIndex, UserDbm::PhraseIndex] {
            expected.push((dbm.file_name(), dbm_request));
        }
        let layout = UserFileLayout::stock();
        for (_, name) in layout.user_libraries().iter().chain(layout.system_logs()) {
            expected.push((name.display().into_owned(), 0o644));
        }
        for (name, request) in &expected {
            assert_eq!(mode_of(&dir.join(name)), request & !umask, "{name}");
        }

        for name in [USER_CONF, "user.bin"] {
            std::fs::set_permissions(dir.join(name), std::fs::Permissions::from_mode(0o640))
                .expect("chmod");
        }
        save(&dir, &state(), &originals, &versions(), 0).expect("save again");
        assert_eq!(
            mode_of(&dir.join(USER_CONF)),
            0o640,
            "user.conf is rewritten in place"
        );
        assert_eq!(
            mode_of(&dir.join("user.bin")),
            0o644 & !umask,
            "user.bin is replaced"
        );

        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn the_diff_log_is_stable_across_save_cycles() {
        let dir = tempdir("stable");
        let originals = originals();
        save(&dir, &state(), &originals, &versions(), 1).expect("save");
        let first = std::fs::read(dir.join("gb_char.dbin")).expect("log");

        // Load back and save again: the loaded state reproduces the same
        // bytes — the replay recomputes the same overrides the diff
        // emitted.
        let loaded = load(&dir, &originals, &versions(), UserConfLaw::Pinyin).expect("load");
        save(&dir, &loaded.state, &originals, &versions(), 2).expect("save 2");
        let second = std::fs::read(dir.join("gb_char.dbin")).expect("log 2");
        assert_eq!(first, second);

        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    /// A system library with one mapped item per probe slot and one inline
    /// item, so every in-place comparison runs against the mapping and the
    /// inline map alike.
    fn library_for_equivalence(name: &str) -> SystemLibrary {
        let key = |n: u16| vec![n, n + 1];
        let items = vec![
            (1, item(&[0x4f60, 0x597d], 9, &[(key(1), 5), (key(3), 7)])),
            (2, item(&[0x4f60], 4, &[])),
            (3, item(&[0xd800], 4, &[(vec![1], 2)])),
            (4, item(&[0x4f60, 0x597d], 9, &[(key(1), 5), (key(1), 8)])),
        ];
        let chunk = oxpinyin_data::chunk_write::build_chunk(&items).expect("chunk");
        let dir = tempdir(name);
        let path = dir.join("equiv.bin");
        std::fs::write(&path, chunk).expect("write");
        let mapped = oxpinyin_data::phrase_library::PhraseLibrary::open(&path).expect("open");
        std::fs::remove_dir_all(&dir).expect("cleanup");
        SystemLibrary {
            total: 0,
            range_end: 0,
            items: BTreeMap::from([(9, item(&[0x1234], 3, &[(vec![7], 11), (vec![7], 12)]))]),
            mapped: Some(mapped),
        }
    }

    #[test]
    fn in_place_item_reads_agree_with_the_materialized_item() {
        let library = library_for_equivalence("equivalence");
        let probes = [
            item(&[0x4f60, 0x597d], 9, &[(vec![1, 2], 5), (vec![3, 4], 7)]),
            item(&[0x4f60, 0x597d], 10, &[(vec![1, 2], 5), (vec![3, 4], 7)]),
            item(&[0x4f60, 0x597e], 9, &[(vec![1, 2], 5), (vec![3, 4], 7)]),
            item(&[0x4f60], 9, &[(vec![1, 2], 5), (vec![3, 4], 7)]),
            item(&[0x4f60, 0x597d], 9, &[(vec![1, 2], 6), (vec![3, 4], 7)]),
            item(&[0x4f60, 0x597d], 9, &[(vec![1, 2], 5)]),
            item(
                &[0x4f60, 0x597d],
                9,
                &[(vec![1, 2], 5), (vec![3, 4], 7), (vec![5, 6], 1)],
            ),
            item(&[0x4f60, 0x597d], 9, &[(vec![3, 4], 7), (vec![1, 2], 5)]),
            item(&[0x4f60], 4, &[]),
            item(&[0xd800], 4, &[(vec![1], 2)]),
            item(&[0x1234], 3, &[(vec![7], 11), (vec![7], 12)]),
            item(&[0x1234], 3, &[(vec![7], 12), (vec![7], 11)]),
            item(&[], 0, &[]),
        ];
        let key_probes: [&[u16]; 6] = [&[1, 2], &[3, 4], &[5, 6], &[7], &[], &[1]];
        // Present, inline, unreadable text, and absent slots alike.
        for slot in [0_u32, 1, 2, 3, 4, 5, 9, 1000] {
            let materialized = library.item(slot);
            assert_eq!(
                library.is_readable(slot),
                materialized.is_some(),
                "slot {slot}"
            );
            assert_eq!(
                library.unigram(slot).filter(|_| materialized.is_some()),
                materialized.as_ref().map(|current| current.unigram),
                "slot {slot}"
            );
            for probe in &probes {
                // Bytes after the last pronunciation change nothing.
                for tail in [&[][..], &[0xa5, 0x5a]] {
                    let bytes = [raw_wire(probe), tail.to_vec()].concat();
                    let wire = WireItem::parse(&bytes).expect("a well-formed probe");
                    assert_eq!(
                        library.item_matches(slot, &wire),
                        materialized.as_ref().map(|current| current == probe),
                        "slot {slot} against {probe:?}"
                    );
                }
            }
            for keys in key_probes {
                let expected = materialized.as_ref().map_or(0, |current| {
                    current
                        .prons
                        .iter()
                        .find(|(base, _)| base.as_slice() == keys)
                        .map_or(0, |(_, frequency)| *frequency)
                });
                assert_eq!(
                    library.pronunciation_frequency(slot, keys),
                    expected,
                    "slot {slot} keys {keys:?}"
                );
            }
        }
    }

    /// The replay reads the originals in place when they are mapped; it must
    /// land on exactly what it lands on with the same items held inline, for
    /// every record kind and every way a record can disagree with the original.
    #[test]
    fn replay_over_mapped_originals_matches_replay_over_inline_ones() {
        let key = |n: u16| vec![n];
        let items = vec![
            (1_u32, item(&[0x4f60], 100, &[(key(1), 100)])),
            (2, item(&[0x597d], 50, &[(key(2), 30), (key(3), 20)])),
            (3, item(&[0x4e16], 7, &[(key(4), 7)])),
            (4, item(&[0x754c], 9, &[(key(5), 9)])),
        ];
        let total = 100 + 50 + 7 + 9;
        let chunk = build_chunk(&items).expect("chunk");
        let dir = tempdir("mapped-vs-inline");
        let path = dir.join("lib.bin");
        std::fs::write(&path, chunk).expect("write");
        let mapped_library =
            oxpinyin_data::phrase_library::PhraseLibrary::open(&path).expect("open");
        let originals_of = |inline: bool| -> SystemOriginals {
            let library = SystemLibrary {
                total,
                range_end: 5,
                items: if inline {
                    items.iter().cloned().collect()
                } else {
                    BTreeMap::new()
                },
                mapped: (!inline).then(|| mapped_library.clone()),
            };
            BTreeMap::from([(1_u8, library)]).into()
        };
        let grown = |base: &ChunkItem, extra: u32| {
            let mut next = base.clone();
            next.prons[0].1 += extra;
            next.unigram += extra;
            next
        };
        let longer = {
            let mut next = items[1].1.clone();
            next.prons.push((key(9), 4));
            next
        };
        let wire = |item: &ChunkItem| encode_phrase_item(item).expect("wire");
        let scenarios: Vec<Vec<LogRecord>> = vec![
            // An in-place modify, a growing modify, a matching remove.
            vec![
                LogRecord::ModifyHeader {
                    old_total: total,
                    new_total: total,
                },
                LogRecord::Modify {
                    token: 1,
                    old_item: wire(&items[0].1),
                    new_item: wire(&grown(&items[0].1, 5)),
                },
                LogRecord::Modify {
                    token: 2,
                    old_item: wire(&items[1].1),
                    new_item: wire(&longer),
                },
                LogRecord::Remove {
                    token: 3,
                    old_item: wire(&items[2].1),
                },
            ],
            // A record over a slot an earlier record already changed.
            vec![
                LogRecord::Modify {
                    token: 1,
                    old_item: wire(&items[0].1),
                    new_item: wire(&grown(&items[0].1, 5)),
                },
                LogRecord::Modify {
                    token: 1,
                    old_item: wire(&grown(&items[0].1, 5)),
                    new_item: wire(&grown(&items[0].1, 9)),
                },
                LogRecord::Remove {
                    token: 1,
                    old_item: wire(&grown(&items[0].1, 9)),
                },
                LogRecord::Remove {
                    token: 1,
                    old_item: wire(&grown(&items[0].1, 9)),
                },
            ],
            // Each way a record disagrees with the original stops the replay.
            vec![LogRecord::Modify {
                token: 4,
                old_item: wire(&grown(&items[3].1, 1)),
                new_item: wire(&items[3].1),
            }],
            vec![LogRecord::Remove {
                token: 4,
                old_item: wire(&grown(&items[3].1, 1)),
            }],
            vec![LogRecord::Remove {
                token: 4,
                old_item: wire(&item(&[0x754c, 0x754c], 9, &[(vec![5, 5], 9)])),
            }],
            // A slot with no original item.
            vec![LogRecord::Remove {
                token: 40,
                old_item: wire(&items[3].1),
            }],
            vec![LogRecord::Modify {
                token: 40,
                old_item: wire(&items[3].1),
                new_item: wire(&items[3].1),
            }],
            // A payload that does not decode.
            vec![LogRecord::Modify {
                token: 4,
                old_item: vec![1],
                new_item: wire(&items[3].1),
            }],
            // A header that misses the running total.
            vec![LogRecord::ModifyHeader {
                old_total: total + 1,
                new_total: 0,
            }],
            // An add, then a modify of the added item.
            vec![
                LogRecord::Add {
                    token: 7,
                    new_item: wire(&item(&[0x1234], 3, &[(key(8), 3)])),
                },
                LogRecord::Modify {
                    token: 7,
                    old_item: wire(&item(&[0x1234], 3, &[(key(8), 3)])),
                    new_item: wire(&item(&[0x1234], 4, &[(key(8), 4)])),
                },
            ],
        ];
        for (index, records) in scenarios.iter().enumerate() {
            let outcome = |inline: bool| {
                let run_dir = tempdir(&format!("mapped-vs-inline-{index}-{inline}"));
                std::fs::write(
                    run_dir.join("gb_char.dbin"),
                    build_memory_chunk(&encode_log_records(records).expect("log")).expect("chunk"),
                )
                .expect("write log");
                std::fs::write(
                    run_dir.join("user.conf"),
                    UserTableInfo::conform_to(&versions()).to_text(),
                )
                .expect("marker");
                let loaded = load(
                    &run_dir,
                    &originals_of(inline),
                    &versions(),
                    UserConfLaw::Pinyin,
                )
                .expect("load");
                std::fs::remove_dir_all(run_dir).expect("cleanup");
                (
                    loaded.state.system_overrides.clone(),
                    loaded.library_totals.clone(),
                    loaded.skipped.clone(),
                )
            };
            assert_eq!(outcome(false), outcome(true), "scenario {index}");
        }
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    /// `ChunkItem`'s wire form without `encode_phrase_item`'s shape refusals,
    /// so a test can write the payloads a hostile journal carries: a key run
    /// of the wrong length, an empty phrase.
    fn raw_wire(item: &ChunkItem) -> Vec<u8> {
        let mut bytes = vec![item.phrase.len() as u8, item.prons.len() as u8];
        bytes.extend_from_slice(&item.unigram.to_le_bytes());
        for code in &item.phrase {
            bytes.extend_from_slice(&code.to_le_bytes());
        }
        for (keys, frequency) in &item.prons {
            for key in keys {
                bytes.extend_from_slice(&key.to_le_bytes());
            }
            bytes.extend_from_slice(&frequency.to_le_bytes());
        }
        bytes
    }

    /// Items that differ from `item` in one way each: every field the
    /// comparison reads, the pronunciation count and order, and the shape.
    fn one_edit_apart(item: &ChunkItem) -> Vec<ChunkItem> {
        let mut out = Vec::new();
        let mut edit = |change: &dyn Fn(&mut ChunkItem)| {
            let mut next = item.clone();
            change(&mut next);
            out.push(next);
        };
        edit(&|next| next.unigram += 1);
        edit(&|next| next.phrase.push(0x4e00));
        edit(&|next| next.prons.push((vec![9; next.phrase.len()], 3)));
        edit(&|next| {
            next.prons.pop();
        });
        edit(&|next| next.prons.reverse());
        if let Some(first) = item.phrase.first() {
            let first = *first;
            edit(&|next| next.phrase[0] = first ^ 1);
            edit(&|next| next.phrase.clear());
        }
        if !item.prons.is_empty() {
            edit(&|next| next.prons[0].1 += 1);
            edit(&|next| next.prons[0].0.push(1));
            edit(&|next| next.prons[0].0.clear());
            edit(&|next| {
                if let Some(key) = next.prons[0].0.first_mut() {
                    *key ^= 1;
                }
            });
        }
        out
    }

    /// `WireItem` is the old route's decode: it accepts the payloads
    /// `decode_phrase_item` accepts and no others, reads the same unigram and
    /// stored length, and says "equal" for the same items.
    #[test]
    fn wire_item_agrees_with_decode_phrase_item() {
        let key = |n: u16| vec![n, n + 1];
        let seeds = [
            item(&[], 3, &[]),
            item(&[], 3, &[(vec![], 9)]),
            item(&[0x4f60], 1, &[]),
            item(&[0x4f60], 1, &[(vec![1], 2)]),
            item(&[0xd800], 6, &[(vec![1], 2)]),
            item(&[0x4f60, 0x597d], 9, &[(key(1), 5), (key(3), 7)]),
            item(&[0x4f60, 0x597d], 9, &[(key(1), 5), (key(1), 5)]),
            item(&[7; 255], 1, &[(vec![1; 255], 2)]),
        ];
        let mut payloads: Vec<Vec<u8>> = Vec::new();
        for seed in &seeds {
            let bytes = raw_wire(seed);
            // Every truncation, with the header intact.
            for end in 0..=bytes.len() {
                payloads.push(bytes[..end].to_vec());
            }
            // Bytes after the last pronunciation.
            for tail in 1..=5_u8 {
                payloads.push([bytes.clone(), vec![0xa5; usize::from(tail)]].concat());
            }
            // A header that disagrees with the body, in either direction.
            for value in 0..=255_u8 {
                for field in [0, 1] {
                    let mut lie = bytes.clone();
                    if let Some(slot) = lie.get_mut(field) {
                        *slot = value;
                    }
                    payloads.push(lie);
                }
            }
            // A key run one key short or long, as `encode_phrase_item` refuses.
            for edited in one_edit_apart(seed) {
                payloads.push(raw_wire(&edited));
            }
        }
        let (mut accepted, mut refused) = (0, 0);
        for bytes in &payloads {
            let decoded = decode_phrase_item(bytes);
            let parsed = WireItem::parse(bytes);
            assert_eq!(parsed.is_some(), decoded.is_ok(), "{bytes:?}");
            let (Ok(decoded), Some(parsed)) = (decoded, parsed) else {
                refused += 1;
                continue;
            };
            accepted += 1;
            assert_eq!(parsed.unigram(), decoded.unigram, "{bytes:?}");
            assert_eq!(
                parsed.wire_len(),
                encode_phrase_item(&decoded)
                    .ok()
                    .map(|encoded| encoded.len()),
                "{decoded:?}"
            );
            assert!(parsed.equals(&decoded), "{decoded:?}");
            for other in one_edit_apart(&decoded) {
                assert_eq!(
                    parsed.equals(&other),
                    decoded == other,
                    "{decoded:?} {other:?}"
                );
            }
        }
        assert!(accepted > 300 && refused > 1000, "{accepted} {refused}");
    }

    /// The replay as it was before `WireItem`: every record payload decoded
    /// to an owned item, and the held item materialised to compare with it.
    /// Returns what the replay leaves behind: overrides, total and diagnostics.
    fn replay_with_owned_items(
        original: &SystemLibrary,
        name: &str,
        records: &[LogRecord],
    ) -> (BTreeMap<u32, Option<ChunkItem>>, Option<u32>, Vec<String>) {
        let mut overrides: BTreeMap<u32, Option<ChunkItem>> = BTreeMap::new();
        let mut skipped = Vec::new();
        let mut total = Some(original.total);
        let held =
            |overrides: &BTreeMap<u32, Option<ChunkItem>>, slot: u32| match overrides.get(&slot) {
                Some(item) => item.clone(),
                None => original.item(slot),
            };
        let wire_len = |item: &ChunkItem| encode_phrase_item(item).ok().map(|bytes| bytes.len());
        for record in records {
            match record {
                LogRecord::Add { token, new_item } => match decode_phrase_item(new_item) {
                    Ok(item) => {
                        total = total.map(|total| total.wrapping_add(item.unigram));
                        overrides.insert(token & PHRASE_MASK, Some(item));
                    }
                    Err(error) => {
                        skipped.push(format!("{name}: {error}"));
                        break;
                    }
                },
                LogRecord::Remove { token, old_item } => {
                    let slot = token & PHRASE_MASK;
                    match decode_phrase_item(old_item) {
                        Ok(old) => match held(&overrides, slot) {
                            Some(current) if current == old => {
                                total = total.map(|total| total.wrapping_sub(old.unigram));
                                overrides.insert(slot, None);
                            }
                            None => {
                                overrides.insert(slot, None);
                            }
                            Some(_) => {
                                skipped.push(format!(
                                    "{name}: replay stopped at a mismatched REMOVE for {token:#010x}"
                                ));
                                break;
                            }
                        },
                        Err(_) => {
                            skipped.push(format!(
                                "{name}: replay stopped at a mismatched REMOVE for {token:#010x}"
                            ));
                            break;
                        }
                    }
                }
                LogRecord::Modify {
                    token,
                    old_item,
                    new_item,
                } => {
                    let slot = token & PHRASE_MASK;
                    match (decode_phrase_item(old_item), decode_phrase_item(new_item)) {
                        (Ok(old), Ok(new)) if held(&overrides, slot) == Some(old.clone()) => {
                            if wire_len(&old).is_some_and(|stored| new_item.len() > stored) {
                                total = total.map(|total| {
                                    total.wrapping_sub(old.unigram).wrapping_add(new.unigram)
                                });
                            }
                            overrides.insert(slot, Some(new));
                        }
                        _ => {
                            skipped.push(format!(
                                "{name}: replay stopped at a mismatched MODIFY for {token:#010x}"
                            ));
                            break;
                        }
                    }
                }
                LogRecord::ModifyHeader {
                    old_total,
                    new_total,
                } => {
                    let now = total.unwrap_or_else(|| system_new_total(original, &overrides));
                    if now != *old_total {
                        skipped.push(format!("{name}: replay stopped at a mismatched header"));
                        break;
                    }
                    total = total.map(|_| *new_total);
                }
            }
        }
        (overrides, total, skipped)
    }

    /// Replays journals whose old payloads are right, wrong, truncated,
    /// padded and malformed, over mapped and inline originals, and requires
    /// what the pre-`WireItem` replay leaves behind: overrides, total and the
    /// diagnostic naming the record the replay stopped at.
    #[test]
    fn replay_agrees_with_the_owned_decoding_replay() {
        let key = |n: u16| vec![n, n + 1];
        let items = [
            (1_u32, item(&[0x4f60], 100, &[(vec![1], 100)])),
            (2, item(&[0x597d], 50, &[(vec![2], 30), (vec![3], 20)])),
            (3, item(&[0x4f60, 0x597d], 7, &[(key(4), 7), (key(4), 7)])),
            (4, item(&[0x754c], 9, &[])),
            (5, item(&[0xd800], 4, &[(vec![1], 2)])),
        ];
        let chunk = build_chunk(&items).expect("chunk");
        let dir = tempdir("replay-owned-oracle");
        let path = dir.join("lib.bin");
        std::fs::write(&path, chunk).expect("write");
        let mapped = oxpinyin_data::phrase_library::PhraseLibrary::open(&path).expect("open");
        std::fs::remove_dir_all(&dir).expect("cleanup");
        let total = items.iter().map(|(_, item)| item.unigram).sum::<u32>();
        // Inline originals add what a mapping cannot hold: an empty phrase.
        let inline_items: BTreeMap<u32, ChunkItem> = items
            .iter()
            .cloned()
            .chain([(6, item(&[], 5, &[(vec![], 3)]))])
            .collect();
        let originals_of = |inline: bool| -> SystemOriginals {
            BTreeMap::from([(
                1_u8,
                SystemLibrary {
                    total,
                    range_end: 7,
                    items: if inline {
                        inline_items.clone()
                    } else {
                        BTreeMap::new()
                    },
                    mapped: (!inline).then(|| mapped.clone()),
                },
            )])
            .into()
        };
        let name = originals_of(true)
            .layout()
            .system_logs()
            .iter()
            .find(|(nibble, _)| *nibble == 1)
            .map(|(_, file)| file.display().to_string())
            .expect("nibble 1 has a system log");

        let mut corpus: Vec<Vec<LogRecord>> = Vec::new();
        let slots: Vec<u32> = inline_items.keys().copied().chain([40]).collect();
        for &slot in &slots {
            // The item a record about `slot` claims, whether or not it is held.
            let claimed = inline_items.get(&slot).unwrap_or(&inline_items[&1]).clone();
            let exact = raw_wire(&claimed);
            let mut olds = vec![
                exact.clone(),
                Vec::new(),
                [exact.clone(), vec![9, 9]].concat(),
            ];
            olds.extend((0..exact.len()).map(|end| exact[..end].to_vec()));
            olds.extend(one_edit_apart(&claimed).iter().map(raw_wire));
            for value in [0_u8, 1, 2, 255] {
                for field in [0, 1] {
                    let mut lie = exact.clone();
                    if let Some(byte) = lie.get_mut(field) {
                        *byte = value;
                    }
                    olds.push(lie);
                }
            }
            let mut grown = claimed.clone();
            grown.unigram += 5;
            grown.prons.push((vec![8; grown.phrase.len()], 1));
            let mut padded = claimed.clone();
            padded.unigram += 3;
            let news = [
                raw_wire(&grown),
                raw_wire(&padded),
                [raw_wire(&padded), vec![1, 2, 3]].concat(),
                raw_wire(&claimed),
                raw_wire(&item(&[0x1234], 1, &[]))[..4].to_vec(),
                Vec::new(),
            ];
            for old in &olds {
                corpus.push(vec![LogRecord::Remove {
                    token: slot,
                    old_item: old.clone(),
                }]);
                // The record over a slot an earlier record already changed.
                for earlier in [
                    LogRecord::Remove {
                        token: slot,
                        old_item: exact.clone(),
                    },
                    LogRecord::Modify {
                        token: slot,
                        old_item: exact.clone(),
                        new_item: news[0].clone(),
                    },
                    LogRecord::Add {
                        token: slot,
                        new_item: news[1].clone(),
                    },
                ] {
                    corpus.push(vec![
                        earlier.clone(),
                        LogRecord::Remove {
                            token: slot,
                            old_item: old.clone(),
                        },
                    ]);
                    corpus.push(vec![
                        earlier,
                        LogRecord::Modify {
                            token: slot,
                            old_item: old.clone(),
                            new_item: news[0].clone(),
                        },
                    ]);
                }
                for new in &news {
                    corpus.push(vec![
                        LogRecord::ModifyHeader {
                            old_total: total,
                            new_total: total + 1,
                        },
                        LogRecord::Modify {
                            token: slot | 0x0100_0000,
                            old_item: old.clone(),
                            new_item: new.clone(),
                        },
                        LogRecord::ModifyHeader {
                            old_total: total + 1,
                            new_total: 7,
                        },
                    ]);
                }
            }
        }

        let mut stopped = 0;
        let mut kept = 0;
        for inline in [false, true] {
            let originals = originals_of(inline);
            for (index, records) in corpus.iter().enumerate() {
                let mut loaded = Loaded::default();
                load_logs_with_mask(
                    SystemLogSource::Snapshot(SystemLogSnapshot {
                        records: BTreeMap::from([(1, records.clone())]),
                    }),
                    &originals,
                    &mut loaded,
                    None,
                    false,
                );
                let (overrides, expected_total, skipped) =
                    replay_with_owned_items(&originals[&1], &name, records);
                let context = format!("inline {inline}, journal {index}: {records:?}");
                assert_eq!(loaded.skipped, skipped, "{context}");
                assert_eq!(
                    loaded.library_totals.get(&1).copied(),
                    expected_total,
                    "{context}"
                );
                assert_eq!(
                    loaded
                        .state
                        .system_overrides
                        .get(&1)
                        .cloned()
                        .unwrap_or_default(),
                    overrides,
                    "{context}"
                );
                stopped += usize::from(!skipped.is_empty());
                kept += usize::from(skipped.is_empty() && !overrides.is_empty());
            }
        }
        // The corpus exercises both outcomes, not one of them thousands of times.
        assert!(stopped > 1000 && kept > 100, "{stopped} {kept}");
    }

    #[test]
    fn replay_masks_high_token_bits_like_the_subindex() {
        let dir = tempdir("local-slot-mask");
        let originals = originals();
        let old = originals[&1].item(1).expect("original");
        let mut new = old.clone();
        new.prons[0].1 += 7;
        let records = vec![
            LogRecord::ModifyHeader {
                old_total: 150,
                new_total: 150,
            },
            LogRecord::Modify {
                token: 0xab00_0001,
                old_item: encode_phrase_item(&old).expect("old payload"),
                new_item: encode_phrase_item(&new).expect("new payload"),
            },
        ];
        let payload = encode_log_records(&records).expect("log");
        std::fs::write(
            dir.join("gb_char.dbin"),
            build_memory_chunk(&payload).expect("chunk"),
        )
        .expect("write");
        std::fs::write(
            dir.join("user.conf"),
            UserTableInfo::conform_to(&versions()).to_text(),
        )
        .expect("marker");
        let loaded = load(&dir, &originals, &versions(), UserConfLaw::Pinyin).expect("load");
        assert!(loaded.skipped.is_empty());
        assert_eq!(loaded.state.system_overrides[&1][&1], Some(new));
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn replay_stops_at_a_mismatched_record() {
        let dir = tempdir("mismatch");
        let originals = originals();

        // A hand-built log: a MODIFY whose old payload is not the
        // original item. The replay stops at it, keeping the prefix.
        let records = vec![
            LogRecord::ModifyHeader {
                old_total: 150,
                new_total: 150 + 69,
            },
            LogRecord::Modify {
                token: 2,
                // A decodable-but-wrong old item (a real pron run; the
                // encoder rejects empty runs now).
                old_item: encode_phrase_item(&item(&[0x597d], 999, &[(vec![0x5678], 999)]))
                    .unwrap_or_else(|e| panic!("{e}")), // wrong old
                new_item: encode_phrase_item(&item(&[0x597d], 1, &[(vec![0x5679], 1)]))
                    .unwrap_or_else(|e| panic!("{e}")),
            },
            LogRecord::Modify {
                token: 1,
                old_item: encode_phrase_item(&item(&[0x4f60], 100, &[(vec![0x1234], 100)]))
                    .unwrap_or_else(|e| panic!("{e}")),
                new_item: encode_phrase_item(&item(&[0x4f60], 200, &[(vec![0x1235], 200)]))
                    .unwrap_or_else(|e| panic!("{e}")),
            },
        ];
        let framed = build_memory_chunk(&encode_log_records(&records).expect("records encode"));
        std::fs::write(dir.join("gb_char.dbin"), framed.expect("frame")).expect("write");
        let marker = UserTableInfo::conform_to(&versions());
        std::fs::write(dir.join(USER_CONF), marker.to_text()).expect("write");

        let loaded = load(&dir, &originals, &versions(), UserConfLaw::Pinyin).expect("load");
        assert!(!loaded.wiped);
        // Slot 1 was never reached: the mismatch stopped the replay
        // before the second MODIFY (an absent map and an empty one are
        // the same state).
        assert!(
            loaded
                .state
                .system_overrides
                .get(&1)
                .is_none_or(std::collections::BTreeMap::is_empty)
        );

        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn the_bigram_round_trips_through_the_user_bigram_container() {
        let dir = tempdir("bigram");
        let originals = originals();
        save(&dir, &state(), &originals, &versions(), 1).expect("save");

        // Point-read one gram through the *user-bigram* seam, which is
        // where the container differs by backend: a Kyoto Cabinet
        // snapshot stream of an in-memory stash, a tkrzw hash file, the
        // hash file. Opening it as a plain hash file
        // is the bug this assertion exists to catch — on Kyoto Cabinet
        // the user bigram is not a HashDB and a hash open answers
        // "missing magic data of the file".
        let path = dir.join(UserDbm::Bigram.file_name());
        let store = DefaultStore::open_user_bigram(&path).expect("open");
        let value = store.get_raw(&1_u32.to_le_bytes()).expect("get");
        let (total, records) =
            oxpinyin_data::single_gram::decode_single_gram(&value.expect("gram")).expect("decode");
        assert_eq!(total, 207);
        assert_eq!(
            records,
            vec![(1, 69), (0x0100_0001, 138), (0x0700_0001, 69)]
        );

        // The whole gram set walks back, not just one point read: the
        // loader takes `range_raw` over the same container.
        let mut seen = Vec::new();
        store
            .range_raw(
                std::ops::Bound::Unbounded,
                std::ops::Bound::Unbounded,
                &mut |key, _value| {
                    seen.push(u32::from_le_bytes([key[0], key[1], key[2], key[3]]));
                    Ok(())
                },
            )
            .expect("walk");
        assert_eq!(seen, vec![1], "the profile holds one gram");

        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn partial_user_bigram_items_keep_defined_reads_and_raw_bytes() {
        let dir = tempdir("partial-bigram-items");
        let path = dir.join(UserDbm::Bigram.file_name());
        let key = 0x0700_0001_u32.to_le_bytes();
        let record = [9_u32.to_le_bytes(), 7_u32.to_le_bytes()].concat();
        for tail_len in 1..8 {
            for complete_items in 0..=1 {
                let mut value = 7_u32.to_le_bytes().to_vec();
                if complete_items == 1 {
                    value.extend_from_slice(&record);
                }
                value.extend(std::iter::repeat_n(0xa5, tail_len));
                DefaultStore::write_user_bigram(&path, &[(key.to_vec(), value.clone())])
                    .expect("write user bigram");

                // The load decodes nothing: the row, partial bytes and
                // all, is the raw value every later read takes apart the
                // pin's way.
                let mut loaded = Loaded::default();
                load_bigram_rows(&dir, &mut loaded);
                assert!(loaded.skipped.is_empty());
                assert_eq!(loaded.state.bigram, [(key.to_vec(), value.clone())]);

                let raw = DefaultStore::open_user_bigram(&path).expect("open raw");
                assert_eq!(raw.get_raw(&key).expect("raw get"), Some(value));
            }
        }
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn hostile_profile_files_degrade_per_file() {
        let dir = tempdir("hostile");
        let originals = originals();
        save(&dir, &state(), &originals, &versions(), 1).expect("save");

        // A corrupted user.bin checksum and a garbage bigram container.
        let mut chunk = std::fs::read(dir.join("user.bin")).expect("read");
        let last = chunk.len() - 1;
        chunk[last] ^= 0xFF;
        std::fs::write(dir.join("user.bin"), chunk).expect("write");
        std::fs::write(dir.join(UserDbm::Bigram.file_name()), b"not a dbm").expect("write");

        let loaded = load(&dir, &originals, &versions(), UserConfLaw::Pinyin).expect("load");
        assert!(!loaded.wiped, "user.conf is intact; only files skip");
        assert_eq!(loaded.skipped.len(), 2, "{:?}", loaded.skipped);
        assert!(loaded.state.bigram.is_empty());
        assert!(!loaded.state.libraries.contains_key(&7));
        // The untouched halves still load.
        assert!(loaded.state.system_overrides.contains_key(&1));

        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    /// The frozen cross-backend record golden, regenerated by
    /// `the_saved_records_are_backend_independent`'s own dump. Identical
    /// on Kyoto Cabinet, tkrzw and Berkeley DB (measured
    /// 2026-09-09).
    const RECORDS_GOLDEN: &str = r#"## user_bigram (2 rows)
  01000000 = cf0000000100000045000000020000018a0000000100000745000000
  01000007 = 450000000100000545000000
## user_pinyin_index (12 rows)
  0200 = 
  02000f00 = 0100000582008f03
  0300 = 
  03000b00 = 0200000783028b01
  1700 = 
  17000700 = 0100000797008706
  8200 = 
  82008f03 = 0100000582008f03
  8302 = 
  83028b01 = 0200000783028b01
  9700 = 
  97008706 = 0100000797008706
## user_phrase_index (6 rows)
  17530000 = 
  17530000ac4e0000 = 01000005
  4b6d0000 = 
  4b6d0000d58b0000 = 02000007
  604f0000 = 
  604f00007d590000 = 01000007
## user.bin (83 payload bytes)
  2d000000110000001e000000530000002300000000080000001e00000023000000000000000002010f000000604f00007d590000970087060f00000002011e0000004b6d0000d58b000083028b011e00000023
## addon.bin (57 payload bytes)
  09000000110000001a0000003900000023000000000800000023000000000000000002010900000017530000ac4e000082008f030900000023
## network.bin (19 payload bytes)
  00000000110000001200000013000000232323
## gb_char.dbin (62 payload bytes)
  040000000000000004006400000047020000030000000100000010001000010164000000604f0000970064000000010147020000604f00009700a9000000
## gbk_char.dbin (18 payload bytes)
  040000000000000004000000000000000000
## opengram.dbin (18 payload bytes)
  040000000000000004000000000000000000
## merged.dbin (18 payload bytes)
  040000000000000004000000000000000000"#;

    /// The records a fixed user state produces, byte-for-byte, on every
    /// backend. The peer builds cannot share a process (the
    /// exactly-one-backend invariant), so this golden is the
    /// cross-backend equivalence check the store crate uses elsewhere:
    /// each build asserts the same bytes, and CI runs them all. It is
    /// what makes "the same records in whichever container the build
    /// selected" a tested claim rather than an assertion in a doc — the
    /// objective's "as though it were libpinyin with a peer container".
    #[test]
    fn the_saved_records_are_backend_independent() {
        let dir = tempdir("records");
        let (state, originals) = cross_backend_state();
        save(&dir, &state, &originals, &versions(), 1).expect("save");
        let got = dump_user_dir(&dir);
        assert_eq!(
            got.trim_end(),
            RECORDS_GOLDEN,
            "the backend's records drifted from the frozen golden"
        );
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    /// One fixed user state: two user phrases in library 7, one in the
    /// addon library 5, one system-token MODIFY in library 1, and two
    /// grams. Every table the file set carries is non-empty.
    fn cross_backend_state() -> (UserState, SystemOriginals) {
        let k = |i: u8, m: u8, f: u8| ChewingKey::new(i, m, f, 0).to_packed();
        let mut libraries = BTreeMap::new();
        libraries.insert(
            7_u8,
            BTreeMap::from([
                (
                    1_u32,
                    ChunkItem {
                        phrase: vec![0x4f60, 0x597d],
                        unigram: 15,
                        prons: vec![(vec![k(23, 0, 1), k(7, 0, 13)], 15)],
                    },
                ),
                (
                    2_u32,
                    ChunkItem {
                        phrase: vec![0x6d4b, 0x8bd5],
                        unigram: 30,
                        prons: vec![(vec![k(3, 0, 5), k(11, 0, 3)], 30)],
                    },
                ),
            ]),
        );
        libraries.insert(
            5_u8,
            BTreeMap::from([(
                1_u32,
                ChunkItem {
                    phrase: vec![0x5317, 0x4eac],
                    unigram: 9,
                    prons: vec![(vec![k(2, 0, 1), k(15, 0, 7)], 9)],
                },
            )]),
        );
        let gram = oxpinyin_data::single_gram::encode_single_gram;
        let bigram = vec![
            (
                1_u32.to_le_bytes().to_vec(),
                gram(207, &[(1, 69), (0x0100_0002, 138), (0x0700_0001, 69)]),
            ),
            (
                0x0700_0001_u32.to_le_bytes().to_vec(),
                gram(69, &[(0x0500_0001, 69)]),
            ),
        ];
        let mut system_overrides = BTreeMap::new();
        system_overrides.insert(
            1_u8,
            BTreeMap::from([(
                1_u32,
                Some(ChunkItem {
                    phrase: vec![0x4f60],
                    unigram: 583,
                    prons: vec![(vec![k(23, 0, 1)], 169)],
                }),
            )]),
        );
        let originals = BTreeMap::from([(
            1_u8,
            SystemLibrary {
                mapped: None,
                total: 100,
                range_end: 2,
                items: BTreeMap::from([(
                    1_u32,
                    ChunkItem {
                        phrase: vec![0x4f60],
                        unigram: 100,
                        prons: vec![(vec![k(23, 0, 1)], 100)],
                    },
                )]),
            },
        )]);
        (
            UserState {
                bigram,
                libraries,
                system_overrides,
                phrase_table: BTreeMap::new(),
                indexed: std::collections::BTreeSet::new(),
            }
            .index_every_reading(),
            originals.into(),
        )
    }

    /// Every row of every DBM and every chunk payload in `dir`, as one
    /// canonical hex dump sorted by key — the golden's exact form.
    fn dump_user_dir(dir: &Path) -> String {
        use std::fmt::Write as _;
        let mut out = String::new();
        for dbm in [UserDbm::Bigram, UserDbm::PinyinIndex, UserDbm::PhraseIndex] {
            let path = dir.join(dbm.file_name());
            let store = if dbm.is_hash() {
                DefaultStore::open_user_bigram(&path).expect("user bigram opens")
            } else {
                DefaultStore::open_user_index(&path).expect("index opens")
            };
            let mut rows = Vec::new();
            store
                .range_raw(
                    std::ops::Bound::Unbounded,
                    std::ops::Bound::Unbounded,
                    &mut |k, v| {
                        rows.push((k.to_vec(), v.to_vec()));
                        Ok(())
                    },
                )
                .expect("walk");
            rows.sort();
            let _ = writeln!(out, "## {} ({} rows)", dbm.stem(), rows.len());
            for (k, v) in rows {
                let _ = writeln!(out, "  {} = {}", hex(&k), hex(&v));
            }
        }
        for name in [
            "user.bin",
            "addon.bin",
            "network.bin",
            "gb_char.dbin",
            "gbk_char.dbin",
            "opengram.dbin",
            "merged.dbin",
        ] {
            let bytes = std::fs::read(dir.join(name)).expect("chunk file");
            let payload = read_chunk_payload(&bytes).expect("frame");
            let _ = writeln!(
                out,
                "## {name} ({} payload bytes)\n  {}",
                payload.len(),
                hex(payload)
            );
        }
        out
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}
