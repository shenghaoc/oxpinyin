# Build-recipe provenance audit — which perf records timed the shipping artifact (2026-09-10)

Follow-up to **#401**, filed from the four-cell tree/recipe control on #394
([perf-cycle-ir-differential-2026-09-08.md](perf-cycle-ir-differential-2026-09-08.md)).

## Purpose and non-scope

For every document in this repository carrying oxpinyin performance figures,
this audit records **which build recipe produced the artifact that was timed**,
and therefore whether the figure describes the shipping product or a bisection
fixture.

It is a **survey with a scope verdict per record**. It corrects no figure and it
re-measures nothing — both are out of scope by the issue's own terms. Where a
record is affected, the correction it needs is a re-measurement on its own tree,
which is separate work with separate review. What this audit produces is the
list, the per-record scope statement, and the bound on how much may honestly be
said about the size of the effect.

## The two recipes

`tools/bisection/run-perf-same-data.sh`'s `build_capi` builds the timed oxpinyin
artifact like this:

```sh
cargo build --locked --release -p oxpinyin-capi --no-default-features --features "$backend"
cp "$TARGET/release/libpinyin_capi.so" "$out"
strip --strip-all "$out"
```

The shipping path is `cargo cinstall`:

- `.github/workflows/release-packages.yml:31` — "each distro's own cargo-c
  package runs `cargo cinstall` through tools/packaging/install.sh";
- `tools/packaging/install.sh:240` — `cargo cinstall --prefix=… --libdir=…`;
- `crates/oxpinyin-capi/Cargo.toml`, `[package.metadata.capi.library]`
  `name = "pinyin"` / `version = "15.0.0"` — so the installed file is
  `libpinyin.so.15.0.0`, the drop-in unmodified consumers already record in
  `DT_NEEDED`;
- the drop-in additionally carries `--features shipped`, which the manifest
  documents as "enabled only when building the shipped drop-in artifact".

`cargo build` produces `libpinyin_capi.so`, which nothing installs. A record
that timed it is describing a fixture, and — per the control — one that is
**slower than the artifact that ships**.

## How much may be said, and on which axis

The control's numbers are on **one axis only**: the steady keystroke-cycle
wall-clock ratio against pin-built libpinyin.

| recipe effect `Z÷L − Y÷L` | amd64 | arm64 |
|---|---|---|
| n=8, three windows | −0.0873 / −0.0644 / −0.0828 | −0.0361 / −0.0361 / −0.0375 |
| n=16, three windows | −0.0702 / −0.0656 / −0.0808 | −0.0321 / −0.0373 / −0.0347 |
| verdict | **established**, point band −0.064 to −0.087 ([:387-396](perf-cycle-ir-differential-2026-09-08.md)) | **established**, roughly half amd64's ([:1136-1140](perf-cycle-ir-differential-2026-09-08.md), [:1281-1284](perf-cycle-ir-differential-2026-09-08.md)) |

Every one of the twelve windows excludes zero with consistent sign.

The issue body's **+11.0% (n=8) / +8.6% (n=16)** are the *one-window* figures;
[the record supersedes them in part at `:101`](perf-cycle-ir-differential-2026-09-08.md)
and the three-window bands above are what stands.

Four limits bound every use of those numbers, and they are the reason this audit
states a scope verdict per record instead of a correction factor per record.

1. **Axis.** Steady-cycle wall clock only. No recipe control has ever been run
   on init, cold cycle, time-to-first-result, RSS, allocation counts,
   instruction counts, or binary size. **For a figure on any of those axes, no
   magnitude may be stated** — only a direction, and only where the figure is
   plausibly dominated by the same work.
2. **Transfer form — only the multiplicative factor transfers.** The effect is
   published as an additive shift on the ratio, `A = Z÷L − Y÷L`. Cell `L` is one
   artifact dlopened from one path in every round, so the recipe touches only the
   numerators and the whole effect sits in the oxpinyin arm. Writing `R_Y` for
   the record's own oxpinyin ratio level and `M = t_Y / t_Z` for the recipe's
   multiplicative penalty:

   > `A = R_Y × (1/M − 1)`  ⇔  `M = R_Y / (R_Y + A)`

   So **the additive number is the recipe's effect expressed in units of that
   session's libpinyin time, and it scales with the ratio level at which it was
   measured.** The quantity that is a property of the recipe alone — the only
   thing the control varied — is `M`. Correcting a different record means
   applying `M` to the oxpinyin arm (`R' = R / M`, or `t' = t / M` for an
   absolute), and the additive number transfers only to a record at the same
   ratio level: `Y÷L ≈ 1.02–1.05` on amd64, `≈ 1.14–1.15` on arm64. Applied at a
   different level it is wrong in proportion to the mismatch, and it cannot be
   applied at all to a record reporting absolute times with no co-measured
   libpinyin.

   Solving for `M` gives roughly **1.078 on amd64** and **1.033 on arm64**. Those
   differ, so the effect is **not arch-invariant in either form** — which is
   itself the reason a record that does not state its host cannot be corrected.
   `M` transfers only under the assumption that the recipe's penalty is a
   constant proportional factor, level- and workload-independent. The control
   supports the workload half within its own range (the per-unit ratio is flat
   across a 16× sweep on both hosts, and arm64's band matches at n=8 and n=16);
   nothing supports the rest.
3. **Workload size, backend, and configuration.** Measured at `PERF_REPEATS` ∈
   {8, 16}, **tkrzw only** on both hosts, against libpinyin's own installed data
   directory — no KC, redb or LMDB cell, and no datagen, training or sentence
   decode in the timed region. The amd64 control found the cargo-build cell
   **bimodal at n=1** and excluded it — and `n=1` is the default, so it is what
   the affected corpus ran at.
4. **Mechanism.** Unknown. Nothing in this repository explains *why* a cinstall
   artifact is faster than a `cargo build` + `strip` artifact of the same source
   at the same release profile. It is not a version script —
   `crates/oxpinyin-capi/build.rs:13-37` documents at length why the crate
   deliberately has none. Without a mechanism the effect must be treated as
   measured-where-measured, not as a constant of the codebase.

**Consequence.** "Affected" in the table below is a statement about *what the
record's artifact was*, which is certain wherever the evidence column is filled.
It is not a claim that a specific correction applies to a specific figure.

## Method, and the two shortcuts that do not work

Each record's recipe was established by reading the record in full and then
resolving whatever it delegates to, at the record's own date, from git. Two
plausible shortcuts were tried and rejected; both are recorded because a later
reader will reach for them.

**The artifact filename does not discriminate before 2026-08-30.** It is
tempting to read `libpinyin_capi.so` as "cargo build" and `libpinyin.so.15` as
"cinstall". That works only after `d32557e3` (2026-08-29T16:33:26Z), which
introduced the `[package.metadata.capi.library] name = "pinyin"` SONAME. Before
it, **cargo-c installed the artifact under the same name the cargo build
produces**: `run-perf-baseline.sh` at its first commit `3e5e4080` runs
`cargo cinstall` and then times `CAPI_SO="$CAPI_LIBDIR/libpinyin_capi.so"`. For
every record dated before 2026-08-30, only the cargo command line settles it.

**Citing `run-perf-same-data.sh` is not proof of the cargo-build recipe.** The
script honours `OXPINYIN_KC_SO` / `OXPINYIN_TKRZW_SO`, documented at its head as
"prebuilt oxpinyin `.so` paths", which skip `build_capi` entirely. A record that
names the script may still have timed a cinstall artifact supplied that way.

Where a document neither states its recipe nor delegates to something that
settles it, the verdict is **UNDETERMINED**. An inferred provenance in a
provenance audit is the defect the audit exists to find.

### Harness recipes, resolved at every date in the corpus

| harness | recipe | first commit | recipe ever different? |
|---|---|---|---|
| `run-perf-same-data.sh` (`build_capi`) | `cargo build --locked --release` + `strip --strip-all` | `d509041e` 2026-09-03T15:51:27Z | **no** — cargo build from its first commit |
| `run-perf-baseline.sh` | `cargo cinstall --locked --release` | `3e5e4080` 2026-08-16T16:59:57Z | **no** — cinstall from its first commit |
| `run-perf-matrix.sh` | prebuilt from `Dockerfile.perf-matrix` → `cargo cinstall` (`:118`, `:128`) | — | — |
| `run-callgrind-differential.sh` | prebuilt from `Dockerfile.callgrind` → `cargo cinstall` (`:155`, `:166`, `:176`) | — | — |
| `run-shipped-check.sh` | `cargo cinstall`, both cells | — | — |
| `run-tree-recipe-control.sh` | cells X, Y `cargo build`; cells Z, S `cinstall` | — | the control itself |
| `tools/profile/run-w8-cycle.sh` | `cargo cinstall --profile profiling` (`:126`) | — | cinstall, but **not the release profile** |

Because both drivers have carried one recipe since their first commit, the
delegation-drift defect the 2026-09-07 provenance audit names as its observation
7 — a live cross-reference silently changing meaning — **does not apply on this
axis**. That is a finding, not an assumption: it was checked, and it is the one
piece of good news in this audit.

### "Not affected" is not "measures the product"

`run-w8-cycle.sh` stages through `cargo cinstall`, so the W8 records are off the
`build_capi` path. But it does so with `--profile profiling` — thin LTO,
line-tables debug info, `panic = "unwind"` — against the release profile's fat
LTO and single codegen unit. That is a **third recipe**, also not the shipping
artifact, with a delta nobody has measured. Records on it are marked
`cinstall (profiling)` below and are outside #401's finding without being inside
the product's.

## The recipe map

Corpus: the 22 figure-bearing records of
[perf-provenance-audit-2026-09-07.md](perf-provenance-audit-2026-09-07.md), plus
the seven documents added or identified since. Verdicts:

- **AFFECTED** — the timed oxpinyin artifact came from `cargo build` + `strip`;
  the record describes a fixture and overstates the shipping product's cost.
- **NOT AFFECTED** — the timed artifact came from `cargo cinstall`.
- **N/A** — no oxpinyin shared object was timed at all (criterion bench, in-tree
  example binary, or a PyO3 extension module). The recipe axis is *absent*, which
  is not the same as passing it.
- **MIXED** — the document genuinely contains both, by design.

`resolved from` distinguishes a record that **states** its recipe from one where
the recipe had to be established elsewhere — and, in that case, from what. The
distinction is the point: a record in the first group can be re-checked by
reading it, and a record in the second cannot.

| record | verdict | recipe | resolved from |
|---|---|---|---|
| `perf-steady-cycle-cross-host-2026-09-07.md` | **AFFECTED** | `cargo build` + strip | **states it** — `:169` and `:278`, both hosts, with per-artifact sha256, `NEEDED` and byte size |
| `perf-keycost-first-alloc-2026-09-07.md` | **AFFECTED** | `cargo build` + strip | harness row `:158` names `run-perf-same-data.sh`; script read at the record's date |
| `runtime-direct-libpinyin-data-2026-09-02.md` | **AFFECTED** | `cargo build` + strip | §4 `:122` names `run-perf-same-data.sh`; §4 landed in `d509041e`, the script's own introducing commit |
| `perf-cycle-ir-differential-2026-09-08.md` | **MIXED** | both, deliberately | states it — cells X/Y are `cargo build`, Z/S `cinstall`; it is the control that found the effect |
| `rss-attribution-2026-09-09.md` | not affected | `cinstall --features …,shipped` | **states it** — `:159`, with sha256, SONAME and byte size. The only record that timed the *literal* product |
| `perf-backend-matrix-2026-09.md` | not affected | `cinstall` | **states it** — `:51-52` |
| `perf-backend-matrix-2026-08-31.md` | not affected | `cinstall` | `Dockerfile.perf-matrix` + `run-perf-matrix.sh` at `82a619a9`, the record's own introducing commit |
| `perf-baseline-kc-2026-09.md` | not affected | `cinstall` | `bisect --perf` → `run-perf-baseline.sh` (cinstall since its first commit) |
| `perf-baseline-kc-2026-08-31.md` | not affected | `cinstall` | same |
| `perf-baseline-kc-validation-2026-08-31.md` | not affected | `cinstall` | same |
| `perf-baseline-2026-08.md` | not affected | `cinstall` | `run-perf-baseline.sh`, named in the record |
| `perf-mmap-system-indexes-2026-08-31.md` | not affected | `cinstall` | delegated; chains to the matrix image. Status is REJECTED regardless |
| `perf-init-text-slurp-2026-08.md` | not affected | `cinstall (profiling)` | `run-w8-cycle.sh`, named in the record |
| `perf-init-typed-map-2026-08.md` | not affected | `cinstall (profiling)` | same |
| `perf-stage2-harness-2026-08.md` | not affected | `cinstall (profiling)` | same |
| `perf-fill-lookup-2026-08.md` | not affected | `cinstall (profiling)` + criterion | **states it** — "`--profile profiling` cargo-c install" |
| `perf-so-size-2026-09.md` | not affected | criterion; quotes `cinstall` figures | states its own bench; quoted `.so` figures chain to the KC baseline series |
| `perf-store-opt-2026-09.md` | N/A | criterion, no `.so` | states it |
| `perf-backend-matrix-bdb-store-2026-09.md` | N/A | criterion, no `.so` | states it — `:99-103`, `:231-238` |
| `perf-p2-chewing-table-2026-09-01.md` | N/A | in-tree example binary | states it |
| `data-load-audit-2026-08.md` | N/A | in-tree example binary | states it — `:18`, `:25` |
| `perf-alloc-2026-08.md` | N/A | criterion + dhat | delegated to `perf-exploration.md` |
| `perf-candidate-cap-2026-08.md` | N/A | criterion | states it |
| `perf-exploration.md` | N/A | criterion + Callgrind + dhat | states it |
| `perf-python-shared-engine-2026-08.md` | N/A | PyO3 wheel (`oxpinyin._native`) | a third artifact entirely; no C-ABI `.so`, no libpinyin comparison |
| `datagen-compat-2026-09-01.md` | N/A | — | names `run-perf-same-data.sh` once (`:143`) as a routing note, not as a source of figures |
| `perf-provenance-audit-2026-09-07.md` | N/A (survey) | — | measures nothing; quotes three figures. Its map has **no recipe column** — see observation 5 |
| `ci-perf-size-gate-proposal-2026-09-09.md` | N/A (proposal) | G1/G2 `cinstall`, G3 `cargo build` | measures nothing; specifies four *future* recipes. Sound — see Downstream |
| `ROADMAP.md` | downstream | — | quotes three affected figures; see Downstream |

**Three records are affected.** Every other record in the corpus either timed a
cinstall artifact or timed no shared object at all.

## The affected set, figure by figure

The bias scales oxpinyin's arm and leaves libpinyin's untouched, so within one
affected record the figures do not all move together. Three classes:

- a **cross-implementation ratio** against pin-built libpinyin moves by the full
  effect — this is exactly what the control measured;
- an **oxpinyin absolute** inherits it directly;
- a **within-pass quotient** — both sides measured in the same pass on the same
  artifact — is robust, because a whole-artifact scale factor cancels.

### `perf-steady-cycle-cross-host-2026-09-07.md`

Both passes built `cargo build --locked --release -p oxpinyin-capi
--no-default-features --features {kyotocabinet,tkrzw}` (`:169` arm64, `:278`
amd64) and published the sha256, `NEEDED` and byte size of each artifact. Its
Tkrzw sha256 `bf8d3b57…` is the one `run-tree-recipe-control.sh` pins as
`RECORD_X_SHA` — the control rebuilt this record's exact artifact, which is why
the effect was measurable at all.

Every steady- and cold-cycle ratio in it is a cargo-build ratio. The record
already carries a **tree-scoping** banner (`:3-24`, added `a118485b`): its
numbers describe the 2026-09-07 tree, not `main`. That banner is about the
engine; the recipe is a second, independent scope limit on the same numbers, and
it was not stated. A scope note is added by this audit.

### `perf-keycost-first-alloc-2026-09-07.md`

Harness row `:158` names `run-perf-same-data.sh`; the adjacent "Script delta"
row names a modification to `build_capi` itself, so the cargo-build path is not
in doubt. arm64, where the effect is roughly half amd64's.

- **Moves** — the implementation ratios `:219` (1.158×/1.168× → 1.156×/1.180×)
  and the "~1.16× steady" headline (`:10`, `:319`); the ttf ratios `:229`
  (2.783× → 1.236×, 2.816× → 1.235×) and "~1.24× first-result"; the
  session-ready RSS comparison "12,840 KiB against libpinyin's 12,738 KiB:
  +0.8%" (`:225-227`); every oxpinyin absolute in the two matrices (`:182-185`,
  `:191-194`).
- **Robust** — the headline result. First allocation ~17 ms → ~1 µs is a
  same-recipe parent→HEAD comparison separated by four orders of magnitude on
  its own axis (`:205-209`); so are the cold-cycle deltas and the closure check
  (`:210-216`), the steady parent→HEAD movement against the libpinyin control's
  own drift (`:217-218`), and the RSS delta `−8.98 MiB` (`:221`). The libpinyin
  cells are the in-image pin build and were never touched by `build_capi`.

  Two of those bullets carry **both** classes at once and must be read a line at
  a time: the steady bullet (`:217-220`) states a robust parent→HEAD movement and
  then a moving implementation ratio, and the RSS bullet (`:221-228`) a robust
  −8.98 MiB delta and then a moving +0.8% comparison against libpinyin.

  **"Same-pass" is not the same property as "same-recipe".** `:229` labels the
  ttf ratios "same-pass quotients", which is true and is what makes them immune
  to session drift — but both arms of a *cross-implementation* ratio are not
  built the same way, so a scale factor on the oxpinyin arm does not cancel. The
  phrase protects against a different hazard than this one.
- **Describes a different object outright** — `:199`, "Stripped `.so` size:
  1,576,960 bytes". That is the size of the `libpinyin_capi.so` fixture; the
  arm64 cinstall artifact in the control is 1,816,024 B
  ([`:1056-1059`](perf-cycle-ir-differential-2026-09-08.md)). **The two numbers
  are not a size effect and must not be quoted as one** — the control's
  `build_cargo` strips and its `build_cinstall` does not, so the comparison is
  confounded. They are simply different objects, and only one of them ships.

**PR #375's merged headline survives.** The issue's own comment anticipated
this and the audit confirms it: the first-allocation elimination and the ttf
collapse are dominated by a 17 ms term, and a single-digit-percent scale factor
on one arm does not reach it. What the audit does *not* endorse is the record's
absolute steady figures or its `.so` size line.

### `runtime-direct-libpinyin-data-2026-09-02.md`

§4 `:122` names `run-perf-same-data.sh`. **Its filename date is not its
measurement date**: §4 was added in `d509041e` (2026-09-03T15:51:27Z), the same
commit that introduced the script. A date-based check would have cleared this
record wrongly.

- **Moves** — "steady-state is ~12.2 ms vs the pin's ~8.1 ms (~1.5×)" (`:147`),
  restated as §6 item 1 (`:186`); the cold-cycle ratio; the init ratios "within
  ~1.1× and ~1.3× of the pin" (`:136`); every oxpinyin absolute in the §4 table.
- **Robust** — the four pin cells in full, and the within-oxpinyin cross-backend
  quotients (KC vs Tkrzw), both sides of which came out of one pass.
- **Recipe axis absent** — the §4 "Memory" paragraph (`:153-156`), produced by
  the `open_profile` example, which links the crate directly and dlopens nothing.
- **Cross-recipe, and worse than affected** — "~90–106× faster than before"
  (`:136`). Its numerator is the #260 baseline from
  `perf-backend-matrix-2026-08-31.md`, a **cinstall** record; its denominator is
  this record's **cargo-build** figure. At ~100× the effect is noise and the
  quotient survives, but it is not like-for-like, and no reader could tell.

**The architecture is not stated anywhere in the record**, and the effect
differs by 2× between the two hosts. So the direction is certain and the size is
single-digit-percent, but no corrected ratio can be given: whether ~1.5 becomes
~1.46 or ~1.38 depends on an architecture the document never records and on the
unresolved additive-vs-multiplicative transfer question above.

## Downstream: where an affected figure is quoted

An overstated figure does damage where it is **quoted**, not only where it is
measured, and a summary document carries no provenance of its own.

### `ROADMAP.md`

Nine perf figures on five lines. Three trace to
`runtime-direct-libpinyin-data-2026-09-02.md` and are therefore on the
cargo-build path:

| ROADMAP | figure | axis |
|---|---|---|
| `:369` | "the steady-state candidate lookup (~1.5× the pin)" | steady cycle — **the one figure the established band quantifies** |
| `:368` | "key-cost table (~16.5 ms…)" | oxpinyin absolute, alloc axis — inherits the recipe, unquantified |
| `:158`, `:356-357` | "init within ~1.1× (KC) / ~1.3× (tkrzw) of the pin" | init axis — inherits the recipe, unquantified |

The rest — `:358` init 102 → 21 ms and RSS 72,652 → 28,388 KiB, `:359` runtime
data 101.80 → 36.88 MiB, `:362` +5.5% for −64 KiB — trace to
`perf-baseline-kc-2026-09.md`, a cinstall record, and are clean on this axis.

**And the citation hides the exposure.** `:158` and `:356-357` attach the
~1.1× / ~1.3× init pair to `perf-backend-matrix-2026-09.md` and
`perf-baseline-kc-2026-09.md` — two **cinstall** records. Neither contains those
numbers: the first reports init at 1.16× (Tkrzw) / 1.12× (KC) (`:17-18`), the
second at 4.9× (`:52`). The pair is verbatim the P6 record's `:136`, which is on
the cargo-build harness. **A reader checking provenance by following ROADMAP's
own citations would clear these figures wrongly** — which is precisely the
failure mode this audit exists to catch, arriving by a different route.

Three further ROADMAP defects surfaced here. They are **not** #401 defects and
are recorded so they are not lost, not fixed here:

- **stale target** — `:367-368` presents the key-cost table as a pending next
  target. It was deferred on 2026-09-04 (`perf-baseline-kc-2026-09.md:307`) and
  eliminated on 2026-09-07 (`perf-keycost-first-alloc-2026-09-07.md:5-11`).
- **stale ratio** — `:369`'s ~1.5× is superseded three times over: 0.94×/0.95×
  at parity (`perf-backend-matrix-2026-09.md:22-23`), ~1.16× steady
  (`perf-keycost-first-alloc-2026-09-07.md`), and below 1 on the current tree
  (`perf-steady-cycle-cross-host-2026-09-07.md:14-17`). This compounds with
  #401 in the same direction but is independent of it: re-measuring on the
  current tree would have caught it whatever the recipe.
- **unsourced host** — `:356` labelled the P1–P6 figures "x86_64". Their
  source record states no host architecture anywhere; the label appears to
  have come from the record ROADMAP mis-cited, which is x86_64. Removed.
- **frozen intermediate** — `:358`'s "init 102 → 21 ms" quotes
  `perf-baseline-kc-2026-09.md:306` while the next row of the same table
  (`:307`) records init falling further to 3.3 ms, and that record's own result
  table has 3.216 ms.

### `ci-perf-size-gate-proposal-2026-09-09.md`

The proposal is **sound on this axis**, and it matters that it is sound for a
stated reason rather than by luck. All four gates are self-ratchets — oxpinyin
against its own baseline, same recipe on both sides — so the recipe contributes
exactly zero to any gated delta. G1 and G2 build the product path through
`tools/packaging/install.sh` (`:63-64`); G3 deliberately uses `cargo build`
because it needs `--features alloc-count`, which cannot coexist with `shipped`.

It nonetheless **never names cinstall-vs-cargo-build as a hazard**. G3 is the
near-miss: `:182-184` notices that it "is the one place the gate measures
something other than the product artifact" and then bounds that difference
entirely in terms of the two `shipped` fixture hooks — the feature axis — with
the recipe axis unmentioned. The conclusion holds; the argument is missing a
term. Two of its quoted absolutes (`:39` amd64 bimodal medians, `:225-226` RSS
bands) come from the affected cross-host record, but both are used only as
illustrations of runner noise, and every conclusion drawn from them rests on
within-pass quotients where the recipe cancels.

## Observations

1. **The corpus is mostly clean, and that is a finding, not a relief.** Three
   records of 29 are affected. The reason is not discipline: it is that the
   affected harness existed for only five days before the control caught it
   (`d509041e`, 2026-09-03 → the #394 control, 2026-09-08). Every record before
   it ran on `run-perf-baseline.sh` or `run-w8-cycle.sh`, both of which have used
   `cargo cinstall` since their first commit. **A recipe divergence was
   introduced silently and would have propagated indefinitely**; what bounded the
   damage was the interval, not the process.

2. **A new defect class: the cross-recipe quotient.** `runtime-direct…:136`'s
   "~90–106× faster than before" divides a cinstall numerator by a cargo-build
   denominator. Neither record is wrong; the quotient is not like-for-like and no
   reader could tell. This class is invisible to a per-record audit — it only
   appears when two records are divided — so it will not be caught by any check
   that looks at one document at a time.

3. **A second defect class: provenance laundering by citation.**
   `ROADMAP.md`'s two init citations named `perf-backend-matrix-2026-09.md` and
   `perf-baseline-kc-2026-09.md` — both **cinstall** records, neither containing
   the quoted figures — for numbers that are verbatim the **cargo-build** P6
   record's. A reader who does the responsible thing and follows the citation to
   check provenance lands on a clean record and concludes the number is clean.
   No amount of banner work on the affected record catches this, because the
   affected record is never named. It is the mirror image of observation 2: there
   a correct citation hid a mismatch between two records, here an incorrect
   citation hid the record entirely.

4. **A size figure is not a biased number, it is a different object.**
   `perf-keycost-first-alloc…:199`'s "1,576,960 bytes" is the size of
   `libpinyin_capi.so`. The shipping artifact on that architecture is 1,816,024 B.
   No correction factor connects them: one is the size of a file that ships and
   the other is the size of a file that does not.

5. **The provenance map has no recipe column, and its harness column has an
   error.** `perf-provenance-audit-2026-09-07.md` records date, commit, window
   side, configuration, harness and captures — six columns, none of which
   distinguishes `cargo build` from `cargo cinstall`. That gap is the whole
   reason #401 had to be a separate audit. Separately, that map's harness cell
   for `perf-backend-matrix-bdb-store-2026-09.md` reads `bisect --perf`; the
   record contains no occurrence of the string `bisect` and states its harness as
   criterion (`:99-103`, `:231-238`). The row is corrected by a pointer, not
   rewritten — see the note added there.

6. **"Not affected" was the wrong question to stop at.** `run-w8-cycle.sh` is on
   the cinstall path but uses `--profile profiling`: thin LTO, line tables,
   `panic = "unwind"`. Four records sit on it. They are outside #401's finding
   and outside the product's description at the same time, and nothing has
   measured that third gap.

7. **The 9–11% session offset is *not* this effect, and the coincidence is a
   trap.** `perf-provenance-audit-2026-09-07.md` twice records (`:92`, `:353`) a
   whole-session offset of **9–11%** co-moving across all four cells between two
   sessions on one machine, and uses it to argue that absolute agreement is not a
   valid verification criterion for any re-measurement. That range is the same
   sign and nearly the same size as the one-window `Y÷Z` figures in the issue
   body, and the two will be conflated. They are unrelated: **both sides of the
   session-offset comparison ran `run-perf-same-data.sh`**, so the recipe is
   common-mode there and cannot be what moved. The session-offset caveat stands
   on its own and is not explained away by #401.

8. **The best-provenanced records are the affected ones.**
   `perf-steady-cycle-cross-host-2026-09-07.md` states its build flags, sha256,
   `NEEDED` and byte size per artifact — which is why the control could rebuild
   its exact `.so` and measure the effect at all. The records that state nothing
   are not thereby safe; they are merely unfalsifiable. Provenance quality and
   correctness are independent, and this corpus inverts the intuition.

## Corpus boundary

**The corpus above is filename-shaped, and that is a weakness it inherits.** It
is `docs/perf/*.md` ∪ `docs/findings/perf-*.md` ∪ four named findings ∪
`ROADMAP.md` — the 2026-09-07 map's set plus what has landed since. A content
sweep of all 801 tracked files finds figure-carriers outside it. None changes
the affected set, and the reason matters: **every one of them is downstream of a
`cinstall` record**, so they are clean on this axis by luck of ancestry rather
than by having been checked.

- `Cargo.toml:83-94` — the release-profile comment restates "+5.5% steady /
  +6.4% cold" and `−65,536 B`, on the steady-cycle axis, and sets a standing
  policy gate ("Re-adopting abort needs a keystroke-cycle number next to the
  size number"). Source: `perf-baseline-kc-2026-09.md`, cinstall.
- `crates/oxpinyin-engine/src/session/mod.rs:166`,
  `crates/oxpinyin-runtime/src/lib.rs:956`,
  `crates/oxpinyin-engine/src/session/tests.rs:1304` — the "42–57 ms first-alloc"
  figure, one of them in **published rustdoc**. Source:
  `perf-backend-matrix-2026-09.md`, cinstall.
- `docs/safety/enforcement-matrix.md:31` and
  `docs/safety/executive-summary.md:132-137` — the same size and cycle figures,
  used to settle a governance question. Sources cinstall.

One carries an **original** figure that no delegation resolves:
`libpinyin-system-data-formats-2026-09-01.md:240`'s "strictly better than
#269's 21.7 ms". It is an oxpinyin init time attributed to a pull request
rather than to any record, it appears nowhere else in the tree, and the
document names no host, harness or build. It is used as the benchmark a
redesign must beat. **UNDETERMINED**, and only a re-measurement resolves it.

(`trainer-replacement-report.md:232`'s "`oxpinyin-kmm generate` completes in
~6 ms" is a trainer-CLI timing — an in-tree Rust binary, no shared object — so
the recipe axis is absent, not unresolved.)

`datagen-compat-2026-09-01.md` is the one corpus member with no figures at all.
It is retained above as harness context, labelled as such, rather than dropped
silently.

## What this audit cannot conclude

It establishes **which artifact each record timed**, from the record or from the
harness at the record's date, and it says which figures inside an affected record
the bias reaches.

It **cannot state a corrected figure for any record**, and it does not try. The
effect is measured on one axis (steady-cycle wall clock), at two workload sizes
(8 and 16) neither of which is the default the affected corpus ran at, on two
specific hosts, with no mechanism known and the additive-versus-multiplicative
transfer unresolved. One affected record does not state its architecture, and the
effect differs by 2× between architectures.

A correction is therefore a **re-measurement**, on each record's own tree,
producing a new record with its own provenance — not a multiplication applied to
an old table. Per the 2026-09-07 audit's own warning, such a re-measurement
cannot be validated against the old absolutes either: a whole-session offset of
9–11% co-moving across all four cells has been observed between sessions on one
machine, so a faithful re-run will disagree with a correct original.

## Forward: record the recipe

Nothing in `docs/runbooks/benches.md`, `docs/perf/README.md` or
`docs/findings/README.md` asks a measurer which artifact to build, or asks a
record to say which one it built. That omission is what let the divergence run
unnoticed, and it is the one part of this that is cheap to close: the write-up
rules now require the build recipe, and
`perf-steady-cycle-cross-host-2026-09-07.md:169` is the worked example of what
that looks like — command line, sha256, `NEEDED`, byte size, per artifact.
