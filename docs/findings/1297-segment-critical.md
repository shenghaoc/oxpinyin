# Phrase segmentation writes no invalid-UTF-8 critical

Status: accepted, 2026-10-31 UTC, maintainer decision D1 of #1297 (a
narrow exception to the library stderr ruling, #545). Disposition:
POLICY-ACCEPTED. Rule: [source policy, no hand-written shims](../source-policy.md);
record form: [template](language-runtime-discrepancy-template.md).
Pin: libpinyin 074a2219 (`src/pinyin.cpp` blob `f27f7cf7`,
`src/zhuyin.cpp` blob `d1520c18`), read from a `git archive` of the pin;
GLib 2.84.4 of the reference image.

GLib documents `g_return_val_if_fail` as a precondition assertion on the
caller's arguments, to be used at the top of a public function
(https://docs.gtk.org/glib/warnings.html). Neither site follows that
pattern. `SubPhraseIndex::load` checks file data inside an internal
method. `phrase_segment` checks the result of conversions it has already
made, not its argument on entry. oxpinyin therefore implements the
control flow these checks express, an early return of the pin's value,
and does not reproduce the critical.

`pinyin_phrase_segment` and `zhuyin_phrase_segment` count the sentence's
characters with `g_utf8_strlen`, convert it with `g_utf8_to_ucs4`, and
check `g_return_val_if_fail(num_of_chars == ucs4_len, FALSE)`
(`pinyin.cpp:1450-1454`, `zhuyin.cpp:969-973`). Invalid UTF-8 makes the
conversion fail, so the check fails: the pin logs GLib's critical, in the
default domain, `bool pinyin_phrase_segment(pinyin_instance_t*, const
char*): assertion 'num_of_chars == ucs4_len' failed` (the zhuyin twin
with its own `G_STRFUNC`), and returns `FALSE` with the earlier phrase
result unchanged.

oxpinyin (`oxpinyin-capi` and `oxpinyin-zhuyin-capi`, `phrase.rs`) returns
`false` and keeps the earlier result, and makes no log call. Two
observable differences follow, both accepted:

- The critical is missing: a caller's GLib log handler receives nothing,
  and GLib's default handler writes nothing to stderr.
- Under GLib fatal-critical settings (`G_DEBUG=fatal-criticals`,
  `g_log_set_always_fatal`) the pin terminates with SIGTRAP at the critical.
  oxpinyin does not abort; it returns `false`. **The fatal-critical behaviour is lost.**
  This is an intentional observable difference, not exact compatibility.

Debian 13's GLib 2.84.4 uses `G_BREAKPOINT()` for a non-recursive fatal
log message. On the linux/amd64 reference host, `g_logv+0x309` is `int3`,
which raises SIGTRAP. The earlier SIGABRT bindings came from a newer GLib,
before the oracle moved to Debian stable; #1541 replaces those bindings
with captures from Debian 13 and GLib 2.84.4.

Return values, the phrase result and every other state are the pin's.
The NULL-sentence path is not covered by this record: it makes the pin's
own `g_utf8_strlen` call, whose critical is GLib's.

The record covers these cases, each in both facades:

- On bdb, kc and tkrzw: `glib-924-segment-critical-{pinyin,zhuyin}` and
  `issue542-phrase-segment-invalid-utf8-{pinyin,zhuyin}`, where only the pin
  logs the critical; and `glib-924-segment-fatal-critical-{pinyin,zhuyin}`,
  where the pin terminates with SIGTRAP and oxpinyin returns `false`.
- On tkrzw only: `axis-c-encoding-{pinyin,zhuyin}-phrase_segment-ff-start`,
  where only the pin logs the critical.

Matching controls are outside this policy record.
Unit tests: `surrogate_segment_matches_pin_length_and_no_critical` in both
facades.
