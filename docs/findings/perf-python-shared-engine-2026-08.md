# Python binding: shared `Engine` vs one `Engine` per thread (2026-08)

Date: 2026-08-27 · Status: **measurement only — nothing implemented, no
Python or Rust surface changed by this document.** Written for review #179
(item D1) to answer one question with numbers instead of intuition: what
does a caller actually get today by sharing one `Engine` across threads,
and what would a `Runtime`/`Session` split be worth?

Measured at `f2eedd7` on branch `feat/python-api`.

## The design as it stands

`Engine` is one `Runtime` plus one `Session` behind a single
`Arc<Mutex<EngineInner>>`. Every call takes that mutex, so N threads calling
`lookup()` on one engine execute strictly one at a time. The alternative is
already latent in the crate: `Runtime` is `Send + Sync` (there is a
compile-time assertion for it in `oxpinyin-runtime`), `new_session()`
exists, and sessions share the runtime's table handles through `Arc`. Today
the only route to concurrent lookups is N `Engine`s, and each of those
reopens the redb tables.

## Host and build

Intel(R) Xeon(R) @ 2.10 GHz, **4 logical CPUs**, 16 GiB RAM, Linux
6.18.44. rustc 1.97.1 (the pinned toolchain), PyO3 0.29.2, maturin 1.15.0.
The extension is a maturin PEP 517 wheel build, i.e. a release build. No
CPU pinning (unlike `perf-baseline-2026-08.md`, which uses `taskset`);
figures are best-of-5 minimum wall time, which is the robust statistic here.

Two interpreters, the second as a control:

| Interpreter | `Py_GIL_DISABLED` | `sys._is_gil_enabled()` |
|---|---|---|
| CPython 3.15.0rc1 free-threading build | 1 | False |
| CPython 3.13.12 (stock, control) | 0 | True |

Data: `fixtures/w3`, the committed 2.2 MB mini fixture, opened through
`Engine.from_fixture_dir` (flat unigrams derived from the phrase index).
**This is fixture scale, not production scale** — see Caveats.

## Method

Workload is `engine.lookup("nihao")`: reset, type the batch, snapshot the
candidate list, all inside one locked call. Each of N threads runs 500
lookups. Threads are released together by a `threading.Barrier` and the
timed region is barrier-to-barrier, so thread startup is outside the
measurement; every engine is warmed with one lookup first. Both modes do
the same total work (N × 500 lookups):

- **shared** — one `Engine`, handed to all N threads.
- **private** — N `Engine`s, one per thread.

Single-threaded reference: one `lookup` costs 1548 µs on 3.15t, 1534 µs on
3.13. Opening an `Engine` costs a median 0.826 ms on 3.15t, 0.815 ms on
3.13 (30 samples).

## Result 1 — a shared `Engine` does not scale at all

Free-threaded 3.15.0rc1:

| threads | shared (lookups/s) | private (lookups/s) | private ÷ shared | shared scaling | private scaling |
|---:|---:|---:|---:|---:|---:|
| 1 | 659 | 657 | 1.00× | 1.00× | 1.00× |
| 2 | 641 | 1292 | 2.02× | 0.97× | 1.97× |
| 4 | 640 | 2538 | 3.97× | 0.97× | 3.86× |
| 8 | 634 | 2620 | 4.13× | 0.96× | 3.99× |

The shared column is flat. Eight threads over one `Engine` deliver 0.96× of
what one thread delivers — the mutex serialises the work completely, and the
lock traffic makes it marginally worse than not threading at all. Private
engines scale near-linearly to the machine's 4 CPUs and then saturate, which
is the expected shape.

## Result 2 — free-threading is not the lever here

The same measurement on the stock 3.13 GIL build:

| threads | shared (lookups/s) | private (lookups/s) | private ÷ shared | private scaling |
|---:|---:|---:|---:|---:|
| 1 | 648 | 658 | 1.01× | 1.00× |
| 2 | 645 | 1309 | 2.03× | 1.99× |
| 4 | 644 | 2560 | 3.97× | 3.89× |
| 8 | 643 | 2557 | 3.98× | 3.89× |

Within noise of the free-threaded numbers, in both columns.

This is worth stating plainly because it corrects the intuitive framing.
Private engines already scale under a GIL, because the binding runs every
decode inside `Python::detach` — the GIL is released around the engine call,
so CPython was never the thing serialising these lookups. What serialises
them is `Engine`'s own mutex, and it does so identically with or without a
GIL.

So: the free-threaded build is not what would unlock parallelism for this
binding, and a shared `Engine` is not GIL-equivalent-plus-a-lock — it is
*worse* than the GIL build's private-engine throughput by the full factor of
N. The lever is shared-vs-private, not GIL-vs-free-threaded.

(This does not make the free-threaded CI pin pointless: it is what makes
`test_shared_engine_is_thread_safe` actually contend the lock rather than
watch the interpreter serialise the worker loop.)

## Result 3 — what a `Runtime`/`Session` split would save

Measured in Rust directly (release, rustc 1.97.1, 50 samples, median), on
the same fixture:

| Step | Median |
|---|---:|
| `Runtime::open_fixtures` + `new_session` (what one `Engine` costs) | 0.806 ms |
| `Runtime::open_fixtures` alone (the table opens) | 0.478 ms |
| `new_session` over an already-open `Runtime` | 0.313 ms |

Today N private engines pay N × 0.806 ms, of which N × 0.478 ms is the same
tables opened N times. A shared `Runtime` would pay 0.478 ms once and
0.313 ms per session, so about **59% of per-additional-engine init is
redundant today** at this scale.

Directionally that understates it: the fixture's redb files are ~1 MB each,
while the real model20 tables are far larger, so the `Runtime` share of the
open grows with the data and the `Session` share does not. That is a
prediction, not a measurement — this checkout has no real-unigram model to
measure against, so it is offered as the shape of the curve rather than a
number.

## The cost of exposing `Runtime` as a Python object

Not proposed here; recorded so the decision has its price attached.

- **API surface.** Two new Python items (a `Runtime` class and a way to get
  a `Session` from it) on a branch whose rule is to expose only what
  fcitx5 and ibus-libpinyin need. Against that: the split is not novel
  shape, it is *the ABI's own shape*. `pinyin_init` returns a context and
  `pinyin_alloc_instance` returns an instance from it; both are in the 55
  exported C functions. A Python `Runtime` + `Session` maps onto them
  one-for-one, whereas today's `Engine` is the thing with no ABI
  counterpart — it fuses the two.
- **Lifetimes: cheap.** A session does not borrow from the runtime it came
  from. `new_session` hands it `Arc` clones of the dictionary and language
  model handles, so a `Session` outliving its `Runtime` is already sound in
  Rust and needs no back-reference on the Python side to stay safe.
- **`Engine` does not have to go.** It is the right shape for the
  batch-query workflow — `lookup()` is one locked call precisely so that a
  single-threaded caller needs no locking of their own. A `Runtime` could be
  added beside it for callers that want per-thread sessions, leaving the
  simple case simple.
- **Ordering risk: none identified.** Sessions built from one runtime share
  the same backends and the same defaults-only configuration, so this is a
  concurrency change, not a ranking change. Any implementation would still
  have to prove that through the parity corpus.

## Caveats

- **Fixture scale.** `fixtures/w3` is a 2.2 MB mini fixture in fixture mode.
  Absolute throughput here is not production throughput. What this measures
  is the *ratio* between shared and private, and that ratio is a property of
  the mutex, not of the data size — which is why it is identical across two
  interpreters and every thread count.
- **4 CPUs.** n=8 oversubscribes, which is why the private column saturates
  between n=4 and n=8 rather than continuing to climb.
- **No CPU pinning**, unlike the W8 baseline. Best-of-5 minimum absorbs
  most of the scheduling noise; the shared column's flatness across 1–8
  threads is far outside any plausible noise band.

## What this does not conclude

Whether to do the split. That is a maintainer decision about API surface,
and this document deliberately stops at the measurements it was asked for.
