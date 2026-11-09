# g_utf8_next_char from a byte that leads no sequence — one-byte step

- **Status:** accepted · 2026-10-31 UTC · PR #1464
- **Disposition:** POLICY-ACCEPTED (never MATCH)
- **Policy limb:** the result depends on a violated library precondition
  ([no ported external-library internals](compatibility-policy.md),
  ruling 2026-10-29 UTC). GLib documents `g_utf8_next_char` for a valid
  UTF-8 string, with no error checking. For a macro that does work,
  oxpinyin implements what its contract says (maintainer ruling
  2026-10-31 UTC). GLib's table, not libpinyin's product, sets how far the
  macro steps from a byte that leads no sequence. Neither memory safety
  nor availability is involved. A language-neutral requirement for the
  line tagger would not mention a five- or six-byte step.
- **Pin:** libpinyin 2.11.92 at 074a2219, `src/storage/tag_utility.cpp`
  blob `232a07dc`. GLib at the time of the record: 2.84.4 (recorded evidence). Facade:
  neither; tools only. Backend: none (the tagger reads text).
- **Pin derivation:** `split_line` (`tag_utility.cpp:118-169`) steps every
  character with `g_utf8_next_char` (`:122`, `:132`, `:136`, `:141`,
  `:154`). The macro adds `g_utf8_skip[*(const guchar *)p]`
  (`glib/gunicode.h`). The table (`glib/gutf8.c:115-124`) holds 1 for
  `0x00..=0xBF` and `0xFE..=0xFF`, 2 for `0xC0..=0xDF`, 3 for
  `0xE0..=0xEF`, 4 for `0xF0..=0xF7`, 5 for `0xF8..=0xFB`, and 6 for
  `0xFC..=0xFD`. Take such a lead followed by a byte that is not a
  continuation byte. For it, `g_utf8_get_char` (`:333-345`, the macro block
  at `:45-114`) returns `(gunichar)-1`, and `g_unichar_isspace` and
  `g_unichar_isgraph` are false.
- **Rust counterpart:** `oxpinyin_input::tool::utf8_next_char_step`. It
  returns the length of the UTF-8 sequence that the byte leads
  (`u8::leading_ones` of 2, 3 or 4), else 1. Its one caller is `oxpinyin_kmm::taglib::split_line`
  (`crates/oxpinyin-kmm/src/taglib.rs`, re-exported as
  `oxpinyin_utils::taglib`).
  `g_utf8_get_char`, `g_unichar_isspace` and `g_unichar_isgraph` stay
  GLib's, through `glib-sys`.
- **Forward trace:** `k_mixture_model_to_interpolation` stdin →
  `std::getline` → `taglib_read` (`:171-264`) → `split_line` → the line's
  tokens → tag and value lookup → output or refusal.
- **Backward trace:** `utf8_next_char_step` has no other caller.
  The interpolation grammar of `oxpinyin-data`, the text reader of
  `oxpinyin-kmm` and the C facades decode UTF-8 through their own code.
  They do not call the table. The table generators (`gen_pinyin_table`,
  `gen_zhuyin_table`) and `ngseg` call GLib's own `g_utf8_strlen` and
  `g_utf8_to_ucs4` through `glib-sys`. These functions step GLib's table
  and are unchanged. No C API path reaches the Rust step.
- **Input partition:**
  - Valid supported input: UTF-8 lines, in which each step starts at a lead
    byte. Both sides step the same: 1, 2, 3 or 4.
  - Malformed: a byte for which GLib's table step differs from the lead
    length. Only `0xF8..=0xFD` differ. Continuation bytes (`0x80..=0xBF`)
    and `0xFE..=0xFF` step 1 on both sides, `0xC0..=0xC1` step 2, and
    `0xF5..=0xF7` step 4.
  - A truncated sequence at the end of the line (a valid lead whose step
    passes the NUL) is unchanged. `split_line` refuses it as
    `Undefined::PastLineEnd`
    ([eio-kmm-to-interpolation-undefined-lines](eio-kmm-to-interpolation-undefined-lines.md)).
- **Difference:** the triggering input is a line with a byte in
  `0xF8..=0xFD`. From that byte, the pin skips 5 or 6 bytes and oxpinyin
  skips 1. Thus oxpinyin tokenizes the bytes after it, and the pin skips
  them. Where the pin's step from such a byte passes the NUL of the line,
  the pin reads past the string and has no defined outcome. oxpinyin reads
  only the line.
- **Why accepted:** GLib's internal table sets the step from a byte outside
  the precondition of the macro. The contract describes the lead-byte
  length, which plain Rust computes. A read of the table needs a
  hand-declared GLib data symbol. MSVC cannot import that symbol without a
  Windows-only link attribute.
- **Contract preserved:** C ABI, ownership, persistent formats and the C
  facades are unchanged. Valid UTF-8 tokenizes identically, so the output
  of each tool on valid input is unchanged. Diagnostics are unchanged.
- **Affected inputs:** `k_mixture_model_to_interpolation` lines containing
  `0xF8..=0xFD`. The cases below are for `split_line`. The oxpinyin tokens
  are observed; the pin tokens are derived above.

  | Line bytes | Pin tokens | oxpinyin tokens |
  | --- | --- | --- |
  | `F8 'abcd e'` | `["", "e"]` | `["", "abcd", "e"]` |
  | `FC 'abcde f'` | `["", "f"]` | `["", "abcde", "f"]` |
  | `F8 'ab'` | reads past the NUL | `["", "ab"]` |
  | `F5 'abc d'` (control) | `["", "d"]` | `["", "d"]` |

- **Tests:** `split_line_steps_one_byte_from_a_byte_that_leads_no_sequence`
  (`crates/oxpinyin-kmm/src/taglib.rs`) holds the oxpinyin tokens and the
  control. The `tool.rs` boundary test of `oxpinyin-input` holds the step
  for each byte class. The frozen `k_mixture_model_to_interpolation`
  cases in `crates/oxpinyin-utils/tests/` feed valid UTF-8 text, so none
  reaches these bytes. The cases above are unit cases with pin results
  derived from source.
- **Prior rulings:** none reversed.
- **Evidence:** the pin derivation above, from source, and the tests
  above (`cargo test -p oxpinyin-input -p oxpinyin-kmm -p oxpinyin-utils`).
- **Conclusion:** accepted, 2026-10-31 UTC.
