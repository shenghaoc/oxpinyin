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
`tools/oracle/build-oracle.sh --libs-only`, then its two utility targets in
that build tree. Cell artifact selection builds `oxpinyin-utils` once and
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
