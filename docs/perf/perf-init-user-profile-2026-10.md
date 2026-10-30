# Init: opening a profile that modifies system phrases (2026-10-17)

Status: **measured init-path change**, `oxpinyin-user` only
(`persistence.rs`, `store_libpinyin.rs`). No store, backend, data-format,
ABI or dependency change. The benchmark instrument is the independent
`oxpinyin-initbench` of PR #842, built from a separate checkout; nothing in
this change depends on it.

> **Note (2026-10-18 UTC).** PR #842 was later closed unmerged under the
> Python 3 + `just` decision. Its exact revision is preserved at
> `a204f9c0e6874eee9d08795fe77a42b7a25da1d6`; the figures here remain valid
> for the artifact that revision timed and are not reattributed. The recovered
> capability is specified in
> [`../findings/tooling-consolidation-python-just.md`](../findings/tooling-consolidation-python-just.md)
> section 7.

## Problem

On a profile whose `.dbin` journals modify many system phrases, `*_init` is
several times the pin's. Whole-process figures (same run, below): 17.9–19.7 ms
against the pin's 2.5–3.1 ms, and 106 M instructions against 10 M. The fixtures
(`phrases`, `journals`) are 10,000 distinct system phrases at frequency 20,000
each; they differ in shape from `modest` (80 import calls of 2 distinct phrases)
and the three are **not** a size-scaling series.

## Measured attribution (before the change)

Callgrind, `pinyin` facade, stop after `init`, inclusive cost, `phrases`
(142.4 M Ir in the child; `pinyin_init` 140.9 M, `open_user_store` 136.9 M):

| function | Ir | note |
| --- | ---: | --- |
| `GenericUserStore::open_session` | 136.9 M | |
| `persistence::load_checked` → `load_logs_with_mask` | 44.4 M | journal replay |
| `store_libpinyin::seed_txn` | 38.2 M | of which `BdbTxn::put` 21.4 M |
| `SharedDb::put` (replay of the buffered writes into libdb) | 33.9 M | 10,009 calls, mostly `__db_put` |
| `SystemLibrary::item` | 16.2 M | 10k in the replay, 10k in `seed_txn` |
| `ChunkItem::clone` | 7.2 M | `open_session` cloning the overrides |
| `decode_phrase_item` | 6.7 M | two per MODIFY record |
| `encode_phrase_item` | 4.4 M | length check in the replay |

Allocation calls at the init stop, by caller (callgrind call counts into
`malloc`; DHAT totals 268,449): `SystemLibrary::item` 64k and
`decode_phrase_item` 64k (about 3 per call: phrase, pronunciation list, key
run), `ChunkItem::clone` 32k, `seed_txn` 20k, `decode_log_records_partial` 20k,
`frame` and `BdbTxn::put` 20k.

So for each of the 10,000 overridden slots the original item was materialised
three times (replay comparison, `seed_txn` base frequencies, and the clone into
the session's live map), plus a re-encode only to learn its length.

## Change

1. `open_session` moves `loaded.state.system_overrides` into
   `StoreInner::system_items` (`std::mem::take` and an owning iterator) instead
   of cloning every item and dropping the originals with `loaded`.
   `has_system_items` is still computed from the loaded map first, and
   removals (`None`) still keep no item.
2. `seed_txn` no longer builds the original item to read its base frequencies:
   `SystemLibrary::{is_readable, pronunciation_frequency}` answer from the
   mapping (and from the inline map) with the answers `item` would give.
3. The journal replay decides "does the held item equal this record's old
   item" with `SystemLibrary::item_matches` (against the override, else against
   the mapping in place) and takes the stored length from
   `phrase_item_wire_len`, the encoded length computed from the item's shape
   with `encode_phrase_item`'s refusals.

Idiom: borrow the mapping instead of copying out of it, and move values to
their last owner instead of cloning. No `unsafe`, no new type or format.

## Why behaviour is unchanged

Source authority: the pin's `SubPhraseIndex::merge` compares the record's old
item with the item it holds, in place, and the library's log replay is
unchanged in order and arithmetic (`phrase_index.cpp` `merge`,
`add_phrase_item`; the replay's own comments cite 074a2219).

- Every new reader is defined as the old expression and tested against it:
  `in_place_item_reads_agree_with_the_materialized_item` compares each helper
  with `item(slot)` over a mapped library, an inline item, a slot whose text does
  not decode (the helper must answer "absent"), absent slots, and probes that
  differ in phrase, unigram, count, order and keys. A mutation (ignoring the
  unigram) makes it fail. `phrase_item_wire_len_is_the_encoded_length_or_the_same_refusal`
  does the same against `encode_phrase_item`, with a mutation check.
- `replay_over_mapped_originals_matches_replay_over_inline_ones`: ten journal
  scenarios (in-place and growing MODIFY, matching and mismatched REMOVE,
  chained records on one slot, missing slots, undecodable payloads, a header that
  misses the total, ADD then MODIFY) give the same overrides, totals and skip
  messages with mapped and inline originals, **and the same output as the
  parent commit** (the scenario outcomes were dumped from this tree and from a
  checkout of `main` and diffed: 10 of 10 identical).
- `session_owns_the_replayed_overrides_it_was_opened_with`: modify, remove and
  no-journal profiles, `has_system_items` (true for a removal-only journal, as
  before), and save then reopen.
- Pin differential, full default case set, all three cells (the contributor rules'
  path check lists two changed files that contain `feature = "bdb|tkrzw|kyotocabinet"`
  — `persistence.rs` and `store_libpinyin.rs`, whose cfg lines this change does
  not touch — so rule 2 applies although the diff changes no `oxpinyin-store`
  file and no `Cargo.toml` backend line). Run with
  `tools/bisection/run-contract-diff.sh` on the tree at base `e351f59a` plus
  this change:

  | cell | subject build | MATCH | SKIP | DIFFER |
  | --- | --- | ---: | ---: | ---: |
  | bdb | release `install.sh` objects | 2 556 | 1 | 4 |
  | kc | debug `oxpinyin-capi` + `-zhuyin-capi`, `--features kyotocabinet` | 2 544 | 13 | 4 |
  | tkrzw | debug, `--features tkrzw`, against the sound libtkrzw `setup.sh` installs | 2 544 | 13 | 4 |

  The 4 DIFFER are the same in every cell (`table-conf-lambda-exponent-without-digits`
  and the hex and zhuyin variants, a `table.conf` parse), and are identical on
  the base build in all three cells (a rerun of just those cases on the base).
  The SKIPs are cases that craft another backend's storage format.

  Two changes to `main` followed that base, neither on the init path. #839
  (`79ab9a11`) reports those four cases as host-dependent; it edits only the
  harness and a findings doc. #844 (`944a1db2`) makes a second
  `pinyin_fini`/`zhuyin_fini` return safely (register row 105) and adds the cases
  `fini-twice` and `zhuyin-fini-twice`; it edits `context.rs` and `live.rs` in
  `oxpinyin-capi` and `oxpinyin-zhuyin-capi`, the harness and two findings docs,
  and none of `oxpinyin-user`, `-data`, `-store`, `-runtime` or `-facade`. With
  #844 in the tree, those two cases were run on this change and on `main`, in all
  three cells: MATCH in each (pin SIGSEGV, subject returns). The full suite was
  not repeated.

  `cargo test -p oxpinyin-user` passes on bdb (166 + 17), kc (173) and tkrzw
  (173).
- `initbench run --rounds 0` (reference = parent build, subjects `opt` and the
  pin): all 10 facade × profile cells and both save/reopen scenarios comparable,
  stderr and leftover files identical. The one known difference (zhuyin `'` and
  `''`, pin returns false, oxpinyin true) is unchanged and is not touched here.

## Measurements

One host (Linux x86_64, 4 vCPU KVM guest, kernel 6.18), Berkeley DB, `--cpu 2`,
warm cache, identical system data and fixture bytes. Base = the exact `main`
revision `e351f59a`; opt = this change on top of it. Both built with
`tools/packaging/install.sh` (`--locked --release --no-default-features
--features bdb,shipped` for libpinyin, `bdb` for libzhuyin):

| object | bytes | SHA-256 |
| --- | ---: | --- |
| base `libpinyin.so.15.0.0` | 5 281 016 | `fbe00e43fa1096a739a310ca0c38e84de3af384c7431f192137494f6a18e72af` |
| opt `libpinyin.so.15.0.0` | 5 295 208 | `d68fc6d60cfc7985961aaf617604b0731e4842e339d31c62e66eec936b02ef9f` |
| base `libzhuyin.so.15.0.0` | 5 065 096 | `0a176b579b8f315bfae8936bf800a6b7c2fc041066675484df7bc4d84edc8f3b` |
| opt `libzhuyin.so.15.0.0` | 5 079 288 | `9435a0ef85a7bbafe25b7e513532a04d984683cdcfd60e55f447af64fe284d24` |
| pin `libpinyin` (2.11.92 @ `074a2219`, BDB) | 4 802 600 | `754499651c302efd91fe7f72dd6a791978400b268cb6206d2ebe644015721f30` |
| pin `libzhuyin` | 4 681 128 | `cf4ccbce9f70f6c7e412b92a52a243ba64fdea1ff197cbc44019c6b85650d53b` |

`base-control` is the base bytes at another path. The four subjects ran in one `initbench run` (timing: 60
rounds, 3 warm-up; memory: 30 and 2), the instrument being PR #842 at `a204f9c0`
built in a separate checkout. 0 samples rejected.

### Deterministic: instructions and allocations (Callgrind, DHAT; init stop)

| facade / profile | init Ir base → opt | allocation calls base → opt | peak live heap base → opt |
| --- | ---: | ---: | ---: |
| pinyin `empty` | 1.45 M → 1.45 M | 1 139 → 1 139 | 0.21 → 0.21 MB |
| pinyin `modest` | 1.85 M → 1.84 M | 1 591 → 1 555 | 0.21 → 0.21 MB |
| pinyin `phrases` | 140.90 M → **105.52 M** (−25%) | 268 449 → **129 992** (−52%) | 4.15 → **3.04 MB** (−27%) |
| pinyin `journals` | 142.93 M → **105.47 M** (−26%) | 274 380 → **130 496** (−52%) | 4.21 → **3.07 MB** (−27%) |
| zhuyin `phrases` | 140.93 M → **105.37 M** (−25%) | 268 465 → **130 008** (−52%) | 4.15 → **3.04 MB** |
| zhuyin `journals` | 142.93 M → **105.27 M** (−26%) | 274 396 → **130 512** (−52%) | 4.21 → **3.07 MB** |

The pin is 10.3–10.4 M Ir and about 1.3 k allocation calls on the same fixtures.
Live heap at the end of init is unchanged (1.52 MB on `phrases`): the session
still holds one copy of the overrides, as before. The gain is the transient copy,
the repeated materialisation, and their allocations.

### Wall clock, `*_init` and time to first candidate (ms, median [95% CI])

| facade / profile | metric | pin | base | opt | base-control | opt/base | opt/control |
| --- | --- | ---: | ---: | ---: | ---: | --- | --- |
| pinyin `phrases` | `pinyin_init` | 2.65 | 21.18 | 16.64 | 20.41 | 0.79× [0.75–0.84] | 0.82× [0.74–0.87] |
| pinyin `phrases` | first candidate | 4.39 | 22.23 | 17.65 | 21.38 | 0.79× [0.74–0.86] | 0.83× [0.76–0.89] |
| pinyin `journals` | `pinyin_init` | 2.72 | 22.80 | 17.42 | 21.35 | 0.76× [0.69–0.86] | 0.82× [0.74–0.89] |
| pinyin `journals` | first candidate | 4.32 | 23.80 | 18.55 | 22.43 | 0.78× [0.71–0.87] | 0.83× [0.75–0.90] |
| zhuyin `phrases` | `zhuyin_init` | 2.29 | 20.11 | 16.03 | 20.60 | 0.80× [0.74–0.84] | 0.78× [0.73–0.83] |
| zhuyin `phrases` | first candidate | 3.85 | 21.22 | 17.08 | 21.51 | 0.80× [0.76–0.85] | 0.79× [0.74–0.84] |
| zhuyin `journals` | `zhuyin_init` | 2.72 | 21.28 | 16.01 | 20.75 | 0.75× [0.70–0.82] | 0.77× [0.73–0.84] |
| zhuyin `journals` | first candidate | 4.38 | 22.36 | 17.15 | 21.72 | 0.77× [0.71–0.84] | 0.79× [0.74–0.85] |
| pinyin `empty` | `pinyin_init` | 1.87 | 1.57 | 1.51 | 1.49 | 0.96× [0.87–1.04] | 1.01× [0.91–1.21] |
| pinyin `modest` | `pinyin_init` | 6.30 | 1.70 | 1.73 | 1.66 | 1.02× [0.86–1.12] | 1.04× [0.93–1.16] |
| zhuyin `empty` | `zhuyin_init` | 1.60 | 1.36 | 1.35 | 1.29 | 0.99× [0.89–1.12] | 1.05× [0.98–1.18] |
| zhuyin `modest` | `zhuyin_init` | 6.52 | 1.71 | 1.81 | 1.55 | 1.06× [0.93–1.21] | 1.16× [1.00–1.32] |

Against the pin, `*_init` on `phrases` goes from 8.0× (pinyin) and 8.8× (zhuyin)
to 6.3× and 7.0×; on `journals` from 8.4× and 7.8× to 6.4× and 5.9×. Time to first
candidate on `phrases`: 5.1× → 4.0× (pinyin), 5.5× → 4.4× (zhuyin).

The effect on the large fixtures is a 20–25% reduction in `*_init` and in time
to first candidate, with intervals that exclude 1 against both the base and its
same-bytes control. On `empty` and `modest` the executed code is the same
(instruction counts equal to within 0.5%) and the ratios are within noise; the
zhuyin `modest` opt/control 1.16× [1.00–1.32] has no code difference behind it and
is treated as noise.

### Memory (memory-mode, median; pinyin `phrases`; `journals` and zhuyin within ±0.01)

| point · quantity | pin | base | opt | opt/base |
| --- | ---: | ---: | ---: | --- |
| init · anonymous MiB | 0.80 | 5.00 | 4.62 | 0.92× |
| init · peak RSS MiB | 11.84 | 16.17 | 15.69 | 0.97× |
| init · RSS Δ MiB | 9.05 | 13.17 | 12.61 | 0.96× |
| init · minor faults | 418 | 1 780 | 1 680 | 0.94× |
| first candidate · anonymous MiB | 0.82 | 5.00 | 4.62 | 0.92× |
| first candidate · peak RSS MiB | 12.17 | 16.17 | 15.72 | 0.97× |

A small, reproducible reduction: 0.38 MiB private, about 0.5 MiB peak, 100 fewer
page faults. The base is still 4.2 MiB more anonymous memory than the pin at init.
`empty` is unchanged.

## Trade-offs measured, not explained

On the large fixtures `*_fini` goes up 25–32% (pinyin 1.65 → 2.06 ms; zhuyin 0.96
→ 1.28 ms) and the pinyin `*_alloc_instance` from about 3 µs to 8 µs, against
4.7–5.2 ms saved in `*_init`; time to first candidate, which includes the
allocation step, falls 20–23%. These changes are reproducible (intervals exclude 1
against the base and the control) but are **not** explained by instruction
counts: Callgrind puts the teardown at +80 k Ir (+1.2%) and the allocation phase
at fewer instructions than the base. An allocator-state or cache effect from the
smaller peak is a hypothesis that was not tested. The `*_fini` figures above were measured
at base `e351f59a`; #844 (`944a1db2`) later reordered the first `pinyin_fini` /
`zhuyin_fini` call's registry removal against its locale call and added an early
return for a repeated call, and they were not remeasured.

## Not addressed (candidates, from the same profile)

After the change, `phrases` init is 105 M Ir; the largest remaining pieces are
the 10,009 `SharedDb::put` calls (33.8 M, mostly libdb), `BdbTxn::put` (21.9 M:
two allocations per put and the buffered `BTreeMap<Vec<u8>, Option<Vec<u8>>>`
in `oxpinyin-store`), and the two `decode_phrase_item` calls per MODIFY record
(8.4 M). The first two are in the shared store, so changing them needs the three
backend cells and a transaction-semantics argument; they were not attempted
here.

## Commands

```sh
# artifacts (per object), then the instrument from a separate PR #842 checkout
tools/packaging/install.sh libpinyin --prefix=<p> --libdir=<p>/lib -- --locked --release \
    --no-default-features --features bdb,shipped
cargo build --release -p oxpinyin-initbench          # in the #842 checkout
initbench run --subject pin=… --subject base=… --subject opt=… --subject base-control=… \
    --zhuyin-subject … --system-dir <data> --profile <empty|modest|phrases|journals> \
    --work-dir <scratch> --cpu 2 --kind timing --rounds 60 --warmup 3 --out run-timing.jsonl
initbench run … --kind memory --rounds 30 --warmup 2 --out run-memory.jsonl
initbench report --input run-timing.jsonl --reference pin --ratio base/pin \
    --ratio opt/pin --ratio opt/base --ratio opt/base-control --ratio base-control/base
initbench instrument --lib <so> --facade pinyin --system-dir <data> --profile <fixture> --tool callgrind
tools/bisection/run-contract-diff.sh bdb <oracle-prefix> <pinyin.so> <zhuyin.so>
```

Raw JSONL and profiler output are not committed and were not retained
(ephemeral container). Fixtures were built by `initbench profiles`.
