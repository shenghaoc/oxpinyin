//! Integer seed arithmetic for user-store learning.
//!
//! These are the exact update constants and formulae pinned in
//! `docs/findings/user-store.md` §2 (derived there from libpinyin's
//! `train_result3`, `phonetic_lookup.h:844-936`). Everything is `u64`
//! integer maths — no floats appear anywhere in the count path, matching the
//! W9 counter discipline.
//!
//! The reproduction target is *values*, so the constants are written in the
//! same factored form as the source (`23 * 3`, `23 * 15 * 64`) to keep the
//! provenance visible.

/// First-selection seed: the increment applied the first time a token is
/// recorded after a given predecessor. `23 * 3` in the pinned source (§2).
pub const INITIAL_SEED: u64 = 23 * 3; // 69

/// Multiplier applied to the prior stored count on each reselection (§2).
pub const EXPAND_FACTOR: u64 = 2;

/// Upper bound on a single reselection seed. `23 * 15 * 64` in the source (§2).
pub const CEILING_SEED: u64 = 23 * 15 * 64; // 22080

/// Multiplier turning a bigram seed into the phrase-index unigram delta (§2).
pub const UNIGRAM_FACTOR: u64 = 7;

/// Training seed for a `(last_token -> token)` selection (the `pinyin_train`
/// path, §2).
///
/// `prev_count` is the pair's current stored bigram count, or `None` when the
/// pair has never been recorded:
///
/// - unseen pair -> [`INITIAL_SEED`] (69);
/// - seen pair -> `min(max(prev_count, INITIAL_SEED) * EXPAND_FACTOR, CEILING_SEED)`.
///
/// Note the growth is not a literal doubling of the *count*: because the store
/// adds this seed to `prev_count`, the stored count roughly triples per
/// reselection (`new = prev + 2*prev`). The `× 2` is `EXPAND_FACTOR` applied to
/// `prev_count`, exactly as §2 specifies. Seeds run 69, 138, 414, 1242, …,
/// clamping to 22080.
#[must_use]
pub fn training_seed(prev_count: Option<u64>) -> u64 {
    match prev_count {
        None => INITIAL_SEED,
        Some(freq) => freq
            .max(INITIAL_SEED)
            .saturating_mul(EXPAND_FACTOR)
            .min(CEILING_SEED),
    }
}

/// Flat seed for an accepted *predicted* candidate (the
/// `pinyin_choose_predicted_candidate` path, §2): always [`INITIAL_SEED`],
/// never the reselection expansion.
#[must_use]
pub const fn predicted_seed() -> u64 {
    INITIAL_SEED
}

/// Phrase-index unigram delta derived from a bigram `seed` (§2: the unigram
/// frequency rises by `seed * 7`).
#[must_use]
pub const fn unigram_delta(seed: u64) -> u64 {
    seed.saturating_mul(UNIGRAM_FACTOR)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_match_pinned_spec() {
        // docs/findings/user-store.md §2 constants table.
        assert_eq!(INITIAL_SEED, 69);
        assert_eq!(EXPAND_FACTOR, 2);
        assert_eq!(CEILING_SEED, 22080);
        assert_eq!(UNIGRAM_FACTOR, 7);
    }

    #[test]
    fn first_selection_is_initial_seed() {
        assert_eq!(training_seed(None), 69);
    }

    #[test]
    fn second_selection_is_138() {
        // After the first selection the stored count is 69, so the next seed
        // is max(69, 69) * 2 = 138.
        assert_eq!(training_seed(Some(69)), 138);
    }

    #[test]
    fn reselection_sequence_matches_pinned_formula() {
        // Accumulate exactly as the store does: new_count = prev + seed.
        let mut count = 0u64;
        let mut seeds = Vec::new();
        for _ in 0..8 {
            let prev = (count != 0).then_some(count);
            let seed = training_seed(prev);
            seeds.push(seed);
            count += seed;
        }
        // 69, 138, 414, 1242, 3726, 11178, then clamped at 22080.
        assert_eq!(seeds, vec![69, 138, 414, 1242, 3726, 11178, 22080, 22080]);
        // Stored counts after each: 69, 207, 621, 1863, 5589, 16767, 38847, 60927.
        assert_eq!(count, 60927);
    }

    #[test]
    fn ceiling_clamps_large_counts() {
        // 2 * count >= 22080  <=>  count >= 11040.
        assert_eq!(training_seed(Some(11039)), 22078); // just below the cap
        assert_eq!(training_seed(Some(11040)), 22080); // exactly at the cap
        assert_eq!(training_seed(Some(11041)), 22080); // clamped
        assert_eq!(training_seed(Some(1_000_000)), 22080);
    }

    #[test]
    fn predicted_path_is_flat_69() {
        assert_eq!(predicted_seed(), 69);
    }

    #[test]
    fn unigram_delta_is_seed_times_seven() {
        assert_eq!(unigram_delta(69), 483);
        assert_eq!(unigram_delta(138), 966);
        assert_eq!(unigram_delta(22080), 22080 * 7);
    }
}
