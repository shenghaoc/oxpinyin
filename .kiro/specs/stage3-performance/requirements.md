# Requirements Document — Stage 3: performance

## Introduction

Stage 3 (maintainer ruling 2026-10-14 UTC) makes oxpinyin smaller,
faster and much lighter on RAM than libpinyin. The default build stays
the bug-for-bug binary Stage 2 established; anything that is not
bug-for-bug sits behind an opt-in compile flag. It is an oxpinyin improvement
(`.kiro/steering/oxpinyin-improvements.md`); the behaviour it must keep
is `.kiro/steering/pinned-behaviour.md`. It starts after Stage 2. Until
the 2026-10-14 relabel this stage was called Stage 2.

## Requirements

### Requirement 1: The default build stays bug-for-bug

**User Story:** As a distributor, I want the default oxpinyin build to
stay the bug-for-bug libpinyin binary, so that performance work never
costs compatibility unless someone opts in at build time.

#### Acceptance Criteria

1. IN the default build, EACH Stage 3 change SHALL keep the E2E I/O rule
   as Stage 2 left it: every differential that was identical stays
   identical, and the files on disk are unchanged.
2. A Stage 3 change that is not bug-for-bug — any change to observable
   output or to the files on disk — SHALL sit behind an opt-in cargo
   feature, off by default, and SHALL NOT change the default build. Model
   upgrades (trigram/KN, typo edges, own data) remain candidates under
   this rule.
3. Storage SHALL stay the three backends libpinyin is built on, one per
   build, each handled natively; a Stage 3 change SHALL NOT add a
   backend.

### Requirement 2: Smaller, faster, lighter than the pin

**User Story:** As a user, I want the input method to start quickly and
use little memory and disk.

#### Acceptance Criteria

1. Stage 3 SHALL measure binary size, init time, steady keystroke-cycle
   time, RSS and on-disk data size against the pin.
2. THE default install payload SHALL stay within the pinned reference
   stack +10% (constitution §2).
3. An internal change SHALL NOT worsen time and space both; a regression
   in one SHALL be traded against a gain in the other, minimized and
   justified.

### Requirement 3: Measured the same way on both sides

**User Story:** As the maintainer, I want every performance claim
reproducible and like-for-like.

#### Acceptance Criteria

1. A baseline SHALL be captured before any code change.
2. Both sides SHALL be built and timed in the same container; each
   record SHALL state the build recipe of each artifact, and only
   `cargo cinstall --release` is the product
   (`docs/runbooks/benches.md`).
3. Decision rules and falsifiers SHALL be registered before the run; a
   sub-1 amd64 result SHALL have its build provenance checked before it
   is accepted.
4. Performance SHALL NOT be a PR gate; the nightly snapshot series is the
   tracking surface (`docs/perf/ci-perf-size-gate-proposal-2026-09-09.md`).
