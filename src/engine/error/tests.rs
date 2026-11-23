use super::abort_toned_incomplete;

fn key(text: &str) -> crate::vocab::SyllableKey {
    crate::vocab::SyllableKey::from_text(text).expect("key")
}

// Basis: pinyin_phrase3.h:148–154 at 074a2219.
#[test]
fn only_a_toned_first_initial_only_key_is_the_table_assert() {
    abort_toned_incomplete(&[key("h"), key("n")], &[0, 4]);
    abort_toned_incomplete(&[key("ni")], &[4]);
}

// Basis: pinyin_phrase3.h:148–154 at 074a2219.
#[test]
fn a_toned_initial_only_key_behind_a_complete_key_aborts() {
    crate::store::death_test::assert_aborts(
        "engine::error::tests::a_toned_initial_only_key_behind_a_complete_key_aborts",
        || abort_toned_incomplete(&[key("ni"), key("n")], &[3, 4]),
    );
}
