# g_utf8_next_char from a byte that leads no sequence — one-byte step

- **Status:** accepted · 2026-10-31 UTC · PR #1464
- **Disposition:** POLICY-ACCEPTED (never MATCH)
- **Policy limb:** the result depends on a violated library precondition
  ([no ported external-library internals](compatibility-policy.md),
  ruling 2026-10-29 UTC). GLib documents `g_utf8_next_char` for a valid
  UTF-8 string, with no error checking. For a macro that does work,
  oxpinyin implements what its contract says (maintainer ruling
  2026-10-31 UTC). How far the macro steps from a byte that leads no
  sequence is GLib's table, not libpinyin's product. Neither memory safety
  nor availability is involved, and a language-neutral requirement for the
  line tagger would not mention a five- or six-byte step.
- **Pin:** libpinyin 2.11.92 at 074a2219, `src/storage/tag_utility.cpp`
  blob `232a07dc`. GLib of the reference image: 2.84.4 (trixie). Facade:
  neither; tools only. Backend: none (the tagger reads text).
- **Pin derivation:** `split_line` (`tag_utility.cpp:118-169`) steps every
  character with `g_utf8_next_char` (`:122`, `:132`, `:136`, `:141`,
  `:154`). The macro adds `g_utf8_skip[*(const guchar *)p]`
  (`glib/gunicode.h`). The table (`glib/gutf8.c:115-124`) holds 1 for
  `0x00..=0xBF` and `0xFE..=0xFF`, 2 for `0xC0..=0xDF`, 3 for
  `0xE0..=0xEF`, 4 for `0xF0..=0xF7`, 5 for `0xF8..=0xFB`, and 6 for
  `0xFC..=0xFD`. `g_utf8_get_char` (`:333-345`, the macro block at
  `:45-114`) answers `(gunichar)-1` for such a lead followed by a byte that
  is not a continuation byte, and `g_unichar_isspace` and
  `g_unichar_isgraph` are false for it.
- **Rust counterpart:** `oxpinyin_input::tool::utf8_next_char_step`, the
  length of the UTF-8 sequence the byte leads (`u8::leading_ones` of 2, 3
  or 4), else 1. Its one caller is `oxpinyin_utils::taglib::split_line`.
  `g_utf8_get_char`, `g_unichar_isspace` and `g_unichar_isgraph` are still
  GLib's, through `glib-sys`.
- **Forward trace:** `k_mixture_model_to_interpolation` stdin →
  `std::getline` → `taglib_read` (`:171-264`) → `split_line` → the line's
  tokens → tag and value lookup → output or refusal.
- **Backward trace:** `utf8_next_char_step` has no other caller.
  `oxpinyin-data`'s interpolation grammar, `oxpinyin-kmm`'s text reader and
  the C facades decode UTF-8 through their own code and never called the
  table. The table generators (`gen_pinyin_table`, `gen_zhuyin_table`) and
  `ngseg` call GLib's own `g_utf8_strlen` and `g_utf8_to_ucs4` through
  `glib-sys`, which still step GLib's table and are unchanged. No C API
  path reaches the Rust step.
- **Input partition:**
  - Valid supported input: UTF-8 lines, whose every step starts at a lead
    byte. Both sides step the same: 1, 2, 3 or 4.
  - Malformed: a byte that GLib's table steps differently from the lead
    length. Only `0xF8..=0xFD` differ: continuation bytes (`0x80..=0xBF`)
    and `0xFE..=0xFF` step 1 on both sides, `0xC0..=0xC1` step 2 and
    `0xF5..=0xF7` step 4.
  - A truncated sequence at the line's end (a valid lead whose step passes
    the NUL) is unchanged: `split_line` refuses it as
    `Undefined::PastLineEnd`
    ([eio-kmm-to-interpolation-undefined-lines](eio-kmm-to-interpolation-undefined-lines.md)).
- **Difference:** the triggering input is a line with a byte in
  `0xF8..=0xFD`. The pin skips 5 or 6 bytes from it, oxpinyin skips 1, so
  the bytes after it are tokenized on oxpinyin's side and skipped on the
  pin's. Where the pin's step from such a byte passes the line's NUL, the
  pin reads past the string and has no defined outcome; oxpinyin reads only
  the line.
- **Why accepted:** the step from a byte outside the macro's precondition
  is GLib's internal table. The lead-byte length is what the contract
  describes, in plain Rust, and the table read that replaces it needed a
  hand-declared GLib data symbol, which MSVC cannot import without a
  Windows-only link attribute.
- **Contract preserved:** C ABI, ownership, persistent formats and the C
  facades are untouched. Valid UTF-8 tokenizes identically, so every
  tool's output on valid input is unchanged. Diagnostics are unchanged.
- **Affected inputs:** `k_mixture_model_to_interpolation` lines containing
  `0xF8..=0xFD`. Cases (`split_line`, oxpinyin observed, pin derived above):

  | Line bytes | Pin tokens | oxpinyin tokens |
  | --- | --- | --- |
  | `F8 'abcd e'` | `["", "e"]` | `["", "abcd", "e"]` |
  | `FC 'abcde f'` | `["", "f"]` | `["", "abcde", "f"]` |
  | `F8 'ab'` | reads past the NUL | `["", "ab"]` |
  | `F5 'abc d'` (control) | `["", "d"]` | `["", "d"]` |

- **Tests:** `split_line_steps_one_byte_from_a_byte_that_leads_no_sequence`
  (`crates/oxpinyin-utils/src/taglib.rs`) holds the oxpinyin tokens and
  the control, and `tool.rs`'s boundary test holds the step for every byte
  class. No `tool-diff` case reaches these bytes: the 16 registered
  `k_mixture_model_to_interpolation` cases feed `stdin.text`, a JSON
  string, so their input is valid UTF-8 by construction. The cases above
  are therefore unit cases with derived pin results, not contract-diff
  cases. The nightly `KMM live: to_interpolation` gate and the registered
  cases on the tkrzw cell cover valid input.
- **Prior rulings:** none reversed.
- **Evidence:** `just heavy -- cargo test -p oxpinyin-input -p
  oxpinyin-utils -p oxpinyin-training`; the pin derivation above, from the
  source.
- **Conclusion:** accepted, 2026-10-31 UTC.
