# Corpus tail (W12)

Date: 2026-08-21 · Status: **residual enumeration; Class B closed**

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

Measured 2026-08-21, before and after the Class B fix:

| residual | pre-fix | post-fix | pinned in |
|---|---:|---:|---|
| compared | 10,190 | 10,190 | `real_tables_session_reports_parity` |
| top-1 misses | **13** | **12** | same (top-1 = 10,177 → **10,178**) |
| top-5 misses | **1** | **0** | same (top-5-set = 10,189 → **10,190**) |
| absent | **1** | **0** | same |
| order-only (tie-swaps) | **1,036** | **1,036** | same |
| prefix-10 gap | **4,059 of 98,930** | **4,058 of 98,930** | same (94,871 → **94,872** overlap) |

Bit-identical to the five frozen assertions in the pin test (before and
after the re-freeze). The pin re-freeze is recorded in
`docs/findings/pin-refreeze-2026-08.md` as the 2026-08-21 amendment.

## The 13 top-1 misses

Two classes. Twelve are the same tie-swap species the 1,036 order-only
inputs sit in — same depth-10 set, comparator tie at the top pair.
One (Class B, `ni''hao`) was a novel parse-side signal, closed
2026-08-21 by aligning the doubled-apostrophe separator with the pin.
The post-fix residual is 12; Class B moved out of the tail.

### Class A — top-two comparator tie-swap (12 of 13)

Same depth-10 candidate set as the oracle. Every RankKey-1 (phrase text
length in characters, `candidate-construction.md` §8.2) and RankKey-2
(pinyin span in bytes) is tied at the top pair; only RankKey-3 (real
unigram count) breaks the tie, and the choice inverts under the
port's fixed-point-vs-float divergence and insertion-order tie-breaks
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

## The 4,058 prefix-10 residual

The prefix-10 gap counts oracle top-10 positions that do not appear
in our top-10. Post-fix it sums to 4,058 of 98,930 positions across
the corpus (was 4,059; the Class B fix recovered the `你好` position
of `ni''hao`), so its population is much larger than the 12 top-1
misses. Distribution characterisation is out of scope of this
enumeration; a targeted follow-up would either (a) sample by rank at
which the gap starts (are the missing oracle candidates always at the
tail, or do they push earlier?), or (b) categorise by the shared
prefix depth between our list and the oracle's. Recorded as
follow-up; the pin does not gate on this figure at any threshold
beyond "unchanged".

## The 1,036 order-only tie-swaps

Same-set-different-order at depth 10. These are the same comparator
class as Class A above, just where the swap sits below rank 1 and so
does not degrade `top-1`. Pinned at 1,036 by the parity test; no
Stage-1 change to the comparator can move the count without moving
`top-1` as well.

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
window rather than in the W2 corpus fixture. The `option-sweep.sh`
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

The 13 top-1 misses are named and split into two classes: 12 in the
comparator tie-swap species that already has a §3 characterisation
(`sentence-surface.md`), and 1 in a parse-side species (`ni''hao`)
closed on 2026-08-21 by aligning the doubled-apostrophe separator with
the pin. The frozen residual counts are reproducible by
`bin/corpus-tail` against the same tables the pin test uses, so a
future move on either class is diffable against this baseline.

W12 remains open — 12 Class A residuals stay unresolved pending a
comparator re-freeze — but the tail is no longer aggregate, and the
one non-tie-swap tail is closed.
