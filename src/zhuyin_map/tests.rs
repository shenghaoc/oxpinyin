use super::ZhuyinIndex;
use crate::vocab::SyllableKey;

/// Every source row is found by its input spelling, and its stored key
/// is the canonical pinyin's key and renders the canonical zhuyin.
fn matches_original_rows(index: ZhuyinIndex, rows: &[(&str, &str, &str, u32)]) {
    for (row, &(input, canonical, pinyin, flags)) in rows.iter().enumerate() {
        assert_eq!(index.position(input), Some(row), "{input:?}");
        let key = SyllableKey::from_canonical_text(pinyin).expect("canonical pinyin");
        assert_eq!(
            index.get(row),
            Some((key, crate::OptionBits::from_bits_retain(flags))),
            "{input:?}"
        );
        assert_eq!(
            key.chewing_key().zhuyin_text().to_string(),
            canonical,
            "{input:?}"
        );
    }
    assert_eq!(index.get(rows.len()), None);
}

#[test]
fn zhuyin_pinyin_map_matches_original_rows() {
    matches_original_rows(
        super::zhuyin_pinyin_map(),
        &super::reference::ZHUYIN_PINYIN_MAP,
    );
}

#[test]
fn hsu_zhuyin_index_matches_original_rows() {
    matches_original_rows(
        super::hsu_zhuyin_index(),
        &super::reference::HSU_ZHUYIN_INDEX,
    );
}

#[test]
fn eten26_zhuyin_index_matches_original_rows() {
    matches_original_rows(
        super::eten26_zhuyin_index(),
        &super::reference::ETEN26_ZHUYIN_INDEX,
    );
}
