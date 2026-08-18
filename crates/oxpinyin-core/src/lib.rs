//! Pure pinyin algorithms: parsing, SegmentGraph, k-best search, scoring
//! traits. No I/O, no platform deps, no `unsafe`. Internal crate — the
//! supported public API is `oxpinyin-engine`.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod cost;
pub mod fixture;
pub mod graph;
pub mod kbest;
pub mod options;
mod parser;
mod scheme;
pub mod scoring;
mod syllables;
mod vocab;
mod zhuyin_map;

pub use options::{
    DYNAMIC_ADJUST, OptionBits, PINYIN_AMB_ALL, PINYIN_AMB_AN_ANG, PINYIN_AMB_C_CH,
    PINYIN_AMB_EN_ENG, PINYIN_AMB_F_H, PINYIN_AMB_G_K, PINYIN_AMB_IN_ING, PINYIN_AMB_L_N,
    PINYIN_AMB_L_R, PINYIN_AMB_S_SH, PINYIN_AMB_Z_ZH, PINYIN_CORRECT_ALL, PINYIN_CORRECT_GN_NG,
    PINYIN_CORRECT_IOU_IU, PINYIN_CORRECT_MG_NG, PINYIN_CORRECT_ON_ONG, PINYIN_CORRECT_UE_VE,
    PINYIN_CORRECT_UEI_UI, PINYIN_CORRECT_UEN_UN, PINYIN_CORRECT_V_U, PINYIN_INCOMPLETE,
};
pub use parser::{
    Completeness, FullPinyinParser, MAX_PARSE_RESULTS, ParseError, ParseResult, ParsedSyllable,
};
pub use scheme::{
    DoublePinyinKey, DoublePinyinParse, DoublePinyinParser, DoublePinyinScheme, ZhuyinKey,
    ZhuyinParse, ZhuyinParser, ZhuyinScheme,
};
pub use syllables::{
    FULL_PINYIN_SYLLABLE_COUNT, FULL_PINYIN_SYLLABLES, INCOMPLETE_PINYIN_KEY_COUNT,
    INCOMPLETE_PINYIN_KEYS, MAX_SYLLABLE_LEN, OPTION_ONLY_COMPLETE_SYLLABLE_COUNT,
    OPTION_ONLY_COMPLETE_SYLLABLES, canonical_complete_syllable, option_alias_canonical,
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
    fn lookup(&self, syllables: &[Self::Syllable]) -> Result<Vec<Self::Entry>, Self::Error>;

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
    fn phrase_prefix_exists(&self, _syllables: &[Self::Syllable]) -> Result<bool, Self::Error> {
        Ok(true)
    }

    /// Addon-facade lookup pass, empty for every existing implementor.
    ///
    /// W11: the default facade stays on [`Dictionary::lookup`]; the addon
    /// facade is a second pass so an addon token never has to be
    /// distinguished from `MERGED_DICTIONARY` by nibble alone
    /// (`docs/findings/phrase-union.md` §6.3). Defaulted so other
    /// implementors compile unchanged.
    fn lookup_addon(&self, _syllables: &[Self::Syllable]) -> Result<Vec<Self::Entry>, Self::Error> {
        Ok(Vec::new())
    }

    /// Addon-facade `SEARCH_CONTINUED` probe, `false` for every existing
    /// implementor.
    fn phrase_prefix_exists_addon(
        &self,
        _syllables: &[Self::Syllable],
    ) -> Result<bool, Self::Error> {
        Ok(false)
    }
}

impl<D: Dictionary + ?Sized> Dictionary for &D {
    type Syllable = D::Syllable;
    type Entry = D::Entry;
    type Error = D::Error;

    fn lookup(&self, syllables: &[Self::Syllable]) -> Result<Vec<Self::Entry>, Self::Error> {
        (**self).lookup(syllables)
    }

    fn phrase_prefix_exists(&self, syllables: &[Self::Syllable]) -> Result<bool, Self::Error> {
        (**self).phrase_prefix_exists(syllables)
    }

    fn lookup_addon(&self, syllables: &[Self::Syllable]) -> Result<Vec<Self::Entry>, Self::Error> {
        (**self).lookup_addon(syllables)
    }

    fn phrase_prefix_exists_addon(
        &self,
        syllables: &[Self::Syllable],
    ) -> Result<bool, Self::Error> {
        (**self).phrase_prefix_exists_addon(syllables)
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
    fn score(&self, history: &[Self::Token], token: &Self::Token) -> Result<Cost, Self::Error>;

    /// Records an accepted `token` after `history`.
    ///
    /// Learning-off callers omit this operation entirely.
    fn observe(&mut self, history: &[Self::Token], token: &Self::Token) -> Result<(), Self::Error>;
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
    fn score(
        &self,
        history: &[Self::Token],
        token: &Self::Token,
        edge_cost: Cost,
    ) -> Result<Cost, Self::Error>;

    /// The model's real unigram frequency of `token`, when the model carries
    /// one.
    ///
    /// `None` means the model exposes no frequency table at all; the engine
    /// then keeps its pre-frequency behaviour. `Some(0)` is a real table miss
    /// — the phrase has no frequency — and ranks below every counted phrase.
    ///
    /// Defaulted so the frozen implementors and any third-party model keep
    /// compiling unchanged (`docs/findings/core-trait-seam.md`: the seam grows
    /// by defaulted methods only).
    fn unigram_freq(&self, _token: &Self::Token) -> Result<Option<u64>, Self::Error> {
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

    /// Sum of the default-facade unigram table, when the model carries one.
    ///
    /// Used only to put addon frequencies on a comparable scale. Defaulted
    /// to `None` so existing implementors compile unchanged.
    fn unigram_total(&self) -> Result<Option<u64>, Self::Error> {
        Ok(None)
    }

    /// Addon-facade unigram of `token`, when an addon set is loaded.
    ///
    /// `None` means no addon table; `Some(0)` is a loaded-table miss.
    /// Defaulted so existing implementors compile unchanged
    /// (`docs/findings/phrase-union.md` §6.4).
    fn addon_unigram_freq(&self, _token: &Self::Token) -> Result<Option<u64>, Self::Error> {
        Ok(None)
    }

    /// Sum of loaded addon unigrams, when an addon set is loaded.
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
    fn nbest_step_costs(
        &self,
        _prev: &Self::Token,
        _token: &Self::Token,
    ) -> Result<NbestStepCosts, Self::Error> {
        Ok(NbestStepCosts::default())
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
    fn parse(&self, input: &[u8]) -> Result<Vec<Self::Parse>, Self::Error>;
}
