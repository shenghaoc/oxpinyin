# pinyin-rs

A portable Rust re-expression of
[libpinyin](https://github.com/libpinyin/libpinyin): Stage 1 is exact-output
parity with a checksum-pinned, source-built libpinyin oracle; Stage 2 is
measured algorithm upgrades.

The author acknowledges limited knowledge of Rust; this project is
a proof of concept.

**Status: pre-alpha — scaffolding only. Not yet usable.**

## Quickstart

```sh
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
```

## Layout

| Path | What |
|---|---|
| `crates/*` | Eight-crate workspace (stubs today) |
| `ROADMAP.md` | Stages, pin, workstream names |
| `AGENTS.md` | Agent/collaborator contract |
| `docs/findings/` | Pin, ABI, schema, SPEC inputs |
| `tools/oracle/` | Pin build recipe |
| `.kiro/` | Steering + foundation task specs |

## Upstream

- Behaviour oracle: [libpinyin](https://github.com/libpinyin/libpinyin) — pin in
  `docs/findings/oracle-environment.md`
- Frontend ABI/settings surface:
  [ibus-libpinyin](https://github.com/libpinyin/ibus-libpinyin)

## Licence

GPL-3.0-or-later.
