# User phrase content past 4 GiB (#1514, phase 1)

Date: 2026-11-01 UTC, captured with `date -u`. Updated 2026-11-02 UTC against main `851a6304`; the update read `/private/tmp/oxpinyin-1514-pin` at `074a2219` and did not run the pin. The object inspection below is historical evidence, not a new disassembly.
Status: maintainer ruling recorded 2026-11-02 UTC; implementation pending the mapping stack ([#1547](https://github.com/shenghaoc/oxpinyin/pull/1547) → [#1557](https://github.com/shenghaoc/oxpinyin/pull/1557)).

The pin's first wrapped append is an **in-buffer overwrite, not inherently
undefined behaviour**. It changes the bytes seen through other live slots without
updating their offsets or accounting for their changed frequencies. Once the
content size stops growing, subsequent additions reuse the same wrapped offset;
this is not an advancing circular buffer. Defined corruption can survive save
and reload after compaction. A later operation becomes class (b) when it forms
an out-of-allocation item view, copies beyond a real source or destination, or
uses a corrupted length to address another object out of bounds.

Two qualifications matter. The pin's content grows monotonically **between
explicit compactions**, not throughout a context's lifetime. End-import, save
and mask-out compact it. Also, serializing a still-large arena has a separate
signed allocation-size narrowing; it is not a defined way to write a complete
4-GiB arena and reload it.

## Scope and evidence

Read-only derivation from libpinyin
`074a2219c90feaf962d0d24f034514033ece5f99`, with a clean source checkout;
oxpinyin items below describe `origin/main` at `f3d08577`.
The pin was not run. No build, test, differential or measurement was performed.
This covers mutable user content, including user modifications to a loaded
system sub-index. It does not investigate oversized original system files or
backend DBM internals.

Pin citations below are `path:line` at that commit. Source links:
[phrase_index.cpp][pi], [phrase_index.h][ph], [memory_chunk.h][mc],
[phrase_index_logger.h][logger], [novel_types.h][types],
[pinyin.cpp][py], [zhuyin.cpp][zy].
Rust citations use file and item names, as required by the source policy.

Only the implementation-defined signed conversion in allocation needed an
object inspection. The existing Linux/amd64 tkrzw reference `libpinyin.so.15`
matches `libpinyin_sha256_tkrzw` in
[`tools/oracle/oracle-pin.txt`](../../tools/oracle/oracle-pin.txt).
Its `pinyin::MemoryChunk::set_content(unsigned long, void const*, unsigned long)
[clone .isra.0]` at `0x4d340` subtracts the old end at `+0x39`, tests the low
32 bits with signed `test %eax,%eax; jle` at `+0x3c..+0x3e`, sign-extends the
positive result at `+0x4b`, and calls `memmove` with the original 64-bit length
at `+0x6a..+0x6d`. `SubPhraseIndex::store` at `0x4eb00`, `+0xbd..+0xcf`,
passes its full content length to that helper. These instructions settle the
allocation truncation, not an outcome after an invalid copy. They can be
inspected without executing the library:

```sh
objdump -d -C --no-show-raw-insn --start-address=0x4d340 --stop-address=0x4d4d0 "$PIN_LIB"
objdump -d -C --no-show-raw-insn --start-address=0x4eb00 --stop-address=0x4ec20 "$PIN_LIB"
```

Here `PIN_LIB` is that already-built reference object. All other conclusions
below follow from the source. Under the
[2026-10-29 contract ruling](../source-policy.md#internal-language-artefacts),
observable unsigned wrap is reproduced unless a maintainer grants an exception.
Calling the entire over-4-GiB state class (b) would discard defined behaviour.
The classifications here identify operations. The maintainer ruling below specifies the safe answers; implementation is pending.

## Offset and arithmetic inventory

Let `M = 2^32`, `C` be the content's `size_t` logical size, `A` its actual
allocation capacity, and `s[t]` a slot's stored offset. `table_offset_t` is
`guint32` (`src/include/novel_types.h:128`). On the reference build the item
extent read from header bytes `l` and `n` is
`L(l,n) = 6 + 4*l + n*(2*l + 4)`, at most 132,096 bytes.
The six-byte header is `src/storage/phrase_index.h:56`; the packed key fields
are `src/storage/chewing_key.h:41–47`; the extent calculation is
`src/storage/phrase_index.cpp:203`.

| Path | Pin behaviour and classification | oxpinyin today | Observable difference |
| --- | --- | --- | --- |
| Add or replace an item | `src/storage/phrase_index.cpp:208–215`: narrow `C` to `u32`, change zero to 8, copy the complete supplied item, publish that offset, add its unigram modulo `M`. **Defined** for valid source/destination storage; see the fixed-offset overwrite below. | `crates/oxpinyin-user/src/user_library.rs`, `UserLibrary::replacement` checks both offset and end before publication; `apply_replacement` appends. `store_arena.rs`, `arena_add_phrase` and `arena_promote`, propagate preparation errors. | Refuses even the append whose end first exceeds `u32::MAX`; the pin can append that item and only wrap on the next add. No earlier-slot overwrite or new alias is produced. |
| Remove; remove/re-add on import | `src/storage/phrase_index.cpp:219–234`: read the existing `u32` slot, copy its header-derived extent, zero only that slot, subtract the copied unigram modulo `M`. No shrinking, offset subtraction or content-size narrowing occurs. **Defined** if the view and copy fit; class (b) at the invalid view/copy otherwise. | `user_library.rs`, `remove_body`, `remove`, `add_reading`, `compact_garbage`; `store_arena.rs`, `arena_remove` / `arena_add_phrase`. Safe views, checked replacement and extra garbage compaction. | Pin removal can subtract an overwritten value and leave other aliases live; Rust's normal history has neither the wrap nor those collateral changes. Churn is reclaimed earlier. |
| Read / add unigram | `src/storage/phrase_index.cpp:150–205`: stored slot is `u32`; slot position is `(token & PHRASE_MASK) * sizeof(table_offset_t)`. On this 64-bit build `sizeof` promotes `offset+1` and `offset+2` to `size_t`, so they do **not** wrap at `u32::MAX`. The two header reads and unigram read are checked against `C`. The full item extent is not checked. See consumer bounds below. | `user_library.rs`, `item`, `pronunciation_count`, `increment_unigram`; `crates/oxpinyin-data/src/phrase_library.rs`, `PhraseItemView`. | Rust reads checked content views; it does not generate the pin's wrapped aliasing. A loaded overlapping valid layout can still expose shared bytes, but it does not make the add algorithm equivalent. |
| Nth pronunciation | `src/storage/phrase_index.cpp:33–43`: narrows `6 + 4*l + index*(2*l+4)` to `table_offset_t`; the following frequency offset is widened by `sizeof`. This is a separate **defined unsigned narrowing**, also possible with a huge public `nth`; normal `index < n <= 255` cannot overflow it. Copy bounds and the caller's buffer still apply. | `crates/oxpinyin-capi/src/dict.rs`, `pinyin_token_get_nth_pronunciation`, and the Zhuyin twin implement the getter through the session's item access. | Arena wrap changes the getter's input. Separately, `PhraseItemView::pronunciation` rejects `nth >= n`; it does not wrap its byte offset, so a huge `nth` that lands back on a valid pin reading yields zeroed keys in the C getter. No policy change is made here. |
| Diff | `src/storage/phrase_index.cpp:392–436`: no content offset is stored in the log and no new `u32` content cursor is introduced. Walk the union of slot ranges, compare complete header-derived items, emit ADD/REMOVE/MODIFY plus a total header. **Defined** while reads fit; class (b) at invalid item view, `memcmp` (`phrase_index.h:235–239`) or payload copy. | `crates/oxpinyin-user/src/persistence.rs`, `diff_records_with_new_total`, `diff_records_against`; checked live/override items. | Diff sees the Rust state, which lacks wrap-caused changes. Even a pin item over 65,535 bytes encounters a separate Rust log-encoding refusal. |
| Journal replay | `src/storage/phrase_index.cpp:439–519`: ADD and growing MODIFY call the same wrapped add. REMOVE copies/zeros; non-growing MODIFY directly overwrites the current view. `MODIFY_HEADER` replaces the total; it does not recompute it. **Defined** for in-buffer views and payloads; invalid payload/stale-pointer/NULL paths are class (b), with asserts separately preserved. | `crates/oxpinyin-user/src/replay.rs`, `Content::append`, `Merge::add`, `Merge::modify`: append at the full `usize` end, then return `u32::try_from(offset).unwrap_or(u32::MAX)`. | Saturates the slot to `0xffffffff`, keeps growing the buffer, and does not overwrite the low address. This may read unrelated bytes near 4 GiB, fail decoding, or alias later saturated slots; it is neither the pin's wrap nor the arena's refusal. |
| Compact / range | `src/storage/phrase_index.cpp:624–675`: range end narrows the pointer difference (number of offset words) to `phrase_token_t`; ordinary masked slots bound it at most to 2^24. Compact starts a fresh sub-index and calls add for every readable slot, in slot order. **Defined** while reads/copies fit; the new content can itself wrap. No independent content-offset conversion is hidden here. | `user_library.rs`, `compacted` refuses if `live + 8` does not fit `u32`, then uses checked replacements; `crates/oxpinyin-user/src/store.rs`, `compact_libraries`, discards a `library.compact()` error. | Rust retains the old arena on that error, instead of constructing a wrapped compacted arena. Extra dead-greater-than-live compaction also changes which histories reach the limit. |
| Store a `.bin` sub-index | `src/storage/phrase_index.cpp:365–389`: `offset`, `index`, each `+= sizeof(...)`, `offset += m_phrase_index.size()` and `offset += m_phrase_content.size()` narrow back to `u32`. Copies retain their full `size_t` lengths. These unsigned cursor changes are **defined**; the large one-shot copy can first hit class (b) through the allocation bug below. `end` is never assigned; the facade does not use it (`:251–258`). | `user_library.rs`, `store`, checks the section offsets, content length and frame size. `crates/oxpinyin-data/src/chunk_write.rs`, `build_chunk`, is the value-only writer. | Session save returns a codec error instead of producing wrapped section offsets. The value-only writer has additional inconsistent arithmetic, detailed below. |
| Load a sub-index | `src/storage/phrase_index.cpp:237–247,271–278,335–362`: passing `chunk->size()` to the `u32 end` parameter narrows it; increments of the `u32 offset` narrow; `index_two - 1 - index_one` and `index_three - 1 - index_two` are `u32` subtractions and can underflow. Direct separator reads and `set_chunk` precede the final `index_three <= end` check. **Defined** only while those addresses/views lie in real storage; class (b) otherwise. | `user_library.rs`, `load` / `load_headers`; `chunk_write.rs`, `sub_phrase_index_sections` and `validate_phrase_item`: checked section slices and item extents. | Malformed wrapped sections are rejected/skipped, sometimes retaining readable count headers, rather than dereferenced. Ordinary small files containing overwritten but well-formed items can load on both sides. |
| Logger framing / cursors | `src/storage/phrase_index_logger.h:61,158–220,233–301`: whole-log cursor and append offset are `size_t`, **not `u32`**. Item lengths narrow to `guint16` at `:248,259,269–270,285–286`. Copies use full sizes; MODIFY places the new item after the **narrowed** old length. **Defined** byte overwrite/framing damage if storage fits; `next_record` can later copy outside the log. | `crates/oxpinyin-data/src/user_files.rs`, `encode_log_records` refuses payloads over `u16::MAX`; `LogReader` checks framing; `replay.rs` applies reached-record fault rules. | No imitation of the pin's truncated length fields or MODIFY payload overlap. The existing replay safety dispositions do not authorize a different defined content wrap. |
| Whole-chunk allocation / file framing | `src/include/memory_chunk.h:95–98,290–296,416–460,475–521,536–557`: allocation delta narrows to signed `int`; file length, checksum length and reload `file_size - 8` narrow to `guint32`. The unsigned conversions are **defined**; an underallocated bulk copy is class (b). | `crates/oxpinyin-data/src/chunk_format.rs`, `build_memory_chunk` / `build_memory_chunk_owned` refuse oversized payloads; `user_files.rs`, `read_chunk_payload` reads the declared prefix. `crates/oxpinyin-input/src/chunk.rs`, `save`, writes the already-framed bytes. | No oversized frame is produced by the normal Rust builders. The reader's declared-prefix rule alone is not the pin's physical-file-size-modulo comparison. |

`src/storage/phrase_index.cpp:677–717,750–795` adds mask removal and the
logger's unsigned header-total arithmetic, not another content-offset cursor.
The active `PhraseItem` append/removal routines use `size_t`
(`:56–100,114–140`); their `guint8` pronunciation count can wrap separately.
The disabled append routine at `:46–54` adds no live path.

## What is overwritten, and what the totals mean

For supplied item bytes `B` of length `k`, add computes
`w = C mod M`, replacing `w == 0` by 8. If `C >= M` and `w+k <= C`, the write
changes exactly `[w,w+k)` and leaves logical size `C` unchanged
(`src/include/memory_chunk.h:290–296`). With the bounded items at the first
ordinary crossing, this interval is near the beginning and well inside the
already allocated multi-gigabyte content. The allocation helper's signed
delta may spuriously request a small amount of spare space; it does not turn
this into an append at the high end.

An earlier live item's bytes change precisely where its old byte interval
intersects `[w,w+k)`. This can overwrite its header, unigram, text, keys,
pronunciation frequencies, several adjacent items, or only dead bytes. All
other slots keep their old offsets. The added token alone receives `s[t]=w`;
an existing slot at `w` aliases it exactly. A slot inside the write now treats
interior bytes of `B` as its own header. A slot whose header is unchanged can
still have changed text or frequencies. Unreachable old copies remain in the
arena. No phrase-table or pinyin-table index is rebuilt as a consequence of
these collateral changes (`src/storage/phrase_index.cpp:208–234` versus the
explicit index updates in `src/pinyin.cpp:597–605`).

Because `C` did not change, later adds choose **the same `w`** until explicit
compaction or another operation actually changes the content size. Their
different payload lengths can overwrite a longer or shorter prefix at that
address. Slots created by successive adds therefore alias the latest bytes,
not separate positions around a ring.

There is no maintained per-item sum in the pin's sub-index. Define `S` only
as the sum, modulo `M`, of the unigrams currently readable through all live
slots, counting aliases once per slot. If the added payload's unigram is `f`,
the sub-index total changes from `T` to `T+f mod M`. It does **not** subtract
the old value of a replaced slot or compensate for collateral overwrites.
For public import replacement, the preceding removal subtracts the copied
old unigram first. Thus even with a numerically unchanged replacement unigram,
`T` can diverge from `S` because another live slot changed under the write.
Pronunciation sums likewise become sums of the bytes now read, modulo `M`;
they are not repaired. Sources: `src/storage/phrase_index.cpp:61–83,120–140,
208–234`; `src/storage/phrase_index.h:136–163`.

The facade has its **own** `guint32 m_total_freq`. Add/remove change it through
`src/storage/phrase_index.h:663–691`; unigram addition advances it before the
sub-index's overflow/error result (`:628–634`; sub-index guard at
`src/storage/phrase_index.cpp:169–175`). Load/merge/mask subtract the old
library total and add the resulting one (`:237–247,286–298,713–715`).
Compaction starts the new library total at zero and adds every copied item's
unigram, so it becomes the sum of the **source items read during that walk**.
It leaves the facade total unchanged (`:648–675`). If the compacted output
also wraps, its final visible sum can again differ from that accumulated total.
Even without another wrap, the facade and compacted library totals can differ.

## Later readers: first invalid operation, not a blanket verdict

`get_phrase_item` checks only the slot and two header-byte reads against `C`
(`src/storage/phrase_index.cpp:185–201`). Missing header bytes return an error
before creating the new view. Otherwise it calls `set_chunk` with `L(l,n)`
(`:203–205`). That helper records `begin+L` as both end and capacity without
checking the parent (`src/include/memory_chunk.h:251–258`).

Three bounds must stay separate:

1. The old item's nominal end is not a safety boundary: bytes borrowed from a
   following item are real bytes in the same allocation.
2. `C` is the logical parent end. A header-derived item extending past `C` but
   within `A` is a logical over-read, not automatically C++ undefined behaviour.
   The owned allocation's spare bytes are zero-filled on growth
   (`src/include/memory_chunk.h:112–113,125–132`). Reads/writes confined to that
   initialized allocation can be defined. In a loaded subsection, a following
   separator or other bytes still within the owning chunk are also real
   storage; `A` means that owner's actual bound, not the child view's asserted
   capacity. A compact/save can make such bytes part of a new item.
3. If `s[t]+L > A`, `set_chunk` first forms an out-of-allocation end pointer
   at `src/include/memory_chunk.h:256–257`: class (b) already starts there.
   The subsequent first byte access outside `A` is separately identifiable
   below; no deterministic result after the invalid view is claimed. A pointer
   exactly one past the allocation is permitted as an end, not to dereference.

Rust consumers converge on `crates/oxpinyin-runtime/src/lib.rs`, `RuntimeDict`
(including `token_text`, `token_pronunciation_count`,
`token_nth_pronunciation`, `with_user_nth_pronunciation_words`) and `RuntimeLm`,
with `GenericUserStore::with_user_item` in `store_arena.rs` supplying user
item views. The table names the additional reader-specific paths.

| Consumer | Defined result while its bounds/preconditions hold | First class-(b) trigger or other boundary | Rust reader / difference |
| --- | --- | --- | --- |
| Length/count and unigram getters; candidate length/frequency filters | Return overwritten header/frequency values; ranking and filtering can change while indexes still name the old text. Count needs only the two header bytes, though the pin constructs a full view first. Sources: `src/pinyin.cpp:1731–1737,1817–1865,1893–1921,1953–1984,2354–2365,2393–2405,2786–2833`; `src/zhuyin.cpp:1166–1173,1259–1270,1306–1308,1778–1825`. | Invalid view as above; otherwise unigram's first over-read is `MemoryChunk::get_content`'s `memcpy` at `src/include/memory_chunk.h:375` if its four bytes exceed the real allocation. A zero facade total reaches the explicit assert at `src/pinyin.cpp:1859`; that is a defined abort, not class (b). Out-of-range floating-to-integer conversion in scoring is a separate undefined numeric operation, not unsigned content wrap. | `user_library.rs`, `item` / `pronunciation_count`; `crates/oxpinyin-capi/src/dict.rs`, token getters; Zhuyin's `dict.rs` twins. Rust's usual state never acquired these overwritten values. Header-only fallback can retain the count of an otherwise rejected loaded item. |
| Phrase/token strings, candidate strings, sentence strings, export, addon promotion | Copy the current text and use it, including a different valid string or conversion failure for invalid UCS-4. `src/pinyin.cpp:1632–1645,1995–2029,698–739,812–837,2548–2558`; `src/zhuyin.cpp:1096–1113`; `src/lookup/lookup.cpp:37–58`. | `get_phrase_string` calls `memcpy` (`src/storage/phrase_index.cpp:103–105`, `memory_chunk.h:375`). It can over-read the source or overflow the caller's `ucs4_t[MAX_PHRASE_LENGTH]` when `l > 16`, even when the entire arena view fits. Prefix `begin > l` can also send a pointer/length outside the local buffer at `pinyin.cpp:1644`. | `UserLibrary::item`, the session dictionary and C `dict.rs` getters use bounded views/owned output. They cannot reproduce a stack overwrite; a safe answer for a newly identified class-(b) case still needs its disposition. |
| Nth pronunciation / export keys | Reads the keys and frequency at the narrowed item-relative offset; valid `nth` returns overwritten bytes. `src/storage/phrase_index.cpp:33–43`; `src/pinyin.cpp:718–728,2801–2818`; `src/zhuyin.cpp:1793–1810`. | First source over-read at the same `memcpy`, or destination overflow when `l > 16` in a local `ChewingKey[16]`. A rejected nth read is ignored by the public getter; appending its uninitialized buffer is a distinct existing invalid-index path, not evidence that all valid wrapped reads are undefined. | C `dict.rs` nth getters operate on checked Rust item data; no low-arena overwrite was generated. |
| Phonetic lookup, constraints and training | Uses current unigram, length, keys and pronunciation frequencies; length mismatch can simply make a path unpronounceable. `src/lookup/phonetic_lookup.h:643–697,924–929`, `phonetic_lookup.cpp:140–163`; `src/storage/phonetic_key_matrix.cpp:534–600,603–669`. Training edits current frequency words in place and does not append content. | After the common view check, the first outside frequency load/store is `UnalignedMemory<guint32>::load/store` in `src/storage/phrase_index.h:146–147` or `phrase_index.cpp:128,140`; key comparison can read outside supplied keys. The matrix helper checks equal accumulated key length before passing its dynamic key array, so a changed item length alone does not imply that key over-read here. | `store_arena.rs`, `arena_train`, `arena_add_unigram`; `user_library.rs`, `train`, `increment_unigram`; runtime's dictionary/LM consumers read canonical checked items. Changed counts from wrap are absent. |
| Character-offset / remember-input helpers | A still-one-character item is priced through its current pronunciation rows. `src/pinyin.cpp:3166–3172,3643–3653`; `src/zhuyin.cpp:2121–2127`. | These pass `&key`, a **single** key, to `get_pronunciation_possibility`. If overwritten `l > 1` and comparison advances beyond that key, the first invalid read is in `pinyin_compare_with_tones` (`src/storage/pinyin_phrase3.h:111–137`). This differs from the dynamic-key matrix caller above. | C/facade helpers consume safe Rust keys; no unchecked single-key over-read. |
| Phrase segmentation | Uses overwritten `l` to choose the next step and overwritten unigram for its cost (`src/lookup/phrase_lookup.cpp:307–345`). | If `nstep+l` is outside the allocated steps arrays, first indexed read is `g_ptr_array_index` at `:350–353`. It need not reach a text or pronunciation read first. | Runtime/session dictionary consumers have checked storage and Rust step handling. The pre-wrap state already differs. |
| Remove, diff, compact, replay | Can copy bytes spanning old item boundaries, compare them, remove a slot, or persist them. Non-growing replay MODIFY writes its new bytes directly (`src/storage/phrase_index.cpp:498–499`). | The common invalid view precedes `remove`'s copy (`:228`), `diff`'s `memcmp`, or compact's copy (`:668` → `:212`). Replay payload over-read starts at logger `set_content` (`phrase_index_logger.h:176,184,194–197,206–210` → `memory_chunk.h:293`); absent REMOVE dereferences NULL at `phrase_index.cpp:468`, and growing absent MODIFY at `:489`. A retained view into an allocation moved by an append becomes dangling on its next use. These are the existing row-107 family, not a new reason to waive in-buffer wrap. | `replay.rs`, `Content::header`, `item`, `write`, `Merge` and finalization; `persistence.rs` reached-record framing checks. They distinguish data end from modeled allocation capacity and retain existing class-(b) handling. Their append offset still saturates. |

The older `PinyinLookup2` has the same item consumers at
`src/lookup/pinyin_lookup2.cpp:420–467,646–651,748–761`; the C contexts
construct the `PhoneticLookup` implementation (`src/pinyin.cpp:406–409`,
`src/zhuyin.cpp:348–351`). The disabled character-offset alternatives in
`src/pinyin.cpp:3242–3269` and `src/zhuyin.cpp:2197–2224` are not live readers.
The tool helper `src/storage/tag_utility.cpp:384–404` uses the same fixed text
buffer and has the same `l > 16` destination bound. Once the API has copied a
candidate string, its string getter reads that copy; a later arena overwrite
does not retroactively change already-returned caller-owned text.

## Persistence and reload

### Small compacted result: corruption can survive

`pinyin_end_add_phrases` compacts and marks the context modified
(`src/pinyin.cpp:655–659`). A permitted save compacts again before writing
(`:1132–1145`); Zhuyin does the same (`src/zhuyin.cpp:540–554`). Each live slot
is copied independently from what it **currently** points to. Exact aliases
become independent copies with the same bytes if the new compacted arena stays
below the boundary. Compaction does not restore original text or reconcile
the phrase/pinyin index keys with it.

For a `USER_FILE`, `_write_files` serializes the compacted sub-index to its
configured `.bin.tmp` (`src/pinyin.cpp:978–992`), then renames it in the normal
file pass. Its payload has total, three offsets, slot array, content and `#`
separators (`src/storage/phrase_index.cpp:365–389`), framed by a four-byte
length and checksum (`src/include/memory_chunk.h:541–549`). Reload reads
that file and loads its sub-index (`src/pinyin.cpp:303–320`). When its sections
and items fit, the overwritten text/frequencies persist. The reloaded facade
adds the compacted library total; it need not equal the stale facade total
of the previous process.

For a user-modified `SYSTEM_FILE`, save rereads the original chunk and writes
a `.dbin` **snapshot diff**, not the history of each append
(`src/pinyin.cpp:944–975`, `src/storage/phrase_index.cpp:271–283,392–436`).
There is no separate user-content write-ahead journal: `.dbin` is the
`PhraseIndexLogger` journal. It holds the changed bytes by token, not content
offsets, alias relationships or the dead 4-GiB history. Valid small MODIFY/ADD
records can therefore preserve the corruption on replay into a small arena,
without recreating the original offset wrap. Replay at init/reload is
`src/pinyin.cpp:266–276` (`src/zhuyin.cpp:209–219`). A compare mismatch stops
the merge; it is not repaired by consulting the old phrase-table text.

The saved header is the compacted library total, but merge applies that
header **before** ADD/REMOVE/growing-MODIFY arithmetic. In-place MODIFY does
not adjust the total (`src/storage/phrase_index.cpp:399–400,453–512`). Thus a
reloaded log's total is the header value plus the subsequent applicable
additions/subtractions modulo `M`, not necessarily its item sum. Both facade
and library bookkeeping must be derived from this order.

The separately stored `user_pinyin_index.bin` and `user_phrase_index.bin` are
DBMs, not these `MemoryChunk` files (`src/pinyin.cpp:996–1012`). The content
overwrite does not rename their keys or change their token values. Consequently
an old text key can still find a token whose returned text is now another
phrase, after save and reload. The bigram file likewise stores token-based
state independently (`:1014–1018`).

### Still-large result: stop at the first invalid allocation/copy

`MemoryChunk::ensure_has_space` computes a pointer difference and stores it in
signed `int` (`src/include/memory_chunk.h:95–98`). The inspected reference
object uses the low signed 32 bits. In normal small incremental appends the
positive delta is small; the capacity doubles as necessary. When a fresh
output chunk receives a whole content area in one copy
(`src/storage/phrase_index.cpp:384`), a delta in `[2^31,2^32)` instead looks
non-positive and skips allocation; a delta `2^32+r`, for small `r`, requests
only `r`. Existing destination capacity is nowhere near the full requested
extent. The ensuing `memmove` (`src/include/memory_chunk.h:293`) is the first
actual out-of-allocation write. Strictly, forming an end pointer beyond the
destination allocation in the helper already lacks a valid C++ pointer
range. No defined saved bytes or return status can be asserted after this.
This problem can happen **before** the content-offset wrap, at a 2-GiB bulk
copy; it does not prevent small incremental growth through 4 GiB in memory.
For content size C and fresh output chunk, `B = 18 + I` bytes (header,
separators and index) have been written before the copy. The requested
growth is C: for C < 2^31 the positive delta grows sufficiently; for
2^31 ≤ C < 2^32 the signed delta is negative and growth is skipped. For
C = k·2^32+r at or beyond 2^32, GCC reduces it modulo 2^32 to r if
r < 2^31 and r−2^32 otherwise; this is never sufficient growth for C.
The attempted destination ends at B+C. The separate `table_offset_t`
addition wraps when B+C reaches 2^32, if execution gets that far. GCC's
[documented signed conversion](https://gcc.gnu.org/onlinedocs/gcc/Integers-implementation.html)
settles the narrowing without a new build or disassembly. Rust does not reproduce the pin’s internal capacity calculation; the unsafe bulk copy is class (b).

`PhraseIndexLogger::store` performs the same kind of full-log copy into a new
chunk (`src/storage/phrase_index_logger.h:115–117`), so a huge diff log has the
same separate problem even though its cursor is `size_t`. For a fresh empty
destination and a log of J bytes, the copy requests growth J; at J ≥ 2^31
the narrowing is nonpositive or loses whole multiples of 2^32, so its own
unsafe-copy threshold is 2^31 bytes. The logger is reached by the
SYSTEM_FILE/DICTIONARY diff (`src/storage/phrase_index.cpp:271–283`,
`src/pinyin.cpp:944–971`, `src/zhuyin.cpp:577–607`), not USER_FILE storage.

For completeness, the unsigned file-format consequences can be derived
conditionally, without pretending that the invalid copy successfully executed:

- If a sufficiently allocated destination were available, let
  `a = 18 + slot_array_size` and assume that part fits. Store copies all `C`
  content bytes at `a`, then places `#` at `(a+C) mod M` and writes
  `index_three = (a+C+1) mod M`. If `a+C >= M`, that separator overwrites an
  earlier byte; it does not extend the output. The payload size is then
  `a+C`, **not** `a+C+1` (`src/storage/phrase_index.cpp:384–388`).
- `MemoryChunk::save` writes the payload length modulo `M`, computes the
  checksum over only that modulo-length prefix (the checksum parameter is
  `guint32`, `src/include/memory_chunk.h:136,541–545`), but requests a single
  write of the **full** payload (`:549–552`). A short write returns false;
  these facade writers ignore that result. No retry or guaranteed full-file
  output follows from the source.
- `MemoryChunk::load` narrows physical `file_size-8` to `u32`, compares it to
  the header, allocates and reads only that prefix (`:424–460`). The mmap
  variant maps the full physical file but exposes only the narrowed prefix
  as its chunk (`:483–521`). For the wrapped store above, the final separator
  usually lies exactly at the declared prefix end and `index_three` is one
  beyond it. The user-file heap loader's direct separator dereference
  (`src/storage/phrase_index.cpp:356`) is then out of bounds, before the end
  check. Other wraps can damage the header or make earlier separator checks
  fail. This is not a defined full-arena round trip.
- A hypothetical full oversized `.dbin` frame likewise reloads only its
  modulo-length prefix. If the prefix ends on a record boundary, replay sees
  only those records; if it cuts a claimed payload, `next_record` copies past
  the log at the sites listed above. Malformed length fields can also select
  the existing assert/abort/mismatch paths.

There is another, smaller journal boundary. The maximum header-derived item
is 132,096 bytes, which exceeds a logger's `u16` length. ADD/REMOVE write the
full item after its narrowed length; MODIFY copies the new item after the
narrowed old length, overwriting part of a large old payload
(`src/storage/phrase_index_logger.h:243–293`). These are defined writes when
the allocation fits. Replay follows the short lengths, so remaining payload
bytes can become record headers; the first later invalid payload copy or
explicit abort must be classified separately. A normal public import item
with `l <= 15, n <= 255` is at most 8,736 bytes and does not cross this boundary.

## Reachability and workload

A minimal **call shape**, without a premanufactured giant user file, is:

1. `pinyin_init(systemdir, userdir)` and, if necessary,
   `pinyin_load_phrase_library` for a configured user library
   (`src/pinyin.cpp:326–408,446–462`).
2. `pinyin_begin_add_phrases(context, library)` (`:506–511`).
3. Repeated `pinyin_iterator_add_phrase` for the same phrase and pronunciation,
   without ending the import. The existing-token arm always removes and
   re-adds the complete item, even for an identical pronunciation or zero
   count (`:563–583`; `src/storage/phrase_index.cpp:56–92`). A zero count
   avoids introducing a frequency overflow while accumulating dead bytes.
4. After the cumulative append sizes reach at least `M`, one further accepted
   add uses the wrapped offset. No training call is required.

The public import iterator admits lengths 1–15 because `MAX_PHRASE_LENGTH` is 16 and
the iterator rejects `>= MAX_PHRASE_LENGTH`
(`src/include/novel_types.h:119`, `src/pinyin.cpp:640–644`). A one-pronunciation
item uses `10+6*l` bytes: 16 for one character, 100 for fifteen. Starting
nearly empty requires about 268 million one-character replacements or 43
million fifteen-character replacements, plus the first wrapped add.
The initial content reservation is eight bytes.

Fewer calls are possible without a giant input file: prepare one
15-character item with 255 distinct valid pronunciation sequences, then
repeat one of those sequences with count zero. The whole 8,736-byte item is
copied each time, so about 492,000 replacements suffice (plus preparation).
The growing 255-pronunciation preparation itself consumes about 1.1 MiB of
append history. Do not add a 256th distinct pronunciation for this derivation:
that introduces the separate eight-bit count wrap. These are source-derived
orders of magnitude, not timings, executions, or a proof of a globally
minimum call count over arbitrary starting files.

A concrete arithmetic witness shows that **defined** corruption can survive,
without relying on malformed headers. In a fresh user sub-index, add a
one-character item A (16 bytes) first, then four distinct two-character
one-pronunciation padding items (22 bytes each). The content size is
`8+16+4*22 = 112`. Add/re-add a distinct one-character item B with one
pronunciation and count zero exactly `(M-112)/16 = 268,435,449` times.
The size is then exactly `M`. The next B replacement writes 16 bytes at
offset 8, exactly over A. A and B now both point to B's valid one-character
item. No out-of-bounds operation is needed for this overwrite or their
getters. End-import compacts them to independent copies; a subsequent save
and clean fini/reinit can retain both copies of B while A's phrase-table key
still names A's token. Use phrases absent from the initial tables and a
fresh configured user sub-index. This is an algebraic construction, **not a
performed reproduction**.

Ordinary training updates pronunciation/unigram words in place
(`src/pinyin.cpp:2670–2690`, `src/lookup/phonetic_lookup.h:924–929`), so it
cannot grow this content by itself. Addon promotion does append a complete
item (`src/pinyin.cpp:2534–2558`); remembered input reaches `_add_phrase`
(`src/pinyin.cpp:3594–3606`). Mask-out removes user slots or reloads/replays
system logs, then compacts (`:1224–1295`); replay can reach the add boundary,
but repeated ordinary mask-out is not a monotonic dead-byte accumulator.
Zhuyin's matching import/end/save/mask paths are
`src/zhuyin.cpp:392–544,547–554,759–829`.

The pin also compacts at save and end-import. Therefore ordinary typing,
training, and modest imports ended normally do not accumulate gigabytes of
dead phrase content across those calls. An unusually large uninterrupted
bulk import, many long highly polyphonic replacements, or large replay can
reach it on a 64-bit host with sufficient memory. Multi-gigabyte live data
can also reach the limit during compaction. This is feasible for automated
or adversarial workloads, not a credible expectation for normal interactive
use; no measured rate or time-to-reach is claimed. A 32-bit process may
exhaust address space before owning such an arena; the in-memory reachability
argument is for the 64-bit reference environment.

oxpinyin additionally compacts whenever dead bytes exceed live bytes
(`user_library.rs`, `replacement`, `compact_garbage`), preserving the stored
total during this extra collection. A repeatedly replaced small live set
therefore does not approach 4 GiB there; identical-reading imports are even
cheaper because `add_reading` changes them in place. This history difference
is observable once the pin would overwrite live bytes; reproducing just the final `u32`
conversion in Rust would not settle it. A decision must cover virtual append
history or the compaction policy as well as arithmetic.

## Remaining Rust paths and error propagation

The mapping stack changes the implementation after this baseline: [#1553](https://github.com/shenghaoc/oxpinyin/pull/1553) changes replay storage and mapping plumbing; [#1557](https://github.com/shenghaoc/oxpinyin/pull/1557) builds on it for training in the live buffer. Neither is a dependency of this finding; recheck their final form before implementation.

The inconsistent mechanisms are not interchangeable safe answers:

- `UserLibrary::replacement` / `compacted` in
  `crates/oxpinyin-user/src/user_library.rs` refuse with an arena-size or
  integer-conversion error. Offset/end checks precede extra compaction.
  `prepare_replace` lets `arena_add_phrase` / `arena_promote` in
  `store_arena.rs` reject before publishing new index metadata. Thus the
  pin's successful append/overwrite and public success can become a Rust
  operation failure. `UserLibrary::add_reading` additionally updates a matched
  reading in place: identical-reading import churn does not append at all in
  Rust, even before its extra garbage compaction matters. New readings use
  checked replacement. In-place training does not allocate an arena slot.
- `GenericUserStore::arena_replacement` in `store_arena.rs` ignores a failed
  `replace`; its comment explicitly identifies #1514's interim known
  divergence from #1512. It can install other items while leaving the
  unholdable one absent. `open_session` in `store_libpinyin.rs` uses `?` on
  these same replacements, including replay ADD candidates, and refuses the
  open with `UserStoreError::Persistence`. This is conditional on the arena
  reconstruction exceeding its bound; it is not a universal rejection of
  every log whose replay history exceeds 4 GiB.
- `Content::append` in `replay.rs` grows at the full `usize` end and only
  saturates the **returned slot**. Its `origin_and_content` inline-original
  fallback also saturates offsets/lengths with `unwrap_or(u32::MAX)`.
  `Merge::visible` follows that slot, so finalization may retain different
  bytes, fall back, or omit an unreadable item under existing replay rules.
  The saturated slot is not a sentinel meaning “append failed.”
- `UserLibrary::load` validates every live item's complete extent within the
  content slice. `load_libraries` in `persistence.rs` records a skipped
  malformed library and may retain `load_headers` for count getters. The
  pin can instead use overlapping readable items or reach the invalid
  load/view operations above. Valid low-offset aliases are not automatically
  rejected merely because they overlap; full-frame validity matters.
- `UserLibrary::store` checks section sums and also sums unigrams into `u64`
  and refuses a non-`u32` total, whereas pin compaction adds modulo `M`.
  `GenericUserStore::compact_libraries` in `store.rs` ignores its arena
  compaction error after updating other bookkeeping. No coherent limit
  policy should be inferred from this mixture.
- `build_chunk` in `crates/oxpinyin-data/src/chunk_write.rs`, reached by
  `persistence.rs::stage_rest` for a value-only save, checks each starting
  offset but not that item's ending offset. Its final `index_three` uses
  `u32::try_from(content.len()).unwrap_or(u32::MAX)` followed by ordinary
  `u32` additions; overflowing those additions can panic with overflow
  checks, or wrap without them. The subsequent `build_memory_chunk` still
  refuses a payload over `u32::MAX`. This is another path to align, not the
  checked session writer or an implementation of the pin's overwrite.
- `encode_log_records` in `crates/oxpinyin-data/src/user_files.rs` refuses
  item payloads over 65,535 bytes. The `chunk_format.rs` framing functions
  refuse payloads over `u32::MAX`; `stage_rest` propagates these codec errors
  before the affected file's write. `oxpinyin-input/src/chunk.rs::save` adds
  no content-offset conversion: it writes the caller's framed bytes.
  `read_chunk_payload` checks and exposes the declared prefix, ignoring
  trailing physical bytes; the pin additionally compares physical length
  modulo `M` with the header. Valid full oversized frames can agree on the
  exposed prefix, while arbitrary trailing-length mismatches need not.

These observations describe the baseline, before the ruling below is implemented. Existing compatibility rulings continue to govern their already-classified cases.

## Maintainer ruling (2026-11-02 UTC)

The maintainer, shenghaoc, selected option 1 in a human turn on 2026-11-02 UTC,
after the 16:27 UTC #1514 comment and before the 16:44:02 UTC time observed
while processing that turn. The exact receipt time was not captured. This review
clarifies the read and save boundaries. Match the pin wherever its behavior is
defined. For undefined behavior, use class (b) and the safe answers below.
Implementation waits until the mapping stack ([#1547](https://github.com/shenghaoc/oxpinyin/pull/1547) → [#1557](https://github.com/shenghaoc/oxpinyin/pull/1557))
has merged; it rewrites the same user-library replay and store files.

- Reproduce the defined content offset narrowing modulo 2^32, including
  zero → 8, the overwritten bytes, aliases and totals, with `wrapping_*`
  for unsigned wrap. Apply this consistently to `UserLibrary::replacement`
  and `compacted`, `GenericUserStore::arena_replacement`, `open_session`,
  replay `Content::append`, and the dependent mask, diff, save and reload
  paths. A saturated slot or silently omitted item is not the ruling.
- Compact at the pin's points only: end-import, save and mask-out, in the
  pin's slot order and with its totals (`src/storage/phrase_index.cpp:624–675`,
  `src/pinyin.cpp:655–659`). Remove the dead-over-live automatic trigger so
  both reach the wrap after the same history. The pin's explicit compaction
  rebuilds the items; do not assume content grows across those operations.
- A header-derived read that crosses the original item end but remains inside
  content returns the following bytes as the pin does. A read that would pass
  the content end answers **no item** and keeps other changes, as row 107
  option 1. Passing the logical content end is the ruled safe boundary; it is
  not necessarily C++ undefined if the owning allocation still contains the
  bytes. Other independently unsafe operations remain class (b).
- Preflight all planned serializations and refuse before writing or removing
  any file if one reaches the unsafe copy. Both facades compact first
  (`src/pinyin.cpp:1132–1146`, `src/zhuyin.cpp:547–554`). For a USER_FILE,
  the copy uses compacted content C, with B = 18 + I bytes already in the
  fresh chunk: its growth threshold is C ≥ 2^31, destination end B+C.
  SYSTEM_FILE and DICTIONARY diff journals have their separate fresh-chunk
  copy threshold J ≥ 2^31 for the complete log J. Refusal answers `false`,
  retains the dirty session and permits retry; see policy rows 111 and 112.

Between the pin's compaction points, content space grows with cumulative
appended bytes, as at the pin; logical content space is the same for the same
history, although allocation capacities need not be. This is worse than main's
dead-over-live trigger, which keeps garbage proportional to live content. The
cost preserves the pin's offset history; the implementation PR must restate
this source-derived space trade-off. No measurement is needed.

Use a small, test-only wrap modulus in the same offset computation as
production; `max_content_len` alone only refuses growth. Exercise zero → 8,
overwrites, aliases, totals and compaction at the pin's points, alongside
REMOVE/MODIFY, replay, mask and diff. Target facade checks at preflight,
false return, unchanged files, dirty state and retry/reload on both facades.
A real-boundary contract differential is appropriate only if reachable in
reasonable time. No oracle run is required for this documentation PR.

[pi]: https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/phrase_index.cpp
[ph]: https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/phrase_index.h
[mc]: https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/include/memory_chunk.h
[logger]: https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/phrase_index_logger.h
[types]: https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/include/novel_types.h
[py]: https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/pinyin.cpp
[zy]: https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/zhuyin.cpp
