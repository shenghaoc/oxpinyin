# Architecture

## Crates

| Crate | Role | unsafe | Portable | Ships |
|---|---|---|---|---|
| oxpinyin-core | Parser, SegmentGraph, k-best and scoring traits | forbid | yes | via engine |
| oxpinyin-chewing | Dependency-free chewing/zhuyin leaf below core. Packed keys, renderers and frozen content tables. Core depends on chewing. | forbid | yes | via capi, engine |
| oxpinyin-data | Native readers for the formats used by a same-backend libpinyin build (D3 route). | forbid | yes | via engine |
| oxpinyin-input | External-I/O boundary through the `libc` crate and a safe API. Stage 1 covers configuration and locale. Stage 2 activates a single-owner memmap2 API for system and addon chunks; remaining chunk-load calls are separate. The `tools` feature adds the utilities' GLib option and text, C++ stream and C stdio calls; the shipping libraries never enable it. | deny, scoped allows | yes (`tools`: where GLib is installed) | via data |
| oxpinyin-user | The libpinyin session's user data: the user phrase arenas, the pin's user bigram container, the live system items and the user-file persistence over DefaultStore. No format-version row, matching unversioned user files (ruling 2026-09-08, `docs/findings/user-store.md` §4). | forbid | yes | via engine |
| oxpinyin-engine | Internal session API for in-tree C facades, tools and tests. No supported external Rust interface. | forbid | yes | yes |
| oxpinyin-facade | Shared context/instance state machines, parse seams, cursor laws and §9 user-data export materialization. See facade dependencies below. | forbid | yes | via capi |
| oxpinyin-capi | `libpinyin.so.15`, all 79 `pinyin_*` exports, and the SONAME/header/pkg-config through cargo-c. | allow | Linux | yes |
| oxpinyin-zhuyin-capi | `libzhuyin.so.15`, with 52 symbols and its own SONAME. The `--enable-libzhuyin` counterpart uses the same engine/chewing surface as oxpinyin-capi. | allow | Linux | yes |
| oxpinyin-capi-marshal | Shared declarative C-ABI marshalling macros, including opaque-handle casts and the `char **` sentence writer. No compiled unsafe code of its own. | forbid | yes | via capi, zhuyin-capi |
| oxpinyin-runtime | Shared concrete assembly of tables, model and user wiring into Session. | forbid | yes | via capi |
| oxpinyin-store | Ordered byte-KV interface with one compile-time Berkeley DB, Kyoto Cabinet or tkrzw backend. See backend dependencies below. | deny | yes | via engine |
| oxpinyin-utils | libpinyin's utilities under their own names and interfaces, through `oxpinyin-input`'s `tools` boundary (stage 5 of `docs/findings/external-io-boundary.md`): the table generators, `k_mixture_model_to_interpolation`, the segmentation trio, the storage trio and the five n-gram/interpolation utilities (`gen_unigram`, `gen_ngram`, `gen_deleted_ngram`, `estimate_interpolation`, `eval_correction_rate`). | forbid | Linux, macOS (needs GLib) | never |
| oxpinyin-training | The `eval` module: the decode behind `eval_correction_rate`. `oxpinyin-utils` carries the pin-named utilities. | forbid | yes | never |
| oxpinyin-kmm | K-mixture-model operations on the pin's `FlexibleBigram` container: generate, estimate, merge, validate, prune, export and import; `oxpinyin-utils` carries the seven pin-named binaries. | forbid | yes | never |

`oxpinyin-core`, `oxpinyin-chewing` and `oxpinyin-engine` never depend on storage (`oxpinyin-store`, `cxx`, `bindgen` or a backend binding), including through dev and build dependencies; `cargo deny check` enforces it through the `[bans] deny` wrappers in `deny.toml`.

Both C-ABI facades consume `oxpinyin-facade`.
It depends on core, engine, runtime and user.
It holds runtime's concrete dictionary, language-model and user handles by value.
It therefore forwards the complete backend feature matrix instead of staying generic over engine traits.

See [backend selection](runbooks/backends.md) for the Cargo default.
The store backends are system native-library dependencies, verified on Linux and buildable on macOS through Homebrew.
CI's Linux, macOS and Windows lanes build tkrzw with `.github/actions/tkrzw`.
The peer matrix remains on Linux.

Datagen uses `MemoryChunk`/`SubPhraseIndex` constants and the checksum from `chunk_format`.
It uses the four DBM row schemas and library-name tables from `row_format` and `system_files`.
Readers and writers therefore share one layout definition.

The fixture `Dictionary`/`LanguageModel` doubles are one source file, `crates/oxpinyin-core/tests/common/fixture_doubles.rs`.
Each test target that needs them includes the file with `#[path]`, so no shared test crate can close a dependency cycle through core.
The production build graph carries no test doubles.

## Backend selection

Select exactly one backend per binary at compile time.
`DefaultStore` is a `#[cfg]` alias for the enabled backend feature.
Select a peer with `--no-default-features --features {bdb|kyotocabinet}`.
This follows libpinyin's `--with-dbm` model.
Do not add runtime dispatch or fallback.
`oxpinyin-store` emits `compile_error!` when zero or multiple backends are enabled.

Saving makes the pin's synchronisation call for the selected backend (`ngram_tkrzwdb.cpp:80`, `ngram_kyotodb.cpp:102` and `ngram_bdb.cpp:103` at `074a2219`), so durability is the backend's for that call, and oxpinyin promises nothing beyond it.

## Dependencies

**Dependencies (amended by the [Rust/C++ interoperability ruling](findings/compatibility-policy.md#rust-cpp-interoperability), 2026-11-02 UTC):**

Use a Rust library as Rust.
Redb was the example; it was removed 2026-09-22.
The tree has no pure-Rust database replacement.
`glib-sys` binds a C library.
Link Kyoto Cabinet, tkrzw and Berkeley DB to the distribution's libraries.
Use Berkeley DB through its official C API, `db.h`, with bindgen over system headers.
tkrzw binds with CXX over the concrete classes the pin uses (`crates/oxpinyin-store/src/tkrzw/bridge.rs`, with the semantic adapter `adapter.cc`); the `tkrzw_langc.h` C API is no longer used.
Find tkrzw, Kyoto Cabinet, Berkeley DB where it ships a `.pc` file, and GLib with the `pkg-config` crate, a build-dependency of `oxpinyin-store`, `oxpinyin-input` and `oxpinyin-training` (maintainer approval 2026-10-30 UTC).
Berkeley DB falls back to fixed header directories and the `OXPINYIN_BDB_*` overrides, because the distributions and Homebrew install no `.pc` file for it.
The [interoperability ruling](findings/compatibility-policy.md#rust-cpp-interoperability) supersedes the prohibition on tkrzw's C++ interface and sets the migration order and source-proof obligations.

Use Kyoto Cabinet as libpinyin does.
Kyoto Cabinet binds with CXX over the concrete classes the pin uses (`crates/oxpinyin-store/src/kyotocabinet/bridge.rs`, with the semantic adapter `adapter.cc`), under the `kyotocabinet` feature.
It constructs `TreeDB`, `HashDB`, `StashDB` and `ProtoTreeDB` from build-time headers.
No bridge function returns `Result`, so a C++ exception terminates the process, as it ends the pin's.
Like libpinyin, it depends on the system library's C++ ABI.
The KC native-call inventory holds the source behind the migration from the C++ shim.
The facades import 68 `kyotocabinet::` names, 67 among the pin's 77.
This describes the current tree.
The `oxpinyin-input` boundary calls the C library through the `libc` crate, with no build step; its [C-library calls and macro checks](source-policy.md#external-io) retain the 2026-10-31 UTC rule.
It declares in Rust, per platform and in one place, the C library's `stdout` and `stderr` objects, which `libc` does not name.
Its `tools` feature adds GLib through `glib-sys` and reads the C++ streams' lines with `std::io`.

Kyoto Cabinet left `kclangc.h` after measurement in #1118, through a C++ shim that the CXX bridge replaced.
See the shim results.
`kclangc.h` reaches every class through `PolyDB`.
That dispatch and linked but unopened classes kept about 580 KiB more libkyotocabinet text resident than the pin.
Constructing the concrete classes directly, as the shim and the bridge both do, retains the pin's libkyotocabinet residency.
An earlier cxx bridge was measured and rejected in #1043; this is historical evidence about that implementation, not a restriction on CXX bindings under the [current ruling](findings/compatibility-policy.md#rust-cpp-interoperability).
See its results and PR #1043 for code.
Its glue added 0.6–5.5% instructions to point reads, writes and framed scans.
Cargo-built libraries also exported cxx runtime and bridge symbols beyond the version scripts.
The shim that #1118 adopted had no glue layer.
It used 0.980–1.004× the C binding's instructions per store operation against exact same-bytes controls, and added about 310 KB to each stripped library.
The current bridge's cdylibs hide the cxx runtime symbols as the tkrzw bridge's do ([export gate](packaging.md#export-gate)).

## External I/O

`oxpinyin-input` owns bidirectional external calls in scoped unsafe modules.
Stage 1 covers library configuration read/write and locale.
The [boundary plan](findings/external-io-boundary.md) lists later stages.
C facades retain ABI marshalling.
`oxpinyin-store` retains DBM backends.

## Centralized assembly

Construct decodable engines in exactly one place, `oxpinyin-runtime`.
It assembles system tables, unigram model, λ, optional user store, addons and punctuation.
Facades through `oxpinyin-facade`, capi and future adapters consume that assembly.
Do not assemble equivalents independently.
This prevents native and C-ABI paths from silently diverging.

Runtime wires the public Rust APIs of data, user and engine.
Its deliberate pinned algorithms are:

- The user-count overlay feed.
  Arithmetic lives in data's `*_with_user_delta` methods.
  Gate 2 merges whole rows through `load_merged_row` over core's `MergedGram`.
- Suggestion row order through `merge_suggestion_rows`.
  C facades consume it through `oxpinyin-facade::merged_suggestions`.

Each algorithm's source cites the pin.

## Data path

**Data path (P6, 2026-09-02):**

Use one native reader set per backend.
Data lazily reads the pinyin and phrase DBMs, per-library `MemoryChunk` files, `bigram.db`, `punct.bin` and the addon DBM pair.
Each reader opens a handle and performs a point read.
Chunk files use mmap and checksums.
Nothing is scanned at open.

Read `table.conf` once through data's `table_info`, the input scanner and `SystemTableInfo2::load`'s own calls.
Its rows determine which libraries exist, their filenames and which calls accept them.
Its header supplies λ and the versions used to check the user profile.
Runtime opens the same system directory as `pinyin_init` for that backend.
libpinyin's own `gen_binary_files` → `import_interpolation` → `gen_unigram` sequence, which `oxpinyin-utils` provides under those names, produces it; `just w3-fixtures` (`tools/dev/w3_fixtures.py`) runs that sequence over the mini subset of the model to regenerate and check `fixtures/w3`.
The caller supplies directories to `Runtime`, which retains `StoragePaths`.
Do not auto-detect a distribution layout.

## Portability

Keep the engine session API independent of frameworks.
It exposes parsing, candidate iteration, selection, training and reset operations.
Inject configuration and backend handles as data; runtime owns storage paths.
Do not add platform services or `cfg(target_os)` to portable crates.
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

Use a weighted SegmentGraph with index-based arenas from the start, not references.
Current EdgeKind variants are Exact and Segmentation.
Fuzzy, Typo and Abbrev change output and are Stage 3 candidates behind an opt-in compile flag.
The scorer accepts edge costs.
Its Rust API is internal and can change under the 2026-10-18 UTC interface ruling.
In parity mode, mirror the path-set SPEC's upstream path-enumeration policy.
Produce neither more paths nor fewer.
