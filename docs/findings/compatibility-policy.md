# Compatibility policy — what oxpinyin may diverge on, and what it may not

Date: 2026-08-28 · Status: **policy** (maintainer-decided; this document
records the decision and classifies the existing register against it) ·
Branch: `claude/pr2-compatibility-policy`.

## The goal this policy serves

oxpinyin is a **drop-in replacement**: rename the built shared object to
`libpinyin.so.15`, put it on the library path, and existing consumers
work against the data already on the system. Not a compatible
reimplementation a consumer is ported to — the same binary interface,
the same file formats, the same observable behaviour, with the consumer
unchanged and unaware.

That goal sets the default: **oxpinyin reproduces the pin.** Divergence
is not a design freedom to be exercised where the Rust is nicer. It is
an exception that has to be argued into one of four classes below, and
everything outside those four classes is a defect to be reverted.

## The four exception classes

There are four, and no others.

### (a) MATH — platform-dependent floating-point accumulation

Upstream's result is a pure function of `gfloat` values accumulated
through a transcendental (`log`), and no fixed-point or integer form
reproduces it. Reproducing it would require delegating to the platform
libm, which carries no cross-platform bit-exactness guarantee.

**Cited against constitution item 6:** *"Determinism: output is a pure
function of (input, user state, config)."* Output that depends on which
libm the build linked is not a function of those three, so reproducing
upstream here would violate the constitution rather than serve it. The
build's refusal of `-march=native` is the same rule applied earlier.

**The class is narrow.** Basic IEEE-754 operations — add, subtract,
multiply, divide, compare — are bit-reproducible across platforms and
are **not** covered. `amplified_frequency` ((1−λ)·unigram/total·2²⁴) is
basic-ops-only and is ported to 100%; that is the standard. A float in
the call graph does not make an entry class (a). A transcendental in
the accumulation does.

**Example:** the n-best trellis (`phonetic_lookup.h:663,692`) does
`m_poss += log(...)` per step into a `gfloat`, and near-ties among the
top-3 survivors are decided down to the ULP.

### (b) MEMORY SAFETY — upstream is UB and Rust structurally prevents it

Upstream's observable behaviour *is* undefined behaviour — a
use-after-free, an out-of-bounds read — and the Rust construction that
replaces it cannot express the bug. The divergence is not a decision;
there is no safe Rust that reproduces it.

**Scope note:** this class covers only cases where reproduction is
structurally impossible. Where upstream's UB happens to produce a
*stable, reproducible* value that a safe construction could also
produce, it is not class (b).

**Examples:** the bigram export iterator's stale pinyin buffer
(`pinyin.cpp:842-872`), a use-after-free the pin segfaults on when an
export cycle repeats; the aux-text heap over-read.

### (c) AVAILABILITY — upstream aborts on caller input

Upstream `assert()`s or `abort()`s on input a caller can supply.
oxpinyin returns `false` / `Err` instead and logs the point.

**Rust does not prevent aborting.** `panic!`, `abort()`, `unwrap()` and
`assert!` are all available and would reproduce upstream faithfully.
This class is therefore **not** a language-mechanism residue — it is a
deliberate product decision, and it must be labelled as one rather than
smuggled in as something Rust forced.

**Justification — MISRA C:2025 guideline D.4.1 (Required), assessed for
Rust.** MISRA C Directive 4.1, "Run-time failures shall be minimized",
is category Required. MISRA C:2025 Addendum 6 ("Applicability of MISRA
C:2025 to the Rust Programming Language", March 2025 —
[`MISRA-C-2025-ADD6`](https://misra.org.uk/app/uploads/2025/03/MISRA-C-2025-ADD6.pdf))
assesses D.4.1 as applicable to Rust, keeps its adjusted category
Required, and records the run-time failure as "often in the form of
panics". A library loaded into a long-lived input-method process must
not take the process down on caller error: an IME abort loses the user's
session, not just the call. Constitution item 4 ("nothing panics on any
input; public APIs return `Result`") is the in-house statement of the
same rule.

**Obligations on every (c) site:** return `false` (C ABI) or `Err`
(Rust), *and log the point*. A silently-swallowed abort is not class
(c) — it is a behaviour change with no record.

**Boundary:** (c) covers *aborts*. It does not cover upstream returning
a wrong-but-defined answer. Where upstream half-mutates and reports
success, reproducing it is possible and the divergence is a revert
target, not an availability exception. See the double-pinyin
out-of-enum row in the table below, which sits on exactly that line.

### (d) CONSUMER SCOPE — only what the two reference consumers call

Only what **ibus-libpinyin 1.16.5** and **fcitx-libpinyin** actually
call is in scope. Symbols in `libpinyin.ver` that neither consumer
touches are out of scope until a new consumer demonstrates a need.

**The two reference sources, named:**

1. **ibus-libpinyin 1.16.5** — the live call-site set is enumerated in
   `docs/findings/abi-subset.md` §1 (50 symbols), plus
   `pinyin_get_parsed_input_length` from the W8 fork (`2c5baa9`,
   `PYPLibPinyinCandidates.cc:151`), giving the 51-symbol W8 contract.
   The 28-symbol complement is §6 of the same document.
2. **fcitx-libpinyin** — a `src/` call-site grep, dead code excluded.
   Source identity is **not yet frozen**: unlike ibus above, no tag or
   commit is recorded here or in `abi-subset.md`. Pinning that release
   (tag or commit) and freezing fcitx's per-consumer symbol manifest from
   it is owed before the union below is reproducible for the fcitx half.

**Dead code is not a call site.** Both consumers carry `#if 0` blocks
naming libpinyin symbols; they do not count. `pinyin_get_pinyin_key`
and `pinyin_get_pinyin_string` are already recorded that way for ibus
in `abi-subset.md` §6, and `pinyin_get_raw_full_pinyin` is the fcitx
case (`eim.cpp:377-391`, inside `#if 0`) — a symbol upstream does not
export at all, so a live call would not even link.

**The measured union is 58 symbols** (see PR 3). The enforcement
mechanism is a linker version script exposing exactly that set; new
consumers join it via a documented PR.

## (e) The E2E I/O compatibility rule

The four exceptions say when divergence is permitted. This says what
compliance *means* everywhere else, and it is the rule the other four are
exceptions to.

> **E2E I/O COMPATIBILITY RULE:** For every exported symbol in the
> consumer union, given the same inputs and state, oxpinyin MUST return
> byte-identical outputs to the pinned libpinyin 2.11.91 at `0c5e80e` —
> except where one of the four named exceptions (a)/(b)/(c)/(d)
> explicitly applies. Exporting a symbol that returns a wrong value is
> worse than not exporting it: the consumer gets a silent wrong answer
> instead of a link error. **A stub returning `false` is not compliance —
> it is a defect.**
>
> **Corollary:** if implementing a symbol correctly requires an engine
> change, that engine change is mandatory. The engine serves the ABI, not
> the other way around.
>
> **Verification:** every symbol in the consumer union must have a
> differential probe that drives it with the same input on both libraries
> and asserts byte-identical output — where *output* is the whole
> observable surface, not just the scalar return: return status,
> out-parameters and the data they point to, written lengths, and any
> state transition on the handle. A symbol with no probe is unverified,
> not compliant.

Three consequences worth stating, because each is currently unmet
somewhere:

1. **The version script is not a compliance mechanism.** Exception (d)
   decides which symbols are *in* the union; this rule decides what they
   must *do*. A symbol may be legitimately absent (out of union) or
   legitimately divergent (a named exception). It may not be present and
   wrong.
2. **`pinyin_get_pinyin_key_rest` and `pinyin_get_pinyin_key_rest_positions`
   are defects today**, not gaps: both are exported and both return
   `false` unconditionally (`cursor.rs`, "Provisional"). Under this rule
   they are worse than the five siblings that are simply missing, because
   a linker error is a diagnosis and a `false` is not.
3. **Probe coverage is itself a deliverable.** 58 symbols are in the
   union; the differential suite does not drive all of them. The
   uncovered ones are unverified rather than compliant, and closing that
   gap is work, not bookkeeping.

## The classification table

Every entry in `upstream-divergences.md`, and the one parked entry in
`all-off-tails.md`, classified against (a)/(b)/(c)/(d) or **REVERT
TARGET**. This table drives PR 5.

| # | Entry | Class | Basis |
| --- | --- | --- | --- |
| 1 | Bigram export iterator's pinyin buffer | **(b)** | pin segfaults on a repeated export cycle; stale C buffer aliasing has no safe-Rust reproduction |
| 2 | Public bigram export is a rendering surface | **no ABI divergence** | the C ABI reproduces the rendering; only the internal migration tool reads the raw store |
| 3 | HANYU full pinyin ignores tone digits under `USE_TONE` | **CLOSED** | ported; `PARSE_AUX_IDENTICAL` |
| 4 | Tone digit on an initial-only key aborts the phrase search | **(c)** | pin SIGABRTs on `n4` under `USE_TONE\|PINYIN_INCOMPLETE` (`pinyin_phrase3.h:146-156`) |
| 5a | Scheme setters — double `CUSTOMIZED` (30) | **(c)** | aborts mid-call (`pinyin_parser2.cpp:611-612`) |
| 5b | Scheme setters — double out-of-enum (0, 7–29, 31+) | **REVERT TARGET** | **not an abort**: the parser clears the fallback and returns `false`, the wrapper answers `true`. A half-mutation is reproducible; see the (c) boundary above |
| 5c | Scheme setters — zhuyin `STANDARD_DVORAK` (7) | **(c)** | dvorak arm falls through to `abort()` (`zhuyin_parser2.cpp:291-295`) |
| 5d | Scheme setters — zhuyin / full-pinyin out-of-enum | **(c)** | aborts at `pinyin.cpp:1188` / `pinyin_parser2.cpp:398` |
| 6 | Constraint-aware train without the consistency assert | **(c)** | `train_result3` asserts and aborts on a stale result |
| 7 | `validate_constraint`'s drop test is the span-search shape | **REVERT TARGET** | see below — the arithmetic is basic-ops, not class (a) |
| 8 | Constraints survive every re-parse except the selection-committed one | **REVERT TARGET** | a behaviour choice; upstream never resets on re-parse |
| 9 | The n-best row-choose cursor is the row's own end | **REVERT TARGET** | upstream returns `matrix.size()-1` unconditionally; reproducible |
| 10 | `pinyin_get_sentence` asserts a past-the-rows index | **(c)** | SIGABRTs on a non-empty result set (`pinyin.cpp:1463-1482`) |
| 11 | N-best trellis accumulates `gfloat` log costs | **(a)** | `log()` per step into a `gfloat`; ties decided at the ULP |
| 12 | Predicted-candidate tie order | **REVERT TARGET** | named in PR 5; the target order changes with the backend (see below) |
| 13 | Mid-syllable candidate-lookup offset | **REVERT TARGET** | named in PR 5; empty matrix column vs suffix re-parse |
| 14 | Cursor helpers' `_check_offset` aborts answer `false` | **(c)** | pin SIGABRTs at `pinyin.cpp:2175` |
| 15 | Apostrophe-only input: pin consumes every byte, engine none | **REVERT TARGET** | a parse-length difference (pin 1/2/3, oxpinyin 0), not an abort |
| 16 | `FORCE_TONE` honoured on the full-pinyin seam only | **(d)** | `FORCE_TONE` appears in **neither** consumer's `src/` — 0 hits in ibus-libpinyin 1.16.5 and 0 in fcitx-libpinyin |
| 17 | Literal `0x0` option gating (`jv`/`zon`; `xian` divided-table) | **REVERT TARGET** | named in PR 5; see the unreachability note below |
| 18 | The pinyin index DBMs carry uninitialized struct padding | **(b)** | upstream copies a stack struct's tail padding into the DBM; datagen zeroes it and the reader never touches it |

Totals: **(a)** 1 · **(b)** 2 · **(c)** 6 · **(d)** 1 · **REVERT
TARGET** 7 · closed or not a divergence 2.

### Notes on the three entries whose class was not obvious

**#7 — why `validate_constraint` is not class (a).** The drop test is
`compute_pronunciation_possibility(...) < FLT_EPSILON`
(`phonetic_lookup.cpp:161-164`). Reading the function first-hand
(`phonetic_key_matrix.cpp:534-600`), it is a recursive **sum over every
path** of `PhraseItem::get_pronunciation_possibility` — `gfloat`
addition and a frequency ratio, no transcendental anywhere. That is the
`amplified_frequency` standard, which is ported to 100%, so the
threshold is bit-reproducible and the entry does not qualify under (a).

Reverting it is real work rather than a flag flip: oxpinyin's
`span_finds_token` follows the §3 step-cost model (first path per
token), while upstream sums **all** paths. The revert has to port the
all-paths sum, not just re-thread the comparison.

**#16 — why `FORCE_TONE` is class (d), and why that is not a criticism
of the port.** The double-pinyin parser has a genuinely different
`FORCE_TONE` law (a length-3 gate not nested under `USE_TONE`,
`pinyin_parser2.cpp:412,448`), and oxpinyin implements only the
full-pinyin shape. Measured: `FORCE_TONE` appears **zero times** in
ibus-libpinyin 1.16.5's `src/` and **zero times** in fcitx-libpinyin's
`src/`. Neither reference consumer can set the bit, so the differing
law is unreachable through the drop-in surface.

Class (d) here means *correctly out of scope*, not *wrong*. The pin's
`USE_TONE` branch was ported in #178 and the port is correct and
internally consistent; the full-pinyin seam matches the pin, and the
unported double/zhuyin shapes sit outside the consumer boundary rather
than being an oversight. **This is not a revert target and no work is
owed on it.** The entry stays in the register only so that a future
consumer which does set `FORCE_TONE` finds the analysis already done
instead of rediscovering it — at which point the double/zhuyin law
enters scope and gets ported with a measured differential, as the (d)
rule's "until a new consumer demonstrates a need" clause provides.

**#17 — recorded as a revert target, with the evidence against it
stated.** Both consumers OR the bits unconditionally before every
`pinyin_set_options` call — ibus-libpinyin at `PYLibPinyin.cc:195-196`
and fcitx-libpinyin at `eim.cpp:941` — so **neither can produce the
literal `0x0` option word**, which is the same evidence shape that
puts #16 in class (d). It is classified REVERT TARGET here because the
maintainer's decision names it as one explicitly, and because (d) as
written is scoped to *symbols* rather than to unreachable inputs
generally.

One decision is therefore open, and it is one line: **does (d) cover
consumer-unreachable inputs, or only consumer-uncalled symbols?** If
inputs, #17 moves to (d) and PR 5 drops it. If symbols only, #17 stays
a revert target and PR 5 ports the pin's `0x0` gating. Nothing else in
the table turns on the answer.

## What is not an exception

Stated so the classes are not read as broader than they are:

- **"The Rust is cleaner."** Not a class. Internal structure is free to
  diverge (source policy); externally observable behaviour is not.
- **"Upstream's behaviour is meaningless."** Not a class. #12's
  predicted-candidate order was defended that way; it is a revert
  target.
- **"No frontend does that."** Not a class on its own — that is (d),
  and (d) is scoped to the two named consumers with a call-site grep
  behind it, not to a judgement about what a frontend would plausibly
  do.
- **"A float is involved."** Not class (a) unless a transcendental is
  in the accumulation.
- **"Upstream returns something useless."** Not class (c) unless
  upstream *aborts*. A wrong-but-defined answer is reproducible.

## Consequences for the register

`upstream-divergences.md` keeps its stated purpose — the residue of
what a Rust mechanism prevents — but the four classes are narrower than
what the register accumulated. Class (c) entries in particular are not
language-mechanism residue at all; they are product decisions, and the
register should say so where it currently implies Rust forced them.

PR 5 flips every REVERT TARGET's differential probe from "recorded
divergence" to "must be IDENTICAL". Entry #12 carries an extra step:
Kyoto Cabinet is the reference backend (what distros ship), so the
target order is KC's physical hash walk, which must be established
experimentally — it is **not** the Tkrzw bucket order the current entry
measured.
