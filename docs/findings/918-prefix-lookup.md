# #918 — G1 prefix processing before a non-scalar

Status: implemented with raw-UCS-4 spans; bdb gate and named mutation passed ·
2026-10-24 UTC. This is G1 coverage under existing class (c), not a new
discrepancy or POLICY-ACCEPTED runtime quirk. The source-supported result
for the short gram is **identical under class (c)**: the pin aborts;
oxpinyin refuses with the ordinary row-85 warning, false return and
preserved old phrase result. The same raw path returns the pin's
true/token when the table holds the non-scalar key
([record](918-nonscalar-token-result.md)).

## Input and source derivation

Pin source: `074a2219c90feaf962d0d24f034514033ece5f99`, GLib
2.84.4-3~deb13u5, glibc 2.41-12+deb13u4 (trixie; amended 2026-10-25 UTC);
both facades and the three backend wrappers. Paths below
start at `src/`. The bounded trigger is `e4 bd a0 ed a0 80 00`: `你`,
U+D800's historical UTF-8 encoding, NUL. The scalar phrase table and
items are consistent; the reached sentence-start predecessor (token 1)
has a sound system gram and user value `01 00 00`. Both reads succeed.

1. `pinyin.cpp:1446-1456` / `zhuyin.cpp:965-975` count and convert two
   words. GLib `gutf8.c:665-747,940-997` accepts the surrogate; it is not
   a rejected malformed/overlong sequence under that converter.
2. `lookup/phrase_lookup.cpp:120-149` seeds token 1, then searches each
   span in increasing start/end order. A matching span invokes
   `search_bigram2` before `search_unigram2`.
3. `phrase_lookup.cpp:255-302` visits the reached step's predecessors in
   insertion order, loads each system/user gram, merges, then visits the
   matched tokens. It reads no predecessor at an unreachable step.
   `storage/ngram_bdb.cpp:127-146`, `ngram_kyotodb.cpp:124-142` and
   `ngram_tkrzwdb.cpp:102-121` deliver the raw user value; initialization
   copies it without reading its total (`pinyin.cpp:398-401`,
   `zhuyin.cpp:340-343`).
4. With both grams present, `storage/ngram.cpp:298-304` reads both
   totals. `SingleGram::get_total_freq` (`:48-51`) requests four bytes;
   `include/memory_chunk.h:372-391` checks the three-byte bound and
   asserts before copying. `include/pinyin_utils.h:27-31` defines
   `check_result` as assert in the reference build.
5. The pin aborts before `final_step` (`phrase_lookup.cpp:382-397`),
   before replacing the public result and before the caller's `g_free`.
   There is no post-abort pin getter observation. Class (c) requires
   one facade-domain warning and the ordinary false refusal instead.

For the sound-user-gram twin, prefix processing completes. No scalar
key crosses the non-scalar, so the pin finalizes false with **two zero
tokens**, no diagnostic and no learned/persistent update. A non-scalar
first makes later scalar spans unreachable; a short gram under another
predecessor is never read. These are silent controls, not row-85 refusals.
Source settles these paths; no binary question or runtime probe was
needed to derive them.

## Rust path and observable channels

Both facade `phrase.rs::segment_text` functions decode valid UTF-8 as
text; a Rust-invalid but historically valid sequence goes through
`oxpinyin_core::ucs4::decode` to raw `u32` words (canonical 1–6 byte
forms, no `char`/`str`, no GLib converter). A rejected sequence still
emits the same critical and preserves the old result. Scalar input still
uses the existing ordinary decoder.

Each `*_phrase_segment` export calls `Session::phrase_segment` for text
and `Session::phrase_segment_ucs4` for the raw words. The shared span DP
walks starts and ends in order, loads a predecessor row only at a matching
span of a reachable step, and runs the bigram then unigram relaxations in
the pin's order. It reuses `step_costs`, `MergedGram` and the existing
`EngineError` refusals. `Dictionary::tokens_for_ucs4` reads the system and
user phrase tables with the raw key; the item's declared length and
unigram frequency price it without rendering text. The facade reuses
`sentence.rs::refuse_lookup_gram`: exactly the existing row-85 warning
text/domain, false return, old result untouched. A successful completion
publishes the pin's result — true/token for a raw key match, or the
length-sized all-null failure array — and stores/trains no gram.

The decoder and raw dictionary methods have only the facade exports and
the segmenter as callers. The neighboring `ffi.rs` helpers remain
unchanged; their independent audit is in the
[follow-through](918-boundary-follow-through.md). ABI/layout, real GLib
allocation ownership and file formats are unchanged. No valid scalar
input enters the raw path. This bounded conclusion does not admit backend
IO failures, invalid item bounds, extreme lengths or allocation failure.

## Cases and verification

The existing contract driver registers, for each of `pinyin` and `zhuyin`:

- `abort-phrase-segment-prefix-short-user-gram-<facade>`: pin SIGABRT;
  subject false, one facade-domain warning, all old result tokens kept.
- `phrase-segment-nonscalar-raw-key-<facade>`: a private table key
  `[0xD800]` and item; pin and subject true with that token, no warning,
  old result replaced ([record](918-nonscalar-token-result.md)).
- `phrase-segment-prefix-sound-user-gram-<facade>`: false, two zero
  tokens, no warning on both sides.
- `phrase-segment-nonscalar-before-short-user-gram-<facade>` and
  `phrase-segment-prefix-unrelated-short-user-gram-<facade>`: silent
  false/two-zero controls; no broad scan for corrupt grams.

The prefix cases seed the old result with three non-matching non-scalars,
giving three zero tokens without reading a gram. The raw-key case seeds a
different non-scalar (U+DFFF) and then matches U+D800. The corrupt case
checks every old token and its cardinality; all cases check out-of-range
getter zeroing. The ordinary scalar pinyin case remains
`abort-phrase-segment-short-user-gram`.

Native tests passed for `oxpinyin-core`, `oxpinyin-data`,
`oxpinyin-engine`, `oxpinyin-runtime`, `oxpinyin-user`,
`oxpinyin-store`, `oxpinyin-capi` and `oxpinyin-zhuyin-capi`; fmt and
clippy passed. The engine checks both the short-gram refusal and
competing faults whose span/predecessor orders differ. On linux/amd64 the bdb reference cell, kc and tkrzw each ran the ten
registered cases 10/10 MATCH.
The named `disable-raw-ucs4-lookup` mutation replaced the raw span lookup
with an empty result while keeping the result sizing; after rebuilding,
`--issue 918 --expect-parent` made both corrupt-prefix cases and both
raw-key cases DIFFER and the six controls MATCH. Restored, rebuilt and
rerun, the same ten cases returned 10/10 MATCH. Shipping sizes are
recorded in the PR.

Reproduction after provisioning the documented toolchain:

```sh
just heavy -- cargo test -p oxpinyin-core -p oxpinyin-data -p oxpinyin-engine \
    -p oxpinyin-runtime -p oxpinyin-user -p oxpinyin-store \
    -p oxpinyin-capi -p oxpinyin-zhuyin-capi
cargo fmt --all -- --check
just heavy -- cargo clippy -p oxpinyin-core -p oxpinyin-data -p oxpinyin-engine \
    -p oxpinyin-runtime -p oxpinyin-user -p oxpinyin-store \
    -p oxpinyin-capi -p oxpinyin-zhuyin-capi --all-targets -- -D warnings
bash tools/cloud/setup.sh --cells bdb
. ~/.oxpinyin-oracle.env
export CARGO_TARGET_DIR=target-bdb
just heavy -- cargo build -p oxpinyin-capi -p oxpinyin-zhuyin-capi
just heavy -- bash tools/bisection/run-contract-diff.sh bdb "$PINYIN_ORACLE_PREFIX" \
    "$CARGO_TARGET_DIR/debug/libpinyin_capi.so" \
    "$CARGO_TARGET_DIR/debug/libzhuyin_capi.so" -- --issue 918
```

The mutation is a temporary edit to
`engine/src/phrase.rs::phrase_segment_ucs4`; it was removed before the
final build and is not committed.
