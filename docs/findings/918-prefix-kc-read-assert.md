# #918 — skipped prefix read hides Kyoto Cabinet's second-read assert

Source-established conditional fault path · 2026-10-24 UTC · #918.
**Class (c); current shortcut omits its required warning.** No exception
admission, implementation change or executed fault injection.

At libpinyin `074a2219`, both phrase-segment exports reach
`lookup/phrase_lookup.cpp:255-276` for a matching scalar prefix before a
later non-scalar. With input `e4 bd a0 ed a0 80 00`, sound scalar phrase
keys/items and successful GLib 2.89.4-2 conversion, the system gram lookup
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
before replacing the public phrase-result array. Both Rust facades'
`phrase.rs::SegmentText::Unmatched` instead return false with `[0,0]`
and no warning, without entering the backend. They write no gram, and
neither does the pin's attempted read. Rust's getters remain usable;
there are no post-abort pin getter observations to equate with them.

The backward path is the private segment helper's sole export caller
per facade. Normal scalar lookup errors are routed through the engine
and `sentence.rs::refuse_lookup_gram`; that route cannot supply a warning
on this shortcut. A first-check failure is a separate, silent no-gram
path. The analogous two-call phrase-table check is at
`storage/phrase_large_table3_kyotodb.cpp:175-190` and can fail before any
bigram consultation; it is not a successful prefix lookup.

Class (c) takes precedence over any malformed-text quirk admission.
Returning false silently does not meet its warning rule. Keep the early
return unchanged under the current instruction; this record neither
waives the warning nor chooses a new refusal lifecycle.

Reproduce the derivation with `git show
074a2219:src/storage/ngram_kyotodb.cpp` and the cited patched kc source.
The existing contract driver would need a separately authorized,
source-derived fault case to gate a fix; no such case or run is claimed.
See the [backend partition table](918-boundary-follow-through.md#backend-failures-during-prefix-consultation)
for normal returned failures and the distinct short-value assertion.
