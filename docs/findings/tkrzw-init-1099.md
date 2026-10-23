# tkrzw init attribution (#1099)

Date: 2026-10-23 UTC (`date -u` at run time). Cell: tkrzw on both sides.
Base `origin/main` 294f14eb; source `wip/1099-tkrzw-init` b764e197, which
changes only this document. No library code changed. Setup, builds and
every measurement ran in one sitting on one host, 12:17Z–12:44Z. The command for each
figure is given where the figure is used, and the scripts are in
[Scripts](#scripts). The raw Callgrind streams were parsed in memory. The
derived tables were written to `/dev/shm` in a disposable cloud container
and were not kept.

## Summary

- **One function costs about 97% of init on both sides.** A single call of
  `tkrzw::HashDBMImpl::ReadNextBucketRecords` costs **329,255,106 Ir**
  inclusive. This is 96.8–97.0% of init Ir. It is identical on ox, the
  same-bytes control and the pin, in all four facade/profile cells. It is
  the first iterator step over the user's `user_bigram.db`:
  - pin: `pinyin::Bigram::load_db` → `copy_tkrzwdb`
    (`src/storage/ngram_tkrzwdb.cpp:48-63`,
    `src/storage/tkrzwdb_utils.h:48-74` at 074a2219);
  - ox: `oxpinyin_user::persistence::load_user_bigram_db` →
    `TkrzwUserBigramDb::load_db` → `visit_db` → `tkrzw_dbm_iter_process`.
- **Why it costs so much.** A HashDBM iterator reads every bucket slot of
  the file in turn until it finds a record
  (`tkrzw_dbm_hash.cc:1836-1886` at tkrzw 9ee8416). The pin's
  `Bigram::save_db` writes `user_bigram.db` with HashDBM's default tuning
  (`ngram_tkrzwdb.cpp:65-84`), which is 1,048,583 buckets
  (`tkrzw_dbm_hash.h:47`) whatever the record count. Every provisioned
  profile has such a file with **0 records** (4,198,400 B). The walk
  reads all 1,048,583 bucket slots, at 314.0 Ir per bucket.
- **Scaling.** The cost is linear in the bucket count: 314.0 Ir per bucket
  on both sides, from 16,411 to 1,048,583 buckets. It does not depend on
  file bytes: offset width 5 makes the file 25% larger with the same bucket
  count, and Ir is identical. Records add a per-record term. That term is
  **6,030–6,061 Ir per record on ox and 3,466–3,500 on the pin**, from
  synthetic files with 1,000 to 100,000 records. With 10,000 records, ox
  init is 1.068× the pin's Ir; with 100,000 records it is 1.371×.
- **The open is the same on both sides.** Both open the file read-only,
  with HashDBM default tuning and no-create. No open flag or tuning
  parameter avoids the walk: an existing file's bucket count comes from its
  metadata, and `cache_buckets` does not apply to the memory-mapped file
  that both sides use. The files are healthy, so no recovery runs.
- **Reproduction.** Init Ir is within +0.34% to +0.36% of #1096 on both
  sides. ox equals the control exactly, so the control is VALID (§7.5 item
  3). ox/pin is 0.9991–0.9994.
- **Wall time (warm).** Init medians are 54.7–58.7 ms on both sides. Only
  zhuyin `journals` passes the §7.5 control rule: init wall ox/pin
  **0.996** (95% CI 0.953–1.044). The other seven init figures (wall and
  CPU) are INCONCLUSIVE. Diagnostic only: on the `phrases` profile without
  `user_bigram.db`, init medians are 2.9–3.3 ms.
- **This is an upstream finding, not a divergence.** The pin pays the same
  cost in the same tkrzw code. Two changes could remove it and keep
  compatibility. Both are described [below](#changes-that-would-remove-the-cost)
  for a ruling. Neither is implemented.

## Setup

| item | value |
| --- | --- |
| host | claude.ai cloud session; Intel Xeon @ 2.10 GHz, 4 CPUs, 15 GiB; kernel 6.18.44-fc-v153; Ubuntu 24.04.5; 4 KiB pages; no cpufreq governor exposed; measured CPU 2 |
| filesystems | the measured library copies, `libtkrzw.so.1.77.0`, the system data, the profiles and their variants, the work directories and `TMPDIR`: `findmnt -T` gives `/ /dev/vda ext4` for each |
| pin | libpinyin 074a2219, tkrzw cell from `tools/cloud/setup.sh --cells "tkrzw"`; libpinyin.so.15.0.0 `d78a91676af4…f7b8`, libzhuyin.so.15.0.0 `adbcc8c8c3de…3d` (both equal `oracle-pin.txt`) |
| libtkrzw | setup's build of upstream 9ee8416 (1.0.34), `libtkrzw.so.1.77.0` sha256 `ead8c80b3fc155827989e8c9bbcc90d9b02bba22b8848271a90f2248876f38d9`, built by setup with the host GCC 13.3.0. The host's packaged library failed `distro-probe.sh`. `ldd` resolves `libtkrzw.so.1` to this file for both pin facades and both installed subject facades. `tools/tkrzw/runtime.py` `check` (`ldd` on the exact object) passed for every measured copy before each run |
| subject libpinyin.so.15.0.0 | sha256 `776e48a2db31e4979147a65cffa0d5c507a082734a20f901044a703a8308a18b`; installed 5,200,584 B; stripped 2,487,008 B |
| subject libzhuyin.so.15.0.0 | sha256 `7a35c0a0ef430e9e85e1f037e7826e04c2ea0cfa57921dcaae40dd76e24b16bb`; installed 4,926,800 B; stripped 2,245,344 B |
| subject build | compatibility image `oxpinyin-compat:ddd2c1a8e182d4d3` (`tools/oracle/Dockerfile.compat`; GNU ld (GNU Binutils for Debian) 2.47, GCC 15.3.0), cargo-c 0.10.24, Rust 1.97.1 |
| function ordering | applied: `install.sh` printed `function ordering: using measured policy …/function-order/tkrzw/libpinyin.order` and `…/libzhuyin.order`; `tools/packaging/function_order.py` reports `ordered` (266 and 251 paths) for ox and control, `reference` for pin |
| system data | regenerated by setup; manifest OK; `generate-function-order.py --print-identity` `8ce410ee5ba0628733bee849700ba4993e74271a74150bc1fd22ed400047e9c1` |
| profiles | teardown provisioner on the tkrzw cell; `profile.manifest` sha256 `phrases` `451e2dfd…52b8`, `journals` `cd28481d…10fd` |
| worker | `tools/bisection/bisect.c` built with host GCC 13.3.0 (`cc -std=gnu11 -Wall -Wextra -Werror -O2 -ldl`); the timing script's teardown build adds `-g` |
| Valgrind / Python | 3.22.0 / CPython 3.15.0 |

**Subject build.** The script started from a `git archive` of b764e197
in `$OUT/src`, on the host ext4 disk. It mounted setup's verified libtkrzw
read-only at the same path and selected it with `PKG_CONFIG_PATH`. This is
the tkrzw dependency selection in `docs/packaging.md` "Regeneration". The
first attempt used the image's Debian `libtkrzw-dev` 1.0.32 instead. Its
link failed, because that `tkrzw.pc` lists `-llz4 -lzstd` and the image
has no `liblz4-dev` or `libzstd-dev`.

```sh
TK=/root/.cache/oxpinyin/tkrzw
python -m tools.dev slot run -- docker run --rm --platform linux/amd64 --network host \
  -e HTTPS_PROXY -e CARGO_HTTP_CAINFO=/ca.crt -e SSL_CERT_FILE=/ca.crt \
  -e CARGO_TARGET_DIR=/w/target -e CARGO_INSTALL_ROOT=/w/cargo -e PKG_CONFIG_PATH=$TK/lib/pkgconfig \
  -v "$CA:/ca.crt:ro" -v "$TK:$TK:ro" -v "$OUT:/w" -w /w/src oxpinyin-compat:ddd2c1a8e182d4d3 bash -euc '
    export PATH=/w/cargo/bin:$PATH
    # cargo-c was installed by an earlier run of the same container:
    # CARGO_TARGET_DIR=/w/cargo-build cargo install cargo-c --version 0.10.24 --locked --features vendored-openssl
    tools/packaging/install.sh libpinyin --prefix=/w/stage -- --locked --release --no-default-features --features tkrzw,shipped
    tools/packaging/install.sh libzhuyin --prefix=/w/stage -- --locked --release --no-default-features --features tkrzw'
```

The image was built with `compat.sh`'s recipe: `docker build` of
`tools/oracle/Dockerfile.compat` with the pinned deps and reference
images. The proxy CA was passed as the `cacert` secret, and the tag key
came from `compat.sh`.

**Profiles.** These came from the teardown provisioner, which drives only
the pin:

```sh
source ~/.oxpinyin-oracle.env
python -m tools.dev slot run -- python -B tools/bisection/contract-diff.py tkrzw "$OXPINYIN_ORACLE_PREFIX_TKRZW" \
  "$STAGE/lib/libpinyin.so.15.0.0" --zhuyin-so "$STAGE/lib/libzhuyin.so.15.0.0" \
  --teardown --teardown-stage provision --teardown-work-dir "$W/teardown" --teardown-cpu 2
```

The measurements use the provisioner's own copies:
`$W/teardown/libraries/{subject,control,pinlibs}/lib{pinyin,zhuyin}.so`.
The three directory names have equal lengths.

## Init instruction counts

Init is the `<facade>_init` toggle plus the load constructors. This is the
definition #1096 used (`tools/measure-function-order.py`). Command:
[`attrib.py`](#attribpy), with sides ox, control and pin, and cells
`pinyin:phrases pinyin:journals zhuyin:phrases zhuyin:journals`.

| cell | ox | control | pin | ox/pin | ox vs #1096 PR | pin vs #1096 pin |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| pinyin `phrases` | 339,895,079 | 339,895,079 | 340,085,618 | 0.999440 | +0.355% | +0.350% |
| pinyin `journals` | 339,892,540 | 339,892,540 | 340,146,557 | 0.999253 | +0.358% | +0.348% |
| zhuyin `phrases` | 339,456,555 | 339,456,555 | 339,698,939 | 0.999286 | +0.345% | +0.338% |
| zhuyin `journals` | 339,452,055 | 339,452,055 | 339,774,605 | 0.999051 | +0.345% | +0.340% |

- Control equals ox exactly in every cell, so the control is VALID.
- The constructors contribute 1 Ir on ox and 19,011 Ir on the pin.
- The two profiles differ by less than 0.03%, so no further profiles
  were added.
- The comparison with #1096 is only this task's within-1% reproduction
  check, not a baseline. The libtkrzw build, compiler, Valgrind version
  and CPU all differ from that run.

## Attribution by function

pinyin `phrases`. The other three cells have the same rows: the tkrzw
figures are identical and the facade figures differ by under 1%. Ir is
exclusive (self) or inclusive (self plus callees). Inclusive cost is
exclusive cost plus the sum of the function's outgoing call-edge costs,
from `contract-diff.py::per_call_profile`.

**Inclusive chain to the cost** (ox / pin):

| function | ox Ir | pin Ir | calls (ox / pin) |
| --- | ---: | ---: | --- |
| `pinyin_init` | 339,895,078 | 340,066,607 | 1 / 1 |
| ox `persistence::load_user_bigram_db`; pin `pinyin::Bigram::load_db` | 329,312,985 | 329,268,636 | 1 / 1 |
| ox `tkrzw::drive_scan`; pin `copy_tkrzwdb` | 329,264,067 | 329,263,081 | 3 / 3 |
| `tkrzw::HashDBMIteratorImpl::ReadKeys` | 329,255,173 | 329,255,173 | 1 / 1 |
| `tkrzw::HashDBMImpl::ReadNextBucketRecords` | **329,255,106** | **329,255,106** | 1 / 1 |
| `tkrzw::HashDBMImpl::GetBucketValue` | 257,951,910 | 257,951,910 | 1,048,588 / 1,048,590 |
| `tkrzw::MemoryMapParallelFile::Read` | 166,726,821 | 166,726,821 | 1,048,602 / 1,048,606 |

**Exclusive cost** (the largest rows):

| function (object) | ox Ir | pin Ir | calls (ox / pin) |
| --- | ---: | ---: | --- |
| `tkrzw::HashDBMImpl::ReadNextBucketRecords` (libtkrzw) | 62,915,024 | 62,915,024 | 1 / 1 |
| `tkrzw::HashDBMImpl::GetBucketValue` (libtkrzw) | 59,769,345 | 59,769,345 | 1,048,588 / 1,048,590 |
| `tkrzw::MemoryMapParallelFile::Zone::Zone` (libtkrzw) | 57,672,780 | 57,672,780 | 1,048,605 / 1,048,611 |
| `tkrzw::MemoryMapParallelFile::Read` (libtkrzw) | 56,624,184 | 56,624,184 | 1,048,602 / 1,048,606 |
| `__memcpy_chk_avx_unaligned_erms` (libc) | 23,071,668 | 23,072,524 | 1,049,699 / 1,050,658 |
| `__memcpy_avx_unaligned_erms` (libc) | 15,907,778 | 16,068,035 | 1,061,740 / 1,080,838 |
| `free` (libc) | 12,629,959 | 12,618,092 | 3,150,777 / 3,151,621 |
| `Zone::~Zone`, `Zone::Size`, `Zone::Pointer` (libtkrzw) | 17,826,132 | 17,826,132 | about 1, 2 and 1 per bucket |
| unnamed entries (`???`) | 23,101,466 | 23,090,939 | each a multiple of the bucket count |
| largest facade row | 1,814,457 `persistence::load_logs_with_mask` | 5,214,843 `pinyin::MemoryChunk::mmap` | 1 / 4 |

Callgrind attributes the unnamed entries to no object. Their call counts
are one, two or three per bucket. They were not identified further.

**Exclusive cost by object:**

| object | ox Ir | pin Ir |
| --- | ---: | ---: |
| libtkrzw.so.1.77.0 | 255,108,673 | 254,950,717 |
| libc.so.6 | 52,756,573 | 52,884,568 |
| unnamed (`???`) | 23,101,466 | 23,090,939 |
| facade | 8,873,677 | 9,082,941 |
| libstdc++.so.6.0.33 | 53,239 | 40,918 |
| libglib-2.0.so.0.8000.0 | 0 | 16,500 |
| ld-linux-x86-64.so.2 | 1,450 | 24 |

**Outside the walk.** Init minus the walk is 10,639,972 (ox) / 10,811,501
(pin) Ir for pinyin `phrases`, and 10,196,948–10,872,440 across the four
cells. The other tkrzw opens are small. Seven `TreeDBM::OpenAdvanced`
calls cost 779,668 / 787,688 Ir; of that, `TreeDBMImpl::InitializePageCache`
is 649,740 / 655,513. Nine `HashDBM::OpenAdvanced` calls cost 92,272 /
91,871 Ir. These nine include the seven tree files' own hash files.

## What libtkrzw does at init, for which files

At 074a2219, init attaches five ordered tables and the system bigram
read-only. It also loads three user files (two ordered tables and one hash
file) into memory. Each load opens its file read-only. oxpinyin opens the
same files with the same classes and flags (`crates/oxpinyin-store/src/tkrzw/mod.rs`:
`open`, `open_hash`, `open_in_memory`, `TkrzwUserBigramDb::{empty, load_db}`).
`strace -f -e trace=openat` of one worker run (the whole run, not only
init) shows both sides opening these nine files `O_RDONLY`.

| file | pin call at 074a2219 | tkrzw class and open | ox | walked at init |
| --- | --- | --- | --- | --- |
| `pinyin_index.bin`, `addon_pinyin_index.bin` | `ChewingLargeTable2::attach`, `ATTACH_READONLY` (`chewing_large_table2_tkrzwdb.cpp:75-89`, `facade_chewing_table2.h:75-90`) | `TreeDBM::Open(path, false, OPEN_NO_CREATE)` (`tkrzwdb_utils.h:32-46`) | `open`: `dbm=tree,no_create=true`, read-only | no |
| `phrase_index.bin`, `addon_phrase_index.bin` | `PhraseLargeTable3::attach` (`phrase_large_table3_tkrzwdb.cpp:65-80`) | the same | the same | no |
| `punct.bin` | `PunctTable::attach` (`punct_table_tkrzwdb.cpp:58-72`) | the same | the same | no |
| `bigram.db` | `Bigram::attach`, `ATTACH_READONLY` (`ngram_tkrzwdb.cpp:86-99`; `pinyin.cpp:396`, `zhuyin.cpp:338`) | `HashDBM::Open(path, false, OPEN_NO_CREATE)` | `open_hash`: `dbm=hash,no_create=true` | no |
| `user_pinyin_index.bin`, `user_phrase_index.bin` | `load_db`: a read-only `TreeDBM` copied into a `BabyDBM` (`chewing_large_table2_tkrzwdb.cpp:92-109`, `phrase_large_table3_tkrzwdb.cpp:83-100`) | TreeDBM iterator: walks leaf nodes, cost proportional to the records (0 in every profile) | `open` read-only, then a range walk | yes, cheap |
| `user_bigram.db` | `load_db`: a read-only `HashDBM` with default tuning, copied into a `TinyDBM` (`ngram_tkrzwdb.cpp:48-63`; `pinyin.cpp:401`, `zhuyin.cpp:343`) | HashDBM iterator: **every bucket** | `open_hash`, then `visit_db` into `dbm=tiny` | **yes: the cost** |

**The file.** `tkrzw_dbm_util inspect --dbm hash` gives the same result
for the `user_bigram.db` of every provisioned profile (`modest`,
`phrases`, `journals`):

| property | value |
| --- | --- |
| `num_buckets` | 1,048,583 |
| `num_records` | 0 |
| `file_size` | 4,198,400 |
| `offset_width` / `align_pow` | 4 / 3 |
| `update_mode` | in-place |
| `healthy` | true |

The pin's `pinyin_save` writes this file. It goes through
`m_user_bigram->save_db(user_bigram.db.tmp)` and a rename
(`pinyin.cpp:1014-1018`, `1116-1121`; `zhuyin.cpp:685-687`).
`save_db` opens a fresh `HashDBM` with `OPEN_DEFAULT` and no tuning
(`ngram_tkrzwdb.cpp:65-84`). That gives HashDBM's default bucket count,
and `InitializeBuckets` zero-fills the whole bucket array
(`tkrzw_dbm_hash.cc:1384`). The file therefore holds at least 4 MiB of
bucket slots, even when it has no rows.

**The walk.** At tkrzw 9ee8416:

- `HashDBMIteratorImpl::First` sets bucket 0 (`tkrzw_dbm_hash.cc:2298-2306`).
- `ReadKeys` calls `ReadNextBucketRecords` (`:2384-2399`).
- That function loops over the buckets until one has a record
  (`:1836-1886`). For each bucket it takes a `ScopedHashLock` and calls
  `GetBucketValue` (`:1638-1647`).
- `GetBucketValue` calls `MemoryMapParallelFile::Read` for 4 bytes
  (`tkrzw_file_mmap.cc:433-445`). That builds a `Zone`, copies the bytes
  with `memcpy` and constructs and destroys several `Status` objects.
- With 0 records, one call walks all 1,048,583 buckets and then reports
  NOT_FOUND. With records, the same total walk is split across the
  `Next` calls.

**Open state and parameters.** All three files are healthy, so the
read-only open runs no restore: restore runs only on a writable open
(`tkrzw_dbm_hash.cc:286`). `HashDBMImpl::OpenImpl` costs 14,864 Ir in
total for its nine calls. No open parameter changes the walk:

- `HashDBMImpl::LoadMetadata` reads `num_buckets_` from the file
  (`:1304-1349`). The `num_buckets` tuning given at open
  (`SetTuning`, `:1123-1170`) is replaced for an existing file.
- `cache_buckets` takes effect only for a positional file
  (`TuneFileAfterOpen`, `:1373-1381`). Both sides use the default
  `MemoryMapParallelFile`: for the pin, `HashDBM::HashDBM` at `:2402`;
  for ox, the PolyDBM default. Callgrind shows that class on both sides.

## Scaling

Command: [`mkbigram.py`](#mkbigrampy) wrote each `user_bigram.db` variant
into a copy of the `phrases` profile, through the same libtkrzw. It uses
`dbm=hash` with only `num_buckets` and `offset_width` set, as `save_db`
does with defaults. The 1,048,583-bucket empty variant differs from the
pin-written file only in metadata bytes 53–56 (`cmp -l`; the timestamp).
Its init Ir equals the provisioned profile's exactly. Init Ir for pinyin
then came from [`attrib.py`](#attribpy) with sides ox and pin. The tables
in this section give the `<facade>_init` toggle alone; the constructors add
1 (ox) and 19,011 (pin) Ir. These are analysis inputs only, not shipping
candidates.

| variant | buckets | records | file bytes | ox init Ir | pin init Ir | ox per bucket | pin per bucket |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| no `user_bigram.db` | — | — | — | 10,637,928 | 10,808,683 | — | — |
| b16384 | 16,411 | 0 | 69,632 | 15,793,070 | 15,964,599 | 314.13 | 314.17 |
| b65536 | 65,537 | 0 | 266,240 | 31,218,634 | 31,390,163 | 314.03 | 314.04 |
| b262144 | 262,147 | 0 | 1,052,672 | 92,954,174 | 93,125,703 | 314.01 | 314.01 |
| default (as provisioned) | 1,048,583 | 0 | 4,198,400 | 339,895,078 | 340,066,607 | 314.00 | 314.00 |
| default, offset width 5 | 1,048,583 | 0 | 5,246,976 | 339,895,078 | 340,066,607 | 314.00 | 314.00 |

"Per bucket" is (init Ir − the no-file row) / buckets. Requested bucket
counts are rounded by `GetHashBucketSize`. The empty user directory
(no user files at all) gives 1,898,429 (ox) / 5,997,766 (pin) Ir.

| records (1,048,583 buckets) | file bytes | ox init Ir | pin init Ir | ox per record | pin per record | ox/pin per record | ox/pin init |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 0 | 4,198,400 | 339,895,078 | 340,066,607 | — | — | — | 0.999 |
| 1,000 | 4,222,400 | 345,927,743 | 343,532,873 | 6,032.7 | 3,466.3 | 1.740 | 1.007 |
| 10,000 | 4,438,400 | 400,190,916 | 374,742,353 | 6,029.6 | 3,467.6 | 1.739 | 1.068 |
| 100,000 | 6,598,400 | 945,952,159 | 690,096,077 | 6,060.6 | 3,500.3 | 1.731 | 1.371 |

"Per record" is (init Ir − the 0-record row) / records. Each synthetic
record has a 4-byte key and a 12-byte single-item SingleGram value. A
profile the pin trained has larger values. The per-record figures are
therefore for these inputs only.

So the cost is linear in the bucket count. The file size does not
matter, apart from the records in it. The records add a separate term,
which is not part of the reported 35×: every provisioned profile has 0
records.

**The ox per-record term (oxpinyin's own).** This term is above the pin's
by about 2,560 Ir per record. In the 100,000-record variant:

- On ox, `HashDBMImpl::ProcessImpl` costs 435,757,672 Ir inclusive. On
  the pin it costs 119,341,870.
- ox inserts each row from inside the iterator's record callback.
  `TkrzwUserBigramDb::load_db` calls `db_set_one` → `db_apply` →
  `tkrzw_dbm_process_multi` for one record. In libtkrzw this goes through
  `TinyDBMImpl::ProcessMulti`, `HashMutex::LockMulti` and
  `ScopedHashLockMulti`.
- The pin copies each row with `DBM::Iterator::Get` and then
  `TinyDBM::Set` (`tkrzwdb_utils.h:62-69`).
- `malloc` runs 13.0 times per record on ox and 2.0 on the pin
  (1,302,717 and 202,958 calls).

This term is not reached by the provisioned profiles. It is not a
behaviour difference. It is a cost difference against the pin that grows
with the user's bigram rows. It falls under the Stage 2 complexity floor,
and needs its own issue.

## Warm init wall time

Command: [`warmtime.py`](#warmtimepy) with cases `phrases,journals`, run
12:40:45Z–12:42:07Z. It drives `tools/bisection/teardown.py`'s own
`Campaign` with the `confirm` stage's timing shape:

- 60 timed rounds after 3 warm-ups, ox, control and pin rotating in each
  round;
- one worker process per sample, pinned to CPU 2, with a fresh profile
  copy;
- every timed row's workload observation must equal its side's first row;
- all three sides compare equal before timing starts. All four cells
  were COMPARABLE.

`teardown.report_times` applies the §7.5 rule (`stable_control`,
`CONTROL_INTERVAL`, 4,000 resamples, seed `0x074A2219`) and suppresses
ox/pin when the control fails. The teardown controller rejects
`--teardown-stage confirm` on tkrzw, because its stage comparisons read
BDB containers. That guard was not changed; the script calls the
controller's functions directly. The 1-minute load average was 0.23
before the run and 0.86 after it.

| cell | clock | ox median µs | control median µs | pin median µs | control/ox (95% CI) | verdict | ox/pin (95% CI) |
| --- | --- | ---: | ---: | ---: | --- | --- | --- |
| pinyin `phrases` | wall | 54,676 | 56,584 | 55,090 | 1.006 (0.979–1.069) | INCONCLUSIVE | — |
| pinyin `phrases` | CPU | 54,315 | 54,263 | 54,389 | 1.014 (0.969–1.074) | INCONCLUSIVE | — |
| pinyin `journals` | wall | 56,245 | 56,526 | 56,099 | 1.010 (0.968–1.053) | INCONCLUSIVE | — |
| pinyin `journals` | CPU | 54,577 | 54,532 | 54,111 | 1.023 (0.983–1.058) | INCONCLUSIVE | — |
| zhuyin `phrases` | wall | 58,694 | 57,289 | 54,882 | 0.988 (0.931–1.026) | INCONCLUSIVE | — |
| zhuyin `phrases` | CPU | 57,198 | 57,039 | 53,611 | 0.984 (0.929–1.019) | INCONCLUSIVE | — |
| zhuyin `journals` | wall | 57,772 | 56,635 | 55,562 | 0.969 (0.952–1.029) | STABLE | **0.996 (0.953–1.044)** |
| zhuyin `journals` | CPU | 55,870 | 55,273 | 54,462 | 0.963 (0.945–0.999) | INCONCLUSIVE | — |

The medians of INCONCLUSIVE rows are what the tool printed. They carry no
ratio.

**Diagnostic: without the file.** Command: `warmtime.py` with the
profiles directory `$W/scale` and case `nobigram`. That is the `phrases`
profile with `user_bigram.db` removed. Both cells were COMPARABLE.

| cell | ox median µs | control median µs | pin median µs | control/ox (95% CI) | verdict |
| --- | ---: | ---: | ---: | --- | --- |
| pinyin, init wall | 3,332 | 2,957 | 3,041 | 0.952 (0.899–1.020) | INCONCLUSIVE |
| zhuyin, init wall | 3,026 | 2,866 | 2,911 | 0.982 (0.935–1.014) | INCONCLUSIVE |

These rows come from a second campaign in the same sitting. No ratio is
taken between the two campaigns. The medians only show the size of the
walk: about 52 ms of the 55–59 ms warm init on this host, on both sides.

## Changes that would remove the cost

For a ruling; none is implemented. The pin pays the cost the same way,
so a change that keeps the result exact is a Stage 2 improvement: it
makes oxpinyin faster than the pin.

1. **Skip the walk when the source is healthy and empty.** Before
   iterating, read the record count (`tkrzw_dbm_count`; HashDBM returns
   its metadata `num_records_`, `tkrzw_dbm_hash.cc:619-626`) and the
   health (`tkrzw_dbm_is_healthy`). If the file is healthy and the count
   is 0, do not iterate. Otherwise walk as now.
   - *Effect:* this removes the 329M Ir and about 52 ms only for a
     `user_bigram.db` with no rows. That covers every provisioned profile
     and every profile saved before the first bigram is learned. A
     profile with learned bigrams still pays the full walk.
   - *Compatibility:* the in-memory result is the same (an empty
     `TinyDBM`). No file is written or changed, and later saves are
     unchanged.
   - *Why the health check:* an unhealthy file is opened read-only without
     restore. Its metadata count may not match its records, so it must
     keep the pin's walk.
2. **Walk the source with `ProcessEach`.** For an in-place HashDBM opened
   read-only, `HashDBMImpl::ProcessEach` scans the record section in file
   order and reads no buckets (`tkrzw_dbm_hash.cc:518-552`). An
   appending-mode file falls back to the bucket walk (`:553`). The pin
   writes in-place files. The C API entry is `tkrzw_dbm_process_each`
   with `writable=false`. The cost becomes proportional to the record
   bytes, for any record count.
   - *Plain form: compatibility effect.*
     - Rows arrive in file order instead of iterator order. Iterator order
       is bucket index, then key bytes ascending within a bucket, because
       `HashDBMIteratorImpl::keys_` is a `std::set`
       (`tkrzw_dbm_hash.cc:228`).
     - The destination `TinyDBM` has the same 1,048,583 buckets
       (`tkrzw_dbm_tiny.h:42`) and the same `PrimaryHash`. It prepends a
       new key to its bucket chain (`tkrzw_dbm_tiny.cc:680`), and it
       iterates bucket by bucket, then along the chain (`:730-757`).
     - That iteration order is the order in which the next `save_db`
       writes `user_bigram.db`.
     - So when two keys share a bucket, a file saved after a
       `ProcessEach` load can hold its records in a different order from
       the pin's. Its bytes then differ. The plain form is therefore not
       Stage 2 as it stands.
   - *Exact form.* Collect the rows with `ProcessEach`. Then insert them
     in the iterator's order: sort by `(PrimaryHash(key, source
     num_buckets), key)`. That reproduces the pin's `TinyDBM` state.
     - It needs tkrzw's `PrimaryHash` reimplemented in oxpinyin
       (`tkrzw_dbm_common_impl.h:36-44`: MurmurHash with seed 19780211
       and a fold). That hash is part of the file format.
     - It also needs the source's bucket count, from `tkrzw_dbm_inspect`.
     - The cost becomes O(n log n) in records and needs O(n) memory for
       the collected rows. The time gain must be weighed against that
       space under the Source policy.

Not removable by an open change:

- **A different open.** No open flag or tuning parameter changes the walk
  (see [Open state and parameters](#what-libtkrzw-does-at-init-for-which-files)).
- **Writing fewer buckets.** Sizing `user_bigram.db` to its records when
  oxpinyin saves would change the saved bytes (metadata `num_buckets`
  and the bucket array). That breaks byte compatibility with the pin's
  `save_db`, so it would be a Stage 3 opt-in. Files the pin wrote would
  still have 1,048,583 buckets.

Separately, the ox per-record term [above](#scaling) could be reduced
internally, for example by setting each copied row with a single-record
call instead of `tkrzw_dbm_process_multi`. That needs its own analysis
that the resulting `TinyDBM` state is identical.

## Upstream finding

libpinyin's tkrzw `Bigram::load_db` copies `user_bigram.db` with a HashDBM
iterator. tkrzw's HashDBM iterator walks every bucket. `Bigram::save_db`
always writes 1,048,583 buckets. Together these make every tkrzw init
read about 4 MiB of bucket slots, even for a user with no learned bigrams.
On this host that is about 329M instructions and about 52 ms warm. This
is the pin's own cost in tkrzw code, and oxpinyin reproduces it exactly.
It is recorded here for upstream, not in `upstream-divergences.md`.

## What this does not show

- The cost at save. `save_db` also walks a 1,048,583-bucket `TinyDBM` and
  writes a fresh file of that size. It was not measured.
- Memory. The walk reads the whole 4 MiB bucket array through the mapping
  and touches its pages. Pss and Rss were not measured here.
- Cold cache, and the BDB and KC cells. These were not run.
- Profiles with real learned bigrams. The record axis uses synthetic
  files.
- Another CPU or arm64. These were not run. The Ir figures depend on this
  libtkrzw build (host GCC 13.3.0).

## Scripts

The scripts below ran from the scratch directory of this session, with
the repository at `/home/user/oxpinyin`. They are shown condensed:
docstrings and one diagnostic print are removed and some lines are
joined. The calls and the arithmetic are the ones that ran. Each Python
script ran as
`python -m tools.dev slot run -- uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal <script> …`,
except `mkbigram.py`, which is light. The environment came from
`source ~/.oxpinyin-oracle.env`, with `W=/home/user/w1099`,
`L=$W/teardown/libraries` and `TMPDIR=$W/tmp`.

### attrib.py

Run as
`attrib.py $W/bisect "$OXPINYIN_ORACLE_DATA_TKRZW" $W/teardown/profiles $W/scratch 2 30 ox=$L/subject/lib{facade}.so control=$L/control/lib{facade}.so pin=$L/pinlibs/lib{facade}.so -- pinyin:phrases pinyin:journals zhuyin:phrases zhuyin:journals`.
For scaling, the profiles directory was `$W/scale`, the sides were ox and
pin, and the cells were the variant names. The profile name `none`
selects an empty user directory. The Callgrind options are
`generate-function-order.py::callgrind_prefix`:
`--cache-sim=no --branch-sim=no --demangle=no --separate-recs=1 --separate-callers=0 --dump-instr=yes --collect-atstart=no --toggle-collect=<toggle> --callgrind-out-file=/dev/stderr`.

```python
"""Init-scoped Callgrind attribution for #1099, in memory."""
import collections, json, os, re, runpy, shutil, subprocess, sys
from pathlib import Path

ROOT = Path('/home/user/oxpinyin')
CD = runpy.run_path(str(ROOT / 'tools/bisection/contract-diff.py'), run_name='contract_diff')
GEN = runpy.run_path(str(ROOT / 'tools/generate-function-order.py'), run_name='gen')
CHECK = runpy.run_path(str(ROOT / 'tools/tkrzw/runtime.py'))['check']


def demangle(names):
    names = list(names)
    out = subprocess.run(['c++filt'], input='\n'.join(names) + '\n', capture_output=True, text=True, check=True)
    return dict(zip(names, out.stdout.splitlines()))


def run(worker, data, library, user, facade, cpu, toggle, env, totals_only=False):
    command = [*GEN['callgrind_prefix'](worker, cpu, toggle),
               '--teardown', facade, str(library), str(data), str(user),
               'time', '0', '1', '1', 'first-candidate', '-']
    child = subprocess.run(command, capture_output=True, env=env, timeout=3600)
    if child.returncode:
        raise RuntimeError(child.stderr[-3000:].decode(errors='replace'))
    text = child.stderr.decode('utf-8', 'strict')
    if totals_only:  # a constructor toggle may collect nothing
        totals = [int(v) for v in re.findall(r'^totals:\s*(\d+)\s*$', text, re.M)]
        if len(totals) != 1:
            raise ValueError('expected one Callgrind totals line')
        return dict(total=totals[0]), command
    return CD['per_call_profile'](text), command


def main():
    worker, data, profiles, work, cpu, top = sys.argv[1:7]
    rest = sys.argv[7:]
    split = rest.index('--')
    sides = dict(item.split('=', 1) for item in rest[:split])
    cells = [item.split(':') for item in rest[split + 1:]]
    worker, data, profiles, work, top = Path(worker), Path(data), Path(profiles), Path(work), int(top)
    env = dict(os.environ, LC_ALL='C')
    for facade, profile in cells:
        results = {}
        for index, (side, lib) in enumerate(sides.items()):
            library = Path(lib.replace('{facade}', facade))
            env['OXPINYIN_TEARDOWN_EXPORTS'] = GEN['teardown_exports'](library, facade)
            scratch = work / ('attrib-%d' % index)  # equal-length paths for every side
            if scratch.exists():
                shutil.rmtree(scratch)
            scratch.mkdir()
            try:
                copy = scratch / ('lib' + facade + '.so')
                shutil.copyfile(library, copy)
                user = scratch / 'user'
                if profile == 'none':
                    user.mkdir()
                else:
                    shutil.copytree(profiles / profile / 'user', user)
                CHECK('auto', 'attrib.py', side, copy, env=env)
                profile_data, command = run(worker, data, copy, user, facade, cpu, facade + '_init', env)
                constructors = sum(run(worker, data, copy, user, facade, cpu, symbol, env, totals_only=True)[0]['total']
                                   for symbol in GEN['init_array_symbols'](copy))
            finally:
                shutil.rmtree(scratch)
            results[side] = (profile_data, constructors, copy)
        for side, (p, ctor, copy) in results.items():
            print(json.dumps(dict(cell=[facade, profile], side=side, init_toggle_ir=p['total'],
                                  constructor_ir=ctor, init_plus_constructors=p['total'] + ctor)))
        for side, (p, ctor, copy) in results.items():
            exclusive = p['exclusive']
            inclusive = collections.Counter(exclusive)
            calls = collections.Counter()
            for (caller, callee), (cost, count) in p['edges'].items():
                inclusive[caller] += cost
                calls[callee] += count
            by_object = collections.Counter()
            for (obj, fn), cost in exclusive.items():
                by_object[Path(obj).name if obj != str(copy) else 'FACADE'] += cost
            names = demangle({fn for _, fn in list(exclusive) + list(inclusive)})
            def label(key):
                obj, fn = key
                return ('FACADE' if obj == str(copy) else Path(obj).name) + ' :: ' + names.get(fn, fn)
            print(json.dumps(dict(cell=[facade, profile], side=side, exclusive_by_object=by_object.most_common())))
            for kind, table in (('exclusive', collections.Counter(exclusive)), ('inclusive', inclusive)):
                for key, cost in table.most_common(top):
                    print('%s %s %s %-9s %12d %7.3f%% calls=%d  %s' % (facade, profile, side, kind, cost,
                          100 * cost / p['total'], calls.get(key, 0), label(key)))


if __name__ == '__main__':
    main()
```

### mkbigram.py

Run as `python3 -I mkbigram.py "$OXPINYIN_TKRZW_LIBRARY" <variant>/user/user_bigram.db <buckets> <offset width> <records>`.
The variants were `16384 4 0`, `65536 4 0`, `262144 4 0`, `1048583 4 0`,
`1048583 5 0`, `1048583 4 1000`, `1048583 4 10000` and
`1048583 4 100000`. Each variant directory is `$W/scale/<name>`, holding
a copy of the provisioned `phrases/user` with `user_bigram.db` replaced.
`nobigram` is the same copy with the file removed.

```python
import ctypes, struct, sys

lib = ctypes.CDLL(sys.argv[1])
lib.tkrzw_dbm_open.restype = ctypes.c_void_p
lib.tkrzw_dbm_open.argtypes = [ctypes.c_char_p, ctypes.c_bool, ctypes.c_char_p]
lib.tkrzw_dbm_set.restype = ctypes.c_bool
lib.tkrzw_dbm_set.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_int32,
                              ctypes.c_char_p, ctypes.c_int32, ctypes.c_bool]
lib.tkrzw_dbm_synchronize.restype = ctypes.c_bool
lib.tkrzw_dbm_synchronize.argtypes = [ctypes.c_void_p, ctypes.c_bool, ctypes.c_void_p, ctypes.c_void_p,
                                      ctypes.c_char_p]
lib.tkrzw_dbm_close.restype = ctypes.c_bool
lib.tkrzw_dbm_close.argtypes = [ctypes.c_void_p]
out, buckets, width, records = sys.argv[2], int(sys.argv[3]), int(sys.argv[4]), int(sys.argv[5])
params = f'dbm=hash,truncate=true,num_buckets={buckets},offset_width={width}'.encode()
db = lib.tkrzw_dbm_open(out.encode(), True, params)
if not db:
    sys.exit('open failed')
value = struct.pack('<III', 1, 0x01000001, 1)  # total_freq 1, one BigramItem
for i in range(records):
    if not lib.tkrzw_dbm_set(db, struct.pack('<I', 0x01000001 + i), 4, value, len(value), True):
        sys.exit('set failed')
if not lib.tkrzw_dbm_synchronize(db, False, None, None, b'') or not lib.tkrzw_dbm_close(db):
    sys.exit('close failed')
```

### warmtime.py

Run as `warmtime.py $W/timing "$OXPINYIN_ORACLE_DATA_TKRZW" $W/teardown/profiles 2 $L/subject $L/control $L/pinlibs phrases,journals`.
The diagnostic used `$W/scale` and `nobigram`.

```python
import runpy, sys, types
from pathlib import Path

ROOT = Path('/home/user/oxpinyin')
sys.path.insert(0, str(ROOT / 'tools/bisection'))
import teardown

CD = runpy.run_path(str(ROOT / 'tools/bisection/contract-diff.py'), run_name='contract_diff')
CHECK = runpy.run_path(str(ROOT / 'tools/tkrzw/runtime.py'))['check']


def main():
    work, data, profiles, cpu, ox, control, pin, cases = sys.argv[1:9]
    work, data, profiles = Path(work), Path(data), Path(profiles)
    driver = types.SimpleNamespace(**{name: CD[name] for name in
                                      ('check_cancelled', 'terminate_group', 'index_file_rows', 'zero_item_padding')})
    worker, identity = teardown.build_worker(work)
    teardown.print_record('worker', worker=identity)
    campaign = teardown.Campaign(worker, data, work, int(cpu), driver)
    dirs = dict(ox=Path(ox), control=Path(control), pin=Path(pin))
    for facade in ('pinyin', 'zhuyin'):
        subjects = {side: directory / ('lib' + facade + '.so') for side, directory in dirs.items()}
        for side, library in subjects.items():
            CHECK('auto', 'warmtime.py', side, library)
        for case in cases.split(','):
            profile = profiles / case / 'user'
            references = {side: teardown.workload_observation(campaign.run(facade, library, profile, mode='time'))
                          for side, library in subjects.items()}
            comparable = all(value == references['pin'] for value in references.values())
            teardown.print_record('equivalence', facade=facade, case=case,
                                  verdict='COMPARABLE' if comparable else 'NOT COMPARABLE',
                                  digests={side: teardown.identity(value) for side, value in references.items()})
            if not comparable:
                continue
            samples = {side: [] for side in subjects}
            for index in range(teardown.TIMING_ROUNDS + teardown.TIMING_WARMUPS):
                order = ['ox', 'control', 'pin']
                order = order[index % 3:] + order[:index % 3]
                for side in order:
                    row = campaign.run(facade, subjects[side], profile, mode='time')
                    if teardown.workload_observation(row) != references[side]:
                        raise ValueError(f'timed workload observation changed: {facade} {case} {side}')
                    if index >= teardown.TIMING_WARMUPS:
                        samples[side].append(row)
            teardown.report_times(facade, case, samples, False)


if __name__ == '__main__':
    main()
```

## References

- [Issue #1099](https://github.com/shenghaoc/oxpinyin/issues/1099).
- [#1096 backend figures](backend-function-order-1076.md).
- [BDB init attribution](init-attribution-2026-10-22.md).
- [Section 7.5](tooling-consolidation-python-just.md#75-metrics-and-statistics).
- [tkrzw distro compatibility](tkrzw-distro-compat.md).
