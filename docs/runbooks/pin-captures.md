# Pin captures

`tools/oracle/pin_capture.py` stages proposed pin observations for review. One capture session
takes every observation in its `OBSERVATIONS`; the observation ID's first segment is its family.

| families | what they hold | become |
| --- | --- | --- |
| `key-abort`, `key-walk` | the apostrophe inputs `'`, `''`, `'''`, `ni'`, `ni'!` and `ni'i` (below) | the waiting C-API tests in the two capi crates |
| `export`, `import`, `import-count`, `deep-choose`, `last-choose`, `past-end`, `option-row`, `option-distance`, `zhuyin-noguess-before`, `zhuyin-guess-before`, `scheme-key`, `word-moves`, `assembly`, `dynamic`, `nbest-train`, `nbest-choose`, `train-index`, `two-context`, `file-modes`, `locale`, `segment`, `tie`, `zhuyin-import` | the behaviours only the live C drivers in `tools/bisection/` checked (surfaces S1–S7 and rows D1, D3–D5, D8–D16 of the driver-retirement ledger) | C-API tests with literal expectations in the existing suites of the two capi crates |
| `utility` | the utility runs [the next pin capture](goldens-and-pins.md#the-next-pin-capture) lists, on the bdb, kc and tkrzw cells; and C1, `utility/bdb/data-sequence`, the pin's data sequence on the bdb cell | the frozen cases that table names; for C1, frozen-literal versions of the two pin-written Berkeley DB tests 3ff93c6 removed from `crates/oxpinyin-store/tests/bdb_libpinyin_files.rs` (`the_real_system_bigram_carries_libpinyins_key_and_value_shapes`, `the_real_index_tree_walks_in_byte_order`) |

The apostrophe inputs are tested natively in the crates that own the behaviour.
`crates/oxpinyin-capi/src/parse.rs` and `crates/oxpinyin-zhuyin-capi/src/parse.rs` hold the
parse and parsed length of `'`, `''` and `'''`:
- the values are Rust literals;
- each case cites the pin's frozen observation and the pin source line behind it;
- the tests need no pin, no container, no Python and no environment variable, and read no file;
- they read no database, so they run on every cell.

Two groups of those tests wait for the capture: the abort of a key query on an apostrophe-only
parse, and the key walk of `ni'`, `ni'!` and `ni'i`.

## Observation kinds

A **C API observation** (`Api`) is a sequence of calls on one facade, pinyin or zhuyin, in a
fresh worker process with a fresh user directory, on the image's tkrzw cell and the pinned
libpinyin-data. Each call is recorded with its arguments and result. Besides the C functions a
few named steps record what is not one call: `reopen`, `open_context` and `use_context` (a second
context on the same user directory), `user_conf`, `user_files` (names, modes and sizes),
`chmod_user_files`, `numeric_locale` and `set_numeric_locale`. An observation that ends in the
pin's abort keeps the results of the calls before it.

A **utility run** (`Utility`) runs one pin utility in a private copy of `fixtures/w3/<cell>/`
with a given stdin. It stages what the frozen utility cases in `crates/oxpinyin-utils/tests/`
record:
- the end (exit status or signal), stdout and stderr: text when they are UTF-8, else hex;
- every path of the tree afterwards: its kind and mode; a file's size and whether its bytes are
  the pre-run ones; the clock-masked SHA-256 of a tkrzw file; a removed input as removed;
- the SHA-256 of an unchanged or non-DBM file;
- for each new or changed DBM file, instead of a SHA-256, its sorted `[key hex, value hex]` rows
  and their `rows_digest`, unmasked, as `common::rows` reads them (a tree first, then a hash; BDB
  through `db5.3_dump`). Its container bytes are not the same on two runs: tkrzw keeps a clock in
  bytes 48–56, and Berkeley DB containers are not byte-stable (#358). No frozen case this capture
  feeds records a DBM file by its bytes;
- for the bigram runs, each named token's `SingleGram` row decoded as its total and successor
  counts, or `null` when the file has no row for it.

The **data sequence** (`DataSequence`, C1) runs the pin's `gen_binary_files --gen-punct-table`,
`import_interpolation` and `gen_unigram`, in that order (`data/Makefile.am:60-62` at 074a2219),
from the bdb cell's verified build tree, in a private work directory. Its input is the W3 subset
that `tools/dev/w3_fixtures.py` builds: `subset` writes the subset tables and interpolation text,
`table_conf('BerkeleyDB', build=True)` the art-only build `table.conf`, and `sequence` the three
programs' arguments, which `produce` runs too. It stages each program's end, stdout and stderr,
with the subset's temporary directory written as `<table-dir>`, and the rows and `rows_digest`
of `bigram.db`, `phrase_index.bin` and `pinyin_index.bin`; never their container bytes.
`fixtures/w3/db/` holds what `oxpinyin-utils`' ports wrote (`fixtures/w3/README.md`), so these
rows are the pin-written evidence for it. After the import, compare them with
`fixtures/w3/db/` through the same reader. If they match, the two tests return as frozen-literal
tests on the committed files, citing the pin lines, without `#[ignore]`. If they do not, stop and
report the first rows that differ.

## Capture

The capture runs on the pin only. It is a STOP under the
[approval conditions](../../CONTRIBUTING.md#approval-conditions): do it only when the maintainer
asks for it.

1. Provision the three cells with their build trees into a fresh `CACHE` with
   `just provision-oracle` (`provision oracle` builds a tree only when it regenerates the data). It
   runs `provision oracle` in the performance image through the selected runtime
   ([container runtimes](setup.md#container-runtimes)), into the provisioning cache that
   `just provision` also uses: `OXPINYIN_PROVISION_CACHE`, else `~/.cache/oxpinyin`. The cells are
   `oracle/<cell>` there, and model20 is `model20`:

   ```bash
   OXPINYIN_PROVISION_CACHE=CACHE just provision-oracle
   ```

   The data sequence needs a model20 extraction: `provision oracle` fetches it into
   `$PINYIN_MODEL_CACHE` (default `CACHE/model20`), as `extracted/` beside the `verified` marker
   that records the pinned archive digest. The capture refuses an extraction without that marker
   (`w3_fixtures.model_dir`). The extraction and the subset text are not redistributable: they are
   mounted read-only for the run, and nothing commits them.

   The utility runs need `CACHE/oracle/<cell>`, which `prefix_ok` accepts, and
   `CACHE/data-<cell>/src/libpinyin-<commit>`, the tree `build-oracle.sh --regen-data` built.
   `gen_ngram`, `gen_deleted_ngram` and `import_k_mixture_model` are `noinst_PROGRAMS`
   (`utils/training/Makefile.am:35` at 074a2219), so no prefix installs them, and they link the
   pin's static archives, not the prefix's library: the tree that built the verified library is
   what ties them to it.

2. On a native host, capture two runs in the capture image:

   ```bash
   OXPINYIN_PROVISION_CACHE=CACHE PINYIN_MODEL_CACHE=MODEL just capture-pin 1
   ```

   ```bash
   OXPINYIN_PROVISION_CACHE=CACHE PINYIN_MODEL_CACHE=MODEL just capture-pin 2
   ```

   The runtime is the platform's first installed one ([container runtimes](setup.md#container-runtimes)),
   and the run prints it on its `Runtime:` line. `OXPINYIN_CAPTURE_RUNTIME`, else
   `OXPINYIN_CONTAINER`, names another. On Windows, run them from Developer PowerShell, setting
   the variables first (`$env:OXPINYIN_PROVISION_CACHE = 'CACHE'`, then `just capture-pin 1`);
   `wslc` is the default there (the [WSL container limits](setup.md#native-windows-and-wsl-containers)
   apply).

   Each run first refuses the capture unless:
   - `tools.oracle.image.prefix_ok` accepts the image's tkrzw prefix;
   - the SHA-256 of the `libpinyin.so` and `libzhuyin.so` it loads equal the prefix's recorded
     `shared_object_sha256` and `zhuyin_shared_object_sha256`;
   - the data's digest equals `content_sha256` in `tools/oracle/libpinyin-data.json`;
   - `MODEL/extracted` is a complete model20 extraction with the verified marker;
   - for each cell a utility run or the data sequence uses: `prefix_ok` accepts `CACHE/oracle/<cell>`, its build tree
     is the pin commit, the `src/.libs/libpinyin.so.*` the tree built has the prefix's recorded
     `shared_object_sha256`, and each utility is a built executable in the tree.

   Only then does it take the observations. The run writes
   `target/pin-observations/<run>/capture.json` and `observations.json`, and nothing else. A run
   directory that already exists is refused.

   `capture.json` is provenance, not frozen values: the families, the source commit, the recipe
   hash, each loaded library's SHA-256, `versions_sha256`, the capture image, the data identity,
   the model20 extraction, and for each utility cell its prefix, build tree and each utility's
   SHA-256.

3. The two runs' `observations.json` must be equal:

   ```bash
   diff target/pin-observations/1/observations.json target/pin-observations/2/observations.json
   ```

4. These are proposed observations. Show them, next to the pin source lines, as a STOP before
   any test uses them ([goldens and pins](goldens-and-pins.md)). The approved values then go
   into the Rust tests as literals, each citing its run and source line. No test reads the
   staging directory.

`tools/oracle/pin_capture.test.py` holds the refusals: a prefix `prefix_ok` rejects, a loaded
libpinyin or libzhuyin that differs from the prefix's record, other data, a capture without a
provision cache, a data sequence without a verified model20 extraction, a utility cell whose prefix
`prefix_ok` rejects, a build tree of another commit
or whose library differs from the prefix's record, a utility that is not a built executable, and
a capture that would write outside its staging directory. It also checks what a utility run
stages and that the run writes only inside its private copy.

A new observation goes into `OBSERVATIONS` in `pin_capture.py`.
