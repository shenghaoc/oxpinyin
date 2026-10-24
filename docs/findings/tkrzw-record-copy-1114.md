# tkrzw user-bigram record copy (#1114)

Source preparation, 2026-10-23 UTC; reconciliation 2026-10-24 UTC
(`date -u` on this host). Parent PR #1110 merged with final head
`d9227a32c55f7be5b24c24e8fbc463d164ad3986`; its branch was deleted.
The two owned commits were rebased onto landing tip
`2a0f5748646b9fdaa5bac22a350670794deb1319`, which contains that parent.
No provisioning,
build, oracle run, instruction collection, timing or memory measurement
has run for this change. The parent released the heavy-work queue after
#1031 stopped. Setup remains unstarted: `tools/cloud/setup.sh` always
redirects its tkrzw host probe to `tkrzw-host-probe.log`, and other build
paths also write logs. It has no fileless-diagnostics option. Running it
as written would violate the task's explicit no-log-file instruction;
no recipe workaround or logging change has been applied.

## Source and ordering proof

The pinned libpinyin source was read with `git show 074a2219:PATH` in
`/home/sheng/repos/libpinyin`. The tkrzw source was read from upstream
at `9ee84164cf45e441aa8186418c03a02c9b89f08b`, which is the revision
recorded by the host's cached runtime. These are source conclusions,
not gate results.

At the pin, `src/storage/ngram_tkrzwdb.cpp:48-63` constructs a fresh
default `TinyDBM` and copies a read-only `HashDBM` through
`src/storage/tkrzwdb_utils.h:48-74`: `First`, then `Get`, `Set`, `Next`
for every row. In oxpinyin, `persistence::load_user_bigram_db` delegates
to `TkrzwUserBigramDb::load_db`. The latter retains its source iterator
and #1110's final `holds_no_record` check, including the independent
file-size guard. Only its destination insertion changes.

The previous destination path builds owned key/value vectors and a
single-element `ProcessMulti` batch. The new `db_set_record` lends the
callback's slices to `tkrzw_dbm_set` with validated lengths and overwrite
enabled. The slices stay alive until the synchronous destination call
has copied them. Neither path retains a reference into the source.
Transaction batches, ordinary stores, removals and saves keep their
existing paths.

In tkrzw, `tkrzw_langc.cc:684-703` sends `tkrzw_dbm_set` through
`PolyDBM::Set` (`tkrzw_dbm_poly.cc:604-609`) to `DBM::Set`.
`DBM::RecordProcessorSet` returns the supplied value on both a miss and
an overwrite (`tkrzw_dbm.h:178-205,1123-1132`). `TinyDBMImpl::Process`
and `ProcessMulti` reach the same `ProcessImpl` with the same key,
bucket, value and writable flag (`tkrzw_dbm_tiny.cc:279-311`).

Let S be the source iterator's record sequence. HashDBM yields buckets
in increasing index and uses a `std::set` of byte-string keys within
each bucket (`tkrzw_dbm_hash.cc:228,1836-1886`). Both implementations
consume exactly S; neither sorts, batches across rows or changes the
source bucket count. For any destination bucket b, let S_b be S filtered
by the destination's hash. Each previously absent key prepends to b
(`tkrzw_dbm_tiny.cc:674-686`). Induction over S therefore gives the same
records and the same chain reverse(S_b) after each insertion. This also
covers source files whose bucket count differs from TinyDBM's default:
both paths still consume the same S and filter with the same destination
hash. An overwrite changes the value without changing chain position
(`:638-669`). Native iteration follows bucket then chain (`:730-755`),
so the later save receives the same ordered record sequence, not merely
the same record set.

`loading_preserves_the_pins_colliding_bucket_chain_and_values` uses
three four-byte keys derived from `HashMurmur`'s four-byte case and
`PrimaryHash`'s fold (`tkrzw_hash_util.cc:27-58`,
`tkrzw_dbm_common_impl.h:36-44`). Tokens `0x01000d46`, `0x01004404`,
and `0x01004dd8` hash to default bucket 74121. The expected chain is
`0x01004dd8`, `0x01000d46`, `0x01004404`. The test checks that sequence
and all values against the previous insertion path, including an empty
value, embedded NUL bytes and an 804-byte value. Those are opaque-copy
test inputs, not a claimed realistic trained-profile distribution.
The test is written but has not run.

Both paths retain O(n) record storage. The new path removes temporary
owned key/value copies and batch allocations; it introduces no retained
structure. Time and memory still require same-run measurements. Any
time-for-memory trade is a STOP.

## Saved-byte comparison

The maintainer's clarified ruling authorizes exactly zero-based bytes
48-55: the eight-byte `META_OFFSET_TIMESTAMP` field. It supersedes the
earlier wording about bytes 53-56. Every other byte and bucket-chain
order must match.

The source field is `HashDBMImpl::timestamp_`, located by
`META_OFFSET_TIMESTAMP = 48` (`tkrzw_dbm_hash.cc:46`).
`SaveMetadata` serializes eight bytes there (`:1296`), so the complete
field spans zero-based offsets 48-55. Creation, synchronization and close
assign `GetWallTime() * 1000000` (`:749,1184,1241`).
`META_OFFSET_DB_TYPE = 56` (`:47`), so zero-based offset 56 belongs to
`db_type_`, not the timestamp. It remains exact, as do cyclic-magic bytes,
the rest of the header, bucket pointers, record links and payloads.
This authorization masks no other field or file.

`contract-diff.py::tkrzw_saved_bigram_bytes` masks only the named field;
`tkrzw_saved_bigram_equal` additionally requires exact ordered bucket
chains. Neither changes an existing contract's file-comparison rules.
The chains must be supplied from native iteration, never sorted row
sets. The Rust collision regression separately checks that native walk.

The six `TkrzwSavedBigramTests` pass without builds or oracle calls.
They flip every bit at every outside-field byte in a synthetic 512-byte
buffer (4,032 rejected mutations), explicitly check offset 56 and both
cyclic-magic bytes, require length identity, reject truncated metadata,
accept changes confined to 48-55, and reject chain permutations even
when they contain the same keys and the saved bytes match. Command:

```sh
UV_CACHE_DIR=/home/sheng/Documents/Codex/2026-10-23/task-19/uv-cache \
TMPDIR=/home/sheng/Documents/Codex/2026-10-23/task-19/tmp \
uv run --offline --no-project --managed-python --python cpython@3.15+gil \
  python -B -X lazy_imports=normal tools/bisection/contract-diff.test.py \
  TkrzwSavedBigramTests
```

Results were printed directly; no capture was created. Pin to subject
and subject to pin reload/save checks are pending admission. Logical-row
equality or file presence cannot substitute for either.

## Trained-size fixture and remaining gates

A `SingleGram` consists of a four-byte total and eight bytes per
distinct successor (`src/storage/ngram.cpp:31-40` at 074a2219).
Training creates or loads the user row and adds a successor on its
first occurrence (`src/lookup/phonetic_lookup.h:876-909`). Thus trained
value size is 4 + 8d for successor degree d, rather than always 12.
Generate a deterministic profile from public text through the existing
training path, then report its degree and byte-size distribution and
recipe. Do not substitute repeated independent 12-byte rows, or access
private user profiles. Fixture generation has not run.

After admission: run setup for bdb, kc and tkrzw; STOP on setup or GNU
ld 2.47 compatibility-image failure. Build ordered subjects against the
verified runtime and check every measured resolved path with `findmnt -T`.
Collect trained-profile init attribution before and after the change.
Run all three backend gates, tkrzw contracts and same-data checks, plus
touched-crate tests, fmt and clippy. Measure base/PR/pin together at 0,
10,000 and 100,000 records and on the trained fixture, with exact Ir
and heap controls and the existing timing controls. No relaxed controls.
Observations remain in memory; create no archives, logs, captures or
evidence files. Publish a template-based draft PR with Fixes #1114,
verify exact-head CI and STOP without reviews, merging or draft changes.
