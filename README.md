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

## Quickstart

```sh
# Debian/Ubuntu: apt-get install libdb-dev libclang-dev libglib2.0-dev pkg-config
# The default build links the system Berkeley DB (the analogue of a bare libpinyin ./configure);
# add --no-default-features --features tkrzw for the peer the docs' macOS path uses:
# macOS (Homebrew): brew install tkrzw glib pkgconf; libclang ships with the Xcode Command Line Tools.
#   export LIBRARY_PATH="$(brew --prefix)/lib"  # tkrzw links lz4/lzma/zstd from there, but the store's build
#   script emits only tkrzw's own -L dir: without this `cargo test` fails at link time ("library 'lz4' not
#   found") while `cargo check`/`clippy`, which never link, still pass. For the default (BDB) macOS build
#   use the `berkeley-db@5` keg and the OXPINYIN_BDB_* overrides in docs/runbooks/backends.md.
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
```

## Layout

| Path | What |
|---|---|
| `crates/*` | 20 workspace packages, none stubs (grouped below); crate map in `.kiro/steering/structure.md` |
| `ROADMAP.md` | Stages, pin, workstream names |
| `AGENTS.md` | Agent/collaborator contract |
| `docs/runbooks/` | How to: backends, the oracle and differentials, benches, goldens and pins, releases |
| `docs/findings/` | Decisions, divergence records, audit trail — indexed with a status per file |
| `docs/perf/` | Performance measurement snapshots |
| `docs/testing/` | Corpus pipeline, oracle environment, fixture formats |
| `docs/safety/` | MISRA-Rust mapping, audit, safety profile |
| `docs/packaging.md` | cargo-c packaging and installed-tree layout |
| `tools/oracle/` | Pin build recipe (`docs/runbooks/oracle.md`) |
| `.kiro/` | Steering and specs in Kiro's documented layout (<https://kiro.dev/docs/>), split into pinned libpinyin behaviour (`steering/pinned-behaviour.md`; spec `drop-in`; spec `bug-for-bug` Requirements 1–3) and oxpinyin's improvements (`steering/oxpinyin-improvements.md`; spec `bug-for-bug` Requirements 4–5; spec `stage3-departures`); the format is used, the Kiro IDE and CLI are not |

| Group | Crates |
|---|---|
| Engine / data / runtime / bindings | `oxpinyin-core`, `oxpinyin-chewing`, `oxpinyin-store`, `oxpinyin-data`, `oxpinyin-user`, `oxpinyin-engine`, `oxpinyin-facade`, `oxpinyin-runtime`, `oxpinyin-capi`, `oxpinyin-zhuyin-capi`, `oxpinyin-capi-marshal` |
| Training toolchain (never ships) | `oxpinyin-training` (modules: `segment`, `counter`, `lambda`, `emitter`, `eval`, `train`; existing stage binary names remain), `oxpinyin-corpus`, `oxpinyin-kmm`, `oxpinyin-word`, `oxpinyin-punct`, `oxpinyin-datagen` |
| Tools | `oxpinyin-dictool` |
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
