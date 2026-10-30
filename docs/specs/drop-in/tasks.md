# Implementation Plan — Drop-in: the libpinyin binary

## Overview

This is a status snapshot.
Binary identity, native system data (P6) and native user data are merged and measured on three distribution backends.
Task 9 landed 2026-09-09 under the maintainer ruling 2026-09-09.
Task 10 added Berkeley DB on 2026-09-12; see `docs/findings/berkeleydb-backend.md`.

## Tasks

- [x] 1. Set SONAME and cargo-c library metadata (#206).
  _Requirements: 1_

- [x] 2. Install the header under `libpinyin-<pin version>/` and ship `libpinyin.pc` with installed naming (#206, #192).
  The version was `2.11.91`, then `2.11.92` from #592 under the version ruling 2026-09-26 UTC.
  _Requirements: 1_

- [x] 3. Open the backend's system directory.
  P6 native readers replaced #228's first reader on 2026-09-02.
  See `docs/findings/runtime-direct-libpinyin-data-2026-09-02.md`.
  _Requirements: 2_

- [x] 4. Implement MemoryChunk reading with checksum verification (#228).
  _Requirements: 2_

- [x] 5. Measure Kyoto Cabinet drop-in on Fedora rawhide, kyotocabinet 1.2.80.
  Result: 1,571/1,571 rows, sorted sets byte-identical.
  This was sets-only evidence with R1's order residual (task 8).
  _Requirements: 2_

- [x] 6. Measure tkrzw drop-in on Debian testing.
  Result: 1,571/1,571 rows, sorted sets identical.
  This was also sets-only evidence with R1's order residual (task 8).
  _Requirements: 2_

- [x] 7. Measure Kyoto Cabinet drop-in on NixOS, nixpkgs-unstable.
  Results matched Fedora; punctuation rows were identical, including order.
  _Requirements: 2_

- [x] 8. Attribute the divergence to R1's defined-order rule (`docs/findings/upstream-divergences.md`, 2026-08-30).
  **Superseded 2026-09-02 by P6:** native phrase-DBM traversal produces the pin's predicted order.
  Policy row 12's `pred-order-diff` was IDENTICAL on Kyoto Cabinet and tkrzw: 1,588 lines, 0 mismatches.
  That differential supplies Requirement 3.3's evidence; tasks 5–6 do not.
  _Requirements: 3_

- [x] 9. Use native user files for Kyoto Cabinet and tkrzw, with Berkeley DB added by task 10.
  The maintainer ruling 2026-09-09 requires the same files as a same-backend libpinyin build.
  These are `user_bigram.db`, `user_pinyin_index.bin`, `user_phrase_index.bin`, `user.bin`, `*.dbin` logs and `user.conf`.
  Load and save them as the pin does.
  A different backend starts fresh.
  System-token REMOVE replay is disclosed as a skip, not lossless replay (`docs/findings/user-store.md` §11).
  _Requirements: 2, 3_

- [x] 10. Add BerkeleyDB as the fifth store peer with `--features bdb`.
  The consumer request reopened it on 2026-09-12.
  System `bigram.db` is BDB, so every session on a BerkeleyDB distribution opens it through this backend.
  Verification used Debian-testing and Fedora-44 containers against a pin-built `--with-dbm=BerkeleyDB` oracle.
  Checks covered real-file walks, both round-trip directions and sanitizers.
  `docs/findings/berkeleydb-backend.md` also records the discovered `user_driver.c` regression.
  That pre-existing harness defect affected every backend, not only BerkeleyDB.
  _Requirements: 2_
