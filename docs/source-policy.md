# Source policy

oxpinyin is a Rust re-expression of libpinyin under the same license, GPL-3.0-or-later.
The project expects and encourages contributors to read and copy the upstream C++ source.
There is no clean-room restriction.
Clean-room reverse engineering adds time and effort and gives no benefit when the source is legally available.

The [compatibility policy](findings/compatibility-policy.md) owns what may differ from the pin: the decision order, the exception classes (a) and (b), internal language artefacts, external I/O, the maths rule, the standing rulings, the classification of every register entry and the `POLICY-ACCEPTED` disposition.
Where this page sends a difference to a class or a discrepancy record, the compatibility policy decides it.

## Source citations

Read upstream source from a checkout at the pinned commit.
As an alternative, use a clone at the pin after you verify the hashes of the cited blobs against it.
Identify the tree that you read.
The Linux host convention is `~/Documents/repos/libpinyin`; the repository does not guarantee that path.

Cite oxpinyin code by file and item name, never by line number.
Cite the pin by file:line at 074a2219.
Dated records keep the line numbers of the tree they name.

## Method

Maintainer ruling 2026-10-16 UTC.

Stage 2 requires compatibility with the pin.
It permits internal refactoring, idiomatic Rust rewrites, and performance or memory improvements that keep compatibility intact.
Compatibility does not require you to defer internal improvements until after release.
An improvement does not close, excuse or reclassify an unrelated compatibility gap.
A correction of an oxpinyin defect towards the pin's output is Stage 2 work.

For each subsystem:

1. Record the pin's structures and algorithms in the [algorithm record](findings/libpinyin-structures-and-algorithms.md).
   Include layout, constants, tie orders and every quirk that reaches output.
   State time and space complexity.
   Cite each item as `file:line` at `074a2219`.
2. Design and analyse oxpinyin's structures and algorithms.
   Explain how they preserve the pin's product behaviour, including tie orders and product quirks.
   Apply the separate rules for [internal language artefacts](findings/compatibility-policy.md#internal-language-artefacts) and [external I/O](findings/compatibility-policy.md#external-io).
   Compare time and space complexity with the pin.
3. Implement the behaviour in natural, efficient Rust across loosely coupled crates.
4. Verify it with the established oracle differentials.
5. Record each policy-approved departure.

Keep the algorithm record current, also for work that predates this ruling.
Update it in the same PR that changes a recorded structure or algorithm.

Use the pin's values for every coefficient: seeds, factors, ceilings, thresholds, beam widths, store widths, epsilons, penalties and discounts.
Record each value with its pin citation in the algorithm record.
Never sweep, tune or fit a coefficient to make outputs match.

### Internal implementation and performance

**Maintainer decision 2026-11-01 UTC.** Source-derived complexity is the default performance evidence.
Routine performance-measurement campaigns are not required.

#### Controlling rule

**Performance evidence:** read the source and argue the complexity first.
Measure only for a concrete unresolved question that affects a decision.
Before you measure, name the question, the arms and inputs relevant to it, and the decision rule.

Use the cheapest sufficient measurement: wall time and peak RSS at full speed.
Use Callgrind or Massif only to attribute a gap that a cheaper measurement has already shown, once, on the operation that shows it.
Stop as soon as the answer cannot change the decision.
No fixed number of arms, samples, repetitions, profiles or instrumentation passes is required.

Preserve external interface behaviour.
The target is a smaller binary, faster execution and much lower RAM use than libpinyin.
Internal implementation freedom serves that target.
Binary size follows [constitution §2](constitution.md), separately from time and space comparisons.
Binary size per PR is not complexity: a change can be split into arbitrarily small PRs.

Prefer idiomatic Rust over a line-by-line translation of C++ written in a C style.
Use owned or borrowed slices, enums, `Result`, iterators and standard collections as appropriate.
Do not reproduce the pin's code shape.
Mutable byte buffers, GLib containers, raw back-pointers and integer status codes can obstruct idiomatic Rust.
They can add copies or unsafe code and reduce the benefits of the compiler and the standard library.
Record internal language-artefact differences instead of imitating them.

#### Complexity evidence

A change that alters an algorithm, a data structure, allocation behaviour or an input bound gives a short complexity argument derived from the source.
Name the input dimensions and constant bounds.
Bound the loops, recursion, collection operations, allocations and relevant callees.
Distinguish worst-case, amortised and expected bounds.
State assumptions about external libraries.
Compare with the previous code, and with the pin where the two do comparable work.

If the source does not establish a bound, say so.
Do not present an estimate as a proof.

#### Measurement is the exception

Do not require timing, instruction counts, heap profiles or base/PR/pin campaigns because code changed or might affect performance.
Measure only under the [controlling rule](#controlling-rule), for a concrete unresolved question that affects a decision, such as:

- a question the source cannot settle, including a constant-factor or memory concern;
- an observed regression;
- a numerical performance claim in the PR;
- a maintainer request.

Reuse the existing recipes.
New measurement tooling, drivers or instrumentation need the maintainer's approval.
When a measurement compares arms, measure the arms relevant to the question in one run on one host.
See the [measurement procedure](runbooks/benches.md#measure-compared-arms-in-one-run).

#### Performance standard

Time and space complexity must never both worsen compared with the pin.
A regression on one axis must be traded for a gain on the other, kept as small as possible, and justified in the PR.
Equal asymptotic bounds do not prove equal cost.
Catch obvious allocation waste, large constants and hot-path work by reading the code, and by a targeted test or assertion where that suffices.
Do not compare against the pin where the two intentionally do different amounts of work.
The pin's algorithm is the first candidate, not a mandatory choice.

#### Unchanged verification

Correctness, C ABI compatibility and persistent-data compatibility stay as they are.
Unit tests, contract differentials, revert-and-check, sanitizers, CI gates and bounded parallel test execution stay as they are.
Serialise a test only for a stated shared-state or resource reason.
Report stripped shipping-library sizes and increases over base under constitution §2 ([shipping-library sizes](packaging.md#shipping-library-sizes)).
Take the sizes from the `shipping-sizes` CI job summary.

## Compatibility decision order

The compatibility policy's [decision order](findings/compatibility-policy.md#decision-order) decides every proposed difference from the pin, and its [dispositions](findings/compatibility-policy.md#dispositions-and-discrepancy-records) govern how each case is reported.
Read the compatibility policy before you record a divergence.

### Adapter placement

The compatibility policy's [crate-placement rule](findings/compatibility-policy.md#crate-placement) decides which crate holds external I/O, DBM, C ABI and core code.
`oxpinyin-input` exists from stage 1, for library configuration read/write and locale.
The [boundary plan](findings/external-io-boundary.md) lists later stages.
The broader user-library audit stays in #1198.

### External I/O

The compatibility policy's [external I/O rule](findings/compatibility-policy.md#external-io) decides every call across the program's edge, including the macro-check rule; the [Rust/C++ interoperability ruling](findings/compatibility-policy.md#rust-cpp-interoperability) governs external-library bindings.
`oxpinyin-input` has no C shim.

#### Platform scope of pinned behaviour

The compatibility policy's [platform scope](findings/compatibility-policy.md#platform-scope) decides where the drop-in and pin-call rules bind: on Linux only.
Do not start a Windows libpinyin build project.

### Internal language artefacts

The compatibility policy's [internal-language-artefact rule](findings/compatibility-policy.md#internal-language-artefacts) (#1303) decides behaviour outside external I/O, as decision-order limb 3.
A language artefact is not reproduced; record the difference and continue the work.

The ruling [No ported external-library internals](findings/compatibility-policy.md#no-ported-external-library-internals) (maintainer ruling 2026-10-29 UTC) is in the compatibility policy.
It holds the candidate-frequency comparator, the Zhuyin selection order and the Pinyin two-slot node.
Supply evidence for each observable difference that comes from the replacement of implementation-specific behaviour, and record it as `POLICY-ACCEPTED`.

**Contract, not implementation.** (Maintainer decision 2026-10-29 UTC.)

Reproduce the contract guarantees of a language or a library, not incidental implementation behaviour.
C guarantees unsigned wrapping arithmetic.
Reproduce observable unsigned wrap with Rust's `wrapping_*` methods.
Signed overflow has no C contract and falls under class (b).
Documented `g_array_sort` stability is contract.
Equal heap-element order, search results on unsorted input and a float's last bit are not contract guarantees.

### Maths

The compatibility policy's [maths rule](findings/compatibility-policy.md#a-maths-platform-dependent-floating-point-accumulation) decides floating-point work: use Rust's standard maths at full speed, and record a last-bit difference as class (a).
The rule includes logarithms, trigonometric functions and hyperbolic functions.
Investigate any remaining discrepancy.
Do not automatically classify it as a host difference.

Floating point alone is not a Rust language limitation.
Rust's `f32` and `f64` use the same IEEE formats as `gfloat` and `gdouble`.
Write the product's IEEE-exact operations in idiomatic Rust: basic arithmetic, `sqrt`, `abs`, rounding and comparisons.
Record a difference that comes only from internal C/C++ semantics or a compiler transformation as a language artefact, and do not imitate it.
Examples are implicit promotions, conversions, storing a float in an integer, and multiply-add contraction.
On Linux, formatted output also uses the pin's external I/O call ([platform scope](#platform-scope-of-pinned-behaviour)).

## Stages

Maintainer ruling 2026-10-14 UTC.

Compatibility, not performance, separates Stage 2 from Stage 3.

| Property | Stage 2 (current) | Stage 3 (after Stage 2) |
|---|---|---|
| Goal | Bug-for-bug compatibility with internal improvements | Intentional departures from libpinyin |
| Performance work | Allowed, with no limit on improvement over the pin | Allowed when it requires a compatibility-breaking change |
| Internal refactoring | Allowed | Allowed |
| Observable behaviour | Match product behaviour under existing exceptions; use the pin's external I/O calls on Linux ([platform scope](#platform-scope-of-pinned-behaviour)) | Explicit differences permitted |
| Data formats and ABI | Keep drop-in compatibility | Require explicit opt-in and a suitable binary identity for an incompatible change |

Stage 1, exact-output parity with the pin-built oracle on the corpus surfaces, is complete.

The language-artefact exception applies to internal behaviour, not to external I/O.
Data formats and ABI do not change.

Older records use "Stage 2" for compatible performance work.
That work stays Stage 2 under this ruling; keep the wording of those records.
Where such a record refers to an output change, such as `EdgeKind` Fuzzy/Typo/Abbrev variants or a model upgrade, that change belongs to Stage 3.
Give new benchmarks names that do not include a stage.

### Stage 2

Reproduce libpinyin's observable behaviour, including its logic defects.
Record an observable pin defect as an upstream defect instead of fixing it.
Apply the existing classes and the internal language-artefact exception to departures.
Use the pin's calls for external I/O on Linux ([platform scope](#platform-scope-of-pinned-behaviour)).
Improve performance, memory use, safety and internal architecture when compatibility stays intact.
Verify observable behaviour and persistent-data compatibility.

The target is what the pin does, not what it should do.
Derive behaviour by reading the pin, never by running experiments:

1. Read source at the commit in [`tools/oracle/oracle-pin.txt`](../tools/oracle/oracle-pin.txt).
   Read the called GLib, libc and DBM functions when they determine the behaviour.
2. Read the disassembly of the relevant pin-built object when source does not settle the question.
   This includes unresolved macros, templates, inline helpers, implementation-defined behaviour, undefined behaviour and compiler-selected evaluation order.
   Use objects from [`tools/oracle/build.py`](../tools/oracle/build.py).
   Use the [default reference-cell build](runbooks/backends.md).
   Use `--dbm bdb|kc` for backend-specific code compiled only into that backend's build.
   Use `libzhuyin.so.15` from an `--enable-libzhuyin` build for the Zhuyin facade.
   Autoconf's default `-g -O2` retains line tables.
   Use `objdump -dlS -C --no-show-raw-insn` or `gdb -batch -ex 'disassemble /s <symbol>'` for interleaved source.
   Read object tables with `readelf`, `.rodata` or relocations when the question concerns contents rather than execution.
3. Cite `path:line` at the pin, or the symbol and instruction offsets, in the report.

Do not use exploratory probes, throwaway drivers or tests to discover a pin return value.
Do not tune code against the oracle until outputs match.
A differential verifies behaviour already derived from source.
When a differential contradicts the derivation, read the source again.

Use the pin's I/O call if reading cannot prove a Rust operation equivalent.
Stop if surrounding pin behaviour remains unsettled.
Report what you read and where reading stopped resolving the question.
Keep the confirmation questions open in the boundary finding.

Give a short complexity argument derived from the source for a change that alters an algorithm, a data structure, allocation behaviour or an input bound ([complexity evidence](#complexity-evidence)).
Measurement is the exception ([controlling rule](#controlling-rule)).
Fix an implementation that is worse on both time and space.
There is no performance ceiling when output, files and ABI stay compatible.
Examples include `memchr` substring search (#858), dictionary cursor walks and per-session arenas.
A mapping change must preserve the pin's external call ([external I/O](findings/compatibility-policy.md#external-io)).

### Stage 3

Stage 3 starts after the Stage 2 exit gate, the third bug-for-bug sweep in the [roadmap](../ROADMAP.md).

Put each intentional departure behind an opt-in cargo feature, off by default.
Examples include new language models, different candidate behaviour and incompatible data formats.
State whether each departure changes observable output, files, the ABI or more than one of these.

Keep the default build bug-for-bug compatible under the recorded exceptions.
Each default-build differential keeps its Stage 2 disposition: `MATCH`, `POLICY-ACCEPTED` or `HOST-DEPENDENT`.
Default-build files do not change.

Do not claim libpinyin's binary identity for a build that breaks its ABI or data formats, unless a maintainer ruling permits it.
That identity is SONAME `libpinyin.so.15`, `libpinyin.pc` and the `libpinyin-<version>` header directory.
A departure that changes only output states the departure in its build metadata.

A departure uses only libpinyin's three backends, one for each build.
It does not add a backend.

## Pinned behaviour and oxpinyin improvements

Classify every requirement, SPEC line and finding on exactly one side:

- **Pinned behaviour:** the pin's SONAME, header, pkg-config, exported I/O, file formats, stderr bytes, process-wide side effects and defects.
  This is the Stage 2 target.
- **oxpinyin improvements:** class (a)–(b), standing divergences, recorded language artefacts and constitution rules.
  This side also includes internal architecture, crate decoupling, performance and Stage 3 departures.

An improvement with no observable change is internal and permitted in Stage 2, including performance work.
An observable change requires class (a)–(b), a recorded internal language artefact, or a Stage 3 opt-in feature.
An internal language artefact cannot change external I/O, the ABI or files on disk.
The default build stays bug-for-bug compatible under the recorded exceptions.

The compatibility policy states the [drop-in goal](findings/compatibility-policy.md#goal-a-bug-for-bug-drop-in) and the [backend rules](findings/compatibility-policy.md#backends-and-user-data).
The [default backend](runbooks/backends.md) is the reference cell.
Two decisions are settled: the choice of tkrzw as the default backend (maintainer decision 2026-10-30 UTC), and the fresh start for both implementations after a backend switch (maintainer ruling 2026-09-09).
Do not reopen either decision as an unresolved trade-off.
