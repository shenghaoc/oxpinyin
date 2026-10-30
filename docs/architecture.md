# Architecture

## Crates

| Crate | Role | unsafe | Portable | Ships |
|---|---|---|---|---|
| oxpinyin-core | Parser, SegmentGraph, k-best and scoring traits | forbid | yes | via engine |
| oxpinyin-chewing | Dependency-free chewing/zhuyin leaf below core. Packed keys, renderers and frozen content tables. Core depends on chewing. | forbid | yes | via capi, engine |
| oxpinyin-data | Native readers for the formats used by a same-backend libpinyin build (D3 route). | deny (+mmap), target forbid | yes | via engine |
| oxpinyin-input | External-I/O boundary through a small C shim built with `cc` and a safe API. Stage 1 covers configuration and locale. The `tools` feature adds the utilities' GLib option and text, C++ stream and C stdio calls; the shipping libraries never enable it. | deny, scoped allows | yes (`tools`: where GLib is installed) | via data |
| oxpinyin-user | ACID store over DefaultStore. No format-version row, matching unversioned user files (ruling 2026-09-08, `docs/findings/user-store.md` §4). | forbid | yes | via engine |
| oxpinyin-engine | Internal session API for in-tree C facades, tools and tests. No supported external Rust interface. | forbid | yes | yes |
| oxpinyin-facade | Shared context/instance state machines, parse seams, cursor laws and §9 user-data export materialization. See facade dependencies below. | forbid | yes | via capi |
| oxpinyin-capi | `libpinyin.so.15`, all 79 `pinyin_*` exports, and the SONAME/header/pkg-config through cargo-c. | allow | Linux | yes |
| oxpinyin-zhuyin-capi | `libzhuyin.so.15`, with 52 symbols and its own SONAME. The `--enable-libzhuyin` counterpart uses the same engine/chewing surface as oxpinyin-capi. | allow | Linux | yes |
| oxpinyin-capi-marshal | Shared declarative C-ABI marshalling macros, including opaque-handle casts and the `char **` sentence writer. No compiled unsafe code of its own. | forbid | yes | via capi, zhuyin-capi |
| oxpinyin-runtime | Shared concrete assembly of tables, model and user wiring into Session. | forbid | yes | via capi |
| pinyin-oracle | Differential harness against pinned libpinyin. | allow | Linux | never |
| oxpinyin-dictool | Conversions and standalone vocabulary export through facade §9 import/export. Pure Rust, with no C-ABI dependency. | forbid | yes | yes |
| oxpinyin-store | Ordered byte-KV interface with one compile-time Berkeley DB, Kyoto Cabinet or tkrzw backend. See backend dependencies below. | deny | yes | via engine |
| oxpinyin-datagen | model20 compiler for each backend's native runtime files. Shares layouts with data readers. | forbid | yes | never |
| oxpinyin-utils | libpinyin's utilities under their own names and interfaces, through `oxpinyin-input`'s `tools` boundary (stage 5 of `docs/findings/external-io-boundary.md`): `gen_pinyin_table` and `gen_zhuyin_table`. | forbid | Linux, macOS (needs GLib) | never |
| oxpinyin-corpus | Training corpus frontend: zhwiki dump to ngseg raw text. | forbid | yes | never |
| oxpinyin-training | Backend-sensitive training modules: segment, counter, lambda, emitter, eval and train. Existing stage binary names remain. One backend selection covers all modules. | forbid | yes | never |
| oxpinyin-testsupport | Shared fixture Dictionary/LanguageModel doubles, model20 cache locator and scorer suites. See test-support dependencies below. | forbid | yes | never |
| oxpinyin-kmm | W9 K-mixture pipeline: generate, estimate, merge, validate, prune, export, import and interpolation conversion. | forbid | yes | never |
| oxpinyin-punct | W9 punctuation-table generator reproducing `genpunct.py`. | forbid | yes | never |
| oxpinyin-word | W9 word-recognition pipeline: populate, partialword, newword and markpinyin. | forbid | yes | never |

Both C-ABI facades consume `oxpinyin-facade`.
It depends on core, engine, runtime and user.
It holds runtime's concrete dictionary, language-model and user handles by value.
It therefore forwards the complete backend feature matrix instead of staying generic over engine traits.

Berkeley DB has been the default backend since 2026-09-20.
The store backends are system C-library dependencies, verified on Linux and buildable on macOS through Homebrew.
Portable CI lanes install each operating system's backend.
The peer matrix remains on Linux.

Datagen uses `MemoryChunk`/`SubPhraseIndex` constants and the checksum from `chunk_format`.
It uses the four DBM row schemas and library-name tables from `row_format` and `system_files`.
Readers and writers therefore share one layout definition.

The scorer suites `tests/scoring.rs` and `tests/scoring_fixture.rs` reside in testsupport.
This prevents core from having a development dependency on testsupport.
Testsupport is for development except for its datagen consumer, which never ships.

## Backend selection

Select exactly one backend per binary at compile time.
`DefaultStore` is a `#[cfg]` alias for the enabled backend feature.
Select a peer with `--no-default-features --features {kyotocabinet|tkrzw}`.
This follows libpinyin's `--with-dbm` model.
Do not add runtime dispatch or fallback.
`oxpinyin-store` emits `compile_error!` when zero or multiple backends are enabled.

## Dependencies

**Dependencies (maintainer, 2026-09-12; Kyoto Cabinet shim, maintainer
approval 2026-10-23 UTC):**

Use a Rust library as Rust.
Redb was the example; it was removed 2026-09-22.
The tree has no pure-Rust database replacement.
`glib-sys` binds a C library.
Link Kyoto Cabinet, tkrzw and Berkeley DB to the distribution's libraries.
Use tkrzw and Berkeley DB through their maintainer-provided C ABIs, `tkrzw_langc.h` and `db.h`.
Generate bindings with bindgen over system headers.
Do not use tkrzw's C++ interface.
It has no ABI stability across compilers, standard libraries or library versions.
Its C API is the maintainer's stability promise.

Use Kyoto Cabinet as libpinyin does.
Compile `crates/oxpinyin-store/src/kyotocabinet/shim.cc` with `cc` under the `kyotocabinet` feature.
It constructs `TreeDB`, `HashDB`, `StashDB` and `ProtoTreeDB` from build-time headers.
Its `noexcept` C entry points in `shim.h` always turn C++ exceptions into failure returns.
Like libpinyin, it depends on the system library's C++ ABI.
The facades import 68 `kyotocabinet::` names, 67 among the pin's 77.
This describes the current tree.
The `oxpinyin-input` boundary has a small C shim, `native/config.c`, in stage 1.
Its `tools` feature adds `native/tool.c`, compiled against GLib's headers found with `pkg-config`, and the tool-only C++ stream shim `native/tool_stream.cc`, both through the existing `cc` build dependency.
Use narrow hand-written extern declarations and no direct `libc` crate dependency.

Kyoto Cabinet left `kclangc.h` after measurement in #1118.
See the [shim results](findings/kc-shim-1118.md).
`kclangc.h` reaches every class through `PolyDB`.
That dispatch and linked but unopened classes kept about 580 KiB more libkyotocabinet text resident than the pin.
The shim retains the pin's libkyotocabinet residency.
An earlier cxx bridge was measured and rejected.
See its [results](findings/kc-c-vs-cxx-adapter-2026-10-22.md) and PR #1043 for code.
Its glue added 0.6–5.5% instructions to point reads, writes and framed scans.
Cargo-built libraries also exported cxx runtime and bridge symbols beyond the version scripts.
The adopted shim has no glue layer.
It uses 0.980–1.004× the C binding's instructions per store operation against exact same-bytes controls.
It exports nothing beyond the version scripts.
It adds about 310 KB to each stripped library.

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
Either libpinyin tools or `oxpinyin-datagen compile` can produce it.
The caller supplies `StoragePaths`.
Do not auto-detect a distribution layout.

## Portability

Keep the engine session API independent of frameworks.
It uses abstract `KeyInput`, preedit spans with a style enum, and candidate iteration.
Inject configuration and storage paths as data.
Do not add platform services or `cfg(target_os)` to portable crates.
Keep IBus keysym translation in oxpinyin-capi, never in the engine.
Sessions are instance-per-context and suitable for the TSF, IMK and ArkTS main-thread models.
See the [native Rust policy](findings/compatibility-policy.md#native-rust-implementation).

## Supported external interfaces

The [interface ruling](constitution.md#external-interfaces) sets the supported interfaces: the C APIs/ABIs and the pin-compatible tools, with every crate's Rust interface internal.
The export check in `ci.yml` gates the C export set.

## Configuration

Apply configuration in this order:

1. The frozen [upstream defaults](findings/upstream-schema.md), verbatim.
2. System drop-ins.
3. A user layer supplied by the shell as data.

The shell can supply a `ConfigLayer` file or construct it from its own settings store.
Neither libpinyin nor the engine has a settings store.
Merge layers with a pure core function.
Keep `Config::default()` equal to the captured upstream defaults.
The default is the parity configuration used by S1b.
Use data overlays for rules, maps and schemes, and live preferences for customization.
Engine weights and language-model order are never user configuration.

## Decoder

Use a weighted SegmentGraph with index-based arenas from the start, not references.
Current EdgeKind variants are Exact and Segmentation.
Fuzzy, Typo and Abbrev change output and are Stage 3 candidates behind an opt-in compile flag.
The scorer accepts edge costs.
Its Rust API is internal and can change under the 2026-10-18 UTC interface ruling.
In parity mode, mirror the path-set SPEC's upstream path-enumeration policy.
Produce neither more paths nor fewer.
