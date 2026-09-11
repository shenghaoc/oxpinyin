# Scan-matrix path fan-out attribution — probe-time expansion (2026-09-10 UTC)

Follow-up to [`rss-attribution-2026-09-09.md`](rss-attribution-2026-09-09.md) §"Characterising the 3.07× — it is breadth" and
[shenghaoc/oxpinyin#403](https://github.com/shenghaoc/oxpinyin/issues/403).
Answers the issue's open question mechanistically. **No fix is applied
here**; the mechanism identification is behaviour-inert against the
parity gate only after a user-store change that this session does not
have the environment to validate.

## The finding

**oxpinyin's `search_scan_path` inflates every incomplete-containing key-path into the Cartesian product of that path's completions before probing, while the DBM already stores each phrase under its incomplete-index and would answer the raw path in one probe.** The expansion is the redundant work.

The two sites, side by side:

| step | oxpinyin | libpinyin pin `074a2219` |
|---|---|---|
| scan visits a complete key-path with an incomplete key | `crates/oxpinyin-engine/src/session/lookup.rs:1025-1040` — `expand_keys(path, SCAN_EXPANSION_LIMIT)` returns the Cartesian product of every incomplete syllable's `phonetic_initial` completions; each expansion is one `lookup_and_append` call | `src/storage/chewing_large_table2.cpp:161-172` — `ChewingLargeTable2::search` picks the incomplete or complete index by `contains_incomplete_pinyin(keys)` and issues a single `search_internal(phrase_length, index, keys, ranges)` |
| DBM writes | `crates/oxpinyin-data/src/table_entries.rs:87-113` — each row is written under **both** the incomplete and complete keyspaces (line 95's `dbm_keys = [encode_incomplete_key(&row.keys), encode_complete_key(&row.keys)]`), mirroring upstream's two-space add | `src/storage/chewing_large_table2.cpp:184-197` — `add_index` calls `add_index_internal` for both `compute_incomplete_chewing_index` and `compute_chewing_index` |
| DBM reads | `crates/oxpinyin-data/src/chewing_table.rs:202-216` — `ChewingTable::search` uses `index_key(keys)` which routes to `encode_incomplete_key` when the query has any partial syllable, then one `dbm.get`; matches are refined by `keys_match` (partial syllables accept every final) | one `search_internal`, same shape |

The datagen-layer symmetry (both engines double-index at build time) plus
the DBM-layer symmetry (both accept incomplete queries by routing to the
incomplete keyspace) says the fan-out is **not** a data-layer difference.
It is [`search_scan_path`](../../crates/oxpinyin-engine/src/session/lookup.rs)'s scan-time
policy: `if has_incomplete { expand_keys(...) } else { path }`. Remove
the branch and the DBM does the right thing in one probe, same as
upstream.

## Method

A gated diagnostic test measures oxpinyin's scan matrix and its
post-`expand_keys` probe count over the RSS report's frozen 20-input
corpus, keystroke-by-keystroke. Compiled and run on macOS with the
`redb` backend; the counts do not depend on the backend (the matrix
walk is a pure function of the graph, and `expand_keys` reads no DBM).

**Command** (from the worktree root):

```bash
cargo test -p oxpinyin-engine --features 'diagnostic_403 oxpinyin-testsupport/redb' --release -- diagnostic_403_scan_matrix_fanout --nocapture --test-threads=1
```

Feature `diagnostic_403` is off by default (see
[`crates/oxpinyin-engine/Cargo.toml`](../../crates/oxpinyin-engine/Cargo.toml)); the test asserts nothing and only prints
`[403]`-tagged rows plus one `[403-JSON]` blob.

**Two caveats to read the numbers with:**

1. The diagnostic is a **matrix-only enumeration** — it builds a graph
   and a scan matrix per keystroke, walks every end position in
   `1..=graph.consumed()`, and counts paths and post-`expand_keys`
   probes without any DBM. Real widening from
   [`collect_window_scan`](../../crates/oxpinyin-engine/src/session/lookup.rs) breaks early
   on `!continued` (matrix column empty, no overhang, no prefix probe
   hit), which cuts iterations off; real widening also keeps going while
   `phrase_prefix_exists` is true, which walks past matrix-column
   emptiness the diagnostic cannot re-enter. **These are two different
   aggregates and are not directly comparable at the ABSOLUTE level.**
   What is comparable is the ratio `probes_per_window` (the diagnostic
   sees ~3.97; the RSS callgrind saw ~4.11) and the mechanism the
   `expand_keys` inflation names. The 581-vs-1,038 gap is left
   unexplained here and does not carry any conclusion.
2. Sequence keys are `(text, tone)`; parity has `USE_TONE` off, so tone
   is always 0 and the key reduces to the syllable text. This is the
   same shape `ChewingTable::search`'s incomplete-index consumes.

**Aggregate over 123 keystrokes:**

| metric | this diagnostic | RSS-report callgrind (oxpinyin) |
|---|---:|---:|
| keystrokes | 123 | 123 |
| windows | 581 | 1,038 |
| matrix key-paths | 256 | — |
| post-`expand_keys` probes | 2,308 | 4,262 |
| probes / window | 3.97 | 4.11 |
| unique probes | 1,676 | — |
| unique / probes | 0.726 | — |
| matrix entries (sum over all cycles) | 245 | — |

Absolute totals differ from the RSS numbers because the two harnesses
measure different aggregates (see caveat 1). The load-bearing comparison
is `probes_per_window` — 3.97 here against RSS's 4.11 — and the
**9.0× jump from matrix paths (256) to probes (2,308)** for this
harness alone, which isolates the effect of `expand_keys`.

**Probe histogram — the memoization question the RSS report calls out:**

| distinct sequence appeared | 1× | 2× | 3× | 4× | ≥5× |
|---|---:|---:|---:|---:|---:|
| probe key-sequences | 1,368 | 178 | 31 | 76 | 23 |

Unique-to-total probe ratio is 0.726: **72.6 % of probes hit a syllable-key sequence no earlier probe used**. Memoization headroom is bounded by 1 − 0.726 = **27.4 %**, and only 23 sequences appear five or more times, so the tail is short. Memoization is a small effect; the fan-out itself is the big one.

## Ruled out

- **Extra keys in the scan matrix.** From source:
  [`build_scan_matrix`](../../crates/oxpinyin-engine/src/session/mod.rs) applies the
  same tables upstream applies (RESPLIT, DIVIDED, fuzzy) plus a
  key-only dedupe at `crates/oxpinyin-engine/src/session/mod.rs:591` that
  upstream does not; every path that produces oxpinyin's matrix entries
  produces at most upstream's. The measurement dumps
  `matrix_entries_total = 245` over 123 keystroke cycles — ~2 entries
  per prefix graph — consistent with that source read. Matrix column
  contents are not the amplifier.
- **Tone / fuzzy matrix materialisation** — the leading hypothesis in
  the issue text. Parity has `USE_TONE` off and fuzzy off (`docs/findings/option-bits.md` §"Bit values" and `crates/oxpinyin-engine/src/session/mod.rs:589-591`),
  so `fuzzy_additions` and `keep_first_in_column(true)` are no-ops for
  this workload. The measured `matrix_entries_total` confirms — nothing
  here for fuzzy to materialise.

**Note on windows.** The 581-vs-1,038 gap between this diagnostic and
the RSS callgrind is left unexplained; see caveat 1. It does not carry
any ruling one way or the other on window breadth. The mechanism call
rests on the source symmetry above and the 9.0× matrix-to-probe
inflation the same harness measures internally, both of which are
independent of the window-count discrepancy.

## Left open — pin-side unique-key measurement

The RSS report says "settling [memoization] needs a measurement this
pass did not take: the count of *unique* index keys probed per window on
each side, or a per-key hit count" and specifically asks for both sides.
This diagnostic delivers the **oxpinyin side** only; the **upstream side**
still needs a gated counter inside `search_internal` or
`ChewingTable2::search` (in a rebuild of the pin), and a run against
the same 123-keystroke corpus.

On the oxpinyin side alone the 27.4 % headroom bound is enough to say
memoization is the smaller lever; the pin-side number is not required
to justify the mechanism call above.

## Next action

**The fix is a scan-time change that must not change the surface.**
Removing the `expand_keys` branch in
[`search_scan_path`](../../crates/oxpinyin-engine/src/session/lookup.rs:986-996) is behaviour-preserving
for the **system dictionary** — the DBM already handles the incomplete
query — but it silently drops user-store matches for incomplete
queries because
[`UserLookup::lookup`](../../crates/oxpinyin-user/src/lookup.rs:120-128)
uses an exact-text-only `HashMap`. Its neighbour
[`phrase_prefix_exists`](../../crates/oxpinyin-user/src/lookup.rs:132-144)
already keeps two indices (`initial_keys` and `pinyin_keys`); extending
the `lookup` path to use them is the shape the fix needs.

Concretely, in the order a follow-up PR should take:

1. Add an initial-only index to `UserLookup` (mirror upstream's
   `add_index`'s incomplete-space write; the datagen side already does
   this at `crates/oxpinyin-data/src/table_entries.rs:95-98`).
2. Route `UserLookup::lookup` through it when the query is
   incomplete, so incomplete user-store queries return the same set the
   current `expand_keys` fan-out delivers.
3. Delete the `has_incomplete` branch in `search_scan_path` (~10 lines),
   restoring the single probe per key-path.
4. Run the 10,190-row parity gate on Linux with Kyoto Cabinet and the
   pin-built oracle — see [`docs/testing/parity-corpus.md`](../testing/parity-corpus.md). This session
   cannot run that gate.
5. Re-measure RSS with `perf-cycle` on the same host used for
   [`rss-attribution-2026-09-09.md`](rss-attribution-2026-09-09.md); the expected outcome is
   probes → ~1,536 (matching upstream's callgrind), live blocks →
   ~22,832 (the 40 % addressable RSS term).

Steps 1–3 alone are STOP items until the parity gate has run. This
session records the mechanism and its measurement; the change itself is
someone else's turn under the constitution's rebase discipline and
concurrent-sessions rules.

## Provenance

- Upstream pin: libpinyin **2.11.92**, commit `074a2219c90feaf962d0d24f034514033ece5f99`, fetched into `/tmp/libpinyin-403` via `git fetch --depth=1`.
- Diagnostic test: [`crates/oxpinyin-engine/src/session/tests.rs`](../../crates/oxpinyin-engine/src/session/tests.rs) `diagnostic_403_scan_matrix_fanout`, gated behind the crate's `diagnostic_403` feature (off by default).
- Host: macOS 27.0.0 (`Darwin`), Rust `1.97.1` per [`rust-toolchain.toml`](../../rust-toolchain.toml), `redb` backend selected for `oxpinyin-testsupport` to satisfy the engine's dev-deps link; the diagnostic reads no DBM.
- Aggregate JSON emitted by the test:
  `{"keystrokes":123,"windows":581,"paths":256,"probes":2308,"unique_paths":114,"unique_probes":1676,"paths_per_window":0.440620,"probes_per_window":3.972461,"unique_paths_ratio":0.445312,"unique_probes_ratio":0.726170,"matrix_entries":245,"paths_hist":{"1x":68,"2x":18,"3x":5,"4x":11,"5+x":12},"probes_hist":{"1x":1368,"2x":178,"3x":31,"4x":76,"5+x":23}}`
