//! Data-table reads that fail.

use super::*;

/// A phrase-table read that fails is a miss: `PhraseLargeTable3::search`
/// answers `SEARCH_NONE` (`phrase_large_table3_tkrzwdb.cpp:136-139`,
/// `phrase_large_table3_bdb.cpp:196-198`,
/// `phrase_large_table3_kyotodb.cpp:174-177` at 074a2219). With the read
/// of the row of 你你 failing, `_add_phrase` finds no token for the phrase
/// and adds it (`zhuyin.cpp:422-428`, `:472-492`).
#[test]
fn a_failed_phrase_table_read_is_a_miss() {
    crate::test_support::run_in_child(
        "boundary_tests::table_reads::a_failed_phrase_table_read_is_a_miss",
        |user| {
            let (context, instance) = open_with_user(user);
            oxpinyin_store::read_faults::fail_point_read_of(
                &crate::test_support::ucs4_key("你你"),
                0,
            );
            assert!(crate::test_support::import(
                context,
                7,
                "你你",
                "ㄋㄧˇ ㄋㄧˇ".as_bytes(),
                5
            ));
            close(context, instance);
        },
    );
}

/// A failed read of a pinyin-table key is a miss for the search and for
/// the existence check alike (`chewing_large_table2_tkrzwdb.cpp:146-149`,
/// `:511` at 074a2219): with every read of the `ni` row failing, the
/// candidate lookup goes on and the guess answers true.
#[test]
fn a_failed_pinyin_table_existence_read_is_a_miss() {
    crate::test_support::run_in_child(
        "boundary_tests::table_reads::a_failed_pinyin_table_existence_read_is_a_miss",
        |user| {
            let (context, instance) = open_with_user(user);
            let mut key = ChewingKey::ZERO;
            assert!(zhuyin_parse_chewing(
                instance,
                cstr("su3").as_ptr(),
                &raw mut key
            ));
            // The index key: the packed key with its tone bits, 12 to 14,
            // zero.
            let row = (key.packed & 0x0fff).to_le_bytes();
            assert_eq!(
                zhuyin_parse_more_chewings(instance, cstr("su3cl3").as_ptr()),
                6
            );
            oxpinyin_store::read_faults::fail_point_read_of(&row, 0);
            assert!(zhuyin_guess_candidates_after_cursor(instance, 0));
            close(context, instance);
        },
    );
}
