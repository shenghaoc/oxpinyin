# Compatibility policy — what oxpinyin may diverge on, and what it may not

Date: 2026-08-28 · Status: **policy** (maintainer-decided; this document
records the decision and classifies the existing register against it) ·
Branch: `claude/pr2-compatibility-policy` (#203). **Amended 2026-09-06:**
the classification table re-synced with `upstream-divergences.md` at
`87f25055` — every register entry now has a row, closures since
2026-08-28 are recorded, and the "PR 5" the original text named is
`docs/findings/revert-plan.md` (#209, the work order), which never
became a PR of its own; the reverts landed one by one (see the table).
**Amended 2026-09-12 (maintainer):** the E2E rule's pin is libpinyin
2.11.92 at `074a2219` — the repository is feature-complete, at measured
parity under this policy's exceptions with one open defect (row 30, the
pinyin facade's chewing batch `FORCE_TONE` seam), and the pin was moved
to libpinyin's latest commit on 2026-09-06
(`docs/testing/oracle-environment.md`); and the goal amendment's task-9
gap closed on 2026-09-09, and row 2's basis no longer cites the
cancelled migration tool. **Amended 2026-09-09:** the goal's byte-level
guarantee is per KV
backend family — same-backend pairs (Kyoto Cabinet↔Kyoto Cabinet,
tkrzw↔tkrzw, oxpinyin↔libpinyin either direction) interoperate
seamlessly, user data included; data loss when the KV database backend
actually changes is taken for granted (maintainer ruling; recorded in
place under "The goal this policy serves").

## The goal this policy serves

oxpinyin is a **drop-in replacement**: rename the built shared object to
`libpinyin.so.15`, put it on the library path, and existing consumers
work against the data already on the system. Not a compatible
reimplementation a consumer is ported to — the same binary interface,
the same file formats, the same observable behaviour, with the consumer
unchanged and unaware.

> **Amended 2026-09-09 — same-backend user data is seamless (maintainer
> ruling).** The guarantee is per KV database backend family: an
> oxpinyin built on Kyoto Cabinet and a libpinyin built on Kyoto
> Cabinet — likewise the tkrzw pair — read and write the same user
> dir, and a swap in either direction carries the learned data with
> it. oxpinyin reads the user state libpinyin left (`user_bigram.db`,
> `user_pinyin_index.bin`, `user_phrase_index.bin`, `user.bin`, the
> `*.dbin` diff logs, `user.conf` — characterized at the pin
> `074a2219`: `pinyin_internal.h:55-66`, `_write_files`/
> `_rename_files` `pinyin.cpp:922-1130`, the per-library user
> filenames in `data/table.conf.in`), and `pinyin_save` writes back
> what a same-backend libpinyin picks up (drop-in task 9, reopened
> with exactly this scope; landed 2026-09-09 —
> `docs/findings/user-store.md` §11, measured both ways on Kyoto
> Cabinet and tkrzw by `tools/oracle/user-dir-round-trip.sh`; until
> then the runtime opened its own `user_store.<ext>` and left those
> files untouched, and the maintainer confirmed the closure 2026-09-12). **Data loss when the KV database backend actually
> changes is taken for granted** — a BDB-built libpinyin (Debian
> stable, Ubuntu) against oxpinyin's KC/tkrzw builds, redb/LMDB builds
> with no libpinyin counterpart, KC↔tkrzw transitions. There the
> ecosystem's own fresh-start norm applies: libpinyin discards user
> data across its own backend switches (Debian's 2.11.91-1
> BerkeleyDB→Tkrzw carried a `debian/NEWS` warning: "all previous
> user data will be lost after the upgrade") and across
> model-version bumps (`check_format` → `_clean_user_files`,
> `pinyin.cpp:172-199`), and it migrated nothing from its own
> predecessors (W7, `legacy-migration.md`). Across a genuine backend
> change the migration path is the library's value-level interchange —
> the `pinyin_*_add_phrase*` import trio and the
> `pinyin_*_get_(bigram_)phrases` export iterators (W6-T7) — which
> carries the user *phrase dictionary* but not the trained *bigram*
> (upstream's ABI has no bigram import); that residual asymmetry is a
> cross-backend concern only. **Consequence for the E2E rule below:**
> "state" includes the on-disk user state of a same-backend user dir;
> a user dir in another backend's format is outside the compared
> state, and a fresh start there is not a divergence. The BerkeleyDB
> path (task 10) stays shelved behind its existing consumer-need gate
> — a BDB revival would bring system and user files at once, on a
> distro set whose own backend switch already discarded the user
> data.

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

> **RETIRED (maintainer decision, 2026-09-06).** This class was written
> for the 51/58-symbol consumer-union contract and became moot the
> moment the target changed to the full ABI with `pinyin.h` and
> `zhuyin.h` copied verbatim (W8, 79/79; libzhuyin, 52/52): every
> exported symbol and every option bit is in scope, whether or not a
> known consumer reaches it. The text below is kept as the record of
> what (d) meant; no new entry may be classified (d), and the two rows
> that were (16, 17) are re-dispositioned in the table. "No consumer
> calls it" remains useful as a *priority* signal, never as an
> exception.

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

**The measured union is 58 symbols** (`abi-subset.md` §1 plus the fcitx
grep). Since W8 closed (2026-08-30) the shipped object exports all 79
`pinyin_*` symbols from `libpinyin.ver` live (`abi-subset.md` §6), so
the union no longer bounds the *export* set; it bounds the E2E probe
obligation below and the (d) scope decision. New consumers extend the
union via a documented PR.

## (e) The E2E I/O compatibility rule

The four exceptions say when divergence is permitted. This says what
compliance *means* everywhere else, and it is the rule the other four are
exceptions to.

> **E2E I/O COMPATIBILITY RULE:** For every exported symbol in the
> consumer union, given the same inputs and state, oxpinyin MUST return
> byte-identical outputs to the pinned libpinyin 2.11.92 at `074a2219`
> (the pin since 2026-09-06; the rule was written at 2.11.91 / `0c5e80e`,
> against which the candidate surface is byte-identical —
> `docs/testing/oracle-environment.md`) —
> except where one of the named exceptions (a)/(b)/(c) ((d) retired 2026-09-06)
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
   were defects when this was written** — exported, returning `false`
   unconditionally. Closed with the W8 79/79 work: both are implemented
   against a per-instance key-rest slot
   (`crates/oxpinyin-capi/src/cursor.rs`). The rule they illustrated
   stands: a linker error is a diagnosis and a `false` is not.
3. **Probe coverage is itself a deliverable.** 58 symbols are in the
   union; the differential suite does not drive all of them. The
   uncovered ones are unverified rather than compliant, and closing that
   gap is work, not bookkeeping.

## The classification table

Every entry in `upstream-divergences.md`, and the one parked entry in
`all-off-tails.md`, classified against (a)/(b)/(c)/(d), **REVERT
TARGET** (reproducible, not yet reproduced, no blocker but the work),
**OPEN DEFECT** (reproducible, blocked on a STOP — an engine-interface
ask), **CLOSED** (reproduced, or proven equivalent on the pinned data),
or **no ABI divergence**. Rows 1–18 are the 2026-08-28 table with their
status brought to `87f25055`; rows 19–31 are the entries the register
gained or that the original table skipped (row 19 landed with the
074a2219 pin bump). The work order for the
revert targets is `revert-plan.md`.

| # | Entry | Class | Basis |
| --- | --- | --- | --- |
| 1 | Bigram export iterator's pinyin buffer | **(b)** | pin segfaults on a repeated export cycle; stale C buffer aliasing has no safe-Rust reproduction |
| 2 | Public bigram export is a rendering surface | **no ABI divergence** | the C ABI reproduces the rendering; nothing in-tree reads the raw store — the internal migration tool that would have was cancelled (`legacy-migration.md`, SHELVED on `feat/w7-t2-legacy-migrate`; maintainer confirmation 2026-09-12) |
| 3 | HANYU full pinyin ignores tone digits under `USE_TONE` | **CLOSED** | ported; `PARSE_AUX_IDENTICAL` |
| 4 | Tone digit on an initial-only key aborts the phrase search | **(c)** | pin SIGABRTs on `n4` under `USE_TONE\|PINYIN_INCOMPLETE` (`pinyin_phrase3.h:146-156`) |
| 5a | Scheme setters — double `CUSTOMIZED` (30) | **(c)** | aborts mid-call (`pinyin_parser2.cpp:611-612`) |
| 5b | Scheme setters — double out-of-enum (0, 7–29, 31+) | **REVERT TARGET** | **not an abort**: the parser clears the fallback and returns `false`, the wrapper answers `true`. A half-mutation is reproducible; see the (c) boundary above |
| 5c | Scheme setters — zhuyin `STANDARD_DVORAK` (7) | **(c)** | dvorak arm falls through to `abort()` (`zhuyin_parser2.cpp:291-295`) |
| 5d | Scheme setters — zhuyin / full-pinyin out-of-enum | **(c)** | aborts at `pinyin.cpp:1188` / `pinyin_parser2.cpp:398` |
| 6 | Constraint-aware train without the consistency assert | **(c)** | `train_result3` asserts and aborts on a stale result |
| 7 | `validate_constraint`'s drop test is the span-search shape | **CLOSED** (was REVERT TARGET) | 4c2fe02b, 2026-08-29: the `FLT_EPSILON` drop boundary is unreachable on model20 (max per-token total 2,945,481 < 2²³), so the two tests are observably equivalent on the pinned data; the arithmetic still differs (see the note below) |
| 8 | Constraints survive every re-parse except the selection-committed one | **CLOSED** (was REVERT TARGET) | #217 (`fix/revert-r5-constraint-reset`): constraints survive a selection-committed re-parse; frozen pins bit-identical |
| 9 | The n-best row-choose cursor is the row's own end | **CLOSED** (was REVERT TARGET) | eca8d43b: every `NBEST_MATCH_CANDIDATE` choose answers `parsed_len` — upstream's `matrix.size()-1` in the active parse mode's coordinates |
| 10 | `pinyin_get_sentence` asserts a past-the-rows index | **(c)** | SIGABRTs on a non-empty result set (`pinyin.cpp:1463-1482`) |
| 11 | N-best trellis accumulates `gfloat` log costs | **(a)** | `log()` per step into a `gfloat`; ties decided at the ULP. **FROZEN** as a permanent Stage-1 divergence (maintainer ruling 2026-09-02, re-frozen 2026-09-04 at 491/396/390 of 496) |
| 12 | Predicted-candidate tie order | **CLOSED** (was REVERT TARGET) | superseded by P6 (345af16d, 2026-09-02): on KC and tkrzw the runtime walks the pin's own phrase DBM, so `pred-order-diff` is IDENTICAL on the pin's `data/` (1588 lines, 0 mismatches) — the KC hash-walk experiment the original row asked for is moot. On redb/LMDB the text-ascending *defined* order stands (maintainer decision 2026-08-25); those containers are not the pin's and are outside the drop-in surface. `ROADMAP.md` records the same disposition |
| 13 | Mid-syllable candidate-lookup offset | **CLOSED** (was REVERT TARGET) | the pin's empty-column law is reproduced (register entry re-titled "closed"; the C2 residue closed 2026-08-29 per `uncovered-surface-differentials.md` phase E) |
| 14 | Cursor helpers' `_check_offset` aborts answer `false` | **(c)** | pin SIGABRTs at `pinyin.cpp:2175` |
| 15 | Apostrophe-only input: pin consumes every byte, engine none | **CLOSED** (was REVERT TARGET) | 678f3259, 2026-08-26 (B2, the parser-termination class): `SegmentGraph` propagates each apostrophe one byte, counted — `'''` consumes 3, pinned in `graph.rs`. Closed two days *before* this policy was written; the register entry had not been updated (amended 2026-09-06) |
| 16 | `FORCE_TONE` honoured on the full-pinyin seam only | **CLOSED** (was (d); (d) retired 2026-09-06) | the zhuyin batch (1671954), double-pinyin batch (#289) and one-key seams have since been ported; the last seam, the pinyin facade's chewing batch, is row 30 |
| 17 | Literal `0x0` option gating (`jv`/`zon`; `xian` divided-table) | **REVERT TARGET** | not a register entry: the parked paragraph in `all-off-tails.md`. With (d) retired the consumer-unreachability argument is moot: port the pin's gating at the literal `0x0` word (empty guess for `jv`/`zon`; drop the divided-table inventory without `USE_DIVIDED_TABLE`), probe via `run-option-sweep.sh` at that word |
| 18 | The pinyin index DBMs carry uninitialized struct padding | **(b)** | upstream copies a stack struct's tail padding into the DBM; datagen zeroes it and the reader never touches it |
| 19 | `pinyin_get_character_offset`'s recursion asserts answer `false` | **(c)** | pin SIGABRTs at `pinyin.cpp:3152` / `:3166` (and the #14 family's range and `_check_offset` asserts); the unbounded `cached_tokens` read (`:3172`) is a (b) sub-shape answered as a deterministic miss |
| 20 | One bigram-prediction row differs on the pin's own data (trellis residual) | **(a)** | downstream of row 11: the trained count for `测测 → 你` straddles the `m_count ≥ 10` filter because the trellis residual picks a different phrase; one `union-diff` line, everything else identical |
| 21 | The single-key surface aborts the pin where oxpinyin answers `false` | **(c)** | `assert` on apostrophes in `parse_one_key` (`pinyin_parser2.cpp:170`), `assert(index < PHRASE_INDEX_LIBRARY_COUNT)` on unload (`pinyin.cpp:499`), empty-input over-reads; pinned in `crates/oxpinyin-capi/tests/abi/keys.rs` |
| 22 | Empty-string phrase lookup SIGFPEs the pin | **(c)** | `pinyin_phrase_segment(instance, "")` divides by the zero span length; oxpinyin answers `false`. A crash on caller input is the (c) shape whether the signal is ABRT or FPE |
| 23 | Sanitizer scope on the tkrzw shim CI · native data-file naming · R1 measured on the compat paths | **no ABI divergence** | three dated records, not behaviour entries: a CI-instrumentation note, a file-naming decision (`installed-naming.md`), and a measurement whose subject was removed (its SUPERSEDED banner is itself amended 2026-09-06 — P6 restored direct reads of libpinyin's files) |
| 24 | zhuyin batch `FORCE_TONE` law | **CLOSED** | 1671954: `ZhuyinParser::parse_with_options` honours the three per-keyboard shapes |
| 25 | zhuyin candidate-tag grouping + `after(consumed)` terminal offset | **CLOSED** | both halves closed by the display-law collapse and the builder terminal mapping (amended 2026-08-31) |
| 26 | zhuyin before-cursor candidate window | **CLOSED** | window builder closed (c2ad5925); the residual — a row whose span starts after the offset was constrained as `[0, offset)` — closed by #374 (`Candidate::span_start` = upstream's `m_begin`, constraint `[m_begin, m_end)`, `zhuyin_choose_candidate` answers `m_begin`, `zhuyin.cpp:1660`) and **measured IDENTICAL on the pin-built libzhuyin at 074a2219** on 2026-09-08: the three-input choose battery byte-identical (register entry, second amendment) |
| 27 | zhuyin multi-syllable candidate construction | **CLOSED** | the divergence was the pinyin string-fill law, not the construction model (amended 2026-08-31) |
| 28 | zhuyin n-best trellis constants `<1, 1>` vs the engine's `<2, 3>` | **CLOSED** in code (#374) | per-session `NbestShape` (`PINYIN` = `<2, 3>`, `ZHUYIN` = `<1, 1>`), set by both zhuyin facades at instance allocation; not observable through today's libzhuyin candidate surface, so no gate moves |
| 29 | zhuyin `FORCE_TONE` / `ZHUYIN_INCOMPLETE` default | **no ABI divergence** | `CapiContext::try_open` seeds the pin's `USE_TONE \| FORCE_TONE`; entry kept as analysis for a future consumer |
| 30 | pinyin-facade chewing batch seam does not forward `FORCE_TONE` | **OPEN DEFECT** | the register says it: no class fits, a defect to close; `ROADMAP.md` carries it as the bopomofo SPEC's one open implementation item. Observable only under a caller-set `FORCE_TONE` (pin consumes 0 on toneless `su`, oxpinyin 2). Fix shape: forward `inst.options().bits()` through `parse_with_options` plus a `FORCE_TONE` profile in `chewing-diff.c`. Not STOP-gated — the same one-line shape #289 used on the double-pinyin seam |
| 31 | redb write-side emptiness probe creates the table it probes | **no ABI divergence** | a redb API constraint below the store traits; nothing above them observes it. Stage-2 store-trait note, not a compatibility entry |

Totals at `2a99761a` (2026-09-06, oracle pin 074a2219): **(a)** 2 ·
**(b)** 2 · **(c)** 10 · **(d)** 0 (class retired, see below) · **REVERT
TARGET** 2 (rows 5b, 17) · **OPEN DEFECT** 1 (row 30) · **CLOSED** 13
(rows 3, 7, 8, 9, 12, 13, 15, 16, 24, 25, 26, 27, 28 — 26 measured
identical on the pin 2026-09-08; 28 in code via #374, not observable
through today's surface) · **no ABI divergence** 4 rows (2, 23,
29, 31).

The 2026-08-28 totals were (a) 1 · (b) 2 · (c) 6 · (d) 1 · REVERT
TARGET 7 · closed or not a divergence 2. Of the seven revert targets,
five closed by reproduction or proven equivalence (7, 8, 9, 13, 15) and
one was superseded by P6 (12); 5b and 17 remain, and 17 is no longer
conditional.

### What is still owed, in order

1. **Row 30** — the chewing batch seam's `FORCE_TONE` forward. One line
   plus a differential profile; no ask needed.
2. **Row 5b** — the double out-of-enum half-mutation. Reproducing a
   half-mutation that lies about success is the (c) boundary case the
   policy singles out; the maintainer named it a revert target and it
   has not moved.
3. **Row 17** — port the pin's `0x0` gating; unconditional since (d)
   was retired.
4. **Rows 26 and 28** — landed in code (#374) under the maintainer's
   2026-09-06 approval, copying libpinyin's source; row 26's
   three-input oracle battery ran on 2026-09-08 and is byte-identical.
   Nothing is owed on either.

### Notes on the three entries whose class was not obvious

**#7 — why `validate_constraint` is not class (a), and how it closed
anyway.** The drop test is
`compute_pronunciation_possibility(...) < FLT_EPSILON`
(`phonetic_lookup.cpp:161-164`). Reading the function first-hand
(`phonetic_key_matrix.cpp:534-600`), it is a recursive **sum over every
path** of `PhraseItem::get_pronunciation_possibility` — `gfloat`
addition and a frequency ratio, no transcendental anywhere. That is the
`amplified_frequency` standard, which is ported to 100%, so the
threshold is bit-reproducible and the entry does not qualify under (a).

Reverting it would be real work rather than a flag flip: oxpinyin's
`span_finds_token` follows the §3 step-cost model (first path per
token), while upstream sums **all** paths. It closed without the port
(4c2fe02b): the threshold can only bite when a token's pronunciation
total exceeds 2²³, and the largest total in model20 is 2,945,481, so on
the pinned data the drop boundary is unreachable and the two tests are
observably equivalent. The register entry carries the scan. A different
model with a total above 2²³ would reopen it.

**#16 — why `FORCE_TONE` is class (d), and why that is not a criticism
of the port.** The double-pinyin parser has a genuinely different
`FORCE_TONE` law (a length-3 gate not nested under `USE_TONE`,
`pinyin_parser2.cpp:412,448`), and oxpinyin implements only the
full-pinyin shape. Measured: `FORCE_TONE` appears **zero times** in
ibus-libpinyin 1.16.5's `src/` and **zero times** in fcitx-libpinyin's
`src/`. Neither reference consumer can set the bit, so the differing
law is unreachable through the drop-in surface.

Class (d) here meant *correctly out of scope*, not *wrong*. The pin's
`USE_TONE` branch was ported in #178 and the port is correct and
internally consistent; the full-pinyin seam matches the pin, and the
double/zhuyin shapes — unported when this note was written — sat
outside the consumer boundary rather than being an oversight. Since
then the zhuyin batch seam (1671954, row 24) and the double-pinyin
batch seam (#289) were ported anyway, and with (d) retired
(2026-09-06) the last one, the pinyin facade's chewing batch seam, is
row 30's open defect. The note is kept as the record of the original
reasoning; row 16 carries the current status.

**#17 — recorded as a revert target, with the evidence against it
stated.** Both consumers OR the bits unconditionally before every
`pinyin_set_options` call — ibus-libpinyin at `PYLibPinyin.cc:195-196`
and fcitx-libpinyin at `eim.cpp:941` — so **neither can produce the
literal `0x0` option word**, which is the same evidence shape that
puts #16 in class (d). It is classified REVERT TARGET here because the
maintainer's decision names it as one explicitly, and because (d) as
written is scoped to *symbols* rather than to unreachable inputs
generally.

The decision that was open here — whether (d) covers
consumer-unreachable inputs or only uncalled symbols — was overtaken on
2026-09-06 by the retirement of (d) itself (see the banner above): row
17 is a plain revert target and the pin's `0x0` gating gets ported.

## What is not an exception

Stated so the classes are not read as broader than they are:

- **"The Rust is cleaner."** Not a class. Internal structure is free to
  diverge (source policy); externally observable behaviour is not.
- **"Upstream's behaviour is meaningless."** Not a class. #12's
  predicted-candidate order was defended that way; it is a revert
  target.
- **"No frontend does that."** Not a class. It was the basis of (d)
  while the consumer-union contract stood; with (d) retired it is at
  most a reason to schedule a port later, never to skip it.
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

The work order (`revert-plan.md`) flips each REVERT TARGET's
differential probe from "recorded divergence" to "must be IDENTICAL" as
it lands. Entry #12's extra step — establishing Kyoto Cabinet's physical
hash walk experimentally — was overtaken by P6: reading the pin's own
DBM reproduces the pin's own walk, and `pred-order-diff` is IDENTICAL on
KC without any order having been modelled.
