# Decode-path criterion baseline — first run (2026-09-19)

**First baseline.** Nothing earlier measures these two targets. There is
no before/after, no delta, and no comparison cell. The figures below are
what this host produced on this commit; they are not a claim about any
other machine or any later tip.

The two criterion targets landed with PR #470
(`oxpinyin-core --bench parse_and_graph`,
`oxpinyin-engine --bench session_keystroke`) and had never been executed
before this record. Every other criterion target in the workspace already
had a `docs/perf/` or `docs/findings/perf-*` note; these did not.

## Environment

| | |
| --- | --- |
| Measurement date (UTC) | **2026-09-19** (`date -u` at the run; wall clock spanned 2026-09-18T23:48Z–2026-09-19T00:12Z) |
| Commit | `3965f3686cce1c95f1c135f086b6aaa1fede5218` (`origin/main` tip at measurement) |
| Image | `debian:testing`, full digest **`debian@sha256:dab11cdb0a9dcf4bbd68f671635b35f1f726b452b92396875b69bb2c7daa42a9`** |
| Container ID | `ac99545ff18d0636b790da076b8a45b53eae2cb7d44ad79186a35c4eae7d6843` |
| Image ID (local) | `sha256:5d177234994493ff9cb408140e5029810042abd22a190091a25e0cee2ceb6f6d` |
| Guest | Debian forky/sid, `Linux 6.12.94+ x86_64` |
| Host CPU | `Intel(R) Xeon(R) Processor` (KVM hypervisor; 4 vCPU visible on the host; measurement pinned to one) |
| Pin | container `--cpuset-cpus 1` and in-container `taskset -c 1` |
| Toolchain | `rust-toolchain.toml` → **1.97.1** (`rustc 1.97.1 (8bab26f4f 2026-07-14)`, `cargo 1.97.1 (c980f4866 2026-06-30)`), rustup `--profile minimal` inside the container |
| Harness | **criterion 0.8.2** (Cargo.lock), crate-local `[[bench]]` targets, criterion defaults (100 samples / 3 s warmup / ~5 s measurement) |
| Host idle? | **Mostly.** At measurement start loadavg was `0.50 0.34 0.15` on 4 CPUs; top non-bench processes were a light XFCE/VNC desktop session and dockerd (single-digit %CPU). No competing `cargo`/`rustc` job. Load rose to ~1.0 while the pinned bench itself ran. |

Raw criterion stdout lived under `/tmp/oxp-bench-out/` on the measurement
host for this run. That tree is ephemeral cloud-agent state and is **not**
retained in the repository; the numbers below are what the two runs
printed.

## Build recipe

Timed artifacts are the **criterion bench binaries** produced by
`cargo bench` under Cargo's `bench` profile, which inherits this
workspace's `[profile.release]` (`lto = "fat"`, `codegen-units = 1`).
They are **not** the shipping `cargo cinstall` `libpinyin.so.15.0.0`,
not a `cargo build --release` + `strip` bisection fixture, and not the
`profiling` profile used by `tools/profile/run-w8-cycle.sh`.

`oxpinyin-core`'s and `oxpinyin-engine`'s benches do not open a store.
Their packages nevertheless pull `oxpinyin-testsupport` as a
dev-dependency, and that crate edges `oxpinyin-store` with
`default-features = false` and no backend of its own. A bare
`cargo bench -p oxpinyin-core --bench parse_and_graph` (or the engine
equivalent) therefore fails the store's "exactly one backend" compile
gate. Feature-unifying `oxpinyin-testsupport/redb` is enough to compile
the unused edge; redb does not enter the timed path. The same pattern
appears in `docs/findings/scan-matrix-fanout-2026-09-10.md` for the
engine diagnostic.

Under `lto = "fat"` with `codegen-units = 1` the whole graph is one
optimisation unit, so the backend chosen to satisfy the store gate is
part of this record's build recipe even though it is off the timed
path. A re-run must use `oxpinyin-testsupport/redb`; a different
backend produces a differently-optimised binary and numbers that are
not comparable to these.

### Verbatim command lines

```sh
taskset -c 1 cargo bench -p oxpinyin-core -p oxpinyin-testsupport \
  --features oxpinyin-testsupport/redb --bench parse_and_graph

taskset -c 1 cargo bench -p oxpinyin-engine -p oxpinyin-testsupport \
  --features oxpinyin-testsupport/redb --bench session_keystroke
```

Each target was run twice, back-to-back, on the same container, same
binary, same pin. Criterion's change-vs-baseline lines on run 2 are
run-to-run noise against run 1's saved baseline, not a code change.

## Window-scan branch (engine)

Before trusting any engine number, the production decode branch was
confirmed — not assumed:

1. **Bench setup assert.** `fixture_session()` in
   `crates/oxpinyin-engine/benches/session_keystroke.rs` builds a
   [`FrequencyFixtureModel`](../../crates/oxpinyin-testsupport/src/fixture.rs),
   which hard-codes `has_real_unigrams() == true`, and asserts that before
   constructing the session. That is the same gate
   `Session::refresh` uses to choose `collect_window_scan` over the
   pre-frequency k-best fallback (`crates/oxpinyin-engine/src/session/lookup.rs`).
   Both full runs completed without panic, so the assert held on every
   timed iteration that builds a fixture session.
2. **Runtime probe on the same model and fixtures.** A one-shot release
   probe against the same vocab/bigram fixtures typed `ni` and observed
   Phrase candidates `你, 尼, 呢, 泥, 妮, 拟, 逆, 倪, 腻, 溺` — the
   fixture's `keys=ni` rows in token-ascending order, which is the
   window-scan flush order. `window_scan_branch=confirmed`.

Groups that do **not** take the window-scan path, and must not be read as
decode-path numbers:

- `engine_session_new/*` — times `Session::new` only. `empty_backends`
  uses empty stubs; `fixture_backends` uses plain
  `FixtureLanguageModel` (`has_real_unigrams() == false`).

## What each bench covers (and does not)

### `parse_and_graph` (`oxpinyin-core`)

Pure functions over the frozen syllable inventory: full-pinyin parse,
parse-with-options, segment-graph build, fewest-keys, k-best, syllable-key
lookup, `expand_keys`, fuzzy alternatives. **No I/O, no dictionary, no
language model, no session, no window scan.**

### `session_keystroke` (`oxpinyin-engine`)

End-to-end public session API over the W4 mini fixtures
(`fixtures/w4/mini-vocab.txt`, `mini-bigram.txt`):

- keystroke / composition / incremental / backspace groups →
  `FrequencyFixtureModel` → **production window-scan + frequency ranking**
  on a tiny authored vocabulary, not model20 and not a system table.
- session-construction group → stubs / non-frequency fixture (see above).

**Not covered:** C-ABI surface, shipping `.so`, real unigram archive,
bigram DBM backends, user-store train/commit, oracle parity.

## Results

Criterion mean (the middle of the reported `[lo mid hi]` interval). Spread
is `|run1 − run2| / mean(run1, run2)`. Criterion did **not** report any
target below its own resolution; every ID below has a recorded figure.

### `parse_and_graph`

| Bench ID | Run 1 mean | Run 2 mean | Spread |
|---|---:|---:|---:|
| `core_parse/parse/short_ni` | 345.37 ns | 291.43 ns | 16.9% |
| `core_parse/parse/medium_nihao` | 1.0076 µs | 1.0860 µs | 7.5% |
| `core_parse/parse/long_nihaoshijie` | 5.2056 µs | 5.2616 µs | 1.1% |
| `core_parse/parse/ambiguous_xian` | 776.57 ns | 828.14 ns | 6.4% |
| `core_parse/parse/ambiguous_fangan` | 1.6189 µs | 1.6503 µs | 1.9% |
| `core_parse/parse/separated` | 563.04 ns | 667.93 ns | 17.0% |
| `core_parse/parse/partial_nih` | 4.9347 µs | 4.4317 µs | 10.7% |
| `core_parse/parse/initials_only` | 5.8181 µs | 5.6931 µs | 2.2% |
| `core_parse_with_options/options/medium_nihao` | 1.0511 µs | 988.39 ns | 6.1% |
| `core_parse_with_options/options/long_nihaoshijie` | 5.2092 µs | 4.3234 µs | 18.6% |
| `core_parse_with_options/options/ambiguous_xian` | 735.18 ns | 774.66 ns | 5.2% |
| `core_graph_build/build/short_ni` | 205.27 ns | 205.68 ns | 0.2% |
| `core_graph_build/build/medium_nihao` | 872.72 ns | 945.93 ns | 8.1% |
| `core_graph_build/build/long_nihaoshijie` | 3.6206 µs | 3.9356 µs | 8.3% |
| `core_graph_build/build/sentence` | 8.7644 µs | 8.8317 µs | 0.8% |
| `core_graph_build/build/initials_zz` | 2.6953 µs | 3.0415 µs | 12.1% |
| `core_graph_build/build/separated` | 1.6215 µs | 1.6706 µs | 3.0% |
| `core_graph_build_with_options/options/medium_nihao` | 866.84 ns | 854.54 ns | 1.4% |
| `core_graph_build_with_options/options/sentence` | 8.7715 µs | 8.6093 µs | 1.9% |
| `core_fewest_keys/complete_only/medium_nihao` | 39.221 ns | 39.045 ns | 0.4% |
| `core_fewest_keys/allow_incomplete/medium_nihao` | 43.320 ns | 42.780 ns | 1.3% |
| `core_fewest_keys/complete_only/long_nihaoshijie` | 54.727 ns | 52.853 ns | 3.5% |
| `core_fewest_keys/allow_incomplete/long_nihaoshijie` | 63.221 ns | 61.822 ns | 2.2% |
| `core_fewest_keys/complete_only/sentence` | 125.20 ns | 126.51 ns | 1.0% |
| `core_fewest_keys/allow_incomplete/sentence` | 148.45 ns | 146.30 ns | 1.5% |
| `core_fewest_keys/complete_only/ambiguous_xian` | 32.862 ns | 34.696 ns | 5.4% |
| `core_fewest_keys/allow_incomplete/ambiguous_xian` | 34.608 ns | 34.397 ns | 0.6% |
| `core_k_best/k_best/nihao_k1` | 598.24 ns | 602.39 ns | 0.7% |
| `core_k_best/k_best/nihao_k8` | 664.68 ns | 676.27 ns | 1.7% |
| `core_k_best/k_best/nihaoshijie_k1` | 1.3652 µs | 1.3173 µs | 3.6% |
| `core_k_best/k_best/nihaoshijie_k8` | 2.0546 µs | 1.7789 µs | 14.4% |
| `core_k_best/k_best/sentence_k1` | 3.3322 µs | 3.0545 µs | 8.7% |
| `core_k_best/k_best/sentence_k8` | 8.8243 µs | 7.9708 µs | 10.2% |
| `core_k_best/k_best/ambiguous_fangan_k8` | 1.8136 µs | 1.6319 µs | 10.5% |
| `core_k_best/k_best/xian_k4` | 619.64 ns | 641.13 ns | 3.4% |
| `core_syllable_key/from_text_hit` | 155.15 ns | 146.32 ns | 5.9% |
| `core_syllable_key/from_text_miss` | 140.83 ns | 137.36 ns | 2.5% |
| `core_syllable_key/from_option_text_alias` | 529.65 ns | 496.34 ns | 6.5% |
| `core_expand_keys/complete_nihao` | 115.53 ns | 106.05 ns | 8.6% |
| `core_expand_keys/one_incomplete_nih` | 14.170 µs | 14.296 µs | 0.9% |
| `core_expand_keys/two_incomplete_hh` | 29.498 µs | 27.971 µs | 5.3% |
| `core_fuzzy/alternatives_all_amb` | 987.67 ns | 940.85 ns | 4.9% |

### `session_keystroke`

| Bench ID | Run 1 mean | Run 2 mean | Spread | Path |
|---|---:|---:|---:|---|
| `engine_session_new/empty_backends` | 627.98 ns | 697.70 ns | 10.5% | construction only |
| `engine_session_new/fixture_backends` | 21.431 µs | 22.916 µs | 6.7% | construction only |
| `engine_process_key/first_char_n` | 18.715 µs | 19.753 µs | 5.4% | window-scan |
| `engine_process_key/completing_nihao` | 4.3231 µs | 4.7281 µs | 8.9% | window-scan |
| `engine_full_composition/compose/short_ni` | 20.880 µs | 21.021 µs | 0.7% | window-scan |
| `engine_full_composition/compose/medium_nihao` | 42.040 µs | 43.384 µs | 3.1% | window-scan |
| `engine_full_composition/compose/long_nihaoshijie` | 111.25 µs | 115.91 µs | 4.1% | window-scan |
| `engine_incremental_typing/nihao_5_keystrokes` | 42.166 µs | 42.548 µs | 0.9% | window-scan |
| `engine_incremental_typing/zhongguoren_11_keystrokes` | 201.54 µs | 201.07 µs | 0.2% | window-scan |
| `engine_backspace/erase_last_of_nihao` | 4.2920 µs | 4.0643 µs | 5.4% | window-scan |

Across all 52 IDs: median run-to-run spread **4.9%**, mean **5.4%**.
Engine window-scan IDs alone: mean spread **3.6%**, max **8.9%**.
Several sub-µs core IDs land above 10% spread (worst
`core_parse_with_options/options/long_nihaoshijie` at 18.6%).

## Ratchet fit (report only — nothing wired)

Does **not** fit the adopted direction in
[`ci-perf-size-gate-proposal-2026-09-09.md`](ci-perf-size-gate-proposal-2026-09-09.md).
That document rejected per-PR gates and adopted a nightly snapshot series
that deliberately **does not record wall clock** (table: wall clock →
record no, flag no). These benches are criterion wall-clock means. Putting
them on a PR gate would revive the rejected proposal; putting them on the
nightly series would contradict the series' own instrument list
(section byte sums, stripped size, allocs, callgrind Ir — not criterion
µs).

Stability enough for a *manual* canary later, on the same container and
host only:

- **Yes, loosely:** `engine_incremental_typing/zhongguoren_11_keystrokes`
  (~201 µs, 0.2% spread) and `engine_full_composition/compose/short_ni`
  (~21 µs, 0.7% spread) are the quietest window-scan IDs here.
- **No, for a hard threshold:** core sub-µs IDs and
  `engine_process_key/completing_nihao` already move 9–19% run-to-run on
  a quiet machine — inside the 9–11% whole-session offset the proposal
  cites from the cross-host steady-cycle record.

Cost of making any of them a ratchet later: a second measurement lane
(or extending `tools/perf-gate/snapshot.sh`) that pins image-by-digest
against the repo's deliberate unpinned `debian:testing` policy, stores
series outside the tree, and still cannot defend a PR-path comparison —
the reasons the proposal was rejected. Not attempted here.
