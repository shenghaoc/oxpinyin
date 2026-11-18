use crate::types::lookup_candidate_type_t;

/// The Phase-1 correction, pinned: the zhuyin 4-value enum's exact
/// discriminants. The zhuyin header (`zhuyin.h:41-45`) defines four
/// enumerators, and they collide with the pinyin eight at 3 and 4 — so
/// the enum must never be aliased to the pinyin one.
#[test]
fn zhuyin_candidate_type_discriminants_match_zhuyin_h() {
    assert_eq!(lookup_candidate_type_t::BEST_MATCH_CANDIDATE as i32, 1);
    assert_eq!(
        lookup_candidate_type_t::NORMAL_CANDIDATE_AFTER_CURSOR as i32,
        2
    );
    assert_eq!(
        lookup_candidate_type_t::NORMAL_CANDIDATE_BEFORE_CURSOR as i32,
        3
    );
    assert_eq!(lookup_candidate_type_t::ZOMBIE_CANDIDATE as i32, 4);
}
