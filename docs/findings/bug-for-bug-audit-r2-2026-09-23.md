# Bug-for-bug audit, round 2 (partial, salvaged)

## 0. Status

**This report is PARTIAL.** Read this section before any verdict below.

- The execution phase ran from 2026-09-24T15:06Z to 2026-09-24T23:01Z UTC. It
  was stopped by the maintainer's instruction ("salvage results") before most
  axis agents had finished.
  - Two axes returned complete ledgers: C (driver part) and E.
  - Four were salvaged from the files their agents had written: B (libpinyin
    half), D, F and G.
  - The other axes are partial, as the table below states.
- **Axis L (complexity) never ran.**
- **Step 4 (line-by-line verification of round 1, PR #516) was not done.**
  The round-1 report was never opened. The round-1 table (section 6) is
  therefore empty.
- **Step 5 (GitHub tracking) was not done.**
  - No label, milestone, project or issue exists yet.
  - The `gh` token still lacks the `project` scope.
- **Measured subject:** `origin/main` at `18d782089bd1`. Main has since moved
  to `34a66bc915c9`, adding 00466d50 (`expand_keys` early stop in
  `oxpinyin-core/src/scoring.rs`) and 34a66bc9 (a fuzz corpus seed). Nothing
  here was re-measured on the new tip.
- **Findings were not adversarially re-verified.** Subagent enumeration was
  spot-checked (section 1.3). Salvaged verdicts come from the raw run
  artifacts, not from a finished agent's conclusion.

| axis | tkrzw | bdb | kc | basis |
|---|---|---|---|---|
| A ABI surface | static MATCH (exc. .pc); layout MATCH | same | same | A-m1 and A-m3 detected and reverted; abidiff type section NOT-ESTABLISHED by pre-registration |
| B libpinyin contract | DIVERGENT (40 findings) | same | same | B-m1, B-m2, F-m2 detected; gated revert identical |
| B libzhuyin contract | observations only | same | same | 458 probes/cell, 278–279 differ; no mutation validation: **NOT-ESTABLISHED** for any MATCH |
| C drivers | DIVERGENT (6 kinds) | same, +1 broader | same | C-m2 detected by 6 of 13 drivers; the 7 non-detecting drivers are NOT-ESTABLISHED |
| C options/encoding/zhuyin layouts | **NOT-ESTABLISHED** | same | same | sweep killed at stop; C-m1 not run |
| D persisted state | DIVERGENT (open counter, cross-read) | same | same, +kc interop | D-m1 (round-trip) and D-m3 (cycle) detected; revert identical |
| E defect preservation | 37 DIVERGENT, 14 NE | same | same | E-m1 detected on all cells, reverted |
| F errors/diagnostics | 74 sites DIVERGENT | same | same | F-m1, F-m2 detected (tkrzw); bdb subject rerun not byte-identical (pinyin), so bdb F is NOT-ESTABLISHED for MATCH |
| G numerics | §12 gate 491/396/390 | gate passes | 491/396/390 | **gate mutations G-m1/G-m2 never run → NOT-ESTABLISHED**; counterfactuals show non-(a) selection divergences |
| H process state | DIVERGENT | same | same, +fork hang | H-m1 and H-m2 detected and reverted |
| I drop-in | DIVERGENT (.pc, consumer runtime) | same | same | I-m2 and I-m3 (C-m2) detected; header matrix fails identically on both sides |
| J coverage | export ledger 131 = 131, identical across cells (static) | same | same | internal ledger **NOT-ESTABLISHED** (not produced) |
| K register | 3 lists produced (section 7) | — | — | static reconciliation, no mutation by design |
| L complexity | **NOT RUN** | NOT RUN | NOT RUN | — |

## 1. Provenance

### 1.1 Identities

| item | value |
|---|---|
| subject | `origin/main` `18d782089bd1dff1eec95d92a9897269b011a035`. The pristine copy was checked equal to `git archive` of that commit with `diff -r` (no output) |
| libpinyin pin | 2.11.92, `074a2219c90feaf962d0d24f034514033ece5f99` (`tools/oracle/oracle-pin.txt` and `build-oracle.sh:27` agree; the scratch clone's `git rev-parse` matches) |
| ibus-libpinyin pin | 1.16.5, `2d2cdac0187101aa0cd7ac06694a8340721ddfbb` |
| container base | `debian:testing@sha256:dab11cdb0a9dcf4bbd68f671635b35f1f726b452b92396875b69bb2c7daa42a9` |
| apt | `snapshot.debian.org/archive/debian/20260831T000000Z testing main` |
| audit image | `ox-audit-r2:env`, id `d6153cd00d57bb70aabd6cc8ea43256e9e0d7183835238c87d6f16cea06f6f2c` |
| toolchain | rustc 1.97.1 (8bab26f4f 2026-07-14), cargo-c 0.10.25 |
| model | model20, archive sha256 `59c68e89…1155` (checked in-image) |
| pre-registration | `dbee7c04dab055f0d0777e846b5dff55219f2503`, committed 2026-09-23T10:54:33Z |
| first measurement | image build 2026-09-24T15:06:26Z. An earlier interrupted attempt started 2026-09-23T~11:08Z. Both are after the pre-registration |
| run window | 2026-09-24T15:06Z to 2026-09-24T23:01Z |

### 1.2 Cells

The oracle was built inside the image with exactly the pre-registered
configure arguments, and the `DBM` line printed by configure was checked.
The subject is the relinked object from `tools/packaging/install.sh`
(`cargo cinstall` followed by `relink-versioned.sh`).

| cell | oracle configure | DBM printed | oracle NEEDED backend | subject features | subject `libpinyin.so.15.0.0` sha256 |
|---|---|---|---|---|---|
| tkrzw | `--with-dbm=Tkrzw --enable-libzhuyin` | Tkrzw | libtkrzw.so.1 | `--no-default-features --features tkrzw,shipped` | `eda40100…` |
| bdb | bare `--enable-libzhuyin` | BerkeleyDB | libdb-5.3.so | default + `shipped` | `7d8b6120…` |
| kc | `--with-dbm=KyotoCabinet --enable-libzhuyin` | KyotoCabinet | libkyotocabinet.so.16 | `--no-default-features --features kyotocabinet,shipped` | `60fd22eb…` |

- The bare configure selects BerkeleyDB, so the pre-registered bdb-cell
  falsifier does not fire.
- Mutant objects were built with the same recipe from the scratch tree at
  commit `5eb5c51` ("all gated mutations").
- An E-m1 mutant was built from `b7e052f`.

### 1.3 Spot-checks of subagent enumeration

Each spot-check was done by the orchestrator against source before the claim
was used.

1. "The shipped crates log only at init failure."
   - **CONFIRMED for glib logging.** `log_warning` is called only from
     `oxpinyin-capi/src/context.rs:21` and
     `oxpinyin-zhuyin-capi/src/context.rs:44`.
   - **Corrected for stderr.** Axis E found four more stderr writers:
     `oxpinyin-runtime/src/lib.rs:987,1004`,
     `oxpinyin-user/src/store_libpinyin.rs:177` and
     `oxpinyin-store/src/bdb/ffi.rs:151`. None of them sits at an abort-site
     counterpart, so the (c) conclusion stands.
2. "The pin's fini decrements the `user.conf` open counter; the subject never
   does." **CONFIRMED** in source and then by execution (D-01).
   - Pin: `pinyin.cpp:1194-1198` and `:185-186`.
   - Subject: `oxpinyin-capi/src/context.rs:119-129` and
     `oxpinyin-user/src/persistence.rs:223-266`.
3. "Upstream issues #566/#542/#518 belong to libpinyin/ibus-libpinyin, not
   libpinyin/libpinyin." **CONFIRMED** with `gh issue view`.
4. "Row 35 was fixed by `7c9a6923`." **CONFIRMED** with
   `git merge-base --is-ancestor 7c9a6923 18d78208`.
5. "357 raw assert/abort lines." **CONFIRMED**:
   `grep -rnE '\bassert\s*\(|\babort\s*\(\s*\)' src | wc -l` at the pin gives
   357. The merged F ledger has 357 rows. Separately there are 70
   `check_result()` sites (`pinyin_utils.h:27-31` defines it as an assert).

## 2. Pre-registration and falsifiers

The pre-registration was committed as `dbee7c04` before any measurement.

| falsifier | outcome |
|---|---|
| bdb cell must select BerkeleyDB and link libdb | not fired: DBM=BerkeleyDB, NEEDED libdb-5.3.so |
| `.ver` files byte-identical to the pin | not fired: `cmp` identical, 79 + 52 |
| any one-sided export | not fired: `nm -D --defined-only` sets are equal on every cell |
| SONAME ≠ `lib{pinyin,zhuyin}.so.15` | not fired |
| global 1: pristine re-run not byte-identical | **fired** on bdb for the F (pinyin) and B battery oracle side (double-free UB text). Affected rows are NOT-ESTABLISHED for MATCH; divergence rows are unaffected because they reproduce |
| global 2: an artifact predates the pre-registration | not fired |
| global 3: subject is not the relinked `shipped` object | not fired |
| global 4: subagent enumeration used without a spot-check | not fired for the items in section 1.3; the salvaged items are marked salvaged |
| G: gate reports skipped or no oracle | not fired. But the gate compares against a **committed oracle fixture**, not each cell's live oracle, so per-cell gate runs vary only the exported tables |

### Deviations from the pre-registration

1. **F-m1 moved.** It sits on the init-failure log arm, because that is the
   only glib-logging site in the shipped crates. Its intended home, a (c)
   site's log line, does not exist at any (c) site.
2. **B-m2 needed extra plumbing.** The pristine `pinyin_train` discards
   `index` (`candidates.rs:550`), so B-m2 needs thread-local plumbing to
   observe the index at all.
3. **The C-m3 detector was extended.** The repo's `zhuyin-diff` never types
   key `1`, so it cannot detect C-m3. An extended copy (`zhuyin-diff-ext`)
   detects it.
4. **Execution stopped early,** by maintainer instruction, before
   G-m1/G-m2, C-m1, L-m1/L-m2, the J internal ledger, the full option sweep,
   B-zhuyin validation, and Steps 4–5.

## 3. Instrument validation

| axis | cell | mutation | detected | reverted | notes |
|---|---|---|---|---|---|
| A | all | A-m1 (relink without `pinyin_get_n_phrase`) | yes: abidiff "1 Removed"; relink reports 78 exports | yes | |
| A | all | A-m3 (ChewingKey bit-field swap) | yes: the layout probe moves `m_initial`/`m_middle` | yes (subject side re-run identical) | |
| A | all | A-m2, A-m4 | not recorded | — | the static comparisons are MATCH on their static basis |
| B-pinyin | all | B-m1 | yes: `get_candidate(n)` false→true | yes | |
| B-pinyin | all | B-m2 | yes: `diff_train_idx1/2/255` bigram rows→0 | yes | |
| B-pinyin | all | F-m2 | yes: `left_offset` false→true | yes | |
| C | all | C-m2 | yes, on 6 of 13 same-data-dir drivers plus every scheme/chewing/fullpin/bisect job | yes (37/37 identical unset) | key-/dict-/phrase-surface, pred-order, predict, punct and import do **not** detect C-m2 |
| C | all | C-m3 | yes, only on `scheme.bopomofo.1` and on `zhuyin-diff-ext` | yes | |
| D | all | D-m1 (round-trip) | yes (392–506 differing lines) | yes | |
| D | all | D-m3 (cycle) | yes (8 lines) | yes | |
| E | all | E-m1 (gated "fix" of URD-4c) | yes: flips reproduced → silently fixed | yes: unset identical; `git apply -R` clean | |
| F | tkrzw | F-m1 | yes: the init warning disappears | yes | |
| F | tkrzw | F-m2 | yes: `left_offset(3)` 0→1 | yes | |
| H | all | H-m1 (extra getenv) | yes: `OXPINYIN_AUDIT_H` appears | yes | |
| H | host | H-m2 (static Mutex) | yes: inventory line added | yes (`h2-reverted` == pristine) | |
| I | all | I-m2 (file removed) | yes | yes | |
| I | tkrzw | I-m3 = C-m2 | yes: the ibus replay differs by 304 vs 71 lines | yes (mut-unset == subject) | |
| G | all | G-m1, G-m2 | **not run** | — | G is NOT-ESTABLISHED |
| C-options | all | C-m1 | **not run** | — | |
| L | all | L-m1, L-m2 | **not run** | — | |

## 4. Ledger: distinct defects

One row per distinct defect, merged across the axes that observed it.
Severity follows the pre-registration scale (1 highest). Class is "none"
unless a live class demonstrably fits.

| ID | axes | cells | claim | upstream | subject | verdict | sev |
|---|---|---|---|---|---|---|---|
| D-01 | D, H, B (BP-10) | all | **The subject wipes the whole user profile on the 8th launch.** The pin decrements `user.conf`'s open counter in `pinyin_fini`, so a steady user stays at 0/1. The subject increments on every open and never decrements, so after 7 clean init→train→save→fini cycles the 8th init finds counter > 6 and deletes all user files. Executed: 10 cycles; subject `c8: WIPE`, 7 learned phrases → 0 on all cells; pin keeps 9 phrases. libzhuyin behaves the same | `pinyin.cpp:185-186`, `:1194-1198`; `storage/table_info.cpp:409-425` | `oxpinyin-capi/src/context.rs:119-129`; `oxpinyin-user/src/persistence.rs:223-266` | DIVERGENT-UNREGISTERED | 1 |
| D-02 | B (BP-01), E (A-1), K (row 38) | all | `pinyin_train` ignores `index`. `train(1)`/`train(2)` write `train(0)`'s deltas; the pin trains the index-th n-best row. `train(len)`/`train(255)` abort on the pin and return true on the subject | `pinyin.cpp:2670-2691` | `oxpinyin-capi/src/candidates.rs:550` | DIVERGENT-UNREGISTERED | 1 |
| D-03 | F, B (BP-04), C, E | all | **No class-(c) row meets (c).** 74 executed pin abort sites (73 SIGABRT, 1 SIGSEGV) are answered silently by the subject: false, true, data, or a store write, with no log. This covers register rows 4, 5a, 5c, 5d, 6, 10, 14, 19, 21 and 22 | F ledger (357 sites) | only log site: `context.rs:21` | DIVERGENT-UNREGISTERED | 1 |
| D-04 | B (BP-03) | all | NULL pointer arguments to 68 exports: the pin SIGSEGVs, the subject returns false/0/NULL silently. There is no register row; (b) was not argued | `pinyin.cpp` (unguarded derefs) | C ABI null guards | DIVERGENT-UNREGISTERED | 1 |
| D-05 | C (union), K | all | Register row 33 (REVERT TARGET) is still present: after a whole-row NBEST choose + train the subject writes the user bigram and predicts `你`; the pin predicts nothing. Row 20 attributes the same line to the (a) residual, which is falsified | `pinyin.cpp:2515-2520`, `phonetic_lookup.h:866` | `constraint.rs`, `selection.rs` | DIVERGENT-UNREGISTERED | 1 |
| D-06 | B (BP-12, BP-12b) | all | The subject crashes where the pin does not: `pinyin_alloc_instance` after `pinyin_fini` → SIGSEGV. Inversely, guessing on an instance that outlives its context crashes the pin and answers true on the subject | `pinyin.cpp` lifecycle | `oxpinyin-capi/src/context.rs` | DIVERGENT-UNREGISTERED | 1 |
| D-07 | B (BP-42) | kc | The subject writes `user_pinyin_index.bin`/`user_phrase_index.bin` as native KC databases, and the pin cannot `load_snapshot` them. A phrase imported on the subject is lost to the pin. This contradicts the policy's same-backend interop claim | `load_snapshot` path | kc user store | DIVERGENT-UNREGISTERED | 1 |
| D-08 | D (round-trip) | all | The same user dir read by pin and subject gives different learned state: unigram of 你 53853 vs 52887, and the bigram row sets differ. The attribution was not completed | — | — | DIVERGENT-UNREGISTERED (attribution owed) | 2 |
| D-09 | H (fork) | kc | Parent init, forked child trains and saves: the subject run never exits (killed by the 300 s timeout, exit 137). The pin exits 0 | — | kc store after fork | DIVERGENT-UNREGISTERED | 1 |
| D-10 | B (BP-02) | all | The default option word after `pinyin_init` is `USE_TONE` on the pin and `PINYIN_INCOMPLETE` on the subject | `pinyin.cpp:329` | `oxpinyin-facade/src/lib.rs:56` | DIVERGENT-UNREGISTERED | 2 |
| D-11 | B (BP-07) | all | `pinyin_begin_get_phrases(ctx, 1..4)` exports every system row on the pin (95698/21234/28255/1051) and nothing on the subject | `pinyin.cpp:698-769` | `iterators.rs` | DIVERGENT-UNREGISTERED | 2 |
| D-12 | B (BP-08), E (SIGN-1, IMP-1) | all | Import semantics differ. Negative counts, toned pinyin (`ce4'shi4`) and libraries 1/255 are accepted by the pin and refused by the subject. Count 0 exports as −1 on the pin | `pinyin.cpp` import | `iterators.rs` | DIVERGENT-UNREGISTERED | 2 |
| D-13 | G | tkrzw (fixture) | Trellis selection logic is not class (a). The node-store keep rule (Q12, `phonetic_lookup_heap.h:56-81`, a max-heap that evicts the best) moves 62 of 504 corpus inputs, and a counterfactual oracle using the subject's rule reproduces the subject on 42 of them. The comparator clause (Q09, `phonetic_lookup.h:75-88`, dead "longer" clause) moves 7. Part of the residual frozen under row 11 (a) is selection-logic divergence | `phonetic_lookup.h`, `phonetic_lookup_heap.h` | `oxpinyin-engine/src/nbest.rs:194-197,241-267` | DIVERGENT-BROADER (row 11) | 2 |
| D-14 | I | all | ibus-libpinyin 1.16.5 at the pin, with libpinyin swapped for the subject (LD_LIBRARY_PATH and relink): the lookup table content and stderr differ (71 lines, deterministic) | — | — | DIVERGENT-UNREGISTERED | 2 |
| D-15 | A, I, K | all | `libpinyin.pc`/`libzhuyin.pc` say `Version: 2.11.91` and `includedir …/libpinyin-2.11.91`; the pin says 2.11.92. `pkg-config --atleast-version=2.11.92` fails on the subject. `libdir` is hard-coded `/usr/lib` (the pin uses `${exec_prefix}/lib`), which breaks `--define-variable=prefix` relocation | `configure.ac:7-9`, `libpinyin.pc.in` | `oxpinyin-capi/Cargo.toml:145-162` | DIVERGENT-UNREGISTERED | 2 |
| D-16 | H | all | Two contexts on one user dir: independent on the pin (B does not see A's import; counter 2). Shared on the subject through the process registry (B sees A's import; counter 1; save results flip) | — | `oxpinyin-user/src/registry.rs:107-108` | DIVERGENT-UNREGISTERED | 2 |
| D-17 | E (G-LOC-1) | all | `pinyin_init` leaves the process `LC_NUMERIC` at "C" on the pin (`table_info.cpp` setlocale); the subject does not touch the locale | `storage/table_info.cpp` | — | DIVERGENT-UNREGISTERED | 2 |
| D-18 | B (16, 17), E (INT-1..4) | all | Integer edge semantics: wrap vs saturate (`remember_user_input`, bigram totals), `add_unigram_frequency(G_MAXUINT)`, input length cap (pin 32767 via gint16, subject 4096), and overflow drops | various | various | DIVERGENT-UNREGISTERED | 2 |
| D-19 | B (15), C, E (ORD-1/2, OFF-2) | all | Bigram export surface: last-row `get_next` returns true (register row 36 open); DB-walk export order; the pin skips the last key; the pin attributes `sentence_start` successors to the next predecessor | `pinyin.cpp:842-911` | `iterators.rs:371-411` | DIVERGENT-UNREGISTERED | 2 |
| D-20 | B (14), C, E (MSB-3.2) | all | Register row 1 (b) says "repeated export cycle". The pin SIGSEGVs on the **first** export cycle after a train (bdb deterministic, 4/4) | `pinyin.cpp:862` | — | DIVERGENT-BROADER (row 1) | 1 |
| D-21 | B (41) | bdb | The pin creates the bdb user DB files mode 0600; the subject creates them 0644 under umask 022 | libdb default | store create | DIVERGENT-UNREGISTERED | 3 |
| D-22 | H | all | The subject reads `TMPDIR` in `pinyin_init` and leaves a temp entry; the pin reads neither | — | — | DIVERGENT-UNREGISTERED | 3 |
| D-23 | B (11), C | all | Diagnostics differ: the pin prints `open <dir>/user.conf failed.`; the subject prints "non-conforming user profile wiped" on a fresh dir and a glib warning on init failure. Save rename messages are absent on the subject | — | `persistence.rs` | DIVERGENT-UNREGISTERED | 3 |
| D-24 | A (ck-runtime) | all | Raw `ChewingKey` bytes and returns from single-key parses across schemes and options: 73 both-true-bytes-differ, 2830 pin-false/subject-true, 1991 pin-true/subject-false, and 337 chewing keys accepted only by the subject. The attribution was not completed | `chewing_key.h` | parser | DIVERGENT-UNREGISTERED (attribution owed) | 3 |
| D-25 | B (06, 18–22, 26, 32–39), E (D-1..3) | all | Assorted return-value and out-param contract differences on error or edge paths. Examples: `get_sentence` before a guess (pin false, subject true + raw input); aux text after full-pinyin parse; `guess_sentence` on no keys; out-param write-on-failure; function-static key slots shared across instances on the pin | per BP row | per BP row | DIVERGENT-UNREGISTERED | 2–3 |
| D-26 | C (double3) | all | ZIGUANG `zhrgguor` candidate[2] NBEST: 宗人光卓然 on the pin vs 总人光卓然 on the subject. Attributed to row 11 by the C agent, but D-13 shows row 11's scope is contested | — | — | DIVERGENT-REGISTERED (row 11), under D-13 | 2 |

The per-probe evidence for B (757 probe rows, 79 per-export rows) and the
per-site F ledger (357 rows) exist only in the ephemeral scratch area (see
section 8).

## 5. Per-axis narrative (non-MATCH only)

### A

- **Static comparisons MATCH on every cell** (a pure set/byte basis):
  - SONAME, the symlink chain and the version nodes;
  - the 79/52 versioned symbols;
  - the five headers, byte for byte.
- **Header layout MATCH**, from the generated probe: 185 lines, and the
  oracle-vs-subject diff is empty on all cells.
- **NEEDED differs, by construction:** the subject has no libstdc++/libm and
  adds ld-linux. This is recorded, not a finding.
- abidiff reports "75 Changed" functions on the type level (C++ vs Rust
  DWARF), which the pre-registration says cannot be a parity statement.
- D-15 and D-24 are the findings.

### C

- Axis C's drivers are **not uniformly non-vacuous.** Seven identical
  drivers (key-surface, dict-surface, phrase-surface, pred-order, predict,
  punct, import) fail to detect C-m2. Their IDENTICAL results cannot count
  as parity evidence until an injected divergence on their own surface is
  shown to reach them.
- The **zhuyin-diff corpus never exercises key `1`.**

### G

- The §12 gate reproduces 491/396/390 of 496 on tkrzw and kc, and passes its
  equality assertion on bdb.
- The gate measures the Rust engine against a **committed oracle fixture**.
  It does not test the bdb or kc oracle's own behaviour, and its mutation
  check was never run.
- The counterfactual experiment (D-13) is the substantive result:
  - two selection-logic differences are visible at the C ABI on real data;
  - neither is a transcendental accumulation.

### H

- Findings D-09, D-16 and D-22.
- Upstream has no synchronisation primitives. The subject has two process
  registries and a `Once`. "Safer" sharing appears as D-16.

### I

- The header compile matrix fails **identically** on both sides: `bool` is
  undeclared in C modes, because the headers are identical. That is MATCH on
  a static basis.

## 6. Round-1 verification

**Not done.** The round-1 report (`docs/findings/bug-for-bug-audit-2026-09-23.md`
on PR #516) was not opened in this round. No row of round 1 is CONFIRMED,
REFUTED or UNSUPPORTED here.

## 7. Register reconciliation (axis K, static, executed where noted)

### 7.1 Unregistered divergences

D-01 to D-12, D-14 to D-19 and D-21 to D-25 above. In addition, the
REVERT TARGET rows still present in code (a revert target that is still
present counts as unregistered):

| row | status | basis |
|---|---|---|
| 33 | present | executed (D-05) |
| 34 | present | source only |
| 36 | present | executed (D-19) |
| 37 | present | source only; K says broader |

### 7.2 Broader than registered

| row | how it is broader |
|---|---|
| 1 (b) | the pin crashes on the first export cycle, not only a repeated one (D-20) |
| 11 (a) | selection-logic components are not (a) (D-13) |
| 20 (a) | falsified: row 33's mechanism, and it occurs on all cells, not KC only |
| 38 | the train gate is broader than the pin |

### 7.3 Stale entries and register integrity

1. **The totals line does not match the table.**
   - The table's class column counts REVERT TARGET 6 and CLOSED 17; the
     totals line says 5 and 18.
   - Row 17's cell still says REVERT TARGET, while the code closes it
     (`8ec75085`).
   - Row 35 is closed in code (`7c9a6923`) but the table and totals still
     count it open.
2. **All ten class-(c) rows** (4, 5a, 5c, 5d, 6, 10, 14, 19, 21, 22) fail the
   policy's own log obligation (D-03). Rows 4 and 6 also fail the false/Err
   half.
3. **Cites are stale:**
   - rows 5d, 14 and 19 have stale line cites;
   - row 14's runner comment cites `:2175`, but the abort fires at
     `pinyin.cpp:3092`;
   - row 22 names the wrong symbol: `pinyin_phrase_segment("")` returns
     true; the crash is `pinyin_lookup_tokens("")`.
4. **The policy lead-in is stale.** `compatibility-policy.md:107-110` says
   "one of four classes … There are four, and no others", but (d) is
   retired.
5. **The tkrzw standing divergence is not in the register.** It is recorded
   only in `tkrzw-langc-exception-classification.md`; neither the policy
   table nor `upstream-divergences.md` carries it.
6. **Upstream issues are misattributed.** The catalogue cites them as
   libpinyin issues #566/#542/#518; they are ibus-libpinyin issues.
7. **Cross-register gaps.**
   - `upstream-divergences.md` has 32 entries.
   - Policy rows 17 and 32–38 have no entry there; row 30 does.

### 7.4 Stale behaviour-asserting prose

The static sweep found 65 prose rows, 43 of them marked STALE in some form.
The candidates the mandate named are all confirmed:

| location | stale claim |
|---|---|
| `crates/oxpinyin-capi/Cargo.toml:8` description | "C ABI subset … for the borrowed frontend". The crate exports all 79 symbols with a pin-identical `.ver`, and (d) is retired |
| the `Cargo.toml` pkg-config metadata | 2.11.91 (D-15) |
| `docs/findings/installed-naming.md` | version, KC default, LMDB/redb, "glib-free pinyin.h" |
| `tools/bisection/Dockerfile.perf-matrix:3-7,116` | cell D is labelled "Kyoto Cabinet (default)" |
| several findings docs | still call tkrzw the default |

## 8. Coverage ledgers and evidence location

### Export ledger (J)

- Command:
  `nm -D --defined-only /opt/oracle/<cell>/lib/lib{pinyin,zhuyin}.so.15 | awk '$2=="T"{print $3}'`
- Result: 79 + 52 = 131 on every cell; tkrzw == bdb == kc.
- **The internal (transitive) ledger was not produced.**

### F site ledger

- 357 lines from the pre-registered grep, 70 `check_result` sites;
  87 caller- or data-reachable sites executed.
- Per-site verdicts:

| verdict | sites |
|---|---|
| DIVERGENT-UNREGISTERED | 74 |
| trigger refuted | 8 |
| not executed | 5 |

### Evidence

The raw evidence stays in the auditor host's scratch area
`/home/sheng/audit-r2-scratch/{results,harness,mutations,env}`. It holds:
- the harness sources (contract battery, open-counter cycle, layout probe,
  env interposer, counterfactual builds);
- the mutation patches and `mutations/INDEX.md`;
- the container recipe;
- every run log.

**That area is ephemeral and is not retained with this document.** Any
figure above must be regenerated from the recipes before it is relied on.
Harnesses worth keeping are proposed for adoption in section 10.

## 9. Provenance of the "Q1 ruling"

1. **Where "Q1" appears.**
   - It occurs exactly once in history: a `ci.yml` comment added by
     `ef7f2b41` ("the workspace default (Berkeley DB, the Q1 ruling)").
   - `cf658a32` deleted that comment, so no file at `18d78208` mentions Q1.
2. **What `ef7f2b41` itself says.** The commit message cites no ruling.
   - It argues the analogy that a bare `./configure` selects BerkeleyDB.
   - That fact is true at the pin: `configure.ac:94 DBM="BerkeleyDB"`.
   - It carries `Assisted-by: ZCode:GLM-5.3`.
3. **PR #502** (merged 2026-09-23T01:52:46Z) has an agent-authored body.
   - Its "as ruled" and "Ruling 1" concern merge order and fixture checks.
   - No comment or review records a default-backend decision.
4. **Sibling PRs.** #497 shows that a numbered Phase-1 question list (Q1–Q4)
   existed, but neither the questions nor their answers appear in any PR
   body, comment, commit or tree file. #497's own ruling 1 calls tkrzw "the
   default, shipping backend".
5. **What records a human decision.** Nothing on record is a written human
   decision for the BDB default. The only maintainer act is the merge of
   #502; `mergedBy` was not queried.
6. **This is not decided here, and no default is changed.** The default
   build (bdb) is also the cell whose parity is least established. The §12
   gate uses a tkrzw-derived fixture, and several same-data-dir drivers are
   vacuous (section 5, C).

## 10. Open gaps and proposed follow-ups

| gap | what blocked it | next step |
|---|---|---|
| L on all cells | never started | run the pre-registered workloads on a quiet host, one cell at a time |
| G mutations on all cells | the mutant gate builds finished; the runs were never started | build the gate test from `mut-src` (`5eb5c51`) and run it with `OXPINYIN_AUDIT_MUT=G-m1` / `G-m2` / unset |
| C option sweep, encoding battery, zhuyin layouts | killed at stop | rerun `harness/C-options/` sweep v3 per cell; add C-m1 |
| B libzhuyin | no mutation validation or ledger | adopt the pinyin battery's analysis; C-m3 plus an LD_PRELOAD shim as the non-vacuity check |
| J internal ledger | not produced | clang `-emit-llvm` call graph from the 131 exports |
| C non-detecting drivers (7) | C-m2 does not reach their surfaces | design a per-driver mutation on its own surface |
| D-08, D-24 attribution | salvaged, not analysed | diff the unigram/bigram dumps per phrase; classify the key-byte deltas by scheme and option |
| Round-1 verification (Step 4) | not started | read #516's report and grade every row against sections 3–7 |
| GitHub tracking (Step 5) | not started; token lacks the `project` scope | `gh auth refresh -s project`; file one issue per D-row, register item and gap |
| Re-measure on the current main | main moved to `34a66bc9` (`expand_keys` change in core) | re-run B, C and G on the new tip |

Proposed for adoption as repo tools (each would need its own reviewed PR):
- the fork-per-probe contract battery (`contract-battery.c`);
- the open-counter cycle driver;
- the getenv interposer;
- the header layout probe generator;
- `zhuyin-diff-ext`.
