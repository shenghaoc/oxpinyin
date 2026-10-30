# Source policy

oxpinyin is a Rust rewrite of libpinyin under the same license, GPL-3.0-or-later.
Reading and copying upstream C++ source is expected and encouraged.
There is no clean-room restriction.
The original restriction avoided verbatim copying.
Clean-room reverse engineering adds time and effort without benefit when the source is legally available.

This page owns how contributors work against the pin: reading and citing its source, the method for each subsystem, the decision order for language and runtime differences, and the stages.
The [compatibility policy](findings/compatibility-policy.md) owns what may diverge from the pin: the exception classes (a)–(c), the standing divergences, the classification of every register entry and the `POLICY-ACCEPTED` disposition.
Where this page sends a difference to a class or a discrepancy record, the compatibility policy decides it.

## Source citations

Read upstream source from a checkout at the pinned commit.
Alternatively, use a clone at the pin after verifying the cited blobs' hashes against it.
Identify the tree that you read.
The Linux host convention is `~/Documents/repos/libpinyin`.
The repository does not guarantee that path.

Cite oxpinyin code by file and item name, never by line number; cite the pin by file:line at 074a2219.
Dated records keep the line numbers of the tree they name.

## Method

Maintainer ruling 2026-10-16 UTC.

Amended 2026-10-18 UTC: binary size follows Constitution §2's recording
and disclosure rule, separately from runtime time/space comparisons.

Stage 2 requires compatibility with the pin.
It permits internal refactoring, idiomatic Rust rewrites, and performance or memory improvements when compatibility remains intact.
Compatibility does not require deferring internal improvements until after release.
An improvement does not close, excuse or reclassify an unrelated compatibility gap.
Correcting an oxpinyin defect towards the pin's output is Stage 2 work.

For each subsystem:

1. Record the pin's structures and algorithms in the [algorithm record](findings/libpinyin-structures-and-algorithms.md).
   Include layout, constants, tie orders and every quirk that reaches output.
   State time and space complexity.
   Cite each item as `file:line` at `074a2219`.
2. Design and analyse oxpinyin's structures and algorithms.
   Explain how they preserve the pin's product behaviour, including tie orders and product quirks.
   Apply the separate rules for internal language artefacts and external I/O.
   Compare time and space complexity with the pin.
3. Implement the behaviour in natural, efficient Rust across loosely coupled crates.
4. Verify it with the established oracle differentials.
5. Record each policy-approved departure.

Keep the algorithm record current for work that predates this ruling too.
Use the pin's values for every coefficient.
These include seeds, factors, ceilings, thresholds, beam widths, store widths, epsilons, penalties and discounts.
Record each value with its pin citation in the algorithm record.
Never sweep, tune or fit a coefficient to make outputs match.

### Internal implementation and performance

Preserve external interface behaviour.
Do not worsen time and space complexity together.
A regression in one requires a gain in the other.
Minimize the regression and justify the trade in the change's report.
Binary size per PR is not complexity: a change can be split into arbitrarily small PRs.
Apply [constitution §2](constitution.md) to binary size separately.
The target is a smaller binary, faster execution and much lower RAM than libpinyin.
Internal implementation freedom serves that target.

Prefer idiomatic Rust over a line-by-line translation of C++ written in a C style.
Use owned or borrowed slices, enums, `Result`, iterators and standard collections as appropriate.
Do not reproduce the pin's code shape.
Mutable byte buffers, GLib containers, raw back-pointers and integer status codes can obstruct idiomatic Rust.
They can add copies or unsafe code and reduce compiler and standard-library benefits.
Record internal language-artefact differences instead of imitating them.
Use the pin's external I/O call unless source proves a Rust equivalent for every input.

When an operation is measurably slower or larger than the pin:

1. Try the pin's structure or algorithm in idiomatic Rust first.
2. Measure base, PR and pin together on one host, with repetitions within that run.
3. Assess both time and peak memory.
4. Keep a version that is never worse than the pin in both runtime time and space.
5. Find or retain an idiomatic alternative if the pin's algorithm gives no improvement on those measures.
6. Prefer the idiomatic version on a tie.
7. Record binary file sizes and report increases over base under constitution §2.

See the [measurement procedure](runbooks/benches.md).
The pin's algorithm is the first candidate, not a mandatory choice.

## Compatibility decision order

Read the [compatibility policy](findings/compatibility-policy.md) before recording a divergence.
It defines class (a)–(c), the language/runtime-quirk exception, citations and register classifications.
Class (d), consumer scope, was retired 2026-09-06.

Maintainer rulings, 2026-10-24 UTC, as amended 2026-10-28 UTC: internal
behaviour follows the language-artefact decision order below.

External I/O follows the pin's call.
Use Rust's `std` maths at full speed under class (a).
Do not add specialised maths dependencies, such as SIMD crates.

1. **Preserve the C-facing contract.** Preserve the C ABI, layout, ownership and lifetime exactly.
   This includes real `GArray`, `g_free`, and every other GLib-owned structure and allocation.
   Preserve every exported symbol, system and user file format, and data ownership.
   Preserve observable product behaviour except for class (a)–(c) or the internal language-artefact exception.
2. **Apply class (a)–(c).** Classify each applicable difference in its class.
   Follow that class's rules.
3. **Apply the external-I/O and internal-language rules below.**
4. **Escalate product-level changes.** Reproduce product behaviour bug for bug.
   Ranking rules, limits, refusals, option meanings and libpinyin logic defects are product behaviour.
   Changing them requires a maintainer ruling or a Stage 3 feature.
5. **Revert any other difference as a defect.**
6. **Preserve earlier maintainer rulings.** This exception cannot reverse them.
   `LC_NUMERIC` (policy row 39, #923) requires a new human ruling before it changes.

Report an intentional difference under a still-applicable recorded exception as `POLICY-ACCEPTED`, never as `MATCH`.
Retain its case and pin evidence.
Existing case dispositions remain governed by the [disposition definitions](findings/compatibility-policy.md#discrepancy-records-and-the-policy-accepted-disposition).

### Adapter placement

**Adapter placement (amended 2026-10-28 UTC).**

Place external file, locale and tool I/O calls in `oxpinyin-input`.
Place DBM backend calls in `oxpinyin-store`.
Keep C ABI marshalling and C-facing encodings in `oxpinyin-capi`, `oxpinyin-zhuyin-capi` and `oxpinyin-capi-marshal`.
`oxpinyin-input` exists from stage 1, for library configuration read/write and locale.
The [boundary plan](findings/external-io-boundary.md) lists later stages.

Core and engine types carry only the stored length, outward words and full stored words required by the pin.
The length and outward words serve ranking, comparison, sentence, export and token-getter paths.
Store and table maintenance require the full stored words.
Preserve all characters in general Rust conversions.
NUL-containing text becomes Stored, with its full declared character count and first-zero outward cut.
`phrase_text_bytes` is the explicit C-boundary encoder.
The broader user-library audit stays in #1198.

### External I/O

**External I/O uses the pin's calls (maintainer ruling 2026-10-28
UTC).**

Use the pin's call for every library and tool call across the program's edge.
Use the host C library for ISO C and POSIX calls.
Use GLib for GLib calls.
Use a tool-only C++ shim for C++ streams.
Permit a Rust equivalent only after reading both implementations' sources proves identical observable results for every input.
Use the pin's call when the comparison is unsettled.
This replaces #1304's external-I/O language-artefact treatment.

The call rule does not require an undefined read or abort.
Existing class (b)/(c) records stand, including punctuation and assertion refusals.
`LC_NUMERIC` row 39 stands.
Row 109's mapping call follows the pin.
Its post-fini residual mapping remains open.

### Internal language artefacts

**Internal language artefacts (2026-10-26 UTC, #1303).**

Outside external I/O, the programming language is an implementation detail.
The behaviour is what a user or developer observes when calling the shared object.
Implement that behaviour in idiomatic Rust.
Do not reproduce another programming language's behaviour or that of a library written for it.
This includes C/C++ semantics, GLib, glibc, other libc implementations, libstdc++, compilers and processors.
Examples include integer-returning comparators, float-to-integer truncation and an integer width chosen for storage.

A language artefact is behaviour that a language-neutral requirement would not mention.
Widths or wrapping required by the C ABI, file formats or domain algorithms are contract or product behaviour.
Checksums and hashes are examples of domain algorithms.
An operation's origin in C does not make every use a language artefact.
No further work is needed when idiomatic Rust produces the pin's result.

If valid or malformed input differs only through an internal language artefact:

1. Keep the Rust result.
2. Create a [discrepancy record](findings/language-runtime-discrepancy-template.md).
3. Register it by name in the [divergence register](findings/upstream-divergences.md).
4. Mark distinguishing cases `POLICY-ACCEPTED`.
5. Continue the work.

No approval is needed for this recorded difference.
Do not write Rust to imitate the artefact.
When classification is uncertain, prefer idiomatic Rust and record the difference.
This rule never permits a different external I/O call or result.

**Contract, not implementation.** (Maintainer decision 2026-10-29 UTC.)

Reproduce language and library contract guarantees, not incidental implementation behaviour.
C guarantees unsigned wrapping arithmetic.
Reproduce observable unsigned wrap with Rust's `wrapping_*` methods.
Signed overflow has no C contract and falls under class (b).
Documented `g_array_sort` stability is contract.
Equal heap-element order, search results on unsorted input and a float's last bit are not contract guarantees.

**No ported external-library internals (maintainer ruling
2026-10-29 UTC).**

Implement libpinyin's own algorithms.
Do not transcribe internal algorithms from GLib, libstdc++, libc++, glibc or another external library.
Do not preserve their implementation-specific comparison schedules, probe sequences or other internal details.

- Use the Rust standard-library API with the corresponding documented semantics.
  Keep necessary representation adapters and libpinyin-specific product logic as ordinary Rust code.
  Obtain maintainer approval for a new dependency.
  This ruling authorizes none.
  Reproduce the exact result when the pin uses a library routine within its documented preconditions.
  Choose the predicate, comparator, key and tie-break accordingly.
  Only results that depend on violated preconditions may differ.
  Record each such difference as `POLICY-ACCEPTED`.
- Preserve product rules, ranking keys, limits, C ABI, ownership and persistent formats.
  Only specifically recorded departures under this ruling or another existing ruling are exceptions.
  Supply evidence for observable differences caused by replacing implementation-specific behaviour.
  Record them as `POLICY-ACCEPTED`.
- Give sorting and selection comparisons a consistent ordering.
  Specify special-value and tie treatment where necessary.
  Keep `partition_point` predicates partitioned on valid data.
  Do not silently sort or clean malformed stored data to force that condition.
  Record resulting differences.
  Preserve memory safety and existing refusal rules.
- **Candidate-frequency sorting (maintainer decision 2026-10-29 UTC).**
  Reproduce libpinyin's frequency comparator with Rust's wrapping arithmetic.
  Only lists on which the comparator is cyclic may differ.
- Keep Zhuyin selection length-first.
  At equal length, use descending raw score, with NaN last and signed zeros equal.
  Use earlier batch position as the next tie-break.
  Keep its single-slot admission rule unchanged.
- Preserve front eviction and physical-slot semantics in Pinyin's two-slot node.
- Treat ABI-prescribed encodings as boundary code, not external-algorithm ports.
  Preserve the existing C ABI and external-I/O policies.
  This ruling does not authorize an exhaustive search for unrelated upstream defects.
  See the [external-library inventory](findings/glib-libc-inventory.md).

### Maths

**Math rule (maintainer direction 2026-10-19 UTC).**

Use Rust's standard maths at full speed: `f64::ln`, `exp`, `powf` and every other `std` float method.
This includes logarithms, trigonometric functions and hyperbolic functions.
Do not slow code to imitate glibc, libm, GLib `gfloat` or other C behaviour.
Record a last-bit difference caused by Rust's maths as class (a).
Do not chase it.
Investigate any remaining discrepancy.
Do not automatically classify it as a host difference.

Floating point alone is not a Rust language limitation.
Rust's `f32` and `f64` use the same IEEE formats as `gfloat` and `gdouble`.
Write the product's IEEE-exact operations in idiomatic Rust.
These include basic arithmetic, `sqrt`, `abs`, rounding and comparisons.
Record differences caused only by internal C/C++ semantics or compiler transformations as language artefacts.
Examples include implicit promotions, conversions, storing a float in an integer, and multiply-add contraction.
Do not imitate them.
Use the pin's external I/O call, including for formatted output.

## Stages

Maintainer ruling 2026-10-14 UTC.

Compatibility separates Stage 2 from Stage 3.
Performance does not define that boundary.

| Property | Stage 2 (current) | Stage 3 (after Stage 2) |
|---|---|---|
| Goal | Bug-for-bug compatibility with internal improvements | Intentional departures from libpinyin |
| Performance work | Allowed, with no limit on improvement over the pin | Allowed when it requires a compatibility-breaking change |
| Internal refactoring | Allowed | Allowed |
| Observable behaviour | Match product behaviour under existing exceptions; use the pin's external I/O calls | Explicit differences permitted |
| Data formats and ABI | Keep drop-in compatibility | Require explicit opt-in and a suitable binary identity for an incompatible change |

Stage 1, exact-output parity with the pin-built oracle on the corpus surfaces, is complete.

Amended 2026-10-28 UTC: the language-artefact exception remains for
internal behaviour but no longer applies to external I/O. Data formats
and ABI remain unchanged.

Earlier records use "Stage 2" for compatible performance work, including the `stage2` bench and `docs/perf/` records.
That work remains Stage 2 under this ruling.
Keep those records' wording.
References to output changes, such as `EdgeKind` Fuzzy/Typo/Abbrev variants or a model upgrade, now belong to Stage 3.
Give new benchmarks names that do not include a stage.

### Stage 2

Reproduce libpinyin's observable behaviour, including its logic defects.
Record an observable pin defect as an upstream defect instead of fixing it.
Apply the existing classes and internal language-artefact exception to departures.
Use the pin's calls for external I/O.
Improve performance, memory use, safety and internal architecture when compatibility remains intact.
Verify observable behaviour and persistent-data compatibility.
The target is what the pin does, not what it should do.

Derive behaviour by reading the pin, never by running experiments:

1. Read source at the commit in [`tools/oracle/oracle-pin.txt`](../tools/oracle/oracle-pin.txt).
   Read the called GLib, libc and DBM functions when they determine the behaviour.
2. Read the relevant pin-built object's disassembly when source does not settle the question.
   This includes unresolved macros, templates, inline helpers, implementation-defined behaviour, undefined behaviour and compiler-selected evaluation order.
   Use objects from [`tools/oracle/build-oracle.sh`](../tools/oracle/build-oracle.sh).
   Use the Berkeley DB reference-cell build by default.
   Use `--dbm kc|tkrzw` for backend-specific code compiled only into that backend's build.
   Use `libzhuyin.so.15` from an `--enable-libzhuyin` build for the Zhuyin facade.
   Autoconf's default `-g -O2` retains line tables.
   Use `objdump -dlS -C --no-show-raw-insn` or `gdb -batch -ex 'disassemble /s <symbol>'` for interleaved source.
   Read object tables with `readelf`, `.rodata` or relocations when the question concerns contents rather than execution.
3. Cite `path:line` at the pin, or the symbol and instruction offsets, in the report.

Do not use exploratory probes, throwaway drivers or tests to discover a pin return value.
Do not tune code against the oracle until outputs match.
A differential verifies behaviour already derived from source.
Use the pin's I/O call if reading cannot prove a Rust operation equivalent.
Stop if surrounding pin behaviour remains unsettled.
Report what you read and where reading stopped resolving the question.
Keep the confirmation questions open in the boundary finding.

Measure time, instructions and peak heap on named workloads against the pin (round-2 audit axis L; #757).
Fix an implementation that is worse on both time and space.
There is no performance ceiling when output, files and ABI remain compatible.
Examples include `memchr` substring search (#858), dictionary cursor walks and per-session arenas.
A mapping change must preserve the pin's external call under the 2026-10-28 ruling.

### Stage 3

Put each intentional departure behind an opt-in cargo feature, off by default.
Examples include new language models, different candidate behaviour and incompatible data formats.
Keep the default build bug-for-bug compatible under the recorded exceptions.
Do not claim libpinyin's binary identity for a build that breaks its ABI or data formats.

## Pinned behaviour and oxpinyin improvements

Classify every requirement, SPEC line and finding on exactly one side:

- **Pinned behaviour:** the pin's SONAME, header, pkg-config, exported I/O, file formats, stderr bytes, process-wide side effects and defects.
  This is the Stage 2 target.
- **oxpinyin improvements:** class (a)–(c), class (c)'s required warning, standing divergences, recorded language artefacts and constitution rules.
  This side also includes internal architecture, crate decoupling, performance and Stage 3 departures.

An improvement with no observable change is internal and permitted in Stage 2, including performance work.
An observable change requires class (a)–(c), a recorded internal language artefact, or a Stage 3 opt-in feature.
An internal language artefact cannot change external I/O, the ABI or files on disk.
The default build remains bug-for-bug compatible under the recorded exceptions.

The drop-in goal is `libpinyin.so.15` on the library path with unmodified consumers and existing system data.
Build with exactly one of Berkeley DB, Kyoto Cabinet or tkrzw.
Handle that backend's system and user files natively, as a same-backend libpinyin build does.
Switching backend starts fresh for both implementations (maintainer ruling 2026-09-09).
Do not reopen either decision as an unresolved trade-off.
