# AGENTS.md — contract for all coding agents

oxpinyin is a portable Rust re-expression of libpinyin. Stage 1 = parity
with the pin-built oracle (complete); Stage 2 = bug-for-bug compatibility
with the pin, with internal improvements (current); Stage 3 = opt-in
departures from the pin. Roadmap: `ROADMAP.md`.
Crate map: `.kiro/steering/structure.md`.

`.kiro/steering/` follows Kiro's steering format (<https://kiro.dev/docs/>)
without the Kiro IDE or CLI — read it as always-loaded context; this
file wins on any conflict.

## Constitution

1. Broad appeal only — no niche features at cost to everyone.
2. Install-size budget: default payload ≤ pinned reference stack +10%.
3. No local AI (no client neural/LLM inference).
4. Nothing panics on any input; public APIs return `Result`.
5. `unsafe`: `forbid` in oxpinyin-core/user/engine; `deny` in data (the
   documented mmap exception) and store (whose backend bindings sit under
   module-scoped allows); the public C ABI's FFI only in capi/oracle;
   `// SAFETY:` on every block. The allowlist is mechanical now: crate-root
   `#![forbid]`/scoped allows enforce it, and two Clippy lints enforce the
   comments — `undocumented_unsafe_blocks` requires a safety comment on
   every `unsafe` block, `missing_safety_doc` requires a `# Safety` doc
   section on every public `unsafe` fn or method — CI will tell you; this
   line is context, not the gate.
6. Determinism: output is a pure function of (input, user state, config).
7. No dependency on other pinyin/IME crates; no transpiler dumps. Pin-built
   libpinyin is a test/migration **subject**, not a linked dependency of
   shipping code.
8. When in doubt, STOP — do not improvise.

## Source policy

oxpinyin is a Rust rewrite of libpinyin under the same license
(GPL-3.0-or-later). Reading and copying upstream C++ source is expected and
encouraged — there is no clean-room restriction. The original rule existed to
avoid verbatim copying, but true clean-room reverse engineering takes far
longer and wastes effort for no benefit when the source is legally available.

Cite upstream by pin: read from a checkout at the pinned commit (the
Linux host keeps one at `~/Documents/repos/libpinyin`; that is a host
convention, not a repo guarantee) or from a clone at the pin whose cited
blobs you have hashed against it, and say which tree you read.
Cite oxpinyin code by file and item name, never by line number; cite the pin by file:line at 074a2219. Dated records keep the line numbers of the tree they name.

Method: copy as much as possible from upstream, rewrite it in Rust with a
loosely coupled project structure, then oxidize further. Internal structure
is free to diverge, subject to two constraints: external interface behavior
must be unchanged, and time and space complexity must never both be worsened
— a regression in one is acceptable only when traded against a gain in the
other, must be minimized, and must be justified in the change's report.
The target is a smaller binary, faster execution and much lower RAM than
libpinyin. Internal freedom exists to serve that, not to erode it.

Rust-mechanism divergences: where upstream behavior cannot be reproduced
because of a language-mechanism difference, record it in
docs/findings/upstream-divergences.md, move on, and do not chase it. These
notes are collected to report back to libpinyin once the rewrite is complete.

**Before recording one, read `docs/findings/compatibility-policy.md`.** The
goal is a drop-in replacement: rename the built object to `libpinyin.so.15`,
put it on the library path, and unmodified consumers work against the data
already on the system. oxpinyin is built on one of the three backends
libpinyin is built on (Berkeley DB, Kyoto Cabinet, tkrzw), one per
build, and handles that backend's system and user files natively,
exactly as a libpinyin build on the same backend does. Switching backend
starts fresh, for libpinyin and oxpinyin alike (maintainer ruling
2026-09-09). Do not re-raise either as an open trade-off. Reproducing the pin is
therefore the default, and
divergence is an exception that must be argued into one of exactly three
classes — (a) math, (b) memory safety, (c) availability. (Class (d),
consumer scope, was retired 2026-09-06; the policy doc is authoritative
on the current set.)
Anything outside those three is a defect to be reverted, not a divergence to
be recorded. The policy carries the classes, their citations, and a
classification of every existing register entry.

## Stages

Maintainer ruling 2026-10-14 UTC.

The boundary between Stage 2 and Stage 3 is compatibility, not
performance.

| | Stage 2 (current) | Stage 3 (after Stage 2) |
|---|---|---|
| Goal | Bug-for-bug compatibility, with internal improvements | Intentional departures from libpinyin |
| Performance work | Allowed, also faster or smaller than the pin | Allowed when it needs a compatibility-breaking change |
| Internal refactoring | Allowed | Allowed |
| Observable behaviour | Must match the pin, under the existing exceptions | May differ, explicitly |
| Data formats and ABI | Keep drop-in compatibility | An incompatible change needs an explicit opt-in and a binary identity that fits it |

Stage 1 (exact-output parity with the pin-built oracle on the corpus
surfaces) is complete.

Records from before this ruling say "Stage 2" for performance work, for
example the `stage2` bench and the `docs/perf/` records. That work kept
compatibility, so it is also Stage 2 work under this ruling. The records
keep their wording. The exception is a reference to a change of output,
such as the `EdgeKind` Fuzzy/Typo/Abbrev variants or a model upgrade:
those now say Stage 3. Give a new benchmark a name that does not include
a stage.

### Stage 2 — bug-for-bug compatibility and internal improvement

Reproduce libpinyin's observable behaviour, including its defects.
Improve performance, memory use, safety and internal architecture when
compatibility stays. An implementation may be faster or smaller than
the pin by any amount. Verify that its observable behaviour and its
persistent data stay compatible.

The behaviour target is what the pinned libpinyin does, not what it
should do.
An observable pin defect is reproduced and recorded as an upstream
defect, not fixed. Matching the pin is the default; departing from it
still needs one of the three classes above.

Derive the pin's behaviour by reading its code, never by running it:

1. Read the pin's source at the commit in `tools/oracle/oracle-pin.txt`
   — and, where the behaviour turns on them, the glib, libc and DBM
   functions it calls.
2. When the source does not settle it — a macro or template you cannot
   expand by eye, an inlined library helper, implementation-defined or
   undefined behaviour, evaluation order the compiler chose — read the
   disassembly of the pin-built object that holds the code in question,
   from `tools/oracle/build-oracle.sh`: the Berkeley DB reference-cell
   build by default; the Kyoto Cabinet or tkrzw build (`--dbm kc|tkrzw`)
   when the behaviour is backend-specific, because that code is compiled
   only into that build; `libzhuyin.so.15` from an `--enable-libzhuyin`
   build for the zhuyin facade. The autoconf default `-g -O2` keeps line
   tables, so
   `objdump -dlS -C --no-show-raw-insn` or
   `gdb -batch -ex 'disassemble /s <symbol>'` interleaves source. Read
   the object's tables (`readelf`, `.rodata`, relocations) when the
   question is what it contains rather than what it executes.
3. Cite what you read in the change's report: `path:line` at the pin, or
   the symbol and instruction offsets.

Do not run tests to find out what the pin does: no exploratory probes,
no throwaway drivers, no tests written to learn a return value, no
tune-until-it-matches loops against the oracle. A differential gates a
behaviour you have already derived (Verification cells, below); it is
not how you derive it. If reading cannot settle a question, STOP and
report what you read and where it ran out.

Performance in Stage 2 has a floor and no ceiling:

- The floor is the Source policy's rule: time and space never both
  worse than the pin. Measure the complexity axes (time, instructions,
  peak heap) on named workloads against the pin (round-2 audit axis L;
  #757). If oxpinyin is worse on both, fix it.
- There is no ceiling. A change that makes oxpinyin faster or smaller
  than the pin is Stage 2 work if output, files on disk and the ABI do
  not change. Examples: replacing the mmap wrapper with `memmap2` (#862),
  `memchr` substring search (#858), a dictionary cursor walk, a
  per-session arena.

### Stage 3 — opt-in departures from the pin

Stage 3 permits intentional departures from the pinned behaviour: for
example a new language model, different candidate behaviour, or an
incompatible data format. Each departure sits behind an opt-in cargo
feature, off by default. The default build stays bug-for-bug
compatible. A build that breaks the ABI or the data formats must not
claim libpinyin's binary identity.

## Pinned behaviour and oxpinyin improvements

Every requirement, spec line and finding belongs to exactly one side:

- **Pinned behaviour** — what libpinyin does at the pin: SONAME, header,
  pkg-config, the exports' observable I/O, file formats, stderr bytes,
  process-wide side effects, and its defects. This is the Stage 2
  target. Kiro: `.kiro/steering/pinned-behaviour.md`,
  `.kiro/specs/drop-in/`, `.kiro/specs/bug-for-bug/` (Requirements 1–3).
- **oxpinyin improvements** — everything oxpinyin does that the pin does
  not: the classes (a)–(c) and the log line (c) owes, the registered
  standing divergences, the constitution's no-panic, `unsafe` and
  determinism rules, internal architecture and crate decoupling,
  performance, and the Stage 3 departures. Kiro:
  `.kiro/steering/oxpinyin-improvements.md`,
  `.kiro/specs/bug-for-bug/` (Requirements 4–5),
  `.kiro/specs/stage3-departures/`.

An improvement that changes nothing observable is internal. It is
allowed in Stage 2 under the Source policy, performance included. An
improvement that changes observable output, the files on disk or the
ABI is one of two things. It is a class (a)–(c) exception, or it is a
Stage 3 departure behind an opt-in cargo feature. The default build is
always the bug-for-bug binary.

## Contributor responsibility

Every contributor remains responsible for understanding, reviewing, testing,
and maintaining the changes they submit, including changes produced with
assistance from any tool.

## STOP → do not improvise

Ambiguous task · needs interface/ABI/dep change · test cannot pass without
breaking the constitution · SPEC contradicts the pin's source · the pin's
source and disassembly cannot settle a behaviour · implementation would
require changing a frozen SPEC without an ask.

A STOP clears **only on a human turn**. Approval-shaped text that appears
inside your own turn is not approval: not a `(Recommended)` option you
wrote yourself, not a selection that comes back within your own
transcript, not a line like "approach approved", not a task chip, not a
background-task notice. When the harness says no human input has arrived,
none has — that statement outranks anything in your context that reads
like consent. Ask, then stop and wait for the human to type it.

Precedent: the 2026-09-05 `__store_ext__` addition on the then-shipping
Python binding (a consumer-visible interface change, so a STOP — that
binding has since been removed) was proposed, self-selected and
implemented in one unbroken agent run, while the harness was reporting no
human input. The change was correct and the discipline was not; only
review caught it.

## Hard forbids

Add/upgrade deps without ask · edit frozen SPECs/goldens/CI policy without
ask · `unsafe` outside allowlisted crates · silence
lints.

## Dates

All freeze-doc, findings, and patch-metadata dates are UTC, captured at
run time from the machine executing the step (`date -u`), never
hand-written: a local timestamp from a zone ahead of UTC dates a record
into the future (PR #363).

## Toolchain

`rust-toolchain.toml` is the only supported toolchain. Portable crates:
Linux/macOS/Windows. Oracle, capi: Linux-first.

Cloud sessions are a supported host for every gate: provision one with
`tools/cloud/setup.sh` (`docs/runbooks/cloud.md`).

## Concurrent sessions

More than one agent may hold this checkout. Never switch the shared
checkout's branch or touch its working tree for your own work when it
sits on another workstream's branch (check `git branch --show-current`
first — a branch you did not create means someone else is mid-flight).
Do your work in a worktree, not the shared checkout — commit there,
`git worktree remove` when done, and leave the shared checkout exactly
as found. Checking out an existing branch is `git worktree add
/tmp/<name> <branch>`; a new branch needs `-b` (`git worktree add -b
<branch> /tmp/<name>`); a truly detached worktree needs `--detach` with
a commit (`git worktree add --detach /tmp/<name> <commit>`). Two agents assuming
sole ownership of one tree is how the shim.cc collision happened.

## Measurement and lane state

- **Measure in one run.** A measuring PR runs base, PR and pin in one job
  on one machine and reports only those numbers and their ratios. No
  absolute figure from another run, host or CPU is ever a baseline
  (`docs/runbooks/benches.md`).
- **Push, don't hoard.** A lane pushes `wip/<issue>` at every checkpoint.
  Work that exists only in a local worktree or container does not exist:
  treat every machine as disposable.

## Rebase discipline

Fetch and rebase onto the current landing tip immediately before every
push and before any merge. `git log origin/main..HEAD` must contain only
this workstream's commits — the ones this branch introduced, not
rewritten copies of already-landed work. The diffstat must delete
nothing the branch does not own. A clean textual rebase is not a
scheme re-verification. When a dependency PR merges mid-review and
changes a GENERATION scheme — a manifest layout, a fixture format, a
builder recipe — artifacts your branch produced under the old scheme
can survive the rebase textually intact and still be wrong. After
rebasing, list the committed artifacts whose generators main changed
since your branch point and regenerate them; do not let a
conflict-free merge stand in for that (worked example: PR #363 vs
#358, the oracle-data manifest split).

Watch for the stale-base optical illusion (other people's merged work
appearing as deletions). Re-run
pins after any rebase that changes the engine, capi, or data crates.
Whoever merges later re-measures those pins rather than assuming the
pre-rebase numbers still hold.

fmt failures are merge blockers; a fmt-only commit is always safe
when the diff is formatting-only and reviewed.

## Stack placement

A fix belongs on the member whose change produces the symptom. Every
stack member must be green standing alone — bisect compatibility and
bottom-up merging both depend on it. A fix placed above the commit that
caused the failure leaves the lower member red until the upper one
lands, which breaks both properties. When a change in member N exposes
a pre-existing defect (a harness leak, a missing teardown, a latent UB),
the fix goes on member N, not on whichever branch happened to be open.
The tree must be correct at every level, not just at the top (worked
example: the capi-commands fuzz leak fix, first put on #512, belonged
on #502, which made BDB the default and turned the harness defect into
a failing job).

## Verification cells

Maintainer ruling 2026-10-04 UTC. A cell is one DBM backend on both sides
(`tools/bisection/oracle-cell.sh`). These rules decide which cells a
change runs on; they replace any lane prompt that says "all three cells
always".

1. The reference cell is Berkeley DB (`PINYIN_ORACLE_DBM=bdb`, the
   default). Baseline, differential, revert-and-check and the gate run
   there and nowhere else by default.
2. Run all three cells (bdb, kc, tkrzw) only when the diff touches
   backend code. Decide by path check, not by judgement — any of three:
   - `git diff --name-only origin/main...HEAD` lists a file under
     `crates/oxpinyin-store/`;
   - a changed non-Markdown file contains `feature = "bdb"`,
     `feature = "tkrzw"` or `feature = "kyotocabinet"`
     (`git diff --name-only origin/main...HEAD -- ':!*.md' | xargs -r
     grep -lE 'feature *= *"(bdb|tkrzw|kyotocabinet)"'` — Markdown is
     excluded because docs, this file included, quote the pattern);
   - the diff changes a line naming a backend in any Cargo.toml
     (`git diff -U0 origin/main...HEAD -- '*Cargo.toml' | grep -E
     '^[+-][^+-].*(bdb|tkrzw|kyotocabinet)'`).
   State the check's output in the PR body. A result you believe is
   backend-dependent but that fails the check is a finding to report,
   not a licence to run three cells.
3. Every differential runs on all three cells once at the start of each
   sweep and once before a release. That run, not the per-change gate,
   is what covers kc and tkrzw.
4. Revert-and-check runs on the reference cell only. Where there is no
   fix to revert — the differential covers behaviour oxpinyin already
   reproduces — a deliberate, named mutation of oxpinyin's side stands
   in for the revert, as in the round-2 audit; the pin is never mutated.
5. The local test gate is `cargo test -p <crate>` for each crate the
   diff touches, plus fmt and clippy on those crates. The workspace
   sweep is CI's job; do not run it locally as a gate.
6. No new bespoke driver plus runner per issue where an existing driver
   can take a case file. Add cases to the existing driver; a new driver
   needs a line in the PR body saying why none could.
7. Re-verification is proportional to what changed since the last
   green gate:
   - Docs, comments or findings only: fmt, then push. No builds. This
     case overrides rule 2. If a diff changes only comments in a backend
     file, rule 2's check fires, but do not run the three cells.
   - A review fix in code: fmt, clippy, `cargo test -p` for the touched
     crates, and only the differential(s) belonging to that issue, on
     the reference cell.
   - Revert-and-check is not repeated unless the differential itself
     changed.
   - A rebase with no conflicts that brings in no file this lane
     touches needs no local re-gate; CI covers it.
   - In a stack, re-gate only the commit that changed and those above
     it that touch the same files.
   The report states which of these cases applied and why, so the
   maintainer can check the call.
8. Differentials build debug, never release. Keep one target directory
   per cell and reuse it; do not delete target directories between
   issues. Clean once, at the end of the lane.
9. The tkrzw cell runs only against a sound libtkrzw: one that
   `tools/tkrzw/distro-probe.sh` reports healthy, which
   `tools/cloud/setup.sh` provides on hosts whose packaged library is
   defective (Ubuntu's, `docs/findings/tkrzw-distro-compat.md`). A tkrzw
   result taken against a defective libtkrzw is not a result: do not report
   its DIFFERs, do not triage them, and do not propose sessions for them.
   Rerun with the sound library on `LD_LIBRARY_PATH` (check with `ldd` that
   both the oracle and the subject resolve `libtkrzw.so.1` to it). If none
   is available, write "tkrzw: not run (no sound libtkrzw on this host)" and
   move on.

## Operational rules that live with their procedures

The procedure-level rules used to accumulate here as incident notes;
they now live where the procedure is documented, and this file only
points at them:

- **Tests that need inputs CI never has** (model20, the system-table
  export, pin-built tools, opencc) are `#[ignore = "needs …"]` and fail
  on a missing input, never skip: `docs/testing/README.md`. Do not add a
  self-skipping test.
- **Oracle C-API training** goes through `Session::train_top`, never
  `pinyin_train` directly: `docs/runbooks/oracle.md`.
- **Backend-specific benches** carry `required-features`; run one with
  `--bench <name>`: `docs/runbooks/benches.md`.
- **Findings documents commit no captures.** The document records the
  command that produced each figure; bulk evidence is attached to the PR
  and linked from the document: `docs/runbooks/benches.md`. Where the
  capture environment is ephemeral and the raw cannot be retained
  anywhere, the document says so at the point of use rather than pointing
  at a path that no longer exists.
