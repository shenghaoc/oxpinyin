//! Integer seed arithmetic for user-store learning.
//!
//! These are the exact update constants and formulae pinned in
//! `docs/findings/user-store.md` §2 (derived there from libpinyin's
//! `train_result3`, `phonetic_lookup.h:844-936`). Stored fields and seed
//! expansion use the pin's `guint32` arithmetic, widened at the Rust seam.
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
/// An absent count uses [`INITIAL_SEED`]. For a recorded pair, narrows the
/// previous count to `u32`, applies the initial-seed minimum, doubles with
/// wrapping `u32` arithmetic, and caps the result at [`CEILING_SEED`].
/// Returns the widened result (`074a2219 phonetic_lookup.h:892-894`).
///
/// Note the growth is not a literal doubling of the *count*: because the store
/// adds this seed to `prev_count`, the stored count roughly triples per
/// reselection (`new = prev + 2*prev`). The `× 2` is `EXPAND_FACTOR` applied to
/// `prev_count`, exactly as §2 specifies. Seeds run 69, 138, 414, 1242, …,
/// clamping to 22080.
#[must_use]
pub fn training_seed(prev_count: Option<u64>) -> u64 {
    prev_count.map_or(INITIAL_SEED, |freq| {
        u64::from(
            (freq as u32)
                .max(INITIAL_SEED as u32)
                .wrapping_mul(EXPAND_FACTOR as u32)
                .min(CEILING_SEED as u32),
        )
    })
}

/// Flat seed for an accepted *predicted* candidate (the
/// `pinyin_choose_predicted_candidate` path, §2): always [`INITIAL_SEED`],
/// never the reselection expansion.
#[must_use]
pub const fn predicted_seed() -> u64 {
    INITIAL_SEED
}

/// Multiplies the seed by [`UNIGRAM_FACTOR`], saturating at `u64::MAX`.
#[must_use]
pub const fn unigram_delta(seed: u64) -> u64 {
    seed.saturating_mul(UNIGRAM_FACTOR)
}

#[cfg(test)]
mod tests;
