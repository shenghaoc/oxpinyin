# #918 — G1 prefix processing before a non-scalar

Status: implemented; compatibility gates pending · 2026-10-24 UTC.
This is G1 coverage under existing class (c), not a new discrepancy or
POLICY-ACCEPTED runtime quirk. The source-supported result is **identical
under class (c)**: the pin aborts; oxpinyin refuses with the ordinary
row-85 warning, false return and preserved old phrase result.

## Input and source derivation

Pin source: `074a2219c90feaf962d0d24f034514033ece5f99`, GLib 2.89.4-2,
glibc 2.43-3; both facades and the three backend wrappers. Paths below
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

Both facade `phrase.rs::segment_text` functions retain their real GLib
conversion and `g_free`. Successful Rust-invalid conversion retains the
scalar prefix ending at `Utf8Error::valid_up_to()` and the full converted
length. Rejected conversion still emits the same critical and preserves
the old result. Scalar input still uses the existing ordinary decoder.

Both `*_phrase_segment` exports call
`engine/src/session/guess.rs::Session::check_phrase_prefix`, then
`engine/src/phrase.rs::check_phrase_prefix`. This private walk orders
spans before predecessor rows, performs bigram relaxations before the
span's unigram relaxations, and loads a row only at a matching span of a
reachable step. Its scratch result is never published. It reuses
`step_costs`, `MergedGram` and `EngineError::ShortLookupPredecessor`.
The facade reuses `sentence.rs::refuse_lookup_gram`: exactly the existing
row-85 warning text/domain, false return, old result untouched. A
successful prefix check publishes the full-length zero-token result and
returns false silently. Neither path stores/trains a gram.

These two private conversion helpers have only their own facade export
as a caller. The neighboring `ffi.rs` helpers remain unchanged; their
independent audit is in the [follow-through](918-boundary-follow-through.md).
ABI/layout, real GLib allocation ownership and file formats are unchanged.
No valid scalar input enters this new path. This bounded conclusion does
not admit non-scalar dictionary keys, backend IO failures, invalid item
bounds, extreme lengths or allocation failure.

## Cases and verification

The existing contract driver registers, for each of `pinyin` and `zhuyin`:

- `abort-phrase-segment-prefix-short-user-gram-<facade>`: pin SIGABRT;
  subject false, one facade-domain warning, all old result tokens kept.
- `phrase-segment-prefix-sound-user-gram-<facade>`: false, two zero
  tokens, no warning on both sides.
- `phrase-segment-nonscalar-before-short-user-gram-<facade>` and
  `phrase-segment-prefix-unrelated-short-user-gram-<facade>`: silent
  false/two-zero controls; no broad scan for corrupt grams.

The old result is seeded with three non-scalars, giving three zero tokens
without reading a gram. The corrupt case checks every old token and its
cardinality; all cases check out-of-range getter zeroing. The ordinary
scalar pinyin case remains `abort-phrase-segment-short-user-gram`.

Native tests passed for `oxpinyin-engine`, `oxpinyin-capi` and
`oxpinyin-zhuyin-capi`; fmt/clippy passed. The engine checks both the
short-gram refusal and competing faults whose span/predecessor orders
differ. The 141 Python driver tests passed. Compatibility execution,
the named `skip-prefix-processing` mutation, and shipping-size checks
remain pending; no runtime MATCH/POLICY-ACCEPTED result is claimed.

Reproduction after provisioning the documented toolchain:

```sh
just heavy -- cargo test -p oxpinyin-engine -p oxpinyin-capi -p oxpinyin-zhuyin-capi
cargo fmt --all -- --check
just heavy -- cargo clippy -p oxpinyin-engine -p oxpinyin-capi -p oxpinyin-zhuyin-capi --all-targets -- -D warnings
uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal tools/bisection/contract-diff.test.py
bash tools/oracle/compat.sh --runtime docker --cell bdb --issue 918
```

The named mutation must skip both facade calls to `check_phrase_prefix`,
keep the final zero-token return, and make both corrupt-prefix cases
DIFFER while sound controls remain MATCH. Restore it before the final
build. This describes the pending mutation gate, not an executed result.
