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
pub const INITIAL_SEED: u32 = 23 * 3; // 69

/// Multiplier applied to the prior stored count on each reselection (§2).
pub(crate) const EXPAND_FACTOR: u32 = 2;

/// Upper bound on a single reselection seed. `23 * 15 * 64` in the source (§2).
pub const CEILING_SEED: u32 = 23 * 15 * 64; // 22080

/// Multiplier turning a bigram seed into the phrase-index unigram delta (§2).
pub(crate) const UNIGRAM_FACTOR: u32 = 7;

/// Training seed for a `(last_token -> token)` selection (the `pinyin_train`
/// path, §2).
///
/// An absent count uses [`INITIAL_SEED`]. For a recorded pair, applies the
/// initial-seed minimum to the previous `guint32` count, doubles it with
/// wrapping `u32` arithmetic, and caps the result at [`CEILING_SEED`]
/// (`074a2219 phonetic_lookup.h:892-894`).
///
/// Note the growth is not a literal doubling of the *count*: because the store
/// adds this seed to `prev_count`, the stored count roughly triples per
/// reselection (`new = prev + 2*prev`). The `× 2` is `EXPAND_FACTOR` applied to
/// `prev_count`, exactly as §2 specifies. Seeds run 69, 138, 414, 1242, …,
/// clamping to 22080.
#[must_use]
pub(crate) fn training_seed(prev_count: Option<u32>) -> u32 {
    prev_count.map_or(INITIAL_SEED, |freq| {
        freq.max(INITIAL_SEED)
            .wrapping_mul(EXPAND_FACTOR)
            .min(CEILING_SEED)
    })
}

/// Flat seed for an accepted *predicted* candidate (the
/// `pinyin_choose_predicted_candidate` path, §2): always [`INITIAL_SEED`],
/// never the reselection expansion.
#[must_use]
pub(crate) const fn predicted_seed() -> u32 {
    INITIAL_SEED
}

/// Multiplies the seed by `UNIGRAM_FACTOR`. Every seed is at most
/// [`CEILING_SEED`], so the product (at most 154560) fits.
#[must_use]
pub const fn unigram_delta(seed: u32) -> u32 {
    seed * UNIGRAM_FACTOR
}

#[cfg(test)]
mod tests;
