---
inclusion: always
---

# Agent protocol

Follow `/AGENTS.md` (wins on conflict). Tiers: `[A]` architect · `[B]` basic ·
`[H]` human — never hand `[A]`/`[H]` work to a basic agent.

Implement from frozen SPECs/fixtures only.

## Phases

Explain back before writing code. Phase 1 states the plan — files,
interfaces, verification — with no code, and stops for confirmation.
For Stage 2 work the plan also names the pin's source lines (or the
disassembled symbol and offsets) each behaviour was read from; Phase 1
reads code, it does not run the oracle to find out what it does.
Phase 2 implements. STOP in Phase 1 when `/AGENTS.md` says so: ambiguous
task; an interface/ABI/dependency change; a test that cannot pass without
breaking the constitution; a SPEC contradicting the pin's source; a
behaviour the pin's source and disassembly cannot settle; an
implementation that would change a frozen SPEC without an ask.

## Assisted-by trailer

Every agent-assisted commit carries an `Assisted-by: AGENT:MODEL` trailer
for each distinct agent and model, nothing after the model; plain human
commits need none. The linter enforces the house form when a trailer is
present (`.github/scripts/lint-commits.sh`):

    ^Assisted-by: [[:alnum:]][[:alnum:]._-]*:[[:alnum:]][[:alnum:].+_-]*$

No slash in either character class, and each token starts alphanumeric. The
model token must contain an ASCII letter (`Kiro:kiro-1` passes;
`Kiro:4.6` fails). No duplicate lines (set semantics). The regex is the
gate; there is no vendor allowlist — `git log --format=%B | grep Assisted-by`
shows the forms in use (`ZCode:GLM-5.3`, `Claude:claude-fable-5-1`,
`Kiro:kiro-cli`, …).

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
(`pinned-behaviour.md`), except the classes (a)–(c) and the registered
standing divergences in `oxpinyin-improvements.md`. Nothing panics on
any input, and public APIs return `Result` (constitution §4 — the broad
rule stands on its own). Class (c) is narrower: where the pin asserts or
aborts on input a caller or file can supply, the C ABI returns
`false`/`Err` and logs the point; it does not mask internal invariant
failures and does not govern the return types of unrelated errors.
Engine output is a pure function of (input, user state, config).
