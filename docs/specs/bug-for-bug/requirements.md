# Requirements Document — Stage 2: bug-for-bug compatibility and internal improvement

## Introduction

Stage 2 follows the maintainer ruling 2026-10-14 UTC, amended 2026-10-26 UTC by #1303.
It reproduces the pin's product behaviour bug for bug across the exported ABI and user-store surfaces in idiomatic Rust.
The programming language is an implementation detail.
Do not reproduce behaviour caused only by another language's semantics or libraries (Requirement 6).
Stage 2 also permits compatible internal improvement.

Requirements 1–3 describe [pinned behaviour](../../pinned-behaviour.md).
Requirements 4–6 describe [oxpinyin improvements](../../oxpinyin-improvements.md): the performance floor, measurement method, language artefacts and maths.
Follow the [compatibility decision order](../../findings/compatibility-policy.md).
Intentional departures belong to [Stage 3](../stage3-departures/requirements.md).

## Glossary

- **Pin:** libpinyin at the commit in `tools/oracle/oracle-pin.txt` (2.11.92, `074a2219`).
- **Reference cell:** tkrzw on both sides, `PINYIN_ORACLE_DBM=tkrzw`.
  The maintainer decision of 2026-10-30 UTC superseded the 2026-10-04 choice of Berkeley DB.
- **Pin defect:** observable product behaviour upstream would call a bug, reproduced as written by idiomatic Rust.
- **Product behaviour:** observations that a language-neutral requirement would state.
  These include candidate selection/order/options, saved and loaded state, C returns/ownership/freeing, and files.
- **Language artefact:** behaviour caused only by C/C++ semantics, their libraries, compilers or processors.
  Examples include integer comparators, unsigned wrapping, float-to-integer truncation and storage-selected integer widths.
  GLib, glibc, other libc implementations and libstdc++ are examples of these libraries.
  Width or wrap required by the C ABI, file formats or domain algorithms is not an artefact.
- **Derivation:** source lines or symbol/instruction offsets that establish reproduced behaviour.
  Use the reference object by default, the backend's own object for backend-specific code, and `libzhuyin.so.15` for Zhuyin.

## Requirements

### Requirement 1: Match the pin, defects included

Distributors must be able to replace the library without changing observable behaviour, including pin defects.

#### Acceptance Criteria

1. Reproduce each `libpinyin.so.15` and `libzhuyin.so.15` export's observations for identical inputs and state, except for Requirement 6's recorded artefacts.
   Preserve returns, out-parameters, pointed-to data, written lengths, handle transitions, written files, stderr bytes and process-wide side effects.
2. Reproduce pin defects and record them in `docs/findings/upstream-report-drafts.md`.
   Class (a)–(c) and Requirement 6's recorded artefacts remain exceptions.
3. Match the pin by default.
   Name each departure's class or record.
   Do not create another class or exception.
4. Do not compare successful-call `errno` unless the API specifies it (ruling 2026-10-08 UTC).

### Requirement 2: Behaviour is derived by reading the pin

Source derivation prevents time spent on experiments.

#### Acceptance Criteria

1. Cite each change's derivation as `path:line` at the pin, or symbol and instruction offsets.
2. Read source first.
   Read disassembly when source does not settle the behaviour.
3. Do not run the pin to discover its behaviour.
   Do not use exploratory probes, throwaway drivers, return-value experiments or tune-until-matching loops.
4. Stop when source and disassembly cannot settle behaviour.
   Report what you read and where it stopped answering the question.
5. Derive predicted-candidate ranking and training from the pin's source.
   Tuning until examples agree does not establish the rule.

### Requirement 3: A derived behaviour is gated by a differential

Differentials protect derived behaviour against regressions.

#### Acceptance Criteria

1. Gate each derived behaviour on the reference cell under the [verification rules](../../runbooks/verification.md).
2. Show the differential detects a reverted fix.
   If no fix exists because oxpinyin already reproduces the behaviour, use a deliberate, named mutation of oxpinyin.
   Never mutate the pin.
3. Add cases to an existing driver when it can accept them.
4. Mark recorded language-artefact cases `POLICY-ACCEPTED` with oxpinyin's idiomatic result.
   Do not change them towards the pin.

### Requirement 4: Performance floor, no ceiling

Do not worsen both time and space against the pin.
Compatible improvements have no ceiling.

#### Acceptance Criteria

1. Measure axis L on named workloads against the pin.
   Measure executed instructions or time, plus peak heap.
   State workload, build and repetitions; #757 is the example.
2. Fix a workload regression on both axes in Stage 2.
   A written justification does not replace the fix.
3. Trade a regression on one axis for improvement on the other.
   Minimize the regression and justify it under the [source policy](../../source-policy.md).
4. Faster or smaller implementations qualify for Stage 2 when output, files and ABI remain unchanged.
   Recorded language artefacts remain exceptions.
   Keep differentials identical, or `POLICY-ACCEPTED` for a recorded artefact.
   Eligibility is not approval.
   Satisfy safety, dependency, constitution §2 disclosure, review and verification rules too.
5. Do not land an observable or on-disk performance change in Stage 2 without an applicable exception.
   A registered class (a)–(c) or recorded language artefact can permit it.
   Otherwise it belongs to Stage 3.

### Requirement 5: Performance measured the same way on both sides

Performance claims must be reproducible and comparable.

#### Acceptance Criteria

1. Capture a baseline before changing code.
2. Build and time base, PR and pin in one run on one machine.
   State each artifact's build recipe.
   Only `cargo cinstall --release` is the product; see [benches](../../runbooks/benches.md).
3. Register decision rules and falsifiers before the run.
   Check build provenance before accepting an amd64 result below 1.
4. Do not make performance a PR gate.
   Use the [nightly snapshot series](../../perf/ci-perf-size-gate-proposal-2026-09-09.md) for tracking.

### Requirement 6: Language artefacts and the math rule

Implement product behaviour in idiomatic Rust while preserving the C contract.

#### Acceptance Criteria

1. Apply the policy's order: C contract, class (a)–(c), language artefact, product change, then defect.
2. Do not write Rust to reproduce a language artefact.
   Ask whether a language-neutral requirement would mention the behaviour.
   When uncertain, prefer idiomatic Rust and record the difference.
3. Keep the Rust result when valid or malformed input differs only through a language artefact.
   Use `docs/findings/language-runtime-discrepancy-template.md`.
   Register it by name in `docs/findings/upstream-divergences.md` and retain a `POLICY-ACCEPTED` case.
   No approval is required.
4. Preserve C ABI/layout, ownership/lifetime, every exported symbol, file formats and data ownership exactly.
   This includes real `GArray`, `g_free` and other GLib-owned structures.
   None is a language artefact.
5. Stop for a human decision on a product change, untraceable path or conflict with an earlier maintainer ruling.
   Product changes include ranking rules, limits, refusals, option meanings and pin defects.
6. Use Rust's standard maths at full speed in every crate, including training and word.
   This covers `f64::ln`, `exp`, `powf` and every other `std` float method.
   Do not slow code to imitate glibc, libm, GLib `gfloat` or other C behaviour.
   Record resulting last-bit differences as class (a).
   Do not chase them.
