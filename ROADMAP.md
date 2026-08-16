# Roadmap

Portable Rust re-expression of
[libpinyin](https://github.com/libpinyin/libpinyin). Constitution and agent
rules: `AGENTS.md`. Crate roles: `.kiro/steering/structure.md`.

> **Project identity:** **oxpinyin** (repo, crate, docs). The prior
> project name is retained in git history only.
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
2. **Implement** under the Source policy in `AGENTS.md` (copy upstream,
   rewrite in Rust, then oxidize), with frozen SPECs/fixtures as the gate.
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
| W1 | Types, parser, correction flags | oxpinyin-core |
| W2 | Oracle FFI + differential runner | pinyin-oracle |
| W3 | Table loading | oxpinyin-data |
| W4 | SegmentGraph, k-best, engine session | oxpinyin-core, oxpinyin-engine |
| W5 | C ABI subset | oxpinyin-capi |
| W6 | User store (redb) | oxpinyin-user |
| W7 | Classic text-format interop via oxpinyin-dictool (import + export) | oxpinyin-dictool, oxpinyin-capi |
| W8 | oxpinyin library release + compatibility bootstrap for the ibus-libpinyin fork | oxpinyin-capi |
| W9 | Training toolchain | oxpinyin-segment, oxpinyin-counter, oxpinyin-lambda, oxpinyin-emitter, oxpinyin-corpus |
| W10 | Option bits: correction, fuzzy/ambiguity, dynamic-adjust gating | oxpinyin-core, oxpinyin-engine |
| W11 | Phrase-index union at lookup (user, network, addon) | oxpinyin-engine, oxpinyin-data, oxpinyin-user |
| W12 | Corpus tail (undiagnosed parity gaps) | oxpinyin-core, oxpinyin-engine, oxpinyin-capi |
| W13 | Double-pinyin and bopomofo input schemes (feature implementation) | oxpinyin-core, oxpinyin-engine |

### Workstream notes (recorded as decisions settle)

- **W7 is flat, not a task stack.** One deliverable: classic text-format
  interop via oxpinyin-dictool (import + export). The line-oriented
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
  the maintainer's ibus-libpinyin fork.** oxpinyin-capi exposes the
  51-symbol call surface of the fork (`feat/oxpinyin-backend` Phase-0 doc
  `docs/oxpinyin-switch.md` in that ibus-libpinyin repo, tip `0d71866`):
  the 50 symbols pinned from ibus-libpinyin 1.16.5 plus
  `pinyin_get_parsed_input_length` (fork commit `2c5baa9`). For W8, the
  fork surface supersedes the upstream tag freeze.
  The first oxpinyin release ships this as a binary the fork links against
  with minimal changes — enough to switch the fork off the C++ libpinyin
  backend and onto oxpinyin.

  After that first release, the surface is **free to evolve**. The
  51-symbol fork call surface is a bootstrap contract for the initial
  switch, not a permanent ABI freeze. Long-term soname or header
  compatibility with upstream libpinyin is explicitly a non-goal: the fork
  and oxpinyin evolve together, and upstream compatibility is not
  maintained.

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

  W8 closes the bootstrap milestone, not Stage 1: the fork switched and
  running against oxpinyin, wire-level parity on the defined bootstrap
  surface, cargo-c packaging, and a compatibility + performance report
  that establishes the Stage-2 measurement baseline. Stage 1 parity
  continues through W10–W12 and closes when the parity bar is met.
  Measuring Stage-2 baselines while Stage-1 work continues is deliberate —
  those numbers are prerequisites for improving against them. Remaining
  work is not W8 — it is W10–W13 below.

- **W9 merged out of numeric order.** W9 is the training toolchain and
  shipped five stages: segmenter (`ngseg`), counter (`gen_ngram`), λ
  estimator (`gen_deleted_ngram` + `estimate_interpolation`), emitter
  (`interpolation2.text` via `export_interpolation`), and the corpus
  front-end (zhwiki cleaner). Deliberate scope cut: the KMM path is
  skipped; `interpolation2.text` is the shipped format.

- **W10–W12 are three parity workstreams, not one.** They have different shapes —
  bounded/mechanical (W10), architectural (W11), open-ended (W12) — and
  bundling them would make completion hostage to the least predictable
  member. Same reasoning that flattened W7.

- **W10 is option bits.** Correction (`PINYIN_CORRECT_*`), fuzzy/ambiguity
  (`PINYIN_AMB_*`), and `DYNAMIC_ADJUST` gating. Correct-pinyin is on by
  default in the fork's gschema, making this the one true blocker for
  default-settings parity; fuzzy is default-off but shares the same
  parser-table machinery. Verification has two shapes: the correction and
  fuzzy bits are parser-table bits, swept against the pinned oracle via the
  parse differential; `DYNAMIC_ADJUST` gates training behavior and is
  verified by `run-train-diff.sh`. Both are mechanically checkable and
  bounded in scope.

- **W11 is the phrase-index union at lookup.** User, network, and addon
  phrases don't currently surface in candidates (the W8 parity gate had to
  empty `network.txt`). Upstream's `FacadePhraseIndex` unions up to 16
  libraries by token nibble; oxpinyin's decode reads a single system index.
  This is the gap a user notices first — user-dictionary phrases (added via
  the add-phrase iterators or dictool import) never surface as candidates
  at all — and it carries real architectural risk: scope it before
  estimating it. Trained counts already rank existing candidates through
  W6's additive merge; what is missing is the user-dictionary surface, not
  ranking. Landing it also un-no-ops `pinyin_load_addon_phrase_library`,
  and owns the prediction/suggestion gap —
  `pinyin_guess_predicted_candidates_with_punctuations` and
  `pinyin_choose_predicted_candidate` — which the gap inventories list as
  no-ops with no owning workstream.

- **W12 is the corpus tail.** After the #85 re-freeze: 13 of 10,190 inputs
  differ at top-1, ~4,059 prefix-10 positions beyond tie-order. Could be one
  systematic cause or thirteen separate ones — undiagnosed. This workstream
  is open-ended by nature, unlike W10/W11: its completion criterion is
  diagnosis-driven rather than feature-driven. Also parked here:
  the live-typing behaviors the parity sequence doesn't yet exercise (deep
  paging, mid-composition edits, punctuation modes).

- **W13 is double-pinyin and bopomofo input schemes.** Previously parked in
  W12, these are new input schemes (feature implementation), not corpus-tail
  diagnosis; an open-ended workstream would make their completion criterion
  meaningless. Completion is feature-driven: each scheme is implemented and
  verified against the pinned oracle on a scheme-specific differential
  surface, with its own frozen scheme SPEC rather than W12's open-ended
  diagnosis criterion.
