# Roadmap

Portable Rust re-expression of
[libpinyin](https://github.com/libpinyin/libpinyin). Constitution and agent
rules: `AGENTS.md`. Crate roles: `.kiro/steering/structure.md`.

## Stages

| Stage | Goal |
|---|---|
| **0** | Scaffold, pin, SPECs/fixtures (here) |
| **1** | Exact-output parity with the pin-built libpinyin oracle (differential testing) |
| **2** | Measured upgrades (trigram/KN, typo edges, own data) — every divergence vs Stage 1 baseline |

Stage 1 uses installed/libpinyin-format tables (no redistribution required).
Stage 2 is optional and measurement-gated.

## Reference pin

Authoritative freeze: `docs/findings/oracle-environment.md`  
(libpinyin `2.11.91` / ibus-libpinyin `1.16.5` / model archive SHA-256s).

Build: `tools/oracle/build-oracle.sh` (optional container recipe alongside).

## How work proceeds

1. **Freeze behaviour** into `docs/findings/` SPECs and golden fixtures
   (characterisation of the pin first; see `docs/findings/spec-derivation.md`).
2. **Implement** only from frozen SPECs/fixtures — not by reading upstream C++.
3. **Prove** with fixture tests everywhere; live oracle diff on Linux is the
   verification tier for Stage 1 gates.

Detailed task cards live under `.kiro/specs/` as they are derived. Until a
SPEC is frozen, do not implement that slice.

## Phase 0 (blocks feature work)

Still open or partial — see `.kiro/specs/foundation/tasks.md` and findings:

| Need | Output |
|---|---|
| Pin + recipe | `docs/findings/oracle-environment.md` (recorded) |
| Frontend ABI subset | `docs/findings/abi-subset.md` (recorded) |
| Upstream schema | `docs/findings/upstream-schema.md` (recorded) |
| Parser / path-set / scoring SPECs | not yet frozen |
| Data load route (D3) | not yet decided |
| Capture harness + F-A fixtures | not yet built |

## Stage 1 workstreams (names only)

| ID | Focus | Crate(s) |
|---|---|---|
| W1 | Types, parser, correction flags | pinyin-core |
| W2 | Oracle FFI + differential runner | pinyin-oracle |
| W3 | Table loading | pinyin-data |
| W4 | SegmentGraph, k-best, engine session | pinyin-core, pinyin-engine |
| W5 | C ABI subset | pinyin-capi |
| W6 | User store (redb) | pinyin-user |
| W7 | Vocab export / migrate | pinyin-dictool, pinyin-migrate |
| W8 | Frontend path + soak | capi + forked frontend (later) |
