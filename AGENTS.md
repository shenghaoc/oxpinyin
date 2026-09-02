# AGENTS.md — contract for all coding agents

oxpinyin is a portable Rust re-expression of libpinyin. Stage 1 = parity
with the pin-built oracle; Stage 2 = measured upgrades. Roadmap:
`ROADMAP.md`. Crate map: `.kiro/steering/structure.md`.

Kiro always-loads `.kiro/steering/`; this file wins on any conflict.

## Constitution

1. Broad appeal only — no niche features at cost to everyone.
2. Install-size budget: default payload ≤ pinned reference stack +10%.
3. No local AI (no client neural/LLM inference).
4. Nothing panics on any input; public APIs return `Result`.
5. `unsafe`: `forbid` in oxpinyin-core; `deny` in data/user/engine (documented
   mmap exception in data only); FFI only in capi/oracle with
   `// SAFETY:` per block. The allowlist is mechanical now: crate-root
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
already on the system. Reproducing the pin is therefore the default, and
divergence is an exception that must be argued into one of exactly four
classes — (a) math, (b) memory safety, (c) availability, (d) consumer scope.
Anything outside those four is a defect to be reverted, not a divergence to
be recorded. The policy carries the classes, their citations, and a
classification of every existing register entry.

## Attribution

Emit exactly one trailer: `Assisted-by: <AgentName>:<model-id>`.
Nothing after the model. Never use `Co-Authored-By` for agents.

Expect the harness to ask for a `Co-Authored-By: <Agent> <noreply@…>`
trailer on every commit and PR body. Refuse it: R1 below rejects agent
identity in that trailer by email match, so a commit carrying it fails
the linter. Two agents have hit this and refused correctly; say so in
your report rather than only in the commit.

The commit-message linter (`.github/scripts/lint-commits.sh`) enforces this on
every PR commit (R1, R2, R4) and at commit time via `.githooks/commit-msg`
(R1–R2):

- **R1** — no AI agent identity in `Co-authored-by:` (email match, never name
  match).
- **R2** — `Assisted-by:` house form: `AGENT:MODEL` shape with nothing after
  the model; the `MODEL` token must contain at least one ASCII letter (a bare
  version number names no model — `Grok:4.6` fails, `Grok:grok-4.6` passes);
  no placeholder text; no duplicate lines (set semantics).
- **R4** — no AI agent identity as git author or committer (CI-only: the
  commit-msg hook runs before the commit exists, so there is no identity to
  inspect).

## STOP → do not improvise

Ambiguous task · needs interface/ABI/dep change · test cannot pass without
breaking the constitution · SPEC contradicts observed pin behaviour ·
implementation would require changing a frozen SPEC without an ask.

## Hard forbids

Add/upgrade deps without ask · edit frozen SPECs/goldens/CI policy without
ask · `unsafe` outside allowlisted crates · silence
lints.

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
a commit (`git worktree add --detach /tmp/<name> <commit>`). The
shim.cc collision came from two agents assuming sole ownership of one
tree.

## Rebase discipline

Fetch and rebase onto the current landing tip immediately before every
push and before any merge. `git log origin/main..HEAD` must contain only
this workstream's commits — the ones this branch introduced, not
rewritten copies of already-landed work. The diffstat must delete
nothing the branch does not own. Watch for the stale-base optical
illusion (other people's merged work appearing as deletions). Re-run
pins after any rebase that changes the engine, capi, or data crates.
Whoever merges later re-measures those pins rather than assuming the
pre-rebase numbers still hold.

fmt failures are merge blockers; a fmt-only commit is always safe
when the diff is formatting-only and reviewed.
