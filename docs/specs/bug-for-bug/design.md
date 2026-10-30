# Design — Stage 2: bug-for-bug compatibility

## Overview

For each behaviour, read the pin, record the rule, implement it in idiomatic Rust, then run the gate.
Run the pin only at the last step.
Reading distinguishes product behaviour from language artefacts.
Record artefacts instead of reproducing them (Requirement 6).

## Reading ladder

Stop at the first step that settles the behaviour:

1. Read source at the commit in `tools/oracle/oracle-pin.txt`.
   Use a checkout at that commit or a clone whose cited blob hashes match it.
   Identify the tree read.
2. Read called functions when they determine behaviour.
   Use GLib `g_*`, libc and DBM C/C++ sources at the reference container's versions.
   Identify product behaviour and artefacts, rather than reproducing library internals.
3. Read disassembly of the relevant object from `tools/oracle/build-oracle.sh`.
   Use the tkrzw reference build by default.
   Use `--dbm bdb|kc` for backend-specific code, such as `chewing_large_table2_kyotodb.cpp`.
   That code exists only in its own build.
   Use `libzhuyin.so.15` from an `--enable-libzhuyin` build for Zhuyin.
   Autoconf's `-g -O2` without `-DNDEBUG` keeps line tables and assertions.
   Use `objdump -dlS -C --no-show-raw-insn <prefix>/lib/libpinyin.so.15`.
   Alternatively, use `gdb -batch -ex 'disassemble /s <symbol>' <prefix>/lib/libpinyin.so.15`.
   Inspect unresolved macros, templates, inline helpers, implementation-defined behaviour, undefined behaviour and compiler-selected evaluation order.
4. Inspect `readelf -sW`, `.rodata` and relocations for questions about binary contents rather than execution.

Stop if reading cannot settle the question (Requirement 2.4).

## Writing it down

- Cite `path:line` at the pin, or symbol and instruction offsets, in the report.
- Record deliberately reproduced pin defects in `docs/findings/upstream-report-drafts.md`.
- Justify departures under `docs/findings/compatibility-policy.md` with a register row.
  For language artefacts, use `docs/findings/language-runtime-discrepancy-template.md` and a register entry.
  Name the artefact, its idiomatic Rust replacement and affected inputs.
  Show that the C contract remains unchanged.
  Do not write Rust to imitate the artefact.
- Retain a `POLICY-ACCEPTED` contract-diff case for each recorded artefact, never `MATCH`.
  Keep its pin and subject evidence.

## Math

Read the pin's arithmetic to identify the product's calculation.
Implement that calculation in idiomatic Rust.
C promotions/conversions, floats stored in integers and compiler contraction are language artefacts.
Use Rust's standard maths at full speed, including `ln`, `exp`, `powf` and every other `std` float method.
Do not slow code to imitate glibc, libm, GLib `gfloat` or other C behaviour.
Record resulting last-bit differences as class (a).
Do not chase them.

## Complexity (axis L)

Measure complexity; reading alone cannot establish runtime cost.
Run both implementations on a named workload in the same container.
Measure whole-process instructions with callgrind and peak heap with massif.
Use three repetitions and state the spread.
Attribute regressions to source components on both sides, as in #757.
Fix regressions on both axes.
Written justification does not replace a fix.
Use the pin's method or a better method while preserving the differential's recorded output.
Keep identical results, or `POLICY-ACCEPTED` for a recorded artefact.
The differential remains the gate.
The 1.10× thresholds in #757 and #870 apply only to their own workloads.

## Internal improvement

Stage 2 permits compatible internal changes, including performance improvements without a ceiling.
Keep output, files and ABI unchanged.
Examples include `memmap2` replacing the mmap wrapper (#862) and `memchr` substring search (#858).

Compatible items from the 2026-09 direction include:

- Dictionary cursor walks for lattice building.
- Incremental per-keystroke lattices.
- Per-session arenas.
- Removing ordered atomics from the steady cycle.
- Host-independent cost derivation with identical output on every cell.
  An output change requires class (a) consideration or Stage 3.

Recorded measurement leads remain:

- Steady-cycle allocation churn, from `docs/findings/perf-cycle-ir-differential-2026-09-08.md`.
- Arm64 is about 10% behind the pin where amd64 reaches parity, in `docs/findings/perf-steady-cycle-cross-host-2026-09-07.md`.
  Open leads are memory ordering and scan-matrix fan-out: 4.11 versus 1.32 paths per window.
  See `docs/findings/scan-matrix-fanout-2026-09-10.md`.
- The RSS gap follows 3.07× dictionary probes that fault Kyoto Cabinet leaf pages.
  See `docs/findings/rss-attribution-2026-09-09.md`.

## Gating

Write the differential after deriving the behaviour.
Use the reference cell, or all three when the backend path check selects them.
Show once that reverting the fix makes the differential detect the defect.
If no fix exists, use a deliberate, named mutation of oxpinyin.
Add cases to an existing driver under the [verification rules](../../runbooks/verification.md).
Re-read source when a differential contradicts the derivation.
Do not tune the implementation towards the pin.
Record any remaining language-artefact difference.
