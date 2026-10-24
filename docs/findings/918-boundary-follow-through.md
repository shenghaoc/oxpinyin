# #918 — remaining input, caller and store partitions

Source investigation · 2026-10-24 UTC. This extends the
[entry/result audit](918-phrase-segment-source-audit.md), retaining its
exact pin, dependency versions and Rust baseline. Baseline conclusions
are distinguished below from the new row-85 prefix processing. The GLib
conversion and neighboring shared helpers remain; this is not removal
clearance. No exploratory oracle execution or fault injection was used.

## Segment input and backing-data partition closure

All claims below assume live ABI pointers, a NUL-terminated allocation,
no concurrent access to the instance, and lengths representable by the
pin's `int` lookup interface unless the row explicitly says otherwise.

| Partition | Source result and verdict | Completeness |
| --- | --- | --- |
| Empty/scalar input, sound data | Conversion/entry/result behavior is identical; ordinary decoder and math records retain their scope. | Conversion and lifecycle trace complete; not a new proof of the whole decoder. |
| GLib-rejected malformed/overlong/partial encoding, length walk stays inside the allocation and reaches NUL | Pin count is positive, `items_written` remains zero, false plus the exact precondition critical, old phrase result preserved. Rust matches. GLib's `g_set_error_literal` with NULL error returns immediately (`glib/gerror.c:518-530`), so conversion adds no second warning. | Source complete for these conditions; compiled length/conversion order checked on both architectures. |
| The lead-byte walk leaves the supplied allocation | Pin's load is out of bounds; no deterministic return/result/stderr can be assigned. Avoiding that walk is class (b), not an unclassified runtime quirk. | Source and amd64/arm64 instructions establish the illegal load. Possible process outcomes are undefined, not an unread policy question. No new crash probe. |
| GLib-accepted non-scalar, sound consistent scalar-only keys/items and grams | False/all-zero length-sized result, no warning or observable prefix state; identical. | Complete bounded source argument in the entry audit. |
| GLib-accepted non-scalar with a bounded matching raw phrase key and item | The pin can return true and a token; Rust returns false/zeros. | [Independent token-result discrepancy](918-nonscalar-token-result.md), source established; proposed runtime-quirk treatment requires its own admission/gates, not #894 by analogy. |
| Reached user gram shorter than a total, with a system gram | Defined assert; existing class-(c) rule. New prefix processing uses the ordinary warning/false/old-result refusal, identical under class (c). | [G1 prefix entry](918-prefix-lookup.md), both facades/all three wrappers; native checks passed, contract gates pending. |
| Reached partial gram item, or sub-total row without a system gram | The merge/get-frequency walk can read outside the row. Existing row 98 explicitly covers these lookup sites; class (b), not a new quirk. The new prefix walk routes reached `UnsafeLookupPredecessor` through its existing one-warning refusal. | Exact unsafe shapes and safe early exits are already source-traced by row 98 and `single_gram.rs::{checked_get_freq,checked_search}`. The new prefix check can encounter these existing Rust refusals; this audit claims no new runtime witness for them. No deterministic pin result is claimed. |
| Structurally bounded but unsorted/duplicate/zero-total/wrapping-frequency grams | They can change prefix scores, but cannot create a path across a non-scalar when keys/items remain scalar and consistent. No persistent update occurs. | Same final-step proof applies provided the actual merge/search walk stays in bounds and no assert is reached; unsafe/asserting shapes belong to the preceding rows. |
| Inconsistent key/item length, truncated item storage or malformed token-array bounds | `PhraseTableEntry::search` walks four-byte tokens to the raw byte end (`storage/phrase_large_table3.cpp:29-51`); `get_phrase_item` creates a view using declared length (`phrase_index.cpp:180-208`); `save_next_step` indexes by that length (`phrase_lookup.cpp:348-377`). Out-of-range accesses are class (b); an in-range mismatched length can manufacture a complete path and a different token result, an unadmitted difference under limb 4. | Source hazards established. Not admitted by the bounded non-scalar record, which requires matching lengths and valid storage. Exact output of arbitrary corrupt lengths is deliberately not normalized. |
| NULL/dangling/unterminated pointers | Existing row 41 governs NULL guards; dangling or unterminated pointers violate the C input lifetime/allocation precondition. | No new compatibility promise or fabricated deterministic output for invalid pointers. |
| Extreme lengths or allocation failure | The pin narrows `glong` to `int` and adds one for step count (`phrase_lookup.cpp:120-127`); allocations elsewhere may assert/abort. Rust has its own allocation failure paths. | Excluded from bounded equivalence. A whole-process OOM/oversized-input equivalence is not established by this audit; no removal may rely on it. |

The converter accepts canonical historical encodings through `0x7fffffff`,
including surrogates, above-Unicode scalars and five/six-byte encodings
(`glib/gutf8.c:665-747`). These share the same Rust `Unmatched` branch.
Embedded NUL terminates the C input on both sides; bytes after it are not
another partition of this API's string. The pin does not render item
text during segmentation, so #894's text-output records cannot certify
the token-result partition.

## Backend failures during prefix consultation

These baseline rows concern a reachable scalar prefix followed by a non-scalar,
with consistent scalar phrase keys/items. They distinguish a returned
read failure from process faults or invalid memory. The wrappers never
write a gram during segmentation.

| Failure point | Propagation and observable state | Verdict |
| --- | --- | --- |
| bdb `DB->get` returns an ordinary silent miss/error without poisoning its environment | `ngram_bdb.cpp:140-146` leaves the gram NULL. The other gram or unigram path may score the prefix, but final result stays false/zeros. | Identical for that explicitly silent, non-poisoning return. Not all bdb failures have these properties. |
| bdb reaches a damaged hash key's format error | `__ham_getindex` -> `__db_pgfmt` prints diagnostics and poisons the environment; later reads can emit `DB_RUNRECOVERY` diagnostics. | [Separate source-established defect](918-prefix-bdb-format-error.md). The published amd64/arm64 libraries return from panic; this is not an abort/class-(c) escape. |
| bdb unrecoverable OS read error | `src/mp/mp_bh.c:238-267` calls `__os_io`; `src/os/os_rw.c:21-115,145-185` falls back to the checked read path and reports its error through `__db_syserr`. The libpinyin wrapper's false return does not erase stderr. | Diagnostic difference is established for the reached error branch; no exact address-dependent diagnostic bytes or error frequency are asserted. No blanket quirk admission. |
| kc first `check` fails | `ngram_kyotodb.cpp:130-134` returns no gram. `HashDB::read_record` errors (`kchashdb.h:2962-3015`) set private error/FFATAL, but default NULL loggers suppress its report calls. The system HashDB is read-only; its `accept` does not test FFATAL, and `close`/`synchronize_impl` write metadata only under `writer_` (`:556-600,933-956,2384-2418`). User StashDB reads are in-memory NOP visitors (`kcstashdb.h:1285-1348`). No public API reads these private error/flag slots. | Same final false/zeros and no public/persistent change for a returned first-check error, assuming the remaining reads are safe. This closes the earlier FFATAL-state question for this read-only system handle. |
| kc first check succeeds, second get fails or changes size | `ngram_kyotodb.cpp:138-139` asserts the saved size equals get's result. A real later IO failure in `read_record` can return -1; source order is unambiguous. | [Defined assert, class (c)](918-prefix-kc-read-assert.md); shortcut skips the required warning. This is a distinct fault schedule from a short value, not a claim that an immutable healthy DB spontaneously changes size. No runtime injection performed. |
| tkrzw metadata/body/file read returns Status failure | `HashRecord::ReadMetadataKey/ReadBody` (`tkrzw_dbm_hash_impl.cc:53-185`) returns the failure; HashDB's read-only `ProcessImpl` returns it without changing `healthy_` or invoking a writer/update logger (`tkrzw_dbm_hash.cc:1427-1480`). `MemoryMap{Parallel,Atomic}File::Read` (`tkrzw_file_mmap.cc:433-446,1004-1017`) returns status silently. libpinyin supplies no gram. | Same final false/zeros for these returning failures, with no observable learned/persistent state. Memory mapping SIGBUS, allocation termination or arbitrary corrupted-pointer execution is not a Status-return path. |

The source distinguishes backing-store corruption from malformed gram
**values** in an otherwise healthy DB. The latter reach the common
`SingleGram` code and its existing class-(b)/(c) obligations regardless
of backend. Replacing the shortcut's classifier cannot discard these
obligations merely because the text is non-scalar.

## Every neighboring helper caller

The caller list in the entry audit is complete for production references
to these four helpers in the two facade crates. The test-only user-gram
hook is not a libpinyin-compatible export. This follow-through reads the
pin boundary as well as each Rust callsite. All helpers pass valid UTF-8
unchanged; this table audits the conversion boundary, not every downstream
algorithm or existing independent defect. None writes `phrase_result`
or calls `segment_text`.

| Caller group | Pin boundary / Rust behavior / conclusion |
| --- | --- |
| Both multi-full-pinyin parsers and pinyin import's reading | Pin passes raw bytes and strlen (`pinyin.cpp:638,1497-1524`, `zhuyin.cpp:1017-1043`). `FullPinyinParser2::parse/final_step` (`storage/pinyin_parser2.cpp:217-380`) cannot bridge an invalid byte with a matching table spelling; disconnected later steps do not satisfy `position == parsed_len`. Rust retains the valid prefix. Existing row 48's bounded byte-prefix conclusion holds. Its matrices/parse lengths change normally; the phrase result does not. Large gint16/int lengths are outside this bounded claim. |
| Pinyin multi-double and both multi-chewing parsers | `pinyin_parser2.cpp:532-577` probes ASCII key/tone bytes. The three reachable keyboard parser classes probe scheme membership first (`zhuyin_parser2.cpp:216-270,408-461,718-769`); non-ASCII malformed bytes cannot be keyboard keys. Scheme dispatch (`pinyin.cpp:1161-1193`, `zhuyin.cpp:708-740`) does not expose the direct Unicode import parser here. Rust's valid-prefix cut preserves this boundary. |
| Both single full-pinyin parsers | Raw whole-input parsing resets the key after the apostrophe assert (`pinyin_parser2.cpp:164-215`). Invalid bytes without apostrophe produce a miss/zero key; Rust's empty fallback does likewise. With any apostrophe plus invalid UTF-8, Rust loses the apostrophe before its guard: pin asserts, subject silently zeroes the key and returns false. Existing row 21 class-(c) warning obligation is unmet. This caller must not be certified by row 48. |
| Pinyin single-double and both single-chewing parsers | Raw whole-input paths are `pinyin.cpp:1527-1537,1566-1579`, `zhuyin.cpp:1045-1056`; byte/table checks reject malformed high bytes and preserve the key on those misses. Rust's empty fallback also returns false without writing the key. Empty-input parser under-reads and unrelated scheme-specific assertions retain their existing safety records; no blanket empty-input equivalence is claimed. |
| Both sentence-prefix guesses | Both pins clear/reseed prefixes, convert, then run constraint update/decode even on converter failure (`pinyin.cpp:1389-1443`, `zhuyin.cpp:911-962`). Pinyin Rust's empty fallback preserves that conversion-rejected route; zhuyin Rust returns before it. A non-scalar followed by a known scalar suffix can still yield suffix tokens in the pin; the strict Rust fallback cannot. These are independent prefix-list/lookup lifecycle differences, not phrase-result differences. |
| Pinyin prediction and punctuation prediction | Pin clears candidates/prefixes before conversion; rejected input leaves them empty and returns false (`pinyin.cpp:2413-2453`). Strict Rust prediction returns before clearing, preserving old candidates/prefixes: a defined lifecycle difference. The punctuation wrapper instead uses the empty fallback and its normal clear path, returning true on no prefixes like `:2456-2498`. Non-scalar-plus-scalar-suffix input can reach pin prediction/punctuation work that neither empty fallback nor strict rejection performs. Borrowed-candidate pointer lifetime is the separate existing #542 record. |
| Both character-offset callers | Pin checks matrix/range before conversion (`pinyin.cpp:3193-3240`, `zhuyin.cpp:2148-2195`), then precomputes every character's token (`pinyin.cpp:3098-3130`, `zhuyin.cpp:2063-2092`). On rejected text with a valid offset, false/out untouched matches Rust. Rust validates text before the engine's range assertion guard: invalid text plus an assertion-triggering offset omits that class-(c) warning. Accepted non-scalars with matching raw keys can proceed at the pin; Rust refuses. |
| Both token lookups | `pinyin.cpp:2652-2668`, `zhuyin.cpp:1678-1694` convert and search the raw whole key; `reduce_tokens` clears the caller's real GArray. Rust's dictionary search also clears it on a miss. Accepted non-scalars with matching raw keys can produce tokens at the pin; Rust searches empty text. Rejected conversion leaves zero length/NULL at the pin, which still reaches a backend search; zero-key/backend assertion behavior is not assumed identical to searching an empty Rust string. The real GArray contract stays intact. |
| Both imports' phrase text; pinyin remember | Converter rejection leaves length zero and fails the pre-store length gate (`pinyin.cpp:627-646,3677-3681`, `zhuyin.cpp:513-532`); Rust's empty fallback also refuses without storing. Accepted non-scalars can be stored by the pin's `_add_phrase`; remember additionally needs a token for every character (`:3688-3703`). Rust cannot store them. This produces malformed data, not a valid-input counterexample; #894 traces those producers but does not admit this input-side difference. |
| Zhuyin import's reading | Pin's `ZhuyinDirectParser2` parses space/apostrophe-delimited Unicode chunks and preserves successfully parsed earlier chunks (`zhuyin_parser2.cpp:859-957`); Rust turns the entire malformed reading into empty text. For reading bytes `e3 84 8b e3 84 a7 20 ff 00` (`ㄋㄧ`, space, ff, NUL), one key survives at the pin and can satisfy a one-character phrase, while Rust refuses. A bounded `ff` byte in the same first chunk instead makes both fail. This may change a store operation and cannot be conflated with the keyboard multi-parser prefix rule. |
| `cstr_to_owned_lossy` wrapper | It is exactly all-or-nothing `cstr_to_string`, despite the name. Its callers are covered by the import rows; test-only context hook inputs are not another supported ABI route. |

The adjacent differences are source findings, **not new exceptions**.
The assert-warning omissions are defects under existing class-(c) rows.
Other changed prefix/candidate/store behavior has no new admission here;
these helpers remain unchanged and cannot be replaced under a
phrase-segment-only claim. The shared-helper proof now identifies the
actual bypasses instead of calling them unverified or needs-ruling.

### Zero-length token-lookup boundary

`*_lookup_tokens` is different from the segmenter: it forwards the
converter's zero length and NULL result to `FacadePhraseTable3::search`
(`storage/facade_phrase_table3.h:111-138`). Successful empty-string
conversion also searches a zero-length key, but with a live allocation.
Rust's `data/src/phrase_table.rs::PhraseTable::search` skips the system
DB for empty text; the user search
(`user/src/phrase_table.rs::search`, reached through
`store.rs::phrase_table_tokens_for_text`) still probes an empty key.
Consequently even a bounded empty-key row with enabled tokens in the
system table is outside a generic empty-text equivalence claim. This is
a malformed-table condition, not proof of a valid supported-data defect.

The BDB phrase table is a B-tree, unlike its hash gram store. Its get
argument checks do not reject a zero-sized DBT
(`src/db/db_iface.c:825-950,2879-2940`), and the default comparator
does not dereference zero-sized keys (`src/btree/bt_compare.c:155-181`).
KC's `_assert_` is build-conditional (`kccommon.h:157-188`); it is not
evidence for an unconditional NULL-key abort. The exact kc pin's
`ProtoTreeDB::accept` string-construction guard accepts NULL with size
zero on both architectures (amd64 `+0xe5,+0x310,+0x313`; arm64
`+0xf0,+0xfc,+0x100`). The compiled `PlantDB<HashDB,49>::accept`
instead passes the original pointer and size to `memcpy` (amd64
`+0x102`, arm64 `+0x110`). These are symbols in the hash-matching kc
`libpinyin.so.15` listed by `tools/oracle/oracle-pin.txt`;
`ProtoTreeDB` denotes the `ProtoDB<std::map<std::string,std::string>,17>`
instantiation. Read with `objdump -dlS -C --no-show-raw-insn` and its
arm64 counterpart; no execution was used.

Tkrzw's phrase system/user stores are TreeDBM/BabyDBM, not the gram
HashDBM/TinyDBM pair. Their lookup scratch constructors copy the key
(`tkrzw_dbm_tree_impl.cc:68-75,111-118`,
`tkrzw_dbm_baby.cc:275-283`). A NULL source to `memcpy`, even with zero
size, does not establish a portable defined output. Source identifies
the invalid library precondition; exact compiled outcomes of these
NULL/zero copy calls and their resolved libc implementations remain
unverified. No deterministic diagnostic/return is assigned to them,
and the ordinary non-NULL empty-key path is kept separate. This narrow
remaining binary question blocks a complete shared-helper replacement
proof, not the sound-data segment-prefix conclusion. No relevant
artifact is claimed unavailable.

## Current prefix implementation and remaining gates

The new prefix check removes the unconditional bypass for the scalar
prefix. The bounded row-85 path is implemented as identical under class
(c), with the normal false/warning/old-result refusal. Sound completion
still publishes the full-length zero-token result silently. The
[entry](918-prefix-lookup.md) names tests and pending gates.

Returned backend failures need separate treatment from that proved path.
The pin wrappers above leave a failed gram NULL and continue. In Rust,
`runtime/src/lib.rs::RuntimeLm::merged_row` first reads the user value,
then `data/src/lm/mod.rs::BigramLanguageModel::load_merged_row` reads the
system value. `data/src/bigram_table.rs::BigramTable::load_raw_successors`
propagates backend errors and also rejects empty/partial system values.
`store/src/bdb/ffi.rs::Db::get` maps every status except DB_NOTFOUND to
an error; `kyotocabinet/ffi.rs::Db::get` uses one read-only accept and
returns its error; `tkrzw/mod.rs::db_get` also propagates its status.
The new `engine/src/phrase.rs::step_costs` call converts that failure to
`EngineError::Scoring`. The facade's ordinary warning helper recognizes
only the two lookup-gram refusal variants, so this error returns false
silently and preserves the old result. The pin can instead continue to
its length-sized zero result. This is a **source-established, unadmitted
error-lifecycle difference**, not silent-failure equivalence for the new
implementation. The same-data backend diagnostic/handle-state details
are not certified by an eight-case short-gram gate. KC's second-read
schedule is particularly separate: the Rust wrapper makes one visit.

The row-85 warning does not require a new policy exception; the earlier
scope conflict was resolved by the explicit prefix-processing ruling.
No blanket ruling is inferred for the error-lifecycle differences above,
raw non-scalar dictionary keys or the other neighboring callers. These
are not grounds to remove the GLib call. Named baseline store-error
paths and all listed caller boundaries have been traced; the adjacent
NULL/zero-copy compiled outcome and whole-process allocation/extreme-size
behavior remain unverified. No unavailable artifact is asserted.

Native touched-crate tests, fmt/clippy and 141 Python harness tests pass.
Compatibility cases, the named skip-prefix mutation and shipping-size
checks are pending. Full affected-family removal coverage is not claimed,
and no PR is opened while the compatibility gate is blocked.
