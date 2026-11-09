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
| [`abi-allocator-pairing`](abi-allocator-pairing.md) | ABI allocator pairing — the register and the gate | contract (the two `.alloc` registers) |
| [`abi-reference`](abi-reference.md) | Reference — the exported libpinyin ABI (all 79 `pinyin_*` symbols; signatures, state model, config, error behaviour) | living reference |
| [`addon-choose-promotion`](addon-choose-promotion.md) | Addon choose-promotion (default nibble 5) — #105 | record (#105) |
| [`algorithm-precondition-witnesses-2026-10-28`](algorithm-precondition-witnesses-2026-10-28.md) | Reachable algorithm-precondition witnesses | evidence (policy registered sites) |
| [`arm64-pin-portability-1137`](arm64-pin-portability-1137.md) | Pin-built libpinyin on arm64: five non-crash contract cases, two out-of-range float-to-`guint32` stores (#1137) | policy record |
| [`bdb-failed-handle`](bdb-failed-handle.md) | bdb-failed-handle — Berkeley DB handles whose open failed | policy record |
| [`berkeleydb-backend`](berkeleydb-backend.md) | BerkeleyDB backend | implemented |
| [`bigram-punct-format-2026-09-01`](bigram-punct-format-2026-09-01.md) | libpinyin bigram and punctuation formats | verified |
| [`bopomofo-spec`](bopomofo-spec.md) | Bopomofo/Zhuyin scheme SPEC | frozen |
| [`candidate-construction`](candidate-construction.md) | Candidate-construction SPEC (Discrepancy 2) | frozen (§8) |
| [`compatibility-policy`](compatibility-policy.md) | Compatibility policy — what oxpinyin may diverge on, and what it may not | policy |
| [`core-trait-seam`](core-trait-seam.md) | Core trait seam SPEC | behaviour frozen |
| [`data-formats`](data-formats.md) | Data formats — libpinyin system data files | current reference |
| [`double-pinyin-spec`](double-pinyin-spec.md) | Double-pinyin scheme SPEC | frozen |
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
| [`interpolation2-grammar`](interpolation2-grammar.md) | The `interpolation2.text` taglib grammar — one taglib port | record (implemented) |
| [`language-runtime-discrepancy-template`](language-runtime-discrepancy-template.md) | Language/runtime discrepancy record — template | template |
| [`libpinyin-structures-and-algorithms`](libpinyin-structures-and-algorithms.md) | libpinyin's components, data structures, algorithms and coefficients beside oxpinyin's, with the open gaps | register |
| [`library-transitions-in-memory`](library-transitions-in-memory.md) | Library transitions in memory | current reference |
| [`matrix-split-tables`](matrix-split-tables.md) | Matrix split tables SPEC | frozen |
| [`model-provenance`](model-provenance.md) | Model and table provenance | policy (model20 not redistributable) |
| [`option-bits`](option-bits.md) | Option bits | record (implemented) |
| [`parser-path-set`](parser-path-set.md) | Full-pinyin parser path-set SPEC | frozen |
| [`parser-spec`](parser-spec.md) | Full-pinyin parser SPEC | frozen |
| [`phrase-dbm-format-2026-09-01`](phrase-dbm-format-2026-09-01.md) | libpinyin phrase-index DBM format | verified |
| [`phrase-union`](phrase-union.md) | Phrase-index union at lookup | implemented (W11) |
| [`pin-abort-assert-line`](pin-abort-assert-line.md) | Reproduced library aborts write no assertion line | discrepancy record (accepted) |
| [`pin-aborts-journal-replay`](pin-aborts-journal-replay.md) | Pin aborts reproduced in the journal replay | current reference |
| [`pin-aborts-pinyin-facade`](pin-aborts-pinyin-facade.md) | Pin aborts reproduced in the pinyin C facade | current reference |
| [`pin-aborts-remaining`](pin-aborts-remaining.md) | Pin aborts reproduced: the short user-bigram total and the directory library | current reference |
| [`pin-aborts-zhuyin-facade`](pin-aborts-zhuyin-facade.md) | Pin aborts reproduced in the zhuyin C facade | current reference |
| [`pinyin-dbm-format-2026-09-01`](pinyin-dbm-format-2026-09-01.md) | libpinyin pinyin-index DBM format | verified |
| [`probe-coverage-abi`](probe-coverage-abi.md) | Probe coverage over the full exported ABI (79 symbols; residues closed) | current reference (live matrix) |
| [`returned-string-allocation`](returned-string-allocation.md) | Returned-string allocation (#1227) | current reference |
| [`robustness-evidence`](robustness-evidence.md) | F-E cross-lane robustness evidence register | register (F-E) |
| [`scoring-spec`](scoring-spec.md) | Scoring: the language-model term, the cost scale and incomplete keys | current reference |
| [`segment-graph`](segment-graph.md) | SegmentGraph SPEC | frozen |
| [`sentence-surface`](sentence-surface.md) | Sentence surface (W14) | machine-read (frozen) |
| [`session-api`](session-api.md) | Framework-neutral session API SPEC | behaviour frozen |
| [`session-replay`](session-replay.md) | Session replay SPEC | frozen |
| [`store-key-ordering`](store-key-ordering.md) | Store key-ordering contract — one place for the whole stack | contract |
| [`tkrzw-distro-compat`](tkrzw-distro-compat.md) | Debian ships libpinyin on tkrzw; Ubuntu's tkrzw corrupts records | record (distro matrix) |
| [`trainer-parity-audit`](trainer-parity-audit.md) | Trainer-workflow parity audit | audit (current trainer record) |
| [`training-algorithm`](training-algorithm.md) | libpinyin training pipeline — algorithm characterization | verified |
| [`trellis-adjusted-cost-s1-s2`](trellis-adjusted-cost-s1-s2.md) | Pinyin trellis adjusted-cost comparison | policy record |
| [`upstream-divergences`](upstream-divergences.md) | Upstream divergences | register (living) |
| [`upstream-report-drafts`](upstream-report-drafts.md) | Upstream report drafts — the libpinyin report-back batch | open (drafts, not filed) |
| [`user-store`](user-store.md) | libpinyin user store — behaviour characterization | characterization |
