# Window-scan performance exploration

Date: 2026-08-14 · Status: **characterization only — no decode-path change.**
Stacked on PR #46 (`feat/candidate-collection-window-scan`, `017a610`).

PR #46's expanding-window scan is the frozen construction
(`candidate-construction.md` §8). This finding records measurements of that
implementation and a recommendation for each of five hypothesized
optimizations. Nothing here is a construction change.

Host: 12 logical cores, one measurement thread unless noted. Release
(`opt-level=3`), rustc 1.97.1. Exported tables from `/tmp/pinyin-rs-export`.
Model cache: `target/model20/extracted/` (SHA-256
`59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155`).
`perf` and `heaptrack` are not installed. CPU profile is Valgrind Callgrind
(`Ir` instruction counts). Allocation profile is `dhat` 0.3. Criterion HTML:
`target/criterion/report/index.html`.

Noise floor: a one-shot serial W2 batch was 1865 ms (instrumented) vs
1990 ms (uninstrumented) on the same process; treat ±10% as run-to-run
noise. Criterion medians below are 20 samples after 500 ms warmup.

## Baseline (serial, one thread)

| Work | Result |
|---|---|
| W2 batch `type_pinyin` (10 465 inputs) | 1.87–1.99 s |
| 20-input per-keystroke cycle | 38.4 ms Criterion (38.3–38.7 ms) / 53 ms one-shot |
| `prefix_probe` × 24 isolated needles | 4.22 µs ⇒ **176 ns/probe** |
| `parse_interpolation2` | **18.8 ms** warm (Criterion); 24.8 ms first open, 18.3 ms second |
| Full table load (redb + prefix tables + interp) | 320 ms |
| `interpolation2.text` size | **83 457 181 bytes**, 63 907 records — not the ~1 MB the prompt guessed |

The parallel 0.5 s wall-clock quoted on PR #46 is the *test harness*
(`thread::scope`, 12 workers). These benches are serial so a future
optimization's delta is not mixed with scheduling.

## A. FST for the two prefix tables

**Hypothesis.** `fst` replaces `pinyin_keys` / `initial_keys`
(`Box<[String]>`, binary-searched in `prefix_probe`) with a byte-DAG:
O(|prefix|) probes and 2–5× less RAM from shared-prefix collapse.

**Measurement.**

- Call count, one serial W2 batch: **24 551** `phrase_prefix_exists` calls
  (2.35 per input).
- Wall share, Instant-wrapped: 30.3 ms / 1865 ms = **1.62%**. Instant
  overhead is included, so the true share is lower. Isolated 176 ns ×
  24 551 = **4.3 ms ⇒ 0.23%** of the uninstrumented 1.99 s batch.
- Resident heap of the two tables (slice of `String` headers +
  `capacity()`): **3 360 018 + 1 520 264 = 4 880 282 bytes** (93 349 +
  45 404 keys).
- Scratch-crate prototype (`/tmp/pinyin-fst-proto`, `fst` 0.4.7, not in
  this tree): FST image **647 220 + 186 646 = 833 866 bytes** (5.8×
  smaller). Correctness: 20 needles agree with `prefix_probe` on both
  tables. Throughput, 800 000 probes: binary **169 ns/op**, FST
  **187 ns/op**, ratio **1.10× slower**. Not tuned, per the STOP rule.

Callgrind (50× the 20-input keystroke cycle, 15.6e9 `Ir`) does not list
`prefix_probe` as its own function — it inlines. `LookupTable::get` is
14.1% of `Ir`; `phrase_prefix_exists` does not appear above 0.1%.

**Recommendation: skip.** The probe is already ~176 ns and under a
quarter-percent of a serial parity run. An FST would save ~4 MB RSS and
would *lose* ~10% probe throughput in the prototype. Not worth a runtime
dependency.

**Follow-up scope if someone still wants the RAM.** Isolated crate swap
behind `phrase_prefix_exists`; risk is low if the 85+20 matrix tables stay
untouched, but the measured speedup is negative.

## B. `SmallVec<[T; 16]>` on scan scratch paths

**Hypothesis.** `Vec::with_capacity(16)` per window (`collect_window_scan`)
and the fallback `walk` vecs heap-allocate; `SmallVec` would keep the
common case on the stack.

**Measurement.**

- Path length is capped at `MAX_PHRASE_LENGTH = 16` (`visit_scan_key`
  stops recursing at 16). **Spill rate past 16 is 0 by construction.**
- `walk` (`session.rs` ~697) runs only on the *pre-frequency* path. With
  real unigrams it is not on the pinned construction.
- dhat, one W2 batch: 12 242 977 blocks / 1.41 GB lifetime. Sites whose
  average size is 32 bytes (16 × `size_of::<SyllableKey>()`) attributed to
  `type_pinyin` / `scan_paths` sum to about **6.8e4 blocks / 2.2 MB**. That
  is **0.6% of blocks**, not in the top 20 by count. Top-by-count is
  `append_scan_entries` / `LookupTable::get` at **1.33 M blocks each**.

**Recommendation: skip.** Zero spill, and the site is not an allocation
hotspot. A `smallvec` runtime dep would buy tens of thousands of 32-byte
allocs on a run that already does 12 M allocations elsewhere.

## C. `Cow<'a, str>` for candidate text

**Hypothesis.** `SystemDictionary::lookup` could return `&str` into stored
`Box<str>`, and `Candidate` could hold `Cow::Borrowed`. The
`impl Dictionary for &D` blanket (PR #32) is supposed to make the
lifetime story cheap.

**Measurement.**

- One W2 batch: **72 186** lookups, **1 373 330** returned entries.
- dhat: `append_scan_entries` clones each text (**1 332 549** blocks,
  4.0 MB — mean ~3 bytes, i.e. one CJK scalar). `LookupTable::get`
  produces the same 1.33 M block count (redb value copy + `from_utf8`).
  Growing `Vec<PhraseEntry>` is 119 MB lifetime; growing `Vec<Candidate>`
  is 245 MB lifetime. Those two *vectors* dominate bytes, not the
  three-byte string buffers.
- `LookupTable::get` returns an **owned `Vec<u8>`** because the redb read
  transaction ends inside the call. There is no long-lived `Box<str>` to
  borrow today. `Cow::Borrowed` therefore requires a new phrase-text cache
  (or a session-scoped redb txn), not just a type change on `Candidate`.
- API churn: `PhraseEntry.text: String` is in `pinyin-core`;
  `Dictionary::Entry` is an associated type; `Candidate`, `CandidateList`,
  `Session::select` (already `to_owned`s the text into `selected`), and
  every fixture adapter would grow a lifetime. The blanket `&D` impl
  helps callers pass a reference; it does **not** create a place to
  borrow phrase bytes from.

**Recommendation: skip** as specified (a `Cow` on top of today's
`LookupTable`). A later intern/cache of phrase text could cut 1.3 M
allocator hits; that is a different PR and a different hypothesis.

## D. `bstr` + `memchr` + `memmap2` for `parse_interpolation2`

**Hypothesis.** The file is ~1 MB of ASCII; Gallant tooling would skip
UTF-8 checks, vectorize delimiters, and avoid a read-buffer copy.

**Measurement.** The file is **83.5 MB**, 63 907 `\1-gram` records.
`parse_interpolation2` is **18.8 ms** warm / 24.8 ms cold. That is **6–8%
of the 320 ms** full table load (redb open + prefix-table build + parse).
Callgrind attributes **0.11%** of a 50× keystroke run to
`parse_interpolation2` (one parse at startup). The parity harness builds
the model **once per process** (workers then borrow it); a CLI invocation
does the same.

A 3× parse speedup would save ~12 ms once per process.

**Recommendation: skip.** One-shot 19 ms on an 83 MB file is already
fine; mmap/bstr would add three runtime deps for a 12 ms startup that is
drowned by redb.

## E. `SystemDictionary::unigrams: BTreeMap<u32, u64>`

**Not dead.** It is written in `open` via `build_unigram_map` and **read**
by `unigram_count` and `unigram_map`. `BigramLanguageModel::set_unigrams_from_dict`
clones that map for the export-ABI (flat-100) interpolation path.
`parity_sweep` and `parity_worst` still call `set_unigrams_from_dict`.
The interpolation2 path sets `real_unigrams = true` and does not consult
this BTreeMap, but the field remains the only source of export-ABI
unigrams. **No deletion in this PR.**

## CPU profile (Callgrind, 50 × 20-input keystroke cycle)

Tool: Valgrind 3.26.0 Callgrind, `Ir` only (no cache simulation). 15.65
billion instructions. `perf` / `cargo-flamegraph` were not available.
dhat's global allocator is compiled into this bench binary, so
`dhat::Alloc` / `malloc` appear; they would be smaller with the system
allocator.

Top functions by self-`Ir` (decode-dominated; load is <1% after 50
repeats):

| Share | Function |
|---|---|
| 15.8% | `memcmp` (redb key compare + string/sort) |
| 14.1% | `LookupTable::get` |
| 7.7% | `malloc` |
| 5.4% | `memcpy` |
| 4.9% | `free` |
| 4.1% | `BigramLanguageModel::unigram_freq` |
| 2.8% | `SegmentGraph::build` |
| 2.95% | `HashMap::contains_key` (`dedup_by_text_keep_first`) |

`prefix_probe` is inlined and not a listed site. `parse_interpolation2`
is 0.11%. The hot path is **phrase-table get + allocator + ranking**, not
the prefix tables, the path `Vec`, or the interp parser.

## Allocation profile (dhat, one serial W2 batch)

Tool: `dhat` 0.3, `cargo bench -p pinyin-oracle --bench alloc_profile -- --dhat`.
Totals: **1 414 276 456 bytes in 12 242 977 blocks**; peak 78.8 MB /
787 298 blocks; 0 at exit.

Top 5 by **block count**:

| Blocks | Bytes | Site |
|---|---|---|
| 1 332 549 | 4.0 MB | `append_scan_entries` (candidate `String`) |
| 1 332 549 | 4.0 MB | `LookupTable::get` (value copy) |
| 1 145 722 | 2.9 MB | corpus `FullPinyinParser::parse` (input load, not decode) |
| 1 018 337 | 2.6 MB | corpus parse clone |
| 1 018 337 | 2.6 MB | corpus parse clone |

Top 5 by **lifetime bytes**:

| Bytes | Blocks | Site |
|---|---|---|
| 245 MB | 56 156 | `Vec<Candidate>` growth in `append_scan_entries` |
| 215 MB | 18 682 | `Vec<(RankKey, Candidate)>` in `refresh` |
| 135 MB | 67 795 | `k_best_to` (still invoked before the scan) |
| 119 MB | 8 843 | stable sort scratch for the three-key order |
| 119 MB | 77 111 | `Vec<PhraseEntry>` growth in `lookup` |

The path `Vec<SyllableKey>` (candidate B) is not in either top-5.

## Suggested implementation order

None of A–E is worth a follow-up PR on this evidence.

1. **Do not ship FST, SmallVec, Cow, bstr/mmap, or a BTreeMap deletion.**
2. If a later PR hunts allocator traffic, the first real lead is **not**
   in the five: `LookupTable::get` + `append_scan_entries` (1.3 M tiny
   strings) and `Vec<Candidate>` growth. That is a phrase-text cache /
   reuse-the-output-vec design, not `Cow` on today's redb copies.
3. A second, independent lead — also outside the five — is that
   `refresh` still runs `k_best` on the real-unigram path (135 MB
   lifetime, 0.14% `Ir`). Dropping that call is a construction-adjacent
   change and needs its own SPEC note; it is not started here.

The current implementation is already fast enough that A–E are not worth
the complexity: serial W2 is ~2 s on one core, prefix probes are 0.23% of
that, interp parse is 19 ms once, and the path `Vec` is a rounding error
in a 12 M-block allocation profile.

## Harness

- `crates/pinyin-oracle/benches/scan_perf.rs` — Criterion groups
  `keystroke_cycle_20_inputs`, `prefix_probe_12_needles`,
  `parse_interpolation2`. `cargo bench -p pinyin-oracle --bench scan_perf`.
- `crates/pinyin-oracle/benches/alloc_profile.rs` — one-shot counts,
  `--dhat` heap dump, `--keystroke-only --repeats N` for Callgrind.
- `crates/pinyin-oracle/benches/harness.rs` — shared loaders. Refuses to
  start without the export dir and the fetched model.
