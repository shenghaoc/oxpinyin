# #918 — phrase-segment input boundary, source investigation

Status: **bounded source audit; no removal cleared** · 2026-10-24 UTC ·
[#918](https://github.com/shenghaoc/oxpinyin/issues/918), parent
[#915](https://github.com/shenghaoc/oxpinyin/issues/915).
Source baseline: oxpinyin `1c5d96b9605a3e5f1caef3a79fbb653707fd031f`;
rebased onto `ae516625` with no intervening runtime changes in the cited paths.
This is an investigation checkpoint, not a discrepancy admission or a
claim that the child is complete. No production behavior changes. The
non-scalar early return is retained. The prefix audit found a
[missing class-(c) warning](918-prefix-short-gram-discrepancy.md).
The [follow-through](918-boundary-follow-through.md) resolves the
previously unread caller boundaries and returning store-error paths,
and consolidates the precise remaining limits.

The shared coverage ledger landed via [#1156](https://github.com/shenghaoc/oxpinyin/pull/1156).
The integrated G1 entries are updated here; no code may be removed before
every affected family and shared-helper caller is completed.
The [policy decision order](compatibility-policy.md#the-languageruntime-quirk-exception--outside-the-classes)
still governs; unread paths are unverified, not requests for a ruling.

## Sources and method

Pin source was read at `074a2219c90feaf962d0d24f034514033ece5f99`.
Pin paths below start at `src/`. GLib source is Debian **2.89.4-2** from
the inventory's `20260831T000000Z` snapshot, not the host's GLib or the
2.90.0 named in the existing `segment_text` comment. The orig and Debian
source tarballs were checked against the SHA-256 fields in
`glib2.0_2.89.4-2.dsc`; its patch series changes none of the cited UTF-8
or diagnostic code. glibc **2.43-3** remains the reference environment;
no libc behavior is reclassified in this checkpoint.

Relevant GLib source:

- `glib/gutf8.c:117-128,237-273`: byte-lead skip table and
  `g_utf8_strlen`; `glib/gunicode.h:951`: `g_utf8_next_char` macro.
- `glib/gutf8.c:665-747`: `g_utf8_get_char_extended`; it rejects
  malformed/overlong/incomplete encodings, but has no Unicode scalar
  restriction and accepts the historical five/six-byte encodings.
- `glib/gutf8.c:940-997`: `g_utf8_to_ucs4`; `items_written` is assigned
  only after successful allocation/conversion. Error leaves the caller's
  initial zero unchanged. The pin passes NULL `items_read` and error.
- `glib/gmessages.h:663-675`, `glib/gmessages.c:3193-3202`: failed
  precondition invokes `g_return_if_fail_warning`, logging a critical.
  `glib/gmacros.h:931-939` supplies the compiler-dependent `G_STRFUNC`.

No oracle execution, exploratory test, build or measurement has been
performed. The conversion/over-read questions were checked with
`objdump -dlS -C --no-show-raw-insn` and `readelf` on the hash-matching
bdb pin libraries from `tools/oracle/oracle-pin.txt` and the snapshot's
GLib 2.89.4-2 libraries, on amd64 and arm64. amd64 is the reference.
The normal-prefix and short-gram state questions below are settled by
source. A later [bdb format-error check](918-prefix-bdb-format-error.md)
used both architectures' dependency binaries only to resolve the
source's build-conditional abort. No decompiler was used.

| Object / symbol | amd64 instruction offsets | arm64 instruction offsets | Finding |
| --- | --- | --- | --- |
| Pin `pinyin_phrase_segment`, `zhuyin_phrase_segment` (same offsets in each facade) | strlen call `+0x18`, conversion `+0x3d`, warning `+0x5e`, lookup `+0x82`, free `+0x8c` | strlen `+0x20`, conversion `+0x40`, warning `+0x68`, lookup `+0x94`, free `+0xa0` | Length walk precedes conversion; equality gates lookup. |
| GLib `g_utf8_strlen` | skip/add `+0xa0/+0xa9`, byte test `+0xac` | skip/add `+0x8c/+0x94`, byte load `+0x98` | The negative-length loop dereferences after advancing by the lead-byte width, even when it crosses the allocation's NUL. |
| GLib `g_utf8_to_ucs4` | successful `items_written` store `+0x11c`; error bypass `+0x11f` | successful store `+0xec`; error bypass `+0xf0` | Failure preserves the initial zero; canonical non-scalar encodings are not rejected by a scalar-range check. |

These are instruction findings, not executed crash/diagnostic outcomes.
`readelf` also confirmed both facades' assertion-expression and C++
signature strings. Allocation failures, extreme lengths and all possible
over-read outcomes remain outside this proof.

## Entry, result and lifetime trace

`pinyin_phrase_segment` (`pinyin.cpp:1446-1460`) and
`zhuyin_phrase_segment` (`zhuyin.cpp:965-979`) first count characters with
`g_utf8_strlen(sentence, -1)`, initialize `ucs4_len` to zero, convert,
and check equality. A failed precondition returns false before calling
`PhraseLookup::get_best_match` or changing the phrase result. Success
passes the converted words to the lookup and then frees the GLib buffer.
The real allocation/free contract stays intact (#924).

`PhraseLookup::get_best_match` (`lookup/phrase_lookup.cpp:120-157`)
prepares steps, seeds the sentence-start node (`:36-51`), searches each
UCS-4 span and evaluates bigram then unigram candidates. `final_step`
(`:382-428`) resizes and zero-fills the result **before** checking the
last step. An unmatched last step returns false with all null tokens;
an empty sentence has the seeded initial/final step, returns true and
leaves zero slots. A successful nonempty result contains tokens at span
starts and null tokens elsewhere. No text rendering is involved in these
getters; this differs from #894's candidate/sentence/export decoding.

The result's pin writers are allocation (`pinyin.cpp:1324-1325`,
`zhuyin.cpp:856-857`), this lookup and reset (`pinyin.cpp:2701`,
`zhuyin.cpp:1726`); free destroys it (`pinyin.cpp:1351`,
`zhuyin.cpp:879`). `*_get_n_phrase` writes the array length and returns
true (`pinyin.cpp:3271-3275`, `zhuyin.cpp:2227-2231`).
`*_get_phrase_token` first writes zero, then refuses an out-of-range
index, otherwise writes the stored token and returns true
(`pinyin.cpp:3277-3288`, `zhuyin.cpp:2233-2244`). NULL pointer cases
remain subject to the existing row-41 contract record, not a new quirk.

In Rust, each facade's `phrase.rs::segment_text` has one production
caller: its `*_phrase_segment` export. The helpers are duplicated, not
shared between crates. Valid `str` goes to
`engine/src/session/guess.rs::Session::phrase_segment`, then
`engine/src/phrase.rs::phrase_segment`. `Ok` replaces
`facade/src/instance.rs::InstanceCore::phrase_result`; `Err` logs through
`sentence.rs::refuse_lookup_gram` and preserves the old result. A failed
GLib conversion emits the exact precondition critical and preserves the
old result. A GLib-accepted non-scalar instead replaces it immediately
with a length-sized null-token vector and returns false, without lookup.
`InstanceCore::reset` clears the vector. The two getter implementations
read it and preserve the pin's out-of-range zeroing law for valid pointers.

## Partitions and current conclusions

| Input/state partition | Source conclusion / policy verdict | Investigation status |
| --- | --- | --- |
| Valid nonempty scalar UTF-8, sound tables/grams | Conversion produces the same scalar sequence; ordinary lookup contract retained, including existing math exceptions. | Input conversion and result plumbing established; not a new proof of every decoder path. |
| Empty C string | Pin lookup returns true with empty result; Rust seeded-step path does likewise. Prior phrase result is replaced. | Source-established; not run in this task. |
| Well-bounded rejected byte sequence whose length walk reaches its NUL, e.g. `ff fe 00` | Conversion leaves length zero; count is nonzero; false plus default-domain critical, old result preserved. Rust follows that lifecycle. | Source-established for this bounded shape; not every malformed byte string. |
| Accepted non-scalar encoding with sound scalar-only phrase keys/items, sound grams and successful backing-store reads | Identical in the requested observable channels: false, character-count-sized null result, getter outputs, no diagnostic, no observable session or persistent mutation beyond replacing the result. Prefix consultation only changes private scratch/cache state. | Bounded source proof below, including both facades and three backend read paths. Inherited tests exercise surrogate placement in `nihao`, not all non-scalars or state combinations; no fresh tests. |
| Truncated multibyte sequence, or a lead-byte skip crossing the C terminator | `g_utf8_strlen(-1)` does not validate continuations: it advances by the lead-byte width before testing the next byte. For a two-byte allocation `c2 00`, its next test is outside that allocation. Avoiding that illegal load is class (b). | Out-of-allocation load established in source and both compiled architectures; undefined execution has no single return/result/diagnostic to reproduce. Do not claim uniformly “one critical, false, old result preserved.” |
| Accepted non-scalar plus a bounded matching raw phrase key/item | Pin can return true/a token; Rust returns false/zeros. [Separate proposed runtime quirk](918-nonscalar-token-result.md), not an admission by analogy with #894. | Complete bounded source path; admission/gates not completed. |
| Accepted non-scalar after a matching scalar prefix; reached predecessor has a sound system gram and a user value of 1–3 bytes | Pin asserts while reading the user's total before finalizing the result; Rust returns false with a null vector and no warning. Existing class-(c) row 85 applies; the omitted warning is a defect, not an accepted runtime quirk. | Source-established in [the separate discrepancy record](918-prefix-short-gram-discrepancy.md), both facades/all backend wrappers; no new runtime witness. |
| Other malformed grams, backend read failures or damaged backend state during that prefix consultation | Normal-data equivalence does not cover these paths. [Follow-through](918-boundary-follow-through.md) separates existing safety/availability obligations, silent returned failures, and bdb diagnostic/handle-state differences. | Source traces and verdicts for the named fault classes complete; no deterministic claim for arbitrary undefined execution or whole-process resource failure. |
| NULL/dangling pointers, extreme lengths/narrowing, allocation failure, other malformed tables/grams | Outside the bounded conclusions above; existing independent records keep their scope. | Unverified here. No new “needs ruling” label or exception admission. |

The matching-key boundary follows `storage/facade_phrase_table3.h:111-138`
and `PhraseLargeTable3::search`: bdb
`storage/phrase_large_table3_bdb.cpp:180-207`, kc
`storage/phrase_large_table3_kyotodb.cpp:164-193`, tkrzw
`storage/phrase_large_table3_tkrzwdb.cpp:124-153`. Each passes raw word
bytes as the DB key. That establishes the missing validation boundary,
not an executed malformed-table witness or a complete backend audit.
`lookup/phrase_lookup.cpp:255-302` loads/merges grams on reached spans;
`:305-345` reads item length/frequency without decoding phrase text.

## Valid-prefix consultation before a non-scalar

The normal-data conclusion assumes a live instance, bounded complete
canonical encodings accepted by GLib, at least one non-scalar, consistent
scalar-only phrase keys/items, readable well-formed grams, successful
allocations and no concurrent mutation. It does not assume that the
scalar prefix is absent from the dictionary.

At `lookup/phrase_lookup.cpp:137-149`, a matching prefix invokes
`search_bigram2` before `search_unigram2`. The former reads predecessor
grams and merges them into `m_merged_single_gram` (`:255-302`);
`storage/ngram.cpp:277-351` changes only the merged destination, never
either source gram. The candidate generators read length/frequency
through `get_phrase_item` (`phrase_lookup.cpp:305-345`,
`storage/phrase_index.cpp:180-208`), then update private step arrays
(`phrase_lookup.cpp:348-377`). They call no training, store, remove,
save, unigram-frequency update or public log operation. The cached item
is a view, not an edit to the phrase body.

Consistent scalar-only keys cannot cover the non-scalar. Thus no complete
path reaches the final step, irrespective of prefix scores. `final_step`
(`:382-397`) replaces every old token with zero and returns false.
`*_phrase_segment` has no direct output pointer; its observable array is
the phrase result, subsequently exposed by the two getters traced above.
Both facades therefore have the same return, length, token values,
out-of-range zeroing, and silence as Rust's immediate `Unmatched` branch.

There **are internal mutations**: steps, cached item, merged gram and
backend read buffers/cache/error slots. They are not new phrase results
or learned state. The next `get_best_match` resets the sentence and
clears/reinitializes all steps (`:120-132`); each use of the cached item
first replaces it, and each successful merge clears/replaces its chunk.
If both source grams are absent the merge returns false and its old
chunk is not consumed (`ngram.cpp:279-295`). These members have no public
getter (`lookup/phrase_lookup.h:44-80`). Later scalar segmentation,
reset, training and save cannot observe the discarded prefix score.
This establishes normal-path equivalence in the requested channels, not
bitwise equality of private session memory.

### Backend reads and the limit of that conclusion

The exact snapshot source packages were read with Debian patches applied:
`db5.3 5.3.28+dfsg2-11`, `kyotocabinet 1.2.80-2`, and `tkrzw 1.0.32-1`.
Package files were checked against `Checksums-Sha256` in the
`20260831T000000Z` testing source index. These are dependency source
citations, not libpinyin line numbers. The pin attaches the system gram
read-only and loads the user gram into memory (`pinyin.cpp:393-401`,
`zhuyin.cpp:335-343`).

| Backend | Prefix read, diagnostics and state |
| --- | --- |
| bdb | `storage/ngram_bdb.cpp:127-146` calls `DB->get` with flags zero; a nonzero status yields no gram. `db5.3/src/db/db_iface.c:671-810` uses a transient read cursor and handle-owned return memory, not a put/delete. Ordinary hit/miss changes read/cache state only. Fault diagnostics are **not** automatically suppressed: `src/dbinc/debug.h:82-104` defaults to stderr when no output channel was configured, and the pin configures none. `src/common/db_err.c:200-240` can also set environment panic; `src/dbinc/region.h:310-324` makes it affect later operations. The [format-error record](918-prefix-bdb-format-error.md) traces a concrete diagnostic/panic path; do not classify all DB errors as silent misses. |
| kc | `storage/ngram_kyotodb.cpp:124-142` first checks size, then gets into a scratch chunk. `kcdb.h:1985-2041` uses read-only visitors returning `NOP`; no gram update. `kcstashdb.h:313-314,348-374,610-620` and `kchashdb.h:514-522,556-600,768-780` keep private error state; loggers default to NULL and the pin never installs one. A missing row is silent, although its private error slot changes. HashDB's default defrag unit is zero. A BROKEN/SYSTEM error can set `FFATAL`; the [follow-through](918-boundary-follow-through.md) proves that this private flag is neither exposed nor persisted by the read-only system handle. A successful size check followed by a failed/different-sized get reaches the pin's `check_result` assertion at `ngram_kyotodb.cpp:138-139`. The [second-read record](918-prefix-kc-read-assert.md) specifies the failure schedule and class-(c) obligation. |
| tkrzw | `storage/ngram_tkrzwdb.cpp:102-121` returns no gram for non-OK `Get`, otherwise copies into scratch. `tkrzw_dbm.h:132-163,1038-1046`, `tkrzw_dbm_tiny.cc:279-285,628-683` and `tkrzw_dbm_hash.cc:396-414,1427-1480,2427-2435` implement `Get` with a `NOOP` record processor and `writable=false`. The traced status-return branches do not invoke an update logger or write a row. The [follow-through](918-boundary-follow-through.md) traces metadata/body/file Status failures; process signal faults and invalid memory are separate from returned errors. |

The short user-value discrepancy is stronger than these remaining
fault questions: all three wrappers can deliver those raw bytes, and
the common merge necessarily asserts when a system gram is also present.
Its [record](918-prefix-short-gram-discrepancy.md) classifies the current
silent early return as a defect under the existing class-(c) obligation.
No binary was needed to settle that source-defined assertion or the
normal prefix's state changes. The runtime-quirk limb cannot override it.

### G1 ledger update

The integrated [G1 entry](glib-libc-verification.md#g1--utf-8ucs-4-conversion-and-category)
now carries the bounded findings and the remaining limits. #1156 landed
as `495afe2f`; only its broad phrase-segment row and associated #918
citations are extended here. The other family entries remain unchanged.

## Neighboring helpers and shared callers

The production C-string helper call sites were inventoried in both
facade crates. None calls `segment_text`, and none directly writes the
phrase-result vector. They are separate input boundaries, not alternate
routes into the segmenter's gate. Their input/lifecycle boundaries are
now traced against the pin in the [follow-through](918-boundary-follow-through.md#every-neighboring-helper-caller).
That bounded trace does not certify all downstream algorithms or clear
replacement of a helper with known unadmitted differences.
Paths in this table are relative to each facade's `src/`.

| Helper | Pinyin callers | Zhuyin callers |
| --- | --- | --- |
| `ffi.rs::cstr_to_parsed_prefix` | `parse.rs::parse_c_string` -> `pinyin_parse_more_full_pinyins`; `pinyin_parse_more_double_pinyins`; `pinyin_parse_more_chewings`; `iterators.rs::pinyin_iterator_add_phrase` reading | `parse.rs::zhuyin_parse_more_full_pinyins`, `zhuyin_parse_more_chewings` |
| `ffi.rs::cstr_to_strict` | `sentence.rs::pinyin_guess_sentence_with_prefix`, `pinyin_get_character_offset`; `predict.rs::pinyin_guess_predicted_candidates` | `sentence.rs::zhuyin_guess_sentence_with_prefix`, `zhuyin_get_character_offset` |
| `ffi.rs::cstr_to_string` | `dict.rs::pinyin_lookup_tokens`; `keys.rs::pinyin_parse_full_pinyin`, `pinyin_parse_double_pinyin`, `pinyin_parse_chewing`; `sentence.rs::pinyin_guess_predicted_candidates_with_punctuations`; `user_data.rs::pinyin_remember_user_input`; wrapper `cstr_to_owned_lossy` | `dict.rs::zhuyin_lookup_tokens`; `keys.rs::zhuyin_parse_full_pinyin`, `zhuyin_parse_chewing`; wrapper `cstr_to_owned_lossy` |
| `ffi.rs::cstr_to_owned_lossy` (actually wraps all-or-nothing conversion) | `iterators.rs::pinyin_iterator_add_phrase` text; test hook `context.rs::oxpinyin_test_set_user_bigram` | `iterators.rs::zhuyin_iterator_add_phrase` text and reading |

The sentence-prefix callers themselves differ on invalid input: pinyin
uses `cstr_to_strict(...).unwrap_or_default()`, whereas zhuyin returns
false on `None`. The follow-through confirms that the pin runs the decode
after conversion rejection on both facades. Parsing,
prediction, import, token lookup and character-offset output must not be
implicitly changed under a phrase-segment-only record.

## Existing verification and remaining work

Read, not executed here: both `phrase.rs` unit modules contain
`surrogate_segment_matches_pin_length_and_diagnostics`. They check a
surrogate before/inside/after `nihao`, six null slots, out-of-range zeroing,
no diagnostic, then recovery through `你好` and rejection of `ff fe` with
old **count** two and exact critical. They do not compare every old token
value after rejection. The existing contract driver has the six axis-C
surrogate cases, `issue542-phrase-segment-invalid-utf8-{pinyin,zhuyin}`,
`zhuyin-phrase-result-tokens`, and separate malformed-gram cases. This
checkpoint claims no fresh gate results.

To reproduce the reading, use the named revisions/packages rather than
host headers. Source archives are under the
[pinned Debian snapshot](https://snapshot.debian.org/archive/debian/20260831T000000Z/);
`dists/testing/main/source/Sources.xz` identifies the source package
versions, pool paths and checksums. `dpkg-source -x <package.dsc>` applies
the Debian patches. From the exact pin checkout and extracted libraries:

```sh
git show 074a2219:src/lookup/phrase_lookup.cpp
git show 074a2219:src/storage/ngram.cpp
objdump -dlS -C --no-show-raw-insn --disassemble=pinyin_phrase_segment "$ORACLE_PREFIX/lib/libpinyin.so.15"
objdump -dlS -C --no-show-raw-insn --disassemble=zhuyin_phrase_segment "$ORACLE_PREFIX/lib/libzhuyin.so.15"
objdump -dlS -C --no-show-raw-insn --disassemble=g_utf8_strlen "$GLIB_LIBRARY"
objdump -dlS -C --no-show-raw-insn --disassemble=g_utf8_to_ucs4 "$GLIB_LIBRARY"
readelf -p .rodata "$ORACLE_PREFIX/lib/libpinyin.so.15"
```

Use the corresponding arm64 libraries and `aarch64-linux-gnu-objdump`
for the second architecture. Match both pin-library hashes against
`tools/oracle/oracle-pin.txt`; the GLib libraries come from
`libglib2.0-0t64_2.89.4-2_{amd64,arm64}.deb`. These commands read files;
they execute neither library. No captures or disassembly dumps belong
in the repository.

Before a change: finish the unresolved partitions and any remaining
source-triggered binary questions, decide each independent difference under the
policy, use a discrepancy record only where a difference is established,
and update the landed shared ledger for all affected families/callers.
Then add narrowly scoped lifecycle cases to the existing contract driver
and focused tests for both facades, covering old token values and warning
behavior, and run the applicable gates. Keep both expected observations
for any admitted POLICY-ACCEPTED case. No removal is authorized by this
audit alone. Known differences and the unrun behavior gates still prevent
a completed #918 implementation PR.
