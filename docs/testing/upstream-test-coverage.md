# Upstream test coverage ledger

Status: living document · Created: 2026-08-29

This ledger records, for every test program in upstream
[libpinyin](https://github.com/libpinyin/libpinyin)/`tests` and
[chewing/libchewing](https://github.com/chewing/libchewing)/`tests`, what
behavior it protects and where that behavior lives in oxpinyin.

**Audited upstream revisions** (every entry below describes these
snapshots; re-audit the delta when either moves):

| Project | Revision | Snapshot date |
|---|---|---|
| libpinyin (pin) | `074a2219c90feaf962d0d24f034514033ece5f99` | 2026-09-03 |
| libchewing (`master`) | `179a02f0629c1137050c736acddc17c9424bf1d2` | 2026-07-17 |

The libchewing behavior-coverage vocabulary (the libpinyin drop-in statuses are defined below):

- **PORT** — the invariant is reproduced with its assertion strength
  intact against the oxpinyin equivalent.
- **ADAPT** — the behavior is preserved through oxpinyin's own API shapes
  (assertions re-expressed where the surface differs; the divergence is
  stated).
- **REPLACE** — the same risk is covered by a stronger or differently
  shaped oxpinyin mechanism (usually a pin-verified differential).
- **N/A** — no oxpinyin equivalent exists or the behavior is
  harness/C++-mechanics specific. Deliberately not faked.

The task ordering: **never silently omit an upstream test.** Every N/A
states why.

---

## libpinyin tests

Maintainer ruling, 2026-11-01 UTC (`date -u`): **every program under the
pin's `tests/` is ported as a drop-in tool, without exception**. Its binary
name, arguments, stdin protocol, stdout/stderr bytes, exit status and
fixed file paths are part of the contract. The tools live in nonshipping
`oxpinyin-utils` and are not installed. This is the single coverage ledger
linked by [the external-I/O ruling](../findings/external-io-boundary.md#ruling-and-scope).

All **17** programs are `noinst_PROGRAMS`; only **six** also appear in
Automake `TESTS`. The inventory is `tests/Makefile.am:37-59`,
`tests/storage/Makefile.am:31-46`, `tests/include/Makefile.am:25-27` and
`tests/lookup/Makefile.am:31-32` at the audited pin. `test_zhuyin` is
conditional on `--enable-libzhuyin`. Interactive drivers and manual test
programs have the same porting requirement as the six automated tests.

**GAP** means the complete drop-in executable is pending. **Ported** means
its source-defined interface is implemented and its registered `tool-diff`
cases pass the required cells. Existing unit tests, adapted APIs and
algorithm differentials do not by themselves make a program Ported.
The previous PORT/ADAPT/REPLACE/N/A entries described internal behavior
coverage and are superseded for these programs; no program is exempt.

| Family | Pin source / binary | Automated `TESTS` | Program behavior and fixed inputs | Drop-in status |
| --- | --- | --- | --- | --- |
| A | `tests/test_pinyin.cpp` / `test_pinyin` | No | `../data`: full-pinyin parse, prefix guess, sorted candidates, auxiliary text at every offset, train/reset/save, then mask-out/save/fini | **Ported** (2026-11-01 UTC) |
| A | `tests/test_phrase.cpp` / `test_phrase` | No | `../data`: raw Chinese phrase segmentation, token text, save and mask-out/save/fini | **Ported** (2026-11-01 UTC) |
| A | `tests/test_chewing.cpp` / `test_chewing` | No | `../data`: Pinyin C API's chewing parse, sentence and auxiliary text, train/reset/save and teardown | **Ported** (2026-11-01 UTC) |
| A | `tests/test_zhuyin.cpp` / `test_zhuyin` | No | `../data`: the separate `zhuyin_*` C API, chewing parse and sentence, train/reset/save and teardown | **Ported** (2026-11-01 UTC) |
| B | `tests/storage/test_parser2.cpp` / `test_parser2` | No | GLib `--parser`, `--incomplete`, `--scheme`; stdin; every parser variant, consumed length and raw key spans; `timer.h` output | **Ported** — `test_parser2.json`, 26 tkrzw cases |
| C | `tests/storage/test_ngram.cpp` / `test_ngram` | Yes | `SingleGram`/`Bigram` operations; `/tmp/test.db`, `/tmp/snapshot.db` | **Ported** — same-named tool case file, all three cells |
| C | `tests/storage/test_flexible_ngram.cpp` / `test_flexible_ngram` | Yes | Flexible single-gram range search and mutation, bigram storage; `/tmp/training.db` | **Ported** — same-named tool case file, all three cells |
| C | `tests/storage/test_punct_table.cpp` / `test_punct_table` | Yes | Punctuation append/remove and snapshot save/load; `/tmp/punct.bin`, `/tmp/snapshot.db` | **Ported** — same-named tool case file, all three cells |
| D | `tests/storage/test_table_info.cpp` / `test_table_info` | Yes | System/user table configuration and conformance; `../../data/table.conf`, `/tmp/user.conf` | **Ported** — `test_table_info.json`, 11 MATCH and 3 POLICY-ACCEPTED on tkrzw |
| E | `tests/include/test_memory_chunk.cpp` / `test_memory_chunk` | Yes | Chunk content, capacity, insert/remove, save/load/map; `/tmp/test.bin` | **Ported** — `test_memory_chunk.json`, 3 MATCH and 1 POLICY-ACCEPTED on tkrzw |
| E | `tests/storage/test_phrase_index.cpp` / `test_phrase_index` | No | Phrase-item/index mutation, frequency, compact and chunk round trips; `../../data/*` through `tests_helper.h` | **GAP** |
| E | `tests/storage/test_phrase_index_logger.cpp` / `test_phrase_index_logger` | Yes | Phrase-index logger diff/merge; `../../data/*`; `/tmp/gb_char.bin`, `/tmp/gb_char.dbin`, `/tmp/gb_char2.bin` | **GAP** |
| E | `tests/storage/test_phrase_table.cpp` / `test_phrase_table` | No | Phrase-table lookup and mutation over `../../data/*` | **GAP** |
| E | `tests/storage/test_chewing_table.cpp` / `test_chewing_table` | No | Chewing-table lookup and mutation over `../../data/*` | **GAP** |
| E | `tests/storage/test_matrix.cpp` / `test_matrix` | No | Phonetic-key matrix construction and search over `../../data/*` | **GAP** |
| E | `tests/lookup/test_pinyin_lookup.cpp` / `test_pinyin_lookup` | No | Phonetic n-best lookup over parsed matrices, phrase chunks and `../../data/bigram.db` | **GAP** |
| E | `tests/lookup/test_phrase_lookup.cpp` / `test_phrase_lookup` | No | Raw-text phrase lookup with phrase chunks and `../../data/bigram.db` | **GAP** |

Family A is implemented by the four same-named `oxpinyin-utils` binaries
and `src/pin_tests.rs`, using tool-gated ownership adapters in the two
existing C facades. Its one-per-program case files are
`tools/bisection/tool-cases/test_{pinyin,phrase,chewing,zhuyin}.json`:
21 cases match on tkrzw, bdb and kc, including saved user files. The named
`pin-tests-a-prefix-prompt.patch` mutation is detected by
`test-pinyin-quit` on tkrzw; reversing it and rebuilding restores MATCH.

Family C ports `test_ngram`, `test_flexible_ngram` and `test_punct_table`
with 14 source-derived cases in their three same-named case files. The
fixed `/tmp` paths remain isolated per side. Combined with all 45 current
K-mixture cases, the all-cell result is tkrzw 50 MATCH / 9 POLICY-ACCEPTED,
BDB 44 / 15, and KC 48 / 11, with no DIFFER or INVALID. Policy records bind
the existing assertion-text and BDB failed-handle diagnostic exceptions. The tkrzw abort files use
only the approved eight-byte clock-field normalization. The named
`pin-tests-c-ngram-token-plus-one.patch` mutation is detected by
`ngram-fresh`; reverting and rebuilding restores the complete gate.
[Shared attach and backend evidence](../findings/eio-test-flexible-attach-empty.md)
records the native open, magic-header and mutation calls.

Family E1 adds `test_memory_chunk` with the pin's logical capacity rule.
Its four source-derived cases give 3 MATCH and 1 POLICY-ACCEPTED on tkrzw.
The `pin-tests-e1-remove-wrong-integer` mutation makes `memory-chunk-fresh`
DIFFER in stdout and the framed file; restoring it restores the gate.

The existing Rust Pinyin and Zhuyin C facades are separate implementations;
raw-text phrase segmentation and phrase lookup exist. Those earlier N/A
claims and the claim that Zhuyin is unified onto the Pinyin C API are stale.
The direct parsers' complete key-rest and consumed-length interfaces must
be checked separately from full/keyboard parser tests. Likewise, internal
punctuation mutation and flexible n-gram operations are not replaced by
training-utility coverage. The remaining complete binaries stay GAP until ported.

`tests/tests_helper.h` and `tests/timer.h` are shared source dependencies,
not additional executables. Helper-loaded `../../data` trees and every
user file written back into `../data` are compared per side. Fixed `/tmp`
paths are isolated across processes by the runner. Timing lines are the
only clock-dependent test-program output; all other bytes remain compared
under the existing external-I/O/check-message policies.

Delivery order is A, B, D, C. Family E is unblocked by the landed chunk
mapping and `MemoryChunk::load` work (#1549, #1576 and #1579). Its eight
programs are next as new members above #1528, outside this pass.

## libchewing tests

libchewing is a Zhuyin/Bopomofo IME with its own dictionary formats,
symbol tables, and C API conventions. oxpinyin shares the *behavior
class* (an IME session: keys → preedit/candidates/commit, user phrases,
config) but not the data or ABI. Tests that protect chewing-only UI or
ABI behaviors are N/A by design rather than artificially reconstructed.

| Source | Behavior protected | Status | oxpinyin home |
|---|---|---|---|
| `test-bopomofo.c` | ~60 session behaviors: candidate selection (forward/rearward/mid-composition/paging), Esc/Backspace/Del semantics, mode switches, ShiftSpace full-shape, Numlock, auto-commit thresholds, interval enumeration, per-keyboard conversions (HSU/ET/ET26/CP26/GinYieh/IBM/Dvorak/Colemak), pinyin keyboards (HANYU/THL/MPS2), fuzzy search, `phone_to_bopomofo` | ADAPT (selection + keys + keyboards), N/A (rest) | Selection: `crates/oxpinyin-engine/tests/session_lifecycle.rs` (choose/commit/out-of-range/mid-composition) and `crates/oxpinyin-engine/tests/decoding.rs` (choose advances composition, bigram feedback, apostrophe boundaries). Key semantics: `crates/oxpinyin-engine/tests/decoding.rs` + `crates/oxpinyin-engine/tests/session_replay.rs` (Enter/Escape/Backspace outcomes, determinism). Keyboards: `crates/oxpinyin-core/tests/scheme_parsers.rs` (8 zhuyin keyboards parse with pinned dachen expectations; HSU discreteness; CP26 exists as a distinct keyboard). N/A with reasons: rearward choice mode (not implemented), full-shape, mode-switch keys, Numlock handling, auto-commit thresholds, interval enumeration (no such surface), THL/MPS2 romanizations (not implemented), static-buffer aliasing (Rust `String`), 4-byte UTF-8 selection (covered generically by the engine's property tests) |
| `test-keyboard.c` | set/get round-trip over 17 keyboard layouts, invalid rejection, `KBStr2Num`, enumeration | ADAPT | layout switching + rejection contracts: `crates/oxpinyin-capi/tests/abi/contract.rs` (every implemented scheme value sticks; out-of-enum and abort slots rejected without state damage — the #109 contract-lock) and `crates/oxpinyin-core/tests/scheme_parsers.rs` (`customized`/`StandardDvorak` slots). N/A: string-based keyboard names and enumeration (`KBStr2Num`, `kbtype_Enumerate`) — no string-keyboard API exists |
| `test-keyboardless.c` | programmatic API: `cand_open/close/choose_by_index` incl. out-of-range and not-in-select, `cand_list_*` traversal, `commit_preedit_buf`/`clean_preedit_buf`/`clean_bopomofo_buf` | ADAPT | `crates/oxpinyin-engine/tests/session_lifecycle.rs` (choose without composition fails, out-of-range fails without state damage, choice commits the exact phrase, offset windows via `candidates_at`/`select_anchored`). N/A: list traversal (`cand_list_next/prev/first/last`) and the three explicit buffer operations — oxpinyin's composition model has no equivalent operations to bind |
| `test-config.c` | option existence, defaults, setter validation (booleans 0/1 only, clamped ranges), syspath/userpath construction variants, version reporting | ADAPT | defaults: `crates/oxpinyin-engine/tests/upstream_defaults.rs` pins all 69 upstream keys key-for-key. Setter rejection: `crates/oxpinyin-capi/tests/abi/contract.rs`. Construction variants: `crates/oxpinyin-runtime/tests/assembly.rs` (system/user dir combinations incl. degrade-on-file, empty-path) and the capi fixture init. N/A: runtime setter clamping (config is layered files + option bits, not live setters), `chewing_version()` |
| `test-userphrase.c` | add/remove/lookup/enumerate, persistence across contexts, same-reading coexistence, max length, error handling, auto-learn gating (only after commit, learn-excluded) | PORT (store behavior) / ADAPT (learning) | `crates/oxpinyin-user/tests/user_store_semantics.rs` (§ phrase lifecycle: roundtrip, persistence across handles, multi-reading accumulation, 15/16 length ceiling, key-count mismatch, remove-absent). Learning: `crates/oxpinyin-capi/src/e2e_tests.rs` (train entry points, counts, mask-out; `pinyin_train` refuses a selection-less instance) and the store's observe path in `user_store_semantics.rs`. N/A: Shift-Left/Right and Ctrl+Num add-phrase UI gestures (no keystroke surface binds them) |
| `test-symbol.c` / `test-special-symbol.c` / `test-easy-symbol.c` | symbol-table typing, backtick menus, easy-symbol expansion, punctuation collisions | N/A | oxpinyin has no symbol-input surface: the punct table is consulted only by prediction (`pinyin_guess_predicted_candidates_with_punctuations`, covered in `crates/oxpinyin-capi/src/union_e2e_tests.rs`). Easy/special symbols do not exist |
| `test-fullshape.c` | full/half-shape toggle and full-width output | N/A | full-shape is not implemented (the `init-full` schema keys are captured defaults with no reader) |
| `test-reset.c` | `Reset` clears session state without dropping static data/config | PORT | `crates/oxpinyin-engine/tests/session_lifecycle.rs` (§ reset): config/options survive, a reset session converts identically to a fresh one |
| `test-error-handling.c` | ~80 NULL-context contracts; fallback dictionary keeps the engine alive on corrupt data | N/A / ADAPT | NULL contracts are C-ABI mechanics the Rust surface cannot express (type safety); the capi layer still guards every entry (`if instance.is_null()`) and the abort-contract behavior of the scheme setters is `crates/oxpinyin-capi/tests/abi/contract.rs`. Fail-fast on missing tables (`OpenError`) replaces the fallback-dictionary tolerance — an explicit divergence (oxpinyin refuses to open rather than degrading) |
| `test-regression.c` | ~15 specific crash/conversion regressions (fuzz-found sequences, cursor/selection interactions) | REPLACE | generic crash resistance: `fuzz/fuzz_targets/parser.rs`, `crates/oxpinyin-engine/tests/session_replay.rs` (`arbitrary_characters_never_panic`), `crates/oxpinyin-core/tests/parser_acceptance.rs` (arbitrary-byte determinism/limits). The specific chewing conversion regressions are N/A: they convert through chewing dictionaries oxpinyin does not ship |
| `test-struct-size.c` | `sizeof` guards on `ChewingConfigData`/`IntervalType` | N/A | the C ABI uses opaque handles; ABI stability is governed by the cargo-c soname contract (W8), not struct layouts |
| `test-logger.c` | custom logger install/disable | N/A | no logger API |
| `performance.c`, `stress.c`, `randkeystroke.c`, `simulate.c`, `genkeystroke.c`, `stresstest.py` | timing, fuzzing, batch scoring, scenario authoring tools | REPLACE | criterion benches (`crates/oxpinyin-capi/benches/stage2.rs`, `crates/pinyin-oracle/benches/*`), the bounded fuzz run in CI plus `fuzz/corpus/`, recorded session scenarios (`fixtures/w4/f-d-session.txt` replayed by `session_replay.rs` — the `simulate.c` analog with assertions) |
| `data/` (CHEW dictionaries, swkb/symbols tables, materials.txt) | test dictionaries | N/A | oxpinyin tests run on its own committed fixtures (`fixtures/w3`, `fixtures/w4`, `fixtures/w9`, `fixtures/foundation`) |

## Totals

- **libpinyin**: 17 `noinst_PROGRAMS`, six also in automated `TESTS`;
  **10 Ported, 7 GAP** after families A, B, D, C and E1. Family PRs update their rows
  and this total when their drop-in verification passes.
- The libchewing rows retain their separately audited behavior-coverage
  classifications. They do not count toward the libpinyin drop-in total.

## Maintenance rule

Each libpinyin family PR updates its rows from GAP to Ported, names its
source-derived case files and records the selected-cell results. Keep the
17-program inventory tied to the pin's four Makefiles; do not replace a
missing executable with an internal-test claim or silently omit it.

For libchewing, when a row says N/A because a surface does not exist and
that surface later ships, update its behavior-coverage classification and
add the test in the same change.
