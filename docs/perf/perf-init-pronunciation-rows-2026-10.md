# Init: opening a replayed system item without its reading rows (2026-10-18)

Status: **measured init-path optimization, with the compatibility corrections it
exposed.** `oxpinyin-user` only (`store.rs`, `store_libpinyin.rs`,
`persistence.rs`), a test seam in `store.rs`, and the pin differential
`tools/bisection/contract-diff.py`. No store, backend, data-format, ABI or
dependency change, no `WriteTxn` or `DB->sync` change. Built on `main` at
`6d9e23b2`. #853 (a unigram added to a live system item) is open; this change
touches no unigram, facade-total or overflow-guard code and imports no #853
branch. The benchmark instrument is the independent `oxpinyin-initbench` of
PR #842 at `a204f9c0`.

## Problem

On a profile whose `.dbin` journals modify shipped system items, `*_init` is
several times the pin's. The attribution (#871) put the store seeding at
69.3 M of 110.2 M instructions on pinyin `phrases`. Counting the writes by table
showed that 10,000 of the 10,009 puts are `PRONUNCIATION` delta rows, one per
changed reading (framed key 25 bytes, value 13 bytes); the other 9 are metadata.
Neither large fixture writes a unigram row.

## Source-derived behavior (074a2219)

- An import is `PhraseItem::add_pronunciation` (`phrase_index.cpp:56-91`): the
  first entry whose keys match exactly takes the delta and the call returns; no
  match appends an entry.
- Training is `increase_pronunciation_possibility` (`:114-143`): it walks every
  entry and adds the delta to **each** entry whose keys match
  (`pinyin_compare_with_tones`), on that entry's own `guint32` count, with the
  running-total overflow guard.
- The save diffs the item's bytes (`:392-436`). Nothing keys the entries by
  their keys: two entries with the same keys are two entries. Shipped data has
  none (0 of 138,096 items in the four libraries); only a crafted journal makes
  them.

## Change

1. **`perf(user)`**: `seed_txn` stops writing the reading rows for a replayed
   MODIFY of an item already in the shipped library. It keeps `SYSTEM_BASE`, the
   alloc cursors, `UNIGRAM_TOTAL`, `LIBRARY_OFFSET`, the unigram delta (the count
   reads and the library totals consult it), appended and user items with their
   text, reading and index rows, and the raw phrase table.
   `replace_system_libraries` (library reload) shares `seed_txn` and still removes
   the rows a session wrote before it. `SystemLibrary::pronunciation_frequency` had
   no other reader and goes.
2. **`fix(user)`**: with the rows gone, an item with two entries of one reading
   put the session and the save out of step (the session kept the first entry's
   count for both, the save wrote the last entry's delta into both). The live item
   already is where pricing and an import read a reading's count; training and
   the export now read it too. `train_pronunciations` takes each entry's count from
   the item and matches rows to entries by place; `export_state` writes the item's
   counts instead of rebuilding `original + row` for every entry with the keys.
   `effective_pronunciation_count`, with no other reader, goes. This commit is
   separable: without it, four `train` cases of duplicate entries change their
   answer without moving toward the pin, and a session and its save disagree.

## The `has_user_data` gate

`has_user_data` gates `count_delta`, `unigram_delta` and `bigram_row`. Its
recomputation after a mask, a removal or a reload probes `PRONUNCIATION` among
other tables, so replayed rows could hold it open. They no longer do, and the
gate is left as it was. What the count reads consult is `UNIGRAM`,
`UNIGRAM_TOTAL` and the grams; `PRONUNCIATION` and `PHRASE` feed none of them.
At each recompute site `UNIGRAM_TOTAL` has just been rewritten as the sum of the
`UNIGRAM` rows, and the grams are added by hand (`has_grams`, `grams_left`)
because a session's grams live in the container. The test
`the_gate_never_hides_a_count_or_a_gram_...` holds the observable result: after an
open, a gram, training, a mask of another library, an import, a user phrase and
its removal, a mask of the replayed library, a reload and an unload and load, the
count reads are the same with the gate as the session holds it and with it forced
open; a second profile reaches the state the rows used to hide, a gram that
outlives every unigram, phrase and table row after a reload. Dropping
`|| has_grams` from the reload site fails it. The same mutation on `main` passes,
because the replayed rows held the gate open by accident; the dependence is now
explicit.

## Outcomes against the pin

`replay-pron-*` in `tools/bisection/contract-diff.py`: one journalled item (癫痫)
in nine shapes (a raised reading, a lowered one, unigram only, two entries of one
reading in four arrangements, a second reading) through three lifecycles (`train`:
train, import, save, reopen, train, save, reopen; `import`: import, save, reopen,
import again, save, reopen; `mask`: mask the library, import, save, reopen), both
facades; and `replay-pron-mixed` (a shipped item with a reading and unigram change,
an appended item, a user phrase in library 7, training that also trains a gram, two
save and reopen rounds). 56 cases. `main` is the `6d9e23b2` build (bdb); the
candidate is the final commit's build (bdb, kc and tkrzw; tkrzw against the sound
libtkrzw `tools/cloud/setup.sh` built, which the pin and the subject both resolve).

| class | cases | `main` | candidate |
| --- | --- | --- | --- |
| matches the pin on `main` and still does | 42: `import`, `mask` and `train` of the non-duplicate, non-lowered shapes, and `mixed` | MATCH | MATCH |
| fixed: duplicate entries through import, save and reopen | `import-dup-{raised-last,raised-first,both}` × 2 facades (6) | DIFFER | MATCH |
| fixed: duplicate entries through training (#873) | `train-dup-{raised-last,raised-first,both}` × 2 (6) | DIFFER | MATCH |
| fixed: a lowered reading through training (#874) | `train-lowered` × 2 (2) | DIFFER | MATCH |
| mismatch left unchanged | none | | |
| new or worsened mismatch | none | | |

The candidate matches 56 of 56 on bdb, 56 of 56 on kc and 56 of 56 on tkrzw. `main`
differs on the 14 cases in the three "fixed" rows and matches the other 42.

The `train` cases of duplicate entries and of a lowered count are the inherited
mismatches (#873, #874). I expected to leave them open as known failures. Doing so
was not possible without a new mismatch: with the rows removed and training and
the export as on `main`, `train-dup-raised-first` (both facades), `train-dup-raised-last`
(pinyin) and `train-dup-both` (pinyin) gave an answer that differs from `main`'s at the
same distance from the pin (9 to 11 of 308 pinyin observations differ either way), and left
the session and its saved file disagreeing (`[169, 169]` in the session, `[229, 229]`
saved). That change of answer is a new mismatch, so the second commit resolves it,
and in doing so also makes the lowered case match. The 14 cases stay reproducible:
run their names with this commit's harness against a `main` build and they give
`DIFFER`; the harness has no `known=` mechanism and none was added. The issues
#873 and #874 describe the `main` behavior and are closed by this PR.

The identified mask case is at the store API (`store.mask_out` alone): a shipped
item with two readings whose second reading the journal changed, the library masked,
then a change to the first reading left the second at its journalled count on `main`
(`[104, 45]` against the shipped `[104, 40]`): the rows outlived the reset. The C ABI
does not reach it, because the facade's `mask_out` reloads the libraries
(`reload_system_libraries`) and the reload removes the rows, so the `mask` lifecycle
matches the pin on `main` too. The unit test `a_mask_leaves_nothing_of_the_replayed_counts`
holds it.

Unit tests (`store_libpinyin/replayed_readings.rs`, 7): the open writes no reading
row and trains from the replayed count through two save and reopen cycles; duplicate
entries through import, save and reopen; training moves each matching entry on its
own count (duplicates, reversed duplicates, a lowered count: 169/229 then 307/367,
109 then 247, as the pin); a mask leaves nothing of the replay; a populated user
library and a pinyin index entry that outlives a removed phrase are the same with and
without the journal; a mixed profile (a shipped item with a reading and unigram
change, a unigram-only item, an appended item with its seeded reading, user phrases,
grams) through two save and reopen cycles; the gate. On `main` 6 of the 7 fail (the
gate test passes). On the candidate, reinstating the seeded rows fails the four
that assert none; the mask and training tests then pass because the second commit
makes stale rows harmless. `main` and the candidate give byte-identical views for the
mixed profile, live and reopened.

Parity dump (not committed): 14 journal shapes × 12 operation sequences dumped from
`main` and from the candidate and diffed (live and exported items, readings, every
count read, the saved `gb_char.dbin`, the user view). With a shipped-shaped original
every non-duplicate, non-lowered scenario is identical except the gate flag after a
mask or an unload and load, where `main` reports true on a state whose rows are all
stale and every read is equal; the duplicate, lowered and store-level mask scenarios
differ as above.

`cargo test -p oxpinyin-user`: 185 + 3 + 14 on bdb, 192 + 3 + 14 on kc and tkrzw;
clippy `-D warnings` and `cargo fmt --check` clean on all three.

The broader differential suite is every other default case of `contract-diff.py`
(2,691), release subjects, candidate against the pin, on each cell. The four
`wrap-256th-reading*` cases of #869 were **not executed**: they are neither passes nor
failures here.

| cell | MATCH | SKIP | HOST-DEPENDENT | DIFFER |
| --- | ---: | ---: | ---: | ---: |
| bdb | 2,684 | 1 | 4 | 2 |
| kc | 2,672 | 13 | 4 | 2 |
| tkrzw | 2,672 | 13 | 4 | 2 |

The SKIPs craft another backend's storage format. The 4 HOST-DEPENDENT are the
`table-conf-lambda-*exponent-without-digits` cases (#839). The 2 DIFFER are
`save-fatal-valid-to-invalid` and its zhuyin twin, the same on every cell and the same
on `main` (bdb, checked): under `G_DEBUG=fatal-criticals` both the pin and the subject die
of SIGTRAP (-5) on this host's glib where the case expects SIGABRT (-6); stderr and the
files left behind are equal apart from the process id and the timestamp. They are
host-dependent and unrelated to this change. With the 56 focused cases, no case this
change touches differs from the pin on any cell.

## Measurements

One host (Linux x86_64, 4 vCPU KVM guest, kernel 6.18.44), Berkeley DB 5.3.21 from
the host, `--cpu 2`, warm cache, identical system data and fixture bytes
(identities `1f5ba68b`, `9e3c8bf2`, `3d8826a1`, `4bee8390`), valgrind 3.22.0,
rustc 1.97.1, cargo-c 0.10.26. One session, in the order: every build and the
provisioning, verification (30 of 30 cells comparable), Callgrind and DHAT, the
resident-memory run, then native wall clock with nothing else running on the host.
Subjects built with `tools/packaging/install.sh` (`--locked --release
--no-default-features --features bdb,shipped`; `bdb` for libzhuyin); the control is
the base bytes at another path.

| object | bytes | SHA-256 |
| --- | ---: | --- |
| base `libpinyin.so.15.0.0` (`6d9e23b2`) | 5,364,808 | `6d3218b6d4720e0a…` |
| candidate `libpinyin.so.15.0.0` (`bbb9564`) | 5,359,912 | `1a15326766cfea12…` |
| base `libzhuyin.so.15.0.0` | 5,144,576 | `989e54d0a5644013…` |
| candidate `libzhuyin.so.15.0.0` | 5,143,928 | `a34788cf18601a0c…` |
| pin `libpinyin` (2.11.92 @ `074a2219`, BDB) | 4,802,600 | `754499651c302efd…` |

The candidate sources are the final commit's; the commits after `bbb9564` change
only the harness script. The pin was built in the pinned Debian-testing container
and runs against the host's `libdb`, `glib` and `libc`, as the subjects do.

### Instructions (Callgrind; init stage = cumulative init minus `lib_load`)

| facade / profile | pin | base | candidate | candidate / base | candidate / pin |
| --- | ---: | ---: | ---: | ---: | ---: |
| pinyin `phrases` | 10.32 M | 110.19 M | **44.68 M** | −59.4% | 4.33× (base 10.68×) |
| pinyin `journals` | 10.39 M | 109.81 M | **42.60 M** | −61.2% | 4.10× (base 10.57×) |
| zhuyin `phrases` | 10.15 M | 110.16 M | **44.68 M** | −59.4% | 4.40× (base 10.86×) |
| zhuyin `journals` | 10.21 M | 109.76 M | **42.71 M** | −61.1% | 4.18× (base 10.75×) |
| pinyin `empty` | 5.86 M | 1,444,856 | 1,444,881 | +25 | |
| pinyin `modest` | 11.82 M | 4,007,784 | 4,001,953 | −0.15% | |

Through first candidate, pinyin `phrases`: pin 15.45 M, base 112.89 M, candidate
47.19 M (3.05× the pin); `journals`: 15.52 M, 112.54 M, 45.10 M. The measured saving,
65.5 M on `phrases`, is below the attribution's 69.3 M upper bound because
`seed_txn` still reads the original of every overridden slot (below).

### Writes (instrumented build for counts and bytes, Callgrind for native calls)

| pinyin | base | candidate |
| --- | ---: | ---: |
| `phrases`: logical `txn.put` / rows surviving the buffer / native `DB->put` | 10,009 / 10,009 / 10,009 | **9 / 9 / 9** |
| `phrases`: framed key bytes / value bytes | 250,340 / 130,056 | 148 / 56 |
| `journals`: puts / key bytes / value bytes | 10,009 / 257,738 / 130,056 | 9 / 148 / 56 |
| `modest`: puts / key bytes / value bytes | 45 / 953 / 346 | 44 / 926 / 333 |
| `seed_txn` inclusive (`phrases`) | 31.06 M | 3.15 M |
| `BdbTxn::put` / `SharedDb::put` inclusive (`phrases`) | 22.03 M / 34.02 M | 5.4 K / 26.2 K |
| `open_session` inclusive (`phrases`) | 108.89 M | 43.39 M |
| `load_checked` (journal decode and replay), unchanged | 34.96 M | 34.96 M |
| libdb temp file `/var/tmp/BDB<pid>` (`strace`): opens / `pwrite64` / bytes | 1 / 30 / 245,760 (`journals`: 1 / 31 / 253,952) | 0 / 0 / 0 |

The removed rows are `user_pronunciation` of library 1 (`phrases`) and of libraries
1–4 (`journals`, 3000/3000/3000/1000). The 9 that remain are `system_base` ×4,
`user_phrase_alloc` ×4 and `user_unigram_total`. `modest` loses the one reading row
of its replayed shipped item; its appended-item rows, phrase-table rows and
metadata stay. The pin never opens the temp file.

### Allocation and memory (DHAT, init stop, pinyin; zhuyin within 20 calls)

| | allocation calls pin / base / cand | bytes allocated pin / base / cand | peak live pin / base / cand | live at end pin / base / cand |
| --- | --- | --- | --- | --- |
| `phrases` | 1,272 / 108,746 / **66,988** (−38.4%) | 1.08 / 12.72 / **10.13** MB (−20.4%) | 0.67 / 2.75 / **2.75** MB (0) | 0.29 / 1.52 / **1.27** MB (−16.3%) |
| `journals` | 1,288 / 109,050 / **67,291** (−38.3%) | 1.13 / 12.37 / **9.74** MB (−21.2%) | 0.36 / 2.59 / **2.34** MB (−9.6%) | 0.29 / 1.55 / **1.31** MB (−16.0%) |

At first candidate, `phrases`: calls 1,656 / 109,817 / 68,059, retained 0.31 / 1.59 /
1.34 MB. The `phrases` peak is unchanged because it is reached inside the journal
replay, before the first store write (sort and write-run vectors, decoded items: 2.63
of 2.75 MB at that point); `journals` peaks later and moves with the change. Peak
live heap, bytes allocated, live at end and resident memory are separate figures:

| memory mode, medians (MiB) | pin | base | candidate | candidate / base |
| --- | ---: | ---: | ---: | --- |
| pinyin `phrases` · init anonymous | 0.80 | 4.12 | **3.51** | 0.85× |
| pinyin `phrases` · init peak RSS | 11.85 | 15.15 | 14.62 | 0.97× |
| pinyin `journals` · init anonymous | 0.85 | 4.14 | **2.82** | 0.68× |
| pinyin `journals` · init peak RSS | 11.90 | 15.24 | 14.01 | 0.92× |
| minor faults `phrases` / `journals` | 439 / 448 | 1,654 / 1,405 | 1,497 / 1,068 | 0.91× / 0.76× |

The attribution predicted unchanged resident memory on `phrases`; it fell 0.61 MiB
although DHAT's peak live heap did not move. The libdb spill (above) and the
allocator's smaller transient footprint are candidate explanations; neither is
separately measured.

### Wall clock (60 rounds, 3 warm-up, median ms [95% CI])

| | pin | base | candidate | candidate / base | candidate / pin |
| --- | ---: | ---: | ---: | --- | --- |
| pinyin `phrases` · `pinyin_init` | 2.73 | 16.38 | 8.57 | 0.52× [0.47–0.55] | 3.14× [2.87–3.39] |
| pinyin `journals` · `pinyin_init` | 2.62 | 15.52 | 7.71 | 0.50× [0.43–0.60] | 2.94× [2.52–3.57] |
| zhuyin `phrases` · `zhuyin_init` | 2.60 | 20.12 | 9.64 | 0.48× [0.39–0.65] | 3.71× [2.90–4.32] |
| zhuyin `journals` · `zhuyin_init` | 2.66 | 16.17 | 8.28 | 0.51× [0.45–0.58] | 3.11× [2.77–3.75] |
| pinyin `phrases` · time to first candidate | 4.52 | 17.45 | 9.58 | 0.55× [0.50–0.59] | 2.12× [1.99–2.31] |
| pinyin `journals` · time to first candidate | 4.21 | 16.46 | 8.60 | 0.52× [0.45–0.63] | 2.04× [1.75–2.48] |
| zhuyin `phrases` · time to first candidate | 4.26 | 21.22 | 10.86 | 0.51× [0.42–0.69] | 2.55× [1.97–2.99] |
| zhuyin `journals` · time to first candidate | 4.33 | 17.30 | 9.16 | 0.53× [0.47–0.61] | 2.12× [1.88–2.52] |

The same-bytes control against base reads 0.97× [0.87–1.04] (pinyin `phrases`),
1.01× [0.84–1.17] (pinyin `journals`), 0.91× [0.75–1.27] and 1.01× [0.88–1.17]
(zhuyin): the host's noise on these cells is wide (zhuyin `phrases` base spans
15.3–21.3 ms). `empty` and `modest` execute the same code (instructions within
0.15%); their candidate/base ratios are 0.99–1.05× with intervals around 1.

## Not addressed

- **Replay is now 78% of `pinyin_init`** (`load_checked` 34.96 M, against the pin's
  `merge` 3.8 M) and owns the `phrases` peak: two 640,000-byte vectors (the sort
  scratch and the sorted `(slot, item)` run), a 524,288-byte write-run vector and the
  decoded items. The next memory change is those vectors and the sort, before any
  lazy-decoding redesign. The 1.05 MB `kept_items` vector in `open_session` is a
  later, separate transient; reducing it alone does not lower the earlier peak.
- `seed_txn` still costs 3.15 M: `item_at_slot` 40,000 calls (2.28 M) reading each
  overridden slot's original for the unigram delta, and the emptiness probe
  (`BdbTxn::is_empty`, 1.17 M on `phrases`).
- Rows training wrote are still not removed by a store-level mask of the token's
  library; the facade's reload removes them.
- Measurements are bdb only. Not executed: the four `wrap-256th-reading*` cases
  (#869, harness timeout).
- #853 integration cases for whichever change lands later: a unigram added to a live
  replayed item then saved and reopened (the reading counts must stay as the item
  holds them); the facade total moving before a sub-index refusal on an item whose
  reading count the journal changed; `replay-unigram-add-live-item-*` together with
  `replay-pron-mixed-*`.

## Commands

```sh
tools/cloud/setup.sh --cells "bdb kc tkrzw"
tools/packaging/install.sh libpinyin --prefix=<p> --libdir=<p>/lib -- --locked --release \
    --no-default-features --features bdb,shipped      # and libzhuyin with --features bdb
cargo build --release --locked -p oxpinyin-initbench   # in a separate #842 checkout
initbench profiles --out <d> --pin-lib <pin so> --system-dir <data> --work-dir <w>
initbench run --subject pin=… --subject base=… --subject cand=… --subject base-control=… \
    --zhuyin-subject … --system-dir <data> --profile <empty|modest|phrases|journals> \
    --work-dir <w> --cpu 2 --rounds 0 --out v.jsonl                     # verification
initbench run … --kind memory --rounds 20 --warmup 2 --out m.jsonl
initbench run … --kind timing --rounds 60 --warmup 3 --out t.jsonl
initbench report --input t.jsonl --reference pin --ratio base/pin --ratio cand/pin --ratio cand/base
taskset -c 2 valgrind --tool=callgrind --cache-sim=no --branch-sim=no \
    --callgrind-out-file=<o> initbench child --facade pinyin --lib <so> --system-dir <data> \
    --user-dir <copy of the fixture's user dir> --bind now --stop-after init   # and --tool=dhat
cargo build --release --locked -p oxpinyin-capi -p oxpinyin-zhuyin-capi \
    --no-default-features --features bdb|kyotocabinet|tkrzw     # differential subjects
tools/bisection/run-contract-diff.sh <bdb|kc|tkrzw> <oracle-prefix> <pinyin.so> <zhuyin.so> \
    -- --cases replay-pron-…
```

Raw JSONL, Callgrind and DHAT output are not committed and were not retained
(ephemeral container). The write counts come from an instrumented build with
counters in `BdbTxn::put` and the write loop, which was not used for any timing.
