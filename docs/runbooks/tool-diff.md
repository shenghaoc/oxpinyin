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

The registered table generators and `k_mixture_model_to_interpolation`
open no DBM: `DATABASE_FREE` in `tools/dev/tool_diff.py` names them, and
each capture row carries `database_free` from it. The three segmentation
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
