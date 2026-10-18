# Dynamic dispatch inventory — what was erased, what replaced it

> Current measurements use main a36a23b0 / PR source 519ec6ed / pin 074a2219 on one Linux host. Current verification, shipping size, commands and artifact identities follow the explicitly historical sections. The original historical size column was GNU `size` aggregate text, not actual ELF `.text`; values are retained and labeled accordingly. No size regression is declared accepted. Raw profiler captures/logs/archives are neither saved nor uploaded. The complete 162-run cost campaign and the pending maintainer decision are recorded below.

## Incoming source audit

The incoming range is `25ba78f8263e5e107ae38ea0221069c00e451db1` to
`dd55aeb583abd475a6f9d72a592726a2d32ed1de`: system-unigram persistence
(#878), packaging section garbage collection (#881), and oracle backend
feature forwarding (#887). A search of added lines over that range finds
no new `dyn`, `&dyn ReadStore`/`WriteStore`, or `Box<dyn` boundary.

`UserStore::flush_system_unigrams` uses `db.write` with the associated
concrete transaction and calls the already-generic transaction helpers.
`save_reporting_inner` keeps the incoming directory/modified guards before
calling persistence; its baseline remains `Option<B>` with `B: FnMut`.
The guard tests' panic closures are concrete closures. Persistence stages
carry the same generic baseline. No new callback seam requires conversion.
The facade's layout-fault callback remains the previously documented
`dyn FnMut`. No ablation or boundary trimming is performed in this campaign;
its earlier individual-boundary saving remains a historical estimate.

The replayed-readings repair from `28d0a326` is folded into the first
store/data refactor (original `9e0834c5`), where the trait-object API ceased
to compile. Per-commit compile/test checks have passed (table below). Current-base verification and shipping size results follow below.

Historical campaign scope (2026-10-18 UTC): The change is the second member of a
two-PR stack: PR A (`docs/c-api-sole-external-interface`) makes the C APIs
the only supported external interfaces; this one uses the resulting freedom
to change Rust signatures. Figures below compare PR A's tree (the
baseline, `85cfdcd` on `main` `6d9e23b`) with the code commits of this
change. That record predates the incoming-file overlap audited above;
its figures are not post-rebase results.

Authority: Rust interfaces are not a compatibility boundary (`AGENTS.md`,
"External interfaces"; maintainer ruling 2026-10-18 UTC). The C APIs and
ABIs, their behaviour, the supported persistent data and the required
diagnostics are untouched; every Rust signature below was free to change
and every in-tree caller changed with it.

Method: `rg '\bdyn\b'` and `rg 'Any|TypeId|downcast'` over `crates/`,
`fuzz/`, `tests/` and `tools/` (library, binary, test, bench, example and
fuzz sources), then each type alias that hides a trait object
(`Visitor`, `Row`, `RowVisitor`, `ExtensionVisitor`) was followed to its
uses. A boundary was judged by its concrete implementations and call
sites, not by how it is spelled. "Static" below means monomorphised: a
removed `dyn` says nothing about allocation, which is reported separately.

## Removed

"Path" says where the boundary sat: **ship** = reached by the shipping C
facades (both `libpinyin.so.15` and `libzhuyin.so.15`), **tool** = reached
only by an in-tree command, **test** = tests, benches, examples or fuzz
targets only.

| # | Boundary (item) | Path | Concrete implementations / call sites | Why it was erased | Replacement |
|---|---|---|---|---|---|
| 1 | `WriteStore::write(f: impl FnOnce(&mut dyn WriteTxn))` and every helper taking `&mut dyn WriteTxn` / `&dyn WriteTxn` (`oxpinyin-user`: `store.rs`, `store_libpinyin.rs`, `phrase_table.rs`; datagen; dictool; benches) | ship | `BdbTxn`, `KcTxn`, `TkrzwTxn` — exactly one is compiled into a build (`DefaultStore`) | The seam was written object-safe so a transaction could be handed around as a trait object | `WriteStore::Txn<'a>` (a lifetime-generic associated type); `write` hands the closure `&mut Self::Txn<'_>`; helpers take `&mut impl WriteTxn` |
| 2 | `Visitor<'a> = dyn FnMut(&[u8], &[u8]) -> Result<(), StoreError>` in `ReadStore::{range, for_each}`, `WriteTxn::{range, for_each}`, `RawReadStore::range_raw` | ship | Closures at about 85 call sites; one backend per build | Object safety of the three store traits | `impl FnMut` visitors; one required scan per tier (`scan`, `scan_raw`) that answers `ControlFlow`, with `range` / `for_each` / `range_raw` provided on top. The cursor logic stays out of the generic code (`TableRows::next` / `MergedRows::next` in Berkeley DB, `TableRows::next` in Kyoto Cabinet, `drive_scan` in tkrzw), so each call site instantiates only a thin loop — see "Binary size" |
| 3 | tkrzw `Row<'a>` (`dyn FnMut` carried through `ScanCtx` into the C record callback) | ship (tkrzw build) | One closure per `scan` call | A `void*` callback argument looked like it needed a fat pointer | A generic trampoline `walk_row::<F>`; the `ScanCtx<F>` is cast back to the type it was made from (same `unsafe` kind, same module-scoped allow, SAFETY text updated). The shared walk state sits in `Cell`s, so the loop and the callback never hold aliasing `&mut` |
| 4 | Berkeley DB `WriteTxn::is_empty`: `StoreError::Backend("oxpinyin-store: stop walk")`, recognised by `to_string()` | ship (bdb build) | One site | Error-based control flow: the visitor had no stop channel | `ControlFlow::Break` |
| 5 | `RawChewingDbm::walk`: private `WalkStopped` sentinel put in `StoreError::Backend(Box<dyn Error>)` and recovered by `downcast_ref` | ship | The one adapter over `RawReadStore` | Same: stop and visitor failure rode the error path | The visitor's stop and failure both `Break` the store scan; the failure is parked beside the scan and returned as itself |
| 6 | `table::for_each_row_with_store`: visitor error smuggled as `StoreError::Backend("visitor error")` | ship (system-table loader) | Every `LookupTable::open` | Same | `ControlFlow::Break` + parked failure |
| 7 | `Box<dyn ChewingDbm + Send + Sync>` in `ChewingTable`, `PhraseTable`, `BigramTable`, `PunctTable` (`Option<Box<…>>`), their `new` constructors, `RowVisitor` / `ExtensionVisitor`, and `Arc<dyn ChewingDbm …>` in one test | ship | Production: `RawChewingDbm<DefaultStore>` (every `open_*`), one type per build. Test doubles: `MemoryDbm` (×4), `Counted` | The compiled-in store was erased immediately after it was opened | The tables are generic over `D: ChewingDbm` with `DefaultDbm = RawChewingDbm<DefaultStore>` as the default parameter, so `ChewingTable` etc. still name the production type everywhere else; `walk` takes `impl FnMut`. The memory/counting doubles drive the same table code through `D` |
| 8 | `&mut dyn GramOps` (`mask_grams`, `gram_mask_fault`) and `TableGrams(&mut dyn WriteTxn)` | ship | `ContainerGrams`, `TableGrams`, `ReadGrams`, each named at its call site | The helpers took the trait object so one body served all three | `impl GramOps`; `TableGrams<'a, T>` |
| 9 | `&dyn Fn(Token) -> bool` in `IndexRows::read` and `remove_reloaded_rows`; `visible: &dyn Fn(u32) -> bool` in `visible_extension_exists` | ship | One closure each | Predicates passed by reference to avoid a generic | `impl Fn` |
| 10 | `SaveReporting.baseline: Option<&mut dyn FnMut(u8) -> SaveBaseline>`, `save_reporting_with_baseline`, `save_reporting_inner` | ship | The facade's closure, or none | A hook that is present or absent, never of two types | `SaveReporting<B>` with `Option<B>`; saves without a hook use the `NoBaseline` fn type |
| 11 | `readings: &mut dyn Iterator<Item = Vec<ChewingKey>>` in `UserModel::observe_with_keys`, `train_pronunciations` | ship | The engine's concrete reading iterator; tests pass arrays | Iterator by reference | `impl Iterator` |
| 12 | `config: &dyn ConfigSource` in `Session::new`, `new_with_key_costs`, `init`, `Settings::read`, and `Runtime::new_session` | ship | `EmptyConfigSource`, `Config`, `ConfigLayer`, a test map | A once-at-construction read | `&impl ConfigSource`; `init` receives the already-read `Settings`, so the large generic `init` is not instantiated per config type |
| 13 | `TrellisPathMemo(OnceCell<Box<dyn Any + Send>>)` and `Dictionary::trellis_item_on_path(…, &TrellisPathMemo)` | ship | Producer and consumer are one pair: `RuntimeDict` fills it with `PathPricing`; the engine's span pricing files one memo per key path and drops it with the span search. Every other dictionary ignores it | Type erasure with a runtime downcast and a "memo of another type" fallback | `Dictionary::PathMemo` (an associated type; `()` for dictionaries with nothing to keep) and `TrellisPathMemo<T>`; `SpanRanges<M>` keeps `Vec<D::PathMemo>` beside the paths. Lifecycle unchanged; the foreign-memo case is unrepresentable |
| 14 | `StoreError::Backend(Box<dyn Error + Send + Sync>)` | ship | About 65 producers, every one a message string, plus tkrzw's code-and-message status; nothing downcasts any more (rows 4–6) | Free-form payload | `BackendError { code: Option<i32>, message }`, same `Display` text; `StoreError::source()` still chains to it |
| 15 | `ToolArgs::parse(extra: … &mut dyn Iterator<Item = String>)` | tool | The counting tools and `oxpinyin-lambda`, each passing its own closure over the same iterator type | Closure parameter | Generic over the iterator type |
| 16 | `permute(emit: &mut dyn FnMut …)` in a `scheme.rs` test | test | One test | Recursive helper | `impl FnMut` with the recursion over `&mut F` |
| 17 | `&dyn oxpinyin_store::ReadStore` cast in a user-store test | test | One test | Method resolution | The trait imported |

## Retained, with the reason

| Boundary | Path | Reason it stays |
|---|---|---|
| `on_layout_fault: &mut dyn FnMut(&LayoutFault)` in `ContextCore::save_user_with_layout_faults`, the two C-facade `state.rs` wrappers and their one caller each | ship | A demonstrated trade-off, not a heterogeneity of types. Making the hook `impl FnMut` specialised the whole 13 KB save path (`save_user_with_layout_faults`, which inlines the user store's staging and diffing) for a callback that runs only when a library's layout is refused: **+16.5 KB of GNU size text** in the Berkeley DB build, no change in the work done. The hook cannot be collected and replayed instead: its order against the file writes is pinned (a fatal critical stops the save before the failing library's write and after the earlier ones, `pinyin.cpp:956`, `zhuyin.cpp:589`) |
| `fn source(&self) -> Option<&(dyn std::error::Error + 'static)>` — 23 `impl std::error::Error` blocks | ship, tool | The signature is `std`'s. `source()` keeps standard error chaining for in-tree tools and the facade's diagnostics; it is interoperability with the standard library, not a promise about oxpinyin's own Rust API |
| `Box<dyn std::error::Error>` as the result of the 14 command-line binaries' `run`/`main` bodies (`oxpinyin-dictool`, `oxpinyin-counter`, `-emitter`, `-lambda`, `-segment`, `-spseg`, `-mergeseq`, `-train`, `-eval`, `-punct`, `-word`, `-corpus`, `-kmm`) and `tool_cli::run`'s `fn()` pointer | tool | Top-level error aggregation: each body `?`s `io::Error`, `FromUtf8Error`, parse errors and several crate error enums into one value that is printed once and mapped to an exit code. That is real heterogeneity at a cold, once-per-process boundary; an enum per binary would restate the `From` impls the box already gives |

Third-party and generated code is untouched. The `std::any::TypeId`
comparisons in `oxpinyin-store`'s tests assert which type `DefaultStore`
resolves to; they erase nothing.

## Dyn is not allocation

* `Box<dyn Error>` from a string and `BackendError` from a string both
  allocate the message once; row 14 changes the type, not the allocation
  count.
* A `Box<ZeroSizedType>` allocates nothing, so the old `WalkStopped`
  sentinel (rows 4–6) allocated nothing; what it cost was a branch on the
  error path, an `Err` per stop and a `downcast_ref`, now a `Break`.
* The steppers (`TableRows`, `MergedRows`) hand out the same owned rows the
  closures did; the merge keeps its buffered rows in one `Vec` as before.
  In the historical campaign, peak heap was equal to within 100 bytes in every measured workload
  (below).

## Historical binary size (pre-rebase campaign)

Static visitors have a price: `impl FnMut` instantiates the callee once per
closure. The first cut of rows 1–2 (store traits generic, cursor loops left
inside the backends) grew `libpinyin_capi.so`'s GNU size text by **+204 KB (+8.4 %)**
in the Berkeley DB build, because the cursor loops and the write
transaction's merge were instantiated at each of about 85 call sites. The
steppers and shared commit bodies (`#[inline(never)]` on the shared bodies
and on the transactions' point operations) bring that down to the figures
below. Release profile (`lto = "fat"`, `codegen-units = 1`), `cargo build
--release -p oxpinyin-capi --no-default-features --features <backend>,shipped`
on both trees, `size` for GNU size text, `stat` for the file:

| backend | baseline GNU size text | candidate GNU size text | change | baseline file | candidate file | change |
|---|---:|---:|---:|---:|---:|---:|
| Berkeley DB | 2,416,604 | 2,457,576 | +40,972 (+1.70 %) | 3,193,728 | 3,212,664 | +18,936 (+0.59 %) |
| Kyoto Cabinet | 2,434,552 | 2,482,280 | +47,728 (+1.96 %) | 3,220,408 | 3,245,224 | +24,816 (+0.77 %) |
| tkrzw | 2,472,891 | 2,523,879 | +50,988 (+2.06 %) | 3,262,064 | 3,303,824 | +41,760 (+1.28 %) |

So this change **does not shrink the library**; it grows it by about 2 % of
GNU size text and 0.6–1.3 % of the file. Where the growth was attributed by
ablation on the Berkeley DB build (one boundary reverted at a time on the
final tree; each build is a fat-LTO link):

* visitors static rather than `&mut dyn FnMut` (with the steppers kept):
  +14.9 KB — the thin per-call-site loops; no instruction-count benefit
  (below), kept because the callbacks and transaction types were to be
  retained through the chain and the cost is 0.6 % of GNU size text;
* the layout-fault hook generic: +16.5 KB (retained as `dyn`, above);
* `GramOps` and the predicates generic: +3.8 KB together;
* the baseline hook and the reading iterator generic: no measurable
  change (±1 KB);
* the concrete `BackendError` instead of `Box<dyn Error>`: **−7.6 KB**;
* the typed path memo (`PathMemo`): −1 KB.

The rest of the growth (about 26 KB) is spread over the user store's
transaction code, which the compiler now specialises and inlines across the
former `dyn` boundary; it does not attribute to any one boundary.

## Historical generated code (pre-rebase campaign)

Structural evidence from the shipped Berkeley DB `libpinyin_capi.so`
(`nm -C`, `objdump -d --no-show-raw-insn`), baseline against candidate:

* Closure vtable shims (`{shim:vtable#…}`) and vtable-reached `dyn` entry
  points: **56 → 10**. Of the 10, 9 are `std` (`Once`, backtrace printing,
  the unwinder's personality routine) and 1 is the retained layout-fault
  hook.
* Register-based indirect calls (`call *0x…(%reg)`, i.e. not through the
  GOT): 1,814 → 1,646 over the whole library; the remainder is mostly libdb's
  own `DB`/`DBC` method tables, a foreign interface.
* `oxpinyin_user::store::txn_get_u64_or`: baseline `call *0x18(%rax)` — the
  `WriteTxn::get` vtable slot; candidate `txn_get_u64_or::<BdbTxn>` has no
  register-indirect call, `BdbTxn::get` is inlined or called directly.
* `<BdbTxn as WriteTxn>::{get, put}` and `<RawChewingDbm<BdbStore> as
  ChewingDbm>::{get, walk}` exist in the baseline only as vtable targets;
  in the candidate they are ordinary direct callees (the `#[inline(never)]`
  point operations) or absent.

This is a statement about the code that was generated, not about speed.

## Historical measured effect (pre-rebase campaign)

Workloads from `docs/runbooks/benches.md` ("BDB guess, training, and
import profiles"): the harness `tools/bisection/system-import-profile.c`
(`cc -std=gnu11 -O2 … -ldl`) loads the `.so` and replays the frozen
sentence fixture (500 guesses), 200 guessed training steps with one save, or
10,000 phrase imports with one save and an export. Same libpinyin data
directory per backend (the oracle's own, provisioned by
`tools/cloud/setup.sh`), a fresh empty user directory per run, CPU 0
(`taskset -c 0`), Valgrind 3.22.0; three runs per cell; whole-process
Callgrind `Ir` and Massif `mem_heap_B`. Both trees' stdout SHA-256 equals the
registered value for every workload and backend (`50dfc9c8…`,
`e5855249…`, `34e98cd8…`).

| backend | workload | baseline `Ir` | candidate `Ir` | change | peak heap change |
|---|---|---:|---:|---:|---:|
| Berkeley DB | guess ×500 | 2,623,663,508 | 2,625,622,318 | +0.075 % | −16 B |
| Berkeley DB | train ×200 + save | 381,992,426 | 382,101,754 | +0.029 % | +32 B |
| Berkeley DB | import ×10,000 + save + export | 738,068,489 | 738,736,243 | +0.090 % | −96 B |
| Kyoto Cabinet | guess ×500 | 3,039,155,314 | 3,042,294,729 | +0.103 % | +32 B |
| Kyoto Cabinet | train ×200 + save | 615,743,955 | 616,827,083 | +0.176 % | +32 B |
| Kyoto Cabinet | import ×10,000 + save + export | 810,380,808 | 810,813,022 | +0.053 % | −96 B |
| tkrzw | guess ×500 | 2,836,624,830 | 2,828,676,952 | −0.280 % | +32 B |
| tkrzw | train ×200 + save | 588,517,901 | 588,415,328 | −0.017 % | +32 B |
| tkrzw | import ×10,000 + save + export | 869,899,921 | 870,292,522 | +0.045 % | −96 B |

Callgrind `Ir` is deterministic (spread ≤ 0.0004 % over the three runs).
**The change is neutral on instructions and on peak heap**: within
±0.3 % in either direction. The Berkeley DB variant with `&mut dyn FnMut`
visitors instead (steppers kept) differs from the candidate by less than
0.005 % `Ir` on every workload, so the per-row virtual call was not a cost
worth removing on these paths: a visitor runs once per row of a cursor step
that is an FFI call.

Criterion timing (`backend_matrix_bdb`, `phrase_read`, `export_phrases`,
`session_keystroke`, `stage2` without its sentence group, and
`open_profile`'s phases; `--sample-size 50 --measurement-time 3`, five
interleaved rounds, one process at a time) has run-to-run spread of 10–50 %
on this container and no consistent direction: candidate-to-baseline ratios
of the medians span 0.85–1.14 with both signs on almost every group, and the
dyn-visitor variant spans the same range. It is reported as "no measurable
change", not as a win or a loss; the `Ir` table above is the number to
trust.

Not measured: wall-clock time for the Kyoto Cabinet and tkrzw builds
(instruction counts only), RSS beyond Massif's heap, and any workload
through a C consumer other than the profile harness.

## What the change is not

* It does not alter the C ABI, either facade's export set, ownership rules,
  the diagnostics, the data formats or the oracle pin.
* It is not a claim that every `Box` or `dyn` is gone: the table above lists
  what remains and why.
* It does not make the Rust interfaces any more or less stable: there is
  nothing to stabilise (`AGENTS.md`, "External interfaces").


## Current-base verification

Main/base `a36a23b054d9da33e0a60a54ebe67af95e32296b`; #872 `c4e24899f1f27af46414709280c8b58da1352b83`; measured #879 source `519ec6ed3f6f6004c09f8f8236f449ca36ca3db8`; pin `074a2219c90feaf962d0d24f034514033ece5f99`. Production crates tree `9fb8ba0e57b14f6747d14ce5ed20c25c5b9e7606`. The final findings-only amendment does not change this shipping source. Both rebases were clean; #879 remains based on #872's branch. Main's phrase-text stack decoder and Cow borrowed/owned handling are preserved. The incoming normalizer is landed main, not a pending branch cherry-pick; no new dyn boundary was found in later incoming changes. The original source audit above retains its named range.

Original #872 base/head: `25ba78f8263e5e107ae38ea0221069c00e451db1` / `b09ea41f93b069342ddfd8a32509bba8c820e259`. Original #879 base/head: `b09ea41f93b069342ddfd8a32509bba8c820e259` / `28d0a3264e617c97d0ddc36ae81ce65871baa521`. Latest code-restack exact leases: #872 `25f0a4bd53b4454ecab752a60b7b83abaf94ad40`, #879 `0407930debaf2527251e8883fa57d01018cecac3`. Both published current source heads had all 16 Actions checks and CodeRabbit successful. #872's specified thread is resolved; no other thread was resolved.

`28d0a326` is folded into the first refactor (original `9e0834c5`). On new main, each code member passed `cargo test --offline --locked --all-targets --no-run -p <every touched crate>` (full linking, not just cargo check), followed by `cargo test --offline --locked -p <every touched crate>` on BDB:

| Member | Passed | Failed | Ignored |
|---|---:|---:|---:|
| `87c71a6d7de042fe6b606cfc077ddf3fbb804109` | 476 | 0 | 7 |
| `ff02d4e5b554bd8e151937fec709515cd28877f5` | 1120 | 0 | 15 |
| `f3a599c721643d70039fd5d85826bd3510f4d79d` | 953 | 0 | 6 |
| `9c5c326dcd3eabde11df2af5fdd038e39a04dc42` | 48 | 0 | 4 |
| `3b03bc5a9b6edb539c931a79db3c22433f452438` | 50 | 0 | 4 |
| `33dbc61438e3d1e7f06ef1c6aa1a385359f3ccfe` | 235 | 0 | 4 |

The whole-stack touched union is core, data, datagen, engine, facade, runtime, store, testsupport, training, user, pinyin-oracle and both C facades. Clippy uses `--all-targets -- -D warnings`; tests use explicit `-p` selection, never a workspace sweep. Each package with a backend feature selects that backend and disables defaults. Single-job builds, incremental disabled, retained separate targets per backend, task-owned on-disk TMPDIR and offline/locked dependencies. Fmt/diff checks passed; final findings-only changes require fmt/diff under rule 7, no rebuild.

| Cell | All-target clippy | Tests passed / failed / ignored | Dev builds, exports, pc-version, C++ smoke, user/system interchange | Allocator ABI | Default contract coverage |
|---|---|---|---|---|---|
| bdb | PASS | 1321 / 0 / 24 | PASS | PASS | 2786 MATCH + 1 SKIP in full sandbox invocation; four blocked wrap cases separately MATCHed outside sandbox |
| kc | PASS | 1319 / 0 / 22 | PASS | PASS | 2778 MATCH, 13 backend-inapplicable SKIP, zero unexpected/blocked, exit 0, 1781.6 s |
| tkrzw | PASS | 1325 / 0 / 22 | PASS | PASS | 2778 MATCH, 13 backend-inapplicable SKIP, zero unexpected/blocked, exit 0, 1646.8 s |

The existing default contract selection is 2791 cases and excludes the separate axis-C battery. No axis-C pass is claimed here. Commands: `tools/bisection/run-contract-diff.sh <cell> <oracle-prefix> <debug-pinyin-so> <debug-zhuyin-so>`. Subjects use the prescribed `CARGO_PROFILE_DEV_OPT_LEVEL=1` dev profile, with debug assertions, as documented by `docs/runbooks/parity-gates.md` and exported by `tools/bisection/oracle-cell.sh`; the direct contract runner consumes supplied artifacts and never builds them. Shipping release profiles below are separate. All three cells' two save-fatal-valid-to-invalid cases MATCHed; #886 item 1 differences did not appear. The incoming normalizer's eight unit tests passed earlier and its source is unchanged.

Allocator gates ran outside the sandbox: all 26 pinyin and 12 zhuyin slots exercised/released, both LeakSanitizer negative controls fired on each backend. Existing driver/assertions unchanged; intermediate observations were routed to anonymous memory. User-directory interchange and all four system-library interchange checks passed. Both tkrzw subjects and both oracles resolved the sound 1.0.34 prefix. Shipped export gates passed on all three unversioned Cargo endpoints. One initial export-check invocation mistakenly supplied a versioned installed zhuyin library and rejected the @@LIBZHUYIN decoration despite finding all 52 names; using the correct unversioned cargo-c artifact passed without changing code or assertions. Installed metadata (both libraries' headers, .pc files, pkg-config queries and relocation) PASS on all three cells via the supported install wrapper with exact matching oracle prefixes and DESTDIR; query captures stayed in anonymous memory.

### BDB sandbox limitation and paired controls

The full BDB invocation took 2110.4 s and returned raw exit 1: 2786 MATCH, one SKIP and four terminated workers labeled DIFFER [0,-15]. Those four attempts remain BLOCKED/TIMEOUT, never retrospectively passes. Cases: wrap-256th-reading, wrap-256th-reading-same-iterator, wrap-256th-reading-zhuyin, wrap-256th-reading-zhuyin-same-iterator. Each worker was bounded at 150 s and the suite continued. Earlier inside-sandbox main-179de71c/old-PR attempts also blocked, but that did not establish a cause.

Current-main a36a23b0 controls outside the sandbox MATCHed all four ([0,0], expected_ok true): 12.7/9.2/1.3/1.3 s. The exact same PR debug libraries used by the blocked full run then MATCHed all four in that same outside-sandbox environment: 10.6/9.4/1.3/1.2 s. No source, oracle, compiler flag or workaround changed. BDB default coverage therefore comprises 2790 valid matched cases and one skip across the full invocation and targeted reruns, not a claim of a single exit-0 full invocation. The observed environment difference does not establish a PR code defect; the exact restricted native operation was not established. Control durations are diagnostic deadlines, not performance results.

### Post-rebase rule 2 output

Store paths:

- `crates/oxpinyin-store/benches/support/mod.rs`
- `crates/oxpinyin-store/examples/backend_bench.rs`
- `crates/oxpinyin-store/src/bdb/ffi.rs`
- `crates/oxpinyin-store/src/bdb/mod.rs`
- `crates/oxpinyin-store/src/common.rs`
- `crates/oxpinyin-store/src/kyotocabinet/mod.rs`
- `crates/oxpinyin-store/src/lib.rs`
- `crates/oxpinyin-store/src/tkrzw/mod.rs`
- `crates/oxpinyin-store/tests/bdb_libpinyin_files.rs`
- `crates/oxpinyin-store/tests/trait_laws.rs`

Changed existing non-Markdown files containing backend cfg:

- `crates/oxpinyin-data/src/chewing_table.rs`
- `crates/oxpinyin-datagen/src/write.rs`
- `crates/oxpinyin-datagen/tests/libpinyin_parity.rs`
- `crates/oxpinyin-runtime/src/lib.rs`
- `crates/oxpinyin-store/examples/backend_bench.rs`
- `crates/oxpinyin-store/src/common.rs`
- `crates/oxpinyin-store/src/lib.rs`
- `crates/oxpinyin-store/tests/bdb_libpinyin_files.rs`
- `crates/oxpinyin-user/src/lib.rs`
- `crates/oxpinyin-user/src/persistence.rs`
- `crates/oxpinyin-user/src/phrase_table.rs`
- `crates/oxpinyin-user/src/store.rs`
- `crates/oxpinyin-user/src/store_libpinyin.rs`

Backend-naming Cargo.toml added/deleted lines: none. The literal grep pipeline also reports the deleted #872 snapshot `docs/api/oxpinyin-engine.public-api.txt`; store and cfg matches independently require all three cells.

## Current shipping size attribution

All deltas below are actual ELF `.text` bytes, from the sequential existing stack members. The last row includes shared cursor/merge/commit/point-operation bodies and restoring the cold layout-fault hook to dyn. It was measured at findings-only head 519ec6ed, whose shipping source equals code member 33dbc614. LTO interactions mean these are whole-library marginal commit costs, not independent ablations or additive estimates for selectively undoing a boundary. No trimming was performed.

| Cell | Change | Pinyin `.text` delta B | Zhuyin `.text` delta B | Pinyin file delta B | Zhuyin file delta B |
|---|---|---:|---:|---:|---:|
| bdb | 87c71a6d store/data transactions and visitors | +180,912 | +151,664 | +180,224 | +143,360 |
| bdb | ff02d4e5 static callbacks | +14,128 | +15,792 | +16,384 | +16,384 |
| bdb | f3a599c7 typed memo | -624 | -512 | +0 | +0 |
| bdb | 9c5c326d concrete BackendError | -6,416 | -9,488 | -8,192 | -12,288 |
| bdb | 3b03bc5a scan tests | +0 | +0 | +0 | +0 |
| bdb | pr shared bodies + retained dyn layout hook | -133,616 | -114,832 | -143,360 | -122,880 |
| bdb | **final PR vs main** | +54,384 | +42,624 | +45,056 | +24,576 |
| kc | 87c71a6d store/data transactions and visitors | +114,160 | +94,176 | +106,496 | +86,016 |
| kc | ff02d4e5 static callbacks | +36,416 | +36,416 | +36,864 | +40,960 |
| kc | f3a599c7 typed memo | -240 | -128 | +0 | +0 |
| kc | 9c5c326d concrete BackendError | -2,736 | -3,360 | -4,096 | -8,192 |
| kc | 3b03bc5a scan tests | +0 | +0 | +0 | +0 |
| kc | pr shared bodies + retained dyn layout hook | -93,120 | -84,640 | -102,400 | -90,112 |
| kc | **final PR vs main** | +54,480 | +42,464 | +36,864 | +28,672 |
| tkrzw | 87c71a6d store/data transactions and visitors | +122,352 | +106,208 | +118,784 | +98,304 |
| tkrzw | ff02d4e5 static callbacks | +38,304 | +40,416 | +40,960 | +45,056 |
| tkrzw | f3a599c7 typed memo | +512 | +624 | +0 | +0 |
| tkrzw | 9c5c326d concrete BackendError | -10,720 | -14,160 | -16,384 | -16,384 |
| tkrzw | 3b03bc5a scan tests | +0 | +0 | +0 | +0 |
| tkrzw | pr shared bodies + retained dyn layout hook | -99,680 | -85,920 | -102,400 | -90,112 |
| tkrzw | **final PR vs main** | +50,768 | +47,168 | +40,960 | +36,864 |

## Post-rebase shipping size campaign

Measured UTC 2026-10-19T04:24:49Z. Base `a36a23b054d9da33e0a60a54ebe67af95e32296b`, PR `519ec6ed3f6f6004c09f8f8236f449ca36ca3db8`, pin `074a2219c90feaf962d0d24f034514033ece5f99`. All figures below were produced in this single Linux-host campaign. Raw evidence was neither saved nor uploaded, per the explicit task instruction.

`.text` is the actual ELF section from `size -A`; GNU size text is also given to disambiguate earlier reports that used its aggregate text column. Installed libraries are stripped with `strip --strip-unneeded`; the cargo fixture is measured as built.

| Cell | Revision | Artifact | `.text` B | GNU size text B | File B |
|---|---|---|---:|---:|---:|
| bdb | base (`a36a23b0`) | libpinyin | 1631045 | 2247087 | 2523888 |
| bdb | base (`a36a23b0`) | libzhuyin | 1483813 | 2065212 | 2339568 |
| bdb | base (`a36a23b0`) | libpinyin_capi | 1722869 | 2441012 | 3224672 |
| bdb | 87c71a6d (`87c71a6d`) | libpinyin | 1811957 | 2426971 | 2704112 |
| bdb | 87c71a6d (`87c71a6d`) | libzhuyin | 1635477 | 2213640 | 2482928 |
| bdb | ff02d4e5 (`ff02d4e5`) | libpinyin | 1826085 | 2442519 | 2720496 |
| bdb | ff02d4e5 (`ff02d4e5`) | libzhuyin | 1651269 | 2230856 | 2499312 |
| bdb | f3a599c7 (`f3a599c7`) | libpinyin | 1825461 | 2441491 | 2720496 |
| bdb | f3a599c7 (`f3a599c7`) | libzhuyin | 1650757 | 2229996 | 2499312 |
| bdb | 9c5c326d (`9c5c326d`) | libpinyin | 1819045 | 2432375 | 2712304 |
| bdb | 9c5c326d (`9c5c326d`) | libzhuyin | 1641269 | 2217884 | 2487024 |
| bdb | 3b03bc5a (`3b03bc5a`) | libpinyin | 1819045 | 2432375 | 2712304 |
| bdb | 3b03bc5a (`3b03bc5a`) | libzhuyin | 1641269 | 2217884 | 2487024 |
| bdb | pr (`519ec6ed`) | libpinyin | 1685429 | 2288447 | 2568944 |
| bdb | pr (`519ec6ed`) | libzhuyin | 1526437 | 2094236 | 2364144 |
| bdb | pr (`519ec6ed`) | libpinyin_capi | 1770533 | 2477732 | 3239408 |
| bdb | pin (`074a2219`) | libpinyin | 458888 | 750530 | 846600 |
| bdb | pin (`074a2219`) | libzhuyin | 441765 | 724825 | 826096 |
| kc | base (`a36a23b0`) | libpinyin | 1651125 | 2269049 | 2552552 |
| kc | base (`a36a23b0`) | libzhuyin | 1505061 | 2088202 | 2360040 |
| kc | base (`a36a23b0`) | libpinyin_capi | 1737173 | 2457316 | 3250936 |
| kc | 87c71a6d (`87c71a6d`) | libpinyin | 1765285 | 2380289 | 2659048 |
| kc | 87c71a6d (`87c71a6d`) | libzhuyin | 1599237 | 2178958 | 2446056 |
| kc | ff02d4e5 (`ff02d4e5`) | libpinyin | 1801701 | 2418893 | 2695912 |
| kc | ff02d4e5 (`ff02d4e5`) | libzhuyin | 1635653 | 2217374 | 2487016 |
| kc | f3a599c7 (`f3a599c7`) | libpinyin | 1801461 | 2418481 | 2695912 |
| kc | f3a599c7 (`f3a599c7`) | libzhuyin | 1635525 | 2217130 | 2487016 |
| kc | 9c5c326d (`9c5c326d`) | libpinyin | 1798725 | 2413977 | 2691816 |
| kc | 9c5c326d (`9c5c326d`) | libzhuyin | 1632165 | 2211874 | 2478824 |
| kc | 3b03bc5a (`3b03bc5a`) | libpinyin | 1798725 | 2413977 | 2691816 |
| kc | 3b03bc5a (`3b03bc5a`) | libzhuyin | 1632165 | 2211874 | 2478824 |
| kc | pr (`519ec6ed`) | libpinyin | 1705605 | 2313113 | 2589416 |
| kc | pr (`519ec6ed`) | libzhuyin | 1547525 | 2120230 | 2388712 |
| kc | pr (`519ec6ed`) | libpinyin_capi | 1793349 | 2503604 | 3274040 |
| kc | pin (`074a2219`) | libpinyin | 728392 | 1106025 | 1209592 |
| kc | pin (`074a2219`) | libzhuyin | 711269 | 1080284 | 1189080 |
| tkrzw | base (`a36a23b0`) | libpinyin | 1687253 | 2312544 | 2589416 |
| tkrzw | base (`a36a23b0`) | libzhuyin | 1529957 | 2119381 | 2388712 |
| tkrzw | base (`a36a23b0`) | libpinyin_capi | 1776117 | 2501311 | 3297920 |
| tkrzw | 87c71a6d (`87c71a6d`) | libpinyin | 1809605 | 2431092 | 2708200 |
| tkrzw | 87c71a6d (`87c71a6d`) | libzhuyin | 1636165 | 2220821 | 2487016 |
| tkrzw | ff02d4e5 (`ff02d4e5`) | libpinyin | 1847909 | 2471784 | 2749160 |
| tkrzw | ff02d4e5 (`ff02d4e5`) | libzhuyin | 1676581 | 2263925 | 2532072 |
| tkrzw | f3a599c7 (`f3a599c7`) | libpinyin | 1848421 | 2471964 | 2749160 |
| tkrzw | f3a599c7 (`f3a599c7`) | libzhuyin | 1677205 | 2264273 | 2532072 |
| tkrzw | 9c5c326d (`9c5c326d`) | libpinyin | 1837701 | 2456824 | 2732776 |
| tkrzw | 9c5c326d (`9c5c326d`) | libzhuyin | 1663045 | 2245745 | 2515688 |
| tkrzw | 3b03bc5a (`3b03bc5a`) | libpinyin | 1837701 | 2456824 | 2732776 |
| tkrzw | 3b03bc5a (`3b03bc5a`) | libzhuyin | 1663045 | 2245745 | 2515688 |
| tkrzw | pr (`519ec6ed`) | libpinyin | 1738021 | 2351572 | 2630376 |
| tkrzw | pr (`519ec6ed`) | libzhuyin | 1577125 | 2155329 | 2425576 |
| tkrzw | pr (`519ec6ed`) | libpinyin_capi | 1828565 | 2545755 | 3333072 |
| tkrzw | pin (`074a2219`) | libpinyin | 471496 | 768124 | 867216 |
| tkrzw | pin (`074a2219`) | libzhuyin | 454373 | 742415 | 838520 |

### Artifact identity

| Cell | Revision | Artifact | SHA-256 | NEEDED |
|---|---|---|---|---|
| bdb | base | libpinyin | `28992e2806dcf8ef8dd64b10632f637a947c89a76cb23e80c17fad80a53f06fe` | libglib-2.0.so.0, libdb-5.3.so, libgcc_s.so.1, libc.so.6, ld-linux-x86-64.so.2 |
| bdb | base | libzhuyin | `6b339232e69dfbb266ff8f7bcc73a37579b4b0256c19ea57485501d66d861e28` | libglib-2.0.so.0, libdb-5.3.so, libgcc_s.so.1, libc.so.6, ld-linux-x86-64.so.2 |
| bdb | base | libpinyin_capi | `d55014304730ad38c1d1639be6a57e20efc451b26032bf3e6ca5d6123620e1b0` | libdb-5.3.so, libglib-2.0.so.0, libgcc_s.so.1, libc.so.6, ld-linux-x86-64.so.2 |
| bdb | pr | libpinyin | `d70a0c9de6587f4d4a91832a0d2318e4e9f20cf5a24d8dbd9011ab67ed82fd19` | libglib-2.0.so.0, libdb-5.3.so, libgcc_s.so.1, libc.so.6, ld-linux-x86-64.so.2 |
| bdb | pr | libzhuyin | `2b4110d63e2fcc8044f8c5f50fda05a5c0a05eb490e0c1b288b7555b0e309d84` | libglib-2.0.so.0, libdb-5.3.so, libgcc_s.so.1, libc.so.6, ld-linux-x86-64.so.2 |
| bdb | pr | libpinyin_capi | `044fcf5a5313c5ecda05093b58bfe290af20c6ee81cf3b0bccec7a3555aaa91a` | libdb-5.3.so, libglib-2.0.so.0, libgcc_s.so.1, libc.so.6, ld-linux-x86-64.so.2 |
| bdb | pin | libpinyin | `24d602265bb26a1eb5588f02e9a11275bff72f33b9d2df5a4c98df5ce60a3288` | libglib-2.0.so.0, libdb-5.3.so, libstdc++.so.6, libm.so.6, libc.so.6, libgcc_s.so.1 |
| bdb | pin | libzhuyin | `e35427d0859a797ef61e634bbc268797ae46a7643dc4e37935ea5f115a1c86a1` | libglib-2.0.so.0, libdb-5.3.so, libstdc++.so.6, libm.so.6, libc.so.6, libgcc_s.so.1 |
| kc | base | libpinyin | `0d67862b7b889ced182212a6aa75eac9dfe123fc72de904a29f53cff3b0425cf` | libglib-2.0.so.0, libkyotocabinet.so.16, libgcc_s.so.1, libc.so.6, ld-linux-x86-64.so.2 |
| kc | base | libzhuyin | `e39ca1072bf9a0260d1a6ffd252f725523ca36827e00ab88258cb5544de524e5` | libglib-2.0.so.0, libkyotocabinet.so.16, libgcc_s.so.1, libc.so.6, ld-linux-x86-64.so.2 |
| kc | base | libpinyin_capi | `560ba34b5c24c1adf72107f35caff9aa67be6ecedb855de26ed23c21d9a9b268` | libkyotocabinet.so.16, libglib-2.0.so.0, libgcc_s.so.1, libc.so.6, ld-linux-x86-64.so.2 |
| kc | pr | libpinyin | `edbfba36280a15863a40ba3ef878981664398f6458dd8d9d31170715802db320` | libglib-2.0.so.0, libkyotocabinet.so.16, libgcc_s.so.1, libc.so.6, ld-linux-x86-64.so.2 |
| kc | pr | libzhuyin | `fe8ee163edc42e02c96d48e69d85c94ef4ccde5995b05f2d718ca086f1a420b0` | libglib-2.0.so.0, libkyotocabinet.so.16, libgcc_s.so.1, libc.so.6, ld-linux-x86-64.so.2 |
| kc | pr | libpinyin_capi | `bcd45ce2df06bc311ba2190d6c006b2d851cf4a30414ecb353f6d73b153a6ffe` | libkyotocabinet.so.16, libglib-2.0.so.0, libgcc_s.so.1, libc.so.6, ld-linux-x86-64.so.2 |
| kc | pin | libpinyin | `a4b6c58084bc7a2f722c2364d60a67d83273d4f2e0140457d1d3d789e0040415` | libglib-2.0.so.0, libkyotocabinet.so.16, libstdc++.so.6, libm.so.6, libc.so.6, libgcc_s.so.1 |
| kc | pin | libzhuyin | `db4dc70b009249d4c95858333be47c2d27e983f7e8485aa926d71d980bf5c25a` | libglib-2.0.so.0, libkyotocabinet.so.16, libstdc++.so.6, libm.so.6, libc.so.6, libgcc_s.so.1 |
| tkrzw | base | libpinyin | `3ad70b26a6c37df8160ea2aaf90b6748fc089dbac911013645d1eb41254abe9e` | libglib-2.0.so.0, libtkrzw.so.1, libc.so.6, ld-linux-x86-64.so.2, libgcc_s.so.1 |
| tkrzw | base | libzhuyin | `63b2f5083071a8fafc894a349ba3e9edcd530fcd23fb642abbda1a638c125b10` | libglib-2.0.so.0, libtkrzw.so.1, libc.so.6, ld-linux-x86-64.so.2, libgcc_s.so.1 |
| tkrzw | base | libpinyin_capi | `f03d6f7cc83ad3f26e81ee1969756ba4ec619df0f2d5176adfaa04526a8a42ec` | libtkrzw.so.1, libc.so.6, ld-linux-x86-64.so.2, libglib-2.0.so.0, libgcc_s.so.1 |
| tkrzw | pr | libpinyin | `fafee2d805196411b3ad9e56431bd1f6936a9c5f8560d7e551041ebaa9a0495f` | libglib-2.0.so.0, libtkrzw.so.1, libc.so.6, ld-linux-x86-64.so.2, libgcc_s.so.1 |
| tkrzw | pr | libzhuyin | `6ed2e81b111776af2b17a0a5e18391242e72a15c89418772262910de64fd85e1` | libglib-2.0.so.0, libtkrzw.so.1, libc.so.6, ld-linux-x86-64.so.2, libgcc_s.so.1 |
| tkrzw | pr | libpinyin_capi | `40cf3826651322da98fcf34c6e78f0a0e7b638342e2a4b443b1eecbcc2cf9cfd` | libtkrzw.so.1, libc.so.6, ld-linux-x86-64.so.2, libglib-2.0.so.0, libgcc_s.so.1 |
| tkrzw | pin | libpinyin | `29df58c38ecfa4d0a325bc2355fede949467ac2634e6dba498dd76850fc541f2` | libglib-2.0.so.0, libtkrzw.so.1, libstdc++.so.6, libm.so.6, libc.so.6, libgcc_s.so.1 |
| tkrzw | pin | libzhuyin | `d88b6748a57cc55f8aaacb39cb1790b4949728dd866d8465a6765b9941ba4f59` | libglib-2.0.so.0, libtkrzw.so.1, libstdc++.so.6, libm.so.6, libc.so.6, libgcc_s.so.1 |

### Size commands

Run from the task-owned `base` (current main) or `measure` worktree at the revision named in the table, once per backend (`bdb`, `kyotocabinet`, `tkrzw`). `CARGO_BUILD_JOBS=1`, `CARGO_INCREMENTAL=0`, `CARGO_NET_OFFLINE=true`, and an on-disk task-owned `TMPDIR` were set. Base and PR have separate Cargo targets per backend. `LD_LIBRARY_PATH=/home/sheng/.cache/oxpinyin/tkrzw/lib`, `PKG_CONFIG_PATH=/home/sheng/.cache/oxpinyin/tkrzw/lib/pkgconfig`; tkrzw is sound 1.0.34.

```sh
CARGO_TARGET_DIR="$TASK/release-targets/$TREE/$CELL" tools/packaging/install.sh libpinyin --prefix="$TASK/stages/$REV/$CELL" -- --locked --release --jobs 1 --no-default-features --features "$FEATURE,shipped"
CARGO_TARGET_DIR="$TASK/release-targets/$TREE/$CELL" tools/packaging/install.sh libzhuyin --prefix="$TASK/stages/$REV/$CELL" -- --locked --release --jobs 1 --no-default-features --features "$FEATURE"
strip --strip-unneeded "$TASK/stages/$REV/$CELL/lib/libpinyin.so.15.0.0" "$TASK/stages/$REV/$CELL/lib/libzhuyin.so.15.0.0"
CARGO_TARGET_DIR="$TASK/release-targets/$TREE/$CELL" cargo build --locked --release -p oxpinyin-capi --no-default-features --features "$FEATURE,shipped"
size -A "$SO"; size "$SO"; stat -c %s "$SO"; sha256sum "$SO"; readelf -d "$SO"
```

Intermediate commit attribution uses the same libpinyin and libzhuyin cinstall/relink/strip commands as the endpoints; only the checked-out commit changes. The final code commit and the inventory-only head have identical shipping source. No boundary was trimmed or reverted for these measurements. The pin originals match `tools/oracle/oracle-pin.txt`; stripped copies are used for file-size comparability and profiling, without changing `.text`.


Installed metadata gate: PASS on bdb, kc and tkrzw. Both libraries were staged through the supported install wrapper under DESTDIR with the exact cell-oracle prefix; copied pin headers/.pc inputs and all pkg-config queries matched, including relocation. The existing check-pc-metadata.sh assertions ran unchanged, with query captures routed to anonymous memory.

## Post-rebase whole-process cost campaign

Measured UTC 2026-10-19T05:02:20Z; valgrind-3.27.1. Intel Core i7-9750H, CPU 0, Ubuntu 26.10, Linux 7.3.0-9-generic; rustc 1.97.1, cargo-c 0.10.24, release fat LTO/codegen-units 1. Artifacts and SHAs are those in the shipping-size table. Campaign identity: `b1590fc87868bb4b21b8362c96c4e0378fe2712bde0309bff05cc6aec99fbb71`. Profiled the stripped, versioned, section-GC relinked libpinyin libraries, plus the stripped pin copy.

All profiles ran serially after builds and correctness work stopped. Equal-length base/pr__/pin_ artifact paths were hardlinks to the measured stripped libraries (identical bytes and SHA-256). Base/PR/pin were interleaved within each repetition and profiler; three repetitions each. Desktop processes were active on this host, so no uncontended wall-latency claim is made. These are instrumented instruction/heap measurements, not latency results; repetition spreads are reported. Only runs passing the registered stdout hash and suspend check contribute values; invalid combinations, if any, are listed below and never counted as passes. Massif heap excludes allocator overhead; the additional total column includes Massif-reported heap overhead. Stacks and mappings are excluded by the stated configuration. Profiler output was parsed in memory and not saved or uploaded.

| Cell | Workload | Revision | Median Ir | Ir spread | Median peak heap B | Heap spread | Median Massif total B | Total spread |
|---|---|---|---:|---:|---:|---:|---:|---:|
| bdb | guess | base | 2,428,702,517 | 0.000603% | 2,122,286 | 0.000000% | 2,131,576 | 0.000000% |
| bdb | guess | pr | 2,412,416,080 | 0.000250% | 2,122,270 | 0.000000% | 2,131,520 | 0.000000% |
| bdb | guess | pin | 2,420,587,004 | 0.000000% | 2,149,461 | 0.000000% | 2,159,640 | 0.000000% |
| bdb | train | base | 369,497,617 | 0.000000% | 1,249,020 | 0.000000% | 1,258,024 | 0.000000% |
| bdb | train | pr | 366,656,858 | 0.000000% | 1,249,052 | 0.000000% | 1,257,976 | 0.000000% |
| bdb | train | pin | 484,239,621 | 0.000000% | 7,126,914 | 0.000000% | 7,137,328 | 0.000000% |
| bdb | import | base | 634,233,566 | 0.000000% | 7,411,586 | 0.000000% | 9,166,912 | 0.000000% |
| bdb | import | pr | 634,766,640 | 0.000000% | 7,411,490 | 0.000000% | 9,166,704 | 0.000000% |
| bdb | import | pin | 757,332,615 | 0.000000% | 9,357,095 | 0.000000% | 9,364,328 | 0.000000% |
| kc | guess | base | 2,841,796,086 | 0.000141% | 12,707,648 | 0.000000% | 14,595,112 | 0.000000% |
| kc | guess | pr | 2,827,600,638 | 0.000192% | 12,707,680 | 0.000000% | 14,593,576 | 0.003399% |
| kc | guess | pin | 2,974,191,676 | 0.000000% | 13,725,282 | 0.000000% | 16,085,832 | 0.000000% |
| kc | train | base | 603,505,730 | 0.000000% | 9,612,052 | 0.000000% | 10,697,832 | 0.000000% |
| kc | train | pr | 601,114,715 | 0.000000% | 9,612,084 | 0.000000% | 10,698,104 | 0.000000% |
| kc | train | pin | 747,455,533 | 0.000000% | 15,049,465 | 0.000000% | 16,138,264 | 0.000000% |
| kc | import | base | 716,088,969 | 0.000000% | 19,571,755 | 0.000000% | 24,197,592 | 0.000000% |
| kc | import | pr | 716,679,267 | 0.000000% | 19,571,659 | 0.000000% | 24,194,920 | 0.000000% |
| kc | import | pin | 848,413,371 | 0.000000% | 21,400,235 | 0.000000% | 24,273,128 | 0.000000% |
| tkrzw | guess | base | 2,634,477,754 | 0.000283% | 16,022,819 | 0.000000% | 18,045,384 | 0.000000% |
| tkrzw | guess | pr | 2,621,142,052 | 0.000295% | 16,022,851 | 0.000000% | 18,046,520 | 0.000000% |
| tkrzw | guess | pin | 2,963,061,468 | 0.000000% | 17,350,010 | 0.000000% | 19,852,840 | 0.000000% |
| tkrzw | train | base | 578,734,007 | 0.000000% | 13,143,509 | 0.000000% | 14,382,856 | 0.000000% |
| tkrzw | train | pr | 576,646,886 | 0.000000% | 13,143,541 | 0.000000% | 14,382,632 | 0.000000% |
| tkrzw | train | pin | 804,638,437 | 0.000000% | 18,766,424 | 0.000000% | 20,015,384 | 0.000000% |
| tkrzw | import | base | 779,465,465 | 0.000000% | 22,498,053 | 0.000000% | 27,206,336 | 0.000000% |
| tkrzw | import | pr | 780,274,936 | 0.000000% | 22,497,957 | 0.000000% | 27,206,192 | 0.000000% |
| tkrzw | import | pin | 895,891,068 | 0.000000% | 24,327,435 | 0.000000% | 27,296,984 | 0.000000% |

Invalid/blocked combinations: []


### Cost commands and input identity

Build the unchanged current-tree driver with `cc -std=gnu11 -Wall -Wextra -Werror -O2 tools/bisection/system-import-profile.c -o "$HARNESS" -ldl`. Driver source SHA-256 `0407542de135659fbac3b7150236d1d21c2c8c9d6daaa3a7015f8be6dd5c184b`; executable SHA-256 `8a491beb74d7b890d9de4ce19fc18f7b39755d4a3b863e8166b4956cbc5ba7e9`. Each invocation used the same cell-specific oracle data directory and a new empty user directory. The sentence fixture SHA-256 is `d119f3a7f665333ef5717b1925171799c29e710d4ad1575748e0a9d3ae2c9e21`. The 10,000-row phrase input is generated from BDB pin `--export-system` exactly as in docs/runbooks/benches.md; 121,316 bytes, SHA-256 `22637aee560c86761bb6d9d187d9af8b9fea970a6690bc21e21e5b6fc1cbb117`; it was removed after the campaign.

```sh
# Repeat three times, interleaving SO=base,pr,pin in each profiler/workload/cell.
# GUESS: MODE=--sentence-fixture INPUT=fixtures/w4/oracle-sentence-surface.txt
# TRAIN: MODE=--train-fixture INPUT=fixtures/w4/oracle-sentence-surface.txt
# IMPORT: MODE=--phrase-file INPUT="$PHRASE_FILE"
taskset -c 0 /usr/bin/time -v valgrind --tool=callgrind --cache-sim=no --branch-sim=no --callgrind-out-file=/dev/stderr "$HARNESS" "$SO" "$DATA" "$FRESH_USER" "$MODE" "$INPUT"
taskset -c 0 /usr/bin/time -v valgrind --tool=massif --time-unit=i --heap=yes --stacks=no --peak-inaccuracy=0.0 --detailed-freq=1 --max-snapshots=1000 --massif-out-file=/dev/stderr "$HARNESS" "$SO" "$DATA" "$FRESH_USER" "$MODE" "$INPUT"
```

Whole-process Ir is `summary:` from Callgrind; heap is max `mem_heap_B` over Massif snapshots; Massif total is max `(mem_heap_B + mem_heap_extra_B + mem_stacks_B)` over the same snapshots. Each table entry is the median of three runs; spread is `(max-min)/median`. Expected stdout SHA-256: guesses `50dfc9c87e8eb2ce9121ac4f138584ab226a69308a9becc727cfca1c68c3100d`, training `e5855249fc5c7cc2c4de81aa1e792c2f41152a84c8a3e10f04c52a824bda698d`, imports `34e98cd8bf5a191b27e0b8fc50ffdefca9e76a2dab7dd01106cf6b6d3c9eec1d`.


## Current measured trade and maintainer ruling

All 162 profiler invocations passed the registered stdout hash and suspend checks; three valid repetitions for each main/PR/pin, backend, workload and tool. No profiler combination was invalid or blocked. Profiler output was consumed in memory only; the generated phrase input was removed. All size, code/test and metadata phases had ended before profiling. The exclusive heavy slot was coordinated with other tasks. Normal desktop/light audit activity existed; no uncontended wall-latency result is claimed. Outer GNU time peak RSS was only a profiler resource check, not an application-memory comparison. The host was released when the final profiler ended.

Final shipping deltas versus current main:

| Cell | Facade | ELF `.text` delta B | `.text` change | Stripped file delta B | File change |
|---|---|---:|---:|---:|---:|
| bdb | libpinyin | +54,384 | +3.334% | +45,056 | +1.785% |
| bdb | libzhuyin | +42,624 | +2.873% | +24,576 | +1.050% |
| kc | libpinyin | +54,480 | +3.300% | +36,864 | +1.444% |
| kc | libzhuyin | +42,464 | +2.821% | +28,672 | +1.215% |
| tkrzw | libpinyin | +50,768 | +3.009% | +40,960 | +1.582% |
| tkrzw | libzhuyin | +47,168 | +3.083% | +36,864 | +1.543% |

Whole-process PR/main median changes (full base/PR/pin absolute figures and spreads are above):

| Cell | Workload | Ir change | Peak requested heap delta B | Massif total peak delta B |
|---|---|---:|---:|---:|
| bdb | guess | -0.6706% | -16 | -56 |
| bdb | train | -0.7688% | +32 | -48 |
| bdb | import | +0.0841% | -96 | -208 |
| kc | guess | -0.4995% | +32 | -1,536 |
| kc | train | -0.3962% | +32 | +272 |
| kc | import | +0.0824% | -96 | -2,672 |
| tkrzw | guess | -0.5062% | +32 | +1,136 |
| tkrzw | train | -0.3606% | +32 | -224 |
| tkrzw | import | +0.1038% | -96 | -144 |

### Remaining reduction choices — estimates, not applied

The measured sequential commit attribution above is exact for this campaign. The selective-cut estimates below use historical BDB ablations as proxies; they are not current ablations, not verified for every facade/backend, and not additive because LTO and erasure boundaries interact. No new trimming variant was built. Each selected cut would need a new three-cell gate and measurements before a merge decision.

| Choice | Estimated saving and basis | Cost |
|---|---|---|
| Return only store scan visitors to `&mut dyn FnMut`, retaining ControlFlow, concrete transactions and shared steppers | Rough current ELF-text estimate 10–20 KiB in a pinyin build; historical BDB raw GNU-text saving 14.9 KB is the proxy, not a current measured saving | One indirect call per visited row; weakens the all-static goal. Historical Ir impact was under 0.005%, but current performance must be measured |
| Share more of the user-store transaction/persistence call bodies, or introduce a narrow erased transaction adapter on cold persistence paths | Estimate 0–30 KiB; historical residual transaction GNU text was about 26 KB. Existing cursor, merge, commit and point-operation outlining is already included, so its measured savings cannot be counted again | Additional call/erasure seams and maintenance; possible lost inlining. Preserves the new stop/error semantics; savings overlap visitors and are uncertain |
| Restore dyn only for GramOps and visibility predicates | Rough ELF-text estimate 2–6 KiB, anchored in the historical 3.8 KB combined GNU-text ablation | Indirect operations on masking/filter paths and less static specialization; current counter/heap effect unmeasured |
| Restore the baseline hook and reading iterator boundaries | Estimate 0–1 KiB, based on the historical approximately ±1 KB result | Runtime optional-hook checks and iterator virtual calls; small expected return |
| Restore ConfigSource erasure at construction | Estimate 0–2 KiB, unmeasured; large init already shares pre-read Settings | Additional cold virtual reads; small/possibly zero saving |
| Decline the code change in #879 | Recovers the complete measured main/PR delta in the table (42,464–54,480 B shipping `.text`, 24,576–45,056 B file per facade) relative to measured main | Forgoes the typed contracts, removal of sentinel/downcast control flow and measured guess/training instruction reductions. #872's interface-policy fix is independent |

The cold layout-fault hook is already dyn; its historical 16.5 KB avoidance is not a remaining saving. Concrete BackendError reduced `.text` in all three measured sequences, so reversing it is not supported as a size cut. Typed memo changes are small and mixed (BDB/KC decreases, tkrzw increases of 512/624 B) with zero stripped file-size delta at that member; reversing it loses the unrepresentable foreign-memo invariant for little demonstrated benefit.

### Justification submitted for the maintainer's ruling

The argument is explicit internal contracts: associated transaction/memo types, direct error data and ControlFlow instead of sentinel errors/downcasts, with all in-tree callers migrated and the supported C boundaries gated. The existing shared-body commit minimizes the initial code growth, and the cold layout-fault callback remains erased. Current measurements show approximately 0.50–0.67% fewer instructions for guesses and 0.36–0.77% fewer for training, alongside approximately 0.08–0.10% more for imports; peak requested heap changes are −96 to +32 B. These are limited workload counter results, not proof of a wall-clock speedup.

The cost is the disclosed 2.82–3.33% shipping ELF-text increase and 1.05–1.79% stripped file increase per facade. Current AGENTS.md Constitution §2 governs binary-size disclosure separately from runtime time/space complexity; the old pin +10% cap is retired. No runtime asymptotic complexity change is intended or justified here. I would submit the measured instruction reduction and stronger internal invariants for consideration against that size cost, while explicitly acknowledging the lack of a demonstrated latency gain and the import instruction increase. This report does not declare the regression accepted.

**STOP: maintainer ruling pending. No further trimming, merge or auto-merge is authorized by these measurements.**
