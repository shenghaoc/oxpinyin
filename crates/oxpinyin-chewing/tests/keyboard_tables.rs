//! Key-data-table totality over the packed key space — G1 of the
//! testing-strategy assessment (`testing-strategy.md` §2).
//!
//! The hostile input surface for this crate is the packed `guint16` the
//! C ABI hands across the boundary (`pinyin_parse_chewing` fills a
//! `_ChewingKey` from caller memory): every one of the 65,536 bit
//! patterns decodes to *some* element quadruple, and upstream indexes
//! `chewing_key_table` unguarded under asserts that oxpinyin replaces
//! with a graceful zero table index (`chewing_key.rs` no-abort note).
//! These tests pin that policy at the public seam:
//!
//! - exhaustive: **every** packed pattern renders through all six
//!   renderers and round-trips the packed form (bit 15, the zero padding
//!   bit, drops — matching `_ChewingKey`'s declaration, which has no
//!   field there);
//! - property: every hand-crafted element quadruple (the unpacked values
//!   a caller could build in Rust) also renders totally, and
//!   `from_packed(to_packed(key))` is the identity on the masked fields.
//!
//! The frozen-table cross-validation (every `content_table` row resolving
//! to its own index through `chewing_key_table`) is a unit test inside
//! the crate; what only a test *outside* the crate can pin is the
//! no-panic contract on inputs the tables were never meant to see.

use oxpinyin_chewing::ChewingKey;
use proptest::prelude::*;

/// Every renderer must terminate and return for any key — the module's
/// no-abort policy. Exhaustive over the packed space the C ABI carries.
#[test]
fn every_packed_pattern_renders_totally() {
    for bits in 0_u16..=u16::MAX {
        let key = ChewingKey::from_packed(bits);
        // Six display surfaces; the bindings keep the results alive so
        // the calls cannot be optimized away.
        let _ = key.pinyin_string();
        let _ = key.shengmu_string();
        let _ = key.yunmu_string();
        let _ = key.zhuyin_string();
        let _ = key.luoma_pinyin_string();
        let _ = key.secondary_zhuyin_string();
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

/// Hand-crafted element quadruples (not reachable from the packed form
/// when a field exceeds its bitfield width) render totally and mask
/// back into the packed form consistently.
#[test]
fn arbitrary_element_quadruples_render_totally() {
    proptest!(|(initial in 0_u8.., middle in 0_u8.., final_ in 0_u8.., tone in 0_u8..)| {
        let key = ChewingKey::new(initial, middle, final_, tone);
        let _ = key.pinyin_string();
        let _ = key.shengmu_string();
        let _ = key.yunmu_string();
        let _ = key.zhuyin_string();
        let _ = key.luoma_pinyin_string();
        let _ = key.secondary_zhuyin_string();

        // Masking law: the packed form truncates each element to its
        // bitfield width (initial 5 bits, middle 2, final 5, tone 3) and
        // unpacking the result reproduces exactly the truncated key.
        let packed = key.to_packed();
        let round = ChewingKey::from_packed(packed);
        prop_assert_eq!(round.initial, initial & 0x1f);
        prop_assert_eq!(round.middle, middle & 0x3);
        prop_assert_eq!(round.final_, final_ & 0x1f);
        prop_assert_eq!(round.tone, tone & 0x7);
        prop_assert_eq!(round.to_packed(), packed);
    });
}

/// The zero key is the fixed point of the packed form, and tone
/// variation never changes the spelling columns — the renderers append
/// the tone to a *frozen* base (`chewing_key.cpp:47-121`).
#[test]
fn tone_variation_never_moves_the_base_spelling() {
    proptest!(|(bits in 0_u16..)| {
        let key = ChewingKey::from_packed(bits);
        for tone in 0_u8..=7 {
            let toned = key.with_tone(tone);
            prop_assert_eq!(toned.pinyin_spelling(), key.pinyin_spelling());
            prop_assert_eq!(toned.shengmu_string(), key.shengmu_string());
            prop_assert_eq!(toned.yunmu_string(), key.yunmu_string());
        }
    });
}
