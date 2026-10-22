# Init memory and time against the pin, re-attributed (2026-10-22 UTC)

Status: **measurement only.** No code changed. Refs #982.

This re-runs #982's per-mapping attribution on current `main` after #977,
#1000, #1002, #1004, #1007, #1008 and #1010 merged, and adds init timing.
Raw captures were not kept: every figure below came from tool output in a
scratch directory that was discarded after this document was written. The
command behind each figure is given where the figure is used.

## Summary

- **Pss at M1 (after init, options, alloc and the first candidate) is now
  at the pin's level in all four cells:** pinyin `phrases` 8,336 KiB
  against 8,470 (−134), pinyin `journals` 8,377 against 8,329 (+48), zhuyin
  `phrases` 8,026 against 8,007 (+19), zhuyin `journals` 8,066 against
  8,015 (+51). Rss is about 1,000 KiB below the pin in every cell, and
  Private_Dirty is 140–164 KiB below.
- **Two contributors are still above the pin** by more than the residency
  bound. They are offset by mappings below the pin, chiefly the libstdc++
  that the pin maps and oxpinyin does not:
  1. Facade library text: **+572 KiB** (libpinyin) / **+532 KiB**
     (libzhuyin) of Pss. This was +804 / +920 in #986 before function
     ordering (#1000, #1002). Owner: #986 (closed).
  2. Duplicate replay views of system phrase files: **+172 KiB**
     (`phrases`) / **+212 KiB** (`journals`) of Pss, unchanged from #985.
     Owner: #985 (open).
- The relro difference that #987 found is gone. The facade library's
  `r--` mappings now hold 52 / 40 KiB of Private_Dirty against the pin's
  88. The #989 zhuyin addon and punct mappings are also gone.
- **Init time:** ox/pin is 1.03–1.09 on the warm cache (all four cells are
  STABLE). Library load is 0.43–0.44, and the first candidate is 1.74–2.05
  (zhuyin `journals` is INCONCLUSIVE). Cold timing was not measured; see
  [Cold cache](#cold-cache).
- **Deterministic:** init Ir is 0.937–0.946× the pin. The Massif peak heap
  is 0.93× (`phrases`) and 0.80× (`journals`). The control did not equal
  the subject exactly; see [Deterministic figures](#deterministic-figures).

## Setup

| item | value |
| --- | --- |
| base | `origin/main` 26c41440 (contains e5667e93, fb787035, 933dcb8d, 9f588276, 800fddca, b26f35c6, 78ceb76e) |
| pin | libpinyin 074a2219, Berkeley DB cell from `tools/cloud/setup.sh --cells bdb` |
| pin libpinyin.so.15.0.0 sha256 | `754499651c302efd91fe7f72dd6a791978400b268cb6206d2ebe644015721f30` (equals `oracle-pin.txt`) |
| pin libzhuyin.so.15.0.0 sha256 | `cf4ccbce9f70f6c7e412b92a52a243ba64fdea1ff197cbc44019c6b85650d53b` (equals `oracle-pin.txt`) |
| subject libpinyin.so.15.0.0 | sha256 `565738eb6c7182449607c78ad73a8049da2cf2ec7eb704e499dc7a9f2d95a205`; installed 5,122,672 B; stripped 2,425,584 B |
| subject libzhuyin.so.15.0.0 | sha256 `8ed600d383e5a18e45d02bf00c944eb42763586395f181cfd9113f45f8d3e600`; installed 4,853,472 B; stripped 2,188,016 B |
| function ordering | applied: `install.sh` printed `function ordering: using measured policy …/libpinyin.order` and `…/libzhuyin.order`; the first `.text` symbols after `_init` follow the policy file order (`nm -n`) |
| linker | GNU ld (GNU Binutils for Ubuntu) 2.47 |
| cargo-c | 0.10.24 |
| system data | `--print-identity` `84211a2ec9766ec4fc9859c6b24492a34edc0afd840172f2b079986cb9f71433`; the teardown tool's model identity is `eab71f4c…d593` |
| profiles | the teardown provisioner's `phrases` (`c70bf19e…b8f1`) and `journals` (`ef4617b3…e149`), as in `docs/packaging.md` "Regeneration" |
| host | Intel Core i7-9750H, 12 CPUs, governor `powersave` on all CPUs, Ubuntu 26.10, kernel 7.3.0-9-generic; measured CPU 2 |
| filesystems | every library, the data, the profiles, the work directory and `TMPDIR`: `findmnt -T` gives `/ /dev/nvme0n1p2 ext4` |
| Valgrind / cc / Python | 3.27.1 / GCC 15.3.0 / CPython 3.15.0 |

The stripped size is the size of `strip -o` on the installed object; the
measured objects are the installed ones. Stripping removes only sections
that are not loaded, so the mappings are the same.

Subject build (`CARGO_TARGET_DIR` on the home disk, cargo-c on `PATH`):

```sh
just heavy -- tools/packaging/install.sh libpinyin --prefix="$STAGE" \
  -- --locked --release --no-default-features --features bdb,shipped
just heavy -- tools/packaging/install.sh libzhuyin --prefix="$STAGE" \
  -- --locked --release --no-default-features --features bdb
```

The control is the teardown tool's same-bytes copy of each subject library
at a second path (`<work>/libraries/control/`). The tool copies the
subject, the control and the pin into `<work>/libraries/` on ext4.

**Wait for other lanes.** Before the run, the store-ops campaign in
`/tmp/kc-b` held heavy slot 1. The campaign script polled the heavy-slot
records and the process list every 60 s and started only when both were
clear: it waited from 10:25:36Z to 10:43:37Z (18 min). The run took
10:43:37Z–10:46:19Z. Load average was 1.19 at the start and 2.80 at the
end. Desktop sessions were running throughout.

## Tools and commands

| figure | tool | command |
| --- | --- | --- |
| memory at M0/M1, warm timing | #993 teardown worker and controller | `just heavy -- python tools/bisection/contract-diff.py bdb "$PIN" "$STAGE/lib/libpinyin.so.15.0.0" --zhuyin-so "$STAGE/lib/libzhuyin.so.15.0.0" --teardown --teardown-work-dir "$WORK" --teardown-cpu 2 --teardown-cases phrases,journals --teardown-stage confirm` |
| Massif heap | #993 teardown, `attribute` stage | the same command with `--teardown-stage attribute --teardown-attribution-case phrases` (then `journals`) |
| profiles | #993 teardown, `provision` stage | the same command with `--teardown-stage provision` |
| init Ir | #842 initbench at a204f9c0 | `initbench instrument --lib <work>/libraries/<side>/lib<facade>.so --facade <facade> --system-dir "$DATA" --profile <profile> --tool callgrind --cpu 2`, for side ∈ {ox, control, pin} |
| data identity | `tools/generate-function-order.py --print-identity "$DATA"` | |

The `confirm` stage first verified that subject, control and pin give the
same observations: all four cells were COMPARABLE. Then, in each
facade × profile cell, it ran 60 timed rounds after 3 warm-ups and 30
memory rounds after 2 warm-ups, rotating the order of ox, control and pin
each round, with one process per sample.

The `attribute` stage ran its Callgrind (teardown-scoped) and Massif parts
for both facades. It then stopped with `heap repetition needs an
equivalence-verified case`, because its Memcheck repetition also needs
the `empty` case, which this run did not include. Only the Massif figures
are used here.

What the teardown tool does not provide, and what was used instead:

- **Init-scoped Ir.** The tool's Callgrind toggles only the free and fini
  units. The #842 initbench `instrument` subcommand runs a whole-process
  Callgrind for each stop point. The init figure here is the
  `init` − `lib_load` delta. initbench needs its own manifest format
  (`file\t` before each line), so it read a copy of the same teardown
  profiles with that prefix added. The `user/` trees are byte-identical
  (`sha256sum` over both trees).
- **A RELRO split.** The tool groups each library's mappings by
  permission (`r-x`, `r--`, `rw-`). After relocation, RELRO is `r--`, so
  it falls in the read-only group with `.rodata`. initbench at a204f9c0
  does not split it either. Private_Dirty in a library's `r--` group is
  its RELRO copy-on-write pages, because `.rodata` is never written. RELRO
  pages that were never written are not separable from `.rodata` here.
- **Cold cache.** See [Cold cache](#cold-cache).

M1 is the teardown worker's checkpoint after `*_init`, `*_set_options`,
`*_alloc_instance` and the first candidate (`tools/bisection/teardown.h`).
It is not the bare init stop of #982's first phase. M0 is after `dlopen`
and symbol resolution.

## Memory: totals

`memory` records of the `confirm` stage, KiB. The intervals are the
tool's p25–p75 over 30 samples. The control − ox column is the paired
difference: its median and [min, max] over the 30 pairs. The §7.5
residency bound is ±max(64 KiB, 1% of |median|) of the subject. "inside"
means the whole [min, max] range lies within the bound. The rule is not
enforced in code.

| cell | point | metric | ox median [p25, p75] | control median | pin median [p25, p75] | control − ox paired median [min, max] | bound | inside | ox − pin |
| --- | --- | --- | ---: | ---: | ---: | --- | ---: | --- | ---: |
| pinyin `phrases` | M0 | Private_Dirty | 828 [828, 828] | 828 | 1,036 [1,036, 1,036] | +0 [-4, +4] | 64 | yes | -208 |
| pinyin `phrases` | M0 | Pss | 1,339 [1,336, 1,339] | 1,338 | 2,179 [2,037, 2,184] | +0 [-15, +14] | 64 | yes | -840 |
| pinyin `phrases` | M0 | Rss | 4,632 [4,632, 4,642] | 4,634 | 6,444 [6,444, 6,472] | +0 [-64, +44] | 64 | yes | -1,812 |
| pinyin `phrases` | M1 | Private_Dirty | 1,256 [1,256, 1,259] | 1,256 | 1,396 [1,396, 1,396] | +0 [-4, +4] | 64 | yes | -140 |
| pinyin `phrases` | M1 | Pss | 8,336 [8,335, 8,337] | 8,335 | 8,470 [8,315, 8,470] | +0 [-15, +14] | 83 | yes | -134 |
| pinyin `phrases` | M1 | Rss | 11,844 [11,844, 11,854] | 11,848 | 12,910 [12,908, 12,924] | +0 [-64, +44] | 118 | yes | -1,066 |
| pinyin `journals` | M0 | Private_Dirty | 828 [828, 828] | 828 | 1,036 [1,036, 1,036] | +0 [-4, +4] | 64 | yes | -208 |
| pinyin `journals` | M0 | Pss | 1,336 [1,334, 1,338] | 1,336 | 2,034 [2,018, 2,178] | +0 [-11, +12] | 64 | yes | -698 |
| pinyin `journals` | M0 | Rss | 4,632 [4,628, 4,636] | 4,632 | 6,450 [6,444, 6,476] | +4 [-88, +56] | 64 | NO | -1,818 |
| pinyin `journals` | M1 | Private_Dirty | 1,300 [1,300, 1,300] | 1,300 | 1,448 [1,448, 1,448] | +0 [-4, +4] | 64 | yes | -148 |
| pinyin `journals` | M1 | Pss | 8,377 [8,375, 8,379] | 8,378 | 8,329 [8,320, 8,481] | +0 [-11, +12] | 84 | yes | +48 |
| pinyin `journals` | M1 | Rss | 11,954 [11,908, 11,956] | 11,956 | 12,932 [12,920, 12,939] | +8 [-152, +124] | 120 | NO | -978 |
| zhuyin `phrases` | M0 | Private_Dirty | 764 [764, 764] | 764 | 984 [984, 984] | +0 [-8, +8] | 64 | yes | -220 |
| zhuyin `phrases` | M0 | Pss | 1,322 [1,319, 1,326] | 1,322 | 2,033 [1,984, 2,130] | +0 [-8, +12] | 64 | yes | -712 |
| zhuyin `phrases` | M0 | Rss | 4,616 [4,580, 4,620] | 4,616 | 6,422 [6,396, 6,476] | +2 [-108, +116] | 64 | NO | -1,806 |
| zhuyin `phrases` | M1 | Private_Dirty | 1,160 [1,160, 1,164] | 1,160 | 1,316 [1,316, 1,316] | +0 [-4, +4] | 64 | yes | -156 |
| zhuyin `phrases` | M1 | Pss | 8,026 [8,022, 8,027] | 8,026 | 8,007 [8,003, 8,165] | +0 [-6, +8] | 80 | yes | +19 |
| zhuyin `phrases` | M1 | Rss | 11,534 [11,497, 11,536] | 11,536 | 12,624 [12,620, 12,656] | +4 [-104, +116] | 115 | NO | -1,090 |
| zhuyin `journals` | M0 | Private_Dirty | 764 [764, 764] | 764 | 984 [984, 984] | +0 [-4, +4] | 64 | yes | -220 |
| zhuyin `journals` | M0 | Pss | 1,321 [1,320, 1,324] | 1,321 | 2,002 [1,930, 2,033] | +0 [-9, +13] | 64 | yes | -680 |
| zhuyin `journals` | M0 | Rss | 4,618 [4,585, 4,620] | 4,620 | 6,420 [6,396, 6,476] | +0 [-80, +128] | 64 | NO | -1,802 |
| zhuyin `journals` | M1 | Private_Dirty | 1,204 [1,204, 1,208] | 1,204 | 1,368 [1,368, 1,368] | +0 [-4, +4] | 64 | yes | -164 |
| zhuyin `journals` | M1 | Pss | 8,066 [8,066, 8,070] | 8,066 | 8,015 [7,968, 8,019] | +0 [-9, +13] | 81 | yes | +51 |
| zhuyin `journals` | M1 | Rss | 11,640 [11,577, 11,648] | 11,640 | 12,664 [12,633, 12,672] | +0 [-148, +200] | 116 | NO | -1,024 |

Pss and Private_Dirty are inside the bound in every cell and at both
points. Rss is not inside it in six of the eight Rss rows: the paired
range reaches −152 to +200 KiB, while the paired medians are 0 to +8 KiB.
The likely cause, not traced, is shared clean pages of the system
libraries that other processes on the host fault in and evict. Read the
Rss rows with that in mind. The per-mapping comparison below uses Pss.
The pin's own Pss spread (p25–p75 up to 155 KiB at M1) comes from its
libstdc++ pages, whose Pss share depends on the other processes that map
libstdc++.

## Memory: per mapping at M0 and M1

`memory-deltas` records of the same `confirm` run: the medians over 30
samples of each mapping group, summed into the classes below, in KiB.
"other shared libraries" is libc, libm, libgcc_s, libatomic, glib, pcre2
and the dynamic loader. A data file's row adds up all mappings of that
file. For the subject that means the read-only view and the private
replay view (`rw-p`). For the pin it is its single `rw-p` mapping
(`memory_chunk.h:479–523` at 074a2219). The last column is the largest
control − ox difference of the group medians over the three metrics.

The group summaries are per side, not paired, so no paired interval
exists per group. The p25–p75 width of each group is 0–8 KiB for the
subject and the control. For the pin it is 0–12 KiB, except libstdc++
(up to 103 KiB) and libdb `r--` (up to 58 KiB). The pin's libstdc++ Pss
also changes between cells (569 or 413 KiB), because its shared pages
are divided among the other processes on the host that map libstdc++.

### pinyin `phrases`, M0

| mapping | Rss ox / pin | Pss ox / pin | Private_Dirty ox / pin | Pss ox − pin | control − ox (max abs, any metric) |
| --- | ---: | ---: | ---: | ---: | ---: |
| anonymous | 604 / 704 | 604 / 704 | 604 / 704 | -100 | 0 |
| facade lib text (r-x) | 192 / 220 | 192 / 220 | 0 / 0 | -28 | 0 |
| facade lib read-only (r--, incl. RELRO) | 116 / 252 | 116 / 252 | 52 / 88 | -136 | 0 |
| facade lib writable (rw-) | 4 / 4 | 4 / 4 | 4 / 4 | +0 | 0 |
| libdb-5.3 (all) | 268 / 286 | 247 / 265 | 32 / 32 | -18 | 0 |
| libstdc++ (all) | 0 / 1,572 | 0 / 569 | 0 / 72 | -569 | 0 |
| other shared libraries | 3,376 / 3,372 | 115 / 112 | 76 / 76 | +2 | 10 |
| other | 72 / 72 | 60 / 60 | 60 / 60 | +0 | 0 |
| sum of group medians | 4,632 / 6,482 | 1,338 / 2,186 | 828 / 1,036 | -848 | 10 |

### pinyin `journals`, M0

| mapping | Rss ox / pin | Pss ox / pin | Private_Dirty ox / pin | Pss ox − pin | control − ox (max abs, any metric) |
| --- | ---: | ---: | ---: | ---: | ---: |
| anonymous | 604 / 704 | 604 / 704 | 604 / 704 | -100 | 0 |
| facade lib text (r-x) | 192 / 220 | 192 / 220 | 0 / 0 | -28 | 0 |
| facade lib read-only (r--, incl. RELRO) | 116 / 252 | 116 / 252 | 52 / 88 | -136 | 0 |
| facade lib writable (rw-) | 4 / 4 | 4 / 4 | 4 / 4 | +0 | 0 |
| libdb-5.3 (all) | 268 / 292 | 247 / 271 | 32 / 32 | -24 | 0 |
| libstdc++ (all) | 0 / 1,572 | 0 / 413 | 0 / 72 | -413 | 0 |
| other shared libraries | 3,376 / 3,364 | 112 / 111 | 76 / 76 | +1 | 0 |
| other | 72 / 72 | 60 / 60 | 60 / 60 | +0 | 0 |
| sum of group medians | 4,632 / 6,480 | 1,335 / 2,035 | 828 / 1,036 | -700 | 0 |

### zhuyin `phrases`, M0

| mapping | Rss ox / pin | Pss ox / pin | Private_Dirty ox / pin | Pss ox − pin | control − ox (max abs, any metric) |
| --- | ---: | ---: | ---: | ---: | ---: |
| anonymous | 604 / 704 | 604 / 704 | 604 / 704 | -100 | 0 |
| facade lib text (r-x) | 200 / 220 | 200 / 220 | 0 / 0 | -20 | 0 |
| facade lib read-only (r--, incl. RELRO) | 96 / 252 | 96 / 252 | 40 / 88 | -156 | 0 |
| facade lib writable (rw-) | 4 / 4 | 4 / 4 | 4 / 4 | +0 | 0 |
| libdb-5.3 (all) | 268 / 292 | 247 / 271 | 32 / 32 | -24 | 0 |
| libstdc++ (all) | 0 / 1,572 | 0 / 413 | 0 / 72 | -413 | 0 |
| other shared libraries | 3,376 / 3,364 | 110 / 111 | 76 / 76 | -1 | 0 |
| other | 72 / 72 | 60 / 60 | 8 / 8 | +0 | 0 |
| sum of group medians | 4,620 / 6,480 | 1,321 / 2,035 | 764 / 984 | -714 | 0 |

### zhuyin `journals`, M0

| mapping | Rss ox / pin | Pss ox / pin | Private_Dirty ox / pin | Pss ox − pin | control − ox (max abs, any metric) |
| --- | ---: | ---: | ---: | ---: | ---: |
| anonymous | 604 / 704 | 604 / 704 | 604 / 704 | -100 | 0 |
| facade lib text (r-x) | 200 / 188 | 200 / 188 | 0 / 0 | +12 | 0 |
| facade lib read-only (r--, incl. RELRO) | 96 / 252 | 96 / 252 | 40 / 88 | -156 | 0 |
| facade lib writable (rw-) | 4 / 4 | 4 / 4 | 4 / 4 | +0 | 0 |
| libdb-5.3 (all) | 268 / 292 | 247 / 271 | 32 / 32 | -24 | 0 |
| libstdc++ (all) | 0 / 1,572 | 0 / 413 | 0 / 72 | -413 | 0 |
| other shared libraries | 3,376 / 3,400 | 110 / 109 | 76 / 76 | +1 | 0 |
| other | 72 / 72 | 60 / 60 | 8 / 8 | +0 | 0 |
| sum of group medians | 4,620 / 6,484 | 1,321 / 2,001 | 764 / 984 | -680 | 0 |

### pinyin `phrases`, M1

| mapping | Rss ox / pin | Pss ox / pin | Private_Dirty ox / pin | Pss ox − pin | control − ox (max abs, any metric) |
| --- | ---: | ---: | ---: | ---: | ---: |
| anonymous | 860 / 892 | 860 / 892 | 860 / 892 | -32 | 0 |
| facade lib text (r-x) | 1,024 / 452 | 1,024 / 452 | 0 / 0 | +572 | 0 |
| facade lib read-only (r--, incl. RELRO) | 316 / 352 | 316 / 352 | 52 / 88 | -36 | 0 |
| facade lib writable (rw-) | 4 / 4 | 4 / 4 | 4 / 4 | +0 | 0 |
| libdb-5.3 (all) | 1,516 / 1,632 | 1,495 / 1,611 | 32 / 32 | -116 | 0 |
| libstdc++ (all) | 0 / 1,572 | 0 / 569 | 0 / 72 | -569 | 0 |
| other shared libraries | 3,440 / 3,552 | 116 / 120 | 76 / 76 | -4 | 10 |
| data addon_phrase_index.bin | 64 / 64 | 64 / 64 | 0 / 0 | +0 | 0 |
| data addon_pinyin_index.bin | 64 / 192 | 64 / 192 | 0 / 0 | -128 | 0 |
| data gb_char.bin | 3,224 / 2,904 | 3,076 / 2,904 | 172 / 172 | +172 | 0 |
| data gbk_char.bin | 340 / 340 | 340 / 340 | 0 / 0 | +0 | 0 |
| data merged.bin | 32 / 32 | 32 / 32 | 0 / 0 | +0 | 0 |
| data opengram.bin | 804 / 804 | 804 / 804 | 0 / 0 | +0 | 0 |
| data phrase_index.bin | 64 / 64 | 64 / 64 | 0 / 0 | +0 | 0 |
| data punct.bin | 16 / 16 | 16 / 16 | 0 / 0 | +0 | 0 |
| other | 76 / 72 | 60 / 60 | 60 / 60 | +0 | 0 |
| sum of group medians | 11,844 / 12,944 | 8,335 / 8,476 | 1,256 / 1,396 | -140 | 10 |

### pinyin `journals`, M1

| mapping | Rss ox / pin | Pss ox / pin | Private_Dirty ox / pin | Pss ox − pin | control − ox (max abs, any metric) |
| --- | ---: | ---: | ---: | ---: | ---: |
| anonymous | 864 / 904 | 864 / 904 | 864 / 904 | -40 | 0 |
| facade lib text (r-x) | 1,024 / 452 | 1,024 / 452 | 0 / 0 | +572 | 0 |
| facade lib read-only (r--, incl. RELRO) | 316 / 352 | 316 / 352 | 52 / 88 | -36 | 0 |
| facade lib writable (rw-) | 4 / 4 | 4 / 4 | 4 / 4 | +0 | 0 |
| libdb-5.3 (all) | 1,516 / 1,636 | 1,495 / 1,615 | 32 / 32 | -120 | 0 |
| libstdc++ (all) | 0 / 1,572 | 0 / 413 | 0 / 72 | -413 | 0 |
| other shared libraries | 3,440 / 3,544 | 113 / 117 | 76 / 76 | -4 | 0 |
| data addon_phrase_index.bin | 64 / 64 | 64 / 64 | 0 / 0 | +0 | 0 |
| data addon_pinyin_index.bin | 64 / 192 | 64 / 192 | 0 / 0 | -128 | 0 |
| data gb_char.bin | 3,032 / 2,904 | 2,960 / 2,904 | 56 / 56 | +56 | 0 |
| data gbk_char.bin | 472 / 340 | 400 / 340 | 60 / 60 | +60 | 8 |
| data merged.bin | 64 / 32 | 64 / 32 | 32 / 32 | +32 | 0 |
| data opengram.bin | 936 / 804 | 868 / 804 | 64 / 64 | +64 | 4 |
| data phrase_index.bin | 64 / 64 | 64 / 64 | 0 / 0 | +0 | 0 |
| data punct.bin | 16 / 16 | 16 / 16 | 0 / 0 | +0 | 0 |
| other | 76 / 72 | 60 / 60 | 60 / 60 | +0 | 0 |
| sum of group medians | 11,952 / 12,952 | 8,376 / 8,333 | 1,300 / 1,448 | +43 | 4 |

### zhuyin `phrases`, M1

| mapping | Rss ox / pin | Pss ox / pin | Private_Dirty ox / pin | Pss ox − pin | control − ox (max abs, any metric) |
| --- | ---: | ---: | ---: | ---: | ---: |
| anonymous | 828 / 864 | 828 / 864 | 828 / 864 | -36 | 0 |
| facade lib text (r-x) | 968 / 436 | 968 / 436 | 0 / 0 | +532 | 0 |
| facade lib read-only (r--, incl. RELRO) | 240 / 356 | 240 / 356 | 40 / 88 | -116 | 0 |
| facade lib writable (rw-) | 4 / 4 | 4 / 4 | 4 / 4 | +0 | 0 |
| libdb-5.3 (all) | 1,516 / 1,636 | 1,495 / 1,615 | 32 / 32 | -120 | 0 |
| libstdc++ (all) | 0 / 1,572 | 0 / 413 | 0 / 72 | -413 | 0 |
| other shared libraries | 3,440 / 3,544 | 111 / 117 | 76 / 76 | -6 | 0 |
| data gb_char.bin | 3,224 / 2,904 | 3,076 / 2,904 | 172 / 172 | +172 | 0 |
| data gbk_char.bin | 340 / 340 | 340 / 340 | 0 / 0 | +0 | 0 |
| data merged.bin | 32 / 32 | 32 / 32 | 0 / 0 | +0 | 0 |
| data opengram.bin | 804 / 804 | 804 / 804 | 0 / 0 | +0 | 0 |
| data phrase_index.bin | 64 / 64 | 64 / 64 | 0 / 0 | +0 | 0 |
| other | 76 / 72 | 60 / 60 | 8 / 8 | +0 | 0 |
| sum of group medians | 11,536 / 12,628 | 8,022 / 8,009 | 1,160 / 1,316 | +13 | 0 |

### zhuyin `journals`, M1

| mapping | Rss ox / pin | Pss ox / pin | Private_Dirty ox / pin | Pss ox − pin | control − ox (max abs, any metric) |
| --- | ---: | ---: | ---: | ---: | ---: |
| anonymous | 832 / 876 | 832 / 876 | 832 / 876 | -44 | 0 |
| facade lib text (r-x) | 968 / 436 | 968 / 436 | 0 / 0 | +532 | 0 |
| facade lib read-only (r--, incl. RELRO) | 240 / 356 | 240 / 356 | 40 / 88 | -116 | 0 |
| facade lib writable (rw-) | 4 / 4 | 4 / 4 | 4 / 4 | +0 | 0 |
| libdb-5.3 (all) | 1,516 / 1,636 | 1,495 / 1,615 | 32 / 32 | -120 | 0 |
| libstdc++ (all) | 0 / 1,572 | 0 / 413 | 0 / 72 | -413 | 0 |
| other shared libraries | 3,440 / 3,580 | 111 / 115 | 76 / 76 | -4 | 0 |
| data gb_char.bin | 3,032 / 2,904 | 2,960 / 2,904 | 56 / 56 | +56 | 0 |
| data gbk_char.bin | 470 / 340 | 400 / 340 | 60 / 60 | +60 | 2 |
| data merged.bin | 64 / 32 | 64 / 32 | 32 / 32 | +32 | 0 |
| data opengram.bin | 936 / 804 | 868 / 804 | 64 / 64 | +64 | 0 |
| data phrase_index.bin | 64 / 64 | 64 / 64 | 0 / 0 | +0 | 0 |
| other | 76 / 72 | 60 / 60 | 8 / 8 | +0 | 0 |
| sum of group medians | 11,642 / 12,676 | 8,066 / 8,019 | 1,204 / 1,368 | +47 | 2 |

### Mappings above the pin by more than the bound

M1, Pss, ox − pin, with the bound ±max(64 KiB, 1% of |median|) of the subject.
Every mapping's median is below 6,400 KiB, so the bound is 64 KiB in each row:

| mapping | pinyin `phrases` | pinyin `journals` | zhuyin `phrases` | zhuyin `journals` | likely owner |
| --- | ---: | ---: | ---: | ---: | --- |
| facade library text (`r-x`) | +572 | +572 | +532 | +532 | #986 (closed): text residency |
| `gb_char.bin` (both views) | +172 | +56 | +172 | +56 | #985 (open): duplicate replay views |
| `gbk_char.bin`, `opengram.bin`, `merged.bin` (both views) | 0 | +60 / +64 / +32 | 0 | +60 / +64 / +32 | #985 |

In `journals`, each of the four files is inside the bound on its own
(+32 to +64). Together they are +212 KiB, the gross excess that #985
records. The #985 figures are reproduced exactly: +172 (`phrases`) and
56 / 60 / 64 / 32 (`journals`). The replay view's pages are counted as
Private_Dirty on both sides, at the same size (172, 56, 60, 64, 32 KiB).
The excess is the original file pages that the read-only view keeps
resident.

The facade text is the only contributor present at M1 and absent at M0.
At M0 the subject's text is at or below the pin's (192 / 200 against 220,
with one cell at 200 against 188). It is created by init and the first
candidate. Function ordering reduced it from +804 / +920 (#986, measured
before #1000 and #1002 at a different checkpoint) to +572 / +532. Before
#986 is reopened or a new issue is filed, the remaining text needs a
function-level attribution: init and the first candidate execute about
9.6–9.7 M Ir in the subject (below).

No new contributor is above the pin by more than the bound.

### Mappings below the pin

These offset the two contributors above. M1, Pss, ox − pin:

| mapping | pinyin | zhuyin | note |
| --- | ---: | ---: | --- |
| libstdc++ (all mappings) | −569 / −413 | −413 | the pin links libstdc++; oxpinyin does not. The size depends on how many other processes share it |
| `gb_char.bin` and the other replayed files, rearranged | — | — | counted above per file |
| libdb-5.3 (all mappings) | −116 / −120 | −120 | the same libdb; the subject touches fewer of its pages |
| `addon_pinyin_index.bin` | −128 | n/a | the pin holds 192 KiB of it, the subject 64 |
| facade library `r--` (incl. RELRO) | −36 | −116 | RELRO copy-on-write is 52 / 40 KiB against the pin's 88 (#987) |
| anonymous | −32 / −40 | −36 / −44 | |

At M0 the subject is 680–850 KiB of Pss below the pin. libstdc++
(−413 to −569), anonymous memory (−100) and the facade `r--` group
(−136 / −156) account for most of it.

## Ranked remaining contributors

By Pss above the pin at M1, largest first:

1. Facade library text residency: +572 KiB (libpinyin), +532 KiB
   (libzhuyin). Owner: #986, closed after function ordering. Whether to
   reopen it or file a new issue is the maintainer's decision.
2. Duplicate replay views: +172 KiB (`phrases`), +212 KiB (`journals`),
   in both facades. Owner: #985.

Nothing else is above the bound. The net Pss at M1 is −134 to +51 KiB
against the pin.

## Time

`time` records of the same `confirm` run, warm page cache. The control
rule (§7.5) requires the paired control/ox median and its whole
bootstrap 95% interval to lie within [0.95, 1.05]. A metric that fails
it is INCONCLUSIVE, and no ox/pin ratio is given for it. The ox/pin
ratios are paired medians with the tool's bootstrap 95% interval
(4,000 resamples). Medians are in µs, from 60 samples per side.

| cell | phase | clock | ox median | pin median | control/ox [95%] | verdict | ox/pin [95%] |
| --- | --- | --- | ---: | ---: | --- | --- | --- |
| pinyin `phrases` | load | wall | 681.2 | 1,556.2 | 1.011 [0.983, 1.039] | STABLE | 0.440 [0.419, 0.453] |
| pinyin `phrases` | load | cpu | 680.4 | 1,555.9 | 1.013 [0.979, 1.039] | STABLE | 0.441 [0.420, 0.453] |
| pinyin `phrases` | init | wall | 2,700.3 | 2,596.8 | 0.997 [0.962, 1.030] | STABLE | 1.039 [1.009, 1.059] |
| pinyin `phrases` | init | cpu | 2,700.9 | 2,593.8 | 0.997 [0.960, 1.027] | STABLE | 1.036 [1.009, 1.059] |
| pinyin `phrases` | first_candidate | wall | 171.7 | 92.0 | 1.002 [0.966, 1.034] | STABLE | 1.866 [1.819, 1.928] |
| pinyin `phrases` | first_candidate | cpu | 172.0 | 92.2 | 1.002 [0.966, 1.034] | STABLE | 1.866 [1.819, 1.929] |
| pinyin `journals` | load | wall | 696.1 | 1,565.4 | 1.004 [0.993, 1.019] | STABLE | 0.435 [0.427, 0.447] |
| pinyin `journals` | load | cpu | 695.8 | 1,561.2 | 1.004 [0.993, 1.016] | STABLE | 0.435 [0.427, 0.448] |
| pinyin `journals` | init | wall | 2,789.2 | 2,689.7 | 0.991 [0.972, 1.008] | STABLE | 1.033 [1.022, 1.061] |
| pinyin `journals` | init | cpu | 2,790.0 | 2,687.9 | 0.992 [0.972, 1.009] | STABLE | 1.033 [1.026, 1.060] |
| pinyin `journals` | first_candidate | wall | 164.6 | 94.2 | 0.982 [0.968, 1.018] | STABLE | 1.736 [1.697, 1.781] |
| pinyin `journals` | first_candidate | cpu | 165.0 | 94.3 | 0.983 [0.968, 1.019] | STABLE | 1.737 [1.698, 1.779] |
| zhuyin `phrases` | load | wall | 670.7 | 1,559.0 | 1.021 [0.976, 1.040] | STABLE | 0.429 [0.418, 0.443] |
| zhuyin `phrases` | load | cpu | 670.2 | 1,558.8 | 1.021 [0.976, 1.039] | STABLE | 0.429 [0.418, 0.443] |
| zhuyin `phrases` | init | wall | 2,638.0 | 2,443.5 | 0.977 [0.958, 0.998] | STABLE | 1.083 [1.053, 1.119] |
| zhuyin `phrases` | init | cpu | 2,635.2 | 2,444.0 | 0.979 [0.958, 0.997] | STABLE | 1.085 [1.055, 1.119] |
| zhuyin `phrases` | first_candidate | wall | 146.0 | 71.8 | 1.003 [0.977, 1.029] | STABLE | 2.051 [2.004, 2.099] |
| zhuyin `phrases` | first_candidate | cpu | 146.2 | 71.9 | 1.003 [0.977, 1.028] | STABLE | 2.049 [2.000, 2.098] |
| zhuyin `journals` | load | wall | 672.3 | 1,594.0 | 0.996 [0.987, 1.031] | STABLE | 0.427 [0.418, 0.435] |
| zhuyin `journals` | load | cpu | 675.0 | 1,593.8 | 0.996 [0.983, 1.032] | STABLE | 0.426 [0.418, 0.435] |
| zhuyin `journals` | init | wall | 2,631.7 | 2,514.3 | 1.008 [0.980, 1.037] | STABLE | 1.049 [1.015, 1.071] |
| zhuyin `journals` | init | cpu | 2,631.9 | 2,514.3 | 1.007 [0.980, 1.037] | STABLE | 1.048 [1.015, 1.071] |
| zhuyin `journals` | first_candidate | wall | 134.5 | 72.9 | 1.011 [0.984, 1.052] | INCONCLUSIVE | — |
| zhuyin `journals` | first_candidate | cpu | 134.8 | 73.0 | 1.011 [0.984, 1.052] | INCONCLUSIVE | — |

`load` is `dlopen` with `RTLD_NOW` plus resolving every export, `init` is
`*_init`, and `first_candidate` is parse, guess and fetch for `nihao`
(`tools/bisection/teardown.h`). This document gives no faster or slower
verdict.

### Cold cache

Not measured. Neither tool evicts pages before a sample:

- The #993 teardown tool has no cold mode.
- initbench at a204f9c0 offers only `--cache cold`, which writes
  `/proc/sys/vm/drop_caches` and needs root. Root is not available on
  this host (`sudo -n true` asks for a password).

Neither tool provides the requested per-file `posix_fadvise(DONTNEED)`
eviction of the measured library and data pages before each sample.
Adding it would be a code change, which is out of scope here.

## Deterministic figures

Init Ir is the initbench Callgrind `init` stop minus the `lib_load` stop.
Each stop is a separate whole-process run under
`valgrind --tool=callgrind --cache-sim=no --branch-sim=no`.

| cell | ox init Ir | control init Ir | pin init Ir | ox/pin | ox first-candidate Ir | pin first-candidate Ir |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| pinyin `phrases` | 9,628,630 | 9,628,642 | 10,252,041 | 0.939 | 632,805 | 311,649 |
| pinyin `journals` | 9,670,999 | 9,671,057 | 10,318,187 | 0.937 | 584,468 | 311,967 |
| zhuyin `phrases` | 9,548,672 | 9,549,010 | 10,090,812 | 0.946 | 586,846 | 282,618 |
| zhuyin `journals` | 9,590,803 | 9,591,157 | 10,156,478 | 0.944 | 534,023 | 281,366 |

Massif, from the teardown `attribute` stage
(`--time-unit=i --detailed-freq=1 --peak-inaccuracy=0`), in bytes.
`mem_heap_B` is shown at M0 and M1, and the peak is the largest snapshot
over the whole lifecycle. For the subject the peak falls before M1, so it
is an init or first-candidate peak.

| cell | ox M0 | ox M1 | ox peak | control peak | pin M0 | pin M1 | pin peak | ox/pin M1 | ox/pin peak |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| pinyin `phrases` | 32,394 | 258,951 | 623,408 | 623,423 | 113,454 | 318,974 | 669,284 | 0.812 | 0.931 |
| pinyin `journals` | 32,394 | 253,921 | 286,568 | 286,583 | 113,454 | 318,974 | 357,148 | 0.796 | 0.802 |
| zhuyin `phrases` | 32,394 | 224,925 | 623,408 | 623,423 | 113,454 | 281,998 | 669,196 | 0.798 | 0.932 |
| zhuyin `journals` | 32,394 | 219,895 | 286,568 | 286,583 | 113,454 | 281,998 | 357,060 | 0.780 | 0.803 |

**The control does not equal the subject exactly.** The control's Massif
heap is 15 B larger at every checkpoint. Its init Ir is 12–58 Ir larger
(pinyin) and 338–354 Ir larger (zhuyin), and its `lib_load` Ir is
84 / 484–504 Ir larger. The bytes are identical (same sha256). The only
input that differs is the library path:
`<work>/libraries/control/lib<facade>.so` is 5 characters longer than
`…/ox/…`. A path-length cause is consistent with 15 B = 3 × 5 B, but it
was not traced. The differences are below 0.004% of init Ir.
