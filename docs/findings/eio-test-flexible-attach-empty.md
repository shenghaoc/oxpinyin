# FlexibleBigram test-program attach and magic headers

The DBM test-program port uses the shared `FlexibleBigram` attach on the
backend implementation on main. The pin behaviour below is
from libpinyin `074a2219`.

`tests/storage/test_flexible_ngram.cpp:59-60` attaches `/tmp/training.db`
with read-write and create flags. It aborts if attach returns false. The
backend paths differ:

- `src/storage/flexible_ngram_tkrzwdb.h:128-187` first opens without
  create. It retries with create when requested, and it deletes the object
  after a final failure. The signature write uses native `Set`.
- `src/storage/flexible_ngram_kyotodb.h:129-179` first opens without
  `OCREATE`. It retries the same object when requested, and it keeps that
  object after a failed retry. The signature write uses native `set`.
- `src/storage/flexible_ngram_bdb.h:94-152` first opens without `DB_CREATE`,
  and retries with it when requested. The pin reuses a failed-open handle.
  The BDB implementation on main closes failed handles under the
  [external API misuse ruling](compatibility-policy.md#bdb-failed-handles).
  It keeps the recorded library-diagnostic exception. Its `attach_hash`
  calls `Db::open_attach`. The `open_steps` of that call checks the supported
  runtime version before it creates a handle.

For an existing empty file, KC and tkrzw can initialize it on the first
open, then refuse its missing signature. The test aborts and does not close
it. The BDB create retry can succeed instead. The tkrzw abort case leaves an
unclosed container with a clock timestamp. The
[tkrzw HashDBM clock field](compatibility-policy.md#dispositions-and-discrepancy-records)
rule of the compatibility policy governs its comparison.

## Magic-header reads

The backends read the magic header as follows:

- KC (`flexible_ngram_kyotodb.h:282-308`) reads into a buffer sized for
  the magic and header. A signature-only value returns false. A missing,
  short or overlong value reaches an assertion. A wrong magic in a complete
  value also reaches an assertion.
- tkrzw (`flexible_ngram_tkrzwdb.h:292-323`) returns false on a failed
  read. After a successful read, it asserts on an invalid size or a wrong magic.
- BDB (`flexible_ngram_bdb.h:302-328`) reads only the header at offset four with
  `DB_DBT_PARTIAL`. It returns false when the header is short.

The KC bounded read uses the read-only `accept` visitor of the existing CXX
bridge. Kyoto Cabinet 1.2.80 (`kcdb.h:1985-2012`) implements its bounded
`get` with the same `accept` call. Its visitor copies at most the
buffer size and reports the whole record length. The Rust sink does the
same under the lock of the visitor, and adds no C shim. For header size H,
it copies at most 4 + H bytes and uses O(H) temporary space. The database
lookup cost is that of the native backend. An error or missing record reaches the
same KC assertion. The tkrzw and BDB paths use their existing native reads.

The test tools use a plain native KC or tkrzw setter for their writes.
The shipping `WriteStore::write` keeps its durability behaviour. The shared
flexible bigram uses the deferred-write path on main. The case files compare
complete output and file inventories. The only exceptions are the documented
assertion text and the named tkrzw timestamps.
