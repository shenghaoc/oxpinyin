# Fuzzing the store seam's file ingress — instrumentation, and what a seeded corpus finds

Date: 2026-09-10 · Status: **harness landed, finding open** (no shipping
code changed; one fuzz target, one seeding script, and this record) ·
Branch: `claude/oxpinyin-dbm-fuzzing-6cdb7b`.

Two fuzz targets for file ingress landed 2026-09-08 — `phrase_library`
(the mmap'd chunk reader) and `table_conf` (the one system file read as
text). The remaining ingress was the DBM container behind
`oxpinyin-store`: the file a distro, a user, or an attacker leaves in
the data directory, whose parser is a foreign library reached through
`ReadStore::open_read_only` and `RawReadStore::open_hash_read_only`.
`store-open` fuzzes it.

The project already treats data-directory files as untrusted — audit row
F-3 (`docs/safety/oxpinyin-audit.md`) is a trust-boundary fix against a
`u32` header field in a table file — so the store seam is a trust
boundary by the same standard. The audit's own cross-reference table
listed the store peers' coverage as "the ABI smoke gate and integration
tests", with no fuzzing; this closes the harness half of that gap and
opens a decision on the other half.

## 1. How each backend gets instrumented

`cargo fuzz` passes ASan (`-Zsanitizer=address`) and libFuzzer's
coverage instrumentation through `RUSTFLAGS`, so both reach **Rust**
code and nothing else. The store compiles exactly one backend per
binary, and the four peers sit in three different positions with respect
to that:

| backend | container code | ASan | libFuzzer coverage |
| --- | --- | --- | --- |
| redb | pure Rust | yes | yes — the container steers the fuzzer |
| lmdb | `mdb.c` + `midl.c`, compiled **in this build** by `lmdb-master-sys` through the `cc` crate | yes, with `CFLAGS` | yes, with `CFLAGS` |
| tkrzw | system `libtkrzw.so` | partial (see below) | no |
| kyotocabinet | system `libkyotocabinet.so` | partial (see below) | no |

**LMDB is the one C backend that can be fully instrumented in-tree**,
because its source is compiled by this build rather than linked from the
distro. The `cc` crate honours `CFLAGS`, so:

```sh
CC=clang CFLAGS="-fsanitize=address -fsanitize-coverage=inline-8bit-counters,pc-table,trace-cmp -fno-omit-frame-pointer -g" \
  cargo +nightly fuzz run store-open --no-default-features --features lmdb
```

puts LMDB's C under the same sanitizer and the same coverage feedback as
the Rust. That it works is measurable rather than assumed: libFuzzer
reports the instrumented edge count at startup, and the same target
built with and without those `CFLAGS` loads

```
INFO: Loaded 1 modules   (8038 inline 8-bit counters)   # plain
INFO: Loaded 1 modules   (10482 inline 8-bit counters)  # with CFLAGS
```

— **+2 444 counters**, which is `mdb.c` and `midl.c` entering libFuzzer's
feedback loop. Debian clang 21.1.8 against the fuzz lanes' pinned
`nightly-2026-08-01` links cleanly; there is no ASan-runtime skew to
work around at those versions. Two details are load-bearing:

- **`CFLAGS`, never `CXXFLAGS`.** libfuzzer-sys compiles libFuzzer
  itself through `cc::Build::cpp(true)`, so a `CXXFLAGS` carrying
  `-fsanitize-coverage` would instrument libFuzzer with its own
  counters. `mdb.c` is C and libFuzzer is C++, so the two env vars
  separate them exactly.
- **`inline-8bit-counters,pc-table`, not `trace-pc-guard`**, to match
  what cargo-fuzz asks rustc for; and `CC=clang`, because GCC's
  `-fsanitize-coverage` supports neither of those forms.

**tkrzw and Kyoto Cabinet cannot be instrumented here at all** — the
container is a shared object the distro built, and rebuilding it under a
sanitizer is a provisioning exercise outside this repository. The target
is still worth running on them, for two reasons that are not
consolation:

1. **ASan is process-wide even against an uninstrumented `.so`.** It
   replaces the allocator and interposes the libc string and memory
   routines, so a heap chunk overflowed or used after free inside
   `libtkrzw.so` is still caught when it routes through `malloc`,
   `free`, or `memcpy`. What is lost is intra-object overflow inside one
   C allocation, C stack frames, and coverage feedback — not everything.
2. **The code this repository owns is fully instrumented, and it is
   where the hostile file's influence lands.** The tkrzw backend hands a
   C pointer and an `i32` length to a Rust slice in the
   `tkrzw_dbm_iter_process` callback (`walk_row`), converts every length
   across an `i32` boundary (`c_len`), and un-frames a
   `table || 0x00 || key` prefix from whatever key the library reports.
   A corrupted container that makes the library report a record it does
   not have is exactly the input that tests those, and no C
   instrumentation is needed to test them.

## 2. What a seeded corpus finds

`store-open` takes its input as a database file verbatim, so random
bytes are refused by every backend's header check and an unseeded run
reaches only open-time rejection.
`tools/store/seed-store-fuzz-corpus.sh <backend>` seeds the corpus from
the committed `fixtures/w3/<dir>/` store files, which is what gets past
the header.

**All four peers fault on a corrupted database file opened through the
store seam.** Measured in a `debian:testing` container (aarch64), one
backend per build, ASan on, `detect_leaks=0:abort_on_error=1`:

| backend | how it was reached | first faulting frame | oxpinyin entry point | signal |
| --- | --- | --- | --- | --- |
| kyotocabinet | seeded `cargo fuzz run`, under 120 s | `memcpy` ← `File::read_fast` ← `HashDB::get_bucket` ← `PlantDB::load_meta` ← `PolyDB::open` ← `kcdbopen` | `KcStore::open_read_only` (`kyotocabinet/mod.rs:111`) | ASan SEGV |
| lmdb | seeded `cargo fuzz run`, under 120 s | `mdb_node_search` (`mdb.c:6152`) ← `mdb_page_search_root` ← `mdb_page_search` ← `mdb_cursor_set` ← `mdb_dbi_open` | `LmdbStore::get` (`lmdb.rs:694`) | ASan SEGV |
| tkrzw | dense-container sweep (§4), replayed through `store-open` | `tkrzw::DeserializeLeafNode` ← `CallRecordProcessFull` ← `HashDBMImpl::ProcessImpl` | the `open_hash_read_only` half of `drive` | ASan SEGV |
| redb | dense-container sweep (§4), replayed through `store-open` | `AccessGuard::<&[u8]>::value` (redb 4.2.0 `tree_store/btree_base.rs:237`) | `ReadStore::for_each` → `read_for_each` (`lib.rs:541`) | panic: `range end index … out of range for slice of length 4096` |

Two of the four fall straight out of a 120-second seeded run from the
committed fixtures. The other two do not, and the reason is the seeds,
not the backends: `fixtures/w3/tkt/addon_pinyin_index.bin` holds 11 rows
in 529 408 bytes, so a mutation almost never lands on a live page. A
seeded 120 s pass was clean for redb (234 016 executions) and for tkrzw
(5 341 executions — tkrzw's file operations are two orders of magnitude
more expensive per input); both fall to the denser sweep in §4, and both
crash inputs replay through `store-open` itself.

The signals differ in kind, and the difference is the whole point of §1:

- **Kyoto Cabinet, LMDB and tkrzw fault in C**, all three characteristic
  of a container trusting offsets its own file supplied. LMDB documents
  that it does not defend against a corrupted database; Kyoto Cabinet
  faults inside `open` itself, before any record is asked for.
- **redb's is a safe-Rust panic** — no memory unsafety, an in-bounds
  failure of a bounds check — but still an abort of a long-lived process
  from caller-supplied input.

**The instrumentation matrix in §1 is visible in the reports.** On LMDB
the ASan stack carries `mdb.c` source frames with line numbers, because
that C was compiled by this build under `CFLAGS`. On tkrzw and Kyoto
Cabinet the same crash class comes back as `.so` symbol names with no
source position — and it comes back at all only because ASan is
process-wide: the Kyoto Cabinet report's faulting frame is ASan's own
`memcpy` interceptor firing inside an uninstrumented library. That is
exactly the reduced-but-real coverage §1 claims for those two, observed
rather than asserted.

## 3. Why no lane gates on the seeded pass

Because every backend measured fails it, and none of the failures is in
code this repository owns.

Constitution rule 4 says nothing panics on any input and public APIs
return `Result`; `docs/findings/compatibility-policy.md` class (c)
restates it as a product decision — a library loaded into a long-lived
input-method process must not take the process down on caller error.
A corrupted DBM file is caller-supplied input, and today it takes the
process down on all four peers.

Closing that is a decision about the store seam's contract, not a fuzz
harness change, and it is not one to improvise:

- For **tkrzw and Kyoto Cabinet** there is a parity argument. libpinyin
  opens the same files with the same libraries and has the same
  exposure, so matching it is drop-in behaviour rather than a
  regression. `DEFAULT_STORE_IS_LIBPINYIN_DBM` names exactly these two.
- For **redb and LMDB** there is no parity argument: they are
  oxpinyin-only containers, and their exposure is oxpinyin's alone.
- redb's is the only one a wrapper could contain at all — a panic can be
  caught where a SIGSEGV cannot — and doing so would change what the
  seam promises.

**This is a STOP.** The options are at least: (a) record the exposure as
a class (c) limit shared with upstream and say so in the public docs;
(b) contain redb's panic at the seam and leave the mmap'd peers as they
are; (c) treat corrupted-file robustness as a peer-selection criterion.
Each changes what the store seam promises, so each needs a maintainer
decision before a lane can require it.

Until then:

- `store-open` runs **unseeded** wherever it runs, which is honest
  coverage of the open-time rejection path and a boot check for the
  harness, and is green on every peer (§5).
- `store-backends.yml`'s `store-file-fuzz` job gates on the one thing
  that *can* be gated today: that the fuzz workspace still builds under
  a non-default peer, that the LMDB instrumentation recipe in §1 still
  links, and that an unseeded instrumented run stays clean. Its comment
  block says the same thing at the point someone would extend it.
- The seeding script exists, is documented, and is deliberately not a
  CI step. `tools/store/seed-store-fuzz-corpus.sh` says so at the top,
  so nobody wires it into a lane without reading this.

## 4. Reproducing

No captures are committed (`docs/runbooks/benches.md`: findings
documents commit no captures). Kyoto Cabinet and LMDB reproduce from
committed inputs alone, in a `debian:testing` container with the fuzz
lane's pinned nightly and cargo-fuzz 0.13.2:

```sh
tools/store/seed-store-fuzz-corpus.sh kyotocabinet
cargo +nightly-2026-08-01 fuzz run store-open \
  --no-default-features --features kyotocabinet -- -max_total_time=120 -timeout=25

tools/store/seed-store-fuzz-corpus.sh lmdb
CC=clang CFLAGS="-fsanitize=address -fsanitize-coverage=inline-8bit-counters,pc-table,trace-cmp -fno-omit-frame-pointer -g" \
  cargo +nightly-2026-08-01 fuzz run store-open \
  --no-default-features --features lmdb -- -max_total_time=120 -timeout=25
```

redb and tkrzw need a denser container than the committed fixtures. The
sweep that produced them built one through `WriteStore::create` (2 000
framed rows plus 2 000 raw rows), spliced one to four runs of up to 32
random bytes per round with half the offsets biased into the first
8 KiB, wrote each result to a fresh path, and drove `open_read_only` /
`open_hash_read_only` followed by `get`, `is_empty`, `range`,
`for_each`, `get_raw`, `range_raw` and `count_raw`. First fault at
corruption 205 (redb) and 342 (tkrzw). That driver was a throwaway and
is not in the tree; its two crash inputs were kept only for the duration
of the investigation and are **not retained** — regenerate them with the
recipe above, or reach the same class by seeding the corpus from a
container written by `oxpinyin-datagen` rather than from the mini
fixtures.

That the crash inputs replay through the landed target
(`cargo fuzz run store-open <file>`) is what ties the sweep to the
harness: the same two files that faulted under the throwaway driver
fault under `store-open`, at `store_open::drive`.

## 5. One thing to watch

`store-open` joins ci.yml's fuzz smoke and verify-nightly's soak
automatically — both derive their target list from `cargo fuzz list`, by
design, so a new target cannot be left out. Both run it under the
default tkrzw and neither seeds its corpus, and an unseeded pass is
green: 397 842 executions in 46 s on tkrzw, 273 646 on redb, 256 647 on
LMDB, 66 217 on Kyoto Cabinet, no findings.

The soak's corpus does persist between nightly runs, so a corpus that
eventually synthesises a valid container from scratch would surface the
§2 class there. That is unlikely — none of these headers is reachable by
mutation from nothing in 180-second increments — and it would be a
genuine finding in the lane built to surface findings, not a
misconfiguration. It is written down here so that whoever sees it first
recognises it.
