//! `pinyin_option_t` bits and the candidate sort word.
//!
//! Values are those of the public libpinyin headers (`PinyinTableFlag` and
//! `PinyinAmbiguity2` in `pinyin_custom2.h`; the sort enumeration in
//! `pinyin.h`). They are plain `u32` words because that is what
//! `pinyin_set_options` and `pinyin_guess_candidates` take; this module holds
//! no policy about which words are acceptable. A caller that needs to forbid a
//! bit (`pinyin-oracle` forbids [`DYNAMIC_ADJUST`] for its fresh-state
//! protocol) wraps the word itself.

/// `IS_PINYIN` — the base full-pinyin bit.
pub const IS_PINYIN: u32 = 1 << 1;

/// `IS_ZHUYIN`.
pub const IS_ZHUYIN: u32 = 1 << 2;

/// `PINYIN_INCOMPLETE` — admit partial (initial-only) tails.
pub const PINYIN_INCOMPLETE: u32 = 1 << 3;

/// `ZHUYIN_INCOMPLETE`.
pub const ZHUYIN_INCOMPLETE: u32 = 1 << 4;

/// `USE_TONE`.
pub const USE_TONE: u32 = 1 << 5;

/// `FORCE_TONE`.
pub const FORCE_TONE: u32 = 1 << 6;

/// `USE_DIVIDED_TABLE`.
pub const USE_DIVIDED_TABLE: u32 = 1 << 7;

/// `USE_RESPLIT_TABLE`.
pub const USE_RESPLIT_TABLE: u32 = 1 << 8;

/// `DYNAMIC_ADJUST` — turns on learning during candidate selection.
pub const DYNAMIC_ADJUST: u32 = 1 << 9;

/// Every pinyin ambiguity bit (`PINYIN_AMB_ALL`).
pub const PINYIN_AMB_ALL: u32 = 0x3ff << 10;

/// `SORT_BY_PHRASE_LENGTH_AND_PINYIN_LENGTH_AND_FREQUENCY`, a value of the
/// sort enumeration `pinyin_guess_candidates` takes.
pub const SORT_BY_PHRASE_LENGTH_AND_PINYIN_LENGTH_AND_FREQUENCY: u32 = 0x1e;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bit_values_match_the_public_headers() {
        assert_eq!(IS_PINYIN, 0x002);
        assert_eq!(IS_ZHUYIN, 0x004);
        assert_eq!(PINYIN_INCOMPLETE, 0x008);
        assert_eq!(ZHUYIN_INCOMPLETE, 0x010);
        assert_eq!(USE_TONE, 0x020);
        assert_eq!(FORCE_TONE, 0x040);
        assert_eq!(USE_DIVIDED_TABLE, 0x080);
        assert_eq!(USE_RESPLIT_TABLE, 0x100);
        assert_eq!(DYNAMIC_ADJUST, 0x200);
        assert_eq!(PINYIN_AMB_ALL, 0x000f_fc00);
        assert_eq!(SORT_BY_PHRASE_LENGTH_AND_PINYIN_LENGTH_AND_FREQUENCY, 0x1e);
    }

    #[test]
    fn ambiguity_bits_do_not_overlap_the_table_flags() {
        let table = IS_PINYIN
            | IS_ZHUYIN
            | PINYIN_INCOMPLETE
            | ZHUYIN_INCOMPLETE
            | USE_TONE
            | FORCE_TONE
            | USE_DIVIDED_TABLE
            | USE_RESPLIT_TABLE
            | DYNAMIC_ADJUST;
        assert_eq!(table & PINYIN_AMB_ALL, 0);
    }
}
