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
| libpinyin (`main`) | `55e9051189db5d2f07723edebc5611eb63e52d3c` | 2026-08-19 |
| libchewing (`master`) | `179a02f0629c1137050c736acddc17c9424bf1d2` | 2026-07-17 |

The classification vocabulary:

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

Upstream shape: most top-level programs are interactive stdin *drivers*
with no assertions (they print results for eyeballing); the hard
assertions live in the `storage/` programs. Test data is the installed
`../data` table set; oxpinyin's equivalents run against the committed
`fixtures/w3` mini tables and the w4 portable fixtures.

| Source | Behavior protected | Status | oxpinyin home |
|---|---|---|---|
| `test_pinyin.cpp` | full-pinyin pipeline: parse → prefix guess → candidates (`SORT_BY_PHRASE_LENGTH \| SORT_BY_FREQUENCY`) → auxiliary text at **every** offset `0..=len` → train → reset → save; closes with `mask_out(0,0)` → save | ADAPT | `crates/oxpinyin-capi/tests/abi/pipeline.rs` (aux text at every offset, sentence 你好, candidates, the train/reset/save/mask-out cycle). Candidate ordering under the pinned sort option: `src/e2e_tests.rs` (`predicted_tie_groups…`), `tests/abi/exact_scheme.rs` |
| `test_phrase.cpp` | `pinyin_phrase_segment`: raw Chinese text → phrase tokens | N/A | no oxpinyin surface segments raw Chinese text; the reachable slice of the same risk (text ↔ token reverse lookup) is `crates/oxpinyin-data/tests/search_continuation.rs`. Phrase-level segmentation parity is held by the oracle differentials, not a unit surface |
| `test_zhuyin.cpp` | zhuyin pipeline: `parse_more_chewings` → `guess_sentence` → `get_sentence` → train/reset/save/mask-out | ADAPT | `crates/oxpinyin-capi/tests/abi/pipeline.rs` (`the_zhuyin_pipeline…`): oxpinyin unifies the separate `zhuyin_*` C API onto the same instance surface, so the port drives the chewing parser and asserts a sentence answers |
| `test_chewing.cpp` | the pinyin-context variant of the same pipeline, with chewing auxiliary texts | ADAPT | `crates/oxpinyin-capi/tests/abi/pipeline.rs` (`the_chewing_pipeline…`) |
| `lookup/test_pinyin_lookup.cpp` | `PhoneticLookup::get_nbest_match` over the parsed key matrix (n-best sentence paths) | REPLACE | `crates/pinyin-oracle/tests/graph_paths.rs` + `decode_differential.rs` + `tests/differential_replay.rs` against the frozen `fixtures/w4/oracle-paths.txt` / `oracle-sentence-surface.txt` — the same path enumeration checked against the pin instead of eyeballed |
| `lookup/test_phrase_lookup.cpp` | `PhraseLookup::get_best_match` over raw Chinese text | N/A | same as `test_phrase.cpp`: no raw-text phrase-lookup surface in oxpinyin |
| `storage/test_parser2.cpp` | every parser variant (`fullpinyin`, `doublepinyin`, `zhuyin` × standard/hsu/dachen26, `pinyindirect`, `zhuyindirect`) with the one hard assert: aligned key/key-rest streams | PORT | `crates/oxpinyin-core/tests/scheme_parsers.rs`: aligned spans across all 6 double-pinyin layouts, all 8 zhuyin keyboards (StandardDvorak is the abort slot, covered as a rejection contract), and both romanization indexes; the parse itself is exhaustively covered by `tests/parser_acceptance.rs` + the F-A frozen captures |
| `storage/test_matrix.cpp` | `PhoneticKeyMatrix` fill + resplit/inner-split/fuzzy steps; `search_matrix` over all spans | REPLACE | the matrix steps are oxpinyin's `SegmentGraph` edge construction (`crates/oxpinyin-core/src/graph.rs` unit tests) and its span search is exercised end-to-end by the oracle `graph_paths` differentials above |
| `storage/test_phrase_index.cpp` | `PhraseItem` pronunciation/frequency invariants; phrase-index store/load roundtrip; `add_unigram_frequency`; `compact` | ADAPT | `crates/oxpinyin-data/tests/search_continuation.rs` (text/pronunciation reverse invariants, unknown answers empty, reopen equality). The pronunciation-possibility ratio arithmetic lives at the scoring seam, covered by `crates/oxpinyin-core/tests/scoring.rs` with the fixture doubles. `compact()` is storage-mechanics (redb) — N/A |
| `storage/test_ngram.cpp` | `SingleGram` insert-or-set semantics, totals, bigram store roundtrip, snapshot save/load, `mask_out(0,0)` | PORT | `crates/oxpinyin-user/tests/user_store_semantics.rs` (§ bigram). Divergence: `set_bigram_count` unifies insert/set into one overwrite and the store maintains totals (upstream makes the caller do it) |
| `storage/test_flexible_ngram.cpp` | the training-side flexible single-gram/bigram machinery | ADAPT | the training gram layer is exercised against the pin by the w9 differentials: `crates/oxpinyin-counter/tests/differential.rs`, `crates/oxpinyin-lambda/tests/differential.rs`, `crates/oxpinyin-emitter/tests/{differential,roundtrip}.rs` — behavior proven against `gen_ngram`/`estimate_interpolation`/`export_interpolation` rather than against C++ internals |
| `storage/test_punct_table.cpp` | `PunctTable` append/remove/save/load | ADAPT | read side: `crates/oxpinyin-data/tests/table_loading.rs` (fixture contents, missing-file tolerance mirroring `pinyin_init`). Mutation side: N/A — oxpinyin punctuation tables are compiled system data (`oxpinyin-datagen`), there is no user punct-edit API (upstream divergence to report back) |
| `storage/test_table_info.cpp` | `SystemTableInfo2` load, λ, per-index table infos; `UserTableInfo` conform | ADAPT | λ pinned by `crates/oxpinyin-data/src/table_conf.rs` (`PINNED_LAMBDA`) and `lm/tests.rs` (`default_lambda_is_the_pinned_config_value`); table manifests by `oxpinyin-datagen` (`tests/fixtures_identity.rs`). `UserTableInfo` conform is N/A: the user store carried a format version from day one, so there is nothing to conform |
| `storage/test_phrase_index_logger.cpp` | training logger diff/merge of phrase indexes | N/A | the training pipeline reproduces the pin's outputs end-to-end (`crates/oxpinyin-corpus/tests/differential.rs`: identical `interpolation2.text`), which subsumes the logger's role; the user-side count overlay (`unigram_delta`, `mask_out`) is covered in `user_store_semantics.rs` |
| `include/`, `tests_helper.h`, `timer.h` | harness infrastructure (memory-chunk loaders, timing) | N/A | cargo test is the harness; timing moved to the criterion benches (`crates/oxpinyin-capi/benches`, `crates/pinyin-oracle/benches`) |

## libchewing tests

libchewing is a Zhuyin/Bopomofo IME with its own dictionary formats,
symbol tables, and C API conventions. oxpinyin shares the *behavior
class* (an IME session: keys → preedit/candidates/commit, user phrases,
config) but not the data or ABI. Tests that protect chewing-only UI or
ABI behaviors are N/A by design rather than artificially reconstructed.

| Source | Behavior protected | Status | oxpinyin home |
|---|---|---|---|
| `test-bopomofo.c` | ~60 session behaviors: candidate selection (forward/rearward/mid-composition/paging), Esc/Backspace/Del semantics, mode switches, ShiftSpace full-shape, Numlock, auto-commit thresholds, interval enumeration, per-keyboard conversions (HSU/ET/ET26/CP26/GinYieh/IBM/Dvorak/Colemak), pinyin keyboards (HANYU/THL/MPS2), fuzzy search, `phone_to_bopomofo` | ADAPT (selection + keys + keyboards), N/A (rest) | Selection: `crates/oxpinyin-engine/tests/session_lifecycle.rs` (choose/commit/out-of-range/mid-composition) and `tests/decoding.rs` (choose advances composition, bigram feedback, apostrophe boundaries). Key semantics: `tests/decoding.rs` + `tests/session_replay.rs` (Enter/Escape/Backspace outcomes, determinism). Keyboards: `crates/oxpinyin-core/tests/scheme_parsers.rs` (8 zhuyin keyboards parse with pinned dachen expectations; HSU discreteness; CP26 exists as a distinct keyboard). N/A with reasons: rearward choice mode (not implemented), full-shape, mode-switch keys, Numlock handling, auto-commit thresholds, interval enumeration (no such surface), THL/MPS2 romanizations (not implemented), static-buffer aliasing (Rust `String`), 4-byte UTF-8 selection (covered generically by the engine's property tests) |
| `test-keyboard.c` | set/get round-trip over 17 keyboard layouts, invalid rejection, `KBStr2Num`, enumeration | ADAPT | layout switching + rejection contracts: `crates/oxpinyin-capi/tests/abi/contract.rs` (every implemented scheme value sticks; out-of-enum and abort slots rejected without state damage — the #109 contract-lock) and `crates/oxpinyin-core/tests/scheme_parsers.rs` (`customized`/`StandardDvorak` slots). N/A: string-based keyboard names and enumeration (`KBStr2Num`, `kbtype_Enumerate`) — no string-keyboard API exists |
| `test-keyboardless.c` | programmatic API: `cand_open/close/choose_by_index` incl. out-of-range and not-in-select, `cand_list_*` traversal, `commit_preedit_buf`/`clean_preedit_buf`/`clean_bopomofo_buf` | ADAPT | `crates/oxpinyin-engine/tests/session_lifecycle.rs` (choose without composition fails, out-of-range fails without state damage, choice commits the exact phrase, offset windows via `candidates_at`/`select_anchored`). N/A: list traversal (`cand_list_next/prev/first/last`) and the three explicit buffer operations — oxpinyin's composition model has no equivalent operations to bind |
| `test-config.c` | option existence, defaults, setter validation (booleans 0/1 only, clamped ranges), syspath/userpath construction variants, version reporting | ADAPT | defaults: `crates/oxpinyin-engine/tests/upstream_defaults.rs` pins all 69 upstream keys key-for-key. Setter rejection: `crates/oxpinyin-capi/tests/abi/contract.rs`. Construction variants: `crates/oxpinyin-runtime/tests/assembly.rs` (system/user dir combinations incl. degrade-on-file, empty-path) and the capi fixture init. N/A: runtime setter clamping (config is layered files + option bits, not live setters), `chewing_version()` |
| `test-userphrase.c` | add/remove/lookup/enumerate, persistence across contexts, same-reading coexistence, max length, error handling, auto-learn gating (only after commit, learn-excluded) | PORT (store behavior) / ADAPT (learning) | `crates/oxpinyin-user/tests/user_store_semantics.rs` (§ phrase lifecycle: roundtrip, persistence across handles, multi-reading accumulation, 15/16 length ceiling, key-count mismatch, remove-absent). Learning: `crates/oxpinyin-capi/src/e2e_tests.rs` (train entry points, counts, mask-out; `pinyin_train` refuses a selection-less instance) and the store's observe path in `user_store_semantics.rs`. N/A: Shift-Left/Right and Ctrl+Num add-phrase UI gestures (no keystroke surface binds them) |
| `test-symbol.c` / `test-special-symbol.c` / `test-easy-symbol.c` | symbol-table typing, backtick menus, easy-symbol expansion, punctuation collisions | N/A | oxpinyin has no symbol-input surface: the punct table is consulted only by prediction (`pinyin_guess_predicted_candidates_with_punctuations`, covered in `crates/oxpinyin-capi/src/union_e2e_tests.rs`). Easy/special symbols do not exist |
| `test-fullshape.c` | full/half-shape toggle and full-width output | N/A | full-shape is not implemented (the `init-full` schema keys are captured defaults with no reader) |
| `test-reset.c` | `Reset` clears session state without dropping static data/config | PORT | `crates/oxpinyin-engine/tests/session_lifecycle.rs` (§ reset): config/options survive, a reset session converts identically to a fresh one |
| `test-error-handling.c` | ~80 NULL-context contracts; fallback dictionary keeps the engine alive on corrupt data | N/A / ADAPT | NULL contracts are C-ABI mechanics the Rust surface cannot express (type safety); the capi layer still guards every entry (`if instance.is_null()`) and the abort-contract behavior of the scheme setters is `tests/abi/contract.rs`. Fail-fast on missing tables (`OpenError`) replaces the fallback-dictionary tolerance — an explicit divergence (oxpinyin refuses to open rather than degrading) |
| `test-regression.c` | ~15 specific crash/conversion regressions (fuzz-found sequences, cursor/selection interactions) | REPLACE | generic crash resistance: `fuzz/fuzz_targets/parser.rs`, `crates/oxpinyin-engine/tests/session_replay.rs` (`arbitrary_characters_never_panic`), `crates/oxpinyin-core/tests/parser_acceptance.rs` (arbitrary-byte determinism/limits). The specific chewing conversion regressions are N/A: they convert through chewing dictionaries oxpinyin does not ship |
| `test-struct-size.c` | `sizeof` guards on `ChewingConfigData`/`IntervalType` | N/A | the C ABI uses opaque handles; ABI stability is governed by the cargo-c soname contract (W8), not struct layouts |
| `test-logger.c` | custom logger install/disable | N/A | no logger API |
| `performance.c`, `stress.c`, `randkeystroke.c`, `simulate.c`, `genkeystroke.c`, `stresstest.py` | timing, fuzzing, batch scoring, scenario authoring tools | REPLACE | criterion benches (`crates/oxpinyin-capi/benches/stage2.rs`, `crates/pinyin-oracle/benches/*`), the bounded fuzz run in CI plus `fuzz/corpus/`, recorded session scenarios (`fixtures/w4/f-d-session.txt` replayed by `session_replay.rs` — the `simulate.c` analog with assertions) |
| `data/` (CHEW dictionaries, swkb/symbols tables, materials.txt) | test dictionaries | N/A | oxpinyin tests run on its own committed fixtures (`fixtures/w3`, `fixtures/w4`, `fixtures/w9`, `fixtures/foundation`) |

## Totals

Counts below are `cargo test --workspace` executed totals on this
repository's default feature set (the same numbers CI produces), not
static `#[test]` greps — the two differ because the oracle-ffi tier, the
lmdb/tkrzw store-backend copies, and the live-oracle `#[ignore]`d tests
are feature-gated out of the default build.

- **libpinyin**: 14 source programs → 2 PORT, 6 ADAPT, 2 REPLACE, 4 N/A.
- **libchewing**: 15 test programs + 6 tools → 1 PORT, 6 ADAPT, 2 REPLACE,
  and the remainder N/A (chewing-specific symbol/full-shape/ABI/Logger/UI
  surfaces oxpinyin does not implement — each listed above with its
  reason).
- New behavioral tests written for this ledger: 43
  (`user_store_semantics` 11, `scheme_parsers` 11, `search_continuation`
  8, `session_lifecycle` 8, `pipeline` 5), all green under
  `cargo test --workspace`.

## Maintenance rule

When a test here says N/A because a surface does not exist, and that
surface later ships, move the entry to ADAPT/PORT and add the test in the
same change. The ledger travels with the test architecture; it is the
answer to "how much of the upstream suites have we actually ported?"
