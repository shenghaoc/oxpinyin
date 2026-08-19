# Dictionary / LM load audit — init time and RSS (2026-08)

Date: 2026-08-19 · Status: **measured, one cheaper-win PR landed, pins
unchanged.** Follow-up to `perf-baseline-2026-08.md` (W8: `pinyin_init`
158× the oracle because oxpinyin parses the text model and materializes
redb at init; runtime data 3.48×). This note attributes those costs and
records the option that was implemented.

Host: Intel(R) Core(TM) i7-9750H @ 2.60 GHz, 12 logical CPUs, Linux 6.12,
rustc 1.97.1. Release `opt-level=3`. Tables: `/tmp/oxpinyin-export`
(`pinyin_index.redb` 8,425,472 B, `phrase_index.redb` 4,214,784 B,
`bigram.redb` 33,689,600 B). Model:
`PINYIN_MODEL_DIR` = fetched model20 `interpolation2.text` 83,457,181 B
(SHA-256 `59c68e89…`). Punct: W3 `punct.redb` (the W8 staged
`share/oxpinyin` tree did not ship punct). The W8 dlopen harness was **not**
re-run; those pin numbers stay the Stage-2 scoreboard.

Harness: `crates/oxpinyin-data/examples/load_profile.rs`. Isolated steps
are medians of 3 in one process; RSS rows are **fresh processes** so the
allocator's retained heap from earlier steps cannot hide behind `VmHWM`.

```text
PINYIN_EXPORT_DIR=/tmp/oxpinyin-export \
PINYIN_MODEL_DIR=/path/to/model20/extracted \
cargo run -p oxpinyin-data --release --example load_profile
```

With no mode argument the example runs `all` (inventory + isolated). Named
modes: `inventory`, `isolated`, `cumulative`, `dict`, `pinyin`, `phrase`,
`bigram`, `interp`, `full`, `keycosts`.

## 1. Where init time actually goes

W8 attributed 586 ms of `pinyin_init` to "parsing the 83 MB
`interpolation2.text` and slurping the redb tables." That pairing is
misleading. The parser **stops at the `\2-gram` header** and never reads
the 81 MB bigram dump.

### `interpolation2.text` shape

| Section | Items | Bytes in section |
|---|---:|---:|
| `\1-gram` | 63,907 | 2,065,372 |
| `\2-gram` | 1,849,609 | 81,391,778 |
| headers | — | 31 |

Parsed result: `UnigramTable` of 63,907 `(u32, u64)` records, total count
50,913,735. Resident after parse: **~1.1 MiB anonymous**.

| Step | Warm median (this host, before the PR) |
|---|---:|
| `parse_interpolation2` (BufReader, stop at `\2-gram`) | **29.0 ms** |
| `fs::read` of the whole 83 MB file | 15.3 ms |
| parse from a `Cursor` over that copy | 23.5 ms |

29 ms is ~5% of W8's 586 ms, not the dominant term. It is also not an
mmap problem: the useful payload is 2 MB of ASCII lines.

### redb: mmap vs copy

`redb::Builder::open_read_only` is already a file-backed mmap. The cost
is the subsequent **copy into `BTreeMap<Vec<u8>, Vec<u8>>`**.

| Table | rows | payload (key+value) | `open_read_only` | mmap-scan, drop values | slurp to BTreeMap | slurp to HashMap |
|---|---:|---:|---:|---:|---:|---:|
| `pinyin_index` | 93,349 | 2,289,546 B | 0.1 ms | 13.2 ms | **53.8 ms** | 39.4 ms |
| `phrase_index` | 138,096 | 1,646,460 B | 0.1 ms | 16.4 ms | **75.5 ms** | 80.8 ms |
| `bigram` | 56,359 | 15,247,744 B | 0.1 ms | 22.5 ms | **47.9 ms** | 32.5 ms |
| W3 `punct.redb` | 272 | 2,568 B | 0.1 ms | 0.1 ms | 0.2 ms | 0.1 ms |

Three-table slurp ≈ **177 ms** warm. `HashMap` is not a consistent win
(phrase is slower) and would scramble `iter` key order, so it was not
taken.

Before the PR, `LookupTable::iter` cloned every row into a `Vec`. That
clone was 9–15 ms per extra walk and was paid **again** by each derived
map.

### `SystemDictionary::open` (the real body of `pinyin_init`)

Before the PR this did: slurp pinyin, slurp phrase, then three more full
walks (`build_unigram_map`, `build_prefix_tables`, `build_text_tokens`).

| Step | Warm median | Notes |
|---|---:|---|
| slurp `pinyin_index` | 53.8 ms | kept for decode `get` |
| slurp `phrase_index` | 75.5 ms | kept for `phrase_text` |
| `build_unigram_map` | 36.2 ms | pronunciation totals; decode needs this |
| `build_prefix_tables` | 65.3 ms | `SEARCH_CONTINUED` probes; decode needs this |
| **`build_text_tokens`** | **135.8 ms** | reverse map for **predict/import only** |
| `SystemDictionary::open` | **348.0 ms** | **end-to-end** isolated warm median of the whole `open`; not the sum of the rows above (those are overlapping isolated walks) |

`text_tokens` is 39% of dictionary open and is unused until
`pinyin_guess_predicted_candidates` / suggestion. That is the lazy-open
target.

### Rest of capi init

| Step | Warm median | Notes |
|---|---:|---|
| `BigramLanguageModel::open` | 47.9 ms | slurp 33 MB `bigram.redb` |
| `set_unigrams_from_interpolation2` | 29.0 ms | see above |
| `PunctTable::open_optional` | 0.3 ms | empty if the file is absent (W8 install) |
| `key_cost_table` (430 syllable lookups) | see §5 | **`pinyin_alloc_instance`, not init** |

A capi-shaped `dict + lm + interp + punct` process on this host, before
the PR, was **855 ms / 117,064 KiB RSS** in a later (colder) run. The
warm isolated sum is ~425 ms, in the same band as W8's 586 ms once C ABI
+ user-store + cache effects are allowed for.

## 2. Where RSS actually goes

Fresh-process anonymous RSS, before the PR. Payload is the sum of redb
key+value bytes from a mmap-scan.

| Held object | RssAnon | payload | overhead |
|---|---:|---:|---|
| `LookupTable` pinyin | 21,016 KiB | 2.18 MiB | **~9×** (short UTF-8 keys) |
| `LookupTable` phrase | 25,400 KiB | 1.57 MiB | **~16×** (4-byte keys, short CJK) |
| `LookupTable` bigram | 45,892 KiB | 14.54 MiB | **~3×** (large values) |
| `SystemDictionary` (incl. reverse map) | 66,616 KiB | — | HWM 75,704 KiB (clone spikes) |
| `parse_interpolation2` | 1,232 KiB | ~0.73 MiB of `(u32,u64)` | small |
| capi-shaped dict+lm+interp+punct | **114,448 KiB** | — | RSS 117,064 KiB |

W8 post-init was 98,708 KiB with 95,542 KiB anonymous / 3,156 KiB
file-backed. Same shape: **the working set is the copied BTreeMaps, not
the mmap**. Oracle's tables stay file-backed; oxpinyin's do not.

`BTreeMap<Vec<u8>, Vec<u8>>` is a bad fit for phrase_index (138k tiny
pairs) and still the live decode cache — replacing it is a later
representation change, not this PR.

## 3. mmap vs copy, in one sentence

redb already mmaps. Init copies every byte into anonymous `Vec`s.
`interpolation2.text` is not a POD blob; the parser already streams 2 MB
of the 83 MB file. memmap2 of either file would not remove the copy that
dominates RSS.

## 4. 2026 options — written choice

Evaluated against the numbers above. Constitution: no new dependency
without an ask.

### memmap2 — skip

Use only if a file is already a stable POD blob. None of the runtime
files is:

- `interpolation2.text` is CMU-Cambridge ASCII; 97.5% of it is the unused
  `\2-gram` section.
- `*.redb` is a database. redb mmaps it. The keys are variable-length
  UTF-8 (pinyin) or 4-byte tokens; the values are variable-length UTF-8
  (phrase) or 8n-byte records (pinyin/bigram).
- W3 `punct.redb` is 2.5 KB of payload inside a 1 MB redb file.

A *new* aligned `(u32, u32, u64)` unigram blob *would* qualify. Creating
that file is a format addition, not mmap of what we ship today.

### zerocopy (`FromBytes` / `TryFromBytes`, not `repr(packed)` casts) — defer

The 8-byte `{token, freq}` and `{next, count}` records are POD. The
right parse, if we kept a byte buffer without `to_vec`, is zerocopy's
derive + `TryFromBytes`. That does nothing while `LookupTable` owns
`Vec<u8>` copies: we already `from_le_bytes` the chunks.

Do **not** hand-roll `ptr::cast` on `repr(packed)`. If a later PR
introduces a POD sidecar or stops copying values, add `zerocopy` then
(that is a dep ask) and parse with `TryFromBytes`.

### rkyv — skip

rkyv is for a **new** archived format plus a migrator. Do not rkyv the
live redb schema. Replacing `interpolation2.text` with an archive is
exactly the "do not replace interpolation2 as a first move" ban. Disk
size of the 81 MB `\2-gram` section is a Stage-2 format project, not
this one.

### Cheaper win — **chosen**

No new crate. Do not replace redb. Do not replace `interpolation2.text`.

1. **Lazy table work that decode does not need.** `text_tokens` (predict
   / import reverse map) is 136 ms and ~17 MiB of dictionary RSS. Build
   it on first `tokens_for_text` / `suggest_after`. Punct is already
   `open_optional` and 0.3 ms; leaving it eager is fine. Do not lazy-open
   `bigram.redb`: `key_cost_table` and the first keystroke both need it.
2. **Reuse the W3 fixture path.** Tests already load prepared redb
   through `SystemDictionary::open` / `oxpinyin_init_for_fixtures` and
   skip the text model. Production already uses those same redb tables.
   The missing analogue was "do not rebuild derived maps that fixtures
   never needed at first keystroke."
3. **Do not parse `interpolation2.text` on every init if a prepared
   unigram table exists — but not as redb.** A 63,907-row `unigrams.redb`
   was 4,206,592 B and **36 ms to slurp, slower than the 29 ms text
   parse**. An aligned 16-byte POD blob is 1,022,512 B and **0.1 ms** to
   `fs::read` + `from_le_bytes`. That sidecar is the right follow-up if
   we ever drop the 81 MB `\2-gram` from the runtime dir; it is not
   worth a loader branch while the text 1-gram parse is 29 ms and the
   file still has to be shipped. Public `pinyin_init` still fail-closes
   without a parsable `interpolation2.text`.
4. **Stop cloning table rows.** `LookupTable::get` / `iter` now borrow
   the in-memory map. Derived maps walk once. This is also the decode
   `LookupTable::get` clone that dhat counted at 1.33 M blocks in
   `perf-exploration.md`.

## 5. What the PR changed

- `LookupTable::get` → `Result<Option<&[u8]>, TableError>` (borrow, no
  `to_vec`).
- `LookupTable::iter` → borrowed `(&[u8], &[u8])` (no `Vec` of clones).
- `SystemDictionary::open` builds unigrams + prefix tables in **one**
  walk of pinyin_index.
- `text_tokens` is a `OnceLock`, filled on first predict/import use.
- Pins, goldens, SPECs, CI policy: **unchanged**. No new dependencies.

`pinyin_alloc_instance` is `Session::new` → `key_cost_table`: 430
dictionary lookups + empty-history scores. W8: 48.5 ms. After borrowed
`get`, **28.0 ms** on this host (one run, tables already loaded). Still
the alloc-side story, not init.

## 6. After numbers (same host, same tables)

Headline is the **isolated warm median** of 3, same process protocol
before and after. Fresh-process rows are RSS snapshots; their wall times
are **not comparable** across the before/after pair (different cache
state, not a paired protocol).

| Process | Before | After |
|---|---:|---:|
| `SystemDictionary::open` (isolated warm median) | 348 ms | **200 ms** |
| `SystemDictionary::open` (fresh process, RSS only) | 69,304 KiB RSS / HWM 75,704 KiB | **51.7–51.9 MiB / HWM 52.8 MiB** |
| capi-shaped dict+lm+interp+punct (fresh, RSS) | 117,064 KiB / 114,448 KiB anon | **97.5 MiB / 94,880 KiB anon** |
| `parse_interpolation2` | 29 ms / ~1.1 MiB | unchanged (not this PR) |
| `PunctTable::open_optional` | 0.3 ms | unchanged |
| `key_cost_table` | (W8 alloc 48.5 ms) | 28 ms on this host |

This PR did **not** re-run the W8 dlopen scoreboard. The published 158×
`pinyin_init` figure is unchanged as a pin. The 348 → 200 ms dictionary
open is a load-profile measurement of one function inside init, not a
claim that the 158× ratio is gone.

Dictionary RSS drop is the reverse map (~17 MiB) plus the clone spike
that no longer happens (HWM −23 MiB on the dict-only process). The three
raw BTreeMaps remain: ~21 + 25 + 46 MiB anonymous. That is the next
representation target, not a first move.

Borrowed `iter` on the slurped map: 1.1 / 1.8 / 0.7 ms vs the old clone
walk 9–15 ms.

## 7. What was not done

- No `memmap2`, `zerocopy`, or `rkyv` dependency.
- redb stays the on-disk format; tables are still slurped at open
  because a redb `get` per decode lookup was already measured too slow
  (`LookupTable` comment, `perf-exploration.md`).
- `interpolation2.text` stays the real-unigram source of truth for
  public `pinyin_init`.
- W8 `run-perf-baseline.sh` pins were not re-measured. Whoever next
  runs that harness should expect `pinyin_init` to move with dictionary
  open (348 → ~200 ms of it) and post-init RSS to lose the reverse map;
  the 83 MB text file and the three slurped BTreeMaps still set the
  floor.

## 8. Next representation work (not this PR)

Ranked by these numbers, still without replacing redb or the text model
as a first move:

1. **Denser in-memory maps** for the three slurped tables (typed
   `HashMap<u32, String>` / `HashMap<String, Box<[(u32, u32)]>>` /
   `HashMap<u32, BigramRow>`). Phrase_index is 16× payload in RSS today.
   Decode `get` would also stop parsing 8-byte records on every lookup.
2. **POD unigram sidecar** (16-byte records, `TryFromBytes` when the dep
   is asked for) plus eventually not shipping the 81 MB `\2-gram` half.
   Init win is ~29 ms; disk win is the 3.48× data ratio.
3. **`key_cost_table`** still walks 430 keys at alloc. After borrowed
   `get` it is 28 ms. Caching it on the context rather than per instance
   could reduce W8's 48,483× alloc gap.

## Caveats

- This is the same shared machine as W8; absolute milliseconds moved
  between processes depending on cache. Isolated warm medians of 3 are
  the headline speed numbers. Fresh-process rows are RSS snapshots and
  are not a paired before/after speed protocol.
- The example's `cumulative` mode after the PR is the live
  `SystemDictionary::open` path (lazy reverse map). Before the PR it
  forced `build_text_tokens` so the 136 ms / 17 MiB cost was visible.
- W3 `punct.redb` is not the full punct table; it is 272 tokens. A
  missing punct file is the W8 install shape and is 0.3 ms either way.
- User-store open was not in this harness (empty user dir). It is not
  the 96 MiB anonymous set.
