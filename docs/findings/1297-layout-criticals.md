# Phrase-library layout faults write no critical

Status: accepted, 2026-10-31 UTC, maintainer decision D1 of #1297 (a
narrow exception to the library stderr ruling, #545). Disposition:
POLICY-ACCEPTED. Rule: [source policy, no hand-written shims](../source-policy.md);
record form: [template](language-runtime-discrepancy-template.md).
Pin: libpinyin 074a2219 (`src/storage/phrase_index.cpp` blob `d58e37e2`,
`src/pinyin.cpp` blob `f27f7cf7`, `src/zhuyin.cpp` blob `d1520c18`), read
from a `git archive` of the pin; GLib 2.84.4 of the reference image.

GLib documents `g_return_val_if_fail` as a precondition assertion on the
caller's arguments, to be used at the top of a public function
(https://docs.gtk.org/glib/warnings.html). Neither site follows that
pattern. `SubPhraseIndex::load` checks file data inside an internal
method. `phrase_segment` checks the result of conversions it has already
made, not its argument on entry. oxpinyin therefore implements the
control flow these checks express, an early return of the pin's value,
and does not reproduce the critical.

`SubPhraseIndex::load` reads `m_total_freq` and the three offsets, then
checks the three separators and the end with `g_return_val_if_fail`
(`phrase_index.cpp:354-356`, `:361`). A chunk whose header and checksum
`MemoryChunk::mmap` accepts but whose layout fails a check (a header-only
chunk, a sparse chunk whose truncated payload is empty, a misplaced
second or third separator) makes the pin log GLib's critical in the
default domain, `bool pinyin::SubPhraseIndex::load(pinyin::MemoryChunk*,
table_offset_t, table_offset_t): assertion '<expr>' failed`, and return
`FALSE`. Every caller ignores that answer (`pinyin.cpp:264`, `:298`,
`:313`; `zhuyin.cpp:208`, `:242`) and goes on with an empty sub-index.
The init, a library load or reload, the save's remap of each system
library (`pinyin.cpp:956`, `zhuyin.cpp:589`) and `mask_out`'s reload
(`pinyin.cpp:1265`, `zhuyin.cpp:800`) reach it.

oxpinyin keeps all of that: the empty sub-index, its local `m_total_freq`
outside the facade total, the save's diff against the empty sub-index and
every return value and file. It makes no log call. The fault queue that
carried each fault from `oxpinyin-data` through the runtime and facade to
the C adapters, the copied `G_STRFUNC` string and the copied expressions
are gone; `LayoutFault` keeps only `total_freq`. Two observable
differences follow, both accepted:

- The critical is missing: a caller's GLib log handler receives nothing,
  and GLib's default handler writes nothing to stderr.
- Under GLib fatal-critical settings the pin terminates with SIGTRAP at
  the critical, for example in the save after the earlier libraries'
  files are written. oxpinyin does not abort; it completes the operation.
  **The fatal-critical behaviour is lost.** This is an intentional
  observable difference, not exact compatibility.

The pin's signal depends on the GLib it links. Debian 13's GLib 2.84.4
ends a non-recursive fatal log message in `_g_log_abort` with
`G_BREAKPOINT()` whenever a debugger might be attached, and on Unix it
always assumes one is
([gmessages.c:409-434](https://gitlab.gnome.org/GNOME/glib/-/blob/2.84.4/glib/gmessages.c#L409-L434),
called at `:1287`). `G_BREAKPOINT()` is `int $03` on x86-64 and
`raise (SIGTRAP)` on Linux aarch64
([gbacktrace.h:58-70](https://gitlab.gnome.org/GNOME/glib/-/blob/2.84.4/glib/gbacktrace.h#L58-L70)),
so both reference architectures observe SIGTRAP (exit `-5`). GLib
`a184d5d3` (first released in 2.87.0, in Debian testing's 2.88 and 2.90)
reads `TracerPid` from `/proc/self/status` and calls `g_abort()` when
no tracer is attached
([gmessages.c:408-455](https://gitlab.gnome.org/GNOME/glib/-/blob/2.87.0/glib/gmessages.c#L408-L455)),
which is SIGABRT (`-6`). The `save-fatal-valid-to-invalid*` entries were
first captured under that newer GLib (#1509), before the oracle images
moved to Debian 13 (`d3457495`); they were recaptured on GLib 2.84.4 as
[1297-segment-critical.md](1297-segment-critical.md)'s were (#1544).
Only the pin's exit changed; its stderr, result and files and the
subject's observation are unchanged, and the fixture is not involved.

A layout the pin reaches only through an out-of-chunk read stays the
existing class (b) refusal (policy row 76); it is not affected.

Cases, bound below with `just policy-record` from tkrzw, bdb and kc captures (each
differs from the pin only in the missing critical, or for
`save-fatal-valid-to-invalid*` in the pin's SIGTRAP under the fatal mask):
`header-only-library-init`, `header-only-library-init-zhuyin`, `header-only-library-load`, `header-only-library-load-zhuyin`, `header-only-library-save`, `nonzero-total-save`, `nonzero-total-save-zhuyin`, `save-fatal-valid-to-invalid`, `save-fatal-valid-to-invalid-zhuyin`, `save-replacement-invalid-to-valid`, `save-replacement-invalid-to-valid-zhuyin`, `save-replacement-valid-to-invalid`, `save-replacement-valid-to-invalid-zhuyin`, `save-unchanged-layout-failed`, `save-unchanged-layout-failed-zhuyin`, `second-separator-library-init`, `second-separator-library-init-zhuyin`, `short-chunk-layout`, `short-chunk-layout-zhuyin`, `sparse-library-addon`, `sparse-library-addon-reload`, `sparse-library-candidates`, `sparse-library-candidates-zhuyin`, `sparse-library-init`, `sparse-library-init-settled`, `sparse-library-init-settled-zhuyin`, `sparse-library-init-zhuyin`, `sparse-library-load`, `sparse-library-load-zhuyin`, `sparse-library-mask-out`, `sparse-library-mask-out-zhuyin`, `sparse-library-reinit`, `sparse-library-reinit-zhuyin`, `sparse-library-save`, `sparse-library-save-zhuyin`, `sparse-library-tokens`, `sparse-library-tokens-zhuyin`, `third-separator-library-init`, `third-separator-library-init-zhuyin`.
`oxpinyin-capi`'s `pinned_data_tests::a_save_over_a_failed_layout_journals_against_an_empty_index`
holds oxpinyin's value for `save-replacement-valid-to-invalid` and names this record.
`oxpinyin-data`'s `phrase_library::sparse_mapping_tests::a_sparse_chunk_maps_the_whole_file` and
`oxpinyin-user`'s `store_libpinyin::tests::layout_failed_sparse_library_keeps_and_releases_whole_mapping`
hold the sparse-library init of `sparse-library-init` and its zhuyin twin.
The tool case `estimate_interpolation-layout-refusal-total` also binds, on every cell,
the missing critical: its refused library has a nonzero local total, which
the facade excludes from the unigram denominator.
Since class (c) was retired (2026-10-31 UTC), the
`nonzero-total-candidates` case and its zhuyin twin abort on both sides at
`pinyin.cpp:1859` and `zhuyin.cpp:1261`:
the pin's critical is in the result document the abort discards, so the
cases are compared by the `sigabrt` matcher ([pin-abort-assert-line.md](pin-abort-assert-line.md)),
not this record.
Controls that still MATCH include `crash-oob-layout*`, `save-unchanged-valid*`
and `save-replacement-valid-to-different-valid*`.

Policy evidence is in [1297-layout-criticals.json](../../fixtures/contract-policy/1297-layout-criticals.json).
