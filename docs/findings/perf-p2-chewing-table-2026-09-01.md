# P2 performance findings — lazy ChewingTable vs eager PinyinIndex

Date: 2026-09-01 · Status: **measured (mini fixtures; full-data expected
to show the architectural benefit)**

## The architectural change

P2 replaces the eager `PinyinIndex` (sorted `Vec` of all keys + arena of
all entries, materialized at `SystemDictionary::open`) with a lazy
`ChewingTable` (DBM handle opened at construction, per-keystroke `Get`s
at lookup time — the same architecture libpinyin uses).

The eager path's cost was established by the #260 baseline matrix
(`perf-backend-matrix-2026-08-31.md`): **~86 ms** for
`SystemDictionary::open` on full export data with KC, dominated by
iterating ~100K+ pinyin-index records and resolving them into the entry
arena. libpinyin's own init is ~0.9 ms for the same data, because its
`attach` opens the DBM handle without scanning.

## Mini-fixture measurement (10 keys)

```
cargo run --features redb --release --example load_profile -- chewing
```

| Path                    | Init time (release) | RssAnon |
|------------------------|--------------------:|--------:|
| ChewingDictionary::open |            0.3 ms   | 392 KiB |
| SystemDictionary::open  |            0.3 ms   | 448 KiB |

With 10 keys the dataset fits in a handful of cache lines — the eager
scan barely registers. The 56 KiB RSS difference is the absence of the
`PinyinIndex` arena (rows + entries).

## Expected full-data effect

On full export data (~100K+ pinyin-index records):

| Metric                         | Eager (measured #260) | P2 lazy (expected) |
|-------------------------------|----------------------:|-------------------:|
| pinyin_index init              |             ~86 ms    |          ~0.3 ms   |
| total SystemDictionary::open   |             ~86 ms    |        ~phrase_idx |
| RssAnon from pinyin index      |            ~3-5 MiB   |        ~file open  |
| per-lookup latency             |   O(log n) Vec search |    O(log n) DBM get|

The P2 path eliminates the pinyin-index contribution to init entirely.
The remaining init time is the phrase_index load (P3 scope) and the
interpolation2 parse.

## What is NOT measured yet

- KC/Tkrzw backends (not available in this CI container; measured on
  the oracle host)
- Same-data-directory comparison against the libpinyin oracle
- Full-data RSS and HWM profiles
- Steady-state lookup latency on the full dataset
- The combined effect with P3 (phrase DBM) and P4 (bigram/punt)

## Regression protection

The `ChewingDictionary::open` constructor does not call `for_each`,
`range`, or any scan method on the pinyin-index store. It calls
`open_read_only` (one file open) and wraps the handle in a
`ChewingTable`. The first lookup is the first `Get`.

This is verified by:
1. The test `open_does_not_scan_pinyin_index` (constructs a
   `ChewingDictionary`, does nothing with it — would fail if open
   scanned)
2. The `chewing` mode of `load_profile` (prints init time + RSS, shows
   no index-proportional cost)
