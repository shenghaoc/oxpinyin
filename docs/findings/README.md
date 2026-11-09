# Findings

Decisions, registers, frozen SPECs and the records that kept documents and tools still read: one file per finding, dated in the file. New results go in pull request bodies ([results and reproduction](../../CONTRIBUTING.md#results-and-reproduction)). Git history holds every retired record.

Statuses:

- **frozen / policy / contract / template** — normative; changing one is a STOP (../../CONTRIBUTING.md).
- **register / living reference** — living lists and references.
- **policy record / discrepancy record / evidence** — an accepted difference or the evidence behind a rule. A policy record states the pin's and oxpinyin's results and their source derivation; the tests that hold oxpinyin's side name it.
- **machine-read** — a program or test reads or writes this file.
- **current reference / record / verified / measured / audit / report / implemented / approved / characterization / design rationale / historical** — a dated record; it describes the tree within the scope the file states.
- **open** — work still owed.

A parenthetical after a status is a note about scope or provenance, not a new status.

| Document | Subject | Status |
| --- | --- | --- |
| [`1297-candidate-numeric-sort`](1297-candidate-numeric-sort.md) | #1297 item A — stable sorting with guarded wrapping frequency | discrepancy record (accepted) |
| [`1297-gram-partition-segmentation`](1297-gram-partition-segmentation.md) | 1297-gram-partition-segmentation — phrase segmentation over stored grams | policy record |
| [`1297-gram-partition-training`](1297-gram-partition-training.md) | 1297-gram-partition-training — training over stored grams | policy record |
| [`1297-layout-criticals`](1297-layout-criticals.md) | Phrase-library layout faults write no critical | policy record |
| [`1297-punct-words`](1297-punct-words.md) | #1297 — Punctuation values contain whole words | policy record |
| [`1297-segment-critical`](1297-segment-critical.md) | Phrase segmentation writes no invalid-UTF-8 critical | policy record |
| [`1378-zhuyin-selection`](1378-zhuyin-selection.md) | #1378 — zhuyin selection order | policy record |
| [`1501-gint16-input-cap`](1501-gint16-input-cap.md) | 1501-gint16-input-cap — the full-pinyin parse's 16-bit steps | policy record |
| [`1501-guint16-key-positions`](1501-guint16-key-positions.md) | 1501-guint16-key-positions — key positions past byte 65535 | policy record |
| [`918-boundary-follow-through`](918-boundary-follow-through.md) | #918 — remaining input, caller and store partitions | current reference |
| [`abi-allocator-pairing`](abi-allocator-pairing.md) | ABI allocator pairing — the audit and the gate | contract (the two `.alloc` registers) + audit |
| [`abi-reference`](abi-reference.md) | Reference — the exported libpinyin ABI (all 79 `pinyin_*` symbols; signatures, state model, config, error behaviour) | living reference |
| [`addon-choose-promotion`](addon-choose-promotion.md) | Addon choose-promotion (default nibble 5) — #105 | record (#105) |
| [`algorithm-precondition-witnesses-2026-10-28`](algorithm-precondition-witnesses-2026-10-28.md) | Reachable algorithm-precondition witnesses | evidence (policy registered sites) |
| [`arm64-pin-portability-1137`](arm64-pin-portability-1137.md) | Pin-built libpinyin on arm64: five non-crash contract cases, two out-of-range float-to-`guint32` stores (#1137) | policy record |
| [`axis-j-bdb-ledger`](axis-j-bdb-ledger.md) | Axis J: Berkeley DB internal coverage ledger | machine-read |
| [`axis-j-kc-ledger`](axis-j-kc-ledger.md) | Axis J: Kyoto Cabinet internal coverage delta ledger | machine-read |
| [`axis-j-tkrzw-ledger`](axis-j-tkrzw-ledger.md) | Axis J: Tkrzw internal coverage delta ledger | machine-read |
| [`bdb-access-patterns-2026-10-22`](bdb-access-patterns-2026-10-22.md) | Berkeley DB adapter access patterns | current reference |
| [`bdb-failed-handle`](bdb-failed-handle.md) | bdb-failed-handle — Berkeley DB handles whose open failed | policy record |
| [`berkeleydb-backend`](berkeleydb-backend.md) | BerkeleyDB backend — drop-in task 10 | current reference |
| [`bigram-punct-format-2026-09-01`](bigram-punct-format-2026-09-01.md) | libpinyin bigram and punctuation formats — P4 source-level findings | verified (P4) |
| [`bopomofo-spec`](bopomofo-spec.md) | Bopomofo/Zhuyin scheme SPEC | frozen |
| [`candidate-construction`](candidate-construction.md) | Candidate-construction SPEC (Discrepancy 2) | frozen (§8); §1-7 record |
| [`chewing-crate-seam`](chewing-crate-seam.md) | Findings — the oxpinyin-chewing crate seam (D6) | design rationale |
| [`compatibility-policy`](compatibility-policy.md) | Compatibility policy — what oxpinyin may diverge on, and what it may not | policy |
| [`core-trait-seam`](core-trait-seam.md) | Core trait seam SPEC | behaviour frozen; Rust-signature freeze retired 2026-10-18 |
| [`data-formats`](data-formats.md) | Data formats — pinned oracle table files | current reference (§2-3); §1 historical |
| [`decode-differential`](decode-differential.md) | Decode-level differential SPEC | frozen |
| [`double-pinyin-spec`](double-pinyin-spec.md) | Double-pinyin scheme SPEC | frozen |
| [`drop-in-abi-identity`](drop-in-abi-identity.md) | What `dlopen("libpinyin.so.15")` actually requires | record |
| [`dynamic-adjust`](dynamic-adjust.md) | DYNAMIC_ADJUST | record (implemented, #204) |
| [`eio-flexible-bigram-malformed-records`](eio-flexible-bigram-malformed-records.md) | FlexibleBigram malformed records (class (b), 2026-11-01 UTC) | discrepancy record (accepted) |
| [`eio-flexible-bigram-unsorted-records`](eio-flexible-bigram-unsorted-records.md) | FlexibleBigram item search and the unsorted KC fixture | discrepancy record (accepted) |
| [`eio-kmm-to-interpolation-undefined-lines`](eio-kmm-to-interpolation-undefined-lines.md) | k_mixture_model_to_interpolation undefined lines — normal refusal | discrepancy record (accepted) |
| [`eio-kmm-tools`](eio-kmm-tools.md) | KMM tool asserts and their policy records | discrepancy record (accepted) |
| [`eio-pinyin-index-padding`](eio-pinyin-index-padding.md) | eio-pinyin-index-padding — the pin's uninitialised padding in pinyin-index items | discrepancy record (accepted) |
| [`eio-table-generator-invalid-utf8`](eio-table-generator-invalid-utf8.md) | Table-generator invalid UTF-8 — normal refusal | discrepancy record (accepted) |
| [`eio-test-flexible-attach-empty`](eio-test-flexible-attach-empty.md) | FlexibleBigram test-program attach and magic headers | current reference |
| [`eio-tool-check-messages`](eio-tool-check-messages.md) | Utility check macros write no message | discrepancy record (accepted) |
| [`eio-utf8-next-char-invalid-lead`](eio-utf8-next-char-invalid-lead.md) | g_utf8_next_char from a byte that leads no sequence — one-byte step | discrepancy record (accepted) |
| [`eio-zhuyin-missing-frequency`](eio-zhuyin-missing-frequency.md) | gen_zhuyin_table missing frequency — normal refusal | discrepancy record (accepted) |
| [`external-io-boundary`](external-io-boundary.md) | External I/O boundary | policy (maintainer ruling; stage 5 in progress) |
| [`glib-libc-call-sites`](glib-libc-call-sites.tsv) | GLib and libc call sites at the pin, generated by `tools/audit/glib_call_sites.py` | machine-read |
| [`glib-libc-inventory`](glib-libc-inventory.md) | GLib and libc at the pin: reachable calls and oxpinyin counterparts | current reference |
| [`glib-libc-verification`](glib-libc-verification.md) | GLib/libc verification coverage | current reference |
| [`installed-naming`](installed-naming.md) | The installed tree takes libpinyin's name; the source tree keeps ours | implemented |
| [`interpolation2-grammar`](interpolation2-grammar.md) | The `interpolation2.text` taglib grammar — one shared reader, and the duplicate/zero-count policy split | record (implemented) |
| [`kbest-search`](kbest-search.md) | K-best search SPEC | frozen |
| [`kyotocabinet-backend`](kyotocabinet-backend.md) | Kyoto Cabinet compat backend | record (backend; the compat framing is pre-P6) |
| [`language-runtime-discrepancy-template`](language-runtime-discrepancy-template.md) | Language/runtime discrepancy record — template | template |
| [`libpinyin-structures-and-algorithms`](libpinyin-structures-and-algorithms.md) | libpinyin's components, data structures, algorithms and coefficients beside oxpinyin's, with the open gaps | register |
| [`libpinyin-system-data-formats-2026-09-01`](libpinyin-system-data-formats-2026-09-01.md) | libpinyin system-data formats vs the #269 sysimage — source-level comparison | record (P1 input) |
| [`library-transitions-in-memory`](library-transitions-in-memory.md) | Library transitions in memory | current reference |
| [`macos-shipped-exports`](macos-shipped-exports.md) | Staticlib relink for exact shipped Mach-O exports, transitive dependency negative control and nightly cost | verified |
| [`matrix-split-tables`](matrix-split-tables.md) | Matrix split tables SPEC | frozen |
| [`model-provenance`](model-provenance.md) | Model and table provenance | policy (model20 not redistributable) |
| [`option-bits`](option-bits.md) | Findings — option bits | record (W10 Phase 0) |
| [`oracle-arm64-1132`](oracle-arm64-1132.md) | Pin-built oracle on native arm64 Linux (#1132) | current reference |
| [`oracle-data-reproducibility`](oracle-data-reproducibility.md) | Oracle data reproducibility | record (issue #358) |
| [`parser-path-set`](parser-path-set.md) | Full-pinyin parser path-set SPEC | frozen |
| [`parser-spec`](parser-spec.md) | Full-pinyin parser SPEC | frozen |
| [`phrase-dbm-format-2026-09-01`](phrase-dbm-format-2026-09-01.md) | libpinyin phrase-index DBM format — P3 source-level findings | verified (P3) |
| [`phrase-union`](phrase-union.md) | Phrase-index union at lookup — W11 Phase 0 scope and proposed design | approved (W11 Phase 0) |
| [`pin-abort-assert-line`](pin-abort-assert-line.md) | Reproduced library aborts write no assertion line | discrepancy record (accepted) |
| [`pin-aborts-journal-replay`](pin-aborts-journal-replay.md) | Pin aborts reproduced in the journal replay | current reference |
| [`pin-aborts-pinyin-facade`](pin-aborts-pinyin-facade.md) | Pin aborts reproduced in the pinyin C facade | current reference |
| [`pin-aborts-remaining`](pin-aborts-remaining.md) | Pin aborts reproduced: the short user-bigram total and the directory library | current reference |
| [`pin-aborts-zhuyin-facade`](pin-aborts-zhuyin-facade.md) | Pin aborts reproduced in the zhuyin C facade | current reference |
| [`pinyin-dbm-format-2026-09-01`](pinyin-dbm-format-2026-09-01.md) | libpinyin pinyin-index DBM format — P2 source-level findings | verified (P2) |
| [`probe-coverage-abi`](probe-coverage-abi.md) | Findings — probe coverage over the full exported ABI (79 symbols, four unclassified residues) | current reference (live matrix; residues closed) |
| [`returned-string-allocation`](returned-string-allocation.md) | Returned-string allocation (#1227) | current reference |
| [`robustness-evidence`](robustness-evidence.md) | F-E cross-lane robustness evidence register | register (F-E) |
| [`scoring-spec`](scoring-spec.md) | Scoring SPEC | frozen |
| [`segment-graph`](segment-graph.md) | SegmentGraph SPEC | frozen |
| [`sentence-surface`](sentence-surface.md) | Sentence surface (W14) | machine-read (frozen) |
| [`session-api`](session-api.md) | Framework-neutral session API SPEC | behaviour frozen; Rust-signature freeze retired 2026-10-18 |
| [`session-replay`](session-replay.md) | Session replay SPEC | frozen |
| [`store-key-ordering`](store-key-ordering.md) | Store key-ordering contract — one place for the whole stack | contract |
| [`tkrzw-distro-compat`](tkrzw-distro-compat.md) | Debian now ships libpinyin on tkrzw; Ubuntu's tkrzw silently corrupts records | record (distro matrix; amended 2026-10-05 — tkrzw 1.0.34 fixes the sentinel defect upstream; what a defect-1 library does to the backend before and after #621) |
| [`trainer-parity-audit`](trainer-parity-audit.md) | Trainer-workflow parity audit (W9 full-scope re-audit) | audit (current trainer record) |
| [`training-algorithm`](training-algorithm.md) | libpinyin training pipeline — algorithm characterization (W9-T0) | verified (W9-T0) |
| [`trellis-adjusted-cost-s1-s2`](trellis-adjusted-cost-s1-s2.md) | Pinyin trellis adjusted-cost comparison | policy record |
| [`upstream-divergences`](upstream-divergences.md) | Upstream divergences | register (living) |
| [`upstream-report-drafts`](upstream-report-drafts.md) | Upstream report drafts — the libpinyin report-back batch | open (drafts, not filed) |
| [`upstream-schema`](upstream-schema.md) | Findings — upstream GSettings schema | historical (pre-P6) |
| [`user-store`](user-store.md) | libpinyin user store — behaviour characterization (W6-T0) | characterization (W6) |
