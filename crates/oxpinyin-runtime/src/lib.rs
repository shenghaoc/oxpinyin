//! The concrete engine assembly shared by every oxpinyin consumer.
//!
//! Wiring only: this crate opens a system data directory the way
//! `pinyin_init` does — the pinyin and phrase DBMs, the per-library
//! chunk files, `bigram.db`, `punct.bin`, the addon DBM pair, λ from
//! `table.conf` — installs the optional user store, and hands out
//! [`Session`]s over the merged backends. On Kyoto Cabinet and tkrzw that
//! directory is an unmodified libpinyin install's `data/`; on every
//! backend it is what `oxpinyin-datagen compile` writes. Nothing is
//! scanned at open: every reader in `oxpinyin-data` is a handle plus a
//! point read. The algorithms stay where they
//! belong — decoding/composition in `oxpinyin-engine`, tables and model math
//! in `oxpinyin-data`, user state in `oxpinyin-user` — and the user-count
//! overlay arithmetic lives in `oxpinyin-data`'s `*_with_user_delta` methods,
//! which [`RuntimeLm`] merely feeds. Centralizing the assembly here is what
//! keeps the C ABI (`oxpinyin-capi`) and the Python binding
//! (`oxpinyin-python`) from silently diverging: one construction, one set of
//! parity-tested semantics.
//!
//! Pure Rust; no FFI of any kind, `unsafe_code` forbidden.
// Constitution §4, mechanically: library builds may not unwrap, expect,
// or panic. Inline #[cfg(test)] modules are exempt (see the allow below
// their declaration); tests/, benches/ and examples/ are separate crates.
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::panic))]
#![cfg_attr(not(test), deny(clippy::panic_in_result_fn))]
#![warn(missing_docs)]

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

const GBK_DICTIONARY: u32 = 2;
use std::sync::{Arc, Mutex, RwLock};

use oxpinyin_core::scoring::{ScoringError, key_cost_table};
use oxpinyin_core::{
    Cost, Dictionary, LanguageModel, MergedGram, NbestStepCosts, PhraseEntry, PhraseToken,
    SyllableKey, UserCountDelta,
};
use oxpinyin_data::{
    AddonDictionary, BigramLanguageModel, DictError, LmError, PunctTable, SystemDbm,
    SystemDictionary, default_store_file,
};
use oxpinyin_engine::{ConfigSource, EngineError, Session, StoragePaths};
use oxpinyin_user::{PinyinKey, UserLookup, UserStore};

/// File name of the user store under the user data directory —
/// `user_store.<ext>`, where the extension names the compiled-in backend
/// (`kct` Kyoto Cabinet, `tkt` tkrzw, `lmdb` LMDB, `redb` redb).
#[must_use]
pub fn user_store_file() -> String {
    default_store_file("user_store")
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
    /// The per-key cost table could not be computed from the opened backends.
    KeyCosts(ScoringError),
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
            Self::KeyCosts(error) => write!(f, "key-cost table error: {error}"),
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

    fn load(&mut self, index: u8, system_dir: &Path) -> bool {
        self.dict.load(index, system_dir)
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
/// when any syllable is not a frozen key.
fn pinyin_to_keys(pinyin: &str) -> Option<Vec<PinyinKey>> {
    pinyin
        .split('\'')
        .map(|syllable| {
            SyllableKey::from_text(syllable).and_then(|key| PinyinKey::try_from(key.index()).ok())
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

// ── Merged backends ─────────────────────────────────────────────────────

/// The generation-stamped user-phrase lookup cache shared by clones.
type LookupCache = Arc<Mutex<Option<(u64, Arc<UserLookup>)>>>;

/// The system dictionary with the user-phrase lookup merged in.
///
/// `SystemDictionary` holds DBM handles and mappings, so it rides an
/// `Arc`.
#[derive(Clone)]
pub struct RuntimeDict {
    system: Arc<SystemDictionary>,
    user: Option<UserStore>,
    user_lookup_cache: LookupCache,
    addons: Arc<RwLock<AddonSet>>,
    punct: Arc<PunctTable>,
    /// The `add_unigram_frequency` overlay: in-memory per-token deltas
    /// over the baked counts. Upstream writes the same deltas into its
    /// in-memory FacadePhraseIndex and nothing persists them
    /// (`pinyin_save` flushes user data exclusively), so the overlay is
    /// the faithful shape: shared per context, gone at fini.
    unigram_overlay: Arc<Mutex<HashMap<u32, u64>>>,
    /// The facade-total bump `add_unigram_frequency` applies
    /// unconditionally once the token's library is loaded
    /// (`phrase_index.h:632` — before the item-level dispatch, so an
    /// absent-token add still moves the total). Observable through the
    /// amplified-law denominator (`pinyin.cpp:1817`).
    unigram_total_delta: Arc<AtomicU64>,
    /// The loaded-library mask: bit `n` **set** = library `n` unloaded
    /// (matches `library_visible` at :344 and the query surface —
    /// `library_visible_token`, `visible_item_count`, and the
    /// `unload_library` setter — that all consult it).  Only
    /// `GBK_DICTIONARY` (2) is ever settable — upstream's
    /// `pinyin_unload_phrase_library` refuses every other index
    /// (`pinyin.cpp:464-472`).
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
        let mut addons = self
            .addons
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        addons.load(index, system_dir)
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

    /// The library-visibility mask: bit `n` set = library `n` unloaded.
    /// Only `GBK_DICTIONARY` (2) is ever settable — upstream's
    /// `pinyin_unload_phrase_library` refuses every other index and
    /// answers `false` on an already-unloaded one
    /// (`phrase_index.cpp:260-268`).
    #[must_use]
    pub fn unload_library(&self, index: u32) -> bool {
        if index != GBK_DICTIONARY {
            return false;
        }
        let mask = 1u32 << index;
        // The flip is bracketed by epoch bumps so a concurrent key-cost
        // walk can detect that visibility moved under it and retry
        // (see `library_epoch`).
        self.library_epoch.fetch_add(1, Ordering::SeqCst);
        let newly_unloaded = self.library_mask.fetch_or(mask, Ordering::SeqCst) & mask == 0;
        self.library_epoch.fetch_add(1, Ordering::SeqCst);
        newly_unloaded
    }

    /// Re-loads library `index` after an unload — upstream re-attaches
    /// the sub-index from disk and answers `true`; already-loaded (mask
    /// clear) answers `false` (`pinyin.cpp:234-243`). Every other index
    /// is `false`: the system tables are loaded at init, so upstream's
    /// already-loaded rule applies there too.
    #[must_use]
    pub fn load_library(&self, index: u32) -> bool {
        // The GBK-reload path alone: the system tables (1, 2, 4) and the
        // USER_FILE library (7) all load at init (the default-tables loop
        // includes the USER_DICTIONARY row — measured `load(7)` = false
        // on the pin), so the already-loaded rule answers `false` for
        // every index but a GBK that an unload cleared.
        if index != GBK_DICTIONARY {
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
        let overlay = self
            .unigram_overlay
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        overlay.get(&token).copied()
    }

    /// Applies `add_unigram_frequency`'s effects for a loaded library:
    /// the facade-total bump is unconditional (`phrase_index.h:632`,
    /// before the item dispatch — an absent-token add still moves the
    /// amplified-law denominator), and the item delta lands only when
    /// the token exists. Returns whether the token was found. A token
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
            let addons = self.addons.read().unwrap_or_else(|p| p.into_inner());
            let addon_found = addons.unigram_freq(token).is_some();
            let user_found = self
                .user
                .as_ref()
                .and_then(|store| store.phrase(token).ok())
                .flatten()
                .is_some();
            system_found || addon_found || user_found
        };
        // Saturating on both edges: the total is an amplified-law
        // denominator that would fail catastrophically on a wraparound,
        // and the per-token overlay entry can pile up under repeated
        // `add_unigram_frequency` calls. `AtomicU64::fetch_add` wraps
        // silently even in debug, and the plain `+=` panics in debug
        // + wraps in release; both replaced with saturating equivalents
        // so debug and release stay consistent.
        let _ = self
            .unigram_total_delta
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |cur| {
                Some(cur.saturating_add(delta))
            });
        if !found {
            return false;
        }
        let mut overlay = self
            .unigram_overlay
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let entry = overlay.entry(token).or_insert(0);
        *entry = entry.saturating_add(delta);
        true
    }

    /// The overlay total — the amplified-law denominator's live shift
    /// (`pinyin.cpp:1817` reads the facade total this mirrors).
    #[must_use]
    pub fn unigram_total_delta(&self) -> u64 {
        self.unigram_total_delta.load(Ordering::SeqCst)
    }

    /// The addon library's stored unigram count for `token`.
    #[must_use]
    pub fn addon_unigram_frequency(&self, token: u32) -> Option<u64> {
        let addons = self.addons.read().unwrap_or_else(|p| p.into_inner());
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
        self.system.unigram_count(token)
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
                let text = self.system.phrase_text(token)?;
                let pronunciations = self.system.pronunciations(token);
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
            5 | 6 => {
                let item = self.addon_phrase_item(token)?;
                Some(TokenIntrospection {
                    text: item.text,
                    pronunciations: item.readings,
                })
            }
            7 => {
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

impl Dictionary for RuntimeDict {
    type Syllable = SyllableKey;
    type Entry = PhraseEntry;
    type Error = DictError;

    fn lookup(&self, syllables: &[Self::Syllable]) -> Result<Vec<Self::Entry>, Self::Error> {
        let mut entries = Vec::new();
        self.lookup_into(syllables, &mut entries)?;
        Ok(entries)
    }

    fn lookup_into(
        &self,
        syllables: &[Self::Syllable],
        out: &mut Vec<Self::Entry>,
    ) -> Result<(), Self::Error> {
        self.system.lookup_into(syllables, out)?;
        out.extend(self.user_lookup()?.lookup(syllables));
        if self.library_mask.load(Ordering::SeqCst) != 0 {
            // An unloaded library's phrases leave every read: upstream
            // frees the sub-index (`phrase_index.cpp:260-268`), the
            // mask keeps the monolithic tables resident but invisible.
            out.retain(|entry| self.library_visible_token(entry.token().value()));
        }
        Ok(())
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
        if system_extends {
            return Ok(true);
        }
        Ok(self.user_lookup()?.phrase_prefix_exists(syllables))
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
            .filter(|token| self.library_visible_token(token.value()))
            .collect();
        if let Ok(lookup) = self.user_lookup() {
            tokens.extend(
                lookup
                    .tokens_for_text(text)
                    .iter()
                    .map(|&token| PhraseToken::new(token)),
            );
        }
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
}

/// The bigram language model with the user-count overlay.
#[derive(Clone)]
pub struct RuntimeLm {
    inner: Arc<BigramLanguageModel>,
    user: Option<UserStore>,
    addons: Arc<RwLock<AddonSet>>,
}

impl RuntimeLm {
    fn delta(&self, prev: Option<u32>, token: u32) -> Result<UserCountDelta, LmError> {
        let Some(store) = self.user.as_ref() else {
            return Ok(UserCountDelta::ZERO);
        };
        store
            .count_delta(prev, token)
            .map_err(|error| LmError::User(error.to_string()))
    }

    /// The phrase-index total the pin's amplified law divides by
    /// (`pinyin.cpp:1813-1814`, `get_phrase_index_total_freq`, live per
    /// call): the facade's Σ item unigram over the visible libraries, plus
    /// the user store's extra. Must not be snapshotted — training changes
    /// it.
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

    fn score(
        &self,
        history: &[Self::Token],
        token: &Self::Token,
        edge_cost: Cost,
    ) -> Result<Cost, Self::Error> {
        let extra = self.delta(history.last().map(|token| token.value()), token.value())?;
        self.inner
            .score_with_user_delta(history, token, edge_cost, extra)
    }

    /// Upstream's Gate 2 (`pinyin.cpp:2209-2213`): the system gram and the
    /// user gram loaded once each and merged, as ONE row the caller indexes
    /// per candidate.
    ///
    /// `merge_single_gram` is additive over both the per-token counts and
    /// the row totals, which `merge_counts` already encodes for the n-best
    /// path; this applies it across the whole row instead of one pair at a
    /// time, so a guess costs two row loads rather than two per candidate.
    fn merged_successors(&self, prev: &Self::Token) -> Result<Option<MergedGram>, Self::Error> {
        let system = self.inner.load_successors(prev.value())?;
        let user = match self.user.as_ref() {
            None => None,
            Some(store) => {
                let rows = store
                    .bigram_successors(prev.value())
                    .map_err(|error| LmError::User(error.to_string()))?;
                let total = store
                    .bigram_total(prev.value())
                    .map_err(|error| LmError::User(error.to_string()))?;
                (!rows.is_empty() || total != 0).then_some((rows, total))
            }
        };
        // `merge_single_gram` answers false when both loads miss, and the
        // pin then leaves `merged_gram` empty so every possibility is zero.
        if system.is_none() && user.is_none() {
            return Ok(None);
        }
        let mut counts: BTreeMap<u32, u64> = BTreeMap::new();
        let mut total: u64 = 0;
        if let Some(row) = system {
            total = total.saturating_add(u64::from(row.total));
            for (token, count) in row.records {
                let slot = counts.entry(token).or_default();
                *slot = slot.saturating_add(u64::from(count));
            }
        }
        if let Some((rows, user_total)) = user {
            total = total.saturating_add(user_total);
            for (token, count) in rows {
                let slot = counts.entry(token).or_default();
                *slot = slot.saturating_add(count);
            }
        }
        Ok(Some(MergedGram::new(total, counts.into_iter().collect())))
    }

    fn unigram_freq(&self, token: &Self::Token) -> Result<Option<u64>, Self::Error> {
        let extra = match self.user.as_ref() {
            None => 0,
            Some(store) => store
                .unigram_delta(token.value())
                .map_err(|error| LmError::User(error.to_string()))?,
        };
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
        let extra = match self.user.as_ref() {
            None => 0,
            Some(store) => store
                .unigram_total()
                .map_err(|error| LmError::User(error.to_string()))?,
        };
        Ok(Some(self.inner.unigram_total().saturating_add(extra)))
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
        self.inner
            .nbest_step_costs_with_user_delta(prev, token, extra)
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

impl Runtime {
    /// Opens a system data directory the way `pinyin_init` does.
    ///
    /// `system_dir` holds the compiled-in backend's DBMs
    /// (`SystemDbm::file_name` — libpinyin's own names on Kyoto Cabinet
    /// and tkrzw, `<stem>.<ext>` on redb and LMDB), the per-library chunk
    /// files, and optionally `table.conf` (λ), `punct.bin`, and the addon
    /// DBM pair. On Kyoto Cabinet and tkrzw an unmodified libpinyin
    /// install's `data/` opens as is. When `user_dir` is given, the
    /// learning store opens too (its creation or read failure degrades to
    /// "no user state", matching the C ABI so a bad user dir cannot fail
    /// init).
    ///
    /// Nothing is read beyond the handles: the DBMs are opened, the chunk
    /// files mapped and checksummed, `table.conf` parsed for λ.
    ///
    /// # Errors
    ///
    /// Returns [`OpenError`] when a required file is missing or a DBM or
    /// chunk file cannot be opened.
    pub fn open(system_dir: &Path, user_dir: Option<&Path>) -> Result<Self, OpenError> {
        let pinyin_index = system_dir.join(SystemDbm::PinyinIndex.file_name());
        let phrase_index = system_dir.join(SystemDbm::PhraseIndex.file_name());
        let bigram = system_dir.join(SystemDbm::Bigram.file_name());
        require_file(&pinyin_index)?;
        require_file(&phrase_index)?;
        require_file(&bigram)?;

        let dict = SystemDictionary::open(system_dir).map_err(OpenError::Dict)?;
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
        lm.set_lambda_from_table_conf(&system_dir.join("table.conf"));

        // An empty path means no user directory; otherwise an unusable
        // directory must not fail init either — training then refuses,
        // upstream-style.
        let user = user_dir
            .filter(|dir| !dir.as_os_str().is_empty())
            .and_then(|dir| UserStore::open(&dir.join(user_store_file())).ok());

        let addons = Arc::new(RwLock::new(AddonSet {
            dict: AddonDictionary::open(system_dir).map_err(OpenError::Dict)?,
        }));
        let punct = PunctTable::open_optional(&system_dir.join(SystemDbm::Punct.file_name()));

        let dict = RuntimeDict {
            system: Arc::new(dict),
            user: user.clone(),
            user_lookup_cache: LookupCache::default(),
            addons: Arc::clone(&addons),
            punct: Arc::new(punct),
            unigram_overlay: Arc::new(Mutex::new(HashMap::new())),
            unigram_total_delta: Arc::new(AtomicU64::new(0)),
            library_mask,
            library_epoch: Arc::new(AtomicU64::new(0)),
        };
        let lm = RuntimeLm {
            inner: Arc::new(lm),
            user: user.clone(),
            addons,
        };

        Ok(Self {
            paths: StoragePaths::new(user_dir.unwrap_or_else(|| Path::new("")))
                .with_system_dirs([system_dir]),
            dict,
            lm,
            user,
            key_costs: RwLock::new(None),
        })
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
    pub fn new_session(&self, config: &dyn ConfigSource) -> Result<RuntimeSession, EngineError> {
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
            return Session::new_with_key_costs(
                config,
                self.paths.clone(),
                self.dict.clone(),
                self.lm.clone(),
                key_costs.to_vec(),
            );
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
        Session::new_with_key_costs(
            config,
            self.paths.clone(),
            self.dict.clone(),
            self.lm.clone(),
            key_costs.to_vec(),
        )
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

    /// Unloads default library `index` — GBK-only; `false` for any
    /// other index or when already unloaded.
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
    use oxpinyin_engine::EmptyConfigSource;

    fn w3_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("fixtures")
            .join("w3")
            .join(oxpinyin_data::DEFAULT_STORE_EXT)
    }

    // The key-cost cache stamp must always name the visibility the table
    // was actually computed under, even when library visibility flips
    // race the rebuild walk: a torn walk published under a stale stamp
    // would serve wrong costs to every later session under that mask.
    //
    // Each round pins the cache to the unloaded stamp, then races one
    // session build against a bounded flip storm. Without epoch
    // validation the racing build snapshots mask 0 (a miss against the
    // 0b100 pin), walks while the flipper tears visibility, and
    // publishes that torn walk stamped 0 — which the quiesced probe then
    // serves. With it, a published stamp always names the visibility the
    // table was computed under, so the probe must see exactly the loaded
    // table. (The flipper is bounded by count, not a reader-set flag: the
    // rebuild's retry loop can only settle once the flips stop.)
    #[test]
    fn key_costs_cache_stays_stamp_true_under_concurrent_visibility_flips() {
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

        const ROUNDS: usize = 12;
        const FLIP_PAIRS: usize = 20_000;
        for _ in 0..ROUNDS {
            let _ = runtime.unload_library(2);
            runtime
                .new_session(&EmptyConfigSource)
                .expect("pin session (GBK unloaded)");

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
                runtime
                    .new_session(&EmptyConfigSource)
                    .expect("session mid-storm");
            });

            // Quiesced: the flip pairs leave GBK loaded, so the probe
            // must hold the true loaded-visibility table under stamp 0.
            runtime
                .new_session(&EmptyConfigSource)
                .expect("probe session");
            let cache = runtime
                .key_costs
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let (stamp, table) = cache
                .as_ref()
                .expect("a session build leaves the cache populated");
            assert_eq!(*stamp, 0, "final visibility is all-loaded");
            assert_eq!(
                table.as_ref(),
                loaded.as_slice(),
                "cached table must be the true loaded-visibility table, not a torn walk"
            );
        }
    }
}
