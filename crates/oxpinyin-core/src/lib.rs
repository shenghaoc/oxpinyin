//! Pure pinyin algorithms: parsing, `SegmentGraph`, k-best search, scoring
//! traits. No I/O, no platform deps, no `unsafe`. Internal crate — the
//! supported public API is `oxpinyin-engine`.
#![forbid(unsafe_code)]
#![warn(missing_docs)]
// Constitution §4, mechanically: library builds may not unwrap, expect,
// or panic. Inline #[cfg(test)] modules are exempt (see the allow below
// their declaration); tests/, benches/ and examples/ are separate crates.
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::panic))]
#![cfg_attr(not(test), deny(clippy::panic_in_result_fn))]

pub mod cost;
mod full_pinyin_index;
pub mod graph;
pub mod kbest;
pub mod options;
mod parser;
mod scheme;
pub mod scoring;
mod syllables;
mod vocab;
mod zhuyin_map;
pub use zhuyin_map::{zhuyin_display_for_pinyin, zhuyin_for_pinyin};

pub use full_pinyin_index::{
    FullPinyinIndexKey, FullPinyinIndexParse, LUOMA_PINYIN_COUNT, LUOMA_PINYIN_INDEX,
    MAX_INDEX_SPELLING_LEN, SECONDARY_ZHUYIN_COUNT, SECONDARY_ZHUYIN_INDEX,
    parse_full_pinyin_index, parse_full_pinyin_index_with_options,
};
pub use options::{
    DYNAMIC_ADJUST, FORCE_TONE, OptionBits, PINYIN_AMB_ALL, PINYIN_AMB_AN_ANG, PINYIN_AMB_C_CH,
    PINYIN_AMB_EN_ENG, PINYIN_AMB_F_H, PINYIN_AMB_G_K, PINYIN_AMB_IN_ING, PINYIN_AMB_L_N,
    PINYIN_AMB_L_R, PINYIN_AMB_S_SH, PINYIN_AMB_Z_ZH, PINYIN_CORRECT_ALL, PINYIN_CORRECT_GN_NG,
    PINYIN_CORRECT_IOU_IU, PINYIN_CORRECT_MG_NG, PINYIN_CORRECT_ON_ONG, PINYIN_CORRECT_UE_VE,
    PINYIN_CORRECT_UEI_UI, PINYIN_CORRECT_UEN_UN, PINYIN_CORRECT_V_U, PINYIN_INCOMPLETE,
    USE_DIVIDED_TABLE, USE_RESPLIT_TABLE, USE_TONE, ZHUYIN_CORRECT_ALL, ZHUYIN_CORRECT_ETEN26,
    ZHUYIN_CORRECT_HSU, ZHUYIN_CORRECT_SHUFFLE, ZHUYIN_INCOMPLETE,
};
pub use oxpinyin_chewing::{CHEWING_ZERO_TONE, ChewingKey};
pub use parser::{
    Completeness, FullPinyinParser, MAX_PARSE_RESULTS, ParseError, ParseResult, ParsedSyllable,
};
pub use scheme::{
    DoublePinyinKey, DoublePinyinParse, DoublePinyinParser, DoublePinyinScheme, FullPinyinScheme,
    ZhuyinKey, ZhuyinParse, ZhuyinParser, ZhuyinScheme, parse_zhuyin_direct,
};
pub use syllables::{
    FULL_PINYIN_SYLLABLE_COUNT, FULL_PINYIN_SYLLABLES, INCOMPLETE_PINYIN_KEY_COUNT,
    INCOMPLETE_PINYIN_KEYS, MAX_SYLLABLE_LEN, OPTION_ONLY_COMPLETE_SYLLABLE_COUNT,
    OPTION_ONLY_COMPLETE_SYLLABLES, ZHUYIN_ONLY_COMPLETE_SYLLABLE_COUNT,
    ZHUYIN_ONLY_COMPLETE_SYLLABLES, canonical_complete_syllable, option_alias_canonical,
    phonetic_initial, syllable_initial,
};
pub use vocab::{PhraseEntry, PhraseToken, SYLLABLE_KEY_COUNT, SyllableKey};

/// Deterministic signed cost used by scoring seams.
///
/// The concrete scale is defined by the decoder/scoring specification.
pub type Cost = i64;

/// User-side count overlay for the decode-time additive merge
/// (`docs/findings/user-store.md` §5).
///
/// The merge is count addition, not a user-vs-system weight: λ still
/// interpolates bigram-vs-unigram over the *merged* counts. All zeros is
/// identity — `system.saturating_add(0) == system` — so an empty store is
/// indistinguishable from no store.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct UserCountDelta {
    /// User bigram count for `(prev → token)`.
    pub bigram_count: u64,
    /// User bigram total mass after `prev`.
    pub bigram_total: u64,
    /// User phrase-index unigram delta for `token`.
    pub unigram_delta: u64,
    /// Sum of every user unigram delta (the user contribution to the
    /// unigram total).
    pub unigram_total_delta: u64,
}

impl UserCountDelta {
    /// No user data: the additive merge is a no-op.
    pub const ZERO: Self = Self {
        bigram_count: 0,
        bigram_total: 0,
        unigram_delta: 0,
        unigram_total_delta: 0,
    };
}

/// Read-only lookup seam for dictionaries.
///
/// Implementations return entries in stable order, use an empty vector for a
/// successful lookup with no entries, and do not panic on caller input.
pub trait Dictionary {
    /// Syllable representation accepted by this dictionary.
    type Syllable;

    /// Entry representation returned by this dictionary.
    type Entry;

    /// Lookup failure reported by this dictionary.
    type Error;

    /// Looks up entries matching `syllables`.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] when the dictionary backend cannot serve the lookup.
    fn lookup(&self, syllables: &[Self::Syllable]) -> Result<Vec<Self::Entry>, Self::Error>;

    /// Returns every matching pronunciation index record in library, range,
    /// and item order, retaining repeated tokens for the sentence trellis.
    /// `tones` supplies the query tones; missing tones are zero.
    /// Ordinary lookup consumers continue to use [`Self::lookup`].
    ///
    /// The default preserves existing dictionary implementations.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] when the dictionary backend cannot serve the lookup.
    fn trellis_records(
        &self,
        syllables: &[Self::Syllable],
        _tones: &[u8],
    ) -> Result<Vec<Self::Entry>, Self::Error> {
        self.lookup(syllables)
    }

    /// [`Self::lookup`] into a caller-owned buffer, which keeps capacity
    /// across keystrokes on the window-scan path.
    ///
    /// The default copies [`Self::lookup`]'s vector. Implementors that
    /// already fill a `Vec` should override this and `clear`+`push` into
    /// `out` instead.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] when the dictionary backend cannot serve the lookup.
    fn lookup_into(
        &self,
        syllables: &[Self::Syllable],
        out: &mut Vec<Self::Entry>,
    ) -> Result<(), Self::Error> {
        let entries = self.lookup(syllables)?;
        out.clear();
        out.extend(entries);
        Ok(())
    }

    /// [`Self::lookup_into`], also answering whether the lookup met a token
    /// of a loaded library whose item could not be read (the entry is
    /// dropped, as ever). The pin's candidate listing dies on such a token
    /// (`pinyin.cpp:1635-1637`, `:2053`); the answer rides the call that met
    /// it, not shared state, so the caller can fail exactly the listing it
    /// was building.
    ///
    /// # Errors
    ///
    /// As [`Self::lookup_into`].
    fn lookup_into_flagged(
        &self,
        syllables: &[Self::Syllable],
        out: &mut Vec<Self::Entry>,
    ) -> Result<bool, Self::Error> {
        self.lookup_into(syllables, out)?;
        Ok(false)
    }

    /// Whether a stored phrase's pinyin can extend `syllables`.
    ///
    /// This is libpinyin's `SEARCH_CONTINUED` probe: the phrase index reports
    /// the bit when the key sequence is a prefix of some stored phrase's
    /// pinyin, which is what lets the candidate window scan stop widening.
    /// When `syllables` contains an initial-only key the probe runs on the
    /// initial sequence instead — the index the pin uses for incomplete
    /// spellings — where a complete key contributes its own initial and an
    /// initial-only key stands for every syllable that shares it.
    ///
    /// Defaulted to `true` so the frozen implementors keep compiling: always
    /// continuing the scan yields the same candidate set (the probe only
    /// stops windows that would return nothing), at the cost of searching
    /// every window to the end of the input
    /// (`docs/findings/core-trait-seam.md`: the seam grows by defaulted
    /// methods only).
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] when the dictionary backend cannot evaluate the prefix probe.
    fn phrase_prefix_exists(&self, _syllables: &[Self::Syllable]) -> Result<bool, Self::Error> {
        Ok(true)
    }

    /// The phrase tokens stored for an exact UTF-8 `text`, the probe
    /// `pinyin_phrase_segment`'s span DP looks up per character span
    /// (`PhraseLookup::get_best_match` searches the phrase table per
    /// span; `phrase_lookup.cpp:135-157` at the pin). Empty for every
    /// existing implementor except the table-backed dictionaries, which
    /// answer from their reverse text maps in stored order (system seam
    /// first, then the user seam — the same merge order as
    /// [`Dictionary::lookup`]).
    ///
    /// Defaulted to empty so existing implementors compile unchanged
    /// (`docs/findings/core-trait-seam.md`: the seam grows by defaulted
    /// methods only).
    fn tokens_for_text(&self, _text: &str) -> Vec<PhraseToken> {
        Vec::new()
    }

    /// Addon-facade lookup pass, empty for every existing implementor.
    ///
    /// W11: the default facade stays on [`Dictionary::lookup`]; the addon
    /// facade is a second pass so an addon token never has to be
    /// distinguished from `MERGED_DICTIONARY` by nibble alone
    /// (`docs/findings/phrase-union.md` §6.3). Defaulted so other
    /// implementors compile unchanged.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] when the addon backend cannot serve the lookup.
    fn lookup_addon(&self, _syllables: &[Self::Syllable]) -> Result<Vec<Self::Entry>, Self::Error> {
        Ok(Vec::new())
    }

    /// [`Self::lookup_addon`] into a caller-owned buffer.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] when the addon backend cannot serve the lookup.
    fn lookup_addon_into(
        &self,
        syllables: &[Self::Syllable],
        out: &mut Vec<Self::Entry>,
    ) -> Result<(), Self::Error> {
        let entries = self.lookup_addon(syllables)?;
        out.clear();
        out.extend(entries);
        Ok(())
    }

    /// Addon-facade `SEARCH_CONTINUED` probe, `false` for every existing
    /// implementor.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] when the addon backend cannot evaluate the prefix probe.
    fn phrase_prefix_exists_addon(
        &self,
        _syllables: &[Self::Syllable],
    ) -> Result<bool, Self::Error> {
        Ok(false)
    }

    /// Number of phrase items the default facade carries.
    ///
    /// The pinned oracle's candidate frequency divides every unigram by the
    /// phrase index's total frequency (`pinyin.cpp:1858`), and that total is
    /// the sum of one baked unigram per item. A table-backed implementor
    /// answers with its item count so the ranking can reproduce that
    /// denominator; `Ok(0)` is the fixture answer, where no such index
    /// exists. Fallible like every other state-reading method of this
    /// trait: an implementor that must query its store reports the failure
    /// instead of guessing a count.
    ///
    /// Defaulted so existing implementors compile unchanged
    /// (`docs/findings/core-trait-seam.md`: the seam grows by defaulted
    /// methods only).
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] when the backend cannot report the item count.
    fn phrase_index_item_count(&self) -> Result<u64, Self::Error> {
        Ok(0)
    }

    /// Whether [`Self::lookup_into`] and [`Self::lookup_addon_into`] accept
    /// queries whose syllables mix complete and initial-only keys under
    /// upstream's `pinyin_compare_with_tones` matching rule.
    ///
    /// A `true` return says the implementor's stored index answers a query
    /// with any [`Completeness::Partial`] syllable in one call, returning
    /// every record whose stored keys match the query — a complete syllable
    /// exactly, an initial-only syllable by shared initial. That is the
    /// libpinyin `chewing_large_table2.cpp:161-172` (pin `074a2219`)
    /// contract: `add_index` writes each phrase into both the incomplete-
    /// projected and the complete keyspace at `:184-197`, and `search`
    /// dispatches on `contains_incomplete_pinyin`. The oxpinyin
    /// [`SystemDictionary`](../../oxpinyin-data/src/dict.rs) and
    /// [`UserLookup`](../../oxpinyin-user/src/lookup.rs) both hold that
    /// invariant.
    ///
    /// A `false` return (the default) says the implementor answers only
    /// exact-key queries and callers must enumerate completions themselves
    /// — that is the behaviour of fixture-backed dictionaries that store
    /// entries under complete-key sequences alone. The window-scan caller
    /// consults this flag and expands partial paths through
    /// [`crate::scoring::expand_keys`] when it is false, so implementations
    /// stay compatible without a scan-side branch per implementor.
    #[must_use]
    fn handles_partial_keys(&self) -> bool {
        false
    }

    /// Tokens of phrases whose stored pinyin **strictly extends**
    /// `syllables` — `ChewingLargeTable2::search_suggestion`
    /// (`chewing_large_table2_tkrzwdb.cpp:493-535` at the pin `074a2219`),
    /// the row source of the LONGER candidate
    /// (`_prepend_longer_candidates`, `pinyin.cpp:1870-1933`).
    ///
    /// The pin's walk: the query's index key (tone-zeroed complete, or the
    /// initial-only incomplete projection when any query syllable is
    /// partial) must itself exist in the DBM; the cursor then walks the
    /// rows **after** it, keeping a row when its key is strictly longer
    /// and its first `prefix_len` keys equal the query under
    /// `pinyin_compare_with_tones` (an incomplete query syllable accepts
    /// every final; a zero tone accepts every tone), and each surviving
    /// record's token is appended in cursor order. Rows at the query's
    /// own length are the phrase itself and are never returned.
    ///
    /// Tokens come out in the pin's `reduce_tokens` order: grouped by
    /// library nibble ascending, cursor order within a group — the
    /// winner-selection loop over them keeps the first token on unigram
    /// ties, so the order is observable. Defaulted to empty so existing
    /// implementors compile unchanged (`docs/findings/core-trait-seam.md`:
    /// the seam grows by defaulted methods only) — a dictionary that does
    /// not override it surfaces no LONGER candidate, the pre-§9 behaviour
    /// of every fixture double.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] when the backend cannot serve the walk.
    fn suggest_extension_tokens(
        &self,
        _syllables: &[Self::Syllable],
    ) -> Result<Vec<PhraseToken>, Self::Error> {
        Ok(Vec::new())
    }

    /// Suggestions with the overlong-index gate checked in the same walk.
    /// `None` names the pin's `search_suggestion` abort; `Some` holds the
    /// tokens, including an empty list when the query has no extensions.
    /// The default preserves existing dictionary implementations.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] when the backend cannot serve the walk.
    fn checked_extension_tokens(
        &self,
        syllables: &[Self::Syllable],
    ) -> Result<Option<Vec<PhraseToken>>, Self::Error> {
        if self.overlong_extension_gate(syllables) {
            Ok(None)
        } else {
            self.suggest_extension_tokens(syllables).map(Some)
        }
    }

    /// The phrase text a token owns, for rows the candidate surface must
    /// render from the token alone — `_token_get_phrase`
    /// (`pinyin.cpp:1994-2050` at the pin), which fills a LONGER
    /// candidate's string from the phrase index item.
    ///
    /// Defaulted to `None` so existing implementors compile unchanged;
    /// a dictionary that does not override it cannot back a LONGER row's
    /// text (the row is then not built).
    fn phrase_text_for_token(&self, _token: u32) -> Option<String> {
        None
    }

    /// The pin's user-chewing-table over-long-key gate
    /// (`src/storage/chewing_large_table2_bdb.cpp:282`): `true` when the
    /// dictionary's user index holds a reading past `MAX_PHRASE_LENGTH`
    /// syllables that extends `syllables`, so the pin's longer-candidate
    /// walk reaches the key and dies at `switch`'s `default: abort()`.
    ///
    /// Defaulted to `false` so existing implementors and fixture doubles
    /// compile unchanged (the seam grows by defaulted methods only,
    /// `docs/findings/core-trait-seam.md`): only a user-index-backed
    /// dictionary answers otherwise.
    fn overlong_extension_gate(&self, _syllables: &[Self::Syllable]) -> bool {
        false
    }
}

impl<D: Dictionary + ?Sized> Dictionary for &D {
    type Syllable = D::Syllable;
    type Entry = D::Entry;
    type Error = D::Error;

    fn lookup(&self, syllables: &[Self::Syllable]) -> Result<Vec<Self::Entry>, Self::Error> {
        (**self).lookup(syllables)
    }

    fn trellis_records(
        &self,
        syllables: &[Self::Syllable],
        tones: &[u8],
    ) -> Result<Vec<Self::Entry>, Self::Error> {
        (**self).trellis_records(syllables, tones)
    }

    fn lookup_into(
        &self,
        syllables: &[Self::Syllable],
        out: &mut Vec<Self::Entry>,
    ) -> Result<(), Self::Error> {
        (**self).lookup_into(syllables, out)
    }

    fn lookup_into_flagged(
        &self,
        syllables: &[Self::Syllable],
        out: &mut Vec<Self::Entry>,
    ) -> Result<bool, Self::Error> {
        (**self).lookup_into_flagged(syllables, out)
    }

    fn phrase_prefix_exists(&self, syllables: &[Self::Syllable]) -> Result<bool, Self::Error> {
        (**self).phrase_prefix_exists(syllables)
    }

    fn tokens_for_text(&self, text: &str) -> Vec<PhraseToken> {
        (**self).tokens_for_text(text)
    }

    fn lookup_addon(&self, syllables: &[Self::Syllable]) -> Result<Vec<Self::Entry>, Self::Error> {
        (**self).lookup_addon(syllables)
    }

    fn lookup_addon_into(
        &self,
        syllables: &[Self::Syllable],
        out: &mut Vec<Self::Entry>,
    ) -> Result<(), Self::Error> {
        (**self).lookup_addon_into(syllables, out)
    }

    fn phrase_prefix_exists_addon(
        &self,
        syllables: &[Self::Syllable],
    ) -> Result<bool, Self::Error> {
        (**self).phrase_prefix_exists_addon(syllables)
    }

    fn phrase_index_item_count(&self) -> Result<u64, Self::Error> {
        (**self).phrase_index_item_count()
    }

    fn handles_partial_keys(&self) -> bool {
        (**self).handles_partial_keys()
    }

    fn suggest_extension_tokens(
        &self,
        syllables: &[Self::Syllable],
    ) -> Result<Vec<PhraseToken>, Self::Error> {
        (**self).suggest_extension_tokens(syllables)
    }

    fn checked_extension_tokens(
        &self,
        syllables: &[Self::Syllable],
    ) -> Result<Option<Vec<PhraseToken>>, Self::Error> {
        (**self).checked_extension_tokens(syllables)
    }

    fn phrase_text_for_token(&self, token: u32) -> Option<String> {
        (**self).phrase_text_for_token(token)
    }

    fn overlong_extension_gate(&self, syllables: &[Self::Syllable]) -> bool {
        (**self).overlong_extension_gate(syllables)
    }
}

/// Scoring and observation seam for explicit user-learning state.
///
/// Implementations are deterministic for the same explicit input and state,
/// do not consult hidden process-global state, and do not panic on caller
/// input.
pub trait UserModel {
    /// Token representation scored and observed by this model.
    type Token;

    /// Scoring or observation failure reported by this model.
    type Error;

    /// Returns the user-specific cost for `token` after `history`.
    ///
    /// The §5 decode-time merge is count addition *before* the λ blend, so
    /// a `Cost` returned here is not the merge — adding it to the language-
    /// model cost would be a new weighting scheme. Implementors that hold
    /// counts expose them as [`UserCountDelta`]; the language model folds
    /// that overlay into the frozen interpolated formula.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] when the model backend cannot score the transition.
    fn score(&self, history: &[Self::Token], token: &Self::Token) -> Result<Cost, Self::Error>;

    /// Records an accepted `token` after `history`.
    ///
    /// Learning-off callers omit this operation entirely.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] when the model backend cannot record the observation.
    fn observe(&mut self, history: &[Self::Token], token: &Self::Token) -> Result<(), Self::Error>;

    /// Records a trained token and the tone-bearing readings of its matrix span.
    ///
    /// Readings retain matrix order and multiplicity; each has the phrase's
    /// syllable length. The default preserves existing implementors by
    /// delegating to [`Self::observe`]. Learning-off callers omit this call.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] when the observation cannot be recorded.
    fn observe_with_keys(
        &mut self,
        history: &[Self::Token],
        token: &Self::Token,
        _readings: &mut dyn Iterator<Item = Vec<ChewingKey>>,
    ) -> Result<(), Self::Error> {
        self.observe(history, token)
    }
}

/// One `prev → *` bigram row with the user overlay already merged — the
/// result of upstream's `merge_single_gram` (`pinyin.cpp:2209`).
///
/// Loaded **once** per candidate guess and indexed per candidate, which is
/// the pin's structure: `pinyin_guess_candidates` merges one gram outside
/// its candidate loop and `_compute_frequency_of_items` then calls
/// `merged_gram->get_freq(token, ...)` inside it. Merging per candidate
/// would turn one row load into one per candidate and worsen time without
/// buying space, which the source policy forbids.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MergedGram {
    total: u64,
    records: Vec<(u32, u64)>,
}

impl MergedGram {
    /// Builds a row from its total and `(token, count)` records.
    ///
    /// Records are sorted by token so [`Self::count`] is a binary search;
    /// upstream's `SingleGram` keeps its item array sorted by token for the
    /// same reason (`ngram.cpp:178-196`).
    #[must_use]
    pub fn new(total: u64, mut records: Vec<(u32, u64)>) -> Self {
        records.sort_unstable_by_key(|(token, _)| *token);
        Self { total, records }
    }

    /// The row total — upstream's `get_total_freq`.
    #[must_use]
    pub const fn total(&self) -> u64 {
        self.total
    }

    /// The count stored for `token`, or 0 — upstream's `get_freq`, whose
    /// out-param stays 0 on a miss.
    #[must_use]
    pub fn count(&self, token: u32) -> u64 {
        self.count_if_present(token).unwrap_or(0)
    }

    /// A stored successor's count, retaining presence when its count is zero.
    #[must_use]
    pub fn count_if_present(&self, token: u32) -> Option<u64> {
        self.records
            .binary_search_by_key(&token, |(stored, _)| *stored)
            .ok()
            .map(|index| self.records[index].1)
    }

    /// `bigram_freq / total` as the pin computes it — `gfloat` division,
    /// and 0 when the total is 0 (`pinyin.cpp:1855-1857` guards on it).
    #[must_use]
    pub fn possibility(&self, token: u32) -> f32 {
        if self.total == 0 {
            return 0.0;
        }
        self.count(token) as f32 / self.total as f32
    }

    /// Whether the row carries no records.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Number of records in the row.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.records.len()
    }

    /// Sorted successor records, including stored zero-count successors.
    #[must_use]
    pub fn records(&self) -> &[(u32, u64)] {
        &self.records
    }
}

/// One predecessor's lazily loaded row, owned by a single n-best step.
/// Drop it before advancing the step; training and mask changes must be
/// observed by the next step or guess. A loaded miss is reused too.
#[derive(Default)]
pub struct NbestStepRow {
    loaded: Option<MergedGram>,
    // The generation tag adds one word per cached row. None means unloaded;
    // a loaded missing gram still has a nonzero generation tag.
    user_generation: Option<core::num::NonZeroU64>,
    raw: Option<NbestRawGram>,
    raw_generation: Option<core::num::NonZeroU64>,
}

impl NbestStepRow {
    /// Loads once on success, including a missing row. Errors are propagated
    /// without turning corruption into a cached miss.
    ///
    /// # Errors
    /// Returns the loader's error unchanged.
    pub fn get_or_try_init<E>(
        &mut self,
        load: impl FnOnce() -> Result<Option<MergedGram>, E>,
    ) -> Result<Option<&MergedGram>, E> {
        self.get_or_try_init_in_generation(0, load)
    }

    /// Loads once per user-store generation, including a missing row.
    /// The caller must validate the generation after reading the candidate's
    /// unigram deltas and retry if a committed write changed it.
    ///
    /// # Errors
    /// Returns the loader's error unchanged, without caching it.
    pub fn get_or_try_init_in_generation<E>(
        &mut self,
        generation: u64,
        load: impl FnOnce() -> Result<Option<MergedGram>, E>,
    ) -> Result<Option<&MergedGram>, E> {
        let tag = core::num::NonZeroU64::new(generation.wrapping_add(1));
        // At u64::MAX, do not cache: zero remains the unloaded tag and
        // cannot be confused with a loaded miss even when the counter wraps.
        if tag.is_none() || self.user_generation != tag {
            self.loaded = load()?;
            self.user_generation = tag;
        }
        Ok(self.loaded.as_ref())
    }

    /// The merged row just loaded and validated by the caller.
    #[must_use]
    pub fn cached_merged(&self) -> Option<&MergedGram> {
        self.loaded.as_ref()
    }

    /// Retains an encoded system row and its user overlay for this step only.
    /// A loaded miss is reused; a failed load is retried and never cached.
    ///
    /// # Errors
    /// Returns the loader's error unchanged.
    pub fn get_or_try_init_raw<E>(
        &mut self,
        load: impl FnOnce() -> Result<Option<NbestRawGram>, E>,
    ) -> Result<Option<&NbestRawGram>, E> {
        self.get_or_try_init_raw_in_generation(0, load)
    }

    /// Loads the encoded row once per user-store generation. A loaded miss
    /// has a generation tag too, while a failed load is never cached.
    ///
    /// # Errors
    /// Returns the loader's error unchanged.
    pub fn get_or_try_init_raw_in_generation<E>(
        &mut self,
        generation: u64,
        load: impl FnOnce() -> Result<Option<NbestRawGram>, E>,
    ) -> Result<Option<&NbestRawGram>, E> {
        let tag = core::num::NonZeroU64::new(generation.wrapping_add(1));
        if tag.is_none() || self.raw_generation != tag {
            self.raw = load()?;
            self.raw_generation = tag;
        }
        Ok(self.raw.as_ref())
    }

    /// The encoded row just loaded and validated by the caller.
    #[must_use]
    pub fn cached_raw(&self) -> Option<&NbestRawGram> {
        self.raw.as_ref()
    }
}

/// A step-owned raw predecessor row. Normal installed rows remain encoded;
/// only out-of-order records are stably reordered in those same bytes. The
/// user overlay is kept separately, avoiding a decoded system row and a
/// second merged-row copy for each predecessor.
#[derive(Debug)]
pub struct NbestRawGram {
    system: Option<Vec<u8>>,
    user: Vec<(u32, u64)>,
    total: u64,
}

impl NbestRawGram {
    /// Builds a sorted view over one encoded `total:u32, (token:u32,count:u32)*`
    /// system value and the positive-count user overlay. A present empty
    /// system value is a parse error, including when the user side is present.
    ///
    /// # Errors
    /// Returns the frozen schema's length error for malformed system bytes.
    pub fn try_new(
        mut system: Option<Vec<u8>>,
        mut user: Vec<(u32, u64)>,
        user_total: u64,
    ) -> Result<Option<Self>, String> {
        let system_total = if let Some(bytes) = system.as_mut() {
            if bytes.len() < 4 || !(bytes.len() - 4).is_multiple_of(8) {
                return Err(format!("bigram value length {} is not 4 + 8n", bytes.len()));
            }
            let total = u32::from_le_bytes(bytes[..4].try_into().unwrap_or([0; 4]));
            let (records, _) = bytes[4..].as_chunks_mut::<8>();
            if records
                .windows(2)
                .any(|pair| Self::record_token(&pair[0]) > Self::record_token(&pair[1]))
            {
                // The prior decoded-row path accepted arbitrary stored order
                // and kept the first duplicate. Stable sorting of raw records
                // retains that behavior without a decoded copy.
                records.sort_by_key(Self::record_token);
            }
            total
        } else {
            0
        };
        user.retain(|(_, count)| *count > 0);
        if system.is_none() && user.is_empty() && user_total == 0 {
            return Ok(None);
        }
        user.sort_by_key(|(token, _)| *token);
        let mut write = 0;
        for read in 0..user.len() {
            let (token, count) = user[read];
            if write > 0 && user[write - 1].0 == token {
                user[write - 1].1 = Self::wrap_count(user[write - 1].1, count);
            } else {
                user[write] = (token, Self::wrap_count(0, count));
                write += 1;
            }
        }
        user.truncate(write);
        Ok(Some(Self {
            system,
            user,
            total: Self::wrap_count(u64::from(system_total), user_total),
        }))
    }

    fn wrap_count(left: u64, right: u64) -> u64 {
        u64::from((left as u32).wrapping_add(right as u32))
    }

    fn record_token(record: &[u8; 8]) -> u32 {
        u32::from_le_bytes(record[..4].try_into().unwrap_or([0; 4]))
    }

    fn record_count(record: &[u8; 8]) -> u32 {
        u32::from_le_bytes(record[4..].try_into().unwrap_or([0; 4]))
    }

    fn system_records(&self) -> &[[u8; 8]] {
        self.system
            .as_ref()
            .map_or(&[], |bytes| bytes[4..].as_chunks::<8>().0)
    }

    /// The wrapped merged total, even when it is zero for a present row.
    #[must_use]
    pub const fn total(&self) -> u64 {
        self.total
    }

    /// A successor's wrapped count, preserving stored zero-count presence.
    #[must_use]
    pub fn count_if_present(&self, token: u32) -> Option<u64> {
        let records = self.system_records();
        let position = records.partition_point(|record| Self::record_token(record) < token);
        let system = records
            .get(position)
            .filter(|record| Self::record_token(record) == token)
            .map(|record| u64::from(Self::record_count(record)));
        let user = self
            .user
            .binary_search_by_key(&token, |(stored, _)| *stored)
            .ok()
            .map(|index| self.user[index].1);
        match (system, user) {
            (None, None) => None,
            (left, right) => Some(Self::wrap_count(left.unwrap_or(0), right.unwrap_or(0))),
        }
    }

    fn visit_range(&self, lo: u32, hi: u32, mut visit: impl FnMut(u32)) {
        let records = self.system_records();
        let mut position = records.partition_point(|record| Self::record_token(record) < lo);
        while let Some(record) = records.get(position) {
            let token = Self::record_token(record);
            if token > hi {
                break;
            }
            visit(token);
            position += 1;
        }
        let mut position = self.user.partition_point(|(token, _)| *token < lo);
        while let Some(&(token, _)) = self.user.get(position) {
            if token > hi {
                break;
            }
            visit(token);
            position += 1;
        }
    }
}

/// Successors owned by one n-best step, represented as a decoded slice or an
/// encoded system row plus user overlay. `None` at the trait boundary means
/// the model lacks sparse traversal; an empty variant is a loaded row miss.
pub enum NbestSuccessors<'a> {
    /// An already merged sorted row.
    Merged(&'a [(u32, u64)]),
    /// An encoded system row and its separate overlay.
    Raw(&'a NbestRawGram),
}

impl NbestSuccessors<'_> {
    /// Visits successor tokens within an inclusive candidate token range.
    pub fn visit_range(&self, lo: u32, hi: u32, mut visit: impl FnMut(u32)) {
        match self {
            Self::Merged(records) => {
                let mut position = records.partition_point(|(token, _)| *token < lo);
                while let Some(&(token, _)) = records.get(position) {
                    if token > hi {
                        break;
                    }
                    visit(token);
                    position += 1;
                }
            }
            Self::Raw(gram) => gram.visit_range(lo, hi, visit),
        }
    }
}

/// Deterministic language-model scoring seam.
///
/// Implementations do not panic on caller input. They may combine the supplied
/// edge cost with their own cost and report arithmetic or backend failures
/// through [`Result`].
pub trait LanguageModel {
    /// Token representation scored by this model.
    type Token;

    /// Scoring failure reported by this model.
    type Error;

    /// Returns the model cost for `token` after `history` with `edge_cost`
    /// available for deterministic composition.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] when the model backend cannot score the transition.
    fn score(
        &self,
        history: &[Self::Token],
        token: &Self::Token,
        edge_cost: Cost,
    ) -> Result<Cost, Self::Error>;

    /// The model's real unigram frequency of `token`, when the model carries
    /// one — upstream's phrase-item field (`PhraseItem::get_unigram_frequency`:
    /// the corpus count plus the one `gen_unigram` adds to every item, so a
    /// counted phrase is never 0).
    ///
    /// `None` means the model exposes no frequency table at all; the engine
    /// then keeps its pre-frequency behaviour. `Some(0)` is a real table miss
    /// — the phrase has no item — and ranks below every counted phrase.
    ///
    /// Defaulted so the frozen implementors and any third-party model keep
    /// compiling unchanged (`docs/findings/core-trait-seam.md`: the seam grows
    /// by defaulted methods only).
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] when the model backend cannot read the unigram frequency.
    fn unigram_freq(&self, _token: &Self::Token) -> Result<Option<u64>, Self::Error> {
        Ok(None)
    }

    /// The merged `prev → *` successor row, or `None` when the model has no
    /// bigram entry for `prev`.
    ///
    /// This is upstream's Gate 2 — `m_system_bigram->load` plus
    /// `m_user_bigram->load` plus `merge_single_gram` — as ONE call. The
    /// engine invokes it once per candidate guess and indexes the result per
    /// candidate; an implementor that loads per lookup would reintroduce the
    /// complexity the single row exists to avoid.
    ///
    /// Defaulted to `None` so the frozen implementors keep compiling
    /// unchanged (`docs/findings/core-trait-seam.md`: the seam grows by
    /// defaulted methods only). A model answering `None` simply contributes
    /// no bigram term, which is the DYNAMIC_ADJUST-clear behaviour.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] when the model backend cannot read the merged-gram record.
    fn merged_successors(&self, _prev: &Self::Token) -> Result<Option<MergedGram>, Self::Error> {
        Ok(None)
    }

    /// Whether the model carries a real unigram frequency table.
    ///
    /// This is the construction switch: when it returns `true` the engine
    /// collects and ranks candidates by the pinned construction, and when it
    /// returns `false` the pre-frequency behaviour runs. Defaulted to `false`
    /// so the frozen implementors and any third-party model keep compiling
    /// unchanged.
    fn has_real_unigrams(&self) -> bool {
        false
    }

    /// Sum of the default-facade unigram table, when the model carries one
    /// — `FacadePhraseIndex::get_phrase_index_total_freq`, the denominator
    /// of the pinned candidate law.
    ///
    /// Defaulted to `None` so existing implementors compile unchanged.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] when the model backend cannot read the unigram total.
    fn unigram_total(&self) -> Result<Option<u64>, Self::Error> {
        Ok(None)
    }

    /// Addon-facade unigram of `token`, when an addon set is loaded.
    ///
    /// `None` means no addon table; `Some(0)` is a loaded-table miss.
    /// Defaulted so existing implementors compile unchanged
    /// (`docs/findings/phrase-union.md` §6.4).
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] when the addon model backend cannot read the unigram frequency.
    fn addon_unigram_freq(&self, _token: &Self::Token) -> Result<Option<u64>, Self::Error> {
        Ok(None)
    }

    /// Sum of loaded addon unigrams, when an addon set is loaded.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] when the addon model backend cannot read the unigram total.
    fn addon_unigram_total(&self) -> Result<Option<u64>, Self::Error> {
        Ok(None)
    }

    /// The two per-step costs of the n-best sentence trellis (W14).
    ///
    /// Upstream `PhoneticLookup` steps carry `log((λ·b + (1−λ)·u) · p)` when
    /// the previous token has bigram evidence for `token`, and
    /// `log(u · (1−λ) · p)` otherwise (`phonetic_lookup.h:628-676`); the
    /// pronunciation term `p` belongs to the caller (the span match), not the
    /// model. This method reports both branches as fixed-point surprisal so
    /// the engine can pick per step exactly like upstream: the blended cost
    /// when `blended` is `Some`, else `unigram`.
    ///
    /// `None` in either field means the branch's possibility is below
    /// upstream's epsilon floor (`bigram_poss < FLT_EPSILON &&
    /// unigram_poss < DBL_EPSILON` skips the step). A model that answers
    /// `None` overall — the default — carries no n-best cost data, and the
    /// engine's trellis yields no rows for it.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] when the model backend cannot assemble the step costs.
    fn nbest_step_costs(
        &self,
        _prev: &Self::Token,
        _token: &Self::Token,
    ) -> Result<NbestStepCosts, Self::Error> {
        Ok(NbestStepCosts::default())
    }

    /// Step costs with caller-owned reuse of this predecessor's row.
    /// The default preserves existing models' pair-wise implementation.
    /// Callers must use a fresh row for every predecessor and n-best step.
    ///
    /// # Errors
    /// Returns the same backend errors as [`Self::nbest_step_costs`].
    fn nbest_step_costs_in_row(
        &self,
        prev: &Self::Token,
        token: &Self::Token,
        _row: &mut NbestStepRow,
    ) -> Result<NbestStepCosts, Self::Error> {
        self.nbest_step_costs(prev, token)
    }

    /// Sorted successors of one n-best predecessor, owned by this step.
    /// `Some(&[])` is a loaded row miss; `None` asks callers to retain the
    /// ordinary per-candidate scoring path for models without row traversal.
    ///
    /// # Errors
    /// Propagates row-load and parse errors without caching a failed load.
    fn nbest_successors_in_row<'a>(
        &self,
        _prev: &Self::Token,
        _row: &'a mut NbestStepRow,
    ) -> Result<Option<NbestSuccessors<'a>>, Self::Error> {
        Ok(None)
    }

    /// The `gfloat` λ of the install's `table.conf` — the weight the pin's
    /// candidate frequency (`(λ·bigram + (1−λ)·unigram)·2²⁴`,
    /// `pinyin.cpp:1862-1866`, `zhuyin.cpp:1236-1280`) is computed with.
    /// Defaulted to the pinned model's `0.312699`.
    fn amplification_lambda(&self) -> f32 {
        0.312_699
    }
}

/// The branch costs of [`LanguageModel::nbest_step_costs`].
///
/// Both are surprisal on the core fixed-point scale: lower is better, and
/// they are summed along a path.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NbestStepCosts {
    /// `-log2(λ·bigram_poss + (1−λ)·unigram_poss)`: the branch with bigram
    /// evidence. `None` when the blend's possibility is below the floor.
    pub blended: Option<Cost>,
    /// `-log2((1−λ) · unigram_poss)`: the no-evidence branch. `None` when
    /// the unigram possibility is below the floor (count 0 or no table).
    pub unigram: Option<Cost>,
}

impl NbestStepCosts {
    /// The cost upstream charges for this step: the blended branch when it
    /// exists, else the unigram branch. `None` skips the step entirely.
    #[must_use]
    pub const fn step(&self) -> Option<Cost> {
        match (self.blended, self.unigram) {
            (Some(blended), _) => Some(blended),
            (None, Some(unigram)) => Some(unigram),
            (None, None) => None,
        }
    }
}

impl<L: LanguageModel + ?Sized> LanguageModel for &L {
    type Token = L::Token;
    type Error = L::Error;

    fn score(
        &self,
        history: &[Self::Token],
        token: &Self::Token,
        edge_cost: Cost,
    ) -> Result<Cost, Self::Error> {
        (**self).score(history, token, edge_cost)
    }

    fn unigram_freq(&self, token: &Self::Token) -> Result<Option<u64>, Self::Error> {
        (**self).unigram_freq(token)
    }

    fn has_real_unigrams(&self) -> bool {
        (**self).has_real_unigrams()
    }

    fn unigram_total(&self) -> Result<Option<u64>, Self::Error> {
        (**self).unigram_total()
    }

    fn addon_unigram_freq(&self, token: &Self::Token) -> Result<Option<u64>, Self::Error> {
        (**self).addon_unigram_freq(token)
    }

    fn addon_unigram_total(&self) -> Result<Option<u64>, Self::Error> {
        (**self).addon_unigram_total()
    }

    fn nbest_step_costs(
        &self,
        prev: &Self::Token,
        token: &Self::Token,
    ) -> Result<NbestStepCosts, Self::Error> {
        (**self).nbest_step_costs(prev, token)
    }

    fn nbest_step_costs_in_row(
        &self,
        prev: &Self::Token,
        token: &Self::Token,
        row: &mut NbestStepRow,
    ) -> Result<NbestStepCosts, Self::Error> {
        (**self).nbest_step_costs_in_row(prev, token, row)
    }

    fn nbest_successors_in_row<'a>(
        &self,
        prev: &Self::Token,
        row: &'a mut NbestStepRow,
    ) -> Result<Option<NbestSuccessors<'a>>, Self::Error> {
        (**self).nbest_successors_in_row(prev, row)
    }

    fn amplification_lambda(&self) -> f32 {
        (**self).amplification_lambda()
    }
    fn merged_successors(&self, prev: &Self::Token) -> Result<Option<MergedGram>, Self::Error> {
        (**self).merged_successors(prev)
    }
}

/// Total byte-input parsing seam.
pub trait InputParser {
    /// One owned parse alternative.
    type Parse;

    /// Parser backend or resource-limit failure (for example too many alternatives).
    ///
    /// Malformed, junk, and partial caller input are represented in parse
    /// outputs rather than as this error.
    type Error;

    /// Returns every valid segmentation of `input` in frozen path-set order.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] when the input cannot be parsed under the scheme.
    fn parse(&self, input: &[u8]) -> Result<Vec<Self::Parse>, Self::Error>;
}
