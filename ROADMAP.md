# Roadmap

Portable Rust re-expression of
[libpinyin](https://github.com/libpinyin/libpinyin). Constitution and agent
rules: `AGENTS.md`. Crate roles: `.kiro/steering/structure.md`.

> **Project rename:** pinyin-rs → **oxpinyin** (repo, crate, docs).
> Shipped artifact naming for the compatibility bootstrap is a separate
> concern from project identity.

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
| W7 | Classic text-format interop via pinyin-dictool (import + export) | pinyin-dictool, pinyin-capi |
| W8 | oxpinyin library release + compatibility bootstrap for the ibus-libpinyin fork | pinyin-capi |
| W9 | Training toolchain | pinyin-segment, pinyin-counter, pinyin-lambda, pinyin-emitter, pinyin-corpus |

### Workstream notes (recorded as decisions settle)

- **W7 is flat, not a task stack.** One deliverable: classic text-format
  interop via pinyin-dictool (import + export). The line-oriented
  `phrase (SP|TAB) pinyin [count]` format has been libpinyin's public
  interchange since 1.1.0; ibus-libpinyin's Import/Export buttons drive
  `LibPinyinBackEnd::importPinyinDictionary` /
  `exportPinyinDictionary` (`PYLibPinyin.cc:230-277`, `:280-353`).
  Historically neither libpinyin nor ibus-libpinyin migrated user data
  from their predecessors (pinyin engine, novel-pinyin, ibus-pinyin) —
  the pattern is fresh start plus the text-format interchange for users
  who care. Binary legacy-DB migration was investigated
  (`feat/w7-t2-legacy-migrate`, shelved with findings at
  `docs/findings/legacy-migration.md`) and cancelled per that precedent.
  GSettings key-for-key mapping was cancelled too: a Rust-language rewrite
  would have its own component/schema ids per the W8 decision. No
  T-numbering here — one deliverable delivered in one PR, flat like the
  decoder-λ fix (PR #55).

- **W8 is the oxpinyin library release, with a compatibility bootstrap for
  the maintainer's ibus-libpinyin fork.** pinyin-capi already exposes the
  ~50-symbol subset the pinned ibus-libpinyin 1.16.5 calls. The first
  oxpinyin release ships this as a binary the fork links against with
  minimal changes — enough to switch the fork off the C++ libpinyin backend
  and onto oxpinyin.

  After that first release, the surface is **free to evolve**. The 50-symbol
  subset is a bootstrap contract for the initial switch, not a permanent ABI
  freeze. Long-term soname or header compatibility with upstream libpinyin
  is explicitly a non-goal: the fork and oxpinyin evolve together, and
  upstream compatibility is not maintained.

  Two precedents, cited for what they inform:

  - **libchewing** (Kan-Ru Chen): a library-only rewrite; frontend packages
    were left alone. oxpinyin follows this pattern.
  - **pinyin → libpinyin** (Peng Huang → Peng Wu): historically a new library
    name with a new frontend, no drop-in. oxpinyin's bootstrap is a
    transitional inversion of that — the first release IS a working swap for
    the fork — but the long-term shape returns to the pinyin → libpinyin
    pattern: own library, own frontend fork, no upstream compatibility
    promise.

  Acceptance for the initial release: the maintainer's ibus-libpinyin fork
  builds against oxpinyin's compatibility surface with minimal changes, and
  the resulting engine produces the same wire-level output on a scripted
  input sequence as the pinned upstream configuration.

  Earlier language in this repo variously described W8 as
  "capi + forked frontend", then "ibus-pinyin-rs zbus rewrite", then
  "drop-in libpinyin.so.15" — all superseded by the above. The maintainer
  being independent of Red Hat / Fedora / upstream libpinyin is what enables
  this scope; a maintainer bound to those distros would be forced into the
  drop-in shape.

- **W9 merged out of numeric order.** W9 is the training toolchain and
  shipped five stages: segmenter (`ngseg`), counter (`gen_ngram`), λ
  estimator (`gen_deleted_ngram` + `estimate_interpolation`), emitter
  (`interpolation2.text` via `export_interpolation`), and the corpus
  front-end (zhwiki cleaner). Deliberate scope cut: the KMM path is
  skipped; `interpolation2.text` is the shipped format.
