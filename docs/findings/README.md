# Findings

This directory holds the project's decisions, registers, frozen SPECs and records: one file per finding, with its date in the file.
Documents and tools still read every file listed here.
New results go in pull request bodies ([results and reproduction](../../CONTRIBUTING.md#results-and-reproduction)).
Git history holds every retired record.

Statuses:

- **frozen / policy / contract / template** — normative; a change to one is a STOP ([contributor procedures](../../CONTRIBUTING.md)).
  Under the [frozen-SPEC ruling](compatibility-policy.md#frozen-specs), a frozen SPEC binds only pin-derived behaviour within the supported compatibility surfaces (the C APIs and ABIs, persistent data and drop-in utilities), plus approved compatibility decisions; frozen oracle observations stay verification evidence.
  Each frozen SPEC states at the top which part binds; the rest is current reference.
- **register / living reference** — living lists and references.
- **policy record / discrepancy record / evidence** — an accepted difference, or the evidence behind a rule.
  A policy record states the pin's and oxpinyin's results and their source derivation; the tests that hold oxpinyin's side name it, and the [register](upstream-divergences.md) links it.
- **machine-read** — a program or test reads or writes this file.
- **current reference / record / verified / measured / audit / report / implemented / approved / characterization / design rationale / historical** — a dated record.
  It describes the tree within the scope that the file states.
- **open** — work still owed.

A parenthetical after a status is a note about scope or provenance, not a new status.

| Document | Subject | Status |
| --- | --- | --- |
| [`1297-candidate-numeric-sort`](1297-candidate-numeric-sort.md) | Stable sorting with guarded wrapping frequency (#1297 item A) | discrepancy record (accepted) |
| [`1297-gram-partition-segmentation`](1297-gram-partition-segmentation.md) | Phrase segmentation over stored grams (#1297) | policy record |
| [`1297-gram-partition-training`](1297-gram-partition-training.md) | Training over stored grams (#1297) | policy record |
| [`1297-layout-criticals`](1297-layout-criticals.md) | Phrase-library layout faults write no critical (#1297) | policy record |
| [`1297-punct-words`](1297-punct-words.md) | Punctuation values contain whole words (#1297) | policy record |
| [`1297-segment-critical`](1297-segment-critical.md) | Phrase segmentation writes no invalid-UTF-8 critical (#1297) | policy record |
| [`1378-zhuyin-selection`](1378-zhuyin-selection.md) | Zhuyin selection order (#1378) | policy record |
| [`1501-gint16-input-cap`](1501-gint16-input-cap.md) | The full-pinyin parse's 16-bit steps (#1501) | policy record |
| [`1501-guint16-key-positions`](1501-guint16-key-positions.md) | Key positions past byte 65535 (#1501) | policy record |
| [`abi-allocator-pairing`](abi-allocator-pairing.md) | ABI allocator pairing: the register and the gate | contract (the two `.alloc` registers) |
| [`abi-reference`](abi-reference.md) | The exported libpinyin ABI: all 79 `pinyin_*` symbols, with signatures, state model, config and error behaviour | living reference |
| [`addon-choose-promotion`](addon-choose-promotion.md) | Addon choose-promotion, default nibble 5 (#105) | record (#105) |
| [`algorithm-precondition-witnesses-2026-10-28`](algorithm-precondition-witnesses-2026-10-28.md) | Reachable algorithm-precondition witnesses | evidence (policy registered sites) |
| [`arm64-pin-portability-1137`](arm64-pin-portability-1137.md) | Pin-built libpinyin on arm64: five non-crash contract cases, two out-of-range float-to-`guint32` stores (#1137) | policy record |
| [`bdb-failed-handle`](bdb-failed-handle.md) | Berkeley DB handles whose open failed | policy record |
| [`berkeleydb-backend`](berkeleydb-backend.md) | The BerkeleyDB storage backend | implemented |
| [`bigram-punct-format-2026-09-01`](bigram-punct-format-2026-09-01.md) | libpinyin bigram and punctuation formats | verified |
| [`bopomofo-spec`](bopomofo-spec.md) | Bopomofo/Zhuyin scheme SPEC: the four chewing functions, keyboards, indexes and the exact seam | frozen |
| [`candidate-construction`](candidate-construction.md) | Candidate-construction SPEC: the window scan, the three-key order, the frequency key and the dedup | frozen (§8) |
| [`compatibility-policy`](compatibility-policy.md) | Compatibility policy: where oxpinyin may differ from the pin, and where it may not | policy |
| [`core-trait-seam`](core-trait-seam.md) | Core trait seam: the `oxpinyin-core` traits and their behavioural rules | current reference |
| [`data-formats`](data-formats.md) | libpinyin system data file formats | current reference |
| [`double-pinyin-spec`](double-pinyin-spec.md) | Double-pinyin scheme SPEC: the scheme ABI, parser laws, tone law and auxiliary text | frozen |
| [`dynamic-adjust`](dynamic-adjust.md) | The `DYNAMIC_ADJUST` option | record (implemented, #204) |
| [`eio-flexible-bigram-malformed-records`](eio-flexible-bigram-malformed-records.md) | FlexibleBigram malformed records (class (b)) | discrepancy record (accepted) |
| [`eio-flexible-bigram-unsorted-records`](eio-flexible-bigram-unsorted-records.md) | FlexibleBigram item search and the unsorted KC fixture | discrepancy record (accepted) |
| [`eio-kmm-to-interpolation-undefined-lines`](eio-kmm-to-interpolation-undefined-lines.md) | `k_mixture_model_to_interpolation` undefined lines: normal refusal | discrepancy record (accepted) |
| [`eio-kmm-tools`](eio-kmm-tools.md) | KMM tool asserts and their policy records | discrepancy record (accepted) |
| [`eio-pinyin-index-padding`](eio-pinyin-index-padding.md) | The pin's uninitialised padding in pinyin-index items | discrepancy record (accepted) |
| [`eio-table-generator-invalid-utf8`](eio-table-generator-invalid-utf8.md) | Table-generator invalid UTF-8: normal refusal | discrepancy record (accepted) |
| [`eio-test-flexible-attach-empty`](eio-test-flexible-attach-empty.md) | FlexibleBigram test-program attach and magic headers | current reference |
| [`eio-tool-check-messages`](eio-tool-check-messages.md) | Utility check macros write no message | discrepancy record (accepted) |
| [`eio-utf8-next-char-invalid-lead`](eio-utf8-next-char-invalid-lead.md) | `g_utf8_next_char` from a byte that leads no sequence: one-byte step | discrepancy record (accepted) |
| [`eio-zhuyin-missing-frequency`](eio-zhuyin-missing-frequency.md) | `gen_zhuyin_table` missing frequency: normal refusal | discrepancy record (accepted) |
| [`external-io-boundary`](external-io-boundary.md) | External I/O boundary | policy (maintainer ruling; stage 5 in progress) |
| [`glib-libc-call-sites`](glib-libc-call-sites.tsv) | GLib and libc call sites at the pin, generated by `tools/audit/glib_call_sites.py` | machine-read |
| [`glib-libc-inventory`](glib-libc-inventory.md) | GLib and libc at the pin: reachable calls and oxpinyin counterparts | current reference |
| [`glib-libc-verification`](glib-libc-verification.md) | GLib/libc verification coverage | current reference |
| [`installed-naming`](installed-naming.md) | The installed tree takes libpinyin's name; the source tree keeps ours | implemented |
| [`interpolation2-grammar`](interpolation2-grammar.md) | The `interpolation2.text` taglib grammar: one taglib port | record (implemented) |
| [`language-runtime-discrepancy-template`](language-runtime-discrepancy-template.md) | Template for a language/runtime discrepancy record | template |
| [`libpinyin-structures-and-algorithms`](libpinyin-structures-and-algorithms.md) | libpinyin's components, data structures, algorithms and coefficients beside oxpinyin's, with the open gaps | register |
| [`library-transitions-in-memory`](library-transitions-in-memory.md) | Library transitions in memory | current reference |
| [`matrix-split-tables`](matrix-split-tables.md) | Matrix split tables SPEC: the resplit pairs and divided syllables of the scan matrix | frozen |
| [`model-provenance`](model-provenance.md) | Model and table provenance | policy (model20 not redistributable) |
| [`option-bits`](option-bits.md) | Option bits: where each `pinyin_option_t` bit class acts, and oxpinyin's handling | record (implemented) |
| [`parser-path-set`](parser-path-set.md) | Full-pinyin parser path-set SPEC: the ordered alternatives and the result bound | frozen |
| [`parser-spec`](parser-spec.md) | Full-pinyin parser SPEC: the inventory, input domain, result fields and apostrophe rules | frozen |
| [`phrase-dbm-format-2026-09-01`](phrase-dbm-format-2026-09-01.md) | libpinyin phrase-index DBM format | verified |
| [`phrase-union`](phrase-union.md) | Phrase-index union at lookup | implemented |
| [`pin-abort-assert-line`](pin-abort-assert-line.md) | Reproduced library aborts write no assertion line | discrepancy record (accepted) |
| [`pin-aborts-journal-replay`](pin-aborts-journal-replay.md) | Pin aborts reproduced in the journal replay | current reference |
| [`pin-aborts-pinyin-facade`](pin-aborts-pinyin-facade.md) | Pin aborts reproduced in the pinyin C facade | current reference |
| [`pin-aborts-remaining`](pin-aborts-remaining.md) | Pin aborts reproduced: the short user-bigram total and the directory library | current reference |
| [`pin-aborts-zhuyin-facade`](pin-aborts-zhuyin-facade.md) | Pin aborts reproduced in the zhuyin C facade | current reference |
| [`pinyin-dbm-format-2026-09-01`](pinyin-dbm-format-2026-09-01.md) | libpinyin pinyin-index DBM format | verified |
| [`probe-coverage-abi`](probe-coverage-abi.md) | Probe coverage over the full exported ABI (79 symbols; residues closed) | current reference (live matrix) |
| [`returned-string-allocation`](returned-string-allocation.md) | Returned-string allocation (#1227) | current reference |
| [`robustness-evidence`](robustness-evidence.md) | F-E cross-lane robustness evidence | register (F-E) |
| [`scoring-spec`](scoring-spec.md) | Scoring: the language-model term, the cost scale and incomplete keys | current reference |
| [`segment-graph`](segment-graph.md) | SegmentGraph SPEC: the byte-position key graph and the incomplete-key inventory | frozen |
| [`sentence-surface`](sentence-surface.md) | Sentence surface: the n-best sentence rows and the measured selection residuals | machine-read |
| [`session-api`](session-api.md) | Framework-neutral session API: the `oxpinyin-engine` session design under the C facades | current reference |
| [`session-replay`](session-replay.md) | Session replay: authored scenarios with the pin's expected candidates | current reference |
| [`store-key-ordering`](store-key-ordering.md) | Store key-ordering contract for the whole stack | contract |
| [`tkrzw-distro-compat`](tkrzw-distro-compat.md) | Debian ships libpinyin on tkrzw; Ubuntu's tkrzw corrupts records | record (distro matrix) |
| [`trainer-parity-audit`](trainer-parity-audit.md) | Trainer-workflow parity audit | audit (current trainer record) |
| [`training-algorithm`](training-algorithm.md) | The libpinyin training pipeline: algorithm characterization | verified |
| [`trellis-adjusted-cost-s1-s2`](trellis-adjusted-cost-s1-s2.md) | Pinyin trellis adjusted-cost comparison | policy record |
| [`upstream-divergences`](upstream-divergences.md) | Upstream divergences: pin behaviours oxpinyin does not reproduce, and pin defects to report | register (living) |
| [`upstream-report-drafts`](upstream-report-drafts.md) | Upstream report drafts: the libpinyin report-back batch | open (drafts, not filed) |
| [`user-store`](user-store.md) | The libpinyin user store: behaviour characterization | characterization |
