//! The scoring configuration.
//!
//! `docs/findings/scoring-spec.md` freezes the shape: a log-linear
//! combination of a language-model term and a small set of structural
//! features over the graph. Its **constants are provisional** and are not
//! claimed to be upstream's; the SPEC says why, and W3+W4 integration against
//! real tables is what will settle them.

use core::fmt::Display;

use crate::cost::UNKNOWN_COST;
use crate::graph::{Edge, EdgeKind};
use crate::kbest::EdgeCost;
use crate::{
    Cost, Dictionary, FULL_PINYIN_SYLLABLES, LanguageModel, PhraseEntry, PhraseToken,
    SYLLABLE_KEY_COUNT, SyllableKey,
};

/// Denominator of the log-linear weights, so they can be fractional without
/// floating point.
///
/// [`ScoringConfig::lm_weight`] is expressed over this: 100 is a weight of
/// 1.0, 50 is 0.5, 0 removes the language-model term entirely. Integer
/// numerator over a fixed denominator is how a fractional weight is
/// represented here, because floating point is barred from the cost path —
/// `f64` arithmetic is not required to be bit-identical across platforms, and
/// constitution item 6 makes engine output a pure function of (input, user
/// state, config) on every operating system.
///
/// The form is normative and frozen in `docs/findings/scoring-spec.md`; the
/// weight *values* it scales are provisional. See [`ScoringConfig::default`].
pub const WEIGHT_SCALE: i64 = 100;

/// Why scoring failed.
///
/// Backend failures arrive as text because the frozen `Dictionary` and
/// `LanguageModel` seams leave their `Error` types unbounded.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ScoringError {
    /// The dictionary backend failed.
    Dictionary(String),
    /// The language model backend failed.
    LanguageModel(String),
}

impl core::fmt::Display for ScoringError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Dictionary(message) => write!(formatter, "dictionary error: {message}"),
            Self::LanguageModel(message) => write!(formatter, "language model error: {message}"),
        }
    }
}

impl std::error::Error for ScoringError {}

/// The weights of the log-linear score.
///
/// Every value is a cost on the fixed-point negative-log₂ scale of
/// [`crate::cost`], where [`crate::cost::COST_PER_BIT`] is one bit of
/// surprisal. **Provisional**: see `docs/findings/scoring-spec.md`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScoringConfig {
    /// Weight of the language-model term, over [`WEIGHT_SCALE`].
    pub lm_weight: i64,
    /// Charged for an [`EdgeKind::Exact`] edge.
    pub exact_penalty: Cost,
    /// Charged for an [`EdgeKind::Segmentation`] edge.
    pub segmentation_penalty: Cost,
    /// Charged for an [`EdgeKind::Incomplete`] edge.
    ///
    /// Measured constraint: it must stay **below**
    /// [`ScoringConfig::phrase_key_bonus`]. The pin offers `你好` ahead of
    /// `你` for `nih`, and `中国` ahead of `中` for `zhongg`; a phrase that
    /// covers one more key through an initial-only edge therefore has to win.
    pub incomplete_penalty: Cost,
    /// Credited for each key a phrase covers beyond the first.
    pub phrase_key_bonus: Cost,
    /// Largest number of complete-key sequences one incomplete key sequence
    /// may expand into.
    pub expansion_limit: usize,
}

/// The provisional weights.
///
/// **Every number below is provisional.** `docs/findings/scoring-spec.md`
/// freezes the functional form, the cost scale and the sign convention as
/// normative, and explicitly does *not* freeze these values: the pinned
/// oracle exposes candidate order at its public API and never a probability,
/// and its real probabilities live in a non-redistributable model archive, so
/// the captures give inequalities rather than magnitudes.
///
/// The three inequalities the captures do prove, each asserted by a test:
///
/// - `phrase_key_bonus > 0` — the pin lists `你好` before `你`;
/// - `segmentation_penalty > 0` — it lists `方案` (`fang` + `an`) before
///   `反感` (`fan` + `gan`);
/// - `incomplete_penalty < phrase_key_bonus` — it lists `你好` before `你`
///   for `nih`.
///
/// These values are one point in the space those allow. Settling them needs
/// a cross-check against W3's real tables; until then, do not quote a number
/// from here as upstream's.
impl Default for ScoringConfig {
    fn default() -> Self {
        Self {
            lm_weight: WEIGHT_SCALE,
            exact_penalty: 0,
            segmentation_penalty: 500,
            incomplete_penalty: 1_000,
            phrase_key_bonus: 2_000,
            expansion_limit: 64,
        }
    }
}

impl ScoringConfig {
    /// Cost charged for one edge of this kind.
    #[must_use]
    pub const fn edge_penalty(&self, kind: EdgeKind) -> Cost {
        match kind {
            EdgeKind::Exact => self.exact_penalty,
            EdgeKind::Segmentation => self.segmentation_penalty,
            _ => self.incomplete_penalty,
        }
    }

    /// Credit for a phrase covering `keys` keys.
    #[must_use]
    pub const fn coverage_bonus(&self, keys: usize) -> Cost {
        let extra = if keys == 0 { 0 } else { keys as i64 - 1 };
        self.phrase_key_bonus.saturating_mul(extra)
    }

    /// Applies [`ScoringConfig::lm_weight`] to a model cost.
    #[must_use]
    pub const fn weigh(&self, model_cost: Cost) -> Cost {
        model_cost.saturating_mul(self.lm_weight) / WEIGHT_SCALE
    }
}

/// Scores graph edges and dictionary phrases against a dictionary and model.
///
/// Per-key costs are computed once, at construction, so [`EdgeCost`] is a
/// table lookup that cannot fail — which is what lets the k-best sweep stay
/// infallible while the backends behind it are not.
#[derive(Clone, Debug)]
pub struct Scorer<'a, D, L> {
    config: ScoringConfig,
    dictionary: &'a D,
    model: &'a L,
    key_costs: Vec<Cost>,
}

impl<'a, D, L> Scorer<'a, D, L>
where
    D: Dictionary<Syllable = SyllableKey, Entry = PhraseEntry>,
    D::Error: Display,
    L: LanguageModel<Token = PhraseToken>,
    L::Error: Display,
{
    /// Builds a scorer and precomputes the per-key cost table.
    ///
    /// # Errors
    ///
    /// Returns [`ScoringError`] when a backend fails while the table is built.
    pub fn new(
        config: ScoringConfig,
        dictionary: &'a D,
        model: &'a L,
    ) -> Result<Self, ScoringError> {
        let key_costs = key_cost_table(dictionary, model)?;
        Ok(Self::with_key_costs(config, dictionary, model, key_costs))
    }

    /// Builds a scorer over an already computed key-cost table.
    ///
    /// A caller that scores repeatedly — a session, once per keystroke —
    /// computes the table once with [`key_cost_table`] and reuses it, rather
    /// than making the backend answer 428 lookups on every key press.
    #[must_use]
    pub const fn with_key_costs(
        config: ScoringConfig,
        dictionary: &'a D,
        model: &'a L,
        key_costs: Vec<Cost>,
    ) -> Self {
        Self {
            config,
            dictionary,
            model,
            key_costs,
        }
    }

    /// The weights in force.
    #[must_use]
    pub const fn config(&self) -> &ScoringConfig {
        &self.config
    }

    /// The precomputed cost of the cheapest phrase spelled by `key` alone.
    #[must_use]
    pub fn key_cost(&self, key: SyllableKey) -> Cost {
        self.key_costs
            .get(key.index())
            .copied()
            .unwrap_or(UNKNOWN_COST)
    }

    /// Cost of `entry` covering `keys` whose edges are `kinds`, after
    /// `history`.
    ///
    /// The structural penalties are handed to the model as its `edge_cost`,
    /// which is the seam `core-trait-seam.md` froze for exactly this. The
    /// coverage credit is applied afterwards, so it is a property of the
    /// decoder's preference rather than of the model.
    ///
    /// # Errors
    ///
    /// Returns [`ScoringError::LanguageModel`] when the model fails.
    pub fn phrase_cost(
        &self,
        history: &[PhraseToken],
        entry: &PhraseEntry,
        keys: usize,
        kinds: &[EdgeKind],
    ) -> Result<Cost, ScoringError> {
        let structural = kinds.iter().fold(0_i64, |total, kind| {
            total.saturating_add(self.config.edge_penalty(*kind))
        });

        let combined = self
            .model
            .score(history, &entry.token(), structural)
            .map_err(|error| ScoringError::LanguageModel(error.to_string()))?;

        Ok(self
            .config
            .weigh(combined)
            .saturating_sub(self.config.coverage_bonus(keys)))
    }

    /// The phrases that can spell `keys`, cheapest first.
    ///
    /// An incomplete key stands for every complete key it is a proper prefix
    /// of, which is what the pin does: `nih` offers `你好`, `霓虹`, `拟合`.
    /// Expansion is bounded by [`ScoringConfig::expansion_limit`]; beyond it
    /// the sequence yields nothing rather than a combinatorial blow-up.
    ///
    /// # Errors
    ///
    /// Returns [`ScoringError`] when a backend fails.
    pub fn rank_phrases(
        &self,
        history: &[PhraseToken],
        keys: &[SyllableKey],
        kinds: &[EdgeKind],
    ) -> Result<Vec<(PhraseEntry, Cost)>, ScoringError> {
        let mut ranked: Vec<(PhraseEntry, Cost)> = Vec::new();

        for sequence in expand_keys(keys, self.config.expansion_limit) {
            let entries = self
                .dictionary
                .lookup(&sequence)
                .map_err(|error| ScoringError::Dictionary(error.to_string()))?;
            for entry in entries {
                let cost = self.phrase_cost(history, &entry, keys.len(), kinds)?;
                // Linear scan rather than a set: bounded by expansion_limit
                // (default 64) sequences × the entries each spells, and the
                // caller asks per key-prefix, so `ranked` stays short. A hash
                // set here would also have to be ordered to keep ranking
                // deterministic.
                if ranked.iter().any(|(seen, _)| seen.text() == entry.text()) {
                    continue;
                }
                ranked.push((entry, cost));
            }
        }

        ranked.sort_by_key(|(_, cost)| *cost);
        Ok(ranked)
    }
}

impl<D, L> EdgeCost for Scorer<'_, D, L> {
    fn cost(&self, _previous: Option<&Edge>, edge: &Edge) -> Cost {
        self.key_costs
            .get(edge.key().index())
            .copied()
            .unwrap_or(UNKNOWN_COST)
            .saturating_add(self.config.edge_penalty(edge.kind()))
    }
}

/// Cost of the cheapest phrase each frozen key spells on its own.
///
/// Indexed by [`SyllableKey::index`], so a scorer's per-edge cost is a slice
/// lookup rather than a backend round trip.
///
/// # Errors
///
/// Returns [`ScoringError`] when a backend fails.
pub fn key_cost_table<D, L>(dictionary: &D, model: &L) -> Result<Vec<Cost>, ScoringError>
where
    D: Dictionary<Syllable = SyllableKey, Entry = PhraseEntry>,
    D::Error: Display,
    L: LanguageModel<Token = PhraseToken>,
    L::Error: Display,
{
    let mut key_costs = Vec::with_capacity(SYLLABLE_KEY_COUNT);
    for index in 0..SYLLABLE_KEY_COUNT {
        let Some(key) = SyllableKey::from_index(index) else {
            key_costs.push(UNKNOWN_COST);
            continue;
        };

        let entries = dictionary
            .lookup(&[key])
            .map_err(|error| ScoringError::Dictionary(error.to_string()))?;

        let mut best = UNKNOWN_COST;
        for entry in &entries {
            let cost = model
                .score(&[], &entry.token(), 0)
                .map_err(|error| ScoringError::LanguageModel(error.to_string()))?;
            best = best.min(cost);
        }
        key_costs.push(best);
    }

    Ok(key_costs)
}

/// Every complete-key sequence `keys` can stand for.
///
/// A complete key stands for itself. An incomplete key stands for each
/// complete syllable it is a proper prefix of, in frozen inventory order.
///
/// # Known gap: long incomplete-key sequences yield nothing
///
/// The expansion is a Cartesian product, and each initial-only key multiplies
/// it by the number of syllables that key prefixes — 9 (`j`) to 36 (`z`) over
/// the frozen inventory. When the product would exceed `limit`, this returns
/// an **empty vector** — not a truncated one — so a caller cannot mistake a
/// subset for the whole answer.
///
/// The consequence is worth stating plainly rather than hiding behind
/// "bounded": a key sequence with two or more initials gets **no candidates
/// from the dictionary at all**, not a shorter list. The smallest possible
/// pair is 9 × 9 = 81, already past the default `limit` of 64, so in practice
/// the ceiling is one initial per sequence. `zzzzzzzz` decodes to eight `z`
/// keys, expands to 36⁸, and offers nothing; the session falls back to
/// handing the raw input back.
///
/// That is deliberate for W4 — a wrong candidate list is worse than none, and
/// the pin's own behaviour here is not something the captures pin down — but
/// it is a gap, not a design endpoint. Closing it wants a dictionary that can
/// answer a prefix query directly instead of being asked one exact key
/// sequence at a time, which is a W3 loader concern.
#[must_use]
pub fn expand_keys(keys: &[SyllableKey], limit: usize) -> Vec<Vec<SyllableKey>> {
    let alternatives: Vec<Vec<SyllableKey>> = keys.iter().map(|key| completions(*key)).collect();

    let mut product = 1_usize;
    for choices in &alternatives {
        if choices.is_empty() {
            return Vec::new();
        }
        product = match product.checked_mul(choices.len()) {
            Some(product) if product <= limit => product,
            _ => return Vec::new(),
        };
    }

    let mut sequences: Vec<Vec<SyllableKey>> = vec![Vec::with_capacity(keys.len())];
    for choices in alternatives {
        let mut next = Vec::with_capacity(sequences.len() * choices.len());
        for prefix in &sequences {
            for choice in &choices {
                // bounded by expansion_limit (default 64)
                let mut extended = prefix.clone();
                extended.push(*choice);
                next.push(extended);
            }
        }
        sequences = next;
    }
    sequences
}

/// The complete keys one key stands for.
fn completions(key: SyllableKey) -> Vec<SyllableKey> {
    if key.completeness() == crate::Completeness::Complete {
        return vec![key];
    }

    let prefix = key.text();
    FULL_PINYIN_SYLLABLES
        .iter()
        .filter(|syllable| syllable.len() > prefix.len() && syllable.starts_with(prefix))
        .filter_map(|syllable| SyllableKey::from_text(syllable))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Scorer, ScoringConfig, ScoringError, WEIGHT_SCALE, expand_keys};
    use crate::cost::{COST_PER_BIT, UNKNOWN_COST};
    use crate::fixture::{FixtureDictionary, FixtureLanguageModel};
    use crate::graph::EdgeKind;
    use crate::kbest::{EdgeCost, k_best};
    use crate::{Cost, PhraseToken, SyllableKey, graph::SegmentGraph};

    const VOCAB: &str = include_str!("../../../fixtures/w4/mini-vocab.txt");
    const BIGRAM: &str = include_str!("../../../fixtures/w4/mini-bigram.txt");

    fn keys(text: &str) -> Vec<SyllableKey> {
        text.split(',')
            .map(|key| SyllableKey::from_text(key).expect("frozen key"))
            .collect()
    }

    fn backends() -> (FixtureDictionary, FixtureLanguageModel) {
        (
            FixtureDictionary::parse(VOCAB).expect("committed fixture"),
            FixtureLanguageModel::parse(VOCAB, BIGRAM).expect("committed fixtures"),
        )
    }

    #[test]
    fn the_default_weights_are_the_frozen_provisional_ones() {
        let config = ScoringConfig::default();
        assert_eq!(config.lm_weight, WEIGHT_SCALE);
        assert_eq!(config.edge_penalty(EdgeKind::Exact), 0);
        assert_eq!(config.edge_penalty(EdgeKind::Segmentation), 500);
        assert_eq!(config.edge_penalty(EdgeKind::Incomplete), 1_000);
        assert_eq!(config.coverage_bonus(1), 0);
        assert_eq!(config.coverage_bonus(2), 2_000);
        assert_eq!(config.coverage_bonus(3), 4_000);
        assert_eq!(config.coverage_bonus(0), 0);
    }

    #[test]
    fn the_weight_scales_the_model_term() {
        let mut config = ScoringConfig::default();
        assert_eq!(config.weigh(3 * COST_PER_BIT), 3 * COST_PER_BIT);
        config.lm_weight = WEIGHT_SCALE / 2;
        assert_eq!(config.weigh(3 * COST_PER_BIT), 3 * COST_PER_BIT / 2);
        config.lm_weight = 0;
        assert_eq!(config.weigh(UNKNOWN_COST), 0);
    }

    #[test]
    fn an_incomplete_key_stands_for_what_it_prefixes() {
        let expanded = expand_keys(&keys("ni,h"), 64);
        assert!(expanded.len() > 10, "h prefixes many syllables");
        assert!(
            expanded
                .iter()
                .all(|sequence| sequence[0] == keys("ni")[0] && sequence.len() == 2)
        );
        assert!(expanded.contains(&keys("ni,hao")));
        assert!(expanded.contains(&keys("ni,hong")));

        assert_eq!(expand_keys(&keys("ni,hao"), 64), vec![keys("ni,hao")]);
        assert_eq!(expand_keys(&[], 64), vec![Vec::new()]);
    }

    #[test]
    fn expansion_beyond_the_limit_yields_nothing_rather_than_a_subset() {
        assert!(expand_keys(&keys("h,h,h,h"), 64).is_empty());
        assert!(expand_keys(&keys("ni,h"), 2).is_empty());
    }

    #[test]
    fn the_edge_cost_charges_the_kind_and_the_key() {
        let (dictionary, model) = backends();
        let scorer =
            Scorer::new(ScoringConfig::default(), &dictionary, &model).expect("fixtures load");

        let graph = SegmentGraph::build(b"xian").expect("short input");
        let mut by_key = Vec::new();
        for edge in graph.edges() {
            by_key.push((edge.key().text(), edge.kind(), scorer.cost(None, edge)));
        }

        let exact = by_key
            .iter()
            .find(|(key, ..)| *key == "xian")
            .expect("xian edge");
        let split = by_key
            .iter()
            .find(|(key, ..)| *key == "xi")
            .expect("xi edge");
        assert_eq!(split.1, EdgeKind::Segmentation);
        assert_eq!(
            split.2 - scorer.key_cost(keys("xi")[0]),
            500,
            "a segmentation edge carries its penalty"
        );
        assert_eq!(exact.2, scorer.key_cost(keys("xian")[0]));

        let unknown = by_key.iter().find(|(key, ..)| *key == "x").expect("x edge");
        assert!(
            unknown.2 >= UNKNOWN_COST,
            "an initial with no phrase of its own costs the floor plus its penalty"
        );
    }

    #[test]
    fn worked_example_a_longer_phrase_beats_its_first_syllable() {
        // The pin's list for nihao opens 你好 then 你; for zhongguoren it is
        // 中国人, 中国, 中. Coverage credit is what reproduces that.
        let (dictionary, model) = backends();
        let scorer =
            Scorer::new(ScoringConfig::default(), &dictionary, &model).expect("fixtures load");

        let whole = scorer
            .rank_phrases(&[], &keys("ni,hao"), &[EdgeKind::Exact; 2])
            .expect("ranking cannot fail here");
        let first = scorer
            .rank_phrases(&[], &keys("ni"), &[EdgeKind::Exact])
            .expect("ranking cannot fail here");
        assert_eq!(whole[0].0.text(), "你好");
        assert_eq!(first[0].0.text(), "你");
        assert!(
            whole[0].1 < first[0].1,
            "你好 covers two keys and must outrank 你"
        );

        let three = scorer
            .rank_phrases(&[], &keys("zhong,guo,ren"), &[EdgeKind::Exact; 3])
            .expect("ranking cannot fail here");
        let two = scorer
            .rank_phrases(&[], &keys("zhong,guo"), &[EdgeKind::Exact; 2])
            .expect("ranking cannot fail here");
        assert_eq!(three[0].0.text(), "中国人");
        assert!(three[0].1 < two[0].1);
    }

    #[test]
    fn worked_example_the_segmentation_penalty_orders_fangan() {
        // Measured: the pin ranks 方案 (fang + an, both exact) ahead of
        // 反感 (fan + gan, where fan is the shorter split at position 0).
        let (dictionary, model) = backends();
        let scorer =
            Scorer::new(ScoringConfig::default(), &dictionary, &model).expect("fixtures load");

        let fangan = scorer
            .rank_phrases(&[], &keys("fang,an"), &[EdgeKind::Exact, EdgeKind::Exact])
            .expect("ranking cannot fail here");
        let fangan_alt = scorer
            .rank_phrases(
                &[],
                &keys("fan,gan"),
                &[EdgeKind::Segmentation, EdgeKind::Exact],
            )
            .expect("ranking cannot fail here");

        assert_eq!(fangan[0].0.text(), "方案");
        assert_eq!(fangan_alt[0].0.text(), "反感");
        assert!(
            fangan[0].1 < fangan_alt[0].1,
            "方案 {} must beat 反感 {}",
            fangan[0].1,
            fangan_alt[0].1
        );
    }

    #[test]
    fn worked_example_an_incomplete_key_reaches_the_pins_phrases() {
        // nih: the pin offers 你好, 霓虹, 拟合 — two-key phrases whose second
        // syllable starts with h.
        let (dictionary, model) = backends();
        let scorer =
            Scorer::new(ScoringConfig::default(), &dictionary, &model).expect("fixtures load");

        let ranked = scorer
            .rank_phrases(&[], &keys("ni,h"), &[EdgeKind::Exact, EdgeKind::Incomplete])
            .expect("ranking cannot fail here");
        let texts: Vec<&str> = ranked.iter().map(|(entry, _)| entry.text()).collect();

        assert_eq!(texts[0], "你好");
        for wanted in ["霓虹", "拟合", "你和", "你很", "你会"] {
            assert!(texts.contains(&wanted), "missing {wanted}");
        }
    }

    #[test]
    fn a_bigram_context_changes_the_cost() {
        let (dictionary, model) = backends();
        let scorer =
            Scorer::new(ScoringConfig::default(), &dictionary, &model).expect("fixtures load");

        let alone = scorer
            .rank_phrases(&[], &keys("zhong,guo"), &[EdgeKind::Exact; 2])
            .expect("ranking cannot fail here");
        let after = scorer
            .rank_phrases(
                &[PhraseToken::new(11)],
                &keys("zhong,guo"),
                &[EdgeKind::Exact; 2],
            )
            .expect("ranking cannot fail here");

        assert_eq!(alone[0].0.text(), "中国");
        assert_eq!(after[0].0.text(), "中国");
        assert!(after[0].1 < alone[0].1, "你好 -> 中国 is a known bigram");
    }

    #[test]
    fn the_scorer_drives_k_best() {
        let (dictionary, model) = backends();
        let scorer =
            Scorer::new(ScoringConfig::default(), &dictionary, &model).expect("fixtures load");
        let graph = SegmentGraph::build(b"nihao").expect("short input");

        let paths = k_best(&graph, &scorer, 4).expect("k is small");
        assert!(!paths.is_empty());
        let spelled: Vec<&str> = paths[0]
            .edges()
            .iter()
            .map(|id| graph.edge(*id).expect("id from this graph").key().text())
            .collect();
        assert_eq!(spelled, ["ni", "hao"], "the phrase-bearing split wins");
    }

    #[test]
    fn scoring_is_deterministic_and_never_panics_on_an_empty_backend() {
        let empty = FixtureDictionary::default();
        let model = FixtureLanguageModel::default();
        let scorer = Scorer::new(ScoringConfig::default(), &empty, &model).expect("empty loads");

        assert_eq!(scorer.key_cost(keys("ni")[0]), UNKNOWN_COST);
        let ranked = scorer
            .rank_phrases(&[], &keys("ni,hao"), &[EdgeKind::Exact; 2])
            .expect("an empty dictionary is not an error");
        assert!(ranked.is_empty());

        let graph = SegmentGraph::build(b"nihao").expect("short input");
        let first: Vec<Cost> = graph.edges().iter().map(|e| scorer.cost(None, e)).collect();
        let second: Vec<Cost> = graph.edges().iter().map(|e| scorer.cost(None, e)).collect();
        assert_eq!(first, second);
    }

    #[test]
    fn a_failing_backend_is_reported_not_swallowed() {
        #[derive(Debug)]
        struct Broken;

        impl crate::Dictionary for Broken {
            type Entry = crate::PhraseEntry;
            type Error = &'static str;
            type Syllable = SyllableKey;

            fn lookup(&self, _: &[SyllableKey]) -> Result<Vec<crate::PhraseEntry>, &'static str> {
                Err("closed")
            }
        }

        let model = FixtureLanguageModel::default();
        assert_eq!(
            Scorer::new(ScoringConfig::default(), &Broken, &model)
                .expect_err("the dictionary is broken"),
            ScoringError::Dictionary("closed".to_owned())
        );
    }
}
