use super::ChewingKey;
use crate::chewing_key_data::reference::CONTENT_TABLE;
use crate::chewing_key_data::{
    CHEWING_KEY_TABLE, CONTENT_KEYS, NUM_FINALS, NUM_MIDDLES, content_strings, spelling_key,
};

#[test]
fn content_table_matches_original_rows() {
    let entries = content_strings().entries();
    assert_eq!(entries.len(), CONTENT_TABLE.len());
    assert_eq!(CONTENT_KEYS.len(), CONTENT_TABLE.len());
    for (index, (entry, row)) in entries.zip(&CONTENT_TABLE).enumerate() {
        assert_eq!(
            entry,
            [
                row.pinyin,
                row.shengmu,
                row.yunmu,
                row.zhuyin,
                row.luoma,
                row.secondary
            ],
            "row {index}"
        );
        let key = CONTENT_KEYS[index];
        assert_eq!(
            [key.initial, key.middle, key.final_],
            [row.initial, row.middle, row.final_],
            "row {index}"
        );
        assert_eq!(Some(key.spelling), spelling_key(row.pinyin), "row {index}");
    }
}

/// Every content row's components resolve, through
/// `chewing_key_table`, to the row's own index — the two ported
/// tables cross-validate (the zero key, is exempt: its
/// components are the all-zero key, which maps to row 0 anyway).
#[test]
fn tables_cross_validate() {
    for (index, key) in CONTENT_KEYS.iter().enumerate() {
        let (initial, middle, final_) = (key.initial, key.middle, key.final_);
        if index == 0 {
            continue;
        }
        let entry = CHEWING_KEY_TABLE[(usize::from(initial) * NUM_MIDDLES + usize::from(middle))
            * NUM_FINALS
            + usize::from(final_)];
        assert_eq!(
            entry,
            i16::try_from(index).expect("table index fits i16"),
            "row {index} ({})",
            content_strings().field(index, 0).expect("content spelling")
        );
    }
}

/// The rows stay sorted by spelling, so `from_pinyin` may binary
/// search; the zero row sorts first.
#[test]
fn content_rows_are_sorted() {
    for pair in content_strings().entries().collect::<Vec<_>>().windows(2) {
        assert!(pair[0][0] <= pair[1][0]);
    }
    assert_eq!(content_strings().field(0, 0), Some(""));
    for pair in CONTENT_KEYS.windows(2) {
        assert!(pair[0].spelling < pair[1].spelling);
    }
}

/// Look up boundary spellings against the source table by string equality.
#[test]
fn spelling_lookup_matches_a_scan_of_the_original_rows() {
    for spelling in ["", "ni", "ni\0", "zhuang", "zhuangzh", "zhuangzhu"] {
        let expected = CONTENT_TABLE
            .iter()
            .find(|row| row.pinyin == spelling)
            .map(|row| ChewingKey {
                initial: row.initial,
                middle: row.middle,
                final_: row.final_,
                tone: super::CHEWING_ZERO_TONE,
            });
        assert_eq!(ChewingKey::from_pinyin(spelling), expected, "{spelling:?}");
    }
}

/// The zero key renders empty everywhere and answers table index 0;
/// the packed zero key unpacks to zero.
#[test]
fn zero_key_renders_empty() {
    let key = ChewingKey::ZERO;
    assert_eq!(key.table_index(), 0);
    assert_eq!(key.pinyin_string(), "");
    assert_eq!(key.shengmu_string(), "");
    assert_eq!(key.yunmu_string(), "");
    assert_eq!(key.zhuyin_string(), "");
    assert_eq!(key.luoma_pinyin_text().to_string(), "");
    assert_eq!(key.secondary_zhuyin_text().to_string(), "");
    assert_eq!(key.to_packed(), 0);
}

/// Renderer spot checks against the pinned content rows: tone digit
/// placement differs per renderer (zhuyin bare at tone 1, luoma and
/// secondary zhuyin digit at tone 1).
#[test]
fn renderers_match_the_pinned_rows() {
    let zhang = ChewingKey::from_pinyin("zhang").expect("row");
    assert_eq!(zhang.shengmu_string(), "zh");
    assert_eq!(zhang.yunmu_string(), "ang");
    assert_eq!(zhang.zhuyin_string(), "ㄓㄤ");
    assert_eq!(zhang.luoma_pinyin_text().to_string(), "jhang");
    assert_eq!(zhang.secondary_zhuyin_text().to_string(), "jang");

    let zhang3 = zhang.with_tone(3);
    assert_eq!(zhang3.pinyin_string(), "zhang3");
    assert_eq!(zhang3.zhuyin_string(), "ㄓㄤˇ");
    assert_eq!(zhang3.luoma_pinyin_text().to_string(), "jhang3");
    assert_eq!(zhang3.secondary_zhuyin_text().to_string(), "jang3");

    let jong = ChewingKey::from_pinyin("zhong").expect("row");
    assert_eq!(jong.luoma_pinyin_text().to_string(), "jhong");
    assert_eq!(jong.secondary_zhuyin_text().to_string(), "jung");
    assert_eq!(
        jong.with_tone(1).luoma_pinyin_text().to_string(),
        "jhong1",
        "luoma appends the first-tone digit"
    );
    assert_eq!(
        jong.with_tone(1).secondary_zhuyin_text().to_string(),
        "jung1",
        "secondary zhuyin appends the first-tone digit"
    );
    assert_eq!(
        jong.with_tone(1).zhuyin_string(),
        "ㄓㄨㄥ",
        "zhuyin stays bare at the first tone"
    );

    let b = ChewingKey::from_pinyin("b").expect("initial-only row");
    assert_eq!(b.shengmu_string(), "b");
    assert_eq!(b.yunmu_string(), "");
    assert_eq!(b.zhuyin_string(), "ㄅ");
    assert_eq!(b.luoma_pinyin_text().to_string(), "None");
    assert_eq!(b.secondary_zhuyin_text().to_string(), "None");
}

fn key(spelling: &str) -> ChewingKey {
    ChewingKey::from_pinyin(spelling).expect("a pinyin spelling")
}

#[test]
fn matches_with_tones_follows_the_three_upstream_comparisons() {
    let ni = key("ni");
    let ni3 = ni.with_tone(3);
    let n = ChewingKey::new(ni.initial, 0, 0, 0);
    let na = key("na");
    assert!(ni.matches_with_tones(ni3), "zero tone matches any tone");
    assert!(ni3.matches_with_tones(ni), "either side zero");
    assert!(!ni3.with_tone(1).matches_with_tones(ni3), "tones differ");
    assert!(
        n.matches_with_tones(ni3),
        "incomplete query matches any final"
    );
    assert!(ni3.matches_with_tones(n), "incomplete stored matches too");
    assert!(!na.matches_with_tones(ni3), "finals differ");
    assert!(!key("hao").matches_with_tones(ni), "initials differ");
    assert!(
        !n.with_tone(1).matches_with_tones(n.with_tone(2)),
        "an incomplete key still compares tones"
    );
}

// Key-data-table totality over the packed key space
// (`docs/testing/upstream-test-coverage.md`, "libpinyin tests").
//
// The hostile input surface for this crate is the packed `guint16` the
// C ABI hands across the boundary (`pinyin_parse_chewing` fills a
// `_ChewingKey` from caller memory): every one of the 65,536 bit
// patterns decodes to *some* element quadruple, and upstream indexes
// `chewing_key_table` unguarded under asserts that oxpinyin replaces
// with a graceful zero table index (see `chewing_key.rs`).
// These tests pin that policy at the public seam:
//
// - exhaustive: **every** packed pattern renders through all six
//   renderers and round-trips the packed form (bit 15, the zero padding
//   bit, drops — matching `_ChewingKey`'s declaration, which has no
//   field there);
// - every combination of in-range fields and the overflow boundaries of
//   each field render totally and round-trip as their masked values.
//
// The frozen-table cross-validation (every `content_table` row resolving
// to its own index through `chewing_key_table`) is `tables_cross_validate`
// above; these tests pin the no-panic contract on inputs the tables were
// never meant to see.

fn render_key(key: ChewingKey) {
    let _ = key.pinyin_string();
    let _ = key.shengmu_string();
    let _ = key.yunmu_string();
    let _ = key.zhuyin_string();
    let _ = key.luoma_pinyin_text().to_string();
    let _ = key.secondary_zhuyin_text().to_string();
}

/// Every renderer must terminate and return for any key — the module's
/// out-of-dims rule. Exhaustive over the packed space the C ABI carries.
#[test]
fn every_packed_pattern_renders_totally() {
    for bits in 0_u16..=u16::MAX {
        render_key(ChewingKey::from_packed(bits));
    }
}

/// `from_packed ∘ to_packed` is the identity because `to_packed` masks
/// each element to its bitfield width; the padding bit (15) drops.
#[test]
fn every_packed_pattern_round_trips_the_packed_form() {
    for bits in 0_u16..=u16::MAX {
        let key = ChewingKey::from_packed(bits);
        assert_eq!(
            key.to_packed(),
            bits & 0x7fff,
            "padding bit must drop; every other bit must survive"
        );
        assert_eq!(
            ChewingKey::from_packed(key.to_packed()),
            key,
            "the packed form must be a fixed point"
        );
    }
}

// _ChewingKey field widths in chewing_key.h:41–48.
const INITIAL_MASK: u8 = 0x1f;
const MIDDLE_MASK: u8 = 0x3;
const FINAL_MASK: u8 = 0x1f;
const TONE_MASK: u8 = 0x7;

fn check_element_quadruple(initial: u8, middle: u8, final_: u8, tone: u8) {
    let key = ChewingKey::new(initial, middle, final_, tone);
    render_key(key);
    let packed = key.to_packed();
    let round = ChewingKey::from_packed(packed);
    let masked = ChewingKey {
        initial: initial & INITIAL_MASK,
        middle: middle & MIDDLE_MASK,
        final_: final_ & FINAL_MASK,
        tone: tone & TONE_MASK,
    };
    assert_eq!(round, masked, "{key:?}");
    assert_eq!(round.to_packed(), packed, "{key:?}");
}

/// Cover every in-range field combination and each field's first overflow
/// value and largest input.
#[test]
fn element_quadruples_render_totally_and_pack_as_masked_fields() {
    for initial in 0..=INITIAL_MASK {
        for middle in 0..=MIDDLE_MASK {
            for final_ in 0..=FINAL_MASK {
                for tone in 0..=TONE_MASK {
                    check_element_quadruple(initial, middle, final_, tone);
                }
            }
        }
    }

    // Keep bit zero clear in adjacent fields so a bit spill changes the result.
    for (initial, middle, final_, tone) in [
        (INITIAL_MASK + 1, 2, 2, 2),
        (u8::MAX, 2, 2, 2),
        (2, MIDDLE_MASK + 1, 2, 2),
        (2, u8::MAX, 2, 2),
        (2, 2, FINAL_MASK + 1, 2),
        (2, 2, u8::MAX, 2),
        (2, 2, 2, TONE_MASK + 1),
        (2, 2, 2, u8::MAX),
    ] {
        check_element_quadruple(initial, middle, final_, tone);
    }
}

/// Tone variation preserves the spelling columns for every packed key
/// (`chewing_key.cpp:47–121`).
#[test]
fn tone_variation_never_moves_the_base_spelling() {
    for bits in 0_u16..=u16::MAX {
        let key = ChewingKey::from_packed(bits);
        for tone in 0..=TONE_MASK {
            let toned = key.with_tone(tone);
            assert_eq!(toned.pinyin_spelling(), key.pinyin_spelling(), "{toned:?}");
            assert_eq!(toned.shengmu_string(), key.shengmu_string(), "{toned:?}");
            assert_eq!(toned.yunmu_string(), key.yunmu_string(), "{toned:?}");
        }
    }
}
