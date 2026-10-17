# Init: comparing journal old items without materializing them (2026-10-17)

Status: **measured init-path change**, `oxpinyin-user` only
(`persistence.rs`). No store, backend, data-format, ABI or dependency change.
Follows #845 (`docs/perf/perf-init-user-profile-2026-10.md`), whose "not
addressed" list ends with the two `decode_phrase_item` calls per MODIFY record.
The instrument is the independent `oxpinyin-initbench` of PR #842
(`a204f9c`), built from a separate checkout; nothing here depends on it.

**Result in one line:** a deterministic, small gain — −5.5% instructions and
−25% allocation calls during `*_init` on the large fixtures, no change in peak
or retained memory, and a wall-clock effect that this host cannot separate from
noise.

## Source evidence

Pin `libpinyin` 2.11.92 at `074a2219c90feaf962d0d24f034514033ece5f99`, read
from a clone fetched at that commit (`git rev-parse HEAD` equals it).

- `SubPhraseIndex::merge` (`phrase_index.cpp:439`): a REMOVE removes the held
  item and requires `olditem == *tmpitem` (`:460-475`); a MODIFY requires
  `item == olditem` (`:483`) and takes the "increase size" path when
  `newchunk.size() > item.m_chunk.size()` (`:486`). The record's old payload is
  only ever *compared* with the held item; it is never stored.
- `PhraseItem::operator==` (`phrase_index.h:235`) is `memcmp` over the whole
  chunk after a size check; `get_phrase_item` (`phrase_index.cpp:180`) sizes the
  held item from its own header.

oxpinyin's replay (`load_logs_with_mask`) follows the same shape, but decides
"equal" on **decoded** items: `decode_phrase_item(old)` into an owned
`ChunkItem`, then `==` against the override or `SystemLibrary::item_matches`
against the mapping. The decoded old item is used for exactly three things:
the equality test, `old.unigram`, and its encoded length (`grows`). It is
dropped at the end of the record.

## Measured attribution (before the change)

Unmodified `main` `6c665d44`, release `libpinyin.so` (bdb, `shipped`), pinyin
facade, Callgrind on the child process stopped after `init`, `phrases` fixture
(10,000 MODIFY records):

| function | inclusive Ir |
| --- | ---: |
| whole child, stop after `init` | 106,985,053 |
| `load_logs_with_mask` | 30,207,851 |
| `decode_phrase_item` (20,000 calls) | 8,388,582 |
| replay equality closure (`current_matches`) | 5,499,744 |

DHAT, same stop: 129,992 allocation calls, 11,000,653 bytes allocated, 3,035,373
bytes peak live, 1,523,251 bytes live. Each `decode_phrase_item` makes three
allocations (phrase, pronunciation list, one key run per pronunciation), so the
old-item decode is about 32,000 of the 130,000 calls. They are freed within the
record, which is why peak and retained heap are not expected to move.

## Change

A private `WireItem<'a>` in `persistence.rs`, a validated borrowed view of a
record payload:

- `WireItem::parse(bytes)` returns `Some` exactly when `decode_phrase_item(bytes)`
  returns `Ok`: both need `6 + 4·L + n·(2·L + 4)` bytes for the header's `L` and
  `n`; bytes after the last pronunciation are ignored by both, and neither checks
  that the phrase words are Unicode scalars. It stores the header fields and two
  sub-slices; there is no indexing outside `get`.
- `WireItem::equals(&ChunkItem)` and the rewritten
  `SystemLibrary::item_matches(slot, &WireItem)` compare unigram, count, phrase
  words and each pronunciation's keys and frequency in order. The mapped path is
  the previous expression with the probe read from the slice (the mapped item's
  text is still scanned to the end first, so an undecodable item is still `None`).
- `WireItem::wire_len()` replaces `phrase_item_wire_len(&ChunkItem)`: the same
  refusal (an empty phrase) and, otherwise, `6 + 4·L + n·(2·L + 4)`.
- The MODIFY arm parses and compares the old payload first and decodes the new
  one only then. `new_item` still goes through `decode_phrase_item` because it
  becomes retained state. Every failing path still ends at the same
  "replay stopped at a mismatched MODIFY" record and message.

`decode_phrase_item` and `ChunkItem` are untouched, so the previous path remains
the reference the tests use. Net: a private type of about 80 lines (with its
comments) replaces a 15-line helper and the owned decode of the old item in
every REMOVE and MODIFY record.

## Why behaviour is unchanged

- `wire_item_agrees_with_decode_phrase_item`: 5,878 payloads (400 accepted, 5,478 refused) built from
  eight seeds (an empty phrase with and without a pronunciation, a lone surrogate
  word, duplicate pronunciations, two-character phrases, a 255-character phrase) —
  every truncation, 1 to 5 trailing bytes, the header's two length bytes set to
  every value 0–255, and key runs of the wrong length. For each, `parse` accepts
  iff `decode_phrase_item` does, and for accepted ones the unigram, the encoded
  length (against `encode_phrase_item`, refusals included) and `equals` against
  the decoded item and against every one-edit neighbour (unigram, a phrase word,
  count, order, frequency, a key) equal the owned results.
- `in_place_item_reads_agree_with_the_materialized_item` (existing) now probes
  `item_matches` with wire bytes, with and without trailing bytes, over a mapped
  library, an inline item, a slot whose text does not decode, absent slots, and
  the same probes as before.
- `replay_agrees_with_the_owned_decoding_replay`: a test-only copy of the replay
  as it was (all payloads decoded to owned items, the held item materialized)
  against the real `load_logs_with_mask`, over 3,484 journals per original,
  mapped and inline (inline adds an empty-phrase item a chunk cannot hold). Old
  payloads: exact, empty, padded, every truncation, each one-edit variant, header
  bytes set to 0/1/2/255; new payloads: grown, same size, padded, shorter,
  truncated, empty. Shapes: REMOVE alone; a REMOVE or MODIFY over a slot an earlier
  REMOVE, MODIFY or ADD changed; and a MODIFY between two `MODIFY_HEADER`s. It
  compares the override map, the library total and the skip diagnostics, and
  asserts both outcomes occur (over both originals: 6,534 replays stop at a
  refusal, 434 keep overrides without one).
- Existing: `replay_over_mapped_originals_matches_replay_over_inline_ones`,
  `removed_system_phrase_stays_removed_across_save_and_reopen`,
  `session_owns_the_replayed_overrides_it_was_opened_with` (save then reopen),
  `replay_stops_at_a_mismatched_record`.
- Mutation check (each applied alone, tests rerun, file restored): equality
  ignoring the unigram → 2 failures; mapped compare ignoring key bytes → 1;
  ignoring frequency → 2; rejecting trailing bytes → 3; accepting a truncated last
  frequency → 3; giving an empty phrase a length → 2; counting the raw payload
  length → 1; REMOVE accepting any non-missing item → 1; `>=` for growth → 1;
  not requiring the mapped text to decode → 2. None survived.

### Differential

AGENTS.md rule 2's path check, on this diff against `origin/main`:

```
git diff --name-only origin/main            crates/oxpinyin-user/src/persistence.rs, docs/perf/…
… | grep -c '^crates/oxpinyin-store/'       0
… | xargs grep -lE 'feature *= *"(bdb|tkrzw|kyotocabinet)"'
                                            crates/oxpinyin-user/src/persistence.rs
git diff -U0 -- '*Cargo.toml' | grep …      (no output)
```

`persistence.rs` contains such cfg lines (this change touches none), so the
second bullet fires and all three cells run. `tools/bisection/run-contract-diff.sh`
at `6c665d44` + this change, default case set (pinyin and zhuyin subjects):

| cell | subject build | MATCH | SKIP | HOST-DEPENDENT | DIFFER |
| --- | --- | ---: | ---: | ---: | ---: |
| bdb, this change | release `install.sh` objects | 2,558 | 1 | 4 | 0 |
| bdb, `main` (same run) | release `install.sh` objects | 2,558 | 1 | 4 | 0 |
| kc | debug `oxpinyin-capi` + `-zhuyin-capi`, `--no-default-features --features kyotocabinet` | 2,546 | 13 | 4 | 0 |
| tkrzw | debug, `--features tkrzw`, sound libtkrzw | 2,546 | 13 | 4 | 0 |

The four HOST-DEPENDENT cases are the `table-conf-lambda-*` parse cases #839
classifies; they are the same on `main` (run on bdb; kc and tkrzw had no DIFFER, so
`main` was not rerun there). The 13 SKIPs craft another backend's storage format. `cargo test -p oxpinyin-user`: bdb 167 +
3 + 14, kc 174 + 3 + 14, tkrzw 174 + 3 + 14 (tkrzw against the sound libtkrzw
`setup.sh` builds; `ldd` shows oracle and subject on `~/.cache/oxpinyin/tkrzw`).
Revert-and-check on the reference cell is the mutation table above: the new tests
compare against an in-test oracle, so the revert is a deliberate break of each
rule. `initbench run --rounds 0` and every timing run verified all four profiles
and the save/reopen scenario comparable with the pin (stderr and leftover files
equal), 0 samples rejected.

## Measurements

One host (Linux x86_64, 4 vCPU KVM guest, kernel 6.18), Berkeley DB, pinyin
facade, `--cpu 2`, warm cache, identical system data and fixture bytes
(`initbench profiles`). Valgrind 3.22.0. Built with `tools/packaging/install.sh
libpinyin … -- --locked --release --no-default-features --features bdb,shipped`.

| object | bytes | SHA-256 |
| --- | ---: | --- |
| pin `libpinyin.so` (2.11.92 @ `074a2219`, BDB) | 4,802,600 | `754499651c302efd91fe7f72dd6a791978400b268cb6206d2ebe644015721f30` |
| base `libpinyin.so.15.0.0` (`6c665d44`) | 5,295,208 | `dd6ae15c1658d0388263cd21c65162cc854108ef82693acc5d562bcc8fbc769e` |
| opt `libpinyin.so.15.0.0` | 5,291,328 | `e491b9cbe9313415e664a5e198424a2e08ed231287761ad523338439393d8899` |

`base-control` is the base bytes at another path (`cmp` identical).

### Deterministic: instructions and allocations (stop after `init`)

| profile | child Ir base → opt | alloc calls base → opt | bytes allocated base → opt | peak live base → opt |
| --- | ---: | ---: | ---: | ---: |
| `empty` | 2,925,785 → 2,925,852 | 1,139 → 1,139 | 270,174 → 270,167 | 210,885 → 210,880 |
| `modest` | 3,319,693 → 3,319,071 | 1,555 → 1,552 | 370,792 → 370,741 | 212,545 → 212,540 |
| `phrases` | 106,985,053 → **101,076,677** (−5.5%) | 129,992 → **97,816** (−24.8%) | 11,000,653 → 10,546,086 (−4.1%) | 3,035,373 → 3,035,368 |
| `journals` | 106,928,044 → **100,933,273** (−5.6%) | 130,496 → **98,079** (−24.8%) | 10,846,603 → 10,361,450 (−4.5%) | 3,069,613 → 3,069,608 |

`phrases`, inclusive Ir: `load_logs_with_mask` 30.21 M → 24.28 M;
`decode_phrase_item` 8.39 M → 5.07 M (10,000 calls left, for the new items); the
equality closure 5.50 M → 5.85 M (it now also walks the wire bytes). The saving
is 5.9 M, not the 8.4 M of the old decode: the comparison absorbs part of it.
Live heap at the end of `init` is unchanged (1,523,251 → 1,523,248 on `phrases`).

### Wall clock (ms, median [95% bootstrap CI], `phrases` / `journals`)

Two 300-round runs, the second with the subject order reversed (the first had
`base` ahead of `opt` and showed a 3% gap between `base` and its own identical
control, so it is order-biased):

| run | metric | base | opt | base-control | opt/base | opt/base-control |
| --- | --- | ---: | ---: | ---: | --- | --- |
| order pin, base, opt, control | `phrases` `pinyin_init` | 13.600 | 12.920 | 13.143 | 0.95× [0.93–0.97] | 0.98× [0.97–1.01] |
| | `phrases` first candidate | 14.587 | 13.968 | 14.079 | 0.96× [0.94–0.98] | 0.99× [0.97–1.01] |
| | `journals` `pinyin_init` | 13.545 | 12.970 | 13.208 | 0.96× [0.94–0.97] | 0.98× [0.96–1.00] |
| | `journals` first candidate | 14.594 | 14.003 | 14.201 | 0.96× [0.94–0.98] | 0.99× [0.97–1.01] |
| order pin, opt, control, base | `phrases` `pinyin_init` | 14.232 | 14.260 | 14.359 | 1.00× [0.97–1.03] | 0.99× [0.97–1.02] |
| | `phrases` first candidate | 15.416 | 15.380 | 15.422 | 1.00× [0.97–1.03] | 1.00× [0.97–1.02] |
| | `journals` `pinyin_init` | 14.282 | 13.931 | 14.210 | 0.98× [0.94–1.01] | 0.98× [0.94–1.02] |
| | `journals` first candidate | 15.401 | 15.040 | 15.330 | 0.98× [0.94–1.01] | 0.98× [0.94–1.02] |

Against the pin (first run): `pinyin_init` on `phrases` is 4.81× (base) and 5.06×
(opt), time to first candidate 3.30× and 3.45× — the same within the run's
position bias. A 60-round run including `empty` and `modest` (pin, base, opt,
control) gave `opt/base` `pinyin_init` 1.01× [0.95–1.07] and 0.95× [0.89–1.02],
first candidate 1.00× [0.94–1.07] and 0.97× [0.91–1.05]: no change, as the
instruction counts say. Pin figures there: `empty` 1.70 ms, `modest` 7.01 ms.

**Reading.** 5.5% of about 13 ms is 0.7 ms, which is the size of the position
bias and of the control's own spread on this host. Point estimates favour `opt`
in seven of the eight rows of the table, but no interval clearly excludes 1
against the same-bytes control. The honest claim is "at
most a few percent, not resolved here".

### Memory (memory mode, 30 rounds, median; `phrases`, `journals` within ±0.02)

| point · quantity | pin | base | opt | opt/base |
| --- | ---: | ---: | ---: | --- |
| init · anonymous MiB | 0.80 | 4.62 | 4.62 | 1.00× |
| init · peak RSS MiB | 11.92 | 15.55 | 15.55 | 1.00× |
| init · RSS Δ MiB | 9.15 | 12.67 | 12.67 | 1.00× |
| init · minor faults | 443 | 1,705 | 1,706 | 1.00× |
| first candidate · anonymous MiB | 0.82 | 4.62 | 4.62 | 1.00× |
| first candidate · peak RSS MiB | 12.24 | 15.70 | 15.70 | 1.00× |

Memory is unchanged, as the DHAT peak says. The base is still 3.8 MiB more
anonymous memory than the pin at init.

## Unresolved

- **The pin compares the whole old chunk by `memcmp` including its size**
  (`phrase_index.h:235`); `decode_phrase_item` ignores bytes after the last
  pronunciation, so an old payload with trailing bytes is a mismatch in the pin
  and a match here. That is the behaviour before this change and it is kept
  (the task requires it; changing it is a STOP). It is read from source, not run
  against the pin binary, and the case set has no case for it. Under
  `docs/findings/compatibility-policy.md` this matches none of classes (a)–(c);
  it needs a human decision and a pin differential case, not a fix in this PR.
  The same holds for the empty-phrase `grows` rule (`wire_len` is `None`, so the
  total is not moved), which the pin would size from the header.
- Remaining `phrases` hotspots after this change are unchanged from #845's list:
  the 10,009 `SharedDb::put` calls (33.8 M, mostly libdb) and `BdbTxn::put`
  (21.9 M) in the shared store, which are outside this PR's scope. `zhuyin` was
  not measured for time or memory; the change is the shared `persistence.rs`
  replay, and the pin differential ran both facades.

## Commands

```sh
. ~/.oxpinyin-oracle.env     # after tools/cloud/setup.sh --cells "bdb kc tkrzw"
tools/packaging/install.sh libpinyin --prefix=<p> --libdir=<p>/lib -- --locked --release \
    --no-default-features --features bdb,shipped       # base (main) and opt, one tree each
cargo build --release -p oxpinyin-initbench           # in the #842 checkout
initbench profiles --out <fx> --pin-lib $PINYIN_ORACLE_PREFIX/lib/libpinyin.so \
    --system-dir $OXPINYIN_LIBPINYIN_DATA_DIR
initbench instrument --lib <so> --facade pinyin --system-dir <data> --profile <fx>/phrases --tool callgrind|dhat
valgrind --tool=callgrind initbench child --facade pinyin --lib <so> --system-dir <data> \
    --user-dir <copy of fx/phrases/user> --stop-after init --kind timing   # per-function
initbench run --subject pin=… --subject base=… --subject opt=… --subject base-control=… \
    --system-dir <data> --profile <fx>/{empty,modest,phrases,journals} --facade pinyin \
    --cpu 2 --kind timing --rounds 300 --warmup 5 --out run.jsonl          # and --kind memory --rounds 30
initbench report --input run.jsonl --reference pin --ratio opt/base --ratio opt/base-control …
tools/bisection/run-contract-diff.sh <bdb|kc|tkrzw> <oracle-prefix> <pinyin.so> <zhuyin.so>
```

Raw JSONL and profiler output are not committed and were not retained
(ephemeral container).
