# W8 report — bootstrap milestone

Date: 2026-08-17 · Status: **synthesis only**. Every number and verdict
below already exists in a findings document, a PR body, or the fork's
`docs/oxpinyin-switch.md`. Nothing was re-measured for this file; no code
changed.

W8 closes the **bootstrap milestone**, not Stage 1. The remaining Stage-1
parity work is W10–W13 (`ROADMAP.md`, post-#90).

---

## 1. What W8 was

W8 is the oxpinyin library release plus a compatibility bootstrap for the
maintainer's ibus-libpinyin fork (`ROADMAP.md` W8 note; `docs/findings/abi-subset.md`
"W8 bootstrap contract").

The bootstrap contract is the fork's live **51-symbol** call surface: the 50
`pinyin_*` symbols pinned from ibus-libpinyin `1.16.5`, plus
`pinyin_get_parsed_input_length` (fork commit `2c5baa9`). For W8 that fork
surface supersedes the upstream tag freeze. The first oxpinyin release ships
this as a binary the fork links against with minimal changes — enough to
switch the fork off the C++ libpinyin backend and onto oxpinyin.

After that first release the surface is **free to evolve**. Long-term soname
or header compatibility with upstream libpinyin is a non-goal: the fork and
oxpinyin evolve together.

Two precedents, cited for what they inform (`ROADMAP.md`, not re-derived
here):

- **libchewing** (Kan-Ru Chen): a library-only rewrite; frontend packages
  were left alone. oxpinyin follows this pattern.
- **pinyin → libpinyin** (Peng Huang → Peng Wu): historically a new library
  name with a new frontend, no drop-in. oxpinyin's bootstrap is a
  transitional inversion of that — the first release **is** a working swap
  for the fork — but the long-term shape returns to the pinyin → libpinyin
  pattern: own library, own frontend fork, no upstream compatibility
  promise.

Acceptance for the initial release (`ROADMAP.md`): the fork builds against
oxpinyin's compatibility surface with minimal changes, and the resulting
engine produces the same wire-level output on a scripted input sequence as
the pinned upstream configuration. This report is the last acceptance
input: the compatibility verdict, the Stage-2 measurement baseline, and
the packaging record.

---

## 2. Compatibility verdict

**Headline:** tie-order-only parity on the defined bootstrap surface.

The same installed fork binary
(`shenghaoc/ibus-libpinyin`, `feat/oxpinyin-backend`, tip `612004e`)
against clean oxpinyin `main` (revalidation checkout `3ec6172`,
`git status --porcelain` empty, zero local patches) produces the same
observable stream as the pinned C++ backend except one equal-cost
candidate tie-swap: input `be`, oracle `避恶, 保额` vs oxpinyin
`保额, 避恶`. The two texts have identical rank keys (phrase length 2,
pinyin span 2, real unigram count 2).
`tools/oxpinyin-parity/compare-streams.py` verdict: **TIE-ORDER-ONLY**.
(**SHOWN**, fork `docs/oxpinyin-switch.md` revalidation.)

This is **not** universal wire parity. The five caveats below are
load-bearing. Each gap has an owning workstream.

### Caveat 1 — one parity profile, not fork defaults

The gate uses GSettings `correct-pinyin=false`, `fuzzy-pinyin=false`,
`dynamic-adjust=false`, `incomplete-pinyin=true`, sort option 2
(`oxpinyin-switch.md` Phase 3). Correction (`PINYIN_CORRECT_*`),
fuzzy/ambiguity (`PINYIN_AMB_*`), and `DYNAMIC_ADJUST` are still not
decoded by oxpinyin-engine; correct-pinyin is on by default in the fork's
gschema (`ROADMAP.md` W10). Default-settings parity is W10, not this
verdict.

### Caveat 2 — `network.txt` emptied for the gate

The installed `network.txt` is emptied so imported network phrases cannot
leak into the oracle candidate table (`oxpinyin-switch.md` Phase 3).
oxpinyin decode reads a single system index; user, network, and addon
phrases do not surface. Upstream unions them through **two**
`FacadePhraseIndex` instances (default space including user + network;
separate empty-at-init addon space) (`docs/findings/phrase-union.md` §3.2,
PR #91; `ROADMAP.md` W11). User/network/addon surfacing is W11.

### Caveat 3 — the corpus residual

On the W2 corpus / `oracle-candidates.txt` surface, after the #85
re-freeze (`docs/findings/pin-refreeze-2026-08.md`;
`docs/findings/candidate-construction.md` §0, §8.2; `ROADMAP.md` W12):

| residual | value |
|---|---|
| top-1 differences | 13 of 10,190 |
| prefix-10 positions beyond tie-order | ~4,059 (98,930 − 94,871) |
| order-only tie-swaps | 1,036 |

W12 owns diagnosis of this tail. It is open-ended: one systematic cause or
thirteen separate ones — undiagnosed.

### Caveat 4 — provisional stand-ins on the 51-symbol surface

Still open on oxpinyin `main` as of the revalidation
(`oxpinyin-switch.md` "Findings / current known gaps"):

- Prediction APIs (`pinyin_guess_predicted_candidates_with_punctuations`,
  `pinyin_choose_predicted_candidate`) remain no-op / false → **W11**.
- Double-pinyin and chewing auxiliary text remain provisional (preedit
  text rather than C++-formatted key text). The schemes themselves —
  parsers and formatted aux — are W13 (`ROADMAP.md`).
- `pinyin_load_addon_phrase_library` remains a provisional no-op → **W11**.

### Caveat 5 — the parity profile never reaches `pinyin_train`

Sort option 2 excludes NBEST / sentence candidates, so the bootstrap
parity sequence never calls `pinyin_train` (`oxpinyin-switch.md`
revalidation, "Save/train cycle"; `docs/findings/perf-baseline-2026-08.md`
caveats). Training was verified under a **second** profile
(`sort-candidate-option=1`, `LIBPINYIN_SAVE_TIMEOUT_SECONDS=5`):
`pinyin_train → 1`, timer-driven `pinyin_save → 1`, `user_store.redb`
compacted from 1,056,768 bytes to 32,768 bytes. That is train → modified
→ timeout → save. It is not part of the TIE-ORDER-ONLY stream.

**Erratum.** The gap list omitted the sentence surface. That surface
(W14 / #100) was not yet in scope at the time of this report.

---

## 3. The pin history

The candidate-construction pins were re-frozen at #85. The re-freeze
**is** the fidelity changelog (`docs/findings/pin-refreeze-2026-08.md`;
PR #85 body):

| metric | pre-#85 | post-#85 |
|---|---:|---:|
| top-1 | 10,136 | **10,177** |
| top-5-set | 10,182 | **10,189** |
| prefix-10 overlap | 94,456 of 98,930 | **94,871 of 98,930** |
| absent | 1 | **1** |
| tie-swaps (order-only) | 1,030 | **1,036** |

Cause: `oxpinyin-core` expanded an initial-only key by **string prefix**.
The pinned C++ incomplete index is keyed by `ChewingKey.m_initial`. That
was a Stage-1 reproduction bug, not a scoring-taste change. The fix
(expand by phonetic initial) strictly increased oracle agreement on every
re-frozen metric and moved no import or train differential.

The fork's Phase-3 record still prints the pre-#85 pins
(`oxpinyin-switch.md` Phase 3: `10136 / 10182 / 94456 of 98930 / absent 1`
and 1,030 tie-swaps). That is the historical wire-level run, not the
frozen contract. The revalidation at `612004e` re-ran the wire sequence
after #85 and kept the TIE-ORDER-ONLY verdict; the corpus pins live in
`pin-refreeze-2026-08.md`.

Pins move only by deliberate, documented re-freeze.

---

## 4. Performance baseline

The Stage-2 scoreboard, from `docs/findings/perf-baseline-2026-08.md`
(installed `libpinyin_capi.so` via `pkg-config oxpinyin`, same dlopen
harness and W8 parity profile `0x18a` / sort `0x1e`). Verdicts first,
then the table numbers they rest on.

### Per goal

- **Smaller binary: mixed.** The shared object is **0.40×** (2.18 vs
  5.44 MiB) — that is the win. Runtime footprint is **3.07×** (125.96 vs
  40.98 MiB), driven by the 79.59 MiB runtime-mandatory
  `interpolation2.text` (public `pinyin_init` fail-closed since #84).
- **Faster: no.** Steady cycle **2.19×** (repeat-run band 2.09–2.26×),
  cold cycle **2.06×**, `pinyin_init` **158×** (586.472 ms vs 3.711 ms).
  `pinyin_alloc_instance` is 48.483 ms vs 0.001 ms (~1 µs).
- **Much less RAM: no.** Post-init RSS **8.22×** (98,708 vs 12,012 KiB),
  lifetime peak HWM **6.42×** (98,804 vs 15,398 KiB). Kind of memory
  (**SHOWN** as `RssAnon` / `RssFile`): after init, oracle is 1,420 KiB
  anonymous / 10,612 KiB file-backed; oxpinyin is 95,542 KiB anonymous /
  3,156 KiB file-backed. Oracle's resident set is mostly file-backed
  pages of mmap'd binary tables (evictable under memory pressure —
  **INFERRED** from that file-backed shape). oxpinyin's is anonymous
  heap from materialized redb / text-model structures.

### Ranked Stage-2 observations

Re-ranked from the findings-doc list per maintainer review. Observations,
not commitments.

1. **One cause, three symptoms.** Compile `interpolation2.text` to a
   binary / redb form at data-prep time and read-through instead of
   materializing it (and the three redb tables) at `pinyin_init`. That
   single representation change attacks the 158× init, the 8.22× RSS,
   and the 3.48× runtime-data ratio together. It also converges with
   upstream's own split: `interpolation2.text` is the textual
   interchange / export (`docs/findings/training-algorithm.md` §8);
   libpinyin ships binary tables at runtime
   (`docs/findings/data-formats.md`; perf-baseline Axis 2: "an ASCII
   text file that libpinyin ships in binary tables").
2. **`pinyin_alloc_instance` 48.483 ms.** Per-editor (**INFERRED** from
   the ABI: the fork calls `pinyin_alloc_instance` once per editor),
   user-visible, undiagnosed. The 48.483 ms figure is **SHOWN**; no
   finding yet says why it is 48 ms rather than the oracle's ~1 µs.
3. **Hot-decode allocation.** Steady state is 2.19×. The prior
   Callgrind / alloc profile points at `LookupTable::get`, redb key
   `memcmp`, candidate-string clones, and `Vec<Candidate>` growth
   (perf-baseline observation 2).
4. **(demoted) the cargo-c `.a`.** 27,094,770 bytes of the 29,382,810-byte
   installed-code column, not on the shared-library runtime path
   (perf-baseline Axis 2). Distros can subpackage it; the honest size
   number for Stage 2 is the runtime footprint (3.07×), not total
   install (2.59×).

---

## 5. Packaging

`docs/packaging.md` and PR #89.

- Tooling: **cargo-c**. `pinyin.h` ships verbatim (no cbindgen
  regeneration).
- `.pc` name: **`oxpinyin`**. Library basename: **`pinyin_capi`**.
  SONAME: **`libpinyin_capi.so.0.1`**.
- **Minor-version coupling caveat** (0.x): cargo-c derives the SONAME
  from `library.version`. A `0.1.0` → `0.2.0` bump changes the SONAME
  to `libpinyin_capi.so.0.2` and breaks the fork's dynamic link. At 1.0,
  decide whether to hold `library.version` so the SONAME bumps only on a
  deliberate C-ABI break.
- **Data-files gap:** `cargo cinstall` ships the `.so` / `.a`,
  `pinyin.h`, and `oxpinyin.pc`. Nothing installs the `.redb` tables or
  `interpolation2.text`. The documented locator
  `$(pkg-config --variable=prefix oxpinyin)/share/oxpinyin` is
  **unenforced** — nothing writes there and nothing validates the path.
- **Fork switch line** (PR #89 handoff; the `.pc` name is
  `docs/packaging.md`): after this packaging, the fork drops
  `--with-oxpinyin-capi=<checkout>` + RPATH into `target/release` and
  resolves oxpinyin with

  ```
  PKG_CHECK_MODULES(LIBPINYIN, [oxpinyin])
  ```

  The revalidation at `612004e` still used the checkout/RPATH path;
  the `.pc` is what a packaged consumer uses.

---

## 6. What closes with this report / what does not

**Closes:** the W8 bootstrap milestone, per `ROADMAP.md` post-#90
acceptance. The fork is switched and running against oxpinyin;
wire-level parity holds on the defined bootstrap surface (with the
§2 caveats); cargo-c packaging exists; this report is the compatibility
+ performance record that establishes the Stage-2 measurement baseline.

**Does not close:** Stage 1. Parity continues through W10–W13
(`ROADMAP.md`):

- **W10** — option bits: correction, fuzzy/ambiguity, `DYNAMIC_ADJUST`.
- **W11** — phrase-index union at lookup (user / network / addon) plus
  the prediction APIs and `pinyin_load_addon_phrase_library`.
- **W12** — corpus tail (the §2 caveat 3 residual) and live-typing
  behaviours the parity sequence does not exercise.
- **W13** — double-pinyin and bopomofo input schemes, including the
  provisional parsers and auxiliary text.

**Release note.** `v0.1.0` is tagged on the existing history;
commit-management failures are documented in
`docs/findings/commit-hygiene-failures.md`, not rewritten away.

---

## Source index

| claim | source |
|---|---|
| W8 = bootstrap, not Stage 1; acceptance; W10–W13 | `ROADMAP.md` (post-#90) |
| 51-symbol fork surface; 50 + `pinyin_get_parsed_input_length`; `2c5baa9`; free to evolve; soname non-goal | `ROADMAP.md`; `docs/findings/abi-subset.md` |
| libchewing / pinyin→libpinyin precedents | `ROADMAP.md` (copied, not re-derived) |
| TIE-ORDER-ONLY; `be` 避恶/保额; rank keys (2, 2, 2); fork tip `612004e`; oxpinyin `3ec6172`; zero local patches | fork `docs/oxpinyin-switch.md` revalidation |
| parity profile `correct/fuzzy/dynamic-adjust` off; sort option 2 | `oxpinyin-switch.md` Phase 3 |
| correction/fuzzy/`DYNAMIC_ADJUST` undecoded; W10 | `oxpinyin-switch.md` gaps; `ROADMAP.md` W10 |
| `network.txt` emptied | `oxpinyin-switch.md` Phase 3; `ROADMAP.md` W11 |
| two-facade union (default vs addon) | `docs/findings/phrase-union.md` §3.2 (PR #91) |
| 13 / 10,190 top-1; ~4,059 prefix-10; 1,036 tie-swaps | `ROADMAP.md` W12; `pin-refreeze-2026-08.md`; `candidate-construction.md` §0 / §8.2 |
| prediction / addon no-ops; double-pinyin & chewing aux provisional | `oxpinyin-switch.md` gaps |
| parity profile never reaches `pinyin_train`; second-profile train→save; 1,056,768 → 32,768 | `oxpinyin-switch.md` revalidation |
| pins 10136/10182/94456/1030 → 10177/10189/94871/1036 at #85; string-prefix vs phonetic-initial | `pin-refreeze-2026-08.md`; PR #85 |
| Phase 3 still prints pre-#85 pins | `oxpinyin-switch.md` Phase 3 |
| .so 0.40× (2.18 / 5.44 MiB); runtime 3.07×; 79.59 MiB text model; `.a` 27,094,770 B of 29,382,810 B installed code | `perf-baseline-2026-08.md` Axis 2 / Stage-2 verdict |
| steady 2.19× (2.09–2.26); cold 2.06×; init 158× (586.472 / 3.711 ms); alloc 48.483 / 0.001 ms | `perf-baseline-2026-08.md` Axis 1 |
| post-init 8.22× (98,708 / 12,012 KiB); peak 6.42× (98,804 / 15,398 KiB); RssAnon/RssFile | `perf-baseline-2026-08.md` Axis 3 |
| text = interchange, binary = runtime | `training-algorithm.md` §8; `data-formats.md`; perf-baseline Axis 2 |
| cargo-c; `.pc` = oxpinyin; lib = `pinyin_capi`; SONAME `libpinyin_capi.so.0.1`; 0.x coupling; data-files gap | `docs/packaging.md` |
| `PKG_CHECK_MODULES(LIBPINYIN, [oxpinyin])` | PR #89 body (handoff); `.pc` name from `docs/packaging.md` |
