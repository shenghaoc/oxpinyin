# #918 — skipped prefix read hides a Berkeley DB format error

- **Status:** source-established difference; no exception admitted ·
  2026-10-24 UTC · #918. The early return is unchanged.
- **Disposition/classification:** defect under decision-order limb 4.
  This is backend error handling with a surviving session consequence,
  not a GLib/glibc/C++ incidental conversion detail. The bounded witness
  returns an error rather than aborting or accessing invalid memory, so
  class (b)/(c) does not automatically admit it. No valid-input difference
  is asserted: the call includes a non-scalar and damaged backing data.
- **Pin:** libpinyin source `074a2219`, both facades, bdb build;
  GLib 2.89.4-2/glibc 2.43-3. Dependency source db5.3
  `5.3.28+dfsg2-11`, binary `5.3.28+dfsg2-11+b1`, from the pinned snapshot.
- **Rust:** both facade `phrase.rs::{segment_text,*_phrase_segment}`,
  `SegmentText::Unmatched`, baseline `1c5d96b9`.

## Trigger and observable difference

Use input `e4 bd a0 ed a0 80 00` (`你`, U+D800, NUL), a sound matching
scalar prefix and scalar-only phrase keys/items. The system bigram hash
handle is open, but the bucket visited for sentence-start token 1 contains
a bounded key entry with an invalid key type (for example H_DUPLICATE in
the key position). Metadata and offsets remain valid so execution reaches
the explicit type check. This is a source-defined damaged-store condition,
not an executed corruption fixture or a claim that all damaged files open.

At the pin, the prefix calls `Bigram::load` before the non-scalar blocks
completion (`lookup/phrase_lookup.cpp:137-149,255-276`,
`storage/ngram_bdb.cpp:127-146`). In db5.3 source:

1. `src/db/db_iface.c:754-810` selects DB_SET for this zero-flag get.
2. `src/hash/hash.c:529-534,1884-1970` performs the hash lookup;
   `__ham_getindex` (`src/hash/hash_page.c:655-705`) rejects that key type.
3. `src/common/db_err.c:120-128` emits
   `page <n>: illegal page type or format` and calls `__env_panic`.
   `:200-240` sets environment panic and emits the PANIC diagnostic.
   `src/dbinc/debug.h:82-104` and `src/env/env_method.c:383` establish
   default stderr; the pin installs no alternate error channel.
4. The wrapper receives failure and leaves the system gram NULL. With
   other reads safe, segmentation can still finish false/all-zero. Its
   return alone therefore hides the difference: stderr already changed,
   and future accesses to that poisoned handle meet `PANIC_CHECK`
   (`src/dbinc/region.h:310-324`, `src/common/db_err.c:159-184`).

Rust takes its early return without that backend read, returns false with
two zero tokens and emits none of these diagnostics. It does not poison
the handle. Neither route trains or writes a gram in this call. The pin's
private backend-state mutation is nevertheless observable through later
operations/diagnostics, unlike the discarded normal-prefix score.

## Conditional binary check

Source has a `DIAGNOSTIC`-conditional process abort inside `__env_panic`.
The exact snapshot binaries were read on both architectures to resolve
that branch, without executing them. In `libdb-5.3.so`:

| Architecture | `__env_panic` offsets |
| --- | --- |
| amd64 | `+0x19` calls `__env_panic_set`; `+0x2e` calls `__db_err`; `+0x7b` loads DB_RUNRECOVERY and `+0x82` returns. |
| arm64 | `+0x20` calls `__env_panic_set`; `+0x34` calls `__db_err`; `+0x7c` loads DB_RUNRECOVERY and `+0x88` returns. |

There is no unconditional `__os_abort` on this path in either library.
The pin configures no panic/event callback that replaces that conclusion.
This check concerns the containing bdb dependency, not kc/tkrzw or the
unrelated conversion over-read. amd64 remains the reference.

Reproduce with the corresponding snapshot package and architecture's
objdump: `objdump -dlS -C --no-show-raw-insn --disassemble=__env_panic
<extracted-libdb-5.3.so>`. No disassembly dumps, captures, experiments or
new runtime gate results are published.

## Scope and conclusion

The result writers/getters and opposite-facade path are in the
[entry audit](918-phrase-segment-source-audit.md). Ordinary hits, missing
grams, short **values** in healthy DBs, silent kc/tkrzw returned errors,
OS signal faults and arbitrary corrupt-pointer execution are different
partitions. Healthy backing data cannot reach this format-error branch.

Conclusion: **the general prefix path is not identical on faulting
stores**. Preserve the separate normal-data equivalence, retain the early
return as instructed, and do not grant removal clearance or label this
diagnostic/state difference POLICY-ACCEPTED. No new ruling is needed to
identify the unadmitted difference; accepting it would need a separate
policy basis.
