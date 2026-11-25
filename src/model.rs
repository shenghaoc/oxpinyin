//! Dictionary and language-model contracts used by the session engine.
use crate::parser::ChewingKey;
use crate::single_gram::GramFault;
use crate::vocab::{
    PHRASE_INDEX_LIBRARY_COUNT, PhraseEntry, PhraseText, PhraseToken, phrase_index_library_index,
};
use crate::{sentence_facts, single_gram};

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
}

/// Read-only seam the engine reads dictionaries through
/// (`docs/findings/core-trait-seam.md`).
///
/// Every method is one the engine calls, and every method is required: a
/// dictionary answers each one itself. A dictionary that only lists
/// entries implements [`PhraseList`] instead, and the blanket
/// implementation derives these methods from its lookup.
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
    /// ([`Self::trellis_item_with_facts`]). A dictionary with nothing to
    /// keep uses `()`.
    type PathMemo: Default;

    /// One `FacadeChewingTable2::search` call of the sentence trellis,
    /// with the same search's `SEARCH_CONTINUED` probe: appends this
    /// lookup's `PhraseIndexRange`s to `ranges`
    /// (`chewing_large_table2.h:91-133` at the pin `074a2219`), the system
    /// table's call before the user table's
    /// (`facade_chewing_table2.h:109-129`), and returns whether a stored
    /// phrase's pinyin extends `syllables`. The pin's one
    /// `ChewingLargeTable2::search` call gives the ranges and
    /// `SEARCH_CONTINUED` from a single DBM get
    /// (`chewing_large_table2_bdb.cpp:165-192`). `tones` supplies the query
    /// tones; missing tones are zero.
    ///
    /// The ranges carry tokens only, as the pin's `convert` does: a step
    /// prices the tokens it meets through [`Self::trellis_item_with_facts`],
    /// and the text of a result is read through
    /// [`Self::trellis_text_with_facts`] when it is emitted. Every ranged
    /// token holds a `Some(())` slot.
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
        scratch: &mut Vec<u8>,
    ) -> Result<bool, Self::Error>;

    /// What one trellis step prices of `token`, either ranged by
    /// [`Self::trellis_ranges_continued`] or explicitly forced over
    /// `syllables` and `tones` (`get_phrase_item` and
    /// `compute_pronunciation_possibility`, `phonetic_lookup.h:646-684`);
    /// `None` when the token has no readable item there. It also records
    /// the sentence facts of the read in `facts`.
    ///
    /// `memo` is what the caller keeps for this key path while one n-best
    /// call prices the tokens ranged on it. A dictionary may keep there
    /// whatever it would otherwise work out again for each token of the
    /// path; the item must not depend on what the memo holds. The caller
    /// makes a fresh [`Self::PathMemo`] per path and drops it with the span
    /// search that found the path.
    ///
    /// # Errors
    ///
    /// Preserves the dictionary's pricing errors.
    fn trellis_item_with_facts(
        &self,
        syllables: &[Self::Syllable],
        tones: &[u8],
        token: u32,
        memo: &Self::PathMemo,
        facts: &mut sentence_facts::SentenceFacts,
    ) -> Result<Option<TrellisItem>, Self::Error>;

    /// The phrase [`Self::trellis_item_with_facts`] priced, read when a result is
    /// emitted (`convert_to_utf8`, `lookup/lookup.cpp:39-68`), recording the
    /// sentence facts of the read in `facts`: its stored length and its
    /// text, which is `None` where GLib's conversion answers NULL. `None`
    /// where that item is `None`. It takes no path memo.
    ///
    /// # Errors
    ///
    /// Preserves the dictionary's text errors.
    fn trellis_text_with_facts(
        &self,
        syllables: &[Self::Syllable],
        tones: &[u8],
        token: u32,
        facts: &mut sentence_facts::SentenceFacts,
    ) -> Result<Option<TokenPhrase>, Self::Error>;

    /// The candidate search of one key path: the entries, the first
    /// unreadable token with the unreadable row count, and the
    /// `SEARCH_CONTINUED` bit. The pin answers all three from one
    /// `search_internal` read per table (`chewing_large_table2_bdb.cpp:
    /// 180-189`, `facade_chewing_table2.h:109-129` at 074a2219), and a
    /// nonzero query tone keeps only stored readings with an equal or zero
    /// tone (`pinyin_phrase3.h:97-105`). `row` is the caller's buffer for
    /// that read.
    ///
    /// A token of a loaded library whose item cannot be read is counted as
    /// unreadable and its entry is dropped. The pin's candidate listing
    /// dies on such a token (`pinyin.cpp:1635-1637`, `:2053`); the answer
    /// rides the call that met it, not shared state, so the caller can fail
    /// exactly the listing it was building.
    ///
    /// # Errors
    /// Returns a dictionary read or decoding error.
    fn candidate_search_into(
        &self,
        syllables: &[Self::Syllable],
        tones: &[u8],
        out: &mut Vec<PhraseEntry>,
        row: &mut Vec<u8>,
    ) -> Result<(UnreadableRows, bool), Self::Error>;

    /// Reads addon candidates into a caller-owned buffer and returns the
    /// same search's `SEARCH_CONTINUED` bit.
    ///
    /// The default lookup uses [`Self::candidate_search_into`]. A separate
    /// pass keeps addon tokens distinct from `MERGED_DICTIONARY` tokens
    /// that use the same library nibble. See
    /// [candidate collection](https://github.com/shenghaoc/oxpinyin/blob/main/docs/findings/phrase-union.md#63-candidate-collection).
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] when the addon backend cannot serve the lookup.
    fn search_addon_into(
        &self,
        syllables: &[Self::Syllable],
        out: &mut Vec<PhraseEntry>,
        row: &mut Vec<u8>,
    ) -> Result<bool, Self::Error>;

    /// Tokens of phrases whose stored pinyin **strictly extends**
    /// `syllables`, with the overlong-index gate checked in the same walk —
    /// `ChewingLargeTable2::search_suggestion`
    /// (`chewing_large_table2_tkrzwdb.cpp:493-535` at the pin `074a2219`),
    /// the row source of the LONGER candidate
    /// (`_prepend_longer_candidates`, `pinyin.cpp:1870-1933`). `None` names
    /// the pin's `search_suggestion` abort; `Some` holds the tokens,
    /// including an empty list when the query has no extensions.
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
    /// ties, so the order is observable.
    ///
    /// The gate is the pin's user-chewing-table over-long-key gate
    /// (`src/storage/chewing_large_table2_bdb.cpp:282`): it closes when the
    /// dictionary's user index holds a reading past `MAX_PHRASE_LENGTH`
    /// syllables that extends `syllables`, so the pin's longer-candidate
    /// walk reaches the key and dies at `switch`'s `default: abort()`.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] when the backend cannot serve the walk.
    fn checked_extension_tokens(
        &self,
        syllables: &[Self::Syllable],
    ) -> Result<Option<Vec<PhraseToken>>, Self::Error>;

    /// The phrase tokens stored for an exact raw UCS-4 span, the probe
    /// `pinyin_phrase_segment`'s span DP looks up per character span
    /// (`PhraseLookup::get_best_match` searches the phrase table per
    /// span; `phrase_lookup.cpp:135-157` at the pin). The span may contain
    /// surrogate code points or values above U+10FFFF. A table-backed
    /// dictionary answers from its reverse text maps in stored order
    /// (system seam first, then the user seam).
    fn tokens_for_ucs4(&self, ucs4: &[u32]) -> Vec<PhraseToken>;

    /// Whether phrase segmentation must retain native search calls even
    /// at unreachable steps. Failed native handles can emit diagnostics.
    fn phrase_span_searches_are_observable(&self) -> bool;

    /// Continuation after the preceding exact phrase-span search. This
    /// auxiliary query must not repeat that search on a failed handle.
    fn phrase_span_continues(&self, ucs4: &[u32]) -> bool;

    /// The phrase text a token owns, for rows the candidate surface must
    /// render from the token alone — `_token_get_phrase`
    /// (`pinyin.cpp:1994-2050` at the pin), which fills a LONGER
    /// candidate's string from the phrase index item. `None` when the
    /// dictionary cannot back the row's text (the row is then not built).
    fn phrase_text_for_token(&self, token: u32) -> Option<PhraseText>;

    /// The current phrase item's stored length, or `None` for a missing or
    /// unloaded item. Offset-zero dynamic ranking chooses the longest retained
    /// prefix (`074a2219` pinyin.cpp:1711-1740; zhuyin.cpp:1146-1175).
    fn phrase_length_for_token(&self, token: u32) -> Option<usize>;

    /// The stored pronunciations of the phrase item behind `token` —
    /// `FacadePhraseIndex::get_phrase_item` then `get_nth_pronunciation`
    /// (`phrase_index.h` at the pin `074a2219`): each pronunciation's keys,
    /// tones included, and its `guint32` frequency, in stored order. This is
    /// what `ForwardPhoneticConstraints::validate_constraint` revalidates a
    /// forcing against (`phonetic_lookup.cpp:120-165`): it searches no
    /// table. [`PhraseItemRead::NoItem`] when the dictionary has no item for
    /// the token, as the pin's `get_phrase_item` fails then.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] when the backend cannot read the item.
    fn phrase_item_pronunciations(&self, token: PhraseToken)
    -> Result<PhraseItemRead, Self::Error>;
}

/// A dictionary that lists entries: the fixture doubles and the evaluation
/// tool's dictionary (`docs/findings/core-trait-seam.md`).
///
/// `lookup` is the one required method. The other methods are hooks with
/// defaults; an implementor overrides only those it models. The blanket
/// implementation derives [`Dictionary`] from them, with `PathMemo = ()`.
pub trait PhraseList {
    /// Syllable representation accepted by this dictionary.
    type Syllable;

    /// Lookup failure reported by this dictionary.
    type Error;

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
    ///
    /// The default is [`Self::lookup`], which ignores the tones.
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

    /// What one trellis step prices of `token`
    /// ([`Dictionary::trellis_item_with_facts`]): the last of
    /// [`Self::trellis_records`]' entries for the token, `None` when there is
    /// none.
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
            .find(|entry| entry.token() == token)
            .map(|entry| TrellisItem {
                chars: entry.phrase_length().try_into().unwrap_or(u32::MAX),
                pronunciation: entry.pronunciation_possibility(),
            }))
    }

    /// Looks up a key sequence into `out` and reports whether a loaded item was unreadable.
    /// The default replaces `out` after [`Self::lookup`] succeeds and reports no unreadable
    /// item.
    ///
    /// # Errors
    ///
    /// The default propagates a lookup error and preserves the previous contents of `out`.
    fn lookup_into_flagged(
        &self,
        syllables: &[Self::Syllable],
        out: &mut Vec<PhraseEntry>,
    ) -> Result<bool, Self::Error> {
        lookup_into(self, syllables, out)?;
        Ok(false)
    }

    /// Searches one candidate path into `out` and reports unreadable rows and possible
    /// continuations. The default calls [`Self::lookup_into_flagged`], then
    /// [`Self::phrase_prefix_exists`].
    ///
    /// # Errors
    ///
    /// Returns a lookup or prefix-probe error. A prefix-probe error leaves the entries from
    /// the completed lookup in `out`.
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
    /// The default is `true`: always continuing the scan yields the same
    /// candidate set (the probe only stops windows that would return
    /// nothing), at the cost of searching every window to the end of the
    /// input.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] when the dictionary backend cannot evaluate the prefix probe.
    fn phrase_prefix_exists(&self, _syllables: &[Self::Syllable]) -> Result<bool, Self::Error> {
        Ok(true)
    }

    /// The phrase tokens stored for an exact UTF-8 `text`.
    ///
    /// The default is empty.
    fn tokens_for_text(&self, _text: &str) -> Vec<PhraseToken> {
        Vec::new()
    }

    /// [`Dictionary::tokens_for_ucs4`] for this dictionary: the
    /// non-scalar counterpart of [`Self::tokens_for_text`].
    ///
    /// The default converts only an all-scalar span to text; a span with a
    /// non-scalar is empty.
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

    /// Addon-facade lookup pass ([`Dictionary::search_addon_into`]).
    ///
    /// The default is empty.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] when the addon backend cannot serve the lookup.
    fn lookup_addon(&self, _syllables: &[Self::Syllable]) -> Result<Vec<PhraseEntry>, Self::Error> {
        Ok(Vec::new())
    }

    /// Tokens of phrases whose stored pinyin strictly extends `syllables`,
    /// in the order [`Dictionary::checked_extension_tokens`] states.
    ///
    /// The default is empty: a dictionary that does not override it
    /// surfaces no LONGER candidate.
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

    /// [`Dictionary::phrase_text_for_token`] for this dictionary.
    ///
    /// The default is `None`: the dictionary cannot back a LONGER row's
    /// text.
    fn phrase_text_for_token(&self, _token: u32) -> Option<PhraseText> {
        None
    }

    /// [`Dictionary::phrase_length_for_token`] for this dictionary.
    ///
    /// The default is the length of [`Self::phrase_text_for_token`]'s text.
    fn phrase_length_for_token(&self, token: u32) -> Option<usize> {
        PhraseList::phrase_text_for_token(self, token).map(|text| text.phrase_length())
    }

    /// [`Dictionary::phrase_item_pronunciations`] for this dictionary.
    ///
    /// The default answers [`PhraseItemRead::NoItem`]: a dictionary that
    /// keeps no phrase items has none to read.
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

/// [`Dictionary`] derived from a [`PhraseList`].
impl<T: PhraseList> Dictionary for T {
    type Syllable = <T as PhraseList>::Syllable;
    type Error = <T as PhraseList>::Error;
    type PathMemo = ();

    /// The ranges of [`PhraseList::trellis_records`]' tokens, then
    /// [`PhraseList::phrase_prefix_exists`].
    fn trellis_ranges_continued(
        &self,
        syllables: &[Self::Syllable],
        tones: &[u8],
        ranges: &mut PhraseIndexRanges<()>,
        _scratch: &mut Vec<u8>,
    ) -> Result<bool, Self::Error> {
        trellis_ranges(self, syllables, tones, ranges)?;
        self.phrase_prefix_exists(syllables)
    }

    /// [`PhraseList::trellis_item`]; the memo and the facts are not used.
    fn trellis_item_with_facts(
        &self,
        syllables: &[Self::Syllable],
        tones: &[u8],
        token: u32,
        memo: &Self::PathMemo,
        _facts: &mut sentence_facts::SentenceFacts,
    ) -> Result<Option<TrellisItem>, Self::Error> {
        trellis_item_on_path(self, syllables, tones, token, memo)
    }

    /// The text of the last of [`PhraseList::trellis_records`]' entries for
    /// the token; the facts are not used.
    fn trellis_text_with_facts(
        &self,
        syllables: &[Self::Syllable],
        tones: &[u8],
        token: u32,
        _facts: &mut sentence_facts::SentenceFacts,
    ) -> Result<Option<TokenPhrase>, Self::Error> {
        trellis_text(self, syllables, tones, token)
    }

    fn candidate_search_into(
        &self,
        syllables: &[Self::Syllable],
        tones: &[u8],
        out: &mut Vec<PhraseEntry>,
        row: &mut Vec<u8>,
    ) -> Result<(UnreadableRows, bool), Self::Error> {
        PhraseList::candidate_search_into(self, syllables, tones, out, row)
    }

    /// [`PhraseList::lookup_addon`], with no continuation bit.
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

    /// [`PhraseList::suggest_extension_tokens`]; the gate is always open.
    fn checked_extension_tokens(
        &self,
        syllables: &[Self::Syllable],
    ) -> Result<Option<Vec<PhraseToken>>, Self::Error> {
        if overlong_extension_gate(self, syllables) {
            Ok(None)
        } else {
            self.suggest_extension_tokens(syllables).map(Some)
        }
    }

    fn tokens_for_ucs4(&self, ucs4: &[u32]) -> Vec<PhraseToken> {
        PhraseList::tokens_for_ucs4(self, ucs4)
    }

    /// `false`: a listed dictionary has no native handle.
    fn phrase_span_searches_are_observable(&self) -> bool {
        false
    }

    /// `true`.
    fn phrase_span_continues(&self, _ucs4: &[u32]) -> bool {
        true
    }

    fn phrase_text_for_token(&self, token: u32) -> Option<PhraseText> {
        PhraseList::phrase_text_for_token(self, token)
    }

    fn phrase_length_for_token(&self, token: u32) -> Option<usize> {
        PhraseList::phrase_length_for_token(self, token)
    }

    fn phrase_item_pronunciations(
        &self,
        token: PhraseToken,
    ) -> Result<PhraseItemRead, Self::Error> {
        PhraseList::phrase_item_pronunciations(self, token)
    }
}

/// Applies [`PhraseIndexRanges::convert`] to
/// [`PhraseList::trellis_records`]' tokens as one call.
fn trellis_ranges<L: PhraseList>(
    list: &L,
    syllables: &[L::Syllable],
    tones: &[u8],
    ranges: &mut PhraseIndexRanges<()>,
) -> Result<(), L::Error> {
    let entries = list.trellis_records(syllables, tones)?;
    ranges.convert(entries.iter().map(|entry| (entry.token(), Some(()))));
    Ok(())
}

/// [`PhraseList::trellis_item`]; a listed dictionary keeps no path memo.
fn trellis_item_on_path<L: PhraseList>(
    list: &L,
    syllables: &[L::Syllable],
    tones: &[u8],
    token: u32,
    memo: &(),
) -> Result<Option<TrellisItem>, L::Error> {
    let _ = memo;
    list.trellis_item(syllables, tones, token)
}

/// The text of the last of [`PhraseList::trellis_records`]' entries for
/// `token`.
fn trellis_text<L: PhraseList>(
    list: &L,
    syllables: &[L::Syllable],
    tones: &[u8],
    token: u32,
) -> Result<Option<TokenPhrase>, L::Error> {
    Ok(list
        .trellis_records(syllables, tones)?
        .iter()
        .rev()
        .find(|entry| entry.token() == token)
        .map(|entry| TokenPhrase::from(entry.text().clone())))
}

/// [`PhraseList::lookup`] into a caller-owned buffer.
fn lookup_into<L: PhraseList + ?Sized>(
    list: &L,
    syllables: &[L::Syllable],
    out: &mut Vec<PhraseEntry>,
) -> Result<(), L::Error> {
    let entries = list.lookup(syllables)?;
    out.clear();
    out.extend(entries);
    Ok(())
}

/// A listed dictionary has no user index, so the over-long-key gate is
/// always open.
fn overlong_extension_gate<L: PhraseList>(_list: &L, _syllables: &[L::Syllable]) -> bool {
    false
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
pub(crate) fn sort_tokens_by_library(tokens: &mut [u32]) {
    crate::stable_sort::sort_by_key(tokens, |token| {
        u128::from(crate::vocab::phrase_index_library_index(*token))
    });
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
    pub(crate) fn get_or_try_init<E>(&self, init: impl FnOnce() -> Result<T, E>) -> Result<&T, E> {
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
    libraries: [Vec<PhraseIndexRange>; PHRASE_INDEX_LIBRARY_COUNT as usize],
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
                        && usize::from(phrase_index_library_index(open.first))
                            == usize::from(phrase_index_library_index(token)) =>
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
        if let Some(ranges) = self
            .libraries
            .get_mut(usize::from(phrase_index_library_index(range.first)))
        {
            ranges.push(range);
        }
    }

    /// Library `library`'s ranges in append order; empty past the last library.
    #[cfg(test)]
    #[must_use]
    pub fn ranges(&self, library: usize) -> &[PhraseIndexRange] {
        self.libraries.get(library).map_or(&[], Vec::as_slice)
    }

    /// Every range, libraries `0..16` then append order — the n-best
    /// walk's order (`phonetic_lookup.h:560-572`, `:615-632`).
    pub fn iter(&self) -> impl Iterator<Item = &PhraseIndexRange> {
        self.libraries.iter().flatten()
    }

    // ChewingTableEntry::convert, chewing_large_table2.h:91–133.
    /// Resolves `token` within the inclusive endpoints of `range`. Use a range obtained
    /// from this collection since its last [`Self::clear`] or drain. Calling
    /// [`Self::clear`], or draining the collection through `append_from`, invalidates
    /// previously obtained ranges.
    #[must_use]
    pub fn entry(&self, range: &PhraseIndexRange, token: u32) -> Option<&E> {
        if token < range.first || token > range.last {
            return None;
        }
        let offset = usize::try_from(token - range.first).ok()?;
        let first = usize::try_from(range.slot).ok()?;
        self.slots.get(first.checked_add(offset)?)?.as_ref()
    }

    /// Every filled slot with its token, in the walk order of [`Self::iter`].
    pub(crate) fn walk_tokens(&self) -> impl Iterator<Item = (u32, &E)> {
        self.iter().flat_map(move |range| {
            (range.first..=range.last)
                .filter_map(move |token| self.entry(range, token).map(|entry| (token, entry)))
        })
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
    /// Observation failure reported by this model.
    type Error;

    /// Records an accepted `token` after `history`.
    ///
    /// Learning-off callers omit this operation entirely.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] when the model backend cannot record the observation.
    fn observe(&mut self, history: &[PhraseToken], token: &PhraseToken) -> Result<(), Self::Error>;

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
        history: &[PhraseToken],
        token: &PhraseToken,
        _readings: impl Iterator<Item = Vec<ChewingKey>>,
    ) -> Result<(), Self::Error> {
        self.observe(history, token)
    }
}

// m_merged_single_gram is built by merge_single_gram, ngram.cpp:277–352.
// pinyin_guess_candidates loads it before the candidate loop;
// _compute_frequency_of_items reads get_freq inside the loop.
/// A predecessor's bigram row after applying the user overlay. Supply whole records
/// ordered by token so that [`single_gram::lower_bound`] can locate the requested range.
/// Merging keeps record order and does not deduplicate a single input. With one input,
/// its bytes are retained for later reads. With two, the merge reads both headers and
/// walks both record lists.
#[derive(Clone, Debug, Default)]
pub struct MergedGram {
    chunk: Vec<u8>,
    fault: Option<GramFault>,
}

impl MergedGram {
    const fn chunk(chunk: Vec<u8>) -> Self {
        Self { chunk, fault: None }
    }

    /// A row from its total and `(token, count)` items, kept in the order
    /// given; each field is stored as the pin's `guint32`.
    #[cfg(test)]
    #[must_use]
    pub fn new(total: u64, records: Vec<(u32, u64)>) -> Self {
        let records: Vec<_> = records
            .into_iter()
            .map(|(token, count)| (token, count as u32))
            .collect();
        let chunk = single_gram::encode_single_gram(total as u32, &records);
        Self::chunk(chunk)
    }

    // merge_single_gram, ngram.cpp:277–352. Checked Rust accesses retain GramFault::Short
    // and GramFault::OverRead where upstream cannot read both inputs.
    /// Merges two optional bigram rows. Two absent rows produce [`None`]. A single row keeps
    /// its original bytes. For a two-sided merge, supply a header followed by whole records
    /// in ascending token order.
    #[must_use]
    pub fn merge(system: Option<Vec<u8>>, user: Option<Vec<u8>>) -> Option<Self> {
        match (system, user) {
            (None, None) => None,
            (Some(chunk), None) | (None, Some(chunk)) => Some(Self::chunk(chunk)),
            (Some(system), Some(user)) => Some(match single_gram::merge_fault(&system, &user) {
                Some(fault) => Self {
                    chunk: Vec::new(),
                    fault: Some(fault),
                },
                None => Self::chunk(single_gram::merged_chunk(&system, &user)),
            }),
        }
    }

    /// The row's bytes, for a reader to reuse as its next value buffer.
    pub(crate) fn into_chunk(self) -> Vec<u8> {
        self.chunk
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
    pub(crate) fn search_into(
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
    #[cfg(test)]
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
    #[expect(
        clippy::option_option,
        reason = "Unloaded, cached miss and cached row differ; a failed load leaves the row unloaded"
    )]
    loaded: Option<Option<MergedGram>>,
}

impl NbestStepRow {
    /// Empties the row and hands its bytes back to the caller.
    pub(crate) fn take_chunk(&mut self) -> Option<Vec<u8>> {
        self.loaded.take().flatten().map(MergedGram::into_chunk)
    }

    /// Loads once on success, including a missing row. Errors are propagated
    /// without turning corruption into a loaded miss.
    ///
    /// # Errors
    /// Returns the loader's error unchanged.
    pub(crate) fn get_or_try_init<E>(
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
    fn unigram_freq(&self, _token: &PhraseToken) -> Result<Option<u64>, Self::Error> {
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
    fn merged_successors(&self, _prev: &PhraseToken) -> Result<Option<MergedGram>, Self::Error> {
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
    fn addon_unigram_freq(&self, _token: &PhraseToken) -> Result<Option<u64>, Self::Error> {
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
        _prev: &PhraseToken,
        _token: &PhraseToken,
    ) -> Result<NbestStepCosts, Self::Error> {
        Ok(NbestStepCosts::default())
    }

    /// [`Self::nbest_token_costs`] with the facts sentence pricing already
    /// read for this guess.
    ///
    /// # Errors
    /// Returns the same errors as [`Self::nbest_token_costs`].
    fn nbest_token_costs_with_facts(
        &self,
        token: &PhraseToken,
        _facts: &mut sentence_facts::SentenceFacts,
    ) -> Result<NbestTokenCosts, Self::Error> {
        self.nbest_token_costs(token)
    }

    /// The part of `token`'s step costs that no predecessor changes: its
    /// unigram possibility and the two weights. A step from a predecessor's
    /// merged row applies [`NbestTokenCosts::costs`] to the row's item
    /// (`phonetic_lookup.h:646-697`, `phrase_lookup.cpp:310-336` at
    /// 074a2219). The n-best search asks at each step, as the pin's step
    /// reads the unigram and the facade total (`phonetic_lookup.h:650-651`,
    /// `:678-679`). The default prices nothing.
    ///
    /// # Errors
    /// Returns [`Self::Error`] when the model backend cannot read the token's
    /// unigram or the facade total.
    fn nbest_token_costs(&self, _token: &PhraseToken) -> Result<NbestTokenCosts, Self::Error> {
        Ok(NbestTokenCosts::ABSENT)
    }

    /// The merged row of one n-best predecessor, owned by this step.
    /// [`NbestSuccessors::Miss`] is a loaded miss of both grams; `None`
    /// is a model that reads no rows. In both cases the predecessor takes
    /// no bigram step.
    ///
    /// # Errors
    /// Propagates row-load and parse errors without caching a failed load.
    fn nbest_successors_in_row<'a>(
        &self,
        _prev: &PhraseToken,
        _row: &'a mut NbestStepRow,
    ) -> Result<Option<NbestSuccessors<'a>>, Self::Error> {
        Ok(None)
    }

    /// Ends the step that loaded `row`: the model may reuse the row's bytes
    /// for its next read. The default drops the row's contents.
    fn release_row(&self, _row: &mut NbestStepRow) {}

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
    fn amplification_lambda(&self) -> f32;
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

/// One token's step costs before any predecessor's row item: what
/// [`LanguageModel::nbest_token_costs`] returns. [`Self::costs`] completes
/// them for one item; [`Self::blended`] and [`Self::unigram`] are its two
/// branches.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NbestTokenCosts {
    /// Unweighted f64 `unigram_frequency / total`, IEEE division included.
    /// `None` when the token has no visible phrase item: neither branch
    /// steps (the pin's failing `get_phrase_item`).
    pub elem: Option<f64>,
    /// The f32 bigram weight λ.
    pub lambda: f32,
    /// `(1.0_f64 - lambda as f64) as f32`, the unigram weight.
    pub unigram_lambda: f32,
}

impl NbestTokenCosts {
    /// A token the model does not price.
    pub const ABSENT: Self = Self {
        elem: None,
        lambda: 0.0,
        unigram_lambda: 1.0,
    };

    /// The unigram branch: `elem`, absent below `DBL_EPSILON`
    /// (`phonetic_lookup.h:652`). NaN passes the `<` gate.
    #[must_use]
    pub fn unigram(&self) -> Option<f64> {
        let elem = self.elem?;
        if elem < f64::EPSILON {
            None
        } else {
            Some(elem)
        }
    }

    /// The bigram branch for one row item: `f64::from(lambda * bigram) +
    /// f64::from(unigram_lambda) * elem`, the first product in f32, absent
    /// when both possibilities are below their epsilons
    /// (`phonetic_lookup.h:680-693`).
    #[must_use]
    pub fn blended(&self, freq: u64, total: u64) -> Option<f64> {
        let elem = self.elem?;
        let bigram = freq as f32 / total as f32;
        if bigram < f32::EPSILON && elem < f64::EPSILON {
            None
        } else {
            let weighted = self.lambda * bigram;
            Some(f64::from(weighted) + f64::from(self.unigram_lambda) * elem)
        }
    }

    /// Both branches for one transition `(freq, total, present)`; a
    /// transition that is absent or not present leaves the unigram branch
    /// alone. No branch answers the default.
    #[must_use]
    pub fn costs(&self, transition: Option<(u64, u64, bool)>) -> NbestStepCosts {
        if self.elem.is_none() {
            return NbestStepCosts::default();
        }
        let unigram = self.unigram();
        let blended = match transition {
            Some((freq, total, true)) => self.blended(freq, total),
            _ => None,
        };
        if blended.is_none() && unigram.is_none() {
            return NbestStepCosts::default();
        }
        NbestStepCosts {
            blended,
            unigram,
            unigram_lambda: self.unigram_lambda,
        }
    }
}

#[cfg(test)]
mod tests;
