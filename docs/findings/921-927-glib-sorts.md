# GLib sorts: n-best tails and candidate lists (#921, #927)

Date: 2026-10-26 UTC. Base: oxpinyin `e84f06f`. Pin: libpinyin
`074a2219c90feaf962d0d24f034514033ece5f99`, read from a clone at that
commit. GLib: 2.84.4 (`libglib2.0-0t64 2.84.4-3~deb13u5` in the reference
image), `glib/gqsort.c` and `glib/garray.c` read at the upstream `2.84.4`
tag (`gqsort.c` SHA-256 `7c24f23e…6950`). Binary: the bdb reference cell's
`libpinyin.so.15.0.0` (SHA-256 `58f73afe…c34c`, the value in
`tools/oracle/oracle-pin.txt`). No pin run was used to learn a behaviour.

## Result

Every sort in scope is kept as GLib's merge. None of the comparators is a
total order on every input the C API reaches, so a standard-library sort
would not be guaranteed to give the pin's order and may panic (Rust 1.81).
The two copies of the merge are now one, `oxpinyin_engine::nbest::glib_merge_sort`,
used by the tails, `glib_rank_sort` and `sort_predicted`. Each merge copies
its left half only (the old engine copy took both halves; `sort_predicted`
had its own index merge), with the same comparisons in the same order. It
stays recursive at its old path, so the function-order policies still name
it. No exception or discrepancy record is needed: the output is the
pin's on all inputs.

## The algorithm

`g_ptr_array_sort`, `g_array_sort` and `g_array_sort_with_data`
(`garray.c:919-965,2409-2421`) call `g_sort_array` → `msort_r`
(`gqsort.c:197-281,328-335`). For every element size this runs
`msort_with_tmp` (`:69-193`): split `n1 = n / 2`, sort both halves, then
compare only while both halves have elements and take the left on
`cmp <= 0`; a right remainder stays in place (`:190-192`). Elements over
32 bytes are sorted through pointers (`:206-275`), which changes copying,
not comparisons. `glib_merge_sort` makes the same comparisons in the same
sequence; `the_in_place_merge_matches_the_transcribed_one` checks it
against a transcription of `msort_with_tmp` on every array of up to seven
keys from five values, NaN included.

## Comparators

| Sort | Pin | Total on valid input? | Total on all reachable input? |
| --- | --- | --- | --- |
| n-best tails | `trellis_value_compare`, `phonetic_lookup.h:174-178`, sorted at `:337` | **No** | No |
| pinyin candidates | `compare_item_with_sort_option`, `pinyin.cpp:1678-1709`, sorted at `:2286` | Yes | No |
| predicted candidates | the same comparator with `SORT_BY_PHRASE_LENGTH \| SORT_BY_FREQUENCY`, `pinyin.cpp:2440-2445` | Yes | No |
| zhuyin candidates | `compare_item_with_length_and_frequency`, `zhuyin.cpp:1129-1144`, sorted at `:1529,1610` | Yes | No |
| phrase-string dedup | `compare_indexed_item_with_phrase_string`, `pinyin.cpp:2041-2068` | Yes (`strcmp`) | Yes |

**Tails.** The comparator returns `-((*lhs)->m_poss - (*rhs)->m_poss)` as
a `gint`. In the binary it is `subss`, a sign flip (`xorps`) and
`cvttss2si %xmm0,%eax` (symbol `0x96010`, `+0x0b`..`+0x17`), passed to
`g_ptr_array_sort` from `get_nbest_match` (`0x98dc1`, call at `0x98dcb`);
`tail_compare` is that sequence. The conversion truncates toward zero, so
two possibilities less than one natural-log unit apart compare 0. That
breaks transitivity on ordinary finite scores: −10.0 and −10.6 tie,
−10.6 and −11.2 tie, but −10.0 sorts before −11.2. The input order is the
heap selection's (`get_top_results`, `:143-171`), which ranks by sentence
length before possibility, so it need not already be sorted by
possibility. NaN (a `table.conf` λ above 1, policy row 83) converts to
`INT_MIN` and compares below everything, itself included; ±∞ is reachable
the same way.

**Candidate lists.** Phrase length and pinyin length are `guint8`
differences, a total preorder. The frequency key is `-(freq_lhs -
freq_rhs)` in `guint32` arithmetic returned as a `gint`. `m_freq` is
`(λ·bigram + (1−λ)·unigram/total)·2²⁴` or `(1−λ)·unigram/total·2²⁴`
(`pinyin.cpp:1810-1866,1918-1921`), below 2²⁴ for λ in `[0, 1]`, so the key
is a total order there. A `table.conf` λ outside `[0, 1]` makes
frequencies 2³¹ or more apart; their difference flips sign, and 0,
2³⁰+1 and 2³¹+2 form a cycle (`a_wrapping_frequency_cycle_takes_the_merge_order`).
`table.conf` is read by `pinyin_init`/`zhuyin_init`, so this input is
reachable through the C API.

The phrase-string dedup sort is a total order, and oxpinyin already
replaces it with text-keyed winners (`dedup_ranked_by_text` in
`engine/src/session/mod.rs`; the winner map in `capi/src/predict.rs`), which is #913/#914's ruling and is not changed
here.

## What a standard sort would change

Two named mutations of oxpinyin's side, built in the bdb cell and run on
the selection below:

| Mutation | Change | DIFFER |
| --- | --- | --- |
| `tie-takes-right` | the merge takes the right element on `cmp == 0` (an unstable merge) | 67 of 71; the four that match have one tail or an empty list (`glib-sort-tails-zhuyin`, `nbest-user-table-order-zhuyin`, `final-step-{pinyin,zhuyin}-n-20`) |
| `std-stable-sort` | `slice::sort_by` with the same comparator | 5 of 71: `table-conf-lambda-{million,nan,inf,one-and-a-half,two}` (pinyin), whose sentences come back in another order |

The second row is the evidence for keeping the merge. With λ in `[0, 1]`
the current `std` stable sort happened to match on every case. On up to
three finite tails (pinyin's `nbest`; zhuyin has one) it agrees with the
merge on all 29,791 triples of a 0.1-unit grid. The standard library
leaves the order unspecified, and allows a panic, when the comparator is
not a total order, so that agreement cannot be relied on. With NaN tails,
which the C API reaches, the order differs.

## Cases

New controls in `tools/bisection/contract-diff.py`, tagged 921 and 927:
`glib-sort-tails-{pinyin,zhuyin}` (20 inputs, every sentence rank),
`glib-sort-candidates-{pinyin,zhuyin}` (seven inputs with many equal keys;
sort words 0x14, 0x1c and 0x1e, or both zhuyin cursors; zhuyin with
`USE_TONE`, as the other zhuyin cases) and
`glib-sort-predicted` (six prefixes, the plain call without punctuation).

```sh
source ~/.oxpinyin-oracle.env
just heavy -- tools/bisection/cell-artifacts.sh --artifacts pinyin-capi,zhuyin-capi
L=$(for n in million nan inf negative-half negative-two one-and-a-half two; do
      printf 'table-conf-lambda-%s,table-conf-lambda-%s-zhuyin,' "$n" "$n"; done)
L+=nbest-user-table-order-pinyin,nbest-user-table-order-zhuyin
tools/bisection/run-contract-diff.sh bdb "$PINYIN_ORACLE_PREFIX" \
  target/cells/bdb/debug/libpinyin_capi.so target/cells/bdb/debug/libzhuyin_capi.so \
  -- --issue 921 --issue 927 --issue 594 --cases "$L"
```

Result on bdb: 71 of 71 MATCH. kc and tkrzw: not run; no backend code
changed (`tools/bisection/cells-for-diff.py`).

## Pinyin-length key (#1275)

The pin's pinyin-length key wraps at 8 bits (`guint8`, `pinyin.cpp:1692-1694`),
and a candidate can span 256 bytes or more, since apostrophes become zero
keys that do not count toward `MAX_PHRASE_LENGTH`
(`phonetic_key_matrix.cpp:69-80,383-393`). `RankKey::for_sort_word` now
truncates the span to `u8` (upstream report draft 9). It does not affect the
merge decision: a wrapped key is still a total preorder.

The pin's zhuyin facade has no span key: `compare_item_with_length_and_frequency`
(`zhuyin.cpp:1129-1144`) compares phrase length and frequency only, under
`g_array_sort` (`:1529,1610`). oxpinyin's zhuyin lists now use the same
comparator: the zhuyin C API sets `CandidateRanking::LengthThenFrequency` on
each session at instance setup, and both the after-cursor scan and the
before-cursor slices rank with `SORT_BY_PHRASE_LENGTH | SORT_BY_FREQUENCY`
under it (`Session::rank_word`). Nothing observable changes, because the span key always tied
there:

- a phrase length is a key count (`phrase_index.h:136-150`; zhuyin import
  refuses a mismatch, `zhuyin.cpp:525`);
- the zhuyin parses run no split pass (`zhuyin.cpp:1030-1038,1069-1076`),
  `fill_matrix` puts one key per column (`phonetic_key_matrix.cpp:51-55`)
  and fuzzy keys keep the span (`:254-258`), so every phrase of one length
  from one offset ends at one byte; oxpinyin's zhuyin matrix has no split
  keys either (`Session::ensure_matrix`);
- the only same-length rows with different spans the pin makes are copies of
  one token across an apostrophe run (`zhuyin.cpp:1499-1512` does not skip
  zero keys), and they share the token's `m_freq` (`:1229-1270`). oxpinyin's
  scan does not make the copies, and the pin's dedup keeps the shortest one
  (`:1408`).
