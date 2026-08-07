# Requirements — Foundation

## Introduction

Foundation freezes the reproducible upstream reference, provenance,
configuration contract, behavioural fixtures and portable build guarantees
that every implementation task depends on.

## Glossary

- **Oracle** — the binaries built from the pinned upstream source and data
  archives and used for differential testing.
- **Pin** — release tags, commit SHAs and data-artefact checksums in the
  reference freeze.
- **Fixture** — a frozen input and expected-output record in families F-A–F-F.
- **Lane-P** — portable implementation and acceptance against frozen fixtures;
  it needs no oracle binary.
- **Lane-L** — Linux-only integration and differential verification against
  the source-built oracle.

## Requirements

### Requirement R0: Oracle reference freeze recorded

**User Story:** As the maintainer, I want a source-built oracle pin so that
parity measurements are reproducible across distributions.

WHEN P0-0 completes THEN the reference freeze for reproducibility (upstream
release state as of 2026-07-31) SHALL record the latest upstream release tags
on or before that date, their commit SHAs, and every data artefact URL and
SHA-256. Every authoritative run SHALL verify the recorded checksums and use
only binaries produced by the canonical recipe.

### Requirement R1: Model/data provenance resolved for shipping

**User Story:** As a distributor, I want every model and table to have recorded
provenance so that releases contain only redistributable artefacts.

1. WHEN licence inspection completes THEN a written finding SHALL state
   redistribution status of tables and model. 2. Stage 1 reads pinned-archive
   data, so this gates standalone shipping and Stage 2 only. 3. WHEN
   inconclusive after two weeks THEN proceed as if not redistributable.

### Requirement R2: Upstream GSettings schema captured verbatim

**User Story:** As a migrating user, I want existing settings represented
exactly so that migration is mechanical and verifiable.

Every key, type and default recorded; treated as a fixed input to our
superset schema.

### Requirement R3: Pinyin parsing

**User Story:** As an input-method user, I want parsing to be deterministic and
total so that malformed or partial input never crashes the engine.

1. Valid full-pinyin parses to correct syllables. 2. Apostrophe is a hard
boundary. 3. Incomplete input returns partial + remainder, no error.
4. Unparseable characters yield partial parse, never failure. 5. The
parser SHALL NOT panic on any byte sequence. 6. All valid syllables parse
(table-driven). 7. Ambiguous inputs return all segmentations; choosing is
the decoder's job.

### Requirement R4: Buildable and checked

**User Story:** As a contributor, I want one reproducible toolchain and
portable checks so that platform drift is caught before review.

`cargo fmt --check`, `clippy -D warnings`, and the portable-crate test
set pass on Linux, macOS and Windows; the Linux lane tests the full workspace;
the five portable crates carry no platform deps;
pinyin-core carries `#![forbid(unsafe_code)]`.

### Requirement R5: Behavioural fixtures and cross-lane evidence

**User Story:** As an implementer, I want frozen fixtures and a registered
evidence base so that Lane-P acceptance is mechanical and needs no oracle.

1. The capture harness SHALL emit fixture families F-A–F-F as JSON records
   per `docs/findings/spec-derivation.md`. 2. The F-E register SHALL hold
   all 13 cases with reproducible evidence attached in each case's
   applicable lane. 3. WHEN a dependent `[B]` task is declared ready THEN
   its cited fixtures SHALL already be human-frozen.

### Requirement R6: Core trait seam defined

**User Story:** As an architect, I want the core seam frozen as signatures so
that implementation tasks compose without interface drift.

1. The four core traits SHALL be defined signature-only with doc comments.
   2. They SHALL compile in pinyin-core under `#![forbid(unsafe_code)]`
   with no dependencies. 3. WHEN a signature change seems needed after the
   freeze THEN an Architect correction PR SHALL merge before implementation
   resumes.
