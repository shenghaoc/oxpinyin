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
Phase 2 implements. STOP in Phase 1 when `/AGENTS.md` says so: ambiguous
task; an interface/ABI/dependency change; a test that cannot pass without
breaking the constitution; a SPEC contradicting observed pin behaviour; an
implementation that would change a frozen SPEC without an ask.

## Assisted-by trailer

Every agent-assisted commit carries exactly one trailer — the full line
`Assisted-by: AGENT:MODEL`, nothing after the model; plain human commits
need none. The linter enforces the house form when a trailer is present
(`.github/scripts/lint-commits.sh`):

    ^Assisted-by: [[:alnum:]][[:alnum:]._-]*:[[:alnum:]][[:alnum:].+_-]*$

No slash in either character class, and each token starts alphanumeric. The
model token must contain an ASCII letter (`Kiro:kiro-1` passes;
`Kiro:4.6` fails). No duplicate lines (set semantics). Never
`Co-Authored-By:` for agents. Vendors currently in use: `ZCode:GLM-5.3`,
`ZCode:GLM-5.3-Flash`, `Claude:claude-opus-4-8`, `Claude:claude-opus-5`,
and `Kiro:kiro-<id>`.

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

Nothing panics on any input, and public APIs return `Result`
(constitution §4 — the broad rule stands on its own). Separate from it,
exception class (c) in `compatibility-policy.md` is scoped to
caller-supplied input: where upstream asserts or aborts on input a caller
can supply, the C ABI returns `false`/`Err` and logs the point. Class (c)
does not mask internal invariant failures and does not govern the return
types of unrelated errors. Engine output is a pure function of (input,
user state, config), and E2E I/O is byte-identical to the pinned oracle
given the same inputs and state — except the four named exception classes
(a)–(d) in `compatibility-policy.md`.
