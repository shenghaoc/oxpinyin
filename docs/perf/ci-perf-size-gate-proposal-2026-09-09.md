# Perf-and-size CI gate — proposal (2026-09-09 UTC)

Status: **proposal only. Nothing in `.github/` is touched.** AGENTS.md
lists editing CI policy as a hard forbid without an ask, and every item
under "What needs a human decision" below is such an ask. Written at
`a118485b`.

Reviewed on the build-recipe axis by
[`../findings/perf-build-recipe-audit-2026-09-10.md`](../findings/perf-build-recipe-audit-2026-09-10.md),
which found the design sound (all four gates are self-ratchets, so the recipe
cancels) and the G3 argument short one term. That term is now in G3, and the
per-artifact recipe block that audit's convention asks for is in the baseline
schema.

**Provenance of the figures quoted here.** Two of the absolutes used as
illustration — the amd64 bimodal medians in "The constraint that shapes
everything", and G4's RSS bands — come from
`../findings/perf-steady-cycle-cross-host-2026-09-07.md`, whose oxpinyin arm
was built by `cargo build` rather than the shipping `cargo cinstall`. Both are
used only to characterise **runner noise**, and every conclusion drawn from
them rests on within-pass quotients where the recipe cancels. No threshold in
this document is derived from a cross-recipe comparison.

## The gap this fills

Constitution §2 (install-size budget) and the Stage 2 targets — smaller
binary, faster execution, lower RAM than libpinyin — have no mechanical
enforcement. `docs/safety/AGENTS-reduction.md` §2 already records this in
its own words: enforcement is **"assisted"**, the mechanism is "prose +
Stage-2 pins". `docs/runbooks/benches.md` opens with "Four tiers, none run
by CI (`verify-nightly` has no perf lane)". So a size or allocation
regression lands silently and is found only when somebody next runs the
container by hand.

`main` requires exactly one status check, `ci-aggregate` at the tail of
`.github/workflows/ci.yml`, so any new lane must be added to that job's
`needs` list and to its per-lane accounting to matter at all.

## The constraint that shapes everything: wall clock is not gateable here

Every runner in this repository is GitHub-hosted (`ubuntu-latest`;
`.github/workflows/{ci,verify-nightly,store-backends,coverage}.yml`).
There is no self-hosted runner. Four independent facts already in the
repository say a wall-clock threshold on such a runner cannot work:

1. `docs/runbooks/benches.md`: "Numbers are only comparable within one
   container on one host."
2. `docs/findings/perf-steady-cycle-cross-host-2026-09-07.md` measured a
   **whole-session offset of 9–11%** co-moving across all four cells,
   between two sessions of the same harness on the same *quiet, dedicated*
   Apple-silicon host. Its rule: "Absolute milliseconds from different
   sessions, even on the same machine, are not comparable."
3. The same record's amd64 section (a shared dev box — the closest analogue
   to a CI runner) is **bimodal**: per-process steady medians split into
   ≈30–38 ms and ≈43–51 ms modes, with per-run spread 1.1–1.8× at all five
   workload sizes.
4. `docs/perf/perf-stage2-harness-2026-08.md` configures criterion at
   `noise_threshold(0.50)` and says in terms: "Do not tighten it into a
   required check … CI does not run the benches and must not grow a
   required check for them."

The regressions Stage 2 actually cares about are smaller than that noise:
the reverted release `panic = "abort"` cost **+5.5%** on the steady cycle.
A band wide enough to be quiet on a shared runner is wider than the effect.

**So this proposal gates no timing at all.** It gates quantities that are
deterministic by construction, and it leaves the wall-clock comparison
against libpinyin exactly where it is — on a provisioned host, by hand,
written up under `docs/perf/`. The criterion benches stay out of required
checks, as their own document instructs.

## What is measured

Four metrics. Three are exact; the fourth is reported on PRs and gated only
on the nightly.

### G1 — Size (noise floor: exactly zero)

Build the real product path, not a proxy: `tools/packaging/install.sh`, which
runs `cargo cinstall` — the same path `release-packages.yml` ships and the one
`tools/bisection/run-shipped-check.sh` argues is the product. Then
`strip --strip-all`.

The backend is named explicitly rather than inherited, and the two libraries
do not take the same feature list:

```sh
tools/packaging/install.sh libpinyin --prefix=/usr --destdir=… \
    -- --no-default-features --features "$BACKEND",shipped
tools/packaging/install.sh libzhuyin --prefix=/usr --destdir=… \
    -- --no-default-features --features "$BACKEND"
```

Two reasons for the shape. `default = ["tkrzw"]` on both capi crates means a
bare `--features shipped` inherits whatever the default backend happens to be
that month — the default has already flipped once (`05688575`), and a
measurement lane must not change artifact silently when it does. And
`oxpinyin-zhuyin-capi` has **no `shipped` feature** at all (its `[features]`
block is `capi`, `default`, and the four backends), so passing one to it is a
build error, not a no-op.

Gated quantities, per artifact:

| quantity | source | why |
|---|---|---|
| **section byte sum** — `.text`, `.rodata`, `.data.rel.ro`, `.rela.dyn`, `.eh_frame`+`.eh_frame_hdr`+`.gcc_except_table` | `readelf -S` | byte resolution |
| stripped file size | `stat -c%s` | the shipped number, but coarse |
| staged payload bytes | total of the `--destdir` tree (`.so` + headers + `.pc`) | §2's "default payload", code half |

**The section sum is the primary gate, and the file size is secondary, for a
reason this repository already measured.** `docs/perf/perf-so-size-2026-09.md`
found stripped file size page-quantized on its host — seven distinct probe
`cdylib`s all reported an identical 266,736 B, and both `panic = "abort"`
deltas were exactly −65,536 and −131,072 B. Its instruction: "Do not cite a
zero stripped-size delta as evidence that a change costs nothing." A gate
built on file size alone would inherit that blind spot; a real 4 KiB `.text`
growth (the size of the `Custom`/`Box<dyn Error>` machinery that document
removed) would pass unseen.

Thresholds, written as the predicates that fire:

```text
limit   = baseline_section_sum + max(baseline_section_sum * 0.005, 4096)
fail if actual_section_sum   > limit
fail if actual_stripped_size > baseline_stripped_size
```

The percentage allowance is 0.5% **of the baseline**, floored at 4096 B so a
small artifact still gets one page of slack. Both predicates fail on
*exceeding* the limit; equality passes, and a decrease always passes.

**Not proposed yet: the §2 budget ceiling.** See "What needs a human
decision", item 1 — "pinned reference stack" is not defined anywhere in the
repository, and on the `.so` alone the current ratio is 1.832× (ARM64/KC
1,446,528 B vs oracle 789,512 B), which is an accepted state, not a breach.
G1 can only be a self-ratchet until someone defines the payload.

### G2 — Instructions on the keystroke steady unit (measured noise floor ≤ 0.031%)

`valgrind --tool=callgrind --collect-atstart=no
--toggle-collect=bisect_perf_steady_unit`, driving `tools/bisection/bisect.c
--perf <so> <datadir>`. The anchor already exists: `bisect.c` deliberately
routes cycle 0 through a separate cold anchor so a toggle on the steady one
never sees it.

Artifact: **G1's shipped `libpinyin` build**, reused rather than rebuilt — G2
gates the product. That works against the fixture tables even though `shipped`
compiles out `oxpinyin_init_for_fixtures`, because `bisect.c` falls back to
`pinyin_init`, which `context.rs` documents as the same function: the fixture
symbol "is `pinyin_init` under another name", both calling `init_context`.
(`bisect.c`'s "prefer the non-header constructor" comment predates that and no
longer describes a difference.)

Workload: **`fixtures/w3/tkt`** — committed, frozen, already a CI path input,
matching the compiled backend, and already opened as a real drop-in directory
by `tools/bisection/run-cpp-smoke.sh` inside the existing `test` job. No
pin-built oracle, no model20 (which is non-redistributable and never enters
CI), no network.

Gate on the **per-object Ir for the oxpinyin object only** (callgrind's `ob=`
attribution), not `PROGRAM TOTALS`. That excludes glibc, libtkrzw and the
loader from the gated number, so a system-library change inside the image
cannot move it — and, in the other direction, cannot mask a regression in our
own code. Report the totals alongside, for the record.

Noise floor: `docs/findings/perf-cycle-ir-differential-2026-09-08.md` measured
callgrind determinism across two rounds at **lp 0.0000%, ox 0.0031%**.

Thresholds: warn at +0.5%, fail at +2.0% against baseline. 2.0% is ~65× the
measured determinism floor, so a breach is never a noise question — it is a
policy question about how much instruction growth one change may add
silently.

**This is oxpinyin against its own baseline, never against libpinyin, and
that is deliberate.** The IR differential record found callgrind's
environment masks CPUID, which splits libpinyin's scalar memcpy into many
memory operations and makes any *cross-engine* memory comparison circular —
"the distortion is largest exactly for the side the argument needs to look
efficient". A tree-vs-its-own-baseline comparison has no such asymmetry: the
distortion is identical on both sides and divides out. It also removes the
oracle from CI entirely.

### G3 — Allocations per steady cycle (noise floor: exactly zero)

A second, **diagnostic** artifact, run **natively** (no valgrind):

```sh
cargo build --locked --release -p oxpinyin-capi \
    --no-default-features --features tkrzw,alloc-count
```

run against the fixture directory for the backend it was compiled with
(`tkrzw` → `fixtures/w3/tkt`). `shipped` is deliberately absent and the two
features are mutually exclusive in practice: the `nm -D` step below requires
the shipped artifact to export **zero** `oxpinyin_alloc_*` symbols, so an
artifact carrying both would fail the lane it belongs to.

This is the one place the gate measures something other than the product
artifact, and the difference has **two** terms, not one.

*Feature axis.* `--features shipped` compiles out exactly two symbols, and
`oxpinyin_init_for_fixtures` is `crates/oxpinyin-capi/src/context.rs`'s own
words "`pinyin_init` under another name" — a byte-identical alias calling the
same `init_context`. Neither hook is on the steady keystroke anchor.

*Recipe axis.* G1 and G2 build through `tools/packaging/install.sh`
(`cargo cinstall`, the shipping path); G3 builds with `cargo build` + `strip`,
which `docs/findings/perf-build-recipe-audit-2026-09-10.md` establishes is a
**different artifact**, not merely a differently-named one — on the steady
keystroke-cycle ratio the two differ by 0.064–0.087 units on amd64 and
0.032–0.038 on arm64, with **no mechanism established**. The first version of
this section bounded the G3 difference entirely on the feature axis and left
this term unstated; that audit's review of this document named the omission,
and this paragraph is the correction.

*Why the conclusion survives both terms.* All four gates are **self-ratchets**
— oxpinyin against its own baseline, the same recipe on both sides of every
comparison — so the recipe contributes exactly zero to any gated delta. It
would matter only if a G3 number were quoted against a figure produced by the
other recipe, which is the cross-recipe-quotient defect class that audit
identifies as invisible to any per-document check. Hence the rule in the
baseline schema below: **every artifact carries its own recipe block**, so a
future reader dividing one of these numbers by another can see whether the two
are like-for-like without reconstructing it.

That also fixes the axis on which nothing may be claimed. The recipe control
was run on steady-cycle wall clock only; the audit's limit 1 states that for
allocation counts, instruction counts, RSS and binary size **no magnitude may
be stated** for the recipe effect. This proposal states none: G3's numbers are
compared only to G3's own baseline.

Gate the delta of `oxpinyin_alloc_count` and `oxpinyin_alloc_bytes` across the
steady cycles, plus `oxpinyin_alloc_peak_live_bytes`.

**Validate the readers before computing anything.** `bisect.c`'s
`read_alloc_counters` sets each field to `-1` when its `dlsym` failed, so a
run against an artifact built without `alloc-count` emits four `-1`s rather
than failing — and a delta of `-1 − (-1)` is `0`, which reads as *zero
allocations per cycle*: a spectacular improvement that would ratchet the
baseline down to a number no real build can ever meet. So: any of
`oxpinyin_alloc_count`, `oxpinyin_alloc_bytes` or
`oxpinyin_alloc_peak_live_bytes` reading `-1` in any round is an
`INSTRUMENT FAULT`, raised **before** two-round agreement is evaluated and
before any comparison against the baseline. Agreeing rounds do not redeem it,
and neither does a value that appears to improve on the baseline.

Thresholds: **allocation count must not increase at all** (exact ratchet);
bytes +1% (capacity rounding moves bytes without moving counts).

This is the cheapest and highest-value gate in the set: it is a pure function
of the code path, it needs no timing, and it is the quantity the keycost /
first-alloc work was actually about
(`docs/findings/perf-keycost-first-alloc-2026-09-07.md`).

One extra step, promoting an existing runbook rule to a gate: `nm -D` on the
shipped artifact must report **zero** `oxpinyin_alloc_*` symbols. The runbook
already says "The feature is never in a shipped artifact — `nm -D` is the
check"; nothing checks it today.

### G4 — RSS (report on PRs, gate on the nightly)

`PERF_MODE=ram-init` and `ram-cycle`, medians over 10 processes,
`rss_kib` / `hwm_kib`.

Not deterministic like G1–G3, but far steadier than wall clock. In the
cross-host record's amd64 section, across five workload sizes:
`rss-init` 14,058–14,130 KiB (±0.26%), `rss-cycle` 23,358–23,458 KiB
(±0.2%) — while the wall clock in the same table was bimodal.

PR: reported as an artifact, not gated. Nightly, in the pinned image: fail at
+3% against baseline — ~12× the observed within-session spread, yet far under
the effects that matter (P1–P6 moved RSS 72,652 → 28,388 KiB).

**A pinned container image does not pin what RSS depends on.** glibc, the
allocator and the measurement tools come from the image and are pinned by it;
the **kernel is the host's**, and so is the GitHub runner image around it.
GitHub rotates both on its own schedule, and page accounting, transparent
hugepage policy and overcommit behaviour all live there. So the G4 gate's
environment contract is larger than the image digest, and its fingerprint
block carries, beyond the common set in mechanism 3 below:

| field | source |
|---|---|
| runner image label + version | `$ImageVersion` / `$ImageOS` |
| kernel identity | `uname -srvm` |
| container image digest, and the apt snapshot date its packages came from | image metadata |
| exact tool versions — `valgrind --version`, `readelf --version`, glibc, `libtkrzw` | in-image, recorded not assumed |

A change in any of them makes a G4 delta uninterpretable, so it takes the same
`BASELINE STALE` path as every other fingerprint field rather than being
reported as a memory regression. `valgrind --version` is in the common set
too, not only G4's: callgrind's Ir is a simulation, and a simulator version
change moves G2's number without a line of oxpinyin changing.

## On what runner

GitHub-hosted `ubuntu-latest`, job container **pinned by digest**, apt from a
pinned `snapshot.debian.org` archive.

This departs from the convention the other lanes state — `debian:testing #
deliberately unpinned; see store-backends.yml header` — and the departure is
the point. Drift-surfacing and measurement are opposite requirements: the
test lanes are unpinned *so that* distro drift breaks them early; a
measurement lane whose environment drifts produces numbers that cannot be
compared to yesterday's. There is already precedent in-repo for the pinned
form: `tools/bisection/Dockerfile.perf-matrix` pins the base by digest and
apt by snapshot date.

No PMU is assumed. `tools/env-probe/probe-perf-event-open.c` exists precisely
because hardware counters are absent on more hosts than expected (measured
`ENOENT` on a Firecracker microVM, 2026-09-08 UTC) — and its own comment names
the substitute: "use callgrind, whose counts are simulated but exact." The
lane runs that probe and records the result; it never depends on it.

`taskset -c 0` on every measured process, as every existing harness does.

## How a threshold breach is distinguished from runner variance

Four mechanisms, in order of how much work they do.

**1. Nothing gated is a wall clock.** G1–G3 are byte counts, simulated
instruction counts and allocation counts — deterministic functions of the
artifact and the input. "Runner variance" in the timing sense cannot move
them. This is the whole reason the metric set looks the way it does.

**2. Two-round self-agreement, with a published floor per metric.** Each
capture runs twice inside the same job. What a "round" covers differs per
metric, and saying so is load-bearing — re-reading one binary twice tests
nothing:

| metric | a round is | agreement floor | compared to baseline |
|---|---|---|---|
| G1 size | a **full rebuild** into a fresh `--target-dir` and `--destdir`, restripped, remeasured | exact | round 1 |
| G2 Ir | a **fresh callgrind process** over the same artifact (built once) | ≤ 0.05% (floor measured at 0.031%) | round 1 |
| G3 alloc count | a **fresh native process** over the same artifact (built once) | exact | round 1 |
| G4 RSS (nightly) | a **fresh 10-process pass**, median taken per pass | ≤ 1% between passes | median of the two passes |

G1 rebuilds because build reproducibility is the thing that could silently
move a size number; G2 and G3 build once and re-run, because the artifact is
the input whose determinism they are not testing — the measurement process is.
**Round 1 is the value compared to the baseline** for G1–G3 (round 2 exists to
falsify round 1, not to be averaged with it); G4 compares the median of its
two passes, since it is the one metric that is statistical rather than exact.

Ordering is fixed and matters: **validity first, then agreement, then the
baseline comparison.** A `-1` from any `oxpinyin_alloc_*` reader (G3, above),
a missing ELF section, an empty callgrind output or an unresolved anchor
symbol is an `INSTRUMENT FAULT` raised at the validity step — before rounds
are compared to each other, so two rounds that agree on a missing reading can
never be promoted into a valid G3 value.

A fault fails the lane with a **different message and a different exit
status** from a regression. "The number moved" and "the instrument was not
reliable on this runner today" are different findings and must never arrive
looking the same — that is exactly how a flaky gate gets muted.

**3. Environment fingerprint match — and this is where the un-homed rule
lands.** The baseline file carries a fingerprint: container image digest and
its apt snapshot date, the GitHub runner image label and version, `uname
-srvm`, `rustc -vV`, glibc, `libtkrzw`, `valgrind --version`, `readelf
--version`, the SHA-256 of the fixture tree, the full build recipe (backend
and feature list per artifact), and **the harness commit**. The lane
recomputes it. On any mismatch the lane does **not** compare numbers: it
reports `BASELINE STALE` and fails, naming what moved.

The harness commit is the recorded commit of the harness inputs
(`tools/bisection/bisect.c` and the scripts the lane runs), taken from the
tree under test and written into the baseline — not left implicit in whatever
the PR head happens to be. A PR that edits the harness therefore invalidates
the baseline by construction and must refresh it in the same change, which is
the intended behaviour rather than an obstacle: the alternative is a number
compared against one produced by different code.

The 2026-09-07 provenance audit found that no perf record pins its harness to
a commit, and that this is the enabling mechanism for reference drift —
`871139a1` changed `run-w8-cycle.sh`'s `PIN_REF` from libpinyin 2.11.91 to
2.11.92, leaving three records still reading as "same W8 protocol" while
pointing at a different oracle. The fix agreed there ("every record states the
harness commit alongside the harness name") was scoped out of the audit PR and
**has had no owner since**. A gate cannot be built without it — a delta across
a fingerprint change is uninterpretable — so this is its natural home, in a
machine-checked rather than a documentary form.

**4. Like-for-like only.** The lane's single comparison is: this tree, this
image, this fixture set, this harness → the committed baseline captured under
the same fingerprint. No cross-engine, cross-host or cross-session comparison
happens inside CI, because none of them is valid there.

## The baseline file

A committed **strict JSON** file — `json.load` with no extensions, because the
gate reads it in CI and a hand-added comment must not be able to break the
lane.

The block below is therefore **illustrative, not the file format**: it is
JSONC purely so each field can be annotated, and the values are elided
(`…`, zero bytes) because they are properties of a future capture. The emitted
file carries no comments and no elisions.

```jsonc
{
  "captured_utc": "2026-09-09T16:17:11Z",   // date -u at run time, never written by hand
  "fingerprint": {
    "image_digest":   "sha256:dab11cdb…",
    "apt_snapshot":   "20260831T000000Z",
    "runner_image":   "ubuntu24/20260901.1",  // $ImageOS / $ImageVersion
    "kernel":         "Linux 6.12.0-… x86_64", // uname -srvm
    "rustc":          "rustc 1.97.1 (8bab26f4f 2026-07-14)",
    "glibc":          "2.42",
    "libtkrzw":       "1.0.30",
    "valgrind":       "3.26.0",
    "readelf":        "GNU binutils 2.47",
    "fixture_sha256": "…",                    // over fixtures/w3/tkt, sorted
    "harness_commit": "a118485b…"             // of bisect.c + the lane's scripts
  },
  "artifacts": {
    // benches.md, "State the build recipe, per artifact": the exact command
    // line, sha256, NEEDED and byte size, for EVERY artifact measured.
    "shipped_libpinyin": {
      "recipe": "tools/packaging/install.sh libpinyin --prefix=/usr --destdir=<stage> -- --no-default-features --features tkrzw,shipped",
      "installed_name": "libpinyin.so.15.0.0",
      "sha256": "…", "needed": ["libtkrzw.so.1", "libglib-2.0.so.0"], "bytes": 1446528
    },
    "shipped_libzhuyin": {
      "recipe": "tools/packaging/install.sh libzhuyin --prefix=/usr --destdir=<stage> -- --no-default-features --features tkrzw",
      "installed_name": "libzhuyin.so.15.0.0",
      "sha256": "…", "needed": ["libtkrzw.so.1", "libglib-2.0.so.0"], "bytes": 0
    },
    "alloccount_libpinyin": {
      "recipe": "cargo build --locked --release -p oxpinyin-capi --no-default-features --features tkrzw,alloc-count",
      "installed_name": "libpinyin_capi.so — a fixture; nothing installs it",
      "sha256": "…", "needed": ["libtkrzw.so.1"], "bytes": 0
    }
  },
  "metrics": {
    "g1": { "section_sum": 0, "stripped_size": 0, "payload_bytes": 0 },
    "g2": { "ir_oxpinyin_object": 0, "ir_program_total": 0 },
    "g3": { "alloc_count_per_cycle": 0, "alloc_bytes_per_cycle": 0, "peak_live_bytes": 0 },
    "g4": { "rss_init_kib": 0, "rss_cycle_kib": 0, "hwm_cycle_kib": 0 }
  },
  "capture_params": {
    "g2": { "cg_cycles": 4, "cg_repeats": 1, "rounds": 2 },
    "g3": { "cycles": 8, "repeats": 1, "rounds": 2 },
    "g4": { "processes_per_pass": 10, "passes": 2 },
    "cpu": 0
  },
  "justification": null   // a docs/** path; required when a gated value rises
}
```

The `artifacts` block is not decoration. `docs/runbooks/benches.md` now
requires every record to name the exact cargo command line, `sha256`, `NEEDED`
list and byte size per timed artifact; a gate that writes that block on every
run enforces the convention mechanically instead of relying on whoever writes
the next record remembering to.

Which means the block has to be checkable, or it decays into the prose it
replaced. Two rules:

- **`recipe` is the command the lane actually ran**, written out in full, with
  one normalization: the per-run staging directory is recorded as `<stage>`.
  That path is a `mktemp` scratch location with no bearing on the artifact,
  and leaving it verbatim would change the string on every run and make the
  fingerprint compare unequal to itself.
- **The gate rejects an incomplete recipe** as `BASELINE STALE` (exit 3), not
  as a pass: any `recipe` that is empty, contains an ellipsis, or carries an
  unexpanded `<…>` token other than the one normalized `<stage>` fails the
  check. A baseline whose provenance field has been hand-edited into a
  fragment is the failure mode this whole block exists to prevent, so it must
  not be the one thing the gate takes on trust.

Refreshing it is an ordinary PR that regenerates the file from the lane's own
uploaded artifact. When a gated value moves **upward**, the lane requires the
`justification` field to name a `docs/**` path, and that file to be modified
in the same PR. That makes the AGENTS.md source-policy rule — a change that
worsens time or space "must be minimized, and must be justified in the
change's report" — mechanically checkable instead of advisory.

Residual, stated plainly: a PR that raises the baseline *and* writes a
document saying why will pass. That is the same residual class ci.yml's own
header already documents for the path-set bypass ("a PR that rewrites the
workflow itself can always rewrite its gates"), and like that one it is a
review question, not a workflow one.

**The file must not live under `docs/`.** `docs/**` is deliberately outside
ci.yml's path gate, so a baseline-only PR would run no lanes at all — the one
change that most needs re-measuring would be the one change that measures
nothing. It needs a gated path (e.g. `perf/ci-baseline.json`, added to the
`changes` job's pathspec list and to the header comment that documents it).

## Where the lane hangs, and what it costs

A new `perf-gate` job in `ci.yml` with `needs: changes` and
`if: needs.changes.outputs.run == 'true'`, added to `ci-aggregate`'s `needs`
list and to its `lane` accounting — the only way a new lane reaches the
required check. Its inputs (`crates/**`, `tools/**`, `fixtures/**`,
`Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`) are already in the gate
set; the baseline file is the one addition.

### The lane, step by step

```text
 1  probe        tools/env-probe/probe-perf-event-open.c  → record, never depend on
 2  fingerprint  compute; compare to baseline      → mismatch: BASELINE STALE (exit 3)
 3  build        cinstall libpinyin  (tkrzw,shipped)      round 1
 4  build        cinstall libzhuyin  (tkrzw)              round 1
 5  build        cargo build alloc-count (tkrzw,alloc-count)
 6  guard        nm -D shipped | grep oxpinyin_alloc_  → non-empty: FAULT (exit 2)
 7  G1           strip, readelf -S, stat; rebuild 3-4 into fresh dirs → round 2
 8  G2           callgrind × 2 rounds over the step-3 artifact
 9  G3           native × 2 rounds; validate readers ≠ -1 first
10  G4           2 × 10-process passes (nightly gates, PR reports)
11  validate     any -1 / missing section / empty callgrind → FAULT (exit 2)
12  agree        round 1 vs round 2 per metric floor → FAULT (exit 2)
13  compare      round 1 (G4: median of passes) vs baseline → REGRESSION (exit 1)
14  upload       the run's own baseline-shaped JSON, always, pass or fail
```

Step 14 matters as much as the gate: a refresh PR is then "download the
artifact, commit it", not "re-derive the file by hand".

| exit | meaning | who acts |
|---|---|---|
| 0 | within every threshold | — |
| 1 | `REGRESSION` — a gated value exceeded its limit | the PR author: fix, or raise the baseline with a `justification` |
| 2 | `INSTRUMENT FAULT` — the measurement is not trustworthy | re-run once; if it persists, the lane is broken, not the PR |
| 3 | `BASELINE STALE` — the environment moved | refresh the baseline in its own PR, then rebase |

Exit 1 and exit 2 must never be collapsed. A gate whose flakes and whose real
findings look alike gets muted, and then it is worse than no gate.

Steps 11–13 — the decision logic, and the only part of the lane with rules
rather than commands — are implemented in `tools/perf-gate/check.py`, with
`tools/perf-gate/check.test.sh` covering every exit path. That is a tool, not
CI policy: nothing runs it until the lane exists, and the lane is what needs
the ask. It is there so Phase 0 can be run by hand — capture, check, read the
exit code — and so that the rules above are reviewable as code rather than as
a description of code that might later be written differently.

### Cost

Dominated by release builds under `lto = "fat"` + `codegen-units = 1`: **five**
of them. The two cinstall builds (shipped libpinyin, shipped libzhuyin) run
twice, because G1's round 2 rebuilds steps 3–4 into fresh directories; the
alloc-count build runs once, since G3's second round re-runs the measurement
process rather than the build.
The measurement itself is seconds — callgrind at `CG_CYCLES=4` over a ~9 ms
cycle under a ~50× interpreter is a couple of seconds per round, and the
fixture tables are smaller than the records' full ones. **I have not measured
the lane on a GitHub runner and am not going to guess a minute figure.**
Phase 0 measures it, and the number is an input to the Phase 2 decision, not
an assumption behind it. If the rebuild for G1's round 2 turns out to dominate,
dropping it to a nightly-only check is the first thing to trade away — it tests
build reproducibility, which is the least likely of the four to regress
silently.

## Rollout — three phases, three separate asks

**Phase 0 — measure the runner before gating on it. No gate; no `ci-aggregate`
change.** Land the capture as a `workflow_dispatch` + nightly job in
`verify-nightly.yml`, uploading the JSON. Run it ~10 nights and on ~10 PR
heads. Output: `docs/perf/ci-gate-noise-floor-<date>.md` — the observed spread
of each metric *on GitHub-hosted runners* (not on the Apple-silicon or amd64
hosts every existing figure comes from) and the lane's real wall-clock cost.
That record establishes the first baseline. Tier 3's failure policy in
`docs/safety/ci-strategy.md` — findings open issues, they do not auto-block —
is exactly the right place for a lane that has not yet earned a gate.

**Phase 1 — G1/G2/G3 gate, still outside `ci-aggregate.needs`.** Visible and
red when breached, not blocking. One to two weeks.

**Phase 2 — add `perf-gate` to `ci-aggregate.needs`.** It becomes part of the
required check. G4 stays report-only on PRs and gated on the nightly.

## What this gate does not do

Worth stating so a green lane is not over-read:

- **It does not certify the Stage 2 targets.** Those are defined against
  libpinyin; this is a ratchet against oxpinyin's own last approved state. It
  catches regressions. The libpinyin comparison still needs a pin-built
  oracle, the drop-in data directory and a quiet host, and stays where it is.
- **It measures the MINI tables.** `tools/bisection/system-dir.sh` calls
  `fixtures/w3` the MINI tables in those words and refuses to score parity
  against them without an explicit override. For instruction and allocation
  counting they are fine and, crucially, pinned — but a cost that scales with
  table size (the `fill_lookup` / `memcmp` mass named in
  `perf-stage2-harness-2026-08.md`) is under-represented. The gate resolves
  code-path regressions, not table-size-sensitive ones.
- **It has no timing threshold, by design**, and no hardware counters.
- **It measures the drop-in configuration only** — no training, no sentence
  decode, no oxpinyin-generated data, matching every existing steady-cycle
  record.

## What needs a human decision

1. **§2's "pinned reference stack" is undefined in-repo.** The only
   occurrences are the AGENTS.md line itself and
   `docs/safety/AGENTS-reduction.md` calling its enforcement "assisted". On
   the `.so` alone oxpinyin is at 1.832× the oracle — so the +10% budget must
   be about a payload including data, but nothing says which. Until that is
   defined, G1 ships as a self-ratchet with no §2 ceiling.
2. **Pinning the perf lane's container by digest**, against the repo's
   deliberately-unpinned convention for the other lanes. Recommended, with
   the reason above; still a policy choice.
3. **Adding a gated path for the baseline file** (a `changes`-job pathspec
   edit plus its header comment) — CI policy.
4. **Phase 2 promotion into `ci-aggregate`**, which changes what branch
   protection actually enforces.

Each of 1–4, and each phase boundary, is a separate ask. Nothing is
implemented.
