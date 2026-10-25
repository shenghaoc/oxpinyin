# #918 — skipped prefix read hides Kyoto Cabinet's second-read assert

Source-established conditional fault path · 2026-10-24 UTC · #918.
**Class (c); implemented in the current prefix path.** The `1c5d96b9`
shortcut omitted the required warning; the raw UCS-4 span path now
performs the pin's checked read and substitutes false plus one warning.
No exception admission and no executed fault injection.

At libpinyin `074a2219`, both phrase-segment exports reach
`lookup/phrase_lookup.cpp:255-276` for a matching scalar prefix before a
later non-scalar. With input `e4 bd a0 ed a0 80 00`, sound scalar phrase
keys/items and successful GLib 2.84.4 (trixie; the testing run used
2.89.4-2, whose cited conversion paths are unchanged) conversion, the system gram lookup
uses the kc `Bigram::load` at `storage/ngram_kyotodb.cpp:124-142`.

The precise failure schedule is: `check` returns nonnegative size S,
then `get` returns -1 because its second read fails. For example, an IO
failure can occur on that later body read after the earlier metadata/size
visit succeeded. This describes the required backend return sequence;
it is not a claim that a static healthy file changes size, or that every
corrupt record passes the first check.

Kyoto Cabinet 1.2.80-2 source `kcdb.h:1985-2041` makes these separate
read-only visits. A failed visitor returns -1. HashDB's
`kchashdb.h:1837-1855,2962-3015,3116-3140` propagates a file read failure;
`set_error` records it with a NULL logger by default. The pin's
`check_result(S == -1)` at `ngram_kyotodb.cpp:138-139` then asserts
(`include/pinyin_utils.h:27-31`, asserts-live reference build). No binary
ambiguity is needed to identify the inequality or assertion.

The pin terminates with an assertion diagnostic before `final_step` and
before replacing the public phrase-result array. The `1c5d96b9` shortcut
returned false with `[0,0]` and no warning without entering the backend.
The current raw span path performs the pin's checked read; a body failure
after a successful check is the class-(c) false plus one warning
described below. Neither path writes a gram, and neither does the pin's
attempted read. Rust's getters remain usable; there are no post-abort pin
getter observations to equate with them.

The backward path is the private segment helper's sole export caller per
facade. Normal scalar lookup errors are routed through the engine and
`sentence.rs::refuse_lookup_gram`; the checked-read error is now one of
them. A first-check failure remains a separate, silent no-gram path, as
`Bigram::load` returns false. The analogous two-call phrase-table check is
at `storage/phrase_large_table3_kyotodb.cpp:175-190` and can fail before
any bigram consultation; it is not a successful prefix lookup.

Class (c) takes precedence over any malformed-text quirk admission.
Returning false silently would not meet its warning rule. The current
path returns false and logs exactly one warning. The ordinary short-value
assert remains row 85; this record covers the distinct two-read schedule.

## Current head implementation

`BigramTable::load_raw_successors` now calls `ChewingDbm::get_bigram`.
KC overrides it with `get_bigram_raw`: `kcdbcheck` returns the value size
(a failed check is a silent miss, as `Bigram::load` returns false), then
`kcdbaccept` reads the body. If the check found the record and the body
read fails or writes a different size, the store returns
`StoreError::ReadAfterCheck`; `LmError::is_read_after_check` carries it to
the engine, which returns `EngineError::ReadAfterCheck`; both facades'
`refuse_lookup_gram` logs one warning in their domain and returns false
with the old phrase result kept. This is the class-(c) substitution for
the pin's `check_result` assert.

Unit tests cover the seams:
`bigram_table::checked_read_failure_is_not_a_miss`,
`phrase::a_checked_read_failure_is_a_class_c_refusal`, and
`phrase::read_after_check_emits_one_class_c_warning` in both facades. The
kc cell also ran the ten #918 contract cases 10/10 MATCH on the ordinary
path; the fault schedule itself is not injected.

Reproduce the derivation with `git show
074a2219:src/storage/ngram_kyotodb.cpp` and the cited patched kc source.
See the [backend partition table](918-boundary-follow-through.md#backend-failures-during-prefix-consultation)
for normal returned failures and the distinct short-value assertion.
