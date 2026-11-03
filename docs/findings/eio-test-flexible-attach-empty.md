# FlexibleBigram test-program attach and magic headers

The DBM test-program port uses the shared `FlexibleBigram` attach on the
backend implementation already landed on main. The pin's behavior below is
from libpinyin `074a2219`.

`tests/storage/test_flexible_ngram.cpp:59-60` attaches `/tmp/training.db`
with read-write and create flags and aborts if attach returns false. The
backend paths differ:

- `src/storage/flexible_ngram_tkrzwdb.h:128-187` first opens without
  create, retries with create when requested, and deletes the object after
  a final failure. The signature write uses native `Set`.
- `src/storage/flexible_ngram_kyotodb.h:129-179` first opens without
  `OCREATE`, retries the same object when requested, and retains that object
  after a failed retry. The signature write uses native `set`.
- `src/storage/flexible_ngram_bdb.h:94-152` first opens without `DB_CREATE`
  and retries with it when requested. The pin reuses a failed-open handle.
  The landed BDB implementation closes failed handles under the
  [external API misuse ruling](compatibility-policy.md#bdb-failed-handles),
  retaining the recorded library-diagnostic exception. Its `attach_hash`
  calls `Db::open_attach`, whose `open_steps` checks the supported runtime
  version before creating a handle.

For an existing empty file, KC and tkrzw can initialize it on the first
open, then refuse its missing signature. The test aborts without closing
it. BDB's create retry can instead succeed. The tkrzw abort case registers
its specific unclosed-container timestamp in
[the timestamp finding](eio-test-dbm-abort-timestamp.md).

## Magic-header reads

At `flexible_ngram_kyotodb.h:282-308`, KC reads into a buffer sized for
the magic and header. A signature-only value returns false; a missing,
short or overlong value, or a wrong magic in a complete value, reaches an
assertion. `flexible_ngram_tkrzwdb.h:292-323` returns false on a failed
read, but asserts on an invalid size or wrong magic after a successful read.
`flexible_ngram_bdb.h:302-328` reads only the header at offset four with
`DB_DBT_PARTIAL` and returns false when it is short.

The KC bounded read uses the existing CXX bridge's read-only `accept`
visitor. Kyoto Cabinet 1.2.80 `kcdb.h:1985-2012` implements its bounded
`get` with the same `accept` call and a visitor that copies at most the
buffer size and reports the whole record length. The Rust sink does the
same under the visitor's lock, without adding a C shim. For header size H,
it copies at most 4 + H bytes and uses O(H) temporary space; database
lookup cost is the native backend's. An error or missing record reaches the
same KC assertion. The tkrzw and BDB paths use their existing native reads.

The test tools use a plain native KC or tkrzw setter for their writes;
shipping `WriteStore::write` retains its durability behavior. The shared
flexible bigram uses the landed deferred-write path. The case files compare
complete output and file inventories, with only the documented assertion
text and named tkrzw timestamp exceptions.
