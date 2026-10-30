# 1297-chain-partition — standard-library search of carried entries

Status: **retained as normal parity on the registered cases** ·
2026-10-29T09:47:05Z (`date -u`) · Refs #1297.

The 2026-10-29 UTC “No ported external-library internals” ruling in
[Source policy](../source-policy.md), merged in #1402,
requires std searches. The [inventory](glib-libc-inventory.md#executable-copies-of-external-library-algorithms-2026-10-29-utc)
identifies carried pinyin entry searches as D. The pin at `074a2219c90feaf962d0d24f034514033ece5f99` was read
with `git show 074a2219:<path>` from the local libpinyin repository:
`src/storage/chewing_large_table2.h:208–255` searches exact keys before
its add/remove token walks; `src/storage/pinyin_phrase3.h:33–65` defines
the key precedence. All initials across the phrase precede middle/final
pairs per syllable, which precede all tones. Token and padding are not
part of this comparison.

`pinyin_chain::Entry::equal_range` dispatches lengths 1–16 over their
eight bounded strides to `as_chunks::<N>()`. Two `partition_point` calls
search whole borrowed records; the upper bound is taken over the suffix
after the lower bound. The comparator reads packed keys in place. There
is no allocation per probe or pre-decoded search array. Time remains
O(L log n), while auxiliary allocation changes from an O(L) Vec per probe
to O(1). Trailing bytes remain outside the whole-record view and stay
where the existing splice/drain operations leave them.

Both facades reach Entry add/remove through IndexKeys note_add/note_remove,
which guard lengths 1–16. Only those edits consume this equality range.
mask_walk and file_tails retain the carried bytes. Ordinary derived rows
use table_entries::exact_compare2. BDB has no carried-entry path.

Sorted records retain equality-range semantics, including duplicate keys,
empty input and misses at both ends. A KC/tkrzw mask walk can retain an
older record's tail behind a shorter entry and violate the ordering
precondition. Such input uses std's unspecified result for Rust 1.97.1;
no storage is sorted or cleaned. A future observable difference would
need its own discrepancy record and registration.

All 32 existing #785 carried-entry cases, including both facades, MATCH
on bdb, kc and sound tkrzw; the four phrase-table #785 cases also MATCH.
D has no observed movement in that set. This ordinary finding documents
the replacement's contract and parity evidence; it has no divergence-register
entry, case `policy_record` link or digest block. `just policy-record`
records differences and rejects parity pairs.

D's user-crate gates run at D with the kyotocabinet feature, which compiles
pinyin_chain: 230 unit tests and 23 integration tests pass, as does strict
Clippy. Tests cover every supported length/stride, sorted duplicate keys,
both-end misses, empty records, all partial-tail lengths, stored-order
preservation, packed-key field precedence, and ignored padding bits.

C ABI/layout, ownership/lifetime, GArray/g_free, persistent formats and
ownership, diagnostics and process-wide effects are unchanged. No prior
ruling is reversed; the facade total remains awaiting ruling. Time,
allocation, instruction and stripped-size comparisons are reported with
the PR using the existing measurement workers. No raw captures are committed.
