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
- **Step 4 (round-1 verification) is done** (section 6).
- **Step 5 (GitHub tracking)** was done without a project board, which the maintainer removed from Step 5. Tracking issue #573 has the 50 audit issues (#523–#572) as sub-issues, under milestone *Bug-for-bug audit r2*, with `bug-for-bug`/`verdict:*`/`axis:*`/`severity:*`/`backend:*` labels.
- **Measured subject:** `origin/main` at `18d782089bd1`. Main has since moved
  to `34a66bc915c9`, adding 00466d50 (`expand_keys` early stop in
  `oxpinyin-core/src/scoring.rs`) and 34a66bc9 (a fuzz corpus seed). Nothing
  in the primary ledger was re-measured on the new tip; selected severity-1/2
  findings were re-run in section 11, and item 4's severity-2 findings were
  re-run in section 15.
- **Findings were not adversarially re-verified.** Subagent enumeration was
  spot-checked (section 1.3). Salvaged verdicts come from the raw run
  artifacts, not from a finished agent's conclusion.

| axis | tkrzw | bdb | kc | basis |
|---|---|---|---|---|
| A ABI surface | static MATCH (exc. .pc); layout MATCH | same | same | A-m1 and A-m3 detected and reverted; abidiff type section NOT-ESTABLISHED by pre-registration |
| B libpinyin contract | DIVERGENT (40 findings) | same | same | B-m1, B-m2, F-m2 detected; gated revert identical |
| B libzhuyin contract | DIVERGENT: all 52 exports (Z-1..Z-3 plus twins, §14) | same | same | C-m3 and the Bz-shim-m1 shim detected and reverted on every cell |
| C drivers | DIVERGENT (6 kinds) | same, +1 broader | same | C-m2 detected by 6 drivers; the other 7 were validated by their own DRV-* mutations (§13.2), so their IDENTICAL results are MATCH |
| C options/encoding/zhuyin layouts | DIVERGENT (§15) | same | same | 539 option words, 1,043 encoding cases and 1,236 layout corpus lines; C-m1/C-m3 detected and reverted. Payload equality is conditional on D-23 diagnostics |
| D persisted state | DIVERGENT (open counter, cross-read) | same | same, +kc interop | D-m1 (round-trip) and D-m3 (cycle) detected; revert identical |
| E defect preservation | 37 DIVERGENT, 14 NE | same | same | E-m1 detected on all cells, reverted |
| F errors/diagnostics | 74 sites DIVERGENT | same | same | F-m1, F-m2 detected on every cell; bdb determinism holds under normalisation 2 (§13.3) |
| G numerics | **NOT-ESTABLISHED** (G-m2 detected, **G-m1 not**: blind gate, §13.1) | **NOT-ESTABLISHED** (same) | **NOT-ESTABLISHED** (same) | no G row may be MATCH. The §12 gate mutation runs were done: G-m2 was detected and reverted on all cells; G-m1 was reached and changed four C-ABI outputs outside the fixture, but the gate missed it. D-13 stands on counterfactual and source evidence |
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
| pre-registration | first committed and pushed as `dbee7c04dab055f0d0777e846b5dff55219f2503` at 2026-09-23T10:54:33Z. The required rebase onto the landing tip rewrote it to `fad9ad47`. The author date is unchanged, and the file blob `d47e9b50aae0` is identical in both commits (`git rev-parse <sha>:<path>`) |
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

6. **Recovered verdicts** (B-libpinyin, D, F, G, H, I, static A) were each
   checked by the orchestrator against source on 2026-09-24/25 (UTC), before
   the verdict was kept. A recovered verdict with no source spot-check is
   downgraded to NOT-ESTABLISHED.

   | row | spot-check | result |
   |---|---|---|
   | D-01 | pin `pinyin.cpp:185-186,1194-1198`; subject `context.rs:119-129`, `persistence.rs:223-266` | CONFIRMED |
   | D-02 | pin `pinyin.cpp:2670-2691` (`assert(index < results.size())`, trains `results[index]`); subject `candidates.rs:550` (`_index` unused) | CONFIRMED |
   | D-03 | the only glib log calls are `context.rs:21` and zhuyin `context.rs:44`; the extra stderr writers (item 1) are not at abort-site counterparts | CONFIRMED |
   | D-04 | pin `pinyin.cpp:2845-2848` dereferences `instance` unguarded; subject `candidates.rs:22` returns false on NULL | CONFIRMED |
   | D-06 | subject `instance.rs:24` dereferences the (freed) context in `pinyin_alloc_instance` | CONFIRMED |
   | D-07 | pin KC `chewing_large_table2_kyotodb.cpp:100-106` loads the user index with `load_snapshot` into an in-memory tree; subject `oxpinyin-data/src/user_files.rs:11-17` writes the native tree DBM and claims byte compatibility | CONFIRMED |
   | D-08 | round-trip dumps differ, but no per-phrase attribution | **downgraded to NOT-ESTABLISHED** |
   | D-09 | harness `hsess.c:468-483`: the child finished (status 0), the parent was killed at the 300 s timeout; stdout buffering hides where; the run was at load ~68 | **downgraded to NOT-ESTABLISHED** (hang vs slowness not separated) |
   | D-10 | pin `pinyin.cpp:329` `m_options = USE_TONE`; subject `oxpinyin-facade/src/lib.rs:56` `PINYIN_INCOMPLETE`, used at `state.rs:63` | CONFIRMED |
   | D-11 | subject `oxpinyin-facade/src/export_rows.rs:33-35` returns empty for any library but USER/NETWORK; pin `pinyin.cpp:662-674` walks any sub-index | CONFIRMED |
   | D-12 | subject `iterators.rs:128-133` refuses counts below −1; pin `pinyin.cpp:630` parses the import with `USE_TONE` | CONFIRMED |
   | D-13 | pin `phonetic_lookup_heap.h:25-29` (`comp` = `less_than`, so `std::push_heap` builds a max-heap), and `:69-77` evicts `m_elements[0]` = best; subject `oxpinyin-engine/src/nbest.rs:254-265` evicts its worst | CONFIRMED |
   | D-14 | harness `harness/I-dropin/batch.sh:14-17`: same oracle-built ibus binary, only the library dir swapped; relink run identical | CONFIRMED (observational) |
   | D-15 | static `.pc` diff (section 5, A) | CONFIRMED |
   | D-16 | subject `oxpinyin-user/src/registry.rs:107-108`, process-global `OPEN_STORES` keyed by path | CONFIRMED |
   | D-18 | pin `phrase_index.cpp:168-170` (`ERROR_INTEGER_OVERFLOW` → false); subject `dict.rs:364-377` adds a u64 delta unchecked | CONFIRMED (the add-frequency part; the other parts rest on axis E's completed ledger) |
   | D-19 | pin `pinyin.cpp:896-910` returns `has_next_phrase`; subject `iterators.rs:407-408` returns true | CONFIRMED |
   | D-21 | pin BDB user tables are created 0600 (`chewing_large_table2_bdb.cpp:149`, `phrase_large_table3_bdb.cpp:164`, `ngram_bdb.cpp:55`); subject `oxpinyin-store/src/bdb/ffi.rs:298` uses 0644 | CONFIRMED |
   | D-22 | every `std::env::temp_dir` use in the shipped crates is inside `#[cfg(test)]`, so the observed `TMPDIR` read has no located source | **downgraded to NOT-ESTABLISHED** |
   | D-23 | subject `persistence.rs` "non-conforming user profile wiped" path; pin `open … failed.` from `table_info` | CONFIRMED |
   | D-24 | key-byte deltas not classified | **downgraded to NOT-ESTABLISHED** |

   D-05, D-17, D-20, D-26 and the E-derived parts of D-02/D-12/D-18/D-19 rest
   on axes that returned complete ledgers (C, E).

## 2. Pre-registration and falsifiers

The pre-registration was committed as `dbee7c04` (rebased to `fad9ad47`, same blob) before any measurement.

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
4. **Execution stopped early at the salvage point,** by maintainer instruction, before
   G-m1/G-m2, C-m1, L-m1/L-m2, the J internal ledger, the full option sweep,
   B-zhuyin validation, and Steps 4–5. Sections 13–15 record later work.

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
| G | all | G-m1, G-m2 through the §12 gate | G-m1 not detected; G-m2 detected | both unset runs match pristine | G remains NOT-ESTABLISHED (§13.1) |
| G | all | G-m1 at the C-ABI fixture surface | **no** (0 lines) | unset identical | the surface instrument is blind to a uniform +1e-3 nat shift |
| G | all | G-m2 at the C-ABI fixture surface | yes (4 lines) | unset identical | |
| C-options | all | C-m1 | yes: 34/104 single words and 29/435 pair words per cell | yes: unset matches pristine exactly | §15 |
| C-encoding | all | C-m1, C-m3 | yes: 2 and 3 cases respectively per cell | yes: unset matches pristine exactly | §15 |
| C-layouts | all | C-m3 | yes: table and all three corpus words per cell | yes: unset matches pristine exactly | §15 |
| L | all | L-m1, L-m2 | **not run** | — | |

## 4. Ledger: distinct defects

One row per distinct defect, merged across the axes that observed it.
Severity follows the pre-registration scale (1 highest). Class is "none"
unless a live class demonstrably fits.

| ID | axes | cells | claim | upstream | subject | verdict | sev | issue |
|---|---|---|---|---|---|---|---|---|
| D-01 | D, H, B (BP-10) | all | **The subject wipes the whole user profile on the 8th launch.** The pin decrements `user.conf`'s open counter in `pinyin_fini`, so a steady user stays at 0/1. The subject increments on every open and never decrements, so after 7 clean init→train→save→fini cycles the 8th init finds counter > 6 and deletes all user files. Executed: 10 cycles; subject `c8: WIPE`, 7 learned phrases → 0 on all cells; pin keeps 9 phrases. libzhuyin behaves the same | `pinyin.cpp:185-186`, `:1194-1198`; `storage/table_info.cpp:409-425` | `oxpinyin-capi/src/context.rs:119-129`; `oxpinyin-user/src/persistence.rs:223-266` | DIVERGENT-UNREGISTERED | 1 | #523 |
| D-02 | B (BP-01), E (A-1), K (row 38) | all | `pinyin_train` ignores `index`. `train(1)`/`train(2)` write `train(0)`'s deltas; the pin trains the index-th n-best row. `train(len)`/`train(255)` abort on the pin and return true on the subject | `pinyin.cpp:2670-2691` | `oxpinyin-capi/src/candidates.rs:550` | DIVERGENT-UNREGISTERED | 1 | #524 |
| D-03 | F, B (BP-04), C, E | all | **No class-(c) row meets (c).** 74 executed pin abort sites (73 SIGABRT, 1 SIGSEGV) are answered silently by the subject: false, true, data, or a store write, with no log. This covers register rows 4, 5a, 5c, 5d, 6, 10, 14, 19, 21 and 22 | F ledger (357 sites) | only log site: `context.rs:21` | DIVERGENT-UNREGISTERED | 1 | #525 |
| D-04 | B (BP-03) | all | NULL pointer arguments to 68 exports: the pin SIGSEGVs, the subject returns false/0/NULL silently. There is no register row; (b) was not argued | `pinyin.cpp` (unguarded derefs) | C ABI null guards | DIVERGENT-UNREGISTERED | 1 | #526 |
| D-05 | C (union), K | all | Register row 33 (REVERT TARGET) is still present: after a whole-row NBEST choose + train the subject writes the user bigram and predicts `你`; the pin predicts nothing. Row 20 attributes the same line to the (a) residual, which is falsified | `pinyin.cpp:2515-2520`, `phonetic_lookup.h:866` | `constraint.rs`, `selection.rs` | DIVERGENT-UNREGISTERED | 1 | #527 |
| D-06 | B (BP-12, BP-12b) | all | The subject crashes where the pin does not: `pinyin_alloc_instance` after `pinyin_fini` → SIGSEGV. Inversely, guessing on an instance that outlives its context crashes the pin and answers true on the subject | `pinyin.cpp` lifecycle | `oxpinyin-capi/src/context.rs` | DIVERGENT-UNREGISTERED | 1 | #528 |
| D-07 | B (BP-42) | kc | The subject writes `user_pinyin_index.bin`/`user_phrase_index.bin` as native KC databases, and the pin cannot `load_snapshot` them. A phrase imported on the subject is lost to the pin. This contradicts the policy's same-backend interop claim | `load_snapshot` path | kc user store | DIVERGENT-UNREGISTERED | 1 | #529 |
| D-08 | D (round-trip) | all | The same user dir read by pin and subject gives different learned state: unigram of 你 53853 vs 52887, and the bigram row sets differ. The attribution was not completed | — | — | NOT-ESTABLISHED (downgraded, section 1.3 item 6) | 2 | #543 |
| D-09 | H (fork) | kc | Parent init, forked child trains and saves: the subject run never exits (killed by the 300 s timeout, exit 137). The pin exits 0 | — | kc store after fork | NOT-ESTABLISHED (downgraded, section 1.3 item 6) | 1 | #531 |
| D-10 | B (BP-02) | all | The default option word after `pinyin_init` is `USE_TONE` on the pin and `PINYIN_INCOMPLETE` on the subject | `pinyin.cpp:329` | `oxpinyin-facade/src/lib.rs:56` | DIVERGENT-UNREGISTERED | 2 | #532 |
| D-11 | B (BP-07) | all | `pinyin_begin_get_phrases(ctx, 1..4)` exports every system row on the pin (95698/21234/28255/1051) and nothing on the subject | `pinyin.cpp:698-769` | `iterators.rs` | DIVERGENT-UNREGISTERED | 2 | #533 |
| D-12 | B (BP-08), E (SIGN-1, IMP-1) | all | Import semantics differ. Negative counts, toned pinyin (`ce4'shi4`) and libraries 1/255 are accepted by the pin and refused by the subject. Count 0 exports as −1 on the pin | `pinyin.cpp` import | `iterators.rs` | DIVERGENT-UNREGISTERED | 2 | #534 |
| D-13 | G | tkrzw (fixture) | Trellis selection logic is not class (a). The node-store keep rule (Q12, `phonetic_lookup_heap.h:56-81`, a max-heap that evicts the best) moves 62 of 504 corpus inputs, and a counterfactual oracle using the subject's rule reproduces the subject on 42 of them. The comparator clause (Q09, `phonetic_lookup.h:75-88`, dead "longer" clause) moves 7. Part of the residual frozen under row 11 (a) is selection-logic divergence | `phonetic_lookup.h`, `phonetic_lookup_heap.h` | `oxpinyin-engine/src/nbest.rs:194-197,241-267` | DIVERGENT-BROADER (row 11) | 2 | #535 |
| D-14 | I | all | ibus-libpinyin 1.16.5 at the pin, with libpinyin swapped for the subject (LD_LIBRARY_PATH and relink): the lookup table content and stderr differ (71 lines, deterministic) | — | — | DIVERGENT-UNREGISTERED | 2 | #536 |
| D-15 | A, I, K | all | `libpinyin.pc`/`libzhuyin.pc` say `Version: 2.11.91` and `includedir …/libpinyin-2.11.91`; the pin says 2.11.92. `pkg-config --atleast-version=2.11.92` fails on the subject. `libdir` is hard-coded `/usr/lib` (the pin uses `${exec_prefix}/lib`), which breaks `--define-variable=prefix` relocation | `configure.ac:7-9`, `libpinyin.pc.in` | `oxpinyin-capi/Cargo.toml:145-162` | DIVERGENT-UNREGISTERED | 2 | #537 |
| D-16 | H | all | Two contexts on one user dir: independent on the pin (B does not see A's import; counter 2). Shared on the subject through the process registry (B sees A's import; counter 1; save results flip) | — | `oxpinyin-user/src/registry.rs:107-108` | DIVERGENT-UNREGISTERED | 2 | #538 |
| D-17 | E (G-LOC-1) | all | `pinyin_init` leaves the process `LC_NUMERIC` at "C" on the pin (`table_info.cpp` setlocale); the subject does not touch the locale | `storage/table_info.cpp` | — | DIVERGENT-UNREGISTERED | 2 | #539 |
| D-18 | B (16, 17), E (INT-1..4) | all | Integer edge semantics: wrap vs saturate (`remember_user_input`, bigram totals), `add_unigram_frequency(G_MAXUINT)`, input length cap (pin 32767 via gint16, subject 4096), and overflow drops | various | various | DIVERGENT-UNREGISTERED | 2 | #540 |
| D-19 | B (15), C, E (ORD-1/2, OFF-2) | all | Bigram export surface: last-row `get_next` returns true (register row 36 open); DB-walk export order; the pin skips the last key; the pin attributes `sentence_start` successors to the next predecessor | `pinyin.cpp:842-911` | `iterators.rs:371-411` | DIVERGENT-UNREGISTERED | 2 | #541 |
| D-20 | B (14), C, E (MSB-3.2) | all | Register row 1 (b) says "repeated export cycle". The pin SIGSEGVs on the **first** export cycle after a train (bdb deterministic, 4/4) | `pinyin.cpp:862` | — | DIVERGENT-BROADER (row 1) | 1 | #530 |
| D-21 | B (41) | bdb | The pin creates the bdb user DB files mode 0600; the subject creates them 0644 under umask 022 | libdb default | store create | DIVERGENT-UNREGISTERED | 3 | #544 |
| D-22 | H | all | The subject reads `TMPDIR` in `pinyin_init` and leaves a temp entry; the pin reads neither | — | — | NOT-ESTABLISHED (downgraded, section 1.3 item 6) | 3 | #546 |
| D-23 | B (11), C | all | Diagnostics differ: the pin prints `open <dir>/user.conf failed.`; the subject prints "non-conforming user profile wiped" on a fresh dir and a glib warning on init failure. Save rename messages are absent on the subject | — | `persistence.rs` | DIVERGENT-UNREGISTERED | 3 | #545 |
| D-24 | A (ck-runtime) | all | Raw `ChewingKey` bytes and returns from single-key parses across schemes and options: 73 both-true-bytes-differ, 2830 pin-false/subject-true, 1991 pin-true/subject-false, and 337 chewing keys accepted only by the subject. The attribution was not completed | `chewing_key.h` | parser | NOT-ESTABLISHED (downgraded, section 1.3 item 6) | 3 | #547 |
| D-25 | B (06, 18–22, 26, 32–39), E (D-1..3) | all | Assorted return-value and out-param contract differences on error or edge paths. Examples: `get_sentence` before a guess (pin false, subject true + raw input); aux text after full-pinyin parse; `guess_sentence` on no keys; out-param write-on-failure; function-static key slots shared across instances on the pin | per BP row | per BP row | DIVERGENT-UNREGISTERED | 2–3 | #542 |
| D-26 | C (double3) | all | ZIGUANG `zhrgguor` candidate[2] NBEST: 宗人光卓然 on the pin vs 总人光卓然 on the subject. Attributed to row 11 by the C agent, but D-13 shows row 11's scope is contested | — | — | DIVERGENT-REGISTERED (row 11), under D-13 | 2 | #535 (scope) |
| C-1 | C options | all | Secondary-zhuyin `tsz` consumes and exposes incomplete key `c` on both sides, but with option `0x00000002` the pin returns 718 candidates and sentence 从 while the subject returns zero candidates/no sentence. Subject's `walk` drops `Incomplete` unless `PINYIN_INCOMPLETE` is set | `storage/zhuyin_parser2.cpp:48-55`; `pinyin.cpp:1590-1605` | `oxpinyin-engine/src/session/lookup.rs:1030-1051` | DIVERGENT-UNREGISTERED | 2 | #585 |
| C-2 | C options | all | With `PINYIN_AMB_L_N`, transformed exact keys omit fuzzy alternates: double-pinyin `nihk` yields 499 candidates including 利好 on the pin, 126 without 利好 on the subject; all four bytes are consumed on both sides. The same loss appears for chewing `su3cl3` | `pinyin.cpp:1557-1559,1602-1604` | `oxpinyin-engine/src/session/mod.rs:593-640` | DIVERGENT-UNREGISTERED | 2 | #586 |
| C-3 | C encoding | all | For C bytes `ni\xffhao`, `pinyin_parse_more_full_pinyins` consumes the valid `ni` prefix (2) on the pin, but zero bytes on the subject. Other invalid UTF-8 import cases differ under the same C-string conversion | `pinyin.cpp:1498-1515,615-640` | `oxpinyin-capi/src/ffi.rs:19-28` | DIVERGENT-UNREGISTERED | 3 | #587 |

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

- Axis C's drivers did not all detect C-m2. The seven identical drivers
  (key-surface, dict-surface, phrase-surface, pred-order, predict, punct,
  import) were later validated by their own DRV mutations (§13.2); their
  conditional IDENTICAL results are evidence only for those driver surfaces.
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

## 6. Round-1 verification (Step 4)

The round-1 report is read at `origin/docs/bug-for-bug-audit-2026-09-23`
(`4a9fddb5`): only its §2.2 falsifier table, §3 instrument table and §4 ledger.
Grades use round-2 evidence and cite the round-2 row or issue.

- **CONFIRMED**: round 2 reproduces the claim.
- **REFUTED**: round-2 evidence contradicts the claim.
- **UNSUPPORTED**: round-1 evidence could not establish it.
- **MISSED**: a round-2 finding on an axis where round 1 claimed MATCH.

### 6.1 Ledger rows

| r1 row | round-1 claim (short) | grade | round-2 basis |
|---|---|---|---|
| D1 | `.pc` Version/include-subdir 2.11.91 vs 2.11.92 | CONFIRMED | D-15 (#537) |
| D2 | `pinyin_train` ignores `index` (source only) | CONFIRMED | D-02 (#524), executed |
| D3 | `zhuyin_iterator_add_phrase` substitutes `MAX` for an out-of-range key where the pinyin twin rejects | REFUTED as characterised | §14.3: no observable effect on 18 key-shape probes; the real divergence is the parser (Z-1) |
| D4 | row 33 divergence, verdict R (registered) | REFUTED (verdict) | divergence CONFIRMED (D-05, #527), but an open REVERT TARGET counts as unregistered |
| D5 | row 34, verdict R | REFUTED (verdict) | still present by source (K-policy); an open REVERT TARGET is unregistered |
| D6 | row 36, verdict R | REFUTED (verdict) | divergence CONFIRMED (D-19, #541); unregistered while open |
| D7 | row 37, verdict R | REFUTED (verdict) | still present by source (K-policy); unregistered while open |
| D8 | union-diff `pred 你` is class (a), row 20 | REFUTED | D-05 (#527): the pin writes no bigram after a whole-row NBEST train, so the mechanism is row 33 and occurs on every cell; R-3 (#550) |
| D9 | §12 residual entirely (a) | REFUTED | D-13 (#535): keep-rule and comparator differences move results and are not (a) |
| D10 | class-(b) defects (aux over-read, bigram-export stale buffer, padding) registered | REFUTED (scope) | D-20 (#530): the pin crashes on the first export cycle, not only a repeated one |
| D11 | class-(c) rows answered false/Err **plus logs** | REFUTED | D-03 (#525): 74 executed abort sites, no log line anywhere; R-2 (#549) |
| D12 | tkrzw error collapse registered, "no broader" | UNSUPPORTED | doc-cited only; not executed in either round (memory-exhaustion path); R-4 (#551): absent from both registers |
| D13 | row 35 stale | CONFIRMED | R-1 (#548) |
| D14 | `installed-naming.md` 2.11.91 prose | CONFIRMED | P-1 (#552) |
| D15 | `Dockerfile.perf-matrix` cell D label | CONFIRMED | P-1 (#552) |
| D16 | `upstream-report-drafts.md` has 5 items, not 13 | CONFIRMED | E-exec count (5) |
| N1–N3, N5–N9 | axes declared NOT-ESTABLISHED | CONFIRMED | accurate as declared; round 2 filled N1 (B-pinyin), N3 (E), N5 (H), N6 (I) and N9 (C-drivers zhuyin) |
| N4 | 280 `assert(` + 61 `abort()` = 341 | CONFIRMED | `grep -rn 'assert(\|abort()' src` at the pin = 341 (280 + 61). Round 2's 357 comes from a different pattern (`\b…\s*\(`, which includes comments and disabled code) |
| M1 | version scripts identical | CONFIRMED | static A |
| M2 | installed headers identical ("7 pairs") | CONFIRMED (count UNSUPPORTED) | 5 installed headers, byte-identical on all cells; "7/7" does not match the installed set |
| M3, M4 | shipped ABI identical; abidiff exit 0 is a deep comparison | CONFIRMED for symbol sets and versioning; REFUTED for the "deep" abidiff claim | the release subject has a `.debug_info` section, yet abidiff on it reports nothing, while a DWARF-built subject reports **75 changed** functions on every cell (`results/X-A-dyn/abidiff/<cell>-dwarf-r1-libpinyin.stat.txt`) |
| M5 | 11/12 drivers IDENTICAL, so the surface is identical | UNSUPPORTED | 7 of those drivers do not detect C-m2 (#568–570); the union-diff attribution is REFUTED (D8) |
| M6 | same-backend user dir round-trips | UNSUPPORTED | round 1 had no injected mutation; round-2 D-m1 detection exists, and the round-trip differences are NOT-ESTABLISHED (D-08, #543). kc interop fails (D-07, #529) on a cell round 1 never built |
| M7 | §12 residual stable, MATCH | UNSUPPORTED | no round-1 mutation; item 2 found the gate blind to G-m1 despite changed C-ABI outputs (§13.1) |
| M8 | all 131 exports implemented | CONFIRMED | 131 exported on every cell; the round-2 batteries exercised all 79 pinyin and 52 zhuyin exports (§14) |

**MISSED:** findings on axes where round 1 claimed MATCH.

| finding | round-1 claim it falls under | basis |
|---|---|---|
| D-01 (#523), the profile wipe on the 8th launch | M6, axis D | on the tkrzw cell round 1 ran |
| D-15's `--atleast-version=2.11.92` failure and non-relocatable `libdir` (#537) | M3, axis A | beyond its D1 version string |
| D-10 (#532), D-11 (#533), D-12 (#534), D-19 (#541), D-25 (#542) | M5's surface claim | reachable through the C ABI |
| C-1, C-2, C-3 (§15) | M5's output-equivalence claim | option words and invalid C-string bytes omitted from its drivers; exposed by the mandated axis-C sweep |

### 6.2 Falsifiers

| Φ | grade | basis |
|---|---|---|
| Φ1 export sets | CONFIRMED | static A |
| Φ2 abidiff exit 0 means structurally identical | REFUTED | see M3/M4 |
| Φ3 headers | CONFIRMED (count UNSUPPORTED) | 5, not 7 |
| Φ4 11/12 drivers | UNSUPPORTED | vacuous drivers (#568–570); union attribution refuted |
| Φ5 §12 gate | UNSUPPORTED | item 2 shows G-m1 changes non-fixture outputs while the gate remains green (§13.1) |
| Φ6 round-trip | UNSUPPORTED | see M6; D-01 was MISSED |
| Φ7 no STUB/ABSENT | CONFIRMED | |
| Φ8 "the differential instrument is non-vacuous" | UNSUPPORTED | one driver (key-surface) was validated, then generalised to all 12 |
| Φ9 version fired | CONFIRMED | D-15 |
| Φ10 train index fired | CONFIRMED | D-02 |
| Φ11 revert targets open | CONFIRMED | K-policy |
| Φ12 row 35 stale | CONFIRMED | R-1 |

### 6.3 Round-1 methodology breaches

1. **Pre-registration after measurement.** Self-disclosed in round 1's preamble.
2. **MATCH without an injected mutation.** M3, M4, M6, M7 and M8 on axes A, D, G and J were supported by real detection, strict assertions or DWARF presence instead.
3. **Single backend.** Every behavioural axis ran on tkrzw only. The workspace default (bdb) and kc were never built, and D-07 (kc) was therefore unreachable.
4. **Premature generalisations:**
   - Φ8 extends one validated driver to the whole instrument;
   - D11 asserts "+ logs" without checking the log calls (the subject has two, both at init);
   - D12 asserts "no broader" without enumerating the error paths.
5. **Register text accepted as evidence.** D8 (row 20's mechanism), D10 and D11 restate register rows without measuring them.
6. **The abidiff "depth" was argued from the presence of a `.debug_info` section,** not from type coverage of the exported functions.
7. **Counts not regenerated against the installed set.** "7/7 headers" versus the 5 installed.
8. **An open REVERT TARGET classified as registered (R).** D4–D7.

## 7. Register reconciliation (axis K, static, executed where noted)

### 7.1 Unregistered divergences

D-01 to D-07, D-10 to D-12, D-14 to D-19, D-21, D-23, D-25 and
C-1 to C-3 above. D-08, D-09, D-22 and D-24 are NOT-ESTABLISHED,
not established unregistered divergences. In addition, the
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
6. **Upstream issues: no repo defect.** The repo's docs attribute #566,
   #542 and #518 correctly to ibus-libpinyin (`robustness-evidence.md:40,169,183`).
   Only the audit mandate called them libpinyin issues. An earlier draft of this
   list wrongly recorded that as a register defect.
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

**That area is ephemeral and is not retained with this document.** Before the build target dirs were deleted, everything else in it was archived to `~/audit-r2-evidence-20260924T231404Z.tar.zst` on the auditor host (`tar --exclude='./work/target-*' | zstd -10`). The archive is 815,523,174 bytes, sha256 `79c912b8a60eb987b9b42cafe77c5ffdcc66f79f4398ea1510261632ca52eeaf`, and `zstd -dc | tar -tf` lists 155,219 entries cleanly with no `work/target-*` member. The deleted `work/target-*` dirs held only cargo build output. Any
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
   build (bdb) is also a cell with remaining NOT-ESTABLISHED axes. The §12
   gate uses a tkrzw-derived fixture and misses G-m1 (§13.1).

## 10. Open gaps and proposed follow-ups

| gap | what blocked it | next step |
|---|---|---|
| L on all cells | never started | run the pre-registered workloads on a quiet host, one cell at a time |
| G on all cells | G-m1 reaches code and changes four non-fixture C-ABI outputs, but the §12 gate misses it (§13.1) | extend the fixture with a changed input and rerun both mutants and unset |
| J internal ledger | not produced | clang `-emit-llvm` call graph from the 131 exports |
| D-08, D-24 attribution | salvaged, not analysed | diff the unigram/bigram dumps per phrase; classify the key-byte deltas by scheme and option |
| GitHub tracking (Step 5) | not started; token lacks the `project` scope | `gh auth refresh -s project`; file one issue per D-row, register item and gap |
| Re-measure on the current main | main moved to `34a66bc9` (`expand_keys` change in core) | re-run B, C and G on the new tip |

Proposed for adoption as repo tools (each would need its own reviewed PR):
- the fork-per-probe contract battery (`contract-battery.c`);
- the open-counter cycle driver;
- the getenv interposer;
- the header layout probe generator;
- `zhuyin-diff-ext`.

## 11. Re-run of severity-1 and severity-2 findings at the current main

The subject was rebuilt at `origin/main` =
`34a66bc915c93a09a1680e70c5c2c252f95ffdfe` (same recipe and features per
cell; stage `work/stage-main-<cell>`) on 2026-09-24/25 (UTC). Each finding's
own harness was re-run with only the subject library swapped. Because the
oracle side is unchanged, an unchanged subject output means the divergence
still reproduces.

| rows | harness | comparison against the `18d78208` subject | result |
|---|---|---|---|
| D-01 | D open-counter cycle (A/B/C protocols) plus the libzhuyin cycle | logs identical on all cells; wipe at launch 8 | **still reproduces at 34a66bc9** |
| D-02, D-04, D-06, D-10, D-11, D-12, D-18, D-19, D-25 (the B parts) | B contract battery (252 probes) | identical apart from the three stderr self-size lines that embed the run tag | **still reproduces at 34a66bc9** |
| D-03 | F site executor (fx-pinyin, fx-zhuyin) | 0 differing lines on every cell | **still reproduces at 34a66bc9** |
| D-02, D-12, D-17, D-18, D-19, D-20, D-25 (the E parts) | E probes (e-pinyin, e-zhuyin, plus the named `H_gint_*` probes) | 0 differing lines after timestamp masking | **still reproduces at 34a66bc9** |
| D-05, D-26 | C drivers, subject side (80 outputs/cell) | 80/80 identical | **still reproduces at 34a66bc9** |
| D-07 | kc write-by-subject / read-by-oracle | 泥壕 still absent on kc; tkrzw and bdb interoperate | **still reproduces at 34a66bc9** |
| D-08 (NOT-ESTABLISHED) | D round-trip | 56/56 text dumps identical; the DB byte hashes vary run to run on both SHAs | observation unchanged at 34a66bc9 |
| D-09 (NOT-ESTABLISHED) | H fork, kc, on a quiet host | oracle 0.06 s exit 0; subject exit 137 at the 300 s timeout at **both** SHAs | observation unchanged at 34a66bc9. Slowness is now excluded; the source of the hang is still unlocated |
| D-13 | G C-ABI fixture surface (504 inputs) | subject identical; oracle vs subject still 106 lines | **still reproduces at 34a66bc9** |
| D-14 | ibus-libpinyin 1.16.5 engine replay, library swapped | identical apart from the resolved-library path | **still reproduces at 34a66bc9** |
| D-15 | `Cargo.toml` metadata and the built `.pc` at main | `Version: 2.11.91`, `libpinyin-2.11.91` | **still reproduces at 34a66bc9** |
| D-16 | H `init_twice` | identical on all cells | **still reproduces at 34a66bc9** |

## 12. Issue map

Tracking issue: #573.

| row | issue |
|---|---|
| D-01 | #523 |
| D-02 | #524 |
| D-03 | #525 |
| D-04 | #526 |
| D-05 | #527 |
| D-06 | #528 |
| D-07 | #529 |
| D-20 | #530 |
| D-09 | #531 |
| D-10 | #532 |
| D-11 | #533 |
| D-12 | #534 |
| D-13 | #535 |
| D-14 | #536 |
| D-15 | #537 |
| D-16 | #538 |
| D-17 | #539 |
| D-18 | #540 |
| D-19 | #541 |
| D-25 | #542 |
| D-08 | #543 |
| D-21 | #544 |
| D-23 | #545 |
| D-22 | #546 |
| D-24 | #547 |
| C-1 | #585 |
| C-2 | #586 |
| C-3 | #587 |
| R-1 | #548 |
| R-2 | #549 |
| R-3 | #550 |
| R-4 | #551 |
| P-1 | #552 |
| NE-L-tkrzw | #553 |
| NE-L-bdb | #554 |
| NE-L-kc | #555 |
| NE-G-tkrzw | #556 |
| NE-G-bdb | #557 |
| NE-G-kc | #558 |
| NE-Copt-tkrzw | #559 |
| NE-Copt-bdb | #560 |
| NE-Copt-kc | #561 |
| NE-Bzy-tkrzw | #562 |
| NE-Bzy-bdb | #563 |
| NE-Bzy-kc | #564 |
| NE-J-tkrzw | #565 |
| NE-J-bdb | #566 |
| NE-J-kc | #567 |
| NE-Cdrv-tkrzw | #568 |
| NE-Cdrv-bdb | #569 |
| NE-Cdrv-kc | #570 |
| NE-Fdet-bdb | #571 |
| NE-Step4 | #572 |

## 13. Completion run: item 2 (gate mutations, vacuous drivers, bdb F determinism)

This work was pre-registered in addendum 1 of the pre-registration file
(`e18fd9fc`). The subject is `18d78208`; the mutant trees are `mut-src`
`5eb5c51` and `mut-src-2` (`5eb5c51` plus the scratch-only `DRV-*` mutations
and a G-m1 hit marker, `7cf6f05`). Measured 2026-09-24/25 (UTC).

### 13.1 G-m1 / G-m2 through the §12 gate

The runs used the release-profile gate
(`harness/G-gate/r2-run-gate.sh <cell> mut <label> <MUTID|unset>`).

| cell | G-m1 | G-m2 | unset (revert) |
|---|---|---|---|
| tkrzw | pass, 491/396/390: **not detected** | FAIL, 491/395/388: detected | pass, 491/396/390 |
| bdb | pass, 491/396/390: **not detected** | FAIL, 491/395/388: detected | pass, 491/396/390 |
| kc | pass, 491/396/390: **not detected** | FAIL, 491/395/388: detected | pass, 491/396/390 |

**G-m1 determination: blind instrument.** It applies on every cell:

- **The path is reached.** A hit marker in the gated branch (`mut-src-2`)
  fires at least 262,144 times over the 504 gate inputs and at least
  4,194,304 times over the 10,465-input corpus, on each cell.
- **The output changes.** The C-ABI surface
  (`harness/G-gate/rerun-surface.sh <cell> mut2 <inputs> …`) with G-m1
  against unset differs on **4 of 10,465** corpus inputs, the same 4 on
  every cell. For example, `xiehenshuaitong` n-best rows 1 and 2 swap
  (写很帅通 / 写很率同), and `yaomeichong` row 1 changes 妖媚冲 → 要枚冲.
- **The gate does not see it.** None of the gate's 504 fixture inputs
  changes, and the unset surface equals the pristine surface.

So the §12 gate cannot detect a per-step cost shift that does change
observable n-best output. Its fixture is too narrow. G therefore stays
**NOT-ESTABLISHED** on every cell. The gate fails the pre-registration's
mutation rule because G-m1 is undetected, even though G-m2 is detected and
reverted. This is filed as a register/instrument-integrity finding (§12
issue map, G-1).

### 13.2 The seven drivers that missed C-m2

The `sdd` group was run with `SUBJECT_ENV=OXPINYIN_AUDIT_MUT=<id>` against the
`mut2` object. In the table, "detected" is the count of subject-output lines
that differ from `pristine-1`.

| driver | mutation | tkrzw | bdb | kc | unset = pristine |
|---|---|---|---|---|---|
| key-surface | DRV-key | 36 | 36 | 36 | yes (all cells) |
| dict-surface | DRV-dict | 8 | 8 | 8 | yes |
| phrase-surface | DRV-phrase | 22 | 22 | 22 | yes |
| pred-order | DRV-punct (substitute) | 8 | 8 | 8 | yes |
| predict | DRV-punct (substitute) | 1 | 1 | 1 | yes |
| punct | DRV-punct2 (substitute) | 12 | 12 | 12 | yes |
| import | DRV-import | 8 | 8 | 8 | yes |

Deviations, both allowed by the pre-registration's falsifier clause:

1. **DRV-pred is never on these drivers' path.** `pred-order-diff` resolves
   `pinyin_guess_predicted_candidates_with_punctuations` first and calls the
   plain function only as a fallback (`pred-order-diff.c:149-151`).
   `predict-diff`'s plain call on 测测 yields at most one row, so reversing
   it is invisible. The substitute DRV-punct mutates the function both
   drivers actually call.
2. **DRV-punct removed a row that `punct-diff` never prints.** It dropped the
   last candidate, a phrase row. The substitute DRV-punct2 drops the first,
   a punctuation row.

All seven drivers now detect a mutation in their own scope and revert
cleanly. Their IDENTICAL oracle-vs-subject results (section 4, C) therefore
become **MATCH** on every cell. Gap issues #568–570 are answered.

### 13.3 F determinism on bdb

- **Source of the nondeterminism.** On bdb the subject side prints libdb's
  error message `BDB0137 write: 0x<heap address>, <n>: File too large`, and
  the heap address changes per run under ASLR. That accounts for all 4 raw
  differing lines between runs a and b.
- **Normalisation.** Pre-registered normalisation 2 (pointer → `PTR`) removes
  them. Fresh bdb runs a and b (2026-09-25T00:06Z) are identical after it,
  and identical to the earlier v3a run.
- **Mutations.** F-m1 (5 lines) and F-m2 (6 lines) are detected on **all
  three** cells.
- **Unset vs pristine.** Unset equals pristine except the failing-malloc
  sweep's `tried=` tallies. The gate's own `std::env::var` read allocates,
  which adds allocation points (a known baseline, see `mutations/INDEX.md`).
  With those tallies excluded, the difference is 0 on every cell.
- **Result.** bdb F is deterministic under the declared normalisation. Gap
  #571 is answered.

## 14. Completion run: item 3 (libzhuyin half of axis B)

This work was pre-registered in addendum 2 (`44a46988`). The protocol re-ran
2026-09-25T00:5x–01:22Z. Runs from before the addendum are kept separately and
are not used for verdicts.

### 14.1 Instrument validation

`harness/B-zhuyin/v2/cell.sh <cell>` covers all 52 exports; none is unmapped
or has zero probes. It runs 458 probes per cell. In the table, "stable" means
the r1 and r2 runs of that side are identical.

| cell | oracle stable | subject stable | C-m3 detected | C-m3 revert | Bz-shim-m1 detected | shim revert | pristine sha |
|---|---|---|---|---|---|---|---|
| tkrzw | 0 diff | 0 diff | 10 probes | 0 diff | 134 probes | 0 diff | = BUILD-INFO |
| bdb | 0 diff | 1 diff: the libdb `BDB0137` heap address; 0 after normalisation 2 | 11 probes | 0 diff after normalisation 2 | 135 probes | 0 diff after normalisation 2 | = BUILD-INFO |
| kc | 0 diff | 0 diff | 10 probes | 0 diff | 134 probes | 0 diff | = BUILD-INFO |

### 14.2 Result

The oracle-vs-subject comparison masks only the init-time user-profile
diagnostic line, which is already D-23:
`cmp.py <cell>-oracle-r1.log <cell>-pristine-r1.log --mask-diag`.

| cell | probes differing | probes identical |
|---|---|---|
| tkrzw | 279 | 179 |
| bdb | 279 | 179 |
| kc | 278 | 180 |

Every one of the 52 exports has at least one differing probe on every cell. So
**no libzhuyin export is MATCH**, and the per-export ledger (52 exports × 3
cells) is **DIVERGENT** throughout. It is regenerated by
`python3 harness/B-zhuyin/v2/verdicts.py` → `results/X-B-zhuyin/v2/per-export.tsv`.

The differing probes fall into these defects:

| defect | status | evidence |
|---|---|---|
| **Z-1** (new) | new issue | `zhuyin_iterator_add_phrase` parses the reading with `FewestKeys` (full pinyin) on the subject (`crates/oxpinyin-zhuyin-capi/src/iterators.rs:87-93`) but `ZhuyinDirectParser2` (bopomofo) on the pin (`src/zhuyin.cpp:516-523`). The pin accepts `ㄘㄜˋ ㄘㄜˋ` and rejects `ce'ce`; the subject does the reverse. Every bopomofo import fails on the subject, whatever the count (−2, −1, 0, 5, INT_MAX, INT_MIN, library 1–6) |
| **Z-2** (new) | new issue | `zhuyin_token_get_unigram_frequency` returns `system_unigram_count + 1` (`crates/oxpinyin-zhuyin-capi/src/dict.rs:233-239`), so a system token reads 52888 against the pin's 52887. The pinyin twin returns the stored field, which already includes upstream's +1 |
| **Z-3** (new) | new issue | `zhuyin_guess_candidates_after_cursor` at mid-key offsets of `su3cl3`: offsets 1, 2 and 5 give pin n=1, subject n=126/126/94. After a choose, `before_cursor` gives pin 94/126, subject 1/94. Register rows 25 and 26 claim this window is CLOSED |
| D-03 twin | comment on #525 | 16 zhuyin abort shapes answered silently: `_check_offset` (4 exports), `get_character_offset`, scheme 0/7/10/30, full scheme 0/4, load/unload library, the `train_result3` assert |
| D-04 twin | comment on #526 | NULL instance or out-param: pin SIGSEGV, subject silent |
| D-25 twin | comment on #542 | out-param write-on-failure (`get_sentence` and `token_get_phrase` write NULL where the pin leaves the sentinel), `save`/`train` return values, `unload` twice |
| D-23 twin | comment on #545 | init diagnostics; `user.conf` written at init by the subject |

- **Library index 16** (`iap:16`): the pin SIGSEGVs and the subject returns
  false. This is the zhuyin twin of BP-31 and falls under D-04's crash
  asymmetry.
- **`zhuyin_set_options(0xFFFFFFFF)`** (pin 499 candidates, subject 126) is
  left to item 4's option sweep.

### 14.3 Round-1 D3

Round 1 claimed that the subject's zhuyin iterator substitutes
`PinyinKey::MAX` for an out-of-range key where the pinyin twin rejects
(`iterators.rs:93`). None of the 18 key-shape probes shows any divergence:
garbage, bad, 3keys, empty, NULL and bad UTF-8 are all IDENT on every cell,
because both sides reject them. The observable divergence is the parser
itself (Z-1). D3 is therefore graded **REFUTED as characterised**: the
`MAX` substitution has no observable effect, and the real defect is Z-1.

## 15. Completion run: item 4 (options, encoding, layouts)

Addendum 3 of the pre-registration preceded the option sweep; addendum 4,
commit `065f601f`, preceded the isolated-import rerun. The audited objects
remain those built from `18d782089bd1`, with tkrzw, bdb and kc using their
matched oracle data directories. The option sweep began
`2026-09-25T01:28:02Z`; the current-main import recheck ended
`2026-09-25T20:58:48Z` (timestamps emitted by the run scripts).

The scratch drivers and raw results are under
`/home/sheng/audit-r2-scratch/harness/C-options/` and
`/home/sheng/audit-r2-scratch/results/X-C-options/`. Regenerate the bounded
summaries with `python3 harness/C-options/audit_sweep_results.py`,
`python3 harness/C-options/audit_encoding_results.py`,
`python3 harness/C-options/audit_layout_results.py`, and
`python3 harness/C-options/audit_isolated_import.py`. Their captures are
`logs/item4-{sweep,encoding,layout,isolated-import}-analysis.log`.
None is in the earlier evidence archive. The supplemental local archive
`/home/sheng/audit-r2-item4-evidence-20260925T211313Z.tar.zst` contains the
item-4 harness, raw results and logs. Its SHA-256 is
`0e4a1c51d84ed8f4bad9df50736073e764923cf09c31e8132ce553df4f69dfdd`;
`sha256sum -c` passed and `tar --zstd -tf` listed 19,259 entries without
error. The archive is on the auditor host, not uploaded to the draft PR.

### 15.1 Instrument controls and hygiene

| surface | inputs per cell | oracle repeat | subject repeat | mutation detected | unset vs pristine |
|---|---:|---|---|---|---|
| options, single words | 104 words × 496 corpus lines | byte-identical | byte-identical | C-m1: 34 words | byte-identical |
| options, defined-bit pairs | 435 words × 149 corpus lines | byte-identical | byte-identical | C-m1: 29 words | byte-identical |
| encoding battery | 1,043 case headers | identical after process-ID/time normalization | identical | C-m1: 2 cases; C-m3: 3 cases | identical |
| layout table + three corpora | schemes 0–10; 1,236 lines per corpus | identical | identical | C-m3: table and all three corpora | identical |
| isolated import | 4 exports × 58 named inputs | two identical fresh-dir runs | two identical fresh-dir runs | C-m1/C-m3 in parent encoding battery | pristine staged hashes unchanged |

Source-side spot checks: pinned `pinyin.cpp:1498-1515,1557-1559,1602-1604`,
`storage/zhuyin_parser2.cpp:48-55`, `zhuyin.cpp:500-537`; audited Rust
`session/mod.rs:593-640`, `session/lookup.rs:1030-1051`, `capi/ffi.rs:19-28`,
and `zhuyin-capi/iterators.rs:59-92`. On each cell,
`head -n 2 results/X-C-options/sweep/<cell>/sha-before.txt | cmp -
results/X-C-options/sweep/<cell>/sha-after.txt` passed; the corresponding
layout SHA files compare directly. `git -C mut-src status --short` was empty.
The gated mutant with its variable unset also matched the pristine object.

Hygiene deviation: after addendum 3, the encoding launch script changed
from background fan-out to sequential execution with `set -euo pipefail`;
the probe source, inputs and output format were unchanged. That contradicts
addendum 3's sentence that the *only* harness change was the option runner's
parallelism. The original `encbat.c` SHA-256 remained
`0743076fbd0373c2f015f68201b5f0cf43cc7892126029e2c9a7e4c99d20da4c`;
the isolated importer used a separately compiled, exact-filter copy.

The analyzers remove D-23's known fresh-user-dir diagnostic to compare
payloads, and normalize glib process IDs/times. Thus "same" below is
**conditional payload equality**, not a MATCH verdict under the
pre-registered diagnostic-inclusive rule. D-23 remains DIVERGENT for those
cases. The analyses never mask option words or candidate hashes.

### 15.2 Option words

`wc -l harness/C-options/words-{single,pair}.txt` gives 104 and 435;
the corpus counts are 496 and 149. All three cells gave the same conditional
block counts per cell:

| kind | same payload | oracle abort | candidate | parse/key | sentence |
|---|---:|---:|---:|---:|---:|
| single | 35,052 | 11,088 | 4,955 | 294 | 299 |
| pair | 43,932 | 14,988 | 5,779 | 0 | 551 |

The first changed fields, not a blanket verdict for each cluster:

- **C-1:** word `0x00000002`, `P3 input=tsz`: both sides consume three
  bytes and expose the same incomplete `c` key; pin `sentence=1/1 从,
  n=718`, subject `sentence=1/0, n=0`. Among single words the pin gave
  n=718 vs 0 on 34 words and n=2188 vs 0 on one; six pin runs aborted.
  In the subject, `walk` drops `EdgeKind::Incomplete` whenever
  `PINYIN_INCOMPLETE` is clear (`lookup.rs:1042-1051`), even though the
  transformed chewing key was parsed. This is a source-path explanation,
  not a claim that every affected word has the same cause.
- **C-2:** word `0x00008002` (`PINYIN_AMB_L_N`), `D1 input=nihk`:
  both sides consume all four bytes and select 你好; pin n=499 with 利好,
  subject n=126 without it. `Z1 input=su3cl3` gives pin n=497 and subject
  n=125. The pin calls `fuzzy_syllable_step` after matrix fill for both
  transformed parsers; the subject's `build_scan_matrix` returns early when
  `divided` is false, before `fuzzy_additions`. The pair sweep found this
  D1 count mismatch on 28 words per cell.

### 15.3 Encoding and isolated import

The battery's 1,043 case headers per cell had stable repeats and successful
mutation controls. Excluding the 232 import case headers that shared a user
directory, the analyzer found 237 conditional payload differences per cell;
many are previously ledgered abort, input-limit and key differences. The
shared-directory import counts (133/27/27 differing cases on
tkrzw/bdb/kc) are **not** per-input verdicts and were discarded.

The addendum-4 rerun put each import probe/input/side/repeat in a new user
directory. `audit_isolated_import.py` regenerated the same counts on every
cell, with no missing or unstable case:

| import export | conditional equal payloads | differing payloads | first differing inputs |
|---|---:|---:|---|
| pinyin phrase | 56 | 2 | invalid surrogate, above-Unicode-max bytes |
| pinyin reading | 53 | 5 | `ni\xffhao`, half-width digit, 256/1024-byte `su3`, `jv` |
| zhuyin phrase | 53 | 5 | apostrophe, space, invalid bytes, 绿 |
| zhuyin reading | 43 | 15 | tab/DEL, full-width punctuation/digit, `lu:`, long `su3`, others |

The distinct new boundary defect is **C-3**: for `ni\xffhao`, the pin's
`pinyin_parse_more_full_pinyins` reports consumed/parsed 2 and key `ni`,
while the subject reports 0 and no key. The pin passes the raw C bytes to
the parser; subject `cstr_to_string` converts an invalid-UTF-8 C string to
empty. The pinyin import of the same byte string returns 1 with exported
`ni` on the pin and 0 on the subject; invalid phrase-byte imports also
differ. The ordinary `zhuyin_iterator_add_phrase("你", "lu:")` result,
pin false/subject true, is **Z-1 / #575**, not a new issue: its direct-vs-full
parser difference was already found in §14.2. The remaining pinyin import
behaviour extends **D-12 / #534** and the input-cap finding **D-18 / #540**;
these cases are added to those issues rather than duplicated.

### 15.4 Keyboard layouts and current-main checks

The table enumerated printable ASCII for schemes 0–10 on both C ABIs.
The oracle/subject table differs in 14 of 22 function/scheme blocks on
each cell: six invalid-scheme abort-vs-false blocks already under D-03,
and the eight valid pinyin blocks' space/tone symbols reflect the existing
default-option difference D-10 / #532. Valid zhuyin table blocks agree
conditionally on D-23. For each of the three 1,236-line corpora, 980 cases
differ per cell, all with an oracle abort; no non-aborting case differs
after D-23 normalization. C-m3 is detected in the table and every corpus,
and its unset control matches pristine.

The selected severity-2 rerun against current `origin/main`
`34a66bc915c93a09a1680e70c5c2c252f95ffdfe` used relinked
`stage-main-{tkrzw,bdb,kc}` objects (`src-main`'s changed `scoring.rs` blob
matches that commit) and the same oracle prefixes. On **every** cell,
`repro-main-options.sh` still gives `P3 tsz` n=0 at `0x00000002`
against pin n=718, and `D1 nihk` n=126 at `0x00008002` against pin
n=499. `repro-main-import.sh` still gives `zhuyin_iterator_add_phrase`
for `lu:` pin false, current-main subject true on all cells. No default
was changed, and these rechecks do not replace the audited-SHA measurements.
