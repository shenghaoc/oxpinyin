# Implementation Plan — Drop-in replacement

## Overview

Status snapshot. The binary identity and the compat read path are merged
and measured on three distro backends; the remaining open item is task
9 — same-backend user files read and written seamlessly (maintainer
ruling 2026-09-09, `docs/findings/compatibility-policy.md` goal
amendment) — and the BerkeleyDB route remains shelved.

## Tasks

- [x] 1. Set the SONAME and the cargo-c library metadata (#206).
  _Requirements: 1_

- [x] 2. Install the header under `libpinyin-2.11.91/` and ship
  `libpinyin.pc` with the installed naming (#206, #192).
  _Requirements: 1_

- [x] 3. Implement `CompatLayout` detection and the compat load path (#228).
  _Requirements: 2_

- [x] 4. Implement the `MemoryChunk` reader with checksum verification (#228).
  _Requirements: 2_

- [x] 5. Measure the Kyoto Cabinet compat path on Fedora rawhide
  (kyotocabinet 1.2.80): 1,571/1,571 rows, sorted sets byte-identical,
  order-only.
  _Requirements: 2, 4_

- [x] 6. Measure the tkrzw compat path on Debian testing: the same shape —
  1,571/1,571 rows, sets identical, order-only.
  _Requirements: 2, 4_

- [x] 7. Measure the Kyoto Cabinet compat path on NixOS
  (nixpkgs-unstable): identical to Fedora; punct rows identical, order
  included.
  _Requirements: 2_

- [x] 8. Attribute the drop-in divergence to R1's defined-order rule
  (`docs/findings/upstream-divergences.md`, 2026-08-30).
  _Requirements: 4_

- [x] 9. User files read and written in libpinyin's own formats, drop-in
  set only (Kyoto Cabinet, tkrzw) — seamless in both directions with a
  same-backend libpinyin (maintainer ruling 2026-09-09): read the user
  state it left (`user_bigram.db`, `user_pinyin_index.bin`,
  `user_phrase_index.bin`, `user.bin`, the `*.dbin` diff logs,
  `user.conf`), save back what it picks up; answers the 2026-09-08
  design review's finding that a swap starts blank. Fresh-start
  applies only when the KV database backend actually changes (BDB
  distros, redb/LMDB builds). Done for the same-backend read/write and
  the reverse-direction oracle; the system-token REMOVE log record is
  disclosed as a skip, not lossless replay
  (`docs/findings/user-store.md` §11).
  _Requirements: 2, 3, 4_

- [ ] 10. BerkeleyDB compat path — SHELVED; revive only if a consumer
  requires it (incomplete implementation on `feat/bdb-backend`).
  _Requirements: 2_
