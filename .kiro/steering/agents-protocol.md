---
inclusion: always
---

# Agent protocol

Project rules are in the human documents, starting with the [contributor procedures](../../CONTRIBUTING.md).
This file holds the procedures an agent follows.

## Tiers

The tiers are `[A]` architect, `[B]` basic and `[H]` human.
Never assign `[A]` or `[H]` work to a basic agent.

## Phases

Explain the plan before writing code.
In Phase 1, name the files, interfaces and verification.
Write no code in Phase 1.
Stop for confirmation before Phase 2 implementation.
For Stage 2, name the pin source lines or disassembled symbols and offsets that establish each behaviour.
Follow the [source-reading procedure](../../docs/source-policy.md#stage-2).
Apply the human [approval conditions](../../CONTRIBUTING.md#approval-conditions).

## Worktrees

Do your work in a separate worktree.
More than one agent may hold a checkout; never switch another workstream's shared checkout or change its working tree.
Check `git branch --show-current` first: a branch you did not create indicates another workstream.

1. Base root work on `origin/main`.
2. Base a dependent stack member on its current parent.
3. Create a new branch with `git worktree add -b <branch> /tmp/<name> <base>`.
4. Use `git worktree add /tmp/<name> <branch>` for an existing branch.
5. Use `git worktree add --detach /tmp/<name> <commit>` for detached work.
6. Commit in your worktree.
7. Remove your worktree with `git worktree remove` when finished.
8. Leave the shared checkout exactly as found.

Never touch or prune another workstream's worktree.
Precedent: two agents assuming sole ownership of one tree caused the shim.cc collision.

## Stacks

Revise a stack member in place; each member keeps its own scope and stays green alone.
Put each fix on the member whose change produces it, under the human [stack placement](../../CONTRIBUTING.md#stack-placement) rule.
Restack with `gh stack rebase`.
Follow the human [rebase](../../CONTRIBUTING.md#checkpoints-and-rebases) and [stack merge](../../CONTRIBUTING.md#stack-merges) procedures.

## Publication

Push a `wip/<issue>` branch at each checkpoint.
Write PR and issue bodies to a file under the human [PR](../../CONTRIBUTING.md#pull-request-descriptions) and [issue](../../CONTRIBUTING.md#issue-descriptions) rules.
Update an existing body in place; for a stacked PR, name its direct parent PR by number.
Report results under the human [results and reproduction](../../CONTRIBUTING.md#results-and-reproduction) rules.

## Output contract

Read the [constitution](../../docs/constitution.md), [source policy](../../docs/source-policy.md) and [compatibility policy](../../docs/findings/compatibility-policy.md).
