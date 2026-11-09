# DYNAMIC_ADJUST

Date: 2026-08-28 · Status: **implemented** (#204)

This record describes the pin's `DYNAMIC_ADJUST` option (bit 9) and how
oxpinyin reproduces it. With the bit set, candidate ranking folds a
bigram term from the previous token into each candidate's frequency.
Sections (a)–(d) give the pin's behaviour; the later sections give
oxpinyin's implementation and its tests.

## (a) `_get_previous_token` — what "previous token" means

`pinyin.cpp:1711-1767`. Two branches, and neither reads the constraint
store:

**`offset == 0`** — starts at `prev_token = sentence_start` (**not**
`null_token`; `novel_types.h:121-122` gives `null_token = 0`,
`sentence_start = 1`), then scans `instance->m_prefixes` for the
**longest** non-`sentence_start` token and takes that if one exists.

Correction (#1776): the full exported surface includes sentence guesses
with prefixes in both ABIs and Pinyin prediction guesses. These populate
`m_prefixes` even when the subsequent lookup refuses. Consumer call sets
do not make this branch unreachable. Missing or unloaded phrase items
are skipped; equal lengths retain the first token.

**`offset > 0`** — from `m_nbest_results.get_result(0)`, the 1-best
result. Returns `null_token` immediately if there are no results. Then a
guard that is easy to miss: it reads `result[offset]` first and only if
**that** is non-null does it walk backwards from `offset - 1` for the
first non-null token. If `result[offset]` is null, `prev_token` stays
null and the whole feature is inert at that offset.

**Interaction with the constraint machinery: none.** It reads the n-best
result array, not `m_constraints`. oxpinyin holds that array as the
session's last result, upstream's `m_nbest_results[0]`.

## (b) `merge_single_gram` — once per guess

The pin merges the *whole gram* once per guess (Gate 2, outside the
candidate loop) and then indexes the merged gram per candidate. Merging
per candidate would turn one gram load plus O(1) lookups into
O(candidates) bigram lookups, so oxpinyin keeps the pin's placement as
well as its arithmetic.

## (c) The bit is not inert at offset 0

At offset 0 the pin returns `sentence_start`, not null.
`sentence_start != null_token`, so Gate 2's
`if (null_token != prev_token)` is **true** at offset 0 and the pin
loads and merges the sentence-start bigram — a real, populated gram in
model20. The bit is *not* inert at offset 0.

**The frozen candidate pins do not move because no frozen option word
sets the bit.**

```
DYNAMIC_ADJUST = 1U << 9 = 0x200        (pinyin_custom2.h:40)

0x18a = IS_PINYIN | PINYIN_INCOMPLETE | USE_DIVIDED_TABLE | USE_RESPLIT_TABLE
        bit 9 CLEAR
0x1e    bit 9 CLEAR
0x0     bit 9 CLEAR
```

The frozen candidate pins were measured with the bit clear on both
sides, and the implementation is gated on the bit. Adding DYNAMIC_ADJUST
to a frozen word would move the pins: the safety argument is *bit-clear*,
not *null-prev*.

## (d) zhuyin — a separate library

`zhuyin.cpp` has 5 DYNAMIC_ADJUST sites, and **all of them are in a
different shared library**:

```
libpinyin_la_SOURCES = $(pinyin_SOURCES) pinyin.cpp     → libpinyin.so.15
libzhuyin_la_SOURCES = $(pinyin_SOURCES) zhuyin.cpp     → libzhuyin.so
```

(`src/Makefile.am:89,110`; Debian ships `libzhuyin15` and
`libzhuyin-dev` as separate binary packages from the same source.)
`libpinyin.ver` exports **zero** `zhuyin_*` symbols — its three "zhuyin"
entries are `pinyin_set_zhuyin_scheme`, `pinyin_get_zhuyin_string` and
`pinyin_get_secondary_zhuyin_string`, which are `pinyin_*` functions.

The libpinyin drop-in is `libpinyin.so.15`. libzhuyin is a separate
drop-in with a separate consumer (ibus-libzhuyin); the zhuyin C crate
reproduces its own sites (`crates/oxpinyin-zhuyin-capi/src/sentence.rs`).

## Why it matters

- `abi-reference.md` §3 (config surface) — enabled by default, user-togglable via the
  `dynamic-adjust` GSettings key, part of `PINYIN_DEFAULT_OPTION`.
- ibus-libpinyin maps the key to the bit (`PYPConfig.cc:219`).
- **fcitx-libpinyin sets it unconditionally** — `settings |=
  DYNAMIC_ADJUST;` (`eim.cpp:940`), with no toggle at all.

Every fcitx session and every default ibus session runs with the bit
set, so the bigram term reaches candidate ranking for both consumers.

## The three gates, at their pin placements

| Gate | Pin | oxpinyin | Placement |
|---|---|---|---|
| 1 — previous token | `_get_previous_token` (`pinyin.cpp:1711-1767`) | `Session::previous_token` (`session/lookup.rs`) | once per guess |
| 2 — merge the gram | `m_system_bigram->load` + `m_user_bigram->load` + `merge_single_gram` (`pinyin.cpp:2200-2214`) | `LanguageModel::merged_successors` → `MergedGram`, called by `Session::dynamic_adjust_gram` | once per guess |
| 3 — index it | `merged_gram->get_total_freq` and `get_freq(token, …)` inside `_compute_frequency_of_items` (`pinyin.cpp:1845-1852`) | `MergedGram::total` and `MergedGram::get_freq` via `dynamic_adjust_bigram_possibility` | once per candidate |

The placement matters as much as the arithmetic. Merging per candidate
would turn one row load into `O(candidates)` loads and worsen time
without buying space, which the source policy forbids. It is invisible
to any output-only assertion, because it produces the same numbers, so
the engine test counts the merges (below).

### Gate 1

`previous_token(0)` scans the retained prefix tokens and chooses the
longest current phrase item, keeping the first tie. It falls back to
`sentence_start` (1) when no readable prefix has positive length.
The engine retains these tokens across parsing. Sentence guesses replace
them before lookup, including refused guesses; Pinyin prediction also
replaces them. Full reset clears them. The runtime reads the live item's
stored length without decoding text or resurrecting a removed system item.

Above 0 the pin's guard is carried verbatim: `result[offset]` is
inspected **first**, and only when a phrase begins at `offset` does the
walk go backwards for the nearest preceding token. A guess at an offset
no phrase starts at contributes no bigram term at all.

### Gate 2

`merged_successors` is a **defaulted** trait method returning `None`, so
every existing implementor compiles unchanged and contributes no bigram
term — which is exactly the DYNAMIC_ADJUST-clear behaviour. `RuntimeLm`
implements it: one system point read and the user gram's raw value
(`bigram_row`), merged by `MergedGram::merge` as `merge_single_gram`
merges them (`ngram.cpp:277-352`, #757 PR 3).

`MergedGram` is the merged chunk itself. It is read with the ported
`SingleGram` walks, from `lower_bound` (`ngram.cpp:231-250`), so an
unsorted or duplicated row answers as it does in the pin; nothing is
sorted or deduplicated. A user value the pin cannot read refuses the
guess (register rows 85 and 98). The eager refresh a parse or an option
change runs does not refuse it, because the pin's parse merges no gram
(`pinyin.cpp:1497-1520`); that list leaves the bigram term out instead.

### Gate 3

The bigram possibility joins the unigram term **inside** the pin's
expression, before its single truncation:

```c
freq = (lambda * bigram_poss * BIGRAM_FREQUENCY_DISCOUNT +
        (1 - lambda) * unigram / (gfloat) total_freq) * 256 * 256 * 256;
```

`trunc(a) + trunc(b)` differs from `trunc(a + b)` by up to one unit, and
that unit is not a rounding detail: the truncation is what collapses
near-ties into equal comparator keys (Class A), so an
off-by-one moves candidates between tie classes and reorders the list.
Hence one function taking the possibility, not an additive term bolted
onto the unigram result.

With `bigram_poss == 0.0` the first term is exactly `0.0` and
`0.0 + x == x` in IEEE-754, so the bit-clear path is bit-identical to
the pre-existing unigram law **by construction** — not merely because
the frozen words happen to leave the bit clear.

The addon and predicted branches carry no bigram term, matching the pin,
which returns from those branches before reaching the expression.

## Tests

Two engine tests in `crates/oxpinyin-engine/src/session/tests.rs` hold the gates:

- `dynamic_adjust_folds_the_bigram_term_only_with_the_bit_and_a_gram` —
  the three ways the term is zero (bit clear, no gram, token absent from
  the row), the one way it is not, and that a non-zero possibility
  actually raises the amplified frequency.
- `dynamic_adjust_merges_one_row_per_guess_and_lifts_only_the_credited_token`
  — the gates end to end through `Session`, over a counting model.

Both fail against a stub. Each poison below was applied and observed to
fail (recorded evidence):

| Poison | Assertion that fired |
|---|---|
| Gate 3 returns a constant `0.0` possibility | order stayed `["系","统"]`, expected `["统","系"]` |
| Gate 1 answers `null_token` at offset 0 | asked about `0xffffffff`, expected `1` |
| one merge per candidate (bit-gated) | 2 candidates merged 3×, 6 candidates merged 7× — counts must be equal |

The merge-count assertion is the one that catches the complexity
regression, and it is the reason the model in that test counts calls
rather than only answering them.

## The frozen-profile safety argument

The safety argument for the frozen pins is that no frozen option word
sets bit 9 (§c), not that the term is zero at offset 0. The bit-clear
arithmetic is bit-identical to the unigram law by construction (Gate 3).
The C facades' instance-law tests cover the bit-set path against a
short sentence-start gram (`crates/oxpinyin-facade/tests/instance_laws.rs`,
register row 85).
