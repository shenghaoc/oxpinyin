use crate::vocab::SyllableKey;

// Frozen 074a2219 pinyin_parser_table.h Zhuyin indexes map input spellings to keys and flags.
#[test]
fn zhuyin_pinyin_map_matches_original_rows() {
    for (index, rows) in [
        (
            super::zhuyin_pinyin_map(),
            super::reference::ZHUYIN_PINYIN_MAP.as_slice(),
        ),
        (
            super::hsu_zhuyin_index(),
            super::reference::HSU_ZHUYIN_INDEX.as_slice(),
        ),
        (
            super::eten26_zhuyin_index(),
            super::reference::ETEN26_ZHUYIN_INDEX.as_slice(),
        ),
    ] {
        for (row, &(input, _, pinyin, flags)) in rows.iter().enumerate() {
            assert_eq!(index.position(input), Some(row), "{input:?}");
            let key = SyllableKey::from_canonical_text(pinyin).expect("canonical pinyin");
            assert_eq!(
                index.get(row),
                Some((key, crate::OptionBits::from_bits_retain(flags))),
                "{input:?}"
            );
        }
        assert_eq!(index.get(rows.len()), None);
    }
}
