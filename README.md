# oxpinyin

A portable Rust re-expression of
[libpinyin](https://github.com/libpinyin/libpinyin): Stage 1 is exact-output
parity with a checksum-pinned, source-built libpinyin oracle; Stage 2 is
measured algorithm upgrades.

The author acknowledges limited knowledge of Rust; this project is
a proof of concept.

**Status: pre-alpha.** Stage 1 workstreams through W7 are merged (W9 too);
not a library release yet.

## Quickstart

```sh
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
```

## Layout

| Path | What |
|---|---|
| `crates/*` | 15 crates, none stubs (grouped below) |
| `ROADMAP.md` | Stages, pin, workstream names |
| `AGENTS.md` | Agent/collaborator contract |
| `docs/findings/` | Pin, ABI, schema, SPEC inputs |
| `tools/oracle/` | Pin build recipe |
| `.kiro/` | Steering + foundation task specs |

| Group | Crates |
|---|---|
| Engine / data / runtime / bindings | `oxpinyin-core`, `oxpinyin-store`, `oxpinyin-data`, `oxpinyin-user`, `oxpinyin-engine`, `oxpinyin-runtime`, `oxpinyin-capi`, `oxpinyin-python` |
| Training toolchain | `oxpinyin-segment`, `oxpinyin-counter`, `oxpinyin-lambda`, `oxpinyin-emitter`, `oxpinyin-corpus` |
| Tools | `oxpinyin-dictool` |
| Oracle | `pinyin-oracle` |

## Python

The engine is consumable from Python with no libpinyin install — the same
Rust implementation the C frontends use. It serves the use case described in
[libpinyin issue #181](https://github.com/libpinyin/libpinyin/issues/181) —
call a pinyin engine from Python and get Chinese candidates back — without
requiring libpinyin.

```python
import oxpinyin

with oxpinyin.Engine.from_fixture_dir("fixtures/w3") as engine:
    for candidate in engine.lookup("nihao"):
        print(candidate.text)   # 你好 first
```

Build with maturin (`pip install .` inside `crates/oxpinyin-python`) and see
[docs/python.md](docs/python.md) for data requirements, selection/learning
workflows, thread-safety and error mapping.

## Upstream

- Behaviour oracle: [libpinyin](https://github.com/libpinyin/libpinyin) — pin in
  `docs/findings/oracle-environment.md`
- Frontend ABI/settings surface:
  [ibus-libpinyin](https://github.com/libpinyin/ibus-libpinyin)

## Licence

GPL-3.0-or-later.
