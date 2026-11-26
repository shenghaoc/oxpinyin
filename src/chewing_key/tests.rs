use super::ChewingKey;
use crate::chewing_key_data::reference::CONTENT_TABLE;

// Frozen 074a2219 content_table rows; chewing_key.cpp:27–121 resolves and renders them.
#[test]
fn tables_cross_validate() {
    for (index, row) in CONTENT_TABLE.iter().enumerate() {
        let key = ChewingKey::from_pinyin(row.pinyin).expect("frozen spelling");
        assert_eq!(
            (key.initial, key.middle, key.final_),
            (row.initial, row.middle, row.final_)
        );
        assert_eq!(key.table_index(), index, "{}", row.pinyin);
        assert_eq!(
            [
                key.pinyin_string(),
                key.shengmu_string().to_owned(),
                key.yunmu_string().to_owned(),
                key.zhuyin_string(),
                key.luoma_pinyin_text().to_string(),
                key.secondary_zhuyin_text().to_string(),
            ],
            [
                row.pinyin,
                row.shengmu,
                row.yunmu,
                row.zhuyin,
                row.luoma,
                row.secondary
            ]
            .map(str::to_owned),
            "row {index}"
        );
    }
}

// Rust spelling_key contract rejects NUL and lengths above eight; other misses follow the frozen rows.
#[test]
fn spelling_lookup_matches_a_scan_of_the_original_rows() {
    for spelling in ["ni\0", "zhuangzh", "zhuangzhu"] {
        let expected = CONTENT_TABLE
            .iter()
            .find(|row| row.pinyin == spelling)
            .map(|row| {
                ChewingKey::new(
                    row.initial,
                    row.middle,
                    row.final_,
                    super::CHEWING_ZERO_TONE,
                )
            });
        assert_eq!(ChewingKey::from_pinyin(spelling), expected, "{spelling:?}");
    }
}

// 074a2219 chewing_key.cpp:47–121 appends digits, but Zhuyin leaves the first tone bare.
#[test]
fn renderers_match_the_pinned_rows() {
    for (spelling, tone, zhuyin, luoma, secondary) in [
        ("zhang", 3, "ㄓㄤˇ", "jhang3", "jang3"),
        ("zhong", 1, "ㄓㄨㄥ", "jhong1", "jung1"),
    ] {
        let key = key(spelling).with_tone(tone);
        assert_eq!(key.zhuyin_string(), zhuyin);
        assert_eq!(key.luoma_pinyin_text().to_string(), luoma);
        assert_eq!(key.secondary_zhuyin_text().to_string(), secondary);
    }
    assert_eq!(key("zhang").with_tone(3).pinyin_string(), "zhang3");
}

fn key(spelling: &str) -> ChewingKey {
    ChewingKey::from_pinyin(spelling).expect("a pinyin spelling")
}

// 074a2219 pinyin_phrase3.h:68–144 compares initials, middle/final pairs and tones separately.
#[test]
fn matches_with_tones_follows_the_three_upstream_comparisons() {
    let ni = key("ni");
    let ni3 = ni.with_tone(3);
    let n = ChewingKey::new(ni.initial, 0, 0, 0);
    for (query, stored, expected) in [
        (ni, ni3, true),
        (ni3, ni, true),
        (ni.with_tone(1), ni3, false),
        (n, ni3, true),
        (ni3, n, true),
        (key("na"), ni3, false),
        (key("hao"), ni, false),
        (n.with_tone(1), n.with_tone(2), false),
    ] {
        assert_eq!(
            query.matches_with_tones(stored),
            expected,
            "{query:?}, {stored:?}"
        );
    }
}

// 074a2219 chewing_key.h:41–48 assigns widths 5/2/5/3; the Rust packed form clears padding.
#[test]
fn packed_fields_round_trip_without_padding() {
    for bits in [0_u16, 0x1234, 0x7fff, 0x8000] {
        let key = ChewingKey::from_packed(bits);
        assert_eq!(key.to_packed(), bits & 0x7fff);
        assert_eq!(ChewingKey::from_packed(key.to_packed()), key);
    }
}
