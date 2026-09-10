# Training-commit hard sync — measured cost, and the sync-at-`save` fix

**Measured 2026-09-09 · decided and implemented 2026-09-10** (both UTC,
`date -u` at the step). The file name carries the measurement date, which
is what it was opened to record; the decision in §7 and the change in
§7.1 are a day later and are dated where they appear. Where this document
says "the measurement", it means 2026-09-09.

## Status

**Measured, decided, implemented.** `8ca10158` ("fix(store): commit to
stable storage on every backend", PR #393, on main at `340ef076`) closed
its own report with: *"A perf-baseline measurement of the training-commit
path on the Linux tkrzw host remains a follow-up before this ships in a
release."* This document is that follow-up, and it now also records what
was done about the answer.

The cost was **material** and lands on an interactive path (§5, §6). Three
options were put to the maintainer (§7); **option 2 was chosen on
2026-09-10** — the per-observation commit goes back to a soft sync, and
the hard sync moves to `WriteStore::compact`, which `UserStore::save`
already calls, so `pinyin_save` remains the point where the user's data
reaches the device. §7.1 records the post-change measurement: the whole
regression is recovered.

The measurement in §3–§6 describes the code **as `8ca10158` left it** and
is kept in that tense deliberately — it is the evidence the decision
rests on, not a description of current `main`.

One claim in `8ca10158`'s own report does not survive the measurement, and
§6 says so plainly: *"Runtime writes are user-paced (train / choose /
add-phrase, §4 save) and decode never writes, so no measured surface
regresses."* `pinyin_train` is not merely user-paced — it is called
**synchronously from the candidate-selection keystroke**, and it regressed
13–31× per commit.

## 1. Executive summary

- **Per training commit, the hard sync adds 1.2–1.9 ms** on the default
  (tkrzw) backend. A commit goes from **0.039–0.156 ms** to
  **1.21–2.00 ms** across four runs — a **13–31×** slowdown. (§5)
- **The added cost is per *commit*, not per *row*.** A commit carrying 512
  puts pays the same ~1 ms as one carrying 4 (§5, `train_write/256` vs
  `observe_commit`). That is the signature of one fixed sync per
  transaction, and it is why batching (§7, option 1) would recover almost
  all of it.
- **The mechanism is `msync`, not `fsync`.** tkrzw's TreeDBM is
  mmap-backed, so `Synchronize(hard=true)` issues **two
  `msync(…, MS_SYNC)` per commit** — one over a **551 936-byte** mapped
  data region, one over a 128-byte header. The pre-change arm issues
  **zero** durability syscalls. Counted, not inferred (§4).
- **That region size is the whole story of the absolute number.** A bare
  4 KiB `write` + `fsync` on the same filesystem costs a median **73 µs**
  (§3). The commit costs ~1 ms because `MS_SYNC` writes back every dirty
  page of a ~539 KiB mapping, however few records the transaction touched.
- **Where it lands.** `pinyin_train` walks the trained sentence and calls
  `observe` **once per token**, each its own transaction. An eight-token
  sentence therefore pays **eight** commits: measured **0.92–1.14 ms
  before, 6.3–19.1 ms after** (§5). This runs on the keystroke that
  accepts the candidate, not on a background timer (§6).
- **Upstream pays none of this at train time.** libpinyin's user bigram is
  an in-memory `StashDB` snapshotted only at `pinyin_save` (a 300 s
  consumer timer). The per-observation commit is an already-settled
  oxpinyin divergence; the hard sync is a second cost layered on top of it
  (§6).

## 2. What was measured, and the two arms

The comparison is a **three-line behavioural difference on one tree**, not
two checkouts. Both arms are the branch tip carrying the bench; the
`pre` arm is that tree with `8ca10158` reverse-applied
(`git apply --reverse`, verified to apply cleanly). Exactly three files
differ — `tkrzw/mod.rs`, `kyotocabinet/mod.rs`, and the `WriteStore::write`
doc in `lib.rs` — and the bench source is byte-identical in both. Nothing
else about the tree, the toolchain, the container, or the filesystem
varies between arms.

Verified in-run, printed in each arm's log:

| arm | `tkrzw_dbm_synchronize` | `KcStore` |
| --- | --- | --- |
| `post` | `true` | `self.db.sync(true)` ×2 |
| `pre` | `false` | `self.db.sync(false)` ×2 |

**The bench the measurement needed did not exist.** `backend_matrix`'s
`train_write/{64,256}` put 128 and 512 rows inside **one** transaction, so
a single commit is amortised over the whole batch and a per-commit cost
cannot be resolved from them. The branch adds the two rows that can:

- **`observe_commit`** — one observation: the four read-modify-write
  counter bumps `UserStore::update` commits (bigram pair, that `prev`'s
  bigram total, `cur`'s unigram, the unigram grand total), in one
  transaction.
- **`train_sentence/8`** — eight of those, each its own transaction: the
  shape `Session::train` runs for an eight-token sentence.

Both open a fresh copy of the pre-populated user store in **untimed**
setup, so every iteration starts from byte-identical state and the routine
times commits alone.

## 3. Environment, and what it does and does not license

| | |
| --- | --- |
| Image | `debian:testing`, digest `sha256:dab11cdb0a9dcf4bbd68f671635b35f1f726b452b92396875b69bb2c7daa42a9` |
| Kernel | `Linux 7.0.12-linuxkit aarch64` (Docker Desktop VM, macOS arm64 host) |
| Docker | 29.7.2, arm64; 10 CPUs visible; measurement pinned with `taskset -c 1` |
| Toolchain | 1.97.1 (`rust-toolchain.toml`), `--profile minimal` |
| Backend | tkrzw (the workspace default), Debian `libtkrzw-dev` |
| Store files | `/work/dbtmp` — a Docker named volume, **ext4 on `/dev/vda1`** |
| Harness pin | `crates/oxpinyin-store/benches/support/mod.rs`, git blob **`26469ef1`** (SHA-256 `d8948a7b…`); arms as §2 |

Store files deliberately sit on the named volume, not the container's
overlay and not tmpfs, so the sync reaches a real block device.

**The harness is pinned by content, not by commit.** This row named a
commit SHA until a rebase rewrote it, which is
`perf-provenance-audit-2026-09-07`'s finding happening again in a live
document: a branch-local SHA does not survive a rebase, and will not
survive the rebase-merge that lands it either. A git blob hash does —
`git cat-file -p 26469ef1` recovers the exact bench from any clone that
has the objects, whatever happened to the commits around it. §5.1–§5.3
were taken on the bench as first written; one counter key was corrected
afterwards (the blob above is the corrected form) and §5.4 re-measures
both arms on it.

**Two honest limits on the absolute figures.**

1. `/dev/vda1` is a virtio disk backed by a file on the macOS host's APFS.
   Guest `MS_SYNC` therefore traverses a virtualised block layer whose
   barrier semantics are not a bare-metal SSD's. The numbers are real
   Linux numbers on a real filesystem; they are **not** a bare-metal
   figure, and the bias is most likely **optimistic**.
2. Per-run drift is large — `observe_commit`'s `pre` mean is 0.156 ms in
   run 1 and 0.039 ms in run 3, on identical code. Ranges are reported
   across all three runs rather than a single point, and the **delta** is
   the better-determined quantity than either arm's absolute.

Both limits push the same way: on slower or contended storage — a spinning
disk, an SD card, a network home directory, a loaded machine — a
`MS_SYNC` over ~539 KiB costs far more than it does here. Even on this
host the raw `fsync` p95 (1 105.8 µs) is **15×** its median (73.1 µs).

### Raw durability cost on that filesystem (calibration)

Without this control, no store-tier delta could be attributed to the sync
at all. `fsyncprobe.c`, 4 KiB `pwrite` per iteration, n=400, `taskset -c 1`:

| operation | median | mean | p05 | p95 |
| --- | --- | --- | --- | --- |
| `write` only | 0.4 µs | 0.6 µs | 0.3 µs | 1.9 µs |
| `write` + `fdatasync` | 68.8 µs | 174.3 µs | 56.8 µs | 818.0 µs |
| `write` + `fsync` | 73.1 µs | 216.8 µs | 57.0 µs | 1 105.8 µs |

Syncing is **not** free here (~180× a bare write), so the store-tier delta
in §5 is attributable. It is also far *smaller* than that delta — which §4
explains.

## 4. Mechanism — counted, not inferred

`strace -f -e trace=msync,fsync,fdatasync,sync_file_range` over one
`--test` iteration (criterion's single-pass mode, so the counts are
countable):

| arm | row | sync syscalls | breakdown |
| --- | --- | --- | --- |
| `post` | `observe_commit` | 8 | 4 × `msync(PTR, 551936, MS_SYNC)`, 4 × `msync(PTR, 128, MS_SYNC)` |
| `post` | `train_sentence/8` | 22 | 11 × `msync(PTR, 551936, MS_SYNC)`, 11 × `msync(PTR, 128, MS_SYNC)` |
| `pre` | `observe_commit` | **0** | — |
| `pre` | `train_sentence/8` | **0** | — |

`train_sentence/8` runs exactly seven more commits than `observe_commit`,
and issues exactly 14 more `msync` calls: **2 `msync(MS_SYNC)` per
commit**, one over the mapped data region and one over the 128-byte
header. The absolute counts include the shared untimed setup; the
*difference* is what attributes them, and it is exact.

Three consequences worth stating so nobody re-derives them:

- **`hard=false` issued no durability syscall the trace covers.** The
  traced set is `msync`, `fsync`, `fdatasync` and `sync_file_range`, and
  none of them appears; this says nothing about syscalls outside that
  set, which were not recorded. That is enough for the claim being made,
  because writes land in the mapping and the page cache already makes
  them visible to other processes and durable against a *process* crash
  — exactly what the pre-`8ca10158` module doc claimed. The change is
  `0 → 2` traced durability syscalls per commit, not a cheaper sync made
  dearer.
- **The synced region is fixed at ~539 KiB, whatever the commit touched.**
  Four changed records and 512 changed records cost the same sync. This is
  the direct cause of the flat per-commit delta in §5.
- **oxpinyin cannot narrow that region.** The width is tkrzw's own choice
  of what to map and sync inside `Synchronize`; nothing on the C API it
  exposes lets a caller sync a sub-range. Do not chase it.

## 5. Results

### 5.1 Full matrix, both arms (run 1)

`--sample-size 50 --measurement-time 12 --warm-up-time 3`. Criterion mean,
with median alongside because the distribution is heavy-tailed.

| row | commits | pre mean | post mean | **Δ mean** | × | pre med | post med | Δ med |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `observe_commit` (4 puts) | 1 | 0.156 ms | 2.002 ms | **+1.846 ms** | 12.8× | 0.153 ms | 1.743 ms | +1.590 ms |
| `train_sentence/8` | 8 | 1.145 ms | 15.558 ms | **+14.413 ms** | 13.6× | 0.923 ms | 8.156 ms | +7.233 ms |
| `train_write/64` (128 puts) | 1 | 0.228 ms | 2.801 ms | **+2.573 ms** | 12.3× | 0.191 ms | 1.506 ms | +1.316 ms |
| `train_write/256` (512 puts) | 1 | 0.348 ms | 1.545 ms | **+1.197 ms** | 4.4× | 0.332 ms | 1.390 ms | +1.058 ms |

**Read the last two rows together.** A commit carrying 512 puts pays
+1.197 ms and one carrying 128 pays +2.573 ms — the same order, with no
scaling in the number of rows. The sync is a per-transaction constant.
`train_write/256`'s smaller multiplier (4.4×) is arithmetic, not relief:
its `pre` arm is dearer because it does more real work, so the same
absolute sync divides into a smaller ratio.

### 5.2 `observe_commit` repeated (runs 2 and 3)

`--sample-size 100 --measurement-time 30 --warm-up-time 5`, the row run
alone:

| run | arm | mean | 95% CI | median |
| --- | --- | --- | --- | --- |
| 2 | `post` | 1.444 ms | [1.339, 1.580] | 1.311 ms |
| 3 | `post` | 1.215 ms | [1.174, 1.263] | 1.178 ms |
| 3 | `pre` | 0.0389 ms | [0.0378, 0.0400] | 0.0378 ms |

Run 2's `pre` arm was not measured: the run's syscall step exited on a
`grep` that matched nothing — because the `pre` arm issues no durability
syscalls at all (§4). Run 3 re-ran both arms and is the corrected pass.

### 5.3 The headline, across all runs

| quantity | value |
| --- | --- |
| per-commit, sync off | **0.039 – 0.156 ms** |
| per-commit, sync on | **1.21 – 2.00 ms** |
| **added by the sync** | **+1.18 – +1.90 ms per commit** (§5.4 is the upper bound) |
| ratio | **13× – 31×** |
| eight-token sentence, sync off | 0.92 – 1.14 ms |
| eight-token sentence, sync on | 6.34 – 19.09 ms |

### 5.4 Re-measured on the corrected bench

The bench's observation routine keyed its unigram bump off a token
unrelated to the `(last, cur)` pair the same iteration wrote; the corrected bench
takes it from `pair_key[4..]` (`cur`) so the routine matches production
and its own doc comment. Both keys are 4-byte tokens in comparable
domains, so the correction should be performance-neutral — re-measured
rather than assumed, at the §5.2 flags:

| arm | `observe_commit` | `train_sentence/8` | traced durability syscalls |
| --- | --- | --- | --- |
| soft commit | 0.085 ms (median 0.079, CI [0.080, 0.090]) | 0.426 ms (median 0.397) | **0** |
| hard commit | 1.982 ms (median 1.985, CI [1.878, 2.092]) | 6.428 ms (median 4.856) | **8** |

The picture is reproduced: 0 vs 8 traced durability syscalls, and a
per-commit delta of **+1.90 ms** (23×). Both arms land inside the ranges
§5.3 already reports, except that this run's per-commit delta is a
shade above the old upper bound, so §1 and §5.3 widen it to
**+1.18 – 1.90 ms**. `train_write/{64,256}` are untouched by the
correction — `run_train_write` did not change — so §5.1's rows for them
stand as measured.

The two arms here are "soft commit" and "hard commit" rather than
`pre`/`post`: the bench never calls `compact`, and pre-`8ca10158` and
the shipped fix both soft-sync in `write`, so for these rows the two are
the same measurement.

## 6. Where the cost lands

`pinyin_train` → `Instance::train` (`crates/oxpinyin-facade/src/instance.rs:232`)
→ `Session::train` (`crates/oxpinyin-engine/src/session/selection.rs:300`)
→ `UserModel::observe` → `UserStore::observe_selection` →
`UserStore::update` (`crates/oxpinyin-user/src/store.rs:618`) → **one
`db.write(…)`, one commit, two `msync`.**

`Session::train` calls `observe` **once per token** of the trained
sentence. There is no batching anywhere on that path, so an N-token
sentence costs N commits and 2N `msync` calls.

**This is a keystroke, not a timer.** `docs/findings/abi-subset.md` §
"Phase 3 — Candidate selection" records both consumer paths calling
`pinyin_train` synchronously on the selection that commits the sentence:
`pinyin_train(instance, index)` on the n-best path, `pinyin_train(instance, 0)`
on the normal path once the input is fully consumed. The user waits for it.
`pinyin_choose_predicted_candidate` (Phase 4) is a second such site —
`observe_predicted` is one commit per accepted prediction
(`crates/oxpinyin-capi/src/candidates.rs:503`).

**Against upstream.** libpinyin's user bigram is an in-memory `StashDB`
opened on `"-"` and snapshotted to disk only at `pinyin_save`, which the
consumer runs on a 300 s GLib timer (`docs/findings/rss-attribution-2026-09-09.md`;
`docs/findings/abi-subset.md`, Phase 8). Upstream's per-train disk cost is
therefore **zero**. Two separate oxpinyin decisions stack here, and they
should not be conflated:

1. **The per-observation commit** — already settled and recorded as
   Non-goal 1 of `docs/findings/user-store.md` (oxpinyin's user store is a
   live on-disk store, not upstream's binary format). That decision costs
   0.04–0.16 ms per token (the `pre` arm of §5) against upstream's ~0.
2. **The hard sync** (`8ca10158`) — layered on top, and the subject here.
   It costs a further ~1.2–1.8 ms per token.

**Materiality.** On this host an eight-token sentence commit moves from
~1 ms to ~6–19 ms of blocking store work on the keystroke path. That is
perceptible at the upper end and it is paid on every sentence. On slower
or contended storage it scales with `MS_SYNC` over ~539 KiB, and the tail
is what bites: this host's own raw `fsync` p95 is 15× its median. The
Source policy's rule — a regression "must be minimized, and must be
justified in the change's report" — is not yet met for this path, because
the report that landed it assessed the path as one where "no measured
surface regresses".

**Determinism (constitution item 6) is not engaged.** A sync changes no
output; output remains a pure function of (input, user state, config)
under every option below. The determinism question arises in exactly one
place, and it is option 1's — noted there.

## 7. Options — for a human decision

Listed with the argument each had to clear. This is an externally-visible
durability contract, documented on `WriteStore::write` and in two
backends' module docs, and AGENTS.md's STOP list covers it, so none was
implemented until the maintainer chose one. **Option 2 was chosen and is
implemented (§7.1); options 1 and 3 were not taken.**

### Option 1 — batch a sentence into one transaction *(not taken)*

`Session::train` wraps its whole `observe` loop in a single write
transaction: 8 commits → 1, recovering ~7/8 of the cost on the dominant
path. §5 shows a 512-put commit costs no more sync than a 4-put one, so
the saving is nearly the whole delta.

- **Interface change → STOP.** `UserModel::observe` is a per-token call;
  batching needs a transaction scope across the trait, which is exactly
  the "needs interface change" trigger.
- **It also changes observable failure behaviour.** `Session::train`
  currently trains **prefix-wise** — "Tokens observed before the failure
  stay observed … like the upstream loop"
  (`selection.rs:300`). One transaction rolls all of them back. That is a
  divergence from the pin that would have to be argued into class (a),
  (b) or (c) of `docs/findings/compatibility-policy.md`, and on the face
  of it fits none: it is not math, not memory safety, and not an upstream
  abort. This is the one place where option 1 touches behaviour rather
  than only timing.

### Option 2 — sync at `save`, not at every observation *(**chosen**)*

Restore `hard=false` / `sync(false)` on the per-observation commit and keep
the hard sync in `compact()`, which `UserStore::save` already calls — so
`pinyin_save` still lands the user's data on stable storage. Recovers
essentially 100% of the cost.

- **Closest to the upstream call pattern.** Upstream persists at
  `pinyin_save` and nowhere else; this makes oxpinyin's stable-storage
  point the same one, while still committing every observation to the
  store.
- **It reopens part of what `8ca10158` closed**, and the size of that gap
  should be stated accurately rather than assumed: writes younger than the
  last `pinyin_save` would again be page-cache-resident rather than on the
  device. They survive a process crash — including the IME crashing — and
  are lost only to power loss or a kernel panic. Upstream's exposure over
  the same window is strictly worse (those writes are only in process
  memory), and the consumer's shutdown path never calls `pinyin_save` at
  all.
- **Smallest change of the three**, and it is close to a revert of two
  lines plus a doc correction.

### Option 3 — a durability knob *(not taken)*

Make the sync mode configurable.

- **Weakest.** Constitution item 1 (broad appeal only, no niche features)
  and the drop-in goal both argue against a consumer-visible switch —
  `pinyin.h` has no such option, so it could not be one the consumer sets.
  An internal or environment knob adds a config axis that every durability
  claim then has to be qualified by, and doubles the surface the store
  tests must cover, to serve no user who can reach it.

**Keeping `8ca10158`'s behaviour** would have been a coherent answer too,
recorded against the measured number. It was not the one taken.

## 7.1 What was implemented, and what it recovered

Chosen 2026-09-10. Three files change, all in `oxpinyin-store`:

- **tkrzw** — `db_synchronize` takes a `hard: bool`. `write` passes
  `false`, `compact` passes `true`.
- **Kyoto Cabinet** — `write` commits with `sync(false)`; `compact` keeps
  `sync(true)`.
- **`WriteStore` (the seam contract)** — `write`'s durability note now
  promises only what every backend delivers: a commit that returns has
  left the process, is visible to other readers, and survives a process
  crash. **Stable storage is `compact`'s guarantee**, and the note says
  so, with the measurement cited. redb and LMDB still fsync inside their
  own commits; they exceed the floor, and the doc says callers must not
  read that as the contract. Correcting this over-claim was half the
  point: the architecture review's finding was that the label promised
  more than the code delivered, and the fix for that is an accurate
  label, not only a stronger sync.

`compact` has exactly one production caller — `UserStore::save`
(`crates/oxpinyin-user/src/store.rs:1101`) — so `pinyin_save` is the
stable-storage point, which is precisely upstream's persistence point
(§6). `oxpinyin-datagen` does not compact, so its table files are now
OS-flushed rather than device-synced; for a build-time artifact producer
that is the right trade — a power loss mid-build costs a rebuild, and the
page cache keeps every later read coherent.

### Verified, not assumed

Same container and filesystem as §3. Syscall counts by `strace`, exactly
as §4:

| path | sync syscalls | |
| --- | --- | --- |
| `observe_commit`, one commit | **0** | was 2 × `msync(MS_SYNC)` |
| `train_sentence/8`, eight commits | **0** | was 16 × `msync(MS_SYNC)` |
| `save` → `compact` | **2** — `msync(PTR, 5120, MS_SYNC)`, `msync(PTR, 128, MS_SYNC)` | the hard sync, retained |

The `save` row is the one that matters for the durability claim: the
device-level sync did not disappear, it moved.

### Recovered cost

Criterion, `--sample-size 100 --measurement-time 30 --warm-up-time 5` —
the same flags as §5.2, so the comparison is like-for-like:

| row | hard sync (§5.2) | soft commit + hard `save` | recovered |
| --- | --- | --- | --- |
| `observe_commit` | 1.215 – 1.444 ms | **0.064 ms** (median 0.059, CI [0.060, 0.069]) | −1.15 to −1.38 ms, **19–23×** |
| `train_sentence/8` | 19.090 ms | **0.366 ms** (median 0.367, CI [0.349, 0.383]) | −18.7 ms, **52×** |

`train_sentence/8`'s hard-sync figure at these flags comes from run 2
alone (run 3 measured only `observe_commit`); run 1's 15.558 ms at the
lighter flags gives the same picture. Both post-change rows land at or
below the `pre` arm of §5, so the regression is fully recovered rather
than merely reduced. §5.4 re-runs both arms on the corrected bench and
reproduces this: 0.085 ms against 1.982 ms per commit, a 23× recovery.

### Gates

In-container on the changed tree: `cargo fmt --all --check` clean;
`cargo clippy -p oxpinyin-store --all-targets -- -D warnings` clean on
**all four** backends (the contract doc and both changed backends are
peers behind one seam, so all four are the honest gate); store tests
34 + 6 (tkrzw) and 29 + 6 (Kyoto Cabinet) pass; `oxpinyin-user`'s
92 + 3 + 12 pass, which is where `save`/`compact` is exercised.

## 8. Provenance — the command behind each figure

Regenerable without the capture bundle. The two arms are prepared as §2;
`CARGO_TARGET_DIR` is **per arm**, because cargo's freshness check is
mtime-based and both trees share crate names and paths.

Tree preparation (host):

```sh
# <tip> = any commit carrying bench blob 26469ef1 (§3); verify with
#   git cat-file -p 26469ef1 | shasum -a 256   # -> d8948a7b...
git archive <tip> | tar -x -C <post>
git archive <tip> | tar -x -C <pre>
git show 8ca10158 --format= > hardsync.patch
( cd <pre> && git apply --reverse ../hardsync.patch )
```

§5.4's two arms are simpler still: `<tip>` as-is is the soft arm, and the
hard arm is `<tip>` with `write`'s `db_synchronize(db, false)` flipped to
`true` — the bench never calls `compact`, so that one line is the whole
difference.

Container (`debian:testing` at the digest in §3; apt set = ci.yml's tkrzw
list plus `strace`, `util-linux`, `python3`; rustup `--default-toolchain
1.97.1 --profile minimal`; `TMPDIR=/work/dbtmp` on a named volume):

| figure | command |
| --- | --- |
| §3 raw durability table | `gcc -O2 -o fsyncprobe fsyncprobe.c && taskset -c 1 ./fsyncprobe /work/dbtmp 400` |
| §5.1 full matrix, per arm | `taskset -c 1 cargo bench -p oxpinyin-store --no-default-features --features tkrzw --bench backend_matrix_tkrzw -- '(train_write\|observe_commit\|train_sentence)' --sample-size 50 --measurement-time 12 --warm-up-time 3` |
| §4 syscall counts, per arm/row | `taskset -c 1 strace -f -e trace=msync,fsync,fdatasync,sync_file_range -o tr.txt <bench-bin> --bench <row> --test` |
| §5.2 repeats, per arm | `taskset -c 1 cargo bench -p oxpinyin-store --no-default-features --features tkrzw --bench backend_matrix_tkrzw -- 'observe_commit' --sample-size 100 --measurement-time 30 --warm-up-time 5` |
| §7.1 syscalls, commit path | `strace -f -e trace=msync,fsync,fdatasync -o t.txt <bench-bin> --bench <row> --test` |
| §7.1 syscalls, `save` path | `strace -f -e trace=msync,fsync,fdatasync -o s.txt cargo test -p oxpinyin-user save_reopen_roundtrip_preserves_counts_cursor_and_total` |
| §7.1 recovered cost | the §5.2 command, run on the changed tree |
| §7.1 gates | `cargo fmt --all --check`; `cargo clippy -p oxpinyin-store [--no-default-features --features <be>] --all-targets -- -D warnings` for each of the four backends; `cargo test -p oxpinyin-store` (tkrzw, kyotocabinet) and `cargo test -p oxpinyin-user` |
| all criterion point estimates | read from `$CARGO_TARGET_DIR/criterion/**/new/estimates.json` (`mean`/`median`, `point_estimate` and `confidence_interval`) |

Per-arm build integrity was checked in-run, not assumed: each arm reported
`Compiling count: 65` (a full workspace-local build, not a silent reuse of
the other arm's artifacts) and a distinct bench-binary SHA-256 —
`b3990bae…` for `post`, `a506aa51…` for `pre`.

## 9. Evidence

Per `docs/runbooks/benches.md`, this document commits no captures. The
bundle — every run log (the three measurement runs, the bench gate run,
the post-change verification, and the corrected-bench revalidation), all
runner scripts, `fsyncprobe.c`, and
`hardsync.patch` — is retained on the measuring host at
`~/Documents/oxpinyin-captures/train-commit-fsync-2026-09-09.tar.gz`,
SHA-256 `8ed6764f115c036e754a23d64c32ce199f6a92666b08b27ac2ec6a37171ca4f3`.
It is to be attached to the pull request that carries this document, with
the link added here at that point; until then the host path and the
SHA-256 are what a holder can verify against, and §8 carries the full
reproduction burden.
