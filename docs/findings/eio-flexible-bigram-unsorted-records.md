# FlexibleBigram item search and the unsorted KC fixture

Status: source audit, 2026-11-01 UTC · PRs #1485 and #1487.
Pin: libpinyin `074a2219c90feaf962d0d24f034514033ece5f99`, read with
`git show 074a2219:<path>` in the local libpinyin clone.

`SingleGram::lower_bound` in `crates/oxpinyin-store/src/flexible_ngram.rs`
already calls Rust's `slice::partition_point` with `entry.token < token`.
Range `search` (`flexible_single_gram.h:118-139`) shares that same lower bound.
It does not transcribe libstdc++'s probes. The following linear scans are
libpinyin's own logic (`src/storage/flexible_single_gram.h:151-181,
193-221,232-257,268-290`), and the comparator is the pin's unsigned token
order (`:63-65`). Keep the standard API and these product operations.
On sorted records the result is the first token not less than the query;
get, set and remove therefore address the same first equal record as the
pin, including duplicate tokens. Do not sort or clean loaded records.

For n records, partitioning takes O(log n) comparisons and O(1) temporary
space. On sorted records the following scan examines at most one record.
Insertion/removal shifts O(n) records; lookup/replacement is O(log n).
On unpartitioned input the scan can visit O(n) records. These are the same
bounds as the previous code and the pin. No measurement is needed.

## Producers and malformed input

The pin's empty constructor starts with only the header
(`flexible_single_gram.h:75-78`). Insertion finds the token's lower bound
and inserts before the first larger token (`:151-181`); removal preserves
the remaining order (`:193-221`); replacement changes only the item
(`:268-290`). Thus these operations preserve sorting from an empty gram.
The importer uses this insertion even when text rows arrive out of order
(`utils/training/import_k_mixture_model.cpp:214-233`). Training uses it for
new successor tokens (`gen_k_mixture_model.cpp:195-202`), and merge builds a
fresh gram through it (`merge_k_mixture_model.cpp:180-189`). Backend loads
retain the stored bytes, without validating order
(`flexible_ngram_bdb.h:165-187`, `flexible_ngram_kyotodb.h:192-215`,
`flexible_ngram_tkrzwdb.h:196-219`). A malformed pre-existing gram can
therefore propagate, but normal insertion does not produce an unsorted
array from sorted input.

The committed `fixtures/w3/kct/k_mixture_model.db` has successor tokens
`16817937, 16802309` in its sentence-start row. The corresponding bdb and
tkrzw fixtures have `16802309, 16817937`. This was verified by reading the
stored rows with the existing `dbm_rows` helper, without executing the pin.
The fixture README describes KC's manual `kchashmgr set -sx` construction;
these bytes preserve the text model's input order instead of the pin's
insertion order. They are malformed stored input, not pin-generated
search semantics. The fixture is retained unchanged.

## Disposition

`kmm-estimate-good` on KC exercises that malformed row. The estimator calls
`get_array_item` from `compute_interpolation`
(`utils/training/estimate_k_mixture_model.cpp:53-85`). Neither C++ lower
bound nor Rust partitioning guarantees a result on an unpartitioned range.
The 2026-10-29 source-policy ruling forbids reproducing a library's probe
sequence to obtain one. Any distinguishing result is a language artefact,
to be retained as POLICY-ACCEPTED under this named record with its concrete
pin/subject evidence; sorted controls must still match exactly. The selected `kmm-estimate-good` verification on 2026-11-01 UTC is MATCH
on bdb, kc and sound tkrzw. No distinguishing unsorted result was observed,
so there is no new POLICY-ACCEPTED observation to bind. The current API is
already the prescribed Rust API; the matching malformed KC case does not
turn its answer into a contract.

The file format, stored order, C ABI, ownership, backend I/O, and diagnostic
calls remain unchanged. This audit does not alter a golden or a fixture.
