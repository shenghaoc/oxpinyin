# Design — Stage 2: bug-for-bug compatibility

## Overview

One loop per behaviour: read the pin, write it down, implement it,
gate it. Running the pin is the last step's job, never the first's.

## Reading ladder

Stop at the first rung that settles the behaviour.

1. **The pin's source** at the commit in `tools/oracle/oracle-pin.txt`.
   Read from a checkout at that commit, or from a clone whose cited blobs
   you have hashed against it, and say which tree you read.
2. **The functions it calls**, where the behaviour turns on them: glib
   (`g_*`), libc, the DBM's C or C++ API — at the versions the reference
   container ships.
3. **The disassembly of the pin-built object that holds the code** —
   from `tools/oracle/build-oracle.sh`:
   - the Berkeley DB reference-cell build by default;
   - the Kyoto Cabinet or tkrzw build (`--dbm kc|tkrzw`) when the
     behaviour is backend-specific — that code (for example
     `chewing_large_table2_kyotodb.cpp`) is compiled only into its own
     build;
   - `libzhuyin.so.15` from an `--enable-libzhuyin` build for the zhuyin
     facade.

   The pin's build uses autoconf's default `-g -O2` with no `-DNDEBUG`,
   so line tables are present and every `assert` is compiled in:
   - `objdump -dlS -C --no-show-raw-insn <prefix>/lib/libpinyin.so.15`
   - `gdb -batch -ex 'disassemble /s <symbol>' <prefix>/lib/libpinyin.so.15`

   Use it for macros and templates that cannot be expanded by eye,
   inlined library helpers, implementation-defined or undefined
   behaviour, and the evaluation order the compiler chose.
4. **The object's contents** — `readelf -sW`, `.rodata`, relocations —
   when the question is what the binary contains rather than what it
   executes.

If the ladder runs out, STOP (Requirement 2.4).

## Writing it down

- The change's report cites the derivation: `path:line` at the pin, or
  symbol plus instruction offsets.
- A pin defect reproduced on purpose gets an upstream-report entry in
  `docs/findings/upstream-report-drafts.md`.
- A departure is argued into a class in
  `docs/findings/compatibility-policy.md`, with a register row, or into
  the language/runtime-quirk exception with a full record from
  `docs/findings/language-runtime-discrepancy-template.md` and a
  register entry. The record walks the path forwards and backwards and
  proves the C contract and valid-input behaviour unchanged.
- A quirk admitted by a record carries a `POLICY-ACCEPTED`
  contract-diff case, never `MATCH`; the case keeps its pin and subject
  evidence.

## Math

Read the pin's arithmetic and operation order where it decides
behaviour. oxpinyin uses Rust's standard maths at full speed — `ln`,
`exp`, `powf` and every other `std` float method — and never slows code
down to imitate glibc, libm, GLib `gfloat` or any other C behaviour. A
last-bit difference from the pin that follows from using Rust's maths
is class (a): record it, do not chase it.

## Complexity (axis L)

Measured, not read: complexity is the one Stage 2 question the reading
ladder does not answer, because it is about cost, not output. Run both
sides on a named workload in the same container — whole-process
instructions (callgrind) and peak heap (massif), three repeats, spread
stated — and attribute any regression to a source component on both
sides, as #757 does. A regression on both axes must be fixed; a written
justification does not replace the fix. A fix makes oxpinyin do the work
the way the pin does, or a better way. Output stays byte-identical and
its differential stays the gate. A threshold such as the 1.10× ratio of
#757 and #870 applies to the workloads of that issue only.

## Internal improvement

Stage 2 accepts any internal change that keeps observable output, the
files on disk and the ABI unchanged, performance included. oxpinyin may
be faster or smaller than the pin by any amount. Examples: `memmap2` in
place of the mmap wrapper (#862), `memchr` substring search (#858).

The internal items of the 2026-09 direction belong here when they keep
compatibility:

- dictionary cursor walk for lattice building;
- incremental per-keystroke lattice;
- per-session arena;
- ordered atomics off the steady cycle;
- host-independent cost derivation, if output stays identical to the
  pin on every cell. If it changes output, it is a class (a) question or
  a Stage 3 departure.

Known leads from the measurements so far:

- Steady-cycle allocation churn — the IR differential's target
  (`docs/findings/perf-cycle-ir-differential-2026-09-08.md`).
- arm64 runs about 10% behind the pin where amd64 is at parity
  (`docs/findings/perf-steady-cycle-cross-host-2026-09-07.md`). The open
  leads are memory-ordering cost and the scan-matrix fan-out: 4.11 against
  1.32 paths per window (`docs/findings/scan-matrix-fanout-2026-09-10.md`).
- The RSS gap traces to 3.07× dictionary probe counts that fault Kyoto
  Cabinet leaf pages (`docs/findings/rss-attribution-2026-09-09.md`).

## Gating

The differential is written after the derivation, to hold it: reference
cell (all three when the backend path check fires), shown non-vacuous
once — by reverting the fix, or, where oxpinyin already reproduced the
behaviour and there is no fix, by a deliberate named mutation of
oxpinyin's side — and cases added to an existing driver
(`/AGENTS.md` "Verification cells"). A differential that disagrees with
the derivation sends the agent back to the reading ladder — the source
is re-read, not the implementation tuned.
