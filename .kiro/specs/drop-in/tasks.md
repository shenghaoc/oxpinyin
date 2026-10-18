# Implementation Plan — Drop-in: the libpinyin binary

## Overview

Status snapshot. The binary identity, native storage of the system data
directory (P6) and of the user dir (task 9, landed 2026-09-09; maintainer
ruling 2026-09-09) are merged and measured on three distro backends;
task 10, the Berkeley DB backend, landed 2026-09-12
(`docs/findings/berkeleydb-backend.md`).

## Tasks

- [x] 1. Set the SONAME and the cargo-c library metadata (#206).
  _Requirements: 1_

- [x] 2. Install the header under `libpinyin-<pin version>/` and ship
  `libpinyin.pc` with the installed naming (#206, #192; `2.11.91` then,
  `2.11.92` from #592 per the version ruling 2026-09-26 UTC).
  _Requirements: 1_

- [x] 3. Open the backend's system data directory (#228's first reader;
  replaced 2026-09-02 by P6's native readers —
  `docs/findings/runtime-direct-libpinyin-data-2026-09-02.md`).
  _Requirements: 2_

- [x] 4. Implement the `MemoryChunk` reader with checksum verification (#228).
  _Requirements: 2_

- [x] 5. Measure the drop-in (Kyoto Cabinet) on Fedora rawhide
  (kyotocabinet 1.2.80): 1,571/1,571 rows, sorted sets byte-identical,
  order-only — sets only; the order residual was R1's (task 8).
  _Requirements: 2_

- [x] 6. Measure the drop-in (tkrzw) on Debian testing: the same shape —
  1,571/1,571 rows, sets identical, order-only — sets only; the order
  residual was R1's (task 8).
  _Requirements: 2_

- [x] 7. Measure the drop-in (Kyoto Cabinet) on NixOS
  (nixpkgs-unstable): identical to Fedora; punct rows identical, order
  included.
  _Requirements: 2_

- [x] 8. Attribute the drop-in divergence to R1's defined-order rule
  (`docs/findings/upstream-divergences.md`, 2026-08-30). **Superseded
  2026-09-02 by P6:** the runtime walks the backend's native phrase DBM
  and the predicted order is the pin's — policy row 12, `pred-order-diff`
  IDENTICAL on Kyoto Cabinet and tkrzw (1,588 lines, 0 mismatches). That
  differential, not tasks 5–6, is the evidence for Requirement 3.3.
  _Requirements: 3_

- [x] 9. The user dir as the backend's native user files (Kyoto Cabinet,
  tkrzw; Berkeley DB with task 10) — the same files a libpinyin build on
  that backend keeps (maintainer ruling 2026-09-09): `user_bigram.db`,
  `user_pinyin_index.bin`, `user_phrase_index.bin`, `user.bin`, the
  `*.dbin` diff logs, `user.conf`, loaded and saved as the pin does. A
  user dir from another backend starts fresh, as with libpinyin. The
  system-token REMOVE log record is disclosed as a skip, not lossless
  replay (`docs/findings/user-store.md` §11).
  _Requirements: 2, 3_

- [x] 10. BerkeleyDB backend — the fifth store peer (`--features bdb`),
  un-shelved 2026-09-12 on the consumer ask; the system `bigram.db` is
  itself BDB, so every session on a BerkeleyDB distro opens it through
  this backend. Verified in Debian-testing and Fedora-44 containers
  against a pin-built `--with-dbm=BerkeleyDB` oracle: real-file walks,
  both round-trip directions, sanitizers
  (`docs/findings/berkeleydb-backend.md`, which also records the
  pre-existing `user_driver.c` regression it found in the differential
  harness — broken for every backend, not this one).
  _Requirements: 2_
