use super::*;

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
fn seed_expansion_wraps_before_the_ceiling() {
    assert_eq!(training_seed(Some(0)), 138);
    assert_eq!(training_seed(Some(0x7fff_ffff)), 22080);
    assert_eq!(training_seed(Some(0x8000_0000)), 0);
    assert_eq!(training_seed(Some(0x8000_0001)), 2);
    assert_eq!(training_seed(Some(u64::from(u32::MAX))), 22080);
}
