// Basis: Rust contract of candidate::narrow.
#[test]
fn narrow_saturates_counts_above_the_row_width() {
    let limit = u32::MAX as usize;
    assert_eq!(super::narrow(limit - 1), u32::MAX - 1);
    assert_eq!(super::narrow(limit), u32::MAX);
    #[cfg(target_pointer_width = "64")]
    assert_eq!(super::narrow(limit + 1), u32::MAX);
}

// Basis: Divergence register row 151.
#[test]
fn frequency_guard_agrees_with_the_order_laws_at_wrap_boundaries() {
    use super::{frequency_compare, frequency_order_is_consistent};
    for rows in [
        [7, 7, 7],
        [u32::MAX, 0, 1],
        [0, 0, 0x7fff_ffff],
        [0, 0, 0x8000_0000],
        [0, 0x7fff_ffff, 0x8000_0001],
    ] {
        let consistent = rows.iter().all(|&x| {
            rows.iter().all(|&y| {
                frequency_compare(x, y) == frequency_compare(y, x).reverse()
                    && rows.iter().all(|&z| {
                        !(frequency_compare(x, y).is_le() && frequency_compare(y, z).is_le())
                            || frequency_compare(x, z).is_le()
                    })
            })
        });
        assert_eq!(
            frequency_order_is_consistent(rows.into_iter()),
            consistent,
            "{rows:?}"
        );
    }
    assert!(frequency_order_is_consistent([].into_iter()));
}

// Basis: Divergence register row 151.
#[test]
fn frequency_sort_guards_each_leading_key_group_and_preserves_ties() {
    let mixed = vec![
        (2, 1, 0, 'a'),
        (1, 1, u32::MAX, 'b'),
        (2, 1, 0x6000_0000, 'c'),
        (1, 1, 1, 'd'),
        (2, 1, 0xc000_0000, 'e'),
        (1, 1, 1, 'f'),
        (1, 2, 1 << 31, 'g'),
        (2, 0, 0, 'h'),
    ];
    let antipodal = vec![
        (0, 0, 0, 'a'),
        (0, 0, 1 << 31, 'b'),
        (0, 0, 0, 'c'),
        (0, 0, 1 << 31, 'd'),
    ];
    for (mut rows, expected) in [
        (mixed, &['e', 'c', 'a', 'h', 'g', 'd', 'f', 'b'][..]),
        (antipodal, &['b', 'd', 'a', 'c']),
        (vec![], &[]),
    ] {
        super::rank_by_frequency(&mut rows, |&(length, span, freq, _)| (length, span, freq));
        assert_eq!(rows.iter().map(|row| row.3).collect::<Vec<_>>(), expected);
    }
}
