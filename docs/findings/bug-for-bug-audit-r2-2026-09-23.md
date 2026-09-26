# Bug-for-bug audit, round 2 (execution closed, partial)

## 0. Status

**Execution closed, partial.** Read this section before any verdict below.

- The execution phase ran from 2026-09-24T15:06Z to 2026-09-24T23:01Z UTC. It
  was stopped by the maintainer's instruction ("salvage results") before most
  axis agents had finished.
  - C's driver enumeration and E's defect ledger returned; C's driver
    verdicts are partial after the mutation-protocol correction (§13.2).
  - Four were salvaged from the files their agents had written: B (libpinyin
    half), D, F and G.
  - The other axes are partial, as the table below states.
- **Axis L (complexity) remains NOT-ESTABLISHED.** Cold open was measured
  with Callgrind/Massif on all cells; the other workloads and full structural
  inventory remain open (section 17).
- **Step 4 (round-1 verification) is done** (section 6).
- **Step 5 (GitHub tracking)** was done without a project board, which the maintainer removed from Step 5. Tracking issue #573 links the audit findings and gaps as sub-issues under milestone *Bug-for-bug audit r2*, with `bug-for-bug`/`verdict:*`/`axis:*`/`severity:*`/`backend:*` labels.
- **Measured subject:** `origin/main` at `18d782089bd1`. Main has since moved
  to `34a66bc915c9`, adding 00466d50 (`expand_keys` early stop in
  `oxpinyin-core/src/scoring.rs`) and 34a66bc9 (a fuzz corpus seed). No
  full primary-ledger axis was re-measured on the new tip. Selected
  severity-1/2 findings were re-run in section 11, and item 4's severity-2
  findings were re-run in section 15.
- **Findings were not adversarially re-verified.** Subagent enumeration was
  spot-checked (section 1.3). Section 4 identifies rows whose code or inline
  differential basis could not be located; those rows remain open claims, not
  established findings. Six of them (D-03, D-04, D-18, D-22, D-25, D-26) were
  moved out by the code-basis pass of 2026-09-26 (section 1.4, section 4.2);
  the five others are owned by another lane and stay listed.

| axis | tkrzw | bdb | kc | basis |
|---|---|---|---|---|
| A ABI surface | static MATCH (exc. .pc); layout MATCH | same | same | A-m1 and A-m3 detected and reverted; abidiff type section NOT-ESTABLISHED by pre-registration |
| B libpinyin contract | DIVERGENT; §4.2 gaps D-04/D-18/D-25 closed by the code-basis pass | DIVERGENT, with three nondeterministic oracle probes NOT-ESTABLISHED (§5) | DIVERGENT | B-m1, B-m2, F-m2 detected; gated revert identical for deterministic probes |
| B libzhuyin contract | DIVERGENT among 52 exports probed (Z-1..Z-3 plus twins, §14) | same | same | C-m3 and the Bz-shim-m1 shim detected and reverted on every cell |
| C drivers | DIVERGENT (6 kinds); pred-order, predict and punct NOT-ESTABLISHED | same, +1 broader | same | C-m2 detected by 6 drivers; four of the seven remaining drivers detected their named DRV-* mutation. Three used impermissible substitutes (§13.2); their IDENTICAL output is not MATCH |
| C options/encoding/zhuyin layouts | DIVERGENT (§15) | same | same | 539 option words, 1,043 encoding cases and 1,236 layout corpus lines; C-m1/C-m3 detected and reverted. Payload equality is conditional on D-23 diagnostics |
| D persisted state | DIVERGENT (open counter, cross-read) | same | same, +kc interop | D-m1 (round-trip) and D-m3 (cycle) detected; revert identical |
| E defect preservation | 37 DIVERGENT, 14 NE | same | same | E-m1 detected on all cells, reverted |
| F errors/diagnostics | DIVERGENT: 87 executed sites, each answered silently (§4.2 D-03 closed by the code-basis pass) | same | same | F-m1, F-m2 detected on every cell; bdb determinism holds under normalisation 2 (§13.3) |
| G numerics | **NOT-ESTABLISHED** (G-m2 detected, **G-m1 not**: blind gate, §13.1) | **NOT-ESTABLISHED** (same) | **NOT-ESTABLISHED** (same) | no G row may be MATCH. The §12 gate mutation runs were done: G-m2 was detected and reverted on all cells; G-m1 was reached and changed four C-ABI outputs outside the fixture, but the gate missed it. D-13 also lacks a minimal attributed output (§4.2) |
| H process state | DIVERGENT; D-22 source located by the code-basis pass | same | DIVERGENT; fork observation D-09 NOT-ESTABLISHED | H-m1 and H-m2 detected and reverted |
| I drop-in | DIVERGENT (.pc); D-14 consumer runtime NOT-ESTABLISHED | same | same | I-m2 and I-m3 (C-m2) detected; header matrix fails identically on both sides |
| J coverage | export set MATCH; internal **PARTIAL / NOT-ESTABLISHED** | same | same | static three-cell candidate ledger, but unresolved call edges and unverified counterparts (§16); J-m1/J-m2 detected and reverted |
| K register | 3 lists produced (section 7) | — | — | static reconciliation, no mutation by design |
| L complexity | cold open BOTH-WORSENED; remainder **NOT-ESTABLISHED** | same | same | Callgrind Ir and Massif live heap, three repeats per side; L-m1/L-m2 detected and reverted (§17). Other workloads and structural inventory remain open |

## 1. Provenance

### Method: code and data basis

Each finding rests on code citations at libpinyin pin
`074a2219c90feaf962d0d24f034514033ece5f99` and oxpinyin's audited SHA
`18d782089bd1dff1eec95d92a9897269b011a035`, plus the inline input and
output data needed to see the difference. A code path alone is not proof of a
measured output; where either side's code basis or the minimal differential
cannot be located, section 4 marks the row NOT-ESTABLISHED. Raw run logs and
local working files are not evidence for a finding.
In the ledger, unqualified upstream paths are under the pin's `src/` and
unqualified subject paths under the audited `crates/`; both resolve at the
SHAs above. Section 4's row audit supersedes broad execution counts in the
status table wherever a basis is missing.

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
| model | model20, checked in-image |
| local archive | `~/audit-r2-evidence-20260924T231404Z.tar.zst`: local working copy, not published, not required to verify any finding |
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
   - **Facade correction:** the pin's libzhuyin never raises this counter:
     `zhuyin.cpp:126-176` reads or creates `user.conf` without an increment,
     and `:741-757` has no counter write at fini. The subject's zhuyin
     facade uses the same `CapiContext::try_open` path as pinyin
     (`oxpinyin-zhuyin-capi/src/context.rs:34-45`), so it does raise the
     counter. This correction is documented in #523 and #573.
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
   | D-14 | the same oracle-built ibus binary was run with only the library directory swapped; the source-level attribution and minimal differing output were not retained inline | NOT-ESTABLISHED under this report's code/data rule |
   | D-15 | static `.pc` diff (section 5, A) | CONFIRMED |
   | D-16 | subject `oxpinyin-user/src/registry.rs:107-108`, process-global `OPEN_STORES` keyed by path | CONFIRMED |
   | D-18 | pin `phrase_index.cpp:168-170` (`ERROR_INTEGER_OVERFLOW` → false); subject `dict.rs:364-377` adds a u64 delta unchecked | CONFIRMED (the add-frequency part; the other parts rest on axis E's completed ledger) |
   | D-19 | pin `pinyin.cpp:896-910` returns `has_next_phrase`; subject `iterators.rs:407-408` returns true | CONFIRMED |
   | D-21 | pin BDB user tables are created 0600 (`chewing_large_table2_bdb.cpp:149`, `phrase_large_table3_bdb.cpp:164`, `ngram_bdb.cpp:55`); subject `oxpinyin-store/src/bdb/ffi.rs:298` uses 0644 | CONFIRMED |
   | D-22 | **source located** (code-basis pass, 2026-09-26): subject `crates/oxpinyin-user/src/store_libpinyin.rs:82` — `session_token` calls `std::env::temp_dir()`, and `create_scratch_dir` (`:91-121`, called from `open_libpinyin` `:144`) creates `$TMPDIR/oxpinyin-user-<hash16>-<nanos>-<n>` mode 0700 for the session store; the pin's `src` has zero `TMPDIR`/`g_get_tmp_dir`/`mkstemp`-family hits over 106 files | **CONFIRMED** |
   | D-23 | subject `persistence.rs` "non-conforming user profile wiped" path; pin `open … failed.` from `table_info` | CONFIRMED |
   | D-24 | key-byte deltas not classified | **downgraded to NOT-ESTABLISHED** |

   D-05, D-17, D-20, D-26 and the E-derived parts of D-02/D-12/D-18/D-19 rest
   on axes that returned complete ledgers (C, E).

### 1.4 Code-basis pass (read-only, executed 2026-09-26)

A separate read-only pass traced the rows that section 4.2 lists as lacking a
code basis — D-22, D-04, D-03, D-18, D-25 and D-26 — on both sides, and
supplied the citations and the executed inline data that the ledger rows and
the issues now carry. The five rows 4.2 still lists are owned by another lane
(lane D Phase 1 / lane H) and were not traced. Pin citations were read at
`074a2219` (blobs by `git show` from the host checkout at
`~/Documents/repos/libpinyin`); subject citations were read at `34a66bc9`,
whose `crates/` tree is blob-identical to this document's branch
(`git diff --stat 34a66bc9 HEAD -- crates/` is empty), so a `:line` is the
same line in both. Inline data is re-read from the archived evidence
(section 8 points at the archive); no new capture is committed.

| row | outcome of the pass |
|---|---|
| D-22 | source located — verdict leaves the NOT-ESTABLISHED set (section 1.3 item 6 and the ledger row) |
| D-03 | sites recounted: 357 raw lines in `src`, 70 `check_result` sites, 87 executed (74 divergent / 8 refuted / 5 not executed); the 87-row site table is in #525 |
| D-04 | the 68 exports are the ones the crashing NULL probes touch: 70 of the 83 NULL-class probes crash the pin, over 68 distinct exports; per-export deref and guard lines are in #526 |
| D-18 | every part cited and executed (input-length cap, frequency add, `remember_user_input` count, 97400-train totals) |
| D-25 | the named probes cited and executed |
| D-26 | issue identified as #535 (its body already carries this row); the `zhrgguor` candidate[2] probe is cited |

Skipped here as owned by other lanes: D-13, D-08, D-09, D-14 and D-24
(lane D Phase 1 / lane H); their ledger rows say so.

Commit history: squashed on merge; the original 27 commits, including the
timestamped pre-registration (`fad9ad47`) and its addenda, are preserved at tag
`audit-r2-history-2026-09-26`, and the original pre-registration commit is
preserved at tag `audit-r2-prereg-original`.

## 2. Pre-registration and falsifiers

The pre-registration was committed as `dbee7c04` (rebased to `fad9ad47`, same blob) before any measurement.

| falsifier | outcome |
|---|---|
| bdb cell must select BerkeleyDB and link libdb | not fired: DBM=BerkeleyDB, NEEDED libdb-5.3.so |
| `.ver` files byte-identical to the pin | not fired: `cmp` identical, 79 + 52 |
| any one-sided export | not fired: `nm -D --defined-only` sets are equal on every cell |
| SONAME ≠ `lib{pinyin,zhuyin}.so.15` | not fired |
| global 1: pristine re-run not byte-identical | **fired** on bdb for F (pinyin) and three B oracle probes. B's `fini_double`, `free_instance_double` and `fsite_mask_out_logger_245` are NOT-ESTABLISHED on bdb regardless of whether an oracle-vs-subject difference was observed; the pre-registered rule has no divergence exemption. F's separately pre-registered normalisation and rerun are in §13.3 |
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
| C driver-specific | all | Addendum-1 DRV-key/dict/phrase/import; DRV-pred/punct | first four detected; last two not detected by their assigned drivers | all unset runs matched pristine | pred-order, predict and punct remain NOT-ESTABLISHED (§13.2) |
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
| L | all | L-m1 Callgrind, L-m2 Massif | both detected in every cell | outputs identical; L-m1 Ir returned within 0.0021%, L-m2 heap exactly | cold-open instrument validated; axis still incomplete (§17) |

## 4. Ledger: distinct defects

One row per distinct defect, merged across the axes that observed it.
Severity follows the pre-registration scale (1 highest). Class is "none"
unless a live class demonstrably fits.

| ID | axes | cells | claim | upstream | subject | verdict | sev | issue |
|---|---|---|---|---|---|---|---|---|
| D-01 | D, H, B (BP-10) | all | **The subject wipes the whole user profile on the 8th launch.** The pin decrements `user.conf`'s open counter in `pinyin_fini`, so a steady pinyin user stays at 0/1. The pin's libzhuyin never raises the counter at all, even across clean or killed launches. The subject increments on every open through both facades and never decrements, so after 7 clean init→train→save→fini cycles the 8th init finds counter > 6 and deletes all user files. Executed: 10 cycles; subject `c8: WIPE`, 7 learned phrases → 0 on all cells; pin's pinyin path keeps 9 phrases. The libzhuyin differential also wipes only on the subject side (see #523). | `pinyin.cpp:185-186`, `:1194-1198`; `zhuyin.cpp:126-176`, `:741-757`; `storage/table_info.cpp:409-425` | `oxpinyin-capi/src/context.rs:119-129`; `oxpinyin-zhuyin-capi/src/context.rs:34-45`; `oxpinyin-user/src/persistence.rs:223-266` | DIVERGENT-UNREGISTERED | 1 | #523 |
| D-02 | B (BP-01), E (A-1), K (row 38) | all | `pinyin_train` ignores `index`. `train(1)`/`train(2)` write `train(0)`'s deltas; the pin trains the index-th n-best row. `train(len)`/`train(255)` abort on the pin and return true on the subject | `pinyin.cpp:2670-2691` | `oxpinyin-capi/src/candidates.rs:550` | DIVERGENT-UNREGISTERED | 1 | #524 |
| D-03 | F, B (BP-04), C, E | all | The prior 74-site class-(c) claim had no inline per-site ledger; the code-basis pass supplies it. Recount at `074a2219`: 357 raw `assert(`/`abort()` lines in `src` (54 `pinyin.cpp` + 41 lookup/zhuyin/common headers + 150 storage non-backend + 112 backend; 61 of the 357 are `abort()`), 70 `check_result` sites (`include/pinyin_utils.h:27-31` expands to `assert`; no `NDEBUG` in the build and the oracle `.so` carries `__assert_fail`). 87 sites executed = 74 DIVERGENT-UNREGISTERED (73 SIGABRT + 1 SIGSEGV) + 8 trigger-refuted + 5 not executed; kinds 67 `assert` (2 entered through `check_result`) + 20 `abort`. No divergent site logs. This covers register rows 4, 5a, 5c, 5d, 6, 10, 14, 19, 21 and 22 | 87-row per-site ledger (site, kind, condition, trigger input, oracle outcome, subject answer line, log?) in #525; raw enumeration in the F ledgers (F1-F4) plus `X-F-exec/site-verdicts.tsv` | only glib log calls: `oxpinyin-capi/src/context.rs:21`, `oxpinyin-zhuyin-capi/src/context.rs:44` (both init-failure only); `subject_logged_site = False` on all 87 | DIVERGENT-UNREGISTERED | 1 | #525 |
| D-04 | B (BP-03) | all | The prior 68-export NULL claim had one cited example; the code-basis pass supplies the per-export ledger. Of the 83 NULL-class probes, 70 crash the pin (68 distinct exports, all `signal=11`; e.g. `save_null`: pin `(no stdout) => signal=11`, subject `save(NULL) -> false`) and all 83 answer `status=0` on the subject. The 13 non-crashing NULL probes (`pinyin_init(NULL,…)` false, `pinyin_lookup_tokens` NULL array, …) are excluded from the 68 | per-export first deref of the NULL argument in `src/pinyin.cpp` (e.g. `:1312` `instance->m_context`, `:509`/`:665`/`:777` iterators, `:1196` `context->m_user_table_info…`, `:1348` `g_free(instance->m_prefix_ucs4)`, `:2847` `*num = instance->m_candidates->len`, `:2876` `*utf8_str = candidate->m_phrase_string`, `:2507`/`:2593` live asserts on `candidate->…`); 68-row table in #526 | per-export `is_null()` guard (`instance.rs:19,41,59`, `iterators.rs:63,174,226,349,418`, `config.rs:21,60,100,156,183,212,237,265,294`, `candidates.rs:309,501`, `keys.rs:48,91,127,254`, `cursor.rs:241` `render_key`, `context.rs:120,145`); 68-row table in #526 | DIVERGENT-UNREGISTERED | 1 | #526 |
| D-05 | C (union), K | all | Register row 33 (REVERT TARGET) is still present: after a whole-row NBEST choose + train the subject writes the user bigram and predicts `你`; the pin predicts nothing. Row 20 attributes the same line to the (a) residual, which is falsified | `pinyin.cpp:2515-2520`, `lookup/phonetic_lookup.h:866` | `oxpinyin-engine/src/constraint.rs:193-220`; `oxpinyin-engine/src/session/selection.rs:25-48` | DIVERGENT-UNREGISTERED | 1 | #527 |
| D-06 | B (BP-12, BP-12b) | all | The subject crashes where the pin does not: `pinyin_alloc_instance` after `pinyin_fini` → SIGSEGV. Inversely, guessing on an instance that outlives its context crashes the pin and answers true on the subject | `pinyin.cpp:1194-1200,1310-1320,1372-1391` | `oxpinyin-capi/src/instance.rs:18-27`; `sentence.rs:119-154` | DIVERGENT-UNREGISTERED | 1 | #528 |
| D-07 | B (BP-42) | kc | The subject writes `user_pinyin_index.bin`/`user_phrase_index.bin` as native KC databases, and the pin cannot `load_snapshot` them. A phrase imported on the subject is lost to the pin. This contradicts the policy's same-backend interop claim | `storage/chewing_large_table2_kyotodb.cpp:94-106` | `oxpinyin-user/src/persistence.rs:600-613,763-785` | DIVERGENT-UNREGISTERED | 1 | #529 |
| D-08 | D (round-trip) | all | The same user dir read by pin and subject gives different learned state: unigram of 你 53853 vs 52887, and the bigram row sets differ. The attribution was not completed | code basis not located | code basis not located | NOT-ESTABLISHED (code basis not located); skipped in the code-basis pass — owned by lane D Phase 1 / lane H | 2 | #543 |
| D-09 | H (fork) | kc | Parent init, forked child trains and saves: the subject run never exits (killed by the 300 s timeout, exit 137). The pin exits 0; a hang versus slow run was not separated | code basis not located | code basis not located | NOT-ESTABLISHED (code basis not located); skipped in the code-basis pass — owned by lane D Phase 1 / lane H | 1 | #531 |
| D-10 | B (BP-02) | all | The default option word after `pinyin_init` is `USE_TONE` on the pin and `PINYIN_INCOMPLETE` on the subject | `pinyin.cpp:329` | `oxpinyin-facade/src/lib.rs:56` | DIVERGENT-UNREGISTERED | 2 | #532 |
| D-11 | B (BP-07) | all | `pinyin_begin_get_phrases(ctx, 1..4)` exports every system row on the pin (95698/21234/28255/1051) and nothing on the subject | `pinyin.cpp:662-674,698-769` | `oxpinyin-facade/src/export_rows.rs:26-36` | DIVERGENT-UNREGISTERED | 2 | #533 |
| D-12 | B (BP-08), E (SIGN-1, IMP-1) | all | Import semantics differ. Negative counts, toned pinyin (`ce4'shi4`) and libraries 1/255 are accepted by the pin and refused by the subject. Count 0 exports as −1 on the pin | `pinyin.cpp:615-640` | `oxpinyin-capi/src/iterators.rs:118-143` | DIVERGENT-UNREGISTERED | 2 | #534 |
| D-13 | G | tkrzw (fixture) | Trellis keep-rule and comparator code differ, but the 504-input aggregate lacks a minimal attributed input/output pair in this report | `lookup/phonetic_lookup_heap.h:25-29,56-81`; `lookup/phonetic_lookup.h:75-88` | `oxpinyin-engine/src/nbest.rs:194-197,241-267` | NOT-ESTABLISHED (inline case not located); skipped in the code-basis pass — owned by lane D Phase 1 / lane H | 2 | #535 |
| D-14 | I | all | The 71-line ibus runtime difference has no located minimal differing line or libpinyin/oxpinyin call-path pair | code basis not located | code basis not located | NOT-ESTABLISHED (code basis not located); skipped in the code-basis pass — owned by lane D Phase 1 / lane H | 2 | #536 |
| D-15 | A, I, K | all | `libpinyin.pc`/`libzhuyin.pc` say `Version: 2.11.91` and `includedir …/libpinyin-2.11.91`; the pin says 2.11.92. `pkg-config --atleast-version=2.11.92` fails on the subject. `libdir` is hard-coded `/usr/lib` (the pin uses `${exec_prefix}/lib`), which breaks `--define-variable=prefix` relocation | `configure.ac:7-9`, `libpinyin.pc.in` | `oxpinyin-capi/Cargo.toml:145-162` | DIVERGENT-UNREGISTERED | 2 | #537 |
| D-16 | H | all | Two contexts on one user dir: independent on the pin (B does not see A's import; counter 2). Shared on the subject through the process registry (B sees A's import; counter 1; save results flip) | `pinyin.cpp:172-215,326-365` | `oxpinyin-user/src/registry.rs:104-108`; `store_libpinyin.rs:185-226` | DIVERGENT-UNREGISTERED | 2 | #538 |
| D-17 | E (G-LOC-1) | all | `pinyin_init` leaves the process `LC_NUMERIC` at "C" on the pin (`table_info.cpp` setlocale); the subject does not touch the locale | `storage/table_info.cpp:328,372` | `oxpinyin-capi/src/context.rs:30-80` (no locale call) | DIVERGENT-UNREGISTERED | 2 | #539 |
| D-18 | B (16, 17), E (INT-1..4) | all | The `add_unigram_frequency(G_MAXUINT)` edge has an identified code pair; the code-basis pass adds the wider wrap/saturate, length-cap and export bundle, executed: input cap — pin `parse(L=32767)=32767`, `parse(32768)=32767`, `parse(65536)=32767`, `parse(65537)=32767` (gint16 saturation) vs subject `parse(32767)=4096` (4096 the only SAME case); frequency add — pin `add[0](0x7fffffff)=1 → f=2147536534`, `add[1..3]=0`, `add(0xffffffff)=0` (value held) vs subject all `=1 → f=4294967295`; `remember(你好世界,-1)` export count — pin `-2147483644` (u32 `2147483652`) vs subject `2147483647`; 97400 trains — pin `uni(你)=2122897296`, export `你好\|ni'hao\|5985278`, subject `4294967295`, `2147483647` | `storage/pinyin_parser2.cpp:333` (`gint16 parsed_len`) → `pinyin.cpp:1511`; `storage/phrase_index.cpp:168-171` (overflow guard → `ERROR_INTEGER_OVERFLOW`, `include/novel_types.h:86`) → `pinyin.cpp:2836-2843`; `pinyin.cpp:604-605` (`count * unigram_factor`, 32-bit) with `:520-521` | `oxpinyin-engine/src/session/mod.rs:36` (`MAX_INPUT_BYTES = 4_096`) + `session/buffer.rs:75,111`; `oxpinyin-runtime/src/lib.rs:436-477` (saturating) + `oxpinyin-user/src/store_libpinyin.rs:460-462` (u32) + `oxpinyin-capi/src/iterators.rs:280,403` (`c_int::MAX`) + `oxpinyin-capi/src/dict.rs:364-377` | DIVERGENT-UNREGISTERED | 2 | #540 |
| D-19 | B (15), C, E (ORD-1/2, OFF-2) | all | Bigram export surface: last-row `get_next` returns true (register row 36 open); DB-walk export order; the pin skips the last key; the pin attributes `sentence_start` successors to the next predecessor | `pinyin.cpp:842-911` | `oxpinyin-capi/src/iterators.rs:371-411` | DIVERGENT-UNREGISTERED | 2 | #541 |
| D-20 | B (14), C, E (MSB-3.2) | all | Register row 1 (b) says "repeated export cycle". The pin SIGSEGVs on the **first** export cycle after a train (bdb deterministic, 4/4) | `pinyin.cpp:842-872` | `oxpinyin-capi/src/iterators.rs:371-411` | DIVERGENT-BROADER (row 1) | 1 | #530 |
| D-21 | B (41) | bdb | The pin creates the bdb user DB files mode 0600; the subject creates them 0644 under umask 022 | `storage/chewing_large_table2_bdb.cpp:149`; `ngram_bdb.cpp:55` | `oxpinyin-store/src/bdb/ffi.rs:298` | DIVERGENT-UNREGISTERED | 3 | #544 |
| D-22 | H | all | The reported `TMPDIR` read and temporary entry: **source located** by the code-basis pass. Executed (getenv interposer): subject `GETENV phase=init var=TMPDIR caller=libpinyin.so.15` ×2 vs pin 0; init creates `$TMPDIR/oxpinyin-user-<hash16>-<nanos>-<n>` (0700) holding the session store; an unclean exit leaves it (`fork_exit`: subject `TMPLIST pre-fork entries=1`, leftover `./oxpinyin-user-52d5ae3aa0e3673c-<nanos>-0/store.tkt`; pin `entries=0`, nothing left). A clean `pinyin_fini` removes it | no `TMPDIR` and no temp-dir API anywhere in `src` (0 hits over 106 files); the user bigram is loaded into memory (`pinyin.cpp:399-402`; `storage/ngram_bdb.cpp:47-77` "create in memory db"/"load db into memory"); the only `.tmp` names are `<user_dir>/*.tmp` (`pinyin.cpp:997-1003,1006-1012,1015-1020`) | `oxpinyin-user/src/store_libpinyin.rs:82` (`session_token` → `std::env::temp_dir()`, `:78-84`), `:91-121` (`create_scratch_dir`, 0700), `:144`; reached from `pinyin_init` via `oxpinyin-runtime/src/lib.rs:1001`; removal at `oxpinyin-user/src/registry.rs:166-181` | DIVERGENT-UNREGISTERED | 3 | #546 |
| D-23 | B (11), C | all | On a fresh dir the pin prints `open <dir>/user.conf failed.` and the subject prints `oxpinyin: non-conforming user profile wiped ...`; other diagnostic subclaims need their own code/data pairs | `storage/table_info.cpp:201,332` | `oxpinyin-user/src/store_libpinyin.rs:174-181` | DIVERGENT-UNREGISTERED (fresh-dir case only) | 3 | #545 |
| D-24 | A (ck-runtime) | all | Raw `ChewingKey` bytes and parse returns differ across schemes/options, but the attribution was not completed | code basis not located | code basis not located | NOT-ESTABLISHED (code basis not located); skipped in the code-basis pass — owned by lane D Phase 1 / lane H | 3 | #547 |
| D-25 | B (06, 18–22, 26, 32–39), E (D-1..3) | all | The assorted return-value/out-param bundle had no per-export code/data pair; the code-basis pass supplies per-probe pairs, executed: `get_sentence(0)` before a guess — pin `false`/`UNTOUCHED` vs subject `true`/`"nihao"`; on a false return the subject writes NULL where the pin leaves the slot untouched, while `get_character_offset` failures write `0` on the pin and leave `UNTOUCHED` on the subject; `nth_pron(1..G_MAXUINT)` — pin `true len=2 [0000 0000]` (garbage `4f60 0000` at `G_MAXUINT`) vs subject `false len=0`; double/chewing aux after a full parse — pin `true "\|ni hao "` vs subject `false ""`; `unload_phrase_library(2)` twice — pin `true,true` vs subject `true,false`; `save` with the user dir removed — pin `true` vs subject `false`; `train(0)` with no choose — pin `save → true` (11 files) vs subject `false` (1 file); `parse_full("n")` — pin `false` (`0000`) vs subject `true` (`0b00`); `parse_full("ni3")` — pin `true` (`2b30`) vs subject `false`; `get_sentence(3)` past the rows — pin `SIGNAL 6` (live `:1473` assert) vs subject `false`+NULL; function-static key slots — pin aliases across instances (`same pointer as first: yes`), subject `no` (per-instance `cursor.rs:600`) | `pinyin.cpp:1464-1470` (false when `0 == results.size()`, slot untouched), `:1473` (`assert(index < results.size())`), `:3193` (writes `0` before failing), `:2936`/`:2960` (function-static key slots), `:464` (second unload true), `:1132` (save true even when the renames fail), `:2801`, `:2979`, `:1372`/`:1426`, `:3440`/`:3518`, `:2184`; `pinyin_parser2.cpp` `parse_one_key` (incomplete key refused, written `0000`) | `oxpinyin-capi/src/sentence.rs:127-152` (NULL on the false path; raw input before a lookup), `cursor.rs:600` (per-instance slot), `dict.rs:116,213`, `config.rs:264`, `context.rs:139-164`, `cursor.rs:125`, `sentence.rs:202` (`get_character_offset` leaves the slot untouched), `sentence.rs:24`/`:50`, `text.rs:330`, `keys.rs:43`, `phrase.rs:33` | DIVERGENT-UNREGISTERED | 2–3 | #542 |
| D-26 | C (double3) | all | ZIGUANG `zhrgguor` candidate[2] NBEST: 宗人光卓然 on the pin vs 总人光卓然 on the subject. Executed by the code-basis pass (`pristine-1/scheme.double.3.diff`): `n_candidates: 98`, candidate[0] 纵然光卓然 and candidate[1] 总日光灼热 identical, only `candidate[2]` differs (both `type=NBEST_MATCH`). The probe and both sides' code paths are now cited; whether the difference resolves through the keep rule or another part of the trellis is the open scope question on #535 | `lookup/phonetic_lookup_heap.h:25-29`, `:56-81` | `oxpinyin-engine/src/nbest.rs:194-197`, `:241-267` | DIVERGENT-REGISTERED (row 11), attribution contested — see #535 | 2 | #535 (scope) |
| C-1 | C options | all | Secondary-zhuyin `tsz` consumes and exposes incomplete key `c` on both sides, but with option `0x00000002` the pin returns 718 candidates and sentence 从 while the subject returns zero candidates/no sentence. Subject's `walk` drops `Incomplete` unless `PINYIN_INCOMPLETE` is set | `storage/zhuyin_parser2.cpp:48-55`; `pinyin.cpp:1590-1605` | `oxpinyin-engine/src/session/lookup.rs:1030-1051` | DIVERGENT-UNREGISTERED | 2 | #585 |
| C-2 | C options | all | With `PINYIN_AMB_L_N`, transformed exact keys omit fuzzy alternates: double-pinyin `nihk` yields 499 candidates including 利好 on the pin, 126 without 利好 on the subject; all four bytes are consumed on both sides. The same loss appears for chewing `su3cl3` | `pinyin.cpp:1557-1559,1602-1604` | `oxpinyin-engine/src/session/mod.rs:593-640` | DIVERGENT-UNREGISTERED | 2 | #586 |
| C-3 | C encoding | all | For C bytes `ni\xffhao`, `pinyin_parse_more_full_pinyins` consumes the valid `ni` prefix (2) on the pin, but zero bytes on the subject. Other invalid UTF-8 import cases differ under the same C-string conversion | `pinyin.cpp:1498-1515,615-640` | `oxpinyin-capi/src/ffi.rs:19-28` | DIVERGENT-UNREGISTERED | 3 | #587 |
| Z-1 | B libzhuyin | all | `zhuyin_iterator_add_phrase` accepts bopomofo readings on the pin and full-pinyin readings on the subject; the opposite form fails on each side (§14.2) | `zhuyin.cpp:516-523` | `oxpinyin-zhuyin-capi/src/iterators.rs:87-93` | DIVERGENT-UNREGISTERED | 2 | #575 |
| Z-2 | B libzhuyin | all | A system token's `zhuyin_token_get_unigram_frequency` is 52887 on the pin, 52888 on the subject (§14.2) | `zhuyin.cpp:1813-1839` | `oxpinyin-zhuyin-capi/src/dict.rs:221-243` | DIVERGENT-UNREGISTERED | 2 | #576 |
| Z-3 | B libzhuyin | all | Candidate windows at mid-key offsets of `su3cl3` differ before and after a choose, despite register rows 25/26 claiming closure (§14.2) | `zhuyin.cpp:1460-1580` | `oxpinyin-zhuyin-capi/src/sentence.rs:181-290` | DIVERGENT-UNREGISTERED | 2 | #577 |
| C-4 (post-audit) | C candidates | all | Under `SORT_WITHOUT_SENTENCE_CANDIDATE`, the pin keeps NORMAL rows whose text equals an n-best sentence; the subject dedups them behind sentence rows and then filters those rows. This was found later at `34a66bc9`, not in the original `18d78208` run; the cited implementation files are unchanged between those SHAs | `pinyin.cpp:2058-2160,2295-2300` | `oxpinyin-engine/src/session/lookup.rs:732-755`; `oxpinyin-capi/src/sentence.rs:298-299,359-361` | DIVERGENT-UNREGISTERED (post-audit evidence) | 2 | #582 |
| D-27 (post-audit) | D user.conf | all | The pin parses the open counter with signed `%d` and accepts signs/trailing junk; the subject's bare `u32` parser treats those forms as zero. This was found later at `34a66bc9`, not in the original `18d78208` run; the cited implementation file is unchanged between those SHAs | `storage/table_info.cpp:356-368,409-426` | `oxpinyin-data/src/user_files.rs:282-283,356-363` | DIVERGENT-UNREGISTERED (post-audit evidence) | 3 | #583 |
| L-01 | L cold open | all | A fresh user profile forces an eager `system_originals` pass over every system item/pronunciation before store open; Callgrind Ir and Massif peak live heap are both above the pin by more than 1.10 on all three backends (§17) | `pinyin.cpp:172-199,259-269,326-405` | `oxpinyin-runtime/src/lib.rs:941-1000`; `oxpinyin-user/src/persistence.rs:73-110` | DIVERGENT-UNREGISTERED; overall L NOT-ESTABLISHED | 2 | #588 |

The B battery counted 757 probe rows and the F enumeration counted 357 sites.
Those counts are coverage inventory, not a substitute for the row-level
code/data basis below.

### 4.1 Minimal input and differing output

These are the input and output slices for rows with a located code pair in the
ledger. Unchanged setup is omitted: each C-ABI probe uses the matching cell's
pin data dir and a fresh user dir. For L-01, the complete three-cell values
are already inline in §17. The post-audit C-4/D-27 code blobs are identical at
`18d78208` and `34a66bc9` (`git diff` on their cited files is empty).

| ID | Minimal input | Pin output | oxpinyin output |
|---|---|---|---|
| D-01 | Eight clean `pinyin_init` → train → save → fini cycles on one user dir; reopen | counter returns to 0; learned phrases retained (9 by cycle 10) | counter ratchets; cycle 8 wipes, learned phrases 7 → 0 |
| D-02 | Parse `nihao`, guess, `pinyin_train(1)`; separately `train(255)` | row-1 unigram 好 38539; index 255 aborts | row-0 unigram 好 38056; index 255 returns true |
| D-05 | Choose a whole-row n-best candidate, train, then predict after 测测 | no predicted 你; no user bigram | `测测→你` count 138; predicted 你 |
| D-06 | `alloc_instance(ctx)` after `pinyin_fini(ctx)`; separately guess on an orphaned instance | first call survives; second SIGSEGV | first SIGSEGV; second returns true |
| D-07 | KC: import 泥壕/`ni'hao`/100 with subject, save, reopen same dir with pin | `candidate 泥壕 ABSENT (n=126)` | subject reopening finds 泥壕 at rank 0 |
| D-10 | Read option word immediately after `pinyin_init` | `USE_TONE` | `PINYIN_INCOMPLETE` |
| D-11 | `pinyin_begin_get_phrases(ctx, 1)`, exhaust iterator | 95,698 rows | 0 rows (libraries 2–4: 21,234/28,255/1,051 vs 0) |
| D-12 | `pinyin_iterator_add_phrase` with count −2; separately count 0 | accepts −2; exports count 0 as −1 | refuses −2; exports count 0 as 0 |
| D-15 | `pkg-config --atleast-version=2.11.92 libpinyin`; inspect `.pc` | exit 0; `Version: 2.11.92` | nonzero; `Version: 2.11.91` |
| D-16 | Open A and B on one user dir, import into A before save | B does not see import; counter 2 | B sees import; counter 1 |
| D-17 | Set `LC_ALL=zh_CN.UTF-8`, call `pinyin_init`, query `LC_NUMERIC` | `C` | `zh_CN.UTF-8` |
| D-19 | Train then call bigram iterator `get_next` on its last row | `你好\|ni'hao\|138\|false` | `你好\|ni'hao\|138\|true` |
| D-20 | Train once, then first bigram export cycle | SIGSEGV in `has_next_phrase` | cycle completes |
| D-21 | BDB, umask 022, create user DB | file mode 0600 | file mode 0644 |
| D-23 | `pinyin_init` on a fresh user dir, capture stderr | `open <dir>/user.conf failed.` | `oxpinyin: non-conforming user profile wiped ...` |
| C-1 | Secondary-zhuyin `tsz`, option `0x00000002` | 718 candidates; sentence 从 | 0 candidates; no sentence |
| C-2 | Double-pinyin `nihk` with `PINYIN_AMB_L_N` | 499 candidates including 利好 | 126 candidates, no 利好 |
| C-3 | C bytes `ni\xffhao` in `pinyin_parse_more_full_pinyins` | consumes 2 bytes | consumes 0 bytes |
| Z-1 | Import 测侧 with reading `ㄘㄜˋ ㄘㄜˋ`; separately `ce'ce` | true; false | false; true |
| Z-2 | `zhuyin_token_get_unigram_frequency(0x01001225)` | 52887 | 52888 |
| Z-3 | `su3cl3`, `zhuyin_guess_candidates_after_cursor` at offset 1 | 1 candidate | 126 candidates |
| C-4 | `li'shi`, option `0x1f`, guess sentence then candidates | 385 rows, starting 历史/理事 | 383 rows, neither 历史 nor 理事 |
| D-27 | Seed a conform profile, change `user.conf` to `open counter:+7`, then init | profile wiped; only marker remains | profile kept (11 files) |
| L-01 | Fresh user dir, empty input, cold `pinyin_init` on tkrzw | Callgrind Ir 22,295,138; Massif peak heap 9,449,311 B | Ir 334,313,247; heap 24,556,688 B |

### 4.2 Rows lacking code basis

The following rows are **not evidence-backed findings in this report**. The
prior input/output claims are retained for follow-up, but they cannot be used
to assert parity or divergence until the missing code or minimal data is
supplied. No raw-log pathname fills the gap. These five rows are owned by
another lane (lane D Phase 1 / lane H) and were deliberately not traced in
the code-basis pass of 2026-09-26.

| ID | Prior input and reported pin → subject difference | Missing basis |
|---|---|---|
| D-08 | Reopen a learned dir; unigram 你 53853 → 52887 | code basis not located for the changed value (lane D Phase 1 / lane H) |
| D-09 | Fork after KC init; parent exits 0 → killed after 300 s | code basis not located; hang vs slowness unresolved (lane D Phase 1 / lane H) |
| D-13 | 504-input trellis corpus; 62 keep-rule and 7 comparator moves claimed | code pair located, but no minimal attributed input/output line located (lane D Phase 1 / lane H) |
| D-14 | Pinned ibus key replay; 71 differing lookup/stderr lines claimed | code basis not located, nor one differing input/output line (lane D Phase 1 / lane H) |
| D-24 | Single-key scheme/options battery; thousands of parse-byte differences claimed | code basis not located for individual key/scheme outputs (lane D Phase 1 / lane H) |

**Moved out of this table by the code-basis pass (2026-09-26).** Six rows
were traced on both sides; each now carries its citations and executed
inline data in the ledger (§4), and the same citations are in its issue.

| ID | what was missing before | basis now cited | issue |
|---|---|---|---|
| D-03 | every claimed site and counterpart | 357 raw lines recounted; 87 executed sites, each with its condition, trigger input, oracle outcome, subject answer line and log flag | #525 |
| D-04 | 68-export scope | the 68 exports the crashing NULL probes touch (70 of 83 probes), with the first NULL deref and the guard line per export | #526 |
| D-18 | complete code/data pairs for the bundle | input-length cap, frequency add, `remember_user_input` count and the 97400-train totals, each with both lines | #540 |
| D-22 | the shipped subject path | `oxpinyin-user/src/store_libpinyin.rs:82` (`session_token` → `std::env::temp_dir()`), against the pin's total absence of a temp API | #546 |
| D-25 | the bundled 18 contract probes | the named probes with per-probe pin/subject lines and outputs | #542 |
| D-26 | this output and its attribution to row 11 | the probe's output pair plus both code paths; the attribution stays contested on #535 | #535 |

## 5. Per-axis narrative (non-MATCH only)

### A

- **Static comparisons MATCH on every cell** (a pure set/byte basis):
  - SONAME, the symlink chain and the version nodes;
  - the 79/52 versioned symbols;
  - the five headers, byte for byte.
- **Header layout MATCH**, from the generated probe: 185 lines, and the
  oracle-vs-subject diff is empty on all cells.
- **Raw cargo-c versus relinked object:** the retained
  `nm -D --defined-only` and `readelf -V` comparison of the raw cargo-c object
  with the object produced by committed
  `tools/packaging/relink-versioned.sh` on each cell found: for each
  cell, raw libpinyin has 79 defined API names, raw libzhuyin 52, no
  `@@LIBPINYIN`/`@@LIBZHUYIN` names and no version-definition section.
  Relinking adds one version-node symbol per object (80/53 defined names)
  and a two-entry `.gnu.version_d` with `LIBPINYIN`/`LIBZHUYIN`; the API
  name sets otherwise match. The falsifier that the raw object already
  carried those symbol versions did **not** fire. These static results are
  not a claim that the raw cdylib itself is drop-in compatible.
- **NEEDED differs, by construction:** the subject has no libstdc++/libm and
  adds ld-linux. This is recorded, not a finding.
- abidiff reports "75 Changed" functions on the type level (C++ vs Rust
  DWARF), which the pre-registration says cannot be a parity statement.
- D-15 and D-24 are the findings.

### B

The bdb libpinyin oracle's two pristine contract-battery runs differ at
`fini_double` (UB-dependent stderr bytes), `free_instance_double` (SIGSEGV
versus SIGABRT with a different diagnostic), and
`fsite_mask_out_logger_245` (a user-index file hash). For example,
`free_instance_double` ended with SIGSEGV on one run and SIGABRT on the other.
The reproduction harness is to be committed: fork each probe, run it twice
against the same bdb oracle build and fresh dirs, then compare the three
named result fields before assigning a verdict.
Under B's own falsifier, **all three probes are NOT-ESTABLISHED on bdb**;
observing a divergence on one run does not exempt it. The other deterministic
contract probes and their distinct findings are unaffected. This is not the
F normalisation run in §13.3.

### C

- Axis C's drivers did not all detect C-m2. Of the seven identical drivers,
  key-surface, dict-surface, phrase-surface and import detected the named
  driver-specific mutations pre-registered in Addendum 1. `pred-order`,
  `predict` and `punct` relied on after-the-fact substitutes while the named
  target existed; their IDENTICAL outputs remain NOT-ESTABLISHED (§13.2).
- The **zhuyin-diff corpus never exercises key `1`.**

### G

- The §12 gate reproduces 491/396/390 of 496 on tkrzw and kc, and passes its
  equality assertion on bdb.
- The gate measures the Rust engine against a **committed oracle fixture**.
  It does not test the bdb or kc oracle's own behaviour, and its mutation
  check ran on all three cells: G-m2 was detected and reverted, but G-m1
  reached an observable path and was missed by the fixture (§13.1).
- D-13 has a source-level keep-rule/comparator discrepancy, but the reported
  504-input aggregate lacks a minimal attributed input/output pair here.
  Its behavioural verdict is NOT-ESTABLISHED (§4.2).

### H

- D-16 is supported by the code/data pair in §4.1; D-09 is NOT-ESTABLISHED
  (§4.2), and D-22's source is located by the code-basis pass (§1.4).
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
| M3, M4 | shipped ABI identical; abidiff exit 0 is a deep comparison | CONFIRMED for symbol sets and versioning; REFUTED for the "deep" abidiff claim | the release subject has a `.debug_info` section, yet abidiff on it reports nothing, while a DWARF-built subject reports **75 changed** functions on every cell; build both sides with DWARF and run `abidiff` to reproduce |
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

### 6.4 Round-2 methodology breach

The round-2 continuation prompt's item 2b asked for a mutation within each
vacuous driver's scope or a proof of non-observability. That instruction did
not supersede the original pre-registration's section 3 rule: a named
mutation may be substituted after its failed run only if the named code does
not exist. Addendum 1's `DRV-pred` and `DRV-punct` targets did exist. Using
`DRV-punct` for `pred-order`/`predict` and `DRV-punct2` for `punct` to
upgrade their identical outputs to MATCH was a breach. Those three drivers
are NOT-ESTABLISHED on every cell (§13.2), and #568–#570 remain open.

## 7. Register reconciliation (axis K, static, executed where noted)

### 7.1 Unregistered divergences

D-01 to D-07, D-10 to D-12, D-14 to D-19, D-21, D-23, D-25,
C-1 to C-3, Z-1 to Z-3, and L-01 above. Post-audit follow-ups C-4
(#582) and D-27 (#583) are also unregistered, but their executions belong
to the later `34a66bc9` follow-up rather than the original audited snapshot.
D-08, D-09 and D-24 are NOT-ESTABLISHED,
not established unregistered divergences (D-22 was moved out of this set by
the code-basis pass of 2026-09-26: its source is located, see section 1.3
item 6 and its ledger row). In addition, the
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

## 8. Coverage ledgers and reproduction

### Export ledger (J)

- Command:
  `nm -D --defined-only /opt/oracle/<cell>/lib/lib{pinyin,zhuyin}.so.15 | awk '$2=="T"{print $3}'`
- Result: 79 + 52 = 131 on every cell; tkrzw == bdb == kc.
- A conservative, name-level internal candidate ledger was subsequently
  produced for each cell (§16). It is **not** a complete transitive mapping:
  indirect calls, overloaded method resolution and counterpart identity remain
  open. The J internal verdict remains NOT-ESTABLISHED.

### F site ledger

- 357 lines from the pre-registered grep, 70 `check_result` sites;
  87 caller- or data-reachable sites were reported executed. These are
  inventory figures, not a supported 74-site divergence verdict (§4.2).
- Prior per-site classifications, retained for follow-up:

| verdict | sites |
|---|---|
| DIVERGENT-UNREGISTERED | 74 |
| trigger refuted | 8 |
| not executed | 5 |

### Reproduction status

The installed ABI and export count can be regenerated with the commands
above and `tools/packaging/relink-versioned.sh`. The contract battery,
counterfactual, process-state, and internal-reachability harnesses are
**reproduction harnesses to be committed**. Until then, section 4 gives the
minimal call sequence and observed difference where one is available; a
scratch pathname does not fill a missing basis. Proposed harness adoption is
listed in section 10.

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
5. **Earlier record gap.** Before the human ruling below, no written human
   decision for the BDB default was found in the inspected tree, issue or PR
   record. The only maintainer act identified was the merge of #502;
   `mergedBy` was not queried. This remains the historical provenance finding.
6. **Human ruling, 2026-09-26 UTC (date captured with `date -u`).** The
   reference build is the pin's autoconf build and the reference default is
   bare `./configure`, selecting Berkeley DB (`configure.ac:94-100`). Parity
   coverage retains the tkrzw, bdb and kc cells. No default is changed by
   this audit. BDB still has NOT-ESTABLISHED probes, and the §12 gate misses
   G-m1 (§13.1).

## 10. Open gaps and proposed follow-ups

| gap | what blocked it | next step |
|---|---|---|
| L on all cells | cold open measured; §12, 200-train, 10k import/export and the complete structural inventory remain unmeasured (§17) | run the remaining matched workloads under Callgrind/Massif with per-run UTC/load and close J's structural inventory |
| G on all cells | G-m1 reaches code and changes four non-fixture C-ABI outputs, but the §12 gate misses it (§13.1) | extend the fixture with a changed input and rerun both mutants and unset |
| J internal ledger | a static candidate ledger exists, but mapping and indirect edges are unresolved (§16) | resolve call targets by USR/signature, verify each counterpart or cite deliberate absence, then re-check all three cells |
| C `pred-order`, `predict`, `punct` on all cells | C-m2 missed them; their named Addendum-1 mutations were not detected and later substitutions violated section 3 (§13.2) | inspect each call path (exact commands in §18), pre-register a valid within-scope mutation for round 3, then detect and revert it on each cell |
| D-08, D-24 attribution | salvaged, not analysed | diff the unigram/bigram dumps per phrase; classify the key-byte deltas by scheme and option |
| GitHub tracking (Step 5) | complete under the maintainer's no-board instruction: #573, issue map, labels, milestone and sub-issues exist | keep unresolved gap issues open for human triage; no further Step-5 buildout is pending |
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
| Z-1 | #575 |
| Z-2 | #576 |
| Z-3 | #577 |
| C-4 (post-audit) | #582 |
| D-27 (post-audit) | #583 |
| L-01 | #588 |
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

The gate's committed test is
`crates/pinyin-oracle/tests/sentence_surface_parity.rs`. The mutation
launcher is a **reproduction harness to be committed**: build the same
relinked subject with each gated G mutation set and unset, run the 504
fixture inputs against the matching pin data dir, and compare the verdict
counts below.

| cell | G-m1 | G-m2 | unset (revert) |
|---|---|---|---|
| tkrzw | pass, 491/396/390: **not detected** | FAIL, 491/395/388: detected | pass, 491/396/390 |
| bdb | pass, 491/396/390: **not detected** | FAIL, 491/395/388: detected | pass, 491/396/390 |
| kc | pass, 491/396/390: **not detected** | FAIL, 491/395/388: detected | pass, 491/396/390 |

**G-m1 determination: blind instrument.** It applies on every cell:

- **The path is reached.** A hit marker in the gated branch (`mut-src-2`)
  fires at least 262,144 times over the 504 gate inputs and at least
  4,194,304 times over the 10,465-input corpus, on each cell.
- **The output changes.** A C-ABI surface reproduction harness to be
  committed parses, guesses and prints n-best rows for each corpus input.
  With G-m1
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

| driver | named Addendum-1 mutation | tkrzw | bdb | kc | unset = pristine | verdict for IDENTICAL output |
|---|---|---:|---:|---:|---|---|
| key-surface | DRV-key | 36 | 36 | 36 | yes | MATCH on this driver |
| dict-surface | DRV-dict | 8 | 8 | 8 | yes | MATCH on this driver |
| phrase-surface | DRV-phrase | 22 | 22 | 22 | yes | MATCH on this driver |
| pred-order | DRV-pred | 0 | 0 | 0 | yes | NOT-ESTABLISHED |
| predict | DRV-pred | 0 | 0 | 0 | yes | NOT-ESTABLISHED |
| punct | DRV-punct | 0 | 0 | 0 | yes | NOT-ESTABLISHED |
| import | DRV-import | 8 | 8 | 8 | yes | MATCH on this driver |

The four MATCH rows detected their **named**, pre-registered driver-specific
mutations and reverted on all cells. Addendum 1 was committed before those
driver-specific measurements; these are scoped instrument checks, not a
claim that those four drivers detected the original C-m2 candidate swap.

The three NOT-ESTABLISHED rows were incorrectly upgraded using substitutes:
`DRV-punct` changed 8 `pred-order` lines and one `predict` line per cell,
while `DRV-punct2` changed 12 `punct` lines per cell, with clean unset runs.
Those detections do not rescue MATCH under section 3. `DRV-pred` exists but
`pred-order-diff` resolves the punctuation variant first and calls the plain
function only as a fallback (`pred-order-diff.c:149-151`); `predict-diff`'s
plain call has at most one row, so reversal is invisible. `DRV-punct` exists
but dropped a last row that `punct-diff` does not print. The named targets
were neither absent nor replaced under an allowed exception. Gap issues
#568–#570 remain open with `verdict:not-established` for these drivers.

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

The libzhuyin contract battery is a **reproduction harness to be committed**:
resolve all 52 exports by name, fork each of the 458 per-cell probes, and
compare two fresh-dir runs per side. None of the names was unmapped or had
zero probes. In the table, "stable" means the two runs of that side agreed.

| cell | oracle stable | subject stable | C-m3 detected | C-m3 revert | Bz-shim-m1 detected | shim revert | pristine sha |
|---|---|---|---|---|---|---|---|
| tkrzw | 0 diff | 0 diff | 10 probes | 0 diff | 134 probes | 0 diff | = BUILD-INFO |
| bdb | 0 diff | 1 diff: the libdb `BDB0137` heap address; 0 after normalisation 2 | 11 probes | 0 diff after normalisation 2 | 135 probes | 0 diff after normalisation 2 | = BUILD-INFO |
| kc | 0 diff | 0 diff | 10 probes | 0 diff | 134 probes | 0 diff | = BUILD-INFO |

### 14.2 Result

The oracle-vs-subject comparison masks only the init-time user-profile
diagnostic line, which is already D-23. This is an execution count, not a
code/data basis for each export.

| cell | probes differing | probes identical |
|---|---|---|
| tkrzw | 279 | 179 |
| bdb | 279 | 179 |
| kc | 278 | 180 |

The battery reported at least one differing probe for each of the 52 exports
on every cell, so no export is marked MATCH. The supported, individually
attributed differences are Z-1–Z-3 in §4; the aggregate per-export claim is
not promoted to a finding without inline code/data rows for the other exports.

The differing probes fall into these defects:

| defect | status | evidence |
|---|---|---|
| **Z-1** | #575; ledger §4 | `zhuyin_iterator_add_phrase` parses the reading with `FewestKeys` (full pinyin) on the subject (`crates/oxpinyin-zhuyin-capi/src/iterators.rs:87-93`) but `ZhuyinDirectParser2` (bopomofo) on the pin (`src/zhuyin.cpp:516-523`). The pin accepts `ㄘㄜˋ ㄘㄜˋ` and rejects `ce'ce`; the subject does the reverse. Every bopomofo import fails on the subject, whatever the count (−2, −1, 0, 5, INT_MAX, INT_MIN, library 1–6) |
| **Z-2** | #576; ledger §4 | `zhuyin_token_get_unigram_frequency` returns `system_unigram_count + 1` (`crates/oxpinyin-zhuyin-capi/src/dict.rs:233-239`), so a system token reads 52888 against the pin's 52887. The pinyin twin returns the stored field, which already includes upstream's +1 |
| **Z-3** | #577; ledger §4 | `zhuyin_guess_candidates_after_cursor` at mid-key offsets of `su3cl3`: offsets 1, 2 and 5 give pin n=1, subject n=126/126/94. After a choose, `before_cursor` gives pin 94/126, subject 1/94. Register rows 25 and 26 claim this window is CLOSED |
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

The committed `tools/bisection/option-sweep.c` and
`tools/bisection/run-option-sweep.sh` cover the option baseline. The expanded
encoding, layout and isolated-import battery is a **reproduction harness to
be committed**: enumerate the words and named byte/layout cases described
below, run each side against one pin data dir per cell, and compare the
candidate, parse and sentence fields while retaining diagnostics separately.

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
and `zhuyin-capi/iterators.rs:59-92`. The gated mutant with its variable
unset matched the pristine object; no user state was intentionally reused
between cases.

Hygiene deviation: after addendum 3, the encoding launch script changed
from background fan-out to sequential execution with `set -euo pipefail`;
the probe source, inputs and output format were unchanged. That contradicts
addendum 3's sentence that the *only* harness change was the option runner's
parallelism; the isolated importer also used a separately compiled,
exact-filter copy. Both supplemental probes need committed reproduction
harnesses.

The analyzers remove D-23's known fresh-user-dir diagnostic to compare
payloads, and normalize glib process IDs/times. Thus "same" below is
**conditional payload equality**, not a MATCH verdict under the
pre-registered diagnostic-inclusive rule. D-23 remains DIVERGENT for those
cases. The analyses never mask option words or candidate hashes.

### 15.2 Option words

The enumerated word sets contain 104 singles and 435 defined-bit pairs;
the corpora contain 496 and 149 inputs. The enumeration script is a
**reproduction harness to be committed**; all three cells gave the same conditional
block counts per cell:

| kind | same input payload | oracle abort | candidate | parse/key | sentence | total input blocks | identical preambles (excluded) |
|---|---:|---:|---:|---:|---:|---:|---:|
| single | 34,948 | 11,088 | 4,955 | 294 | 299 | 51,584 | 104 |
| pair | 43,497 | 14,988 | 5,779 | 0 | 551 | 64,815 | 435 |

The analyzer's `blocks()` emits a `<preamble>` before the first `==` input
header in **every** option-word file, even when its body is identical. The
earlier table counted that control block as `same payload`, adding one per
word. The `same input payload` column subtracts those 104/435 preambles;
each row now sums to its 104×496 or 435×149 input blocks. The parser's
control block begins with `options=...` before the first `== F0` input.

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

## 16. Completion run: item 5 (J internal candidate ledger)

This static pass ended at `2026-09-26T03:19:50Z`. It used the pinned
`074a2219` source and the audited `18d78208` subject source, with no build.
Clang 21 ran `-fsyntax-only -Xclang -ast-dump` in the existing pinned
container. The configured tkrzw header was preserved from the earlier oracle
build; scratch-only macro variants selected BDB and KC. Each cell parsed 24
of the 32 source translation units; the eight that failed were the other two
backends' storage units. Partial AST emitted before those failures was
excluded. Reproduction harness to be committed: extract function definitions
and call edges from Clang's AST for each configured backend, traverse from
the export roots, and compare candidate names with audited Rust definitions.
The name-level ledger is intermediate working data, not independent evidence
of counterpart equivalence.

Regeneration of each count is the checker invocation below; the AST reduction
recipe is `clang++ -std=c++17 -fsyntax-only -Xclang -ast-dump` over each
`find src -type f -name '*.cpp' | sort` translation unit, with `-I` for all
pin `src/` subdirectories, the cell's configured `config.h`, the pin-generated
public headers in `/opt/oracle/<cell>/include/libpinyin-2.11.92`, and
`pkg-config --cflags glib-2.0`. A nonzero Clang status excludes that TU's
entire partial stream. After committing the reducer, regenerate separately
for each backend with this procedure:

```sh
find src -type f -name '*.cpp' | sort
clang++ -std=c++17 -fsyntax-only -Xclang -ast-dump <configured flags> <TU>
# Reduce successful ASTs, traverse from all C exports, then review every
# unresolved or unmatched edge against both pinned source trees.
```

| cell | export roots found | reachable definition candidates | non-export candidates | unmapped by name | unresolved reference names |
|---|---:|---:|---:|---:|---:|
| tkrzw | 131/131 | 551 | 420 | 248 | 134 |
| bdb | 131/131 | 528 | 397 | 244 | 118 |
| kc | 131/131 | 546 | 415 | 247 | 132 |

**Source spot-check.** `pinyin.cpp:326-344` calls the internal
`check_format` at `:172`; both appear in every candidate ledger.
`pinyin.cpp:2670-2684` defines `pinyin_train` and its `get_result(index)`
path; the same-name Rust export at
`crates/oxpinyin-capi/src/candidates.rs:550` ignores `_index`, so a name
match is plainly not semantic mapping. `zhuyin.cpp:500-516` defines
`zhuyin_iterator_add_phrase`, with the same-name Rust entry at
`crates/oxpinyin-zhuyin-capi/src/iterators.rs:59`. The backend spot-check
found `ngram_bdb.cpp:127` only in BDB's ledger and
`ngram_kyotodb.cpp:124` only in KC's; neither appeared in the wrong cell
after failed-TU filtering. These checks substantiate candidate locations,
not counterpart equivalence.

| instrument | cell | injected deletion | detected | revert |
|---|---|---|---|---|
| J-m1 | tkrzw, bdb, kc | one `pinyin_init` export root | exit 1 on each | baseline exit 0; ledger byte-identical |
| J-m2 | tkrzw, bdb, kc | one reachable `src/pinyin.cpp:172:check_format` definition | exit 1 on each | baseline exit 0; ledger byte-identical |

The first J-m2 checker iteration was too broad: removing only the pinyin
definition left the zhuyin overload and escaped detection. The final checker
requires the exact source location; the table records only that rerun.

**Verdict: PARTIAL evidence, coverage NOT-ESTABLISHED on all three cells.**
This is a name-level overapproximation: overloaded methods may be conflated,
function pointers/virtual calls may be missed, and a Rust name match is only
a lead. None of the 248/244/247 unmapped rows has a verified deliberate-
absence reason. The 134/118/132 unresolved names include external calls but
have not been individually classified. Thus it does not meet the mandate's
every-transitively-reachable-function-to-counterpart criterion. Gap issues
#565, #566 and #567 remain open; the next pass needs USR/signature-resolved
call targets, indirect-edge closure, and reviewed counterpart or absence
citations for every internal row.

## 17. Completion run: item 6 (L, load-independent measurements)

Measured through `2026-09-26T03:38:12Z` in the pinned container
`sha256:8bf63f196e7a18e7f003adc37b148fc366c0f0ef9ae7c9cc2f6d4ab8e5fd4514`.
The cold-open C caller (reproduction harness to be committed) dlopened each
relinked library against the matching oracle data directory and a fresh,
empty user directory. Empty stdin exercises cold `pinyin_init`, option set,
instance allocation/free and `pinyin_fini`, with no candidate work. All
36 baseline runs exited 0 and had the same empty stdout digest. Callgrind
used `--cache-sim=no --branch-sim=no`; `Ir` came from `summary:`. Massif
used `--time-unit=i --heap=yes --stacks=no`; the maximum `mem_heap_B` is
peak live heap. The one-minute load average ranged 2.67-3.09 across these
traces. No wall-clock number is reported. Reproduction harness to be
committed: for each backend, run a C caller that dlopens each relinked
library, initializes with the same system directory and an empty user
directory, sets options, allocates and frees an instance, then finalizes.
Run each side three times under
`valgrind --tool=callgrind --cache-sim=no --branch-sim=no` and
`valgrind --tool=massif --time-unit=i --heap=yes --stacks=no`; record
`/proc/loadavg`, exit status and stdout for each run.
For each side and metric, the table gives the median of three runs. All
oracle repeats were exact; the largest subject `Ir` spread was 100
instructions in more than 330 million. Heap repeats were exact.

| cell | oracle Ir | subject Ir | ratio | oracle peak heap B | subject peak heap B | ratio |
|---|---:|---:|---:|---:|---:|---:|
| tkrzw | 22,295,138 | 334,313,247 | 14.995 | 9,449,311 | 24,556,688 | 2.599 |
| bdb | 12,343,767 | 330,749,412 | 26.795 | 319,591 | 23,683,414 | 74.105 |
| kc | 26,462,307 | 342,715,226 | 12.951 | 5,960,885 | 30,549,899 | 5.125 |

Massif's heap count excludes mappings. A diagnostic
`--pages-as-heap=yes` pass, separately load-recorded, still found larger
subject mapped-page peaks: tkrzw 97,140,736 vs 70,676,480 B; bdb
66,514,944 vs 36,298,752 B; kc 167,583,744 vs 73,846,784 B. These
figures are not substituted for the pre-registered heap metric.

**Source and asymptotics for L-01.** The pin's `pinyin.cpp:172-199`
checks/writes profile metadata, and `:326-405` opens tables and maps the
system phrase chunks (`:259-269`), then merges user logs. On a fresh empty
user directory, that path has no full system-item materialization pass.
The subject's `oxpinyin-runtime/src/lib.rs:941-1000` always calls
`oxpinyin_user::system_originals` before opening the user store. The latter
iterates every system item and every pronunciation, decoding text and
building `BTreeMap`s (`oxpinyin-user/src/persistence.rs:73-110`). This is
an extra O(N+P) work and O(N+P) transient heap pass for N system items
and P pronunciations, even when the profile is empty. `callgrind_annotate`
attributes 103,593,529 of the subject tkrzw cold-open Ir to the
`Runtime::open` closure, with 25,052,756 in `PhraseItemView::phrase_text`
and substantial allocator cost.
The measured cold-open scope exceeds 1.10 in both dimensions on each cell;
this is a distinct complexity finding, not a claim that every L path was
measured.

**Instrument falsifiers.** The preserved gated scratch build `5eb5c51`
has L-m1 at `oxpinyin-capi/src/sentence.rs:279-288` (an input-length-squared
busy loop) and L-m2 at `oxpinyin-capi/src/context.rs:52-57` (a touched,
retained 64 MiB allocation). Its parent `8567883` matches the audited
subject at both source sites. Reproduction harness to be committed: apply
each mutation to a temporary copy, collect baseline, mutated and reverted
Callgrind/Massif readings for each backend, and compare stdout. L-m1 increased
Callgrind Ir by 32,166,128 / 32,179,739 / 32,211,279 (tkrzw/bdb/kc);
after unsetting it Ir returned within 0.0021% of baseline. L-m2 raised
Massif peak heap by more than 64 MiB in each cell and returned exactly on
unset. Every phase exited 0, and stdout digests were identical within a
cell. Its per-run one-minute load range was 2.03-2.57.

The selected severity-2 cold-open repro was repeated once per cell against
prebuilt relinked `stage-main-*` objects from current `origin/main`
`34a66bc915c93a09a1680e70c5c2c252f95ffdfe` (the runtime, user
persistence and C init source files in `src-main` were byte-equal to that
commit). Current-main subject Ir was 334,313,153 / 330,749,226 /
342,716,300, and peak heap was 24,556,676 / 23,683,402 / 30,549,887 B
(tkrzw/bdb/kc): the finding still reproduces. The six recheck traces had
a one-minute load range of 1.90-2.23. Reproduction harness to be committed:
repeat the cold-open procedure above against the relinked libraries from
that main commit, using the same data directories.

**Overall L verdict: NOT-ESTABLISHED.** The complete structural-divergence
inventory is blocked by J's unresolved graph. The full section-12 input set,
200-input training and 10k-phrase import/export have not been measured with
these instruments. L-01 / #588 is a measured BOTH-WORSENED cold-open finding, but
no other internal path is marked MATCH and gap issues #553-#555 remain open.

## 18. Carried to round 3

These are the remaining axis-level and C-driver NOT-ESTABLISHED items. The
The C drivers have source-inspection commands here. For other rows, the next
reproduction step describes the missing harness or measurement.
This close-out did not run a new differential or mutation measurement.

| Axis / issues | Remaining work | Next reproduction step |
|---|---|---|
| B bdb / [#573](https://github.com/shenghaoc/oxpinyin/issues/573) (tracking; no dedicated gap issue) | The three nondeterministic oracle probes in §5 cannot be MATCH or confidently classified from the existing two runs. Pre-register any legitimate normalization, then repeat the B battery and its mutation/revert checks. | Reproduction harness to be committed: rerun the same BDB contract cases twice and compare only the affected API outputs and return values. |
| C `pred-order` / [#568](https://github.com/shenghaoc/oxpinyin/issues/568), [#569](https://github.com/shenghaoc/oxpinyin/issues/569), [#570](https://github.com/shenghaoc/oxpinyin/issues/570) | The driver selects the punctuation variant before the named `DRV-pred` path. Inspect its call path, then pre-register and test a new within-scope mutation in round 3. | `rg -n 'pinyin_guess_predicted_candidates' tools/bisection/pred-order-diff.c` |
| C `predict` / [#568](https://github.com/shenghaoc/oxpinyin/issues/568), [#569](https://github.com/shenghaoc/oxpinyin/issues/569), [#570](https://github.com/shenghaoc/oxpinyin/issues/570) | Its plain predicted list has at most one row, so the named reversal is invisible. Inspect its call path, then pre-register a non-vacuous corpus/mutation in round 3. | `rg -n 'pinyin_guess_predicted_candidates' tools/bisection/predict-diff.c` |
| C `punct` / [#568](https://github.com/shenghaoc/oxpinyin/issues/568), [#569](https://github.com/shenghaoc/oxpinyin/issues/569), [#570](https://github.com/shenghaoc/oxpinyin/issues/570) | The named `DRV-punct` removed an unprinted row; `DRV-punct2` was an impermissible substitute. Inspect the printed rows, then pre-register a non-vacuous mutation in round 3. | `rg -n 'pinyin_guess_predicted_candidates_with_punctuations' tools/bisection/punct-diff.c` |
| J / [#565](https://github.com/shenghaoc/oxpinyin/issues/565), [#566](https://github.com/shenghaoc/oxpinyin/issues/566), [#567](https://github.com/shenghaoc/oxpinyin/issues/567) | Resolve unmapped and indirect call edges, validate counterparts on all three cells; candidate counts alone do not establish internal coverage. | Reproduction harness to be committed: rerun the AST ledger of §16 with signature-resolved edges, then review every unmatched entry against both source trees. |
| L / [#553](https://github.com/shenghaoc/oxpinyin/issues/553), [#554](https://github.com/shenghaoc/oxpinyin/issues/554), [#555](https://github.com/shenghaoc/oxpinyin/issues/555) | Extend Callgrind/Massif beyond cold open to the full section-12 input set, 200-input training, and 10k-phrase import/export; finish the structural inventory after J. Record load with every run. | Reproduction harness to be committed: feed the committed §12 fixture through the §17 Callgrind/Massif procedure, then add training and import/export workloads. |
| G / [#556](https://github.com/shenghaoc/oxpinyin/issues/556), [#557](https://github.com/shenghaoc/oxpinyin/issues/557), [#558](https://github.com/shenghaoc/oxpinyin/issues/558), [#574](https://github.com/shenghaoc/oxpinyin/issues/574) | The gate is blind to G-m1 despite an observable output change. Wait for lane D's gate extension, then rerun G-m1 and its unset control on each cell. No G row becomes MATCH until the extended gate detects and reverts the mutation. | `gh pr list --repo shenghaoc/oxpinyin --state open --search 'gate' --json number,title,url` |

## 19. Findings index

This is the GitHub issue inventory in the #523-#588 number range, not a new
verdict. It preserves issue labels as filed; some gap issues retain a
  `not-established` label after later work and are not among the remaining
axis-level gaps in section 18. #559-#564 and #572 are **closable by human**
because their audit work finished; this report does not close them. Issue
numbers #578, #579, #580, #581 and #584
are not issues in this range. The last column records literal references by
open fix PRs, not a claim that a fix has landed.

| Issue | Axis | Issue verdict | Severity | Referenced by open fix PR |
|---|---|---|---|---|
| [#523](https://github.com/shenghaoc/oxpinyin/issues/523) | D | unregistered | 1 | [#584](https://github.com/shenghaoc/oxpinyin/pull/584), [#581](https://github.com/shenghaoc/oxpinyin/pull/581), [#579](https://github.com/shenghaoc/oxpinyin/pull/579), [#578](https://github.com/shenghaoc/oxpinyin/pull/578) |
| [#524](https://github.com/shenghaoc/oxpinyin/issues/524) | B | unregistered | 1 | — |
| [#525](https://github.com/shenghaoc/oxpinyin/issues/525) | F | unregistered | 1 | — |
| [#526](https://github.com/shenghaoc/oxpinyin/issues/526) | B | unregistered | 1 | — |
| [#527](https://github.com/shenghaoc/oxpinyin/issues/527) | C | unregistered | 1 | — |
| [#528](https://github.com/shenghaoc/oxpinyin/issues/528) | B | unregistered | 1 | — |
| [#529](https://github.com/shenghaoc/oxpinyin/issues/529) | B | unregistered | 1 | [#579](https://github.com/shenghaoc/oxpinyin/pull/579) |
| [#530](https://github.com/shenghaoc/oxpinyin/issues/530) | B | broader-than-registered | 1 | — |
| [#531](https://github.com/shenghaoc/oxpinyin/issues/531) | H | not-established | 1 | — |
| [#532](https://github.com/shenghaoc/oxpinyin/issues/532) | B | unregistered | 2 | — |
| [#533](https://github.com/shenghaoc/oxpinyin/issues/533) | B | unregistered | 2 | — |
| [#534](https://github.com/shenghaoc/oxpinyin/issues/534) | B | unregistered | 2 | — |
| [#535](https://github.com/shenghaoc/oxpinyin/issues/535) | G | broader-than-registered | 2 | — |
| [#536](https://github.com/shenghaoc/oxpinyin/issues/536) | I | unregistered | 2 | — |
| [#537](https://github.com/shenghaoc/oxpinyin/issues/537) | A | unregistered | 2 | — |
| [#538](https://github.com/shenghaoc/oxpinyin/issues/538) | H | unregistered | 2 | [#584](https://github.com/shenghaoc/oxpinyin/pull/584), [#581](https://github.com/shenghaoc/oxpinyin/pull/581), [#578](https://github.com/shenghaoc/oxpinyin/pull/578) |
| [#539](https://github.com/shenghaoc/oxpinyin/issues/539) | E | unregistered | 2 | — |
| [#540](https://github.com/shenghaoc/oxpinyin/issues/540) | B | unregistered | 2 | — |
| [#541](https://github.com/shenghaoc/oxpinyin/issues/541) | B | unregistered | 2 | — |
| [#542](https://github.com/shenghaoc/oxpinyin/issues/542) | B | unregistered | 2 | — |
| [#543](https://github.com/shenghaoc/oxpinyin/issues/543) | D | not-established | 2 | — |
| [#544](https://github.com/shenghaoc/oxpinyin/issues/544) | B | unregistered | 3 | [#584](https://github.com/shenghaoc/oxpinyin/pull/584), [#581](https://github.com/shenghaoc/oxpinyin/pull/581), [#579](https://github.com/shenghaoc/oxpinyin/pull/579), [#578](https://github.com/shenghaoc/oxpinyin/pull/578) |
| [#545](https://github.com/shenghaoc/oxpinyin/issues/545) | B | unregistered | 3 | — |
| [#546](https://github.com/shenghaoc/oxpinyin/issues/546) | H | not-established | 3 | — |
| [#547](https://github.com/shenghaoc/oxpinyin/issues/547) | A | not-established | 3 | — |
| [#548](https://github.com/shenghaoc/oxpinyin/issues/548) | K | register-integrity | 4 | — |
| [#549](https://github.com/shenghaoc/oxpinyin/issues/549) | K | register-integrity | 4 | — |
| [#550](https://github.com/shenghaoc/oxpinyin/issues/550) | K | register-integrity | 4 | — |
| [#551](https://github.com/shenghaoc/oxpinyin/issues/551) | K | register-integrity | 4 | — |
| [#552](https://github.com/shenghaoc/oxpinyin/issues/552) | K | register-integrity | 4 | — |
| [#553](https://github.com/shenghaoc/oxpinyin/issues/553) | L | not-established | — | — |
| [#554](https://github.com/shenghaoc/oxpinyin/issues/554) | L | not-established | — | — |
| [#555](https://github.com/shenghaoc/oxpinyin/issues/555) | L | not-established | — | — |
| [#556](https://github.com/shenghaoc/oxpinyin/issues/556) | G | not-established | — | — |
| [#557](https://github.com/shenghaoc/oxpinyin/issues/557) | G | not-established | — | — |
| [#558](https://github.com/shenghaoc/oxpinyin/issues/558) | G | not-established | — | — |
| [#559](https://github.com/shenghaoc/oxpinyin/issues/559) | C | not-established; closable by human | — | — |
| [#560](https://github.com/shenghaoc/oxpinyin/issues/560) | C | not-established; closable by human | — | — |
| [#561](https://github.com/shenghaoc/oxpinyin/issues/561) | C | not-established; closable by human | — | — |
| [#562](https://github.com/shenghaoc/oxpinyin/issues/562) | B | not-established; closable by human | — | — |
| [#563](https://github.com/shenghaoc/oxpinyin/issues/563) | B | not-established; closable by human | — | — |
| [#564](https://github.com/shenghaoc/oxpinyin/issues/564) | B | not-established; closable by human | — | — |
| [#565](https://github.com/shenghaoc/oxpinyin/issues/565) | J | not-established | — | — |
| [#566](https://github.com/shenghaoc/oxpinyin/issues/566) | J | not-established | — | — |
| [#567](https://github.com/shenghaoc/oxpinyin/issues/567) | J | not-established | — | — |
| [#568](https://github.com/shenghaoc/oxpinyin/issues/568) | C | not-established | — | — |
| [#569](https://github.com/shenghaoc/oxpinyin/issues/569) | C | not-established | — | — |
| [#570](https://github.com/shenghaoc/oxpinyin/issues/570) | C | not-established | — | — |
| [#571](https://github.com/shenghaoc/oxpinyin/issues/571) | F | not-established | — | — |
| [#572](https://github.com/shenghaoc/oxpinyin/issues/572) | K | not-established; closable by human | — | — |
| [#573](https://github.com/shenghaoc/oxpinyin/issues/573) | — | tracking | — | — |
| [#574](https://github.com/shenghaoc/oxpinyin/issues/574) | G | register-integrity | 3 | — |
| [#575](https://github.com/shenghaoc/oxpinyin/issues/575) | B | unregistered | 2 | — |
| [#576](https://github.com/shenghaoc/oxpinyin/issues/576) | B | unregistered | 2 | — |
| [#577](https://github.com/shenghaoc/oxpinyin/issues/577) | B | unregistered | 2 | — |
| [#582](https://github.com/shenghaoc/oxpinyin/issues/582) | C | unregistered | 2 | — |
| [#583](https://github.com/shenghaoc/oxpinyin/issues/583) | D | unregistered | 3 | [#584](https://github.com/shenghaoc/oxpinyin/pull/584) |
| [#585](https://github.com/shenghaoc/oxpinyin/issues/585) | C | unregistered | 2 | — |
| [#586](https://github.com/shenghaoc/oxpinyin/issues/586) | C | unregistered | 2 | — |
| [#587](https://github.com/shenghaoc/oxpinyin/issues/587) | C | unregistered | 3 | — |
| [#588](https://github.com/shenghaoc/oxpinyin/issues/588) | L | unregistered | 2 | — |
