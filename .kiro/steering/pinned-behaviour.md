---
inclusion: always
---
# Pinned behaviour — what oxpinyin reproduces

The half of the product that is libpinyin's: what the pinned libpinyin
does, defects included. Stage 2 makes oxpinyin match it bug for bug.
Nothing here is an oxpinyin decision; where oxpinyin differs, the
difference and the rule that admits it are in
`oxpinyin-improvements.md`. Canonical policy text:
`docs/findings/compatibility-policy.md`.

## The pin

libpinyin 2.11.92 at `074a2219` — libpinyin's default-branch tip on
2026-09-12; `tools/oracle/oracle-pin.txt` is authoritative. The
reference build is the pin's autoconf build with a bare `./configure`
(Berkeley DB, `configure.ac:94`), built by `tools/oracle/build-oracle.sh`
with autoconf's default `-g -O2` and no `-DNDEBUG`: every `assert` at the
pin is live.

## Identity

- SONAME `libpinyin.so.15` (libtool `-version-info 15:0`);
  `libzhuyin.so.15` for the `--enable-libzhuyin` facade.
- Header under `include/libpinyin-<pin version>/`; `libpinyin.pc` with
  `pkgdatadir`, `database_format`, `exec_prefix`. The version follows
  the pin (`libpinyin_tag`; ruling 2026-09-26 UTC, #592).
- The full ABI: all 79 `pinyin_*` exports (52 for zhuyin). Nothing is
  scoped by which consumer calls it.

## Behaviour — the E2E I/O rule

For every exported symbol, given the same inputs and state, oxpinyin
produces what the pin produces: return status, out-parameters and the
data they point to, written lengths, handle state transitions, files
written, bytes written to stderr, and process-wide side effects. State
includes the on-disk user state of a same-backend user dir.

Not compared: the `errno` a successful call leaves behind, unless the
API gives it a meaning (ruling 2026-10-08 UTC).

## Storage

libpinyin is built on one of three backends — Berkeley DB, Kyoto
Cabinet or tkrzw — chosen by `--with-dbm` (Berkeley DB is the
bare-`./configure` default). oxpinyin is built on one of the same three,
chosen by cargo feature (`bdb` by default), and no other. Each handles
its backend's files natively, and identically:

- **System data:** the directory `pinyin_init` opens — the backend's
  DBMs, the `MemoryChunk` library files, and `table.conf`, which decides
  which libraries exist and what they are called (a valid non-stock
  `table.conf` included).
- **User data:** `user.conf`, `user_bigram`, the user indexes,
  `user.bin` and the `*.dbin` logs. Builds on the same backend use the
  same user dir. A build on a different backend cannot read it and
  starts fresh: libpinyin's tkrzw build cannot read its Berkeley DB user
  files, and neither can oxpinyin's (ruling 2026-09-09).

## Pin defects are part of the target

An observable defect at the pin is reproduced and recorded as an
upstream defect (`docs/findings/upstream-report-drafts.md`), not fixed.
Examples already ruled:

- `pinyin_init`/`zhuyin_init` leave the process `LC_NUMERIC` at `"C"`
  (#539).
- The pin's raw stderr output, byte for byte, including the
  `mmap … failed!` / `open … failed!` writers (#545).
- The pin's behaviour on a defective dependency build is matched too
  (the tkrzw LTO sentinel, #677).
- Predicted-candidate ranking and training follow the pin's own rules,
  derived from its source — not tuned until examples agree.

The only departures are the classes in `oxpinyin-improvements.md`.

## How the pin's behaviour is known

By reading it: the pin's source first, then the disassembly of the
reference-cell object (`/AGENTS.md`, "Stage 2 — bug-for-bug"). Never by
running experiments against the oracle. Every reproduced behaviour cites
`path:line` at the pin, or symbol and instruction offsets.
