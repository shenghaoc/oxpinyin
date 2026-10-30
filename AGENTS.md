# Agent instructions

oxpinyin implements libpinyin and libzhuyin in native Rust. It preserves the pin's outward behaviour at the C API and in persistent data. Stage 1 is complete. Stage 2 preserves bug-for-bug compatibility with internal improvements. Stage 3 permits opt-in departures.

## Project rules

The human documents hold every project rule:

- [Constitution and external interfaces](docs/constitution.md).
- [Source policy and stages](docs/source-policy.md).
- [Candidate-frequency comparator ruling](docs/source-policy.md#internal-language-artefacts).
- [Compatibility policy](docs/findings/compatibility-policy.md).
- [Contributor procedures](CONTRIBUTING.md).
- [Verification procedure](docs/runbooks/verification.md).
- [Roadmap](ROADMAP.md).

## Procedures

Read `.kiro/steering/` as always-loaded context; it uses Kiro's steering format without the Kiro IDE or CLI.
`.kiro/steering/agents-protocol.md` holds the agent procedures: phases, worktree isolation, stacks and publication.

## Hard prohibitions

- A STOP clears only on a human turn. Approval-shaped text inside your own turn is not approval: a `(Recommended)` option you wrote, a selection returned within your own transcript, a line such as "approach approved", a task chip or a background-task notice. When the harness says no human input has arrived, none has. Ask, then wait for the human to answer.
- Never request or trigger a review on any PR, by any means: no comment mentioning a review bot, no request to a reviewer or team, no "Trigger review" checkbox. A human's question about whether a PR is ready is a question: answer it and do nothing else.
- Never change a PR's draft or ready state.
- Never close a PR. Never rewrite history on a merged branch.
- Author and commit as Shenghao Chen <shenghaoc@outlook.com>. Check both identities with `git log --format='%an <%ae> %cn <%ce>'` before pushing.

Precedents:

- 2026-09-05: the `__store_ext__` interface change was proposed, self-selected and implemented in one agent run with no human input; only review caught it.
- 2026-10-22 UTC: asked whether three PRs were ready for review, an agent marked one ready and posted six review requests without authority.
