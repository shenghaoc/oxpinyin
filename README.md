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

- Stage 1 (complete): exact-output parity with the pin on the corpus surfaces.
- Stage 2 (current): bug-for-bug compatibility across the whole exported ABI,
  with internal improvements. Performance work is allowed when compatibility
  stays. It is not complete. Open work is tracked in the open issues with the
  `bug-for-bug` label; the current tracker is #870.
- Stage 3 (next): opt-in departures from the pin, such as a new language
  model or an incompatible data format. The default build stays bug-for-bug.

**Status:** Stage 1 complete — 1,571/1,571 corpus rows; 79/79 exported
symbols; drop-in as `libpinyin.so.15` verified on Fedora rawhide, Debian
testing, and NixOS. Stage 2 (bug-for-bug) is current. The round-2 audit
closed on 2026-10-17 (#573). The open work is the init complexity
tracker (#870), then a third bug-for-bug sweep (`ROADMAP.md`). Performance
work that keeps compatibility continues in Stage 2 (`docs/perf/`). Stage 3
(opt-in departures) follows.

(中文文档在 [这里](README.中文.md), 快看过来!)

## Platforms

The portable crates support Linux, macOS and Windows on x86_64 and arm64.
CI tests Linux x86_64 and macOS arm64 on each PR that changes code, and Windows x64 nightly.

## Quickstart

Start at [CONTRIBUTING.md](CONTRIBUTING.md). It walks through prerequisites,
native Linux/macOS/Windows setup, a first link-and-run test, and returning
in a fresh terminal. Ordinary development does not need an oracle, model20,
or container. For compatibility gates, use a provisioned [remote Linux host](docs/runbooks/cloud.md)
or the [canonical Debian trixie compatibility container](docs/runbooks/compatibility-container.md),
which runs the subject, reference and harness together.

The [setup guide](docs/runbooks/setup.md#storage-backend) names the default
storage backend and gives each platform's packages and settings for it.

## Layout

| Path | What |
|---|---|
| `crates/*` | 20 workspace packages, none stubs (grouped below); crate map in `docs/architecture.md` |
| `ROADMAP.md` | Stages, pin, workstream names |
| [docs/constitution.md](docs/constitution.md) | Project rules and supported external interfaces |
| [docs/source-policy.md](docs/source-policy.md) | Source-reading method, compatibility decisions and stage rules |
| `docs/runbooks/` | How to: backends, the oracle and differentials, benches, goldens and pins, releases |
| `docs/findings/` | Decisions, divergence records, audit trail — indexed with a status per file |
| `docs/perf/` | Performance measurement snapshots |
| `docs/testing/` | Corpus pipeline, oracle environment, fixture formats |
| `docs/safety/` | MISRA-Rust mapping, audit, safety profile |
| `docs/packaging.md` | cargo-c packaging and installed-tree layout |
| `docs/oxidation-journey.md` | How libpinyin has been turned into Rust so far, and the method that follows from it |
| `tools/oracle/` | Pin build recipe (`docs/runbooks/oracle.md`) |
| [docs/specs/](docs/specs/) | Requirements, designs and task records for foundation, drop-in compatibility, oxidation and Stage 3 |

| Group | Crates |
|---|---|
| Engine / data / runtime / bindings | `oxpinyin-core`, `oxpinyin-chewing`, `oxpinyin-store`, `oxpinyin-data`, `oxpinyin-user`, `oxpinyin-engine`, `oxpinyin-facade`, `oxpinyin-runtime`, `oxpinyin-capi`, `oxpinyin-zhuyin-capi`, `oxpinyin-capi-marshal` |
| Training toolchain (never ships) | `oxpinyin-training` (modules: `segment`, `counter`, `lambda`, `emitter`, `eval`, `train`; existing stage binary names remain), `oxpinyin-corpus`, `oxpinyin-kmm`, `oxpinyin-word`, `oxpinyin-punct`, `oxpinyin-datagen` |
| Oracle / testing | `pinyin-oracle`, `oxpinyin-testsupport` |

Training commands keep their existing executable names and arguments. Select
the consolidated Cargo package when running or testing them, for example:
`cargo run --locked -p oxpinyin-training --bin oxpinyin-segment -- --help`.

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
