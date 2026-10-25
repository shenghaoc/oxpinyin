# Prediction with a suggestion token whose item is missing (#1231)

Recorded 2026-10-25 UTC (machine timestamp: 2026-10-25T12:28:10Z).
Part 1 only: source derivation, not a behavioral reproduction or a fix.
Updated 2026-10-25 UTC (machine timestamp: 2026-10-25T12:46:38Z).
**Maintainer ruling, 2026-10-25: option 1, reproduce the sole-candidate
result and its choose-side effects.** Part 2 still waits for #1190's merge
and successful BDB provisioning; this update changes only the findings.
Updated 2026-10-25 UTC (machine timestamp: 2026-10-25T14:53:03Z):
Part 2 implemented, held by the cases below, and gated; see "Part 2
implementation and verification".

## Result and classification

**Source-derived.** With a nonempty resolved prefix, sound surrounding data,
and a missing-item token actually returned by the suggestion walk:

| Outcome at the pin | Classification and outward target | Main today |
| --- | --- | --- |
| Last successfully read suggestion length exceeds `2 * prefix_length + 1`: token is skipped. | **Reproduce.** No candidate for this token; ordinary `true` return if the remaining pipeline completes. No exception is needed. | Differs when fallback text is short enough to pass Rust's independent length gate; already matches this token's absence when that gate also drops it. |
| Token is appended and the complete pre-dedup candidate list has at least two rows, including bigram rows and duplicates. | **Class (b)** NULL string dereference in dedup. Row 106 supplies the crash precedent: missing token contributes no candidate, normal `true`, no warning. This is a source-derived prediction classification, not a newly measured SIGSEGV. | Can retain and render fallback text; that is not row 106's safe answer. |
| Token is appended and is the only row. | **Reproduce**, per the 2026-10-25 ruling. Guess returns `true`; count is 1; index 0 gives a candidate; its type is predicted prefix; string getter returns `true` and writes NULL. Preserve choose's state effects below. Returning NULL is not itself a dereference. | A retained row has an allocated string (possibly empty), not NULL; dropping it also changes count/getter behavior. |
| Library is unloaded or was never enabled, so no token array exists. | **Already matching** for this exclusion; no missing-item candidate reaches the prefix walk. | `phrase_table_token_enabled` filters system and user rows. |

**Source-derived citations for the table:** pin `src/pinyin.cpp:2378–2405`,
`:2420–2453`, `:2018–2022`, `:2041–2083`, `:2845–2877`;
`src/storage/phrase_index.h:762–773`;
`src/storage/phrase_large_table3.cpp:29–49`. Main items are
`merged_suggestions` in `crates/oxpinyin-facade/src/predict.rs`,
`append_predicted_prefix` / `guess_predicted_result` in
`crates/oxpinyin-capi/src/predict.rs`, and `RuntimeDict::phrase_table_token_enabled`
in `crates/oxpinyin-runtime/src/lib.rs`. Policy basis:
[compatibility policy](compatibility-policy.md), classes (b)/(c), decision
order, and rows 102/106. The rows classify crashes, not every missing-item API
result. None of these source-derived comparisons is a runtime `MATCH` claim.

## Scope and source identity

**Source-derived evidence basis.** Read a clone at 074a2219,
detached at the full commit in `tools/oracle/oracle-pin.txt`:
`074a2219c90feaf962d0d24f034514033ece5f99`, tree
`3047d8f0ac3efb8363211d43660797ac6b914162`. Every tracked working-tree blob
was hashed against that commit, including every pin file cited here; the
checkout was clean. Pin citations below mean `path:line` in that tree, not
upstream's current default branch. The tree identity names the complete
checkout; it does not claim every source file was semantically audited.

**Source-derived evidence basis.** The oxpinyin main snapshot is
`27c785c07d30ae4332647d76b106c1bafaf71958`. Read #1231, `AGENTS.md`, steering,
cloud/native setup runbooks, policy rows 102/106 and the
`removed-candidate-*` / `removed-extension-*` registrations and shared
`_removed_extension` builder in `tools/bisection/contract-diff.py`.
The comparison is pinyin prefix prediction, with Berkeley DB as reference.
It does not generalize to unrelated malformed buffers, invalid caller
pointers, concurrent mutation, zero-total float conversions, or missing
bigram-successor items. The latter has its own two-pass cache and only
explicitly skips `ERROR_NO_SUB_PHRASE_INDEX` (`src/pinyin.cpp:2344–2365`).

**Source-derived dependency check.** GLib's singleton-sort and NULL-free
behavior was read in the 2.89.4 sources (the version family named by
`glib-libc-verification.md`):
[`glib/garray.c:971–985`](https://github.com/GNOME/glib/blob/2.89.4/glib/garray.c#L971),
[`glib/gqsort.c:70–88`](https://github.com/GNOME/glib/blob/2.89.4/glib/gqsort.c#L70)
(and the dispatch at `:343–349`), and
[`glib/gmem.c:195–210`](https://github.com/GNOME/glib/blob/2.89.4/glib/gmem.c#L195).
No pin behavioral case, test, or exploratory driver was executed.

## Three independent stale reads, not one inherited item

**Source-derived.** `FacadePhraseIndex::get_phrase_item` returns
`ERROR_NO_SUB_PHRASE_INDEX` before touching the destination when the library
is absent (`src/storage/phrase_index.h:646–651`). The sub-index returns
`ERROR_OUT_OF_RANGE` for an unavailable offset, `ERROR_NO_ITEM` for offset
zero, or `ERROR_FILE_CORRUPTION` for an unreadable length/pronunciation-count
byte; only success replaces `item.m_chunk` (`src/storage/phrase_index.cpp:180–205`).
`PhraseItem` starts with a zeroed header: length and unigram frequency are
zero (`src/storage/phrase_index.h:81–84`, `:100–103`, `:124–126`). A failed
read therefore retains a previous successful item or that default header.
This is not uninitialized memory. A successful read is not complete body
validation; truncated bodies after the checked bytes are outside this finding.

**Source-derived — filter cache.** `_compute_predicted_prefix_candidates`
constructs `item` once, after collecting/reducing suggestions. Every token
tries to replace it before the length comparison, even a successful token
that the comparison itself rejects (`src/pinyin.cpp:2389–2396`). Thus a
rejected long live token can make following missing tokens get rejected too.
A missing first token sees length 0 and passes the upper bound. Repeated
missing tokens leave the last successful length unchanged. There is no lower
bound requiring a suggestion item's length to exceed the prefix length.

**Source-derived — length cache.** `_compute_phrase_length` creates a new,
independent `PhraseItem` (`src/pinyin.cpp:1953–1979`). Its input is the
appended bigram rows followed by the surviving prefix rows (`:2430–2436`).
Consequently the previous success can be a bigram candidate, but cannot be a
suggestion discarded by the filter. It must not reuse the filter's last-read
length. For prefix rows it stores `item.get_phrase_length() - m_begin`.

**Source-derived — exact widths, correcting #1231's “guint” description.**
`get_phrase_length()` returns `guint8`; candidate `m_begin` is `guint16` and
`m_phrase_length` is **guint8**, not guint (`src/storage/phrase_index.h:100–103`;
`src/pinyin.cpp:97–115`). `m_prefix_len` is `glong` (`:78`), set by conversion
at `:1396–1401` and narrowed into `m_begin` at `:2401`. On the reference ABI,
both subtraction operands promote to signed `int`; negative subtraction is
in range, then assignment to `guint8` reduces modulo 256. Example: last
length 0, begin 1 yields stored length 255. It is not a 32-bit unsigned
subtraction, nor undefined signed overflow. The sort reads that byte
(`:1684–1689`). Large prefix lengths also narrow modulo 65536 at `m_begin`;
this record does not claim that other large-input arithmetic is safe.

**Source-derived — frequency cache.** `_compute_frequency_of_items` creates
another new `cached_item` before its loop (`src/pinyin.cpp:1795–1825`). A
prefix row reads that item and computes `(1-lambda) * unigram / total * 2^24`,
using the current facade total. General rows, including preceding predicted
bigram candidates, also update **the same** cache (`:1855–1866`). “Previous
prefix candidate” is too narrow: it is the previous successful read in this
candidate pass, or zero by default. With a positive total and ordinary
lambda, the sole missing prefix row gets zero frequency. Zero total and
nonfinite/out-of-range float conversion are separate cases; token order
alone does not settle them.

**Source-derived — determinism.** For fixed stored tables, live item values,
configuration and ordinary single-threaded invocation, these last-success
reads are determined by traversal order; no allocator accident is needed.
The facade visits system then user tables
(`src/storage/facade_phrase_table3.h:141–153`); BDB requires the exact prefix
key, then follows `DB_NEXT` while keys extend it
(`src/storage/phrase_large_table3_bdb.cpp:210–265`, extension check `:44–62`).
Entry order is raw stored token order (`src/storage/phrase_large_table3.cpp:32–48`),
and `reduce_tokens(..., false)` concatenates library arrays in index order
without sorting tokens (`src/storage/phrase_large_table3.h:77–97`;
`src/pinyin.cpp:2386`). Numeric token order alone is not the ordering law.
Lengths/frequencies and preceding bigram candidates are also inputs. Once a
NULL dereference is reached there is no defined C++ final output to call
“deterministic”; source proves the unsafe access, not a universal signal.

**Source-derived implementation consequence, not implementation.** The
stable filter result needs only a plain `last_read_length = 0` variable,
updated on **every** successful live lookup **before** testing the upper
bound, including successes that get skipped. The later length/frequency
caches need reproduction only where observable outside class (b); the
bounded path below exposes neither cached value in that region.
Use safe values and explicit narrowing, not aliased `PhraseItem` buffers or
C++ object-state emulation. This is O(n) work and O(1) extra state, like the
pin's passes; no mechanism imitation or quirk exception is necessary for
these defined effects. Merely dropping missing rows early must not erase
the successful-but-skipped predecessor that controls subsequent filtering.

**Source-derived — where the later caches are observable.** In this finding's
bounded missing-prefix-item path, there is **no observable stale post-filter
length or frequency outside the NULL-comparison class-(b) path**:

- If the filter skips the token, it never enters either later pass
  (`src/pinyin.cpp:2393–2405`, `:2436–2438`). Other successful item reads
  replace their destination before use (`src/storage/phrase_index.cpp:204`).
- If the missing token is the sole appended row, both later passes start
  with default zero headers. The length is the narrowed negative begin
  described above and frequency is zero under the stated positive-total
  assumptions. Neither singleton sort compares it, and dedup immediately
  continues past its first row (`src/pinyin.cpp:1959–1979`, `:1801–1825`,
  `:2443–2451`, `:2076–2080`; GLib singleton base case cited above).
- Punctuation is prepended only after this pipeline; there is no new sort
  or dedup using the stale fields (`src/pinyin.cpp:2459`, `:2488–2498`). The
  sole NULL row and its position after punctuation remain observable, but
  its cached length/score do not determine them.
- The candidate handle is opaque (`src/pinyin.h:37`). The exported count,
  candidate, type, string and n-best getters expose no cached length/score
  (`src/pinyin.cpp:2845–2885`). Predicted choose reads type and token and
  trains by the fixed 483 increment, not by either cached field
  (`:2591–2616`); `is_user_candidate` checks type (`:3712–3716`). Cleanup
  reads the string, not the caches (`:1335–1344`). Later guesses recompute
  local caches after freeing the old list (`:2420`, `:1959`, `:1801`).
- With two or more pre-dedup rows, stale length/frequency can affect the
  initial sort (`:1684–1705`, `:2443–2445`), but the surviving NULL row then
  reaches `strcmp` (`:2041–2068`). That path is class (b), not a requirement
  to emulate intermediate scores before supplying row 106's safe answer.

**Source-derived implementation consequence under the ruling.** Preserve
the filter's last-read length and all sole-row outputs/state effects; no
second cache or synthetic score is needed merely to preserve inaccessible
fields in this bounded case. The choose-side facade-total mutation is
observable independently and remains mandatory. This conclusion admits no
other difference and does not classify other malformed/zero-total or
bigram-successor paths excluded by this record's scope.

## NULL string and the complete sole-candidate result

**Source-derived.** New candidates start with `m_phrase_string = NULL`
(`src/pinyin.cpp:107–115`). `_token_get_phrase` returns `false` on the failed
item lookup before assigning either optional output (`:1627–1645`). The
prefix string pass ignores that return (`:2018–2022`). No key-text fallback
exists there. Length and frequency sorting happen first; string computation
and string dedup then run (`:2436–2451`).

**Source-derived.** With at least two candidates, the index sort compares
candidate strings through `strcmp` (`src/pinyin.cpp:2041–2068`). GLib's merge
sort recursively compares elements for arrays larger than one; the NULL row
must participate. The later equality loop also has a `strcmp` at `:2082`.
This is the row-106 unsafe site. Source supports a predicted NULL-dereference
case; row 106 records SIGSEGV for the ordinary candidate path. No predicted
SIGSEGV has been measured here and the C++ standard does not promise SIGSEGV.

**Source-derived.** With exactly one row, GLib's `n <= 1` base case calls no
comparator. Dedup saves the first row and continues (`src/pinyin.cpp:2071–2080`),
so it reaches neither `strcmp`. The candidate is not made a zombie; cleanup
only removes zombies (`:2147–2160`). Guess returns `true` (`:2453`). For valid
output addresses and the live candidate handle:

| Call | Exact source-derived result | Pin citation |
| --- | --- | --- |
| `pinyin_get_n_candidate` | `true`, writes 1 | `src/pinyin.cpp:2845–2848` |
| `pinyin_get_candidate(instance, 0, &candidate)` | `true`, writes pointer to the array row | `src/pinyin.cpp:2851–2863` |
| Same getter at index 1 or beyond | `false`, writes NULL | `src/pinyin.cpp:2856–2859` |
| `pinyin_get_candidate_type` | `true`, writes `PREDICTED_PREFIX_CANDIDATE` | `src/pinyin.cpp:2399`, `:2866–2870` |
| `pinyin_get_candidate_string` | **`true`, writes NULL**, including overwriting a non-NULL sentinel; no allocation, conversion, dereference, or empty string | `src/pinyin.cpp:2873–2877` |
| `pinyin_token_get_phrase` on this token | `false`, leaves both supplied outputs unchanged | `src/pinyin.cpp:2776–2783`, `:1635–1637` |
| `pinyin_is_user_candidate` | `false`, regardless of token library | `src/pinyin.cpp:3712–3716` |

**Source-derived — later consumers.** `pinyin_choose_predicted_candidate`
accepts the prefix type and never reads its string (`src/pinyin.cpp:2591–2616`).
It calls `add_unigram_frequency(token, 23 * 3 * 7)` and only checks
`ERROR_INTEGER_OVERFLOW`. For a still-loaded library with a still-missing
item, the facade **first increments its total by 483 modulo 2^32**
(`src/storage/phrase_index.h:628–634`); the sub-index then fails before
updating its item or sub-index total (`src/storage/phrase_index.cpp:150–167`).
The prefix branch nevertheless returns `true`, and performs no bigram write.
Repeated choices repeat that facade-total increment. If the library was
unloaded after the candidate was obtained, the facade returns the no-subindex
error before incrementing, but choose still returns `true`. These defined
side effects are explicitly required by the 2026-10-25 ruling; they must not
be hidden behind the NULL string or replaced with an assumed no-op.

**Source-derived — other consumers and lifetime.** The punctuation wrapper
first completes the plain guess, then prepends punctuation rows without
rerunning dedup (`src/pinyin.cpp:2456–2498`); therefore a surviving sole NULL
row can coexist with punctuation afterwards without this `strcmp` crash.
A later guess/reset/free cleans the string with `g_free(NULL)`, which is safe
(`src/pinyin.cpp:1335–1353`, `:2693–2704`; GLib `gmem.c:195–210`); it does not turn the getter
into an error. Using the ordinary choose API or n-best-index getter with the
prefix type hits their type assertions (`src/pinyin.cpp:2507–2508`,
`:2880–2885`), not a string dereference. An external caller that then treats
NULL as a C string is a separate consumer behavior: the exported getter
itself returned successfully. The pin's sample prints an unchecked getter
result (`tests/test_pinyin.cpp:77–86`), but that sample uses ordinary guessing
(`:65–67`); it is not evidence that this prediction sequence was run or that
all consumers crash.

## Reachability and exclusions

**Source-derived — row 106 journal REMOVE.** A valid REMOVE removes the
sub-index item and zeros its offset, without editing phrase-table rows
(`src/storage/phrase_index.cpp:219–234`, `:460–475`). Library loading merges
the journal and ignores its result (`src/pinyin.cpp:264–277`). An extending
phrase-table key can still return that token. A live prefix/suffix token is
needed to pass the initial prefix gate (`:1389–1423`, `:2423–2427`); the
removed extending token need not be that prefix token. Existing row-106
builders construct this state, but their exact/longer/prefix **reading**
cases are not prediction-string cases. They do not establish sole-prediction
cardinality or stale-filter order.

**Source-derived — row 102 reload.** Import inserts both phrase/pinyin table
entries and the new item (`src/pinyin.cpp:588–605`). Unload deletes the
sub-index and sets it NULL (`src/storage/phrase_index.cpp:261–268`), while
reload recreates it from the system chunk and saved journal
(`src/pinyin.cpp:264–277`). An unsaved imported item can disappear while its
user phrase-table entry remains. Once the library is loaded again, its array
is enabled and suggestion search can reach the stale token. The missing
item can fail by out-of-range or zero offset, depending on the rebuilt index
(`src/storage/phrase_index.cpp:180–193`).

**Source-derived — unloaded is not reloaded.** `prepare_tokens` allocates
arrays only for non-NULL sub-indices (`src/storage/phrase_index.h:762–773`).
`PhraseTableEntry::search` skips a token whose destination array is NULL
(`src/storage/phrase_large_table3.cpp:41–48`). It still walks the DB records
and raw entry tokens: exclusion is **before appending to the candidate token
walk**, not before the DB cursor/entry walk. A merely unloaded/never-loaded
library therefore cannot supply this prefix-suggestion token in a normal
single-threaded call. Public pinyin unload is restricted to GBK library 2
(`src/pinyin.cpp:464–474`). No concurrency hypothesis is needed for reload.

**Source-derived — other permitted file states.** BDB attach/load accepts
DB records without consulting the phrase index
(`src/storage/phrase_large_table3_bdb.cpp:92–147`), and suggestion search
only forwards the stored token values (`:249–253`). A same-backend system or
user phrase table can therefore name a nonexistent slot in an enabled
library, an offset-zero hole, or an item whose first two header bytes cannot
be read. Those are the `ERROR_OUT_OF_RANGE`, `ERROR_NO_ITEM`, and
`ERROR_FILE_CORRUPTION` paths at `src/storage/phrase_index.cpp:180–201`.
These are source-permitted inconsistent files, not a claim that normal
import produces each one. The entry must contain complete token words and
valid library bits, the prefix key must exist, and the context must survive
loading. An earlier partial-tail/loader failure is not this scenario.
Ordinary user-candidate removal also removes the phrase-table entry
(`src/pinyin.cpp:3740–3750`), so successful normal removal alone is not the
journal-REMOVE witness.

## Current Rust, fallback provenance, and #1190

**Source-derived — main, same-token pipeline.** In
`crates/oxpinyin-facade/src/predict.rs::merged_suggestions`, system and user
rows are library-gated, merged, then resolved by
`RuntimeDict::token_introspection(token).map(|item| item.text).or(fallback)`.
Item-level removal makes introspection return `None`, whereas the
library-level gate still permits the token (`RuntimeDict::library_visible_token`,
`phrase_table_token_enabled`, `token_introspection` in
`crates/oxpinyin-runtime/src/lib.rs`). A decodable fallback keeps the same
token; no fallback drops it. This is not an unconditional keep on every
missing-item state.

**Source-derived — system and user fallback differ.** Main
`SystemDictionary::suggest_after` / `resolve_suggestions` in
`crates/oxpinyin-data/src/dict.rs` first resolves system tokens through the
immutable phrase libraries and drops tokens with no base text. For a journal
REMOVE of an otherwise readable system item, the immutable base still
supplies text. `merge_suggestion_rows` in `crates/oxpinyin-runtime/src/lib.rs`
re-encodes that system text as a UCS-4 key and decodes it; user rows supply
their stored raw key bytes. Thus “key text” is literally the user-table key
fallback, but the system fallback originates in the base item's text, not
necessarily the actual cursor key. A system-table-only dangling token with
no immutable item can already disappear before this seam. That token's
absence may match a stale skip or the crash-safe answer, but not the pin's
sole-NULL count. Duplicate occurrences of one token are retained by this
merge and can affect pre-dedup cardinality.

**Source-derived — C adapter.** Main
`append_predicted_prefix` in `crates/oxpinyin-capi/src/predict.rs` checks the
fallback/live text's character count against `2 * prefix_len + 1`, slices
that text after the prefix, and appends the **same token** with predicted
prefix type. It neither tests a previous item's length nor carries its
frequency. `predicted_frequency` reads base/overlay counts for that token;
`guess_predicted_result` sorts, deduplicates strings and interns survivor
bytes. Therefore `merged_suggestions` retaining a token does not guarantee
an exported row: the adapter's length gate or later text dedup can remove
it. Even an empty sliced string is distinct from the pin's NULL pointer.

**Source-derived — origin and justification search.** The literal `.or(fallback)`
and its “Preserve the established valid-key fallback …” comment entered in
`e3016e3867bb4f5b5487645f53102afdf8f65be0`, “refactor(user): port pin
PhraseLargeTable3 records”, referring to #759/#785. Its parent returned
merged text directly; the system branch used `library_visible_token`, and
user suggestions came from the user phrase inventory. The change switched
to raw user-table rows and `phrase_table_token_enabled`, then preferred live
item text while preserving a fallback. The commit message, this source
comment, and its policy/register changes were read: the registered exception
in that commit is row 101's partial token tail, not a missing-item prediction
policy. No pin counterpart or maintainer authorization for this fallback
was found in those records or current policy/register. This is a bounded
provenance finding, not a claim to have searched every historical conversation.

**Source-derived — precise pending snapshot.** #1190 was open/unmerged when
read. Its fetched head and remote branch `fix/1182-limb3-test` were
`a3017fc7ba162ac25a5c9be2d018d45e2bddc636`, based on the main snapshot above.
At that head, `merged_suggestions` returns `PhraseText` and still has
`.or(fallback)`; `append_predicted_prefix` uses `phrase_length()` and
`skip_chars()`, with a held display length. The fetched
`merge_suggestion_rows` still converts system text through `phrase_walk_key`
and `decode_ucs4_key(...).map(PhraseText::from)`.

**Source-derived — subsequent #1190 review fix.** After that read, the review
fix landed on #1190 at `3c28f29aafe0d9170292f18894d652adddc57719`, backed on
`wip/1190`, and was read for this update. `merge_suggestion_rows` in
`crates/oxpinyin-runtime/src/lib.rs` now carries `Some(text.clone())` for each
system row and decodes only user keys, preserving the system row's original
`PhraseText`, including stored non-scalar words and the zero suffix. This
changes fallback representation, not the missing-item fallback policy.
The commit is **not merged main** at the main snapshot above. No #1190 file
or branch was edited.
Main's limb 3 still has the “normal Rust” formulation; #1190 carries the
natural, efficient Rust test. This Part 1 follows the user's explicit
natural-Rust direction without importing the pending implementation.

## Maintainer ruling and remaining prerequisites

**Maintainer decision, 2026-10-25: option 1.** Reproduce the source-derived
sole-candidate result recorded above. `pinyin_get_n_candidate` gives 1; the
type is `PREDICTED_PREFIX_CANDIDATE`; `pinyin_get_candidate_string` returns
`true` and writes NULL; `pinyin_token_get_phrase` returns `false` with its
outputs unchanged; `pinyin_is_user_candidate` returns `false`;
`pinyin_choose_predicted_candidate` returns `true` and adds 483 to the facade
total, wrapping, while the library stays loaded, and leaves that total
unchanged if the library has been unloaded since. The exact getter and
mutation source citations are in the sole-candidate section above.

**Maintainer decision, 2026-10-25:** reproduce the stale filter with a plain
`last_read_length` updated on every successful read before the upper-bound
check, including a success that is then skipped. Class (b) covers only paths
that reach NULL `strcmp`: two or more pre-dedup rows receive row 106's answer
that the missing token adds no candidate. It does not cover a sole NULL row
or the later addition of punctuation without a new comparison. Reproduce
stale length/frequency caches only where observable outside class (b); the
source-derived observability analysis above identifies none in the bounded
missing-prefix-item case. **No other difference is admitted.**

**Source-derived policy analysis under that ruling.** The sole return, NULL
output and facade-total mutation are expressible safely with a nullable
string in the C adapter and an ordinary wrapping total update. Class (c)
does not turn a successful getter/choose into an abort. The ruling therefore
preserves these outputs rather than extending the crash exception to them.

**Part 2 remains held:** #1190 must merge, and a working BDB oracle must be
provisioned successfully for the runtime gate. The cloud proxy-CA failure
below remains a provisioning limitation. Part 2 may run where provisioning
succeeds; otherwise report the failure and wait. No implementation, new
policy/divergence row, contract cases, PR, review or merge occurs in Part 1.
Part 2 can then implement the decided natural Rust behavior, remove dead
fallback as appropriate, register the crash exception and add the planned
`removed-predict-*` cases using row-106 builders, followed by the prescribed
mutation and runtime gates. Those are future work, not results of this record.

**Part 2 superseded this hold (2026-10-25 UTC).** #1190 merged at
`3c28f29a`, and the Ubuntu host's provisioned BDB oracle opened the system
tables and passed the differential. The implementation, records and cases
are recorded in the next section; the earlier hold and the cloud
proxy-CA provisioning failure below remain as the Part 1 record.

## Part 2 implementation and verification

**Implemented (2026-10-25 UTC).** `merged_suggestions` no longer synthesizes
phrase-table key text for an absent item: `SystemDictionary::suggestion_tokens`
returns the raw walk, and `merged_suggestions` maps each token to
`dict.token_introspection(token).map(|item| item.text)`, so an absent item is
`None` at the adapter. The `.or(fallback)` path and the now-unused
`SystemDictionary::suggest_after`, `resolve_suggestions` and
`phrase_walk_key` are removed; `merge_suggestion_rows` keeps only tokens, so
#1190's system-text carry there is gone too.

`crates/oxpinyin-capi/src/predict.rs::select_prefix_rows` implements the
stale gate as one plain `last_read_length`: it starts at zero, every
successful read replaces it before the `2 * prefix_len + 1` check (including
a read the check then skips), and a failed read leaves it alone. The same
helper applies the cardinality rule: a sole missing row survives; with any
other pre-dedup row (a live prefix candidate or a bigram row already
collected) the missing token adds no candidate, the call returns normally
with `true`, and no warning is emitted. The sole row is pushed as a snapshot
row with `TextSpan::NULL`, so `pinyin_get_n_candidate` is 1,
`pinyin_get_candidate_type` is `PREDICTED_PREFIX_CANDIDATE`,
`pinyin_get_candidate_string` returns true and writes NULL,
`pinyin_token_get_phrase` returns false with both outputs untouched, and
`pinyin_is_user_candidate` returns false for the predicted row.

**Separate pre-existing defect: the `pinyin_is_user_candidate` type gate
(2026-10-25 UTC).** The pin rejects every candidate type other than
`NORMAL_CANDIDATE` and `LONGER_CANDIDATE` before it reads the token library
(`src/pinyin.cpp:3713-3715`). Before this change oxpinyin tested only the
token nibble, so a predicted-bigram or predicted-prefix row whose token is
in `USER_DICTIONARY` (7) answered true. The gate is now applied in
`crates/oxpinyin-capi/src/candidates.rs`, and
`predicted-bigram-is-user-candidate` imports a user phrase, plants it as a
predicted-bigram successor through the existing `predicted_bigram_row_context`
builder, and holds the pin's false. The defect predates #1231 and is
independent of the missing-item path.

`pinyin_choose_predicted_candidate` takes that row, advances the loaded
facade total by 483 (wrapping) through `RuntimeDict::add_unigram_delta`, and
returns true; an unloaded library leaves the total unchanged and still
returns true. The later length/frequency caches are not emulated: the
observability analysis above identifies no observable stale post-filter
value outside the class-(b) path, which now drops the missing row.

**Held by.** `removed-predict-sole-pinyin` (count, type, NULL string and
`token_get_phrase`), `removed-predict-choose-pinyin` (choose true and the
later frequency read; the C ABI has no facade-total getter),
`removed-predict-class-b-pinyin` (two pre-dedup rows; pin SIGSEGV, subject
normal `true` with the live row), and `removed-predict-stale-skip-pinyin`
(the long live row's stale length skips the missing row), and
`predicted-bigram-is-user-candidate` (the separate type gate), in
`contract-diff.py`; bdb is the reference cell and the stale case is
bdb-only by registration, while the other cases also MATCH on kc and
tkrzw. The 483-total and unloaded-unchanged effects are
also asserted directly by the in-crate test
`choosing_a_sole_missing_prefix_row_advances_the_facade_total`. The
implementation's filter also has unit tests for the sole, class-(b),
stale-skip and later-short-read cases in `predict.rs`. Policy row 106a and
the named `upstream-divergences.md` entry record the classification.

**Gate (2026-10-25 UTC, Ubuntu host).** `cargo test --locked` for
`oxpinyin-capi`, `oxpinyin-facade`, `oxpinyin-runtime` and
`oxpinyin-data` passed; `cargo clippy --all-targets -- -D warnings` for the
same crates passed; `just fmt --all --check` passed. The seven
`tools/bisection/*.test.py` harnesses (all but the CI-excluded
`internal-ledger.test.py`) passed with an ext4 `TMPDIR`. The four new cases and
the existing `removed-candidate-*` / `removed-extension-*` cases MATCH on
bdb, kc and tkrzw (the stale case is skipped on the latter two by
registration); the four removed-prediction cases and
`predicted-bigram-is-user-candidate` DIFFER against the parent build
`5928b356` with `--expect-parent`. The function-order policies were
regenerated for the three backends.

## Reproduction and verification

Source inspection can be repeated without loading the pin library:

```sh
# In an oxpinyin clone, retain the exact snapshots named above.
git show 27c785c0:tools/oracle/oracle-pin.txt
git show e3016e38 -- crates/oxpinyin-facade/src/predict.rs
git show a3017fc7:crates/oxpinyin-runtime/src/lib.rs
git show 3c28f29a -- crates/oxpinyin-runtime/src/lib.rs
# In a separate libpinyin clone:
git checkout --detach 074a2219c90feaf962d0d24f034514033ece5f99
git rev-parse HEAD 'HEAD^{tree}'
git ls-files -z | while IFS= read -r -d '' file; do
  test "$(git hash-object "$file")" = "$(git rev-parse "HEAD:$file")" || exit 1
done
nl -ba src/pinyin.cpp | sed -n '1795,1867p;1953,2160p;2373,2640p;2845,2885p'
nl -ba src/storage/phrase_index.cpp | sed -n '150,205p;219,268p;460,475p'
nl -ba src/storage/phrase_index.h | sed -n '628,651p;762,773p'
# In the documentation worktree:
git add docs/findings/1231-predict-missing-item.md
git diff --cached --check
git diff --check
```

Provisioning was requested independently of the docs-only gate. The command
was `tools/cloud/setup.sh --cells bdb --cache-dir '<task-owned-cache-directory>'`,
using a task-owned cache directory,
with task-owned `UV_INSTALL_DIR`, `UV_PYTHON_INSTALL_DIR`, `UV_CACHE_DIR` and
`OXPINYIN_ORACLE_ENV`. Setup used plain `uv python install cpython@3.15`
and verified selector `cpython@3.15+gil`; it did not request a `+gil` archive.
CPython 3.15.0 and the BDB library hashes verified. Full setup **failed** in
BDB data regeneration: the container Git fetch of
`https://github.com/libpinyin/libpinyin.git/` failed SSL certificate
verification (“certificate signer not trusted”, using the container's
system CA bundle), then setup reported
`FAIL: bdb: --regen-data failed` and exited 1. No trust bypass or workaround
was installed. The separate source clone succeeded and was verified as above.
No behavioral runs followed; full BDB provisioning is not claimed complete.

The applicable gate is `AGENTS.md` verification rule 7: review the text and
links, then `git diff --check`. Markdown-only changes need no Rust formatter,
build, test, clippy, backend-selection or behavioral gate. No logs or dumps
are included in this deliverable.
