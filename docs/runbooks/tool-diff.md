# Utility compatibility cases

`tool-diff` is the registered case-file runner for the pin's standalone
utilities. The C-API `contract-diff` driver cannot execute utility argv/stdin
or observe arbitrary output files. Both runners use the same discrepancy
record reader, digest comparison and `just policy-record` writer.

Provision per [cloud.md](cloud.md), then run the full utility set in the
canonical image:

```sh
bash tools/oracle/compat.sh --cell tkrzw --runners tool-diff
```

Compatibility preparation builds the unmodified pin's libraries with
`tools/oracle/build-oracle.sh --libs-only`, then every registered utility
target in that build tree (`PIN_UTILITIES` groups them by `utils/`
directory). Cell artifact selection builds `oxpinyin-utils` once and
records both binaries. The runner checks the pin source revision and diff
before executing either utility. The normal heavy slot governs preparation,
subject builds and the differential. No release builds are needed.

With a provisioned oracle source/build tree and built debug subjects, the
same runner accepts explicit cases:

```sh
just runner tool-diff --pin-build /path/to/oracle/src/libpinyin-074a2219c90feaf962d0d24f034514033ece5f99 \
  --subject-dir target/cells/tkrzw/debug --cases zhuyin-normal
```

Case files in `tools/bisection/tool-cases/` declare a utility, arguments,
source citation, environment, stdin, and relative input files. Bytes are
`text`, `hex`, or a checked-in `file`; model-table inputs name a verified
`PINYIN_MODEL_DIR` table and a `pinyin` or `zhuyin` generator row format.
The latter reorders existing model rows and uses the committed canonical
zhuyin spelling map, never an oracle query. Model20 supplies pinyin tables,
not tsi.src: the zhuyin cases convert `gb_char`, `gbk_char` and `merged`.
No model data is committed.

Each side gets identical inputs in a private working directory. The runner
compares exit status, byte-preserving stdout/stderr and the complete file
inventory (including empty files, modified inputs and directory entries).
Only GLib diagnostic process IDs and timestamps are masked. Signals are
invalid unless a source-derived case registers the pin signal and a named
policy record; subject signals and incomplete/timed-out observations fail.

`--observations PATH` writes local JSON lines. Registry runs keep this
capture as `tool-diff-tkrzw/observations.log` alongside local comparison logs.
Use the shared writer to bind named cases:

```sh
just policy-record docs/findings/eio-table-generator-invalid-utf8.md /path/to/observations.log \
  --cases pinyin-invalid-utf8,pinyin-invalid-utf8-after-row,zhuyin-invalid-utf8,zhuyin-invalid-utf8-after-row
```

Every accepted pair binds both complete observations with the existing
schema-2 record digests. A changed exit, stream or file fails the record.

The registered table generators, `k_mixture_model_to_interpolation` and
`gen_unigram` open no DBM: `DATABASE_FREE` in `tools/dev/tool_diff.py`
names them, and each capture row carries `database_free` from it. The three segmentation
utilities do open one, so their rows stay per-cell. The writer records such a case once, for every
cell:

```json
"pinyin-invalid-utf8": {"database_free": {"pin": {...}, "subject": {...}, "divergence": [...]}}
```

One cell's capture is enough. When several cells are passed, every row of
the case must digest the same, or nothing is written; an existing per-cell
entry for the case is replaced, with a notice. The reader accepts a
`database_free` entry on every cell only for a case whose utility is in
`DATABASE_FREE`; a contract-diff case, or a utility that opens a database,
reads it as INVALID and keeps per-cell `cells` entries. A capture taken
before the `database_free` field is refused and must be taken again.
`--expect-differ --cases ID` supports a named mutation or reverted subject;
INVALID is never an expected difference. Captures remain disposable local
working data and are not committed.

## #1436 inventory

The attached task named nine comparisons; #1436's reported body counted
8 MATCH plus 2 POLICY-ACCEPTED. Its committed integration tests contain six
test functions representing seven executions: ordering/diagnostics,
unterminated final row, missing input, bad option, empty phrase, and two
invalid-UTF-8 inputs. The case file preserves those seven inputs and adds
help, output-open failure, and no input: ten cases, eight ordinary and two
policy cases. The unavailable one-off driver's exact inventory is not
claimed as recovered. Future additions extend these case files.

## Regression sensitivity

The named mutation `table-frequency-floor-4` changes only
`gen_pinyin_table::Sorted::write_with` from `reading.freq.max(3)` to
`reading.freq.max(4)`. The full BDB compatibility run detects it in six
cases (both normal cases, zhuyin's unterminated final row, and the three
model20 tables): 6 DIFFER, 13 MATCH, 5 POLICY-ACCEPTED. Restoring the source
byte-for-byte is required before the completion run. The pin is unchanged.

## k_mixture_model_to_interpolation (2026-10-31 UTC)

`tool-cases/k_mixture_model_to_interpolation.json` holds sixteen
source-derived cases. The pin utility is built in `utils/training`;
preparation makes it with the two table generators. Five cases end in the
pin's SIGABRT: four failed `check_result(taglib_read(...))` assertions
and one `abort()` on a line type outside its `switch`. Each registers
`pin_signal: -6` with `reproduces_signal: true`, so the subject must
abort too. The `abort()` case compares as an ordinary case. The four
assertion cases bind to
[eio-tool-check-messages](../findings/eio-tool-check-messages.md): under
the 2026-10-31 UTC ruling oxpinyin does not imitate the macros' stderr
text, while stdout and the exit match. Three class (b) cases bind to
[eio-kmm-to-interpolation-undefined-lines](../findings/eio-kmm-to-interpolation-undefined-lines.md).
Both records hold bdb, kc and tkrzw evidence, since `cells-for-diff.py`
selected all three cells.

After the tools moved to `libc`, `glib-sys` and `std::io`, every
`gen_pinyin_table` and `gen_zhuyin_table` case kept its observation. On
bdb the full set was 28 MATCH and 12 POLICY-ACCEPTED. Re-scored against
the committed records, the kc observations were the same, and tkrzw was
28 MATCH, 7 POLICY-ACCEPTED and 5 INVALID: the five #1436/#1450 policy
cases have bdb and kc evidence here, and #1434 binds their tkrzw
evidence.

The named mutation `kti-freq-positive` changes only the unigram test in
`k_mixture_model_to_interpolation::Converter::parse_unigram` from
`freq != 0` to `freq > 0`. The bdb run detects it in `kti-numeric-fields`
(1 DIFFER, 27 MATCH, 12 POLICY-ACCEPTED). The source was restored byte
for byte before the completion run.

## KMM utilities (2026-10-31 UTC)

`tool-cases/{validate,merge,prune,estimate,import,export,gen}_k_mixture_model.json`
hold 35 source-derived cases for the seven k-mixture-model utilities. They
use the per-cell `{"fixture_dir": "fixtures/w3"}` input, which copies
`fixtures/w3/<cell>` into each side's working directory: the cwd-relative
`table.conf` and SYSTEM_FILE chunks, and the three committed KMM
containers `k_mixture_model{,_zero,_edom}.db` (see
`fixtures/w3/README.md`). Cases that write a container are absent
deliberately: a tkrzw HashDBM file's header timestamp and Kyoto Cabinet's
bucket metadata are not reproducible bytes, so only read-only programs,
non-writing imports and the three source-derived aborts compare a complete
inventory. Merge/prune/estimate/gen writes are covered by the crate tests
instead.

On the tkrzw reference cell the result is **32 MATCH and 3
POLICY-ACCEPTED, 0 DIFFER**: the three aborts (`kmm-merge-no-result-file`,
`kmm-estimate-zero-total`, `kmm-gen-good-document`) bind in
[eio-kmm-tools](../findings/eio-kmm-tools.md); both sides raise SIGABRT,
and the pin's assertion line is the only difference
([eio-tool-check-messages](../findings/eio-tool-check-messages.md)).

The named mutation `kmm-prune-star-threshold` changes only
`oxpinyin-kmm/src/prune.rs`'s `progress >= onestep` to `progress >
onestep`, which drops the per-row progress asterisk. The tkrzw run detects
it in `kmm-prune-edom` (1 DIFFER when that case is selected; the completion
run is 32 MATCH and 3 POLICY-ACCEPTED). The source is restored byte for
byte before the completion run.

## Database-free evidence (2026-10-31 UTC)

The four tool records (eio-table-generator-invalid-utf8,
eio-zhuyin-missing-frequency, eio-tool-check-messages and
eio-kmm-to-interpolation-undefined-lines) were re-rendered from one bdb
capture into `database_free` entries. Each new entry is byte-identical to
every per-cell entry it replaced. Re-scored against them, the bdb and kc
captures stay 28 MATCH and 12 POLICY-ACCEPTED. The tkrzw capture is now
28 MATCH and 12 POLICY-ACCEPTED as well: the five #1436/#1450 cases that
lacked tkrzw evidence are covered by the entry for every cell.

## Segmentation utilities (2026-10-31 UTC)

`tool-cases/{ngseg,spseg,mergeseq}.json` hold ten, ten and nine
source-derived cases. They cover GLib `--help`, an unknown option, too
many arguments (the output file is opened first), a missing input file,
a missing `table.conf`, empty input and an empty line, the
`--generate-extra-enter` flag for the two segmenters, invalid UTF-8, and
ordinary segmentation/merge over the committed `fixtures/w9` inputs.

The case file gained one payload kind for them: `"fixture_dir":
"fixtures/w3"` copies the cell's committed data directory (`db` for bdb,
`kct` for kc, `tkt` for tkrzw — `fixtures/w3/README.md`) into the
private working directory before the case's own `files` are written, so
`table.conf`, `phrase_index.bin`, the chunk files and `bigram.db` are the
relative names the pin opens. The complete file inventory still compares.

On the BDB cell (pin build at the BDB configuration, subject built with
`--features bdb`) and on the tkrzw reference cell (pin build at the
tkrzw configuration, subject built with the default tkrzw features, the
sound `libtkrzw` on `LD_LIBRARY_PATH`), all 29 cases reported MATCH.
`ngseg` exercises the real `PhraseLookup::get_best_match` path over the
committed bigram; `spseg`/`mergeseq` exercise the phrase table alone.

The named mutation `seg-phrase-index-name` changes only the phrase-table
path in `oxpinyin-utils::segment::data::load` from `phrase_index.bin` to
`phrase_index.absent.bin`. The tkrzw run detects it in the three ordinary
cases (`ngseg-mini-fixture`, `spseg-mini-fixture`,
`mergeseq-mini-fixture`): 3 DIFFER, 0 MATCH. The source was restored byte
for byte (sha256 checked) before the completion run, which returned the
29 MATCH above.

## Storage utilities (2026-10-31 UTC)

`tool-cases/{gen_binary_files,import_interpolation,export_interpolation}.json`
hold eight, nine and seven source-derived cases. They cover GLib `--help`
and an unknown option; positional arguments the pin ignores; a missing
`table.conf`, `.table`, `punct.table`, `phrase_index.bin` and chunk; empty
stdin and a wrong model header; and ordinary runs over the committed
`fixtures/w3` data (with the `.table` inputs the case writes).

Results, one run per cell against that cell's pin build:

| cell | MATCH | DIFFER |
|---|---|---|
| tkrzw (reference) | 17 | 7 |
| bdb | 18 | 6 |
| kc | 24 | 0 |

Every tkrzw and bdb DIFFER is the writing DBM library's own metadata:
tkrzw stores a `cyclic_magic` counter and a wall-clock `timestamp` in
each container it opens for writing; Berkeley DB stores its own header
fields. Two runs of the *same* binary never produce equal container
bytes. The pin and subject produce the same logical rows in every one:
`tkrzw_dbm_util list` and `db5.3_dump` output are identical for every
index and bigram file, and the chunk files are byte-identical. The
runner's byte inventory cannot mask those fields without an unsound
utility-specific rule, so those cases stay DIFFER and this paragraph is
their record.

A full-model20 run also reproduces every chunk file byte for byte and
every decoded pinyin-index record (201,658 system and 32,890 addon keys)
in both key spaces; the raw pinyin-index **value** bytes differ only in
the tail padding the pin's C++ struct leaves uninitialised and the Rust
writer zeroes (the same recorded `datagen-compat` artefact, not a record
difference). Running `import_interpolation` over
`model20/interpolation2.text` after both `gen_binary_files` runs leaves
the same chunks and bigram rows, and `export_interpolation` then writes
byte-identical stdout: 83,457,181 bytes in 1,913,520 lines.

One bdb `exp-missing-bigram` case also differs in stderr: the pin calls
`Bigram::get_all_items` on the handle a failed `attach` left unopened,
and libdb itself writes `BDB1565 DB->cursor: method not permitted before
handle's open method`; the subject's store answers the failed open
without a library diagnostic. The record is a backend library
diagnostic, not a pin format.

The named mutation `storage-native-order` replaces
`Hash::native_keys` with the store's ascending `range_raw` walk in
`export_interpolation`. The tkrzw run detects it in `exp-mini-fixture`
(the `\2-gram` group order changes): 1 DIFFER. The source was restored
byte for byte (sha256 checked) before the completion run.

## N-gram and interpolation utilities (2026-10-31 UTC)

`tool-cases/{gen_unigram,gen_ngram,gen_deleted_ngram,estimate_interpolation,eval_correction_rate}.json`
hold seven, nine, nine, six and nine source-derived cases (40 in all).
`gen_unigram` is registered `DATABASE_FREE`: its cases write a literal
`table.conf` and copy the backend-independent chunk files from
`fixtures/w3/db`, so its observations are identical in every cell. The
other four open a DBM and stay per-cell.

The cases cover GLib `--help`, an unknown option, extra arguments (the
pin never reads `argc`), a missing `table.conf`, a missing chunk (the
`ENOENT`/`ENODATA` split between `gen_ngram` and `gen_deleted_ngram`), a
missing/unopenable bigram input, empty stdin, a segmented line that does
not split (`abort()`, registered SIGABRT with `reproduces_signal`), a
token whose phrase differs (unit tests; the glibc assertion text is
policy-accepted), ordinary runs, `--skip-pi-gram-training`, estimate's
EM over the committed `deleted_bigram.db` (`fixtures/w3/<ext>/README.md`),
and eval's pass, mismatch, empty-corpus (`-nan`), missing-corpus and
missing-bigram runs.

The bigram container bytes are not observable through the runner: the pin
and the subject each open/modify a private copy, and the container carries
the writing library's own header (a random file id on Berkeley DB, a
timestamp on tkrzw), so even two pin runs differ. `gen_ngram` and
`gen_deleted_ngram` cases therefore direct the bigram at an absent
directory (the pin ignores a failed `attach`) and compare the chunk
files, stdout/stderr and exit; the row-level writes are covered by
`crates/oxpinyin-utils/tests/training.rs`, including its ignored live
differential, which compares the decoded rows with the pin's.

On the tkrzw reference cell all 40 cases report MATCH with the sound
`libtkrzw` (`PATH=/home/sheng/.cache/oxpinyin/tkrzw/bin:$PATH
LD_LIBRARY_PATH=/home/sheng/.cache/oxpinyin/tkrzw/lib`); the kc cell's 40
are MATCH as well, and the existing 45 non-generator cases stay 78
MATCH plus 7 POLICY-ACCEPTED there with the new ones included (85 cases).
On the bdb cell the failed-`attach` cases and the empty-file cases differ
only in Berkeley DB's own error log (`BDB1565`/`BDB0004`): the pin leaves
an unopened handle whose destructor calls `sync`/`close`, while
`oxpinyin-store` reports the failed open and stops; those lines are
library diagnostics, not the utilities' output. No policy record was
created for them, and the reference cell carries no such lines.

The named mutation `ngram-unigram-two` changes only
`gen_ngram::train`'s `libraries.add_unigram(cur_token, 1)` to `2`. The
tkrzw run detects it in `gen_ngram-missing-bigram-file`,
`gen_ngram-ordinary` and `gen_ngram-skip-pi-gram` (3 DIFFER, 37 MATCH).
The source was restored byte for byte (sha256 checked) before the
completion run, which returned the 40 MATCH above.
