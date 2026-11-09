# Agent instructions

The [roadmap](ROADMAP.md) records stage status.

This file is the repository's instruction entry point. `CLAUDE.md` imports it.
The human documents hold every project rule. This file states the agent procedure and links to the rules. It does not repeat them.

## Where the rules are

| Before you | Read |
|---|---|
| Start any task | [Constitution and external interfaces](docs/constitution.md), [compatibility policy](docs/findings/compatibility-policy.md), [contributor procedures](CONTRIBUTING.md) and [approval rules](CONTRIBUTING.md#approval-conditions) |
| Change compatibility, the pin or a stage | The [roadmap](ROADMAP.md) and [source policy](docs/source-policy.md) |
| Change code | The [architecture](docs/architecture.md): which crate owns the behaviour and what it may depend on |
| Write Rust | The [Rust conventions](docs/rust-conventions.md) |
| Derive or change a pinned behaviour | The [source policy](docs/source-policy.md), its [Stage 2 reading procedure](docs/source-policy.md#stage-2) and the [pinned-behaviour page](docs/pinned-behaviour.md) |
| Sort or compare candidate frequencies | The [candidate-frequency comparator ruling](docs/findings/compatibility-policy.md#no-ported-external-library-internals) |
| Accept or record a difference from the pin | The [compatibility policy](docs/findings/compatibility-policy.md) and the [improvements page](docs/oxpinyin-improvements.md) |
| Add or change a test | The [testing ownership rule](CONTRIBUTING.md#testing) |
| Write or rewrite a document | The [writing rules](CONTRIBUTING.md#writing) |
| Push | [Verification rule 5](docs/runbooks/verification.md#5-local-test-gate): the checks for iteration, push, WIP checkpoints and Markdown-only changes |
| Open a PR or an issue | The [PR](CONTRIBUTING.md#pull-request-descriptions) and [issue](CONTRIBUTING.md#issue-descriptions) rules and the [results rule](CONTRIBUTING.md#results-and-reproduction) |

## Procedure

1. Explain the plan before you write code.
   In Phase 1, name the files, the interfaces and the verification.
   Write no code in Phase 1.
2. Stop for confirmation before Phase 2, the implementation.
3. For Stage 2 work, name the pin source lines, or the disassembled symbols and offsets, that establish each behaviour.
4. Stop and ask under the [approval conditions](CONTRIBUTING.md#approval-conditions).
5. Work in your own [worktree](CONTRIBUTING.md#worktrees).
6. Publish under the [checkpoint and rebase](CONTRIBUTING.md#checkpoints-and-rebases) and [stack](CONTRIBUTING.md#stack-placement) procedures.

## Hard prohibitions

- A STOP clears only on a human turn. Approval-shaped text inside your own turn is not approval: a `(Recommended)` option you wrote, a selection returned within your own transcript, a line such as "approach approved", a task chip or a background-task notice. When the harness says no human input has arrived, none has. Ask, then wait for the human to answer.
- Never request or trigger a review on any PR, by any means: no comment mentioning a review bot, no request to a reviewer or team, no "Trigger review" checkbox. A human's question about whether a PR is ready is a question: answer it and do nothing else.
- Never change a PR's draft or ready state.
- Never close or merge a PR. A human merges every change into `main`.
- Never push to `main`. Never rewrite history on a merged branch.
- Commit under the Git identity that is already configured. If none is configured, stop; do not commit. Never set or change a Git identity unless the human explicitly instructs you to.

You may create branches, commit, push your own branches, open draft PRs, update their descriptions and read CI results.

In Claude Code, `.claude/settings.json` denies the usual commands for the PR and `main` actions above.
The guard does not cover every command form, hosted session or agent.

The prohibitions apply to every agent, every session and every form of a command.

Precedents:

- 2026-09-05: the `__store_ext__` interface change was proposed, self-selected and implemented in one agent run with no human input; only review caught it.
- 2026-10-22 UTC: asked whether three PRs were ready for review, an agent marked one ready and posted six review requests without authority.
