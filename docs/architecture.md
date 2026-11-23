# Architecture

## Crates

Three workspace crates are published. The core library holds the engine and the two C crates expose the C interfaces. The three private crates supply the command-line tools.

| Crate | Role | unsafe | Portable | Ships |
|---|---|---|---|---|
| oxpinyin | The library. Its modules are in the [module table](#library-modules). | deny, with scoped exceptions in `input` and `store` | yes | yes |
| oxpinyin-capi | `libpinyin.so.15`, all 79 `pinyin_*` exports, and the SONAME/header/pkg-config through cargo-c. | allow | Linux | yes |
| oxzhuyin-capi | `libzhuyin.so.15`, with 52 symbols and its own SONAME. The `--enable-libzhuyin` counterpart uses the same engine/chewing surface as oxpinyin-capi. | allow | Linux | yes |
| oxpinyin-tool-support | Native command-line I/O, DBM adapters and C-facade drivers shared by the utilities and REPL. | deny, with scoped exceptions in `input`, `pinyin` and `zhuyin` | Linux, macOS | never |
| oxpinyin-utils | libpinyin's utilities under their own names and interfaces, using the library's native I/O and storage operations (`docs/findings/external-io-boundary.md`): the table generators, the K-mixture-model utilities on the pin's `FlexibleBigram` container, `k_mixture_model_to_interpolation`, the segmentation trio, the storage trio and the five n-gram/interpolation utilities (`gen_unigram`, `gen_ngram`, `gen_deleted_ngram`, `estimate_interpolation`, `eval_correction_rate`). | forbid except the segmentation stdout-close boundary | Linux, macOS | never |
| oxpinyin-repl | One nonshipping binary with ten interactive parser, lookup and C-facade modes. `just repl` supplies disposable mini data; no timing loops. | forbid | Linux, macOS | never |

The utilities and REPL depend on `oxpinyin-tool-support`. It depends on the core library and both C crates. The published crates have no dependency on any private crate, including for tests.

The measurement package is independent of the product crates.
`tools/measurement` owns the nonshipping `bisect` and `system-import-profile`
binaries. They load either subject through the C ABI with `libloading`.
Shared workloads contain no OS or CPU selection. Whole platform modules own
Linux profiling and allocator diagnostics, macOS process counters and image
identity, and Windows process counters, file and dependency identity.
Only module declarations select platforms.

Every product crate links GLib, because the library links it ([native libraries](#native-libraries-and-unsafe-code)).

## Library modules

The library is one crate. Its top-level modules divide the behaviour.
Each row names what the module owns.
The next section states which modules it may depend on.

| Module | Owns |
|---|---|
| `options`, `static_strings`, `string_tables`, `ucs4`, `direct`, `chewing_key`, `chewing_key_data`, `full_pinyin_index`, `syllables`, `vocab`, `zhuyin_map`, `scheme`, `parser` | The parser layer: option bits, the frozen string and content tables, UCS-4 phrase text, the packed `ChewingKey` and its renderers, the syllable inventory and the full-pinyin, double-pinyin and zhuyin parsers. `vocab` also holds the one definition of the pin's `novel_types.h` types and constants: `PhraseToken`, the alias of `phrase_token_t` (`typedef guint32`), the token masks, the library count and library ids, `null_token`, `sentence_start`, `MAX_PHRASE_LENGTH`, and `PHRASE_INDEX_LIBRARY_INDEX` and `PHRASE_INDEX_MAKE_TOKEN`. The public `vocab` module owns these names and exposes phrase text from the private `ucs4` implementation. The public `parser` module exposes the phonetic keys and parser types from their private implementation files. The only item re-exported at the crate root is `OptionBits`. |
| `graph` | The weighted segment graph and the decoder over it. |
| `model`, `sentence_facts`, `single_gram` | The dictionary and language-model contracts, the sentence facts the facades share and the `SingleGram` value layout and operations. |
| `engine` | Session state, candidate construction, selection, constraints, training plans and decoding. It is storage-free: it reads the system and user models only through the `model` traits. |
| `store` | The ordered key–value store traits and the three DBM backends, `DefaultStore` included. |
| `input` | The external-I/O boundary: the pin's C-library and GLib calls for configuration, locale, chunk files, the save path and diagnostics. |
| `data` | The shared on-disk formats and the system-data readers: the row and chunk formats, the pinyin and phrase tables, the phrase libraries, the bigram, punctuation, `table.conf` and the user-file names. |
| `user` | The user overlays, the learning state and the profile persistence of one session: the user phrase arenas, the user pinyin index and phrase table, the user bigram, the live system items and the save cycle. |
| `runtime` | The engine assembly: it opens a system data directory and a user store, and hands out sessions over the merged models. |
| `facade` | The orchestration that the two C crates share: the context and instance state machines, the parse modes, the candidate snapshots, the cursor and offset laws, import, export and prediction. |
| `marshal` | Safe conversions between C-facing bytes and Rust values. |

`fixture_doubles` and `parser_rows` are test-only modules.

### Module dependencies

The base is the parser layer with `graph`, `model`, `sentence_facts` and `single_gram`.
Base modules may depend on each other. Callers name each item through its owning module; `OptionBits` is available at the crate root.
Every other module may depend on the base and on the modules that this table names.
The table states the rule for production code.

| Module | May depend on |
|---|---|
| Base | Other base modules. |
| `engine` | The base only. |
| `store` | No other module. |
| `input` | No other module. |
| `data` | `store`, `input`. |
| `user` | `data`, `store`, `input`. |
| `runtime` | `engine`, `user`, `data`, `input`. |
| `facade` | `runtime`, `engine`, `user`, `data`, `input`, `marshal`. |
| `marshal` | The base only. |

The rules that follow from the table:

- No module depends on `facade`. The C crates depend on `facade` and on the modules below it.
- No module below `runtime` depends on `runtime`.
- `engine` depends on no storage module: not on `store`, `data`, `user` or `runtime`.
- Only `data` and `user` name `store` directly. `runtime`, `facade` and the C crates reach the backend through them.
- `store` and `input` depend on no other module, so the native bindings stay below every behaviour.

Tests may cross these edges to share fixtures. For example, the `data` tests open the fixture directories that `user`'s test support builds.

The data generators in `oxpinyin-utils` use `data::chunk_format` for the `MemoryChunk` header and checksum and `data::phrase_library` for the `SubPhraseIndex` payload.
They use `data::row_format` for DBM keys and index and punctuation values, `single_gram` for bigram values, and `data` for library names.
Readers and writers share one layout definition per format.
Implementation files are private by default. Owners re-export the items callers need. Public submodules name subsystems, formats, backends or command boundaries.

The private tool-support crate calls shared engine, storage and native I/O operations through `oxpinyin::__internal`. Rustdoc hides this unsupported workspace interface. Any Rust crate can still import it. The split follows libpinyin’s separation of internal archives and utility code.

The fixture `Dictionary`/`LanguageModel` doubles are one source file, `tests/common/fixture_doubles.rs`.
Each test target that needs them includes the file with `#[path]`, so no shared test crate can close a dependency cycle through the library.
The production build graph carries no test doubles.

### Native libraries and unsafe code

The library links native libraries.
This is a rule, not a build option:

- Every build links the C library through the `libc` crate and GLib through `glib-sys`.
  `input`'s native module makes the pin's `g_build_filename`, `g_free` and `g_rename` calls, so GLib is not optional.
  The utilities use the same GLib dependency for their native option calls.
- See [backend selection](#backend-selection) for the native store dependency.

Unsafe code is confined by module:

- The crate root denies `unsafe_code`.
- Each top-level module that needs no unsafe code carries `#[forbid(unsafe_code)]` on its `mod` line in the crate root. Only `input` and `store` do not.
- In `input`'s production code, only the `native` module carries a scoped `#[expect(unsafe_code)]`. The one test-only exception is `input::config::tests::numeric_locale`, which queries and restores the process numeric locale.
- In `store`, only each backend's binding modules carry one: `bdb::ffi`, `kyotocabinet::bridge`, `kyotocabinet::ffi` and `tkrzw::bridge`.
- Every unsafe block carries a `// SAFETY:` comment. The private tool I/O module shares the core FILE and CRT primitives.
- Each C crate defines its own unsafe ABI marshalling helpers. Clippy checks their blocks for safety comments, and review checks the reasons. The core `marshal` module forbids unsafe code.

### Chewing keys

The parser layer holds the shared chewing-key machinery: the packed `ChewingKey` (the pin's two-byte bitfield word, `#[repr(C)]`, with size and alignment asserted), its six display renderers and the frozen `content_table` and `chewing_key_table`.
The capi facade's `keys.rs` holds only `#[no_mangle]` wrappers and failure-shape handling.
The three `parse_one_key` seams stay methods on the scheme parsers, because they consume parser tables that the parser layer owns (`zhuyin_map`, the double-pinyin scheme tables and the alias-gated syllable inventory).
The zhuyin cut follows the pin's boundary: a facade file, a version script and a configure flag, never a split inside the engine.
`ENABLE_LIBZHUYIN` does not remove chewing from `libpinyin.so`, and `libpinyin.ver` always exports `pinyin_parse_chewing` and `pinyin_get_zhuyin_string`.
No symbol is feature-gated, so a bopomofo-only facade would be additive: a new facade crate that depends on the library, with no existing export moved.

### Facades

Both C crates use the shared context and instance state machines in `facade`.
The private support crate owns the C-facade drivers and the parser and lookup adapters. It calls the existing C entry points and core graph, matrix and decoder operations. The utilities and REPL forbid unsafe code.
`runtime` assembles the dictionary, language-model and user handles for each session.
Each C crate forwards its backend selection to the library.

## Backend selection

Every build of the library, including the C libraries, enables exactly one of the `tkrzw`, `kyotocabinet` and `bdb` features.
The root `build.rs` checks this before probing native libraries or generating code.
`DefaultStore` is a `#[cfg]` alias for the selected backend.
Cargo enables `tkrzw` by default.
Select a peer with `--no-default-features --features <backend>`.
See the [backend runbook](runbooks/backends.md) for commands.
This follows libpinyin's `--with-dbm` model.
Runtime dispatch and fallback are not added.

The store backends are system native-library dependencies, verified on Linux and buildable on macOS through Homebrew.
macOS and Windows development and CI use the platform's tkrzw source described in [setup](runbooks/setup.md).
Linux runs no tkrzw code: every Linux build, CI included, uses Kyoto Cabinet from Ubuntu's packages ([Linux setup](runbooks/setup.md#linux), [verification rule 10.2](runbooks/verification.md#10-ci-merge-conditions)).
Debian release packages and oracle/reference images retain Debian's library.
The peer matrix remains on Linux.

A save makes the pin's synchronisation call for the selected backend (`ngram_tkrzwdb.cpp:80`, `ngram_kyotodb.cpp:102` and `ngram_bdb.cpp:103` at `074a2219`).
Durability is therefore the backend's durability for that call; oxpinyin promises nothing beyond it.

## Dependencies

The [Rust/C++ interoperability ruling](findings/compatibility-policy.md#rust-cpp-interoperability) governs native bindings.

A Rust library is used as Rust.
The tree has no pure-Rust database backend.
`glib-sys` binds a C library.
Kyoto Cabinet, tkrzw and Berkeley DB link to the distribution's libraries.
Berkeley DB is used through its official C API, `db.h`, with bindgen over system headers.
tkrzw binds with CXX over the concrete classes the pin uses (`src/store/tkrzw/bridge.rs`, with the semantic adapter `adapter.cc`).
The library finds the backends with `system-deps`: tkrzw and Kyoto Cabinet through their `.pc` files, and Berkeley DB without pkg-config, because the distributions and Homebrew install no `.pc` file for it.
The `SYSTEM_DEPS_BDB_*` overrides reach a Berkeley DB outside the defaults ([discovery and overrides](runbooks/backends.md#discovery-and-overrides)).

Kyoto Cabinet is used as libpinyin uses it.
Kyoto Cabinet binds with CXX over the concrete classes the pin uses (`src/store/kyotocabinet/bridge.rs`, with the semantic adapter `adapter.cc`), under the `kyotocabinet` feature.
It constructs `TreeDB`, `HashDB`, `StashDB` and `ProtoTreeDB` from build-time headers.
No bridge function returns `Result`, so a C++ exception terminates the process, as it ends the pin's.
Like libpinyin, it depends on the system library's C++ ABI.
Constructing the concrete classes directly, as the pin does, keeps the pin's libkyotocabinet residency; `kclangc.h` reaches every class through `PolyDB`, which keeps more of the library resident.
The facades import 68 `kyotocabinet::` names, 67 among the pin's 77.
On Linux, the cargo cdylibs hide the cxx runtime symbols of both bridges through `--exclude-libs`; on macOS, the cargo cdylibs export them and the shipped relink does not ([export gate](packaging.md#export-gate)).

`input` calls the C library through the `libc` crate, with no build step; the [C-library call and macro-check rules](findings/compatibility-policy.md#external-io) apply.
The core declares the C library's `stderr` and the private support crate declares `stdout`, which `libc` does not name. Both crates call GLib through `glib-sys`. The private crate supplies the utilities' GLib option calls and reads the C++ streams' lines with `std::io`.

### Dependency decisions

- `thiserror` is used.
  Error types derive `Display`, `Error` and plain `From` conversions with it.
  A conversion with its own semantics, such as `TableError`'s `From<StoreError>`, stays handwritten.
- `fst` is not used for the prefix tables.
  It was slower there than the existing binary search.
- `zerocopy` is not used.
  The fixed headers contain only a few little-endian fields.
  Some readers must check fields in order, and variable-length records still need explicit stride calculations and bounds checks.
- `arrayvec` is not used.
  The bounded collections already stay inline, as the `SmallVec` `QueryKey` in `user::user_index` does, or use plain arrays, as `engine::nbest` does.

## External I/O

`input` owns the bidirectional external calls, in its scoped unsafe modules.
Library configuration read/write and locale belong to `oxpinyin::input`.
The [external I/O boundary](findings/external-io-boundary.md) covers chunks, save and rename, diagnostics, and tools.
All five stages are implemented.
The C crates retain ABI marshalling.
`store` retains the DBM backends.

`data` owns the native system-data readers.
`user` owns the libpinyin session's user data: the user phrase arenas, the pin's user bigram container, the live system items and the user-file persistence over `DefaultStore`. There is no format-version row, which matches the unversioned user files ([user store §4](findings/user-store.md#4-persistence-and-the-save-cycle)). `UserStore::open_transient` opens the pin's NULL-user-directory session in memory, with no profile reads or writes; facade train and save refuse for it before they change training, locale or dirty state.

### Threat model

The assets are host-process memory, learned profile data, compatibility with the pin, and release integrity.
The input boundaries are C arguments, input text, configuration, and on-disk system and user data.
Callers must meet the [Pinyin Safety rules](https://github.com/shenghaoc/oxpinyin/blob/main/capi/src/lib.rs#L23) and [Zhuyin Safety rules](https://github.com/shenghaoc/oxpinyin/blob/main/zhuyin-capi/src/lib.rs#L23), including handle and borrowed-object lifetimes.
The [native libraries](#native-libraries-and-unsafe-code) run in the host process and are not sandboxed.
Malformed data and native failures can terminate that process under the [deliberate-abort policy](findings/compatibility-policy.md#failure-behaviour).

**Shared static-slot rule.** A caller must finish reading or copying a returned key or key-rest pointer before any call from any context that can overwrite the same static slot, and must prevent such calls during that read or copy.
Key and key-rest getters use separate slots, and the two C libraries have separate storage ([Pinyin slots](https://github.com/shenghaoc/oxpinyin/blob/main/capi/src/cursor.rs#L28), [Zhuyin slots](https://github.com/shenghaoc/oxpinyin/blob/main/zhuyin-capi/src/cursor.rs#L18)).

Learned profile data follows the [save cycle](findings/user-store.md#4-persistence-and-the-save-cycle): Pinyin stages files and then renames them individually; Zhuyin can report success despite soft write, rename, or marker failures ([save implementation](https://github.com/shenghaoc/oxpinyin/blob/main/src/user/persistence.rs#L2031)).
The explicit chunk fsync result is ignored, and the save path does not sync directories ([native chunk save](https://github.com/shenghaoc/oxpinyin/blob/main/src/input/native.rs#L603)).
A save that reports success does not guarantee that every file was replaced, that the profile was replaced as a whole, or that the result survives power loss.
[Raw store scans](https://github.com/shenghaoc/oxpinyin/blob/main/src/store/mod.rs#L565) define their own limits when writes occur during visits.
Live `memmap2` backing files must not be modified or truncated; using a mapping after such a change can cause undefined behavior ([mapping owner](https://github.com/shenghaoc/oxpinyin/blob/main/src/input/chunk.rs#L240)).

Compatibility remains governed by the [C and persistent-data contract](findings/compatibility-policy.md#decision-order), including its accepted safety differences.
For release integrity, [external workflow actions](../.github/workflows/) are pinned by commit SHA, and the [toolchain file](../rust-toolchain.toml) and lockfiles ([Cargo](../Cargo.lock), [fuzz](../fuzz/Cargo.lock), [uv](../uv.lock)) record build inputs.
The distribution images are intentionally rolling ([packaging details](packaging.md#release-artifacts-release-packagesyml)); [vcpkg.json](../vcpkg.json) has no builtin baseline.
The [release `SHA256SUMS`](packaging.md#release-artifacts-release-packagesyml) verifies artifact integrity against a trusted manifest; it does not establish publisher authenticity.

## Centralized assembly

Decodable engines are constructed in exactly one place, `runtime`.
It assembles the system tables, the unigram model, λ, a persistent or transient user store, the addons and the punctuation.
`facade`, the C crates and future adapters consume that assembly.
Nothing else assembles equivalents independently.
This prevents native and C-ABI paths from silently diverging.

`runtime` connects the system and user models to the engine.
Its deliberate pinned algorithms are:

- The user-count overlay feed.
  The arithmetic lives in `data`'s `*_with_user_delta` methods.
  Gate 2 merges whole rows through `load_merged_row` over `model::MergedGram`.
- Suggestion row order through `runtime::merge_suggestion_rows`.
  The C crates consume it through `oxpinyin::facade::merged_suggestions`.

Each algorithm's source cites the pin.

## Data path

There is one native reader set per backend.
`data` lazily reads the pinyin and phrase DBMs, per-library `MemoryChunk` files, `bigram.db`, `punct.bin` and the addon DBM pair.
Each reader opens a handle and performs a point read.
Chunk files use mmap and checksums.
Nothing is scanned at open.

`table.conf` is read once through `data::table_info`, the `input` scanner and `SystemTableInfo2::load`'s own calls.
Its rows determine which libraries exist, their filenames and which calls accept them.
Its header supplies λ and the versions used to check the user profile.
`runtime` opens the same system directory as `pinyin_init` for that backend.
libpinyin's own `gen_binary_files` → `import_interpolation` → `gen_unigram` sequence, which `oxpinyin-utils` provides under those names, produces it; the committed `fixtures/w3` sets are that sequence's output over the mini subset of the model.
The caller supplies the system directory to `Runtime`, which retains it.
A distribution layout is not auto-detected.

## Portability

The engine session API stays independent of frameworks.
It exposes parsing, candidate iteration, selection, training and reset operations.
Configuration and backend handles are injected as data; `runtime` owns storage paths.
Platform services and `cfg(target_os)` are not added to portable modules.
Frontends handle key dispatch and preedit display through the C API.
Sessions are instance-per-context and suitable for the TSF, IMK and ArkTS main-thread models.
See the [native Rust policy](findings/compatibility-policy.md#native-rust-implementation).

## Supported external interfaces

The [interface ruling](constitution.md#external-interfaces) sets the supported interfaces: the C APIs/ABIs and the pin-compatible tools, with every crate's Rust interface internal.
The ABI gate checks the exact export set and how a consumer releases each pointer the C ABI returns or writes to an out-parameter. It exercises borrowed pointers through their owner's lifetime. See the [allocator-pairing register](findings/abi-allocator-pairing.md).

## Configuration

The C adapters retain the typed session settings and apply live option words through the session's setters.
Neither libpinyin nor the engine has a settings store.
The engine has no configuration-file parser or layer merger.
Engine weights and language-model order are never user configuration.

## Decoder

The decoder uses a weighted SegmentGraph with index-based arenas, not references.
Current EdgeKind variants are Exact, Segmentation and Incomplete.
Fuzzy, Typo and Abbrev change output and are Stage 3 candidates behind an opt-in compile flag.
Its Rust API is internal and can change under the [interface ruling](constitution.md#external-interfaces).
The graph and decoder follow the pin's parsing and lookup rules ([parser SPEC](findings/parser-spec.md)).
