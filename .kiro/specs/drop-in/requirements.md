# Requirements Document — Drop-in replacement

## Introduction

oxpinyin replaces libpinyin at the ABI level: a distributor renames the
built object to `libpinyin.so.15`, puts it on the library path, and
unmodified consumers run against the data already on the system. The
surface is merged and measured (PRs #206, #192, #228); this spec records
the goal, the mechanisms and the remaining work.

## Glossary

- **Consumer_union** — the 58 exported symbols the two reference consumers
  (ibus-libpinyin 1.16.5, fcitx-libpinyin) call.
- **Compat_path** — reading installed libpinyin data directly, without
  conversion.
- **R1** — the predicted-candidate row-order divergence; oxpinyin's
  defined order is text-ascending (maintainer decision, 2026-08-25).

## Requirements

### Requirement 1: Binary identity with the pinned ABI

**User Story:** As a distributor, I want oxpinyin under libpinyin's names
so that unmodified consumers link and run.

#### Acceptance Criteria

1. THE cdylib SHALL carry SONAME `libpinyin.so.15` (libtool
   -version-info 15:0).
2. THE header SHALL install under `include/libpinyin-2.11.91/`.
3. THE pkg-config file SHALL ship as `libpinyin.pc` exposing `pkgdatadir`,
   `database_format` and `exec_prefix`.
4. THE exported surface SHALL be the 58-symbol consumer union, no more and
   no less.

### Requirement 2: Compat read path for installed data

**User Story:** As a packager, I want oxpinyin to consume the installed
libpinyin data so that no conversion step ships.

#### Acceptance Criteria

1. WHEN `pinyin_init` is pointed at a libpinyin data directory THEN the
   runtime SHALL detect the layout and open the compat path.
2. THE reader SHALL parse libpinyin's `MemoryChunk` container (8-byte
   header: u32 LE length, u32 XOR checksum) and verify before use.
3. THE path SHALL cover Kyoto Cabinet installs (Fedora, NixOS) and tkrzw
   installs (Debian).
4. ON every measured backend THE prediction surface SHALL read 1,571/1,571
   rows with sorted row sets byte-identical.

### Requirement 3: Output compatibility under the recorded exceptions

**User Story:** As a consumer, I want byte-identical output so that the
replacement changes nothing observable.

#### Acceptance Criteria

1. FOR every consumer-union symbol, given the same inputs and state, the
   whole observable output SHALL be byte-identical to the pinned libpinyin
   2.11.91.
2. Divergence SHALL be permitted only under classes (a) MATH, (b) MEMORY
   SAFETY, (c) AVAILABILITY, (d) CONSUMER SCOPE — see
   `docs/findings/compatibility-policy.md`.
3. THE predicted-candidate list order (the exempt surface: the rows of
   `pinyin_guess_predicted_candidates[_with_punctuations]` /
   `pinyin_choose_predicted_candidate`) SHALL follow the defined
   text-ascending order (register R1): the rows' content and sorted sets
   SHALL be byte-identical, and only the list positions are exempt from
   the pin-order comparison.
4. A stub returning `false` SHALL be treated as a defect, not compliance.

### Requirement 4: Divergences attributed, not hidden

**User Story:** As a maintainer, I want every recorded divergence owned by
a rule or a plan so that nothing is silently re-frozen.

#### Acceptance Criteria

1. THE predicted-row order (R1) SHALL follow the defined text-ascending
   order, per Requirement 3's bounded exception; the pin's order SHALL be
   recorded as a constant, never a target.
2. THE BerkeleyDB compat path SHALL remain SHELVED until a consumer needs
   it.
