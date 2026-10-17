# Requirements Document — Stage 2: bug-for-bug compatibility

## Introduction

Stage 2 (maintainer ruling 2026-10-14 UTC) makes oxpinyin match the
pinned libpinyin bug for bug across the whole exported ABI and the
user-store surfaces. Requirements 1–3 are pinned behaviour
(`.kiro/steering/pinned-behaviour.md`). Requirement 4 is different: it
is an oxpinyin guardrail, not a behaviour of the pin. It keeps oxpinyin
from being worse than the pin on both time and space. Where oxpinyin may
differ from the pin, the rule is in
`.kiro/steering/oxpinyin-improvements.md`. Performance beyond the pin is
Stage 3 (`.kiro/specs/stage3-performance/`).

## Glossary

- **Pin** — libpinyin at the commit in `tools/oracle/oracle-pin.txt`
  (2.11.92, `074a2219`).
- **Reference cell** — the Berkeley DB build on both sides
  (`PINYIN_ORACLE_DBM=bdb`).
- **Pin defect** — an observable behaviour of the pin that upstream
  would call a bug.
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
   process-wide side effects.
2. WHEN the pin's behaviour is a pin defect THEN oxpinyin SHALL
   reproduce it and record it as an upstream defect
   (`docs/findings/upstream-report-drafts.md`), unless the policy places
   it in class (a), (b) or (c).
3. Matching the pin SHALL be the default; a departure SHALL name its
   class and SHALL NOT create a new one.
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

### Requirement 4: Complexity no worse than the pin; no optimisation beyond it

**User Story:** As the maintainer, I want oxpinyin never worse than the
pin on both time and space by the end of Stage 2, without spending
Stage 2 on beating the pin.

#### Acceptance Criteria

1. THE complexity axis (round-2 audit axis L) SHALL be measured against
   the pin on named workloads — executed instructions or time, and peak
   heap — with the workload, build and repeats stated (#757 is the
   worked example).
2. WHEN oxpinyin is worse than the pin on both time and space for a
   workload THEN the regression SHALL be fixed in Stage 2, by reproducing
   the pin's way of doing the work, or the trade justified per the
   Source policy.
3. A change whose aim is to be faster or smaller than the pin SHALL be
   reported as a Stage 3 lead in the change's report, not implemented.
