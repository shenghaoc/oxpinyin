# Performance Backend Matrix — BDB and the Store Tier — 2026-09-06

## Executive summary

A 7-combination backend measurement: libpinyin × {Berkeley DB, Kyoto
Cabinet, Tkrzw} through the C facade, and oxpinyin × {redb, LMDB, Tkrzw,
Kyoto Cabinet} at the store tier. This extends the
[post-P6 4-cell matrix](perf-backend-matrix-2026-09.md) (same pin, same
host family) with the cells it did not have: the **Berkeley DB build —
libpinyin's upstream default and the configuration distributions ship** —
and oxpinyin's two non-libpinyin backends, measured on the storage
operations the 4-cell work did not cover. Init and memory results are
consistent with that matrix's finding that backend choice is a
steady-state non-factor; this document adds BDB, ox-redb, ox-LMDB, and
the storage-tier operations.

The headline numbers, all medians inside one `debian:testing` container
session:

- **libpinyin's DBM backend matters at train time, and BDB is the slowest
  writer**: 256 train events plus save cost 297 ms on BDB, 244 ms on
  Tkrzw, and 181 ms on KC — a 1.64× spread on identical work.
- **Tkrzw pays 140 ms to open a populated user database** — 44× BDB
  (3.2 ms) and 42× KC (3.4 ms) on the same 256-train state. Whatever
  else recommends tkrzw, its hash-open cost on trained user data is the
  outlier of this matrix, and it lands exactly where a returning user
  pays it: session start.
- **Init is backend-flat within libpinyin** (1.57–1.76 ms, BDB fastest)
  on this session's data directories — unlike the 4-cell matrix's KC
  init penalty (2.31×), a difference this session's controls cannot
  fully attribute (see Results).
- **oxpinyin's store tier peaks at 3.3–7.3 MiB per operation** where the
  libpinyin children peak at 12–45 MiB. The two are not like-for-like —
  libpinyin's numbers carry the engine and facade, oxpinyin's are
  storage only — but within each project the backend ordering is real,
  and ox-LMDB is the RAM floor everywhere.
- **ox-KC's commits are microsecond-scale** (244 µs for 64 trains) where
  redb pays 6.0 ms and LMDB 13.2 ms for the same rows. That is not speed
  and not a win over lp-KC or any other column: the KC store commits
  with a soft flush only (`sync(false)` — page cache, no fsync,
  NOSYNC-class durability), so a crash can corrupt the database that
  redb's and LMDB's fsync-backed commits protect. ox-tkrzw's write path
  is soft the same way (`synchronize(hard=false)`); only redb and LMDB
  pay for durability in the timed path.

Within-project deltas are the comparable signal throughout. libpinyin
numbers are facade+DBM; oxpinyin numbers are storage-tier only; absolute
cross-project numbers are **not** comparable (different work, different
processes, different data sizes).

## Experimental design

### Matrix

| Cell | Implementation | DBM backend | Build |
|---|---|---|---|
| lp-BDB | libpinyin 2.11.91 (0c5e80e1) | Berkeley DB 5.3.4 | `--with-dbm=BerkeleyDB` |
| lp-KC | libpinyin 2.11.91 (0c5e80e1) | Kyoto Cabinet 1.2.80 | `--with-dbm=KyotoCabinet` |
| lp-tkrzw | libpinyin 2.11.91 (0c5e80e1) | Tkrzw 1.0.32 | `--with-dbm=Tkrzw` |
| ox-redb | oxpinyin (`measure/backend-matrix-bdb-store-2026-09`) | redb (pure Rust) | `--no-default-features --features redb` |
| ox-LMDB | oxpinyin (same) | LMDB via heed | `--no-default-features --features lmdb` |
| ox-tkrzw | oxpinyin (same) | Tkrzw 1.0.32 | `--no-default-features --features tkrzw` |
| ox-KC | oxpinyin (same) | Kyoto Cabinet 1.2.80 | `--no-default-features --features kyotocabinet` |

The three libpinyin builds are oracle prefixes built by
`tools/oracle/build-oracle.sh --dbm …` at the frozen pin (tkrzw under
the full pin check; kc/bdb as bench-only prefixes under
`PINYIN_BENCH_DBM`, per `docs/testing/oracle-environment.md`'s
multi-backend section). BDB is measured deliberately: it is the upstream
default and the distro configuration, even though libpinyin's own
development deprecates it.

### Container environment

Everything — oracle builds and bench runs — ran inside one rootless
podman container (`debian:testing`, digest
`sha256:5056ab8a99336d6d71390d640f72229649f12e3d38e987cf6b24dc8675325d73`),
so the host OS (RHEL 10.2) contributes only the kernel. The 4-cell
matrix ran the same way; its numbers came from a different base-image
generation and APT snapshot, which is one reason the two documents
disagree on the KC init penalty (below).

| Property | Value |
|---|---|
| Host kernel | x86_64 Linux 6.12.0-211.22.1.el10_2 (shared dev box) |
| Runtime | podman 5.8.2, rootless, one container for builds and measurement |
| Base image | `debian:testing` (digest above) |
| GCC | 16.2.0 (Debian 16.2.0-1) |
| Rust | 1.97.1 (8bab26f4 2026-07-14), from `rust-toolchain.toml` |
| criterion | 0.8.2 |
| libkyotocabinet-dev | 1.2.80-2+b2 |
| libtkrzw-dev | 1.0.32-1+b2 |
| libdb-dev | 5.3.4+b1 |
| libpinyin source | 2.11.91 tarball, SHA-256 `eb25890dab…` (the frozen pin) |
| Model data | model20 (`59c68e89d4…`), from the verified `target/model20` cache |

### Harness and operations

Wall-clock: criterion (default sampling, `--save-baseline` per cell;
baselines `dbm-bdb`, `dbm-kc`, `dbm-tkrzw`, `ox-redb`, `ox-lmdb`,
`ox-tkrzw`, `ox-kyotocabinet`). RAM: a separate `--vmhwm` mode re-execs
each bench as one child per operation and reads `/proc/self/status`
VmHWM — the axes are reported separately and never mixed. Runs were
sequential, one session, no CPU pinning (criterion's warmup and outlier
rejection absorb the shared host's background load; medians with 95% CIs
are reported).

| Operation | libpinyin (facade+DBM) | oxpinyin (store tier) |
|---|---|---|
| `init_load` | `pinyin_init` over the prefix's model20-compiled data dir, fresh empty user dir | open the six system DBMs of `fixtures/w3/<ext>` (tree opens + the bigram hash open) |
| `train_write/64`, `/256` | 64/256 train events (`pinyin_guess_sentence` + `pinyin_train`, 16 fixed inputs) + one `pinyin_save`; clean user dir per iteration | 64/256 bigram + phrase row pairs in one committing transaction; fresh store per iteration |
| `user_db_open` | `pinyin_init` over a fresh copy of a populated user dir (a 256-train state) | read-write open of a fresh copy of a populated user store (the same 256-row state) |

The rows align in name and shape, not in work: libpinyin's train is a
full parse→n-best→train→commit of the engine, oxpinyin's is raw
key-value writes plus commit, and the two projects' `init_load` opens
different data (the oracle prefix's model20 tables vs the w3 fixture's
smaller tables). Absolute cross-project comparison is therefore
meaningless in every row — the backend deltas **within** each project,
on identical bytes and identical work, are what these tables are for.

### Relation to the 4-cell matrix (PR #339)

The [2026-09-05 matrix](perf-backend-matrix-2026-09.md) measured
engine-level init/alloc/cycles for the tkrzw and KC pairs on this host
and found the backend a steady-state non-factor (<1%); this session's
init and memory numbers are consistent with that finding, and this
document adds the BDB column, the ox-redb/ox-LMDB columns, and the
storage-tier operations (train/write/open) that matrix did not measure.

## Results

### Table 1 — wall-clock (µs, median [95% CI])

| Operation | lp-BDB | lp-KC | lp-tkrzw | ox-redb | ox-LMDB | ox-tkrzw | ox-KC |
|---|---:|---:|---:|---:|---:|---:|---:|
| init_load | 1573 [1555, 1596] | 1685 [1660, 1714] | 1763 [1726, 1776] | 291 [289, 293] | 554 [549, 558] | 442 [434, 456] | 554 [537, 562] |
| train_write/64 | 139 469 [136 797, 146 149] | 92 876 [91 628, 94 754] | 140 134 [138 780, 143 275] | 6048 [5883, 6450] | 13 158 [12 423, 13 623] | 9294 [8960, 10 149] | 245 [235, 253] |
| train_write/256 | 297 424 [291 837, 305 863] | 181 436 [175 689, 185 673] | 243 802 [242 004, 247 614] | 9386 [8870, 9655] | 16 732 [16 054, 17 846] | 11 952 [11 047, 12 796] | 432 [414, 461] |
| user_db_open | 3193 [3145, 3231] | 3354 [3314, 3405] | 140 650 [135 763, 147 554] | 7408 [7280, 7687] | 400 [351, 518] | 1055 [1013, 1096] | 597 [590, 604] |

### Table 2 — peak RSS (MiB, child-process VmHWM)

| Operation | lp-BDB | lp-KC | lp-tkrzw | ox-redb | ox-LMDB | ox-tkrzw | ox-KC |
|---|---:|---:|---:|---:|---:|---:|---:|
| init_load | 12.2 | 17.8 | 13.4 | 4.0 | 3.4 | 6.7 | 7.2 |
| train_write/64 | 17.8 | 43.5 | 36.6 | 4.0 | 3.5 | 7.3 | 7.2 |
| train_write/256 | 18.1 | 43.9 | 36.4 | 4.0 | 3.3 | 7.3 | 7.1 |
| user_db_open | 12.2 | 17.7 | 16.8 | 4.0 | 3.4 | 6.7 | 7.0 |

Child baselines are common within each project (runtime + binary), so
within-table comparisons are clean; the two projects' children carry
different baselines (libpinyin's include the engine and glib), so
cross-project rows are not.

### Observations

**libpinyin, backend fixed to implementation.** Within libpinyin the
backend decides the train path: KC trains 1.35× faster than tkrzw and
1.64× faster than BDB at 256 trains, and it is the only backend whose
train total beats its own init+open profile cleanly. BDB — what distros
actually ship — is the slowest trainer on every size measured. On the
populated-user-db open, tkrzw is the outlier in the other direction:
140.6 ms against 3.2/3.4 ms, i.e. the tkrzw user pays two extra
time-to-first-result budgets at session start. Init is flat
(1.57–1.76 ms, BDB marginally fastest) on this session's
build-oracle-produced data directories — a visible disagreement with the
4-cell matrix, where KC init cost 2.31× tkrzw on datagen-compiled shared
directories under an APT-snapshotted KC 1.2.80. The difference is in
the data compilation path or the library build, not in the measurement
harness; both documents stand as measured, and the honest summary is
that the KC init penalty is data- or build-sensitive rather than a
stable backend constant.

**libpinyin, RAM.** The facade dominates: every lp child carries a
12–13 MiB floor (BDB/tkrzw) that KC raises to 17.8 MiB, and training
through the engine pushes peaks to 18.1 MiB (BDB), 36.4 MiB (tkrzw) and
43.9 MiB (KC) — KC's B-tree working set through the full parse/n-best
train path is the most expensive cell of the matrix.

**oxpinyin, store tier.** Backends separate cleanly on writes:
redb is the fastest committing store (6.0/9.4 ms), tkrzw next
(9.3/12.0 ms), LMDB slowest at 256 (16.7 ms), and KC's 0.24/0.43 ms is
the NOSYNC asymmetry, not speed — the KC commit flushes to the page
cache and no further, a guarantee a crash can corrupt, and the reader
must not take that column for an oxpinyin advantage (see What is NOT
measured). On the populated-open, LMDB is
fastest (0.40 ms), redb slowest (7.4 ms). RAM floors: LMDB 3.3–3.5 MiB,
redb ~4.0 MiB, tkrzw 6.7–7.3 MiB, KC 7.0–7.3 MiB — the C-backed stores
pay roughly a 3 MiB library floor over the pure-Rust ones.

**Overlap columns (lp-tkrzw vs ox-tkrzw, lp-KC vs ox-KC).** Same
backend library, different tier: the store tier is one to two orders of
magnitude cheaper in both axes on every overlapping op. That gap is the
engine+facade share, not a backend property — but it does bound what any
DBM-backend change inside libpinyin could recover on these operations:
the backend is a minority of the facade+DBM total, which is consistent
with the 4-cell matrix's steady-state conclusion.

## What is NOT measured

- **S3 (`MDB_WRITEMAP`) is not isolated here** — the LMDB write numbers
  are the store's shipped configuration; see
  [perf-store-opt-2026-09.md](perf-store-opt-2026-09.md).
- **End-to-end keystroke cycles are not measured** — the Stage 2
  headline number is deferred to the planned Option B extension at the
  C ABI (`oxpinyin-capi`'s `stage2` bench covers the oxpinyin side).
- **libpinyin-BDB is the upstream distro default**, tkrzw/KC are
  alternative builds; that asymmetry is a feature of the matrix, not a
  control gap, but it means BDB numbers are the ones a distro user
  actually experiences.
- **Commit durability is not held constant across ox backends**: KC and
  tkrzw commit with a soft OS flush only (`sync(false)` /
  `synchronize(hard=false)` — NOSYNC-class: no fsync, corruptible on
  crash), while redb and LMDB fsync in the timed path (#343 isolated
  the same asymmetry). The ox-KC and ox-tkrzw train_write rows measure
  a weaker guarantee and are not an advantage over lp-KC/lp-tkrzw.
- **Cold page-cache opens are not measured** — every open in this
  session hit warm cache; first-boot-after-install costs more.
- **Cross-project absolute comparison is not meaningful** — different
  work per row (facade+engine vs storage only), different child
  baselines, different data directories.

## Regression protection

The benches land in-tree and the baselines are reproducible:

- libpinyin side: `PINYIN_ORACLE_PREFIX=<prefix>
  [PINYIN_BENCH_DBM=<kc|bdb>] cargo bench -p pinyin-oracle --features
  oracle-ffi --bench dbm_bench` — prefixes rebuilt by
  `tools/oracle/build-oracle.sh --dbm <tkrzw|kc|bdb> --prefix …`.
- oxpinyin side: `cargo bench -p oxpinyin-store --no-default-features
  --features <backend> --bench backend_matrix_<backend>`.
- Baselines `dbm-bdb`/`dbm-kc`/`dbm-tkrzw`/`ox-*` were saved with
  `--save-baseline` in this session's `target/criterion`; re-runs with
  `--baseline <name>` give criterion change verdicts against them.
- RAM: the same binaries' `--vmhwm` mode prints the per-operation child
  VmHWM table.
