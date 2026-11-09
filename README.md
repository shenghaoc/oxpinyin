# oxpinyin

A portable Rust re-expression of
[libpinyin](https://github.com/libpinyin/libpinyin). The default build is
the drop-in target: it replaces `libpinyin.so.15` and `libzhuyin.so.15`.
Its target is bug-for-bug compatibility with a pinned libpinyin, defects
included. Full bug-for-bug coverage is not complete yet.

An opt-in Stage 3 build that changes only observable output keeps the ABI
and the data formats. It states its departure in its build metadata. An
opt-in Stage 3 build that changes the ABI or the native data formats does
not use libpinyin's SONAME, pkg-config file or header directory, unless a
maintainer ruling permits it.

The [source policy](docs/source-policy.md#stages) defines the stages and the [roadmap](ROADMAP.md) lists the remaining work.

**Status:** Stage 1 complete — 1,571/1,571 corpus rows; 79/79 exported
symbols; drop-in as `libpinyin.so.15` verified on Fedora rawhide, Debian
testing, and NixOS. Stage 2 (bug-for-bug) is current. Its exit gate is a
third bug-for-bug sweep (`ROADMAP.md`). Performance work that keeps
compatibility continues in Stage 2. Stage 3 (opt-in departures) follows.

(中文文档在 [这里](README.中文.md), 快看过来!)

## Platforms

The portable crates support Linux, macOS and Windows on x86_64 and arm64.
CI tests Linux x86_64 on each PR that changes code, and macOS arm64 and Windows x64 nightly and on dispatch.

## Quickstart

Start at [CONTRIBUTING.md](CONTRIBUTING.md). It walks through prerequisites,
native Linux/macOS/Windows setup, a first link-and-run test, and returning
in a fresh terminal. Ordinary development does not need an oracle, model20,
or container. For compatibility gates, use a provisioned [remote Linux host](docs/runbooks/cloud.md).

The [setup guide](docs/runbooks/setup.md#storage-backend) names the default
storage backend and gives each platform's packages and settings for it.

## Layout

| Path | What |
|---|---|
| `crates/*` | 20 workspace packages, none stubs (grouped below); crate map in `docs/architecture.md` |
| `ROADMAP.md` | Current stage, pin and open work |
| [docs/constitution.md](docs/constitution.md) | Project rules and supported external interfaces |
| [docs/source-policy.md](docs/source-policy.md) | Source-reading method, compatibility decisions and stage rules |
| `docs/runbooks/` | How to: backends, the oracle and differentials, Callgrind and Massif recipes, goldens and pins, releases |
| `docs/findings/` | Decisions, divergence records, audit trail — indexed with a status per file |
| `docs/testing/` | Corpus pipeline, oracle environment, fixture formats |
| `docs/packaging.md` | cargo-c packaging and installed-tree layout |
| `tools/oracle/` | Pin build recipe (`docs/runbooks/oracle.md`) |

See the [crate map](docs/architecture.md#crates) for package ownership.

The remaining stage binaries keep their names and arguments. Select the
consolidated Cargo package when running or testing them, for example:
`cargo run --locked -p oxpinyin-utils --bin ngseg -- --help`.

## Upstream

- Behaviour oracle: [libpinyin](https://github.com/libpinyin/libpinyin) — pin in
  `docs/testing/oracle-environment.md`; its
  [wiki](https://github.com/libpinyin/libpinyin/wiki) is the upstream
  documentation (n-gram model format, internals, multiple dictionaries)
- Frontend ABI/settings surface:
  [ibus-libpinyin](https://github.com/libpinyin/ibus-libpinyin) — its
  [wiki](https://github.com/libpinyin/ibus-libpinyin/wiki) documents the
  consumer's features and installation

## Licence

GPL-3.0-or-later.
