# External I/O boundary

Status: **policy**. All five stages are implemented, and all 21 pin
utilities are ported ([Utility coverage and gaps](#utility-coverage-and-gaps)).

This record holds the per-call inventory, the design of the boundary crate
and the decisions made at each stage. The pin is libpinyin
`074a2219c90feaf962d0d24f034514033ece5f99`. Pin citations are paths and
lines in [that tree](https://github.com/libpinyin/libpinyin/tree/074a2219c90feaf962d0d24f034514033ece5f99).
oxpinyin citations name files and items, not line numbers. GLib and glibc
citations are to GLib 2.84.4 and glibc 2.41.

## Ruling and scope

The rulings are in the compatibility policy:

- [External I/O uses the pin's call](compatibility-policy.md#external-io),
  including the macro-check rule, the class (b) records that stand and the
  drop-in tools;
- [platform scope](compatibility-policy.md#platform-scope);
- the [Rust/C++ interoperability ruling](compatibility-policy.md#rust-cpp-interoperability)
  for library bindings and semantic adapters;
- [crate placement](compatibility-policy.md#crate-placement);
- [external API misuse](compatibility-policy.md#external-api-misuse),
  including the mapping lifetime of row 109.

The pin's `tests/` programs are product tests and the nonshipping
`oxpinyin-repl`, not drop-in tools; the
[coverage ledger](../testing/upstream-test-coverage.md#libpinyin-tests)
records them. The 21 genuine utilities, the pin's `bin_PROGRAMS` and
`noinst_PROGRAMS`, are drop-in tools. `scripts2/` Python build generators
are build steps, out of scope.

## Stage 2 mapping

`oxpinyin-input/src/chunk.rs` owns `MappingSource`, `PendingMapping` and
`Mapping`:

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
`src/zhuyin.cpp:196-203,230-237,585-592,795-803`. memmap2 is used on every
OS: `MmapInner::map_copy` calls `MmapInner::new` with
`PROT_READ | PROT_WRITE`, `MAP_PRIVATE`, offset zero, and its `Drop`
releases the entire mapping. On Unix `finish` closes the descriptor; on
Windows memmap2 keeps a duplicated handle until the mapping drops. The
boundary does not reproduce the pin's non-mmap Windows heap path. On
Windows it opens chunk paths with `_wopen`, keeping the Rust `OsStr` as
UTF-16 under the [Windows platform adaptation ruling](compatibility-policy.md#native-rust-implementation).

Each loaded system chunk's one mapping belongs to its live sub-index; an
addon owns its one mapping directly. `oxpinyin-data` has no mapper and
forbids unsafe code. Save, reload and mask make operation-scoped fresh
mappings; there is no persistent second mapping. `mask_out` maps each
system library and reads its journal once, one library at a time, and stops
at the first fault (rows 76, 91, 92, 96, 97, 108); no later library is
opened after a fault. The walk's `masked_log` raises every journal fault
the mask path has (`HeaderToken`, `MultipleHeaders`, `SurvivingRecord`,
`ShortMaskedItem`); the records it hands to the merge are a header and
MODIFYs, so `ShortAdd` cannot occur there and a mismatch only ends a merge.

Both `load` and `mmap` require the declared length to equal
`(physical_file_size - 8) as u32`, keeping the pin's modulo 2^32 narrowing
(`memory_chunk.h:439-443,499-503`). A one-byte excess rejects; an exact
multiple of 2^32 passes and the declared extent is kept.

### Replay writes and the undo vector

Replay writes offsets and content directly into the writable private
mapping, or into the grown `Vec`, with the pin's logical operations:
`O(1)` offset access. Before each mutation of a mapped byte, the replay
pushes one typed undo entry: the old 4-byte offset word (slot and value),
or the destination range of an in-place content write. The old content
bytes are not copied: the entry borrows the journal record's checked old
payload from the log buffer. A write into a grown `Vec` needs no entry,
because undoing discards that `Vec`.

Normal completion, a mismatch stop or an unknown-record stop drops the undo
entries and keeps every applied change, offsets and total frequency
included, as `SubPhraseIndex::merge` does (`phrase_index.cpp:439-519`,
which returns early at a mismatch with the prefix applied). Only when the
replay reaches the payload over-read (class (b)) are the entries applied in
reverse, the original total restored from a scalar saved at the start, and
any grown buffer dropped; the library keeps its pre-replay content. Row 107
is not a rollback trigger: a record that reaches an unreadable or
out-of-bounds item answers "no item" and the other changes are kept.

Cost: the undo `Vec` is `O(R)` constant-size entries. Every record occupies
at least eight bytes of the log (`phrase_index_logger.h:233-241`), so
`R ≤ L/8` and the space stays within the pin's `O(L)` journal order. The
success path pays an `O(1)` push per mutation; the reversal costs
`O(Σ range bytes)` and runs only on the over-read. This transient vector is
the only cost above the pin's.

**Reverse-order undo restores the mapped bytes and offsets.** Each entry
restores the exact bytes its destination range held immediately before its
write, so applying the entries in reverse restores the state before the
first write. The saved bytes equal those bytes in every reachable case:

- *In-place MODIFY*: the apply compares the stored item (`len` bytes) with
  the record's old payload and writes `new.len() ≤ len` bytes at its
  offset, so the saved bytes are `old[..new.len()]` (`replay.rs` `modify`,
  `phrase_index.cpp:485-501`).
- *Repeated MODIFY of one token*: the second compare sees the first write's
  result; the reverse application restores it, then the first entry
  restores the original.
- *Overlapping writes*: the argument is over byte ranges, not tokens.
- *Relocated items* (a longer MODIFY, REMOVE then ADD): the content append
  goes to the grown `Vec`; the only mapped mutation is the offset word.
  REMOVE of a mismatching item still removes (the pin removes first), and
  its offset entry is pushed before the write.
- *Stale-item writes* (row 107's absent-MODIFY path): the stale compare
  reads the stale place and requires it equal to the old payload before
  writing. A write into the default six-byte stale buffer is not library
  state and has no entry. A `Shipped` stale place addresses the mapping,
  also after growth, and has an entry. An `Owned` place addresses the
  `Vec`, which the reversal discards.
- *Offset-array growth*: an offset write past the array's end copies the
  array into a zero-filled `Vec` of `cursize + extra` bytes. Writes before
  the copy have entries on the mapping; reversal drops the `Vec`.
- *Content growth*: the copy takes the mapping's current bytes, and every
  later content write goes to the `Vec`, except a `Shipped` stale write.
  Reversal restores the mapping and drops the `Vec`, and the logical size,
  capacity and `appends` count return to the saved scalars.

**Logical capacity.** The pin's growth is `malloc(cursize+extra)` and
`memset` on the first copy (a copy of the whole current sub-chunk,
`memory_chunk.h:102-123`), then `realloc(max(2·capacity, cursize+extra))`
and `memset` of the tail (`:127-130`). The reached-payload outcome reads
`capacity()`, so the logical `capacity` and `data_end`, and the zero-filled
extension, are tracked in their own fields (`Content::capacity`). A `Vec`'s
own capacity is never consulted.

**Failed remap** (row 76): the pin's failed `mmap` leaves an empty chunk
that `SubPhraseIndex::load` dereferences. oxpinyin's save stops at that
library and answers `false`; filesystem changes before it stay.

**Rejected: a pre-walk (option (b)).** `merge_log` and
`SubPhraseIndex::merge` stop at a mismatch with the applied prefix kept, so
a pre-walk that found an over-read tail would discard a prefix the pin
keeps. An extra replay mapping is also rejected.

## Stage 1 rulings

- **D1, λ.** The scanned `f32` is the only source of λ. Real-table
  sentence and phrase scoring use the scanned `f32` directly.
- **D2, diagnostics.** `table_info.cpp`'s three `fprintf(stderr)` calls
  (`:201`, `:332`, `:382`) are made with their native I/O sequences: exact
  calls, bytes, channel and order. A context without a user dir makes the
  same calls on `""`, as the pin's `g_build_filename(NULL, "user.conf", NULL)`
  does.
- **D3, `table.conf`.** Every `table.conf` load failure follows the pin,
  ENOENT included: init fails with the pin's diagnostics (`open %s
  failed.` from the load, then `load %s failed!`). There is no
  `TableConf::stock()` fallback on that path.
- **D4, marker save.** `check_format` ignores the marker save's result
  and continues (`pinyin.cpp:187`); no extra `oxpinyin:` line.
- **Order.** The marker is written before the wipe and unlinks, as the
  pin does (`pinyin.cpp:187`, `:194-215`).

### Native text-mode line endings

Under constitution §6, `user.conf` line endings are **platform-dependent
native-host behaviour**. `UserTableInfo::save` opens with
`fopen(filename, "w")` (`src/storage/table_info.cpp:380`) and prints four
newline-terminated directives with `fprintf` (`:386-390`).
`UserTableInfo::load` opens with `fopen(filename, "r")` (`:330`) and scans
them with `fscanf` (`:338-357`).

The host C runtime therefore writes LF on Linux/macOS and CRLF on
Windows, where text-mode input translates CRLF back to LF. A Windows
build of the pin follows the same text-mode round trip. The boundary
keeps the pin's `"w"` and `"r"` calls, as `libc::fopen` with the same mode
strings. It does not force binary mode or normalize the written bytes.
`user_files::tests::user_conf_round_trips_and_conforms` and
`config::tests::the_writer_prints_the_four_lines` assert the exact
host-native bytes and read the saved file through the boundary's
`fscanf`. The former also checks that the reloaded marker conforms. The LF
text returned by `UserTableInfo::to_text` is a fixture composer.

Native scanning also governs malformed identity bytes. Microsoft UCRT's
[`input_adapter_character_source`](https://github.com/huangqinjin/ucrt/blob/d6e817a4cc90f6f1fe54f8a0aa4af4fff0bb647d/inc/corecrt_internal_strtox.h#L1956-L1980)
narrows an input byte to `char`
and skips pushback when it compares equal to EOF. Its integer parser
pushes back the first non-digit (`parse_integer`, same file, `:301-324`).
With signed `char`, `0xFF` after the binary version is consumed instead
of restored; the following whitespace directive and model directive can
then succeed, and the marker keeps the profile on MSVC Windows. This is
native-host behaviour, not permission to alter the pin's scanner calls
(`table_info.cpp:338-348`).
`persistence::tests::stray_identity_bytes_follow_the_native_scanner`
keeps the `0xFF` case and checks both facade lifecycles against the
native-host result: the marker conforms on MSVC Windows and is refused on
Linux/macOS, where the non-digit remains for the next directive. A separate
`0xFE` case must fail the directive and wipe the profile; both bytes are
invalid UTF-8, so the protection against lossy decoding remains covered.

## How to read the inventory

**EQUIVALENT** means both implementation sources establish the same
observable result for every input in the explicitly stated domain.
**NOT EQUIVALENT** names a distinguishing input or condition.
**UNSETTLED** means the proof is missing; it is treated as not equivalent.
Equivalence of a single primitive does not establish equivalence of its
enclosing reader, writer, path conversion or error handling. A verdict
describes the inspected sources, not an executed result. **The pin's call**
means that oxpinyin makes it; a rejected Rust reader or writer is named with
the input that distinguishes it.

The pin has no direct `sscanf`, `fread`, `fwrite`, `getenv`, `g_getenv`,
`strto*`, `truncate`, or `ftruncate` call in the scoped `src/` and
`utils/` source. It calls `g_ascii_strtoull` directly. Called libraries
can make more calls internally. This is an inventory of the pin's explicit
calls and the identified library effects. It does not prove all
transitive runtime or environment reads of every dependency. Cwd, file
type and permissions, umask, descriptor state, signal interruption, locale
and native library versions affect the calls.

## Input inventory

| Input and exact pin call | oxpinyin item | Verdict |
| --- | --- | --- |
| `table.conf`: `SystemTableInfo2::load` calls `fopen(filename, "r")`; `fscanf` formats `"binary format version:%d\n"`, `"model data version:%d\n"`, `"lambda parameter:%f\n"`, `"source table format:%255s\n"`, `"database format:%255s\n"`, and `"%255s %255s %255s %255s %255s %255s\n"`; `strcmp` maps tokens; `fclose`. `src/storage/table_info.cpp:194-294`. | `oxpinyin-input` `config::SystemTableInfoReader`; `oxpinyin-data/src/table_info.rs` `TableConf::load` keeps the mappings and refusals. | The pin's call. Rejected Rust line reader: a collapsed one-line header is consumed by scanner whitespace; a 256-byte field is cut by `%255s`; a FIFO with no writer can block in the pin's open. |
| Lambda in that same stream: `fscanf(input, "lambda parameter:%f\n", &lambda)`, under `setlocale(LC_NUMERIC, "C")`; `table_info.cpp:197-224`. | Both facades and `lm::set_lambda_from_table_conf` share `TableConf::load`'s `fscanf("%f")`. | The pin's call. Rejected Rust `f32` parse and an isolated `strtof`: `0x1p-1` and `nan` are accepted by the pin scanner, and a line-only reader omits preceding directives and scanner consumption ([glibc 2.41 scanner](https://github.com/bminor/glibc/blob/glibc-2.41/stdio-common/vfscanf-internal.c)). |
| Addon slot: `to_index_of_addon_tables` calls `atoi`, returns `guint8`; `SystemTableInfo2::load` checks the resulting slot. `table_info.cpp:159-161,270-280`. | Host `atoi`, then the pin's `guint8` cut and check. | The pin's call. Rejected `usize` parse: `abc` and `256` reach slot zero at the pin ([glibc 2.41 `atoi`](https://github.com/bminor/glibc/blob/glibc-2.41/stdlib/atoi.c)). |
| `user.conf`: `UserTableInfo::load` calls `fopen("r")`, `fscanf` with `"binary format version:%d\n"`, `"model data version:%d\n"`, `"database format:%255s\n"`, `"open counter:%d\n"`, token `strcmp`, and `fclose`; `table_info.cpp:325-373`. | `config::UserTableInfoReader`; `oxpinyin-data/src/user_files.rs` `UserTableInfo::load` keeps the pin's `EOF`/`0`/`1` rules. | The pin's call. Rejected complete-field parse: a version plus `2^32` narrows back to a conforming value; `BerkeleyDB\0junk` matches the pin's C string. |
| System/addon chunk: `MemoryChunk::mmap` calls `open(filename, O_RDONLY\|O_BINARY)`, `lseek(fd, 0, SEEK_END)`, `lseek(fd, 0, SEEK_SET)`, two 4-byte `read`s, `mmap(NULL, file_size, PROT_READ\|PROT_WRITE, MAP_PRIVATE, fd, 0)`, and `close`; it calls `munmap` on checksum failure. `src/include/memory_chunk.h:475-524`. | `oxpinyin-data/src/phrase_library.rs` validates through `oxpinyin-input::chunk::{MappingSource, PendingMapping, Mapping}`. | The pin's call ([Stage 2 mapping](#stage-2-mapping)). Data's file-type metadata prechecks remain. |
| User chunks and journals: `MemoryChunk::load` calls `open`, both `lseek`s, two 4-byte `read`s, one payload `read`, and `close`; `memory_chunk.h:416-463`. | `oxpinyin-input::chunk::load` shares the mapping source's open and seeks, makes the header reads and one payload read, and keeps the descriptor through `oxpinyin-data::user_files::load_chunk_payload`'s checksum. User-library loads and journal replay, including reload and mask, use it. | The pin's call. The returned byte count governs the checksum and chunk length on a nonnegative short read: a match keeps the shortened chunk, a mismatch frees, closes and answers false, with no retry. A negative read, including `EINTR`, takes the class (b) safe failure of policy row 114. The header `assert`s (`:433`, `:437`) are reproduced. Windows uses `_wopen`. |
| System chunk fallback: `MemoryChunk::load` for system chunks | none | Not compiled in the pin's build: the callers use `load()` only when `LIBPINYIN_USE_MMAP` is undefined, `memory_chunk.h:30-33` defines it whenever `HAVE_MMAP` is, and the pin's build defines `HAVE_MMAP` (`config.h:42`; [row 76](compatibility-policy.md)). No heap fallback is connected. |
| System, addon, punctuation and user DBMs: backend `attach`, `load_db`, open, cursor/get and close calls; e.g. `src/storage/chewing_large_table2_bdb.cpp:77-132`, `chewing_large_table2_kyotodb.cpp:77-122`, `chewing_large_table2_tkrzwdb.cpp:75-108`. | `oxpinyin-data/src/dict.rs` `SystemDictionary::open_with`, `AddonDictionary::attach_with`; `punct.rs` `PunctTable::open_optional`; user persistence loaders; `oxpinyin-store` backend adapters. | **UNSETTLED** as an independent all-input proof. The DBM backends follow the native backend through store. |
| `.table` and `punct.table`: `gen_binary_files` uses `fopen("r")`, loaders, `fseek(tablefile, 0L, SEEK_SET)` between three table passes and `fclose`; `utils/storage/gen_binary_files.cpp:36-105`. `table_text_read_pinyin_record`/`table_text_read_punct_record` use `fgets(line, sizeof(line), infile)` with a 4096-byte array, `strspn`, `strcspn`, `g_strlcpy`, and `g_ascii_strtoull(field, &end, 10)`; `src/storage/table_text_parser.h:34-121`. | `oxpinyin-utils` `gen_binary_files` (`storage/table_text.rs`). | The pin's call. Rejected UTF-8 whitespace reader: a fifth field is ignored by the pin; a count `-0` is rejected by the pin's helper; the pin splits long lines at `fgets`'s buffer and keeps non-UTF-8 bytes. |
| `gen_pinyin_table` source rows: `fopen(filename, "r")`, `fscanf(infile, "%1023s %1023s %u", ...)`, `fclose`; `utils/storage/gen_pinyin_table.cpp:138-157`. `gen_zhuyin_table` uses `std::ifstream`, `std::getline`, then `atoi(strs[1])`; `utils/storage/gen_zhuyin_table.cpp:141-157`. | `oxpinyin-utils` `gen_pinyin_table`, `gen_zhuyin_table`. | The pin's call. |
| Segmented corpus, interpolation and KMM text from stdin or files: `std::cin`/`std::ifstream`, `std::getline`, and `LineString::operator const gchar *` returning `c_str()`; `src/include/pinyin_utils.h:33-40`, `utils/segment/ngseg.cpp:107-111,187-194`, `utils/storage/import_interpolation.cpp:56-68,235-238`. | `oxpinyin-input` `LineReader`, which reproduces `std::getline`'s `eofbit`/`failbit`, its tied-`stdout` flush and the `c_str()` view. | **EQUIVALENT** ([libstdc++ 14.2 `getline`](https://github.com/gcc-mirror/gcc/blob/releases/gcc-14.2.0/libstdc%2B%2B-v3/include/bits/basic_string.tcc#L905-L965)). |
| Segmented tokens and tagged values: `g_strsplit_set(line, " \t", 2)`, `atoi` in `TAGLIB_PARSE_SEGMENTED_LINE`, `atoi` in `TAGLIB_GET_TOKEN`, and `atol` in `TAGLIB_GET_TAGVALUE`; `utils/utils_helper.h:27-69`, `utils/training/import_k_mixture_model.cpp:148-207`, `utils/storage/import_interpolation.cpp:135-182`. | `oxpinyin-input/src/tool.rs` `strsplit_set`/`atoi`/`atol`. | The pin's call. |
| Tagged text and phrase Unicode: `split_line` calls `g_utf8_get_char`, `g_utf8_next_char`, `g_unichar_isspace`, `g_unichar_isgraph`; loaders use `g_utf8_to_ucs4`. `src/storage/tag_utility.cpp:118-170`, `src/storage/phrase_index.cpp:537-544`. | `oxpinyin-utils/src/utils_helper/taglib.rs` makes the GLib calls for every tool. | The pin's call. Rejected Rust character properties: a format character such as U+200B in an unquoted field is excluded by GLib's graph classification ([grammar finding](interpolation2-grammar.md)). |
| Utility argv/options: `g_option_context_parse(context, &argc, &argv, &error)`; e.g. `utils/storage/gen_binary_files.cpp:112-130`, `utils/segment/ngseg.cpp:111-139`. | `oxpinyin-input` `tool::parse_options`. | The pin's call ([GLib `goption.c`](https://github.com/GNOME/glib/blob/2.84.4/glib/goption.c)). Rejected hand-written parsing: option names, grouping and error paths differ. |
| Path building: `g_build_filename` for system, user, addon, staging and diagnostics, e.g. `src/pinyin.cpp:150-165,250-259,952-985,997-1017,1050-1121`; `src/zhuyin.cpp:104-119,196-203,585-687`. | `oxpinyin-input/src/path.rs` `build_filename`, `build_path`, `build_bytes`; `oxpinyin-data` and `oxpinyin-user` call those. | The pin's call through `glib-sys`, with the pin's C arguments: a `NULL` first element answers `""` and a `NULL` name the directory. Unix keeps the bytes; Windows takes GLib's UTF-8 name. Cases `glib-922-*`, `init-system-*`, `table-conf-user-file-*`. |
| Locale and environment: config load/save calls `setlocale(LC_NUMERIC, "C")` and, on the success path, `setlocale(LC_NUMERIC, locale)` using the returned pointer; `table_info.cpp:197,291,328,372,378,394`. Many utilities call `setlocale(LC_ALL, "")`, e.g. `ngseg.cpp:111`, `gen_binary_files.cpp:112`. Relative paths use process cwd. | The six `setlocale` calls are made inside the `table.conf`/`user.conf` reader and writer at the pin's sites. | The pin's call. glibc locale lookup consults `LC_ALL`, category variables, `LANG` and `LOCPATH` where applicable; GLib warning handling includes `G_DEBUG` ([glibc locale lookup](https://github.com/bminor/glibc/blob/glibc-2.41/locale/findlocale.c#L110-L124), [GLib messages](https://github.com/GNOME/glib/blob/2.84.4/glib/gmessages.c#L218-L225)). |

The C++ `std::getline` sites are `utils/storage/gen_zhuyin_table.cpp:148`,
`import_interpolation.cpp:68`; `utils/segment/ngseg.cpp:188`,
`spseg.cpp:269`, `mergeseq.cpp:264`; `utils/training/gen_ngram.cpp:79`,
`gen_deleted_ngram.cpp:80`, `gen_k_mixture_model.cpp:74`,
`eval_correction_rate.cpp:177`, `import_k_mixture_model.cpp:69`, and
`k_mixture_model_to_interpolation.cpp:52`. Named input-file stream opens
are in the corresponding utilities, including `gen_k_mixture_model.cpp:352`
and `eval_correction_rate.cpp:166`; otherwise they consume `std::cin`.

## Output inventory

| Output and exact pin call | oxpinyin item | Verdict |
| --- | --- | --- |
| `user.conf`: `UserTableInfo::save` calls `fopen(filename, "w")`, `fprintf` with `"binary format version:%d\n"`, `"model data version:%d\n"`, `"database format:%s\n"`, `"open counter:%d\n"`, then `fclose`. It checks the open, not subsequent write/close results. `table_info.cpp:377-395`. | `oxpinyin-user/src/persistence.rs` `write_marker` through `config::save_user_table_info`. | The pin's call, checking only the open. Rejected `std::fs::write`: if `user.conf` points at `/dev/full`, opening succeeds and the output fails; the pin ignores that failure. |
| Chunk save: `MemoryChunk::save` calls `open(filename, O_CREAT\|O_WRONLY\|O_TRUNC\|O_BINARY, 0644)`, `write(fd, &length, sizeof(guint32))`, `write(fd, &checksum, sizeof(guint32))`, one `write(fd, begin(), size())`, `g_fsync(fd)`, and `close(fd)`. A short/failed payload closes and returns before sync. `memory_chunk.h:536-557`. | `oxpinyin-input/src/chunk.rs` `save`; `oxpinyin-data/src/chunk_format.rs` `build_memory_chunk` supplies the bytes. The utilities save through the same call (`utils/utils_helper.h:97-145`). | The pin's call: `libc::open`, one `write` per header word, one payload `write`, `libc::fsync` and `libc::close`. The header's assert is reproduced as an abort (row 77); a short payload is the pin's soft `false`. Cases `abort-save-chunk-header-*`, `abort-zhuyin-save-chunk-header-*`: POLICY-ACCEPTED. Windows uses `libc::wopen` with the same flags and mode, one CRT write per header and payload, `libc::commit`, then close. Rejected `std::fs::write`: with umask `000` it creates at `0666`, not `0644`, and has no sync. |
| Chunk sync: `g_fsync(fd)` at `memory_chunk.h:555`. Ordinary Unix GLib headers macro-expand it to `fsync`; Windows GLib's function calls `_commit` ([GLib header](https://github.com/GNOME/glib/blob/2.84.4/glib/gstdio.h#L60-L84), [wrapper](https://github.com/GNOME/glib/blob/2.84.4/glib/gstdio.c#L1666-L1702)). | `chunk::save` calls `libc::fsync` once and ignores its answer. | The pin's call on Unix: no Rust retry and no macOS `F_FULLFSYNC`. Windows calls `libc::commit` once and ignores its result. |
| Mapping release: `MemoryChunk::freemem` calls `munmap(m_data_begin - header, header + capacity())`; `memory_chunk.h:72-78`. A failed checksum instead calls `munmap(data - header, file_size)` at `:516`. | The input `Mapping` owns the whole-file mapping; `PendingMapping` drops a rejected mapping before its descriptor. | memmap2 releases the whole mapping; the wrapped-size residual is POLICY-ACCEPTED under the [external API misuse ruling](compatibility-policy.md#external-api-misuse). |
| Profile cleanup: `_clean_user_files` and `check_format` call plain `unlink(filename)` for selected user files and fixed DBM names; `pinyin.cpp:150-215`, `zhuyin.cpp:104-159`. | `oxpinyin-user/src/persistence.rs` `clean_user_files` calls `oxpinyin-input` `path::unlink` (`libc::unlink`). | The pin's call and order: the default rows' user files by sub-index, libpinyin's addon rows (`pinyin.cpp:199-200`), then `USER_PINYIN_INDEX`, `USER_PHRASE_INDEX` and `USER_BIGRAM`; libzhuyin stops after the default rows. Every result is ignored. Cases `table-conf-addon-cleanup{,-zhuyin}`, `table-conf-user-library-named-user-conf{,-zhuyin}`. |
| Save staging/rename: pinyin unlinks the three DBM staging names, writes chunks/DBMs, then calls `g_rename(tmppathname, chunkpathname)` or `g_rename(tmpfilename, filename)` for each; `pinyin.cpp:940-1128`. Zhuyin interleaves write and rename; `zhuyin.cpp:554-689`. Each failed rename calls `fprintf(stderr, "rename %s to %s failed.\n", ...)`. | `oxpinyin-user/src/persistence.rs` `stage_chunk`, `stage_dbm`, `stage_all` (the user bigram's `save_db`), `save_reporting`, `Staged::finish`; `oxpinyin-input` `chunk::save`, `path::rename`, `path::unlink`. | The pin's call on Unix (`libc::rename`). Pinyin keeps its two passes and zhuyin its interleaved order; the three DBM staging names are unlinked before the write; a failed save leaves every staged `.tmp` where the pin leaves it. The reporting save carries on after a failed rename, as the pin does. Windows calls GLib's `g_rename`; before it, `path::rename` refuses a directory over an existing file (`ENOTDIR`) and a file over an existing directory (`EISDIR`), the refusals `rename(2)` makes and `MoveFileExW` does not, under the Windows platform adaptation ruling. Held by `a_directory_is_not_renamed_over_a_file`, `a_file_is_not_renamed_over_a_directory`, `failed_renames_report_exact_order_and_preserve_destinations` and the cases `save-929-*`, `stderr-save-*`. |
| Library diagnostics: `fprintf(stderr, "open %s failed.\n", ...)` for system config, `"open %s failed."` for user config, `"write %s failed.\n"` for marker open, `"load %s failed!\n"`, `"mmap %s failed!\n"`, `"open %s failed!\n"` for loads, and the rename format above. `table_info.cpp:199-202,330-333,380-383`; `pinyin.cpp:255-259,289-293,338,955-959,1061,1080,1096,1110,1123,1264-1268`; `zhuyin.cpp:199-203,233-237,282,588-592,611,637,657,674,689,799-803`. | `oxpinyin-input` `diagnostic` (`report_*`, `report_mmap_failed`) through `oxpinyin-data` `c_stderr`. | The pin's call: each line is `libc::fprintf` on the C library's own `stderr` object, declared per platform in `oxpinyin-input` `native.rs` (glibc and musl `stderr`, Apple and FreeBSD `__stderrp`, UCRT `__acrt_iob_func(2)`), not a new stream on descriptor 2. oxpinyin never writes `open %s failed!`, the non-mmap build's line. The `SubPhraseIndex::load` criticals (`phrase_index.cpp:354-356`, `:361`) are not made (decision D1, [Phrase-library layout faults write no critical](upstream-divergences.md#phrase-library-layout-faults-write-no-critical)). |
| Backend DBM saves: BDB `unlink`, `db_create`, `tmp_db->open(..., DB_CREATE, 0600)`, copy, `tmp_db->sync(m_db, 0)`, `tmp_db->close(tmp_db, 0)` (`chewing_large_table2_bdb.cpp:135-159`, `phrase_large_table3_bdb.cpp:150-174`, `ngram_bdb.cpp:81-105`, `punct_table_bdb.cpp:111-135`). KC `unlink`, `dump_snapshot` (`chewing_large_table2_kyotodb.cpp:124-130`, `phrase_large_table3_kyotodb.cpp:139-145`, `ngram_kyotodb.cpp:82-91`, `punct_table_kyotodb.cpp:99-105`). Tkrzw `RemoveFile`, `Open(..., true, File::OPEN_DEFAULT)`, copy, `Synchronize(false)`, `Close` (`chewing_large_table2_tkrzwdb.cpp:111-127`, `phrase_large_table3_tkrzwdb.cpp:102-118`, `ngram_tkrzwdb.cpp:65-81`, `punct_table_tkrzwdb.cpp:96-109`). | `oxpinyin-store/src/bdb/mod.rs` `BdbUserBigramDb::save_db`; `kyotocabinet/mod.rs` `KcUserBigramDb::save_db`, `KcStore::write_user_index`/`write_user_bigram`; `tkrzw/mod.rs` `TkrzwUserBigramDb::save_db`. | **UNSETTLED** independently. BDB source passes `m_db` to the method selected from `tmp_db`, while `BdbUserBigramDb::save_db` calls `file.sync()` on the new handle. The difference has no observable effect, because `DB->close` syncs the new file ([upstream report item 8](upstream-report-drafts.md#8-berkeley-db-save_db-syncs-the-in-memory-handle-not-the-file)). KC `write_user_bigram` also documents a sibling-temp/rename sequence. |
| Backend reset and maintenance: BDB `m_db->sync(m_db, 0)` and `m_db->close(m_db, 0)`, including `chewing_large_table2_bdb.cpp:68,544`, `phrase_large_table3_bdb.cpp:80,412`, `ngram_bdb.cpp:41`, `punct_table_bdb.cpp:43`, `flexible_ngram_bdb.h:53`. Tkrzw `m_db->Synchronize(false)` and `Close()`, e.g. `chewing_large_table2_tkrzwdb.cpp:65,489`, `phrase_large_table3_tkrzwdb.cpp:52,312`, `punct_table_tkrzwdb.cpp:45`, `flexible_ngram_tkrzwdb.h:86`. KC closes its handles, e.g. `phrase_large_table3_kyotodb.cpp:79,132`. | Backend-specific `WriteStore`/`UserBigramDb` methods in `oxpinyin-store/src/{bdb,kyotocabinet,tkrzw}/mod.rs`. | **UNSETTLED** as an independent all-input proof. The exact source-side arguments, ordering and failure handling must be kept. |
| KMM working models: `FlexibleBigram::attach` opens/creates backend DBMs and later reads/writes/syncs through the selected backend; `src/storage/flexible_ngram_bdb.h:94-153,165-235,245-291`, `src/storage/flexible_ngram_kyotodb.h:129-180,192-272`, `src/storage/flexible_ngram_tkrzwdb.h:128-188,200-282`, `utils/training/gen_k_mixture_model.cpp:316-355`. | `oxpinyin-store/src/flexible_ngram.rs` `FlexibleBigram` opens the same hash contract (`create_hash`/`open_hash_read_only`), magic record, single-gram records and per-token header writes on all three backends, and walks `get_all_items` in the backend-native order. `oxpinyin-utils/src/k_mixture_model/` carries the seven programs' operations. | The pin's container on all three backends. A DBM file the tools write is read by the pin and vice versa (`oxpinyin-store/tests/flexible_bigram.rs`; `oxpinyin-utils/src/k_mixture_model/flexible_records.rs`). Three source-derived asserts are POLICY-ACCEPTED ([KMM tool asserts write no macro text](upstream-divergences.md#kmm-tool-asserts-write-no-macro-text)). |
| Utility text output/diagnostics: `fopen(outputfile, "w")`, `fprintf`, `fclose`, `printf`, `g_print`, `g_warning`, `perror`; formats and sites below. | `oxpinyin-input/src/tool.rs` makes the pin's calls for every utility. | The pin's call. Rejected Rust formatting: on empty held-out or evaluation input the pin prints the host C `%f` NaN spelling, where Rust prints `NaN`; NaN sign is host and compiler dependent. |

### Utility formatted calls

The following catalog groups identical calls but keeps the pin's
function, formats and locations. Literal help and diagnostic strings at
the cited sites are part of the tool contract, including their case,
punctuation, whitespace and exit behaviour.

| Pin function/sites and `printf`/`fprintf`/GLib format | oxpinyin item |
| --- | --- |
| `ngseg.cpp:36,81-100,119-260`: `"Usage: ngseg [--generate-extra-enter]  [-o outputfile] [inputfile]\n"`, `"%s\n"`, `"%d %s\n"`, `"%d \n"`, `"Un-segmentable sentence encountered:%s\n"`, `"non-ucs4 characters encountered:%s.\n"`, `"too many arguments.\n"`, `"load table.conf failed.\n"`, `perror("open file failed")`; `fopen(outputfile, "w")`, `fclose(output)`. | `oxpinyin-utils` `segment::ngseg` (`print_help` is dead code and GLib prints `--help`). |
| `spseg.cpp:37,183-195,214-343`: `"Usage: spseg [--generate-extra-enter] [-o outputfile] [inputfile]\n"`, `"%d %s\n"`, `"%d \n"`, the non-UCS4/argument/config errors and `perror("open file failed")`; `fopen("w")`/`fclose`. | `oxpinyin-utils` `segment::spseg`. |
| `mergeseq.cpp:37,141-170,213-280`: `"Usage: mergeseq [-o outputfile] [inputfile]\n"`, `"%d %s\n"`, `"%s\n"`, argument/config errors and `perror("open file failed")`; `fopen("w")`/`fclose`. | `oxpinyin-utils` `segment::mergeseq`. |
| `export_interpolation.cpp:34-39,76-132`: `"\\data model interpolation\n"`, `"\\1-gram\n"`, `"\\2-gram\n"`, `"\\end\n"`, `"\\item %d %s count %ld\n"`, `"\\item %d %s %d %s count %d\n"`. | `oxpinyin-utils` `export_interpolation`. |
| `export_k_mixture_model.cpp:39-108`: `"no magic header in k mixture model.\n"`, `"\\data model \"k mixture model\" count %d N %d total_freq %d\n"`, section/end literals, `"\\item %d %s count %d freq %d\n"`, `"\\item %d %s %d %s count %d T %d N_n_0 %d n_1 %d Mr %d\n"`; `k_mixture_model_to_interpolation.cpp:62-170` uses interpolation section literals, `"\\item %d %s count %ld\n"`, `"\\item %d %s %d %s count %ld\n"`. | `oxpinyin-utils/src/k_mixture_model/export.rs` `Sink`, `print_array_headers`, `print_array_items`; `oxpinyin-input/src/tool.rs` `k_mixture_model` makes the pin's `fprintf` calls; `oxpinyin-utils/src/export_k_mixture_model.rs` parses the options and loads the cwd-relative tables. `k_mixture_model_to_interpolation`'s lines are the pin's calls in `oxpinyin-utils`. |
| `estimate_interpolation.cpp:129,139` and `estimate_k_mixture_model.cpp:141,152`: `"token:%d lambda:%f\n"`, `"average lambda:%f\n"`; `eval_correction_rate.cpp:119-121,168,203`: `"test sentence:%s\n"`, `"guessed sentence:%s\n"`, `"the result mis-matches.\n"`, `"Can't open file:%s\n"`, `"correction rate:%f\n"`. | `oxpinyin-utils` `estimate_interpolation` and `eval_correction_rate`; KMM `estimate::run`/`compute_interpolation` and `oxpinyin-input/src/tool.rs` `k_mixture_model::print_token_lambda`/`print_average_lambda` (the C library's `%f`, `-nan` included). |
| `gen_pinyin_table.cpp:28-33,106-114,138-157,169-215,261-325` and `gen_zhuyin_table.cpp:34-39,112-120,141-157,175-221,267-334`: help `"Usage: gen_pinyin_table -t <PHRASE_INDEX> \n-o <OUTPUTFILE> <FILE1> <FILE2> .. <FILEn>\n<OUTPUTFILE> the result output file\n<FILEi> input pinyin files\n<PHRASE_INDEX> phrase index identifier\n"`; `"nnodes: %d\n"`, `"Can't open file %s.\n"`, `"Can't write file %s.\n"`, `"Too long phrase:%s\t%s\t%d\n"`, `"Invalid pinyin:%s\t%s\t%d\n"`, `"Duplicate item: phrase:%s\tpinyin:%s\tfreq:%u\n"`, `"%s\t%s\t%d\t%d\n"`; zhuyin also `"wrong line format:%s\n"`. The zhuyin help itself says `gen_pinyin_table`. | `oxpinyin-utils` `gen_pinyin_table`, `gen_zhuyin_table`. |
| `gen_binary_files.cpp:64,92,99,120,130`: `"open %s failed!\n"`, `g_print("option parsing failed:%s\n", error->message)`, `"load table.conf failed.\n"`. `utils_helper.h:87,112,137`: `"load %s failed!\n"`, `"save %s failed."`. `phrase_index.cpp:581`: `"FacadePhraseIndex::load_text:%s\t%s\n"`; `chewing_large_table2.cpp:145` and `chewing_large_table.cpp:699`: `"ChewingLargeTable2::load_text:%s\t%s\t%u\t%" G_GSIZE_FORMAT "\n"` and the corresponding `ChewingLargeTable::` format. `punct_table.cpp:102`: `"duplicated punctuations: %s\n"`. | `oxpinyin-utils` `gen_binary_files`. |
| Common utility option failures use `g_print("option parsing failed:%s\n", error->message)` after `g_option_context_parse`, e.g. `gen_binary_files.cpp:119-120`, `ngseg.cpp:118-119`, `import_k_mixture_model.cpp:268-269`. `taglib_read` uses `g_warning("un-expected tags:%s.\n", tmp)` and `g_warning("missed required tags: %s.\n", required_tag_str)` (`tag_utility.cpp:239,256`); `g_return_val_if_fail` sites at `:137,202,211,246` have GLib's own diagnostic. | `oxpinyin-input` `tool::parse_options` and `oxpinyin-utils` `utils_helper::taglib`, which make the pin's `g_print` and warnings; the check macros are plain Rust ([Utility check macros write no message](upstream-divergences.md#utility-check-macros-write-no-message)). GLib fatal-warning settings can affect the outcome. |
| `validate_k_mixture_model.cpp:25-168`: help `"Usage: validate_k_mixture_model <FILENAME>\n"`; errors `"no magic header in k mixture model.\n"`, `"word count in magic header is unexpected zero.\n"`, `"total freq in magic header is unexpected zero.\n"`, `"the word count doesn't match the total freq.\n"`, `"word count in magic header:%d\n"`, `"sum of word count in array headers:%d\n"`, `"the sum differs from word count.\n"`, `"total freq in magic header:%d\n"`, `"sum of freqs in array headers:%d\n"`, `"the total freq differs from sum of freqs.\n"`, `"in the array header of token %d:\n"`, `"word count is zero but has array items.\n"`, `"both word count and freq are unexpected zero.\n"`, `"word count in array header:%d\n"`, `"sum of word count in array items:%d\n"`, `"wrong arguments.\n"`, `"k mixture model validation failed.\n"`. | `oxpinyin-utils/src/k_mixture_model/validate.rs` `validate_unigram`/`validate_bigram`; `oxpinyin-utils/src/validate_k_mixture_model.rs` parses `argv` against the empty option table; `oxpinyin-input/src/tool.rs` `k_mixture_model` makes the pin's `fprintf` calls. |
| `prune_k_mixture_model.cpp:31,73-75,116-144,175`: help `"Usage: prune_k_mixture_model -k <INT> --CDF <DOUBLE>  <FILENAME>\n"`, `"some wrong possibility is encountered:%f.\n"`, `"k:%d N:%d WC:%d n_0:%d n_1:%d\n"`, `"wrong arguments.\n"`, `"no magic header in k mixture model.\n"`, progress `"*"`, `"\n"`. `gen_k_mixture_model.cpp:42-47,274-304,355-356` uses help `"Usage: gen_k_mixture_model [--skip-pi-gram-training]\n                           [--maximum-occurs-allowed <INT>]\n                           [--maximum-increase-rates-allowed <FLOAT>]\n                           [--k-mixture-model-file <FILENAME>]\n                           {<FILENAME>}+\n"`, `"the m_WC integer in magic header overflows.\n"`, `"the m_total_freq in magic header overflows.\n"`, `"can't open file: %s.\n"`, `"error:%s.\n"`. `merge_k_mixture_model.cpp:26-27,110-116` prints `"Usage: merge_k_mixture_model [--result-file <RESULT_FILENAME>]\n"`, `"                             {<SOURCE_FILENAME>}+\n"`, and the same overflow formats. | `oxpinyin-utils/src/k_mixture_model/{prune,generate,merge}.rs`; the `oxpinyin-utils` binaries parse the pin's `GOptionEntry` tables; `oxpinyin-input/src/tool.rs` `k_mixture_model` makes the pin's `fprintf`, progress `*` and newline calls. |
| `import_interpolation.cpp:77,85,246-290`: `"error: interpolation model expected.\n"`, option/config messages above, `"open %s failed!\n"`, `"empty file input.\n"`. `import_k_mixture_model.cpp:78,86,269-303`, `k_mixture_model_to_interpolation.cpp:62,69,197-199`: `"error: k mixture model expected.\n"`, `"empty file input.\n"` and their common option/config diagnostics. `export_k_mixture_model.cpp:122-144` additionally uses `"open %s failed.\n"`. `gen_ngram.cpp:54,62`, `gen_deleted_ngram.cpp:55,63`, `gen_unigram.cpp:56,90,99`, `estimate_interpolation.cpp:95`, `export_interpolation.cpp:51` use the option/config/load formats above. | `oxpinyin-utils` `import_interpolation`, `k_mixture_model/import.rs`, `gen_ngram`, `gen_deleted_ngram`, `gen_unigram`, `estimate_interpolation`, `export_interpolation`, `k_mixture_model_to_interpolation`. |
| Debug functions `lookup/pinyin_lookup2.cpp:52-68`, `lookup/phonetic_lookup.cpp:40-56` use `"max value: %f\n"`, `"values:"`, `"%f\t"`, `"\n"`; `storage/phonetic_key_matrix.cpp:330-340` uses `"Column:%ld:\n"`, `"ChewingKey:%s\n"`, `"ChewingKeyRest:%hd\t%hd\n"`. Other matrix/search and table-info debug print sites are under `#if 0`. | No externally reachable Rust debug writer. |

## Utility coverage and gaps

The pinned lists are `utils/storage/Makefile.am`,
`utils/segment/Makefile.am`, and `utils/training/Makefile.am`, including
`noinst_PROGRAMS`. Every utility is an `oxpinyin-utils` binary under the
pin's name, with the pin's options, help, messages, inputs and outputs, and
registered `tool-diff` cases ([frozen tool cases](../runbooks/tool-diff.md)).

| Pin utility | Notes |
| --- | --- |
| `gen_binary_files` | The `.table` scanner, index DBMs, per-library chunks and `punct.bin`. |
| `import_interpolation` | Stdin taglib grammar, unigram delta and bigram `SingleGram` store. |
| `export_interpolation` | No options or argument check; `get_all_items`' native key order and `taglib_token_to_string`. |
| `gen_pinyin_table` | Input scanner and output. |
| `gen_zhuyin_table` | Direct zhuyin rows; the same tool boundary and shared phrase tree as `gen_pinyin_table`, including source-derived class (b) refusal records. |
| `ngseg` | `table.conf`/phrase-index/bigram inputs; `PhraseLookup::get_best_match` over `phrase_index.bin` and `bigram.db`. |
| `spseg` | The fewest-words dynamic program over `phrase_index.bin`. |
| `mergeseq` | `TAGLIB_PARSE_SEGMENTED_LINE`, token validation and queue merge over `phrase_index.bin`. |
| `gen_unigram` | `--table-dir` accepted and unread, as its static is; `table.conf` load, the `SYSTEM_FILE`/`DICTIONARY` +1 sweep and both chunk saves. |
| `gen_ngram` | `--skip-pi-gram-training` with `G_OPTION_FLAG_REVERSE`, `--bigram-file`, `std::getline`, `TAGLIB_PARSE_SEGMENTED_LINE`, the uni/pi-gram writes into the backend hash DBM and the chunk save. |
| `gen_deleted_ngram` | The same loop over `deleted_bigram.db`, no chunk save, and `ENODATA` where `gen_ngram` uses `ENOENT` on a refused chunk load. |
| `gen_k_mixture_model` | Document scanner and `FlexibleBigram` container. |
| `estimate_interpolation` | The pin's C-double EM over `single_gram` rows and the facade unigram table; `token:%d lambda:%f` and `average lambda:%f` with the host's NaN spelling. |
| `estimate_k_mixture_model` | The pin's EM and `%f` lines. |
| `merge_k_mixture_model` | Attaches `KMixtureModelBigram` through the store layer on all three backends, folds the source containers in `argv` order, and exits `EOVERFLOW` on the pin's magic-header checks. |
| `prune_k_mixture_model` | `exit(EDOM)` and the progress asterisks. |
| `import_k_mixture_model` | Taglib grammar, cwd-relative `table.conf` and chunk loads, and the `FlexibleBigram` writes. |
| `export_k_mixture_model` | Native token order, `taglib_token_to_string`, and `exit(ENODATA)` on a missing header. |
| `k_mixture_model_to_interpolation` | No options or arguments; `std::cin` and `stdout`; taglib's GLib calls; the pin's aborts, with its check macros' text not imitated ([Utility check macros write no message](upstream-divergences.md#utility-check-macros-write-no-message)). |
| `validate_k_mixture_model` | The pin's checks, lines and `ENODATA`. |
| `eval_correction_rate` | `evals2.text`, the pinyin index resolved through the phrase libraries, the `PhoneticLookup<1,1>` decode port (`oxpinyin-utils` `training::eval`) over the binary chunks and `bigram.db`, and the `correction rate:%f` line. |

The utility links GLib, because taglib's character classes and warnings
are GLib's calls (`g_unichar_isspace`/`isgraph`, `g_log`). The Windows job
installs the vcpkg `glib` port and excludes no crate
([setup](../runbooks/setup.md)). The utilities that load phrase-library
chunks use the `MemoryChunk::load` boundary; their chunk saves use the save
boundary and reload the newly stored chunk into the index.

## Records that stand

| Record | Disposition |
| --- | --- |
| [interpolation2-grammar](interpolation2-grammar.md) | The tools use GLib's `g_unichar_isgraph` and warnings through `oxpinyin-utils` `taglib`. No matching pin runtime `interpolation2.text` reader is established for the standalone decoder. |
| [Policy row 109](compatibility-policy.md) | The whole-file writable private mapping; memmap2 releases it in full. |
| [Punctuation values contain whole words](upstream-divergences.md#punctuation-values-contain-whole-words) | **Stands:** the class (b) treatment for the undefined surrounding read. DBM record access stays in store; no out-of-bounds native read is introduced. |
| Policy row 39 and assertion records | **Stand:** reproduce the `LC_NUMERIC` side effect and keep the class (b) refusal and warning behaviour and the reproduced aborts. |

## Platform gaps

| Platform issue | Pin call or fallback | Verdict / unresolved limit |
| --- | --- | --- |
| No `mmap` | `HAVE_MMAP` controls `LIBPINYIN_USE_MMAP` (`memory_chunk.h:31-33`; `CMakeLists.txt:64`); callers use `MemoryChunk::load` without it (`pinyin.cpp:250-260`, `zhuyin.cpp:196-203`). | On Linux, a build without `mmap` uses the heap-read fallback and its exact calls. On Windows and macOS, `memmap2` is permitted under the [platform scope ruling](../source-policy.md#platform-scope-of-pinned-behaviour): platform-dependent behaviour, not a compatibility defect. |
| `g_fsync` | Ordinary Unix GLib headers macro-expand to `fsync`; Windows GLib's real function calls `_commit`. The separately compiled GLib Unix wrapper has `F_FULLFSYNC`/EINTR/fallback branches, but those are not the ordinary macro-expanded call. | On Linux, the actual host header/build branch is respected. On Windows and macOS the [platform scope ruling](../source-policy.md#platform-scope-of-pinned-behaviour) permits a portable Rust abstraction; a difference there is platform-dependent behaviour. |
| `g_rename` and plain `unlink` | Ordinary Unix `g_rename` expands to `rename`; Windows GLib converts UTF-8 to UTF-16 and calls `MoveFileExW`. The pin separately uses plain C `unlink`. | On Linux, GLib wide-name unlink is not substituted for the pin's plain C call. On Windows, Rust's rename adds a retry; under the [platform scope ruling](../source-policy.md#platform-scope-of-pinned-behaviour) that is platform-dependent behaviour, not a compatibility defect. |
| `O_BINARY` | The pin defines it as zero if unavailable (`memory_chunk.h:39-40`). | The target constant comes from the C headers, not a guessed Rust numeric value. |
| Windows CRT text mode | Config and tool streams use `"r"`/`"w"` without `b` or `t`; behaviour depends on the host CRT's default. | **UNSETTLED:** [Microsoft documents](https://learn.microsoft.com/en-us/cpp/c-runtime-library/file-translation-constants?view=msvc-170) CRLF and CTRL+Z translation in text mode. Platform-dependent behaviour under the [platform scope ruling](../source-policy.md#platform-scope-of-pinned-behaviour), not a compatibility defect. |
| Tool input streams on Windows | `std::ifstream::open(const char *)` and `fopen` take a narrow name, which the C runtime converts through the active code page; MSVC's `std::ifstream` opens in text mode, translating CRLF to LF and treating Ctrl+Z as end of file. | **UNSETTLED:** `oxpinyin-input`'s `tool::LineReader::open` uses the shared lossy UTF-8 filename decoder for C-string names; invalid bytes become U+FFFD. Rust Path inputs bypass that decoder. Unix passes the bytes. `LineReader` reads a Rust `File`'s raw bytes, so a CR before LF stays in the line and a Ctrl+Z does not end the input: on Windows it can differ from the pin's text-mode stream. This is platform-dependent behaviour under the [platform scope ruling](../source-policy.md#platform-scope-of-pinned-behaviour), not reproduced. |
| POSIX/C++ availability on Windows | Pin includes `<unistd.h>` and directly calls `open`, `read`, `write`, `lseek`, `close`, `unlink`, with no source-level fallback for a toolchain without them. | **UNSETTLED:** availability and semantics depend on the native toolchain or compatibility layer. No pin fallback is invented, and glibc/libstdc++ is not assumed on macOS or Windows; the [platform scope ruling](../source-policy.md#platform-scope-of-pinned-behaviour) lets oxpinyin use portable Rust abstractions there. |
| Locale and GLib versions | Pin calls the host's libc/GLib and their locale data. | The cited glibc 2.41 and GLib 2.84.4 sources do not establish Apple libc, another Linux libc, Windows CRT or every GLib build. oxpinyin uses the same native call, including locale effects. |

## Proposed crate and staged implementation

`oxpinyin-input` is the boundary crate, named despite its bidirectional
scope. Its safe API owns only calls without a proved Rust equivalent;
domain models, checksums, profile orchestration and record interpretation
stay in their existing crates.

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

`scan_system_config` uses the ordered `fopen`/`fscanf` stream and returns
the raw `f32` lambda; the training loader and
`lm::set_lambda_from_table_conf` share it. Nonfinite and out-of-range
values keep their actual `f32`. `scan_user_config` and `write_user_config`
keep the marker's native calls and error checks. Chunk helpers own the
open/read/map/write/sync/close sequence; path and diagnostic helpers own
their pin calls.

The native modules use scoped unsafe with `// SAFETY:` on every block.
They call the `libc` crate and `glib-sys`. They declare by hand only what
neither crate binds: the C library's `stdout` and `stderr` objects.
`native.rs` declares these once per platform, for the library and the
tools (Linux, Apple/FreeBSD/DragonFly and Windows; any other target is a
`compile_error!`). `g_utf8_next_char`'s step is the lead byte's sequence
length in plain Rust, not GLib's `g_utf8_skip` table
([g_utf8_next_char from a byte that leads no sequence](upstream-divergences.md#g_utf8_next_char-from-a-byte-that-leads-no-sequence)).
The crate has no build script and no C shim. No borrowed raw pointer
escapes the safe facade. `oxpinyin-data` is `forbid(unsafe_code)`. User,
runtime, core and engine keep their forbids. C facades keep ABI
marshalling. Store keeps DBM calls.

Each stage derives the expected behaviour from the pin and its native
library sources before a differential gates it. All five stages are
implemented:

1. **Library config read/write and locale.** The system and user config
   scans, marker writing and locale calls, including the shared lambda
   scanner (Stage 1 rulings). `oxpinyin-input` `native.rs` makes the
   `setlocale`, `fopen`, `fscanf`, `feof`, `atoi`, `fprintf` and `fclose`
   calls through the `libc` crate, with the pin's format strings, call
   order and result checks. Class (b) handling and the reproduced aborts
   are kept.
2. **Chunks.** System/addon mapping through the input boundary
   ([Stage 2 mapping](#stage-2-mapping)); user chunk and journal loads use
   its single payload read. The system chunk fallback is not compiled in
   the pin's build and is not connected.
3. **Save and rename.** Path building, unlink, staging and rename calls,
   and the chunk `open`/`write`/`fsync`/`close` calls (`chunk.rs`).
   Pinyin's two-pass order and zhuyin's interleaved order are kept. Path
   building is the `g_build_filename` row of the
   [classification](glib-libc-inventory.md#classification-on-main) (row 18,
   decision D2): it keeps the pin's paths, including a name that begins
   with a separator, which `Path::join` does not keep. DBM calls stay in
   store.
4. **Diagnostics.** The library's raw formatted writes are
   `libc::fprintf` on the C library's own `stderr` object. oxpinyin's own
   warnings stay the facades' `g_log` in the pin's domain; the pin has no
   call there.
5. **Tools.** The tool-only stream and GLib option/text boundary
   (`oxpinyin-input` `tools` feature) and all 21 utilities; KMM working
   files are native backend DBMs through store. A change touching backend
   code follows the three-cell path rule.

## Open

- Missing system DBMs: the behaviour is not settled by a differential.
- Windows `g_build_filename` path behaviour and `g_rename`.
