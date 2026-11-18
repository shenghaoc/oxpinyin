# Architecture

## Crates

| Crate | Role | unsafe | Portable | Ships |
|---|---|---|---|---|
| oxpinyin-core | Shared C facade orchestration, session state, candidate selection, decoding, Pinyin and Zhuyin parsers, SegmentGraph, the key comparison and model traits. Packed chewing keys, renderers, frozen content tables, system-data readers, user persistence, native I/O and the selected DBM backend. | deny, scoped input and backend exceptions | yes | yes |
| oxpinyin-capi | `libpinyin.so.15`, all 79 `pinyin_*` exports, and the SONAME/header/pkg-config through cargo-c. | allow | Linux | yes |
| oxpinyin-zhuyin-capi | `libzhuyin.so.15`, with 52 symbols and its own SONAME. The `--enable-libzhuyin` counterpart uses the same engine/chewing surface as oxpinyin-capi. | allow | Linux | yes |
| oxpinyin-utils | libpinyin's utilities under their own names and interfaces, through `oxpinyin-core`'s `tools` boundary (stage 5 of `docs/findings/external-io-boundary.md`): the table generators, the K-mixture-model utilities on the pin's `FlexibleBigram` container, `k_mixture_model_to_interpolation`, the segmentation trio, the storage trio and the five n-gram/interpolation utilities (`gen_unigram`, `gen_ngram`, `gen_deleted_ngram`, `estimate_interpolation`, `eval_correction_rate`). | forbid | Linux, macOS (needs GLib) | never |
| oxpinyin-repl | One nonshipping binary with ten interactive parser, lookup and C-facade modes. `just repl` supplies disposable W3 data; no timing loops. | forbid | Linux, macOS (needs GLib) | never |

See [backend selection](runbooks/backends.md) for the Cargo default.
The store backends are system native-library dependencies, verified on Linux and buildable on macOS through Homebrew.
Development and CI install tkrzw from the vcpkg overlay port on all three platforms.
Debian release packages and oracle/reference images retain Debian's library.
The peer matrix remains on Linux.

The data generators in `oxpinyin-utils` use `MemoryChunk`/`SubPhraseIndex` constants and the checksum from `chunk_format`.
They use the four DBM row schemas and library-name tables from `row_format` and `system_files`.
Readers and writers therefore share one layout definition.

The fixture `Dictionary`/`LanguageModel` doubles are one source file, `crates/oxpinyin-core/tests/common/fixture_doubles.rs`.
Each test target that needs them includes the file with `#[path]`, so no shared test crate can close a dependency cycle through core.
The production build graph carries no test doubles.

### Chewing keys

`oxpinyin-core` holds the shared chewing-key machinery: the packed `ChewingKey` (the pin's two-byte bitfield word, `#[repr(C)]`, with size and alignment asserted), its six display renderers and the frozen `content_table` and `chewing_key_table`.
The capi facade's `keys.rs` holds only `#[no_mangle]` wrappers and failure-shape handling.
The three `parse_one_key` seams stay methods on `oxpinyin-core`'s scheme parsers, because they consume parser tables that core owns (`zhuyin_map`, the double-pinyin scheme tables and the alias-gated syllable inventory).
The zhuyin cut follows the pin's boundary: a facade file, a version script and a configure flag, never a split inside the engine.
`ENABLE_LIBZHUYIN` does not remove chewing from `libpinyin.so`, and `libpinyin.ver` always exports `pinyin_parse_chewing` and `pinyin_get_zhuyin_string`.
No symbol is feature-gated, so a bopomofo-only facade would be additive: a new facade crate that depends on `oxpinyin-core`, with no existing export moved.

### Facades

Both C crates use the shared context and instance state machines in `oxpinyin_core::facade`. Its runtime module assembles the dictionary, language-model and user handles for each session. Each C crate forwards its backend selection to core.

## Backend selection

Exactly one backend per binary is selected at compile time.
`DefaultStore` is a `#[cfg]` alias for the enabled backend feature.
A peer is selected with `--no-default-features --features {bdb|kyotocabinet}`.
This follows libpinyin's `--with-dbm` model.
Runtime dispatch and fallback are not added.
`oxpinyin-core` emits `compile_error!` when zero or multiple backends are enabled.

A save makes the pin's synchronisation call for the selected backend (`ngram_tkrzwdb.cpp:80`, `ngram_kyotodb.cpp:102` and `ngram_bdb.cpp:103` at `074a2219`).
Durability is therefore the backend's durability for that call; oxpinyin promises nothing beyond it.

## Dependencies

The [Rust/C++ interoperability ruling](findings/compatibility-policy.md#rust-cpp-interoperability) governs native bindings.

A Rust library is used as Rust.
The tree has no pure-Rust database backend.
`glib-sys` binds a C library.
Kyoto Cabinet, tkrzw and Berkeley DB link to the distribution's libraries.
Berkeley DB is used through its official C API, `db.h`, with bindgen over system headers.
tkrzw binds with CXX over the concrete classes the pin uses (`crates/oxpinyin-core/src/store/tkrzw/bridge.rs`, with the semantic adapter `adapter.cc`).
`oxpinyin-core` finds the backends with `system-deps`: tkrzw and Kyoto Cabinet through their `.pc` files, and Berkeley DB without pkg-config, because the distributions and Homebrew install no `.pc` file for it.
The `SYSTEM_DEPS_BDB_*` overrides reach a Berkeley DB outside the defaults ([discovery and overrides](runbooks/backends.md#discovery-and-overrides)).

Kyoto Cabinet is used as libpinyin uses it.
Kyoto Cabinet binds with CXX over the concrete classes the pin uses (`crates/oxpinyin-core/src/store/kyotocabinet/bridge.rs`, with the semantic adapter `adapter.cc`), under the `kyotocabinet` feature.
It constructs `TreeDB`, `HashDB`, `StashDB` and `ProtoTreeDB` from build-time headers.
No bridge function returns `Result`, so a C++ exception terminates the process, as it ends the pin's.
Like libpinyin, it depends on the system library's C++ ABI.
Constructing the concrete classes directly, as the pin does, keeps the pin's libkyotocabinet residency; `kclangc.h` reaches every class through `PolyDB`, which keeps more of the library resident.
The facades import 68 `kyotocabinet::` names, 67 among the pin's 77.
The cdylibs hide the cxx runtime symbols of both bridges ([export gate](packaging.md#export-gate)).

The `oxpinyin-core` boundary calls the C library through the `libc` crate, with no build step; the [C-library call and macro-check rules](findings/compatibility-policy.md#external-io) apply.
It declares in Rust, per platform and in one place, the C library's `stdout` and `stderr` objects, which `libc` does not name.
Its `tools` feature adds GLib through `glib-sys` and reads the C++ streams' lines with `std::io`.

### Dependency decisions

- `thiserror` is used.
  Error types derive `Display`, `Error` and plain `From` conversions with it.
  A conversion with its own semantics, such as `TableError`'s `From<StoreError>`, stays handwritten.
- `fst` is not used for the prefix tables.
  It was slower there than the existing binary search.
- `zerocopy` is not used.
  Packed records are decoded as the [C struct parsing rule](rust-conventions.md#c-struct-parsing) states.
- `arrayvec` is not used.
  The bounded collections already stay inline, as the `SmallVec` `QueryKey` in `oxpinyin-core`'s `user_index.rs` does, or use plain arrays, as `oxpinyin-core`'s `nbest.rs` does.

## External I/O

`oxpinyin-core` owns bidirectional external calls in scoped unsafe modules.
Stage 1 covers library configuration read/write and locale.
The [boundary plan](findings/external-io-boundary.md#proposed-crate-and-staged-implementation) lists stages 2 to 5: chunks, save and rename, diagnostics, and tools.
All five stages are implemented.
C facades retain ABI marshalling.
`oxpinyin-core` retains DBM backends.

The core user module owns the following data. Native system-data readers and the libpinyin session's user data: the user phrase arenas, the pin's user bigram container, the live system items and the user-file persistence over DefaultStore. No format-version row, matching unversioned user files ([user store §4](findings/user-store.md#4-persistence-and-the-save-cycle)). `UserStore::open_transient` opens the pin's NULL-user-directory session in memory, with no profile reads or writes; facade train and save refuse for it before they change training, locale or dirty state.

## Centralized assembly

Decodable engines are constructed in exactly one place, `oxpinyin-core`.
It assembles system tables, unigram model, λ, a persistent or transient user store, addons and punctuation.
Facades through `oxpinyin-core`, capi and future adapters consume that assembly.
Nothing else assembles equivalents independently.
This prevents native and C-ABI paths from silently diverging.

The facade runtime connects the system and user models to the engine.
Its deliberate pinned algorithms are:

- The user-count overlay feed.
  Arithmetic lives in data's `*_with_user_delta` methods.
  Gate 2 merges whole rows through `load_merged_row` over core's `MergedGram`.
- Suggestion row order through `merge_suggestion_rows`.
  C facades consume it through `oxpinyin-core::merged_suggestions`.

Each algorithm's source cites the pin.

## Data path

There is one native reader set per backend.
Data lazily reads the pinyin and phrase DBMs, per-library `MemoryChunk` files, `bigram.db`, `punct.bin` and the addon DBM pair.
Each reader opens a handle and performs a point read.
Chunk files use mmap and checksums.
Nothing is scanned at open.

`table.conf` is read once through data's `table_info`, the input scanner and `SystemTableInfo2::load`'s own calls.
Its rows determine which libraries exist, their filenames and which calls accept them.
Its header supplies λ and the versions used to check the user profile.
Runtime opens the same system directory as `pinyin_init` for that backend.
libpinyin's own `gen_binary_files` → `import_interpolation` → `gen_unigram` sequence, which `oxpinyin-utils` provides under those names, produces it; the committed `fixtures/w3` sets came from that sequence over the mini subset of the model (recorded evidence).
The caller supplies directories to `Runtime`, which retains `StoragePaths`.
A distribution layout is not auto-detected.

## Portability

The engine session API stays independent of frameworks.
It exposes parsing, candidate iteration, selection, training and reset operations.
Configuration and backend handles are injected as data; runtime owns storage paths.
Platform services and `cfg(target_os)` are not added to portable crates.
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
