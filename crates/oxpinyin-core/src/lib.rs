//! Pure pinyin and zhuyin algorithms: parsing, `SegmentGraph`, chewing-key
//! rendering and model traits. No I/O, platform dependencies or
//! unsafe code. Both C ABIs use these same keys and tables, as upstream does
//! in `src/Makefile.am:108-125` at 074a2219.
#![forbid(unsafe_code)]
#![warn(missing_docs)]
// Constitution §4: keep these non-test denials. A permitted site needs the
// narrowest justified #[expect], citing the pin's abort site or an invariant
// making failure unreachable; see docs/rust-conventions.md.
// An expectation proves lint coverage, not correspondence with the pin.
// #[cfg(test)] modules are exempt; tests/, benches/ and examples/
// are separate crates.
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::panic))]
#![cfg_attr(not(test), deny(clippy::panic_in_result_fn))]
#![cfg_attr(not(test), deny(clippy::unreachable))]

// The fixture doubles name this crate by its package name.
#[cfg(test)]
extern crate self as oxpinyin_core;

mod chewing_key;
mod chewing_key_data;
pub mod direct;
pub mod engine;
#[cfg(test)]
#[path = "../tests/common/fixture_doubles.rs"]
mod fixture_doubles;
mod full_pinyin_index;
pub mod graph;
pub mod marshal;
pub mod options;
mod parser;
mod scheme;
pub mod scoring;
pub mod sentence_facts;
pub mod single_gram;
mod static_strings;
#[cfg(feature = "tools")]
pub mod tool;
pub mod ucs4;
mod string_tables {
    use crate::options::table_row::{
        ZHUYIN_CORRECT_ETEN26, ZHUYIN_CORRECT_HSU, ZHUYIN_CORRECT_SHUFFLE, ZHUYIN_INCOMPLETE,
    };
    include!(concat!(env!("OUT_DIR"), "/string_tables.rs"));
}
mod syllables;
mod vocab;
mod zhuyin_map;

pub use chewing_key::{CHEWING_ZERO_TONE, ChewingKey, KeyText};
pub use full_pinyin_index::{
    FullPinyinIndex, FullPinyinIndexKey, FullPinyinIndexParse, LUOMA_PINYIN_COUNT,
    SECONDARY_ZHUYIN_COUNT, luoma_pinyin_index, parse_full_pinyin_index_with_options,
    secondary_zhuyin_index,
};
pub use options::OptionBits;
pub use parser::{Completeness, FullPinyinParser};
pub use scheme::{
    DoublePinyinKey, DoublePinyinParse, DoublePinyinParser, DoublePinyinScheme, FullPinyinScheme,
    ZhuyinKey, ZhuyinParse, ZhuyinParser, ZhuyinScheme, parse_zhuyin_direct,
};
pub use syllables::{
    FULL_PINYIN_SYLLABLE_COUNT, INCOMPLETE_PINYIN_KEY_COUNT, INCOMPLETE_PINYIN_KEYS,
    MAX_SYLLABLE_LEN, OPTION_ONLY_COMPLETE_SYLLABLE_COUNT, OPTION_ONLY_COMPLETE_SYLLABLES,
    ZHUYIN_ONLY_COMPLETE_SYLLABLE_COUNT, ZHUYIN_ONLY_COMPLETE_SYLLABLES,
    canonical_complete_syllable, full_pinyin_syllable, full_pinyin_syllables,
    option_alias_canonical,
};
pub use ucs4::{OutwardWords, PhraseText, PhraseTextConcat};
pub use vocab::{
    PhraseEntry, PhraseToken, SYLLABLE_KEY_COUNT, SyllableKey, fuzzy_chewing_alternatives,
};
pub(crate) use vocab::{fuzzy_chewing_finals, fuzzy_chewing_initials};

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
    /// The live `guint32` unigram field of the item the pin reads for the
    /// token (`get_phrase_item`, then `PhraseItem::get_unigram_frequency`):
    /// a system item the user side holds, or a `USER_FILE` library's user
    /// item. `Some` is the item's frequency, zero included: the model takes
    /// it instead of the library's stored value plus `unigram_delta`.
    /// A `USER_FILE` token with `None` has no item, and the model skips it.
    pub unigram_live: Option<u32>,
}

impl UserCountDelta {
    /// No user data: the additive merge is a no-op.
    pub const ZERO: Self = Self {
        bigram_count: 0,
        bigram_total: 0,
        unigram_delta: 0,
        unigram_total_delta: 0,
        unigram_live: None,
    };
}

/// Unreadable rows from one candidate search.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct UnreadableRows {
    /// Number of unreadable rows encountered.
    pub count: usize,
    /// First unreadable token, when the search retained its identity.
    pub first: Option<PhraseToken>,
}

impl UnreadableRows {
    /// Adds a token without allocating storage for further failures.
    pub fn push(&mut self, token: PhraseToken) {
        if self.count == 0 {
            self.first = Some(token);
        }
        self.count += 1;
    }
}

/// Read-only lookup seam for dictionaries.
///
/// Implementations return entries in stable order, use an empty vector for a
/// successful lookup with no entries, and do not panic on caller input.
pub trait Dictionary {
    /// Syllable representation accepted by this dictionary.
    type Syllable;

    /// Lookup failure reported by this dictionary.
    type Error;

    /// What this dictionary keeps about one trellis key path while one
    /// n-best call prices the tokens ranged on it
    /// ([`Self::trellis_item_on_path`]). A dictionary with nothing to keep
    /// uses `()`.
    type PathMemo: Default;

    /// Looks up entries matching `syllables`.
    ///
    /// `syllables` may mix complete and initial-only keys, and an
    /// implementation answers such a query directly, as the pin's
    /// incomplete-key index does. `ChewingLargeTable2::search`
    /// (`chewing_large_table2.cpp:161-173` at 074a2219) looks the query up
    /// under `compute_incomplete_chewing_index` (`pinyin_phrase3.h:170-178`),
    /// which projects each position to its initial. The record comparison
    /// still checks each complete syllable with `pinyin_compare_with_tones`
    /// (`chewing_large_table2.h:102-104`; [`ChewingKey::matches_with_tones`]).
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] when the dictionary backend cannot serve the lookup.
    fn lookup(&self, syllables: &[Self::Syllable]) -> Result<Vec<PhraseEntry>, Self::Error>;

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
    ) -> Result<Vec<PhraseEntry>, Self::Error> {
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
    ) -> Result<(), Self::Error> {
        let entries = self.trellis_records(syllables, tones)?;
        ranges.convert(
            entries
                .iter()
                .map(|entry| (entry.token().value(), Some(()))),
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
    /// `scratch` is the caller's buffer for the read's value bytes, which
    /// the decode loop passes to every search of a guess, as the pin's
    /// table reuses its own chunk. Its contents on return are unspecified.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] when the dictionary backend cannot serve the lookup.
    fn trellis_ranges_continued(
        &self,
        syllables: &[Self::Syllable],
        tones: &[u8],
        ranges: &mut PhraseIndexRanges<()>,
        _scratch: &mut Vec<u8>,
    ) -> Result<bool, Self::Error> {
        self.trellis_ranges(syllables, tones, ranges)?;
        self.phrase_prefix_exists(syllables)
    }

    /// What one trellis step prices of `token`, either ranged by
    /// [`Self::trellis_ranges`] or explicitly forced over `syllables` and `tones`
    /// (`get_phrase_item` and `compute_pronunciation_possibility`,
    /// `phonetic_lookup.h:646-684`); `None` when the token has no
    /// readable item there.
    ///
    /// The default reads the last of [`Self::trellis_records`]' entries
    /// for the token. Dictionaries with unindexed item readings override it
    /// to read those items independently of their index membership.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] when the dictionary backend cannot serve the lookup.
    fn trellis_item(
        &self,
        syllables: &[Self::Syllable],
        tones: &[u8],
        token: u32,
    ) -> Result<Option<TrellisItem>, Self::Error> {
        Ok(self
            .trellis_records(syllables, tones)?
            .iter()
            .rev()
            .find(|entry| entry.token().value() == token)
            .map(|entry| TrellisItem {
                chars: entry.phrase_length().try_into().unwrap_or(u32::MAX),
                pronunciation: entry.pronunciation_possibility(),
            }))
    }

    /// [`Self::trellis_item`], given the `memo` the caller keeps for this
    /// key path while one n-best call prices the tokens ranged on it. A
    /// dictionary may keep there whatever it would otherwise work out
    /// again for each token of the path; the answer must be exactly
    /// [`Self::trellis_item`]'s. The caller makes a fresh
    /// [`Self::PathMemo`] per path and drops it with the span search that
    /// found the path.
    ///
    /// The default ignores the memo.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] when the dictionary backend cannot serve the lookup.
    fn trellis_item_on_path(
        &self,
        syllables: &[Self::Syllable],
        tones: &[u8],
        token: u32,
        memo: &Self::PathMemo,
    ) -> Result<Option<TrellisItem>, Self::Error> {
        let _ = memo;
        self.trellis_item(syllables, tones, token)
    }

    /// Sentence-only pricing with a guess-local token memo.
    /// # Errors
    /// Preserves the dictionary's pricing errors.
    fn trellis_item_with_facts(
        &self,
        syllables: &[Self::Syllable],
        tones: &[u8],
        token: u32,
        memo: &Self::PathMemo,
        _facts: &mut sentence_facts::SentenceFacts,
    ) -> Result<Option<TrellisItem>, Self::Error> {
        self.trellis_item_on_path(syllables, tones, token, memo)
    }

    /// Sentence-only final text read using the same token memo.
    /// # Errors
    /// Preserves the dictionary's text errors.
    fn trellis_text_with_facts(
        &self,
        syllables: &[Self::Syllable],
        tones: &[u8],
        token: u32,
        _facts: &mut sentence_facts::SentenceFacts,
    ) -> Result<Option<TokenPhrase>, Self::Error> {
        self.trellis_text(syllables, tones, token)
    }

    /// The phrase [`Self::trellis_item`] priced, read when a result is
    /// emitted (`convert_to_utf8`, `lookup/lookup.cpp:39-68`): its stored
    /// length and its text, which is `None` where GLib's conversion answers
    /// NULL. `None` where that item is `None`.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] when the dictionary backend cannot serve the lookup.
    fn trellis_text(
        &self,
        syllables: &[Self::Syllable],
        tones: &[u8],
        token: u32,
    ) -> Result<Option<TokenPhrase>, Self::Error> {
        Ok(self
            .trellis_records(syllables, tones)?
            .iter()
            .rev()
            .find(|entry| entry.token().value() == token)
            .map(|entry| TokenPhrase::from(entry.text().clone())))
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
        out: &mut Vec<PhraseEntry>,
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
        out: &mut Vec<PhraseEntry>,
    ) -> Result<bool, Self::Error> {
        self.lookup_into(syllables, out)?;
        Ok(false)
    }

    /// The candidate search of one key path: the entries, the first
    /// unreadable token with the unreadable row count, and the
    /// `SEARCH_CONTINUED` bit. The pin answers all three from one
    /// `search_internal` read per table (`chewing_large_table2_bdb.cpp:
    /// 180-189`, `facade_chewing_table2.h:109-129` at 074a2219), and a
    /// nonzero query tone keeps only stored readings with an equal or zero
    /// tone (`pinyin_phrase3.h:97-105`). `row` is the caller's buffer for
    /// that read.
    ///
    /// The default is the tone-less [`Self::lookup_into_flagged`] then
    /// [`Self::phrase_prefix_exists`].
    ///
    /// # Errors
    /// Returns a dictionary read or decoding error.
    fn candidate_search_into(
        &self,
        syllables: &[Self::Syllable],
        _tones: &[u8],
        out: &mut Vec<PhraseEntry>,
        _row: &mut Vec<u8>,
    ) -> Result<(UnreadableRows, bool), Self::Error> {
        let failed = self.lookup_into_flagged(syllables, out)?;
        let unreadable = UnreadableRows {
            count: usize::from(failed),
            first: None,
        };
        Ok((unreadable, self.phrase_prefix_exists(syllables)?))
    }

    /// Whether a stored phrase's pinyin can extend `syllables`.
    ///
    /// This is libpinyin's `SEARCH_CONTINUED` probe: the phrase index reports
    /// the bit when the key sequence is a prefix of some stored phrase's
    /// pinyin, which is what lets the candidate window scan stop widening.
    /// When `syllables` contains an initial-only key the probe runs on the
    /// initial sequence instead — the index the pin uses for incomplete
    /// spellings — where a complete key contributes its own initial and an
    /// initial-only key stands for every syllable that shares it. An
    /// implementation answers such a mixed query directly, as the pin's
    /// incomplete-key index does: `compute_incomplete_chewing_index`
    /// (`pinyin_phrase3.h:170-178` at 074a2219) projects each position to
    /// its initial.
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

    /// The phrase tokens stored for an exact raw UCS-4 span — the
    /// non-scalar counterpart of [`Self::tokens_for_text`]. The span may
    /// contain surrogate code points or values above U+10FFFF, so the
    /// default converts only an all-scalar span to text; a span with a
    /// non-scalar is empty, and a table-backed dictionary overrides this
    /// with a raw key read.
    fn tokens_for_ucs4(&self, ucs4: &[u32]) -> Vec<PhraseToken> {
        let mut text = String::with_capacity(ucs4.len());
        for &word in ucs4 {
            let Some(character) = char::from_u32(word) else {
                return Vec::new();
            };
            text.push(character);
        }
        self.tokens_for_text(&text)
    }

    /// Whether phrase segmentation must retain native search calls even
    /// at unreachable steps. Failed native handles can emit diagnostics.
    fn phrase_span_searches_are_observable(&self) -> bool {
        false
    }

    /// Continuation after the preceding exact phrase-span search. This
    /// auxiliary query must not repeat that search on a failed handle.
    fn phrase_span_continues(&self, _ucs4: &[u32]) -> bool {
        true
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
    fn lookup_addon(&self, _syllables: &[Self::Syllable]) -> Result<Vec<PhraseEntry>, Self::Error> {
        Ok(Vec::new())
    }

    /// Reads addon candidates into a caller-owned buffer and returns the
    /// same search's `SEARCH_CONTINUED` bit. The default uses
    /// [`Self::lookup_addon`] and has no continuation bit.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] when the addon backend cannot serve the lookup.
    fn search_addon_into(
        &self,
        syllables: &[Self::Syllable],
        out: &mut Vec<PhraseEntry>,
        _row: &mut Vec<u8>,
    ) -> Result<bool, Self::Error> {
        let entries = self.lookup_addon(syllables)?;
        out.clear();
        out.extend(entries);
        Ok(false)
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
    fn phrase_text_for_token(&self, _token: u32) -> Option<PhraseText> {
        None
    }

    /// The current phrase item's stored length, or `None` for a missing or
    /// unloaded item. Offset-zero dynamic ranking chooses the longest retained
    /// prefix (`074a2219` pinyin.cpp:1711-1740; zhuyin.cpp:1146-1175).
    /// Dictionaries without separate item storage may use their token text.
    fn phrase_length_for_token(&self, token: u32) -> Option<usize> {
        self.phrase_text_for_token(token)
            .map(|text| text.phrase_length())
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
    /// table.
    ///
    /// The default answers [`PhraseItemRead::NoItem`]: a dictionary that
    /// keeps no phrase items has none to read, as the pin's
    /// `get_phrase_item` fails for a token it has no item for.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] when the backend cannot read the item.
    fn phrase_item_pronunciations(
        &self,
        _token: PhraseToken,
    ) -> Result<PhraseItemRead, Self::Error> {
        Ok(PhraseItemRead::NoItem)
    }
}

/// A phrase item's stored pronunciations in stored order: each one's keys,
/// tones included, and its `guint32` frequency.
pub type PronunciationRows = Vec<(Vec<ChewingKey>, u32)>;

/// What `_token_get_phrase` and `convert_to_utf8` read from a present item
/// (074a2219 `pinyin.cpp:1627-1646`, `zhuyin.cpp:1096-1114`,
/// `lookup/lookup.cpp:39-68`): the stored length and GLib's conversion of
/// the stored words.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TokenPhrase {
    /// The stored `phrase_length`, which the pin writes to `*len`.
    pub phrase_length: usize,
    /// The text, `None` where `g_ucs4_to_utf8` answers NULL: a word at or
    /// above 0x80000000 before the first zero. The item is still present.
    pub text: Option<PhraseText>,
}

impl From<PhraseText> for TokenPhrase {
    /// A text that converts, with its own stored length.
    fn from(text: PhraseText) -> Self {
        Self {
            phrase_length: text.phrase_length(),
            text: Some(text),
        }
    }
}

/// What [`Dictionary::phrase_item_pronunciations`] found behind a token.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PhraseItemRead {
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

/// The order `reduce_tokens` gives a token list
/// (`storage/phrase_large_table3.h:77-91` at the pin `074a2219`): it appends
/// the per-library arrays in library order. `add_index` files a token under
/// `PHRASE_INDEX_LIBRARY_INDEX(token)` and keeps its whole value
/// (`phrase_large_table3.cpp:42`), so the key is the masked library index.
/// A stable sort by it gives a merged list that order and keeps the given
/// order within one library.
///
/// Every caller sorts through this function, so the libraries hold one
/// instance of the sort.
pub fn sort_tokens_by_library(tokens: &mut [u32]) {
    tokens.sort_by_key(|token| phrase_index_library(*token));
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

/// A value a dictionary works out once per trellis key path and keeps for
/// the n-best call that prices the path ([`Dictionary::PathMemo`]). The
/// caller holds one per path and drops it with the span search that found
/// the path, so nothing kept here outlives one guess, or any user-store
/// write between guesses.
#[derive(Debug)]
pub struct TrellisPathMemo<T>(std::cell::OnceCell<T>);

impl<T> Default for TrellisPathMemo<T> {
    fn default() -> Self {
        Self(std::cell::OnceCell::new())
    }
}

impl<T> TrellisPathMemo<T> {
    /// The kept value, computed by `init` the first time.
    ///
    /// # Errors
    ///
    /// Returns `init`'s error, keeping nothing, so the next call tries
    /// again.
    pub fn get_or_try_init<E>(&self, init: impl FnOnce() -> Result<T, E>) -> Result<&T, E> {
        if let Some(value) = self.0.get() {
            return Ok(value);
        }
        let value = init()?;
        Ok(self.0.get_or_init(|| value))
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
    #[cfg(test)]
    fn walk(&self) -> impl Iterator<Item = &E> {
        self.iter().flat_map(|range| self.entries(range))
    }

    /// Appends `other`'s ranges after this one's, library by library, with
    /// its entries mapped through `map`; `other` is left empty. Per-library
    /// append order is kept, which is all the walk observes.
    pub(crate) fn append_from<F>(
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
    /// Token representation observed by this model.
    type Token;

    /// Observation failure reported by this model.
    type Error;

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
    /// syllable length. The default records the token alone, by delegating
    /// to [`Self::observe`]. Learning-off callers omit this call.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] when the observation cannot be recorded.
    fn observe_with_keys(
        &mut self,
        history: &[Self::Token],
        token: &Self::Token,
        _readings: impl Iterator<Item = Vec<ChewingKey>>,
    ) -> Result<(), Self::Error> {
        self.observe(history, token)
    }
}

/// One `prev → *` bigram row with the user overlay already merged: the
/// pin's `m_merged_single_gram`, the chunk `merge_single_gram` builds
/// (`ngram.cpp:277-352` at `074a2219`).
///
/// The row answers as the merged chunk does, read with the ported
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
#[derive(Clone, Debug, Default)]
pub struct MergedGram {
    chunk: Vec<u8>,
    fault: Option<GramFault>,
}

pub use single_gram::GramFault;

/// What the pin's merge of two present grams meets before its loop: both
/// totals are read first (`ngram.cpp:303-304`), asserting on a value
/// shorter than one ([`GramFault::Short`], register row 85); every item of
/// both values is then consumed, so a partial tail on either is read past
/// ([`GramFault::OverRead`], row 98).
fn merge_fault(system: &[u8], user: &[u8]) -> Option<GramFault> {
    let short = |gram: &[u8]| single_gram::get_total_freq(gram).is_none();
    if short(system) || short(user) {
        Some(GramFault::Short)
    } else if single_gram::has_partial_tail(system) || single_gram::has_partial_tail(user) {
        Some(GramFault::OverRead)
    } else {
        None
    }
}

/// The chunk `merge_single_gram` builds from two present, readable grams.
fn merged_chunk(system: &[u8], user: &[u8]) -> Vec<u8> {
    let mut chunk = Vec::with_capacity(
        system
            .len()
            .saturating_add(user.len())
            .saturating_sub(single_gram::TOTAL_LEN),
    );
    single_gram::merge_single_gram(&mut chunk, Some(system), Some(user));
    chunk
}

impl MergedGram {
    const fn chunk(chunk: Vec<u8>) -> Self {
        Self { chunk, fault: None }
    }

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
        Self::chunk(chunk)
    }

    /// `merge_single_gram(merged, system, user)`: `None` when both grams
    /// are absent (the pin answers `false`), the present side's own bytes
    /// when the other is absent, and otherwise the two-pointer merge.
    ///
    /// Both present, a value the pin cannot read is a fault
    /// ([`GramFault::Short`], [`GramFault::OverRead`]) and no chunk.
    #[must_use]
    pub fn merge(system: Option<Vec<u8>>, user: Option<Vec<u8>>) -> Option<Self> {
        match (system, user) {
            (None, None) => None,
            (Some(chunk), None) | (None, Some(chunk)) => Some(Self::chunk(chunk)),
            (Some(system), Some(user)) => Some(match merge_fault(&system, &user) {
                Some(fault) => Self {
                    chunk: Vec::new(),
                    fault: Some(fault),
                },
                None => Self::chunk(merged_chunk(&system, &user)),
            }),
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
        let end = last.wrapping_add(1);
        single_gram::checked_search(&self.chunk, first, end, out)
    }

    /// A copy of the merged chunk's bytes.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        self.chunk.clone()
    }
}

/// One predecessor's row for one span load: the pin's
/// `m_merged_single_gram` between one `search_bigram2` load and the next
/// (`phonetic_lookup.h:588-599`, `phrase_lookup.cpp:268-276` at 074a2219).
/// It is never kept for another span, step or guess. A loaded miss is
/// reused within the load too.
#[derive(Default)]
pub struct NbestStepRow {
    /// `None` until the load; a loaded miss is `Some(None)`.
    loaded: Option<Option<MergedGram>>,
}

impl NbestStepRow {
    /// Loads once on success, including a missing row. Errors are propagated
    /// without turning corruption into a loaded miss.
    ///
    /// # Errors
    /// Returns the loader's error unchanged.
    pub fn get_or_try_init<E>(
        &mut self,
        load: impl FnOnce() -> Result<Option<MergedGram>, E>,
    ) -> Result<Option<&MergedGram>, E> {
        if self.loaded.is_none() {
            self.loaded = Some(load()?);
        }
        Ok(self.loaded.as_ref().and_then(Option::as_ref))
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

/// Deterministic language-model seam.
///
/// Implementations do not panic on caller input. They report arithmetic or
/// backend failures through [`Result`].
pub trait LanguageModel {
    /// Token representation scored by this model.
    type Token;

    /// Scoring failure reported by this model.
    type Error;

    /// The model's real unigram frequency of `token`, when the model carries
    /// one — upstream's phrase-item field (`PhraseItem::get_unigram_frequency`:
    /// the corpus count plus the one `gen_unigram` adds to every item, so a
    /// counted phrase is never 0).
    ///
    /// `None` means the model exposes no frequency table at all; every
    /// candidate then ranks at frequency zero. `Some(0)` is a real table miss
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

    /// The two pre-log branches of the n-best sentence trellis.
    ///
    /// `blended` is `f64::from(lambda * bigram_poss) +
    /// f64::from(unigram_lambda) * elem_poss`, where the first product is
    /// f32. `unigram` is the unweighted f64 `elem_poss`, accompanied by
    /// `unigram_lambda`. The model applies the pin's epsilon gates before
    /// weighting; None skips a branch. The engine supplies pronunciation,
    /// then takes f64::ln and narrows the accumulated sum to f32.
    /// See `phonetic_lookup.h:646-697` at 074a2219.
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

    /// Score an occurrence with facts already read by sentence pricing.
    /// # Errors
    /// Preserves the model's occurrence-scoring errors.
    fn nbest_item_costs_with_facts(
        &self,
        prev: &Self::Token,
        token: &Self::Token,
        item: Option<(u32, u32)>,
        row: &mut NbestStepRow,
        _facts: &mut sentence_facts::SentenceFacts,
    ) -> Result<NbestStepCosts, Self::Error> {
        self.nbest_item_costs_in_row(prev, token, item, row)
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

    /// Whether `error` is a backend read that failed after its size check
    /// succeeded — the point where Kyoto Cabinet's `Bigram::load` asserts
    /// (`ngram_kyotodb.cpp:131-139` at 074a2219). Defaulted to `false` for
    /// models whose errors never carry that shape.
    fn is_read_after_check(&self, _error: &Self::Error) -> bool {
        false
    }

    /// The `gfloat` λ of the install's `table.conf` — the weight the pin's
    /// candidate frequency (`(λ·bigram + (1−λ)·unigram)·2²⁴`,
    /// `pinyin.cpp:1862-1866`, `zhuyin.cpp:1236-1280`) is computed with.
    /// Defaulted to the pinned model's `0.312699`.
    fn amplification_lambda(&self) -> f32 {
        0.312_699
    }
}

/// Pre-log pieces returned by [`LanguageModel::nbest_step_costs`].
/// The method names are shared with the row-reading seam; these values
/// are probabilities, never integer costs or precomputed logarithms.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NbestStepCosts {
    /// Weighted blend before pronunciation and log; absent only below
    /// the pin's combined bigram/unigram epsilon gate or without an item.
    pub blended: Option<f64>,
    /// Unweighted f64 frequency/total; absent below DBL_EPSILON or
    /// without a visible phrase item. NaN passes the pin's `<` gate.
    pub unigram: Option<f64>,
    /// `(1.0_f64 - lambda_f32 as f64) as f32`. Kept separate so the
    /// engine evaluates `(elem_poss * pinyin_poss) * unigram_lambda`.
    pub unigram_lambda: f32,
}

impl Default for NbestStepCosts {
    fn default() -> Self {
        Self {
            blended: None,
            unigram: None,
            unigram_lambda: 1.0,
        }
    }
}

#[cfg(test)]
mod tests;
