# 1297-selection-structure — Tournament selection with a cyclic predicate

- **Status:** accepted · 2026-10-26 UTC · issue #1297.
- **Disposition:** POLICY-ACCEPTED (never MATCH).
- **Policy limb:** language artefact, limb 3 under #1303 / merged #1304. A language-neutral ranking requirement does not name selection traversal. This is neither a transcendental precision difference (a), a memory-safety substitution (b), nor an abort refusal (c).
- **Pin:** libpinyin 2.11.92 at 074a2219; tree read: a checkout at 074a2219 whose cited blobs were hashed against the pin. Reference GLib 2.84.4; both facades; backend-independent logic (bdb gate).
- **Pin derivation:** `lookup/phonetic_lookup_heap.h:56-79,108-125`; `lookup/phonetic_lookup.h:65-91,141-172,272-297,329-339,771`. Existing binary evidence for the conversion instructions is retained in [921-927-glib-sorts.md](921-927-glib-sorts.md).
- **Rust counterpart:** `crates/oxpinyin-engine/src/nbest.rs`; affected C entry points: guess_sentence, guess_sentence_with_prefix, phrase_segment.
- **Forward trace:** C facade parse / validation -> engine session matrix and dictionary/model lookups -> trellis or candidate construction -> ranking / dedup -> sentence and candidate getters. Predicted variants collect prefix/bigram rows, use the same score and ordering, then prepend punctuation where requested.
- **Backward trace:** ordinary and offset candidate windows, the before-cursor zhuyin path, prefix-seeded and constrained sentence guesses, addon and LONGER candidates, and plain / punctuation prediction are included as applicable. Both facades delegate decoding and window ranking to the engine. Backend adapters provide counts and items; they do not choose these orders. The fallback model without real unigrams uses its existing cost path. Punctuation itself is prepended rather than frequency-sorted.
- **Input partition / affected inputs:** Valid inputs can contain exact ties and adjacent sentence lengths; malformed lambda values can also produce special costs. The current 263-unit quantisation of log(1.2f) is the existing class (a) difference, not introduced here. Corpus figures are recorded below before enabling the replacement.
- **Difference:** The libstdc++ heap traverses ties and contradictory pairs in a library-specific sequence. The replacement retains the predicate, the 263-unit margin, nstore 2/1, beam width 32 and nbest 3/1. Site A writes the pin's two slots directly in its argument and slot order. Sites B and C retain the directed winner bracket in one compact loser tree, with lower leaf index as the left argument and the right winner selected only when the predicate says the left loses. Exact ties retain the left leaf. Contradictory adjacent-length pairs select the right leaf. Neither policy claims a strict weak ordering.
- **Why accepted:** the replaced operation is a language/library artefact classified by the pinned #1306 Phase 2 plan. Rust expresses the specified product ranking directly. No product comparator constant, limit, option meaning or interpolation rule is replaced.
- **Contract preserved:** C ABI/layout, exported symbols, real GArray allocation and g_free ownership/lifetime are unchanged. System and user file formats and data ownership are unchanged. Process-wide side effects and diagnostics are unchanged. The documented list/sentence order and text-dedup winner are the affected observables; selecting a changed row naturally selects its existing token path.
- **Complexity:** The tree costs O(m + k log m) time and O(m) reusable session scratch: one Option<NonZeroU32> per padded leaf, containing the match losers and overall winner, against O(m + k log m) and O(m) temporary copied candidates. Site A is O(1) time and space.
- **Tests:** existing contract-diff driver, issues #921/#927/#594/#1275, table-conf-lambda, prediction and punctuation cases; n-best choose/train differentials. Changed cases receive exact POLICY-ACCEPTED observations, retained controls remain MATCH. A named mutation verifies the selection. No new generic driver is required.
- **Prior rulings:** none reversed. LC_NUMERIC is unchanged.
- **Evidence:** BDB setup uses `tools/cloud/setup.sh --cells bdb --cache-dir <private-cache>`; reproduction methods are below; retained results and limitations are recorded in PR #1314. Captures stay local and disposable.
- **Conclusion:** accepted under the maintainer's explicit Phase 2 instruction and language-artefact policy, 2026-10-26 UTC.

## Authorized baseline before the replacement

Source checkpoint `855636532fa7bb1a9c5a3bf01d56bf8e3913e403`, based on
`bab92bf0`, keeps default production selection unchanged. Its off-by-default
diagnostic compares the current port and the planned structures on each
step's live storage, and runs each complete decode twice. Tail sorting and
candidate ranking remain the old implementation on both passes, isolating
selection. All nine committed corpus strata are read in order (10,465
entries, including repeats between strata); 10,190 entries produce sentences.
The one-store shape uses the same pinyin input matrices, isolating the shape
rather than claiming a separate zhuyin parsing/facade measurement.

| Shape | First sentence changes | Ordered top N changes | Distinct top N changes | Node-slot differences |
| --- | ---: | ---: | ---: | ---: |
| nstore 2, nbest 3 | 34 | 242 | 236 | 0 |
| nstore 1, nbest 1 | 3 | 3 | 3 | 0 |

Same-storage beam comparisons below count nonempty steps. Contradictory and
exact-tie columns are overlapping subsets of all steps, not disjoint strata.

| Shape / step partition | Steps | Ordered beam changes | Beam-set changes | First beam member changes |
| --- | ---: | ---: | ---: | ---: |
| 2 / all | 49,978 | 29,916 | 15,235 | 3,694 |
| 2 / contradictory adjacent-length pair | 2,377 | 1,991 | 1,677 | 304 |
| 2 / exact cost-and-length tie | 38,675 | 29,913 | 15,235 | 3,693 |
| 1 / all | 49,978 | 31,134 | 17,480 | 18 |
| 1 / contradictory adjacent-length pair | 0 | 0 | 0 | 0 |
| 1 / exact cost-and-length tie | 38,395 | 31,134 | 17,480 | 18 |

### Cause attribution from the retained baseline data

This breakdown uses only the retained `selection-baseline.log` and the
experiment source at `85563653`; no additional run was made. The experiment
changed the selection structures only. Both arms used the old truncated
tail comparator and merge, the same `loses_to` predicate and 263 margin,
and the same candidate scorer. Every site-A slot matched.

| Cause / level of attribution | First-sentence changes | Ordered top-three changes | What the retained data establishes |
| --- | ---: | ---: | --- |
| Winner-tree selection, combined | 34 | 242 | Demonstrated by the isolated old-selection / planned-selection comparison; site A had zero differences. |
| Specifically a contradictory-pair resolution | Not separable | Not separable | Step counters identify contradictions, but retain no input-to-step or outcome-to-cause linkage. |
| Specifically a positional tie | Not separable | Not separable | Exact-tie step counters likewise have no linkage to the changed sentence outcomes, and overlap with contradictory-pair steps. |
| Replacing the truncated final-tail comparator with score sorting | 0 | 0 | That replacement was disabled in both baseline arms. A changed selector can still change the input order presented to the unchanged truncated sort; that downstream effect is not separately attributable. |
| A pair exactly on the 263 margin | Unknown | Unknown | The log retains neither pair costs nor a link from a comparison to a changed input. The constant and representation were held fixed. |

Thus all 34 / 242 outcomes belong to the combined selection replacement,
while all 34 / 242 remain unresolved **within** a contradictory-pair versus
positional-tie partition. This is an attribution gap, not a claim that each
outcome has both causes or exactly one cause. The count of outcomes involving
an exactly-263 pair is unknown; holding the margin fixed does not establish
absence of such pairs. The existing class (a) gfloat-versus-quantised
margin difference was held fixed and was not measured by this comparison.

The retained aggregate beam counters provide context, not that partition:
1,991 of the 2,377 steps containing a contradictory adjacent-length pair
changed ordered beam; 29,913 of the 38,675 steps containing an exact tie
did so. These subsets overlap. Three of the 29,916 ordered-beam changes
occurred on steps without an exact tie; one of the 3,694 beam-head changes
did so. Those are **step** counts, not changed final-input counts, and do
not cover tail selection as a separate instrumented site. A local beam
change need not affect a returned sentence, and earlier changes alter the
later trellis. The retained log prints only the first twelve changed
ordered sentence lists and their texts, without costs, selected leaf indices,
comparison traces or per-input cause counters. It cannot reconstruct a
disjoint or overlapping 34 / 242 cause table.

The exhaustive two-slot check also preserved physical slots on 7,290
insertion steps across both shapes, including costs 0/263/264 and lengths
1/2/3. `liaoyun` changes its first sentence from 离奥运 to 了云; `sao`
keeps 扫 as the first sentence but replaces its third tail 嫂 with 骚.

Reproduce at the checkpoint (the experiment code is intentionally removed
when the production port is removed): provision BDB as above, source the
private setup environment, and set PINYIN_EXPORT_DIR to that BDB data dir.

```sh
just heavy -- cargo build --locked -p pinyin-oracle --features diagnostic-ranking --bin sentence-tail
just heavy -- cargo test --locked -p oxpinyin-engine --features diagnostic_ranking two_slot_rule_preserves_slot_order_on_ties_and_contradictions
just heavy -- target/debug/sentence-tail --selection-baseline
```

One run on Ubuntu x86_64, Rust 1.97.1, debug builds, model20 and the verified
BDB prefix; the host-wide single heavy slot excluded scanner builds and
other heavy work throughout. No oracle was run to infer behaviour.

The production implementation now stores the same bracket as a compact
loser tree. Its test-only plain winner reference checks every winner and
comparator-call trace through exhaustion, including contradictory relations,
exact ties, sparse occupancy and power-of-two padding. The predicate rewrite
also preserves every reachable length case; only the old u32::MAX saturation
choice is excluded explicitly. Thus the retained tournament baseline applies
to this representation without another tournament-baseline run.

## Verification provenance while parked

The 175-case ranking result reported by PR #1314 predates the scanner rebase.
It combines the 66 ranking cases and 109 extended cases, with the two NaN-last
expectations rechecked after their final source-derived correction. It is a
retained result, not a combined-head differential. On 2026-10-27 the maintainer
parked #1314 to merge last and cancelled the new final-pass plan (combined-head
differential, rebase, policy generation and benchmark) before any job in that
plan started.
Only retained symbol inspection and documentation changes followed. The final
reference-cell ranking differential and new-base record/measure job wait for
an explicit maintainer instruction; no earlier figures substitute for that pass.
The final-pass candidate-window recount is now unconditional and must split
order-only changes from row-multiset changes, as specified in
[1297-ranking-score](1297-ranking-score.md).
The existing source-derived POLICY-ACCEPTED blocks are unchanged.

## Exact directed-selection witness

`ranking-selection-tie` marks default system phrase libraries NOT_USED,
retains the empty user library, and imports only 龘龘 then 靐靐 with the
same reading and count 100. Source `pinyin.cpp:376-392` skips NOT_USED;
`:514-610` assigns increasing user tokens and unigram 300 each.
`phonetic_lookup.h:540-575,578-640` emits one unigram hypothesis per
node: the system bigram has no user-library tokens. The two costs are
identical, so final sorting cannot change their order. For exactly two
leaves libstdc++'s make_heap moves the right child to the root, while the
specified winner tree keeps the left leaf. Thus the pin emits 靐靐 then
龘龘, while Rust emits 龘龘 then 靐靐. Missing fresh-profile user.conf
emits the existing diagnostic on both sides.



The extended selection witnesses retain the original `nbest-user-table-order`
observations for both facades. Lambda cases outside [0,1] can make the trellis
costs special or produce contradictory pairs. The directed tree then changes
sentence heads and duplicate tails, and the existing NBEST/string dedup changes
the displayed count accordingly. Their candidate-score changes also fall under
[1297-ranking-score.md](1297-ranking-score.md), and final tail order under
[1297-tail-score-order.md](1297-tail-score-order.md). The complete observations
retain all candidate texts, counts, save/train/add results, file summaries and
diagnostics; the latter non-ranking fields remain equal in these cases.

```contract-diff-policy
{"schema":1,"record":"1297-selection-structure","status":"current","disposition":"POLICY-ACCEPTED","cases":{"ranking-selection-tie":{"pin":{"exit":0,"result":{"parsed":5,"guessed":true,"sentences":["靐靐","龘龘"],"logs":[]},"stderr":"open SCRATCH/user-X/user.conf failed."},"subject":{"exit":0,"result":{"parsed":5,"guessed":true,"sentences":["龘龘","靐靐"],"logs":[]},"stderr":"open SCRATCH/user-X/user.conf failed."}},"nbest-user-table-order-pinyin":{"pin":{"exit":0,"result":{"guess":true,"logs":[],"parsed":4,"sentences":[[true,"龘龘"],[true,"靐靐"],[true,"大大"]],"tokens":["0x7000001","0x7000002"]},"stderr":"open SCRATCH/user-X/user.conf failed."},"subject":{"exit":0,"result":{"guess":true,"logs":[],"parsed":4,"sentences":[[true,"靐靐"],[true,"龘龘"],[true,"大大"]],"tokens":["0x7000001","0x7000002"]},"stderr":"open SCRATCH/user-X/user.conf failed."}},"nbest-user-table-order-zhuyin":{"pin":{"exit":0,"result":{"guess":true,"logs":[],"parsed":4,"sentence":[true,"龘龘"],"tokens":["0x7000001","0x7000002"]},"stderr":"open SCRATCH/user-X/user.conf failed."},"subject":{"exit":0,"result":{"guess":true,"logs":[],"parsed":4,"sentence":[true,"靐靐"],"tokens":["0x7000001","0x7000002"]},"stderr":"open SCRATCH/user-X/user.conf failed."}},"table-conf-lambda-negative-half":{"pin":{"exit":0,"result":{"add":true,"files":{"addon.bin":27,"gb_char.dbin":26,"gbk_char.dbin":26,"merged.dbin":26,"network.bin":27,"opengram.dbin":26,"user.bin":71,"user.conf":"binary format version:7|model data version:14|database format:BerkeleyDB|open counter:0|","user_bigram.db":null,"user_phrase_index.bin":"dbm","user_pinyin_index.bin":"dbm"},"logs":[],"save":true,"train":true,"view":{"nihao":["你好",true,127,["你好","你好吗","疒","祢","禰","妳","呢","堄","逆","猊","霓","儗"]],"yishu":["艺术",true,606,["艺术","一数","艺术家","艾","饎","搋","眙","扡","誃","昳","斁","睪"]]}},"stderr":"open SCRATCH/user-X/user.conf failed."},"subject":{"exit":0,"result":{"add":true,"files":{"addon.bin":27,"gb_char.dbin":26,"gbk_char.dbin":26,"merged.dbin":26,"network.bin":27,"opengram.dbin":26,"user.bin":71,"user.conf":"binary format version:7|model data version:14|database format:BerkeleyDB|open counter:0|","user_bigram.db":null,"user_phrase_index.bin":"dbm","user_pinyin_index.bin":"dbm"},"logs":[],"save":true,"train":true,"view":{"nihao":["你好",true,127,["你好","你好吗","疒","祢","禰","妳","呢","堄","逆","猊","霓","儗"]],"yishu":["艺术",true,605,["艺术","医术","艺术家","艾","饎","搋","眙","扡","誃","昳","斁","睪"]]}},"stderr":"open SCRATCH/user-X/user.conf failed."}},"table-conf-lambda-negative-two":{"pin":{"exit":0,"result":{"add":true,"files":{"addon.bin":27,"gb_char.dbin":26,"gbk_char.dbin":26,"merged.dbin":26,"network.bin":27,"opengram.dbin":26,"user.bin":71,"user.conf":"binary format version:7|model data version:14|database format:BerkeleyDB|open counter:0|","user_bigram.db":null,"user_phrase_index.bin":"dbm","user_pinyin_index.bin":"dbm"},"logs":[],"save":true,"train":true,"view":{"nihao":["你好",true,128,["你好","你豪","你好吗","疒","祢","禰","妳","呢","堄","逆","猊","霓"]],"yishu":["艺术",true,606,["艺术","一数","艺术家","艾","饎","搋","眙","扡","誃","昳","斁","睪"]]}},"stderr":"open SCRATCH/user-X/user.conf failed."},"subject":{"exit":0,"result":{"add":true,"files":{"addon.bin":27,"gb_char.dbin":26,"gbk_char.dbin":26,"merged.dbin":26,"network.bin":27,"opengram.dbin":26,"user.bin":71,"user.conf":"binary format version:7|model data version:14|database format:BerkeleyDB|open counter:0|","user_bigram.db":null,"user_phrase_index.bin":"dbm","user_pinyin_index.bin":"dbm"},"logs":[],"save":true,"train":true,"view":{"nihao":["你好",true,128,["你好","尼号","你好吗","疒","祢","禰","妳","呢","堄","逆","猊","霓"]],"yishu":["艺术",true,605,["艺术","医术","艺术家","艾","饎","搋","眙","扡","誃","昳","斁","睪"]]}},"stderr":"open SCRATCH/user-X/user.conf failed."}},"table-conf-lambda-negative-two-zhuyin":{"pin":{"exit":0,"result":{"add":true,"files":{"addon.bin":27,"gb_char.dbin":26,"gbk_char.dbin":26,"merged.dbin":26,"network.bin":27,"opengram.dbin":26,"user.bin":71,"user.conf":"binary format version:7|model data version:14|database format:BerkeleyDB|open counter:0|","user_bigram.db":null,"user_phrase_index.bin":"dbm","user_pinyin_index.bin":"dbm"},"logs":[],"save":true,"train":true,"view":{"ni3hao3":["尼豪",true,127,["尼豪","你好","你","尼","呢","泥","妮","拟","逆","倪","腻","溺"]],"yi4shu4":["艺术",true,604,["艺术","医术","遗书","一束","一树","依属","一书","一","以","已","亿","亦"]]}},"stderr":"open SCRATCH/user-X/user.conf failed."},"subject":{"exit":0,"result":{"add":true,"files":{"addon.bin":27,"gb_char.dbin":26,"gbk_char.dbin":26,"merged.dbin":26,"network.bin":27,"opengram.dbin":26,"user.bin":71,"user.conf":"binary format version:7|model data version:14|database format:BerkeleyDB|open counter:0|","user_bigram.db":null,"user_phrase_index.bin":"dbm","user_pinyin_index.bin":"dbm"},"logs":[],"save":true,"train":true,"view":{"ni3hao3":["你好",true,126,["你好","你","尼","呢","泥","妮","拟","逆","倪","腻","溺","昵"]],"yi4shu4":["医术",true,604,["医术","艺术","遗书","一束","一树","依属","一书","一","以","已","亿","亦"]]}},"stderr":"open SCRATCH/user-X/user.conf failed."}},"table-conf-lambda-one-and-a-half":{"pin":{"exit":0,"result":{"add":true,"files":{"addon.bin":27,"gb_char.dbin":26,"gbk_char.dbin":26,"merged.dbin":26,"network.bin":27,"opengram.dbin":26,"user.bin":71,"user.conf":"binary format version:7|model data version:14|database format:BerkeleyDB|open counter:0|","user_bigram.db":null,"user_phrase_index.bin":"dbm","user_pinyin_index.bin":"dbm"},"logs":[],"save":true,"train":true,"view":{"nihao":["鯓虠",true,130,["鯓虠","鯓号","鯓嚆","你好吗","疒","祢","禰","妳","呢","堄","逆","猊"]],"yishu":["呭䜹",true,608,["呭䜹","呭墅","呭疋","艺术家","艾","饎","搋","眙","扡","誃","昳","斁"]]}},"stderr":"open SCRATCH/user-X/user.conf failed."},"subject":{"exit":0,"result":{"add":true,"files":{"addon.bin":27,"gb_char.dbin":26,"gbk_char.dbin":26,"merged.dbin":26,"network.bin":27,"opengram.dbin":26,"user.bin":71,"user.conf":"binary format version:7|model data version:14|database format:BerkeleyDB|open counter:0|","user_bigram.db":null,"user_phrase_index.bin":"dbm","user_pinyin_index.bin":"dbm"},"logs":[],"save":true,"train":true,"view":{"nihao":["你好",true,128,["你好","拟好","你好吗","疒","祢","禰","妳","呢","堄","逆","猊","霓"]],"yishu":["艺术",true,605,["艺术","医术","艺术家","艾","饎","搋","眙","扡","誃","昳","斁","睪"]]}},"stderr":"open SCRATCH/user-X/user.conf failed."}},"table-conf-lambda-one-and-a-half-zhuyin":{"pin":{"exit":0,"result":{"add":true,"files":{"addon.bin":27,"gb_char.dbin":26,"gbk_char.dbin":26,"merged.dbin":26,"network.bin":27,"opengram.dbin":26,"user.bin":71,"user.conf":"binary format version:7|model data version:14|database format:BerkeleyDB|open counter:0|","user_bigram.db":null,"user_phrase_index.bin":"dbm","user_pinyin_index.bin":"dbm"},"logs":[],"save":true,"train":true,"view":{"ni3hao3":["儞䝥",true,127,["儞䝥","你好","禰","妳","堄","儗","輗","蜺","鉨","齯","鶂","貎"]],"yi4shu4":["苅襩",true,605,["苅襩","依属","一树","一束","一书","遗书","医术","艺术","饎","搋","扡","誃"]]}},"stderr":"open SCRATCH/user-X/user.conf failed."},"subject":{"exit":0,"result":{"add":true,"files":{"addon.bin":27,"gb_char.dbin":26,"gbk_char.dbin":26,"merged.dbin":26,"network.bin":27,"opengram.dbin":26,"user.bin":71,"user.conf":"binary format version:7|model data version:14|database format:BerkeleyDB|open counter:0|","user_bigram.db":null,"user_phrase_index.bin":"dbm","user_pinyin_index.bin":"dbm"},"logs":[],"save":true,"train":true,"view":{"ni3hao3":["你好",true,126,["你好","禰","妳","堄","輗","鉨","齯","鶂","貎","膩","薿","柅"]],"yi4shu4":["艺术",true,604,["艺术","依属","一书","一树","一束","遗书","医术","饎","搋","扡","睪","愒"]]}},"stderr":"open SCRATCH/user-X/user.conf failed."}},"table-conf-lambda-two":{"pin":{"exit":0,"result":{"add":true,"files":{"addon.bin":27,"gb_char.dbin":26,"gbk_char.dbin":26,"merged.dbin":26,"network.bin":27,"opengram.dbin":26,"user.bin":71,"user.conf":"binary format version:7|model data version:14|database format:BerkeleyDB|open counter:0|","user_bigram.db":null,"user_phrase_index.bin":"dbm","user_pinyin_index.bin":"dbm"},"logs":[],"save":true,"train":true,"view":{"nihao":["鯓虠",true,130,["鯓虠","鯓号","鯓嚆","你好吗","疒","祢","禰","妳","呢","堄","逆","猊"]],"yishu":["呭䜹",true,608,["呭䜹","呭墅","呭疋","艺术家","艾","饎","搋","眙","扡","誃","昳","斁"]]}},"stderr":"open SCRATCH/user-X/user.conf failed."},"subject":{"exit":0,"result":{"add":true,"files":{"addon.bin":27,"gb_char.dbin":26,"gbk_char.dbin":26,"merged.dbin":26,"network.bin":27,"opengram.dbin":26,"user.bin":71,"user.conf":"binary format version:7|model data version:14|database format:BerkeleyDB|open counter:0|","user_bigram.db":null,"user_phrase_index.bin":"dbm","user_pinyin_index.bin":"dbm"},"logs":[],"save":true,"train":true,"view":{"nihao":["你好",true,128,["你好","拟好","你好吗","疒","祢","禰","妳","呢","堄","逆","猊","霓"]],"yishu":["医术",true,605,["医术","艺术","艺术家","艾","饎","搋","眙","扡","誃","昳","斁","睪"]]}},"stderr":"open SCRATCH/user-X/user.conf failed."}},"table-conf-lambda-two-zhuyin":{"pin":{"exit":0,"result":{"add":true,"files":{"addon.bin":27,"gb_char.dbin":26,"gbk_char.dbin":26,"merged.dbin":26,"network.bin":27,"opengram.dbin":26,"user.bin":71,"user.conf":"binary format version:7|model data version:14|database format:BerkeleyDB|open counter:0|","user_bigram.db":null,"user_phrase_index.bin":"dbm","user_pinyin_index.bin":"dbm"},"logs":[],"save":true,"train":true,"view":{"ni3hao3":["儞䝥",true,127,["儞䝥","你好","禰","妳","堄","儗","輗","鉨","齯","鶂","貎","膩"]],"yi4shu4":["苅襩",true,605,["苅襩","依属","一树","一书","一束","遗书","医术","艺术","饎","搋","扡","睪"]]}},"stderr":"open SCRATCH/user-X/user.conf failed."},"subject":{"exit":0,"result":{"add":true,"files":{"addon.bin":27,"gb_char.dbin":26,"gbk_char.dbin":26,"merged.dbin":26,"network.bin":27,"opengram.dbin":26,"user.bin":71,"user.conf":"binary format version:7|model data version:14|database format:BerkeleyDB|open counter:0|","user_bigram.db":null,"user_phrase_index.bin":"dbm","user_pinyin_index.bin":"dbm"},"logs":[],"save":true,"train":true,"view":{"ni3hao3":["你好",true,126,["你好","禰","妳","堄","輗","鉨","齯","鶂","貎","膩","薿","柅"]],"yi4shu4":["亦属",true,605,["亦属","依属","一书","一树","一束","遗书","医术","艺术","饎","搋","扡","睪"]]}},"stderr":"open SCRATCH/user-X/user.conf failed."}},"table-conf-lambda-million":{"pin":{"exit":0,"result":{"add":true,"files":{"addon.bin":27,"gb_char.dbin":26,"gbk_char.dbin":26,"merged.dbin":26,"network.bin":27,"opengram.dbin":26,"user.bin":71,"user.conf":"binary format version:7|model data version:14|database format:BerkeleyDB|open counter:0|","user_bigram.db":null,"user_phrase_index.bin":"dbm","user_pinyin_index.bin":"dbm"},"logs":[],"save":true,"train":true,"view":{"nihao":["鯓虠",true,130,["鯓虠","鯓号","鯓嚆","你好吗","疒","祢","禰","妳","呢","堄","逆","猊"]],"yishu":["呭䜹",true,608,["呭䜹","呭墅","呭疋","艺术家","艾","饎","搋","眙","扡","誃","昳","斁"]]}},"stderr":"open SCRATCH/user-X/user.conf failed."},"subject":{"exit":0,"result":{"add":true,"files":{"addon.bin":27,"gb_char.dbin":26,"gbk_char.dbin":26,"merged.dbin":26,"network.bin":27,"opengram.dbin":26,"user.bin":71,"user.conf":"binary format version:7|model data version:14|database format:BerkeleyDB|open counter:0|","user_bigram.db":null,"user_phrase_index.bin":"dbm","user_pinyin_index.bin":"dbm"},"logs":[],"save":true,"train":true,"view":{"nihao":["你好",true,128,["你好","呢好","你好吗","疒","祢","禰","妳","呢","堄","逆","猊","霓"]],"yishu":["医术",true,605,["医术","艺术","艺术家","艾","饎","搋","眙","扡","誃","昳","斁","睪"]]}},"stderr":"open SCRATCH/user-X/user.conf failed."}},"table-conf-lambda-million-zhuyin":{"pin":{"exit":0,"result":{"add":true,"files":{"addon.bin":27,"gb_char.dbin":26,"gbk_char.dbin":26,"merged.dbin":26,"network.bin":27,"opengram.dbin":26,"user.bin":71,"user.conf":"binary format version:7|model data version:14|database format:BerkeleyDB|open counter:0|","user_bigram.db":null,"user_phrase_index.bin":"dbm","user_pinyin_index.bin":"dbm"},"logs":[],"save":true,"train":true,"view":{"ni3hao3":["儞䝥",true,127,["儞䝥","你好","尼","禰","妳","堄","輗","鉨","齯","鶂","貎","膩"]],"yi4shu4":["苅襩",true,605,["苅襩","依属","一书","一树","一束","遗书","医术","艺术","易","亿","已","意"]]}},"stderr":"open SCRATCH/user-X/user.conf failed."},"subject":{"exit":0,"result":{"add":true,"files":{"addon.bin":27,"gb_char.dbin":26,"gbk_char.dbin":26,"merged.dbin":26,"network.bin":27,"opengram.dbin":26,"user.bin":71,"user.conf":"binary format version:7|model data version:14|database format:BerkeleyDB|open counter:0|","user_bigram.db":null,"user_phrase_index.bin":"dbm","user_pinyin_index.bin":"dbm"},"logs":[],"save":true,"train":true,"view":{"ni3hao3":["你好",true,126,["你好","禰","妳","堄","輗","鉨","齯","鶂","貎","膩","薿","柅"]],"yi4shu4":["医术",true,604,["医术","依属","一书","一树","一束","遗书","艺术","饎","搋","扡","睪","愒"]]}},"stderr":"open SCRATCH/user-X/user.conf failed."}},"table-conf-lambda-inf":{"pin":{"exit":0,"result":{"add":true,"files":{"addon.bin":27,"gb_char.dbin":26,"gbk_char.dbin":26,"merged.dbin":26,"network.bin":27,"opengram.dbin":26,"user.bin":71,"user.conf":"binary format version:7|model data version:14|database format:BerkeleyDB|open counter:0|","user_bigram.db":null,"user_phrase_index.bin":"dbm","user_pinyin_index.bin":"dbm"},"logs":[],"save":true,"train":true,"view":{"nihao":["鯓虠",true,130,["鯓虠","鯓号","鯓嚆","你好吗","疒","祢","禰","妳","呢","堄","逆","猊"]],"yishu":["呭䜹",true,608,["呭䜹","呭墅","呭疋","艺术家","艾","饎","搋","眙","扡","誃","昳","斁"]]}},"stderr":"open SCRATCH/user-X/user.conf failed."},"subject":{"exit":0,"result":{"add":true,"files":{"addon.bin":27,"gb_char.dbin":26,"gbk_char.dbin":26,"merged.dbin":26,"network.bin":27,"opengram.dbin":26,"user.bin":71,"user.conf":"binary format version:7|model data version:14|database format:BerkeleyDB|open counter:0|","user_bigram.db":null,"user_phrase_index.bin":"dbm","user_pinyin_index.bin":"dbm"},"logs":[],"save":true,"train":true,"view":{"nihao":["你好",true,128,["你好","呢好","你好吗","疒","祢","禰","妳","呢","堄","逆","猊","霓"]],"yishu":["医术",true,605,["医术","艺术","艺术家","艾","饎","搋","眙","扡","誃","昳","斁","睪"]]}},"stderr":"open SCRATCH/user-X/user.conf failed."}},"table-conf-lambda-inf-zhuyin":{"pin":{"exit":0,"result":{"add":true,"files":{"addon.bin":27,"gb_char.dbin":26,"gbk_char.dbin":26,"merged.dbin":26,"network.bin":27,"opengram.dbin":26,"user.bin":71,"user.conf":"binary format version:7|model data version:14|database format:BerkeleyDB|open counter:0|","user_bigram.db":null,"user_phrase_index.bin":"dbm","user_pinyin_index.bin":"dbm"},"logs":[],"save":true,"train":true,"view":{"ni3hao3":["儞䝥",true,127,["儞䝥","你好","疒","祢","禰","妳","呢","堄","逆","猊","霓","儗"]],"yi4shu4":["苅襩",true,605,["苅襩","医术","艺术","依属","一树","一束","一书","遗书","艾","饎","搋","眙"]]}},"stderr":"open SCRATCH/user-X/user.conf failed."},"subject":{"exit":0,"result":{"add":true,"files":{"addon.bin":27,"gb_char.dbin":26,"gbk_char.dbin":26,"merged.dbin":26,"network.bin":27,"opengram.dbin":26,"user.bin":71,"user.conf":"binary format version:7|model data version:14|database format:BerkeleyDB|open counter:0|","user_bigram.db":null,"user_phrase_index.bin":"dbm","user_pinyin_index.bin":"dbm"},"logs":[],"save":true,"train":true,"view":{"ni3hao3":["你好",true,126,["你好","疒","祢","禰","妳","呢","堄","逆","猊","霓","儗","輗"]],"yi4shu4":["医术",true,604,["医术","艺术","依属","一树","一束","一书","遗书","艾","饎","搋","眙","扡"]]}},"stderr":"open SCRATCH/user-X/user.conf failed."}},"table-conf-lambda-nan":{"pin":{"exit":0,"result":{"add":true,"files":{"addon.bin":27,"gb_char.dbin":26,"gbk_char.dbin":26,"merged.dbin":26,"network.bin":27,"opengram.dbin":26,"user.bin":71,"user.conf":"binary format version:7|model data version:14|database format:BerkeleyDB|open counter:0|","user_bigram.db":null,"user_phrase_index.bin":"dbm","user_pinyin_index.bin":"dbm"},"logs":[],"save":true,"train":true,"view":{"nihao":["鯓虠",true,130,["鯓虠","鯓号","鯓嚆","你好吗","疒","祢","禰","妳","呢","堄","逆","猊"]],"yishu":["呭䜹",true,608,["呭䜹","呭墅","呭疋","艺术家","艾","饎","搋","眙","扡","誃","昳","斁"]]}},"stderr":"open SCRATCH/user-X/user.conf failed."},"subject":{"exit":0,"result":{"add":true,"files":{"addon.bin":27,"gb_char.dbin":26,"gbk_char.dbin":26,"merged.dbin":26,"network.bin":27,"opengram.dbin":26,"user.bin":71,"user.conf":"binary format version:7|model data version:14|database format:BerkeleyDB|open counter:0|","user_bigram.db":null,"user_phrase_index.bin":"dbm","user_pinyin_index.bin":"dbm"},"logs":[],"save":true,"train":true,"view":{"nihao":["你好",true,128,["你好","呢好","你好吗","疒","祢","禰","妳","呢","堄","逆","猊","霓"]],"yishu":["医术",true,605,["医术","艺术","艺术家","艾","饎","搋","眙","扡","誃","昳","斁","睪"]]}},"stderr":"open SCRATCH/user-X/user.conf failed."}},"table-conf-lambda-nan-zhuyin":{"pin":{"exit":0,"result":{"add":true,"files":{"addon.bin":27,"gb_char.dbin":26,"gbk_char.dbin":26,"merged.dbin":26,"network.bin":27,"opengram.dbin":26,"user.bin":71,"user.conf":"binary format version:7|model data version:14|database format:BerkeleyDB|open counter:0|","user_bigram.db":null,"user_phrase_index.bin":"dbm","user_pinyin_index.bin":"dbm"},"logs":[],"save":true,"train":true,"view":{"ni3hao3":["儞䝥",true,127,["儞䝥","你好","疒","祢","禰","妳","呢","堄","逆","猊","霓","儗"]],"yi4shu4":["苅襩",true,605,["苅襩","医术","艺术","依属","一树","一束","一书","遗书","艾","饎","搋","眙"]]}},"stderr":"open SCRATCH/user-X/user.conf failed."},"subject":{"exit":0,"result":{"add":true,"files":{"addon.bin":27,"gb_char.dbin":26,"gbk_char.dbin":26,"merged.dbin":26,"network.bin":27,"opengram.dbin":26,"user.bin":71,"user.conf":"binary format version:7|model data version:14|database format:BerkeleyDB|open counter:0|","user_bigram.db":null,"user_phrase_index.bin":"dbm","user_pinyin_index.bin":"dbm"},"logs":[],"save":true,"train":true,"view":{"ni3hao3":["你好",true,126,["你好","疒","祢","禰","妳","呢","堄","逆","猊","霓","儗","輗"]],"yi4shu4":["医术",true,604,["医术","艺术","依属","一树","一束","一书","遗书","艾","饎","搋","眙","扡"]]}},"stderr":"open SCRATCH/user-X/user.conf failed."}}}}
```


## Final-pass integration STOP — 2026-10-27 23:52 UTC

The final pass rebased the three ranking commits onto main
`a52461cc297a4e67b07406167e6581e0e9f211fa` without textual conflicts.
Main's `399b813e` exposes provisioned uv through `GITHUB_PATH`; the six
unordered shipping bootstrap builds completed on sheng-G7-7790. Main changed
the function-order generator to include the empty profile and changed its
packaging resolver, including Mach-O derivation. The six committed
`tools/packaging/function-order/{bdb,kc,tkrzw}/lib{pinyin,zhuyin}.order`
artifacts therefore require regeneration. No policy generation or shipping
measurement was started before the STOP below. Mach-O order files are derived
at installation, rather than additional committed artifacts.

The existing 175-case selection was recovered by case ID from the retained
ranking observations and run once against final-source BDB debug libraries.
Two preceding invocations refused before case execution (a wrong library
basename, then measurement-only function-order arguments); neither supplied
results. The completed run reports **56 MATCH / 112 POLICY-ACCEPTED / 7 DIFFER**,
against the earlier pre-scanner 61 / 114 / 0. Reproduce with the existing
contract driver, the same 175 case IDs, both debug C facades, and its
`--observations` output; no expectations were regenerated or weakened.

The seven DIFFER cases are `table-conf-lambda-inf`,
`table-conf-lambda-inf-zhuyin`, `table-conf-lambda-nan`,
`table-conf-lambda-nan-zhuyin`, `table-conf-lambda-hex-zhuyin`,
`table-conf-lambda-hex-fraction-zhuyin`, and
`table-conf-lambda-hex-upper-zhuyin`. Their retained observations show pin
initialization succeeds, while oxpinyin returns `ctx=false` and emits
`load SCRATCH/sys-X/table.conf failed!`. The two pinyin inf/NaN cases also
leave the user open counter at zero rather than one. These are initialization
observations, not tournament ordering.

Source inspection identifies a registration-composition conflict:
`contract-diff.py`'s `_SCANNER_RECORDS` replaces these probes with
`scanner_initialization` and registers `1297-table-floats.md`; later ranking
loops overwrite seven `policy_record` values with this record or
`1297-ranking-score.md`. Main's `TableInfo` decimal grammar refuses these
spellings. The current final-pass instruction requires STOP on every DIFFER,
so no precedence fix, new classification or expectation update was made.
The proposed treatment for a maintainer ruling is to preserve the scanner
registration on probes that stop during initialization, retaining ranking
records on probes that reach ranking. This remains pending, not applied.

The bounded stable tail insertion is equivalent to the previous stable Cost
sort, tested over every arrangement of zero through three representative
costs including ties, infinities and NaN. Candidate/prediction full stable
sorts remain distinct: `(RankKey, usize)` and `Predicted` have different
layouts and enabled comparison fields, so sharing their sorter instance
would require restructuring beyond the shared score key.

Engine, pinyin C API and oracle crate tests and clippy passed. The changed-tool
gate passed with disk-backed TMPDIR and provisioned runtime/loader overrides
removed from isolated harness fixtures. Existing Axis C layout cases do put
chewing input through the pinyin facade's `guess_sentence` and candidate
collection, but no additional coverage run was made after STOP. Candidate
recount, six-policy generation, linked-policy verification, other sentence
surfaces, all measurement and new stripped sizes remain unrun. The exclusive
heavy hold was released after all related processes ended. Retained earlier
figures remain historical and do not describe these changed binaries.


### STOP resolved by maintainer — 2026-10-28 00:26 UTC

The maintainer accepted scanner precedence for the seven initialization-only
probes. `register_policy` now refuses duplicate record registration without
explicit composition. Ranking's explicit composition retains
`1297-table-floats.md` only on a scanner probe marked initialization-only;
unhandled overlaps fail before mutating the original record. Ranking applies
where the probe reaches ranking. No record expectation or classification was
changed. Contract-driver regressions cover duplicate refusal, preservation of
the original registration, explicit composition, all seven scanner probes,
and an unrelated overlap's hard failure.

The existing BDB selection ran again with the unchanged 175 case IDs and
final-source debug libraries: **56 MATCH / 119 POLICY-ACCEPTED / 0 DIFFER**.
All seven cases are POLICY-ACCEPTED under `1297-table-floats.md`; there was no
other classification movement. The source-change completion gates from the
preceding pass remain applicable; the changed tooling is gated again.

The same existing candidate-attribution worker now names separate input
strata. `full_pinyin` uses the same nine committed corpus files in both real
facades. `chewing_standard`, on the pinyin facade only, uses the committed
Axis C `layout_inputs`, standard scheme 0, and `parse_more_chewings`. It
retains complete windows at offset zero without calling `guess_sentence`,
so this is score/dedup attribution, separate from sentence/tournament
counts. Candidate counts must agree with the complete row windows. Order-only
changes require identical row multisets; row-multiset changes include changed
multiplicity. Parse fields and other protected envelope fields remain strict.
Each class reports changed windows, first candidate, top five and largest
changed one-based position. No Bopomofo corpus or one-store simulation is
substituted for the two actual facade results.
