# oxpinyin

A portable Rust re-expression of
[libpinyin](https://github.com/libpinyin/libpinyin): Stage 1 is exact-output
parity with a checksum-pinned, source-built libpinyin oracle; Stage 2 is
measured algorithm upgrades.

**Status:** Stage 1 implementation complete — 1,571/1,571 corpus rows
(ORDER-ONLY divergence class); 79/79 exported symbols; drop-in as
`libpinyin.so.15` verified on Fedora rawhide, Debian testing, and NixOS.
One verification gap open — the differential suite does not yet drive the
full 79-symbol exported ABI; the one defect the policy carried (row 30, the
pinyin facade's chewing batch `FORCE_TONE` seam) closed in code 2026-09-14
and verified live 2026-09-16 — all eight implemented keyboards IDENTICAL
(`docs/findings/compatibility-policy.md`
row 30; `ROADMAP.md`, Stage 1 status). Stage 2 (binary model compilation,
init/RAM reduction) in progress.

## Quickstart

```sh
# Debian/Ubuntu: apt-get install libtkrzw-dev liblzma-dev liblz4-dev libzstd-dev zlib1g-dev libclang-dev libglib2.0-dev pkg-config
# macOS (Homebrew): brew install tkrzw glib pkgconf; libclang ships with the Xcode Command Line Tools.
#   export LIBRARY_PATH="$(brew --prefix)/lib"  # tkrzw links lz4/lzma/zstd from there, but the store's build
#   script emits only tkrzw's own -L dir: without this `cargo test` fails at link time ("library 'lz4' not
#   found") while `cargo check`/`clippy`, which never link, still pass.
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
```

## Layout

| Path | What |
|---|---|
| `crates/*` | 26 crates, none stubs (grouped below); crate map in `.kiro/steering/structure.md` |
| `ROADMAP.md` | Stages, pin, workstream names |
| `AGENTS.md` | Agent/collaborator contract |
| `docs/runbooks/` | How to: backends, the oracle and differentials, benches, goldens and pins, releases |
| `docs/findings/` | Decisions, divergence records, audit trail — indexed with a status per file |
| `docs/perf/` | Performance measurement snapshots (Stage 2 baseline) |
| `docs/testing/` | Corpus pipeline, oracle environment, fixture formats |
| `docs/safety/` | MISRA-Rust mapping, audit, safety profile |
| `docs/packaging.md` | cargo-c packaging and installed-tree layout |
| `tools/oracle/` | Pin build recipe (`docs/runbooks/oracle.md`) |
| `.kiro/` | Steering and specs (foundation/drop-in) in Kiro's documented layout (<https://kiro.dev/docs/>); the format is used, the Kiro IDE and CLI are not |

| Group | Crates |
|---|---|
| Engine / data / runtime / bindings | `oxpinyin-core`, `oxpinyin-chewing`, `oxpinyin-store`, `oxpinyin-data`, `oxpinyin-user`, `oxpinyin-engine`, `oxpinyin-facade`, `oxpinyin-runtime`, `oxpinyin-capi`, `oxpinyin-zhuyin-capi`, `oxpinyin-capi-marshal` |
| Training toolchain (never ships) | `oxpinyin-corpus`, `oxpinyin-segment`, `oxpinyin-kmm`, `oxpinyin-lambda`, `oxpinyin-word`, `oxpinyin-punct`, `oxpinyin-eval`, `oxpinyin-train`, `oxpinyin-datagen`; legacy interpolation utilities the trainer never invokes but the others share: `oxpinyin-counter`, `oxpinyin-emitter` |
| Tools | `oxpinyin-dictool` |
| Oracle / testing | `pinyin-oracle`, `oxpinyin-testsupport` |

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
