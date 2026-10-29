---
inclusion: always
---

# Agent protocol

Follow `/AGENTS.md` (wins on conflict). Tiers: `[A]` architect · `[B]` basic ·
`[H]` human — never hand `[A]`/`[H]` work to a basic agent.

Implement from frozen SPECs/fixtures only. A SPEC that freezes behaviour binds;
one that only froze a Rust signature does not (`/AGENTS.md`, "External
interfaces").

## Phases

Explain back before writing code. Phase 1 states the plan — files,
interfaces, verification — with no code, and stops for confirmation.
For Stage 2 work the plan also names the pin's source lines (or the
disassembled symbol and offsets) each behaviour was read from; Phase 1
reads code, it does not run the oracle to find out what it does.
Phase 2 implements. STOP in Phase 1 when `/AGENTS.md` says so: ambiguous
task; a C interface/ABI or dependency change (Rust interfaces are free —
`/AGENTS.md`, "External interfaces"); a test that cannot pass without
breaking the constitution; a SPEC contradicting the pin's source; a
behaviour the pin's source and disassembly cannot settle; an
implementation that would change a frozen SPEC without an ask.

## Worktrees

The shared checkout is never yours: check `git branch --show-current` first
— a branch you did not create means another session is mid-flight. Work in
a separate worktree (`git worktree add -b <branch> /tmp/<name> <base>`),
commit there, `git worktree remove` when done. Root work bases on
`origin/main`; a dependent stack layer bases on its current stack parent.
Never touch or prune another session's worktree.

## Merges

Stacks merge bottom-up through the PR button. Manage stacks with the
`github/gh-stack` extension (`gh extension install github/gh-stack`);
`gh stack rebase` restacks — a cascading rebase across the stack. Never
close a PR. Never rewrite history on a merged branch. Fetch and rebase
onto the landing tip before every push and before any merge
(`/AGENTS.md` rebase discipline).

## Output contract

E2E I/O is what the pin produces given the same inputs and state
(`pinned-behaviour.md`), except the classes (a)–(c) and the recorded
internal language artefacts in `oxpinyin-improvements.md`. External I/O
uses the pin's call unless source proves a Rust equivalent for every
input; internal language artefacts are recorded instead. The C ABI,
ownership, the on-disk formats and data ownership never change. Panics and
error handling follow constitution §4: a Rust panic is permitted only where
the pin asserts or aborts on the corresponding condition, unless an existing
compatibility ruling requires a different outcome. Where the pin continues
or returns an error, Rust must not introduce a panic. Internal Rust APIs
are not universally required to return `Result`. Existing class (b)/(c)
rulings remain authoritative. Class (c) refusals still return `false`/`Err`
and emit exactly one warning per occurrence; this policy reverses none of
them. Permitted lint expectations follow `rust-conventions.md`; the lint
only sees the construct and does not prove correspondence with the pin.
Engine output is a pure function of (input, user state, config) on each host.

See `docs/findings/compatibility-policy.md`, "Native Rust implementation" (#1303).
