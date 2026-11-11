# Utility check macros write no message

Status: accepted language artefact, 2026-10-31 UTC. Disposition:
POLICY-ACCEPTED. Rule: [source policy, external I/O](../source-policy.md#external-io);
record form: [template](language-runtime-discrepancy-template.md).
Pin: libpinyin 074a2219 (`utils/training/k_mixture_model_to_interpolation.cpp`
blob `957765f4`, `src/storage/tag_utility.cpp` blob `232a07dc`).

## k_mixture_model_to_interpolation

When `taglib_read` refuses a section line, `check_result` fails. In the
reference build, `check_result` is `assert` (`src/include/pinyin_utils.h:26-31`).
The pin writes glibc's assertion line and then aborts. Where a value is
missing, the pin first writes the `g_return_val_if_fail` critical of GLib
(`tag_utility.cpp:202`, `:246`). These cases reach the abort:

- `kti-unknown-line-assert`: an unknown line in `\1-gram` (`:119`).
- `kti-missing-value-assert`: `\item` without its phrase (`:202`, then
  `:119`).
- `kti-bigram-assert`: a `\2-gram` item without `count` (`:256` warning,
  then `:158`).
- `kti-last-line-reread-assert`: a last line without a newline, which the
  body reads again (`:88`).

oxpinyin aborts on the same condition and writes neither message. GLib
warnings from the pin's own code stay. Stdout, files and SIGABRT match
(`reproduces_signal`).

## Other cases that bind only the assertion text

The family C tkrzw `ngram-snapshot-directory` and
`punct-table-snapshot-directory` records bind only this assertion-text
exception. Their files compare exactly after the approved
HashDBM clock timestamp normalization.
The timestamp normalization itself is not a POLICY-ACCEPTED divergence.

The KC snapshot-abort and K-mixture existing-empty witnesses also bind
only assertion text, after the 2026-11-02 UTC native-write and bounded-read
corrections. Their exit, stdout and complete files match without a KC
mask. The WAL/file and missing-abort differences are corrected, not
accepted. See [the shared attach finding](eio-test-flexible-attach-empty.md).
For `kmm-prune-existing-empty` on BDB, this same case entry binds a
different exception: the separately accepted failed-open-handle diagnostic under
[register row 113](compatibility-policy.md#bdb-failed-handles). Its other
output and files match.

The family E1 `memory-chunk-save-directory` case reaches
`tests/include/test_memory_chunk.cpp:61` (074a2219). There,
`check_result(save(...))` asserts after the open of `/tmp/test.bin` fails.
Both sides abort, keep the input directory and lose unflushed C stdout.
Only the assertion text differs.

The family E2 `phrase-index-logger-refused-layout` case uses a valid
27-byte frame whose payload separator is `!`, not `#`.
`SubPhraseIndex::load` rejects it with `g_return_val_if_fail`
(`phrase_index.cpp:354` at 074a2219). Both programs complete normally and
print cached totals 0 and -7. Only the critical messages of that macro differ.
The separate stored local total of the source stays 7 through the refused load.

The E3 `chewing-table-overlong-toned-first` witness is defined. The tone
assertion of the first incomplete key (`pinyin_phrase3.h:146-157` at 074a2219)
stops the 17-key query before either out-of-bounds index copy. Both sides
abort there, and only the assertion message differs. The plain 17-key query
still uses the source-only row 116 refusal.

Each case is accepted on every cell unless its test says otherwise:
`flexible-ngram-empty-file` on kc and tkrzw, `kmm-gen-existing-empty` on kc,
and `kmm-prune-existing-empty` on bdb and kc. The frozen utility cases in
`crates/oxpinyin-utils/tests/` hold oxpinyin's side, name this record and
state their cells.

## Config test driver (2026-11-01 UTC)

`table-info-save-fails` makes `/tmp/user.conf` a directory. The pin's
`UserTableInfo::save` writes `write /tmp/user.conf failed.\n` and returns
false (`src/storage/table_info.cpp:377-385` at `074a2219`).
`check_result` in `tests/storage/test_table_info.cpp:93` then aborts.
oxpinyin preserves that diagnostic and SIGABRT without imitating the
assertion text. The `table_info_save_fails` case in
`crates/oxpinyin-utils/tests/storage_tools.rs` holds the complete streams
and files.
