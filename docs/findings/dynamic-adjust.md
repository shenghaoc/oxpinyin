# DYNAMIC_ADJUST

Date: 2026-08-28 · Status: **implemented (Phase 2), and oracle-verified
— the frozen pins were re-measured on 2026-08-28 and none moved. One
premise of the Phase 1 brief did not hold — see §c; the conclusion
survives for a different reason.** · Branch: `feat/dynamic-adjust`.

Phase 1 (the read-back) is below unchanged; Phase 2 (what was built, and
what is still unverified) is the last two sections.

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

---

# Phase 2 — what was built

## The three gates, at their pin placements

| Gate | Pin | oxpinyin | Placement |
|---|---|---|---|
| 1 — previous token | `_get_previous_token` (`pinyin.cpp:1711-1767`) | `Session::previous_token` (`session.rs`) | once per guess |
| 2 — merge the gram | `m_system_bigram->load` + `m_user_bigram->load` + `merge_single_gram` (`pinyin.cpp:2200-2214`) | `LanguageModel::merged_successors` → `MergedGram`, called by `Session::dynamic_adjust_gram` | once per guess |
| 3 — index it | `merged_gram->get_freq(token, …)` inside `_compute_frequency_of_items` (`pinyin.cpp:1855-1866`) | `MergedGram::possibility` via `dynamic_adjust_bigram_possibility` | once per candidate |

The placement is the whole point of Phase 2, not the arithmetic. Merging
per candidate would turn one row load into `O(candidates)` loads and
worsen time without buying space, which the source policy forbids — and
it is invisible to any output-only assertion, because it produces the
same numbers. So the in-tree test counts the merges (below).

### Gate 1

`previous_token(0)` answers `sentence_start` (1), matching the pin. The
`m_prefixes` scan the pin performs there is unreachable on the drop-in
surface (only `pinyin_guess_sentence_with_prefix` populates it, and
neither reference consumer calls it), so offset 0 is deterministic.

Above 0 the pin's guard is carried verbatim: `result[offset]` is
inspected **first**, and only when a phrase begins at `offset` does the
walk go backwards for the nearest preceding token. A guess at an offset
no phrase starts at contributes no bigram term at all.

### Gate 2

`merged_successors` is a **defaulted** trait method returning `None`, so
every existing implementor compiles unchanged and contributes no bigram
term — which is exactly the DYNAMIC_ADJUST-clear behaviour. Only
`SharedLm` (capi) implements it: one `load_successors`, one
`bigram_successors`, one `bigram_total`, merged into one `MergedGram`.

`MergedGram` keeps its records sorted by token so `count` is a binary
search, mirroring `SingleGram`'s sorted item array
(`ngram.cpp:178-196`).

### Gate 3

The bigram possibility joins the unigram term **inside** the pin's
expression, before its single truncation:

```c
freq = (lambda * bigram_poss * BIGRAM_FREQUENCY_DISCOUNT +
        (1 - lambda) * unigram / (gfloat) total_freq) * 256 * 256 * 256;
```

`trunc(a) + trunc(b)` differs from `trunc(a + b)` by up to one unit, and
that unit is not a rounding detail: the truncation is what collapses
near-ties into equal comparator keys (Class A, `corpus-tail.md`), so an
off-by-one moves candidates between tie classes and reorders the list.
Hence one function taking the possibility, not an additive term bolted
onto the unigram result.

With `bigram_poss == 0.0` the first term is exactly `0.0` and
`0.0 + x == x` in IEEE-754, so the bit-clear path is bit-identical to
the pre-existing unigram law **by construction** — not merely because
the frozen words happen to leave the bit clear.

The addon and predicted branches carry no bigram term, matching the pin,
which returns from those branches before reaching the expression.

## The pinned defect, replaced

`candidate_frequency_sort_key` / `dynamic_adjust_bigram_term(options)
-> u64` are **gone**, not filled in: the pin's term depends on
`(prev_token, candidate_token)` and neither was reachable from that
signature. The test that pinned the stub's `0` is replaced by two:

- `dynamic_adjust_folds_the_bigram_term_only_with_the_bit_and_a_gram` —
  the three ways the term is zero (bit clear, no gram, token absent from
  the row), the one way it is not, and that a non-zero possibility
  actually raises the amplified frequency.
- `dynamic_adjust_merges_one_row_per_guess_and_lifts_only_the_credited_token`
  — the gates end to end through `Session`, over a counting model.

Both fail against a stub. Verified by reverting the implementation three
ways and observing the failure each time:

| Poison | Assertion that fired |
|---|---|
| Gate 3 returns a constant `0.0` possibility | order stayed `["系","统"]`, expected `["统","系"]` |
| Gate 1 answers `null_token` at offset 0 | asked about `0xffffffff`, expected `1` |
| one merge per candidate (bit-gated) | 2 candidates merged 3×, 6 candidates merged 7× — counts must be equal |

The merge-count assertion is the one that catches the complexity
regression, and it is the reason the model in that test counts calls
rather than only answering them.

## The frozen-profile guard

The Phase 1 safety argument is **not** "the term is zero at offset 0" —
that premise is false (§c). It is only that no frozen option word sets
bit 9. `no_frozen_option_word_sets_dynamic_adjust` now reads the
harness's own `PARITY_OPTIONS` words out of `tools/bisection/*.c` and
asserts `word & 0x200 == 0`, so adding the bit to a frozen profile fails
the suite instead of silently moving pins. It also asserts it found at
least one word, so the guard cannot pass vacuously — verified by
poisoning `live-typing-diff.c`'s word to `0x38a`, watching the guard
fire, and restoring.

## What is NOT verified

**The differential did not run.** `tools/bisection/dynamic-adjust-diff.c`
and `run-dynamic-adjust-diff.sh` are in-tree and the driver compiles
clean under `-Wall -Wextra -Werror`, but the runner **self-skipped**
(exit 0) because the pin-built oracle is not present in this
environment: `tools/oracle/build-oracle.sh` fetches SHA-pinned tarballs
from `codeload.github.com`, which the network policy answers with 403.
Nothing in this branch has been compared against upstream.

The runner enforces non-vacuity when it does run — both engines are
driven with the bit set and clear, and identical output from either is a
failure (exit 3), not a pass — because the shape that exercises the bit
is parse → guess sentence → choose → guess at the advanced offset, and a
probe that never reaches Gate 3 would otherwise "agree" trivially.

Driving oxpinyin alone against the in-tree `fixtures/w3` mini tables
produces **identical** output for both bit states (five lines each,
mostly `no-first-candidate`): the mini fixture is too small to reach a
second offset with candidates. That is why the wiring proof is the
engine-level test rather than the differential — it needs no data set —
and why the differential's skip message now says so explicitly instead
of claiming to be the only coverage.

**The frozen pins were not re-measured** *at the time this was written*.
Candidate pins (10,190/10,190; top-5-set 10,190; absent 0; order-only 0;
prefix-10 98,930/98,930) and sentence (488/385/379) are measured against
the oracle, which could not be provisioned then. The §c argument says
they cannot move — the bit is clear in every frozen word, and the
bit-clear arithmetic is bit-identical by construction — but that was an
argument, not a measurement.

> **Measured 2026-08-28 (`docs/findings/dropin-stack-remeasurement.md`).**
> Oracle provisioning was fixed, and the stack tip was scored against a
> live pin: **every one of those pins holds exactly**, and every
> differential agrees with main. The argument above is now a
> measurement, and the "re-measure before merging" condition is
> discharged.
