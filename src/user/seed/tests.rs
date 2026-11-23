use super::*;

/// Basis: `074a2219 phonetic_lookup.h:892-894` (initial seed, doubling, ceiling, `guint32` wrap).
#[rstest::rstest]
#[case::absent(None, 69)]
#[case::below_the_initial_seed(Some(0), 138)]
#[case::doubling(Some(207), 414)]
#[case::below_the_ceiling(Some(11039), 22078)]
#[case::on_the_ceiling(Some(11040), 22080)]
#[case::above_the_ceiling(Some(11041), 22080)]
#[case::below_the_wrap(Some(0x7fff_ffff), 22080)]
#[case::on_the_wrap(Some(0x8000_0000), 0)]
#[case::above_the_wrap(Some(0x8000_0001), 2)]
#[case::u32_max(Some(u64::from(u32::MAX)), 22080)]
fn training_seed_follows_the_pinned_formula(#[case] prev: Option<u64>, #[case] seed: u64) {
    assert_eq!(training_seed(prev), seed);
}
