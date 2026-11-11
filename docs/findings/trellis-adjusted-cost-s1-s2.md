# Pinyin trellis adjusted-cost comparison

Date: 2026-10-28 UTC (`date -u`). Policy: [witnessed algorithm-precondition
violations, S1/S2](compatibility-policy.md#algorithm-precondition-violations).
This is the registered product-level ruling, not the language-artefact exception.
The pin is libpinyin 2.11.92 at `074a2219c90feaf962d0d24f034514033ece5f99`.
Its named source blobs were read at that revision. Base: `2506b264`.

## Scope and derivation

Recorded evidence: PR B of #1378 measured these four BDB observation pairs
again on 2026-10-29 UTC against main `9015a178`, and all four are unchanged.

The integer cost derivation below describes the original implementation.
The current code (PR B) implements the same ruled ordering on the float scale:

- `S = poss - length * P`, with `P = 1.2_f32.ln()` (#1416; the pin's folded
  `0x3e3ab283` on this host, class (a) elsewhere);
- separate f32 multiplication and subtraction;
- larger S first, NaN last, and signed zeros equivalent.

The node compares S alone. S1/S2 complete ties with earlier batch position.
See the approved float-scoring record.

`src/lookup/phonetic_lookup.h:39,65-91 at 074a2219` provides the adjacent-length
margin. The ruling completes it as `cost_add(cost, 263 * length)`, best first:
negative infinity, finite ascending, positive infinity, NaN. Multiplication
widens the `u32` length to `i64` first. Its maximum is 1,129,576,398,585.
Finite addition saturates at `i64::MAX - 1` and stays finite. Infinities and
NaN stay special. Saturation can create additional equivalent costs.

The node heap uses this comparison alone (`src/lookup/phonetic_lookup_heap.h:
25-81 at 074a2219`). Its full-node comparison rejects a tie, and an
underfilled node still fills its spare slot. The front-eviction defect remains.
S1 beam selection (`src/lookup/phonetic_lookup.h:770-771 at 074a2219`) and S2
final-tail selection (`:331-335`) use the total key of adjusted cost and earlier
batch position. Batch positions follow node creation order and physical slot
order before any selection (`:272-297`). They never replace the physical slot
that is copied into the `sub` of a successor and read during extraction.

Shipping changes are confined to `nbest.rs`. The literal heap routines, tail
comparison and merge, zhuyin comparison, cost arithmetic, external calls,
C ABI, ownership and persistent formats are unchanged. Time and space
complexities are unchanged. `BeamEntry` adds one batch-position integer per
candidate; node storage does not grow.

## CI expectation audit

Recorded evidence: this section is the audit of the original S1/S2 repair.
The six-residual expectations in it are obsolete. The approved measurement of
PR B replaces them with the 156 unchanged selection residuals of current main
([the current sentence declaration, §12](sentence-surface.md)).

Before implementation, the audit inspected `ci.yml`, `store-backends.yml`,
`verify-nightly.yml`, the since-retired coverage workflow and the nextest configuration. The ordinary
workspace tests include `nbest.rs`, the engine's constrained-session tests,
mini-vocabulary decoding/replay, runtime assembly and C ABI pipeline tests.
The latter expectations remain unchanged. The following changed expectations
are each licensed by the linked S1/S2 amendment:

| Changed expectation | Attribution |
| --- | --- |
| NaN at length 3 loses to finite cost 10 at length 3, formerly a tie | [S1/S2 special-cost completion](compatibility-policy.md#algorithm-precondition-violations) |
| NaN at length 3 loses to finite cost 10 at length 4, formerly preferred | [S1/S2 special-cost completion](compatibility-policy.md#algorithm-precondition-violations) |
| Finite cost 10 at length 4 no longer loses to NaN at length 3 | [S1/S2 special-cost completion](compatibility-policy.md#algorithm-precondition-violations) |
| `(length=3,cost=0)` no longer loses to `(2,263)`; they tie | [S1/S2 threshold equivalence](compatibility-policy.md#algorithm-precondition-violations) |
| Former cycle's `(6,68730)` no longer loses to `(4,70848)` | [S1/S2 adjusted-cost ordering](compatibility-policy.md#algorithm-precondition-violations) |

The 500-row and 8,592-input live sentence checks require real-table inputs.
Ordinary CI ignores them. Their active tests validate fixture structure and
the existing six-residual membership, not new decoder outputs. Frozen corpus
generation and capture-replay tests validate their committed inputs/captures.
No frozen corpus or golden is regenerated.

The focused BDB ranking selection used the complete sentence-rank
observer, including the captured S1/S2 spellings and table-off controls
(recorded evidence). Every intentionally moved case is listed below and
reported `POLICY-ACCEPTED`; other observations failed. The broader
behavioral campaign, including corpus-wide movements, is PR4.

## Exact verified-cell expectations

Status: accepted under the S1/S2 ruling. Each case is accepted on every
verified cell (BDB, KC and sound-tkrzw) unless its row names fewer cells. The
fresh-profile diagnostic is unchanged.

| Moved case | Attribution |
| --- | --- |
| `nbest-user-table-order-pinyin` | S2: equal-count, equal-length final candidates retain earlier candidate-batch position, so 靐靐 precedes 龘龘; the pin heap reverses that tie. |
| `table-conf-lambda-one-and-a-half` | S1/S2 special-cost completion: finite blended paths precede NaN unigram paths in beam and final-tail selection; only sentence/NBEST-prefix observations change. |
| `table-conf-lambda-two` | S1/S2 special-cost completion: finite blended paths precede NaN unigram paths in beam and final-tail selection; only sentence/NBEST-prefix observations change. |
| `table-conf-lambda-million` | S1/S2 special-cost completion: finite blended paths precede NaN unigram paths in beam and final-tail selection; only sentence/NBEST-prefix observations change. |
| `table-conf-row-removed-gb` | S2 tie rule: with the GB dictionary row removed, 蠔, 獆, 皥 and 滈 tie at count 2 among the final tails. The pin breaks the tie by heap pop order (`get_top_results`, `src/lookup/phonetic_lookup.h:142-171 at 074a2219`, over the candidates `get_candidates` lists in node and slot order, `:272-297`, from `get_tails`, `:330-341`) and gets 獆; the ruled key breaks it by earlier batch position and gets 蠔. |
| `table-conf-not-used-gb` | The same S2 tie rule with `gb_char.dbin NOT_USED`: the pin's heap pops 獆, the ruled key keeps 蠔. |
| `axis-c-layout-pinyin-3-00000000`, `axis-c-layout-pinyin-4-00000000` | Registered on tkrzw only. S2 tie rule on the syllable ri (`u` under scheme 3, `n` under scheme 4). 鈤 (`gbk_char.bin`, slot 8963) and 馹 (`gb_char.bin`, slot 5541) each have unigram 1, one pronunciation of frequency 3 for the key `ri`, and no sentence-start bigram row (frequency 0 of 13,857,886) in `libpinyin-data` 2.11.92-1. The pin therefore computes the same `m_poss` for both (`src/lookup/phonetic_lookup.h:650-663`; `src/storage/phrase_index.h:136-165`), and `trellis_value_less_than` (`:55-83`) orders neither first. It breaks the tie by heap pop order (`get_top_results`, `:142-171`, over the candidates `get_candidates` lists in node and slot order, `:272-297`, from `get_tails`, `:330-341`) and puts 鈤 second among the n-best sentences; the ruled key breaks it by earlier batch position and puts 馹 there. The other seven candidates and the nine-candidate multiset are identical. |
| `table-conf-lambda-negative-half` | S1/S2 special-cost completion: a negative λ leaves `bigram_lambda` negative and `unigram_lambda = 1 − λ` above one (`src/lookup/phonetic_lookup.h:712-713 at 074a2219`), so unigram steps stay finite (`:663`) while a blended step (`:692-693`) takes the log of a negative number, NaN, wherever −λ·b > (1−λ)·u. The ruled order puts NaN last; the pin's comparator does not order NaN at all. |
| `table-conf-lambda-negative-two` | The same special-cost completion at λ = −2. |
| `s2-nbest-longer-normal-precedence` | S2 changes the selected sentence tails for `xian`; `甲乙` becomes NBEST instead of NORMAL. The sentence prepend and NBEST-over-phrase precedence (`src/pinyin.cpp:1934-1947,2116-2125 at 074a2219`) remove its duplicate phrase row. The before-guess and sentence-suppressed lists remain identical. Registered on BDB only. |

For the tie case, the user-table reading order and equal counts are established
by `nbest_user_table_order`; no intermediate beam follows the complete
two-character user phrase before S2. For the three lambda cases, the negative
unigram coefficient produces NaN step costs (`src/lookup/phonetic_lookup.h:
651-663,679-693,712-713 at 074a2219`). `expand_entries` expands every bigram
predecessor and only the beam head for the independent unigram pass, so
S1 ordering propagates into the final range. S2 uses the same explicit
finite-before-NaN rule. These cases jointly cover S1 and S2; this record
does not claim that their effects can be isolated from each other. The
normal-candidate frequency comparator, lambda acceptance, training/save
flags, serialized-file observations and diagnostics remain unchanged.

Recorded evidence: the PR B sweep on main `9015a178` and PR B found identical
complete observations for the seven additional rows above. They follow the
existing S1/S2 ruling. The three longer-normal fixtures of that sweep plant a
sentence-start bigram of 1,000,000 / 1,000,000 for library-6 xi'an → 甲乙. The
pin's additive merge feeds that tail beside the one-key tails. Their `at_end`,
`without_sentence` and `sentence_suppressed` observations stay pin-equal.
The #913/#1398 split replaces these three registrations with reduced
fixtures and the separate S2 witness below.

After #1400 merged, PR B rebased onto `9789880a`. The four GB/negative-lambda
registrations and KC/tkrzw entries for the three positive-lambda cases now
come from main. Their pin and subject results are identical to PR B's
prior observations; float scoring does not change them. PR B adds no duplicate
registration or replacement record for those seven cases. The original BDB
S1/S2 pairs are also unchanged. KC/tkrzw user-table-order pairs retain PR B's registrations. The three
longer-normal registrations are replaced by the #913/#1398 split below.

The historical performance and size results were not rerun for this rebase.

The #1398 split preserves the former dynamic `longer-normal-*` full-model
observation as `s2-nbest-longer-normal-precedence`. This witness retains the
full model and its 757-row candidate lists because it protects the S2 selection
interaction that contaminated the deduplication regression. On the pin, the sentence rows are 县
and 现 and 甲乙 remains NORMAL at the end of the list. On the subject, the
sentence rows are 甲乙 and 县 and NBEST precedence removes the NORMAL 甲乙.
Only `with_sentence` differs; the other three observed lists agree.

The three original `longer-normal-*` IDs instead use `items_system(...,
isolated=True)`: exactly two 100-count items for 甲乙 (`xi1'an1` and
`xian1'ni1`), only library 1, empty addon indexes and system bigram, and one
sentence-start user bigram of count 1 to the NORMAL token. Before the guess
and with sentence rows suppressed, the dynamic case is `[(2, 甲乙)]`, the
equal-frequency case is `[(7, 甲乙)]`, and the LONGER-disabled control is
`[(2, 甲乙)]`. After the guess, all three are `[(1, 甲乙)]`. At the end cursor,
the first two are `[(7, 甲乙)]` and the control is empty. These expectations
follow `src/pinyin.cpp:1845-1866,1916-1927,2116-2138,2292-2300 at 074a2219`.
There is only one complete sentence path, so S2 cannot select a different text.

## Verification

This section is recorded evidence. The 20-case BDB selection reports 20 MATCH on base production code and
16 MATCH plus four POLICY-ACCEPTED on the repair. No other selected observation
moves. The four witness/table-off cases and all three zhuyin controls remain
MATCH. This conclusion covers the explicit selection below, not a corpus sweep.
The base run uses the same four additional case registrations, without changing
base production code or adding policy exemptions.

A later full bdb sweep found more cases that the same repair moves:
`table-conf-row-removed-gb`,
`table-conf-not-used-gb`, `table-conf-lambda-negative-half` and
`table-conf-lambda-negative-two` (#1400). They are registered above with
their bdb, kc and tkrzw pairs, and the three lambda cases of the original
selection gained their kc and tkrzw pairs at the same time. The focused
seven-case registration check reports 7 POLICY-ACCEPTED on each of bdb, kc
and tkrzw, with no policy diagnosis. All retained entries are unchanged from
the previous #1400 head. The three `longer-normal-*` cases were excluded
from #1400, and the #913/#1398 split removes their PR B registrations.

The three `longer-normal-*` cases are not registered
here; their reduced #913/#1398 fixture reports 3 MATCH on BDB, KC and tkrzw. The separate
`s2-nbest-longer-normal-precedence` witness reports POLICY-ACCEPTED on BDB
with the difference described above. On the original
#1411 head, removing the final
`dedup_ranked_by_text(collected, &keys)` call in `Session::refresh_candidates`
(`session/lookup.rs`). On the original #1411 head, it makes the dynamic and
equal-frequency cases fail their complete-list assertions (INVALID: the pin
exits 0 and the subject exits 1). The LONGER-disabled control stays MATCH. The
mutation is reverted before the final gate. The reduced lists contain at most one row
instead of 757; the two-item library image is 91 bytes. No fixture file is added.

The isolated system bigram uses the existing `d18_bigram_db` native writer.
KC attaches it as HashDB (`src/storage/ngram_kyotodb.cpp:110-119 at 074a2219`),
while the user bigram is a StashDB snapshot (`:53-90`). Tkrzw attaches HashDBM
(`src/storage/ngram_tkrzwdb.cpp:86-98 at 074a2219`). The cloned source blobs
were verified against that pin. Both the pin and oxpinyin open the resulting
system containers in the selected cases. Before the fixture correction at
`21397188`, KC's three cases were INVALID: the pin exited 0 but oxpinyin failed
initialization on the snapshot. No S2 result or case registration changed.

| Cell | Three #913 cases before | Three #913 cases after | Separate S2 witness before / after |
| --- | --- | --- | --- |
| BDB | 3 MATCH | 3 MATCH | POLICY-ACCEPTED / POLICY-ACCEPTED |
| KC | 3 INVALID | 3 MATCH | SKIP / SKIP (unregistered) |
| tkrzw | 3 MATCH | 3 MATCH | SKIP / SKIP (unregistered) |

The 68 fixture and case-module harness tests, diff check and formatting pass.
These checks used the supported Debian cloud environment. Tkrzw 1.0.34 passed
the health probe; `ldd` showed the pin and subject loading the same sound
library. No Ubuntu host was used.

Original repair verification: 196 unit tests, 39 integration tests and four doctests
passed. The focused contract-driver and verdict harnesses passed 163 and 35
tests respectively. Formatting and engine Clippy passed. No existing session,
replay or frozen-corpus golden changed; the five comparator expectation changes
are listed above.

```sh
cargo fmt --all --check
cargo clippy --locked -p oxpinyin-engine --all-targets -- -D warnings
cargo test --locked -p oxpinyin-engine
```

The #1400 registration check used the supported cloud setup on native Debian
trixie Linux amd64, repository Rust, debug opt-level 1 libraries, verified
oracle/data manifests for each cell, and the sound tkrzw 1.0.34 runtime.

The compatibility run used Debian trixie Linux amd64 (recorded evidence), the matching
unpatched BDB reference and verified model20 data. Native tests and shipping
builds used Ubuntu 26.10 x86_64 and repository Rust 1.97.1.

Both revisions' shipping files were built in the same run with the BDB
release-stage recipe and equivalent source-path remapping, and verified
stripped. Each regular shipping file is counted once (#864):

| Shipping library | Base `2506b264` | Repair | Increase |
| --- | ---: | ---: | ---: |
| `libpinyin.so.15.0.0` | 2,478,832 bytes | 2,478,832 bytes | 0 |
| `libzhuyin.so.15.0.0` | 2,228,976 bytes | 2,228,976 bytes | 0 |

Run separately at base and repair, using each checkout's own target directory:

```sh
RUSTFLAGS="--remap-path-prefix=$PWD=/oxpinyin" just packaging release-stage bdb --prefix=/usr --libdir=/usr/lib
file target/release-stage/bdb/usr/lib/libpinyin.so.15.0.0 target/release-stage/bdb/usr/lib/libzhuyin.so.15.0.0
stat -c '%n %s' target/release-stage/bdb/usr/lib/libpinyin.so.15.0.0 target/release-stage/bdb/usr/lib/libzhuyin.so.15.0.0
```
