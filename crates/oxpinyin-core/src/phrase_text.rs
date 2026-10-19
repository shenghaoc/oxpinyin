//! A phrase's stored text, and the string the pin makes of it.
//!
//! The pin keeps a phrase as `ucs4_t phrase[L]` and the item header's `L`
//! (`PhraseItem::get_phrase_length`, `PhraseItem::get_phrase_string`). It
//! makes a string only where a caller needs one: `g_ucs4_to_utf8(phrase, L,
//! NULL, NULL, NULL)` in `_token_get_phrase` (`pinyin.cpp:1644`,
//! `zhuyin.cpp:1112`), `convert_to_utf8` (`lookup.cpp:54,58`) and the
//! export iterators (`pinyin.cpp:715,822,828`). GLib 2.89.4, the GLib of
//! the pinned oracle image (`glib/gutf8.c:1023-1070`), reads the code
//! points in order and **stops at the first zero** (`:1037-1038`): what
//! follows is never examined, not even for a value out of range. A value
//! of 0x80000000 or more before that point makes the whole string NULL
//! (`:1040-1045`).
//!
//! oxpinyin keeps a stored phrase as one `char` per `ucs4_t`, so that
//! `text.chars().count()` stays the item's stored length wherever the pin
//! reads the length from the header (the sort key, the longer-candidate
//! gate, the trellis, `*len` of `pinyin_token_get_phrase`) and the text
//! stays the item's own bytes where the pin keys an index on them. The
//! pin's string is then the part before the first U+0000
//! ([`phrase_c_string`]), taken wherever the pin makes a string.
//! [`ucs4_phrase_char`] is the one place the conversion's reading of the
//! stored words lives.

/// One stored `ucs4_t` as a character, in a phrase read front to back.
///
/// `converting` starts `true` and goes `false` at the first zero, the
/// code point `g_ucs4_to_utf8` stops at. Until then a value that is not a
/// Unicode scalar value is `None` — the pin hands out a string that is not
/// UTF-8 (a surrogate, a value above U+10FFFF) or none at all (0x80000000
/// and above), which a `str` cannot hold. Past the zero the pin never reads
/// the word, so the character only keeps the phrase's length: a value that
/// is no scalar value stands as U+FFFD and nothing refuses the phrase.
#[must_use]
pub fn ucs4_phrase_char(code: u32, converting: &mut bool) -> Option<char> {
    if !*converting {
        return Some(char::from_u32(code).unwrap_or(char::REPLACEMENT_CHARACTER));
    }
    *converting = code != 0;
    char::from_u32(code)
}

/// [`ucs4_phrase_char`] over a whole stored phrase.
pub fn ucs4_phrase_chars(
    codes: impl IntoIterator<Item = u32>,
) -> impl Iterator<Item = Option<char>> {
    let mut converting = true;
    codes
        .into_iter()
        .map(move |code| ucs4_phrase_char(code, &mut converting))
}

/// The string the pin makes of a phrase kept one character per stored
/// `ucs4_t` (see the module): the text up to its first U+0000.
///
/// A text with no U+0000, the only kind a UTF-8 import can make, is
/// returned whole.
#[must_use]
pub fn phrase_c_string(text: &str) -> &str {
    text.split_once('\0').map_or(text, |(string, _)| string)
}

/// Whether a phrase kept one character per stored `ucs4_t` holds a U+0000,
/// that is, whether [`phrase_c_string`] has anything to cut.
///
/// The decoder that reads the stored words learns this for free (its
/// `converting` flag in [`ucs4_phrase_char`] clears at the first zero) and
/// hands it on with the text, so a dictionary hit is not scanned again by
/// every reader of its string.
#[must_use]
pub fn holds_zero(text: &str) -> bool {
    text.as_bytes().contains(&0)
}

#[cfg(test)]
mod tests {
    use super::{phrase_c_string, ucs4_phrase_chars};

    fn text(codes: &[u32]) -> Option<String> {
        ucs4_phrase_chars(codes.iter().copied()).collect()
    }

    #[test]
    fn a_phrase_with_no_zero_is_read_whole() {
        assert_eq!(text(&[0x4f60, 0x597d]).as_deref(), Some("你好"));
        assert_eq!(text(&[]).as_deref(), Some(""));
    }

    #[test]
    fn a_zero_is_kept_and_the_rest_counts_toward_the_length() {
        assert_eq!(text(&[0x59d4, 0, 0x4f1a]).as_deref(), Some("委\0会"));
        assert_eq!(text(&[0]).as_deref(), Some("\0"));
        assert_eq!(text(&[0, 0, 0]).as_deref(), Some("\0\0\0"));
    }

    #[test]
    fn a_value_the_pin_converts_but_cannot_be_a_char_refuses_the_phrase() {
        for bad in [
            0xd800,
            0xdfff,
            0x11_0000,
            0x7fff_ffff,
            0x8000_0000,
            u32::MAX,
        ] {
            assert_eq!(text(&[0x59d4, bad, 0x4f1a]), None, "{bad:#x}");
            assert_eq!(text(&[bad]), None, "{bad:#x}");
        }
    }

    #[test]
    fn a_value_after_the_first_zero_is_never_read() {
        // `g_ucs4_to_utf8` breaks out of its loop at the zero, so the
        // out-of-range words behind it neither fail the string nor reach it.
        for bad in [0xd800, 0x11_0000, 0x8000_0000, u32::MAX] {
            assert_eq!(
                text(&[0x4fc4, 0, bad, 0x7fff_ffff]).as_deref(),
                Some("俄\0\u{fffd}\u{fffd}"),
                "{bad:#x}"
            );
        }
        // The same words before the zero still refuse it.
        assert_eq!(text(&[0x4fc4, 0x8000_0000, 0]), None);
    }

    #[test]
    fn the_string_is_the_text_up_to_the_first_zero() {
        assert_eq!(phrase_c_string("你好"), "你好");
        assert_eq!(phrase_c_string(""), "");
        assert_eq!(phrase_c_string("委\0会"), "委");
        assert_eq!(phrase_c_string("\0务院"), "");
        assert_eq!(phrase_c_string("办公\0"), "办公");
        assert_eq!(phrase_c_string("俄\0\0\u{fffd}"), "俄");
    }
}
