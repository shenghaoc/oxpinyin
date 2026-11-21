use super::{EngineError, abort_toned_incomplete};

fn key(text: &str) -> crate::SyllableKey {
    crate::SyllableKey::from_text(text).expect("key")
}

/// `contains_incomplete_pinyin` returns at the first initial-only key
/// (`pinyin_phrase3.h:148-154`): a later toned initial-only key is never
/// checked, and an untoned initial-only or complete first key passes.
#[test]
fn only_a_toned_first_initial_only_key_is_the_table_assert() {
    abort_toned_incomplete(&[key("h"), key("n")], &[0, 4]);
    abort_toned_incomplete(&[key("n")], &[0]);
    abort_toned_incomplete(&[key("ni")], &[4]);
}

/// The pin's table search asserts on a toned first initial-only key
/// (`CHEWING_ZERO_TONE == key.m_tone`, `pinyin_phrase3.h:152` at
/// 074a2219): the check dies at the check site.
#[test]
fn a_toned_first_initial_only_key_aborts() {
    crate::store::death_test::assert_aborts(
        "engine::error::tests::a_toned_first_initial_only_key_aborts",
        || abort_toned_incomplete(&[key("n")], &[4]),
    );
}

/// The first initial-only key decides even behind a complete key: `ni n`
/// with a tone on the `n` is the pin's assert.
#[test]
fn a_toned_initial_only_key_behind_a_complete_key_aborts() {
    crate::store::death_test::assert_aborts(
        "engine::error::tests::a_toned_initial_only_key_behind_a_complete_key_aborts",
        || abort_toned_incomplete(&[key("ni"), key("n")], &[3, 4]),
    );
}

/// The over-read gram fault keeps its typed refusal: the C layers warn
/// once and refuse (`ngram.cpp:318-349`, register row 98).
#[test]
fn an_over_read_gram_fault_keeps_its_refusal() {
    assert_eq!(
        EngineError::lookup_gram(crate::GramFault::OverRead),
        EngineError::UnsafeLookupPredecessor
    );
}

/// The short merged-gram total dies at the mapping every lookup site
/// shares: the pin reads the total through `memory_chunk.h:390`
/// (`merge_single_gram`, `ngram.cpp:303-304`; `SingleGram::search`,
/// `:165`; `get_total_freq`, `:49`; register row 85).
#[test]
fn a_short_gram_fault_aborts_at_the_shared_mapping() {
    crate::store::death_test::assert_aborts(
        "engine::error::tests::a_short_gram_fault_aborts_at_the_shared_mapping",
        || {
            let _ = EngineError::lookup_gram(crate::GramFault::Short);
        },
    );
}
