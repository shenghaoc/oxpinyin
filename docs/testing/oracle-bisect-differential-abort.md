# Findings — pinned oracle aborts under bisect's differential mode

Date: 2026-08-18 · Source tier: post-W11 verification observation.
Status: recorded (upstream defect; oxpinyin unaffected; upstream fixed at 95e3af7).

## Summary

`tools/bisection/run-bisect.sh` accepts an optional differential form that
loads a second `.so` and its data dir to compare oxpinyin-capi output against
the pinned oracle:

```sh
./run-bisect.sh /path/to/libpinyin.so /path/to/oracle-data
```

In this mode the pinned oracle aborts inside its own code:

```text
pinyin.cpp:2175: Assertion `zero_key != key' failed
```

This is `assert()` reaching `abort()`, so the harness exits non-zero from
inside the oracle process rather than reaching the differential diff step.

## Reproduction

Reproduced on the pinned oracle (libpinyin 2.11.91 @ `0c5e80e`).

### Bisect differential mode (Mode 4)

```sh
cd tools/bisection
gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o bisect bisect.c -ldl
./bisect ~/.local/opt/pinyin-oracle/lib/libpinyin.so \
         ~/.local/opt/pinyin-oracle/lib/libpinyin/data
```

Abort output (last lines before crash):

```text
=== input: "nihao" ===
parse_full: consumed=5
...
get_left_pinyin_offset(5): true left=2
bisect: pinyin.cpp:2175: bool _check_offset(pinyin::PhoneticKeyMatrix&, size_t):
  Assertion `zero_key != key' failed.
```

The abort fires inside `pinyin_get_right_pinyin_offset(inst, 5, &right)`.
With `USE_RESPLIT_TABLE` enabled, the parser builds a PhoneticKeyMatrix
that includes zero-key entries (representing potential resplit boundaries).
`_check_offset` asserts that the position at `offset - 1` is not a zero
key, but resplit processing makes that position a zero key for "nihao"
at consumed=5.

### ibus-libpinyin#570 pattern

The same `_check_offset` function is also called at the top of
`pinyin_guess_candidates`. The frontend pattern that triggers
[ibus-libpinyin#570](https://github.com/libpinyin/ibus-libpinyin/issues/570)
is:

1. Parse a phrase containing the separator `'`
2. `guess_candidates(offset=0)` — choose a 1-character candidate
3. `choose_candidate(inst, 0, cand)` — returns `new_offset`
4. `guess_candidates(inst, new_offset, ...)` — fires `_check_offset(matrix, new_offset)`

Tested on the pin with examples from #570:

```sh
# Throwaway C driver (test570.c): parse → guess(0) → choose 1-char → guess(new_offset)
/tmp/test570 ~/.local/opt/pinyin-oracle/lib/libpinyin.so \
             ~/.local/opt/pinyin-oracle/lib/libpinyin/data
```

| Pattern    | Consumed | 1st 1-char | new_offset | guess(new_offset) |
|------------|----------|------------|------------|-------------------|
| xiang'a    | 7        | 向 [3]     | 5          | ok (10 cands)     |
| jiang'a    | 7        | 将 [3]     | 5          | ok (10 cands)     |
| liang'a    | 7        | 两 [3]     | 5          | ok (10 cands)     |
| bian'a     | 6        | 便 [3]     | 4          | ok (10 cands)     |
| bian'e     | 6        | 便 [3]     | 4          | ok (192 cands)    |
| bian'l     | 6        | 便 [13]    | 4          | ok (2187 cands)   |
| bian'li    | 7        | 便 [4]     | 4          | ok (375 cands)    |

On pin 0c5e80e, the #570 patterns (with explicit `'` separator) do not
abort in isolation because the returned `new_offset` lands before the
separator position for these specific inputs. The bisect abort triggers
via a different code path (`get_right_pinyin_offset`) where `_check_offset`
is called with `consumed` pointing at a resplit-induced zero key.

Both share the same root cause: `_check_offset` has
`assert(zero_key != key)` that fires when position `start - 1` in the
matrix contains a zero key.

## Attribution

- **Upstream, not oxpinyin.** The abort fires inside pin-built
  `libpinyin.so`; nothing in the oxpinyin tree runs on that stack. The
  bisection harness is a plain `dlopen` driver — see `tools/bisection/bisect.c`
  — and the failure reproduces on `main` independently of any oxpinyin change.
- Same shape as the concurrency-only `zero_key != key` abort already noted
  in `crates/pinyin-oracle/src/live.rs` (see `ORACLE_LOCK`), but a different
  trigger: this one fires under the single-threaded bisect driver.

## Scope

- The **canonical CI form** of the bisection harness (capi-only, plus the
  valgrind pass at Mode 2 of `run-bisect.sh`) is **unaffected** — it never
  loads the oracle `.so`.
- Only the **optional differential form** (Mode 4 in `run-bisect.sh`, gated
  on the two positional arguments being present) exercises the oracle path
  that trips the assertion.
- Parity coverage against the oracle is not lost: the routine differential
  runs through `pinyin-oracle` (`crates/pinyin-oracle/src/live.rs`) under
  `ORACLE_LOCK`, not through the bisect driver, and remains green.

## Upstream status

- **Pin `0c5e80e`** — still asserts (`_check_offset` calls `abort()` via
  `assert(zero_key != key)`). This is the assertion the W11 bisect hit.
- **libpinyin `95e3af7`** — Peng Wu's
  [`Fix _check_offset function`](https://github.com/libpinyin/libpinyin/commit/95e3af71cca3ce6a974e55ab68db1424da79c286)
  (2026-08-18) replaces the assert with a graceful `return false`:

  ```cpp
  // Before (0c5e80e):
  assert(zero_key != key);

  // After (95e3af7):
  if (zero_key != key)
      return false;
  ```

  Verified locally: rebuilt libpinyin at `95e3af7`, the `nihao` +
  `get_right_pinyin_offset(5)` test no longer aborts (returns `true`,
  `right=6`). The #570 patterns also complete without abort.

  Note: the full bisect harness still hits a **different** assertion on the
  fixed build (`phonetic_key_matrix.h:103: Assertion 'index < m_table_content->len'`)
  for "beijing". This is a separate bug not addressed by 95e3af7.

- **ibus-libpinyin#570** — reported 2026-08-06:
  <https://github.com/libpinyin/ibus-libpinyin/issues/570>

## oxpinyin-capi

oxpinyin does not abort on any of the trigger patterns:

```sh
/tmp/test_oxpinyin target/debug/libpinyin_capi.so fixtures/w3
```

| Test                                        | Result              |
|---------------------------------------------|---------------------|
| nihao + get_right_pinyin_offset(5)          | true, right=5       |
| xiang'a: choose → guess(new_offset)        | ok (83 cands after) |
| jiang'a / liang'a / bian'a / bian'e / ...   | ok (1 cand; no abort) |

Some patterns return fewer candidates than the oracle (1 vs hundreds)
because oxpinyin's candidate generation is still less complete in the
current stage. The important result: **no abort, no panic, processing
continues normally** for all tested inputs.

## Action

None on this side. No code, test, or pin change. Recorded here so future
runs of the optional differential form of `run-bisect.sh` have a pointer
to the known upstream cause rather than being re-diagnosed. The oracle pin
stays at `0c5e80e` until the next formal pin bump.

## Addendum (2026-08-26) — fourth sighting: the word-move second check, and a corrected call site

The D1/D2 closure (`fix/cursor-offset-normalization`) measured the
cursor-helper surface first-hand on the rebuilt pin with a fork-per-probe
driver (every offset in its own child, so an abort is a datum). For
`nihaoshijie` under the parity word `0x18a`, 48 probes over
`pinyin_get_pinyin_offset` / `get_left_pinyin_offset` /
`get_right_pinyin_offset` produce exactly ONE abort:

```text
get_right_pinyin_offset(inst, 11, &right)  ->  SIGABRT
pinyin.cpp:2175: bool _check_offset(...): Assertion `zero_key != key' failed.
```

Mechanism, traced end to end: the FIRST `_check_offset` at offset 11
passes (column 10 holds the lone non-zero `e`); column 11 is a singleton
zero key (the pin's reserved extra slot), so the walk reads its
`m_raw_end` = 12, and the SECOND `_check_offset` — the one on the
COMPUTED result, `pinyin.cpp:3090` inside `get_right_pinyin_offset` —
sees column 11's lone zero key and asserts.

**Correction, recorded not silently applied:** the uncovered-surface
measurement (PR #174, `uncovered-surface-differentials.md` harness note)
attributed the tail-cursor abort to `get_left_pinyin_offset(11)` via the
left walk's second check (`pinyin.cpp:3055`). That is wrong: `get_left(11)`
genuinely does not abort — its walk halts at column 10 (the `e` key ends
at 11), the zero-start walk stops at the empty column 9, and the second
check at 10 passes. The abort lives at `get_right(11)` via `pinyin.cpp:3090`.
Both aborts are the same `_check_offset` assert at `pinyin.cpp:2175` —
same landmine, wrong call site recorded. Incidentally, offset 8 is fully
measurable for both moves (`left=5`, `right=11`); only offset 11 aborts,
and only the right call — the harness's word-move probe skip at cursor 8
is over-cautious (the skip prints identical lines on both sides, so the
differential is unaffected).

**oxpinyin's answer at the abort points:** the no-abort policy —
`pinyin_get_right_pinyin_offset` (and its siblings) return `false` where
the pin aborts (`EngineError::ZeroKeyOffsetCheck`), extending the same
policy as the guess seam above. This diverges from BOTH the pin (abort)
and post-`95e3af7` upstream (which discards an inverted-condition
`false` and returns true with the unusable computed value); both arms
are recorded in `upstream-divergences.md`, as the fifth finding in this
family.
