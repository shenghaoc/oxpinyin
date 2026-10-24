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
2. Binary size (ruling 2026-10-18 UTC): for changes affecting shipping
   libraries, record stripped shipping-library file sizes and note increases
   over base in the PR; minor decreases need no mention. Documentation-only
   changes need no size build or report. No pin +10% cap.
   Details: `docs/perf/ci-perf-size-gate-proposal-2026-09-09.md`.
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

Method (`.kiro/steering/oxidation-method.md`, ruling 2026-10-16 UTC):
compatibility with the pin is Stage 2's acceptance requirement, and
internal refactoring, idiomatic rewrites and performance or memory
improvements are permitted alongside it when compatibility is
preserved (Stages, below). For each subsystem, record the pin's data
structures and algorithms
(`docs/findings/libpinyin-structures-and-algorithms.md`), design and
analyse oxpinyin's, then implement in idiomatic Rust across the loosely
coupled crate structure and gate on the oracle. Coefficients are the
pin's, never fitted. Internal structure
is free to diverge, subject to two constraints: external interface behavior
must be unchanged, and time and space complexity must never both be worsened
— a regression in one is acceptable only when traded against a gain in the
other, must be minimized, and must be justified in the change's report.
Binary size per PR is not complexity (it can be split into arbitrarily
small PRs), so §2 governs it outside the runtime time/space trade rule.
The target is a smaller binary, faster execution and much lower RAM than
libpinyin. Internal freedom exists to serve that, not to erode it.

Language and runtime-library divergences follow one decision order,
stated in full in `docs/findings/compatibility-policy.md` under "The
language/runtime-quirk exception":

1. **The C-facing contract is never a quirk.** The C ABI and its layout,
   ownership and lifetime (real `GArray`, `g_free`, every other
   GLib-owned structure and allocation), the on-disk system and user
   formats, data ownership, and observable behaviour on valid supported
   input are preserved exactly, except where class (a), (b) or (c)
   applies. Otherwise a proposed change there is a defect to revert, or
   a Stage 3 departure; the quirk limb below cannot admit it.
2. **If it is class (a), (b) or (c)**, classify it there and follow that
   class's rules.
3. **A GLib, glibc or C++ language/runtime quirk may be accepted**
   instead of emulated in Rust. oxpinyin uses normal Rust functions in
   place of GLib-conformity code. The exception is admitted only by a
   discrepancy record
   (`docs/findings/language-runtime-discrepancy-template.md`) after the
   whole reachable path is traced; it must be confined to malformed,
   non-scalar, host-dependent or otherwise undefined input, or to a
   diagnostic/comparator detail with no valid-input consequence, and it
   must preserve every item in limb 1. Register it by name in
   `docs/findings/upstream-divergences.md`; one record does not admit
   another by analogy, though the category itself is
   maintainer-authorized.
4. **Anything else is a defect to revert.** An intentional departure
   that changes valid behaviour or the contract is Stage 3, behind an
   opt-in feature.
5. **An earlier maintainer ruling is not reversed by this exception.**
   `LC_NUMERIC` (policy row 39, #923) needs a new human ruling before it
   changes.

A differential that intentionally differs under a recorded exception is
reported as `POLICY-ACCEPTED`, never as `MATCH`; its case and pin
evidence are kept.

**Math rule (maintainer direction 2026-10-19 UTC).** oxpinyin uses
Rust's standard maths at full speed: `f64::ln`, `exp`, `powf` and every
other `std` float method. It never slows code down to imitate glibc,
libm, GLib `gfloat` or any other C behaviour. A last-bit difference from
the pin that follows from using Rust's maths is class (a): record it and
do not chase it.

**Before recording a divergence, read
`docs/findings/compatibility-policy.md`.** The goal is a drop-in
replacement: rename the built object to `libpinyin.so.15`, put it on the
library path, and unmodified consumers work against the data already on
the system. oxpinyin is built on one of the three backends libpinyin is
built on (Berkeley DB, Kyoto Cabinet, tkrzw), one per build, and handles
that backend's system and user files natively, exactly as a libpinyin
build on the same backend does. Switching backend starts fresh, for
libpinyin and oxpinyin alike (maintainer ruling 2026-09-09). Do not
re-raise either as an open trade-off. Reproducing the pin is therefore
the default, and a divergence is an exception argued into class (a), (b)
or (c) or into the language/runtime-quirk exception. (Class (d),
consumer scope, was retired 2026-09-06; the policy doc is authoritative
on the current set.) The policy carries the classes, the exception, their
citations, and a classification of every existing register entry.

## External interfaces

Maintainer ruling 2026-10-18 UTC. Rust interfaces are not a compatibility
boundary.

> The only supported external library interfaces are the libpinyin-compatible and libzhuyin-compatible C APIs and ABIs.
>
> All native Rust interfaces are implementation details. No external Rust consumers are expected or supported, and there is currently no intention to stabilize a native Rust API.
>
> Rust interfaces may be changed incompatibly as required by the implementation. Such changes require no deprecation period, backwards-compatible adapter, preservation of object safety, or accommodation of hypothetical downstream Rust implementations. All affected in-tree callers must be updated in the same change.
>
> This ruling supersedes earlier Rust-interface stability promises, interface freezes, and restrictions imposed solely for Rust source compatibility.
>
> The C APIs and ABIs, their observable behavior, ownership and lifetime rules, required diagnostics, and compatible persistent data remain governed by the existing pinned-compatibility policy. Safety, determinism, portability, and performance requirements remain in force.

What this does and does not change:

- A change to a Rust `pub` item, trait, associated type, generic
  parameter, object safety, error type, visibility or module layout is
  ordinary internal work. It is not an interface change in the STOP
  sense below; update every in-tree caller (crates, tools, tests,
  benches, examples, fuzz targets) in the same change.
- Rust `pub` visibility, published crate names, doc comments and
  historical "frozen" or "unchanged" wording about a Rust interface
  create no obligation. Do not keep a wrapper, old constructor,
  defaulted trait method, object-safe shape or `dyn` boundary only to
  spare a Rust signature. Any future decision to support a stable
  native Rust API needs a separate explicit maintainer ruling.
- Frozen oracle inputs, goldens, coefficients, behavioural SPECs, data
  formats and the C compatibility contract stay frozen. "Frozen" means
  a Rust interface freeze only where a document says it freezes a Rust
  signature; those freezes are retired here.
- The Rust surface is not a reason for a divergence from the pin, and
  not a reason to skip a verification the C-facing contract needs.

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
still needs one of the three classes above or a recorded
language/runtime-quirk exception.

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
  standing divergences and recorded language/runtime-quirk
  discrepancies, the constitution's no-panic, `unsafe` and determinism
  rules, internal architecture and crate decoupling, performance, and
  the Stage 3 departures. Kiro:
  `.kiro/steering/oxpinyin-improvements.md`,
  `.kiro/specs/bug-for-bug/` (Requirements 4–6),
  `.kiro/specs/stage3-departures/`.

An improvement that changes nothing observable is internal. It is
allowed in Stage 2 under the Source policy, performance included. An
improvement that changes observable output, the files on disk or the
ABI is one of three things: a class (a)–(c) exception, a recorded
language/runtime-quirk discrepancy, or a Stage 3 departure behind an
opt-in cargo feature. The default build is always the bug-for-bug
binary under those recorded exceptions.

## Contributor responsibility

Every contributor remains responsible for understanding, reviewing, testing,
and maintaining the changes they submit, including changes produced with
assistance from any tool.

## Results and reproduction

Follow [the shared reporting policy](CONTRIBUTING.md#results-and-reproduction).
Deliver the software, maintained Python/`just` tooling, and a concise final
result with reproducible commands. One command may reproduce an entire table.
Keep only the revision, input and environment details needed to interpret and
reproduce it; a separate findings document is optional.

Raw verification output is disposable local working data. Do not commit,
attach, upload, archive or mirror logs, captures or tar/zip evidence bundles
unless the maintainer explicitly requests specific diagnostic material.
Never initiate a browser sign-in, account-setup or upload request to publish
verification output. Missing captures are not a blocker and require no
replacement manifest, checksum inventory or "evidence unavailable" statement.
Functional fixtures, pins and release packages are governed by their own
procedures; this rule concerns verification reporting.

A broken reproduction command or a result that contradicts the claim beyond
the method's expected variability is a bug to investigate. Fix the software,
tooling or documented assumptions and correct the claim. Do not replace that
investigation with an evidence package. This policy supersedes older reporting
requirements in runbooks, findings and lane instructions.

## Pull request descriptions

- Use [.github/PULL_REQUEST_TEMPLATE.md](.github/PULL_REQUEST_TEMPLATE.md)
  for every PR, including PRs made with `gh` or the API. Write the body to
  a file. Use `gh pr create --body-file <file>`, not `--fill`.
- Keep the template headings and their order.
- Use short, factual sentences. Do not use emoji, badges, decorative
  headings, or generic checklists.
- Do not write chronological logs, status updates, or pasted transcripts.
- When results change, edit the existing sections. Do not append a new
  report at the end.
- For a stacked PR, describe only that PR's own diff and name its direct
  parent PR.
- Keep an ordinary PR brief; 150–300 words is a guide, not a minimum.
  Report final results and reproducible commands under "Results and
  reproduction" above. Add detail only when it helps assess the change.
- The verification rules elsewhere in this file stay authoritative. This
  section changes only the format.

## Issue descriptions

- Use the appropriate template in `.github/ISSUE_TEMPLATE/` for every new issue,
  including issues created with `gh` or the API: bug report, task/proposal,
  or tracking issue. Search open and closed issues first; link existing work
  rather than creating duplicates.
- Preserve the chosen template's headings and order. For CLI/API creation,
  write a completed body file without YAML front matter or instruction
  comments, then submit it with `gh issue create --body-file <file>`; where
  GraphQL is unavailable, use `gh api repos/shenghaoc/oxpinyin/issues -f title=<title> -F body=@<file>`.
- Apply existing repository labels where they fit (for example bug-for-bug,
  axis:*, severity:*, backend:*). Do not create new labels.
- Use a specific, factual title and short sentences. No emoji, badges,
  decorative headings, agent/model attribution fields, transcripts, or
  chronological execution logs. Use checkboxes only for concrete outcomes
  or linked work items, not generic assurances.
- Separate observed results, source-derived conclusions, hypotheses, and
  proposals. Identify the revision relevant to evidence. Say "Unknown" or
  "Not reproduced" where appropriate; never invent results, dependencies,
  issue numbers, approvals, or acceptance thresholds.
- Keep each non-tracking issue focused on one outcome. Describe the problem
  before a proposed solution, and leave implementation choices open unless
  an explicit requirement settles them. Link extensive analysis rather than
  duplicating it. Do not create extra issues merely to fill a template.
- Keep the body current in its existing sections. Use comments for meaningful
  new evidence or decisions, not repeated status dumps. Preserve links to
  decisions when summarizing them; do not erase their history.
- Creating an issue does not authorize implementation or change project policy.
  Existing approval, compatibility, verification, and STOP rules remain
  authoritative. These templates change reporting format only.

## STOP → do not improvise

Ambiguous task · needs a C interface/ABI or dependency change (Rust
interfaces are free: External interfaces) · test cannot pass without
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
Python binding (a consumer-visible interface change, so a STOP then — that
binding has since been removed) was proposed, self-selected and
implemented in one unbroken agent run, while the harness was reporting no
human input. The change was correct and the discipline was not; only
review caught it.

## Reviews

An agent never requests or triggers a review on any PR, by any means.
This includes:

- a comment that mentions a review bot (`@coderabbitai review`,
  `@codex review` or similar);
- a request to a reviewer or a team;
- ticking a bot's "Trigger review" checkbox;
- changing a PR from draft to ready for review.

Only a human requests reviews. A question from a human about whether a
PR is ready for review is a question: answer it, and do not act on it.

Precedent: on 2026-10-22 UTC, an agent was asked whether three PRs were
ready for review. It marked one ready and posted six review-request
comments, without authority. A readiness question authorizes an answer;
only a human requests reviews.

## Hard forbids

Add/upgrade deps without ask · edit frozen SPECs/goldens/CI policy without
ask · `unsafe` outside allowlisted crates · silence
lints · request a review.

## Dates

All freeze-doc, findings, and patch-metadata dates are UTC, captured at
run time from the machine executing the step (`date -u`), never
hand-written: a local timestamp from a zone ahead of UTC dates a record
into the future (PR #363).

## Toolchain

`rust-toolchain.toml` is the only supported toolchain. Portable crates:
Linux/macOS/Windows. Oracle, capi: Linux-first.
Portable crates are architecture-neutral (x86_64 and arm64). linux/amd64 is only the reference for pin comparisons. Do not use emulation for work that does not compare against the pin.

Cloud sessions are a supported host for every gate: provision one with
`tools/cloud/setup.sh` (`docs/runbooks/cloud.md`).

Start with `CONTRIBUTING.md` and `docs/runbooks/setup.md`, the same setup
procedures humans use. Ordinary development is native and needs no oracle
or container. Alongside cloud provisioning, compatibility tasks may use
`tools/oracle/compat.sh`, which runs
the subject, pinned reference and harness together in the canonical pinned Debian Linux amd64 environment
(`docs/runbooks/compatibility-container.md`). Native tests are separate evidence.

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
   - the diff lists a file under `crates/oxpinyin-store/`;
   - a changed non-Markdown file contains `feature = "bdb"`,
     `feature = "tkrzw"` or `feature = "kyotocabinet"` (Markdown is
     excluded because docs, this file included, quote the pattern);
   - the diff changes a line naming a backend in any Cargo.toml.
   `tools/bisection/cells-for-diff.py [base]` runs the three checks on
   `base...HEAD` (`base` defaults to `origin/main`) and prints the cells
   they select, which check fired and the matching paths and lines.
   Rule 7's docs/comments-only case needs no backend-selection run.
   A result you believe is backend-dependent but that fails the check
   is a finding to report, not a licence to run three cells.
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

   **Unified tooling.** Use the repository's `just` recipes and in-tree
   Python tooling under `tools/`, with managed CPython via `uv`, for
   builds, gates, differentials and measurements. The migration plan is
   `docs/findings/tooling-consolidation-python-just.md`; existing documented
   shell entry points remain supported until migrated. Put new repeatable
   orchestration in the shared Python tooling and expose it through `just`.
   No ad-hoc drivers, private "operators" or out-of-tree controllers.
   Add only the missing capability needed for the task, with proportionate
   tests; do not expand an unrelated tooling migration or build reporting
   infrastructure. If a required check remains unavailable, report it as
   not run and state the blocker. Never substitute a private procedure or
   claim an unrun check passed.
7. Re-verification is proportional to what changed since the last
   green gate:
   - Docs, comments or findings only: review changed text and links,
     run `git diff --check`, then push. Run fmt when source comments
     change; Markdown-only edits need no Rust formatter. No builds,
     tests, clippy, differentials or backend-selection checks. This case
     overrides rule 2, including comments-only edits in backend files.
   - A review fix in code: fmt, clippy, `cargo test -p` for the touched
     crates, and only the differential(s) belonging to that issue, on
     the reference cell.
   - Revert-and-check is not repeated unless the differential itself
     changed.
   - A rebase with no conflicts that brings in no file this lane
     touches needs no local re-gate; CI covers it.
   - In a stack, re-gate only the commit that changed and those above
     it that touch the same files.
   Stop once the applicable checks pass and concrete failures are
   addressed. Additional runs need a changed relevant input, a specific
   unresolved failure or an explicit
   maintainer request; do not rerun merely to collect more evidence.
   Repetitions prescribed by the measurement method remain required.
   Rules 5 and 7 describe the **completion gate** (maintainer ruling
   2026-10-19 UTC): what runs when a coherent fix is ready, before a
   push that asks for review. Between two such points, while a fix is
   still taking shape, **iteration feedback** is enough: the named unit
   tests of the touched crate (`cargo test -p <crate> <test name>`),
   plus an explicit, small case selection of the differential that
   reaches the changed behaviour — the defect's witness case and its
   neighbours, named by case ID (`tools/bisection/run-contract-diff.sh
   ... -- --cases default-option-parse,default-option-single-key`),
   never the default suite.
   Iteration feedback is not a gate result and is not reported as one.
   Report the final results and reproduction commands, with failed or
   unrun required checks as limitations. A fix that never reaches a
   completion gate is not ready to push for review;
   a `wip/` checkpoint push does not need one.
8. Differentials build debug, never release. Keep one target directory
   per cell and reuse it; do not delete target directories between
   issues. Clean once, at the end of the lane.
   On Linux and macOS, heavy work uses the host-wide slot and the
   memory admission check (maintainer ruling 2026-10-19 UTC). Run direct
   cargo build, test and clippy commands through `just heavy`. The native
   slot is not validated on Windows. Heavy work includes cell builds
   and runners that `tools/bisection/runners.json` marks `"heavy": true`.
   The bisect, contract-diff, open-counter-diff, and transformed-options-sweep
   runners are heavy at every selection size. Direct `cargo build`,
   `cargo test`, and `cargo clippy` commands are heavy because they compile.
   Full or family-wide differentials, mutation runs, Valgrind, Massif,
   release builds, and LTO builds are also heavy. `run-all.py` applies
   both checks to cell builds and heavy runners. Start other heavy work
   with `just heavy -- <command>`.
   The heavy child receives an inheritable slot descriptor through
   `pass_fds`. The slot stays held until the child and its descendants
   close their copies. The parent keeps its copy until it reaps the child.
   It closes that copy without unlocking the shared file description.
   On SIGTERM, SIGINT, or SIGHUP, terminate each child process group.
   Reap the child before releasing checkout locks or the slot. Nested work
   uses `OXPINYIN_HEAVY_SLOT_HELD` to identify the slot and holder pid.
   Inherit only when the slot record names that pid, the pid is alive,
   and the pid is an ancestor. Report a stale marker and acquire normally
   if this check fails. Valid nested work does not acquire another slot.
   Two slots are the default when total memory is at least 15 GiB; one
   below that or when total memory is unknown. `OXPINYIN_HEAVY_SLOTS` sets
   the slot count.
   `OXPINYIN_LOCK_DIR` must name an absolute directory. Reject a relative
   value with a clear message. Resolve the default directory once. Pass
   the resolved path in each heavy child's environment. Nested calls use
   that path after a directory change. The slot limits concurrent
   heavy tasks. The defaults limit processes inside each task. Keep
   `CARGO_BUILD_JOBS`, `RUST_TEST_THREADS`, and `--jobs` unset unless a
   measurement needs fixed values. Explicit child settings stay unchanged.
   Cargo job counts accept positive integers, negative integers, and
   `default`. Test thread counts must be positive integers. Invalid values
   cause a clear failure. On Linux, read MemAvailable and each visible
   cgroup v2 ancestor's `memory.max` and `memory.current`. Use the smallest
   known memory room. Read CPU affinity and each ancestor's `cpu.max`.
   Report unreadable limits as unknown. Use the limits that can be read.
   macOS and Windows keep their platform memory readers. If no memory
   reading is available, treat the capacity as zero. Use no fixed fallback
   for heavy work. Explicit caller settings do not bypass this check.
   The plan before admission is provisional. Take the slot and read
   resources again. Print the effective settings and any change. Take the short host-wide
   `heavy-admission` lock before the final resource read and decision.
   Charge each live holder in another locked slot its estimate less the
   current use of its process tree (summed Pss on Linux, physical
   footprint on macOS, working sets on Windows), at least zero; charge the
   full estimate when any process's use cannot be read.
   Record this holder's estimate in its slot file before releasing the
   admission lock. Ignore records whose pid is dead. The cell build
   requires its build estimate plus the headroom: 10% of total memory, at
   most 2,048 MiB, and 2,048 MiB when total memory is unknown. Heavy runners
   require the larger of `RUNNER_PEAK_MIB` and their build estimate.
   The minimum build estimate is one rustc.
   Explicit settings require room for their full process counts.
   Under an inherited slot, insufficient memory causes immediate FAIL:
   `resource-blocked: inherited slot; <estimate> MiB needed, <available> MiB available`.
   Unknown memory under an inherited slot also causes immediate FAIL:
   `resource-blocked: available memory unknown`.
   Do not wait for memory while an ancestor holds that slot.
   If memory is insufficient or unknown, release this task's acquired
   slot before waiting. Hold no checkout lock during this wait. Print the
   estimate, available memory after headroom, and `no process started`.
   Retry slot acquisition and resource checks until admission or expiry.
   `OXPINYIN_RESOURCE_WAIT_SECONDS` limits admission waits; its default
   is 1800 seconds. One limit covers the slot wait and the memory wait.
   Expiry gives FAIL with the cause of the last attempt. When memory was
   the cause, the reason is
   `resource-blocked: <estimate> MiB needed, <available> MiB available`,
   from the figures of that memory decision. If memory stays unknown, the
   reason is `resource-blocked: available memory unknown`. When the slot
   was still held, the reason starts `slot-wait-expired:` and names the
   holder of each slot (pid and checkout, from the slot record), the time
   waited and the limit, with no memory figures. The wrapper exits nonzero.
   Cancellation, expiry, and an OOM-killed child never count as PASS or SKIP. The runner timeout starts after
   admission. Heavy rows wait before entering the worker pool. Light rows
   can start while heavy rows wait; the pool keeps the requested size.
   Acquire the slot before sorted checkout locks. Report slot wait in
   `slot_wait_seconds`, checkout lock wait in `lock_wait_seconds`, and
   cell build slot wait in `build_slot_wait_seconds`. Report build and
   runner memory wait in `memory_wait_seconds`. Light work includes
   `cargo fmt`, plans, listings, and harness tests. It also includes
   `contract-diff.py --cases` against built artifacts and registry runners
   that are not marked heavy. Light work runs without resource admission.
   Runner scripts that need Cargo builds start them through
   `just heavy --`.
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
- **Report results and reproduction commands**, under "Results and
  reproduction" above. Measurement-specific interpretation belongs in
  `docs/runbooks/benches.md`; raw captures are not submission requirements.
