# Oracle pin 0c5e80e1 → 074a2219 — verification record

Date: 2026-09-06 UTC · Status: verification for the pin-change PR
(`chore/oracle-pin-074a221`); human rulings 2026-09-06 UTC accepted V2's
withdrawal and V3's reframing.

This document records the three Step-0 verifications behind the pin
change, the corpus re-verification, and where the raw evidence lives
(`docs/findings/oracle-pin-074a221-evidence/`, SHA-256s in its
`SHA256SUMS.txt`). Instruments: throwaway debian:testing containers and
scratch worktrees, all deleted after use; the shared checkout was never
touched.

## The pin

libpinyin moves `2.11.91`/`0c5e80e1…` → `2.11.92`/`074a2219…` (nine
commits; `2.11.92` is untagged upstream, so the recipe fetches by commit
SHA — verified by `git rev-parse`, not by an archive hash). ibus-libpinyin
stays `1.16.5`; the model stays `model20-59c68e89…`; the DBM stays Tkrzw.
oxpinyin's drop-in identity stays `2.11.91` (distros ship it; the pin PR
does not claim an unreleased version).

## V1 — the keystroke-bench container (STOP, cleared by human ruling)

Container `c17a2e4a4e70` (recorded in `~/.local/opt/backend-matrix-container.txt`)
exists on neither this host's docker daemon nor its filesystem. Ruling
2026-09-06 UTC: it was recorded on a different host (this daemon is arm64;
the backend-matrix workstream is x86_64); proceed without it, do not
recreate, do not touch the Option B workstream. This session's cleanups
removed only its own named containers and image and can be audited in
the session transcripts.

## V2 — determinism control (attribution withdrawn)

Two clean builds of the oracle at the OLD pin (separate work dirs, same
model, same image, unmodified `build-oracle.sh`): exactly **6 of 23**
files under `lib/libpinyin/data` differ — `addon_phrase_index.bin`,
`addon_pinyin_index.bin`, `bigram.db`, `phrase_index.bin`,
`pinyin_index.bin`, `punct.bin` (the DBM-generation-backed set). The
other 17 are byte-stable across builds and byte-identical between pins.

Consequences, accepted by ruling:

- The earlier between-pins attribution of those 6 files to upstream
  `03bc5ef` is **withdrawn** — they differ between any two builds, pin
  or not. No between-pins claim is made for them in either direction.
- The manifest gates on them, so a fresh prefix at a fixed pin rewrites
  `oracle-data.sha256` — issue **#358**.
- Evidence: `determinism-v2.diff`, `data-sha-0c5e80e1-build{1,2}.txt`,
  `data-sha-074a221-build1.txt`.

## V3 — `_check_offset` (divergence CLOSING, not opening)

Static: 10 call sites at the pin, all bare; at 074a221 six become
`assert(_check_offset(...))`, four stay bare (2226, 2933, 2956, 3251).
Runtime corrections (fork-per-probe C driver, `v3-probe-driver.c`; full
matrix in `v3-matrix.log`; oracle@both pins vs port):

- **3251 is dead code** (`#if 0` overload); the live
  `pinyin_get_character_offset` check is the assert-wrapped `:3204` —
  it aborts at BOTH pins. No pin-to-pin change.
- `pinyin_get_pinyin_key`/`_pinyin_key_rest` (`:2933`/`:2956`) return
  `false` at their range guard before the check on this shape —
  identical at both pins and the port.
- **The one pin-to-pin change: `pinyin_guess_candidates` (`:2226`).** On
  `ni'` (a corpus input, `09-edge.txt` — invisibility comes from the
  harness never querying these offsets, not from input absence):

  | offset | 0c5e80e1 | 074a221 | port |
  |---|---|---|---|
  | 0–2 | true, 0 cands | true, 0 cands | true, 0 cands |
  | 3 (one-past-end) | **ABORT** (`:2175`) | **true, 0 cands** | **true, 0 cands** |
  | 4 (illegal) | **ABORT** | true, 0 cands | **false** |

  At the legal boundary upstream moved TO the port — a divergence
  closing. The offset-4 residual (port stricter than upstream on an
  illegal offset) is the already-registered abort-on-caller-input
  class; per ruling it is an amendment to the `ZeroKeyOffsetCheck`
  register entry (appended in `upstream-divergences.md`), NOT a defect.
- Controls: `get_right_pinyin_offset("nihao", 5/6)` aborts at both pins
  (old via inner assert, new via caller asserts) — assert-wrapped sites
  unchanged.
- Unrelated side observation on invalid-phrase input, filed as issue
  **#356**: the port's `pinyin_get_character_offset` answers `true`
  where both oracles answer `false`. Fixed on main after this
  verification landed (`9d369d7c`, 2026-09-06 UTC); the table above
  records the port as it stood at `87f25055`.
- F-E-14 (apostrophe-only empty-matrix abort) is a different assert,
  untouched by the range — still present at both pins; the classes are
  independent.

## Pin-bump surface verification (2026-09-06 UTC, debian:testing container)

The rebuilt oracle (new git-fetch recipe, unpatched) re-verified against
the frozen fixtures:

- **Candidate surface: byte-identical.** `oracle-candidates` at 074a221
  produces 97,442 triples over 10,465 inputs / 10,312 distinct / 10,037
  with candidates — the frozen fixture's exact counts — with a zero-line
  payload diff. A superset of the 10,190-input parity subset, hence
  10,190/10,190 there.
- **Parity metric (`corpus-tail`, run outright, verbatim):**

  ```
  compared            10190
  top-1 misses        0
  top-5 misses        0
  absent              0
  order-only          0
  prefix-10 gap       0 of 98930 (positions not in our top-10)
  ```

  i.e. 10,190/10,190 and 98,930/98,930 with absent 0 and tie-swaps 0 —
  the frozen `real_tables_session_reports_parity` pins hold against the
  regenerated fixture.
- **Sentence surface (verbatim):** comparable 496, guessed
  disagreements 0, row-0 491/496 (5 miss), ordered 390/496 (106 miss:
  0 order-only, 106 set-diff), distinct-set 396/496 (6 distinct-same),
  breakdown row0 5 / row1 81 / row2 20, first-6 rows 390/496 — the §12
  freeze exactly.
- **Parse-level surface (paths fixture, regenerated):** a fresh
  `parity-diff` run against the live 074a221 oracle (9,508
  output-identical, 468 tie-swap, 468 divergence-only) rebuilt
  `fixtures/w4/oracle-paths.txt`; the payload is byte-identical to the
  2.11.91 fixture — only the `pin_ref` stamp line changed in any w4
  fixture (one line in each of the four files).
- `fullpin-aux-overread.patch` applies at 074a221 (`--forward` dry-run,
  one hunk at a 2-line offset, no rejects).
- `libpinyin.so.15.0.0` at both pins (`libpinyin_abi_current=15`,
  revision 0); the public `pinyin.h` is byte-identical across pins.

## Review follow-up (2026-09-06 UTC, post-rebase)

Rebased onto main `c6b371da` (#356 and #358 now fixed there; #357 remains
open and blocked on this merge). Results, all from the rebased tree:

- **Merged `build-oracle.sh`**: diffed against both parents. Kept from
  main: the #358 split-manifest machinery (17-file reproducible gate +
  6-file informational unstable manifest, `data_unstable_manifest_sha256`
  manifest line). Kept from this branch: the commit-SHA git fetch with
  `rev-parse` verification, SHA-named source dir, version-named header
  path and pin ref, `git` in the required commands. The rebuilt prefix
  emits both manifests (17 + 6 files) and the unstable manifest line.
- **Metrics (post-rebase, verbatim).** `corpus-tail`: compared 10,190,
  top-1 misses 0, top-5 misses 0, absent 0, order-only 0, prefix-10 gap
  0 of 98,930. `sentence-tail`: comparable 496, guessed disagreements 0,
  row-0 491/496 (5 miss), ordered 390/496 (106 miss: 0 order-only,
  106 set-diff), distinct-set 396/496 (6 distinct-same), breakdown 5/81/20,
  first-6 rows 390/496.
- **Foundation capture reproduction**: fresh `run-capture.sh` over the
  merged-script prefix reproduces the committed f-a/f-c SHA-256s exactly
  (`6690f849…`, `1712555f…`).
- **Tests (post-rebase)**: `pinyin-oracle` lib 68 passed / 0 failed;
  `live_smoke` against the live prefix 9 passed / 0 failed.
- **Dates**: every stamp in this changeset re-dated to UTC (they were
  hand-written from SGT local, a day ahead); AGENTS.md now carries the
  UTC-date convention.
- **Perf notes**: rewritten to state that timing at `074a2219` was not
  measured and every commit's basis for "no Linux runtime-path change";
  cross-pin timing comparison still requires re-measurement.

### Image-build boundary (D3)

Built from the branch tree:

- `Dockerfile.perf-matrix` — **both libpinyin cells pass** (git-fetched,
  configure-less source through `autoreconf --force --install`,
  `./configure`, `make`, `make install`; RC 0). The cargo-c/datagen tail
  after Cell B was not exercised, per the granted relaxation; that image
  carries `liblz4-dev` and `libzstd-dev`, so the step the other two
  images fail on is expected to pass there — but that is expected, not
  proven.
- `Dockerfile.perf-baseline` and `Dockerfile.perf-validation` — the
  `git` fix is verified working: `build-oracle.sh` completes inside both
  images (libpinyin installs into `/opt/pinyin-oracle`). Both full
  builds then fail at the later `cargo run -p oxpinyin-datagen` step:
  `-llz4`/`-lzstd` not found — the images' apt lists predate the tkrzw
  default backend and lack `liblz4-dev`/`libzstd-dev`/`liblzma-dev`
  (libtkrzw-dev does not pull them). Reproduced on pristine main
  `c6b371da` with the same package list: pre-existing main-side
  breakage, not introduced by this PR, and out of scope here — tracked
  as #370.

## Known inconsistency shipped with the pin

`tools/oracle/oracle-pin.txt` (schema `oracle-provisioning-pin-v2`)
verifies its two upstreams asymmetrically: libpinyin by commit SHA
(forced — `2.11.92` is untagged upstream), ibus-libpinyin still by its
tagged archive's SHA-256. A follow-up issue tracks moving ibus to
commit-SHA verification; this PR does not change the ibus pin.

## Issues filed from this verification

- **#356** — port `pinyin_get_character_offset` true-on-invalid-phrase
  (parity defect, both pins disagree with the port).
- **#357** — `7165d2a` vs `oxpinyin-kmm`'s mirrored pre-fix
  `set_array_header` no-op (trainer surface only).
- **#358** — `oracle-data.sha256` gates on the 6 nondeterministic
  files. Fixed on main after this verification landed (`e2f57d52`,
  2026-09-06 UTC): the manifest is now split into a reproducible gate
  (the 17 stable files) and an informational unstable section.

## Collected for the upstream report (not filed upstream)

1. `074a221` places a function call inside `assert()` at six of ten
   `_check_offset` call sites, so the check is not called at all under
   `NDEBUG`. It is pure, so nothing breaks today, but the construct is
   a defect.
2. libpinyin's generated data is not reproducible: 6 of 23 files differ
   between two clean builds at a fixed pin (the DBM-backed generation
   path). Relevant to distro reproducible-builds work.
