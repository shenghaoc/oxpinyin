//! The concrete engine assembly shared by every oxpinyin consumer.
//!
//! Wiring only: this crate opens a system data directory the way
//! `pinyin_init` does — the pinyin and phrase DBMs, the per-library
//! chunk files, `bigram.db`, `punct.bin`, the addon DBM pair, λ from
//! `table.conf` — or as `zhuyin_init` does, without addon or punctuation
//! tables. It installs the optional user store and hands out
//! [`Session`]s over the merged backends. On each backend that
//! directory is the backend's native data directory — the one a
//! libpinyin build on the same backend uses, or what the pin's
//! `gen_binary_files` / `import_interpolation` / `gen_unigram` sequence
//! writes (the `oxpinyin-utils` binaries). Nothing is scanned at open: every reader in `oxpinyin-data`
//! is a handle plus a point read. The algorithms stay where they
//! belong — decoding/composition in `oxpinyin-engine`, tables and model math
//! in `oxpinyin-data`, user state in `oxpinyin-user` — and the user-count
//! overlay arithmetic lives in `oxpinyin-data`'s `*_with_user_delta` methods
//! and `load_merged_row`, which [`RuntimeLm`] merely feeds. The one
//! ordering law held here is [`merge_suggestion_rows`], the pinned
//! suggestion row order the facades rank predicted candidates in.
//! Centralizing the assembly here is what keeps every C-ABI consumer
//! on one construction and one set of semantics.
//!
//! Pure Rust; no FFI of any kind, `unsafe_code` forbidden.
#![forbid(unsafe_code)]
// Constitution §4: keep these non-test denials. A permitted site needs the
// narrowest justified #[expect], citing the pin's abort site or an invariant
// making failure unreachable; see docs/rust-conventions.md.
// An expectation proves lint coverage, not correspondence with the pin.
// Inline #[cfg(test)] modules are exempt; tests/, benches/ and examples/
// are separate crates.
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::panic))]
#![cfg_attr(not(test), deny(clippy::panic_in_result_fn))]
#![cfg_attr(not(test), deny(clippy::unreachable))]
#![warn(missing_docs)]

mod sentence_facts;

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};

/// `PHRASE_INDEX_LIBRARY_COUNT` (`phrase_index.h`): sixteen sub-indices.
const PHRASE_INDEX_LIBRARY_COUNT: u32 = 16;

use oxpinyin_core::{
    Dictionary, LanguageModel, MergedGram, NbestStepCosts, PhraseEntry, PhraseIndexRanges,
    PhraseItemRead, PhraseText, PhraseToken, SyllableKey, TrellisItem, TrellisPathMemo,
    UserCountDelta,
};
pub use oxpinyin_data::phrase_library::{
    LibraryError, PhraseItemView, RowFault, directory_reads_as_a_chunk,
};
pub use oxpinyin_data::table_info::{FileType, TableConfSite};
pub use oxpinyin_data::table_info::{TableConf, TableConfError};
use oxpinyin_data::user_files::{SystemVersions, UserFileLayout};
use oxpinyin_data::{
    AddonDictionary, BigramLanguageModel, DictError, LmError, PunctTable, SystemDbm,
    SystemDictionary,
};
use oxpinyin_engine::{EngineError, Session, SessionSettings, StoragePaths};
use oxpinyin_user::{
    ItemOrigin, LibraryLoopRows, LibraryLoopStop, LiveSystemItems, SaveBaseline, SystemLibrary,
    UnigramAdd, UserConfLaw, UserLookup, UserStore, UserStoreError,
};

/// Orders the system and user phrase-table suggestion tokens the way
/// `_compute_predicted_prefix_candidates` receives them
/// (`pinyin.cpp:2371-2405`): `FacadePhraseTable3::search_suggestion` runs
/// the system phrase table then the user one, each filing tokens into
/// its library's array in the DBM's cursor order (byte-lexical over the
/// UCS-4 keys), and `reduce_tokens` concatenates the arrays library by
/// library. So: grouped by library nibble ascending — the system
/// libraries 1–4, then the user library 7 — and inside a group the UCS-4
/// walk order, stored token order within one text. The system rows arrive
/// in that order already (`SystemDictionary::suggestion_tokens`); the
/// user rows are ordered by their little-endian UCS-4 keys, preserving
/// the stored token order within each record.
///
/// Tokens are kept even when their live item is absent: the caller needs
/// the pin's stale-length filter and its sole-missing/class-(b) split
/// (`pinyin.cpp:2389-2405`, `:2443-2451`; #1231).
///
/// Lives beside the other pinned data+user compositions this crate holds
/// (the [`RuntimeLm`] overlay feed), so the C-ABI facades share one
/// suggestion ordering instead of each assembling an equivalent against
/// raw UCS-4 keys.
#[must_use]
pub fn merge_suggestion_rows(system: &[u32], user_rows: &[(u32, Vec<u8>)]) -> Vec<u32> {
    let mut user = user_rows.to_vec();
    user.sort_by(|a, b| a.1.cmp(&b.1));
    let mut merged = Vec::with_capacity(system.len() + user.len());
    merged.extend_from_slice(system);
    merged.extend(user.into_iter().map(|(token, _)| token));
    oxpinyin_core::sort_tokens_by_library(&mut merged);
    merged
}

// ── Open errors ─────────────────────────────────────────────────────────

/// Why a runtime could not be opened.
///
/// A missing or irregular system DBM file is not an error here: the pin
/// keeps the failed native object and initializes (pinyin.cpp:357,369,396
/// at 074a2219), and so does the runtime on every backend.
#[derive(Debug)]
pub enum OpenError {
    /// A required path exists but could not be read.
    Io(PathBuf, std::io::Error),
    /// The dictionary tables failed to open or parse.
    Dict(DictError),
    /// The language model failed to open or parse.
    Lm(LmError),
    /// The user dir's `user.conf` names a database format upstream's
    /// mapper does not know, where `to_table_database_format_type`
    /// `abort()`s (`table_info.cpp:122-133`, called at `:353-354` from
    /// `UserTableInfo::load`, which `check_format` calls).
    ///
    /// The open fails here and the C adapters abort, as upstream does
    /// (register row 44). Nothing is cleaned and no marker is written —
    /// upstream never reaches its wipe either.
    UnknownDatabaseFormat(PathBuf),
    /// The system `table.conf` is one the pin refuses or dies on
    /// (`SystemTableInfo2::load`, `table_info.cpp:194-294`, and the init
    /// loop's asserts, `pinyin.cpp:377-392`): [`TableConfError::Unopenable`]
    /// and [`TableConfError::Header`] are the pin's ordinary `false` returns
    /// (`open %s failed.` and `load %s failed!`), an ordinary init failure
    /// the C adapters answer with NULL and no warning. Every other kind
    /// aborts in the C adapters at the pin's abort sites
    /// (rows 79 and 80), and is a class (b) site the C ABI answers with NULL
    /// and one warning otherwise.
    TableConf(TableConfError),
    /// A `SYSTEM_FILE` library's user `.dbin` replay reached a pin
    /// assertion while the library loop merged it
    /// ([`oxpinyin_user::JournalFault`], rows 91 and 108): the open stops at
    /// that library, after `check_format`'s writes, and the C adapters abort.
    Journal(oxpinyin_user::JournalFault),
    /// The user session could not be built after the library loop handed
    /// it the system libraries ([`oxpinyin_user::UserStoreError::SessionSeed`],
    /// or another failure of the open past `check_format`). The pin's
    /// counterparts die, and the C adapters abort.
    UserSession(oxpinyin_user::UserStoreError),
}

/// [`Runtime::reload_system_libraries`]'s result.
#[derive(Debug)]
pub struct SystemRemap {
    /// The libraries mapped again, in the mask loop's order.
    pub originals: oxpinyin_user::SystemOriginals,
    /// The files `MemoryChunk::mmap` refused, with their sub-index, in the
    /// loop's order: each a `mmap %s failed!` line the pin writes when its
    /// loop reaches that library.
    pub unmappable: Vec<(u8, PathBuf)>,
    /// The library that is a directory the pin asserts on, where the walk
    /// ended (`memory_chunk.h:493`).
    pub header_fault: Option<u8>,
}

/// A mask loop's remap, advanced one library at a time by
/// [`Runtime::remap_next`]: the libraries mapped so far, and the sub-indices
/// still to visit. Nothing past a stop is opened.
#[derive(Debug)]
pub struct RemapWalk {
    originals: oxpinyin_user::SystemOriginals,
    indices: std::ops::Range<u8>,
    /// One library's `_load_phrase_library`, not the mask loop: it maps an
    /// unloaded library, and a row whether or not the row names a user file.
    reload: bool,
}

/// What [`Runtime::remap_next`] reached. A library the walk skips (in the
/// mask loop one that is not loaded or names no user file; in either a chunk
/// that fails some other check) is not a step.
#[derive(Debug)]
pub enum RemapStep {
    /// The library mapped and is in the walk's originals.
    Mapped(u8),
    /// `MemoryChunk::mmap` refuses the file (row 76): the `mmap %s failed!`
    /// path.
    Unmappable(u8, PathBuf),
    /// A directory whose header read the pin asserts on (`memory_chunk.h:493`,
    /// row 96). The walk is over.
    Directory(u8),
}

/// Why `save_disk_baseline` could not give a baseline for a
/// system library the save maps again at its `_write_files` position
/// (`pinyin.cpp:955`, `zhuyin.cpp:588`).
#[derive(Debug)]
pub enum SaveRemapFault {
    /// `MemoryChunk::mmap` refuses the file: the pin writes
    /// `mmap %s failed!` (row 76, class (b)).
    Unmappable(PathBuf),
    /// The file is a directory whose header read `MemoryChunk::mmap`
    /// asserts on (`memory_chunk.h:493`, register row 96).
    ChunkHeaderRead,
}

impl OpenError {
    /// The system library file the pin's `MemoryChunk::mmap` refuses, when
    /// that is why the open failed: the pin writes `mmap %s failed!` for it
    /// (`pinyin.cpp:256`) and dies on the empty chunk it goes on with. A
    /// layout fault past the chunk and every other failure answer `None`.
    #[must_use]
    pub fn unmappable_library(&self) -> Option<&Path> {
        match self {
            Self::Dict(DictError::Library(error)) => error.unmappable_path(),
            Self::Io(..)
            | Self::Dict(_)
            | Self::Lm(_)
            | Self::UnknownDatabaseFormat(_)
            | Self::TableConf(_)
            | Self::Journal(_)
            | Self::UserSession(_) => None,
        }
    }

    /// Whether the open failed on a default row that names no system chunk
    /// ([`TableConfSite::NullSystemFile`]): the pin reads the system
    /// directory itself as the chunk.
    #[must_use]
    pub fn null_system_chunk(&self) -> bool {
        matches!(
            self,
            Self::TableConf(TableConfError::Abort(TableConfSite::NullSystemFile))
        )
    }

    /// Whether the open failed on a system library that is a directory the
    /// pin's `MemoryChunk::mmap` asserts on (`memory_chunk.h:493`, register
    /// row 96), after the libraries before it were mapped and merged. The C
    /// adapters abort.
    #[must_use]
    pub fn chunk_header_read(&self) -> bool {
        matches!(
            self,
            Self::Dict(DictError::Library(LibraryError::Row(
                RowFault::ChunkHeaderRead
            )))
        )
    }
}

impl core::fmt::Display for OpenError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Io(path, error) => write!(f, "cannot read {}: {error}", path.display()),
            Self::Dict(error) => write!(f, "dictionary error: {error}"),
            Self::Lm(error) => write!(f, "language model error: {error}"),
            Self::UnknownDatabaseFormat(path) => write!(
                f,
                "unknown database format in {} (upstream aborts, table_info.cpp:132)",
                path.join("user.conf").display()
            ),
            Self::TableConf(error) => write!(f, "table.conf: {error}"),
            Self::Journal(fault) => write!(f, "journal replay: {fault}"),
            Self::UserSession(error) => write!(f, "user session: {error}"),
        }
    }
}

impl std::error::Error for OpenError {}

// ── Addon libraries ─────────────────────────────────────────────────────

/// The addon facade shared by every session derived from one runtime:
/// the addon DBM pair opened at init (upstream attaches
/// `addon_pinyin_index.bin` / `addon_phrase_index.bin` in `pinyin_init`)
/// and the chunk files `pinyin_load_addon_phrase_library` loads.
struct AddonSet {
    dict: AddonDictionary,
}

impl AddonSet {
    /// Drops addon library `index`, if it is loaded.
    ///
    /// The pin's `unload` is unconditional and answers `true` whether or
    /// not the library was loaded (`pinyin.cpp:124-131`,
    /// `FacadePhraseIndex::unload`), so this reports success the same way
    /// rather than distinguishing the two.
    fn unload(&mut self, index: u8) -> bool {
        self.dict.unload(index)
    }

    fn load_reporting(&mut self, index: u8, system_dir: &Path) -> Result<bool, LibraryError> {
        self.dict.load_reporting(index, system_dir)
    }

    fn search_into(
        &self,
        syllables: &[SyllableKey],
        out: &mut Vec<PhraseEntry>,
        row: &mut Vec<u8>,
    ) -> Result<bool, DictError> {
        self.dict.search_into(syllables, out, row)
    }

    fn unigram_freq(&self, token: u32) -> Option<u64> {
        self.dict.unigram_freq(token)
    }

    fn unigram_total(&self) -> Option<u64> {
        self.dict.unigram_total()
    }

    fn is_empty(&self) -> bool {
        self.dict.is_empty()
    }

    /// The addon phrase item behind `token` as stored bytes, the
    /// `get_phrase_item` half of the promotion (`pinyin.cpp:2534-2549`).
    ///
    /// `None` when no loaded addon library owns `token`.
    fn phrase_item(&self, token: u32) -> Option<Vec<u8>> {
        self.dict.phrase_item(token)
    }
}

/// `PHRASE_INDEX_LIBRARY_INDEX(token)` (`novel_types.h:44` at the pin
/// `074a2219`): the library a token names. The high four bits are masked.
const fn library_of(token: u32) -> u8 {
    oxpinyin_user::phrase_index_library_index(token)
}

pub use oxpinyin_core::TokenPhrase;

/// One token reading, without the text or any other reading's keys.
pub struct TokenPronunciation {
    /// Stored character count, also used for the out-of-range C result.
    pub phrase_length: usize,
    /// The reading's stored packed `ChewingKey` words, which the pin's
    /// `get_nth_pronunciation` copies verbatim (`phrase_index.cpp:33-50`)
    /// for system and USER_FILE items alike. `None` when the item exists but
    /// the requested reading does not.
    pub keys: Option<Vec<u16>>,
}

/// The pin's query keys for `syllables`.
fn chewing_query(syllables: &[SyllableKey]) -> Vec<oxpinyin_core::ChewingKey> {
    syllables.iter().map(|key| key.chewing_key()).collect()
}

// ── Merged backends ─────────────────────────────────────────────────────

/// The system dictionary with the user-phrase lookup merged in.
///
/// It borrows the runtime's tables and store for one operation.
#[derive(Clone, Copy)]
pub struct RuntimeDict<'a> {
    system: SystemDictionary<
        &'a oxpinyin_data::phrase_libraries::PhraseLibraries,
        &'a oxpinyin_data::dict::SystemTables,
    >,
    user: &'a UserStore,
    addons: &'a RefCell<AddonSet>,
    punct: &'a PunctTable,
    /// The loaded-library mask: bit `n` **set** = library `n` unloaded
    /// (matches `library_visible` at :344 and the query surface —
    /// `library_visible_token` and the
    /// `unload_library` setter — that all consult it).  The pinyin ABI only
    /// ever sets bit 2 (`GBK_DICTIONARY`, `pinyin.cpp:464-472`); the zhuyin
    /// ABI any bit but 1 (`zhuyin.cpp:378-388`).
    library_mask: &'a Cell<u32>,
}

impl<'a> RuntimeDict<'a> {
    /// The one lookup: the system table's search, then the user table's, as
    /// `FacadeChewingTable2::search` runs them (`facade_chewing_table2.h:
    /// 109-129` at 074a2219), each token with its text. The search carries
    /// tokens only; the ranking reads each token's frequency
    /// (`_compute_frequency_of_items`, `pinyin.cpp:1794`) and a step reads
    /// its pronunciation possibility (`phonetic_lookup.h:646-656`) on their
    /// own. The answer is whether a token of a loaded library had no
    /// readable item.
    fn fill_lookup(
        &self,
        syllables: &[SyllableKey],
        tones: &[u8],
        out: &mut Vec<PhraseEntry>,
        row: &mut Vec<u8>,
    ) -> Result<(oxpinyin_core::UnreadableRows, bool), DictError> {
        let (mut unreadable, system_present) = self.system.lookup_with_live(
            syllables,
            tones,
            out,
            row,
            |token| self.library_visible(u32::from(library_of(token))),
            |token| {
                self.live_items()
                    .read(token, |item| {
                        Some(PhraseEntry::new(
                            PhraseToken::new(token),
                            item.phrase_text()?,
                        ))
                    })
                    .found()
                    .flatten()
            },
        )?;
        let query: Vec<_> = syllables.iter().map(|key| key.chewing_key()).collect();
        self.user.note_pinyin_search_keys(&query);
        let mut query = query;
        for (key, &tone) in query.iter_mut().zip(tones) {
            *key = key.with_tone(tone);
        }
        self.user_lookup()
            .lookup_resolving_missing(&query, out, |token| {
                if !self.system.library_loaded(token)
                    || !self.library_visible(u32::from(library_of(token)))
                {
                    return None;
                }
                let text = self.token_text(token);
                if text.is_none() {
                    unreadable.push(PhraseToken::new(token));
                }
                text
            });
        if self.token_filter_active() {
            // An unloaded library's phrases leave every read: upstream
            // frees the sub-index (`phrase_index.cpp:260-268`), the
            // mask keeps the monolithic tables resident but invisible.
            out.retain(|entry| self.library_visible_token(entry.token().value()));
        }
        Ok((unreadable, system_present))
    }

    /// The user pinyin table's search for `syllables`, as the pin's
    /// `ChewingLargeTable2::search` runs it: the table entry it leaves
    /// behind is what a later `mask_out` walk starts from (#785).
    fn note_user_search(&self, syllables: &[SyllableKey]) {
        self.user.note_pinyin_search(|| chewing_query(syllables));
    }

    /// The query's toned keys: each syllable's key with the tone at its
    /// index, `0` past the end of `tones`.
    fn trellis_query(syllables: &[SyllableKey], tones: &[u8]) -> Vec<oxpinyin_core::ChewingKey> {
        syllables
            .iter()
            .enumerate()
            .map(|(index, key)| {
                key.chewing_key()
                    .with_tone(tones.get(index).copied().unwrap_or(0))
            })
            .collect()
    }

    /// Whether a ranged `token` is loaded and visible to the library mask.
    fn token_ranged(&self, token: u32) -> bool {
        self.system.library_loaded(token) && self.library_visible_token(token)
    }

    /// The user table's `convert` call of [`Self::trellis_ranges`].
    fn user_trellis_ranges(
        &self,
        query: &[oxpinyin_core::ChewingKey],
        ranges: &mut PhraseIndexRanges<()>,
    ) {
        let tokens = self.user_lookup().search_toned(query);
        ranges.convert(
            tokens
                .into_iter()
                .filter(|token| self.token_ranged(*token))
                .map(|token| (token, Some(()))),
        );
    }

    /// `(matched, total)` of `token`'s effective system item over `query`,
    /// the wrapping `guint32` sums of `phrase_index.h:136-163`.
    fn effective_possibility(
        &self,
        query: &[oxpinyin_core::ChewingKey],
        token: u32,
    ) -> Option<(u32, u32)> {
        self.live_items()
            .read(token, |item| {
                if item.origin() != ItemOrigin::Overlay {
                    return None;
                }
                let (matched, total) = Self::item_possibility(item, query);
                Some((matched, total))
            })
            .found()
            .flatten()
    }

    fn item_possibility(
        item: oxpinyin_user::SystemItem<'_>,
        query: &[oxpinyin_core::ChewingKey],
    ) -> (u32, u32) {
        let mut total = 0_u32;
        let mut matched = 0_u32;
        for reading in item.pronunciations() {
            total = total.wrapping_add(reading.frequency);
            if query.len() == reading.keys.len()
                && query.iter().zip(reading.keys.iter()).all(|(q, bits)| {
                    let s = oxpinyin_core::ChewingKey::from_packed(bits);
                    q.initial == s.initial
                        && ((q.middle == s.middle && q.final_ == s.final_)
                            || (q.middle == 0 && q.final_ == 0)
                            || (s.middle == 0 && s.final_ == 0))
                        && (q.tone == s.tone || q.tone == 0 || s.tone == 0)
                })
            {
                matched = matched.wrapping_add(reading.frequency);
            }
        }
        (matched, total)
    }

    /// What [`Self::priced_token`] reads of `syllables` whatever the
    /// token: the user table's lookup and the path's toned keys. The pin
    /// prices a step with the matrix keys, tones included
    /// (`phonetic_key_matrix.cpp:562,575` at 074a2219), and a nonzero
    /// query tone matches only an equal or zero stored tone
    /// (`pinyin_compare_tone3`, `pinyin_phrase3.h:96-105`).
    fn path_pricing(
        &self,
        syllables: &[SyllableKey],
        tones: &[u8],
    ) -> Result<PathPricing, DictError> {
        Ok(PathPricing {
            user: self.user_lookup().lookup(syllables),
            keys: Self::trellis_query(syllables, tones),
        })
    }

    /// The item a step prices for `token`, including an unranged ONESTEP
    /// token. Ranged user entries retain their query text; otherwise read
    /// the stored item. The effective-item overlay and library mask apply.
    /// The text is built only when `want_text`.
    fn priced_token(
        &self,
        syllables: &[SyllableKey],
        tones: &[u8],
        token: u32,
        want_text: bool,
    ) -> Result<Option<PricedToken>, DictError> {
        if syllables.is_empty() {
            return Ok(None);
        }
        Ok(self.priced_on_path(&self.path_pricing(syllables, tones)?, token, want_text))
    }

    /// [`Self::priced_token`] over a non-empty path's [`PathPricing`].
    fn priced_on_path(
        &self,
        path: &PathPricing,
        token: u32,
        want_text: bool,
    ) -> Option<PricedToken> {
        let user = path
            .user
            .iter()
            .rev()
            .find(|entry| entry.token().value() == token);
        let mut priced = if let Some(entry) = user {
            // The pin prices a user item from its own readings, as a system
            // one (`phonetic_lookup.h:646-656`, `phrase_index.h:136-164` at
            // 074a2219). The user lookup lists only tokens with a session
            // item, which `with_user_item` reads.
            let pronunciation = self.user.with_user_item(token, |view| {
                let (matched, total) =
                    Self::item_possibility(oxpinyin_user::SystemItem::Replayed(view), &path.keys);
                (u64::from(matched), u64::from(total))
            });
            PricedToken {
                item: TrellisItem {
                    chars: entry.phrase_length().try_into().unwrap_or(u32::MAX),
                    pronunciation,
                },
                text: want_text.then(|| TokenPhrase::from(entry.text().clone())),
            }
        } else if let Some(priced) = self.user.with_user_item(token, |view| {
            // A second imported reading need not have an index entry
            // (074a2219 pinyin.cpp:563-582). ONESTEP still reads its item.
            let (matched, total) =
                Self::item_possibility(oxpinyin_user::SystemItem::Replayed(view), &path.keys);
            let pronunciation = Some((u64::from(matched), u64::from(total)));
            PricedToken {
                item: TrellisItem {
                    chars: view.phrase_length() as u32,
                    pronunciation,
                },
                text: want_text.then(|| TokenPhrase {
                    phrase_length: view.phrase_length(),
                    text: view.phrase_text(),
                }),
            }
        }) {
            priced
        } else {
            let live = self.live_items();
            // A text that does not convert keeps the item: the pin's
            // `convert_to_utf8` drops only that phrase from the sentence
            // (`lookup/lookup.cpp:58-65` at 074a2219).
            let (chars, text) = live
                .read(token, |item| {
                    (
                        u32::try_from(item.phrase_length()).unwrap_or(u32::MAX),
                        want_text.then(|| TokenPhrase {
                            phrase_length: item.phrase_length(),
                            text: item.phrase_text(),
                        }),
                    )
                })
                .found()?;
            PricedToken {
                item: TrellisItem {
                    chars,
                    pronunciation: None,
                },
                text,
            }
        };
        // The effective-item overlay prices against the same keys, when
        // the store holds system items.
        if self.live_items().has_overlay()
            && let Some((matched, total)) = self.effective_possibility(&path.keys, token)
        {
            priced.item.pronunciation = Some((u64::from(matched), u64::from(total)));
        }
        if self.token_filter_active() && !self.library_visible_token(token) {
            return None;
        }
        Some(priced)
    }

    fn user_lookup(&self) -> UserLookup<'_> {
        self.user.lookup()
    }

    /// The underlying system table set, without the user overlay.
    #[must_use]
    pub fn system(
        &self,
    ) -> &SystemDictionary<
        &'a oxpinyin_data::phrase_libraries::PhraseLibraries,
        &'a oxpinyin_data::dict::SystemTables,
    > {
        &self.system
    }

    /// Punctuations registered for `token`, if the punct table shipped
    /// with the system dir — one point read; a failed read answers none.
    #[must_use]
    pub fn punctuations(&self, token: u32) -> Vec<PhraseText> {
        self.punct.punctuations(token).unwrap_or_default()
    }

    /// Loads addon library `index` from `system_dir`: an error when
    /// the library file is absent or does not verify, where
    /// [`LibraryError::unmappable_path`] names the file the pin could not
    /// map. `Ok(false)` for an index that names no addon library and for
    /// one already loaded.
    ///
    /// # Errors
    ///
    /// [`LibraryError`] when the library file is absent or does not verify.
    fn load_addon_reporting(&self, index: u8, system_dir: &Path) -> Result<bool, LibraryError> {
        let mut addons = self.addons.borrow_mut();
        addons.load_reporting(index, system_dir)
    }

    /// Unloads addon library `index`.
    ///
    /// Answers `true` whether or not the library was loaded, mirroring the
    /// pin's unconditional `unload`. The caller applies the pin's
    /// `index < PHRASE_INDEX_LIBRARY_COUNT` bound: that assertion is an ABI
    /// availability-class concern, not a runtime one.
    #[must_use]
    fn unload_addon(&self, index: u8) -> bool {
        let mut addons = self.addons.borrow_mut();
        addons.unload(index)
    }

    /// Unloads default library `index`: sets bit `n` of the visibility mask,
    /// so library `n` stops answering. `false` only for an index outside the
    /// sixteen sub-indices; the pin ignores what its `unload` returns
    /// (`pinyin.cpp:473-474`, `zhuyin.cpp:386-387`), so a repeat unload
    /// answers `true` too. Which indices a facade lets through is the C
    /// ABI's rule, not the runtime's: `pinyin_unload_phrase_library` refuses
    /// everything but `GBK_DICTIONARY` (2), `zhuyin_unload_phrase_library`
    /// everything but `TSI_DICTIONARY` (1).
    #[must_use]
    fn unload_library(&self, index: u32) -> bool {
        if index >= PHRASE_INDEX_LIBRARY_COUNT {
            return false;
        }
        let mask = 1u32 << index;
        self.library_mask.set(self.library_mask.get() | mask);
        true
    }

    /// Re-loads library `index` after an unload — upstream re-attaches
    /// the sub-index from disk and answers `true`; already-loaded (mask
    /// clear) answers `false` (`pinyin.cpp:234-243`). A library that was
    /// never unloaded is already loaded: the system tables load at init, so
    /// upstream's already-loaded rule answers `false` there too.
    #[must_use]
    fn load_library(&self, index: u32) -> bool {
        // The system tables (1, 2, 4) and the USER_FILE library (7) all load
        // at init (the default-tables loop includes the USER_DICTIONARY row —
        // measured `load(7)` = false on the pin), so the already-loaded rule
        // answers `false` for every index an unload has not cleared.
        if index >= PHRASE_INDEX_LIBRARY_COUNT {
            return false;
        }
        let bit = 1u32 << index;
        let mask = self.library_mask.get();
        self.library_mask.set(mask & !bit);
        mask & bit != 0
    }

    /// Whether the configured default row is a system library.
    #[must_use]
    pub fn is_system_library(&self, index: u8) -> bool {
        self.live_items().is_system_library(index)
    }

    /// Borrow the effective system items for one operation.
    #[must_use]
    pub fn live_items(&self) -> LiveSystemItems<'_> {
        LiveSystemItems::new(self.system.libraries(), self.user, self.library_mask)
    }

    /// Borrow the effective system items with no library unloaded, for the
    /// phrase export iterator, which checks a library's visibility once when
    /// it probes.
    #[must_use]
    pub fn live_items_ignoring_mask(&self) -> LiveSystemItems<'_> {
        LiveSystemItems::unmasked(self.system.libraries(), self.user)
    }

    /// Whether library `nibble` is visible (not unloaded). Shared
    /// primitive: `mask == 0` (all libraries loaded) and any nibble
    /// outside the u32 bit range both answer `true` — those nibbles
    /// have no bit to be masked off, so they are trivially visible.
    /// Every other visibility surface (`library_visible_token`,
    /// `pinyin_token_add_unigram_frequency` callers) routes through
    /// this to keep the shift and the `mask == 0` shortcut in one
    /// place.
    #[must_use]
    pub fn library_visible(&self, nibble: u32) -> bool {
        let mask = self.library_mask.get();
        if mask == 0 || nibble >= 32 {
            return true;
        }
        mask & (1u32 << nibble) == 0
    }

    /// Whether the token's item is readable, for entry-level filters: its
    /// library is not unloaded, and no journal REMOVE took the item out of the
    /// sub-index (`get_phrase_item` fails for both, 074a2219
    /// `phrase_index.cpp:180-206`). The phrase table still holds the token
    /// either way ([`Self::phrase_table_token_enabled`]).
    #[must_use]
    pub fn library_visible_token(&self, token: u32) -> bool {
        let live = self.live_items();
        if live.is_system_library(library_of(token)) {
            return live.read(token, |_| ()).found().is_some();
        }
        live.is_visible(token)
    }

    /// Whether any entry-level filter can hide a token: a library is
    /// unloaded, or the user store holds a removed system item.
    fn token_filter_active(&self) -> bool {
        self.live_items().filter_active()
    }

    /// Whether phrase-table search has an enabled token array for this library.
    /// Unlike visibility alone, this excludes libraries never loaded into the
    /// facade (074a2219 phrase_index.h:762-772; phrase_large_table3.cpp:40-42).
    #[must_use]
    pub fn phrase_table_token_enabled(&self, token: u32) -> bool {
        // `add_index` files a token under its masked library
        // (074a2219 phrase_large_table3.cpp:42, novel_types.h:44).
        let library = library_of(token);
        self.library_visible(u32::from(library))
            && (self.system.library_loaded(token) || self.user.has_user_library(library))
    }

    /// Read the live item's stored pronunciation count.
    /// The pin reads only its offset and two header bytes (074a2219
    /// `phrase_index.cpp:180-205`, `pinyin.cpp:2786-2798`,
    /// `zhuyin.cpp:1778-1790`). This path skips the introspection view.
    #[must_use]
    pub fn token_pronunciation_count(&self, token: u32) -> Option<u32> {
        // PHRASE_INDEX_LIBRARY_INDEX ignores the high four bits
        // (074a2219 novel_types.h:42-44).
        let token = token & (oxpinyin_user::PHRASE_INDEX_LIBRARY_MASK | oxpinyin_user::PHRASE_MASK);
        if self.is_system_library(library_of(token)) {
            return self.live_items().pronunciation_count(token).found();
        }
        if !self.library_visible(u32::from(library_of(token))) {
            return None;
        }
        self.user.user_pronunciation_count(token)
    }

    /// The live item's text for `_add_phrase`'s length and content checks
    /// (074a2219 pinyin.cpp:566-571; zhuyin.cpp:452-457). Reading spellings,
    /// frequencies and the unigram do not participate in those checks.
    #[must_use]
    pub fn token_text(&self, token: u32) -> Option<PhraseText> {
        if self.is_system_library(library_of(token)) {
            return self
                .live_items()
                .read(token, |item| item.phrase_text())
                .found()
                .flatten();
        }
        if !self.library_visible(u32::from(library_of(token))) {
            return None;
        }
        self.user
            .with_user_item(token, |item| item.phrase_text())
            .flatten()
    }

    /// The stored length and text of the token's item, as the C token
    /// getters read them (`_token_get_phrase`, 074a2219 `pinyin.cpp:1627-1646`,
    /// `zhuyin.cpp:1096-1114`). `None` only when `get_phrase_item` fails, or
    /// for a shipped item whose text passes the content end (compatibility
    /// policy row 130). A shipped item is read through its header-bounded
    /// view, so readings that pass the content end do not hide its text.
    #[must_use]
    pub fn token_phrase(&self, token: u32) -> Option<TokenPhrase> {
        let read = |length: usize, words_present: bool, text: Option<PhraseText>| {
            words_present.then_some(TokenPhrase {
                phrase_length: length,
                text,
            })
        };
        if self.is_system_library(library_of(token)) {
            return self
                .live_items()
                .read_fields(token, |item| {
                    read(
                        item.phrase_length(),
                        item.phrase_words_present(),
                        item.phrase_text(),
                    )
                })
                .found()
                .flatten();
        }
        if !self.library_visible(u32::from(library_of(token))) {
            return None;
        }
        self.user
            .with_user_item(token, |item| {
                read(
                    item.phrase_length(),
                    item.phrase_words_present(),
                    item.phrase_text(),
                )
            })
            .flatten()
    }

    /// Whether the default facade owns this item. Missing/unloaded libraries
    /// and absent slots both answer false, as `get_phrase_item` does
    /// (074a2219 `phrase_index.h:646-652`, `phrase_index.cpp:180-205`).
    /// No text or pronunciation payload is decoded.
    #[must_use]
    pub fn token_exists(&self, token: u32) -> bool {
        if self.is_system_library(library_of(token)) {
            return self
                .live_items()
                .pronunciation_count(token)
                .found()
                .is_some();
        }
        self.library_visible(u32::from(library_of(token))) && self.user.phrase_exists(token)
    }

    /// Read only the nth insertion-ordered key list and the stored length of
    /// a system item. `None` means a missing item/library; a present item
    /// with no nth reading retains its length for the existing class-(b)
    /// zero-key C result (074a2219 `pinyin.cpp:2801-2818`,
    /// `zhuyin.cpp:1793-1810`). A `USER_FILE` item is read in place with
    /// [`Self::with_user_nth_pronunciation_words`]; this answers `None` for
    /// it.
    #[must_use]
    pub fn token_nth_pronunciation(&self, token: u32, nth: usize) -> Option<TokenPronunciation> {
        if self.is_system_library(library_of(token)) {
            // A reading at or past the count is the zero-key result; one
            // below it whose bytes pass the content end answers no reading
            // (compatibility policy row 130).
            return self
                .live_items()
                .read_fields(token, |item| {
                    let keys = item
                        .pronunciation(nth)
                        .map(|reading| reading.keys.iter().collect());
                    (keys.is_some() || nth >= item.n_pronunciations()).then(|| TokenPronunciation {
                        phrase_length: item.phrase_length(),
                        keys,
                    })
                })
                .found()
                .flatten();
        }
        None
    }

    /// Read one `USER_FILE` reading's stored key words without copying them
    /// out of the session arena.
    ///
    /// The callback receives the stored phrase length and the reading's
    /// packed little-endian words, or `None` when the item has no such
    /// reading (the pin's zeroed past-the-end result). `None` is returned
    /// when no visible `USER_FILE` item backs the token, so a caller falls
    /// back to [`Self::token_nth_pronunciation`], which serves a system item.
    pub fn with_user_nth_pronunciation_words<R>(
        &self,
        token: u32,
        nth: usize,
        read: impl FnOnce(usize, Option<&[u8]>) -> R,
    ) -> Option<R> {
        if self.is_system_library(library_of(token))
            || !self.library_visible(u32::from(library_of(token)))
        {
            return None;
        }
        self.user.with_nth_pronunciation_words(token, nth, read)
    }

    /// The addon phrase item behind `token`, for the choose-promotion path.
    #[must_use]
    pub fn addon_phrase_item(&self, token: u32) -> Option<Vec<u8>> {
        let addons = self.addons.borrow();
        addons.phrase_item(token)
    }
}

/// What one key path's pricing reads whatever the token, kept in a
/// [`TrellisPathMemo`] for the n-best call that prices the path.
pub struct PathPricing {
    /// The user table's lookup of the path.
    user: Vec<PhraseEntry>,
    /// The path's toned keys ([`RuntimeDict::trellis_query`]).
    keys: Vec<oxpinyin_core::ChewingKey>,
}

/// One token as the ordinary lookup prices it.
struct PricedToken {
    item: TrellisItem,
    /// The item's stored length and text, read only when wanted.
    text: Option<TokenPhrase>,
}

impl Dictionary for RuntimeDict<'_> {
    type Syllable = SyllableKey;
    type Error = DictError;
    type PathMemo = TrellisPathMemo<PathPricing>;

    fn lookup(&self, syllables: &[Self::Syllable]) -> Result<Vec<PhraseEntry>, Self::Error> {
        let mut entries = Vec::new();
        self.lookup_into(syllables, &mut entries)?;
        Ok(entries)
    }

    fn trellis_records(
        &self,
        syllables: &[Self::Syllable],
        tones: &[u8],
    ) -> Result<Vec<PhraseEntry>, Self::Error> {
        let mut ranges = PhraseIndexRanges::new();
        self.trellis_ranges(syllables, tones, &mut ranges)?;
        let mut out = Vec::new();
        for (token, ()) in ranges.walk_tokens() {
            if let Some(priced) = self.priced_token(syllables, tones, token, true)? {
                // An entry needs a text; an item whose text does not convert
                // is not listed.
                let Some(text) = priced.text.and_then(|phrase| phrase.text) else {
                    continue;
                };
                let mut entry = PhraseEntry::new(PhraseToken::new(token), text);
                if let Some((matched, total)) = priced.item.pronunciation {
                    entry = entry.with_pronunciation_possibility(matched, total);
                }
                out.push(entry);
            }
        }
        Ok(out)
    }

    /// `FacadeChewingTable2::search` (`facade_chewing_table2.h:109-129` at
    /// the pin `074a2219`): the system table's ranges, then the user
    /// table's, each its own `convert` call. Tokens only, as the pin's
    /// `convert` files them; [`Self::trellis_item`] prices one on demand.
    fn trellis_ranges(
        &self,
        syllables: &[Self::Syllable],
        tones: &[u8],
        ranges: &mut PhraseIndexRanges<()>,
    ) -> Result<(), Self::Error> {
        let query = Self::trellis_query(syllables, tones);
        // The user table's search of this path (#785); the pin makes one
        // per `FacadeChewingTable2::search`, and a repeat is idempotent.
        self.note_user_search(syllables);
        let loaded = |token: u32| self.token_ranged(token);
        self.system
            .trellis_ranges_with(syllables, tones, ranges, loaded)?;
        self.user_trellis_ranges(&query, ranges);
        Ok(())
    }

    /// [`Self::trellis_ranges`] and [`Self::phrase_prefix_exists`] with one
    /// system DBM get.
    fn trellis_ranges_continued(
        &self,
        syllables: &[Self::Syllable],
        tones: &[u8],
        ranges: &mut PhraseIndexRanges<()>,
        scratch: &mut Vec<u8>,
    ) -> Result<bool, Self::Error> {
        let query = Self::trellis_query(syllables, tones);
        let loaded = |token: u32| self.token_ranged(token);
        let system_extends = self
            .system
            .trellis_ranges_continued_with(syllables, tones, ranges, loaded, scratch)?;
        self.user_trellis_ranges(&query, ranges);
        let user = self.user_lookup();
        self.note_user_search(syllables);
        if system_extends {
            return Ok(true);
        }
        Ok(user.phrase_prefix_exists(syllables))
    }

    fn trellis_item(
        &self,
        syllables: &[Self::Syllable],
        tones: &[u8],
        token: u32,
    ) -> Result<Option<TrellisItem>, Self::Error> {
        Ok(self
            .priced_token(syllables, tones, token, false)?
            .map(|priced| priced.item))
    }

    /// [`Self::trellis_item`], reading the user lookup and the keys once
    /// per path.
    fn trellis_item_on_path(
        &self,
        syllables: &[Self::Syllable],
        tones: &[u8],
        token: u32,
        memo: &Self::PathMemo,
    ) -> Result<Option<TrellisItem>, Self::Error> {
        if syllables.is_empty() {
            return Ok(None);
        }
        let path = memo.get_or_try_init(|| self.path_pricing(syllables, tones))?;
        Ok(self
            .priced_on_path(path, token, false)
            .map(|priced| priced.item))
    }

    fn trellis_item_with_facts(
        &self,
        syllables: &[Self::Syllable],
        tones: &[u8],
        token: u32,
        memo: &Self::PathMemo,
        facts: &mut oxpinyin_core::sentence_facts::SentenceFacts,
    ) -> Result<Option<TrellisItem>, Self::Error> {
        if syllables.is_empty() {
            return Ok(None);
        }
        let path = memo.get_or_try_init(|| self.path_pricing(syllables, tones))?;
        Ok(self
            .priced_with_facts(path, token, false, facts)
            .map(|priced| priced.item))
    }

    fn trellis_text_with_facts(
        &self,
        syllables: &[Self::Syllable],
        tones: &[u8],
        token: u32,
        facts: &mut oxpinyin_core::sentence_facts::SentenceFacts,
    ) -> Result<Option<TokenPhrase>, Self::Error> {
        if syllables.is_empty() {
            return Ok(None);
        }
        Ok(self
            .priced_with_facts(&self.path_pricing(syllables, tones)?, token, true, facts)
            .and_then(|priced| priced.text))
    }

    fn trellis_text(
        &self,
        syllables: &[Self::Syllable],
        tones: &[u8],
        token: u32,
    ) -> Result<Option<TokenPhrase>, Self::Error> {
        Ok(self
            .priced_token(syllables, tones, token, true)?
            .and_then(|priced| priced.text))
    }

    fn lookup_into(
        &self,
        syllables: &[Self::Syllable],
        out: &mut Vec<PhraseEntry>,
    ) -> Result<(), Self::Error> {
        self.lookup_into_flagged(syllables, out).map(|_| ())
    }

    fn lookup_into_flagged(
        &self,
        syllables: &[Self::Syllable],
        out: &mut Vec<PhraseEntry>,
    ) -> Result<bool, Self::Error> {
        self.fill_lookup(syllables, &[], out, &mut Vec::new())
            .map(|(rows, _)| rows.count != 0)
    }

    fn candidate_search_into(
        &self,
        syllables: &[Self::Syllable],
        tones: &[u8],
        out: &mut Vec<PhraseEntry>,
        row: &mut Vec<u8>,
    ) -> Result<(oxpinyin_core::UnreadableRows, bool), Self::Error> {
        // One system get per path serves the tokens and `SEARCH_CONTINUED`,
        // then the user table's search, with no early return
        // (`facade_chewing_table2.h:109-129` at 074a2219). A failed
        // attachment's get is a miss; its native read stays the only one.
        let (unreadable, system_present) = self.fill_lookup(syllables, tones, out, row)?;
        Ok((
            unreadable,
            system_present || self.user_lookup().phrase_prefix_exists(syllables),
        ))
    }

    fn phrase_prefix_exists(&self, syllables: &[Self::Syllable]) -> Result<bool, Self::Error> {
        // `SEARCH_CONTINUED` comes from the pinyin table's DB get alone, before
        // any token or library check (`chewing_large_table2_tkrzwdb.cpp:146-152`,
        // `chewing_large_table2_bdb.cpp:182-187`,
        // `chewing_large_table2_kyotodb.cpp:160-166` at 074a2219). A library mask
        // only drops a token at `convert`'s NULL range
        // (`chewing_large_table2.h:107-111`), and a journal REMOVE clears only the
        // sub-index, so neither reaches this answer.
        let system_extends = self.system.phrase_prefix_exists(syllables)?;
        // The pin searches the system table and then the user table with no
        // early return (`facade_chewing_table2.h:120-126` at 074a2219), so
        // the user lookup and its search note come before the answer.
        let user = self.user_lookup();
        self.note_user_search(syllables);
        if system_extends {
            return Ok(true);
        }
        Ok(user.phrase_prefix_exists(syllables))
    }

    /// Exact-text token lookup across the system and user seams, in that
    /// order — the merge order `Dictionary::lookup_into` established. The
    /// phrase-segment span DP consumes this per character span.
    fn tokens_for_text(&self, text: &str) -> Vec<PhraseToken> {
        let mut tokens: Vec<u32> = self.system.tokens_for_text(text).unwrap_or_default();
        // The pin probes the user phrase table directly (074a2219
        // pinyin.cpp:2652-2667, storage/facade_phrase_table3.h:120-126).
        // Read only this text's row. A phrase write must not make this
        // query rebuild the full pronunciation and phrase indices.
        tokens.extend(
            self.user
                .phrase_table_tokens_for_text(text)
                .unwrap_or_default(),
        );
        tokens.retain(|token| self.phrase_table_token_enabled(*token));
        // reduce_tokens groups the system and user table results by library.
        oxpinyin_core::sort_tokens_by_library(&mut tokens);
        tokens.into_iter().map(PhraseToken::new).collect()
    }

    /// [`Self::tokens_for_text`] over raw UCS-4 spans, including
    /// surrogate or above-Unicode words. The system and user phrase
    /// tables compare the stored raw keys; no `char` or `str` is built
    /// from a non-scalar.
    fn tokens_for_ucs4(&self, ucs4: &[u32]) -> Vec<PhraseToken> {
        let mut tokens: Vec<u32> = self.system.tokens_for_ucs4(ucs4).unwrap_or_default();
        tokens.extend(
            self.user
                .phrase_table_tokens_for_ucs4(ucs4)
                .unwrap_or_default(),
        );
        tokens.retain(|token| self.phrase_table_token_enabled(*token));
        oxpinyin_core::sort_tokens_by_library(&mut tokens);
        tokens.into_iter().map(PhraseToken::new).collect()
    }

    fn phrase_span_searches_are_observable(&self) -> bool {
        self.system.phrase_attachment_failed()
    }

    fn phrase_span_continues(&self, words: &[u32]) -> bool {
        !self.system.phrase_attachment_failed() || self.user.phrase_table_contains_ucs4(words)
    }

    fn lookup_addon(&self, syllables: &[Self::Syllable]) -> Result<Vec<PhraseEntry>, Self::Error> {
        let mut entries = Vec::new();
        self.search_addon_into(syllables, &mut entries, &mut Vec::new())?;
        Ok(entries)
    }

    fn search_addon_into(
        &self,
        syllables: &[Self::Syllable],
        out: &mut Vec<PhraseEntry>,
        row: &mut Vec<u8>,
    ) -> Result<bool, Self::Error> {
        self.addons.borrow().search_into(syllables, out, row)
    }

    /// `FacadeChewingTable2::search_suggestion`'s composition: the system
    /// table's walk, then the user table's, `reduce_tokens` order over
    /// the union (nibble ascending — system libraries 1–4 before the
    /// user library 7 — cursor order within a group). The library mask
    /// drops an unloaded library's tokens, as every other read here
    /// does; the addon facade is absent because the pin's suggestion
    /// facade does not consult it.
    fn suggest_extension_tokens(
        &self,
        syllables: &[Self::Syllable],
    ) -> Result<Vec<PhraseToken>, Self::Error> {
        Ok(self
            .checked_extension_tokens(syllables)?
            .unwrap_or_default())
    }

    fn checked_extension_tokens(
        &self,
        syllables: &[Self::Syllable],
    ) -> Result<Option<Vec<PhraseToken>>, Self::Error> {
        let Some(mut tokens) = self.system.checked_extension_tokens(syllables)? else {
            return Ok(None);
        };
        if self.user.overlong_extension_gate(syllables) {
            return Ok(None);
        }
        let user = self.user_lookup();
        self.user
            .note_pinyin_suggestion(|| chewing_query(syllables));
        tokens.extend(user.suggest_extensions(syllables));
        if self.library_mask.get() != 0 {
            tokens.retain(|token| self.library_visible(u32::from(library_of(*token))));
        }
        oxpinyin_core::sort_tokens_by_library(&mut tokens);
        Ok(Some(tokens.into_iter().map(PhraseToken::new).collect()))
    }

    /// The pin's `search_suggestion` gate (`chewing_large_table2_bdb.cpp:282`):
    /// the facade consults both the system and the user `ChewingLargeTable2`
    /// (`facade_chewing_table2.h` `search_suggestion`), so an over-long key in
    /// either `pinyin_index.bin` drives the abort. The system half is the
    /// walk's key range ([`SystemDictionary::overlong_extension_gate`]), the
    /// user half the keys `user_pinyin_index.bin` carried at open.
    fn overlong_extension_gate(&self, syllables: &[Self::Syllable]) -> bool {
        self.system.overlong_extension_gate(syllables)
            || self.user.overlong_extension_gate(syllables)
    }

    /// `_token_get_phrase` over the facade: the system libraries' item
    /// text, else the user store's phrase text (the user library's
    /// items live there). The library mask hides an unloaded library's
    /// token, matching the pin's freed sub-index.
    fn phrase_text_for_token(&self, token: u32) -> Option<PhraseText> {
        if !self.library_visible_token(token) {
            return None;
        }
        if let Some(text) = self
            .token_text(token)
            .or_else(|| self.system.phrase_text(token))
        {
            return Some(text);
        }
        let store = self.user;
        store.phrase(token).and_then(|phrase| phrase.phrase_text())
    }

    fn phrase_length_for_token(&self, token: u32) -> Option<usize> {
        if self.is_system_library(library_of(token)) {
            return self
                .live_items()
                .read(token, |item| item.phrase_length())
                .found();
        }
        if !self.library_visible(u32::from(library_of(token))) {
            return None;
        }
        self.user.phrase_length(token)
    }

    /// `get_phrase_item` over the facade (074a2219 `phrase_index.h:646-657`):
    /// the item a library mask hides is the pin's freed sub-index, an item
    /// the user store replaced or owns is read from there, and any other
    /// system item from the shipped chunk.
    fn phrase_item_pronunciations(
        &self,
        token: PhraseToken,
    ) -> Result<PhraseItemRead, Self::Error> {
        let value = token.value();
        if !self.library_visible_token(value) {
            return Ok(PhraseItemRead::NoItem);
        }
        if let Some(rows) = self
            .user
            .item_pronunciations(value)
            .map_err(|error| DictError::Parse(error.to_string()))?
        {
            return Ok(PhraseItemRead::Item(rows));
        }
        self.system.phrase_item_pronunciations(token)
    }
}

/// The bigram language model with the user-count overlay.
#[derive(Clone, Copy)]
pub struct RuntimeLm<'a> {
    inner: BigramLanguageModel<
        &'a oxpinyin_data::phrase_libraries::PhraseLibraries,
        &'a oxpinyin_data::lm::BigramTables,
        &'a Cell<u32>,
    >,
    user: &'a UserStore,
    addons: &'a RefCell<AddonSet>,
}

/// `m_user_bigram->load`'s answer to a failed read: no gram, which
/// `search_bigram2` ignores (`phrase_lookup.cpp:269-270` at 074a2219), so
/// the system gram then stands alone (`ngram.cpp:292-296`). The checked-read
/// failure is the pin's assert and stays an error.
fn user_gram_or_none(
    read: Result<Option<Vec<u8>>, UserStoreError>,
) -> Result<Option<Vec<u8>>, LmError> {
    match read {
        Err(error) if error.is_read_after_check() => Err(LmError::User(error.to_string())),
        Err(_) => Ok(None),
        Ok(row) => Ok(row),
    }
}

impl RuntimeLm<'_> {
    fn delta(&self, prev: Option<u32>, token: u32) -> Result<UserCountDelta, LmError> {
        let mut delta = self
            .user
            .count_delta(prev, token)
            .map_err(|error| LmError::User(error.to_string()))?;
        delta.unigram_total_delta = self
            .user
            .unigram_total_over(|nibble| self.inner.is_visible(nibble));
        Ok(delta)
    }

    /// `prev`'s merged row: the system gram and the user gram, one point
    /// read each, merged as the pin's lookups merge them
    /// ([`BigramLanguageModel::load_merged_row`]). The user side is the
    /// stored value itself (`bigram_row`), `None` without a key.
    fn merged_row(&self, prev: u32) -> Result<Option<MergedGram>, LmError> {
        let user = user_gram_or_none(self.user.bigram_row(prev))?;
        self.inner.load_merged_row(prev, user)
    }

    /// The phrase-index total the pin's amplified law divides by
    /// (`pinyin.cpp:1813-1814`, `get_phrase_index_total_freq`, live per
    /// call): the visible libraries' stored totals, the user store's extra
    /// (including unapplied increments), wrapped at `guint32`. Must
    /// not be snapshotted — training changes it.
    #[must_use]
    pub fn amplified_total(&self) -> u64 {
        <Self as LanguageModel>::unigram_total(self)
            .ok()
            .flatten()
            .unwrap_or(0)
    }
}

impl LanguageModel for RuntimeLm<'_> {
    type Token = PhraseToken;
    type Error = LmError;

    fn amplification_lambda(&self) -> f32 {
        self.inner.amplification_lambda()
    }

    /// Upstream's Gate 2 (`pinyin.cpp:2210-2215`): the system gram and the
    /// user gram loaded once each and merged, as ONE row the caller indexes
    /// per candidate. The n-best and phrase paths load the same row.
    fn merged_successors(&self, prev: &Self::Token) -> Result<Option<MergedGram>, Self::Error> {
        self.merged_row(prev.value())
    }

    fn is_read_after_check(&self, error: &Self::Error) -> bool {
        error.is_read_after_check()
    }

    fn unigram_freq(&self, token: &Self::Token) -> Result<Option<u64>, Self::Error> {
        let live = self.user.system_unigram(token.value());
        let extra = if live.is_some() && self.inner.is_visible(library_of(token.value())) {
            // The model uses only the live field on this branch.
            0
        } else {
            self.user.unigram_delta(token.value())
        };
        Ok(Some(self.inner.unigram_freq_with_user_delta(
            token.value(),
            live,
            extra,
        )))
    }

    fn unigram_total(&self) -> Result<Option<u64>, Self::Error> {
        let user_total = self
            .user
            .unigram_total_over(|nibble| self.inner.is_visible(nibble));
        Ok(Some(self.inner.facade_total(user_total)))
    }

    fn addon_unigram_freq(&self, token: &Self::Token) -> Result<Option<u64>, Self::Error> {
        let addons = self.addons.borrow();
        if addons.is_empty() {
            return Ok(None);
        }
        Ok(Some(addons.unigram_freq(token.value()).unwrap_or(0)))
    }

    fn addon_unigram_total(&self) -> Result<Option<u64>, Self::Error> {
        let addons = self.addons.borrow();
        Ok(addons.unigram_total())
    }

    /// The §5 overlay the n-best step costs take.
    fn nbest_step_costs(
        &self,
        prev: &Self::Token,
        token: &Self::Token,
    ) -> Result<NbestStepCosts, Self::Error> {
        let extra = self.delta(Some(prev.value()), token.value())?;
        self.inner
            .nbest_step_costs_with_user_delta(prev, token, extra)
    }

    fn nbest_item_costs_with_facts(
        &self,
        prev: &Self::Token,
        token: &Self::Token,
        item: Option<(u32, u32)>,
        row: &mut oxpinyin_core::NbestStepRow,
        facts: &mut oxpinyin_core::sentence_facts::SentenceFacts,
    ) -> Result<NbestStepCosts, Self::Error> {
        if let Some(costs) = self.costs_with_facts(token, item, facts) {
            return costs;
        }
        self.nbest_item_costs_in_row(prev, token, item, row)
    }

    fn nbest_item_costs_in_row(
        &self,
        _prev: &Self::Token,
        token: &Self::Token,
        item: Option<(u32, u32)>,
        _row: &mut oxpinyin_core::NbestStepRow,
    ) -> Result<NbestStepCosts, Self::Error> {
        let extra = self.delta(None, token.value())?;
        self.inner.nbest_item_costs(token, extra, item)
    }

    fn nbest_successors_in_row<'a>(
        &self,
        prev: &Self::Token,
        row: &'a mut oxpinyin_core::NbestStepRow,
    ) -> Result<Option<oxpinyin_core::NbestSuccessors<'a>>, Self::Error> {
        Ok(Some(
            row.get_or_try_init(|| self.merged_row(prev.value()))?
                .map_or(
                    oxpinyin_core::NbestSuccessors::Miss,
                    oxpinyin_core::NbestSuccessors::Row,
                ),
        ))
    }
}

// ── Runtime ─────────────────────────────────────────────────────────────

/// A [`Session`] over this crate's concrete backends.
pub type RuntimeSession<'a> =
    Session<RuntimeDict<'a>, RuntimeLm<'a>, &'a mut oxpinyin_engine::SessionState>;

/// One opened engine: configuration-independent wiring for the converted
/// system tables, the unigram model, λ, the learning store, and the
/// merged backends sessions decode over.
///
/// Storage has one owner. [`Runtime::dict`], [`Runtime::lm`] and
/// [`Runtime::user_store`] borrow it for an operation; writers require
/// exclusive access. [`Runtime::new_session`] retains composition only.
/// Every successful open owns a store, including when the user directory
/// is absent: that store is transient, and its directory flag controls
/// training and saving.
pub struct Runtime {
    paths: StoragePaths,
    system: oxpinyin_data::dict::SystemTables,
    model: oxpinyin_data::lm::BigramTables,
    user: UserStore,
    conf: TableConf,
    addons: RefCell<AddonSet>,
    punct: PunctTable,
    library_mask: Cell<u32>,
}

impl Runtime {
    /// Perform context fini effects once: write the marker and close every
    /// native file attachment, as the pin's fini deletes its tables. The
    /// in-memory state stays for instances and iterators that outlive the
    /// context; their reads through a closed attachment are misses with no
    /// native call (compatibility row 42).
    pub fn finish(&mut self) {
        // pinyin.cpp:1200 writes the marker before deleting the tables.
        self.user.finish();
        // pinyin.cpp:1205-1215 and zhuyin.cpp:744-748: the open files and
        // the failed objects are closed in the pin's member order on every
        // backend; a second finish finds them gone.
        self.system.finish_attachments();
        self.model.finish_attachment();
        self.addons.borrow_mut().dict.finish_attachments();
        self.punct.finish_attachment();
    }

    /// Whether [`Self::finish`] has closed every native file attachment.
    #[must_use]
    pub fn attachments_finished(&self) -> bool {
        self.system.attachments_finished()
            && self.model.attachment_finished()
            && self.addons.borrow().dict.attachments_finished()
            && self.punct.attachment_finished()
    }

    /// Whether the main pinyin index is a retained failed attachment.
    #[must_use]
    pub fn pinyin_attachment_failed(&self) -> bool {
        self.system.pinyin_attachment_failed()
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        self.finish();
    }
}

/// `check_format` where the pin runs it: right after `table.conf` loads,
/// before any table or library (`pinyin.cpp:337-346`, `zhuyin.cpp:281-292`),
/// so an init that fails later on a library has already written the
/// profile's diagnostic, written the marker and wiped what it wipes, as the
/// pin has when it dies. Without a user dir the pin judges the marker it
/// names `""`.
///
/// The marker save's answer is ignored, as the pin ignores it
/// (`pinyin.cpp:187`; maintainer ruling 2026-10-28 UTC, D4), so the only
/// failure is the database-format abort point.
fn check_profile_early(
    conf: &TableConf,
    user_dir: Option<&Path>,
    law: UserConfLaw,
) -> Result<Option<oxpinyin_user::ProfileCheck>, OpenError> {
    let versions = SystemVersions::from_conf(conf);
    let Some(user_dir) = user_dir else {
        UserStore::check_profile_unnamed(&versions, law);
        return Ok(None);
    };
    let layout = UserFileLayout::from_conf(conf);
    match UserStore::check_profile(user_dir, &versions, &layout, law) {
        Ok(check) => Ok(Some(check)),
        Err(UserStoreError::UnknownDatabaseFormat) => {
            Err(OpenError::UnknownDatabaseFormat(user_dir.to_path_buf()))
        }
        Err(
            error @ (UserStoreError::Io(_)
            | UserStoreError::Store(_)
            | UserStoreError::Decode
            | UserStoreError::InvalidPhrase
            | UserStoreError::Persistence(_)
            | UserStoreError::ChunkHeaderWrite(_)
            | UserStoreError::LibraryChunkHeaderRead
            | UserStoreError::SaveRemapFailed
            | UserStoreError::SaveTooLarge
            | UserStoreError::UnigramTotalOverflow
            | UserStoreError::OverlongIndexKey
            | UserStoreError::NonTokenUserBigramKey
            | UserStoreError::ShortUserBigramValue
            | UserStoreError::PartialUserBigramTail
            | UserStoreError::ResidualUserBigramGram
            | UserStoreError::Journal(_)
            | UserStoreError::LibraryLoop(_)
            | UserStoreError::TrainingSetFreqMiss
            | UserStoreError::UserReadingRemoval
            | UserStoreError::SessionSeed(_)
            | UserStoreError::FailedUserBigramStore),
        ) => Err(OpenError::Dict(DictError::Parse(error.to_string()))),
        // A failed record copy is the pin's assert (`tkrzwdb_utils.h:65`,
        // `:72`; `bdb_utils.h:61`, `:67`), which fails the open as
        // `open_user_store` fails it; `check_format` does not copy, so it
        // never reaches here.
        Err(error @ UserStoreError::FatalCopy) => Err(OpenError::UserSession(error)),
    }
}

/// Opens the user dir's session over the libraries the loop maps:
/// `check_format`'s `user.conf` half has already run
/// ([`check_profile_early`]).
///
/// A failure is never a degraded open. A `user.conf` whose `database
/// format:` upstream cannot map is where `check_format` `abort()`s
/// (`table_info.cpp:132`): [`OpenError::UnknownDatabaseFormat`]. The loop's
/// first fault is the pin's. Anything else fails after the loop handed the
/// session the system libraries, where the pin dies
/// ([`OpenError::UserSession`]).
fn open_user_store(
    conf: &TableConf,
    user_dir: &Path,
    originals: &mut oxpinyin_user::SystemOriginals,
    rows: &LibraryLoopRows<'_>,
    law: UserConfLaw,
    check: oxpinyin_user::ProfileCheck,
) -> Result<UserStore, OpenError> {
    let versions = SystemVersions::from_conf(conf);
    UserStore::open_libpinyin_checked(user_dir, originals, rows, versions, law, check).map_err(
        |error| match error {
            UserStoreError::UnknownDatabaseFormat => {
                OpenError::UnknownDatabaseFormat(user_dir.to_path_buf())
            }
            UserStoreError::LibraryLoop(stop) => library_loop_error(stop),
            error @ (UserStoreError::Io(_)
            | UserStoreError::Store(_)
            | UserStoreError::Decode
            | UserStoreError::InvalidPhrase
            | UserStoreError::Persistence(_)
            | UserStoreError::ChunkHeaderWrite(_)
            | UserStoreError::LibraryChunkHeaderRead
            | UserStoreError::SaveRemapFailed
            | UserStoreError::SaveTooLarge
            | UserStoreError::UnigramTotalOverflow
            | UserStoreError::OverlongIndexKey
            | UserStoreError::NonTokenUserBigramKey
            | UserStoreError::ShortUserBigramValue
            | UserStoreError::PartialUserBigramTail
            | UserStoreError::ResidualUserBigramGram
            | UserStoreError::Journal(_)
            | UserStoreError::TrainingSetFreqMiss
            | UserStoreError::UserReadingRemoval
            | UserStoreError::SessionSeed(_)
            | UserStoreError::FailedUserBigramStore
            | UserStoreError::FatalCopy) => OpenError::UserSession(error),
        },
    )
}

/// The library loop's fault, as the open's answer.
fn library_loop_error(stop: LibraryLoopStop) -> OpenError {
    match stop {
        LibraryLoopStop::Row(site) => OpenError::TableConf(TableConfError::Abort(site)),
        LibraryLoopStop::Library(error) => OpenError::Dict(DictError::Library(error)),
        LibraryLoopStop::Journal(fault) => OpenError::Journal(fault),
    }
}

impl Runtime {
    /// Opens a system data directory the way `pinyin_init` does.
    ///
    /// `system_dir` holds `table.conf`, the compiled-in backend's DBMs
    /// (`SystemDbm::file_name` — libpinyin's own names on Kyoto Cabinet,
    /// tkrzw and Berkeley DB), the per-library chunk files, and optionally
    /// `punct.bin` and the addon DBM pair — the backend's native data
    /// directory, opened as is.
    /// When `user_dir` is given, the learning store opens that profile (an
    /// unreadable or partial profile loads per file as the pin's does, so a
    /// bad user dir cannot fail init; a tkrzw or Berkeley DB copy that fails
    /// part way fails the open, where the pin's copy asserts and the C
    /// adapters abort), under libpinyin's
    /// `user.conf` law ([`UserConfLaw::Pinyin`]). An empty `user_dir` is
    /// given, not absent: it is the working directory, as `pinyin_init`'s
    /// `""` is, and stays relative for the life of the store.
    /// Without a user directory the store is transient; imports and lookup
    /// still use it, while training and saving refuse.
    ///
    /// Nothing is read beyond the handles: the DBMs are opened, the chunk
    /// files mapped and checksummed, `table.conf` scanned for its rows,
    /// versions and λ.
    ///
    /// # Errors
    ///
    /// Returns [`OpenError`] when `table.conf` is one `SystemTableInfo2::load`
    /// refuses, missing included (maintainer ruling 2026-10-28 UTC, D3), or
    /// when a required file is missing or a DBM or chunk file cannot be
    /// opened, and [`OpenError::UserSession`] where the user session cannot
    /// be built after the libraries were handed to it.
    pub fn open(system_dir: &Path, user_dir: Option<&Path>) -> Result<Self, OpenError> {
        Self::open_with_law(system_dir, user_dir, UserConfLaw::Pinyin)
    }

    /// [`Runtime::open`] with the user dir's `user.conf` law named: the
    /// pinyin facade's init passes [`UserConfLaw::Pinyin`], the zhuyin
    /// facade's [`UserConfLaw::Zhuyin`] — the two pins share the data
    /// directory and the user file set but not the open counter's
    /// lifecycle (`oxpinyin_user::persistence::UserConfLaw`). The law also
    /// selects the facade's system tables: only pinyin opens the addon
    /// indexes and punctuation table (074a2219 `pinyin.cpp:416-440`;
    /// `zhuyin.cpp:270-358`).
    ///
    /// # Errors
    ///
    /// As [`Runtime::open`].
    pub fn open_with_law(
        system_dir: &Path,
        user_dir: Option<&Path>,
        law: UserConfLaw,
    ) -> Result<Self, OpenError> {
        Self::open_with_law_reporting(system_dir, user_dir, law, &mut |_| {})
    }

    /// [`Runtime::open_with_law`], calling `unmapped` with each default
    /// library whose system chunk is absent, in index order, as the pin's
    /// init loop reaches it and writes `mmap %s failed!`
    /// (`pinyin.cpp:253-256`, `zhuyin.cpp:197-200`). Where the open stops
    /// at a default row the pin dies on, only the libraries the loop
    /// passed before that row are reported.
    ///
    /// # Errors
    ///
    /// As [`Runtime::open`].
    pub fn open_with_law_reporting(
        system_dir: &Path,
        user_dir: Option<&Path>,
        law: UserConfLaw,
        unmapped: &mut dyn FnMut(&Path),
    ) -> Result<Self, OpenError> {
        // `SystemTableInfo2::load`, the pin's first step: every refusal,
        // a missing file included, fails the init (`pinyin.cpp:335-340`,
        // `zhuyin.cpp:279-284`).
        let conf = TableConf::load_from_dir(system_dir).map_err(OpenError::TableConf)?;
        let early = check_profile_early(&conf, user_dir, law)?;
        // The pin attaches the system tables before its library loop
        // (`pinyin.cpp:350-373`, `zhuyin.cpp:294-316`).
        let dict = SystemDictionary::attach_tables(system_dir, &conf).map_err(OpenError::Dict)?;
        // The library mask is shared with the language model: an unloaded
        // library's items leave both the lookups and the unigram denominator,
        // as freeing the sub-index does upstream.
        let library_mask = Cell::new(0);
        let mut originals = oxpinyin_user::system_originals(
            oxpinyin_data::phrase_libraries::PhraseLibraries::empty(),
            UserFileLayout::from_conf(&conf),
        );
        // The library loop: each default row is judged, mapped and merged
        // before the next (`pinyin.cpp:377-392`, `zhuyin.cpp:318-334`), after
        // `check_format`'s writes (`pinyin.cpp:344`), so the first fault is
        // the pin's and nothing past it is touched.
        //
        // `None` has transient user state without a directory (#642). An
        // empty path is a user directory like any other: the pin keeps
        // `g_strdup("")` (`pinyin.cpp:332`, `zhuyin.cpp:276`) and
        // `g_build_filename` drops the empty element, so its user files
        // are bare names in the working directory (#619). `Path::join`
        // on an empty path gives the same bare names, and the store
        // keeps the path as given, so each file resolves against the
        // directory current when it is opened, as the pin's does. An
        // unusable directory must not fail init either — training then
        // refuses, upstream-style. The store persists in libpinyin's own
        // user-dir file set (drop-in task 9): the profile is read through
        // check_format, the session runs in memory, and `pinyin_save`
        // writes the pin's files — a same-backend libpinyin picks them
        // up seamlessly.
        let rows = LibraryLoopRows::new(system_dir, &conf, user_dir);
        let opened = match (user_dir, early) {
            (Some(dir), Some(check)) => {
                open_user_store(&conf, dir, &mut originals, &rows, law, check)
            }
            _ => rows
                .map_libraries(&mut originals)
                .map_err(library_loop_error)
                .and_then(|()| {
                    UserStore::open_transient(
                        std::mem::replace(
                            &mut originals,
                            oxpinyin_user::SystemOriginals::new(
                                Default::default(),
                                UserFileLayout::from_conf(&conf),
                            ),
                        ),
                        SystemVersions::from_conf(&conf),
                        law,
                    )
                    .map_err(OpenError::UserSession)
                }),
        };
        // The one exit from the loop. The pin wrote each absent library's
        // line as it reached it (`pinyin.cpp:253-256` within `:377-392`); the
        // list holds only the libraries this loop reached, and none of them
        // stopped it.
        let user = match opened {
            Ok(user) => user,
            Err(error) => {
                for path in originals.phrase_libraries().unmapped() {
                    unmapped(path);
                }
                return Err(error);
            }
        };
        for path in user.originals().phrase_libraries().unmapped() {
            unmapped(path);
        }

        // The bigram follows the library loop (`pinyin.cpp:394-397`,
        // `zhuyin.cpp:336-339`).
        let bigram = oxpinyin_input::path::build_path(
            system_dir,
            std::ffi::OsStr::new(&SystemDbm::Bigram.file_name()),
        );
        let mut lm = oxpinyin_data::lm::BigramTables::attach(&bigram).map_err(OpenError::Lm)?;
        // λ is the `gfloat` table.conf's scanner read.
        lm.set_lambda_from_conf(&conf);

        let addons = RefCell::new(AddonSet {
            dict: match law {
                UserConfLaw::Pinyin => {
                    AddonDictionary::attach_with(system_dir, &conf).map_err(OpenError::Dict)?
                }
                UserConfLaw::Zhuyin => AddonDictionary::without_indexes(&conf),
            },
        });
        let punct = match law {
            UserConfLaw::Pinyin => {
                PunctTable::attach(&system_dir.join(SystemDbm::Punct.file_name()))
                    .map_err(OpenError::Dict)?
            }
            UserConfLaw::Zhuyin => PunctTable::empty(),
        };

        Ok(Self {
            paths: StoragePaths::new(vec![system_dir.to_path_buf()]),
            system: dict,
            model: lm,
            user,
            conf,
            addons,
            punct,
            library_mask,
        })
    }

    fn libraries(&self) -> &oxpinyin_data::phrase_libraries::PhraseLibraries {
        self.user.originals().phrase_libraries()
    }

    fn originals_mut(&mut self) -> &mut oxpinyin_user::SystemOriginals {
        self.user.originals_mut()
    }

    /// The system `table.conf` this runtime opened (the stock layout for a
    /// data directory without one): its rows decide which libraries exist,
    /// what they are called, and which calls the pin accepts for them.
    #[must_use]
    pub fn table_conf(&self) -> &TableConf {
        &self.conf
    }

    /// Builds retained composition state for this backend set. Operations
    /// bind it to current readers through [`Self::session`].
    ///
    /// Settings arrive per call (the facade passes its context's; simple
    /// embedders pass [`SessionSettings::default`]), because the session
    /// reads them once at construction.
    ///
    /// Construction cannot fail: [`Session::new`] reads no dictionary or
    /// model record.
    pub fn new_session(&self, settings: SessionSettings) -> oxpinyin_engine::SessionState {
        Session::new(settings, self.dict(), self.lm()).into_state()
    }

    /// Borrow the merged dictionary for one operation.
    #[must_use]
    pub fn dict(&self) -> RuntimeDict<'_> {
        RuntimeDict {
            system: self.system.read(self.libraries()),
            user: &self.user,
            addons: &self.addons,
            punct: &self.punct,
            library_mask: &self.library_mask,
        }
    }

    /// Borrow the overlaid language model for one operation.
    #[must_use]
    pub fn lm(&self) -> RuntimeLm<'_> {
        RuntimeLm {
            inner: self.model.read(self.libraries(), &self.library_mask),
            user: &self.user,
            addons: &self.addons,
        }
    }

    /// Borrow composition and phrase storage for one operation.
    pub fn session<'a>(
        &'a self,
        state: &'a mut oxpinyin_engine::SessionState,
    ) -> RuntimeSession<'a> {
        state.bind(self.dict(), self.lm())
    }

    /// Borrow the learning store without retaining an independent owner.
    #[must_use]
    pub fn user_store(&self) -> &UserStore {
        &self.user
    }

    /// Check training with operation-local bookkeeping, then release all phrase
    /// readers before applying the observations to the sole store owner.
    ///
    /// # Errors
    /// The engine's ordered training errors; completed observations stay applied.
    pub fn train_session(
        &mut self,
        state: &oxpinyin_engine::SessionState,
        index: u8,
    ) -> Result<bool, EngineError> {
        let user = &self.user;
        if !user.has_user_directory() {
            return Ok(false);
        }
        let mut check = oxpinyin_user::TrainingCheck::default();
        let prepared =
            state
                .read(self.dict(), self.lm())
                .prepare_training(index, &mut |history, token| {
                    user.check_training_step_with(
                        &mut check,
                        history
                            .last()
                            .map_or(oxpinyin_user::SENTENCE_START, |token| token.value()),
                        token.value(),
                    )
                    .map_err(|error| EngineError::UserModel(error.to_string()))
                });
        let user = &mut self.user;
        let result = match prepared {
            Ok(Some(plan)) => plan.observe(user),
            Ok(None) => Ok(false),
            Err(EngineError::UserModel(error)) => match check.fault() {
                Some(oxpinyin_user::TrainingFault::ShortValue(token)) => {
                    Err(EngineError::ShortTrainingPredecessor { token })
                }
                Some(oxpinyin_user::TrainingFault::PartialTail(token)) => {
                    Err(EngineError::UnsafeTrainingPredecessor { token })
                }
                Some(oxpinyin_user::TrainingFault::SetFreqMiss(token)) => {
                    Err(EngineError::TrainingSetFreqMiss { token })
                }
                Some(oxpinyin_user::TrainingFault::FailedUserBigramStore(token)) => {
                    Err(EngineError::FailedUserBigramStore { token })
                }
                None => Err(EngineError::UserModel(error)),
            },
            Err(
                error @ (EngineError::CandidateIndexOutOfRange { .. }
                | EngineError::LookupOffsetPastSeparator { .. }
                | EngineError::LookupOffsetOutOfRange { .. }
                | EngineError::LookupOffsetInsideCharacter { .. }
                | EngineError::SelectionAnchorBeforeComposition { .. }
                | EngineError::ZeroKeyOffsetCheck { .. }
                | EngineError::MatrixColumnAssert { .. }
                | EngineError::StaleTrainingConstraint { .. }
                | EngineError::UnsafeTrainingPredecessor { .. }
                | EngineError::ShortTrainingPredecessor { .. }
                | EngineError::ShortLookupPredecessor { .. }
                | EngineError::UnsafeLookupPredecessor { .. }
                | EngineError::ReadAfterCheck { .. }
                | EngineError::TrainingSetFreqMiss { .. }
                | EngineError::FailedUserBigramStore { .. }
                | EngineError::StaleTrainingSpan { .. }
                | EngineError::OverlongUserIndexKey { .. }
                | EngineError::TonedIncompleteKey
                | EngineError::ReduceTokensCount { .. }
                | EngineError::Graph(_)
                | EngineError::Scoring(_)),
            ) => Err(error),
        };
        if !matches!(
            &result,
            Ok(false)
                | Err(EngineError::CandidateIndexOutOfRange { .. }
                    | EngineError::StaleTrainingConstraint { .. }
                    | EngineError::StaleTrainingSpan { .. }
                    | EngineError::UnsafeTrainingPredecessor { .. }
                    | EngineError::ShortTrainingPredecessor { .. }
                    | EngineError::TrainingSetFreqMiss { .. }
                    | EngineError::FailedUserBigramStore { .. })
        ) {
            user.mark_modified();
        }
        result
    }

    /// Apply a unigram increment with exclusive access to the storage owner.
    #[must_use]
    pub fn add_unigram_delta(&mut self, token: u32, delta: u64) -> bool {
        self.add_unigram_outcome(token, delta).is_accepted()
    }

    /// [`Self::add_unigram_delta`] with the answer in the pin's terms. A
    /// caller that acts on `ERROR_INTEGER_OVERFLOW` alone, as the choose legs
    /// do (`pinyin.cpp:2527-2531`, `:2567-2575`), tells [`UnigramAdd::Refused`]
    /// from an unloaded library or a missing item, which are
    /// [`UnigramAdd::Missing`].
    #[must_use]
    pub fn add_unigram_outcome(&mut self, token: u32, delta: u64) -> UnigramAdd {
        // PHRASE_INDEX_LIBRARY_INDEX and PHRASE_MASK ignore the tag nibble
        // (074a2219 novel_types.h:41-44, phrase_index.cpp:154).
        let token = token & (oxpinyin_user::PHRASE_INDEX_LIBRARY_MASK | oxpinyin_user::PHRASE_MASK);
        if !self.dict().library_visible(u32::from(library_of(token))) {
            return UnigramAdd::Missing;
        }
        let Ok(delta) = u32::try_from(delta) else {
            return UnigramAdd::Missing;
        };
        self.user.add_unigram_outcome(token, delta)
    }

    /// Publish dictionary remaps while retaining any unreplaced journal originals.
    pub fn install_baselines(&mut self, originals: oxpinyin_user::SystemOriginals) {
        self.originals_mut().publish_current(originals);
    }

    /// Borrow the learning store exclusively for a mutation.
    pub fn user_store_mut(&mut self) -> &mut UserStore {
        &mut self.user
    }

    /// Loads addon library `index` from the system directory; `Ok(false)`
    /// when the runtime has no system directory or the library is already
    /// loaded.
    ///
    /// # Errors
    ///
    /// [`LibraryError`] when the library file is absent or does not verify.
    pub fn load_system_addon_reporting(&mut self, index: u8) -> Result<bool, LibraryError> {
        let Some(system_dir) = self.paths.system_data_dirs().first() else {
            return Ok(false);
        };
        self.dict().load_addon_reporting(index, system_dir)
    }

    /// The system directory the libraries load from, when there is one.
    #[must_use]
    pub fn system_dir(&self) -> Option<&Path> {
        self.paths.system_data_dirs().first().map(PathBuf::as_path)
    }

    /// One validation pass over the system files this operation remaps.
    /// Save and mask skip slot zero, absent sub-indices, unused rows and
    /// rows without a user filename (`pinyin.cpp:927-943`, `:1235-1251`).
    /// An explicit reload still checks its target, including slot zero.
    #[must_use]
    pub fn check_system_libraries(
        &self,
        only: Option<u8>,
    ) -> oxpinyin_data::phrase_libraries::LibraryCheck {
        self.libraries().check_now(
            |nibble| {
                only.is_some()
                    || (nibble != 0
                        && self.dict().library_visible(u32::from(nibble))
                        && self.conf.default_row(nibble).is_some_and(|row| {
                            row.user.is_some()
                                && matches!(
                                    row.file_type,
                                    FileType::SystemFile | FileType::Dictionary
                                )
                        }))
            },
            only,
        )
    }

    /// `FacadePhraseIndex::compact` over the system layout-failed empty
    /// sub-indices that are loaded: an unloaded library has no sub-index to
    /// compact (`phrase_index.cpp:651-652`). Returns the compacted nibbles.
    #[must_use]
    pub fn compact_layout_failed(&mut self) -> Vec<u8> {
        let unloaded = self.unloaded_libraries();
        self.originals_mut()
            .phrase_libraries_mut()
            .compact_layout_failed(unloaded)
    }

    /// The current loaded-library mask, snapshotted for an exclusive save.
    pub fn unloaded_libraries(&self) -> u32 {
        self.library_mask.get()
    }

    /// Read a save baseline while its unique owner performs the operation.
    ///
    /// # Errors
    /// The remap faults reported by `save_disk_baseline`.
    pub fn read_save_baseline(
        originals: &oxpinyin_user::SystemOriginals,
        unloaded: u32,
        nibble: u8,
    ) -> Result<SaveBaseline, SaveRemapFault> {
        // Nibble 0 is the reserved row the loop never visits.
        if nibble == 0 {
            return Ok(SaveBaseline::Keep);
        }
        // `_write_files` skips a default library whose sub-index is gone
        // (`pinyin.cpp:928-933`, `zhuyin.cpp:563-568`), a `USER_FILE`
        // library included: an unloaded library is neither remapped nor
        // written, and its file on disk stays.
        if unloaded & (1 << nibble) != 0 {
            return Ok(SaveBaseline::Unloaded);
        }
        let Some((path, _)) = originals.phrase_libraries().loaded_path_library(nibble) else {
            return Ok(SaveBaseline::Keep);
        };
        match oxpinyin_data::phrase_library::PhraseLibrary::open_retaining_layout(path) {
            Ok(current) => Ok(SaveBaseline::Changed(SystemLibrary::from_library(current))),
            Err(LibraryError::Row(RowFault::ChunkHeaderRead)) => {
                Err(SaveRemapFault::ChunkHeaderRead)
            }
            Err(
                error @ (LibraryError::Io(_)
                | LibraryError::Format(_)
                | LibraryError::Layout(_)
                | LibraryError::Unmappable { .. }
                | LibraryError::Row(RowFault::NotDictionary | RowFault::NullChunkName)),
            ) => match error.unmappable_path() {
                Some(path) => Err(SaveRemapFault::Unmappable(path.to_path_buf())),
                None => Ok(SaveBaseline::Keep),
            },
        }
    }

    /// Maps every loaded SYSTEM_FILE/DICTIONARY default library again for
    /// mask_out, in the mask loop's order, replacing the shared sub-index,
    /// never the phrase DBM. Read from the 074a2219 tree: pinyin.cpp:1253-1273,
    /// zhuyin.cpp:788-808. Unloaded libraries and rows without a user file
    /// are skipped. A file `MemoryChunk::mmap` refuses keeps the existing
    /// safe fallback and is listed in [`SystemRemap::unmappable`] with its
    /// sub-index: the pin writes its `mmap %s failed!` line (row 76) at that
    /// library's place in the loop, so the caller writes it only if the loop
    /// reached it. A library that is a directory whose header read the pin
    /// asserts on (`memory_chunk.h:493`, register row 96) ends the walk
    /// ([`SystemRemap::header_fault`]), and the originals hold the libraries
    /// mapped before it, whose logs the pin merged before it died.
    #[must_use]
    pub fn reload_system_libraries(&mut self) -> SystemRemap {
        self.remap_system_libraries(1..16, false)
    }

    /// Maps the unloaded SYSTEM_FILE/DICTIONARY default library `index`
    /// again, as `_load_phrase_library` does before it merges the saved
    /// `.dbin` (074a2219 `pinyin.cpp:246-264`, `zhuyin.cpp:298-312`). The
    /// result is empty for a row with no system chunk (the `USER_FILE` row,
    /// the unused rows) and for an unreadable file. A row that names no user
    /// file is mapped like any other: `_load_phrase_library` reads the user
    /// name only after it has mapped the chunk (`pinyin.cpp:266`), where the
    /// mask loop skips such a row (`:1250-1251`).
    /// Call it before [`Runtime::load_library`] makes the library visible,
    /// so no reader sees the session's state of the freed sub-index.
    #[must_use]
    pub fn reload_system_library(&mut self, index: u8) -> oxpinyin_user::SystemOriginals {
        self.remap_system_libraries(index..index.saturating_add(1), true)
            .originals
    }

    /// Starts the mask loop's remap over every loaded SYSTEM_FILE/DICTIONARY
    /// default library, in index order (074a2219 pinyin.cpp:1253-1273,
    /// zhuyin.cpp:788-808). Advance it with [`Self::remap_next`] and end it
    /// with [`Self::finish_system_remap`].
    #[must_use]
    pub fn begin_system_remap(&self) -> RemapWalk {
        self.begin_remap(1..16, false)
    }

    fn begin_remap(&self, indices: std::ops::Range<u8>, reload: bool) -> RemapWalk {
        let layout = UserFileLayout::from_conf(&self.conf);
        let indices = if self.paths.system_data_dirs().is_empty() {
            0..0
        } else {
            indices
        };
        RemapWalk {
            originals: oxpinyin_user::SystemOriginals::new(Default::default(), layout),
            indices,
            reload,
        }
    }

    /// Maps the walk's next library and says what it reached; `None` when
    /// the walk is over. A directory ends it. Nothing after the library
    /// returned is opened until the next call, so a caller that stops there
    /// leaves every later library untouched.
    pub fn remap_next(&self, walk: &mut RemapWalk) -> Option<RemapStep> {
        let dir = self.paths.system_data_dirs().first()?;
        let libraries = self.libraries();
        while let Some(index) = walk.indices.next() {
            if !(1..16).contains(&index) {
                continue;
            }
            let Some(row) = self.conf.default_row(index) else {
                continue;
            };
            // The mask loop skips a library that is not loaded
            // (`pinyin.cpp:1240-1241`) and a row that names no user file
            // (`:1250-1251`). A reload does neither: it maps the unloaded
            // library, and the user name is not read until it has.
            if !matches!(row.file_type, FileType::SystemFile | FileType::Dictionary)
                || (!walk.reload
                    && (row.user.is_none() || !self.dict().library_visible(u32::from(index))))
            {
                continue;
            }
            let Some(name) = row.system.as_ref() else {
                continue;
            };
            let path = oxpinyin_input::path::build_chunk_path(dir, name.as_ref());
            if !libraries.is_loaded(index) {
                continue;
            }
            match oxpinyin_data::phrase_library::PhraseLibrary::open_retaining_layout(&path) {
                Ok(library) => {
                    walk.originals.insert_mapping(index, path, library);
                    return Some(RemapStep::Mapped(index));
                }
                Err(LibraryError::Row(RowFault::ChunkHeaderRead)) => {
                    walk.indices = 0..0;
                    return Some(RemapStep::Directory(index));
                }
                Err(
                    error @ (LibraryError::Io(_)
                    | LibraryError::Format(_)
                    | LibraryError::Layout(_)
                    | LibraryError::Unmappable { .. }
                    | LibraryError::Row(RowFault::NotDictionary | RowFault::NullChunkName)),
                ) => {
                    if let Some(path) = error.unmappable_path() {
                        return Some(RemapStep::Unmappable(index, path.to_path_buf()));
                    }
                }
            }
        }
        None
    }

    /// Ends a remap walk and returns the libraries it mapped.
    #[must_use]
    pub fn finish_system_remap(&mut self, walk: RemapWalk) -> oxpinyin_user::SystemOriginals {
        walk.originals
    }

    fn remap_system_libraries(
        &mut self,
        indices: std::ops::Range<u8>,
        reload: bool,
    ) -> SystemRemap {
        let mut walk = self.begin_remap(indices, reload);
        let mut unmappable = Vec::new();
        let mut header_fault = None;
        while let Some(step) = self.remap_next(&mut walk) {
            match step {
                RemapStep::Mapped(_) => {}
                RemapStep::Unmappable(index, path) => unmappable.push((index, path)),
                RemapStep::Directory(index) => {
                    header_fault = Some(index);
                    break;
                }
            }
        }
        SystemRemap {
            originals: self.finish_system_remap(walk),
            unmappable,
            header_fault,
        }
    }

    /// Unloads addon library `index` from this runtime's dictionary.
    ///
    /// The bound the pin asserts on `index` is the caller's to apply; see
    /// `RuntimeDict::unload_addon`.
    #[must_use]
    pub fn unload_system_addon(&mut self, index: u8) -> bool {
        self.dict().unload_addon(index)
    }

    /// Loads default library `index` — the GBK-reload path after an
    /// unload; `false` when already visible.
    #[must_use]
    pub fn load_library(&mut self, index: u32) -> bool {
        let changed = self.dict().load_library(index);
        if index < PHRASE_INDEX_LIBRARY_COUNT {
            self.user.set_library_loaded(index as u8, true);
        }
        changed
    }

    /// Unloads default library `index`; `false` only outside the sixteen
    /// sub-indices, `true` on every call otherwise (the pin ignores what
    /// `unload` returns, `pinyin.cpp:473-474`). Which indices a facade lets
    /// through is the C ABI's rule.
    #[must_use]
    pub fn unload_library(&mut self, index: u32) -> bool {
        let accepted = self.dict().unload_library(index);
        if accepted {
            self.user.set_library_loaded(index as u8, false);
        }
        accepted
    }
}

#[cfg(test)]
mod tests;

/// The standalone pin test programs use the same data and model readers.
#[cfg(feature = "tools")]
pub mod tool;
