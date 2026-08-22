# Corpus tail (W12)

Date: 2026-08-21 · Status: **residual enumeration; Class B closed 2026-08-21;
Class A closed 2026-08-22 — candidate residual is zero**

W12 is the corpus tail (`ROADMAP.md` W12): the undiagnosed parity gap
against the pinned oracle at `0c5e80e`. This finding names the inputs
behind the frozen residual counts so a later, targeted fix has a
concrete target rather than an aggregate.

The initial enumeration split the 13 top-1 misses into Class A
(comparator tie-swaps) and Class B (`ni''hao` — doubled-apostrophe
parse). Class B was closed the same day by aligning with the pin; the
population section below now records both the pre-fix and post-fix
numbers.

## Population

Reproducible from the exports in `/tmp/oxpinyin-export` and a fetched
`interpolation2.text` (matching the scoring path of
`real_tables_session_reports_parity`):

```bash
PINYIN_EXPORT_DIR=/tmp/oxpinyin-export \
PINYIN_MODEL_DIR=<extracted-model20> \
cargo run -p pinyin-oracle --release --bin corpus-tail
```

Measured 2026-08-21, before and after the Class B fix, and 2026-08-22
after the Class A comparator port:

| residual | pre-fix | post-B-fix | post-A-fix (2026-08-22) | pinned in |
|---|---:|---:|---:|---|
| compared | 10,190 | 10,190 | 10,190 | `real_tables_session_reports_parity` |
| top-1 misses | **13** | **12** | **0** | same (top-1 = 10,177 → 10,178 → **10,190**) |
| top-5 misses | **1** | **0** | **0** | same (top-5-set = 10,189 → **10,190**) |
| absent | **1** | **0** | **0** | same |
| order-only (tie-swaps) | **1,036** | **1,036** | **0** | same |
| prefix-10 gap | **4,059 of 98,930** | **4,058 of 98,930** | **0 of 98,930** | same (94,871 → 94,872 → **98,930** overlap) |

The 2026-08-22 re-freeze is recorded in
`docs/findings/pin-refreeze-2026-08.md` as its third amendment. Sentence
pins the same day: row-0 and full-list agreement hold (488 / 385), the
first-6 candidate rows rise 370 → **379** (`sentence_surface_reports_
parity`) — the candidate list is the first-6 rows' tail, so closing the
candidate residual lifts exactly that figure.

## The 13 top-1 misses

Two classes. Twelve are the same tie-swap species the 1,036 order-only
inputs sit in — same depth-10 set, comparator tie at the top pair.
One (Class B, `ni''hao`) was a novel parse-side signal, closed
2026-08-21 by aligning the doubled-apostrophe separator with the pin.
The post-fix residual is 12; Class B moved out of the tail.

### Class A — top-two comparator tie-swap (12 of 13) — CLOSED 2026-08-22

Same depth-10 candidate set as the oracle. Every RankKey-1 (phrase text
length in characters, `candidate-construction.md` §8.2) and RankKey-2
(pinyin span in bytes) is tied at the top pair; only RankKey-3 (real
unigram count) breaks the tie, and the choice inverts under the port's
fixed-point-vs-float divergence and insertion-order tie-breaks
(`sentence-surface.md` §3). All twelve share the shape "2-char phrase
vs 2-char phrase, both from the same syllable cut":

| input | oracle #1 | our #1 | shared depth-5 set (order aside) |
|---|---|---|---|
| `liangbi` | 两笔 | 量比 | 两笔, 量比, 里昂, 两, 量 |
| `guxi` | 股息 | 古稀 | 古稀, 股息, 姑息, 顾惜, 古 |
| `jiancang` | 减仓 | 建仓 | 建仓, 减仓, 吉安, 集安, 间 |
| `changzhecangxiangqiahunyueshuonvhuan` | 唱着 | 长着 | 长着, 唱着, 长, 场, 常 |
| `maogenqingpiaotanlangnongdun` | 毛茛 | 毛根 | 毛根, 毛茛, 毛, 茂, 猫 |
| `meijiatangmaichuangleijugaixugou` | 美加 | 每家 | 每家, 美加, 美甲, 美, 每 |
| `guxi'niner'yenang'wo'mo'penen'suannv'aner` | 股息 | 古稀 | 古稀, 股息, 姑息, 顾惜, 古 |
| `xiego` | 写稿 | 写歌 | 写歌, 写稿, 写给, 斜钩, 写个 |
| `suanch` | 算出 | 酸楚 | 酸楚, 算出, 酸橙, 酸, 算 |
| `goug` | 沟谷 | 狗狗 | 狗狗, 沟谷, 句, 沟, 狗 |
| `baidao'tua` | 白道 | 拜倒 | 拜倒, 白道, 摆到, 白, 百 |
| `bingba(n` | 并把 | 冰坝 | 冰坝, 并把, 并, 兵, 病 |

Two are twins: `guxi` and `guxi'niner'yenang'wo'mo'penen'suannv'aner`
both show the same (股息, 古稀) tie-swap at the head — the long
apostrophe tail does not change the top pair, so this class is not
input-length-sensitive. Similarly `changzhe...` and `maogen...` /
`meijia...` show the tail is inert on the top swap.

This class is the same species `sentence-surface.md` §3 already prices
in as the trellis' second/third-tail near-tie residual. A fix moves the
comparator (fixed-point → float, or the tie-break rule) and would move
the pin — deferred to a maintainer-approved re-freeze, not a W12
diagnostic.

**Diagnosis and fix (2026-08-22).** The pin's comparator key is not the
raw unigram count. `_compute_frequency_of_items`
(`pinyin.cpp:1855-1866`) fills `m_freq` with the unigram possibility
`(1−λ)·unigram/total` computed in C `float`, amplified by 2²⁴ and
truncated to `guint32` (λ = 0.312699 from `table.conf`; `DYNAMIC_ADJUST`
clear in the parity profile, so the bigram term is zero). Two inputs
decide the rest:

- **The frequency data is the phrase-index item unigram, which over
  model20 is exactly interpolation2 count + 1** — probe-verified across
  all 138,096 items (63,907 interpolation2 tokens are +1; the other
  74,189 items are exactly 1). The index total follows:
  50,913,735 + 138,096 = **51,051,831**, also probe-verified.
- **The sort is stable.** `g_array_sort_with_data` runs GLib's merge sort
  (stable since 2.32), so a comparator-0 pair keeps the array order
  `_append_items` (`pinyin.cpp:1769-1791`) laid down: per window,
  library-ascending then token-ascending, system facade before addon.

Under that law all twelve top pairs collapse to equal `m_freq` (the
probe values: 0/0, 4/4, 17/17, 19/19, 3/3) and the pin's #1 is always
the lower (library,) token — the array order, not a frequency choice.
`goug` proved the tie-break half: its pair ties on raw counts too
(86 = 86), and the swap was collection order alone.

The port (`feat/w12-class-a-comparator`): RankKey-3 becomes
`amplified_frequency(count + 1, interpolation2_total + item_count)` —
the f32 chain in C evaluation order, unit-pinned to the probe values and
to a corpus-scale count (2,349,890) where f32 and f64 truncate apart —
and the window scan flushes each window token-ascending (system batch,
then addon), reproducing the pin's array order for the stable sort.
`loses_to` and the n-best trellis are untouched. Result: every residual
count above drops to zero — the 1,036 order-only swaps and the 4,058
prefix-10 gap were the same species below rank 1, so the port closed
them with the top pair.

### Class B — double-apostrophe parse (1 of 13) — CLOSED 2026-08-21

| input | oracle top-5 | our top-5 (pre-fix) | our top-5 (post-fix) |
|---|---|---|---|
| `ni''hao` | 你好, 你, 尼, 呢, 泥 | 你, 尼, 呢, 泥, 妮 | 你好, 你, 尼, 呢, 泥 |

The one **absent** case in the corpus. The oracle admitted `你好` at
rank 1; our candidate list started at 你 and `你好` never appeared at
any depth. The double apostrophe collapsed a group boundary that the
pinned parser treats as a single break, letting the two-syllable
`ni+hao` reach the phrase table on the oracle side while our parser
disallowed the two-syllable phrase across the doubled separator.

The single-apostrophe form is not in this residual — corpus stratum
`05-apostrophe.txt` is generated with a single `'` join
(`parity-corpus.md` "Strata"), so `ni'hao` sits in the agreement bulk.
The double-apostrophe shape reaches the corpus through the generator's
edge stratum (`09-edge.txt` line 33). This is parse-side (path set),
not a comparator issue; the fix is bounded and does not touch the
tie-break scoring.

**Fix.** Upstream's `FullPinyinParser2::parse`
(`pinyin_parser2.cpp:237-250`) treats every apostrophe as a zero-width
step-propagation, so any run of consecutive apostrophes acts as a
single separator when the group after it consumes at least one byte.
`oxpinyin-core::FullPinyinParser::parse` and `SegmentGraph::key_start`
now do the same; the leading-apostrophe half of maintainer decision 3
(`parser-spec-contradiction-incomplete-keys.md`) stays open.

**Freeze move.** `real_tables_session_reports_parity` re-freezes from
10,177 / 10,189 / 94,871 of 98,930 / absent 1 to
**10,178 / 10,190 / 94,872 of 98,930 / absent 0**; tie-swaps stay at
1,036. Deliberate re-freeze, recorded in
`docs/findings/pin-refreeze-2026-08.md` 2026-08-21 amendment and
`docs/findings/parser-spec.md` architect correction log 2026-08-21.

## The 1 top-5 / 1 absent

Both were `ni''hao`, closed together by the Class B fix. Post-fix
top-5-set is 10,190 and absent is 0. The twelve Class-A entries are all
top-2 hits, so they cross the top-5 line trivially.

## The 4,058 prefix-10 residual — CLOSED 2026-08-22

The prefix-10 gap counts oracle top-10 positions that do not appear in
our top-10. Post-B-fix it summed to 4,058 of 98,930 positions across the
corpus (was 4,059; the Class B fix recovered the `你好` position of
`ni''hao`). The Class A port closed it to **0 of 98,930**: the gap's
population was the same amplified-frequency collapse species below rank
1, not a distinct tail class. The distribution characterisation proposed
below (gap start rank, shared prefix depth) is moot.

## The 1,036 order-only tie-swaps — CLOSED 2026-08-22

Same-set-different-order at depth 10: the comparator tie class of Class
A, with the swap below rank 1. The 2026-08-22 port (amplified key + the
pin's array order) moved the count to **0**, reported by the parity
run's `order-only` diagnostic and by `bin/corpus-tail`. The count is not
one of the five frozen `assert_eq!` values (top-1, top-5-set, absent,
prefix-10 numerator and denominator); it is a printed diagnostic, kept
at zero by the same pins that freeze the rest of the surface.

## Option-sweep all-off tails (outside the W2 corpus)

`docs/findings/option-bits.md:194-207` labels six inputs as W12
outside the W2 corpus, measured through the C ABI top-10 by
`tools/bisection/run-option-sweep.sh` under ALL-BITS-OFF (`0x0`):

| input | verdict | cause |
|---|---|---|
| `cang` | TEXT-set DIFF prefix=8, n=31 both | tail set diverges (~4k prefix-10 class) |
| `sang` | TEXT-set DIFF prefix=6, n=16 both | same |
| `lve` | TEXT-set DIFF prefix=4, n=22 both | same (native `lve`) |
| `lue` | cross-engine tail is `lve`'s all-off residual under `CORRECT_UE_VE` | same, via `content_table` alias |
| `agn` | cross-engine tail is `ang`'s all-off residual under `CORRECT_GN_NG` | same |
| `amg` | same as `agn` via `CORRECT_MG_NG` → `ang` | same |

These are the prefix-10 residual class showing up in the option-sweep
window rather than in the W2 corpus fixture. Each input's all-off parse,
top-1, divergence rank, and class are enumerated in
`docs/findings/all-off-tails.md` (reproduced with
`bin/corpus-tail --all-off-tails`, no live oracle). The `option-sweep.sh`
gate already excludes them from W10's STOP set
(`run-option-sweep.sh:116-124,151-155`); no additional gate is added
here.

## Live-typing coverage

`ROADMAP.md` W12 also parks live-typing behaviours the parity
sequence does not exercise: deep paging, mid-composition edits, and
punctuation modes. None of them are on the corpus surface this
finding enumerates — the W2 corpus captures fresh-composition
candidate lists only. Coverage extension is a separate deliverable
and would carry its own capture fixture (not `oracle-candidates.txt`,
which is the fresh-composition contract) and its own oracle
differential — under the same exclusions as every W12 differential
(pin `0c5e80e`; never send zhuyin scheme 7, double scheme 30, or a
toned incomplete key such as `n4` under `USE_TONE | PINYIN_INCOMPLETE`
to the oracle, per `upstream-divergences.md` "Tone digit on an
initial-only key aborts the pin's phrase search" and "Scheme setters
abort or half-mutate on the no-op slots").

Not implemented here; recorded so a later live-typing card knows the
existing corpus is not the venue.

## What this finding closes

The 13 top-1 misses were named and split into two classes: 12 in the
comparator tie-swap species (`sentence-surface.md` §3), and 1 in a
parse-side species (`ni''hao`) closed on 2026-08-21 by aligning the
doubled-apostrophe separator with the pin. Class A closed 2026-08-22 by
porting the pin's tie law — the amplified f32 frequency key and the
array order its stable sort keeps — taking every frozen residual count
(top-1, top-5, absent, order-only, prefix-10) to zero. The W2 corpus
candidate surface now agrees with the pinned oracle bit-identically on
every input at depth 10.

W12's corpus residual is closed. What remains open under W12 is the
live-typing coverage below, which no frozen pin gates.
