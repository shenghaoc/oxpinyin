//! The concrete engine assembly shared by every oxpinyin consumer.
//!
//! Wiring only: this crate opens a system data directory the way
//! `pinyin_init` does — the pinyin and phrase DBMs, the per-library
//! chunk files, `bigram.db`, `punct.bin`, the addon DBM pair, λ from
//! `table.conf` — installs the optional user store, and hands out
//! [`Session`]s over the merged backends. On each backend that
//! directory is the backend's native data directory — the one a
//! libpinyin build on the same backend uses, or what
//! `oxpinyin-datagen compile` writes. Nothing is scanned at open: every reader in `oxpinyin-data`
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
// Constitution §4, mechanically: library builds may not unwrap, expect,
// or panic. Inline #[cfg(test)] modules are exempt (see the allow below
// their declaration); tests/, benches/ and examples/ are separate crates.
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::panic))]
#![cfg_attr(not(test), deny(clippy::panic_in_result_fn))]
#![warn(missing_docs)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

/// `PHRASE_INDEX_LIBRARY_COUNT` (`phrase_index.h`): sixteen sub-indices.
const PHRASE_INDEX_LIBRARY_COUNT: u32 = 16;
use std::sync::{Arc, Mutex, RwLock};

use oxpinyin_core::scoring::key_cost_table;
use oxpinyin_core::{
    Cost, Dictionary, LanguageModel, MergedGram, NbestStepCosts, PhraseEntry, PhraseIndexRanges,
    PhraseItemRead, PhraseToken, SyllableKey, TrellisItem, TrellisPathMemo, UserCountDelta,
};
pub use oxpinyin_data::phrase_library::{LibraryError, PhraseItemView, RowFault};
pub use oxpinyin_data::table_info::{FileType, TableConfSite};
pub use oxpinyin_data::table_info::{TableConf, TableConfError};
use oxpinyin_data::user_files::{SystemVersions, UserFileLayout};
use oxpinyin_data::{
    AddonDictionary, BigramLanguageModel, DictError, LmError, PunctTable, SystemDbm,
    SystemDictionary, default_store_file,
};
use oxpinyin_engine::{ConfigSource, EngineError, Session, StoragePaths};
use oxpinyin_user::{PinyinKey, UserConfLaw, UserLookup, UserStore, UserStoreError};

/// File name of a *standalone* user store — `user_store.<ext>`, the
/// extension naming the compiled-in backend (`kct` Kyoto Cabinet, `tkt`
/// tkrzw, `db` Berkeley DB).
///
/// The **runtime's** user store no longer lives in such a file: it
/// persists in the backend's native user-dir file set
/// (`oxpinyin_user::persistence`) — the same files a libpinyin build on
/// this backend keeps. The helper is kept as a public name for
/// standalone stores; no in-tree caller remains.
#[must_use]
pub fn user_store_file() -> String {
    default_store_file("user_store")
}

/// Orders the system and user `suggest_after` rows the way
/// `_compute_predicted_prefix_candidates` receives them
/// (`pinyin.cpp:2371-2405`): `FacadePhraseTable3::search_suggestion` runs
/// the system phrase table then the user one, each filing tokens into
/// its library's array in the DBM's cursor order (byte-lexical over the
/// UCS-4 keys), and `reduce_tokens` concatenates the arrays library by
/// library. So: grouped by library nibble ascending — the system
/// libraries 1–4, then the user library 7 — and inside a group the UCS-4
/// walk order, stored token order within one text. The system rows arrive
/// in that order already (`SystemDictionary::suggest_after`); the user
/// rows are ordered by little-endian UCS-4 keys, preserving the stored
/// token order within each record.
///
/// Lives beside the other pinned data+user compositions this crate holds
/// (the [`RuntimeLm`] overlay feed), so the C-ABI facades share one
/// suggestion ordering instead of each assembling an equivalent against
/// raw UCS-4 keys.
#[must_use]
pub fn merge_suggestion_rows(
    system: &[(u32, String)],
    user_rows: &[(u32, Vec<u8>)],
) -> Vec<(u32, Option<String>)> {
    let mut user = user_rows.to_vec();
    user.sort_by(|a, b| a.1.cmp(&b.1));
    let mut merged = Vec::with_capacity(system.len() + user.len());
    merged.extend(
        system
            .iter()
            .map(|(token, text)| (*token, oxpinyin_data::ucs4_walk_key(text))),
    );
    merged.extend(user);
    merged.sort_by_key(|(token, _)| token >> 24);
    merged
        .into_iter()
        .map(|(token, key)| {
            (
                token,
                oxpinyin_data::row_format::phrase_index::decode_ucs4_key(&key),
            )
        })
        .collect()
}

// ── Open errors ─────────────────────────────────────────────────────────

/// Why a runtime could not be opened.
///
/// Deliberately distinguishes missing files ([`OpenError::Missing`]) from
/// present-but-unreadable/unparsable ones, so adapters can raise
/// `FileNotFoundError`, `OSError` and friends without string matching.
#[derive(Debug)]
#[non_exhaustive]
pub enum OpenError {
    /// A required path does not exist.
    Missing(PathBuf),
    /// A required path exists but could not be read.
    Io(PathBuf, std::io::Error),
    /// A required path exists and is readable but is not a regular file
    /// (a directory, socket, FIFO, …).
    ///
    /// Its own variant rather than an [`OpenError::Io`] carrying a
    /// synthesized `std::io::Error`: no operating-system call failed, so
    /// there is no `ErrorKind` or `raw_os_error` to report and nothing
    /// for a caller to inspect beyond the path.
    NotRegularFile(PathBuf),
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
    /// A `SYSTEM_FILE`/`DICTIONARY` library's user `.dbin` carries a
    /// non-null-token `MODIFY_HEADER` record. `PhraseIndexLogger::next_record`
    /// asserts `token == null_token` there (`phrase_index_logger.h:202`),
    /// reached while the library loads at init. The class-(c) availability
    /// answer: the open fails and the C ABI answers NULL, where upstream
    /// takes the process down.
    CorruptLogHeader(PathBuf),
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
            _ => None,
        }
    }

    /// Whether the open failed on a system library that is a directory the
    /// pin's `MemoryChunk::mmap` asserts on (`memory_chunk.h:493`): class
    /// (c), answered with NULL and one warning.
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
            Self::Missing(path) => write!(f, "missing file: {}", path.display()),
            Self::Io(path, error) => write!(f, "cannot read {}: {error}", path.display()),
            Self::NotRegularFile(path) => {
                write!(f, "cannot read {}: not a regular file", path.display())
            }
            Self::Dict(error) => write!(f, "dictionary error: {error}"),
            Self::Lm(error) => write!(f, "language model error: {error}"),
            Self::UnknownDatabaseFormat(path) => write!(
                f,
                "unknown database format in {} (upstream aborts, table_info.cpp:132)",
                path.join("user.conf").display()
            ),
            Self::TableConf(error) => write!(f, "table.conf: {error}"),
            Self::CorruptLogHeader(path) => write!(
                f,
                "a system library's user .dbin under {} carries a non-null-token \
                 MODIFY_HEADER (upstream asserts, phrase_index_logger.h:202)",
                path.display()
            ),
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
    pub text: String,
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

/// One token's dictionary introspection: the `pinyin_token_*` read
/// surface's answer, assembled per library seam.
pub struct TokenIntrospection {
    /// The phrase text.
    pub text: String,
    /// Pronunciations as `(structured keys, count)` pairs.
    pub pronunciations: Vec<(Vec<PinyinKey>, u64)>,
}

/// The pin's query keys for `syllables`.
#[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
fn chewing_query(syllables: &[SyllableKey]) -> Option<Vec<oxpinyin_core::ChewingKey>> {
    syllables
        .iter()
        .map(|key| oxpinyin_core::ChewingKey::from_pinyin(key.text()))
        .collect()
}

// ── Merged backends ─────────────────────────────────────────────────────

/// The generation-stamped user-phrase lookup cache shared by clones.
type LookupCache = Arc<Mutex<Option<(u64, Arc<UserLookup>)>>>;

// Only the trellis consumes this unreduced index. Ordinary UserLookup stays
// unchanged. The initial projection bounds partial and complete queries alike.
type TrellisBucket = Vec<(Vec<oxpinyin_core::ChewingKey>, PhraseEntry)>;
type TrellisIndex = HashMap<Vec<u8>, TrellisBucket>;
type TrellisCache = Arc<Mutex<Option<(u64, Arc<TrellisIndex>)>>>;

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
#[derive(Clone)]
pub struct RuntimeDict {
    system: Arc<SystemDictionary>,
    user: Option<UserStore>,
    user_lookup_cache: LookupCache,
    trellis_cache: TrellisCache,
    addons: Arc<RwLock<AddonSet>>,
    punct: Arc<PunctTable>,
    /// The `add_unigram_frequency` overlay: in-memory per-token deltas
    /// over the baked counts, and the facade-total bump it applies
    /// unconditionally once the token's library is loaded
    /// (`phrase_index.h:632` — before the item-level dispatch, so an
    /// absent-token add still moves the total). Upstream writes the same
    /// deltas into its in-memory `FacadePhraseIndex` and nothing persists
    /// them (`pinyin_save` flushes user data exclusively), so the overlay
    /// is the faithful shape: shared per context with the language model,
    /// gone at fini.
    bumps: Arc<FacadeBumps>,
    /// The loaded-library mask: bit `n` **set** = library `n` unloaded
    /// (matches `library_visible` at :344 and the query surface —
    /// `library_visible_token`, `visible_item_count`, and the
    /// `unload_library` setter — that all consult it).  The pinyin ABI only
    /// ever sets bit 2 (`GBK_DICTIONARY`, `pinyin.cpp:464-472`); the zhuyin
    /// ABI any bit but 1 (`zhuyin.cpp:378-388`).
    library_mask: Arc<AtomicU32>,
    /// Seqlock epoch bracketing every [`RuntimeDict::load_library`] /
    /// [`RuntimeDict::unload_library`] mask flip: each bumps it once
    /// before and once after the flip. Readers that must not observe a
    /// torn visibility window — the key-cost walk behind
    /// [`Runtime::new_session`] — load it before and after the walk and
    /// discard the walk when it moved. The protocol leans on the
    /// `library_mask` reads inside those walks staying `SeqCst` (the
    /// single total order keeps `SeqCst` operations in program order),
    /// so keep every mask read `SeqCst`.
    library_epoch: Arc<AtomicU64>,
}

impl RuntimeDict {
    /// The user pinyin table's search for `syllables`, as the pin's
    /// `ChewingLargeTable2::search` runs it: the table entry it leaves
    /// behind is what a later `mask_out` walk starts from (#785).
    #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
    fn note_user_search(&self, syllables: &[SyllableKey], lookup: &UserLookup) {
        if let Some(store) = self.user.as_ref() {
            store.note_pinyin_search(|| chewing_query(syllables), || lookup.has_record(syllables));
        }
    }

    /// The query's toned keys; `None` when a syllable is no key.
    fn trellis_query(
        syllables: &[SyllableKey],
        tones: &[u8],
    ) -> Option<Vec<oxpinyin_core::ChewingKey>> {
        syllables
            .iter()
            .enumerate()
            .map(|(index, key)| {
                oxpinyin_core::ChewingKey::from_pinyin(key.text())
                    .map(|key| key.with_tone(tones.get(index).copied().unwrap_or(0)))
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
    ) -> Result<(), DictError> {
        let initial: Vec<_> = query.iter().map(|key| key.initial).collect();
        if let Some(bucket) = self.user_trellis_index()?.get(&initial) {
            ranges.convert(
                bucket
                    .iter()
                    .filter(|(keys, entry)| {
                        query.len() == keys.len()
                            && query.iter().zip(keys).all(|(query, stored)| {
                                query.initial == stored.initial
                                    && ((query.middle == stored.middle
                                        && query.final_ == stored.final_)
                                        || (query.middle == 0 && query.final_ == 0)
                                        || (stored.middle == 0 && stored.final_ == 0))
                                    && (query.tone == stored.tone
                                        || query.tone == 0
                                        || stored.tone == 0)
                            })
                            && self.token_ranged(entry.token().value())
                    })
                    .map(|(_, entry)| (entry.token().value(), Some(()))),
            );
        }
        Ok(())
    }

    /// The query keys the effective-item overlay prices against; `None`
    /// when the store holds no system items or a syllable is no key.
    fn effective_query(&self, syllables: &[SyllableKey]) -> Option<Vec<oxpinyin_core::ChewingKey>> {
        self.user
            .as_ref()
            .filter(|store| store.has_system_items())?;
        syllables
            .iter()
            .map(|key| oxpinyin_core::ChewingKey::from_pinyin(key.text()))
            .collect()
    }

    /// `(matched, total)` of `token`'s effective system item over `query`,
    /// the wrapping `guint32` sums of `phrase_index.h:136-163`.
    fn effective_possibility(
        &self,
        query: &[oxpinyin_core::ChewingKey],
        token: u32,
    ) -> Option<(u32, u32)> {
        self.user
            .as_ref()?
            .with_effective_system_item(token, |item| {
                let mut total = 0_u32;
                let mut matched = 0_u32;
                for (keys, frequency) in &item.prons {
                    total = total.wrapping_add(*frequency);
                    if query.len() == keys.len()
                        && query.iter().zip(keys).all(|(q, bits)| {
                            let s = oxpinyin_core::ChewingKey::from_packed(*bits);
                            q.initial == s.initial
                                && ((q.middle == s.middle && q.final_ == s.final_)
                                    || (q.middle == 0 && q.final_ == 0)
                                    || (s.middle == 0 && s.final_ == 0))
                                && (q.tone == s.tone || q.tone == 0 || s.tone == 0)
                        })
                    {
                        matched = matched.wrapping_add(*frequency);
                    }
                }
                (matched, total)
            })
    }

    /// What [`Self::priced_token`] reads of `syllables` whatever the
    /// token: the user table's lookup and the path's tone-less keys.
    fn path_pricing(&self, syllables: &[SyllableKey]) -> Result<PathPricing, DictError> {
        Ok(PathPricing {
            user: self.user_lookup()?.lookup(syllables),
            keys: SystemDictionary::trellis_keys(syllables),
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
                    chars: entry.text().chars().count().try_into().unwrap_or(u32::MAX),
                    pronunciation: entry.pronunciation_possibility(),
                },
                text: want_text.then(|| entry.text().to_owned()),
            }
        } else {
            let Some(item) = path
                .keys
                .as_deref()
                .and_then(|keys| self.system.trellis_item_keys(keys, token))
            else {
                return Ok(None);
            };
            PricedToken {
                item,
                text: if want_text {
                    match self.system.phrase_text(token) {
                        Some(text) => Some(text),
                        None => return Ok(None),
                    }
                } else {
                    None
                },
            }
        };
        // The effective-item overlay prices against the same keys, when
        // the store holds system items.
        if let Some(query) = path.keys.as_deref()
            && self.user.as_ref().is_some_and(UserStore::has_system_items)
            && let Some((matched, total)) = self.effective_possibility(query, token)
        {
            priced.item.pronunciation = Some((u64::from(matched), u64::from(total)));
        }
        if self.library_mask.load(Ordering::SeqCst) != 0 && !self.library_visible_token(token) {
            return Ok(None);
        }
        Ok(Some(priced))
    }

    fn user_lookup(&self) -> Result<Arc<UserLookup>, DictError> {
        let Some(store) = self.user.as_ref() else {
            return Ok(Arc::new(UserLookup::empty()));
        };
        let mut cache = self
            .user_lookup_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        UserLookup::refresh_in(&mut cache, store)
            .map_err(|error| DictError::Parse(error.to_string()))?;
        Ok(cache.as_ref().map_or_else(
            || Arc::new(UserLookup::empty()),
            |(_, lookup)| Arc::clone(lookup),
        ))
    }

    fn user_trellis_index(&self) -> Result<Arc<TrellisIndex>, DictError> {
        let Some(store) = self.user.as_ref() else {
            return Ok(Arc::new(HashMap::new()));
        };
        let mut cache = self
            .trellis_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let generation = store.phrase_generation();
        if let Some((seen, index)) = cache.as_ref()
            && *seen == generation
        {
            return Ok(Arc::clone(index));
        }
        let mut index: TrellisIndex = HashMap::new();
        for phrase in store
            .index_phrases()
            .map_err(|error| DictError::Parse(error.to_string()))?
        {
            for pronunciation in phrase
                .pronunciations()
                .iter()
                .filter(|reading| reading.indexed())
            {
                let Some(pinyin) = pronunciation.render_pinyin_toneless() else {
                    continue;
                };
                let Some(keys) = pinyin
                    .split('\'')
                    .zip(pronunciation.keys())
                    .map(|(text, key)| {
                        oxpinyin_core::ChewingKey::from_pinyin(text)
                            .map(|key_| key_.with_tone(oxpinyin_user::key_tone(*key)))
                    })
                    .collect::<Option<Vec<_>>>()
                else {
                    continue;
                };
                let initial = keys.iter().map(|key| key.initial).collect();
                let entry =
                    PhraseEntry::new(PhraseToken::new(phrase.token()), phrase.text().to_owned());
                index.entry(initial).or_default().push((keys, entry));
            }
        }
        // 074a2219 chewing_large_table2.h:208-231: add_index keeps each
        // entry ordered by pinyin_exact_compare2 (pinyin_phrase3.h:33-63:
        // initials, then middles and finals, then tones), then token.
        for bucket in index.values_mut() {
            bucket.sort_by_cached_key(|(keys, entry)| {
                (
                    keys.iter().map(|key| key.initial).collect::<Vec<_>>(),
                    keys.iter()
                        .map(|key| (key.middle, key.final_))
                        .collect::<Vec<_>>(),
                    keys.iter().map(|key| key.tone).collect::<Vec<_>>(),
                    entry.token().value(),
                )
            });
        }
        let index = Arc::new(index);
        *cache = Some((generation, Arc::clone(&index)));
        Ok(index)
    }

    /// The underlying system table set, without the user overlay.
    #[must_use]
    pub fn system(&self) -> &SystemDictionary {
        &self.system
    }

    /// Punctuation strings registered for `token`, if the punct table
    /// shipped with the system dir — one point read; a malformed value
    /// answers none, as upstream's `get_all_punctuations` failing does.
    #[must_use]
    pub fn punctuations(&self, token: u32) -> Vec<String> {
        self.punct.punctuations(token).unwrap_or_default()
    }

    /// Loads addon library `index` from `system_dir`; `false` when already
    /// loaded or the tables are missing/unopenable.
    #[must_use]
    pub fn load_addon(&self, index: u8, system_dir: &Path) -> bool {
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
    pub fn load_addon_reporting(&self, index: u8, system_dir: &Path) -> Result<bool, LibraryError> {
        let mut addons = self
            .addons
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        addons.load_reporting(index, system_dir)
    }

    /// The system library files absent when the runtime opened, in the
    /// pin's load order.
    #[must_use]
    pub fn unmapped_system_libraries(&self) -> &[PathBuf] {
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
    pub fn unload_addon(&self, index: u8) -> bool {
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
    pub fn unload_library(&self, index: u32) -> bool {
        if index >= PHRASE_INDEX_LIBRARY_COUNT {
            return false;
        }
        let mask = 1u32 << index;
        // The flip is bracketed by epoch bumps so a concurrent key-cost
        // walk can detect that visibility moved under it and retry
        // (see `library_epoch`).
        self.library_epoch.fetch_add(1, Ordering::SeqCst);
        let newly_unloaded = self.library_mask.fetch_or(mask, Ordering::SeqCst) & mask == 0;
        // The freed sub-index takes its accepted bumps with it, and a
        // reload maps the library's items as stored (`phrase_index.cpp:261-269`).
        if newly_unloaded {
            self.bumps.drop_libraries(|nibble| nibble == index);
        }
        self.library_epoch.fetch_add(1, Ordering::SeqCst);
        true
    }

    /// Re-loads library `index` after an unload — upstream re-attaches
    /// the sub-index from disk and answers `true`; already-loaded (mask
    /// clear) answers `false` (`pinyin.cpp:234-243`). A library that was
    /// never unloaded is already loaded: the system tables load at init, so
    /// upstream's already-loaded rule answers `false` there too.
    #[must_use]
    pub fn load_library(&self, index: u32) -> bool {
        // The system tables (1, 2, 4) and the USER_FILE library (7) all load
        // at init (the default-tables loop includes the USER_DICTIONARY row —
        // measured `load(7)` = false on the pin), so the already-loaded rule
        // answers `false` for every index an unload has not cleared.
        if index >= PHRASE_INDEX_LIBRARY_COUNT {
            return false;
        }
        // Bracketed like `unload_library`'s flip (see `library_epoch`).
        self.library_epoch.fetch_add(1, Ordering::SeqCst);
        let newly_loaded = self
            .library_mask
            .fetch_and(!(1u32 << index), Ordering::SeqCst)
            & (1u32 << index)
            != 0;
        self.library_epoch.fetch_add(1, Ordering::SeqCst);
        newly_loaded
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
    pub fn add_unigram_delta(&self, token: u32, delta: u64) -> bool {
        if !self.library_visible_token(token) {
            return false;
        }
        let found = {
            let system_found = self.system_unigram_count(token).is_some();
            let addons = self
                .addons
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let addon_found = addons.unigram_freq(token).is_some();
            let user_found = self
                .user
                .as_ref()
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
        let user_bound = match self
            .user
            .as_ref()
            .map(|user| user.unigram_total())
            .transpose()
        {
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
            .as_ref()
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
                .as_ref()
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

    /// The system item's stored unigram for `token` —
    /// `PhraseItem::get_unigram_frequency`, `gen_unigram`'s `+1` included —
    /// the field the pin's predicted-candidate law reads
    /// (`pinyin.cpp:1811-1824`). `None` when the token's library is
    /// unloaded or owns no such item.
    #[must_use]
    pub fn system_unigram_count(&self, token: u32) -> Option<u64> {
        if !self.library_visible_token(token) {
            return None;
        }
        self.system.unigram_count(token).or_else(|| {
            self.user
                .as_ref()?
                .system_item_override(token)
                .ok()?
                .map(|item| u64::from(item.unigram))
        })
    }

    /// The visible item count for the amplified-law denominator: the
    /// items of every library that is not unloaded. Per-library counts
    /// are tallied once and cached.
    #[must_use]
    pub fn visible_item_count(&self) -> u64 {
        self.system
            .item_count_where(|nibble| self.library_visible(u32::from(nibble)))
    }

    /// The token's library-nibble visibility, for entry-level filters.
    #[must_use]
    pub fn library_visible_token(&self, token: u32) -> bool {
        self.library_visible(token >> 24)
    }

    /// Whether phrase-table search has an enabled token array for this library.
    /// Unlike visibility alone, this excludes libraries never loaded into the
    /// facade (074a2219 phrase_index.h:762-772; phrase_large_table3.cpp:40-42).
    #[must_use]
    pub fn phrase_table_token_enabled(&self, token: u32) -> bool {
        let nibble = token >> 24;
        self.library_visible_token(token)
            && (self.system.library_loaded(token)
                || ((5..=7).contains(&nibble)
                    && self
                        .user
                        .as_ref()
                        .is_some_and(|store| store.has_user_library(token.to_be_bytes()[0]))))
    }

    /// `FacadePhraseIndex::get_phrase_item`'s dispatch by library
    /// nibble, for the token-introspection surface. `None` when the
    /// library is missing/unloaded (`ERROR_NO_SUB_PHRASE_INDEX`) or the
    /// token is absent (`ERROR_NO_ITEM`).
    #[must_use]
    pub fn token_introspection(&self, token: u32) -> Option<TokenIntrospection> {
        let nibble = token >> 24;
        match nibble {
            1..=4 => {
                if !self.library_visible(nibble) {
                    return None;
                }
                // The facade reads the replaced item, not the immutable
                // shipped chunk (074a2219 phrase_index.h:646-657).
                let override_item = self
                    .user
                    .as_ref()
                    .and_then(|store| store.system_item_override(token).ok().flatten());
                let (text, pronunciations) = if let Some(item) = override_item {
                    let text = item
                        .phrase
                        .into_iter()
                        .map(char::from_u32)
                        .collect::<Option<String>>()?;
                    let pronunciations = item
                        .prons
                        .into_iter()
                        .map(|(keys, frequency)| {
                            let spelling = keys
                                .into_iter()
                                .map(|key| {
                                    oxpinyin_core::ChewingKey::from_packed(key).pinyin_string()
                                })
                                .collect::<Vec<_>>()
                                .join("'");
                            (spelling, u64::from(frequency))
                        })
                        .collect();
                    (text, pronunciations)
                } else {
                    (
                        self.system.phrase_text(token)?,
                        self.system.pronunciations(token),
                    )
                };
                Some(TokenIntrospection {
                    text,
                    // Drop only the pronunciations whose spelling can't
                    // resolve to a `SyllableKey`; keep the rest. A single
                    // unmappable row used to poison the whole
                    // introspection via `collect::<Option<Vec<_>>>()?`,
                    // hiding every valid pronunciation for a token that
                    // happens to carry one bad spelling.
                    pronunciations: pronunciations
                        .into_iter()
                        .filter_map(|(spelling, freq)| {
                            pinyin_to_keys(&spelling).map(|keys| (keys, freq))
                        })
                        .collect(),
                })
            }
            // The default facade's USER_FILE sub-indexes (addon.bin /
            // network.bin / user.bin): the pin's `get_phrase_item` reads the
            // item from the sub-index the token's nibble names
            // (`phrase_index.h:646-657`, the default table set's
            // `ADDON_DICTIONARY` / `NETWORK_DICTIONARY` / `USER_DICTIONARY`
            // rows of `data/table.conf.in`) — never the addon facade, whose
            // own nibbles 4..15 are a different token space (the
            // `DICTIONARY` rows, art.bin … technology.bin).
            5..=7 => {
                if !self.library_visible(nibble) {
                    return None;
                }
                let store = self.user.as_ref()?;
                let phrase = store.phrase(token).ok()??;
                Some(TokenIntrospection {
                    text: phrase.text().to_owned(),
                    pronunciations: phrase
                        .pronunciations()
                        .iter()
                        .map(|pronunciation| (pronunciation.keys().to_vec(), pronunciation.count()))
                        .collect(),
                })
            }
            _ => None,
        }
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
struct PathPricing {
    /// The user table's lookup of the path.
    user: Vec<PhraseEntry>,
    /// The path's tone-less keys; `None` when a syllable is no key.
    keys: Option<Vec<oxpinyin_core::ChewingKey>>,
}

/// One token as the ordinary lookup prices it.
struct PricedToken {
    item: TrellisItem,
    text: Option<String>,
}

impl Dictionary for RuntimeDict {
    type Syllable = SyllableKey;
    type Entry = PhraseEntry;
    type Error = DictError;

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
        #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
        {
            let user = self.user_lookup()?;
            self.note_user_search(syllables, &user);
        }
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
    ) -> Result<bool, Self::Error> {
        let query = Self::trellis_query(syllables, tones);
        let (Some(query), 0) = (query, self.library_mask.load(Ordering::SeqCst)) else {
            self.trellis_ranges(syllables, tones, ranges)?;
            return self.phrase_prefix_exists(syllables);
        };
        let loaded = |token: u32| self.token_ranged(token);
        let system_extends = self
            .system
            .trellis_ranges_continued_with(syllables, tones, ranges, loaded)?;
        self.user_trellis_ranges(&query, ranges)?;
        let user = self.user_lookup()?;
        #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
        self.note_user_search(syllables, &user);
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
    /// per path. A memo some other dictionary filled is not read: the
    /// answer is then [`Self::trellis_item`]'s, recomputed.
    fn trellis_item_on_path(
        &self,
        syllables: &[Self::Syllable],
        tones: &[u8],
        token: u32,
        memo: &TrellisPathMemo,
    ) -> Result<Option<TrellisItem>, Self::Error> {
        if syllables.is_empty() {
            return Ok(None);
        }
        let Some(path) = memo.get_or_try_init(|| self.path_pricing(syllables))? else {
            return self.trellis_item(syllables, tones, token);
        };
        Ok(self
            .priced_on_path(path, token, false)?
            .map(|priced| priced.item))
    }

    fn trellis_text(
        &self,
        syllables: &[Self::Syllable],
        _tones: &[u8],
        token: u32,
    ) -> Result<Option<String>, Self::Error> {
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
        let unreadable = self.system.lookup_into_flagged(syllables, out)?;
        let user = self.user_lookup()?;
        #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
        self.note_user_search(syllables, &user);
        out.extend(user.lookup(syllables));
        // 074a2219 phrase_index.h:136-163 uses wrapping guint32 sums;
        // pinyin_lookup2.cpp:437-466 reads pronunciation possibility from
        // the current item even when the match came from the shipped index.
        if let Some(query) = self.effective_query(syllables) {
            for entry in out.iter_mut() {
                if let Some((matched, total)) =
                    self.effective_possibility(&query, entry.token().value())
                {
                    *entry = entry
                        .clone()
                        .with_pronunciation_possibility(u64::from(matched), u64::from(total));
                }
            }
        }
        if self.library_mask.load(Ordering::SeqCst) != 0 {
            // An unloaded library's phrases leave every read: upstream
            // frees the sub-index (`phrase_index.cpp:260-268`), the
            // mask keeps the monolithic tables resident but invisible.
            out.retain(|entry| self.library_visible_token(entry.token().value()));
        }
        Ok(unreadable)
    }

    fn phrase_prefix_exists(&self, syllables: &[Self::Syllable]) -> Result<bool, Self::Error> {
        // A live library mask must hide unloaded entries from the widen
        // probe too — the CR finding on PR #234. Without this the n-best
        // decoder keeps extending paths that lead only to invisible
        // tokens (`pinyin_unload_phrase_library(2)` leaves the underlying
        // rows resident; only the visibility mask changes). With the
        // mask clear, the plain probe's fast path stays.
        let system_extends = if self.library_mask.load(Ordering::SeqCst) == 0 {
            self.system.phrase_prefix_exists(syllables)?
        } else {
            self.system
                .phrase_prefix_exists_visible(syllables, |token| {
                    self.library_visible_token(token)
                })?
        };
        #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
        {
            let user = self.user_lookup()?;
            self.note_user_search(syllables, &user);
            if system_extends {
                return Ok(true);
            }
            Ok(user.phrase_prefix_exists(syllables))
        }
        #[cfg(not(any(feature = "kyotocabinet", feature = "tkrzw")))]
        {
            if system_extends {
                return Ok(true);
            }
            Ok(self.user_lookup()?.phrase_prefix_exists(syllables))
        }
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
        if let Ok(lookup) = self.user_lookup() {
            tokens.extend(
                lookup
                    .tokens_for_text(text)
                    .iter()
                    .copied()
                    .map(PhraseToken::new),
            );
        }
        tokens.retain(|token| self.phrase_table_token_enabled(token.value()));
        // reduce_tokens groups the system and user table results by library.
        tokens.sort_by_key(|token| token.value() >> 24);
        tokens
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
            .as_ref()
            .is_some_and(|store| store.overlong_extension_gate(syllables))
        {
            return Ok(None);
        }
        let user = self.user_lookup()?;
        #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
        if let Some(store) = self.user.as_ref() {
            store.note_pinyin_suggestion(|| chewing_query(syllables));
        }
        tokens.extend(user.suggest_extensions(syllables));
        if self.library_mask.load(Ordering::SeqCst) != 0 {
            tokens.retain(|token| self.library_visible_token(*token));
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
                .as_ref()
                .is_some_and(|store| store.overlong_extension_gate(syllables))
    }

    /// `_token_get_phrase` over the facade: the system libraries' item
    /// text, else the user store's phrase text (the user library's
    /// items live there). The library mask hides an unloaded library's
    /// token, matching the pin's freed sub-index.
    fn phrase_text_for_token(&self, token: u32) -> Option<String> {
        if !self.library_visible_token(token) {
            return None;
        }
        if let Some(text) = self.system.phrase_text(token) {
            return Some(text);
        }
        let store = self.user.as_ref()?;
        store
            .phrase(token)
            .ok()
            .flatten()
            .map(|phrase| phrase.text().to_owned())
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
        if let Some(store) = self.user.as_ref()
            && let Some(rows) = store
                .item_pronunciations(value)
                .map_err(|error| DictError::Parse(error.to_string()))?
        {
            return Ok(PhraseItemRead::Item(rows));
        }
        self.system.phrase_item_pronunciations(token)
    }

    /// Both seams under this facade handle partial-key queries with one
    /// probe: the system dictionary's DBM is double-indexed at datagen time
    /// (see [`SystemDictionary`]), and `UserLookup` mirrors that with a
    /// `by_initial` index (`crates/oxpinyin-user/src/lookup.rs`). Addon
    /// facades share the DBM shape.
    fn handles_partial_keys(&self) -> bool {
        true
    }
}

/// The bigram language model with the user-count overlay.
#[derive(Clone)]
pub struct RuntimeLm {
    inner: Arc<BigramLanguageModel>,
    user: Option<UserStore>,
    addons: Arc<RwLock<AddonSet>>,
    /// The dictionary's `add_unigram_frequency` bumps: the total's is part
    /// of the one facade total every reader divides by, and a token's is
    /// part of its item's frequency, which every reader prices with.
    bumps: Arc<FacadeBumps>,
}

impl RuntimeLm {
    fn delta(&self, prev: Option<u32>, token: u32) -> Result<UserCountDelta, LmError> {
        let mut delta = match self.user.as_ref() {
            None => UserCountDelta::ZERO,
            Some(store) => store
                .count_delta(prev, token)
                .map_err(|error| LmError::User(error.to_string()))?,
        };
        delta.unigram_delta = delta.unigram_delta.saturating_add(self.bumps.token(token));
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
        let user = match self.user.as_ref() {
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

impl LanguageModel for RuntimeLm {
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

    fn unigram_freq(&self, token: &Self::Token) -> Result<Option<u64>, Self::Error> {
        let extra = match self.user.as_ref() {
            None => 0,
            Some(store) => store
                .unigram_delta(token.value())
                .map_err(|error| LmError::User(error.to_string()))?,
        };
        let extra = extra.saturating_add(self.bumps.token(token.value()));
        Ok(self
            .inner
            .unigram_freq_with_user_delta(token.value(), extra))
    }

    fn has_real_unigrams(&self) -> bool {
        self.inner.has_real_unigrams()
    }

    fn unigram_total(&self) -> Result<Option<u64>, Self::Error> {
        if !self.inner.has_real_unigrams() {
            return Ok(None);
        }
        let user_total = match self.user.as_ref() {
            None => 0,
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
        let imported = if let Some(store) = self.user.as_ref() {
            store
                .system_item_override(token.value())
                .map_err(|error| LmError::User(error.to_string()))?
                .is_some()
        } else {
            false
        };
        self.inner
            .nbest_step_costs_with_imported_item(prev, token, extra, imported)
    }

    fn nbest_item_costs_in_row(
        &self,
        _prev: &Self::Token,
        token: &Self::Token,
        item: Option<(u32, u32)>,
        row: &mut oxpinyin_core::NbestStepRow,
    ) -> Result<NbestStepCosts, Self::Error> {
        loop {
            // A committed write moves the generation under the same database
            // guard used by the unigram and override reads below.
            let generation = self.user.as_ref().map_or(0, UserStore::generation);
            let result = (|| {
                let extra = self.delta(None, token.value())?;
                let imported = if let Some(store) = self.user.as_ref() {
                    store
                        .system_item_override(token.value())
                        .map_err(|error| LmError::User(error.to_string()))?
                        .is_some()
                } else {
                    false
                };
                self.inner
                    .nbest_item_costs_with_memo(token, extra, imported, item, row.log2_memo())
            })();
            if self
                .user
                .as_ref()
                .is_none_or(|store| store.generation() == generation)
            {
                return result;
            }
        }
    }

    fn nbest_successors_in_row<'a>(
        &self,
        prev: &Self::Token,
        row: &'a mut oxpinyin_core::NbestStepRow,
    ) -> Result<Option<oxpinyin_core::NbestSuccessors<'a>>, Self::Error> {
        loop {
            let generation = self.user.as_ref().map_or(0, UserStore::generation);
            let result = row
                .get_or_try_init_in_generation(generation, || self.merged_row(prev.value()))
                .map(|_| ());
            if self
                .user
                .as_ref()
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
pub type RuntimeSession = Session<RuntimeDict, RuntimeLm>;

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
/// `Runtime` itself is not `Clone`; the backend handles it hands out —
/// [`Runtime::dict`], [`Runtime::lm`], [`Runtime::user_store`] — are cheap
/// clones sharing the table handles, and sessions built through
/// [`Runtime::new_session`] share them too.
pub struct Runtime {
    paths: StoragePaths,
    dict: RuntimeDict,
    lm: RuntimeLm,
    user: Option<UserStore>,
    conf: Arc<TableConf>,
    /// The per-key initial cost table, memoised alongside the
    /// library-visibility mask it was computed under. Deferred out of
    /// [`Runtime::open`]: computing it walks the dictionary, which dominates
    /// `pinyin_init`, so it is filled lazily on the first
    /// [`Runtime::new_session`]. Its values depend on which phrase libraries
    /// are visible — an unloaded library's items drop out of the lookups and
    /// the unigram denominator — so every session build compares the current
    /// mask against the stored one and rebuilds when a
    /// [`Runtime::load_library`]/[`Runtime::unload_library`] has changed
    /// visibility since. Rebuilds are epoch-validated: a mask stamp is
    /// only published when the visibility epoch held steady across the
    /// rebuild walk, so a concurrent flip can never leave a table
    /// stamped with a mask it was not computed under. Addon load/unload
    /// never touch the mask, so they never invalidate it.
    key_costs: RwLock<Option<(u32, Arc<[Cost]>)>>,
}

/// The system `table.conf` as `pinyin_init` finds it.
enum ConfFile {
    /// Not there: a fixture directory keeps the stock layout, and the
    /// profile is judged late.
    Missing,
    /// There but not readable as a regular file.
    Unreadable(std::io::Error),
    /// Read and accepted.
    Read(Box<TableConf>),
}

impl ConfFile {
    /// The layout the rest of the open follows.
    fn conf(&self) -> TableConf {
        match self {
            Self::Read(conf) => (**conf).clone(),
            Self::Missing | Self::Unreadable(_) => TableConf::stock(),
        }
    }
}

/// `SystemTableInfo2::load`'s `fopen` and parse. A file the pin would refuse
/// is an [`OpenError::TableConf`].
fn read_conf_file(system_dir: &Path) -> Result<ConfFile, OpenError> {
    let path = system_dir.join("table.conf");
    // A FIFO would block the read forever.
    match std::fs::metadata(&path) {
        Ok(meta) if !meta.is_file() => {
            return Ok(ConfFile::Unreadable(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "not a regular file",
            )));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(ConfFile::Missing),
        Err(error) => return Ok(ConfFile::Unreadable(error)),
    }
    match std::fs::read(&path) {
        Ok(bytes) => TableConf::parse(&bytes)
            .map(|conf| ConfFile::Read(Box::new(conf)))
            .map_err(OpenError::TableConf),
        Err(error) => Ok(ConfFile::Unreadable(error)),
    }
}

/// The user profile's `check_format`, run ahead of the system libraries.
enum EarlyProfile {
    /// Not run here: no user dir, or no readable `table.conf` to judge
    /// against; [`open_user_store`] judges it as before.
    Unchecked,
    /// Judged under these versions.
    Checked(SystemVersions, oxpinyin_user::ProfileCheck),
    /// The judgement could not be written; the store degrades to no user
    /// state.
    Degraded,
}

/// `check_format` where the pin runs it: right after `table.conf` loads,
/// before any table or library (`pinyin.cpp:337-346`, `zhuyin.cpp:281-292`),
/// so an init that fails later on a library has already written the
/// profile's diagnostic, wiped what it wipes and marked it, as the pin has
/// when it dies. Only a readable `table.conf` is judged against; a fixture
/// dir without one keeps the late judgement.
fn check_profile_early(
    conf: &ConfFile,
    user_dir: &Path,
    law: UserConfLaw,
) -> Result<EarlyProfile, OpenError> {
    let ConfFile::Read(conf) = conf else {
        return Ok(EarlyProfile::Unchecked);
    };
    let versions = SystemVersions::from_conf(conf);
    let layout = UserFileLayout::from_conf(conf);
    match UserStore::check_profile(user_dir, &versions, &layout, law) {
        Ok(check) => Ok(EarlyProfile::Checked(versions, check)),
        Err(UserStoreError::UnknownDatabaseFormat) => {
            Err(OpenError::UnknownDatabaseFormat(user_dir.to_path_buf()))
        }
        Err(error) => {
            let user_dir = user_dir.display();
            eprintln!(
                "oxpinyin: user store degraded to no-user-state \
                 (user dir {user_dir}: {error})"
            );
            Ok(EarlyProfile::Degraded)
        }
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
    conf_file: &ConfFile,
    user_dir: &Path,
    dict: &SystemDictionary,
    law: UserConfLaw,
    early: EarlyProfile,
) -> Result<Option<UserStore>, OpenError> {
    let (judged, check) = match early {
        EarlyProfile::Degraded => return Ok(None),
        EarlyProfile::Checked(versions, check) => (Some(versions), Some(check)),
        EarlyProfile::Unchecked => (None, None),
    };
    // An absent table.conf is the fixture-dir case: the pinned versions
    // stand. An unreadable *existing* one must not fall back to them —
    // the profile would be judged non-conforming against the wrong
    // triple and wiped, so the store degrades to no-user-state instead.
    let versions = match (judged, conf_file) {
        (Some(versions), _) => versions,
        (None, ConfFile::Read(conf)) => SystemVersions::from_conf(conf),
        (None, ConfFile::Missing) => SystemVersions::from_conf(&TableConf::stock()),
        (None, ConfFile::Unreadable(e)) => {
            eprintln!(
                "oxpinyin: table.conf unreadable ({e}); user store degraded to \
                 no-user-state rather than risk wiping the profile"
            );
            return Ok(None);
        }
    };
    let originals = oxpinyin_user::system_originals(
        dict.libraries(),
        UserFileLayout::from_conf(&conf_file.conf()),
    );
    let opened = match check {
        Some(check) => UserStore::open_libpinyin_checked(user_dir, originals, versions, law, check),
        None => UserStore::open_libpinyin(user_dir, originals, versions, law),
    };
    match opened {
        Ok(store) => Ok(Some(store)),
        Err(UserStoreError::UnknownDatabaseFormat) => {
            Err(OpenError::UnknownDatabaseFormat(user_dir.to_path_buf()))
        }
        Err(UserStoreError::SystemLogHeaderToken) => {
            Err(OpenError::CorruptLogHeader(user_dir.to_path_buf()))
        }
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
    /// `system_dir` holds the compiled-in backend's DBMs
    /// (`SystemDbm::file_name` — libpinyin's own names on Kyoto Cabinet,
    /// tkrzw and Berkeley DB), the
    /// per-library chunk files, and optionally `table.conf` (λ),
    /// `punct.bin`, and the addon DBM pair — the backend's native data
    /// directory, opened as is.
    /// When `user_dir` is given, the learning store opens too (its
    /// creation or read failure degrades to "no user state", matching the
    /// C ABI so a bad user dir cannot fail init), under libpinyin's
    /// `user.conf` law ([`UserConfLaw::Pinyin`]). An empty `user_dir` is
    /// given, not absent: it is the working directory, as `pinyin_init`'s
    /// `""` is, and stays relative for the life of the store.
    ///
    /// Nothing is read beyond the handles: the DBMs are opened, the chunk
    /// files mapped and checksummed, `table.conf` parsed for λ.
    ///
    /// # Errors
    ///
    /// Returns [`OpenError`] when a required file is missing or a DBM or
    /// chunk file cannot be opened.
    pub fn open(system_dir: &Path, user_dir: Option<&Path>) -> Result<Self, OpenError> {
        Self::open_with_law(system_dir, user_dir, UserConfLaw::Pinyin)
    }

    /// [`Runtime::open`] with the user dir's `user.conf` law named: the
    /// pinyin facade's init passes [`UserConfLaw::Pinyin`], the zhuyin
    /// facade's [`UserConfLaw::Zhuyin`] — the two pins share the data
    /// directory and the user file set but not the open counter's
    /// lifecycle (`oxpinyin_user::persistence::UserConfLaw`).
    ///
    /// # Errors
    ///
    /// As [`Runtime::open`].
    pub fn open_with_law(
        system_dir: &Path,
        user_dir: Option<&Path>,
        law: UserConfLaw,
    ) -> Result<Self, OpenError> {
        let conf_file = read_conf_file(system_dir)?;
        let conf = conf_file.conf();
        let early = match user_dir {
            Some(dir) => check_profile_early(&conf_file, dir, law)?,
            None => EarlyProfile::Unchecked,
        };
        // The init loop over the default rows runs after `check_format`
        // (`pinyin.cpp:344`, `:377-392`), so a profile judgement has been
        // written when it dies.
        if let ConfFile::Read(read) = &conf_file {
            read.check_default_rows(user_dir)
                .map_err(|site| OpenError::TableConf(TableConfError::Abort(site)))?;
        }
        let pinyin_index = system_dir.join(SystemDbm::PinyinIndex.file_name());
        let phrase_index = system_dir.join(SystemDbm::PhraseIndex.file_name());
        let bigram = system_dir.join(SystemDbm::Bigram.file_name());
        require_file(&pinyin_index)?;
        require_file(&phrase_index)?;
        require_file(&bigram)?;

        let dict = SystemDictionary::open_with(system_dir, &conf).map_err(OpenError::Dict)?;
        // The loaded-library mask is shared with the language model: an
        // unloaded library's items leave both the lookups and the unigram
        // denominator, as freeing the sub-index does upstream.
        let library_mask = Arc::new(AtomicU32::new(0));
        let mut lm = BigramLanguageModel::open_with_mask(
            &bigram,
            Arc::clone(dict.libraries()),
            Arc::clone(&library_mask),
        )
        .map_err(OpenError::Lm)?;
        // λ rides the install's table.conf when one ships; absent, the
        // pinned default stands.
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
        let user = match user_dir {
            None => Some(
                UserStore::open_transient(
                    oxpinyin_user::system_originals(
                        dict.libraries(),
                        UserFileLayout::from_conf(&conf),
                    ),
                    SystemVersions::from_conf(&conf),
                    law,
                )
                .map_err(|error| OpenError::Dict(DictError::Parse(error.to_string())))?,
            ),
            Some(dir) => open_user_store(&conf_file, dir, &dict, law, early)?,
        };

        let addons = Arc::new(RwLock::new(AddonSet {
            dict: AddonDictionary::open_with(system_dir, &conf).map_err(OpenError::Dict)?,
        }));
        let punct = PunctTable::open_optional(&system_dir.join(SystemDbm::Punct.file_name()));

        let bumps = Arc::new(FacadeBumps::default());
        let dict = RuntimeDict {
            system: Arc::new(dict),
            user: user.clone(),
            user_lookup_cache: LookupCache::default(),
            trellis_cache: TrellisCache::default(),
            addons: Arc::clone(&addons),
            punct: Arc::new(punct),
            bumps: Arc::clone(&bumps),
            library_mask,
            library_epoch: Arc::new(AtomicU64::new(0)),
        };
        let lm = RuntimeLm {
            inner: Arc::new(lm),
            user: user.clone(),
            addons,
            bumps,
        };

        Ok(Self {
            paths: StoragePaths::new(user_dir.unwrap_or_else(|| Path::new("")))
                .with_system_dirs([system_dir]),
            dict,
            lm,
            user,
            conf: Arc::new(conf),
            key_costs: RwLock::new(None),
        })
    }

    /// The system `table.conf` this runtime opened (the stock layout for a
    /// data directory without one): its rows decide which libraries exist,
    /// what they are called, and which calls the pin accepts for them.
    #[must_use]
    pub fn table_conf(&self) -> &TableConf {
        &self.conf
    }

    /// Builds a fresh session over this backend set. Sessions are cheap:
    /// they share the table handles.
    ///
    /// Configuration arrives per call (`oxpinyin-capi` passes its layered
    /// `Config`; simple embedders pass [`EmptyConfigSource`]), because the
    /// session reads it once at construction.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when the key-cost table cannot be computed —
    /// a backend failure while walking the dictionary, on the first build or
    /// the first after a library-visibility change — and otherwise forwards
    /// any [`EngineError`] from session construction (itself currently
    /// infallible over valid backends).
    ///
    /// The walk runs only for a model without real unigram frequencies. A
    /// runtime over [`BigramLanguageModel`] always has them, so in practice
    /// this allocates the session and reads nothing.
    pub fn new_session(&self, config: &dyn ConfigSource) -> Result<RuntimeSession, EngineError> {
        // Real frequencies mean the session decodes through the pinned
        // construction, which prices each candidate inline from the phrase
        // item it has already fetched — upstream's
        // `unigram_gen_next_step`/`bigram_gen_next_step`
        // (`src/lookup/phonetic_lookup.h:643-698` at the pin `0c5e80e1`),
        // whose `pinyin_alloc_instance` reads nothing at all. The key-cost
        // table is read only by the pre-frequency fallback scorer, which
        // that branch cannot reach, so walking the frozen key inventory
        // here would spend 42–57 ms on a value no decode consults
        // (`docs/findings/perf-backend-matrix-2026-09.md`). Skip the walk
        // and the cache with it; `BigramLanguageModel` always reports real
        // unigrams, so this is the live path for every runtime-backed
        // session.
        let key_costs = if self.lm.has_real_unigrams() {
            Vec::new()
        } else {
            self.cached_key_costs()?.to_vec()
        };
        Session::new_with_key_costs(
            config,
            self.paths.clone(),
            self.dict.clone(),
            self.lm.clone(),
            key_costs,
        )
    }

    /// The visibility-stamped key-cost table, walked on demand and shared
    /// by every later session built under the same mask.
    ///
    /// Only [`Self::new_session`]'s pre-frequency fallback branch needs
    /// this; a model with real unigram frequencies decodes through the
    /// pinned construction and never reads a key-cost table.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when a backend fails during the walk.
    fn cached_key_costs(&self) -> Result<Arc<[Cost]>, EngineError> {
        // The key-cost table is filled on first use rather than at open,
        // keeping `pinyin_init` off the dictionary walk. Its values depend on
        // the library-visibility mask (an unloaded library's items leave the
        // lookups and the unigram denominator), so the cache is stamped with
        // the mask it was built under and rebuilt whenever a
        // load/unload_library has changed visibility since — each session
        // decodes with the visibility in effect when it is built.
        let mask = self.dict.library_mask.load(Ordering::Acquire);
        // Fast path: shared read lock — concurrent `new_session` calls with
        // an unchanged mask do not contend. Published stamps are always
        // epoch-validated (slow path below), so a hit serves the true table
        // for its mask even under a concurrent flip; the flip at worst
        // linearizes this session just before it.
        let cached = {
            let cache = self
                .key_costs
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match cache.as_ref() {
                Some((cached_mask, table)) if *cached_mask == mask => Some(Arc::clone(table)),
                _ => None,
            }
        };
        if let Some(key_costs) = cached {
            return Ok(key_costs);
        }

        // Slow path — mask changed or cache empty. The walk runs unlocked
        // (fast-path readers keep going) but bracketed by the library
        // epoch: a load/unload_library flip during the walk would compute
        // the table under a mix of visibilities while stamping it with the
        // pre-walk mask, and that poisoned entry would then be served to
        // every later session under the same mask (mask values repeat —
        // an unload/reload pair returns to the same stamp). The epoch
        // check discards such a walk and retries, so a published stamp
        // always names the exact visibility the table was computed under.
        // Retrying can only starve under visibility flips faster than
        // every walk; flips are rare, user-driven operations.
        let key_costs: Arc<[Cost]> = loop {
            let epoch = self.dict.library_epoch.load(Ordering::SeqCst);
            let mask = self.dict.library_mask.load(Ordering::SeqCst);
            let computed: Arc<[Cost]> = Arc::from(key_cost_table(&self.dict, &self.lm)?);
            if self.dict.library_epoch.load(Ordering::SeqCst) != epoch {
                continue;
            }
            // The mask read sat between two unchanged epoch reads, so the
            // walk ran under exactly this mask.
            let mut cache = self
                .key_costs
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match cache.as_ref() {
                Some((cached_mask, table)) if *cached_mask == mask => break Arc::clone(table),
                _ => {
                    *cache = Some((mask, Arc::clone(&computed)));
                    break computed;
                }
            }
        };
        Ok(key_costs)
    }

    /// A handle clone of the merged dictionary backend.
    #[must_use]
    pub fn dict(&self) -> RuntimeDict {
        self.dict.clone()
    }

    /// A handle clone of the overlaid language model backend.
    #[must_use]
    pub fn lm(&self) -> RuntimeLm {
        self.lm.clone()
    }

    /// Clone of the user-learning handle, when the runtime was opened with
    /// a usable user directory.
    #[must_use]
    pub fn user_store(&self) -> Option<UserStore> {
        self.user.clone()
    }

    /// Loads addon library `index` from `system_dir`; `false` when already
    /// loaded or the library tables do not open.
    #[must_use]
    pub fn load_addon(&self, index: u8, system_dir: &Path) -> bool {
        self.dict.load_addon(index, system_dir)
    }

    /// Loads addon library `index` from this runtime's first configured
    /// system directory; `false` when the runtime has no system directory,
    /// the library is already loaded, or its tables do not open. Keeps the
    /// system-directory resolution here, next to the `paths` it reads,
    /// rather than duplicated in every embedder.
    #[must_use]
    pub fn load_system_addon(&self, index: u8) -> bool {
        let Some(system_dir) = self.paths.system_data_dirs().first() else {
            return false;
        };
        self.load_addon(index, system_dir)
    }

    /// [`Runtime::load_system_addon`] with the failure kept; `Ok(false)`
    /// when the runtime has no system directory or the library is already
    /// loaded.
    ///
    /// # Errors
    ///
    /// [`LibraryError`] when the library file is absent or does not verify.
    pub fn load_system_addon_reporting(&self, index: u8) -> Result<bool, LibraryError> {
        let Some(system_dir) = self.paths.system_data_dirs().first() else {
            return Ok(false);
        };
        self.dict.load_addon_reporting(index, system_dir)
    }

    /// The system library files absent when this runtime opened.
    #[must_use]
    pub fn unmapped_system_libraries(&self) -> &[PathBuf] {
        self.dict.unmapped_system_libraries()
    }

    /// The loaded, visible system libraries whose files no longer map; see
    /// [`RuntimeDict::unmappable_system_libraries`].
    #[must_use]
    pub fn unmappable_system_libraries(&self, only: Option<u8>) -> Vec<PathBuf> {
        self.dict.unmappable_system_libraries(only)
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
        self.dict.system.libraries().check_now(
            |nibble| {
                only.is_some()
                    || (nibble != 0
                        && self.dict.library_visible(u32::from(nibble))
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

    /// Maps every loaded SYSTEM_FILE/DICTIONARY default library again for
    /// mask_out, replacing the shared sub-index, never the phrase DBM.
    /// Read from the 074a2219 tree: pinyin.cpp:1253-1273,
    /// zhuyin.cpp:788-808. Unloaded libraries and rows without a user file
    /// are skipped. Failed mappings keep the existing safe fallback;
    /// the facade reports their mmap diagnostic before calling this.
    pub fn reload_system_libraries(&self) -> oxpinyin_user::SystemOriginals {
        self.remap_system_libraries(1..16, false)
    }

    /// Maps the unloaded SYSTEM_FILE/DICTIONARY default library `index`
    /// again, as `_load_phrase_library` does before it merges the saved
    /// `.dbin` (074a2219 `pinyin.cpp:246-264`, `zhuyin.cpp:298-312`). The
    /// result is empty for a row with no system chunk and no user file
    /// (the `USER_FILE` row, the unused rows) and for an unreadable file.
    /// Call it before [`Runtime::load_library`] makes the library visible,
    /// so no reader sees the session's state of the freed sub-index.
    pub fn reload_system_library(&self, index: u8) -> oxpinyin_user::SystemOriginals {
        self.remap_system_libraries(index..index.saturating_add(1), true)
    }

    /// The loaded SYSTEM_FILE/DICTIONARY default library `index` as it is
    /// mapped now, without mapping it again: what `unload` frees
    /// (074a2219 `phrase_index.cpp:261-268`). Empty for a row with no
    /// system chunk and no user file, and for a library already unloaded.
    pub fn current_system_library(&self, index: u8) -> oxpinyin_user::SystemOriginals {
        let layout = UserFileLayout::from_conf(&self.conf);
        let mut originals = oxpinyin_user::SystemOriginals::new(Default::default(), layout);
        let Some(row) = self.conf.default_row(index) else {
            return originals;
        };
        if !(1..16).contains(&index)
            || !matches!(row.file_type, FileType::SystemFile | FileType::Dictionary)
            || row.user.is_none()
            || !self.dict.library_visible(u32::from(index))
        {
            return originals;
        }
        if let Some(library) = self.dict.system.libraries().library(u32::from(index) << 24) {
            originals.insert(
                index,
                oxpinyin_user::SystemLibrary {
                    total: library.total_freq(),
                    range_end: library.token_range().end,
                    items: Default::default(),
                    mapped: Some(library),
                },
            );
        }
        originals
    }

    fn remap_system_libraries(
        &self,
        indices: std::ops::Range<u8>,
        include_unloaded: bool,
    ) -> oxpinyin_user::SystemOriginals {
        let layout = UserFileLayout::from_conf(&self.conf);
        let mut originals = oxpinyin_user::SystemOriginals::new(Default::default(), layout);
        let Some(dir) = self.paths.system_data_dirs().first() else {
            return originals;
        };
        let libraries = self.dict.system.libraries();
        for index in indices.filter(|&index| (1..16).contains(&index)) {
            let Some(row) = self.conf.default_row(index) else {
                continue;
            };
            if !matches!(row.file_type, FileType::SystemFile | FileType::Dictionary)
                || row.user.is_none()
                || (!include_unloaded && !self.dict.library_visible(u32::from(index)))
            {
                continue;
            }
            let Some(name) = row.system.as_ref() else {
                continue;
            };
            let path = oxpinyin_data::table_info::pin_join(dir, name.as_ref());
            if !libraries.reload(index, &path).unwrap_or(false) {
                continue;
            }
            if let Some(library) = libraries.library(u32::from(index) << 24) {
                originals.insert(
                    index,
                    oxpinyin_user::SystemLibrary {
                        total: library.total_freq(),
                        range_end: library.token_range().end,
                        items: Default::default(),
                        mapped: Some(library),
                    },
                );
            }
        }
        // add_unigram_frequency's accepted deltas belong to the replaced
        // sub-index too. Keep overlays for every library we did not reload.
        self.dict
            .bumps
            .drop_libraries(|nibble| originals.contains_key(&(nibble as u8)));
        originals
    }

    /// Unloads addon library `index` from this runtime's dictionary.
    ///
    /// The bound the pin asserts on `index` is the caller's to apply; see
    /// `RuntimeDict::unload_addon`.
    #[must_use]
    pub fn unload_system_addon(&self, index: u8) -> bool {
        self.dict.unload_addon(index)
    }

    /// Loads default library `index` — the GBK-reload path after an
    /// unload; `false` when already visible.
    #[must_use]
    pub fn load_library(&self, index: u32) -> bool {
        self.dict.load_library(index)
    }

    /// Unloads default library `index`; `false` only outside the sixteen
    /// sub-indices, `true` on every call otherwise (the pin ignores what
    /// `unload` returns, `pinyin.cpp:473-474`). Which indices a facade lets
    /// through is the C ABI's rule.
    #[must_use]
    pub fn unload_library(&self, index: u32) -> bool {
        self.dict.unload_library(index)
    }
}

fn require_file(path: &Path) -> Result<(), OpenError> {
    let meta = std::fs::metadata(path).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => OpenError::Missing(path.to_path_buf()),
        _ => OpenError::Io(path.to_path_buf(), error),
    })?;
    if meta.is_file() {
        Ok(())
    } else {
        Err(OpenError::NotRegularFile(path.to_path_buf()))
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

    // `new_session`'s own gate, pinned. The engine gate has
    // `real_unigrams_skip_the_key_cost_walk_but_the_fallback_keeps_it`;
    // this one had nothing — the concurrency test below deliberately
    // drives `cached_key_costs` directly, and `tests/assembly.rs`'s
    // visibility test says in its own comment that its `new_session`
    // calls assert nothing about the cache. So reverting the gate hunk
    // at `new_session` and putting the unconditional
    // `cached_key_costs()?.to_vec()` back would have left every test
    // green while restoring the ~430-lookup walk on the shipped path.
    //
    // A white-box test is the only way to see this: the cache is a
    // private field, so an integration test cannot observe whether a
    // session populated it. `RuntimeLm` always reports real unigrams, so
    // an opened runtime plus one session must leave the cache untouched.
    #[test]
    fn new_session_does_not_populate_the_key_cost_cache() {
        let runtime = Runtime::open(&w3_dir(), None).expect("fixture opens");
        assert!(
            runtime.lm().has_real_unigrams(),
            "vacuity guard: a runtime-backed model must report real \
             unigrams, or the gate under test is never taken"
        );
        assert!(
            runtime
                .key_costs
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_none(),
            "a freshly opened runtime has not walked the key inventory"
        );

        runtime
            .new_session(&oxpinyin_engine::EmptyConfigSource)
            .expect("session");

        assert!(
            runtime
                .key_costs
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_none(),
            "new_session must not walk the key inventory for a \
             real-unigram model: the table it would build has no reader \
             on this path"
        );
    }

    // The key-cost cache stamp must always name the visibility the table
    // was actually computed under, even when library visibility flips
    // race the rebuild walk: a torn walk published under a stale stamp
    // would serve wrong costs to every later session under that mask.
    //
    // Each round pins the cache to the unloaded stamp, then races one
    // rebuild against a bounded flip storm. Without epoch validation the
    // racing build snapshots mask 0 (a miss against the 0b100 pin), walks
    // while the flipper tears visibility, and publishes that torn walk
    // stamped 0 — which the quiesced probe then serves. With it, a
    // published stamp always names the visibility the table was computed
    // under, so the probe must see exactly the loaded table. (The flipper
    // is bounded by count, not a reader-set flag: the rebuild's retry loop
    // can only settle once the flips stop.)
    //
    // Driven through `cached_key_costs` rather than `new_session`: only
    // the pre-frequency fallback branch consults the cache, and a
    // `RuntimeLm` always reports real unigram frequencies, so no
    // `new_session` call can reach it. The cache and its epoch validation
    // stay covered here for the branch that does.
    #[test]
    fn key_costs_cache_stays_stamp_true_under_concurrent_visibility_flips() {
        const ROUNDS: usize = 12;
        const FLIP_PAIRS: usize = 20_000;
        let runtime = Runtime::open(&w3_dir(), None).expect("fixture opens");
        let dict = runtime.dict();
        let lm = runtime.lm();

        let loaded = key_cost_table(&dict, &lm).expect("walk (loaded)");
        assert!(runtime.unload_library(2), "first GBK unload arms the mask");
        let unloaded = key_cost_table(&dict, &lm).expect("walk (GBK unloaded)");
        assert_ne!(
            loaded, unloaded,
            "vacuity guard: the loaded and unloaded tables must differ"
        );
        assert!(runtime.load_library(2), "reload clears the mask");

        for _ in 0..ROUNDS {
            let _ = runtime.unload_library(2);
            runtime
                .cached_key_costs()
                .expect("pin the cache (GBK unloaded)");

            std::thread::scope(|scope| {
                scope.spawn(|| {
                    for _ in 0..FLIP_PAIRS {
                        let _ = runtime.unload_library(2);
                        let _ = runtime.load_library(2);
                    }
                });
                // Wait until the flipper is provably mid-storm — its
                // first load has landed (the pin left the mask at
                // 0b100) — so the racing build's snapshot happens under
                // flapping visibility, not before the thread is ever
                // scheduled.
                for _ in 0..100_000 {
                    if runtime.dict.library_mask.load(Ordering::SeqCst) == 0 {
                        break;
                    }
                    std::thread::yield_now();
                }
                runtime.cached_key_costs().expect("build mid-storm");
            });

            // Quiesced: the flip pairs leave GBK loaded, so the probe
            // must hold the true loaded-visibility table under stamp 0.
            runtime.cached_key_costs().expect("probe build");
            let cache = runtime
                .key_costs
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let (stamp, table) = cache
                .as_ref()
                .expect("a rebuild leaves the cache populated");
            assert_eq!(*stamp, 0, "final visibility is all-loaded");
            assert_eq!(
                table.as_ref(),
                loaded.as_slice(),
                "cached table must be the true loaded-visibility table, not a torn walk"
            );
        }
    }

    #[test]
    fn suggestion_rows_group_by_library_then_walk_the_ucs4_keys() {
        // The pin's list: system library groups first, the user library
        // last, each in the DBM's byte-lexical UCS-4 order — not text
        // (UTF-8 / code point) order. U+4E50 sorts before U+4F2D by code
        // point but after it by little-endian bytes (0x50 > 0x2D).
        let system = vec![
            (0x0200_0001, "中年".to_owned()),
            (0x0100_0010, "中华".to_owned()),
        ];
        let user_rows = vec![
            (0x0700_0001, oxpinyin_data::ucs4_walk_key("中乐")), // U+4E50
            (0x0700_0002, oxpinyin_data::ucs4_walk_key("中伭")), // U+4F2D
        ];
        let merged = merge_suggestion_rows(&system, &user_rows);
        let tokens: Vec<u32> = merged.iter().map(|(token, _)| *token).collect();
        assert_eq!(tokens, [0x0100_0010, 0x0200_0001, 0x0700_0002, 0x0700_0001]);
    }
}
