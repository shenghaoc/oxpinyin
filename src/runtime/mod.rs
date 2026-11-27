//! Assembles the system dictionary, language model, user storage, and engine sessions.
//! Opening a [`Runtime`] reads the system configuration, handles the profile, and loads
//! the configured phrase libraries. Without a user directory, the runtime owns transient
//! user storage.
//!
//! [`RuntimeDict`] and [`RuntimeLm`] provide views of the live data. [`RuntimeSession`]
//! connects them to the engine.

// pinyin_init, pinyin.cpp:331–396 and zhuyin_init, zhuyin.cpp:279–334.

use crate::vocab::{PHRASE_INDEX_LIBRARY_COUNT, phrase_index_library_index};
use std::path::{Path, PathBuf};

pub(crate) use crate::data::phrase_library::{LibraryError, PhraseItemView, RowFault};
use crate::data::user_files::{SystemVersions, UserFileLayout};
use crate::data::{
    AddonDictionary, BigramLanguageModel, DictError, LmError, PunctTable, SystemDbm,
    SystemDictionary, SystemTables,
};
pub(crate) use crate::data::{FileType, TableConf, TableConfError, TableConfSite};
use crate::engine::{EngineError, Session, SessionSettings};
use crate::user::{
    ItemOrigin, LibraryLoopRows, LibraryLoopStop, LiveSystemItems, UnigramAdd, UserConfLaw,
    UserLookup, UserStore, UserStoreError,
};
use crate::{
    model::{
        Dictionary, LanguageModel, MergedGram, NbestStepCosts, NbestTokenCosts, PhraseIndexRanges,
        TrellisItem, TrellisPathMemo, UserCountDelta,
    },
    vocab::{PhraseEntry, PhraseText, PhraseToken, SyllableKey},
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
pub(crate) fn merge_suggestion_rows(system: &[u32], user_rows: &[(Vec<u8>, Vec<u32>)]) -> Vec<u32> {
    let user_len: usize = user_rows.iter().map(|(_, tokens)| tokens.len()).sum();
    let mut merged = Vec::with_capacity(system.len() + user_len);
    merged.extend_from_slice(system);
    merged.extend(user_rows.iter().flat_map(|(_, tokens)| tokens).copied());
    crate::model::sort_tokens_by_library(&mut merged);
    merged
}

// ── Open errors ─────────────────────────────────────────────────────────

/// Why a runtime could not be opened.
///
/// A missing or irregular system DBM file leaves an empty table.
/// Initialization continues (`pinyin.cpp:357,369,396` at 074a2219).
#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    /// The dictionary tables failed to open or parse.
    #[error("dictionary error: {0}")]
    Dict(DictError),
    /// The language model failed to open or parse.
    #[error("language model error: {0}")]
    Lm(LmError),
    /// The system `table.conf` is one the pin refuses or dies on
    /// (`SystemTableInfo2::load`, `table_info.cpp:194-294`, and the init
    /// loop's asserts, `pinyin.cpp:377-392`): [`TableConfError::Unopenable`]
    /// and [`TableConfError::Header`] are the pin's ordinary `false` returns
    /// (`open %s failed.` and `load %s failed!`), an ordinary init failure
    /// the C adapters answer with NULL and no warning. Every other kind
    /// aborts in the C adapters at the pin's abort sites, and is an undefined-read site the C ABI answers with NULL
    /// and one warning otherwise.
    #[error("table.conf: {0}")]
    TableConf(TableConfError),
    /// A `SYSTEM_FILE` library's user `.dbin` replay reached a pin
    /// assertion while the library loop merged it
    /// ([`crate::user::JournalFault`], rows 91 and 108): the open stops at
    /// that library, after `check_format`'s writes, and the C adapters abort.
    #[error("journal replay: {0}")]
    Journal(crate::user::JournalFault),
    /// The user session could not be built after the library loop handed
    /// it the system libraries ([`crate::user::UserStoreError::SessionSeed`],
    /// or another failure of the open past `check_format`). The pin's
    /// counterparts die, and the C adapters abort.
    #[error("user session: {0}")]
    UserSession(crate::user::UserStoreError),
}

/// A mask loop's remap, advanced one library at a time by
/// [`Runtime::remap_next`]: the libraries mapped so far, and the sub-indices
/// still to visit. Nothing past a stop is opened.
#[derive(Debug)]
pub(crate) struct RemapWalk {
    originals: crate::user::SystemOriginals,
    indices: std::ops::Range<u8>,
    /// One library's `_load_phrase_library`, not the mask loop: it maps an
    /// unloaded library, and a row whether or not the row names a user file.
    reload: bool,
}

impl RemapWalk {
    /// Ends a remap walk and returns the libraries it mapped.
    #[must_use]
    pub(crate) fn into_originals(self) -> crate::user::SystemOriginals {
        self.originals
    }
}

/// What [`Runtime::remap_next`] reached. A library the walk skips (in the
/// mask loop one that is not loaded or names no user file; in either a chunk
/// that fails some other check) is not a step.
#[derive(Debug)]
pub(crate) enum RemapStep {
    /// The library mapped and is in the walk's originals.
    Mapped(u8),
    /// `MemoryChunk::mmap` refuses the file: the `mmap %s failed!`
    /// path.
    Unmappable(PathBuf),
    /// A directory whose header read the pin asserts on (`memory_chunk.h:493`,
    /// row 96). The walk is over.
    Directory,
}

// ── Addon libraries ─────────────────────────────────────────────────────

pub(crate) use crate::model::TokenPhrase;

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
fn chewing_query(syllables: &[SyllableKey]) -> Vec<crate::parser::ChewingKey> {
    syllables.iter().map(|key| key.chewing_key()).collect()
}

// ── Merged backends ─────────────────────────────────────────────────────

/// The system dictionary with the user-phrase lookup merged in.
///
/// It borrows the runtime's tables and store for one operation.
#[derive(Clone, Copy)]
pub struct RuntimeDict<'a> {
    system: SystemDictionary<'a>,
    user: &'a UserStore,
    addons: &'a AddonDictionary,
    punct: &'a PunctTable,
    /// The loaded-library mask: bit `n` **set** = library `n` unloaded
    /// (matches `library_visible` at :344 and the query surface —
    /// `library_visible_token` and the
    /// `unload_library` setter — that all consult it).  The pinyin ABI only
    /// ever sets bit 2 (`GBK_DICTIONARY`, `pinyin.cpp:464-472`); the zhuyin
    /// ABI any bit but 1 (`zhuyin.cpp:378-388`).
    library_mask: u32,
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
    ) -> Result<(crate::model::UnreadableRows, bool), DictError> {
        let (mut unreadable, system_present) = self.system.lookup_with_live(
            syllables,
            tones,
            out,
            row,
            |token| self.library_visible(phrase_index_library_index(token)),
            |token| {
                self.live_items()
                    .read(token, |item| {
                        Some(PhraseEntry::new(token, item.phrase_text()?))
                    })
                    .found()
                    .flatten()
            },
        )?;
        let mut query: Vec<_> = syllables.iter().map(|key| key.chewing_key()).collect();
        self.user.note_pinyin_search_keys(&query);
        for (key, &tone) in query.iter_mut().zip(tones) {
            *key = key.with_tone(tone);
        }
        self.user_lookup()
            .lookup_resolving_missing(&query, out, |token| {
                if !self.system.library_loaded(token)
                    || !self.library_visible(phrase_index_library_index(token))
                {
                    return None;
                }
                let text = self.token_text(token);
                if text.is_none() {
                    unreadable.count += 1;
                }
                text
            });
        // An unloaded library's phrases leave every read: upstream frees
        // the sub-index (`phrase_index.cpp:260-268`), the mask keeps the
        // monolithic tables resident but invisible. A removed item fails
        // its `get_phrase_item` (`:180-206`), as at every pin read.
        out.retain(|entry| self.library_visible_token(entry.token()));
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
    fn trellis_query(syllables: &[SyllableKey], tones: &[u8]) -> Vec<crate::parser::ChewingKey> {
        crate::data::toned_query(syllables, tones)
    }

    /// The libraries whose tokens the trellis table search files, as a bit
    /// set: loaded and not unloaded by the mask, the pin's non-`NULL` range
    /// arrays (074a2219 `phrase_index.h:703-711`,
    /// `chewing_large_table2.h:107-109`). No item is read here. A removed or
    /// unreadable item stays ranged, and the step's item read fails for it,
    /// as `get_phrase_item` fails in the pin's step
    /// (`phonetic_lookup.h:646,674`).
    fn ranged_libraries(&self) -> crate::data::LibrarySet {
        self.system
            .loaded_libraries()
            .without_mask(self.library_mask)
    }

    /// Whether the trellis table search files `token`, tested token by
    /// token: the independent form of [`Self::ranged_libraries`] that the
    /// test-only [`Self::trellis_ranges`] keeps.
    #[cfg(test)]
    fn token_ranged(&self, token: u32) -> bool {
        self.system.library_loaded(token) && self.library_visible(phrase_index_library_index(token))
    }

    /// The user table's `convert` call of the trellis table search: the
    /// tokens whose library `ranged` holds.
    fn user_trellis_ranges(
        &self,
        query: &[crate::parser::ChewingKey],
        ranged: crate::data::LibrarySet,
        ranges: &mut PhraseIndexRanges<()>,
    ) {
        let tokens = self.user_lookup().search_toned(query);
        ranges.convert(
            tokens
                .into_iter()
                .filter(|&token| ranged.contains(&phrase_index_library_index(token)))
                .map(|token| (token, Some(()))),
        );
    }

    /// `(matched, total)` of `token`'s effective system item over `query`,
    /// the wrapping `guint32` sums of `phrase_index.h:136-163`.
    fn effective_possibility(
        &self,
        query: &[crate::parser::ChewingKey],
        token: u32,
    ) -> Option<(u32, u32)> {
        self.live_items()
            .read(token, |item| {
                if item.origin() != ItemOrigin::Overlay {
                    return None;
                }
                Some(Self::item_possibility(item, query))
            })
            .found()
            .flatten()
    }

    fn item_possibility(
        item: crate::user::SystemItem<'_>,
        query: &[crate::parser::ChewingKey],
    ) -> (u32, u32) {
        let mut total = 0_u32;
        let mut matched = 0_u32;
        for reading in item.pronunciations() {
            total = total.wrapping_add(reading.frequency);
            if query.len() == reading.keys.len()
                && query.iter().zip(reading.keys.iter()).all(|(q, bits)| {
                    let s = crate::parser::ChewingKey::from_packed(bits);
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
    fn path_pricing(&self, syllables: &[SyllableKey], tones: &[u8]) -> PathPricing {
        PathPricing {
            user: self.user_lookup().lookup(syllables),
            keys: Self::trellis_query(syllables, tones),
            libraries: self.live_items().step_libraries(),
        }
    }

    /// The item a step prices for `token`, including an unranged ONESTEP
    /// token. Ranged user entries retain their query text; otherwise read
    /// the stored item. The effective-item overlay and library mask apply.
    /// The text is built only under [`StepText::Build`].
    #[cfg(test)]
    fn priced_token(
        &self,
        syllables: &[SyllableKey],
        tones: &[u8],
        token: u32,
        text: StepText,
    ) -> Option<PricedToken> {
        if syllables.is_empty() {
            return None;
        }
        self.priced_on_path(&self.path_pricing(syllables, tones), token, text)
    }

    /// [`Self::priced_token`] over a non-empty path's [`PathPricing`].
    fn priced_on_path(
        &self,
        path: &PathPricing,
        token: u32,
        text: StepText,
    ) -> Option<PricedToken> {
        let user = path.user.iter().rev().find(|entry| entry.token() == token);
        let mut priced = if let Some(entry) = user {
            // The pin prices a user item from its own readings, as a system
            // one (`phonetic_lookup.h:646-656`, `phrase_index.h:136-164` at
            // 074a2219). The user lookup lists only tokens with a session
            // item, which `with_user_item` reads.
            let pronunciation = self.user.with_user_item(token, |view| {
                let (matched, total) =
                    Self::item_possibility(crate::user::SystemItem::Replayed(view), &path.keys);
                (u64::from(matched), u64::from(total))
            });
            PricedToken {
                item: TrellisItem {
                    chars: entry.phrase_length().try_into().unwrap_or(u32::MAX),
                    pronunciation,
                    unigram: None,
                },
                text: text.build(|| TokenPhrase::from(entry.text().clone())),
            }
        } else if let Some(priced) = self.user.with_user_item(token, |view| {
            // A second imported reading need not have an index entry
            // (074a2219 pinyin.cpp:563-582). ONESTEP still reads its item.
            let (matched, total) =
                Self::item_possibility(crate::user::SystemItem::Replayed(view), &path.keys);
            let pronunciation = Some((u64::from(matched), u64::from(total)));
            PricedToken {
                item: TrellisItem {
                    chars: view.phrase_length() as u32,
                    pronunciation,
                    unigram: None,
                },
                text: text.build(|| TokenPhrase {
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
                        text.build(|| TokenPhrase {
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
                    unigram: None,
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
        if !self.library_visible_token(token) {
            return None;
        }
        Some(priced)
    }

    /// What a trellis step prices of `token` on `path`, with its text under
    /// [`StepText::Build`]: [`Self::read_in_place`], else the full resolution of
    /// [`Self::priced_on_path`], which answers the same for an ordinary live
    /// item.
    fn priced(&self, path: &PathPricing, token: u32, text: StepText) -> Option<PricedToken> {
        self.read_in_place(path, token, |item| PricedToken {
            item: Self::step_item(item, &path.keys),
            text: text.build(|| TokenPhrase {
                phrase_length: item.phrase_length(),
                text: item.phrase_text(),
            }),
        })
        .or_else(|| self.priced_on_path(path, token, text))
    }

    /// The ordinary live item behind `token`, read in place and handed to
    /// `read`. The pin's step reads one phrase item for the length, the
    /// unigram and the readings (074a2219 `phonetic_lookup.h:646-656`,
    /// `:674-685`), as one array test and one offset read
    /// (`phrase_index.h:646-652`, `phrase_index.cpp:180-206`). The path's
    /// memo holds the per-library half ([`PathPricing::libraries`]). `None`
    /// for a token the path's user lookup lists, and for every other origin:
    /// the caller resolves those in full.
    fn read_in_place<R>(
        &self,
        path: &PathPricing,
        token: u32,
        read: impl FnOnce(crate::user::SystemItem<'_>) -> R,
    ) -> Option<R> {
        if path.user.iter().any(|entry| entry.token() == token) {
            return None;
        }
        self.live_items().read_in_place(path.libraries, token, read)
    }

    /// The step's view of a read item: its length, its readings against
    /// the path's `keys`, and its unigram.
    fn step_item(
        item: crate::user::SystemItem<'_>,
        keys: &[crate::parser::ChewingKey],
    ) -> TrellisItem {
        let (matched, total) = Self::item_possibility(item, keys);
        TrellisItem {
            chars: item.phrase_length() as u32,
            pronunciation: Some((u64::from(matched), u64::from(total))),
            unigram: Some(item.unigram()),
        }
    }

    fn user_lookup(&self) -> UserLookup<'_> {
        self.user.lookup()
    }

    /// The underlying system table set, without the user overlay.
    #[must_use]
    pub fn system(&self) -> &SystemDictionary<'a> {
        &self.system
    }

    /// Punctuations registered for `token`, if the punct table shipped
    /// with the system dir — one point read; a failed read answers none.
    #[must_use]
    pub fn punctuations(&self, token: u32) -> Vec<PhraseText> {
        self.punct.punctuations(token).unwrap_or_default()
    }

    /// Whether the configured default row is a system library.
    #[must_use]
    pub(crate) fn is_system_library(&self, index: u8) -> bool {
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
    pub(crate) fn live_items_ignoring_mask(&self) -> LiveSystemItems<'_> {
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
    pub(crate) const fn library_visible(&self, nibble: u8) -> bool {
        crate::data::library_visible(self.library_mask, nibble)
    }

    /// Whether the token's item is readable, for entry-level filters: its
    /// library is not unloaded, and no journal REMOVE took the item out of the
    /// sub-index (`get_phrase_item` fails for both, 074a2219
    /// `phrase_index.cpp:180-206`). The phrase table still holds the token
    /// either way ([`Self::phrase_table_token_enabled`]).
    #[must_use]
    pub(crate) fn library_visible_token(&self, token: u32) -> bool {
        let live = self.live_items();
        if live.is_system_library(phrase_index_library_index(token)) {
            return live.read(token, |_| ()).found().is_some();
        }
        live.is_visible(token)
    }

    /// Whether phrase-table search has an enabled token array for this library.
    /// Unlike visibility alone, this excludes libraries never loaded into the
    /// facade (074a2219 phrase_index.h:762-772; phrase_large_table3.cpp:40-42).
    #[must_use]
    pub fn phrase_table_token_enabled(&self, token: u32) -> bool {
        // `add_index` files a token under its masked library
        // (074a2219 phrase_large_table3.cpp:42, novel_types.h:44).
        let library = phrase_index_library_index(token);
        self.library_visible(library)
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
        let token = token & (crate::vocab::PHRASE_INDEX_LIBRARY_MASK | crate::vocab::PHRASE_MASK);
        if self.is_system_library(phrase_index_library_index(token)) {
            return self.live_items().pronunciation_count(token).found();
        }
        if !self.library_visible(phrase_index_library_index(token)) {
            return None;
        }
        self.user.user_pronunciation_count(token)
    }

    /// The live item's text for `_add_phrase`'s length and content checks
    /// (074a2219 pinyin.cpp:566-571; zhuyin.cpp:452-457). Reading spellings,
    /// frequencies and the unigram do not participate in those checks.
    #[must_use]
    pub fn token_text(&self, token: u32) -> Option<PhraseText> {
        if self.is_system_library(phrase_index_library_index(token)) {
            return self
                .live_items()
                .read(token, |item| item.phrase_text())
                .found()
                .flatten();
        }
        if !self.library_visible(phrase_index_library_index(token)) {
            return None;
        }
        self.user
            .with_user_item(token, |item| item.phrase_text())
            .flatten()
    }

    /// Whether [`Self::token_text`] answers a text, without building it.
    fn token_text_converts(&self, token: u32) -> bool {
        if self.is_system_library(phrase_index_library_index(token)) {
            return self
                .live_items()
                .read(token, |item| item.view().phrase_text_converts())
                .found()
                .unwrap_or(false);
        }
        if !self.library_visible(phrase_index_library_index(token)) {
            return false;
        }
        self.user
            .with_user_item(token, |item| item.phrase_text_converts())
            .unwrap_or(false)
    }

    /// The stored length and text of the token's item, as the C token
    /// getters read them (`_token_get_phrase`, 074a2219 `pinyin.cpp:1627-1646`,
    /// `zhuyin.cpp:1096-1114`). `None` only when `get_phrase_item` fails, or
    /// for a shipped item whose text passes the content end instead. A shipped item is read through its header-bounded
    /// view, so readings that pass the content end do not hide its text.
    #[must_use]
    pub fn token_phrase(&self, token: u32) -> Option<TokenPhrase> {
        self.with_token_item(token, |item| {
            item.phrase_words_present().then(|| TokenPhrase {
                phrase_length: item.phrase_length(),
                text: item.phrase_text(),
            })
        })
    }

    /// The stored words of `token`'s item, copied, where
    /// [`Self::token_phrase`] resolves the item: the words `convert_to_utf8`
    /// copies with `get_phrase_string` (074a2219 `lookup/lookup.cpp:45-47`).
    #[must_use]
    pub(crate) fn token_words(&self, token: u32) -> Option<crate::marshal::SentencePhrase> {
        self.words_for_token(token)
            .map(crate::marshal::SentencePhrase::Words)
    }

    /// The stored words of `token`'s item, as `get_phrase_string` copies the
    /// words into `convert_to_utf8`'s stack buffer: the system item a library
    /// mask leaves visible, else the user item. `None` where the item is
    /// absent or its bytes end before a declared word.
    pub(crate) fn words_for_token(
        &self,
        token: u32,
    ) -> Option<smallvec::SmallVec<[u32; crate::vocab::MAX_PHRASE_LENGTH]>> {
        let library = phrase_index_library_index(token);
        let items = self.live_items();
        if items.is_system_library(library) {
            return items.phrase_words(token);
        }
        if !self.library_visible(library) {
            return None;
        }
        self.user
            .with_user_item(token, |item| item.stored_words())
            .unwrap_or(None)
    }

    /// Reads `token`'s item where the facade resolves it: the system item
    /// a library mask leaves visible, else the user item; `None` where the
    /// item is absent or `read` answers `None`. Each reader refuses an item
    /// whose bytes end before a declared word.
    fn with_token_item<R>(
        &self,
        token: u32,
        read: impl FnOnce(PhraseItemView<'_>) -> Option<R>,
    ) -> Option<R> {
        if self.is_system_library(phrase_index_library_index(token)) {
            return self
                .live_items()
                .read_fields(token, |item| read(item.view()))
                .found()
                .flatten();
        }
        if !self.library_visible(phrase_index_library_index(token)) {
            return None;
        }
        self.user.with_user_item(token, read).flatten()
    }

    /// Whether the default facade owns this item. Missing/unloaded libraries
    /// and absent slots both answer false, as `get_phrase_item` does
    /// (074a2219 `phrase_index.h:646-652`, `phrase_index.cpp:180-205`).
    /// No text or pronunciation payload is decoded.
    #[must_use]
    pub fn token_exists(&self, token: u32) -> bool {
        if self.is_system_library(phrase_index_library_index(token)) {
            return self
                .live_items()
                .pronunciation_count(token)
                .found()
                .is_some();
        }
        self.library_visible(phrase_index_library_index(token)) && self.user.phrase_exists(token)
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
        if self.is_system_library(phrase_index_library_index(token)) {
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
        if self.is_system_library(phrase_index_library_index(token))
            || !self.library_visible(phrase_index_library_index(token))
        {
            return None;
        }
        self.user.with_nth_pronunciation_words(token, nth, read)
    }

    /// The addon phrase item behind `token`, for the choose-promotion path.
    #[must_use]
    pub fn addon_phrase_item(&self, token: u32) -> Option<Vec<u8>> {
        self.addons.phrase_item(token)
    }
}

/// What one key path's pricing reads whatever the token, kept in a
/// [`TrellisPathMemo`] for the n-best call that prices the path.
pub struct PathPricing {
    /// The user table's lookup of the path.
    user: Vec<PhraseEntry>,
    /// The path's toned keys ([`RuntimeDict::trellis_query`]).
    keys: Vec<crate::parser::ChewingKey>,
    /// The libraries whose items the path's steps read in place. The memo
    /// lives for one span search of one guess, and none of these facts
    /// changes while a guess runs ([`crate::user::StepLibraries`]).
    libraries: crate::user::StepLibraries,
}

/// Whether a priced step also builds its phrase text. The pin's trellis
/// step reads no text (`phonetic_lookup.h:646-656`); only a sentence's text
/// read builds one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StepText {
    /// Price the item only.
    Skip,
    /// Price the item and build its text.
    Build,
}

impl StepText {
    /// `build`'s answer under [`Self::Build`], else `None`.
    fn build<T>(self, build: impl FnOnce() -> T) -> Option<T> {
        (self == Self::Build).then(build)
    }
}

/// One token as the ordinary lookup prices it.
struct PricedToken {
    item: TrellisItem,
    /// The item's stored length and text, read only when wanted.
    text: Option<TokenPhrase>,
}

impl RuntimeDict<'_> {
    /// Looks up the entries matching `syllables`: the system table's, then
    /// the user table's.
    ///
    /// # Errors
    ///
    /// Returns [`DictError`] when the dictionary backend cannot serve the lookup.
    #[cfg(test)]
    pub fn lookup(&self, syllables: &[SyllableKey]) -> Result<Vec<PhraseEntry>, DictError> {
        let mut entries = Vec::new();
        self.lookup_into(syllables, &mut entries)?;
        Ok(entries)
    }

    /// Every matching pronunciation index record of the sentence trellis,
    /// each priced and resolved to its entry. `tones` supplies the query
    /// tones.
    ///
    /// # Errors
    ///
    /// Returns [`DictError`] when the dictionary backend cannot serve the lookup.
    #[cfg(test)]
    pub(crate) fn trellis_records(
        &self,
        syllables: &[SyllableKey],
        tones: &[u8],
    ) -> Result<Vec<PhraseEntry>, DictError> {
        let mut ranges = PhraseIndexRanges::new();
        self.trellis_ranges(syllables, tones, &mut ranges)?;
        let mut out = Vec::new();
        for (token, ()) in ranges.walk_tokens() {
            if let Some(priced) = self.priced_token(syllables, tones, token, StepText::Build) {
                // An entry needs a text; an item whose text does not convert
                // is not listed.
                let Some(text) = priced.text.and_then(|phrase| phrase.text) else {
                    continue;
                };
                let mut entry = PhraseEntry::new(token, text);
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
    #[cfg(test)]
    pub(crate) fn trellis_ranges(
        &self,
        syllables: &[SyllableKey],
        tones: &[u8],
        ranges: &mut PhraseIndexRanges<()>,
    ) -> Result<(), DictError> {
        let query = Self::trellis_query(syllables, tones);
        // The user table's search of this path (#785); the pin makes one
        // per `FacadeChewingTable2::search`, and a repeat is idempotent.
        self.note_user_search(syllables);
        let visible = |token: u32| self.library_visible(phrase_index_library_index(token));
        self.system
            .trellis_ranges_with(syllables, tones, ranges, visible)?;
        let tokens = self.user_lookup().search_toned(&query);
        ranges.convert(
            tokens
                .into_iter()
                .filter(|&token| self.token_ranged(token))
                .map(|token| (token, Some(()))),
        );
        Ok(())
    }

    /// What one trellis step prices of `token` over `syllables` and
    /// `tones`, without a path memo; `None` when the token has no readable
    /// item there.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn trellis_item(
        &self,
        syllables: &[SyllableKey],
        tones: &[u8],
        token: u32,
    ) -> Option<TrellisItem> {
        self.priced_token(syllables, tones, token, StepText::Skip)
            .map(|priced| priced.item)
    }

    /// [`Self::trellis_item`], reading the user lookup and the keys once
    /// per path.
    #[cfg(test)]
    pub(crate) fn trellis_item_on_path(
        &self,
        syllables: &[SyllableKey],
        tones: &[u8],
        token: u32,
        memo: &TrellisPathMemo<PathPricing>,
    ) -> Result<Option<TrellisItem>, DictError> {
        if syllables.is_empty() {
            return Ok(None);
        }
        let path =
            memo.get_or_try_init(|| Ok::<_, DictError>(self.path_pricing(syllables, tones)))?;
        Ok(self
            .priced_on_path(path, token, StepText::Skip)
            .map(|priced| priced.item))
    }

    /// [`Self::lookup`] into a caller-owned buffer.
    ///
    /// # Errors
    ///
    /// Returns [`DictError`] when the dictionary backend cannot serve the lookup.
    #[cfg(test)]
    pub(crate) fn lookup_into(
        &self,
        syllables: &[SyllableKey],
        out: &mut Vec<PhraseEntry>,
    ) -> Result<(), DictError> {
        self.lookup_into_flagged(syllables, out).map(|_| ())
    }

    /// [`Self::lookup_into`], also answering whether the lookup met a token
    /// of a loaded library whose item could not be read.
    ///
    /// # Errors
    ///
    /// Returns [`DictError`] when the dictionary backend cannot serve the lookup.
    #[cfg(test)]
    pub(crate) fn lookup_into_flagged(
        &self,
        syllables: &[SyllableKey],
        out: &mut Vec<PhraseEntry>,
    ) -> Result<bool, DictError> {
        self.fill_lookup(syllables, &[], out, &mut Vec::new())
            .map(|(rows, _)| rows.count != 0)
    }

    /// `SEARCH_CONTINUED`: whether a stored phrase's pinyin can extend
    /// `syllables`.
    #[cfg(test)]
    #[must_use]
    pub fn phrase_prefix_exists(&self, syllables: &[SyllableKey]) -> bool {
        // `SEARCH_CONTINUED` comes from the pinyin table's DB get alone, before
        // any token or library check (`chewing_large_table2_tkrzwdb.cpp:146-152`,
        // `chewing_large_table2_bdb.cpp:182-187`,
        // `chewing_large_table2_kyotodb.cpp:160-166` at 074a2219). A library mask
        // only drops a token at `convert`'s NULL range
        // (`chewing_large_table2.h:107-111`), and a journal REMOVE clears only the
        // sub-index, so neither reaches this answer.
        let system_extends = self.system.phrase_prefix_exists(syllables);
        // The pin searches the system table and then the user table with no
        // early return (`facade_chewing_table2.h:120-126` at 074a2219), so
        // the user lookup and its search note come before the answer.
        let user = self.user_lookup();
        self.note_user_search(syllables);
        system_extends || user.phrase_prefix_exists(syllables)
    }

    /// The addon facade's entries for `syllables`.
    ///
    /// # Errors
    ///
    /// Returns [`DictError`] when the dictionary backend cannot serve the lookup.
    #[cfg(test)]
    pub(crate) fn lookup_addon(
        &self,
        syllables: &[SyllableKey],
    ) -> Result<Vec<PhraseEntry>, DictError> {
        let mut entries = Vec::new();
        self.search_addon_into(syllables, &mut entries, &mut Vec::new())?;
        Ok(entries)
    }
}

impl Dictionary for RuntimeDict<'_> {
    type Syllable = SyllableKey;
    type Error = DictError;
    type PathMemo = TrellisPathMemo<PathPricing>;

    /// `Self::trellis_ranges` and `Self::phrase_prefix_exists` with one
    /// system DBM get. The path's toned keys are built once, on the stack,
    /// for both tables, as the pin's search keeps its index in a stack array
    /// (074a2219 `chewing_large_table2.cpp:161-176`), and the ranged
    /// libraries are read once for both `convert` calls.
    fn trellis_ranges_continued(
        &self,
        syllables: &[Self::Syllable],
        tones: &[u8],
        ranges: &mut PhraseIndexRanges<()>,
        scratch: &mut Vec<u8>,
    ) -> Result<bool, Self::Error> {
        let query: crate::data::SearchQuery = crate::data::toned_query(syllables, tones);
        let ranged = self.ranged_libraries();
        let system_extends = self
            .system
            .trellis_ranges_continued_with(&query, ranges, ranged, scratch)?;
        self.user_trellis_ranges(&query, ranged, ranges);
        let user = self.user_lookup();
        self.note_user_search(syllables);
        if system_extends {
            return Ok(true);
        }
        Ok(user.phrase_prefix_exists(syllables))
    }

    fn trellis_step_item(
        &self,
        syllables: &[Self::Syllable],
        tones: &[u8],
        token: u32,
        memo: &Self::PathMemo,
    ) -> Result<Option<TrellisItem>, Self::Error> {
        if syllables.is_empty() {
            return Ok(None);
        }
        let path =
            memo.get_or_try_init(|| Ok::<_, DictError>(self.path_pricing(syllables, tones)))?;
        Ok(self
            .read_in_place(path, token, |item| Self::step_item(item, &path.keys))
            .or_else(|| {
                self.priced_on_path(path, token, StepText::Skip)
                    .map(|priced| priced.item)
            }))
    }

    fn trellis_step_text(
        &self,
        syllables: &[Self::Syllable],
        tones: &[u8],
        token: u32,
    ) -> Result<Option<TokenPhrase>, Self::Error> {
        if syllables.is_empty() {
            return Ok(None);
        }
        Ok(self
            .priced(&self.path_pricing(syllables, tones), token, StepText::Build)
            .and_then(|priced| priced.text))
    }

    fn candidate_search_into(
        &self,
        syllables: &[Self::Syllable],
        tones: &[u8],
        out: &mut Vec<PhraseEntry>,
        row: &mut Vec<u8>,
    ) -> Result<(crate::model::UnreadableRows, bool), Self::Error> {
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

    /// Exact-text token lookup over raw UCS-4 spans, including
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
        crate::model::sort_tokens_by_library(&mut tokens);
        tokens
    }

    fn phrase_span_searches_are_observable(&self) -> bool {
        self.system.phrase_attachment_failed()
    }

    fn phrase_span_continues(&self, words: &[u32]) -> bool {
        !self.system.phrase_attachment_failed() || self.user.phrase_table_contains_ucs4(words)
    }

    fn search_addon_into(
        &self,
        syllables: &[Self::Syllable],
        out: &mut Vec<PhraseEntry>,
        row: &mut Vec<u8>,
    ) -> Result<bool, Self::Error> {
        self.addons.search_into(syllables, out, row)
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
        if self.library_mask != 0 {
            tokens.retain(|token| self.library_visible(phrase_index_library_index(*token)));
        }
        crate::model::sort_tokens_by_library(&mut tokens);
        Ok(Some(tokens))
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

    /// The first source [`Self::phrase_text_for_token`] reads, checked
    /// without building its text; any other answer takes the full read.
    fn has_phrase_text_for_token(&self, token: u32) -> bool {
        if !self.library_visible_token(token) {
            return false;
        }
        self.token_text_converts(token) || self.phrase_text_for_token(token).is_some()
    }

    fn phrase_length_for_token(&self, token: u32) -> Option<usize> {
        if self.is_system_library(phrase_index_library_index(token)) {
            return self
                .live_items()
                .read(token, |item| item.phrase_length())
                .found();
        }
        if !self.library_visible(phrase_index_library_index(token)) {
            return None;
        }
        self.user.phrase_length(token)
    }

    /// `get_phrase_item` over the facade (074a2219 `phrase_index.h:646-657`):
    /// the item a library mask hides is the pin's freed sub-index, an item
    /// the user store replaced or owns is read from there, and any other
    /// system item from the shipped chunk.
    fn read_phrase_item<R>(
        &self,
        token: PhraseToken,
        read: impl FnOnce(PhraseItemView<'_>) -> R,
    ) -> Result<Option<R>, Self::Error> {
        if !self.library_visible_token(token) {
            return Ok(None);
        }
        let mut read = Some(read);
        if let Some(found) = self
            .user
            .with_effective_item(token, |item| read.take().map(|read| read(item)))
            .flatten()
        {
            return Ok(Some(found));
        }
        match read {
            Some(read) => self.system.read_phrase_item(token, read),
            None => Ok(None),
        }
    }
}

/// The bigram language model with the user-count overlay.
#[derive(Clone, Copy)]
pub struct RuntimeLm<'a> {
    inner: BigramLanguageModel<'a>,
    user: &'a UserStore,
    addons: &'a AddonDictionary,
}

/// The engine error a training step's store refusal stands for: the pin's
/// own aborts at the step (a gram shorter than its total, `memory_chunk.h:390`;
/// `set_freq` missing the item it inserted, `phonetic_lookup.h:903`; the
/// failed container's `store`, `:904`), the partial item tail the pin reads
/// past (register row 100), or another user-model failure.
fn training_error(error: &crate::user::UserStoreError) -> EngineError {
    use crate::user::UserStoreError;
    match error {
        UserStoreError::ShortUserBigramValue
        | UserStoreError::TrainingSetFreqMiss
        | UserStoreError::FailedUserBigramStore => EngineError::TrainingAbort,
        UserStoreError::PartialUserBigramTail => EngineError::UnsafeTrainingPredecessor,
        _ => EngineError::UserModel,
    }
}

/// `m_user_bigram->load`'s answer to a failed read: no gram, which
/// `search_bigram2` ignores (`phrase_lookup.cpp:269-270` at 074a2219), so
/// the system gram then stands alone (`ngram.cpp:292-296`). The pin's user
/// bigram is the in-memory copy `load_db` makes (`pinyin.cpp:399-401`), so
/// its checked read cannot fail after the size check; no user container
/// reports that failure either.
fn user_gram_or_none(read: Result<Option<Vec<u8>>, UserStoreError>) -> Option<Vec<u8>> {
    read.ok().flatten()
}

impl RuntimeLm<'_> {
    fn delta(&self, prev: Option<u32>, token: u32) -> Result<UserCountDelta, LmError> {
        let mut delta = self
            .user
            .count_delta(prev, token)
            .map_err(|_| LmError::User)?;
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
        let user = user_gram_or_none(self.user.bigram_row(prev));
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
    type Error = LmError;

    fn amplification_lambda(&self) -> f32 {
        self.inner.amplification_lambda()
    }

    /// Upstream's Gate 2 (`pinyin.cpp:2210-2215`): the system gram and the
    /// user gram loaded once each and merged, as ONE row the caller indexes
    /// per candidate. The n-best and phrase paths load the same row.
    fn merged_successors(&self, prev: &PhraseToken) -> Result<Option<MergedGram>, Self::Error> {
        self.merged_row(*prev)
    }

    fn is_read_after_check(&self, error: &Self::Error) -> bool {
        error.is_read_after_check()
    }

    fn unigram_freq(&self, token: &PhraseToken) -> Result<Option<u64>, Self::Error> {
        let live = self.user.system_unigram(*token);
        let extra = if live.is_some() && self.inner.is_visible(phrase_index_library_index(*token)) {
            // The model uses only the live field on this branch.
            0
        } else {
            self.user.unigram_delta(*token)
        };
        Ok(Some(
            self.inner.unigram_freq_with_user_delta(*token, live, extra),
        ))
    }

    fn unigram_total(&self) -> Result<Option<u64>, Self::Error> {
        let user_total = self
            .user
            .unigram_total_over(|nibble| self.inner.is_visible(nibble));
        Ok(Some(self.inner.facade_total(user_total)))
    }

    fn addon_unigram_freq(&self, token: &PhraseToken) -> Result<Option<u64>, Self::Error> {
        if self.addons.is_empty() {
            return Ok(None);
        }
        Ok(Some(self.addons.unigram_freq(*token).unwrap_or(0)))
    }

    fn addon_unigram_total(&self) -> Result<Option<u64>, Self::Error> {
        Ok(self.addons.unigram_total())
    }

    /// The §5 overlay the n-best step costs take.
    fn nbest_step_costs(
        &self,
        prev: &PhraseToken,
        token: &PhraseToken,
    ) -> Result<NbestStepCosts, Self::Error> {
        let extra = self.delta(Some(*prev), *token)?;
        self.inner
            .nbest_step_costs_with_user_delta(prev, token, extra)
    }

    /// A step that read the token's ordinary live item has its unigram
    /// field, as the live-visible branch of [`Self::nbest_token_costs`]
    /// reads it (that branch ignores `unigram_delta`). The facade total's
    /// user side is read once per guess into `total`.
    fn nbest_token_costs_read(
        &self,
        token: &PhraseToken,
        unigram: Option<u32>,
        total: &std::cell::Cell<Option<u64>>,
    ) -> Result<NbestTokenCosts, Self::Error> {
        let Some(unigram) = unigram else {
            return self.nbest_token_costs(token);
        };
        let user_total = total.get().unwrap_or_else(|| {
            let user_total = self
                .user
                .unigram_total_over(|nibble| self.inner.is_visible(nibble));
            total.set(Some(user_total));
            user_total
        });
        Ok(self.inner.nbest_token_costs_with(
            token,
            UserCountDelta {
                unigram_live: Some(unigram),
                unigram_total_delta: user_total,
                ..UserCountDelta::ZERO
            },
        ))
    }

    fn nbest_token_costs(&self, token: &PhraseToken) -> Result<NbestTokenCosts, Self::Error> {
        let extra = self.delta(None, *token)?;
        Ok(self.inner.nbest_token_costs_with(token, extra))
    }

    fn release_row(&self, row: &mut crate::model::NbestStepRow) {
        self.inner.release_row(row);
    }

    fn nbest_successors_in_row<'a>(
        &self,
        prev: &PhraseToken,
        row: &'a mut crate::model::NbestStepRow,
    ) -> Result<Option<crate::model::NbestSuccessors<'a>>, Self::Error> {
        Ok(Some(
            row.get_or_try_init(|| self.merged_row(*prev))?.map_or(
                crate::model::NbestSuccessors::Miss,
                crate::model::NbestSuccessors::Row,
            ),
        ))
    }
}

// ── Runtime ─────────────────────────────────────────────────────────────

/// A [`Session`] over this crate's concrete backends.
pub(crate) type RuntimeSession<'a> =
    Session<RuntimeDict<'a>, RuntimeLm<'a>, &'a mut crate::engine::SessionState>;

/// One opened engine: configuration-independent wiring for the converted
/// system tables, the unigram model, λ, the learning store, and the
/// merged backends sessions decode over.
///
/// Storage has one owner. [`Runtime::dict`], [`Runtime::lm`] and
/// [`Runtime::user_store`] borrow it for an operation; writers require
/// exclusive access. `Runtime::new_session` retains composition only.
/// Every successful open owns a store, including when the user directory
/// is absent: that store is transient, and its directory flag controls
/// training and saving.
pub struct Runtime {
    system_dir: PathBuf,
    system: crate::data::SystemTables,
    model: crate::data::BigramTables,
    // The store and the addon dictionary each hold a `PhraseLibraries`
    // inline (2.3 KiB), which phrase-item reads index on every lookup. One
    // allocation for each owner keeps those reads direct and the runtime
    // small, so the open and the contexts that wrap it do not copy them.
    user: Box<UserStore>,
    conf: TableConf,
    addons: Box<AddonDictionary>,
    punct: PunctTable,
    library_mask: u32,
}

impl Runtime {
    // pinyin_fini, pinyin.cpp:1195–1215, and zhuyin_fini, zhuyin.cpp:741–757. Compatibility
    // row 42 covers surviving handles.
    /// Applies the facade's final marker policy and closes each native attachment once.
    /// Repeated calls are inert. In-memory state remains available to surviving handles.
    pub fn finish(&mut self) {
        // pinyin.cpp:1200 writes the marker before deleting the tables.
        self.user.finish();
        // Close open files in member order (pinyin.cpp:1205-1215 and
        // zhuyin.cpp:744-748). A second finish finds them closed.
        self.system.finish_attachments();
        self.model.finish_attachment();
        self.addons.finish_attachments();
        self.punct.finish_attachment();
    }

    /// Whether [`Self::finish`] has closed every native file attachment.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn attachments_finished(&self) -> bool {
        self.system.attachments_finished()
            && self.model.attachment_finished()
            && self.addons.attachments_finished()
            && self.punct.attachment_finished()
    }

    /// Whether the main pinyin index failed to open.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn pinyin_attachment_failed(&self) -> bool {
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
/// (`pinyin.cpp:187`).
///
/// # Aborts
///
/// An unknown database format aborts during the profile read.
fn check_profile_early(
    conf: &TableConf,
    user_dir: Option<&Path>,
    law: UserConfLaw,
) -> Option<crate::user::ProfileCheck> {
    let versions = SystemVersions::from_conf(conf);
    let Some(user_dir) = user_dir else {
        UserStore::check_profile_unnamed(&versions, law);
        return None;
    };
    let layout = UserFileLayout::from_conf(conf);
    Some(UserStore::check_profile(user_dir, &versions, &layout, law))
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
    /// refuses, including a missing file, or
    /// when a required file is missing or a DBM or chunk file cannot be
    /// opened, and [`OpenError::UserSession`] where the user session cannot
    /// be built after the libraries were handed to it.
    #[cfg(test)]
    pub fn open(system_dir: &Path, user_dir: Option<&Path>) -> Result<Self, OpenError> {
        Self::open_with_law(system_dir, user_dir, UserConfLaw::Pinyin)
    }

    /// [`Runtime::open`] with the user dir's `user.conf` law named: the
    /// pinyin facade's init passes [`UserConfLaw::Pinyin`], the zhuyin
    /// facade's [`UserConfLaw::Zhuyin`] — the two pins share the data
    /// directory and the user file set but not the open counter's
    /// lifecycle (`crate::user::UserConfLaw`). The law also
    /// selects the facade's system tables: only pinyin opens the addon
    /// indexes and punctuation table (074a2219 `pinyin.cpp:416-440`;
    /// `zhuyin.cpp:270-358`).
    ///
    /// # Errors
    ///
    /// As [`Runtime::open`].
    #[cfg(test)]
    pub(crate) fn open_with_law(
        system_dir: &Path,
        user_dir: Option<&Path>,
        law: UserConfLaw,
    ) -> Result<Self, OpenError> {
        Self::open_with_law_reporting(system_dir, user_dir, law, &mut |_| {})
    }

    /// Calls `unmapped` at each default row where the pin writes
    /// `mmap %s failed!` (`pinyin.cpp:253-256`, `zhuyin.cpp:197-200`).
    /// A fault ends the loop before the next row.
    ///
    /// # Errors
    ///
    /// As [`Runtime::open`].
    pub(crate) fn open_with_law_reporting(
        system_dir: &Path,
        user_dir: Option<&Path>,
        law: UserConfLaw,
        unmapped: &mut dyn FnMut(&Path),
    ) -> Result<Self, OpenError> {
        // `SystemTableInfo2::load`, the pin's first step: every refusal,
        // a missing file included, fails the init (`pinyin.cpp:335-340`,
        // `zhuyin.cpp:279-284`).
        let conf = TableConf::load_from_dir(system_dir).map_err(OpenError::TableConf)?;
        let early = check_profile_early(&conf, user_dir, law);
        let user_profile = user_dir.zip(early);
        let pinyin = SystemTables::attach_pinyin(system_dir).map_err(OpenError::Dict)?;
        let user_pinyin = crate::user::LoadedPinyinIndex::open(user_profile);
        let dict =
            SystemTables::attach_phrase(system_dir, &conf, pinyin).map_err(OpenError::Dict)?;
        let mut profile = user_pinyin.load_phrase_table();
        // The library mask is shared with the language model: an unloaded
        // library's items leave both the lookups and the unigram denominator,
        // as freeing the sub-index does upstream.
        let library_mask = 0;
        let mut originals = crate::user::system_originals(
            crate::data::PhraseLibraries::empty(),
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
        // user-dir file set: the profile is read through
        // check_format, the session runs in memory, and `pinyin_save`
        // writes the pin's files — a same-backend libpinyin picks them
        // up seamlessly.
        let rows = LibraryLoopRows::new(system_dir, &conf, user_dir);
        profile
            .load_libraries(user_profile, &mut originals, &rows, unmapped)
            .map_err(library_loop_error)?;
        // The system bigram attaches first (pinyin.cpp:394-401, zhuyin.cpp:336-343).
        let bigram = crate::input::build_path(
            system_dir,
            std::ffi::OsStr::new(&SystemDbm::Bigram.file_name()),
        );
        let lm =
            crate::data::BigramTables::attach(&bigram, conf.lambda_f32()).map_err(OpenError::Lm)?;
        let user = UserStore::open_session(
            user_dir,
            originals,
            SystemVersions::from_conf(&conf),
            law,
            profile,
        )
        .map_err(OpenError::UserSession)?;

        // λ is the `gfloat` table.conf's scanner read.

        let addons = match law {
            UserConfLaw::Pinyin => {
                AddonDictionary::attach_with(system_dir, &conf).map_err(OpenError::Dict)?
            }
            UserConfLaw::Zhuyin => AddonDictionary::without_indexes(&conf),
        };
        let punct = match law {
            UserConfLaw::Pinyin => {
                PunctTable::attach(&system_dir.join(SystemDbm::Punct.file_name()))
                    .map_err(OpenError::Dict)?
            }
            UserConfLaw::Zhuyin => PunctTable::empty(),
        };

        Ok(Self {
            system_dir: system_dir.to_path_buf(),
            system: dict,
            model: lm,
            user,
            conf,
            addons: Box::new(addons),
            punct,
            library_mask,
        })
    }

    fn libraries(&self) -> &crate::data::PhraseLibraries {
        self.user.originals().phrase_libraries()
    }

    fn originals_mut(&mut self) -> &mut crate::user::SystemOriginals {
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
    pub(crate) fn new_session(&self, settings: SessionSettings) -> crate::engine::SessionState {
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
            library_mask: self.library_mask,
        }
    }

    /// Borrow the overlaid language model for one operation.
    #[must_use]
    pub fn lm(&self) -> RuntimeLm<'_> {
        RuntimeLm {
            inner: self.model.read(self.libraries(), self.library_mask),
            user: &self.user,
            addons: &self.addons,
        }
    }

    /// Borrow composition and phrase storage for one operation.
    pub fn session<'a>(&'a self, state: &'a mut crate::engine::SessionState) -> RuntimeSession<'a> {
        state.bind(self.dict(), self.lm())
    }

    /// Borrow the learning store without retaining an independent owner.
    #[must_use]
    pub fn user_store(&self) -> &UserStore {
        &self.user
    }

    // pinyin_train checks the directory and result count and asserts the selected index,
    // pinyin.cpp:2670–2690; zhuyin.cpp:1696–1715. train_result3 applies successive
    // observations, lookup/phonetic_lookup.h:844–936.
    /// Applies the retained session's training plan at `index`. A runtime without a user
    /// directory or a session without a training plan returns `Ok(false)`. Select an existing
    /// sentence row when rows are present.
    ///
    /// # Errors
    ///
    /// Returns the first training error. Completed observations can survive a later error.
    ///
    /// # Aborts
    ///
    /// Terminates where the pin's own training abort fires: the index past
    /// nonempty results (`pinyin.cpp:2684` at 074a2219), a stale forced
    /// token (`phonetic_lookup.h:868`) or a stale span
    /// (`storage/phonetic_key_matrix.cpp:661`, `:663`, `:664`). The abort
    /// precedes the modified mark, as the pin's abort precedes its save.
    pub(crate) fn train_session(
        &mut self,
        state: &crate::engine::SessionState,
        index: u8,
    ) -> Result<bool, EngineError> {
        let user = &mut *self.user;
        if !user.has_user_directory() {
            return Ok(false);
        }
        // The pin marks the context modified once results exist, before
        // its walk (`pinyin.cpp:2679-2681`); the steps a refusal leaves
        // recorded are saved with it.
        match state.train(index, user) {
            Ok(false) => Ok(false),
            Ok(true) => {
                user.mark_modified();
                Ok(true)
            }
            Err(crate::engine::TrainingStop::PinAbort) => std::process::abort(),
            Err(crate::engine::TrainingStop::User(error)) => {
                user.mark_modified();
                Err(training_error(&error))
            }
        }
    }

    /// Apply a unigram increment with exclusive access to the storage owner.
    #[must_use]
    pub fn add_unigram_delta(&mut self, token: u32, delta: u32) -> bool {
        self.add_unigram_outcome(token, delta).is_accepted()
    }

    /// [`Self::add_unigram_delta`] with the answer in the pin's terms. A
    /// caller that acts on `ERROR_INTEGER_OVERFLOW` alone, as the choose legs
    /// do (`pinyin.cpp:2527-2531`, `:2567-2575`), tells [`UnigramAdd::Refused`]
    /// from an unloaded library or a missing item, which are
    /// [`UnigramAdd::Missing`].
    #[must_use]
    pub fn add_unigram_outcome(&mut self, token: u32, delta: u32) -> UnigramAdd {
        // PHRASE_INDEX_LIBRARY_INDEX and PHRASE_MASK ignore the tag nibble
        // (074a2219 novel_types.h:41-44, phrase_index.cpp:154).
        let token = token & (crate::vocab::PHRASE_INDEX_LIBRARY_MASK | crate::vocab::PHRASE_MASK);
        if !self
            .dict()
            .library_visible(phrase_index_library_index(token))
        {
            return UnigramAdd::Missing;
        }
        self.user.add_unigram_outcome(token, delta)
    }

    /// Publish dictionary remaps while retaining any unreplaced journal originals.
    pub(crate) fn install_baselines(&mut self, originals: crate::user::SystemOriginals) {
        self.originals_mut().publish_current(originals);
    }

    /// Borrow the learning store exclusively for a mutation.
    pub fn user_store_mut(&mut self) -> &mut UserStore {
        &mut self.user
    }

    /// Loads addon library `index` from the system directory; `Ok(false)`
    /// when the library is already loaded.
    ///
    /// # Errors
    ///
    /// [`LibraryError`] when the library file is absent or does not verify.
    pub(crate) fn load_system_addon_reporting(&mut self, index: u8) -> Result<bool, LibraryError> {
        self.addons.load_reporting(index, &self.system_dir)
    }

    /// The system directory the libraries load from.
    #[must_use]
    pub fn system_dir(&self) -> &Path {
        &self.system_dir
    }

    /// `FacadePhraseIndex::compact` over the system layout-failed empty
    /// sub-indices that are loaded: an unloaded library has no sub-index to
    /// compact (`phrase_index.cpp:651-652`). Returns the compacted nibbles.
    #[must_use]
    pub(crate) fn compact_layout_failed(&mut self) -> Vec<u8> {
        let unloaded = self.unloaded_libraries();
        self.originals_mut()
            .phrase_libraries_mut()
            .compact_layout_failed(unloaded)
    }

    /// The current loaded-library mask, snapshotted for an exclusive save.
    pub(crate) fn unloaded_libraries(&self) -> u32 {
        self.library_mask
    }

    /// Starts the single map for an explicit load. The facade uses the same
    /// result for the diagnostic and the replacement sub-index
    /// (`pinyin.cpp:242-277`, `zhuyin.cpp:186-221`).
    #[must_use]
    pub(crate) fn begin_library_reload(&self, index: u8) -> RemapWalk {
        self.begin_remap(index..index.saturating_add(1), true)
    }

    /// Starts the mask loop's remap over every loaded SYSTEM_FILE/DICTIONARY
    /// default library, in index order (074a2219 pinyin.cpp:1253-1273,
    /// zhuyin.cpp:788-808). Advance it with [`Self::remap_next`] and end it
    /// with [`RemapWalk::into_originals`].
    #[must_use]
    pub(crate) fn begin_system_remap(&self) -> RemapWalk {
        self.begin_remap(1..16, false)
    }

    fn begin_remap(&self, indices: std::ops::Range<u8>, reload: bool) -> RemapWalk {
        let layout = UserFileLayout::from_conf(&self.conf);
        RemapWalk {
            originals: crate::user::SystemOriginals::new(Default::default(), layout),
            indices,
            reload,
        }
    }

    /// Maps the walk's next library and says what it reached; `None` when
    /// the walk is over. A directory ends it. Nothing after the library
    /// returned is opened until the next call, so a caller that stops there
    /// leaves every later library untouched.
    pub(crate) fn remap_next(&self, walk: &mut RemapWalk) -> Option<RemapStep> {
        let dir = &self.system_dir;
        let libraries = self.libraries();
        while let Some(index) = walk.indices.next() {
            let Some(row) = self.conf.default_row(index) else {
                continue;
            };
            // The mask loop skips a library that is not loaded
            // (`pinyin.cpp:1240-1241`) and a row that names no user file
            // (`:1250-1251`). A reload does neither: it maps the unloaded
            // library, and the user name is not read until it has.
            if !matches!(row.file_type, FileType::SystemFile | FileType::Dictionary)
                || (!walk.reload
                    && (row.user.is_none()
                        || !self.dict().library_visible(index)
                        || !libraries.is_loaded(index)))
            {
                continue;
            }
            let Some(name) = row.system.as_ref() else {
                continue;
            };
            let path = crate::input::build_path(dir, name.as_ref());
            let opened = if walk.reload {
                crate::data::phrase_library::PhraseLibrary::open_retaining_layout(&path)
            } else {
                crate::data::phrase_library::PhraseLibrary::open_for_mask_prefetch(&path)
            };
            match opened {
                Ok(library) => {
                    walk.originals.insert_mapping(index, path, library);
                    return Some(RemapStep::Mapped(index));
                }
                Err(LibraryError::Row(RowFault::ChunkHeaderRead)) => {
                    walk.indices = 0..0;
                    return Some(RemapStep::Directory);
                }
                Err(
                    error @ (LibraryError::Io(_)
                    | LibraryError::Format(_)
                    | LibraryError::Layout(_)
                    | LibraryError::Unmappable { .. }
                    | LibraryError::Row(RowFault::NullChunkName)),
                ) => {
                    if let Some(path) = error.unmappable_path() {
                        return Some(RemapStep::Unmappable(path.to_path_buf()));
                    }
                }
            }
        }
        None
    }

    /// Unloads addon library `index` from this runtime's dictionary.
    ///
    /// The bound the pin asserts on `index` is the caller's to apply; see
    /// [`AddonDictionary::unload`].
    #[must_use]
    pub fn unload_system_addon(&mut self, index: u8) -> bool {
        self.addons.unload(index)
    }

    /// Loads default library `index` — the GBK-reload path after an
    /// unload; `false` when already visible.
    #[must_use]
    pub(crate) fn load_library(&mut self, index: u8) -> bool {
        if index >= PHRASE_INDEX_LIBRARY_COUNT {
            return false;
        }
        let bit = 1u32 << index;
        let was_set = self.library_mask & bit != 0;
        self.library_mask &= !bit;
        self.user.set_library_loaded(index, true);
        was_set
    }

    /// Unloads default library `index`; `false` only outside the sixteen
    /// sub-indices, `true` on every call otherwise (the pin ignores what
    /// `unload` returns, `pinyin.cpp:473-474`). Which indices a facade lets
    /// through is the C ABI's rule.
    #[must_use]
    pub fn unload_library(&mut self, index: u8) -> bool {
        if index >= PHRASE_INDEX_LIBRARY_COUNT {
            return false;
        }
        self.library_mask |= 1u32 << index;
        self.user.set_library_loaded(index, false);
        true
    }
}

#[cfg(test)]
mod tests;
