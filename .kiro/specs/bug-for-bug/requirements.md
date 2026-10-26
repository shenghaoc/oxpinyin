# Requirements Document — Stage 2: bug-for-bug compatibility and internal improvement

## Introduction

Stage 2 (maintainer ruling 2026-10-14 UTC, amended 2026-10-26 UTC by
#1303) makes oxpinyin match the pinned libpinyin's product behaviour bug
for bug across the whole exported ABI and the user-store surfaces, in
idiomatic Rust. The programming language is an implementation detail:
behaviour that comes only from C or C++ semantics or from a library
written for another language is a language artefact and is not
reproduced (Requirement 6). Stage 2 also allows internal improvement
that keeps compatibility. This spec contains requirements from both sides:

- Requirements 1–3 are pinned behaviour
  (`.kiro/steering/pinned-behaviour.md`).
- Requirements 4–6 are oxpinyin rules, not behaviours of the pin
  (`.kiro/steering/oxpinyin-improvements.md`). Requirement 4 sets a
  performance floor and no ceiling. Requirement 5 sets the measurement
  method. Requirement 6 sets the language-artefact rule and the math
  rule.

Where oxpinyin may differ from the pin, the rule is in
`.kiro/steering/oxpinyin-improvements.md` and the decision order in
`docs/findings/compatibility-policy.md`. Intentional departures
from the pin are Stage 3 (`.kiro/specs/stage3-departures/`).

## Glossary

- **Pin** — libpinyin at the commit in `tools/oracle/oracle-pin.txt`
  (2.11.92, `074a2219`).
- **Reference cell** — the Berkeley DB build on both sides
  (`PINYIN_ORACLE_DBM=bdb`).
- **Pin defect** — an observable product behaviour of the pin that
  upstream would call a bug: a logic defect that idiomatic Rust
  reproduces as written.
- **Product behaviour** — what a user, or a developer calling the shared
  object, observes and a language-neutral requirement would state: which
  candidates, in what order, under which options; what is saved and
  loaded; what the C API returns, owns and frees; what lands on disk.
- **Language artefact** — a behaviour that comes only from C or C++
  semantics (an integer-returning comparator, unsigned wrap-around,
  float-to-integer truncation, an integer width chosen for storage) or
  from a library written for another language (GLib, glibc or another
  libc, libstdc++), or from compiler or processor behaviour. A width or
  wrap that the C ABI, an on-disk format or a domain algorithm prescribes
  is not an artefact.
- **Derivation** — the pin's source lines, or the symbol and
  instruction offsets in the pin-built object holding the code (the
  reference cell's by default, the backend's own for backend-specific
  code, `libzhuyin.so.15` for the zhuyin facade), that a reproduced
  behaviour was read from.

## Requirements

### Requirement 1: Match the pin, defects included

**User Story:** As a distributor, I want swapping libpinyin for oxpinyin
to change nothing observable, the pin's bugs included, so that consumers
built against libpinyin's actual behaviour keep working.

#### Acceptance Criteria

1. FOR every export of `libpinyin.so.15` and `libzhuyin.so.15`, given
   the same inputs and state, oxpinyin SHALL produce the pin's return
   status, out-parameters and the data they point to, written lengths,
   handle state transitions, files written, stderr bytes and
   process-wide side effects, except where Requirement 6 records a
   language artefact.
2. WHEN the pin's behaviour is a pin defect THEN oxpinyin SHALL
   reproduce it and record it as an upstream defect
   (`docs/findings/upstream-report-drafts.md`), unless the policy places
   it in class (a), (b) or (c), or the difference is a language
   artefact recorded under Requirement 6.
3. Matching the pin SHALL be the default; a departure SHALL name its
   class or its record and SHALL NOT create a new class or exception.
4. THE `errno` a successful call leaves behind SHALL NOT be compared
   unless the API specifies it (ruling 2026-10-08 UTC).

### Requirement 2: Behaviour is derived by reading the pin

**User Story:** As the maintainer, I want every reproduced behaviour
derived from the pin's code, so that no agent time goes on experiments.

#### Acceptance Criteria

1. EACH behaviour change SHALL cite its derivation: `path:line` at the
   pin, or symbol and instruction offsets in the pin-built object.
2. THE source SHALL be read first; THE disassembly SHALL be read when the
   source does not settle the behaviour.
3. THE pin SHALL NOT be run to discover its behaviour — no exploratory
   probes, no throwaway drivers, no tests written to learn a return
   value, no tune-until-it-matches loops.
4. WHEN neither source nor disassembly settles a behaviour THEN the agent
   SHALL STOP and report what it read and where reading ran out.
5. Predicted-candidate ranking and training SHALL follow rules derived
   from the pin's source; tuning until examples agree SHALL NOT count.

### Requirement 3: A derived behaviour is gated by a differential

**User Story:** As the maintainer, I want each derived behaviour held in
place by a test, so that it cannot silently regress.

#### Acceptance Criteria

1. EACH derived behaviour SHALL be gated by a differential on the
   reference cell, under `/AGENTS.md` "Verification cells".
2. THE differential SHALL be shown non-vacuous: by reverting the fix
   (revert-and-check), or, WHEN oxpinyin already reproduced the behaviour
   and there is no fix to revert, by a deliberate named mutation of
   oxpinyin's side. The pin SHALL NOT be mutated.
3. New cases SHALL go into an existing driver where one can take them.
4. A case that differs from the pin only through a recorded language
   artefact SHALL carry `POLICY-ACCEPTED` with oxpinyin's idiomatic
   result; it SHALL NOT be fixed towards the pin.

### Requirement 4: Performance floor, no ceiling

**User Story:** As the maintainer, I want oxpinyin never worse than the
pin on both time and space, and free to be better by any amount, while
compatibility stays.

#### Acceptance Criteria

1. THE complexity axis (round-2 audit axis L) SHALL be measured against
   the pin on named workloads — executed instructions or time, and peak
   heap — with the workload, build and repeats stated (#757 is the
   worked example).
2. WHEN oxpinyin is worse than the pin on both time and space for a
   workload THEN the regression SHALL be fixed in Stage 2. A written
   justification SHALL NOT replace the fix.
3. WHEN a change makes oxpinyin worse on one axis only THEN the change
   SHALL make the other axis better. The change SHALL keep the
   regression as small as possible, and its report SHALL justify the
   trade (`/AGENTS.md`, Source policy).
4. A change that makes oxpinyin faster or smaller than the pin is
   eligible for Stage 2 WHEN observable output, the files on disk and the
   ABI do not change, other than through a recorded language artefact.
   Its differentials SHALL stay identical, or `POLICY-ACCEPTED` for a
   recorded artefact. Eligibility
   is not approval: the change SHALL also satisfy the other rules that
   apply to it (safety, dependencies, binary-size disclosure under
   Constitution §2, review and verification).
5. A performance change that needs an observable or on-disk change
   SHALL NOT land in Stage 2, unless a registered class (a)–(c)
   exception covers that change or the change is a recorded language
   artefact (`docs/findings/compatibility-policy.md`). Otherwise the
   change is a Stage 3 departure.

### Requirement 5: Performance measured the same way on both sides

**User Story:** As the maintainer, I want every performance claim to be
reproducible and like-for-like.

#### Acceptance Criteria

1. A baseline SHALL be captured before the code change.
2. Base, PR and pin SHALL be built and timed in one run on one machine
   (`/AGENTS.md`, "Measure in one run"). Each record SHALL state the
   build recipe of each artifact. Only `cargo cinstall --release` is the
   product (`docs/runbooks/benches.md`).
3. Decision rules and falsifiers SHALL be registered before the run. A
   result below 1 on amd64 SHALL have its build provenance checked
   before it is accepted.
4. Performance SHALL NOT be a PR gate. The nightly snapshot series is
   the tracking surface (`docs/perf/ci-perf-size-gate-proposal-2026-09-09.md`).

### Requirement 6: Language artefacts and the math rule

**User Story:** As the maintainer, I want the product's behaviour
implemented in idiomatic Rust, with no effort spent imitating another
programming language or a library written for one, and without losing
the C contract.

#### Acceptance Criteria

1. A difference from the pin SHALL be decided in the policy's order: the
   C-facing contract first, then classes (a)–(c), then a language
   artefact, then a product-level change, otherwise a defect.
2. No Rust SHALL be written to reproduce a language artefact. Whether a
   behaviour is an artefact SHALL be decided by asking whether a
   language-neutral requirement for it would mention it; when unsure,
   the agent SHALL err towards idiomatic Rust and record the difference.
3. WHEN idiomatic Rust differs from the pin only through a language
   artefact, on valid or malformed input, THEN the Rust result SHALL
   stand, and the difference SHALL be recorded with
   `docs/findings/language-runtime-discrepancy-template.md`, registered
   by name in `docs/findings/upstream-divergences.md` and carried by a
   `POLICY-ACCEPTED` differential case. No approval SHALL be required.
4. THE C ABI and layout, ownership and lifetime (real `GArray`, `g_free`,
   other GLib-owned structures), every exported symbol, the on-disk
   formats and data ownership SHALL be preserved exactly; none of them
   is an artefact.
5. A product-level change (a ranking rule, a limit, a refusal, the
   meaning of an option, a pin defect), an untraceable path, or a
   conflict with an earlier maintainer ruling SHALL STOP for a human
   decision.
6. EVERY math function SHALL use Rust's standard maths at full speed —
   `f64::ln`, `exp`, `powf` and every other `std` float method — in
   every crate, training and word included. oxpinyin SHALL NOT slow code
   down to imitate glibc, libm, GLib `gfloat` or any other C behaviour.
   A last-bit difference from the pin that follows from using Rust's
   maths SHALL be recorded as class (a) and SHALL NOT be chased.
