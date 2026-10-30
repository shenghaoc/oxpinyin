# AGENTS.md — contract for all coding agents

oxpinyin is a native Rust implementation of libpinyin and libzhuyin, not
an emulator of the pin; it reproduces the pin's outward behaviour, at the
C API and in persistent data, in idiomatic Rust. Stage 1 = parity
with the pin-built oracle (complete); Stage 2 = bug-for-bug compatibility
with the pin, with internal improvements (current); Stage 3 = opt-in
departures from the pin. Roadmap: `ROADMAP.md`.
Crate map: `.kiro/steering/structure.md`.

`.kiro/steering/` follows Kiro's steering format (<https://kiro.dev/docs/>)
without the Kiro IDE or CLI.
Read it as always-loaded context.
Human documents define the project rules linked below.
This file takes precedence over steering for agent-only rules.

## Constitution

Follow the [constitution](docs/constitution.md).

## Source policy

Follow the [source policy](docs/source-policy.md).
Follow the [candidate-frequency comparator ruling](docs/source-policy.md#internal-language-artefacts).

## External interfaces

Follow the [external-interface ruling](docs/constitution.md#external-interfaces).

## Stages

Follow the [stage rules](docs/source-policy.md#stages).
See the [roadmap](ROADMAP.md) for progress.

## Pinned behaviour and oxpinyin improvements

Use the [two-part classification](docs/source-policy.md#pinned-behaviour-and-oxpinyin-improvements).

## Contributor responsibility

Every contributor remains responsible for understanding, reviewing, testing,
and maintaining the changes they submit, including changes produced with
assistance from any tool.

`README.md` and `README.中文.md` state the same content. A PR that changes
the content of either updates the other in the same PR. A correction to
Chinese wording alone that does not change content needs no English change.

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
require changing a frozen SPEC without an ask · a change to product
behaviour that is not a language artefact (limb 4 of the quirk decision
order).

An unsettled Rust equivalent to an external call is resolved by using
the pin's call; unsettled surrounding pin behaviour remains a STOP.

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

Shell placement: follow [the Python orchestration rule and audit](docs/plans/tooling-python.md#shell-placement-rule-1260).

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
the subject, pinned reference and harness together in the canonical Debian Linux amd64 compatibility environment
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
(`tools/dev/cell.py`). These rules decide which cells a
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
   Function-order policies are per backend, so regenerating or checking them always covers bdb, kc and tkrzw, independent of `cells-for-diff.py`; see [docs/packaging.md, "Regeneration"](docs/packaging.md#regeneration).
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
   neighbours, named by case ID (`just runner contract-diff
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
   memory admission check (maintainer ruling 2026-10-19 UTC). The native
   slot is not validated on Windows. Run direct cargo build, test and
   clippy commands through `just heavy`. Cell builds and runners that
   `tools/bisection/runners.json` marks `"heavy": true` are heavy work and
   go through the slot too.
   The bisect, contract-diff, open-counter-diff, and transformed-options-sweep
   runners are heavy at every selection size. Direct `cargo build`,
   `cargo test`, and `cargo clippy` commands are heavy because they compile.
   Full or family-wide differentials, mutation runs, Valgrind, Massif,
   release builds, and LTO builds are also heavy. `run-all.py` applies
   both checks to cell builds and heavy runners. Start other heavy work
   with `just heavy -- <command>`.
   Light work includes `cargo fmt`, plans, listings, and harness tests. It
   also includes `contract-diff.py --cases` against built artifacts and
   registry runners that are not marked heavy. Light work runs without
   resource admission. Runner scripts that need Cargo builds start them
   through `just heavy --`.
   Leave `CARGO_BUILD_JOBS`, `RUST_TEST_THREADS` and `--jobs` unset unless
   a measurement needs fixed values.
   Cancellation, expiry, and an OOM-killed child never count as PASS or SKIP.
   The mechanics (slot inheritance, signals, settings, memory readers,
   estimates, waits and refusal reasons) are in `docs/runbooks/setup.md`,
   "Heavy-work admission".
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

- **Independent campaign subprocesses/builds** use bounded pools under heavy admission; isolation exceptions and the audit live in `docs/plans/tooling-python.md`, "Independent campaign work".
- **Managed Python** is stated once, in `.python-version`, and provisioned
  through `just python`; never restate the version or pass the file's `+gil`
  request to `uv python install`: `docs/runbooks/setup.md`.
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
- **Shell scripts require Bash 5.0 or newer; ShellCheck 0.11.0 is their lint
  floor**: `docs/runbooks/setup.md`.
