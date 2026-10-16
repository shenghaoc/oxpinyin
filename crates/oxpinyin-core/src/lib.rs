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
pub mod single_gram;
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
    /// What the user side and `add_unigram_frequency` added to the
    /// facade's unigram total. The facade total is a `guint32`, so the
    /// model adds this to it wrapping.
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

    /// One `FacadeChewingTable2::search` call of the sentence trellis:
    /// appends this lookup's `PhraseIndexRange`s to `ranges`
    /// (`chewing_large_table2.h:91-133` at the pin `074a2219`), the system
    /// table's call before the user table's
    /// (`facade_chewing_table2.h:109-129`). `tones` is as for
    /// [`Self::trellis_records`].
    ///
    /// The ranges carry tokens only, as the pin's `convert` does: a step
    /// prices the tokens it meets through [`Self::trellis_item`], and the
    /// text of a result is read through [`Self::trellis_text`] when it is
    /// emitted. Every ranged token holds a `Some(())` slot.
    ///
    /// The default applies [`PhraseIndexRanges::convert`] to
    /// [`Self::trellis_records`]' tokens as one call, so dictionaries
    /// without a stored index need no change.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] when the dictionary backend cannot serve the lookup.
    fn trellis_ranges(
        &self,
        syllables: &[Self::Syllable],
        tones: &[u8],
        ranges: &mut PhraseIndexRanges<()>,
    ) -> Result<(), Self::Error>
    where
        Self::Entry: PhraseIndexToken,
    {
        let entries = self.trellis_records(syllables, tones)?;
        ranges.convert(
            entries
                .iter()
                .map(|entry| (entry.phrase_index_token(), Some(()))),
        );
        Ok(())
    }

    /// [`Self::trellis_ranges`] and [`Self::phrase_prefix_exists`] for the
    /// same key path, answered together: the pin's one
    /// `ChewingLargeTable2::search` call gives the ranges and
    /// `SEARCH_CONTINUED` from a single DBM get
    /// (`chewing_large_table2_bdb.cpp:165-192` at `074a2219`). Returns
    /// whether the prefix is continued.
    ///
    /// The default is the two calls; a dictionary that can serve both from
    /// one read overrides it and must answer exactly as the pair does.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] when the dictionary backend cannot serve the lookup.
    fn trellis_ranges_continued(
        &self,
        syllables: &[Self::Syllable],
        tones: &[u8],
        ranges: &mut PhraseIndexRanges<()>,
    ) -> Result<bool, Self::Error>
    where
        Self::Entry: PhraseIndexToken,
    {
        self.trellis_ranges(syllables, tones, ranges)?;
        self.phrase_prefix_exists(syllables)
    }

    /// What one trellis step prices of `token`, a token
    /// [`Self::trellis_ranges`] ranged over `syllables` and `tones`
    /// (`get_phrase_item` and `compute_pronunciation_possibility`,
    /// `phonetic_lookup.h:646-684`); `None` when the token has no
    /// readable item there.
    ///
    /// The default reads the last of [`Self::trellis_records`]' entries
    /// for the token.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] when the dictionary backend cannot serve the lookup.
    fn trellis_item(
        &self,
        syllables: &[Self::Syllable],
        tones: &[u8],
        token: u32,
    ) -> Result<Option<TrellisItem>, Self::Error>
    where
        Self::Entry: PhraseIndexToken,
    {
        Ok(self
            .trellis_records(syllables, tones)?
            .iter()
            .rev()
            .find(|entry| entry.phrase_index_token() == token)
            .map(PhraseIndexToken::phrase_index_item))
    }

    /// The phrase text [`Self::trellis_item`] priced, read when a result
    /// is emitted (`convert_to_utf8`); `None` where that item is `None`.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] when the dictionary backend cannot serve the lookup.
    fn trellis_text(
        &self,
        syllables: &[Self::Syllable],
        tones: &[u8],
        token: u32,
    ) -> Result<Option<String>, Self::Error>
    where
        Self::Entry: PhraseIndexToken,
    {
        Ok(self
            .trellis_records(syllables, tones)?
            .iter()
            .rev()
            .find(|entry| entry.phrase_index_token() == token)
            .map(PhraseIndexToken::phrase_index_text))
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

    /// The stored pronunciations of the phrase item behind `token` —
    /// `FacadePhraseIndex::get_phrase_item` then `get_nth_pronunciation`
    /// (`phrase_index.h` at the pin `074a2219`): each pronunciation's keys,
    /// tones included, and its `guint32` frequency, in stored order. This is
    /// what `ForwardPhoneticConstraints::validate_constraint` revalidates a
    /// forcing against (`phonetic_lookup.cpp:120-165`): it searches no
    /// table, so it has no expansion of incomplete keys to bound.
    ///
    /// Defaulted to [`PhraseItemRead::Unsupported`] so existing implementors
    /// compile unchanged (`docs/findings/core-trait-seam.md`: the seam grows
    /// by defaulted methods only); a dictionary that keeps no phrase items is
    /// revalidated through [`Self::lookup`] instead.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] when the backend cannot read the item.
    fn phrase_item_pronunciations(
        &self,
        _token: PhraseToken,
    ) -> Result<PhraseItemRead, Self::Error> {
        Ok(PhraseItemRead::Unsupported)
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

    fn trellis_ranges(
        &self,
        syllables: &[Self::Syllable],
        tones: &[u8],
        ranges: &mut PhraseIndexRanges<()>,
    ) -> Result<(), Self::Error>
    where
        Self::Entry: PhraseIndexToken,
    {
        (**self).trellis_ranges(syllables, tones, ranges)
    }

    fn trellis_ranges_continued(
        &self,
        syllables: &[Self::Syllable],
        tones: &[u8],
        ranges: &mut PhraseIndexRanges<()>,
    ) -> Result<bool, Self::Error>
    where
        Self::Entry: PhraseIndexToken,
    {
        (**self).trellis_ranges_continued(syllables, tones, ranges)
    }

    fn trellis_item(
        &self,
        syllables: &[Self::Syllable],
        tones: &[u8],
        token: u32,
    ) -> Result<Option<TrellisItem>, Self::Error>
    where
        Self::Entry: PhraseIndexToken,
    {
        (**self).trellis_item(syllables, tones, token)
    }

    fn trellis_text(
        &self,
        syllables: &[Self::Syllable],
        tones: &[u8],
        token: u32,
    ) -> Result<Option<String>, Self::Error>
    where
        Self::Entry: PhraseIndexToken,
    {
        (**self).trellis_text(syllables, tones, token)
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

    fn phrase_item_pronunciations(
        &self,
        token: PhraseToken,
    ) -> Result<PhraseItemRead, Self::Error> {
        (**self).phrase_item_pronunciations(token)
    }
}

/// A phrase item's stored pronunciations in stored order: each one's keys,
/// tones included, and its `guint32` frequency.
pub type PronunciationRows = Vec<(Vec<ChewingKey>, u32)>;

/// What [`Dictionary::phrase_item_pronunciations`] found behind a token.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PhraseItemRead {
    /// The dictionary keeps no phrase items to read.
    Unsupported,
    /// The token has no readable item: its library is not loaded or hidden,
    /// or the slot is empty. The pin's `get_phrase_item` then fails and
    /// leaves the caller's `PhraseItem` untouched (`phrase_index.h:649-650`,
    /// `phrase_index.cpp:189-193`), so the caller keeps whatever item it
    /// last read.
    NoItem,
    /// The item's pronunciations.
    Item(PronunciationRows),
}

/// `PHRASE_INDEX_LIBRARY_COUNT` (`novel_types.h` at the pin `074a2219`):
/// one range array per library nibble.
pub const PHRASE_INDEX_LIBRARY_COUNT: usize = 16;

/// `PHRASE_INDEX_LIBRARY_INDEX`: the library nibble of a phrase token.
#[must_use]
pub const fn phrase_index_library(token: u32) -> usize {
    ((token >> 24) & 0x0f) as usize
}

/// What one trellis step prices of a ranged token: the phrase's length in
/// characters and its pronunciation possibility `(matched, total)`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TrellisItem {
    /// The phrase's length in characters.
    pub chars: u32,
    /// The pronunciation possibility, where the item counts one.
    pub pronunciation: Option<(u64, u64)>,
}

/// The phrase token a trellis entry stands for, so
/// [`Dictionary::trellis_ranges`]' default can file it into a range, and
/// what [`Dictionary::trellis_item`]'s default reads of it.
pub trait PhraseIndexToken {
    /// The entry's raw phrase token.
    fn phrase_index_token(&self) -> u32;

    /// The entry's [`TrellisItem`].
    fn phrase_index_item(&self) -> TrellisItem;

    /// The entry's phrase text.
    fn phrase_index_text(&self) -> String;
}

impl PhraseIndexToken for PhraseEntry {
    fn phrase_index_token(&self) -> u32 {
        self.token().value()
    }

    fn phrase_index_item(&self) -> TrellisItem {
        TrellisItem {
            chars: self.text().chars().count().try_into().unwrap_or(u32::MAX),
            pronunciation: self.pronunciation_possibility(),
        }
    }

    fn phrase_index_text(&self) -> String {
        self.text().to_owned()
    }
}

/// One `PhraseIndexRange`: the consecutive tokens `first..=last` of one
/// library that one table search met back to back.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PhraseIndexRange {
    first: u32,
    last: u32,
    /// Index of `first`'s slot; token `t` sits at `slot + (t - first)`.
    /// `u32` keeps the range at 12 bytes; a span never holds 2^32 items.
    slot: u32,
}

impl PhraseIndexRange {
    /// The first token of the range.
    #[must_use]
    pub const fn first(&self) -> u32 {
        self.first
    }

    /// The last token of the range (the pin's `m_range_end - 1`).
    #[must_use]
    pub const fn last(&self) -> u32 {
        self.last
    }
}

/// The pin's `PhraseIndexRanges`: one array of [`PhraseIndexRange`]s per
/// library, each in append order, plus the entry every ranged token
/// resolved to. A token whose library is loaded but whose entry could not
/// be resolved keeps its place in its range with no entry, as the pin's
/// range keeps it until `get_phrase_item` fails for it.
#[derive(Clone, Debug)]
pub struct PhraseIndexRanges<E> {
    libraries: [Vec<PhraseIndexRange>; PHRASE_INDEX_LIBRARY_COUNT],
    /// One entry slot per converted item, in item order; a range's tokens
    /// map to consecutive slots from its `slot`.
    slots: Vec<Option<E>>,
}

impl<E> Default for PhraseIndexRanges<E> {
    fn default() -> Self {
        Self {
            libraries: Default::default(),
            slots: Vec::new(),
        }
    }
}

impl<E> PhraseIndexRanges<E> {
    /// Empty ranges.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Empties every array, keeping capacity.
    pub fn clear(&mut self) {
        for ranges in &mut self.libraries {
            ranges.clear();
        }
        self.slots.clear();
    }

    /// `ChewingTableEntry::convert` (`chewing_large_table2.h:91-133` at the
    /// pin `074a2219`) over one search call's matched items, in table-item
    /// order: a token one past the open range's end in the same library
    /// extends it; any other token closes it into its library's array and
    /// opens a new one. The last range is appended when the call ends, so
    /// no range spans two calls.
    ///
    /// The pin skips an item whose library is not loaded without closing
    /// the open range; callers drop those items before this call.
    pub fn convert(&mut self, items: impl IntoIterator<Item = (u32, Option<E>)>) {
        let mut cursor: Option<PhraseIndexRange> = None;
        for (token, entry) in items {
            let slot = u32::try_from(self.slots.len()).unwrap_or(u32::MAX);
            self.slots.push(entry);
            match cursor.as_mut() {
                Some(open)
                    if open.last.checked_add(1) == Some(token)
                        && phrase_index_library(open.first) == phrase_index_library(token) =>
                {
                    open.last = token;
                }
                _ => {
                    if let Some(closed) = cursor.take() {
                        self.push(closed);
                    }
                    cursor = Some(PhraseIndexRange {
                        first: token,
                        last: token,
                        slot,
                    });
                }
            }
        }
        if let Some(closed) = cursor {
            self.push(closed);
        }
    }

    fn push(&mut self, range: PhraseIndexRange) {
        if let Some(ranges) = self.libraries.get_mut(phrase_index_library(range.first)) {
            ranges.push(range);
        }
    }

    /// Library `library`'s ranges in append order; empty past the last library.
    #[must_use]
    pub fn ranges(&self, library: usize) -> &[PhraseIndexRange] {
        self.libraries.get(library).map_or(&[], Vec::as_slice)
    }

    /// Every range, libraries `0..16` then append order — the n-best
    /// walk's order (`phonetic_lookup.h:560-572`, `:615-632`).
    pub fn iter(&self) -> impl Iterator<Item = &PhraseIndexRange> {
        self.libraries.iter().flatten()
    }

    /// The entry `token` resolved to inside `range`.
    #[must_use]
    pub fn entry(&self, range: &PhraseIndexRange, token: u32) -> Option<&E> {
        if token < range.first || token > range.last {
            return None;
        }
        let offset = usize::try_from(token - range.first).ok()?;
        let first = usize::try_from(range.slot).ok()?;
        self.slots.get(first.checked_add(offset)?)?.as_ref()
    }

    /// `range`'s resolved entries, ascending by token.
    pub fn entries<'a>(&'a self, range: &PhraseIndexRange) -> impl Iterator<Item = &'a E> + 'a {
        let count =
            usize::try_from(range.last - range.first).map_or(0, |span| span.saturating_add(1));
        self.slots
            .iter()
            .skip(usize::try_from(range.slot).unwrap_or(usize::MAX))
            .take(count)
            .filter_map(Option::as_ref)
    }

    /// Every filled slot with its token, in the walk order of [`Self::iter`].
    pub fn walk_tokens(&self) -> impl Iterator<Item = (u32, &E)> {
        self.iter().flat_map(move |range| {
            (range.first..=range.last)
                .filter_map(move |token| self.entry(range, token).map(|entry| (token, entry)))
        })
    }

    /// Every resolved entry, in the walk order of [`Self::iter`].
    pub fn walk(&self) -> impl Iterator<Item = &E> {
        self.iter().flat_map(|range| self.entries(range))
    }

    /// The resolved entries in the walk order of [`Self::iter`] —
    /// libraries, then ranges in append order, then tokens — moved out: a
    /// flat [`Dictionary::trellis_records`] answer.
    #[must_use]
    pub fn into_walk_entries(self) -> Vec<E> {
        let Self {
            libraries,
            mut slots,
        } = self;
        let mut out = Vec::with_capacity(slots.len());
        for range in libraries.iter().flatten() {
            let first = usize::try_from(range.slot).unwrap_or(usize::MAX);
            let count =
                usize::try_from(range.last - range.first).map_or(0, |span| span.saturating_add(1));
            out.extend(
                slots
                    .iter_mut()
                    .skip(first)
                    .take(count)
                    .filter_map(Option::take),
            );
        }
        out
    }

    /// Appends `other`'s ranges after this one's, library by library, with
    /// its entries mapped through `map`; `other` is left empty. Per-library
    /// append order is kept, which is all the walk observes.
    pub fn append_from<F>(
        &mut self,
        other: &mut PhraseIndexRanges<F>,
        mut map: impl FnMut(F) -> E,
    ) {
        let base = u32::try_from(self.slots.len()).unwrap_or(u32::MAX);
        self.slots
            .extend(other.slots.drain(..).map(|entry| entry.map(&mut map)));
        for (ours, theirs) in self.libraries.iter_mut().zip(other.libraries.iter_mut()) {
            ours.extend(theirs.drain(..).map(|range| PhraseIndexRange {
                slot: range.slot.saturating_add(base),
                ..range
            }));
        }
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

    /// Checks one training step before any step is observed: the store
    /// reads the gram `token`'s step will load after `history` and refuses
    /// what the pin dies on. The training walk calls it per step, in step
    /// order, between the step's constraint check and its span checks
    /// (`phonetic_lookup.h:858-933` at the pin `074a2219`), so the first
    /// failing check in step order wins. The default accepts every step.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] when the step would make the pin abort or
    /// read past a value.
    fn check_step(
        &mut self,
        _history: &[Self::Token],
        _token: &Self::Token,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

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

/// One `prev → *` bigram row with the user overlay already merged: the
/// pin's `m_merged_single_gram`, the chunk `merge_single_gram` builds
/// (`ngram.cpp:277-352` at `074a2219`).
///
/// The row is the merged chunk itself, read with the ported
/// [`single_gram`] operations: `get_total_freq`, `get_freq` and `search`
/// walk it from `lower_bound` exactly as the pin does, so an unsorted or
/// duplicated input row answers as it does there. Nothing is sorted,
/// deduplicated or filtered. A side that is absent leaves the other
/// side's own buffer in place, which is the pin's byte copy without the
/// copy.
///
/// Loaded once per predecessor and read per candidate, which is the pin's
/// structure: `pinyin_guess_candidates` merges one gram outside its
/// candidate loop and `_compute_frequency_of_items` calls
/// `merged_gram->get_freq(token, ...)` inside it.
///
/// A merge whose input the pin cannot read keeps no chunk and answers
/// every read with its [`GramFault`]; the caller refuses the call.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MergedGram {
    chunk: Vec<u8>,
    fault: Option<GramFault>,
}

pub use single_gram::GramFault;

impl MergedGram {
    /// A row from its total and `(token, count)` items, kept in the order
    /// given; each field is stored as the pin's `guint32`.
    #[must_use]
    pub fn new(total: u64, records: Vec<(u32, u64)>) -> Self {
        let mut chunk = Vec::with_capacity(
            single_gram::TOTAL_LEN + records.len().saturating_mul(single_gram::ITEM_LEN),
        );
        chunk.extend_from_slice(&(total as u32).to_le_bytes());
        for (token, count) in records {
            chunk.extend_from_slice(&token.to_le_bytes());
            chunk.extend_from_slice(&(count as u32).to_le_bytes());
        }
        Self { chunk, fault: None }
    }

    /// `merge_single_gram(merged, system, user)`: `None` when both grams
    /// are absent (the pin answers `false`), the present side's own bytes
    /// when the other is absent, and otherwise the two-pointer merge.
    ///
    /// Both present, the pin reads both totals first (`ngram.cpp:303-304`)
    /// and asserts on a value shorter than one ([`GramFault::Short`],
    /// register row 85); it then consumes every item of both values, so a
    /// partial tail on either is read past ([`GramFault::OverRead`], row
    /// 98).
    #[must_use]
    pub fn merge(system: Option<Vec<u8>>, user: Option<Vec<u8>>) -> Option<Self> {
        match (system, user) {
            (None, None) => None,
            (Some(chunk), None) | (None, Some(chunk)) => Some(Self { chunk, fault: None }),
            (Some(system), Some(user)) => {
                let short = |gram: &[u8]| single_gram::get_total_freq(gram).is_none();
                let fault = if short(&system) || short(&user) {
                    Some(GramFault::Short)
                } else if single_gram::has_partial_tail(&system)
                    || single_gram::has_partial_tail(&user)
                {
                    Some(GramFault::OverRead)
                } else {
                    None
                };
                if fault.is_some() {
                    return Some(Self {
                        chunk: Vec::new(),
                        fault,
                    });
                }
                let mut chunk = Vec::with_capacity(
                    system.len().saturating_add(user.len()) - single_gram::TOTAL_LEN,
                );
                single_gram::merge_single_gram(&mut chunk, Some(&system), Some(&user));
                Some(Self { chunk, fault: None })
            }
        }
    }

    /// The fault the merge met, if any.
    #[must_use]
    pub const fn fault(&self) -> Option<GramFault> {
        self.fault
    }

    /// `get_total_freq`; [`GramFault::Short`] where the pin asserts.
    ///
    /// # Errors
    /// The merge's fault, or [`GramFault::Short`] for a value shorter than
    /// its total.
    pub fn total(&self) -> Result<u32, GramFault> {
        if let Some(fault) = self.fault {
            return Err(fault);
        }
        single_gram::get_total_freq(&self.chunk).ok_or(GramFault::Short)
    }

    /// `get_freq(token)`: the first match walking from `lower_bound`, or
    /// `None` on a miss.
    ///
    /// # Errors
    /// The merge's fault, or [`GramFault::OverRead`] where the pin's walk
    /// leaves the value (see [`single_gram::checked_get_freq`]).
    pub fn get_freq(&self, token: u32) -> Result<Option<u32>, GramFault> {
        if let Some(fault) = self.fault {
            return Err(fault);
        }
        single_gram::checked_get_freq(&self.chunk, token)
    }

    /// `search(range)` over `first..=last` into `out`: every item from
    /// `lower_bound(first)` until the first at or past `last + 1`,
    /// duplicates included. `last + 1` wraps as the pin's `guint32`
    /// `m_range_end = token + 1` does (`chewing_large_table2.h:115`, `:120`
    /// at the pin `074a2219`), so a range ending at `u32::MAX` stops at
    /// its first item and yields nothing, as `m_token >= m_range_end`
    /// (`ngram.cpp:168`) does over an end of zero.
    ///
    /// # Errors
    /// The merge's fault, else what [`single_gram::checked_search`] meets.
    pub fn search_into(
        &self,
        first: u32,
        last: u32,
        out: &mut Vec<(u32, u32)>,
    ) -> Result<(), GramFault> {
        if let Some(fault) = self.fault {
            return Err(fault);
        }
        single_gram::checked_search(&self.chunk, first, last.wrapping_add(1), out)
    }

    /// The merged chunk's bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.chunk
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
    generation: Option<core::num::NonZeroU64>,
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
        if tag.is_none() || self.generation != tag {
            self.loaded = load()?;
            self.generation = tag;
        }
        Ok(self.loaded.as_ref())
    }

    /// The merged row just loaded and validated by the caller.
    #[must_use]
    pub fn cached_merged(&self) -> Option<&MergedGram> {
        self.loaded.as_ref()
    }
}

/// One n-best predecessor's successors, owned by its step. `None` at the
/// trait boundary means the model lacks row traversal.
pub enum NbestSuccessors<'a> {
    /// The merged row.
    Row(&'a MergedGram),
    /// Neither gram is present: `merge_single_gram` answered `false` and
    /// the predecessor takes no bigram step.
    Miss,
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

    /// Step costs for one item the caller read from this predecessor's
    /// merged row: `item` is the item's `(freq, total)`, the pin's
    /// `bigram_poss = freq / total` (`phonetic_lookup.h:606-608`, `:629`;
    /// `phrase_lookup.cpp:288-292`), and `None` asks for the unigram
    /// branch alone. Duplicated items each come here with their own
    /// frequency. The default preserves existing models' pair-wise
    /// implementation and ignores `item`.
    ///
    /// # Errors
    /// Returns the same backend errors as [`Self::nbest_step_costs`].
    fn nbest_item_costs_in_row(
        &self,
        prev: &Self::Token,
        token: &Self::Token,
        _item: Option<(u32, u32)>,
        row: &mut NbestStepRow,
    ) -> Result<NbestStepCosts, Self::Error> {
        self.nbest_step_costs_in_row(prev, token, row)
    }

    /// The merged row of one n-best predecessor, owned by this step.
    /// [`NbestSuccessors::Miss`] is a loaded miss of both grams; `None`
    /// asks callers to retain the ordinary per-candidate scoring path for
    /// models without row traversal.
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

    fn nbest_item_costs_in_row(
        &self,
        prev: &Self::Token,
        token: &Self::Token,
        item: Option<(u32, u32)>,
        row: &mut NbestStepRow,
    ) -> Result<NbestStepCosts, Self::Error> {
        (**self).nbest_item_costs_in_row(prev, token, item, row)
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

#[cfg(test)]
mod tests {
    use super::{PhraseIndexRanges, phrase_index_library};

    fn spans(ranges: &PhraseIndexRanges<u32>, library: usize) -> Vec<(u32, u32)> {
        ranges
            .ranges(library)
            .iter()
            .map(|range| (range.first(), range.last()))
            .collect()
    }

    fn call(tokens: &[u32]) -> Vec<(u32, Option<u32>)> {
        tokens.iter().map(|&token| (token, Some(token))).collect()
    }

    /// `convert` merges a token one past the open range and opens a new
    /// range otherwise — including for a repeated or descending token.
    #[test]
    fn convert_merges_consecutive_runs_in_item_order() {
        let mut ranges = PhraseIndexRanges::new();
        ranges.convert(call(&[5, 6, 7, 9, 8, 8]));
        assert_eq!(spans(&ranges, 0), [(5, 7), (9, 9), (8, 8), (8, 8)]);
        let walked: Vec<u32> = ranges.walk().copied().collect();
        assert_eq!(walked, [5, 6, 7, 9, 8, 8]);
    }

    /// A run never crosses a library nibble, and each range lands in its
    /// own library's array; the walk visits libraries in ascending order.
    #[test]
    fn convert_splits_runs_at_library_boundaries() {
        let top = 0x00ff_ffff;
        let mut ranges = PhraseIndexRanges::new();
        ranges.convert(call(&[0x0200_0001, top, top + 1, 0x0200_0002]));
        assert_eq!(phrase_index_library(top + 1), 1);
        assert_eq!(spans(&ranges, 0), [(top, top)]);
        assert_eq!(spans(&ranges, 1), [(top + 1, top + 1)]);
        assert_eq!(
            spans(&ranges, 2),
            [(0x0200_0001, 0x0200_0001), (0x0200_0002, 0x0200_0002)]
        );
        let walked: Vec<u32> = ranges.walk().copied().collect();
        assert_eq!(walked, [top, top + 1, 0x0200_0001, 0x0200_0002]);
    }

    /// Each call closes its last range: `[5,6]` and `[7]` from two calls
    /// stay two ranges, in append order.
    #[test]
    fn convert_builds_no_run_across_calls() {
        let mut ranges = PhraseIndexRanges::new();
        ranges.convert(call(&[5, 6]));
        ranges.convert(call(&[7, 4]));
        assert_eq!(spans(&ranges, 0), [(5, 6), (7, 7), (4, 4)]);
        let mut merged = PhraseIndexRanges::new();
        merged.convert(call(&[1]));
        merged.append_from(&mut ranges, |token| token * 10);
        assert_eq!(spans(&merged, 0), [(1, 1), (5, 6), (7, 7), (4, 4)]);
        let walked: Vec<u32> = merged.walk().copied().collect();
        assert_eq!(walked, [1, 50, 60, 70, 40]);
        assert!(ranges.iter().next().is_none());
    }

    /// `trellis_records`' contract is the walk order, not item order: a
    /// row that interleaves libraries comes back library by library, each
    /// library's ranges in append order.
    #[test]
    fn walk_entries_follow_libraries_then_ranges_on_an_interleaved_row() {
        let mut ranges = PhraseIndexRanges::new();
        ranges.convert(call(&[0x0200_0005, 0x0100_0007, 0x0200_0006, 0x0100_0003]));
        ranges.convert(call(&[0x0200_0001, 0x0100_0008]));
        assert_eq!(
            ranges.into_walk_entries(),
            [
                0x0100_0007,
                0x0100_0003,
                0x0100_0008,
                0x0200_0005,
                0x0200_0006,
                0x0200_0001,
            ]
        );
    }

    /// An unresolved item keeps its token in the range, so `entry` finds
    /// its neighbours at the right offsets.
    #[test]
    fn unresolved_items_keep_their_place_in_the_range() {
        let mut ranges = PhraseIndexRanges::new();
        ranges.convert([(3, Some(30)), (4, None), (5, Some(50))]);
        let [range] = ranges.ranges(0) else {
            panic!("one range: {:?}", ranges.ranges(0));
        };
        assert_eq!((range.first(), range.last()), (3, 5));
        assert_eq!(ranges.entry(range, 3), Some(&30));
        assert_eq!(ranges.entry(range, 4), None);
        assert_eq!(ranges.entry(range, 5), Some(&50));
        assert_eq!(ranges.entry(range, 6), None);
        assert_eq!(ranges.into_walk_entries(), [30, 50]);
    }
}
