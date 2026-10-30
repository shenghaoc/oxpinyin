# Requirements — Foundation

## Introduction

Foundation freezes the reproducible reference, provenance, configuration contract, behavioural fixtures and portable build guarantees needed by implementation tasks.

## Glossary

- **Oracle:** binaries built from pinned upstream source/data archives for differential testing.
- **Pin:** release tags, commit SHAs and data checksums in the reference freeze.
- **Fixture:** frozen input/output records in families F-A–F-F.
- **Lane-P:** portable implementation and acceptance against frozen fixtures, without an oracle binary.
- **Lane-L:** Linux integration and differential verification against the source-built oracle.

## Requirements

### Requirement R0: Oracle reference freeze recorded

The source-built pin makes measurements reproducible across distributions.
At P0-0 completion, record upstream release state as of 2026-07-31.
Include the latest release tags on or before that date, commit SHAs, and every data artifact URL and SHA-256.
Verify recorded checksums for every authoritative run.
Use only binaries produced by the canonical recipe.

### Requirement R1: Model/data provenance resolved for shipping

Distributors need recorded provenance so releases contain only redistributable artifacts.

1. Record model and table redistribution status after license inspection.
2. Apply this gate only to standalone shipping and Stage 3's own-data departure.
   Stage 1 reads pinned-archive data.
3. Treat an inconclusive result after two weeks as non-redistributable.

### Requirement R2: Upstream GSettings schema captured verbatim

Exact settings make migration mechanical and verifiable.
Record every key, type and default.
Treat them as fixed inputs to the superset schema.

### Requirement R3: Pinyin parsing

Parsing must be deterministic and total for malformed and partial input.

1. Parse valid full pinyin into correct syllables.
2. Treat apostrophes as hard boundaries.
3. Return a partial parse and remainder for incomplete input, without an error.
4. Return a partial parse for unparseable characters, never failure.
5. Do not panic on any byte sequence.
6. Cover all valid syllables with table-driven tests.
7. Return all segmentations for ambiguous input.
   The decoder selects among them.

### Requirement R4: Buildable and checked

A reproducible toolchain and portable checks detect platform drift before review.
Pass `cargo fmt --check`, `clippy -D warnings` and portable-crate tests on Linux, macOS and Windows.
The Linux lane tests the full workspace.
The five portable crates have no platform dependencies.
Core carries `#![forbid(unsafe_code)]`.

### Requirement R5: Behavioural fixtures and cross-lane evidence

Frozen fixtures make Lane-P acceptance mechanical without an oracle.

1. Emit F-A–F-F JSON records from the capture harness under `docs/findings/spec-derivation.md`.
2. Keep all 13 cases in the F-E register, with reproducible evidence in each applicable lane.

### Requirement R6: Core trait seam defined

The original goal was composable implementation without interface drift.

1. Define the four core traits as signatures with doc comments only.
2. Compile them in core with `#![forbid(unsafe_code)]` and no dependencies.
3. The original freeze required an Architect correction PR before implementation resumed after a signature change.
   This requirement was superseded 2026-10-18 UTC.
   Rust signatures are internal; update in-tree callers in the same change under the [interface ruling](../../constitution.md#external-interfaces).

### Requirement R7: Drop-in replacement surface

Unmodified consumers must link and run under libpinyin's names.

1. Give the cdylib SONAME `libpinyin.so.15` (libtool -version-info 15:0).
2. Install headers under `include/libpinyin-<version>/`, using the pin's `libpinyin_tag` in `tools/oracle/oracle-pin.txt`.
   This is `2.11.92` at `074a2219`.
   The version ruling 2026-09-26 UTC, #592, supersedes PR #363's hold at 2.11.91.
3. Ship `libpinyin.pc` with `pkgdatadir`, `database_format` and `exec_prefix`.
4. Export all 79 live `pinyin_*` symbols from `docs/findings/abi-reference.md`.

### Requirement R8: Native storage on libpinyin's backends

Use the replaced libpinyin build's backend and files without shipped conversion.

1. Open the supplied backend directory natively through `pinyin_init` (P6).
   Do not detect layouts or convert data.
2. Parse the 8-byte MemoryChunk header: u32 LE length and u32 XOR checksum.
   Verify the checksum before use.
3. Include Kyoto Cabinet on Fedora/NixOS and tkrzw on Debian in measurements.
4. Read 1,571/1,571 prediction rows with byte-identical sorted sets on every measured backend.
   The historical order-only residual was R1, closed by P6 (policy row 12).

### Requirement R9: Output compatibility rule and its exceptions

Replacing the library must preserve observable output.

1. Reproduce byte-identical output for every export on identical inputs and state at libpinyin 2.11.92, `074a2219`.
   The pin is in `tools/oracle/oracle-pin.txt`.
   Observations include returns, out-parameters, pointed-to data, written lengths and handle-state transitions.
2. Apply the compatibility policy's three classes.
   Class (a) MATH covers platform-dependent floating-point accumulation.
   Class (b) MEMORY SAFETY covers upstream undefined behaviour that Rust structurally prevents.
   Class (c) AVAILABILITY covers caller-input aborts with `false`/`Err` refusals.
3. Preserve predicted-candidate list order.
   R1's text-ascending order, from the maintainer decision 2026-08-25, applied only to the removed redb/LMDB containers.
   P6 closed R1 (policy row 12).
4. Treat a wrong return value as worse than an absent export.
   A stub returning `false` is a defect, not compliance.
