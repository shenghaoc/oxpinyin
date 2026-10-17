# Backend dependency audit

Date: 2026-10-14 (UTC) · Status: **audit. Phase 1 is accepted. G1 (#769), C1 (#771), G2 (#772) and C2 (#773) are authorised. The PRs are the implementations. See §7.**

This document shows how the storage-backend choice (`bdb`, `kyotocabinet`, `tkrzw`) reaches the 20 workspace crates. It also shows which of that coupling the project can remove. The workspace keeps all 20 crates. This audit does not cover consolidation, the split of crates or the move of code between crates.

This document keeps three things separate:

- **Crate boundaries** enforce dependency direction and visibility.
- **Cargo feature propagation** coordinates compile-time configuration across the dependency graph.
- **Runtime coupling** is whether higher-level code needs to know which backend implements its operations.

A change that removes a feature edge does not remove architectural coupling by itself. A change that adds generics does not improve the design by itself. This audit does not count a lower crate count as a benefit. It does not count fewer lines of feature configuration as a benefit.

## Sources

- oxpinyin: `origin/main` = `5ef525848f38a985900c0785834683eead973db4`. Every result applies to that SHA.
- libpinyin: the oracle pin `074a2219c90feaf962d0d24f034514033ece5f99` (`tools/oracle/oracle-pin.txt`, tag `2.11.92`). The audit reads the source from a checkout at that commit. `backend-selection-audit.md` cites the older pin `0c5e80e1200f84fab185d1c5bde458b770a0636c`. It also describes the redb and lmdb peers. Both points are out of date.
- Cargo: the reference `src/doc/src/reference/features.md` at the tag for the toolchain (cargo `1.97`, commit `c980f48`).

## Method

The audit did not compile code. It did not run a build script. The tools were `git`, `grep`, file reads and `cargo tree --locked`.

The audit took the feature resolution of each selection with this command:

```
cargo tree --locked --target all --prefix none --format '{p}|{f}' <selection>
```

The audit used 125 selections:

- Every workspace crate with default features.
- Every workspace crate with `--no-default-features`.
- Every workspace crate with each of `--no-default-features --features {bdb,kyotocabinet,tkrzw}`.
- `--workspace` with the same four flag sets.
- `--workspace` with the CI `--exclude` set.
- `-p oxpinyin-capi -p oxpinyin-zhuyin-capi`.
- `-p oxpinyin-engine|oxpinyin-core --features oxpinyin-testsupport/<backend>`.
- `-p oxpinyin-corpus --features oxpinyin-training/<backend>`.
- The `fuzz/` workspace.

The audit removed one edge at a time from a scratch copy of the manifests. This gave 95 variants. The audit never changed a tracked file. For each variant, it compared the resolved feature sets of each package over all selections.

The audit also removed together the 33 edges that §2 marks redundant. The result was identical across all selections.

## 1. Cargo's rule for `--features` across several selected packages

`features.md` at the cited tag says:

- With resolver 2, `--features` can enable features "for any of the packages selected on the command-line with `-p` and `--workspace` flags". The workspace uses resolver 3. `--no-default-features` disables the defaults for all selected members.
- "Features are unique to the package that defines them". When several packages build a dependency, it gets "the union of all features enabled on it".

`cargo tree` showed two results. `--workspace --features kyotocabinet` succeeds, although `oxpinyin-core` and `oxpinyin-engine` declare no such feature. `-p oxpinyin-core --features bdb` fails with "does not contain this feature". The CI cascade (`.github/workflows/store-backends.yml`) therefore enables `<peer>` on each member that declares it. The cascade does not depend on forwarders between crates.

## 2. Q1: backend feature edges

### Inventory

Every `default = ["bdb"]` is also an edge. All forwarders have the same shape for the three backends. `fuzz/Cargo.toml` has no forwarder.

| Crate | Forwarders (`[features]`) | `default-features = false` edges |
| --- | --- | --- |
| `oxpinyin-store` | `bdb`, `kyotocabinet`, `tkrzw` = `["dep:bindgen"]`; `default = ["bdb"]` | none |
| `oxpinyin-data` | `B` = `[store/B]`; `default = ["bdb"]` | store, via the workspace table |
| `oxpinyin-user` | `B` = `[data/B, store/B]`; default | data and store, via the workspace table |
| `oxpinyin-runtime` | `B` = `[data/B, user/B]`; default | data and user, via the workspace table |
| `oxpinyin-facade` | `B` = `[runtime/B, user/B]`; default | runtime and user, via the workspace table |
| `oxpinyin-capi`, `oxpinyin-zhuyin-capi` | `B` = `[runtime/B, data/B, user/B, facade/B]`; default | data and facade, direct; runtime and user, via the workspace table |
| `oxpinyin-dictool` | `B` = `[facade/B, user/B]`; default | facade and user, direct |
| `oxpinyin-training` | `B` = `[data/B]`; default | data, via the workspace table |
| `pinyin-oracle` | `B` = `[data/B]`; default | data, direct; user, dev-dependency |
| `oxpinyin-datagen` | `B` = `[store/B, testsupport/B]` at the audited SHA; default | store and data, direct |
| `oxpinyin-testsupport` | `B` = `[store/B]` at the audited SHA; `default = []` | none |
| `oxpinyin-corpus` | none | `oxpinyin-training`, dev-dependency |

The workspace table (`Cargo.toml`) has `default-features = false` on data, runtime, store and user. `fuzz/Cargo.toml` uses the defaults of data, user and capi. It resolves to bdb. These crates declare no backend feature: `oxpinyin-core`, `oxpinyin-engine`, `oxpinyin-chewing`, `oxpinyin-kmm`, `oxpinyin-punct`, `oxpinyin-word` and `oxpinyin-capi-marshal`.

Only one non-backend feature gates backend code. It is `bench-internal` in `oxpinyin-store`. It widens the Kyoto Cabinet open path for the `kyotocabinet_bnum` bench. These features gate no backend code: `capi`, `fuzz-api`, `shipped`, `alloc-count`, `oracle-ffi` and `diagnostic_403`.

### Backend-gated code per crate

The audit searched for `cfg(feature = "<backend>")` and `cfg!`. It also searched the build scripts for `CARGO_FEATURE_*`.

| Crate | Count | Locations |
| --- | --- | --- |
| `oxpinyin-store` | 86 | `src/lib.rs` (49), `src/common.rs` (13), `build.rs` (8), benches, examples, `tests/bdb_libpinyin_files.rs`; `required-features` on `backend_matrix_*` and `kyotocabinet_bnum` |
| `oxpinyin-datagen` | 38 | `src/write.rs` (compiled code); `tests/{cross_backend,libpinyin_parity,backend_default,fixtures_identity}.rs`; `build.rs:23,31` (Kyoto Cabinet rpath) |
| `oxpinyin-user` | 3 | `src/store.rs:4247,4249,4251` (test macros) |
| `oxpinyin-data` | 1 | `src/chewing_table.rs:776` (inside `#[cfg(test)]`) |
| `oxpinyin-runtime` | 1 | `tests/libpinyin_data_dir.rs:14` (`cfg(any(kyotocabinet, tkrzw))`) |
| `oxpinyin-dictool`, `oxpinyin-training` | 2 each | `build.rs:23,31` (Kyoto Cabinet rpath; link args are package-scoped) |
| `oxpinyin-capi` | 2 | `build.rs:174,176` read `CARGO_FEATURE_KYOTOCABINET` and `CARGO_FEATURE_TKRZW` of the crate itself to write `database_format` into `libpinyin.pc.in.baked`; `tests/database_format.rs` checks it against `DEFAULT_STORE_DB_FORMAT` |
| `oxpinyin-zhuyin-capi` | 2 | `build.rs:94,96`, same mechanism; no equivalent test |
| `oxpinyin-facade`, `oxpinyin-testsupport`, `pinyin-oracle`, `fuzz` | 0 | none |

### Builds and CI invocations that depend on the edges

These builds and CI invocations depend on the edges:

- `--workspace` with defaults: `ci.yml`; `store-backends.yml` (bdb cell).
- `--workspace --no-default-features --features kyotocabinet`: `coverage.yml`, `verify-nightly.yml`, `store-backends.yml`.
- `--workspace --no-default-features --features tkrzw`: `ci.yml` (macOS), `store-backends.yml`.
- `-p oxpinyin-store --no-default-features --features {kyotocabinet,tkrzw,bdb}`: `tools/store/backend-matrix.sh`, the sanitizer jobs, the benches.
- `cd <crate> && cargo cinstall … --features X` for both cdylibs: `tools/packaging/install.sh`, `release-packages.yml`.
- `cargo build -p oxpinyin-capi [-p oxpinyin-zhuyin-capi] --no-default-features --features $backend`: the perf and bisection scripts.
- `-p oxpinyin-datagen --no-default-features --features tkrzw`: `tools/oracle/`.
- `cargo test -p oxpinyin-engine --features oxpinyin-testsupport/bdb`: dated findings records only, not CI.

### Edge verdicts

| Edge | Verdict | Evidence |
| --- | --- | --- |
| Every crate's `default = ["bdb"]` (store, data, user, runtime, facade, capi, zhuyin-capi, dictool, training, oracle, datagen) | **necessary** | Dropping it zeroes the store backend in `-p <crate>` default builds. `fuzz/` relies on the data, user and capi defaults; at least one of the three must stay. |
| `oxpinyin-testsupport` `default = []` | **necessary** at the audited SHA | The manifest comment says a default would re-enable kyotocabinet through the macOS path dev-dependencies. |
| `data`: `B = [store/B]` | **necessary** | The only path to the store. |
| `training`: `B = [data/B]`; `pinyin-oracle`: `B = [data/B]` | **necessary** | Each is the only path. Training also has `build.rs:23,31`. |
| `testsupport`: `B = [store/B]` | **necessary** at the audited SHA | The only path for `-p oxpinyin-testsupport --features B` and the engine gate in the dated records. See G1. |
| `store`: `B = ["dep:bindgen"]` | **necessary** | `build.rs:184`, `:335` and the bdb arm call `bindgen::Builder` under `cfg`. Removing `dep:bindgen` from bdb breaks `fuzz/`'s `--locked` resolution. For kyotocabinet and tkrzw the resolution is unchanged, so the necessity rests on the source reading. |
| `capi`, `zhuyin-capi`: own `B` features and one path to the store | **necessary** | `CARGO_FEATURE_*` reads in `build.rs`; `cinstall --features` and the perf scripts pass the feature to the crate itself. |
| `dictool`, `training`: own `kyotocabinet` feature | **necessary** | `build.rs` rpath. |
| `datagen`: own `B` features | **necessary** | 38 gated sites. |
| `user`, `runtime`, `data`: own feature names | **necessary** | They gate tests that the `--workspace --features B` cascade turns on; without the name the cascade silently compiles those tests out. |
| Workspace-table `default-features = false` on data, runtime, store, user | **necessary** | Dropping it changes store selection in 15 to 43 selections (kyotocabinet plus bdb reaches the two-backend `compile_error!`). |
| Crate-level `default-features = false`: capi→data, capi→facade, zhuyin→data, zhuyin→facade, dictool→facade, dictool→user, datagen→store, datagen→data, oracle→data, oracle→user (dev) | **necessary** | Each changes store selection in 6 to 9 selections when dropped. |
| `corpus`→`training` (dev, `default-features = false`) | **necessary** | Dropping it adds bdb under `--workspace --features kyotocabinet`. |
| `capi`, `zhuyin-capi`: `runtime/B`, `data/B`, `user/B` | **redundant** | Identical resolution, singly and jointly, with `facade/B` retained. |
| `datagen`: `store/B` | **redundant** | `testsupport/B` retained reaches the store (at the audited SHA). |
| `dictool`: `user/B`; `facade`: `user/B`; `runtime`: `data/B`; `user`: `store/B` | **redundant** | The chain through the retained edge reaches the store. |
| `capi`/`zhuyin-capi` `facade/B`; `dictool` `facade/B`; `facade` `runtime/B`; `runtime` `user/B`; `user` `data/B`; `datagen` `testsupport/B` | **redundant, flag-only** | Each is an alternative to a row above, so at most one of each pair can go. Removal changes only the enabled flag on facade, runtime, data or testsupport, none of which has compiled backend-gated code (their gated code is test-only). This rests on the gating inventory. |

Two more entries also resolved identically when the audit removed them one at a time. They are `oxpinyin-store`'s `kyotocabinet` and `tkrzw` `dep:bindgen`. They are not forwarder edges. They are not among the 33.

They are necessary. `crates/oxpinyin-store/build.rs` calls `bindgen::Builder` under those features. See the `store` row above.

After the audit removes the 33 redundant edges, two chains remain. The first chain is capi, zhuyin-capi and dictool → facade → runtime → user → data → store. The second chain is datagen → testsupport → store. The store selection and the own flag of each crate resolve identically across all 125 selections.

## 3. Q2: backend-concrete types crossing a boundary

| Item | Producer → consumers | What the consumer does | Observable / compat-required | Existing interface | Verdict |
| --- | --- | --- | --- | --- | --- |
| `DefaultStore` | store → data (`dict.rs:93,98`, `bigram_table.rs:56`, `punct.rs:58`, `table.rs:78`); user (`persistence.rs:754,848,958,1767,1796`, `registry.rs:108`, `store.rs:946,2736`, `store_libpinyin.rs:182,278`) | Opens or writes stores. In data it is boxed at once into `Box<dyn ChewingDbm>` (`ChewingTable`, `PhraseTable`, `BigramTable`) and does not leave the crate. | no | `ReadStore`, `WriteStore`, `RawReadStore`; `ChewingDbm` inside data | **not valuable**: opening stores is the job |
| `DefaultUserBigramDb` | store → user (`registry.rs:96`, `persistence.rs:998,1339,1410,1493`) | Opens the user bigram DB; stays inside user. | no | `UserBigramDb` trait | **not valuable**: constructing stores is the job |
| `UserStore` (alias of `GenericUserStore<DefaultStore>`) | user → runtime (`lib.rs:344,1159,1420,1512,1573,1687,1876`), facade (`context.rs:310,394,684`, `instance.rs:55`, `predict.rs:18`, `export_rows.rs:34`), capi (`state.rs:104`, `predict.rs`, `iterators.rs`), zhuyin-capi (`state.rs:88`, `iterators.rs:18`), dictool (`context.rs:40`, `import.rs:103`), oracle bench, fuzz | Constructors (`open_libpinyin*`, `open_transient`, `check_profile`) are the job; the rest passes the handle through or calls generic inherent methods. None names a backend. | the type: no | `GenericUserStore<S: WriteStore>` and `UserLookup` | **not valuable**: using the generic needs generics in runtime and facade (ruled out); parameterising only capi's helpers leaves `ContextCore.user` concrete |
| `RuntimeDict`, `RuntimeLm` | runtime → facade (`instance.rs:58,61`), capi, zhuyin-capi | The compiled-in backend's runtime view; the only backend-typed field is a private `Option<UserStore>`. The engine consumes them through `Session<RuntimeDict, RuntimeLm>` via core's `Dictionary` and `LanguageModel`. | no | core traits already used by the engine | **not valuable**: library load/unload, persistence and pin-specific errors in `ContextCore` are concrete jobs |
| `DEFAULT_STORE_DB_FORMAT` | store → data (`table_info.rs:341`, `user_files.rs:215`); user (tests) | Reads backend identity as the `database format:` token. | **yes.** Pin: `src/storage/table_info.cpp:122-130,180-188` (tokens), `:399-413` (`is_conform`), `configure.ac:134-135`, `libpinyin.pc.in:7`, `libzhuyin.pc.in:5` | `DatabaseFormat` enum (`data/table_info.rs:76`) | **not valuable**: observable and required |
| Literal tokens in `capi`/`zhuyin-capi` `build.rs` (`database_format()`) | feature flags → build script | A second copy of the identity, tied to the store constant only by `crates/oxpinyin-capi/tests/database_format.rs`; zhuyin has no such test. | **yes**, same pin lines | none | **not valuable**: a build-dependency on the store would put bindgen in the build graph |
| `DEFAULT_STORE_IS_LIBPINYIN_DBM` | store → data (`user_files.rs:121,953`, `system_files.rs:66`); testsupport (`model_cache.rs:249`); datagen test (`backend_default.rs:50`) | Branches on a constant that is always `true`; the `else` arms are unreachable. | no. Pin file names never vary by backend (`src/pinyin_internal.h:57-66`). | none needed | **valuable**: dead identity reads |
| `DEFAULT_STORE_EXT`, `default_store_file` | store → data (re-export `lib.rs:66`) → runtime (`lib.rs:51,67`), capi/zhuyin-capi tests, oracle bench (`scan_perf.rs:108`), fuzz (`capi_commands.rs:112`); training re-export (`segment.rs:29`, no consumer) | Non-test readers are the dead `else` arms above and `runtime::user_store_file()` (no in-tree caller). The rest are fixture-directory lookups (`fixtures/w3/<ext>`). | no. The extension is internal naming. | none needed | dead readers and the unused re-export: **valuable**. Fixture lookups: **not valuable**. `fuzz/` needs the data re-export, so it stays. |
| `KcStore`, `TkrzwStore`, `BdbStore` (+ `*UserBigramDb`) | store → datagen (`write.rs`, per `--backend`), user test macros (`store.rs:4247-4251`) | A producer tool writing the chosen backend, plus per-peer tests. | no | traits exist; datagen already uses them generically per arm | **not valuable**: concrete job |
| `LookupTable`, `GenericLookupTable` | data re-export (`lib.rs:73`) | No consumer outside `table.rs`. | no | n/a | **valuable** only as dead-export cleanup |
| `oxpinyin-testsupport` → `oxpinyin-store` | store → testsupport (`model_cache.rs:249-259`, its only use) | Calls the dead `IS_LIBPINYIN_DBM` branch; both arms yield the same libpinyin names. | no | n/a | **valuable**: see G1 |

## 4. Q3: candidates

### Genuine decoupling

**G1: testsupport no longer depends on the store.** `system_dbm_names()` always takes the libpinyin-name arm. G1 removes the `oxpinyin-store` dependency, the three backend features of testsupport and the `testsupport/B` edge of datagen.

- `oxpinyin-testsupport` no longer knows a backend. After G1, the store leaves the graphs of `oxpinyin-core`, `oxpinyin-engine` and `oxpinyin-testsupport` **only**. `oxpinyin-capi` and `oxpinyin-training` keep the store through their own normal dependencies. `oxpinyin-corpus` keeps it through `oxpinyin-training`.
- Before G1, `cargo test --locked -p` failed for testsupport, core, engine and corpus. Each failed at the "no store backend selected" `compile_error!` of the store (§6). In #769, the first three pass. Corpus is unchanged.

G1 meets the constraints:

- G1 adds no new trait or method.
- G1 adds no dynamic dispatch.
- G1 does not make runtime or facade generic.
- G1 does not confine backend selection to the cdylibs.
- G1 does not change the bdb default, the exactly-one `compile_error!` guards or the bare-name cascade.
- The set of members that declare `<peer>` becomes smaller by testsupport. The cascade results are identical.
- G1 does not change the three-backend verification rules.
- The file names are identical in both arms. libpinyin compatibility is not affected.
- The independent builds (dictool, datagen, training, oracle, both cdylibs, `fuzz/`) resolve identically.
- `--features oxpinyin-testsupport/<backend>` no longer resolves. Only dated records use that form.

**G2: dead identity reads in data and runtime.** G2 removes these items:

- the `DEFAULT_STORE_IS_LIBPINYIN_DBM` const;
- the `else` arms of the const in `data/system_files.rs` and `data/user_files.rs`;
- `runtime::user_store_file()`;
- the training re-export of `default_store_file`;
- the data re-export of the const.

After G2, the non-test code of data does not read `DEFAULT_STORE_EXT` or the flag. Runtime does not export a helper that names the backend. `data::DEFAULT_STORE_EXT` stays, because `fuzz/` and several test fixtures use it. After G2, one non-test reader of backend identity remains outside the store and datagen. It is `DEFAULT_STORE_DB_FORMAT` in data. Compatibility requires it.

### Cosmetic

- **C1:** remove the 33 redundant forwarder edges that §2 lists. This simplifies the manifests only. No crate loses knowledge of the backend. The resolution is identical across 125 selections.
- **C2:** remove the `data::LookupTable` and `GenericLookupTable` re-exports. They have no consumer.

### Rejected

- Make runtime, facade or capi generic over the store: ruled out (generics in runtime or facade). This gives no independence.
- A trait or `dyn` seam over `UserStore`, `RuntimeDict` or `RuntimeLm`: ruled out (new traits, dynamic dispatch).
- Move backend selection into the two cdylibs: ruled out.
- Remove the `bdb` default, the `compile_error!` guards or the bare-name cascade: ruled out, because each is a change to the selection semantics.
- Read `DEFAULT_STORE_DB_FORMAT` through `DatabaseFormat` alone: not eliminable, because the token is observable from outside and compatibility needs it.
- Add a build-dependency from the cdylibs on the store for the `.pc` token: not eliminable under the constraints. It adds coupling. It puts bindgen in the build graph.
- Remove the own backend features of capi or zhuyin-capi: `build.rs` and `cinstall --features` read them.

## 5. Undetermined at the audited SHA

1. Compile result for the necessity of `dep:bindgen` on kyotocabinet and tkrzw. Run `cargo check -p oxpinyin-store --no-default-features --features kyotocabinet` (and `tkrzw`) on a copy without the element.
2. Whether the flag-only edges change a compiled artifact. Run `cargo test --locked --workspace --no-run` in each backend cell on a copy without them.

The audit settled two items since the Phase 1 report. The first is the default `-p` builds of core, engine, testsupport and corpus (§6). The second is the lockfile effect of G1: one line each in `Cargo.lock` and `fuzz/Cargo.lock`.

## 6. Baseline recorded on the pinned toolchain

The toolchain is `rustc`/`cargo` 1.97.1 (`rust-toolchain.toml`). The tree is `origin/main` at `5ef525848f38a985900c0785834683eead973db4`. The command is `cargo test --locked -p <crate>`.

| Crate | Result |
| --- | --- |
| `oxpinyin-testsupport` | exit 101, `crates/oxpinyin-store/src/lib.rs:61`, "no store backend selected" |
| `oxpinyin-core` | exit 101, same |
| `oxpinyin-engine` | exit 101, same |
| `oxpinyin-corpus` | exit 101, same |

## 7. Rulings

Phase 1 is accepted. These are the rulings:

- **G1: authorised; implementation: #769** (`refactor(testsupport): drop the oxpinyin-store dependency`).
- **C1: authorised; implementation: #771** (`build: drop redundant backend feature forwarders`, on #769).
- **G2: authorised; implementation: #772** (`refactor: remove dead backend file-naming branches`, on #771).
- **C2: authorised; implementation: #773** (`refactor(data): drop unused LookupTable re-exports`, on #772).
- All 20 crates stay. The project does not make `oxpinyin-runtime` or `oxpinyin-facade` generic over the store (§4, Rejected).

Measured result for G1: in #769, `cargo test --locked -p` passes for `oxpinyin-testsupport`, `oxpinyin-core` and `oxpinyin-engine`. `oxpinyin-corpus` is unchanged. It still reaches the store through `oxpinyin-training`.

Measured result for C1: the audit recomputed the redundant set on the graph after G1. It used the Phase 1 method: 125 selections that include `fuzz/`. The result is **30** redundant forwarder edges. The audit removed all 30 together. Every selection stayed identical.

The `store/B` edge of datagen became necessary after G1. It is now the only route of datagen to the backend, so it stays. The "flag-only" edges also stay.

Correction to the Phase 1 count: the audited SHA has 33 redundant forwarder edges, not 35. Two other entries resolved identically when the audit removed them one at a time. They are `oxpinyin-store`'s `kyotocabinet` and `tkrzw` `dep:bindgen`. They are not forwarders.

The source shows that these two entries are necessary: `crates/oxpinyin-store/build.rs` calls `bindgen::Builder` under those features. The joint removal at the audited SHA covered the 33 forwarders. The Method section, §2 and §4 give the corrected figure of 33.

Correction to the Phase 1 report: G1 removes the store only from the graphs of `oxpinyin-core`, `oxpinyin-engine` and `oxpinyin-testsupport`. The Phase 1 report said that G1 also removes the store from the graphs of capi, corpus and training. That was wrong. Capi and training keep the store through their own normal dependencies. Corpus keeps it through training.

`docs/findings/sentence-surface.md` and `docs/findings/scan-matrix-fanout-2026-09-10.md` are dated records. They show commands that the author ran with `--features oxpinyin-testsupport/<backend>`. This stack does not edit them.
