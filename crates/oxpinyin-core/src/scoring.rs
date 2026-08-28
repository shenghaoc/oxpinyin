//! The scoring configuration.
//!
//! `docs/findings/scoring-spec.md` freezes the shape: a log-linear
//! combination of a language-model term and a small set of structural
//! features over the graph. Its **constants are provisional** and are not
//! claimed to be upstream's; the SPEC says why, and W3+W4 integration against
//! real tables is what will settle them.

use core::fmt::Display;

use smallvec::SmallVec;

use crate::cost::UNKNOWN_COST;
use crate::graph::{Edge, EdgeKind};
use crate::kbest::EdgeCost;
use crate::{
    Cost, Dictionary, FULL_PINYIN_SYLLABLES, LanguageModel, PhraseEntry, PhraseToken,
    SYLLABLE_KEY_COUNT, SyllableKey,
};

/// One expanded key sequence. Phrase length is capped at 16, so this stays
/// on the stack.
pub type ExpandedKeys = SmallVec<[SyllableKey; 16]>;

/// Completions of one (possibly incomplete) key. The densest initial is 26
/// syllables (`l`).
type KeyCompletions = SmallVec<[SyllableKey; 32]>;

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
/// Settled against the full W2 corpus + exported tables by the parity-climb
/// constant sweep (`docs/findings/scoring-constant-sweep.md`). Magnitudes
/// are still not upstream's; they maximise measured top-1 under the frozen
/// functional form.
impl Default for ScoringConfig {
    fn default() -> Self {
        Self {
            lm_weight: WEIGHT_SCALE,
            exact_penalty: 0,
            // Higher than the first provisional 500: favours exact splits.
            segmentation_penalty: 750,
            // Just below phrase_key_bonus so nih still prefers 你好 over 你.
            incomplete_penalty: 999,
            // Lower than the first provisional 2_000: less over-reward of
            // long phrases relative to the pin's first candidate.
            phrase_key_bonus: 1_000,
            // Yield-nothing bound for the pre-frequency rank_phrases path.
            // The window scan has its own, larger bound
            // (`SCAN_EXPANSION_LIMIT` in oxpinyin-engine) and does not read
            // this value.
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
    /// An incomplete key stands for every complete key that shares its
    /// phonetic initial, which is what the pin does: `nih` offers `你好`,
    /// `霓虹`, `拟合`.
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
                .lookup(sequence.as_slice())
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
/// complete syllable with the same phonetic initial, in frozen inventory
/// order.
///
/// # Known gap: long incomplete-key sequences yield nothing
///
/// The expansion is a Cartesian product, and each initial-only key multiplies
/// it by the number of syllables with that phonetic initial — 9 (`w`) to 26
/// (`l`) over the frozen inventory. When the product would exceed `limit`,
/// this returns
/// an **empty vector** — not a truncated one — so a caller cannot mistake a
/// subset for the whole answer.
///
/// The consequence is worth stating plainly rather than hiding behind
/// "bounded": a key sequence with two or more initials gets **no candidates
/// from the dictionary at all**, not a shorter list. The smallest possible
/// pair is 9 × 9 = 81, already past the default `limit` of 64, so in practice
/// the ceiling is one initial per sequence. `zzzzzzzz` decodes to eight `z`
/// keys, expands to 17⁸, and offers nothing; the session falls back to
/// handing the raw input back.
///
/// That is deliberate for W4 — a wrong candidate list is worse than none, and
/// the pin's own behaviour here is not something the captures pin down — but
/// it is a gap, not a design endpoint. Closing it wants a dictionary that can
/// answer a prefix query directly instead of being asked one exact key
/// sequence at a time, which is a W3 loader concern.
///
/// Each sequence is a stack [`ExpandedKeys`] (phrase length ≤ 16).
#[must_use]
pub fn expand_keys(keys: &[SyllableKey], limit: usize) -> Vec<ExpandedKeys> {
    let alternatives: SmallVec<[KeyCompletions; 16]> =
        keys.iter().map(|key| completions(*key)).collect();

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

    let mut sequences: Vec<ExpandedKeys> = vec![ExpandedKeys::new()];
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

/// The complete keys one initial-only key stands for.
///
/// An incomplete key `K` stands for every complete syllable whose
/// [`phonetic_initial`] is `K`. That is the pinned `m_initial` index:
/// `n` does not reach `ng`, and `z`/`c`/`s` do not reach `zh`/`ch`/`sh`.
fn completions(key: SyllableKey) -> KeyCompletions {
    if key.completeness() == crate::Completeness::Complete {
        let mut out = KeyCompletions::new();
        out.push(key);
        return out;
    }

    let initial = key.text();
    FULL_PINYIN_SYLLABLES
        .iter()
        .filter(|syllable| crate::phonetic_initial(syllable) == Some(initial))
        .filter_map(|syllable| SyllableKey::from_text(syllable))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{ScoringConfig, WEIGHT_SCALE, expand_keys};
    use crate::SyllableKey;
    use crate::cost::{COST_PER_BIT, UNKNOWN_COST};
    use crate::graph::EdgeKind;

    fn keys(text: &str) -> Vec<SyllableKey> {
        text.split(',')
            .map(|key| SyllableKey::from_text(key).expect("frozen key"))
            .collect()
    }
    #[test]
    fn the_default_weights_are_the_swept_ones() {
        let config = ScoringConfig::default();
        assert_eq!(config.lm_weight, WEIGHT_SCALE);
        assert_eq!(config.edge_penalty(EdgeKind::Exact), 0);
        assert_eq!(config.edge_penalty(EdgeKind::Segmentation), 750);
        assert_eq!(config.edge_penalty(EdgeKind::Incomplete), 999);
        assert_eq!(config.coverage_bonus(1), 0);
        assert_eq!(config.coverage_bonus(2), 1_000);
        assert_eq!(config.coverage_bonus(3), 2_000);
        assert_eq!(config.coverage_bonus(0), 0);
        // Capture inequality 3: incomplete must stay strictly below the bonus.
        assert!(config.incomplete_penalty < config.phrase_key_bonus);
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
    fn an_incomplete_key_expands_by_phonetic_initial() {
        // `n` reaches every N-initial syllable and never the zero-initial
        // spelling `ng`; `z`/`c`/`s` stop at their initial and exclude the
        // retroflex `zh`/`ch`/`sh` initials.
        for (initial, included, excluded) in [
            ("n", &["na", "nei", "ni", "nv"][..], &["ng"][..]),
            ("z", &["za", "zeng", "zuo"][..], &["zha", "zhong"][..]),
            ("c", &["ca", "ceng", "cuo"][..], &["cha", "cheng"][..]),
            ("s", &["sa", "seng", "suo"][..], &["sha", "sheng"][..]),
            ("zh", &["zha", "zhong"][..], &["za", "zeng"][..]),
            ("ch", &["cha", "cheng"][..], &["ca", "ceng"][..]),
            ("sh", &["sha", "sheng"][..], &["sa", "seng"][..]),
        ] {
            let expanded = expand_keys(&keys(initial), 4096);
            for syllable in included {
                assert!(
                    expanded
                        .iter()
                        .any(|sequence| sequence.as_slice() == keys(syllable).as_slice()),
                    "{initial} must include {syllable}"
                );
            }
            for syllable in excluded {
                assert!(
                    !expanded
                        .iter()
                        .any(|sequence| sequence.as_slice() == keys(syllable).as_slice()),
                    "{initial} must exclude {syllable}"
                );
            }
        }
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
        assert!(
            expanded
                .iter()
                .any(|sequence| sequence.as_slice() == keys("ni,hao").as_slice())
        );
        assert!(
            expanded
                .iter()
                .any(|sequence| sequence.as_slice() == keys("ni,hong").as_slice())
        );

        let complete = expand_keys(&keys("ni,hao"), 64);
        assert_eq!(complete.len(), 1);
        assert_eq!(complete[0].as_slice(), keys("ni,hao").as_slice());
        let empty = expand_keys(&[], 64);
        assert_eq!(empty.len(), 1);
        assert!(empty[0].is_empty());
    }

    #[test]
    fn expansion_beyond_the_limit_yields_nothing_rather_than_a_subset() {
        assert!(expand_keys(&keys("h,h,h,h"), 64).is_empty());
        assert!(expand_keys(&keys("ni,h"), 2).is_empty());
    }
}
