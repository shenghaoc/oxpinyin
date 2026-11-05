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
// making failure unreachable; see .kiro/steering/rust-conventions.md.
// An expectation proves lint coverage, not correspondence with the pin.
// Inline #[cfg(test)] modules are exempt; tests/, benches/ and examples/
// are separate crates.
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::panic))]
#![cfg_attr(not(test), deny(clippy::panic_in_result_fn))]
#![warn(missing_docs)]

mod sentence_facts;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// `PHRASE_INDEX_LIBRARY_COUNT` (`phrase_index.h`): sixteen sub-indices.
const PHRASE_INDEX_LIBRARY_COUNT: u32 = 16;
use std::sync::{Mutex, RwLock};

use oxpinyin_core::{
    Cost, Dictionary, LanguageModel, MergedGram, NbestStepCosts, PhraseEntry, PhraseIndexRanges,
    PhraseItemRead, PhraseText, PhraseToken, SyllableKey, TrellisItem, TrellisPathMemo,
    UserCountDelta,
};
pub use oxpinyin_data::phrase_library::{
    LayoutFault, LibraryError, PhraseItemView, RowFault, directory_reads_as_a_chunk,
};
pub use oxpinyin_data::table_info::{FileType, TableConfSite};
pub use oxpinyin_data::table_info::{TableConf, TableConfError};
use oxpinyin_data::user_files::{SystemVersions, UserFileLayout};
use oxpinyin_data::{
    AddonDictionary, BigramLanguageModel, DictError, LmError, PunctTable, SystemDbm,
    SystemDictionary,
};
use oxpinyin_engine::{ConfigSource, EngineError, Session, StoragePaths};
use oxpinyin_user::{
    ItemOrigin, LiveSystemItems, PinyinKey, SaveBaseline, SystemLibrary, UserConfLaw, UserLookup,
    UserStore, UserStoreError,
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
    merged.sort_by_key(|token| token >> 24);
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
    /// The class-(c) availability answer of
    /// `docs/findings/compatibility-policy.md`: the open fails and the C
    /// ABI answers NULL, where upstream takes the process down. Nothing
    /// is cleaned and no marker is written — upstream never reaches its
    /// wipe either.
    UnknownDatabaseFormat(PathBuf),
    /// The system `table.conf` is one the pin refuses or dies on
    /// (`SystemTableInfo2::load`, `table_info.cpp:194-294`, and the init
    /// loop's asserts, `pinyin.cpp:377-392`): [`TableConfError::Header`] is
    /// the pin's ordinary `false` return (`load %s failed!`, no warning),
    /// every other kind a class-(b)/(c) site the C ABI answers with NULL and
    /// one warning.
    TableConf(TableConfError),
    /// A `SYSTEM_FILE` library's user `.dbin` replay reached a pin
    /// assertion while the library loop merged it
    /// ([`oxpinyin_user::JournalFault`], rows 91 and 108): the open stops at
    /// that library, after `check_format`'s writes, and the C adapters abort.
    Journal(oxpinyin_user::JournalFault),
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
    include_unloaded: bool,
}

/// What [`Runtime::remap_next`] reached. A library the loop skips (not
/// loaded, no user file, or a chunk that fails some other check) is not a
/// step.
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

/// Why [`Runtime::save_disk_baseline`] could not give a baseline for a
/// system library the save maps again at its `_write_files` position
/// (`pinyin.cpp:955`, `zhuyin.cpp:588`).
#[derive(Debug)]
pub enum SaveRemapFault {
    /// The chunk passed the header and checksum, but `SubPhraseIndex::load`
    /// refuses its layout: the pin diffs against the empty sub-index.
    Layout(LayoutFault),
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
            | Self::Journal(_) => None,
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

    fn lookup_into(
        &self,
        syllables: &[SyllableKey],
        out: &mut Vec<PhraseEntry>,
    ) -> Result<(), DictError> {
        self.dict.lookup_into(syllables, out)
    }

    fn lookup_candidates_into(
        &self,
        syllables: &[SyllableKey],
        out: &mut Vec<PhraseEntry>,
    ) -> Result<(), DictError> {
        self.dict.lookup_candidates_into(syllables, out)
    }

    fn prefix_exists(&self, syllables: &[SyllableKey]) -> Result<bool, DictError> {
        self.dict.prefix_exists(syllables)
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

    /// The addon phrase item behind `token`: its text, its pronunciations as
    /// `(key sequence, count)` pairs, and its copied unigram frequency — the
    /// `get_phrase_item` half of the promotion (`pinyin.cpp:2534-2549`).
    ///
    /// `None` when no loaded addon library owns `token`. A pronunciation
    /// whose spelling does not map back to syllable keys is dropped, the same
    /// rule the reverse rendering applies.
    fn phrase_item(&self, token: u32) -> Option<AddonPhraseItem> {
        let item = self.dict.phrase_item(token)?;
        let readings = item
            .pronunciations
            .into_iter()
            .filter_map(|(pinyin, freq)| Some((pinyin_to_keys(&pinyin)?, freq)))
            .collect();
        Some(AddonPhraseItem {
            text: item.text,
            readings,
            unigram: item.unigram,
        })
    }
}

/// A chosen addon phrase item, ready to promote into default nibble 5.
pub struct AddonPhraseItem {
    /// Phrase text.
    pub text: PhraseText,
    /// Pronunciations as `(key sequence, count)` pairs.
    pub readings: Vec<(Vec<PinyinKey>, u64)>,
    /// The item's copied unigram frequency (`add_phrase_item`).
    pub unigram: u64,
}

/// Splits a `'`-joined pinyin spelling into [`SyllableKey`] ids, or `None`
/// when any syllable is not a frozen key. The canonical lookup is used, so the
/// zhuyin-only syllables a stored reading can carry (`fe`, `den`, ...) resolve
/// as the pin's `content_table` does.
fn pinyin_to_keys(pinyin: &str) -> Option<Vec<PinyinKey>> {
    pinyin
        .split('\'')
        .map(|syllable| {
            let (spelling, tone) = match syllable.as_bytes().last().copied() {
                Some(digit @ b'1'..=b'5') => (&syllable[..syllable.len() - 1], digit - b'0'),
                _ => (syllable, 0),
            };
            SyllableKey::from_canonical_text(spelling)
                .and_then(|key| oxpinyin_user::toned_key(key.index(), tone))
        })
        .collect()
}

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
fn chewing_query(syllables: &[SyllableKey]) -> Option<Vec<oxpinyin_core::ChewingKey>> {
    Some(syllables.iter().map(|key| key.chewing_key()).collect())
}

// ── Merged backends ─────────────────────────────────────────────────────

/// One exact-text row. Its size follows the query and its matches.
struct TextTokens {
    generation: u64,
    text: Box<str>,
    tokens: Vec<u32>,
}

type TextLookupCache = Mutex<Option<TextTokens>>;

#[derive(Default)]
struct UnigramOverlay {
    tokens: HashMap<u32, u64>,
    libraries: HashMap<u32, u32>,
}

/// What `add_unigram_frequency` did to the default facade
/// (074a2219 `phrase_index.h:628-634`, `phrase_index.cpp:150-177`), shared
/// by the dictionary that applies it and the model that prices with it.
#[derive(Default)]
struct FacadeBumps {
    /// Every bump the facade total took, accepted or not: a `guint32`
    /// that wraps.
    total: AtomicU32,
    /// Whether `overlay` holds any token, so a step with no bump in the
    /// session takes no lock.
    live: AtomicBool,
    /// The accepted bumps: per token, as the item's own frequency rose,
    /// and per library, as its sub-index total did.
    overlay: Mutex<UnigramOverlay>,
}

impl FacadeBumps {
    fn overlay(&self) -> std::sync::MutexGuard<'_, UnigramOverlay> {
        self.overlay
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The accepted bumps on `token`'s item.
    fn token(&self, token: u32) -> u64 {
        if !self.live.load(Ordering::SeqCst) {
            return 0;
        }
        self.overlay().tokens.get(&token).copied().unwrap_or(0)
    }

    /// Drops the accepted bumps of every library `replaced` names: the
    /// facade takes the old sub-index's total away, accepted bumps
    /// included, and the library's items come back as stored
    /// (`phrase_index.cpp:237-248`, `:261-269`). A bump the sub-index
    /// refused never reached it, so it stays in the facade total.
    fn drop_libraries(&self, replaced: impl Fn(u32) -> bool) {
        let mut overlay = self.overlay();
        overlay.tokens.retain(|token, _| !replaced(token >> 24));
        let mut removed = 0_u32;
        overlay.libraries.retain(|&nibble, delta| {
            if replaced(nibble) {
                removed = removed.wrapping_add(*delta);
                false
            } else {
                true
            }
        });
        self.live
            .store(!overlay.tokens.is_empty(), Ordering::SeqCst);
        self.total.fetch_sub(removed, Ordering::SeqCst);
    }
}

/// The system dictionary with the user-phrase lookup merged in.
///
/// `SystemDictionary` holds DBM handles and mappings, so it rides an
/// `Arc`.
#[derive(Clone, Copy)]
pub struct RuntimeDict<'a> {
    system: SystemDictionary<
        &'a oxpinyin_data::phrase_libraries::PhraseLibraries,
        &'a oxpinyin_data::dict::SystemTables,
    >,
    user: Option<&'a UserStore>,
    text_lookup_cache: &'a TextLookupCache,
    addons: &'a RwLock<AddonSet>,
    punct: &'a PunctTable,
    /// Transient bumps for libraries without canonical store ownership,
    /// plus rejected calls' unconditional facade-total changes. Accepted
    /// system-item bumps live in the user store alongside training/replay.
    bumps: &'a FacadeBumps,
    /// The loaded-library mask: bit `n` **set** = library `n` unloaded
    /// (matches `library_visible` at :344 and the query surface —
    /// `library_visible_token`, `visible_item_count`, and the
    /// `unload_library` setter — that all consult it).  The pinyin ABI only
    /// ever sets bit 2 (`GBK_DICTIONARY`, `pinyin.cpp:464-472`); the zhuyin
    /// ABI any bit but 1 (`zhuyin.cpp:378-388`).
    library_mask: &'a AtomicU32,
}

#[cfg(test)]
thread_local! {
    static PRICING_EVALUATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl<'a> RuntimeDict<'a> {
    /// Shared lookup keeps search side effects and visibility identical;
    /// candidate collection omits only the pronunciation prices it discards.
    fn fill_lookup(
        &self,
        syllables: &[SyllableKey],
        out: &mut Vec<PhraseEntry>,
        price: bool,
    ) -> Result<bool, DictError> {
        // The ranking prices each row from `RuntimeLm::unigram_freq`, whose
        // visible-live answer is `UserStore::system_unigram`: the libpinyin
        // session's live-buffer item, the same bytes this read decodes for
        // the text. Capture that field here, on exactly that branch (a
        // session store, an `Overlay` item), so the ranking of this lookup
        // does not read the item a second time. A removed item or a masked
        // library leaves no entry; a shipped or user-table entry captures
        // nothing and the ranking reads as before.
        let capture = self.user.is_some_and(UserStore::is_libpinyin_session);
        let unreadable = self.system.lookup_with_live(
            syllables,
            out,
            price,
            |token| self.library_visible_token(token),
            |token, keys| {
                self.live_items()
                    .read(token, |item| {
                        let mut entry =
                            PhraseEntry::new(PhraseToken::new(token), item.phrase_text()?);
                        if capture && item.origin() == ItemOrigin::Overlay {
                            entry = entry.with_live_unigram(item.unigram());
                        }
                        if let Some(keys) = keys {
                            let (matched, total) = Self::item_possibility(item, keys);
                            entry = entry.with_pronunciation_possibility(
                                u64::from(matched),
                                u64::from(total),
                            );
                        }
                        Some(entry)
                    })
                    .found()
                    .flatten()
            },
        )?;
        let user = self.user_lookup()?;
        self.note_user_search(syllables);
        // System-index hits were already priced by the callback above. A
        // user-index hit may name a system token too; price that result once
        // from the same live item before appending it to the system results.
        let query = price.then(|| self.effective_query(syllables)).flatten();
        out.extend(user.lookup(syllables).into_iter().map(|entry| {
            let Some((matched, total)) = query
                .as_ref()
                .and_then(|keys| self.effective_possibility(keys, entry.token().value()))
            else {
                return entry;
            };
            entry.with_pronunciation_possibility(u64::from(matched), u64::from(total))
        }));
        if self.token_filter_active() {
            // An unloaded library's phrases leave every read: upstream
            // frees the sub-index (`phrase_index.cpp:260-268`), the
            // mask keeps the monolithic tables resident but invisible.
            out.retain(|entry| self.library_visible_token(entry.token().value()));
        }
        Ok(unreadable)
    }

    /// The user pinyin table's search for `syllables`, as the pin's
    /// `ChewingLargeTable2::search` runs it: the table entry it leaves
    /// behind is what a later `mask_out` walk starts from (#785).
    fn note_user_search(&self, syllables: &[SyllableKey]) {
        if let Some(store) = self.user {
            store.note_pinyin_search(|| chewing_query(syllables));
        }
    }

    /// The query's toned keys: each syllable's key with the tone at its
    /// index, `0` past the end of `tones`. Always `Some`; the `Option`
    /// keeps the callers' signature.
    fn trellis_query(
        syllables: &[SyllableKey],
        tones: &[u8],
    ) -> Option<Vec<oxpinyin_core::ChewingKey>> {
        Some(
            syllables
                .iter()
                .enumerate()
                .map(|(index, key)| {
                    key.chewing_key()
                        .with_tone(tones.get(index).copied().unwrap_or(0))
                })
                .collect(),
        )
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
    ) -> Result<(), DictError> {
        let tokens = self.user_lookup()?.search_toned(query);
        ranges.convert(
            tokens
                .into_iter()
                .filter(|token| self.token_ranged(*token))
                .map(|token| (token, Some(()))),
        );
        Ok(())
    }

    /// The query keys the effective-item overlay prices against; `None`
    /// when the store holds no system items.
    fn effective_query(&self, syllables: &[SyllableKey]) -> Option<Vec<oxpinyin_core::ChewingKey>> {
        self.live_items().has_overlay().then_some(())?;
        Some(syllables.iter().map(|key| key.chewing_key()).collect())
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
        #[cfg(test)]
        PRICING_EVALUATIONS.with(|count| count.set(count.get() + 1));
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
    /// token: the user table's lookup and the path's tone-less keys.
    fn path_pricing(&self, syllables: &[SyllableKey]) -> Result<PathPricing, DictError> {
        Ok(PathPricing {
            user: self.user_lookup()?.lookup(syllables),
            keys:
                SystemDictionary::<oxpinyin_data::phrase_libraries::PhraseLibraries>::trellis_keys(
                    syllables,
                ),
        })
    }

    /// What the ordinary lookup of `syllables` lists for `token`, which
    /// is what a step prices: the last entry of its system-then-user
    /// merge, the effective-item overlay applied, the library mask
    /// respected. `None` when the lookup does not list it. The text is
    /// built only when `want_text`.
    fn priced_token(
        &self,
        syllables: &[SyllableKey],
        token: u32,
        want_text: bool,
    ) -> Result<Option<PricedToken>, DictError> {
        if syllables.is_empty() {
            return Ok(None);
        }
        self.priced_on_path(&self.path_pricing(syllables)?, token, want_text)
    }

    /// [`Self::priced_token`] over a non-empty path's [`PathPricing`].
    fn priced_on_path(
        &self,
        path: &PathPricing,
        token: u32,
        want_text: bool,
    ) -> Result<Option<PricedToken>, DictError> {
        let user = path
            .user
            .iter()
            .rev()
            .find(|entry| entry.token().value() == token);
        let mut priced = if let Some(entry) = user {
            PricedToken {
                item: TrellisItem {
                    chars: entry.phrase_length().try_into().unwrap_or(u32::MAX),
                    pronunciation: entry.pronunciation_possibility(),
                },
                text: want_text.then(|| entry.text().clone()),
            }
        } else {
            let live = self.live_items();
            let Some((chars, text)) = live
                .read(token, |item| {
                    (
                        u32::try_from(item.phrase_length()).unwrap_or(u32::MAX),
                        if want_text { item.phrase_text() } else { None },
                    )
                })
                .found()
            else {
                return Ok(None);
            };
            if want_text && text.is_none() {
                return Ok(None);
            }
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
        if let Some(query) = path.keys.as_deref()
            && self.live_items().has_overlay()
            && let Some((matched, total)) = self.effective_possibility(query, token)
        {
            priced.item.pronunciation = Some((u64::from(matched), u64::from(total)));
        }
        if self.token_filter_active() && !self.library_visible_token(token) {
            return Ok(None);
        }
        Ok(Some(priced))
    }

    fn user_lookup(&self) -> Result<UserLookup<'_>, DictError> {
        self.user.map_or_else(
            || Ok(UserLookup::empty()),
            |store| {
                store
                    .lookup()
                    .map_err(|error| DictError::Parse(error.to_string()))
            },
        )
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

    /// Loads addon library `index` from `system_dir`; `false` when already
    /// loaded or the tables are missing/unopenable.
    #[must_use]
    fn load_addon(&self, index: u8, system_dir: &Path) -> bool {
        self.load_addon_reporting(index, system_dir)
            .unwrap_or(false)
    }

    /// [`RuntimeDict::load_addon`] with the failure kept: an error when
    /// the library file is absent or does not verify, where
    /// [`LibraryError::unmappable_path`] names the file the pin could not
    /// map. `Ok(false)` for an index that names no addon library and for
    /// one already loaded.
    ///
    /// # Errors
    ///
    /// [`LibraryError`] when the library file is absent or does not verify.
    fn load_addon_reporting(&self, index: u8, system_dir: &Path) -> Result<bool, LibraryError> {
        let mut addons = self
            .addons
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        addons.load_reporting(index, system_dir)
    }

    /// The system library files absent when the runtime opened, in the
    /// pin's load order.
    pub fn unmapped_system_libraries(&self) -> impl Iterator<Item = &Path> {
        self.system.libraries().unmapped()
    }

    /// The loaded, visible system libraries whose files no longer map, in
    /// library order; with `only` set, that library alone. An unloaded
    /// library is not looked at.
    #[must_use]
    pub fn unmappable_system_libraries(&self, only: Option<u8>) -> Vec<PathBuf> {
        self.system
            .libraries()
            .unmappable_now(|nibble| self.library_visible(u32::from(nibble)), only)
    }

    /// Unloads addon library `index`.
    ///
    /// Answers `true` whether or not the library was loaded, mirroring the
    /// pin's unconditional `unload`. The caller applies the pin's
    /// `index < PHRASE_INDEX_LIBRARY_COUNT` bound: that assertion is an ABI
    /// availability-class concern, not a runtime one.
    #[must_use]
    fn unload_addon(&self, index: u8) -> bool {
        let mut addons = self
            .addons
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
        let newly_unloaded = self.library_mask.fetch_or(mask, Ordering::SeqCst) & mask == 0;
        // The freed sub-index takes its accepted bumps with it, and a
        // reload maps the library's items as stored (`phrase_index.cpp:261-269`).
        if newly_unloaded {
            self.bumps.drop_libraries(|nibble| nibble == index);
        }
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
        self.library_mask
            .fetch_and(!(1u32 << index), Ordering::SeqCst)
            & (1u32 << index)
            != 0
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
    /// `visible_item_count`, `add_unigram_delta`,
    /// `pinyin_token_add_unigram_frequency` callers) routes through
    /// this to keep the shift and the `mask == 0` shortcut in one
    /// place.
    #[must_use]
    pub fn library_visible(&self, nibble: u32) -> bool {
        let mask = self.library_mask.load(Ordering::SeqCst);
        if mask == 0 || nibble >= 32 {
            return true;
        }
        mask & (1u32 << nibble) == 0
    }

    /// The in-memory unigram delta for `token`, if the overlay carries
    /// one. The caller applies its library-loaded rules first.
    #[must_use]
    pub fn unigram_delta(&self, token: u32) -> Option<u64> {
        self.bumps.overlay().tokens.get(&token).copied()
    }

    /// Applies `add_unigram_frequency`'s effects for a loaded library:
    /// the facade-total bump is unconditional (`phrase_index.h:632`,
    /// before the item dispatch — an absent-token add still moves the
    /// amplified-law denominator), and the item delta lands only when
    /// the token exists and its library total accepts the addition. Returns
    /// whether the item update succeeded. A token
    /// whose library is unloaded is invisible on both edges: neither
    /// the total nor the overlay moves, matching the visibility filter
    /// the rest of the surface honours.
    #[must_use]
    fn add_unigram_delta(&self, token: u32, delta: u64) -> bool {
        if !self.library_visible(token >> 24) {
            return false;
        }
        let found = {
            let system_found = self.live_items().read(token, |_| ()).found().is_some();
            let addons = self
                .addons
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let addon_found = addons.unigram_freq(token).is_some();
            let user_found = self
                .user
                .and_then(|store| store.phrase(token).ok())
                .flatten()
                .is_some();
            system_found || addon_found || user_found
        };
        // The facade total moves before the sub-index overflow check, and it
        // is a `guint32` that wraps (`m_total_freq += delta`,
        // `phrase_index.h:633`): a wrap to zero is the `0 < total_freq`
        // assert of `pinyin.cpp:1859`, which the capi layer answers (class
        // (c)) instead of dividing by it. `fetch_add` wraps in debug and
        // release alike.
        let Ok(delta) = u32::try_from(delta) else {
            return false;
        };
        self.bumps.total.fetch_add(delta, Ordering::SeqCst);
        if !found {
            return false;
        }
        let mut overlay = self.bumps.overlay();
        let library = token >> 24;
        let base = self
            .system
            .libraries()
            .library(library << 24)
            .map_or(0, |lib| lib.total_freq());
        let accepted = overlay.libraries.get(&library).copied().unwrap_or(0);
        let user_bound = match self.user.map(|user| user.unigram_total()).transpose() {
            Ok(total) => total.unwrap_or(0),
            Err(_) => return false,
        };
        // Ordinary additions need only the cached/global bound. Scan the
        // user library only at the overflow edge, not on every training bump.
        // Until the next compact the library still holds its stored total,
        // not its items' sum (`phrase_index.cpp:170-171`): the difference
        // the user store keeps is part of the guarded sum, and it breaks the
        // item-sum bound. One atomic load when no library carries one.
        let offset = match self
            .user
            .map(|user| user.library_offset(library as u8))
            .transpose()
        {
            Ok(offset) => offset.unwrap_or(0),
            Err(_) => return false,
        };
        let bound = u64::from(base) + u64::from(accepted);
        if offset != 0
            || bound
                .saturating_add(user_bound)
                .saturating_add(u64::from(delta))
                > u64::from(u32::MAX)
        {
            let user_total = match self
                .user
                .map(|user| user.unigram_library_total(library as u8))
                .transpose()
            {
                Ok(total) => total.unwrap_or(0),
                Err(_) => return false,
            };
            let total = base
                .wrapping_add(accepted)
                .wrapping_add(user_total)
                .wrapping_add(offset);
            if total.checked_add(delta).is_none() {
                return false;
            }
        }
        overlay
            .libraries
            .insert(library, accepted.wrapping_add(delta));
        let entry = overlay.tokens.entry(token).or_insert(0);
        *entry = entry.saturating_add(u64::from(delta));
        self.bumps.live.store(true, Ordering::SeqCst);
        true
    }

    /// The overlay total — the amplified-law denominator's live shift
    /// (`pinyin.cpp:1817` reads the facade total this mirrors). A `guint32`
    /// that wraps, as the facade total does.
    #[must_use]
    pub fn unigram_total_delta(&self) -> u32 {
        self.bumps.total.load(Ordering::SeqCst)
    }

    /// The addon library's stored unigram count for `token`.
    #[must_use]
    pub fn addon_unigram_frequency(&self, token: u32) -> Option<u64> {
        let addons = self
            .addons
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        addons.unigram_freq(token)
    }

    /// The visible item count for the amplified-law denominator: the
    /// items of every library that is not unloaded. Per-library counts
    /// are tallied once and cached.
    #[must_use]
    pub fn visible_item_count(&self) -> u64 {
        self.system.libraries().item_count_where_live(
            |nibble| self.library_visible(u32::from(nibble)),
            |nibble| {
                self.user
                    .and_then(|store| store.pristine_system_item_count(nibble))
            },
        )
    }

    /// Whether the token's item is readable, for entry-level filters: its
    /// library is not unloaded, and no journal REMOVE took the item out of the
    /// sub-index (`get_phrase_item` fails for both, 074a2219
    /// `phrase_index.cpp:180-206`). The phrase table still holds the token
    /// either way ([`Self::phrase_table_token_enabled`]).
    #[must_use]
    pub fn library_visible_token(&self, token: u32) -> bool {
        self.live_items().is_visible(token)
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
        let nibble = token >> 24;
        self.library_visible(nibble)
            && (self.system.library_loaded(token)
                || ((5..=7).contains(&nibble)
                    && self
                        .user
                        .as_ref()
                        .is_some_and(|store| store.has_user_library(token.to_be_bytes()[0]))))
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
        if u8::try_from(token >> 24).is_ok_and(|index| self.is_system_library(index)) {
            return self.live_items().pronunciation_count(token).found();
        }
        match token >> 24 {
            5..=7 => {
                if !self.library_visible(token >> 24) {
                    return None;
                }
                self.user?.user_pronunciation_count(token)
            }
            _ => None,
        }
    }

    /// The live item's text for `_add_phrase`'s length and content checks
    /// (074a2219 pinyin.cpp:566-571; zhuyin.cpp:452-457). Reading spellings,
    /// frequencies and the unigram do not participate in those checks.
    #[must_use]
    pub fn token_text(&self, token: u32) -> Option<PhraseText> {
        if u8::try_from(token >> 24).is_ok_and(|index| self.is_system_library(index)) {
            return self
                .live_items()
                .read(token, |item| item.phrase_text())
                .found()
                .flatten();
        }
        match token >> 24 {
            5..=7 => {
                if !self.library_visible(token >> 24) {
                    return None;
                }
                let store = self.user?;
                store
                    .with_user_item(token, |item| item.phrase_text())
                    .unwrap_or_else(|| {
                        store.phrase_text(token).ok().flatten().and_then(|words| {
                            PhraseText::from_stored_words(words.into_iter().map(Some))
                        })
                    })
            }
            _ => None,
        }
    }

    /// Whether the default facade owns this item. Missing/unloaded libraries
    /// and absent slots both answer false, as `get_phrase_item` does
    /// (074a2219 `phrase_index.h:646-652`, `phrase_index.cpp:180-205`).
    /// No text or pronunciation payload is decoded.
    #[must_use]
    pub fn token_exists(&self, token: u32) -> bool {
        if u8::try_from(token >> 24).is_ok_and(|index| self.is_system_library(index)) {
            return self
                .live_items()
                .pronunciation_count(token)
                .found()
                .is_some();
        }
        match token >> 24 {
            5..=7 => {
                self.library_visible(token >> 24)
                    && self
                        .user
                        .as_ref()
                        .is_some_and(|store| store.phrase_exists(token).unwrap_or(false))
            }
            _ => false,
        }
    }

    /// Read only the nth insertion-ordered key list and the stored length.
    /// `None` means a missing item/library; a present item with no nth reading
    /// retains its length for the existing class-(b) zero-key C result
    /// (074a2219 `pinyin.cpp:2801-2818`, `zhuyin.cpp:1793-1810`).
    #[must_use]
    pub fn token_nth_pronunciation(&self, token: u32, nth: usize) -> Option<TokenPronunciation> {
        if u8::try_from(token >> 24).is_ok_and(|index| self.is_system_library(index)) {
            return self
                .live_items()
                .read(token, |item| TokenPronunciation {
                    phrase_length: item.phrase_length(),
                    keys: item
                        .pronunciation(nth)
                        .map(|reading| reading.keys.iter().collect()),
                })
                .found();
        }
        match token >> 24 {
            5..=7 => {
                if !self.library_visible(token >> 24) {
                    return None;
                }
                let store = self.user?;
                Some(TokenPronunciation {
                    phrase_length: store.phrase_length(token).ok()??,
                    keys: store.nth_pronunciation_words(token, nth).ok()?,
                })
            }
            _ => None,
        }
    }

    /// Read one `USER_FILE` reading's stored key words without copying them
    /// out of the session arena.
    ///
    /// The callback receives the stored phrase length and the reading's
    /// packed little-endian words, or `None` when the item has no such
    /// reading (the pin's zeroed past-the-end result). `None` is returned
    /// when no visible `USER_FILE` item backs the token, so a caller falls
    /// back to [`Self::token_nth_pronunciation`], which serves a row store.
    pub fn with_user_nth_pronunciation_words<R>(
        &self,
        token: u32,
        nth: usize,
        read: impl FnOnce(usize, Option<&[u8]>) -> R,
    ) -> Option<R> {
        if !(5..=7).contains(&(token >> 24)) || !self.library_visible(token >> 24) {
            return None;
        }
        self.user?.with_nth_pronunciation_words(token, nth, read)
    }

    /// The addon phrase item behind `token`, for the choose-promotion path.
    #[must_use]
    pub fn addon_phrase_item(&self, token: u32) -> Option<AddonPhraseItem> {
        let addons = self
            .addons
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        addons.phrase_item(token)
    }
}

/// What one key path's pricing reads whatever the token, kept in a
/// [`TrellisPathMemo`] for the n-best call that prices the path.
pub struct PathPricing {
    /// The user table's lookup of the path.
    user: Vec<PhraseEntry>,
    /// The path's tone-less keys; `None` when a syllable is no key.
    keys: Option<Vec<oxpinyin_core::ChewingKey>>,
}

/// One token as the ordinary lookup prices it.
struct PricedToken {
    item: TrellisItem,
    text: Option<PhraseText>,
}

impl Dictionary for RuntimeDict<'_> {
    type Syllable = SyllableKey;
    type Entry = PhraseEntry;
    type Error = DictError;
    type PathMemo = TrellisPathMemo<PathPricing>;

    fn lookup(&self, syllables: &[Self::Syllable]) -> Result<Vec<Self::Entry>, Self::Error> {
        let mut entries = Vec::new();
        self.lookup_into(syllables, &mut entries)?;
        Ok(entries)
    }

    fn trellis_records(
        &self,
        syllables: &[Self::Syllable],
        tones: &[u8],
    ) -> Result<Vec<Self::Entry>, Self::Error> {
        let mut ranges = PhraseIndexRanges::new();
        self.trellis_ranges(syllables, tones, &mut ranges)?;
        let mut out = Vec::new();
        for (token, ()) in ranges.walk_tokens() {
            if let Some(priced) = self.priced_token(syllables, token, true)? {
                let mut entry =
                    PhraseEntry::new(PhraseToken::new(token), priced.text.unwrap_or_default());
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
        let Some(query) = Self::trellis_query(syllables, tones) else {
            return Ok(());
        };
        // The user table's search of this path (#785); the pin makes one
        // per `FacadeChewingTable2::search`, and a repeat is idempotent.
        self.note_user_search(syllables);
        let loaded = |token: u32| self.token_ranged(token);
        self.system
            .trellis_ranges_with(syllables, tones, ranges, loaded)?;
        self.user_trellis_ranges(&query, ranges)
    }

    /// [`Self::trellis_ranges`] and [`Self::phrase_prefix_exists`] with one
    /// system DBM get. A live library mask keeps the two calls: the
    /// visibility walk behind its probe is a different read.
    fn trellis_ranges_continued(
        &self,
        syllables: &[Self::Syllable],
        tones: &[u8],
        ranges: &mut PhraseIndexRanges<()>,
        scratch: &mut Vec<u8>,
    ) -> Result<bool, Self::Error> {
        let Some(query) = Self::trellis_query(syllables, tones) else {
            self.trellis_ranges(syllables, tones, ranges)?;
            return self.phrase_prefix_exists(syllables);
        };
        if self.library_mask.load(Ordering::SeqCst) != 0 && !self.system.pinyin_attachment_failed()
        {
            self.trellis_ranges(syllables, tones, ranges)?;
            return self.phrase_prefix_exists(syllables);
        }
        let loaded = |token: u32| self.token_ranged(token);
        let system_extends = self
            .system
            .trellis_ranges_continued_with(syllables, tones, ranges, loaded, scratch)?;
        self.user_trellis_ranges(&query, ranges)?;
        let user = self.user_lookup()?;
        self.note_user_search(syllables);
        if system_extends {
            return Ok(true);
        }
        Ok(user.phrase_prefix_exists(syllables))
    }

    fn trellis_item(
        &self,
        syllables: &[Self::Syllable],
        _tones: &[u8],
        token: u32,
    ) -> Result<Option<TrellisItem>, Self::Error> {
        Ok(self
            .priced_token(syllables, token, false)?
            .map(|priced| priced.item))
    }

    /// [`Self::trellis_item`], reading the user lookup and the keys once
    /// per path.
    fn trellis_item_on_path(
        &self,
        syllables: &[Self::Syllable],
        _tones: &[u8],
        token: u32,
        memo: &Self::PathMemo,
    ) -> Result<Option<TrellisItem>, Self::Error> {
        if syllables.is_empty() {
            return Ok(None);
        }
        let path = memo.get_or_try_init(|| self.path_pricing(syllables))?;
        Ok(self
            .priced_on_path(path, token, false)?
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
        let path = memo.get_or_try_init(|| self.path_pricing(syllables))?;
        let _ = tones; // The existing path memo deliberately uses tone-less keys.
        Ok(self
            .priced_with_facts(path, token, false, facts)?
            .map(|priced| priced.item))
    }

    fn trellis_text_with_facts(
        &self,
        syllables: &[Self::Syllable],
        _tones: &[u8],
        token: u32,
        facts: &mut oxpinyin_core::sentence_facts::SentenceFacts,
    ) -> Result<Option<PhraseText>, Self::Error> {
        if syllables.is_empty() {
            return Ok(None);
        }
        Ok(self
            .priced_with_facts(&self.path_pricing(syllables)?, token, true, facts)?
            .and_then(|priced| priced.text))
    }

    fn trellis_text(
        &self,
        syllables: &[Self::Syllable],
        _tones: &[u8],
        token: u32,
    ) -> Result<Option<PhraseText>, Self::Error> {
        Ok(self
            .priced_token(syllables, token, true)?
            .and_then(|priced| priced.text))
    }

    fn lookup_into(
        &self,
        syllables: &[Self::Syllable],
        out: &mut Vec<Self::Entry>,
    ) -> Result<(), Self::Error> {
        self.lookup_into_flagged(syllables, out).map(|_| ())
    }

    fn lookup_into_flagged(
        &self,
        syllables: &[Self::Syllable],
        out: &mut Vec<Self::Entry>,
    ) -> Result<bool, Self::Error> {
        self.fill_lookup(syllables, out, true)
    }

    fn lookup_candidates_into(
        &self,
        syllables: &[Self::Syllable],
        out: &mut Vec<Self::Entry>,
    ) -> Result<bool, Self::Error> {
        self.fill_lookup(syllables, out, false)
    }

    fn candidate_search_into(
        &self,
        syllables: &[Self::Syllable],
        tones: &[u8],
        out: &mut Vec<Self::Entry>,
    ) -> Result<Option<(bool, bool)>, Self::Error> {
        if !self.system.pinyin_attachment_failed() {
            return Ok(None);
        }
        // One failed native get/check per search_matrix leaf. Tone and
        // continuation probes must not make additional system reads.
        let unreadable = self.fill_lookup(syllables, out, false)?;
        if tones.iter().any(|tone| *tone != 0) {
            let mut ranges = PhraseIndexRanges::new();
            if let Some(query) = Self::trellis_query(syllables, tones) {
                self.user_trellis_ranges(&query, &mut ranges)?;
            }
            let mut tokens: Vec<u32> = ranges.walk_tokens().map(|(token, ())| token).collect();
            tokens.sort_unstable();
            out.retain(|entry| tokens.binary_search(&entry.token().value()).is_ok());
        }
        Ok(Some((
            unreadable,
            self.user_lookup()?.phrase_prefix_exists(syllables),
        )))
    }

    fn phrase_prefix_exists(&self, syllables: &[Self::Syllable]) -> Result<bool, Self::Error> {
        // A live library mask must hide unloaded entries from the widen
        // probe too — the CR finding on PR #234. Without this the n-best
        // decoder keeps extending paths that lead only to invisible
        // tokens (`pinyin_unload_phrase_library(2)` leaves the underlying
        // rows resident; only the visibility mask changes). With the
        // mask clear, the plain probe's fast path stays.
        // A journal REMOVE does not reach this answer: `SEARCH_CONTINUED` comes
        // from the pinyin table's DB get alone (`chewing_large_table2_bdb.cpp:182-187`
        // at 074a2219), and the removal clears only the sub-index. The removed item
        // leaves where `get_phrase_item` is read, so the walk below sees the library
        // mask and nothing else.
        let system_extends = if self.library_mask.load(Ordering::SeqCst) == 0 {
            self.system.phrase_prefix_exists(syllables)?
        } else {
            self.system
                .phrase_prefix_exists_visible(syllables, |token| {
                    self.library_visible(token >> 24)
                        && self
                            .system
                            .libraries()
                            .library(token)
                            .is_some_and(|library| {
                                !library.is_detached()
                                    || self.live_items().read(token, |_| ()).found().is_some()
                            })
                })?
        };
        // The pin searches the system table and then the user table with no
        // early return (`facade_chewing_table2.h:120-126` at 074a2219), so
        // the user lookup and its search note come before the answer.
        let user = self.user_lookup()?;
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
        let mut tokens: Vec<PhraseToken> = self
            .system
            .tokens_for_text(text)
            .unwrap_or_default()
            .into_iter()
            .map(PhraseToken::new)
            .collect();
        // The pin probes the user phrase table directly (074a2219
        // pinyin.cpp:2652-2667, storage/facade_phrase_table3.h:120-126).
        // Read only this text's row. A phrase write must not make this
        // query rebuild the full pronunciation and phrase indices.
        if let Some(store) = self.user {
            let mut cache = self
                .text_lookup_cache
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let generation = store.phrase_table_generation();
            if cache
                .as_ref()
                .is_none_or(|row| row.generation != generation || row.text.as_ref() != text)
            {
                *cache = store
                    .phrase_table_tokens_for_text(text)
                    .ok()
                    .map(|tokens| TextTokens {
                        generation,
                        text: text.into(),
                        tokens,
                    });
            }
            if let Some(row) = cache.as_ref() {
                tokens.extend(row.tokens.iter().copied().map(PhraseToken::new));
            }
        }
        tokens.retain(|token| self.phrase_table_token_enabled(token.value()));
        // reduce_tokens groups the system and user table results by library.
        tokens.sort_by_key(|token| token.value() >> 24);
        tokens
    }

    /// [`Self::tokens_for_text`] over raw UCS-4 spans, including
    /// surrogate or above-Unicode words. The system and user phrase
    /// tables compare the stored raw keys; no `char` or `str` is built
    /// from a non-scalar. The rare non-scalar path reads the user row
    /// directly rather than through the text cache.
    fn tokens_for_ucs4(&self, ucs4: &[u32]) -> Vec<PhraseToken> {
        let mut tokens: Vec<PhraseToken> = self
            .system
            .tokens_for_ucs4(ucs4)
            .unwrap_or_default()
            .into_iter()
            .map(PhraseToken::new)
            .collect();
        if let Some(store) = self.user {
            tokens.extend(
                store
                    .phrase_table_tokens_for_ucs4(ucs4)
                    .unwrap_or_default()
                    .into_iter()
                    .map(PhraseToken::new),
            );
        }
        tokens.retain(|token| self.phrase_table_token_enabled(token.value()));
        tokens.sort_by_key(|token| token.value() >> 24);
        tokens
    }

    fn phrase_span_searches_are_observable(&self) -> bool {
        self.system.phrase_attachment_failed()
    }

    fn phrase_span_continues(&self, words: &[u32]) -> bool {
        !self.system.phrase_attachment_failed()
            || self
                .user
                .as_ref()
                .is_some_and(|store| store.phrase_table_contains_ucs4(words).unwrap_or(false))
    }

    fn lookup_addon(&self, syllables: &[Self::Syllable]) -> Result<Vec<Self::Entry>, Self::Error> {
        let mut entries = Vec::new();
        self.lookup_addon_into(syllables, &mut entries)?;
        Ok(entries)
    }

    fn lookup_addon_into(
        &self,
        syllables: &[Self::Syllable],
        out: &mut Vec<Self::Entry>,
    ) -> Result<(), Self::Error> {
        let addons = self
            .addons
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        addons.lookup_into(syllables, out)
    }

    fn lookup_addon_candidates_into(
        &self,
        syllables: &[Self::Syllable],
        out: &mut Vec<Self::Entry>,
    ) -> Result<(), Self::Error> {
        let addons = self
            .addons
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        addons.lookup_candidates_into(syllables, out)
    }

    fn phrase_prefix_exists_addon(
        &self,
        syllables: &[Self::Syllable],
    ) -> Result<bool, Self::Error> {
        let addons = self
            .addons
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        addons.prefix_exists(syllables)
    }

    fn phrase_index_item_count(&self) -> Result<u64, Self::Error> {
        // System items only, of the libraries that are loaded: upstream
        // frees an unloaded sub-index, so its items leave the facade
        // count. The parity surface the ranking denominator reproduces
        // runs an empty user store, where this is the whole facade; a
        // trained store's user items are not folded in.
        Ok(self.visible_item_count())
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
        if self
            .user
            .is_some_and(|store| store.overlong_extension_gate(syllables))
        {
            return Ok(None);
        }
        let user = self.user_lookup()?;
        if let Some(store) = self.user {
            store.note_pinyin_suggestion(|| chewing_query(syllables));
        }
        tokens.extend(user.suggest_extensions(syllables));
        if self.library_mask.load(Ordering::SeqCst) != 0 {
            tokens.retain(|token| self.library_visible(*token >> 24));
        }
        tokens.sort_by_key(|token| token >> 24);
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
            || self
                .user
                .is_some_and(|store| store.overlong_extension_gate(syllables))
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
        let store = self.user?;
        store
            .phrase(token)
            .ok()
            .flatten()
            .and_then(|phrase| phrase.phrase_text())
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
        if let Some(store) = self.user
            && let Some(rows) = store
                .item_pronunciations(value)
                .map_err(|error| DictError::Parse(error.to_string()))?
        {
            return Ok(PhraseItemRead::Item(rows));
        }
        self.system.phrase_item_pronunciations(token)
    }

    /// Both seams under this facade handle partial-key queries with one
    /// probe: the system dictionary's DBM is double-indexed when it is built
    /// (see [`SystemDictionary`]), and `UserLookup` mirrors that with a
    /// `by_initial` index (`crates/oxpinyin-user/src/lookup.rs`). Addon
    /// facades share the DBM shape.
    fn handles_partial_keys(&self) -> bool {
        true
    }
}

/// The bigram language model with the user-count overlay.
#[derive(Clone, Copy)]
pub struct RuntimeLm<'a> {
    inner: BigramLanguageModel<
        &'a oxpinyin_data::phrase_libraries::PhraseLibraries,
        &'a oxpinyin_data::lm::BigramTables,
        &'a AtomicU32,
    >,
    user: Option<&'a UserStore>,
    libraries: &'a oxpinyin_data::phrase_libraries::PhraseLibraries,
    addons: &'a RwLock<AddonSet>,
    /// The dictionary's `add_unigram_frequency` bumps: the total's is part
    /// of the one facade total every reader divides by, and a token's is
    /// part of its item's frequency, which every reader prices with.
    bumps: &'a FacadeBumps,
}

impl RuntimeLm<'_> {
    /// Whether the session holds an item for `token` that the shipped chunk
    /// does not: a SYSTEM_FILE ADD, absent from the immutable chunk but
    /// present in the current facade (074a2219 `pinyin.cpp:598-607`). A store
    /// that cannot read the held item's count fails the step.
    fn imported_item(&self, token: u32) -> Result<bool, LmError> {
        LiveSystemItems::unmasked(self.libraries, self.user)
            .is_imported(token)
            .map_err(|error| LmError::User(error.to_string()))
    }

    fn delta(&self, prev: Option<u32>, token: u32) -> Result<UserCountDelta, LmError> {
        let mut delta = match self.user {
            None => UserCountDelta::ZERO,
            Some(store) => store
                .count_delta(prev, token)
                .map_err(|error| LmError::User(error.to_string()))?,
        };
        delta.unigram_delta = delta.unigram_delta.saturating_add(self.bumps.token(token));
        if let Some(store) = self.user.filter(|store| store.is_libpinyin_session()) {
            delta.unigram_total_delta =
                store.unigram_total_over(|nibble| self.inner.is_visible(nibble));
        }
        delta.unigram_total_delta = self.total_extra(delta.unigram_total_delta);
        Ok(delta)
    }

    /// What the facade total holds beyond the visible libraries' stored
    /// totals: `user_total`, the user store's part, plus every
    /// `add_unigram_frequency` bump (`phrase_index.h:633`). The model
    /// adds it to the libraries' total wrapping, as the `guint32` does.
    fn total_extra(&self, user_total: u64) -> u64 {
        user_total.wrapping_add(u64::from(self.bumps.total.load(Ordering::SeqCst)))
    }

    /// `prev`'s merged row: the system gram and the user gram, one point
    /// read each, merged as the pin's lookups merge them
    /// ([`BigramLanguageModel::load_merged_row`]). The user side is the
    /// stored value itself (`bigram_row`), `None` without a key.
    fn merged_row(&self, prev: u32) -> Result<Option<MergedGram>, LmError> {
        let user = match self.user {
            None => None,
            Some(store) => store
                .bigram_row(prev)
                .map_err(|error| LmError::User(error.to_string()))?,
        };
        self.inner.load_merged_row(prev, user)
    }

    /// The phrase-index total the pin's amplified law divides by
    /// (`pinyin.cpp:1813-1814`, `get_phrase_index_total_freq`, live per
    /// call): the visible libraries' stored totals, the user store's extra
    /// and the `add_unigram_frequency` bumps, wrapped at `guint32`. Must
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

    fn score(
        &self,
        history: &[Self::Token],
        token: &Self::Token,
        edge_cost: Cost,
    ) -> Result<Cost, Self::Error> {
        // The bigram comes from the previous token's merged row; the
        // overlay supplies the unigram fields alone.
        let extra = self.delta(None, token.value())?;
        self.inner
            .score_with_merged_row(history, token, edge_cost, extra, |prev| {
                self.merged_row(prev)
            })
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
        let live = self
            .user
            .and_then(|store| store.system_unigram(token.value()));
        let extra = if live.is_some() && self.inner.is_visible((token.value() >> 24) as u8) {
            // The model uses only the live field on this branch, ignoring even
            // facade bumps. A live value also implies a libpinyin session,
            // where unigram_delta is infallible: skipping it loses no error.
            0
        } else {
            let delta = match self.user {
                None => 0,
                Some(store) => store
                    .unigram_delta(token.value())
                    .map_err(|error| LmError::User(error.to_string()))?,
            };
            delta.saturating_add(self.bumps.token(token.value()))
        };
        Ok(self
            .inner
            .unigram_freq_with_user_delta(token.value(), live, extra))
    }

    fn has_real_unigrams(&self) -> bool {
        self.inner.has_real_unigrams()
    }

    fn unigram_total(&self) -> Result<Option<u64>, Self::Error> {
        if !self.inner.has_real_unigrams() {
            return Ok(None);
        }
        let user_total = match self.user {
            None => 0,
            Some(store) if store.is_libpinyin_session() => {
                store.unigram_total_over(|nibble| self.inner.is_visible(nibble))
            }
            Some(store) => store
                .unigram_total()
                .map_err(|error| LmError::User(error.to_string()))?,
        };
        Ok(Some(self.inner.facade_total(self.total_extra(user_total))))
    }

    fn addon_unigram_freq(&self, token: &Self::Token) -> Result<Option<u64>, Self::Error> {
        let addons = self
            .addons
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if addons.is_empty() {
            return Ok(None);
        }
        Ok(Some(addons.unigram_freq(token.value()).unwrap_or(0)))
    }

    fn addon_unigram_total(&self) -> Result<Option<u64>, Self::Error> {
        let addons = self
            .addons
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Ok(addons.unigram_total())
    }

    /// The §5 overlay `score` takes, forwarded into the n-best step costs.
    fn nbest_step_costs(
        &self,
        prev: &Self::Token,
        token: &Self::Token,
    ) -> Result<NbestStepCosts, Self::Error> {
        let extra = self.delta(Some(prev.value()), token.value())?;
        // A SYSTEM_FILE ADD is absent from the immutable chunk but present
        // in the current facade (074a2219 pinyin.cpp:598-607).
        let imported = self.imported_item(token.value())?;
        self.inner
            .nbest_step_costs_with_imported_item(prev, token, extra, imported)
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
        loop {
            // A committed write moves the generation under the same database
            // guard used by the unigram and override reads below.
            let generation = self.user.map_or(0, UserStore::generation);
            let result = (|| {
                let extra = self.delta(None, token.value())?;
                let imported = self.imported_item(token.value())?;
                self.inner.nbest_item_costs(token, extra, imported, item)
            })();
            if self
                .user
                .is_none_or(|store| store.generation() == generation)
            {
                return result;
            }
        }
    }

    fn begin_successor_span(&self, row: &mut oxpinyin_core::NbestStepRow) {
        if self.inner.attachment_failed() {
            *row = oxpinyin_core::NbestStepRow::default();
        }
    }

    fn nbest_successors_in_row<'a>(
        &self,
        prev: &Self::Token,
        row: &'a mut oxpinyin_core::NbestStepRow,
    ) -> Result<Option<oxpinyin_core::NbestSuccessors<'a>>, Self::Error> {
        loop {
            let generation = self.user.map_or(0, UserStore::generation);
            let result = row
                .get_or_try_init_in_generation(generation, || self.merged_row(prev.value()))
                .map(|_| ());
            if self
                .user
                .is_none_or(|store| store.generation() == generation)
            {
                result?;
                return Ok(Some(match row.cached_merged() {
                    Some(gram) => oxpinyin_core::NbestSuccessors::Row(gram),
                    None => oxpinyin_core::NbestSuccessors::Miss,
                }));
            }
        }
    }
}

// ── Runtime ─────────────────────────────────────────────────────────────

/// A [`Session`] over this crate's concrete backends.
pub type RuntimeSession<'a> =
    Session<RuntimeDict<'a>, RuntimeLm<'a>, &'a mut oxpinyin_engine::SessionState>;

// Compile-time proof that adapters may share a `Runtime` across threads
// behind their own synchronization.
const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<RuntimeDict>();
    assert_send_sync::<RuntimeLm>();
    assert_send_sync::<UserStore>();
};

/// One opened engine: configuration-independent wiring for the converted
/// system tables, the unigram model, λ, the optional learning store, and the
/// merged backends sessions decode over.
///
/// Storage has one owner. [`Runtime::dict`], [`Runtime::lm`] and
/// [`Runtime::user_store`] borrow it for an operation; writers require
/// exclusive access. [`Runtime::new_session`] retains composition only.
pub struct Runtime {
    paths: StoragePaths,
    system: oxpinyin_data::dict::SystemTables,
    model: oxpinyin_data::lm::BigramTables,
    originals: oxpinyin_user::SystemOriginals,
    user: Option<UserStore>,
    conf: TableConf,
    text_lookup_cache: TextLookupCache,
    addons: RwLock<AddonSet>,
    punct: PunctTable,
    bumps: FacadeBumps,
    library_mask: AtomicU32,
}

impl Runtime {
    /// Perform context fini effects once without releasing storage retained by
    /// orphan instances or iterators (compatibility row 42).
    pub fn finish(&mut self) {
        // pinyin.cpp:1200 writes the marker before deleting failed attachments.
        if let Some(user) = self.user.as_mut() {
            user.finish();
        }
        // pinyin.cpp:1205-1215 and zhuyin.cpp:744-748: the failed objects
        // are reset in the pin's member order on every backend; a second
        // finish finds them gone.
        self.system.finish_failed_attachments();
        self.model.finish_failed_attachment();
        self.addons
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .dict
            .finish_failed_attachments();
        self.punct.finish_failed_attachment();
    }

    /// Whether every retained failed system attachment has been finalized
    /// by [`Self::finish`]; `true` when none failed.
    #[must_use]
    pub fn failed_attachments_finished(&self) -> bool {
        self.system.failed_attachments_finished()
            && self.model.failed_attachment_finished()
            && self
                .addons
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .dict
                .failed_attachments_finished()
            && (!self.punct.retains_failed_attachment() || self.punct.failed_attachment_finished())
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
        Err(error) => Err(OpenError::Dict(DictError::Parse(error.to_string()))),
    }
}

/// Opens the optional user dir: `check_format`'s `user.conf` half.
///
/// A bad user dir does not fail the open — the C ABI degrades to "no user
/// state" — but it must not vanish silently either: those failures go to
/// stderr like upstream's own loader notes, and the store's wipe
/// (non-conforming profile cleaned) is logged inside `open_libpinyin`.
///
/// One failure is not a degradation. A `user.conf` whose `database
/// format:` upstream cannot map is where `check_format` `abort()`s
/// (`table_info.cpp:132`), and the class-(c) answer is a failed open:
/// [`OpenError::UnknownDatabaseFormat`], with nothing cleaned and no
/// marker written — upstream never reaches its wipe either.
fn open_user_store(
    conf: &TableConf,
    user_dir: &Path,
    originals: &mut oxpinyin_user::SystemOriginals,
    law: UserConfLaw,
    check: oxpinyin_user::ProfileCheck,
) -> Result<Option<UserStore>, OpenError> {
    let versions = SystemVersions::from_conf(conf);
    let opened = UserStore::open_libpinyin_checked(user_dir, originals, versions, law, check);
    match opened {
        Ok(store) => Ok(Some(store)),
        Err(UserStoreError::UnknownDatabaseFormat) => {
            Err(OpenError::UnknownDatabaseFormat(user_dir.to_path_buf()))
        }
        Err(UserStoreError::Journal(fault)) => Err(OpenError::Journal(fault)),
        Err(UserStoreError::LibraryChunkHeaderRead) => Err(OpenError::Dict(DictError::Library(
            LibraryError::Row(RowFault::ChunkHeaderRead),
        ))),
        Err(error) => {
            let user_dir = user_dir.display();
            eprintln!(
                "oxpinyin: user store degraded to no-user-state \
                 (user dir {user_dir}: {error})"
            );
            Ok(None)
        }
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
    /// When `user_dir` is given, the learning store opens too (its
    /// creation or read failure degrades to "no user state", matching the
    /// C ABI so a bad user dir cannot fail init), under libpinyin's
    /// `user.conf` law ([`UserConfLaw::Pinyin`]). An empty `user_dir` is
    /// given, not absent: it is the working directory, as `pinyin_init`'s
    /// `""` is, and stays relative for the life of the store.
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
    /// opened.
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
        // The init loop over the default rows runs after `check_format`
        // (`pinyin.cpp:344`, `:377-392`), so a profile judgement has been
        // written when it dies.
        if let Err((index, site)) = conf.check_default_rows(user_dir) {
            // The rows before it were mapped and their lines written; a
            // row without a user file maps its own chunk first (the log
            // `load` that asserts comes after, `pinyin.cpp:268-271`).
            let through = site == oxpinyin_data::table_info::TableConfSite::NullUserFile;
            for path in conf.absent_default_libraries(system_dir, index, through) {
                unmapped(&path);
            }
            return Err(OpenError::TableConf(TableConfError::Abort(site)));
        }
        let bigram = oxpinyin_input::path::build_path(
            system_dir,
            std::ffi::OsStr::new(&SystemDbm::Bigram.file_name()),
        );

        let (dict, libraries) = SystemDictionary::attach_with(system_dir, &conf)
            .map_err(OpenError::Dict)?
            .into_parts();
        let mut originals =
            oxpinyin_user::system_originals(libraries, UserFileLayout::from_conf(&conf));
        // Register row 96: the library loop maps each default library and
        // merges its `.dbin` in order (`pinyin.cpp:377-392`,
        // `zhuyin.cpp:318-334`); `MemoryChunk::mmap` asserts on a library
        // that is a directory (`memory_chunk.h:493`). The open stopped there;
        // the earlier libraries' logs are merged (a merge can assert first,
        // rows 91 and 108) and the fault is returned at that point, after
        // `check_format`'s writes.
        if let Some(stop) = originals.phrase_libraries().chunk_header_fault() {
            if let (Some(dir), Some(check)) = (user_dir, early)
                && let Err(error) = open_user_store(&conf, dir, &mut originals, law, check)
            {
                if let OpenError::Journal(fault) = &error {
                    for path in originals
                        .phrase_libraries()
                        .unmapped_before(fault.library(), true)
                    {
                        unmapped(path);
                    }
                }
                return Err(error);
            }
            // The libraries before it were mapped and their lines written.
            for path in originals.phrase_libraries().unmapped_before(stop, false) {
                unmapped(path);
            }
            return Err(OpenError::Dict(DictError::Library(LibraryError::Row(
                RowFault::ChunkHeaderRead,
            ))));
        }
        // The loaded-library mask is shared with the language model: an
        // unloaded library's items leave both the lookups and the unigram
        // denominator, as freeing the sub-index does upstream.
        let library_mask = AtomicU32::new(0);
        let mut lm = oxpinyin_data::lm::BigramTables::attach(&bigram).map_err(OpenError::Lm)?;
        // λ is the `gfloat` table.conf's scanner read.
        lm.set_lambda_from_conf(&conf);

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
        let user = match (user_dir, early) {
            (Some(dir), Some(check)) => {
                match open_user_store(&conf, dir, &mut originals, law, check) {
                    Ok(user) => user,
                    // Rows 91 and 108: the loop died merging library `library`'s
                    // log, after mapping it and every library before it.
                    Err(error @ OpenError::Journal(fault)) => {
                        for path in originals
                            .phrase_libraries()
                            .unmapped_before(fault.library(), true)
                        {
                            unmapped(path);
                        }
                        return Err(error);
                    }
                    Err(error) => return Err(error),
                }
            }
            _ => Some(
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
                .map_err(|error| OpenError::Dict(DictError::Parse(error.to_string())))?,
            ),
        };

        // The init loop wrote each absent library's line as it mapped it
        // (`pinyin.cpp:253-256` within `:377-392`); none of them stopped it.
        for path in user
            .as_ref()
            .map_or(&originals, UserStore::originals)
            .phrase_libraries()
            .unmapped()
        {
            unmapped(path);
        }

        let addons = RwLock::new(AddonSet {
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
            paths: StoragePaths::new(user_dir.unwrap_or_else(|| Path::new("")))
                .with_system_dirs([system_dir]),
            system: dict,
            model: lm,
            originals,
            user,
            conf,
            text_lookup_cache: TextLookupCache::default(),
            addons,
            punct,
            bumps: FacadeBumps::default(),
            library_mask,
        })
    }

    fn libraries(&self) -> &oxpinyin_data::phrase_libraries::PhraseLibraries {
        self.user
            .as_ref()
            .map_or(&self.originals, UserStore::originals)
            .phrase_libraries()
    }

    fn originals_mut(&mut self) -> &mut oxpinyin_user::SystemOriginals {
        match self.user.as_mut() {
            Some(user) => user.originals_mut(),
            None => &mut self.originals,
        }
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
    /// Configuration arrives per call (`oxpinyin-capi` passes its
    /// `Config`; simple embedders pass
    /// [`EmptyConfigSource`](oxpinyin_engine::EmptyConfigSource)), because the
    /// session reads it once at construction.
    ///
    /// # Errors
    ///
    /// Forwards any [`EngineError`] from session construction, currently
    /// infallible for the runtime's [`BigramLanguageModel`]. It always has
    /// real unigram frequencies, so [`Session::new`] skips the fallback
    /// key-cost walk and reads no dictionary or model records.
    pub fn new_session(
        &self,
        config: &impl ConfigSource,
    ) -> Result<oxpinyin_engine::SessionState, EngineError> {
        Session::new(config, self.dict(), self.lm()).map(Session::into_state)
    }

    /// Borrow the merged dictionary for one operation.
    #[must_use]
    pub fn dict(&self) -> RuntimeDict<'_> {
        RuntimeDict {
            system: self.system.read(self.libraries()),
            user: self.user.as_ref(),
            text_lookup_cache: &self.text_lookup_cache,
            addons: &self.addons,
            punct: &self.punct,
            bumps: &self.bumps,
            library_mask: &self.library_mask,
        }
    }

    /// Borrow the overlaid language model for one operation.
    #[must_use]
    pub fn lm(&self) -> RuntimeLm<'_> {
        RuntimeLm {
            inner: self.model.read(self.libraries(), &self.library_mask),
            user: self.user.as_ref(),
            libraries: self.libraries(),
            addons: &self.addons,
            bumps: &self.bumps,
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
    pub fn user_store(&self) -> Option<&UserStore> {
        self.user.as_ref()
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
        let Some(user) = self.user.as_ref().filter(|user| user.has_user_directory()) else {
            return Ok(false);
        };
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
        let Some(user) = self.user.as_mut() else {
            return Ok(false);
        };
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
                None => Err(EngineError::UserModel(error)),
            },
            Err(error) => Err(error),
        };
        if !matches!(
            &result,
            Ok(false)
                | Err(EngineError::CandidateIndexOutOfRange { .. }
                    | EngineError::StaleTrainingConstraint { .. }
                    | EngineError::StaleTrainingSpan { .. }
                    | EngineError::UnsafeTrainingPredecessor { .. }
                    | EngineError::ShortTrainingPredecessor { .. }
                    | EngineError::TrainingSetFreqMiss { .. })
        ) {
            user.mark_modified();
        }
        result
    }

    /// Apply a unigram increment with exclusive access to the storage owner.
    #[must_use]
    pub fn add_unigram_delta(&mut self, token: u32, delta: u64) -> bool {
        if !self.dict().library_visible(token >> 24) {
            return false;
        }
        if u8::try_from(token >> 24).is_ok_and(|index| self.dict().is_system_library(index))
            && let Some(user) = self.user.as_mut()
        {
            let Ok(delta) = u32::try_from(delta) else {
                return false;
            };
            return user
                .add_system_unigram_frequency(token, delta)
                .unwrap_or(false);
        }
        self.dict().add_unigram_delta(token, delta)
    }

    /// Publish dictionary remaps while retaining any unreplaced journal originals.
    pub fn install_baselines(&mut self, originals: oxpinyin_user::SystemOriginals) {
        self.originals_mut().publish_current(originals);
    }

    /// Borrow the learning store exclusively for a mutation.
    pub fn user_store_mut(&mut self) -> Option<&mut UserStore> {
        self.user.as_mut()
    }

    /// Loads addon library `index` from `system_dir`; `false` when already
    /// loaded or the library tables do not open.
    #[must_use]
    pub fn load_addon(&mut self, index: u8, system_dir: &Path) -> bool {
        self.dict().load_addon(index, system_dir)
    }

    /// Loads addon library `index` from this runtime's first configured
    /// system directory; `false` when the runtime has no system directory,
    /// the library is already loaded, or its tables do not open. Keeps the
    /// system-directory resolution here, next to the `paths` it reads,
    /// rather than duplicated in every embedder.
    #[must_use]
    pub fn load_system_addon(&mut self, index: u8) -> bool {
        let Some(system_dir) = self.paths.system_data_dirs().first() else {
            return false;
        };
        self.dict().load_addon(index, system_dir)
    }

    /// [`Runtime::load_system_addon`] with the failure kept; `Ok(false)`
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

    /// The system library files absent when this runtime opened.
    pub fn unmapped_system_libraries(&self) -> impl Iterator<Item = &Path> {
        self.libraries().unmapped()
    }

    /// The loaded, visible system libraries whose files no longer map; see
    /// [`RuntimeDict::unmappable_system_libraries`].
    #[must_use]
    pub fn unmappable_system_libraries(&self, only: Option<u8>) -> Vec<PathBuf> {
        self.dict().unmappable_system_libraries(only)
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
    /// sub-indices. Returns the compacted nibbles.
    #[must_use]
    pub fn compact_layout_failed(&mut self) -> Vec<u8> {
        self.originals_mut()
            .phrase_libraries_mut()
            .compact_layout_failed()
    }

    /// The save's current on-disk baseline for one selected system library.
    /// The pin remaps the library at its `_write_files` position and diffs
    /// that chunk against the live sub-index (`pinyin.cpp:956`,
    /// `zhuyin.cpp:589`). `Changed` carries the chunk as it is on disk now,
    /// whether or not it differs from the one the session loaded; a
    /// header-accepted layout refusal is returned for the caller to log and
    /// turn into the pin's empty old sub-index. A file `MemoryChunk::mmap`
    /// refuses is [`SaveRemapFault::Unmappable`]: the registered row 76 safe
    /// outcome (the caller writes `mmap %s failed!` and the save stops at
    /// that library). An unloaded default library, system or `USER_FILE`, is
    /// `Unloaded`: the save leaves its file alone.
    ///
    /// # Errors
    ///
    /// Returns [`SaveRemapFault::Unmappable`] when `MemoryChunk::mmap` refuses it,
    /// and [`SaveRemapFault::ChunkHeaderRead`] when it is a directory whose
    /// header read the pin asserts on (`memory_chunk.h:493`, register row
    /// 96): the save stops there.
    pub fn save_disk_baseline(&self, nibble: u8) -> Result<SaveBaseline, SaveRemapFault> {
        Self::read_save_baseline(
            self.user
                .as_ref()
                .map_or(&self.originals, UserStore::originals),
            self.unloaded_libraries(),
            nibble,
        )
    }

    /// The current loaded-library mask, snapshotted for an exclusive save.
    pub fn unloaded_libraries(&self) -> u32 {
        self.library_mask.load(Ordering::SeqCst)
    }

    /// Read a save baseline while its unique owner performs the operation.
    ///
    /// # Errors
    /// The remap faults reported by [`Self::save_disk_baseline`].
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
            Err(error) => match error.unmappable_path() {
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
    /// result is empty for a row with no system chunk and no user file
    /// (the `USER_FILE` row, the unused rows) and for an unreadable file.
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

    fn begin_remap(&self, indices: std::ops::Range<u8>, include_unloaded: bool) -> RemapWalk {
        let layout = UserFileLayout::from_conf(&self.conf);
        let indices = if self.paths.system_data_dirs().is_empty() {
            0..0
        } else {
            indices
        };
        RemapWalk {
            originals: oxpinyin_user::SystemOriginals::new(Default::default(), layout),
            indices,
            include_unloaded,
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
            if !matches!(row.file_type, FileType::SystemFile | FileType::Dictionary)
                || row.user.is_none()
                || (!walk.include_unloaded && !self.dict().library_visible(u32::from(index)))
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
                Err(error) => {
                    if let Some(path) = error.unmappable_path() {
                        return Some(RemapStep::Unmappable(index, path.to_path_buf()));
                    }
                }
            }
        }
        None
    }

    /// Ends a remap walk and returns the libraries it mapped.
    /// add_unigram_frequency's accepted deltas belong to the replaced
    /// sub-index too, so they go; overlays of every library the walk did not
    /// map stay.
    #[must_use]
    pub fn finish_system_remap(&mut self, walk: RemapWalk) -> oxpinyin_user::SystemOriginals {
        let RemapWalk { originals, .. } = walk;
        self.bumps
            .drop_libraries(|nibble| originals.contains_key(&(nibble as u8)));
        originals
    }

    fn remap_system_libraries(
        &mut self,
        indices: std::ops::Range<u8>,
        include_unloaded: bool,
    ) -> SystemRemap {
        let mut walk = self.begin_remap(indices, include_unloaded);
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
        self.dict().load_library(index)
    }

    /// Unloads default library `index`; `false` only outside the sixteen
    /// sub-indices, `true` on every call otherwise (the pin ignores what
    /// `unload` returns, `pinyin.cpp:473-474`). Which indices a facade lets
    /// through is the C ABI's rule.
    #[must_use]
    pub fn unload_library(&mut self, index: u32) -> bool {
        self.dict().unload_library(index)
    }
}

#[cfg(test)]
mod tests {
    //! White-box tests needing private field access; the public assembly
    //! seam is covered by `tests/assembly.rs`.

    use super::*;

    fn w3_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("fixtures")
            .join("w3")
            .join(oxpinyin_data::DEFAULT_STORE_EXT)
    }

    #[test]
    fn unigram_freq_live_count_ignores_bumps_until_library_is_masked() {
        let mut runtime = Runtime::open(&w3_dir(), None).unwrap();
        let token = runtime.dict().system().tokens_for_text("你").unwrap()[0];
        let library = runtime.libraries().library(token).unwrap();
        assert!(library.is_detached());
        assert_eq!(
            runtime.user_store().unwrap().unigram_delta(token).unwrap(),
            0
        );
        let initial = runtime.user_store().unwrap().system_unigram(token).unwrap();
        assert!(initial > 0);

        // Exercise an overlay alongside a live system item. Public system
        // increments now belong to the store, so seed the older overlay here.
        runtime.bumps.overlay().tokens.insert(token, 17);
        runtime.bumps.live.store(true, Ordering::SeqCst);
        for (increment, delta) in [(0, 0), (9, 9)] {
            if increment != 0 {
                assert!(runtime.add_unigram_delta(token, increment));
            }
            assert_eq!(runtime.bumps.token(token), 17);
            // A session keeps no delta for a system item: the increment is in
            // the live field.
            assert_eq!(
                runtime.user_store().unwrap().unigram_delta(token).unwrap(),
                0
            );
            assert_eq!(
                runtime.lm().unigram_freq(&PhraseToken::new(token)).unwrap(),
                Some(u64::from(initial) + delta),
                "visible live count wins, including when the original is detached"
            );

            // Mask directly to retain the bump: public unload drops it.
            runtime
                .library_mask
                .store(1 << (token >> 24), Ordering::SeqCst);
            assert_eq!(
                runtime.user_store().unwrap().system_unigram(token),
                Some(initial + delta as u32)
            );
            // No public call leaves a masked library its live buffer and its
            // bump: unload drops both. Masked here, the token prices from
            // the bump alone.
            assert_eq!(
                runtime.lm().unigram_freq(&PhraseToken::new(token)).unwrap(),
                Some(17),
                "the masked token uses the bump alone"
            );
            runtime.library_mask.store(0, Ordering::SeqCst);
        }
    }

    /// `fill_lookup` on the candidate path, as the window scan calls it
    /// (`Dictionary::lookup_candidates_into`).
    fn candidate_entries(runtime: &Runtime, text: &str) -> Vec<PhraseEntry> {
        let key = SyllableKey::from_text(text).unwrap();
        let mut out = Vec::new();
        runtime
            .dict()
            .lookup_candidates_into(&[key], &mut out)
            .unwrap();
        out
    }

    /// Every live unigram the lookup captured is what the model answers for
    /// its token in the same state; returns how many entries captured one.
    fn assert_captures_price_as_the_model(runtime: &Runtime, entries: &[PhraseEntry]) -> usize {
        let mut captured = 0;
        for entry in entries {
            let model = runtime.lm().unigram_freq(&entry.token()).unwrap();
            if let Some(live) = entry.live_unigram() {
                captured += 1;
                assert_eq!(Some(u64::from(live)), model, "token {}", entry.token());
            }
        }
        captured
    }

    fn entry_for(entries: &[PhraseEntry], token: u32) -> Option<&PhraseEntry> {
        entries.iter().find(|entry| entry.token().value() == token)
    }

    #[test]
    fn candidate_lookup_captures_the_live_unigram_the_ranking_reads() {
        let mut runtime = Runtime::open(&w3_dir(), None).unwrap();
        let token = runtime.dict().system().tokens_for_text("你").unwrap()[0];
        let library = runtime.libraries().library(token).unwrap();
        assert!(library.is_detached());

        // A pristine item of a detached library on the phrases path: the
        // capture fires, and it is the live field the model prices with.
        let entries = candidate_entries(&runtime, "ni");
        let entry = entry_for(&entries, token).expect("你 is a ni candidate");
        let live = runtime.user_store().unwrap().system_unigram(token).unwrap();
        assert!(live > 0);
        assert_eq!(
            entry.live_unigram(),
            Some(live),
            "a pristine detached item captures its live unigram"
        );
        assert_eq!(
            runtime.lm().unigram_freq(&PhraseToken::new(token)).unwrap(),
            Some(u64::from(live))
        );
        assert!(assert_captures_price_as_the_model(&runtime, &entries) >= 1);

        // A facade bump lands in the session store's live item; a window
        // collected after it captures the moved field.
        assert!(runtime.add_unigram_delta(token, 9));
        let entries = candidate_entries(&runtime, "ni");
        let bumped = entry_for(&entries, token).unwrap().live_unigram();
        assert_eq!(bumped, Some(live + 9));
        assert_captures_price_as_the_model(&runtime, &entries);

        // A training write, then the window ranked again: the recapture is
        // the trained field, not the capture the earlier window carried.
        runtime
            .user_store_mut()
            .unwrap()
            .observe_selection(oxpinyin_user::SENTENCE_START, token)
            .unwrap();
        let trained = runtime.user_store().unwrap().system_unigram(token).unwrap();
        assert!(trained > live + 9, "training raises the live field");
        let entries = candidate_entries(&runtime, "ni");
        let recaptured = entry_for(&entries, token).unwrap().live_unigram();
        assert_eq!(recaptured, Some(trained));
        assert_ne!(recaptured, bumped);
        assert_captures_price_as_the_model(&runtime, &entries);

        // A masked library leaves no entry to capture, and the model's
        // fallback answer for its token is untouched (the mask is set
        // directly, as the bump test does, so the bump stays).
        runtime
            .library_mask
            .store(1 << (token >> 24), Ordering::SeqCst);
        let entries = candidate_entries(&runtime, "ni");
        assert!(
            entries
                .iter()
                .all(|entry| entry.token().value() >> 24 != token >> 24),
            "a masked library's tokens leave the lookup"
        );
        assert_captures_price_as_the_model(&runtime, &entries);
        let delta = runtime.user_store().unwrap().unigram_delta(token).unwrap();
        assert_eq!(
            runtime.lm().unigram_freq(&PhraseToken::new(token)).unwrap(),
            Some(delta),
            "the masked token still prices from its delta and bumps"
        );
        runtime.library_mask.store(0, Ordering::SeqCst);
        let entries = candidate_entries(&runtime, "ni");
        assert_eq!(
            entry_for(&entries, token).unwrap().live_unigram(),
            Some(trained)
        );
        assert_captures_price_as_the_model(&runtime, &entries);
    }

    #[test]
    fn candidate_lookup_leaves_user_items_to_the_model() {
        let mut runtime = Runtime::open(&w3_dir(), None).unwrap();
        let ni = SyllableKey::from_text("ni").unwrap();
        let keys = [oxpinyin_user::toned_key(ni.index(), 0).unwrap()];
        // A user library's phrase arrives through the user table and
        // captures nothing; the ranking reads the model for it as before.
        let user = runtime
            .user_store_mut()
            .unwrap()
            .add_phrase_in(5, &"鎄".into(), &keys, Some(5))
            .unwrap();
        assert_eq!(user >> 24, 5);
        // An item appended to a system library is a user item for its
        // token (`with_user_item` answers it first); captured or not, it
        // prices as the model does.
        let appended = runtime
            .user_store_mut()
            .unwrap()
            .add_system_phrase_in(1, None, &"龘".into(), &keys, Some(0))
            .unwrap();
        assert_eq!(appended >> 24, 1);
        assert_eq!(
            runtime.user_store().unwrap().system_unigram(appended),
            Some(0)
        );

        let entries = candidate_entries(&runtime, "ni");
        let entry = entry_for(&entries, user).expect("the user library's phrase is a ni candidate");
        assert_eq!(
            entry.live_unigram(),
            None,
            "a user-table entry captures nothing"
        );
        assert_eq!(
            runtime.lm().unigram_freq(&PhraseToken::new(user)).unwrap(),
            Some(15)
        );
        assert!(assert_captures_price_as_the_model(&runtime, &entries) >= 1);
        let entry = entry_for(&entries, appended)
            .expect("the appended system token for 龘 is a ni candidate");
        assert_eq!(
            entry.live_unigram().map(u64::from).or(Some(0)),
            runtime
                .lm()
                .unigram_freq(&PhraseToken::new(appended))
                .unwrap()
        );
    }

    #[test]
    fn unigram_freq_preserves_user_library_counts_and_bumps() {
        let mut runtime = Runtime::open(&w3_dir(), None).unwrap();
        let ni = SyllableKey::from_text("ni").unwrap();
        let keys = [oxpinyin_user::toned_key(ni.index(), 3).unwrap()];
        for (text, count, expected) in [("鎄", 5, 15), ("龘", 0, 0)] {
            let token = runtime
                .user_store_mut()
                .unwrap()
                .add_phrase_in(5, &text.into(), &keys, Some(count))
                .unwrap();
            assert_eq!(token >> 24, 5);
            assert_eq!(runtime.user_store().unwrap().system_unigram(token), None);
            assert_eq!(
                runtime.lm().unigram_freq(&PhraseToken::new(token)).unwrap(),
                Some(expected)
            );
            assert!(runtime.add_unigram_delta(token, 7));
            assert_eq!(
                runtime.lm().unigram_freq(&PhraseToken::new(token)).unwrap(),
                Some(expected + 7)
            );
        }
    }

    #[test]
    fn unigram_freq_preserves_standalone_count_cache_reads() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = UserStore::create_standalone(&dir.path().join("user.db")).unwrap();
        let token = store.add_phrase(&"词".into(), &[7], Some(5)).unwrap();
        assert_eq!(store.system_unigram(token), None);
        let mut runtime = Runtime::open(&w3_dir(), None).unwrap();
        runtime.user = Some(store);
        for _ in 0..2 {
            assert_eq!(
                runtime.lm().unigram_freq(&PhraseToken::new(token)).unwrap(),
                Some(15)
            );
        }
        runtime
            .user_store_mut()
            .unwrap()
            .observe_selection(1, token)
            .unwrap();
        assert_eq!(
            runtime.lm().unigram_freq(&PhraseToken::new(token)).unwrap(),
            Some(15 + 483),
            "a committed write invalidates the cached count"
        );
    }

    #[test]
    fn unigram_freq_preserves_removed_and_missing_item_fallbacks() {
        let mut runtime = Runtime::open(&w3_dir(), None).unwrap();
        let ni = SyllableKey::from_text("ni").unwrap();
        let keys = [oxpinyin_user::toned_key(ni.index(), 3).unwrap()];
        let token = runtime
            .user_store_mut()
            .unwrap()
            .add_system_phrase_in(1, None, &"龘".into(), &keys, Some(0))
            .unwrap();
        assert_eq!(runtime.user_store().unwrap().system_unigram(token), Some(0));
        assert_eq!(
            runtime.lm().unigram_freq(&PhraseToken::new(token)).unwrap(),
            Some(0)
        );
        let dir = tempfile::tempdir().unwrap();
        let token = runtime.dict().system().tokens_for_text("你").unwrap()[0];
        assert_eq!(token >> 24, 1);
        let old_item = runtime
            .dict()
            .live_items()
            .read(token, |item| match item {
                oxpinyin_user::SystemItem::Shipped(view)
                | oxpinyin_user::SystemItem::Replayed(view) => view.as_bytes().to_vec(),
            })
            .found()
            .unwrap();
        let payload = oxpinyin_data::user_files::encode_log_records(&[
            oxpinyin_data::user_files::LogRecord::Remove {
                token: token & oxpinyin_user::PHRASE_MASK,
                old_item,
            },
        ])
        .unwrap();
        std::fs::write(
            dir.path().join("gb_char.dbin"),
            oxpinyin_data::chunk_format::build_memory_chunk(&payload).unwrap(),
        )
        .unwrap();
        std::fs::write(
            dir.path().join("user.conf"),
            oxpinyin_data::user_files::UserTableInfo::conform_to(&SystemVersions::from_conf(
                runtime.table_conf(),
            ))
            .to_text(),
        )
        .unwrap();
        drop(runtime);
        let mut runtime = Runtime::open(&w3_dir(), Some(dir.path())).unwrap();
        assert_eq!(runtime.user_store().unwrap().system_unigram(token), None);
        assert_eq!(
            runtime.dict().live_items().read(token, |_| ()),
            oxpinyin_user::Lookup::Removed
        );
        // The removed item leaves no entry, so nothing is captured for it;
        // the other items of the window capture what the model answers.
        let entries = candidate_entries(&runtime, "ni");
        assert!(entry_for(&entries, token).is_none());
        assert!(assert_captures_price_as_the_model(&runtime, &entries) >= 1);
        runtime.bumps.overlay().tokens.insert(token, 17);
        runtime.bumps.live.store(true, Ordering::SeqCst);
        assert_eq!(
            runtime.lm().unigram_freq(&PhraseToken::new(token)).unwrap(),
            Some(17)
        );
        runtime.user = None;
        assert_eq!(
            runtime.lm().unigram_freq(&PhraseToken::new(token)).unwrap(),
            Some(17)
        );
        assert_eq!(
            runtime
                .lm()
                .unigram_freq(&PhraseToken::new(u32::MAX))
                .unwrap(),
            Some(0)
        );
    }

    #[test]
    fn candidate_lookup_omits_prices_without_changing_live_priced_reads() {
        let mut runtime = Runtime::open(&w3_dir(), None).unwrap();

        let ni = SyllableKey::from_text("ni").unwrap();
        let token = runtime.dict().system().tokens_for_text("你").unwrap()[0];
        assert!(
            runtime
                .dict()
                .system()
                .libraries()
                .library(token)
                .unwrap()
                .is_detached()
        );
        assert!(
            runtime
                .dict()
                .lookup(&[ni])
                .unwrap()
                .iter()
                .any(|entry| entry.token().value() == token),
            "a transferred chunk routes through its live owner before history exists"
        );

        let keys = [oxpinyin_user::toned_key(ni.index(), 3).unwrap()];
        runtime
            .user_store_mut()
            .unwrap()
            .add_system_phrase_in(1, Some(token), &"你".into(), &keys, Some(5))
            .unwrap();
        // A newly appended system token is also indexed by the user table.
        // It must be priced once when that table contributes it.
        let indexed_system = runtime
            .user_store_mut()
            .unwrap()
            .add_system_phrase_in(1, None, &"龘".into(), &keys, Some(5))
            .unwrap();
        let mut candidates = Vec::new();
        assert!(
            !runtime
                .dict()
                .lookup_candidates_into(&[ni], &mut candidates)
                .unwrap()
        );
        assert!(!candidates.is_empty());
        assert!(
            candidates
                .iter()
                .all(|entry| entry.pronunciation_possibility().is_none())
        );
        PRICING_EVALUATIONS.with(|count| count.set(0));
        let priced = runtime.dict().lookup(&[ni]).unwrap();
        let evaluated = PRICING_EVALUATIONS.with(std::cell::Cell::get);
        let priced_system_results = priced
            .iter()
            .filter(|entry| {
                runtime
                    .dict()
                    .live_items()
                    .is_system_library((entry.token().value() >> 24) as u8)
                    && entry.pronunciation_possibility().is_some()
            })
            .count();
        assert!(priced_system_results > 0);
        assert!(
            priced
                .iter()
                .any(|entry| entry.token().value() == indexed_system
                    && entry.pronunciation_possibility().is_some()),
            "the user-index result naming a system token is priced"
        );
        assert_eq!(
            evaluated, priced_system_results,
            "each system result is priced once"
        );
        let rows = |entries: &[PhraseEntry]| {
            entries
                .iter()
                .map(|entry| (entry.token(), entry.text().clone()))
                .collect::<Vec<_>>()
        };
        assert_eq!(rows(&candidates), rows(&priced));
        assert!(
            priced
                .iter()
                .find(|entry| entry.token().value() == token)
                .unwrap()
                .pronunciation_possibility()
                .is_some()
        );
        assert!(
            runtime
                .dict()
                .trellis_records(&[ni], &[3])
                .unwrap()
                .iter()
                .find(|entry| entry.token().value() == token)
                .unwrap()
                .pronunciation_possibility()
                .is_some()
        );
        assert!(runtime.unload_library(1));
        runtime
            .dict()
            .lookup_candidates_into(&[ni], &mut candidates)
            .unwrap();
        assert!(
            candidates
                .iter()
                .all(|entry| entry.token().value() != token)
        );
        assert!(runtime.load_library(1));
        runtime
            .dict()
            .lookup_candidates_into(&[ni], &mut candidates)
            .unwrap();
        assert!(
            candidates
                .iter()
                .any(|entry| entry.token().value() == token)
        );
    }

    #[test]
    fn zhuyin_open_holds_no_addon_or_punct_handles() {
        let system = w3_dir();
        let pinyin = Runtime::open(&system, None).expect("pinyin fixture opens");
        assert!(
            pinyin.dict().addons.read().unwrap().dict.has_open_indexes(),
            "vacuity guard: pinyin eagerly opens the fixture's addon indexes"
        );
        assert!(
            pinyin.dict().punct.is_open(),
            "vacuity guard: pinyin eagerly opens the fixture's punctuation table"
        );

        let user = tempfile::tempdir().unwrap();
        for user_dir in [None, Some(user.path())] {
            let mut zhuyin = Runtime::open_with_law(&system, user_dir, UserConfLaw::Zhuyin)
                .expect("zhuyin fixture opens");
            // An empty loaded-library set would also pass before the fix;
            // inspect the actual index handles, not just visible items.
            assert!(!zhuyin.dict().addons.read().unwrap().dict.has_open_indexes());
            assert!(!zhuyin.dict().punct.is_open());

            // Default user library 5 (addon.bin) is separate from the
            // addon DBM pair and remains available to zhuyin imports.

            let ni = SyllableKey::from_text("ni").unwrap();
            let hao = SyllableKey::from_text("hao").unwrap();
            let keys = [
                oxpinyin_user::toned_key(ni.index(), 3).unwrap(),
                oxpinyin_user::toned_key(hao.index(), 3).unwrap(),
            ];
            let token = zhuyin
                .user_store_mut()
                .unwrap()
                .add_phrase_in(5, &"你鎄".into(), &keys, Some(5))
                .unwrap();
            assert_eq!(token >> 24, 5);
            assert!(
                zhuyin
                    .dict()
                    .tokens_for_text("你鎄")
                    .contains(&PhraseToken::new(token))
            );
        }

        // A table.conf whose addon 4 names a non-stock chunk: the stock
        // rows would say `art.bin`, so a facade built from them would
        // lose the configuration.
        let system = tempfile::tempdir().unwrap();
        for entry in std::fs::read_dir(w3_dir()).unwrap() {
            let path = entry.unwrap().path();
            std::fs::copy(&path, system.path().join(path.file_name().unwrap())).unwrap();
        }
        let conf_path = system.path().join("table.conf");
        let conf = std::fs::read_to_string(&conf_path).unwrap();
        let edited = conf.replace(
            "addon 4 art.table art.bin NULL DICTIONARY",
            "addon 4 art.table art-local.bin NULL DICTIONARY",
        );
        assert_ne!(conf, edited, "vacuity guard: the fixture names art.bin");
        std::fs::write(&conf_path, edited).unwrap();

        for user_dir in [None, Some(user.path())] {
            let zhuyin = Runtime::open_with_law(system.path(), user_dir, UserConfLaw::Zhuyin)
                .expect("edited fixture opens");
            let addons = zhuyin.dict().addons.read().unwrap();
            assert!(
                !addons.dict.has_open_indexes(),
                "no addon index handle is open"
            );
            let row = addons.dict.row(4).expect("sub-index 4 is in range");
            assert_eq!(
                row.system,
                Some(oxpinyin_data::table_info::FileName::from_bytes(
                    b"art-local.bin"
                )),
                "the configured addon row is kept, not the stock one"
            );
            assert_eq!(Some(row), zhuyin.table_conf().addon_row(4));
            assert!(!zhuyin.dict().punct.is_open());
        }
    }

    #[test]
    fn tokens_for_text_reads_live_rows_without_rebuilding_user_lookup() {
        let dir = tempfile::tempdir().unwrap();
        let mut runtime = Runtime::open(&w3_dir(), Some(dir.path())).unwrap();

        let phrase = "你鎄";
        assert!(runtime.dict().tokens_for_text(phrase).is_empty());

        // Each operation re-borrows the same storage owner. Exact-text
        // queries still read their live rows after writes.

        let keys: Vec<_> = ["ni", "hao"]
            .into_iter()
            .map(|text| PinyinKey::try_from(SyllableKey::from_text(text).unwrap().index()).unwrap())
            .collect();

        let token = runtime
            .user_store_mut()
            .unwrap()
            .add_phrase(&oxpinyin_core::PhraseText::from(phrase), &keys, None)
            .unwrap();
        assert_eq!(
            runtime.dict().tokens_for_text(phrase),
            [PhraseToken::new(token)]
        );
        let syllables = [
            SyllableKey::from_text("ni").unwrap(),
            SyllableKey::from_text("hao").unwrap(),
        ];
        assert!(
            runtime
                .dict()
                .user_lookup()
                .unwrap()
                .lookup(&syllables)
                .iter()
                .any(|entry| entry.token().value() == token && entry.text() == phrase)
        );
        assert_eq!(
            runtime
                .dict()
                .user_lookup()
                .unwrap()
                .phrase_text(token)
                .unwrap(),
            phrase
        );
        let row_generation = runtime.user_store_mut().unwrap().phrase_table_generation();
        let row_tokens = runtime
            .dict()
            .text_lookup_cache
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .tokens
            .as_ptr();
        assert_eq!(
            runtime
                .user_store_mut()
                .unwrap()
                .add_phrase(&oxpinyin_core::PhraseText::from(phrase), &keys, Some(5))
                .unwrap(),
            token
        );
        assert_eq!(
            runtime.user_store_mut().unwrap().phrase_table_generation(),
            row_generation
        );
        assert_eq!(
            runtime.dict().tokens_for_text(phrase),
            [PhraseToken::new(token)]
        );
        assert_eq!(
            runtime
                .dict()
                .text_lookup_cache
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .generation,
            row_generation
        );
        assert_eq!(
            runtime
                .dict()
                .text_lookup_cache
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .tokens
                .as_ptr(),
            row_tokens
        );
        assert!(
            runtime
                .user_store_mut()
                .unwrap()
                .remove_user_phrase(token)
                .unwrap()
        );
        assert!(runtime.dict().tokens_for_text(phrase).is_empty());

        // A later operation re-borrows the owner without rebuilding query state.

        let imported = runtime
            .user_store_mut()
            .unwrap()
            .add_phrase_in(7, &oxpinyin_core::PhraseText::from(phrase), &keys, Some(5))
            .unwrap();
        assert_eq!(
            runtime.dict().tokens_for_text(phrase),
            [PhraseToken::new(imported)]
        );
        runtime
            .user_store_mut()
            .unwrap()
            .mask_out(u32::MAX, imported)
            .unwrap();
        assert!(runtime.dict().tokens_for_text(phrase).is_empty());
        assert!(
            runtime
                .dict()
                .user_lookup()
                .unwrap()
                .tokens_for_ucs4(&phrase.chars().map(u32::from).collect::<Vec<_>>())
                .is_empty()
        );
    }

    #[test]
    fn tokens_for_text_retains_the_row_after_a_system_reading_merge() {
        let dir = tempfile::tempdir().unwrap();
        let mut runtime = Runtime::open(&w3_dir(), Some(dir.path())).unwrap();

        let tokens = runtime.dict().tokens_for_text("你好");
        let token = tokens[0].value();
        let library = u8::try_from(token >> 24).unwrap();
        let keys: Vec<_> = ["ni", "hao"]
            .into_iter()
            .map(|text| PinyinKey::try_from(SyllableKey::from_text(text).unwrap().index()).unwrap())
            .collect();

        let generation = runtime.user_store_mut().unwrap().phrase_table_generation();
        runtime
            .user_store_mut()
            .unwrap()
            .add_system_phrase_in(library, Some(token), &"你好".into(), &keys, Some(5))
            .unwrap();
        assert_eq!(
            runtime.user_store_mut().unwrap().phrase_table_generation(),
            generation
        );
        assert_eq!(runtime.dict().tokens_for_text("你好"), tokens);
        let added = runtime
            .user_store_mut()
            .unwrap()
            .add_system_phrase_in(library, None, &"你鎄".into(), &keys, Some(5))
            .unwrap();
        assert!(runtime.user_store_mut().unwrap().phrase_table_generation() > generation);
        assert_eq!(
            runtime.dict().tokens_for_text("你鎄"),
            [PhraseToken::new(added)]
        );
    }

    #[test]
    fn suggestion_rows_group_by_library_then_walk_the_ucs4_keys() {
        // The pin's list: system library groups first, the user library
        // last, each in the DBM's byte-lexical UCS-4 order — not text
        // (UTF-8 / code point) order. U+4E50 sorts before U+4F2D by code
        // point but after it by little-endian bytes (0x50 > 0x2D).
        let system = vec![0x0200_0001, 0x0100_0010];
        let user_rows = vec![
            (0x0700_0001, oxpinyin_data::ucs4_walk_key("中乐")), // U+4E50
            (0x0700_0002, oxpinyin_data::ucs4_walk_key("中伭")), // U+4F2D
        ];
        let merged = merge_suggestion_rows(&system, &user_rows);
        assert_eq!(merged, [0x0100_0010, 0x0200_0001, 0x0700_0002, 0x0700_0001]);
    }

    #[test]
    fn suggestion_merge_retains_missing_tokens_and_duplicate_occurrences() {
        let system = [0x0100_0001];
        let user_rows = [(0x0100_0001, vec![0xff; 4]), (0x0700_0002, vec![0; 4])];
        assert_eq!(
            merge_suggestion_rows(&system, &user_rows),
            [0x0100_0001, 0x0100_0001, 0x0700_0002]
        );
    }
}

/// The standalone pin test programs use the same data and model readers.
#[cfg(feature = "tools")]
pub mod tool;
