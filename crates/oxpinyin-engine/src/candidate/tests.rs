#[test]
fn frequency_guard_agrees_with_the_order_laws_at_wrap_boundaries() {
    use super::{frequency_compare, frequency_order_is_consistent};
    let values = [
        0,
        1,
        0x6000_0000,
        0x7fff_ffff,
        0x8000_0000,
        0x8000_0001,
        0xc000_0000,
        u32::MAX,
    ];
    for a in values {
        for b in values {
            for c in values {
                let rows = [a, b, c];
                // Independent definition of a consistent comparison:
                // antisymmetry and transitivity on this complete list.
                let consistent = rows.iter().all(|&x| {
                    rows.iter().all(|&y| {
                        frequency_compare(x, y) == frequency_compare(y, x).reverse()
                            && rows.iter().all(|&z| {
                                !(frequency_compare(x, y).is_le()
                                    && frequency_compare(y, z).is_le())
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
        }
    }
    assert!(frequency_order_is_consistent([].into_iter()));
    assert!(frequency_order_is_consistent([7].into_iter()));
    assert!(frequency_order_is_consistent([7, 7, 7].into_iter()));
    assert!(!frequency_order_is_consistent([0, 1 << 31].into_iter()));
    assert!(!frequency_order_is_consistent(
        [0, 0x6000_0000, 0xc000_0000].into_iter()
    ));
    assert!(frequency_order_is_consistent([u32::MAX, 0, 1].into_iter()));
    assert!(frequency_order_is_consistent([0, 0x7fff_ffff].into_iter()));
}

#[test]
fn frequency_sort_guards_each_leading_key_group_and_preserves_ties() {
    // The first group cycles and falls back numerically. The second
    // crosses zero consistently and must keep wrapping order, despite
    // the cycle elsewhere. Different leading fields never compare freq.
    let mut rows = vec![
        (2, 1, 0, 'a'),
        (1, 1, u32::MAX, 'b'),
        (2, 1, 0x6000_0000, 'c'),
        (1, 1, 1, 'd'),
        (2, 1, 0xc000_0000, 'e'),
        (1, 1, 1, 'f'),
        (1, 2, 1 << 31, 'g'),
        (2, 0, 0, 'h'),
    ];
    super::rank_by_frequency(&mut rows, |&(length, span, freq, _)| (length, span, freq));
    assert_eq!(
        rows.iter().map(|row| row.3).collect::<Vec<_>>(),
        ['e', 'c', 'a', 'h', 'g', 'd', 'f', 'b']
    );
}

#[test]
fn frequency_sort_has_a_defined_antipodal_fallback() {
    let mut rows = [(1, 0), (0, 1 << 31), (2, 0), (3, 1 << 31)];
    super::rank_by_frequency(&mut rows, |&(_, freq)| (0, 0, freq));
    assert_eq!(rows, [(0, 1 << 31), (3, 1 << 31), (1, 0), (2, 0)]);
    super::rank_by_frequency::<(usize, u32)>(&mut [], |&(_, freq)| (0, 0, freq));
}
