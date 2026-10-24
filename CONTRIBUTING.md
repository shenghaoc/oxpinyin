# Contributing

Start with **native development**: edit, compile, debug and test on your own
operating system. You need Git, the repository-selected Rust toolchain and
a native compiler/linker; the [setup guide](docs/runbooks/setup.md) gives
complete commands for Linux, macOS and native Windows, including a first
passing test and fresh-terminal resumption. No container, libpinyin oracle,
or model download is required for ordinary work.

A Rust **crate** is a package. A storage **backend** is the native library
selected when building storage-dependent packages; Berkeley DB is the
default, while the maintained macOS route selects tkrzw explicitly. Packages
that use that library are called storage-dependent.

When a change needs **compatibility verification**, compare it with the
**oracle** (the fixed upstream libpinyin implementation) using the
[canonical Debian compatibility procedure](docs/runbooks/compatibility-container.md).
It runs oxpinyin, the pinned reference and the comparison harness (the test driver) together;
you do not install libpinyin or reconstruct its environment on your host.
A **differential** runs the same behavior against both implementations.
Native tests and Debian-container differentials provide different evidence.
Linux, macOS and Windows are the three native targets. Agent-platform cloud
sessions are also first-class supported hosts for every gate: use the
[cloud provisioner](docs/runbooks/cloud.md). The canonical Debian container
is a second supported route using the same frozen reference contracts.
Package-manager examples and runtime adapters are not extra test targets.

After the first test, follow [checks for your contribution](docs/runbooks/setup.md#contribution-checks).
Advanced [backend](docs/runbooks/backends.md), [oracle](docs/runbooks/oracle.md),
[packaging](docs/packaging.md) and [measurement](docs/runbooks/benches.md)
procedures apply only when that task needs them. Keep model downloads and
derived data local; they never belong in Git or published images.

## Results and reproduction

A PR or findings document reports what a paper reports: the results, plus
the tools that produced them. Readers reproduce those results with the
supplied tools and commands; they do not need a narrative of intermediate
checks, selections or execution steps.

The deliverable is the software and maintained tooling that lets another
contributor reproduce its results from a checkout. Use the repository's
`just` recipes and in-tree Python tools with the managed interpreter from
the setup guide. Extend shared tooling when a required capability is missing;
do not depend on a session-only script or an agent's private environment.
Existing documented shell entry points remain supported during migration.

Report the conclusion, relevant final results, and the commands to reproduce
them. Identify the revisions, inputs, build options and environment needed
to interpret and repeat the comparison. One command can cover a whole table;
link existing setup instructions instead of copying an execution transcript.
A short PR needs no separate findings document.

Raw logs, profiler captures and intermediate output are disposable local
working data. Do not commit, attach, upload, archive or mirror them as routine
evidence, or create manifests merely to certify a run. Supply diagnostic files
only when a maintainer explicitly requests them. Do not ask a maintainer to
sign in to an agent browser or another service to upload verification output.
Missing or discarded captures are not a completion or merge blocker and need
no replacement disclaimer. Source, test fixtures, pin/configuration files and
release packages remain normal project inputs or deliverables.

If the documented procedure cannot be run, or its results contradict a claim
beyond the measurement method's expected variability, investigate it as a bug
in the software, tooling, environment assumptions or documentation. Fix the
cause and correct or withdraw the affected claim. Reproduction must work
without the author's old captures; timing results need not be bit-identical.

Run the checks applicable to the change and stop when they pass and concrete
failures are addressed. Keep repetitions required by the measurement method;
do not add reruns or broader sweeps merely to accumulate evidence. Report
required checks that failed or could not run as limitations, with a brief
relevant diagnostic when useful. Omit inventories of passed or inapplicable
checks. This reporting policy supersedes older capture-retention and
publication instructions.

Every contributor remains responsible for understanding, reviewing, testing
and maintaining their submitted changes. [ROADMAP.md](ROADMAP.md) describes
the current compatibility stage and remaining work.
