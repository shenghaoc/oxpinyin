# Requirements Document — Drop-in: the libpinyin binary

## Introduction

The default build is a bug-for-bug `libpinyin.so.15` and `libzhuyin.so.15` on one of libpinyin's three backends.
It handles that backend's files natively.
Distributors install it in libpinyin's place, with consumers and system data unchanged.
Identity and storage are merged and measured (PRs #206, #192; P6; tasks 9 and 10).
This is [pinned behaviour](../../pinned-behaviour.md).
The [improvements policy](../../oxpinyin-improvements.md) defines permitted differences.

## Requirements

### Requirement 1: libpinyin's binary identity

Unmodified consumers must link and run.

#### Acceptance Criteria

1. Give the cdylib SONAME `libpinyin.so.15` (libtool -version-info 15:0).
2. Install the header under `include/libpinyin-<version>/`.
   Set pkg-config `Version` to the pin's version.
   Use `libpinyin_tag` in `tools/oracle/oracle-pin.txt`: `2.11.92` at `074a2219`.
   Preserve the version ruling 2026-09-26 UTC, #592.
3. Ship `libpinyin.pc` with `pkgdatadir`, `database_format` and `exec_prefix`.
4. Export all 79 live upstream `pinyin_*` symbols listed in `docs/findings/abi-reference.md`.

### Requirement 2: Storage — libpinyin's three backends, natively

Use the same backend and files as the replaced libpinyin build.

#### Acceptance Criteria

1. Build with exactly one of Berkeley DB, Kyoto Cabinet or tkrzw, selected at compile time.
   Use no other backend.
   Default to `bdb`, matching bare `./configure`.
2. Open the system directory supplied to `pinyin_init` as the pin does.
   Use its backend DBMs, `MemoryChunk` files and `table.conf`.
   Do not convert data or detect layouts.
3. Parse the 8-byte `MemoryChunk` header: u32 LE length and u32 XOR checksum.
   Verify the checksum before use.
4. Keep native user files as the pin does.
   These are `user.conf`, `user_bigram`, `user_pinyin_index`, `user_phrase_index`, `user.bin` and `*.dbin` logs.
5. Treat a different-backend user directory as non-conforming under `check_format`.
   Clean it and start fresh, as the pin does.

### Requirement 3: Output identical to the pin

The replacement must preserve observable output.

#### Acceptance Criteria

1. For each exported symbol, reproduce byte-identical output for identical inputs and state at libpinyin 2.11.92, `074a2219`.
   Recorded language artefacts remain exceptions.
   State includes the same-backend user directory.
2. Permit only class (a) MATH, class (b) MEMORY SAFETY, class (c) AVAILABILITY and recorded language artefacts.
   The language-artefact amendment is 2026-10-26 UTC, #1303.
   Follow `docs/findings/compatibility-policy.md`.
3. Match predicted-candidate rows and list order, except for policy-admitted differences.
   This covers `pinyin_guess_predicted_candidates[_with_punctuations]` and `pinyin_choose_predicted_candidate`.
   Register R1 closed with P6; see policy row 12.
4. Treat a stub returning `false` as a defect, not compliance.
