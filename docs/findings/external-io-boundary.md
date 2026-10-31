# External I/O boundary

Date: 2026-10-28 UTC · Status: **maintainer ruling and implementation
plan; stage 1 implemented (2026-10-28 UTC), stage 4 implemented (2026-10-30 UTC, #1437), stage 5 in progress (the segmentation trio and the training family added 2026-10-31 UTC; the storage trio by the preceding stack member), stages 2 and 3 not implemented**. The pin is libpinyin
`074a2219c90feaf962d0d24f034514033ece5f99`; oxpinyin source is main
`a52461cc297a4e67b07406167e6581e0e9f211fa`. The pin source read was the
clean `/tmp/libpinyin-src` checkout at that commit. Pin citations below
are paths and lines in [that tree](https://github.com/libpinyin/libpinyin/tree/074a2219c90feaf962d0d24f034514033ece5f99);
oxpinyin citations name files and items, not line numbers. The source
comparison used Rust 1.97.1, GLib 2.84.4 and glibc 2.41 where specified;
those versions do not establish the implementation of every native host.
No oracle, probe or differential was run for this record.

## Ruling and scope

Maintainer ruling, 2026-10-28 UTC:

1. Every call crossing the program's edge, in the library and utilities,
   on input and output, uses the pin's exact call. Use host ISO C/POSIX
   calls, GLib calls where the pin uses GLib, and (since the 2026-10-31
   UTC ruling in item 5) Rust `std::io` under the C++ contract for the
   pin's C++ streams. Rust std otherwise is allowed only when source for both
   implementations proves identical observable results for every
   input. NOT EQUIVALENT and UNSETTLED both require the pin call.
2. The tools are drop-in replacements for the pin's utilities: binary
   names, options, help, messages, inputs and outputs. K-mixture working
   models use the pin's backend DBM through `oxpinyin-store`.
3. `scripts2/` Python build generators are out of scope: they are build
   steps, not installed tools. The utility list below includes the pin's
   `noinst_PROGRAMS` as well as `bin_PROGRAMS`, per the tool ruling.
4. Same call is not same defect. The existing class (b)/(c) records for
   undefined reads and aborts stand, including `1297-punct-words` and
   assertion refusals. Policy row 39 (`LC_NUMERIC`) stands. Row 109's
   mapping call is a revert target; its post-fini residual mapping is a
   separate question, not a decision made here.
5. **Superseded 2026-10-31 UTC (maintainer ruling): no hand-written C or
   C++ shims.** C library calls go through the `libc` crate (approved for
   `oxpinyin-input`) or `extern "C"` declarations, GLib calls through the
   workspace's `glib-sys`, and C++ streams through Rust `std::io` under the
   C++ contract. Only a function neither crate binds is declared by hand.
   Macros (`assert`, `g_return_val_if_fail`, `G_STRFUNC`) are not
   reproduced: their checks are plain Rust and their text is not imitated,
   recorded as a POLICY-ACCEPTED language artefact
   ([eio-tool-check-messages](eio-tool-check-messages.md)). The tools
   follow it, and so do the library's stage 1 and 4 calls:
   `oxpinyin-input/native/config.c` and `native/diagnostic.c` are gone,
   their calls made through `libc` in `native.rs`. The Kyoto Cabinet
   adapter, `oxpinyin-store/src/kyotocabinet/shim.cc`, is retained under
   the [Kyoto Cabinet exception](compatibility-policy.md#kyoto-cabinet-exception).
6. A new bidirectional `oxpinyin-input` crate owns these calls in scoped
   unsafe modules with `// SAFETY:` comments and safe typed functions.
   `oxpinyin-data` becomes `forbid(unsafe_code)` after extraction. The C
   facades retain C ABI marshalling; `oxpinyin-store` retains DBM backends.

The ruling replaces #1304's language-artefact treatment **for external
I/O**. It does not change the remaining internal language-artefact
policy, class (a) maths, class (b)/(c), or Rust-interface freedom. C API
arguments and marshalling stay in the C adapters. DBM records and the
backend calls that obtain and persist them stay in `oxpinyin-store`;
decoding their contents remains in the owning data/domain layer.

**Implementation must not start until the last #1175 extraction has
landed.** That prerequisite was met when #1374 merged (main `410c9ca5`).
Stage 1 then created the crate and changed the case markers its records
own; the inventory below keeps the source comparison as it was made and
notes where stage 1 now makes the pin's call.

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
  context. **Retired from real-table sentence and phrase scoring by
  #1406:** these paths use the scanned `f32` directly. The pre-frequency
  fallback scorer still uses the rational routing and overflow fallback;
  #1378 PR C retains them to preserve that scorer's behavior. See
  [the reachability inventory](float-scoring-1378.md#61-pr-c-reachability-and-retained-code).
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
build of the pin follows the same text-mode round trip. The boundary
keeps the pin's `"w"` and `"r"` calls under the 2026-10-28 external-I/O
ruling, now as `libc::fopen` with the same mode strings; it does not
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
can of course make additional calls internally. This is an inventory of
the pin's explicit calls and the identified library effects, not a claim
that all transitive runtime/environment reads of every dependency have
been proved. Cwd, file type and permissions, umask, descriptor state,
signal interruption, locale and native library versions affect the calls.

## Input inventory

| Input and exact pin call | Current oxpinyin item and candidate | Verdict and first difference or limit |
| --- | --- | --- |
| `table.conf`: `SystemTableInfo2::load` calls `fopen(filename, "r")`; `fscanf` formats `"binary format version:%d\n"`, `"model data version:%d\n"`, `"lambda parameter:%f\n"`, `"source table format:%255s\n"`, `"database format:%255s\n"`, and `"%255s %255s %255s %255s %255s %255s\n"`; `strcmp` maps tokens; `fclose`. `src/storage/table_info.cpp:194-294`. | `oxpinyin-data/src/table_info.rs` `TableConf::read`, `TableConf::parse`, `ConfigLines`; `oxpinyin-runtime/src/lib.rs` `read_conf_file`. Rust whole-file read, line split and conversions. **Stage 1:** `oxpinyin-input` `config::SystemTableInfoReader` makes these calls; `TableConf::load` keeps the mappings and refusals. | **NOT EQUIVALENT.** A collapsed one-line header can be consumed by scanner whitespace but is rejected by `ConfigLines`; a 256-byte field is cut by `%255s` but retained as one Rust word. A FIFO with no writer can block in the pin's open while `TableConf::read` rejects a non-regular file. [Lines](1297-table-lines.md), [words](1297-table-words.md). |
| Lambda in that same stream: `fscanf(input, "lambda parameter:%f\n", &lambda)`, under `setlocale(LC_NUMERIC, "C")`; `table_info.cpp:197-224`. | `TableConf::parse` uses Rust `f32` parsing. `oxpinyin-training/src/segment/model.rs` `parse_table_conf_lambda` delegates to it. `oxpinyin-data/src/table_conf.rs` has a separate line-only rational `read_table_conf_lambda`/`parse_table_conf_lambda`; `lm::set_lambda_from_table_conf` uses it. **Stage 1:** both facades, `lm::set_lambda_from_table_conf` and the training loader share `TableConf::load`'s `fscanf("%f")`; the line-only reader is gone. | **NOT EQUIVALENT.** `0x1p-1` and `nan` are accepted by the pin scanner and refused by the Rust scanner; the line-only reader also omits preceding directives and scanner consumption. Use the host `fscanf` stream, not an isolated `strtof`. [Finding](1297-table-floats.md); [glibc 2.41 scanner](https://github.com/bminor/glibc/blob/glibc-2.41/stdio-common/vfscanf-internal.c). |
| Addon slot: `to_index_of_addon_tables` calls `atoi`, returns `guint8`; `SystemTableInfo2::load` checks the resulting slot. `table_info.cpp:159-161,270-280`. | `table_info.rs` `TableConf::parse` uses Rust `usize` parsing and a range check. **Stage 1:** host `atoi`, then the pin's `guint8` cut and check. | **NOT EQUIVALENT.** `abc` and `256` reach slot zero at the pin but Rust refuses them. Use host `atoi` and the pin's subsequent conversion. [Finding](1297-addon-index.md); [glibc 2.41 `atoi`](https://github.com/bminor/glibc/blob/glibc-2.41/stdlib/atoi.c). |
| `user.conf`: `UserTableInfo::load` calls `fopen("r")`, `fscanf` with `"binary format version:%d\n"`, `"model data version:%d\n"`, `"database format:%255s\n"`, `"open counter:%d\n"`, token `strcmp`, and `fclose`; `table_info.cpp:325-373`. | `oxpinyin-user/src/persistence.rs` `check_format`, `parse_user_conf`; `oxpinyin-data/src/user_files.rs` `UserTableInfo::parse`. Rust whole-file read and complete-field parsing. **Stage 1:** `config::UserTableInfoReader` makes these calls; `UserTableInfo::load` keeps the pin's `EOF`/`0`/`1` rules. | **NOT EQUIVALENT.** On the documented glibc reference, a version plus `2^32` can narrow back to a conforming value while Rust rejects it. `BerkeleyDB\0junk` matches the pin's C string and not Rust's full token. [Decimals](1297-user-decimals.md), [token](1297-user-token.md). |
| System/addon chunk: `MemoryChunk::mmap` calls `open(filename, O_RDONLY\|O_BINARY)`, `lseek(fd, 0, SEEK_END)`, `lseek(fd, 0, SEEK_SET)`, two 4-byte `read`s, `mmap(NULL, file_size, PROT_READ\|PROT_WRITE, MAP_PRIVATE, fd, 0)`, and `close`; it calls `munmap` on checksum failure. `src/include/memory_chunk.h:475-524`; `pinyin.cpp:250-260,284-294,952-959,1260-1268`; `zhuyin.cpp:196-203,230-237,585-592,795-803`. | `oxpinyin-data/src/phrase_library.rs` `map_chunk`, `MappedFile::from_file`, using Rust `File`, metadata, and `memmap2::map_copy_read_only`. | **NOT EQUIVALENT.** Rust Unix `File::open` adds `O_CLOEXEC` and retries `EINTR`; an `exec` while the descriptor is open can observe inheritance. The pin maps writable/private and closes the descriptor; Rust initially maps read-only/private and retains its file. [Rust Unix `open_c`](https://github.com/rust-lang/rust/blob/1.97.1/library/std/src/sys/fs/unix.rs), [memmap2 0.9.11 Unix mapping](https://github.com/RazrFalcon/memmap2-rs/blob/v0.9.11/src/unix.rs). The separate trailing-byte question stays open. |
| User chunks and journals, and system chunk fallback: `MemoryChunk::load` calls `open`, both `lseek`s, two 4-byte `read`s, one payload `read`, and `close`; `memory_chunk.h:416-463`. | `oxpinyin-user/src/persistence.rs` `load_libraries`, `load_logs_with_mask` use `std::fs::read`; `oxpinyin-data/src/user_files.rs` `read_chunk_payload` decodes bytes. | **NOT EQUIVALENT.** A short or interrupted payload read is one call at the pin; Rust reads to completion and retries interruptions. Use the pin sequence. Do not settle trailing-byte behavior here. |
| System, addon, punctuation and user DBMs: backend `attach`, `load_db`, open, cursor/get and close calls; e.g. `src/storage/chewing_large_table2_bdb.cpp:77-132`, `chewing_large_table2_kyotodb.cpp:77-122`, `chewing_large_table2_tkrzwdb.cpp:75-108`. Paths include `pinyin_index.bin`, `phrase_index.bin`, `bigram.db`, `punct.bin`, the addon pair and user indexes/bigram. | `oxpinyin-data/src/dict.rs` `SystemDictionary::open_with`, `AddonDictionary::open_with`; `punct.rs` `PunctTable::open_optional`; user persistence loaders; `oxpinyin-store` backend adapters. | **UNSETTLED as an independent all-input proof.** The ruling states DBM backends already follow the native backend through store; this inventory does not independently prove every backend's operations. Missing-system-DBM behavior stays open. |
| `.table` and `punct.table`: `gen_binary_files` uses `fopen("r")`, loaders, `fseek(tablefile, 0L, SEEK_SET)` between three table passes and `fclose`; `utils/storage/gen_binary_files.cpp:36-105`. `table_text_read_pinyin_record`/`table_text_read_punct_record` use `fgets(line, sizeof(line), infile)` with a 4096-byte array, `strspn`, `strcspn`, `g_strlcpy`, and `g_ascii_strtoull(field, &end, 10)`; `src/storage/table_text_parser.h:34-121`. | `oxpinyin-datagen/src/table.rs` `read_table_file`/`parse_table_line`; `punct.rs` `read_punct_file`/`parse_punct_line`; Rust UTF-8 `read_to_string`, whitespace split and integer parses. | **NOT EQUIVALENT.** A fifth field is ignored by the pin but rejected by datagen; a count `-0` is rejected by the pin's helper but accepted by the table parser. The pin splits long lines at `fgets`'s buffer and retains bytes Rust's UTF-8 read rejects. [#926](926-tool-tokenizers.md). |
| `gen_pinyin_table` source rows: `fopen(filename, "r")`, `fscanf(infile, "%1023s %1023s %u", ...)`, `fclose`; `utils/storage/gen_pinyin_table.cpp:138-157`. `gen_zhuyin_table` uses `std::ifstream`, `std::getline`, then `atoi(strs[1])`; `utils/storage/gen_zhuyin_table.cpp:141-157`. | No direct corresponding oxpinyin utility. Datagen consumes generated `.table` files, not these same utility inputs. | **UNSETTLED / GAP.** No Rust equivalent is established. The pin calls remain required when these tools are added. |
| Segmented corpus, interpolation and KMM text from stdin or files: `std::cin`/`std::ifstream`, `std::getline`, and `LineString::operator const gchar *` returning `c_str()`; `src/include/pinyin_utils.h:33-40`, `utils/segment/ngseg.cpp:107-111,187-194`, `utils/storage/import_interpolation.cpp:56-68,235-238`, and the utility sources listed below. | `oxpinyin-training/src/segment/tool_cli.rs` `read_input` drains stdin or uses `std::fs::read`; segment/counter/eval parsers consume bytes. `oxpinyin-kmm/src/bin/oxpinyin-kmm.rs` `read_text`/`read_stdin` require UTF-8; `text.rs` `import` uses Rust lines. | **NOT EQUIVALENT.** With one complete interactive line and stdin still open, the pin can process that line before EOF; `read_input` waits for EOF. Rust KMM also refuses invalid UTF-8 before parsing. [libstdc++ 14.2 `getline`](https://github.com/gcc-mirror/gcc/blob/releases/gcc-14.2.0/libstdc%2B%2B-v3/include/bits/basic_string.tcc#L905-L965); Rust 1.97.1 [`Read::read_to_end`](https://github.com/rust-lang/rust/blob/1.97.1/library/std/src/io/mod.rs). Use `std::getline`'s contract; under the 2026-10-31 UTC ruling it is `BufRead::read_until(b'\n')` with the stream's `eofbit`/`failbit`, and `std::cin`'s tie flushes `stdout` before each read. |
| Segmented tokens and tagged values: `g_strsplit_set(line, " \t", 2)`, `atoi` in `TAGLIB_PARSE_SEGMENTED_LINE`, `atoi` in `TAGLIB_GET_TOKEN`, and `atol` in `TAGLIB_GET_TAGVALUE`; `utils/utils_helper.h:27-69`, `utils/training/import_k_mixture_model.cpp:148-207`, `utils/storage/import_interpolation.cpp:135-182`. | `oxpinyin-kmm/src/numeric.rs` `decimal_field`; `oxpinyin-training/src/counter/parse.rs` `parse_segmented_line`; `oxpinyin-kmm/src/text.rs` `import`. Rust parses complete fields. | **NOT EQUIVALENT.** `12abc` converts to 12 at the pin and is a Rust line error; `abc` becomes zero at the pin. Use host `atoi`/`atol` and `g_strsplit_set`. [Finding](1297-tool-decimals.md). |
| Tagged text and phrase Unicode: `split_line` calls `g_utf8_get_char`, `g_utf8_next_char`, `g_unichar_isspace`, `g_unichar_isgraph`; loaders use `g_utf8_to_ucs4`. `src/storage/tag_utility.cpp:118-170`, `src/storage/phrase_index.cpp:537-544`. | `oxpinyin-data/src/interp_grammar.rs` `Tokens`, `is_graph`; datagen uses Rust strings and character properties. `oxpinyin-utils` `taglib` makes the GLib calls for `k_mixture_model_to_interpolation`. | **NOT EQUIVALENT** outside `oxpinyin-utils`. A format character such as U+200B in an unquoted field is excluded by GLib's graph classification but not by Rust's `is_graph` approximation. Use the host GLib calls. [Grammar finding](interpolation2-grammar.md). |
| Utility argv/options: `g_option_context_parse(context, &argc, &argv, &error)`; e.g. `utils/storage/gen_binary_files.cpp:112-130`, `utils/segment/ngseg.cpp:111-139`. | `oxpinyin-training/src/segment/tool_cli.rs` `ToolArgs::parse`; KMM/datagen binaries use `std::env::args` and hand-written parsing. | **UNSETTLED.** Option names, grouping and error paths already differ; no all-input equivalence to [GLib `goption.c`](https://github.com/GNOME/glib/blob/2.84.4/glib/goption.c) was proved. Use the pin's GLib option operations for its interface. |
| Path building: pin calls `g_build_filename` for system, user, addon, staging and diagnostics, e.g. `src/pinyin.cpp:150-165,250-259,952-985,997-1017,1050-1121`; `src/zhuyin.cpp:104-119,196-203,585-687`. | `oxpinyin-data/src/table_info.rs` `FileName::from_bytes`, `pin_join`, `join_bytes`; `pin_stderr.rs` `build_filename`; user `persistence::user_file`. | **UNSETTLED.** Rust implements the join; `FileName::from_bytes` uses lossy UTF-8 conversion on non-Unix. Full GLib and platform path equivalence is not proved. Call `g_build_filename`. |
| Locale and environment: config load/save calls `setlocale(LC_NUMERIC, "C")` and, on the success path, `setlocale(LC_NUMERIC, locale)` using the returned pointer; `table_info.cpp:197,291,328,372,378,394`. Many utilities call `setlocale(LC_ALL, "")`, e.g. `ngseg.cpp:111`, `gen_binary_files.cpp:112`. Relative paths use process cwd. GLib diagnostics can depend on GLib environment settings. | `oxpinyin-capi/src/locale.rs` and `oxpinyin-zhuyin-capi/src/locale.rs` `pin_table_info_locale` call host libc, but are placed at facade entry points. **Stage 1:** those two files are gone; the six `setlocale` calls are made inside the `table.conf`/`user.conf` reader and writer at the pin's sites. `oxpinyin-training/src/segment/paths.rs` `locate_export_dir`/`locate_model_dir` additionally use `PINYIN_EXPORT_DIR`, `PINYIN_MODEL_DIR`, `PINYIN_MODEL_CACHE`, `CARGO_TARGET_DIR`: oxpinyin-specific tool discovery, not direct pin `getenv` calls. | **UNSETTLED for the complete lifecycle.** One same-argument native `setlocale` invocation is the same call; the number and placement of invocations differ. glibc locale lookup consults `LC_ALL`, category variables, `LANG` and `LOCPATH` where applicable; GLib warning handling includes `G_DEBUG`. [glibc locale lookup](https://github.com/bminor/glibc/blob/glibc-2.41/locale/findlocale.c#L110-L124), [GLib messages](https://github.com/GNOME/glib/blob/2.84.4/glib/gmessages.c#L218-L225). |

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
| Chunk creation/truncation/save: `MemoryChunk::save` calls `open(filename, O_CREAT\|O_WRONLY\|O_TRUNC\|O_BINARY, 0644)`, `write(fd, &length, sizeof(guint32))`, `write(fd, &checksum, sizeof(guint32))`, one `write(fd, begin(), size())`, `g_fsync(fd)`, and `close(fd)`. A short/failed payload closes and returns before sync. `memory_chunk.h:536-557`. | `oxpinyin-user/src/persistence.rs` `write_chunk_file`, `write_chunk`, `write_header_word`; `oxpinyin-data/src/chunk_format.rs` `build_memory_chunk`. Rust `OpenOptions`, `write` and `write_all`. | **NOT EQUIVALENT.** A short payload write or `EINTR` is retried by [`Write::write_all`](https://github.com/rust-lang/rust/blob/1.97.1/library/std/src/io/mod.rs); the pin stops. [Finding](1297-chunk-payload.md). For the narrow domain of an already-open Unix descriptor and one four-byte header, Rust's [`FileDesc::write`](https://github.com/rust-lang/rust/blob/1.97.1/library/std/src/sys/fd/unix.rs) calls the same libc `write` once: **EQUIVALENT only for that primitive**, not the whole save or its class-(c) refusal. |
| Chunk sync: `g_fsync(fd)` at `memory_chunk.h:555`. Ordinary Unix GLib headers macro-expand it to `fsync`; Windows GLib's function calls `_commit`. [GLib header](https://github.com/GNOME/glib/blob/2.84.4/glib/gstdio.h#L60-L84), [wrapper](https://github.com/GNOME/glib/blob/2.84.4/glib/gstdio.c#L1666-L1702). | `persistence::ChunkSink::sync` calls `File::sync_all`. | **NOT EQUIVALENT.** Rust retries `EINTR`; on macOS it uses `fcntl(F_FULLFSYNC)` while the ordinary Unix pin macro calls `fsync`. An interrupted sync distinguishes the sequence. [Rust Unix filesystem source](https://github.com/rust-lang/rust/blob/1.97.1/library/std/src/sys/fs/unix.rs). |
| Mapping release: `MemoryChunk::freemem` calls `munmap(m_data_begin - header, header + capacity())`; `memory_chunk.h:72-78`. A failed checksum instead calls `munmap(data - header, file_size)` at `:516`. | `oxpinyin-data/src/phrase_library.rs` `MappedFile` owns a `memmap2::Mmap` whose drop releases its mapping. | **UNSETTLED as a complete lifecycle.** The original whole-file mapping call is required by row 109; whether to retain the pin's post-fini residual mapping is separately open. |
| Profile cleanup: `_clean_user_files` and `check_format` call plain `unlink(filename)` for selected user files and fixed DBM names; `pinyin.cpp:150-215`, `zhuyin.cpp:104-159`. | `oxpinyin-user/src/persistence.rs` `clean_user_files`, `std::fs::remove_file`. | **UNSETTLED across platforms and full path handling.** On Unix, for an already formed NUL-free C path, the pin and [Rust Unix `unlink`](https://github.com/rust-lang/rust/blob/1.97.1/library/std/src/sys/fs/unix.rs) call the same libc function once: **EQUIVALENT for that primitive only**. The path construction and Windows behavior are not proved. |
| Save staging/rename: pinyin unlinks the three DBM staging names, writes chunks/DBMs, then calls `g_rename(tmppathname, chunkpathname)` or `g_rename(tmpfilename, filename)` for each; `pinyin.cpp:940-1128`. Zhuyin interleaves write and rename; `zhuyin.cpp:554-689`. Each failed rename calls `fprintf(stderr, "rename %s to %s failed.\n", ...)`. | `oxpinyin-user/src/persistence.rs` `stage_chunk`, `stage_dbm`, `save_reporting`, `Staged::finish`, `finish_save`; Rust remove/rename; facade diagnostics. | **NOT EQUIVALENT on Windows.** GLib calls `MoveFileExW` once; Rust can retry with `FileRenameInfoEx` after access denied, including the read-only-attribute path. [GLib `g_rename`](https://github.com/GNOME/glib/blob/2.84.4/glib/gstdio.c#L1158-L1197), [Rust Windows rename](https://github.com/rust-lang/rust/blob/1.97.1/library/std/src/sys/fs/windows.rs). On ordinary Unix the header expands `g_rename` to libc `rename`, and Rust's Unix primitive calls that function once for the same NUL-free C paths: **EQUIVALENT only for that primitive**, not the complete sequence. |
| Library diagnostics: `fprintf(stderr, "open %s failed.\n", ...)` for system config, `"open %s failed."` for user config, `"write %s failed.\n"` for marker open, `"load %s failed!\n"`, `"mmap %s failed!\n"`, `"open %s failed!\n"` for loads, and the rename format above. `table_info.cpp:199-202,330-333,380-383`; `pinyin.cpp:255-259,289-293,338,955-959,1061,1080,1096,1110,1123,1264-1268`; `zhuyin.cpp:199-203,233-237,282,588-592,611,637,657,674,689,799-803`. | `oxpinyin-data/src/pin_stderr.rs` `emit`, `mmap_failed`. Rust assembles bytes and uses `stderr().write_all`. **Stage 1:** a line that shares a failure sequence with a boundary line is the boundary's `fprintf` (`oxpinyin-data` `c_stderr`, `oxpinyin-input` `report_*`): `table_info.cpp`'s three lines (D2); in init, `load %s failed!` (`pinyin.cpp:338`, `zhuyin.cpp:282`) and the library loop's `mmap %s failed!` (`pinyin.cpp:256`, `:290`; `zhuyin.cpp:200`, `:234`), after `check_format`'s `user.conf` lines; in save, `mmap %s failed!` (`pinyin.cpp:956`, `zhuyin.cpp:589`) and `rename %s to %s failed.`, before `mark_version`'s line. Fini writes only `mark_version`'s line. Left to stage 4 on `pin_stderr`: `mmap %s failed!` in `mask_out` (`pinyin.cpp:1265`, `zhuyin.cpp:800`) and in the library load calls (`pinyin.cpp:256`, `:290` through `:446` and `:477`; zhuyin's twins), whose calls write no boundary line. oxpinyin never writes `open %s failed!`, the non-mmap build's line. **Stage 4:** those remaining lines are the boundary's `fprintf` too (`oxpinyin-input` `diagnostic::report_mmap_failed`, through `oxpinyin-data` `c_stderr::mmap_failed`), so every library line goes to the host C library's `stderr`; since 2026-10-31 UTC each is `libc::fprintf` on the C library's own `stderr` object, declared per platform in `oxpinyin-input` `native.rs` (glibc and musl `stderr`, Apple and FreeBSD `__stderrp`, UCRT `__acrt_iob_func(2)`), not a new stream on descriptor 2; `pin_stderr` and its Rust-stderr writer are gone. The `SubPhraseIndex::load` criticals (`phrase_index.cpp:354-356`, `:361`) are made with the call the pin's `g_return_val_if_fail` expands to, `g_return_if_fail_warning(NULL, G_STRFUNC, #expr)` (oracle `SubPhraseIndex::load+492`, `+552`, `+576`, `+608` in both objects), not a preformatted `g_log`. Since 2026-10-31 UTC (#1297 decision D1) they are not made at all ([1297-layout-criticals](1297-layout-criticals.md)). | **UNSETTLED for all sinks.** Byte assembly on ordinary stderr does not prove C `FILE *` buffering, retries and error behavior equivalent. The `/dev/full` marker example above is a confirmed surrounding difference. |
| Backend DBM saves: BDB `unlink`, `db_create`, `tmp_db->open(..., DB_CREATE, 0600)`, copy, `tmp_db->sync(m_db, 0)`, `tmp_db->close(tmp_db, 0)` (`chewing_large_table2_bdb.cpp:135-159`, `phrase_large_table3_bdb.cpp:150-174`, `ngram_bdb.cpp:81-105`, `punct_table_bdb.cpp:111-135`). KC `unlink`, `dump_snapshot` (`chewing_large_table2_kyotodb.cpp:124-130`, `phrase_large_table3_kyotodb.cpp:139-145`, `ngram_kyotodb.cpp:82-91`, `punct_table_kyotodb.cpp:99-105`). Tkrzw `RemoveFile`, `Open(..., true, File::OPEN_DEFAULT)`, copy, `Synchronize(false)`, `Close` (`chewing_large_table2_tkrzwdb.cpp:111-127`, `phrase_large_table3_tkrzwdb.cpp:102-118`, `ngram_tkrzwdb.cpp:65-81`, `punct_table_tkrzwdb.cpp:96-109`). | `oxpinyin-store/src/bdb/mod.rs` `BdbUserBigramDb::save_db`; `kyotocabinet/mod.rs` `KcUserBigramDb::save_db`, `KcStore::write_user_index`/`write_user_bigram`; `tkrzw/mod.rs` `TkrzwUserBigramDb::save_db`; user persistence stages DBMs. | **UNSETTLED independently.** The ruling keeps DBM backends in store. BDB source passes `m_db` to the method selected from `tmp_db`, while `BdbUserBigramDb::save_db` calls `file.sync()` on the new handle. The external consequence remains an open confirmation question. KC `write_user_bigram` also documents a sibling-temp/rename sequence; do not infer its reachability in a facade save from its name alone. |
| Backend reset and maintenance: BDB invokes `m_db->sync(m_db, 0)` and `m_db->close(m_db, 0)` on its handles, including `chewing_large_table2_bdb.cpp:68,544`, `phrase_large_table3_bdb.cpp:80,412`, `ngram_bdb.cpp:41`, `punct_table_bdb.cpp:43`, `flexible_ngram_bdb.h:53`. Tkrzw uses `m_db->Synchronize(false)` and `Close()`, e.g. `chewing_large_table2_tkrzwdb.cpp:65,489`, `phrase_large_table3_tkrzwdb.cpp:52,312`, `punct_table_tkrzwdb.cpp:45`, `flexible_ngram_tkrzwdb.h:86`. KC closes its handles, e.g. `phrase_large_table3_kyotodb.cpp:79,132`. | Backend-specific `WriteStore`/`UserBigramDb` methods in `oxpinyin-store/src/{bdb,kyotocabinet,tkrzw}/mod.rs` and their FFI adapters. | **UNSETTLED as an independent all-input proof.** Backend calls remain in store; their exact source-side arguments, ordering and failure handling must be retained under the ruling. |
| Datagen chunk files: `save_phrase_index` and `save_dictionary` call `MemoryChunk::save`; `utils/utils_helper.h:97-145`. | `oxpinyin-datagen/src/bin/oxpinyin-datagen.rs` `write_chunk_file` uses `std::fs::write`. | **NOT EQUIVALENT.** With umask `000`, a new pin chunk is requested at mode `0644`, Rust's ordinary create at `0666`. Datagen also omits the pin's chunk sync. |
| KMM working models: `FlexibleBigram::attach` opens/creates backend DBMs and later writes/syncs through the selected backend; `src/storage/flexible_ngram_bdb.h:94-115`, `utils/training/gen_k_mixture_model.cpp:316-355`. | `oxpinyin-kmm/src/bin/oxpinyin-kmm.rs` `run_generate`, `run_merge`, `load_model`, `write_output`; `oxpinyin-kmm/src/text.rs` `export`/`import` persist text. | **NOT EQUIVALENT.** The pin working file is a backend DBM; oxpinyin's is text. The ruling requires the backend DBM through `oxpinyin-store`. |
| Utility text output/diagnostics: `fopen(outputfile, "w")`, `fprintf`, `fclose`, `printf`, `g_print`, `g_warning`, `perror`; individual formats and sites are catalogued below. | Training `tool_cli::write_output`, segment binaries, emitter/lambda/eval, KMM CLI use Rust formatting, `write_all` and `std::fs::write`. | **UNSETTLED generally.** Buffering, incremental output, errors, locale and platform text mode are not proved equivalent. Empty held-out/evaluation input is **NOT EQUIVALENT**: the pin prints the host C `%f` NaN spelling and Rust prints `NaN`. [Finding](1297-tool-special-values.md). |

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
| `export_interpolation.cpp:34-39,76-132`: `"\\data model interpolation\n"`, `"\\1-gram\n"`, `"\\2-gram\n"`, `"\\end\n"`, `"\\item %d %s count %ld\n"`, `"\\item %d %s %d %s count %d\n"`. | `oxpinyin-training/src/emitter/emit.rs` `emit_interpolation2`, `write_interpolation2`. |
| `export_k_mixture_model.cpp:39-108`: `"no magic header in k mixture model.\n"`, `"\\data model \"k mixture model\" count %d N %d total_freq %d\n"`, section/end literals, `"\\item %d %s count %d freq %d\n"`, `"\\item %d %s %d %s count %d T %d N_n_0 %d n_1 %d Mr %d\n"`; `k_mixture_model_to_interpolation.cpp:62-170` uses interpolation section literals, `"\\item %d %s count %ld\n"`, `"\\item %d %s %d %s count %ld\n"`. | `oxpinyin-kmm/src/text.rs` `export`; CLI `write_output`. `k_mixture_model_to_interpolation`'s lines are the pin's calls in `oxpinyin-utils`. |
| `estimate_interpolation.cpp:129,139` and `estimate_k_mixture_model.cpp:141,152`: `"token:%d lambda:%f\n"`, `"average lambda:%f\n"`; `eval_correction_rate.cpp:119-121,168,203`: `"test sentence:%s\n"`, `"guessed sentence:%s\n"`, `"the result mis-matches.\n"`, `"Can't open file:%s\n"`, `"correction rate:%f\n"`. | Training `oxpinyin-lambda`, `eval::EvalReport::correction_rate_line`; KMM `run_estimate`, `numeric::format_six_decimals`. |
| `gen_pinyin_table.cpp:28-33,106-114,138-157,169-215,261-325` and `gen_zhuyin_table.cpp:34-39,112-120,141-157,175-221,267-334`: help `"Usage: gen_pinyin_table -t <PHRASE_INDEX> \n-o <OUTPUTFILE> <FILE1> <FILE2> .. <FILEn>\n<OUTPUTFILE> the result output file\n<FILEi> input pinyin files\n<PHRASE_INDEX> phrase index identifier\n"`; `"nnodes: %d\n"`, `"Can't open file %s.\n"`, `"Can't write file %s.\n"`, `"Too long phrase:%s\t%s\t%d\n"`, `"Invalid pinyin:%s\t%s\t%d\n"`, `"Duplicate item: phrase:%s\tpinyin:%s\tfreq:%u\n"`, `"%s\t%s\t%d\t%d\n"`; zhuyin also `"wrong line format:%s\n"`. The zhuyin help itself says `gen_pinyin_table`. | **GAP:** neither generator has a direct utility counterpart. |
| `gen_binary_files.cpp:64,92,99,120,130`: `"open %s failed!\n"`, `g_print("option parsing failed:%s\n", error->message)`, `"load table.conf failed.\n"`. `utils_helper.h:87,112,137`: `"load %s failed!\n"`, `"save %s failed."`. `phrase_index.cpp:581`: `"FacadePhraseIndex::load_text:%s\t%s\n"`; `chewing_large_table2.cpp:145` and `chewing_large_table.cpp:699`: `"ChewingLargeTable2::load_text:%s\t%s\t%u\t%" G_GSIZE_FORMAT "\n"` and the corresponding `ChewingLargeTable::` format. `punct_table.cpp:102`: `"duplicated punctuations: %s\n"`. | Datagen `table::read_table_file`, `punct::read_punct_file`, `system::read_interpolation`, binary `write_chunk_file`; not every pin diagnostic is emitted. |
| Common utility option failures use `g_print("option parsing failed:%s\n", error->message)` after `g_option_context_parse`, e.g. `gen_binary_files.cpp:119-120`, `ngseg.cpp:118-119`, `import_k_mixture_model.cpp:268-269`. `taglib_read` uses `g_warning("un-expected tags:%s.\n", tmp)` and `g_warning("missed required tags: %s.\n", required_tag_str)` (`tag_utility.cpp:239,256`); `g_return_val_if_fail` sites at `:137,202,211,246` have GLib's own diagnostic. | Training `ToolArgs::parse`; data `interp_grammar::read`; KMM CLI; `oxpinyin-utils` `taglib`, which makes the pin's warnings and criticals. `interp_grammar::read` skips an unknown key/value without the pin's warning; GLib fatal-warning settings can affect the outcome. |
| `validate_k_mixture_model.cpp:25-168`: help `"Usage: validate_k_mixture_model <FILENAME>\n"`; errors `"no magic header in k mixture model.\n"`, `"word count in magic header is unexpected zero.\n"`, `"total freq in magic header is unexpected zero.\n"`, `"the word count doesn't match the total freq.\n"`, `"word count in magic header:%d\n"`, `"sum of word count in array headers:%d\n"`, `"the sum differs from word count.\n"`, `"total freq in magic header:%d\n"`, `"sum of freqs in array headers:%d\n"`, `"the total freq differs from sum of freqs.\n"`, `"in the array header of token %d:\n"`, `"word count is zero but has array items.\n"`, `"both word count and freq are unexpected zero.\n"`, `"word count in array header:%d\n"`, `"sum of word count in array items:%d\n"`, `"wrong arguments.\n"`, `"k mixture model validation failed.\n"`. | `oxpinyin-kmm` `validate`, `run_validate`, CLI error display. |
| `prune_k_mixture_model.cpp:31,73-75,116-144,175`: help `"Usage: prune_k_mixture_model -k <INT> --CDF <DOUBLE>  <FILENAME>\n"`, `"some wrong possibility is encountered:%f.\n"`, `"k:%d N:%d WC:%d n_0:%d n_1:%d\n"`, `"wrong arguments.\n"`, `"no magic header in k mixture model.\n"`, progress `"*"`, `"\n"`. `gen_k_mixture_model.cpp:42-47,274-304,355-356` uses help `"Usage: gen_k_mixture_model [--skip-pi-gram-training]\n                           [--maximum-occurs-allowed <INT>]\n                           [--maximum-increase-rates-allowed <FLOAT>]\n                           [--k-mixture-model-file <FILENAME>]\n                           {<FILENAME>}+\n"`, `"the m_WC integer in magic header overflows.\n"`, `"the m_total_freq in magic header overflows.\n"`, `"can't open file: %s.\n"`, `"error:%s.\n"`. `merge_k_mixture_model.cpp:26-27,110-116` prints `"Usage: merge_k_mixture_model [--result-file <RESULT_FILENAME>]\n"`, `"                             {<SOURCE_FILENAME>}+\n"`, and the same overflow formats. | KMM `prune`, `run_prune`, `run_generate`, `run_merge`, CLI error display. |
| `import_interpolation.cpp:77,85,246-290`: `"error: interpolation model expected.\n"`, option/config messages above, `"open %s failed!\n"`, `"empty file input.\n"`. `import_k_mixture_model.cpp:78,86,269-303`, `k_mixture_model_to_interpolation.cpp:62,69,197-199`: `"error: k mixture model expected.\n"`, `"empty file input.\n"` and their common option/config diagnostics. `export_k_mixture_model.cpp:122-144` additionally uses `"open %s failed.\n"`. `gen_ngram.cpp:54,62`, `gen_deleted_ngram.cpp:55,63`, `gen_unigram.cpp:56,90,99`, `estimate_interpolation.cpp:95`, `export_interpolation.cpp:51` use the option/config/load formats above. | Datagen `system::read_interpolation`, KMM `text::import`/CLI, training counter, lambda and emitter; `oxpinyin-utils` `k_mixture_model_to_interpolation` for its own lines. |
| Debug functions `lookup/pinyin_lookup2.cpp:52-68`, `lookup/phonetic_lookup.cpp:40-56` use `"max value: %f\n"`, `"values:"`, `"%f\t"`, `"\n"`; `storage/phonetic_key_matrix.cpp:330-340` uses `"Column:%ld:\n"`, `"ChewingKey:%s\n"`, `"ChewingKeyRest:%hd\t%hd\n"`. Other matrix/search and table-info debug print sites are under `#if 0`. | No corresponding externally reachable Rust debug writer identified; presence of a function alone does not establish reachability. |

## Utility coverage and gaps

The pinned lists are `utils/storage/Makefile.am`,
`utils/segment/Makefile.am`, and `utils/training/Makefile.am`. **GAP**
means the named pin-compatible executable and complete interface do not
currently exist; a partial functional analogue is not a drop-in tool.
The scope includes both installed and `noinst_PROGRAMS` utilities.

| Pin utility | Current oxpinyin analogue | Status |
| --- | --- | --- |
| `gen_binary_files` | `oxpinyin-utils` `gen_binary_files` | **Ported** at stage 5 (2026-10-31 UTC): the pin's name, options, messages, `.table` scanner, index DBMs, per-library chunks and `punct.bin`; registered `tool-diff` cases. `oxpinyin-datagen compile` is removed with the training family. |
| `import_interpolation` | `oxpinyin-utils` `import_interpolation` | **Ported** at stage 5 (2026-10-31 UTC): the pin's name, option, messages, stdin taglib grammar, unigram delta and bigram `SingleGram` store. |
| `export_interpolation` | `oxpinyin-utils` `export_interpolation` | **Ported** at stage 5 (2026-10-31 UTC): no options or argument check, the pin's model text, `get_all_items`' native key order and `taglib_token_to_string`. `oxpinyin-emitter` is removed. |
| `gen_pinyin_table` | `oxpinyin-utils` `gen_pinyin_table` | **Ported** at stage 5 (2026-10-30 UTC, #1436): the pin's name, options, messages, input scanner and output. |
| `gen_zhuyin_table` | `oxpinyin-utils` `gen_zhuyin_table` | **Ported** at stage 5 (2026-10-30 UTC): direct zhuyin rows, native calls, and registered `tool-diff` cases. |
| `ngseg` | `oxpinyin-utils` `ngseg` | **Ported** at stage 5 (2026-10-31 UTC): the pin's name, options, messages, `table.conf`/phrase-index/bigram inputs and output; `PhraseLookup::get_best_match` over `phrase_index.bin` and `bigram.db`; registered `tool-diff` cases. |
| `spseg` | `oxpinyin-utils` `spseg` | **Ported** at stage 5 (2026-10-31 UTC): the pin's name, options, messages and fewest-words dynamic program over `phrase_index.bin`; registered `tool-diff` cases. |
| `mergeseq` | `oxpinyin-utils` `mergeseq` | **Ported** at stage 5 (2026-10-31 UTC): the pin's name, options, messages, `TAGLIB_PARSE_SEGMENTED_LINE`, token validation and queue merge over `phrase_index.bin`; registered `tool-diff` cases. |
| `gen_unigram` | `oxpinyin-utils` `gen_unigram` | **Ported** at stage 5 (2026-10-31 UTC): the pin's name, `--table-dir` (accepted and unread, as its static is), `table.conf` load, the `SYSTEM_FILE`/`DICTIONARY` +1 sweep and both chunk saves. |
| `gen_ngram` | `oxpinyin-utils` `gen_ngram` | **Ported** at stage 5 (2026-10-31 UTC): the pin's options (`--skip-pi-gram-training` with `G_OPTION_FLAG_REVERSE`, `--bigram-file`), `std::getline`, `TAGLIB_PARSE_SEGMENTED_LINE`, the uni/pi-gram writes into the backend hash DBM and the chunk save. `oxpinyin-counter` is removed. |
| `gen_deleted_ngram` | `oxpinyin-utils` `gen_deleted_ngram` | **Ported** at stage 5 (2026-10-31 UTC): the same loop over `deleted_bigram.db`, no chunk save, and `ENODATA` where `gen_ngram` uses `ENOENT` on a refused chunk load. |
| `gen_k_mixture_model` | `oxpinyin-kmm generate` | **GAP** — subcommand and text working model. |
| `estimate_interpolation` | `oxpinyin-utils` `estimate_interpolation` | **Ported** at stage 5 (2026-10-31 UTC): the pin's C-double EM over `single_gram` rows and the facade unigram table, `token:%d lambda:%f` and `average lambda:%f` with the host's NaN spelling. `oxpinyin-lambda` is removed. |
| `estimate_k_mixture_model` | `oxpinyin-kmm estimate` | **GAP** — subcommand and text working model. |
| `merge_k_mixture_model` | `oxpinyin-kmm merge` | **GAP** — subcommand and text working model. Remaining with `estimate_`, `prune_` and `validate_k_mixture_model` as one family (2026-10-31 UTC): no chunk load, save or rename call, but each attaches `KMixtureModelBigram` to the backend DBM, so a store `FlexibleBigram` layer on all three backends comes first. |
| `prune_k_mixture_model` | `oxpinyin-kmm prune` | **GAP** — subcommand and text working model. |
| `import_k_mixture_model` | `oxpinyin-kmm import` | **GAP** — subcommand and text working model. |
| `export_k_mixture_model` | `oxpinyin-kmm export` | **GAP** — subcommand and text working model. |
| `k_mixture_model_to_interpolation` | `oxpinyin-utils` `k_mixture_model_to_interpolation` | **Ported** at stage 5 (2026-10-31 UTC): no options or arguments, the pin's messages, `std::cin` and `stdout`, taglib's GLib calls, the pin's aborts with its check macros' text not imitated ([eio-tool-check-messages](eio-tool-check-messages.md)), and registered `tool-diff` cases. `oxpinyin-kmm to-interpolation` is removed. |
| `validate_k_mixture_model` | `oxpinyin-kmm validate` | **GAP** — subcommand and text working model. |
| `eval_correction_rate` | `oxpinyin-utils` `eval_correction_rate` | **Ported** at stage 5 (2026-10-31 UTC): `evals2.text`, the pinyin index resolved through the phrase libraries, the `PhoneticLookup<1,1>` decode port (`oxpinyin-training::eval`) over the binary chunks and `bigram.db`, and the `correction rate:%f` line. `oxpinyin-eval` is removed. |

## Records affected by the ruling

These are policy decisions and future work, not claims that code has
already changed. Existing frozen case markers and dated evidence remain
as they were until the relevant implementation stage changes them.

| Record | Disposition and replacement |
| --- | --- |
| [1297-table-floats](1297-table-floats.md) | Revert decimal-only acceptance; host `fscanf` `%f` on the pin stream. **Done at stage 1.** |
| [1297-table-decimals](1297-table-decimals.md) | Revert complete `i32` acceptance; host `fscanf` `%d`. **Done at stage 1.** |
| [1297-table-lines](1297-table-lines.md) | Revert line-boundary substitution; pin's ordered scanner formats. **Done at stage 1.** |
| [1297-table-words](1297-table-words.md) | Revert unlimited/exact token substitution; `%255s`, `strcmp`, pin C-string filename handling. **Done at stage 1.** |
| [1297-addon-index](1297-addon-index.md) | Revert strict slot parsing; host `atoi`, then the pin's `guint8` conversion and checks. **Done at stage 1.** |
| [1297-user-decimals](1297-user-decimals.md) | Revert strict marker decimal acceptance; marker `fscanf` `%d`. **Done at stage 1.** |
| [1297-user-token](1297-user-token.md) | Revert complete token acceptance; `%255s` and `strcmp`. **Done at stage 1.** |
| [1297-chunk-payload](1297-chunk-payload.md) | Revert payload `write_all`; one host `write`, then the pin's branch and sync behavior. |
| [1297-tool-decimals](1297-tool-decimals.md) | Revert strict corresponding tool fields; host `atoi`/`atol`. **Done for `k_mixture_model_to_interpolation` at stage 5** (2026-10-31 UTC); the other KMM, counter and eval fields remain. |
| [1297-tool-special-values](1297-tool-special-values.md) | Revert Rust special-value spelling; the pin's `%f` formatted call and the externally required value. NaN sign is host/compiler dependent and not established here for every host. |
| [926-tool-tokenizers](926-tool-tokenizers.md) | Revert #1297's external-scanner amendments and the datagen strict-input decision for the corresponding pin utilities; use their `fgets`, `g_ascii_strtoull`, token/locale calls. Dictool's separate ibus frontend format has no libpinyin-pin call identified here. `k_mixture_model_to_interpolation`'s taglib and `atoi`/`atol` calls are the pin's at stage 5 (2026-10-31 UTC). |
| [interpolation2-grammar](interpolation2-grammar.md) | Revert the bounded `g_unichar_isgraph` approximation and omitted GLib warnings on matching pin-tool inputs; use GLib. The standalone decoder reader has no matching pin runtime `interpolation2.text` reader established here. `oxpinyin-utils` `taglib` uses GLib's calls (2026-10-31 UTC); the datagen reader is not yet moved. |
| [Policy row 109](compatibility-policy.md) | Revert to the pin's whole-file writable private `mmap` call. Whether its post-fini residual mapping must be retained is a separate open question. |
| [1297-punct-words](1297-punct-words.md) | **Not reverted as an unsafe defect:** the class (b)/(c) treatment for the undefined surrounding read stands. DBM record access stays in store; no out-of-bounds native read is introduced. |
| Policy row 39 and assertion records | **Stand:** reproduce the `LC_NUMERIC` side effect and preserve the established class (b)/(c) refusal and warning behavior. |

## Platform gaps

| Platform issue | Pin call or fallback | Verdict / unresolved limit |
| --- | --- | --- |
| No `mmap` | `HAVE_MMAP` controls `LIBPINYIN_USE_MMAP` (`memory_chunk.h:31-33`; `CMakeLists.txt:64`); callers use `MemoryChunk::load` without it (`pinyin.cpp:250-260`, `zhuyin.cpp:196-203`). | On Linux, a build without `mmap` uses the heap-read fallback and its exact calls. On Windows and macOS, `memmap2` is permitted under the [platform scope ruling](../source-policy.md#platform-scope-of-pinned-behaviour) (2026-10-31 UTC): platform-dependent behaviour, not a compatibility defect. |
| `g_fsync` | Ordinary Unix GLib headers macro-expand to `fsync`; Windows GLib's real function calls `_commit`. The separately compiled GLib Unix wrapper has `F_FULLFSYNC`/EINTR/fallback branches, but those are not the ordinary macro-expanded call. | On Linux, respect the actual host header/build branch. On Windows and macOS the [platform scope ruling](../source-policy.md#platform-scope-of-pinned-behaviour) permits a portable Rust abstraction; a difference there is platform-dependent behaviour. |
| `g_rename` and plain `unlink` | Ordinary Unix `g_rename` expands to `rename`; Windows GLib converts UTF-8 to UTF-16 and calls `MoveFileExW`. The pin separately uses plain C `unlink`. | On Linux, do not substitute GLib wide-name unlink for the pin's plain C call. On Windows, Rust's rename adds a retry; under the [platform scope ruling](../source-policy.md#platform-scope-of-pinned-behaviour) that is platform-dependent behaviour, not a compatibility defect. |
| `O_BINARY` | The pin defines it as zero if unavailable (`memory_chunk.h:39-40`). | Obtain the target constant from the C headers, not a guessed Rust numeric value. |
| Windows CRT text mode | Config and tool streams use `"r"`/`"w"` without `b` or `t`; behavior depends on the host CRT's default. | **UNSETTLED:** [Microsoft documents](https://learn.microsoft.com/en-us/cpp/c-runtime-library/file-translation-constants?view=msvc-170) CRLF and CTRL+Z translation in text mode, but this record did not read the exact CRT implementation or establish a native Windows pin build. Platform-dependent behaviour under the [platform scope ruling](../source-policy.md#platform-scope-of-pinned-behaviour), recorded here, not a compatibility defect. |
| Tool input streams on Windows | `std::ifstream::open(const char *)` and `fopen` take a narrow name, which the C runtime converts through the active code page; MSVC's `std::ifstream` opens in text mode, translating CRLF to LF and treating Ctrl+Z as end of file. | **UNSETTLED:** `oxpinyin-input`'s `tool::LineReader::open` converts a UTF-8 name only; a name that is not UTF-8 does not open. Unix passes the bytes. `LineReader` reads a Rust `File`'s raw bytes, so a CR before LF stays in the line and a Ctrl+Z does not end the input: on Windows it can differ from the pin's text-mode stream. This is platform-dependent behaviour under the [platform scope ruling](../source-policy.md#platform-scope-of-pinned-behaviour), not reproduced, and unverified until Windows runs the tools. |
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
  src/locale.rs       locale calls at the pin's reader/writer sites
  src/tool.rs         tool streams, numeric and GLib text adapters
  src/native.rs       libc calls; the C library's stdout/stderr per platform
```

`scan_system_config` uses the actual ordered `fopen`/`fscanf` stream and
returns the raw `f32` lambda. The training loader and
`lm::set_lambda_from_table_conf` must share it. The current line-only
rational reader cannot be the pin scanner: nonfinite and out-of-range
values need their actual `f32`, not a fallback when conversion to the
current rational `Lambda` fails. `scan_user_config` and
`write_user_config` preserve the marker's native calls and error checks.
Chunk helpers own the selected open/read/map/write/sync/close sequence;
path and diagnostic helpers own their corresponding pin calls.

The native modules use scoped unsafe with `// SAFETY:` on every block.
Under the 2026-10-31 UTC ruling they call the `libc` crate and
`glib-sys`, and declare by hand only what neither binds: the C library's
`stdout` and `stderr` objects, once per platform in `native.rs` for the
library and the tools (Linux, Apple/FreeBSD/DragonFly and Windows; any
other target is a `compile_error!`), and GLib's `g_utf8_skip` for the
tools. The crate has no build script.
No borrowed raw pointer escapes the safe facade. `oxpinyin-data` becomes
`forbid(unsafe_code)` after its mmap implementation moves; user,
runtime, core and engine retain their existing forbids. C facades retain
ABI marshalling; store retains DBM calls. `oxpinyin-data`'s config,
mapping and `pin_stderr` call mechanics, `oxpinyin-user`'s persistence
file calls, and `oxpinyin-runtime`'s config probing move to the boundary;
their domain logic stays put.

**Do not start implementation before #1175's last extraction lands.** Then each numbered stage is a separately reviewable,
independently gated change. Keep the selected backend and applicable
verification-cell rules from the contributor rules; do not alter frozen cases or
specifications in advance of the stage that owns them.

1. **Library config read/write and locale** (implemented 2026-10-28
   UTC, with the rulings above). Create the minimal boundary
   and C shim; move system/user config scans, marker writing and locale
   calls together, including the shared lambda scanner. Gate both facade
   paths and the touched crates; preserve class (b)/(c) handling. Since
   2026-10-31 UTC `config.c` is gone: `oxpinyin-input` `native.rs` makes
   the same `setlocale`, `fopen`, `fscanf`, `feof`, `atoi`, `fprintf`
   and `fclose` calls through the `libc` crate, with the same format
   strings, call order and result checks.
2. **Chunks.** Move chunk load/map/save calls, single payload write,
   native sync and mapping ownership. Gate chunk readers/writers and
   both facades independently; make data `forbid(unsafe_code)` when its
   last unsafe mapping moves. Row 109's call is in this stage.
3. **Save and rename.** Move pin path building, unlink, staging and
   rename calls while preserving pinyin's two-pass and zhuyin's
   interleaved order. Gate failure ordering and both facades; DBM calls
   stay in store.
   Path building takes over the `g_build_filename` row of the #1297
   classification (`glib-libc-inventory.md`, row 18; maintainer decision
   2026-10-31 UTC): it keeps the pin's paths, including a name that begins
   with a separator, which `Path::join` does not, and it settles the
   Windows/macOS question and the `glib-sys` dependency of
   `oxpinyin-input`'s library path.
4. **Diagnostics** (implemented 2026-10-30 UTC, #1437). Move the
   library's raw formatted writes and GLib diagnostics to the correct
   native calls; gate exact bytes, channels, ordering and existing
   class-(b)/(c) warnings. oxpinyin's own class-(c) warnings stay the
   facades' `g_log` in the pin's domain; the pin has no call there.
   Since 2026-10-31 UTC `diagnostic.c` is gone: each line is
   `libc::fprintf` on the C library's own `stderr` object.
5. **Tools, in small independently gated slices.** First add the
   tool-only stream and GLib option/text boundary. Then port each
   utility family with the pin names and interface, including the two
   missing table generators. Move KMM working files to native backend
   DBMs through store; a change touching backend code follows the
   three-cell path rule. Gate each utility or cohesive small family on
   its own rather than waiting for the whole tools set. **In progress:**
   the boundary (`oxpinyin-input` `tools`) and `gen_pinyin_table`
   (`oxpinyin-utils`) landed together (2026-10-30 UTC, #1436), so CI
   builds and tests the boundary through its first user;
   `gen_zhuyin_table` followed (#1450). `k_mixture_model_to_interpolation`
   (2026-10-31 UTC) adds the shared taglib port and the `assert` and
   `abort()` sites a `tool-diff` case reproduces on both sides as
   SIGABRT, and replaces `oxpinyin-kmm to-interpolation`. The same change
   moves the three tools off `native/tool.c` and the C++ stream shim onto
   `libc`, `glib-sys` and `std::io` (2026-10-31 UTC ruling, item 5). The
   trainer runs the utility's conversion in memory; it still links GLib,
   because taglib's character classes and warnings are GLib's calls
   (`g_unichar_isspace`/`isgraph`, `g_log`), so the Windows lane keeps
   excluding `oxpinyin-training` until GLib-on-Windows. The DBM-attached KMM family
   (`estimate_`, `merge_`, `prune_`, `validate_k_mixture_model`) needs
   neither stage 2 nor stage 3 and is the next stack. Utilities that
   load chunks (`MemoryChunk::load`, e.g. `ngseg`, `gen_binary_files`)
   follow stage 2's chunk calls.

Each stage derives expected behavior from the pin and its native library
sources before using a differential as a gate. The present docs-only
change needs the requested fmt gate only; it does not run those stages.

## Open questions — differential confirmation

Keep these open. Source-derived observations in the tables are not a
resolution, and no confirmation was run for this document:

1. Trailing bytes after a chunk's declared content.
2. A missing `table.conf` — settled by ruling D3 above, not by a
   differential.
3. Missing system DBMs.
4. The external consequence of BDB `tmp_db->sync(m_db, 0)` in save.

The row-109 post-fini residual mapping is separately open, as ruled
above; this document does not decide whether to retain the residual.

Stage 5 follow-up: `gen_zhuyin_table` now uses the same tool boundary and
shared phrase tree as `gen_pinyin_table`. The registered
[`tool-diff` runner](../runbooks/tool-diff.md) compares both generators'
case files, including source-derived class (b) refusal records.
