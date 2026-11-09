# FlexibleBigram item search and the unsorted KC fixture

Status: source audit, 2026-11-01 UTC · PRs #1485 and #1487.
Pin: libpinyin `074a2219c90feaf962d0d24f034514033ece5f99`, read with
`git show 074a2219:<path>` in the local libpinyin clone.

## Search

`SingleGram::lower_bound` in `crates/oxpinyin-store/src/flexible_ngram.rs`
calls Rust's `slice::partition_point` with `entry.token < token`.
Range `search` (`flexible_single_gram.h:118-139`) uses the same lower bound.
It does not transcribe the probes of libstdc++. The linear scans after it are
libpinyin's own logic (`src/storage/flexible_single_gram.h:151-181,
193-221,232-257,268-290`). The comparator is the pin's unsigned token
order (`:63-65`). Keep the standard API and these product operations.
On sorted records, the result is the first token not less than the query.
Thus get, set and remove address the same first equal record as the
pin, also for duplicate tokens. Do not sort or clean loaded records.

For n records, partitioning takes O(log n) comparisons and O(1) temporary
space. On sorted records, the scan after it examines at most one record.
Insertion and removal shift O(n) records. Lookup and replacement are O(log n).
On unpartitioned input, the scan can visit O(n) records. The prior code and
the pin have the same bounds. No measurement is necessary.

## Producers and malformed input

The pin's empty constructor starts with only the header
(`flexible_single_gram.h:75-78`). The operations keep the order:

- Insertion finds the lower bound of the token and inserts before the first
  larger token (`:151-181`).
- Removal keeps the order of the remaining records (`:193-221`).
- Replacement changes only the item (`:268-290`).

Thus these operations keep a gram sorted from an empty gram.
These producers use this insertion:

- The importer, also when text rows arrive out of order
  (`utils/training/import_k_mixture_model.cpp:214-233`).
- Training, for new successor tokens (`gen_k_mixture_model.cpp:195-202`).
- Merge, which builds a new gram through it (`merge_k_mixture_model.cpp:180-189`).

Backend loads keep the stored bytes and do not validate the order
(`flexible_ngram_bdb.h:165-187`, `flexible_ngram_kyotodb.h:192-215`,
`flexible_ngram_tkrzwdb.h:196-219`). Thus a malformed gram that already
exists can propagate. Normal insertion does not make an unsorted
array from sorted input.

The committed `fixtures/w3/kct/k_mixture_model.db` has successor tokens
`16817937, 16802309` in its sentence-start row. The matching bdb and
tkrzw fixtures have `16802309, 16817937`. The existing `dbm_rows` helper read
the stored rows to verify this; the pin did not run.
The fixture README describes the manual `kchashmgr set -sx` construction of KC.
These bytes keep the input order of the text model, not the pin's
insertion order. They are malformed stored input, not search semantics that
the pin generates. The fixture stays unchanged.

## Disposition

`kmm-estimate-good` on KC exercises that malformed row. The estimator calls
`get_array_item` from `compute_interpolation`
(`utils/training/estimate_k_mixture_model.cpp:53-85`). Neither the C++ lower
bound nor Rust partitioning guarantees a result on an unpartitioned range.
The 2026-10-29 source-policy ruling forbids reproducing a library's probe
sequence to get one. Any distinguishing result is a language artefact.
Such a result is to be kept as POLICY-ACCEPTED under this named record, with
its concrete pin and subject evidence. Sorted controls must still match exactly.

Recorded evidence: the selected `kmm-estimate-good` verification on
2026-11-01 UTC is MATCH on bdb, kc and sound tkrzw. It showed no
distinguishing unsorted result, so no new POLICY-ACCEPTED observation is
bound. The current API is the prescribed Rust API. The malformed KC case
matches, but that does not make its answer a contract.

The file format, stored order, C ABI, ownership, backend I/O and diagnostic
calls stay unchanged. This audit does not change a golden or a fixture.
