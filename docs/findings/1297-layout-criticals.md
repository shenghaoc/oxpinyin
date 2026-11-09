# Phrase-library layout faults write no critical

Status: accepted, 2026-10-31 UTC, maintainer decision D1 of #1297 (a
narrow exception to the library stderr ruling, #545). Disposition:
POLICY-ACCEPTED. Rule: [source policy, no hand-written shims](../source-policy.md);
record form: [template](language-runtime-discrepancy-template.md).
Pin: libpinyin 074a2219 (`src/storage/phrase_index.cpp` blob `d58e37e2`,
`src/pinyin.cpp` blob `f27f7cf7`, `src/zhuyin.cpp` blob `d1520c18`), read
from a `git archive` of the pin; GLib 2.84.4 (recorded evidence;
the reference image is unpinned Debian testing).

GLib documents `g_return_val_if_fail` as a precondition assertion on the
caller's arguments, to be used at the top of a public function
(https://docs.gtk.org/glib/warnings.html). Neither site follows that
pattern. `SubPhraseIndex::load` checks file data inside an internal
method. `phrase_segment` checks the result of conversions it has already
made, not its argument on entry. Thus oxpinyin implements the
control flow that these checks express, an early return of the pin's value.
It does not reproduce the critical.

`SubPhraseIndex::load` reads `m_total_freq` and the three offsets, then
checks the three separators and the end with `g_return_val_if_fail`
(`phrase_index.cpp:354-356`, `:361`). Some chunks have a header and checksum
that `MemoryChunk::mmap` accepts, but a layout that fails a check:

- a header-only chunk;
- a sparse chunk whose truncated payload is empty;
- a misplaced second or third separator.

For such a chunk, the pin logs GLib's critical in the
default domain, `bool pinyin::SubPhraseIndex::load(pinyin::MemoryChunk*,
table_offset_t, table_offset_t): assertion '<expr>' failed`, and returns
`FALSE`. Every caller ignores that return value (`pinyin.cpp:264`, `:298`,
`:313`; `zhuyin.cpp:208`, `:242`) and continues with an empty sub-index.
These paths reach it: the init, a library load or reload, the remap of each
system library in the save (`pinyin.cpp:956`, `zhuyin.cpp:589`), and the
reload in `mask_out` (`pinyin.cpp:1265`, `zhuyin.cpp:800`).

oxpinyin keeps all of that: the empty sub-index, its local `m_total_freq`
outside the facade total, the diff of the save against the empty sub-index,
and every return value and file. It makes no log call. oxpinyin has no fault
queue from `oxpinyin-data` through the runtime and facade to the C adapters,
and no copied `G_STRFUNC` string or copied expressions. `LayoutFault` keeps
only `total_freq`. Two observable differences follow, and the policy accepts both:

- The critical is missing. A GLib log handler of the caller receives nothing,
  and the default handler of GLib writes nothing to stderr.
- Under GLib fatal-critical settings, the pin terminates with SIGTRAP at
  the critical. An example is the save, after it writes the files of the
  earlier libraries. oxpinyin does not abort; it completes the operation.
  **The fatal-critical behaviour is lost.** This is an intentional
  observable difference, not exact compatibility.

The pin's signal depends on the GLib that it links. GLib 2.84.4
ends a non-recursive fatal log message in `_g_log_abort` with
`G_BREAKPOINT()` when a debugger might be attached. On Unix, it
always assumes that one is
([gmessages.c:409-434](https://gitlab.gnome.org/GNOME/glib/-/blob/2.84.4/glib/gmessages.c#L409-L434),
called at `:1287`). `G_BREAKPOINT()` is `int $03` on x86-64 and
`raise (SIGTRAP)` on Linux aarch64
([gbacktrace.h:58-70](https://gitlab.gnome.org/GNOME/glib/-/blob/2.84.4/glib/gbacktrace.h#L58-L70)),
so both reference architectures observe SIGTRAP (exit `-5`). GLib
`a184d5d3` (first released in 2.87.0, in Debian testing's 2.88 and 2.90)
is different. It reads `TracerPid` from `/proc/self/status` and calls
`g_abort()` when no tracer is attached
([gmessages.c:408-455](https://gitlab.gnome.org/GNOME/glib/-/blob/2.87.0/glib/gmessages.c#L408-L455)),
which is SIGABRT (`-6`).

Recorded evidence: the `save-fatal-valid-to-invalid*` entries were
first captured under that newer GLib (#1509), before the oracle images
moved to Debian 13 (`d3457495`). The images now use unpinned Debian testing.
They were captured again on GLib 2.84.4, as
were those of [1297-segment-critical.md](1297-segment-critical.md) (#1544).
Only the pin's exit changed. The pin's stderr, result and files and the
subject's observation are unchanged. The fixture is not involved.

A layout the pin reaches only through an out-of-chunk read stays the
existing class (b) refusal (policy row 76). This record does not affect it.

These contract cases are POLICY-ACCEPTED on tkrzw, bdb and kc. Each differs from
the pin only in the missing critical. For `save-fatal-valid-to-invalid*`, the difference
is the pin's SIGTRAP under the fatal mask, where oxpinyin completes the save:
`header-only-library-init`, `header-only-library-load`, `header-only-library-save`, `nonzero-total-save`, `save-fatal-valid-to-invalid`, `save-fatal-valid-to-invalid-zhuyin`, `save-replacement-invalid-to-valid`, `save-replacement-valid-to-invalid`, `save-replacement-valid-to-invalid-zhuyin`, `save-unchanged-layout-failed`, `second-separator-library-init`, `second-separator-library-init-zhuyin`, `short-chunk-layout`, `sparse-library-addon`, `sparse-library-addon-reload`, `sparse-library-candidates`, `sparse-library-candidates-zhuyin`, `sparse-library-init`, `sparse-library-init-zhuyin`, `sparse-library-load`, `sparse-library-load-zhuyin`, `sparse-library-mask-out`, `sparse-library-mask-out-zhuyin`, `sparse-library-reinit`, `sparse-library-reinit-zhuyin`, `sparse-library-save`, `sparse-library-save-zhuyin`, `sparse-library-tokens`, `sparse-library-tokens-zhuyin` and `third-separator-library-init`.

Nine more cases were POLICY-ACCEPTED under this record until `c41dfabd`
(2026-11-06 UTC) removed them from the contract case set: `header-only-library-init-zhuyin`, `header-only-library-load-zhuyin`, `nonzero-total-save-zhuyin`, `save-replacement-invalid-to-valid-zhuyin`, `save-unchanged-layout-failed-zhuyin`, `short-chunk-layout-zhuyin`, `sparse-library-init-settled`, `sparse-library-init-settled-zhuyin` and `third-separator-library-init-zhuyin`.
That change kept one case per caller and facade, each broken-library form at
least once, each facade's save and load path and every distinct outcome. None
of the nine was MATCH.

`oxpinyin-capi`'s `pinned_data_tests::a_save_over_a_failed_layout_journals_against_an_empty_index`
holds oxpinyin's value for `save-replacement-valid-to-invalid` and names this record.
`oxpinyin-data`'s `phrase_library::sparse_mapping_tests::a_sparse_chunk_maps_the_whole_file` and
`oxpinyin-user`'s `store_libpinyin::tests::layout_failed_sparse_library_keeps_and_releases_whole_mapping`
hold the sparse-library init of `sparse-library-init` and its zhuyin twin.
The tool case `estimate_interpolation-layout-refusal-total` is also accepted
on every cell for the missing critical. Its refused library has a nonzero local total,
which the facade excludes from the unigram denominator. Its frozen case in
`crates/oxpinyin-utils/tests/training.rs` holds oxpinyin's side.

Class (c) is retired (2026-10-31 UTC). The `nonzero-total-candidates` case and
its zhuyin twin abort on both sides at `pinyin.cpp:1859` and `zhuyin.cpp:1261`.
The pin's critical is in the result document that the abort discards. Thus
the `sigabrt` matcher ([pin-abort-assert-line.md](pin-abort-assert-line.md))
compares these cases, not this record.
Controls that still MATCH include `crash-oob-layout*`, `save-unchanged-valid*`
and `save-replacement-valid-to-different-valid*`.
