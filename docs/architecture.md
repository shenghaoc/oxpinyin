# Architecture

## Crates

The workspace has one library crate and four crates that use it.
The library holds the behaviour; the other crates hold only their own interfaces.

| Crate | Role | unsafe | Portable | Ships |
|---|---|---|---|---|
| oxpinyin | The library. Its modules are in the [module table](#library-modules). | deny, with scoped exceptions in `input` and `store` | yes | yes |
| oxpinyin-capi | `libpinyin.so.15`, all 79 `pinyin_*` exports, and the SONAME/header/pkg-config through cargo-c. | allow | Linux | yes |
| oxpinyin-zhuyin-capi | `libzhuyin.so.15`, with 52 symbols and its own SONAME. The `--enable-libzhuyin` counterpart uses the same engine/chewing surface as oxpinyin-capi. | allow | Linux | yes |
| oxpinyin-utils | libpinyin's utilities under their own names and interfaces, through the library's `tools` feature (stage 5 of `docs/findings/external-io-boundary.md`): the table generators, the K-mixture-model utilities on the pin's `FlexibleBigram` container, `k_mixture_model_to_interpolation`, the segmentation trio, the storage trio and the five n-gram/interpolation utilities (`gen_unigram`, `gen_ngram`, `gen_deleted_ngram`, `estimate_interpolation`, `eval_correction_rate`). | forbid | Linux, macOS | never |
| oxpinyin-repl | One nonshipping binary with ten interactive parser, lookup and C-facade modes. `just repl` supplies disposable W3 data; no timing loops. | forbid | Linux, macOS | never |

Every crate links GLib, because the library links it ([native libraries](#native-libraries-and-unsafe-code)).

## Library modules

The library is one crate. Its top-level modules divide the behaviour.
Each row names what the module owns.
The next section states which modules it may depend on.

| Module | Owns |
|---|---|
| `options`, `static_strings`, `string_tables`, `ucs4`, `direct`, `chewing_key`, `chewing_key_data`, `full_pinyin_index`, `syllables`, `vocab`, `zhuyin_map`, `scheme`, `parser` | The parser layer: option bits, the frozen string and content tables, UCS-4 phrase text, the packed `ChewingKey` and its renderers, the syllable inventory and the full-pinyin, double-pinyin and zhuyin parsers. `vocab` also holds the one definition of the pin's `novel_types.h` types and constants: `PhraseToken`, the alias of `phrase_token_t` (`typedef guint32`), the token masks, the library count and library ids, `null_token`, `sentence_start`, `MAX_PHRASE_LENGTH`, and `PHRASE_INDEX_LIBRARY_INDEX` and `PHRASE_INDEX_MAKE_TOKEN`. The crate root re-exports them. |
| `graph` | The weighted segment graph and the decoder over it. |
| `model`, `scoring`, `sentence_facts`, `single_gram` | The dictionary and language-model contracts, the scoring error, the sentence facts the facades share and the pin's `SingleGram` code. |
| `engine` | Session state, candidate construction, selection, constraints, training plans and decoding. It is storage-free: it reads the system and user models only through the `model` traits. |
| `store` | The ordered key–value store traits and the three DBM backends, `DefaultStore` included. |
| `input` | The external-I/O boundary: the pin's C-library and GLib calls for configuration, locale, chunk files, the save path, diagnostics and, under `tools`, the utilities' calls. |
| `data` | The shared on-disk formats and the system-data readers: the row and chunk formats, the pinyin and phrase tables, the phrase libraries, the bigram, punctuation, `table.conf` and the user-file names. |
| `user` | The user overlays, the learning state and the profile persistence of one session: the user phrase arenas, the user pinyin index and phrase table, the user bigram, the live system items and the save cycle. |
| `runtime` | The engine assembly: it opens a system data directory and a user store, and hands out sessions over the merged models. |
| `facade` | The orchestration that the two C crates share: the context and instance state machines, the parse modes, the candidate snapshots, the cursor and offset laws, import, export and prediction. |
| `marshal` | The safe C marshalling helpers and the macros that the two C crates expand at the ABI boundary. |
| `tool` | Under the `tools` feature only: the pin test programs' parser surface for the REPL and the utilities. |

`fixture_doubles` and `parser_rows` are test-only modules.

### Module dependencies

The base is the parser layer with `graph`, `model`, `scoring`, `sentence_facts` and `single_gram`.
Base modules may depend on each other and on the items that the crate root re-exports.
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
| `tool` | The base only. |

The rules that follow from the table:

- No module depends on `facade`. The C crates depend on `facade` and on the modules below it.
- No module below `runtime` depends on `runtime`.
- `engine` depends on no storage module: not on `store`, `data`, `user` or `runtime`.
- Only `data` and `user` name `store` directly. `runtime`, `facade` and the C crates reach the backend through them.
- `store` and `input` depend on no other module, so the native bindings stay below every behaviour.

Tests may cross these edges to share fixtures. For example, the `data` tests open the fixture directories that `user`'s test support builds.

The data generators in `oxpinyin-utils` use the `MemoryChunk`/`SubPhraseIndex` constants and the checksum from `data::chunk_format`.
They use the four DBM row schemas and the library-name tables from `data::row_format` and `data::system_files`.
Readers and writers therefore share one layout definition.

The fixture `Dictionary`/`LanguageModel` doubles are one source file, `tests/common/fixture_doubles.rs`.
Each test target that needs them includes the file with `#[path]`, so no shared test crate can close a dependency cycle through the library.
The production build graph carries no test doubles.

### Native libraries and unsafe code

The library links native libraries.
This is a rule, not a build option:

- Every build links the C library through the `libc` crate and GLib through `glib-sys`.
  `input`'s native module makes the pin's `g_build_filename`, `g_free` and `g_rename` calls, so GLib is not optional.
  The `tools` feature adds the utilities' GLib option calls; it does not add GLib.
- Every build links exactly one store backend: tkrzw, Kyoto Cabinet or Berkeley DB ([backend selection](#backend-selection)).

Unsafe code is confined by module:

- The crate root denies `unsafe_code`.
- Each top-level module that needs no unsafe code carries `#[forbid(unsafe_code)]` on its `mod` line in the crate root. Only `input` and `store` do not.
- In `input`'s production code, only the `native` module and, under `tools`, the `tool` module carry a scoped `#[expect(unsafe_code)]`. The one test-only exception is `input::config::tests::numeric_locale`, which queries and restores the process numeric locale.
- In `store`, only each backend's binding modules carry one: `bdb::ffi`, `kyotocabinet::bridge`, `kyotocabinet::ffi` and `tkrzw::bridge`.
- Every unsafe block carries a `// SAFETY:` comment, and no raw pointer leaves these modules.
- The two C crates allow unsafe code at the ABI boundary. The `marshal` macros generate their unsafe blocks there; `marshal` itself forbids unsafe code.

### Chewing keys

The parser layer holds the shared chewing-key machinery: the packed `ChewingKey` (the pin's two-byte bitfield word, `#[repr(C)]`, with size and alignment asserted), its six display renderers and the frozen `content_table` and `chewing_key_table`.
The capi facade's `keys.rs` holds only `#[no_mangle]` wrappers and failure-shape handling.
The three `parse_one_key` seams stay methods on the scheme parsers, because they consume parser tables that the parser layer owns (`zhuyin_map`, the double-pinyin scheme tables and the alias-gated syllable inventory).
The zhuyin cut follows the pin's boundary: a facade file, a version script and a configure flag, never a split inside the engine.
`ENABLE_LIBZHUYIN` does not remove chewing from `libpinyin.so`, and `libpinyin.ver` always exports `pinyin_parse_chewing` and `pinyin_get_zhuyin_string`.
No symbol is feature-gated, so a bopomofo-only facade would be additive: a new facade crate that depends on the library, with no existing export moved.

### Facades

Both C crates use the shared context and instance state machines in `facade`.
`runtime` assembles the dictionary, language-model and user handles for each session.
Each C crate forwards its backend selection to the library.

## Backend selection

The library builds with exactly one backend.
Every build of the library, the C libraries included, enables exactly one of the `tkrzw`, `kyotocabinet` and `bdb` features.
`src/store/mod.rs` emits `compile_error!` when zero backends or two or more are enabled.
`DefaultStore` is a `#[cfg]` alias for the enabled backend.
See [backend selection](runbooks/backends.md) for the Cargo default.
A peer is selected with `--no-default-features --features {bdb|kyotocabinet}`.
This follows libpinyin's `--with-dbm` model.
Runtime dispatch and fallback are not added.

The store backends are system native-library dependencies, verified on Linux and buildable on macOS through Homebrew.
Development and CI install tkrzw from the vcpkg overlay port on all three platforms.
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
The cdylibs hide the cxx runtime symbols of both bridges ([export gate](packaging.md#export-gate)).

`input` calls the C library through the `libc` crate, with no build step; the [C-library call and macro-check rules](findings/compatibility-policy.md#external-io) apply.
It declares in Rust, per platform and in one place, the C library's `stdout` and `stderr` objects, which `libc` does not name.
It calls GLib through `glib-sys` in every build.
Its `tool` module, under the `tools` feature, adds the utilities' GLib option calls and reads the C++ streams' lines with `std::io`.

### Dependency decisions

- `thiserror` is used.
  Error types derive `Display`, `Error` and plain `From` conversions with it.
  A conversion with its own semantics, such as `TableError`'s `From<StoreError>`, stays handwritten.
- `fst` is not used for the prefix tables.
  It was slower there than the existing binary search.
- `zerocopy` is not used.
  Packed records are decoded as the [C struct parsing rule](rust-conventions.md#c-struct-parsing) states.
- `arrayvec` is not used.
  The bounded collections already stay inline, as the `SmallVec` `QueryKey` in `user::user_index` does, or use plain arrays, as `engine::nbest` does.

## External I/O

`input` owns the bidirectional external calls, in its scoped unsafe modules.
Stage 1 covers library configuration read/write and locale.
The [boundary plan](findings/external-io-boundary.md#proposed-crate-and-staged-implementation) lists stages 2 to 5: chunks, save and rename, diagnostics, and tools.
All five stages are implemented.
The C crates retain ABI marshalling.
`store` retains the DBM backends.

`data` owns the native system-data readers.
`user` owns the libpinyin session's user data: the user phrase arenas, the pin's user bigram container, the live system items and the user-file persistence over `DefaultStore`. There is no format-version row, which matches the unversioned user files ([user store §4](findings/user-store.md#4-persistence-and-the-save-cycle)). `UserStore::open_transient` opens the pin's NULL-user-directory session in memory, with no profile reads or writes; facade train and save refuse for it before they change training, locale or dirty state.

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
libpinyin's own `gen_binary_files` → `import_interpolation` → `gen_unigram` sequence, which `oxpinyin-utils` provides under those names, produces it; the committed `fixtures/w3` sets came from that sequence over the mini subset of the model (recorded evidence).
The caller supplies directories to `Runtime`, which retains `StoragePaths`.
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
The export check in `ci.yml` gates the C export set.
The allocator-pairing check in `ci.yml` gates the release function of every pointer the C ABI returns or writes to an out-parameter, including the borrowed ones the caller must not free; see the [record](findings/abi-allocator-pairing.md).

## Configuration

The C adapters retain the typed session settings and apply live option words through the session's setters.
Neither libpinyin nor the engine has a settings store.
The engine has no configuration-file parser or layer merger.
Engine weights and language-model order are never user configuration.

## Decoder

The decoder uses a weighted SegmentGraph with index-based arenas, not references.
Current EdgeKind variants are Exact and Segmentation.
Fuzzy, Typo and Abbrev change output and are Stage 3 candidates behind an opt-in compile flag.
Its Rust API is internal and can change under the [interface ruling](constitution.md#external-interfaces).
The graph and decoder follow the pin's parsing and lookup rules ([parser SPEC](findings/parser-spec.md)).
