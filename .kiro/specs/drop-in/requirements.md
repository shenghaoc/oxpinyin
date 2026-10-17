# Requirements Document — Drop-in: the libpinyin binary

## Introduction

An oxpinyin build is a bug-for-bug libpinyin binary: `libpinyin.so.15`
(and `libzhuyin.so.15`), built on one of the three backends libpinyin is
built on, handling that backend's files natively. A distributor installs
it in libpinyin's place and consumers run unchanged on the data already
on the system. The identity and the storage are merged and measured
(PRs #206, #192; P6; tasks 9 and 10). This spec is pinned behaviour
(`.kiro/steering/pinned-behaviour.md`); the differences oxpinyin is
allowed are in `.kiro/steering/oxpinyin-improvements.md`.

## Requirements

### Requirement 1: libpinyin's binary identity

**User Story:** As a distributor, I want an oxpinyin build to be
libpinyin's binary so that unmodified consumers link and run.

#### Acceptance Criteria

1. THE cdylib SHALL carry SONAME `libpinyin.so.15` (libtool
   -version-info 15:0).
2. THE header SHALL install under `include/libpinyin-<version>/`, and the
   pkg-config `Version` SHALL be `<version>`, where `<version>` is the
   pinned libpinyin's (`libpinyin_tag` in `tools/oracle/oracle-pin.txt`;
   `2.11.92` at `074a2219`; version ruling 2026-09-26 UTC, #592).
3. THE pkg-config file SHALL ship as `libpinyin.pc` exposing `pkgdatadir`,
   `database_format` and `exec_prefix`.
4. THE exported surface SHALL be the full live upstream ABI — 79
   `pinyin_*` symbols (`docs/findings/abi-reference.md`).

### Requirement 2: Storage — libpinyin's three backends, natively

**User Story:** As a distributor, I want an oxpinyin build to use the
same backend and the same files as the libpinyin build it replaces, so
that nothing on disk changes.

#### Acceptance Criteria

1. oxpinyin SHALL be built on exactly one of Berkeley DB, Kyoto Cabinet
   and tkrzw, chosen at compile time (`bdb` by default, as a bare
   `./configure` chooses Berkeley DB), and on no other backend.
2. WHEN `pinyin_init` is given its backend's system data directory THEN
   the runtime SHALL open it the way the pin does — the backend's DBMs,
   the `MemoryChunk` library files and `table.conf` — with no conversion
   and no layout detection.
3. THE `MemoryChunk` reader SHALL parse the 8-byte header (u32 LE length,
   u32 XOR checksum) and verify the checksum before use.
4. THE user dir SHALL be the backend's native user files (`user.conf`,
   `user_bigram`, `user_pinyin_index`, `user_phrase_index`, `user.bin`,
   the `*.dbin` logs), kept as the pin keeps them.
5. WHEN a user dir was written on another backend THEN it SHALL be
   treated as the pin treats it: non-conforming, cleaned, fresh start
   (`check_format`).

### Requirement 3: Output identical to the pin

**User Story:** As a consumer, I want byte-identical output so that the
replacement changes nothing observable.

#### Acceptance Criteria

1. FOR every exported symbol, given the same inputs and state, the whole
   observable output SHALL be byte-identical to the pinned libpinyin
   2.11.92 at `074a2219`. State includes the user dir on the same
   backend.
2. Divergence SHALL be permitted only under classes (a) MATH, (b) MEMORY
   SAFETY, (c) AVAILABILITY — `docs/findings/compatibility-policy.md`.
3. THE predicted-candidate rows
   (`pinyin_guess_predicted_candidates[_with_punctuations]` /
   `pinyin_choose_predicted_candidate`) SHALL match the pin, list order
   included (register R1, closed by P6; policy row 12).
4. A stub returning `false` SHALL be treated as a defect, not compliance.
