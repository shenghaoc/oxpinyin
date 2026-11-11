# Upstream test coverage ledger

Status: living document · Created: 2026-08-29

This ledger covers every test program in upstream
[libpinyin](https://github.com/libpinyin/libpinyin)/`tests` and
[chewing/libchewing](https://github.com/chewing/libchewing)/`tests`.
For each program, it records the behavior that the program protects and where that behavior lives in oxpinyin.

**Audited upstream revisions.** Every entry below describes these
snapshots. When either revision moves, audit the delta again:

| Project | Revision | Snapshot date |
|---|---|---|
| libpinyin (pin) | `074a2219c90feaf962d0d24f034514033ece5f99` | 2026-09-03 |
| libchewing (`master`) | `179a02f0629c1137050c736acddc17c9424bf1d2` | 2026-07-17 |

The libchewing rows use this behavior-coverage vocabulary. The libpinyin drop-in statuses are defined below.

- **PORT** — the invariant is reproduced against the oxpinyin equivalent, with its assertion strength
  intact.
- **ADAPT** — the behavior is kept through the API shapes of oxpinyin.
  Assertions are re-expressed where the surface differs, and the divergence is
  stated.
- **REPLACE** — a stronger or differently
  shaped oxpinyin mechanism covers the same risk (usually a pin-verified differential).
- **N/A** — no oxpinyin equivalent exists, or the behavior is specific to
  harness or C++ mechanics. It is deliberately not faked.

The rule: **never silently omit an upstream test.** Every N/A
states why.

---

## libpinyin tests

Maintainer ruling, 2026-11-01 UTC (`date -u`): **every program under the
pin's `tests/` is ported as a drop-in tool, without exception**. The binary
name, arguments, stdin protocol, stdout/stderr bytes, exit status and
fixed file paths of each program are part of the contract. The tools live in the nonshipping
`oxpinyin-utils` crate and are not installed. [The external-I/O ruling](../findings/external-io-boundary.md#ruling-and-scope)
links this ledger as the single coverage ledger.

All **17** programs are `noinst_PROGRAMS`. Only **six** of them also appear in
Automake `TESTS`. The inventory is `tests/Makefile.am:37-59`,
`tests/storage/Makefile.am:31-46`, `tests/include/Makefile.am:25-27` and
`tests/lookup/Makefile.am:31-32` at the audited pin. `test_zhuyin` is
conditional on `--enable-libzhuyin`. Interactive drivers and manual test
programs have the same porting requirement as the six automated tests.

**GAP** means that the complete drop-in executable is pending. **Ported** means
that its source-defined interface is implemented and that its [frozen cases](../runbooks/tool-diff.md)
pass the required cells. Existing unit tests, adapted APIs and
algorithm differentials alone do not make a program Ported.
For these programs, the drop-in statuses replace the PORT/ADAPT/REPLACE/N/A vocabulary, which describes internal behavior
coverage. No program is exempt.

| Family | Pin source / binary | Automated `TESTS` | Program behavior and fixed inputs | Drop-in status |
| --- | --- | --- | --- | --- |
| A | `tests/test_pinyin.cpp` / `test_pinyin` | No | `../data`: full-pinyin parse, prefix guess, sorted candidates, auxiliary text at every offset, train/reset/save, then mask-out/save/fini | **Ported** (2026-11-01 UTC) |
| A | `tests/test_phrase.cpp` / `test_phrase` | No | `../data`: raw Chinese phrase segmentation, token text, save and mask-out/save/fini | **Ported** (2026-11-01 UTC) |
| A | `tests/test_chewing.cpp` / `test_chewing` | No | `../data`: Pinyin C API's chewing parse, sentence and auxiliary text, train/reset/save and teardown | **Ported** (2026-11-01 UTC) |
| A | `tests/test_zhuyin.cpp` / `test_zhuyin` | No | `../data`: the separate `zhuyin_*` C API, chewing parse and sentence, train/reset/save and teardown | **Ported** (2026-11-01 UTC) |
| B | `tests/storage/test_parser2.cpp` / `test_parser2` | No | GLib `--parser`, `--incomplete`, `--scheme`; stdin; every parser variant, consumed length and raw key spans; `timer.h` output | **Ported** — `test_parser2.json`, 16 tkrzw cases |
| C | `tests/storage/test_ngram.cpp` / `test_ngram` | Yes | `SingleGram`/`Bigram` operations; `/tmp/test.db`, `/tmp/snapshot.db` | **Ported** — same-named frozen test module, all three cells |
| C | `tests/storage/test_flexible_ngram.cpp` / `test_flexible_ngram` | Yes | Flexible single-gram range search and mutation, bigram storage; `/tmp/training.db` | **Ported** — same-named frozen test module, all three cells |
| C | `tests/storage/test_punct_table.cpp` / `test_punct_table` | Yes | Punctuation append/remove and snapshot save/load; `/tmp/punct.bin`, `/tmp/snapshot.db` | **Ported** — same-named frozen test module, all three cells |
| D | `tests/storage/test_table_info.cpp` / `test_table_info` | Yes | System/user table configuration and conformance; `../../data/table.conf`, `/tmp/user.conf` | **Ported** — `test_table_info.json`, 8 MATCH and 3 POLICY-ACCEPTED on tkrzw |
| E | `tests/include/test_memory_chunk.cpp` / `test_memory_chunk` | Yes | Chunk content, capacity, insert/remove, save/load/map; `/tmp/test.bin` | **Ported** — `test_memory_chunk.json`, 2 MATCH and 1 POLICY-ACCEPTED on tkrzw |
| E | `tests/storage/test_phrase_index.cpp` / `test_phrase_index` | No | Phrase-item/index mutation, frequency, compact and chunk round trips; `../../data/*` through `tests_helper.h` | **Ported** — same-named frozen test module and source-only class (b) guards |
| E | `tests/storage/test_phrase_index_logger.cpp` / `test_phrase_index_logger` | Yes | Phrase-index logger diff/merge; `../../data/*`; `/tmp/gb_char.bin`, `/tmp/gb_char.dbin`, `/tmp/gb_char2.bin` | **Ported** — same-named frozen test module and source-only class (b) guards |
| E | `tests/storage/test_phrase_table.cpp` / `test_phrase_table` | No | Phrase-table lookup and mutation over `../../data/*` | **Ported** — same-named frozen test module, all three cells, with class (b) source-only guards |
| E | `tests/storage/test_chewing_table.cpp` / `test_chewing_table` | No | Chewing-table lookup and mutation over `../../data/*` | **Ported** — same-named frozen test module, all three cells, with class (b) source-only guards |
| E | `tests/storage/test_matrix.cpp` / `test_matrix` | No | Phonetic-key matrix construction and search over `../../data/*` | **Ported** — same-named frozen test module; matrix, result positions and text compared |
| E | `tests/lookup/test_pinyin_lookup.cpp` / `test_pinyin_lookup` | No | Phonetic n-best lookup over parsed matrices, phrase chunks and `../../data/bigram.db` | **Ported** — same-named frozen test module; matrix, result positions and text compared |
| E | `tests/lookup/test_phrase_lookup.cpp` / `test_phrase_lookup` | No | Raw-text phrase lookup with phrase chunks and `../../data/bigram.db` | **Ported** — same-named frozen test module; matrix, result positions and text compared |

Family A is implemented by the four same-named `oxpinyin-utils` binaries
and `src/pin_tests.rs`. They use tool-gated ownership adapters in the two
existing C facades. Its cases are the `frozen::test_{pinyin,phrase,chewing,zhuyin}`
modules of `crates/oxpinyin-utils/tests/storage_tools.rs`.
13 cases match on tkrzw, bdb and kc, including saved user files. The module-local test
`oxpinyin_input::tool::tests::prefix_prompt_writes_the_pins_exact_bytes` holds the exact prompt bytes.
The existing differential case is `test-pinyin-quit`.

Family C ports `test_ngram`, `test_flexible_ngram` and `test_punct_table`
with 11 source-derived cases in their three same-named frozen test modules.
The fixed `/tmp` paths stay isolated for each side. Combined with all 44 current
K-mixture cases, the all-cell result is:

- tkrzw: 46 MATCH / 9 POLICY-ACCEPTED;
- BDB: 40 / 15;
- KC: 44 / 11;
- no DIFFER or INVALID.

Policy records bind
the existing assertion-text and BDB failed-handle diagnostic exceptions. The tkrzw abort files use
only the approved eight-byte clock-field normalization. The `frozen::test_ngram` case
`ngram_ignored_input` in `oxpinyin-utils` holds the token arithmetic.
[Shared attach and backend evidence](../findings/eio-test-flexible-attach-empty.md)
records the native open, magic-header and mutation calls.

Family E1 adds `test_memory_chunk` with the logical capacity rule of the pin.
Its three source-derived cases give 2 MATCH and 1 POLICY-ACCEPTED on tkrzw.
The `frozen::test_memory_chunk` case
`memory_chunk_ignored_input` in `oxpinyin-utils` holds the removal offset.

Family E2 adds the phrase-index and logger executables with 13 cases:
9 MATCH and 4 POLICY-ACCEPTED on tkrzw.

- The sparse and overflow logger cases
  keep the facade cache across load and merge, independently of the local
  stored total.
- Refused layouts contribute no load total; a reload still subtracts
  the prior local total.
- Defined same-token invalid conversions continue.
- The invalid-UTF-8 index guard runs only the subject. It checks
  exit EINVAL and the output printed before conversion.
- Required-load failures
  use pin_not_run and exit ENOENT without a diagnostic.

In `oxpinyin-utils`, the
`frozen::test_phrase_index_logger` cases `phrase_index_logger_ignored_input` and
`phrase_index_logger_refused_layout` hold the logger delta and the total of the refused load.
The `frozen::test_phrase_index` cases
`phrase_index_same_token_invalid_utf8_reading` and
`phrase_index_same_token_invalid_utf8_skip` hold defined same-token conversions.
A review mutation also detects an omitted
token-zero conversion guard. All three consuming guard tests are
subject-only and keep the preceding output.

Family E3 adds the phrase-table and chewing-table executables. Their 18
source-derived cases pass all three cells: 12 MATCH / 6 POLICY-ACCEPTED.

- Only timer fields are masked. Token, range and pronunciation order, and the
  literal backspace before frequencies, stay exact.
- Token-zero cursors are
  empty before extending/flushing and at the final flush.
- Only the first
  incomplete key asserts its zero tone, before the overlong-query refusal.
- Invalid UTF-8 and the
  17-key direct search use source-only guards.
- Subject-only tests also check
  that EINVAL keeps earlier timed results.

The module-local test
`storage_tests::tables::tests::an_empty_phrase_row_still_reports_continuation`
in `oxpinyin-utils` holds continuation, together with `phrase-table-session`.
`storage_tests::tables::tests::a_zero_cursor_keeps_the_following_chewing_token`
holds the token after a zero cursor.
The `frozen::test_chewing_table` cases `chewing_table_toned_initial` and
`chewing_table_overlong_toned_first` hold the first-key tone assertion and its
order before the length refusal.

Family E4 adds `test_matrix`, `test_pinyin_lookup` and `test_phrase_lookup`.
Their 19 source-derived cases give 16 MATCH / 3 POLICY-ACCEPTED on the
selected cells.

- The matrix includes separator and terminal zero keys.
- Lookup results keep non-null positions, token order and the
  newline-separated `token phrase` entries of the phrase lookup.
- Required chunk failures
  and empty/unsegmentable text keep the diagnostics and normal exits of the pin.
- The `frozen::test_matrix` case
  `matrix_session` in `oxpinyin-utils` holds the ambiguity options.
- Only source timer fields
  are masked. No undefined path of the pin is executed.

The Rust Pinyin and Zhuyin C facades are separate implementations.
Raw-text phrase segmentation and phrase lookup exist. Zhuyin is not unified onto the Pinyin C API.
The complete key-rest and consumed-length interfaces of the direct parsers must
be checked separately from the full/keyboard parser tests. In the same way,
training-utility coverage does not replace internal
punctuation mutation and flexible n-gram operations. Every listed libpinyin executable has its drop-in implementation.

`tests/tests_helper.h` and `tests/timer.h` are shared source dependencies,
not additional executables. Helper-loaded `../../data` trees and every
user file written back into `../data` are compared for each side. The runner isolates the fixed `/tmp`
paths across processes. Timing lines are the
only clock-dependent test-program output. All other bytes stay compared
under the existing external-I/O and check-message policies.

All eight family E programs are ported across E1–E4.

## libchewing tests

libchewing is a Zhuyin/Bopomofo IME with its own dictionary formats,
symbol tables and C API conventions. oxpinyin shares the *behavior
class* (an IME session: keys → preedit/candidates/commit, user phrases,
config), but not the data or the ABI. Tests that protect chewing-only UI or
ABI behaviors are N/A by design; they are not artificially reconstructed.

| Source | Behavior protected | Status | oxpinyin home |
|---|---|---|---|
| `test-bopomofo.c` | ~60 session behaviors: candidate selection (forward/rearward/mid-composition/paging), Esc/Backspace/Del semantics, mode switches, ShiftSpace full-shape, Numlock, auto-commit thresholds, interval enumeration, per-keyboard conversions (HSU/ET/ET26/CP26/GinYieh/IBM/Dvorak/Colemak), pinyin keyboards (HANYU/THL/MPS2), fuzzy search, `phone_to_bopomofo` | ADAPT (selection + keyboards), N/A (rest) | Selection: `crates/oxpinyin-engine/tests/session_lifecycle.rs` (choose/commit/out-of-range/mid-composition) and `crates/oxpinyin-engine/tests/decoding.rs` (choose advances composition, bigram feedback, apostrophe boundaries). Determinism: `crates/oxpinyin-engine/tests/decoding.rs` + `crates/oxpinyin-engine/tests/session_replay.rs`; Enter/Escape/Backspace are N/A (the engine has no key layer). Keyboards: `crates/oxpinyin-core/tests/scheme_parsers.rs` (8 zhuyin keyboards parse with pinned dachen expectations; HSU discreteness; CP26 exists as a distinct keyboard). N/A with reasons: rearward choice mode (not implemented), full-shape, mode-switch keys, Numlock handling, auto-commit thresholds, interval enumeration (no such surface), THL/MPS2 romanizations (not implemented), static-buffer aliasing (Rust `String`), 4-byte UTF-8 selection (covered generically by the engine's property tests) |
| `test-keyboard.c` | set/get round-trip over 17 keyboard layouts, invalid rejection, `KBStr2Num`, enumeration | ADAPT | layout switching + rejection contracts: `crates/oxpinyin-capi/tests/abi/contract.rs` (every implemented scheme value sticks; out-of-enum and abort slots rejected without state damage — the #109 contract-lock) and `crates/oxpinyin-core/tests/scheme_parsers.rs` (`customized`/`StandardDvorak` slots). N/A: string-based keyboard names and enumeration (`KBStr2Num`, `kbtype_Enumerate`) — no string-keyboard API exists |
| `test-keyboardless.c` | programmatic API: `cand_open/close/choose_by_index` incl. out-of-range and not-in-select, `cand_list_*` traversal, `commit_preedit_buf`/`clean_preedit_buf`/`clean_bopomofo_buf` | ADAPT | `crates/oxpinyin-engine/tests/session_lifecycle.rs` (choose without composition fails, out-of-range fails without state damage, choice commits the exact phrase, offset windows via `candidates_at`/`select_anchored`). N/A: list traversal (`cand_list_next/prev/first/last`) and the three explicit buffer operations — oxpinyin's composition model has no equivalent operations to bind |
| `test-config.c` | option existence, defaults, setter validation (booleans 0/1 only, clamped ranges), syspath/userpath construction variants, version reporting | ADAPT | defaults: no current test pins the upstream keys key-for-key. Setter rejection: `crates/oxpinyin-capi/tests/abi/contract.rs`. Construction variants: `crates/oxpinyin-runtime/tests/assembly.rs` (system/user dir combinations incl. degrade-on-file, empty-path) and the capi fixture init. N/A: runtime setter clamping (config is layered files + option bits, not live setters), `chewing_version()` |
| `test-userphrase.c` | add/remove/lookup/enumerate, persistence across contexts, same-reading coexistence, max length, error handling, auto-learn gating (only after commit, learn-excluded) | PORT (store behavior) / ADAPT (learning) | `crates/oxpinyin-user/tests/user_store_semantics.rs` (§ phrase lifecycle: roundtrip, persistence across handles, multi-reading accumulation, 15/16 length ceiling, key-count mismatch, remove-absent). Learning: `crates/oxpinyin-capi/src/e2e_tests.rs` (train entry points, counts, mask-out; `pinyin_train` refuses a selection-less instance) and the store's observe path in `user_store_semantics.rs`. N/A: Shift-Left/Right and Ctrl+Num add-phrase UI gestures (no keystroke surface binds them) |
| `test-symbol.c` / `test-special-symbol.c` / `test-easy-symbol.c` | symbol-table typing, backtick menus, easy-symbol expansion, punctuation collisions | N/A | oxpinyin has no symbol-input surface: the punct table is consulted only by prediction (`pinyin_guess_predicted_candidates_with_punctuations`, covered in `crates/oxpinyin-capi/src/union_e2e_tests.rs`). Easy/special symbols do not exist |
| `test-fullshape.c` | full/half-shape toggle and full-width output | N/A | full-shape is not implemented (oxpinyin carries no `init-full` setting) |
| `test-reset.c` | `Reset` clears session state without dropping static data/config | PORT | `crates/oxpinyin-engine/tests/session_lifecycle.rs` (§ reset): config/options survive, a reset session converts identically to a fresh one |
| `test-error-handling.c` | ~80 NULL-context contracts; fallback dictionary keeps the engine alive on corrupt data | N/A / ADAPT | NULL contracts are C-ABI mechanics the Rust surface cannot express (type safety); the capi layer still guards every entry (`if instance.is_null()`) and the abort-contract behavior of the scheme setters is `crates/oxpinyin-capi/tests/abi/contract.rs`. Fail-fast on missing tables (`OpenError`) replaces the fallback-dictionary tolerance — an explicit divergence (oxpinyin refuses to open rather than degrading) |
| `test-regression.c` | ~15 specific crash/conversion regressions (fuzz-found sequences, cursor/selection interactions) | REPLACE | generic crash resistance: `fuzz/fuzz_targets/parser.rs`, `crates/oxpinyin-engine/tests/session_replay.rs` (`arbitrary_characters_never_panic`), `crates/oxpinyin-core/tests/parser_acceptance.rs` (arbitrary-byte determinism/limits). The specific chewing conversion regressions are N/A: they convert through chewing dictionaries oxpinyin does not ship |
| `test-struct-size.c` | `sizeof` guards on `ChewingConfigData`/`IntervalType` | N/A | the C ABI uses opaque handles; ABI stability is governed by the cargo-c soname contract (W8), not struct layouts |
| `test-logger.c` | custom logger install/disable | N/A | no logger API |
| `performance.c`, `stress.c`, `randkeystroke.c`, `simulate.c`, `genkeystroke.c`, `stresstest.py` | timing, fuzzing, batch scoring, scenario authoring tools | REPLACE | the `bisect --perf` timing driver (`tools/bisection/bisect.c`), the nightly fuzz soak (`verify-nightly.yml`) plus `fuzz/corpus/`, the authored F-D session scenarios in `crates/oxpinyin-engine/tests/session_replay.rs` (the `simulate.c` analog with assertions) |
| `data/` (CHEW dictionaries, swkb/symbols tables, materials.txt) | test dictionaries | N/A | oxpinyin tests run on its own committed fixtures (`fixtures/w3`, `fixtures/w4`, `fixtures/w9`, `fixtures/foundation`) |

## Fuzz harnesses

The `fuzzer/` crate of libchewing holds the two harness shapes with the best
recorded yield for an IME. oxpinyin has both, in the nightly `fuzz/`
workspace (`fuzz/Cargo.toml` lists the targets):

- **Stateful C-API fuzzer** (libchewing `fuzzer.rs`):
  `capi-commands` (`fuzz/fuzz_targets/capi_commands.rs`). Each input byte
  selects one command: parse, guess, candidate walks, setters and iterator
  begin/end pairs.
- **Hostile data-file fuzzer** (libchewing `trieloader.rs`): `dict-loader`,
  `phrase-library` and `table-conf`.

The nightly soak runs every target with cargo-fuzz 0.13.2 and its default
`-s address` instrumentation. On Linux, this includes LeakSanitizer.
`just fuzz <target>` runs one target locally. If an AFL++ lane is added, prefer a version-pinned tool
over an ad-hoc build.

For valid command sequences, the contract tests in
`crates/oxpinyin-capi/tests/abi/contract.rs` hold the postconditions:

- every entry point returns; an abort is a finding, except where the pin
  aborts (`docs/findings/pin-aborts-pinyin-facade.md`);
- a rejected setter returns `false` and leaves the parse behaviour unchanged;
- candidate walks after a failed guess return the earlier list or an empty
  list;
- junk bytes into `pinyin_parse_more_*` give the parsed-length accounting
  (0 when nothing is consumed).

Double-`end` and use-after-`end` are undefined behaviour by contract. The
harness asserts no postconditions on them. A sanitizer report is the
finding.

## Totals

- **libpinyin**: 17 `noinst_PROGRAMS`, six also in automated `TESTS`;
  **17 Ported, 0 GAP** after families A, B, D, C and E1–E4. A family PR updates its rows
  and this total when its drop-in verification passes.
- The libchewing rows keep their separately audited behavior-coverage
  classifications. They do not count toward the libpinyin drop-in total.

## Maintenance rule

Each libpinyin family PR updates its rows from GAP to Ported, names its
source-derived frozen cases and records the selected-cell results. Keep the
17-program inventory tied to the four Makefiles of the pin. Do not replace a
missing executable with an internal-test claim, and do not silently omit it.

For libchewing: a row can say N/A because a surface does not exist. If
that surface ships later, update the behavior-coverage classification of the row and
add the test in the same change.
