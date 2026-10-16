# Requirements Document — Oxidation record

## Introduction

`.kiro/steering/oxidation-method.md` (maintainer ruling 2026-10-16 UTC)
sets the order of work for turning libpinyin into Rust: record the pin's
data structures and algorithms, design and analyse oxpinyin's, then
implement and gate on the oracle. Most of oxpinyin was written before
that order existed. This spec brings the existing tree into the record,
closes the gaps the first survey found, and keeps the record current.

The guiding principle: reproduce the existing system's observable
behaviour before improving its internal design — compatibility in the
first release, elegance, extensibility and performance afterwards.
Requirements 1–4 and 7 serve the first release; Requirements 5.2 and 6
govern the work after it.

The record is `docs/findings/libpinyin-structures-and-algorithms.md`.
The narrative for human readers is `docs/oxidation-journey.md`.

## Glossary

- **Pin** — libpinyin `074a2219` (2.11.92), `tools/oracle/oracle-pin.txt`.
- **Row** — one entry of the record: a pin structure or algorithm, its
  oxpinyin counterpart, and its mark (same / same behaviour / different).
- **Parity path** — code whose result reaches any of the 79 `pinyin_*`
  exports or the files they write.
- **Coefficient** — a numeric constant that shapes output: a seed,
  factor, ceiling, threshold, width, epsilon, penalty or discount.

## Requirements

### Requirement 1: Every subsystem is recorded

**User Story:** As an agent or reviewer, I want the pin's structures and
algorithms written down beside oxpinyin's, so that a gap is visible in
the source before it shows up at the oracle.

#### Acceptance Criteria

1. THE record SHALL cover libpinyin's component layout and how
   oxpinyin's crates divide it.
2. THE record SHALL have a row for each structure and algorithm on the
   parity path in storage, parsing, the key matrix, sentence decoding,
   the candidate list, training and user phrases, the object model and
   persistence, and for the offline tools.
3. EACH row SHALL cite the pin by `file:line` at `074a2219` and oxpinyin
   by file and item.
4. WHEN a change alters a recorded structure or algorithm THEN the same
   change SHALL update its row.

### Requirement 2: Coefficients come from the pin

**User Story:** As the maintainer, I want every coefficient to be the
pin's, so that no output depends on a value fitted to the oracle.

#### Acceptance Criteria

1. THE record SHALL list every coefficient on the parity path with its
   pin value and citation.
2. A coefficient on the parity path SHALL equal the pin's value or be
   the pin's value converted to oxpinyin's scale.
3. WHEN a coefficient was chosen by sweeping or fitting THEN it SHALL be
   removed from every path that reaches output, and the pin's rule SHALL
   take its place.

### Requirement 3: Every non-identical row is argued

**User Story:** As a reviewer, I want each place where oxpinyin's
structure differs from the pin's to carry its argument, so that "same
behaviour" is a claim I can check.

#### Acceptance Criteria

1. EACH row marked "same behaviour" or "different" SHALL state why the
   observable behaviour matches the pin, or which compatibility-policy
   class covers the difference.
2. EACH such row SHALL compare oxpinyin's time and space complexity with
   the pin's for the operation, and SHALL NOT be worse in both.
3. A row whose argument fails SHALL be treated as a defect and fixed to
   the pin's behaviour.

### Requirement 4: Known defects are fixed bug for bug

**User Story:** As a consumer, I want the full-pinyin parser to choose
the segmentation the pin chooses under every option word.

#### Acceptance Criteria

1. WHEN two parse steps tie on parsed length and key count THEN the
   parser SHALL prefer the step with the lower total distance, as the
   pin does (`src/storage/pinyin_parser2.cpp:306-310`), with correction
   spellings at distance 1.
2. WITH `PINYIN_CORRECT_ON_ONG` set, `chonga`, `chongan` and `zhonge`
   SHALL parse as `chong'a`, `chong'an` and `zhong'e`.

### Requirement 5: Idiomatic Rust, the pin's shape only where needed

**User Story:** As the maintainer, I want oxpinyin written as idiomatic
Rust, with C-shaped code kept only where the behaviour depends on it.

#### Acceptance Criteria

1. THE record SHALL list every place where oxpinyin keeps the pin's code
   shape, and whether behaviour depends on it.
2. WHERE behaviour does not depend on the pin's code shape, new code
   SHALL be written in idiomatic Rust, and existing code SHALL be
   rewritten in it after the first release or when it is changed for
   compatibility.

### Requirement 6: Performance gaps try the pin's algorithm first

**User Story:** As the maintainer, I want a measured performance gap to
be closed by trying the pin's own algorithm, written idiomatically,
before anything else.

#### Acceptance Criteria

1. AFTER the first release, WHEN oxpinyin is measurably slower or larger than the pin for an
   operation THEN the pin's structure or algorithm, written as
   idiomatic Rust, SHALL be measured first, in one run beside the base
   and the pin.
2. THE faster of the measured versions SHALL be kept, subject to
   Requirement 3.2.

### Requirement 7: The library's operations are the Rust surface

**User Story:** As a frontend author, I want oxpinyin's Rust surface to
be libpinyin's operations, so that libpinyin's semantics are not forced
through an abstraction designed for a general IME.

#### Acceptance Criteria

1. THE Rust surface for the first release SHALL be the pin's operations
   — parse, guess, select, train, segment, save — on a context and an
   instance.
2. A key-driven layer (key input, preedit, commit) SHALL sit in an
   optional adapter built on those operations, not in the same type.
3. THE C ABI SHALL be built on the operations alone.
