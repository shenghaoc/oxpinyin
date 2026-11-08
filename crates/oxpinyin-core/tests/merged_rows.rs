//! Merged-row and step-row behaviour against the shared fixture doubles.

#[test]
fn step_row_reuses_hits_and_misses_but_retries_errors() {
    use oxpinyin_core::{MergedGram, NbestStepRow};
    let mut row = NbestStepRow::default();
    assert_eq!(
        row.get_or_try_init::<&str>(|| Err("empty value")).err(),
        Some("empty value")
    );
    // The row keeps the order given: `lower_bound(9)` over `[9, 3]` lands
    // past the end and `lower_bound(3)` on the 9, so the pin's `get_freq`
    // misses both (`ngram.cpp:231-250`).
    let gram = row
        .get_or_try_init::<&str>(|| Ok(Some(MergedGram::new(0, vec![(9, 0), (3, 7)]))))
        .unwrap()
        .unwrap();
    assert_eq!(gram.get_freq(9), Ok(None));
    assert_eq!(gram.get_freq(4), Ok(None));
    assert_eq!(
        row.get_or_try_init::<&str>(|| panic!("hit reloaded"))
            .unwrap()
            .unwrap()
            .get_freq(3),
        Ok(None)
    );
    let mut miss = NbestStepRow::default();
    assert!(miss.get_or_try_init::<&str>(|| Ok(None)).unwrap().is_none());
    assert!(
        miss.get_or_try_init::<&str>(|| panic!("miss reloaded"))
            .unwrap()
            .is_none()
    );
}

/// `merge_single_gram` over an unsorted system row and a user row
/// (`ngram.cpp:308-349`): `3 > 2` appends the user's 2, the equal 3s sum,
/// and the system's remainder follows, giving `[2, 3, 1, 3]`. The pin's
/// `get_freq` then misses 1 and 2 and finds the second 3; `search` over
/// `1..=3` yields every item, duplicates included.
#[test]
fn a_merged_row_answers_as_the_pins_merged_chunk() {
    use oxpinyin_core::{MergedGram, NbestStepRow, NbestSuccessors};
    let chunk = |total: u32, items: &[(u32, u32)]| {
        let mut value = total.to_le_bytes().to_vec();
        for (token, count) in items {
            value.extend_from_slice(&token.to_le_bytes());
            value.extend_from_slice(&count.to_le_bytes());
        }
        value
    };
    let system = chunk(0, &[(3, 0), (1, 4), (3, 9)]);
    let user = chunk(0, &[(2, 7), (3, u32::MAX)]);
    let mut row = NbestStepRow::default();
    let gram = row
        .get_or_try_init::<&str>(|| Ok(MergedGram::merge(Some(system), Some(user))))
        .unwrap()
        .unwrap();
    assert_eq!(
        gram.to_bytes(),
        chunk(0, &[(2, 7), (3, u32::MAX), (1, 4), (3, 9)])
    );
    assert_eq!(gram.total(), Ok(0));
    assert_eq!(gram.get_freq(1), Ok(None));
    assert_eq!(gram.get_freq(2), Ok(None));
    assert_eq!(gram.get_freq(3), Ok(Some(9)));
    assert_eq!(gram.get_freq(4), Ok(None));
    let mut items = Vec::new();
    gram.search_into(1, 3, &mut items).unwrap();
    assert_eq!(items, [(2, 7), (3, u32::MAX), (1, 4), (3, 9)]);
    assert!(matches!(
        NbestSuccessors::Row(gram),
        NbestSuccessors::Row(row) if row.fault().is_none()
    ));
    assert!(MergedGram::merge(None, None).is_none());
}

/// A range ending at `u32::MAX`: the pin's exclusive end is the `guint32`
/// `token + 1` (`chewing_large_table2.h:115`, `:120` at the pin
/// `074a2219`), zero, and `search` stops at the first item at or past it
/// (`ngram.cpp:168`) — every item — so the walk yields nothing.
#[test]
fn a_range_ending_at_the_last_token_searches_nothing_as_the_pin_does() {
    use oxpinyin_core::MergedGram;
    let gram = MergedGram::new(2, vec![(u32::MAX - 1, 1), (u32::MAX, 1)]);
    let mut items = Vec::new();
    gram.search_into(u32::MAX, u32::MAX, &mut items).unwrap();
    gram.search_into(u32::MAX - 1, u32::MAX, &mut items)
        .unwrap();
    assert!(items.is_empty());
    gram.search_into(u32::MAX - 1, u32::MAX - 1, &mut items)
        .unwrap();
    assert_eq!(items, [(u32::MAX - 1, 1)]);
}

/// N2 refusals 1 and 5 at the merge (`ngram.cpp:303-349`): both grams
/// present, a user value shorter than its total asserts and a partial
/// tail is consumed past its end; every read of the row then answers the
/// fault. With no system gram the user value is the row as stored, and
/// the fault surfaces at the first read that meets it.
#[test]
fn a_merge_the_pin_cannot_read_answers_its_fault() {
    use oxpinyin_core::{GramFault, MergedGram};
    let system = {
        let mut value = 4_u32.to_le_bytes().to_vec();
        value.extend_from_slice(&2_u32.to_le_bytes());
        value.extend_from_slice(&4_u32.to_le_bytes());
        value
    };
    let short = MergedGram::merge(Some(system.clone()), Some(vec![1, 2])).unwrap();
    assert_eq!(short.fault(), Some(GramFault::Short));
    assert_eq!(short.total(), Err(GramFault::Short));
    assert_eq!(short.get_freq(2), Err(GramFault::Short));
    let mut tailed = 0_u32.to_le_bytes().to_vec();
    tailed.push(9);
    let over = MergedGram::merge(Some(system.clone()), Some(tailed.clone())).unwrap();
    assert_eq!(over.fault(), Some(GramFault::OverRead));
    let mut items = Vec::new();
    assert_eq!(over.search_into(0, 9, &mut items), Err(GramFault::OverRead));
    // No system gram: the user value itself, read as the pin reads it.
    let alone = MergedGram::merge(None, Some(vec![1, 2])).unwrap();
    assert_eq!(alone.fault(), None);
    assert_eq!(alone.to_bytes(), [1, 2]);
    assert_eq!(alone.total(), Err(GramFault::Short));
    assert_eq!(alone.get_freq(2), Err(GramFault::OverRead));
    assert_eq!(alone.search_into(0, 9, &mut items), Err(GramFault::Short));
    let alone = MergedGram::merge(None, Some(tailed)).unwrap();
    assert_eq!(alone.total(), Ok(0));
    assert_eq!(alone.get_freq(2), Err(GramFault::OverRead));
    // The system gram alone is its own bytes.
    assert_eq!(
        MergedGram::merge(Some(system.clone()), None)
            .unwrap()
            .to_bytes(),
        system
    );
}
