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

Every contributor remains responsible for understanding, reviewing, testing
and maintaining their submitted changes. [ROADMAP.md](ROADMAP.md) describes
the current compatibility stage and remaining work.
