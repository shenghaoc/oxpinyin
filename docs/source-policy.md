# Source policy

oxpinyin is a Rust re-expression of libpinyin under the same license, GPL-3.0-or-later.
Contributors read libpinyin's own source and re-express its algorithms in idiomatic Rust.
They do not copy the implementation of a library that libpinyin calls.
There is no clean-room restriction.
Clean-room reverse engineering adds time and effort and gives no benefit when the source is legally available.

The [compatibility policy](findings/compatibility-policy.md) owns what may differ from the pin: the decision order, the exception classes (a) and (b), internal language artefacts, external I/O, the maths rule, the standing rulings, the classification of every register entry and the `POLICY-ACCEPTED` disposition.
Where this page sends a difference to a class or a divergence register entry, the compatibility policy decides it.

## Source citations

The pin is cited by file and line at the commit named in [oracle-pin.txt](../tools/oracle/oracle-pin.txt).

## Method

Stages 1 and 2 reproduce the pin in idiomatic Rust.
Internal refactoring may replace how existing required work is represented or performed ([internal implementation](#internal-implementation-and-performance)).
It adds no unapproved capability, independently maintained parallel product model or observable behaviour; the compatibility policy governs permitted differences.
A performance or memory improvement is Stage 2 work only within that limit.
The pin's own code decides what oxpinyin does.
An unapproved difference in required behaviour is corrected by adding or removing oxpinyin code.
oxpinyin's present behaviour is not a reason to retain a difference.
Divergence starts at Stage 3.
Existing registered exceptions remain in force until each is revisited on its own.

The [algorithm reference](findings/libpinyin-structures-and-algorithms.md) describes the pin’s structures, algorithms and complexity.
The PR that changes a recorded structure or algorithm updates it.

Every coefficient of product semantics takes the pin's value: seeds, factors, ceilings, thresholds, beam widths, store widths, epsilons, penalties and discounts.
A choice internal to oxpinyin, such as when to switch between equivalent in-memory representations, is not a coefficient.
The algorithm record holds each value with its pin citation.
A coefficient is never swept, tuned or fitted to make outputs match.

### Internal implementation and performance

A replacement changes how existing required work is represented or performed and supersedes the old implementation.
A different Rust representation, algorithm, ownership strategy or allocation strategy is permitted when it replaces existing required work; the pin need not contain the same field, helper or allocation strategy.
Stages 1 and 2 add no unapproved capability, independently maintained parallel product model or observable behaviour; the compatibility policy governs permitted differences.

Code is removed when it is unused, redundant, introduces an unapproved behavioural difference or capability, or retains complexity whose costs are not justified.
A valid replacement is not removed merely because the pin implements it differently.
Optional retained complexity, such as a cache, must justify itself in memory, execution time or binary size, without compromising compatibility, safety or existing explicit rulings.

A PR records measurements where they already exist, explains the expected resource trade-off, and marks unmeasured effects as unknown.
Systematic measurement is deferred to the end of the phase.

The pin's source defines the required product behaviour.
An external library's documented contract defines what its calls guarantee.
oxpinyin does not reproduce a library's implementation details or rely on consequences of violated preconditions.
Rust ownership, borrowing, lifetimes, types and error handling are implementation choices, not product features, and need no resource justification.
A change to an algorithm or data structure follows the [performance standard](#performance-standard).

Unreleased Rust internals create no independent compatibility obligation.
The pinned C interfaces, behaviour and persistent formats remain binding.

Source-derived complexity is the default performance evidence.
Routine performance-measurement campaigns are not required.

#### Controlling rule

**Performance evidence:** the source is read and the complexity argued first.
A measurement is made only for a concrete unresolved question that affects a decision.
Before a measurement, the question, the arms and inputs relevant to it, and the decision rule are named.

The cheapest sufficient measurement is used: wall time and peak RSS at full speed.
Callgrind or Massif is used only to attribute a gap that a cheaper measurement has already shown, once, on the operation that shows it.
Measurement stops as soon as the answer cannot change the decision.
No fixed number of arms, samples, repetitions, profiles or instrumentation passes is required.

External interface behaviour is preserved.
The target is a smaller binary, faster execution and much lower RAM use than libpinyin.
Binary size follows [constitution §2](constitution.md), separately from time and space comparisons.
Binary size per PR is not complexity: a change can be split into arbitrarily small PRs.

Idiomatic Rust is preferred over a line-by-line translation of C++ written in a C style.
Owned or borrowed slices, enums, `Result`, iterators and standard collections are used as appropriate.
The pin's code shape is not reproduced.
Mutable byte buffers, GLib containers, raw back-pointers and integer status codes can obstruct idiomatic Rust.
They can add copies or unsafe code and reduce the benefits of the compiler and the standard library.
Internal language-artefact differences are recorded instead of imitated.

#### Complexity evidence

A change that alters an algorithm, a data structure, allocation behaviour or an input bound gives a short complexity argument derived from the source.
The argument names the input dimensions and constant bounds.
It bounds the loops, recursion, collection operations, allocations and relevant callees.
It distinguishes worst-case, amortised and expected bounds.
It states assumptions about external libraries.
It compares with the previous code, and with the pin where the two do comparable work.

Where the source does not establish a bound, the argument says so.
An estimate is not presented as a proof.

#### Measurement is the exception

Timing, instruction counts, heap profiles and base/PR/pin campaigns are not required because code changed or might affect performance.
A measurement is made only under the [controlling rule](#controlling-rule), for a concrete unresolved question that affects a decision, such as:

- a question the source cannot settle, including a constant-factor or memory concern;
- an observed regression;
- a numerical performance claim in the PR;
- a maintainer request.

The existing recipes are reused.
New measurement tooling, drivers or instrumentation need the maintainer's approval.
When a measurement compares arms, the arms relevant to the question are measured in one run on one host.
See the [measurement procedure](runbooks/benches.md#measure-compared-arms-in-one-run).

#### Performance standard

Time and space complexity must never both worsen compared with the pin.
A regression on one axis must be traded for a gain on the other, kept as small as possible, and justified in the PR.
Equal asymptotic bounds do not prove equal cost.
Obvious allocation waste, large constants and hot-path work are caught by reading the code, and by a targeted test or assertion where that suffices.
No comparison with the pin is made where the two intentionally do different amounts of work.
In Stages 1 and 2, oxpinyin performs the pin's required work in idiomatic Rust and matches the contracts of the libraries it calls.

#### Unchanged verification

Correctness, C ABI compatibility and persistent-data compatibility stay as they are.
Unit tests, contract differentials, revert-and-check, sanitizers, CI gates and bounded parallel test execution stay as they are.
A test is serialised only for a stated shared-state or resource reason.
Stripped shipping-library sizes and increases over base are reported under constitution §2 ([shipping-library sizes](packaging.md#shipping-library-sizes)).
The sizes come from the `shipping-sizes` CI job summary.

## Compatibility decision order

The compatibility policy's [decision order](findings/compatibility-policy.md#decision-order) decides every proposed difference from the pin, and its [dispositions](findings/compatibility-policy.md#dispositions-and-register-entries) govern how each case is reported.

### Adapter placement

The compatibility policy's [crate-placement rule](findings/compatibility-policy.md#crate-placement) decides which crate holds external I/O, DBM, C ABI and core code.
`oxpinyin-input` exists from stage 1, for library configuration read/write and locale.
All five stages of the [boundary plan](findings/external-io-boundary.md#proposed-crate-and-staged-implementation) are implemented.
The broader user-library audit stays in #1198.

### External I/O

The compatibility policy's [external I/O rule](findings/compatibility-policy.md#external-io) decides every call across the program's edge, including the macro-check rule; the [Rust/C++ interoperability ruling](findings/compatibility-policy.md#rust-cpp-interoperability) governs external-library bindings.
`oxpinyin-input` has no C shim.

#### Platform scope of pinned behaviour

The compatibility policy's [platform scope](findings/compatibility-policy.md#platform-scope) decides where the drop-in and pin-call rules bind: on Linux only.
No Windows libpinyin build project is started.

### Internal language artefacts

The compatibility policy's [internal-language-artefact rule](findings/compatibility-policy.md#internal-language-artefacts) (#1303) decides behaviour outside external I/O, as decision-order limb 3.
A language artefact is not reproduced; the difference is recorded and the work continues.

The ruling [No ported external-library internals](findings/compatibility-policy.md#no-ported-external-library-internals) is in the compatibility policy.
It holds the candidate-frequency comparator, the Zhuyin selection order and the Pinyin two-slot node.
Each observable difference that comes from the replacement of implementation-specific behaviour is supported by evidence and recorded as `POLICY-ACCEPTED`.

**Contract, not implementation.**

The contract guarantees of a language or a library are reproduced, not incidental implementation behaviour.
C guarantees unsigned wrapping arithmetic.
Observable unsigned wrap is reproduced with Rust's `wrapping_*` methods.
Signed overflow has no C contract and falls under class (b).
Documented `g_array_sort` stability is contract.
Equal heap-element order, search results on unsorted input and a float's last bit are not contract guarantees.

**Removal gate.** Before an emulation is removed or replaced, every affected partition is complete,
including every caller of a shared helper, and the proposed code passes the applicable
compatibility gates. A prior entry is reused only after
checking that intervening changes have not invalidated its reasoning.
Real GLib ABI objects/allocations and `GArray`/`g_free` stay.

### Maths

The compatibility policy's [maths rule](findings/compatibility-policy.md#a-maths-platform-dependent-floating-point-accumulation) decides floating-point work: Rust's standard maths is used at full speed, and a last-bit difference is recorded as class (a).
The rule includes logarithms, trigonometric functions and hyperbolic functions.
Any remaining discrepancy is investigated.
It is not automatically classified as a host difference.

Floating point alone is not a Rust language limitation.
Rust's `f32` and `f64` use the same IEEE formats as `gfloat` and `gdouble`.
The product's IEEE-exact operations are written in idiomatic Rust: basic arithmetic, `sqrt`, `abs`, rounding and comparisons.
A difference that comes only from internal C/C++ semantics or a compiler transformation is recorded as a language artefact and not imitated.
Examples are implicit promotions, conversions, storing a float in an integer, and multiply-add contraction.
On Linux, formatted output also uses the pin's external I/O call ([platform scope](#platform-scope-of-pinned-behaviour)).

## Stages

Replacement of existing required work, rather than an added capability, separates Stage 2 from Stage 3; observable compatibility alone does not.

| Property | Stage 2 (current) | Stage 3 (after Stage 2) |
|---|---|---|
| Goal | Replace how existing required work is represented or performed, in idiomatic Rust, without adding a capability | Intentional departures from libpinyin |
| Performance work | Replace existing required work with a better Rust representation or algorithm; add no capability that libpinyin lacks | Added capabilities and intentional departures |
| Internal refactoring | Allowed as replacement of existing required work, not as an added capability | Allowed |
| Observable behaviour | Product behaviour matches under existing exceptions; external I/O uses the pin's calls on Linux ([platform scope](#platform-scope-of-pinned-behaviour)) | Explicit differences permitted |
| Data formats and ABI | Drop-in compatibility stays | An incompatible change requires explicit opt-in and a suitable binary identity |

Stage 1, exact-output parity with the pin-built oracle on the corpus surfaces, is complete.

The language-artefact exception applies to internal behaviour, not to external I/O.
Data formats and ABI do not change.

Older records use "Stage 2" for compatible performance work.
Those records describe the ruling under which the work was done.
Current Stage 2 work adds no unapproved capability, independently maintained parallel product model or observable behaviour; the compatibility policy governs permitted differences.
An unapproved capability or intentional output change belongs to Stage 3.
New benchmark names do not include a stage.

### Stage 2

oxpinyin may replace what libpinyin's own code does with plain, idiomatic Rust.
It preserves the pin's observable behaviour, including its logic defects.
An observable pin defect is recorded as an upstream defect instead of being fixed.
The existing classes and the internal language-artefact exception apply to departures.
The contract of each library call is matched; the library's implementation is never copied.
The real boundary interface is used through the established crate (`cxx`, `bindgen`, `libc`, `glib-sys` or `cargo-c`) under the existing [boundary and platform rules](findings/compatibility-policy.md#external-io).
Code is removed under the [internal implementation](#internal-implementation-and-performance) rule; a valid replacement is not removed merely because the pin implements it differently.
An expectation about the pin's behaviour derives from cited lines of the pin.
A Rust-specific safety or tooling expectation follows its own stated contract.
A test of behaviour that survives a removal is kept, and only its fixtures are adjusted.
A test that exists only to exercise removed behaviour is removed with it.
Observable behaviour and persistent-data compatibility are verified.

Pin source and, where necessary, disassembly establish the expected behaviour. Oracle differentials verify that behaviour. If the equivalence of a Rust replacement cannot be established, use the pin’s external I/O call. The maintainer decides unresolved questions about the surrounding pin behaviour.

A change that alters an algorithm, a data structure, allocation behaviour or an input bound gives a short complexity argument derived from the source ([complexity evidence](#complexity-evidence)).
Measurement is the exception ([controlling rule](#controlling-rule)).
An implementation that is worse on both time and space is fixed.
A faster Rust expression of what the pin's code does needs no artificial performance ceiling.
Dictionary cursor walks for lattice building, incremental lattices and new per-session arenas are reserved for Stage 3 by roadmap decision ([ROADMAP.md, Stage 3 items 4–6](../ROADMAP.md#stage-3-after-stage-2)), independent of the replacement rule.
A mapping change must preserve the pin's external call ([external I/O](findings/compatibility-policy.md#external-io)).

### Stage 3

Stage 3 starts after the Stage 2 exit gate, the third bug-for-bug sweep in the [roadmap](../ROADMAP.md).

Each intentional departure sits behind an opt-in cargo feature, off by default.
Examples include new language models, different candidate behaviour and incompatible data formats.
Each departure states whether it changes observable output, files, the ABI or more than one of these.

The default build stays bug-for-bug compatible under the recorded exceptions.
Each default-build differential keeps its Stage 2 disposition: `MATCH`, `POLICY-ACCEPTED` or `HOST-DEPENDENT`.
Default-build files do not change.

A build that breaks libpinyin's ABI or data formats does not claim libpinyin's binary identity, unless a maintainer ruling permits it.
That identity is SONAME `libpinyin.so.15`, `libpinyin.pc` and the `libpinyin-<version>` header directory.
A departure that changes only output states the departure in its build metadata.

A departure uses only libpinyin's three backends, one for each build.
It does not add a backend.

## Pinned behaviour and oxpinyin improvements

Every requirement, SPEC line and finding is classified on exactly one side:

- **Pinned behaviour:** the pin's SONAME, header, pkg-config, exported I/O, file formats, stderr bytes, process-wide side effects and defects.
  This is the Stage 2 target.
- **oxpinyin improvements:** class (a)–(b), standing divergences, recorded language artefacts and constitution rules.
  This side also includes internal architecture, crate decoupling, performance and Stage 3 departures.

An improvement with no observable change is permitted in Stage 2 only as a replacement of existing required work, not as an added capability.
An observable change requires class (a)–(b), a recorded internal language artefact, or a Stage 3 opt-in feature.
An internal language artefact cannot change external I/O, the ABI or files on disk.
The default build stays bug-for-bug compatible under the recorded exceptions.

The compatibility policy states the [drop-in goal](findings/compatibility-policy.md#goal-a-bug-for-bug-drop-in) and the [backend rules](findings/compatibility-policy.md#backends-and-user-data).
The [default backend](runbooks/backends.md) is the reference cell.
Two decisions are settled: the choice of tkrzw as the default backend, and the fresh start for both implementations after a backend switch.
Neither decision is reopened as an unresolved trade-off.
