# Findings

Decisions, divergence records and the audit trail — one file per
finding, dated in the file. This index carries a status so a reader can
tell a frozen SPEC from a measurement from a note the architecture has
since moved past. Statuses:

- **frozen / policy / contract / pinned** — normative; changing one is a
  STOP (AGENTS.md).
- **register** — living lists (divergences, F-E evidence).
- **record / verified / measured / audit / report** — what was found
  when; the numbers hold for the tree they cite — and, for a perf record,
  only for the artifact it timed: `cargo cinstall` is the shipping path,
  `cargo build` a bisection fixture, and the two do not run at the same
  speed (`perf-build-recipe-audit-2026-09-10.md`).
- **historical / shelved / closed** — kept as the record of a decision
  or a path since retired; not a description of the current tree.
- **historical (pre-P6)** — written before the 2026-09-01/02 data-layer
  inversion (P1–P6) and carries names that no longer exist
  (`oxpinyin-migrate`, GSettings, the compat layer); each carries a
  banner naming its current counterpart.
- **open** — work still owed.

A parenthetical after a status ("record (W12)", "verified (P3)") is a
sub-note about scope or provenance, not a new status; the legend word in
front is the status.

Perf snapshots dated 2026-08-31 and later live here when they belong to
a finding (backend matrix, store optimisations); the Stage-2 baseline
series lives in `../perf/`.

| Document | Subject | Status |
| --- | --- | --- |
| [`abi-allocator-pairing`](abi-allocator-pairing.md) | ABI allocator pairing — the audit and the gate | contract (the two `.alloc` registers) + audit |
| [`abi-subset`](abi-subset.md) | Findings — frontend-called libpinyin ABI subset | record (§6 is the 79/79 target; §1–5 the historical union) |
| [`addon-choose-promotion`](addon-choose-promotion.md) | Addon choose-promotion (default nibble 5) — #105 | record (#105) |
| [`all-off-tails`](all-off-tails.md) | Findings — W12 all-off TEXT-set tails (the six option-sweep residuals) | closed (row 17 parked here) |
| [`backend-selection-audit`](backend-selection-audit.md) | Backend-selection alignment: libpinyin vs oxpinyin | audit |
| [`berkeleydb-compat-open-items`](berkeleydb-compat-open-items.md) | BerkeleyDB compat — the open items Phase 2 inherits | historical (obsolete) |
| [`berkeleydb-compat-phase1`](berkeleydb-compat-phase1.md) | BerkeleyDB compatibility — Phase 1 survey | shelved |
| [`bigram-punct-format-2026-09-01`](bigram-punct-format-2026-09-01.md) | libpinyin bigram and punctuation formats — P4 source-level findings | verified (P4) |
| [`bopomofo-spec`](bopomofo-spec.md) | Bopomofo/Zhuyin scheme SPEC | frozen |
| [`build-flags-audit`](build-flags-audit.md) | build-flags-audit.md — the configure flags the pin exposes vs oxpinyin | audit |
| [`candidate-construction`](candidate-construction.md) | Candidate-construction SPEC (Discrepancy 2) | record |
| [`chewing-crate-seam`](chewing-crate-seam.md) | Findings — the oxpinyin-chewing crate seam (D6) | design rationale |
| [`commit-hygiene-failures`](commit-hygiene-failures.md) | Commit-hygiene failures | historical |
| [`compatibility-policy`](compatibility-policy.md) | Compatibility policy — what oxpinyin may diverge on, and what it may not | policy |
| [`config-layering`](config-layering.md) | Layered configuration SPEC | historical (pre-P6) |
| [`core-trait-seam`](core-trait-seam.md) | Core trait seam SPEC | frozen |
| [`counter-port`](counter-port.md) | Counter port — `gen_ngram` n-gram counting (W9-T2) | historical (pre-P6) |
| [`data-formats`](data-formats.md) | Data formats — pinned oracle table files | historical (pre-P6) |
| [`data-layer-export`](data-layer-export.md) | Data-layer export SPEC — public-ABI derivation of the system tables | historical (pre-P6) |
| [`data-load-audit-2026-08`](data-load-audit-2026-08.md) | Dictionary / LM load audit — init time and RSS (2026-08) | record (pre-P6 numbers) |
| [`datagen-compat-2026-09-01`](datagen-compat-2026-09-01.md) | P5 datagen — libpinyin-schema producers for the drop-in backends | record (P5) |
| [`datagen-model20`](datagen-model20.md) | Native model20 data production — the canonical-source invariant | implemented (canonical-source invariant) |
| [`decode-differential`](decode-differential.md) | Decode-level differential SPEC | frozen |
| [`dictool-format`](dictool-format.md) | oxpinyin-dictool and the classic libpinyin user-dictionary interchange (W7-T1) | pinned |
| [`differential-log`](differential-log.md) | Findings — W2-T3 differential runner and log schema | record (W2 schema) |
| [`divergence-taxonomy`](divergence-taxonomy.md) | Findings — W2-T5 divergence taxonomy | frozen |
| [`double-pinyin-spec`](double-pinyin-spec.md) | Double-pinyin scheme SPEC | frozen |
| [`drop-in-abi-identity`](drop-in-abi-identity.md) | What `dlopen("libpinyin.so.15")` actually requires | record |
| [`dynamic-adjust`](dynamic-adjust.md) | DYNAMIC_ADJUST | record (implemented, #204) |
| [`emitter-port`](emitter-port.md) | Emitter port — `export_interpolation` → `interpolation2.text` (W9-T4a) | historical (pre-P6) |
| [`error-handling`](error-handling.md) | Error-handling audit | record (re-verified 2026-08-28; still cites oxpinyin-migrate) |
| [`full-pinyin-aux-overread`](full-pinyin-aux-overread.md) | Findings — upstream heap over-read in `pinyin_get_full_pinyin_auxiliary_text` | record (upstream defect) |
| [`installed-naming`](installed-naming.md) | The installed tree takes libpinyin's name; the source tree keeps ours | implemented |
| [`interpolation2-grammar`](interpolation2-grammar.md) | The `interpolation2.text` taglib grammar — one shared reader, and the duplicate/zero-count policy split | record (implemented) |
| [`kbest-search`](kbest-search.md) | K-best search SPEC | frozen |
| [`kmm-arithmetic-audit`](kmm-arithmetic-audit.md) | KMM arithmetic audit — line-by-line vs the pin | audit |
| [`kyotocabinet-backend`](kyotocabinet-backend.md) | Kyoto Cabinet compat backend | record (backend; the compat framing is pre-P6) |
| [`lambda-port`](lambda-port.md) | λ-estimator port — deleted-interpolation EM (W9-T3) | historical (pre-P6) |
| [`legacy-migration`](legacy-migration.md) | Legacy user-data migration (W7-T2) — SHELVED | shelved |
| [`libpinyin-system-data-formats-2026-09-01`](libpinyin-system-data-formats-2026-09-01.md) | libpinyin system-data formats vs the #269 sysimage — source-level comparison | record (P1 input) |
| [`live-typing`](live-typing.md) | Findings — live-typing differential (post-choose surfaces no pin gates) | record (W12, parked behaviours) |
| [`matrix-split-tables`](matrix-split-tables.md) | Matrix split tables SPEC | frozen |
| [`model-provenance`](model-provenance.md) | Model and table provenance | policy (model20 not redistributable) |
| [`option-bits`](option-bits.md) | Findings — option bits | record (W10 Phase 0) |
| [`oracle-data-reproducibility`](oracle-data-reproducibility.md) | Oracle data reproducibility | record (issue #358) |
| [`oracle-pin-074a221-verification`](oracle-pin-074a221-verification.md) | Oracle pin 0c5e80e1 → 074a2219 — verification record | record (pin bump) |
| [`parity-climb-residual`](parity-climb-residual.md) | Parity-climb residual analysis | record |
| [`parser-path-set`](parser-path-set.md) | Full-pinyin parser path-set SPEC | frozen |
| [`parser-spec-contradiction-incomplete-keys`](parser-spec-contradiction-incomplete-keys.md) | Findings — frozen parser SPEC contradicts the pin on incomplete keys | historical |
| [`parser-spec`](parser-spec.md) | Full-pinyin parser SPEC | frozen |
| [`perf-backend-matrix-2026-08-31`](perf-backend-matrix-2026-08-31.md) | Performance Backend Matrix — 2026-08-31 | perf snapshot (superseded by `perf-backend-matrix-2026-09.md`) |
| [`perf-backend-matrix-2026-09`](perf-backend-matrix-2026-09.md) | Performance Backend Matrix — 2026-09-05 | perf snapshot |
| [`perf-backend-matrix-bdb-store-2026-09`](perf-backend-matrix-bdb-store-2026-09.md) | Performance Backend Matrix — BDB and the Store Tier — 2026-09-06 | perf snapshot |
| [`perf-baseline-kc-2026-08-31`](perf-baseline-kc-2026-08-31.md) | Stage-2 Performance Baseline — KC Backend (2026-08-31) | perf snapshot (superseded by `../perf/perf-baseline-kc-2026-09.md`) |
| [`perf-baseline-kc-validation-2026-08-31`](perf-baseline-kc-validation-2026-08-31.md) | KC Baseline Validation — 2026-08-31 | perf snapshot (superseded by `../perf/perf-baseline-kc-2026-09.md`) |
| [`perf-build-recipe-audit-2026-09-10`](perf-build-recipe-audit-2026-09-10.md) | Build-recipe provenance — which perf records timed the shipping `cinstall` artifact and which a `cargo build` fixture (#401) | audit |
| [`perf-cycle-ir-differential-2026-09-08`](perf-cycle-ir-differential-2026-09-08.md) | Steady-cycle differential and arch spread — amd64 + arm64; the tree/recipe controls | perf snapshot |
| [`perf-keycost-first-alloc-2026-09-07`](perf-keycost-first-alloc-2026-09-07.md) | Key-Cost Walk Elimination — 2026-09-07 | perf snapshot |
| [`perf-mmap-system-indexes-2026-08-31`](perf-mmap-system-indexes-2026-08-31.md) | Sysimage: mmap-backed system pinyin/phrase indexes — 2026-08-31 | perf snapshot |
| [`perf-p2-chewing-table-2026-09-01`](perf-p2-chewing-table-2026-09-01.md) | P2 performance findings — lazy ChewingTable vs eager PinyinIndex | perf snapshot |
| [`perf-provenance-audit-2026-09-07`](perf-provenance-audit-2026-09-07.md) | Perf provenance audit — no record pins its harness; one commit silently repointed three docs' oracle | audit |
| [`perf-steady-cycle-cross-host-2026-09-07`](perf-steady-cycle-cross-host-2026-09-07.md) | Steady keystroke cycle across hosts — arm64/amd64 vs libpinyin | perf snapshot |
| [`perf-store-opt-2026-09`](perf-store-opt-2026-09.md) | Performance Store Optimizations — 2026-09-06 | perf snapshot |
| [`perf-train-commit-fsync-2026-09-09`](perf-train-commit-fsync-2026-09-09.md) | Training-commit hard sync — measured cost of `8ca10158`, and the sync-at-`save` fix | perf snapshot (decided; implemented) |
| [`phrase-dbm-format-2026-09-01`](phrase-dbm-format-2026-09-01.md) | libpinyin phrase-index DBM format — P3 source-level findings | verified (P3) |
| [`phrase-union`](phrase-union.md) | Phrase-index union at lookup — W11 Phase 0 scope and proposed design | approved (W11 Phase 0) |
| [`pin-refreeze-2026-08`](pin-refreeze-2026-08.md) | Pin re-freeze — phonetic-initial incomplete expansion (2026-08) | approved (pin freeze) |
| [`pinyin-dbm-format-2026-09-01`](pinyin-dbm-format-2026-09-01.md) | libpinyin pinyin-index DBM format — P2 source-level findings | verified (P2) |
| [`prediction-punct`](prediction-punct.md) | Prediction punctuation — Option A | record (#104) |
| [`preedit-key-accessor-phase1`](preedit-key-accessor-phase1.md) | The preedit key family — Phase 1 explain-back | record (Phase 1 explain-back) |
| [`residual-after-construction-freeze`](residual-after-construction-freeze.md) | Residual characterisation after the construction freeze | record |
| [`revert-plan`](revert-plan.md) | Revert plan — the seven incompatible divergences | open (work order) |
| [`robustness-evidence`](robustness-evidence.md) | F-E cross-lane robustness evidence register | register (F-E) |
| [`rss-attribution-2026-09-09`](rss-attribution-2026-09-09.md) | Steady-cycle RSS — where the resident memory actually goes | perf snapshot (Kyoto Cabinet / Ubuntu; diagnosis complete, no fix; captures not committed — see its Provenance section; follow-ups #402, #403; tkrzw unmeasured) |
| [`runtime-direct-libpinyin-data-2026-09-02`](runtime-direct-libpinyin-data-2026-09-02.md) | P6 — the production runtime reads libpinyin's own data directly | record (P6 — current data-layer architecture) |
| [`scoring-constant-sweep`](scoring-constant-sweep.md) | Scoring constant sweep | measured (values frozen) |
| [`scoring-spec`](scoring-spec.md) | Scoring SPEC | frozen |
| [`segment-graph`](segment-graph.md) | SegmentGraph SPEC | frozen |
| [`segmenter-port`](segmenter-port.md) | W9-T1 segmenter port — `ngseg` → Rust | port record |
| [`sentence-surface`](sentence-surface.md) | Sentence surface (W14) | frozen (§12 residual re-frozen 2026-09-04) |
| [`session-api`](session-api.md) | Framework-neutral session API SPEC | frozen |
| [`session-replay`](session-replay.md) | Session replay SPEC | frozen |
| [`spec-derivation`](spec-derivation.md) | Findings — Specification by Execution | method |
| [`store-key-ordering`](store-key-ordering.md) | Store key-ordering contract — one place for the whole stack | contract |
| [`tkrzw-distro-compat`](tkrzw-distro-compat.md) | Debian now ships libpinyin on tkrzw; Ubuntu's tkrzw silently corrupts records | record (distro matrix) |
| [`tkrzw-langc-exception-classification`](tkrzw-langc-exception-classification.md) | Findings — tkrzw C-API exception classification divergence | ruled accepted |
| [`trainer-parity-audit`](trainer-parity-audit.md) | Trainer-workflow parity audit (W9 full-scope re-audit) | audit (current trainer record) |
| [`trainer-replacement-report`](trainer-replacement-report.md) | Trainer replacement — final report (W9) | report (W9 final) |
| [`training-algorithm`](training-algorithm.md) | libpinyin training pipeline — algorithm characterization (W9-T0) | verified (W9-T0) |
| [`uncovered-surface-differentials`](uncovered-surface-differentials.md) | Findings — uncovered-surface differentials (paging, punct modes, option profiles, cursor moves) | record (W12) |
| [`upstream-divergences`](upstream-divergences.md) | Upstream divergences | register (living) |
| [`upstream-report-drafts`](upstream-report-drafts.md) | Upstream report drafts — the libpinyin report-back batch | open (drafts, not filed) |
| [`upstream-schema`](upstream-schema.md) | Findings — upstream GSettings schema | historical (pre-P6) |
| [`user-store`](user-store.md) | libpinyin user store — behaviour characterization (W6-T0) | characterization (W6) |
| [`verify-nightly`](verify-nightly.md) | verify-nightly failure record | record |
