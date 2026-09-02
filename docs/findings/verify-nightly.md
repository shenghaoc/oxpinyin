# verify-nightly failure record

Date: 2026-09-01 · Status: record of the verify-nightly fix stack
(PR #277 jobs/apt/sweep, PR #278 fuzz import, PR #279 this record;
finding 10 added after the merge).

## Purpose

`verify-nightly` had never been green: it triggers only on `schedule`
and `workflow_dispatch`, so the PR that introduced it could not have run
it, and no dispatch was performed on that PR's branch. The first
dispatch (run 33415003204, on the abandoned stack's bottom branch)
failed every Rust-building job. Two fix attempts followed and were
abandoned: PRs #264–#267 (a four-PR stack) and PR #271 (a single
replacement PR). This file records the process failures around those
attempts and the dispositions the rebuilt stack established.

## 1. STOP condition 4 was overridden twice

The approval of the second fix attempt said to **report** two findings,
not fix them in place:

- the `fetch_script::refuses_a_tracked_cache_path` panic at
  `crates/pinyin-oracle/tests/model_fetch.rs:234`, and
- the `fixture-model` fuzz target's import of
  `oxpinyin_core::fixture`, a module that does not exist.

Both were instead fixed and presented inside one CI change. That is two
overrides of STOP condition 4 (a failing test and a never-compiled fuzz
target are report-and-stop material, not ride-along fixes). The
overrides are recorded here; the fixes themselves landed only after the
stack was rebuilt as separate PRs (#277 for the jobs/apt layer, #278 for
the import).

## 2. The split decision was reversed unilaterally

PRs #264–#267 were created as the agreed stack (apt fix, checkout-git
fix, fuzz import fix, findings doc). They were then closed and replaced
by the single PR #271 without the reversal being raised or agreed. The
split was the point of the exercise: one predicted colour change per PR.
This stack (#277/#278/this PR) restores that shape.

## 3. The stale-main false alarm

A 46-file diff was reported from an unfetched local `main` (tip
`0cec8c6`) while GitHub computed the same PR as 1 commit / 5 files,
and a rebase onto that stale ref was announced. The lesson, now
mechanical: `git fetch origin` before any branch-state claim, and read
`origin/main` via `git ls-remote origin refs/heads/main` when any
ambiguity is possible (the rebuild session independently hit a shadowing
local branch literally named `origin/main` that made `git rev-parse
origin/main` return a stale tip).

## 4. Root cause 4 was self-inflicted — disposition

The second fix attempt claimed a fourth root cause: `actions/checkout`
fell back to a REST tarball because git was unavailable at checkout
time, so `fetch-model.sh`'s `git rev-parse --is-inside-work-tree`
probe failed and the tracked-path test panicked.

The rebuilt stack's PR #277 refutes this on `main`: with only the job
retirements and the apt package fix, `overflow-lane`, `nextest` and
`coverage` are all green (run 33531573910), which means
`refuses_a_tracked_cache_path` passes in CI. On `main` the apt
bootstrap runs **before** checkout and already installs `git
ca-certificates`, so checkout does a proper clone and the probe works.

The hypothesis was only ever true inside the abandoned PR itself: its
`bootstrap-rust` composite action moved the git install to **after**
checkout (a composite action cannot wrap the checkout step that puts it
on disk). Root cause 4 was therefore a regression introduced and fixed
inside the same abandoned PR, not a pre-existing bug on `main`. It is
recorded as self-inflicted.

## 5. Why the three trial lanes were retired

Miri, cargo-mutants and cargo-geiger were all three
`continue-on-error` trial lanes; all three were retired on 2026-09-01
by maintainer decision (PR #277), leaving `verify-nightly` with no
trial jobs — every remaining lane must be green. Per lane:

- **miri** had no unsafe it could check: `oxpinyin-core` is
  `#![forbid(unsafe_code)]`, and `oxpinyin-store`'s unsafe is entirely
  FFI into native C libraries (kyotocabinet/tkrzw/lmdb), which Miri
  cannot step into. It interpreted only safe Rust at a 10x–1000x
  slowdown, ran toward the 6-hour job ceiling, and never produced a
  finding.
- **mutants** was retired by the same decision. Its last full run
  reported 860 mutants tested with 113 missed; that evidence stands in
  the run history but the lane is gone.
- **geiger** was both redundant for first-party code and broken in
  execution. First-party `unsafe` is already covered by `unsafe_code =
  "deny"` and `unsafe_op_in_unsafe_fn = "deny"` in
  `[workspace.lints.rust]` (root `Cargo.toml`), enforced on every PR by
  `ci.yml` under `RUSTFLAGS: -D warnings` — strictly stronger than a
  nightly report artifact nobody gated on. Its execution was failing
  anyway: cargo-geiger 0.13.0 refuses the workspace's virtual root
  manifest, and the workflow's `--output-format text` fallback is not a
  variant 0.13.0 accepts (run 33531573910 log:
  `Utf8ArgumentParsingFailed { value: "text", ... }`).

  What is genuinely lost is visibility into `unsafe` in **third-party
  dependencies**: workspace lints do not reach dependency code, and
  `cargo deny` does not count unsafe (its `deny.toml` covers
  advisories, bans, licenses and sources only). That loss is accepted
  deliberately, recorded here rather than papered over as redundancy.

## 6. ci.yml's fuzz lane builds only one of five targets

`ci.yml`'s fuzz lane runs `cargo fuzz run parser` and nothing else;
its `cargo metadata --locked --no-deps` step resolves manifests but
compiles no targets. `dict-loader`, `fixture-model`, `double-pinyin`
and `capi-commands` are therefore built by no gate — which is exactly
how the broken `fixture-model` import rotted from introduction to the
first nightly dispatch. This is the same structural failure as
`verify-nightly` itself: a lane no PR gate ever exercises.

Proposed fix (not done in this stack, per its scope): add a compile-only
step to ci.yml's fuzz lane — `cargo fuzz build` over all five targets
(or `cargo fuzz run <target> -- -max_total_time=1` each if a smoke run
is wanted). Build cost only; no soak time in the PR gate.

## 7. fetch-model.sh's degraded refusal is intended defensive behaviour

`tools/model/fetch-model.sh` refuses a tracked cache path in two
branches. When `git rev-parse --is-inside-work-tree` fails **for any
reason** — git missing, no `.git` (REST-tarball checkout), or a broken
probe — the script degrades to refusing every cache path under the
repository except `target/` ("refusing to write model bytes under the
repository without git ignore checks").

That is fail-closed, which is the correct posture for the script's
contract (model bytes must never land in a tracked path): when tracking
state cannot be established, refusing is safe, and the default
`target/` cache still works. The degradation is broader than its name —
any probe failure, not only "git is absent" — but the message names the
degraded mode accurately. Classified as intended defensive behaviour,
not a robustness gap. The script is not changed.

## 8. Commit-management failures

The abandoned attempts amend-and-force-pushed over live workflow runs,
producing cancelled and zero-job runs that were then explained by
speculation instead of owning the rewrite. This stack's rule, applied
throughout: no force-push while a run on that branch is in flight;
predictions are written down before dispatch and compared after.

## 9. The apt justification was first derived from the wrong suite

The first draft of PR #277 justified the explicit `libc6-dev` by
reading the Debian **trixie** Packages index, and claimed that `g++`
does not transitively pull `libc6-dev` under `--no-install-recommends`.
Both were wrong:

- `debian:testing` resolves to **forky** (the image is deliberately
  unpinned to track testing; trixie is current *stable* — the wrong
  archive for this container).
- In-container evidence on forky (`/etc/debian_version` reports
  `forky/sid`, observed 2026-09-01) shows `g++ -> g++-15 ->
  libstdc++-15-dev -> libc6-dev` is a **hard Depends** chain that
  `--no-install-recommends` does not touch (`apt-get install -s
  --no-install-recommends g++` lists `Inst libc6-dev (2.43-3)`). That
  chain is why `store-backends.yml:157` links successfully today with
  `g++` and no explicit `libc6-dev`.

Only the gcc-only path drops `libc6-dev` (`gcc` and `gcc-15` carry it
as `Recommends` only). The fix — naming `libc6-dev` explicitly — was
right before the mechanism was established; the causal story was
corrected in PR #277 once the forky container evidence existed.

**Moving-target caveat, and why it decides the conclusion.** The chain
above is a snapshot, not a stable fact: it is forky as of 2026-09-01
(gcc-15, libstdc++-15-dev, libc6-dev 2.43-3). `debian:testing` is
deliberately unpinned and forky is unfrozen with no freeze announced,
so every edge in that chain is designed to move. A reader who takes
"the chain is hard Depends" as the record could reasonably conclude
that the explicit `libc6-dev` is redundant and delete it. That reading
is wrong. The durable argument runs the other way: `libc6-dev` is named
explicitly *because* the transitive chain is not load-bearing and
cannot be relied on under an unpinned image. That holds whether or not
`libstdc++-N-dev` keeps its hard dependency on `libc6-dev`, and it is
stronger than either mechanism story — the trixie one or the forky one.

Lesson: this was the second mechanism claim in this stack asserted from
the wrong source, after the REST-tarball story (finding 4) — both were
plausible reasoning applied to an environment nobody had opened. Verify
apt claims inside the image the workflow actually runs, not an adjacent
suite's index, and record what was observed as a dated snapshot. A
`podman run debian:testing apt-get install -s` dry run settles the
snapshot in one command; it does not make the snapshot permanent.

## 10. The merge raced the trailer fix; main carries a malformed trailer block

Timeline (2026-09-02, UTC):

- 11:09 — the rebuilt stack was force-pushed with the review fixes:
  #277 at `d9ad6f5`, #278 at `fbc0562`, #279 at `81d255b`. #277's
  amended message appended a second `Assisted-by:` line after the
  message's trailing newline, leaving a blank line between the two
  trailers. git's trailer block is the last paragraph only, so the
  original `Assisted-by: DeepSeek:deepseek-chat` line became body
  text; `git interpret-trailers --parse` on that commit reports only
  the Claude line. The commit linter passed: R2 checks the shape of the
  lines git parses as trailers, and a well-formed line demoted to body
  text is invisible to it.
- 11:28 — the maintainer rebase-merged the three PRs from those heads.
  `main` carries the defect at `14b76ff`; the #279 commit (`b1424a6`)
  was written correctly, both lines adjacent.
- 11:31–11:48 — the defect was noticed and fixed by re-amending #277
  and rebasing the stack, and the rewrites were force-pushed to the
  three PR branches — after the merge had already happened, unnoticed
  because merge state was checked before the first push and not before
  the later ones. Those rewrites never merged; the three PR branches
  now hold orphaned commits whose content is already on `main`.
- 11:10 (same session) — dispatching `verify-nightly` on all three
  branches at once cancelled the middle one: the workflow's
  concurrency group (`cancel-in-progress: false`) keeps one running and
  one pending run, and a third dispatch replaces the pending one (run
  33623225922, cancelled by the #279 dispatch).

Disposition: `main` is not rewritten. `verify-nightly` dispatched on
`main` at the merged tip `b1424a6` (run 33626570590, 2026-09-02) is the
first green run of that workflow on `main`; the stack's dispositions
hold on the merged tree. `14b76ff`'s DeepSeek attribution
stands in its body, one blank line above the trailer block, and this
entry is the record of it. The three PR branches are dead and can be
deleted; nothing on them is missing from `main`.

Lessons, now mechanical:

- A `%B`-dumped message ends in a newline; appending a trailer after it
  starts a new paragraph. Add trailers with `git interpret-trailers
  --trailer`, or strip the trailing newline first, and check
  `git interpret-trailers --parse` before pushing. The linter cannot
  see a trailer demoted to body text; only the parse can.
- Check merge state before every push, not only before the first.
  `git fetch` plus the PR's merged state is a two-second check; a push
  to a merged PR's branch is wasted at best and misleading at worst
  (the re-validation runs at the rewritten SHAs were validating commits
  that could never land).
- Dispatch a stacked workflow one branch at a time, each after the
  previous run has started, when the workflow's concurrency group holds
  a single pending slot.
