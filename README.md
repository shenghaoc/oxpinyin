# oxpinyin

A portable Rust re-expression of
[libpinyin](https://github.com/libpinyin/libpinyin): Stage 1 is exact-output
parity with a checksum-pinned, source-built libpinyin oracle; Stage 2 is
measured algorithm upgrades.

**Status:** Stage 1 complete — 1,571/1,571 corpus rows (ORDER-ONLY divergence
class); 79/79 exported symbols; drop-in as `libpinyin.so.15` verified on
Fedora rawhide, Debian testing, and NixOS. Stage 2 (binary model compilation,
init/RAM reduction) in progress.

## Quickstart

```sh
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
# Debian/Ubuntu: apt-get install libkyotocabinet-dev libclang-dev libglib2.0-dev pkg-config
cargo test --locked --workspace
cargo test --locked --workspace --no-default-features --features redb  # portable fallback
```

## Layout

| Path | What |
|---|---|
| `crates/*` | 18 crates, none stubs (grouped below) |
| `ROADMAP.md` | Stages, pin, workstream names |
| `AGENTS.md` | Agent/collaborator contract |
| `docs/findings/` | Decisions, divergence records, audit trail |
| `docs/perf/` | Performance measurement snapshots (Stage 2 baseline) |
| `docs/testing/` | Corpus pipeline, oracle environment, fixture formats |
| `docs/safety/` | MISRA-Rust mapping, audit, safety profile |
| `docs/packaging.md` | cargo-c packaging and installed-tree layout |
| `docs/python.md` | Python binding data requirements and API |
| `tools/oracle/` | Pin build recipe |
| `.kiro/` | Steering, specs (foundation/drop-in/python-binding), agent configs |

| Group | Crates |
|---|---|
| Engine / data / runtime / bindings | `oxpinyin-core`, `oxpinyin-chewing`, `oxpinyin-store`, `oxpinyin-data`, `oxpinyin-user`, `oxpinyin-engine`, `oxpinyin-runtime`, `oxpinyin-capi`, `oxpinyin-python` |
| Training toolchain | `oxpinyin-segment`, `oxpinyin-counter`, `oxpinyin-lambda`, `oxpinyin-emitter`, `oxpinyin-corpus`, `oxpinyin-datagen` |
| Tools | `oxpinyin-dictool` |
| Oracle / testing | `pinyin-oracle`, `oxpinyin-testsupport` |

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
  `docs/testing/oracle-environment.md`
- Frontend ABI/settings surface:
  [ibus-libpinyin](https://github.com/libpinyin/ibus-libpinyin)

## Licence

GPL-3.0-or-later.
