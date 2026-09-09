# Steady-cycle differential and arch spread — amd64 + arm64 (2026-09-08)

What began as the amd64 Ir differential became the dual-architecture record
of the arch spread. The amd64 sections establish that the **shipped artifact**
(`cargo cinstall --features shipped`) is **at parity with libpinyin at both
sizes** (three-window control, 2026-09-09: S÷L contains 1.0 in every n=16
window and in two of three n=8 windows); the arm64 sections — same protocol,
same oracle pin, the same engine tree (identical `crates/`; the instrument
commits between the two passes touch only `tools/`) — establish that the
shipped artifact is measurably *slower* on arm64 (S÷L 1.1048–1.1123 at n=8,
every window's CI entirely above 1.0, verified same-recipe by the 2026-09-09
control). **A ~10–14-point architecture spread on identical source, now
established at both sizes on symmetric three-window designs, is this
document's conclusion.** Each pass's evidence is in its own sections; the
arm64 section's ending carries the exclusions that narrow the cause and the
pointer they support.

The recipe effect (`cinstall` vs `cargo build`+strip) is **established on
both architectures** — ≈−0.07 on amd64, ≈−0.036 on arm64, all windows
excluding zero. The tree effect (the session-layer rewrite) is **not
established on either architecture**: on amd64 its single-window +0.046
magnitude is refuted and its smaller end remains open (points positive in
all six amd64 windows, negative in all six arm64 windows — the asymmetry is
real, its size unresolved); see §"The three-window control".

## amd64 verdict, first line

**On this host — the same amd64 machine the cross-host record's amd64 section
used — the shipped artifact (`cargo cinstall --features shipped`) is at
parity with libpinyin at both sizes** (three-window control, §"The
three-window control": n=8 0.9712–0.9806 with 1.0 inside two of three CIs;
n=16 0.9664–0.9948 with 1.0 inside all three). It executes ~35% more
instructions (hardware counter, 1.3537×) and still does not lose on wall
clock, because its IPC is ~42% higher.

This replaces the one-window shipped-check result (S÷L 0.9977 at n=8 /
0.9655 at n=16, "parity at n=8, 3.4% faster at n=16") — the n=16 "faster"
did not survive three windows — and, before that, the single-session claims
"measurably faster" (T_nodebug 0.9527) and "at parity" as settled fact. The
record's numbers do reproduce under the record's own recipe (X÷L in band in
all six windows). What the record's ratio describes is the `cargo build`
fixture, not the shipping artifact; under the shipping recipe the ratio sits
at parity.

## What was measured

The drop-in configuration only: both engines open libpinyin's own installed
data directory, no datagen, no train, no sentence decode. Nothing here says
anything about oxpinyin's own generated data or the training/sentence paths.

| Quantity | Value | Definition |
|---|---|---|
| **I** (callgrind Ir ratio, ox ÷ lp) | **1.0400** | Toggled to the steady anchor, 3 steady cycles per profile, round 1 |
| Determinism | lp 0.0000%, ox 0.0031% | Two rounds; well under the 0.1% report threshold |
| **I_hw** (hardware instruction ratio) | **1.3537** | Native `PERF_COUNT_HW_INSTRUCTIONS`, `exclude_kernel=1`, ENABLE/DISABLE bracketing the same steady anchor region, 10 runs, median per cell |
| **T** (steady wall-clock ratio, prior session, cinstall) | **0.9527** [0.9136, 0.9846] | `PERF_CYCLES=8`, 20 runs, round-robin, medians, 95% percentile bootstrap, seed 20260907 |
| T (debug-info artifact) | 0.9660 [0.9140, 1.0044] | same protocol, the artifact callgrind profiles |
| **Product ratio** (shipped artifact S, three windows) | **S÷L n=8**: 0.9791 [0.9482, 1.0043] / 0.9806 [0.9380, 1.0261] / 0.9712 [0.9565, 0.9834]; **n=16**: 0.9664 / 0.9948 / 0.9835, all containing 1.0 | five-cell three-window control, §"The three-window control" |
| One-window values they replace | S÷L 0.9977 (n=8) / 0.9655 (n=16); Z÷L 0.9559 / 0.9805 | superseded, retained in §"The confound, resolved" |

**Which T entered the decision: T = 0.9660**, the debug-info artifact, because
`I` and `T` must come from the same binary. The two prior-session T values do
not carry the same statistical weight, and the difference matters:

- **T_nodebug = 0.9527, CI [0.9136, 0.9846] excludes 1.0.** The cinstall
  artifact (no `shipped` flag) is *measurably faster* than libpinyin in that
  session — the effect is outside the interval, not a tie.
- **T_debug = 0.9660, CI [0.9140, 1.0044] contains 1.0.** The profiled artifact
  cannot be distinguished from parity by this sample; its point estimate is
  below 1 but the interval straddles it.

So the honest prior-session claim is: *the cinstall artifact was measurably
faster; the profiled artifact indistinguishable from parity.* The decision
rule used T_debug for binary consistency. **The product-level statement does
not rest on either: it rests on S÷L measured directly (§confound), which is at
parity at n=8 and marginally faster at n=16.**

Rule applied (restated in Phase 1 before any number existed): `I ≥ 1.03` and
`I > T + 0.03` → **extra work, partly absorbed by IPC**. 1.0400 > 1.03, and
1.0400 > 0.9660 + 0.03 = 0.9960. The verdict is *extra work*; the instructions
are real and removable regardless of how well the core hides them.

### The IPC result is derived from native numbers only

+35.4% instructions and −4% wall clock ⇒ **IPC ratio ≈ 1.354 / 0.953 = 1.42**,
i.e. oxpinyin retires ~42% more instructions per cycle. That is the whole
argument, and it uses only the hardware counter and the wall clock — both
native measurements on the real machine. (The −4% is the cinstall artifact's
steady ratio against libpinyin on this host, the same quantity the control
re-measures as Z÷L.)

The callgrind `Dr`/`Dw`/`Bc` columns are **not** part of it and were removed
from the reasoning rather than caveated. They are collected in an environment
that masks CPUID, where libpinyin's scalar (non-AVX) memcpy splits one memory
operation into many — inflating *its* memory-operation counts specifically.
Comparing the two engines' memory counts under that distortion and then using
the comparison to explain the time difference is circular: the distortion is
largest exactly for the side the argument needs to look efficient. They are
retained only as raw captured output in the profile files.

## The confound, resolved: two effects, opposite signs

> **Superseded in part (2026-09-09).** This section's decomposition, product
> ratios and the "regression — a new finding" subsection are **one-window
> results**; the three-window control in §"The three-window control" below
> replaces them: the recipe effect stands (somewhat smaller), the +0.046 tree
> magnitude is refuted, the shipped-flag contrast is not measurable at this
> precision, and the product verdict is parity at both sizes. The design
> reasoning here (which recipe is which, the X gate, composition validity)
> still holds and is not repeated below.

The cross-host record's amd64 pass and this branch's pass differed in **two**
ways, not one: the engine tree (`f79f665d` → current, the session rewrite) and
the build recipe (the record's timed cells came from
`run-perf-same-data.sh`'s `build_capi`: `cargo build --locked --release` +
`strip --strip-all`; this branch's came from `cargo cinstall`). The branch
attributed the whole movement to the tree. A four-cell control separates them.

### Which recipe is which

`cargo cinstall` is **the shipping path**, not a contaminant:
`.github/workflows/release-packages.yml` builds every distro package through
`tools/packaging/install.sh`, which runs `cargo cinstall`; `Cargo.toml` names
the shipped drop-in as `cargo capi install --features shipped`; and
`docs/packaging.md` says the `shipped` feature "is enabled only for the
shipped drop-in artifact". `cargo build` produces `libpinyin_capi.so`, a
bisection fixture that nothing installs. **The product artifact is S
(`cargo cinstall --features shipped`); Z is the same recipe without the flag
and serves as a supporting cell; X and Y (cargo build) are the artificial
ones.** The record measured a non-shipping artifact; this is not a defect in
the measurement, it is a scope fact about what its numbers describe.

### The control

Four cells, one container, one data directory, one harness, one session,
round-robin (`tools/bisection/run-tree-recipe-control.sh`, commit `787ef383`):

| cell | tree | recipe | artifact |
|---|---|---|---|
| **L** | libpinyin @ pin `074a2219` | in-image | `19349637…` |
| **X** | `f79f665d` (record's tree) | `cargo build` + strip | `bf8d3b57…` |
| **Y** | current tree | `cargo build` + strip | `2cdf2fd9…` |
| **Z** | current tree | `cargo cinstall` | `7b60a63e…` |

`X` is **byte-identical to the record's documented Tkrzw artifact**
(`bf8d3b5707b5bc1bafc09670185b96770b73f596dd2eb57b8a9cba46bad4aec8`,
1,688,840 B) — a hard gate refuses to read any ratio otherwise.

**The shipped flag, measured directly.** The literal packaged artifact
additionally sets `--features shipped` (that is what `Cargo.toml` and
`docs/packaging.md` name as the shipped drop-in), which compiles out two
fixture hooks the timed path never calls. Cell Z lacked that flag, so the
literal product artifact **S** was measured against L and Z in one session,
round-robin (`tools/bisection/run-shipped-check.sh`, 20 runs per cell per
size; all three cells unimodal by the locked rule):

| ratio | n=8 | n=16 |
|---|---|---|
| **S÷L** (the product) | **0.9977 [0.9582, 1.0230]** | **0.9655 [0.9423, 0.9998]** |
| Z÷L (same session) | 0.9641 [0.9271, 0.9925] | 0.9742 [0.9481, 1.0004] |
| S÷Z | 1.0349 [0.9975, 1.0668] | 0.9910 [0.9704, 1.0238] |

The S÷Z point estimates straddle 1.0 in opposite directions (+3.5% at n=8,
−0.9% at n=16) and n=8's lower bound sits at 0.9975, against a recipe effect
of ~9%. **That supports "no consistent effect detected, and not tight enough
to exclude a few percent" — not "the flag is not a ratio factor."** The
honest statement is that the flag is small relative to the recipe effect and
the data cannot resolve it further at these run counts.

**Composition is invalid here, and it matters.** S÷L cannot be obtained as
(Z÷L from the control) × (S÷Z): Z÷L differs between the two runs (0.9559 in
the control vs 0.9641 in the shipped run) by the session offset, so composing
would give 0.9893 at n=8 where the direct paired measurement gives 0.9977.
The direct number is the one reported; Z÷L is a supporting cell from its own
session.

Paired round bootstrap (seed 20260907, `tools/bisection/perf-decomp.py`):

| n | X÷L | Y÷L | Z÷L | **tree = Y÷L − X÷L** | **recipe = Z÷L − Y÷L** |
|---:|---|---|---|---|---|
| 8 | 1.0146 [0.9989, 1.0413] | 1.0606 [1.0460, 1.0948] | 0.9559 [0.9373, 0.9891] | **+0.0460 [+0.0220, +0.0750]** | **−0.1047 [−0.1301, −0.0746]** |
| 16 | 1.0358 [1.0145, 1.0607] | 1.0653 [1.0359, 1.0999] | 0.9805 [0.9570, 1.0004] | **+0.0295 [+0.0055, +0.0624]** | **−0.0849 [−0.1129, −0.0607]** |

Both effects are real (CIs exclude zero at both trustworthy sizes) and they
have **opposite signs**. The arithmetic closes: tree + recipe = Z÷L − X÷L
(−0.0587 at n=8, −0.0554 at n=16). **Bucket: partial** — the recipe accounts
for ~69–74% of the magnitude, the tree ~26–31%.

### The regression — a new finding (one window; superseded)

> **Superseded by §"The three-window control".** The claim below rested on
> one window. Three windows refute its +0.046 magnitude, leave its +0.0295
> end unexcluded, and do not establish the effect. Retained verbatim as the
> record of what was claimed and when.

**With the build recipe held constant, the current engine tree is 3–5% slower
than the tree the record measured** (+0.0460 and +0.0295; CIs excluding
zero). The branch had claimed the rewrite made the product faster; the
controlled measurement says the opposite, and only `cinstall` produces a
sub-1 ratio. The rewrite claim is **withdrawn and inverted**: the session-layer
rewrite is a measurable steady-cycle regression on amd64.

This is not softened as a "correction" — it is a finding in its own right, and
the first time this project has measured the rewrite's steady-cycle cost with
the recipe controlled. It does not overturn the product verdict: the shipped
artifact is at parity at n=8 and 3.4% faster at n=16, so the regression is
absorbed into a ratio that still does not exceed 1.

### Reproduction quality

At **n=16 the reproduction is to 0.0015** (X÷L 1.0358 vs the record's own
1.0373) on byte-identical artifacts — that is the strong signal. At n=8 the
gap is 0.0397 (1.0146 vs 1.0543), inside the pre-registered band
[0.9904, 1.1041] but visibly noisier; the record's own n=8 row sat at the top
of its sweep's range (1.0543 against 1.0260–1.0350 at n=2/4), so part of that
gap is the record's own ratio instability. "In band" understates the n=16
agreement; both sizes reproduce, one of them essentially exactly.

### The soundness check

Z is byte-identical to the prior session's nodebug artifact (`7b60a63e…`), so
Z÷L re-measures the same bytes the earlier T = 0.9527 was taken on. At the
comparable size (n=1): **Z÷L = 0.9626 vs 0.9527, Δ = +0.0099 — inside the
0.01415 band, CIs overlapping.** The prior session's number holds. This is a
check, not a decision input: it is the one comparison in the chain that
crosses sessions, images, data directories and harnesses, and it agrees.

### The factor is still constant — constant at about parity

The workload sweep (`PERF_REPEATS` ∈ {1, 8, 16}) confirms the *shape* the
record reported; only the level moved. The product artifact was measured at
n=8 and n=16: 0.9977 → 0.9655, a spread of 3.3% over a 2× workload change
(the prior session's Z ran n=1 at 0.9626 and n=8/16 at 0.9559/0.9805, the same
flat shape). **That is a constant ratio** at or just below parity: the same
structural property the record found, moved from above 1 to about 1. The
engine does a fixed amount of extra work per unit at every size and now breaks
even on wall clock. Anyone reading "the factor moved to about 1" as "the
structure changed" would be wrong: the structure is the same, the level moved
because the code and the recipe changed.

### The arch spread survives, with a consistency check

> **The arm64 pass's recipe is now verified, not inferred.**
> `Dockerfile.callgrind` at the pass's instrument commit `9b5a89fe` builds the
> timed artifact with `cargo cinstall --no-default-features --features tkrzw`
> (unstripped, no `shipped` flag), and the 2026-09-09 control rebuilt exactly
> that recipe on the same engine tree and got a **byte-identical** artifact
> (`c19629f8…`, 1,816,024 B) — the pass's timed artifact, reproduced to the
> byte. That upgrades "the ≈1.10 was a Z-recipe number" from inference to
> verified fact and puts the arm64 Z÷L on the same footing as amd64's. The
> flag-for-flag caveat is closed by measurement in the control section below:
> on arm64 S÷Z ≈ 1.00, so the flag does not move the arm64 number.

The arm64 pass and the amd64 pass both used `cargo cinstall`, so the arch
spread is measured on one recipe. As a **check, not evidence** — effect sizes
need not transfer across architectures — the amd64 decomposition applied to
the record's arm64 1.158 lands on the measured arm64 number:

```
1.158 (record arm64, cargo-build-era) + 0.038 (mean tree) − 0.095 (mean recipe) = 1.101
measured arm64 (cinstall, current tree): 1.10
```

That the two agree to 0.001 is a consistency check that the decomposition's
signs and rough magnitudes are not absurd; it is not evidence for the
decomposition, because there is no arm64 control and the effects were
estimated on amd64. **The arm64 same-recipe control has since run — see
"arm64: the same-recipe control" below; it supersedes this consistency
check.**

### Consequence for every record built through `build_capi`

Records that measured oxpinyin through `run-perf-same-data.sh`'s `build_capi`
(`cargo build` + strip) describe a **non-shipping artifact** and therefore
**overstate the shipping product's cost**: at the sizes measured here, Y÷Z is
**+11.0% (n=8)** and **+8.6% (n=16)**. The cross-host record's amd64 section,
`perf-keycost-first-alloc-2026-09-07.md`, and the datagen/runtime records that
cite the same driver are in that set. Records that used `cinstall`
(`perf-backend-matrix-2026-09.md`, the KC baselines, `run-perf-baseline.sh`)
are not.

This needs **its own issue** — an audit of which records are affected and by
how much each ratio moves. It is filed as **#401** and recorded here, not
fixed: correcting other records' numbers is out of this task's scope and would
need its own measurements per tree.

## The three-window control (2026-09-09, amd64)

**Verdict, first line.** At three windows with all five cells in-session, the
shipped artifact is **at parity with libpinyin on amd64 at both sizes**
(S÷L contains 1.0 in every n=16 window and in two of three n=8 windows); the
**recipe effect is established** (≈−0.07, all six windows excluding zero);
the shipped-flag and tree-regression single-window magnitudes are **not
confirmed** — the flag is *not measurable at this precision*, and the tree
effect's +0.046 end is *refuted* while its smaller end remains open; and the
**arch spread is established at both sizes** with no interval overlap. This
section replaces the one-window decomposition, product ratios and regression
finding of §"The confound, resolved"; the replacements are named claim by
claim below.

### Design

The arm64 control's design, exactly: five cells (L X Y Z S — S now
in-session with X, Y and Z, the defect this run exists to fix), one container
session per window, the record's own image, round-robin within each window,
20 runs per cell, `PERF_CYCLES=8`, sizes n ∈ {8, 16}, `taskset -c 0`,
`perf-ratio.py`/`perf-decomp.py` seed 20260907. Controller commit `3fa702f3`.
The X gate passed on the **full sha256** in all three windows (in-situ
rebuilds; X 1,688,840 B and Y 1,696,520 B differ by 7,680 B on amd64, so size
corroborates the gate here, unlike aarch64's 64 KB-quantised sizes). Z
exports 2 fixture hooks, S exports 0 — verified in each window's identity
table.

**The corrected bimodality rule was pre-registered before any data on this
host** (largest-gap split with both groups ≥ 3 → bimodal; 1–2-run minority →
outlier, kept, median reported with and without). Outcome: all 30
cell×window×size combinations **unimodal** (max adjacent ratio ≤ 1.133) — no
outlier, no split, nothing to report with/without. Abandon rules: S÷L max
width 0.0881 (n=8) / 0.0817 (n=16), both < 0.20; three-window point range
0.0094 / 0.0284, both < 0.05. **Measurable at both sizes.**

### The reboot — an unplanned control

The original W3 was killed mid-window by a **host reboot** (machine up 2 min
when the session resumed; 210 of 300 rows collected, preserved as
`amd64-w3-partial` with the reason). Per the pre-registered replacement rule
it was replaced in full. W1 and W2 are therefore pre-reboot and W3r
post-reboot — a full machine-state reset between windows — and **W3r agrees
with W1/W2 to under 0.01 on every ratio** (S÷L 0.9712 vs 0.9791/0.9806 at
n=8; 0.9835 vs 0.9664/0.9948 at n=16). That is direct evidence the
three-window protocol measures what it claims: a complete session-scale
state change produced no session-scale drift in the within-window ratios,
which is precisely the property the round-robin design exists to guarantee.
The unplanned control is worth more than the interruption that produced it;
the partial window stays in the record.

### The product ratio, S÷L

| size | W1 | W2 | W3r | verdict |
|---|---|---|---|---|
| n=8 | 0.9791 [0.9482, 1.0043] w=0.0561 | 0.9806 [0.9380, 1.0261] w=0.0881 | 0.9712 [0.9565, 0.9834] w=0.0269 | **at parity** |
| n=16 | 0.9664 [0.9383, 1.0200] w=0.0817 | 0.9948 [0.9661, 1.0203] w=0.0542 | 0.9835 [0.9651, 1.0173] w=0.0522 | **at parity** |

Replaces the one-window values 0.9977 / 0.9655 and their "3.4% faster at
n=16" reading (that CI's upper bound was 0.9998 — one window, and it did not
replicate).

### The shipped flag, S÷Z — not measurable at this precision

| size | W1 | W2 | W3r | vs prior |
|---|---|---|---|---|
| n=8 | 1.0178 [0.9913, 1.0415] w=0.0502 | 1.0214 [0.9647, 1.0629] w=0.0983 | 1.0089 [0.9963, 1.0198] w=0.0235 | prior 1.0349 inside W1/W2 → not excluded |
| n=16 | 1.0092 [0.9697, 1.0619] w=0.0922 | 1.0097 [0.9760, 1.0418] w=0.0657 | 1.0084 [0.9716, 1.0462] w=0.0746 | prior 0.9910 inside all three → not excluded |

Every window contains 1.0 (never established), and no window set excludes
the prior single-window value — category 4 of the pre-registered
classification: **no consistent effect detected, and not tight enough to
exclude a few percent; the prior is neither confirmed nor excluded.** By the
pre-committed rule the headline therefore **stays on S÷L** (it would have
moved to Z÷L only if S÷Z excluded 1.0349). The point estimates (1.009–1.021
at n=8) sit between arm64's null and the amd64 prior, and the arm64-vs-amd64
flag contrast remains open at this precision. This replaces the earlier
"open discrepancy" framing and the one-window 1.0349 [0.9975, 1.0668].

### The tree effect, Y÷L − X÷L — in its own words

| size | W1 | W2 | W3r | classification |
|---|---|---|---|---|
| n=8 | +0.0213 [−0.0038, +0.0331] w=0.0369 | +0.0147 [−0.0097, +0.0298] w=0.0395 | +0.0238 [+0.0037, +0.0424] w=0.0387 | prior +0.046 **outside all three → refuted**; zero inside W1/W2 |
| n=16 | +0.0002 [−0.0209, +0.0286] w=0.0495 | +0.0312 [+0.0067, +0.0480] w=0.0413 | +0.0086 [−0.0070, +0.0441] w=0.0511 | prior +0.0295 **inside W2/W3 → not excluded** |

**Single-window magnitude unreplicated at this precision: the 5% end
refuted, the 3% end open, the effect not established on either host.** The
amd64 point estimates are **positive in all six windows** (+0.0002 to
+0.0312); arm64's were negative in all six. The asymmetry is real — same
recipe, same protocol, opposite point directions on the two architectures —
but its size is unresolved: no window set excludes zero with consistency,
and no window set establishes the magnitude either way. This replaces the
one-window finding "the session-layer rewrite is a measurable 3–5% steady-cycle
regression on amd64" — which is neither confirmed nor withdrawn-to-neutral;
it is unreplicated at this precision, and per the pre-committed rule the
Phase 3 motivation in §"Why Phase 3 is now about allocation churn" **stays
as-is**: that section's resource-axis argument (instructions +35%, RSS 1.23×)
does not rest on the tree effect, and the tree question is open rather than
settled, so there is nothing to update it to. Better data, not this run.

### The recipe effect, Z÷L − Y÷L — established

| size | W1 | W2 | W3r |
|---|---|---|---|
| n=8 | −0.0873 [−0.0982, −0.0732] w=0.0250 | −0.0644 [−0.0876, −0.0380] w=0.0496 | −0.0828 [−0.1049, −0.0729] w=0.0320 |
| n=16 | −0.0702 [−0.0922, −0.0549] w=0.0373 | −0.0656 [−0.0882, −0.0452] w=0.0430 | −0.0808 [−0.1022, −0.0529] w=0.0493 |

All six windows exclude zero, sign consistent → **established**, point band
−0.064 to −0.087. The one-window −0.1047 sat at the edge of this band; the
effect is real and somewhat smaller, roughly 2× arm64's −0.032 to −0.038 —
now a symmetric three-window contrast on both hosts.

### The arch spread — established at both sizes, on record bounds

| size | amd64 max S÷L upper bound (this control) | arm64 lowest S÷L lower bound (from the record) | overlap |
|---|---|---|---|
| n=8 | **1.0261** (W2) | **1.0924** (T-nodebug gate line, W2 of the 2026-09-08 pass — the record's in-print per-window bounds) | **none** |
| n=16 | **1.0203** (W2) | **1.0841** (control W3, corrected-rule table) | **none** |

Both hosts now carry three-window, S-in-session designs, so the
design-asymmetry caveats that accompanied earlier cross-arch contrasts are
removed. The n=8 arm64 bound is the record's own printed per-window lower
bound (its gate line cites 1.0926 / 1.0924 / 1.1051); the n=16 bound is the
arm64 control's corrected-rule table. Reproduction on the amd64 side: X÷L in
band in all six windows (n=8 1.0097–1.0280 in [0.9904, 1.1041]; n=16
1.0196–1.0475 in [0.9735, 1.0777]).

### Host conditions

Load 1.08→2.04 around W1, ~2.1 around W2, 0.89→~2 around W3r (fresh boot;
the co-tenant containers did not restart) — quieter than the previous amd64
session's 4.84→2.29, so **load is a candidate explanation, alongside window
count, for differences from that session's single-window values**; the
round-robin protects each window's internal terms. Captures:
`/tmp/amd64-w{1,2,3}/` and `/tmp/amd64-w3-partial/`, not committed.

### The epistemic note

This is the third amd64 result in the favourable direction this workstream
has produced, and each of the previous two was wrong for a reason nobody
suspected in advance: the 2026-09-05 "x86_64" timings were a platform-pinned
recipe artifact; the one-window "measurably faster" over-read its interval.
This one — three-window parity — had a flattering reading available ("the
flag collapses; the headline is Z÷L ≈ 0.96, measurably faster, wider spread")
and it is **not** licensed by the pre-registered categories: the flag is
category 4, not the null. What caught it was the four-category rule plus
pre-registration decided before the data; that combination, not the analyst's
discipline, is the reusable safeguard, and it belongs in the next
pre-registration too.

## Instruction counts: callgrind understates the gap

`I = 1.0400` is systematically low, and `I_hw = 1.3537` is the true
instruction gap. The reason is the CPUID/allocator distortion, and the user's
reasoning was confirmed and completed here:

- Callgrind links `vg_replace_malloc.c` into every tool, so Ir attributed to
  allocation is valgrind's simplified allocator, not glibc's.
- More importantly, valgrind masks CPUID, so glibc's ifunc-dispatched string
  routines fall back to non-AVX paths. That inflates the *memcpy-heavy* engine
  far more than the other: `__memcpy_avx_unaligned_erms` is **32% of
  libpinyin's** collected Ir but only **8.2% of oxpinyin's**. Inflating the
  libpinyin denominator more than the oxpinyin numerator compresses the ratio.
- Evidence that this is real, not an argument: libpinyin's callgrind figure is
  **177.8 M instructions per cycle**, its native hardware count is **126.5 M**.
  A 40% inflation that the allocator alone cannot explain is exactly the
  masked-CPUID signature. oxpinyin: 184.9 M callgrind vs 171.2 M native.

So the real story is: **oxpinyin executes ~35% more instructions than
libpinyin on this cycle, and hides all of it in IPC.** The IPC claim rests on
the native numbers above, not on any simulated counter — see the derivation in
the header.

The callgrind `Dr`/`Dw`/`Bc` figures are captured in the profile files but are
**not used anywhere in this document's reasoning**, for the circularity reason
given in the header.

## Allocation attribution

The question was whether the 257,790 allocations per cycle are a diffuse
sweep or a few dominant sites. **They are highly concentrated: the top four
sites are 95.9% of all allocation calls.**

Immediate-caller histogram, client allocation entry points (`malloc`, `calloc`,
`realloc`), 3 steady cycles, parsed from the profile's call edges:

| calls / cycle | share | cumulative | site |
|---:|---:|---:|---|
| 111,643 | 32.5% | 32.5% | `oxpinyin_data::dict::resolve_items` |
| 76,769 | 22.3% | 54.8% | `<alloc::raw_vec::RawVecInner>::finish_grow` (Rust Vec growth; caller set in profile) |
| 72,643 | 21.1% | 75.9% | `oxpinyin_data::chewing_table::decode_items` |
| 68,532 | 19.9% | 95.9% | `<&[u8] as CString>::new::SpecNewImpl` (the `sentence.rs` per-candidate `CString::new`) |
| 3,141 | 0.9% | 96.8% | `oxpinyin_data::chewing_table::index_key` |
| 3,141 | 0.9% | 97.7% | `oxpinyin_data::dict::syllables_to_chewing_keys` |
| 2,887 | 0.8% | 98.6% | `oxpinyin_user::lookup::index_key` |
| 1,400 | 0.4% | 99.0% | `oxpinyin_store::tkrzw::get_value` |
| remainder | 1.0% | 100% | graph build, `__rust_alloc`, scan matrix, dedup, … |

Total: 343,732 client malloc-family calls per cycle (profile). The gated Rust
counter reports 257,790 allocating calls and 25.06 MB per cycle over the same
region; the difference is the glib/C-side allocations the Rust counter cannot
see. Call counts are valid for both engines because one instrument produces
them symmetrically.

### `finish_grow` resolved: it lands on `resolve_items`

The 22.3% column is not a separate site — `<alloc::raw_vec::RawVecInner>::
finish_grow` is Rust's `Vec`-growth shim, so its callers are the real sites.
Walking the call graph through the shim (and through `do_reserve_and_handle`,
its other frame) resolves them:

| via `finish_grow` | calls/cycle | site |
|---|---:|---|
| `PhraseItemView::phrase_text` ← `resolve_items` | 71,644 | `phrase_libraries.rs:172` → `phrase_library.rs:339` |
| `oxpinyin_user::lookup::index_key` | 3,236 | user lookup |
| `RawVec<Candidate>::grow_one` ← `flush_window_batch` | 1,041 | candidate list |
| `RawVec<Edge>::grow_one` ← `SegmentGraph::fewest_keys` | 504 | trellis |
| `RawVec<ScanKey>::grow_one` ← `build_scan_matrix` | 325 | scan matrix |
| remainder | 19 | graph, prefix-exists |

So `resolve_items` owns **111,643 direct + 71,644 via `phrase_text` =
183,287 calls/cycle = 53.3%** of all client allocations. The answer to "is
this one site or a sweep" is unambiguous: **one function is the majority.**

Line-level attribution inside it (disassembly of the `resolve_items` symbol
with line tables; it is a real symbol, not inlined):

- **`phrase_libraries.rs:249`** — the 111,643 direct mallocs. Inside
  `pronunciation_possibility`, for each stored pronunciation it does
  `let stored: Vec<ChewingKey> = view.keys.chunks_exact(2).map(…).collect();`
  purely to hand a slice to `keys_match`. One heap `Vec` per pronunciation per
  item, ~39 per call, discarded immediately after the comparison.
- **`phrase_libraries.rs:172` / `phrase_library.rs:339`** — the 71,644
  `phrase_text` mallocs. `PhraseItemView::phrase_text` returns
  `Option<String>` (`self.phrase_chars().collect()`), and the result is handed
  straight to `PhraseEntry::new`, whose text is a `CompactString` that is
  inline for typical 1–4-CJK phrases — so the `String` is allocated and then
  dropped without ever owning the heap copy that survives.

Both are single-site, per-item, purely-lazy allocations with no observable
behaviour: the collected `Vec<ChewingKey>` is only ever read by `keys_match`,
and the `String` is only ever converted into the `CompactString` that the
existing code already uses. Replacing either with a borrowed iterator /
`with_capacity`-sized buffer changes no output. **This, not `hash_one`, is the
Phase 3 head item.**

`decode_items` (72,643 calls, 21.1%) is the other concentration, but it is
already `Vec::with_capacity`-shaped (`chewing_table.rs:120`) and its per-record
`keys` vector is inherent to the returned structure; it is a smaller,
structural cost, not a gratuitous temporary.

**This is a legitimate single item.** `resolve_items` is one function holding
53.3% of the allocation calls and 8.0% of total Ir.

### The `hash_one::<&String>` + SipHash path, and why it is not the head item

The 22.8 M Ir attributed to SipHash has exactly one caller in the profile:
`oxpinyin_engine::session::dedup_by_text_keep_first`, 205,608 calls. The
source declares `HashSet<&str>`; the profile labels the instantiation
`hash_one::<&String>`. That is not a discrepancy in the code: hashbrown's
`make_hash` goes through `Equivalent`/`Borrow`, so a `&str`-keyed set
instantiates the hasher with the borrowed `&String` type. Same set, same
behaviour.

The set is never iterated: `dedup_by_text_keep_first` builds it with
`HashSet::with_capacity`, does `seen.insert(candidate.text())` in a `map`, and
drops it; there is no `for`/`iter`/`drain` over `seen`. Its only external
effect is the `bool` insert result, which drives an order-preserving `retain`.
So hash iteration order is not an external behaviour surface here, and the
dedup result is order-stable by construction.

**It was the head item under the old motivation and is not any more.** The
histogram superseded it: `resolve_items` holds 53.3% of the allocation calls
against this path's 205,608/cycle (60% of the calls, but a different cost
shape), and more importantly the motivation for Phase 3 has changed. See the
next section.

## Why Phase 3 is now about allocation churn, not catching up

The original motivation was to close a gap: oxpinyin was assumed slower, so
reducing its work was how you caught up. On the shipped artifact there is no
time gap left — three windows put it at parity at both sizes. What remains
is the resource axis:

- **Instructions: +35.4%** (hardware counter, this host).
- **RSS per cycle: 1.23×** (this session: oxpinyin 20,730 KiB vs libpinyin
  16,828 KiB steady; the init snapshot is the same 1.24×, so it is resident,
  not transient). The record's own arm64/amd64 tables showed the same
  ~1.20–1.22×, arch-invariantly.
- **Wall clock: parity** — with an unresolved tree question (points positive
  on amd64, negative on arm64, not established on either; §"The three-window
  control").

So the remaining cost is paid in memory and instructions, not time, and
**allocation churn is the plausible common cause of both**: the +35% instruction
count is dominated by the allocator and copy family (26.6% of Ir), and the
1.23× RSS tracks how much is allocated. That is a *weaker* motivation than
"catch up to libpinyin" — it is a Stage-2 efficiency argument, not a parity
one — and whether it justifies a change is the maintainer's call. It is
recorded here as the reason Phase 3 has a head item at all, and the head item
is `resolve_items` (`phrase_libraries.rs:249` and `:172`), not `hash_one`.

**This section stays as-is after the three-window control, deliberately.**
The control changed the wall-clock bullet's parenthetical ("3–5% regression"
→ "unresolved tree question") and nothing else, because the section's
argument rests on the instruction and RSS axes, which the control did not
touch, and because the pre-committed rule forbids updating the tree
motivation in either direction on unestablished data: the regression is
neither confirmed nor refuted-to-neutral, so there is nothing to move the
motivation to. Better data, not this run.

This document proposes no change. Phase 3 awaits a human decision.

## Environment

| Property | Value |
|---|---|
| Host | the record's amd64 host: Intel i7-9750H, 6C/12T SMT on, kernel `6.12.0-211.22.1.el10_2.x86_64`, MemTotal 15,912,492 kB, same boot as the record |
| Container (Ir pass) | x86_64, `debian:testing@sha256:dab11cdb…`, apt snapshot 20260831 |
| Container (control) | `localhost/oxpinyin-matrix:knob-amd64` — **the record's own image**; its `/repo` verified byte-identical to `f79f665d` |
| Toolchain | rustc/cargo 1.97.1, gcc 15.3.0, valgrind 3.27.1, libtkrzw 1.0.32 |
| Oracle | libpinyin **2.11.92**, pin `074a2219c90feaf962d0d24f034514033ece5f99` — **the operational pin in `build-oracle.sh` / `Dockerfile.perf-matrix` / the record; NOT the stale `tools/oracle/oracle-pin.txt` value (2.11.91 / `0c5e80e1`)** |
| Instrument commit | Ir pass `be32253b`; control `787ef383` |
| Artifacts | lp `f1f56fa6…`; ox debug `6b30453a…`; ox no-debug `7b60a63e…` (= control's Z); **product S `435fab75…`**; ox alloc-count `9038967e…`; control X `bf8d3b57…`; control Y `2cdf2fd9…` |
| Capture windows | Ir pass 2026-09-08T15:58:52Z; one-window control 2026-09-09T00:03:16Z; shipped check 2026-09-09T00:20:24Z; **three-window control 2026-09-09T04:10Z–04:52Z (W1, W2) and post-reboot W3r ~13:5xZ** — the host rebooted between W2 and W3r |
| perf_event_open | works on this host; inside the container it needs `--security-opt label=disable --security-opt seccomp=unconfined` (SELinux + seccomp deny it by default; the host PMU is open to the user) |

**Host load at the control.** The record ran under a <3%-CPU co-tenant
condition. The control's 1-minute load ran **4.84 → 3.83** (n=1), **3.83 →
2.35** (n=8), **2.35 → 2.29** (n=16) — the load was this session's own
processes plus ~3% of co-tenant containers, but it was not the record's quiet
condition. This is recorded, not absorbed: the round-robin design protects the
within-session terms (tree, recipe, and every Z/X/Y/L ratio — all four cells
were measured seconds apart under identical conditions), but the **X÷L
reproduction gate is the one cross-session comparison exposed to it**, and its
n=8 0.0397 gap against the record is the place a load difference would show.
The n=16 agreement to 0.0015 argues the load did not dominate.

Debug-info neutrality re-checked under tkrzw and required by the amended
instrument: identical normalized instruction multiset (208,864 instructions,
30,816 distinct forms); 1,065 symbols pair by hash-stripped name (733
relocated), 13 pair by identical instruction body — 1,078 text symbols all
accounted for. Case (b): Ir totals are safe, simulated cache/branch figures
are not.

## Captures

Ir pass written to `/tmp/cg-out` (not committed): `environment.txt`,
`anchors.txt`, `debuginfo-neutrality.txt`, `malloc-interception.txt`,
`callgrind.{libpinyin,oxpinyin}-tkrzw.r{1,2}.out` and their
`.annotated.txt`/`.inclusive.txt`, `hw-instructions.jsonl`/`.md`,
`alloc-count.txt`, `timing.md`, `speed.jsonl`, plus `t-control/` and `t-sweep/`.
One-window control written to `/tmp/control-out`: `environment.txt`, `build-identity.txt`,
`load.txt`, `speed.jsonl` (240 rows), `ratios-n{1,8,16}.md`,
`decomp-n{1,8,16}.md`. Three-window control written to `/tmp/amd64-w{1,2,3}/`
(300 rows each) plus the reboot-interrupted partial `/tmp/amd64-w3-partial/`
(210 rows), none committed. Shipped check written to `/tmp/shipped-out`:
`shipped-identity.txt`, `speed-shipped.jsonl` (120 rows: L, Z and **S** at
n=8/16, all cells unimodal), `shipped-ratios-n{8,16}.md`. The product ratio
S÷L and the supporting S÷Z are both computed from that one capture with
`perf-ratio.py`.

## Scope and exclusions

- **The candidate sort is excluded in every phase**, regardless of ranking. It
  ranks second here (50.0 M Ir, 9.0%: `driftsort` ×2 + `quicksort` ×2, and on
  the libpinyin side `compare_item_with_sort_option` 22.2 M). Recorded, not
  touched: sort stability is parity-relevant and adjacent to the frozen
  491/396/390 sentence surface.
- The tkrzw user-db open cost, the default-backend question, the latent
  `RUSTFLAGS`-vs-profile trap, and a CI workflow for this capture are out of
  scope and recorded, not done.
- The stale `tools/oracle/oracle-pin.txt` entry is recorded, not fixed; it
  needs its own small PR.
- **Recorded, not fixed — the record's recipe split:** it builds its
  size-comparison artifacts with `cargo cinstall` in `Dockerfile.perf-matrix`
  but its *timed* artifacts with `cargo build` + strip at run time in
  `run-perf-same-data.sh`. Its environment table's `cargo build` row describes
  what was timed and is right; the image's own artifacts are a different
  recipe. Worth its own note.
- **Recorded, not fixed — the n=1 bimodality:** in the control, cell
  `oxpinyin-cur-cargo` at n=1 is BIMODAL by the locked rule (max adjacent
  ratio 1.433 — one 42.79 ms run against a 24.7–29.9 ms body); the other 13 of
  14 cell×size combinations are unimodal. n=1 figures for that cell are
  therefore excluded from the verdict; n=8 and n=16 carry it. The record's own
  amd64 n=1 was likewise bimodal in all four cells.
- No Phase 3 change is included in this commit series. This document records
  the differential and the verdict only.

## Phase 3 candidate, for the next step

The head item is **`resolve_items`**: `phrase_libraries.rs:249` (a `Vec<ChewingKey>`
collected per pronunciation only to feed `keys_match`) and `:172` /
`phrase_library.rs:339` (an `Option<String>` from `phrase_text` that becomes an
inline `CompactString` and is dropped). Together 53.3% of client allocation
calls; behaviour-lazy; no output change.

`hash_one`/`dedup_by_text_keep_first` is no longer the head item — the
histogram superseded it and the motivation changed (see "Why Phase 3 is now
about allocation churn"). `CString::new` (19.9% of calls) remains a valid
second item.

Awaiting a human go-ahead before any code change. **No Phase 3 work is in this
commit series.**

## arm64: unmeasured in the amd64 session

The record's arm64 1.158/1.157 came from the same superseded tree as its
amd64 ≈ 1.04, so at the time of the amd64 capture the current tree had **no
valid arm64 measurement at all** — the arm64 section below supplies it.
Re-running it was attempted in the amd64 session: that host has
no arm64 execution path — no `binfmt_misc` handler registered, no `qemu-user`
package, no `qemu-aarch64` binary, no remote arm64 machine — so it could not be
taken there. It needs an Apple-silicon (or other arm64) host.

The instrument is ready for it. `Dockerfile.callgrind` selects `rustup-init`
per build arch and carries the arm64 checksum; the driver now **degrades
gracefully when there is no PMU** (Apple containers have none): stage 5 prints
`NO HARDWARE COUNTERS`, records the distortion as unquantified, and the capture
continues through the remaining stages instead of aborting. On such a host the
`--security-opt` flags in the header are unnecessary.

What an arm64 run can and cannot settle: the Ir differential is
architecture-specific, so it must be measured there rather than inferred. The
arm64 memory-model instructions the stall hypothesis concerns do not exist in
this amd64 stream, so the amd64 capture above could not close the arm64
question in either direction. The arm64 pass below is the capture that
answers it.

## arm64: the spread, measured on the current tree (2026-09-08, Apple M5)

**Verdict, first line.** On native aarch64 — in a Docker Desktop VM on the
same Apple-silicon Mac the record's arm64 pass used, platform-verified below
— the current tree still runs oxpinyin **slower** than libpinyin on the
steady keystroke cycle: **T = 1.0997 / 1.1005 / 1.1118** across three
independent capture windows, every window's 95% CI entirely above 1.0.
Against this document's amd64 product ratio (one-window 0.9977 / 0.9655 at
the time of this pass; three-window parity, 0.9664–0.9948, since — see
§"The three-window control") on the same tree, that is a **~10–14-point
architecture spread on identical source** — the headline finding of the
arm64 pass, recorded here before the callgrind Ir numbers exist. Both passes are cinstall artifacts, so the spread
is same-recipe up to the arm64 artifact's unverified `shipped` flag (see the
consistency check above).

### Protocol, as pre-registered before the 20-run data

A 5-run × 3-cell probe (2026-09-08T17:08Z) sized the noise first: ratio CI
widths 0.126 (no-debug) / 0.150 (debug) at 5 runs, projecting ≈ 0.063 / 0.075
at 20 runs — comfortably inside the abandonment bounds, so no run-count
increase. Locked before any 20-run number existed:

1. **Abandonment (width):** any window's ratio CI width ≥ 0.20 → verdict
   "unmeasurable on this hardware", no performance verdict.
2. **Abandonment (dispersion):** range of the three windows' point estimates
   > 0.05 → "unmeasurable", regardless of CI widths. Rationale: the 1/√n
   projection assumes stationary noise, but this platform's noise source is a
   person using the computer; a 4-second window can be swallowed whole by one
   scheduling event and produce a CI that is narrow and wrong.
3. **Callgrind trigger (merged rows 3+4 of the original rule):** all three
   windows' ratio CI lower bounds > 1.0 → run the callgrind differential.
   The original T ≥ 1.10 threshold was retired before measurement because
   the probe's point estimates (1.1163 / 1.1021) sat exactly on it — the
   decision would have been a coin flip between two very different actions,
   and the arch spread itself is the finding, so T's exact value no longer
   decides whether the result is worth chasing.
4. Bimodality by the locked rule: sort each cell's 20 per-run steady medians;
   largest adjacent ratio ≥ 1.15 → bimodal → no clean point estimate from
   that cell.

### The three windows

`PERF_CYCLES=8`, `PERF_REPEATS=1`, 20 rounds × 3 cells round-robin per
window, `taskset -c 0`, both engines opening libpinyin's own installed data
directory (drop-in configuration). Ratios from the committed
`tools/bisection/perf-ratio.py` (whole-run percentile bootstrap, 10,000
resamples, seed 20260907).

| window | UTC window | host load¹ | lp ms | ox nodebug ms | **T nodebug [95% CI]** | T debug [95% CI] |
|---|---|---|---:|---:|---|---|
| W1 | 17:15:31–35 | 3.46 | 7.984 | 8.780 | **1.0997 [1.0926, 1.1058]** | 1.1026 [1.0947, 1.1089] |
| W2 | 17:16:27–31 | 2.64 | 7.957 | 8.757 | **1.1005 [1.0924, 1.1084]** | 1.0994 [1.0922, 1.1075] |
| W3 | 17:17:21–25 | 2.44 | 7.932 | 8.819 | **1.1118 [1.1051, 1.1231]** | 1.1044 [1.0956, 1.1156] |

¹ 1-minute load average sampled immediately before each window.

Gate outcomes: maximum CI width 0.0200 (≪ 0.20); three-window point-estimate
range 0.0121 (< 0.05); all three lower bounds (1.0926, 1.0924, 1.1051) > 1.0
→ **measurable, and the callgrind differential runs**. Every gate was passed
with an order of magnitude to spare.

### Bimodality: all nine cell×window combinations unimodal

Max adjacent ratio over the sorted 20 per-run steady medians:

| window | libpinyin-tkrzw | oxpinyin-nodebug | oxpinyin-tkrzw (debug) |
|---|---:|---:|---:|
| W1 | 1.0648 | 1.0123 | 1.0134 |
| W2 | 1.1160 | 1.0486 | 1.0268 |
| W3 | 1.0097 | 1.0758 | 1.0101 |

All < 1.15. The largest value in each column is a single mild outlier run
(one E-core landing, e.g. W2 lp 9.01 ms vs the 7.7–8.1 ms bulk; W3 nodebug
9.64 ms vs the 8.7–9.0 ms bulk) — the median is robust to them and the
locked rule classifies every cell unimodal. The P/E hazard was live (the VM
exposes all 10 logical cores of a 4P+6E die as vCPUs) but did not materialise
into bimodality at this load level.

### The factor is still constant — constant above 1

A `PERF_REPEATS` sweep (20 rounds × 3 cells, one window per size) confirms
the record's structural claim with the level moved from 1.158 to ≈ 1.10:

| workload n | lp ms | ox nodebug ms | ox debug ms | per-unit ratio (nodebug) [95% CI] |
|---:|---:|---:|---:|---|
| 1 (3-window median) | 7.957–7.984 | 8.757–8.819 | 8.748–8.803 | 1.0997–1.1118 |
| 8 | 63.298 | 70.249 | 70.172 | 1.1098 [1.1029, 1.1232] |
| 16 | 127.041 | 141.358 | 140.700 | 1.1127 [1.1048, 1.1192] |

The per-unit ratio varies by 1.2% across a 16× workload range — a constant
factor, on the same tree where amd64's is constant at ≈ 0.95–0.97. Whatever
the arch-specific cost is, it is per-unit and proportional, not a threshold
or a startup effect.

### RSS: the same ~1.16–1.19× as amd64's 1.23×

Median over 10 runs per cell per mode (`ram-init` = after init snapshot,
`ram-cycle` = after last steady cycle):

| cell | init KiB | steady KiB | init ratio | steady ratio |
|---|---:|---:|---:|---:|
| libpinyin-tkrzw | 11,644 | 17,004 | 1.0000 | 1.0000 |
| oxpinyin-nodebug | 13,780 | 19,790 | 1.1834 | **1.1638** |
| oxpinyin-tkrzw (debug) | 14,106 | 20,192 | 1.2114 | 1.1875 |

amd64 measured 1.23× steady (20,730 vs 16,828 KiB) in the section above;
arm64 lands at 1.16× (shipping artifact). The RSS axis behaves
arch-invariantly — oxpinyin is ~16–23% more resident everywhere — so it does
not explain the arch spread in T.

### Machine identity

The record's arm64 environment row matches every recordable identifier on
this machine byte-for-byte: kernel string `Linux 7.0.12-linuxkit #1 SMP
PREEMPT Fri Aug 14 16:27:59 UTC 2026 aarch64`; VM MemTotal 8,124,516 kB
(re-read today: identical); Docker 29.7.2; darwin 27; rustc build hash
`8bab26f4`; 10 vCPUs. The record never wrote down the Mac model or chip, so
chip-level identity is unverifiable — consistent with the same physical
machine, provable no further. The record's capture window (2026-09-07T13:20Z)
falls inside the current boot (uptime 6 days at capture). Consequence: the
within-session ratio that the verdict rests on is unaffected by identity;
"the rewrite moved the ratio" versus "a different machine answered" is
separable exactly to the extent the record permits — which is every recorded
identifier, and not the chip.

### Environment

| Property | Value |
|---|---|
| Host | Mac17,2, **Apple M5**, 4 P-cores + 6 E-cores (10 logical), 16 GiB, macOS 27.0 (26A5425a), AC power, low-power mode off |
| Container runtime | Docker Desktop 4.88.1 (engine 29.7.2), linuxkit VM, 10 vCPUs, MemTotal 8,124,516 kB |
| Container arch | `aarch64` = host `arm64`; base `debian:testing@sha256:dab11cdb0a9dcf4bbd68f671635b35f1f726b452b92396875b69bb2c7daa42a9` resolving to `arch=arm64 os=linux`, digest present in RepoDigests |
| Emulation controls | no qemu handlers in `binfmt_misc`; no `--platform` in any command; no qemu in any process tree |
| Kernel (container) | `Linux 7.0.12-linuxkit #1 SMP PREEMPT Fri Aug 14 16:27:59 UTC 2026 aarch64` |
| CPU (container view) | CPU implementer 0x61 (Apple), CPU part 0x000, revision 0; `nproc` 10 |
| Toolchain | rustc/cargo 1.97.1 (`8bab26f4` / `c980f486`), gcc (Debian 15.3.0-2) 15.3.0, valgrind 3.27.1, libtkrzw 1.0.32, `perf_event_paranoid` 2 (no PMU regardless — see callgrind section) |
| Oracle | libpinyin **2.11.92**, pin `074a2219c90feaf962d0d24f034514033ece5f99` — the operational pin in `build-oracle.sh` / the Dockerfiles; **not** the stale `tools/oracle/oracle-pin.txt` (2.11.91 / `0c5e80e1`) |
| Instrument commit | `9b5a89fe` (= `b0271a5f` + two instrument commits: `bac1d0dd` `perf-ratio.py`, `9b5a89fe` cargo-jobs cap); every number in this section comes from the image built at this commit. The callgrind differential additionally ran with the neutrality fix `91bb4f58` bind-mounted over the baked copy — artifacts, harness and every other stage byte-identical (stage 0 re-printed the same artifact SHAs). Post-`CARGO_BUILD_JOBS` layers built fresh; only the pinned apt/rustup layers were served from local cache of identical text |
| Artifacts (unstripped) | lp `74ccbeda82aec3a1…` 5,545,536 B; ox debug `c4964e34847e3c80…` 9,213,328 B; ox nodebug `c19629f82f7c4470…` 1,816,024 B; all `NEEDED libtkrzw.so.1` |
| Capture windows | probe 17:08:33–34Z; W1 17:15:31–35Z; W2 17:16:27–31Z; W3 17:17:21–25Z; RSS 17:18:37–39Z; sweep n=8 17:18:39–17:19:12Z, n=16 17:19:12–17:20:18Z (all 2026-09-08, UTC) |

### Interference record (the measurer lives on the measured machine)

Host 1-minute load: 4.80 → 3.46 (pre-W1, after an Apple updater burst
`UARPUpdaterService*` was allowed to finish) → 2.64 (pre-W2) → 2.44 (pre-W3)
→ 2.69 (post-sweep). Top consumers at every sample: WindowServer 29–49%,
this session's ZCode Helper processes 18–53%, and — before the capture
windows — Firefox/plugin-container 11–22% with active video decode; Firefox
was closed before W1 at the operator's direction. The ~4-second windows are
too short to register in load averages; the honest limitation is that no
without-me baseline exists (the sampler is itself the session), so the
"baselines" bound session-active, not true idle. The three-window design and
the round-robin within each window are the controls that carry the verdict:
both cells of every ratio were measured seconds apart under identical
co-tenant conditions. This is a noisier box than the amd64 host's <3%-CPU
idle containers, and the intervals should be read with that asymmetry in
mind.

### Scope

Drop-in configuration only: no train, no sentence decode, oxpinyin reading
libpinyin's own installed data directories, tkrzw backend, `PERF_CYCLES=8`
per the record protocol. Nothing here measures oxpinyin's generated data or
the training/sentence paths. The ordered-atomics audit that this finding
motivates — aarch64 paying `ldar`/release barriers where x86-64's TSO makes
acquire/release free — is a **separate task, recorded, not started**.

### Captures

Written to `/tmp/arm64-out` on the host (not committed):
`probe-n1/`, `t-n1-w{1,2,3}/`, `ram/`, `t-n8/`, `t-n16/` (each with
`speed.jsonl` and per-cell stderr), `probe.sh`, `window.sh`, `rss-sweep.sh`,
and `cg/` (the full differential output tree).

### The callgrind Ir differential

The merged trigger fired — all three windows' ratio CI lower bounds above
1.0 — so the differential ran, T first having already been captured above.

**Instrument incident, recorded.** The driver's first run aborted in stage 2:
the debug-info neutrality check classified the arm64 artifact pair as case
(c) "different code" because both of its link-order normalizations were
x86-only spellings — branch targets matched `j*`/`loop`/`call` (arm64 uses
`b`, `b.cond`, `bl`, `cbz`…) and PC-relative data matched `(%rip)` (arm64's
analogue is `adrp`, whose objdump annotation embeds the crate-disambiguator
hash). The diff lines differed only in branch-target addresses and `Cs…`
hashes inside annotations — the documented case-(b) effect. Fixed in
`91bb4f58` by adding the aarch64 equivalents of exactly those two documented
normalizations (indirect `br`/`blr` stay verbatim; case (c) remains fatal and
unweakened). The fix was applied to the differential run by read-only bind
mount over the baked copy; the image, both artifacts and the bisect binary
are unchanged from the timing session — stage 0 re-printed identical
artifact SHAs (`74ccbeda…`, `c4964e34…`, `c19629f8…`). After the fix the
check classifies **PASS (b)**: identical instruction multiset (174,474
instructions, 19,277 distinct forms), 1,155 symbols pair by hash-stripped
name (668 relocated), 10 leftovers all paired by identical instruction body.

Recorded, not fixed here: the neutrality check had been amd64-only-correct
since it was written — the same defect class as the latent
`RUSTFLAGS`-vs-profile debuginfo trap in `Dockerfile.perf-matrix`: a
single-arch assumption baked into a tool that is cross-arch by contract.
Both instances argue for an AGENTS.md line on the pattern; that is a
separate PR, and this capture fixes only the instance it hit.

**Stage 5 degraded exactly as designed, confirmed live:** the hardware
counter stage printed `NO HARDWARE COUNTERS` (`hw_errno=1`, EPERM) and the
capture continued through stages 6–7 instead of aborting. Apple-silicon
containers have no PMU; the callgrind-vs-hardware distortion cross-check is
unavailable on this host, permanently.

| Quantity | Value | Note |
|---|---|---|
| **I_cg** (callgrind Ir ratio, ox debug ÷ lp) | **1.1916** | toggled to the steady anchor, 3 steady cycles per profile, round 1; round 2 identical to 4 decimals |
| Determinism | lp +0.0000% (6 of 432,137,650), ox −0.0000% (−138 of 514,946,539) | two rounds, far under the 0.1% report threshold |
| Ir per steady cycle | lp 144,045,883; ox 171,648,846 | callgrind view, allocator un-intercepted (below) |
| Allocations per steady cycle (stage 6, native) | 257,790 calls, 25.06 MB | **identical to the amd64 figures** — allocation behaviour is arch-invariant |
| T in the same session (driver stage 7, debug artifact) | 1.1059 [1.0984, 1.1156] | fourth consistent timing point; `I` and `T` from one binary |

**Malloc interception is NOT in effect on this arm64 valgrind** — the
opposite of the amd64 session. Stage 3's probe credits allocation Ir to real
glibc symbols in `libc.so.6` (`calloc`, `__libc_calloc2`, `_int_realloc`,
`realloc`, `alloc_perturb`); no vgpreload object appears among the profile's
objects. Consequence: the allocator-replacement understatement that helped
compress amd64's `I_cg` (1.0400 against a hardware-counter 1.3537) is
structurally absent here — both cells' allocation cost is glibc code under
translation. The CPUID/HWCAP-masking axis, the other half of the amd64
distortion, remains unmeasurable without a PMU.

**The bias note, stated flatly: the amd64 cross-check measured callgrind
understating the real instruction gap by thirty points, and no PMU exists on
this host to repeat it. `I_cg = 1.1916` therefore carries an unquantified
net bias of unknown direction — one known distortion (replacement
allocator) verifiably absent, the other (masked CPU features) unquantified
— and no stall verdict may rest on it.**

**The two I_cg numbers are not comparable and must not be read side by
side.** amd64's 1.0400 was collected with the replacement allocator active
and was measured against a hardware counter showing the true gap thirty
points higher (1.3537). arm64's 1.1916 was collected with interception
provably inactive — the one known compression is structurally absent from
it — leaving only the unquantified CPUID/HWCAP axis. Of the two, the arm64
figure is the structurally closer to ground truth. A juxtaposition like
"amd64 ≈4% vs arm64 ≈19% instructions" treats two differently-biased
instruments as one and will mislead the next reader; this paragraph exists
so it cannot.

### An indicative IPC comparison — inference, labelled as such

Reading each pass's instruction ratio against its own wall-clock ratio:

| pass | instruction ratio | wall ratio | IPC ratio (ox ÷ lp) |
|---|---|---:|---:|
| amd64 | 1.3537 (hardware counter, native) | 0.9527 (nodebug, prior session; the amd64 section's own 1.42 derivation) | ≈ 1.42 |
| arm64 | 1.1916 (`I_cg`, allocator un-intercepted) | 1.1059 (debug — the same binary the Ir came from) | ≈ 1.08 |

The shape of the arch spread is an **IPC-advantage collapse, not an
increase in work**. On amd64, oxpinyin retires ~42% more instructions per
cycle than libpinyin and converts that into a 4% wall-clock win; on arm64
it retires only ~8% more per cycle and loses ~10% of wall clock. The work
is arch-invariant (the common-mode exclusions below); what collapsed
between the two passes is how cheaply the core retires it.

The label, plainly: the arm64 leg rests on `I_cg` being roughly credible,
with no PMU to cross-verify it. The one measured bias direction —
callgrind *understating* the instruction gap, thirty points on amd64 —
would, if it held here too, make the true Ir gap larger and the true IPC
advantage smaller: the collapse deeper, not shallower. The inference is
direction-robust against the known bias. It is recorded because it is the
most explanatory single sentence this round produced — not because it is a
measurement.

### The common-mode exclusions: the work is arch-invariant

Three quantities were measured on both architectures, and none of them
moved:

1. **Allocation churn — identical, verbatim.** The native gated counter
   reports **257,790 allocating calls and 25.06 MB per steady cycle** on
   amd64 and on arm64 alike (stage 6 of each pass). The same code
   allocates the same buffers in the same places on both cores.
2. **Resident memory — same order.** 1.1638× (arm64 steady, shipping
   artifact) against 1.23× (amd64): the RSS axis is arch-invariant to
   within session noise.
3. **Workload scaling — constant on both.** The per-unit ratio is flat
   across a 16× sweep on arm64 (1.1098–1.1127) exactly as on amd64
   (prior session 0.9527–0.9707; controlled cinstall 0.9626–0.9805): no
   threshold, no startup term, the same per-unit structure everywhere.

Together these exclude allocation churn, memory footprint, and
algorithmic/work-structure differences as causes of the arch spread. The
work is the same work; what differs between the passes is what the core
pays to execute it. That is precisely the class of residual — per-
instruction execution cost, the class that barrier/fence retirement
serialization belongs to — that the ordered-atomics hypothesis names, and
it is why the pointer below goes where it goes.

### What the arm64 pass establishes, and the pointer it leaves

On the current tree, same source, same protocol, same oracle pin: amd64
product ratio at parity (three-window 0.9664–0.9948, 1.0 inside every n=16
CI), arm64 T_nodebug = 1.0997–1.1118 (measurably slower, constant across a
16× workload sweep). The ~10–14-point architecture spread is the finding of
this round. It is the first
solid evidence for the memory-ordering hypothesis — aarch64 paying `ldar`/release
barriers where x86-64's TSO makes acquire/release effectively free, which a
wide out-of-order core hides less well than extra ALU work — a hypothesis
that until now rested on weak evidence and was explicitly set aside. **The
audit of ordered atomics on the per-candidate path is a separate task,
recorded here as the pointer, not started.** No change under `crates/` is
part of this capture; the standing Phase 3 allocation candidates
(`phrase_libraries.rs:249` and `:172`) remain amd64-host work, unchanged in
priority by anything measured here.

## arm64: the same-recipe control (2026-09-09, Apple M5)

**Verdict, first line.** With the build recipe held constant, the shipped
artifact is still measurably slower than libpinyin on arm64 — **S÷L =
1.1123 / 1.1048 / 1.1090 at n=8, every window's CI entirely above 1.0** —
and the arch spread survives on same-recipe terms at n=8 (no interval
overlap with amd64's one-window S÷L 0.9977 [0.9582, 1.0230]; with amd64's three-window control the comparison is symmetric and still non-overlapping — see §"The three-window control"); the **recipe effect
transfers** (−0.036 in all three windows, every CI excluding zero), the
**tree regression does not transfer** (not established; point direction
inverted), and **n=16 is permanently not measurable on this hardware**
under the locked rules — one externally interrupted window and one
replacement whose S cell was bimodal, both recorded below. (That last
classification is superseded for the replacement window by the disclosed
rule revision at the end of this document: under the corrected rule n=16
is measurable and corroborates everything above.)

### Cells and identity

Five cells, one container session per window, the record's own image
(`oxpinyin-matrix:knob`, local id `sha256:9b3f50e4…` — byte-exact the id the
record names; created 7 minutes before the record's capture window), its own
data directory, its own harness (`bisect.c` from its `/repo`, sha
`9c5f2cbe…`):

| cell | tree | build | sha256 (prefix) | size | notes |
|---|---|---|---|---:|---|
| L | libpinyin pin `074a2219` / 2.11.92 | in-image | `fabc68fa…` | 789,560 B | record's own install |
| X | the record's arm64 tree (`/repo`, engine byte-identical to `50afb7f6` and to `f79f665d`'s `crates/`) | `cargo build` + `strip --strip-all` | `8500f4d1be75dfae…` | 1,577,696 B | **gate passed** |
| Y | current (`4cc53194`) | `cargo build` + `strip --strip-all` | `7da55270…` | 1,577,696 B | |
| Z | current | `cargo cinstall` (tkrzw) | `c19629f8…` | 1,816,024 B | **byte-identical to the 2026-09-08 pass's timed artifact** |
| S | current | `cargo cinstall` (tkrzw,shipped) | `9b3214bc…` | 1,815,240 B | fixture hooks: Z 2, S 0 |

**The X gate, and why it is bounded rather than open.** The record truncates
its artifact hash (`sha256:8500f4d1be75dfae…`), so the gate is a 16-hex
prefix plus size plus `NEEDED libtkrzw.so.1` — weaker than amd64's
full-sha gate. But X is rebuilt *inside the record's own image, from its own
`/repo`, with its own toolchain and cargo cache*: the gate checks that an
in-situ rebuild reproduces in situ, and a prefix match under those
conditions is sufficient. **Size must not be read as corroborating the gate**:
X and Y came out at exactly 1,577,696 B despite the ~6,300-line engine
rewrite between them — aarch64's 64 KB section alignment quantizes total
file size, where on amd64 the same pair differed by 7,680 B. Size is a much
weaker provenance discriminator on aarch64 than on x86-64, and this repo's
records use sizes in provenance rows — a general caveat, recorded here.

**Z's byte-identity** upgrades "the 2026-09-08 arm64 pass was Z-recipe" from
inference to verified fact: same engine tree (the commits between `b0271a5f`
and `4cc53194` touch only `tools/` and `docs/`), same `Dockerfile.callgrind`
recipe, byte-identical output. The previous section's ≈1.10 was therefore a
Z÷L measured on exactly these bytes, and the cross-session comparison below
is same-bytes.

**Build time, explained.** All four builds completed in ~40 s of wall time
(cargo's own timings: 8.49 / 7.10 / 11.68 / 11.36 s) against a 40–60 min
Phase-1 estimate. The estimate was anchored to the amd64 host's build times;
the deviation is real but not anomalous: the record image carries a warm
cargo registry cache (no network fetches), each build used a fresh `mktemp`
target directory so nothing was skipped (full `Compiling` lists in the log),
and the M5 compiles the ~60-crate workspace at `jobs=4` quickly. The gates
and Z's byte-identity say the builds were real.

### Windows, gates, and the n=16 outcome

Three windows, 20 rounds × 5 cells round-robin each, sizes n=8 and n=16,
`PERF_CYCLES=8`, `taskset -c 0`, host 1-min load recorded before each
(2.75 / 3.28 / 3.34). Window 1's first attempt aborted on a
`/lib/lib/` path typo in the driver before producing any rows — zero data,
so no selection question — and was rerun cleanly; recorded plainly so the
window timeline has no silent gap.

- **n=8: all 15 cell×window combinations unimodal** by the locked rule
  (max adjacent ratio ≤ 1.150); S÷L CI widths ≤ 0.0381 (< 0.20);
  three-window point range 0.0075 (< 0.05). **Measurable; the verdict
  above stands on these.**
- **Window 2, n=16: all five cells bimodal** (1.58–1.70) with an identical
  19+1 structure — one round of each cell hit by a documented external host
  interruption (load 8.73 recorded at the window's end; L's spike round
  249.2 ms against a 129.3 ms bulk mode, S's 293.2 ms against 143.1 ms).
  S÷L width 0.2427 ≥ 0.20 → the pre-registered width rule fired.
- **The replacement, pre-committed before it ran:** rerun window 2 in full
  under quiesced conditions (pre-load 1.63), used only if it passes both
  abandon rules and the bimodality check; a second failure makes n=16
  permanently not measurable; both runs go in the record. The justification
  for replacing the window is the **documented external interruption, not
  the numbers** — this is not a re-roll, and n=8 was already decided either
  way. Outcome: the replacement's S cell at n=16 was **bimodal** (max
  adjacent 1.1808; again 19+1 — one 168.6 ms round against a 139.8–142.8 ms
  bulk). **Gate failed → n=16 is permanently not measurable on this
  hardware.** Both windows are retained above with their evidence.
  (Superseded for this replacement window by the disclosed rule revision
  at the end of this document; the original W2 block stays excluded under
  either rule, on width alone.) The
  honest reading: each n=16 block offers only 20 rounds of ~1 s timed work
  per cell, and this machine's co-tenant noise injects ≥1.15× single rounds
  often enough that three certified-clean windows are not obtainable while
  the measurer lives on the measured machine.

For context only — not a verdict, n=16 carries none — every clean n=16
window agrees with n=8 (W1 1.1019, W3 1.1042, W2-replacement 1.1048, all
above amd64's then-current n=16 interval [0.9423, 0.9998]; amd64's three-window control later put n=16 at 0.9664–0.9948 with 1.0 inside every CI).

### Effects at n=8, and how they may be compared to amd64's

Pre-registered thresholds: an effect is *established as transferring* only
if its CI excludes zero in **all three windows** with consistent sign;
anything less is **"not established"**, never "smaller than amd64's".

- **Recipe effect (Z÷L − Y÷L): −0.0361 [−0.0399, −0.0309] /
  −0.0361 [−0.0456, −0.0288] / −0.0375 [−0.0503, −0.0225]** — all three
  exclude zero, sign consistent → **established**. `cargo cinstall` is
  faster than `cargo build`+strip on arm64 too, at roughly half of amd64's
  three-window −0.064 to −0.087.
- **Tree effect (Y÷L − X÷L): −0.0097 [−0.0167, +0.0043] /
  −0.0123 [−0.0228, −0.0046] / −0.0003 [−0.0141, +0.0119]** — two of three
  contain zero → **not established**. The point estimates are negative in
  all three windows, opposite in direction to amd64's (positive in all six
  of its windows): the effect is not established on either host, and the
  point-direction asymmetry across architectures is itself a finding, with
  its size unresolved (see the amd64 three-window control for the
  refutation of the +0.046 magnitude).

**Design asymmetry — removed.** This caveat was written when amd64 had one
window and a separate S session; the amd64 three-window control (2026-09-09)
now matches this design on both axes, so cross-arch contrasts below are
symmetric three-window comparisons and the caveat no longer applies
anywhere. Retained here only as the record of what it was.

### The shipped flag — open at this precision, no longer an amd64-vs-arm64 contradiction

arm64: **0.9990 / 0.9986 / 0.9952**, three windows — no measurable
difference here. amd64's three-window re-run: 1.0178 / 1.0214 / 1.0089 at
n=8, every CI containing 1.0, prior single-window value 1.0349 not excluded
— **not measurable at this precision; the prior neither confirmed nor
excluded** (the pre-committed classification's category 4, not a null). The
flag removes two fixture hooks (verified: Z exports both, S exports
neither), and removing code should not cost 3.5% — but the data on neither
host licenses calling the amd64 single-window +3.5% wrong, only
unconfirmed. The contrast is recorded as open at this precision. The
follow-up this paragraph called for — a three-window S/Z/L run on the amd64
box — **has run**; its outcome is the category-4 result in §"The
three-window control", and the headline stays on S÷L per the pre-committed
rule.

### Reproduction and calibration

- **X÷L** (the record's own recipe on the record's own engine bytes):
  1.1592 / 1.1548 / 1.1522 at n=8 and 1.1487 / 1.1463 at n=16 (clean
  windows), against the record's arm64 per-unit ratios of 1.158 (n=1) and
  1.157 (n=16) — the record's arm64 result reproduces within ~1%.
- **Z÷L vs the 2026-09-08 pass** (byte-identical artifact, so purely
  cross-session): 1.1098 → 1.1064–1.1144 at n=8, 1.1127 → 1.1056–1.1065 at
  n=16. Drift ≲ 1 point on this machine, comparable to amd64's ≈ 1 point.
  Calibration only, never a decision input.

### Environment

| Property | Value |
|---|---|
| Host / VM | Mac17,2, Apple M5 (4P+6E), 16 GiB, macOS 27.0; Docker Desktop 4.88.1 (engine 29.7.2), linuxkit `7.0.12-linuxkit`, 10 vCPU, MemTotal 8,124,516 kB |
| Platform verification | container `uname -m` = aarch64 = host arm64; base `debian:testing@sha256:dab11cdb…` (the record image's own FROM pin), variant arm64; no qemu/binfmt handlers; no `--platform` in any command |
| Image | the record's own `oxpinyin-matrix:knob` (`sha256:9b3f50e4…`); `/repo` engine tree byte-identical to `50afb7f6` (`crates/`, `Cargo.lock`, `rust-toolchain.toml`, `bisect.c`) |
| Toolchain (in-image) | rustc/cargo 1.97.1 (`8bab26f4`/`c980f486`), gcc 15.3.0, cargo-c 0.10.25, libtkrzw 1.0.32, libpinyin 2.11.92 @ `074a2219`; `CARGO_BUILD_JOBS=4` |
| Controller commit | `4cc53194` (the arm64 control ran from the worktree at this commit; scripts `build-five.sh`, `run-window.sh` under `/tmp/arm64-control/`, captures not committed) |
| Capture windows | W1 02:00:40–02:03:32Z; W2 02:04:25–02:07:31Z (interrupted, n=16); W3 02:09:33–02:12:29Z; W2-replacement 02:22:00–02:24:51Z (2026-09-09, UTC) |
| Interference | host load 1.63–3.95 across windows (Chrome closed at the operator's direction); the W2 n=16 interruption (load 8.73) and the replacement's single bimodal round are both recorded above |

### What the control changes

The arch spread — the standing finding and the ordered-atomics pointer's
basis — **survives the same-recipe control at n=8**, now measured
S-against-S, and is **corroborated at n=16** under the corrected rule
below. The recipe effect is real on both architectures. *(Updated
2026-09-09: the amd64 three-window control has since run — the arch spread
is now established at both sizes on symmetric designs, the tree effect is
not established on either host with the amd64 +0.046 magnitude refuted, and
the shipped-flag question is open at this precision rather than
headline-moving. The ordered-atomics audit remains the next task **if** the
spread stands — it does stand — and is not started here.)*

### n=16 revisited: the corrected bimodality rule (a disclosed rule revision)

**The revision record, stated plainly:**

1. **The rule was locked before the data and changed after data it had
   excluded.** The original bimodality rule (any max adjacent ratio ≥ 1.15
   over the sorted per-run medians → bimodal) was pre-registered before any
   window ran; this revision is adopted after that rule excluded a block.
2. **The defect was identified from the rule's structure before the
   replacement's width was known — but after the bimodality failure was
   known.** In that order: the original W2's 19+1 splits were visible at
   analysis time and were reported then as "not a genuine two-mode
   performance; a one-round interruption caught by the locked rule"; the
   structural defect — a 19+1 split is an outlier, not two modes — was
   evident from that structure; W2r's CI width (0.0133) became known only
   later. The corrected rule is: **bimodal only if the largest-gap split
   leaves both groups with ≥ 3 runs.**
3. **The width rule independently catches genuine contamination.** The
   original W2 block remains excluded under either rule on width alone
   (S÷L width 0.2427 ≥ 0.20): the interruption hit all five cells hard
   enough to inflate the interval, and no rule revision admits it. The
   revision admits exactly one block — the replacement — not all of them.
4. **The median is empirically insensitive to the outlier in question.**
   W2r's n=16 S÷L point estimate is 1.1055 with the outlier round and
   1.1044 with it paired-dropped — 0.001 apart. The excluded block's
   problem was rule classification, never the point estimate.
5. **n=8 established the verdict and the arch spread under the original
   locked rules.** n=16 is corroboration and carries no weight n=8 does
   not already carry. Had n=16 contradicted n=8 under the corrected rule,
   the revision would have been withdrawn rather than the verdict changed —
   it does not contradict it; it agrees.

Two boundary notes. The corrected rule is **pre-registered before any data
on the amd64 side**, so that host does not inherit this question. And the
amd64 control's one bimodal cell (`oxpinyin-cur-cargo`, n=1) is unaffected:
n=1 carries no verdict on either host.

**Verification under the corrected rule, all 15 cell×window combinations at
n=16** (windows W1, W3, W2-replacement): fourteen are unimodal under both
rules (max adjacent ratio ≤ 1.0992, no split to classify). The one
difference is the replacement window's S cell: max adjacent 1.1808, split
19+1 — BIMODAL under the original rule, a single-round outlier (both-groups
test fails) and **usable** under the corrected rule. The decomposition
cells X, Y and Z are clean in all three windows, so the tree and recipe
effects at n=16 are computable on certified-clean data.

**n=16 under both rules, side by side.** Original locked rule: original W2
bimodal in all five cells plus width 0.2427; replacement gated by
pre-commitment, failed the bimodality check → *not measurable, no verdict.*
Corrected rule: original W2 still excluded (width alone), W1/W3/W2r clean
→ **measurable**, with:

| window | S÷L n=16 [95% CI] | width |
|---|---|---:|
| W1 | 1.1019 [1.0935, 1.1100] | 0.0166 |
| W3 | 1.1042 [1.0841, 1.1464] | 0.0623 |
| W2r | 1.1048 [1.0990, 1.1123] | 0.0133 |

Gates: widths ≤ 0.0623 (< 0.20); point range 0.0029 (< 0.05); all three
lower bounds > 1.0. **Decision row: the shipped artifact is measurably
slower at n=16 — same verdict as n=8.** Arch spread at n=16: arm64's
lowest lower bound 1.0841 against amd64's n=16 upper bound 0.9998 — no
overlap; **the spread is corroborated at n=16.** (X÷L at n=16:
1.1487 / 1.1463 / 1.1469 against the record's 1.157 — the record
reproduction also corroborates.)

**n=16 decomposition** (amendment-1 threshold: CI excludes zero in all
three windows, consistent sign):

- **Tree effect:** −0.0101 [−0.0216, −0.0047] / −0.0034 [−0.0379, +0.0117]
  / −0.0060 [−0.0136, +0.0024] — one of three excludes zero →
  **not established**, exactly as at n=8; point signs negative in all
  three windows, opposite to amd64's.
- **Recipe effect:** −0.0321 [−0.0381, −0.0215] / −0.0373 [−0.0552,
  −0.0156] / −0.0347 [−0.0398, −0.0270] — all three exclude zero, sign
  consistent → **established**, magnitude matching n=8's −0.036-to−0.038
  band.
