# RSS attribution — steady-cycle resident memory (2026-09-09 UTC)

Status: **diagnosis complete. No fix.** No shipping source changed. What
landed with this document is instrumentation — an `rss-diag` mode in
`tools/bisection/bisect.c`, its runner `tools/bisection/run-rss-diag.sh`,
the `tools/bisection/rss-smaps.py` and `tools/bisection/cg-calls.py`
readers, and a live/peak-bytes extension to the gated `alloc-count`
allocator, which is absent from every default and every shipped artifact.

The body below is the investigation in the order it ran, in three parts:
Phase 1 (where the resident memory sits), Phase 2 Step 1 (the Kyoto
Cabinet open parameters), and Phase 2 narrowed (the live block count and
the mechanism behind it). The **Summary** immediately following is the
whole result; everything after it is evidence for one of its claims.

## Summary

**The gap.** oxpinyin's steady-cycle RSS runs **1.205×** libpinyin's on
this configuration — 27,570 KiB against 22,886 KiB, a gap of 4,684 KiB —
reproducing the arch-invariant ~1.16–1.23× this work was commissioned
against, on a third environment and on the **shipped** artifact rather
than a non-shipping one.

**It is not allocator retention.** `malloc_trim(0)` after the cycles
recovers 352 KiB of oxpinyin's 27,570 and 88 KiB of libpinyin's: the gap
narrows by 264 KiB, and **94.4% of it survives**. The count-based
attribution that framed this task — 257,790 allocations and 25.06 MB per
cycle in four Rust sites — is measurably the wrong map for RSS. That
volume peaks at **1.28 MiB live** and settles at 816 KiB; the entire Rust
live heap is smaller than the gap it was hypothesised to cause.

**The gap is three things, and only one is addressable.** Native
measurement, mapping round:

| | KiB | share | standing |
|---|---:|---:|---|
| priced design decision — oxpinyin's user store | 1,157 | 24.7% | not addressable; the measured cost of Non-goal 1 in [`user-store.md`](user-store.md), recorded there |
| code residency and paging pattern | 1,632 | 34.9% | not addressable as memory work |
| **KC small-block record traffic** | **1,891** | **40.4%** | **the only actionable term** |

So **at most ~40% of the gap is addressable**, and the ceiling on any one
change is lower again. The 1,057/2,081 KiB figures used in the body are
**DHAT-side** — requested bytes live at the global maximum, under
valgrind, summing to the DHAT heap gap of 3,138 KiB; 961/1,891 are those
same two terms **rescaled onto the natively measured** `[heap]` gap of
2,852 KiB. The two instruments agree to about 10% on the total. Mixing
their absolutes does not add up, so both are labelled everywhere they
appear.

**The mechanism, and the decoy.** On both engines the live small blocks
are Kyoto Cabinet B+ tree leaf records, allocated as a leaf page is
decoded and then **retained in KC's page cache for the process lifetime**
— `#pccap` defaults to 64 MiB and neither engine comes near it, so
nothing is ever evicted. The live block count is therefore a monotone
function of how many *distinct leaf pages were ever touched*.

Two oxpinyin behaviours differ from upstream, and **the larger of the two
is not the one that matters**:

| | multiple | what it drives |
|---|---:|---|
| index probes issued: 4,712 vs 1,536 | **3.07×** | leaf pages faulted into a cache that never evicts ⇒ **the live block count, i.e. the RSS term** |
| value fetches performed: 4,712 vs 423 | **11.1×** | one `kcdbget` malloc + one `Vec` copy per fetch, **freed within the call** ⇒ transient churn, **not an RSS term** |

**State plainly, because this is the finding most likely to be misread:
the 11.1× is the decoy.** It is the bigger number, it is the more obvious
defect — `ChewingTable::search` fetches unconditionally where upstream's
`check()` short-circuits on 72.5% of probes, and 68% of oxpinyin's
fetches materialise an empty continuation marker and discard it — and it
contributes **nothing** to resident memory. `malloc_trim` recovering
almost nothing (§4) is the direct evidence: those buffers are already
gone. Anything aimed at the RSS gap must target the **probe count**.

**Behind the probe count: 3.11× path fan-out.** Both engines run an
expanding window with the *same* termination condition, and oxpinyin
opens slightly *fewer* windows (1,038 vs 1,162). It enumerates **4.11
complete key-paths per window against upstream's 1.32**. Breadth — with
decomposition and "more windows" ruled out against source rather than
assumed, and memoization neither established nor excluded by the evidence
taken here.
Why the scan matrix fans out ~3× against a bit-identical candidate
surface is **not answered here** and is filed as its own issue.

**Follow-ups, none of them taken here.**

| | where |
|---|---|
| Kyoto Cabinet's untuned `#bnum` costs ~4.8 MiB in **both** engines | shenghaoc/oxpinyin#402 |
| 3× scan-matrix path fan-out — the mechanism behind the addressable term | shenghaoc/oxpinyin#403 |
| **tkrzw, the shipped default, is unmeasured** | needs a host that can build it: the web environment's egress allowlist rejects every Debian repository and tkrzw has a bug on Ubuntu |

Two things this diagnosis produced that were not asked for and are worth
keeping: a **build-invariance control** (51,335 live blocks identical
across three build profiles) and a **tool finding** (valgrind's stack
unwind through the C-ABI boundary is wrong in a way that reads as a
result). Both are in §"Three notes for the record", and the tool finding
is also in [`../runbooks/benches.md`](../runbooks/benches.md) where the
procedure lives.

Every figure below was produced in this session on the host named under
**Environment**. Nothing is carried over from an earlier record; where an
earlier record is referred to, it is cited and its number is not reused as
if it were measured here.

## Scope and what these numbers can be read as

**They can be read as the ratio.** Cells L and S ran in one session, on one
host, against one data directory, on the same backend, round-robin, minutes
apart. L-versus-S under Kyoto Cabinet is a valid comparison.

**They cannot be read as the shipped default's absolute footprint.** RSS is
backend-sensitive in a way timing largely is not — different stores carry
different caches and mappings — so these are Kyoto Cabinet numbers and
describe the Kyoto Cabinet configuration. The shipped default backend is
tkrzw, which was not measured here and must not be assumed to sit at these
absolutes. Ubuntu 24.04's glibc 2.39, gcc 13.3 and Kyoto Cabinet 1.2.80
also differ from the `debian:testing` image the cross-host record used, so
the absolute values here are **not** comparable with
[`perf-steady-cycle-cross-host-2026-09-07.md`](perf-steady-cycle-cross-host-2026-09-07.md)
or with [`perf-backend-matrix-2026-09.md`](perf-backend-matrix-2026-09.md).
Only the within-session L/S ratios travel.

Drop-in scope throughout: both cells open **libpinyin's own installed
`data/`**. Nothing oxpinyin-generated is involved.

## Environment

Claude Code **web**, not a local host and not a container of our own: this
session already runs inside an Ubuntu 24.04 container whose egress
allowlist rejects every Debian repository, so the standing
`debian:testing` + tkrzw recipe could not be used. Ubuntu's archive is
reachable, so the dependencies were installed with `apt` and the work was
done in the environment as found, on Kyoto Cabinet. This is recorded as a
deviation from the standing recipe, not as a new default.

| Property | Value |
|---|---|
| Environment | Claude Code web container, Ubuntu 24.04.4 LTS |
| Kernel | `Linux 6.18.44-fc-v24 x86_64` |
| CPU / RAM | 4 cores; MemTotal 16,075 MiB |
| glibc | `Ubuntu GLIBC 2.39-0ubuntu8.7` |
| Compiler | `gcc (Ubuntu 13.3.0-6ubuntu2~24.04.1) 13.3.0` |
| Backend | Kyoto Cabinet `1.2.80-1build1` (`libkyotocabinet.so.16.14.0`) |
| glib | `2.80.0-6ubuntu3.8` |
| Toolchain | `rustc 1.97.1 (8bab26f4f 2026-07-14)`, matching `rust-toolchain.toml` `channel = "1.97.1"` — no override |
| cargo-c | `cargo-c 0.10.25+cargo-0.99.0` |
| oxpinyin tree | `61e09aa7ff042bac4f219f303b1e9917c1d54603` (branch point off `main`) |
| Oracle pin | libpinyin **2.11.92**, commit `074a2219c90feaf962d0d24f034514033ece5f99`, verified by `git rev-parse` after a depth-1 fetch |
| model20 | `59c68e89…` archive, fetched and SHA-verified by `tools/model/fetch-model.sh` |
| Capture window | 2026-09-09T05:10:33Z – 2026-09-09T05:14Z (UTC, from the measuring container); the Phase 2 profiling passes 2026-09-09T10:55Z – 11:55Z |
| Profilers (Phase 2 only) | `valgrind-3.22.0` — massif, DHAT, callgrind. Kyoto Cabinet frames symbolised with `libkyotocabinet16v5-dbgsym 1.2.80-1build1` from `ddebs.ubuntu.com`; the `.so` bytes are unchanged by installing it. |
| Host load | `loadavg[1m]` sampled before and after every round: min 0.78, median 0.78, max 0.80 over 48 samples |

### The two cells

| cell | build |
|---|---|
| **L** | libpinyin at the pin, `./configure --disable-static --with-dbm=KyotoCabinet`, `strip --strip-all`. `libpinyin.so.15.0.0`, 1,185,232 B, `sha256:61953fc3…`. NEEDED: glib-2.0, kyotocabinet.so.16, stdc++, m, c, gcc_s |
| **S** | oxpinyin through the **shipping** path — `tools/packaging/release-stage.sh kyotocabinet --prefix=/usr --libdir=/usr/lib`, i.e. `cargo cinstall --no-default-features --features kyotocabinet,shipped`, all its gates passed, stripped by the script. `libpinyin.so.15.0.0`, 1,597,240 B, `sha256:92b99c3c…`, SONAME `libpinyin.so.15`. NEEDED: kyotocabinet.so.16, glib-2.0, gcc_s, c, ld-linux |
| data | `/opt/libpinyin-kc/lib/libpinyin/data` for **both** cells |

**`shipped` composes with the backend feature — verified, not assumed.**
`--no-default-features --features kyotocabinet,shipped` builds and links,
and the store crate's one-backend `compile_error!` is satisfied:
`readelf -d` on the staged artifact shows `libkyotocabinet.so.16` and no
second backend. The gate is real in the other direction too — `nm -D` on
the staged artifact finds **no** `oxpinyin_init_for_fixtures` and **no**
`oxpinyin_alloc_*`.

This is cell **S**, not cell Z. The RSS figures this work was commissioned
against were taken on a `cargo cinstall` **without** `--features shipped`.

## Method

`bisect --perf` in the new `PERF_MODE=rss-diag`. One process per round:
`pinyin_init` + `pinyin_alloc_instance`, snapshot; then 8 keystroke cycles
over the frozen 20-input corpus (123 parse + guess + count steps each),
snapshot; then `malloc_trim(0)`, snapshot. Each snapshot reads
`/proc/self/status`, `/proc/self/smaps_rollup` and `mallinfo2()`.

12 rounds per cell, round-robin (L, S, L, S, …) so any drift in host state
lands on both cells rather than on whichever ran second. `taskset -c 0`.
Medians and full ranges below; nothing is averaged.

The `/proc/self/maps`, `/proc/self/smaps` and `malloc_info` text dumps are
written only when `RSS_DIAG_DIR` is set, and that is a **separate** round:
writing them allocates, which would move the very RSS the `malloc_trim`
delta measures.

The three-window protocol from the timing work is deliberately not used.
It is not needed: see **Spread** below.

Reproduce with:

```sh
tools/bisection/run-rss-diag.sh \
    --lp /opt/libpinyin-kc/lib/libpinyin.so.15.0.0 \
    --ox /opt/oxpinyin-kc-stage/usr/lib/libpinyin.so.15.0.0 \
    --data /opt/libpinyin-kc/lib/libpinyin/data \
    --out <dir> --rounds 12 --cycles 8
```

Raw captures are **not committed** — see **Provenance** below for the
command behind every figure, and for where the capture bundle lives.

## Results

12 rounds per cell. KiB. Median [min, max].

| quantity | L median | L range | S median | S range | S/L |
|---|---:|---|---:|---|---:|
| RSS after init | 16,892 | [16,864, 17,016] | 18,878 | [18,804, 18,924] | **1.118** |
| RSS after 8 cycles | 22,886 | [22,836, 22,936] | 27,570 | [27,500, 27,620] | **1.205** |
| RSS after `malloc_trim(0)` | 22,798 | [22,748, 22,848] | 27,218 | [27,148, 27,268] | **1.194** |
| VmHWM | 22,886 | [22,836, 22,936] | 27,730 | [27,672, 27,796] | 1.212 |
| init — anonymous | 6,184 | [6,184, 6,188] | 7,364 | [7,364, 7,368] | 1.191 |
| init — file-backed † | 10,708 | [10,680, 10,832] | 11,578 | [11,504, 11,624] | 1.081 |
| cycle — anonymous | 7,940 | [7,940, 7,944] | 11,208 | [11,208, 11,212] | **1.412** |
| cycle — file-backed † | 14,946 | [14,892, 14,996] | 16,362 | [16,292, 16,412] | 1.095 |
| cycle — `Private_Dirty` | 7,940 | [7,940, 7,944] | 11,212 | [11,212, 11,216] | 1.412 |
| cycle — `Private_Clean` | 12,848 | [12,820, 12,932] | 14,212 | [14,164, 14,240] | 1.106 |
| cycle — `Shared_Clean` | 2,104 | [2,064, 2,104] | 2,164 | [2,108, 2,168] | 1.029 |

† **File-backed is derived**, as `smaps_rollup`'s `Rss − Anonymous`, from
the medians of those two columns rather than as the median of the
per-round `file_kib` column — otherwise the rows do not reconcile, since
each column's median may come from a different round. Both derived
medians fall inside their measured ranges, and the measured column
medians differ by 2 KiB. The three `RSS after …` rows are
`/proc/self/status` `VmRSS`; the `anonymous`/`file-backed`/`Private_*`/
`Shared_Clean` rows are `smaps_rollup`. They are separate reads taken
microseconds apart and can disagree slightly: S's rollup `Rss` at init
medians 18,942 against `VmRSS`'s 18,878, which is why §1's table — which
is `smaps_rollup` throughout — shows 18,942 there.

The 1.205× steady-cycle ratio reproduces the arch-invariant ~1.16–1.23×
this work was commissioned against, on a third environment and a third
backend configuration, and on the shipped artifact rather than a
non-shipping one.

### Spread

The full range is 100 KiB on L's cycle RSS (0.44% of the median) and 120
KiB on S's (0.44%). The `mallinfo2` fields are **bit-identical across all
12 rounds of each cell** — `arena`, `uordblks`, `fordblks` and `hblks` do
not vary at all. RSS here is as deterministic as the brief expected, and
the wide-range finding the protocol guards against did not occur. No
averaging was needed and none was done.

### Reproduction, and a provenance note on the primary capture

**The primary capture's harness binary predates two later additions to
`tools/bisection/bisect.c`** — the per-mapping `smaps` dump and the
`dlsym` reads of the gated allocator's counters — so its
`identity.txt` names the two `.so` files but not the harness that drove
them. Neither addition can move the captured quantities: the measurement
rounds run with `RSS_DIAG_DIR` unset, so the dump code never executes,
and the counter resolution is four `dlsym` calls returning null for both
cells before `pinyin_init` is reached. But "cannot" is an argument, so
the capture was **repeated end to end with the final harness**, and
`run-rss-diag.sh` now records the repository revision and the SHA-256 of
both `bisect` and `bisect.c` in every identity table it writes.

Second capture, 12 rounds per cell, same host, same artifacts, 2026-09-09
12:38:30Z (`identity-recapture.txt`, `rounds-recapture.jsonl` in the
capture bundle — see **Provenance**):

| | capture 1 L | S | S/L | capture 2 L | S | S/L |
|---|---:|---:|---:|---:|---:|---:|
| RSS after init | 16,892 | 18,878 | 1.118 | 17,148 | 19,168 | **1.118** |
| RSS after 8 cycles | 22,886 | 27,570 | 1.205 | 23,204 | 27,864 | **1.201** |
| RSS after `malloc_trim(0)` | 22,798 | 27,218 | 1.194 | 23,112 | 27,512 | 1.190 |
| cycle — anonymous | 7,940 | 11,208 | 1.412 | 7,940 | 11,208 | **1.412** |
| cycle — file-backed | 14,946 | 16,362 | 1.095 | 15,262 | 16,656 | 1.091 |
| `uordblks` | 7,565,216 | 10,321,616 | 1.364 | 7,565,200 | 10,321,616 | **1.364** |

**The anonymous figures are bit-identical across sessions** and
`uordblks` agrees to 16 bytes, which is the strongest statement this
record can make about determinism. Every ratio holds to within 0.4%.
What moved is file-backed residency — L +316 KiB, S +294 KiB, landing on
both cells almost equally — which is a page-cache warmth difference
between sessions, not an implementation difference; it is the same
whole-session offset
[`perf-steady-cycle-cross-host-2026-09-07.md`](perf-steady-cycle-cross-host-2026-09-07.md)
records for timing, and the reason only within-session quantities are
compared here.

The per-mapping attribution reproduces too, on the headline terms
exactly: `[heap]` **+2,852**, `pinyin_index.bin` **+1,344**,
`user_store.kct` **+196**, `[anon]` **+216** — identical to §2's table in
capture 1. The code-residency rows move by tens of KiB with the same
page-cache warmth.

> **Measured in session, not retained.** This paragraph is the one claim
> in the reproduction that rests on capture 2's `smaps` dumps, which were
> written to the container's temporary directory and never copied into the
> capture bundle. The rounds and identity tables for capture 2 *were*
> retained; these mapping figures were read once, in session, and are
> reported at that strength. Everything else in this section comes from
> `rounds-recapture.jsonl`.

**The figures throughout the rest of this document are capture 1's**, as
originally taken. They are not restated from capture 2, which is here as
a control and as the fully-provenanced run.

## 1. Anonymous versus file-backed

Both engines' resident sets are majority **file-backed**, and both grow
mostly file-backed pages during the cycles.

`smaps_rollup` throughout, so the columns reconcile (see the footnote to
Results: the `Rss` row here is rollup's, not `VmRSS`).

| | L init | L cycle | S init | S cycle |
|---|---:|---:|---:|---:|
| Rss | 16,892 | 22,886 | 18,942 | 27,570 |
| Anonymous | 6,184 | 7,940 | 7,364 | 11,208 |
| file-backed (Rss − Anonymous) | 10,708 | 14,946 | 11,578 | 16,362 |
| file-backed share | 63.4% | 65.3% | 61.1% | 59.3% |

**The headline the brief flagged as possible did not occur.** libpinyin's
resident set is not substantially file-backed where oxpinyin's is
anonymous: oxpinyin maps the same files and pages in a comparable amount
of them. Since the P6 runtime switch
([`runtime-direct-libpinyin-data-2026-09-02.md`](runtime-direct-libpinyin-data-2026-09-02.md))
oxpinyin reads libpinyin's own `MemoryChunk` files directly, and §2 shows
that reaching all the way down to identical per-file residency.

But the **gap** is mostly anonymous. Of the 4,684 KiB steady-cycle gap:

| | KiB | share of gap |
|---|---:|---:|
| anonymous | +3,268 | **69.8%** |
| file-backed | +1,416 | 30.2% |
| total | +4,684 | 100% |

## 2. Which mappings

Per-mapping `Rss`, one mapping round per cell, from `/proc/self/smaps`
(`tools/bisection/rss-smaps.py --diff`). KiB.

### After 8 cycles

| mapping | L | S | S − L |
|---|---:|---:|---:|
| `[heap]` | 7,444 | 10,296 | **+2,852** |
| `DATA:pinyin_index.bin` | 3,572 | 4,916 | **+1,344** |
| `DATA:addon_pinyin_index.bin` | 980 | 180 | **−800** |
| `CODE:libkyotocabinet.so.16.14.0` | 272 | 816 | +544 |
| `CODE:libpinyin.so.15.0.0` (the artifact itself) | 1,076 | 1,448 | +372 |
| `[anon]` (non-heap anonymous) | 204 | 420 | +216 |
| `user_store.kct` (in the per-process user dir) | 0 | 196 | +196 |
| `CODE:libglib-2.0.so.0` | 640 | 512 | −128 |
| `CODE:libc.so.6` | 1,556 | 1,620 | +64 |
| `[stack]` | 24 | 44 | +20 |
| **every other mapping** | | | **±0** |
| TOTAL | 22,872 | 27,552 | +4,680 |

**`DATA:gb_char.bin`, `gbk_char.bin`, `opengram.bin`, `merged.bin`,
`punct.bin`, `phrase_index.bin`, `addon_phrase_index.bin` and `bigram.db`
are resident to the byte in both cells.** That is the direct answer to
"does libpinyin mmap data structures that oxpinyin loads onto the heap":
for the phrase-library chunk files and the phrase index, **no** — both map
the same files with the same residency. Establishing this directly, rather
than inferring it, was the point of the mapping round.

Both engines map the same ten files from the installed `data/` (the four
`*_index.bin`/`bigram.db` as `r--s`, the phrase-library chunks as `rw-p`).
S maps one file L does not: its own `user_store.kct`, +196 KiB.

The `pinyin_index.bin` / `addon_pinyin_index.bin` pair is one finding, not
two: L pages in 3,572 + 980 = 4,552 KiB across the two, S pages in
4,916 + 180 = 5,096 KiB — a **net +544 KiB**, from a different lookup
pattern over the same two files, not from a different representation.

### After init

| mapping | L | S | S − L |
|---|---:|---:|---:|
| `[heap]` | 5,768 | 6,836 | **+1,068** |
| `CODE:libkyotocabinet.so.16.14.0` | 272 | 816 | +544 |
| `CODE:libpinyin.so.15.0.0` | 1,076 | 1,384 | +308 |
| `user_store.kct` | 0 | 196 | +196 |
| `CODE:libc.so.6` | 1,556 | 1,620 | +64 |
| `CODE:libglib-2.0.so.0` | 576 | 512 | −64 |
| `[anon]` | 128 | 44 | −84 |
| `[stack]` | 24 | 44 | +20 |
| **every DATA: mapping** | | | **±0 (−16 on `addon_pinyin_index.bin`)** |
| TOTAL | 16,892 | 18,928 | +2,036 |

At init the data mappings are identical between the cells. The init gap is
`[heap]` plus code residency plus the extra user store.

## 3. Init versus growth

| | L | S | S − L | S/L |
|---|---:|---:|---:|---:|
| RSS after init | 16,892 | 18,878 | **+1,986** | 1.118 |
| RSS after 8 cycles | 22,886 | 27,570 | **+4,684** | 1.205 |
| growth added by the cycles | +5,994 | +8,692 | **+2,698** | 1.450 |

**58% of the steady-cycle gap accrues during the cycles**, not at init. The
brief's clue holds and is now quantified: the incremental growth under
keystroke work is the larger share, and S's cycle growth is 1.45× L's.

Per mapping, from the mapping round's own two snapshots (whose growth
totals are +5,980 for L and +8,624 for S, a growth gap of +2,644 KiB):

| | L growth | S growth | difference |
|---|---:|---:|---:|
| `[heap]` | +1,676 | +3,460 | **+1,784** |
| `DATA:pinyin_index.bin` | +3,380 | +4,724 | +1,344 |
| `DATA:addon_pinyin_index.bin` | +784 | 0 | −784 |
| `[anon]` | +76 | +376 | +300 |
| `CODE:libpinyin.so.15.0.0` | 0 | +64 | +64 |
| `CODE:libglib-2.0.so.0` | +64 | 0 | −64 |
| **every other mapping** | 0 | 0 | 0 |
| TOTAL | +5,980 | +8,624 | **+2,644** |

## 4. The decisive experiment: `malloc_trim(0)`

| | L | S | gap |
|---|---:|---:|---:|
| RSS after 8 cycles | 22,886 | 27,570 | 4,684 |
| RSS after `malloc_trim(0)` | 22,798 | 27,218 | **4,420** |
| released | −88 (0.38%) | −352 (1.28%) | |

**The gap does not collapse. 94.4% of it survives the trim.**

`malloc_trim(0)` returned 352 KiB of S's 27,570 KiB — 1.3% of its
resident set. Because L gave back 88 KiB at the same time, the gap itself
narrowed by only 264 KiB, or 5.6%. Whatever holds oxpinyin's extra resident memory,
glibc does not consider it free, so it is not allocator retention driven
by transient churn.

By the brief's own decision rule, this puts the work in the second branch:
**the memory is genuinely held, the count-based attribution is the wrong
map for RSS, and any Phase 2 must attribute by live bytes.**

The counting allocator makes the same point independently and much more
sharply — see §6.

## 5. glibc arena state

`mallinfo2()` after the cycles. Bytes. Identical in every one of the 12
rounds per cell.

| field | L | S | S/L |
|---|---:|---:|---:|
| `arena` (sbrk heap) | 7,720,960 | 10,924,032 | 1.415 |
| `uordblks` (in-use chunk bytes) | 7,565,216 | 10,321,616 | **1.364** |
| `fordblks` (free chunks retained) | 155,744 | 602,416 | 3.868 |
| `hblks` (mmap'd regions) | 1 | 2 | 2.0 |
| `hblkhd` (mmap'd region bytes) | 135,168 | 602,112 | 4.455 |
| `keepcost` (releasable top) | 83,280 | 426,624 | 5.123 |
| `fordblks` after `malloc_trim` | 73,824 | 176,432 | 2.390 |
| `arena` after `malloc_trim` | 7,639,040 | 10,498,048 | 1.374 |

`malloc_info` shows **one heap and one arena** for both cells — no
per-thread arena proliferation on either side.

The load-bearing row is `uordblks`: S holds **+2,756,400 bytes (+2,691
KiB) more in in-use chunks** than L. That is within 6% of the +2,852 KiB
`[heap]` residency gap. The heap gap is live, in-use allocation — not
fragmentation, not retention. Retained free chunks (`fordblks`) differ by
only 436 KiB, and after the trim by only 100 KiB.

## 6. Live and peak live bytes — the gated counting allocator

`crates/oxpinyin-capi/src/alloc_count.rs`, previously counting only
cumulative calls and cumulative requested bytes, now also tracks
**currently-live bytes** and **peak live bytes** (`dealloc` subtracts;
`realloc` posts only the difference; peak is a relaxed `fetch_max`). Five
`oxpinyin_alloc_*` readers are exported, and `bisect` resolves them with
`dlsym` and reports `-1` where they are absent — so the JSON shape is the
same for libpinyin and for any default artifact.

Gate verified both ways with `nm -D --defined-only`:

* shipped artifact (`--features kyotocabinet,shipped`) — **0** matches for
  `oxpinyin_alloc`;
* instrumented artifact (`--features kyotocabinet,shipped,alloc-count`) —
  `oxpinyin_alloc_bytes`, `oxpinyin_alloc_count`,
  `oxpinyin_alloc_live_bytes`, `oxpinyin_alloc_peak_live_bytes`,
  `oxpinyin_alloc_reset_peak`.

`bisect` reads the initialization counters, then calls
`oxpinyin_alloc_reset_peak` immediately before the cycle loop, so the
`after 8 cycles` peak below is the **cycles' own** high-water mark rather
than the larger of the two regions. On this workload that distinction
changes nothing — initialization peaks at 13,167 B against the cycles'
1,339,514 B, so the maximum was already the cycle's — but the column now
means what its name says on a workload where it would not be.

Three rounds of the instrumented artifact, identical to the last byte in
all three:

> **Measured in session, not retained — the weakest provenance in this
> document.** These eight figures have **no capture file at all**. They
> came from three ad-hoc runs of the `alloc-count` artifact whose stdout
> was read and not saved; `rounds.jsonl` and `rounds-recapture.jsonl`
> record the **shipped** artifact, which does not carry the counters and
> reports `-1` for all four fields. The runs are reproducible from the
> command in **Provenance**, and their agreement with the branch's
> independently-derived per-cycle figures (§ below) is the only external
> check on them. Read them as one session's reading, not as a retained
> measurement.

| | after init | after 8 cycles |
|---|---:|---:|
| allocating calls | 212 | 2,061,587 |
| cumulative bytes requested | 19,943 | 202,053,959 |
| **live bytes** | 11,487 | **835,647** |
| **peak live bytes** | 13,167 | **1,339,514** |

Two things follow, and they are the most important numbers in this
document.

**First, the instrument agrees with the branch's existing count-based
figures**, which is the cross-check that makes the rest of the table
trustworthy: 2,061,587 / 8 = **257,698 allocating calls per cycle** and
202,053,959 / 8 = **25.26 MB per cycle**, against the
`perf/steady-cycle-constant-factor-mkeg4k` branch's 257,790 and 25.06 MB
per cycle. Same workload, same order of magnitude, independently derived.

**Second, that 25.26 MB per cycle never exists at once.** Peak live is
**1.28 MiB** (1,339,514 B) and steady live is **816 KiB** (835,647 B) — the
transient churn is **19× the peak live bytes and 30× the steady live
bytes**.

Stated precisely, because "allocates and drops, so holds no resident
memory" is not quite true: freeing a block ends its *liveness*, but glibc
does not necessarily return the page to the kernel, so churn can leave
allocator-resident bytes behind. That retention was measured rather than
assumed, and it is small. `fordblks` — free chunks the allocator is
holding — stands at 602,416 B for oxpinyin against 155,744 B for
libpinyin, a difference of **436 KiB**, and `malloc_trim(0)` actually
returns 352 KiB of it (§4). Against a `[heap]` gap of 2,852 KiB and a
total gap of 4,684 KiB, **the retention this churn leaves behind is far
too small to explain either**. What the churn does not do is stay live,
and that is what the next paragraph turns on.

This does not merely fail to explain the RSS gap; it cannot explain it.
The whole Rust live heap at cycle end is 816 KiB, against a `[heap]` gap
of 2,852 KiB and a total gap of 4,684 KiB. **Rust's live allocations are
smaller than the gap they were hypothesised to cause.** By subtraction,
9.49 MB (9.05 MiB) of S's 10.32 MB in-use heap (`uordblks` 10,321,616 B)
is allocated by C — the Kyoto Cabinet backend through `kclangc`, and glib in the C-ABI
marshalling — and does not pass through Rust's `GlobalAlloc` at all.

## Reading

1. **The premise the branch's documents carried is disconfirmed for RSS.**
   The four-site count-based attribution (111,643 calls at
   `phrase_libraries.rs:249`; `finish_grow`; `decode_items`;
   per-candidate `CString::new`) describes 25 MB per cycle of allocation
   that peaks at 1.31 MiB live and is then freed. `malloc_trim` recovers
   352 KiB of it. It is a plausible map for the **instruction** gap, which
   is not this task; it is the wrong map for the resident-memory gap. The
   brief's warning that "count and resident bytes can point at entirely
   different code" is confirmed by measurement, and the Phase 3 candidate
   list — which is drawn from the count ranking — should be treated as
   unsupported until a byte ranking exists.

2. **The gap is five things, not one.** From the mapping round, whose
   own S − L total is +4,680 KiB (against +4,684 KiB on the 12-round
   medians — the two agree to 0.1%):

   | | KiB | share |
   |---|---:|---:|
   | `[heap]`, live in-use allocation, almost all C-side | +2,852 | 61.0% |
   | pinyin/addon index paging pattern, net | +544 | 11.6% |
   | `libkyotocabinet.so` code residency | +544 | 11.6% |
   | the artifact's own code + data residency | +372 | 7.9% |
   | oxpinyin's `user_store.kct` mapping | +196 | 4.2% |
   | `[anon]`, `[stack]`, libc, glib | +172 | 3.7% |

3. **The largest term is not Rust's.** `uordblks` places +2,691 KiB of
   live in-use heap on S's side, and the counting allocator places only
   816 KiB of live Rust bytes in the whole process. The `[heap]` gap therefore
   sits overwhelmingly in allocations oxpinyin makes **through** the
   backend and glib, not in its own data structures. That is a different
   problem from the one the Phase 3 candidate list addresses, and it is a
   Kyoto-Cabinet-specific finding that may not transfer to tkrzw — which
   is the shipped default and was not measured here.

4. **Growth beats init.** 58% of the gap appears during the cycles. The
   growth is `[heap]` (+1,784 KiB of the +2,644 KiB growth gap in the
   mapping round) and the index paging pattern (+1,344/−784). Since `malloc_trim` recovers almost
   none of it and `uordblks` accounts for nearly all of it, this growth is
   the working set the engine actually holds while typing, not high-water
   residue.

5. **Nothing here points at Stage 2's binary text-model work.** The brief
   asked this to be said plainly rather than rediscovered under a new
   name. The standing Stage 2 goal — compiling the text model to a binary
   format at data-prep time — targets a representation oxpinyin loads onto
   the heap. That is not what these measurements found. Every phrase
   library chunk file and the phrase index are resident to the byte in
   both engines; the largest gap term is C-side backend allocation. This
   diagnosis neither supports nor advances that Stage 2 item, and the two
   should not be conflated.

6. **Timing was not measured and is not claimed.** The cycle timings the
   harness emits were not analysed and no timing figure appears in this
   document. Anything downstream of this diagnosis that plausibly moves
   timing must be measured separately, on
   `perf/steady-cycle-constant-factor-mkeg4k`'s terms and not here.

## What this diagnosis cannot say

* **Nothing about the shipped tkrzw default's RSS.** One backend was
  measured, as the brief scoped. The `[heap]` term in particular is
  backend-mediated and the tkrzw number could differ substantially. A
  second backend was not run, and expanding to one was deliberately not
  done.
* **Nothing about which C allocation sites hold the +2,691 KiB.** The
  counting allocator sees Rust only. Attributing the C-side heap needs a
  different instrument (`LD_PRELOAD` malloc tracing, heaptrack, or massif)
  and is not in Phase 1's scope.
* **Nothing about why the two index files page differently.** That the
  net is +544 KiB is measured; the lookup-pattern cause is not.
* **Nothing comparable to the record's absolutes.** Different distro,
  glibc, compiler and Kyoto Cabinet version. Only the ratios travel.

## Standing at the Phase 1 STOP

*Recorded as it stood at the time; Phase 2 follows below.*

The diagnosis is in. Phase 2 as briefed — attribute by live bytes — has a
measured shape that differs from the one anticipated: the live bytes that
matter are mostly not Rust's, so a Phase 2 built only on the Rust counting
allocator would attribute at most 816 KiB of a 4,684 KiB gap.

---

# Phase 2, Step 1 — the Kyoto Cabinet open parameters (2026-09-09 UTC)

Added after the Phase 1 STOP cleared. The framing of Kyoto Cabinet as a
test-only proxy is **withdrawn**: both backends are supported upstream and
exposed in oxpinyin, and the merged cross-host record carries KC cells
throughout. A KC user's RSS is a real user's RSS. Findings below are
classified as:

* **backend-independent** — applies to every configuration;
* **KC-internal, identical configuration on both sides** — a real cost for
  KC users, and any excess is oxpinyin's usage pattern;
* **KC-internal, different configuration on the two sides** — a **parity
  finding first**, reported as a drop-in behavioural divergence
  independent of what it does to memory.

The tkrzw default is **unmeasured here** and is named as a follow-up on a
host that can build it. That is a scope note, not a reason to withhold
anything.

## The answer: the open parameters are identical, and both are defaults

**Neither engine passes `#bnum=`, `#msiz=`, `#pccap=`, `#apow=`, `#fpow=`,
`#psiz=` or `#opts=` — on any database.**

**libpinyin** (pin `074a2219`) constructs the C++ classes directly and
calls `open(path, mode)` with a bare path. `attach_options()`
(`src/storage/kyotodb_utils.h:32-45`) maps its flags to
`OREADER`/`OWRITER`/`OCREATE` and nothing else; no `tune_*` call appears
anywhere in the KC backend:

| table | class | site |
|---|---|---|
| `pinyin_index.bin`, `addon_pinyin_index.bin` | `TreeDB` | `chewing_large_table2_kyotodb.cpp:87-89` |
| `phrase_index.bin`, `addon_phrase_index.bin` | `TreeDB` | `phrase_large_table3_kyotodb.cpp:102-104` |
| `punct.bin` | `TreeDB` | `punct_table_kyotodb.cpp:62-64` |
| `bigram.db` | `HashDB` | `ngram_kyotodb.cpp:117-119` |
| `user_bigram.db` | `StashDB`, in memory, `open("-")` | `ngram_kyotodb.cpp:53-62`, reached from `pinyin.cpp:399-401` |

**oxpinyin** goes through the C API, which is `PolyDB`, and appends
exactly one parameter — `#type=kct` or `#type=kch`
(`crates/oxpinyin-store/src/kyotocabinet/ffi.rs:291-338`, with
`DbType::tuning()` at `:140-145`). That override exists because
`PolyDB` picks the class from the path suffix and libpinyin's files are
named `.bin`/`.db`; it is not tuning.

**`PolyDB` with only `type=` set is byte-for-byte the same object.** Its
parser initialises every tuning variable to a sentinel
(`kcpolydb.h:458-471`: `bnum = -1`, `msiz = -1`, `psiz = -1`,
`apow = -1`, `fpow = -1`, `pccap = 0`) and the construction branches call
each setter only when the sentinel moved (`kcpolydb.h:796-849`:
`if (bnum > 0) …`, `if (msiz >= 0) …`, `if (pccap > 0) …`). With no
tuning keys present it runs `new TreeDB()` / `new HashDB()` and calls no
setter — the same construction libpinyin performs by hand.

Class defaults therefore apply identically on both sides, from Kyoto
Cabinet 1.2.80's own headers:

| | `TreeDB` (`kcplantdb.h:77-85`) | `HashDB` (`kchashdb.h:102-112`) |
|---|---|---|
| `bnum` | 65,536 | 1,048,583 |
| `psiz` | 8,192 | — |
| `pccap` | 64 MiB | — |
| `msiz` | (inner HashDB) 64 MiB | 64 MiB |
| `apow` / `fpow` | 8 / 10 | 3 / 10 |

**Confirmed empirically, not only from source.** Every shared database is
mapped at an identical extent in both cells (from the mapping round's
`smaps`, KiB):

| file | L mapped | S mapped | L Rss | S Rss |
|---|---:|---:|---:|---:|
| `pinyin_index.bin` | 5,108 | 5,108 | 3,572 | 4,916 |
| `phrase_index.bin` | 3,668 | 3,668 | 192 | 192 |
| `addon_pinyin_index.bin` | 1,056 | 1,056 | 980 | 180 |
| `addon_phrase_index.bin` | 796 | 796 | 136 | 136 |
| `punct.bin` | 396 | 396 | 16 | 16 |
| `bigram.db` | 22,040 | 22,040 | 64 | 64 |

Identical mapped extents mean an identical `msiz` in effect. **No case-3
finding exists among the six shared databases**; Step 1 lands in case 2
throughout.

### The one configuration difference is already-settled, not a new divergence

libpinyin's **user** bigram is an in-memory `StashDB` opened on `"-"` and
snapshotted to disk only at `pinyin_save`; oxpinyin's user store is a live
on-disk KC `TreeDB` opened for write (`user_store.kct`). That is not a
tuning-parameter divergence and not unreported: it is Non-goal 1 of
[`user-store.md`](user-store.md) — "**Not** reproducing libpinyin's binary
user-data format … redb is the store; only the values and semantics are
the target" — with the durability consequence settled there too. Recorded
here only because it now has a measured cost: **+196 KiB resident**, and
**64 MiB of address space** (exactly `HashDB::DEFMSIZ`), which is the
whole of S's `VmSize` ≈ 125 MB against L's ≈ 63.7 MB.

One further difference, with no RSS consequence: libpinyin maps the
phrase-library chunk files `rw-p`, oxpinyin `r--p`. Same extent, same
residency, `Private_Clean` in both — recorded for completeness.

## What Step 1 uncovered: the heap is Kyoto Cabinet's page cache

Since the open parameters are identical, the `[heap]` term had to come
from the number of databases held open and the access pattern. DHAT gives
that directly, and it is the by-live-bytes attribution Phase 2 was asked
for.

**Method.** `valgrind --tool=dhat`, 2 cycles, same data directory, same
`bisect` driver. Ranked by bytes **live at the global maximum** (`gb`),
with per-site peak-live (`mb`) alongside; addresses resolved through
`/proc/self/maps` captured from inside the same process. Symbolisation
needs unstripped objects, so this pass used the unstripped libpinyin and
`target/release/libpinyin_capi.so` — the **same** build configuration as
cell S (`--no-default-features --features kyotocabinet,shipped`), only
without release-stage's strip. Under valgrind absolute RSS is meaningless;
the attribution is what is being read, and it is symmetric across cells.

| | L | S |
|---|---:|---:|
| live at global max | 7,179,236 B | 10,392,220 B |
| blocks live at global max | 22,832 | 51,335 |

The 3,212,984 B (3,138 KiB) difference is close to the natively measured
`[heap]` gap of 2,852 KiB, which is the cross-check that lets this pass
stand in for it.

**Every large site is Kyoto Cabinet's `PlantDB` page cache**, and it has a
signature that identifies it exactly: `524,672 B in 16 blocks`, which is
`create_leaf_cache()` (`kcplantdb.h:2399-2409`) allocating one
`LinkedHashMap` per slot — `SLOTNUM` 16 slots × `nearbyprime(bnum/16+1)` =
4,099 buckets × 8 B. `create_inner_cache()` (`:2822-2830`) is the matching
`32,896 B in 16 blocks` (16 × 257 × 8). Each open `TreeDB` allocates a
**hot** and a **warm** leaf cache plus one inner cache:

**1,082,240 B — 1,057 KiB — per open TreeDB, fixed by `#bnum` alone, and
independent of how much data is read or how it is accessed.**

Counting those signatures gives the whole story, and it agrees exactly
with an independent `strace` count of concurrently-open database files:

| | leaf slot-sets | inner slot-sets | ⇒ open `TreeDB`s | fixed page-cache bytes | share of live-at-max |
|---|---:|---:|---:|---:|---:|
| L | 10 | 5 | **5** | 5,411,200 | 75.4% |
| S | 12 | 6 | **6** | 6,493,440 | 62.5% |

`strace -e trace=openat,close`, peak concurrently-open database files:
L = `pinyin_index.bin`, `phrase_index.bin`, `addon_pinyin_index.bin`,
`addon_phrase_index.bin`, `punct.bin` (five TreeDBs) + `bigram.db`
(a `HashDB`, which has no `PlantDB` page cache); S = the same six **plus**
`user_store.kct`.

So the largest single heap item **in both engines** is a Kyoto Cabinet
default that neither of them tunes. libpinyin spends 5.16 MiB on it,
oxpinyin 6.19 MiB.

### The heap gap, attributed

| | bytes | KiB | share of the DHAT gap | class |
|---|---:|---:|---:|---|
| the sixth `TreeDB`'s page cache (the user store) | 1,082,240 | 1,057 | 33.7% | KC-internal, identical config; the extra open is oxpinyin's, and is `user-store.md`'s settled decision |
| everything else — KC small-block record and cursor traffic live at the peak | 2,130,744 | 2,081 | 66.3% | KC-internal, identical config; **excess is oxpinyin's usage pattern** |
| total | 3,212,984 | 3,138 | 100% | |

The second row is now the biggest open item, and the block counts sharpen
it: S holds **51,335** live blocks at its peak against L's **22,832**,
2.25×. At glibc's ~16 B per-chunk header that is a further ~445 KiB of
heap that never appears in requested-byte figures at all. One S site alone
holds 1,164,068 B in 46,199 blocks live at the global maximum.

## Classification, and what follows

**Backend-independent (proceed).** The artifact's own code residency
(+372 KiB) and whatever share of the small-block traffic is glib
marshalling rather than KC. Neither is the leading term.

**KC-internal, identical configuration (proceed; unmeasured on tkrzw).**
Everything above. Two actionable items, in size order:

1. **KC small-block traffic, +2,081 KiB and +28,503 live blocks.** The
   excess is oxpinyin's access pattern against an identically configured
   store — cursor-based `walk`/`range_raw` against libpinyin's targeted
   `get`s is the obvious suspect, and it is the same divergence that shows
   up in the mapping table as `pinyin_index.bin` +1,344 KiB paged in.
   Worth noting that this cost is *shaped* by KC but is a property of how
   oxpinyin asks, so a tkrzw measurement is likely to find something
   analogous rather than nothing.
2. **The sixth TreeDB, +1,057 KiB.** oxpinyin holds its user store open as
   a file `TreeDB` for the process lifetime where libpinyin holds an
   in-memory `StashDB`. The decision is settled; the *cost* is newly
   measured, and whether the store needs to be open when idle is a
   separate question from what format it uses.

**KC-internal, different configuration: none found.** Worth stating the
converse explicitly, because it constrains the fix space: **lowering
`#bnum` or `#pccap` on oxpinyin's read-only opens would create a case-3
difference where none exists today.** It would be cheap — dropping `#bnum`
to 4,096 on the five read-only system TreeDBs would cut ~4.77 MiB of
bucket array from *both* engines' resident sets — but it is a deliberate
divergence in drop-in configuration, it trades space for lookup time with
the time side unquantified, and libpinyin carries the same untuned
default, which makes it a candidate for
[`upstream-report-drafts.md`](upstream-report-drafts.md) rather than a
local-only change.

**Filed as its own issue rather than carried here: shenghaoc/oxpinyin#402.**
~4.8 MiB off both engines is a bigger result than this task was scoped for
and a different kind of result — a shared-cost tuning decision about a
Kyoto Cabinet default, not a parity question. The issue carries the
mechanism, the estimate, the divergence it would create, and the
complexity-rule requirement that the time side be measured first.

**tkrzw, the shipped default, is unmeasured.** Nothing above has been
observed on it. The `PlantDB` page-cache term is Kyoto-Cabinet-specific by
construction and has no reason to transfer; the access-pattern term
plausibly does. This needs a host that can build tkrzw — the web
environment's egress allowlist rejects every Debian repository, and tkrzw
has a bug on Ubuntu — so it is named here as the follow-up it is.

## The addressable fraction of the gap

Stated explicitly, because "the RSS gap" must not read as uniformly
reducible. It is not, and reading it that way has already cost this
workstream once.

Apportioning the mapping round's `[heap]` term by the DHAT split
(33.7% the sixth `TreeDB`, 66.3% small-block traffic — §"The heap gap,
attributed"), the +4,680 KiB steady-cycle gap divides into three columns
with genuinely different standing:

| | KiB | share | standing |
|---|---:|---:|---|
| **Priced design decision** — the user store: its `TreeDB` page cache (≈961) plus its `user_store.kct` mapping (196) | **1,157** | **24.7%** | Not addressable. The measured cost of Non-goal 1 in [`user-store.md`](user-store.md), recorded there next to the decision. Not a fix target. |
| **Code pages and paging pattern** — index paging net (+544), `libkyotocabinet.so` code residency (+544), the artifact's own code (+372), `[anon]`/`[stack]`/libc/glib (+172) | **1,632** | **34.9%** | Not addressable as memory work. Code residency is a property of which code paths run; the index paging is the same access-pattern fact as the row below, seen from the mmap side rather than the heap side. |
| **KC small-block record traffic** | **1,891** | **40.4%** | **The only actionable term.** |

So **at most ~40% of the gap is addressable**, and the ceiling on any
single change is lower than that again.

Two arithmetic notes, so the numbers can be checked rather than trusted:

* the 1,057 KiB and 2,081 KiB figures in §"The heap gap, attributed" are
  **DHAT-side** — requested bytes live at the global maximum, measured
  under valgrind, summing to the DHAT heap gap of 3,138 KiB. The 961 and
  1,891 above are those same two terms rescaled onto the **natively
  measured** `[heap]` gap of 2,852 KiB. The two instruments agree to
  about 10% on the total; mixing their absolutes would not add up.
* the +4,680 KiB base is the mapping round's own total. The 12-round
  median gap is +4,684 KiB; the two agree to 0.1%.

## Phase 2, narrowed — the 2.25× live block count

**Question.** What accounts for oxpinyin holding **51,335** live blocks at
peak against libpinyin's **22,832**, when both run the same store with
identical configuration? The ~445 KiB of glibc chunk headers is a symptom
of block count, so the target is what holds 2.25× as many small
allocations live — not their total bytes.

**Attribution only. Nothing is proposed here.**

### Method, and one instrument that did not work

DHAT names the allocation site: on **both** engines the live small blocks
are Kyoto Cabinet B+ tree leaf-node records, allocated in
`PlantDB<HashDB,49>::load_leaf_node(...)::VisitorImpl::visit_full`
(`kcplantdb.h:2663`) as a leaf page is decoded, and then **retained in the
leaf page cache for the process lifetime** — `#pccap` defaults to 64 MiB
and neither engine comes near it, so nothing is ever evicted. The live
block count is therefore a monotone function of how many *distinct* leaf
pages were ever touched, not of an instantaneous working set.

| cell | DHAT sites | blocks | avg |
|---|---|---:|---:|
| L | four `visit_full` leaf sites + one `load_inner_node` site | 21,293 of 22,832 (93%) | 22–47 B |
| S | two `visit_full` leaf sites + one `load_inner_node` site | 47,126 of 51,335 (92%) | 23–36 B |

**DHAT's call stacks above the FFI boundary are not usable on the oxpinyin
side and were not used.** Valgrind unwinds the libpinyin cell correctly —
its stacks resolve cleanly to `pinyin_guess_candidates` →
`search_matrix` → `search_matrix_recur` → `ChewingLargeTable2::search` —
but on the oxpinyin cell the frames above `kcdbget` do not form a real
call chain (they name `Arc<PunctTable>::drop_slow` calling
`dict::ucs4_walk_key` calling `PhraseLibrary::open`, and put
`pinyin_iterator_add_phrase` directly under `main`, which the harness
never calls). This was checked, not assumed: rebuilding at
`debug = 2` with LTO off, and again with `-C force-frame-pointers=yes`,
produced byte-identical stacks — and identical block counts (51,335 in all
three builds), which is itself the useful result that the allocation
behaviour does not depend on the build profile.

So the caller attribution below comes from **callgrind caller→callee call
counts**, which are exact and need no unwinding, taken on the same two
cells with the same driver and workload.

> **Measured in session, partially retained.** The caller→callee *edges*
> — every arrow in the two chains below — come from
> `cg-calls.py … callers`, which reads the raw callgrind out-files. Those
> are in the capture bundle but were never committed, and the committed
> extracts this document previously carried were **calls-to only**, so no
> committed artifact ever held the edges. Six cited figures were also
> outside the extracts' filter: `kcfree` 4,965 / 4,712-from-`get_raw`,
> `decode_items` 1,498, `build_scan_matrix` 246, `flush_window_batch`
> 2,076, `lookup_and_append` 4,262 (and its callers 2,978 / 1,284), and
> `ChewingTableEntry<N>::search` 266/132/18/2 = 418. All are regenerable
> from the bundle with the commands in **Provenance**; none was verifiable
> from the tree, and that was true before the captures were removed.

### The chains, named

Two keystroke cycles, 20 inputs, 123 steps each. Call counts.

**libpinyin**

```
pinyin_guess_candidates
  search_matrix                                        2,324
    search_matrix_recur                                2,804
      ChewingLargeTable2::search                       1,536
        search_internal<1..5>          798/450/186/78/24 = 1,536
          BasicDB::check   (inlined; 1,290 accept edges)        size probe, no allocation
          BasicDB::get(kbuf, ksiz, vbuf, vsiz)                   fills a REUSED MemoryChunk
            (152 call edges + inlined sites; its visitor ran 423 times)
            PlantDB::accept                            1,442
              load_leaf_node                           1,442
                HashDB::accept_impl                      119   page-cache miss: read the page
                  visit_full                             108   ← one heap block per record
```

**oxpinyin**

```
Session::lookup_and_append                             4,262
  RuntimeDict::lookup_into                             4,262
    SystemDictionary::lookup_into                      4,262
      SystemDictionary::fill_lookup                    4,262
        ChewingTable::search                           4,262
          RawChewingDbm<KcStore>::get / KcStore::get_raw  4,712   (+450 from key_exists)
            kcdbget                                    4,960   (+246 user store, +2 txn)
              PlantDB::accept                          4,962
                search_tree 4,968 / load_inner_node    9,424
                load_leaf_node                         4,973
                  HashDB::accept_impl                    255   page-cache miss: read the page
                    visit_full                           238   ← one heap block per record
```

`SystemDictionary::phrase_prefix_exists` → `ChewingTable::key_exists`
accounts for the other 450; `RuntimeDict::lookup_addon_into` →
`AddonDictionary::lookup_into` is called 4,262 times alongside the system
path.

### What the counts say

| | L | S | S/L |
|---|---:|---:|---:|
| dictionary-layer lookups issued | 1,536 | 4,712 | **3.07×** |
| KC tree walks (`PlantDB::accept`) | 1,442 | 4,962 | 3.44× |
| tree walks **per lookup** | 0.94 | 1.05 | 1.12× |
| inner nodes loaded | 2,318 | 9,424 | 4.07× |
| leaf pages read from file (`HashDB::accept_impl`) | 119 | 255 | 2.14× |
| leaf pages decoded (`visit_full`) | 108 | 238 | **2.20×** |
| **small blocks live at peak** | **22,832** | **51,335** | **2.25×** |

The block ratio (2.25×) tracks the leaf-page-decode ratio (2.20×) almost
exactly, and that in turn is downstream of the lookup count. **The
per-lookup cost is essentially identical** — 0.94 tree walks per lookup on
libpinyin against 1.05 on oxpinyin. Nothing about how oxpinyin performs a
lookup is more expensive in blocks; it performs **3.07× as many of them**,
against the same tables, over the same 123 keystroke steps.

That is also the same fact the mapping table records from the other side:
more distinct leaf pages touched is exactly why `pinyin_index.bin`
residency is +1,344 KiB.

### One structural difference in the same function pair, recorded not proposed

libpinyin's `search_internal<N>`
(`chewing_large_table2_kyotodb.cpp:149-178`) probes with
`m_db->check(kbuf, N * sizeof(ChewingKey))` and returns on `-1`; only
**152 of 1,290** probes go on to fetch a value, and that fetch is
`BasicDB::get(kbuf, ksiz, vbuf, vsiz)` — the overload that fills a
caller-owned, reused `MemoryChunk` rather than returning an allocation.
oxpinyin's `ChewingTable::search` (`chewing_table.rs:342-356`) issues the
full `get` unconditionally; its `key_exists` analogue
(`chewing_table.rs:360-365`) exists but on this path is reached only from
`phrase_prefix_exists`.

Recorded because it is the visible structural difference between the two
functions, **not** as an explanation of the block count: `check` and `get`
both walk the tree and both can fault in a leaf page, so the cheap probe
would not by itself reduce the number of pages decoded. What it changes is
value materialisation, which is transient bytes, not live blocks.

### The open question this leaves

**Why does the engine issue 4,262 dictionary lookups where libpinyin
issues 1,536?** Both drive the same 123 keystroke steps over the same
corpus and the same tables. libpinyin reaches `ChewingLargeTable2::search`
through `search_matrix_recur`; oxpinyin reaches `fill_lookup` through
`Session::lookup_and_append`. Answering that is a question about the
lookup/segment-graph traversal, not about the store or the backend, and it
is where the only actionable ~40% of the RSS gap lives. Nothing is
proposed for it here.

## Settling the `check()`-versus-`get()` question with counts

Filed above as a non-explanation. Counted here, because unconditional
fetch into a fresh allocation versus conditional fetch into a reused
buffer is exactly the shape that produces small-block traffic, and if
oxpinyin fetches on every probe the block excess has two causes with
different fixes.

**A correction to the counts published above.** `BasicDB::check` and, at
most call sites, `BasicDB::get` are **inlined** into
`search_internal<N>`, so their call edges under-report them: only 152
`BasicDB::get` edges survive. The visitor objects are not inlined —
they are heap-allocated and virtually dispatched — so
`BasicDB::…::VisitorImpl::visit_full` is the reliable instrument, and
`ChewingTableEntry<N>::search` corroborates it. The ladder below uses
those. The KC-layer figures published earlier (`PlantDB::accept`
1,442/4,962, `load_leaf_node`'s `visit_full` 108/238,
`load_inner_node` 2,318/9,424) were re-verified against callgrind's own
`(object, file, name)` identity and are unchanged.

| | libpinyin | oxpinyin |
|---|---:|---:|
| index probes issued | **1,536** | **4,712** |
| probes that found a key | 670 (`check`'s visitor) | 4,712 — every `kcdbget` returned a buffer |
| **value fetches performed** | **423** (`get`'s visitor) | **4,712** (`kcfree` from `get_raw`: 4,712) |
| fetches that carried data and were decoded | 418 (`ChewingTableEntry<N>::search`) | 1,498 (`decode_items`) |
| **fetch rate** | **27.5%** | **100%** |
| cost of a fetch | none — `BasicDB::get(kbuf, ksiz, vbuf, vsiz)` fills `entry->m_chunk`, a reused `MemoryChunk` | one `kcdbget` malloc + one Rust `Vec` copy, freed on drop |

**Yes — unconditionally.** `ChewingTable::search`
(`crates/oxpinyin-data/src/chewing_table.rs:342-356`) issues
`self.dbm.get(&index_key(keys))?` and only then tests `value.is_empty()`.
`kcfree` is called 4,712 times from `KcStore::get_raw`, exactly matching
the 4,712 `get_raw` calls: every probe allocated and freed a buffer.
**3,214 of those 4,712 fetches (68%) materialise an empty continuation
marker and discard it.**

Upstream never fetches those. `search_internal<N>`
(`chewing_large_table2_kyotodb.cpp:149-178` at pin `074a2219`) reads
`check`'s three-way result — `-1` absent, `0` present-but-empty,
`> 0` present-with-data — and only the third case reaches `get`. Of 670
keys found, 247 were empty continuation markers and were never fetched.

### The two causes are separable, and differently sized

| cause | measure | drives |
|---|---:|---|
| **more probes** | 4,712 vs 1,536 — **3.07×** | leaf pages faulted into KC's page cache, retained for the process lifetime ⇒ the **live** block count, i.e. the RSS term |
| **unconditional fetch, into a fresh allocation** | 4,712 vs 423 — **11.1×** | one KC malloc + one `Vec` copy per fetch, freed immediately ⇒ **transient** churn: instructions and time, **not** RSS |

The second is the larger multiple and does **not** show up in the
resident-set measurement at all — the buffers are freed within the call,
and §4 established that `malloc_trim` recovers almost nothing. It is
recorded here because it is real and because it points somewhere else:
see the note on the instruction gap below. **Phase 3 targeting the RSS
gap must target the probe count, not the fetch rate.**

## Characterising the 3.07× — it is breadth

Of the three candidates — breadth, memoization, decomposition — the
counts say **breadth**. "More windows" and decomposition are ruled out
below. Memoization is **not** ruled out: the evidence taken here cannot
decide it, and the earlier claim that it could is withdrawn — see below.
Breadth is established positively, on the window and path counts, and does
not rest on memoization having been excluded.

**Not more windows.** Both engines run an expanding window from the
anchor and stop on the same signal. Upstream:
`pinyin_guess_candidates` (`src/pinyin.cpp:2229-2262` at pin
`074a2219`) loops `for (size_t end = start + 1; end < matrix.size();)`
and `break`s on `!(retval & SEARCH_CONTINUED)`. oxpinyin:
`collect_window_scan`
(`crates/oxpinyin-engine/src/session/lookup.rs:879-923`) loops
`while end <= bound` and `break`s on `if !continued`. **The early
termination the upstream shape suggested is present on both sides**, and
oxpinyin in fact opens slightly *fewer* windows:

| | libpinyin | oxpinyin | ratio |
|---|---:|---:|---:|
| windows opened | 1,162 (`search_matrix` 2,324 ÷ 2 tables) | 1,038 (`flush_window_batch` 2,076 ÷ 2 batches) | **0.89×** |
| complete key-paths searched | 1,536 | 4,262 | **2.77×** |
| **paths per window** | **1.32** | **4.11** | **3.11×** |
| scans (keystroke steps reaching the scan) | — | 246 (`build_scan_matrix`) | — |

**Not decomposition.** Both issue exactly **one** index probe per
complete key-path — `search_internal<N>` → one `check`;
`ChewingTable::search` → one `get_raw` — and both span key lengths 1..5
(upstream's per-length split is 798/450/186/78/24). The same coverage is
not being reached through more, smaller queries; the queries are the
same size and there are more of them.

**Memoization: not established, and not ruled out.** The evidence
available here does not decide it. Aggregate decode rates — 1,498 of
4,262 oxpinyin searches decode a record (35%) against 418 of 1,536
upstream (27%) — say how often a probe found data, and say **nothing**
about whether the probed keys were distinct, repeated, or already
resolved: a workload that re-probes the same hot key repeatedly would
show a high decode rate, not a low one. An earlier draft of this section
read that 35% as evidence that oxpinyin's extra paths reach more distinct
index entries. **It is not evidence of that** and the claim is
withdrawn.

Settling it needs a measurement this pass did not take: the count of
*unique* index keys probed per window on each side, or a per-key hit
count. Both are straightforward to add to `ChewingTable::search` and to
`search_internal<N>` under a gated counter, and neither was needed for
the breadth conclusion below, which rests on the window and path counts
alone.

So: same window count, same termination, same query granularity — and
**3.11× as many complete key-paths enumerated inside each window**.
`build_scan_matrix` (`lookup.rs:887`) builds the matrix the path walk
fans out over; `scan_paths`/`visit_scan_key` (`:927-959`) enumerate it.
Upstream's equivalent fan-out is `search_matrix_recur`
(`src/storage/phonetic_key_matrix.cpp:351-410` at the pin), which walks
`matrix->get_column_size(start)` entries per column with no pruning of
its own.

**The open question, one level down from before and not answered here:
why does oxpinyin's scan matrix carry ~3× the path fan-out per window
that upstream's phonetic key matrix does, when both produce a
bit-identical candidate surface?** The candidate surface is pinned
identical on all 10,190 corpus rows, so the extra paths cannot be
producing extra candidates — they are additional index probes whose
results are folded into the same answer. Whether that is tone or fuzzy
variants materialised as separate matrix keys where upstream folds them
into one incomplete index key is the thing to measure next; it is a guess
and is recorded as one.

**Filed as its own issue, scoped larger than this RSS term:
shenghaoc/oxpinyin#403.** It carries these numbers, the two eliminated
hypotheses and the one left open so nobody re-runs them, the argument that the fan-out is a
candidate common cause for three open items — this RSS term, the ~35%
instruction gap, and the arm64 timing spread — with that commonality
marked **unverified and worth checking early, because it changes how much
a fix is worth**, and the observation that a bit-identical surface makes
pruning behaviour-inert if the pruning condition is right, which puts the
whole risk and the whole test on the parity gate.

## Three notes for the record

### This likely reaches past RSS — a pointer, not a claim

3.07× as many index probes and 11.1× as many value fetches, each into a
fresh allocation, is a **candidate contributor to the ~35% steady-cycle
instruction gap and to the arm64 timing spread**. It is not measured as
such here and nothing in this document claims it: the instruction
differential is
[`perf-cycle-ir-differential-2026-09-08.md`](perf-cycle-ir-differential-2026-09-08.md)'s
subject on `perf/steady-cycle-constant-factor-mkeg4k`, and the timing
work is out of scope by this task's terms.

It is written down because three investigations may have been circling
one cause. The allocation work found 257,790 allocations per cycle
concentrated in four sites; this work finds the store issuing 3× the
probes and 11× the fetches; the timing work finds a constant factor that
survives every architecture. **The next person should check whether the
scan-matrix path fan-out is upstream of all three** before treating them
as separate problems.

### The build-invariance control

Nobody asked for this and it is worth keeping. **51,335 live blocks at
peak, identical across three builds** — the shipped-recipe release
artifact, `debug = 2` with LTO off, and the same again with
`-C force-frame-pointers=yes` — and cumulative totals identical to within
35 bytes of 64,275,794. The allocation behaviour under measurement does
not depend on the build profile, so the block counts are trustworthy
independently of the stack-attribution failure recorded next, and a
profiling build may be substituted for the shipped one for attribution
without qualification.

### DHAT's cross-FFI stacks were wrong in a way that looked plausible

Recorded as a tool finding so the next person does not repeat the three
rebuilds. This belongs beside the single-architecture debuginfo
neutrality check as another instance of an instrument being wrong in a
way that reads as a result.

On the **libpinyin** cell, valgrind unwinds correctly: stacks resolve to
`pinyin_guess_candidates` → `search_matrix` → `search_matrix_recur` →
`ChewingLargeTable2::search` → `search_internal<N>`, which is exactly
right.

On the **oxpinyin** cell, every frame above `kcdbget` is garbage — and
*symbol-table-plausible* garbage, which is what makes it dangerous. The
reported chain has `Arc<PunctTable>::drop_slow` calling
`dict::ucs4_walk_key` calling `PhraseLibrary::open`, and puts
`pinyin_iterator_add_phrase` directly beneath `main` — a function the
harness never calls. Each address really does fall inside the named
function's symbol range, so nothing looks obviously broken; only reading
the chain as a chain reveals it cannot be one.

What was tried, all of it fruitless:

1. `--num-callers=24` instead of the default 12 — deeper, same garbage.
2. Resolving return addresses at `addr - 1` rather than the return
   address itself, the standard off-by-one fix — no change.
3. Kyoto Cabinet debug symbols (`libkyotocabinet16v5-dbgsym` from
   `ddebs.ubuntu.com`) — this **did** fix the KC half of the stack, which
   is how the `PlantDB::load_leaf_node` frames became readable, and
   changed the oxpinyin half not at all.
4. Rebuilding at `debug = 2` with `lto = false` instead of the
   `profiling` profile's `line-tables-only` + thin LTO — byte-identical
   stacks.
5. Rebuilding again with `-C force-frame-pointers=yes` — byte-identical
   stacks.

The failure is confined to frames above the C-ABI boundary in the
dlopened Rust cdylib; it is not a symbolisation problem and not a
debuginfo problem, and forcing frame pointers does not fix it.

**The way through is not to unwind at all.** callgrind records
caller→callee edges with exact call counts and needs no stack walk, and
every caller attribution in this document comes from those edges. Two
cautions learned in the process, both of which produced wrong numbers
before they were caught:

- **Match callee identity, not the demangled name.** A needle of
  `BasicDB::get(char const*, unsigned long, char*, unsigned long)` also
  matches
  `BasicDB::get(char const*, unsigned long, char*, unsigned long)::VisitorImpl::visit_full`,
  which is a different function with a different count. The first
  caller-side pass silently summed the two and produced 575 where the
  answer was 152.
- **Inlined callees have no call edge.** `BasicDB::check` and most
  `BasicDB::get` sites are inlined into `search_internal<N>`, so their
  call counts under-report by ~3×. Where a function may be inlined, count
  something that cannot be — a virtually-dispatched visitor, a heap
  allocation — and corroborate it against a second such marker.

## Provenance — the command behind every figure

**No captures are committed with this document.** The environment was
ephemeral (Claude Code web), so unlike the amd64 and arm64 passes there is
no path on a physical machine to point at. What replaces the path is this
section: the exact command that produced each class of figure, so a number
is regenerable rather than merely asserted.

### The capture bundle

`rss-attribution-2026-09-09-raw.tar.gz`, 220 KiB,
`sha256:cda45e7eba91611cf9935aa6df817ec6d4f7e15104d2366bdb27630d2d304456`,
18 files: the 12 that were briefly committed (two `rounds*.jsonl`, two
`identity*.txt`, two `callgrind-calls-*.txt`, four `smaps` dumps, two DHAT
JSONs) plus the two raw callgrind out-files and capture 2's four `smaps`
dumps, neither of which was ever committed.

**The bundle was not retained.** It was assembled inside an ephemeral
container and that container is gone. There is no link and there will not
be one. Reproduce with the commands below instead; the SHA-256 is kept
only so that anyone who does still hold a copy can verify it is the same
bundle.

### Toolchain and packages

Ubuntu 24.04. One line, copy-pasteable; `autopoint` and `intltool` are
load-bearing — libpinyin's `autoreconf --force --install` fails without
them:

```sh
sudo apt-get update && sudo apt-get install -y \
    build-essential pkg-config git curl autoconf automake libtool \
    autotools-dev gettext autopoint intltool libglib2.0-dev \
    libkyotocabinet-dev libsqlite3-dev libclang-dev zlib1g-dev
```

For the profiling passes only (§"What Step 1 uncovered" onward), plus the
Kyoto Cabinet debug symbols that make its frames readable:

```sh
sudo apt-get install -y valgrind
echo "deb http://ddebs.ubuntu.com $(lsb_release -cs) main universe" \
    | sudo tee /etc/apt/sources.list.d/ddebs.list
sudo apt-get install -y ubuntu-dbgsym-keyring && sudo apt-get update
sudo apt-get install -y libkyotocabinet16v5-dbgsym
```

Rust comes from `rust-toolchain.toml` (1.97.1) via rustup; the shipping
path additionally needs cargo-c:

```sh
cargo install cargo-c@0.10.25 --locked
bash tools/model/fetch-model.sh          # SHA-verified model20 export
```

### Cells

```sh
# L — libpinyin at the pin, Kyoto Cabinet, stripped
git init /tmp/libpinyin-src && git -C /tmp/libpinyin-src fetch --depth=1 \
    https://github.com/libpinyin/libpinyin.git \
    074a2219c90feaf962d0d24f034514033ece5f99
git -C /tmp/libpinyin-src checkout --detach FETCH_HEAD
cp target/model20/extracted/* /tmp/libpinyin-src/data/     # tools/model/fetch-model.sh
cd /tmp/libpinyin-src && autoreconf --force --install \
  && ./configure --prefix=/opt/libpinyin-kc --disable-static --with-dbm=KyotoCabinet \
  && make -j"$(nproc)" && make install
strip --strip-all /opt/libpinyin-kc/lib/libpinyin.so.15.0.0

# S — oxpinyin through the shipping path (strips as part of its gates)
tools/packaging/release-stage.sh kyotocabinet \
    --prefix=/usr --libdir=/usr/lib --dest=/opt/oxpinyin-kc-stage
```

### Figures, by section

| figures | command |
|---|---|
| Results, §1, §3 init/cycle/trim rows, §4 `malloc_trim`, §5 `mallinfo2` — and `identity.txt` | `tools/bisection/run-rss-diag.sh --lp /opt/libpinyin-kc/lib/libpinyin.so.15.0.0 --ox /opt/oxpinyin-kc-stage/usr/lib/libpinyin.so.15.0.0 --data /opt/libpinyin-kc/lib/libpinyin/data --out <dir> --rounds 12 --cycles 8` → `rounds.jsonl` |
| Reproduction table | the same command, re-run 2026-09-09T12:38:30Z → `rounds-recapture.jsonl`, `identity-recapture.txt` |
| §2 per-mapping tables, §3 growth table | the mapping round the runner takes last (`RSS_DIAG_DIR` set), then `tools/bisection/rss-smaps.py --diff <L-smaps-{init,cycle}.txt> <S-smaps-{init,cycle}.txt>` |
| Step 1 mapped extents and `r--s`/`rw-p`/`r--p` perms | read directly from the same `smaps` dumps — the `Size:` field and the permission bits of each mapping header. `rss-smaps.py` does not emit these two columns; they were taken with an ad-hoc reader, and `grep -A1 'libpinyin/data' <dump>` recovers them |
| Step 1 page-cache table; Phase 2 block counts, byte totals, per-site rows, the `524,672 B × 16 blocks` and `32,896 B × 16 blocks` signatures | `PERF_MODE=rss-diag PERF_CYCLES=2 valgrind --tool=dhat --num-callers=24 --dhat-out-file=dhat.<cell> tools/bisection/bisect --perf <so> /opt/libpinyin-kc/lib/libpinyin/data`, then a census of the `pps` array's `gb`/`gbk`/`tb`/`tbk` fields. **The `fs` stack trees in that JSON are not usable above the C-ABI boundary** — see the tool finding in §"Three notes"; nothing in this document rests on them |
| check/get ladder, breadth tables, both call chains | `PERF_MODE=speed PERF_CYCLES=2 valgrind --tool=callgrind --cache-sim=no --branch-sim=no --callgrind-out-file=cg.<cell> tools/bisection/bisect --perf <so> <data>`, then `tools/bisection/cg-calls.py cg.<cell> calls <name>` and `… callers <name>` |
| §6 allocation counters | `cargo cinstall --locked --release -p oxpinyin-capi --no-default-features --features kyotocabinet,shipped,alloc-count --destdir=/opt/oxpinyin-kc-alloccount --prefix=/usr --libdir=/usr/lib`, then the `rss-diag` invocation above against that artifact. **Not retained** — see the note at the table |
| symbolisation of Kyoto Cabinet frames | `libkyotocabinet16v5-dbgsym 1.2.80-1build1` from `ddebs.ubuntu.com`; the `.so` bytes are unchanged by installing it |
| gate checks | `nm -D --defined-only <so> \| grep oxpinyin_alloc`; `readelf -d <so> \| grep NEEDED` |

### Reading the captures back

The commands above write the captures; these turn them into the tables in
this document. Both readers are committed; the DHAT census is a one-liner
because nothing in the tree reads DHAT and nothing needed to.

```sh
# §2 per-mapping tables and §3's growth table
tools/bisection/rss-smaps.py --diff <out>/maps/L-smaps-init.txt \
                                    <out>/maps/S-smaps-init.txt
tools/bisection/rss-smaps.py --diff <out>/maps/L-smaps-cycle.txt \
                                    <out>/maps/S-smaps-cycle.txt

# Step 1's mapped extents and the r--s / rw-p / r--p perms: read from the
# same dumps. rss-smaps.py does not emit those two columns.
grep -B1 -E '^(Size|Rss):' <out>/maps/L-smaps-cycle.txt | grep -A1 'libpinyin/data'

# check/get ladder, breadth tables, and both call chains. `calls` gives
# calls TO a function; `callers` gives who called it, which is the only
# sound source for the arrows -- valgrind's stack unwind is not (see the
# tool finding in §"Three notes").
tools/bisection/cg-calls.py <out>/cg.L calls   'ChewingLargeTable2::search(int'
tools/bisection/cg-calls.py <out>/cg.S callers 'kcdbget'

# Step 1's page-cache table and the Phase 2 block counts: a census of the
# DHAT program points. `gb`/`gbk` are bytes/blocks live at the global
# maximum; the 524,672 x 16 and 32,896 x 16 signatures are one
# create_leaf_cache / create_inner_cache slot set each.
python3 - <<'EOF'
import collections, json
for cell in ('L', 'S'):
    d = json.load(open(f'dhat.{cell}'))
    pps = d['pps']
    print(cell, 'at t-gmax:',
          sum(p.get('gb', 0) for p in pps), 'B in',
          sum(p.get('gbk', 0) for p in pps), 'blocks')
    sig = collections.Counter((p.get('gb', 0), p.get('gbk', 0)) for p in pps)
    for (b, k), n in sorted(sig.items(), key=lambda kv: -kv[0][0])[:8]:
        if b:
            print(f'   {b:>10,} B x {k:>6,} blk -> {n} site(s)')
EOF
```

### What is not regenerable, and reads at lower strength

Three groups of figures were measured in session and not written to any
retained file. Each is marked at its point of use, and none is load-bearing
for a conclusion that does not have an independent check:

1. **§6's eight allocation-counter figures** — no capture file exists;
   `rounds*.jsonl` record the shipped artifact, which reports `-1`. Their
   only external check is agreement with the branch's independently-derived
   per-cycle count and byte volume.
2. **The reproduction's per-mapping paragraph** — capture 2's `smaps`
   dumps went to a temporary directory. They are in the bundle; they were
   never committed.
3. **The callgrind caller→callee edges and six cited call counts** — the
   raw out-files are in the bundle but were never committed, and the
   extracts this document briefly carried were calls-to only and filtered.

## Closing — what this diagnosis delivers, and what it deliberately does not

**Delivered.** The steady-cycle RSS gap is decomposed to named mappings,
named allocation sites and named call paths, on the shipped artifact, with
every figure produced in one session on one host and every instrument's
scaling labelled. The premise the workstream carried in — that the
per-cycle allocation count explains the resident-memory gap — is
disconfirmed by two independent measurements. The addressable fraction is
bounded at ~40% and the mechanism behind it is characterised down to a
3.11× path fan-out, with two alternative explanations eliminated against
upstream source rather than assumed and the third — memoization — left
explicitly open, since the evidence taken here cannot decide it.

**Not delivered, on purpose: no fix.** The mechanism is "3.11× path
fan-out inside each window". There is nothing narrow to change until it is
known *which* paths fan out and *why* they survive pruning. A change
invented from what is known today would be a guess carrying a
10,190-row parity surface, and this workstream has already paid once for
building conclusions on an artifact that turned out to be the wrong
subject. The fan-out is #403; the `#bnum` tuning decision is #402; the
user store's cost is priced in [`user-store.md`](user-store.md) and is not
a fix target.

**Not delivered, by scope.** Timing. Nothing here measures the
steady-cycle ratio, the arch spread, or anything on
`perf/steady-cycle-constant-factor-mkeg4k`. The pointer in §"Three notes"
that the probe and fetch counts may reach those is a pointer and is marked
as one.

**Not delivered, by environment.** **tkrzw, the shipped default backend,
is unmeasured.** Every number in this document is Kyoto Cabinet on Ubuntu
24.04. The `PlantDB` page-cache term is a Kyoto Cabinet structure and has
no reason to transfer; the access-pattern term plausibly does, since it is
a property of how oxpinyin asks rather than of what answers. Measuring it
needs a host that can build tkrzw — this environment's egress allowlist
rejects every Debian repository, and tkrzw has a bug on Ubuntu — and that
is named as the follow-up it is, not as a caveat that weakens the ratios
above. Two cells on one backend, in one session, on one host, give a valid
ratio; the absolutes describe that backend's configuration and nothing
else.
