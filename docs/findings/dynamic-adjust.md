# DYNAMIC_ADJUST

Status: **reference**

This record describes the pin's `DYNAMIC_ADJUST` option (bit 9) and how
oxpinyin reproduces it. With the bit set, candidate ranking folds a
bigram term from the previous token into each candidate's frequency.
Sections (a)-(e) give the pin's behaviour; the later sections give
oxpinyin's implementation and its tests. Pin lines are at libpinyin
`074a2219` (`tools/oracle/oracle-pin.txt`).

## (a) `_get_previous_token`: what "previous token" means

`pinyin.cpp:1711-1767`. Two branches, and neither reads the constraint
store:

**`offset == 0`**: starts at `prev_token = sentence_start` (**not**
`null_token`; `novel_types.h:121-122` gives `null_token = 0`,
`sentence_start = 1`), then scans `instance->m_prefixes` for the
**longest** non-`sentence_start` token and takes it if one exists. Missing
or unloaded phrase items are skipped; equal lengths keep the first token.
Sentence guesses with prefixes (in both ABIs) and Pinyin prediction
guesses populate `m_prefixes`, also when the following lookup refuses, so
this branch is reachable from the exported surface.

**`offset > 0`**: from `m_nbest_results.get_result(0)`, the 1-best
result. It returns `null_token` at once if there is no result. Then it
reads `result[offset]` first, and only if **that** is non-null does it walk
backwards from `offset - 1` for the first non-null token. If
`result[offset]` is null, `prev_token` stays null and the feature is inert
at that offset.

**Interaction with the constraint machinery: none.** It reads the n-best
result array, not `m_constraints`. oxpinyin holds that array as the
session's last result, upstream's `m_nbest_results[0]`.

## (b) What the bit gates

`DYNAMIC_ADJUST` gates the **bigram contribution to candidate
frequency** and nothing else:

| Site | Under the bit | When the bit is clear |
|---|---|---|
| `pinyin_guess_candidates` (`pinyin.cpp:2203-2205`) | looks up `prev_token` | `prev_token` stays `null_token` |
| `pinyin_guess_candidates` (`pinyin.cpp:2210-2216`) | loads the system and user bigram of `prev_token` and merges them, once per guess | no merged gram |
| `_compute_frequency_of_items` (`pinyin.cpp:1845-1853`) | sets `bigram_poss` from the merged gram | no `λ · bigram_poss · DISCOUNT` term in `m_freq` |

The unigram term of `m_freq` (`pinyin.cpp:1856-1866`) always reads
`FacadePhraseIndex` (system and user unigrams) with no `DYNAMIC_ADJUST`
check. Sentence decode has no check in `pinyin.cpp` either, and
`pinyin_train` (`pinyin.cpp:2670-2691`) has none: it writes whenever a user
directory and an n-best result exist. oxpinyin matches each of these: the
bigram term is gated, and the unigram term, decode and training are not.
Gating training on the bit would diverge from the pin.

## (c) The bit is not inert at offset 0

At offset 0 the pin returns `sentence_start`, not null.
`sentence_start != null_token`, so the
`if (null_token != prev_token)` test of the merge is **true** at offset 0,
and the pin loads and merges the sentence-start bigram, a populated gram
in model20.

**The frozen candidate pins do not move because no frozen option word
sets the bit.**

```
DYNAMIC_ADJUST = 1U << 9 = 0x200        (pinyin_custom2.h:40)

0x18a = IS_PINYIN | PINYIN_INCOMPLETE | USE_DIVIDED_TABLE | USE_RESPLIT_TABLE
        bit 9 CLEAR
0x1e    bit 9 CLEAR
0x0     bit 9 CLEAR
```

The frozen candidate pins hold with the bit clear on both sides, and the
implementation is gated on the bit. Adding `DYNAMIC_ADJUST` to a frozen
word would move the pins: the safety argument is *bit-clear*, not
*null-prev*.

## (d) Zhuyin: a separate library

`zhuyin.cpp` has five `DYNAMIC_ADJUST` sites (`zhuyin.cpp:1247, 1476,
1483, 1570, 1577`), and **all of them are in a different shared library**:

```
libpinyin_la_SOURCES = $(pinyin_SOURCES) pinyin.cpp     → libpinyin.so.15
libzhuyin_la_SOURCES = $(pinyin_SOURCES) zhuyin.cpp     → libzhuyin.so
```

(`src/Makefile.am:89,110`; Debian ships `libzhuyin15` and `libzhuyin-dev`
as separate binary packages from the same source.) `libpinyin.ver`
exports **no** `zhuyin_*` symbol; its three "zhuyin" entries
(`pinyin_set_zhuyin_scheme`, `pinyin_get_zhuyin_string`,
`pinyin_get_secondary_zhuyin_string`) are `pinyin_*` functions. libzhuyin
is a separate drop-in with a separate consumer (ibus-libzhuyin); the
zhuyin C crate reproduces its own sites
(`crates/oxpinyin-zhuyin-capi/src/sentence.rs`).

## (e) Consumers

- ibus-libpinyin maps the GSettings key `dynamic-adjust` (schema default
  `true`) to the bit (`PYPConfig.cc:219`), but its option mask never gains
  the bit, so a default ibus session runs with `DYNAMIC_ADJUST` **clear**
  (`PYPConfig.cc:145`, `PYConfig.h:56`;
  [option bits, GSettings mapping](option-bits.md#gsettings--bit-mapping-in-the-fork)).
  The bit-clear path is therefore the ibus default.

## The three gates, at their pin placements

| Gate | Pin | oxpinyin | Placement |
|---|---|---|---|
| 1: previous token | `_get_previous_token` (`pinyin.cpp:1711-1767`) | `Session::previous_token` (`session/lookup.rs`) | once per guess |
| 2: merge the gram | `m_system_bigram->load`, `m_user_bigram->load` and `merge_single_gram` (`pinyin.cpp:2210-2216`) | `LanguageModel::merged_successors` → `MergedGram`, called by `Session::dynamic_adjust_gram` | once per guess |
| 3: index it | `merged_gram->get_total_freq` and `get_freq(token, …)` inside `_compute_frequency_of_items` (`pinyin.cpp:1845-1853`) | `MergedGram::total` and `MergedGram::get_freq` through `dynamic_adjust_bigram_possibility` | once per candidate |

The placement matters as much as the arithmetic. Merging per candidate
would turn one gram load and O(1) lookups into `O(candidates)` loads and
worsen time without buying space, which the source policy forbids. It is
invisible to an output-only assertion, because it gives the same numbers,
so the engine test counts the merges (below).

### Gate 1

`previous_token(0)` scans the retained prefix tokens and chooses the
longest current phrase item, keeping the first tie. It falls back to
`sentence_start` (1) when no readable prefix has positive length. The
engine retains these tokens across parsing. Sentence guesses replace them
before lookup, including refused guesses; Pinyin prediction also replaces
them. Full reset clears them. The runtime reads the live item's stored
length without decoding text or resurrecting a removed system item.

Above 0 the pin's guard is carried verbatim: `result[offset]` is
inspected **first**, and only when a phrase begins at `offset` does the
walk go backwards for the nearest preceding token. A guess at an offset
where no phrase starts contributes no bigram term.

### Gate 2

`merged_successors` is a **defaulted** trait method that returns `None`,
so every implementor that does not override it contributes no bigram
term, which is the bit-clear behaviour. `RuntimeLm` implements it: one
system point read and the user gram's raw value (`bigram_row`), merged by
`MergedGram::merge` as `merge_single_gram` merges them
(`ngram.cpp:277-352`).

`MergedGram` is the merged chunk itself. It is read with the ported
`SingleGram` walks, from `lower_bound` (`ngram.cpp:231-250`), so an
unsorted or duplicated row answers as it does in the pin; nothing is
sorted or deduplicated. A user value the pin cannot read refuses the
guess (register rows 85 and 98). The eager refresh that a parse or an
option change runs does not refuse it, because the pin's parse merges no
gram (`pinyin.cpp:1497-1520`); that list leaves the bigram term out
instead.

### Gate 3

The bigram possibility joins the unigram term **inside** the pin's
expression, before its single truncation (`pinyin.cpp:1862-1865`):

```c
freq = (lambda * bigram_poss * BIGRAM_FREQUENCY_DISCOUNT +
        (1 - lambda) * unigram / (gfloat) total_freq) * 256 * 256 * 256;
```

`trunc(a) + trunc(b)` differs from `trunc(a + b)` by up to one unit. The
truncation makes near-ties into equal comparator keys (Class A), so an
off-by-one moves candidates between tie classes and reorders the list.
For this reason one function takes the possibility; the bigram term is not
a separate term added to the unigram result.

With `bigram_poss == 0.0` the first term is exactly `0.0`, and
`0.0 + x == x` in IEEE-754. The bit-clear path is therefore bit-identical
to the unigram law **by construction**, not only because the frozen words
leave the bit clear.

The addon and predicted branches carry no bigram term, matching the pin,
which leaves those branches with `continue` before reaching the expression
(`pinyin.cpp:1813-1843`).

## Tests

Two engine tests in `crates/oxpinyin-engine/src/session/tests.rs` hold the
gates:

- `dynamic_adjust_folds_the_bigram_term_only_with_the_bit_and_a_gram`:
  the three ways the term is zero (bit clear, no gram, token absent from
  the row), the one way it is not, and that a non-zero possibility raises
  the amplified frequency.
- `dynamic_adjust_merges_one_row_per_guess_and_lifts_only_the_credited_token`:
  the gates end to end through `Session`, over a counting model. Its
  merge-count assertion catches a per-candidate merge, which is why the
  model counts calls rather than only answering them.

The C facades' instance-law tests cover the bit-set path against a short
sentence-start gram (`crates/oxpinyin-facade/tests/instance_laws.rs`,
register row 85).
