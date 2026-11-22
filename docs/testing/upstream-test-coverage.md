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
| `test_ngram` | `core::single_gram::row_edits_keep_the_stored_total_and_half_open_order`; `store::a_native_hash_snapshot_keeps_both_predecessor_rows`; existing `user/src/store/tests.rs` (`backend_files`) `mask_out_zero_clears_every_recorded_item` |
| `test_flexible_ngram` | `store::flexible_ngram::search_uses_the_pin_test_program_ranges_and_preserves_append_order`; `store/src/flexible_ngram/tests.rs` round-trip, native-order, empty-file and occupied-directory retry cases |
| `test_punct_table` | `utils::storage::punctuation::punctuation_keeps_its_order_in_a_snapshot_and_the_empty_row_is_readable`: real append, native snapshot/reopen in append order, failed attach/save and a readable zero-byte row. The three `remove_punctuation` calls (`test_punct_table.cpp:59`, `:68`, `:72`) are pin assertions with no product counterpart: `oxpinyin`'s `punct.rs` has no remove operation |
| `test_table_info` | `data::table_info::addon_replacement_and_null_columns_keep_their_indexed_types`, existing slot/header/abort/missing-file tests; `data::user_files::a_fresh_zero_version_marker_requires_conformance_for_either_source_format` and existing save/reopen/conformance test |
| `test_memory_chunk` | `input::chunk::framed_chunk_replaces_an_old_file_and_reads_through_heap_and_map`: surviving words 10 and 12, framed replacement, file and mapped read and directory refusal |
| `test_phrase_index` | `user::replay::a_heap_loaded_item_keeps_reading_order_when_training_matches_the_first`, with the 0.75 and 0.5 possibilities (`test_phrase_index.cpp:54`, `:62`, `:70`) read through `PhraseLibraries::pronunciation_possibility`; `utils::storage::gen_binary_files::tests` through the utility's `load_phrase_index`: long item, same-token readings, actual unigram change and wrong-length skip |
| `test_phrase_index_logger` | `utils::storage::libraries::unigram_generation_counts_holes_and_refused_adds_in_the_facade_only`; `user::replay::sparse_unigram_edits_emit_only_changed_items_and_replay_the_native_journal`; `data::phrase_libraries::a_failed_layout_keeps_its_local_total_out_of_the_facade_total` |

The simulated C++ buffer's capacity, resize/insert/remove operations have
no product counterpart. Fixed temporary names, progress formatting,
ignored arguments/input, benchmark repetition counts and continuing after a
driver-only save failure are harness behavior, not covered. Appended punctuation
state, file refusal, partial writes and journal replay are product
assertions.

Punctuation removal (`punct_table.cpp:117-145`, `:169-177`) is library code with no
oxpinyin caller, so no test writes a post-removal row.

The REPL selects strict text loading. In `oxpinyin-utils`,
`storage::gen_binary_files::tests::invalid_text_for_a_new_item_is_refused_at_the_table_reader`
checks the three EINVAL paths in `read_phrase_items`.
`storage::gen_binary_files::tests::phrase_table_invalid_text` checks the
refusal in `load_phrase_table`.

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
| `test_parser2` | `parser2` | Full/double/keyboard/direct parser dispatch, scheme differences, exact consumed/key-rest spans |
| `test_phrase_table` | `phrase_table` | Native text-table lookup, empty-row continuation, Unicode refusal and preceding results |
| `test_chewing_table` | `chewing_table` | Native ranges/order, zero-token cursor, incomplete-tone abort before overlong refusal |
| `test_matrix` | `matrix` | Physical columns, separator/terminal zero keys and search continuation |
| `test_pinyin_lookup` | `pinyin_lookup` | Real-table/model n-best results, original byte-length bound, ordered tokens and text |
| `test_phrase_lookup` | `phrase_lookup` | Real phrase/model segmentation and invalid conversion |

`repl/tests/it` checks the frozen product outputs and
complete file effects for these modes.
The comparisons account for timing lines, temporary data paths,
and volatile GLib diagnostic prefixes.
The matrix module checks the shared lookup loader's required chunk errors.
`table_guards.rs` checks exact prior output and the input tree after a later EINVAL.
The two table state-machine tests live in `repl/src/tables/tests.rs`.
The input module's `getline_keeps_the_pins_lines_and_its_last_line` checks the
unterminated final line and the C-string view of embedded NULs.
Fixed-path missing-config cases and the upstream executable's help text
are harness-specific; the data layer's missing-file/header tests cover the data side.

A1 row 30 has a real-model witness in
`runtime/src/tool/tests.rs::embedded_nul_lookup_keeps_the_original_terminal_column`.
Both `ni\0hao` and `ni\0'hao` preserve their six/seven-byte bounds and ni key
span, then return the three frozen ordered mini tokens/texts through actual
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
| `test-bopomofo.c` | ~60 session behaviors: candidate selection (forward/rearward/mid-composition/paging), Esc/Backspace/Del semantics, mode switches, ShiftSpace full-shape, Numlock, auto-commit thresholds, interval enumeration, per-keyboard conversions (HSU/ET/ET26/CP26/GinYieh/IBM/Dvorak/Colemak), pinyin keyboards (HANYU/THL/MPS2), fuzzy search, `phone_to_bopomofo` | ADAPT (selection + keyboards), N/A (rest) | Selection: `src/engine/session/selection/tests.rs` (`session_lifecycle`: choose/commit/mid-composition) and `src/engine/session/lookup/tests.rs` (choose advances composition, bigram feedback, apostrophe boundaries). Determinism: `src/engine/session/lookup/tests.rs` + `src/engine/session/selection/tests.rs` (`session_replay`); Enter/Escape/Backspace are N/A (the engine has no key layer). Keyboards: `src/scheme/tests.rs` (8 zhuyin keyboards parse with pinned dachen expectations; HSU discreteness; CP26 exists as a distinct keyboard). N/A with reasons: rearward choice mode (not implemented), full-shape, mode-switch keys, Numlock handling, auto-commit thresholds, interval enumeration (no such surface), THL/MPS2 romanizations (not implemented), static-buffer aliasing (Rust `String`), 4-byte UTF-8 selection (covered generically by the engine's property tests) |
| `test-keyboard.c` | set/get round-trip over 17 keyboard layouts, invalid rejection, `KBStr2Num`, enumeration | ADAPT | layout switching + rejection contracts: `capi/tests/it/contract.rs` (every implemented scheme value sticks; out-of-enum and abort slots rejected without state damage) and `src/scheme/tests.rs` (`customized`/`StandardDvorak` slots). N/A: string-based keyboard names and enumeration (`KBStr2Num`, `kbtype_Enumerate`) — no string-keyboard API exists |
| `test-keyboardless.c` | programmatic API: `cand_open/close/choose_by_index` incl. out-of-range and not-in-select, `cand_list_*` traversal, `commit_preedit_buf`/`clean_preedit_buf`/`clean_bopomofo_buf` | ADAPT | `src/engine/session/selection/tests.rs` (`session_lifecycle`: choice commits the exact phrase, offset windows via `candidates_at`/`select_target`); a candidate pointer that is no row of a live published list is a caller precondition, as at the pin ([register entry](../findings/upstream-divergences.md#a-candidate-pointer-that-is-not-a-row-of-a-live-published-list)); a row of another live instance selects on the receiving instance (`foreign_candidate_tests` in both C crates). N/A: list traversal (`cand_list_next/prev/first/last`) and the three explicit buffer operations — oxpinyin's composition model has no equivalent operations to bind |
| `test-config.c` | option existence, defaults, setter validation (booleans 0/1 only, clamped ranges), syspath/userpath construction variants, version reporting | ADAPT | defaults: no current test pins the upstream keys key-for-key. Setter rejection: `capi/tests/it/contract.rs`. Construction variants: `src/runtime/tests.rs` (`backend_files`: system/user dir combinations incl. degrade-on-file, empty-path) and the capi fixture init. N/A: runtime setter clamping (config is layered files + option bits, not live setters), `chewing_version()` |
| `test-userphrase.c` | add/remove/lookup/enumerate, persistence across contexts, same-reading coexistence, max length, error handling, auto-learn gating (only after commit, learn-excluded) | PORT (store behavior) / ADAPT (learning) | `src/user/store/tests.rs` (`backend_files`, § phrase lifecycle: roundtrip, persistence across handles, multi-reading accumulation, 15/16 length ceiling); key-count mismatch and remove-absent: `invalid_phrase_is_rejected_without_allocation` and `remove_user_phrase_deletes_everywhere_and_rejects_others` in `src/user/store.rs`. Learning: `capi/tests/it/training.rs` and `capi/tests/it/user_store.rs` (train entry points, counts, mask-out), `src/engine/session/tests.rs` (training refuses without sentence results) and the store's observe path in `src/user/store.rs` (`count_delta_reads_committed_writes`). N/A: Shift-Left/Right and Ctrl+Num add-phrase UI gestures (no keystroke surface binds them) |
| `test-symbol.c` / `test-special-symbol.c` / `test-easy-symbol.c` | symbol-table typing, backtick menus, easy-symbol expansion, punctuation collisions | N/A | oxpinyin has no symbol-input surface: the punct table is consulted only by prediction (`pinyin_guess_predicted_candidates_with_punctuations`, covered in `capi/tests/it/guess_predict.rs`). Easy/special symbols do not exist |
| `test-fullshape.c` | full/half-shape toggle and full-width output | N/A | full-shape is not implemented (oxpinyin carries no `init-full` setting) |
| `test-reset.c` | `Reset` clears session state without dropping static data/config | PORT | `src/engine/session/tests.rs` (`session_lifecycle`): config/options survive, a reset session converts identically to a fresh one |
| `test-error-handling.c` | ~80 NULL-context contracts; fallback dictionary keeps the engine alive on corrupt data | N/A / ADAPT | NULL contracts are C-ABI mechanics the Rust surface cannot express (type safety); the capi layer still guards every entry (`if instance.is_null()`) and the abort-contract behavior of the scheme setters is `capi/tests/it/contract.rs`. Fail-fast on missing tables (`OpenError`) replaces the fallback-dictionary tolerance — an explicit divergence (oxpinyin refuses to open rather than degrading) |
| `test-regression.c` | ~15 specific crash/conversion regressions (fuzz-found sequences, cursor/selection interactions) | REPLACE | generic crash resistance: `src/engine/session/tests.rs` (`session_replay::arbitrary_characters_never_panic`). The specific chewing conversion regressions are N/A: they convert through chewing dictionaries oxpinyin does not ship |
| `test-struct-size.c` | `sizeof` guards on `ChewingConfigData`/`IntervalType` | N/A | the C ABI uses opaque handles; ABI stability is governed by the cargo-c soname contract, not struct layouts |
| `test-logger.c` | custom logger install/disable | N/A | no logger API |
| `performance.c`, `stress.c`, `randkeystroke.c`, `simulate.c`, `genkeystroke.c`, `stresstest.py` | timing, fuzzing, batch scoring, scenario authoring tools | REPLACE | the `bisect --perf` timing driver (`tools/bisection/bisect.c`), the nightly fuzz soak (`verify-nightly.yml`) plus `fuzz/corpus/`, the authored F-D session scenarios in `src/engine/session/selection/tests.rs` (`session_replay`; the `simulate.c` analog with assertions) |
| `data/` (CHEW dictionaries, swkb/symbols tables, materials.txt) | test dictionaries | N/A | oxpinyin tests run on its own committed fixtures (`fixtures/w3`, `fixtures/w4`, `fixtures/w9`, `fixtures/foundation`) |

## Fuzz harnesses

The `fuzzer/` crate of libchewing holds the two harness shapes with the best
recorded yield for an IME. oxpinyin has both, in the nightly `fuzz/`
workspace (`fuzz/Cargo.toml` lists the targets):

- **Stateful C-API fuzzer** (libchewing `fuzzer.rs`):
  `capi-commands` (`fuzz/fuzz_targets/capi_commands.rs`). Each command is
  one selector byte and its own arguments: parse, guess, a candidate guess
  with a choose, setters and iterator begin/end pairs. Each input runs on a
  new context and a new user directory.
- **Hostile data-file fuzzer** (libchewing `trieloader.rs`): `dict-loader`,
  `phrase-library` and `table-conf`.

The other targets feed hostile bytes to the parsers below the ABI:
`double-pinyin` and `chewing-parser` (the eight parseable Zhuyin
keyboards). `user-store-ops` drives arbitrary add, remove, train and mask-out
interleavings over a fresh user directory. Its phrases take any Unicode
scalar and any of the 375 syllables per character, its counts reach past
`u32::MAX`, and it checks each bigram write against the store's documented
contract: the exact totals law where no field saturates or wraps, the cap
and the refusals where one does (the target's "Bigram rows").

### Engine

The targets run under AFL++, as libchewing's fuzzer does, in persistent
mode. Each target is an ordinary executable. Its `main` builds the state
that every input shares, then calls `oxpinyin_fuzz::fuzz`
(`fuzz/src/lib.rs`) with its body. With the fuzz workspace's `afl` feature,
which the recipes select, the `afl` crate takes the body: the forkserver
forks, and each child runs up to its loop count of inputs, which AFL++
delivers in shared memory. Each target states its count (`LOOP_COUNT`): 1000
for the parsers and the file readers, and 1 for `capi-commands` and
`user-store-ops`. `AFL_FUZZER_LOOPCOUNT` overrides it; `tools/dev/fuzz.py`
does not set it, so a standalone replay with no environment takes the same
path as a fuzzing run. In a persistent child, the same input of those two covered
different edges depending on the input before it, although each input opens
a new session and user directory. With a count of 1, a child runs one input
and exits, as a process per input does. With a count above 1, the body first
runs on the empty input and on the target's warm-up inputs (committed
fixtures and command streams), so state that the code builds on first use,
such as `memmap2`'s cached page size, exists before the forkserver starts.
The first input of a child enters the crate's loop from
another call than the later inputs, so the loop's own edges would differ:
the recipe leaves that loop uninstrumented (`AFL_LLVM_DENYLIST`,
`fuzz/instrument-denylist.txt`), and the body, a separate function, stays
instrumented. The fuzz package's build script (`fuzz/build.rs`) reruns when
that file changes, so a change to it rebuilds the targets. An ordinary build leaves the feature off and links no
AFL++ runtime. Either build reads one input from standard input when afl-fuzz
does not run it, so an input reproduces by running the executable with that
input on standard input.

- **Tooling.** `tools/dev/fuzz.py` holds the fuzz logic, and `just fuzz-setup`
  and `just fuzz` are one-line recipes that call it
  (`python -m tools.dev.fuzz setup` and `run`); `just fuzz-minimize` calls
  `minimize`, and the nightly calls `archive`. Its tests,
  `tools/dev/fuzz.test.py`, check the option refusals, the corpus rules and
  the finding count without a fuzzer build.
- **Build.** `just fuzz <target>` builds the target with cargo-afl,
  AddressSanitizer and the AFL++ LLVM plugins (`afl-llvm-dict2file`,
  `split-switches`, `SanitizerCoveragePCGUARD` and `afl-llvm-ijon`). CmpLog
  is off by default. `FUZZ_CMPLOG=1` adds it: a second binary with the CmpLog
  passes, run through `afl-fuzz -c`. Without the plugins the build fails; it
  never falls back to plain sancov instrumentation. `cargo afl build` adds no
  sanitizer: `just fuzz` sets `RUSTFLAGS=-Zsanitizer=address`, so rustc
  instruments the crates and links its ASan runtime into the executable, and
  cargo-afl appends its own flags. `--target` keeps build scripts and proc
  macros out of the sanitizer. Cargo prefers `CARGO_ENCODED_RUSTFLAGS` to
  `RUSTFLAGS`, so the recipe removes an inherited one from both builds. The
  build selects the `afl` feature. The build and the launch take the executable
  from one place: cargo's target directory as `cargo metadata` reports it
  (so `CARGO_TARGET_DIR` and `build.target-dir` apply), passed to the build
  as `--target-dir`, with cargo's layout `<host>/release/<target>` below it.
  The CmpLog binary is built and found the same way, under `cmplog/`.
- **Crashes.** The fuzz workspace's release profile sets `panic = "abort"`, so
  a panic, a failed assertion included, is a signal that AFL++ records as a
  crash. A hang is an input that runs longer than the 15 s timeout.
- **Curated inputs.** `tools/dev/fuzz.py` names each target's curated
  inputs: `fuzz/seeds/<target>`, plus the five MemoryChunk files of
  `fixtures/w3/kct/` for `dict-loader`, `art.bin` without its 8-byte header
  for `phrase-library`, and `table.conf` for `table-conf`. A missing one
  fails. In the corpus, a curated input has its own name and every other
  input its content name, so no two distinct inputs share a name.
  `just fuzz-minimize` stages the restored corpus with them, one file per
  content: a restored file goes under its content name, so one that has a
  curated input's name but other content is kept. It then runs each curated
  input
  alone through the target's standard input, then runs afl-cmin, and puts
  back a curated input that afl-cmin dropped. afl-cmin runs with
  `AFL_MAP_SIZE=65536` unless set. afl-showmap makes its shared memory the
  size that afl-cmin passes, which is otherwise the size the binary reports.
  The AFL++ 4.40c runtime maps that memory before the coverage guards are
  counted, so the first pass of a persistent child's loop clears `MAP_SIZE`
  (65536) bytes, past the end of a smaller map. A curated input that fails
  alone stays in the run's `seed-issues/` with its output, and the message
  names its source. Any failure leaves the corpus as it was. `just fuzz`
  writes the curated inputs into the corpus before each run; a corpus file
  with a curated input's name and other content first moves to its content
  name. `dict-loader`
  reads whole files, so its input limit (`-G`) admits its largest curated
  input. An explicit `-G` replaces it.
- **Seed issues.** By default afl-fuzz skips an initial input that crashes,
  or one that times out when `-t` is given, and fuzzes on from the other
  inputs, so the run passes. `just fuzz` sets `AFL_EXIT_ON_SEED_ISSUES=1`:
  afl-fuzz stops on such an input, and the recipe fails and copies the queue
  entry that afl-fuzz names into the run's `seed-issues/`. If it finds no such
  entry, it still fails and says so. The input stays in the corpus: the
  recipe never removes or renames a corpus file, and after a seed issue it
  adds no queue entry to the corpus, so no second copy appears. The nightly
  saves the corpus cache only when the job passes, so the input comes back
  the next night and fails again until the code is fixed. It is also in the
  uploaded `seed-issues/`.
- **Findings.** Every file in `crashes/` or `hangs/` is a finding, except
  AFL++'s `README.txt`, whatever its name: with `AFL_SHA1_FILENAMES=1`,
  AFL++ names findings by hash, not `id:*`. Every queue entry joins the
  corpus under its content name on the same terms, unless the corpus
  already holds that content under any name. So a seed gets no second copy:
  with `AFL_SHA1_FILENAMES=1`, afl-fuzz names each seed's queue entry by its
  hash and stops on two seeds with the same content.
- **Refused options.** The recipe feeds each input on standard input, runs
  one afl-fuzz instance (`default/`) and supplies the target command. So it
  refuses `-f`, any argument with `@@`, `-M`, `-S`, `--` and any other
  argument that afl-fuzz would take as the target command. It reads the
  arguments with afl-fuzz 4.40c's option string, so a value such as
  `-x -f` is not taken for an option.
- **Sanitizers.** `ASAN_OPTIONS` sets `abort_on_error=1`, so an ASan error
  and a LeakSanitizer report are both an abort, which AFL++ records as a
  crash. `just fuzz` sets `LSAN_OPTIONS` explicitly. Without it, afl-fuzz
  exports `fast_unwind_on_malloc=0` and `malloc_context_size=30`, and every
  allocation takes the slow unwinder. LeakSanitizer is on. A child of the
  persistent forkserver runs many inputs, so a check at exit alone would
  blame a later input: `oxpinyin_fuzz::fuzz` calls
  `__lsan_do_recoverable_leak_check` after each input, on the shared-memory
  and the standard-input paths alike, and aborts on a leak. With a loop count
  of 1, a child runs one input, so its check at exit names that input, and
  the per-input check is skipped.
- **Isolation.** An input leaves no state that a later input of the same
  child can observe. A file reader writes its input to a file of its own and
  removes it; `user-store-ops` and `capi-commands` open a new session over a
  new user directory and close and remove both when the input ends. The
  state that `main` builds before the input runs is read-only:
  `user-store-ops` opens over the committed system fixture
  `fuzz/fixtures/user-store-ops/`, and `capi-commands` holds the files of its
  template user directory in memory and removes the directory before the
  input runs. So only an input that crashes leaves files behind.
- **Runs.** Each run writes a new directory, `fuzz/out/<target>/<UTC
  time>-<pid>/`, so a run never deletes the findings of an earlier one. In
  a reused output directory, afl-fuzz deletes the earlier run's output,
  findings included, when that run fuzzed for less than 25 minutes, and
  refuses to start when it fuzzed longer.
  Resuming the previous run (`AFL_AUTORESUME`) is not used: it reads the old
  queue and not `fuzz/corpus/<target>`, so new seeds would not enter, and the
  old crashes would fail every later run. Old run directories stay until you
  delete them. afl-fuzz hard-links its seed queue entries to the corpus
  files, so the recipe writes a seed into the corpus as a new file and renames
  it over the old one: an earlier run's queue entry keeps its bytes. A run
  directory holds afl-fuzz's output in `afl-fuzz.log`; through that pipe
  afl-fuzz prints status lines, not its terminal UI.
- **Versions.** The `afl` crate and cargo-afl must match, so the version that
  `fuzz/Cargo.lock` resolves for the `afl` crate is the cargo-afl version,
  stated once; `fuzz/Cargo.toml` names no exact version. AFL++ is the release
  that this cargo-afl bundles (4.40c for cargo-afl 0.18.2), so its version
  follows from the cargo-afl pin. `just fuzz-setup`
  installs cargo-afl and builds that AFL++ with the plugins against an LLVM
  of the fuzz nightly's major version (LLVM 22, `fuzz/rust-toolchain.toml`).
  An AFL++ git checkout that `cargo afl config --update` left in cargo-afl's
  data directory would be built in place of the bundled release, and it may
  hold your own work. So `just fuzz-setup` and `just fuzz` refuse while one
  exists and name the directory to remove; neither deletes it. One function,
  `tools_rule` in `tools/dev/fuzz.py`, states the rule: the pinned cargo-afl,
  with the plugins, and no such checkout. `just fuzz` applies it first, and
  `just doctor` calls it, so the two accept the same installations and give
  the same advice. The bundled AFL++ is Apache-2.0. It is a tool that
  cargo-afl builds from source, and no shipping crate depends on it.
- **Platforms.** Linux only, as in libchewing; AFL++ does not support
  Windows. `just fuzz-setup` and `just fuzz` exist on every platform, and on
  macOS and Windows `tools/dev/fuzz.py` refuses with one message. The setup
  runbook's [fuzzing](../runbooks/setup.md#fuzzing) section gives the reason
  and the route: a native Linux container of the host's own architecture.

The nightly soak runs every target for 180 s of fuzzing (`afl-fuzz -V`,
counted from the end of the start-up run that calibrates the corpus) with a
per-target input limit, then fails on any crash or hang, on a target that ran
no input, on a seed issue, and on a run that does not end within its time
limit. It prints each run's `stability` and `execs_per_sec`. The corpus
cache saves only when the job passes. So on a failure the nightly packs the
queue, crashes, hangs and `seed-issues/` of every run directory, the runs of
targets that passed and runs that stopped early included, into
`fuzz/out/fuzz-inputs.tar.gz` (`python -m tools.dev.fuzz archive`): AFL++
names inputs with `:`, which an artifact's file name cannot hold. It
uploads that file with each run's statistics, `afl-cmin.log` and
`afl-fuzz.log`.
`just fuzz <target> [afl-fuzz arguments]` runs one target locally: it fuzzes
`fuzz/corpus/<target>`, writes the target's curated inputs into it, and
writes findings to `fuzz/out/<target>/<run>/default/`.

### Postconditions

For valid command sequences, the contract tests in
`capi/tests/it/contract.rs` hold the postconditions:

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
