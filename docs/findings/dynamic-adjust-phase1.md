# DYNAMIC_ADJUST — Phase 1

Date: 2026-08-28 · Status: **Phase 1 only; no code. One premise of the
brief does not hold — see §c.** · Branch: `feat/dynamic-adjust`.

`session.rs:2634`'s `dynamic_adjust_bigram_term` returns `0` and
discards the bit, and the test at `:3570` pins that. Under the policy's
E2E I/O rule that is a pinned defect, not a passing compliance claim.
This is the read-back before implementing.

## (a) `_get_previous_token` — what "previous token" means

`pinyin.cpp:1711-1767`. Two branches, and neither reads the constraint
store:

**`offset == 0`** — starts at `prev_token = sentence_start` (**not**
`null_token`; `novel_types.h:121-122` gives `null_token = 0`,
`sentence_start = 1`), then scans `instance->m_prefixes` for the
**longest** non-`sentence_start` token and takes that if one exists.

`m_prefixes` is populated only by `pinyin_guess_sentence_with_prefix`,
which **neither reference consumer calls** — it is in `abi-subset.md`'s
28-symbol complement and absent from fcitx's live set. So for the
drop-in surface `m_prefixes` is always empty and offset 0 yields exactly
`sentence_start`, deterministically.

**`offset > 0`** — from `m_nbest_results.get_result(0)`, the 1-best
result. Returns `null_token` immediately if there are no results. Then a
guard that is easy to miss: it reads `result[offset]` first and only if
**that** is non-null does it walk backwards from `offset - 1` for the
first non-null token. If `result[offset]` is null, `prev_token` stays
null and the whole feature is inert at that offset.

**Interaction with the constraint machinery: none.** It reads the n-best
result array, not `m_constraints`. oxpinyin already holds that array as
`Session::last_result: Vec<PhraseSpan>`, documented in-tree as
"upstream's `m_nbest_results[0]`". The data (a) needs is present.

## (b) `merge_single_gram` — callable, but not free

Ported as `merge_counts` / `merge_bigram` in
`oxpinyin-data/src/lm/mod.rs:110-135` for the n-best step costs. Both
are **pure functions over count pairs** holding no state, so calling
them from the candidate path duplicates nothing and cannot desynchronise
anything.

**But the shapes differ, and this is a complexity question, not a
plumbing one.** The pin merges the *whole gram* once per guess (Gate 2,
outside the candidate loop) and then indexes the merged gram per
candidate. oxpinyin's `merge_bigram` merges *one* pair. Calling it per
candidate turns one gram load plus O(1) lookups into O(candidates)
bigram lookups. AGENTS.md forbids worsening time and space together, so
Phase 2 must hoist the merged gram to the guess, matching Gate 2's
placement rather than only its arithmetic.

## (c) The safe-by-construction claim — wrong mechanism, right conclusion

The brief states: *"at offset 0 with null prev, the bigram term is zero
and the bit is a no-op. The frozen corpus runs exactly that way."*

**The first half is false.** At offset 0 the pin returns
`sentence_start`, not null. `sentence_start != null_token`, so Gate 2's
`if (null_token != prev_token)` is **true** at offset 0 and the pin
loads and merges the sentence-start bigram — a real, populated gram in
model20. The bit is *not* inert at offset 0, and an implementation built
on the belief that it is would be wrong.

**The conclusion holds anyway, for a different reason: no frozen option
word sets the bit.**

```
DYNAMIC_ADJUST = 1U << 9 = 0x200        (pinyin_custom2.h:40)

0x18a = IS_PINYIN | PINYIN_INCOMPLETE | USE_DIVIDED_TABLE | USE_RESPLIT_TABLE
        bit 9 CLEAR
0x1e    bit 9 CLEAR
0x0     bit 9 CLEAR
```

So the frozen candidate pins were measured with the bit clear on both
sides, and implementing the feature cannot move them **provided the
implementation stays gated on the bit** — which the existing stub's
`options.has_dynamic_adjust()` already is.

The distinction matters beyond pedantry: anyone later adding
DYNAMIC_ADJUST to a frozen word on the belief that "offset 0 is inert"
would move the pins. The safety argument is *bit-clear*, not *null-prev*,
and should be recorded that way.

## (d) zhuyin — out of scope, and not by a judgement call

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

The drop-in target is `libpinyin.so.15`. libzhuyin is a separate drop-in
with a separate consumer (ibus-libzhuyin, not ibus-libpinyin). So this
is not a consumer-union scoping decision — those sites are simply not in
the library being replaced.

## The implementation fact the brief understates

> "the call site exists … the plumbing is in place"

The plumbing is in place for a term that depends on **nothing but the
options**:

```rust
fn candidate_frequency_sort_key(options: OptionBits, unigram: u64) -> u64 {
    unigram.saturating_add(dynamic_adjust_bigram_term(options))
}
fn dynamic_adjust_bigram_term(options: OptionBits) -> u64 { … }
```

The pin's Gate 3 term depends on **(prev_token, candidate_token)** via
the merged gram. Neither is reachable from this signature. So Phase 2 is
not "fill in the stub": the signature, the call site and the guess-level
setup all change — `prev_token` resolved once per guess (Gate 1), the
merged gram built once per guess (Gate 2), and the per-candidate lookup
threaded into `candidate_frequency_sort_key` (Gate 3). The existing
one-line call site is where the change *lands*, not evidence that it is
small.

## Why it is required, confirmed

- `abi-subset.md:784` — enabled by default, user-togglable via the
  `dynamic-adjust` GSettings key, part of `PINYIN_DEFAULT_OPTION`.
- ibus-libpinyin maps the key to the bit (`PYPConfig.cc:219`).
- **fcitx-libpinyin sets it unconditionally** — `settings |=
  DYNAMIC_ADJUST;` (`eim.cpp:940`), with no toggle at all.

Every fcitx session and every default ibus session runs with the bit
set, so candidate ranking at any non-first-position lookup is currently
wrong for both consumers.

## What Phase 2 owes

1. Gates 1–3 implemented at their pin placements, not just their
   arithmetic — prev token and merged gram hoisted to the guess.
2. The test at `session.rs:3570` replaced: it must assert the term is
   **non-zero** with the bit set and a prev token available. A test that
   passes against the stub is not a test.
3. A new differential: the frozen corpus is single-shot at offset 0 with
   the bit clear, so it cannot exercise this. Drive choose → guess at
   the resulting offset with the bit **set**, compare ranked candidates
   against the pin, and prove non-vacuity by clearing the bit and
   watching the assertions go red.
4. Candidate pins bit-identical (10,190/10,190; top-5-set 10,190;
   absent 0; order-only 0; prefix-10 98,930/98,930) and sentence
   488/385/379 — expected to hold by the §c argument, and to be
   re-measured rather than assumed.
