# Phrase segmentation writes no invalid-UTF-8 critical

Status: accepted, 2026-10-31 UTC, maintainer decision D1 of #1297 (a
narrow exception to the library stderr ruling, #545). Disposition:
POLICY-ACCEPTED. Rule: [source policy, no hand-written shims](../source-policy.md);
record form: [template](language-runtime-discrepancy-template.md).
Pin: libpinyin 074a2219 (`src/pinyin.cpp` blob `f27f7cf7`,
`src/zhuyin.cpp` blob `d1520c18`), read from a `git archive` of the pin;
GLib 2.84.4 (recorded evidence; the reference image is unpinned Debian testing).

GLib documents `g_return_val_if_fail` as a precondition assertion on the
caller's arguments, to be used at the top of a public function
(https://docs.gtk.org/glib/warnings.html). `phrase_segment` does not follow
that pattern. It checks the result of conversions that it has already
made, not its argument on entry. Thus oxpinyin implements the
control flow that this check expresses, an early return of the pin's value.
It does not reproduce the critical.
[1297-layout-criticals.md](1297-layout-criticals.md) records the other D1 site, `SubPhraseIndex::load`.

`pinyin_phrase_segment` and `zhuyin_phrase_segment` do these steps:

1. Count the characters of the sentence with `g_utf8_strlen`.
2. Convert it with `g_utf8_to_ucs4`.
3. Check `g_return_val_if_fail(num_of_chars == ucs4_len, FALSE)`
   (`pinyin.cpp:1450-1454`, `zhuyin.cpp:969-973`).

Invalid UTF-8 makes the conversion fail, so the check fails. The pin then
logs GLib's critical in the default domain, `bool pinyin_phrase_segment(pinyin_instance_t*, const
char*): assertion 'num_of_chars == ucs4_len' failed` (the zhuyin twin
with its own `G_STRFUNC`). It returns `FALSE`, and the earlier phrase
result stays unchanged.

oxpinyin (`oxpinyin-capi` and `oxpinyin-zhuyin-capi`, `phrase.rs`) returns
`false`, keeps the earlier result and makes no log call. Two
observable differences follow, and the policy accepts both:

- The critical is missing. A GLib log handler of the caller receives nothing,
  and the default handler of GLib writes nothing to stderr.
- Under GLib fatal-critical settings (`G_DEBUG=fatal-criticals`,
  `g_log_set_always_fatal`) the pin terminates with SIGTRAP at the critical.
  oxpinyin does not abort; it returns `false`. **The fatal-critical behaviour is lost.**
  This is an intentional observable difference, not exact compatibility.

GLib 2.84.4 uses `G_BREAKPOINT()` for a non-recursive fatal
log message. On the linux/amd64 reference host, `g_logv+0x309` is `int3`,
which raises SIGTRAP. Recorded evidence: the bindings come from Debian 13
and GLib 2.84.4 captures (PR #1544, which fixed #1541). They replace SIGABRT bindings from a newer
GLib. The reference image is now unpinned Debian testing, and a difference between GLib versions is accepted
([host-dependent results](compatibility-policy.md#host-dependent-results)).

Return values, the phrase result and every other state are the pin's.
This record does not cover the NULL-sentence path. That path makes the pin's
own `g_utf8_strlen` call, and the critical of that call is GLib's.

The record covers these cases, each in both facades:

- On bdb, kc and tkrzw: `glib-924-segment-critical-{pinyin,zhuyin}` and
  `issue542-phrase-segment-invalid-utf8-{pinyin,zhuyin}`. Only the pin
  logs the critical.
- On bdb, kc and tkrzw: `glib-924-segment-fatal-critical-{pinyin,zhuyin}`.
  The pin terminates with SIGTRAP, and oxpinyin returns `false`.
- On tkrzw only: `axis-c-encoding-{pinyin,zhuyin}-phrase_segment-ff-start`.
  Only the pin logs the critical.

Matching controls are outside this policy record.
Unit tests: `surrogate_segment_matches_pin_length_and_no_critical` in both
facades.
