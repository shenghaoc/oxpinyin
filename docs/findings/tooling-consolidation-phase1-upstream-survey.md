# Appendix F — upstream developer-tooling survey

# Upstream developer-tooling survey: libpinyin and anthy-unicode

Read-only inspection, nothing built or modified. Every row is tagged
OBSERVATION (what the files do, with path) or INTERPRETATION (what it
suggests). Paths are relative to each clone root.

Trees read:

- libpinyin: `scratchpad/ext/libpinyin` at `074a2219c90feaf962d0d24f034514033ece5f99`
  (the oxpinyin pin). Partial clone, blobs fetched on demand.
- anthy-unicode: `scratchpad/ext/anthy-unicode` at
  `fc2260cf60100bb868c20f7ca493e8c2e01dcf11`.

---

## Part 1 — libpinyin

### 1.1 Revision and directories inspected

| | |
|---|---|
| OBSERVATION | Commit `074a2219c90feaf962d0d24f034514033ece5f99`, committer date `2026-09-03T17:26:19+08:00`, subject "use g_rename to replace rename since rename fails by replace-existing-file". Version in `configure.ac`: 2.11.92, ABI 15.0; `CMakeLists.txt` says 2.11.91 (the two build systems disagree by one micro version). |
| OBSERVATION | Developer-tooling-shaped locations enumerated: `tests/` (`Makefile.am`, `CMakeLists.txt`, `tests_helper.h`, `timer.h`, `test_pinyin.cpp`, `test_phrase.cpp`, `test_chewing.cpp`, `test_zhuyin.cpp`), `tests/include/` (`test_memory_chunk.cpp`), `tests/storage/` (10 `test_*.cpp`), `tests/lookup/` (`test_pinyin_lookup.cpp`, `test_phrase_lookup.cpp`), `utils/` (`utils_helper.h`, `storage/`, `segment/`, `training/` with their `Makefile.am`/`CMakeLists.txt`), `data/` (`Makefile.am`, `CMakeLists.txt`, `table.conf.in`, `opengram.license`), `scripts2/` (Python table generators, `Makefile.data`, `templates/`, `tools/distill.py`), `cmake/` (Find modules), `.github/workflows/make-check.yml`, `.github/workflows/cmake.yml`, `autogen.sh`, `configure.ac`, `Makefile.am`, `libpinyin.spec.in`, `doc/libpinyin.1`, `.gitignore`. |
| OBSERVATION | There is no `scripts/`, `tools/`, `bench/`, `fuzz/`, or `src/*/tests` directory; no `.clang-format`, no `.editorconfig`, no lint config of any kind. `grep -rniE 'fsanitize|valgrind|fuzz|asan|ubsan'` over build and CI files returns nothing. |

### 1.2 Test runners and harnesses

| | |
|---|---|
| OBSERVATION | Two parallel build systems each define their own test set. Autotools: `tests/storage/Makefile.am` sets `TESTS = test_phrase_index_logger test_ngram test_flexible_ngram test_table_info test_punct_table`; `tests/include/Makefile.am` sets `TESTS = test_memory_chunk`. These six run under `make check` (automake's simple exit-status runner). CMake: `tests/include/CMakeLists.txt`, `tests/storage/CMakeLists.txt` register only four with `add_test` (`memory_chunk`, `phrase_index_logger`, `ngram`, `flexible_ngram`); `test_table_info` and `test_punct_table` are built but not registered under CTest. `tests/lookup/` and the top-level `tests/*.cpp` are `noinst_PROGRAMS` only, never run automatically. |
| OBSERVATION | All test programs are plain `int main` executables. Expectations are `assert()` and the `check_result(expr)` macro from `src/include/pinyin_utils.h`, defined as `assert(expr)` normally and as bare `expr` when `NDEBUG` or `G_DISABLE_ASSERT` is set. Failure = abort/non-zero exit; success = exit 0 after printing values with `printf`. No test framework, no named cases, no per-check messages. |
| INTERPRETATION | Under a Release build (`cmake -DCMAKE_BUILD_TYPE=Release` as in `.github/workflows/cmake.yml`) `check_result` degrades to the bare expression, so CI's CTest job silently loses every `check_result` assertion; only raw `assert()` calls remain, and those too are disabled if `NDEBUG` is defined by the toolchain. The pass/fail signal of the CMake CI lane is therefore weaker than it looks. |
| OBSERVATION | Interactive drivers (`tests/test_pinyin.cpp`, `test_phrase.cpp`, `test_chewing.cpp`, `test_zhuyin.cpp`, `tests/storage/test_parser2.cpp`, `test_chewing_table.cpp`, `test_phrase_table.cpp`, `test_matrix.cpp`, `tests/lookup/test_pinyin_lookup.cpp`, `test_phrase_lookup.cpp`) read stdin line by line with `std::getline`, stop on the literal line `quit`, and print results to stdout. `test_pinyin.cpp` alternates two prompts, `prefix:` then `pinyin:`, per iteration. |
| OBSERVATION | Data location is hard-coded relative to the cwd: `tests_helper.h` builds `g_build_filename("..", "..", "data", ...)`; `tests/storage/*.cpp` and `tests/lookup/*.cpp` open `"../../data/table.conf"`, `"../../data/pinyin_index.bin"`, `"../../data/bigram.db"`; top-level `tests/*.cpp` call `pinyin_init("../data", "../data")`. Scratch files go to absolute `/tmp/test.db`, `/tmp/training.db`, `/tmp/snapshot.db`, `/tmp/punct.bin`, `/tmp/user.conf`, `/tmp/gb_char.bin`, `/tmp/test.bin`. |
| OBSERVATION | `test_parser2.cpp` is the only test with a CLI: GLib `GOptionEntry` flags `--parser {fullpinyin,doublepinyin,zhuyin,pinyindirect,zhuyindirect}`, `--incomplete`, `--scheme {standard,hsu,dachen26}`. |
| INTERPRETATION | The suite is a set of smoke/REPL probes, not a regression suite. Its value for oxpinyin is as a catalogue of which internal API each probe touches (Appendix B), not as something to port verbatim. |

### 1.3 Scenario / sequence representation

| | |
|---|---|
| OBSERVATION | No committed typing-session or key-sequence file format exists. Searched `tests/`, `utils/`, `data/`, `scripts2/`: no `*.seq`, `*.txt` scenario files, no ibus-style sequence files, no expected-output files of any kind. The interactive drivers accept raw stdin: one pinyin string per line (e.g. `nihao`), or for `test_pinyin` a prefix line followed by a pinyin line. |
| OBSERVATION | Structured text formats that do exist are all model-data formats, parsed by the tag library in `src/storage/tag_utility.h` (`taglib_add_tag`, `taglib_read`, `taglib_push_state`/`pop_state`): |

Interpolation model (`utils/storage/import_interpolation.cpp` reader,
`export_interpolation.cpp` writer). Grammar, as read by `taglib_read`:

```
file     := "\data model interpolation" NL body "\end" NL
body     := ("\1-gram" NL unigram*)? ("\2-gram" NL bigram*)?
unigram  := "\item" SP token SP phrase SP "count" SP N NL
bigram   := "\item" SP token1 SP phrase1 SP token2 SP phrase2 SP "count" SP N NL
```

Example lines (format strings in `export_interpolation.cpp` `gen_unigram`
/ `gen_bigram`):

```
\data model interpolation
\1-gram
\item 16777222 你好 count 1234
\2-gram
\item 16777222 你好 16777300 世界 count 56
\end
```

K-mixture model (`utils/training/export_k_mixture_model.cpp`): same shape
with a richer header `\data model "k mixture model" count N N M ...`, unigram
items `\item <tok> <phrase> count <n> freq <f>`, bigram items
`\item <t1> <p1> <t2> <p2> count <c> T <t> N_n_0 <a> n_1 <b> Mr <m>`.

Segmented corpus (consumed by `gen_ngram.cpp`, `gen_deleted_ngram.cpp`,
`gen_k_mixture_model.cpp`, `eval_correction_rate.cpp`, `mergeseq.cpp`;
produced by `spseg.cpp`, `ngseg.cpp`, `mergeseq.cpp`). Parsed by the
`TAGLIB_PARSE_SEGMENTED_LINE` macro in `utils/utils_helper.h`:

```
line      := token SP phrase NL          ; one phrase per line
sentinel  := "0" SP NL                   ; null_token = 0 (src/include/novel_types.h)
                                         ; separates sentences; also emitted for
                                         ; unknown/unsegmentable runs with the raw text
```

Example (what `ngseg` writes: `fprintf(output, "%d %s\n", token, utf8)`;
`"%d \n", null_token` at sentence end; `--generate-extra-enter` adds a
second sentinel):

```
16777222 你好
16777300 世界
0 
```

`taglib_validate_token_with_string` (`src/storage/tag_utility.h`) is called
on every non-null line, so a token/phrase mismatch aborts the tool
(`abort()` in the macro).

Phrase table `.table` (read by `ChewingLargeTable2::load_text`,
`PhraseLargeTable3::load_text`, `FacadePhraseIndex::load_text`; sampled by
`scripts2/tools/distill.py` as `(pinyin, word, token, freq) = line.split(None, 3)`):

```
ni'hao	你好	16777222	1234
```

`gen_pinyin_table.cpp` / `gen_zhuyin_table.cpp` input (upstream raw
dictionary; `fscanf("%1023s %1023s %u")`): `phrase pinyin freq`, e.g.
`你好 ni'hao 1234`; they emit the `.table` form above with assigned tokens
(`-t <phrase_index>` selects the library slot encoded in the token's high
bits).

`data/table.conf.in`: key/value header lines (`binary format version:7`,
`model data version:14`, `lambda parameter:0.312699`,
`source table format:pinyin`, `database format:@DATABASE_FORMAT@`) then one
line per dictionary slot: `default <NAME> <table> <bin> <dbin> <type>` and
`addon <idx> <table> <bin> NULL DICTIONARY`.

| | |
|---|---|
| INTERPRETATION | The `\data … \item … \end` tag grammar is the one artefact in libpinyin that is already a line-oriented, diffable, self-describing text interchange; oxpinyin's import/export interchange (the compatibility-policy migration path) must reproduce it byte-for-byte, and a Rust reader/writer for it is the natural seed for a shared "model text" module in a unified toolchain. |

### 1.4 Regression and golden testing

| | |
|---|---|
| OBSERVATION | No golden files are stored anywhere in the tree. No test compares its output to a stored expectation. The only diff-shaped operation is `test_phrase_index_logger.cpp`, which exercises `FacadePhraseIndex::diff` / `merge` (binary delta of a phrase index) and writes `/tmp/gb_char.dbin`, `/tmp/gb_char2.bin`; its comment says `TODO: check whether gb_char.bin and gb_char2.bin should be the same` — the comparison is not implemented. |
| OBSERVATION | `test_phrase_index.cpp` hard-codes model facts as asserts: token `16870553` must have phrase length 14 and one pronunciation; token `16777222` must have length 1 and two pronunciations. These are implicit goldens against the specific `model20` data. |
| INTERPRETATION | Regression coverage upstream is effectively "does it still run on model20 without asserting". Any refresh mechanism is manual (edit the assert). Nothing here to borrow for golden management; oxpinyin's existing oracle-cell differential already exceeds it. |

### 1.5 Instrumentation, stress and performance tooling

| | |
|---|---|
| OBSERVATION | `tests/timer.h` provides `record_time()` (gettimeofday microseconds) and `print_time(start, times)` printing `Spent %d us for %d operations, %f us/op, %f times/s.` Used by `test_chewing_table.cpp`, `test_phrase_table.cpp`, `test_matrix.cpp`, `test_parser2.cpp`, `test_phrase_index.cpp` (`bench_times = 1000`) and `test_pinyin_lookup.cpp` (`bench_times = 100`), each looping the operation under test `bench_times` times on every stdin line. |
| OBSERVATION | No memory tooling, no stress loops beyond the fixed bench loop, no fuzz targets, no sanitizer flags in `configure.ac`, `CMakeLists.txt`, or either workflow. `autogen.sh` defaults `CFLAGS="-Wall -Werror"` (C only; C++ flags not set). |
| INTERPRETATION | Timing is embedded inside the interactive probes, so numbers depend on what the operator types and are never recorded. Nothing to reuse; keep oxpinyin's criterion-style benches and the `docs/runbooks/benches.md` discipline. |

### 1.6 Quality evaluation

| | |
|---|---|
| OBSERVATION | `utils/training/eval_correction_rate.cpp` is the one accuracy tool. Input: hard-coded file `evals2.text` in cwd, in the segmented-corpus format of 1.3 (one `token phrase` per line, `0 ` sentinel between sentences). Method: for each sentence, `get_possible_pinyin` picks each token's highest-frequency pronunciation from the phrase index, fabricates `ChewingKeyRest` spans, fills a `PhoneticKeyMatrix`, runs `PhoneticLookup<1,1>::get_nbest_match` with `ForwardPhoneticConstraints`, converts both token sequences to UTF-8 and `strcmp`s them. Metric: `correction rate: passed/tested` (whole-sentence exact match). Mismatches print `test sentence:` / `guessed sentence:` / `the result mis-matches.` to stderr. |
| OBSERVATION | Model-data paths are the `SYSTEM_*` constants from `src/pinyin_internal.h` (`table.conf`, `pinyin_index.bin`, `bigram.db`, …) resolved relative to cwd; no CLI options beyond `print_help` printing `Usage: eval_correction_rate`. |
| OBSERVATION | `estimate_interpolation.cpp` / `estimate_k_mixture_model.cpp` compute the lambda parameter by deleted-interpolation EM (`compute_interpolation`, epsilon 0.001, start 0.6), printing `token:%d lambda:%f` per token and `average lambda:%f`; `data/Makefile.am` target `modify` seds the result into `table.conf`. |
| INTERPRETATION | Sentence-exact-match on a segmented corpus is the right minimum metric for oxpinyin Stage-3 measurement, but it should be given a real CLI (corpus path, data dir, n-best depth) and should also emit per-sentence results so diffs between engines are attributable. The "evaluate on the model's own most-frequent pronunciation" trick is a cheap way to turn any segmented corpus into pinyin input without a separate pinyin-annotated corpus. |

### 1.7 Data / model build tooling

| | |
|---|---|
| OBSERVATION | `data/Makefile.am`: `interpolation2.text` rule runs `wget http://downloads.sourceforge.net/libpinyin/models/model20.text.tar.gz` and untars into `$(top_srcdir)/data`; `bigram.db` rule deletes all binaries then runs `gen_binary_files --gen-punct-table --table-dir`, `import_interpolation --table-dir < interpolation2.text`, `gen_unigram --table-dir`; all other binaries depend on `bigram.db`. `data/CMakeLists.txt` mirrors this with `add_custom_command` (without `--gen-punct-table`, and listing fewer `.bin` outputs than the automake side). |
| OBSERVATION | No manifest, no checksum, no size assertion, no version pin on the downloaded tarball; the only versioning is `model data version:14` inside `table.conf.in` and `binary format version:7`. `libpinyin.spec.in` packages `gen_binary_files`, `import_interpolation`, `gen_unigram` as the `tools` subpackage; `doc/libpinyin.1` documents exactly those three. |
| OBSERVATION | `scripts2/Makefile.data` target `update-header` regenerates five C++ headers (`pinyin_parser_table.h`, `special_table.h`, `double_pinyin_table.h`, `zhuyin_table.h`, `chewing_enum.h`) from `templates/*.h.in` via `generateheader.py`, which substitutes `@TABLE_NAME@` lines with Python-generated content; `tools/distill.py` builds `pinyins.txt` (pinyin→summed freq of 1–2 char words) from `../data/gb_char.table` and `gbk_char.table`. Fuzzy/correction rules live as Python data in `scripts2/options.py`. |
| INTERPRETATION | The template-plus-generator pattern (`@NAME@` placeholder expanded by a script, output committed with a "generated, don't edit" banner) is simple and worth keeping as the shape for oxpinyin's generated parser tables, but the generator should be a `cargo xtask` or build script, not a side Makefile. The unpinned network download in the data build is the thing not to copy; oxpinyin already treats model20 as a CI-absent input (`docs/testing/README.md`). |

### 1.8 Developer workflow

| | |
|---|---|
| OBSERVATION | `.github/workflows/make-check.yml`: Fedora rawhide container, `--with-dbm=BerkeleyDB`, `./autogen.sh`, `make V=1`, `make check`, `make distcheck`. `.github/workflows/cmake.yml`: Fedora rawhide, `kyotocabinet-devel`, `cmake -G Ninja -DCMAKE_BUILD_TYPE=Release`, build, install to DESTDIR, `ctest --output-on-failure`, `find .` listing. Both trigger on push/PR to `main`. |
| OBSERVATION | Because `make check` needs `../data` populated, each CI job performs the SourceForge download at build time. Tkrzw (the third backend in `configure.ac`) has no CI lane. |
| OBSERVATION | Release metadata: `configure.ac` `libpinyin_released` flag appends a datestamp to the version when 0; `Makefile.am` `dist-hook` writes `git log --name-status --date=iso` into `ChangeLog`; `NEWS` is hand-maintained; `libpinyin.spec.in` carries an RPM changelog frozen at 2012. No lint, no formatter, no pre-commit hook, no release script. |
| INTERPRETATION | CI's job is "compiles and smoke-runs on one backend per build system". It does not catch behavioural regressions. oxpinyin's per-crate `cargo test` + fmt + clippy gate plus the oracle cells is already strictly stronger; the one thing to carry over is the per-backend lane matrix idea, applied by the AGENTS.md path check rather than always.

### 1.9 Ideas worth borrowing / not copying (INTERPRETATION)

Borrow:

1. **Tag-line model interchange** (`utils/storage/export_interpolation.cpp`, `src/storage/tag_utility.h`): a stack-scoped tag grammar where every record is `\item` + positional values + named `key value` pairs. Reuse the exact grammar as the on-disk form of oxpinyin's value-level import/export, and make the Rust reader strict (unknown tag ⇒ error with line number) rather than `abort()`.
2. **Corpus-as-pinyin evaluation** (`utils/training/eval_correction_rate.cpp`): deriving pinyin input from the model's own top pronunciation lets any segmented Chinese corpus serve as an accuracy test set. Keep the sentence-exact-match metric; add per-sentence output and a CLI.
3. **Segmentation pipeline as composable filters** (`utils/segment/spseg.cpp` → `mergeseq.cpp` → `gen_ngram.cpp`): each tool reads and writes the same `token phrase` line format with `0 ` sentinels, so the corpus pipeline is Unix-pipeable. A unified Rust toolchain should keep one line format shared by segmenters, trainers and evaluators.
4. **Generated-header templates** (`scripts2/generateheader.py`, `templates/*.h.in`): `@TABLE@` placeholders expanded by a generator with a "generated, don't edit" banner. Good shape for Rust `include!` tables or `build.rs` output.
5. **Backend lane matrix in CI** (`.github/workflows/make-check.yml` vs `cmake.yml`): one lane per DBM backend. Already policy in oxpinyin's AGENTS.md verification cells; the upstream shape confirms kc/tkrzw must each get a lane at sweep time.

Do not copy:

- Relative `../../data` and absolute `/tmp/*.db` paths baked into tests (`tests/tests_helper.h`, `tests/storage/test_ngram.cpp`); oxpinyin tests should take a data dir from an env var or `#[ignore = "needs …"]` and use `tempfile`.
- `check_result` collapsing to a no-op under `NDEBUG` (`src/include/pinyin_utils.h`) — assertions that vanish in Release are a trap; Rust tests must use `assert!`/`Result`, never `debug_assert!` for correctness.
- Unpinned, un-checksummed `wget` of model data inside the build (`data/Makefile.am`, `data/CMakeLists.txt`).
- Interactive stdin REPL probes with `quit` sentinels as the primary test interface; they cannot run unattended and leave no record.
- Two divergent build systems that disagree on the test set and version (`tests/storage/Makefile.am` `TESTS` vs `tests/storage/CMakeLists.txt` `add_test`).

### Appendix A — `utils/` programs (full list)

Installed (`bin_PROGRAMS`) are marked ★; all others are `noinst_PROGRAMS`.
Paths are resolved against cwd using the `SYSTEM_*` names from
`src/pinyin_internal.h` unless `--table-dir` is given.

`utils/storage/`

| Program | Inputs → outputs |
|---|---|
| ★ `gen_binary_files` | `--table-dir D [--gen-punct-table]`; reads `table.conf` (cwd) and every `*.table` listed as SYSTEM_FILE/DICTIONARY in D → writes `pinyin_index.bin`, `phrase_index.bin`, `addon_pinyin_index.bin`, `addon_phrase_index.bin`, per-library `*.bin` phrase indexes, optional `punct.bin` (all in cwd). Uses `ChewingLargeTable2`, `PhraseLargeTable3`, `FacadePhraseIndex::load_text/compact`, `PunctTable::load_text`. |
| ★ `import_interpolation` | `--table-dir D`, stdin = interpolation text (`\data model interpolation` …) → updates unigram freqs in the `*.bin` phrase indexes and writes `bigram.db` (`Bigram::attach(SYSTEM_BIGRAM, CREATE|READWRITE)`). |
| `export_interpolation` | reads `table.conf`, `*.bin`, `bigram.db` → stdout interpolation text (inverse of import). |
| `gen_pinyin_table` | `-t <phrase_index> -o <out> FILE…`; FILE lines `phrase pinyin freq` → `.table` lines `pinyin phrase token freq`, warns on too-long phrases, invalid pinyin, duplicates. |
| `gen_zhuyin_table` | same as above for zhuyin tables. |

`utils/segment/`

| Program | Inputs → outputs |
|---|---|
| `spseg` | `[--generate-extra-enter] [-o out] [in]`; raw UTF-8 text → segmented `token phrase` lines by shortest-path segmentation over `PhraseLargeTable3` (`FacadePhraseTable3`), `0 ` sentinel per sentence/unknown run. |
| `ngseg` | same CLI; segmentation via `PhraseLookup::get_best_match` (bigram-scored) instead of shortest path. |
| `mergeseq` | `[-o out] [in]`; segmented lines → segmented lines with adjacent tokens merged into longer dictionary phrases where the phrase table has them (uses `TAGLIB_PARSE_SEGMENTED_LINE`). |

`utils/training/`

| Program | Inputs → outputs |
|---|---|
| ★ `gen_unigram` | `--table-dir D`; adds +1 to every token's unigram frequency in all SYSTEM_FILE/DICTIONARY `*.bin` (smoothing so no zero freq) → rewrites the `*.bin`. |
| `gen_ngram` | `[--skip-pi-gram-training] [--bigram-file F]`; stdin segmented corpus → increments unigram counts in `*.bin` and bigram counts in `F` (default `bigram.db`). |
| `gen_deleted_ngram` | same CLI shape, `--deleted-bigram-file`; builds the held-out ("deleted") bigram DB for interpolation estimation. |
| `estimate_interpolation` | reads `bigram.db` + `deleted_bigram.db` + `*.bin` → prints per-token and average lambda (EM). |
| `estimate_k_mixture_model` | `--bigram-file`, `--deleted-bigram-file` (defaults `k_mixture_model_ngram.db`, `k_mixture_model_deleted_ngram.db`) → same lambda output for the k-mixture model. |
| `gen_k_mixture_model` | `[--skip-pi-gram-training] [--maximum-occurs-allowed N] [--maximum-increase-rates-allowed F] [--k-mixture-model-file F] FILE…`; segmented corpus files → k-mixture model DB. |
| `merge_k_mixture_model` | `[--result-file R] SRC…`; merges several k-mixture DBs into R. |
| `prune_k_mixture_model` | `-k INT --CDF DOUBLE FILE`; prunes low-count entries in place. |
| `import_k_mixture_model` | `--k-mixture-model-file F`, stdin = k-mixture text → DB. |
| `export_k_mixture_model` | `--k-mixture-model-file F` → stdout k-mixture text (`\data model "k mixture model" …`). |
| `k_mixture_model_to_interpolation` | k-mixture DB → stdout interpolation text (converts counts). |
| `validate_k_mixture_model` | `FILE`; consistency checks on a k-mixture DB (counts vs totals). |
| `eval_correction_rate` | no options; reads `evals2.text` + system data in cwd → prints `correction rate:%f` (see 1.6). |

### Appendix B — `tests/` programs and the API each exercises

| Program | Run by | API exercised (file/item names) |
|---|---|---|
| `tests/test_pinyin.cpp` | manual | public C API `pinyin.h`: `pinyin_init`, `pinyin_set_options`, `pinyin_alloc_instance`, `pinyin_parse_more_full_pinyins`, `pinyin_guess_sentence_with_prefix`, `pinyin_guess_candidates` (`SORT_BY_PHRASE_LENGTH|SORT_BY_FREQUENCY`), `pinyin_get_full_pinyin_auxiliary_text`, `pinyin_get_n_candidate`, `pinyin_get_candidate`, `pinyin_get_candidate_string`, `pinyin_train`, `pinyin_reset`, `pinyin_save`, `pinyin_mask_out`, `pinyin_fini`. Options `PINYIN_INCOMPLETE|PINYIN_CORRECT_ALL|USE_DIVIDED_TABLE|USE_RESPLIT_TABLE|DYNAMIC_ADJUST`. |
| `tests/test_phrase.cpp` | manual | `pinyin_phrase_segment`, `pinyin_get_n_phrase`, `pinyin_get_phrase_token`, `pinyin_token_get_phrase`. |
| `tests/test_chewing.cpp` | manual | `pinyin_parse_more_chewings`, `pinyin_guess_sentence`, `pinyin_get_sentence`, `pinyin_get_chewing_auxiliary_text`, `pinyin_train`. |
| `tests/test_zhuyin.cpp` | manual, `--enable-libzhuyin` only | `zhuyin.h`: `zhuyin_init`, `zhuyin_parse_more_chewings`, `zhuyin_guess_sentence`, `zhuyin_get_sentence`, `zhuyin_train`, `zhuyin_save`, `zhuyin_mask_out`. |
| `tests/include/test_memory_chunk.cpp` | make check + CTest | `MemoryChunk`: `set_content`, `insert_content`, `remove_content`, `get_content`, `set_size`, `set_chunk`, `save`, `load`, `mmap` (under `LIBPINYIN_USE_MMAP`). |
| `tests/storage/test_phrase_index.cpp` | manual | `PhraseItem` (`append_pronunciation`, `get_nth_pronunciation`, `get_pronunciation_possibility`, `increase_pronunciation_possibility`), `FacadePhraseIndex` (`add_phrase_item`, `store`, `load`, `load_text`, `compact`, `add_unigram_frequency`), `SystemTableInfo2::load`; asserts model facts for tokens 16870553 / 16777222. |
| `tests/storage/test_phrase_index_logger.cpp` | make check + CTest | `FacadePhraseIndex::diff`, `merge`, `get_phrase_index_total_freq` on `gb_char.bin`. |
| `tests/storage/test_phrase_table.cpp` | manual | `PhraseLargeTable3::load_text`, `search` (incl. `SEARCH_CONTINUED` prefix probing), `mask_out`; timed. |
| `tests/storage/test_chewing_table.cpp` | manual | `ChewingLargeTable2::load_text`, `search`, `mask_out`; `FullPinyinParser2::parse`; `dump_ranges` from `tests_helper.h`; timed. |
| `tests/storage/test_ngram.cpp` | make check + CTest | `SingleGram` (`insert_freq`, `set_freq`, `get_freq`, `remove_freq`, `search`, `get/set_total_freq`), `Bigram` (`attach`, `store`, `load`, `get_all_items`, `save_db`, `load_db`, `mask_out`). |
| `tests/storage/test_flexible_ngram.cpp` | make check + CTest | `FlexibleSingleGram<guint32,guint32>`, `FlexibleBigram<…>` (`attach`, `store`, `load`, `remove`, `get/set_magic_header`, `get/set_array_header`, `get_all_items`). |
| `tests/storage/test_parser2.cpp` | manual | `FullPinyinParser2`, `DoublePinyinParser2`, `ZhuyinSimpleParser2`, `ZhuyinDiscreteParser2`, `ZhuyinDaChenCP26Parser2`, `PinyinDirectParser2`, `ZhuyinDirectParser2` via `PhoneticParser2::parse`; prints `pinyin raw_begin raw_end` per key; timed. |
| `tests/storage/test_matrix.cpp` | manual | `PhoneticKeyMatrix`: `fill_matrix`, `resplit_step`, `inner_split_step`, `fuzzy_syllable_step`, `dump_matrix`, `search_matrix`; `compute_pronunciation_possibility` / `increase_pronunciation_possibility` (under `#if 0`); timed. |
| `tests/storage/test_table_info.cpp` | make check (autotools only) | `SystemTableInfo2::load`, `get_lambda`, `get_default_tables`, `get_addon_tables`; `UserTableInfo::is_conform`, `make_conform`, `save`, `load`. |
| `tests/storage/test_punct_table.cpp` | make check (autotools only) | `PunctTable::attach`, `append_punctuation`, `remove_punctuation`, `get_all_items`, `get_all_punctuations`, `save_db`, `load_db`. |
| `tests/lookup/test_pinyin_lookup.cpp` | manual | `PhoneticLookup<2,3>::get_nbest_match`, `convert_to_utf8`; `ForwardPhoneticConstraints::validate_constraint`; `NBestMatchResults::get_result`; prints `pos:%ld,token:%d` and the sentence; timed. |
| `tests/lookup/test_phrase_lookup.cpp` | manual | `PhraseLookup::get_best_match`, `convert_to_utf8` over `FacadePhraseTable3` + `Bigram`. |

### Appendix C — tests comparing against stored expected output

OBSERVATION: none. No test in libpinyin reads an expectation file; the
closest thing is the in-source asserts on model20 facts in
`tests/storage/test_phrase_index.cpp` and the unimplemented
`gb_char.bin`/`gb_char2.bin` equality TODO in `test_phrase_index_logger.cpp`.

---

## Part 2 — anthy-unicode

### 2.1 Revision and directories inspected

| | |
|---|---|
| OBSERVATION | Commit `fc2260cf60100bb868c20f7ca493e8c2e01dcf11`, committer date `2026-09-10T10:36:34+09:00`, subject "COPYING: Fix rpmlint error". Version 1.0.0 with a datestamp suffix (`configure.ac` `anthy_released=0`; `meson.build` uses `date +%Y%m%d`). |
| OBSERVATION | Developer-tooling-shaped locations enumerated: `test/` (`main.c`, `check.c`, `prediction.c`, `test-matrix.c`, `test.txt`, `test.exp`, `Makefile.am`, `meson.build`), `src-util/` (`agent.c`, `egg.c`, `convdb.c`, `dic-tool.c`, `morph-main.c`, `dic-tool-input`, `dic-tool-result`, `dic-tool-usage.txt`, `typetab`, `anthy.i` SWIG, elisp), `calctrans/` (`calctrans.c`, `proccorpus.c`, `corpus.c`, `input_set.c`, `corpus.0-5.txt`, `corpus_info`, `weak_words`), `mkworddic/` (`mkdic.c`, `mkudic.c`, `writewords.c`, `calcfreq.c`, `dict.args.in`, `*.t` sources, `udict`), `mkanthydic/mkfiledic.c`, `depgraph/` (`mkdepgraph.c`, `*.depword`, `conjugate.table`, `indepword.txt`), `alt-cannadic/` (dictionary sources), `doc/` (`TESTING`, `DICUTIL`, `protocol.txt`, `SPLITTER`, `LEARNING`, …), `.travis.yml`, `meson.build`, `meson_options.txt`, `meson_dist.py`, `Makefile.am` (`update_params*`, `dist-hook`, `.sum` rule), `autogen.sh`, `anthy-unicode-test.conf.in`, `anthy-unicode.spec.in`, `Doxyfile`, `DIARY`. |
| OBSERVATION | No `.github/`, no `scripts/`, no sanitizer/valgrind/fuzz references in any build or CI file (grep over `*.am`, `*.build`, `*.yml`, `*.ac`, `*.sh`, `*.py`, `*.txt`). `doc/TESTING` mentions a `test.sh`/`test.rb` Ruby-via-SWIG path that no longer exists in the tree. |

### 2.2 Test runners and harnesses

| | |
|---|---|
| OBSERVATION | `test/Makefile.am` builds four `noinst_PROGRAMS` — `anthy` (`main.c`), `checklib` (`check.c`), `prediction`, `test-matrix` — with `-DSRCDIR="$(srcdir)" -DTEST_HOME="`pwd`"`; it defines no `TESTS`, so `make check` runs nothing. `test/meson.build` registers all four with `test()`: `anthy --all`, `checklib`, `prediction` (no args, exits 0 immediately), `test-matrix`. |
| OBSERVATION | `test/main.c` (`./anthy`) is the conversion driver: reads `test.txt`, for each `*`-prefixed line calls `anthy_set_string`, derives two result strings via `src-util/convdb.c` `find_conv_res` (converted text, and `|`-delimited segmentation), looks each up in the in-memory DB loaded from `test.exp`, tallies ok/miss/dontcare/unknown, prints `anthy_print_context` for misses (default `--print-miss-only`), then **unconditionally rewrites `test.exp`** (`save_db(expdata, db)` in `main`, `expdata = SRCDIR "/" EXPDATA`). CLI: `[N]`, `--from N --to N`, `--all`, `--ask`, `--quiet`, `--verbose`, `--print-miss-only`, `--euc`, `--ll <level>`, bare `name` ⇒ `SRCDIR/name.txt`. Exit status is always 0; failure is only visible in the printed `miss : N` counts. |
| OBSERVATION | `test/check.c` (`checklib`): init twice (`anthy_init`/`anthy_quit`/`anthy_init`), context create/release, candidate-kind queries (`NTH_UNCONVERTED_CANDIDATE`, `NTH_KATAKANA_CANDIDATE`, `NTH_HIRAGANA_CANDIDATE`, `NTH_HALFKANA_CANDIDATE`), `anthy_xstr_hira_to_half_kata`, and `shake_test`: 50 random `anthy_resize_segment` calls seeded from `timespec_get` nanoseconds. Prints `fail (…)` on failure but still exits 0. |
| OBSERVATION | `test/test-matrix.c`: `anthy_sparse_matrix_*` / `anthy_matrix_image_*` round-trip of a zero matrix and a dense 100×100 matrix through the byte-swapped image, counting mismatches; exits 0 regardless. |
| OBSERVATION | Harness isolation is by `anthy_conf_override("CONFFILE", "../anthy-unicode.conf")`, `("HOME", TEST_HOME)`, `("DIC_FILE", "../mkanthydic/anthy.dic")` in `test/main.c`, `check.c`, `prediction.c`, `calctrans/proccorpus.c`; `anthy-unicode-test.conf.in` provides the same for `anthy-agent --conffile`. |
| INTERPRETATION | The driver is a human-in-the-loop accuracy tracker, not a pass/fail test: it never returns non-zero, and because it rewrites `test.exp` in the source tree on every run, a CI run mutates the checked-in expectation file. Meson's `test()` registration therefore only proves "did not crash". |

### 2.3 Scenario / sequence representation

| | |
|---|---|
| OBSERVATION | Input file `test/test.txt`: first line `# UTF-8`; lines starting `#`, `:`, `-` are skipped by `read_file` in `test/main.c`; a line starting `*` is one test case whose body is the hiragana reading to convert (e.g. `*そのにわさきに`); a `**` line yields a body beginning with `*` (test 39: `**まにゅある`). Cases are numbered by order of appearance (`serial`), which is what `--from/--to/N` select. No key sequences, no commits, no candidate selection — one `anthy_set_string` per case. |
| OBSERVATION | Expectation file `test/test.exp` (1229 lines), parsed by `parse_line` in `src-util/convdb.c` with `sscanf("%s %s %s")`: |

```
exp_line  := src SP res [SP check] NL
           | seg SP check NL                 ; src derived by stripping '|' and '~'
src       := reading (no spaces)
res       := converted-string                 ; conversion result
           | "|" seg ("|" seg)* "|"           ; segmentation result
check     := "OK" | "X" | "*" | "?" | "|"…    ; ok / miss / dont-care / unknown /
                                              ; (a '|'-string here is a candidate
                                              ;  assignment stored in cand_str)
```

Example lines from `test/test.exp`:

```
私の名前は田畑です わたしの名前は田畑です ?
私の名前は田畑です |わたしの|なまえは|でんぱたです| ?
かいてくれた 書いてくれた ?
かいてくれた |かいてくれた| ?
```

Histogram of the trailing mark over the file: 1179 `?`, 38 `OK`, 12 `X`.
A two-field line (`src res` with no mark) is treated as OK.

| | |
|---|---|
| OBSERVATION | Corpus files `calctrans/corpus.N.txt` use a richer two-column form: `|seg|seg| |conv|conv|` — reading segmentation then converted segmentation, each `|`-delimited; `~` before a converted segment marks a candidate miss, `!` a segment-length miss (comments in `corpus.4.txt`, `calctrans.c` header). Example: `|それを|かくには| |それを|~各には|`. `corpus.0.txt` documents the role of each file (debug, main, test-set input, donated, mis-conversions, unknown origin). |
| OBSERVATION | `src-util/morph-main.c` (`anthy-morphological-analyzer`) reads plain text (one sentence per line, `#` comments, `\include FILE` directive) and emits the analysis stream consumed by `calctrans`: `segments: N`, then per element `indep_word hash=… features=1,2,3 <POS> <conv> <reading>` / `dep_word hash=… <reading>`, with prefixes `~` (candidate miss), `!` (size miss), `^` (second indep word in a segment), and `eos features=…` (`print_context_info` in `convdb.c`). |
| OBSERVATION | `src-util/agent.c` (`anthy-agent`, `--egg` mode) speaks the line protocol in `doc/protocol.txt`: `NEW-CONTEXT INPUT=UTF-8 OUTPUT=UTF-8`, `CONVERT`, `RESIZE-SEGMENT`, `SELECT-CANDIDATE`, `COMMIT`, replies `+OK`, `+DATA …`, `-ERR code msg`. This is the closest thing to a scriptable session driver, but no session scripts are committed. |
| INTERPRETATION | `test.exp` is a three-state verdict table (ok/miss/dont-care) keyed by (input, output) rather than a golden "expected output" — a change in output creates a new `?` row instead of failing. That is deliberately tolerant for an IME whose "correct" conversion is subjective; the cost is that nothing ever fails. The `|seg|` convention and `~`/`!` miss marks are compact and worth adopting as oxpinyin's human-readable segmentation notation. |

### 2.4 Regression and golden testing

| | |
|---|---|
| OBSERVATION | Goldens: `test/test.exp` is both the store and the output — `save_db` overwrites it after every run, adding newly seen (src,res) pairs as `?`. Refresh is `./anthy --all --ask`, which prompts `src -> res (y/n/d/q)` per unknown row (`ask_results`). Diff tool: none; `git diff test/test.exp` is the review mechanism. |
| OBSERVATION | `.travis.yml` runs a true golden diff for the dictionary tool: `./anthy-dic-tool-unicode --load dic-tool-input` then `diff $HOME/.config/anthy/private_words_default dic-tool-result`, and again after `--migrate` from `$HOME/.anthy`. `dic-tool-result` is one line: `みゅうつー #T00*1 ミュウツー`. |
| INTERPRETATION | The `--ask` triage loop is a nice UX for curating a verdict table; a Rust equivalent would be a subcommand that lists new `?` rows and accepts a verdict file, never an interactive prompt inside the test binary. Writing goldens back from the test run itself is the anti-pattern to avoid. |

### 2.5 Instrumentation, stress and performance tooling

| | |
|---|---|
| OBSERVATION | Debug instrumentation is env-driven: `src-splitter/splitter.c` `anthy_init_splitter` reads `ANTHY_ENABLE_DEBUG_PRINT` / `ANTHY_DISABLE_DEBUG_PRINT` and the letter set in `ANTHY_SPLITTER_PRINT` (`w` wordlist, `m` metaword, `l`, `i` indep type, `a` A* node, `1` single-segment boundary per `doc/TESTING`). `test/main.c` `print_run_env` echoes those env vars, `SRCDIR` and a timestamp at the top of every run. `anthy_set_logger(fn, level)` and `--ll N` route library logs. `DEBUG` env makes `main.c`/`proccorpus.c` print progress every 50/1000 items. |
| OBSERVATION | Stress: only `shake_test` in `test/check.c` (50 random segment resizes). No timing, no benchmark, no memory tooling, no fuzzers, no sanitizers. `configure.ac` sets `-W -Werror=format-security -Wall -Wextra -Wwrite-strings -Wstrict-prototypes -Wmissing-prototypes -pedantic` for GCC. |
| INTERPRETATION | The env-var debug-print selector is cheap and effective for a C library; in Rust the same role is `tracing` targets/`RUST_LOG`. The run-environment banner (which env vars were set, which data dir, when) is worth copying into oxpinyin's differential and bench reports so a captured log is self-describing. |

### 2.6 Quality evaluation

| | |
|---|---|
| OBSERVATION | `test/main.c` itself is the accuracy tool: `show_stat` prints `N items`, then for `conversion result` and `split result` separately `ok : / miss : / unknown :` counts (dontcare counted but not printed). Metric = whole-sentence exact match of conversion, and separately of segmentation, against curated verdicts. |
| OBSERVATION | `calctrans/` is the training-side evaluation loop: `proccorpus corpus.?.txt > parsed_data` (resizes segments to match the corpus annotation, commits the annotated candidate, dumps features), `calctrans parsed_data -o corpus_info` (empirical transition/candidate statistics as text), `calctrans parsed_data -e -o weak_words` (extract words the model mis-converts), `calctrans -c corpus_info weak_words [-d dir]` (convert to the binary sections `anthy.cand_info`, `anthy.trans_info`, `anthy.corpus_array`, `anthy.corpus_bucket`, `anthy.weak_words`). `corpus_info` is `section <name> <rows> <cols> <colnames>` then CSV rows. `Makefile.am` targets `update_params`, `update_params0` (debug corpus only), `update_params2` (incremental append). |
| INTERPRETATION | anthy separates "accuracy on curated sentences" (test/) from "parameter re-estimation on annotated corpora" (calctrans/), but both consume the same `|seg|` notation and the same `convdb.c` result DB. A unified oxpinyin tool should likewise share one corpus/verdict format between its accuracy report and any Stage-3 reweighting experiment. |

### 2.7 Data / model build tooling

| | |
|---|---|
| OBSERVATION | Dictionary pipeline, all `noinst_PROGRAMS` with `noinst_DATA` outputs: `mkworddic/mkworddic -f dict.args` (script of `set_input_encoding`, `read FILE`, `build_reverse_dict`, `set_dict_encoding`, `read_uc udict`, `write anthy.wdic`, `done` — `dict.args.in` configured by autoconf/meson with `@top_srcdir@`) → `anthy.wdic`; `depgraph/mkdepgraph` reads `master.depword` and the `*.depword`/`conjugate.table` grammar files → `anthy.dep`; `calctrans` → the five `anthy.*_info` sections; `mkanthydic/mkfiledic` concatenates `anthy.wdic`, `anthy.dep`, and the calctrans sections into one `anthy.dic` with a header of `(name_offset, key_len, contents_offset)` triples, 64-byte aligned, network byte order (header comment in `mkfiledic.c`). |
| OBSERVATION | Verification: none at build time — no checksums or manifests for `anthy.dic` or its inputs. The only checksum machinery is the top-level `Makefile.am` pattern rule `%.sum` emitting `cksum`, `sha1sum`, `sha256sum`, `sha512sum` of the release tarball. Dictionary sources (`alt-cannadic/*.ctd`, `mkworddic/*.t`, `zipcode.t` 5.8 MB, `compound.t` 3.2 MB) are committed in-tree, so the build is offline and reproducible. |
| INTERPRETATION | The `dict.args` mini-script is a readable, diffable build recipe for a composite dictionary — a shape oxpinyin's data builder could mirror as a TOML manifest listing inputs, encodings and output sections. Committing a sectioned container with a self-describing header (`mkfiledic.c`) is close to what oxpinyin's data crate already does; the missing piece upstream, per-section digests, is where oxpinyin should go further. |

### 2.8 Developer workflow

| | |
|---|---|
| OBSERVATION | CI: `.travis.yml` only (Ubuntu focal, gcc, branch `main`): `./autogen.sh --enable-installed-tests --disable-static` (the flag is not defined in `configure.ac`; autoconf ignores it), `make`, generate `test.conf` by sed from `anthy-unicode.conf.in`, `cd test && ./anthy --all && ./checklib`, then the `anthy-dic-tool-unicode --load/--dump/--migrate` + `diff` sequence from 2.4. Because `./anthy` and `./checklib` always exit 0, only the two `diff` steps can actually fail the job. No GitHub Actions. |
| OBSERVATION | Dual build systems: autotools and meson (`meson.build` 2024-era, `meson_dist.py` as `add_dist_script`). Release: `dist-hook` in `Makefile.am` and `meson_dist.py` both generate `ChangeLog` from `git log --name-status` and `NEWS` from `git shortlog PREV...CUR` + `git log --reverse --pretty='%s (%an) %h'`, with the previous tag derived from the version or `ANTHY_PREV_VERSION`; `.sum` rule for tarball digests. `Doxyfile` present. No lint, no formatter, no pre-commit hooks. |
| INTERPRETATION | The only real CI assertions are the two `diff`s; the conversion driver is run for its side-effect output, not its verdict. The auto-generated `NEWS` from git history between tags is a small, portable release idea (oxpinyin could script it with `git shortlog` in an xtask) — but generating `ChangeLog` with `--name-status` for every file is noise, not worth copying. |

### 2.9 Ideas worth borrowing / not copying (INTERPRETATION)

Borrow:

1. **Verdict table with three states** (`src-util/convdb.c` `parse_line`, `test/test.exp`): `OK` / `X` / `*` (don't-care) per (input, output) pair, so a changed-but-acceptable conversion is recorded without pretending it is wrong. Adopt as the format for oxpinyin's curated-sentence accuracy set, but make the test fail on `X` rows and on new `?` rows unless `--allow-unknown` is passed, and never write the file from the test itself.
2. **`|seg|seg|` segmentation notation with `~`/`!` miss marks** (`calctrans/corpus.4.txt`, `calctrans.c` header, `convdb.c` `get_prefix`): a one-line, human-writable way to state both the expected segmentation and which segment was wrong. Reuse verbatim for oxpinyin phrase-segmentation cases.
3. **Separate "conversion" and "split" tallies** (`test/main.c` `sum_up`/`show_stat`): scoring segmentation independently from candidate choice localises regressions. Emit both in oxpinyin's accuracy report.
4. **Run-environment banner** (`test/main.c` `print_run_env`): print the debug env vars, data dir and timestamp at the head of every run so captured logs are self-describing. Fits the `docs/runbooks/benches.md` "record the command that produced each figure" rule.
5. **Config override for hermetic runs** (`anthy_conf_override` in `test/main.c`, `anthy-unicode-test.conf.in`): tests point the library at build-tree data and an isolated `HOME` without touching installed files. oxpinyin's oracle harness already does the equivalent; keep it a single injectable config struct, not scattered `..` paths.
6. **Build-recipe script for composite data** (`mkworddic/dict.args.in`): an explicit ordered list of inputs, encodings and output steps that is itself a tracked file. Good model for a data-build manifest.

Do not copy:

- Test binaries that always exit 0 (`test/main.c`, `check.c`, `test-matrix.c`) and rely on a human reading `miss : N`.
- Rewriting the committed expectation file from inside the test run (`save_db(expdata, db)` with `expdata = SRCDIR "/test.exp"`).
- Interactive `--ask` prompting inside the test executable; make triage a separate subcommand reading a verdict file.
- `../anthy-unicode.conf`, `../mkanthydic/anthy.dic`, `"`pwd`"`-baked `TEST_HOME`, `$HOME/.config/anthy` in CI (`test/Makefile.am`, `.travis.yml`) — non-portable cwd and home-directory coupling.
- Fixed `char buf[256]`/`[1024]` line buffers with `sscanf("%s %s %s")` in the expectation parser (`convdb.c`); inputs longer than the buffer are silently truncated.
- Travis-only CI with an undefined configure flag.

---

## Cross-project summary (INTERPRETATION)

Neither upstream has a scenario/key-sequence file format for typing
sessions; both reduce a "test" to one input string → one conversion. anthy
has a curated verdict table (`test.exp`) and a corpus-annotation notation;
libpinyin has a corpus-derived exact-match rate and a diffable model text
format. A unified Rust toolchain for oxpinyin should combine: libpinyin's
`\data/\item` text as the model interchange; anthy's `|seg|` + `~`/`!`
notation and three-state verdicts as the human-facing case format; separate
conversion vs segmentation tallies; a run banner; and strict non-zero exit on
any `X` or unexpected `?`. Everything else surveyed (REPL probes, cwd-relative
data, self-rewriting goldens, assertions that vanish in release builds,
unpinned downloads) is cautionary.

---

# External reference: libchewing and mozc developer tooling

Read-only inspection, 2026-10-16 UTC. Purpose: reference material for
consolidating oxpinyin's tooling. Every row is tagged OBSERVATION (what the
files do, with path) or INTERPRETATION (what it suggests). Paths are relative
to each clone root unless absolute.

Clone roots:

- libchewing: `/tmp/claude-1000/-home-sheng-repos-oxpinyin/75efaa0c-4dbb-478f-81c2-905b9635f463/scratchpad/ext/libchewing`
- mozc: `/tmp/claude-1000/-home-sheng-repos-oxpinyin/75efaa0c-4dbb-478f-81c2-905b9635f463/scratchpad/ext/mozc` (partial clone, blob filter; `src/` is the Bazel workspace root)

---

## Part A — libchewing

### A1. Revision and directories inspected

| Tag | Item |
|---|---|
| OBSERVATION | Commit `3c4a93aa03d574c7f011ff84e8a2437c2f79b2cf`, committed 2026-03-28T08:28:30+09:00, "chore: link issue tracking to Codeberg". Crate version `chewing 0.12.0-alpha.3`, `rust-version = "1.88.0"`, edition 2024 (`Cargo.toml`). |
| OBSERVATION | Workspace members: root crate `chewing` (library), `capi` (`chewing_capi`, `crate-type = ["rlib","staticlib"]`), `fuzzer`, `tests/testhelper` (`chewing_testhelper`, cdylib), `tools` (`chewing-cli`) (`Cargo.toml` `[workspace]`). |
| OBSERVATION | Developer-tooling-shaped directories enumerated: `tests/` (14 C test programs, 5 C test tools, `testhelper.{c,h}`, `stresstest.py`, `simulate.sh`, `debug-chewing-shell.sh`, `materials.txt`, `data/`, `testhelper/` Rust crate, `CMakeLists.txt`, `README`); `fuzzer/src/bin/{fuzzer.rs,trieloader.rs}`; `benches/conversion.rs`; `tools/src/{main,flags,init_database,info,dump}.rs`; `capi/{tests/test-user-dictionary.rs,cbindgen.toml,src/symbols-{elf.map,mach_o.map,msvc.def},include/}`; `scripts/{create_keystroke_from_text.py,gendiff-from-libchewingdata.sh}`; `.github/workflows/{ci.yml,authenticate-commits.yml}`, `.github/dependabot.yml`; `CMakeLists.txt`, `CMakePresets.json`, `cmake/`; `.devcontainer/`; `.cargo/config.toml`; `rustfmt.toml`; `.clang-format`; `.helix/`, `.vscode/`; `swift/unit_tests/`, `swift/tools/CargoBuildPlugin/`, `Package.swift`; `contrib/{simple-select.c,python/}`; `doc/` (texinfo + 4 man pages); `data/` (empty submodule `chewing/libchewing-data`, `.gitmodules`). |
| OBSERVATION | `tests/data/` holds committed fixtures: `chewing.dat`, `golden-chewing.sqlite3`, `golden-chewing-v1.sqlite3`, `golden-uhash-le-64.dat`, `golden-uhash-text.dat`, `tsi.src`/`tsi.dat`, `word.src`/`word.dat`, `swkb.dat`, `symbols.dat`, `phone.cin`, `pinyin.tab`, `extra.src`, `dictionary.d/{01-extra.dat,02-empty.dat}`, `default-test.txt` (one line). |
| INTERPRETATION | The repo is a C-API-first library whose build system is CMake wrapping Cargo via Corrosion; test tooling is split between CTest-driven C programs (the legacy suite) and ordinary `cargo test` units. |

### A2. Test runners and harnesses

| Tag | Item |
|---|---|
| OBSERVATION | `tests/CMakeLists.txt`: `ALL_TESTCASES` = 14 C executables (`test-bopomofo`, `test-config`, `test-easy-symbol`, `test-error-handling`, `test-fullshape`, `test-keyboard`, `test-keyboardless`, `test-logger`, `test-regression`, `test-reset`, `test-special-symbol`, `test-struct-size`, `test-symbol`, `test-userphrase`), each registered with `add_test(NAME ${target} COMMAND ${target})`, env `RUST_LOG=trace`, and (if `USE_VALGRIND`, default ON) a second `valgrind-${target}` entry with `--error-exitcode=255 --leak-check=full`. `ALL_TESTTOOLS` = `performance randkeystroke simulate stress testchewing` (+ `genkeystroke` if curses found) are built but not registered as tests. |
| OBSERVATION | Three CTest entries shell out to Cargo: `cargo-test-lib` (`cargo test --features sqlite?`), `cargo-test-capi` (`cargo test -p chewing_capi`), `cargo-test-cli` (`cargo test -p chewing-cli`) (`tests/CMakeLists.txt` tail). Root `CMakeLists.txt` sets `CTEST_PARALLEL_LEVEL 1`. |
| OBSERVATION | Every C test program links `testhelper` (C static lib, `tests/testhelper.c`) and `chewing_testhelper` (Rust cdylib exporting `UintFromPhone`, `get_search_path`, `find_path_by_files`, UTF-8 helpers: `tests/testhelper/src/*.rs`). Compile definitions bake in `CHEWING_DATA_PREFIX` (build-dir `static_data/`), `TEST_HASH_DIR` (build `tests/` dir), `TEST_DATA_DIR`, `TESTDATA`. |
| OBSERVATION | Assertion style is TAP-like: `internal_ok()` prints `ok N msg` / `not ok N msg` + `# <expr> failed in file:line`; `exit_status()` returns 0 only when `test_run == test_ok` (`tests/testhelper.c`). Macros: `ok`, `ok_commit_buffer`, `ok_preedit_buffer`, `ok_bopomofo_buffer`, `ok_aux_buffer`, `ok_candidate`, `ok_candidate_len`, `ok_keystroke_rtn`, `has_userphrase`, `start_testcase` (`tests/testhelper.h`). `ok_*_buffer` checks the `_Check`, `_Len`, `_String`, `_String_static` (and deprecated alt) variants of the same buffer in one call via a `BufferType` function table. |
| OBSERVATION | A C test file is a `main()` calling `test_*()` functions in sequence after `putenv("CHEWING_PATH=…"); putenv("CHEWING_USER_PATH=" TEST_HASH_DIR);` (`tests/test-bopomofo.c` `main`, 2722 lines; `tests/test-userphrase.c` 1018 lines). Expected strings are UTF-8 byte escapes with a trailing comment of the glyphs. |
| OBSERVATION | Rust-side tests: 100+ `#[test]` across `src/**` (e.g. `src/editor/mod.rs` 12, `src/conversion/chewing.rs` 11, `src/dictionary/trie.rs` 10), one integration test `capi/tests/test-user-dictionary.rs`, unit tests in `tools/src/init_database.rs` (5, parsing `tsi.src` lines). No Rust-level scenario/keystroke-file runner. |
| OBSERVATION | Swift test target (`Package.swift`, `swift/unit_tests/*.swift`) drives the C ABI through a Clang module; `TestUtilAPIs.swift` re-implements a tiny `type_keystroke_by_string` that understands `<L> <R> <U> <D> <E> <EE> <H> <B>` etc. Run in CI by `swift test -v` on non-Windows (`.github/workflows/ci.yml`). |
| OBSERVATION | Failure reporting: C programs print to stdout and exit non-zero; CTest `--output-on-failure` surfaces the TAP lines (`ci.yml`). |
| INTERPRETATION | The suite that defines "correct" behaviour is the C program suite written against the original C implementation; the Rust `cargo test` units are supplementary. CTest is the single umbrella runner. |

### A3. Scenario / sequence representation

| Tag | Item |
|---|---|
| OBSERVATION | Keystroke string grammar (`tests/testhelper.c` `chewing_test_special_keys[]`, parser `get_keystroke()`): any plain byte is passed to `chewing_handle_Default(ctx, ch)`; angle-bracket tokens map to handler calls. Full token list: `<L>` Left, `<SL>` ShiftLeft, `<R>` Right, `<SR>` ShiftRight, `<U>` Up, `<D>` Down, ` ` (space) Space, `<E>` Enter, `<B>` Backspace, `<EE>` Esc, `<DC>` Del, `<H>` Home, `<EN>` End, `<T>` Tab, `<CB>` Capslock, `<PD>` PageDown, `<PU>` PageUp, `<SS>` ShiftSpace, `<TT>` DblTab, `<C0>`..`<C9>` CtrlNum(n), `<N0>`..`<N9>`,`<N+>`,`<N->`,`<N*>`,`<N/>`,`<N.>` Numlock(key). Escapes `<<>` and `<>>` yield literal `<` and `>`. An unknown multi-char token prints `unknown key: '...'` to stderr and falls back to the first char. |
| OBSERVATION | Example (`tests/test-bopomofo.c` `test_select_candidate_no_rearward`): `type_keystroke_by_string(ctx, "u6g;4x96<L><L><L>")` then `type_keystroke_by_string(ctx, "<D>")` then `ok_candidate(ctx, CAND_1, ARRAY_SIZE(CAND_1))`. Expectations are not in the keystroke string; they are C calls interleaved. |
| OBSERVATION | `tests/materials.txt` (59 lines) is the only data file with expectations: `# comment` lines; otherwise `<keystrokes ending in <E>>` + whitespace + `<expected committed string>`. Example line: `ru.4qu/6t;6rul e; fup6<E>		就平常教鋼琴`. Consumed by `tests/simulate.c`, which splits at `<E>`, feeds the keys, concatenates all commits, and compares per UTF-8 character (mismatches painted with ANSI colour), then prints `Checks: N words, Failures: M words, Ratio: x%`. No pass/fail exit code — it is a reporting tool. `tests/simulate.sh random` shuffles the file into `materials.txt-random` and deletes `uhash.dat` first. |
| OBSERVATION | `tests/data/default-test.txt` (`ji3cp3vu3cj0 vup dj4up <E> `) is the input for `tests/testchewing.c`, a filter that reads keystroke text from stdin/file and prints committed strings (configures KB_DEFAULT, candPerPage 9, maxChiSymbolLen 16, selKey 1..0, spaceAsSelection, `auto_snapshot_selections=0`). |
| OBSERVATION | `tests/genkeystroke.c` is an ncurses recorder: interactive typing is logged to a keystroke file in the same `<X>` grammar (`fprintf(fout, "<C%c>", …)`, `"<CB>"`, `"<SS>"`, `"<PD>"`, `"<PU>"`, `"<%c>"`), so recorded sessions replay through `testchewing`. `scripts/create_keystroke_from_text.py` converts Chinese text to DaChen keystrokes using `phone.cin` (no tones from context; it only maps characters). |
| OBSERVATION | Fuzzer (`fuzzer/src/bin/fuzzer.rs`) consumes raw stdin bytes: first 12 bytes set config (KB type, candPerPage clamp 5..10, maxChiSymbolLen clamp 0..39, 7 booleans, `conversion_engine` clamp 0..2), then each byte `% 25` selects one of 25 `ChewingHandle` ops (Default+next byte, Backspace, Capslock, CtrlNum+digit, Del, Enter, Esc, Space, Tab, Home, End, Left, Right, Up, Down, ShiftLeft, ShiftRight, ShiftSpace, PageUp, PageDown, DblTab, Numlock+digit, Reset, ChiEngMode+byte, ShapeMode+byte). After each op it exercises every getter (`aux`, `bopomofo`, `buffer`, intervals, candidates, commit, cursor, phoneSeq, absorb/ignore). With env `GEN` set it prints the equivalent `<X>`-grammar keystroke line plus `# chewing_set_…` comments, so a crashing corpus entry becomes a reproducible test-string. User dictionary is `":memory:"`. |
| INTERPRETATION | The keystroke grammar is the lingua franca shared by C tests, `testchewing`, `simulate`, `genkeystroke`, `stresstest.py`, the Swift tests and the fuzzer's `GEN` output. Expectations are mostly in C code, not in data files; `materials.txt` is the only data-driven case file and is advisory. |

### A4. Regression and golden testing

| Tag | Item |
|---|---|
| OBSERVATION | `tests/test-regression.c` encodes bug reports as literal keystroke strings replayed for "must not crash" (e.g. `test_libchewing_googlecode_issue_472` loops over 5 random-looking strings after `chewing_Reset`), plus named `test_libchewing_issue_NN` functions with explicit `ok(...)` checks. |
| OBSERVATION | Golden user-dictionary files are committed binaries in `tests/data/` and consumed by `capi/tests/test-user-dictionary.rs`: each test copies a golden (`chewing.dat`, `golden-chewing.sqlite3`, `golden-chewing-v1.sqlite3`, `golden-uhash-le-64.dat`, `golden-uhash-text.dat`) into a `tempfile::tempdir()`, opens via `chewing_new2(syspath, userpath, …)` or via env (`CHEWING_PATH`/`CHEWING_USER_PATH`, guarded by a `static ENV_LOCK: Mutex<()>`), types `hk4g4` and asserts preedit `策試` (a phrase that exists only in the user dictionary). Migration (uhash text/binary → trie; sqlite v1 → current) is tested this way. `#[cfg(all(target_endian = "little", target_pointer_width = "64"))]` gates the LE64 binary golden. |
| OBSERVATION | No regeneration tool for the goldens exists in-tree (grep for "golden" only hits the test). `git log` shows them at the inspected commit only (partial history). |
| OBSERVATION | `test-struct-size.c` asserts `sizeof(ChewingConfigData)` and `sizeof(IntervalType)` against hand-copied original struct layouts "for ABI compatibility". |
| INTERPRETATION | Golden testing is limited to on-disk user-data formats; conversion output has no golden corpus beyond `materials.txt` and the per-test literals. Refresh is manual (edit the C literal). |

### A5. Instrumentation, stress and performance tooling

| Tag | Item |
|---|---|
| OBSERVATION | `tests/stress.c`: loop (`-loop N`, default infinite) creating a context with `chewing_new2(sys, userphrase_path, logger, NULL)` after `clean_userphrase()`; input bytes from `rand()%256`, `-stdin`, or `-file`; values `< 192` index special keys then `normal_keys[]`, values `>= 192` select config setters (`-extra` enables all 12, default only ChiEngMode/ShapeMode); `-init` randomises initial config; `-verbose` logs each keystroke; `AFL_PERSISTENT` env raises `SIGSTOP` per iteration for AFL persistent mode. |
| OBSERVATION | `tests/randkeystroke.c`: `-n num -s seed -r`; default mode emits plausible Bopomofo syllables (initial/medial/final/tone tables) with random `<E>`; `-r` emits uniformly random printable keys and special tokens. `tests/stresstest.py` (Python 2): for seed in 0..1e9, `randkeystroke -r -s seed -n 100 | testchewing`, removes `uhash.dat` before each run, on non-zero exit performs delta-debugging (drop one token at a time while it still fails) and prints the minimal `echo '…' | ./testchewing` repro. |
| OBSERVATION | `tests/performance.c`: pipes every stdin byte through `chewing_handle_Default` with no output — a timing harness for `time`/`perf`. |
| OBSERVATION | `fuzzer/` crate is a plain binary (no `cargo-fuzz`/libFuzzer dependency; `fuzzer/Cargo.toml` deps are chewing, chewing_capi, env_logger, log); intended for AFL-style external drivers via stdin. `trieloader.rs` is a load-and-lookup smoke binary for a `.dat` trie. |
| OBSERVATION | `benches/conversion.rs` uses nightly `#![feature(test)]` `Bencher` with a two-entry in-memory `TrieBuf` and 40 pushed syllables, measuring `engine.convert(...).nth(1)`. Not in CI. |
| OBSERVATION | Memory tooling = valgrind under CTest (A2). Coverage: CMake `ENABLE_GCOV` preset `rust-coverage`/`rust-with-sqlite-coverage` sets `RUSTFLAGS=-Cinstrument-coverage …`; CI job `coverage` runs ctest then `grcov … --keep-only 'src/*' --keep-only 'capi/src/*' --llvm -t lcov` and uploads to Codecov (`.github/workflows/ci.yml`). No sanitizer configs found. |
| INTERPRETATION | Stress tooling is byte-stream driven (fuzz-friendly) and reproducible through the `<X>` grammar; the C `stress` tool and the Rust `fuzzer` overlap in purpose. Performance tooling is minimal and not gated. |

### A6. Quality evaluation

| Tag | Item |
|---|---|
| OBSERVATION | `tests/simulate.c` + `tests/materials.txt` (A3) is the only accuracy tool: per-character match ratio over 50-ish hand-collected sentences, printed, never thresholded, not run by CTest. |
| OBSERVATION | No corpus, no TSV of expected conversions, no CI gate on accuracy. |
| INTERPRETATION | Conversion quality is not measured in CI; behaviour is pinned by the unit/regression suite instead. |

### A7. Data/model build tooling

| Tag | Item |
|---|---|
| OBSERVATION | Dictionary source is a separate repo (`data/` submodule `chewing/libchewing-data`, shallow). Build: root `CMakeLists.txt` imports `chewing-cli` via Corrosion and `add_subdirectory(data)` (submodule's CMake, not present in this clone) produces `tsi.dat`, `word.dat`, `swkb.dat`, `symbols.dat` which `tests/CMakeLists.txt` copies into `static_data/` for tests. `USE_SYSTEM_CHEWING_CLI` lets distros use an installed CLI. |
| OBSERVATION | `chewing-cli init-database [-t trie|sqlite] [--csv] [--fix] [--skip-invalid] [--name/--copyright/--license/--version/--usage] tsi.src out.dat` (`tools/src/init_database.rs`): parses `tsi.src` lines `phrase freq syl syl … # comment` or CSV, reads Dublin-Core front matter (`# dc:title`, `dc:rights`, `dc:license`, `dc:identifier`, `dc:type`), validates syllable-count == char-count, collects all parse errors with `line | text` context, and prints trie statistics (node/leaf/phrase counts, heights, branch factors) on stderr. |
| OBSERVATION | `chewing-cli dump [--csv] [-o file] dict.dat|dict.sqlite3` round-trips a dictionary back to `tsi.src` format including the `# dc:*` header (`tools/src/dump.rs`). `chewing-cli info [--system] [--user] [--json] [path]` prints metadata (`tools/src/info.rs`). Man pages for each subcommand are generated by `clap_mangen` behind feature `mangen` + env `UPDATE_MANPAGE` and committed under `doc/` (`tools/src/main.rs`, `doc/CMakeLists.txt`). |
| OBSERVATION | `scripts/gendiff-from-libchewingdata.sh` diffs two SVN revisions of `phone.cin`/`tsi.src` (legacy, OpenFoundry URLs). |
| INTERPRETATION | One CLI binary with subcommands owns all data generation; verification is "dump round-trips" plus statistics, with no checksum manifest. |

### A8. Developer workflow

| Tag | Item |
|---|---|
| OBSERVATION | CI (`.github/workflows/ci.yml`): matrix {ubuntu, windows, macos} × presets {`rust-release`, `rust-with-sqlite-release`}; steps `cmake --preset`, `cmake --build build`, `cmake --install build`, `ctest --test-dir build --output-on-failure`, `swift test`. Extra jobs: `aarch64-smoke` (`cargo build --target=aarch64-unknown-linux-gnu` for lib and capi), `coverage` (fedora container, grcov → Codecov), `results` (fails if any needed job failed/cancelled). Triggers: push/PR to `master`, `release/*`, `merge_group`. |
| OBSERVATION | `.github/workflows/authenticate-commits.yml` runs `sequoia-pgp/authenticate-commits` on PRs (OpenPGP commit signing policy in `openpgp-policy.toml`). `dependabot.yml` monthly for GitHub Actions only. |
| OBSERVATION | No clippy or rustfmt job in CI; `rustfmt.toml` sets `group_imports = "StdExternalCrate"`; `.clang-format` present; `.editorconfig`. `.cargo/config.toml` defines alias `xtask = "run --package xtask --"` but no `xtask` crate exists in the workspace. |
| OBSERVATION | `CMakePresets.json` presets: `rust`, `rust-release`, `rust-coverage`, `rust-with-sqlite{,-release,-coverage}`, `default` = `rust-with-sqlite-release`. Options: `WITH_SQLITE3`, `USE_VALGRIND`, `BUILD_DATA`, `BUILD_DOC`, `BUILD_TESTING`. |
| OBSERVATION | Release artefacts: CPack `TZST` source tarball with SHA256, `chewing.pc`, CMake config package, installed man pages, `install(IMPORTED_RUNTIME_ARTIFACTS chewing-cli)`; `NEWS` is the hand-written changelog; `rsign.pub` for minisign. MSRV policy: "minimum rust version available from major distributions' next release branch" (README). |
| INTERPRETATION | CMake/CTest is the integration point for distro packagers; Cargo-only workflows exist but are secondary. Lint enforcement is absent; signing/provenance is strong. |

### A9. libchewing-specific: C-ABI compatibility, test sharing, user-dictionary isolation, CLI organisation

| Tag | Item |
|---|---|
| OBSERVATION | **C-ABI against former C implementation**: there is no differential/oracle run against the old C library (grep for differential/oracle: none). Compatibility is asserted by (a) the inherited C test suite (`tests/test-*.c`, copyrights 2004–2015) linking the Rust `chewing_capi` staticlib through `capi/src/chewing.c` (0 bytes — an empty TU to give CMake a C link language); (b) `test-struct-size.c` ABI struct sizes; (c) `test-error-handling.c` NULL-context behaviour for every API (`chewing_set_KBType(NULL,0)` returns -1, etc.); (d) ELF symbol-version script `capi/src/symbols-elf.map` with 134 exported `chewing_*` symbols in version nodes `CHEWING_0.5`, `CHEWING_0.9`, `CHEWING_0.10`, `CHEWING_0.11`, `local: *`, linked with `-u,chewing_new -u,chewing_version --gc-sections`; Mach-O exported-symbols list and MSVC `.def` parallel it; (e) `capi/include/chewing.h` generated by cbindgen (`cbindgen.toml`: `rename_fields = "CamelCase"`, `rename_variants = "ScreamingSnakeCase"`, explicit `export.include`) plus hand-kept 0.5.x compat headers (`chewing-compat.h`, `chewingio.h`, `global.h`, `mod_aux.h`). |
| OBSERVATION | **Test sharing Rust/C**: the C suite is the only consumer-level suite; Rust `#[test]`s test internal types (`Editor`, `TrieBuf`, `Syllable`) with in-memory dictionaries (`TrieBuf::new_in_memory()`, `Layered`). The one Rust integration test on the C surface (`capi/tests/test-user-dictionary.rs`) calls `chewing_capi::setup::chewing_new2` etc. directly as `unsafe extern "C"` Rust functions — no C compiler needed. The Rust `tests/testhelper` crate exports helpers to the C side so the C tests do not duplicate syllable parsing. Swift tests are a third surface re-implementing the keystroke mini-parser. |
| OBSERVATION | **User dictionary in tests**: C tests set `CHEWING_USER_PATH=TEST_HASH_DIR` (the build `tests/` dir, shared by all C test binaries, hence `CTEST_PARALLEL_LEVEL 1`) and call `clean_userphrase()` (removes `chewing.dat` and `chewing-deleted.dat`, overridable by `TEST_USERPHRASE_PATH`/`TEST_USER_DELETED_PATH`) at the start of each userphrase-sensitive case (30 calls in `test-bopomofo.c`, 28 in `test-userphrase.c`, 14 in `test-regression.c`). Rust tests use `tempfile::tempdir()` per test (`src/dictionary/trie_buf.rs`, `src/dictionary/sqlite.rs`, `src/path.rs`) or `":memory:"` (`src/dictionary/loader.rs` `UD_MEM_FILE_NAME`; fuzzer). Env-var-based tests serialise with a process-wide `Mutex` because `env::set_var` is global. |
| OBSERVATION | **CLI organisation**: one binary `chewing-cli` (crate `tools`, clap derive, `tools/src/flags.rs`) with subcommands `init-database` (alias `init`), `info`, `dump`; features `sqlite`, `sqlite-bundled`, `mangen`. Test tools (`testchewing`, `simulate`, `stress`, `randkeystroke`, `performance`, `genkeystroke`) remain separate C executables under `tests/`; `fuzzer`/`trieloader` are separate Rust bins in the `fuzzer` crate. |
| INTERPRETATION | libchewing never built an oracle harness; it treated the legacy black-box C suite + symbol map + struct-size test as the compatibility contract and let the C implementation go. That is weaker than oxpinyin's pin-built oracle but shows the minimum set worth keeping after parity is reached. |

---

## Part B — mozc

### B1. Revision and directories inspected

| Tag | Item |
|---|---|
| OBSERVATION | Commit `0136c35782536cb43d710575c311cd4185982738`, 2026-10-16T12:33:33Z, "Update the version: 3.34.6345 → 3.35.6354". `src/version.bzl`: MAJOR 3, MINOR 35, BUILD_OSS 6354, ENGINE_VERSION 24, DATA_VERSION 11. Bazel 9.0.2 via bazelisk (`src/.bazeliskrc`); deps pinned in `src/MODULE.bazel` (googletest 1.17.0, google_benchmark 1.9.5, re2 2025-08-12, protobuf 34.1, abseil). |
| OBSERVATION | Directories inspected: `src/session/` (scenario driver `session_handler_scenario_test.cc`, interpreter `session_handler_tool.{h,cc}`, `session_handler_main.cc` + `session_handler_main_sample.tsv`, `session_client_main.cc` + `testdata/input.txt`, `session_handler_stress_test.cc`, `random_keyevents_generator.{h,cc}`, `gen_session_stress_test_data.py`, `session_regression_test.cc`, `session_handler_test_util.{h,cc}`); `src/converter/` (`quality_regression_util.{h,cc}`, `quality_regression_test_lib.{h,cc}`, `quality_regression_test.cc` [29-line licence-only stub in OSS], `quality_regression_main.cc`, `quality_regression.py`, `gen_quality_regression_test_data.py`, `evaluation.bzl`, `converter_main.cc`, `immutable_converter_main.cc`, `connector_benchmark_test.cc`, `converter_regression_test.cc`); `src/data/test/{session/scenario/*.txt (83 files), quality_regression_test/*.tsv, stress_test/sentences.txt, dictionary/, config/, calculator/, renderer/}`; `src/data/dictionary_oss/` (`BUILD.bazel`, `evaluation.tsv`, dictionary00..09.txt, etc.); `src/data_manager/` (`mozc_data.bzl`, `dataset.proto`, `dataset_writer_main.cc`, `data_manager_test_base.{h,cc}`, `gen_data_version.py`, `oss/`, `testing/`); `src/dictionary/gen_*.py`; `src/testing/` (`mozctest.{h,cc}`, `testing_util.h`, `test_peer.h`, `benchmark.h`, `gunit.h`, `BUILD.bazel`); `src/build_tools/` (`collect_test_logs.py`, `mozc_version.py`, `embed_file.py`, …); `src/base/` (listing only); `src/.bazelrc`, `src/build_defs.bzl`; `.github/workflows/{linux,lint,macos,windows,android}.yaml`; `docs/`; `docker/`. |
| OBSERVATION | `src/prediction/` contains no evaluation tool; "Prediction Expected" rows in the quality TSVs are the prediction evaluation. No `src/usage_stats/` directory exists; `usage_stats` strings survive only in `config/config_handler.cc` and the GUI; the scenario interpreter's `MOZC_EXPECT_*_STATS` macros are compiled to no-ops (`session_handler_tool.cc`). |
| INTERPRETATION | mozc's test tooling is Bazel-native and data-driven: text scenario files and TSV expectation tables are first-class, embedded into test binaries by Python generators. |

### B2. Test runners and harnesses

| Tag | Item |
|---|---|
| OBSERVATION | Runner: `bazel test ... -c dbg --build_tests_only --test_output=errors --build_event_json_file=bep.jsonl` (`.github/workflows/linux.yaml` job `test`), all tests via `mozc_cc_test` wrapper (`src/build_defs.bzl`, adds `//:macro` dep and `-funsigned-char`). Tags `nolinux`/`nomac`/`nowin` filter per platform (`.bazelrc` `--test_tag_filters`). Test sizes declared (`size = "small"|"large"`). |
| OBSERVATION | gtest/gmock via `testing/gunit.h`; `testing/gunit_main.cc` is `gtest_main.cc` plus `InitMozc` (flags). `testing/testing_util.h` provides `EXPECT_PROTO_EQ` / `EXPECT_PROTO_PEQ` (text-format proto equality, partial variant). `testing/test_peer.h` gives `PEER_METHOD`/`PEER_VARIABLE` macros to poke privates. `testing/benchmark.h` wraps google_benchmark. |
| OBSERVATION | `testing/mozctest.h`: `GetSourceFile({"data","test",...})` resolves runfiles via `TEST_SRCDIR` or a generated `kMozcDataDir` header (`gen_mozc_data_dir_header` + `build_tools/embed_pathname.py`); `TestWithTempUserProfile` fixture creates a `TempDirectory`, calls `SystemUtil::SetUserProfileDirectory(temp)` in the ctor and in the dtor keeps the directory (`set_keep(true)`) when the test failed (`testing/mozctest.cc`). 60 test files derive from it. |
| OBSERVATION | Failure reporting: gtest output; CI collects logs of failed attempts from the BEP JSONL (`build_tools/collect_test_logs.py` walks `testResult` events' `file://` URIs rather than the `bazel-testlogs` symlink) and uploads as an artifact on push. |
| OBSERVATION | Standalone drivers (`mozc_cc_binary`, `testonly = 1`): `session_handler_main` (`--input file --profile dir --dictionary oss|mock --test`; in `--test` mode prints `[ PASSED ] file` / `[ FAILED ] file` and exits 0/1, otherwise REPL on stdin with extra `SHOW*` commands), `session_client_main` (`--input --output --profile_dir`; one key per line, `##` comments, blank line = new session: `session/testdata/input.txt`), `converter_main` (flags `--engine_type desktop|mobile`, `--engine_data_path`, `--magic`, `--id_def`, `--decoder_experiment_params`, `--user_profile_dir`, `--max_conversion_candidates_size`), `immutable_converter_main` (`--query --dictionary --output`), `quality_regression_main` (B6). |
| INTERPRETATION | One in-process interpreter class (`SessionHandlerInterpreter`) backs three consumers — gtest parameterised test, CLI test runner, interactive REPL — so the scenario grammar has exactly one implementation. |

### B3. Scenario / sequence representation

| Tag | Item |
|---|---|
| OBSERVATION | **File format** (`session/session_handler_tool.cc` `SessionHandlerInterpreter::Parse`): one command per line; empty lines and lines starting with `#` are skipped; columns split on TAB; a column wrapped in double quotes has the quotes stripped; empty columns are kept as empty args. Example (`data/test/session/scenario/conversion.txt`): |

```
SET_CONFIG	preedit_method	ROMAN
SWITCH_COMPOSITION_MODE	HIRAGANA
SET_CONFIG	session_keymap	MSIME
SEND_KEY	ON
RESET_CONTEXT

SEND_KEYS	syuugousuru
EXPECT_PREEDIT	しゅうごうする
SEND_KEY	SPACE
EXPECT_IN_ALL_CANDIDATE_WORDS	集合する
SEND_KEY	SHIFT LEFT
SUBMIT_CANDIDATE_BY_VALUE	集合
EXPECT_RESULT	集合
```

| Tag | Item |
|---|---|
| OBSERVATION | **Full directive list** (`Eval` in `session_handler_tool.cc`; arity = args including command). Actions: `RESET_CONTEXT` (1); `SEND_KEYS text` (each byte → `KeyEvent.key_code`); `SEND_KANA_KEYS keys kanas` (same length; pairs key_code + key_string); `SEND_KEY keyspec`; `SEND_KEY_WITH_OPTION keyspec field=value…` (protobuf `Input` fields by dotted path); `TEST_SEND_KEY keyspec`; `TEST_SEND_KEY_WITH_OPTION …`; `UPDATE_COMPOSITION (text prob)+`; `SELECT_CANDIDATE id`; `SELECT_CANDIDATE_BY_VALUE value`; `SUBMIT_CANDIDATE id`; `SUBMIT_CANDIDATE_BY_VALUE value`; `UNDO_OR_REWIND`; `DELETE_CANDIDATE_FROM_HISTORY [id]`; `SWITCH_COMPOSITION_MODE mode` (enum name); `SET_DEFAULT_REQUEST`; `SET_MOBILE_REQUEST`; `SET_HANDWRITING_REQUEST`; `SET_REQUEST field… value`; `SET_CONFIG field… value` (nested path, repeated fields appended); `MERGE_DECODER_EXPERIMENT_PARAMS textproto`; `SET_SELECTION_TEXT text`; `UPDATE_MOBILE_KEYBOARD table space_behaviour`; `CLEAR_ALL`; `CLEAR_USER_PREDICTION`. Expectations (inspect `last_output_`): `EXPECT_CONSUMED true|false`; `EXPECT_PREEDIT [text]` (concatenated segments, text normalised); `EXPECT_PREEDIT_IN_DETAIL seg…`; `EXPECT_PREEDIT_CURSOR_POS n`; `EXPECT_CANDIDATE id value`; `EXPECT_CANDIDATE_DESCRIPTION value desc`; `EXPECT_RESULT [text]` (empty = must have no result); `EXPECT_IN_ALL_CANDIDATE_WORDS value`; `EXPECT_NOT_IN_ALL_CANDIDATE_WORDS value`; `EXPECT_HAS_CANDIDATES [min]`; `EXPECT_NO_CANDIDATES`; `EXPECT_SEGMENTS_SIZE n`; `EXPECT_HIGHLIGHTED_SEGMENT_INDEX n`. Unknown command → `kUnimplemented` status → test failure. `session_handler_main` adds `SHOW`, `SHOW_ALL`, `SHOW_OUTPUT`, `SHOW_RESULT`, `SHOW_CANDIDATES`, `SHOW_REMOVED_CANDIDATES`, `SHOW_LOG id`, `SHOW_LOG_BY_VALUE value` (debug only). Legacy `CLEAR_USAGE_STATS`/`EXPECT_*_STATS` are documented in `session_handler_main_sample.tsv` but no-ops. |
| OBSERVATION | **Keyspec grammar** (`composer/key_parser.cc` `KeyParser::ParseKey`): space-separated tokens, case-insensitive; a single-character token sets `key_code` (only one allowed); modifier tokens `ctrl|control|alt|option|meta|super|hyper|shift|caps|keydown|keyup|leftctrl|rightctrl|leftalt|rightalt|leftshift|rightshift`; special keys `on off left down up right enter|return esc|escape delete|del bs|backspace henkan muhenkan kana|hiragana katakana eisu home end space textinput tab pageup pagedown insert hankaku|zenkaku|hankaku/zenkaku kanji f1..f24 numpad0..9 multiply add separator subtract decimal divide equals comma clear virtualleft virtualright virtualenter virtualup virtualdown imeaction undefinedkey` (one allowed). Examples in scenarios: `SEND_KEY	SHIFT LEFT`, `SEND_KEY	CTRL BACKSPACE`, `SEND_KEY	HENKAN`. |
| OBSERVATION | Usage across the 83 files (tally): EXPECT_PREEDIT 2514, SEND_KEYS 1716, SEND_KEY 1555, RESET_CONTEXT 831, EXPECT_IN_ALL_CANDIDATE_WORDS 271, SWITCH_COMPOSITION_MODE 233, UNDO_OR_REWIND 128, SET_CONFIG 105, EXPECT_RESULT 89, UPDATE_MOBILE_KEYBOARD 60, EXPECT_NOT_IN_ALL_CANDIDATE_WORDS 60, EXPECT_PREEDIT_CURSOR_POS 43, SET_MOBILE_REQUEST 39, EXPECT_HIGHLIGHTED_SEGMENT_INDEX 33, SEND_KANA_KEYS 31, EXPECT_PREEDIT_IN_DETAIL 24, SUBMIT_CANDIDATE_BY_VALUE 23, EXPECT_CANDIDATE 15, EXPECT_CONSUMED 11, others ≤ 7; `SEND_KEY_WITH_OPTION`, `SET_REQUEST`, `MERGE_DECODER_EXPERIMENT_PARAMS` appear in no committed scenario. |
| OBSERVATION | Driver (`session/session_handler_scenario_test.cc`): `TEST_P` over an explicit `kScenarioFileList[]` (75 desktop files, `input_mode.txt` excluded on Apple) and a second suite `SessionHandlerScenarioTestForRequest` over 8 `mobile_*.txt` × `GetMobileRequest()`; each line is run under `SCOPED_TRACE("Scenario: <line> [<path>:<lineno>]")` and a non-OK status does `FAIL() << line << "\n" << status.message()`. The fixture uses `MockDataEngineFactory` (test dictionary from `data/test/dictionary/`), forces `--use_history_rewriter=true` via `absl::FlagSaver`, and `SessionHandlerTestBase` backs up/restores `Config` and 6 flags, unlinks `user://boundary.db` and `user://segment.db` (`session_handler_test_util.cc`). `Eval` calls `SyncDataToStorage()` before every command. Files are exported with `filegroup(glob(["*.txt"]))` (`data/test/session/scenario/BUILD.bazel`). |
| OBSERVATION | File naming: `bNNNNNNN_scenario.txt` = internal bug IDs; otherwise feature names; comments `# Test for MS-IME keymap` separate sections inside one file (`undo.txt`). |
| INTERPRETATION | The format is deliberately flat (TSV, no nesting, no variables); expressiveness comes from proto-path setters (`SET_CONFIG a.b value`) and from the gtest trace carrying file:line. The explicit file list in C++ (rather than a glob) is the one maintenance wart. |

### B4. Regression and golden testing

| Tag | Item |
|---|---|
| OBSERVATION | Golden conversion results live in `data/dictionary_oss/evaluation.tsv` (681 lines: header `# status input output command argument version`; 482 `OK:` and 198 `FAILED:` rows, e.g. `OK:	これはうれしいごさんです	これは嬉しい誤算です	Conversion Match	.*誤算.*	2.26.4423`). Produced by `evaluation()` macro (`converter/evaluation.bzl`): genrule 1 runs `quality_regression_main --test_files=oss.tsv,regression.tsv --data_file=mozc.data --data_type=oss --engine_type=desktop > *_result.tsv` (tag `manual`); genrule 2 runs `quality_regression.py --base evaluation.tsv --input result --version_file` which carries forward the version stamp of unchanged rows and stamps changed rows with the current version. |
| OBSERVATION | Gate: `diff_test(name="evaluation_diff_test", file1="evaluation.tsv", file2=":evaluation", failure_message="You need to update evaluation.tsv. See the BUILD file")` (`data/dictionary_oss/BUILD.bazel`). Refresh procedure is written in the BUILD comment: `bazel build ...:evaluation` then `cp bazel-genfiles/.../evaluation_updated.tsv .../evaluation.tsv`. |
| OBSERVATION | Scenario files (B3) are the behavioural goldens; `session_regression_test.cc` and `converter_regression_test.cc` (`QueryOfDeathTest`, `Regression3323108`) hold code-form regressions on `TestWithTempUserProfile`. |
| INTERPRETATION | mozc accepts `FAILED:` rows in the golden — the golden records the *current* outcome, and the diff test catches any change in either direction; thresholds (B6) are a separate mechanism. The version column gives free "since when" provenance per row. |

### B5. Instrumentation, stress and performance tooling

| Tag | Item |
|---|---|
| OBSERVATION | `session/session_handler_stress_test.cc`: `TEST_P` sharded into 32 shards of 2500/32 key events; `RandomKeyEventsGenerator::GenerateSequence` (seeded with `--random_seed + shard` when given) emits romaji from `kTestSentences[]` plus random ASCII/special keys; every event goes through `TestSendKey` then `SendKey`, both must return true; `--set_mobile_request` switches request. Sentences are generated at build time by `gen_session_stress_test_data.py` from `data/test/quality_regression_test/oss.tsv` (column 2) into a C++ header (`session/BUILD.bazel` `session_stress_test_data`); `data/test/stress_test/sentences.txt` (1.2 MB, Aozora Bunko text converted to readings with MeCab) exists as a larger corpus. `RandomKeyEventsGenerator::PrepareForMemoryLeakTest()` preloads data so leak checkers see a flat baseline. |
| OBSERVATION | Benchmarks: google_benchmark via `testing/benchmark.h`; `converter/connector_benchmark_test.cc` defines `BM_Create`, `BM_GetTransitionCost_Random` (LCG-seeded 1024 queries, `SetItemsProcessed`), `BM_GetTransitionCost_Column` against `data_manager/testing/connection.data` mmapped. Built as a test target (name ends `_test`), not gated on numbers. |
| OBSERVATION | No sanitizer/valgrind configs in `.bazelrc`; no fuzz targets. `calculator` test data is generated with self-verification (`data/test/calculator/gen_test.py` evaluates each generated expression in Python before emitting `expr=ans`). |
| INTERPRETATION | Stress = deterministic-seed random typing of real sentences with "every key must be accepted" as the only oracle; sharding makes it parallel under Bazel. |

### B6. Quality evaluation

| Tag | Item |
|---|---|
| OBSERVATION | **TSV schema** (`converter/quality_regression_util.cc` `TestItem::ParseFromTSV`, split on TAB with empty columns skipped): `label	key	expected_value	command	[expected_rank]	[accuracy]	[platform]`. `expected_value` is text-normalised; 4-column form allows `Conversion Expected N` to carry the rank in the command column; `accuracy` defaults 1.0; `platform` is a comma list of `desktop|oss|mobile|preload|all` (bitfield), default `desktop`. A label prefixed `DISABLED_` is embedded with `enabled=false` (`gen_quality_regression_test_data.py`). |
| OBSERVATION | **Commands** (status/expectation values): `Conversion Expected`, `Conversion Not Expected`, `Conversion Match` (RE2 full-match of top result), `Conversion Not Match`, `ReverseConversion Expected`, `ReverseConversion Not Expected`, `Prediction Expected`, `Prediction Not Expected`, `Suggestion Expected`, `Suggestion Not Expected`, `ZeroQuery Expected`, `ZeroQuery Not Expected`. Pass rule for `*Expected`: `GetRank(expected, segments)` finds the smallest candidate rank at which the expected string can be assembled across segments; pass iff `0 <= rank <= expected_rank`; `Not Expected` negates; `Prediction/Suggestion Not Expected` with zero candidates passes. Example rows: `regression	おだい	お題	Conversion Expected 2`; `oss_issue12	でびあんをつかっています	デビアン.*	Conversion Match`; `myoji	あい	愛	Prediction Expected	30`. |
| OBSERVATION | Corpus sizes: `oss.tsv` 501 rows (per-issue labels like `oss_issue12`), `regression.tsv` 181, `myoji.tsv` 50 614 (surname predictions, rank ≤ 30). Command tally: Prediction Expected 50 679, Conversion Expected 305 (+ ranked variants 2/3/5/9/12/19/99/999), Conversion Match 53, Conversion Not Match 37, Suggestion Not Expected 2. |
| OBSERVATION | **Threshold enforcement** (`converter/quality_regression_test_lib.cc`): results grouped by label; per label, rows are sorted so passes (accuracy -1.0) come first; for each failing row the running pass ratio `correct/total` must exceed the row's `accuracy` (`EXPECT_LT(accuracy, actual_ratio)`), so `accuracy` is "minimum pass ratio for this label" and 1.0 means "no failure allowed"; disabled rows are only logged (`PASSED/FAILED (DISABLED)`), and a label whose disabled rows all pass logs `CLOSED ISSUE [platform=…]: label with N cases`. Platform mask filters rows per run. |
| OBSERVATION | **Gating in OSS CI**: `quality_regression_test.cc` is a 29-line licence-only stub, and `quality_regression_test_lib` has no `mozc_cc_test` consumer in the OSS tree, so the threshold test is not run in OSS CI. What runs is `//data/dictionary_oss:evaluation_diff_test` (B4) — exact-match of the evaluation TSV — plus `oss_data_manager_test` and `mock_data_manager_test` (B7). `quality_regression_main` is `testonly`, visible to `//data:__subpackages__` and `//tools/periodical_update` (not in OSS). |
| INTERPRETATION | mozc's measured gate is "nothing changed" (diff_test) and its tolerance gate (accuracy ratios) is internal-only; in OSS the thresholds are documentation. Both read the same TSV, so one corpus format serves exact-golden, ranked-threshold, and regex modes. |

### B7. Data/model build tooling

| Tag | Item |
|---|---|
| OBSERVATION | `data_manager/mozc_data.bzl` `mozc_dataset()` macro: ~25 genrules (`name@user_pos`, `@pos_matcher`, `@pos_id_map`, `@collocation`, `@connection`, `@dictionary`, `@suggestion_filter`, `@pos_group`, `@boundary`, `@segmenter`, `@counter_suffix`, `@suffix`, `@reading_correction`, `@symbol`, `@emoticon`, `@emoji`, `@single_kanji`, `@zero_query`, `@a11y_description`, `@version`, optional `@usage`, `extra_data`) each a Python or C++ generator (`//dictionary:gen_user_pos_data`, `gen_pos_matcher_code`, `gen_system_dictionary_data_main` `--input --user_pos_manager_data --output`, `//data_manager:gen_connection_data --use_1byte_cost`, `//prediction:gen_suggestion_filter_main`, `//rewriter:gen_collocation_data_main --error_rate`), then `dataset_writer_main --magic=<escaped bytes> --output=mozc.data name:align:file …` packs them, and `//build_tools:embed_file` turns the blob into a C++ `.inc` (`kOssMozcDataSet`). |
| OBSERVATION | Container format `data_manager/dataset.proto`: magic bytes, aligned file chunks, trailing `DataSetMetadata` (name/offset/size entries), metadata size, SHA1, file size — self-describing and checksummed. `gen_data_version.py` writes `ENGINE_VERSION.DATA_VERSION.tag`. Two datasets: `//data_manager/oss:mozc_dataset_for_oss` (magic `\xEFMOZC\r\n`) and `//data_manager/testing:mozc_dataset_for_testing` from `data/test/dictionary/` (small mock dictionary used by scenario tests). |
| OBSERVATION | Verification: `data_manager/data_manager_test_base.cc` `RunAllTests()` — `ConnectorTest_RandomValueCheck` (samples 1/100000 of the text connection matrix and checks the binary agrees modulo resolution), `SegmenterTest_SameAsInternal` (all lid×rid against the generated `segmenter_inl.inc` rule), `SuggestionFilterTest_IsBadSuggestion` (bloom-filter false positive ratio ≤ 1e-4 over the dictionary), `CounterSuffixTest_ValidateTest` (sorted), `PosIdMapTest_Load`; instantiated for oss and mock (`oss_data_manager_test.cc`, size `large`). Dictionary sources are plain text split into `dictionary00..09.txt` with `aux_dictionary.tsv`/`dictionary_filter.tsv` applied by `gen_aux_dictionary.py --strict` / `gen_filtered_dictionary.py`. |
| INTERPRETATION | Generation and verification are both Bazel targets reading the same text sources; the binary blob is never committed, so "data drift" cannot happen silently. |

### B8. Developer workflow

| Tag | Item |
|---|---|
| OBSERVATION | CI workflows per OS (`.github/workflows/{linux,macos,windows,android}.yaml`); Linux: `bazelisk build package --config release_build` and separately `bazelisk test ... -c dbg --build_tests_only --test_output=errors --build_event_json_file=bep.jsonl`, timeout 60 min, `concurrency.cancel-in-progress`, actions pinned by SHA, `persist-credentials: false`, failing-test logs uploaded. `lint.yaml`: only `buildifier --mode=diff -r .` (Bazel file formatting). Style is Google style by policy (`CONTRIBUTING.md`); no clang-format job. |
| OBSERVATION | `.bazelrc` encodes platform/channel configs (`oss_linux`, `prod_linux_dev`, …), C++20, `-funsigned-char`, `--per_file_copt="external/.*@-w"`, `--test_tag_filters`. `docker/ubuntu24.04/Dockerfile` is the reference build env (bazelisk pinned). `docs/build_mozc_*.md` per platform; `docs/release_history.md` lists tagged ranges with checkout commands. Version bumps are commits (`version.bzl`). `CONTRIBUTING.md`/`VOCABULARY_POLICY.md`: dictionary-content suggestions only via forms, no PRs. |
| INTERPRETATION | One build system (Bazel) owns build, data generation, tests, benchmarks and packaging; CI is thin because Bazel is thick. |

### B9. Ideas worth borrowing (INTERPRETATION) and what not to copy

| # | Idea | Source file |
|---|---|---|
| 1 | **One scenario interpreter, three front-ends.** A Rust `ScenarioInterpreter` type that parses TSV lines and returns `Result<(), ScenarioError>` can back (a) a `#[test]` that walks a directory of `.txt` files with `file:line` in the panic message, (b) a CLI `oxpinyin-tool scenario --test file` returning 0/1, (c) a REPL with `SHOW*` commands for debugging. oxpinyin's oracle cell could be a fourth front-end running the same file against pin-built libpinyin. | `src/session/session_handler_tool.cc`, `src/session/session_handler_scenario_test.cc`, `src/session/session_handler_main.cc` |
| 2 | **Flat TSV grammar with proto-path setters.** Keep commands as `VERB<TAB>args`, `#` comments, blank-line separators; express config as `SET_CONFIG key value` rather than inventing per-option verbs. Keyspec tokens as space-separated case-insensitive names. | `src/session/session_handler_tool.cc` `Parse`/`Eval`; `src/composer/key_parser.cc` |
| 3 | **Quality TSV with rank + ratio + platform columns, plus a committed `evaluation.tsv` status golden and a diff test.** The same corpus drives an exact "nothing changed" gate (diff) and an optional tolerance gate (per-label pass ratio). The version column is cheap provenance. For oxpinyin the "platform" column maps naturally to DBM cell / backend. | `src/converter/quality_regression_util.cc`, `src/converter/quality_regression_test_lib.cc`, `src/converter/evaluation.bzl`, `src/data/dictionary_oss/BUILD.bazel` (`diff_test`) |
| 4 | **Keystroke mini-grammar as the universal reproducer.** libchewing's `<E>`, `<B>`, `<C1>` tokens appear in C tests, the filter tool, the recorder, the Python reducer and the fuzzer's `GEN` output. A fuzzer that can print its input as a human-readable keystroke line (and a `stresstest.py`-style one-token-at-a-time reducer) turns every crash into a test literal. | `tests/testhelper.c` (`chewing_test_special_keys`), `fuzzer/src/bin/fuzzer.rs` (`GEN`), `tests/stresstest.py` (`try_to_reduce_key`) |
| 5 | **Temp-profile fixture that keeps the directory on failure, and golden on-disk user files copied into a tempdir per test.** `TestWithTempUserProfile` (keep on `HasFailure()`) + libchewing's copy-golden-then-open pattern for format migrations (uhash→trie, sqlite v1→v2) is exactly the shape of oxpinyin's KC/tkrzw/BDB interoperability tests. | `src/testing/mozctest.cc`; `capi/tests/test-user-dictionary.rs` |
| 6 | **Seeded, sharded random-typing stress with a corpus of real readings, and data-manager self-checks sampling the text source against the binary.** The 32-shard `TEST_P`, `--random_seed`, and `ConnectorTest_RandomValueCheck`'s 1/100000 sampling are cheap to port to `cargo test` + bench. | `src/session/session_handler_stress_test.cc`, `src/data_manager/data_manager_test_base.cc` |
| 7 | **Single CLI with subcommands + committed man pages generated from clap.** `chewing-cli {init-database,info,dump}` with `clap_mangen` behind a feature; `dump` round-trips to the source format with Dublin-Core header so `init-database(dump(x)) == x` is a test. | `tools/src/{main,flags,dump,init_database}.rs`, `doc/CMakeLists.txt` |

Not to copy:

- OBSERVATION/INTERPRETATION: libchewing's shared `TEST_HASH_DIR` user-dictionary path across all C tests with `CTEST_PARALLEL_LEVEL 1` (`tests/CMakeLists.txt`, `tests/testhelper.c` `get_test_userphrase_path`) — forces serial tests and global env mutation (`putenv`, `ENV_LOCK`). oxpinyin should pass paths explicitly, never via env, and give each test its own tempdir.
- OBSERVATION/INTERPRETATION: mozc's hard-coded `kScenarioFileList[]` in C++ (`session_handler_scenario_test.cc`) duplicating the directory listing; use a directory walk (with an explicit exclusion list for platform-specific files).
- OBSERVATION/INTERPRETATION: mozc's golden `evaluation.tsv` is regenerated by hand-copying a Bazel output (`data/dictionary_oss/BUILD.bazel` comment); provide an `--update` flag on the same driver instead.
- OBSERVATION/INTERPRETATION: `materials.txt`/`simulate.c` accuracy report that never fails (`tests/simulate.c`) — a metric without a gate decays; if accuracy is measured, commit the status TSV and diff it (idea 3).
- OBSERVATION/INTERPRETATION: libchewing's nightly-only `#![feature(test)]` bench (`benches/conversion.rs`) and Python 2 `stresstest.py` — toolchain-pinned repos should use stable `criterion`/`divan`-style benches and Rust-native reducers.
- OBSERVATION/INTERPRETATION: libchewing's absence of fmt/clippy in CI and the dangling `xtask` alias (`.cargo/config.toml`) — oxpinyin already gates fmt/clippy; keep it.
- OBSERVATION/INTERPRETATION: mozc's internal-only threshold test (OSS stub `quality_regression_test.cc`) — do not ship a gate that exists only as a library with no runner.
