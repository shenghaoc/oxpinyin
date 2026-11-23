use super::abort_toned_incomplete;

fn key(text: &str) -> crate::vocab::SyllableKey {
    crate::vocab::SyllableKey::from_text(text).expect("key")
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
