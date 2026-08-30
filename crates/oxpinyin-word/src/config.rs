//! Word-recognizer parameters (`trainer/lib/myconfig.py:135-189`).

/// `getMaximumCombineNumber` — the highest n-gram order (`myconfig.py:141-144`).
pub const MAX_COMBINE: usize = 7;
/// `getPruneMinimumOccurrence` — drop n-gram rows with `freq ≤ 1` after each
/// populate pass (`:146-147`).
pub const PRUNE_MINIMUM_OCCURRENCE: u64 = 1;
/// `getWordMinimumOccurrence` — a dictionary word needs at least this
/// unigram frequency to seed the partial-word threshold (`:149-150`).
pub const WORD_MINIMUM_OCCURRENCE: u64 = 3;
/// `getNgramMinimumOccurrence` — the merge stage only considers n-gram rows
/// with at least this frequency (`:152-153`).
pub const NGRAM_MINIMUM_OCCURRENCE: u64 = 9;
/// `getPartialWordThreshold` — the partial-word freq threshold position
/// (`:155-156`).
pub const PARTIAL_WORD_THRESHOLD: f64 = 0.50;
/// `getNewWordThreshold` — the new-word entropy threshold position
/// (`:158-159`).
pub const NEW_WORD_THRESHOLD: f64 = 0.60;
/// `getMinimumEntropy` — a dictionary word needs at least this entropy to
/// seed the new-word threshold (`:161-162`).
pub const MINIMUM_ENTROPY: f64 = 0.01;
/// `getMaximumIteration` — the partial-word discovery iteration cap
/// (`:164-165`).
pub const MAXIMUM_ITERATION: usize = 20;
/// `getDefaultPinyinTotalFrequency` — the per-word pinyin total the marks
/// are rescaled to (`:185-186`).
pub const DEFAULT_PINYIN_TOTAL: f64 = 100.0;
/// `getMinimumPinyinFrequency` — drop a marked pinyin below this frequency
/// (`:188-189`).
pub const MINIMUM_PINYIN_FREQUENCY: u64 = 3;

/// The word separator: a single space (`getWordSep`, `:138-139`).
pub const SEP: char = ' ';
/// `null_token`.
pub const NULL_TOKEN: u32 = 0;
