# Upstream test coverage ledger

Status: living document

This ledger covers every test program in upstream
[libpinyin](https://github.com/libpinyin/libpinyin)/`tests` and
[chewing/libchewing](https://github.com/chewing/libchewing)/`tests`.
For each program, it records the behavior that the program protects and where that behavior lives in oxpinyin.

**Audited upstream revisions.** Every entry below describes these
revisions. When either revision moves, audit the delta again:

| Project | Revision |
|---|---|
| libpinyin (pin) | `074a2219c90feaf962d0d24f034514033ece5f99` |
| libchewing (`master`) | `179a02f0629c1137050c736acddc17c9424bf1d2` |

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

The rule: **no upstream test is silently omitted.** Every N/A
states why.

---

## libpinyin tests

The upstream test programs are not drop-in tools.
The **seven fixed programs are product tests**, and the **ten interactive programs are modes
of one nonshipping `oxpinyin-repl` binary**. The 21 genuine utilities keep
their drop-in interfaces. No C API, persistent format or product refusal
is waived by this.

The inventory is tied to `tests/Makefile.am:37-59`,
`tests/storage/Makefile.am:31-46`, `tests/include/Makefile.am:25-27` and
`tests/lookup/Makefile.am:31-32` at `074a2219`: 17 `noinst_PROGRAMS`, six
also in Automake `TESTS`. `test_zhuyin` is conditional on libzhuyin upstream.

### Fixed programs

These tests call the owning Rust product code. Their pin citations are
beside the assertions.

| Upstream program | Retained product witness |
| --- | --- |
| `test_ngram` | `core::single_gram::row_edits_keep_the_stored_total_and_half_open_order`; `store::a_native_hash_snapshot_keeps_both_predecessor_rows`; existing `user::store::mask_out_all_marks_store_empty_again` |
| `test_flexible_ngram` | `store::flexible_ngram::search_uses_the_pin_test_program_ranges_and_preserves_append_order`; `store/tests/flexible_bigram.rs` round-trip, native-order, empty-file and occupied-directory retry cases |
| `test_punct_table` | `utils::storage::punctuation::punctuation_keeps_its_order_in_a_snapshot_and_the_empty_row_is_readable`: real append, native snapshot/reopen in append order, failed attach/save and a readable zero-byte row. The three `remove_punctuation` calls (`test_punct_table.cpp:59`, `:68`, `:72`) are pin assertions with no product counterpart: `oxpinyin-data`'s `punct.rs` has no remove operation |
| `test_table_info` | `data::table_info::addon_replacement_and_null_columns_keep_their_indexed_types`, existing slot/header/abort/missing-file tests; `data::user_files::a_fresh_zero_version_marker_requires_conformance_for_either_source_format` and existing save/reopen/conformance test |
| `test_memory_chunk` | `data::user_files::framed_chunk_replaces_an_old_file_and_reads_through_heap_and_map`: surviving words 10 and 12, framed replacement, heap/mapped read and directory refusal |
| `test_phrase_index` | `user::replay::a_heap_loaded_item_keeps_reading_order_when_training_matches_the_first`, with the 0.75 and 0.5 possibilities (`test_phrase_index.cpp:54`, `:62`, `:70`) read through `PhraseLibraries::pronunciation_possibility`; `utils::storage::gen_binary_files::phrase_item_tests` through the utility's `load_phrase_index`: long item, same-token readings, actual unigram change and wrong-length skip |
| `test_phrase_index_logger` | `utils::storage::libraries::unigram_generation_counts_holes_and_refused_adds_in_the_facade_only`; `user::replay::sparse_unigram_edits_emit_only_changed_items_and_replay_the_native_journal`; `data::phrase_libraries::a_failed_layout_keeps_its_local_total_out_of_the_facade_total` |

The simulated C++ buffer's capacity, resize/insert/remove operations have
no product counterpart. Fixed temporary names, progress formatting,
ignored arguments/input, benchmark repetition counts and continuing after a
driver-only save failure are harness behavior, not covered. Appended punctuation
state, file refusal, partial writes and journal replay are product
assertions.

Punctuation removal (`punct_table.cpp:117-145`, `:169-177`) is library code with no
oxpinyin caller, so no test writes a post-removal row.

The REPL-only refusal of invalid text for a new phrase item is not a
product witness: only `oxpinyin-repl` passes `refuse_invalid`, and the
`gen_binary_files` utility does not. Its three EINVAL paths are held by
`oxpinyin-repl`'s `tests::invalid_text_for_a_new_item_is_refused_at_the_table_reader`.

### Interactive modes

`just repl <mode> [--backend bdb|kyotocabinet]` runs on Linux or macOS: the
REPL needs GLib, as `oxpinyin-utils` does, and is not verified on Windows.
The launcher copies
`fixtures/w3/<backend>` into a disposable directory and passes it explicitly.
The two text-table modes also stage the authored `fixtures/datagen-toned`
text inputs. No command writes back to the source fixtures. The parser mode
accepts its parser/scheme options, for example
`just repl parser2 -- --parser zhuyin --scheme hsu`.

| Upstream program | Mode | Retained mechanism |
| --- | --- | --- |
| `test_pinyin` | `pinyin` | Pinyin C parse/prefix/candidate/auxiliary sequence, training/reset/save and mask teardown |
| `test_phrase` | `phrase` | C phrase segmentation, token text and saved-profile effects |
| `test_chewing` | `chewing` | Pinyin ABI keyboard parsing, sentence and auxiliary cursors |
| `test_zhuyin` | `zhuyin` | Separate Zhuyin parse/guess/train/reset/save and profile effects |
| `test_parser2` | `parser2` | Full/double/keyboard/direct parser results, scheme differences, exact consumed/key-rest spans and final unterminated input |
| `test_phrase_table` | `phrase_table` | Native text-table lookup, empty-row continuation, Unicode refusal and preceding results |
| `test_chewing_table` | `chewing_table` | Native ranges/order, zero-token cursor, incomplete-tone abort before overlong refusal |
| `test_matrix` | `matrix` | Physical columns, separator/terminal zero keys and search continuation |
| `test_pinyin_lookup` | `pinyin_lookup` | Real-table/model n-best results, original byte-length bound, ordered tokens and text |
| `test_phrase_lookup` | `phrase_lookup` | Real phrase/model segmentation, invalid conversion and required chunk errors |

`crates/oxpinyin-repl/tests/interactive.rs` holds the frozen
product outputs and complete file-effect assertions for these modes.
Only timing lines and the temporary data path spelling are
normalized; the frozen expected bytes are not.
`table_guards.rs` preserves prior successful output before a later EINVAL,
and the two table state-machine tests sit beside their code.
Fixed-path missing-config cases and the upstream executable's help text
are harness-specific; the data layer's missing-file/header tests cover the data side.

A1 row 30 has a real-model witness in
`runtime/tests/tool_lookup.rs::embedded_nul_lookup_keeps_the_original_terminal_column`.
Both `ni\0hao` and `ni\0'hao` preserve their six/seven-byte bounds and ni key
span, then return the three frozen ordered W3 tokens/texts through actual
`SystemTables`, `BigramTables` and the existing decoder. `ni'\0hao` separates
the key span's end (2) from the parsed length (3): each row spans the parsed
length, and the rows need the physical separator column.
The parser has a separate two-option-word NUL-span assertion.

The safe C ownership adapters stay in the ABI crates and call the owning
ABI modules directly. The REPL has no benchmark timer or repeated decode loop.

## libchewing tests

libchewing is a Zhuyin/Bopomofo IME with its own dictionary formats,
symbol tables and C API conventions. oxpinyin shares the *behavior
class* (an IME session: keys → preedit/candidates/commit, user phrases,
config), but not the data or the ABI. Tests that protect chewing-only UI or
ABI behaviors are N/A by design; they are not artificially reconstructed.

| Source | Behavior protected | Status | oxpinyin home |
|---|---|---|---|
| `test-bopomofo.c` | ~60 session behaviors: candidate selection (forward/rearward/mid-composition/paging), Esc/Backspace/Del semantics, mode switches, ShiftSpace full-shape, Numlock, auto-commit thresholds, interval enumeration, per-keyboard conversions (HSU/ET/ET26/CP26/GinYieh/IBM/Dvorak/Colemak), pinyin keyboards (HANYU/THL/MPS2), fuzzy search, `phone_to_bopomofo` | ADAPT (selection + keyboards), N/A (rest) | Selection: `crates/oxpinyin-engine/tests/session_lifecycle.rs` (choose/commit/mid-composition) and `crates/oxpinyin-engine/tests/decoding.rs` (choose advances composition, bigram feedback, apostrophe boundaries). Determinism: `crates/oxpinyin-engine/tests/decoding.rs` + `crates/oxpinyin-engine/tests/session_replay.rs`; Enter/Escape/Backspace are N/A (the engine has no key layer). Keyboards: `crates/oxpinyin-core/tests/scheme_parsers.rs` (8 zhuyin keyboards parse with pinned dachen expectations; HSU discreteness; CP26 exists as a distinct keyboard). N/A with reasons: rearward choice mode (not implemented), full-shape, mode-switch keys, Numlock handling, auto-commit thresholds, interval enumeration (no such surface), THL/MPS2 romanizations (not implemented), static-buffer aliasing (Rust `String`), 4-byte UTF-8 selection (covered generically by the engine's property tests) |
| `test-keyboard.c` | set/get round-trip over 17 keyboard layouts, invalid rejection, `KBStr2Num`, enumeration | ADAPT | layout switching + rejection contracts: `crates/oxpinyin-capi/tests/abi/contract.rs` (every implemented scheme value sticks; out-of-enum and abort slots rejected without state damage) and `crates/oxpinyin-core/tests/scheme_parsers.rs` (`customized`/`StandardDvorak` slots). N/A: string-based keyboard names and enumeration (`KBStr2Num`, `kbtype_Enumerate`) — no string-keyboard API exists |
| `test-keyboardless.c` | programmatic API: `cand_open/close/choose_by_index` incl. out-of-range and not-in-select, `cand_list_*` traversal, `commit_preedit_buf`/`clean_preedit_buf`/`clean_bopomofo_buf` | ADAPT | `crates/oxpinyin-engine/tests/session_lifecycle.rs` (choice commits the exact phrase, offset windows via `candidates_at`/`select_target`); a choose of a candidate the regenerated list no longer holds is refused without state damage: `a_stale_candidate_pointer_after_regeneration_is_refused` in `crates/oxpinyin-capi/src/e2e_tests.rs` and `crates/oxpinyin-zhuyin-capi/src/e2e_tests.rs`. The index-based not-in-select and out-of-range ports were retired with `Session::select`: a choose names a candidate pointer, not an index. N/A: list traversal (`cand_list_next/prev/first/last`) and the three explicit buffer operations — oxpinyin's composition model has no equivalent operations to bind |
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

The other targets feed hostile bytes to the parsers below the ABI: `parser`
(full pinyin), `double-pinyin` and `chewing-parser` (the eight parseable Zhuyin
keyboards). `user-store-ops` drives arbitrary add, remove, train and mask-out
interleavings over a fresh user directory.

The nightly soak runs every target with cargo-fuzz 0.13.2 and its default
`-s address` instrumentation. On Linux, this includes LeakSanitizer.
`just fuzz <target>` runs one target locally. If an AFL++ lane is added, prefer a version-pinned tool
over an ad-hoc build.

For valid command sequences, the contract tests in
`crates/oxpinyin-capi/tests/abi/contract.rs` hold the postconditions:

- every entry point returns; an abort is a finding, except where the pin
  aborts (`docs/findings/upstream-divergences.md#pin-aborts-reproduced`);
- a rejected setter returns `false` and leaves the parse behaviour unchanged;
- candidate walks after a failed guess return the earlier list or an empty
  list;
- junk bytes into `pinyin_parse_more_*` give the parsed-length accounting
  (0 when nothing is consumed).

Double-`end` and use-after-`end` are undefined behaviour by contract. The
harness asserts no postconditions on them. A sanitizer report is the
finding.

## Maintenance rule

All 17 libpinyin programs are accounted for above.
Each program's distinct product mechanism stays in its owning tests before a
witness is removed. This ledger is updated when a mechanism moves; a lower test
count or an inaccessible reference alone is not evidence of redundancy.
The 21 real utility interfaces remain governed by their frozen cases.

For libchewing: a row can say N/A because a surface does not exist. If
that surface ships later, the behavior-coverage classification of the row is updated and
the test is added in the same change.
