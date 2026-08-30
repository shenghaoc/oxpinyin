# Implementation Plan — Drop-in replacement

## Overview

Status snapshot. The binary identity and the compat read path are merged
and measured on three distro backends; the write path for user data and
the shelved BerkeleyDB route remain.

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

- [ ] 9. MemoryChunk write path for user data — learned bigrams written
  back in libpinyin's format.
  _Requirements: 2_

- [ ] 10. BerkeleyDB compat path — SHELVED; revive only if a consumer
  requires it (incomplete implementation on `feat/bdb-backend`).
  _Requirements: 2_
