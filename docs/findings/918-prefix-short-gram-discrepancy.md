# #918 — non-scalar shortcut omits the reached short-gram warning

- **Status:** source-established defect, implementation unchanged ·
  2026-10-24 UTC · [#918](https://github.com/shenghaoc/oxpinyin/issues/918).
- **Disposition:** not POLICY-ACCEPTED; no new runtime-quirk admission.
- **Policy:** decision-order limb 2, existing class (c),
  [compatibility-policy](compatibility-policy.md), row 85 and its lookup
  amendment. An assert avoided by returning false owes exactly one
  facade-domain warning. The present silent return fails that obligation.
- **Pin:** libpinyin 2.11.92 source at
  `074a2219c90feaf962d0d24f034514033ece5f99`; GLib 2.89.4-2, glibc
  2.43-3 reference environment. Both facades; bdb, kc and tkrzw wrappers.
  Paths below start at `src/` in that pin. No binary was necessary to
  settle this path's state changes.
- **Rust baseline:** `1c5d96b9605a3e5f1caef3a79fbb653707fd031f`;
  both `crates/oxpinyin-{capi,zhuyin-capi}/src/phrase.rs`,
  `segment_text`, `SegmentText::Unmatched`, `*_phrase_segment`;
  both `sentence.rs::refuse_lookup_gram`.

## Trigger and forward trace

Use a live instance with a consistent scalar dictionary containing `你`
at offset zero. Give predecessor `sentence_start` (token 1) a sound system
gram and a user gram whose raw value is exactly `01 00 00` (three bytes).
The backend database itself is well-formed and both reads succeed. The
C input is `e4 bd a0 ed a0 80 00`: `你`, followed by the canonical
historical encoding of U+D800, followed by NUL. Other dictionary keys
and items are scalar and consistent. This is a source-defined fixture
condition, not a claim that this fixture was executed.

1. `pinyin.cpp:1446-1456` / `zhuyin.cpp:965-975` count two words and
   successfully convert them. GLib `gutf8.c:665-747,940-997` accepts
   this surrogate encoding; it is not an ill-formed/overlong sequence
   under that converter's rules.
2. `lookup/phrase_lookup.cpp:120-149` seeds token 1 and searches the
   first scalar span. Its `SEARCH_OK` invokes `search_bigram2` before
   the unigram branch, before reaching the later non-scalar.
3. `phrase_lookup.cpp:255-276` loads both predecessor grams. Raw values
   reach `SingleGram` through `storage/ngram_bdb.cpp:127-146`,
   `storage/ngram_kyotodb.cpp:124-142`, or
   `storage/ngram_tkrzwdb.cpp:102-121`. Loading does not reject the
   three-byte total. Initialization loads user data into memory
   (`pinyin.cpp:398-401`, `zhuyin.cpp:340-343`); it does not validate
   every total by reading it. These are the existing row-85 conditions.
4. With both grams present, `storage/ngram.cpp:298-304` reads both
   totals. `SingleGram::get_total_freq` (`:48-51`) requests a guint32.
   `include/memory_chunk.h:372-391` returns false for the three-byte
   source and `check_result` asserts. `include/pinyin_utils.h:27-31`
   defines that macro as `assert` in the asserts-live reference build.
5. The pin emits the assertion diagnostic and aborts before
   `final_step` (`phrase_lookup.cpp:382-397`). It does not return a
   boolean, finalize the phrase-result array, or reach the caller's
   `g_free`. Exact process-specific assertion formatting is not an
   executed observation here.
6. Rust's successful GLib conversion becomes `Unmatched(2)`. Each
   facade replaces its prior result with `[0, 0]` and returns false,
   without consulting either gram or calling `refuse_lookup_gram`.
   It emits no warning; subsequent getters report two zero tokens.

## Observable channels and backward trace

The pin's abort is the return/liveness difference and has an assertion
diagnostic on stderr. Rust returns normally and silently. No persistent
gram, unigram or dictionary write precedes the pin assertion: the only
mutations are lookup scratch steps, backend read state and the merged
scratch chunk. The old public phrase-result array has not yet been
replaced at the assertion, but it cannot be read after process death;
this is not a claim of an observable surviving pin result. Rust replaces
the old vector. A subsequent getter is therefore a subject observation,
not a pin/subject post-abort equality claim.

The two facades each have their own private `segment_text`, with only
their phrase-segment export as a production caller. All non-scalars
accepted by this converter take the same early return. The ordinary
scalar path instead reaches `engine/src/phrase.rs::step_costs`, where
the merged-row fault becomes `ShortLookupPredecessor`; both facades'
error arms call `sentence.rs::refuse_lookup_gram` and preserve the old
result. The warning is produced there, not by the converter, store
loader, phrase-result getters, or reset. No alternate producer supplies
the missing warning on the non-scalar shortcut.

The pin result writers/getters and neighboring string helpers are
enumerated in the [source audit](918-phrase-segment-source-audit.md).
Those helpers do not call this private conversion helper. This record
does not admit changes to sentence-prefix parsing, dictionary decoding,
real GLib allocations, the C ABI, or on-disk formats.

## Partition and classification

For sound backing data, the prefix consultation is unobservable and the
shortcut is identical in the channels proved by the source audit. The
trigger here combines a GLib-accepted non-scalar with an actually reached
short user value beside a system gram. It is **class (c)** because the
pin reaches a defined assertion, not class (a), a memory over-read, or a
language/runtime quirk. The user value's length is checked before the
copy, so no undefined read is needed to reach this assertion.

The early return's missing warning is an existing defect under row 85;
it cannot be registered as a policy-accepted quirk by analogy with #894.
The requested ordinary-path warning is already implemented: both
`phrase.rs::*_phrase_segment` error arms call
`sentence.rs::refuse_lookup_gram`, whose `ShortLookupPredecessor` arm
emits the existing facade-domain warning and returns false without
replacing the old result. Row 85's 2026-10-15 lookup amendment already
names both phrase-segment entry points. The scalar pinyin contract case
already exists; its zhuyin twin is not registered.

**Implementation STOP:** the requested distinction between a missing
ordinary-path warning and an observably identical malformed-prefix
shortcut does not hold for the trigger above. The only missing row-85
warning identified here is on that shortcut. Its sound-data partition
is identical; this reached short-gram partition is not. Retaining the
shortcut without consulting the prefix cannot detect this fault. No
runtime change or waiver is made; the exact overlapping input/store
scope needs clarification before implementing the requested fix.

Controls for eventual verification must include: both facades; the same
input with sound grams; a short gram under an unreached predecessor;
the non-scalar before any reachable scalar prefix; an ordinary scalar
prefix reaching the short gram; and old-result/token/warning checks.
Other malformed gram shapes and backend read failures have separate
proof obligations and are not covered by this classification.

## Verification and reproduction

Source reading only. No new runtime test, build, measurement or pin
probe. Existing `contract-diff.py` case
`abort-phrase-segment-short-user-gram` covers scalar pinyin input; it
does not cover this non-scalar combination or its zhuyin twin. The
existing surrogate unit tests use ordinary backing data and also do
not cover it. A future case belongs in that driver under the existing
abort/class-(c) mechanism, not the normally-terminating
`POLICY-ACCEPTED` mechanism.

From a libpinyin checkout, reproduce the decisive source reading with:

```sh
git show 074a2219:src/lookup/phrase_lookup.cpp
git show 074a2219:src/storage/ngram.cpp
git show 074a2219:src/include/memory_chunk.h
git show 074a2219:src/include/pinyin_utils.h
```

Conclusion: **defect recorded; early return retained; no removal cleared**.
The [integrated G1 update](glib-libc-verification.md#g1--utf-8ucs-4-conversion-and-category)
records these exact limits. No raw captures or disassembly are published.
