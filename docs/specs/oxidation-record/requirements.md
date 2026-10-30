# Requirements Document — Oxidation record

Amended 2026-10-18 UTC: Requirement 6 uses Constitution §2's binary-size
recording rule separately from runtime time/space comparisons.

## Introduction

The [source policy](../../source-policy.md) states the method ruled on 2026-10-16 UTC.
Record the pin's structures and algorithms, design and analyse oxpinyin's, then implement and gate on the oracle.
Most code predates that method.
This specification records existing code, closes survey gaps and maintains the record.

Stage 2 requires bug-for-bug compatibility under established exceptions.
It also permits compatible refactoring, idiomatic rewrites and performance or memory improvements.
Compatibility does not defer internal improvement.
An improvement does not excuse, close or reclassify an unrelated compatibility gap.
Intentional departures without an approved exception belong to Stage 3 behind an opt-in feature, off by default.
Requirements 1–4 serve pinned behaviour; Requirements 5–7 serve internal improvements.

The record is `docs/findings/libpinyin-structures-and-algorithms.md`.
The narrative is `docs/oxidation-journey.md`.

## Glossary

- **Pin:** libpinyin `074a2219` (2.11.92), from `tools/oracle/oracle-pin.txt`.
- **Row:** a pin structure/algorithm and its oxpinyin counterpart.
  Structure is same or different.
  Behaviour is follows, argued, unverified, a policy class, defect or off path.
- **Parity path:** code reaching either C ABI, its written files, or offline output promised identical to the pin.
  The ABIs have 79 pinyin and 52 Zhuyin exports.
  Offline output includes datagen system files.
- **Coefficient:** an output-shaping numeric constant: seed, factor, ceiling, threshold, width, epsilon, penalty or discount.

## Requirements

### Requirement 1: Every subsystem is recorded

The side-by-side record must expose gaps before oracle verification.

#### Acceptance Criteria

1. Record the pin's component layout and oxpinyin's crate split.
2. Cover every parity-path structure and algorithm in storage, parsing, key matrices, sentence decoding and candidate lists.
   Include training, user phrases, object models, persistence and offline tools.
3. Cite the pin by `file:line` at `074a2219` and oxpinyin by file and item.
4. Update a row in the same change that alters its structure or algorithm.

### Requirement 2: Coefficients come from the pin

No output may depend on a value fitted to the oracle.

#### Acceptance Criteria

1. Record every parity-path coefficient with its pin value and citation.
2. Use the pin value or its conversion to oxpinyin's scale.
3. Remove swept or fitted coefficients from every output-reaching path.
   Replace them with the pin's rule.

### Requirement 3: Every non-identical row is argued

Reviewers must be able to check claims of identical behaviour.

#### Acceptance Criteria

1. Explain how each structurally different row preserves observable behaviour, or name the compatibility-policy class permitting its difference.
2. Compare time and space complexity for the operation.
   Do not worsen both.
3. Fix a failed argument as a defect against the pin's product behaviour.
   Record language artefacts instead of fixing them (amended 2026-10-26 UTC, #1303).

### Requirement 4: Known defects are fixed bug for bug

The full-pinyin parser must choose the pin's segmentation under every option word.

#### Acceptance Criteria

1. Break equal parsed-length/key-count ties with lower total distance, matching `src/storage/pinyin_parser2.cpp:306-310`.
   Correction spellings have distance 1.
2. With `PINYIN_CORRECT_ON_ONG`, parse `chonga`, `chongan` and `zhonge` as `chong'a`, `chong'an` and `zhong'e`.
3. Score sentences with the pin's float types, conversion points and operation order.
   Verify compiler transformations and resolved `log` for the identified artifacts.
   Investigate and attribute remaining differences before classifying them.

### Requirement 5: Idiomatic Rust, the pin's shape only where needed

Keep C-shaped code only where behaviour depends on it.

#### Acceptance Criteria

1. Record every retained pin code shape and whether behaviour depends on it.
2. Write new code idiomatically when behaviour does not depend on the shape.
   Rewrite existing such code as internal work without observable changes.

### Requirement 6: Performance gaps try the pin's algorithm first

Try the pin's algorithm idiomatically before other responses to a measured performance gap.

#### Acceptance Criteria

1. First measure the pin's structure or algorithm in idiomatic Rust when an operation is measurably slower or larger.
   Compare it with base and pin in one run.
2. Measure each candidate's time and peak resident memory, with repetitions in that run.
   Record stripped shipping-library sizes under constitution §2.
   Report increases over base in the PR.
   Insignificant decreases need no mention.
3. Keep a version that is not worse than the pin in both runtime time and space (Requirement 3.2).
   Binary size per PR is outside that complexity trade.
   Justify the choice on runtime time and memory, not speed alone.
   Prefer idiomatic Rust on a tie.

### Requirement 7: The library's operations are the Rust surface

> *Note 2026-10-18 UTC: the Rust surface is internal and not a supported
> external interface. See the [interface ruling](../../constitution.md#external-interfaces).
> This is an in-tree architecture goal, not an external promise.*

The Rust surface should express libpinyin operations without forcing them through a general IME abstraction.

#### Acceptance Criteria

1. Expose parse, guess, select, train, segment and save operations on a context and instance in the bug-for-bug build.
2. Put key input, preedit and commit in an optional adapter over those operations, not in the same type.
3. Build both C ABIs on the operations alone.
