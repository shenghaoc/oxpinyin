# Compatibility policy

Status: **policy**. A change to this document needs the maintainer's approval under the [contributor procedures](../../CONTRIBUTING.md#approval-conditions).

This document decides what oxpinyin may do differently from the pin, and what it may not.
The pin is libpinyin 2.11.92 at `074a2219`; [`tools/oracle/oracle-pin.txt`](../../tools/oracle/oracle-pin.txt) is authoritative.
The [source policy](../source-policy.md) decides how contributors work against the pin: reading its source, the method for each subsystem and the stages.
The [divergence register](upstream-divergences.md) lists each accepted difference and each outstanding upstream defect.

The [classification table](#classification-table) keeps the current disposition of every registered entry.

## Goal: a bug-for-bug drop-in

oxpinyin is a drop-in replacement.
Rename the built shared object to `libpinyin.so.15`, put it on the library path, and existing consumers work with the data already on the system.
The consumer is not ported and does not know about the change: the binary interface, the file formats and the observable behaviour are the same.
The drop-in claim applies on Linux.
On Windows and macOS the C ABI, product behaviour and file formats are the same, and differences that come from native platform facilities are platform-dependent ([platform scope](#platform-scope)).

The default build is a bug-for-bug libpinyin binary.
A Stage 3 departure is opt-in and off by default ([stage rules](../source-policy.md#stages)).

### Backends and user data

libpinyin is built on one of three backends, Berkeley DB, Kyoto Cabinet or tkrzw, selected by `--with-dbm`.
oxpinyin is built on one of the same three, selected by a cargo feature.
It handles that backend's system and user files natively, exactly as a libpinyin build on that backend does.

- There is no compatibility mode, no import or migration feature, and no reading of libpinyin's files as foreign data.
- A user directory carries between oxpinyin and libpinyin builds on the same backend, in both directions, because both use that backend's native files.
- A build on a different backend starts fresh, as libpinyin's own builds do: its tkrzw build cannot read its Berkeley DB user files. A fresh start there is not a divergence, and a user directory in another backend's format is outside the compared state.
- libpinyin's import and export calls are ordinary ABI calls, reproduced like every other call. They carry the user phrase dictionary but not the trained bigram, because the ABI has no bigram import.

The reference build is the pin's autoconf build.
A bare `./configure` selects Berkeley DB (`configure.ac:94` at `074a2219`).
oxpinyin's default backend and the differential reference cell are tkrzw, because tkrzw is maintained upstream and Berkeley DB and Kyoto Cabinet are not.
Every crate that selects a store backend has `default = ["tkrzw"]`, and `tools/oracle/build.py` defaults to `--dbm tkrzw`.
Berkeley DB and Kyoto Cabinet are non-default backends with full drop-in compatibility.
Parity coverage includes all three cells.

A change of the default backend changes the backend family of a from-source build that takes the default: `user.conf`'s `database format` line changes, and a profile written by the earlier default is non-conforming and wiped on open, as libpinyin does across its own backend changes.
Released packages select their backend explicitly and are not affected.

The drop-in version follows the pin: 2.11.92, from `libpinyin_tag` in `tools/oracle/oracle-pin.txt`.

### The default is the pin

oxpinyin reproduces the pin.
A difference is not a design choice where the Rust is nicer.
It is permitted only when one of these applies:

- class (a) or (b) ([exception classes](#exception-classes));
- the [internal-language-artefact rule](#internal-language-artefacts);
- the [external API misuse ruling](#external-api-misuse);
- the [algorithm-precondition ruling](#algorithm-precondition-violations);
- a [standing maintainer ruling](#standing-maintainer-rulings).

Every other difference is a defect to revert.

## Scope

The scope is every exported symbol: all 79 `pinyin_*` symbols in `libpinyin.ver` and all 52 `zhuyin_*` symbols in `libzhuyin.ver`, the same sets the pin exports.
No symbol is in or out of scope because of which consumer calls it.
[`abi-reference.md`](abi-reference.md) is the reference for the exported set and the signatures.

A consumer's call sites are evidence that a behaviour is reachable in practice, for example ibus-libpinyin 1.16.5's default sort word (`PYPConfig.cc:151`).
They never define, count or limit the work.
Dead code in a consumer is not a call site.
"No consumer calls it" can set a priority; it is never an exception.

The `errno` that a successful call leaves behind is outside the compatibility goal, unless the API gives it a meaning.
This is a scope choice, not an exception class: it widens no class and registers no row.
Row 68 records the observation it rests on.

A `user.bin` that the pin did not write through its API, with duplicate key runs, is out of scope except that oxpinyin never panics on it.
oxpinyin loads, reads, exports and saves such a file without a panic; which duplicate entry survives is not specified.

## External interfaces: C only

The [constitution](../constitution.md#external-interfaces) states the full rule.
The only supported external library interfaces are the libpinyin-compatible and libzhuyin-compatible C APIs and ABIs.
Every native Rust interface is an implementation detail; it can change incompatibly when the same change updates its in-tree callers.
A Rust-interface constraint is never a reason to diverge and never an exception class.
Where a register entry calls a Rust signature change an "engine interface change" or an "approved interface change", the change needs no maintainer approval under the [approval conditions](../../CONTRIBUTING.md#approval-conditions).
No document is frozen; the [approval conditions](../../CONTRIBUTING.md#approval-conditions) state when a SPEC is corrected.
Approved compatibility decisions bind, and captured oracle observations remain verification evidence under their pin rules.

## Decision order

Every proposed difference is decided in this order.
The first limb that matches decides.

1. **Contract.** The C ABI and its layout, ownership, exported symbols, on-disk formats and data ownership never change.
   Product behaviour on valid input is the default.
   A change to it that is not a language artefact is a defect to revert, a class (a) or (b) case (limb 2), or a product-level change (limb 4).
2. **Class (a) or (b).** A difference that is maths or memory safety is classified in that class and meets the class's obligations.
3. **External I/O and internal language artefacts.** On Linux, external I/O uses the pin's call ([external I/O](#external-io)).
   Outside external I/O, a language artefact is not reproduced ([internal language artefacts](#internal-language-artefacts)).
4. **Product-level change.** A ranking rule, a limit, a refusal, the meaning of an option, or a logic defect of libpinyin that idiomatic Rust reproduces as written is product behaviour.
   It is reproduced bug for bug.
   A change to it needs a maintainer ruling or a Stage 3 feature.
5. **Anything else.** A defect to revert.

**Preserved exactly.** The C ABI and its layout; ownership and lifetime, including real `GArray` and `g_free` conventions and every other GLib-owned structure or allocation; every exported symbol; the system and user file formats; data ownership; and observable product behaviour on valid input.
No exception changes these, except that valid-input behaviour can differ through an internal language artefact.

**Prior rulings stand.** No limb reverses an earlier maintainer ruling.
[`LC_NUMERIC` after init](#lc_numeric-after-init-row-39) stays bug-for-bug.
A product-level change or a conflict with a prior ruling needs the maintainer's approval under the [approval conditions](../../CONTRIBUTING.md#approval-conditions); it is not a judgement call.

## Exception classes

There are two exception classes, (a) and (b), and no others.
The internal-language-artefact rule, the external API misuse ruling, the algorithm-precondition ruling and the standing maintainer rulings are not classes.

No row may be classified (c) or (d).
Where the pin asserts or aborts, oxpinyin aborts ([failure behaviour](#failure-behaviour)), and every exported symbol and every option bit is in scope ([scope](#scope)).

### (a) Maths: platform-dependent floating-point accumulation

The pin's result is a pure function of `gfloat` values accumulated through a transcendental function such as `log`, and no fixed-point or integer form reproduces it.
Reproducing it would need the platform libm, which has no cross-platform bit-exactness guarantee.

The class is narrow.
Basic IEEE-754 operations (add, subtract, multiply, divide, compare) are bit-reproducible across platforms and are not covered.
`amplified_frequency` ((1−λ)·unigram/total·2²⁴) uses basic operations only and is ported exactly.
A float in the call graph does not make an entry class (a); a transcendental function in the accumulation does.
Example: the n-best trellis (`phonetic_lookup.h:663,692`) adds `log(...)` into a `gfloat` at each step.

**Maths rule.** oxpinyin uses Rust's standard maths at full speed: `f64::ln`, `exp`, `powf` and every other `std` float method.
It never slows code to imitate glibc, libm, GLib `gfloat` or other C behaviour, and it never calls the reference implementation's libm for bit-exact parity.
A last-bit difference from the pin, or between hosts, that follows from Rust's maths is class (a): it is recorded and not chased.
Output stays deterministic on each host (constitution §6).
No specialised maths dependencies, such as SIMD crates, are added.

### (b) Memory safety: the pin's behaviour is undefined

The pin's observable behaviour is undefined behaviour, such as a use-after-free or an out-of-bounds read, and the Rust construction that replaces it cannot express the defect.
There is no safe Rust that reproduces it.

The class covers only cases where reproduction is structurally impossible.
Where the pin's undefined behaviour produces a stable, reproducible value that a safe construction can also produce, the case is not class (b).
Example: the bigram export iterator joins a pronunciation vector that it never NULL-terminates (`pinyin.cpp:844-850`, `:857-863`), a heap over-read (row 1).

A registered class (b) case that returns its prescribed result is `MATCH`.

## Failure behaviour

oxpinyin follows the pin row by row.
Where the pin aborts, oxpinyin aborts.
Where the pin answers, oxpinyin gives the same answer.
Where oxpinyin's outcome differs, the implementation changes.
No row is reclassified to avoid a change.
Class (b) prescribed results and the other outcomes that this document approves stay.
A row that fits neither case needs the maintainer's approval under the [approval conditions](../../CONTRIBUTING.md#approval-conditions).
For calls that the pin makes, the documented contract of the call governs, not one implementation of it.

**Reachable pin aborts.**

- A pin `assert` (including `check_result`, `include/pinyin_utils.h:27-31`) or `abort()` is reproduced with `std::process::abort()`: abnormal termination, no return, no destructors, SIGABRT on Unix.
- The reference build passes no `-DNDEBUG` (autoconf's default `-g -O2`), so every `assert` at the pin is an abort site.
- Before it aborts, oxpinyin does the pin's preceding observable work in the pin's order: files written, removed or renamed, and stderr bytes.
- oxpinyin calls `std::process::abort()` at the corresponding failure point. This may occur in any crate when all affected call paths require termination. Shared code may instead return a typed error when the caller must determine the abort point.
- `abort()` does not unwind or run destructors. Under the current unwind panic strategy, a Rust panic can run `Drop` implementations before termination and is not a substitute for the pin's abort. An unhandled panic in a Rust tool ordinarily exits with status 101; a panic attempting to cross a non-unwinding C ABI boundary aborts. `libc::abort()` is not used in a crate that forbids `unsafe`.

**Unreachable internal invariants.** A panic-producing assertion is permitted only where failure is proven unreachable under the applicable preconditions.
No abort or panic is introduced where the pin continues or returns an error.
Error handling is idiomatic Rust; internal Rust APIs do not all need to return `Result`.
Non-test crate roots deny `clippy::unwrap_used`, `clippy::expect_used`, `clippy::panic`, `clippy::panic_in_result_fn` and `clippy::unreachable`.
A permitted site uses the narrowest scoped `#[expect(..., reason = "...")]` and cites the established invariant that makes failure unreachable.
An `#[expect]` proves only that Clippy saw the construct; review establishes the invariant.
The [Rust conventions](../rust-conventions.md#panics-and-errors) give the lint details.

**The `assert` line.** The pin's `assert` writes glibc's `file:line: func: Assertion '…' failed.` before SIGABRT.
That text is compiler-generated and is not rebuilt.
At every `assert` site the missing line is `POLICY-ACCEPTED` and the SIGABRT status is `MATCH`.
An `abort()` site writes nothing on either side.

**Returned strings.** Where the pin allocates a string that the caller frees with `g_free` or `g_strfreev`, through `g_strdup`, `g_strdup_printf`, `g_strconcat`, `g_strjoinv` or `g_malloc`, oxpinyin calls that function with the pin's arguments, so an allocation failure aborts as the pin's does.
Where the pin converts with `g_ucs4_to_utf8`, a pure conversion, oxpinyin keeps its encoder and follows that call's contract: the bytes, or NULL on invalid input or allocation failure, with no warning and no abort.
Rust's internal `Vec` and `String` out-of-memory abort stays; its message differs from GLib's and is an internal language artefact.

Class (b) is unchanged by this section, including its sites that write a diagnostic.

## Native Rust implementation

oxpinyin is a native Rust implementation of libpinyin and libzhuyin, not an emulator of the pin.
It reproduces the pin's outward behaviour, at the C API and in persistent data, in idiomatic Rust.

<a id="internal-language-artefacts"></a>

### Internal language artefacts

Outside external I/O, the programming language is an implementation detail.
The behaviour is what a user, or a developer who calls the shared object, observes.
oxpinyin implements that behaviour in natural, efficient Rust.
Inefficient code is never written to imitate internal GLib, libc or C++ behaviour.

No code is written to reproduce a behaviour of another programming language or of a library written for one.
That includes C or C++ semantics (an integer-returning comparator, unsigned wrap-around, float-to-integer truncation, an integer width chosen for storage), GLib, glibc or another libc, libstdc++, and compiler or processor behaviour.

**The test.** Would a language-neutral requirement for the behaviour mention it?
If not, it is a language artefact and is not reproduced.
If so, it is product behaviour, produced with idiomatic Rust (a byte scan, a raw comparison, `Vec<u8>` instead of `String`), never by emulating a GLib, libc or C++ mechanism.
A width or wrap that the C ABI or an on-disk format prescribes, or that is part of a domain algorithm such as a checksum or hash, is contract or product behaviour.
An operation's origin in C does not make every use of it an artefact.

Where idiomatic Rust gives the pin's result, which is the normal case, nothing more is needed.
Where the result differs only through a language artefact, on valid or malformed input:

1. Keep the Rust result.
2. Add one entry to the [divergence register](upstream-divergences.md), in the [entry form](upstream-divergences.md#purpose-and-entry-form) stated at its top.
3. Mark the cases that tell the two apart `POLICY-ACCEPTED`.

No approval is needed.
Writing Rust to imitate the artefact is forbidden.
Where it is not settled whether a difference is an artefact, idiomatic Rust is preferred and the difference is recorded.
The entry cites the pin and names the tests; one entry does not admit another by analogy.

The rule is not a licence to change product behaviour (decision-order limb 4).
"No frontend sees it" and "it is only a warning" are not reasons: diagnostics are part of the C contract.

### Host-dependent results

Where an external call's result depends on the host or its libc, oxpinyin calls the same native facility on that host; it does not substitute the reference image's libc.
Internal host-dependent results follow the language-artefact rule and are recorded as host-dependent.
Platform-independent logic must not differ, and output stays deterministic on each host (constitution §6).

**GLib versions** (maintainer ruling).
The tests can meet a behaviour difference between GLib versions.
When that difference is traced to GLib's own code, it is accepted.
It is not an oxpinyin defect, and it needs no new ruling.

### Crate placement

- External file, locale and tool I/O calls belong in `oxpinyin-input`.
- DBM backend calls belong in `oxpinyin-store`.
- C ABI objects, ownership conventions, marshalling and boundary encodings belong in the C adapters: `oxpinyin-capi`, `oxpinyin-zhuyin-capi` and `oxpinyin-capi-marshal`.
- Core and engine code implement domain semantics in native Rust, even where those semantics are observable through the C interfaces.

Core and engine types carry only what the pin's pre-C operations need:

- the stored length read by `_compute_phrase_length`;
- the outward code points read by candidate comparison and deduplication, sentence concatenation, export and the token getters;
- the full stored words read by store and table maintenance.

General Rust conversions preserve all characters.
NUL-containing text becomes Stored, with the full declared character count and a first-zero outward cut.
Scalar is private, NUL-free text with no cached length or metadata bytes.
`phrase_text_bytes` is the explicit C-boundary encoder.

<a id="external-io"></a>

## External I/O and platform scope

### External I/O uses the pin's call

On Linux, every call across the program's edge, in the library and the tools and in both directions, is the pin's call:

- ISO C and POSIX calls go through the host C library, using the `libc` crate or `extern "C"` declarations.
- GLib calls go through the workspace's `glib-sys`.
- A function that neither crate binds is declared by hand.
- C++ streams are Rust `std::io` under the C++ contract; for example, `std::getline` is `BufRead::read_until(b'\n')`.

A Rust equivalent is allowed only when the source of both implementations proves identical observable results for every input.
If equivalence is not settled, use the pin's call.
The NOT EQUIVALENT and UNSETTLED verdicts in the external I/O boundary's [input](external-io-boundary.md#input-inventory) and [output](external-io-boundary.md#output-inventory) inventories stand.
The [external I/O boundary](external-io-boundary.md) lists the boundary stages.

**Macro checks.** Macros such as `assert`, `g_return_val_if_fail` and `G_STRFUNC` are not reproduced.
Their checks are plain Rust, which answers the pin's value or aborts where the pin aborts ([failure behaviour](#failure-behaviour)).
Their message text is not imitated; the stderr difference is a `POLICY-ACCEPTED` language artefact.
A macro that does work rather than check, such as `g_utf8_next_char`, is implemented from its documented contract ([g_utf8_next_char from a byte that leads no sequence](upstream-divergences.md#g_utf8_next_char-from-a-byte-that-leads-no-sequence)).
Where no call or behaviour has the same documented result, the case is reported and the maintainer decides it.

**What the call rule does not require.** It does not reproduce an undefined read or an abort in the pin's surrounding code.
Existing class (b) entries stand, including [Unterminated punctuation value over-read](upstream-divergences.md#unterminated-punctuation-value-over-read). The language-artefact entry [Punctuation values contain whole words](upstream-divergences.md#punctuation-values-contain-whole-words) also stands.
[`LC_NUMERIC` after init](#lc_numeric-after-init-row-39) stands.
Row 109's whole-file mapping call follows the pin, and its release follows the [external API misuse ruling](#external-api-misuse).
The `errno` scope choice ([scope](#scope)) does not excuse a different external I/O call.

**Drop-in tools.** On Linux, the tools are drop-in replacements for the pin's utilities, with the same binary names, options, help, messages, inputs and outputs.
K-mixture working models use the pin's backend DBM through `oxpinyin-store`.
The `scripts2/` Python build generators are outside this scope.

<a id="platform-scope"></a>

### Platform scope

The drop-in and pin-call rules bind Linux only.
libpinyin is developed and used on Linux.
On Windows and macOS, oxpinyin builds and is tested, and may use portable Rust abstractions, such as `memmap2` and `std::fs`, instead of the pin's calls.
A difference that comes from a platform's native facilities (file and mapping APIs, text mode, C runtime streams) is platform-dependent behaviour, recorded where known and tested where practical.

These must be the same on every platform: platform-independent product behaviour (ranking, parsing, training, data handling), safety, determinism, the C API and ABI with its symbol names and its ownership and lifetime rules, and the persistent file formats.
The [Rust/C++ interoperability ruling](#rust-cpp-interoperability) applies on every platform.

**Windows adaptation.** The Linux pin is the behavioural reference on every operating system.
Where the pin's Windows build differs because it does not handle a Windows platform caveat, such as code pages, narrow file-name APIs or missing `mmap`, oxpinyin produces the Linux pin's observable result with the native Windows facility.
This covers platform adaptation only; the pin's own logic, the C ABI and the persistent file formats do not change.

<a id="fundamental-os-differences"></a>

**Fundamental OS differences.** Where the operating system itself does not allow the pin's Linux behaviour, oxpinyin does not imitate it.
Example: on Windows, a file that is mapped, or open without delete sharing, cannot be deleted or replaced.
A test that depends on such behaviour is compiled only for the operating systems that provide it (`#[cfg]`), with a comment that names the facility.
It is not ignored or skipped, and no failure is injected in its place.

**Operating systems and CPU architectures** (maintainer ruling).
Behaviour differences between operating systems and CPU architectures are stable.
A test selects the expected result for each operating system and architecture with `#[cfg(target_os = …)]` or `#[cfg(target_arch = …)]`.
It does not skip or ignore the case.
[Fundamental OS differences](#fundamental-os-differences) covers a test that depends on a facility that an operating system does not provide.

<a id="rust-cpp-interoperability"></a>

## Rust/C++ interoperability

**Principle.** Agents write the code and one person reviews it.
A boundary that the compiler checks does not depend on who wrote it; a boundary that a comment describes does.
The checked boundary is preferred.
This ruling rests on safety, maintainability and review load, not on performance.

**Rule.**

- A library with official or well-established Rust bindings is used through them, for example the `libc` crate and `glib-sys`.
- Otherwise, a C library binds with bindgen over its official C API.
- Otherwise, a C++ library binds with CXX over the concrete classes the pin uses.
- Hand-written C++ is permitted only as a semantic adapter for a native construct that CXX cannot express: a virtual callback subclass (`Visitor`, `RecordProcessor`), a pointer sentinel, or a status-versus-exception split. The adapter calls the library; it does not copy or emulate it.

CXX checks signatures, `Pin`, `UniquePtr` ownership and `unsafe` on raw pointer parameters.
It does not check the items below.
Each PR that touches one of them names the item, states the contract and gives the proof from the library source:

1. the safety claim of each function declared safe inside `unsafe extern "C++"`;
2. `Send` and `Sync` for a native object;
3. a cursor's or iterator's lifetime dependence on its database (`PhantomData<&'db Db>`; CXX does not infer it);
4. the validity and length of each pointer passed to or from a callback;
5. synchronise, close and destruction order, for each concrete class;
6. every pointer value that a library treats as a sentinel.

**Backends.**

| Backend | Binding |
| --- | --- |
| Berkeley DB | bindgen over the C API |
| tkrzw | CXX over the concrete classes the pin constructs: `TreeDBM`, `HashDBM`, `BabyDBM`, `TinyDBM` |
| Kyoto Cabinet | CXX over its concrete classes (`crates/oxpinyin-store/src/kyotocabinet/bridge.rs`), with the semantic adapter `adapter.h`/`adapter.cc` |

The Kyoto Cabinet binding preserves the pin's native semantics.
A CXX function declared without `Result` terminates the process on a C++ exception, as the pin does; the bridge declares none with `Result`.

<a id="flexiblebigram-failed-attach"></a>

**`FlexibleBigram` failed attach.** The ordinary tables keep their object after a failed open.
`FlexibleBigram` differs by backend at the pin:

- tkrzw deletes the object when an open fails, for the first open without CREATE and for the CREATE retry ([flexible_ngram_tkrzwdb.h:128-158](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/flexible_ngram_tkrzwdb.h#L128-L158)).
- Kyoto Cabinet keeps the object when the signature check fails and when the CREATE retry fails, and deletes it only when the open without CREATE fails ([flexible_ngram_kyotodb.h:129-180](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/flexible_ngram_kyotodb.h#L129-L180)).
- Berkeley DB never deletes the handle: it keeps it after every failed open and after a failed signature check ([flexible_ngram_bdb.h:94-153](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/flexible_ngram_bdb.h#L94-L153)), and `reset()` synchronises and closes it ([flexible_ngram_bdb.h:51-57](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/flexible_ngram_bdb.h#L51-L57)). That is the pin's misuse of its handle, not a rule to keep or reuse a failed handle: oxpinyin closes each failed handle ([row 113](#bdb-failed-handles)).

**Exports.** The public symbol set and symbol versions do not depend on the binding.

**Performance.** The [source policy's controlling rule](../source-policy.md#controlling-rule) applies.
The 0.95–1.05 control band qualifies a comparison; it is not an acceptable regression threshold.
A regression found by source reading is reviewed against the compatibility gain; a percentage does not accept it.

## External API misuse

Where the pin calls an external library or system API incorrectly, by passing an argument that contradicts the pin's own state or the API's documented use, oxpinyin uses the API correctly and does not reproduce the consequence of the misuse.
Each case is added to the [divergence register](upstream-divergences.md) with its pin citation, and its affected cases are marked `POLICY-ACCEPTED`.
This covers external calls only.
A defect in the pin's own logic is still reproduced bug for bug unless a separate ruling says otherwise.

**Mapping lifetime (row 109).** `MemoryChunk::mmap` maps `file_size` bytes but does not keep that length.
`freemem` (`src/include/memory_chunk.h:72-78` at `074a2219`) passes `munmap` a length recomputed from the chunk, `header + capacity()`.
That length is short of the mapping when `guint32 data_len = file_size - header` wraps, so the rest of the mapping is never released.
oxpinyin uses the `memmap2` crate, whose `Drop` releases exactly what was mapped.
For every file whose size passes the pin's length check without wrapping, this is the pin's own `munmap(base, file_size)`.
`ManuallyDrop`, a hand-written `libc::munmap` and any other means of leaving part of a mapping behind are not used.

**Berkeley DB failed handles (row 113).** oxpinyin closes each Berkeley DB handle whose open failed; see [`FlexibleBigram` failed attach](#flexiblebigram-failed-attach) and [Berkeley DB handles whose open failed (policy row 113)](upstream-divergences.md#berkeley-db-handles-whose-open-failed-policy-row-113).

## Algorithm-precondition violations

This ruling sits under the product-level limb of the decision order; it is not an exception class.
Its trigger is a reachable violation of a documented precondition of a library outside the libpinyin repository that the pin calls (the C and C++ standard libraries, GLib, Berkeley DB, Kyoto Cabinet, tkrzw), found during normal work.
It is not a mandate to search for violations.

The PR proposing a new repair must include both of these before approval:

- a mathematical demonstration of the violated precondition;
- evidence from accepted pin input that places those values in a single algorithm input range.

An arbitrary counterexample, or values collected from different ranges, does not register a site.
The absence of both comparator argument orders in an actual call trace does not refute a witness evaluated with the pin's comparator on that range.

Data structures and algorithms implemented inside the libpinyin repository are not a trigger; they are followed as written, however flawed.
The repair follows the intent that the source shows.
A semantic choice that the source does not settle needs an explicit maintainer ruling.
Each new site must meet both evidence requirements on its own; similarity to a registered site grants no exception.
Contract-valid but questionable or incorrect algorithms stay bug-for-bug.
Core-language undefined behaviour stays under classes (a) and (b); this rule is not a general undefined-behaviour exception.
The C ABI and layout, ownership and lifetime, exported symbols, system and user file formats and data ownership never change under this rule.

### Registered sites

The register entries are [Pinyin trellis adjusted cost — S1/S2 repair](upstream-divergences.md#pinyin-trellis-adjusted-cost--s1s2-repair) and [Pinyin tail sort — S3 repair](upstream-divergences.md#pinyin-tail-sort--s3-repair).
These are the only registered sites:

| Site | Pin source at `074a2219` | Contract violated |
| --- | --- | --- |
| S1 beam selection | `src/lookup/phonetic_lookup.h:770–771`, `get_top_results<2>(32, ...)` | C++ `[alg.sorting.general]` strict weak ordering for the standard heap algorithms |
| S2 final-tail selection | `src/lookup/phonetic_lookup.h:331–335`, `get_top_results<2>(3, ...)` | The same strict weak ordering requirement |
| S3 tail sort | `src/lookup/phonetic_lookup.h:337`, `g_ptr_array_sort` | GLib's qsort-style comparator contract; C11 §7.22.5 paragraph 4, consistent total ordering |

### S1 and S2: one ordered adjusted score

The enabled `#if 1` branch and its "allow longer sentence" comment (`src/lookup/phonetic_lookup.h:65–90`) establish the margin intent.
`LONG_SENTENCE_PENALTY` is `log(1.2f)` (`src/lookup/phonetic_lookup.h:39`).
For adjacent lengths, the longer sentence wins if its possibility advantage exceeds that margin.
For an additive score `poss − f(L)`, equivalence at the threshold requires `f(L+1) − f(L) = ln(1.2)`, so, up to a common constant:

`S = poss − L · ln(1.2)`; a higher S ranks first.

The approved float form is `S = poss - (L as f32) * P`, with `P = 1.2_f32.ln()`.
The product and the subtraction each round in f32, with no widening and no `mul_add`.
A last-bit difference of `P` on another host is class (a).

Order scores as +∞, finite values descending, −∞, then NaN.
Signed zeros are equal and all NaNs tie.
This special-cost ordering applies to every reachable special value, including negative-infinity scores at λ = 1, where `unigram_lambda = 1 − λ = 0` makes the unigram step `log(0)`.

Selection uses the total key `(ordered S, batch_position)`, with the earlier batch position first on equal S.
`trellis_node<2>::eval_item` uses the same `trellis_value_less_than<2>` for `push_heap` and `pop_heap` through `trellis_value_comp<2>` (`src/lookup/phonetic_lookup_heap.h:25–28,65–76`), so the repair applies to those heap calls by construction.
The node's two-slot admission comparison uses ordered S alone and rejects a tying newcomer; a selection batch position must not leak into that rule.

This repair deliberately changes pairs two or more characters apart: a longer sentence wins when the probability ratio exceeds `1.2^d` for a length gap d, instead of the pin's unconditional preference for the shorter sentence.
The approved score and tie rules settle that choice; they do not authorise another ranking redesign.
Intentional differences under this ruling are reported as `POLICY-ACCEPTED`, not `MATCH`; [Pinyin trellis adjusted cost — S1/S2 repair](upstream-divergences.md#pinyin-trellis-adjusted-cost--s1s2-repair) records them.

### S3: stable descending possibility

`trellis_value_compare` says "in descending order" and compares only `m_poss` (`src/lookup/phonetic_lookup.h:174–178`).
The repair is a stable sort by `m_poss` descending, with no integer truncation.
Its criterion is `m_poss`, not the adjusted S.
Equal possibilities keep their input order.
The repaired comparator uses the special-cost ordering above, with NaN last.

### Retained defect and open question

`trellis_node<2>::eval_item` claims "min heap" and "find minium item" but uses an ordinary max heap and evicts its front (`src/lookup/phonetic_lookup_heap.h:25–36,56–81`).
For same-length finite scores the comparator is a valid order, yet the algorithm discards the best resident when a better newcomer arrives and keeps the weaker resident.
That is a contract-valid logic defect, not this rule's trigger.
It is preserved bug for bug; a fix needs a separate maintainer ruling or Stage 3.

Whether to reject λ outside `[0, 1]` is an open question; its remedy needs an explicit maintainer ruling.

### No ported external-library internals

oxpinyin implements libpinyin's own algorithms.
It does not transcribe the internal algorithms of GLib, libstdc++, libc++, glibc or another external library to preserve that implementation's comparison schedule, probe sequence or other detail.

- **What to use instead.** The Rust standard-library API with the corresponding documented semantics is used. Necessary representation adapters and libpinyin-specific product logic stay ordinary Rust code. A new dependency needs maintainer approval; this ruling authorises none.
- **Exact results within the preconditions.** Where the pin uses a library routine within its documented preconditions, the Rust replacement reproduces its result exactly, with the arguments (predicate, comparator, key, tie-break) chosen to do so. Only results that depend on a violated precondition may differ, and each such difference is recorded as `POLICY-ACCEPTED`.
- **What stays.** libpinyin's product rules, ranking keys, limits, C ABI, ownership and persistent formats stay, except for departures recorded under this ruling or another ruling.
- **Ordering requirements.** Sort and selection comparisons provide a consistent ordering, with explicit special-value and tie treatment where necessary. `partition_point` predicates are partitioned on valid data. Malformed stored data is not silently sorted or cleaned to force that condition; the resulting difference is recorded, and memory safety and the existing refusal rules stay.
- **Scope.** ABI-prescribed encodings are boundary code, not external-algorithm ports. This ruling does not authorise a search for unrelated upstream defects.

Specific rules:

- **Candidate frequency.** libpinyin's frequency comparator is reproduced with Rust's wrapping arithmetic. Stable `std` sorting uses `rhs.wrapping_sub(lhs) as i32` only after proving that every frequency tie group under the preceding key fields has a consistent order; antipodal pairs count as cycles. A cyclic group uses a defined, stable, numeric-descending fallback. Only lists on which the comparator is cyclic may differ. The four `row58-signed-{score,duplicate}-{off,on}` cases must `MATCH` and are not registered as differences. [Candidate sorting guards the wrapping frequency order](upstream-divergences.md#candidate-sorting-guards-the-wrapping-frequency-order) holds the proof and the tests.
- **Zhuyin selection** stays length-first. At equal length it uses descending raw score, with NaN last and signed zeros equal, then the earlier batch position. Its single-slot admission rule is unchanged ([Zhuyin selection uses length, raw score and input order](upstream-divergences.md#zhuyin-selection-uses-length-raw-score-and-input-order)).
- **Pinyin's two-slot node** keeps front eviction and physical-slot semantics.

The [external-library inventory](glib-libc-inventory.md) lists the executable copies of external-library algorithms that this ruling removes, and their replacements.

## Standing maintainer rulings

Each ruling below admits only the difference it names, and no other difference by analogy.
No registered standing divergence outside these rulings and the classes is live.

### Key positions past byte 65535

Key positions past byte 65535 reach the C getters wrapped mod 2^16, as `ChewingKeyRest`'s `guint16` fields hold them at the pin; this is contract (limb 1).
The pin's placement of keys at the wrapped begin, a 65536-column ring matrix, is waived.
See [Key positions past byte 65535](upstream-divergences.md#key-positions-past-byte-65535).

### `LC_NUMERIC` after init (row 39)

The pin leaves the process `LC_NUMERIC` at `"C"` after `pinyin_init` and `zhuyin_init`.
oxpinyin reproduces this bug for bug, and it is recorded as an upstream defect (item 6 in the [upstream report drafts](upstream-report-drafts.md)).
A reversal needs a new human ruling.

<a id="unloaded-token-pronunciation-cache"></a>

### Unloaded-token pronunciation cache (row 118)

For an unloaded token in training, oxpinyin skips the pronunciation step and writes into no item.
Bigram training, the constraint walk, the modified flag and the `true` result stay the pin's.
This is an approved difference, including the defined stale-cache write; it does not authorise a departure for any other missing-item path.

At the pin, after a failed `get_phrase_item`, `train_result3` applies the pronunciation helper to `m_cached_phrase_item` (`phonetic_lookup.h:924-929`).
The cache belongs to the context's one `PhoneticLookup`, which every instance of that context shares (`pinyin.cpp:39-75,406-409`; `zhuyin.cpp:34-62,348-351`; member `phonetic_lookup.h:523`).
Every successful trellis fetch replaces it, including candidates that the decoder later rejects (`phonetic_lookup.h:646,674`), so the written item depends on hidden fetch history.
If the cached item belongs to the library just unloaded, unload frees its bytes without clearing the cache (`phrase_index.cpp:261-268`), and the later read and possible write are a use-after-free (`phonetic_key_matrix.cpp:603-620`, `phrase_index.cpp:114-140`).

`unloaded-cached-pronunciation-pinyin` keeps the defined witness.
Library 1 holds 甲 and library 2 holds 乙, both pronounced ba1 with pronunciation count 10.
The case chooses 乙 and guesses its sentence, unloads library 2, then guesses ba1 on another instance of the same context.
Training the first instance makes the pin write 甲's pronunciation count from 10 to 79, saved in `gb_char.dbin`, while its unigram stays 100; oxpinyin leaves the count at 10.
`unloaded-cached-pronunciation-zhuyin` covers the shared implementation.
Both cases are `POLICY-ACCEPTED`, never `MATCH`.

<a id="sentence-start-bigram-caching"></a>

### Sentence-start system bigram reads

Read the system bigram through its attached DBM handle on every load, including `sentence_start`.
Do not keep a sentence-start value between loads.
This supersedes the permission to omit later native reads after the first successful load.

The pin loads the system and user grams before merging them (`src/lookup/phonetic_lookup.h:588-599`, `src/pinyin.cpp:2210-2215`, `src/zhuyin.cpp:1483-1487` at `074a2219`).
Each system load calls the selected DBM: BDB `get` (`src/storage/ngram_bdb.cpp:140`), Kyoto Cabinet `check` then `get` (`src/storage/ngram_kyotodb.cpp:131,138-139`), or tkrzw `Get` (`src/storage/ngram_tkrzwdb.cpp:111`).
Keep the same attachment until context destruction (`src/pinyin.cpp:394-396,1208`; `src/zhuyin.cpp:336-338,747`).
Follow each read's result; a previous successful read does not suppress a later miss or failure.

### Fresh `UserTableInfo` with zero versions

The pin's `test_table_info` loads `../../data/table.conf`, prints its tables, constructs a fresh `UserTableInfo`, then calls `is_conform` before `make_conform` (`tests/storage/test_table_info.cpp:56-90`).
The constructor initialises both versions and the open counter but leaves `m_table_database_format_type` uninitialised (`src/storage/table_info.cpp:313-317`; layout `table_info.h:97-102`).
When both system versions are zero, the comparisons at `:399-404` reach the first undefined read at `:406`.
The tkrzw constructor disassembly writes zero at offsets 0 and 12 and leaves offset 8 unwritten; `is_conform+0x18` reads that offset.
A nonzero version returns before that read.

This is class (b).
oxpinyin's fresh format is `None`, and `UserTableInfo::is_conform` answers `false`.
The driver then continues through make-conform, save to `/tmp/user.conf` and load (`test_table_info.cpp:89-96`; `table_info.cpp:325-419`).
Cases `table-info-zero-versions-pinyin` and `table-info-zero-versions-zhuyin` are `POLICY-ACCEPTED`, never `MATCH`.
The pin is not run for them: their expectation is the source-derived result, not a recorded pin observation.

### Class (b) tool cases with source-derived expectations

These utility cases are class (b).
The pin is not run for them; each expectation is oxpinyin's prescribed result, derived from the pin's source:

- `chewing-table-invalid-text`, `phrase-table-invalid-text` and `phrase-table-invalid-utf8` (row 115);
- `chewing-table-overlong-query` (row 116);
- `phrase-index-logger-missing` and `phrase-index-logger-short` (row 117).

The code comments in `crates/oxpinyin-utils/tests/storage_tools.rs` cite the pin sources for each case.

## The E2E I/O compatibility rule

> For every exported symbol, given the same inputs and state, oxpinyin returns byte-identical outputs to the pin, except where class (a) or (b), or a recorded internal language artefact or ruling in this document, explicitly applies.

Output is the whole observable surface, not only the scalar return: the return status, the out-parameters and the data they point to, written lengths, and every state transition on the handle.
State includes the on-disk user state of a same-backend user directory.

Exporting a symbol that returns a wrong value is worse than not exporting it: the consumer gets a silent wrong answer instead of a link error.
A stub that returns `false` is not compliance; it is a defect.
The version script decides which symbols exist; this rule decides what they do.

**Corollary.** If implementing a symbol correctly needs an engine change, that engine change is mandatory.
The engine serves the ABI.

**Verification.** Every exported symbol needs a differential probe that drives it with the same input on both libraries and checks the whole observable surface.
A case that this policy admits holds its declared disposition instead: `POLICY-ACCEPTED` for a registered difference, `HOST-DEPENDENT` where registered, or a class (b) declaration.
A symbol with no probe is unverified, not compliant; closing that gap is work.
The probe runs on all three backend cells, tkrzw, bdb and kc, against the pin built for the same cell.

**Evidence standard.** A finding (a divergence, a closure or a class) rests on code plus data: the citation of both sides' code at named revisions, and the input and output needed to see the difference.
Logs are not evidence.
A run log or a local working file may point to where the data came from; it never stands in for the data.

## Dispositions and register entries

The differential dispositions are:

- **`MATCH`**: identical observations on both sides for an ordinary case. A registered class (b) case is compared with its declared semantics, so a pin crash against oxpinyin's prescribed result is `MATCH`.
- **`DIFFER`**: a difference with no current register entry. It is a defect until it is classified.
- **`POLICY-ACCEPTED`**: a non-`MATCH` that a current register entry admits. The case keeps its pin and subject observations. The case is removed only when its ruling is withdrawn. It is never renamed `MATCH` or counted as an exact match. It passes when both sides behave as its entry specifies, and it is reported separately from exact matches.
- **`HOST-DEPENDENT`**: the disposition of the four host-libc `%f` cases (row 78). It does not excuse a scanner that differs from the pin's call ([external I/O](#external-io)); the owning [external-I/O stage](external-io-boundary.md) updates these cases.
- **`SKIP`**, **`INVALID`** and **`TIMEOUT`**: the case produced no comparable verdict.

When a disposition changes, the case keeps its observations and gains the register entry's heading.

**Register entries.** Every difference from the pin, under any class or ruling, is one entry in the [divergence register](upstream-divergences.md), in the [entry form](upstream-divergences.md#purpose-and-entry-form) stated at its top.
No difference has a record file of its own.
The entry gives the pin's behaviour with its cite, oxpinyin's behaviour, the decision and its reason, any rejected alternative, and the tests; the tests that hold oxpinyin's side name the entry.
A new entry carries a `POLICY-ACCEPTED` case.

**tkrzw HashDBM clock field.** A tkrzw HashDBM file that a reproduced SIGABRT leaves unclosed holds a clock timestamp in zero-based bytes 48-55, the `META_OFFSET_TIMESTAMP` field.
A comparison normalises those eight bytes only, for named cases only, and only when the observation ends in SIGABRT.
A later case on the same path names its files.
Every other byte and file is compared exactly, and equal observations stay `MATCH`; this is not a `POLICY-ACCEPTED` difference.
A saved user-bigram comparison may differ in the same eight bytes and nowhere else, and its bucket-chain order must match.
The native setter these test programs use (`tool_set`, `tools` feature) is tools-only; shipping `WriteStore::write` and its durability do not change.

## What is not an exception

These statements do not justify a difference:

- **"The Rust is cleaner."** Internal structure may differ (source policy); externally observable behaviour may not.
- **"The pin's behaviour is meaningless."** Meaningless behaviour is still product behaviour.
- **"No frontend does that."** At most a reason to schedule a port later, never to skip it ([scope](#scope)).
- **"A float is involved."** Not class (a) unless a transcendental function is in the accumulation. A difference that comes from Rust's standard maths is class (a): it is recorded and not chased.
- **"It is only GLib, glibc or C++."** The [decision order](#decision-order) decides: an artefact is not reproduced, and a product-level change needs a ruling (limb 4).
- **"It only affects malformed input."** Necessary, not sufficient: the path is traced end to end, the C contract is preserved, and the register entry names the case and its controls.
- **"It is only a warning."** Diagnostics are observable. A changed diagnostic needs its own register entry and a reason.

<a id="classification-table"></a>

## Classification table

The table classifies every entry that the divergence register holds or held.
Each row keeps its identifier; code, tests and register entries cite rows by number.
The identifiers run 1–4, 5a–5d, 6–98, 100–106, 106a and 107–120; row 99 does not exist.

Dispositions:

- **pin behaviour reproduced**: oxpinyin reproduces the pin, including an assert or abort ([failure behaviour](#failure-behaviour)).
- **CLOSED**: oxpinyin matches the pin, or is proven equivalent on the pinned data.
- **(a)**, **(b)**: an [exception class](#exception-classes).
- **no ABI divergence**: the entry describes no difference at the C ABI.
- **POLICY-ACCEPTED**: a difference that a current register entry or ruling admits; the kind follows in brackets.
- **IMPLEMENTED**: the ruled behaviour is in place; the row names any residual.
- **RETIRED**, **WITHDRAWN**, **reattributed to row N**: the entry no longer stands on its own.

A row with more than one arm states each arm and the condition that selects it.
The [divergence register](upstream-divergences.md) holds the details of each entry that is still accepted.

| # | Entry | Disposition | Basis |
| --- | --- | --- | --- |
| 1 | Bigram export iterator's pinyin buffer | **(b)** | The pin can crash on the first export cycle after a train, saved or not. `pinyin_bigram_iterator_has_next_phrase` builds each pronunciation string from a `GPtrArray` it never NULL-terminates (`g_ptr_array_new` → `g_ptr_array_free(…, FALSE)` → `g_strjoinv`/`g_strfreev`, `pinyin.cpp:844-850` and `:857-863`; the unigram export adds the terminator, `:726-734`). The join reads past the array's end: a heap over-read whose outcome depends on heap layout (GLib 2.90.0 `garray.c:1893-1921`, `:1845-1890`). On tkrzw and kc the pin crashes with the user dir saved and unsaved; on bdb it crashes unsaved. oxpinyin renders every row into owned strings when it creates the iterator (`crates/oxpinyin-capi/src/iterators.rs:576`), so no over-read exists. The contract cases `user-gram-unsorted-export` and `user-gram-duplicate-export` reach this join and carry `strjoinv_over_read`. A pin that dies of SIGSEGV with stderr identical to the subject's is held, and the subject must exit normally with a result; its rows are not compared. Any other pin signal, or different stderr, is INVALID. A pin that completes is compared exactly. |
| 2 | Public bigram export is a rendering surface | **no ABI divergence** | The C ABI reproduces the rendering. No in-tree code reads the raw store; the internal migration tool that would read it (`legacy-migration`) is cancelled. |
| 3 | HANYU full pinyin ignores tone digits under `USE_TONE` | **CLOSED** | oxpinyin ports the pin's behaviour; `PARSE_AUX_IDENTICAL`. |
| 4 | Tone digit on an initial-only key aborts the phrase search | **pin behaviour reproduced** | The pin SIGABRTs on `n4` under `USE_TONE\|PINYIN_INCOMPLETE`. Sites: `storage/pinyin_phrase3.h:152` **`assert`** (`CHEWING_ZERO_TONE == key.m_tone` in `contains_incomplete_pinyin`, `:146-156`) on every phrase search over the key, and `pinyin.cpp:2769` **`assert`** (the same test in `pinyin_get_pinyin_is_incomplete`). The search that reaches the pin's assert decides the abort; the zhuyin guesses reach it through the same engine searches. [Register entry](upstream-divergences.md#pin-aborts-reproduced). |
| 5a | Scheme setters — double `CUSTOMIZED` (30) | **pin behaviour reproduced** | The call aborts mid-call at `storage/pinyin_parser2.cpp:611` **`abort()`**, after the fallback clear at `:580`. [Register entry](upstream-divergences.md#pin-aborts-reproduced). |
| 5b | Scheme setters — double out-of-enum (negatives, 0, 7–29, 31+) | **CLOSED** | oxpinyin reproduces the pin's half-mutation: the CAPI returns `true` and clears the fallback, and the shengmu/yunmu tables stay intact. The contract test `contract.rs::double_out_of_enum_reproduces_the_half_mutation` holds the cleared fallback as observable. |
| 5c | Scheme setters — zhuyin `STANDARD_DVORAK` (7) | **pin behaviour reproduced** | The dvorak arm falls through to `default:` and aborts at `storage/zhuyin_parser2.cpp:295` **`abort()`** (arm at `:291`). [Register entry](upstream-divergences.md#pin-aborts-reproduced). |
| 5d | Scheme setters — zhuyin / full-pinyin out-of-enum | **pin behaviour reproduced** | Zhuyin aborts at `pinyin.cpp:1189` **`abort()`** (the API's `default:`) and its libzhuyin twin `zhuyin.cpp:736` **`abort()`**. Full pinyin aborts at `storage/pinyin_parser2.cpp:398` **`abort()`**. [Register entry](upstream-divergences.md#pin-aborts-reproduced). |
| 6 | Constraint-aware train without the consistency assert | **pin behaviour reproduced** | `train_result3` aborts on a stale result at `lookup/phonetic_lookup.h:868` **`assert`** (`token == constraint->m_token` at a `CONSTRAINT_ONESTEP` position). oxpinyin trains through `Runtime::train_session` → `Session::prepare_training` → `TrainingPlan::observe`, which checks every forced phrase against the decoded row before it observes anything and answers `EngineError::StaleTrainingConstraint`; `InstanceCore::train` leaves the context unmodified. Contract cases `abort-train-stale-forcing` and `abort-zhuyin-train-stale-forcing` show the pin dying of SIGABRT; `train-forcing-after-lookup` is the successful control. [Register entry](upstream-divergences.md#pin-aborts-reproduced). |
| 7 | `validate_constraint`'s drop test is the span-search shape | **CLOSED** (proven equivalent on the pinned data) | The pin drops a constraint when `compute_pronunciation_possibility(...) < FLT_EPSILON` (`phonetic_lookup.cpp:161-164`), a sum over every path. oxpinyin's `span_finds_token` takes the first path per token, so the arithmetic differs. The drop boundary is reachable only when a token's pronunciation total exceeds 2²³. The largest total in model20 is 2,945,481, so the two tests are observably equivalent on the pinned data. A model with a total above 2²³ reopens the row. |
| 8 | Constraints survive every re-parse except the selection-committed one | **CLOSED** | oxpinyin keeps constraints across a selection-committed re-parse, as the pin does. Frozen pins are bit-identical. |
| 9 | The n-best row-choose cursor is the row's own end | **CLOSED** | Every `NBEST_MATCH_CANDIDATE` choose answers `parsed_len`: upstream's `matrix.size()-1` in the active parse mode's coordinates. |
| 10 | `pinyin_get_sentence` asserts a past-the-rows index | **pin behaviour reproduced** | The pin SIGABRTs on a non-empty result set at `pinyin.cpp:1474` **`assert`** (`index < results.size()`; function `:1464-1482`). [Register entry](upstream-divergences.md#pin-aborts-reproduced). |
| 11 | Real-table sentence float accumulation | **(a)** for the demonstrated std-math precision differences | oxpinyin uses pre-log f32/f64 pieces, Rust std `ln` and f32 accumulation. `kaliantai` is an S1/S2/S3 selection residual, not math, and is not part of class (a). The current exact selection residuals are in [sentence-surface.md](sentence-surface.md) §12. |
| 12 | Predicted-candidate tie order | **CLOSED** | On KC and tkrzw the runtime walks the pin's own phrase DBM, so the predicted order is identical to the pin's on the pin's `data/`. |
| 13 | Mid-syllable candidate-lookup offset | **CLOSED** | oxpinyin reproduces the pin's empty-column law. |
| 14 | Cursor helpers' `_check_offset` aborts answer `false` | **pin behaviour reproduced** | At `074a2219` `_check_offset` returns `false` (`pinyin.cpp:2163-2182`). The aborts are its **`assert`**-wrapped calls: `pinyin.cpp:3035` and `:3057` (`get_left_pinyin_offset`), `:3067` and `:3092` (`get_right_pinyin_offset`). Example: `get_right_pinyin_offset(inst, 5, &r)` on `nihao` fires `:3092`. [Register entry](upstream-divergences.md#pin-aborts-reproduced). |
| 15 | Apostrophe-only input: pin consumes every byte, engine none | **CLOSED** | `SegmentGraph` propagates each apostrophe one byte, counted: `'''` consumes 3, as on the pin. A test in `graph.rs` holds this. |
| 16 | `FORCE_TONE` honoured on the full-pinyin seam only | **CLOSED** | oxpinyin ports every seam's `FORCE_TONE` law: the zhuyin batch, the double-pinyin batch, the one-key seams and the pinyin facade's chewing batch (row 30). |
| 17 | Literal `0x0` option gating (`jv`/`zon`; `xian` divided-table) | **CLOSED** | oxpinyin ports the pin's gating at the literal `0x0` word. Resplit and divided additions are gated on `USE_RESPLIT_TABLE`/`USE_DIVIDED_TABLE` (`crates/oxpinyin-engine/src/session/mod.rs`, `:626`; pin `storage/phonetic_key_matrix.cpp:88`, `:171`). An empty matrix answers an empty guess (pin `pinyin.cpp:2194`). |
| 18 | The pinyin index DBMs carry uninitialized struct padding | **(b)** | Upstream copies a stack struct's tail padding into the DBM. oxpinyin zeroes the padding, and the reader never reads it. |
| 19 | `pinyin_get_character_offset`'s recursion asserts answer `false` | **pin behaviour reproduced** for the asserts; **(b)** for the unbounded `cached_tokens` read | Sites, all **`assert`**: `pinyin.cpp:3147` (`size > 0`, the recursion's column), `:3161` (`1 == size`), `:3203` (`offset < matrix.size()`), `:3204` (`_check_offset`); libzhuyin twins `zhuyin.cpp:2110`, `:2158`. The unbounded `cached_tokens` read (`:3168`) is a (b) sub-shape; oxpinyin answers it as a deterministic miss. At index == length the read (`:3167-3168`) lands on the array's zero terminator (`g_array_new(TRUE, TRUE, …)`, `:3219`), token 0 finds no item and the pin answers the same miss, `false` with the reached length (`:3238`); the class (b) sub-shape is an index past the terminator. `_get_char_offset_recur` reads every item of a column (`:3138-3191`; `zhuyin.cpp:2117`), including fuzzy alternatives. In the index-parsed full pinyin, double pinyin and chewing modes oxpinyin reads the matrix kept with the parse. Source-derived at `074a2219`; not run against the pin. [Register entry](upstream-divergences.md#pin-aborts-reproduced). |
| 20 | One bigram-prediction row differs on the pin's own data | **reattributed to row 33**; **CLOSED** with row 33 | The extra `pred: type=4 text=你` line after the train on `测测` is row 33's mechanism, not a trellis residual. The pin's whole-row NBEST choose installs no `CONSTRAINT_ONESTEP` (`pinyin.cpp:2515-2520`), so `train_result3` writes nothing (`lookup/phonetic_lookup.h:866`) and the pin predicts nothing. Row 33's fix removes the line. |
| 21 | The single-key surface aborts the pin where oxpinyin answers `false` | **pin behaviour reproduced** for the asserts; **(b)** for the empty-input over-reads | Sites: `storage/pinyin_parser2.cpp:170` **`assert`** (apostrophe in `parse_one_key`); `pinyin.cpp:499` **`assert`** (`index < PHRASE_INDEX_LIBRARY_COUNT`, addon unload) and its sibling `:466` **`assert`** (phrase-library unload). The empty-input reads under `USE_TONE` (`pinyin_parser2.cpp:178`, `zhuyin_parser2.cpp:171`) are over-reads, neither `assert` nor `abort()`: the (b) shape. [Register entry](upstream-divergences.md#pin-aborts-reproduced). |
| 22 | Empty-string phrase lookup crashes the pin | **WITHDRAWN** | The crash does not reproduce at `074a2219`: `pinyin_lookup_tokens(instance, "", array)` answers `false` with an empty array, and the source shows no `assert`, `abort()` or division on the path (`pinyin.cpp:2652-2667`). oxpinyin answers `false` (`crates/oxpinyin-capi/src/dict.rs:86`). |
| 23 | Sanitizer scope on the tkrzw shim CI · native data-file naming · R1 measured on the compat paths | **no ABI divergence** | These are not behaviour entries: a CI-instrumentation note, a file-naming decision ([installed-naming.md](installed-naming.md)) and a measurement of removed code. |
| 24 | zhuyin batch `FORCE_TONE` law | **CLOSED** | `ZhuyinParser::parse_with_options` honours the three per-keyboard shapes. |
| 25 | zhuyin candidate-tag grouping + `after(consumed)` terminal offset | **CLOSED** | At an offset strictly inside a key the pin's matrix column is empty, so `zhuyin_guess_candidates_after_cursor` answers the prepended sentence rows alone (`zhuyin.cpp:1498-1512`, `:1624-1626`). oxpinyin snapshots only those rows there (`zhuyin_offset_is_mid_key`). |
| 26 | zhuyin before-cursor candidate window | **CLOSED** | The after-cursor mid-key window follows row 25. The before-cursor window searches the complete retained parse at the requested fixed end (`zhuyin.cpp:1562-1588`, `phonetic_key_matrix.cpp:416-423` at `074a2219`). A row whose span starts after the offset is constrained `[m_begin, m_end)` (`Candidate::span_start` = upstream's `m_begin`), and `zhuyin_choose_candidate` answers `m_begin` (`zhuyin.cpp:1660`). |
| 27 | zhuyin multi-syllable candidate construction | **CLOSED** | The divergence was the pinyin string-fill law, not the construction model. |
| 28 | zhuyin n-best trellis constants `<1, 1>` vs the engine's `<2, 3>` | **CLOSED** | A per-session `NbestShape` (`PINYIN` = `<2, 3>`, `ZHUYIN` = `<1, 1>`) is set by both zhuyin facades at instance allocation. The difference is not observable through the current libzhuyin candidate surface. |
| 29 | zhuyin `FORCE_TONE` / `ZHUYIN_INCOMPLETE` default | **no ABI divergence** | `CapiContext::try_open` seeds the pin's `USE_TONE \| FORCE_TONE`. The entry stays as analysis for a future consumer. |
| 30 | pinyin-facade chewing batch seam does not forward `FORCE_TONE` | **CLOSED** | The seam forwards `options().bits() & !ZHUYIN_CORRECT_ALL` through `parse_with_options`. capi tests hold the pin's shape: toneless `su` refuses under `USE_TONE \| FORCE_TONE`. |
| 31 | redb write-side emptiness probe creates the table it probes | **no ABI divergence** | A redb API constraint of a removed backend; the row keeps its identifier. |
| 32 | Sort-option input of `pinyin_guess_candidates` | **CLOSED** | With bit `0x1` (`SORT_WITHOUT_SENTENCE_CANDIDATE`, `pinyin.h:56`) set, the pin never prepends the sentence rows (`pinyin.cpp:2295-2296`), so its phrase-string dedup (`:2298-2300`) keeps the NORMAL rows that repeat a sentence. oxpinyin skips the n-best prepend there and ports the `0x1` choose leg (`:2565-2576`). Bit `0x2` clear builds the LONGER row (`Session::longer_candidate`, `_prepend_longer_candidates`, `pinyin.cpp:1870-1933`), and the three sort keys follow `compare_item_with_sort_option` (`:1678-1709`). |
| 33 | Whole-row NBEST choose + train writes the user bigram | **CLOSED** | The pin's row-0 `NBEST_MATCH_CANDIDATE` choose installs no `CONSTRAINT_ONESTEP` (`pinyin.cpp:2515-2520`), and `train_result3` trains only when `train_next` or a `CONSTRAINT_ONESTEP` is present (`phonetic_lookup.h:866`), so a constraint-free whole-row train writes nothing. oxpinyin makes a decoded constraint-free result train nothing and keeps `diff_result` training for nonzero rows, and trains pronunciation counts through the selected matrix span. Production training is `Runtime::train_session` → `Session::prepare_training` → `TrainingPlan::observe`: a row with spans trains only the spans with a OneStep cell, and a row with empty spans replays the selection history with no OneStep gate. |
| 34 | Imported user phrase lost after `guess_sentence` | **CLOSED** | The pin keeps `m_nbest_results` across a parse (`pinyin_parse_more_full_pinyins` never touches it, `pinyin.cpp:1497-1524`; only `pinyin_reset` clears it, `:2693-2704`), and it rebuilds candidates on each `guess_candidates` (`:2184-2300`). At `0x1f` the imported user NORMAL row remains. oxpinyin keeps the n-best rows across a parse through `Session::discard_composition`. |
| 35 | User-library tokens refused an n-best step cost | **CLOSED** | The pin prices any loaded sub-index's item, the user library included (`unigram_gen_next_step`, `phonetic_lookup.h:643-668`; `_add_phrase`, `pinyin.cpp:597-605`). `nbest_step_costs_with_user_delta` mirrors the pin's presence gate: a visible `USER_FILE` token with no system unigram is priced from its user delta (`crates/oxpinyin-data/src/lm/mod.rs`); a masked library's token or a missing item keeps the default. |
| 36 | Bigram export iterator: `pinyin_bigram_iterator_get_next_phrase`'s return value on the last row | **CLOSED** | The pin fills the out-params, advances and returns `pinyin_bigram_iterator_has_next_phrase(iter)` (`pinyin.cpp:896-911`): `false` on the last row. oxpinyin returns the same value, and it exports from the pin's in-memory user-bigram container with the pin's walk order, skipped last key and `sentence_start` attribution. The unigram export iterator returns `true` on every row on both sides (`pinyin.cpp:698-769`). |
| 37 | Candidate window behind the composition offset after a choose | **CLOSED** | The pin rebuilds the window from `start = offset` over the whole-composition matrix on every call (`pinyin.cpp:2184-2262`), and a choose behind the composition moves the record back (`:2578-2590`). oxpinyin does the same, maps a key boundary to the matrix column the key sits at, and does so for double pinyin, Luoma and secondary zhuyin, including the empty mid-key column. |
| 38 | `pinyin_train` requested n-best row and both facades' empty-result gate | **pin behaviour reproduced** for an index past the results; **CLOSED** for the valid-index and empty-result defects | `Session::prepare_training` reads the requested row's existing spans, and `TrainingPlan::observe` trains them. `Runtime::train_session` (`crates/oxpinyin-runtime/src/lib.rs`) answers `false` on empty decoded results, through `Session::prepare_training`, and marks user data modified unless its answer is `false` or a pin abort. Fresh instances, and parse → NORMAL choose → train without `guess_sentence`, return a quiet `false` and write no training data, zhuyin included (`crates/oxpinyin-zhuyin-capi/src/candidates.rs:272`). Valid pinyin indices 0/1/2 train distinct rows. The `train-index-diff` cases (pinyin `0`, `1`, `2`, `len`, `255`, `fresh`, `choose-only`; zhuyin `0`, `fresh`, `choose-only`) compare the return values, exported records and `.dbin` bytes (`src/pinyin.cpp:2678-2688`; `src/zhuyin.cpp:1704-1713`). Every successful zhuyin case chooses a candidate first. Zhuyin guess → train with no choose is source-derived from `src/zhuyin.cpp:1704-1713` at the pin. Contract case `abort-train-index-past-results`: the pin dies of SIGABRT at `pinyin.cpp:2684` **`assert`** (`index < results.size()`). [Register entry](upstream-divergences.md#pin-aborts-reproduced). |
| 39 | `pinyin_init`/`zhuyin_init` leave the process `LC_NUMERIC` at `"C"` | **CLOSED** (ruled bug-for-bug) | The pin's `UserTableInfo::load` and `SystemTableInfo2::load` save the return of `setlocale(LC_NUMERIC, "C")`, the new locale's name, and restore it on success (`storage/table_info.cpp:328,372` and `:197,291` at `074a2219`; `save` the same, `:378,394`). Every early return skips the restore (e.g. `:330-333`, `:339-348`, `:199-202`), so after any init `LC_NUMERIC` is `"C"`. oxpinyin installs `"C"` through the C library's `setlocale` at the pin's points, inside `oxpinyin-input`'s `table.conf`/`user.conf` reader and writer, under the [`LC_NUMERIC` ruling](#lc_numeric-after-init-row-39). |
| 40 | tkrzw binding: `SYSTEM_ERROR`/`UNKNOWN_ERROR` collapse | **RETIRED** | The collapse came from tkrzw's C API (`tkrzw_langc.cc:530-620`, 1.0.34). The backend calls the concrete classes through cxx (`crates/oxpinyin-store/src/tkrzw/bridge.rs`) and catches nothing, as the pin catches nothing; a concrete call returns its own `tkrzw::Status` (`tkrzw_lib_common.cc:247-278`; `tkrzw_dbm_hash.cc:400-401`), and `std::bad_alloc` from `xmalloc` (`tkrzw_lib_common.h:147-183`) terminates the process, the pin's own outcome. |
| 41 | NULL pointer arguments across 68 exports | **(b)** | At `074a2219` each of 68 `pinyin_*` exports dereferences a NULL argument before any check, and the pin receives SIGSEGV. Patterns: the instance first (`pinyin.cpp:1312`, `pinyin_alloc_instance`); the context or iterator first (`:509`, `:665`, `:777`; `:1196` in `pinyin_fini`); an out-parameter written before any check (`:2847` `*num = instance->m_candidates->len`, `:2876`, `:2982`); a NULL candidate inside a live `assert` (`:2507`, `:2593`); a NULL key or key-rest (`:2711`, `:2722`, `:2733`, `:2744`). oxpinyin opens every such export with an `is_null()` guard and answers `false`/`0`/NULL/void (e.g. `crates/oxpinyin-capi/src/instance.rs:19`, `iterators.rs:64`, `candidates.rs:309`, `cursor.rs:241`, `keys.rs:219`). The 13 NULL-class probes that do not crash the pin are outside this row; there oxpinyin follows the pin. The libzhuyin exports have the same shape. |
| 42 | A guess on an instance whose context was finalised | **(b)** | `pinyin_fini` deletes the context and its members (`pinyin.cpp:1194-1221`) but not the instances allocated from it. `pinyin_guess_sentence` on such an instance reads `instance->m_context->m_pinyin_lookup` (`:1373-1380`): a use after free, and the pin receives SIGSEGV (probe `instance_outlives_context`). oxpinyin's instance holds its own handles to the shared state (`crates/oxpinyin-capi/src/state.rs:69-77`), so the guess completes and answers `true`. The inverse probe `alloc_after_fini` is caller UB on both sides; row 60 covers it. |
| 43 | Zhuyin import into library index 16 | **(b)** | `zhuyin_begin_add_phrases` accepts any `guint8` (`zhuyin.cpp:392-398`). A following `zhuyin_iterator_add_phrase` reaches `phrase_index->get_range(index, …)` (`zhuyin.cpp:475`), which reads `m_sub_phrase_indices[16]` (`storage/phrase_index.cpp:611`) one past the 16-entry array (`storage/phrase_index.h:441`): an out-of-bounds read, SIGSEGV on the pin on all three cells. oxpinyin answers `false` for every library that is neither a `USER_FILE` library nor a loaded system library (`crates/oxpinyin-zhuyin-capi/src/iterators.rs:90`); system libraries 1–4 are accepted as on the pin. The pinyin facade's unchecked index (`pinyin.cpp:589`) is not a stable crash and is outside this row. Contract case `zhuyin-add-phrase-library-16` is `MATCH`, with the pin at SIGSEGV. Held by `boundary_tests::null_arguments_answer_the_declared_value`. |
| 44 | Unknown `database format:` in `user.conf` aborts the pin | **pin behaviour reproduced** | Site: `storage/table_info.cpp:132` **`abort()`**, the fall-through of `to_table_database_format_type` (`:122-133`), called at `:353-354` from `UserTableInfo::load` (`:351-352`). `check_format` calls it (`pinyin.cpp:172-178`, `zhuyin.cpp:126-132`), and `pinyin_init`/`zhuyin_init` ignore its result (`pinyin.cpp:344`, `zhuyin.cpp:288`). The source guarantees the abort when `fscanf` converts a token that is not `BerkeleyDB`, `KyotoCabinet` or `Tkrzw`. When the directive does not match (`fscanf` returns `0`), the pin reads an indeterminate buffer; oxpinyin returns `UserConfError::UnknownDatabaseFormat` for all these shapes and aborts. The `table.conf` path to the same `abort()` (`:232-233`) stays silent. Contract case: `user-conf-abort-database-format-unknown`. [Register entry](upstream-divergences.md#pin-aborts-reproduced). |
| 45 | `pinyin_choose_candidate` under `SORT_WITHOUT_SENTENCE_CANDIDATE` with a nonzero offset | **pin behaviour reproduced** | Site: `src/pinyin.cpp:2566` **`assert`** (`0 == offset`), the first statement of the `0x1` choose leg (`:2565-2576`). ibus-libpinyin 1.16.5 always passes 0 on that path (`PYPLibPinyinCandidates.cc:139-145`), so only another caller reaches it. A NORMAL row chosen at offset 1 after `pinyin_guess_candidates(0, 0x1)` aborts the pin with SIGABRT; at offset 0 it answers `1`. [Register entry](upstream-divergences.md#pin-aborts-reproduced). |
| 46 | Empty-string user dir in `pinyin_init`/`zhuyin_init` | **CLOSED** | The pin stores `g_strdup(userdir)` (`pinyin.cpp:332`, `zhuyin.cpp:276`); its guards test only the pointer (`pinyin.cpp:1133`, `:2671`; `zhuyin.cpp:548`, `:1697`), and `g_build_filename` drops an empty element. With `""` every user file resolves against the current directory at that file operation. A NULL `userdir` makes train and save answer `false` and write nothing. oxpinyin keeps `""` and NULL apart from the C boundary down (`crates/oxpinyin-capi/src/context.rs`, `crates/oxpinyin-zhuyin-capi/src/context.rs`, `ContextCore::try_open`'s `Option<&str>`) and opens the store on the relative empty path (`Runtime::open_with_law`). A NULL user dir's in-memory imports (row 73) and the empty system dir (row 71) are outside this row. |
| 47 | `pinyin_init`'s default option word | **CLOSED** | The pin seeds `context->m_options = USE_TONE` (`pinyin.cpp:329`, bit 5) and nothing else. `PINYIN_DEFAULT_OPTION_WORD` is `USE_TONE` (`crates/oxpinyin-facade/src/lib.rs:62-65`; read at `crates/oxpinyin-capi/src/state.rs:62`); the zhuyin word is unchanged. Cases: `default-option-parse`, `default-option-single-key`. |
| 48 | Invalid UTF-8 after valid pinyin in the multi-key parse entries | **CLOSED** | The pin hands the parser the raw bytes and `strlen` (`pinyin.cpp:1498-1509`; `pinyin_iterator_add_phrase`'s reading, `:638`), and the parser stops at the first byte that is no part of a key. The multi-key parse entries and the add-phrase reading take the longest valid UTF-8 prefix (`cstr_to_parsed_prefix`, `crates/oxpinyin-capi/src/ffi.rs` and its zhuyin twin). The single-key parsers, `lookup_tokens`, `remember_user_input`, the phrase argument of `iterator_add_phrase` and the zhuyin add-phrase reading are unchanged because the pin answers the same. Cases: `bytes-*`. |
| 49 | `pinyin_init` and `zhuyin_init` with a non-UTF-8 directory path | **CLOSED** | The pin passes the `char *` paths to `g_build_filename` and `fopen` as bytes (`pinyin.cpp:331-336`, `zhuyin.cpp`). `ContextCore::open` and `try_open` take the system directory as a `&Path` and the user directory as an `Option<&Path>`, and both C facades pass the caller's bytes unchanged (`ffi::cstr_to_path`). A non-UTF-8 user directory is a user directory. Cases: `non-utf8-directory-system`, `-user`, `-both`, `-zhuyin-both`. |
| 50 | Out-params the pin leaves alone on failure, and writes on a failed walk | **CLOSED** | (1) `pinyin_get_sentence` with no row returns `false` with `*sentence` untouched (`pinyin.cpp:1470-1471`). (2) `pinyin_get_character_offset` whose walk fails stores `*plength = length` (`:3238`); the earlier `false` returns (`:3200-3228`) leave it untouched; the capi layer tells them apart (`character_walk_ran`). (3) `pinyin_token_get_phrase` on an unknown token leaves both out-params untouched (`_token_get_phrase`, `:1627`). (4) `pinyin_token_get_nth_pronunciation` empties the key array first (`:2805`), and an `nth` past the last reading answers `true` with `phrase_length` keys (`:2808-2817`); row 51 covers the content of those keys. oxpinyin follows all four. Cases: `empty-parse-sentence`, `character-offset-out`, `token-get-phrase-out`, `nth-pronunciation-range`. The zhuyin twins are not changed. |
| 51 | `pinyin_token_get_nth_pronunciation` keys past the last reading | **(b)** | An `nth` past the last reading appends `phrase_length` keys from `ChewingKey buffer[MAX_PHRASE_LENGTH]`, an uninitialised stack array, because the pin ignores the return value of `PhraseItem::get_nth_pronunciation` (`pinyin.cpp:2808-2817`; `MemoryChunk::get_content` refuses the read and writes nothing). The content depends on earlier calls, so no safe construction reproduces it. oxpinyin answers `true` with zeroed keys (`crates/oxpinyin-capi/src/dict.rs`). `token_pronunciation_surface` (`crates/oxpinyin-capi/tests/abi/phrase.rs`) holds the pin's return value and key count; the recorded pin observation `nth-pronunciation-range` excludes the content. [Register entry](upstream-divergences.md#pinyin_token_get_nth_pronunciation-past-the-last-reading-policy-row-51). |
| 52 | Process-wide `ChewingKey` and `ChewingKeyRest` slots; `guint16` key-rest length | **CLOSED** | The pin returns `&` of function-local `static`s (`static ChewingKey key`, `pinyin.cpp:2936`; `static ChewingKeyRest key_rest`, `:2960`): one slot for all instances, overwritten by the next call, valid after the instance is freed. oxpinyin keeps both slots as `static`s of `AtomicU16` (`crates/oxpinyin-capi/src/cursor.rs`). `pinyin_get_pinyin_key_rest_length` is `m_raw_end - m_raw_begin` in `guint16` (`chewing_key.h:111-113`, `pinyin.cpp:2979-2984`): 65533 for (5, 2), 1 for (65535, 0); oxpinyin uses `wrapping_sub`. Cases: `static-key-slots`, `key-rest-length`. The zhuyin twins are row 67. |
| 53 | Sentence, candidate and unload return values on empty-state paths | **CLOSED** | (1) `pinyin_get_sentence` with no row answers `false` with `*sentence` untouched (`pinyin.cpp:1470-1471`); oxpinyin no longer answers the raw preedit ([record](sentence-surface.md)). (2) Both guesses on a zero-consumption parse, on both facades, validate against the empty matrix and answer `false` before they clear earlier rows (`lookup/phonetic_lookup.h:743-748`, `storage/phonetic_key_matrix.cpp:34-38`; callers `pinyin.cpp:1379-1384,1434-1439`, `zhuyin.cpp:902-907,955-960`). (3) `pinyin_guess_candidates` on that matrix frees the list and answers `false` (`pinyin.cpp:2193-2196`). (4) `pinyin_unload_phrase_library(GBK)` always answers `true` (`pinyin.cpp:473-474`). (5) An invalid UTF-8 prefix decodes as an empty prefix (`:1397-1403`). Cases: `zero-consumption-*`, `sentence-before-guess`, `sentence-before-guess-schemes`, `guess-sentence-keyless`, `guess-candidates-keyless`, `unload-phrase-library-repeat`, `sentence-with-prefix-bytes`. |
| 54 | Three parse and lookup divergences | **CLOSED** | (1) A leading apostrophe: oxpinyin keeps original-coordinate leading empty columns for generation and candidate lookup, as the pin does. (2) An empty parse after an earlier parse consumes 0 and makes the next guess answer `false` on both facades; restoring the input keeps or validates constraints as the pin does (`pinyin.cpp:1362-1380`, `zhuyin.cpp:886-903`, `lookup/phonetic_lookup.cpp:120-138`). (3) `pinyin_guess_candidates` after `pinyin_unload_phrase_library(2)` rebuilds the list without a re-parse (`pinyin.cpp:2193-2196,2220-2262`, `zhuyin.cpp:1468-1471,1491-1512`). Cases: `row54-*`. |
| 55 | The three auxiliary-text functions read one shared matrix | **CLOSED** | The pin fills one `PhoneticKeyMatrix` per instance whatever parser ran, and the full, double and chewing auxiliary-text functions all read it (`pinyin.cpp:3295-3576`); they differ only in how they cut a split key and spell the others. `crates/oxpinyin-capi/src/aux_matrix.rs` rebuilds that matrix from the current parse (`fill_matrix`, `phonetic_key_matrix.cpp:27-80`) and ports the three loops, tones included. With several keys at one column it reads item 0 (`get_item(column, 0)`). A keyless parse answers `false` with an empty string (`:34-38`). The abort sites (`pinyin.cpp:3311`, `:3488`) are row 63. Cases: `aux-text-*`, including `aux-text-divided-table`, `-resplit-table`, `-divided-and-resplit`, `-fuzzy-keys`. |
| 56 | Unigram adds stop at the sub-index `guint32` total | **CLOSED** | The facade total takes every delta; the sub-index refuses one when `m_total_freq > m_total_freq + delta` (`phrase_index.h:628-635`, `phrase_index.cpp:169-171`). `train_result3` ignores the refusal (`phonetic_lookup.h:928-929`); `pinyin_choose_predicted_candidate` answers `false` on `ERROR_INTEGER_OVERFLOW` before the bigram (`pinyin.cpp:2609-2612`). oxpinyin's `update()` routes both through the guarded add; the predicted path returns `UserStoreError::UnigramTotalOverflow`. Cases: `train-unigram-total`, `predicted-unigram-overflow`. Not covered: a mix of ABI adds (`pinyin_token_add_unigram_frequency`, runtime overlay) and training (user store) near a full library. |
| 57 | Predicted-candidate return table, bigram predecessor, `remember_user_input` negative counts | **CLOSED** | (1) A punctuation row answers `true` without training (`pinyin.cpp:2604-2605`). (2) The bigram predecessor is `_get_previous_token(instance, 0)`, the longest token of `m_prefixes`, or `sentence_start` when empty (`pinyin.cpp:1711-1740`, `:2618`); `CapiInstance::prefixes` mirrors `m_prefixes`. (3) `pinyin_remember_user_input` takes any `gint` count other than -1 as a `guint32` (`pinyin.cpp:520-524`). The overflow answer is row 56. Cases: `choose-predicted-types`, `remember-count-*`. `e2e_tests::a_predicted_bigram_trains_from_the_prediction_prefix` holds the predecessor, because no ordering shows it without `DYNAMIC_ADJUST` (row 58). |
| 58 | Predicted rows: prefix-only training and live unigram ranking | **CLOSED** | (1) A prefix choice adds 483 to its unigram, checks library-total overflow and returns before any bigram write (`074a2219 pinyin.cpp:2607-2616`); `GenericUserStore<S>::observe_predicted_prefix` returns seed 69 and refusal keeps the facade-total increment only. (2) Both prediction row kinds read live item counters and the wrapped facade total; lambda is a `gfloat`, and score conversion, signed wrapping comparison, merge order and duplicate winners follow the pin. (3) Prediction passes a null predecessor (`:2418,2438`), so the bigram term (`:1845-1866`) is zero in both `DYNAMIC_ADJUST` states. The zero-total case is row 61. No zhuyin twin exists. |
| 59 | Session input cap against the pin's `gint16` steps | **POLICY-ACCEPTED** (language artefact) | The pin's full-pinyin parser keeps `parsed_len`, `m_last_step` and step values in `gint16`s (`pinyin_parser2.cpp:67-70`, `:276`, `:290`, `:333`, `:349`, `:353-359`), so no parse consumes more than 32767 bytes, with no fault. oxpinyin parses the whole input. Input up to 32767 bytes agrees; only full-pinyin input or import readings longer than 32767 bytes differ. The stop is a storage width, a language artefact. [Register entry](upstream-divergences.md#the-full-pinyin-parses-16-bit-steps). |
| 60 | `pinyin_alloc_instance` on a finalised context | **(b)** | The pin reads the freed context (`context->m_phrase_index`, `pinyin.cpp:1322`; valgrind on bdb: invalid read of size 8) and survives by chance with a dangling constructor argument. No safe construction reproduces a read of freed memory. A liveness registry (`crates/oxpinyin-capi/src/live.rs`) records live contexts, and `pinyin_alloc_instance` answers NULL for a dead address, with no log. `zhuyin_alloc_instance` after `zhuyin_fini` (`zhuyin.cpp:845-857`) has its own registry (`crates/oxpinyin-zhuyin-capi/src/live.rs`). Cases: `alloc-instance-after-fini`, `zhuyin-alloc-instance-after-fini`. [Register entry](upstream-divergences.md#pinyin_alloc_instance-on-a-finalised-context-policy-row-60). |
| 61 | Facade unigram total wraps as `guint32`; a zero total asserts | **pin behaviour reproduced** | `FacadePhraseIndex::add_unigram_frequency` does `m_total_freq += delta` on a `guint32` (`phrase_index.h:633`); `_compute_frequency_of_items` then **`assert`**s `0 < total_freq` for ranked candidates (`pinyin.cpp:1859`; zhuyin `zhuyin.cpp:1261`). oxpinyin's overlay total (`RuntimeDict::unigram_total_delta() -> u32`) and language-model total wrap (`crates/oxpinyin-runtime/src/lib.rs`), and both facades abort where the pin asserts. ABI-add deltas reach the ranking denominator (`pinyin.cpp:2836-2842`, `:1855-1866`) through `RuntimeLm::unigram_total` and `Session::candidate_frequencies` (`crates/oxpinyin-engine/src/session/lookup.rs`); the case `token-add-unigram-denominator-candidates` isolates it. [Register entry](upstream-divergences.md#pin-aborts-reproduced). |
| 62 | Library-index and export-iterator asserts | **pin behaviour reproduced** | All **`assert`**: `pinyin.cpp:457` (`pinyin_load_phrase_library` of unused slot 0 or 8–15; 16 or more answers `false`, `:448`); `:709` (`pinyin_iterator_get_next_phrase` on an exhausted or empty iterator); `:902` (`pinyin_bigram_iterator_get_next_phrase` with predecessor `null_token` or `sentence_start`). A malformed present item dies at `:714` or `:724` **`check_result`**, `:721`, or `src/storage/chewing_key.cpp:48`, `:50` (from `:728`). An undecodable text still counts as holding a pronunciation; a code point at or above `0x80000000` exports a NULL `phrase` (`pinyin.cpp:715-716`); other non-scalars export as GLib's bytes. The session's phrases are in memory, so no store-read error exists. Tests: `a_reading_the_engine_cannot_spell_is_the_pins_pinyin_string_assert`, `an_undecodable_item_is_an_assert_site_and_a_store_failure_is_not` (`crates/oxpinyin-facade/src/export_rows.rs`), `e2e_tests::export_iterator_aborts_on_a_reading_it_cannot_spell`. No zhuyin twin. The other bigram outcome is row 75. [Register entry](upstream-divergences.md#pin-aborts-reproduced); [in-memory](library-transitions-in-memory.md). |
| 63 | Key accessors on an empty matrix; double aux text cut three bytes in | **pin behaviour reproduced** | `storage/phonetic_key_matrix.h:103` **`assert`**: `pinyin_get_pinyin_key` and `_key_rest` test `offset >= matrix.size() - 1`, which wraps on an empty matrix, so `get_column_size` asserts; on a matrix with keys they answer `false`. `pinyin.cpp:3488` **`abort()`**: `pinyin_get_double_pinyin_auxiliary_text` with a cursor three or more bytes into a key (e.g. `zhong` at 3). `pinyin.cpp:3311` **`assert`**: all three auxiliary-text functions after a leading `'` (`'nihao`, `''ni`). oxpinyin aborts at each site. Cases: `abort-get-pinyin-key-empty-matrix`, `abort-get-pinyin-key-rest-empty-matrix`, `abort-double-auxiliary-text-cut-three-bytes-in`, `abort-*-auxiliary-text-leading-apostrophe`. [Register entry](upstream-divergences.md#pin-aborts-reproduced). |
| 64 | Candidate-type asserts on choose, n-best index and remove | **pin behaviour reproduced** | All **`assert`**: `pinyin.cpp:2507` (`pinyin_choose_candidate` on a predicted row), `:2593` (`pinyin_choose_predicted_candidate` on a non-predicted row), `:2883` (`pinyin_get_candidate_nbest_index` on a non-sentence row), `:3734` and `:3738` (`pinyin_remove_user_candidate` on a non-`NORMAL` or non-user row). oxpinyin aborts at each site. Cases: `abort-choose-candidate-predicted-prefix-row`, `abort-choose-candidate-predicted-bigram-row` (bdb only), `abort-choose-predicted-candidate-normal-row`, `-sentence-row`, `abort-get-candidate-nbest-index-normal-row`, `abort-remove-user-candidate-*`. `boundary_tests::choose_candidate_aborts_on_a_predicted_prefix_row` holds the shared match arm. [Register entry](upstream-divergences.md#pin-aborts-reproduced). |
| 65 | `pinyin_guess_candidates` lists the `LONGER_CANDIDATE` row at every valid offset | **CLOSED** | The pin runs `_prepend_longer_candidates` after the span loop for every valid offset (`pinyin.cpp:2292-2293`). The LONGER search starts at column 0 with `prefix_len = m_parsed_key_len` (`:1876`, `:1883`; `phonetic_key_matrix.cpp:504-529`), so an empty column, an in-span apostrophe (`search_matrix_recur`, `:388-394`) and an unparsed trailing byte (`nih`, `niha`) still get the row. `Session::prepend_longer_row` (`crates/oxpinyin-engine/src/session/lookup.rs`) runs before `prepend_nbest_rows`; `longer_candidate` ends at `selected.last().to()`. The zhuyin facade has no LONGER prepend (`zhuyin.cpp:1460-1539`). Cases, both facades: `guess-candidates-lookup-offset`, `guess-candidates-lookup-offset-zhuyin`, `guess-candidates-past-the-reserved-slot`. |
| 66 | Zhuyin twins and offset probes past the matrix, with pinyin siblings | **pin behaviour reproduced** | All **`assert`** unless marked: `zhuyin_set_chewing_scheme` (`zhuyin_parser2.cpp:295`, `zhuyin.cpp:736` **`abort()`**); `zhuyin_set_full_pinyin_scheme(0 or 4)` (`pinyin_parser2.cpp:398` **`abort()`**); `zhuyin.cpp:372`, `:381` (load/unload library index); `zhuyin.cpp:1453` (`_check_offset`, past the reserved slot and at a leading-separator zero-key column, `'ni'hao`, `'ni'`); `zhuyin.cpp:2158`; key and key-rest on an empty matrix (`phonetic_key_matrix.h:103`); `zhuyin.cpp:1261` (row 61). Pinyin siblings: `pinyin_get_right_pinyin_offset` on an empty matrix (`pinyin.cpp:3085`) and `pinyin_guess_candidates` two or more past the reserved slot; at `parsed_len + 1` it answers `true` (`:2226`). The matrix has `parsed_len + 1` columns once a key is placed, none otherwise (`phonetic_key_matrix.cpp:34-38`); offset getters abort past it. Parse-time matrix reads (`pinyin.cpp:2930-3095`; `zhuyin.cpp:1902-2053`) are source-derived. Death tests: `matrix_bound_tests.rs` of both C crates. [Register entry](upstream-divergences.md#pin-aborts-reproduced). |
| 67 | Zhuyin twins of rows 50, 52 and 53 | **CLOSED** | (1) `zhuyin_get_sentence` answers `false` with `*sentence` untouched when no row exists (`zhuyin.cpp:988-989`). (2) `zhuyin_get_character_offset`'s failed walk stores 0 (`zhuyin.cpp:2192`) and fails at the reserved slot (`:2095-2140`). (3) `zhuyin_token_get_phrase` on an unknown token leaves both out-params untouched. (4) `zhuyin_token_get_nth_pronunciation` content is row 70. (5) Key slots are process-wide `static`s (`zhuyin.cpp:1898`, `:1921`). (6) Key-rest length wraps in `guint16`. (7) `zhuyin_unload_phrase_library` refuses only `TSI_DICTIONARY` (`zhuyin.cpp:378-388`); the pinyin ABI keeps its GBK-only gate. Tests: `boundary_tests::an_unloaded_user_library_hides_its_token_until_the_load`, `boundary_tests::pinyin_unloads_only_the_gbk_library`, `e2e_tests::unloading_gbk_hides_its_tokens_from_the_candidate_surface`; cases: `zhuyin-*`, `user-library-token-unload-zhuyin`. |
| 68 | Raw stderr diagnostics: fresh user dir, failed init, failed save, failed fini | **CLOSED** | The pin writes raw `fprintf(stderr, …)` lines; oxpinyin writes the same bytes on both facades. (1) Unopenable `user.conf`: `open <dir>/user.conf failed.` with no newline (`table_info.cpp:332`). (2) Unopenable `table.conf`: `open <sys>/table.conf failed.` and `load <sys>/table.conf failed!` (`table_info.cpp:201`, `pinyin.cpp:338`, `zhuyin.cpp:282`). (3) Save with the user dir removed: ten `rename … failed.` lines in the pin's order (`pinyin.cpp:1061`…`:1123`), `write <dir>/user.conf failed.` (`table_info.cpp:382`), and `true` (`pinyin.cpp:1140`); each line is printed at its write or rename site. (4) Paths print as bytes. (5) `pinyin_fini` writes the `user.conf` line; libzhuyin writes nothing. Residual `errno` after a successful call is outside scope ([scope](#scope)). Other raw writers are row 76. Cases: `stderr-*`. |
| 69 | A save with one failed file write: the pin updates the rest | **CLOSED** | The pin ignores a failed write (`_write_files`, `pinyin.cpp:940-1020`), renames every other file, prints one `rename … failed.` line (`:1096`; `:1110` for `user_phrase_index.bin`), rewrites `user.conf` and answers `true`. The result is a mixed profile. `persistence::save_reporting` (`crates/oxpinyin-user/src/persistence.rs`) writes each file on its own, tolerates that file's I/O or store failure, renames in the pin's order and reports failures; only an encoding failure stops it. Cases: `stderr-save-one-tmp-blocked`, `stderr-save-one-final-blocked`, `stderr-save-one-tmp-blocked-zhuyin`. Tests: `failed_renames_report_exact_order_and_preserve_destinations`, `sync_rejecting_chunk_target_does_not_stop_later_files` (`crates/oxpinyin-user/tests/save_semantics_929.rs`). |
| 70 | `zhuyin_token_get_nth_pronunciation` keys past the last reading | **(b)** | The zhuyin twin of row 51. An `nth` past the last reading appends `phrase_length` keys from an uninitialised stack array, because the pin ignores the return value of `PhraseItem::get_nth_pronunciation` (`zhuyin.cpp:1800-1810`). No safe construction reproduces a read of uninitialised stack. oxpinyin zeroes the content (`crates/oxpinyin-zhuyin-capi/src/dict.rs`). `boundary_tests::token_getters_answer_the_pins_ranges_and_outputs` holds the pin's answer (`true`) and key count; the recorded pin observation `zhuyin-nth-pronunciation-range` excludes the content. [Register entry](upstream-divergences.md#zhuyin_token_get_nth_pronunciation-past-the-last-reading-policy-row-70). |
| 71 | Empty and NULL system directory | **CLOSED** | `pinyin.cpp:331-338` and `zhuyin.cpp:275-282` keep the argument and call `g_build_filename`: an empty name resolves to cwd `table.conf`; NULL names the empty filename and fails with two raw stderr lines. Both C boundaries preserve NULL with its exact failed-open stderr, and the facade forwards an empty path unchanged. Cases: `init-system-{empty,empty-missing,null,dot,absolute}-{pinyin,zhuyin}` (init, parse, guess, user files, stderr). |
| 72 | Remembered phrase pronunciation tones | **CLOSED** | `pinyin.cpp:3578-3668` keeps `ChewingKey` tones. The C remember path walks the parse-time matrix as `_remember_phrase_recur` does and stores each completed path's keys with their parsed tones (row 93). `boundary_tests::remember_keeps_the_parse_time_tones` covers import, remember and export on an ordinary user dir; the recorded pin observation is `remember-tones-pinyin`. Zhuyin has no equivalent API. |
| 73 | NULL user directory keeps in-memory user state | **CLOSED** | Pin init creates user tables with a NULL user dir; imports and lookup work, and train and save refuse. oxpinyin uses a transient store that holds system originals and mutable indexes with no filesystem target. |
| 74 | Import with five existing same-text tokens | **pin behaviour reproduced** | `phrase_large_table3.h:95` **`assert`**s `0 <= num && num <= 4` during the sixth add after additions to libraries 1–5; the fifth add succeeds. oxpinyin aborts at the same point on both facades. Cases: `combined-library-sixth-add-*` and the `combined-library-fifth-add-*` controls. [Register entry](upstream-divergences.md#pin-aborts-reproduced). |
| 75 | `pinyin_bigram_iterator_get_next_phrase` after a walk that ends on a real predecessor | **(b)** | The other outcome of row 62's bigram call. `has_next` empties `m_pinyins` (`pinyin.cpp:798-801`) and leaves `m_index_token` at the last predecessor (`:885-886`). If that is a real token, the `:902` assert passes and `get_next` indexes the empty array at `:905`: undefined behaviour. The pin dies of SIGSEGV at `pinyin.cpp:905`. No safe construction reproduces the read. oxpinyin answers `false` with the out-params untouched and no warning (`crates/oxpinyin-capi/src/iterators.rs`). Hash order decides which outcome a walk reaches (`pinyin.cpp:779`; `storage/ngram_bdb.cpp:178-200`), so no contract case exists. [Register entry](upstream-divergences.md#pinyin_bigram_iterator_get_next_phrase-after-a-walk-that-ends-on-a-real-predecessor-policy-row-75). |
| 76 | `mmap %s failed!` and the empty chunk the pin dies on | **(b)** for the read through the empty chunk; **CLOSED** for the `mmap %s failed!` line | `MemoryChunk::mmap` answers `false` for a file that cannot be opened, is shorter than the 8-byte header, has a wrong length word or fails its checksum (`memory_chunk.h:470-520`). Each caller prints `mmap %s failed!` and continues with the reset, empty chunk; `SubPhraseIndex::load` then reads `*(buf_begin + 16)` through a NULL base (`phrase_index.cpp:354`) and the pin dies of SIGSEGV. The `open %s failed!` branches are not compiled (`memory_chunk.h:31-34`, oracle `config.h:42` `HAVE_MMAP 1`). Sites: `pinyin.cpp:256` (`pinyin_init`, `pinyin_load_phrase_library`), `zhuyin.cpp:200` (`zhuyin_init`, `zhuyin_load_phrase_library`), `pinyin.cpp:290` (`pinyin_load_addon_phrase_library`), `pinyin.cpp:956` and `zhuyin.cpp:589` (a modified save, through `phrase_index.cpp:278`), `pinyin.cpp:1265` and `zhuyin.cpp:800` (mask-out). The pin stays silent for a library already loaded (`pinyin.cpp:242-244`, `zhuyin.cpp:186`), an unloaded library, a save with nothing modified and a whole-file reload. No public call reaches `zhuyin.cpp:234` (asserts at `:330`, `:372`), `facade_chewing_table.h:115` or `facade_phrase_table2.h:96` (classes not instantiated or exported). For an absent, four-byte or checksum-flipped library the pin writes the line and dies of SIGSEGV at every site (`stderr-library-*` cases). Undefined behaviour, so class (b). oxpinyin writes the pin's bytes through `oxpinyin_data::c_stderr` on `LibraryError::Unmappable`. A separator or index outside the payload is `LibraryError::Format` with no line and no read (`crash-oob-layout-{pinyin,zhuyin}`). A payload shorter than 17 bytes is `LibraryError::Layout`: the caller loads the empty sub-index with no critical ([Phrase-library layout faults write no critical](upstream-divergences.md#phrase-library-layout-faults-write-no-critical); `short-chunk-layout-{pinyin,zhuyin}`). Answers: init answers NULL with one GLib warning for a short or corrupt library and non-NULL for an absent one; `pinyin_load_addon_phrase_library` answers `false`. Init judges the user profile (`check_format`) before any library, as the pin does. Init maps each library only after the logs of earlier rows merge (`library_loop::run`, [register entry](upstream-divergences.md#pin-aborts-reproduced)): an earlier log assertion comes first, an unmappable library ends the loop after the lines of earlier absent libraries, and absent libraries do not end it, so the pin's bytes are a prefix. A save that cannot remap a loaded system library writes the line, stops at that library, keeps all earlier filesystem changes (libpinyin `.tmp` files, libzhuyin interleaved renames; `src/pinyin.cpp:922-1147`, `src/zhuyin.cpp:547-703`), does no later write, rename, marker update or rollback, keeps the session dirty and answers `false`; a later save retries normally. A mask-out checks the user tables first (`pinyin.cpp:1228-1230`), then walks the system libraries in index order (`pinyin.cpp:1260-1294`, `zhuyin.cpp:795-829`); at the first unmappable library it writes its line and answers `true` without masking or writing anything, and keeps all live buffers and unsaved changes. A reload that cannot remap leaves the library unloaded with its mask set, answers `true`, and a later load retries (`pinyin.cpp:246-264`; row 118). A directory of reported size 8 or more is row 96. Held by `save_reports_an_absent_system_library_and_answers_false`, `an_unmappable_save_baseline_stops_before_the_library_write`, `a_failed_reload_keeps_the_library_unloaded_until_the_next_load`, `a_reload_maps_a_system_row_that_names_no_user_file`, `an_absent_library_keeps_its_line_before_a_later_unmappable_one`, `a_bad_log_aborts_before_a_later_library_that_cannot_map` and `init_reports_only_what_the_library_loop_reached`; pin observations `crash-save-missing-system{,-zhuyin}` and `crash-save-remap-failed-retry{,-zhuyin}`. |
| 77 | Chunk header write failure during save (`memory_chunk.h:543`/`:547`) | **pin behaviour reproduced** | `MemoryChunk::save` writes the 4-byte length word and then the 4-byte checksum, and `assert`s that each `write` returned the whole word (`memory_chunk.h:543`, `:547` at 074a2219). A filesystem that refuses the write (`/dev/full`, `RLIMIT_FSIZE`, a full disk) aborts the pin in `pinyin_save`/`zhuyin_save` of a dirty context, after the `.tmp` files of earlier libraries. oxpinyin aborts at the same point with the same files (`SaveOutcome::ChunkWriteFailed`; cases `abort-save-chunk-header-length`, `-checksum`). A refused payload write stays a per-file failure under row 69. [Register entry](upstream-divergences.md#pin-aborts-reproduced). |
| 78 | `table.conf` rows decide the library set | **CLOSED** | `SystemTableInfo2::load` (`table_info.cpp:194-294`, `074a2219`) reads five header `fscanf`s and rows of six words. `pinyin_init`/`zhuyin_init` (`pinyin.cpp:377-392`, `zhuyin.cpp:318-334`) load every default row that is not `NOT_USED`, `pinyin_load_addon_phrase_library` loads addon rows, and `_write_files`/`_rename_files`/`_clean_user_files` (`:915-1130`, `:150-170`) use the file names the rows give. oxpinyin has one reader, `oxpinyin_data::table_info::TableConf::parse`, that follows the pin's `fscanf` stream (`%255s`, short last row dropped, addon index through `atoi` and `guint8`). It drives the system and addon libraries, the user file names (`UserFileLayout`), `UserStore::has_user_library`, the marker versions (`SystemVersions::from_conf`) and λ. File names are raw bytes joined as `g_build_filename` joins them. Write and rename order matches the pin, including names that two roles share. The roles of sub-indices 5, 6 and 7 stay the fixed constants the pin also hard-codes; a default row retyped across them matches only in the cases measured. Held by the `table-conf-*` contract cases on both facades. λ is read with the host libc's `%f` through the pin's own call ([External I/O](../source-policy.md#external-io)); this is not a divergence and no class applies. A host libc whose `%f` accepts other spellings (glibc 2.41 accepts `1e`, `1e+`, `0x1p`, `0x1p+`, `0x1p-` and `0x.`; `vfscanf-internal.c:2196-2201`, `:2545-2549`, `strtod_l.c:690-700`) loads where the subject's 2.43-3 behaviour refuses. The cases `table-conf-lambda-{exponent,hex-exponent}-without-digits`, `table-conf-919-lambda-{exponent-sign,hex-exponent-sign,hex-exponent-minus}-without-digits` and `table-conf-919-lambda-hex-dot-without-digits` are host-dependent on both facades. |
| 79 | Malformed `table.conf` aborts the pin | **pin behaviour reproduced** | Pin sites at `074a2219`: `table_info.cpp:119`, `:132` (the `table.conf` half; `user.conf` is row 44), `:142`, `:156`, `:175` (`abort()`); `:276` (`assert`, addon index outside 0..15 after the `guint8` cut); `pinyin.cpp:388`, `zhuyin.cpp:330` (`assert(DICTIONARY != m_file_type)`); `pinyin.cpp:491`; `pinyin.cpp:457`/`zhuyin.cpp:372`. The `table_info.cpp` sites die before `check_format` and leave the user dir empty; `:388`/`:330` die after it (libpinyin has written `user.conf`); `:491`, `:457`/`:372` die in their load call. A row without a system or user file makes `MemoryChunk` read a directory: `memory_chunk.h:493` (`mmap`) and `:434` (`load`) assert only where `lseek(SEEK_END)` on the directory answers 8 or more, which the file system decides. oxpinyin aborts at the same sites after the same preceding work, and first writes the `mmap %s failed!` lines of libraries mapped before the fatal row (`a_default_dictionary_row_reports_only_the_libraries_before_it`). Where a directory answers less than 8, `:493` is row 76's path: the pin writes `mmap <dir> failed!` and dies of SIGSEGV (class (b)); oxpinyin writes the same line (`phrase_library::directory_reads_as_a_chunk`) and gives the abort site's declared answer with one warning. Held by the `table-conf-abort-*` cases and filesystem-independent Rust tests for the directory shapes. [Register entry](upstream-divergences.md#pin-aborts-reproduced). |
| 80 | Missing `table.conf` header line; candidates over an unreadable item | **(b)** | (1) A missing `source table format:` line leaves `str` unwritten, and `to_table_phonetic_type` compares it (`table_info.cpp:228-229`): undefined behaviour (the pin aborts on both facades). oxpinyin answers init NULL with one warning (`TableConfError::UninitialisedSourceFormat`). A missing `database format:` line is row 79's `:132`. A missing line 1–3 is the pin's ordinary `false` (`:209-224`): NULL and the raw `load %s/table.conf failed!` line (`table-conf-lambda-line-missing`). (2) A `table.conf` that points a loaded library at another file leaves tokens with no item. `_token_get_phrase` returns `false` (`pinyin.cpp:1635-1637`, ignored at `:2001-2007`) and `compare_indexed_item_with_phrase_string` compares the unset `m_phrase_string` (`:2053`). The pin dies of SIGSEGV in `strcmp` from `pinyin_guess_candidates`; no safe construction reproduces it. oxpinyin applies row 106: a token whose live item cannot be read adds no candidate, and the calls return `true` with no warning. An absent or corrupt library file is row 76. Held by `table-conf-candidates-*` and `table-conf-abort-source-format-line-missing` on both facades. |
| 81 | `pinyin_remove_user_candidate` on a phrase missing from a user table (`pinyin.cpp:3743`/`:3750`/`:3759`) | **pin behaviour reproduced** | Three `assert(ERROR_OK == retval)` sites at `074a2219`: `:3743` after `phrase_index->remove_phrase_item`, `:3750` after `phrase_table->remove_index`, `:3759` after `pinyin_table->remove_index`. Triggers abort the pin (SIGABRT): a candidate removed twice (`:3743`); `user_phrase_index.bin` deleted or without the token (`:3750`); a second reading merged by `_add_phrase` and never indexed (`pinyin.cpp:569-582`) (`:3759`). The store loads `user_phrase_index.bin` as exact `(token, text)` pairs (`PHRASE_TABLE`; `persistence.rs`, `store_libpinyin.rs`), checks the pair and every reading's `indexed` flag, and returns `Ok(false)` without change when a check fails. `pinyin_remove_user_candidate` (`candidates.rs`) aborts on that `Ok(false)`, before the bigram fault of the following `mask_out` (`:3766`; `a_removal_assert_comes_before_the_bigram_fault`). Cases `abort-remove-user-candidate-twice`, `-missing-phrase-table`, `-multi-pron`. The candidate-type asserts `:3734`, `:3738` are row 64. [Register entry](upstream-divergences.md#pin-aborts-reproduced). |
| 82 | Training after a re-parse shortened the input (`phonetic_key_matrix.cpp:661`/`:663`/`:664`) | **pin behaviour reproduced** | The training walk tails the last span at `constraints->length() - 1` (`phonetic_lookup.h:921`), and the constraint store survives a parse unvalidated (`pinyin.cpp:1497-1525`). A re-parse that shortens the input leaves the store longer than the matrix, and `increase_pronunciation_possibility` fails `assert(end < matrix->size())` (`storage/phonetic_key_matrix.cpp:661`) or, with a leading apostrophe, `assert(matrix->get_column_size(start) > 0)` (`:663`). Witness: parse `nihao`, guess, choose index ≥ 2, guess, parse `ni` or `'ni`, train; the pin aborts at `:661` and `:663`. oxpinyin's walk indexes the separator-adjusted topology (`ParsedMatrix::from_scan`) and stops at the store's length (`phonetic_lookup.h:858`), and both facades abort on `EngineError::StaleTrainingSpan`. Cases `abort-train-after-reparse-past-matrix-end`, `-empty-start-column`, `-empty-end-column` and zhuyin twins; held by `train_after_a_reparse_and_a_choose_stops_the_walk_at_the_store_length`. [Register entry](upstream-divergences.md#pin-aborts-reproduced). |
| 83 | `lambda parameter:` outside `[0, 1]` | **CLOSED** for λ handling; **(b)**, **POLICY-ACCEPTED**, for the out-of-range candidate-frequency store | The pin scans λ with `%f` into a `gfloat` (`table_info.cpp:220`) and takes any value; `unigram_lambda` is `1 - lambda` (`phonetic_lookup.h:712-713`); each trellis step adds `log(...)` of the step possibility to `m_poss` (`:663`, `:693`). Below 0 the costs go negative; above 1 `log` gives a sticky NaN, every `m_poss <` comparison with it is false, and `get_tails` sorts with `trellis_value_compare` (`:337`). oxpinyin's `f32` log probability carries `+inf`, `-inf` and NaN (`expand_one` in `crates/oxpinyin-engine/src/nbest.rs`), the raw-λ step costs use the pin's `gfloat`/`gdouble` operations, and `loses_to` makes every comparison with NaN false. The candidate frequency `(λ·bigram + (1−λ)·unigram)·2^24` uses the file's λ and Rust's `as u32` store: a negative or NaN score is 0 and a score at or above 2³² is `0xffffffff`. The pin's conversion there is undefined, so the out-of-range cases (λ > 1 on the zhuyin facade; a unigram above the total) are class (b), accepted under the two candidate-frequency entries of [upstream-divergences](upstream-divergences.md). Held by the `table-conf-lambda-*` cases (λ = −0.5, −2, 1.5, 2, 1e6, `inf`, `nan` on both facades) beside the in-range spellings. |
| 84 | Over-long user chewing index key in `mask_out` and the longer-candidate walk (`chewing_large_table2_bdb.cpp:529`/`:282`) | **pin behaviour reproduced** | The pin's user `ChewingLargeTable2` has `m_entries` only for lengths 1..=16, and `mask_out` (`:529`) and `search_suggestion_internal` (`:282`) dispatch on `db_key.size / sizeof(ChewingKey)` into a `switch` with `default: abort()`. A `user_pinyin_index.bin` key longer than `MAX_PHRASE_LENGTH` (16) aborts `pinyin_mask_out`, `zhuyin_mask_out` (`zhuyin.cpp:763`) and `pinyin_guess_candidates` (`_prepend_longer_candidates`, `pinyin.cpp:2292-2293`). No public call writes such a key (`pinyin.cpp:643`), so the cases craft the database. kc and tkrzw abort at `chewing_large_table2_kyotodb.cpp:269`/`:499` and `chewing_large_table2_tkrzwdb.cpp:252`/`:466`. The cases write the fixture in each backend's format and match on bdb, kc and tkrzw. A key with no whole word is row 94; the system table's over-long key is row 95. [Register entry](upstream-divergences.md#pin-aborts-reproduced). |
| 85 | `user_bigram.db` value shorter than a `guint32` `total_freq` (`memory_chunk.h:390`) | **pin behaviour reproduced**; **POLICY-ACCEPTED** (language artefact) for training over an unsorted gram | `SingleGram::get_total_freq` reads four bytes through `MemoryChunk::get_content<guint32>` (`src/storage/ngram.cpp:48-51`), whose `assert` aborts the pin on a shorter value (`src/include/memory_chunk.h:390` at 074a2219). Paths that read the total: `pinyin_mask_out`/`zhuyin_mask_out` (`pinyin.cpp:1230`, `zhuyin.cpp:765`), `pinyin_remove_user_candidate` (`pinyin.cpp:3766`), `pinyin_guess_predicted_candidates` (`pinyin.cpp:2330`), `pinyin_choose_predicted_candidate` (`pinyin.cpp:2623-2630`, after the unigram add at `:2609-2612`), `pinyin_train`/`zhuyin_train` (`phonetic_lookup.h:878-884`), the lookups through `merge_single_gram` (`ngram.cpp:303-304`), `SingleGram::search` (`ngram.cpp:165`) and `DYNAMIC_ADJUST` (`pinyin.cpp:1848`, `zhuyin.cpp:1250`), and the bigram export's `has_next` (`pinyin.cpp:889`, `ngram.cpp:133-141`). `pinyin_init` does not read it. oxpinyin's facade returns the typed fault at each of these points in the pin's order, and both C adapters abort; the export aborts where its walk loads the short predecessor, after `get_next` writes its row. The parse does not refuse, because the pin's parse merges no gram (`pinyin.cpp:1497-1520`, `zhuyin.cpp:1017-1043`). A partial item tail is `PartialUserBigramTail` and keeps the class (b) outcomes of rows 98 and 100. Training over an unsorted gram (`phonetic_lookup.h:888-903`) answers `true` where the pin aborts; `abort-train-unsorted-set-freq-miss-{pinyin,zhuyin}` are recorded under [Stored gram partitions: training](upstream-divergences.md#stored-gram-partitions-training). Cases and items: [Pin aborts reproduced](upstream-divergences.md#pin-aborts-reproduced). |
| 86 | `user_bigram.db` key that is not a `phrase_token_t` (`ngram_bdb.cpp:199`) | **pin behaviour reproduced** | `Bigram::get_all_items` walks the container with a `DBC` cursor and asserts `key.size == sizeof(phrase_token_t)` (`src/storage/ngram_bdb.cpp:199` at 074a2219). `pinyin_mask_out`/`zhuyin_mask_out` (`pinyin.cpp:1230`, `zhuyin.cpp:765`), `pinyin_remove_user_candidate` (`pinyin.cpp:3766`) and `pinyin_begin_get_bigram_phrases` (`pinyin.cpp:779`) walk it, so a key that is not four bytes aborts each. `pinyin_guess_predicted_candidates` loads a named key and does not abort. kc and tkrzw abort at `ngram_kyotodb.cpp:173` and `ngram_tkrzwdb.cpp:150`. The cases write the fixture in each backend's format and match on bdb, kc and tkrzw. [Register entry](upstream-divergences.md#pin-aborts-reproduced). |
| 87 | `user_bigram.db` gram whose items do not cover its total, masked to nothing (`ngram.cpp:70`) | **pin behaviour reproduced** | `SingleGram::get_length` asserts `0 == total_freq` when no item is left (`src/storage/ngram.cpp:70` at 074a2219), and `Bigram::mask_out` reaches it when it removed at least one item (`ngram_bdb.cpp:243`, `ngram_kyotodb.cpp:224`, `ngram_tkrzwdb.cpp:200`). `SingleGram::mask_out` subtracts each removed frequency, so a total that is not the sum of the items stays nonzero and the assert fails. `pinyin_mask_out`/`zhuyin_mask_out` and `pinyin_remove_user_candidate` abort (`pinyin.cpp:1230`, `zhuyin.cpp:765`, `pinyin.cpp:3766`); the predicted-candidate walk, `pinyin_train` and `pinyin_begin_get_bigram_phrases` complete. The cases write the fixture in each backend's format and match on bdb, kc and tkrzw. [Register entry](upstream-divergences.md#pin-aborts-reproduced). |
| 88 | `user_bigram.db` gram with no items and a residual total (`ngram.cpp:70`) | **pin behaviour reproduced** | `SingleGram::get_length` (`src/storage/ngram.cpp:58`) asserts `0 == total_freq` when the chunk has no complete eight-byte item after its total and the total is nonzero (`:70` at 074a2219). `pinyin_guess_predicted_candidates` reaches it through `_compute_predicted_bigram_candidates` (`pinyin.cpp:2332`/`:2336`). `pinyin_mask_out`/`zhuyin_mask_out` do not: `SingleGram::mask_out` answers 0 and the check is skipped (`ngram_bdb.cpp:243`). `pinyin_guess_candidates` does not reach `get_length` (`pinyin.cpp:2184`); a gram with an item the mask removes is row 87. A value with 1–7 partial item bytes and no complete item reaches the same assertion (row 99 is folded here); the raw DBM keeps the partial bytes. The cases write the fixture in each backend's format and match on bdb, kc and tkrzw. [Register entry](upstream-divergences.md#pin-aborts-reproduced). |
| 89 | Duplicate same-library token aborts `zhuyin_iterator_add_phrase` (`zhuyin.cpp:440`) | **pin behaviour reproduced** | `_add_phrase` (`src/zhuyin.cpp:404-` at 074a2219) searches the phrase table, reduces the tokens and walks them. The first candidate is stored, and a later candidate in the same sub-index library fails `assert(PHRASE_INDEX_LIBRARY_INDEX(token) != index)` (`:440`), so two tokens for one phrase in one library abort the add (SIGABRT). No public call writes two such tokens, so the case crafts `user_phrase_index.bin` (`chewing_large_table2_bdb.cpp:58`) with one key mapping to two library-7 tokens. oxpinyin's `plan_import` aborts at the same point (`abort-zhuyin-add-phrase-duplicate-library-token`). The case writes the fixture in each backend's format and matches on bdb, kc and tkrzw. [Register entry](upstream-divergences.md#pin-aborts-reproduced). |
| 90 | Phrase-index item text mismatch aborts `zhuyin_iterator_add_phrase` (`zhuyin.cpp:457`) | **pin behaviour reproduced** | In the same `_add_phrase` walk, when the lone token in the target library names an item, the pin asserts the lengths match (`src/zhuyin.cpp:456`) and `0 == memcmp(phrase, tmp_phrase, sizeof(ucs4_t) * phrase_length)` (`:457`). A stale `user_phrase_index.bin` that maps a phrase to another phrase's token aborts the add. Witness: persist `你們` in library 7, map `你好` to its token, reopen; `zhuyin_iterator_add_phrase` on `你好` aborts at `:457`. oxpinyin's `plan_import` aborts at the same point (`abort-zhuyin-add-phrase-index-text-mismatch`). The case writes the fixture in each backend's format and matches on bdb, kc and tkrzw. [Register entry](upstream-divergences.md#pin-aborts-reproduced). |
| 91 | Non-null-token `MODIFY_HEADER` in a user `.dbin` (`phrase_index_logger.h:202`) | **pin behaviour reproduced** | `PhraseIndexLogger::next_record` asserts `token == null_token` for a `MODIFY_HEADER` record (`src/storage/phrase_index_logger.h:202` at 074a2219). A `SYSTEM_FILE`/`DICTIONARY` library's `.dbin` with such a record aborts init's `merge`, the reload's merge (`pinyin.cpp:276`, `zhuyin.cpp:219`) and `mask_out`'s `merge_with_mask` (`pinyin.cpp:1273-1285`, `zhuyin.cpp:808-820`). With a `0x01000001` header in `gb_char.dbin`, init and mask-out abort on both facades. oxpinyin replays each `.dbin` in the pin's order (`persistence::replay_logs`): a merge that stops at a mismatched REMOVE or MODIFY (`phrase_index.cpp:468-471`, `:483-484`) never reads the header behind it, and a header the walk reaches returns `JournalFault::HeaderToken`, where both C adapters abort. At init the assertion comes before anything in later libraries, which are not mapped, read or reported ([register entry](upstream-divergences.md#pin-aborts-reproduced)). Held by `init_aborts_on_a_log_header_token`, `reload_aborts_on_a_log_header_token`, `mask_out_replays_the_log_it_reads_and_returns_its_faults`, `a_header_token_after_a_mismatch_is_never_read`, `init_aborts_on_a_log_before_an_unmappable_library` and the `library_loop` tests. [Register entry](upstream-divergences.md#pin-aborts-reproduced). |
| 92 | More than one `MODIFY_HEADER` in a user `.dbin` aborts `mask_out` only (`phrase_index.cpp:745`) | **pin behaviour reproduced** | `_peek_header` counts `MODIFY_HEADER` records and asserts `1 >= header_count` (`src/storage/phrase_index.cpp:745` at 074a2219). Only `merge_with_mask` (the `mask_out` read) reaches it; init's `merge` applies every header without peeking. A `.dbin` with two null-token headers therefore loads at init and aborts `pinyin_mask_out`/`zhuyin_mask_out`. oxpinyin's `mask_out` walks each library through `merge_with_mask`'s order, and the count returns `JournalFault::MultipleHeaders`, where both C adapters abort. The `.dbin` is a backend-independent `MemoryChunk`. `abort-mask-out-multiple-log-headers-{pinyin,zhuyin}` and the controls `init-multiple-log-headers-{pinyin,zhuyin}` match on bdb, kc and tkrzw. Held by `mask_out_aborts_on_two_log_headers_and_leaves_the_files` and `init_accepts_two_log_headers`. [Register entry](upstream-divergences.md#pin-aborts-reproduced). |
| 93 | Pinyin import phrase-table asserts (`pinyin.cpp:554`, `:568`, `:571`) | **pin behaviour reproduced** | The pinyin `_add_phrase` (`src/pinyin.cpp:514-` at 074a2219) is the twin of rows 89 and 90: a later token in the target library fails `assert(PHRASE_INDEX_LIBRARY_INDEX(token) != index)` (`:554`); the lone token's item fails `phrase_length == item.get_phrase_length()` (`:568`) or the `memcmp` (`:571`). A crafted or stale `user_phrase_index.bin` reaches each: two library-7 tokens, a longer phrase's token, or another same-length phrase's token. A token without an item leaves length zero and aborts at `:568`. `pinyin_remember_user_input` reaches the same asserts only where `_remember_phrase_recur` (`pinyin.cpp:3578-3666`) calls `_add_phrase` for `USER_DICTIONARY` (`:3702`). It walks the parse-time matrix with the alternatives added under the options at the parse (`pinyin.cpp:1514-1520`, `:1557-1559`, `:1602-1604`), keeps a key only when the item accepts it (`:3640-3645`), and adds each completed path in depth-first order (`:3600-3609`). oxpinyin's `plan_import` decides the asserts for any library index, once per completed path, so a path that stops short or is not pronounced answers `false`. An empty matrix is row 66. Held by one death test per assert and controls (`abort-add-phrase-duplicate-library-token`, `-index-length-mismatch`, `-index-item-missing`, `-index-text-mismatch`). [Register entry](upstream-divergences.md#pin-aborts-reproduced). |
| 94 | User chewing index key with no whole word aborts `mask_out` | **pin behaviour reproduced** | `ChewingLargeTable2::mask_out` dispatches on `db_key.size / sizeof(ChewingKey)` into a `switch` with cases 1..=16 and a `default` of `abort()` (`chewing_large_table2_bdb.cpp:529`; kc `:499`, tkrzw `:466`). An empty or one-byte key has a word count of 0 and reaches that `default`; row 84 holds the over-long end. No public call writes such a key. `pinyin_mask_out` and `zhuyin_mask_out` (`zhuyin.cpp:763`) abort at `:529`. Cases: `abort-mask-out-empty-index-key`, `abort-mask-out-one-byte-index-key`, `abort-zhuyin-mask-out-empty-index-key`. Held by `oxpinyin-capi`'s `boundary_tests::mask_out_aborts_on_an_empty_index_key` and `oxpinyin-zhuyin-capi`'s `boundary_tests::mask_out_over_an_overlong_user_index_key_aborts`. [Register entry](upstream-divergences.md#pin-aborts-reproduced). |
| 95 | Over-long system `pinyin_index.bin` key aborts `search_suggestion` | **pin behaviour reproduced** | `FacadeChewingTable2::search_suggestion` walks the system and then the user `ChewingLargeTable2` (`facade_chewing_table2.h:148`); both dispatch on the key's word count into the 1..=16 `switch` (row 84 covers the user table). A system key past 16 words that extends the query's key aborts `pinyin_guess_candidates` in `search_suggestion_internal` (`chewing_large_table2_bdb.cpp:282`; kc `chewing_large_table2_kyotodb.cpp:269`, tkrzw `chewing_large_table2_tkrzwdb.cpp:252`). `ChewingTable::walk_suggestions` does one exact lookup and one range walk and checks raw key lengths. Cases: `abort-guess-candidates-overlong-system-index-key`, `abort-guess-candidates-overlong-odd-system-index-key`. Held by `chewing_table::tests::has_overlong_extension_reads_keys_past_sixteen_words_under_the_query` (`oxpinyin-data`) and `boundary_tests::guess_candidates_aborts_on_an_overlong_index_key` (`oxpinyin-capi`). [Register entry](upstream-divergences.md#pin-aborts-reproduced). |
| 96 | System library that is a directory aborts `MemoryChunk::mmap` | **pin behaviour reproduced** (the pin site depends on the directory's reported size) | `MemoryChunk::mmap` takes `file_size = lseek(fd, 0, SEEK_END)`; below the 8-byte header it answers `false` (`memory_chunk.h:479-483`, row 76's path). From 8 up, `read` fails with `EISDIR` and `assert(ret_len == sizeof(length))` aborts (`:493`). ext4 always reports a size that reaches `:493`; btrfs reports twice the summed entry-name length. `map_chunk` (`directory_chunk_fault`, `crates/oxpinyin-data/src/phrase_library.rs`) takes the size as the pin does: under 8 it is `LibraryError::Unmappable`, from 8 up `LibraryError::Row(RowFault::ChunkHeaderRead)`. Init, reload, save, mask-out and the addon load (`pinyin.cpp:290`) return the fault at the pin's point and both C facades abort. At init the directory is one stop of `library_loop::run`, after the earlier logs merge. Filesystem-independent Rust tests hold it, including `a_directory_library_stops_the_loop_after_the_earlier_logs`. [Register entry](upstream-divergences.md#pin-aborts-reproduced). |
| 97 | Surviving ADD or kept REMOVE in a masked system `.dbin` aborts `mask_out` | **pin behaviour reproduced** | `merge_with_mask` checks the library bits first (`phrase_index.cpp:309-313`). For a loaded matching library with a log header, `_mask_out_records` re-appends each unselected ADD with `&oldchunk` (`:838`), which fails `assert(NULL == oldone)` (`phrase_index_logger.h:245`); a kept REMOVE fails `assert(NULL == newone)` (`:257`). Both facades abort after saving a library-1 addition and calling `mask_out(0x00FFFFFF, 0)`. Selected ADDs, absent headers, unloaded libraries and mismatched library bits do not reach this site. oxpinyin returns `JournalFault::SurvivingRecord` after the user tables are masked, and both C adapters abort. Cases: `abort-mask-out-surviving-add-{pinyin,zhuyin}`. [Register entry](upstream-divergences.md#pin-aborts-reproduced). |
| 98 | Partial `SingleGramItem` tail read past the value | **(b)** | A value of length `4 + 8k + r`, `1 <= r <= 7`, leaves a partial tail that the pin's `cur != end` walks over-read: `SingleGram::retrieve_all` (`ngram.cpp:133`, loop `:143`) for export (`pinyin.cpp:888-889`) and prediction (`pinyin.cpp:2321-2341`), `SingleGram::mask_out` (`ngram.cpp:76`, loop `:86`) for mask and candidate removal, and the lookup walks `get_freq` (`ngram.cpp:239-248`), `search` (`:167-173`) and `merge_single_gram` (`:318-349`). oxpinyin decodes only complete items. Export ends where `has_next` loads that predecessor with one warning and `false` (`export_rows::GramLoadFault::PartialTail`); mask and removal answer `false`; prediction answers `false` with an empty list; lookups answer `false` (`UnsafeLookupPredecessor`); each gives one facade-domain warning. Source-derived. Held by `checked_get_freq_faults_only_where_the_pins_walk_leaves_the_value`, `checked_search_asserts_short_and_faults_past_a_partial_tail`, `a_merge_the_pin_cannot_read_answers_its_fault`, `a_predecessor_row_the_pin_cannot_read_refuses_the_step`, `a_predecessor_row_the_pin_cannot_read_refuses_the_segment`. |
| 100 | Training on a user-bigram value with a partial item | **(b)** | `SingleGram::insert_freq` (`ngram.cpp:178`, loop `:190`) walks eight-byte items with `cur_item != end` and reads past a partial tail of a value of length `4 + 8k + r`, `1 <= r <= 7`; `get_freq` (`:241`) and `set_freq` (`:261`) walk the same gram. `pinyin_train`/`zhuyin_train` reach it through `train_result3` (`pinyin_lookup2.cpp:611`, `phonetic_lookup.h:889`), and `pinyin_choose_predicted_candidate` through `pinyin.cpp:2630-2633`. oxpinyin checks `has_oob_bigram_value_for` per step through `UserStore::check_training_step_with`, called from the runtime's training walk, on the live raw value. Training returns `false` (`UnsafeTrainingPredecessor`) and the predicted choose answers `false`, each with one facade-domain warning; the raw value stays unchanged. Source-derived. Held by `training_over_a_partial_tail_refuses_and_keeps_the_raw_value` and `a_partial_tail_created_in_session_refuses_training` (`store_libpinyin.rs`). |
| 101 | Partial phrase-table token tail | **(b)** | `src/storage/phrase_large_table3.cpp:32-39` walks four-byte tokens with `iter != end`; a value of length `4k + r`, `1 <= r <= 3`, can be read past. Add, remove and mask use the same loop (`:57-65`, `:74-81`, `:103-117`). oxpinyin's `phrase_table.rs` (`validate`, `PhraseTableEntry::from_bytes`) rejects the record with `InvalidInput`; `UserLookup::from_store` keeps the key with no cached tokens; `load_user_phrase_index` and `stage_rest` preserve the raw bytes across load and save. Source-derived. No uniform C ABI answer or new warning is claimed. Distinct from rows 98 and 100. |
| 102 | Candidate walk over an item a library reload dropped | **(b)** | `FacadePhraseIndex::unload` deletes a sub-index (`phrase_index.cpp:261-268`) and `_load_phrase_library` rebuilds it from the system chunk and the saved `.dbin` (`pinyin.cpp:264-277`, `zhuyin.cpp:298-312`). An imported, unsaved item is gone, but its user phrase and pinyin index entries stay. `lookup_tokens` still answers the token and the item reads answer false; oxpinyin reproduces both. At the pin, `guess_candidates` over a reading that reaches the token (龘龘 in library 2, then `dada`) dies of SIGSEGV on both facades. oxpinyin treats the token as an item that does not exist and adds no candidate. The rest of the unload and reload path is reproduced. |
| 103 | User store refusing the write of a library unload or reload | **pin behaviour reproduced** | The pin has no failing site. `FacadePhraseIndex::unload` always answers (`phrase_index.cpp:261-268`); `pinyin_unload_phrase_library` answers `true` (`pinyin.cpp:464-475`; `zhuyin.cpp:379-390`). An unreadable `.dbin` is an empty log, so `_load_phrase_library` answers `true` (`pinyin.cpp:234-322`; `zhuyin.cpp:178-266`; `memory_chunk.h:416-464`). A save skips a library with no sub-index (`pinyin.cpp:929-932`, `:1032-1035`; `zhuyin.cpp:562-565`). oxpinyin's `ContextCore::unload_phrase_library` and `load_phrase_library` (`crates/oxpinyin-facade/src/context.rs`) change only in-memory state. Load returns only the pin's asserts (`NotLoadable`, row 96's `ChunkHeaderRead`, the row 91/108 `Journal` faults). The save skips an unloaded system or `USER_FILE` library. Held by `library_transition_tests` (`context.rs`) and `a_save_skips_an_unloaded_user_library` (`crates/oxpinyin-user/src/store_libpinyin.rs`); cases `unload-save-skips-*` and `reload-unreadable-log-*`. [Record](library-transitions-in-memory.md). |
| 104 | Zhuyin import after a reload dropped the item aborts at `zhuyin.cpp:454` | **pin behaviour reproduced** | Row 93's item-missing site on the zhuyin facade, reached in-session. `_add_phrase` (`src/zhuyin.cpp:431-458`) fetches the item of the phrase-table token in the target library. For a token whose sub-index was freed and rebuilt (rows 102 and 103) `get_phrase_item` fails, and `assert(phrase_length == item.get_phrase_length())` (`:454`) aborts. Witness: import 好你 into library 7 unsaved, unload and load library 7, import 好你 again. A different phrase imported after the reload does not abort: `get_range(7).m_range_end` (`zhuyin.cpp:472-478`) reuses the token; oxpinyin reproduces this by keeping the index entries of a `USER_FILE` library's dropped items (`UserStore::index_phrases`). Cases: `reload-user-zhuyin-reuse-same`, `reload-user-zhuyin-reuse-other`. Held by `oxpinyin-zhuyin-capi`'s `boundary_tests::adding_a_user_phrase_again_after_a_reload_aborts`. [Register entry](upstream-divergences.md#pin-aborts-reproduced). |
| 105 | Second `pinyin_fini` / `zhuyin_fini` on one context | **(b)** | `pinyin_fini` (`src/pinyin.cpp:1194-1222`) reads the freed context, runs `mark_version`, deletes every member and the context again; `zhuyin_fini` (`src/zhuyin.cpp:741-757`) does the same. This is a double free. The pin dies of SIGSEGV on both facades, with unstable stderr. oxpinyin returns silently with no warning: `pinyin_fini` and `zhuyin_fini` return at once for an address the live-context registries (`crates/oxpinyin-capi/src/live.rs`, `crates/oxpinyin-zhuyin-capi/src/live.rs`, row 60) do not hold. A new context at a reused address is live again (as row 60). A second `*_free_instance` (`pinyin.cpp:1347-1356`, `zhuyin.cpp:876-884`) is caller UB and stays unchanged, because an instance registry would cost time and space against the pin. Cases: `fini-twice`, `zhuyin-fini-twice`. Entry in [the divergence register](upstream-divergences.md). |
| 106 | Candidate list over a system item a journal REMOVE took out | **(b)** | A journal REMOVE of a system item (`SubPhraseIndex::merge`, `phrase_index.cpp:460-475`; `remove_phrase_item` `:219-236`) zeroes the item's offset but leaves the phrase table, so `lookup_tokens` still answers the token. `_compute_phrase_strings_of_items` ignores the failed `_token_get_phrase` (`pinyin.cpp:1635-1637`, `:2013-2016`); `strcmp` in `compare_indexed_item_with_phrase_string` (`:2053`) then reads the NULL string. A predicted bigram row whose token has no item reaches the same `strcmp` (`pinyin.cpp:2357-2363`, `:2053`) and takes the same result. Zhuyin twins: `zhuyin.cpp:1096-1104`, `:1360`, `:1537`, `:1630`. The pin dies of SIGSEGV in `pinyin_guess_candidates` and `zhuyin_guess_candidates_{after,before}_cursor` over `bujinjinshi`. oxpinyin answers as row 102: the token adds no candidate, and the calls return `true` with no warning. The same result applies to row 80’s missing item. The live offset read returns the same error for an empty slot and a slot cleared by REMOVE (`phrase_index.cpp:180-205,219-234`). Sentence calls skip the token (`phonetic_lookup.h:646`, `:674`). Cases: `removed-candidate-{pinyin,zhuyin}`, `removed-candidate-before-cursor-zhuyin`. Held by `pinned_data_tests::a_journal_removed_item_adds_no_candidate` (`oxpinyin-capi`). |
| 106a | Prediction list over a system item a journal REMOVE took out | **(b)** for two or more pre-dedup rows; **pin behaviour reproduced** for a sole row | `_compute_predicted_prefix_candidates` (`src/pinyin.cpp:2389-2405`) ignores a failed `get_phrase_item` (`:2392`), so the length gate uses the last successful read. A missing token keeps `m_phrase_string = NULL` (`:107-115`, `:2018-2022`). With two or more pre-dedup rows, `_remove_duplicated_items_by_phrase_string`'s `strcmp` (`:2041-2068`, `:2082`) reads the NULL: oxpinyin takes row 106's answer. With one row, the sort calls no comparator and the NULL row survives (`:2071-2080`); oxpinyin reproduces the getters (`pinyin.cpp:2845-2885`, `:3712-3718`) and the choose side, which advances the facade total by 483 (`:2608-2616`, `phrase_index.h:628-634`). Implemented in `select_prefix_rows` (`crates/oxpinyin-capi/src/predict.rs`) and `pinyin_choose_predicted_candidate` (`crates/oxpinyin-capi/src/candidates.rs`). Cases: `removed-predict-sole-pinyin`, `removed-predict-choose-pinyin`, `removed-predict-class-b-pinyin`, `removed-predict-stale-skip-pinyin`. Held by `boundary_tests::a_predicted_bigram_row_is_not_a_user_candidate`. |
| 107 | System journal record names an absent token or an over-long item | **(b)** | `SubPhraseIndex::merge` (`src/storage/phrase_index.cpp:439-517`), reached through `_load_phrase_library` (`src/pinyin.cpp:276`), dereferences NULL for a REMOVE of an absent token (`:460-475`, `:468`) and for a growing MODIFY after an earlier MODIFY (`:477-501`, `:489`). MODIFY and ADD do not check the chunk against its header (`:208-217`), so a later `get_phrase_item` (`:180-206`) can read past the content buffer. The pin dies of SIGSEGV at init for the absent-token and `replay-modify-header-overlong-{pinyin,zhuyin}` cases. oxpinyin makes a REMOVE of an absent token a no-op, stops a growing stale MODIFY at the record and keeps earlier records. An item whose read passes the buffer end answers no item; other journal changes and totals stay. In-buffer over-reads are reproduced (`replay-modify-header-overlong-small-{pinyin,zhuyin}`). A payload past the log's end rolls the replay back (`replay-over-read-tail-{pinyin,zhuyin}`, `pin_not_run`; `rollback_restores_the_buffer_for_every_kind_of_mutation`). Held by `an_over_read_item_answers_no_item_and_the_records_around_it_stay` and `valid_records_before_and_after_an_over_read_item_stay`. |
| 108 | Short system journal ADD payload aborts at `memory_chunk.h:390` | **pin behaviour reproduced** | `LOG_ADD_RECORD` (`src/storage/phrase_index.cpp:451-457`) passes the payload to `add_phrase_item` (`:208-217`), which reads the unigram through `MemoryChunk::get_content<guint32>` (`phrase_index.h:124-126`, `memory_chunk.h:388-392`); `check_result` is `assert` (`pinyin_utils.h:28-30`). A payload of 0–5 bytes aborts `pinyin_init`/`zhuyin_init`. The reload path (`pinyin.cpp:276`, `zhuyin.cpp:219`) and `mask_out`'s `_compute_new_header` (`phrase_index.cpp:778-795`) reach the same assert. oxpinyin returns `JournalFault::ShortAdd` or `JournalFault::ShortMaskedItem` at the record, and both C adapters abort. Held by `e2e_tests::init_aborts_on_a_short_add_payload` of both capi crates, `persistence::tests::a_short_add_payload_is_the_load_fault` and `replay::tests::a_short_add_is_reported_after_the_records_before_it`. [Register entry](upstream-divergences.md#pin-aborts-reproduced). |
| 109 | Sparse chunk's whole mapping retained after `fini` (`memory_chunk.h:72-78`) | **IMPLEMENTED**; wrapped-size residual **POLICY-ACCEPTED** | The pin maps the whole `file_size` (`src/include/memory_chunk.h:475-524`). `oxpinyin-input::chunk` owns one writable private whole-file memmap2 mapping for each loaded system or addon chunk. `PendingMapping` keeps the descriptor through checksum validation, then closes it. Data transfers a system chunk to its live sub-index and forbids unsafe code. The crate's `Drop` releases the whole mapping. The pin's wrapped-size residual is POLICY-ACCEPTED under the [mapping lifetime ruling](#external-api-misuse). No manual unmap or `ManuallyDrop` is used. Trailing-byte interpretation is unchanged. |
| 110 | Kyoto Cabinet `Bigram::load` body read fails after `check` | **pin behaviour reproduced** | The KC `Bigram::load` (`src/storage/ngram_kyotodb.cpp:124-143`) sizes the record with `m_db->check`, then reads it under `check_result(vsiz == m_db->get(...))` (`:136-137`); `check_result` is `assert` (`src/include/pinyin_utils.h:27-31`). oxpinyin's KC store reports `StoreError::ReadAfterCheck` (`crates/oxpinyin-store/src/kyotocabinet/ffi.rs`, `get_bigram`); it reaches the engine only through the system bigram. Every row read returns `EngineError::ReadAfterCheck` (`EngineError::row_read`): phrase segmentation (`phrase_lookup.cpp:192`, `:269`), n-best predecessor rows (`phonetic_lookup.h:595`) and the `DYNAMIC_ADJUST` row (`pinyin.cpp:2212`, `zhuyin.cpp:1485`, `:1579`). Both C adapters abort in `refuse_lookup_gram`. No differential can force the fault. Held by `phrase::tests::a_read_failure_after_the_size_check_aborts` in both adapters, `row_read_names_only_the_checked_read_failure` and `a_checked_read_failure_is_the_predecessors_read_after_check_fault`. [Register entry](upstream-divergences.md#pin-aborts-reproduced). |
| 111 | Wrapped phrase-content item extends past content | **(b)** | `SubPhraseIndex::get_phrase_item` builds a header-derived view without checking its full extent (`src/storage/phrase_index.cpp:180-206`). A read past the original item but inside content returns the following bytes and is reproduced. A read that passes the content end answers no item and keeps other changes, as row 107. `UserLibrary::item` and the replay buffer implement this. The wrap comes from `append_offset` (`UserLibrary`, the replay's `Content::append`), which reproduces the content offset narrowing modulo 2^32, including zero → 8, overwritten bytes, aliases and totals. Compaction runs only at end-import, save and mask-out, in the pin's slot order and with its totals (`src/storage/phrase_index.cpp:624–675`, `src/pinyin.cpp:655–659`). Unit tests hold both with a test-only modulus. |
| 112 | Oversized user chunk or journal bulk save | **(b)** | Both saves compact first (`src/pinyin.cpp:1132-1146`, `src/zhuyin.cpp:547-554`). `USER_FILE` `SubPhraseIndex::store` copies compacted content C into a fresh chunk (`src/storage/phrase_index.cpp:365-390`); growth narrows to `int` (`src/include/memory_chunk.h:95-98,290-296`), so the unsafe threshold is C ≥ 2^31. SYSTEM_FILE/DICTIONARY `PhraseIndexLogger::store` copies the J-byte journal (`phrase_index.cpp:271-283`, `phrase_index_logger.h:115-117`); its threshold is J ≥ 2^31. oxpinyin reproduces the pin's operations and filesystem effects up to that copy, then refuses at it: the save stops, keeps earlier filesystem changes, answers `false` and keeps the session dirty. Process survival and kept session state are the class (b) departures. Implemented in `persistence.rs`, `stage_rest` (`PersistenceError::SaveTooLarge`). |
| 113 | <a id="bdb-failed-handles"></a>Berkeley DB handles whose `DB->open` failed print `BDB1565` | **POLICY-ACCEPTED** (external API misuse) | The pin keeps a Berkeley DB handle whose `DB->open` failed and calls it again: the system `attach` paths ([chewing_large_table2_bdb.cpp:87-93](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/chewing_large_table2_bdb.cpp#L87-L93), [phrase_large_table3_bdb.cpp:102-108](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/phrase_large_table3_bdb.cpp#L102-L108), [punct_table_bdb.cpp:64-70](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/punct_table_bdb.cpp#L64-L70), [ngram_bdb.cpp:110-124](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/ngram_bdb.cpp#L110-L124)), `FlexibleBigram::attach` ([flexible_ngram_bdb.h:94-153](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/flexible_ngram_bdb.h#L94-L153)) and the user bigram's in-memory `Bigram::load_db` ([ngram_bdb.cpp:47-57](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/ngram_bdb.cpp#L47-L57)). libdb answers `EINVAL` and prints `BDB1565` (`src/dbinc/db_int.in:902-904`, `src/env/env_method.c:1809-1817`, 5.3.28). Berkeley DB requires `DB->close` after a failed open. oxpinyin closes each failed handle (`crates/oxpinyin-store/src/bdb/mod.rs`: `BdbUnopenedHandle`, `open_pin`, `WriteStore::attach_hash`, `BdbUserBigramDb`; also `crates/oxpinyin-utils/src/storage/dbm.rs`, `crates/oxpinyin-utils/src/k_mixture_model/database.rs`). Every answer, file effect and training abort (`phonetic_lookup.h:904`) stays the pin's; only the `BDB1565` lines are missing. A failed `db_create` aborts with `std::process::abort()`. Affected cases: [Berkeley DB handles whose open failed (policy row 113)](upstream-divergences.md#berkeley-db-handles-whose-open-failed-policy-row-113). Held by `a_failed_in_memory_open_leaves_the_pins_failed_container` and the death tests `a_failed_user_bigram_db_create_aborts_where_load_db_asserts`, `a_failed_db_create_aborts_where_the_pin_asserts` and `a_store_into_the_failed_user_bigram_aborts_training`. |
| 114 | Negative `MemoryChunk::load` payload read | **(b)** | `src/include/memory_chunk.h:452-453` converts a negative `read` result, including `EINTR`, to `guint32` (`0xffffffff`) and checksums that length past a smaller allocation. The tkrzw object's `pinyin::MemoryChunk::load` at offsets `+0x116` to `+0x13b` does the unchecked conversion and checksum loop. The result is undefined. oxpinyin takes the checksum-failure path: it frees the buffer, closes the descriptor and answers false without a retry. The negative-read path is `pin_not_run`; Rust single-read tests cover it and the defined nonnegative short read. |
| 115 | Consumed invalid UTF-8 in family E text rows or phrase-table queries | **(b)** | `tests/storage/test_phrase_table.cpp:45-59` passes failed `g_utf8_to_ucs4` output to search (`src/storage/phrase_large_table3_tkrzwdb.cpp:133-136`); phrase-table text loading also consumes it (`phrase_large_table3.cpp:131-135`). In `FacadePhraseIndex::load_text` the failed conversion leaves `glong written` uninitialized (`phrase_index.cpp:541-543`); `cur_token == 0` or a changed token consumes it in `set_phrase_string(written, NULL)` (`:545-556`), which is undefined. oxpinyin stops those consuming failures with `exit(EINVAL)`, no added diagnostic, and keeps earlier observable work. A same nonzero token never uses the failed conversion (`:577-582`, `:586`); that path is defined, continues and compares against the pin. Undefined paths never run the pin; E3 table cases use `pin_not_run`. |
| 116 | More than sixteen keys passed to the chewing-table test search | **(b)** | `tests/storage/test_chewing_table.cpp:71-86` passes all parsed keys to `src/storage/chewing_large_table2.cpp:161-172`, whose local `ChewingKey index[MAX_PHRASE_LENGTH]` has sixteen slots. `src/storage/pinyin_phrase3.h:160-177` writes phrase_length keys before the switch can abort, a stack-buffer write. After the first incomplete key's zero-tone check (`pinyin_phrase3.h:146-157`), oxpinyin stops before the unsafe copy with `exit(EINVAL)`, no added diagnostic, and keeps earlier observable work. Separate from row 84. `chewing-table-overlong-query` uses `pin_not_run`; the subject guard after a valid timed query checks that earlier stdout is kept. |
| 117 | Logger ignores a failed required phrase-chunk load | **(b)** | The three required `../../data/gb_char.bin` loads (`tests/storage/test_phrase_index_logger.cpp:28-30,45-48,52-54`) ignore `MemoryChunk::load` returning false. The first and third enter `SubPhraseIndex::load`; diff loads the second as its old sub-index (`phrase_index.cpp:271-282,335-354`), reaching row 76's NULL read. oxpinyin stops a failed required load with `exit(ENOENT)`, no added diagnostic, and keeps earlier observable work. The journal reload (`test_phrase_index_logger.cpp:55-57`) is defined on failure (`phrase_index_logger.h:94-133`) and is not refused. `phrase-index-logger-missing` and `phrase-index-logger-short` bind source-only observations; the undefined path is not run against the pin. |
| 118 | Training a token whose library is unloaded writes a stale cached item | **(b)** for a write into the unloaded library's freed chunk; **POLICY-ACCEPTED** (approved divergence) for a write into a library still loaded | After `pinyin_unload_phrase_library` or `zhuyin_unload_phrase_library` deletes a sub-index (`phrase_index.cpp:261-269`), `train_result3` reads the item unchecked (`phonetic_lookup.h:924`); `get_phrase_item` answers `ERROR_NO_SUB_PHRASE_INDEX` and leaves `m_cached_phrase_item` unchanged (`phrase_index.h:649-650`). The call raises that item's reading counts (`:925-927`). A write into the freed chunk is a use-after-free (class (b)). A write into a live library is defined in the pin; oxpinyin declines it under [the unloaded-token pronunciation amendment](#unloaded-token-pronunciation-cache), held by `unloaded-cached-pronunciation-pinyin` and `-zhuyin`. In both cases oxpinyin trains no reading and keeps the rest: the bigram trains (`:875-909`), no total moves (`phrase_index.h:631-632`), and training answers `true`. Predicted rows behave the same (`pinyin.cpp:2609-2639`). Held by `unloaded_library_tests` (`crates/oxpinyin-capi/src/candidates.rs`) and `unloaded` (`crates/oxpinyin-user/src/store_libpinyin/unloaded.rs`). No contract case covers the freed chunk. |
| 119 | Token allocation past the last `USER_FILE` sub-index slot | **pin behaviour reproduced** | `_add_phrase` takes `range.m_range_end` as the new token and bumps only a zero low word (`pinyin.cpp:589-594`); the addon promotion takes the end as is (`:2541-2545`). `get_range` masks the end into the library nibble (`phrase_index.cpp:620`, `novel_types.h:45-46`), and `add_phrase_item` writes the offset word at `token & PHRASE_MASK` (`phrase_index.cpp:208-217`). After slot `PHRASE_MASK`, later adds land on slot 1 and promotions on slot 0, overwriting the offset word and keeping the old item's bytes, index entries and unigram. oxpinyin allocates the same way: `add_phrase_token` (`crates/oxpinyin-user/src/phrase.rs`), `arena_promote` and `UserLibrary::add`; `user_library_token_end` gives the pin's empty export walk (`pinyin.cpp:676-687`). Held by `add_phrase_fills_the_last_slot_then_wraps_to_slot_1` and `promote_addon_phrase_fills_the_last_slot_then_wraps_to_slot_0` (`crates/oxpinyin-user/src/store.rs`). The standalone row store keeps its cursor refusal until its deletion. |
| 120 | `ChewingKey` outside the table dimensions reaches the key string renderers | **pin behaviour reproduced** | The packed key holds an initial in 0..=31, a final in 0..=31 and a tone in 0..=7, against 24 initials, 18 finals and 6 tones. `get_table_index` asserts the initial, middle and final (`chewing_key.cpp:28-32`); every renderer (`pinyin_get_pinyin_string`, `_zhuyin_string`, `_luoma_pinyin_string`, `_secondary_zhuyin_string`, `pinyin_get_pinyin_strings` and the zhuyin twins) calls it before its `0 == get_table_index()` guard. Renderers that print a tone then assert `m_tone < CHEWING_NUMBER_OF_TONES` (`:40`, `:75`, `:92`, `:105`); `get_shengmu_string` and `get_yunmu_string` do not. The C entry points abort at those inputs in the pin's order. Source-derived at `074a2219`. Held by death tests in `matrix_bound_tests.rs` of both C crates. |
| 121 | Choosing an addon candidate when sub-index 5 is missing | **(b)** | The stock `table.conf` names library 5 as `USER_FILE` (`data/table.conf.in:13`), so init loads it or creates it empty (`pinyin.cpp:303-321`). With a custom row 5 of `NOT_USED`, `get_range` returns `ERROR_NO_SUB_PHRASE_INDEX` without writing the range (`phrase_index.cpp:610-613`). The promotion reads the unwritten end as a token (`pinyin.cpp:2541-2545`); its value is undefined. oxpinyin skips promotion and continues selection with the addon candidate. It writes neither index nor an item. Held by `union_e2e_tests::choosing_an_addon_without_sub_index_5_skips_promotion`. |

Totals by the first disposition in each row: **pin behaviour reproduced** 42 (4, 5a, 5c, 5d, 6, 10, 14, 19, 21, 38, 44, 45, 61, 62, 63, 64, 66, 74, 77, 79, 81, 82, 84, 85, 86, 87, 88, 89, 90, 91, 92, 93, 94, 95, 96, 97, 103, 104, 108, 110, 119, 120) · **CLOSED** 44 (3, 5b, 7, 8, 9, 12, 13, 15, 16, 17, 24, 25, 26, 27, 28, 30, 32, 33, 34, 35, 36, 37, 39, 46, 47, 48, 49, 50, 52, 53, 54, 55, 56, 57, 58, 65, 67, 68, 69, 71, 72, 73, 78, 83) · **(b)** 27 (1, 18, 41, 42, 43, 51, 60, 70, 75, 76, 80, 98, 100, 101, 102, 105, 106, 106a, 107, 111, 112, 114, 115, 116, 117, 118, 121) · **(a)** 1 (11) · **no ABI divergence** 4 (2, 23, 29, 31) · **POLICY-ACCEPTED** 2 (59, 113) · **IMPLEMENTED** 1 (109) · **RETIRED** 1 (40) · **WITHDRAWN** 1 (22) · **reattributed** 1 (20) — **124 rows**.
