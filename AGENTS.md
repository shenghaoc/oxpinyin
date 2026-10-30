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

Follow [contributor responsibility](CONTRIBUTING.md#contributor-responsibility).

## Results and reproduction

Follow the [reporting policy](CONTRIBUTING.md#results-and-reproduction).

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

Follow the [date and shell-placement rules](CONTRIBUTING.md#dates-and-shell-placement).

## Toolchain

Follow the [toolchain and platform rules](CONTRIBUTING.md#toolchain-and-platforms).

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

Follow the [measurement rules](CONTRIBUTING.md#measurements) and [checkpoint workflow](CONTRIBUTING.md#checkpoints-and-rebases).

## Rebase discipline

Follow the [rebase procedure](CONTRIBUTING.md#checkpoints-and-rebases).

## Stack placement

Follow the [stack-placement rules](CONTRIBUTING.md#stack-placement).

## Verification cells

Follow the [verification procedure](docs/runbooks/verification.md).

## Operational rules that live with their procedures

Follow the [procedure-specific rules](docs/runbooks/verification.md#procedure-specific-rules).
