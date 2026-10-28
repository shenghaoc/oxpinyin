# Reachable algorithm-precondition witnesses

Date: 2026-10-28 UTC (`date -u`). Diagnostic tools based on oxpinyin
`1cb70696ecf8f80aa1fdcf3e2ba62ba7e0d27fd7`; no production changes.
Source read: a clean libpinyin checkout at
`074a2219c90feaf962d0d24f034514033ece5f99`, independently fetched and
verified at the same commit by `tools/oracle/build-oracle.sh`.

## Results

The fixed 276-case BDB set completed with every parse consuming its whole
input and every `pinyin_guess_sentence` succeeding. All expected diagnostic
ranges were retained and validated. Each case used a fresh user directory,
set its options and scheme, parsed, and called `guess_sentence` once. There
were no candidate guesses, sentence reads, or inferred sentence indices.

| Site | Ranges inspected | Ranges with witnesses | Violating unordered pairs | First witnessed case; step; range size |
| --- | ---: | ---: | ---: | --- |
| S1 beam selection, `nbeam=32` | 1,560 | 26 | 85,054 | `resplit-jianai-on`; 4; 490 |
| S2 final-tail selection, `nbest=3` | 276 | 32 | 54,185 | `divided-bian-on`; 4; 221 |
| S3 pre-sort tails | 276 | 37 | Not counted: first triple only | `divided-qian-on`; 4; 3 |

Only S1, S2 and S3 are registered by this result. The node's front-eviction
defect is source-established and retained, not another witnessed site.

The diagnostic invokes the pin's own predicates on live `gfloat` values
inside `get_nbest_match`. Each selection record contains the range size,
violating-pair count, and first violating pair. Each pre-sort record contains
its first violating triple if one exists. The Python runner keeps `%a`
values as strings and does not recalculate the predicates. Batch positions
and node slots below are zero-based; lengths are `m_sentence_length`.

### S1: `jianai`, full pinyin, tables enabled

At step 4 the 490-candidate beam input has 4,791 violating pairs. Its first
pair is:

| Candidate | Batch position | Length | `m_poss` (`%a`) | Previous token | Current token | Slot |
| --- | ---: | ---: | --- | --- | --- | ---: |
| A | 0 | 1 | `-0x1.b84bb8p+3` | `0x00000001` | `0x01000e40` | 0 |
| B | 124 | 2 | `-0x1.85afap+3` | `0x00000001` | `0x01005350` | 0 |

`less(A,B) = true`; `less(B,A) = true`.

### S2: `bian`, full pinyin, tables enabled

At final step 4 the 221-candidate selection input has 776 violating pairs.
Its first pair is:

| Candidate | Batch position | Length | `m_poss` (`%a`) | Previous token | Current token | Slot |
| --- | ---: | ---: | --- | --- | --- | ---: |
| A | 1 | 1 | `-0x1.99254p+3` | `0x00000001` | `0x01000240` | 1 |
| B | 50 | 2 | `-0x1.9235c4p+3` | `0x00000001` | `0x010028a3` | 0 |

`less(A,B) = true`; `less(B,A) = true`.

For both sites, `src/lookup/phonetic_lookup.h:65–90 at 074a2219` first
allows an adjacent longer sentence to beat the shorter one by the
`LONG_SENTENCE_PENALTY` margin, then unconditionally says the longer one
loses in the reverse comparison. These captured pairs violate asymmetry,
hence the strict weak ordering required by C++
[`[alg.sorting.general]`](https://eel.is/c++draft/alg.sorting.general).
The heap calls are `src/lookup/phonetic_lookup.h:143–171 at 074a2219`,
reached at S1 `src/lookup/phonetic_lookup.h:770–771 at 074a2219` and S2
`src/lookup/phonetic_lookup.h:331–335 at 074a2219`.
`src/include/stl_lite.h:38–44 at 074a2219` imports the standard heap
algorithms directly. This conclusion does not depend on which comparator
argument orders a particular heap implementation happens to call.

### S3: `qian`, full pinyin, tables enabled

Immediately before the final sort at step 4, the three tails are:

| Candidate | Batch position | Length | `m_poss` (`%a`) | Previous token | Current token | Slot |
| --- | ---: | ---: | --- | --- | --- | ---: |
| A | 0 | 1 | `-0x1.c9dc3cp+2` | `0x00000001` | `0x010017b3` | 0 |
| B | 1 | 1 | `-0x1.dda91p+2` | `0x00000001` | `0x010017b3` | 1 |
| C | 2 | 1 | `-0x1.0a68dp+3` | `0x00000001` | `0x010017af` | 0 |

The pin returns `compare(A,B) = compare(B,A) = compare(B,C) = compare(C,B) = 0`,
`compare(A,C) = -1`, and `compare(C,A) = 1`. Thus A~B and B~C but A≁C.
The equality relation is not transitive.

`src/lookup/phonetic_lookup.h:174–178 at 074a2219` returns the negated
`gfloat` difference as `gint`; the call at
`src/lookup/phonetic_lookup.h:337 at 074a2219` passes that comparator to
`g_ptr_array_sort`. Its
[GLib 2.84.4 contract](https://github.com/GNOME/glib/blob/2.84.4/glib/garray.c#L2240-L2295)
uses a qsort-style comparison and promises a stable sort. The captured
triple violates the consistent total ordering requirement in
[C11 N1570 §7.22.5 paragraph 4](https://www.open-std.org/jtc1/sc22/wg14/www/docs/n1570.pdf).
This is a reachable triple, not the illustrative arbitrary-score triple
in `docs/findings/921-927-glib-sorts.md`.

## Inputs and environment

`tools/bisection/nbest-precondition-cases.tsv` is the complete input list:
20 divided-table spellings and 85 concatenated resplit-table spellings
from `src/storage/special_table.h:10–29,33–117 at 074a2219`, 29 additional
segmentation/ambiguity fixtures, and four Microsoft double-pinyin strings
(`xm`, `xioj`, `fjgj`, `nihl`). Each runs with the IBus-equivalent mask
`0x1fe00198` and the table-off control `0x1fe00018`. The enabled mask has
`PINYIN_INCOMPLETE`, `ZHUYIN_INCOMPLETE`, `PINYIN_CORRECT_ALL`,
`USE_DIVIDED_TABLE`, and `USE_RESPLIT_TABLE`; the control clears only the
last two. Full-pinyin scheme is 1; Microsoft double-pinyin scheme is 2.
Full parsing invokes resplitting and inner splitting
(`src/pinyin.cpp:1497–1524 at 074a2219`); double parsing does neither
(`src/pinyin.cpp:1540–1563 at 074a2219`).

The execution environment was native Linux x86_64, Debian trixie 13.7,
GCC/libstdc++ 14.2.0-19, glibc 2.41, GLib 2.84.4, and Berkeley DB
5.3.28+dfsg2-9. The pin used the reference autoconf `-g -O2` build, assertions
enabled. The system data came from the BDB reference prefix generated from
the model in `tools/oracle/oracle-pin.txt`; both data manifests verified.
The patch-manifest identity was
`7cf51d5b3496eddfcfd695017c76690f4986fba96a3e7d1394658613e79d8be0`.

## Reproduction

Run from the repository root with the environment above and the setup in
`docs/runbooks/setup.md`. `PINYIN_ORACLE_PREFIX` names an unpatched BDB
reference prefix with its system data. Use a fresh diagnostic prefix:

```sh
just heavy -- tools/oracle/build-oracle.sh --libs-only --dbm bdb \
  --apply-patches tools/bisection/patches/nbest-tails-dump \
  --work-dir target/algorithm-preconditions/build \
  --prefix target/algorithm-preconditions/pin
PINYIN_ORACLE_DBM=bdb just heavy -- just runner nbest-train-diff \
  --pin-witness-cases tools/bisection/nbest-precondition-cases.tsv \
  --prefix target/algorithm-preconditions/pin \
  --data "$PINYIN_ORACLE_PREFIX/lib/libpinyin/data"
uv run --no-project --managed-python python -X lazy_imports=normal \
  tools/dev/runners.test.py NbestWitnessTests -v
just fmt --check
git diff --check
```

The diagnostic library and existing C driver built successfully; all five
focused runner tests passed. No shipping library was changed, so shipping
size measurements and production differentials do not apply. Policy and
upstream drafts are in `docs/findings/compatibility-policy.md` and
`docs/findings/upstream-report-drafts.md`. The separate legacy
`print_surface` sentence-index defect is tracked in
[#1345](https://github.com/shenghaoc/oxpinyin/issues/1345).
