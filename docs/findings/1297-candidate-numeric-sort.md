# #1297 item A — stable sorting with guarded wrapping frequency

- **Status:** corrected under the maintainer decision dated 2026-10-30 UTC,
  recorded 2026-10-29T17:31:36Z (`date -u` on the executing host). The
  scoped case, native, policy and shipping measurement gates are complete.
- **Disposition:** the four row58 cases must MATCH and are unregistered.
  Only cyclic frequency groups can differ through this sorting change.
  Existing λ observations remain under #1378; their selection
  differences are independent of candidate sorting.
- **Policy limb:** the frequency comparator is libpinyin's product rule,
  reproduced with Rust's defined wrapping arithmetic. Only std sorting's
  violated order precondition admits a fallback difference. GLib's internal
  sorting algorithm is removed, never transcribed.
- **Pin:** libpinyin 2.11.92 at `074a2219c90feaf962d0d24f034514033ece5f99`,
  read from `/tmp/oxpinyin-a-pin`. Verified git blobs: `src/pinyin.cpp`
  `f27f7cf776724ead8d55e14b4af390e790054a91`, `src/zhuyin.cpp`
  `d1520c189afdb9c22878715c841e00e97483fb11`. GLib 2.84.4 in the
  canonical Debian trixie reference. Both facades; the rule applies to
  every backend. The selected sort/row58 cases are verified on all three registered cells.
- **Pin derivation:** `src/pinyin.cpp:1678-1709` compares enabled phrase
  length, pinyin span and frequency, descending, and returns zero after
  enabled keys tie. `:1692-1694` prescribes the unsigned eight-bit span;
  this width and wrap stay. `:1701-1705` subtracts two `guint32` frequencies
  before conversion to `gint`, reversing the numeric sign for gaps above
  2³¹ (and failing antisymmetry at exactly 2³¹). `:2286,2440-2445` use
  stable GLib array sorting. `src/zhuyin.cpp:1129-1144,1529,1610` always
  uses phrase length then frequency. Stability is the documented contract;
  the old GLib internals are recorded in [the dated source reading](921-927-glib-sorts.md).
- **Rust counterpart:** `candidate::rank_by_frequency` stably groups rows
  by descending enabled `(phrase_length, pinyin_span)`, checks each group's
  frequency relation, then uses stable std sorting with
  `rhs.wrapping_sub(lhs) as i32` on consistent groups and stable numeric
  descending frequency on cyclic groups. Both `session::rank_candidates`
  and `predict::sort_predicted` call it. `m_freq` stays available to dedup
  but never breaks a sort tie. `RankKey::Ord` remains absent.
- **Forward trace:** C guess-candidates entry → parsed window and candidate
  collection → stored unigram/bigram frequency calculation → enabled key
  construction → stable sort → LONGER/NBEST insertion and existing text
  dedup → candidate getters. Prediction enters through
  `pinyin_guess_predicted_candidates`, collects bigram/prefix rows, computes
  their phrase lengths and frequencies, sorts, then deduplicates.
- **Backward trace:** after-cursor and before-cursor lists share the same
  candidate sorter. Zhuyin fixes length/frequency dimensions regardless of
  the pinyin sort word; prediction enables length/frequency only. All DBM
  backends feed these same routines. Tails use their existing S3 comparator;
  beam, node and final-tail selection are outside item A. Scoring and the
  conversion that produces `m_freq` are unchanged.
- **Input partition and difference:** wrapping comparison is preserved
  whenever each enabled length/span tie group fits in an open circular
  semicircle, including groups crossing zero with large numeric gaps.
  Only groups failing that condition use the numeric fallback. With consistent
  unigram/total ratios and λ in `[0, 1]`, scaled frequencies remain below
  2²⁴ (`src/pinyin.cpp:1820-1824`). λ alone is insufficient: the pin's
  defined total-counter wrap and failed-token update can leave a unigram
  larger than its total; the row58 diagnosis below demonstrates that at
  default λ. Those row58 groups are consistent and must retain pin order.
  The guard proof below includes antipodal pairs and longer cycles.
  Empty/singleton lists and disabled dimensions keep their existing results.
  NaN/∞ scores convert to zero candidate frequencies and introduce no
  candidate-sort change. No λ validation or storage normalization is added.
- **Contract preserved:** C ABI/layout, exported symbols, real GLib ownership,
  lifetime, persistent formats/bytes, data ownership, process side effects,
  diagnostics, scoring, dedup winners and physical trellis slots remain as
  before. Only cyclic groups may use a different candidate order.
- **Prior rulings:** the maintainer decision dated 2026-10-30 supersedes
  the prior numeric-frequency instruction. The no-ported-internals rule
  still requires stable std sorting; the contract-not-implementation rule
  still retains the eight-bit span.

## Consistency proof and guard

Let `H = 2^31`, and let `compare(a,b)` be the sign of
`b.wrapping_sub(a) as i32`. Equal frequencies compare equal. Frequencies
separated by exactly H compare Less in both directions, violating
antisymmetry; the guard rejects such pairs as a two-way cycle.

For non-antipodal distinct frequencies, the relation is a circular
tournament. It is consistent on a finite list exactly when there is a
frequency M with `M.wrapping_sub(f) < H` for every frequency f: all distinct
points then lie in a circular interval of width strictly less than H.
Unwrapping that interval makes the relation ordinary descending order,
proving sufficiency, including ties. Conversely a consistent finite order
has a maximum M; its comparison against every other value gives exactly
that strict bound, proving necessity. A cyclic example is
`0 -> 0xc0000000 -> 0x60000000 -> 0`, where arrows mean "precedes".

The first scan retains a candidate maximum, replacing it only when the
incoming frequency compares before it. If a maximum exists, it replaces
any earlier candidate and no later value displaces it. The second scan
checks the strict bound for every original value. Thus every consistent
list passes, and every passing list is consistent. Empty/singleton lists
and duplicate frequencies pass. The guard performs only defined wrapping
subtraction, comparisons and iterator walks: no allocation, indexing,
unchecked access or potentially overflowing arithmetic.

Length and span precede frequency. They partition the full relation into
independently ordered equivalence groups; the full relation is consistent
iff each group's frequency relation is consistent. A cycle in one group
therefore never forces a consistent neighbor to use numeric fallback.
Stable sorting by the leading fields preserves collection order inside
each group; a second stable sort orders that group's frequency, with equal
frequencies retaining collection order. Zeroing disabled fields and ignoring
dedup-only `m_freq` are unchanged.

The guard takes two linear scans per group, O(n) total. The stable grouping
and frequency sorts together remain O(n log n) worst-case time and O(n)
auxiliary storage, with no extra guard allocation. No cyclic comparator
is ever passed to std sorting. This is a domain guard around the standard
API, not a port of GLib internals.

Tests compare the guard with independent antisymmetry/transitivity checks
over boundary triples; include empty lists, ties, wraparound, antipodal
pairs and three-way cycles; and prove that leading length/span dimensions
isolate groups, that cyclic fallback is deterministic, and that stable
ties and aligned dedup frequencies survive. Prediction's test now expects
wrapping order across zero rather than the superseded numeric order.

## Scoped correction gate

The explicit 126-case selection is the union of tags `921`, `927`, `1275`,
all 43 `row58-*` cases, all `table-conf-(919-)?lambda-*` cases and both
`table-conf-919-header-indented-lambda` facade cases. On BDB, KC and
sound-tkrzw it gives **118 MATCH and eight existing POLICY-ACCEPTED per
cell** after the three existing λ records are refreshed and revalidated.
Every row58 case MATCHes, including `row58-signed-{score,duplicate}-{off,on}`.
None is registered as a difference; no full/default suite runs.

Compared with the superseded numeric implementation, only the already
registered `table-conf-lambda-{one-and-a-half,two,million}-zhuyin` views move.
The first two now reproduce exactly main's prior complete subject digests
on every cell: their frequency groups fit across zero inside an interval
narrower than 2³¹. Their accepted differences remain the independent #1378
sentence/selection differences. At λ=million, cyclic frequency groups use
the defined fallback, while consistent length groups restore the wrapping
order. This is the only case retaining a new candidate-view difference
against main. All pin digests stay unchanged; only subject `result.view`
moves. `just policy-record` updates those existing entries, never row58.
Only the three refreshed entries are rerun after the initial selection.

The new-frequency mechanism is reached at ordinary λ too: the row58 fixtures
retain λ `0.312699`, import count 1,431,655,631 (unigram 4,294,966,893), then
use the pin's defined wrapping/failed-token update to make total 20,000,000
(`src/storage/phrase_index.h:628-634` at the pin). The f32 score is
2,476,262,656, exceeding `gint::MAX` while the denominator does not.
These consistent groups must use wrapping order; the corrected cases keep
`甲甲` at one-based position 65 and `的心` at 64, exactly as the pin.

Named mutation **M1297-frequency-ascending** changes only the guarded,
consistent-group sort to ascending numeric frequency. On BDB all three
`glib-sort-candidates-{pinyin,zhuyin}` / `glib-sort-predicted` controls
DIFFER. Restoring the source byte-for-byte and rebuilding returns all
three to MATCH. The guard is never disabled and std never receives a
cyclic comparison during the mutation.

The touched engine/capi completion gate passes 366 tests, with the two
isolated timing diagnostics ignored; clippy `--all-targets -- -D warnings`,
fmt and diff checks pass. The original dependent-crate gate and case-module
harnesses passed before this correction; no unrelated workspace sweep is
repeated. Reproduction uses the maintained recipes:

```sh
just heavy -- cargo test --locked -p oxpinyin-engine -p oxpinyin-capi
just heavy -- cargo clippy --locked -p oxpinyin-engine -p oxpinyin-capi --all-targets -- -D warnings
just fmt -- --check
git diff --check
# For each registered cell, build debug facades with just cell-artifacts,
# derive the exact ID union above from contract-diff.py --list-tags, then:
just heavy -- just runner contract-diff "$CELL" "$PIN" "$DEBUG_PINYIN" "$DEBUG_ZHUYIN" -- --cases "$CASES"
```

## Representation choice and measurements

Cached `(RankKey, original_index)` rows preserve one compact per-call buffer
and the frequency keys needed by text dedup. Candidate rows are permuted by
cycle swaps after sorting. Sorting bare indices needs a separate key buffer;
sorting decorated rows moves a larger element. Prediction needs no post-sort
key array, so its owned rows sort directly using their cheap stored fields.
The guard adds no allocation. Both paths remain O(n log n) worst-case time
and O(n) auxiliary storage, with O(n) guarding.

Before the wrapping correction, a same-host 21-round release diagnostic
compared complete sort stages, excluding fixture setup but including key
construction, sorting, permutation and aligned-key materialization. Every
variant first proved identical complete rows and dedup keys. At 32/128 rows
and all fields enabled, median nanoseconds were:

| Representation | 32 rows | 128 rows | Stored bytes per row before sorting |
| --- | ---: | ---: | ---: |
| Cached keys, retained | 785.1 | 3,864.2 | 32 |
| Bare indices plus keys | 870.8 | 4,221.5 | 4 + 24, separate buffers |
| Decorated candidate rows | 1,923.0 | 8,319.9 | 72 |
| Keys constructed during comparison | 4,126.2 | 21,069.3 | 56 |

At eight enabled-key rows, bare indices had the lower median (189.5 versus
203.4 ns), with overlapping observed ranges; cached keys won from 32 rows
upward and at every tested size with all fields disabled. Direct prediction
rows beat cached indices at 8, 32, 128 and 512 rows. Keep these representations.
Those timings used `c05a0fec`'s superseded comparator and are historical
representation evidence, **not the corrected implementation's performance**.
Both maintained `sort_design_measurement` diagnostics now use the guarded
sorter for all variants. They are ignored in ordinary tests and are not
rerun as part of the maintainer's restricted correction gate.

## Shipping correction measurements

The correction uses the same Ubuntu linux/amd64 Intel i7-9750H host,
Rust 1.97.1, GNU BFD ld 2.47 and Valgrind 3.27.1. Libraries, data, profiles,
work and TMPDIR are ext4; no other heavy job overlaps measurement.
The baseline's production code is `ab6c38ea`; current landing tip
`9e89b497` changes only tooling/docs and has identical shipping sources.
The locked release recipe enables `bdb,shipped` for pinyin and `bdb` for
zhuyin. Profiled pinyin adds only `-Cdebuginfo=1 -Cstrip=none`.

The guarded helper changes concrete Rust symbol paths: the prior policy's
`rank_candidates::{closure#0}` stable-sort paths do not resolve against the
corrected archive. This confirms the maintainer's condition for regenerating
all six backend/facade policies. The unchanged pin-provisioned startup
profiles are reused and checked. Both independent generation passes agree
byte-for-byte (396 collections, eight admitted workers), and the six
policies are adopted without hand edits. The shipping relinker and independent
linked-ELF check report `ordered` for every backend/facade:

| Backend | Pinyin policy paths | Zhuyin policy paths |
| --- | ---: | ---: |
| BDB | 282 | 263 |
| KC | 364 | 346 |
| Sound-tkrzw | 294 | 276 |

Both tkrzw pin and subject facades resolve to the sound cached library
verified by setup. After checking the unstripped objects against their
respective policies, private copies stripped with `strip --strip-unneeded`
measure:

| BDB shipping library | Base bytes | Corrected A bytes | Increase |
| --- | ---: | ---: | ---: |
| libpinyin.so.15.0.0 | 2,478,888 | 2,507,560 | 28,672 (1.16%) |
| libzhuyin.so.15.0.0 | 2,220,840 | 2,237,224 | 16,384 (0.74%) |

The guard and separate consistent/cyclic sort paths add code while removing
the GLib merge port. These increases are disclosed; no size reduction is
claimed. Guarding adds no per-row heap allocation and preserves the previous
O(n log n) time / O(n) auxiliary-space bounds.

The sentence and complete typing-sequence campaigns each run once for this
correction, with stock `nihaozhongguo`, options `0x18a`, empty user dirs,
one iteration and three serial rotating fresh-process rounds per arm and
byte-identical control. Both debug-info neutrality checks pass with identical
`.text`. All complete observations agree across base, A, controls and pin.

| Workload | Base Ir | Corrected A Ir | Pin Ir | Control result |
| --- | ---: | ---: | ---: | --- |
| Sentence | 2,470,244 | 2,470,394 | 3,825,915 | VALID; both spreads zero |
| Complete typing sequence | 65,595,178–65,598,907 | 65,159,237–65,160,599 | 77,112,796 | DIAGNOSTIC; RandomState scopes only |

Sentence increases by 150 instructions (+0.0061%). Typing's reported
base-minus-A bounds are 434,579–439,670 instructions, about 0.66–0.67%.
It is not an exact instruction result: the same-bytes control spreads are
4,440 and 1,469, entirely in the existing dedup/tail/memcmp allowlist, with
no out-of-allowlist varying function. These campaigns make no native
wall-time or heap measurement claim. Every count above is from the same
correction campaign, never a baseline taken from the prior comparator run.

Reproduce policy generation and shipping installation using
[packaging regeneration](../packaging.md#regeneration). Reproduce the two
measurements with the runbook's
[`just measure callgrind-differential` sentence/typing commands](../runbooks/benches.md#sentence-and-typing-sequence-callgrind-campaign),
passing base's policy explicitly for `main/pinyin` and `mctl/pinyin`,
and the regenerated policy for `pr__/pinyin` and `pctl/pinyin`. Install
plain and profiled libraries through `just packaging install` using the
same locked release profile; only the profiling flags differ.
