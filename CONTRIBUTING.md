# Contributing

Read the [constitution](docs/constitution.md) and [source policy](docs/source-policy.md) before changing the project.

The usual workflow is:

1. Set up native development with the [setup guide](docs/runbooks/setup.md)
   and run its first passing test.
2. Make the change on a branch and add or extend tests for it.
3. Run the [checks for your contribution](docs/runbooks/setup.md#contribution-checks)
   and the [verification procedure](docs/runbooks/verification.md).
4. When the change affects compatibility with libpinyin, run its
   differential on the oracle, as described below.
5. Open a pull request whose description reports the results and how to
   reproduce them.

Native development means editing, compiling, debugging and testing on your
own operating system: Linux, macOS or native Windows. You need Git, the
repository-selected Rust toolchain and a native compiler and linker; the
setup guide gives complete commands for each platform, including a first
passing test and resuming in a fresh terminal. No container, libpinyin
oracle or model download is required for ordinary work.

A Rust **crate** is a package. A storage **backend** is the native DBM
library that storage-dependent crates are built against; the
[setup guide](docs/runbooks/setup.md#storage-backend) names the default and
how each platform installs it.

When a change needs **compatibility verification**, compare it with the
**oracle** (the fixed upstream libpinyin implementation). A
**differential** runs the same behaviour against both implementations.
Run it on a [provisioned remote Linux host](docs/runbooks/cloud.md) or with
the [canonical Debian compatibility procedure](docs/runbooks/compatibility-container.md),
which runs oxpinyin, the pinned reference and the comparison harness
together, so you do not install libpinyin or reconstruct its environment.
Native tests and differentials provide different evidence.
Package-manager examples and runtime adapters are not extra test targets.

Before changing code, read the [architecture](docs/architecture.md) to see
which crate owns the behaviour and what it may depend on. Read the
[Rust conventions](docs/rust-conventions.md) before writing Rust; they cover
the lints, panics, matches and arithmetic that review checks.

Advanced [backend](docs/runbooks/backends.md), [oracle](docs/runbooks/oracle.md),
[packaging](docs/packaging.md), [release](docs/runbooks/release.md) and
[measurement](docs/runbooks/benches.md) procedures apply only when that task needs them. Keep model downloads and
derived data local; they never belong in Git or published images.

A new regression identifies the behaviour it protects, uses reduced input, and explains any substantial fixture or runtime cost. Extend existing cases and helpers where that keeps the test clear. Don't commit bulk observations when a live comparison or a compact expectation suffices.

## Contributor responsibility

Understand, review, test and maintain every change you submit, including tool-assisted changes.
Keep `README.md` and `README.中文.md` consistent.
Update both in the same PR when their content changes.
A Chinese wording correction that does not change content needs no English change.
The [roadmap](ROADMAP.md) records the current stage and remaining work.

## Results and reproduction

Deliver the software, maintained tooling and a concise final result with reproducible commands.
Use repository `just` recipes and in-tree Python tools with managed CPython.
Provision the interpreter with `just python`.
Extend shared tooling when a required capability is missing.
Do not depend on scripts or environments that exist only on your machine.
Existing documented shell entry points remain supported during migration.

Report the conclusion, relevant final results and reproduction commands.
Identify only the revisions, inputs, build options and environment details needed to interpret and repeat the result.
One command can reproduce a whole table.
Link existing setup instructions instead of copying an execution transcript.
Do not report intermediate checks, selections or execution steps as a chronological narrative.
A separate findings document is optional.

Treat raw logs, profiler captures and intermediate output as disposable local working data.
Do not commit, attach, upload, archive or mirror them unless a maintainer explicitly requests specific diagnostic material.
Do not create replacement manifests, checksum inventories or evidence-unavailable statements for missing captures.
Missing captures do not block completion or merging.
Source, functional fixtures, pins, configuration files and release packages follow their own procedures.
These reporting rules concern verification output.

Investigate a broken reproduction command as a bug.
Investigate a result that contradicts the claim beyond the method's expected variability.
Fix the software, tooling, environment assumptions or documentation that caused the problem.
Correct or withdraw the claim.
Do not replace the investigation with an evidence package.
Reproduction must work without the author's old captures.
Timing results need not be bit-identical.

Run the checks required by the [verification procedure](docs/runbooks/verification.md).
Stop when they pass and concrete failures are addressed.
Keep repetitions prescribed by the measurement method.
Do not add reruns or broader sweeps merely to collect more evidence.
Report failed or unrun required checks as limitations.
Include a brief relevant diagnostic when useful.
Omit inventories of passed or inapplicable checks.
This policy supersedes older reporting requirements in runbooks and findings.

### Measurements

Run base, PR and pin in one job on one machine for a measuring PR.
Report only that run's numbers and ratios.
Do not use an absolute figure from another run, host or CPU as a baseline.
Follow the [measurement procedure](docs/runbooks/benches.md).

## Dates and shell placement

Capture freeze-document, findings and patch-metadata dates with `date -u` on the machine executing the step.
Capture them at run time.
Do not write dates by hand.
A local time zone ahead of UTC can date a record into the future (PR #363).
Follow the [Python orchestration and shell-placement rule](docs/plans/tooling-python.md#shell-placement-rule-1260).

## Toolchain and platforms

Use only the toolchain selected by `rust-toolchain.toml`.
Portable crates support Linux, macOS and Windows, on x86_64 and arm64.
Oracle and capi work is Linux-first.
Linux/amd64 is the reference for pin comparisons.
Do not use emulation for work that does not compare against the pin.

A remote Linux host that meets the [remote Linux requirements](docs/runbooks/cloud.md#host-requirements) supports every gate.
Provision it with `tools/cloud/setup.sh`.
Compatibility work can also use `tools/oracle/compat.sh`, which runs subject, pin and harness together in the [canonical Debian Linux amd64 environment](docs/runbooks/compatibility-container.md).

## Checkpoints and rebases

Push a `wip/<issue>` branch at each checkpoint.
Treat every machine as disposable: work held only in a local checkout or container can be lost.

Fetch and rebase onto the current landing tip immediately before every push and before any merge.
Check `git log origin/main..HEAD`.
It must contain only this workstream's commits, with no rewritten copies of landed work.
Inspect the diffstat for deletions of work the branch does not own.
Do not delete that work.
A stale base can make other contributors' merged work appear as deletions.

A conflict-free rebase does not verify a changed generation scheme.
A dependency can change manifest layouts, fixture formats or builder recipes while old generated artifacts survive the textual rebase.
After rebasing:

1. List committed artifacts whose generators changed on main since the branch point.
2. Regenerate those artifacts.
3. Re-run pins if the rebase changes engine, capi or data crates.

See PR #363 against #358 for the oracle-data manifest split example.
The contributor who merges later re-measures the affected pins.
Do not assume pre-rebase measurements remain valid.
Fmt failures block merging.
A reviewed, formatting-only commit is safe when its diff contains only formatting.

## Stack placement

Put a fix on the stack member whose change produces the symptom.
Keep each stack member green when tested alone.
This preserves bisection and bottom-up merging.
A fix above the failing member leaves that lower member broken until the upper one lands.
If member N exposes a pre-existing defect, put its fix on member N.
Examples include a harness leak, missing teardown or latent undefined behaviour.
Keep the tree correct at every stack level.
The capi-commands fuzz leak fix belonged on #502, which made BDB the default and exposed the failure.
Its initial placement on #512 was wrong.

## Approval conditions

Stop and ask the maintainer when any condition below applies:

- The task is ambiguous.
- The change requires a C interface, ABI or dependency change.
- A test cannot pass without breaking the constitution.
- A SPEC contradicts the pin's source.
- The pin's source and disassembly cannot settle a behaviour.
- The implementation requires an unapproved change to a frozen SPEC.
- A product-behaviour change is not a language artefact under the source policy's decision order.

Use the pin's external call when a Rust equivalent is unsettled.
Unsettled surrounding pin behaviour still requires a decision.
Rust interfaces remain internal under the [interface ruling](docs/constitution.md#external-interfaces).

Obtain approval before adding or upgrading dependencies, or changing frozen SPECs, goldens or CI policy.
Implement from frozen behavioural SPECs and fixtures.
A freeze of a Rust signature alone no longer binds.
Keep unsafe code within the [constitution's allowlist](docs/constitution.md).
Do not silence lints.
Use only the narrow, justified expectations permitted by the [panic amendment](docs/findings/compatibility-policy.md#amendment--pin-matched-panics-and-error-handling).

## Pull request descriptions

Use the [PR template](.github/PULL_REQUEST_TEMPLATE.md) for every PR, including CLI and API submissions.
Keep its headings and their order.
Write the body to a file.
Use `gh pr create --body-file <file>`, not `--fill`.

Use short, factual sentences.
Do not use emoji, badges, decorative headings or generic checklists.
Do not paste chronological logs, status updates or transcripts.
When results change, update the existing sections.
Do not append another report at the end.
Describe only a stacked PR's own diff.
Name its direct parent PR.

Keep ordinary PRs brief.
The 150–300 word range is a guide, not a minimum.
Report final results and reproduction commands under the [reporting policy](#results-and-reproduction).
Add detail only when it helps assess the change.
The [verification rules](docs/runbooks/verification.md) remain authoritative.
These description rules change only the format.

## Issue descriptions

Before creating an issue:

1. Search open and closed issues.
2. Link existing work instead of creating duplicates.
3. Choose the appropriate [issue template](.github/ISSUE_TEMPLATE/): bug report, task/proposal or tracking issue.
4. Preserve its headings and their order.
5. For CLI or API submission, write a completed body file without YAML front matter or instruction comments.
6. Submit it with `gh issue create --body-file <file>`.

If GraphQL is unavailable, use `gh api repos/shenghaoc/oxpinyin/issues -f title=<title> -F body=@<file>`.
Apply suitable existing labels, such as bug-for-bug, axis:*, severity:* or backend:*.
Do not create new labels.

Use a specific, factual title and short sentences.
Do not include emoji, badges, decorative headings, authorship fields, transcripts or chronological execution logs.
Use checkboxes only for concrete outcomes or linked work items.
Do not use them for generic assurances.

Separate observations, source-derived conclusions, hypotheses and proposals.
Identify the revision relevant to the evidence.
Use "Unknown" or "Not reproduced" when appropriate.
Never invent results, dependencies, issue numbers, approvals or acceptance thresholds.

Keep each non-tracking issue focused on one outcome.
Describe the problem before proposing a solution.
Leave implementation choices open unless an explicit requirement settles them.
Link extensive analysis instead of duplicating it.
Do not create extra issues merely to fill a template.

Keep the body current within its existing sections.
Use comments for meaningful new evidence or decisions.
Do not post repeated status dumps.
Preserve links to decisions when summarizing them.
Do not erase their history.

Creating an issue does not authorize implementation or change project policy.
Approval, compatibility and verification requirements still apply.
Issue templates change only reporting format.

## Stack merges

Merge stacks from bottom to top through the PR button.
Manage stacks with the `github/gh-stack` extension.
Install it with `gh extension install github/gh-stack`.
Use `gh stack rebase` for a cascading rebase across the stack.
Follow the [rebase procedure](#checkpoints-and-rebases) before each push and merge.
