# DBM test abort outputs retain a clock timestamp

Status: maintainer-approved clock metadata normalization, 2026-11-01 UTC
(`date -u`). This is not a POLICY-ACCEPTED divergence. Every byte outside
the eight-byte timestamp field remains compared exactly.

## Source and approved mutation path

At libpinyin 074a2219, `src/storage/ngram_tkrzwdb.cpp:124-132` and
`src/storage/punct_table_tkrzwdb.cpp:139-148` call plain native `Set`,
without synchronizing. A nonempty directory at `/tmp/snapshot.db` makes
`RemoveFile` fail (`ngram_tkrzwdb.cpp:65-83`,
`punct_table_tkrzwdb.cpp:96-112`). The test then aborts at
`tests/storage/test_ngram.cpp:78` or `test_punct_table.cpp:63` while the
attached database is still open.

The maintainer approved a tools-only native tkrzw mutation path, with
shipping `WriteStore::write` and its durability unchanged. The checkpoint
adds `TkrzwStore::tool_set` under the `tools` feature and routes these two
test programs through the existing `db_set_record` native setter. The
nonshipping utils crate enables that feature. Existing utility callers
keep their previous write path. `required` calls the existing
`oxpinyin_input::tool::abort` (`std::process::abort`) directly on save failure;
the attached handle stays in the caller, and no unwind, `Drop`, close or
synchronize runs before the abort. The successful save/load path still
closes the old handle after the save.

## Captured witnesses

The source-defined `ngram-snapshot-directory` and
`punct-table-snapshot-directory` cases were run through the existing
`tool-diff` runner with `dbm_rows` disabled in an inspection copy of those
two cases. Both sides used the sound cached tkrzw library at
9ee84164cf45e441aa8186418c03a02c9b89f08b. Both exit with SIGABRT (-6), with
empty stdout. The pin's assertion stderr and the subject's empty stderr
follow the existing [check-message ruling](eio-tool-check-messages.md).
That separate assertion-text exception binds the complete observations
after the clock normalization described below.

| Case | File | Pin and subject length | Differing byte offsets (zero based) |
| --- | --- | ---: | --- |
| `ngram-snapshot-directory` | `tmp/test.db` | 8,396,800 | 53, 54, 55 |
| `punct-table-snapshot-directory` | `tmp/punct.bin` | 529,408 | 53, 54, 55 |

Every other byte, file and directory entry matches. These offsets are
inside the eight-byte timestamp at offset 48:
[`tkrzw_dbm_hash.cc:43,1143,1252`](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_dbm_hash.cc#L43)
defines the field, assigns `GetWallTime() * 1000000`, and writes it.
The earlier committed-record-count, effective-size and file-length
differences disappear with the approved unsynchronized setter.

Checkpoint verification on tkrzw: fmt and clippy (`oxpinyin-store` and
`oxpinyin-utils`, all targets, warnings denied) pass. Tests for
`oxpinyin-input`, `oxpinyin-store` and `oxpinyin-utils` pass: 170 passed,
8 existing ignored. The neighboring `ngram-fresh`,
`flexible-ngram-fresh` and `punct-table-fresh` cases report 3 MATCH.
Before the approved normalization, the two raw abort observations were
2 INVALID, not a passed gate.
Builds, tests, clippy and differential runs used `just heavy`.

This residual difference is clock metadata, not evidence of a PolyDBM
versus direct-class difference. The pin constructs `HashDBM` and
`TreeDBM` directly (`ngram_tkrzwdb.cpp:96`,
`punct_table_tkrzwdb.cpp:70`). The earlier C API adapter constructed PolyDBM
([`tkrzw_langc.cc:498-535`](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_langc.cc#L498)),
which constructs the selected HashDBM/TreeDBM with a file from
[`tkrzw_dbm_poly.cc:371-405`](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_dbm_poly.cc#L371).
The default file is `MemoryMapParallelFile`
([`tkrzw_file_poly.cc:165-172`](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_file_poly.cc#L165)),
also used by both direct constructors
([`tkrzw_dbm_hash.cc:2332-2334`](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_dbm_hash.cc#L2332),
[`tkrzw_dbm_tree.cc:2473-2475`](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_dbm_tree.cc#L2473)).
The PolyDBM header's default-file prose is stale at this revision; the
implementation above is the evidence. Main now binds the concrete classes
through CXX; the timestamp normalization remains limited to observed aborts.

## Comparator scope

The existing row normalizer refuses these unclosed files with `invalid
magic checksum`. The approved `tkrzw_abort_timestamp` case-file list
therefore names `tmp/test.db` in `ngram-snapshot-directory` and
`tmp/punct.bin` in `punct-table-snapshot-directory`. On the tkrzw cell,
the comparator recognizes the HashDBM header and zeroes exactly bytes
48 through 55 of each named file. It preserves every other byte and the
complete file inventory. TreeDBM uses this HashDBM on-disk header too.

The 2026-11-02 UTC shared-attach checkpoint also names `tmp/training.db`
in `flexible-ngram-empty-file`. The successful native no-create open
initializes an existing empty HashDBM, but its missing signature makes
attach return false (`flexible_ngram_tkrzwdb.h:147-177` at 074a2219).
The test aborts at `test_flexible_ngram.cpp:60` without closing that
handle. It follows the same unclosed HashDBM path; exactly the same
eight-byte clock field is normalized, and every other file byte matches.

The field is available only for named cases that register reproduced
SIGABRT and only when the individual observation actually ends in SIGABRT. Later cases leaving an unclosed HashDBM by the same path must
name their files explicitly. Other cells, non-HashDBM files, unnamed
files and short headers are untouched. Tests cover the whole eight-byte
field, differences at offsets 47 and 56, other cells and other files.
Equal normalized observations remain MATCH; the normalizer does not
create an acceptance record. No adapter or shipping durability change
is included.

The comparator harness passes 34 tests, including the new boundary and
scope checks; the policy writer passes its 7 tests. Both named tkrzw
abort witnesses and the subsequent empty-file witness pass after binding
their assertion-text-only records (3 POLICY-ACCEPTED for that existing
language artefact). The full tkrzw C plus K-mixture gate is 50 MATCH and
9 POLICY-ACCEPTED. The subsequent [shared attach and KC corrections](eio-test-flexible-attach-empty.md)
resolve the backend operation and per-cell record gaps without any
additional byte normalization.
