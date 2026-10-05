# AGENTS.md — contract for all coding agents

oxpinyin is a portable Rust re-expression of libpinyin. Stage 1 = parity
with the pin-built oracle; Stage 2 = measured upgrades. Roadmap:
`ROADMAP.md`. Crate map: `.kiro/steering/structure.md`.

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

Method: copy as much as possible from upstream, rewrite it in Rust with a
loosely coupled project structure, then oxidize further. Internal structure
is free to diverge, subject to two constraints: external interface behavior
must be unchanged, and time and space complexity must never both be worsened
— a regression in one is acceptable only when traded against a gain in the
other, must be minimized, and must be justified in the change's report.
Stage 2 targets a smaller binary, faster execution, and much lower RAM than
libpinyin; internal freedom exists to serve that, not to erode it.

Rust-mechanism divergences: where upstream behavior cannot be reproduced
because of a language-mechanism difference, record it in
docs/findings/upstream-divergences.md, move on, and do not chase it. These
notes are collected to report back to libpinyin once the rewrite is complete.

**Before recording one, read `docs/findings/compatibility-policy.md`.** The
goal is a drop-in replacement: rename the built object to `libpinyin.so.15`,
put it on the library path, and unmodified consumers work against the data
already on the system — and user data when the KV database backend is
unchanged: same-backend pairs (Kyoto Cabinet↔Kyoto Cabinet,
tkrzw↔tkrzw) interoperate seamlessly in both directions. Data loss when
the KV database backend actually changes is taken for granted
(maintainer ruling 2026-09-09; the value-level import/export
interchange is then the migration path — the policy's goal amendment).
Do not re-raise either as an open trade-off. Reproducing the pin is
therefore the default, and
divergence is an exception that must be argued into one of exactly three
classes — (a) math, (b) memory safety, (c) availability. (Class (d),
consumer scope, was retired 2026-09-06; the policy doc is authoritative
on the current set.)
Anything outside those three is a defect to be reverted, not a divergence to
be recorded. The policy carries the classes, their citations, and a
classification of every existing register entry.

## Attribution

Trailer form: `Assisted-by: <AgentName>:<model-id>`, where `<AgentName>` is the
tool or harness you are running in, not the model's vendor (e.g. `Codex:qwen3.8-max`).
Nothing after the model.

Every agent that authored or helped write a commit adds its own Assisted-by
line, once per agent and model. A different agent, or the same agent running
a different model, gets a separate line; an identical line is never repeated.
This is the only trailer this repository asks for. Leave all other trailers
exactly as your tool produces them by default, and do not add, remove, or
change any of them because of this file.

The linter checks the shape of these lines on every non-merge PR commit (R2)
and at commit time via `.githooks/commit-msg` (R2):

- **R2** — `Assisted-by:` house form: `AGENT:MODEL` shape with nothing after
  the model; the `MODEL` token must contain at least one ASCII letter (a bare
  version number names no model — `Grok:4.6` fails, `Grok:grok-4.6` passes);
  no placeholder text; no duplicate lines (set semantics).

## STOP → do not improvise

Ambiguous task · needs interface/ABI/dep change · test cannot pass without
breaking the constitution · SPEC contradicts observed pin behaviour ·
implementation would require changing a frozen SPEC without an ask.

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
   - a changed file contains `feature = "bdb"`, `feature = "tkrzw"` or
     `feature = "kyotocabinet"`
     (`git diff --name-only origin/main...HEAD | xargs grep -lE
     'feature *= *"(bdb|tkrzw|kyotocabinet)"'`);
   - the diff changes a line naming a backend in any Cargo.toml
     (`git diff -U0 origin/main...HEAD -- '*Cargo.toml' | grep -E
     '^[+-][^+-].*(bdb|tkrzw|kyotocabinet)'`).
   State the check's output in the PR body. A result you believe is
   backend-dependent but that fails the check is a finding to report,
   not a licence to run three cells.
3. Every differential runs on all three cells once at the start of each
   sweep and once before a release. That run, not the per-change gate,
   is what covers kc and tkrzw.
4. Revert-and-check runs on the reference cell only.
5. The local test gate is `cargo test -p <crate>` for each crate the
   diff touches, plus fmt and clippy on those crates. The workspace
   sweep is CI's job; do not run it locally as a gate.
6. No new bespoke driver plus runner per issue where an existing driver
   can take a case file. Add cases to the existing driver; a new driver
   needs a line in the PR body saying why none could.
7. Re-verification is proportional to what changed since the last
   green gate:
   - Docs, comments or findings only: fmt, then push. No builds.
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
