# External I/O boundary

Date: 2026-10-28 UTC · Status: **maintainer ruling and implementation
plan.** Stage state:

- stage 1 implemented (2026-10-28 UTC);
- stage 2 implemented (2026-11-03 UTC, #1576); its save calls moved in
  stage 3 (2026-11-01 UTC);
- stage 4 implemented (2026-10-30 UTC, #1437);
- stage 5 in progress.

This record states the external I/O ruling, the per-call inventory and
the staged implementation. The pin is libpinyin
`074a2219c90feaf962d0d24f034514033ece5f99`. oxpinyin source is main
`a52461cc297a4e67b07406167e6581e0e9f211fa`. The pin source read was the
clean `/tmp/libpinyin-src` checkout at that commit. Pin citations below
are paths and lines in [that tree](https://github.com/libpinyin/libpinyin/tree/074a2219c90feaf962d0d24f034514033ece5f99).
oxpinyin citations name files and items, not line numbers. The source
comparison used Rust 1.97.1, GLib 2.84.4 and glibc 2.41 where specified.
Those versions do not establish the implementation of every native host.
No oracle, probe or differential was run for this record.

## Ruling and scope

Maintainer ruling, 2026-11-01 UTC (`date -u`): every program under the
pin's `tests/` is ported as a drop-in tool, without exception. Its binary
name, arguments, stdin protocol, stdout/stderr bytes, exit status and fixed
file paths are the interface. The binaries live in nonshipping
`oxpinyin-utils` and are not installed.

Maintainer ruling, 2026-10-28 UTC:

1. Every call that crosses the program's edge, in the library and
   utilities, on input and output, uses the pin's exact call. Use:
   - host ISO C/POSIX calls;
   - GLib calls where the pin uses GLib;
   - Rust `std::io` under the C++ contract for the pin's C++ streams
     (the 2026-10-31 UTC ruling in item 5).

   Otherwise, Rust std is allowed only when the source of both
   implementations proves identical observable results for every input.
   NOT EQUIVALENT and UNSETTLED both require the pin call.
2. The tools are drop-in replacements for the pin's utilities: binary
   names, options, help, messages, inputs and outputs. K-mixture working
   models use the pin's backend DBM through `oxpinyin-store`.
3. `scripts2/` Python build generators are out of scope: they are build
   steps, not installed tools. The utility list below includes the pin's
   `noinst_PROGRAMS` as well as `bin_PROGRAMS`, per the tool ruling.
4. The same call is not the same defect. The existing class (b) records
   for undefined reads stand, including `1297-punct-words`. The pin's
   asserts and aborts are reproduced as aborts. Policy row 39
   (`LC_NUMERIC`) stands. Row 109's mapping call is active. Its release
   follows the later external API misuse ruling below.
5. **Ruling of 2026-11-02 UTC:** library bindings and permitted semantic
   adapters follow the [Rust/C++ interoperability ruling](compatibility-policy.md#rust-cpp-interoperability).
   The 2026-10-31 UTC C-library and macro rules remain:
   - C library calls go through the `libc` crate (approved for
     `oxpinyin-input`) or `extern "C"` declarations.
   - GLib calls go through the workspace's `glib-sys`.
   - C++ streams go through Rust `std::io` under the C++ contract.
   - Only a function that neither crate binds is declared by hand.
   - Macros (`assert`, `g_return_val_if_fail`, `G_STRFUNC`) are not
     reproduced. Their checks are plain Rust, and their text is not
     imitated. This is recorded as a POLICY-ACCEPTED language artefact
     ([eio-tool-check-messages](eio-tool-check-messages.md)).

   The tools follow this rule, and so do the library's stage 1 and 4
   calls through `libc` in `oxpinyin-input`'s `native.rs`. The
   interoperability ruling governs backend binding choices and the frozen
   Kyoto Cabinet contract.
6. A new bidirectional `oxpinyin-input` crate owns these calls in scoped
   unsafe modules with `// SAFETY:` comments and safe typed functions.
   `oxpinyin-data` becomes `forbid(unsafe_code)` after extraction. The C
   facades retain C ABI marshalling; `oxpinyin-store` retains DBM backends.

The ruling replaces #1304's language-artefact treatment **for external
I/O**. It does not change the remaining internal language-artefact
policy, class (a) maths, class (b), or Rust-interface freedom. C API
arguments and marshalling stay in the C adapters. DBM records, and the
backend calls that obtain and persist them, stay in `oxpinyin-store`.
Decoding their contents stays in the owning data/domain layer.

**Implementation must not start until the last #1175 extraction has
landed.** #1374 met that prerequisite (main `410c9ca5`). The inventory
below keeps the source comparison as it was made, and notes where stage 1
makes the pin's call.

## Stage 2 mapping preparation

Member 1 prepares `MappingSource`, `PendingMapping` and `Mapping` in
`oxpinyin-input/src/chunk.rs`:

- The source opens with libc, seeks end then start, and exposes
  individual four-byte reads.
- Consuming the source makes exactly one whole-file writable private
  mapping.
- The data caller interprets and validates the header and checksum while
  the pending owner keeps the descriptor open.
- Rejecting the pending mapping drops the mapping before the descriptor.
  `finish` closes the descriptor and returns the sole mapping owner.
- Reads borrow that owner. Writes require its exclusive borrow.

The pin sites are `src/include/memory_chunk.h:475-524`; callers are
`src/pinyin.cpp:250-260,284-294,952-959,1260-1268` and
`src/zhuyin.cpp:196-203,230-237,585-592,795-803` at `074a2219`.
The existing memmap2 dependency/version/features are used on every OS:
0.9.11 `src/unix.rs::MmapInner::map_copy` calls `MmapInner::new` with
`PROT_READ | PROT_WRITE`, `MAP_PRIVATE`, offset zero; its `Drop` releases
the entire mapping. The unchanged lockfile resolves 0.9.12, whose same
items retain that sequence. The maintainer chose memmap2 on Windows too;
this boundary does not reproduce the pin's non-mmap Windows heap path.
On Windows it opens chunk paths with `_wopen`, preserving the Rust `OsStr`
as UTF-16 under the [Windows platform adaptation ruling](compatibility-policy.md#native-rust-implementation).
Config and other narrow filename opens remain outside this change (#1575).

**Mapping activated.** Production transfers each loaded system chunk's one
whole-file writable private mapping to its live sub-index; an addon owns its
one mapping directly. Data's mapper and memmap2 dependency are gone and data
forbids unsafe code. Save, reload and mask may make operation-scoped fresh
mappings. Row 109's wrapped residual remains POLICY-ACCEPTED.
`mask_out` maps each system library and reads its journal once, one library at
a time, and stops at the first fault (rows 76, 91, 92, 96, 97, 108). The walk's
`masked_log` raises every journal fault the mask path has (`HeaderToken`,
`MultipleHeaders`, `SurvivingRecord`, `ShortMaskedItem`); the records it hands to
the merge are a header and MODIFYs, so `ShortAdd` cannot occur there and a
mismatch only ends a merge. No later library is opened after a fault.

The complexity argument below is a source-derived proposal, kept as the
record. It is not a measurement. The eager B form stored
`present: Vec<bool>`, `unigrams: Vec<u32>` and `readable: Vec<bool>`. Rust
`Vec<bool>` uses one byte per element, so that logical storage was `6S`
bytes plus allocation overhead, not `4S` plus two packed bitmaps.

`LibraryHistory` has no first-write scan and no full presence bitmap or
unigram array (member 4). A journal-addressed slot saves only its offset,
readability and unigram before that slot's first mutation. The sparse entries
are accompanied by the loaded offset array's slot count: every later slot has
an explicitly absent original even if the live array grows during an import.
These entries serve replay's added/removed classification, original offsets on the exceptional
overlong walk, and the internal `unigram_delta` reader. A detached library's
item-count query takes one scalar from the live owner; candidate frequency
reads the live item and total. Removal checks the live offset and item, using
the sparse old offset for an addressed slot. Save still reloads its diff base
for that operation. There is no persistent second mapping.

The sparse entries are packed into 16 bytes each and appended during replay.
The loaded journal's item-record count reserves their capacity; a stable slot
sort at replay finish keeps each slot's first capture. Consecutive captures of
one slot keep the first entry without growing the vector. Later writes insert
into the sorted entries.

Normal replay finalization sorts the journal-addressed slot list; it does not
walk `S` offset slots. An overlong or wrapped write can reach the exceptional
overlap walk, which reads offset words from the live mapping and the saved old
offset for a journal-addressed slot. The replay undo vector remains `O(R)` and
borrows old journal bytes. Actual RSS and page-cache sharing were not measured.

#### Activation: complexity argument (member 3)

This argument is recorded evidence from before stage 2 (#1576). Its "`main`" column describes the oxpinyin code before that stage; that code is removed. Its "Proposed" column is the design that stage 2 implements. The `LibraryHistory` paragraphs above state where the current code differs from "Proposed".

Dimensions, per library unless stated. `F` is the file size, `I` the item
area, `S` the offset slots (offset array `4S` bytes), `L` the journal bytes,
`R` its records, `k` the distinct slots a replay or training touches, `r ≤ R`
the records that change a slot offset (REMOVE, ADD, a MODIFY longer than the
stored item), `p` the distinct pages written, `Lb ≤ 15` the loaded
libraries, and `P` the platform page size. `F ≈ I + 4S + 25`. "Shared" pages are file-backed page-cache pages;
"private" are copy-on-write pages; "heap" is `malloc`/`Vec`.

**What each design holds per loaded system or addon library.**

| | Pin (`memory_chunk.h:475-524`, `phrase_index.cpp:301-330`) | `main` | Proposed |
|---|---|---|---|
| Mappings | one `MAP_PRIVATE` `PROT_READ\|PROT_WRITE` of `F`, freed with its sub-index | an immutable `map_copy_read_only` original, plus a second `map_copy` replay view when a journal exists (`phrase_library.rs:259,283`; `replay.rs:391`) | one `map_copy` of `F` |
| Descriptor | closed after `mmap` (`memory_chunk.h:523`) | retained for the library's life (`MappedFile.file`) | closed by `finish` on Unix; on Windows the memmap2 crate keeps a duplicated handle until the mapping drops (0.9.12 `src/windows.rs:203-225`), so "no retained handle" is a Unix-only claim |
| Journal state | in place; sub-chunk views | `touched`, `unigram_changes`, `offsets` side maps, decoded `system_overrides`, lazily decoded retained views | in place, plus a transient undo `Vec` (below) and the same `touched`/`unigram_changes` bookkeeping until its consumers change (below) |
| Session reads of a touched item | borrowed view, `O(1)` | `BTreeMap` lookup `O(log k)`, a decoded owned item, a clone in `SystemLibrary::item` | borrowed view, `O(1)` |

**1. Init and `*_load_phrase_library`** (`pinyin.cpp:250-294`,
`zhuyin.cpp:196-237`).
Pin: `open`, two `lseek`, two `read`, `mmap`, checksum `O(F)` over the mapped
bytes (`memory_chunk.h:514`), `close`; `SubPhraseIndex::load` `O(1)` (four word reads and
three separator probes). Log: read `O(L)` into the heap with its checksum.
Merge `O(R)` record steps; each step is `O(1)` slot lookup plus a `memcmp`
bounded by the item; an in-place MODIFY writes a private page. Time
`O(F + L + Σ item bytes touched)`. Space: shared `≤ F` (the checksum touches
every page), private `p·P`, heap `L`.
`main`: the same open and checksum `O(F)` over the read-only mapping, then a
second mapping `O(1)` syscall, the same merge, then `O(k log k)` sort and
decode of the touched slots. Space: shared pages are mapped twice (the process
RSS counts them twice), private `p·P` in the second mapping, heap `O(k)`
decoded items and side maps (until the consumers below change), one descriptor per library.
Proposed: the pin's sequence and costs. One mapping and no descriptor on Unix; the
decoded items stay until their consumers read the live buffer (below). Compared with the pin, only the undo vector
below is extra.

*Undo vector (replaces the side map).* Offsets and content are written
directly into the writable private mapping, or into the grown `Vec`, with the
pin's logical operations: `O(1)` offset access, no `BTreeMap`. Before each
mutation of a mapped byte, the replay pushes one typed undo entry onto a
`Vec`: the old 4-byte offset word (slot and value), or the destination range
of an in-place content write. The old content bytes are not copied: the entry
borrows the journal record's checked old payload from the log buffer the
replay already holds. A write into a grown `Vec` needs no entry because
undoing discards that `Vec`. Normal completion, a mismatch stop or an
unknown-record stop drops the undo `Vec` and keeps every applied change,
offsets and total frequency included, as `SubPhraseIndex::merge` does
(`phrase_index.cpp:439-519`, which returns early at a mismatch with the prefix
applied). Only when the replay reaches the payload over-read are the entries
applied in reverse, the original total restored from a scalar saved at the
start, and any grown buffer dropped; the library keeps its pre-replay content.
Space: the undo `Vec` is `O(R)` entries of constant size, separate from the
`O(L)` journal heap. Every record occupies at least eight bytes of the log (a
four-byte type word and a four-byte token, `phrase_index_logger.h:233-241`), so
`R ≤ L/8` and the total space order is `O(L)`, the pin's. Time `O(1)` per
record on the success path; the reversal is `O(Σ range bytes)` of the applied
entries and runs only on the over-read. An optional header-only framing walk
may decide whether to record entries at all (a log whose walk finds no
over-read tail records none). It never decides the outcome, because the walk
cannot know where a mismatch will stop the merge. It is left out: it costs an
extra `O(R)` pass on every replay to save `O(R)` entries only when the log has
no over-read tail.

*Reverse-order undo restores the mapped bytes and offsets (source proof).*
Each entry restores the exact bytes its destination range held immediately
before its write, so applying the entries in reverse restores the state before
the first write (induction: after undoing entries `n..k` the state equals the
state before entry `k`). The entry's saved bytes equal those bytes in every
reachable case:

- *In-place MODIFY*: the apply compares the stored item (`len` bytes) with the
  record's old payload and writes `new.len() ≤ len` bytes at its offset, so the
  destination is a prefix of the checked range and the saved bytes are
  `old[..new.len()]` (`replay.rs` `modify`, `phrase_index.cpp:485-501`).
- *Repeated MODIFY of one token*: the second compare sees the first write's
  result, so its saved bytes are the post-first-write bytes; the reverse
  application restores them, then the first entry restores the original.
- *Overlapping writes*: the argument is over byte ranges, not tokens, so two
  items that share bytes undo correctly.
- *Relocated items* (a longer MODIFY, REMOVE then ADD): the content append goes
  to the grown `Vec`; the only mapped mutation is the slot's offset word, whose
  old value the entry holds. REMOVE of a mismatching item still removes (the
  pin removes first), and its offset entry is pushed before the write.
- *Stale-item writes* (row 107's absent-MODIFY path): the stale compare reads
  the stale place (`stale_read`) and requires it equal to the old payload before
  writing; the destination is a prefix of that place. A write into the default
  six-byte stale buffer is not library state and has no entry. A `Shipped`
  stale place addresses the mapping, also after growth (the pin's stale
  pointer addresses the old mapping, still mapped), and has an entry. An
  `Owned` place addresses the `Vec`, which the reversal discards.
- *Offset-array growth*: an offset write past the array's end copies the
  array into a zero-filled `Vec` of `cursize + extra` bytes. Writes before the
  copy have entries on the mapping; writes after it go to the `Vec`; reversal
  drops the `Vec`, so the mapping's offsets return to the original.
- *Content growth that moves content from the mapping to the `Vec`*: the copy
  takes the mapping's current bytes at that point, and every later content
  write goes to the `Vec`, except a `Shipped` stale write. Earlier writes have
  mapping entries. Reversal restores the mapping and drops the `Vec`, and the
  logical size, capacity and `appends` count return to the saved scalars.

No reachable case lacks an entry. Nothing here needs a second mapping or a
copy of payload bytes.

*Logical capacity.* The pin's growth is `malloc(cursize+extra)` and `memset`
on the first copy, then `realloc(max(2·capacity, cursize+extra))` and `memset`
of the tail (`memory_chunk.h:102-130`). The reached-payload outcome reads
`capacity()`, so the logical `capacity` and `data_end`, and the zero-filled
extension, are tracked in their own fields as `Content` does today, and a
`Vec`'s own capacity is never consulted. The tracked bytes beyond the pin's
`m_data_end` read as zero.

*Bookkeeping consumers (traced).* `touched` and `unigram_changes` do not go
away with the second mapping. `Merge::finish` turns `touched` into typed
`system_overrides`, which the save diffs and
`store_libpinyin.rs:337,405,530,636` and `store_arena.rs:556` seed into the
session store; `finish_views` retains them in `ReplayedLibrary`, which
`system_items.rs:57,97` and `store_libpinyin.rs:356,572,671` read through
`slots`, `add_candidates` and `unigram_slots`. Those consumers decide which
store rows the replay seeds, and they compare against the pristine original
bytes. The `O(k)` bookkeeping and the decoded items are removed only if the
consumers read the live buffer instead; until then they stay, and the claim in
the table that decoded items disappear holds only for that change.

**2. Reads during guess, training and export.** Pin: `get_phrase_item` is two
bounds-checked `get_content` reads and a `set_chunk` view, `O(1)`, no copy
(`phrase_index.cpp:180-206`). Training writes in place: `O(item)` plus one
private-page fault the first time a page is written (`P` bytes, shared to
private). `main`: a read of a touched item is `O(log k)`; first training of a
system token decodes it into `system_items` (`store_arena.rs` `arena_train`),
`O(item)` heap, and `SystemLibrary::item` clones. Proposed: the pin's read and
write. Time better than `main` and equal to the pin. Space: the pin's page
granularity. Where training touches few tokens in widely spread pages, a
private page (`P` bytes) is larger than `main`'s decoded item (about the
item's bytes plus a map node). That is worse than `main` and equal to the pin;
bounded above by `F` for the whole library.

**3. First growth.** Pin: an append or an offset beyond the array end reaches
`ensure_has_more_space` with `m_free_func != free`: `malloc(cursize+extra)`,
`memset`, `memmove` of the whole current sub-chunk, then the view is
abandoned (`memory_chunk.h:102-123`). Time `O(I + extra)` for the content, and
`O(4S)` for the offset array if a slot lies beyond it; space heap `I + extra`
(and `4S + extra`), and the copy touches every shared page of the area.
`main`: replay's `Content::append` does the same copy (`to_vec`, `replay.rs`
`append`); training never grows a system library. Proposed: copy the whole
current content into a contiguous `Vec` with the pin's capacity rule
(`cursize+extra`, then `max(2·capacity, cursize+extra)` below). Equal to the
pin, equal to `main`.

**4. Later growth.** Pin: `realloc` to `max(2·capacity, cursize+extra)` and
`memset` the new tail (`memory_chunk.h:127-130`): amortised `O(1)` per appended byte.
Capacity is at most twice the largest requested extent, not twice the original
content size. Proposed: the same rule and the same capacity, which the
reached-payload over-read outcome already depends on (`Content::capacity`).
Equal. `main`'s arena (`UserLibrary`) is a different structure for user
libraries and is not changed.

**5. Save, `mask_out` and reload remaps.**
*Save* (`pinyin.cpp:952-959`, `zhuyin.cpp:585-592`). Pin: per library, a fresh
mapping `O(F)` for its checksum, then `diff` (`phrase_index.cpp:271-285`,
`SubPhraseIndex::diff`, `:392-430`): a walk over `[1, max(range_end))`, two `O(1)`
`get_phrase_item` per slot and one `memcmp` of equal-sized items, so
`O(S + Σ equal-size item bytes)`. The log chunk is `O(changed bytes)` heap. The
fresh mapping is released when `old_sub_phrases` goes out of scope
(`~SubPhraseIndex` → `reset` → `delete m_chunk`). Peak per library: live state
plus the fresh shared mapping.
`main`: fresh mapping and checksum `O(F)`, `byte_identical` against the
retained original `O(F)`; when equal the diff runs only over `k` overrides;
when different `diff_records_against` walks the union of both sides' items.
Proposed: the pin's `diff` against the fresh mapping, with no retained
original to compare against (the live mapping holds replay and training
writes). Time equals the pin. Against `main`'s unchanged-file fast path it is
the same order `O(F)` with a larger constant (a slot walk and per-item
`memcmp` against a `memcmp` of two regions plus `k` records). Space: the fresh
mapping and the log only. Not both worse than the pin.
*`mask_out` and reload* (`pinyin.cpp:1260-1268`, `zhuyin.cpp:795-803`). Pin:
per library `mmap` and checksum `O(F)`, `load` replaces the sub-index (the old
mapping and its heap copies are freed after the new mapping exists), the
log is read `O(L)` and merged. Peak holds the old state and the new shared
mapping together. Proposed: the same order, mapping and validating the new
chunk before releasing the old one. `main` replaces its original and its
replay view as well. Equal to the pin.
*Failed remap* (row 76, "stop"): the pin's failed `mmap` leaves an empty chunk
that `SubPhraseIndex::load` dereferences. The proposed save stops at that
library and answers `false`. No cost path; filesystem changes before it stay.

**6. Replay rollback.** The pin has none: a record whose payload runs past
the journal's end reads past the allocation (the class (b) payload over-read),
so the pin and the proposed design do not do comparable work there. Row 107 is
not a rollback trigger: a record that reaches an unreadable or out-of-bounds
item answers "no item" and the other changes of the replay are kept.
`main` rolls back by dropping the private replay view (`O(1)`, but only because
the second mapping exists on every successful replay).
Proposed: the undo vector of item 1, applied only when the replay reaches the
payload over-read. The entries are applied in reverse (an offset entry rewrites
its old word; a content entry rewrites its destination range from the
journal's checked old payload, borrowed from the log buffer already held), the
original total frequency is restored from the scalar saved at the start, and
any grown buffer is dropped. Time `O(Σ range bytes of the applied entries)`,
only when a log is refused. Space `O(R)` constant-size entries recorded on
every replay (a mapped-byte mutation records one), dropped when the replay
ends; this is within the journal's `O(L)` order because `R ≤ L/8`. The success
path pays the `O(1)` push per mutation and nothing else. The outcome stays
`main`'s: the log is skipped whole and the library keeps its pre-replay
content. A mismatch stop or an unknown-record stop is not a rollback: it drops
the entries and keeps every applied change.

**7. Fini.** Pin: `~SubPhraseIndex` → one `munmap(header + capacity)` per
library and a `free` of any grown copy. Proposed: one `Drop` of the mapping
(`munmap` of the whole mapping, row 109) and the `Vec`. `main` also drops the
second mapping and the descriptor. `O(Lb)` syscalls in all three.

**Resident and peak memory per library.**

| | Shared (file) | Private (copy-on-write) | Heap |
|---|---|---|---|
| Pin, steady | `≤ F` | `p·P` | grown copies `≤ 2(H_I + H_S)`; log `L` while merging |
| `main`, steady | `≤ 2F` as counted by RSS (two mappings) | `p·P` | `O(k)` decoded items and maps, one descriptor |
| Proposed, steady | `≤ F` | `p·P` | as the pin; transient `O(R)` undo entries while replaying, within the `O(L)` journal order |
| Pin peak (save) | `≤ 2F` (live and fresh) | `p·P` | log `O(changed)` |
| Proposed peak (save, reload) | same | same | same |

`H_I` and `H_S` are the largest logical extents requested so far for the
item content and offset array, including their original extents. Each grown
capacity is at most twice its corresponding high-water extent: on growth, the
old capacity is below the requested extent, and the new capacity is the larger
of twice the old capacity and that extent (`memory_chunk.h:102-130`). A large
append or offset extension can make these extents exceed `I` or `4S`. This is
a steady-capacity bound; a moving `realloc` can transiently hold both the old
and new allocations.

Page-cache pages are counted once physically in all designs; the RSS counts
differ only where one process maps a file twice. A file whose size passes the
pin's length check but wraps `guint32 data_len` maps `F` bytes in the pin and
in the proposed design. `main` maps only the checked extent. The mapping's
virtual size is therefore larger than `main`'s for those files and resident
size is unchanged (only touched pages are resident). The pin leaves the rest of
that mapping unreleased; the proposed design releases it (row 109,
POLICY-ACCEPTED).

**Where the proposed design is worse.**

- Than the pin: space only, transiently, on the replay path: the `O(R)` undo
  vector (item 1), which stays within the pin's `O(L)` order. No other path is worse on either axis.
- Than `main`: the unchanged-file save fast path becomes the pin's full diff
  (same order, larger constant); first training of a sparsely touched page
  costs a private page rather than a decoded item; virtual size of wrapped
  sparse files. Time and space are not both worse than the pin anywhere.

**Maintainer decision (2026-11-02 UTC).** Option (b), a pre-walk, is rejected:
`merge_log` and `SubPhraseIndex::merge` stop at a mismatch with the applied
prefix kept, so a pre-walk that found an over-read tail would discard a prefix
the pin keeps. Option (a) is approved with the undo vector above instead of a
`BTreeMap`.

The later coherent transition must remove retained originals, use one
borrowed live owner for replay/training/reads, copy the whole current
content to a contiguous Vec on first growth (`memory_chunk.h:102-123`),
and remap at the pin's save/mask/load points only. Direct replay uses
failure-only reconstruction from checked journal old-payload bytes for
the existing reached-payload-over-read rollback. The new class (b) safe
outcomes for rows 107 and 76 are recorded in
[compatibility policy](compatibility-policy.md); their production changes
and real-journal / failed-save-and-retry regressions belong to that
transition. They are not parity claims or member-1 gate results.

No extra replay mapping is approved. The later 2026-11-03 ruling on
trailing bytes is recorded below. Mapping setup is O(1) apart from the OS mapping; the caller's
checksum remains linear in interpreted payload bytes. There is no
production runtime-cost change in this preparation.

### Mapping lifetime ruling

Maintainer ruling recorded 2026-11-01 UTC (`date -u`).

**Maintainer ruling: external API misuse.** Where the pin calls an external library or system API incorrectly, by passing an argument that contradicts the pin's own state or the API's documented use, oxpinyin uses the API correctly and does not reproduce the consequence of the misuse. Each such case is recorded with its pin citation and its affected cases marked POLICY-ACCEPTED. This covers external calls only. Defects in the pin's own logic are still reproduced bug for bug unless separately ruled.

**First application, mapping lifetime (row 109).** `MemoryChunk::mmap` maps `file_size` bytes but does not keep that length. `freemem` (`src/include/memory_chunk.h:72-78` at `074a2219`) passes `munmap` a length recomputed from the chunk, `header + capacity()`. That length is short of the mapping when `guint32 data_len = file_size - header` wraps. A partial `munmap` is a valid call. The misuse is the pin's: it passes a length that contradicts the mapping it made. So the rest of that mapping is never released, which is a lifetime defect. oxpinyin uses the memmap2 crate, whose `Drop` releases exactly what was mapped. For every file whose size passes the pin's length check without wrapping, this is the pin's own `munmap(base, file_size)`. oxpinyin uses no `ManuallyDrop`, hand-written `libc::munmap` or other means of leaving part of a mapping behind.

## Stage 1 rulings

Maintainer rulings, 2026-10-28 UTC, recorded 2026-10-28 17:57 UTC
(`date -u`), for stage 1:

- **D1, λ.** The scanned `f32` is the only source of λ. The decoder's
  rational routing value is derived from its shortest round-trippable
  decimal (`Lambda::from_f32`); the existing rational calculation stays
  where the whole computation fits in `u128`, and the existing raw-float
  calculation takes over on overflow. A finite pin score never becomes
  `UNKNOWN_COST` or a missing step because `u128` overflowed. Tested with
  `5e-1`, `1e-30` and the smallest positive `f32` in a nonzero bigram
  context. **Real-table sentence and phrase scoring does not use this
  routing (#1406):** these paths use the scanned `f32` directly. The pre-frequency
  fallback scorer still uses the rational routing and overflow fallback;
  #1378 PR C retains them to preserve that scorer's behavior. See
  the reachability inventory.
- **D2, diagnostics.** `table_info.cpp`'s three `fprintf(stderr)` calls
  (`:201`, `:332`, `:382`) move in stage 1 with their native I/O
  sequences: exact calls, bytes, channel and order. A context without a
  user dir makes the same calls on `""`, as the pin's
  `g_build_filename(NULL, "user.conf", NULL)` does.
- **D3, `table.conf`.** Every `table.conf` load failure follows the pin,
  ENOENT included: init fails with the pin's diagnostics (`open %s
  failed.` from the load, then `load %s failed!`). The
  `TableConf::stock()` fallback is removed from that path. This settles
  open question 2 below by ruling.
- **D4, marker save.** `check_format` ignores the marker save's result
  and continues (`pinyin.cpp:187`); no extra `oxpinyin:` line.
- **Order.** The marker is written before the wipe and unlinks, as the
  pin does (`pinyin.cpp:187`, `:194-215`).

Stage 1 also fixes #1350: the negative-overflow `table.conf` cases and
the `open-counter-diff` seeds that the strict Rust grammar refused are
read by the host scanner.

### Native text-mode line endings

Recorded 2026-10-29 UTC. Under constitution §6 and #1303, `user.conf`
line endings are **platform-dependent native-host behaviour**. At
`074a2219`, `UserTableInfo::save` opens with `fopen(filename, "w")`
(`src/storage/table_info.cpp:380`) and prints four newline-terminated
directives with `fprintf` (`:386-390`). `UserTableInfo::load` opens with
`fopen(filename, "r")` (`:330`) and scans them with `fscanf`
(`:338-357`). These pinned blobs were read directly from a libpinyin
clone and verified against that commit.

The host C runtime therefore writes LF on Linux/macOS and CRLF on
Windows, where text-mode input translates CRLF back to LF. A Windows
build of the pin follows the same text-mode round trip. Under the
2026-10-28 external-I/O ruling, the boundary keeps the pin's `"w"` and
`"r"` calls, as `libc::fopen` with the same mode strings. It does not
force binary mode or normalize the written bytes.
`user_files::tests::user_conf_round_trips_and_conforms` and
`config::tests::the_writer_prints_the_four_lines` assert the exact
host-native bytes and read the saved file through the boundary's
`fscanf`.
The former also checks that the reloaded marker conforms. The LF text
returned by `UserTableInfo::to_text` remains a fixture composer.

Native scanning also governs malformed identity bytes. Microsoft UCRT's
[`input_adapter_character_source`](https://github.com/huangqinjin/ucrt/blob/d6e817a4cc90f6f1fe54f8a0aa4af4fff0bb647d/inc/corecrt_internal_strtox.h#L1956-L1980)
(Microsoft source read from that mirror) narrows an input byte to `char`
and skips pushback when it compares equal to EOF. Its integer parser
pushes back the first non-digit (`parse_integer`, same file, `:301-324`).
With signed `char`, `0xFF` after the binary version is consumed instead
of restored; the following whitespace directive and model directive can
then succeed. Windows run
[40853563512](https://github.com/shenghaoc/oxpinyin/actions/runs/40853563512/job/123378779863)
at `2e80cb49` confirmed that this marker kept the profile. This is
native-host behaviour, not permission to alter the pin's scanner calls
(`table_info.cpp:338-348`).
`persistence::tests::stray_identity_bytes_follow_the_native_scanner`
keeps the `0xFF` case and checks both facade lifecycles against the
explicit native-host result: the marker conforms on MSVC Windows and
is refused on Linux/macOS, where the non-digit remains for the next
directive. The test asserts that scanner result and the corresponding
profile retention or wipe. A separate `0xFE` case must fail
the directive and wipe the profile; both bytes are invalid UTF-8, so
the original protection against lossy decoding remains covered.

## How to read the inventory

**EQUIVALENT** means both implementation sources establish the same
observable result for every input in the explicitly stated domain.
**NOT EQUIVALENT** names a distinguishing input or condition.
**UNSETTLED** means the proof is missing, including where no current
counterpart exists; it is treated as not equivalent. Equivalence of a
single primitive does not establish equivalence of its enclosing
reader, writer, path conversion or error handling. All verdicts describe
the inspected sources, not a newly executed result.

The pin has no direct `sscanf`, `fread`, `fwrite`, `getenv`, `g_getenv`,
`strto*`, `truncate`, or `ftruncate` call in the scoped `src/` and
`utils/` source. It calls `g_ascii_strtoull` directly. Called libraries
can make more calls internally. This is an inventory of the pin's explicit
calls and the identified library effects. It does not claim proof of all
transitive runtime/environment reads of every dependency. Cwd, file type and permissions, umask, descriptor state,
signal interruption, locale and native library versions affect the calls.

## Input inventory

| Input and exact pin call | Current oxpinyin item and candidate | Verdict and first difference or limit |
| --- | --- | --- |
| `table.conf`: `SystemTableInfo2::load` calls `fopen(filename, "r")`; `fscanf` formats `"binary format version:%d\n"`, `"model data version:%d\n"`, `"lambda parameter:%f\n"`, `"source table format:%255s\n"`, `"database format:%255s\n"`, and `"%255s %255s %255s %255s %255s %255s\n"`; `strcmp` maps tokens; `fclose`. `src/storage/table_info.cpp:194-294`. | `oxpinyin-data/src/table_info.rs` `TableConf::read`, `TableConf::parse`, `ConfigLines`; `oxpinyin-runtime/src/lib.rs` `read_conf_file`. Rust whole-file read, line split and conversions. **Stage 1:** `oxpinyin-input` `config::SystemTableInfoReader` makes these calls; `TableConf::load` keeps the mappings and refusals. | **NOT EQUIVALENT.** A collapsed one-line header can be consumed by scanner whitespace but is rejected by `ConfigLines`; a 256-byte field is cut by `%255s` but retained as one Rust word. A FIFO with no writer can block in the pin's open while `TableConf::read` rejects a non-regular file. Lines, words. |
| Lambda in that same stream: `fscanf(input, "lambda parameter:%f\n", &lambda)`, under `setlocale(LC_NUMERIC, "C")`; `table_info.cpp:197-224`. | `TableConf::parse` uses Rust `f32` parsing. `oxpinyin-data/src/table_conf.rs` has a separate line-only rational `read_table_conf_lambda`/`parse_table_conf_lambda`; `lm::set_lambda_from_table_conf` uses it. **Stage 1:** both facades and `lm::set_lambda_from_table_conf` share `TableConf::load`'s `fscanf("%f")`; the line-only reader is gone. | **NOT EQUIVALENT.** `0x1p-1` and `nan` are accepted by the pin scanner and refused by the Rust scanner; the line-only reader also omits preceding directives and scanner consumption. Use the host `fscanf` stream, not an isolated `strtof`. Finding; [glibc 2.41 scanner](https://github.com/bminor/glibc/blob/glibc-2.41/stdio-common/vfscanf-internal.c). |
| Addon slot: `to_index_of_addon_tables` calls `atoi`, returns `guint8`; `SystemTableInfo2::load` checks the resulting slot. `table_info.cpp:159-161,270-280`. | `table_info.rs` `TableConf::parse` uses Rust `usize` parsing and a range check. **Stage 1:** host `atoi`, then the pin's `guint8` cut and check. | **NOT EQUIVALENT.** `abc` and `256` reach slot zero at the pin but Rust refuses them. Use host `atoi` and the pin's subsequent conversion. Finding; [glibc 2.41 `atoi`](https://github.com/bminor/glibc/blob/glibc-2.41/stdlib/atoi.c). |
| `user.conf`: `UserTableInfo::load` calls `fopen("r")`, `fscanf` with `"binary format version:%d\n"`, `"model data version:%d\n"`, `"database format:%255s\n"`, `"open counter:%d\n"`, token `strcmp`, and `fclose`; `table_info.cpp:325-373`. | `oxpinyin-user/src/persistence.rs` `check_format`, `parse_user_conf`; `oxpinyin-data/src/user_files.rs` `UserTableInfo::parse`. Rust whole-file read and complete-field parsing. **Stage 1:** `config::UserTableInfoReader` makes these calls; `UserTableInfo::load` keeps the pin's `EOF`/`0`/`1` rules. | **NOT EQUIVALENT.** On the documented glibc reference, a version plus `2^32` can narrow back to a conforming value while Rust rejects it. `BerkeleyDB\0junk` matches the pin's C string and not Rust's full token. Decimals, token. |
| System/addon chunk: `MemoryChunk::mmap` calls `open(filename, O_RDONLY\|O_BINARY)`, `lseek(fd, 0, SEEK_END)`, `lseek(fd, 0, SEEK_SET)`, two 4-byte `read`s, `mmap(NULL, file_size, PROT_READ\|PROT_WRITE, MAP_PRIVATE, fd, 0)`, and `close`; it calls `munmap` on checksum failure. `src/include/memory_chunk.h:475-524`; `pinyin.cpp:250-260,284-294,952-959,1260-1268`; `zhuyin.cpp:196-203,230-237,585-592,795-803`. | `oxpinyin-data/src/phrase_library.rs` validates through `oxpinyin-input::chunk::{MappingSource, PendingMapping, Mapping}`. | **MAPPING ACTIVE.** The input boundary opens and seeks, reads both words, maps the whole file writable and private, and keeps the descriptor through checksum validation. `finish` closes it on Unix; memmap2 retains a duplicated Windows handle until drop. Existing file-type metadata prechecks in data remain; both paths reject a length unequal to the physical payload length narrowed modulo 2^32. |
| User chunks and journals, and system chunk fallback: `MemoryChunk::load` calls `open`, both `lseek`s, two 4-byte `read`s, one payload `read`, and `close`; `memory_chunk.h:416-463`. | `oxpinyin-input::chunk::load` shares the mapping source's open and seeks, makes the separate header reads and one payload read, and retains the descriptor through `oxpinyin-data::user_files::load_chunk_payload`'s checksum. User-library loads and journal replay, including reload and mask, use it. | **IMPLEMENTED (2026-11-03 UTC, #1576).** The returned byte count governs the checksum and chunk length on a nonnegative short read: a match keeps the shortened chunk, a mismatch frees, closes and answers false, with no retry. A negative read, including `EINTR`, takes the class (b) safe failure in policy row 114 (`pin_not_run`). The header `assert`s (`:433`, `:437`) stay as recorded. Windows uses `_wopen` under the platform-adaptation ruling. Both `load` and `mmap` reject a declared length unequal to `(physical_file_size - 8) as u32` after the short-header check (`memory_chunk.h:439-443,499-503`). One extra byte rejects; an excess that is a multiple of 2^32 passes and the declared extent is kept. **System chunk fallback: no call to connect.** The callers use `load()` only when `LIBPINYIN_USE_MMAP` is undefined; `memory_chunk.h:30-33` defines it whenever `HAVE_MMAP` is, and the oracle's `config.h:42` defines `HAVE_MMAP` ([row 76](compatibility-policy.md)). The `#else` branches at `pinyin.cpp:250-260,284-294,952-959,1260-1268` and `zhuyin.cpp:196-203,230-237,585-592,795-803` are not compiled; the oracle's `libpinyin.so` and `libzhuyin.so` carry `mmap %s failed!` and no `open %s failed!`. System chunks therefore always map on Linux. The pin's Windows heap path is replaced by memmap2 under the rulings above; no heap fallback is connected. |
| System, addon, punctuation and user DBMs: backend `attach`, `load_db`, open, cursor/get and close calls; e.g. `src/storage/chewing_large_table2_bdb.cpp:77-132`, `chewing_large_table2_kyotodb.cpp:77-122`, `chewing_large_table2_tkrzwdb.cpp:75-108`. Paths include `pinyin_index.bin`, `phrase_index.bin`, `bigram.db`, `punct.bin`, the addon pair and user indexes/bigram. | `oxpinyin-data/src/dict.rs` `SystemDictionary::open_with`, `AddonDictionary::attach_with`; `punct.rs` `PunctTable::open_optional`; user persistence loaders; `oxpinyin-store` backend adapters. | **UNSETTLED as an independent all-input proof.** The ruling states DBM backends already follow the native backend through store; this inventory does not independently prove every backend's operations. Missing-system-DBM behavior stays open. |
| `.table` and `punct.table`: `gen_binary_files` uses `fopen("r")`, loaders, `fseek(tablefile, 0L, SEEK_SET)` between three table passes and `fclose`; `utils/storage/gen_binary_files.cpp:36-105`. `table_text_read_pinyin_record`/`table_text_read_punct_record` use `fgets(line, sizeof(line), infile)` with a 4096-byte array, `strspn`, `strcspn`, `g_strlcpy`, and `g_ascii_strtoull(field, &end, 10)`; `src/storage/table_text_parser.h:34-121`. | At the inventory: `oxpinyin-datagen` (since deleted) `read_table_file`/`parse_table_line`, `read_punct_file`/`parse_punct_line`; Rust UTF-8 `read_to_string`, whitespace split and integer parses. Now `oxpinyin-utils` `gen_binary_files` (`storage/table_text.rs`) with the pin's `fgets` reader (stage 5). | **NOT EQUIVALENT** (for the deleted reader). A fifth field is ignored by the pin but rejected by datagen; a count `-0` is rejected by the pin's helper but accepted by the table parser. The pin splits long lines at `fgets`'s buffer and retains bytes Rust's UTF-8 read rejects. #926. |
| `gen_pinyin_table` source rows: `fopen(filename, "r")`, `fscanf(infile, "%1023s %1023s %u", ...)`, `fclose`; `utils/storage/gen_pinyin_table.cpp:138-157`. `gen_zhuyin_table` uses `std::ifstream`, `std::getline`, then `atoi(strs[1])`; `utils/storage/gen_zhuyin_table.cpp:141-157`. | No direct corresponding oxpinyin utility. Datagen consumes generated `.table` files, not these same utility inputs. | **UNSETTLED / GAP.** No Rust equivalent is established. The pin calls remain required when these tools are added. |
| Segmented corpus, interpolation and KMM text from stdin or files: `std::cin`/`std::ifstream`, `std::getline`, and `LineString::operator const gchar *` returning `c_str()`; `src/include/pinyin_utils.h:33-40`, `utils/segment/ngseg.cpp:107-111,187-194`, `utils/storage/import_interpolation.cpp:56-68,235-238`, and the utility sources listed below. | The utilities use `oxpinyin-input`'s `LineReader`, which reproduces `std::getline`'s `eofbit`/`failbit`, its tied-`stdout` flush and the `c_str()` view. | **EQUIVALENT** (the reference `LineReader` cases). [libstdc++ 14.2 `getline`](https://github.com/gcc-mirror/gcc/blob/releases/gcc-14.2.0/libstdc%2B%2B-v3/include/bits/basic_string.tcc#L905-L965). |
| Segmented tokens and tagged values: `g_strsplit_set(line, " \t", 2)`, `atoi` in `TAGLIB_PARSE_SEGMENTED_LINE`, `atoi` in `TAGLIB_GET_TOKEN`, and `atol` in `TAGLIB_GET_TAGVALUE`; `utils/utils_helper.h:27-69`, `utils/training/import_k_mixture_model.cpp:148-207`, `utils/storage/import_interpolation.cpp:135-182`. | `oxpinyin-input/src/tool.rs` `strsplit_set`/`atoi`/`atol` are the pin's own calls; `oxpinyin-utils/src/k_mixture_model/import.rs` and `src/generate.rs` use them. | **EQUIVALENT for the KMM tools.** |
| Tagged text and phrase Unicode: `split_line` calls `g_utf8_get_char`, `g_utf8_next_char`, `g_unichar_isspace`, `g_unichar_isgraph`; loaders use `g_utf8_to_ucs4`. `src/storage/tag_utility.cpp:118-170`, `src/storage/phrase_index.cpp:537-544`. | At the inventory: `oxpinyin-data` `interp_grammar.rs` `Tokens`, `is_graph`, and `oxpinyin-datagen`, both since deleted, used Rust strings and character properties. Now `oxpinyin-utils/src/utils_helper/taglib.rs` makes the GLib calls for every tool. | **NOT EQUIVALENT** for the deleted readers. A format character such as U+200B in an unquoted field is excluded by GLib's graph classification but not by Rust's `is_graph` approximation. Use the host GLib calls. [Grammar finding](interpolation2-grammar.md). |
| Utility argv/options: `g_option_context_parse(context, &argc, &argv, &error)`; e.g. `utils/storage/gen_binary_files.cpp:112-130`, `utils/segment/ngseg.cpp:111-139`. | At the inventory: KMM and `oxpinyin-datagen` binaries (since deleted) used `std::env::args` and hand-written parsing. Now `oxpinyin-input` `tool::parse_options`. | **UNSETTLED** (for the deleted parsers). Option names, grouping and error paths already differ; no all-input equivalence to [GLib `goption.c`](https://github.com/GNOME/glib/blob/2.84.4/glib/goption.c) was proved. Use the pin's GLib option operations for its interface. |
| Path building: pin calls `g_build_filename` for system, user, addon, staging and diagnostics, e.g. `src/pinyin.cpp:150-165,250-259,952-985,997-1017,1050-1121`; `src/zhuyin.cpp:104-119,196-203,585-687`. | `oxpinyin-input/src/path.rs` `build_filename`, `build_path`, `build_bytes`; `oxpinyin-data` and `oxpinyin-user` call those. | **DONE (stage 3, 2026-11-01 UTC).** `build_filename` is the one `g_build_filename` call, through `glib-sys`, with the pin's C arguments: `NULL` first element answers `""` and a `NULL` name the directory. The Rust `pin_join`, `join_bytes` and `FileName::under` are gone. Unix keeps the bytes; Windows takes GLib's UTF-8 name and its path behaviour is deferred (#1475). Cases `glib-922-*`, `init-system-*`, `table-conf-user-file-*`: MATCH on tkrzw. |
| Locale and environment: config load/save calls `setlocale(LC_NUMERIC, "C")` and, on the success path, `setlocale(LC_NUMERIC, locale)` using the returned pointer; `table_info.cpp:197,291,328,372,378,394`. Many utilities call `setlocale(LC_ALL, "")`, e.g. `ngseg.cpp:111`, `gen_binary_files.cpp:112`. Relative paths use process cwd. GLib diagnostics can depend on GLib environment settings. | The earlier facade-local `pin_table_info_locale` helpers call host libc, but are placed at facade entry points. **Stage 1:** those two files are gone; the six `setlocale` calls are made inside the `table.conf`/`user.conf` reader and writer at the pin's sites. | **UNSETTLED for the complete lifecycle.** One same-argument native `setlocale` invocation is the same call; the number and placement of invocations differ. glibc locale lookup consults `LC_ALL`, category variables, `LANG` and `LOCPATH` where applicable; GLib warning handling includes `G_DEBUG`. [glibc locale lookup](https://github.com/bminor/glibc/blob/glibc-2.41/locale/findlocale.c#L110-L124), [GLib messages](https://github.com/GNOME/glib/blob/2.84.4/glib/gmessages.c#L218-L225). |

The C++ `std::getline` sites are `utils/storage/gen_zhuyin_table.cpp:148`,
`import_interpolation.cpp:68`; `utils/segment/ngseg.cpp:188`,
`spseg.cpp:269`, `mergeseq.cpp:264`; `utils/training/gen_ngram.cpp:79`,
`gen_deleted_ngram.cpp:80`, `gen_k_mixture_model.cpp:74`,
`eval_correction_rate.cpp:177`, `import_k_mixture_model.cpp:69`, and
`k_mixture_model_to_interpolation.cpp:52`. Named input-file stream opens
are in the corresponding utilities, including `gen_k_mixture_model.cpp:352`
and `eval_correction_rate.cpp:166`; otherwise they consume `std::cin`.
The diagnostic and argv sites in the output catalog identify the
associated tool entry points.

## Output inventory

| Output and exact pin call | Current oxpinyin item and candidate | Verdict and first difference or limit |
| --- | --- | --- |
| `user.conf`: `UserTableInfo::save` calls `fopen(filename, "w")`, `fprintf` with `"binary format version:%d\n"`, `"model data version:%d\n"`, `"database format:%s\n"`, `"open counter:%d\n"`, then `fclose`. It checks the open, not subsequent write/close results. `table_info.cpp:377-395`. | `oxpinyin-user/src/persistence.rs` `write_marker`; `oxpinyin-data/src/user_files.rs` `UserTableInfo::to_text`; `oxpinyin-facade/src/context.rs` `ContextCore::save_user_with_layout_faults`. Rust `std::fs::write`. **Stage 1:** `config::save_user_table_info` makes the pin's `fopen`/`fprintf`/`fclose` calls and checks only the open. | **NOT EQUIVALENT.** If `user.conf` points at `/dev/full` after initialization, opening can succeed while the subsequent output fails: the pin ignores that failure, while Rust reports the `std::fs::write` failure and emits the marker-write diagnostic. Use the exact sequence and check only the pin's results. |
| Chunk creation/truncation/save: `MemoryChunk::save` calls `open(filename, O_CREAT\|O_WRONLY\|O_TRUNC\|O_BINARY, 0644)`, `write(fd, &length, sizeof(guint32))`, `write(fd, &checksum, sizeof(guint32))`, one `write(fd, begin(), size())`, `g_fsync(fd)`, and `close(fd)`. A short/failed payload closes and returns before sync. `memory_chunk.h:536-557`. | `oxpinyin-input/src/chunk.rs` `save`; `oxpinyin-data/src/chunk_format.rs` `build_memory_chunk` supplies the bytes. | **DONE (stage 3, 2026-11-01 UTC).** `chunk::save` makes the pin's `libc::open`, one `write` per header word, one payload `write`, `libc::fsync` and `libc::close`; the header's assert is reproduced as an abort (row 77) and a short payload is the pin's soft `false`. Cases `abort-save-chunk-header-*`, `abort-zhuyin-save-chunk-header-*`: POLICY-ACCEPTED on tkrzw. Windows uses libc::wopen with the same flags and mode, one CRT write per header and payload, libc::commit, then close. Rust paths reach encode_wide directly. |
| Chunk sync: `g_fsync(fd)` at `memory_chunk.h:555`. Ordinary Unix GLib headers macro-expand it to `fsync`; Windows GLib's function calls `_commit`. [GLib header](https://github.com/GNOME/glib/blob/2.84.4/glib/gstdio.h#L60-L84), [wrapper](https://github.com/GNOME/glib/blob/2.84.4/glib/gstdio.c#L1666-L1702). | `oxpinyin-input/src/chunk.rs` `save` calls `libc::fsync` once and ignores its answer. | **DONE on Unix (stage 3, 2026-11-01 UTC).** The ordinary Unix macro is `fsync`, with no Rust retry and no macOS `F_FULLFSYNC`. Windows calls libc::commit once and ignores its result. |
| Mapping release: `MemoryChunk::freemem` calls `munmap(m_data_begin - header, header + capacity())`; `memory_chunk.h:72-78`. A failed checksum instead calls `munmap(data - header, file_size)` at `:516`. | The input `Mapping` owns the whole-file writable private mapping; `PendingMapping` drops a rejected mapping before its descriptor. | **IMPLEMENTED:** memmap2 releases the whole mapping; the wrapped-size residual is POLICY-ACCEPTED under the [mapping lifetime ruling](#mapping-lifetime-ruling). |
| Profile cleanup: `_clean_user_files` and `check_format` call plain `unlink(filename)` for selected user files and fixed DBM names; `pinyin.cpp:150-215`, `zhuyin.cpp:104-159`. | `oxpinyin-user/src/persistence.rs` `clean_user_files` calls `oxpinyin-input` `path::unlink` (`libc::unlink`). | **DONE (stage 3, 2026-11-01 UTC).** The order is the pin's: the default rows' user files by sub-index, libpinyin's addon rows (`pinyin.cpp:199-200`), then `USER_PINYIN_INDEX`, `USER_PHRASE_INDEX` and `USER_BIGRAM`; libzhuyin stops after the default rows. Every result is ignored. Cases `table-conf-addon-cleanup{,-zhuyin}`, `table-conf-user-library-named-user-conf{,-zhuyin}`: MATCH on tkrzw. |
| Save staging/rename: pinyin unlinks the three DBM staging names, writes chunks/DBMs, then calls `g_rename(tmppathname, chunkpathname)` or `g_rename(tmpfilename, filename)` for each; `pinyin.cpp:940-1128`. Zhuyin interleaves write and rename; `zhuyin.cpp:554-689`. Each failed rename calls `fprintf(stderr, "rename %s to %s failed.\n", ...)`. | `oxpinyin-user/src/persistence.rs` `stage_chunk`, `stage_dbm`, `stage_user_bigram`, `save_reporting`, `Staged::finish`; `oxpinyin-input` `chunk::save`, `path::rename`, `path::unlink`; facade diagnostics. | **DONE on Unix (stage 3, 2026-11-01 UTC).** Pinyin keeps its two passes and zhuyin its interleaved write/rename; the three DBM staging names are unlinked before the write (the bigram too), and a failed save leaves every staged `.tmp` where the pin leaves it. `g_rename` is `libc::rename` on Unix; Windows calls GLib's `g_rename` and remains #1475. Cases `save-929-*`, `stderr-save-*`: MATCH or POLICY-ACCEPTED on tkrzw. Windows adaptation (2026-11-08 UTC, under the 2026-11-03 UTC ruling in `compatibility-policy.md`): before `g_rename`, `path::rename` refuses a directory over an existing file (`ENOTDIR`) and a file over an existing directory (`EISDIR`), the refusals `rename(2)` makes and `MoveFileExW` does not, so a save whose `.tmp` directory blocks the final file fails and prints `rename %s to %s failed.` as on Linux. The `oxpinyin-input` unit tests `a_directory_is_not_renamed_over_a_file` and `a_file_is_not_renamed_over_a_directory` hold the two refusals on every OS; the contract cases `save-929-rename-user_bigram.db.tmp-pinyin`, `save-929-rename-user_bigram.db.tmp-zhuyin`, `stderr-save-one-tmp-blocked` and `stderr-save-one-tmp-blocked-zhuyin` (recorded evidence) record the pin's observations, and `failed_renames_report_exact_order_and_preserve_destinations` (`crates/oxpinyin-user/tests/save_semantics_929.rs`) holds the reporting save's rename order. The reporting save carries on as the pin does; the strict Rust-only save that answered its first rename failure was retired (2026-11-09 UTC). |
| Library diagnostics: `fprintf(stderr, "open %s failed.\n", ...)` for system config, `"open %s failed."` for user config, `"write %s failed.\n"` for marker open, `"load %s failed!\n"`, `"mmap %s failed!\n"`, `"open %s failed!\n"` for loads, and the rename format above. `table_info.cpp:199-202,330-333,380-383`; `pinyin.cpp:255-259,289-293,338,955-959,1061,1080,1096,1110,1123,1264-1268`; `zhuyin.cpp:199-203,233-237,282,588-592,611,637,657,674,689,799-803`. | `crates/oxpinyin-input/src/native.rs` `emit`, `mmap_failed`. Rust assembles bytes and uses `stderr().write_all`. **Stage 1:** a line that shares a failure sequence with a boundary line is the boundary's `fprintf` (`oxpinyin-data` `c_stderr`, `oxpinyin-input` `report_*`): `table_info.cpp`'s three lines (D2); in init, `load %s failed!` (`pinyin.cpp:338`, `zhuyin.cpp:282`) and the library loop's `mmap %s failed!` (`pinyin.cpp:256`, `:290`; `zhuyin.cpp:200`, `:234`), after `check_format`'s `user.conf` lines; in save, `mmap %s failed!` (`pinyin.cpp:956`, `zhuyin.cpp:589`) and `rename %s to %s failed.`, before `mark_version`'s line. Fini writes only `mark_version`'s line. Left to stage 4 on `pin_stderr`: `mmap %s failed!` in `mask_out` (`pinyin.cpp:1265`, `zhuyin.cpp:800`) and in the library load calls (`pinyin.cpp:256`, `:290` through `:446` and `:477`; zhuyin's twins), whose calls write no boundary line. oxpinyin never writes `open %s failed!`, the non-mmap build's line. **Stage 4:** those remaining lines are the boundary's `fprintf` too (`oxpinyin-input` `diagnostic::report_mmap_failed`, through `oxpinyin-data` `c_stderr::mmap_failed`), so every library line goes to the host C library's `stderr`; since 2026-10-31 UTC each is `libc::fprintf` on the C library's own `stderr` object, declared per platform in `oxpinyin-input` `native.rs` (glibc and musl `stderr`, Apple and FreeBSD `__stderrp`, UCRT `__acrt_iob_func(2)`), not a new stream on descriptor 2; `pin_stderr` and its Rust-stderr writer are gone. The `SubPhraseIndex::load` criticals (`phrase_index.cpp:354-356`, `:361`) are made with the call the pin's `g_return_val_if_fail` expands to, `g_return_if_fail_warning(NULL, G_STRFUNC, #expr)` (oracle `SubPhraseIndex::load+492`, `+552`, `+576`, `+608` in both objects), not a preformatted `g_log`. Since 2026-10-31 UTC (#1297 decision D1) they are not made at all ([1297-layout-criticals](1297-layout-criticals.md)). | **UNSETTLED for all sinks.** Byte assembly on ordinary stderr does not prove C `FILE *` buffering, retries and error behavior equivalent. The `/dev/full` marker example above is a confirmed surrounding difference. |
| Backend DBM saves: BDB `unlink`, `db_create`, `tmp_db->open(..., DB_CREATE, 0600)`, copy, `tmp_db->sync(m_db, 0)`, `tmp_db->close(tmp_db, 0)` (`chewing_large_table2_bdb.cpp:135-159`, `phrase_large_table3_bdb.cpp:150-174`, `ngram_bdb.cpp:81-105`, `punct_table_bdb.cpp:111-135`). KC `unlink`, `dump_snapshot` (`chewing_large_table2_kyotodb.cpp:124-130`, `phrase_large_table3_kyotodb.cpp:139-145`, `ngram_kyotodb.cpp:82-91`, `punct_table_kyotodb.cpp:99-105`). Tkrzw `RemoveFile`, `Open(..., true, File::OPEN_DEFAULT)`, copy, `Synchronize(false)`, `Close` (`chewing_large_table2_tkrzwdb.cpp:111-127`, `phrase_large_table3_tkrzwdb.cpp:102-118`, `ngram_tkrzwdb.cpp:65-81`, `punct_table_tkrzwdb.cpp:96-109`). | `oxpinyin-store/src/bdb/mod.rs` `BdbUserBigramDb::save_db`; `kyotocabinet/mod.rs` `KcUserBigramDb::save_db`, `KcStore::write_user_index`/`write_user_bigram`; `tkrzw/mod.rs` `TkrzwUserBigramDb::save_db`; user persistence stages DBMs. | **UNSETTLED independently.** The ruling keeps DBM backends in store. BDB source passes `m_db` to the method selected from `tmp_db`, while `BdbUserBigramDb::save_db` calls `file.sync()` on the new handle. The external consequence remains an open confirmation question. KC `write_user_bigram` also documents a sibling-temp/rename sequence; do not infer its reachability in a facade save from its name alone. |
| Backend reset and maintenance: BDB invokes `m_db->sync(m_db, 0)` and `m_db->close(m_db, 0)` on its handles, including `chewing_large_table2_bdb.cpp:68,544`, `phrase_large_table3_bdb.cpp:80,412`, `ngram_bdb.cpp:41`, `punct_table_bdb.cpp:43`, `flexible_ngram_bdb.h:53`. Tkrzw uses `m_db->Synchronize(false)` and `Close()`, e.g. `chewing_large_table2_tkrzwdb.cpp:65,489`, `phrase_large_table3_tkrzwdb.cpp:52,312`, `punct_table_tkrzwdb.cpp:45`, `flexible_ngram_tkrzwdb.h:86`. KC closes its handles, e.g. `phrase_large_table3_kyotodb.cpp:79,132`. | Backend-specific `WriteStore`/`UserBigramDb` methods in `oxpinyin-store/src/{bdb,kyotocabinet,tkrzw}/mod.rs` and their FFI adapters. | **UNSETTLED as an independent all-input proof.** Backend calls remain in store; their exact source-side arguments, ordering and failure handling must be retained under the ruling. |
| Utility chunk files: `save_phrase_index` and `save_dictionary` call `MemoryChunk::save`; `utils/utils_helper.h:97-145`. | At the inventory: `oxpinyin-datagen` (since deleted) `write_chunk_file` used `std::fs::write`. Now the utilities save through `oxpinyin-input` `chunk::save`, the pin's calls. | **NOT EQUIVALENT** (for the deleted writer). With umask `000`, a new pin chunk is requested at mode `0644`, Rust's ordinary create at `0666`; the deleted writer also omitted the pin's chunk sync. |
| KMM working models: `FlexibleBigram::attach` opens/creates backend DBMs and later reads/writes/syncs through the selected backend; `src/storage/flexible_ngram_bdb.h:94-153,165-235,245-291`, `src/storage/flexible_ngram_kyotodb.h:129-180,192-272`, `src/storage/flexible_ngram_tkrzwdb.h:128-188,200-282`, `utils/training/gen_k_mixture_model.cpp:316-355`. | `oxpinyin-store/src/flexible_ngram.rs` `FlexibleBigram` opens the same hash contract (`create_hash`/`open_hash_read_only`), magic record, single-gram records and per-token header writes on all three backends, and walks `get_all_items` in the backend-native order; pin cross-reads are `oxpinyin-store/tests/flexible_bigram.rs`. `oxpinyin-utils/src/k_mixture_model/{database,records,validate,merge,prune,estimate,generate,import,export}.rs` carry the seven programs' operations on that layer; the text working model and the `oxpinyin-kmm` CLI are removed. | **PORTED (2026-10-31 UTC).** The utilities are drop-ins on all three backends; `tool-diff` on the tkrzw reference cell is 32 MATCH and 3 POLICY-ACCEPTED (the three source-derived asserts bind in [eio-kmm-tools](eio-kmm-tools.md)). A DBM file the tools write is read by the pin and vice versa; `oxpinyin-store/tests/flexible_bigram.rs` covers the container, and `crates/oxpinyin-utils/src/k_mixture_model/flexible_records.rs` the record API. |
| Utility text output/diagnostics: `fopen(outputfile, "w")`, `fprintf`, `fclose`, `printf`, `g_print`, `g_warning`, `perror`; individual formats and sites are catalogued below. | Training `tool_cli::write_output`, segment binaries, emitter/lambda/eval, KMM CLI use Rust formatting, `write_all` and `std::fs::write`. | **UNSETTLED generally.** Buffering, incremental output, errors, locale and platform text mode are not proved equivalent. Empty held-out/evaluation input is **NOT EQUIVALENT**: the pin prints the host C `%f` NaN spelling and Rust prints `NaN`. Finding. |

### Utility formatted calls

The following catalog groups identical calls but retains the pin's
function, formats and locations. Literal help and diagnostic strings at
the cited sites are part of the tool contract, including their case,
punctuation, whitespace and exit behavior. These paths use the output
verdict above; no matching call has been newly proved equivalent.

| Pin function/sites and `printf`/`fprintf`/GLib format | Current oxpinyin item |
| --- | --- |
| `ngseg.cpp:36,81-100,119-260`: `"Usage: ngseg [--generate-extra-enter]  [-o outputfile] [inputfile]\n"`, `"%s\n"`, `"%d %s\n"`, `"%d \n"`, `"Un-segmentable sentence encountered:%s\n"`, `"non-ucs4 characters encountered:%s.\n"`, `"too many arguments.\n"`, `"load table.conf failed.\n"`, `perror("open file failed")`; `fopen(outputfile, "w")`, `fclose(output)`. | `oxpinyin-utils` `segment::ngseg` (drop-in at stage 5; `print_help` is dead code and GLib prints `--help`). |
| `spseg.cpp:37,183-195,214-343`: `"Usage: spseg [--generate-extra-enter] [-o outputfile] [inputfile]\n"`, `"%d %s\n"`, `"%d \n"`, the non-UCS4/argument/config errors and `perror("open file failed")`; `fopen("w")`/`fclose`. | `oxpinyin-utils` `segment::spseg` (drop-in at stage 5). |
| `mergeseq.cpp:37,141-170,213-280`: `"Usage: mergeseq [-o outputfile] [inputfile]\n"`, `"%d %s\n"`, `"%s\n"`, argument/config errors and `perror("open file failed")`; `fopen("w")`/`fclose`. | `oxpinyin-utils` `segment::mergeseq` (drop-in at stage 5). |
| `export_interpolation.cpp:34-39,76-132`: `"\\data model interpolation\n"`, `"\\1-gram\n"`, `"\\2-gram\n"`, `"\\end\n"`, `"\\item %d %s count %ld\n"`, `"\\item %d %s %d %s count %d\n"`. | `oxpinyin-utils` `export_interpolation`. |
| `export_k_mixture_model.cpp:39-108`: `"no magic header in k mixture model.\n"`, `"\\data model \"k mixture model\" count %d N %d total_freq %d\n"`, section/end literals, `"\\item %d %s count %d freq %d\n"`, `"\\item %d %s %d %s count %d T %d N_n_0 %d n_1 %d Mr %d\n"`; `k_mixture_model_to_interpolation.cpp:62-170` uses interpolation section literals, `"\\item %d %s count %ld\n"`, `"\\item %d %s %d %s count %ld\n"`. | `oxpinyin-utils/src/k_mixture_model/export.rs` `Sink`, `print_array_headers`, `print_array_items`; `oxpinyin-input/src/tool.rs` `k_mixture_model` makes the pin's `fprintf` calls; `oxpinyin-utils/src/export_k_mixture_model.rs` parses the options and loads the cwd-relative tables. `k_mixture_model_to_interpolation`'s lines are the pin's calls in `oxpinyin-utils`. |
| `estimate_interpolation.cpp:129,139` and `estimate_k_mixture_model.cpp:141,152`: `"token:%d lambda:%f\n"`, `"average lambda:%f\n"`; `eval_correction_rate.cpp:119-121,168,203`: `"test sentence:%s\n"`, `"guessed sentence:%s\n"`, `"the result mis-matches.\n"`, `"Can't open file:%s\n"`, `"correction rate:%f\n"`. | `oxpinyin-utils` `estimate_interpolation` and `eval_correction_rate`; KMM `estimate::run`/`compute_interpolation` and `oxpinyin-input/src/tool.rs` `k_mixture_model::print_token_lambda`/`print_average_lambda` (the C library's `%f`, `-nan` included). |
| `gen_pinyin_table.cpp:28-33,106-114,138-157,169-215,261-325` and `gen_zhuyin_table.cpp:34-39,112-120,141-157,175-221,267-334`: help `"Usage: gen_pinyin_table -t <PHRASE_INDEX> \n-o <OUTPUTFILE> <FILE1> <FILE2> .. <FILEn>\n<OUTPUTFILE> the result output file\n<FILEi> input pinyin files\n<PHRASE_INDEX> phrase index identifier\n"`; `"nnodes: %d\n"`, `"Can't open file %s.\n"`, `"Can't write file %s.\n"`, `"Too long phrase:%s\t%s\t%d\n"`, `"Invalid pinyin:%s\t%s\t%d\n"`, `"Duplicate item: phrase:%s\tpinyin:%s\tfreq:%u\n"`, `"%s\t%s\t%d\t%d\n"`; zhuyin also `"wrong line format:%s\n"`. The zhuyin help itself says `gen_pinyin_table`. | **GAP:** neither generator has a direct utility counterpart. |
| `gen_binary_files.cpp:64,92,99,120,130`: `"open %s failed!\n"`, `g_print("option parsing failed:%s\n", error->message)`, `"load table.conf failed.\n"`. `utils_helper.h:87,112,137`: `"load %s failed!\n"`, `"save %s failed."`. `phrase_index.cpp:581`: `"FacadePhraseIndex::load_text:%s\t%s\n"`; `chewing_large_table2.cpp:145` and `chewing_large_table.cpp:699`: `"ChewingLargeTable2::load_text:%s\t%s\t%u\t%" G_GSIZE_FORMAT "\n"` and the corresponding `ChewingLargeTable::` format. `punct_table.cpp:102`: `"duplicated punctuations: %s\n"`. | Datagen `table::read_table_file`, `punct::read_punct_file`, `system::read_interpolation`, binary `write_chunk_file`; not every pin diagnostic is emitted. |
| Common utility option failures use `g_print("option parsing failed:%s\n", error->message)` after `g_option_context_parse`, e.g. `gen_binary_files.cpp:119-120`, `ngseg.cpp:118-119`, `import_k_mixture_model.cpp:268-269`. `taglib_read` uses `g_warning("un-expected tags:%s.\n", tmp)` and `g_warning("missed required tags: %s.\n", required_tag_str)` (`tag_utility.cpp:239,256`); `g_return_val_if_fail` sites at `:137,202,211,246` have GLib's own diagnostic. | `oxpinyin-input` `tool::parse_options` and `oxpinyin-utils` `utils_helper::taglib`, which make the pin's `g_print` and warnings; the check macros are plain Rust ([eio-tool-check-messages](eio-tool-check-messages.md)). The deleted `interp_grammar::read` skipped an unknown key/value without the pin's warning; GLib fatal-warning settings can affect the outcome. |
| `validate_k_mixture_model.cpp:25-168`: help `"Usage: validate_k_mixture_model <FILENAME>\n"`; errors `"no magic header in k mixture model.\n"`, `"word count in magic header is unexpected zero.\n"`, `"total freq in magic header is unexpected zero.\n"`, `"the word count doesn't match the total freq.\n"`, `"word count in magic header:%d\n"`, `"sum of word count in array headers:%d\n"`, `"the sum differs from word count.\n"`, `"total freq in magic header:%d\n"`, `"sum of freqs in array headers:%d\n"`, `"the total freq differs from sum of freqs.\n"`, `"in the array header of token %d:\n"`, `"word count is zero but has array items.\n"`, `"both word count and freq are unexpected zero.\n"`, `"word count in array header:%d\n"`, `"sum of word count in array items:%d\n"`, `"wrong arguments.\n"`, `"k mixture model validation failed.\n"`. | `oxpinyin-utils/src/k_mixture_model/validate.rs` `validate_unigram`/`validate_bigram`; `oxpinyin-utils/src/validate_k_mixture_model.rs` parses `argv` against the empty option table; `oxpinyin-input/src/tool.rs` `k_mixture_model` makes the pin's `fprintf` calls. |
| `prune_k_mixture_model.cpp:31,73-75,116-144,175`: help `"Usage: prune_k_mixture_model -k <INT> --CDF <DOUBLE>  <FILENAME>\n"`, `"some wrong possibility is encountered:%f.\n"`, `"k:%d N:%d WC:%d n_0:%d n_1:%d\n"`, `"wrong arguments.\n"`, `"no magic header in k mixture model.\n"`, progress `"*"`, `"\n"`. `gen_k_mixture_model.cpp:42-47,274-304,355-356` uses help `"Usage: gen_k_mixture_model [--skip-pi-gram-training]\n                           [--maximum-occurs-allowed <INT>]\n                           [--maximum-increase-rates-allowed <FLOAT>]\n                           [--k-mixture-model-file <FILENAME>]\n                           {<FILENAME>}+\n"`, `"the m_WC integer in magic header overflows.\n"`, `"the m_total_freq in magic header overflows.\n"`, `"can't open file: %s.\n"`, `"error:%s.\n"`. `merge_k_mixture_model.cpp:26-27,110-116` prints `"Usage: merge_k_mixture_model [--result-file <RESULT_FILENAME>]\n"`, `"                             {<SOURCE_FILENAME>}+\n"`, and the same overflow formats. | `oxpinyin-utils/src/k_mixture_model/{prune,generate,merge}.rs`; the `oxpinyin-utils` binaries parse the pin's `GOptionEntry` tables; `oxpinyin-input/src/tool.rs` `k_mixture_model` makes the pin's `fprintf`, progress `*` and newline calls. |
| `import_interpolation.cpp:77,85,246-290`: `"error: interpolation model expected.\n"`, option/config messages above, `"open %s failed!\n"`, `"empty file input.\n"`. `import_k_mixture_model.cpp:78,86,269-303`, `k_mixture_model_to_interpolation.cpp:62,69,197-199`: `"error: k mixture model expected.\n"`, `"empty file input.\n"` and their common option/config diagnostics. `export_k_mixture_model.cpp:122-144` additionally uses `"open %s failed.\n"`. `gen_ngram.cpp:54,62`, `gen_deleted_ngram.cpp:55,63`, `gen_unigram.cpp:56,90,99`, `estimate_interpolation.cpp:95`, `export_interpolation.cpp:51` use the option/config/load formats above. | Datagen `system::read_interpolation`, `oxpinyin-utils/src/k_mixture_model/import.rs`, training counter, lambda and emitter; `oxpinyin-utils` `k_mixture_model_to_interpolation` for its own lines. |
| Debug functions `lookup/pinyin_lookup2.cpp:52-68`, `lookup/phonetic_lookup.cpp:40-56` use `"max value: %f\n"`, `"values:"`, `"%f\t"`, `"\n"`; `storage/phonetic_key_matrix.cpp:330-340` uses `"Column:%ld:\n"`, `"ChewingKey:%s\n"`, `"ChewingKeyRest:%hd\t%hd\n"`. Other matrix/search and table-info debug print sites are under `#if 0`. | No corresponding externally reachable Rust debug writer identified; presence of a function alone does not establish reachability. |

## Utility coverage and gaps

The pinned lists are `utils/storage/Makefile.am`,
`utils/segment/Makefile.am`, and `utils/training/Makefile.am`. **GAP**
means the named pin-compatible executable and complete interface do not
currently exist; a partial functional analogue is not a drop-in tool.
The scope includes both installed and `noinst_PROGRAMS` utilities.

| Pin utility | Current oxpinyin analogue | Status |
| --- | --- | --- |
| `gen_binary_files` | `oxpinyin-utils` `gen_binary_files` | **Ported** at stage 5 (2026-10-31 UTC): the pin's name, options, messages, `.table` scanner, index DBMs, per-library chunks and `punct.bin`; registered `tool-diff` cases. |
| `import_interpolation` | `oxpinyin-utils` `import_interpolation` | **Ported** at stage 5 (2026-10-31 UTC): the pin's name, option, messages, stdin taglib grammar, unigram delta and bigram `SingleGram` store. |
| `export_interpolation` | `oxpinyin-utils` `export_interpolation` | **Ported** at stage 5 (2026-10-31 UTC): no options or argument check, the pin's model text, `get_all_items`' native key order and `taglib_token_to_string`. `oxpinyin-emitter` is removed. |
| `gen_pinyin_table` | `oxpinyin-utils` `gen_pinyin_table` | **Ported** at stage 5 (2026-10-30 UTC, #1436): the pin's name, options, messages, input scanner and output. |
| `gen_zhuyin_table` | `oxpinyin-utils` `gen_zhuyin_table` | **Ported** at stage 5 (2026-10-30 UTC): direct zhuyin rows, native calls, and registered `tool-diff` cases. |
| `ngseg` | `oxpinyin-utils` `ngseg` | **Ported** at stage 5 (2026-10-31 UTC): the pin's name, options, messages, `table.conf`/phrase-index/bigram inputs and output; `PhraseLookup::get_best_match` over `phrase_index.bin` and `bigram.db`; registered `tool-diff` cases. |
| `spseg` | `oxpinyin-utils` `spseg` | **Ported** at stage 5 (2026-10-31 UTC): the pin's name, options, messages and fewest-words dynamic program over `phrase_index.bin`; registered `tool-diff` cases. |
| `mergeseq` | `oxpinyin-utils` `mergeseq` | **Ported** at stage 5 (2026-10-31 UTC): the pin's name, options, messages, `TAGLIB_PARSE_SEGMENTED_LINE`, token validation and queue merge over `phrase_index.bin`; registered `tool-diff` cases. |
| `gen_unigram` | `oxpinyin-utils` `gen_unigram` | **Ported** at stage 5 (2026-10-31 UTC): the pin's name, `--table-dir` (accepted and unread, as its static is), `table.conf` load, the `SYSTEM_FILE`/`DICTIONARY` +1 sweep and both chunk saves. |
| `gen_ngram` | `oxpinyin-utils` `gen_ngram` | **Ported** at stage 5 (2026-10-31 UTC): the pin's options (`--skip-pi-gram-training` with `G_OPTION_FLAG_REVERSE`, `--bigram-file`), `std::getline`, `TAGLIB_PARSE_SEGMENTED_LINE`, the uni/pi-gram writes into the backend hash DBM and the chunk save. |
| `gen_deleted_ngram` | `oxpinyin-utils` `gen_deleted_ngram` | **Ported** at stage 5 (2026-10-31 UTC): the same loop over `deleted_bigram.db`, no chunk save, and `ENODATA` where `gen_ngram` uses `ENOENT` on a refused chunk load. |
| `gen_k_mixture_model` | `oxpinyin-utils` `gen_k_mixture_model` | **Ported** at stage 5 (2026-10-31 UTC): the pin's name, options, messages, document scanner, `FlexibleBigram` container and registered `tool-diff` cases. |
| `estimate_interpolation` | `oxpinyin-utils` `estimate_interpolation` | **Ported** at stage 5 (2026-10-31 UTC): the pin's C-double EM over `single_gram` rows and the facade unigram table, `token:%d lambda:%f` and `average lambda:%f` with the host's NaN spelling. `oxpinyin-lambda` is removed. |
| `estimate_k_mixture_model` | `oxpinyin-utils` `estimate_k_mixture_model` | **Ported** at stage 5 (2026-10-31 UTC): the pin's options, EM and `%f` lines. |
| `merge_k_mixture_model` | `oxpinyin-utils` `merge_k_mixture_model` | **Ported** at stage 5 (2026-10-31 UTC): attaches `KMixtureModelBigram` through the store layer on all three backends, folds the source containers in `argv` order, and exits `EOVERFLOW` on the pin's magic-header checks. |
| `prune_k_mixture_model` | `oxpinyin-utils` `prune_k_mixture_model` | **Ported** at stage 5 (2026-10-31 UTC): `exit(EDOM)` and the progress asterisks are the pin's. |
| `import_k_mixture_model` | `oxpinyin-utils` `import_k_mixture_model` | **Ported** at stage 5 (2026-10-31 UTC): taglib grammar, cwd-relative `table.conf` and chunk loads, and the `FlexibleBigram` writes. |
| `export_k_mixture_model` | `oxpinyin-utils` `export_k_mixture_model` | **Ported** at stage 5 (2026-10-31 UTC): native token order, `taglib_token_to_string`, and `exit(ENODATA)` on a missing header. |
| `k_mixture_model_to_interpolation` | `oxpinyin-utils` `k_mixture_model_to_interpolation` | **Ported** at stage 5 (2026-10-31 UTC): no options or arguments, the pin's messages, `std::cin` and `stdout`, taglib's GLib calls, the pin's aborts with its check macros' text not imitated ([eio-tool-check-messages](eio-tool-check-messages.md)), and registered `tool-diff` cases. `oxpinyin-kmm to-interpolation` is removed. |
| `validate_k_mixture_model` | `oxpinyin-utils` `validate_k_mixture_model` | **Ported** at stage 5 (2026-10-31 UTC): the pin's checks, lines and `ENODATA`. |
| `eval_correction_rate` | `oxpinyin-utils` `eval_correction_rate` | **Ported** at stage 5 (2026-10-31 UTC): `evals2.text`, the pinyin index resolved through the phrase libraries, the `PhoneticLookup<1,1>` decode port (`oxpinyin-utils`' `training::eval`) over the binary chunks and `bigram.db`, and the `correction rate:%f` line. `oxpinyin-eval` is removed. |

## Test-program coverage

The [upstream test coverage ledger](../testing/upstream-test-coverage.md#libpinyin-tests)
is the single coverage table for every program in the pin's `tests/`.
It records all 17 `noinst_PROGRAMS`, including the six automated `TESTS`,
and tracks each complete drop-in tool as GAP or Ported.

## Records affected by the ruling

These are policy decisions and future work, not claims that code has
already changed. Existing frozen case markers and dated evidence remain
as they were until the relevant implementation stage changes them.

| Record | Disposition and replacement |
| --- | --- |
| `1297-table-floats` | Revert decimal-only acceptance; host `fscanf` `%f` on the pin stream. **Done at stage 1.** |
| `1297-table-decimals` | Revert complete `i32` acceptance; host `fscanf` `%d`. **Done at stage 1.** |
| `1297-table-lines` | Revert line-boundary substitution; pin's ordered scanner formats. **Done at stage 1.** |
| `1297-table-words` | Revert unlimited/exact token substitution; `%255s`, `strcmp`, pin C-string filename handling. **Done at stage 1.** |
| `1297-addon-index` | Revert strict slot parsing; host `atoi`, then the pin's `guint8` conversion and checks. **Done at stage 1.** |
| `1297-user-decimals` | Revert strict marker decimal acceptance; marker `fscanf` `%d`. **Done at stage 1.** |
| `1297-user-token` | Revert complete token acceptance; `%255s` and `strcmp`. **Done at stage 1.** |
| `1297-chunk-payload` | Revert payload `write_all`; one host `write`, then the pin's branch and sync behavior. |
| `1297-tool-decimals` | Revert strict corresponding tool fields; host `atoi`/`atol`. **Done for `k_mixture_model_to_interpolation` at stage 5** (2026-10-31 UTC); the other KMM, counter and eval fields remain. |
| `1297-tool-special-values` | Revert Rust special-value spelling; the pin's `%f` formatted call and the externally required value. NaN sign is host/compiler dependent and not established here for every host. |
| `926-tool-tokenizers` | Revert #1297's external-scanner amendments and the datagen strict-input decision for the corresponding pin utilities; use their `fgets`, `g_ascii_strtoull`, token/locale calls. `k_mixture_model_to_interpolation`'s taglib and `atoi`/`atol` calls are the pin's at stage 5 (2026-10-31 UTC). |
| [interpolation2-grammar](interpolation2-grammar.md) | Revert the bounded `g_unichar_isgraph` approximation and omitted GLib warnings on matching pin-tool inputs; use GLib. The standalone decoder reader has no matching pin runtime `interpolation2.text` reader established here. `oxpinyin-utils` `taglib` uses GLib's calls (2026-10-31 UTC); the datagen reader is not yet moved. |
| [Policy row 109](compatibility-policy.md) | Whole-file writable private mapping is active; memmap2 releases it in full under the mapping-lifetime ruling. |
| [1297-punct-words](1297-punct-words.md) | **Not reverted as an unsafe defect:** the class (b) treatment for the undefined surrounding read stands. DBM record access stays in store; no out-of-bounds native read is introduced. |
| Policy row 39 and assertion records | **Stand:** reproduce the `LC_NUMERIC` side effect and preserve the established class (b) refusal and warning behavior and the reproduced aborts. |

## Platform gaps

| Platform issue | Pin call or fallback | Verdict / unresolved limit |
| --- | --- | --- |
| No `mmap` | `HAVE_MMAP` controls `LIBPINYIN_USE_MMAP` (`memory_chunk.h:31-33`; `CMakeLists.txt:64`); callers use `MemoryChunk::load` without it (`pinyin.cpp:250-260`, `zhuyin.cpp:196-203`). | On Linux, a build without `mmap` uses the heap-read fallback and its exact calls. On Windows and macOS, `memmap2` is permitted under the [platform scope ruling](../source-policy.md#platform-scope-of-pinned-behaviour) (2026-10-31 UTC): platform-dependent behaviour, not a compatibility defect. |
| `g_fsync` | Ordinary Unix GLib headers macro-expand to `fsync`; Windows GLib's real function calls `_commit`. The separately compiled GLib Unix wrapper has `F_FULLFSYNC`/EINTR/fallback branches, but those are not the ordinary macro-expanded call. | On Linux, respect the actual host header/build branch. On Windows and macOS the [platform scope ruling](../source-policy.md#platform-scope-of-pinned-behaviour) permits a portable Rust abstraction; a difference there is platform-dependent behaviour. |
| `g_rename` and plain `unlink` | Ordinary Unix `g_rename` expands to `rename`; Windows GLib converts UTF-8 to UTF-16 and calls `MoveFileExW`. The pin separately uses plain C `unlink`. | On Linux, do not substitute GLib wide-name unlink for the pin's plain C call. On Windows, Rust's rename adds a retry; under the [platform scope ruling](../source-policy.md#platform-scope-of-pinned-behaviour) that is platform-dependent behaviour, not a compatibility defect. |
| `O_BINARY` | The pin defines it as zero if unavailable (`memory_chunk.h:39-40`). | Obtain the target constant from the C headers, not a guessed Rust numeric value. |
| Windows CRT text mode | Config and tool streams use `"r"`/`"w"` without `b` or `t`; behavior depends on the host CRT's default. | **UNSETTLED:** [Microsoft documents](https://learn.microsoft.com/en-us/cpp/c-runtime-library/file-translation-constants?view=msvc-170) CRLF and CTRL+Z translation in text mode, but this record did not read the exact CRT implementation or establish a native Windows pin build. Platform-dependent behaviour under the [platform scope ruling](../source-policy.md#platform-scope-of-pinned-behaviour), recorded here, not a compatibility defect. |
| Tool input streams on Windows | `std::ifstream::open(const char *)` and `fopen` take a narrow name, which the C runtime converts through the active code page; MSVC's `std::ifstream` opens in text mode, translating CRLF to LF and treating Ctrl+Z as end of file. | **UNSETTLED:** `oxpinyin-input`'s `tool::LineReader::open` uses the shared lossy UTF-8 filename decoder for C-string names; invalid bytes become U+FFFD. Rust Path inputs bypass that decoder. Unix passes the bytes. `LineReader` reads a Rust `File`'s raw bytes, so a CR before LF stays in the line and a Ctrl+Z does not end the input: on Windows it can differ from the pin's text-mode stream. This is platform-dependent behaviour under the [platform scope ruling](../source-policy.md#platform-scope-of-pinned-behaviour), not reproduced, and unverified until Windows runs the tools. |
| POSIX/C++ availability on Windows | Pin includes `<unistd.h>` and directly calls `open`, `read`, `write`, `lseek`, `close`, `unlink`; no source-level fallback for a toolchain without them was found. | **UNSETTLED:** availability and semantics depend on the native toolchain or compatibility layer. Do not invent a pin fallback or assume glibc/libstdc++ on macOS or Windows; the [platform scope ruling](../source-policy.md#platform-scope-of-pinned-behaviour) lets oxpinyin use portable Rust abstractions there. |
| Locale and GLib versions | Pin calls the host's libc/GLib and their locale data. | The glibc 2.41/GLib 2.84.4 source comparison does not prove Apple libc, another Linux libc, Windows CRT or every GLib build. Use the same native call, including locale effects. |

## Proposed crate and staged implementation

`oxpinyin-input` is the approved name despite its bidirectional scope.
Its safe API owns only calls without a proved Rust equivalent; domain
models, checksums, profile orchestration and record interpretation stay
in their existing crates.

```text
oxpinyin-input/
  src/lib.rs          safe types and facade
  src/config.rs       scan_system_config, scan_user_config, write_user_config
  src/chunk.rs        open/read/map/save chunk operations
  src/path.rs         pin path construction, rename and unlink
  src/diagnostic.rs   pin stderr/stdout formatted writes
  src/tool.rs         tool streams, numeric and GLib text adapters
  src/native.rs       libc calls; the C library's stdout/stderr per platform
```

`scan_system_config` uses the actual ordered `fopen`/`fscanf` stream and
returns the raw `f32` lambda. The training loader and
`lm::set_lambda_from_table_conf` must share it. A line-only rational
reader cannot be the pin scanner. Nonfinite and out-of-range values need
their actual `f32`, not a fallback when conversion to the rational
`Lambda` fails. `scan_user_config` and
`write_user_config` preserve the marker's native calls and error checks.
Chunk helpers own the selected open/read/map/write/sync/close sequence;
path and diagnostic helpers own their corresponding pin calls.

The native modules use scoped unsafe with `// SAFETY:` on every block.
Under the 2026-10-31 UTC ruling they call the `libc` crate and
`glib-sys`. They declare by hand only what neither crate binds: the C
library's `stdout` and `stderr` objects. `native.rs` declares these once
per platform, for the library and the tools (Linux, Apple/FreeBSD/DragonFly
and Windows; any other target is a `compile_error!`). `g_utf8_next_char`'s step is the
lead byte's sequence length in plain Rust, not GLib's `g_utf8_skip` table
([eio-utf8-next-char-invalid-lead](eio-utf8-next-char-invalid-lead.md)).
The crate has no build script.
No borrowed raw pointer escapes the safe facade. `oxpinyin-data` is
`forbid(unsafe_code)`; its mmap implementation moved to `oxpinyin-input`. User, runtime,
core and engine keep their existing forbids. C facades keep ABI
marshalling. Store keeps DBM calls. `oxpinyin-data`'s config,
mapping and `pin_stderr` call mechanics, `oxpinyin-user`'s persistence
file calls, and `oxpinyin-runtime`'s config probing move to the boundary;
their domain logic stays put.

**Do not start implementation before #1175's last extraction lands.** Each
numbered stage is then a separately reviewable, independently gated
change. Keep the selected backend and the applicable verification-cell
rules from the contributor rules. Do not alter frozen cases or
specifications before the stage that owns them.

1. **Library config read/write and locale** (implemented 2026-10-28
   UTC, with the rulings above). Create the minimal boundary. Move the
   system/user config scans, marker writing and locale calls together,
   including the shared lambda scanner. Gate both facade paths and the
   touched crates. Preserve class (b) handling and the reproduced aborts.
   `config.c` is gone (2026-10-31 UTC ruling): `oxpinyin-input`
   `native.rs` makes the same `setlocale`, `fopen`, `fscanf`, `feof`,
   `atoi`, `fprintf` and `fclose` calls through the `libc` crate, with the
   same format strings, call order and result checks.
2. **Chunks** (implemented 2026-11-03 UTC, #1576). System/addon mapping is
   active through the input boundary; the live sub-index owns the one mapping
   and data forbids unsafe code. Row 109's whole-mapping release follows the
   ruling above. User chunk and journal loads use the input boundary's single
   payload read. The system chunk fallback is not compiled in the pin's build
   (`LIBPINYIN_USE_MMAP`, see the chunk row) and is not connected. Existing
   class (b) records stand; the wrapped length rule is settled below.
3. **Save and rename** (implemented 2026-11-01 UTC). Move the pin's path
   building, unlink, staging and rename calls. Preserve pinyin's two-pass
   order and zhuyin's interleaved order. Gate failure ordering and both
   facades. DBM calls stay in store.
   - Path building takes over the `g_build_filename` row of the #1297
     classification (`glib-libc-inventory.md`, row 18; maintainer decision
     2026-10-31 UTC). It keeps the pin's paths, including a name that
     begins with a separator, which `Path::join` does not keep. It settles
     the Windows/macOS question and the `glib-sys` dependency of
     `oxpinyin-input`'s library path.
   - The chunk `open`/`write`/`fsync`/`close` calls are in this stage
     (`chunk.rs`), so stage 2's save half is implemented here.
     System/addon mapping and user chunk load also use the boundary.
4. **Diagnostics** (implemented 2026-10-30 UTC, #1437). Move the
   library's raw formatted writes and GLib diagnostics to the correct
   native calls. Gate exact bytes, channels, ordering and existing
   class-(b) warnings. oxpinyin's own warnings stay the facades' `g_log` in
   the pin's domain; the pin has no call there. `diagnostic.c` is gone
   (2026-10-31 UTC ruling): each line is `libc::fprintf` on the C
   library's own `stderr` object.
5. **Tools, in small independently gated slices.** First add the
   tool-only stream and GLib option/text boundary. Then port each
   utility family with the pin names and interface, including the two
   missing table generators. Move KMM working files to native backend
   DBMs through store; a change touching backend code follows the
   three-cell path rule. Gate each utility or cohesive small family on
   its own; do not wait for the whole tools set. **In progress:**
   - The boundary (`oxpinyin-input` `tools`) and `gen_pinyin_table`
     (`oxpinyin-utils`) landed together (2026-10-30 UTC, #1436), so CI
     builds and tests the boundary through its first user.
     `gen_zhuyin_table` followed (#1450).
   - `k_mixture_model_to_interpolation` (2026-10-31 UTC) adds the shared
     taglib port and the `assert` and `abort()` sites that a `tool-diff`
     case reproduces on both sides as SIGABRT. It replaces
     `oxpinyin-kmm to-interpolation`. The same change moves the three
     tools off `native/tool.c` and the C++ stream shim onto `libc`,
     `glib-sys` and `std::io` (2026-10-31 UTC ruling, item 5).
   - The utility links GLib, because taglib's character classes and
     warnings are GLib's calls (`g_unichar_isspace`/`isgraph`, `g_log`).
     The Windows job installs the vcpkg `glib` port and excludes no crate
     ([setup](../runbooks/setup.md)).
   - The DBM-attached KMM family (`estimate_`, `merge_`, `prune_`,
     `validate_k_mixture_model`) needs neither stage 2 nor stage 3, and is
     the next stack.
   - The utilities that load phrase-library chunks use the stage 2
     `MemoryChunk::load` boundary. Their chunk saves use the save boundary
     and reload the newly stored chunk into the index.

Each stage derives expected behavior from the pin and its native library
sources before it uses a differential as a gate.

## Open questions — differential confirmation

This list records the remaining questions and the rulings that settled others:

1. **Settled (2026-11-03 UTC):** `load` and `mmap` require the declared length to equal `(physical_file_size - 8) as u32`, preserving modulo 2^32 narrowing (`memory_chunk.h:439-443,499-503`). A one-byte excess rejects; an exact multiple of 2^32 passes and the declared extent is kept.
2. A missing `table.conf` — settled by ruling D3 above, not by a
   differential.
3. Missing system DBMs.
4. The external consequence of BDB `tmp_db->sync(m_db, 0)` in save.

Row 109's post-fini release is settled by the [mapping lifetime ruling](#mapping-lifetime-ruling);
production activation is complete.

Stage 5 follow-up: `gen_zhuyin_table` uses the same tool boundary and
shared phrase tree as `gen_pinyin_table`. The
[frozen tool cases](../runbooks/tool-diff.md) hold both generators to the
pin, including source-derived class (b) refusal records.
