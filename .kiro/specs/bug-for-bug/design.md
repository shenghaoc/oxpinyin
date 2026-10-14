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
3. **The disassembly of the reference-cell object** — the Berkeley DB
   oracle `tools/oracle/build-oracle.sh` installs. The pin's build uses
   autoconf's default `-g -O2` with no `-DNDEBUG`, so line tables are
   present and every `assert` is compiled in:
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
  `docs/findings/compatibility-policy.md`, with a register row.

## Gating

The differential is written after the derivation, to hold it: reference
cell, revert-and-check once, cases added to an existing driver
(`/AGENTS.md` "Verification cells"). A differential that disagrees with
the derivation sends the agent back to the reading ladder — the source
is re-read, not the implementation tuned.
