# Appendix A — tooling inventory records (origin/main e67f19d1, inspected 2026-10-16 UTC; Stage 0 record of the ruling of 2026-10-16 UTC)

> **Historical inventory snapshot — not active policy.** This record reflects
> the named tree. The `Assisted-by` commit-trailer policy and its linter, hook,
> test harness, and CI jobs were later retired; their references here are
> archival and do not describe current requirements.


Record format: one table per unit with the ten required fields; IDs are stable. Produced by nine full-file reading passes over a detached worktree; counts in the main report were re-verified directly. Three units appear twice (TLS-36≡CI-27, TLS-37≡CI-28, ORC-18≡CI-11).


---

# Inventory BIS-A — tools/bisection/ basenames a–l (non-Python, excluding cell-artifacts.sh and contract-diff)

Tree read: the detached worktree at origin/main (e67f19d1), every file below read in full with `cat`. Directory listing confirmed the letter range; `coverage_review_rules/` holds only `.py` files and is excluded by the assignment. `cg-calls.py`, `clang_identity.py`, `compare-nbest-training-state.py`, `compiler_cells.py`, `contract-diff.py`, `coverage_reviews.py`, `coverage_tiers.py`, `delta_ledger.py`, `finish_ledger.py`, `gcov_measure.py`, `internal-ledger*.py` are Python and excluded. `cell-artifacts.sh` excluded by assignment.

Common shape shared by every C driver below unless the record says otherwise: `dlopen(argv[1], RTLD_NOW)` of one shared object, `dlsym` of each `pinyin_*` symbol with an exit(1) + `MISSING: <name>` on stderr when absent, prints a text log to stdout, exits 0 on a completed walk. No driver compares in-process; every differential is "run the same binary twice, one per .so, diff the two logs" done by the wrapper. No driver carries hand-written expected values except where noted (run-bisect.sh's right-tail contract and the probe-presence anchors in the wrappers).

### BIS-A-01 — tools/bisection/abi-probe-diff.c

| Field | Value |
|---|---|
| Entry point | `./abi-probe-diff <so> <systemdir> <sort-hex>`; compiled and executed by run-abi-probe-diff.sh |
| Owning subsystem | bisection (abi whole-surface probe) |
| Purpose | Drives all 79 exported `pinyin_*` symbols through one deterministic sequence and prints one `label=value` row per observation (return status, out-params, lengths, handle state) so a runner can diff pin vs oxpinyin; a completed run also proves the export surface is present (compatibility-policy §(e)). |
| Implementation | C (gnu11); 1089 lines; `dlfcn` only — `g_free`/`g_strfreev`/`g_array_new`/`g_array_free` are resolved by `dlopen("libglib-2.0.so.0")` at runtime and are fatal if absent (no glib link, no libpinyin headers; opaque `typedef void` handles, hand-declared fn-pointer typedefs). |
| Inputs | argv: so path, system dir, sort word (hex, strictly validated by `strtoul` end-pointer). No env vars. Writes a `mkdtemp("/tmp/unionprobe-user-XXXXXX")` user dir. Needs `/tmp`. |
| Outputs | stdout: `sort=0x..`, `resolved=79`, `=== phase: setup/import/full/longer-choose/choose-train/predict/double/chewing/export/abi-extras/teardown ===` sections, rows like `set_options(0x18a)=true`, `full(nihaoshijie): consumed=.. parsed=..`, `tag:cand[i]=true type=.. text=.. nbest=asked/true/N is_user=..`, `phrase[n]=p|py|c`, `bigram[n]=..`. stderr: `MISSING:`/`dlopen:` diagnostics. Exit 0 = walk completed; 1 = missing symbol, bad sort word, NULL context/instance, extras-context init failed. Divergence is never this program's verdict. |
| Platform | Linux-only (dlopen, `/tmp`, `mkdtemp`, glib soname `libglib-2.0.so.0`) |
| Consumers | run-abi-probe-diff.sh (builds it: `gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o abi-probe-diff abi-probe-diff.c -ldl`); runners.json[0] `abi-probe-diff` via that wrapper; .gitignore lists the built binary; cited by docs/findings/probe-coverage-abi.md, revert-plan.md, tkrzw-distro-compat.md, crates/oxpinyin-runtime/examples/nbest_tail_probe.rs. |
| State | One mkdtemp user dir under /tmp, removed by its own `rm_rf` (unlink regular files + rmdir) on every exit path except the missing-symbol/bad-arg paths before mkdtemp. Second context (`ctx2`) opened on the same user dir for the abi-extras phase. No locks, no timeout. |
| Overlap | `rm_rf`, `resolve_g_free` pattern, `dump_rows`, PARITY_WORD 0x18a, DEFAULT sort 0x1e duplicated across bisect.c, live-typing-diff.c, candidate-assembly-diff.c, dynamic-adjust-diff.c; the abi-extras phase explicitly re-drives "key-surface-diff.c's" and "dict-surface-diff.c's smoke-proved-safe shapes"; comments reference uncovered-surface-diff.c's phase-D cursor set and train-diff.c's `TRAINDIFF_MASK=user`. |
| Constraints | Oracle caller contracts hard-coded: `pinyin_get_sentence` only on proved indices; predicted rows never go to `choose_candidate`; double scheme 30 / zhuyin 7 never set (99/-1 used for out-of-enum); `unload_addon`/`unload_lib` out-of-range never sent (pin asserts pinyin.cpp:499/:466); load_lib only indexes {1,2,4,7} (pinyin.cpp:457 assert); left/right offsets only at cursors {0,2,5,8} of "nihaoshijie" (pin asserts pinyin.cpp:3055/:3090); remove_user only behind a true is_user (pin asserts :3734/:3738). Symbol count 79 is hard-coded (`resolved=79`). ChewingKey read as exactly 2 bytes (chewing_key.h:41-48). |

### BIS-A-02 — tools/bisection/run-abi-probe-diff.sh

| Field | Value |
|---|---|
| Entry point | `tools/bisection/run-abi-probe-diff.sh` (no args); executed |
| Owning subsystem | bisection |
| Purpose | Builds abi-probe-diff, builds/resolves libpinyin_capi.so for the cell, runs the driver against the capi and the pin oracle, diffs the two logs. |
| Implementation | bash (set -euo pipefail); 112 lines; gcc, diff, mktemp; sources system-dir.sh and oracle-cell.sh. |
| Inputs | Env: `PINYIN_ORACLE_PREFIX` (default `$HOME/.local/opt/pinyin-oracle`), `ABI_PROBE_SORT` (hex, default `1e`), `ABI_PROBE_SYSTEM` then `OXPINYIN_SYSTEM_DIR` (via `resolve_system_dir ABI_PROBE_SYSTEM abi-probe`), plus everything oracle-cell.sh reads (`PINYIN_ORACLE_DBM`, `CARGO_TARGET_DIR`, `OXPINYIN_TARGET_ROOT`, `OXPINYIN_CAPI_SO`, `OXPINYIN_ZHUYIN_SO`) and system-dir.sh reads (`OXPINYIN_CAPI_BACKEND_EXT`, `OXPINYIN_ALLOW_MINI_FIXTURE`). Files: `$PREFIX/oracle-pin.txt`, `$PREFIX/lib/libpinyin.so`, `$PREFIX/lib/libpinyin/data/bigram.db`. |
| Outputs | stdout progress (`--- building ... ---`, `capi: path`, `sort word: 0x1e`, `oxpinyin-capi: ok (N log lines)`, `abi-probe-diff: IDENTICAL` or `DIVERGENCE` + `diff -u`). Exit 0 identical; 77 SKIP (no oracle, off-pin, no bigram.db); 1 build/run failure (driver crashed; dumps stdout and stderr captures); 2 divergence. |
| Platform | Linux-only (gcc -ldl, pin oracle) |
| Consumers | runners.json[0] (`kind: gate`, cells bdb/kc/tkrzw, exclusive_resources `bisection-source-output`, input_paths `{capi}`); docs/findings/probe-coverage-abi.md. |
| State | Compiles `abi-probe-diff` into tools/bisection/ itself (source tree write — hence the `bisection-source-output` exclusive resource). Four `mktemp` log files removed on each path. Spawns cargo through `oracle_cell_artifact` unless `OXPINYIN_CAPI_SO` is set. No timeout. |
| Overlap | Same skeleton as run-live-typing-diff.sh, run-dynamic-adjust-diff.sh, run-import-diff.sh, run-scheme-diff.sh (cd dirname, source both helpers, gcc into source dir, `oracle_cell_artifact`, PREFIX/oracle-pin.txt + `grep -Fxq "$EXPECTED_PIN_REF"`, bigram.db check, two mktemp logs, diff -u). |
| Constraints | Pin check: exact whole-line `grep -Fxq "$EXPECTED_PIN_REF" oracle-pin.txt` (ref built by oracle-cell.sh from build-oracle.sh constants + `+dbm-<cell>`). Unresolvable system dir is FATAL via system-dir.sh (exit 3), never a mini-fixture run. interpolation2.text explicitly NOT required (comment: since P6 the runtime reads unigrams from chunk files). |

### BIS-A-03 — tools/bisection/addon-candidate-diff.c

| Field | Value |
|---|---|
| Entry point | `./addon-candidate-diff <so> <systemdir>`; compiled by run-w11-diff.sh (via run-addon-candidate-diff.sh) and by run-same-data-dir-diff.sh |
| Owning subsystem | bisection |
| Purpose | Loads addon library 4 (art) and prints the ADDON_CANDIDATE rows for "erhuang", so the W11 addon surface can be diffed between the capi (public-ABI addon_4_* tables) and the pin (art.bin). |
| Implementation | C (gnu11); 104 lines; dlfcn only. |
| Inputs | argv so + systemdir; no env; `mkdtemp("/tmp/addoncand-XXXXXX")` user dir. Prefers `oxpinyin_init_for_fixtures` when exported, else `pinyin_init`. |
| Outputs | stdout: `load_addon(4): true/false`, `load_addon(4) again: ...`, `addon: <text>` per type-6 candidate. Exit 0; 1 on usage/dlopen/mkdtemp/init failure. |
| Platform | Linux-only |
| Consumers | run-addon-candidate-diff.sh → run-w11-diff.sh (`gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o $WORK/addon-candidate-diff addon-candidate-diff.c -ldl`); run-same-data-dir-diff.sh default driver list; runners.json[1]; .gitignore; docs/runbooks/parity-gates.md, docs/findings/probe-coverage-abi.md. |
| State | Leaks its mkdtemp user dir (never removed). Sort word 0x1e, guess at offset 0. |
| Overlap | `must()` helper and fixture-init preference duplicated in dict-surface-diff.c, key-surface-diff.c, import-diff.c, fullpin-diff.c, chewing-diff.c, bisect.c. |
| Constraints | ADDON_CANDIDATE hard-coded as 6; addon index 4 hard-coded; no expected values. |

### BIS-A-04 — tools/bisection/run-addon-candidate-diff.sh

| Field | Value |
|---|---|
| Entry point | `tools/bisection/run-addon-candidate-diff.sh [prebuilt-library] [driver-arg ...]`; `exec`s `./run-w11-diff.sh addon-candidate-diff "$@"` |
| Owning subsystem | bisection |
| Purpose | Thin alias: the W11 addon-candidate differential through the shared W11 runner. |
| Implementation | bash; 9 lines. |
| Inputs | Everything run-w11-diff.sh reads: `PINYIN_ORACLE_PREFIX`, `CAPI_W11_SYSTEM_DIR` (note `_DIR` suffix) / `OXPINYIN_SYSTEM_DIR`, `OXPINYIN_CAPI_SO`, oracle-cell.sh + system-dir.sh vars. |
| Outputs | run-w11-diff.sh's: exit 0 identical; 77 no oracle; 1 off-pin (FAIL, not SKIP) / build / crash; 2 divergence; stdout prints both logs then `addon-candidate-diff: IDENTICAL` or diff. |
| Platform | Linux-only |
| Consumers | runners.json[1] (`gate`, three cells, exclusive `bisection-source-output`, required input "committed fixtures/w3 native cell tables"). |
| State | Delegated: run-w11-diff.sh builds the driver into a mktemp dir, captures stdout in memory (`capture_output`), writes nothing to the tree. |
| Overlap | run-user-candidate-diff.sh and run-predict-diff.sh are the same alias shape over run-w11-diff.sh. |
| Constraints | Header says "Does not edit run-import-diff.sh or run-train-diff.sh". Off-pin is FAIL (exit 1) in run-w11-diff.sh, unlike the SKIP 77 most other wrappers use. |

### BIS-A-05 — tools/bisection/axis-c-replacement.json

| Field | Value |
|---|---|
| Entry point | Data; loaded by contract-diff.py (`json.loads(root/'tools/bisection/axis-c-replacement.json')`, asserts `format == 'axis-c-replacement-v1'`, optionally re-hashes every `sources` entry) |
| Owning subsystem | bisection (contract-diff axis C definition) |
| Purpose | Freezes the axis-C (options / encoding / layouts) replacement corpus for contract-diff.py: which sentence-surface rows, option words, encoding inputs and keyboard-layout sweeps are driven, and the two temporary mutations (C-m1-r, C-m3-r) that must be detected. |
| Implementation | JSON; 966 lines; keys: `format`, `created_utc` (2026-10-12T04:56:11Z), `subject_base` (commit 0338cd12…), `sources` (10 path→sha256: fixtures/w4/oracle-sentence-surface.txt + tests/parity/corpus/inputs/01..09), `sentence_rows` 507, `selected` 17 `{source,row,text}`, `option_inputs` 16, `defined_bits` 30, `words` {single 30, pair 435, boundary 10} hex strings, `encoding_inputs` 23 `{name,hex}`, `encoding_operations` 14 `[facade, op]` pairs, `layout_schemes` 15 (incl. 7, 10, 11, -1, INT_MIN, INT_MAX), `layout_words` 3, `layout_inputs` 16, `layout_ascii` 95, `layout_covered` 2 (rows 5c/5d: pin aborts, subject refuses), `mutations` {C-m1-r → crates/oxpinyin-facade/src/context.rs USE_TONE shadow; C-m3-r → crates/oxpinyin-core/src/scheme.rs STANDARD '1'→ㄆ}. |
| Inputs | None at rest; contract-diff.py verifies `sources` digests against the tree. |
| Outputs | n/a |
| Platform | portable |
| Consumers | tools/bisection/contract-diff.py (`--axis-c-replacement options|encoding|layouts|all`, `--axis-c-mutation C-m1-r|C-m3-r`, `--axis-c-snapshot-check`); docs/findings/bug-for-bug-audit-r2-2026-09-23.md. |
| State | none |
| Overlap | Sentence-surface row selection overlaps fixtures/w4/oracle-sentence-surface.txt and the parity corpus files it hashes. |
| Constraints | Frozen by format tag + source digests: editing any hashed fixture invalidates it (contract-diff.py asserts). Mutation definitions are described as "Ungated, temporary, never committed". |

### BIS-A-06 — tools/bisection/bigram-export-diff.c

| Field | Value |
|---|---|
| Entry point | `./bigram-export-diff <so> <systemdir> empty|one|two|many|repeat|reopen|mask`; compiled by run-bigram-export-diff.sh |
| Owning subsystem | bisection |
| Purpose | Trains one scenario through the public ABI (parse, choose named NORMAL candidates, `pinyin_train`), then runs ONE user-bigram export and prints the walk in iteration order including the row-36 law (`get=` is has_next-after-row) and the ibus-style has_next-after-false tail (issue #541). |
| Implementation | C (gnu11); 255 lines; dlfcn + glib headers (`<glib.h>`, `g_array_new/g_array_index/g_array_free` linked via pkg-config in the wrapper); `g_free` resolved from `libglib-2.0.so.0` with `free` fallback. |
| Inputs | argv; no env. Creates `./user` (mkdir 0700) relative to cwd — the wrapper runs each side in its own mktemp cwd. Symbols: pinyin_init/fini/alloc/free_instance/parse_more_full_pinyins/guess_sentence/guess_candidates/get_n_candidate/get_candidate/get_candidate_string/get_candidate_type/choose_candidate/train/reset/begin_get_bigram_phrases/bigram_iterator_has_next_phrase/bigram_iterator_get_next_phrase/end_get_bigram_phrases; mask scenario adds lookup_tokens + mask_out; reopen adds save. |
| Outputs | stdout (unbuffered): `train <entry>: true`, optional `mask 0x..: true`, `save: true`, `has_next0 <bool>`, `row <phrase>\t<pinyin>\t<count>\tget=<bool>`, `rows=<n>`, `has_next_after_false <bool>`, `tail ...`. Exit 0; 2 usage; 1 dlopen/init/parse/train/begin failure. |
| Platform | Linux-only |
| Consumers | run-bigram-export-diff.sh (`gcc ... bigram-export-diff.c -ldl $(pkg-config --cflags --libs glib-2.0)`); runners.json[2] (oracle_variant `bigram-export-strjoinv`); patches/bigram-export-strjoinv/pin-bigram-export-strjoinv.patch header; docs/runbooks/parity-gates.md, upstream-divergences.md, compatibility-policy.md, abi-allocator-pairing.md, crates/oxpinyin-capi/libpinyin.alloc. |
| State | `./user` dir in cwd, never removed by the driver; row loops bounded at 10000/20000. |
| Overlap | Choose-by-text loop (skip NBEST type 1) duplicated in live-typing-diff.c `find_phrase_by_text`, dynamic-adjust-diff.c, candidate-assembly-diff.c; export loop shape shared with export-diff.c and live-typing-diff.c Phase D. |
| Constraints | Must run against a pin prefix built with patches/bigram-export-strjoinv (unpatched pin over-reads pinyin.cpp:843-849, class (b)); one export per context (pin iterator keeps state); training deliberately never chooses the n-best row (#603). Mask constant 0x0FFFFFFF and sort 0x1e hard-coded. |

### BIS-A-07 — tools/bisection/run-bigram-export-diff.sh

| Field | Value |
|---|---|
| Entry point | `run-bigram-export-diff.sh <libpinyin.so> <libpinyin_capi.so> <data-dir>` ($2 may fall back to `OXPINYIN_CAPI_SO`) |
| Owning subsystem | bisection |
| Purpose | Runs bigram-export-diff.c over seven scenarios on both libraries against ONE data dir, checks the row-36 returns law per log with awk, then diffs byte for byte (gate `full`) or only where the walk has one predecessor (gate `returns`). |
| Implementation | bash; 119 lines; gcc + pkg-config glib-2.0, awk, diff; sources oracle-cell.sh (needs REPO_ROOT). |
| Inputs | argv 3; env `BIGRAM_DIFF_GATE` (full|returns), `OXPINYIN_CAPI_SO`, oracle-cell.sh vars. Requires `<prefix>/oracle-patches.sha256` to mention `pin-bigram-export-strjoinv.patch`. |
| Outputs | per scenario `IDENTICAL (n rows)` / `DIVERGENCE` / `DIVERGENCE (row 36: pin|oxpinyin breaks the law)` / `returns law holds ...; walk differs ... (not gated)` + `head -40` of diff; final `bigram-export-diff (<gate>): PASS|DIVERGENCE|FAILURE`. Exit 0 pass; 77 missing oracle .so; 1 unpatched prefix / driver nonzero; 2 divergence. |
| Platform | Linux-only |
| Consumers | runners.json[2] (`gate`, command `{prefix}/lib/libpinyin.so {capi} {prefix}/lib/libpinyin/data`, `oracle_variant: bigram-export-strjoinv`); runners.json[27] also carries that variant; docs/findings/tooling-lane-gates.md. |
| State | One mktemp work dir (driver binary, per-side-per-scenario cwd dirs, logs, diffs), `trap rm -rf` on EXIT. Driver run with `LD_LIBRARY_PATH=$(dirname so)`, stderr discarded. No cargo spawn (prebuilt .so required). |
| Overlap | Same argv-3 skeleton (resolve to absolute, cd dirname, mktemp work, `run side so`) as run-export-diff.sh, run-import-semantics-diff.sh, run-same-data-dir-diff.sh. |
| Constraints | Pin-manifest check is the patch marker in `oracle-patches.sha256`, NOT oracle-pin.txt (no `EXPECTED_PIN_REF` grep despite sourcing oracle-cell.sh). Gate table is in-script (`returns` gates byte identity only for empty/one). |

### BIS-A-08 — tools/bisection/bisect.c

| Field | Value |
|---|---|
| Entry point | `./bisect <so> <systemdir> [--right-tail-case]` (surface mode) or `./bisect --perf <so> <systemdir>` (perf JSON mode) |
| Owning subsystem | bisection + perf (the harness both the differential and every perf/callgrind recipe drive) |
| Purpose | Surface mode: resolves 51 of 79 `pinyin_*` symbols, drives 8 inputs through the keystroke cycle and probes the remaining symbol groups, printing a deterministic text log for diffing. Perf mode: times init/alloc and N keystroke cycles over the frozen 20-input corpus, reports RSS/smaps/mallinfo2/alloc-counter/hardware-instruction figures as one JSON line. |
| Implementation | C (gnu11, `_DEFAULT_SOURCE` for malloc_trim/mallinfo2/malloc_info); 1588 lines; dlfcn, `<linux/perf_event.h>` + `syscall(SYS_perf_event_open)`, `/proc/self/status|smaps_rollup|maps|smaps`, `system("rm -rf -- ...")` for user-dir cleanup, `g_free`/`g_strfreev` from `libglib-2.0.so.0` with `free` fallback. Exports two `noinline` externally visible anchors `bisect_perf_cold_unit` / `bisect_perf_steady_unit` for callgrind `--toggle-collect`. |
| Inputs | argv. Env (perf mode only): `PERF_BACKEND` (label), `PERF_MODE` (speed|ram-init|ram-cycle|rss-diag), `PERF_CYCLES` (default 8, 1..100000), `PERF_REPEATS` (default 1), `PERF_HW` (any value → open HW instructions counter), `RSS_DIAG_DIR`, `RSS_DIAG_TAG`. Optional dlsym of `oxpinyin_alloc_count/_bytes/_live_bytes/_peak_live_bytes/_reset_peak` (alloc-count feature) and `oxpinyin_init_for_fixtures` (preferred over pinyin_init in surface mode only). User dirs: `mkdtemp(/tmp/bisect-user-XXXXXX)` / `/tmp/bisect-perf-XXXXXX`. |
| Outputs | Surface: `# bisection fixture` header (so/systemdir/userdir/flags 0x18a/sort 0x1e), `=== dlopen/resolve/init ===`, per input `=== input: "x" ===` with parse/guess/get_sentence/aux/candidates (≤10, `type=NAME nbest=N text=".."`)/character_offset/pinyin_offset/left/right/choose/train/reset rows, `=== extra_symbols ===`, `=== teardown ===`. `--right-tail-case`: prints `right-tail input=nihao offset=5 flags=0x0000018a` then either `right-tail: false; output untouched` or exit(2) via stderr. Perf: one JSON object `{"backend","so","systemdir","mode","repeats","init_ns","alloc_ns","after_init":{rss_kib,hwm_kib,vm_size_kib,rss_anon_kib,rss_file_kib},"cycles_ns":[..],"after_first","after_last",[rss-diag: rollup_*/arena_*/alloc_*/trim_released/after_trim],[steady_alloc_cycles/count/bytes],[steady_hw_cycles/steady_hw_instructions | hw_errno]}`. Exit 0; 1 usage/dlopen/missing symbols/NULL ctx or instance; 2 right-tail contract violated. |
| Platform | Linux-only (perf_event_open, /proc, mallinfo2, /tmp) |
| Consumers | Built by run-bisect.sh (`-o bisect` in source dir), Dockerfile.perf-matrix and Dockerfile.callgrind (`-o /repo/tools/bisection/bisect`, callgrind adds `-g` and asserts the two anchors have distinct addresses); driven by run-bisect.sh, run-callgrind-differential.sh (`--perf` with PERF_HW / callgrind), run-perf-baseline.sh, run-perf-same-data.sh, run-rss-diag.sh, run-shipped-check.sh, run-tree-recipe-control.sh, tools/perf-gate/snapshot.sh, tools/profile/run-w8-cycle.sh; referenced by abi-probe-diff.c comments, crates/oxpinyin-capi/src/alloc_count.rs, docs/testing/oracle-bisect-differential-abort.md and many perf findings. runners.json[3] `bisect`. Not in run-same-data-dir-diff.sh's default set (its offset sweep trips pin `_check_offset` asserts). |
| State | mkdtemp user dir per run; surface mode only `rmdir`s it (non-empty after save → leaks contents); perf mode `system("rm -rf")`. rss-diag writes `<RSS_DIAG_DIR>/<tag>-{maps,smaps,smaps-rollup}-{init,cycle,trim}.txt` and `<tag>-mallocinfo.xml`. Hardware counter fd opened/enabled/disabled/reset per steady cycle. |
| Overlap | PERF_CYCLE_INPUTS duplicated from crates/pinyin-oracle/benches/support/mod.rs (comment); DEFAULT_FLAGS 0x18a duplicated from tools/capture/capture.c; candidate-type enum/`ctype_name`, `resolve_g_free`, `drive_input` shape duplicated in chewing-diff.c and fullpin-diff.c; abi-probe-diff.c re-covers the 51 + the 28 remaining symbols. |
| Constraints | Symbol count 51 hard-coded in `resolve_all` printout; anchors must stay distinct non-inlined externs (Dockerfile.callgrind fails the build otherwise); `right-tail-case` expects pin SIGABRT at pinyin.cpp:3092 (run-bisect.sh asserts it); `perf_remove_user_dir` binds `system()` status because of `_FORTIFY_SOURCE` warn_unused_result at -Werror. Perf JSON shape is consumed by perf-ci.py/perf-baseline.py/rss-smaps.py. |

### BIS-A-09 — tools/bisection/run-bisect.sh

| Field | Value |
|---|---|
| Entry point | `./run-bisect.sh` (capi-only) or `./run-bisect.sh <libpinyin.so> <data>` (adds the oracle right-tail contract) |
| Owning subsystem | bisection |
| Purpose | Builds bisect and the capi, runs run-cpp-smoke.sh, runs bisect against the committed W3 fixture, optionally re-runs under valgrind, optionally LD_PRELOADs the ibus engine, and (with args) checks the registered right-tail abort contract on both libraries. |
| Implementation | bash; 238 lines; gcc, valgrind (optional), `LD_DEBUG=bindings`, sleep/kill; sources oracle-cell.sh. |
| Inputs | argv 0 or 2; env `OXPINYIN_CAPI_BACKEND_EXT` (kct|tkt|db; else picks first of `fixtures/w3/{db,tkt,kct}`), `BISECT_LD_PRELOAD=1`, oracle-cell.sh vars (`PINYIN_ORACLE_DBM`, `OXPINYIN_CAPI_SO`, `CARGO_TARGET_DIR`...). Data: `$REPO_ROOT/fixtures/w3/<ext>`. Probes `/usr/libexec/ibus-engine-libpinyin`, `/usr/lib/ibus/ibus-engine-libpinyin`. |
| Outputs | stdout: build/capi/data lines, run-cpp-smoke output, full capi bisect log (`cat`), valgrind `ERROR SUMMARY`/`definitely lost` lines or `SKIP: valgrind not found`, ld-preload `symbols bound from capi: N` or SKIP, right-tail `oracle: expected SIGABRT, pinyin.cpp:3092 ...` / `capi: expected false, output untouched (compatibility register row 14)`, final `bisection: PASS`. Exit 0; 1 build/run failure or unexpected right-tail observation; 2 (documented, never produced in current code — no log diff is performed); 3 valgrind errors; 4 LD_PRELOAD failure. |
| Platform | Linux-only |
| Consumers | runners.json[3] (`gate`, command with `{prefix}/lib/libpinyin.so {prefix}/lib/libpinyin/data`, `optional_skips` for ld-preload/valgrind/ibus); docs/testing/oracle-bisect-differential-abort.md, redb-removal-audit, probe-coverage-abi. |
| State | Writes `tools/bisection/bisect` into the source tree; mktemp logs; background ibus engine killed after `sleep 2`. Spawns cargo via `oracle_cell_artifact` and again inside run-cpp-smoke.sh. |
| Overlap | Backend-ext → fixtures/w3 selection block copied verbatim from run-cpp-smoke.sh (both say "db first since 2026-09-20"); stderr-separation rationale duplicates run-pred-order-diff.sh's. |
| Constraints | Hand-written expected values: oracle exit 134, log line `right-tail input=nihao offset=5 flags=0x0000018a`, stderr regex `pinyin.cpp:3092:.*pinyin_get_right_pinyin_offset.*Assertion.*_check_offset\(matrix, right\).*failed`; capi line `right-tail: false; output untouched`. No oracle-pin.txt check at all. Comment: "this is not a claim of full oracle byte parity". |

### BIS-A-10 — tools/bisection/candidate-assembly-diff.c

| Field | Value |
|---|---|
| Entry point | `./candidate-assembly-diff pinyin <libpinyin.so> <systemdir>` or `... zhuyin <libzhuyin.so> <systemdir>`; compiled by run-candidate-assembly-diff.sh and run-nbest-choose-train-diff.sh |
| Owning subsystem | bisection |
| Purpose | Prints every row of every candidate list it asks for (index, type, string) across cases A/N/C/M/S/T/K/U/R/F plus toned-import cases, on both the libpinyin and libzhuyin facades, so the candidate-list assembly path (issue #582, register rows 34/37, #602, #639, #645, #640) can be diffed case by case. With `CAND_ASSEMBLY_TRAIN_DIR` set it instead chooses every n-best row on a fresh context, trains, saves and snapshots each user dir (lane I, #603/#527). |
| Implementation | C (gnu11); 924 lines; dlfcn with `RTLD_NOW|RTLD_GLOBAL`; `g_free` via `dlsym(RTLD_DEFAULT)` then from the library; symbol names built as `<pinyin|zhuyin>_<suffix>` (`must_prefixed`). |
| Inputs | argv 3; env `TMPDIR` (user dirs `cand-assembly-XXXXXX`), `CAND_ASSEMBLY_TRAIN_DIR` (train-rows mode root), `CAND_ASSEMBLY_BASELINE_ONLY` (skips the #640 leading-apostrophe cases); compile-time `ZHUYIN_READING` override. Symbols: init/fini/set_options/alloc_instance/free_instance/guess_sentence/get_n_candidate/get_candidate/get_candidate_type/get_candidate_string/choose_candidate/save/begin_add_phrases/iterator_add_phrase/end_add_phrases; pinyin adds train/get_candidate_nbest_index/parse_more_full_pinyins/get_sentence/guess_candidates/set_full_pinyin_scheme/set_double_pinyin_scheme/parse_more_double_pinyins; zhuyin adds zhuyin_train/parse_more_chewings/zhuyin_get_sentence/guess_candidates_after_cursor. |
| Outputs | stdout (line-buffered): `== <CASE> word=0x.. import=.. input=..[ scheme=..][ imports=..]` headers; `set_options(0x..)=true`, `import p/r/f=true`, `parse=N`, `guess_sentence=..`, `SENTENCE <label> ret=.. <text>`, `LIST <label> offset=N ret=.. n=N`, `  i type string` rows, `CHOOSE <label> [offset=N] row=N <str> cursor=N`; train mode `== T fixture=.. ordinal=.. input=..` + `reguess=.. train=.. save=..`; stderr `coverage fixture=.. ordinal=.. nbest-index=..`. Exit 0; 1 setup/import/parse failure (several `exit(1)` sites). |
| Platform | Linux-only |
| Consumers | run-candidate-assembly-diff.sh (gcc into mktemp); run-nbest-choose-train-diff.sh (same source, train mode); runners.json[5]; docs/findings/sentence-surface.md, tooling-lane-gates.md, upstream-divergences.md, probe-coverage-abi.md, revert-plan.md, compatibility-policy.md. |
| State | One fresh mkdtemp user dir per case under `$TMPDIR`/`/tmp`, removed by `rm_rf` (or `rename`d into `CAND_ASSEMBLY_TRAIN_DIR/<fixture>-<ordinal>` in train mode). |
| Overlap | `rm_rf`, `yesno`, choose-by-type helpers mirror abi-probe-diff.c / live-typing-diff.c; T-cases re-drive the scheme setters that fullpin-diff.c / scheme-diff.c cover. |
| Constraints | Option words hard-coded: pinyin `USE_TONE` 0x20 (pin default pinyin.cpp:329), zhuyin `0x60`, 0x18a/0x1aa for R/U/F cases; sort words {0x0,0x1,0x1e,0x1f}; zhuyin import state deliberately OFF (#575); Luoma spelling uses `nihaobakua` not 历史 (#626). The wrapper's declared-divergence table is currently EMPTY (`declared=$(cat <<'TABLE' TABLE)`), so every case must be identical. Header: "Diagnosis only ... asserts nothing". |

### BIS-A-11 — tools/bisection/run-candidate-assembly-diff.sh

| Field | Value |
|---|---|
| Entry point | `run-candidate-assembly-diff.sh <oracle-prefix> <libpinyin_capi.so> <libzhuyin_capi.so> [out-dir]` ($2/$3 fall back to `OXPINYIN_CAPI_SO`/`OXPINYIN_ZHUYIN_SO`) |
| Owning subsystem | bisection |
| Purpose | Runs candidate-assembly-diff in pinyin and zhuyin mode against the pin's libpinyin.so/libzhuyin.so and oxpinyin's two cdylibs on the prefix's own data, splits the logs on `== ` case headers in an inline Python block, and requires every case identical except those a declared table names (which must still diverge; `lists-only:` reasons allow only candidate-row/count/`row=` differences). |
| Implementation | bash + embedded python3; 163 lines; gcc, realpath; sources oracle-cell.sh. |
| Inputs | argv; env `OXPINYIN_CAPI_SO`, `OXPINYIN_ZHUYIN_SO`, oracle-cell.sh vars; files `$prefix/oracle-pin.txt`, `lib/libpinyin.so`, `lib/libzhuyin.so`, `lib/libpinyin/data/bigram.db`. |
| Outputs | per mode: `declared  <case> [reason]`, `STALE <case> now identical`, `DIVERGENT <case>`, `N cases, M identical, K declared divergent`; `RESULT: as declared` or `RESULT: FAIL (logs: dir)`. Exit 0; 77 missing oracle-pin.txt/libpinyin.so; 1 missing input, not-the-pin, driver crash; 2 divergence outside table or stale declared row. |
| Platform | Linux-only |
| Consumers | runners.json[5] (`gate`, command `{prefix} {capi} {zhuyin}`, input_paths include `{prefix}/lib/libzhuyin.so`). |
| State | mktemp build dir (driver + per-mode-side cwd) removed on EXIT; out dir mktemp (removed only when result is as declared) or user-supplied (kept). Four logs + four .err files. No cargo spawn. |
| Overlap | Case-splitting-on-header comparison is unique here; path-realpath prologue shared with the argv-3 wrappers. |
| Constraints | Pin check: `grep -q '^pin_ref=libpinyin-2.11.92-074a2219c90feaf962d0d24f034514033ece5f99'` (prefix match, literal SHA — does NOT use `EXPECTED_PIN_REF`, so model/dbm suffix is not checked). "A missing input is a failure, never a skip" (but a missing prefix is still SKIP 77). Declared table is empty; comment says for #594 rows "re-measure, do not just edit the table". |

### BIS-A-12 — tools/bisection/chewing-diff.c

| Field | Value |
|---|---|
| Entry point | `./chewing-diff <so> <systemdir> [scheme]` (scheme ∈ {1,2,3,4,5,6,8,9}; 7 and 30 refused); compiled and run by run-scheme-diff.sh in `bopomofo` mode |
| Owning subsystem | bisection (W13 zhuyin scheme differential) |
| Purpose | Drives the chewing entry points over a corpus of zhuyin symbol sequences whose keystrokes are derived at startup from embedded pinned keyboard tables (zhuyin_table.h at 0c5e80e1), cross-checks those tables against the library via `pinyin_in_chewing_keyboard`, and prints parse length, keyboard lookups, candidates and aux text at every cursor, under CHEWING_FLAGS and again under `+FORCE_TONE` (register row 30). |
| Implementation | C (gnu11); 1348 lines (~420 lines of embedded symbol/tone tables for STANDARD, IBM, GINYIEH, ETEN, HSU, ETEN26, HSU_DVORAK, DACHEN_CP26); dlfcn; g_free/g_strfreev via libglib soname with free fallback. |
| Inputs | argv; env `SCHEME_DIFF_USER_DIR` (user dir; default `""` = cwd). Prefers `oxpinyin_init_for_fixtures` only when `<systemdir>/interpolation2.text` is absent. Symbols: init/fini/alloc/free_instance/set_options/set_zhuyin_scheme/parse_more_chewings/get_parsed_input_length/in_chewing_keyboard/guess_sentence/get_sentence/guess_candidates/get_n_candidate/get_candidate/get_candidate_type/get_candidate_string/get_chewing_auxiliary_text/reset. |
| Outputs | stdout: `set_zhuyin_scheme(N): true`, `table-check: ok` (or `table-check: ... mismatch` rows), per input `=== input: "keys" ===`, `in_chewing_keyboard('c'): true symbols=[..]`, `parse_chewing: consumed=N`, `parsed_input_length`, `guess_sentence`, `get_sentence`, `guess_candidates`, `n_candidates`, ≤12 `candidate[i]: type=.. text=".."`, `chewing_aux(c): ...` for every cursor, `reset_parsed_len`; `=== profile: FORCE_TONE ===` then the corpus again. Exit 0; 1 usage/bad scheme/dlopen/missing symbol/init NULL/scheme rejected/table mismatch. |
| Platform | Linux-only |
| Consumers | run-scheme-diff.sh (`gcc ... -o chewing-diff chewing-diff.c -ldl` when `$1 == bopomofo`); runners.json[31] `scheme-diff`; .gitignore; ROADMAP.md, docs/findings/bopomofo-spec.md, upstream-divergences.md, probe-coverage-abi.md, compatibility-policy.md; crates/oxpinyin-core/src/scheme.rs references it. |
| State | No temp dirs of its own (user dir supplied or cwd). |
| Overlap | `drive_input`, `ctype_name`, `resolve_g_free`/`free_strv`, `file_exists`+fixture-init selection duplicated from bisect.c / fullpin-diff.c; the keyboard tables duplicate crates/oxpinyin-core/src/scheme.rs and key-surface-diff.c / key-parse-diff.c chewing sweeps. |
| Constraints | Embedded tables are "copied verbatim from zhuyin_table.h at the pin 0c5e80e1" (an older pin than 074a2219) and are self-verified against the library at runtime (mismatch = exit 1, "drift ... fails the run loudly"); schemes 7/30 never sent (pin abort); CHEWING_FLAGS = IS_ZHUYIN|ZHUYIN_INCOMPLETE|USE_TONE hard-coded; sort 0x1e. Strict `strtol` scheme parsing. |

### BIS-A-13 — tools/bisection/classic-import.txt

| Field | Value |
|---|---|
| Entry point | Data fixture; path `$REPO_ROOT/tools/bisection/classic-import.txt` read by run-import-diff.sh |
| Owning subsystem | fixtures (bisection) |
| Purpose | Three-line classic-format user dictionary (`你好 ni'hao 3`, `世界 shi'jie 7`, `词 ci` — the last a 2-field line both importers floor at the ABI default count 5) for the frontend-vs-dictool interop round trip. |
| Implementation | text; 3 lines. |
| Inputs | n/a |
| Outputs | n/a |
| Platform | portable |
| Consumers | run-import-diff.sh (`FIXTURE=...classic-import.txt`, fed to `frontend-import` and `oxpinyin-dictool import`); runners.json[11] lists it as a required input. |
| State | none |
| Overlap | none |
| Constraints | The 2-field line is load-bearing for the "floor at 5" assertion in run-import-diff.sh's comment. |

### BIS-A-14 — tools/bisection/cpp-smoke.cc

| Field | Value |
|---|---|
| Entry point | `cpp-smoke <sysdir> <userdir>`; compiled and run by run-cpp-smoke.sh |
| Owning subsystem | abi (C++ header gate) |
| Purpose | Proves crates/oxpinyin-capi/pinyin.h compiles as C++ (extern "C" guard, fork-referenced typedefs/constants) by static-asserting 30-odd option/mask/index constants, then links libpinyin_capi.so directly and exercises `pinyin_get_parsed_input_length` (the W8 gap symbol) around `pinyin_parse_more_full_pinyins`/`pinyin_reset`. |
| Implementation | C++17; 114 lines; `#include <pinyin.h>` (generated header, needs glib include path), direct link (`-lpinyin_capi`, DT_NEEDED resolves to SONAME libpinyin.so.15). No dlopen. |
| Inputs | argv 2; no env. Symbols linked: pinyin_init, pinyin_alloc_instance, pinyin_get_parsed_input_length, pinyin_parse_more_full_pinyins, pinyin_reset, pinyin_free_instance, pinyin_fini. |
| Outputs | No stdout. Exit 64 usage; 1 init NULL; 2 alloc NULL; 3/4/5 parsed-length contract violations; 0 ok. |
| Platform | Linux-only in practice (SONAME/rpath handling in the wrapper); the TU itself is portable C++. |
| Consumers | run-cpp-smoke.sh (`g++ -std=c++17 -Wall -Wextra -Werror -O2 $GLIB_CFLAGS -I crates/oxpinyin-capi cpp-smoke.cc -L$CAPI_DIR -Wl,-rpath,$CAPI_DIR -lpinyin_capi`); run-bisect.sh calls run-cpp-smoke.sh; .github/workflows/ci.yml test job "C++ pinyin.h smoke gate"; tools/abi/check-alloc-pairing.sh mirrors its fixture prep and SONAME law; runners.json[7] (`oracle: false`); system-dir.sh comment; docs/findings/redb-removal-audit-2026-09-20.md, preedit-key-accessor-phase1.md. |
| State | none |
| Overlap | Only driver that links instead of dlopen-ing; the constant list duplicates pinyin.h / novel_types.h values. |
| Constraints | Hand-written expected values: every `static_assert` constant (PINYIN_INCOMPLETE 1<<3 ... PINYIN_CORRECT_ALL 0xff<<21, PHRASE_MASK, PHRASE_INDEX_LIBRARY_MASK, ADDON/NETWORK/USER_DICTIONARY 5/6/7, DOUBLE_PINYIN_DEFAULT==MS, ZHUYIN_DEFAULT==STANDARD, PHRASE_INDEX_MAKE_TOKEN). |

### BIS-A-15 — tools/bisection/run-cpp-smoke.sh

| Field | Value |
|---|---|
| Entry point | `tools/bisection/run-cpp-smoke.sh` (no args) |
| Owning subsystem | abi / ci |
| Purpose | Builds the capi for the cell, assembles a W3 fixture system dir with a stub interpolation2.text, creates a `libpinyin.so.15` symlink so the drop-in SONAME resolves to the build under test, compiles cpp-smoke.cc, verifies DT_NEEDED and ldd resolution, runs it. |
| Implementation | bash; 116 lines; g++, pkg-config glib-2.0, readelf, ldd, ln; sources oracle-cell.sh. |
| Inputs | Env: `OXPINYIN_CAPI_BACKEND_EXT` (kct|tkt|db; default: first existing of fixtures/w3/{db,tkt,kct}), `GLIB_CFLAGS` override, oracle-cell.sh vars (`PINYIN_ORACLE_DBM`, `OXPINYIN_CAPI_SO`, `CARGO_TARGET_DIR`, `OXPINYIN_TARGET_ROOT`). Data: `fixtures/w3/<ext>/*`. |
| Outputs | stdout progress + `cpp-smoke: ok`. Exit 0; 1 on any fatal (bad ext, missing fixture dir, DT_NEEDED != libpinyin.so.15, resolution outside build dir, binary nonzero). No SKIP path. |
| Platform | Linux-only (readelf/ldd/SONAME) |
| Consumers | .github/workflows/ci.yml (test job); run-bisect.sh; runners.json[7] `cpp-smoke` (`oracle: false`, required inputs "g++, glib, readelf, ldd"); tools/abi/check-alloc-pairing.sh duplicates its fixture prep "rather than shared so that adding this gate cannot break the existing one". |
| State | mktemp BUILD_DIR (user/, sys/ copy of fixture + stub `interpolation2.text` = `\data model interpolation` / `\1-gram` / `\item 1 ok count 1`, lib/ symlinks, binary) removed on EXIT. Spawns cargo via `oracle_cell_artifact`. |
| Overlap | Backend-ext/fixture selection block identical to run-bisect.sh; stub interpolation2.text duplicated in check-alloc-pairing.sh. |
| Constraints | No oracle, no pin check. Asserts `readelf -d` shows exactly `libpinyin.so.15` and `LD_LIBRARY_PATH="" ldd` resolves it under `$CAPI_DIR` — "how this gate first caught the change". Comment: default build is Berkeley DB since 2026-09-20, hence `db` first. |

### BIS-A-16 — tools/bisection/debuginfo-neutrality-lib.sh

| Field | Value |
|---|---|
| Entry point | Sourced library; provides `compare_debuginfo_neutrality <a.so> <b.so>` (plus `neutrality_symbol_lines`, `neutrality_symbol_bodies`, `neutrality_normalize_instructions`) |
| Owning subsystem | perf / profile |
| Purpose | Decides whether a debug-info build and a stock build are the same code: (a) identical .text bytes, (b) identical normalized instruction multiset at different addresses (Ir unaffected, simulated cache/branch figures layout-sensitive), (c) different multiset → stop. |
| Implementation | bash; 204 lines; objcopy, objdump, nm, sed/awk, sha256sum, comm, join. Normalizes only direct branch/call targets (x86 `j*`/`loop`/`call`, aarch64 `b`/`b.cond`/`bl`/`cbz`/`cbnz`/`tbz`/`tbnz`) and PC-relative materialization (`(%rip)` displacements, `adr`/`adrp`), and strips the v0 crate-disambiguator hash `Cs…_` → `CsH_`. |
| Inputs | Two .so paths. No env. |
| Outputs | Human-readable report on stdout (`PASS (a)`, `PASS (b): identical instruction multiset ... Symbol accounting: ...`, `FAIL (c): instruction multiset differs`). Returns 0 same code, 1 different. |
| Platform | Linux-only (binutils; supports x86-64 and aarch64 objdump syntax) |
| Consumers | run-callgrind-differential.sh stage 2; tools/env-probe/probe-debuginfo-neutrality.sh ("shared ... so the two cannot drift"). |
| State | Writes fixed scratch files `/tmp/neut-a.text`, `/tmp/neut-b.text`, `/tmp/neut-{a,b}.hist`, `.syms`, `.left`, `.bodies`, `.left-hashes` — fixed names in /tmp, not mktemp, never removed (two concurrent callers would collide). |
| Overlap | None in this range; the probe script is its other caller. |
| Constraints | Comment insists full instruction text (registers, immediates) is compared verbatim so a real codegen difference lands in (c); the (b) case is "expected and not a defect in -Cdebuginfo". |

### BIS-A-17 — tools/bisection/dict-surface-diff.c

| Field | Value |
|---|---|
| Entry point | `./dict-surface-diff <so> <systemdir>`; compiled by run-dict-surface-diff.sh and run-same-data-dir-diff.sh |
| Owning subsystem | bisection (Tier-C ABI surface) |
| Purpose | Only oracle coverage for the eight Tier-C symbols: `pinyin_lookup_tokens`, `pinyin_token_get_phrase/_n_pronunciation/_nth_pronunciation/_unigram_frequency`, `pinyin_token_add_unigram_frequency`, `pinyin_load_phrase_library`, `pinyin_unload_phrase_library`; token sweeps, add-then-read overlay (+11, absent-token add of 500), prediction dump before/after a +25 add (denominator shift), load/unload retval table. |
| Implementation | C; 284 lines; dlfcn + real glib `g_array_new/g_array_free/g_free` declared `extern` and linked (`-lglib-2.0` via pkg-config in the wrapper) — comment: a mirror GArray struct SIGFPEs in `g_array_maybe_expand`. |
| Inputs | argv; no env; user dir `""` (cwd — the wrapper runs each side in a private mktemp cwd). Prefers `oxpinyin_init_for_fixtures`. Also dlsyms `pinyin_guess_predicted_candidates_with_punctuations` (so punct.bin is required on the capi side). |
| Outputs | stdout (unbuffered) pipe-delimited rows: `lookup|<phrase>|<ok>|n=N|tok,tok,`, `token|<name>|phrase=..|len=..|text_ok=..|text=..|npron=..|pron=..|freq_ok=..|freq=..`, `nth-pron|..|nkeys=N|hhhh,`, `token|<name>|add11=..|freq=..|shift=..`, `absent-add=..`, `pred-before=..|n=N`, `pred-before-cand|i|get=|type=|type_ok=|text=|str_ok=`, `pred-after...`, `unload|i|0/1` for 0..7, `load|i|0/1` for {1,2,4,7}. stderr markers (`marker: dlopen done`, `probe token i`). Exit 0; 1 on failures. |
| Platform | Linux-only |
| Consumers | run-dict-surface-diff.sh (`cc -O2 -Wall -o $DRIVER dict-surface-diff.c -ldl $GLIB_LIBS` — note no `-Wextra -Werror`, no `-std`); run-same-data-dir-diff.sh default list; runners.json[8]; gcov_measure.py; abi-probe-diff.c comment; docs/findings/upstream-divergences.md, axis-j-{bdb,kc,tkrzw}-ledger.md. |
| State | Writes user.conf etc. into cwd (pin resolves `""` user dir against cwd). |
| Overlap | abi-probe-diff.c re-drives "dict-surface-diff.c's smoke-proved-safe shapes"; `must()` + fixture-init preference shared with key-surface-diff.c, addon-candidate-diff.c. |
| Constraints | Excluded by comment: zero-length lookup (pin SIGFPE, theirs-bug in upstream-divergences.md), out-of-range library indexes (pin asserts pinyin.cpp:466/:457), so unload only 0..7 and load only {1,2,4,7}. Absent token constant 0x09FFFFFE hard-coded. |

### BIS-A-18 — tools/bisection/run-dict-surface-diff.sh

| Field | Value |
|---|---|
| Entry point | `tools/bisection/run-dict-surface-diff.sh` (no args; `DICT_SYSTEM` env required) |
| Owning subsystem | bisection |
| Purpose | Compiles dict-surface-diff.c, runs it against the capi on `DICT_SYSTEM` and against the oracle on its own data, requires every probe family to be present in both logs (grep anchors), then `diff -u`. |
| Implementation | bash (`set -u` only, no -e); 190 lines; cc, pkg-config glib-2.0, diff; sources oracle-cell.sh and system-dir.sh (after the first oracle check; `REPO_ROOT` from `git rev-parse --show-toplevel`). |
| Inputs | Env: `PINYIN_ORACLE_PREFIX`, `DICT_SYSTEM` (required: pinyin_index.bin, phrase_index.bin, bigram.db, punct.bin, interpolation2.text), `PINYIN_ORACLE_DBM`, `OXPINYIN_CAPI_SO`, `OXPINYIN_CAPI_BACKEND_EXT` (exported by oracle-cell.sh), other oracle-cell/system-dir vars. Backend of DICT_SYSTEM checked with `system_dir_data_ext` (datagen-manifest.txt `backend=` or table.conf `database format:`). |
| Outputs | `IDENTICAL: N probe lines agree with the pin` or `DIVERGENCE` after the diff; exit 0; 77 SKIP (no oracle, off-pin, DICT_SYSTEM missing/incomplete); 1 build failure, pkg-config failure, backend undeterminable, BACKEND MISMATCH, driver crash; 2 divergence; 3 a probe family inactive. |
| Platform | Linux-only |
| Consumers | runners.json[8] (`gate`, env `DICT_SYSTEM: {data}`, exclusive `surface-native-output`). |
| State | mktemp driver dir + two mktemp cwd dirs (one per side) + two mktemp logs, all removed on EXIT (trap re-set once). Spawns cargo via `oracle_cell_artifact` (comment: "Supply OXPINYIN_CAPI_SO to run without Cargo; otherwise build dev opt-level 1"). |
| Overlap | Near-verbatim twin of run-key-surface-diff.sh (same prologue, same backend-mismatch block, same `probe_surfaces` pattern, same CWD isolation comment); sources helpers after `cd` without setting REPO_ROOT first for oracle-cell.sh (relies on `git rev-parse`). |
| Constraints | Pin check `grep -Fxq "$EXPECTED_PIN_REF"` exact line. Probe-presence anchors are hand-written expectations: `lookup|你好|1|n=1|`, `token|你好|phrase=1|len=2|`, `add11=1|.*shift=1$` for 你好 and 中国, `absent-add=0`, `unload|0..7|`, `load|1,2,4,7|`. |

### BIS-A-19 — tools/bisection/Dockerfile.callgrind

| Field | Value |
|---|---|
| Entry point | `docker build -f tools/bisection/Dockerfile.callgrind -t oxpinyin-callgrind .` then `docker run --rm [--security-opt label=disable --security-opt seccomp=unconfined] -v /tmp/cg-out:/out[:Z] oxpinyin-callgrind` (CMD = `bash tools/bisection/run-callgrind-differential.sh`) |
| Owning subsystem | perf / profile (container) |
| Purpose | Builds the Tkrzw cell pair (libpinyin at the pin, oxpinyin with `CARGO_PROFILE_RELEASE_DEBUG=1`, a no-debug control, an `alloc-count` build) plus valgrind and the `-g` bisect harness, so one harness can be run under callgrind against each .so. |
| Implementation | Dockerfile; 202 lines; base `debian:testing@sha256:dab11cdb…` (multi-arch, no --platform), apt snapshot `20260831T000000Z`, rustup-init 1.29.0 pinned per arch sha, toolchain 1.97.1, `cargo install cargo-c@0.10.25 --locked`, `tools/model/fetch-model.sh`, git fetch libpinyin by SHA 074a2219…, autotools `./configure --prefix=/opt/libpinyin-tkrzw --with-dbm=Tkrzw`, three `cargo cinstall --locked --release -p oxpinyin-capi --no-default-features --features tkrzw[,alloc-count]` into /opt/oxpinyin-{tkrzw,nodebug,alloccount}/stage, `gcc -std=gnu11 -Wall -Wextra -Werror -O2 -g -o /repo/tools/bisection/bisect bisect.c -ldl`, build-time `nm` check that `bisect_perf_cold_unit` ≠ `bisect_perf_steady_unit`. |
| Inputs | Build args `BASE_IMAGE` (must carry the same digest), `cargo_jobs` (default 4 → `CARGO_BUILD_JOBS`). Network: snapshot.debian.org, static.rust-lang.org, github.com/libpinyin, the model fetch. Sets `ENV PERF_CPU=0`, `CG_OUT=/out`. |
| Outputs | Image; at run time run-callgrind-differential.sh writes to `/out`. Build fails if anchors are missing or folded. |
| Platform | container (Linux amd64/arm64; "Never build or run this emulated") |
| Consumers | run-callgrind-differential.sh (header), docs/findings/perf-build-recipe-audit-2026-09-10.md, perf-cycle-ir-differential-2026-09-08.md. |
| State | Image layers; `/repo` is `COPY . /repo`. |
| Overlap | Declared "Dockerfile.perf-matrix verbatim" for base digest, apt snapshot, oracle pin, configure flags, cargo flags, release profile — i.e. ~100 lines duplicated; differences enumerated in its header (valgrind installed, no strip, debug info via CARGO_PROFILE_RELEASE_DEBUG, datagen dropped). |
| Constraints | "Do not pin --platform"; "Any override MUST carry this same digest"; libpinyin pin SHA hard-coded (074a2219c90feaf962d0d24f034514033ece5f99) — a third place besides build-oracle.sh and Dockerfile.perf-matrix; `strip` must NOT run; RUSTFLAGS `-Cdebuginfo` explicitly rejected (cargo strips it back out). |

### BIS-A-20 — tools/bisection/Dockerfile.perf-matrix

| Field | Value |
|---|---|
| Entry point | `docker build -f tools/bisection/Dockerfile.perf-matrix -t oxpinyin-matrix .`; `docker run --rm -v /tmp/matrix-out:/out oxpinyin-matrix` (CMD = `bash tools/bisection/run-perf-matrix.sh`) |
| Owning subsystem | perf (container) |
| Purpose | Reproducible 4-cell performance-matrix environment: libpinyin+Tkrzw (A), libpinyin+KyotoCabinet (B), oxpinyin+Tkrzw (C), oxpinyin+KyotoCabinet (D), with datagen-compiled data for C/D, all .so stripped, and the bisect harness built. |
| Implementation | Dockerfile; 173 lines; same base digest/apt snapshot/rustup/toolchain/cargo-c as Dockerfile.callgrind; apt adds libkyotocabinet-dev and libdb-dev (comment: Berkeley DB is the workspace default since 2026-09-20 and the parity oracle default cell since 2026-09-27, so run-perf-baseline.sh's default-feature build needs libdb); two autotools libpinyin builds (`--with-dbm=Tkrzw`, `--with-dbm=KyotoCabinet`); `cargo cinstall --locked --release -p oxpinyin-capi --no-default-features --features kyotocabinet|tkrzw`; `cargo run --locked --release -p oxpinyin-datagen --no-default-features --features <backend> -- compile --model-dir target/model20/extracted --backend <backend> --out-dir /opt/oxpinyin-<cell>/data`; `strip --strip-all` over /opt/*/lib and stages; `gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o /repo/tools/bisection/bisect bisect.c -ldl`. |
| Inputs | Network as above. `ENV PERF_CPU=0 PERF_RUNS=20 PERF_CYCLES=8 PERF_RAM_RUNS=10 MATRIX_OUT=/out`. |
| Outputs | Image; outputs written by run-perf-matrix.sh under `/out`. |
| Platform | container (native silicon only; "This FROM line used to pin linux/arm64 outright") |
| Consumers | run-perf-matrix.sh (CMD), run-perf-baseline.sh (header), run-same-data-dir-diff.sh (header: "Runs inside the perf-matrix container"), tools/datagen/libpinyin-drop-in-differential.sh, tools/oracle/Dockerfile.oracle ("same digest and apt snapshot"), ROADMAP.md, docs/runbooks/benches.md, oracle.md, many perf findings. |
| State | Image layers. |
| Overlap | Dockerfile.callgrind and tools/oracle/Dockerfile.oracle duplicate its base digest, apt snapshot, rustup pin and libpinyin fetch. |
| Constraints | Cell D explicitly pinned to kyotocabinet (2026-09-27, #552) because the default moved to bdb; libpinyin SHA hard-coded; "Do not pin --platform". Strips everything for apples-to-apples size. |

### BIS-A-21 — tools/bisection/dynamic-adjust-diff.c

| Field | Value |
|---|---|
| Entry point | `./dynamic-adjust-diff <so> <systemdir> <on|off> <userdir>`; compiled by run-dynamic-adjust-diff.sh and run-same-data-dir-diff.sh |
| Owning subsystem | bisection (W10 DYNAMIC_ADJUST) |
| Purpose | For seven fixed inputs: parse, guess_sentence, guess_candidates(0), choose a fixed candidate by (type, text), guess again at the advanced offset, print every candidate — with DYNAMIC_ADJUST (1<<9) set or clear so the runner can enforce non-vacuity and diff. |
| Implementation | C; 206 lines; dlfcn only. |
| Inputs | argv 4; no env. Symbols: init/fini/set_options/alloc/free_instance/parse_more_full_pinyins/guess_sentence/guess_candidates/get_n_candidate/get_candidate/get_candidate_type/get_candidate_string/choose_candidate. |
| Outputs | stdout: `CHOICE|<input>|<type>|<text>|<offset>` per input then `<input>|<offset>|<k>|<type>|<text>` rows, or `<input>|no-first-guess` / `<input>|<offset>|no-second-guess`. Exit 0; 1 on usage, dlopen, init, first guess failing, fixed candidate missing, candidate not leaving input. |
| Platform | Linux-only |
| Consumers | run-dynamic-adjust-diff.sh (`gcc -Wall -Wextra -Werror -O2` into mktemp; note no `-std=gnu11`); run-same-data-dir-diff.sh (special-cased: on/off × two engines, CHOICE and non-vacuity checks re-implemented inline); runners.json[9]; docs/findings/dynamic-adjust.md, probe-coverage-abi.md. |
| State | User dir is caller-supplied (runner recreates it fresh per run). |
| Overlap | PARITY_OPTIONS 0x18a "the parity word (tools/bisection/live-typing-diff.c)"; choose-by-identity loop shared with bigram-export-diff.c / candidate-assembly-diff.c. |
| Constraints | Hand-written fixed choices table INPUTS[] = {nihao→你, beijing→被, zhongguo→中, womenshi→我们, xiexieni→谢谢, shijie→世纪, pinyinshurufa→拼音}, all type 2; a missing one is a driver failure. |

### BIS-A-22 — tools/bisection/run-dynamic-adjust-diff.sh

| Field | Value |
|---|---|
| Entry point | `tools/bisection/run-dynamic-adjust-diff.sh` (no args) |
| Owning subsystem | bisection |
| Purpose | Drives dynamic-adjust-diff on/off on both engines, asserts the seven CHOICE rows are identical across all four runs, asserts non-vacuity (on ≠ off per engine), then compares ox vs pin per mode. |
| Implementation | bash; 120 lines; gcc, cmp, diff; sources system-dir.sh then oracle-cell.sh. |
| Inputs | Env: `PINYIN_ORACLE_PREFIX`, `DYNAMIC_ADJUST_SYSTEM` / `OXPINYIN_SYSTEM_DIR` (`resolve_system_dir DYNAMIC_ADJUST_SYSTEM dynamic-adjust`), oracle-cell.sh/system-dir.sh vars. Files: oracle-pin.txt (existence only — NO pin-ref grep), lib/libpinyin.so, lib/libpinyin/data. |
| Outputs | `choices: identical ...`, `non-vacuity: both engines' output changes with the bit`, `DYNAMIC_ADJUST=on|off: IDENTICAL|DIVERGENT` + `head -40` diff. Exit 0; 77 no oracle (prints a long note that the in-tree test agrees with nothing external); 1 build failure / missing choices / mismatched choices; 2 divergence; 3 vacuous. Header comment says "Absent -> skip ... exit 0" but code exits 77. |
| Platform | Linux-only |
| Consumers | runners.json[9] (`gate`). |
| State | mktemp driver dir and OUT dir (user dir recreated under `$OUT/user` per run; logs ox-on/ox-off/pin-on/pin-off + .choices), removed on EXIT. Spawns cargo via `oracle_cell_artifact`. |
| Overlap | The whole CHOICE/non-vacuity/diff protocol is re-implemented inline in run-same-data-dir-diff.sh for the same driver. |
| Constraints | Does not check `EXPECTED_PIN_REF` (only file existence) — the one wrapper in this range sourcing oracle-cell.sh without using the ref. Driver compiled without `-std=gnu11`. |

### BIS-A-23 — tools/bisection/export-diff.c

| Field | Value |
|---|---|
| Entry point | `./export-diff <so> <systemdir>`; compiled by run-export-diff.sh |
| Owning subsystem | bisection |
| Purpose | Imports one user phrase (你好世界/ni'hao'shi'jie/5 into library 7), then walks `pinyin_begin_get_phrases(ctx, index)` for indexes 0..15, 257, 263 (guint8 truncation to 1/7), unloads library 2 and walks 2 again, printing rows in iteration order (order is part of the surface; issue #533). |
| Implementation | C (gnu11); 153 lines; dlfcn; g_free from libglib soname with free fallback. |
| Inputs | argv; no env; `mkdir("user")` in cwd. Symbols: init/fini/begin_add_phrases/iterator_add_phrase/end_add_phrases/unload_phrase_library/begin_get_phrases/iterator_has_next_phrase/iterator_get_next_phrase/end_get_phrases. |
| Outputs | stdout: `add: true`, `row <idx> <phrase>\t<pinyin>\t<count>[\tget=false]`, `end <idx> rows=<n>`, `begin-null <idx>`, `unload 2: true`. Exit 0; 2 usage; 1 dlopen/init/import failure. |
| Platform | Linux-only |
| Consumers | run-export-diff.sh (`gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o $work/export-diff export-diff.c -ldl`); run-bigram-export-diff.sh/bigram-export-diff.c reference it; runners.json[10]; docs/runbooks/parity-gates.md, tooling-lane-gates.md, upstream-divergences.md, compatibility-policy.md, abi-allocator-pairing.md, crates/oxpinyin-capi/libpinyin.alloc. |
| State | `./user` in cwd, not removed. |
| Overlap | Export loop shape shared with import-diff.c `print_phrase_rows`, import-semantics-diff.c `dump`, live-typing-diff.c Phase D. |
| Constraints | 16..255 deliberately not driven (pin indexes `m_sub_phrase_indices[16]` out of bounds, phrase_index.cpp:611, class (b)). |

### BIS-A-24 — tools/bisection/run-export-diff.sh

| Field | Value |
|---|---|
| Entry point | `run-export-diff.sh <libpinyin.so> <libpinyin_capi.so> <data-dir>` |
| Owning subsystem | bisection |
| Purpose | Runs export-diff on both libraries over one data dir with a fresh user dir each and diffs the logs unsorted. |
| Implementation | bash; 57 lines; gcc, diff; sources oracle-cell.sh. |
| Inputs | argv 3 (`$2` falls back to `OXPINYIN_CAPI_SO`); oracle-cell.sh vars. |
| Outputs | `oracle summary:` (non-row lines), `export-diff: IDENTICAL (N lines)` or `DIVERGENCE (N differing lines)` + `head -60` diff. Exit 0; 77 missing oracle .so; 1 driver crash (tail -5 of stderr); 2 divergence. |
| Platform | Linux-only |
| Consumers | runners.json[10] (`gate`, command `{prefix}/lib/libpinyin.so {capi} {prefix}/lib/libpinyin/data`). |
| State | mktemp work dir with per-side cwd, logs, .err, diff; removed on EXIT; `LD_LIBRARY_PATH=$(dirname so)`. No cargo spawn. |
| Overlap | Skeleton identical to run-import-semantics-diff.sh and run-bigram-export-diff.sh. |
| Constraints | No oracle-pin.txt check at all (sources oracle-cell.sh only for path normalization of OXPINYIN_CAPI_SO). Header: run once per backend cell with the capi built on the matching backend. |

### BIS-A-25 — tools/bisection/frontend-import.cc

| Field | Value |
|---|---|
| Entry point | `frontend-import <input-file> <output-file>`; compiled and run by run-import-diff.sh |
| Owning subsystem | bisection (frontend interop) |
| Purpose | Links the pinned ibus-libpinyin build's object files (minus PYMain.o) and calls the real `LibPinyinBackEnd::init/allocPinyinInstance/importPinyinDictionary/exportPinyinDictionary` (PYLibPinyin.cc:230-353) so the frontend's own classic-format export can be diffed against oxpinyin-dictool. |
| Implementation | C++ (gnu++17); 49 lines; includes `PYLibPinyin.h`, `PYPConfig.h` from the frontend source tree; links frontend `.o` files + ibus-1.0/glib/gio/sqlite3 + `-lpinyin` from the oracle prefix. Direct link, no dlopen. |
| Inputs | argv 2. Runtime env set by the wrapper: `LD_LIBRARY_PATH=$IBUS_PREFIX/lib`, `XDG_CACHE_HOME` (fresh per run), `GSETTINGS_SCHEMA_DIR` (compiled private schema). |
| Outputs | Writes the export file. Exit 0 both calls TRUE; 2 usage; 3 alloc failed; 4 import failed; 5 export failed. |
| Platform | Linux-only (ibus, GSettings) |
| Consumers | run-import-diff.sh (`g++ -std=gnu++17 -O2 -I$FRONT_SRC -I$IBUS_PREFIX/include/libpinyin-2.11.92 $(pkg-config --cflags ibus-1.0 glib-2.0 gio-2.0 sqlite3) frontend-import.cc <objects> ... -L$IBUS_PREFIX/lib -lpinyin -o frontend-import`); .gitignore; docs/findings/bug-for-bug-audit-r2-2026-09-23-prereg.md. |
| State | none of its own; frontend writes into XDG cache. |
| Overlap | none |
| Constraints | Depends on build-oracle.sh's work dir layout `src/ibus-libpinyin-<sha>/src/*.o` (formerly `-1.16.5`); header says tag 1.16.5. |

### BIS-A-26 — tools/bisection/fullpin-diff.c

| Field | Value |
|---|---|
| Entry point | `./fullpin-diff <so> <systemdir> [scheme 1|2|3]`; compiled and run by run-scheme-diff.sh in `full` mode |
| Owning subsystem | bisection (W15 full-pinyin scheme differential) |
| Purpose | Drives `pinyin_set_full_pinyin_scheme` + the full-pinyin entry points over a corpus derived at runtime from the embedded pinned index for HANYU/LUOMA/SECONDARY_ZHUYIN (stride, toned, junk-suffixed, apostrophe pair, remap rows), printing parse/sentence/candidates/aux at every cursor. `SCHEME_DIFF_TONE=1` switches scheme 1 to USE_TONE and adds tone-digit-law rows. |
| Implementation | C (gnu11); 437 lines + `#include "fullpin-tables.h"`; dlfcn; g_free via libglib soname. |
| Inputs | argv; env `SCHEME_DIFF_TONE`, `SCHEME_DIFF_USER_DIR`. Fixture-init preferred only when `<systemdir>/interpolation2.text` is absent. Symbols: init/fini/alloc/free_instance/set_options/set_full_pinyin_scheme/parse_more_full_pinyins/get_parsed_input_length/guess_sentence/get_sentence/guess_candidates/get_n_candidate/get_candidate/get_candidate_type/get_candidate_string/get_full_pinyin_auxiliary_text/reset. |
| Outputs | stdout: `set_full_pinyin_scheme(N): true`, per input `=== input: ".." ===`, `parse_full: consumed=`, `parsed_input_length:`, `guess_sentence:`, `get_sentence: .. text=".."`, `guess_candidates:`, `n_candidates:`, ≤12 `candidate[i]: type=NBEST_MATCH|NORMAL text=`, `full_aux(c): .. text=".."` per cursor, `reset_parsed_len:`. Exit 0; 1 usage/dlopen/missing symbols/init NULL/set_options or scheme rejected/alloc NULL/corpus overflow. |
| Platform | Linux-only |
| Consumers | run-scheme-diff.sh (`gcc ... -o fullpin-diff fullpin-diff.c -ldl` when `$1 == full`; schemes 2/3 run against the PATCHED oracle at `PINYIN_PATCHED_ORACLE_PREFIX` with `SCHEME_DIFF_PARSE_AUX_ONLY` filtering); .gitignore; docs/findings/upstream-divergences.md. |
| State | none of its own. |
| Overlap | `drive_input`/`resolve_all`/`resolve_g_free`/`file_exists` near-identical to chewing-diff.c; HANYU inventory duplicates key-parse-diff.c's SYLLABLES and crates/oxpinyin-core/src/full_pinyin_index.rs. |
| Constraints | Corpus is "Nothing hand-authored" but MAX_CORPUS 128 and strides 37/29 are fixed; initial-only keys with tones ("n4") deliberately omitted (pin SIGABRT at pinyin_phrase3.h:152); schemes 2/3 hit the aux over-read (docs/findings/full-pinyin-aux-overread.md) hence the patched oracle in the wrapper. |

### BIS-A-27 — tools/bisection/fullpin-tables.h

| Field | Value |
|---|---|
| Entry point | `#include "fullpin-tables.h"` from fullpin-diff.c (after `struct index_row` is declared) |
| Owning subsystem | fixtures (bisection data header) |
| Purpose | Embeds the pinned `luoma_pinyin_index` (406 rows, pinyin_parser_table.h:674), `secondary_zhuyin_index` (pinyin_parser_table.h:1083) and the frozen 405-syllable HANYU inventory as C arrays. |
| Implementation | C header; 1242 lines; `static const struct index_row LUOMA_INDEX[]`, `SECONDARY_INDEX[]`, `static const char *const HANYU_SYLLABLES[]`; include guard FULLPIN_TABLES_H. |
| Inputs | n/a |
| Outputs | n/a |
| Platform | portable |
| Consumers | fullpin-diff.c only (grep confirms no other reference). |
| State | none |
| Overlap | Header says "keep in sync with crates/oxpinyin-core/src/full_pinyin_index.rs"; overlaps key-parse-diff.c's SYLLABLES list. |
| Constraints | "Generated from libpinyin 2.11.91 (0c5e80e1) pinyin_parser_table.h" — an older pin than 074a2219; sync is by review, not mechanical. |

### BIS-A-28 — tools/bisection/import-diff.c

| Field | Value |
|---|---|
| Entry point | `./import-diff <so> <systemdir>`; compiled by run-import-diff.sh (into the source dir) and by run-same-data-dir-diff.sh |
| Owning subsystem | bisection (W7-T1 user import) |
| Purpose | Scripted raw-ABI import sequence into library 7 (unique batch incl. a trailing-junk reading, re-import of the same pairs to prove counts ADD, a system-known pronunciation, an empty batch, two invalid readings), save once, export once, printing sortable `(phrase, pinyin, count)` triples. |
| Implementation | C (gnu11); 241 lines; dlfcn; g_free via libglib soname. |
| Inputs | argv; no env; `mkdtemp("/tmp/importdiff-user-XXXXXX")`. Prefers `oxpinyin_init_for_fixtures`. Symbols: init/fini/save/begin_add_phrases/iterator_add_phrase/end_add_phrases/begin_get_phrases/iterator_has_next_phrase/iterator_get_next_phrase/end_get_phrases. |
| Outputs | stdout: `add <label>: true|false` (labels unique-nihao, unique-nihao-trailing, unique-shijie, unique-zhongguo, reimport-*, system-meiguo, invalid-bang, invalid-incomplete), `empty batch: ok`, `save: true`, `phrase: <p>|<py>|<n>` rows or `phrase: BEGIN-NULL`/`GET-FAILED`. Exit 0; 1 on failures. |
| Platform | Linux-only |
| Consumers | run-import-diff.sh (`gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o import-diff import-diff.c -ldl`, sorts both logs before diffing); run-same-data-dir-diff.sh; referenced by union-diff.c, zhuyin-import-diff.c, run-union-diff.sh, run-punct-diff.sh, run-predict-diff.sh, run-user-candidate-diff.sh, run-addon-candidate-diff.sh, run-zhuyin-import-diff.sh, run-system-import-round-trip.sh, gcov_measure.py; runners.json[11]; .gitignore; docs/findings/phrase-union.md, dictool-format.md, tkrzw-distro-compat.md, axis-j ledgers. |
| State | mkdtemp user dir; only `rmdir` at the end ("best-effort: non-empty on oracle success" → leaks on the pin side). |
| Overlap | `resolve_all` with fixture-init preference identical in shape to bisect.c; export loop as in export-diff.c. |
| Constraints | Deliberately raw ABI ("the CLI's idempotency layer lives in oxpinyin-dictool"); invalid readings `!!!` and `n` must return false and leave no row. |

### BIS-A-29 — tools/bisection/run-import-diff.sh

| Field | Value |
|---|---|
| Entry point | `tools/bisection/run-import-diff.sh` (no args) |
| Owning subsystem | bisection |
| Purpose | Part 1: builds import-diff and the capi, checks the oracle pin (ref line + explicit `model_sha256=`/`dbm=` fields), resolves the capi system dir, runs both sides, diffs the sorted logs. Part 2: compiles frontend-import.cc against the pinned ibus-libpinyin object tree and round-trips classic-import.txt frontend→A, dictool→B, frontend(B)→C, asserting A==B and C==B. |
| Implementation | bash; 250 lines; gcc, g++, pkg-config (ibus-1.0 glib-2.0 gio-2.0 sqlite3), glib-compile-schemas, sort/diff; sources oracle-cell.sh and system-dir.sh. |
| Inputs | Env: `PINYIN_ORACLE_PREFIX`, `IMPORT_SYSTEM` / `OXPINYIN_SYSTEM_DIR` (`resolve_system_dir IMPORT_SYSTEM import-diff`), `PINYIN_IBUS_BUILD_DIR` (else probes `target/oracle-pin-build`, `target/oracle-tkrzw-build`, `target/oracle-w2` for `src/ibus-libpinyin-*/src/*.o`), `PINYIN_IBUS_PREFIX` (default `$IBUS_BUILD/prefix`, else the oracle prefix), `OXPINYIN_DICTOOL`, oracle-cell/system-dir vars. Reads `MODEL_SHA256=` from tools/oracle/build-oracle.sh. Files: oracle-pin.txt, lib/libpinyin.so, lib/libpinyin/data/bigram.db, the frontend gschema.xml, classic-import.txt. |
| Outputs | capi/oracle triples printed sorted; `import-diff: IDENTICAL`; `classic frontend interop: IDENTICAL (both directions)`. Exit 0; 77 SKIP (no oracle, off-pin ref, off-pin model/dbm fields, no bigram.db, no ibus tree, ibus prefix off-pin, missing g++/pkg-config/glib-compile-schemas); 1 build/run failure or BACKEND MISMATCH (from system-dir.sh exit 3 in practice); 2 divergence. |
| Platform | Linux-only (ibus) |
| Consumers | runners.json[11] (`gate`, env `IMPORT_SYSTEM: {data}`, exclusive `bisection-source-output`, required inputs include dictool and `PINYIN_IBUS_BUILD_DIR`/`PINYIN_IBUS_PREFIX`). |
| State | Writes `tools/bisection/import-diff` and `tools/bisection/frontend-import` into the source tree; mktemp logs, SCHEMA_DIR, WORK (dictool-user, frontend caches) removed on EXIT. Spawns cargo twice via `oracle_cell_artifact` (capi, then `oxpinyin-dictool`). |
| Overlap | Pin-check prologue shared with the other wrappers but the only one that additionally greps `model_sha256=` and `dbm=` (comment records the old `--with-dbm=` scrape bug that matched the empty string). |
| Constraints | Three pin checks: `grep -Fxq "$EXPECTED_PIN_REF"`, `^model_sha256=<build-oracle.sh value>`, `-x dbm=$ORACLE_DBM_NAME`; plus a second literal `IBUS_PIN_REF='libpinyin-2.11.92-074a2219…'` prefix grep for the ibus prefix. Comment says "Absent -> skip ... exit 0" but exits 77. |

### BIS-A-30 — tools/bisection/import-semantics-diff.c

| Field | Value |
|---|---|
| Entry point | `./import-semantics-diff <so> <systemdir> <index> [read|rewrite]`; compiled by run-import-semantics-diff.sh and run-system-import-round-trip.sh |
| Owning subsystem | bisection (issue #534 `pinyin_iterator_add_phrase` semantics) |
| Purpose | Imports a 20-entry battery (counts -1/0/1/5/-2/INT_MIN/INT_MAX/+1 on INT_MAX, toned/toneless readings, repeat readings, corrections, length mismatch, empty) into library `<index>`, prints each add's return, exports only battery phrases live, saves, reopens and exports again; probes candidate presence for "ceshi"/"ceyan"; for indexes 1..4 runs a clear-margin dilution score on "cece". `read|rewrite` cross-reader mode re-reads an existing `./user` profile (optionally re-saving it) for pin→oxpinyin→pin preservation. |
| Implementation | C (gnu11); 295 lines; dlfcn; g_free via libglib soname. |
| Inputs | argv 3–4; no env; `mkdir("user")` in cwd. Symbols: init/fini/save/begin_add_phrases/iterator_add_phrase/end_add_phrases/begin_get_phrases/iterator_has_next_phrase/iterator_get_next_phrase/end_get_phrases/alloc_instance/free_instance/parse_more_full_pinyins/guess_candidates/get_n_candidate/get_candidate/get_candidate_string/guess_sentence/get_sentence. |
| Outputs | stdout: `add <i> <phrase> <pinyin> <count>: true|false`, `before-dilution score: guessed= got= text=`, `after-dilution score: ...`, `live <phrase>\t<pinyin>\t<count>` + `live rows=N`, `live probe ceshi <phrase>: present|absent`, `save: true`, `reopened ...` repeats, `reopened score`. Exit 0; 2 usage/bad mode; 1 dlopen/init/add failures in the dilution block. |
| Platform | Linux-only |
| Consumers | run-import-semantics-diff.sh (gcc into mktemp); run-system-import-round-trip.sh (`cc -std=gnu11 -Wall -Wextra -Werror -O2 ... import-semantics-diff.c`); runners.json[12]. |
| State | `./user` in cwd (the wrapper gives each side+library its own cwd). |
| Overlap | Export/probe loops shared with export-diff.c and import-diff.c. |
| Constraints | Index ≥16 path: adds only, then exits (pin reads a 16-slot array out of bounds, phrase_index.h:630, class (b)) — the wrapper asserts oxpinyin answers every add false. Battery and sort word 0x1e hard-coded; score requires 1..4. |

### BIS-A-31 — tools/bisection/run-import-semantics-diff.sh

| Field | Value |
|---|---|
| Entry point | `run-import-semantics-diff.sh <libpinyin.so> <libpinyin_capi.so> <data-dir>` |
| Owning subsystem | bisection |
| Purpose | Runs import-semantics-diff for libraries 0 1 2 3 4 5 6 7 15 (must be byte-identical) and 16/255 (pin may crash; oxpinyin must exit 0 with every add false). |
| Implementation | bash; 91 lines; gcc, diff, grep; sources oracle-cell.sh. |
| Inputs | argv 3 (`$2` ← `OXPINYIN_CAPI_SO`); oracle-cell.sh vars. |
| Outputs | `library N: IDENTICAL (n lines)` / `DIVERGENCE` + `head -40` / `FAIL (…driver exited nonzero)`; `library 16|255: oxpinyin refuses every add; pin rc=N (class (b), not reproduced)` or `DIVERGENCE (oxpinyin rc=..; every add must be false)`; final `import-semantics-diff: IDENTICAL|DIVERGENCE|FAILURE`. Exit 0/1/2; 77 missing oracle .so. |
| Platform | Linux-only |
| Consumers | runners.json[12] (`gate`, three cells). |
| State | mktemp work dir with per-side-per-library cwd and logs, removed on EXIT; `LD_LIBRARY_PATH=$(dirname so)`; stderr discarded. No cargo spawn. |
| Overlap | Skeleton identical to run-export-diff.sh. |
| Constraints | No oracle-pin.txt check. Hand-written law: for 16/255 `grep -q ': true$'` in the oxpinyin log is a divergence. |

### BIS-A-32 — tools/bisection/key-parse-diff.c

| Field | Value |
|---|---|
| Entry point | `./key-parse-diff <so> <systemdir>`; compiled only by run-same-data-dir-diff.sh (no dedicated wrapper, not in runners.json by name) |
| Owning subsystem | bisection (#547 single-key parse sweep) |
| Purpose | Exhaustive single-key parse differential: under 12 option words, probes `pinyin_parse_full_pinyin` over every prefix of every pinned syllable plus tone digits 0..6 and junk under full schemes 1..3; `pinyin_parse_double_pinyin` over every 1–2 char string of `[a-z;]` under double schemes 1..6; `pinyin_parse_chewing` over every 1–2 char string of the printable keyboard set (and 3-char strings whose first two parsed) under zhuyin schemes {1..6,8,9}; prints retval and the raw 2-byte key (prefilled 0xABAB). |
| Implementation | C (gnu11, dense one-line style); 197 lines (≈90 of them the SYLLABLES table); dlfcn; static `inputs[8192][12]` buffer. |
| Inputs | argv; no env; user dir `""` (cwd). Symbols: init/alloc_instance/set_options/set_full_pinyin_scheme/set_double_pinyin_scheme/set_zhuyin_scheme/parse_full_pinyin/parse_double_pinyin/parse_chewing. Never calls fini/free. |
| Outputs | stdout: `options|0x..|<ret>`, `full-scheme|N|<ret>`, `full|<scheme>|0x<word>|<input>|<ok>|<hex4>`, `double-scheme|..`, `double|..`, `zhuyin-scheme|..`, `chewing|..` (very large logs: 12 words × thousands of inputs). Exit 0; 1 usage/dlopen/init NULL. |
| Platform | Linux-only |
| Consumers | run-same-data-dir-diff.sh default driver list (built with `gcc -std=gnu11 -Wall -Wextra -O2 ... -ldl $glib_flags`); no other references in the tree. |
| State | Writes user.conf into cwd (pin); leaks context. |
| Overlap | Same three entry points as key-surface-diff.c (which is the gated, smaller sweep) and abi-probe-diff.c's extras phase; SYLLABLES duplicates fullpin-tables.h HANYU list + correction aliases. |
| Constraints | Option words table hard-coded `{0x0,0x8,0x20,0x28,0x18a,0x1aa,0x1ea,0x1ca,0x3fe00000,0x1fe00018,0x1fe0039a,0xffffffff}`; apostrophes never probed (pin asserts pinyin_parser2.cpp:170); zhuyin 7 skipped (pin abort). Chewing 3-char probes bypass `probe()` and inline the printf (same format). |

### BIS-A-33 — tools/bisection/key-surface-diff.c

| Field | Value |
|---|---|
| Entry point | `./key-surface-diff <so> <systemdir>`; compiled by run-key-surface-diff.sh and run-same-data-dir-diff.sh |
| Owning subsystem | bisection (Tier-A key surface, #109, D1/D3) |
| Purpose | Single-key parsing under four option profiles (0x18a, 0x1aa, 0x1ea, 0x1ca) over fixed full/double/chewing input lists and scheme sweeps, logging retval + raw 2-byte key (0xAB-prefilled); renders all display getters (`pinyin_get_zhuyin_string`, `_pinyin_string`, `_luoma_pinyin_string`, `_secondary_zhuyin_string`, `_pinyin_strings` incl. NULL out-param skip, `_pinyin_is_incomplete`) over fixed keys; zero-key sentinel probes; `pinyin_get_context` identity; `pinyin_unload_addon_phrase_library` for {0,5,15}. |
| Implementation | C; 363 lines; dlfcn; carries its own `ChewingKey`/`ChewingKeyRest` bitfield mirrors with `_Static_assert` sizes 2/4. |
| Inputs | argv; no env; user dir `""`. Prefers `oxpinyin_init_for_fixtures`. |
| Outputs | stdout: `layout|sizeof_key=2|sizeof_rest=4`, `full|<word>|<input>|<ok>|<hex>`, `options|<word>|refused`, `double|<scheme>|<word>|<input>|<ok>|<hex>`, `chewing|...`, `render|full|<input>|zhuyin|pinyin|luoma|secondary|shengmu|yunmu|shengmu_skip|incomplete|...`, `render|...|unparsed`, `zero|zhuyin|..`, `zero|strings|..`, `zero|incomplete|..`, `context|match|1`, `unload_addon|i|0/1`. Exit 0; 1 on failures. |
| Platform | Linux-only |
| Consumers | run-key-surface-diff.sh (`cc -O2 -o $DRIVER key-surface-diff.c -ldl` — no warnings flags, no std); run-same-data-dir-diff.sh; runners.json[13]; gcov_measure.py; abi-probe-diff.c comments; ROADMAP.md; crates/oxpinyin-chewing/src/chewing_key.rs; docs/findings/double-pinyin-spec.md, bopomofo-spec.md, pinyin-dbm-format-2026-09-01.md, perf-keycost-first-alloc, redb-removal-audit, upstream-divergences.md, axis-j ledgers. |
| State | cwd user.conf (pin). |
| Overlap | Superset/subset relation with key-parse-diff.c; the zero-key `get_strings(&zero,(gchar**)0x1,(gchar**)0x2)` crash-as-check is unique. |
| Constraints | Pointer sentinels 0x1/0x2 passed deliberately ("a guard violation would write through ... and crash before this line — the absence of a crash IS the check"); no `ni'hao` (pin asserts); double 30 / zhuyin 7 never set; addon unload only in-range (pin asserts pinyin.cpp:499); no null-instance probes (pin has no null guards, pinyin.cpp:1358-1360). |

### BIS-A-34 — tools/bisection/run-key-surface-diff.sh

| Field | Value |
|---|---|
| Entry point | `tools/bisection/run-key-surface-diff.sh` (no args; `KEY_SYSTEM` env required) |
| Owning subsystem | bisection |
| Purpose | Compiles key-surface-diff.c, runs it on the capi (`KEY_SYSTEM`) and the oracle (own data), requires the full probe matrix (4 profiles, 6 double schemes, 8 chewing keyboards, 8 render kinds, 3 zero sentinels, context, 3 addon rows) to be present in both logs, then `diff -u`. |
| Implementation | bash (`set -u`); 210 lines; cc, diff; sources oracle-cell.sh + system-dir.sh (`REPO_ROOT` from `git rev-parse`). |
| Inputs | Env: `PINYIN_ORACLE_PREFIX`, `KEY_SYSTEM` (needs pinyin_index.bin, phrase_index.bin, bigram.db, interpolation2.text), `PINYIN_ORACLE_DBM`, `OXPINYIN_CAPI_SO`, `OXPINYIN_CAPI_BACKEND_EXT`, other helper vars. |
| Outputs | `IDENTICAL: N probe lines agree with the pin` / `DIVERGENCE` / `FAIL: diff exited with status N (comparison error, not a divergence)`. Exit 0; 77 SKIP (no oracle, off-pin, KEY_SYSTEM incomplete); 1 build failure, backend undeterminable, BACKEND MISMATCH, crash, diff error; 2 divergence; 3 probe matrix incomplete. Header says "Skip → 0 matches the sibling family" but the code exits 77. |
| Platform | Linux-only |
| Consumers | runners.json[13] (`gate`, env `KEY_SYSTEM: {data}`, exclusive `surface-native-output`). |
| State | mktemp driver dir, two cwd dirs, two logs; removed on EXIT. Spawns cargo via `oracle_cell_artifact`. |
| Overlap | Twin of run-dict-surface-diff.sh (same prologue/backend check/CWD isolation); probe anchors enumerate the driver's matrix. |
| Constraints | Pin check `grep -Fxq "$EXPECTED_PIN_REF"`. Hand-written probe anchors (`^full|0x18a|`, `^double|1..6|0x18a|`, `^chewing|{1..6,8,9}|0x18a|`, `^render|full|.*|<kind>|`, `^zero|…`, `^context|match|`, `^unload_addon|0|5|15|`). Distinguishes diff status >1 as harness failure. |

### BIS-A-35 — tools/bisection/live-typing-diff.c

| Field | Value |
|---|---|
| Entry point | `LIVETYPING_ROUNDS=<n> ./live-typing-diff <so> <systemdir>`; compiled by run-live-typing-diff.sh (into the source dir) and run-same-data-dir-diff.sh |
| Owning subsystem | bisection (W12 live typing / decoded-continuation train) |
| Purpose | Phases: A baseline "nihao"; B0 terminal choose of 你 for "ni"; B live typing after choosing 你 then re-parsing nihaos…nihaoshijie keystroke by keystroke; BP backspace ladder after a choose (down to "ni", skipping "nih"); CR committed re-parse (choose phrase 你好, re-parse longer, `pinyin_clear_constraint` twice — register #8/R5); C decoded-continuation training rounds (choose 你, let 好 decode, `pinyin_train`); D user-store phrase + bigram export. Each probe prints guess retval, candidate head (≤8 with type/nbest), and sentence rows 0..proven. |
| Implementation | C (gnu11); 777 lines; dlfcn; g_free via libglib soname; `parse_rounds_env` strict strtol 1..8. |
| Inputs | argv; env `LIVETYPING_ROUNDS` (default 3, 1..8); `mkdtemp("/tmp/livetypingdiff-user-XXXXXX")`. Uses `pinyin_init` only (no fixture-init). Symbols: init/fini/alloc/free_instance/parse_more_full_pinyins/guess_sentence/guess_candidates/get_n_candidate/get_candidate/get_candidate_type/get_candidate_string/get_candidate_nbest_index/get_sentence/choose_candidate/train/clear_constraint/reset/set_options/begin_get_phrases/iterator_has_next_phrase/iterator_get_next_phrase/end_get_phrases/begin_get_bigram_phrases/bigram_iterator_has_next_phrase/bigram_iterator_get_next_phrase/end_get_bigram_phrases. |
| Outputs | stdout: `probe:<label> guess=N`, `probe:<label> n=N`, `probe:<label> cand[i]=TYPE/idx/text`, `probe:<label> sentence[i]=..`, `probe:<label> parsed=N`, `term:cursor=N`, `live:cursor=N`, `bp:cursor=N`, `cr:cursor=N`, `cr:clear0=N`, `cr:clear0b=N`, `round:N cursor=N`, `round:N train=N`, `phrase: p|py|n`, `bigram: p|py|n`. Exit 0; 1 on any failure (all routed through `fail_inst/fail_ctx/fail` cleanup). |
| Platform | Linux-only |
| Consumers | run-live-typing-diff.sh (`gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o live-typing-diff live-typing-diff.c -ldl`); run-same-data-dir-diff.sh; run-uncovered-surface-diff.sh / uncovered-surface-diff.c and dynamic-adjust-diff.c reference it (PARITY_OPTIONS source); runners.json[14]; .github/workflows/ci.yml (via the wrapper); .gitignore; docs/findings/live-typing.md, dynamic-adjust.md, bug-for-bug-audit-r2 prereg; docs/runbooks/parity-gates.md. |
| State | mkdtemp user dir removed by `rm_rf` on every path. |
| Overlap | Header: "Helpers (shared shape with nbest-train-diff.c)"; `rm_rf`, `find_by_text`, `type_name`, `print_sentences`/proved-index rule duplicated in abi-probe-diff.c `dump_rows`. |
| Constraints | Oracle caller contracts: `get_sentence` only up to the max nbest index the candidate list proved (pin asserts pinyin.cpp:1474); BP ladder refuses a cursor > 2 (pin `_check_offset` abort one-past-end); CR phase skips `guess_sentence` on purpose (W14 dedup would drop the phrase row); NBEST rows never chosen where a live forcing is needed. PARITY_OPTIONS 0x18a and sort 0x1e hard-coded. |

### BIS-A-36 — tools/bisection/run-live-typing-diff.sh

| Field | Value |
|---|---|
| Entry point | `tools/bisection/run-live-typing-diff.sh` (no args) |
| Owning subsystem | bisection / ci |
| Purpose | Builds live-typing-diff and the capi, env-gates on the pin oracle and a real-unigram system dir, runs both sides with `LIVETYPING_ROUNDS`, requires the post-choose surface to be active (`live:cursor=`, `round:1 cursor=`, `probe:after-choose n=[1-9]`, `cr:cursor=`, `cr:clear0=1`), diffs full logs. |
| Implementation | bash; 174 lines; gcc, diff; sources system-dir.sh then oracle-cell.sh. |
| Inputs | Env: `PINYIN_ORACLE_PREFIX`, `LIVETYPING_SYSTEM` / `OXPINYIN_SYSTEM_DIR` (`resolve_system_dir LIVETYPING_SYSTEM live-typing-diff`), `LIVETYPING_ROUNDS` (default 3), helper vars. Files: oracle-pin.txt, lib/libpinyin.so, data/bigram.db, `$CAPI_SYSTEM/interpolation2.text`. |
| Outputs | `post-choose surface active on both sides`, `live-typing-diff: IDENTICAL` + the `phrase:`/`bigram:` export lines, or `DIVERGENCE (live-typing surface or train/export)` + diff. Exit 0 identical OR skipped (no oracle, off-pin, no bigram.db, no interpolation2.text all exit 0 — the only wrapper in this range whose SKIP is 0, so it can sit in CI); 1 build/run failure, surface inactive, diff status >1; 2 divergence. |
| Platform | Linux-only |
| Consumers | .github/workflows/ci.yml test job step "Live-typing differential" (self-skips on GitHub runners; model20 never enters CI); runners.json[14] (`gate`, exclusive `bisection-source-output`); docs/runbooks/parity-gates.md. |
| State | Writes `tools/bisection/live-typing-diff` into the source tree; four mktemp files removed on each path. Spawns cargo via `oracle_cell_artifact`. |
| Overlap | Prologue identical to run-abi-probe-diff.sh / run-dynamic-adjust-diff.sh; the "unresolvable dir is FATAL, never a mini-fixture run" note repeated. |
| Constraints | Pin check `grep -Fxq "$EXPECTED_PIN_REF"`. Comment: presence checks "do NOT bind the tables' identity to the oracle pin (a content hash or manifest belongs to the parked oracle-provisioning work)". Hand-written surface anchors listed above. |

### BIS-A-37 — tools/bisection/locale-diff.c

| Field | Value |
|---|---|
| Entry point | `locale-diff <pinyin|zhuyin> <lib.so> <systemdir> <userdir|NULL|""|.> <scratchdir> [<chdir-to>]`; compiled by run-locale-diff.sh |
| Owning subsystem | bisection (#539 LC_NUMERIC / #619 user-dir forms, register rows 39/46) |
| Purpose | Plays a consumer that adopted the environment's locale and, after every entry point (before, init-ok, chdir, save-unmodified, train, save-modified, fini, six failing inits, init/fini with NULL userdir), prints `LC_NUMERIC`, the `LC_ALL` composite, each watched directory's inventory (`name:size:mode`) and its user.conf text — so the pin's `setlocale(LC_NUMERIC,"C")` leak and where the profile lands can be diffed across user-dir forms on both facades. |
| Implementation | C (c11); 425 lines; dlfcn (`RTLD_NOW|RTLD_LOCAL`), `<locale.h>`, `<dirent.h>` scandir, `<glib.h>` linked (GArray for lookup_tokens); symbols resolved as `<prefix>_<name>` so one binary drives libpinyin and libzhuyin. |
| Inputs | argv 5–6; env `LC_ALL` (set by the wrapper; `setlocale(LC_ALL,"")` failing is fatal). Symbols: init/fini/save/alloc_instance/free_instance/lookup_tokens/token_get_unigram_frequency/parse_more_full_pinyins|parse_more_chewings/guess_sentence/get_n_candidate/get_candidate/get_candidate_type/get_candidate_string/choose_candidate/reset/guess_candidates|guess_candidates_after_cursor/train. |
| Outputs | stdout (line-buffered): `<step>\tLC_NUMERIC=..\tLC_ALL=..`, `<step>\tcwd: name:size:mode ...`, `<step>\tcwd/user.conf: <escaped text>|(absent)`, same for `userdir`/`first-cwd`, `init-ok: init ok|NULL`, `save-unmodified: save N`, `train: watched 历时 unigram 0x..=N`, `train: target <text>`, `train: ok|no-list|no-candidate|choose-failed|reguess-failed|train-false`, `fini: done`, `init-<fixture>: init NULL`, `init-null-userdir: ...`. No paths printed. Exit 0; 2 usage; 1 dlopen/getcwd/chdir/scandir/alloc/init failure. |
| Platform | Linux-only (glibc locale names) |
| Consumers | run-locale-diff.sh (`cc -std=c11 -O1 -Wall -Wextra -o $OUT/locale-diff locale-diff.c $(pkg-config --cflags --libs glib-2.0) -ldl` — no -Werror); runners.json[15]; docs/findings/user-store.md, upstream-divergences.md, compatibility-policy.md; tests crates/oxpinyin-capi/tests/userdir_arg.rs, oxpinyin-zhuyin-capi/tests/userdir_arg.rs, oxpinyin-runtime/tests/empty_user_dir.rs cite it. |
| State | Writes user files into cwd / userdir per form; failing inits leak the pin's half-built context by design. |
| Overlap | Trains "the same pair tools/bisection/open-counter-diff.c trains first" (li'shi → 历时); prefix-symbol resolution like candidate-assembly-diff.c `must_prefixed`. |
| Constraints | Fixed typed inputs/watched words (`li'shi`/历时, zhuyin `2u04vu04`/癫痫), sort word 0x1c, LISTED_CANDIDATE 2 hard-coded; whether train must succeed is decided by the wrapper per form. |

### BIS-A-38 — tools/bisection/run-locale-diff.sh

| Field | Value |
|---|---|
| Entry point | `tools/bisection/run-locale-diff.sh` (no args; env-driven) |
| Owning subsystem | bisection |
| Purpose | Builds locale-diff, prepares shared failure fixtures (empty/, garbage/table.conf, truncated/table.conf = first 2 lines of the real one), and for each lib ∈ {pinyin, zhuyin} × form ∈ {abs, empty, empty-chdir, null, dot} × side runs the driver twice in the same directories under `LC_ALL=$LOCALE`, asserts the oracle trained in both processes (except `null`), diffs oracle vs ox logs. |
| Implementation | bash; 220 lines; cc + pkg-config glib-2.0, `locale -a`, diff; sources oracle-cell.sh (for `CARGO_TARGET_DIR` default paths only). |
| Inputs | Env: `LOCALE_DIFF_ORACLE_PREFIX` (falls back to `PINYIN_ORACLE_PREFIX`; no default), `LOCALE_DIFF_PINYIN_SO` (← `OXPINYIN_CAPI_SO` ← `$CARGO_TARGET_DIR/debug/libpinyin_capi.so`), `LOCALE_DIFF_ZHUYIN_SO` (← `OXPINYIN_ZHUYIN_SO` ← `.../libzhuyin_capi.so`), `LOCALE_DIFF_LIBS`, `LOCALE_DIFF_FORMS`, `LOCALE_DIFF_LOCALE` (default zh_CN.UTF-8, must be in `locale -a`), `LOCALE_DIFF_OUT`, oracle-cell vars. Files: `$PREFIX/oracle-pin.txt`, `lib/libpinyin.so.15`, `lib/libzhuyin.so.15`, `lib/libpinyin/data/table.conf`. |
| Outputs | `data dir:`, `locale:`, per lib/form `identical (N lines)` or `DIVERGENT` + full diff; `run failure: ...` lines. Exit 0; 77 prefix unset/not a dir; 3 missing input (not-the-pin, unknown form, locale not generated, missing .so, no table.conf); 1 driver run failure or oracle train not `ok` twice; 2 divergence. |
| Platform | Linux-only (needs a generated zh_CN.UTF-8 locale) |
| Consumers | runners.json[15] (`gate`, env `LOCALE_DIFF_ORACLE_PREFIX: {prefix}`, `LOCALE_DIFF_PINYIN_SO: {capi}`, `LOCALE_DIFF_ZHUYIN_SO: {zhuyin}`, required input "generated zh_CN.UTF-8 locale"). |
| State | OUT dir (mktemp removed on EXIT, or `LOCALE_DIFF_OUT` kept) holding the driver, `scratch/{empty,garbage,truncated}`, per `<lib>-<form>-<side>/{cwd,userdir|moved-to}` dirs, `.log`/`.err`/`.diff`. No cargo spawn. |
| Overlap | Only wrapper that targets the versioned `libpinyin.so.15`/`libzhuyin.so.15` names and drives both facades with one driver. |
| Constraints | Pin check: `grep -q '^pin_ref=libpinyin-2.11.92-074a2219c90feaf962d0d24f034514033ece5f99'` (literal prefix, not `EXPECTED_PIN_REF`). Hand-written expectation: `grep -c '^train: ok$'` == 2 in the oracle log for every form but null. stderr kept but never compared (#545). |

### BIS-A-39 — tools/bisection/run-callgrind-differential.sh

| Field | Value |
|---|---|
| Entry point | `bash tools/bisection/run-callgrind-differential.sh` inside the Dockerfile.callgrind image (its CMD) |
| Owning subsystem | perf / profile (measurement recipe, not a gate) |
| Purpose | Instruction-level differential of the steady keystroke cycle, libpinyin+Tkrzw vs oxpinyin+Tkrzw on the SAME libpinyin-installed data dir: stage 0 environment table; 1 anchor distinctness (`nm`); 2 debug-info neutrality; 3 valgrind malloc-interception probe (`--trace-redir`); 4 callgrind Ir/D1/LL/branch for both cells × `CG_ROUNDS`; 5 hardware `instructions:u` via `PERF_HW=instructions` (median per cell, ratio, or `NO HARDWARE COUNTERS`); 6 allocations per cycle from the alloc-count build; 7 wall-clock timing via perf-ci.py. |
| Implementation | bash + two embedded python3 blocks; 330 lines; valgrind/callgrind_annotate, taskset, nm, sha256sum, perf-ci.py; sources debuginfo-neutrality-lib.sh. |
| Inputs | Env: `CG_OUT` (default /out), `PERF_CPU` (0), `CG_CYCLES` (4), `CG_REPEATS` (1), `CG_ROUNDS` (2), `T_CYCLES` (8), `T_RUNS` (20), `T_REPEATS` (1), `HW_RUNS` (10). Hard-coded paths: `/opt/libpinyin-tkrzw` (libpinyin.so, lib/libpinyin/data, pkgconfig), `/opt/oxpinyin-tkrzw/stage`, `/opt/oxpinyin-nodebug/stage`, `/opt/oxpinyin-alloccount/stage` (finds `libpinyin.so` — the cargo-c install name, not libpinyin_capi.so), `$SCRIPT_DIR/bisect`. Passes `PERF_BACKEND`, `PERF_MODE=speed`, `PERF_CYCLES`, `PERF_REPEATS`, `PERF_HW` to bisect. |
| Outputs | Under `$OUT`: environment.txt, anchors.txt, debuginfo-neutrality.txt, malloc-interception.txt, redir-probe.{stdout,stderr}, callgrind.redir-probe.out, callgrind.<cell>.r<N>.out (+ .annotated.txt, .inclusive.txt), cg.<cell>.r<N>.{stdout,stderr}, hw-instructions.jsonl + hw-instructions.md, hw.err, alloc-count.txt, speed.jsonl, speed.err, timing.md; final `ls -1`. Exit 1 on missing artifacts/data, folded anchors, neutrality case (c); otherwise 0 (stage 5 continues on missing counters). |
| Platform | container / Linux (valgrind, perf_event_open, taskset) |
| Consumers | Dockerfile.callgrind CMD; runners.json[4] `callgrind-differential` (`kind: not-a-gate`, reason "measurement recipe; excluded from aggregate gates"); debuginfo-neutrality-lib.sh header; bisect.c comments; docs/findings/perf-build-recipe-audit-2026-09-10.md. |
| State | All outputs in `$OUT`; the sourced lib writes fixed `/tmp/neut-*` scratch files. Cells pinned with `taskset -c $CPU`; `LD_LIBRARY_PATH=$LP_LIB` for every run. No locks/timeouts. |
| Overlap | Stage 7 reproduces run-perf-baseline.sh's timing protocol (PERF_CYCLES=8, 20 interleaved runs) by design; stage 6 duplicates run-rss-diag's alloc-counter read; comments duplicate Dockerfile.callgrind's rationale. |
| Constraints | "Nothing here interprets the numbers. The decision rule lives in the accompanying findings document." Stage 7 "deliberately NOT the callgrind pass's cycle count" so timing stays comparable with prior records. Requires `--security-opt label=disable --security-opt seccomp=unconfined` for stage 5 on SELinux hosts. |

## Count

39 records: BIS-A-01 … BIS-A-39 (abi-probe-diff.c, run-abi-probe-diff.sh, addon-candidate-diff.c, run-addon-candidate-diff.sh, axis-c-replacement.json, bigram-export-diff.c, run-bigram-export-diff.sh, bisect.c, run-bisect.sh, candidate-assembly-diff.c, run-candidate-assembly-diff.sh, chewing-diff.c, classic-import.txt, cpp-smoke.cc, run-cpp-smoke.sh, debuginfo-neutrality-lib.sh, dict-surface-diff.c, run-dict-surface-diff.sh, Dockerfile.callgrind, Dockerfile.perf-matrix, dynamic-adjust-diff.c, run-dynamic-adjust-diff.sh, export-diff.c, run-export-diff.sh, frontend-import.cc, fullpin-diff.c, fullpin-tables.h, import-diff.c, run-import-diff.sh, import-semantics-diff.c, run-import-semantics-diff.sh, key-parse-diff.c, key-surface-diff.c, run-key-surface-diff.sh, live-typing-diff.c, run-live-typing-diff.sh, locale-diff.c, run-locale-diff.sh, run-callgrind-differential.sh).

## Architecture notes

Observations (what the code does):

- **Driver → wrapper map.** abi-probe-diff.c→run-abi-probe-diff.sh; addon-candidate-diff.c→run-addon-candidate-diff.sh→run-w11-diff.sh; bigram-export-diff.c→run-bigram-export-diff.sh; bisect.c→run-bisect.sh (+ both Dockerfiles, run-callgrind-differential.sh, perf runners); candidate-assembly-diff.c→run-candidate-assembly-diff.sh and run-nbest-choose-train-diff.sh; chewing-diff.c and fullpin-diff.c→run-scheme-diff.sh (modes `bopomofo` / `full`); cpp-smoke.cc→run-cpp-smoke.sh; dict-surface-diff.c→run-dict-surface-diff.sh; dynamic-adjust-diff.c→run-dynamic-adjust-diff.sh; export-diff.c→run-export-diff.sh; frontend-import.cc→run-import-diff.sh; import-diff.c→run-import-diff.sh; import-semantics-diff.c→run-import-semantics-diff.sh and run-system-import-round-trip.sh; key-parse-diff.c→run-same-data-dir-diff.sh ONLY (no dedicated wrapper, no runners.json name); key-surface-diff.c→run-key-surface-diff.sh; live-typing-diff.c→run-live-typing-diff.sh; locale-diff.c→run-locale-diff.sh. run-same-data-dir-diff.sh additionally builds and runs key-surface, key-parse, dict-surface, addon-candidate, import, live-typing and dynamic-adjust from this range with a uniform `gcc -std=gnu11 -Wall -Wextra -O2 ... -ldl $(pkg-config glib-2.0)` line (no -Werror).
- **Duplicated C boilerplate.** `must()/load_symbol()/resolve_symbol()` (dlsym-or-exit) appears in every driver with three different names and two failure styles (exit immediately vs count `missing`). `resolve_g_free()` (dlopen `libglib-2.0.so.0`, fall back to `free`) is copied verbatim into bisect.c, chewing-diff.c, fullpin-diff.c, import-diff.c, live-typing-diff.c, export-diff.c, import-semantics-diff.c, bigram-export-diff.c; abi-probe-diff.c makes it fatal; candidate-assembly-diff.c uses `RTLD_DEFAULT`; dict-surface-diff.c and locale-diff.c link glib. `rm_rf()` (unlink+rmdir) is copied into abi-probe-diff.c, live-typing-diff.c, candidate-assembly-diff.c; bisect.c uses `rmdir` (surface) and `system("rm -rf")` (perf); import-diff.c `rmdir` only; addon-candidate-diff.c never removes. `ctype_name`/`type_name` and `drive_input` are copied between bisect.c, chewing-diff.c, fullpin-diff.c. Choose-by-(type,text) loops recur in bigram-export, dynamic-adjust, live-typing (`find_by_text`/`find_phrase_by_text`), candidate-assembly (`first_of_type`/`nth_of_type`/`first_single_of_type`), locale-diff (`train_target`). The oracle caller contracts (get_sentence only on proved indices; never set double 30 / zhuyin 7; in-range unload only; no apostrophes in single-key parse) are restated as comments in abi-probe-diff.c, key-surface-diff.c, key-parse-diff.c, live-typing-diff.c, dict-surface-diff.c, chewing-diff.c, fullpin-diff.c.
- **Fixture-init preference.** `oxpinyin_init_for_fixtures` is preferred unconditionally in bisect.c (surface mode), addon-candidate-diff.c, dict-surface-diff.c, key-surface-diff.c, import-diff.c; only when `interpolation2.text` is absent in chewing-diff.c and fullpin-diff.c; never in abi-probe-diff.c, live-typing-diff.c, candidate-assembly-diff.c, locale-diff.c, export-diff.c, import-semantics-diff.c, bigram-export-diff.c, dynamic-adjust-diff.c, key-parse-diff.c.
- **Hard-coded option/sort words.** 0x18a (parity word) in bisect.c (`DEFAULT_FLAGS`), abi-probe-diff.c (`PARITY_WORD`), live-typing-diff.c / dynamic-adjust-diff.c (`PARITY_OPTIONS`), key-surface-diff.c, candidate-assembly-diff.c (R/F cases); sort 0x1e (`DEFAULT_SORT`) in bisect.c, live-typing, fullpin, chewing, dynamic-adjust, bigram-export, import-semantics, addon-candidate; locale-diff.c uses 0x1c; key-parse-diff.c sweeps 12 words; candidate-assembly sweeps {0x0,0x1,0x1e,0x1f} under `USE_TONE` 0x20 / zhuyin 0x60; chewing-diff.c `CHEWING_FLAGS` 0x34 (+FORCE_TONE 0x40).
- **Env var names and aliases.** Oracle prefix: `PINYIN_ORACLE_PREFIX` (default `$HOME/.local/opt/pinyin-oracle` in run-abi-probe/dict-surface/key-surface/dynamic-adjust/import/live-typing/w11/scheme), `LOCALE_DIFF_ORACLE_PREFIX` (falls back to it, no default), `PINYIN_PATCHED_ORACLE_PREFIX` (run-scheme-diff full 2/3); the argv-3 wrappers (bigram-export, export, import-semantics, same-data-dir) take the .so path positionally. Capi system dir per-runner override names: `ABI_PROBE_SYSTEM`, `DICT_SYSTEM`, `KEY_SYSTEM`, `DYNAMIC_ADJUST_SYSTEM`, `IMPORT_SYSTEM`, `LIVETYPING_SYSTEM`, `CAPI_W11_SYSTEM_DIR` (note `_DIR`), `W13_CAPI_SYSTEM`, all falling to `OXPINYIN_SYSTEM_DIR` through `resolve_system_dir`, except DICT_SYSTEM/KEY_SYSTEM which are required outright (no fallback, SKIP 77). Subject libraries: `OXPINYIN_CAPI_SO`, `OXPINYIN_ZHUYIN_SO` (oracle-cell.sh, repo-relative allowed), aliased by `LOCALE_DIFF_PINYIN_SO`/`LOCALE_DIFF_ZHUYIN_SO`. Cell: `PINYIN_ORACLE_DBM` (bdb|kc|tkrzw) → exported `OXPINYIN_CAPI_BACKEND_EXT` (db|kct|tkt), which run-bisect.sh and run-cpp-smoke.sh also read directly. Others: `ABI_PROBE_SORT`, `BIGRAM_DIFF_GATE`, `BISECT_LD_PRELOAD`, `LIVETYPING_ROUNDS`, `SCHEME_DIFF_TONE`, `SCHEME_DIFF_USER_DIR`, `SCHEME_DIFF_PARSE_AUX_ONLY`, `W13_INTERPOLATION`, `CAND_ASSEMBLY_TRAIN_DIR`, `CAND_ASSEMBLY_BASELINE_ONLY`, `LOCALE_DIFF_{LIBS,FORMS,LOCALE,OUT}`, `GLIB_CFLAGS`, `PINYIN_IBUS_BUILD_DIR`, `PINYIN_IBUS_PREFIX`, `OXPINYIN_DICTOOL`, `TMPDIR`, `OXPINYIN_ALLOW_MINI_FIXTURE`, `CARGO_TARGET_DIR`/`OXPINYIN_TARGET_ROOT`; perf: `PERF_BACKEND`, `PERF_MODE`, `PERF_CYCLES`, `PERF_REPEATS`, `PERF_HW`, `PERF_CPU`, `RSS_DIAG_DIR`, `RSS_DIAG_TAG`, `CG_OUT`, `CG_CYCLES`, `CG_REPEATS`, `CG_ROUNDS`, `T_CYCLES`, `T_RUNS`, `T_REPEATS`, `HW_RUNS`, `PERF_RUNS`, `PERF_RAM_RUNS`, `MATRIX_OUT`.
- **Exit-code conventions are inconsistent.** 0/77/1/2 (identical/skip/failure/divergence): run-abi-probe, run-dynamic-adjust (+3 vacuous), run-import, run-dict-surface (+3 inactive), run-key-surface (+3 inactive), run-scheme-diff, run-w11 (but off-pin → 1 not 77). Skip exits 0: run-live-typing-diff.sh only (so it can be a CI step). Skip 77 only for a missing .so but no pin check: run-export, run-import-semantics, run-bigram-export (patch-marker check → 1). run-locale-diff: 77 unset prefix, 3 missing input, 1/2. run-bisect: 0/1/2(unused)/3 valgrind/4 LD_PRELOAD. run-candidate-assembly: 0/77/1/2. run-cpp-smoke: 0/1 only. Several headers still say "Absent -> skip ... exit 0" or "Skip → 0" while the code exits 77 (run-dynamic-adjust, run-import, run-key-surface).
- **Pin-manifest (oracle-pin.txt) parsing, every place:** `grep -Fxq "$EXPECTED_PIN_REF"` (exact line from oracle-cell.sh): run-abi-probe, run-dict-surface, run-key-surface, run-import, run-live-typing, run-w11, run-scheme-diff (patched prefix, `+patches-<sha>`); run-import additionally greps `^model_sha256=` (read from build-oracle.sh) and `-x dbm=<name>`; literal prefix grep `^pin_ref=libpinyin-2.11.92-074a2219c90feaf962d0d24f034514033ece5f99` (ignores model/dbm suffix): run-candidate-assembly, run-locale-diff, run-scheme-diff (unpatched), run-import (IBUS_PIN_REF); existence only: run-dynamic-adjust; none: run-bisect, run-export, run-import-semantics, run-bigram-export (checks `oracle-patches.sha256` instead), run-cpp-smoke, run-callgrind. The libpinyin SHA is hard-coded literally in run-candidate-assembly, run-locale-diff, run-scheme-diff, run-import, Dockerfile.callgrind, Dockerfile.perf-matrix, as well as build-oracle.sh (the declared single source oracle-cell.sh reads). table.conf is read by run-locale-diff.sh (`head -n 2` for the truncated fixture) and by system-dir.sh (`database format:`); datagen-manifest.txt `backend=` by system-dir.sh (`system_dir_data_ext`, used by run-dict-surface, run-key-surface, run-import).
- **Backend names/extensions hard-coded:** `kct|tkt|db` case lists in run-bisect.sh and run-cpp-smoke.sh (duplicated block, "db first since 2026-09-20"); `fixtures/w3/<ext>` layout in both; oracle-cell.sh maps bdb/kc/tkrzw→BerkeleyDB/KyotoCabinet/Tkrzw→bdb/kyotocabinet/tkrzw features→db/kct/tkt; Dockerfiles hard-code `--with-dbm=Tkrzw|KyotoCabinet`, `--features tkrzw|kyotocabinet`, `/opt/libpinyin-{tkrzw,kc}`, `/opt/oxpinyin-{tkrzw,kc,nodebug,alloccount}`; run-callgrind-differential.sh hard-codes the tkrzw paths; file names `bigram.db`, `pinyin_index.bin`, `phrase_index.bin`, `punct.bin`, `gb_char.bin`, `interpolation2.text` appear in run-abi-probe/dict-surface/key-surface/import/live-typing/same-data-dir checks.
- **Cargo spawns:** only through `oracle_cell_artifact` (oracle-cell.sh: `cargo build --locked --manifest-path $REPO_ROOT/Cargo.toml -p <pkg> --no-default-features --features $CAPI_FEATURE`, with `CARGO_PROFILE_DEV_OPT_LEVEL=1` exported and `CARGO_TARGET_DIR=target/cells/<cell>`) in run-abi-probe, run-bisect, run-cpp-smoke, run-dict-surface, run-dynamic-adjust, run-import (twice: capi + oxpinyin-dictool), run-key-surface, run-live-typing, run-w11, run-scheme-diff; and the Dockerfiles' `cargo cinstall`/`cargo run -p oxpinyin-datagen`/`cargo install cargo-c`. The argv-3 wrappers and run-locale/run-candidate-assembly/run-callgrind never spawn cargo.
- **Process control / timeouts / locks:** none in-script; runners.json gives every entry `timeout: 900` and marks source-tree-writing runners with `exclusive_resources: bisection-source-output` (run-abi-probe, run-bisect, run-import, run-live-typing, run-w11 users, run-scheme-diff — all `gcc -o <name>` into tools/bisection/, matching the .gitignore list) or `surface-native-output` (run-dict-surface, run-key-surface). run-bisect.sh's LD_PRELOAD probe backgrounds the ibus engine, `sleep 2`, `kill`. run-callgrind uses `taskset -c`. debuginfo-neutrality-lib.sh writes fixed-name `/tmp/neut-*` files (collision-prone). Several drivers create `/tmp/<name>-XXXXXX` user dirs themselves (abi-probe, bisect, live-typing, import, addon-candidate) while others rely on cwd (`""` or `./user`) and the wrapper's per-side mktemp cwd (dict-surface, key-surface, key-parse, export, import-semantics, bigram-export, locale).
- **Frozen / "do not change" markers:** Dockerfiles: "Do not pin --platform", "Any override MUST carry this same digest", "Never build or run this emulated", "NOT stripped" (callgrind); bisect.c anchors must stay distinct noinline externs (build-time check); candidate-assembly wrapper: declared table "cannot go stale silently ... re-measure, do not just edit the table"; run-addon-candidate: "Does not edit run-import-diff.sh or run-train-diff.sh"; fullpin-tables.h "keep in sync with crates/oxpinyin-core/src/full_pinyin_index.rs"; chewing-diff.c tables "copied verbatim from zhuyin_table.h at the pin 0c5e80e1" and self-verified at runtime; axis-c-replacement.json format tag + source digests asserted by contract-diff.py; check-alloc-pairing.sh deliberately duplicates run-cpp-smoke.sh's fixture prep "so that adding this gate cannot break the existing one".

Interpretation (consolidation leads):

- A single shared C header (dlsym-or-die, g_free resolution, rm_rf, candidate-type names, parity constants) would remove ~150 duplicated lines across the drivers; a shared bash prologue (`cd`, source helpers, pin check, capi resolve, log mktemp, diff verdict) would collapse the six near-identical "env-gated" wrappers, and would let the inconsistent SKIP code (0 vs 77) and the three different pin-check strengths be unified.
- The literal 074a2219 SHA in five scripts and two Dockerfiles contradicts oracle-cell.sh's stated goal that build-oracle.sh is "the one place they live"; run-candidate-assembly and run-locale-diff would accept a prefix built against a different model archive or DBM.
- fullpin-tables.h (pin 0c5e80e1), chewing-diff.c's tables (pin 0c5e80e1) and key-parse-diff.c's SYLLABLES are three hand-copied parser tables that only chewing-diff.c verifies at runtime.
- key-parse-diff.c is orphaned from the gate registry: only run-same-data-dir-diff.sh builds it.
- Dockerfile.callgrind and Dockerfile.perf-matrix share ~100 verbatim lines (and tools/oracle/Dockerfile.oracle shares the base/apt/rustup block); a common base stage would keep the digest/snapshot/pin in one place.


---

# BIS-B — tools/bisection/ files m..z (non-Python; excluding runners.json, run-all.sh, oracle-cell.sh, system-dir.sh)

Tree read: detached worktree at origin/main, commit e67f19d1c3108ec47631ce57d90a7c4a8576eba3. Every file below was read in full.

Shared facts referenced by many records (to keep tables short):

- **Standard oracle gate** = `PREFIX="${PINYIN_ORACLE_PREFIX:-$HOME/.local/opt/pinyin-oracle}"`, `ORACLE_SO=$PREFIX/lib/libpinyin.so`, `ORACLE_DATA=$PREFIX/lib/libpinyin/data`; missing `$PREFIX/oracle-pin.txt` or `.so` → `SKIP`, exit 77; then `grep -Fxq "$EXPECTED_PIN_REF" oracle-pin.txt` (cell-aware line from oracle-cell.sh: `pin_ref=libpinyin-<ver>-<sha>+model20-<sha>+dbm-<cell>`) else `SKIP` 77.
- **Hard-coded pin grep** = `grep -q '^pin_ref=libpinyin-2.11.92-074a2219c90feaf962d0d24f034514033ece5f99'` (prefix match, not cell-aware, not model-checksum-aware).
- **Standard driver build** = `gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o <name> <name>.c -ldl`.
- **oracle_cell_artifact** (oracle-cell.sh): if `$OXPINYIN_CAPI_SO` unset → `cargo build --locked -p oxpinyin-capi --no-default-features --features $CAPI_FEATURE` into `$CARGO_TARGET_DIR` (default `target/cells/<cell>`, `CARGO_PROFILE_DEV_OPT_LEVEL=1`), artifact `$CARGO_TARGET_DIR/debug/libpinyin_capi.so`; else use the prebuilt path (repo-relative made absolute); missing file → return 1.
- **resolve_system_dir VAR label** (system-dir.sh): `$VAR` → `$OXPINYIN_SYSTEM_DIR` → `target/datagen/<ext>` → `/tmp/oxpinyin-export` (same backend only) → `fixtures/w3/<ext>` only with `OXPINYIN_ALLOW_MINI_FIXTURE=1`; else fatal exit 3. Backend ext from `OXPINYIN_CAPI_BACKEND_EXT` (exported by oracle-cell.sh) else `cargo tree --offline`. Validates completeness and `datagen-manifest.txt backend=` / `table.conf database format:`.
- **C-ABI opaque-dlopen pattern** = `dlopen(argv[1], RTLD_NOW)`, every symbol via `dlsym`, missing symbol → `MISSING <name>` on stderr, exit 1; `typedef void` for context/instance/candidate; no libpinyin headers included.
- None of the wrappers in this range sources `debuginfo-neutrality-lib.sh` (its only sourcers are run-callgrind-differential.sh and tools/env-probe/probe-debuginfo-neutrality.sh).
- No GitHub workflow invokes any file in this range (ci.yml runs only run-cpp-smoke.sh and run-live-typing-diff.sh). The only registry is `runners.json` driven by `run-all.py` (PASS 0 / SKIPPED 77 / FAIL otherwise, per-runner timeout).

---

### BIS-B-01 — tools/bisection/nbest-train-diff.c

| Field | Value |
|---|---|
| Entry point | `NBESTTRAINDIFF_USER_ROUNDS=<1..8> NBESTTRAINDIFF_ROUNDS=<1..8> ./nbest-train-diff <path-to-so> <systemdir>`; built by run-nbest-train-diff.sh and run-same-data-dir-diff.sh |
| Owning subsystem | bisection |
| Purpose | W14: do trained user bigrams move the C-ABI n-best sentence rows the same way on both libraries, and does the selection record land the chosen row's own token path (你浩 doubling, sentence-surface.md §8/§9)? Also checks save→fini→reopen persistence. |
| Implementation | C (gnu11), 589 lines; `<dlfcn.h>`, `<dirent.h>`; no glib headers — `g_free` resolved via `dlopen("libglib-2.0.so.0")` with `free` fallback |
| Inputs | argv: .so path, systemdir (must hold interpolation2.text — "the fallback mini fixtures answer no n-best cost data"); env `NBESTTRAINDIFF_ROUNDS` (default 3, strict strtol 1..8), `NBESTTRAINDIFF_USER_ROUNDS` (default 1, same parsing) |
| Outputs | stdout lines: `probe:<label> guess=<0/1>`, `probe:<label> sentence[i]=<text or ->` (rows 0..2), `probe:<label> n=<N>`, `probe:<label> cand[i]=<TYPE>/<nbest-idx or ->/<text>` (first 8), then `phrase: <p>\|<py>\|<count>` and `bigram: <p>\|<py>\|<count>` export rows (twice: in-memory and after reopen). Labels: baseline, user-only, augmented, reopened. stderr diagnostics. Exit 0 complete walk / 1 any failure. No in-process comparison; external diff. No hand-written expected values. |
| Platform | Linux-only (dlopen, /tmp mkdtemp, glib soname) |
| Consumers | run-nbest-train-diff.sh (builds it in-source as `./nbest-train-diff`); run-same-data-dir-diff.sh (default driver list, builds into mktemp work dir); runners.json `nbest-train-diff`; cited by docs/findings/live-typing.md, sentence-surface.md, tkrzw-distro-compat.md; .gitignore lists the binary |
| State | userdir `mkdtemp("/tmp/nbesttraindiff-user-XXXXXX")` (ignores TMPDIR); `rm_rf` on every exit path (flat unlink + rmdir); one bigram export per context (pin UAF when repeated) |
| Overlap | Loads the same 24-symbol set as train-diff.c (resolve_all nearly identical); `rm_rf`, `find_by_text`, `type_name`, `resolve_g_free` duplicated verbatim in train-diff.c / uncovered-surface-diff.c / pred-order-diff.c / residue-*; `train_pair` mirrors train-diff.c Phase 1 round |
| Constraints | ABI symbols exercised: pinyin_init/fini/save/alloc_instance/free_instance, parse_more_full_pinyins, guess_sentence, guess_candidates(0x1e), get_n_candidate, get_candidate, get_candidate_type, get_candidate_string, get_candidate_nbest_index (NBEST rows only — pin asserts), get_sentence (rows 0..2 only), choose_candidate, train(0), reset, begin/iterator_has_next/iterator_get_next/end_get_phrases(7), begin/bigram_iterator_has_next/bigram_iterator_get_next/end_get_bigram_phrases. Comment: upstream bigram get_next returns "more rows follow", not success. Known edge: capi guess_sentence after full consumption answers false vs oracle true — return deliberately unchecked. |

### BIS-B-02 — tools/bisection/run-nbest-train-diff.sh

| Field | Value |
|---|---|
| Entry point | executed, no args; `bash tools/bisection/run-nbest-train-diff.sh` |
| Owning subsystem | bisection |
| Purpose | Builds nbest-train-diff, runs it against capi and oracle, requires the NBEST row path active on both sides, diffs the FULL logs (probes + export). |
| Implementation | bash, 141 lines; gcc; sources oracle-cell.sh and system-dir.sh; cargo via oracle_cell_artifact |
| Inputs | env `PINYIN_ORACLE_PREFIX`, `PINYIN_ORACLE_DBM` (via oracle-cell), `OXPINYIN_CAPI_SO`, `NBEST_CAPI_SYSTEM` then `OXPINYIN_SYSTEM_DIR` (must hold interpolation2.text and core tables per `system_dir_detect_ext`), `NBESTTRAINDIFF_USER_ROUNDS` (wrapper default 3 — differs from the driver's own default 1), `NBESTTRAINDIFF_ROUNDS` (3), `CARGO_TARGET_DIR`/`OXPINYIN_TARGET_ROOT`, `OXPINYIN_CAPI_BACKEND_EXT` |
| Outputs | stdout progress; on IDENTICAL prints capi `phrase:`/`bigram:` lines; exit 0 identical / 77 skip / 1 build-run or prerequisite (zero `cand[0]=NBEST` on either side) / 2 divergence (diff -u printed) |
| Platform | Linux-only |
| Consumers | runners.json `nbest-train-diff` (gate, issues W14, all 3 cells, timeout 900, exclusive `bisection-source-output`) |
| State | driver binary written into tools/bisection; mktemp logs removed; driver stderr discarded (`2>/dev/null`) |
| Overlap | Standard oracle gate + `bigram.db` data check duplicated with run-train-diff.sh, run-residue-*.sh, run-uncovered-surface-diff.sh |
| Constraints | Standard build flags; standard oracle gate (cell-aware); additional `[[ -f $ORACLE_DATA/bigram.db ]]` (hard-coded libpinyin bigram name); skip when system dir unresolved (77), not fatal 3 — does NOT call resolve_system_dir, only `system_dir_detect_ext` |

### BIS-B-03 — tools/bisection/run-nbest-choose-train-diff.sh

| Field | Value |
|---|---|
| Entry point | `run-nbest-choose-train-diff.sh <bdb\|kc\|tkrzw> <oracle-prefix> <pinyin-so> <zhuyin-so> <new-out-dir>` |
| Owning subsystem | bisection |
| Purpose | Lane I: for every offered n-best candidate on both facades (pinyin, zhuyin) with fresh profiles, diff driver logs and compare saved training state (bigram records, pronunciation counts, logs) via compare-nbest-training-state.py. |
| Implementation | bash, 53 lines; gcc; python3 helper; sources oracle-cell.sh (sets `PINYIN_ORACLE_DBM=$1` before sourcing) |
| Inputs | argv 5 positional (all required); env `TMPDIR`/`CAND_ASSEMBLY_TRAIN_DIR` set per side; inputs checked: `$prefix/oracle-pin.txt`, `lib/libpinyin.so`, `lib/libzhuyin.so`, `lib/libpinyin/data/bigram.db`, both subject .so |
| Outputs | `$out/driver`, `$out/<mode>-<side>/driver.log|driver.err` kept for review; diff -u to stdout; exit 0 / 77 (missing oracle) / 1 (missing input, bad cell) / 2 (log diff) or compare script's status |
| Platform | Linux-only |
| Consumers | runners.json `nbest-choose-train-diff` (gate, issue row:38, cells bdb/kc/tkrzw, timeout 900, command `{cell} {prefix} {capi} {zhuyin} {work}/nbest`); cited by docs/findings/compatibility-policy.md |
| State | `mkdir "$out"` must be new; snapshots retained in out |
| Overlap | Builds **candidate-assembly-diff.c** (another agent's driver), not a driver of its own; calls compare-nbest-training-state.py |
| Constraints | Standard build flags into `$out/driver`; hard-coded pin grep (not cell-aware, though cell is an argument); no system-dir.sh — both sides read the oracle's own data dir; no cargo |

### BIS-B-04 — tools/bisection/open-counter-diff.c

| Field | Value |
|---|---|
| Entry point | `open-counter-diff <pinyin\|zhuyin> <lib.so> <systemdir> <userdir> <launch-N> <cycle\|crash\|look>` — one process = one IME launch |
| Owning subsystem | bisection |
| Purpose | #523: does oxpinyin move user.conf's open counter, wipe the profile above OPEN_COUNTER_LIMIT, and keep learned state across launches exactly as the pin (and libzhuyin's never-raise law)? |
| Implementation | C (gnu11), 468 lines; `<dlfcn.h>`, `<signal.h>`, **`<glib.h>` linked directly** (GArray, GString, g_utf8_strlen) |
| Inputs | argv as above; launch k picks target row `(k-1) % 14` from hand-written tables PINYIN_TARGETS/ZHUYIN_TARGETS/PINYIN_IMPORTS; reads `<userdir>/user.conf` bytes in `look` mode |
| Outputs | stdout: `launch: N mode`, `init: ok/NULL`, `targets@init: n` + `row@init\tT\t<word>\t<token>\t<freq>`, `phrases@init: n` + `row@..\tP\t..`, `ranks@..\tR\t<typed>\t<sentence>\t<listed>` (zhuyin), `import: <word> <0/1>`, `train: <word> <ok\|no-list\|no-candidate\|choose-failed\|reguess-failed\|train-false>`, `save: <0/1>`, `bigrams@saved` rows `B`, `user.conf@init: N bytes` + `conf@init: <line>`, `fini`, `crash: SIGKILL before fini`. Exit 0 / 1 init-alloc fail / 2 usage; crash mode dies by SIGKILL (137). External diff only; no in-process compare. |
| Platform | Linux-only |
| Consumers | run-open-counter-diff.sh, run-user-file-modes-diff.sh (both build it into mktemp WORK); referenced by crates/oxpinyin-data/src/user_files.rs, docs/findings/user-store.md, compatibility-policy.md, upstream-divergences.md; two-context-diff.c and session-fork-diff.c copy its tables/functions |
| State | userdir supplied by caller (persists across launches by design); no temp files of its own |
| Overlap | `train_target`, `list_candidates`, `phrase_rows`, `target_rows`, `import_phrase`, `sym("<prefix>_<name>")` duplicated in session-fork-diff.c and two-context-diff.c; `dump_bigram`-like `bigram_rows` |
| Constraints | Symbols via `"<pinyin\|zhuyin>_<name>"`: init, fini, save, alloc_instance, free_instance, lookup_tokens, token_get_unigram_frequency, parse_more_full_pinyins / parse_more_chewings, guess_sentence, guess_candidates(0x1c) / guess_candidates_after_cursor, get_n_candidate, get_candidate, get_candidate_type, get_candidate_string, reset, begin/iterator_has_next/iterator_get_next/end_get_phrases, begin/..._get_bigram_phrases, begin_add_phrases / iterator_add_phrase / end_add_phrases, choose_candidate, train (pinyin takes index 0, zhuyin none). Rules in comments: learned state read in-process (an extra init would step the counter); bigram export once per context (pin class-(b) UAF); `_Static_assert` table sizes equal; `IMPORT_COUNT 5` explicit (−1 would mean 5). |

### BIS-B-05 — tools/bisection/run-open-counter-diff.sh

| Field | Value |
|---|---|
| Entry point | executed, no args |
| Owning subsystem | bisection |
| Purpose | #523/#583/#589/#590: byte-diff the per-launch logs + user.conf + dir inventory for protocols cycle, crash, seeded (24 counter seeds + 13 whole-marker `conf-*` cases) and an abort expectation channel (5 cases where the pin `abort()`s). |
| Implementation | bash, 524 lines; gcc + `pkg-config --cflags --libs glib-2.0`; awk summaries; sha256sum; sources oracle-cell.sh (needs REPO_ROOT) |
| Inputs | env `OPEN_COUNTER_ORACLE_PREFIX` (falls back to `PINYIN_ORACLE_PREFIX`), `OPEN_COUNTER_PINYIN_SO` (→ `OXPINYIN_CAPI_SO` → `$CARGO_TARGET_DIR/debug/libpinyin_capi.so`), `OPEN_COUNTER_ZHUYIN_SO` (→ `OXPINYIN_ZHUYIN_SO` → `.../libzhuyin_capi.so`), `OPEN_COUNTER_LIBS` ("pinyin zhuyin"), `OPEN_COUNTER_PROTOCOLS` (code default "cycle crash seeded abort"; header says "cycle crash seeded"), `OPEN_COUNTER_CYCLES` (10), `OPEN_COUNTER_CRASHES` (10), `OPEN_COUNTER_OUT`; oracle libs `lib/libpinyin.so.15`, `lib/libzhuyin.so.15`, `lib/libpinyin/data/table.conf` |
| Outputs | `$OUT/<lib>-<protocol>-{oracle,oxpinyin}.log(.stderr)`, `$OUT/<lib>-<protocol>.diff`, abort logs; stdout summaries (`launch N mode learned@init=… counter=… WIPE`, seed_summary lines), `RESULT:` line; exit 0 / 77 (prefix var unset — header says "never skips", code exits 77) / 3 off-pin or missing input / 1 launch failed, vacuous (no `save: 1`), diff error / 2 divergence or abort expectation unmet |
| Platform | Linux-only (SIGKILL 137 status, pkg-config glib, GNU find -printf) |
| Consumers | runners.json `open-counter-diff` (gate, cells bdb/kc/tkrzw, timeout 1800, env OPEN_COUNTER_ORACLE_PREFIX/PINYIN_SO/ZHUYIN_SO from `{prefix}/{capi}/{zhuyin}`); referenced by user_files.rs, user-store.md, compatibility-policy.md, upstream-divergences.md; run-two-context-diff.sh and run-user-file-modes-diff.sh cite it |
| State | OUT (kept if OPEN_COUNTER_OUT) + WORK mktemp (trap rm); per-side `TMPDIR` set to private dir; subshell reaps SIGKILLed driver; per-protocol fresh user dir |
| Overlap | Oracle-prefix/so resolution block, `OUT`/`WORK` trap pattern, `ORACLE_SO[...]`/`OX_SO[...]` assoc arrays duplicated with run-two-context-diff.sh, run-user-file-modes-diff.sh, run-session-fork-diff.sh |
| Constraints | Build `gcc -std=gnu11 -Wall -Wextra -Werror -O2 ... $(pkg-config --cflags --libs glib-2.0) -ldl` into `$WORK`; hard-coded pin grep; no system-dir.sh (both sides on `$PREFIX/lib/libpinyin/data`); hand-written expectations: glibc `%d` reads documented per seed (comments), abort cases require oracle exit 134, ox `init: NULL` + exit 1 + exactly one `check_format: unknown database format in user.conf` stderr line, dir digest unchanged; `[missing]=''` key duplicated in SEED_LINE |

### BIS-B-06 — tools/bisection/option-sweep.c

| Field | Value |
|---|---|
| Entry point | `OPTION_SWEEP_SORT=<hex> ./option-sweep <so> <systemdir> <case-name> <options-hex>` |
| Owning subsystem | bisection |
| Purpose | W10: for one option word, do parse length, auxiliary text and the top-10 candidate TEXT/ORDER agree for 30 fixed inputs (correction/fuzzy triggers + xian/fanan/fangan resplit class)? |
| Implementation | C (gnu11), 240 lines; dlopen; g_free via glib dlopen |
| Inputs | argv 4; env `OPTION_SWEEP_SORT` (hex sort word, default 0x1e, strict strtoul); `oxpinyin_init_for_fixtures` used instead of `pinyin_init` when present AND systemdir lacks interpolation2.text |
| Outputs | stdout `sort=0x..`, `case=<name> options=0x%08lx`, per input: `input=<s> consumed=<n> parsed=<n>`, `aux=<text>\|false`, `guess=false n=0` or `n=<N>` + `cand[i]=<text>` (≤10). Exit 0 / 1. External diff. |
| Platform | Linux-only |
| Consumers | run-option-sweep.sh (in-source `./option-sweep`); runners.json `option-sweep`; cited by crates/pinyin-oracle/src/bin/corpus_tail.rs, docs/findings/option-bits.md, all-off-tails.md, revert-plan.md, bug-for-bug-audit-r2, docs/testing/corpus-tail.md, abi-probe-diff.c; system-dir.sh mentions the 2026-08-28 misfire |
| State | `mkdtemp("/tmp/option-sweep-user-XXXXXX")`, only `rmdir` at end (leaks if the engine wrote files) |
| Overlap | Symbol set subset of scheme-diff.c; fixture-init rule shared with scheme-diff.c/train-diff.c |
| Constraints | Symbols: pinyin_init, oxpinyin_init_for_fixtures (optional), fini, alloc/free_instance, set_options, parse_more_full_pinyins, get_parsed_input_length, get_full_pinyin_auxiliary_text, guess_candidates, get_n_candidate, get_candidate, get_candidate_string, reset. Comment: capi ignores every sort bit but 0x1 today; this sweep measures that gap. |

### BIS-B-07 — tools/bisection/run-option-sweep.sh

| Field | Value |
|---|---|
| Entry point | executed, no args |
| Owning subsystem | bisection |
| Purpose | Runs option-sweep per case (baseline 0x188, fork-default 0x1fe00198, correct-21..28, amb-10..19, amb-chain, all-off 0, divided-contrast-off/on) on both libraries; parse/aux must be identical; candidate TEXT/ORDER classified identical / tie-order (same set, phrase_length tied) / w12-residual (set tail on cang,sang,lve,lue,agn,amg,ang) / STOP. |
| Implementation | bash, 344 lines; gcc; awk; sources system-dir.sh then oracle-cell.sh; cargo via oracle_cell_artifact |
| Inputs | env `PINYIN_ORACLE_PREFIX`, `OPTION_SWEEP_CAPI_DATA` (validated by `system_dir_require_complete`), `OXPINYIN_SYSTEM_DIR`, `PINYIN_MODEL_DIR` (for interpolation2.text graft), `OXPINYIN_CAPI_SO`, cell vars; conventional `/tmp/oxpinyin-export` + `target/model20/extracted/interpolation2.text` |
| Outputs | per case `PASS/FAIL <name> 0x<opts> [parse]` + notes; final `option-sweep: PASS` / `STOP`; exit 0 / 77 skip / 1 crash / 2 parse divergence or STOP / 3 incomplete data |
| Platform | Linux-only |
| Consumers | runners.json `option-sweep` (gate, W10/row:17, exclusive bisection-source-output); docs/findings/option-bits.md, all-off-tails.md, revert-plan.md, corpus-tail.md |
| State | in-source binary; mktemp logs; `mktemp -d /tmp/option-sweep-capi-data-XXXXXX` assembled data dir never removed; driver stderr discarded |
| Overlap | Standard oracle gate; system-dir graft logic also in run-train-diff-dynamic-off.sh; `phrase_len` via `grep -o .` locale-dependent |
| Constraints | Standard build flags; cell-aware pin grep; runner does not diff `n=` lines (closure record cites them separately); W12 residual set hard-coded |

### BIS-B-08 — tools/bisection/patches/bigram-export-strjoinv/ (pin-bigram-export-strjoinv.patch)

| Field | Value |
|---|---|
| Entry point | Applied by `tools/oracle/build-oracle.sh --dbm <cell> --apply-patches tools/bisection/patches/bigram-export-strjoinv --prefix <patched-prefix>`; selected by runners.json `oracle_variant: bigram-export-strjoinv` and `oracle_cell_resolve_prefix` (prefix `<base>-bigram-export-strjoinv`) |
| Owning subsystem | oracle provisioning |
| Purpose | Instrumentation patch to the pin's src/pinyin.cpp: adds `g_ptr_array_add(pinyins, NULL)` before each `g_ptr_array_free(..., FALSE)` in `pinyin_bigram_iterator_has_next_phrase` (:843-849, :857-863) so `g_strjoinv` no longer over-reads (class (b) UB) — lets full bigram-export walks run against the pin. |
| Implementation | unified diff, 40 lines (prose header + 2 hunks); one file in the directory, no manifest/README |
| Inputs | pinned libpinyin source tree at 074a2219 |
| Outputs | A prefix whose oracle-pin.txt carries `pin_ref=…+patches-<sha256 of sorted *.patch manifest>` and `patches_manifest_sha256=<digest>`, plus `oracle-patches.sha256` listing the patch |
| Platform | Linux (oracle build) |
| Consumers | run-residue-a-tail-diff.sh (hard requirement, recomputes digest), run-bigram-export-diff.sh, oracle-cell.sh `oracle_cell_resolve_prefix`, runners.json rows 2 and 27 (`oracle_variant`), docs/runbooks/parity-gates.md, docs/findings/abi-allocator-pairing.md, compatibility-policy.md, tooling-lane-gates.md, upstream-divergences.md |
| State | none |
| Overlap | tools/oracle/patches/ (fullpin-aux-overread.patch, used by run-scheme-diff.sh full 2/3) is a second, separate patch set with the same digest convention |
| Constraints | Header: "changes no control flow, no DB access and no iteration"; a patched prefix "must use its own --prefix"; digest = `find . -maxdepth 1 -name '*.patch' -print0 \| sort -z \| xargs -0 sha256sum \| sha256sum` |

### BIS-B-09 — tools/bisection/patches/nbest-tails-dump/ (pin-nbest-tails-dump.patch)

| Field | Value |
|---|---|
| Entry point | `build-oracle.sh --apply-patches tools/bisection/patches/nbest-tails-dump --prefix <dump-prefix>`; used via `RESIDUE_A_DUMP_PREFIX` in run-residue-a-tail-diff.sh with `OXPINYIN_DUMP_TAILS=1` |
| Owning subsystem | oracle provisioning |
| Purpose | Diagnosis instrument for residue A: env-gated (`OXPINYIN_DUMP_TAILS`) stderr dump inside src/lookup/phonetic_lookup.h — per unigram step of USER_DICTIONARY tokens (`unigram_step: start= end= token= unigram= total= elem_poss= pinyin_poss= cur_poss= next_poss=`), `nbest: nstep= total_freq= bigram_lambda= unigram_lambda=` and `tail[i] m_poss= m_sentence_length= m_last_step= m_sub_index= handles= result: k:token…`. |
| Implementation | git diff, 59 lines, 3 hunks; adds `<stdio.h>`, `<stdlib.h>` |
| Inputs | pin source; runtime env `OXPINYIN_DUMP_TAILS` |
| Outputs | stderr only; stdout must stay byte-identical to the unpatched pin (run-residue-a-tail-diff.sh checks) |
| Platform | Linux |
| Consumers | run-residue-a-tail-diff.sh (optional `RESIDUE_A_DUMP_PREFIX`), docs/findings/probe-coverage-abi.md (§"A — common-root experiment"); not referenced by runners.json or oracle-cell.sh |
| State | none |
| Overlap | Counterpart of `crates/oxpinyin-runtime/examples/nbest_tail_probe` on the Rust side |
| Constraints | No header prose in the patch (bare git diff); no manifest; expected-ref convention unknown to oracle-cell.sh (`oracle_cell_resolve_prefix` only knows `bigram-export-strjoinv`) |

### BIS-B-10 — tools/bisection/patches/residue-a-counterfactual/ (nbest-step-costs-user-token.patch)

| Field | Value |
|---|---|
| Entry point | `patch -p1` / `git apply` onto a scratch oxpinyin checkout (crates/oxpinyin-data/src/lm/mod.rs), build to `target/cf`, run via `RESIDUE_A_CAPI_SO`, then `patch -R` |
| Owning subsystem | bisection (experiment) |
| Purpose | Counterfactual for the residue-A common-root experiment: replaces the "no system unigram → default NbestStepCosts" gate with `unwrap_or(0)` so a user-only token is priced from its user delta, to probe consequences for residues B and C. |
| Implementation | unified diff against Rust, 32 lines, 1 hunk |
| Inputs | oxpinyin source |
| Outputs | a scratch libpinyin_capi.so |
| Platform | portable (Rust) |
| Consumers | docs/findings/probe-coverage-abi.md only; no script applies it |
| State | none |
| Overlap | — |
| Constraints | Header: "NEVER part of the shipped tree … It is a probe of A's consequences … not the fix shape"; residue A since closed (register #35, `fix/nbest-step-cost-user-token`) so the hunk context may no longer apply |

### BIS-B-11 — tools/bisection/phrase-surface-diff.c

| Field | Value |
|---|---|
| Entry point | `./phrase-surface-diff <so> <systemdir>` |
| Owning subsystem | bisection |
| Purpose | Tier-B ABI: `pinyin_phrase_segment` + `pinyin_get_n_phrase` + `pinyin_get_phrase_token` token-array shape (11 sentences incl. empty, ASCII, invalid UTF-8), fresh/reset state, and `pinyin_guess_sentence_with_prefix` row-0 text for 5 prefixes over "nihaoshijie" plus "nihao"/"". The only oracle coverage for these 5 symbols. |
| Implementation | C (gnu11), 173 lines; dlopen; no glib; sentence text from get_sentence never freed |
| Inputs | argv 2; prefers `oxpinyin_init_for_fixtures` unconditionally when exported; user dir `""` (cwd) |
| Outputs | stdout `segment\|<s>\|<ok>\|n=<N>\|nok=<ok>\|i:token:got …`, `fresh\|n=\|nok=`, `reset\|rok=\|n=\|nok=`, `prefix\|<input>\|<prefix>\|<ok>\|sok=<ok>\|<text>`; exit 0/1; external diff |
| Platform | Linux-only |
| Consumers | run-phrase-surface-diff.sh (mktemp dir), run-same-data-dir-diff.sh, gcov_measure.py (names list), runners.json `phrase-surface-diff`; docs/findings/axis-j-*-ledger.md, redb-removal-audit |
| State | writes user.conf into cwd (hence wrappers cd into private dirs) |
| Overlap | `must()` helper identical to predict/punct/user-candidate/union |
| Constraints | Symbols: pinyin_init/oxpinyin_init_for_fixtures, fini, alloc/free_instance, parse_more_full_pinyins, phrase_segment, get_n_phrase, get_phrase_token, guess_sentence_with_prefix, get_sentence(0 only — pin aborts past rows), reset |

### BIS-B-12 — tools/bisection/run-phrase-surface-diff.sh

| Field | Value |
|---|---|
| Entry point | executed, no args |
| Owning subsystem | bisection |
| Purpose | Build/run phrase-surface-diff on capi (PHRASE_SYSTEM) and oracle in private cwds; require probe families active; diff. |
| Implementation | bash (`set -u` only, no -e), 165 lines; `cc -O2 -Wall` (no -Wextra/-Werror); sources oracle-cell.sh + system-dir.sh; `git rev-parse --show-toplevel` for REPO_ROOT; cargo via oracle_cell_artifact |
| Inputs | env `PINYIN_ORACLE_PREFIX`, `PHRASE_SYSTEM` (relative anchored to REPO_ROOT; requires pinyin_index.bin, phrase_index.bin, bigram.db, interpolation2.text — hard-coded native names), `PINYIN_ORACLE_DBM`, `OXPINYIN_CAPI_SO`, `OXPINYIN_CAPI_BACKEND_EXT` |
| Outputs | exit 0 identical / 77 no oracle, off-pin, PHRASE_SYSTEM missing tables / 1 build-crash, backend unknown or BACKEND MISMATCH (`system_dir_data_ext` vs `OXPINYIN_CAPI_BACKEND_EXT`), diff error / 2 divergence / 3 probe family inactive |
| Platform | Linux-only |
| Consumers | runners.json `phrase-surface-diff` (gate, coverage:68-70, env `PHRASE_SYSTEM={data}`, exclusive `surface-native-output`) |
| State | DRIVER_DIR, CAPI_CWD, ORACLE_CWD, logs all mktemp, trap rm |
| Overlap | Hand-written vacuity probes (like run-uncovered-surface-diff.sh, run-nbest-train-diff.sh) |
| Constraints | Expected lines: `segment\|你好中国\|1\|n=4\|`, `segment\|你好，世界。\|0\|n=6\|`, `prefix\|nihaoshijie\|你好\|1\|`, `reset\|rok=1\|n=0\|`; uses `system_dir_data_ext`, not resolve_system_dir |

### BIS-B-13 — tools/bisection/plant-oracle-bigram.cc

| Field | Value |
|---|---|
| Entry point | `plant-oracle-bigram <userdir> <prev-token> <cur9> 9 <cur10> 10`; compiled with `-DORACLE_BDB -ldb` / `-DORACLE_KC -lkyotocabinet` / (default) `-ltkrzw -lpthread` |
| Owning subsystem | bisection / fixtures |
| Purpose | Writes raw user-bigram rows (counts 9 and 10) directly into `<userdir>/user_bigram.db` in the pin's SingleGram blob layout, because public `pinyin_train` first-seeds 69 and the `filter=10` edge is unreachable through the C ABI. |
| Implementation | C++17, 130 lines; **links a DBM directly** (`<db.h>` DB_HASH put; `<kcstashdb.h>` StashDB load_snapshot/set/dump_snapshot; `<tkrzw_dbm_hash.h>` HashDBM Set); no libpinyin |
| Inputs | argv 6; counts must be exactly 9 and 10 |
| Outputs | mutates user_bigram.db; exit 0/1 with DB error message |
| Platform | Linux-only (backend libs) |
| Consumers | run-union-diff.sh (built via `g++ -std=c++17 -Wall -Wextra -Werror -O2`, flags chosen by `OXPINYIN_CAPI_BACKEND_EXT` db/kct/tkt); .gitignore lists the binary; union-edge-diff.c references it |
| State | none |
| Overlap | Same blob layout as `oxpinyin_data::single_gram::encode_single_gram` (persistence.rs test) and `oxpinyin_test_set_user_bigram` capi hook (unused by the runner now) |
| Constraints | Hard-codes on-disk layout `{u32 total; {u32 token,u32 freq}[] sorted by token}` (ngram.cpp:31-34, :178-204), key = predecessor token; KC path replaces the snapshot file (pin save_db); cites pin blobs by file:line |

### BIS-B-14 — tools/bisection/predict-diff.c

| Field | Value |
|---|---|
| Entry point | `./predict-diff <so> <systemdir>` |
| Owning subsystem | bisection |
| Purpose | W11 phrase prediction (non-punct): import 测测, train 测测→你好 via the real choose path, then `pinyin_guess_predicted_candidates` (plain, with retval + all rows) and `..._with_punctuations` (type-4 bigram rows only), then an order case with two more imported suffixes. |
| Implementation | C (gnu11), 243 lines; dlopen; no frees |
| Inputs | argv 2; prefers `oxpinyin_init_for_fixtures` when exported |
| Outputs | `predict-plain: true/false`, `predict-plain: n=`, `plain: type=<t> <text>`, `predict: …`, `bigram: <text>`, `order-plain: n=`, `order-plain: [i] type=<t> <text>`; exit 0/1 |
| Platform | Linux-only |
| Consumers | run-predict-diff.sh → run-w11-diff.sh; run-same-data-dir-diff.sh; gcov_measure.py; runners.json `predict-diff` and `w11-diff`; docs/findings/prediction-punct.md, sentence-surface.md, axis-j ledgers |
| State | `mkdtemp("/tmp/predictdiff-XXXXXX")`, **never removed** |
| Overlap | Section 4 of union-diff.c is the same train-then-predict script; punct-diff.c covers the omitted type 8 |
| Constraints | Symbols: init/fixtures-init, fini, alloc/free_instance, begin_add_phrases/iterator_add_phrase/end_add_phrases(7), parse_more_full_pinyins, guess_candidates(0x1e), get_candidate, choose_candidate, train(0), guess_sentence, guess_predicted_candidates, guess_predicted_candidates_with_punctuations, get_n_candidate, get_candidate_type, get_candidate_string. Comment: lookup after choose anchors at `strlen("cece")`, not the returned cursor. |

### BIS-B-15 — tools/bisection/run-predict-diff.sh

| Field | Value |
|---|---|
| Entry point | `run-predict-diff.sh [prebuilt-capi.so]` → `exec ./run-w11-diff.sh predict-diff "$@"` |
| Owning subsystem | bisection |
| Purpose | Thin alias for the W11 shared runner. |
| Implementation | bash, 9 lines |
| Inputs | passes argv through; env as run-w11-diff.sh |
| Outputs | those of run-w11-diff.sh (0/77/1/2) |
| Platform | Linux-only |
| Consumers | runners.json `predict-diff` (gate, W11/104); prediction-punct.md, sentence-surface.md |
| State | none |
| Overlap | run-user-candidate-diff.sh, run-punct-diff.sh, run-addon-candidate-diff.sh are the same shape |
| Constraints | header: "Does not edit run-import-diff.sh or run-train-diff.sh" |

### BIS-B-16 — tools/bisection/pred-order-diff.c

| Field | Value |
|---|---|
| Entry point | `./pred-order-diff <so> <systemdir>` |
| Owning subsystem | bisection |
| Purpose | B1 moving-number gate: dump every PREDICTED_PREFIX (5) and PREDICTED_PUNCTUATION (8) row in list order for 8 prefixes (好 的 一 你 中国 我 是 了) via the `_with_punctuations` entry (fallback to plain if absent) and again via the plain entry. |
| Implementation | C (gnu11), 222 lines; dlopen; `rm_rf` |
| Inputs | argv 2; `pinyin_init` only (no fixture init) |
| Outputs | `pred-<tag>:[i]=<text>`, `punct-<tag>:[i]=<text>`, `pred-plain-<tag>:…`; exit 0 / 1 accessor failure |
| Platform | Linux-only |
| Consumers | run-pred-order-diff.sh (in-source binary), run-same-data-dir-diff.sh (on purpose: reports the registered tie-order divergence), gcov_measure.py, run-bisect.sh comment, runners.json `pred-order-diff`; docs/findings/uncovered-surface-differentials.md, upstream-divergences.md, revert-plan.md |
| State | `mkdtemp("/tmp/predorderdiff-user-XXXXXX")` + rm_rf |
| Overlap | Same 8 prefixes as uncovered-surface-diff.c Phase B |
| Constraints | Symbols: pinyin_init, fini, alloc/free_instance, guess_predicted_candidates, guess_predicted_candidates_with_punctuations (optional), get_n_candidate, get_candidate, get_candidate_type, get_candidate_string |

### BIS-B-17 — tools/bisection/run-pred-order-diff.sh

| Field | Value |
|---|---|
| Entry point | executed, no args |
| Owning subsystem | bisection |
| Purpose | Per prefix (16 tags incl. plain-*), count rows at different positions between oracle and capi — a moving number, not pass/fail. |
| Implementation | bash, 115 lines; gcc; awk/paste; sources oracle-cell.sh + system-dir.sh; cargo via oracle_cell_artifact |
| Inputs | env `PINYIN_ORACLE_PREFIX`, `UNCOVERED_SYSTEM` then `OXPINYIN_SYSTEM_DIR` (checked with `system_dir_detect_ext` only), `OXPINYIN_CAPI_SO`, cell vars |
| Outputs | `<tag>: M/N rows at different positions`, `pred-order-diff: IDENTICAL (rows)` or `DIVERGENCE: total/rows …`; exit 0 / 77 / 1 (crash or zero oracle rows) / 2 (expected measured state; "do not wire into CI green") |
| Platform | Linux-only |
| Consumers | runners.json `pred-order-diff` (gate, row:12, exclusive bisection-source-output) — note runner is registered as a gate although its header says exit 2 is the expected state |
| State | in-source binary; logs held in shell variables (stderr to terminal) |
| Overlap | Standard oracle gate |
| Constraints | Standard build flags; cell-aware pin grep; baseline figure 好 174/178 hard-coded in messages |

### BIS-B-18 — tools/bisection/punct-diff.c

| Field | Value |
|---|---|
| Entry point | `./punct-diff <so> <systemdir> <punctuation-only\|matched-tables>` |
| Owning subsystem | bisection |
| Purpose | #104: PREDICTED_PUNCTUATION (type 8) lists for prefixes 好 中 国 中国 你; matched-tables mode also prints the total candidate count. |
| Implementation | C (gnu11), 125 lines; dlopen |
| Inputs | argv 3; prefers `oxpinyin_init_for_fixtures` |
| Outputs | `<tag>-predict: true/false`, `[<tag>-candidates: N]`, `<tag>-punct: <text>`, `<tag>-n: <seen>`; exit 0/1 |
| Platform | Linux-only |
| Consumers | run-punct-diff.sh → run-w11-diff.sh (`punctuation-only`), run-same-data-dir-diff.sh (`matched-tables`), gcov_measure.py (`matched-tables`), runners.json `punct-diff`; prediction-punct.md |
| State | `mkdtemp("/tmp/punctdiff-XXXXXX")`, never removed |
| Overlap | pred-order-diff.c and uncovered-surface-diff.c Phase B dump the same rows with more context |
| Constraints | Symbols: init/fixtures-init, fini, alloc/free_instance, guess_predicted_candidates_with_punctuations, get_n_candidate, get_candidate, get_candidate_type, get_candidate_string |

### BIS-B-19 — tools/bisection/run-punct-diff.sh

| Field | Value |
|---|---|
| Entry point | `run-punct-diff.sh [capi.so]` → `exec ./run-w11-diff.sh punct-diff "${1:-${OXPINYIN_CAPI_SO:-}}" punctuation-only` |
| Owning subsystem | bisection |
| Purpose | Alias passing the `punctuation-only` driver arg. |
| Implementation | bash, 9 lines |
| Inputs | argv 1 optional; `OXPINYIN_CAPI_SO` |
| Outputs | run-w11-diff.sh's |
| Platform | Linux-only |
| Consumers | runners.json `punct-diff` (gate, 104); predict-diff.c/run-predict-diff.sh comments; prediction-punct.md |
| State | none |
| Overlap | see BIS-B-15 |
| Constraints | — |

### BIS-B-20 — tools/bisection/residue-a-tail-diff.c

| Field | Value |
|---|---|
| Entry point | `./residue-a-tail-diff <so> <systemdir>` (optionally under `OXPINYIN_DUMP_TAILS=1` for the patched oracle) |
| Owning subsystem | bisection |
| Purpose | Diagnosis for residue A (missing user-phrase tail) and common-root test vs residues B, C, X2, D, E: five phases (A tails + token inventory, C window, B whole-row choose + train ×2 with bigram export and unigrams, X 你好时节 choose + clear_constraint, D DYNAMIC_ADJUST window at offset 5, E windows behind the composition offset). Asserts nothing. |
| Implementation | C (gnu11), 615 lines; dlopen; glib `g_free`/`g_array_new`/`g_array_free` via dlopen (fatal if absent); `GArrayPub` struct mirrors GArray |
| Inputs | argv 2; `pinyin_init` only; each phase starts from a fresh import state (set_options 0x18a or 0x38a; import 你好/5, 你好世界/9, 测试/3; save) |
| Outputs | `=== phase: … ===`, `init=`, `set_options(..)=`, `add(..)=`, `save=`, `<tag>:lookup_tokens(..)= n=`, `<tag>:token[i]=0x.. lib= phrase=../len/.. unigram=../..`, `<tag>:n_cand= n=`, `<tag>:cand[i] type= text= nbest=../.. is_user=`, `<tag>:user_row(你好世界)=present at`, `<tag>:nbest_rows= user_rows= max_rank=`, `<tag>:sentence[i]=`, `<tag>:bigram[i]=.. p\|py\|count`, `<tag>:bigram_rows=` (capped 33), `..:unigram(phrase,lib=)=`, `B:choose(0,row0)=`, `B:clear_constraint(0)=`, `B:train(0)=`, X/X2/D/E1/E2 lines. stderr: so=, systemdir=, userdir=. Exit 0 / 1 setup. External diff. |
| Platform | Linux-only |
| Consumers | run-residue-a-tail-diff.sh (in-source); runners.json `residue-a-tail-diff`; probe-coverage-abi.md, revert-plan.md, tooling-lane-gates.md, parity-gates.md, compatibility-policy.md, upstream-divergences.md |
| State | up to 4 `mkdtemp("/tmp/residue-a-XXXXXX")` user dirs, rm_rf'd per phase |
| Overlap | `dump_bigram`, `rm_rf`, `must`, `yesno`, syms struct largely identical to residue-mechanism-diff.c |
| Constraints | Symbols: 30 incl. set_options, get_sentence (only proved ranks), get_candidate_nbest_index, is_user_candidate, clear_constraint, lookup_tokens, token_get_unigram_frequency, token_get_phrase, begin_add_phrases…, begin_get_bigram_phrases…; bigram export capped at 33 rows; comment: residue A closed (#35) — any A/X/D diff line is a regression of that fix |

### BIS-B-21 — tools/bisection/run-residue-a-tail-diff.sh

| Field | Value |
|---|---|
| Entry point | executed, no args |
| Owning subsystem | bisection |
| Purpose | Run residue-a-tail-diff against capi and the strjoinv-patched pin; optional instrumented-oracle tail dump and runtime-side probe; diff. |
| Implementation | bash, 174 lines; gcc; sha256sum; sources system-dir.sh + oracle-cell.sh; cargo via oracle_cell_artifact and (opt) `cargo build -p oxpinyin-runtime --example nbest_tail_probe --features $CAPI_FEATURE` |
| Inputs | env `PINYIN_ORACLE_PREFIX` (MUST be the bigram-export-strjoinv patched prefix), `RESIDUE_A_CAPI_SO` (→ `OXPINYIN_CAPI_SO`), `RESIDUE_A_SAME_DIR=1` (both sides on oracle data), `RESIDUE_A_SYSTEM`/`OXPINYIN_SYSTEM_DIR` via resolve_system_dir, `RESIDUE_A_DUMP_PREFIX`, `RESIDUE_A_PROBE=1`, `OXPINYIN_NBEST_TAIL_PROBE`, `RESIDUE_A_OUT`, cell vars; files `$PREFIX/oracle-patches.sha256`, `oracle-pin.txt` |
| Outputs | `$OUT/residue-a-{capi,oracle}.log`, `capi.err`, `oracle.err`, optional `residue-a-oracle-dump.log`, `pin-tails.log`, `ox-probe.log`; diff -u; exit 0 (OUT removed if temp) / 1 (unpatched or off-pin prefix is FAIL, never skip; build/run/provisioning) / 2 divergence (expected while rows 33/34/36/37 open) |
| Platform | Linux-only |
| Consumers | runners.json `residue-a-tail-diff` (gate, rows 33/34/35/37/1, env `RESIDUE_A_CAPI_SO={capi}`, `oracle_variant: bigram-export-strjoinv`); probe-coverage-abi.md, revert-plan.md, tooling-lane-gates.md |
| State | in-source binary; OUT mktemp kept on divergence; PROBE_USER mktemp |
| Overlap | Patch-digest computation duplicated with oracle-cell.sh `oracle_cell_resolve_prefix` and run-scheme-diff.sh (tools/oracle/patches) |
| Constraints | Standard build flags; pin check = `grep -Fxq "${EXPECTED_PIN_REF}+patches-${digest}"` AND `patches_manifest_sha256=${digest}` AND `pin-bigram-export-strjoinv.patch` in oracle-patches.sha256 — missing → exit 1 not 77; `bigram.db` data check |

### BIS-B-22 — tools/bisection/residue-mechanism-diff.c

| Field | Value |
|---|---|
| Entry point | `./residue-mechanism-diff <so> <systemdir>` |
| Owning subsystem | bisection |
| Purpose | Mechanism probes for residues D (system-token unigram of 你好 on a fresh context) and B (import 你好世界, whole-row NBEST choose, `clear_constraint(0)` as CONSTRAINT_ONESTEP probe, train twice, bigram export each time). Diagnosis only. |
| Implementation | C (gnu11), 328 lines; dlopen; glib helpers via dlopen |
| Inputs | argv 2; `pinyin_init` only; set_options 0x1e |
| Outputs | `=== phase: D-system-unigram ===`, `lookup_tokens(你好)= n=`, `token=0x.. phrase=`, `token_unigram=`, `=== phase: B-whole-row-train ===`, `begin_add=`, `add(你好世界)=`, `save=`, `parse(nihaoshijie)=`, `guess_sentence=`, `n_candidate=`, `row0: type= text= nbest=`, `choose(0,row0)=`, `clear_constraint(0)=`, `<tag>:bigram[i]=…`, `train(0)=`, `train(0)again=`; exit 0/1 |
| Platform | Linux-only |
| Consumers | run-residue-mechanism-diff.sh; runners.json `residue-mechanism-diff` (gate, row:34); probe-coverage-abi.md, revert-plan.md, compatibility-policy.md |
| State | two mkdtemp dirs `/tmp/residue-mech-XXXXXX`, `/tmp/residue-mech-b-XXXXXX`, rm_rf'd |
| Overlap | Subset of residue-a-tail-diff.c (Phase B/D there) |
| Constraints | Comment: clear_constraint(0) should be false on both sides after a whole-row 1-best choose |

### BIS-B-23 — tools/bisection/run-residue-mechanism-diff.sh

| Field | Value |
|---|---|
| Entry point | executed, no args |
| Owning subsystem | bisection |
| Purpose | Build/run residue-mechanism-diff both sides, diff. |
| Implementation | bash, 97 lines; gcc; sources system-dir.sh + oracle-cell.sh; cargo via oracle_cell_artifact |
| Inputs | env `PINYIN_ORACLE_PREFIX`, `OXPINYIN_CAPI_SO`, `RESIDUE_MECH_SAME_DIR=1`, `RESIDUE_MECH_SYSTEM`/`OXPINYIN_SYSTEM_DIR`, `RESIDUE_MECH_OUT`, cell vars |
| Outputs | `$OUT/residue-mech-{capi,oracle}.log`, `*.err`; exit 0 / 77 (no oracle, off-pin, no bigram.db) / 1 / 2 ("expected while residues B/D remain open") |
| Platform | Linux-only |
| Consumers | runners.json `residue-mechanism-diff` (exclusive bisection-source-output) |
| State | in-source binary; OUT mktemp removed only when identical |
| Overlap | Nearly line-for-line the same as run-residue-a-tail-diff.sh minus the patch gate |
| Constraints | Standard flags; standard cell-aware oracle gate; `bigram.db` check |

### BIS-B-24 — tools/bisection/run-perf-baseline.sh

| Field | Value |
|---|---|
| Entry point | executed, no args (typically inside the perf-matrix container: `docker run … -e PINYIN_ORACLE_PREFIX=/opt/libpinyin-tkrzw -e OXPINYIN_PERF_WORK=/out oxpinyin-matrix bash tools/bisection/run-perf-baseline.sh`) |
| Owning subsystem | perf |
| Purpose | W8 Stage-2 scoreboard: oracle vs cargo-cinstall-staged oxpinyin on three axes — speed (bisect --perf, alternating), RAM (/proc/self/status via same harness), installed size (prefix vs staged tree). |
| Implementation | bash, 211 lines; gcc (bisect.c), `cargo cinstall --locked --release -p oxpinyin-capi --destdir --prefix=/usr`, pkg-config, taskset, python3 perf-baseline.py |
| Inputs | env `PINYIN_ORACLE_PREFIX`, `OXPINYIN_PERF_WORK` (default target/perf-baseline), `PERF_CPU` (0), `PERF_RUNS` (20), `PERF_CYCLES` (8), `PERF_RAM_RUNS` (10), `PERF_REUSE_STAGE`, `PINYIN_EXPORT_DIR` (default /tmp/oxpinyin-export), `PKG_CONFIG_PATH`; exported `PKG_CONFIG_SYSROOT_DIR`; data populated from export (pinyin_index.bin, phrase_index.bin, bigram.db, table.conf, *.table) into `$STAGE/usr/share/oxpinyin`, then requires addon_*_index.bin, punct.bin, gb_char.bin |
| Outputs | `$WORK/speed.jsonl`, `ram-init.jsonl`, `ram-cycle.jsonl`, `*-speed.err`, `*-ram-*.err`, reports `speed-report.md`, `ram-report.md`, `size-report.md` (via perf-baseline.py summarize-speed/summarize-ram/size); exit 1 fatal (no 77) |
| Platform | Linux-only (taskset, /proc, cargo-c) |
| Consumers | runners.json `perf-baseline` (not-a-gate, issue 370); Dockerfile.perf-matrix comment; tools/profile/run-w8-cycle.sh continues it; docs/runbooks/benches.md; docs/perf/*.md; several findings |
| State | `$WORK` persistent (stage reused with PERF_REUSE_STAGE=1); in-source `tools/bisection/bisect` binary |
| Overlap | `run_speed_side`/`run_ram_side` taskset+env+bisect block duplicated as `run_one` in run-perf-matrix.sh / run-perf-same-data.sh and inline in run-shipped-check.sh / run-tree-recipe-control.sh |
| Constraints | Pin check `grep -q "^pin_ref=.*libpinyin-2.11.92-074a2219…"` (not cell-aware); measured external tool: none beyond /proc/self/status read inside bisect.c; stages with workspace default features (Berkeley DB since 2026-09-20) — container image has no libdb headers (header comment) |

### BIS-B-25 — tools/bisection/run-perf-matrix.sh

| Field | Value |
|---|---|
| Entry point | executed, no args; `CMD` of Dockerfile.perf-matrix |
| Owning subsystem | perf |
| Purpose | 4-cell matrix (libpinyin+tkrzw, libpinyin+kc, oxpinyin+tkrzw, oxpinyin+kc) speed/RAM/size, round-robin. |
| Implementation | bash, 156 lines; taskset; bisect (prebuilt in image at tools/bisection/bisect — not built here); python3 perf-matrix.py |
| Inputs | env `MATRIX_OUT` (/out), `PERF_CPU`, `PERF_RUNS`, `PERF_CYCLES`, `PERF_REPEATS`, `PERF_RAM_RUNS`; hard-coded `/opt/libpinyin-tkrzw`, `/opt/libpinyin-kc`, `/opt/oxpinyin-tkrzw/{stage,data}`, `/opt/oxpinyin-kc/{stage,data}` |
| Outputs | `$OUT/speed.jsonl`, `ram-init.jsonl`, `ram-cycle.jsonl`, `<label>-<mode>.err`, `matrix-report.md`; exit 1 fatal |
| Platform | container (perf-matrix image) |
| Consumers | runners.json `perf-matrix` (not-a-gate); Dockerfile.perf-matrix CMD; perf-ci.py reads its jsonl; benches.md; perf audits |
| State | `$OUT` |
| Overlap | `find_so` duplicated in run-perf-same-data.sh; no bdb cell |
| Constraints | No oracle pin check at all; no oracle-cell.sh |

### BIS-B-26 — tools/bisection/run-perf-same-data.sh

| Field | Value |
|---|---|
| Entry point | executed inside perf-matrix container with tree at /work |
| Owning subsystem | perf |
| Purpose | #260: four cells on ONE data dir per backend (oxpinyin opens libpinyin's own install data) — the drop-in configuration. |
| Implementation | bash, 192 lines; `cargo build --locked --release -p oxpinyin-capi --no-default-features --features kyotocabinet|tkrzw` + `strip --strip-all`; `readelf -d` backend verification; `gcc -std=gnu11 -Wall -Wextra -O2` (no -Werror) for bisect; taskset; inline python3 median summariser |
| Inputs | env `MATRIX_OUT`, `PERF_CPU`, `PERF_RUNS`, `PERF_CYCLES`, `PERF_REPEATS`, `PERF_RAM_RUNS`, `CARGO_TARGET_DIR`, `OXPINYIN_KC_SO`, `OXPINYIN_TKRZW_SO`; `/opt/libpinyin-{tkrzw,kc}` |
| Outputs | `$OUT/libpinyin_capi-{kc,tkrzw}.so`, `speed.jsonl`, `ram-init.jsonl`, `ram-cycle.jsonl`, `*.err`; stdout median table (init/alloc/cold/steady ms, rss/hwm KiB); exit 1 fatal |
| Platform | container |
| Consumers | runners.json `perf-same-data` (not-a-gate, 260); Dockerfile.callgrind comment; perf-ci.py; benches.md (named as "not naming a recipe"); several perf findings |
| State | `$OUT`; in-source bisect binary |
| Overlap | `verify_backend` (NEEDED must contain exactly one of libkyotocabinet/libtkrzw) unique here; summariser duplicates perf-matrix.py logic |
| Constraints | No pin check; backend never inherited from workspace default (comment on 05688575 flip); no bdb cell |

### BIS-B-27 — tools/bisection/run-rss-diag.sh

| Field | Value |
|---|---|
| Entry point | `run-rss-diag.sh --lp <libpinyin.so> --ox <oxpinyin.so> --data <dir> [--out DIR] [--rounds N] [--cycles N]` |
| Owning subsystem | perf |
| Purpose | RSS attribution: two cells (L pin, S shipped-recipe oxpinyin) on one data dir, round-robin `PERF_MODE=rss-diag` rounds, then one mapping round per cell with `RSS_DIAG_DIR` set. |
| Implementation | bash, 123 lines; gcc (rebuild bisect if missing/stale); taskset; sha256sum, readelf, ldd, git |
| Inputs | argv flags; env `PERF_CPU`; `/proc/loadavg` |
| Outputs | `$OUT/identity.txt` (utc, kernel, glibc, nproc, params, repo rev + dirty, bisect sha, .so sha/size/NEEDED), `$OUT/rounds.jsonl` (`{"cell","round","loadavg_before","loadavg_after","result":<bisect JSON>}`), `$OUT/maps/<cell>-round.json` plus bisect's `/proc/self/maps`, `smaps_rollup`, `malloc_info` dumps; exit 2 usage |
| Platform | Linux-only (/proc, taskset) |
| Consumers | runners.json `rss-diag` (not-a-gate — command has no args, so a registry run hits `usage` exit 2); docs/findings/rss-attribution-2026-09-09.md; benches.md |
| State | `$OUT` default `/tmp/rss-diag` |
| Overlap | identity block similar to run-tree-recipe-control.sh / run-shipped-check.sh |
| Constraints | Measurement rounds must run with RSS_DIAG_DIR unset (writing dumps allocates); `--rounds` validated decimal; harness measured in bisect.c: smaps_rollup, mallinfo2, malloc_trim(0) delta |

### BIS-B-28 — tools/bisection/run-same-data-dir-diff.sh

| Field | Value |
|---|---|
| Entry point | `run-same-data-dir-diff.sh <libpinyin.so> <libpinyin_capi.so> <data-dir> [driver ...]` |
| Owning subsystem | bisection |
| Purpose | Drop-in invariant: every `<so> <systemdir>` driver run on ONE unchanged data dir by both libraries; full stdout compared in memory. Default 15 drivers; dynamic-adjust-diff gets on/off runs with CHOICE and non-vacuity checks. |
| Implementation | bash, 194 lines; gcc `-std=gnu11 -Wall -Wextra -O2` (no -Werror) `+ $(pkg-config --cflags --libs glib-2.0)` when available; sources oracle-cell.sh |
| Inputs | argv 3 (+ driver names); env `OXPINYIN_CAPI_SO` fallback for argv2, `PINYIN_ORACLE_DBM`; requires `$data/gb_char.bin` |
| Outputs | per driver `IDENTICAL (n log lines)` / `DIVERGENCE (k differing lines)` + first 60 diff lines / crash; exit 0 / 77 missing oracle / 1 build-crash / 2 any divergence |
| Platform | Linux-only |
| Consumers | runners.json `same-data-dir-diff` (gate, 635/636/637, args `{prefix}/lib/libpinyin.so {capi} {prefix}/lib/libpinyin/data`); docs/runbooks/oracle.md, goldens-and-pins.md, ROADMAP.md, upstream-divergences.md; crates/oxpinyin-runtime/tests/libpinyin_data_dir.rs names it as the C-ABI half |
| State | mktemp work dir (trap rm), fresh cwd per side |
| Overlap | `capture_output` duplicated verbatim in run-w11-diff.sh; builds drivers owned by other wrappers (key-surface, key-parse, tie-order, dict-surface, phrase-surface, pred-order, predict, punct(matched-tables), addon-candidate, user-candidate, union, import, live-typing, nbest-train, dynamic-adjust) |
| Constraints | No pin manifest check (only file existence); bisect surface mode excluded (pin `_check_offset` asserts); drivers mkdtemp under /tmp are not isolated by cwd |

### BIS-B-29 — tools/bisection/scheme-diff.c

| Field | Value |
|---|---|
| Entry point | `SCHEME_DIFF_USER_DIR=<dir> ./scheme-diff <so> <systemdir> [scheme-number]` (default 2 = MS) |
| Owning subsystem | bisection |
| Purpose | W13 double-pinyin: 15 inputs × (parse_double consumed, parsed len, guess_sentence, get_sentence row 0, candidates ≤12 with type, double_aux at every cursor, reset), the frozen Tone law (3 profiles × 13 inputs), and the out-of-enum half-mutation probe (schemes 1/99/−1/1 with "aa"). |
| Implementation | C (gnu11), 437 lines; dlopen; g_free via glib dlopen |
| Inputs | argv 2-3; env `SCHEME_DIFF_USER_DIR` (else ""); fixture init only when systemdir lacks interpolation2.text |
| Outputs | `set_double_pinyin_scheme(n): …`, `=== input: "x" ===`, `parse_double: consumed=`, `parsed_input_length:`, `guess_sentence:`, `get_sentence: … text=""`, `guess_candidates:`, `n_candidates:`, `  candidate[i]: type=<NAME> text=""`, `double_aux(c): … text=""`, `reset_parsed_len:`, `tonelaw\|0x..\|input\|…`, `halfmut\|…`; exit 0/1 (missing symbols counted) |
| Platform | Linux-only |
| Consumers | run-scheme-diff.sh (`double` mode, in-source); runners.json `scheme-diff`; docs/findings/double-pinyin-spec.md, bopomofo-spec.md, revert-plan.md, ROADMAP.md |
| State | none of its own |
| Overlap | chewing-diff.c / fullpin-diff.c are sibling drivers selected by the same wrapper; ctype_name table repeats type_name of nbest-train/uncovered |
| Constraints | Symbols: pinyin_init/fixtures, fini, alloc/free_instance, set_options, set_double_pinyin_scheme, parse_more_double_pinyins, get_parsed_input_length, guess_sentence, get_sentence, guess_candidates(0x1e), get_n_candidate, get_candidate, get_candidate_type, get_candidate_string, get_double_pinyin_auxiliary_text, reset; DEFAULT_FLAGS 0x18a |

### BIS-B-30 — tools/bisection/run-scheme-diff.sh

| Field | Value |
|---|---|
| Entry point | `run-scheme-diff.sh <double\|bopomofo\|full> [scheme-number]` |
| Owning subsystem | bisection |
| Purpose | Per-scheme differential: builds scheme-diff (+ chewing-diff for bopomofo, fullpin-diff for full) and diffs logs; full schemes 2/3 use the patched oracle (tools/oracle/patches). |
| Implementation | bash, 237 lines; gcc ×1-3; sources system-dir.sh + oracle-cell.sh; cargo via oracle_cell_artifact |
| Inputs | env `PINYIN_ORACLE_PREFIX`, `PINYIN_PATCHED_ORACLE_PREFIX` (default ~/.local/opt/pinyin-oracle-patched), `W13_CAPI_SYSTEM`/`OXPINYIN_SYSTEM_DIR` via resolve_system_dir, `W13_INTERPOLATION`, `SCHEME_DIFF_TONE` (documented; consumed by fullpin-diff), `SCHEME_DIFF_PARSE_AUX_ONLY=1`, `OXPINYIN_CAPI_SO`, cell vars; exports `SCHEME_DIFF_USER_DIR` |
| Outputs | `IDENTICAL` / `PARSE_AUX_IDENTICAL` / `DIVERGENCE` + diff; exit 0 / 77 / 1 (bad scheme number, crash) / 2 |
| Platform | Linux-only |
| Consumers | runners.json `scheme-diff` (gate, W13/W15 — registry passes no mode arg so default `double`); docs/findings/double-pinyin-spec.md, bopomofo-spec.md, probe-coverage-abi.md, revert-plan.md, ROADMAP.md |
| State | in-source binaries; USER_DIR and SYSTEM_TMP mktemp (trap rm); system tables copied via `system_dir_copy_tables` plus interpolation2.text |
| Overlap | Patch-digest check mirrors run-residue-a-tail-diff.sh but over tools/oracle/patches; scheme validation block duplicated (lines 44-52 and 115-137) |
| Constraints | Pin check: hard-coded pin grep for the unpatched oracle, `grep -Fxq "$EXPECTED_PIN_REF+patches-$sha"` for the patched one; bopomofo 7 and double 30 never sent (abort oracle) |

### BIS-B-31 — tools/bisection/session-fork-diff.c

| Field | Value |
|---|---|
| Entry point | `session-fork-diff <lib.so> <systemdir> <userdir> <fork-_exit\|fork-exit\|fork-nosave\|crash>` |
| Owning subsystem | bisection |
| Purpose | #531/#546: one context across `fork()` — child learns/saves/_exit or exit or no-save; parent learns, saves, reads state, fini; crash mode SIGKILL before fini. Confirms nothing outside the user dir (TMPDIR) is touched. |
| Implementation | C (gnu11), 297 lines; dlopen; `<glib.h>` direct; `<sys/wait.h>`; `setvbuf(stdout, _IOLBF)` |
| Inputs | argv 4; fixed targets CHILD/PARENT (历时/寓言; imports 泥壕/罢跨) |
| Outputs | `mode:`, `init: ok`, `alloc: ok`, `row@init\tT\t…`, `phrases@init: n` + `row@..\tP\t..`, `import child: <word> <0/1>`, `train child: <word> <status>`, `save child: <0/1>`, `child: exited N`, parent lines, `fini: ok`, `crash: SIGKILL before fini`; exit 0 / 1 / 2 usage |
| Platform | Linux-only |
| Consumers | run-session-fork-diff.sh; runners.json `session-fork-diff` (531/546) |
| State | userdir from argv |
| Overlap | `train_target`, `import_phrase`, `target_rows`, `phrase_rows` copied from open-counter-diff.c (pinyin only, `pinyin_` prefix hard-coded) |
| Constraints | Symbols: pinyin_init, fini, alloc/free_instance, save, parse_more_full_pinyins, guess_sentence, guess_candidates(0x1c), get_n_candidate, get_candidate, get_candidate_type, get_candidate_string, choose_candidate, train(0), reset, begin_add_phrases/iterator_add_phrase/end_add_phrases, lookup_tokens, token_get_unigram_frequency, begin/iterator_has_next/iterator_get_next/end_get_phrases |

### BIS-B-32 — tools/bisection/run-session-fork-diff.sh

| Field | Value |
|---|---|
| Entry point | executed, no args |
| Owning subsystem | bisection |
| Purpose | Run the four fork/crash modes on both libraries under a fresh TMPDIR and user dir each; log += exit status, user dir inventory, TMPDIR entry count; byte diff. |
| Implementation | bash, 156 lines; gcc + pkg-config glib; `timeout -k 5 $TIMEOUT`; sources oracle-cell.sh |
| Inputs | env `SESSION_FORK_ORACLE_PREFIX` (→ `PINYIN_ORACLE_PREFIX`), `SESSION_FORK_PINYIN_SO` (→ `OXPINYIN_CAPI_SO` → `$CARGO_TARGET_DIR/debug/libpinyin_capi.so`), `SESSION_FORK_MODES`, `SESSION_FORK_TIMEOUT` (60), `SESSION_FORK_OUT`; `$PREFIX/lib/libpinyin.so.15`, `data/table.conf` |
| Outputs | `$OUT/{oracle,oxpinyin}-<mode>.log(.stderr)`, `$OUT/<mode>.diff`; exit 0 / 77 prefix unset / 3 off-pin or missing input / 1 timeout (124, or 137 outside crash) or unexpected status / 2 divergence |
| Platform | Linux-only (timeout(1), SIGKILL status) |
| Consumers | runners.json `session-fork-diff` (gate; env SESSION_FORK_ORACLE_PREFIX/PINYIN_SO/ZHUYIN_SO — ZHUYIN_SO is set by the registry but the script never reads it) |
| State | OUT/WORK pattern; per-run TMPDIR |
| Overlap | see BIS-B-05 |
| Constraints | Build with glib flags into $WORK; hard-coded pin grep; stderr not diffed (libraries word diagnostics differently); only legitimate statuses 0 and 137 (crash) |

### BIS-B-33 — tools/bisection/run-shipped-check.sh

| Field | Value |
|---|---|
| Entry point | executed inside the knob-amd64 perf-matrix container (`/src-curr`, `/repo`, `/ctltools`, `/out` mounts); not executable bit in tree (mode 644) |
| Owning subsystem | perf |
| Purpose | Is the product artifact (`cargo cinstall --features tkrzw,shipped`) the same speed as cell Z (`cinstall --features tkrzw`) vs L libpinyin? Within-session ratios. |
| Implementation | bash, 92 lines; `cargo cinstall --locked --release -p oxpinyin-capi --no-default-features --features …` ×2 into mktemp target/stage; gcc bisect from `/repo/tools/bisection/bisect.c`; nm/sha256sum/readlink; taskset; python3 perf-ratio.py |
| Inputs | env `SRC_CURR` (/src-curr), `CTRL_OUT` (/out), `CTRL_RUNS` (20), `CTRL_CYCLES` (8), `CTRL_SIZES` ("8 16"), `CTRL_CPU` (0), `CTLTOOLS` (/ctltools); hard-coded `/opt/libpinyin-tkrzw` |
| Outputs | `$OUT/shipped-identity.txt` (sha/size per L/Z/S, fixture-hook symbol counts), `speed-shipped.jsonl`, `shipped.err`, `shipped-ratios-n<N>.md`; exit via set -e |
| Platform | container |
| Consumers | runners.json `shipped-check` (not-a-gate); docs/findings/perf-build-recipe-audit-2026-09-10.md, perf-cycle-ir-differential-2026-09-08.md |
| State | /tmp/Z-plain.so, /tmp/S-shipped.so, /tmp/bisect |
| Overlap | Subset of run-tree-recipe-control.sh (same build_cinstall, identity, loop) |
| Constraints | No pin check; expects hooks `oxpinyin_init_for_fixtures`/`oxpinyin_test_set_user_bigram` counted (Z 2, S 0 per tree-recipe comment) |

### BIS-B-34 — tools/bisection/system-import-profile.c

| Field | Value |
|---|---|
| Entry point | `driver <libpinyin.so> <system-data-dir> <fresh-user-dir> [iterations]` (default 1000) \| `--cold-open` \| `--sentence-fixture <file>` \| `--train-fixture <file>` \| `--phrase-file <file>` \| `--export-system` |
| Owning subsystem | profile |
| Purpose | #599 reproducible Callgrind/Massif workloads over the C ABI: import+save (phased), cold open, and the #554 modes: 500 sentence guesses, 200 train_top-equivalent trainings + save, 10k phrase imports + save + export, plus an unmeasured system export to generate the phrase file. |
| Implementation | C (gnu11), 279 lines; dlopen(`RTLD_NOW\|RTLD_LOCAL`); `<valgrind/callgrind.h>` (CALLGRIND_DUMP_STATS_AT client requests); `g_free` resolved by `dlsym(so, "g_free")` on the library handle |
| Inputs | argv as above; the user dir must exist-or-be-created, be empty, and be writable (checked); fixture rows `input\texpected\tsurface` (skips `#`, blank, empty surface); phrase file rows `lib(1..4)\tphrase\tpinyin\tcount` strictly parsed (NUL check, strtoul/strtol ERANGE, CRLF tolerated) |
| Outputs | **Measurements**: (1) no wall-clock timing at all — "No wall-clock timing" in header; instruction counts come from Callgrind outside the process; in import mode three client-request dumps `CALLGRIND_DUMP_STATS_AT("opened" / "imported" / "saved")` partition open/import/save/close; other modes "emit no Callgrind client-request dumps" (peak heap/pages from Massif externally). (2) Counts: `operations\t<count>\t<trained>` for fixtures (500 guesses with 0 trained, or 200 trained), `operations\t<imported>\t<exported>` for phrase-file (10000, exported rows of touched libraries 1-4). (3) Workload definitions: import = per library 1..4, `iterations` × two adds (测试/ce4'shi4/5, 测侧/ce4'ce4/5), then save; cold-open = init, set_options(0x18a), alloc, free, fini; sentence = reset, parse, parsed length (>0), guess_sentence, guess_candidates(0x1e), proved n-best ranks ≤2, print `input\tguessed\t<sent0^A sent1…>\t<up to 6 cands as n/rank/text or N\|a\|?/-/text>`; train = first 200 rows, requires full consumption, guess, train(0), prints `train\t<input>\t<parse>\t<len>\t1\t<top sentence>`, saves once; export rows `lib\tphrase\tpinyin\tfreq`. Exit 0 / 1 failure / 2 usage. |
| Platform | Linux-only (valgrind header, dlopen) |
| Consumers | run-system-import-profile.sh (iterations/--cold-open modes only); docs/runbooks/benches.md "BDB guess, training, and import profiles (#757–#759)" documents the #554 fixture/phrase modes, the SHA-256 of reviewed source `565be2c8…`, the registered stdout hashes, and the 2026-10-14 refresh using `HARNESS=/tmp/pr767-system-import-profile-final`; docs/runbooks/parity-gates.md names it as unchanged/omitted; runners.json `system-import-profile` |
| State | none (user dir supplied) |
| Overlap | Sentence/train replay mirrors `Session::train_top` and `sentence_tail.rs::measure` filter (comments); export walk mirrors import-semantics-diff.c |
| Constraints | **The file's own comments do not mention PR #767**; they cite #599 (callgrind/massif import workload), #588 (cold-open lifecycle), #554 (fixture/phrase modes). The PR-767 wording lives only in benches.md (the "final strict-compiled C driver" and the `pr767-…` harness path). Comments: `Session::train_top checks full consumption before guessing`; `no candidate selection or unguessed training`; `--export-system` is "selection only, unmeasured" |

### BIS-B-35 — tools/bisection/run-system-import-profile.sh

| Field | Value |
|---|---|
| Entry point | `run-system-import-profile.sh <pin-so> <ox-so> <system-data-dir> <new-work-dir> [iterations\|--cold-open]` |
| Owning subsystem | profile |
| Purpose | #599: per side (pin, ox) run the driver under valgrind callgrind (phase instruction counts), massif heap, massif `--pages-as-heap`; discard runs overlapping a suspend; print per-phase instructions and peak bytes. |
| Implementation | bash, 82 lines + embedded python3 (stdlib: subprocess, re, time, pathlib); `cc -std=gnu11 -Wall -Wextra -Werror -O2 … -ldl`; valgrind |
| Inputs | argv 4-5; `/proc/loadavg`; clocks `CLOCK_BOOTTIME − CLOCK_MONOTONIC` (suspend offset, discard if delta > 1 ms, up to 6 attempts) |
| Outputs | stdout `<side> <mode>: utc= load1= suspend_delta_ns= exit=0 stdout_bytes=0 user.conf=present no-user-state=absent`, `<side> <phase>: N instructions` (phases open/import/save/close or cold-open), `<side> total:`, `<side> peak heap\|pages: N bytes`; valgrind output stays on stderr/in memory (`--callgrind-out-file=/dev/stderr`, `--massif-out-file=/dev/stderr`, `--combine-dumps=yes`); `$out/driver`, `$out/<side>-<mode>-user-<attempt>/`; exit 1 (SystemExit) on failure |
| Platform | Linux-only |
| Consumers | runners.json `system-import-profile` (not-a-gate, 599; command has no args → usage failure under the registry) |
| State | `mkdir "$out"` must be new |
| Overlap | valgrind option strings match benches.md recipes; suspend-detection logic described in benches.md |
| Constraints | Requires driver stdout empty and `user.conf` present, no `no-user-state` in stderr; expects exactly 4 (or 1) `summary:` lines; Massif `mem_heap_B` max; no pin check |

### BIS-B-36 — tools/bisection/run-system-import-round-trip.sh

| Field | Value |
|---|---|
| Entry point | `run-system-import-round-trip.sh <pinyin\|zhuyin> <pin-so> <ox-so> <system-data-dir>` |
| Owning subsystem | bisection |
| Purpose | #599: byte-for-byte same-backend SYSTEM_FILE (.dbin) interchange both directions for libraries 1-4: pin→ox read, pin→ox→pin rewrite, ox→pin read. |
| Implementation | bash, 49 lines; `cc -std=gnu11 -Wall -Wextra -Werror -O2`; pkg-config glib (zhuyin side); cmp/diff; sources oracle-cell.sh |
| Inputs | argv 4; builds **import-semantics-diff.c** (pinyin) or **zhuyin-import-diff.c** (zhuyin) — other agents' / BIS-B-54 driver |
| Outputs | `<facade> library <n>: IDENTICAL pin->ox, pin->ox->pin, ox->pin`; any diff/cmp mismatch aborts via `set -e` (exit 1); exit 2 bad facade |
| Platform | Linux-only |
| Consumers | runners.json `system-import-round-trip` (pinyin) and `system-import-round-trip-zhuyin`; tools/oracle/user-dir-round-trip.sh calls it (pinyin); docs/findings/tkrzw-distro-compat.md |
| State | mktemp work (trap rm); drivers run with cwd = profile dir (driver uses `./user`) |
| Overlap | Driver argument protocol `<so> <data> <library> [read\|rewrite]` shared by import-semantics-diff.c and zhuyin-import-diff.c |
| Constraints | Hard-codes `.dbin` names gb_char/gbk_char/opengram/merged; `grep '^reopened '` as expected set; no pin check |

### BIS-B-37 — tools/bisection/train-diff.c

| Field | Value |
|---|---|
| Entry point | `TRAINDIFF_ROUNDS=<1..64> [TRAINDIFF_OPTIONS=<hex>] [TRAINDIFF_MASK=user\|all] [TRAINDIFF_DUMP_CANDIDATES=1] [TRAINDIFF_REOPEN=1] ./train-diff <so> <systemdir>` |
| Owning subsystem | bisection |
| Purpose | W6-T7 user store: N rounds of (你→好) training via choose path, `remember_user_input` (你好 −1, 世界 −1, 世界 7), optional `mask_out` (user 0x0F000000/0x07000000 or all 0/0), one export of phrase+bigram triples; optional candidate dumps (initial@0, nihao@0, after-ni) and Phase 4 save→fini→reopen→re-probe→one more round→reopen- export. |
| Implementation | C (gnu11), 595 lines; dlopen; g_free via glib dlopen |
| Inputs | argv 2 + env above (`TRAINDIFF_ROUNDS` via atoi, range 1..64); fixture init only when no interpolation2.text |
| Outputs | `cand:<label> n=`, `cand:<label>[i]=<text>`, `phrase: p\|py\|c`, `bigram: p\|py\|c`, `train-reopened:<0/1>`, `reopen-phrase:`, `reopen-bigram:`, `BEGIN-NULL`/`GET-FAILED`; exit 0/1 |
| Platform | Linux-only |
| Consumers | run-train-diff.sh, run-train-diff-dynamic-off.sh (both in-source `./train-diff`), run-same-data-dir-diff.sh? (no — not in default list; nbest-train-diff is); runners.json `train-diff`, `train-diff-dynamic-off`; many findings (user-store, option-bits, phrase-union, sentence-surface, tkrzw-distro-compat…) |
| State | `mkdtemp("/tmp/traindiff-user-XXXXXX")` rm_rf'd |
| Overlap | resolve_all/round logic shared with nbest-train-diff.c; `print_bigram_rows` ignores get_next retval (upstream "more rows" semantics) |
| Constraints | Symbols: 25 incl. remember_user_input, mask_out, set_options; comment: predicted-candidate path deliberately absent (capi stub false / pin asserts); seeds 69,138,414,1242,3726,11178,22080 clamp |

### BIS-B-38 — tools/bisection/run-train-diff.sh

| Field | Value |
|---|---|
| Entry point | executed, no args |
| Owning subsystem | bisection |
| Purpose | Run train-diff for rounds 1..8 and masks user/all (rounds=8), one fresh process each, diff sorted logs exactly. |
| Implementation | bash, 147 lines; gcc; sources system-dir.sh + oracle-cell.sh; cargo via oracle_cell_artifact |
| Inputs | env `PINYIN_ORACLE_PREFIX`, `OXPINYIN_CAPI_SO`, `TRAINDIFF_SYSTEM`/`OXPINYIN_SYSTEM_DIR` via resolve_system_dir, cell vars |
| Outputs | per phase `rounds=N: IDENTICAL` + sorted export lines or `DIVERGENCE` + diff; `train-diff: PASS`; exit 0 / 77 / 1 / 2 |
| Platform | Linux-only |
| Consumers | runners.json `train-diff` (gate, W6-T7); cited by run-addon/predict/punct/union/user-candidate scripts ("does not edit run-train-diff.sh"); findings |
| State | in-source binary; mktemp logs; driver stderr discarded |
| Overlap | Standard oracle gate + bigram.db check |
| Constraints | Header says absent oracle → "exit 0"; code exits 77. Sorted comparison (store iteration order not a contract). |

### BIS-B-39 — tools/bisection/run-train-diff-dynamic-off.sh

| Field | Value |
|---|---|
| Entry point | executed, no args |
| Owning subsystem | bisection |
| Purpose | W10 DYNAMIC_ADJUST clear (0x188): one training round exports identical; then populated-store candidate dump with TRAINDIFF_DUMP_CANDIDATES + TRAINDIFF_REOPEN: cand lines identical cross-engine, reopened window equals in-memory per side, reopen- export identical sorted. |
| Implementation | bash, 198 lines; gcc; sources oracle-cell.sh + system-dir.sh; cargo via oracle_cell_artifact |
| Inputs | env `PINYIN_ORACLE_PREFIX`, `OXPINYIN_CAPI_SO`, `TRAINDIFF_SYSTEM`/`OXPINYIN_SYSTEM_DIR`, `TRAINDIFF_OFF_OPTIONS` (0x00000188), `OPTION_SWEEP_CAPI_DATA` (override), `PINYIN_MODEL_DIR` + `target/model20/extracted` for interpolation2.text graft |
| Outputs | `DYNAMIC_ADJUST off: exports identical; wrote N rows`, `populated-store candidates identical…`, `train-diff dynamic-off: PASS`; exit 0 / 77 (no oracle; or no interpolation2.text → "PASS (exports only)" exit 77) / 1 / 2 |
| Platform | Linux-only |
| Consumers | runners.json `train-diff-dynamic-off` (gate, W10); option-bits.md, redb-removal-audit |
| State | in-source binary; `mktemp -d /tmp/traindiff-capi-data-XXXXXX` scratch copy (trap rm); DRIVER_ERROR mktemp |
| Overlap | interpolation graft duplicated with run-option-sweep.sh |
| Constraints | Oracle gate checks file presence only (no EXPECTED_PIN_REF grep); vacuity guard requires `cand:reopened@0 n=`, `train-reopened:1`, `reopen-bigram:`, `reopen-phrase:` |

### BIS-B-40 — tools/bisection/run-tree-recipe-control.sh

| Field | Value |
|---|---|
| Entry point | executed inside `localhost/oxpinyin-matrix:knob-amd64` (podman invocation in header) with `/src-old`, `/src-curr`, `/ctltools`, `/out` |
| Owning subsystem | perf |
| Purpose | Separate engine-tree effect from build-recipe effect on steady keystroke-cycle ratio: cells L (pin), X (old tree f79f665d, cargo build+strip), Y (current, cargo build+strip), Z (current, cinstall), S (current, cinstall shipped); ratios + paired bootstrap decomposition. |
| Implementation | bash, 191 lines; cargo build/cinstall ×4 (`--features tkrzw[,shipped]`), strip, gcc bisect from `/repo`, nm/readelf/sha256sum, taskset, python3 perf-ratio.py and perf-decomp.py |
| Inputs | env `SRC_OLD`, `SRC_CURR`, `CTRL_OUT`, `CTRL_RUNS` (20), `CTRL_CYCLES` (8), `CTRL_SIZES` ("1 8 16"), `CTRL_CPU`, `CTLTOOLS`, `CTRL_COMMIT`; /proc/cpuinfo, meminfo, loadavg; `/opt/libpinyin-tkrzw` |
| Outputs | `$OUT/environment.txt`, `build-identity.txt`, `speed.jsonl`, `run.err`, `load.txt`, `ratios-n<N>.md`, `decomp-n<N>.md`; hard gate exit 1 if X's sha256 ≠ `RECORD_X_SHA` |
| Platform | container (amd64 image) |
| Consumers | runners.json `tree-recipe-control` (not-a-gate); perf-decomp.py docstring; perf-build-recipe-audit, perf-cycle-ir-differential findings |
| State | /tmp/{X,Y,Z,S}-*.so, /tmp/bisect |
| Overlap | superset of run-shipped-check.sh |
| Constraints | Hand-written `RECORD_X_SHA=bf8d3b57…` and size 1688840 B; fixture-hook expectation "Z 2, S 0"; no pin check |

### BIS-B-41 — tools/bisection/two-context-diff.c

| Field | Value |
|---|---|
| Entry point | `two-context-diff <pinyin\|zhuyin> <lib.so> <systemdir> <userdir> <one-learns\|fini-reversed\|both-learn\|late-save>` or `two-context-diff cross <libpinyin.so> <libzhuyin.so> <systemdir> <userdir> <zhuyin-first\|pinyin-first>[-fini-reversed\|-late-save]` |
| Owning subsystem | bisection |
| Purpose | #538/#578: two contexts (same library, or one of each facade) on one user dir in one process — unsaved learning isolation, save return per context, open-counter per init/fini, which save wins; user.conf counter printed after every step. |
| Implementation | C (gnu11), 479 lines; dlopen (two handles in cross mode); `<glib.h>` direct |
| Inputs | argv; tables copied from open-counter-diff.c (first two rows) |
| Outputs | `scenario:`, `init A: ok`, `alloc A: ok`, `counter after <step>: <value\|absent\|no counter line>`, `import A: <word> ok`, `train A: <word> ok`, `view A: P <p> <py> <c>` / `view A: T <word> <freq>` / `view Z: R <typed> *sentence listed…`, `save A: ok\|unchanged`, `fini A: ok`; exit 0 / 1 (init NULL, save mismatch vs expectation, train failure) / 2 usage |
| Platform | Linux-only |
| Consumers | run-two-context-diff.sh; runners.json `two-context-diff`; user-store.md |
| State | userdir from argv |
| Overlap | `learn`, `list_candidates`, `sym(prefix)` duplicated from open-counter-diff.c |
| Constraints | `save_ctx(expect_written)` carries hand-written expectations (B unchanged in one-learns/fini-reversed); exits on mismatch |

### BIS-B-42 — tools/bisection/run-two-context-diff.sh

| Field | Value |
|---|---|
| Entry point | executed, no args |
| Owning subsystem | bisection |
| Purpose | Run 4 same-library scenarios × {pinyin, zhuyin} + 6 cross scenarios on both sides; verify expected step lines; byte diff. |
| Implementation | bash, 238 lines; gcc + pkg-config glib; sources oracle-cell.sh |
| Inputs | env `TWO_CONTEXT_ORACLE_PREFIX` (→ PINYIN_ORACLE_PREFIX), `TWO_CONTEXT_PINYIN_SO`, `TWO_CONTEXT_ZHUYIN_SO` (→ OXPINYIN_CAPI_SO / OXPINYIN_ZHUYIN_SO → $CARGO_TARGET_DIR/debug/*), `TWO_CONTEXT_LIBS` ("pinyin zhuyin cross"), `TWO_CONTEXT_OUT`; `lib/libpinyin.so.15`, `lib/libzhuyin.so.15`, `data/table.conf` |
| Outputs | `$OUT/<lib>-<scenario>-{oracle,oxpinyin}.log(.stderr)`, `.diff`; `RESULT:`; exit 0 / 77 / 3 / 1 / 2 |
| Platform | Linux-only |
| Consumers | runners.json `two-context-diff` (gate, 538/578) |
| State | OUT/WORK pattern; per-run TMPDIR and user dir |
| Overlap | see BIS-B-05 |
| Constraints | Hard-coded pin grep; `verify_steps` hand-written expectations (`save B: unchanged`, `init P2`, `Z2`) |

### BIS-B-43 — tools/bisection/uncovered-surface-diff.c

| Field | Value |
|---|---|
| Entry point | `./uncovered-surface-diff <so> <systemdir>` |
| Owning subsystem | bisection |
| Purpose | W12 parked surfaces: A deep paging (pages 0..11 + last, deep choose idx 10, last-row choose) for shi/yi/ji/nihao; B punct prediction for 8 prefixes + punctuation bytes in composition; C option profiles (0x18a control, 0x38a DYNAMIC_ADJUST, FORCE_TONE 0x1ca, 0x60); D cursor moves (aux text + pinyin_offset at every byte, left/right offsets at 0,2,5,8, left/right walks, mid-buffer choose); E raw offsets at every byte fresh and after guess_sentence. |
| Implementation | C (gnu11), 931 lines; dlopen; g_free via glib dlopen; rm_rf |
| Inputs | argv 2; `pinyin_init`; sets PARITY 0x18a first ("the two libraries' bare defaults differ") |
| Outputs | `page-r0-<in>:parsed=/guess=/n=`, `…:page=<p>:row[i]=TYPE/nbest/text`, `…:proved=`, `…:sentence[i]=`, `…:deep-choose idx= type= text=`, `…:cursor=`, `page-last-<in>:tail-choose…`, `punct-<tag>:predict=/n=/head[i]=/punct[i]=/punct-n=`, `punctparse-<tag>:parsed=/n=`, `opt:0x..-<in>@<off>:parsed=/guess=/n=/row[i]/proved=/sentence[i]`, `cur:parsed= len=`, `cur:<c> aux=<ok>:<text> off=<ok>:<n>`, `cur:left-right@c off= left= right=`, `cur:left@c:n=`+rows, `cur:right@c…`, `cur:mid…`, `raw:<in>:parsed=`, `raw:<in>@<off>:guess=/n=/row[i]`, `raw:<in>:sentence=`, `raw:<in>+sent@<off>:…`; exit 0/1 |
| Platform | Linux-only |
| Consumers | run-uncovered-surface-diff.sh (in-source); runners.json `uncovered-surface-diff`; referenced by crates/oxpinyin-capi/src/union_e2e_tests.rs, crates/oxpinyin-engine/src/session/state.rs, docs/testing/f1-junk-aware-parse.md, oracle-bisect-differential-abort.md, many findings |
| State | `mkdtemp("/tmp/uncoveredsurface-user-XXXXXX")` rm_rf'd |
| Overlap | 8 prefixes shared with pred-order-diff.c; helpers "shared shape with live-typing-diff.c" (comment); type_name/print_candidate_rows like nbest-train-diff.c |
| Constraints | Symbols: 21 incl. set_options, get_pinyin_offset, get_left/right_pinyin_offset, get_full_pinyin_auxiliary_text, guess_predicted_candidates_with_punctuations; get_sentence only for proved indices ≤2 (pin aborts); offset 11 right-move never probed (pin aborts); apostrophe inputs excluded from raw phase |

### BIS-B-44 — tools/bisection/run-uncovered-surface-diff.sh

| Field | Value |
|---|---|
| Entry point | executed, no args |
| Owning subsystem | bisection |
| Purpose | Build/run the W12 driver both sides, require all five phase surfaces active (12 grep probes), full-log diff. |
| Implementation | bash, 170 lines; gcc; sources system-dir.sh + oracle-cell.sh; cargo via oracle_cell_artifact |
| Inputs | env `PINYIN_ORACLE_PREFIX`, `OXPINYIN_CAPI_SO`, `UNCOVERED_SYSTEM`/`OXPINYIN_SYSTEM_DIR` (needs `punct.<ext>` + interpolation2.text; `system_dir_detect_ext`), cell vars |
| Outputs | exit 0 / 77 / 1 (crash, inactive surface, diff error) / 2 ("expected, measured state — do not wire into CI green") |
| Platform | Linux-only |
| Consumers | runners.json `uncovered-surface-diff` (gate, W12); run-pred-order-diff.sh refers to its env; uncovered-surface-differentials.md |
| State | in-source binary; mktemp logs/err (trap rm) |
| Overlap | Standard oracle gate + bigram.db; vacuity probes like run-phrase-surface-diff.sh |
| Constraints | Hand-written expectations: `raw:nihao@3:n=0`, `raw:nihao+sent@3:n=1`, `raw:nihaoshijie@3:n=0`, `raw:nihaoshijie+sent@3:n=3`, `opt:0x60-ni3hao3@0:parsed=`, etc.; `punct.$SYSTEM_EXT` name check breaks for native "bin" layout (punct.bin) only by coincidence of ext string |

### BIS-B-45 — tools/bisection/union-diff.c

| Field | Value |
|---|---|
| Entry point | `./union-diff <so> <systemdir>` |
| Owning subsystem | bisection |
| Purpose | W11 union: (1) imported user phrase index + is_user, (2) `load_addon_phrase_library(4)` twice + ADDON candidates for "erhuang", (3) NBEST row choose returns the parse end (asserted == 9) then re-sentence + train, (4) train 测测→你好 then predict (types 4/5 only). |
| Implementation | C (gnu11), 380 lines; dlopen |
| Inputs | argv 2; prefers `oxpinyin_init_for_fixtures` |
| Outputs | `profile: …`, `user-pos: <idx> 测测 user=yes/no`, `load_addon(4): …`, `addon: <text>`, `nbest-cursor: N`, `nbest-resentence:`, `nbest-train:`, `predict:`, `pred: type=<t> text=`; exit 0/1 |
| Platform | Linux-only |
| Consumers | run-union-diff.sh (in-source), run-same-data-dir-diff.sh, runners.json `union-diff`; crates/oxpinyin-capi/src/predict.rs, parity-gates.md, ROADMAP.md, findings |
| State | `mkdtemp("/tmp/uniondiff-XXXXXX")`, never removed |
| Overlap | Section 4 = predict-diff.c; `find_by_text`/`index_of_text` duplicates |
| Constraints | **Hand-written expected value**: `cursor != strlen("cecenihao")` → exit 1 (pin pinyin.cpp:2513-2519); section 3 must precede 4 (comment) |

### BIS-B-46 — tools/bisection/union-edge-diff.c

| Field | Value |
|---|---|
| Entry point | `./union-edge-diff <so> <systemdir> setup\|plant\|predict <userdir>` |
| Owning subsystem | bisection |
| Purpose | Prediction filter edge (count 9 dropped, 10 kept): setup imports 测测/甲甲/乙乙 and saves; plant uses capi test hook `oxpinyin_test_set_user_bigram` (exit 3 if absent); predict prints whether each successor is a PREDICTED_BIGRAM. |
| Implementation | C (gnu11), 209 lines; dlopen |
| Inputs | argv 4; prefers fixture init |
| Outputs | `edge-setup: 测测 甲甲 乙乙`, `edge-plant: …`, `predict-edge: true/false`, `pred-edge-9: 甲甲 absent\|type=4`, `pred-edge-10: 乙乙 …`; exit 0 / 1 / 3 |
| Platform | Linux-only |
| Consumers | run-union-diff.sh (setup and predict only; plant mode unused — the runner plants with plant-oracle-bigram on both sides) |
| State | userdir from argv |
| Overlap | — |
| Constraints | Symbols incl. pinyin_save and optional `oxpinyin_test_set_user_bigram` |

### BIS-B-47 — tools/bisection/run-union-diff.sh

| Field | Value |
|---|---|
| Entry point | executed, no args |
| Owning subsystem | bisection |
| Purpose | union-diff both sides + filter-edge (setup → plant-oracle-bigram 0x07000001→0x07000002=9, →0x07000003=10 on both user dirs → predict), diff, and lock the constant (9 absent, 10 type=4). |
| Implementation | bash, 169 lines; gcc ×2, g++ (planter with backend-specific libs); sources oracle-cell.sh + system-dir.sh; cargo via oracle_cell_artifact |
| Inputs | env `PINYIN_ORACLE_PREFIX`, `PINYIN_ORACLE_DBM`, `OXPINYIN_CAPI_BACKEND_EXT` (db→-DORACLE_BDB -ldb; kct→-DORACLE_KC -lkyotocabinet; tkt→-ltkrzw -lpthread), `OXPINYIN_CAPI_SO`, `UNION_SYSTEM`/`OXPINYIN_SYSTEM_DIR` via resolve_system_dir |
| Outputs | capi/oracle logs echoed; `union-diff: IDENTICAL`, `union-diff filter-edge: IDENTICAL`; exit 0 / 77 / 1 / 2 (also 2 when the 9/10 expectations fail) |
| Platform | Linux-only (libdb/kc/tkrzw dev libs) |
| Consumers | runners.json `union-diff` (gate, W11); predict.rs, prediction-punct.md, runtime-direct-libpinyin-data finding |
| State | in-source binaries; two mktemp user dirs (trap rm); mktemp logs; stderr discarded |
| Overlap | Standard oracle gate (`grep -qxF` variant) |
| Constraints | Header says "0 identical or skipped" but skip exits 77; planter tokens hard-coded as first three USER_DICTIONARY tokens (pinyin.cpp:592-594) |

### BIS-B-48 — tools/bisection/user-candidate-diff.c

| Field | Value |
|---|---|
| Entry point | `./user-candidate-diff <so> <systemdir>` |
| Owning subsystem | bisection |
| Purpose | W11 user-phrase surface: import 测测/cece/5, parse "cece", guess 0x1e, print every `is_user` candidate as index/type/text. |
| Implementation | C (gnu11), 110 lines; dlopen |
| Inputs | argv 2; prefers fixture init |
| Outputs | `user <i>: type=<t> text=<s>`; exit 0/1 |
| Platform | Linux-only |
| Consumers | run-user-candidate-diff.sh → run-w11-diff.sh; run-same-data-dir-diff.sh; runners.json `user-candidate-diff`; probe-coverage-abi.md |
| State | `mkdtemp("/tmp/usercand-XXXXXX")`, never removed |
| Overlap | Section 1 of union-diff.c |
| Constraints | Symbols: init/fixtures, fini, alloc/free_instance, begin_add_phrases/iterator_add_phrase/end_add_phrases, parse_more_full_pinyins, guess_candidates, get_n_candidate, get_candidate, get_candidate_type, get_candidate_string, is_user_candidate |

### BIS-B-49 — tools/bisection/run-user-candidate-diff.sh

| Field | Value |
|---|---|
| Entry point | `run-user-candidate-diff.sh [capi.so]` → `exec ./run-w11-diff.sh user-candidate-diff "$@"` |
| Owning subsystem | bisection |
| Purpose | Alias. |
| Implementation | bash, 6 lines |
| Inputs | passthrough |
| Outputs | run-w11-diff.sh's |
| Platform | Linux-only |
| Consumers | runners.json `user-candidate-diff` (gate, W11) |
| State | none |
| Overlap | BIS-B-15/19 |
| Constraints | — |

### BIS-B-50 — tools/bisection/run-user-file-modes-diff.sh

| Field | Value |
|---|---|
| Entry point | executed, no args |
| Owning subsystem | bisection |
| Purpose | #544: file creation modes of every user-dir file under umasks 022/002/077, then after `chmod 0640` a second launch shows replace-vs-in-place writes; compare mode listings with the pin for pinyin and zhuyin. |
| Implementation | bash, 175 lines; gcc + pkg-config glib (builds open-counter-diff.c); GNU find `-printf '%m %f'`; sources oracle-cell.sh |
| Inputs | env `MODES_ORACLE_PREFIX` (→ PINYIN_ORACLE_PREFIX), `MODES_PINYIN_SO`, `MODES_ZHUYIN_SO` (→ OXPINYIN_*_SO → target), `MODES_LIBS`, `MODES_UMASKS` ("022 002 077"), `MODES_OUT`; `lib/lib{pinyin,zhuyin}.so.15`, `data/table.conf` |
| Outputs | `$OUT/<lib>-{oracle,oxpinyin}.log` (lines `umask M launch N[ after chmod 640]: <mode> <file>`), `.launch`, `.stderr`, `<lib>.diff`; `RESULT:`; exit 0 / 77 / 3 / 1 (launch failed or `save: 1` missing, empty listing) / 2 |
| Platform | Linux-only |
| Consumers | runners.json `user-file-modes-diff` (gate, 544) |
| State | OUT/WORK; per-umask user and TMPDIR dirs |
| Overlap | BIS-B-05 pattern; driver shared with open-counter |
| Constraints | Hard-coded pin grep; header documents pin modes per file class (0644 .tmp+rename; DB_CREATE 0600; KC ofstream 0666; user.conf fopen("w") in place) |

### BIS-B-51 — tools/bisection/run-w11-diff.sh

| Field | Value |
|---|---|
| Entry point | `run-w11-diff.sh <driver-stem> [prebuilt-capi.so] [driver-arg ...]` (stems: user-candidate-diff, addon-candidate-diff, predict-diff, punct-diff) |
| Owning subsystem | bisection |
| Purpose | Shared pin-gated runner: build `<stem>.c`, resolve capi and system dir, run both sides capturing stdout in memory, print both logs, diff. |
| Implementation | bash, 90 lines; gcc; sources oracle-cell.sh then system-dir.sh; cargo via oracle_cell_artifact |
| Inputs | argv; env `PINYIN_ORACLE_PREFIX`, `OXPINYIN_CAPI_SO`, `CAPI_W11_SYSTEM_DIR` (documented as the once-undocumented name) / `OXPINYIN_SYSTEM_DIR` via resolve_system_dir, cell vars |
| Outputs | `--- capi ---` log, `--- oracle ---` log, `DIVERGENCE: <stem> logs differ` + diff or `<stem>: IDENTICAL`; exit 0 / 77 (no oracle) / **1 when off-pin** (not 77) / 1 crash / 2 |
| Platform | Linux-only |
| Consumers | run-predict-diff.sh, run-punct-diff.sh, run-user-candidate-diff.sh, run-addon-candidate-diff.sh; runners.json `w11-diff` (command `run-w11-diff.sh predict-diff`) |
| State | mktemp WORK for the driver binary (trap rm); driver stderr discarded |
| Overlap | `capture_output` duplicated with run-same-data-dir-diff.sh; stem validated `[a-z0-9-]` |
| Constraints | Standard build flags into $WORK; off-pin is FAIL here where other wrappers SKIP |

### BIS-B-52 — tools/bisection/zhuyin-diff.c

| Field | Value |
|---|---|
| Entry point | `./zhuyin-diff <libzhuyin.so> <systemdir> [userdir] [noguess] [choose]` |
| Owning subsystem | bisection |
| Purpose | libzhuyin facade: STANDARD keyboard corpus of 12 inputs — `zhuyin_in_chewing_keyboard` per key, parse_more_chewings, parsed length, guess_sentence/get_sentence, then 13 lookup windows (after/before at 0, consumed, 3, 1, 2, 4, 5) each dumped in full with the 4-value type enum; `noguess` protocol skips the guess; `choose` protocol runs the before-cursor choose battery (3 inputs, row 1, cursor + sentence after choose and after re-guess). |
| Implementation | C (gnu11), 509 lines; dlopen; g_free/g_strfreev via glib dlopen |
| Inputs | argv; options USE_TONE\|FORCE_TONE (0x60, the pin default); scheme 1 |
| Outputs | `set_options(0x60):`, `set_chewing_scheme(1):`, `=== input: "" ===`, `in_chewing_keyboard('k'): … symbols=[…]`, `parse_chewing: consumed=`, `parsed_input_length:`, `guess_sentence:`, `get_sentence: … text=""`, `<label>: true/false` + `n_candidates:` + `  candidate[k]: type=<BEST_MATCH\|AFTER\|BEFORE\|ZOMBIE> text=""`, `reset_parsed_len:`; choose battery lines `zhuyin_choose_candidate(N, row 1) -> cursor:`; exit 0/1 |
| Platform | Linux-only |
| Consumers | run-zhuyin-diff.sh (mktemp WORK, `cc -O2 -Wall -Wextra` no -Werror); runners.json `zhuyin-diff` (rows 24-29); findings (bopomofo, upstream-divergences, revert-plan, compatibility-policy) |
| State | user dir arg or "" |
| Overlap | chewing-diff.c (pinyin facade bopomofo) covers similar parse surface |
| Constraints | Symbols (zhuyin_*): init, fini, alloc/free_instance, set_options, set_chewing_scheme, parse_more_chewings, get_parsed_input_length, in_chewing_keyboard, guess_sentence, get_sentence, guess_candidates_after_cursor, guess_candidates_before_cursor, choose_candidate, get_n_candidate, get_candidate, get_candidate_type, get_candidate_string, reset; every offset ≤ consumed (pin `_check_offset`) |

### BIS-B-53 — tools/bisection/run-zhuyin-diff.sh

| Field | Value |
|---|---|
| Entry point | executed, no args |
| Owning subsystem | bisection |
| Purpose | Run zhuyin-diff against the pinned libzhuyin (its own data) and the Rust libzhuyin (converted systemdir), require non-empty logs, diff. |
| Implementation | bash, 142 lines; `cc -O2 -Wall -Wextra`; cmp; sources oracle-cell.sh |
| Inputs | env `ZHUYIN_ORACLE_PREFIX` (→ PINYIN_ORACLE_PREFIX; required), `ZHUYIN_ORACLE_SO` (default `$PREFIX/lib/libzhuyin.so.15.0.0`), `ZHUYIN_ORACLE_DATA` (must be byte-identical to the prefix's data — cmp'd), `ZHUYIN_RUST_SO` (→ OXPINYIN_ZHUYIN_SO → $CARGO_TARGET_DIR/debug/libzhuyin_capi.so), `ZHUYIN_RUST_DATA` (required; needs interpolation2.text), `ZHUYIN_USER_DIR` |
| Outputs | `oracle log lines: N`, `rust log lines: N`, `IDENTICAL` / `DIFF FOUND` + diff; exit 0 / 77 (prefix or RUST_DATA unset) / 1 / 2 |
| Platform | Linux-only |
| Consumers | runners.json `zhuyin-diff` (env ZHUYIN_ORACLE_PREFIX={prefix}, ZHUYIN_RUST_SO={zhuyin}, ZHUYIN_RUST_DATA={data}, ZHUYIN_ORACLE_DATA={data}); probe-coverage-abi.md, bug-for-bug prereg |
| State | mktemp WORK (+ separate RUST_USER_DIR unless supplied) |
| Overlap | — |
| Constraints | No pin-ref grep (only dir/file existence); no system-dir.sh; header still says "tkrzw tables" for the Rust data |

### BIS-B-54 — tools/bisection/zhuyin-import-diff.c

| Field | Value |
|---|---|
| Entry point | `./zhuyin-import-diff <so> <systemdir> <index> [read\|rewrite]` (user dir = `./user` relative to cwd) |
| Owning subsystem | bisection |
| Purpose | #575: `zhuyin_iterator_add_phrase` battery of 16 adds (toned/unmarked bopomofo, romanized, apostrophe, trailing space, neutral tone, incomplete, length mismatch, counts −1/0/−2/INT_MIN/INT_MAX/+1, empty); for index <16 dump tokens, unigram and packed pronunciations live, save, reopen, dump again; cross-reader modes read/rewrite. |
| Implementation | C (gnu11), 207 lines; dlopen RTLD_LOCAL; `<glib.h>` direct (GArray) |
| Inputs | argv 3-4 |
| Outputs | `add <i> <phrase>\|<reading>\|<count>: true/false`, `<live\|reopened> <phrase> token=0x.. unigram=N prons=N [xxxx xxxx] …`, `save: true/false`, `init: NULL`/`reinit: NULL`; exit 0 / 1 / 2 usage |
| Platform | Linux-only |
| Consumers | run-zhuyin-import-diff.sh; run-system-import-round-trip.sh (zhuyin facade); runners.json `zhuyin-import-diff` (575/600/601) and `system-import-round-trip-zhuyin`; tkrzw-distro-compat.md |
| State | `mkdir("user")` in cwd |
| Overlap | Same argv protocol as import-semantics-diff.c |
| Constraints | Symbols (zhuyin_*): init, fini, save, begin_add_phrases, iterator_add_phrase, end_add_phrases, alloc/free_instance, lookup_tokens, token_get_n_pronunciation, token_get_nth_pronunciation, token_get_unigram_frequency; rewrite mode: `end_add(begin_add())` arms modified with no additions (pinyin.cpp:657) |

### BIS-B-55 — tools/bisection/run-zhuyin-import-diff.sh

| Field | Value |
|---|---|
| Entry point | `run-zhuyin-import-diff.sh <libzhuyin.so> <libzhuyin_capi.so> <data-dir>` |
| Owning subsystem | bisection |
| Purpose | Libraries 0 1 2 3 4 5 6 7 15 must be IDENTICAL; libraries 16 and 255: pin reads OOB (class (b)) so only oxpinyin's refusal (every add false, rc 0) is asserted. |
| Implementation | bash, 96 lines; gcc + pkg-config glib; sources oracle-cell.sh |
| Inputs | argv 3 (`OXPINYIN_ZHUYIN_SO` fallback for argv2) |
| Outputs | `library N: IDENTICAL (n lines)\|DIVERGENCE (head -40 diff)\|FAIL`, `library 16/255: oxpinyin refuses every add; pin rc=…`; `zhuyin-import-diff: IDENTICAL\|DIVERGENCE\|FAILURE`; exit 0 / 77 missing oracle / 1 / 2 |
| Platform | Linux-only |
| Consumers | runners.json `zhuyin-import-diff` (args `{prefix}/lib/libzhuyin.so {zhuyin} {prefix}/lib/libpinyin/data`) |
| State | mktemp work with per side-library cwd (trap rm); driver stderr discarded |
| Overlap | run-same-data-dir-diff.sh shape (both sides on one data dir, cwd isolation) |
| Constraints | No pin-ref check; no system-dir.sh |

### BIS-B-56 — tools/bisection/system-log-golden.cpp

| Field | Value |
|---|---|
| Entry point | `system-log-golden <out-file>`; **no wrapper builds it** (needs libpinyin's internal headers `config.h`, `phrase_index.h` and the pinyin namespace — a direct link against the pin's source tree, not dlopen) |
| Owning subsystem | fixtures / testsupport |
| Purpose | Produce the exact SYSTEM_FILE `.dbin` log bytes (PhraseIndexLogger diff of one item 你 token 1: unigram 100→583, pronunciation packed 0x0097 count 100→169) that `crates/oxpinyin-user/src/persistence.rs::cross_backend_state` encodes, to confirm the frozen `RECORDS_GOLDEN` gb_char.dbin against upstream classes. |
| Implementation | C++, 28 lines; libpinyin internal API (PhraseItem, SubPhraseIndex, PhraseIndexLogger, MemoryChunk::save) |
| Inputs | argv 1 |
| Outputs | writes the MemoryChunk log to argv[1]; exit 0 / 1 / 2 usage |
| Platform | Linux (pin build tree) |
| Consumers | none in-tree (grep finds no reference besides the file); persistence.rs test `the_saved_records_are_backend_independent` holds the golden it validates |
| State | none |
| Overlap | compare-nbest-training-state.py decodes the same MemoryChunk frame; oxpinyin-data user_files.rs::decode_log_records |
| Constraints | `static_assert(sizeof key == sizeof packed)`; hand-written values must match persistence.rs (100/583, 100/169, token 1, range_end 2) |

### BIS-B-57 — tools/bisection/tie-order-diff.c

| Field | Value |
|---|---|
| Entry point | `./tie-order-diff <so> <systemdir>` (user dir `$TMPDIR/tie-order-diff-<pid>`) — **no dedicated wrapper**; built only by run-same-data-dir-diff.sh |
| Owning subsystem | bisection |
| Purpose | #536: array order behind comparator ties — for option words {0x18a, 0x38a, 0x1fe0039a} × sort words {0x1c, 0x1e} × 29 inputs (initial-only keys + ni'h/zh'g/w'm), full candidate list (index, type, text), system phase then user phase after importing 10 one-character phrases under `n` readings into USER_DICTIONARY. |
| Implementation | C (gnu11), 211 lines; dlopen; `<glib.h>` direct (gint/guint types only; links glib); `remove_tree` recursive with lstat |
| Inputs | argv 2; env `TMPDIR` |
| Outputs | `options 0x..: <ok>`, `<phase>\|0x<opt>\|0x<sort>\|<input> parsed=N guess=B list=B n=N` + `  i\ttype\ttext` rows, `import <text> <pinyin>: <ok>`; exit 0/1 |
| Platform | Linux-only |
| Consumers | run-same-data-dir-diff.sh default list; no runners.json entry of its own; .gitignore does not list a binary for it |
| State | user dir created/removed under TMPDIR (honours TMPDIR, unlike most drivers) |
| Overlap | Symbol access via `sym()` global handle like open-counter-diff.c |
| Constraints | Excludes `n'h` (frozen class (a) trellis divergence); no tokens/unigram per row (10× slower; attribution in #620) |

---

## Count

57 records: BIS-B-01 nbest-train-diff.c, 02 run-nbest-train-diff.sh, 03 run-nbest-choose-train-diff.sh, 04 open-counter-diff.c, 05 run-open-counter-diff.sh, 06 option-sweep.c, 07 run-option-sweep.sh, 08 patches/bigram-export-strjoinv, 09 patches/nbest-tails-dump, 10 patches/residue-a-counterfactual, 11 phrase-surface-diff.c, 12 run-phrase-surface-diff.sh, 13 plant-oracle-bigram.cc, 14 predict-diff.c, 15 run-predict-diff.sh, 16 pred-order-diff.c, 17 run-pred-order-diff.sh, 18 punct-diff.c, 19 run-punct-diff.sh, 20 residue-a-tail-diff.c, 21 run-residue-a-tail-diff.sh, 22 residue-mechanism-diff.c, 23 run-residue-mechanism-diff.sh, 24 run-perf-baseline.sh, 25 run-perf-matrix.sh, 26 run-perf-same-data.sh, 27 run-rss-diag.sh, 28 run-same-data-dir-diff.sh, 29 scheme-diff.c, 30 run-scheme-diff.sh, 31 session-fork-diff.c, 32 run-session-fork-diff.sh, 33 run-shipped-check.sh, 34 system-import-profile.c, 35 run-system-import-profile.sh, 36 run-system-import-round-trip.sh, 37 train-diff.c, 38 run-train-diff.sh, 39 run-train-diff-dynamic-off.sh, 40 run-tree-recipe-control.sh, 41 two-context-diff.c, 42 run-two-context-diff.sh, 43 uncovered-surface-diff.c, 44 run-uncovered-surface-diff.sh, 45 union-diff.c, 46 union-edge-diff.c, 47 run-union-diff.sh, 48 user-candidate-diff.c, 49 run-user-candidate-diff.sh, 50 run-user-file-modes-diff.sh, 51 run-w11-diff.sh, 52 zhuyin-diff.c, 53 run-zhuyin-diff.sh, 54 zhuyin-import-diff.c, 55 run-zhuyin-import-diff.sh, 56 system-log-golden.cpp, 57 tie-order-diff.c.

## Architecture notes

Observation unless marked (interpretation).

**Wrapper↔driver map and orphans**
- Driver with no wrapper of its own: tie-order-diff.c (only run-same-data-dir-diff.sh builds it); system-log-golden.cpp (nothing builds it; needs the pin's internal headers). union-edge-diff.c's `plant` mode is dead (the runner plants with plant-oracle-bigram on both sides). nbest-tails-dump and residue-a-counterfactual patches have no registry/oracle-cell entry; only run-residue-a-tail-diff.sh (env-optional) and a findings doc use them.
- Wrappers that build another agent's driver: run-nbest-choose-train-diff.sh → candidate-assembly-diff.c (+ compare-nbest-training-state.py); run-system-import-round-trip.sh → import-semantics-diff.c; run-scheme-diff.sh → chewing-diff.c / fullpin-diff.c; run-user-file-modes-diff.sh → open-counter-diff.c; run-perf-*/rss/shipped/tree-recipe → bisect.c; run-same-data-dir-diff.sh → 15 drivers.
- Alias chain: run-{predict,punct,user-candidate}-diff.sh → run-w11-diff.sh (plus run-addon-candidate-diff.sh outside this range). runners.json registers both the aliases and `w11-diff predict-diff`, so predict-diff is run twice per sweep.

**Oracle discovery and pin-manifest parsing (every place oracle-pin.txt is read)**
- Cell-aware exact line `grep -Fxq "$EXPECTED_PIN_REF"` (from oracle-cell.sh, which itself parses `LIBPINYIN_VERSION=`, `LIBPINYIN_SHA=`, `MODEL_SHA256=` out of tools/oracle/build-oracle.sh): run-nbest-train-diff, run-option-sweep, run-phrase-surface-diff, run-pred-order-diff, run-residue-mechanism-diff, run-train-diff, run-uncovered-surface-diff, run-union-diff (`-qxF`), run-w11-diff.
- Cell-aware + patch digest (`${EXPECTED_PIN_REF}+patches-<sha>` and `patches_manifest_sha256=`): run-residue-a-tail-diff (tools/bisection/patches/bigram-export-strjoinv, also requires `oracle-patches.sha256` to name the patch); run-scheme-diff for full 2/3 (tools/oracle/patches, no manifest line check). oracle-cell.sh `oracle_cell_resolve_prefix` computes the same digest a third time.
- Hard-coded, non-cell-aware prefix grep `^pin_ref=libpinyin-2.11.92-074a2219…`: run-nbest-choose-train-diff, run-open-counter-diff, run-session-fork-diff, run-two-context-diff, run-user-file-modes-diff, run-scheme-diff (unpatched path), run-perf-baseline (`^pin_ref=.*<pin>`). (Interpretation: these accept a kc oracle on a bdb cell.)
- No pin check at all (file existence only or nothing): run-train-diff-dynamic-off, run-same-data-dir-diff, run-zhuyin-diff, run-zhuyin-import-diff, run-system-import-round-trip, run-system-import-profile, run-perf-matrix, run-perf-same-data, run-rss-diag, run-shipped-check, run-tree-recipe-control.
- `PINYIN_ORACLE_PREFIX` default `$HOME/.local/opt/pinyin-oracle` is retyped in 11 wrappers; four "user-store" wrappers take a runner-specific name first (`OPEN_COUNTER_ORACLE_PREFIX`, `SESSION_FORK_ORACLE_PREFIX`, `TWO_CONTEXT_ORACLE_PREFIX`, `MODES_ORACLE_PREFIX`, `ZHUYIN_ORACLE_PREFIX`) then fall back to `PINYIN_ORACLE_PREFIX`; their headers say "no default: a missing oracle fails the run, never skips" while the code exits 77 when unset and 3 when off-pin.
- Oracle .so name differs: `lib/libpinyin.so` (most), `lib/libpinyin.so.15` + `lib/libzhuyin.so.15` (open-counter, session-fork, two-context, user-file-modes), `lib/libzhuyin.so.15.0.0` (zhuyin-diff), registry passes `{prefix}/lib/libzhuyin.so` to zhuyin-import-diff and round-trip-zhuyin. Oracle data check `$ORACLE_DATA/bigram.db` (hard-coded libpinyin name) in nbest-train, residue-a, residue-mech, train-diff, uncovered, nbest-choose; `table.conf` in open-counter/session-fork/two-context/user-file-modes; `gb_char.bin` in same-data-dir.

**table.conf / datagen-manifest.txt / backend extension parsing**
- Only system-dir.sh parses `datagen-manifest.txt backend=` and `table.conf database format:` (`system_dir_data_ext`), used via resolve_system_dir (option-sweep, residue-a, residue-mech, scheme, train-diff, train-diff-dynamic-off, union, w11), `system_dir_detect_ext` (nbest-train, pred-order, uncovered — which skip 77 instead of fatal 3 when unresolved) and `system_dir_data_ext` directly (phrase-surface). run-open-counter-diff.sh parses user.conf itself (sed lines 1-3, `open counter:` via awk).
- Hard-coded backend names/extensions outside system-dir.sh: run-union-diff.sh (db/kct/tkt → -DORACLE_BDB/-DORACLE_KC/tkrzw libs), plant-oracle-bigram.cc (`#if ORACLE_BDB/ORACLE_KC/else`), run-perf-same-data.sh (`kyotocabinet|tkrzw` features, `libkyotocabinet.so`/`libtkrzw.so` NEEDED regex), run-perf-matrix/same-data/shipped/tree-recipe (`/opt/libpinyin-{tkrzw,kc}`, `/opt/oxpinyin-{tkrzw,kc}`; no bdb cell), run-shipped-check/tree-recipe (`--features tkrzw`, `tkrzw,shipped`), run-phrase-surface-diff (pinyin_index.bin/phrase_index.bin/bigram.db), run-perf-baseline (same + table.conf, *.table, addon_*_index.bin, punct.bin, gb_char.bin), run-system-import-round-trip (gb_char/gbk_char/opengram/merged .dbin), run-uncovered-surface-diff (`punct.$SYSTEM_EXT`), run-nbest-train-diff header (".db by default"), run-zhuyin-diff header ("tkrzw tables").

**cargo spawn points**
- `oracle_cell_artifact` (cargo build dev, cell feature) from: run-nbest-train-diff, run-option-sweep, run-phrase-surface-diff, run-pred-order-diff, run-residue-a-tail-diff, run-residue-mechanism-diff, run-scheme-diff, run-train-diff, run-train-diff-dynamic-off, run-uncovered-surface-diff, run-union-diff, run-w11-diff. Skipped when `OXPINYIN_CAPI_SO` is set (the registry always sets `{capi}` inputs via env for some runners but passes none for these — they rely on `OXPINYIN_CAPI_SO` exported by run-all.py; verify in BIS-A/C).
- Direct cargo: run-residue-a-tail-diff (`cargo build -p oxpinyin-runtime --example nbest_tail_probe`, only with RESIDUE_A_PROBE=1), run-perf-baseline (`cargo cinstall --release`, default features), run-perf-same-data (`cargo build --release` ×2), run-shipped-check (`cargo cinstall` ×2), run-tree-recipe-control (`cargo build` ×2 + `cargo cinstall` ×2). system-dir.sh runs `cargo tree --offline` when `OXPINYIN_CAPI_BACKEND_EXT` is unset.
- No cargo: nbest-choose, open-counter, session-fork, two-context, user-file-modes, zhuyin-diff, zhuyin-import, same-data-dir, round-trip, system-import-profile, perf-matrix, rss-diag.

**Compiler-flag families**
- `gcc -std=gnu11 -Wall -Wextra -Werror -O2 … -ldl`: 13 wrappers. Plus `$(pkg-config --cflags --libs glib-2.0)`: open-counter, session-fork, two-context, user-file-modes, zhuyin-import, round-trip (zhuyin side). `cc` instead of gcc with the same flags: system-import-profile, round-trip. Weaker: `cc -O2 -Wall` (phrase-surface), `cc -O2 -Wall -Wextra` (zhuyin-diff), `gcc -std=gnu11 -Wall -Wextra -O2` without -Werror (same-data-dir drivers, perf-same-data's bisect). `g++ -std=c++17 -Wall -Wextra -Werror -O2` (planter).
- Binary location: in-source tools/bisection/<name> (gitignored) for nbest-train, option-sweep, pred-order, residue-a, residue-mech, scheme/chewing/fullpin, train-diff, uncovered, union/union-edge/plant, bisect (perf-baseline, rss-diag, perf-same-data) — these carry `exclusive_resources: ["bisection-source-output"]` in runners.json; mktemp/$out for the rest. tie-order-diff has no gitignore entry.

**Driver code duplication (verbatim or near)**
- `rm_rf` (flat unlink+rmdir): nbest-train, pred-order, residue-a, residue-mech, train-diff, uncovered; `remove_tree` (recursive, lstat) in tie-order.
- `resolve_g_free` (dlopen libglib, fallback free): nbest-train, option-sweep, scheme, train-diff, uncovered, zhuyin-diff (+g_strfreev); residue-a/mech dlopen three glib symbols and abort if missing; system-import-profile dlsyms `g_free` from the pinyin .so handle; open-counter, session-fork, two-context, tie-order, zhuyin-import `#include <glib.h>`.
- `must()`/`load()`/`sym()`/`resolve_symbol()` are four spellings of "dlsym or exit 1"; `sym("<prefix>_<name>")` facade-switching only in open-counter and two-context.
- `find_by_text`: nbest-train, train-diff, union-diff, predict-diff (inline twice). `type_name` 8-entry table: nbest-train, uncovered; `ctype_name`: scheme-diff, zhuyin-diff (4-value). `print_candidate_rows`/`print_surface`/`print_top10` are three near-equivalent window dumpers.
- `train_target` + `list_candidates` + `import_phrase` + `target_rows` + `phrase_rows`: open-counter ≈ session-fork ≈ two-context. `dump_bigram`: residue-a ≡ residue-mech. Phase-B import block: residue-a `import_context` ≈ residue-mech main.
- Fixture-init policy differs silently: unconditional `oxpinyin_init_for_fixtures` preference (phrase-surface, predict, punct, user-candidate, union, union-edge) vs only-when-no-interpolation2.text (option-sweep, scheme, train-diff) vs never (nbest-train, pred-order, residue-*, uncovered, tie-order, open-counter, session-fork, two-context, system-import-profile, zhuyin-*). (Interpretation: on a shipped `.so` without the hook the first group still works; on a dev `.so` with real data they take the fixture path.)
- Temp user dirs: hard-coded `/tmp/<name>-XXXXXX` ignoring TMPDIR in 11 drivers; never removed in predict, punct, user-candidate, union; option-sweep only `rmdir`s. tie-order honours TMPDIR; session-fork/open-counter/two-context get TMPDIR set by their wrappers for leak detection.

**Wrapper duplication**
- Standard oracle gate block (prefix default, two-file check, SKIP 77, pin grep, SKIP 77) in ~12 files; OUT/WORK+trap and `ORACLE_SO[]`/`OX_SO[]` assoc arrays in 4; `capture_output` in run-w11-diff and run-same-data-dir; `run_one`/`run_speed_side` taskset+env+bisect in 5 perf scripts; `find_so` in perf-matrix and perf-same-data; `build_cinstall` in shipped-check and tree-recipe; interpolation2.text graft from `$PINYIN_MODEL_DIR`/`target/model20/extracted` in option-sweep and train-diff-dynamic-off; scheme-number validation twice inside run-scheme-diff.
- Comparison idioms: `diff -u oracle capi` (oracle first) everywhere; sorted diff only in run-train-diff / dynamic-off; in-memory capture in w11/same-data-dir; per-driver 60-line diff excerpt in same-data-dir; awk candidate-table + set/order classification only in option-sweep; position-count metric only in pred-order.

**Env var names and aliases**
- Sweep-wide: `PINYIN_ORACLE_PREFIX`, `PINYIN_ORACLE_DBM` (oracle-cell), `OXPINYIN_SYSTEM_DIR` (system-dir), `OXPINYIN_CAPI_SO`, `OXPINYIN_ZHUYIN_SO`, `OXPINYIN_CAPI_BACKEND_EXT`, `CARGO_TARGET_DIR`/`OXPINYIN_TARGET_ROOT`, `OXPINYIN_ALLOW_MINI_FIXTURE`, `PINYIN_MODEL_DIR`.
- Per-runner system-dir aliases: `NBEST_CAPI_SYSTEM`, `OPTION_SWEEP_CAPI_DATA`, `PHRASE_SYSTEM`, `UNCOVERED_SYSTEM` (shared by pred-order and uncovered), `RESIDUE_A_SYSTEM`, `RESIDUE_MECH_SYSTEM`, `W13_CAPI_SYSTEM` (+`W13_INTERPOLATION`), `TRAINDIFF_SYSTEM`, `UNION_SYSTEM`, `CAPI_W11_SYSTEM_DIR`, `ZHUYIN_RUST_DATA`.
- Per-runner .so aliases: `RESIDUE_A_CAPI_SO`, `OPEN_COUNTER_PINYIN_SO/ZHUYIN_SO`, `SESSION_FORK_PINYIN_SO` (registry also sets `SESSION_FORK_ZHUYIN_SO`, unread), `TWO_CONTEXT_PINYIN_SO/ZHUYIN_SO`, `MODES_PINYIN_SO/ZHUYIN_SO`, `ZHUYIN_RUST_SO`, `ZHUYIN_ORACLE_SO/DATA`, `OXPINYIN_KC_SO/OXPINYIN_TKRZW_SO`, `OXPINYIN_NBEST_TAIL_PROBE`.
- Driver knobs: `NBESTTRAINDIFF_ROUNDS`, `NBESTTRAINDIFF_USER_ROUNDS` (wrapper default 3 ≠ driver default 1), `TRAINDIFF_ROUNDS/OPTIONS/MASK/DUMP_CANDIDATES/REOPEN`, `TRAINDIFF_OFF_OPTIONS`, `OPTION_SWEEP_SORT`, `SCHEME_DIFF_USER_DIR`, `SCHEME_DIFF_TONE`, `SCHEME_DIFF_PARSE_AUX_ONLY`, `OXPINYIN_DUMP_TAILS` (patched pin), `RESIDUE_A_SAME_DIR/DUMP_PREFIX/PROBE/OUT`, `RESIDUE_MECH_SAME_DIR/OUT`, `OPEN_COUNTER_LIBS/PROTOCOLS/CYCLES/CRASHES/OUT`, `SESSION_FORK_MODES/TIMEOUT/OUT`, `TWO_CONTEXT_LIBS/OUT`, `MODES_LIBS/UMASKS/OUT`, `ZHUYIN_USER_DIR`, `PERF_CPU/RUNS/CYCLES/REPEATS/RAM_RUNS/REUSE_STAGE`, `OXPINYIN_PERF_WORK`, `PINYIN_EXPORT_DIR`, `MATRIX_OUT`, `CTRL_OUT/RUNS/CYCLES/SIZES/CPU/COMMIT`, `SRC_OLD/SRC_CURR/CTLTOOLS`, `PERF_BACKEND/PERF_MODE/RSS_DIAG_DIR/RSS_DIAG_TAG` (to bisect), `TMPDIR`, `CAND_ASSEMBLY_TRAIN_DIR`, `PINYIN_PATCHED_ORACLE_PREFIX`.

**Exit-code conventions**
- Dominant: 0 identical / 77 skip / 1 build-run / 2 divergence. Extra 3 = "required input missing or off-pin" (open-counter, session-fork, two-context, user-file-modes) or system-dir fatal; 3 = probe family inactive only in run-phrase-surface-diff; 3 = data incomplete in run-option-sweep. run-w11-diff turns off-pin into 1. Perf recipes exit 1 on any fatal, never 77. Driver usage errors: 1 (most), 2 (open-counter, session-fork, two-context, zhuyin-import, system-import-profile, run-rss-diag). SIGKILL 137 is a legitimate status in open-counter (crash) and session-fork (crash); 134 (SIGABRT) is the expected oracle status in open-counter's abort channel.
- Doc/code mismatches observed: run-train-diff ("exit 0" on absent oracle vs 77), run-union-diff ("0 identical or skipped" vs 77), open-counter/session-fork/two-context/modes ("never skips" vs 77 when unset), run-open-counter default PROTOCOLS (header omits `abort`). run-pred-order-diff and run-uncovered-surface-diff are registered as `kind: gate` though their headers call exit 2 the expected state.

**Process control / timeouts / locks**
- Only run-session-fork-diff uses `timeout -k 5` (60 s default). run-all.py applies runners.json timeouts (900 s; 1800 for open-counter). `exclusive_resources` (`bisection-source-output`, `surface-native-output`) are the only lock-like mechanism, declared in runners.json, not in the scripts. Subshell-reap pattern for SIGKILLed children in open-counter and session-fork. taskset CPU pinning in all five perf scripts (fallback without taskset in baseline/matrix/same-data only). Suspend-overlap detection (CLOCK_BOOTTIME−CLOCK_MONOTONIC) only in run-system-import-profile.

**Hand-written expected values (not just diff-equality)**
- union-diff.c cursor == 9; run-union-diff `pred-edge-9 … absent` / `pred-edge-10 … type=4`; plant-oracle-bigram requires counts 9 and 10; run-phrase-surface and run-uncovered-surface probe-line sets; run-nbest-train ≥1 `cand[0]=NBEST`; run-open-counter abort expectations (134 / `init: NULL` / exit 1 / one class-(c) line / dir digest) and glibc-%d seed comments; run-two-context `verify_steps`; two-context-diff.c `save_ctx(expect_written)`; open-counter-diff.c `_Static_assert` table sizes; run-tree-recipe `RECORD_X_SHA` + fixture-hook counts; run-system-import-profile dump counts (4 or 1); system-import-profile.c row counts 500/200/10000; run-same-data-dir dynamic-adjust 7 CHOICE lines; run-option-sweep W12 residual set and bit lists.

**Frozen / "do not change" statements in files**
- residue-a-counterfactual patch: "NEVER part of the shipped tree". bigram-export-strjoinv: patched prefix "must use its own --prefix". run-pred-order-diff / run-uncovered-surface-diff: "do not wire into CI green until…". run-rss-diag: measurement rounds must leave RSS_DIAG_DIR unset. run-perf-same-data: backend never inherited from workspace defaults. run-tree-recipe-control: X must reproduce the record byte-for-byte before any ratio is read. union-diff.c: section 3 MUST run before 4. open-counter-diff.c: learned state read in-process, bigram export once per context. run-predict/punct/user-candidate/union: "does not edit run-import-diff.sh or run-train-diff.sh". system-import-profile.c: `--export-system` unmeasured; no wall-clock timing. docs/runbooks/benches.md records the reviewed source SHA-256 of system-import-profile.c (`565be2c8…`) — (interpretation) any edit to that file invalidates the registered #757–#759 / 2026-10-14 figures' provenance line.

**Measurement tooling summary (perf/profile records)**
- bisect `--perf` (bisect.c, other agent): wall-clock ns per init/alloc/cycle (`init_ns`, `alloc_ns`, `cycles_ns[]`), `/proc/self/status` RSS/HWM (`rss_kib`, `hwm_kib`, nested `after_init`/`after_last`), `rss-diag` adds smaps_rollup, mallinfo2, malloc_trim(0) delta, and optional `/proc/self/maps`, `smaps`, `malloc_info` dumps — consumed by run-perf-baseline (speed/ram-init/ram-cycle + size via perf-baseline.py), run-perf-matrix (perf-matrix.py), run-perf-same-data (inline python medians), run-rss-diag (jsonl + identity.txt), run-shipped-check / run-tree-recipe-control (perf-ratio.py, perf-decomp.py).
- valgrind: only run-system-import-profile.sh (callgrind `--combine-dumps=yes` with client-request dumps from the driver; massif heap and `--pages-as-heap`), outputs to /dev/stderr, parsed with regex, nothing written to disk except the driver and user dirs. No `perf`, no `/proc/smaps` parsing outside bisect.c in this range.


---

# BIS-P — tools/bisection orchestration and Python layer

Tree read: `/tmp/claude-1000/-home-sheng-repos-oxpinyin/75efaa0c-4dbb-478f-81c2-905b9635f463/scratchpad/ox` (detached worktree at origin/main). Every file below was read in full; counts below come from executing `contract-diff.py`'s registration code read-only (no tree writes).

### BIS-P-01 — tools/bisection/runners.json

| Field | Value |
|---|---|
| Entry point | JSON registry read by `run-all.py` (`json.loads((HERE / 'runners.json').read_text())`); not executed. |
| Owning subsystem | bisection |
| Purpose | The single registry of every parity gate and measurement recipe: which script, with which argv and env, on which cells, with which oracle variant, timeout, locks and skip rules. |
| Implementation | JSON; 1756 lines; 55 entries (46 `gate`, 9 `not-a-gate`). |
| Inputs | Substitution tokens resolved by run-all.py (see schema below). |
| Outputs | n/a (data). |
| Platform | portable data; every command is Linux-first bash/python. |
| Consumers | `run-all.py` (only reader); `docs/runbooks/parity-gates.md` documents it. |
| State | none. |
| Overlap | The `required_inputs` strings duplicate what each run-*.sh checks at runtime (prose, not validated); `cells` is `["bdb","kc","tkrzw"]` on all 55 rows (never narrows). |
| Constraints | Keys `command[0]` are always `bash` or `python3`; `oracle_variant` only `bigram-export-strjoinv` (2 rows) and must match `oracle_cell_resolve_prefix` in oracle-cell.sh; `timeout_reason` present on exactly one row (`transformed-options-sweep`, 3600 s). |

#### Schema (every key observed, meaning as used by run-all.py)

| Key | Type | Present on | Meaning in run-all.py |
|---|---|---|---|
| `name` | str | all 55 | Registry key; `--runners` and `--runner-timeout NAME=S` and `--allow-skipped NAME[:CELL]` match it; result column `runner`; work dir `<scratch>/<name>-<cell>`. |
| `kind` | `gate` \| `not-a-gate` | all | `not-a-gate` rows are dropped silently from an unfiltered run; naming one explicitly in `--runners` is a `parser.error` ("not-a-gate (measurement recipes are never executed)"). `--list` prints both kinds. |
| `issues` | [str] | all | Tags for `--issue` (compared lower-cased, leading `#` stripped): numbers, `W11`, `row:N`, `coverage:N`, `register:…`, `task:N`, `lane-C-contract`, `measurement recipe`. |
| `cells` | [str] | all | Cell membership; a selected cell not listed → `SKIPPED` / `unsupported cell` before any oracle check. |
| `required_inputs` | [str] | all | Prose only; never read by code. |
| `timeout` | int seconds | all | Default per-runner deadline (overridden by `--timeout`, then `--runner-timeout`). 900 on 53 rows, 1800 (`open-counter-diff`), 3600 (`transformed-options-sweep`). |
| `timeout_reason` | str | 1 | Prose. |
| `exclusive_resources` | [str] | all | Names of `flock` files under `<ROOT>/target/runner-locks/`; taken in sorted order with `LOCK_EX` around `execute`. Values used: `bisection-source-output` (24 rows), `surface-native-output` (3 rows), `[]` (28 rows). |
| `command` | [str] | all | argv; every element is `.format_map(substitutions)`-expanded (tokens below). Run with `cwd=ROOT`. |
| `env` | {str: str} | all (often `{}`) | Extra env per runner; values are `format_map`-expanded with the same tokens **except** `{env[...]}` (the `env` token is installed after this expansion). |
| `input_paths` | [str] | all | Expanded; any missing path → `SKIPPED` `missing required input: …` (or `FAIL` when `oracle_variant` ≠ `unpatched`). |
| `oracle` | bool | 5 rows explicit (`false` ×4: cpp-smoke, abi-exports, abi-alloc, pc-version; `true` ×2: pc-metadata, user-dir-round-trip); absent = `True` | When truthy, the oracle-prefix checks run (manifest/pin/patch, decision table below). |
| `oracle_variant` | str | 2 rows (`bigram-export-diff`, `residue-a-tail-diff`) = `bigram-export-strjoinv`; absent = `unpatched` | Passed to `cell-artifacts.sh --describe-oracle PREFIX VARIANT` → runner-specific `PINYIN_ORACLE_PREFIX` (`<prefix>-<variant>`), `EXPECTED_ORACLE_PIN_REF` (with `+patches-<digest>`), `ORACLE_PATCH_FILE`. Also flips missing-oracle and missing-input outcomes from SKIPPED to FAIL. |
| `optional_skips` | [str] | 1 row (`bisect`) | Substrings; an output line containing `SKIP:` that also contains one of these is not a "required skip" (does not promote rc to 77). |
| `required_env` | [str] | 1 row (`pc-metadata`) | Names that must be non-empty in the runner env, else `SKIPPED` `missing required environment: …`. |
| `reason` | str | 9 `not-a-gate` rows | Prose (`measurement recipe; excluded from aggregate gates`). |

Substitution tokens (`format_map` on `command`, `input_paths`, and — minus `env` — on `env` values): `{root}` = repo root; `{prefix}` = runner oracle prefix (variant-resolved); `{data}` = system data dir (`--data CELL=DIR` > `$OXPINYIN_SYSTEM_DIR` (single-cell only) > `<base prefix>/lib/libpinyin/data` — note: always the **unpatched** base prefix's data even for a variant runner); `{cell}` = `bdb|kc|tkrzw`; `{capi}` = `<CARGO_TARGET_DIR>/debug/libpinyin_capi.so`; `{zhuyin}` = `<CARGO_TARGET_DIR>/debug/libzhuyin_capi.so`; `{work}` = `<scratch>/<name>-<cell>` (also exported as `TMPDIR`); `{env[NAME]}` = the fully assembled runner env (used by `pc-metadata` and `user-dir-round-trip` only). There is no `{dictool}` token; `OXPINYIN_DICTOOL` is reachable only as an env var.

#### Complete table (55 entries)

| name | kind | command[0..2] | oracle | oracle_variant | exclusive_resources | required_inputs (n) | input_paths | env keys | issues |
|---|---|---|---|---|---|---|---|---|---|
| abi-probe-diff | gate | bash run-abi-probe-diff.sh | (absent→True) | — | bisection-source-output | 5 | {capi} | — | row:33,row:34,row:35,row:36,row:37 |
| addon-candidate-diff | gate | bash run-addon-candidate-diff.sh | (absent→True) | — | bisection-source-output | 5 | {capi} | — | W11 |
| bigram-export-diff | gate | bash run-bigram-export-diff.sh {prefix}/lib/libpinyin.so | (absent→True) | bigram-export-strjoinv | — | 5 | {capi} | — | 541,row:1 |
| bisect | gate | bash run-bisect.sh {prefix}/lib/libpinyin.so | (absent→True) | — | bisection-source-output | 5 | {capi} | — (optional_skips ×3) | coverage:20,coverage:2,coverage:4 |
| callgrind-differential | not-a-gate | bash run-callgrind-differential.sh | (absent→True) | — | — | 2 | — | — | measurement recipe |
| candidate-assembly-diff | gate | bash run-candidate-assembly-diff.sh {prefix} | (absent→True) | — | — | 5 | {capi},{zhuyin},{prefix}/lib/libzhuyin.so | — | 582 |
| contract-diff | gate | bash run-contract-diff.sh {cell} | (absent→True) | — | — | 4 | {capi},{zhuyin},{prefix}/lib/libzhuyin.so | — | lane-C-contract |
| cpp-smoke | gate | bash run-cpp-smoke.sh | False | — | — | 3 | {capi} | — | register:abi-reference |
| dict-surface-diff | gate | bash run-dict-surface-diff.sh | (absent→True) | — | surface-native-output | 5 | {capi} | DICT_SYSTEM | register:probe-coverage-abi/Tier-C |
| dynamic-adjust-diff | gate | bash run-dynamic-adjust-diff.sh | (absent→True) | — | — | 5 | {capi} | — | W10 |
| export-diff | gate | bash run-export-diff.sh {prefix}/lib/libpinyin.so | (absent→True) | — | — | 4 | {capi} | — | 533 |
| import-diff | gate | bash run-import-diff.sh | (absent→True) | — | bisection-source-output | 8 | {capi} | IMPORT_SYSTEM | W7-T1 |
| import-semantics-diff | gate | bash run-import-semantics-diff.sh {prefix}/lib/libpinyin.so | (absent→True) | — | — | 4 | {capi} | — | 534 |
| key-surface-diff | gate | bash run-key-surface-diff.sh | (absent→True) | — | surface-native-output | 5 | {capi} | KEY_SYSTEM | 109 |
| live-typing-diff | gate | bash run-live-typing-diff.sh | (absent→True) | — | bisection-source-output | 5 | {capi} | — | W12 |
| locale-diff | gate | bash run-locale-diff.sh | (absent→True) | — | — | 6 | {capi},{zhuyin},{prefix}/lib/libzhuyin.so | LOCALE_DIFF_ORACLE_PREFIX,LOCALE_DIFF_PINYIN_SO,LOCALE_DIFF_ZHUYIN_SO | 539,619,545 |
| nbest-choose-train-diff | gate | bash run-nbest-choose-train-diff.sh {cell} | (absent→True) | — | — | 6 | {capi},{zhuyin},{prefix}/lib/libzhuyin.so | — | row:38 |
| nbest-train-diff | gate | bash run-nbest-train-diff.sh | (absent→True) | — | bisection-source-output | 5 | {capi} | — | W14 |
| open-counter-diff | gate | bash run-open-counter-diff.sh | (absent→True) | — | — | 5 (timeout 1800) | {capi},{zhuyin},{prefix}/lib/libzhuyin.so | OPEN_COUNTER_ORACLE_PREFIX,OPEN_COUNTER_PINYIN_SO,OPEN_COUNTER_ZHUYIN_SO | 523,583,589,590 |
| option-sweep | gate | bash run-option-sweep.sh | (absent→True) | — | bisection-source-output | 6 | {capi} | — | W10,row:17 |
| perf-baseline | not-a-gate | bash run-perf-baseline.sh | (absent→True) | — | — | 2 | — | — | 370 |
| perf-matrix | not-a-gate | bash run-perf-matrix.sh | (absent→True) | — | — | 2 | — | — | measurement recipe |
| perf-same-data | not-a-gate | bash run-perf-same-data.sh | (absent→True) | — | — | 2 | — | — | 260 |
| phrase-surface-diff | gate | bash run-phrase-surface-diff.sh | (absent→True) | — | surface-native-output | 5 | {capi} | PHRASE_SYSTEM | coverage:68,coverage:69,coverage:70 |
| pred-order-diff | gate | bash run-pred-order-diff.sh | (absent→True) | — | bisection-source-output | 5 | {capi} | — | row:12 |
| predict-diff | gate | bash run-predict-diff.sh | (absent→True) | — | bisection-source-output | 4 | {capi} | — | W11,104 |
| punct-diff | gate | bash run-punct-diff.sh | (absent→True) | — | bisection-source-output | 4 | {capi} | — | 104 |
| residue-a-tail-diff | gate | bash run-residue-a-tail-diff.sh | (absent→True) | bigram-export-strjoinv | bisection-source-output | 6 | {capi} | RESIDUE_A_CAPI_SO | row:33,row:34,row:35,row:37,row:1 |
| residue-mechanism-diff | gate | bash run-residue-mechanism-diff.sh | (absent→True) | — | bisection-source-output | 5 | {capi} | — | row:34 |
| rss-diag | not-a-gate | bash run-rss-diag.sh | (absent→True) | — | — | 2 | — | — | measurement recipe |
| same-data-dir-diff | gate | bash run-same-data-dir-diff.sh {prefix}/lib/libpinyin.so | (absent→True) | — | — | 4 | {capi} | — | 635,636,637 |
| scheme-diff | gate | bash run-scheme-diff.sh | (absent→True) | — | bisection-source-output | 5 | {capi} | — | W13,W15 |
| session-fork-diff | gate | bash run-session-fork-diff.sh | (absent→True) | — | — | 5 | {capi} | SESSION_FORK_ORACLE_PREFIX,SESSION_FORK_PINYIN_SO,SESSION_FORK_ZHUYIN_SO | 531,546 |
| shipped-check | not-a-gate | bash run-shipped-check.sh | (absent→True) | — | — | 2 | — | — | measurement recipe |
| system-import-profile | not-a-gate | bash run-system-import-profile.sh | (absent→True) | — | — | 2 | — | — | 599 |
| system-import-round-trip | gate | bash run-system-import-round-trip.sh pinyin | (absent→True) | — | — | 4 | {capi} | — | 599 |
| train-diff-dynamic-off | gate | bash run-train-diff-dynamic-off.sh | (absent→True) | — | bisection-source-output | 5 | {capi} | — | W10 |
| train-diff | gate | bash run-train-diff.sh | (absent→True) | — | bisection-source-output | 5 | {capi} | — | W6-T7 |
| tree-recipe-control | not-a-gate | bash run-tree-recipe-control.sh | (absent→True) | — | — | 2 | — | — | measurement recipe |
| two-context-diff | gate | bash run-two-context-diff.sh | (absent→True) | — | — | 5 | {capi},{zhuyin},{prefix}/lib/libzhuyin.so | TWO_CONTEXT_ORACLE_PREFIX,TWO_CONTEXT_PINYIN_SO,TWO_CONTEXT_ZHUYIN_SO | 538,578 |
| uncovered-surface-diff | gate | bash run-uncovered-surface-diff.sh | (absent→True) | — | bisection-source-output | 5 | {capi} | — | W12 |
| union-diff | gate | bash run-union-diff.sh | (absent→True) | — | bisection-source-output | 5 | {capi} | — | W11 |
| user-candidate-diff | gate | bash run-user-candidate-diff.sh | (absent→True) | — | bisection-source-output | 4 | {capi} | — | W11 |
| user-file-modes-diff | gate | bash run-user-file-modes-diff.sh | (absent→True) | — | — | 5 | {capi},{zhuyin},{prefix}/lib/libzhuyin.so | MODES_ORACLE_PREFIX,MODES_PINYIN_SO,MODES_ZHUYIN_SO | 544 |
| w11-diff | gate | bash run-w11-diff.sh predict-diff | (absent→True) | — | bisection-source-output | 5 | {capi} | — | W11 |
| zhuyin-diff | gate | bash run-zhuyin-diff.sh | (absent→True) | — | — | 6 | {capi},{zhuyin},{prefix}/lib/libzhuyin.so | ZHUYIN_ORACLE_PREFIX,ZHUYIN_RUST_SO,ZHUYIN_RUST_DATA,ZHUYIN_ORACLE_DATA | row:24..row:29 |
| zhuyin-import-diff | gate | bash run-zhuyin-import-diff.sh {prefix}/lib/libzhuyin.so | (absent→True) | — | — | 5 | {capi},{zhuyin},{prefix}/lib/libzhuyin.so | — | 575,600,601 |
| system-import-round-trip-zhuyin | gate | bash run-system-import-round-trip.sh zhuyin | (absent→True) | — | — | 4 | {zhuyin},{prefix}/lib/libzhuyin.so | — | 599 |
| train-index-diff | gate | python3 train-index-diff.py {cell} | (absent→True) | — | — | 3 | {capi},{zhuyin} | — | row:38 |
| transformed-options-sweep | gate | python3 {root}/tools/oracle/transformed-options-sweep.py --oracle | (absent→True) | — | — | 3 (timeout 3600) | {capi} | — | 634 |
| abi-exports | gate | bash {root}/tools/abi/check-exports.sh | False | — | — | 2 | {capi},{zhuyin} | — | register:libpinyin.ver,register:libzhuyin.ver |
| abi-alloc | gate | bash {root}/tools/abi/check-alloc-pairing.sh | False | — | — | 3 | {capi},{zhuyin} | — | register:libpinyin.alloc,register:libzhuyin.alloc |
| pc-version | gate | bash {root}/tools/packaging/check-pc-version.sh | False | — | — | 2 | — | — | register:oracle-pin.txt |
| pc-metadata | gate | bash {root}/tools/packaging/check-pc-metadata.sh --pin-root={env[OXPINYIN_METADATA_PIN_ROOT]} | True | — | — | 3 | {env[OXPINYIN_METADATA_PIN_ROOT]},{env[OXPINYIN_METADATA_SUBJECT_ROOT]} | — (required_env ×3) | register:oracle-pin.txt |
| user-dir-round-trip | gate | bash {root}/tools/oracle/user-dir-round-trip.sh {prefix} | True | — | — | 4 | {capi},{env[OXPINYIN_USER_RT_TEST]} | — | task:9,599 |

(`command[0..2]` abbreviated by stripping `{root}/tools/bisection/`; "required_inputs (n)" is the element count of the prose list.)

### BIS-P-02 — tools/bisection/run-all.py

| Field | Value |
|---|---|
| Entry point | `python3 tools/bisection/run-all.py [--runners a,b] [--cells bdb,kc,tkrzw] [--issue X]* [--list] [--jobs N] [--timeout S] [--runner-timeout R=S]* [--allow-skipped R[:CELL]\|*]* [--oracle-root DIR] [--oracle CELL=PREFIX]* [--cell-env CELL:NAME=VALUE]* [--ibus-build CELL=DIR]* [--data CELL=DIR]* [--target-root DIR] [--no-build] [--build-timeout S] [--json PATH]`; `exec`'d by run-all.sh. |
| Owning subsystem | bisection |
| Purpose | Executes the gate rows of runners.json per cell, builds the cell's dev artifacts once, enforces oracle manifest/patch expectations, and reduces every runner's exit/output to PASS/SKIPPED/FAIL with one aggregate exit code. |
| Implementation | Python 3 stdlib only (argparse, concurrent.futures, fcntl, json, signal, subprocess, tempfile); 316 lines; shells out to bash (`cell-artifacts.sh`) which in turn runs cargo. |
| Inputs | `runners.json`; env read: `OXPINYIN_ORACLE_ROOT` (default `~/.local/opt/pinyin-oracle`), `OXPINYIN_TARGET_ROOT` (default `<ROOT>/target/cells`), `CARGO_TARGET_DIR` (rejected when >1 cell), `PINYIN_ORACLE_PREFIX` and `OXPINYIN_SYSTEM_DIR` (honoured only for a single cell), `PINYIN_IBUS_BUILD_DIR` (popped in multi-cell runs unless `--ibus-build`); the whole `os.environ` is the base env of every runner. Files: `<prefix>/oracle-pin.txt`, `<prefix>/oracle-patches.sha256`, `<prefix>/lib/libpinyin.so`, `input_paths`. |
| Outputs | stderr progress (`Building dev opt-level 1 artifacts: <cell>`, `Completed <runner> / <cell>: <status>`); stdout Markdown table `runner \| cell \| status \| seconds \| pin_ref \| reason`; `--json PATH` same rows as JSON list; exit 0 iff no FAIL and every SKIPPED is allowed; exit 1 otherwise or on `OSError/ValueError/JSONDecodeError` (`FAIL: …` on stderr); argparse errors exit 2. |
| Platform | Linux-only (`fcntl.flock`, `os.killpg`, `start_new_session`). |
| Consumers | `run-all.sh`; documented in `docs/runbooks/parity-gates.md`, `docs/findings/tooling-lane-gates.md`. Not referenced by any `.github/workflows/*`. |
| State | `tempfile.TemporaryDirectory(prefix='oxpinyin-gates-')` scratch holding `build-<cell>.log` and `<runner>-<cell>/runner.log` (deleted on exit — raw logs are not retained); per-runner `TMPDIR=<work>`; persistent lock dir `<ROOT>/target/runner-locks/<resource>`; cell target dirs `<target-root>/<cell>`; process-group kill on timeout. |
| Overlap | Oracle-manifest check duplicates what every `run-*.sh` re-checks via `EXPECTED_PIN_REF`; `reason_for` regex overlaps each runner's own PASS/SKIP/FAIL vocabulary; the artifact paths (`debug/libpinyin_capi.so` …) are hard-coded here and in oracle-cell.sh's `oracle_cell_artifact`. |
| Constraints | Docstring: "never cache the oracle". Comment: "Legacy CI behavior is preserved in the direct live-typing runner; aggregate skips cannot silently turn into passes". Protected env names cannot be set via `--cell-env`. Only the aggregate deadline counts as timeout ("a runner that wraps its own processes in timeout(1) and exits 124 is reported by its own output"). |

#### run-all.py — behaviours a replacement must preserve verbatim

**Selection.** `selected` = `--runners` split on `,` (any unknown name → `parser.error('unknown runners: …')`) else all names. `--issue` values are lower-cased and `lstrip('#')`; a row survives if any of its `issues` tags (same normalisation) intersects. Empty selection → `parser.error('selection contains no runners')`. `--list` prints every surviving row (gates and not-a-gates) as one `json.dumps(row, ensure_ascii=False)` line each and exits 0 without building. After `--list`: if `--runners` was given and any surviving row is `not-a-gate` → `parser.error('not-a-gate (measurement recipes are never executed): …')`; otherwise not-a-gate rows are dropped; an empty remainder → `parser.error('selection contains no gates')`. `--cells` is de-duplicated preserving order (`dict.fromkeys`) and must be ⊆ `{bdb,kc,tkrzw}` and non-empty. `--oracle/--data/--ibus-build` take `CELL=VALUE` (cell must be in CELLS, value non-empty); `--runner-timeout` takes `NAME=SECONDS` (name must exist; value positive int). `--allow-skipped` accepts `*` or `NAME` or `NAME:CELL` (name must exist, cell must be valid). `--cell-env CELL:NAME=VALUE` requires `NAME` to match `[A-Za-z_][A-Za-z0-9_]*` and not be in the protected set `{CARGO_TARGET_DIR, PINYIN_ORACLE_DBM, PINYIN_ORACLE_PREFIX, OXPINYIN_CAPI_SO, OXPINYIN_ZHUYIN_SO, OXPINYIN_DICTOOL, OXPINYIN_SYSTEM_DIR, CARGO_PROFILE_DEV_OPT_LEVEL}`. If more than one cell and `CARGO_TARGET_DIR` is set in the environment → `parser.error('CARGO_TARGET_DIR is a single-cell override; use --target-root for multiple cells')`.

**Cell config.** For each cell: `cfg = cell-artifacts.sh --describe` run with `PINYIN_ORACLE_DBM=<cell>` and `OXPINYIN_TARGET_ROOT=<target-root>` (JSON of `PINYIN_ORACLE_DBM, ORACLE_DBM, CAPI_FEATURE, OXPINYIN_CAPI_BACKEND_EXT, EXPECTED_PIN_REF, CARGO_TARGET_DIR, CARGO_PROFILE_DEV_OPT_LEVEL`); non-zero → `ValueError` → `FAIL:` exit 1. `prefix` = `--oracle CELL` > `$PINYIN_ORACLE_PREFIX` (single-cell only) > `<oracle-root>/<cell>`, resolved. `data` = `--data CELL` > `$OXPINYIN_SYSTEM_DIR` (single-cell only) > `<prefix>/lib/libpinyin/data`, resolved. Cell env = `os.environ` + cfg + `PINYIN_ORACLE_PREFIX`, `OXPINYIN_SYSTEM_DIR`, `OXPINYIN_CAPI_SO=<CARGO_TARGET_DIR>/debug/libpinyin_capi.so`, `OXPINYIN_ZHUYIN_SO=…/debug/libzhuyin_capi.so`, `OXPINYIN_DICTOOL=…/debug/oxpinyin-dictool` + `--cell-env` extras + `OXPINYIN_USER_RT_TEST=…/debug/user-dir-round-trip-test` + `OXPINYIN_BUILD_USER_RT=1` iff a selected row is named `user-dir-round-trip` else `0` + `PINYIN_IBUS_PREFIX=<prefix>` + `PINYIN_IBUS_BUILD_DIR=<--ibus-build CELL>` (or popped when >1 cell and not given). Substitutions: `root, prefix, data, cell, capi, zhuyin`.

**Oracle / manifest / patch checks (per row, before build).** `variant = row.get('oracle_variant','unpatched')`; `oracle = cell-artifacts.sh --describe-oracle <prefix> <variant>` (cached per variant; non-zero → ValueError). `runner_prefix = oracle['PINYIN_ORACLE_PREFIX']`; `pin_ref` column = `EXPECTED_ORACLE_PIN_REF` with `pin_ref=` stripped. Decision table, first match wins:

| Condition | status | reason |
|---|---|---|
| `cell not in row['cells']` | SKIPPED | `unsupported cell` |
| `row.get('oracle', True)` and (`<runner_prefix>/oracle-pin.txt` not a file or `<runner_prefix>/lib/libpinyin.so` not a file) | SKIPPED if variant==`unpatched`, else FAIL | `missing oracle: <runner_prefix>` |
| oracle and `EXPECTED_ORACLE_PIN_REF` (full `pin_ref=…` line) not among manifest lines | FAIL | `oracle pin/cell mismatch: <manifest>` |
| oracle and `ORACLE_PATCH_FILE` non-empty and (`oracle-patches.sha256` missing or patch filename not a substring of it) | FAIL | `missing required oracle patch: <patches path>` |
| otherwise | → build candidate | |
Rows with a reason are appended to results immediately with `seconds=0.0`.

**Build.** If any candidate and not `--no-build`: `execute(['bash', cell-artifacts.sh], env, ROOT, --build-timeout (900), scratch/build-<cell>.log)`; if rc≠0 or expired → every candidate becomes `FAIL` with `reason='cell build: ' + reason_for(...)` and `seconds` = build seconds; the cell is skipped. Build runs once per cell, serially, before jobs are queued.

**Per job (ThreadPoolExecutor, `max_workers=--jobs` default 2, `pool.map` → results in job order).** `timeout = --runner-timeout[name] or --timeout or row['timeout']`. `work = scratch/<name>-<cell>` (mkdir); substitutions += `work`; env += `row['env']` formatted; `env['TMPDIR']=work`; then `substitutions['env']=env`. Then in order: (1) `required_env` any empty → SKIPPED `missing required environment: a, b`; (2) `command` formatted; `input_paths` formatted, missing → FAIL if variant≠unpatched else SKIPPED `missing required input: p1, p2`; (3) acquire `flock(LOCK_EX)` on each `exclusive_resources` name (sorted) under `target/runner-locks/` (files opened `'a'`), run `execute(command, env, cwd=ROOT, timeout, work/runner.log)`, release.

**SKIP: promotion rule.** `required_skips` = output lines containing `SKIP:` that contain none of `row['optional_skips']` substrings. If `rc == 0 and required_skips and not expired` → `rc = 77` and `output = '\n'.join(required_skips)`.

**Status.** `FAIL` if expired; else `PASS` if rc==0; else `SKIPPED` if rc==77; else `FAIL`. `seconds=round(seconds,3)`. `reason = reason_for(rc, output, timeout, expired)`: expired → `timeout after <timeout>s (exit <rc>)`; rc==0 → `completed`; else the last non-blank line matching `FAIL|DIVERGEN|SKIP|missing input|fatal:|Assertion .* failed|error:`, else the last non-blank line, else `exit <rc>`; truncated to 400 chars.

**Process-group timeout.** `execute`: `Popen(..., stdout=log, stderr=STDOUT, start_new_session=True)`; `OSError` on spawn → `(1, elapsed, str(exc), False)`; `wait(timeout)`; on `TimeoutExpired`: `os.killpg(pid, SIGTERM)`, `wait(5)` (ignore second timeout), `os.killpg(pid, SIGKILL)`, `wait()`; returns `(returncode, elapsed, log text decoded errors='replace', expired)`.

**Allow-skipped.** `allowed(row)` iff any of `'*'`, `runner`, `runner:cell` is in `--allow-skipped`. Exit code = `int(any(status=='FAIL' or (status=='SKIPPED' and not allowed)))`. `*` never allows a FAIL.

**Output.** Results sorted by `(index of runner in filtered gate list, index of cell in --cells)`. Header `runner | cell | status | seconds | pin_ref | reason`, separator `--- | --- | --- | ---: | --- | ---`, cells `str(v).replace('|','/').replace('\n',' ')`. `--json` writes `json.dumps(results, indent=2, ensure_ascii=False)+'\n'` where each row is `{runner, cell, status, seconds, pin_ref, reason}`.

### BIS-P-03 — tools/bisection/run-all.sh

| Field | Value |
|---|---|
| Entry point | `tools/bisection/run-all.sh [args…]` → `exec python3 "$(dirname)/run-all.py" "$@"`. |
| Owning subsystem | bisection |
| Purpose | Shell shim so the aggregate can be invoked like the other `run-*.sh`. |
| Implementation | bash, 4 lines, `set -euo pipefail`. |
| Inputs | argv pass-through. |
| Outputs | those of run-all.py. |
| Platform | Linux (inherits). |
| Consumers | `docs/runbooks/parity-gates.md`, `docs/findings/tooling-lane-gates.md`. |
| State | none. |
| Overlap | pure wrapper. |
| Constraints | none. |

### BIS-P-04 — tools/bisection/oracle-cell.sh

| Field | Value |
|---|---|
| Entry point | **Sourced** (`REPO_ROOT` must be set first); `exit`s the caller on bad input. |
| Owning subsystem | bisection / oracle provisioning |
| Purpose | Maps `PINYIN_ORACLE_DBM` to the cell's cargo feature, table extension and expected `pin_ref`, fixes the per-cell `CARGO_TARGET_DIR`, and provides `oracle_cell_artifact` (build-or-resolve one subject artifact) and `oracle_cell_resolve_prefix` (variant prefix + patch digest). |
| Implementation | bash, 132 lines; `sed` over `tools/oracle/build-oracle.sh`; `cargo build` (in `oracle_cell_artifact`); `sha256sum`/`find`/`sort -z` for the patch digest. |
| Inputs | env: `PINYIN_ORACLE_DBM` (default `bdb`), `REPO_ROOT`, `CARGO_TARGET_DIR`, `OXPINYIN_TARGET_ROOT`, `OXPINYIN_CAPI_SO`, `OXPINYIN_ZHUYIN_SO` (relative → repo-relative), plus the variable named in `oracle_cell_artifact`; files: `tools/oracle/build-oracle.sh` (lines `LIBPINYIN_VERSION=`, `LIBPINYIN_SHA=`, `MODEL_SHA256=`), `tools/bisection/patches/<variant>/*.patch`. |
| Outputs | exports `ORACLE_DBM`, `ORACLE_DBM_NAME` (BerkeleyDB/KyotoCabinet/Tkrzw), `CAPI_FEATURE` (bdb/kyotocabinet/tkrzw), `OXPINYIN_CAPI_BACKEND_EXT` (db/kct/tkt), `EXPECTED_PIN_REF=pin_ref=libpinyin-<ver>-<sha>+model20-<model>+dbm-<cell>`, `CARGO_TARGET_DIR`, `CARGO_PROFILE_DEV_OPT_LEVEL=1`; after `oracle_cell_resolve_prefix`: `PINYIN_ORACLE_PREFIX`, `EXPECTED_ORACLE_PIN_REF` (`+patches-<sha256>` appended for a variant), `ORACLE_PATCH_FILE`. Exit 3 on unknown DBM, exit 1 on unreadable pin constants; functions return 1 with `FAIL: missing … artifact` / `FAIL: unknown oracle variant`. |
| Platform | portable bash (cargo required for the build path). |
| Consumers | sourced by `cell-artifacts.sh`, `tools/abi/check-exports.sh`, `tools/abi/check-alloc-pairing.sh`, and 34 of the 48 `run-*.sh`; the 14 that do NOT source it: run-all.sh, run-addon-candidate-diff.sh, run-callgrind-differential.sh, run-contract-diff.sh, run-perf-same-data.sh, run-system-import-profile.sh, run-punct-diff.sh, run-shipped-check.sh, run-perf-matrix.sh, run-perf-baseline.sh, run-tree-recipe-control.sh, run-rss-diag.sh, run-user-candidate-diff.sh, run-predict-diff.sh. |
| State | `CARGO_TARGET_DIR` default `<OXPINYIN_TARGET_ROOT|REPO_ROOT/target/cells>/<cell>` (retained). |
| Overlap | Pin constants are read here from build-oracle.sh, whereas `contract-diff.py regenerate_sentence_ties` and `gcov_measure.verify_prefix` compute the same `pin_ref` from `tools/oracle/oracle-pin.txt` fields (`libpinyin_tag`, `libpinyin_commit`/PIN, `model_archive_sha256`); `contract-diff.py main` and `train-index-diff.py` additionally hard-code the commit string. Backend name/extension triple also hard-coded in `system-dir.sh`, `gcov_measure.CELLS`, `contract-diff.py`. |
| Constraints | Header: "The pin constants are read from tools/oracle/build-oracle.sh, the one place they live". Comment: "a runner's oracle patch must never change the cell's Cargo target or another runner's oracle". Only variant `bigram-export-strjoinv` → `pin-bigram-export-strjoinv.patch`. Reference cell ruling 2026-09-27 (default bdb). |

### BIS-P-05 — tools/bisection/system-dir.sh

| Field | Value |
|---|---|
| Entry point | **Sourced**; provides `resolve_system_dir VAR LABEL` (echoes dir), `system_dir_capi_ext`, `system_dir_detect_ext DIR`, `system_dir_data_ext DIR`, `system_dir_require_backend DIR EXT LABEL`, `system_dir_require_complete DIR VAR LABEL`, `system_dir_copy_tables SRC DST`, `system_dir_ext_name/feature`. |
| Owning subsystem | bisection |
| Purpose | Resolves and validates the subject's full system data directory for one backend, refusing another backend's directory or the mini fixture unless explicitly allowed. |
| Implementation | bash, 459 lines; `sed`, `cargo tree --offline` (fallback backend detection), `cp`/`find`. |
| Inputs | env: `OXPINYIN_CAPI_BACKEND_EXT`, `OXPINYIN_SYSTEM_DIR`, the caller's `<RUNNER_VAR>`, `OXPINYIN_ALLOW_MINI_FIXTURE`, `REPO_ROOT`; files: `<dir>/datagen-manifest.txt` (`backend=` line), `<dir>/table.conf` (`database format:` token), `gb_char.bin`, `pinyin_index.{db,bin,kct,tkt}`, `phrase_index.*`, `bigram.*`; candidate dirs `<repo>/target/datagen/<ext>`, `/tmp/oxpinyin-export`, `<repo>/fixtures/w3/<ext>`. |
| Outputs | stdout: the directory (or the extension / `bin`); stderr `fatal:` diagnostics; **exit 3** on unresolvable/incomplete/mismatched directory, invalid `OXPINYIN_CAPI_BACKEND_EXT`; loud banner when the mini fixture is used. |
| Platform | portable bash. |
| Consumers | sourced by 19 `run-*.sh` (abi-probe, addon-candidate, dict-surface, dynamic-adjust, import, key-surface, live-typing, nbest-train, option-sweep, phrase-surface, pred-order, residue-a-tail, residue-mechanism, scheme, train-diff-dynamic-off, train-diff, uncovered-surface, union, w11); cited by docs/testing/oracle-environment.md and four findings. |
| State | none written. |
| Overlap | Hard-codes `SYSTEM_DIR_BACKEND_EXTS="db kct tkt"`, `SYSTEM_DIR_CORE_STEMS`, `SYSTEM_DIR_NATIVE_CORE`; the same ext↔feature↔name mapping lives in oracle-cell.sh and gcov_measure.CELLS. Parses `table.conf` like contract-diff.py `_write_user_conf`/`d18_bigram_db` do. |
| Constraints | "THE RULE" block: never fall back to another backend's directory; mini fixture only with `OXPINYIN_ALLOW_MINI_FIXTURE=1`; the oracle-absent SKIP path is deliberately untouched. `system_dir_require_complete` precedence comment: "FIRST match wins, hence the break". `#552 follow-up` ruling: no scan over backends in precedence order. |

### BIS-P-06 — tools/bisection/cell-artifacts.sh

| Field | Value |
|---|---|
| Entry point | `bash cell-artifacts.sh` (build) \| `--describe` \| `--describe-oracle BASE_PREFIX VARIANT`; any other argv → usage, exit 2. |
| Owning subsystem | bisection |
| Purpose | The one shared build stage for a cell (capi, zhuyin-capi, dictool, optional user_dir_round_trip test binary) and a JSON describer of the cell/oracle env for run-all.py. |
| Implementation | bash + embedded python3 (stdlib json/pathlib/os), 56 lines; cargo. |
| Inputs | sources oracle-cell.sh (so `PINYIN_ORACLE_DBM`, `OXPINYIN_TARGET_ROOT`, `CARGO_TARGET_DIR`…); `OXPINYIN_BUILD_USER_RT=1` triggers `cargo test --locked --profile dev --no-run --message-format=json -p oxpinyin-runtime --no-default-features --features $CAPI_FEATURE --test user_dir_round_trip`. |
| Outputs | `--describe*`: one JSON object of the env keys present (exit 0); build: cargo output; `<CARGO_TARGET_DIR>/debug/user-dir-round-trip-test` symlink (renamed atomically from `.<name>.<pid>`); `FAIL: expected exactly one user_dir_round_trip executable` exit 1. |
| Platform | portable (cargo). |
| Consumers | `run-all.py` (`cell_config`, `oracle_config`, build). |
| State | writes into `CARGO_TARGET_DIR`; `mktemp` manifest removed by trap. |
| Overlap | `cargo build --locked --manifest-path … -p oxpinyin-capi -p oxpinyin-zhuyin-capi -p oxpinyin-dictool --no-default-features --features $CAPI_FEATURE` duplicates `oracle_cell_artifact` (single-package) in oracle-cell.sh. |
| Constraints | "executions consume these paths without invoking Cargo"; concurrent-build-safe symlink rename comment. |

### BIS-P-07 — tools/bisection/contract-diff.py

| Field | Value |
|---|---|
| Entry point | `python3 contract-diff.py CELL PREFIX PINYIN_SO [--zhuyin-so P] [--cases a,b] [--expect-parent] [--observations P] [--regenerate-sentence-ties FIXTURE] [--axis-c-replacement options\|encoding\|layouts\|all \| --axis-c-mutation C-m1-r\|C-m3-r \| --axis-c-snapshot-check] [--axis-c-mutant-pinyin-so P] [--axis-c-mutant-zhuyin-so P]`; internal re-exec `python3 contract-diff.py --worker MODE SO DATA CASE`. Also imported as a module by `gcov_measure.contract_module` (via `spec_from_file_location`). |
| Owning subsystem | bisection (lane C contract differential; also the measurement substrate of the Axis J ledgers and the Axis C battery). |
| Purpose | Drives one libpinyin/libzhuyin C-ABI facade through 3449 registered cases in isolated worker processes on both the pin and the subject and requires byte-identical JSON observations (or the registered class (b)/(c) shape). |
| Implementation | Python 3 stdlib (ctypes, json, subprocess, tempfile, resource, signal, hashlib, selectors, copy, re, shutil); 9609 lines; loads `libglib-2.0.so.0`, the two facades, and per cell `libkyotocabinet.so.16`, `libtkrzw.so.1`, `find_library('db')` (private `__db_*_pp` symbols); spawns `db5.3_dump`; reads `tools/bisection/axis-c-replacement.json` and `docs/findings/sentence-surface.md` at import/CLI time. |
| Inputs | positional cell/prefix/so; files `<prefix>/oracle-pin.txt`, `lib/libpinyin.so`, `lib/libzhuyin.so`, `lib/libpinyin/data/{bigram.db,table.conf,gb_char.bin,pinyin_index.bin,…}`, `fixtures/w4/oracle-sentence-tie-order.txt` (regenerate), `fixtures/w4/oracle-sentence-surface.txt` + `tests/parity/corpus/inputs/*.txt` (axis-c source hashes); env: `CONTRACT_DIFF_CELL` (read at import → `CELL`; set by `main`), `TMPDIR` (worker scratch), `CONTRACT_DIFF_OBSERVATIONS`, `AXIS_C_ALLOCATE_EACH`, `AXIS_C_COMPANION_PINYIN`, `AXIS_C_REOPEN_USER`. |
| Outputs | one JSON line per case (`{cell, case, verdict, expected_ok, exit, stderr_lines}`), failure detail lines, optional observations JSONL; exit 0/1 (no 77; cell-mismatch cases print `SKIP` and do not fail). Axis-C: JSON lines + summary + exit 0/1 (mutation mode asserts). Regenerate: rewrites the fixture, prints counts. |
| Platform | Linux-only (ctypes to GLib/DBM libs, `os.fork`, `prctl`, `RLIMIT_FSIZE`, `/dev/full`, `db5.3_dump`, `CLOCK_BOOTTIME`). |
| Consumers | `run-contract-diff.sh` (→ runners.json `contract-diff`), `gcov_measure.py` (`contract_module`, `run_worker`, `Kit`, `matches`, `observed`, `candidate_rows`, `final_step_sentences`), `fixtures/w4/oracle-sentence-tie-order.txt` header, docs (abi-allocator-pairing, bug-for-bug-audit-r2, compatibility-policy, sentence-surface, separator-matrix, tooling-lane-gates, upstream-divergences, runbooks/oracle), `crates/oxpinyin-capi/libpinyin.alloc` and `e2e_tests.rs` comments. |
| State | `TemporaryDirectory(prefix='contract-diff-')` scratch, per-worker `TMPDIR`, per-Kit `mkdtemp(prefix='user-')`, `sys-*`/`cwd-*`/`tmp-*`/`wrap-*` scratch dirs; user dirs removed by `user_dir_listing`; workers `os._exit(0)`; `RLIMIT_CORE=0`; `PR_SET_DUMPABLE=0` for axis-c workers; no per-case timeout in the standalone driver (`run_worker(timeout=None)`), 60 s only when driven by gcov_measure. |
| Overlap | `compare-nbest-training-state.py`/`train-index-diff.py` re-implement ctypes facade loading, MemoryChunk/logger decoding and per-backend bigram dumps; `transformed-options-sweep.py` (not in this assignment) is another ctypes driver; `gcov_measure.surface_worker` reuses `Kit`. |
| Constraints | Frozen inputs: `docs/findings/sentence-surface.md` §12 six residuals (asserted count 6), `axis-c-replacement.json` `format=axis-c-replacement-v1` with source SHA-256s, tie fixture `# format=sentence-tie-order-v1` with 9 tab columns; manifest must contain `libpinyin-2.11.92-074a2219c90feaf962d0d24f034514033ece5f99` and `+dbm-<cell>\n`; regeneration is bdb-only and must use the pin `.so`; `--cases ,` is an error; "ordinary cases cannot carry subject expectations"; suspend-overlap guard (`CLOCK_BOOTTIME − CLOCK_MONOTONIC` drift < 0.01 s) on regenerate, snapshot check and axis-C runs. |

#### contract-diff.py anatomy

**(1) Library loading and ownership.** `class Kit(mode, so, data, scratch)`: `self.lib = C.CDLL(so, use_errno=True)`; `self.glib = C.CDLL('libglib-2.0.so.0')`; installs one `g_log_set_handler` callback (`CFUNCTYPE(None, c_char_p, c_uint, c_char_p, c_void_p)`) for domains `b'libpinyin'`, `b'libzhuyin'` and `None` with mask `0xFFFFFFFC`, recording `[domain, level]` in `kit.logs` and the message text in `kit.log_messages`. `Kit.fn(name, restype, *argtypes)` resolves `getattr(lib, mode + '_' + name)` and (re)assigns `restype/argtypes` on every call — so one `Kit` serves both facades by `mode ∈ {pinyin, zhuyin}`. Lazy `ctx` (`<mode>_init(data, user)`), `inst` (`alloc_instance`), `alloc()`. Ownership: `Kit.text(ptr)` = `string_at` + `g_free` (every caller-owned `gchar*` out-param: sentences, aux text, token phrase, export rows, rendered keys); borrowed strings (`get_candidate_string`) are read with `C.string_at` and never freed (`borrowed_text`); `gchar**` from `in_chewing_keyboard` freed with `g_strfreev`; `GArray`s are allocated by the driver (`g_array_new(0,0,4|2)`) and freed only in `phrase_table_lookup`/`axis_c_lookup` (`g_array_free(arr, TRUE)`), elsewhere leaked; `free_instance`/`fini` are called only where a case needs a reopen; workers end with `os._exit(0)` ("no atexit: a case never shares teardown with the next"). Out-params are seeded with `UNTOUCHED = 0xABCDEF` (`c_ushort` uses `& 0xFFFF`; `c_ubyte` sentinels `0xAB`/`0xff`; `0xBEEF`, `0xABCD`, `0xDEADBEEF`) and reported as the literal `'untouched'` when unchanged. DBM fixtures: `_craft_bdb` declares a 40-byte `DBT` struct and calls `db_create`, `__db_open_pp`, `__db_put_pp`, `__db_get_pp`, `__db_close_pp` (comment: `app_data` pointer needed "omitting it misaligns flags"); `_craft_kc` uses `kcdbnew/kcdbopen/kcdbset/kcdbdumpsnap/kcdbclose/kcdbdel` (`-` in-memory → `dump_snapshot`, or `path#type=kct` attached TreeDB with `keep=True`); `_craft_tkrzw` uses `tkrzw_dbm_open(path, writable, "dbm=TreeDBM|HashDBM")`/`tkrzw_dbm_set`/`close`. Reading back: `index_file_rows` (kc: `kcdbloadsnap` + cursor; tkrzw: iterator; bdb: `db5.3_dump` text split on `HEADER=END`/`DATA=END`), `d18_bigram_db` (chooses backend by **`table.conf` text**, not `CELL`), `_craft_bdb(read_keys=…)`.

**(2) Case model.** Registration is `case(name, mode='pinyin', control=False, abort=NO_ABORT, stderr=False, crash=None, userdir=False, cells=('bdb','kc','tkrzw'), either=False)` returning a decorator that stores `CASES[name] = dict(fn, mode, control, abort, stderr=stderr or crash is not None, crash, userdir, cells, either)`; `assert not either or (abort given and crash given)`. Cases are defined four ways: `@case(...)` on `def _(k)`; `case(...)(fn)` on shared probe functions; module-level `for` loops synthesising names from tables (`TOKEN_REUSE_SEQUENCES`, `ADDON_IMPORT_SEQUENCES`, `INDEX_KEY_SEQUENCES`, `REMOVE_SEQUENCES`, `TAIL_SEQUENCES`, `SEPARATOR_INPUTS × operations × offsets`, `BEFORE_CURSOR_INPUTS`, `MATRIX_RESPLIT_INPUTS`, `FINAL_STEP_INPUTS`, `_RELOAD_PINYIN/_ZHUYIN`, `_RELOAD_USER`, `LIBRARY_BREAKS × sites`, `CONF_*` via `conf_case/conf_abort/stderr_conf_case`, d18/row58 generators); and the data-driven Axis C groups from `axis-c-replacement.json` (`AXIS_C_GROUPS`). Each case function takes a `Kit` and returns a JSON-serialisable dict. Total registered: **3449** (pinyin 2276, zhuyin 1173); `control=True` 1538; `abort≠NO_ABORT` 291 (288 `abort=False`, 3 `abort=0`); `crash≠None` 85; `stderr` 1061; `userdir` 37; `either` 18; `cells` narrowed on 14 (13 `('bdb',)`, 1 `('kc','tkrzw')` = `issue785-pinyin-tails-kept-entry-zhuyin`). A default run (no `--cases`) executes the **2561** non-`axis-c-*` cases sorted by name; the 888 `axis-c-*` cases (options 475, encoding 322, layouts 90, plus `axis-c-export-user` used only as a companion worker) run only under `--axis-c-*`.

Families (name prefix → count, in file order): lane-C #532 option word (`default-option-*`, `explicit-*`) 4; `bytes-*` 7; #542 out-param rows (`empty-parse-sentence`, `character-offset-out`, `token-get-phrase-out`, `nth-pronunciation-range`, `static-key-slots`, `key-rest-length`, `sentence-before-guess*`, `guess-sentence-keyless`, `guess-candidates-keyless`, `unload-phrase-library-repeat`, `sentence-with-prefix-bytes`) 12; `aux-text-*` 11; `[zhuyin-]token-unigram-*` 8 + `[zhuyin-]{system,user}-zhuyin-only-readings` 4; `train-unigram-total`, `predicted-unigram-overflow` 2; `choose-predicted-types` 1; `remember-*` 12; `input-length-cap`, `long-input-sentence` 2; `alloc-instance-after-fini` (+zhuyin twin) 2; `facade-total-*` 4; pinyin `abort-*` 102; `abort-zhuyin-*` 32; `constraint-*` 10; `issue762-*` 31; `issue784-*` 26; `issue785-*` 32; `cursor-and-sentence-neighbours`, `candidate-type-neighbours`, `toned-initial-neighbours`, `scheme-and-library-neighbours`, `zhuyin-cursor-neighbours`, `zhuyin-scheme-and-library-neighbours` 6 + `zhuyin-total-zero-ranks-nothing-*` 2; `guess-candidates-lookup-offset[-zhuyin]`, `guess-candidates-past-the-reserved-slot` 3; `non-utf8-directory-*` 4; `user-library-token-unload-*` 2; zhuyin twins of batch-1 (`zhuyin-sentence-before-guess` … `zhuyin-unload-phrase-library-effect`) 9; `stderr-*` 15 + `stderr-library-*` 50 + `library-directory-*` 17 (+9 `abort-library-directory-*`, already inside the `abort-*` count); `init-system-*` 10; `null-user-session-*` 2; `user-library-{1..7}-{ordinary,null}-*` 28; `issue759-*` 2; `issue790-*` 12; `combined-library-*` 8; `nbest-user-table-order-*` 2; `j-*` 5; `separator/*` 504; `separator-free-control/*` 555; `before-cursor/*` 177; `zero-key/*` 108; `allocator-zero/*` 16; `alloc-register-*` 14; `row54-*` 16; `parse-constraints-*` 80; `zero-consumption-*` 24; `row58-*` 43; `matrix-resplit-*` 96; `table-conf-*` 104 + `table-conf-abort-*` 37 + `user-conf-abort-*` 1; `final-step-*` 50; `mask-out-*` 31; `phrase-table-*` 7; `d18-*` 56; `removal-bigram-*` 23; `user-gram-*` 22; `itemless-*` 4; `import-*`, `stored-*`, `system-add-*`, `log-headers-*`, `token-add-*`, `longer-*`, `unload-drops-*`, `system-gram-*` 17; `reload-*` 30; `wrap-256th-*` 4; libzhuyin per-entry-point (`zhuyin-parse-chewing-key`, `zhuyin-in-chewing-keyboard-*` 9, `zhuyin-key-rest-positions-out`, `zhuyin-phrase-result-tokens`, `zhuyin-string-getters`, `zhuyin-cursor-offset-range`, `zhuyin-null-instance-defined-getters`, `zhuyin-token-get-phrase-optional-out`, `zhuyin-add-phrase-library-16`, `zhuyin-null-*` 19) 36; `axis-c-*` 888.

How a case states its expectation (`matches(spec, pin, subject)`, shared with gcov_measure): (a) default — both sides `exit == 0` with a result, and `observed(pin) == observed(subject)` where `observed` drops keys starting with `~`; `stderr=True` adds `pin['stderr'] == subject['stderr']`; (b) `abort=<v>` (class (c)) — `pin['exit'] == -6` (SIGABRT), `subject['exit'] == 0`, `result['ret'] == v`, `result['logs'] == [[WARNING_DOMAIN[mode], 16]]`, every key starting `untouched` truthy, and `userdir=True` additionally `pin['userfiles'] == subject['userfiles']`; (c) `crash=<dict>` (class (b)) — `pin['exit'] == -SIGSEGV`, `subject['exit'] == 0`, equal normalised stderr, and every `(k,v)` of the dict equals `subject['result'][k]`; (d) `either=True` — abort matcher if `pin['exit'] == -6`, else crash matcher; (e) `control=True` — only `--expect-parent` semantics: control must match, non-control must differ (`expected = same if not expect_parent or control else not same`); 1023 non-control non-axis cases must DIFFER against a parent build; (f) `cells` — `SKIP` verdict with `expected_ok=True`. Hand-written expectations live in: `library_expected(site, how, mode)` (crash dicts for `stderr-library-*`/`library-directory-*`), `_RELOAD_DANGLING` (`{'tokens': ['0x2003cfd'], 'unigram': [False,0], 'readings': [False,0], 'candidates': [88, [['大大',2],['鞑靼',2],['达达',2]], {}]}`), `_ZY_NULL_FALSE/_ZY_NULL_TRUE`, `candidates_probe` crash `{'ret': False, 'logs': [[domain,16]]}`, `zhuyin-add-phrase-library-16` `crash={'ret': False}`, abort values `False` (bool returns) / `0` (`choose_candidate` int returns: `abort-choose-candidate-predicted-prefix-row`, `abort-choose-candidate-without-sentence-offset-1`, `abort-choose-candidate-predicted-bigram-row`). Embedded accepted-divergence registers: `~`-prefixed keys (`'~content nth N'` in `nth-pronunciation-range`/`zhuyin-…`/`_wrap_item_reads`, `'~instance after fini'`, `'~<label>'` loose dicts) — comments say "ruled answer … class (b)"; `SEPARATOR_ABORTS` (34 `mode/index/op → offsets` entries, class (c)) and `SEPARATOR_CONTROLS` (84 entries); `BEFORE_CURSOR_CONTROLS` (69 pairs); `TAIL_CONTROLS`; `CONF_CONTROLS` (17 names) and `CONF_ZHUYIN_CHANGED` (3); `AXIS_C_LAYOUT_COVERED` built from `axis-c-replacement.json` `layout_covered` rows `5c` (scheme 7) and `5d` (schemes 0,10,11,-1,INT_MIN,INT_MAX); `declared_sentence_residuals()` parsing `docs/findings/sentence-surface.md` §12 (exactly six names). Register/policy rows cited in comments: 1, 5c, 5d, 11, 36, 41, 43, 45, 54, 58, 62, 65, 66, 68, 76, 82, 84, 85, 88, 96, 102, and `#542 rows 1,2,3,6,7,8,9,10,14,19,20,21`.

**(3) Observation rules.** Per call a case records whatever it chooses: return values (`c_bool`→`true/false`, `c_int`, `c_size_t`), out-params (value or `'untouched'`, pointers as `'null'/'key'/'set'`), strings (`k.text` utf-8 `'replace'`), key bytes as hex (`C.string_at(ptr, 2|4).hex()`), tokens as hex or ints, candidate lists as `[type, text]` rows (`candidate_rows`, `guess_rows`, `predicted_rows`, `candidate_heads`), sentences via `sentence_out` (`[ret, text|'untouched']`), export rows `[ret, phrase, reading, count]`, DBM record dumps, and user-file snapshots: `token_reuse_files` (index containers record-by-record via `index_file_rows`, `user_pinyin_index.bin` values with `zero_item_padding` applied to odd key lengths; `user_bigram.db` on non-bdb → only `len(raw) > 0`; files whose bytes 12..16 are the BDB magic `62 31 05 00`/`61 15 06 00` → `db5.3_dump` non-empty rows; `user.bin/addon.bin/network.bin` hex with header bytes 4..8 zeroed (timestamp); everything else raw hex; `.tmp` skipped), `transient_files` (`{relpath: (st_mode, bytes)}`), `user_listing` (names latin-1, `user.conf` text, chunk sizes, `user_*` classified `chunk|dbm`), `user_dir_listing` (for `userdir=True`: names, `user.conf` text with `|` for newlines, `.bin/.dbin` sizes; user dirs then removed). Always appended by `worker`: `result['logs'] = kit.logs`; axis-c adds `log_messages`. `run_worker` captures: `exit` (`proc.returncode`, negative = signal), `result` (last stdout line starting with `{`, `json.loads`), `userfiles`, `measurement_roots` (`J-ROOT ` lines), `measurement_abort_flush` (both `J-GCOV-ABORT-*` markers present), `stderr_lines`, `stderr` (decoded `surrogateescape`, normalised by `re.sub(r'(user|sys)-(\udcff)?[A-Za-z0-9_]+', r'\1-\2X', …)`). Normalisation elsewhere: `axis_c_normalize_diagnostics` replaces the scratch path with `SCRATCH` in `stderr/reopen_stderr/log_messages` only; JSON `sort_keys=True`. Comparison is exact dict equality; no numeric tolerance anywhere.

**(4) Isolation, process control, timeouts.** `main` creates `TemporaryDirectory(prefix='contract-diff-')`; for each case `run_worker(mode, so, data, name, scratch)` spawns `[sys.executable, __file__, '--worker', mode, so, data, name]` with `env = os.environ + TMPDIR=scratch`, `capture_output=True`, `timeout=None` (standalone) — the pin side first, then the subject. `worker()` sets `RLIMIT_CORE=(0,0)`, for `axis-c-*` calls `prctl(PR_SET_DUMPABLE,0)`, builds one `Kit` whose user dir is `mkdtemp(prefix='user-', dir=TMPDIR)`, runs the case, prints the JSON, `os._exit(0)`. `run_worker` snapshots `os.listdir(scratch)` before and removes new `user-*` dirs after (`user_dir_listing`). Private system dirs: `private_system` makes `sys-*` dirs of symlinks into the oracle data (comment: "the oracle's own files are never touched"); `_replace_system_start_row`/`d18_bigram_fixture` `copytree` the data dir. Sub-forks inside a worker: `final_step_sentences` forks once per sentence index (pipe + stderr→devnull; `-SIGABRT` child breaks the loop), `axis_c_isolated` forks per observation with two pipes drained by `selectors`. Axis C: `ThreadPoolExecutor(max_workers=2)` runs four workers per case (pin, subject, pin-repeat, subject-repeat) each in its own `worker-*` temp dir; `assert stable` ("NOT-ESTABLISHED: unstable case"). Resource-limit cases mutate the worker itself (`RLIMIT_FSIZE=(4,4)` + `SIGXFSZ` ignored; `/dev/full` symlink; `os.chdir`; `os.environ['TMPDIR']` rewrite in `null_user_session`). The only per-case timeout is `WORKER_TIMEOUT=60` applied by `gcov_measure.bounded_worker`.

**(5) Output format and exit codes.** Ordinary mode: per case `json.dumps(dict(cell, case, verdict='MATCH'|'DIFFER'|'SKIP', expected_ok, exit=[pin, subject], stderr_lines=[pin, subject]), sort_keys=True)`; when `not expected or (not same and not expect_parent)` → `failures += 1` and, if `not same`, prints `  pin exit=<e> <observed result>` / `  subject …`, plus `  <side> userfiles=…` (userdir) and `  <side> stderr=…` (stderr). Exit `1 if failures else 0`. `--observations` truncates then appends `{cell, case, pin, subject}` per case. Axis C: `{"case","verdict":"DIFFER","first":…}` / `{"case","verdict":"COVERED","row","covered_observations"}` lines, `axis C progress: n/N cases` every 25, `difference_signature` lines, final `{battery, mutation, counts, repeats:'identical', suspend_delta, hashes:{pin,subject}}`; mutation mode prints `{mutation, control, verdict}` and asserts every control is DIFFER (exit 0) else `AssertionError`; replacement mode exits `1 if case_differ else 0`. Snapshot check prints `instance snapshot: …` lines, exit 0. Regenerate prints `pin verified: library and N data files`, progress every 1000, the summary, `suspend overlap: none …`, exit 0.

**(6) Env vars and CLI.** CLI as in the Entry point row; mutual exclusion group for the three axis-c modes; `--axis-c-mutant-*` require `--axis-c-mutation`; axis-c modes refuse `--observations/--cases/--expect-parent/--regenerate-sentence-ties`; regenerate refuses `--cases/--expect-parent/--zhuyin-so/--observations`, requires `cell == bdb` and `pinyin_so == <prefix>/lib/libpinyin.so`. Env: `CONTRACT_DIFF_CELL` (module-level `CELL` default `bdb`; `main` sets it so re-exec'd workers inherit), `TMPDIR` (required by `worker`), `CONTRACT_DIFF_OBSERVATIONS` (default for `--observations`), `AXIS_C_ALLOCATE_EACH` (per-child allocation vs forked snapshot; popped before the battery), `AXIS_C_COMPANION_PINYIN` (pinyin `.so` used to reopen a zhuyin profile through the pinyin export API), `AXIS_C_REOPEN_USER` (user dir for `axis-c-export-user`). Also honoured when embedded: `J_MEASUREMENT_CELL`, `J_MATCH_INPUTS` (gcov_measure).

**(7) Generic libpinyin-ABI knowledge vs oxpinyin-specific assertions.** Generic (would transfer to any libpinyin build at 074a2219): the entry-point names and ctypes signatures (`pinyin_*`/`zhuyin_*`), option bits (`1<<5` USE_TONE, `1<<3` INCOMPLETE, `1<<7/1<<8` divided/resplit, `1<<9` DYNAMIC_ADJUST, `1<<12,15,17` fuzzy, `1<<27` CORRECT_V_U, words `0x18a/0x1aa/0x28/0x20`), sort words (`0x1`, `0x1c`, `0x1d`, `0x1e`, `0x1f`), candidate types (1 NBEST, 2 NORMAL, 4 PREDICTED_BIGRAM, 5 PREDICTED_PREFIX, 6 ADDON, 7 LONGER, 8 PUNCT), library indexes 1..7 and token layout `lib<<24`, `USER_ALL`/`ADDON_ALL` masks, scheme numbers, the GLib log domains, file formats (MemoryChunk `{len,xor-checksum}` header via `_chunk`, logger records via `_modify_header` (type 4) and MODIFY (type 3), SingleGram `[total, (token,count)*]`, `user.conf` keys, `table.conf` row grammar, UCS-4 phrase-table keys, `ChewingKey` 2-byte packed keys), and the per-backend container kinds (BDB btree/hash, KC snapshot/TreeDB, tkrzw TreeDBM/HashDBM). Oxpinyin-specific: the class (c) contract that the subject returns `<v>` with exactly one `level 16` warning in `libpinyin`/`libzhuyin`; every `crash=` dict (what the subject answers where the pin segfaults); the `~` indeterminate ruling; `FACADE_TOTAL = 51051831` and fixed tokens (`0x01001225` 你, `0x01003e57` 癫痫, `0x02000001`, `0x02000005` 礙, `0x0FFFFFFF` `_PAST_RANGES`) tied to the pinned oracle data; `_RELOAD_DANGLING`'s 88 candidates; `control=` flags encoding which oxpinyin PR introduced each fix (`--expect-parent`); the §12 residual list and tie fixture; `axis-c-replacement.json` hashes; `WARNING_DOMAIN`; and the gcov `J-ROOT`/`J-GCOV-ABORT-*` protocol.

**(8) Cell/backend-specific branches.** `CELL` from `CONTRACT_DIFF_CELL`; `_craft_db(path, rows, dbtype, keep)` dispatches bdb→`_craft_bdb`, kc→`_craft_kc` (asserts `keep` only with btree), tkrzw→`_craft_tkrzw` (`HashDBM` iff `dbtype == 2`); `index_file_rows` and `token_reuse_files` branch on `CELL` (bdb-only bigram row dump; other cells only presence); `d18_bigram_db` branches on `table.conf` text (`BerkeleyDB`/`KyotoCabinet`/`Tkrzw`) and loads `ctypes.util.find_library('kyotocabinet'|'tkrzw')` rather than the versioned sonames used elsewhere; `cells=('bdb',)` on 13 cases (predicted-bigram-row abort, stale-bigram token-reuse ×4, `abort-add-phrase-*` ×4, `issue542-stale-*` ×4) and `('kc','tkrzw')` on `issue785-pinyin-tails-kept-entry-zhuyin`; comments record per-cell pin abort lines (`chewing_large_table2_{bdb,kyotodb,tkrzwdb}.cpp`); `main` asserts `+dbm-<cell>\n` in the manifest; regeneration is bdb-only; `table-conf-database-format-other` flips `BerkeleyDB↔Tkrzw` in a private `table.conf`.

### BIS-P-08 — tools/bisection/run-contract-diff.sh

| Field | Value |
|---|---|
| Entry point | `run-contract-diff.sh <bdb\|kc\|tkrzw> <oracle-prefix> <pinyin-so> [<zhuyin-so>] [-- contract-diff.py options]` → `exec python3 contract-diff.py CELL PREFIX SO [--zhuyin-so Z] …`. |
| Owning subsystem | bisection |
| Purpose | argv adapter from the registry shape to contract-diff.py. |
| Implementation | bash, 27 lines. |
| Inputs | argv; rejects unknown cell (exit 1). |
| Outputs | those of contract-diff.py (exit 0/1). |
| Platform | Linux (inherits). |
| Consumers | runners.json `contract-diff`; docs (abi-allocator-pairing, compatibility-policy, sentence-surface, upstream-divergences, runbooks/oracle). |
| State | none. |
| Overlap | Does **not** source oracle-cell.sh/system-dir.sh (the only `run-*.sh` beside the three tiny `run-{addon-candidate,predict,punct,user-candidate}-diff.sh` shims that bypasses the shared cell helpers); no `EXPECTED_PIN_REF` check — contract-diff.py does its own. |
| Constraints | Header documents the exit contract ("0 only when every selected case matches the pin (or, with --expect-parent, …)"). |

### BIS-P-09 — tools/bisection/cg-calls.py

| Field | Value |
|---|---|
| Entry point | `python3 cg-calls.py <callgrind.out> calls\|callers <name-substring>`; usage error exit 2. |
| Owning subsystem | perf / profile |
| Purpose | Counts callgrind call edges keyed on the callee's `(ob, fl, fn)` identity triple (not the demangled name) to attribute heap/RSS gaps to callers. |
| Implementation | Python stdlib (re, collections); 77 lines. |
| Inputs | callgrind.out text (name compression `ob=/fl=/fi=/fe=/fn=` and `cob/cfl/cfn`, `calls=` lines). |
| Outputs | stdout table (top 14 callees or top 12 callers + TOTAL); exit 0. |
| Platform | portable. |
| Consumers | `docs/runbooks/benches.md` ("Attributing a heap gap to callers"), `docs/findings/rss-attribution-2026-09-09.md`, `bug-for-bug-audit-r2-…-prereg.md`. Not invoked by any script. |
| State | none. |
| Overlap | `run-callgrind-differential.sh` produces the inputs; `rss-smaps.py` is the sibling RSS tool. |
| Constraints | Docstring rule: identity is the triple because `.isra/.constprop` clones and same-named methods in two objects would merge. |

### BIS-P-10 — tools/bisection/clang_identity.py

| Field | Value |
|---|---|
| Entry point | module; `Clang(library=None).extract(source, flags, pin)` → `(nodes, edges, virtual, overrides, fields, metadata)`. |
| Owning subsystem | profile (Axis J coverage ledger) |
| Purpose | Read-only libclang adapter (no Python `clang` package): USRs, display signatures, mangled names, body extents, virtual/override relations, `CXXDeleteExpr` destructor edges, field references, static receivers, for every function kind in a pin translation unit. |
| Implementation | Python ctypes against `libclang` (`find_library('clang')`), hand-declared `Cursor/String/Location/Type/Range` structs and 38 `clang_*` signatures; 237 lines. Cursor kinds are numeric literals (2,4,31 records; 8,21,24,25,26,30 functions; 44 base; 100/102/103 refs; 135 delete; 202 body; 350/70 parents). |
| Inputs | a `.cpp` path, compiler flags, the pin root (paths outside the pin are dropped). |
| Outputs | in-memory dicts/sets; `RuntimeError` on parse failure or severity ≥3 diagnostics. |
| Platform | needs libclang (Linux-first). |
| Consumers | `internal-ledger.py` (`from clang_identity import Clang`), `internal-ledger.test.py`. |
| State | none written. |
| Overlap | pairs with `internal-ledger.llvm()` (LLVM IR side). |
| Constraints | "Small read-only libclang adapter"; numeric kind constants are libclang-version-sensitive. |

### BIS-P-11 — tools/bisection/compare-nbest-training-state.py

| Field | Value |
|---|---|
| Entry point | `compare-nbest-training-state.py CELL PIN_DIR OX_DIR [--pin-so SO --ox-so SO --data DIR] [--facade pinyin\|zhuyin]`; internal `--read-unigrams SO DATA PROFILE TOKEN…`; also imported by `train-index-diff.py` for `bigrams()`. |
| Owning subsystem | bisection |
| Purpose | Compares per-case saved user profiles (pin vs subject): user bigram records byte-for-byte via the backend's own dump tool, `.dbin` logger unigram/pronunciation fields, user chunks, and (optionally) unigram counts re-read through the pinyin export API. |
| Implementation | Python stdlib + ctypes; 273 lines; spawns `db5.3_dump` (bdb), `kchashmgr load/list -pv -px` (kc), `tkrzw_dbm_util export --dbm hash --tsv --escape` (tkrzw); re-execs itself per `.so` to avoid symbol interposition. |
| Inputs | two directories of case sub-dirs (`user_bigram.db`, `*.dbin`, `user.bin`, `addon.bin`, `network.bin`, `driver.err` with `coverage fixture=… nbest-index=N`). |
| Outputs | per case `IDENTICAL`/`DIVERGENT` line with diff lists and pronunciation dumps; writes `*.unigrams`, `bigram.records`, `unigrams.api` beside the inputs; exit 0 identical, 2 divergent, `ValueError` on missing inputs/zero cases/coverage not `{0,1,2}`/case list ≠ `["0-1","0-2","1-1","2-1","2-2","2-3","3-1","3-2"]`. |
| Platform | Linux (dump tools). |
| Consumers | `run-nbest-choose-train-diff.sh`, `train-index-diff.py`. |
| State | writes derived files into the compared profile dirs. |
| Overlap | MemoryChunk/logger decoding duplicates contract-diff.py `_chunk/_modify_header` and `oxpinyin-data/user_files.rs::decode_log_records` (cited); bigram dumping duplicates `contract-diff.py index_file_rows/token_reuse_files` (ctypes instead of CLI tools). Hard-codes `0x010059F5` (今天). |
| Constraints | Docstring: "Missing inputs fail, including zero cases"; token library map `{gb_char:1, gbk_char:2, opengram:3, merged:4}`. |

### BIS-P-12 — tools/bisection/compiler_cells.py

| Field | Value |
|---|---|
| Entry point | module: `function_cells(ledger, state, key)`, `boundary_cells(ledger, state, executed)`, `projections`, `compiler_projection`, `validate(ledger, state, inventory, rows)`. |
| Owning subsystem | profile (Axis J) |
| Purpose | Produces the compiler-owned columns of a ledger row (kind, pin link, signature, identity, witness export, path) and validates a published table against a fresh extraction, deliberately excluding behavioural columns. |
| Implementation | Python stdlib; 78 lines; imports `render_ledger.labels_text` lazily. |
| Inputs | `state` dict from `internal-ledger.main` (`nodes`, `direct_paths`, `pinned_paths`, `paths`, `fields`, `ast`, `sites`, `ambiguous`, `resolved_sites`, `singleton_callbacks`, `external`). |
| Outputs | row lists; `RuntimeError('compiler-derived ledger cells differ…')`. |
| Platform | portable. |
| Consumers | `delta_ledger.py`, `finish_ledger.py`, `render_ledger.py`, `internal-ledger.py --inventory-check`, tests. |
| State | none. |
| Overlap | row-kind vocabulary (`function/direct`, `function/pinned-virtual`, `function/conservative`, `function/baseline-conservative`, `indirect`, `virtual`, `pinned virtual`, `pinned callback`, `external boundary`) must match `delta_ledger`/`render_ledger` parsers. |
| Constraints | "Membership retention never copies a stale witness or asserts deadness"; `UNPROVEN` export for retained baseline identities. |

### BIS-P-13 — tools/bisection/coverage_reviews.py

| Field | Value |
|---|---|
| Entry point | module; registries `REVIEWS`, `IDENTITY_REVIEWS`, `CELL_IDENTITY_REVIEWS{kc,tkrzw}`, `KNOWN_DEFECTS`, `MEASUREMENT_LIMITS`; registration functions `review(file, lines, targets, reason, status='COUNTERPART')`, `identity_review(key, targets, reason, status, cell)`, `known_defect(file, lines, issue, excluded_prefixes, reason)`, `measurement_limit(names, reason)`; `load_rules()` at import, `reload_rules()`. |
| Owning subsystem | profile (Axis J) |
| Purpose | Holds the source-reviewed mappings from pin definition sites / USRs to Rust anchors, plus the known-defect and measurement-limit registers the ledgers consult. |
| Implementation | Python stdlib (importlib, pathlib); 99 lines; path constants `C E R U P D` for six Rust files. |
| Inputs | `coverage_review_rules/*.py` (globbed, sorted). |
| Outputs | in-memory registries; `ValueError` on duplicate keys. |
| Platform | portable. |
| Consumers | `delta_ledger.py`, `finish_ledger.py`, `render_ledger.py`, `coverage_review_rules/*`, tests. |
| State | module globals (identity-preserving on reload). |
| Overlap | `internal-ledger.MAPPINGS` is a parallel USR→anchor register with its own `counterpart()` renderer. |
| Constraints | "Site keys apply one review to compiler instantiations of the same reviewed template body; they do not resolve calls or infer a counterpart from spelling"; reload keeps registry identity so importers that bound them by name see refreshed rules. |

### BIS-P-14 — tools/bisection/coverage_tiers.py

| Field | Value |
|---|---|
| Entry point | module: `lexical_mask`, `brace_end`, `rust_items(path, root)`, `cited_items(root, pin, nodes, tightened)`, `sample_rows(rows, seed)`, `shipping_crates(root)`, `facility(node)`, constants `CITATION`, `CITATION_LIMITS`, `FACILITIES`. |
| Owning subsystem | profile (Axis J) |
| Purpose | Mechanical T1 tier: finds Rust items whose contiguous doc/comment literally cites a pin `file.cpp:lo-hi` range inside a clang body, under the tightened "role" rule; maps a few storage/lifetime pin sites to facility families (T3). |
| Implementation | Python stdlib (re, tomllib, random, math); 269 lines. |
| Inputs | `crates/**/*.rs` (excluding tests/examples/benches, `build.rs`, `*_test(s).rs`, `#[cfg(test)]` mods), workspace `Cargo.toml` dependency closure of `oxpinyin-capi` and `oxpinyin-zhuyin-capi`, pin `src/**.{cpp,h}`. |
| Outputs | dicts; `ValueError('unclosed Rust body')`. |
| Platform | portable (Python ≥ 3.11 for tomllib). |
| Consumers | `render_ledger.py`, `internal-ledger.py` (`--tier-preview`), `delta_ledger.py` (`facility`, `FACILITIES`), tests; cited in `docs/findings/axis-j-bdb-ledger.md`. |
| State | none. |
| Overlap | Its `CITATION` regex is a second parser of `file:line` citations (AGENTS.md says cite pin by file:line). |
| Constraints | `CITATION_LIMITS` pins the reviewed exclusion of `SubPhraseIndex::add_unigram_frequency` ↔ `store.rs add_unigram_frequency`; `FACILITIES` lines (`memory_chunk.h` ≠136, `unaligned_memory.h`, `chewing_large_table2.{h,cpp}` lines 26/68/52/55/74/86) are pin-line-specific; 5 % sample `math.ceil(len*0.05)` with `random.Random(seed)`. |

### BIS-P-15 — tools/bisection/coverage_review_rules/ (package, 12 modules)

| Field | Value |
|---|---|
| Entry point | imported by `coverage_reviews.load_rules()`/`reload_rules()` as `coverage_review_rules.<stem>`; each module calls `review`/`identity_review`/`known_defect`/`measurement_limit` at import. |
| Owning subsystem | profile (Axis J) |
| Purpose | Commit-sized batches of source reviews keyed by pin definition site at 074a2219. |
| Implementation | Python; 12 files, 338 lines total. |
| Inputs | none beyond `coverage_reviews` constants. |
| Outputs | registry entries. |
| Platform | portable. |
| Consumers | `coverage_reviews.py` (glob), `internal-ledger.test.py` (asserts 15 `ABSENT_INLINE` identities, 4 `KNOWN_DEFECTS`, 2 `MEASUREMENT_LIMITS`). |
| State | none. |
| Overlap | `chewing_large_table2_{bdb,kyotodb,tkrzwdb}_cpp.py` are three near-verbatim copies differing only in line numbers and the DBM verbs. |
| Constraints | Duplicate `(file,line)`/key raises; line numbers are frozen to the pin. |

Rule modules and what each asserts:

| Module | Registers | Asserts |
|---|---|---|
| `chewing_large_table2_bdb_cpp.py` | 6 `review` (lines 30; 165,196; 232,251,550; 291,361; 398,435; 471) | BDB suggestion walk ↔ `walk_suggestions`/`prefix_upper_bound`; point search ↔ `search`; longer-record walk ↔ `walk_suggestions`+`checked_extension_tokens`; add ↔ `add_phrase_in`+`pinyin_index_entries`+persistence `with_kept_keys`; remove ↔ `remove_user_phrase`; mask_out ↔ `mask_out` ("structural counterpart, not a proof of identical malformed-file side effects"). |
| `chewing_large_table2_cpp.py` | 4 `review` (26,68 `ABSENT_STD`; 161; 179; 200) | sixteen entry buffers absent (Vec/DBM ownership); keyspace selection; add to both keyspaces; remove readings. |
| `chewing_large_table2_h.py` | 9 `review` (52,55 and 59,66 `ABSENT_INLINE`; 74,86 `ABSENT_STD`; 91; 136; 155,182; 208; 233; 268) | comparator state inlined; template entry ownership replaced by `decode_items`; search/filter/range folding; incomplete vs complete key choice; insertion/removal/mask membership semantics. |
| `chewing_large_table2_kyotodb_cpp.py` | 6 `review` (34; 149,181; 217,238,524; 278,340; 376,411; 514) | KC twin of the bdb batch (check/get/set, jump/step cursor, writable visitor). |
| `chewing_large_table2_tkrzwdb_cpp.py` | 6 `review` (32; 133,164; 200,221,494; 261,315; 351,383; 481) | tkrzw twin (Get/Set, Jump/Next iterator, record processor). |
| `dbm_callback_lifetimes.py` | 6 `review` `ABSENT_INLINE` (kyotodb 447/349/163, tkrzwdb 419/277/143) | implicit destructors of mask-out / phrase-table mask-out / key-collect callback objects release nothing; GCC emits the ABI body but gcov omits artificial records (gcov.cc:1764-1766 at GCC 16.2.0) → no execution credit. |
| `implicit_identities.py` | 4 `identity_review` `ABSENT_INLINE` (context/instance ctor/dtor ×2 facades), 4 operator==/!= identities (COUNTERPART), 7 `ABSENT_INLINE` synthetic lifetimes (SingleGram dtor, PhoneticKeyMatrix ctor/dtor, PhraseItem dtor, PhraseTableEntry dtor, PhoneticParser2/ZhuyinParser2 ctors), 2 `ABSENT_BDB_EXPORTS` (`PinyinDirectParser2::parse_one_key/parse`) | synthetic member bodies map to Rust struct construction/drop; `_Zeq/_Zne` hand-reviewed cross-compiler join; direct parser not constructible through bdb C exports (bulk-text loaders only). |
| `known_defects.py` | 4 `known_defect` (#785; kyotodb 361/459, tkrzwdb 288/429; excluded prefixes `contract/issue784-`, `contract/issue762-`) | kc/tkrzw mask-out tail leak not reproduced by oxpinyin → rows never BEHAVIOR-COVERED while #785 open. |
| `known_measurement_limits.py` | 1 `measurement_limit` (`user-gram-duplicate-export`, `user-gram-unsorted-export`) | pin `g_strjoinv` over-read (pinyin.cpp:862, register row 1) faults depending on heap layout in `-O0 --coverage` builds → never credited. |
| `kyotodb_key_collect_visitor.py` | 1 `identity_review` `ABSENT_KC_EXPORTS` cell=`kc` | ODR collision of two global `KeyCollectVisitor` classes; punct-table ctor unreachable from exports. |
| `pinyin_cpp.py` | 1 `review` (pinyin.cpp:1615) | `pinyin_in_chewing_keyboard` ↔ capi `pinyin_in_chewing_keyboard`/facade `in_keyboard`/`symbols_for`/`owned_cstr_list`. |
| `pinyin_phrase3_h.py` | 8 `review` (33,201,210; 68,73,96,108 `ABSENT_INLINE`; 146; 160; 170; 186 `ABSENT_INLINE`; 191; 217,226 `ABSENT_INLINE`) | compare2 ordering ↔ `exact_compare2`; wildcard predicates ↔ `prefix_keys_match`; incomplete detection; complete/incomplete key encoding; item encode/decode. |

### BIS-P-16 — tools/bisection/delta_ledger.py

| Field | Value |
|---|---|
| Entry point | module: `render_delta(args, state, provenance)` called from `internal-ledger.main` when `--cell kc\|tkrzw --delta-from <bdb ledger>`; helpers `parse_rows`, `conditional_files`, `clone_families`, `match_bdb`, `partition`, `carrier`, `break_point`, `measure_cell`, `classify`, `evidence`, `split_verdicts`, `prefix_provenance`, `emitted_inventory`. |
| Owning subsystem | profile (Axis J) |
| Purpose | Generates `docs/findings/axis-j-{kc,tkrzw}-ledger.md`: measures only the cell's DBM-conditional sources and shared functions whose export reachability differs from bdb; inherits everything else from the bdb ledger by reference. |
| Implementation | Python stdlib (html, re, hashlib, subprocess); 596 lines; imports `compiler_cells`, `coverage_reviews`, `coverage_tiers`, `gcov_measure`, `render_ledger`, `internal-ledger` (via importlib). |
| Inputs | bdb ledger Markdown (`--delta-from`, must contain `Pin \`<PIN>\`` and the title `Berkeley DB internal coverage ledger`, provenance sentence `Oracle shared-object SHA-256: libpinyin=…; libzhuyin=…`), pin `src/Makefile.am`, oracle prefix (`oracle-pin.txt` fields `shared_object_sha256`, optional `zhuyin_shared_object_sha256`), `config.status`, Rust build artifacts, `git -C REPO status/rev-parse HEAD:crates`, toolchains `clang++ opt g++ gcov`. |
| Outputs | writes `--output` ledger (or compares bytes with `--check`), prints `cell=… roots=… delta_rows=…`, mutation-check lines, `pristine ledger sha256=…`; `RuntimeError` on any guard; `--require-complete` rejects open/known-defect rows, DIFFER or limited verdicts. |
| Platform | Linux (toolchains, gcov). |
| Consumers | `internal-ledger.py`; tests. |
| State | measurement build dir (`*.gcda` cleared), temp dirs via gcov_measure. |
| Overlap | Re-implements the row/mutation loop of `render_ledger.render`; `MUTATIONS` duplicates `internal-ledger.mutation_check` predicates (J-m1 `pinyin_init`, J-m2 `pinyin.cpp:172 check_format`); `PROVENANCE` regex duplicates `gcov_measure.recorded_prefix_hashes`. |
| Constraints | Clone families matched one-to-one by unsuffixed identity + translation unit (ambiguity or underivable unit fails); "Rust sources must match the committed crates tree"; registered `MEASUREMENT_LIMITS` are forced to `MEASUREMENT-DIFFER` and never credited; `surfaces(excused=declared_residuals())` fails on any undeclared surface difference. |

### BIS-P-17 — tools/bisection/finish_ledger.py

| Field | Value |
|---|---|
| Entry point | module: `refresh(args, state)` from `internal-ledger.py --refresh-unknowns`; helpers `table_rows`, `manual_sample`, `source_mapping`, `verify_implicit_objects`, constants `MANUAL_SEED`, `MANUAL_POPULATION_SHA`, `MANUAL_SAMPLE_SHA`, `MANUAL_CRATES_TREE`, `UNKNOWN_POPULATION_SHA`. |
| Owning subsystem | profile (Axis J, bdb only) |
| Purpose | Incremental reconciliation of the bdb ledger: keep published MATCH rows, rebuild compiler identities, replay every `abort=` bdb case plus the five `j-*` controls, re-classify the 24 UNKNOWN rows, and rewrite the ledger header. |
| Implementation | Python stdlib; 286 lines; `nm --defined-only` over `*.cpp.o`. |
| Inputs | existing ledger text, `state`, measurement build/config and both subject `.so`s; `git rev-parse HEAD:crates` must equal `MANUAL_CRATES_TREE = ecead320…`; selected cases = bdb cases with `abort≠NO_ABORT` or name `j-*`. |
| Outputs | rewritten ledger (or byte compare with `--check`), prints `ABORT_FLUSH_LIMIT`, `EXECUTED_GCC_ALIGNMENT_LIMIT`, `J refresh roots=…`, manual-sample line; `RuntimeError` guards (population 178 / sample 20 hashes, 24 unknowns, 15 synthetic bodies, any non-MATCH verdict). |
| Platform | Linux. |
| Consumers | `internal-ledger.py`, `render_ledger.py` (imports `manual_sample`, `table_rows`, `verify_implicit_objects`, `source_mapping`, marks), tests. |
| State | none beyond gcov build dir. |
| Overlap | `source_mapping` ≈ `delta_ledger.reviewed`; header rewriting via string replace of frozen sentences. |
| Constraints | Hash-guarded populations; "No capture is loaded"; `--require-complete` rejects remaining UNKNOWN-LIMIT/NOT-EXECUTED-UNREVIEWED rows or dispatch limits. |

### BIS-P-18 — tools/bisection/gcov_measure.py

| Field | Value |
|---|---|
| Entry point | module (imported as `measure`); CLI `python3 gcov_measure.py --pin P --config config.h --oracle PREFIX --build OUT [--cell C]` (builds only); internal `--worker MODE SO DATA CASE` and `--surface-worker SO DATA GROUP CELL`. |
| Owning subsystem | profile (Axis J measurement substrate) |
| Purpose | Builds a private `-g -O0 --coverage` copy of the pin (with a `__gcov_dump` shim and SIGABRT flush handler), re-runs the contract cases / seven same-data drivers / user-store round trip / sentence-surface replays against reference, subject and instrumented library, and attributes gcov function counters to cases that MATCH. |
| Implementation | Python stdlib + ctypes; 840 lines; spawns `g++`, `gcc`, `nm`, `gcov --json-format --stdout`, `pkg-config`, `config.status --config`, `git`; Unix-socket + pickle for the parallel-stage handoff. |
| Inputs | pin checkout (must be at `PIN` and clean), `config.h`/`config.status` of the cell, oracle prefix (`oracle-pin.txt`, `oracle-data.sha256`, `oracle-data-unstable.sha256`, `lib/*.so`), `tools/oracle/oracle-pin.txt`, `docs/findings/sentence-surface.md` §12, `fixtures/w4/oracle-sentence-{surface,tie-order}.txt`, `tests/parity/corpus/inputs/*.txt`, `tools/oracle/user_driver.c`, Rust test binaries `sentence_surface_parity-*`, `user_dir_round_trip-*`; env written for children: `J_MEASUREMENT_CELL`, `CONTRACT_DIFF_CELL`, `J_MATCH_INPUTS=stdin`, `LD_LIBRARY_PATH`, `OX_*` (user driver), `PINYIN_EXPORT_DIR`, `PINYIN_MODEL_DIR`, `GCOV_PREFIX`, `GCOV_PREFIX_STRIP`. |
| Outputs | returns `(attributed, nonmatch, incomplete, verdicts)` tuples; verdict vocabulary `MATCH / DIFFER / MEASUREMENT-DIFFER / TIMEOUT / SKIP`; stderr progress lines (`contract i/N name: verdict…`, `TIMEOUT_COUNTER_LIMIT`, `COUNTER_DECODE_LIMIT`, `same-data …`, `user-store: …`); surface workers print one JSON list per input to stdout; `RuntimeError`/`AssertionError` on guards. |
| Platform | Linux. |
| Consumers | `internal-ledger.py`, `delta_ledger.py`, `finish_ledger.py`, `render_ledger.py`, tests. |
| State | measurement build dir `OUT` (`*.o`, `gcov-flush.{cpp,o}`, `lib{pinyin,zhuyin}.{ver,so}`, `libpinyin.so.15` alias, `*.gcda` cleared per stage), `gcov-parallel` sibling dir, temp dirs `j-contract-`, `j-same-data-`, `j-user-store-`, `j-surface-`, `j-limit-roots-`; `WORKER_TIMEOUT = 60` per worker; `RLIMIT_CORE=0`; socket `j-dynamic.sock` (0600). |
| Overlap | `CELLS` table (conditional/macro/dbm/configure/library/feature/suffix) duplicates oracle-cell.sh and system-dir.sh mappings; `verify_prefix` is a third `pin_ref` constructor; `same_data` compiles the seven `*-diff.c` drivers itself (duplicating their `run-*.sh`). |
| Constraints | "counters are build intermediates, JSON is not saved"; credit only when the original differential MATCHes and the instrumented observation equals the reference; `FLUSH_SHIM` protocol (`J-GCOV-ABORT-BEGIN/END`, `SA_RESETHAND|SA_NODEFER`, re-raise); `checksum_limit` accepts only the two `abort-*save-chunk-header-checksum` cases with ≤4-byte `.gcda`; `gates` requires exactly one test binary and four exact test names (`GATE_TESTS`). |

### BIS-P-19 — tools/bisection/internal-ledger.py

| Field | Value |
|---|---|
| Entry point | `python3 internal-ledger.py --pin P --config config.h --oracle PREFIX --build DIR --output LEDGER.md [--cell bdb\|kc\|tkrzw] [--delta-from BDB.md] [--check] [--compare-ref SHA40] [--require-complete] [--static-preview] [--inventory-check] [--refresh-unknowns] [--tier-preview] [--sample-seed N] [--measurement-build D] [--measurement-config config.h] [--pinyin-so S] [--zhuyin-so S] [--surface-pinyin-so S] [--rust-target D] [--export D] [--model D]`; `main(contract_results=None, gate_receipt=None)` private continuation reads a JSON receipt from stdin. |
| Owning subsystem | profile (Axis J) |
| Purpose | Builds the pin's call graph (libclang AST + clang `-emit-llvm` + `llvm-link` + `opt -passes=print-callgraph`), resolves export roots from `nm -D` of the oracle `.so.15`, computes witness paths, virtual/callback resolution, then dispatches to `render_delta` / `refresh` / inventory-check / `render`. |
| Implementation | Python stdlib; 626 lines; spawns `clang++`, `llvm-dis`, `llvm-cxxfilt`, `llvm-link`, `opt`, `nm`, `pkg-config`, `git`. |
| Inputs | pin (`git rev-parse HEAD == PIN`, clean tracked tree), `config.h` (must `#define` the cell macro and none other, be named `config.h`), oracle `lib/lib{pinyin,zhuyin}.so.15`, pin `src/lib{pinyin,zhuyin}.ver`, subject `crates/oxpinyin-{,zhuyin-}capi/lib*.ver` (root sets must agree), `src/Makefile.am`. |
| Outputs | `.bc` files under `--build`; ledger Markdown; stdout summary lines; exit 1 with message on `RuntimeError/OSError`. |
| Platform | Linux (LLVM/clang/GCC toolchains). |
| Consumers | documented in all three `docs/findings/axis-j-*-ledger.md`; imported by `delta_ledger`, `render_ledger`, `finish_ledger`, tests. |
| State | `--build` bitcode dir (intentional full recompilation each run). |
| Overlap | `MAPPINGS` (9 USRs → Rust anchors) parallel to `coverage_reviews`; `mutation_check` duplicated in `delta_ledger.MUTATIONS`; `sources()` delegates to `gcov_measure.cell_sources`. |
| Constraints | `PIN = 074a2219…`; `(cell != bdb) == (delta_from given)` enforced; `--tier-preview/--inventory-check/--refresh-unknowns/--compare-ref` bdb-only; "The independent export roots come from nm, not from a checked-in expected count". |

### BIS-P-20 — tools/bisection/internal-ledger.test.py

| Field | Value |
|---|---|
| Entry point | `python3 tools/bisection/internal-ledger.test.py` (unittest). |
| Owning subsystem | profile (Axis J) / testsupport |
| Purpose | Compiler-backed regression tests for identity extraction, alignment, the SIGABRT flush shim, tier rules, mutation checks, cell selection, delta partitioning, residual guards and measurement-limit handling. |
| Implementation | Python unittest + mock; 881 lines; needs libclang, `clang++`, `g++ --coverage`, `gcov`. Six classes: `IdentityTests`, `TierTests`, `MutationTests`, `ReviewGuardTests`, `CellTests` (+ fixtures). |
| Inputs | synthetic `.cpp` probes in temp dirs; real `docs/findings/sentence-surface.md`, `tools/oracle/oracle-pin.txt`, the live review-rule registries. |
| Outputs | unittest result; exit per unittest. |
| Platform | Linux (toolchains). |
| Consumers | none in CI (`.github/workflows` do not reference it); manual. |
| State | temp dirs. |
| Overlap | asserts frozen constants: 15 `ABSENT_INLINE`, 4 known defects, `{user-gram-duplicate-export, user-gram-unsorted-export}` limits, 6 residuals, `WORKER_TIMEOUT` 60, 12 surface worker calls. |
| Constraints | several tests import `contract-diff.py` via `gcov_measure.contract_module()` (3449-case registration executes each time). |

### BIS-P-21 — tools/bisection/perf-baseline.py

| Field | Value |
|---|---|
| Entry point | `perf-baseline.py summarize-speed JSONL` \| `summarize-ram INIT.jsonl CYCLE.jsonl` \| `size ORACLE_PREFIX OXPINYIN_PREFIX`. |
| Owning subsystem | perf |
| Purpose | Markdown summaries (medians [min,max], ratios) of `bisect --perf` captures and installed-prefix sizes for the oracle/oxpinyin pair. |
| Implementation | Python stdlib (statistics, pathlib); 228 lines. |
| Inputs | JSONL rows with `backend ∈ {oracle, oxpinyin}`, `init_ns`, `alloc_ns`, `cycles_ns[]`, `after_init/after_first/after_last {rss_kib, hwm_kib, rss_anon_kib, rss_file_kib}`; prefix trees (`share/oxpinyin/` vs `lib/libpinyin/data`). |
| Outputs | Markdown to stdout; exit 0. |
| Platform | portable. |
| Consumers | `run-perf-baseline.sh`; docs `perf-baseline-kc-2026-08-31.md`, `perf-baseline-kc-validation-2026-08-31.md`, `docs/perf/perf-baseline-2026-08.md`. |
| State | none. |
| Overlap | `load_jsonl/median/ms/fmt_ms/ratio` copied in `perf-matrix.py`; size classification differs from `perf-matrix.measure_size` (`share/oxpinyin/` vs `--data-dirs`). |
| Constraints | "Sizes are measured from the installed prefixes, not from cargo target dirs"; backend labels hard-coded `oracle`/`oxpinyin`. |

### BIS-P-22 — tools/bisection/perf-ci.py

| Field | Value |
|---|---|
| Entry point | `perf-ci.py speed.jsonl [more…] [--metric init\|alloc\|cold\|steady]* [--resamples N=10000] [--seed 20260907] [--format text\|md]`. |
| Owning subsystem | perf |
| Purpose | Per-cell medians with 95 % percentile-bootstrap CIs, resampling whole runs (processes), grouped by `(backend, repeats)`. |
| Implementation | Python stdlib (random, statistics); 164 lines. |
| Inputs | JSONL rows with `cycles_ns` (ram-init rows skipped), optional `repeats` (default 1). |
| Outputs | table (text/md) on stdout; `# whole-run percentile bootstrap; N resamples; seed S` on stderr; `sys.exit` messages on bad JSON/no rows/`--resamples<1`. |
| Platform | portable. |
| Consumers | `run-callgrind-differential.sh`; docs `perf-keycost-first-alloc-2026-09-07.md`, `perf-steady-cycle-cross-host-2026-09-07.md`; referenced by `perf-decomp.py`/`perf-ratio.py` docstrings. |
| State | none. |
| Overlap | loader and percentile code duplicated in `perf-ratio.py` and `perf-decomp.py` (same default seed). |
| Constraints | Deterministic seed; "Resampling unit: the process"; "makes no attempt to reproduce any previously published interval". |

### BIS-P-23 — tools/bisection/perf-decomp.py

| Field | Value |
|---|---|
| Entry point | `perf-decomp.py speed.jsonl… --l LABEL --x LABEL --y LABEL --z LABEL [--repeats 1] [--resamples 10000] [--seed 20260907] [--format text\|md]`. |
| Owning subsystem | perf |
| Purpose | Paired (over rounds) bootstrap of the tree effect `Y/L − X/L` and recipe effect `Z/L − Y/L` from a four-cell round-robin control capture. |
| Implementation | Python stdlib; 171 lines. |
| Inputs | JSONL rows for the four backend labels at one `repeats`; round alignment asserted (equal row counts, ≥2). |
| Outputs | ratios + CIs (text/md); stderr seed note; `sys.exit` on misaligned rounds. |
| Platform | portable. |
| Consumers | `run-tree-recipe-control.sh`; `docs/findings/perf-cycle-ir-differential-2026-09-08.md`. |
| State | none. |
| Overlap | see perf-ci.py. |
| Constraints | "Resampling is therefore over ROUNDS, not cells"; steady cycles only (`cycles_ns[1:]`). |

### BIS-P-24 — tools/bisection/perf-matrix.py

| Field | Value |
|---|---|
| Entry point | `perf-matrix.py summarize --speed F --ram-init F --ram-cycle F --labels L… --size-prefixes P… --data-dirs D…`; `--test` runs one built-in regression. |
| Owning subsystem | perf |
| Purpose | 2×2 (backend × implementation) factorial report for labels `libpinyin-kc`, `libpinyin-tkrzw`, `oxpinyin-kc`, `oxpinyin-tkrzw`: medians, CV %, backend/implementation effects, interaction, init-gap decomposition, installed sizes. |
| Implementation | Python stdlib; 441 lines. |
| Inputs | three JSONL captures; prefix/data dirs (`FileNotFoundError` if absent). |
| Outputs | Markdown on stdout; exit 0. |
| Platform | portable. |
| Consumers | `run-perf-matrix.sh`. |
| State | none. |
| Overlap | duplicates perf-baseline.py helpers; `CELL_ORDER`/`CELL_MATRIX_ORDER` hard-code the four labels (no bdb cell). |
| Constraints | `_test_sibling_directory_not_excluded` guards the `data-old` vs `data` prefix bug. |

### BIS-P-25 — tools/bisection/perf-ratio.py

| Field | Value |
|---|---|
| Entry point | `perf-ratio.py speed.jsonl… --numerator LABEL --denominator LABEL [--metric steady] [--repeats 1] [--resamples 10000] [--seed 20260907] [--format text\|md]`. |
| Owning subsystem | perf |
| Purpose | Whole-run bootstrap CI of `median(num)/median(den)` — the number the steady-cycle decision rule reads. |
| Implementation | Python stdlib; 171 lines. |
| Inputs | JSONL; each cell ≥2 runs. |
| Outputs | ratio, CI, width; stderr seed note; `sys.exit` on missing cells/samples. |
| Platform | portable. |
| Consumers | `run-shipped-check.sh`, `run-tree-recipe-control.sh`, `perf-decomp.py` docs; `docs/findings/perf-cycle-ir-differential-2026-09-08.md`. |
| State | none. |
| Overlap | see perf-ci.py (argument verbatim quoted). |
| Constraints | "Every capture from here on ratios through this". |

### BIS-P-26 — tools/bisection/render_ledger.py

| Field | Value |
|---|---|
| Entry point | module: `render(args, state, contract_results, gate_receipt)`; helpers `align(nodes, measurements, symbol_base)`, `citation`, `anchors(root, targets)`, `labels_text`, `baseline_rows(text)`, `annotate_comparison`; constants `BASELINE`, `BASELINE_ID_SHA256`, `AUDITED_SEED`, `AUDITED_SAMPLE`. |
| Owning subsystem | profile (Axis J, bdb) |
| Purpose | Renders the full bdb ledger table and header: aligns gcov identities to clang/LLVM nodes (exact symbol + extent + TU; hand-reviewed `_Zeq/_Zne` join), classifies each row (`BEHAVIOR-COVERED`, `T1-CITED`, `T3-FACILITY`, `T5-REVIEWED`, `T2-UNREACHABLE`, `UNKNOWN-LIMIT`, `NOT-EXECUTED-UNREVIEWED`), runs mutation checks, optional `--compare-ref` annotation. |
| Implementation | Python stdlib; 417 lines; `input()` pauses for the private parallel-stage protocol (`PARALLEL_STAGES_PENDING`). |
| Inputs | `state`, measurements from gcov_measure, `docs/findings/axis-j-bdb-ledger.md` (baseline 962 identities, SHA-guarded), `git`. |
| Outputs | ledger text (write or `--check`), summary prints, `RuntimeError` guards (baseline drift, citation rule/sample drift: `len(cited)==29`, sample `{pinyin_reset, UserTableInfo::get_open_counter}`). |
| Platform | Linux. |
| Consumers | `internal-ledger.py`; `finish_ledger.py`, `delta_ledger.py` import helpers. |
| State | may open `j-dynamic.sock`. |
| Overlap | header prose is frozen string literals; classification order duplicated in `delta_ledger.classify`. |
| Constraints | `importlib.reload(measure)` and `reload(coverage_tiers)` at render time; "No capture, log, report or evidence archive is written". |

### BIS-P-27 — tools/bisection/rss-smaps.py

| Field | Value |
|---|---|
| Entry point | `rss-smaps.py <smaps>…` \| `rss-smaps.py --diff A B`; usage exit 2. |
| Owning subsystem | perf / profile |
| Purpose | Groups a `/proc/self/smaps` dump by mapping identity (`DATA:<file>` for `/libpinyin/data/`, `CODE:<so>`, `[heap]`, `[anon]`) and diffs two dumps. |
| Implementation | Python stdlib; 124 lines; restores `SIGPIPE` default. |
| Inputs | smaps text written by `bisect --perf` in `PERF_MODE=rss-diag` with `RSS_DIAG_DIR`. |
| Outputs | tables on stdout; exit 0. |
| Platform | Linux (smaps format). |
| Consumers | `docs/runbooks/benches.md`, `docs/findings/rss-attribution-2026-09-09.md`; `run-rss-diag.sh` produces inputs. |
| State | none. |
| Overlap | sibling of cg-calls.py. |
| Constraints | path heuristic `'/libpinyin/data/' in name` identifies the data directory. |

### BIS-P-28 — tools/bisection/train-index-diff.py

| Field | Value |
|---|---|
| Entry point | `train-index-diff.py CELL PREFIX PINYIN_SO ZHUYIN_SO [--expect-parent]`; internal `--worker MODE SO DATA PROFILE CASE`. |
| Owning subsystem | bisection |
| Purpose | Differential of `pinyin_train(index)` for indexes `0,1,2,len,255` (+`fresh`, `choose-only`) and zhuyin `0/fresh/choose-only`: the pin aborts past the result set, the subject must refuse with one `libpinyin` warning and leave the seed profile untouched; otherwise saved state must be identical. |
| Implementation | Python stdlib + ctypes; 230 lines; loads `libglib-2.0.so.0`; imports `compare-nbest-training-state.py` (`bigrams`) → backend dump tools. |
| Inputs | `<prefix>/oracle-pin.txt` (must contain the commit sha, `+dbm-<cell>\n`, and **not** `+patches-`), `lib/lib{pinyin,zhuyin}.so`, data `bigram.db`; env none (besides inherited). |
| Outputs | JSON lines per mode/case (`pre_training_rows`, `pin_return 'SIGABRT'|bool`, `subject_return`, `records_and_dbin_equal`, `stderr_lines`, `warnings`, `exported_bigram_records`, `dbin_sha256`, `raw_index_equal`); `AssertionError` on any mismatch; asserts `count == 10`; exit 0 on success (no 77). |
| Platform | Linux. |
| Consumers | runners.json `train-index-diff`; docs compatibility-policy, upstream-divergences. |
| State | `TemporaryDirectory(prefix='train-index-')` with `seed/`, per-case profiles, `index-dump/`; `RLIMIT_CORE=0`. |
| Overlap | third ctypes facade driver (after contract-diff.py and compare-nbest-training-state.py); hard-codes expected rows `['今天','今田','今添']` / `['你好']` and `0x010059F5`. |
| Constraints | "training requires the unmodified pin" (`+patches-` refused); bounds cases expect pin `-6`. |

### BIS-P-29 — tools/generate_w3_fixtures.py

| Field | Value |
|---|---|
| Entry point | `python3 tools/generate_w3_fixtures.py [--data-dir PATH] [--max-records N=100] [--output-dir DIR] [--hash-only]`. |
| Owning subsystem | fixtures |
| Purpose | Truncates the pinned oracle's 12 addon content files (`art.bin` … `technology.bin`) to ≤N records, rebuilding header/index regions, and writes `fixtures/w3/` with `pin-ref.txt` and `fixtures.sha256`. |
| Implementation | Python stdlib (struct, hashlib); 307 lines; a hand-written record-size model (`sub_record_size`, `record_size`, 28-byte `<7I` header, 140-byte primary index, 6/10-byte preambles). |
| Inputs | `~/.local/opt/pinyin-oracle/lib/libpinyin/data` by default; walks up ≤4 dirs for `oracle-pin.txt` to read the `pin_ref=` line. |
| Outputs | fixture files, `pin-ref.txt`, `fixtures.sha256`; progress on stderr; exit 1 if data dir missing. `SYSTEM_FILES` is defined but never used. |
| Platform | portable. |
| Consumers | `fixtures/w3/README.md`. Not invoked by scripts. |
| State | writes into `fixtures/w3/` (committed artifacts). |
| Overlap | the fourth `oracle-pin.txt` reader (substring scan for `pin_ref=`). |
| Constraints | "Run twice, identical output (reproducible)"; the format model is undocumented heuristics (`fl`/`ng` byte-driven sizes). |

## Count

29 records: BIS-P-01, BIS-P-02, BIS-P-03, BIS-P-04, BIS-P-05, BIS-P-06, BIS-P-07, BIS-P-08, BIS-P-09, BIS-P-10, BIS-P-11, BIS-P-12, BIS-P-13, BIS-P-14, BIS-P-15, BIS-P-16, BIS-P-17, BIS-P-18, BIS-P-19, BIS-P-20, BIS-P-21, BIS-P-22, BIS-P-23, BIS-P-24, BIS-P-25, BIS-P-26, BIS-P-27, BIS-P-28, BIS-P-29.

## Architecture notes

Observation unless marked *(interp.)*.

- **oracle-pin.txt is parsed five different ways.** `oracle-cell.sh` builds `EXPECTED_PIN_REF` from `tools/oracle/build-oracle.sh` constants (`LIBPINYIN_VERSION/LIBPINYIN_SHA/MODEL_SHA256`) and `run-all.py` requires that full line in the prefix manifest; `contract-diff.py regenerate_sentence_ties` and `gcov_measure.verify_prefix` rebuild it from `tools/oracle/oracle-pin.txt` fields (`libpinyin_tag`, `libpinyin_commit`/`PIN`, `model_archive_sha256`) and compare `dbm=` and the data manifests + every data-file SHA; `contract-diff.py main` and `train-index-diff.py main` substring-check the hard-coded commit and `+dbm-<cell>\n` (train-index also refuses `+patches-`); `generate_w3_fixtures.py` only scrapes the `pin_ref=` line. `delta_ledger.prefix_provenance`/`gcov_measure.oracle_hashes` read `shared_object_sha256`/`zhuyin_shared_object_sha256` with `dict(line.split('=',1))`. *(interp.)* One shared Python/bash reader would remove four drift points.
- **table.conf / datagen-manifest.txt parsers:** `system-dir.sh system_dir_data_ext` (`backend=` line, `database format:` token → db/kct/tkt); `contract-diff.py _write_user_conf` (three header keys copied into `user.conf`), `d18_bigram_db` (backend chosen by `database format:` text, *not* by `CELL` — the only place in the file that does so), `d18_unigram` (`default ` rows), `conf_system` (rewrites rows); `coverage` none.
- **Backend name/extension hard-coding sites:** `oracle-cell.sh` (`bdb|kc|tkrzw` → `BerkeleyDB|KyotoCabinet|Tkrzw`, `bdb|kyotocabinet|tkrzw`, `db|kct|tkt`); `system-dir.sh` (`SYSTEM_DIR_BACKEND_EXTS`, ext↔name↔feature functions, `fixtures/w3/<ext>`, `target/datagen/<ext>`); `gcov_measure.CELLS` (conditional/macro/dbm/configure/library/feature/suffix); `run-all.py CELLS=('bdb','kc','tkrzw')` and the three `.so`/dictool names; `contract-diff.py` (`CELL`, sonames `libkyotocabinet.so.16`, `libtkrzw.so.1`, `find_library('db')`, `db5.3_dump`, BDB magic bytes `62310500`/`61150600`, `#type=kct`, `dbm=TreeDBM|HashDBM`); `compare-nbest-training-state.py` (`db5.3_dump`, `kchashmgr`, `tkrzw_dbm_util`); `perf-matrix.py` labels (`-kc`/`-tkrzw` only).
- **Cargo spawns:** `oracle-cell.sh oracle_cell_artifact` (`cargo build --locked … -p <pkg> --no-default-features --features $CAPI_FEATURE`), `cell-artifacts.sh` (three-package build + `cargo test --no-run … --test user_dir_round_trip`), `system-dir.sh system_dir_capi_ext` (`cargo tree --offline … -e features -i oxpinyin-store`). run-all.py never calls cargo directly. The Axis J tools never build Rust (they require prebuilt `--pinyin-so/--zhuyin-so/--rust-target`).
- **Process control / timeouts / locks:** run-all.py — `start_new_session`, `killpg` TERM→5 s→KILL, `fcntl.flock` on `target/runner-locks/<resource>`, ThreadPool `--jobs`; contract-diff.py — subprocess per case per side with **no timeout** standalone, `os.fork` probes, `RLIMIT_CORE=0`, `PR_SET_DUMPABLE`, ThreadPool(2) for Axis C; gcov_measure — `WORKER_TIMEOUT=60`, `TimeoutExpired → exit='TIMEOUT'`, SIGABRT shim; train-index-diff — none; perf tools — none. No other lock mechanism exists.
- **Exit-code conventions:** run-all.py maps runner `0/77/other` → PASS/SKIPPED/FAIL and promotes `SKIP:` lines to 77; contract-diff.py, train-index-diff.py, compare-nbest (0/2), perf-* use 0/1 (or `sys.exit(msg)`), never 77; system-dir.sh/oracle-cell.sh fatal = exit 3; cell-artifacts.sh usage = 2. `run-contract-diff.sh` and `train-index-diff.py` therefore never SKIP on a missing oracle (they assert), relying on run-all.py's pre-check.
- **Env-var aliases:** sweep-wide `OXPINYIN_SYSTEM_DIR` vs per-runner `DICT_SYSTEM`, `IMPORT_SYSTEM`, `KEY_SYSTEM`, `PHRASE_SYSTEM`, `ZHUYIN_RUST_DATA/ZHUYIN_ORACLE_DATA` (all set from `{data}` in runners.json); oracle prefix: `PINYIN_ORACLE_PREFIX` vs `LOCALE_DIFF_ORACLE_PREFIX`, `OPEN_COUNTER_ORACLE_PREFIX`, `SESSION_FORK_ORACLE_PREFIX`, `TWO_CONTEXT_ORACLE_PREFIX`, `MODES_ORACLE_PREFIX`, `ZHUYIN_ORACLE_PREFIX`, `PINYIN_IBUS_PREFIX`; subject `.so`: `OXPINYIN_CAPI_SO` vs `*_PINYIN_SO`, `RESIDUE_A_CAPI_SO`; `OXPINYIN_ZHUYIN_SO` vs `*_ZHUYIN_SO`, `ZHUYIN_RUST_SO`. Cell: `PINYIN_ORACLE_DBM` (bash) vs `CONTRACT_DIFF_CELL`/`J_MEASUREMENT_CELL` (python) — contract-diff.py never reads `PINYIN_ORACLE_DBM`; it takes the cell positionally.
- **Duplicated code:** perf-ci/perf-ratio/perf-decomp share `load`/`pooled`/percentile logic verbatim; perf-baseline/perf-matrix share `load_jsonl/median/ms/fmt_ms/ratio/human`; `delta_ledger.MUTATIONS` = `internal-ledger.mutation_check` predicates; `delta_ledger.reviewed` ≈ `finish_ledger.source_mapping` ≈ the `T5` branch in `render_ledger.render`; the three `chewing_large_table2_*_cpp.py` rule batches; MemoryChunk framing and logger record parsing in `contract-diff.py` (`_chunk`, `_modify_header`, `issue790_readings`) and `compare-nbest-training-state.py` (`unigram_fields`, `pronunciation_fields`); ctypes facade loaders in contract-diff.py (`Kit`), compare-nbest (`read_unigrams`), train-index-diff (`worker`).
- **Frozen / "do not change":** `docs/findings/sentence-surface.md` §12 (six residuals; two parsers: `contract-diff.declared_sentence_residuals`, `gcov_measure.declared_residuals`); `fixtures/w4/oracle-sentence-tie-order.txt` (`format=sentence-tie-order-v1`, regenerated only from the pin on bdb); `axis-c-replacement.json` (`axis-c-replacement-v1`, SHA-256 of 10 source fixtures verified in Axis C modes); ledger constants (`BASELINE_ID_SHA256`, 962 rows, `MANUAL_*_SHA`, 178/20/24 populations, `MANUAL_CRATES_TREE`, `AUDITED_SEED/SAMPLE`, 29 cited rows, 15 synthetic bodies, 4 `GATE_TESTS`); `PIN` literal in `internal-ledger.py`, `gcov_measure.py`, `contract-diff.py`, `train-index-diff.py`; run-all.py protected env set; `oracle-cell.sh` single variant name; bootstrap seed `20260907`.
- *(interp.)* run-all.py's `{data}` for a variant runner resolves against the **unpatched** prefix while `{prefix}` is the patched one; both `bigram-export-diff` and `residue-a-tail-diff` pass `{prefix}/lib/libpinyin/data` or `OXPINYIN_SYSTEM_DIR` respectively, so the discrepancy matters only if the patched prefix ships different data.
- *(interp.)* None of the Python tools is exercised by `.github/workflows/*`; run-all.py and the ledgers are maintainer-driven, so a replacement is verified only by local runs and `internal-ledger.test.py`.


---

# Inventory — ORC (oracle provisioning, cloud, model fetch, drop-in differential, oracle-image workflow)

Tree read: detached worktree at origin/main `e67f19d1` (`/tmp/claude-1000/.../scratchpad/ox`). Every assigned file was read in full. Pin cited: libpinyin `074a2219c90feaf962d0d24f034514033ece5f99`.

### ORC-01 — tools/oracle/build-oracle.sh

| Field | Value |
|---|---|
| Entry point | Executed: `build-oracle.sh [--work-dir DIR] [--prefix DIR] [--jobs N] [--apply-patches DIR] [--model-dir DIR] [--enable-libzhuyin] [--dbm bdb\|kc\|tkrzw] [--libs-only \| --regen-data] [-h]`. Three modes: `full` (default: libs + ibus-libpinyin build + model data), `libs` (`--libs-only`), `data` (`--regen-data`). Also run bind-mounted at `/usr/local/bin/build-oracle.sh` inside the deps image (check-reproducible.sh, cloud/setup.sh) and `COPY`ed into Dockerfile.oracle's `build` stage. |
| Owning subsystem | oracle provisioning |
| Purpose | Produces the pin-built libpinyin (and optionally libzhuyin, ibus-libpinyin, model20-derived data) at an install prefix, verified by commit SHA and archive SHA-256, and writes the prefix manifest `oracle-pin.txt` that every runner checks. The canonical home of the pin constants (`LIBPINYIN_VERSION`, `LIBPINYIN_SHA`, `MODEL_SHA256`, ...). |
| Implementation | bash; 584 lines. Needs curl, git, sha256sum, tar, autoreconf, make, pkg-config, find, sort, xargs, patch (checked at start); build deps: autotools, C/C++ toolchain, gettext, gnome-common, python3, GLib 2.0 / IBus 1.0 / SQLite 3 dev headers, and one of libdb-dev / libkyotocabinet-dev / libtkrzw-dev. |
| Inputs | Network: `LIBPINYIN_GIT_URL` and `IBUS_LIBPINYIN_GIT_URL` (git fetch --depth=1 by SHA), `MODEL_URL` (curl, unless `--model-dir`). `--model-dir` must hold the 18 `MODEL_FILES` AND a sibling `../verified` marker containing `sha256=<MODEL_SHA256>` (fetch-model.sh writes it). `--apply-patches DIR`: every `*.patch` in DIR, sorted. `--regen-data`: the prefix's existing `oracle-pin.txt` must contain `pin_ref=<ORACLE_PIN_REF>` exactly, and `tools/oracle/oracle-pin.txt` (located via `$BASH_SOURCE`) must carry `data_manifest_sha256_<dbm>`. Env read: `TMPDIR` (default work dir), `CC`, `CXX`, `CFLAGS`, `CXXFLAGS`, `LDFLAGS` (passed through; CFLAGS/CXXFLAGS default `-g -O2`), `PKG_CONFIG_PATH` (appended after prefix paths), `LD_LIBRARY_PATH` (overwritten, not inherited). Sets `SOURCE_DATE_EPOCH`, `TZ=UTC`, `LC_ALL=C`. |
| Outputs | Prefix tree: `lib/libpinyin.so[.15*]`, `lib/libzhuyin.so` (opt.), `include/libpinyin-<ver>/pinyin.h`, `lib/pkgconfig/*.pc`, `lib/libpinyin/data/` (full/data modes), `bin/` data utils (data mode via `make -C utils install`), `oracle-pin.txt` (schema `pinyin-oracle-v1`), `oracle-data.sha256` (17 stable files), `oracle-data-unstable.sha256` (6 DBM-path files), `oracle-patches.sha256` (with `--apply-patches`). Stdout: last line is the absolute path of the built libpinyin shared object; `libpinyin_tag=`/`ibus_libpinyin_tag=` lines go to stderr. Exit: 0 ok; 2 usage/option conflicts; 1 missing tool, checksum/commit mismatch, non-empty prefix, provenance marker missing, `FAIL: data_manifest_sha256 mismatch`. No 77. |
| Platform | Linux-only (autotools build of libpinyin, ELF flags `-Wl,--build-id=none`, GNU `find -print0`/`sort -z`). Also runs in the pinned container. |
| Consumers | Dockerfile.oracle (`build` stage, 3 cells), check-reproducible.sh (bind-mount), cloud/setup.sh (`in_image`, both `--libs-only` and `--regen-data`), docker-user-rt.sh (copies and rewrites it with python to a file:// mirror and skips ibus), nightly-fixture-differentials.sh and tools/bisection/oracle-cell.sh (both `sed` the pin constants `LIBPINYIN_VERSION`, `LIBPINYIN_SHA`, `LIBPINYIN_GIT_URL`, `MODEL_SHA256` out of this file), image-tag.sh (hashes the file into the tag), tools/bisection/run-scheme-diff.sh (tells the user to run it with `--apply-patches`), docs/runbooks/oracle.md §2/§2a, docs/runbooks/cloud.md, crates/pinyin-oracle (doc references; `build.rs` verifies its product). |
| State | Work dir `${TMPDIR:-/tmp}/oxpinyin-oracle` with `downloads/` (archive cache, reused if present) and `src/<name>-<sha>` (rm -rf'd and re-fetched every run). Prefix must be empty (full/libs) or an existing libs-only prefix of the same pin (data). `--regen-data` stages into `mktemp -d "$prefix.stage.XXXXXX"` beside the prefix (same filesystem), `trap rm -rf` on EXIT, installs with `DESTDIR=$stage`, swaps in only after the manifest check. No locks, no timeouts, `--jobs` default 1. |
| Overlap | tools/model/fetch-model.sh duplicates `MODEL_URL`, `MODEL_SHA256`, the 18-file inventory (`EXPECTED_FILES` vs `MODEL_FILES`) and the curl+sha256sum fetch; crates/pinyin-oracle/src/model_cache.rs `EXPECTED_MODEL_FILES` is the third copy (comment in this file says keep in lockstep). tools/bisection/Dockerfile.perf-matrix re-implements the configure/make recipe by hand (`/opt/libpinyin-{tkrzw,kc}`, "Mirrors tools/oracle/build-oracle.sh"). nightly-fixture-differentials.sh re-implements the git fetch-by-SHA + autoreconf + configure + make install sequence. run-differentials.sh header carries a hand recipe for the data build. The `dbm` case table (flag→configure name) is repeated in user-dir-round-trip.sh (reverse direction), tools/bisection/oracle-cell.sh, and crates/pinyin-oracle/build.rs (`PINYIN_BENCH_DBM`). |
| Constraints | Pin constants are the single source of truth (comments: "the canonical home"); the manifest key keeps the name `libpinyin_tag` "for compatibility with existing manifests". `--apply-patches` requires an explicit `--prefix` (a patched build must never land in the default prefix). `DATA_UNSTABLE_FILES` is a fixed list (moving a file between classes is a deliberate recipe change: docs/findings/oracle-data-reproducibility.md). Reproducibility recipe (issue #818): `SOURCE_DATE_EPOCH=$(git log -1 --format=%ct)` of the pin, `-ffile-prefix-map=$lib_src=/usr/src/libpinyin -fdebug-prefix-map=...`, `-Wl,--build-id=none`; changing the recipe changes the image tag and the recorded hashes (must be updated in the same commit). Default `--dbm bdb` is a human ruling (2026-09-27 UTC); the frozen capture goldens and the pinyin-oracle crate's `EXPECTED_PIN_REF` are `+dbm-tkrzw`. The `pin_ref` string format `libpinyin-<ver>-<sha>+model20-<sha>+dbm-<flag>[+patches-<sha>]` is frozen: it is matched byte-exactly by ~25 runners, `run-capture.sh`, and `crates/pinyin-oracle/src/pin_ref.rs`. Both `--libs-only` and `--regen-data` and `--apply-patches`+`--regen-data` are mutually exclusive. |

### ORC-02 — tools/oracle/check-reproducible.sh

| Field | Value |
|---|---|
| Entry point | Executed: `check-reproducible.sh [--engine docker\|podman] [--image IMAGE] [--jobs N]`; `--help` prints its own header lines 4–21 via `sed`. |
| Owning subsystem | oracle provisioning |
| Purpose | The reproducibility gate (issue #818): are the oracle libraries a pure function of the recipe? Builds each of bdb, kc, tkrzw twice in the pinned image from different work dirs and different prefixes and requires both builds to hash identically and to equal the values in `tools/oracle/oracle-pin.txt`. |
| Implementation | bash; 135 lines. Needs docker or podman, cmp, diff, mktemp, nproc; network to github.com (the two upstream fetches) and snapshot.debian.org/docker.io when the deps image must be built. |
| Inputs | `tools/oracle/Dockerfile.oracle` (deps stage, built as `oxpinyin-oracle-deps:check` unless `--image`), `tools/oracle/build-oracle.sh` (bind-mounted read-only), `tools/oracle/oracle-pin.txt` (`<lib>_sha256_<dbm>` fields). Env: `HTTPS_PROXY`/`https_proxy`, `OXPINYIN_PROXY_CA` (default `/root/.ccr/ca-bundle.crt`); when docker + proxy + CA present: adds `--network host`, proxy env, `GIT_SSL_CAINFO`/`SSL_CERT_FILE`/`CURL_CA_BUNDLE=/run/ca.crt`, CA mount, and `--build-arg https_proxy`, `--secret id=cacert`. |
| Outputs | Stdout: one `<dbm> <library> <sha256>` line per cell/library (from the "a" build). Stderr: `FAIL: <dbm>: a build exited non-zero`, `FAIL: <dbm> builds differ` + diff, `FAIL: <dbm> <lib> is X, tools/oracle/oracle-pin.txt records Y`. Exit 0 all match; 1 any failure or no engine; 2 unknown option. No 77. |
| Platform | Container (`--platform linux/amd64` on every build/run); host needs docker/podman. |
| Consumers | Nothing invokes it mechanically; docs/runbooks/oracle.md §2a "Reproducing the libraries" tells a human to run it when the recipe/base/snapshot changes; oracle-pin.txt v4 comment names it as the gate. |
| State | `scratch=$(mktemp -d)` with `trap rm -rf` holding `<tag>.out` per build; the two builds of a cell run concurrently as background jobs (`&`, `wait $pid_a`/`wait $pid_b` separately). In-container paths `/build/a` + `/opt/oracle-a/<dbm>` vs `/var/tmp/elsewhere/b` + `/opt/oracle-b/<dbm>`. Build log `/tmp/build.log` inside the container, last 40 lines surfaced on failure. No timeout. Podman gets `--security-opt label=disable`. |
| Overlap | verify-oracle-image.sh and cloud/setup.sh do the same `sed -n "s/^${lib}_sha256_$dbm=//p"` lookup and `sha256sum \| cut` compare; cloud/setup.sh duplicates the proxy/CA flag construction and the `--target deps` build (as `build_deps_image`). The in-container `build-oracle.sh --libs-only --enable-libzhuyin --dbm X --work-dir --prefix --jobs` line is the same as Dockerfile.oracle's and setup.sh's. |
| Constraints | Each build's own exit status counts (PR #819: "a build that fails after printing its hashes must not pass because the other one succeeded"). The deps image tag `oxpinyin-oracle-deps:check` is fixed. Compares against the record only — never updates it. |

### ORC-03 — tools/oracle/Dockerfile.oracle

| Field | Value |
|---|---|
| Entry point | `docker build [--platform linux/amd64] [--target deps] -f tools/oracle/Dockerfile.oracle [-t TAG] tools/oracle` (context is `tools/oracle/`). Build arg `ORACLE_PLATFORM` (default `linux/amd64`), predefined build arg `https_proxy`, optional BuildKit secret `cacert`. |
| Owning subsystem | oracle provisioning |
| Purpose | Pins the build environment (debian:testing by digest `dab11cdb…`, apt snapshot `20260831T000000Z`, listed dev packages) so that `build-oracle.sh --libs-only` is reproducible, and produces a `FROM scratch` image carrying `/opt/pinyin-oracle/{bdb,kc,tkrzw}` (libs, headers, pkgconfig, per-prefix oracle-pin.txt) with no model-derived data. |
| Implementation | Dockerfile (3 stages: `deps`, `build`, final scratch); 63 lines. apt packages: build-essential, autoconf, automake, autopoint, libtool, pkg-config, gettext, gnome-common, curl, ca-certificates, git, tar, patch, python3, libglib2.0-dev, libsqlite3-dev, libdb-dev, libkyotocabinet-dev, libtkrzw-dev. |
| Inputs | `build-oracle.sh` (COPYed from the context); network: snapshot.debian.org (http, or https when the `cacert` secret is present), github.com. |
| Outputs | Images: `deps` stage (toolchain), final image with `/opt/pinyin-oracle/<dbm>/`. Build stage removes `/build/<dbm>` after each cell. |
| Platform | Container, linux/amd64 only (every `FROM --platform=$ORACLE_PLATFORM`). |
| Consumers | .github/workflows/oracle-image.yml (`docker/build-push-action`, context `tools/oracle`), check-reproducible.sh (`--target deps`), cloud/setup.sh (`build_deps_image`, `--target deps`, tag `oxpinyin-oracle-deps:<sha256 of this file, 12 hex>`), image-tag.sh (hashes the file; greps that `base_image` and `apt_snapshot` from oracle-pin.txt appear in it). |
| State | No runtime state. The `deps` stage is cached by the engine under two different tags (`:check` by check-reproducible.sh; `:<dockerfile-hash>` by setup.sh). |
| Overlap | tools/bisection/Dockerfile.perf-matrix uses the same base digest and the same apt snapshot (its line 26/29) and builds its own libpinyin prefixes at `/opt/libpinyin-{tkrzw,kc}` plus the model. |
| Constraints | `base_image` and `apt_snapshot` in oracle-pin.txt must appear verbatim in this file (image-tag.sh refuses otherwise). Must carry NO model20-derived data (docs/findings/model-provenance.md; verify-oracle-image.sh checks). Secret mount mode 0444 so apt's `_apt` user can read it (PR #819). The in-image paths `/build/<dbm>` and `/opt/pinyin-oracle/<dbm>` are the ones setup.sh reproduces so "a prefix means the same thing wherever it was produced". |

### ORC-04 — tools/oracle/docker-user-rt.sh

| Field | Value |
|---|---|
| Entry point | Executed: `docker-user-rt.sh [IMAGE]` (default `oxpinyin-validate:latest`). |
| Owning subsystem | oracle provisioning (drop-in task-9 verification wrapper) |
| Purpose | One-container run of the whole user-dir round-trip verification: build a tkrzw pin oracle inside the container from host-side mirrors, then run `user-dir-round-trip.sh` both directions. Written for a host whose container egress to github/Debian CDN was unreliable. |
| Implementation | bash wrapper; 108 lines; docker, git, plus an inline python3 program that text-patches a copy of build-oracle.sh (`re.sub` to delete the ibus build block, URL swapped to `file:///work/target/mirrors/libpinyin.git`). Inside the container: apt-get with `Acquire::*::Timeout=20`, `Retries=8`, 10 retry loops. |
| Inputs | Host: `<repo>/target/model20/extracted` (required; errors out pointing at fetch-model.sh), `<repo>/target/mirrors/libpinyin.git` (created with a full `git clone` if missing), `$HOME/.cargo/registry` (mounted). Env: `HOME`. Container env set: `CARGO_TARGET_DIR=/tmp/target`, `HOME=/root`, `DEBIAN_FRONTEND=noninteractive`. |
| Outputs | `<repo>/target/rt-oracle.log` (everything), stdout: `CONTAINER-EXIT:<n>` and last 5 log lines. Markers in the log: `UPDATE-OK`, `APT-DONE`, `ORACLE-BUILD-EXIT:n`, `ROUNDTRIP-EXIT:n`. Exit = the container's status (1 on any step failing). |
| Platform | Linux + docker; `--network host`. |
| Consumers | None in the tree (only docs/findings/user-store.md mentions it). The base image `oxpinyin-validate:latest` is not built by any Dockerfile in the tree (grep: only docs/findings and docs/perf mention it). |
| State | Oracle built at `/tmp/oracle-work` / `/tmp/oracle-prefix` inside the container (ephemeral, `--rm`). Writes into the repo's `target/` (mirror, log). `git config --global --add safe.directory "*"` inside the container. |
| Overlap | Duplicates the oracle build invocation of cloud/setup.sh and nightly-fixture-differentials.sh, but by mutating a copy of build-oracle.sh instead of calling it as is. Hard-codes `--dbm tkrzw` (comment: the container has no libdb; default moved to bdb 2026-09-27). |
| Constraints | Depends on build-oracle.sh's exact text (the python `replace`/`re.sub` targets `LIBPINYIN_GIT_URL=…`, the `ibus_src=$(fetch_commit …)` line and the `(\n\tcd "$ibus_src"\n…\n)\n` block); the `fetch_commit` + `mode==full` guard introduced by #818 means the `re.sub` now targets a block that is wrapped in `if [[ $mode == full ]]` — the script's patching has not been re-verified after #818 (interpretation: likely stale). Hard-codes inputs `nihao nisha nihaoa`. |

### ORC-05 — tools/oracle/image-tag.sh

| Field | Value |
|---|---|
| Entry point | Executed: `image-tag.sh` (no args); prints one line. |
| Owning subsystem | oracle provisioning |
| Purpose | Derives the immutable published-image tag `<libpinyin_commit[0:12]>-<base digest[0:12]>-<apt_snapshot>-<sha256(Dockerfile.oracle ‖ build-oracle.sh)[0:12]>` so that any input that can change the library hashes changes the tag. Current value at origin/main: `074a2219c90f-dab11cdb0a9d-20260831T000000Z-fb1afc111b20`. |
| Implementation | bash; 41 lines; sed, grep, cat, sha256sum, cut. |
| Inputs | `tools/oracle/oracle-pin.txt` fields `libpinyin_commit`, `base_image` (must contain `@sha256:`), `apt_snapshot`; `tools/oracle/Dockerfile.oracle` (grep -F for the base_image and snapshot strings; then hashed); `tools/oracle/build-oracle.sh` (hashed). No env. |
| Outputs | Stdout: the tag. Exit 1 with `FAIL:` on missing fields or Dockerfile disagreement. |
| Platform | Portable shell. |
| Consumers | .github/workflows/oracle-image.yml (`tag=$(tools/oracle/image-tag.sh)`), tools/cloud/setup.sh (`published=ghcr.io/shenghaoc/oxpinyin-oracle:$(…)`), docs/runbooks/oracle.md §2a (`img=…:$(tools/oracle/image-tag.sh)`), docs/runbooks/cloud.md. |
| State | None. |
| Overlap | cloud/setup.sh separately derives a *different* short hash for its deps image tag (`sha256sum Dockerfile.oracle \| cut -c1-12`), not via this script. The `field()`/`pinval()` `sed -n "s/^KEY=//p"` idiom is repeated in oracle-image.yml, setup.sh, check-reproducible.sh, verify-oracle-image.sh, build-oracle.sh. |
| Constraints | Comment: the tag "is never pushed twice"; the workflow enforces immutability by refusing to overwrite. The hash covers the two files' *bytes*, so a comment-only edit to either file retags the image. |

### ORC-06 — tools/oracle/nightly-fixture-differentials.sh

| Field | Value |
|---|---|
| Entry point | Executed: `nightly-fixture-differentials.sh [<work-dir>]` (default `/tmp/oxpinyin-fixture-differentials`). |
| Owning subsystem | ci (the CI tier of the oxpinyin↔libpinyin differentials) |
| Purpose | What can the pinned libpinyin source plus COMMITTED fixtures prove in CI with no model20? Builds the pin (Tkrzw) with the toned fixture seeded as its data, exports the same model with oxpinyin-datagen (tkrzw), then runs the ngseg live-parity, KMM (4 gates), spseg live-parity and the drop-in differential. |
| Implementation | bash; 205 lines; git, autoreconf, make, cargo, grep/sed; needs libtkrzw-dev, glib dev, autotools (the verify-nightly job apt-installs them). |
| Inputs | Pin constants `sed`-read from `tools/oracle/build-oracle.sh` (`LIBPINYIN_VERSION`, `LIBPINYIN_SHA`, `LIBPINYIN_GIT_URL`). `fixtures/datagen-toned/` (`*.table`, `interpolation2.text`), `fixtures/w3/tkt/` (must have `backend=tkt` in `datagen-manifest.txt`). Network: github.com. Env read: `LD_LIBRARY_PATH` (prepended). Env exported for the tests: `PINYIN_NGSEG`, `PINYIN_NGSEG_DATA`, `PINYIN_EXPORT_DIR`, `PINYIN_MODEL_DIR`, `PINYIN_GEN_NGRAM_DATA`, `PINYIN_SPSEG`, `PINYIN_GEN_KMM`, `PINYIN_EXPORT_KMM`, `PINYIN_MERGE_KMM`, `PINYIN_VALIDATE_KMM`, `PINYIN_KMM_TO_INTERP`. |
| Outputs | Stdout: `pin: libpinyin <ver> @ <sha>`, `== <title> ==` per suite, `FAILED: <title>`, final `RESULT: at least one differential suite FAILED` (exit 1) or `fixture-tier differentials: OK` (exit 0). Exit 2 on missing pin constants / fixture / installed data file / wrong-backend manifest / pin mismatch. Uses `::warning::` GitHub annotation on the build retry. No 77. |
| Platform | Linux (autotools, `/opt/libpinyin-tkrzw` install, `nproc`). Runs in CI's `debian:testing` container. |
| Consumers | `.github/workflows/verify-nightly.yml` job `differentials` ("Run the fixture-tier differentials"). It calls `tools/datagen/libpinyin-drop-in-differential.sh tkrzw $model $work/drop-in`. |
| State | Work dir `$work/libpinyin` (checkout reused across runs, refreshed when `.pin-ok` marker != pin SHA), `$work/export-tkrzw` (reused when `datagen-manifest.txt` exists), `$work/drop-in`. Install prefix hard-coded `/opt/libpinyin-tkrzw` (comment: not configurable because libpinyin-drop-in-differential.sh hard-codes it). One retry of `make install` after `rm -f data/bigram.db` (the import_interpolation assertion flake). Spawns cargo: `cargo run -p oxpinyin-datagen --no-default-features --features tkrzw -- compile --backend tkrzw`, `cargo test -p oxpinyin-training --features tkrzw --test segment-differential/segment-spseg-mergeseq`, `cargo test -p oxpinyin-kmm --test differential`. |
| Overlap | Re-implements build-oracle.sh's fetch-by-SHA + autoreconf/configure/make install (without the reproducibility flags, without a prefix manifest). The `run_suite` accumulator pattern and the `feat=(--no-default-features --features tkrzw)` array and the `backend=tkt` manifest grep duplicate run-differentials.sh. The `PINYIN_*` tool-path exports are a subset of run-differentials.sh's list. |
| Constraints | store-backends.yml policy: no CI job may download model20 (header cites it). Excluded suites are excluded "by mechanism, not judgment" and listed in the header (KMM prune SIGSEGV, mergeseq token outside w3, lambda/counter/ngseg goldens are model20-space, eval needs consistent interpolation2.text). Hard-pinned to tkrzw on both sides (the workspace default moved to Berkeley DB 2026-09-20). |

### ORC-07 — tools/oracle/oracle-pin.txt (record)

| Field | Value |
|---|---|
| Entry point | Not executed; a `key=value` record with `#` comments, parsed with `sed -n "s/^KEY=//p"` (shell) or `dict(line.split('=',1))` (python). |
| Owning subsystem | oracle provisioning (the provisioning pin record) |
| Purpose | The composite pin identity for the locally built oracle: every field mirrors a constant in build-oracle.sh; runners verify the two sides agree before trusting a prefix. Since v4 it also records the *recipe's* library hashes per cell and the per-cell stable data-manifest digests. |
| Implementation | Text; 66 lines (about half comments recording schema history). |
| Inputs | Hand-maintained; updated in the same commit as its owner file when a pin component, the base image, the apt snapshot or the recipe changes. |
| Outputs | Fields (schema `oracle-provisioning-pin-v4`): `libpinyin_tag=2.11.92`, `libpinyin_commit=074a2219…`, `ibus_libpinyin_tag=1.16.5`, `ibus_libpinyin_commit=2d2cdac0…`, `model_archive_sha256=59c68e89…`, `dbm=BerkeleyDB`, `platform=linux/amd64`, `base_image=debian:testing@sha256:dab11cdb…`, `apt_snapshot=20260831T000000Z`, `source_date_epoch=1788427579`, `libpinyin_sha256_{bdb,kc,tkrzw}`, `libzhuyin_sha256_{bdb,kc,tkrzw}`, `data_manifest_sha256_{bdb,kc,tkrzw}`. **Schema history (from the file's comments):** v2 (2026-09-06) dropped `libpinyin_archive_sha256` (libpinyin verified by commit SHA; no tag carries 2.11.92); **v3 (2026-09-07, #369) dropped `ibus_libpinyin_archive_sha256` so both upstreams are verified symmetrically by commit SHA, the 1.16.5 tag retained as the release-line name only**; v4 (2026-10-16, #818) added platform/base_image/apt_snapshot/source_date_epoch and the per-cell library and data-manifest hashes. libtkrzw is deliberately NOT pinned here (rolling build env). |
| Platform | n/a (hashes are linux/amd64 only, stated in the file). |
| Consumers | **Readers of this record:** image-tag.sh (`libpinyin_commit`, `base_image`, `apt_snapshot`); oracle-image.yml (same + all six `*_sha256_*` into image labels); cloud/setup.sh (`base_image`, `platform`, six lib hashes, three `data_manifest_sha256_*`; also bind-mounts the file to `/usr/local/bin/oracle-pin.txt` so build-oracle.sh's `--regen-data` finds it beside itself); check-reproducible.sh and verify-oracle-image.sh (six lib hashes); build-oracle.sh `--regen-data` (`data_manifest_sha256_<dbm>`); tools/packaging/pc-version.sh (`libpinyin_tag` → `.pc` Version), tools/packaging/check-pc-version.sh, crates/oxpinyin-capi/build_pin_version.rs (`libpinyin_tag` vs `[package.metadata.capi]`); tools/bisection/contract-diff.py and gcov_measure.py / internal-ledger.test.py (recompose `libpinyin-{libpinyin_tag}-{libpinyin_commit}+model20-{model_archive_sha256}+dbm-<cell>` and compare with a prefix's `pin_ref`); runners.json `register:oracle-pin.txt` input entries. **Not** read by tools/bisection/oracle-cell.sh (it reads build-oracle.sh instead). |
| State | Committed; frozen between pin bumps. |
| Overlap | The same identity lives as constants in build-oracle.sh, in `crates/pinyin-oracle/src/pin_ref.rs::EXPECTED_PIN_REF` (tkrzw), `crates/oxpinyin-core/tests/zhuyin_parse_parity.rs::EXPECTED_PIN_REF`, `tools/capture/run-capture.sh::EXPECTED_PIN_REF`, literal `pin_ref=libpinyin-2.11.92-074a2219…` greps in run-candidate-assembly/locale/nbest-choose-train/open-counter/scheme/session-fork/two-context/user-file-modes-diff.sh, and `fixtures/w3/pin-ref.txt`. **Distinct from the prefix manifest** `<prefix>/oracle-pin.txt` (schema `pinyin-oracle-v1`, written by build-oracle.sh; fields `pin_ref`, `libpinyin_tag`, `libpinyin_commit`, `ibus_libpinyin_tag`, `ibus_libpinyin_commit`, `model_sha256`, `dbm=<ConfigureName>`, `header_sha256`, `shared_object_sha256`, [`zhuyin_shared_object_sha256`], [`data_manifest_sha256`, `data_unstable_manifest_sha256`], [`patches_manifest_sha256`]) — same file name, different schema, different key names (`model_archive_sha256` vs `model_sha256`). |
| Constraints | `pin_ref` derivation (build-oracle.sh): `libpinyin-$LIBPINYIN_VERSION-$LIBPINYIN_SHA+model20-$MODEL_SHA256+dbm-$dbm` where `$dbm` is the flag spelling (`bdb`/`kc`/`tkrzw`, not the configure name), plus `+patches-<sha256 of oracle-patches.sha256>` when `--apply-patches` was used. A pin change requires a dedicated human-reviewed PR (docs/testing/oracle-environment.md "Oracle boundary"). The v4 hashes are recipe outputs, not one host's build; "Do not edit the recorded values to match" (cloud.md Failure modes). |

### ORC-08 — tools/oracle/patches/fullpin-aux-overread.patch

| Field | Value |
|---|---|
| Entry point | Applied by `build-oracle.sh --apply-patches tools/oracle/patches` (`patch -p1 --forward --no-backup-if-mismatch`, every `*.patch` in the dir, sorted). |
| Owning subsystem | oracle provisioning (patched-oracle variant) |
| Purpose | Fixes a heap over-read in upstream `pinyin_get_full_pinyin_auxiliary_text` (`src/pinyin.cpp` @3415 region) by clamping `len` to `strlen(pinyin)` in the `g_strdup(pinyin + len)` call, so the LUOMA/SECONDARY_ZHUYIN PARSE_AUX differential can compare against an oracle that does not inject garbage bytes. |
| Implementation | Unified diff, 32 lines (1 hunk, 1 line changed) with a prose header (reproduction: docs/findings/full-pinyin-aux-overread.md). |
| Inputs | The pinned libpinyin tree. |
| Outputs | A patched prefix whose `pin_ref` ends in `+patches-<sha256>` and which carries `oracle-patches.sha256` and `patches_manifest_sha256=`. |
| Platform | n/a |
| Consumers | tools/bisection/run-scheme-diff.sh (for `full` scheme args 2 or 3: requires `$PINYIN_PATCHED_ORACLE_PREFIX` (default `$HOME/.local/opt/pinyin-oracle-patched`), recomputes the patch-dir manifest digest the same way build-oracle.sh does and greps `$EXPECTED_PIN_REF+patches-<digest>`; SKIP 77 otherwise). docs/findings/oracle-pin-074a221-verification.md. |
| State | None. |
| Overlap | A second patch set lives at `tools/bisection/patches/bigram-export-strjoinv/` (selected per runner via `oracle_cell_resolve_prefix` in oracle-cell.sh; prefix `<base>-<variant>`); run-residue-a-tail-diff.sh checks `+patches-` and `patches_manifest_sha256=` the same way. The digest recipe (`find -maxdepth 1 -name '*.patch' -print0 \| sort -z \| xargs -0 sha256sum \| sha256sum`) is written out in build-oracle.sh, run-scheme-diff.sh, oracle-cell.sh. |
| Constraints | Header: authored against 2.11.91 @0c5e80e1, re-verified to apply cleanly at 074a2219 on 2026-09-06 UTC. A patched build must use its own `--prefix` (build-oracle.sh enforces). The patched oracle is used only for the two romanised full-pinyin schemes; every other scheme uses the unpatched pin (run-scheme-diff.sh comment). |

### ORC-09 — tools/oracle/run-differentials.sh

| Field | Value |
|---|---|
| Entry point | Executed: `run-differentials.sh --libpinyin DIR --data DIR --export DIR [--model DIR]`; `-h` prints the `#` header. |
| Owning subsystem | pinyin-oracle / training differentials (developer-machine tier) |
| Purpose | Runs the env-gated `#[ignore]` oxpinyin↔libpinyin differential tests (KMM, spseg/mergeseq, ngseg, lambda, counter, eval) against a *build tree* of the pin (not an installed prefix) with the tkrzw backend, after validating every path so a typo cannot make the tests "skip" and look green. |
| Implementation | bash (`set -uo pipefail`, deliberately not `-e`); 228 lines; cargo, grep. |
| Inputs | `--libpinyin`: built tree with `src/.libs/libpinyin.so.15` and 16 util binaries under `utils/{segment,training,storage}/`; `--data`: dir with `table.conf`, `phrase_index.bin`, `pinyin_index.bin` (+ optional `bigram.db`, `evals2.text`, `interpolation2.text` for the eval gate); `--export`: oxpinyin-datagen output with `datagen-manifest.txt` containing `backend=tkt` (else exit 2); `--model`: optional model20 dir (`interpolation2.text`). Env read: `LD_LIBRARY_PATH` (prepended with `$L/src/.libs`). Env exported: `PINYIN_SPSEG`, `PINYIN_MERGESEQ`, `PINYIN_NGSEG`, `PINYIN_GEN_KMM`, `PINYIN_EXPORT_KMM`, `PINYIN_MERGE_KMM`, `PINYIN_PRUNE_KMM`, `PINYIN_VALIDATE_KMM`, `PINYIN_KMM_TO_INTERP`, `PINYIN_GEN_BINARY_FILES`, `PINYIN_GEN_UNIGRAM`, `PINYIN_GEN_NGRAM`, `PINYIN_GEN_DELETED_NGRAM`, `PINYIN_ESTIMATE_INTERPOLATION`, `PINYIN_EXPORT_INTERPOLATION`, `PINYIN_EVAL_CORRECTION_RATE`, `PINYIN_GEN_NGRAM_DATA`, `PINYIN_NGSEG_DATA`, `PINYIN_EXPORT_DIR`, `PINYIN_MODEL_DIR`, and when the eval gate is wired `PINYIN_EVAL_DATA`, `PINYIN_EVAL_INTERPOLATION2`, `PINYIN_EVAL_TABLE_CONF`, `PINYIN_EVAL_PINYIN_INDEX`, `PINYIN_EVAL_PHRASE_INDEX`. |
| Outputs | Stdout: `== <title> ==` per suite, filtered test output (`report()` greps `live parity\|parity:\|value-identical\|skipping\|test result\|diverges\|stale\|panicked\|assertion\|left:\|right:`), `FAILED: <title>`, `RESULT: every differential suite passed` / `RESULT: at least one differential suite FAILED`. Exit 0/1; 2 for missing args/paths/wrong backend. Eval gate prints `skipping: needs …` (not a failure) when `bigram.db`/`evals2.text` are absent. No 77. |
| Platform | Linux (pin build tree, `.so.15`). |
| Consumers | docs/runbooks/oracle.md §4, docs/testing/README.md ("pass `--include-ignored`"), nightly-fixture-differentials.sh header (refers to it as the developer-run wiring). Not in runners.json. |
| State | None of its own; spawns `cargo test -p oxpinyin-kmm --test differential`, `cargo test -p oxpinyin-training --no-default-features --features tkrzw --test {segment-spseg-mergeseq,segment-differential,lambda-differential,counter-differential,eval-differential} -- --include-ignored --nocapture`. No timeouts. |
| Overlap | nightly-fixture-differentials.sh (same env wiring, subset of suites, committed fixtures); `run_suite` accumulator pattern shared. Its header carries a third hand-written copy of the data-build recipe (`gen_binary_files` → `import_interpolation` → `gen_unigram`), also in libpinyin-drop-in-differential.sh and build-oracle.sh's `make -C data`. |
| Constraints | Hard-pinned tkrzw on every side (header). Header comment is partly stale: says "when the release tarball is reachable, build-oracle.sh is canonical" (the recipe has fetched by commit SHA since 2026-09-06) and describes the oracle as "Tkrzw backend" (the default is bdb since 2026-09-27). Documents the intermittent `import_interpolation` `insert_freq` assertion. |

### ORC-10 — tools/oracle/transformed-options-sweep.py

| Field | Value |
|---|---|
| Entry point | Executed: `python3 transformed-options-sweep.py --oracle LIB --subject LIB --data DIR --expect parent\|fixed [--list-differences] [--cursor-family pinyin\|zhuyin] [--jobs N] [--lane-j-force] [--lane-j-session]`. Re-invokes itself as `--worker LIB DATA WORD CASES_JSON` and `--cursor-worker LIB DATA WORD CASES_JSON FAMILY` subprocesses. |
| Owning subsystem | bisection (gate `transformed-options-sweep`, issues #634/#585/#626/#641/#688/#690/#691) |
| Purpose | Does the subject's candidate/sentence/cursor surface match the pin byte-for-byte across 470 option words × 11 inputs (5,170 paired observations per cell)? Replaces the unavailable `/home/sheng/audit-r2-scratch` harness. |
| Implementation | Python 3 stdlib only (ctypes, json, subprocess, concurrent.futures, os.fork, tempfile, signal); 347 lines. Loads the two libraries with `ctypes.CDLL` and `libglib-2.0.so.0` for `g_free`. |
| Inputs | `transformed-options-inputs.json`, `transformed-options-words.txt` (beside the script); `--oracle` `libpinyin.so` (pin prefix), `--subject` `libpinyin_capi.so` (or `libzhuyin*` for `--cursor-family zhuyin`), `--data` (the pin cell's `lib/libpinyin/data`). No env vars read. |
| Outputs | Stdout: JSON summary `{"words":470,"inputs":11,"schemes":{…ordinary/complete counts…}}`, one JSON line per targeted/lane-J/cursor check; stderr: progress every 50 words, `worker failed: …`. Exit 0 on all `assert`s passing; `AssertionError` traceback (exit 1) on count movement; `SystemExit(1)` on a worker crash or 600 s timeout. Writes no captures or logs. |
| Platform | Linux (dlopen of the `.so`s, `os.fork`, glib). |
| Consumers | tools/bisection/runners.json entry `transformed-options-sweep` (kind gate, cells bdb/kc/tkrzw, timeout 3600 s, command `python3 {root}/tools/oracle/transformed-options-sweep.py --oracle {prefix}/lib/libpinyin.so --subject {capi} --data {prefix}/lib/libpinyin/data --expect fixed`), run via run-all.py; docs/findings/tooling-lane-gates.md, bug-for-bug-audit-r2-2026-09-23.md. |
| State | Fresh `tempfile.TemporaryDirectory(prefix="lane-j-sweep-"/"lane-j-cursor-")` user profile per worker, removed on normal completion; one subprocess per (library, word) with `timeout=600`; `ThreadPoolExecutor(max_workers=jobs)`; cursor calls isolated by `os.fork` + pipe to survive the pin's `assert` SIGABRT. No locks. |
| Overlap | tools/bisection/contract-diff.py and run-option-sweep.sh also drive both libraries over option words via a C-ABI; this one is the Python/ctypes variant. |
| Constraints | Expected counts are frozen in code: `double-ms` 29 / `chewing-standard` 31 ordinary differing words for `--expect parent`, 0 for `fixed`; Hanyu/Luoma/secondary 0; complete-protocol counts 0 (Hanyu 0 since #651). Cursor family: pin SIGABRT must map to subject `false` + exactly one `WARNING` (class (c)). The .md says "retain [cursor output] outside the repository as evidence". |

### ORC-11 — tools/oracle/transformed-options-inputs.json, transformed-options-words.txt, transformed-options-sweep.md

| Field | Value |
|---|---|
| Entry point | Data read by transformed-options-sweep.py at import (`CASES`, `WORDS`); the .md is the human spec. |
| Owning subsystem | bisection (fixtures for the sweep gate) |
| Purpose | The frozen input lists: 11 `[scheme, scheme-number, text]` cases (Hanyu `sh`, `lishbakua`; Luoma `lishihbakua`, `chih`, `rih`, `sih`, `zih`, `shih`; MS double `nihk`; STANDARD chewing `su3cl3`; secondary `tsz`) and 470 hex `uint32` option words (34 singles incl. 0 and 0xffffffff, 435 two-bit pairs over bits 1–19 and 21–31, plus `0x0000018a` the parity profile). |
| Implementation | JSON (13 lines), text (470 lines, one `0x%08x` per line, sorted, deduplicated), Markdown (90 lines). |
| Inputs | Hand-authored. |
| Outputs | n/a |
| Platform | portable |
| Consumers | transformed-options-sweep.py only (runners.json lists "committed transformed-options input and word files" as a required input). |
| State | Committed. |
| Overlap | None. |
| Constraints | The .md states its counts are **not comparable** with the lost audit harness (104/435/496/149) and that bits 0 and 20 are reserved and excluded from pairs. Changing either list moves the frozen expected counts in the .py. |

### ORC-12 — tools/oracle/user-dir-round-trip.sh

| Field | Value |
|---|---|
| Entry point | Executed: `user-dir-round-trip.sh <oracle-prefix> [input ...]` (default inputs `nihao nisha`). |
| Owning subsystem | bisection (gate `user-dir-round-trip`, drop-in task 9, #599) |
| Purpose | Does the user directory round-trip seamlessly between the pin and oxpinyin on one KV backend? Phase A pin trains; B oxpinyin load→save in place; C pin re-renders byte-identically; D oxpinyin trains its own profile and the pin reads every remembered phrase; then the #599 system-import round trip. |
| Implementation | bash; 172 lines; cc + pkg-config + glib-2.0 to build `user_driver.c`; cargo (or a prebuilt test binary); sources `tools/bisection/oracle-cell.sh`; execs `tools/bisection/run-system-import-round-trip.sh`. |
| Inputs | Prefix: `oracle-pin.txt`, `lib/libpinyin.so`, `lib/libpinyin.so.15*`, `include/libpinyin-*/`, `lib/libpinyin/data/`. Reads the prefix's `dbm=` (BerkeleyDB/KyotoCabinet/Tkrzw) to pick the cargo feature. Env read: `TMPDIR`, `OX_CARGO_FEATURES` (overrides `--no-default-features --features $CAPI_FEATURE`), `OXPINYIN_USER_RT_TEST` (prebuilt `user_dir_round_trip` test executable), `OXPINYIN_CAPI_SO` (requires `OXPINYIN_USER_RT_TEST`; else FAIL), `PINYIN_ORACLE_DBM` (defaulted from the prefix's dbm, `kyotocabinet`→`kc`); via oracle-cell.sh: `CARGO_TARGET_DIR`, `OXPINYIN_TARGET_ROOT`, `OXPINYIN_ZHUYIN_SO`. Env set for the Rust test: `OX_SYSTEM_DIR`, `OX_PIN_DIR`, `OX_OWNED_DIR`, `OX_INPUTS`, `OX_EXPECTED_PHRASES`, `REPO_ROOT`. |
| Outputs | Stdout phases `== Phase A/B/C/D ==`, `IDENTICAL: … (N rows)`, `READABLE: … (rows, phrase rows)`; stderr `DIVERGED: …`. Exit 77 `SKIP: missing oracle` when the prefix lacks `oracle-pin.txt`/`libpinyin.so`; 1 on any divergence/empty dump/no bigram rows/unknown dbm; final status from run-system-import-round-trip.sh. |
| Platform | Linux (cc against the pin's headers, rpath, `.so.15`). |
| Consumers | runners.json `user-dir-round-trip` (gate, cells bdb/kc/tkrzw, `oracle: true`, timeout 900, command `bash {root}/tools/oracle/user-dir-round-trip.sh {prefix}`, inputs `{capi}` and `{env[OXPINYIN_USER_RT_TEST]}`); run-all.py sets `OXPINYIN_USER_RT_TEST` and `OXPINYIN_BUILD_USER_RT`; docker-user-rt.sh; crates/oxpinyin-runtime/tests/user_dir_round_trip.rs (its Rust half), crates/oxpinyin-store/src/lib.rs (doc ref), docs/findings/user-store.md, compatibility-policy.md, docs/packaging.md, ROADMAP.md. |
| State | `work=$(mktemp -d "${TMPDIR:-/tmp}/oxpinyin-user-rt.XXXXXX")` with `trap rm -rf` (driver binary, `pin/`, `ox/`, dumps, `diff.txt`, `expected-phrases`). Spawns `cargo test --locked --profile dev -p oxpinyin-runtime <features> --test user_dir_round_trip -- --ignored --nocapture <name>` twice unless a prebuilt test is given; `oracle_cell_artifact OXPINYIN_CAPI_SO libpinyin_capi.so oxpinyin-capi` may `cargo build`. No timeout. |
| Overlap | The dbm→feature mapping duplicates oracle-cell.sh (which it then sources, mapping the *other* direction). Phase D uses `user_driver phrases` only because of the pin's class-(b) bigram-export UAF (compatibility-policy row 1). |
| Constraints | `OX_CARGO_FEATURES` MUST match the prefix's `--with-dbm` (comment: "the seamless claim is per KV backend"). Asserts the pin dump has `^B` rows (guards against the 939400c6 driver regression). Does not check `pin_ref` (only presence of `oracle-pin.txt`); unlike most runners it accepts any cell. |

### ORC-13 — tools/oracle/user_driver.c

| Field | Value |
|---|---|
| Entry point | Compiled ad hoc by user-dir-round-trip.sh (`cc -O2 … -I<prefix>/include/libpinyin-* $(pkg-config glib-2.0) -L<prefix>/lib -lpinyin -Wl,-rpath,…`); run as `user_driver train\|dump\|phrases <system-dir> <user-dir> [input...]`. |
| Owning subsystem | bisection (pin-side driver for the user-dir round trip) |
| Purpose | Drives the *pin* exactly as ibus does (parse → guess_sentence → per-cursor guess_candidates with `SORT_BY_PHRASE_LENGTH\|SORT_BY_PINYIN_LENGTH\|SORT_BY_FREQUENCY` → choose the first NORMAL_CANDIDATE → re-guess → `pinyin_train(instance,0)` → `pinyin_remember_user_input` → `pinyin_reset`), saves, and dumps the §9 exports as `P\t<phrase>\t<pinyin>\t<count>` / `B\t…` lines. |
| Implementation | C (C17 with explicit `<stdbool.h>` because the pin's `pinyin.h` lacks it under glib ≥ 2.84); 283 lines; links libpinyin + glib. |
| Inputs | argv: mode, system dir, user dir, inputs. No env. |
| Outputs | Stdout: export rows (bigram count is the stored count × 2, the pin's rendering). Exit 0; 1 on any API failure (`parse failed`, `guess failed`, `no normal candidate`, `choose failed`, `train failed`, `remember failed`, `save failed`); 2 usage. |
| Platform | Linux (pin ABI). |
| Consumers | user-dir-round-trip.sh only. Referenced by tools/bisection/gcov_measure.py and open-counter-diff.c (comments), docs/findings/berkeleydb-backend.md, user-store.md, .kiro/specs/drop-in/tasks.md, ROADMAP.md. |
| State | None. |
| Overlap | Other pin drivers in tools/bisection (`train-diff.c`, `open-counter-diff.c`, `system-import-profile.c`) re-implement the same ibus call order; this one is the documented reference for why candidate 0 (n-best) must be skipped (`pinyin.cpp:1934`, `:2515-2521`, `phonetic_lookup.h:866`). |
| Constraints | Upstream citations by file:line at the pin; `phrases` mode exists to avoid the registered class-(b) use-after-free in the pin's bigram export iterator (`pinyin.cpp:842-872`). Sort option must not be 0 (comment: rare single characters' grams segfault the BDB export). |

### ORC-14 — tools/oracle/verify-oracle-image.sh

| Field | Value |
|---|---|
| Entry point | Executed: `verify-oracle-image.sh [--engine docker\|podman] IMAGE`. |
| Owning subsystem | ci / oracle provisioning |
| Purpose | Is a built oracle image the recipe's output and free of model data? Copies `/opt/pinyin-oracle` out of the image and checks `libpinyin.so`/`libzhuyin.so` of bdb, kc, tkrzw against `oracle-pin.txt`, then fails if any `lib/libpinyin/data` dir or `*.bin`, `*.table`, `*.text`, `bigram.db` exists under it. |
| Implementation | bash; 78 lines; docker/podman (`create … /none` because the image is FROM scratch, `cp`, `rm`), sha256sum, find. |
| Inputs | IMAGE; `tools/oracle/oracle-pin.txt` (`<lib>_sha256_<dbm>`). No env. |
| Outputs | Stdout `<dbm> <lib> <sha256>` × 6; stderr `FAIL: … records …`, `FAIL: model-derived data in the image:` + list. Exit 0/1; 2 usage; 1 no engine. |
| Platform | Container host (`--platform linux/amd64` on create). |
| Consumers | .github/workflows/oracle-image.yml step "Verify the library hashes and the absence of model data" (`--engine docker oxpinyin-oracle:candidate`). |
| State | `scratch=$(mktemp -d)` + container id, both removed by the EXIT trap. |
| Overlap | Same six-hash check as check-reproducible.sh and cloud/setup.sh (`libs_ok`). cloud/setup.sh extracts cells from the published image with the same `docker create … /none` + `docker cp` idiom; docs/runbooks/oracle.md §2a repeats it for humans. |
| Constraints | Enforces docs/findings/model-provenance.md ("no redistribution") on the published artifact. |

### ORC-15 — tools/cloud/setup.sh

| Field | Value |
|---|---|
| Entry point | Executed from the checkout: `tools/cloud/setup.sh [--cache-dir DIR] [--cells "bdb kc tkrzw"] [--jobs N] [--no-pull]`; `--help` prints the header via awk. Idempotent. |
| Owning subsystem | oracle provisioning (cloud host) |
| Purpose | Brings a Claude Code cloud session (root container behind a TLS-intercepting proxy) to the state every gate expects: valgrind, dockerd, the pinned base image, the three oracle cells (pulled or built, hash-verified), model20 + regenerated data (manifest-verified), and an environment file naming the prefixes. |
| Implementation | bash; 360 lines; docker (daemon + BuildKit), apt-get, sudo (non-root), update-ca-certificates, setsid/nohup, sha256sum, sed, nproc. Calls tools/oracle/image-tag.sh, tools/model/fetch-model.sh, tools/oracle/build-oracle.sh (in-image). |
| Inputs | `tools/oracle/oracle-pin.txt` (`base_image`, `platform`, six `*_sha256_*`, three `data_manifest_sha256_*`), `tools/oracle/Dockerfile.oracle`, `tools/oracle/build-oracle.sh`, `tools/oracle/oracle-pin.txt` (bind-mounted into the container beside build-oracle.sh). Network: docker.io, ghcr.io, snapshot.debian.org, github.com, downloads.sourceforge.net. Env read: `OXPINYIN_CLOUD_CACHE`, `XDG_CACHE_HOME`, `HOME`, `HTTPS_PROXY`/`https_proxy`, `NO_PROXY`/`no_proxy`, `OXPINYIN_PROXY_CA` (default `/root/.ccr/ca-bundle.crt`; single-cert `/root/.ccr/agent-proxy-ca.crt` hard-coded), `OXPINYIN_ORACLE_ENV` (env-file path), `EUID`, `SECONDS`. |
| Outputs | Stderr `[ Ns] …` progress; stdout one line per cell `<dbm> prefix=<dir> libpinyin=<sha> libzhuyin=<sha> data manifest OK`. Writes `~/.oxpinyin-oracle.env` (exports `PINYIN_MODEL_DIR`, `OXPINYIN_ORACLE_PREFIX_<CELL>`, `OXPINYIN_ORACLE_DATA_<CELL>`, `PINYIN_ORACLE_DBM` default, and recomputes `PINYIN_ORACLE_PREFIX` + `OXPINYIN_LIBPINYIN_DATA_DIR` on every `source`; bash-only). Logs `$cache/build-<dbm>.log`, `$cache/data-<dbm>.log`, `/var/log/dockerd.log`. Exit 0; **3** on a library-hash MISMATCH (libraries left in place for readelf/cmp); 1 (`FAIL:`) on everything else; 2 usage/unknown cell. |
| Platform | Linux root container, linux/amd64 (pulls/builds with `--platform`), docker required. |
| Consumers | AGENTS.md ("Cloud sessions are a supported host for every gate: provision one with `tools/cloud/setup.sh`"), docs/runbooks/cloud.md, benches.md (valgrind), oracle.md §2a. Not in runners.json. |
| State | Cache `${OXPINYIN_CLOUD_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/oxpinyin}` with `model20/` (fetch-model cache) and `oracle/<dbm>/` (prefixes, mounted at `/opt/pinyin-oracle` in the container). Docker images: `$base_image`, `oxpinyin-oracle-deps:<Dockerfile hash12>`, published image. `retry` backoff 2/4/8/16 s for 429s; dockerd readiness wait 60 × 1 s. Rewrites each cell's `.pc` `prefix=` to the host path. No locks; a second run re-verifies only (0.33 s measured). |
| Overlap | Duplicates check-reproducible.sh's proxy/CA flag assembly, deps-image build, in-image `build-oracle.sh --libs-only --enable-libzhuyin` invocation, and the six-hash compare; duplicates verify-oracle-image.sh's `docker create /none` + `docker cp` extraction; duplicates oracle.md §2a's manual steps (pull, cp, sed .pc, hash check, `--regen-data`). |
| Constraints | A pulled image is trusted only as far as its hashes match the pin (a non-matching cell is discarded and rebuilt). Exit 3 on MISMATCH "is the result to report" (do not edit the record). Published image name `ghcr.io/shenghaoc/oxpinyin-oracle` is hard-coded here and in the workflow and oracle.md. The env file header says "source it, do not edit it". Lanes run it only when a task needs the pin oracle (cloud.md); the environment's own setup script stays empty. |

### ORC-16 — tools/model/fetch-model.sh

| Field | Value |
|---|---|
| Entry point | Executed: `fetch-model.sh [--cache-dir DIR] [--url URL] [--archive PATH] [--force] [-h]`. |
| Owning subsystem | oracle provisioning (model cache) |
| Purpose | Downloads the checksum-pinned `model20.text.tar.gz` into a gitignored cache, verifies SHA-256, extracts the 18 files atomically, and writes a provenance marker. Idempotent. The only sanctioned way model bytes reach a machine (local-only; no CI). |
| Implementation | bash; 261 lines; curl (unless `--archive`), tar, sha256sum or shasum, git (ignore check), mktemp. |
| Inputs | Env: `PINYIN_MODEL_CACHE` (default `<repo>/target/model20`), `PINYIN_MODEL_URL` (default the pinned SourceForge URL). Network: downloads.sourceforge.net + a rotating `*.dl.sourceforge.net` mirror. |
| Outputs | `<cache>/downloads/model20.text.tar.gz`, `<cache>/extracted/{18 files}`, `<cache>/verified` (`schema=pinyin-model-cache-v1`, `url=`, `sha256=`). Last stdout line = absolute extracted dir; `verified model cache present; skipping fetch` on stderr when cached. Exit 0; 1 on download failure, checksum mismatch, missing file in archive, refusing a git-tracked cache path; 2 unknown option. |
| Platform | Portable (Linux/macOS: `shasum` fallback). |
| Consumers | tools/cloud/setup.sh (`--cache-dir "$model_cache"`), tools/oracle/build-oracle.sh (`--model-dir` consumes `extracted/` and requires `../verified`), tools/bisection/Dockerfile.perf-matrix and Dockerfile.callgrind (`RUN bash tools/model/fetch-model.sh`), docker-user-rt.sh (expects its cache), tools/bisection/run-option-sweep.sh, run-perf-baseline.sh, system-dir.sh (error text), tools/profile/run-w8-cycle.sh, crates/oxpinyin-testsupport/src/model_cache.rs and crates/pinyin-oracle (model_cache/tests/model_fetch.rs; benches support), oxpinyin-datagen/corpus/training/capi tests (doc hints), docs/runbooks/oracle.md §1, cloud.md, store-backends.yml header (policy). |
| State | Cache dir must be gitignored or outside the repo (`git check-ignore`); extraction into `mktemp -d "$cache_dir/.extract.XXXXXX"` then `mv` over `extracted/`; partial download `.part` then `mv`. curl `--retry 3 --connect-timeout 30 --max-time 600`. |
| Overlap | `MODEL_URL`, `MODEL_SHA256`, `EXPECTED_FILES` duplicate build-oracle.sh's `MODEL_URL`/`MODEL_SHA256`/`MODEL_FILES` and crates/pinyin-oracle `EXPECTED_MODEL_FILES`; the fetch-and-verify function duplicates build-oracle.sh `fetch()`. |
| Constraints | Header: "Pinned identity (do not alter; from model-provenance.md)"; "No mirror"; "LOCAL-ONLY workflow: no CI job downloads the archive"; "This script does not feed frequencies into the language model." The `verified` marker is what build-oracle.sh's `--model-dir` provenance check requires. |

### ORC-17 — tools/datagen/libpinyin-drop-in-differential.sh

| Field | Value |
|---|---|
| Entry point | Executed: `libpinyin-drop-in-differential.sh <kc\|tkrzw> <model-dir> [<work-dir>]` (default work `/tmp/drop-in-<backend>`). |
| Owning subsystem | fixtures / datagen differential |
| Purpose | Same-backend drop-in differential for oxpinyin-datagen: compile one model dir with libpinyin's own `gen_binary_files` → `import_interpolation` → `gen_unigram` and with `oxpinyin-datagen compile`, then compare through `crates/oxpinyin-datagen/tests/libpinyin_parity.rs` (chunk files byte-exact, DBM rows field-exact) and `cmp` every non-index `*.bin` chunk on disk. |
| Implementation | bash; 91 lines; cargo, cmp; needs an installed pin prefix with `bin/` utils. |
| Inputs | Hard-coded prefixes `/opt/libpinyin-kc` / `/opt/libpinyin-tkrzw` (`bin/gen_binary_files`, `bin/import_interpolation`, `bin/gen_unigram`, `lib/libpinyin/data/table.conf`); `<model-dir>` with `*.table` + `interpolation2.text`. Env exported for the test: `OXPINYIN_LIBPINYIN_DATA_DIR=$theirs`, `PINYIN_MODEL_DIR=$model`. No env read. |
| Outputs | Stdout: `libpinyin (<b>) wrote:` + ls, `oxpinyin (<b>) wrote:` + ls, test output, `<name>: byte-exact` per chunk, `drop-in differential (<b>, <model>): OK`. Exit 0; non-zero from any step (`set -e`); 2 unknown backend. No 77. |
| Platform | Linux (perf-matrix container paths). |
| Consumers | nightly-fixture-differentials.sh (`tkrzw`, toned model → this is why its prefix is fixed at `/opt/libpinyin-tkrzw`); crates/oxpinyin-datagen/src/lib.rs and tests/libpinyin_parity.rs (docs), fixtures/datagen-toned/README.md, docs/findings/datagen-compat-2026-09-01.md. Header shows the `docker run … oxpinyin-matrix:latest` developer use. |
| State | `rm -rf "$work"` then `$work/libpinyin` (pin output; the pin's tools write into the cwd) and `$work/oxpinyin`. Spawns `cargo run --locked -q -p oxpinyin-datagen [--no-default-features --features tkrzw] -- compile --backend <kyotocabinet\|tkrzw>` and `cargo test --locked -p oxpinyin-datagen … --test libpinyin_parity -- --include-ignored --nocapture`. |
| Overlap | Third copy of the data-build recipe (also build-oracle.sh `make -C data`, run-differentials.sh header, oracle.md §2a). The backend→feature/`datagen_backend` case table repeats oracle-cell.sh's. The skipped-chunk list (`pinyin_index.bin`, `phrase_index.bin`, `addon_*`, `punct.bin`) is the DBM-path subset of build-oracle.sh's `DATA_UNSTABLE_FILES` (minus `bigram.db`, which is not `*.bin`). |
| Constraints | No `bdb` arm (only kc/tkrzw), so it cannot run on the reference cell; prefix paths are hard-coded to the perf-matrix layout, not `PINYIN_ORACLE_PREFIX` or the cloud layout `/opt/pinyin-oracle/<dbm>` (which carries no `bin/` utils unless `--regen-data` installed them). "Neither side reads the other's output (the canonical-source invariant)." |

### ORC-18 — .github/workflows/oracle-image.yml

| Field | Value |
|---|---|
| Entry point | GitHub Actions: `workflow_dispatch`; `push` to `main` and `pull_request` to `main` when any of build-oracle.sh, Dockerfile.oracle, oracle-pin.txt, verify-oracle-image.sh, image-tag.sh or the workflow itself changes. One job `oracle-image` (ubuntu-latest, 90 min, `packages: write`). |
| Owning subsystem | ci |
| Purpose | Builds the libs-only oracle image for linux/amd64, verifies it against oracle-pin.txt, and (not on PRs) pushes `ghcr.io/shenghaoc/oxpinyin-oracle:<tag>` with the tag from image-tag.sh, never overwriting an existing tag. |
| Implementation | YAML; 126 lines; actions pinned by SHA: actions/checkout v7.0.1, docker/setup-buildx-action v4.4.1, docker/build-push-action v7.4.0 (`provenance: false`, `load: true`, `context: tools/oracle`), docker/login-action v4.6.0. |
| Inputs | `tools/oracle/oracle-pin.txt` (`libpinyin_commit`, `base_image`, six `*_sha256_*` → OCI labels `io.github.shenghaoc.oxpinyin.*`), image-tag.sh, Dockerfile.oracle, verify-oracle-image.sh, `secrets.GITHUB_TOKEN`, `github.actor`, `GITHUB_REPOSITORY`, `GITHUB_SHA`. |
| Outputs | Local image `oxpinyin-oracle:candidate`; pushed image + labels; `::notice::` when the tag exists; `::error::` + exit 1 when `docker manifest inspect` fails for a reason other than "no such manifest/manifest unknown/not found". |
| Platform | GitHub-hosted Linux runner; image linux/amd64. |
| Consumers | Produces what cloud/setup.sh and oracle.md §2a pull. Referenced by oracle.md. |
| State | `concurrency: group oracle-image-${{ github.ref }}`, `cancel-in-progress: false`. Buildx cache per run only. |
| Overlap | Its "Derive the tag and labels" step re-implements the `field()` sed helper from image-tag.sh/setup.sh for the labels while calling image-tag.sh for the tag. |
| Constraints | Must fetch no model20 (header; store-backends.yml policy). Immutable tags: "a tag that already exists was built from this recipe: keep it, never overwrite". PR #819 "Not verified": the workflow had not run; the first GHCR push creates a *private* package that someone must make public for anonymous pulls; cloud.md (2026-10-16) reports "no image published yet". |

## Count

18 records: ORC-01, ORC-02, ORC-03, ORC-04, ORC-05, ORC-06, ORC-07, ORC-08, ORC-09, ORC-10, ORC-11, ORC-12, ORC-13, ORC-14, ORC-15, ORC-16, ORC-17, ORC-18.

## Architecture notes

Observation unless marked *(interpretation)*.

- **Two files named `oracle-pin.txt`, two schemas.** The committed record `tools/oracle/oracle-pin.txt` is `oracle-provisioning-pin-v4` (keys `model_archive_sha256`, `dbm=BerkeleyDB`, per-cell `*_sha256_<dbm>`); the per-prefix manifest written by build-oracle.sh is `pinyin-oracle-v1` (keys `pin_ref`, `model_sha256`, `dbm=<ConfigureName>`, `header_sha256`, `shared_object_sha256`, `data_manifest_sha256`, …). Parsers of the **record**: image-tag.sh, oracle-image.yml, cloud/setup.sh, check-reproducible.sh, verify-oracle-image.sh, build-oracle.sh (`--regen-data`), tools/packaging/pc-version.sh + check-pc-version.sh, crates/oxpinyin-capi/build_pin_version.rs, tools/bisection/contract-diff.py, gcov_measure.py, internal-ledger.test.py. Parsers of the **prefix manifest**: crates/pinyin-oracle/src/pin.rs (`PinManifest`, requires schema `pinyin-oracle-v1` and `pin_ref == EXPECTED_PIN_REF`) and build.rs, tools/capture/run-capture.sh (pin_ref, shared_object_sha256, data_manifest_sha256), tools/bisection/run-all.py (grep for `EXPECTED_ORACLE_PIN_REF` line), ~25 `tools/bisection/run-*-diff.sh` (`grep -Fxq "$EXPECTED_PIN_REF"` or a literal `^pin_ref=libpinyin-2.11.92-074a2219…`), run-import-diff.sh (`model_sha256`, `dbm=`), run-scheme-diff.sh / run-residue-a-tail-diff.sh (`+patches-` and `patches_manifest_sha256=`), user-dir-round-trip.sh (`dbm=`), tools/bisection/delta_ledger.py, gcov_measure.py, train-index-diff.py, contract-diff.py, tools/generate_w3_fixtures.py. *(interpretation: the record-vs-manifest key drift — `model_archive_sha256` vs `model_sha256`, flag-spelled `+dbm-bdb` vs configure-spelled `dbm=BerkeleyDB` — is a consolidation hazard; any merge of the two files must keep both key sets.)*
- **Pin constants are copied in at least four places**: build-oracle.sh (declared canonical; `sed`-read by oracle-cell.sh and nightly-fixture-differentials.sh), oracle-pin.txt (record), crates/pinyin-oracle/src/pin_ref.rs `EXPECTED_PIN_REF` (+dbm-tkrzw), tools/capture/run-capture.sh `EXPECTED_PIN_REF`, crates/oxpinyin-core/tests/zhuyin_parse_parity.rs `EXPECTED_PIN_REF`, and literal `pin_ref=libpinyin-2.11.92-074a2219…` greps in eight run-*-diff.sh scripts. `fixtures/w3/pin-ref.txt` and the capture fixtures carry `+dbm-tkrzw`.
- **`pin_ref` derivation** (one place, build-oracle.sh): `libpinyin-$LIBPINYIN_VERSION-$LIBPINYIN_SHA+model20-$MODEL_SHA256+dbm-$dbm` (+`+patches-<sha256 of oracle-patches.sha256>`). `$dbm` is the flag spelling (bdb/kc/tkrzw); `dbm=` in the manifest is the configure spelling (BerkeleyDB/KyotoCabinet/Tkrzw). The dbm mapping table appears in build-oracle.sh, oracle-cell.sh, user-dir-round-trip.sh (reverse), libpinyin-drop-in-differential.sh (kc/tkrzw only), crates/pinyin-oracle/build.rs (`PINYIN_BENCH_DBM`).
- **Model identity is copied three times**: `MODEL_URL`/`MODEL_SHA256`/18-file list in build-oracle.sh, fetch-model.sh, crates/pinyin-oracle/src/model_cache.rs (`EXPECTED_MODEL_FILES`); build-oracle.sh's comment says keep in lockstep. The provenance handshake is fetch-model.sh's `<cache>/verified` marker (`sha256=`) which build-oracle.sh `--model-dir` demands one level above the extracted dir.
- **The data-build recipe** (`gen_binary_files --gen-punct-table` → `import_interpolation < interpolation2.text` → `gen_unigram`) is written out in build-oracle.sh (`make -C data`), run-differentials.sh header, libpinyin-drop-in-differential.sh, oracle.md §2a, and nightly-fixture-differentials.sh (via `make install`). The `insert_freq` assertion flake and its "delete bigram.db and retry" workaround appear in run-differentials.sh and nightly-fixture-differentials.sh.
- **The six-hash compare against `<lib>_sha256_<dbm>`** (`sed -n "s/^${lib}_sha256_$dbm=//p"` + `sha256sum | cut -d' ' -f1`) is implemented three times: check-reproducible.sh, verify-oracle-image.sh, cloud/setup.sh (`libs_ok` + final loop). The `docker create --platform linux/amd64 IMAGE /none` + `docker cp` extraction is in verify-oracle-image.sh, cloud/setup.sh and oracle.md §2a. The proxy/CA flag block (`--network host`, `https_proxy`, `GIT_SSL_CAINFO`/`SSL_CERT_FILE`/`CURL_CA_BUNDLE=/run/ca.crt`, `--secret id=cacert`) is in check-reproducible.sh and cloud/setup.sh.
- **Hard-coded backend names / extensions / paths**: `/opt/libpinyin-tkrzw`, `/opt/libpinyin-kc` (libpinyin-drop-in-differential.sh, nightly-fixture-differentials.sh, Dockerfile.perf-matrix); `/opt/pinyin-oracle/<dbm>` (Dockerfile.oracle, setup.sh, verify-oracle-image.sh, oracle.md); `backend=tkt` manifest grep (run-differentials.sh, nightly); `--features tkrzw` arrays (run-differentials.sh, nightly, drop-in); `libpinyin.so.15` (user-dir-round-trip.sh, run-differentials.sh); `libglib-2.0.so.0` (sweep .py); `/tmp/oxpinyin-export` (oracle.md §3, system-dir.sh legacy path); `ghcr.io/shenghaoc/oxpinyin-oracle` (workflow, setup.sh, oracle.md, cloud.md); `$HOME/.local/opt/pinyin-oracle[-patched]` (oracle.md, run-scheme-diff.sh).
- **Env-var names and aliases**: `PINYIN_ORACLE_DBM` (cell selector; oracle-cell.sh, setup.sh env file, user-dir-round-trip.sh), `PINYIN_ORACLE_PREFIX` (runners, pinyin-oracle build.rs, env file), `OXPINYIN_ORACLE_PREFIX_<CELL>` / `OXPINYIN_ORACLE_DATA_<CELL>` (env file only), `OXPINYIN_LIBPINYIN_DATA_DIR` (env file; drop-in differential exports it for the parity test), `OXPINYIN_SYSTEM_DIR` (system-dir.sh sweep-wide; run-all.py sets it to the pin's data), `PINYIN_MODEL_DIR` (oracle.md, run-differentials.sh, nightly, env file, drop-in), `PINYIN_MODEL_CACHE` + `PINYIN_MODEL_URL` (fetch-model.sh only), `OXPINYIN_CLOUD_CACHE`, `OXPINYIN_PROXY_CA`, `OXPINYIN_ORACLE_ENV` (setup.sh), `OX_CARGO_FEATURES`, `OXPINYIN_USER_RT_TEST`, `OXPINYIN_CAPI_SO`, `OX_SYSTEM_DIR`/`OX_PIN_DIR`/`OX_OWNED_DIR`/`OX_INPUTS`/`OX_EXPECTED_PHRASES` (user-dir-round-trip.sh ↔ its Rust test), `PINYIN_BENCH_DBM` (pinyin-oracle build.rs), `PINYIN_PATCHED_ORACLE_PREFIX` (run-scheme-diff.sh), the `PINYIN_<TOOL>` and `PINYIN_EVAL_*` family (run-differentials.sh, nightly). `TMPDIR` is honoured by build-oracle.sh and user-dir-round-trip.sh.
- **Exit-code conventions differ by file**: 0/1/2 with no SKIP (build-oracle.sh, check-reproducible.sh, verify-oracle-image.sh, image-tag.sh, run-differentials.sh, nightly, fetch-model.sh, drop-in); 0/77/1 SKIP-on-missing-oracle (user-dir-round-trip.sh, and the bisection runners it mirrors); 0/1/2/**3** (cloud/setup.sh: 3 = hash mismatch "the result to report"; oracle-cell.sh also exits 3 on a bad `PINYIN_ORACLE_DBM`); Python assertion/SystemExit (sweep .py). run-differentials.sh reports a missing eval input as `skipping:` text with exit 0 — the one place that still "skips" silently rather than failing.
- **Spawns cargo**: nightly-fixture-differentials.sh (datagen run + 6 test invocations), run-differentials.sh (6 `cargo test`), user-dir-round-trip.sh (2 `cargo test` unless prebuilt; `cargo build` via `oracle_cell_artifact`), libpinyin-drop-in-differential.sh (`cargo run`, `cargo test`). The oracle provisioning scripts themselves (build-oracle, check-reproducible, verify, image-tag, setup, fetch-model, Dockerfile, workflow) never spawn cargo.
- **Process control / timeouts / locks**: no lock files anywhere in this set. Timeouts: sweep .py 600 s per worker; curl `--retry 3` (build-oracle) and `--retry 3 --connect-timeout 30 --max-time 600` (fetch-model); setup.sh `retry` 2/4/8/16 s and a 60 s dockerd wait; docker-user-rt.sh apt timeouts 20 s × 8 retries × 10 loops; runners.json timeouts 3600 s (sweep) and 900 s (round trip). Concurrency: check-reproducible.sh backgrounds the a/b builds and `wait`s each PID; sweep .py `ThreadPoolExecutor` + `os.fork`. Cleanup: EXIT traps in build-oracle.sh (stage), check-reproducible.sh, verify-oracle-image.sh, user-dir-round-trip.sh; `rm -rf "$work"` up front in drop-in.
- **Frozen / "do not change"**: fetch-model.sh "Pinned identity (do not alter)"; build-oracle.sh `DATA_UNSTABLE_FILES` (moving a file is a deliberate recipe change), manifest key `libpinyin_tag` kept for compatibility; oracle-pin.txt hashes updated only in the same commit as the recipe/base/snapshot ("Do not edit the recorded values to match"); image tag immutability; the sweep's expected counts (29/31/0); `--dbm bdb` default (human ruling 2026-09-27), while capture goldens and pinyin-oracle stay `+dbm-tkrzw`; model20 must never enter the repo/image (model-provenance.md, enforced by verify-oracle-image.sh and fetch-model.sh's git-ignore check); no CI job downloads model20 (store-backends.yml header, oracle-image.yml header, nightly header).
- **Stale text observed**: run-differentials.sh header still describes a "release tarball" recipe and a Tkrzw default; docs/testing/oracle-environment.md's top table is the historical 2.11.91 freeze with amendments appended (by design); docker-user-rt.sh's python rewrite targets build-oracle.sh text that #818 restructured *(interpretation: likely broken, unverified)*; cloud.md still says "no image published yet".

## Disposition of #818 / PR #819 and #811 / PR #816

Sources: `gh pr view 819` (merged 2026-10-16T08:35:11Z; three commits `8a9af94`→main `6bf9228b`, `9551b70`→`30ff85e3`, `d8b8bbf`→`3b2bfdfe`), `gh issue view 818` (closed, no comments), `gh pr view 816` (merged 2026-10-16T10:26:09Z; one commit `6e2574de`→main `8eae92de`), `gh issue view 811` (closed; one maintainer audit comment), `git log --stat` on the four main commits, and the files above. "Observed" = read in the tree or the PR/issue text; "Interpretation" marked.

**Commit footprint (observed, `git log --stat`):**
- `6bf9228b` feat(oracle): oracle-image.yml (+126), Dockerfile.oracle (+53), build-oracle.sh (+266/−60), check-reproducible.sh (+119), image-tag.sh (+41), oracle-pin.txt (+32/−1), verify-oracle-image.sh (+78).
- `30ff85e3` docs: AGENTS.md (+10), axis-j-{bdb,kc,tkrzw}-ledger.md (digest lines), benches.md (+31), oracle.md (+114/−1).
- `3b2bfdfe` feat(cloud): AGENTS.md (+3), docs/runbooks/README.md (+1), cloud.md (+193), tools/cloud/setup.sh (+360), Dockerfile.oracle (+42/−11: proxy build arg + `cacert` secret), check-reproducible.sh (+18/−1: proxy flags).
- `8eae92de` fix(bisection): run-addon-candidate-diff.sh (+3/−19), run-import-diff.sh (+7/−25), run-train-diff-dynamic-off.sh (+19/−13), run-union-diff.sh (+4/−13).

### (a) How oracle libraries are made reproducible (established; must be preserved)

Observed in build-oracle.sh (comment "Reproducible-build settings (issue #818)"):
- `SOURCE_DATE_EPOCH=$(git -C "$lib_src" log -1 --format=%ct)` — the pin's commit time (1788427579, recorded as `source_date_epoch=` in oracle-pin.txt); exported with `TZ=UTC LC_ALL=C`.
- `CFLAGS="${CFLAGS--g -O2} -ffile-prefix-map=$lib_src=/usr/src/libpinyin -fdebug-prefix-map=$lib_src=/usr/src/libpinyin"`, same for `CXXFLAGS`; so the work dir does not enter `__FILE__`, debug info or macro expansion. Optimisation level stays autoconf's `-g -O2` unless the caller sets flags.
- `LDFLAGS="${LDFLAGS:+$LDFLAGS }-Wl,--build-id=none"` — the ELF build ID is omitted, not made deterministic.
- **No strip step exists** in build-oracle.sh, Dockerfile.oracle or the workflow; the libraries keep `-g` debug info (observed by absence; PR #819 and oracle.md say the same recipe: "`SOURCE_DATE_EPOCH`…, `-ffile-prefix-map` and `-fdebug-prefix-map`…, and `-Wl,--build-id=none`").
- Toolchain fixed by Dockerfile.oracle `deps`: `debian:testing@sha256:dab11cdb…` + apt snapshot `20260831T000000Z` (same digest/snapshot as Dockerfile.perf-matrix), `--platform linux/amd64` on every stage.
- `--libs-only` builds `src/` only (`make -C src`, `make -C src install`, `make install-pkgconfigDATA`): no ibus-libpinyin, no model, no `data/`.
- Explicitly outside the contract: `.la` and `.pc` files record the install prefix (oracle.md §2a; PR #819 "Only `.la` and `.pc` files differ between prefixes"); setup.sh rewrites `.pc` `prefix=` after relocation.

What check-reproducible.sh compares (observed): for each of bdb, kc, tkrzw, two concurrent in-image builds with different `--work-dir` (`/build/a` vs `/var/tmp/elsewhere/b`) and different `--prefix` (`/opt/oracle-a/<dbm>` vs `/opt/oracle-b/<dbm>`), both `--libs-only --enable-libzhuyin`; it `cmp`s the two `"<lib> <sha256>"` listings for `libpinyin.so` and `libzhuyin.so`, and then each hash against `<lib>_sha256_<dbm>` in tools/oracle/oracle-pin.txt. Both builds' exit statuses are waited on individually (a build that fails after printing hashes fails the gate). It compares only the two `.so` files — not headers, `.pc`, or `oracle-pin.txt` contents.

What the image tag encodes (observed, image-tag.sh): `<libpinyin_commit[0:12]>-<base_image digest[0:12]>-<apt_snapshot>-<sha256(Dockerfile.oracle ‖ build-oracle.sh)[0:12]>`, all but the last read from oracle-pin.txt, with a consistency check that the Dockerfile contains the recorded base image and snapshot. Current: `074a2219c90f-dab11cdb0a9d-20260831T000000Z-fb1afc111b20`. Non-vacuity evidence (PR #819): the same bdb build without the recipe gave different hashes in two work dirs; the recipe's six hashes were identical in the original podman build, in check-reproducible.sh, and in a from-scratch cloud setup.sh. *(Interpretation: the recipe hash means the tag changes on any byte change to either script, including comments, so a consolidation that moves or reformats build-oracle.sh retags the image and requires re-running check-reproducible.sh and re-recording the hashes in the same commit — the hashes themselves should not change if the executed recipe is unchanged, but the gate must be re-run to prove it.)*

### (b) How artifact identity is expressed and who verifies what

Observed:
- **`pin_ref` string** — built only by build-oracle.sh: `libpinyin-<LIBPINYIN_VERSION>-<LIBPINYIN_SHA>+model20-<MODEL_SHA256>+dbm-<bdb|kc|tkrzw>[+patches-<sha256 of oracle-patches.sha256>]`. Written as `pin_ref=` into the prefix's `oracle-pin.txt` (schema `pinyin-oracle-v1`). Verified byte-exactly by: `--regen-data` itself (`grep -qx "pin_ref=$ORACLE_PIN_REF"`), crates/pinyin-oracle `build.rs`/`pin.rs` (`EXPECTED_PIN_REF`, tkrzw; `PINYIN_BENCH_DBM` swaps the suffix), tools/capture/run-capture.sh, tools/bisection/oracle-cell.sh (`EXPECTED_PIN_REF="pin_ref=…+dbm-$ORACLE_DBM"`, derived from build-oracle.sh constants) consumed by ~20 run-*-diff.sh via `grep -Fxq`, run-all.py (line membership in the manifest), contract-diff.py and gcov_measure.py (recomposed from the record's `libpinyin_tag`/`libpinyin_commit`/`model_archive_sha256`), and a literal-prefix `grep` in eight runners. Patched variants: run-scheme-diff.sh and run-residue-a-tail-diff.sh recompute the patch-dir digest and require `$EXPECTED_PIN_REF+patches-<digest>` and `patches_manifest_sha256=<digest>`.
- **Prefix `oracle-pin.txt` fields** (written by build-oracle.sh): `schema`, `pin_ref`, `libpinyin_tag`, `libpinyin_commit`, `ibus_libpinyin_tag`, `ibus_libpinyin_commit`, `model_sha256`, `dbm` (configure spelling), `header_sha256`, `shared_object_sha256`, optional `zhuyin_shared_object_sha256`, optional `data_manifest_sha256` + `data_unstable_manifest_sha256` (not in `--libs-only` output), optional `patches_manifest_sha256`. Verified by: run-capture.sh (`shared_object_sha256`, `data_manifest_sha256` against the files), contract-diff.py (`shared_object_sha256` of the `.so`, both data manifests), gcov_measure.py/train-index-diff.py/delta_ledger.py (provenance), run-import-diff.sh (`model_sha256`, `dbm=`), user-dir-round-trip.sh (`dbm=` → cargo feature), pinyin-oracle `PinManifest` (schema + required fields). The sidecar manifests `oracle-data.sha256` (17 stable files) and `oracle-data-unstable.sha256` (6 DBM-path files: `addon_phrase_index.bin`, `addon_pinyin_index.bin`, `bigram.db`, `phrase_index.bin`, `pinyin_index.bin`, `punct.bin`) are checked by run-capture.sh, contract-diff.py, gcov_measure.py and setup.sh `data_ok` (`sha256sum --check oracle-data.sha256` + digest vs record).
- **Record `tools/oracle/oracle-pin.txt`** (v4) fields and readers: see ORC-07. The v4 additions (`platform`, `base_image`, `apt_snapshot`, `source_date_epoch`, six library hashes, three `data_manifest_sha256_<dbm>`) are read by image-tag.sh, oracle-image.yml (labels), setup.sh, check-reproducible.sh, verify-oracle-image.sh and build-oracle.sh `--regen-data`. The v3 change (#369) is the symmetric commit-SHA verification of both upstreams (recorded in the file's own comments and oracle-environment.md amendment 2026-09-07).
- **`oracle-patches.sha256`** — written by build-oracle.sh with `--apply-patches` (sorted `sha256sum` of every `*.patch`); its own SHA-256 is folded into `pin_ref` and recorded as `patches_manifest_sha256`. run-all.py checks the file's presence for patched variants; run-scheme-diff.sh and run-residue-a-tail-diff.sh verify the digest.
- **Image tag** — see (a). Verified for identity by: oracle-image.yml (`docker manifest inspect` existence check before push), setup.sh (derives the same tag, then trusts the pulled cells only if the six hashes match the record; otherwise discards and rebuilds). verify-oracle-image.sh verifies the image *contents* (six hashes + no model data), not the tag.
- *(Interpretation)* Identity is therefore layered: the record ↔ build-oracle.sh constants (hand-kept in lockstep), the recipe hash ↔ tag, the tag ↔ GHCR image, the image ↔ six library hashes, the prefix ↔ `pin_ref` + `shared_object_sha256` + data manifests. The one link not mechanically checked anywhere is record-vs-build-oracle.sh constant agreement (oracle-cell.sh reads build-oracle.sh; setup.sh/image-tag.sh read the record; nothing diffs the two) — the issue #811 comment notes a similar unchecked link between the datagen manifest's `pin_ref=model20-…` and the record.

### (c) The "same-run measurement" rule and the "wip" rule, as written

AGENTS.md at origin/main, section "## Measurement and lane state" (added by `30ff85e3`; observed verbatim):

> - **Measure in one run.** A measuring PR runs base, PR and pin in one job on one machine and reports only those numbers and their ratios. No absolute figure from another run, host or CPU is ever a baseline (`docs/runbooks/benches.md`).
> - **Push, don't hoard.** A lane pushes `wip/<issue>` at every checkpoint. Work that exists only in a local worktree or container does not exist: treat every machine as disposable.

AGENTS.md "## Toolchain" (added by `3b2bfdfe`): "Cloud sessions are a supported host for every gate: provision one with `tools/cloud/setup.sh` (`docs/runbooks/cloud.md`)."

docs/runbooks/benches.md "### Measure base, PR and pin in one run" (observed verbatim, key sentences):

> **A measurement is the ratio of artifacts timed in the same job on the same machine, never a number carried over from another run.** A measuring PR builds the base tree, the PR tree and the pin-built oracle, runs all three in one job on one machine (same CPU, same data), **interleaving** the base, PR and pin artifacts run by run, and reports only those figures and the ratios between them. Interleaving all three is a requirement of the measurement, not a property of `tools/bisection/run-perf-same-data.sh`, which interleaves two pin-built backends and two artifacts of the current tree and does not run a base artifact; a measuring PR that needs the base/PR/pin triple arranges that itself.
> No absolute figure from an earlier run, another host or another CPU is a baseline: callgrind `Ir` and wall time both move with the CPU (glibc picks `memcpy` and friends at run time), so a baseline taken elsewhere cannot be compared with a PR measured here. Earlier snapshots in this repository remain records of what was measured where; they are not baselines to subtract from. The pin comes from the reproducible oracle (`docs/runbooks/oracle.md`, section 2a), so the pin's hash is the same on every machine.

and "### State the Valgrind version":

> Every measurement report states the Valgrind version it used, as printed by `valgrind --version`, next to the figures.

Observed in cloud.md: "A lane runs the setup only when its task needs the pin oracle (a differential, a same-run measurement against the pin, a data check); a lane that does not touch the oracle never pays for it." The user's shared checkout at `8efd2543` (the AGENTS.md quoted in this session's system context) predates `30ff85e3`/`3b2bfdfe` and lacks both sections; origin/main has them. *(Interpretation: issue #818's fourth target "Optionally, a CI workflow does this on demand for a PR" was not implemented — no workflow in the tree runs base/PR/pin measurements.)*

### (d) What the cloud session setup provisions, in order

Observed in tools/cloud/setup.sh (numbered as the script's own section comments; cloud.md "What it does" matches):
0. **valgrind** — if `valgrind --version` fails: `apt-get install -y valgrind` (via `sudo` when `EUID != 0`), with one `apt-get update` retry; records the version for the summary.
1. **dockerd** — if `docker info` fails: when a proxy is set and `/root/.ccr/agent-proxy-ca.crt` exists and differs from `/usr/local/share/ca-certificates/ccr-agent-proxy.crt`, copy it and `update-ca-certificates`; then `setsid nohup dockerd >/var/log/dockerd.log &` and wait up to 60 s. Builds `net_flags=(--network host)` and, with a proxy, `env_flags` (proxy env, `no_proxy`, `GIT_SSL_CAINFO`/`SSL_CERT_FILE`/`CURL_CA_BUNDLE=/run/ca.crt`, CA bind mount).
2. **pinned base image** — `docker pull --platform <platform> <base_image>` (from the record) with 429 backoff, skipped if present.
3. **oracle libraries** — per requested cell: skip if `libs_ok` (both `.so` hashes equal the record). For the rest, unless `--no-pull`: pull `ghcr.io/shenghaoc/oxpinyin-oracle:$(image-tag.sh)`, `docker create … /none`, `docker cp` each needed cell out, re-check hashes, discard non-matching cells. Remaining cells: build `oxpinyin-oracle-deps:<Dockerfile hash12>` (`--target deps`, proxy build-arg + `cacert` secret when proxied) and run `build-oracle.sh --libs-only --enable-libzhuyin --dbm <cell> --work-dir /build/<cell> --prefix /opt/pinyin-oracle/<cell> --jobs N` in it with `$oracle_root` mounted at `/opt/pinyin-oracle`, build-oracle.sh and the record bind-mounted into `/usr/local/bin/`, model cache at `/model-cache:ro`. Then all cells' six hashes are compared to the record; any mismatch prints `MISMATCH … recorded/built` and exits **3**, leaving the libraries in place.
4. **model20 and data** — per cell, skip if `data_ok` (`oracle-data.sha256` digest equals `data_manifest_sha256_<cell>` and `sha256sum --check` passes); else `tools/model/fetch-model.sh --cache-dir <cache>/model20`, ensure the deps image, run `build-oracle.sh --regen-data --dbm <cell> --prefix /opt/pinyin-oracle/<cell> --model-dir /model-cache/extracted --work-dir /build/<cell>-data` in-image (staged swap only on manifest match), then re-run `data_ok`.
5. **report** — rewrite each cell's `.pc` `prefix=` to `<cache>/oracle/<cell>`, print one stdout line per cell, write `${OXPINYIN_ORACLE_ENV:-~/.oxpinyin-oracle.env}` (atomic `.new` + `mv`) exporting `PINYIN_MODEL_DIR`, `OXPINYIN_ORACLE_PREFIX_<C>`, `OXPINYIN_ORACLE_DATA_<C>`, `PINYIN_ORACLE_DBM` default (`bdb` if provisioned, else first cell), and a block that recomputes `PINYIN_ORACLE_PREFIX` and `OXPINYIN_LIBPINYIN_DATA_DIR` for the selected cell on every `source` (erroring on an unprovisioned cell); log valgrind version and the env-file path.

Measured (cloud.md, 2026-10-16 UTC, 4-core): 353 s from scratch without a published image (dockerd+pull 6 s, deps 62 s, libraries 28/52/32 s, model+regen 44/77/52 s); 0.33 s when everything is present; +2.0 GB disk. The published-image path is unmeasured and untested (no image existed).

### (e) #811 / PR #816: asymmetric system-data inputs

**What "asymmetric system-data inputs" meant** (observed, issue #811 body and the maintainer's audit comment): four bisection runners gave the two sides of a differential *different* system data — oxpinyin's C ABI read the committed mini tables `fixtures/w3/<ext>` (`db`/`kct`/`tkt`), while the pin read its full installed `$PREFIX/lib/libpinyin/data`. The symptom: `run-train-diff-dynamic-off.sh` red on main since `f7fc2b7c` (2026-10-12, "refuse remembered phrases with unknown characters") at `remember 世界 failed`, because the mini fixture has no token for 世 or 界 (the pin refuses the same input on the same fixture via `_pre_compute_tokens`, `src/pinyin.cpp:3688-3695` at 074a2219); oxpinyin's earlier `true` had hidden the asymmetry. The audit comment classified every runner: four mismatched; a long list "same directory on both sides (identical by construction)" (run-bisect, locale, open-counter, session-fork, two-context, user-file-modes, candidate-assembly, nbest-choose-train, bigram-export, export, import-semantics, same-data-dir, residue-* with `*_SAME_DIR=1`); a "same source model, match not enforced" class (train-diff, abi-probe, dynamic-adjust, live-typing, option-sweep, scheme, run-w11-diff, residue-* by default — all via `resolve_system_dir`, which refuses the mini fixture unless `OXPINYIN_ALLOW_MINI_FIXTURE=1`); and "not comparable" (cpp-smoke, zhuyin, the surface runners, perf runners, system-dir.sh helper).

**The four runners and how each was fixed** (observed, `git show 8eae92de`):
1. `tools/bisection/run-train-diff-dynamic-off.sh` — sources `./system-dir.sh`; after the oracle gate (so a missing oracle still exits 77) adds `SYSTEM="$(resolve_system_dir TRAINDIFF_SYSTEM train-diff)"` and passes `"$SYSTEM"` to `./train-diff "$CAPI_SO" …` in phase 1 (was `$REPO_ROOT/fixtures/w3/$OXPINYIN_CAPI_BACKEND_EXT`). Phase 2 (populated store) now reads `CAPI_DATA="${OPTION_SWEEP_CAPI_DATA:-$SYSTEM}"`; if that dir lacks `interpolation2.text` it stages `cp -a` of it into `mktemp -d /tmp/traindiff-capi-data-XXXXXX` and copies `interpolation2.text` from `$PINYIN_MODEL_DIR` or `$REPO_ROOT/target/model20/extracted`; scratch removed by the EXIT trap. The `/tmp/oxpinyin-export` auto-detection branch was removed.
2. `tools/bisection/run-union-diff.sh` — sources `./system-dir.sh`; `CAPI_SYS="$(resolve_system_dir UNION_SYSTEM union-diff)"` replaces the `mktemp -d` + `cp -a fixtures/w3/$FIXTURE_EXT/.` + stub `interpolation2.text` block; `$CAPI_SYS` dropped from the trap.
3. `tools/bisection/run-import-diff.sh` — `CAPI_SYSTEM="$(resolve_system_dir IMPORT_SYSTEM import-diff)"` replaces `${IMPORT_SYSTEM:-$REPO_ROOT/fixtures/w3/$OXPINYIN_CAPI_BACKEND_EXT}`; the now-redundant file-presence and backend-mismatch checks (done inside `resolve_system_dir` by `system_dir_require_complete` + `system_dir_require_backend`) were removed; `IMPORT_SYSTEM` remains the override name.
4. `tools/bisection/run-addon-candidate-diff.sh` — reduced to `exec ./run-w11-diff.sh addon-candidate-diff "$@"`; the fixture copy and `CAPI_W11_SYSTEM_DIR` export were deleted (run-w11-diff.sh already resolves its directory through system-dir.sh; `CAPI_W11_SYSTEM_DIR` stays an override there).

What `resolve_system_dir VAR LABEL` reads (observed, system-dir.sh): the runner's own `$VAR`, then `OXPINYIN_SYSTEM_DIR`, then `target/datagen/<ext>`, then `/tmp/oxpinyin-export` only if written by the same backend; it requires completeness and the cell's backend, and refuses the mini fixture without `OXPINYIN_ALLOW_MINI_FIXTURE=1`. Under run-all.py, `OXPINYIN_SYSTEM_DIR` is set to the cell prefix's own `lib/libpinyin/data`, so these runners become literally same-directory.

**The rule the fix established** (observed in the PR body and the new comments in the runners; interpretation of its generality): a differential runner feeds both sides the same full data, resolved through `resolve_system_dir` *after* the oracle gate, so that (i) a missing oracle still SKIPs with exit 77, (ii) the mini `fixtures/w3` tables never reach one side of a pin comparison, and (iii) the per-runner override variable (`TRAINDIFF_SYSTEM`, `UNION_SYSTEM`, `IMPORT_SYSTEM`, `CAPI_W11_SYSTEM_DIR`) and the sweep-wide `OXPINYIN_SYSTEM_DIR` remain the only ways to point a runner elsewhere. Verification was rule-7 "tooling only" on the bdb reference cell; kc/tkrzw not run. Left open by the issue (observed in the audit comment, not addressed by #816): nothing compares the datagen manifest's `pin_ref=model20-…` with the record inside `resolve_system_dir`; `run-union-diff.sh`'s drivers print some data-sensitive rows (`addon:`, `user-pos`, `pred:`) — the PR reports they were IDENTICAL on full data, so no re-baseline was needed.


---

# Inventory — TLS (abi, perf-gate, profile, capture, env-probe, store, tkrzw, kc, bdb, packaging, repo lint harnesses, benches)

Tree read: detached worktree at origin/main `e67f19d1` (`scratchpad/ox`). Every assigned file was read in full; bench support modules (`crates/*/benches/support/mod.rs`) and the sourced helper `tools/bisection/oracle-cell.sh` were read for Inputs/Overlap and are folded into the records that use them.

---

### TLS-01 — tools/abi/check-exports.sh

| Field | Value |
|---|---|
| Entry point | executed: `tools/abi/check-exports.sh [--shipped]` (cd's to repo root) |
| Owning subsystem | abi |
| Purpose | Proves each cdylib exports EXACTLY the `global:` set of the pin's version script (`libpinyin.ver` 79 `pinyin_*`, `libzhuyin.ver` 52 `zhuyin_*`) — nothing missing, nothing extra, no leaked non-ABI symbol. |
| Implementation | bash, 135 lines; `nm`, `awk`, `comm`; sources `tools/bisection/oracle-cell.sh` (which runs `cargo build`). |
| Inputs | `crates/oxpinyin-capi/libpinyin.ver`, `crates/oxpinyin-zhuyin-capi/libzhuyin.ver` (expectation source: the `global:` list of each .ver, copied verbatim from the pin); built cdylibs `libpinyin_capi.so`/`libzhuyin_capi.so`. Env (via oracle-cell.sh): `PINYIN_ORACLE_DBM` (bdb\|kc\|tkrzw → `CAPI_FEATURE`), `CARGO_TARGET_DIR`, `OXPINYIN_TARGET_ROOT`; prebuilt-artifact overrides `OXPINYIN_CAPI_SO`, `OXPINYIN_ZHUYIN_SO`. `--shipped` appends `,shipped` to `CAPI_FEATURE`. |
| Outputs | stdout: per-library `--- name ---`, counts, `MISSING`/`EXTRA`/`LEAKED non-ABI export`/`allowed non-ABI export` lists, `OK: exactly the .ver set`; final `PASS:`/`FAIL:` line. Exit 0 pass, 1 any deviation or lib not built, 2 unsupported host. |
| Platform | Linux (`nm -D --defined-only`, `.so`) and macOS (`nm -gU`, `.dylib`, leading `_` stripped); other hosts exit 2. |
| Consumers | `.github/workflows/ci.yml` test job ("ABI export gate (development build)" and "(shipped build)"); `tools/bisection/runners.json` entry `abi-exports` (kind gate, cells bdb/kc/tkrzw, oracle false); `docs/runbooks/release.md` pre-tag step 2; referenced by `check-alloc-pairing.sh`, `docs/safety/*`, `docs/findings/abi-allocator-pairing.md`. |
| State | none (no temp dir); builds into `CARGO_TARGET_DIR` (per-cell `target/cells/<dbm>` default from oracle-cell.sh). |
| Overlap | `ver_globals()` awk duplicated verbatim in `check-alloc-pairing.sh`; same .ver parsing re-implemented with sed in `tools/packaging/relink-versioned.sh`. `oracle_cell_artifact` shared via oracle-cell.sh. |
| Constraints | The two .ver files are the frozen drop-in contract ("checked in verbatim beside the crates"). Dev build allows exactly two extra exports: `oxpinyin_init_for_fixtures`, `oxpinyin_test_set_user_bigram` (`#[cfg(not(feature="shipped"))]`); shipped build allows none. SONAME/symbol versioning explicitly out of scope (→ `build.rs`, `docs/findings/drop-in-abi-identity.md`). |

### TLS-02 — tools/abi/check-alloc-pairing.sh

| Field | Value |
|---|---|
| Entry point | executed: `tools/abi/check-alloc-pairing.sh [--static-only]` |
| Owning subsystem | abi |
| Purpose | Freezes WHAT crosses the ABI with each symbol: every pointer-shaped slot in `pinyin.h`/`zhuyin.h` is classified once in the `.alloc` register, the class matches the C type, every `handle:<fn>` destructor is an exported symbol, and a C++ consumer under ASan/LSan proves the declared deallocator really releases each slot. |
| Implementation | bash, 458 lines; sed/awk header flattener; `g++ -std=c++17 -fsanitize=address -O1 -g -fno-omit-frame-pointer`; `pkg-config glib-2.0`; sources `oracle-cell.sh`. |
| Inputs | Registers `crates/oxpinyin-capi/libpinyin.alloc`, `crates/oxpinyin-zhuyin-capi/libzhuyin.alloc`; headers `pinyin.h`, `zhuyin.h`; the two .ver files; drivers `tools/abi/alloc-pairing-{pinyin,zhuyin}.cc`; fixtures `fixtures/w3/<ext>/*` (ext chosen by `OXPINYIN_CAPI_BACKEND_EXT` ∈ {kct,tkt,db}, else first of db/tkt/kct present) plus a synthesized stub `interpolation2.text`. Env: `OXPINYIN_CAPI_BACKEND_EXT` (exported by oracle-cell.sh), `GLIB_CFLAGS` (override), `PINYIN_ORACLE_DBM`, `CARGO_TARGET_DIR`, `OXPINYIN_TARGET_ROOT`, `OXPINYIN_CAPI_SO`, `OXPINYIN_ZHUYIN_SO`; sets `ASAN_OPTIONS=detect_leaks=1:abort_on_error=0` for the drivers. |
| Outputs | stdout: per-ABI static sections (`header slots: N, register entries: M`, lists of unclassified/stale/duplicate slots, shape-check lines), dynamic sections (`slots the driver never reached`, `false-return contracts the driver never probed`, `driver/register drift`, negative-control line); final `PASS:`/`FAIL:`. Exit 0 all pass, 1 any `FAIL:`, 2 usage/non-Linux dynamic/no fixture dir/bad ext. |
| Platform | Dynamic half Linux-only (LeakSanitizer ptrace stop-the-world does not run under macOS sandbox); `--static-only` is portable. |
| Consumers | `ci.yml` test job "Allocator-pairing gate"; `runners.json` entry `abi-alloc` (gate, all three cells, oracle false, needs ASan/LSan); `docs/runbooks/release.md` step 3; `docs/findings/abi-allocator-pairing.md`, `probe-coverage-abi.md`, `abi-reference.md`, `docs/safety/*`. |
| State | `WORK=$(mktemp -d)` removed on EXIT: copies of fixture dir as `sys/`, per-ABI `<abi>-user` dirs (re-created before the leak build), `lib/` with symlinks `libpinyin_capi.so`→built artifact and `libpinyin.so.15`→it (DT_NEEDED resolution via rpath, "run-cpp-smoke.sh's law"), driver binaries. |
| Overlap | `ver_globals` duplicated from `check-exports.sh`; `prepare_fixtures` is a stated copy of `tools/bisection/run-cpp-smoke.sh` ("duplicated rather than shared … kept in step by review"); GLIB_CFLAGS override convention mirrors run-cpp-smoke.sh. |
| Constraints | Register line = five fields `<symbol> <slot> <class> <notes> <verified>`; `VERIFIED_KINDS` regex `^(pin-probe:…\|pin-aborts:row\d+[a-d]?\|source:file:lines\|unverified\|unexecuted\|n/a)$`. Class↔shape law: `ptr-return`→`handle:*`\|`borrowed`; `strv` (`gchar ***`)→`g_strfreev`; `const T **`→`borrowed`; `char **`→`g_free`; opaque `T **`→`borrowed`. Negative controls are REQUIRED to fire (leak build must exit non-zero with "LeakSanitizer"; two corrupted register copies must be rejected). Coverage: every slot `hit`; every non-n/a/non-unreachable contract `checked`; all four register fields must match the driver's table (drift check). |

### TLS-03 — tools/abi/alloc-pairing-pinyin.cc

| Field | Value |
|---|---|
| Entry point | compiled by check-alloc-pairing.sh; `./driver <systemdir> <userdir> [--coverage]`; second build with `-DOXPINYIN_ALLOC_PAIRING_LEAK` |
| Owning subsystem | abi |
| Purpose | The C++ consumer proving `libpinyin.alloc`: drives `libpinyin.so.15` through one full lifecycle, releases every owned slot with exactly the declared deallocator, probes every reachable false-return contract, and reports per-slot coverage. |
| Implementation | C++17, 691 lines; `<pinyin.h>`, glib (`g_free`, `g_strfreev`, `GArray`), `<sanitizer/lsan_interface.h>` (`__lsan_disable/enable`, `__lsan_do_recoverable_leak_check`). Links `-lpinyin_capi`. |
| Inputs | argv: systemdir (w3 fixture copy), userdir, `--coverage`. Embedded `kSlots[26]` table mirroring the register's first four fields. Inputs typed: `nihao`, `你好` token lookup, `xi'` zero-key probe, `pinyin_remember_user_input("你好")`, chooses 你 then 好 and `pinyin_train` so the bigram export iterator has rows; sweeps printable ASCII for `pinyin_in_chewing_keyboard`. Sort mask `kDefaultSort=0x1e`. |
| Outputs | `--coverage`: one `SLOT <symbol> <slot> <class> <on-false> hit|miss n-a|unreachable|checked|unchecked` line per slot on stdout; `FAIL: …` contract lines on stderr. Exit 0 ok; 1/2 init/alloc failure; 10 LSan per-call leak; 11 contract broken; 64 usage. Leak build drops `g_free`/`g_strfreev` (keeps marks). |
| Platform | Linux (LSan). |
| Consumers | `check-alloc-pairing.sh` only. |
| State | Two passes: pass 1 inside `__lsan_disable()` absorbs one-time statics (backend, glib, lazy tables); `reset_coverage()` between passes; pass 2 live. |
| Overlap | Near-duplicate of `alloc-pairing-zhuyin.cc` (same `Slot`, `mark`, `expect_declared`, `release_*`, two-pass main) — separate TU by design because `pinyin.h`/`zhuyin.h` re-declare the same typedefs. |
| Constraints | Pointer-ownership rules asserted (handles): `pinyin_init`→`pinyin_fini`; `pinyin_alloc_instance`→`pinyin_free_instance`; `pinyin_begin_add_phrases`→`pinyin_end_add_phrases`; `pinyin_begin_get_phrases`→`pinyin_end_get_phrases`; `pinyin_begin_get_bigram_phrases`→`pinyin_end_get_bigram_phrases`. `g_free` slots: `pinyin_get_sentence/sentence` (false-untouched), `pinyin_get_{full_pinyin,double_pinyin,chewing}_auxiliary_text/aux_text` (false-ALLOCATES — must free on false), `pinyin_get_{pinyin,zhuyin,luoma_pinyin,secondary_zhuyin}_string/utf8_str` (false-nulls), `pinyin_get_pinyin_strings/{shengmu,yunmu}` (false-untouched), `pinyin_token_get_phrase/utf8_str` (false-untouched), `pinyin_iterator_get_next_phrase/{phrase,pinyin}` (false-untouched), `pinyin_bigram_iterator_get_next_phrase/{phrase,pinyin}` (false-ALLOCATES on the terminal row). `g_strfreev`: `pinyin_in_chewing_keyboard/symbols` (false-nulls). `borrowed` (never freed): `pinyin_get_context` return, `pinyin_get_candidate/candidate`, `pinyin_get_candidate_string/utf8_str` (false-unreachable), `pinyin_get_pinyin_key/key`, `pinyin_get_pinyin_key_rest/key_rest`. Allocation counting: no interposition of its own — ASan's malloc interposition covers libc `malloc` in `ffi::owned_cstr` and Rust `Box`; `g_free` is a thin `free` since GLib 2.46 (the pairing assumption). Never probes NULL-argument refusals (pin would crash). |

### TLS-04 — tools/abi/alloc-pairing-zhuyin.cc

| Field | Value |
|---|---|
| Entry point | compiled by check-alloc-pairing.sh; `./driver <systemdir> <userdir> [--coverage]`; leak variant via `-DOXPINYIN_ALLOC_PAIRING_LEAK` |
| Owning subsystem | abi |
| Purpose | Same proof for `libzhuyin.so.15` against `libzhuyin.alloc` (12 slots). |
| Implementation | C++17, 414 lines; `<zhuyin.h>`, glib, LSan interface; links `-lzhuyin_capi`. |
| Inputs | argv as above. Types standard-scheme chewing `su3cl3` (你好) via `zhuyin_parse_more_chewings` (full-pinyin parse finds no rows in the mini fixture); `zhuyin_guess_candidates_after_cursor(instance,0)`; `zhuyin_train`; `xi1'` zero-key probe at offset 3; `zhuyin_begin_add_phrases(USER_DICTIONARY)`. |
| Outputs | identical `SLOT …` protocol and exit codes (0/1/2/10/11/64). |
| Platform | Linux. |
| Consumers | `check-alloc-pairing.sh`. |
| State | two-pass LSan discipline as TLS-03. |
| Overlap | duplicate of TLS-03 mechanics. |
| Constraints | Handles: `zhuyin_init`→`zhuyin_fini`, `zhuyin_alloc_instance`→`zhuyin_free_instance`, `zhuyin_begin_add_phrases`→`zhuyin_end_add_phrases`. `g_free`: `zhuyin_get_sentence/sentence` (false-untouched), `zhuyin_get_{zhuyin,pinyin}_string/utf8_str` (false-nulls), `zhuyin_token_get_phrase/utf8_str` (false-untouched). `g_strfreev`: `zhuyin_in_chewing_keyboard/symbols` (false-nulls). Borrowed: `zhuyin_get_candidate/candidate`, `zhuyin_get_candidate_string/utf8_str` (false-unreachable), `zhuyin_get_zhuyin_key/key`, `zhuyin_get_zhuyin_key_rest/key_rest`. No export iterators on the zhuyin ABI. |

### TLS-05 — tools/perf-gate/callgrind-ir.py

| Field | Value |
|---|---|
| Entry point | executed: `callgrind-ir.py <callgrind-out-file> <object-substring>`; importable `object_ir(path, want)` |
| Owning subsystem | perf |
| Purpose | Total self-`Ir` attributed to one object (`ob=` name containing the substring) in a callgrind out-file, correctly handling name compression, inclusive `calls=` cost lines, declared `events:` column order and `positions:` count. |
| Implementation | Python 3 stdlib only, 128 lines. |
| Inputs | a callgrind file; substring (snapshot.sh passes `libpinyin`). No env vars. |
| Outputs | stdout integer, or `null` when no `Ir` event or nothing attributed. Exit 0; 2 usage/OSError. |
| Platform | portable |
| Consumers | `tools/perf-gate/snapshot.sh` (`ir_json`); `tools/perf-gate/callgrind-ir.test.sh`. |
| State | none |
| Overlap | `tools/bisection/cg-calls.py` (named in benches.md) also parses callgrind caller→callee edges — different question, separate parser. |
| Constraints | Absence→`null`, never 0 (series.py's "absence is not zero" rule). |

### TLS-06 — tools/perf-gate/callgrind-ir.test.sh

| Field | Value |
|---|---|
| Entry point | executed: `tools/perf-gate/callgrind-ir.test.sh` (cd's to its own dir) |
| Owning subsystem | perf |
| Purpose | Nine hand-computed fixture cases, one per callgrind-format trap (plain sum, other-object exclusion, `ob=(1)` back-reference, inclusive call cost skipped, `Ir` chosen by name from `events: Dr Ir Dw`, two position columns, `cob=` not retargeting, no-match→null, no-Ir→null). |
| Implementation | bash, 157 lines; heredoc fixtures. |
| Inputs | none external. |
| Outputs | `ok/FAIL <label> <got>` lines, `N passed, M failed`; exit 0 iff no failures. |
| Platform | portable |
| Consumers | none in CI or runners.json (grep: only referenced by itself/snapshot.sh comments). |
| State | `mktemp -d`, trap-removed. |
| Overlap | same `check()`/pass-fail harness shape as `series.test.sh`. |
| Constraints | expected numbers "hand-computed from the fixture text" — do not regenerate. |

### TLS-07 — tools/perf-gate/series.py

| Field | Value |
|---|---|
| Entry point | executed: `series.py --snapshot snap.json --series-dir dir/ [--append] [--summary out.md]` |
| Owning subsystem | perf |
| Purpose | Appends a nightly snapshot to the series (only with `--append`) and compares it with the previous sample; flags a night-over-night jump >1% in a deterministic metric only when the recorded environment is unchanged. |
| Implementation | Python 3 stdlib (argparse, json, pathlib), 234 lines. |
| Inputs | snapshot JSON (schema: `captured_utc`, `commit`, `event`, `run_url`, `environment{kernel,runner_image,rustc,cc,glibc,valgrind,binutils,backend,fixture_sha256,cpu_pinned,harness_commit}`, `artifacts`, `metrics{...}`); series dir of `*.json`. No env vars. |
| Outputs | Markdown report on stdout (and `--summary` file): table of `section_sum`, `stripped_size`, `alloc_count_per_cycle`, `alloc_bytes_per_cycle`, `ir_oxpinyin_object` (FLAGGABLE) and `rss_first_cycle_kib`, `rss_last_cycle_kib` (REPORT_ONLY, "trend only"); notes `not measured`/`no predecessor value`/`unattributable`/`**flagged**`. Exit 0 reported, 1 flagged move, 2 malformed snapshot. |
| Platform | portable |
| Consumers | `.github/workflows/verify-nightly.yml` job `perf-snapshot` (appends only when `GITHUB_EVENT_NAME=schedule`; series cached under `.perf-series` with key `perf-snapshot-<run_id>`, restore-key prefix `perf-snapshot-`); `series.test.sh`. |
| State | Series file name `<captured_utc stripped>-<commit[:12]>.json`; retains the newest 30 samples; corrupt or wrong-shape predecessor degrades to "no predecessor". |
| Overlap | none |
| Constraints | `ATTENTION = 0.01` (1%); from-zero transitions flagged; nulls never compared; environment delta → report but never flag; manual `workflow_dispatch` must not become the predecessor. Design doc: `docs/perf/ci-perf-size-gate-proposal-2026-09-09.md` ("NOT a gate … does not run on the PR path"). |

### TLS-08 — tools/perf-gate/series.test.sh

| Field | Value |
|---|---|
| Entry point | executed: `tools/perf-gate/series.test.sh` |
| Owning subsystem | perf |
| Purpose | Encodes series.py's rules as exit-code assertions: no-predecessor, append-only-on-schedule, 0.1% not flagged, 5% flagged (exit 1), env change → unattributable (exit 0), RSS trend-only, null not an improvement, zero→positive flagged, malformed → exit 2, list/string-metric predecessor treated as missing. |
| Implementation | bash + embedded python3 snapshot writer, 177 lines. |
| Inputs | none external. |
| Outputs | `ok/FAIL` lines, summary, exit 0 iff all pass. |
| Platform | portable |
| Consumers | none in CI/runners.json (referenced only from the proposal doc). |
| State | mktemp dir. |
| Overlap | harness shape shared with callgrind-ir.test.sh. |
| Constraints | fixture environment fields define the expected snapshot schema. |

### TLS-09 — tools/perf-gate/snapshot.sh

| Field | Value |
|---|---|
| Entry point | executed: `tools/perf-gate/snapshot.sh --out snapshot.json [--backend tkrzw]` |
| Owning subsystem | perf |
| Purpose | Measures the tree once and emits one JSON sample: size (ELF section sum + stripped size of the shipped `libpinyin.so.15.0.0`), steady-cycle allocations (from an `alloc-count` fixture build), RSS medians, and callgrind `Ir` attributed to the oxpinyin object. No wall clock by design. |
| Implementation | bash + inline python3, 258 lines; needs `cargo gcc readelf strip nm`, optional `taskset`, optional `valgrind`; builds `tools/bisection/bisect.c` with `gcc -std=gnu11 -O2 -ldl`; calls `tools/packaging/install.sh` (cargo cinstall). |
| Inputs | `fixtures/w3/<tkt|kct>` (backend→ext map: tkrzw→tkt, kyotocabinet→kct; bdb NOT supported here). Env: `PERF_CPU` (default 0), `PERF_GATE_CYCLES` (8), `PERF_GATE_CG_CYCLES` (4), `PERF_GATE_RSS_PROCESSES` (5); sets `PERF_BACKEND=gate PERF_MODE=speed PERF_CYCLES=…` for bisect; reads `GITHUB_EVENT_NAME`, `GITHUB_SERVER_URL`, `GITHUB_REPOSITORY`, `GITHUB_RUN_ID`, `ImageOS`, `ImageVersion`; sets `CARGO_TARGET_DIR` per build. |
| Outputs | JSON file (schema in TLS-07) with `artifacts.shipped_libpinyin{recipe,sha256,bytes}` and `alloccount_libpinyin.recipe`; stderr build logs; `wrote <out>`. Exit 0; 2 on missing tool/fixture/`--out`, unsupported backend, cinstall produced no `.so.15.0.0`, shipped artifact exports `oxpinyin_alloc_*`, or unresolvable commit/harness_commit ("a sample with no provenance must not enter the series"). |
| Platform | Linux (readelf/strip/nm, `/proc`-based bisect RSS). |
| Consumers | `verify-nightly.yml` `perf-snapshot` ("Measure": `--backend tkrzw`); docs/perf proposal and decode-path baseline. |
| State | `WORK=$(mktemp -d)` with `stage/`, `target-shipped/`, `target-alloc/`, `bisect`, `callgrind.out`; trap-removed. `taskset -c $PERF_CPU` when available, recorded as `cpu_pinned`. |
| Overlap | Spawns cargo twice (via install.sh cinstall and a direct `cargo build --release -p oxpinyin-capi --features <backend>,alloc-count`); shares `bisect --perf` with `tools/bisection/run-perf-baseline.sh` and `tools/profile/run-w8-cycle.sh`; the `nm -D | grep oxpinyin_alloc_` shipped-artifact check restates the runbook rule. |
| Constraints | "This is NOT a gate … no threshold lives in this file". Section set for `section_sum`: `.text .rodata .data.rel.ro .rela.dyn .eh_frame .eh_frame_hdr .gcc_except_table` (file size is page-quantized, per perf-so-size-2026-09.md). Absent keys (`steady_alloc_*`, valgrind) → `null`, never 0. Ir collected only inside `bisect_perf_steady_unit` (`--toggle-collect`). `harness_commit` = last commit touching `tools/bisection/bisect.c` or `tools/perf-gate`. |

### TLS-10 — tools/profile/run-w8-cycle.sh

| Field | Value |
|---|---|
| Entry point | executed: `tools/profile/run-w8-cycle.sh` (no args) |
| Owning subsystem | profile |
| Purpose | Samples the W8 keystroke cycle (`bisect --perf`, 20 inputs parse+guess+count) against a `cargo cinstall --profile profiling` oxpinyin-capi with debug info; preference order `samply record` → `flamegraph` (cargo-flamegraph) → `perf record --call-graph dwarf -F 999` → `valgrind --tool=callgrind` → timing only. |
| Implementation | bash, 263 lines; cargo-c, gcc (`-std=gnu11 -O2 -g -ldl`), pkg-config, python3 (realpath), sha256sum/shasum, optional samply/flamegraph/perf/inferno-flamegraph/valgrind/callgrind_annotate/taskset. |
| Inputs | tkrzw export dir (`PINYIN_EXPORT_DIR`, default `/tmp/oxpinyin-export`) — must contain `datagen-manifest.txt` with `backend=tkt` and `pinyin_index.bin`, `phrase_index.bin`, `bigram.db`; verified model20 (`PINYIN_MODEL_DIR` > `PINYIN_MODEL_CACHE/extracted` > `target/model20/extracted`; marker `verified` with `sha256=<MODEL20_SHA256>` or archive `downloads/model20.text.tar.gz` digest); env `PERF_CYCLES` (32), `PERF_CPU`, `PROFILE_REUSE_STAGE=1`, `OXPINYIN_PROFILE_DIR` (default `target/profile`); sets `PERF_BACKEND=oxpinyin PERF_MODE=speed`, `LD_LIBRARY_PATH`, `PKG_CONFIG_PATH`, `PKG_CONFIG_SYSROOT_DIR`. Hard-coded `PIN_REF` (…074a2219…+dbm-tkrzw) and `MODEL20_SHA256`, `EXPECTED_MODEL_FILES` (18 names). |
| Outputs | under `$PROFILE_DIR`: `header.txt` (pin, model sha, so, data, cycles, profile), `w8-cycle-timing.json` (bisect's JSON line), and one of `w8-cycle.profile.json.gz` (samply), `w8-cycle.svg`, `perf.data`+`perf-report.txt`, `callgrind.out`+`callgrind-annotate.txt`. Exit 1 on missing export/model/so; 2 if export is not tkrzw. |
| Platform | Linux-first (perf/samply/`taskset`; cargo-c libdir hard-coded `usr/lib64`). |
| Consumers | `docs/runbooks/benches.md` "Profiles"; many `docs/perf/*` snapshots; `tools/oracle/build-oracle.sh` comment (build prefix with `--dbm tkrzw`). Not in CI. |
| State | `target/profile/stage` cinstall tree (reused with `PROFILE_REUSE_STAGE=1`); copies the three tables + `interpolation2.text` into `stage/usr/share/oxpinyin` ("cargo-c does not install data files"). |
| Overlap | `verify_pinned_model` duplicates the model-cache verification in `oxpinyin_testsupport::model_cache` (also re-implemented in `crates/oxpinyin-capi/benches/support/mod.rs`); `bisect --perf` shared with `run-perf-baseline.sh` and `snapshot.sh`; third build recipe (`--profile profiling`) that docs/perf/README.md says is "neither the fixture nor the product". |
| Constraints | tkrzw cell only ("the oracle this cycle compares against is tkrzw"); pin ref hard-coded; profile is release + line-tables + thin LTO + `panic=unwind`. |

### TLS-11 — tools/profile/extract-hot-stacks.py

| Field | Value |
|---|---|
| Entry point | executed: `extract-hot-stacks.py [profile.json(.gz)]` (default `target/profile/w8-cycle.profile.json.gz`); `--self-test` |
| Owning subsystem | profile |
| Purpose | Prints the 12 hottest stacks (by sample self-count) from a samply / Firefox-profiler JSON dump, handling both column-oriented and `{schema,data}` row-oriented tables. |
| Implementation | Python 3 stdlib (gzip, json, collections), 255 lines; embedded self-test. |
| Inputs | profile path; no env vars. |
| Outputs | `# hottest stacks in <path>` then `rank. count (pct%) leaf` + last 8 frames each; exit 0; 1 if file missing or no samples. |
| Platform | portable |
| Consumers | `docs/runbooks/benches.md`, `docs/perf/perf-stage2-harness-2026-08.md`. Not in CI ("not a CI gate"). |
| State | none |
| Overlap | none |
| Constraints | stack walk capped at 12 frames; prefix-only rows and invalid frames skipped (self-test pins this). |

### TLS-12 — tools/capture/capture.c

| Field | Value |
|---|---|
| Entry point | compiled by run-capture.sh (`cc -std=c11 -Wall -Wextra -Werror $(pkg-config --cflags libpinyin)`); `pinyin-capture F-A|F-C|ZH SYSTEM_DATA_DIR FRESH_USER_DIR PIN_REF` |
| Owning subsystem | capture / fixtures |
| Purpose | Emits the frozen `pinyin-capture-v1` observation records of the pin-built libpinyin for three families: F-A (15 parse/candidate cases), F-C (23 option bits × off/on = 46 cases), ZH (26 chewing-keyboard parse cases). |
| Implementation | C11, 652 lines; links the pinned `libpinyin` (pkg-config) and glib (`g_free`, `g_strdup`). |
| Inputs | argv only; option words `BASE_FLAGS=IS_PINYIN`, `DEFAULT_FLAGS=IS_PINYIN\|PINYIN_INCOMPLETE\|USE_DIVIDED_TABLE\|USE_RESPLIT_TABLE`; a 4096-byte `!` junk input per family; ZH schemes STANDARD/HSU/IBM/GINYIEH/ETEN/ETEN26/HSU_DVORAK/DACHEN_CP26 (STANDARD_DVORAK=7 excluded: upstream setter abort). |
| Outputs | One LF-terminated line per case, TAB-separated `key=value`: `schema=pinyin-capture-v1`, `pin_ref`, `family`, `case`, `api_sequence` (exact public-API call list), `input` (escaped `\\ \t \n \r \xNN`), `flags=0x%08x`, [`scheme=%d` ZH], `parse_return`, `parsed_input_length`, `segments=` (`canonical@begin:end[:complete|partial]` comma list, `<missing-rest|position|pinyin>@off`, `-` if none), [F-A/F-C: `candidate_total`, `candidates_hex=` first ≤10 candidates as lowercase hex UTF-8, `-` if none], [ZH: `zhuyin=` renderings], `remainder=`. Exit 0; 1 on any API failure/`pinyin_init` NULL; 2 usage/empty pin ref. Rejects `DYNAMIC_ADJUST` in flags. |
| Platform | Linux (pin-built oracle). |
| Consumers | `run-capture.sh`; the wire format is mirrored by `crates/pinyin-oracle/src/{observation,capture,flags,live}.rs`; `docs/testing/oracle-ffi-seam.md`. |
| State | none; one `pinyin_instance` per case. |
| Overlap | `emit_case` and `emit_zhuyin_case` duplicate the segment-walk loop; escaping is the same convention `docs/findings/differential-log.md` reuses. |
| Constraints | `CAPTURE_SCHEMA "pinyin-capture-v1"` and `MAX_CAPTURED_CANDIDATES 10` are frozen; the harness "never calls training, remembering, choosing, or saving APIs" (capture-fixtures.md). Committed goldens `fixtures/foundation/f-a.txt` (15 records) and `f-c.txt` (46 records) are the F-A/F-C outputs under pin ref `libpinyin-2.11.92-074a2219…+model20-59c68e89…+dbm-tkrzw`; their SHA-256s are registered in `docs/testing/capture-fixtures.md`. |

### TLS-13 — tools/capture/run-capture.sh

| Field | Value |
|---|---|
| Entry point | executed: `tools/capture/run-capture.sh ORACLE_PREFIX OUTPUT_DIR` |
| Owning subsystem | capture / oracle provisioning |
| Purpose | Validates a pin-built oracle prefix (manifest schema, pin ref, DBM=Tkrzw, header/so/data-manifest SHA-256s, full data payload and unstable payload checksums), builds `capture.c` against it, and writes `f-a.txt`, `f-c.txt`, `zhuyin-parse.txt` into OUTPUT_DIR. |
| Implementation | bash, 110 lines; `sha256sum --check`, `realpath`, `cc`, `pkg-config`. |
| Inputs | `$prefix/oracle-pin.txt` (keys: `schema=pinyin-oracle-v1`, `pin_ref`, `dbm`, `header_sha256`, `shared_object_sha256`, `data_manifest_sha256`, optional `data_unstable_manifest_sha256`), `$prefix/oracle-data.sha256`, optional `oracle-data-unstable.sha256`, `include/libpinyin-2.11.92/pinyin.h`, `lib/libpinyin.so.15.0.0`, `lib/libpinyin/data`. Hard-coded `EXPECTED_PIN_REF` (tkrzw). Sets `PKG_CONFIG_PATH`, `LD_LIBRARY_PATH`. |
| Outputs | three capture files; stderr `fail` messages; exit 1 on any validation failure, 2 usage. |
| Platform | Linux. |
| Consumers | `docs/testing/capture-fixtures.md` (regeneration recipe → `fixtures/foundation`), `docs/testing/oracle-environment.md`, `tools/oracle/build-oracle.sh` comment, `crates/pinyin-oracle/src/pin_ref.rs` ("Identical to EXPECTED_PIN_REF"), `crates/oxpinyin-core/tests/zhuyin_parse_parity.rs`. Not in CI. |
| State | `target/capture/` build dir with fresh `user-f-a`, `user-f-c`, `user-zh` (rm -rf'd each run). |
| Overlap | oracle-pin.txt parser (`manifest_value`) is another ad-hoc `key=value` reader (cf. `tools/packaging/pc-version.sh` sed, `oracle-cell.sh` sed of build-oracle.sh constants). |
| Constraints | tkrzw cell only (`dbm == Tkrzw`, pin ref fixed); OUTPUT_DIR name `zhuyin-parse.txt` vs committed `fixtures/w4/oracle-zhuyin-parse.txt` (observation: names differ; not chased). |

### TLS-14 — tools/env-probe/probe-debuginfo-neutrality.sh

| Field | Value |
|---|---|
| Entry point | executed: `bash tools/env-probe/probe-debuginfo-neutrality.sh [backend-feature]` (default `bdb`) |
| Owning subsystem | perf (environment probe) |
| Purpose | Classifies whether adding debug info to a release build changes code: builds stock, `RUSTFLAGS=-Cdebuginfo=1` (control: silently stripped), and `CARGO_PROFILE_RELEASE_DEBUG=1`; compares `.text` bytes and instruction multisets. |
| Implementation | bash, 106 lines; cargo, objcopy, readelf, sha256sum; sources `tools/bisection/debuginfo-neutrality-lib.sh` (`compare_debuginfo_neutrality`). |
| Inputs | env `PKG` (default `oxpinyin-capi`); reads `[lib] name` from the crate's Cargo.toml; touches `crates/$PKG/src/lib.rs` to force rebuilds. |
| Outputs | sizes, `.debug_*` section counts, `.text` sha256 prefixes, CONFIRMED/NOTE on RUSTFLAGS control, then the lib's verdict (a) identical .text / (b) identical multiset / (c) different code. |
| Platform | Linux (ELF tools). |
| Consumers | `tools/bisection/run-callgrind-differential.sh` and `debuginfo-neutrality-lib.sh` reference it; redb-removal audit. Not in CI. |
| State | mktemp dir; writes into `target/release` (three builds). |
| Overlap | spawns `cargo build --locked --release -p $PKG --no-default-features --features $BACKEND` ×3; comparison logic deliberately shared with the differential driver's stage 2. |
| Constraints | documents that `RUSTFLAGS=-Cdebuginfo=1` is useless (cargo re-strips) and that profile debug changes link order/symbol hashes (layout-shifted simulated cache figures). |

### TLS-15 — tools/env-probe/probe-egress.sh

| Field | Value |
|---|---|
| Entry point | executed: `bash tools/env-probe/probe-egress.sh` |
| Owning subsystem | ci / perf (environment probe) |
| Purpose | Checks that the nine hosts a perf/oracle image build needs (deb.debian.org, snapshot.debian.org, Docker Hub CDN/registry, mirror.gcr.io, static.rust-lang.org, index/static.crates.io, github.com) answer; 000/403/407 = DENIED. |
| Implementation | bash + curl + optional python3, 93 lines. |
| Inputs | env `HTTPS_PROXY` (queries `${HTTPS_PROXY}/__agentproxy/status` for recorded denials). |
| Outputs | table `HOST CODE VERDICT NEEDED-FOR`; `RESULT:` line; exit 1 if any denied, else 0. |
| Platform | portable (network). |
| Consumers | none found in the tree. |
| State | none |
| Overlap | none |
| Constraints | "do not route around an egress policy"; probes real paths, never bare roots. |

### TLS-16 — tools/env-probe/probe-perf-event-open.c

| Field | Value |
|---|---|
| Entry point | `gcc -O0 -o probe-perf-event-open probe-perf-event-open.c && ./probe-perf-event-open` |
| Owning subsystem | perf (environment probe) |
| Purpose | Opens one `PERF_COUNT_HW_INSTRUCTIONS` counter to distinguish EACCES/EPERM (paranoid policy) from ENOENT (no PMU: VMs, Apple-silicon containers → use callgrind). |
| Implementation | C, 36 lines; `linux/perf_event.h`, raw `syscall(SYS_perf_event_open)`. |
| Inputs | none |
| Outputs | `perf_event_open OK fd=N` exit 0, or `FAILED: <strerror> (errno N)` exit 1. |
| Platform | Linux-only. |
| Consumers | none found. |
| State | none |
| Overlap | the fallback order in `run-w8-cycle.sh` (samply/perf → callgrind) is the policy this probe informs. |
| Constraints | "do not escalate on a shared box". |

### TLS-17 — tools/store/backend-matrix.sh

| Field | Value |
|---|---|
| Entry point | executed: `tools/store/backend-matrix.sh` |
| Owning subsystem | testsupport / ci (store) |
| Purpose | Proves the exactly-one-backend invariant of `oxpinyin-store`: default and each of `kyotocabinet`, `tkrzw`, `bdb` `cargo check` green; every pair, the triple, and zero backends refused by the crate's `compile_error!` ("more than one store backend selected" / "no store backend selected"). |
| Implementation | bash, 91 lines; `cargo check --locked -p oxpinyin-store`. |
| Inputs | none (env none). |
| Outputs | `── valid/invalid:` sections with PASS/FAIL, `summary: N passed, M failed`; exit 0 iff no failures (`set -u`, no `-e`). |
| Platform | portable (check only; no C libs needed to link). |
| Consumers | `.github/workflows/store-backends.yml` job `backend-matrix` (independent job; `store-backends-gate` needs it); `docs/runbooks/backends.md`; findings. |
| State | one `mktemp -t backend-matrix.XXXXXXXX` log, trap-removed. |
| Overlap | spawns cargo 9×; backend-name list hard-coded (kyotocabinet, tkrzw, bdb; label "default (bdb)"). |
| Constraints | greps the exact compile_error strings from `crates/oxpinyin-store/src/lib.rs`. |

### TLS-18 — tools/tkrzw/distro-probe.sh

| Field | Value |
|---|---|
| Entry point | executed (POSIX sh): `tools/tkrzw/distro-probe.sh` |
| Owning subsystem | oracle provisioning / store (tkrzw) |
| Purpose | Does the system's libtkrzw honour its pointer-identity protocol? static (GOT relocations for `KeyComparator` in libtkrzw), dynamic (create/set/get via `tkrzw_dbm_util` for tkh/tkt/tks), remove (REMOVE sentinel not left behind), and the TreeDBM comparator type byte (offset 53 of the `TDB` block; 1 lexical, 255 unnameable). |
| Implementation | POSIX sh, 126 lines; `tkrzw_dbm_util`, `tkrzw_build_util`, ldd, readelf, od, dpkg-query/pacman/rpm. |
| Inputs | `/etc/os-release`; env `TMPDIR`. |
| Outputs | `distro/tkrzw/package/static/tkh/tkt/tks/remove/tkt/RESULT` lines; exit 0 healthy, 1 broken, 2 tkrzw not found. |
| Platform | Linux (distro package managers; `-Bsymbolic-functions`/LTO breakage is Ubuntu/Arch specific). |
| Consumers | `tools/tkrzw/libpinyin-backend-probe.sh` (comment), `docs/findings/tkrzw-distro-compat.md`, `crates/oxpinyin-store/build.rs`. |
| State | mktemp dir; `trap … 0` + signal traps exit 1. |
| Overlap | `identity-probe.cc` is the library-API counterpart (same three properties from a client TU). |
| Constraints | tkrzw ≥1.0.34 passes the remove check even under LTO. |

### TLS-19 — tools/tkrzw/identity-probe.cc

| Field | Value |
|---|---|
| Entry point | `g++ -std=c++17 -O2 -I PREFIX/include identity-probe.cc -o identity-probe -L PREFIX/lib -Wl,-rpath,PREFIX/lib -ltkrzw -llzma -llz4 -lzstd -lz -lpthread; ./identity-probe DIR` |
| Owning subsystem | store (tkrzw) |
| Purpose | Five rows from a client TU: (a) TreeDBM default comparator reopen, (b) client-passed `LexicalKeyComparator` reopen (breaks under `-Bsymbolic-functions`), (c) NOOP-returning processor leaves value intact, (d) `Remove()` really removes, (e) `Rebuild()` (c–e break under LTO <1.0.34). |
| Implementation | C++17, 150 lines; tkrzw C++ API (`tkrzw_dbm_hash.h`, `tkrzw_dbm_tree.h`). |
| Inputs | DIR for `a.tkt`, `b.tkt`, `c.tkh`. |
| Outputs | one line per row with OK/CORRUPTED/BROKEN; exit 0 all healthy, 1 otherwise, 2 usage. |
| Platform | portable C++ in principle; Linux in practice. |
| Consumers | `docs/findings/tkrzw-distro-compat.md`. |
| State | files in DIR (not cleaned). |
| Overlap | TDB byte-53 reader duplicated from distro-probe.sh. |
| Constraints | none beyond the documented breakage matrix. |

### TLS-20 — tools/tkrzw/libpinyin-backend-probe.sh

| Field | Value |
|---|---|
| Entry point | executed (POSIX sh): `tools/tkrzw/libpinyin-backend-probe.sh` |
| Owning subsystem | oracle provisioning |
| Purpose | Which DBM backend does this system's libpinyin link? Reads `libpinyin.so`'s DT_NEEDED (readelf/objdump, not ldd) and maps libtkrzw/kyotocabinet/libdb → Tkrzw/KyotoCabinet/BerkeleyDB. |
| Implementation | POSIX sh, 60 lines; ldconfig, find, rpm/dpkg-query/pacman, readelf or objdump. |
| Inputs | `/etc/os-release`, installed libpinyin. |
| Outputs | `distro/so/version/backend/needed` lines; exit 0 inspected, 2 not found, 3 could not inspect. |
| Platform | Linux. |
| Consumers | `docs/findings/tkrzw-distro-compat.md`, `backend-selection-audit.md`. |
| State | none |
| Overlap | DT_NEEDED scanning also in `release-deb.sh` (`objdump -p … NEEDED`) and `release-stage.sh` (`readelf -d` SONAME). Hard-codes backend soname fragments `libtkrzw|kyotocabinet|libdb-|libdb5|libdb\.so`. |
| Constraints | never silently classifies an unreadable ELF as unknown. |

### TLS-21 — tools/kc/run-sanitizers.sh

| Field | Value |
|---|---|
| Entry point | executed: `tools/kc/run-sanitizers.sh` |
| Owning subsystem | store (kyotocabinet) / ci |
| Purpose | Runs `cargo +nightly test -p oxpinyin-store --features kyotocabinet` under `-Zsanitizer=address` (LSan included) to catch leak/double-free/wrong-allocator of `kcdbget`/`kccurget*` regions that must go through `kcfree`. |
| Implementation | bash, 46 lines; rustup nightly. |
| Inputs | env `OXPINYIN_SANITIZER_TOOLCHAIN` (default `nightly-2026-08-01`), `OXPINYIN_SANITIZER_TARGET` (default `x86_64-unknown-linux-gnu`), `OXPINYIN_SANITIZER_ALLOW_SKIP` (set → exit 0 with "SKIP … the gate did NOT run"). |
| Outputs | cargo test output; `sanitizers: clean (…)`; exit 0 clean, 1 toolchain missing (unless ALLOW_SKIP), non-zero on findings. |
| Platform | Linux x86_64 default. |
| Consumers | **None executes it.** `store-backends.yml` mentions it only in comments (lines 82/100); there is no kc sanitizer job — only `tkrzw-sanitizers` (inline RUSTFLAGS) and `bdb-sanitizers` (runs `tools/bdb/run-sanitizers.sh`). Referenced by `docs/findings/kyotocabinet-backend.md`. |
| State | none |
| Overlap | sibling of `tools/bdb/run-sanitizers.sh` (different default nightly: 2026-08-01 vs 2026-09-07; different target default: fixed x86_64 vs `rustc -vV` host; this one lacks `-Cdebuginfo=1` and uses `--features kyotocabinet` without `--no-default-features`). |
| Constraints | self-skip via `OXPINYIN_SANITIZER_ALLOW_SKIP` contradicts AGENTS.md "Do not add a self-skipping test" in spirit (observation). Library internals not instrumented. |

### TLS-22 — tools/bdb/run-sanitizers.sh

| Field | Value |
|---|---|
| Entry point | executed: `tools/bdb/run-sanitizers.sh` |
| Owning subsystem | store (bdb) / ci |
| Purpose | Two halves: (1) `cargo +nightly test -p oxpinyin-store --no-default-features --features bdb` under `-Zsanitizer=address -Cdebuginfo=1`; (2) C harnesses `hash-walk.c` and `btree-order.c` under `-fsanitize=address,undefined -fno-sanitize-recover=all`, walking a `bigram.db` the Rust backend itself wrote (`--example bdb_write_profile`) and, if present, the installed libpinyin `bigram.db`. |
| Implementation | bash, 100 lines; rustup nightly, gcc `-ldb`. |
| Inputs | env `OXPINYIN_SANITIZER_TOOLCHAIN` (default `nightly-2026-09-07`), `OXPINYIN_SANITIZER_TARGET` (default host triple), `OXPINYIN_LIBPINYIN_DATA_DIR` (else searches five distro data paths), `CARGO_TARGET_DIR`. |
| Outputs | step banners, hash-walk verdict tables, `sanitizers: clean (libdb's own internals are NOT instrumented)`; exit 1 if nightly missing; non-zero on any finding (`set -e`). |
| Platform | Linux (libdb, nightly sanitizers). |
| Consumers | `store-backends.yml` job `bdb-sanitizers` (`bash tools/bdb/run-sanitizers.sh`); `docs/findings/berkeleydb-backend.md`. |
| State | mktemp work dir with `hash-walk`, `btree-order`, `user_bigram.db`, `btree-order.db`. |
| Overlap | spawns cargo twice (test, build --example); sibling of `tools/kc/run-sanitizers.sh`; no ALLOW_SKIP (fails hard). |
| Constraints | "Load-bearing rather than precautionary: on a BerkeleyDB distro the system bigram.db is BDB". |

### TLS-23 — tools/bdb/btree-order.c

| Field | Value |
|---|---|
| Entry point | compiled by run-sanitizers.sh; `./btree-order [path]` |
| Owning subsystem | store (bdb) |
| Purpose | Confirms DB_BTREE's DEFAULT comparator order over raw little-endian u32 array keys (12 one-element keys + a two-element sibling each), opened exactly as libpinyin does (NULL env, NULL txn, no `set_bt_compare`): raw-byte order, neither integer nor big-endian. |
| Implementation | C, 78 lines; libdb. |
| Inputs | optional path (default `btree-order.db`, unlinked first). |
| Outputs | table `# key-bytes decoded-LE-u32`; exit 0 at DB_NOTFOUND, 1 on any libdb error. |
| Platform | Linux/libdb. |
| Consumers | run-sanitizers.sh; `docs/findings/store-key-ordering.md`, `berkeleydb-backend.md`. |
| State | writes the db file. |
| Overlap | none |
| Constraints | "the evidence behind berkeleydb-backend.md's ordering claim". |

### TLS-24 — tools/bdb/hash-walk.c

| Field | Value |
|---|---|
| Entry point | compiled by run-sanitizers.sh; `./hash-walk FILE` |
| Owning subsystem | store (bdb) |
| Purpose | Walks a libpinyin `bigram.db` (DB_HASH, read-only) with libdb directly and checks every SingleGram invariant: key 4 bytes; value `4 + 8n`; item tokens ascending; `total_freq == sum(item freq)`; no zero-items-nonzero-total. |
| Implementation | C, 88 lines; libdb. |
| Inputs | FILE. |
| Outputs | first 3 records dumped, `LAYOUT VERDICT` counter table, `VERDICT: ok|FAIL`; exit 0 ok, 1 any violation or cursor failure, 2 usage. |
| Platform | Linux/libdb. |
| Consumers | run-sanitizers.sh; `crates/oxpinyin-store/examples/bdb_write_profile.rs`, `tests/bdb_libpinyin_files.rs`, `docs/findings/{berkeleydb-backend,compatibility-policy}.md`. |
| State | none |
| Overlap | the `(size-4)/8` layout is the same one the Rust bdb backend decodes. |
| Constraints | "Ground truth with no Rust involved". |

### TLS-25 — tools/packaging/pc-version.sh

| Field | Value |
|---|---|
| Entry point | executed: `tools/packaging/pc-version.sh` |
| Owning subsystem | packaging |
| Purpose | Prints the drop-in's libpinyin version from `tools/oracle/oracle-pin.txt` `libpinyin_tag=` — the single source for `.pc` Version, `libpinyin-<ver>` include dir, and package Provides. |
| Implementation | bash, 23 lines; sed/grep. |
| Inputs | `tools/oracle/oracle-pin.txt`. |
| Outputs | version on stdout; exit 1 if no well-formed `x.y.z` line. |
| Platform | portable |
| Consumers | `release-{stage,deb,rpm,arch}.sh`, `check-pc-version.sh`, `release-packages.yml`, `ci.yml`, `runners.json` (`pc-version` gate wraps check-pc-version.sh), docs. |
| State | none |
| Overlap | the capi build scripts (`crates/oxpinyin-capi/build_pin_version.rs`) read the same line and fail the build on manifest disagreement. |
| Constraints | parses `oracle-pin.txt` (only the `libpinyin_tag` key). |

### TLS-26 — tools/packaging/check-pc-version.sh

| Field | Value |
|---|---|
| Entry point | executed: `tools/packaging/check-pc-version.sh` |
| Owning subsystem | packaging / ci |
| Purpose | Static CI gate (no pin needed): every `"libpinyin-<ver>"` string and `[package.metadata.capi.pkg_config] version` in both capi Cargo.toml files equals `pc-version.sh`, and both `.pc.in` templates use `@VERSION@` rather than a literal version. |
| Implementation | bash, 50 lines; grep/awk. |
| Inputs | `crates/oxpinyin-capi/Cargo.toml`, `crates/oxpinyin-zhuyin-capi/Cargo.toml`, `libpinyin.pc.in`, `libzhuyin.pc.in`. |
| Outputs | `error:` lines; `OK: every drop-in version declaration is <ver> (tools/oracle/oracle-pin.txt)`; exit 1 on any disagreement. |
| Platform | portable |
| Consumers | `ci.yml` lint job; `runners.json` `pc-version` (gate, oracle false); release runbook step 5. |
| State | none |
| Overlap | the build-time half lives in the crates' build scripts; the full installed-tree diff is `check-pc-metadata.sh`. |
| Constraints | documents that the pin-built libpinyin is unavailable in CI, so this is the CI-checkable half. |

### TLS-27 — tools/packaging/check-pc-metadata.sh

| Field | Value |
|---|---|
| Entry point | executed: `check-pc-metadata.sh --pin-root=DIR --subject-root=DIR --prefix=DIR [--libdir=] [--includedir=] [--build-subject=FEATURES] [--modules="libpinyin libzhuyin"]` |
| Owning subsystem | packaging |
| Purpose | Diffs oxpinyin's installed packaging metadata against the pin's `make install DESTDIR` tree: `.pc` byte-for-byte, include-dir file set, and pkg-config `--modversion/--cflags/--libs/--print-variables/--variable=*` as installed and relocated with `--define-variable=prefix=/relocated-by-check-pc-metadata`, plus `--atleast-version`/`--exact-version` of the pin's modversion. Roots normalised to `<root>`. |
| Implementation | bash, 189 lines; pkg-config, diff; sources `oracle-cell.sh`; optionally runs `install.sh` twice (`--profile dev --locked --no-default-features --features FEATURES[,shipped]`). |
| Inputs | pin and subject roots; env `PINYIN_ORACLE_DBM` (derived from `--build-subject` when given: kyotocabinet→kc), `PKG_CONFIG_PATH` set per query, and oracle-cell.sh's vars. |
| Outputs | `== include directory`, `== <mod>` sections with unified diffs labelled `pin:`/`oxpinyin:`; `OK:`/`FAIL: N item(s) differ`; exit 0/1; 2 usage, missing pkg-config, or pin tree unresolvable. |
| Platform | Linux (pin tree). |
| Consumers | `runners.json` `pc-metadata` (gate, oracle **true**, env `OXPINYIN_METADATA_PIN_ROOT`/`_SUBJECT_ROOT`/`_PREFIX`); `ci.yml` comment (runs locally only); `docs/packaging.md`, `goldens-and-pins.md`, `installed-naming.md`. |
| State | mktemp work dir; `--build-subject` rm -rf's the subject root. |
| Overlap | second script that sources oracle-cell.sh for cell selection without building a cdylib; install.sh invocation pattern shared with release-stage.sh and snapshot.sh. |
| Constraints | "Both trees must carry the same backend; the default build of each is Berkeley DB". |

### TLS-28 — tools/packaging/install.sh

| Field | Value |
|---|---|
| Entry point | executed: `install.sh <libpinyin|libzhuyin> --prefix=DIR [--libdir=] [--includedir=] [--destdir=] [-- <cargo cinstall args>]` |
| Owning subsystem | packaging |
| Purpose | The ONLY supported install path: runs `cargo cinstall` for one crate, fills the build.rs-baked `<name>.pc.in.baked` with `@prefix@/@exec_prefix@/@libdir@/@includedir@` the way autoconf would, overwrites cargo-c's incomplete `.pc`, then (Linux) relinks the installed `.so.15.0.0` from the staticlib under the `.ver` script via `relink-versioned.sh`. |
| Implementation | bash, 342 lines; cargo-c, rustc, sed, cmp, find. |
| Inputs | companion headers `novel_types.h`, `pinyin_custom2.h` in both crates (must be byte-identical); baked template at `<target-dir>/<triple>/<profile>/<name>.pc.in.baked` or unique `build/*/out/<name>`; env `CARGO_TARGET_DIR`, `CARGO_BUILD_TARGET`, `LIBPINYIN_DATABASE_FORMAT` (documented override), `DESTDIR` via flag; passthrough parsed for `--target-dir`, `--target`, `--profile`, `--release`, `--debug`, `--features` (collected for relink). |
| Outputs | installed tree; `installed complete pkg-config file: …`; relink summary; exit 0; 2 usage; 1 on missing baked template/ambiguous candidates/divergent headers/target unresolvable. |
| Platform | Linux relinks; macOS installs the cdylib as built (no relink). |
| Consumers | `release-packages.yml`, `release-stage.sh`, `snapshot.sh`, `check-pc-metadata.sh`, `tools/bisection/run-shipped-check.sh`, benches runbook recipes, both capi `build.rs`, `companion_headers.rs` test, many findings. |
| State | writes under `$TARGET_DIR/<triple>/<profile>/` and `${DESTDIR}${LIBDIR}/pkgconfig`. |
| Overlap | companion-header check duplicated in `release-stage.sh`; cargo spawn (`cargo cinstall … --target-dir … ${DESTDIR:+--destdir}`); .pc substitution values documented as the pin's configure outputs. |
| Constraints | one library per invocation; `exec_prefix` always `${prefix}`; DESTDIR never baked into the .pc; SONAME `lib<name>.so.15`, real file `.so.15.0.0`; staticlib stem `libpinyin.a`/`libzhuyin.a` (cargo-c renames). Verified against cargo-c 0.10.24 source (closed 7-key pkg_config set, no prefix env, `.pc` installed first). |

### TLS-29 — tools/packaging/relink-versioned.sh

| Field | Value |
|---|---|
| Entry point | executed by install.sh: `relink-versioned.sh --staticlib PATH --ver PATH --soname NAME --dest PATH [--features LIST] [--cc CC]` |
| Owning subsystem | packaging / abi |
| Purpose | Relinks a drop-in `.so` from the cargo staticlib with `-Wl,--version-script=<.ver>` and `-Wl,-soname` so every export is versioned (`pinyin_init@@LIBPINYIN`), which rustc's anonymous cdylib version script prevents; verifies export set == .ver globals (both directions), no unversioned exports, SONAME correct, then `mv -f` over the installed file. |
| Implementation | bash, 171 lines; `cc -shared -Wl,--whole-archive`, nm, readelf, pkg-config. |
| Inputs | staticlib, .ver, soname, dest; env `OXPINYIN_RELINK_FEATURES` (default for `--features`), `CC` (default `cc`). Backend link flags by feature precedence tkrzw > kyotocabinet > bdb (default bdb): `pkg-config --libs tkrzw|kyotocabinet` or `-ltkrzw`/`-lkyotocabinet`/`-ldb`; glib via `pkg-config --libs glib-2.0 gobject-2.0`; `-lpthread -ldl -lm`. |
| Outputs | `relinked <dest>` + summary line; exit 0; 1 non-Linux/missing tool/file/export mismatch/unversioned symbol/SONAME mismatch; 2 usage. |
| Platform | Linux + GNU ld only. |
| Consumers | install.sh; both capi `build.rs` comments; `docs/findings/drop-in-abi-identity.md`, `docs/packaging.md`. |
| State | `mktemp "${DEST%.so*}.relink.XXXXXX"` beside dest; two symbol-list temp files. |
| Overlap | third .ver parser (sed form) alongside `check-exports.sh`/`check-alloc-pairing.sh` awk form; hard-codes backend names and link libs. |
| Constraints | version node = first word of the .ver (`LIBPINYIN`/`LIBZHUYIN`); the node's own dynsym entry is filtered as GNU ld's verdef carrier. |

### TLS-30 — tools/packaging/release-stage.sh

| Field | Value |
|---|---|
| Entry point | executed: `release-stage.sh <kyotocabinet|tkrzw|bdb> --prefix=DIR --libdir=DIR [--dest=DIR]` |
| Owning subsystem | packaging |
| Purpose | Builds both cdylibs under one backend through install.sh (`--release --locked --no-default-features --features <backend>[,shipped]`), stages the complete install tree, strips, and gates: expected 15-file layout, nothing extra, SONAME `lib*.so.15`, pkg-config `database_format`/`libpinyin_binary_version`=15.0/modversion=PCVER/`--libs`/`--cflags`/`pkgdatadir` (libpinyin only), and a C compile/link/run smoke per library. |
| Implementation | bash, 241 lines; cargo-c, strip, readelf, pkg-config, cc, cmp. |
| Inputs | `pc-version.sh`; companion headers; `db_format_for` map (kyotocabinet→KyotoCabinet, tkrzw→Tkrzw, bdb→BerkeleyDB — "must mirror build.rs's database_format()"). Env: `PKG_CONFIG_PATH` set to staged dir. |
| Outputs | staged tree listing; `== release-stage gates passed (backend: X) ==`; exit 0 only after all gates; 1 `GATE FAILED:`/missing tool; 2 usage or `--dest` colliding with a real root. |
| Platform | Linux. |
| Consumers | `release-packages.yml` (`tools/packaging/release-stage.sh "$BACKEND" …`), `docs/runbooks/release.md`, `tools/bisection/run-rss-diag.sh`, findings. |
| State | `rm -rf --dest` (default `target/release-stage/<backend>`); mktemp smoke dir. |
| Overlap | companion-header check duplicated from install.sh; `.so.15` naming duplicated across install.sh/relink/release-*; the smoke C programs resemble the workflow's post-install init check. |
| Constraints | usage text says `kyotocabinet | tkrzw` but the case accepts bdb too (observation); "mirroring upstream byte-for-byte is the contract" for `.pc` variables; `<stdbool.h>` must precede the headers; the two public headers cannot link in one program. |

### TLS-31 — tools/packaging/release-deb.sh

| Field | Value |
|---|---|
| Entry point | executed: `release-deb.sh <backend> <version> <stage-root> <libdir> <outdir>` |
| Owning subsystem | packaging |
| Purpose | Splits the staged tree into `oxpinyin-libpinyin15-<backend>` (runtime: `.so.15.0.0` + `.so.15` links; Provides/Conflicts/Replaces `libpinyin15`, `libzhuyin15` at PCVER; Recommends `libpinyin-data`) and `…-dev` (`.so` links, `.a`, `.pc`, headers; Provides/Conflicts/Replaces `libpinyin15-dev`, `libzhuyin-dev`). Depends computed from DT_NEEDED → `ldconfig -p` → `dpkg -S`, plus `libc6 (>= highest GLIBC_*)`. |
| Implementation | bash, 184 lines; dpkg, objdump, ldconfig, dpkg-deb, md5sum. |
| Inputs | staged tree, `pc-version.sh`. |
| Outputs | two `.deb` files in outdir; `== debs written to …`; exit 1 on missing `.so`, unowned soname, no owning package. |
| Platform | Debian/Ubuntu container. |
| Consumers | `release-packages.yml` (debian lane, backend tkrzw), release runbook. |
| State | mktemp tree. |
| Overlap | DT_NEEDED parsing (cf. libpinyin-backend-probe.sh); hard-coded maintainer/homepage; copyright/md5sums helpers. |
| Constraints | Package version = oxpinyin `0.x` tag `-1`; consumer-facing version = PCVER (the "four version streams"). Does not touch `libpinyin-data`. |

### TLS-32 — tools/packaging/release-rpm.sh

| Field | Value |
|---|---|
| Entry point | executed: `release-rpm.sh <backend> <version> <stage-root> <outdir>` |
| Owning subsystem | packaging |
| Purpose | Builds Fedora RPMs `oxpinyin-libpinyin-<backend>` (+`-devel`) from the staged tree as Source0 (no compile): Obsoletes `libpinyin libzhuyin` (`-devel`: `libpinyin-devel libzhuyin-devel`), Provides `libpinyin = PCVER libzhuyin = PCVER`; soname Provides and Requires come from rpm's ELF scanner. |
| Implementation | bash + generated spec, 124 lines; tar, rpmbuild, rpm. |
| Inputs | staged tree, `pc-version.sh`. |
| Outputs | RPMs copied to outdir after `rpm -qlp` check; exit 1 if none produced. |
| Platform | Fedora container. |
| Consumers | `release-packages.yml` (fedora lane, kyotocabinet), release runbook. |
| State | mktemp `_topdir`. |
| Overlap | sibling of release-deb/release-arch; `%changelog` date hard-coded "Sat Sep 06 2026". |
| Constraints | Obsoletes, not Conflicts ("a package may not Conflict with a name it also Provides"); `%define debug_package %{nil}`. |

### TLS-33 — tools/packaging/release-arch.sh

| Field | Value |
|---|---|
| Entry point | executed (as root): `release-arch.sh <kyotocabinet|tkrzw> <version> <stage-root> <outdir> [--data=DIR] [--data-version=VER]` |
| Owning subsystem | packaging |
| Purpose | Builds ONE Arch package `oxpinyin-libpinyin-<backend>` via a generated PKGBUILD and `makepkg --nodeps` as an unprivileged `oxp-builder` user; provides `libpinyin=PCVER libzhuyin=PCVER libpinyin.so=15-64 libzhuyin.so=15-64`, conflicts `libpinyin libzhuyin`; optionally bundles `--data=DIR` (must hold `table.conf`) as `/usr/lib/libpinyin/data` because Arch's libpinyin package carries the model. |
| Implementation | bash, 137 lines; useradd, chown, su, makepkg. |
| Inputs | staged tree, `pc-version.sh`, data dir. |
| Outputs | `<outdir>/<pkg>-<ver>-1-<arch>.pkg.tar.zst`; exit 1 missing stage/data/table.conf/package; 2 bad arg or `--data-version` without `--data`. |
| Platform | Arch container, root. |
| Consumers | `release-packages.yml` (arch lane, kyotocabinet, always passes `--data`), release runbook. |
| State | mktemp work dir chmod 0711, chown'd to builder; creates a system user. |
| Overlap | sibling of deb/rpm makers; checks `table.conf` presence (the only place in this set that touches `table.conf`). |
| Constraints | backend restricted to kyotocabinet|tkrzw (`EXTRA_DEPS` map); `options=(!debug)`; without `--data` "the result cannot initialise on a system whose libpinyin it replaced". |

### TLS-34 — tests/lint-commits.test.sh

| Field | Value |
|---|---|
| Entry point | executed (POSIX sh): `tests/lint-commits.test.sh` |
| Owning subsystem | ci |
| Purpose | Self-contained harness for `.github/scripts/lint-commits.sh`: builds a throwaway git repo, creates fixture commits (valid `Assisted-by: Claude:claude-opus-5`, `Grok:4.6` regression ba25ff7, placeholders, duplicates, trailing token, case variants, merge commit skipped, multi-commit range names the SHA) and asserts exit code + `R2` presence; plus hook/CI parity through `.githooks/commit-msg`. |
| Implementation | POSIX sh + git, 300 lines; unsets `GITHUB_STEP_SUMMARY`. |
| Inputs | `.github/scripts/lint-commits.sh`, `.githooks/commit-msg`. |
| Outputs | TAP-ish `ok - …`/`not ok - …`, summary; exit 0 iff no failures. |
| Platform | portable (git ≥2.32). |
| Consumers | `.github/workflows/commit-trailers.yml` job `trailer-test`. |
| State | `TMPDIR=$(mktemp -d)` repo (note: overrides the `TMPDIR` env name), trap-removed. |
| Overlap | harness shape shared with `lint-crate-cfg-placement.test.sh`. |
| Constraints | encodes R2 only (R1/R4 retired: extra trailers and agent-email authors pass). |

### TLS-35 — tests/lint-crate-cfg-placement.test.sh

| Field | Value |
|---|---|
| Entry point | executed (POSIX sh): `tests/lint-crate-cfg-placement.test.sh` |
| Owning subsystem | ci |
| Purpose | Fixture `.rs` files proving the linter accepts docs-above-gate, gate-only, docs-only, `cfg_attr`, nested-module `//!`; rejects (exit exactly 1, with `file:line` and `on line N`) gate-above-docs even at distance and among clean files; usage errors exit exactly 2 (`--help`, missing path, directory, missing path among valid). |
| Implementation | POSIX sh, 215 lines. |
| Inputs | `.github/scripts/lint-crate-cfg-placement.sh`. |
| Outputs | TAP-ish lines, `1..N`, exit 0/1. |
| Platform | portable |
| Consumers | `ci.yml` lint job (`tests/lint-crate-cfg-placement.test.sh` then the linter itself); listed in the `changes` job's path filter. |
| State | mktemp dir. |
| Overlap | with TLS-34. |
| Constraints | asserts exit 1 vs 2 distinctly ("accepting [2] would let a violation assertion pass on a script that never reached its scan"). |

### TLS-36 — .github/scripts/lint-commits.sh

| Field | Value |
|---|---|
| Entry point | executed: `lint-commits.sh <base-sha> <head-sha>` (CI, non-merge commits in range) or `lint-commits.sh --hook <message-file>` (commit-msg hook) |
| Owning subsystem | ci |
| Purpose | Single source of truth for R2 — `Assisted-by:` house form when present: shape `^Assisted-by: [[:alnum:]][[:alnum:]._-]*:[[:alnum:]][[:alnum:].+_-]*$` (nothing after the model), MODEL token has ≥1 ASCII letter, no placeholder (`AGENT`, `AGENT_NAME`, `MODEL`, `MODEL_VERSION`, `[`/`]`), no duplicate lines. Trailer key matched case-insensitively. |
| Implementation | POSIX sh + git ≥2.32 (`%(trailers:only,unfold)`, `git interpret-trailers --parse`), 205 lines. |
| Inputs | git history or message file; env `GITHUB_STEP_SUMMARY` (writes a per-commit table). |
| Outputs | `::error::<sha> <subject>: R2 — <detail>` lines; `::notice::` on empty range; markdown summary table `| commit | subject | R2 (Assisted-by) |`; exit 0 pass, 1 lint failure, 2 usage/env (bad sha, old git). |
| Platform | portable |
| Consumers | `commit-trailers.yml` job `trailer-lint` (`lint-commits.sh "${MB}" "${HEAD_SHA}"`), `.githooks/commit-msg` (`--hook`), `tests/lint-commits.test.sh`, `docs/findings/commit-hygiene-failures.md`; AGENTS.md "Attribution" describes R2. |
| State | none |
| Overlap | none |
| Constraints | regex constants are the frozen rule text (AGENTS.md: `Grok:4.6` fails, `Grok:grok-4.6` passes). |

### TLS-37 — .github/scripts/lint-crate-cfg-placement.sh

| Field | Value |
|---|---|
| Entry point | executed: `lint-crate-cfg-placement.sh [file...]` (no args → `git ls-files -- '*.rs'`) |
| Owning subsystem | ci |
| Purpose | Rejects a column-0 crate-level `#![cfg(...)]` written above the crate's `//!` docs (a false gate discards following crate attributes, so `missing_docs` fires on macOS/Windows where CI cannot see it). `#![cfg_attr]` deliberately not matched. |
| Implementation | POSIX sh + awk + git, 115 lines. |
| Inputs | tracked `*.rs` or named paths (named paths must be regular files; tracked-but-missing paths are skipped). |
| Outputs | `crate-cfg-placement: N file(s) checked, no violations` or violation list `file:docline: … on line gateline` with explanation; exit 0/1/2. |
| Platform | portable |
| Consumers | `ci.yml` lint job; `changes` path filter; `tests/lint-crate-cfg-placement.test.sh`. |
| State | none |
| Overlap | none |
| Constraints | relies on rustfmt indenting nested-module inner docs (column-0 scan). |

### TLS-38 — crates/oxpinyin-core/benches/parse_and_graph.rs

| Field | Value |
|---|---|
| Entry point | `cargo bench -p oxpinyin-core --bench parse_and_graph` (`[[bench]] name="parse_and_graph" harness=false`; no required-features) |
| Owning subsystem | perf |
| Purpose | Latency baseline for the pure decode chain: full-pinyin parse (8 inputs) and parse_with_options (INCOMPLETE\|CORRECT_ALL\|AMB_ALL), `SegmentGraph::build` (6 inputs incl. `zhongguorenminjiefangjun`), `fewest_keys` complete/incomplete, `k_best` (k=1/4/8, unit edge cost), `SyllableKey::from_text` hit/miss and `from_option_text` aliases, `expand_keys` (complete/one/two incomplete), `fuzzy_alternatives`. |
| Implementation | Rust, criterion, 277 lines; no I/O, no data. |
| Inputs | none (frozen syllable inventory). |
| Outputs | criterion groups `core_parse`, `core_parse_with_options`, `core_graph_build`, `core_graph_build_with_options`, `core_fewest_keys`, `core_k_best`, `core_syllable_key`, `core_expand_keys`, `core_fuzzy`. |
| Platform | portable |
| Consumers | `docs/perf/perf-decode-path-baseline-2026-09-19.md` (first run, PR #470). Not CI. |
| State | none |
| Overlap | none |
| Constraints | none |

### TLS-39 — crates/oxpinyin-engine/benches/session_keystroke.rs

| Field | Value |
|---|---|
| Entry point | `cargo bench -p oxpinyin-engine --bench session_keystroke` (no required-features) |
| Owning subsystem | perf |
| Purpose | End-to-end engine keystroke path on fixture doubles: `Session::new` (empty backends vs fixture backends), single keystroke (`first_char_n`, `completing_nihao`), full compositions + Enter (ni/nihao/nihaoshijie), incremental typing (5 and 11 keystrokes), backspace. Uses `FrequencyFixtureModel` so `has_real_unigrams()==true` and the production window-scan path is taken. |
| Implementation | Rust, criterion, 246 lines; `oxpinyin-testsupport` fixtures. |
| Inputs | `include_str!` of `fixtures/w4/mini-vocab.txt`, `fixtures/w4/mini-bigram.txt`; `StoragePaths::new("/tmp/oxpinyin-bench")`. |
| Outputs | groups `engine_session_new`, `engine_process_key`, `engine_full_composition`, `engine_incremental_typing`, `engine_backspace`. |
| Platform | portable |
| Consumers | `perf-decode-path-baseline-2026-09-19.md`. Not CI. |
| State | hard-coded `/tmp/oxpinyin-bench` storage path. |
| Overlap | none |
| Constraints | asserts the model reports real unigrams. |

### TLS-40 — crates/oxpinyin-store/benches/backend_matrix_bdb.rs

| Field | Value |
|---|---|
| Entry point | `cargo bench -p oxpinyin-store --no-default-features --features bdb --bench backend_matrix_bdb [-- --vmhwm]`; `required-features = ["bdb"]`; `#![cfg(feature="bdb")]` |
| Owning subsystem | perf (store tier) |
| Purpose | 16-line shim: `support::run::<BdbStore>("bdb")`. Workload (support/mod.rs): `backend_matrix/init_load` (open the six w3 system DBMs — five tree opens, hash open for `bigram` — all held), `train_write/{64,256}` (N bigram + N phrase rows in one write txn, store created per iteration), `observe_commit` (one `UserStore::update`-shaped four-counter RMW commit on a pre-populated store), `train_sentence/8` (eight such commits), `user_db_open` (open a fresh copy of a populated store). `--vmhwm` mode re-execs per op and reports `/proc/self/status` VmHWM. |
| Implementation | Rust, criterion + `#[path="support/mod.rs"]`, 16 + ~570 shared lines. |
| Inputs | `fixtures/w3/<DEFAULT_STORE_EXT>` (`db`); env `BACKEND_MATRIX_POPULATED` (child op `user_db_open`). |
| Outputs | criterion rows; `--vmhwm`: `key=value` lines per child (`op`, `vmhwm_kib`, `opened`). |
| Platform | Linux for VmHWM; else `unavailable`. |
| Consumers | benches runbook table; `docs/perf/perf-baseline-kc-2026-09.md` lineage. Not CI. |
| State | `BenchRoot` temp dirs per op. |
| Overlap | rows deliberately aligned with `pinyin-oracle` `dbm_bench` ("NOT comparable in absolute value"); same `--vmhwm` child pattern as dbm_bench. |
| Constraints | required-features convention "b8aff564"; `#![cfg]` mirrors it for rust-analyzer. |

### TLS-41 — crates/oxpinyin-store/benches/backend_matrix_kyotocabinet.rs

| Field | Value |
|---|---|
| Entry point | `cargo bench -p oxpinyin-store --no-default-features --features kyotocabinet --bench backend_matrix_kyotocabinet`; `required-features = ["kyotocabinet"]` |
| Owning subsystem | perf |
| Purpose | `support::run::<KcStore>("kyotocabinet")` — same workload as TLS-40 over `fixtures/w3/kct`. |
| Implementation | Rust, 16 lines + shared support. |
| Inputs | w3 kct fixture; `BACKEND_MATRIX_POPULATED`. |
| Outputs | as TLS-40. |
| Platform | needs libkyotocabinet. |
| Consumers | runbook; KC baseline doc. |
| State | temp dirs. |
| Overlap | TLS-40/42. |
| Constraints | as TLS-40. |

### TLS-42 — crates/oxpinyin-store/benches/backend_matrix_tkrzw.rs

| Field | Value |
|---|---|
| Entry point | `cargo bench -p oxpinyin-store --no-default-features --features tkrzw --bench backend_matrix_tkrzw`; `required-features = ["tkrzw"]` |
| Owning subsystem | perf |
| Purpose | `support::run::<TkrzwStore>("tkrzw")` over `fixtures/w3/tkt`. |
| Implementation | Rust, 16 lines + shared support. |
| Inputs | w3 tkt fixture; `BACKEND_MATRIX_POPULATED`. |
| Outputs | as TLS-40. |
| Platform | needs libtkrzw. |
| Consumers | runbook example command. |
| State | temp dirs. |
| Overlap | TLS-40/41. |
| Constraints | as TLS-40. |

### TLS-43 — crates/oxpinyin-store/benches/kyotocabinet_bnum.rs

| Field | Value |
|---|---|
| Entry point | `OXPINYIN_BNUM_DATA_DIR=<kc data/> cargo bench --no-default-features --features kyotocabinet,bench-internal --bench kyotocabinet_bnum`; `required-features = ["kyotocabinet","bench-internal"]` |
| Owning subsystem | perf (store, KC-only) |
| Purpose | Time side of issue #402: for each `#bnum` candidate (65,536; 16,384; 4,096; 1,024) opens each of the five read-only system TreeDBs, samples ≤512 evenly-strided keys, warms the page cache, then times `KcStore::get_raw` over the sample; a child process per group reports VmHWM. |
| Implementation | Rust, criterion, 385 lines; uses `bench-internal` tuning helpers on `KcStore`. |
| Inputs | env `OXPINYIN_BNUM_DATA_DIR` (else `fixtures/w3/kct`, ~10² records → smoke only); re-exports the var to its child. |
| Outputs | one criterion group per `#bnum`, five benches each (`BenchmarkId::new(stem, sample.len())`); stderr resident line per group. |
| Platform | Linux for VmHWM column (`unavailable` elsewhere). |
| Consumers | runbook "kyotocabinet_bnum" section (Fedora arm64 container recipe). Not CI. |
| State | child re-exec. |
| Overlap | none (KC `PlantDB` page cache specific). |
| Constraints | "Not a local-change proposal" — tuning `#bnum` would be a case-3 divergence outside the compatibility policy; audience is an upstream report. "No capture is committed." Do not pass `-- --bench`. |

### TLS-44 — crates/oxpinyin-user/benches/export_phrases.rs

| Field | Value |
|---|---|
| Entry point | `cargo bench -p oxpinyin-user --bench export_phrases` (backend = compiled-in store feature) |
| Owning subsystem | perf |
| Purpose | `UserStore::export_phrases` at N = 64/256/1024 synthetic phrases (one pronunciation each, keys 1..=300) — the single-walk pronunciation collection of #341. |
| Implementation | Rust, criterion, 63 lines. |
| Inputs | none external; temp store `std::env::temp_dir()/oxpinyin-bench-export-<N>-<pid>` via `oxpinyin_store::default_store_file`. |
| Outputs | group `user_export_phrases`, `Throughput::Elements(N)`. |
| Platform | portable |
| Consumers | runbook table ("the export" listed as a need — observation: the code uses no export dir). |
| State | temp store + `-lock` sidecar cleaned. |
| Overlap | seeding helper duplicated with phrase_read.rs. |
| Constraints | none |

### TLS-45 — crates/oxpinyin-user/benches/phrase_read.rs

| Field | Value |
|---|---|
| Entry point | `cargo bench -p oxpinyin-user --bench phrase_read` |
| Owning subsystem | perf |
| Purpose | `UserStore::phrase(token)`: point-get + range scan of 16 pronunciations over 64 seeded phrases; "a regression detector, not an improvement signal" (read-txn + cursor open ~2.4 µs dominates). |
| Implementation | Rust, criterion, 87 lines. |
| Inputs | temp store `oxpinyin-bench-phrase-read-<pid>`. |
| Outputs | group `user_phrase_read`, id `pron_range/16`. |
| Platform | portable |
| Consumers | runbook table. |
| State | temp store cleaned. |
| Overlap | with TLS-44. |
| Constraints | cannot resolve sub-µs allocation changes (#340 F1/F3). |

### TLS-46 — crates/pinyin-oracle/benches/scan_perf.rs

| Field | Value |
|---|---|
| Entry point | `cargo bench -p pinyin-oracle --bench scan_perf` (no required-features) |
| Owning subsystem | perf (window-scan) |
| Purpose | Criterion baseline for the window-scan construction: `keystroke_cycle_20_inputs` (per-character `type_pinyin` over the 20 `CYCLE_INPUTS`: ni, wo, de, nihao, zhongguo, xian, fangan, xi'an, bu'tian, fan'gan, n, zh, chongke, caisho, paolen, waimenggu, lenglan, naoxion, liangniejue, chuaipengdengzaimiu), `prefix_probe` (`SEARCH_CONTINUED` binary search), `parse_interpolation2`, `decode_pass_user_store` (no/empty/populated store attached). |
| Implementation | Rust, criterion, 216 lines; `benches/support/mod.rs` (harness). |
| Inputs | export dir (`PINYIN_EXPORT_DIR`, default `/tmp/oxpinyin-export`, via `oxpinyin_testsupport::model_cache::export_dir`; asserts the three backend-named system DBMs); model20 cache (`PINYIN_MODEL_DIR`/`PINYIN_MODEL_CACHE`/default) for `interpolation2.text`; temp store paths. Refuses to start without them ("flat-100 fallback numbers would not describe the pinned construction"). |
| Outputs | criterion rows. |
| Platform | portable given data. |
| Consumers | `docs/perf/perf-exploration.md`, `perf-candidate-cap-2026-08.md`. Not CI. |
| State | temp store files. |
| Overlap | `CYCLE_INPUTS` mirrors the 20-input cycle `bisect --perf` runs (run-w8-cycle.sh "same 20-input parse + guess + count cycle"). |
| Constraints | `load_scoring_tables` deliberately avoids model20 unigrams for the decode-pass group. |

### TLS-47 — crates/pinyin-oracle/benches/alloc_profile.rs

| Field | Value |
|---|---|
| Entry point | `cargo bench -p pinyin-oracle --bench alloc_profile -- [--dhat] [--keystroke-only]` (custom `main`, not criterion) |
| Owning subsystem | perf |
| Purpose | One-shot heap profile: with `--dhat` wraps only the decode loop (full W2 parity `type_pinyin` run, or keystroke cycle) in a `dhat::Profiler` writing `/tmp/dhat-heap-parity.json` or `/tmp/dhat-heap-keystroke.json`; without it prints probe counts, table sizes, load timings. |
| Implementation | Rust, `dhat` global allocator, 198 lines. |
| Inputs | export dir + model20 (`load_real_tables`); W2 corpus `tests/parity/corpus/inputs/*` (`load_w2_inputs`, `corpus::CORPUS_DIR`). |
| Outputs | DHAT JSON under `/tmp`; stdout counters. |
| Platform | portable |
| Consumers | `docs/perf/perf-exploration.md`. |
| State | `/tmp` outputs (hard-coded). |
| Overlap | none |
| Constraints | setup allocations intentionally excluded from the dump. |

### TLS-48 — crates/pinyin-oracle/benches/dbm_bench.rs

| Field | Value |
|---|---|
| Entry point | `PINYIN_ORACLE_PREFIX=<prefix> cargo bench -p pinyin-oracle --features oracle-ffi --bench dbm_bench [-- --save-baseline <name> | --vmhwm]`; `required-features = ["oracle-ffi"]`; `#![cfg(feature="oracle-ffi")]` |
| Owning subsystem | perf (pinyin-oracle, libpinyin DBM backends) |
| Purpose | Wall-clock (criterion) and child-process VmHWM of the pin-built libpinyin's user-data path through the C facade: `libpinyin_dbm/init_load` (`pinyin_init` over system data + fresh user dir), `train_write/{64,256}` (N `Session::train_top` events + one `pinyin_save`), `user_db_open` (`pinyin_init` over a copy of a 256-train user dir). Delta across `--dbm` prefixes isolates the backend. |
| Implementation | Rust, criterion + custom main, 380 lines; oracle FFI. |
| Inputs | env `PINYIN_ORACLE_PREFIX` (exit 2 if unset), `PINYIN_BENCH_DBM` (non-tkrzw prefixes), `DBM_BENCH_POPULATED` (child op). |
| Outputs | criterion rows under `libpinyin_dbm/*`; `--vmhwm` key=value child lines. |
| Platform | Linux (oracle prefix, `/proc`). |
| Consumers | runbook table; `docs/perf/perf-baseline-kc-2026-09.md`. Not CI (links the oracle; skipped without the feature). |
| State | per-iteration fresh user dirs; child re-exec. |
| Overlap | row names aligned with `backend_matrix_*` (TLS-40..42); training goes through `Session::train_top` per `docs/runbooks/oracle.md` rule. |
| Constraints | "Do not mix the two axes in one table"; absolute numbers are facade+DBM, not storage-tier. |

### TLS-49 — crates/oxpinyin-capi/benches/stage2.rs

| Field | Value |
|---|---|
| Entry point | `cargo bench -p oxpinyin-capi --bench stage2 [-- --profile-time 10 | --save-baseline/--baseline]` (no required-features) |
| Owning subsystem | perf (C-ABI, Stage 2) |
| Purpose | W8 C-ABI surface groups: `parse_more_full_pinyins` (short/medium/junk_leading), `guess_candidates` (offset_0/mid_phrase, sort `W8_SORT`), `guess_sentence_get_sentence_0/full_nbest_post_116`, `user_store_count_delta_hot_token` (cached overlay via `oxpinyin_user::UserStore`), `alloc_instance`. Prints the pin header (PIN_REF …074a2219…, MODEL20 sha, model dir) on stderr. |
| Implementation | Rust, criterion (`sample_size 20`, warm-up 500 ms, measurement 4 s, `noise_threshold(0.50)`), 248 lines + `benches/support/mod.rs` (dlopen-free direct FFI into the crate, model-cache verification, staged system dir). |
| Inputs | export dir (`PINYIN_EXPORT_DIR`/default) linked/copied into `temp_dir()/oxpinyin-stage2-system-<pid>` with `interpolation2.text` from the verified model20 cache (`PINYIN_MODEL_DIR` > `PINYIN_MODEL_CACHE` > `target/model20`); panics if cache absent/off-pin. |
| Outputs | criterion rows; frees ABI strings with libc `free` (re-declared). |
| Platform | portable given data. |
| Consumers | runbook; `docs/perf/perf-stage2-harness-2026-08.md` and successors. "CI does not run these benches." |
| State | staged temp system dir. |
| Overlap | model-cache verification duplicated with `run-w8-cycle.sh` and `oxpinyin_testsupport::model_cache`; input constants shared with support. |
| Constraints | does not assert the 10190-class candidate pins (those live in `pinyin-oracle` `real_tables_integration`); 50% noise threshold is the only "gate", optional and local. |

---

## Count

49 records: TLS-01 TLS-02 TLS-03 TLS-04 TLS-05 TLS-06 TLS-07 TLS-08 TLS-09 TLS-10 TLS-11 TLS-12 TLS-13 TLS-14 TLS-15 TLS-16 TLS-17 TLS-18 TLS-19 TLS-20 TLS-21 TLS-22 TLS-23 TLS-24 TLS-25 TLS-26 TLS-27 TLS-28 TLS-29 TLS-30 TLS-31 TLS-32 TLS-33 TLS-34 TLS-35 TLS-36 TLS-37 TLS-38 TLS-39 TLS-40 TLS-41 TLS-42 TLS-43 TLS-44 TLS-45 TLS-46 TLS-47 TLS-48 TLS-49

## Architecture notes

Observations (what the code does) unless marked *interpretation*.

- **Three `.ver` parsers.** `ver_globals()` awk is copied verbatim between `check-exports.sh` and `check-alloc-pairing.sh`; `relink-versioned.sh` re-implements it with sed (`sed -n '/global:/,/local:/p' | tr ';' '\n'`). Same frozen inputs (`libpinyin.ver` 79 / `libzhuyin.ver` 52).
- **Duplicated fixture staging.** `check-alloc-pairing.sh::prepare_fixtures` is a declared copy of `tools/bisection/run-cpp-smoke.sh` (w3 per-backend dir + stub `interpolation2.text` `\data model interpolation`), as is the `lib/libpinyin.so.15 → libpinyin_capi.so` symlink law. Both also share the `GLIB_CFLAGS` env override.
- **Duplicated model20 verification.** `run-w8-cycle.sh::verify_pinned_model` (bash), `crates/oxpinyin-capi/benches/support/mod.rs::assert_pinned_model` (Rust) and `oxpinyin_testsupport::model_cache` all check the `verified` marker `sha256=59c68e89…` or the archive digest, with the same env precedence `PINYIN_MODEL_DIR` > `PINYIN_MODEL_CACHE` > `target/model20`.
- **Duplicated companion-header check** (`novel_types.h`, `pinyin_custom2.h` byte-identical across the two capi crates): `install.sh::check_companion_headers` and inline in `release-stage.sh`; the primary gate is the `companion_headers.rs` test.
- **Near-duplicate C++ drivers**: `alloc-pairing-pinyin.cc` / `alloc-pairing-zhuyin.cc` share `Slot`, `mark`, `slot_id`, `expect_declared`, `release_g_free/strfreev`, two-pass `__lsan_disable` main, `SLOT` line format and exit codes 10/11/64 — separate TUs by stated design (header typedef collisions).
- **Near-duplicate sanitizer gates**: `tools/kc/run-sanitizers.sh` vs `tools/bdb/run-sanitizers.sh` — different default nightlies (`nightly-2026-08-01` vs `nightly-2026-09-07`), different target defaults (fixed x86_64 vs host), kc has `OXPINYIN_SANITIZER_ALLOW_SKIP` self-skip and no `--no-default-features`. **The kc script is not invoked by any workflow** (store-backends.yml names it only in comments; the tkrzw job inlines RUSTFLAGS and there is no kc sanitizer job).
- **Near-duplicate test harnesses**: `tests/lint-commits.test.sh` and `tests/lint-crate-cfg-placement.test.sh` (same `ok/fail_test/run_lint/expect_*` shape); `callgrind-ir.test.sh` and `series.test.sh` (same `check/expect/says` shape).
- **Env var names and aliases**:
  - Cell selection: `PINYIN_ORACLE_DBM` (bdb\|kc\|tkrzw) → `CAPI_FEATURE`, `OXPINYIN_CAPI_BACKEND_EXT` (db\|kct\|tkt), `CARGO_TARGET_DIR` default `${OXPINYIN_TARGET_ROOT:-target/cells}/<dbm>` (oracle-cell.sh, sourced by check-exports, check-alloc-pairing, check-pc-metadata). `check-pc-metadata.sh` maps `--build-subject=kyotocabinet` → `PINYIN_ORACLE_DBM=kc` (feature name ≠ cell name).
  - Prebuilt artifact overrides: `OXPINYIN_CAPI_SO`, `OXPINYIN_ZHUYIN_SO`.
  - Perf: `PERF_CPU` (snapshot default 0; run-w8-cycle optional), `PERF_CYCLES` (run-w8-cycle default 32; passed to bisect), `PERF_GATE_CYCLES`/`PERF_GATE_CG_CYCLES`/`PERF_GATE_RSS_PROCESSES` (snapshot), `PERF_BACKEND` (`gate` vs `oxpinyin`), `PERF_MODE=speed`, `OXPINYIN_PROFILE_DIR`, `PROFILE_REUSE_STAGE`.
  - Data: `PINYIN_EXPORT_DIR` (default `/tmp/oxpinyin-export`), `PINYIN_MODEL_DIR`, `PINYIN_MODEL_CACHE`, `OXPINYIN_BNUM_DATA_DIR`, `OXPINYIN_LIBPINYIN_DATA_DIR`, `PINYIN_ORACLE_PREFIX`, `PINYIN_BENCH_DBM`, `BACKEND_MATRIX_POPULATED`, `DBM_BENCH_POPULATED`.
  - Sanitizers: `OXPINYIN_SANITIZER_TOOLCHAIN`, `OXPINYIN_SANITIZER_TARGET`, `OXPINYIN_SANITIZER_ALLOW_SKIP` (kc only).
  - Packaging: `LIBPINYIN_DATABASE_FORMAT` (documented in install.sh usage; not consumed in the script body — *interpretation: consumed by build.rs*), `OXPINYIN_RELINK_FEATURES`, `CC`, `CARGO_BUILD_TARGET`, `PKG_CONFIG_PATH`; runners.json `OXPINYIN_METADATA_{PIN_ROOT,SUBJECT_ROOT,PREFIX}`.
  - CI: `GITHUB_STEP_SUMMARY`, `GITHUB_EVENT_NAME`, `GITHUB_RUN_ID`, `GITHUB_SERVER_URL`, `GITHUB_REPOSITORY`, `ImageOS`, `ImageVersion`; `HTTPS_PROXY` (probe-egress); `PKG` (probe-debuginfo).
- **Exit-code conventions**: abi gates 0/1 (+2 usage/host); alloc drivers 0/1/2/10/11/64; series.py 0 reported / 1 flagged / 2 malformed; lint scripts 0/1/2 (pass/violation/usage, tests assert exactly 1 vs 2); tkrzw probes 0/1/2(/3); kc sanitizer 0 on explicit ALLOW_SKIP; backend-matrix 0/1 with `set -u` only; packaging scripts 0/1/2; capture 0/1/2. No tool in this set uses 77 (SKIP).
- **Pin / manifest parsers**: `run-capture.sh::manifest_value` reads `oracle-pin.txt` keys (`schema`, `pin_ref`, `dbm`, `*_sha256`); `pc-version.sh` reads `libpinyin_tag=` from `tools/oracle/oracle-pin.txt`; `oracle-cell.sh` sed-reads `LIBPINYIN_VERSION/LIBPINYIN_SHA/MODEL_SHA256` from `build-oracle.sh` to synthesize `EXPECTED_PIN_REF`; `run-w8-cycle.sh` greps `datagen-manifest.txt` for `^backend=tkt$`; `release-arch.sh` only checks `table.conf` existence. Hard-coded pin refs: `run-w8-cycle.sh` `PIN_REF`, `run-capture.sh` `EXPECTED_PIN_REF`, capi bench support `PIN_REF`, all `…074a2219…+model20-59c68e89…+dbm-tkrzw`.
- **Hard-coded backend names/extensions**: oracle-cell.sh (bdb/kc/tkrzw → db/kct/tkt); snapshot.sh (tkrzw→tkt, kyotocabinet→kct, no bdb); check-alloc-pairing.sh (`kct|tkt|db`, search order db→tkt→kct); backend-matrix.sh (feature list); relink-versioned.sh (link libs, precedence tkrzw>kyotocabinet>bdb); release-stage.sh (`db_format_for`), release-arch.sh (`EXTRA_DEPS` kyotocabinet|tkrzw only), libpinyin-backend-probe.sh (soname fragments), bench `#![cfg(feature=…)]` gates, `fixture_dir()` via `DEFAULT_STORE_EXT`.
- **Cargo spawns**: oracle-cell.sh `oracle_cell_artifact` (`cargo build --locked -p <pkg> --no-default-features --features $CAPI_FEATURE`, debug) ← check-exports (×2 libs, + `shipped` mode), check-alloc-pairing (×2); backend-matrix.sh (`cargo check` ×9); kc/bdb run-sanitizers (`cargo +nightly test`, bdb also `build --example bdb_write_profile`); probe-debuginfo (`cargo build --release` ×3); snapshot.sh (install.sh cinstall + `cargo build --release … alloc-count`); install.sh (`cargo cinstall`); run-w8-cycle.sh (`cargo cinstall --profile profiling`); check-pc-metadata (`install.sh` ×2 with `--profile dev`); release-stage (`install.sh` ×2 `--release`).
- **Process control / timeouts / locks**: none of these scripts set timeouts themselves (runners.json gives abi/pc gates `timeout: 900`); `taskset -c` pinning in snapshot.sh (recorded as `cpu_pinned`) and run-w8-cycle.sh (`PERF_CPU`); release-arch drops privileges via `su oxp-builder`; distro-probe.sh traps HUP/INT/TERM → exit 1 so the `0` trap cleans; all mktemp dirs are trap-removed except identity-probe outputs and `target/{capture,profile}`. The perf series lives in the GitHub Actions cache (`.perf-series`, 30 samples retained, written only on `schedule`).
- **Frozen / "do not change"**: `.ver` files (verbatim pin copies); `.alloc` registers' five-field shape and `VERIFIED_KINDS`; `pinyin-capture-v1` schema and 10-candidate cap; `fixtures/foundation/*.txt` SHA-256s (capture-fixtures.md); `.pc` variable set "mirroring upstream byte-for-byte"; `@VERSION@` placeholder rule in `.pc.in`; KC open parameters (bnum bench: "not a local-change proposal"); callgrind-ir fixtures' hand-computed expectations; `series.py` is "NOT a gate"; `snapshot.sh` "no threshold lives in this file"; the `alloc-count` feature never in a shipped artifact (enforced by `nm -D` in snapshot.sh).
- *Interpretation:* the four shell-side "gate" scripts with the same `--- section ---` / `PASS:`/`FAIL:` idiom (check-exports, check-alloc-pairing, backend-matrix, release-stage) and the two POSIX lint scripts are candidates for one shared `fail()`/summary helper; the three `.ver` readers and the two companion-header checks are the most mechanical consolidation targets.

## Disposition of PR #767

**PR #767** — "Publish measured BDB profiling workloads for #757–#759" (branch `perf/757-759-harness`, merged 2026-10-14T04:45:04Z; two commits, `Assisted-by: Codex:gpt-6`: `4f430509` "Publish measured BDB profiling driver and recipe" and `56affabf` "Validate profiling inputs and refresh BDB baselines"; landed on main as `cfc1950b` and `1be0b4fd`). Files: `tools/bisection/system-import-profile.c` (+176/−2, then +40/−13) and `docs/runbooks/benches.md` (+106, then +80). PR body: an "existing-driver extension satisfying AGENTS.md rule 6; it adds no product code or new general driver"; path check named the BDB reference cell.

### The driver (`tools/bisection/system-import-profile.c`, 279 lines at origin/main)

Header: "#599 reproducible callgrind/massif workload. No wall-clock timing." Usage `<libpinyin.so> <system-data-dir> <fresh-user-dir> [iterations | --cold-open | --export-system | --sentence-fixture F | --train-fixture F | --phrase-file F]`. C, `dlopen`/`dlsym` of the SO under test (pin or subject), `<valgrind/callgrind.h>` client requests only in the legacy iteration mode (`CALLGRIND_DUMP_STATS_AT("opened"/"imported"/"saved")`); the three #767 modes "emit no Callgrind client-request dumps". The user dir must be fresh and empty (checked). Measurement is external: whole-process Valgrind Callgrind `Ir` and Massif `mem_heap_B`, never wall clock, CPU time or allocation counts.

Workload definitions (exact, from the source and the runbook):

| Workload | Flag | Input | libpinyin calls | Output lines (stdout) |
|---|---|---|---|---|
| Guessing (#757) | `--sentence-fixture fixtures/w4/oracle-sentence-surface.txt` | every row with a nonempty third (surface) field: 500 rows; `pinyin_set_options(ctx, 0x18a)` once | per row: `pinyin_reset`, `pinyin_parse_more_full_pinyins`, `pinyin_get_parsed_input_length` (must be >0), `pinyin_guess_sentence`, `pinyin_guess_candidates(inst, 0, 0x1e)`, `pinyin_get_n_candidate`, for each candidate `pinyin_get_candidate` + `pinyin_get_candidate_type` (+ `pinyin_get_candidate_nbest_index` for type 1), `pinyin_get_sentence(i)` for i ≤ min(max nbest rank, 2), `pinyin_get_candidate_string` for the first 6, `g_free` | `input\ttrue|false\t<sentence0>\x01<sentence1>…\t<kind/rank/text>\x01…` per row; `operations\t500\t0` |
| Training (#758) | `--train-fixture fixtures/w4/oracle-sentence-surface.txt` | first 200 such rows; options `0x18a` | per row: `pinyin_reset`, parse, parsed length **must equal `strlen(input)`** (56affabf: checked before guessing, "Session::train_top checks full consumption before guessing"), `pinyin_guess_sentence` must be true, `pinyin_train(inst, 0)`, `pinyin_get_sentence(inst, 0)`; after the loop one `pinyin_save(ctx)` | `train\t<input>\t<parse_return>\t<len>\t1\t<top sentence>` per row; `operations\t200\t200` |
| Importing (#759) | `--phrase-file $PHRASE_FILE` | 10,000 rows `library\tphrase\tpinyin\t5\n` generated by the **unmeasured** `--export-system` (walks libraries 1–4 via `pinyin_load_phrase_library` + `pinyin_begin_get_phrases`/`has_next`/`get_next`/`end`), first 10,000 distinct phrase texts, first pronunciation, count 5; 121,316 bytes, SHA-256 `22637aee…bbb7`; first row `1\t锕\ta\t5`, last `1\t爱河\tai'he\t5`; disposable, not committed | per library change: `pinyin_load_phrase_library(lib)`, `pinyin_begin_add_phrases(lib)`; per row `pinyin_iterator_add_phrase(it, phrase, pinyin, count)`; `pinyin_end_add_phrases`; one `pinyin_save`; then `export_walk` over each touched library (library 1 → 95,698 rows) printing `lib\tphrase\tpinyin\tfrequency` | export rows; `operations\t10000\t95698` |

56affabf hardened `--phrase-file` parsing: embedded NUL rejected (`strnlen`), library must be a bare decimal in 1..4 with no suffix, count parsed with `strtol` + range check to `int32`, line end must be `\0`, `\n` or `\r\n`; 16 malformed inputs rejected, `+5`/`-5`/CRLF/EOF-terminated rows accepted with byte-identical stdout.

Fixture: `fixtures/w4/oracle-sentence-surface.txt`, SHA-256 `d119f3a7f665333ef5717b1925171799c29e710d4ad1575748e0a9d3ae2c9e21`.

### Measurement protocol (runbook, `docs/runbooks/benches.md` § "BDB guess, training, and import profiles (#757–#759)")

Build: `cc -std=gnu11 -Wall -Wextra -Werror -O2 tools/bisection/system-import-profile.c -o "$HARNESS" -ldl`. Six commands, one Callgrind and one Massif per workload, all `taskset -c 0`:

- `valgrind --tool=callgrind --cache-sim=no --branch-sim=no --callgrind-out-file=/dev/stderr "$HARNESS" "$SO" "$DATA" "$USER_DIR" <mode> <input>`
- `valgrind --tool=massif --time-unit=i --heap=yes --stacks=no --peak-inaccuracy=0.0 --detailed-freq=1 --max-snapshots=1000 --massif-out-file=/dev/stderr "$HARNESS" …`

Policy sentences quoted from the runbook:

- "Use the same BDB system data, fresh empty user directory, CPU, pin SO, and subject SO throughout a comparison."
- "Use a fresh empty `USER_DIR` for **every** command and repeat each pin/subject command three times. Take the median of whole-process Callgrind `Ir` and of Massif's maximum `mem_heap_B`; report spread as `(max − min) / median`. The Massif value excludes stacks, allocator overhead, and file mappings. Check output equivalence and operation counts before comparing costs. Keep profiler output in streams or memory, not in archives, logs, captures, or profile files."
- "It is a disposable workload input, not a committed fixture or profile capture" / "Remove the disposable phrase input after the runs."
- "Later PR comparisons must identify their own subject build and use matched pin/main/subject conditions; these registered figures are not a substitute."
- "A run overlapping suspend is discarded by checking the change in `CLOCK_BOOTTIME - CLOCK_MONOTONIC` and is retried."
- From the general sections: "A measurement is the ratio of artifacts timed in the same job on the same machine, never a number carried over from another run." / "Every measurement report states the Valgrind version it used" / "Which artifact was timed is part of the measurement, not a detail of it. Name the exact cargo command line for every timed oxpinyin artifact, and give its `sha256`, `NEEDED` list and byte size." / "A findings document commits no captures."

Registered outputs: stdout SHA-256 `50dfc9c8…3100d` (500 guesses), `e5855249…698d` (200 training steps), `34e98cd8…ec1d` (10,000 imports + 95,698 exports) — identical on pin and subject. Subject SO recipe: `tools/packaging/install.sh libpinyin --prefix=… -- --locked --release --jobs 1 --no-default-features --features bdb,shipped` with `CARGO_BUILD_JOBS=1`, i.e. `cargo cinstall … --features bdb,shipped` then relink; pin SO SHA-256 `d535d077…badc` (074a2219, BDB). The 2026-10-14 refresh (Fedora 45, i7-9750H, GCC 16.2.1, Valgrind 3.27.1, rustc 1.97.1, cargo-c 0.10.24, main `d2a5d207`) recorded, median Ir / median peak heap: guesses pin 2,245,511,454 / 2,147,301 vs main 6,641,501,950 / 2,620,088; training pin 454,876,602 / 7,124,656 vs main 1,186,203,677 / 1,546,948; import pin 758,108,369 / 9,354,935 vs main 1,725,238,006 / 7,423,931 (spreads ≤0.026%).

### What a Rust replacement would need to show equivalence

Observation-derived (the runbook and driver define the bar; this list restates it):

1. Same observable work: byte-identical stdout to the C driver's three modes for the same SO/data/fixture — the registered hashes `50dfc9c8…`, `e5855249…`, `34e98cd8…` and `operations\t<n>\t<m>` counts (500/0, 200/200, 10000/95698) — and identical rejection behaviour for the 16 malformed phrase inputs.
2. Same call sequence per row (the tables above), same option word `0x18a`, same sort mask `0x1e`, `train_top` preconditions (full consumption, successful guess, `train(0)`), one `pinyin_save` at the end of training/import, export over touched libraries only, fresh empty user dir enforced.
3. Same measurement: whole-process Callgrind `Ir` (`--cache-sim=no --branch-sim=no`) and Massif `mem_heap_B` (`--time-unit=i --heap=yes --stacks=no --peak-inaccuracy=0.0 --detailed-freq=1 --max-snapshots=1000`), three runs, medians, `(max−min)/median` spread, `taskset -c 0`, suspend-overlap discard; no wall clock; profiler output streamed, nothing captured in-tree.
4. Same provenance statement: driver source and executable SHA-256, pin SO SHA-256, subject build command line (`install.sh … --features bdb,shipped`), subject `.so` size/SHA-256/`NEEDED` list, Valgrind version, host/CPU, fixture SHA-256, phrase-input SHA-256 — and, since the harness itself would now be part of the measured process, a demonstration that the replacement harness's own Ir/heap overhead outside the dlopened SO is accounted for (the C driver is thin; a Rust harness adds a runtime to the whole-process figure — *interpretation*).
5. Dynamic loading of the SO under test by path (the driver `dlopen`s pin and subject alike), so the same binary drives both sides; and the AGENTS.md rule 6 stance the PR invoked ("no new bespoke driver … where an existing driver can take a case file") means a Rust replacement must justify retiring the existing driver rather than extending it.


---

# CI inventory — workflows, jobs, actions, scripts, hooks, gate tooling

Tree read: the detached worktree at origin/main (`scratchpad/ox`). Every
workflow, action, script and hook below was read in full; the `tools/`
scripts the jobs call were read at least through their headers and the
parts the job depends on (noted per record). IDs: `CI-NN`.

Shared facts (stated once, referenced below):

- **Container convention.** Every Linux job except `changes`, `ci-aggregate`,
  `oracle-image`, `attach` and `store-backends-gate` runs in
  `container: debian:testing`, deliberately unpinned ("do NOT pin this
  image by digest or let a later change 'stabilise' it", store-backends.yml
  header). Each such job starts with the same inline apt step: `apt-get
  update -qq && apt-get install -y --no-install-recommends bash tar gzip zstd
  git ca-certificates curl` (plus job-specific packages), **before**
  `actions/checkout` (so checkout clones with git rather than falling back to
  a REST tarball — verify-nightly.md §4).
- **Checkout.** `actions/checkout@3d3c42e5…` (v7.0.1, SHA-pinned, "do not bump
  without re-verifying") with `persist-credentials: false` everywhere.
- **Bootstrap.** `./.github/actions/bootstrap` (CI-26) right after checkout.
- **Workflow-level env.** ci.yml, store-backends.yml, release-packages.yml:
  `CARGO_TERM_COLOR=always`, `RUSTFLAGS=-D warnings` (store-backends and
  release add `DEBIAN_FRONTEND=noninteractive`; ci.yml sets it per job).
  coverage.yml, verify-nightly.yml, oracle-image.yml, commit-trailers.yml set
  no workflow-level RUSTFLAGS (overflow-lane sets its own).
- **Permissions.** Every workflow declares `permissions: contents: read`;
  only `oracle-image` (`packages: write`) and `attach` (`contents: write`)
  widen it. The only secret used anywhere is `secrets.GITHUB_TOKEN` /
  `github.token` (GHCR login; `gh release upload`).
- **Oracle.** No job on any PR/push path provisions, pulls or needs the
  pin-built oracle or model20. Two exceptions, both nightly/own-trigger:
  `verify-nightly/differentials` **builds** pinned libpinyin from source
  (tkrzw, no model data; identity by `git rev-parse FETCH_HEAD == LIBPINYIN_SHA`
  from `tools/oracle/build-oracle.sh`), and `oracle-image` builds and
  publishes the library-only image (identity by sha256 of each `.so` against
  `tools/oracle/oracle-pin.txt` via `tools/oracle/verify-oracle-image.sh`).
  `tools/cloud/setup.sh` is the only thing that **pulls** the image, and it is
  a developer/cloud-session tool, not a workflow.

---

## .github/workflows/ci.yml

Workflow: `name: CI`; `on: push: branches [main]`, `pull_request: branches
[main]`. No workflow-level path filter — the `changes` job is the gate.
Header documents the gated path set and the "deliberately NOT gated" set
(docs/**, reference/**, .kiro/**, root *.md, store-backends.yml). Required
checks on main: `ci-aggregate`, `trailer-lint`, `trailer-test`.

### CI-01 — .github/workflows/ci.yml (job: changes)

| Field | Value |
|---|---|
| Entry point | GitHub Actions job; first job of every push/PR run; `outputs.run` consumed by every other lane's `if:` |
| Owning subsystem | ci |
| Purpose | Computes the single path-gate flag `run=true/false` by diffing the event base against HEAD over the gated pathspec, so docs-only changes skip every code lane while still reporting a conclusion |
| Implementation | YAML + inline bash (`set -euo pipefail`); `ubuntu-latest`, no container; `fetch-depth: 0` |
| Inputs | `GITHUB_EVENT_NAME`, `github.event.pull_request.base.sha` (PR) or `github.event.before` (push). Pathspec: `crates fuzz tools Cargo.toml Cargo.lock rust-toolchain.toml deny.toml clippy.toml .github/workflows/ci.yml .github/scripts/lint-crate-cfg-placement.sh tests/lint-crate-cfg-placement.test.sh fixtures tests/parity .github/actions` |
| Outputs | `run` output; missing/unknown base → `run=true` (over-trigger direction). Two-dot diff `git diff --name-only "$base" HEAD -- …` (deliberately not merge-base: a branch behind main over-triggers; documented as intentional, PR #487) |
| Platform | GitHub-hosted Linux (bash) |
| Consumers | `lint`, `test`, `test-macos`, `test-windows`, `deny` (`if: needs.changes.outputs.run == 'true'`), `ci-aggregate` (compares each lane's conclusion against `GATED`) |
| State | none |
| Overlap | coverage.yml, store-backends.yml, oracle-image.yml use workflow-level `paths:` filters instead (no job gate); comment says "no third-party filter action" |
| Constraints | "The pathspec list is the one the header documents; keep the two in lockstep." Skipped lanes report `skipped`, which required checks accept; the residual bypass (PR rewriting ci.yml) is branch-protection policy |

### CI-02 — .github/workflows/ci.yml (job: lint)

| Field | Value |
|---|---|
| Entry point | job `lint`, `needs: changes`, `if: run == 'true'`; `timeout-minutes: 30`; `defaults.run.shell: bash`; `env DEBIAN_FRONTEND=noninteractive` |
| Owning subsystem | ci |
| Purpose | Formatting, Clippy, crate-cfg placement, drop-in pc-version agreement and the Lizard CCN ratchet — the Tier-1 static gate |
| Implementation | `container: debian:testing`; apt `gcc g++ make pkg-config libclang-dev libdb-dev libglib2.0-dev` (Berkeley DB default backend; bindgen reads system `db.h`); bootstrap `safe-directory rustup rustup-components="clippy rustfmt" rust-cache key=lint-job`; `rustup show active-toolchain` |
| Inputs | Commands, exact and in order: `cargo fmt --all --check` · `cargo fmt --manifest-path fuzz/Cargo.toml --all --check` · `tests/lint-crate-cfg-placement.test.sh` · `.github/scripts/lint-crate-cfg-placement.sh` · `tools/packaging/check-pc-version.sh` · `cargo clippy --locked --workspace --all-targets -- -D warnings` · `apt-get install python3-pip` · `pip install --break-system-packages lizard==1.24.0` · `lizard crates/ -l rust -C 40`. Reads `clippy.toml` (msrv), `rust-toolchain.toml`, `tools/oracle/oracle-pin.txt` (via pc-version.sh), both capi `Cargo.toml`s and `.pc.in` templates |
| Outputs | Step exit codes only; no artifacts. Failure = fmt drift, a Clippy warning (RUSTFLAGS -D warnings), a crate-level `#![cfg]` above its `//!` docs, a capi manifest version disagreeing with `libpinyin_tag`, or any function with CCN > 40 |
| Platform | Linux container (every `target_os = "linux"` cfg true, hence the placement checker) |
| Consumers | `ci-aggregate` |
| State | Swatinem/rust-cache (key `lint-job`); pip install into the ephemeral container |
| Overlap | fmt/clippy also run locally per README Quickstart; store-backends children install the same clippy/rustfmt components but run only `cargo test`; `tools/packaging/check-pc-metadata.sh` is the full (local-only, needs the pin) version of `check-pc-version.sh` |
| Constraints | Lizard: "no function may exceed CCN 40 (current max is 38, the user/store.rs quartet …)"; "1.24.0 is the release the ratchet was validated against locally"; no Lizard config file exists — threshold and language are the CLI flags. Harness runs before the checker ("a regression in the checker itself must not pass quietly") |

### CI-03 — .github/workflows/ci.yml (job: test)

| Field | Value |
|---|---|
| Entry point | job `test`, `needs: changes`, gated; `timeout-minutes: 30`; bash; `DEBIAN_FRONTEND` |
| Owning subsystem | ci |
| Purpose | The ONE full workspace `cargo test` per PR (default backend = Berkeley DB), plus the C++ header smoke, the two ABI gates and the self-skipping live-typing differential |
| Implementation | `container: debian:testing`; same apt line as lint; bootstrap `safe-directory rustup rust-cache key=test-job` |
| Inputs | `sha256sum -c fixtures/w3/fixtures.sha256` · `cargo test --locked --workspace` · `tools/bisection/run-cpp-smoke.sh` · `tools/abi/check-exports.sh` · `tools/abi/check-exports.sh --shipped` · `tools/abi/check-alloc-pairing.sh` · `tools/bisection/run-live-typing-diff.sh`. The four tools scripts source `tools/bisection/oracle-cell.sh`, which reads `PINYIN_ORACLE_DBM` (default bdb), `CARGO_TARGET_DIR`/`OXPINYIN_TARGET_ROOT`, `OXPINYIN_CAPI_SO`, `OXPINYIN_ZHUYIN_SO` and the constants `LIBPINYIN_VERSION/LIBPINYIN_SHA/MODEL_SHA256` from `tools/oracle/build-oracle.sh`, and exports `CARGO_PROFILE_DEV_OPT_LEVEL=1`, `OXPINYIN_CAPI_BACKEND_EXT`. run-live-typing-diff.sh additionally reads `PINYIN_ORACLE_PREFIX` (default `$HOME/.local/opt/pinyin-oracle`), `LIVETYPING_SYSTEM`, `OXPINYIN_SYSTEM_DIR`, `LIVETYPING_ROUNDS`; run-cpp-smoke.sh reads `OXPINYIN_CAPI_BACKEND_EXT` |
| Outputs | Exit codes. check-exports: 0 iff both cdylibs export exactly `libpinyin.ver` (79) / `libzhuyin.ver` (52). check-alloc-pairing: static register coverage + a C++ consumer under ASan/LSan with a negative control (`-DOXPINYIN_ALLOC_PAIRING_LEAK` build must fail). run-live-typing-diff: `0 = identical or skipped; 1 = build/run failure; 2 = divergence` — in CI it always prints `SKIP: pin-built oracle not found` and exits 0 |
| Platform | Linux container only: `nm -D` (Linux arm of check-exports), LSan ptrace stop-the-world (check-alloc-pairing header: "Linux only"), `gcc … -ldl` driver in live-typing-diff, glib for capi |
| Consumers | `ci-aggregate` |
| State | rust-cache `test-job`; the tools scripts build capi into `$CARGO_TARGET_DIR` (default `target/cells/bdb`) via `oracle_cell_artifact`; mktemp build dirs with `trap rm -rf` |
| Overlap | store-backends `store-features` re-runs the workspace tests per backend (minus four crates); check-exports/alloc-pairing are also `runners.json` entries for the local aggregate (parity-gates.md: "The registry also includes the ABI export/allocation gates, static package version check …") |
| Constraints | w3 manifest is a **check**, never regenerated in CI ("running [the regeneration] in CI would overwrite the very drift this must catch"). Live-typing differential "Self-skips unless the pin-built oracle … and a real-unigram system dir … are present — runner-local only: the model20 archive is non-redistributable and never enters CI". Note: this is a deliberate self-skip of a *script*, kept for CI (parity-gates.md: "direct `run-live-typing-diff.sh` keeps its legacy missing-oracle exit behavior for CI"), whereas Rust tests may not self-skip |

### CI-04 — .github/workflows/ci.yml (job: test-macos)

| Field | Value |
|---|---|
| Entry point | job `test-macos`, gated; `runs-on: macos-latest`; `timeout-minutes: 45`; no container; bash is the macOS default shell |
| Owning subsystem | ci |
| Purpose | The portable-workspace lane for the path README documents to macOS users: Homebrew tkrzw, explicit `--features tkrzw` |
| Implementation | bootstrap `rust-cache: 'true'` only (no `rustup` input — the runner's preinstalled rustup honours `rust-toolchain.toml`); `brew install tkrzw glib pkgconf`; `LIBRARY_PATH=$(brew --prefix)/lib` → `GITHUB_ENV`; `brew list --versions tkrzw \| grep -Eq '^tkrzw 1[.]'` else `::error::` exit 1 |
| Inputs | `cargo test --locked --workspace --no-default-features --features tkrzw --exclude oxpinyin-capi --exclude oxpinyin-zhuyin-capi --exclude oxpinyin-corpus --exclude oxpinyin-kmm --exclude oxpinyin-punct --exclude oxpinyin-word` |
| Outputs | Exit code. Failure = Homebrew tkrzw off the 1.x line, a link failure (lz4/lzma/zstd), or a portable-crate test failure under tkrzw |
| Platform | macOS runner |
| Consumers | `ci-aggregate` |
| State | rust-cache (automatic key) |
| Overlap | test-windows (same exclude set, different backend); store-backends tkrzw child (Linux, same feature, different exclude set — it keeps capi/zhuyin-capi) |
| Constraints | Excludes: "the two C-ABI crates (glib and cargo-c, Linux-only) and the backend-independent corpus, KMM, punctuation, and word tools. The combined training package remains in these lanes because its segment/eval/train modules use the backend. oxpinyin-dictool stays in the set: its C-ABI dependency was cut". `--no-default-features` "also keeps pinyin-oracle's Linux-only `oracle-ffi` off". "Two jobs rather than a matrix: containers run on Linux runners only" |

### CI-05 — .github/workflows/ci.yml (job: test-windows)

| Field | Value |
|---|---|
| Entry point | job `test-windows`, gated; `runs-on: windows-latest`; `timeout-minutes: 45`; `defaults.run.shell: bash` (pwsh default would leave `$VCPKG_INSTALLATION_ROOT` unset and `>> "$GITHUB_ENV"` writing nowhere) |
| Owning subsystem | ci |
| Purpose | Portable-workspace lane on Windows riding the workspace default (Berkeley DB) linked from vcpkg's 4.8.30 port |
| Implementation | bootstrap `rust-cache` only; `vcpkg install berkeleydb:x64-windows`; `vcpkg list berkeleydb:x64-windows \| grep -Eq 'berkeleydb:x64-windows[[:space:]]+4[.]8[.]'` else fail |
| Inputs | Env set: `OXPINYIN_BDB_INCLUDE_DIR=$VCPKG_INSTALLATION_ROOT\installed\x64-windows\include`, `OXPINYIN_BDB_LIB_DIR=…\lib`, `OXPINYIN_BDB_LIB_NAME=libdb48`, `OXPINYIN_BDB_ALLOW_UNSURVEYED_VERSION=1`, `LIBCLANG_PATH=C:\Program Files\LLVM\bin`; `GITHUB_PATH += …\installed\x64-windows\bin`. Command: `cargo test --locked --workspace --exclude oxpinyin-capi --exclude oxpinyin-zhuyin-capi --exclude oxpinyin-corpus --exclude oxpinyin-kmm --exclude oxpinyin-punct --exclude oxpinyin-word` |
| Outputs | Exit code. Failure = vcpkg port off 4.8, bindgen/link failure, or a test failure under BDB 4.8 |
| Platform | Windows runner |
| Consumers | `ci-aggregate` |
| State | rust-cache |
| Overlap | test-macos |
| Constraints | `OXPINYIN_BDB_ALLOW_UNSURVEYED_VERSION` "is set only here: it covers that one comparison, and the process prints a warning naming both versions. No other lane sets it" (backends.md: "CI-only … A developer recipe does not set it"). The store refuses any libdb other than the surveyed 5.3 otherwise |

### CI-06 — .github/workflows/ci.yml (job: deny)

| Field | Value |
|---|---|
| Entry point | job `deny`, gated; `container: debian:testing`; bash; no `timeout-minutes` (default 360) |
| Owning subsystem | ci |
| Purpose | Supply-chain gate: RustSec advisories, yanked, licenses, sources, duplicate versions — over the default graph, `--all-features`, and the fuzz workspace |
| Implementation | bootstrap `safe-directory rustup` (minimal profile; cargo-deny shells out to `cargo metadata`); cargo-deny **0.20.2** downloaded by curl from GitHub releases (`x86_64-unknown-linux-musl`), `sha256sum -c` against pinned digest `9f12ed4c…8d5f`, installed to `~/.local/bin` |
| Inputs | `cargo deny --locked --log-level error check` · `cargo deny --locked --all-features --log-level error check` · `cargo deny --locked --log-level error --manifest-path fuzz/Cargo.toml check`. Reads `deny.toml` (CI-30), `Cargo.lock`, `fuzz/Cargo.lock` |
| Outputs | Exit code. `--locked` on the fuzz graph: "A root `/` bump that changes workspace pins or path crates reachable from fuzz, and does not refresh fuzz/Cargo.lock, fails --locked. That failure is the gate" |
| Platform | Linux container (musl binary) |
| Consumers | `ci-aggregate` |
| State | none (no rust-cache) |
| Overlap | None; replaces cargo-audit ("consumes the same RustSec DB … so this one job replaces both tools"). Dependabot (CI-31) maintains both locks |
| Constraints | "The binary is downloaded pinned rather than `cargo install`ed to keep the job under a minute." Advisory ignore list is empty; the former bincode scope guard was removed with heed |

### CI-07 — .github/workflows/ci.yml (job: ci-aggregate)

| Field | Value |
|---|---|
| Entry point | job `ci-aggregate`, `needs: [changes, lint, test, test-macos, test-windows, deny]`, `if: always()`; `ubuntu-latest`, no container |
| Owning subsystem | ci |
| Purpose | The one required-status-check shape for ci.yml: red when any lane that should have run ended non-success, or a lane was `skipped` while `GATED == true` |
| Implementation | Inline bash `lane()` function over env-bound results |
| Inputs | env `CHANGES=needs.changes.result`, `GATED=needs.changes.outputs.run`, `LINT`, `TEST`, `TEST_MACOS`, `TEST_WINDOWS`, `DENY` |
| Outputs | exit 0 ("all lanes green (gated=…)") or 1 with `::error::` per lane |
| Platform | GitHub-hosted Linux |
| Consumers | Branch protection (required set `ci-aggregate`, `trailer-lint`, `trailer-test`) |
| State | none |
| Overlap | `store-backends-gate` (CI-18) is the same pattern for store-backends.yml but is **not** a required check |
| Constraints | "store-backends.yml and verify-nightly.yml keep their own triggers and remain non-required lanes beside this one" |

---

## .github/workflows/commit-trailers.yml

Workflow: `on: pull_request: types [opened, synchronize, reopened,
ready_for_review, edited]`; `permissions: contents: read`. Job names are the
required-check contexts and "must not collide with CI's `lint` / `test`".

### CI-08 — .github/workflows/commit-trailers.yml (job: trailer-lint)

| Field | Value |
|---|---|
| Entry point | job `trailer-lint` (`name: trailer-lint`); `container: debian:testing`; `timeout-minutes: 30`; bash |
| Owning subsystem | ci |
| Purpose | Lints every non-merge commit the PR introduces for the Assisted-by house form (R2) |
| Implementation | apt essentials only ("the linter is pure shell plus git, so the jobs need only the bare-runner essentials — no rustup, no build deps"); checkout `fetch-depth: 0`; bootstrap `safe-directory` |
| Inputs | env `BASE_REF=github.base_ref`, `HEAD_SHA=github.event.pull_request.head.sha`; `git fetch --no-tags origin "+refs/heads/${BASE_REF}:refs/remotes/origin/${BASE_REF}"`; `MB=$(git merge-base origin/$BASE_REF $HEAD_SHA)`; `.github/scripts/lint-commits.sh "$MB" "$HEAD_SHA"` |
| Outputs | lint-commits.sh exit 0/1/2; `::error::` lines; summary table into `$GITHUB_STEP_SUMMARY` |
| Platform | Linux container (git ≥ 2.32 required by the script) |
| Consumers | Branch protection (required) |
| State | none |
| Overlap | `.githooks/commit-msg` (CI-29) runs the same script in `--hook` mode |
| Constraints | Merge-base is computed at lint time, not from the stale event payload |

### CI-09 — .github/workflows/commit-trailers.yml (job: trailer-test)

| Field | Value |
|---|---|
| Entry point | job `trailer-test`; `container: debian:testing`; `timeout-minutes: 30`; bash |
| Owning subsystem | ci |
| Purpose | Gates the linter itself: runs the fixture harness so a change to the script or workflow cannot silently regress rule logic |
| Implementation | apt essentials; bootstrap `safe-directory` |
| Inputs | `tests/lint-commits.test.sh` (POSIX sh + git; builds a throwaway repo in mktemp, exercises both CI mode and `--hook` mode; `unset GITHUB_STEP_SUMMARY`) |
| Outputs | TAP-ish `ok`/`not ok` lines; nonzero on any failure |
| Platform | Linux container |
| Consumers | Branch protection (required) |
| State | temp git repo |
| Overlap | The same harness pattern as `tests/lint-crate-cfg-placement.test.sh` (run in ci.yml lint) |
| Constraints | — |

---

## .github/workflows/coverage.yml

Workflow: `on: pull_request: branches [main]` with `paths:` =
`crates/oxpinyin-{core,engine,user,data,facade,training,chewing,capi-marshal,store,runtime,capi,zhuyin-capi}/**`,
`.github/workflows/coverage.yml`, `.github/actions/**`; plus `workflow_call`
(verify-nightly calls it unconditionally). `concurrency:
coverage-${{ github.workflow }}-${{ github.ref }}`, `cancel-in-progress: true`.

### CI-10 — .github/workflows/coverage.yml (job: coverage)

| Field | Value |
|---|---|
| Entry point | job `coverage`; `container: debian:testing`; bash; no `timeout-minutes` |
| Owning subsystem | ci |
| Purpose | Line-coverage floor (ratchet) for the library-path crates: PR gate when they change, nightly whole-tree number via workflow_call |
| Implementation | apt essentials + `gcc g++ make libc6-dev pkg-config libclang-dev libkyotocabinet-dev libglib2.0-dev`; bootstrap `safe-directory rustup rust-cache`; cargo-llvm-cov **0.9.0** curl'd from GitHub releases (musl), `sha256sum -c` against `bc2122fd…2616`, installed to `~/.local/bin` |
| Inputs | `cargo llvm-cov --locked --workspace --no-default-features --features kyotocabinet --lcov --output-path lcov.info` · `cargo llvm-cov report --summary-only --fail-under-lines 77` |
| Outputs | `lcov.info` uploaded as artifact `coverage-lcov` (`actions/upload-artifact@cf430e03…` v7.0.2); per-crate table in the log; failure = lines < 77% |
| Platform | Linux container (musl binary; glib for capi) |
| Consumers | PR path filter; `verify-nightly.coverage` (`uses: ./.github/workflows/coverage.yml`) |
| State | rust-cache (automatic key) |
| Overlap | Backend choice (**kyotocabinet**, not the bdb default) matches overflow-lane and nextest but not ci.yml test |
| Constraints | "Floor 77 = the measured number. Raise it when the number rises, never lower it to pass." "Measured 2026-09-08 on this build: 77.33% lines". Note: docs/safety/ci-strategy.md names `segment` in the path list; the workflow has `oxpinyin-training` (doc drift) |

---

## .github/workflows/oracle-image.yml

Workflow: `on: workflow_dispatch`; `push: branches [main]` and
`pull_request: branches [main]`, both with `paths:` =
`tools/oracle/{build-oracle.sh,Dockerfile.oracle,oracle-pin.txt,verify-oracle-image.sh,image-tag.sh}`,
`.github/workflows/oracle-image.yml`. `concurrency: oracle-image-${{ github.ref }}`,
`cancel-in-progress: false`.

### CI-11 — .github/workflows/oracle-image.yml (job: oracle-image)

| Field | Value |
|---|---|
| Entry point | job `oracle-image`; `ubuntu-latest`, **no container** (needs the runner's Docker); `timeout-minutes: 90`; job `permissions: contents: read, packages: write` |
| Owning subsystem | oracle provisioning |
| Purpose | Reproducibly builds the pin-built oracle LIBRARIES (libpinyin.so/libzhuyin.so, headers, .pc, oracle-pin.txt for bdb/kc/tkrzw) in the digest-pinned image, verifies every hash against `oracle-pin.txt`, and publishes `ghcr.io/shenghaoc/oxpinyin-oracle:<tag>` from main/dispatch only |
| Implementation | `docker/setup-buildx-action@f87e5991…` (v4.4.1); `docker/build-push-action@c3c9e263…` (v7.4.0) with `context: tools/oracle`, `file: tools/oracle/Dockerfile.oracle`, `platforms: linux/amd64`, `load: true`, `provenance: false`, `tags: oxpinyin-oracle:candidate`; `docker/login-action@dbcb8138…` (v4.6.0) |
| Inputs | Meta step parses `tools/oracle/oracle-pin.txt` with `sed -n "s/^$1=//p"` for `libpinyin_commit`, `base_image`, `libpinyin_sha256_{bdb,kc,tkrzw}`, `libzhuyin_sha256_{bdb,kc,tkrzw}`; `tools/oracle/image-tag.sh` (tag = `<commit12>-<base digest12>-<apt_snapshot>-<sha256(Dockerfile.oracle+build-oracle.sh)12>`; fails if Dockerfile disagrees with pin's `base_image`/`apt_snapshot`). Verify: `tools/oracle/verify-oracle-image.sh --engine docker oxpinyin-oracle:candidate` (docker create/cp `/opt/pinyin-oracle`, sha256 each `.so` vs pin, `find` for `*/lib/libpinyin/data`, `*.bin`, `*.table`, `*.text`, `bigram.db` → FAIL). Push: `secrets.GITHUB_TOKEN`, `TAG` env; `docker manifest inspect` → exists: `::notice::` exit 0; unknown error: exit 1; else `docker tag` + `docker push` |
| Outputs | GHCR image (labels `org.opencontainers.image.{source,revision,description}`, `io.github.shenghaoc.oxpinyin.{libpinyin-commit,base-image,<dbm>.libpinyin-sha256,<dbm>.libzhuyin-sha256}`); on PRs build+verify only |
| Platform | Linux runner with Docker; image `linux/amd64` only ("native shared objects differ across architectures") |
| Consumers | `tools/cloud/setup.sh` (pull or build fallback, hash check, exit 3 on mismatch), docs/runbooks/oracle.md §2a, docs/runbooks/benches.md ("The pin comes from the reproducible oracle") |
| State | Docker image store on the runner; immutable tag ("an existing tag is never overwritten") |
| Overlap | `tools/oracle/check-reproducible.sh` (local double build) per the pin record's v4 note; `tools/cloud/setup.sh` re-implements the hash check |
| Constraints | "The image carries NO model20-derived data … no job here fetches it." Pin schema `oracle-provisioning-pin-v4`: "A pin component, the base image, the apt snapshot or the recipe changing changes [the hashes]: update them in the same commit." The job's identity check is the recorded `.so` sha256 per cell, not a commit check |

---

## .github/workflows/release-packages.yml

Workflow: `name: Release packages`; `on: release: types [published]`;
`concurrency: release-packages-${{ github.event.release.id }}`,
`cancel-in-progress: false` ("Queue, never cancel"). Header carries the
distro→backend table (Debian tkrzw, Fedora kyotocabinet, Arch kyotocabinet)
and the takeover semantics.

### CI-12 — .github/workflows/release-packages.yml (job: package)

| Field | Value |
|---|---|
| Entry point | job `package` (`name: <distro> (<backend> store)`), matrix `include`: `debian/debian:testing/tkrzw/Tkrzw`, `fedora/fedora:latest/kyotocabinet/KyotoCabinet`, `arch/archlinux:latest/kyotocabinet/KyotoCabinet`; `fail-fast: false`; `container: ${{ matrix.image }}`; `timeout-minutes: 90`; bash |
| Owning subsystem | packaging |
| Purpose | Builds oxpinyin-capi + oxpinyin-zhuyin-capi under exactly one backend with the distro's own cargo-c, packages them in the distro's shape, installs them back into the same container and re-runs the consumer gates against `/usr` (including `pinyin_init` on the distro's real model) |
| Implementation | Distro-native install step (`apt-get`/`dnf`/`pacman`, closed `case` over `DISTRO`, `exit 2` default) incl. `cargo-c`, backend -dev, glib, packaging tools (`rpm-build`, `fakeroot debugedit`); bootstrap `safe-directory rustup rust-cache key=release-<distro>`; `LIBDIR` per distro (`/usr/lib/$(gcc -print-multiarch)`, `/usr/lib64`, `/usr/lib`); `VERSION` from `github.event.release.tag_name` matching `^v?([0-9]+\.[0-9]+\.[0-9]+)$` else `::error::` exit 1 |
| Inputs | `tools/packaging/release-stage.sh "$BACKEND" --prefix=/usr --libdir="$LIBDIR" --dest="$PWD/stage"` (runs `tools/packaging/install.sh` → `cargo cinstall`, strips, gates SONAME/pkg-config/C smoke); then `tools/packaging/release-deb.sh "$BACKEND" "$VERSION" "$PWD/stage" "$LIBDIR" dist` / `release-rpm.sh "$BACKEND" "$VERSION" "$PWD/stage" dist` / Arch: `pacman -Sw libpinyin`, `pacman -Sp -dd --print-format '%v %l' libpinyin`, `tar -xf … --strip-components=4 usr/lib/libpinyin/data`, `release-arch.sh "$BACKEND" "$VERSION" "$PWD/stage" dist --data="$PWD/arch-data" --data-version="$DATA_VER"`. Install test: `apt-get install -y libpinyin-data dist/*.deb` / `dnf -y install libpinyin-data dist/*.rpm` / `pacman -U dist/*.pkg.tar.zst`; `ldconfig`; `PCVER=$(tools/packaging/pc-version.sh)` (reads `libpinyin_tag` from oracle-pin.txt); pkg-config modversion/`database_format`==`DBFMT`/pkgdatadir/--libs/--cflags checks; two heredoc C programs (`pinyin_init`, `zhuyin_init`) compiled with `cc … $(pkg-config --cflags --libs lib{pinyin,zhuyin})`, run against `$(pkg-config --variable=pkgdatadir libpinyin)/data` with a mktemp user dir — NULL context = failed gate |
| Outputs | `actions/upload-artifact` `packages-<distro>` from `dist/*`, `if-no-files-found: error` |
| Platform | Linux containers only (dpkg-deb, rpmbuild, makepkg, ldconfig, `gcc -print-multiarch`) |
| Consumers | `attach` |
| State | rust-cache per distro; staging under `$PWD/stage`; packages install into the container's `/usr` |
| Overlap | Smoke C programs duplicate the shape of `release-stage.sh`'s staged-tree smoke (that one has no data; this one opens the system model) and `tools/bisection/run-cpp-smoke.sh` |
| Constraints | Debian lane must be `debian:testing` ("stable (trixie …) still ships libpinyin 2.8.1 on BerkeleyDB, whose bigram.db no oxpinyin backend can read"). "Data is deliberately NOT replaced on Debian/Fedora"; Arch ships the data extracted from the distro's own package. Stable `vX.Y.Z` tags only |

### CI-13 — .github/workflows/release-packages.yml (job: attach)

| Field | Value |
|---|---|
| Entry point | job `attach`, `needs: package`; `ubuntu-latest`, not containerized ("gh ships on the hosted runner"); job `permissions: contents: write` |
| Owning subsystem | packaging |
| Purpose | Collects every distro's packages, writes `SHA256SUMS`, uploads all to the published release |
| Implementation | `actions/download-artifact@9000827c…` (v8.0.2) `pattern: packages-*`, `merge-multiple: true`, `path: dist`; `sha256sum -- * > SHA256SUMS`; `gh release upload "$TAG" dist/* --clobber` |
| Inputs | env `GH_TOKEN=github.token`, `TAG=github.event.release.tag_name` |
| Outputs | Release assets; `--clobber` so a re-run replaces partial uploads |
| Platform | GitHub-hosted Linux |
| Consumers | Release consumers |
| State | none |
| Overlap | — |
| Constraints | One run per release id (concurrency) |

---

## .github/workflows/store-backends.yml

Workflow: `name: Store backends`; `on: push`/`pull_request` on `main` with
identical `paths:` = `crates/**`, `Cargo.toml`, `Cargo.lock`,
`rust-toolchain.toml`, `.github/workflows/store-backends.yml`, `fixtures/**`,
`tools/store/**`, `tools/kc/**`, `tools/bdb/**`, `.github/actions/**`.
`concurrency: store-backends-${{ github.ref }}`, `cancel-in-progress: true`.
Header: peers are kc/tkrzw/bdb, exactly one per build (two `compile_error!`
guards in `crates/oxpinyin-store/src/lib.rs`); bdb default since 2026-09-20;
ROLLING TOOLCHAIN rationale; "The pin-built oracle and the model20 archive
are deliberately NOT CI concerns".

### CI-14 — .github/workflows/store-backends.yml (job: store-features)

| Field | Value |
|---|---|
| Entry point | job `store-features` (`name: store backend (<backend>)`); matrix `backend: [kyotocabinet, tkrzw, bdb]`, `fail-fast: false`, `max-parallel: 4`; `container: debian:testing`; `timeout-minutes: 45`; bash |
| Owning subsystem | ci |
| Purpose | Runs the backend-relevant workspace test suite under exactly one peer per child, in parallel, so cross-peer equivalence is proven by the same suite through separate builds |
| Implementation | bootstrap `safe-directory rustup rustup-components="clippy rustfmt" rust-cache key=store-backend-<backend>`; apt (every child installs the full set): `gcc g++ make pkg-config libclang-dev libkyotocabinet-dev libtkrzw-dev libdb-dev liblzma-dev liblz4-dev libzstd-dev zlib1g-dev libglib2.0-dev`; closed `case` over `BACKEND` env (`exit 2` default) |
| Inputs | kyotocabinet: `cargo test --locked --workspace --no-default-features --features kyotocabinet --exclude oxpinyin-corpus --exclude oxpinyin-kmm --exclude oxpinyin-punct --exclude oxpinyin-word`; tkrzw: same with `--features tkrzw`; bdb: `cargo test --locked --workspace <excludes>` (default) |
| Outputs | step output `elapsed`; `$GITHUB_STEP_SUMMARY` table (`if: always()`) |
| Platform | Linux container |
| Consumers | `store-backends-gate` |
| State | rust-cache per backend ("without it the feature configurations would contend one automatic key"); "no shared writable target across the matrix" |
| Overlap | ci.yml `test` (bdb, full workspace incl. the four excluded crates); test-macos/test-windows (portable subset) |
| Constraints | Bare `--features <peer>` at `--workspace` scope is "the documented public interface — see docs/findings/backend-selection-audit.md"; a passthrough `--features oxpinyin-store/<peer>` would not cascade |

### CI-15 — .github/workflows/store-backends.yml (job: backend-matrix)

| Field | Value |
|---|---|
| Entry point | job `backend-matrix` (`name: backend-matrix (exactly-one-backend gate)`); independent (no `needs`); `container: debian:testing`; `timeout-minutes: 30` |
| Owning subsystem | ci |
| Purpose | Proves every valid single-peer selection `cargo check`s and every pairwise/three-way/zero-backend combination is refused by the `compile_error!` guard |
| Implementation | bootstrap with clippy/rustfmt components, rust-cache key `backend-matrix`; same full apt set |
| Inputs | `tools/store/backend-matrix.sh` — `cargo check --locked -p oxpinyin-store` for `""`, `--no-default-features --features {kyotocabinet,tkrzw,bdb}`; then `--features "kyotocabinet,tkrzw"`, `"kyotocabinet,bdb"`, `"tkrzw,bdb"`, `"kyotocabinet,tkrzw,bdb"` and the zero-backend case must fail with `more than one store backend selected` (grep) |
| Outputs | PASS/FAIL per phase to stdout; nonzero on any deviation; `elapsed` to step summary |
| Platform | Linux container (check only — "libtkrzw/etc. do not have to be linkable") |
| Consumers | `store-backends-gate`; backends.md "The exactly-one-backend gate" |
| State | `mktemp -t backend-matrix.XXXXXXXX` log, `trap rm -f` |
| Overlap | None; "replaced the old 'cross-backend conformance in one binary' job, which is architecturally impossible under exactly-one-backend" |
| Constraints | Script's own header says "each of the four explicit ones" in backends.md while it checks three explicit peers + default (doc drift) |

### CI-16 — .github/workflows/store-backends.yml (job: tkrzw-sanitizers)

| Field | Value |
|---|---|
| Entry point | job `tkrzw-sanitizers` (`name: tkrzw FFI under <sanitizer>`); matrix `sanitizer: [address, undefined]`, `fail-fast: true`; `container: debian:testing` with `options: --security-opt seccomp=unconfined`; `timeout-minutes: 45`; job env `RUSTUP_TOOLCHAIN: nightly-2026-09-07` |
| Owning subsystem | ci |
| Purpose | Runs the store crate's tkrzw suite under ASan (Rust-side instrumentation of the bindgen FFI) and a UBSan-linked arm; any finding fails the merge (blocking since 2026-09-08) |
| Implementation | apt essentials + `python3`, then `gcc g++ make pkg-config util-linux libclang-dev libtkrzw-dev liblzma-dev liblz4-dev libzstd-dev zlib1g-dev`; bootstrap `safe-directory` only; nightly installed inline via `curl https://sh.rustup.rs \| sh -s -- -y --profile minimal --default-toolchain "$RUSTUP_TOOLCHAIN"`; **no rust-cache** by design |
| Inputs | `triple=$(rustc -vV \| sed -n 's/^host: //p')`. address: `RUSTFLAGS="-D warnings -Zsanitizer=address -Cdebuginfo=1"`, `CFLAGS="-fsanitize=address -fno-sanitize-recover=all -fno-omit-frame-pointer -g"`, `CXXFLAGS=$CFLAGS`, `ASAN_OPTIONS=detect_leaks=0:abort_on_error=1`, `setarch "$(uname -m)" -R cargo test --locked --target "$triple" -p oxpinyin-store --no-default-features --features tkrzw`. undefined: `RUSTFLAGS="-D warnings -C linker=cc -Clink-arg=-fsanitize=undefined -Clink-arg=-lubsan"`, CFLAGS/CXXFLAGS with `-fsanitize=undefined`, same cargo test without setarch |
| Outputs | step summary "clean — zero findings" / "FINDINGS PRESENT — STOP"; `elapsed` |
| Platform | Linux container; needs `personality(2)` (seccomp unconfined) for `setarch -R`; nightly rustc |
| Consumers | `store-backends-gate` |
| State | none cached; `--target` explicit so host build scripts/proc-macros stay uninstrumented |
| Overlap | `tools/kc/run-sanitizers.sh` and `tools/bdb/run-sanitizers.sh` are script-form equivalents for the other peers; this lane inlines its recipe instead of calling a script |
| Constraints | "Date-pinned so a nightly regression cannot red the gate on its own; bump the date deliberately, in its own commit". LSan off by documented choice. Prebuilt std stays uninstrumented (recorded in upstream-divergences.md). "UBSan is close to a no-op without C targets" |

### CI-17 — .github/workflows/store-backends.yml (job: bdb-sanitizers)

| Field | Value |
|---|---|
| Entry point | job `bdb-sanitizers` (`name: bdb backend sanitizers`); `container: debian:testing`, seccomp unconfined; `timeout-minutes: 45`; env `RUSTUP_TOOLCHAIN: nightly-2026-09-07` |
| Owning subsystem | ci |
| Purpose | Berkeley DB FFI under ASan (Rust) plus the C call-pattern harnesses under ASan+UBSan — load-bearing because on a BDB distro every session opens the system bigram.db through this FFI |
| Implementation | apt essentials + python3; `gcc g++ make pkg-config util-linux libclang-dev libdb-dev`; bootstrap `safe-directory`; inline nightly rustup install |
| Inputs | `bash tools/bdb/run-sanitizers.sh` — reads `OXPINYIN_SANITIZER_TOOLCHAIN` (default `nightly-2026-09-07`), `OXPINYIN_SANITIZER_TARGET` (default host triple), `CARGO_TARGET_DIR`, `OXPINYIN_LIBPINYIN_DATA_DIR` (else probes five fixed `/usr/lib*/libpinyin/data` paths); runs `RUSTFLAGS="-Zsanitizer=address -Cdebuginfo=1" cargo +$NIGHTLY test -p oxpinyin-store --no-default-features --features bdb --target $TARGET`; `gcc … -fsanitize=address,undefined -fno-sanitize-recover=all … tools/bdb/hash-walk.c -ldb` and `btree-order.c`; `cargo +$NIGHTLY build … --example bdb_write_profile`; walks the backend-written db and (if present) an installed `bigram.db` |
| Outputs | Exit 0 clean / nonzero finding or build failure; the script errors (exit 1) if the nightly is not installed ("sanitizer gate did NOT run") |
| Platform | Linux container (libdb, gcc sanitizers, `setarch` not used here) |
| Consumers | `store-backends-gate` |
| State | `mktemp -d` work dir with trap |
| Overlap | `tools/kc/run-sanitizers.sh` (kc twin; **not run by any workflow** though `tools/kc/**` is in this workflow's path gate and the gate comment claims "the sanitizer jobs run tools/kc/run-sanitizers.sh and tools/bdb/run-sanitizers.sh"); CI-16 |
| Constraints | "libdb's own internals are NOT instrumented". kc script defaults to `nightly-2026-08-01` and `x86_64-unknown-linux-gnu`, and has an `OXPINYIN_SANITIZER_ALLOW_SKIP` self-skip the bdb script does not |

### CI-18 — .github/workflows/store-backends.yml (job: store-backends-gate)

| Field | Value |
|---|---|
| Entry point | job `store-backends-gate` (`name: store backends (kyotocabinet / tkrzw / bdb)`); `if: always()`; `needs: [store-features, backend-matrix, tkrzw-sanitizers, bdb-sanitizers]`; `ubuntu-latest` |
| Owning subsystem | ci |
| Purpose | Stable aggregate status: red unless every matrix child, the exactly-one-backend gate and both sanitizer jobs succeeded |
| Implementation | env-bound `FEATURES_RESULT`, `MATRIX_RESULT`, `SANITIZER_RESULT`, `BDB_SANITIZER_RESULT`; summary table; `exit 1` unless all `success` |
| Inputs | `needs.*.result` |
| Outputs | step summary; exit code |
| Platform | hosted Linux |
| Consumers | Not a required check ("main's required-status-check contexts are `ci-aggregate`, `trailer-lint`, and `trailer-test`") |
| State | none |
| Overlap | `ci-aggregate` (CI-07) — different rule: this one has no path-gate justification for `skipped` |
| Constraints | Display name "is free to track the set" since 2026-09-08 |

---

## .github/workflows/verify-nightly.yml

Workflow: `on: schedule: cron '0 3 * * *'`, `workflow_dispatch`;
`concurrency: verify-nightly`, `cancel-in-progress: false`. Header: Tier-3
scheduled verification; "There are no continue-on-error lanes; every job in
this workflow must be green"; lists surviving lanes as "fuzz-soak,
differentials, overflow-lane, coverage (via coverage.yml …), public-api,
nextest" — **perf-snapshot is absent from that header list** (drift).

### CI-19 — .github/workflows/verify-nightly.yml (job: fuzz-soak)

| Field | Value |
|---|---|
| Entry point | job `fuzz-soak`; `container: debian:testing` with `--security-opt seccomp=unconfined`; **no `timeout-minutes`** (GitHub default 360); bash |
| Owning subsystem | fuzz |
| Purpose | The only fuzz lane: every `cargo fuzz list` target soaks 3 min under ASan+LSan nightly; crashes survive as artifacts |
| Implementation | apt essentials + `gcc g++ make libc6-dev pkg-config libclang-dev libdb-dev libglib2.0-dev util-linux`; bootstrap `safe-directory`; inline rustup `--default-toolchain nightly-2026-08-01 --component rust-src`; `cargo install cargo-fuzz --version 0.13.2 --locked` |
| Inputs | `actions/cache@55cc8345…` (v6.1.0) `path: fuzz/corpus`, `key: fuzz-corpus-${{ github.run_id }}`, `restore-keys: fuzz-corpus-` (latest-wins); seed `fixtures/w3/*.bin` → `fuzz/corpus/dict-loader/` (`::warning::` if none); `mapfile -t targets < <(cargo +nightly-2026-08-01 fuzz list)` (`::error::` exit 1 if empty); per target: `setarch "$(uname -m)" -R cargo +nightly-2026-08-01 fuzz run "$target" -- -max_total_time=180 -timeout=15 -max_len=4096 -print_final_stats=1`. Targets (fuzz/Cargo.toml `[[bin]]`): parser, dict-loader, fixture-model, double-pinyin, capi-commands, phrase-library, table-conf, chewing-parser, session-keys, user-store-ops (10). Backend: workspace default (bdb) via path deps; capi built with `fuzz-api` |
| Outputs | Accumulated status (every target runs even after a failure); `::group::` per target; on failure `actions/upload-artifact` `fuzz-crash-artifacts` from `fuzz/artifacts/` |
| Platform | Linux container; `personality(2)` for `setarch -R`; libFuzzer |
| Consumers | Nightly only; ci-strategy Tier 3; safety-profile Layer 7 |
| State | Corpus cache (cross-run, latest-wins); committed seeds `fuzz/corpus/parser/zhuan`, `fuzz/corpus/session-keys/9131…`; `fuzz/Cargo.lock` separate |
| Overlap | None now ("ci.yml's per-PR build and smoke pass moved here on 2026-09-23"); deny job checks the fuzz graph |
| Constraints | "The list comes from `cargo fuzz list`, so a new target cannot be left out of the soak" (the previous five-name literal silently skipped three). `fuzz/Cargo.toml`: capi-commands "Linux-only, like the nightly fuzz soak — as is capi" |

### CI-20 — .github/workflows/verify-nightly.yml (job: differentials)

| Field | Value |
|---|---|
| Entry point | job `differentials`; `container: debian:testing`; `timeout-minutes: 60`; bash |
| Owning subsystem | oracle provisioning / bisection (fixture tier) |
| Purpose | Builds the pinned libpinyin (tkrzw) from public source inside CI and runs every oxpinyin↔libpinyin differential that COMMITTED fixtures can carry (ngseg live, KMM gen/export/to-interp/merge/validate, spseg live, the tkrzw drop-in datagen parity) — the nightly drift alarm honoring no-model20-in-CI |
| Implementation | apt essentials + `gcc g++ make pkg-config autoconf automake autopoint libtool gettext libglib2.0-dev libtkrzw-dev liblzma-dev liblz4-dev libzstd-dev zlib1g-dev libclang-dev`; bootstrap `safe-directory rustup rust-cache` |
| Inputs | `tools/oracle/nightly-fixture-differentials.sh` (205 lines, read in full): reads `LIBPINYIN_VERSION`, `LIBPINYIN_SHA`, `LIBPINYIN_GIT_URL` from `tools/oracle/build-oracle.sh` by `sed`; work dir default `/tmp/oxpinyin-fixture-differentials`; `git init` + `git fetch --depth=1 "$pin_url" "$pin_sha"`; **identity check** `git rev-parse FETCH_HEAD == $pin_sha` else exit 2; `.pin-ok` marker; seeds `fixtures/datagen-toned/*.table` + `interpolation2.text` into the source `data/`; `autoreconf --force --install`; `./configure --prefix=/opt/libpinyin-tkrzw --disable-static --with-dbm=Tkrzw`; `make -j$(nproc) && make install` (one retry after `rm -f data/bigram.db`); checks installed `table.conf phrase_index.bin pinyin_index.bin bigram.db`; `LD_LIBRARY_PATH=$prefix/lib`; `cargo run --locked -q -p oxpinyin-datagen --no-default-features --features tkrzw -- compile --backend tkrzw --model-dir fixtures/datagen-toned --out-dir $work/export-tkrzw`; asserts `backend=tkt` in both `datagen-manifest.txt`s; exports `PINYIN_NGSEG`, `PINYIN_NGSEG_DATA`, `PINYIN_EXPORT_DIR`, `PINYIN_MODEL_DIR`, `PINYIN_GEN_NGRAM_DATA`, `PINYIN_SPSEG`, `PINYIN_GEN_KMM`, `PINYIN_EXPORT_KMM`, `PINYIN_MERGE_KMM`, `PINYIN_VALIDATE_KMM`, `PINYIN_KMM_TO_INTERP`; runs `cargo test --locked -q -p oxpinyin-training --no-default-features --features tkrzw --test segment-differential -- --include-ignored rust_matches_live_ngseg`, `cargo test … -p oxpinyin-kmm --test differential -- --include-ignored rust_kmm_matches_pin_{gen_and_export,to_interpolation,merge,validate}`, `… --test segment-spseg-mergeseq -- --include-ignored rust_matches_live_spseg`, and `tools/datagen/libpinyin-drop-in-differential.sh tkrzw fixtures/datagen-toned $work/drop-in` (hard-codes `/opt/libpinyin-tkrzw`) |
| Outputs | `== title ==` / `FAILED: title` per suite; `RESULT: at least one differential suite FAILED` exit 1; `fixture-tier differentials: OK` |
| Platform | Linux container (autotools, glib, `LD_LIBRARY_PATH`, `/opt` install) |
| Consumers | Nightly only |
| State | `/tmp/oxpinyin-fixture-differentials` (ephemeral in CI); `/opt/libpinyin-tkrzw`; rust-cache |
| Overlap | `tools/oracle/run-differentials.sh` (developer, model20) and `tools/oracle/build-oracle.sh` (canonical oracle build) — this script re-implements a minimal autotools build with the pin constants read from build-oracle.sh ("never duplicated here") |
| Constraints | "The install prefix is fixed at /opt/libpinyin-tkrzw, not configurable". Excluded suites listed by mechanism (KMM prune SIGSEGV, mergeseq token outside w3, lambda/counter model20-space goldens, eval). This is the only PR-adjacent/nightly job that compiles the pin; it provisions **no model data** |

### CI-21 — .github/workflows/verify-nightly.yml (job: overflow-lane)

| Field | Value |
|---|---|
| Entry point | job `overflow-lane`; `container: debian:testing`; no `timeout-minutes`; job env `RUSTFLAGS: -C overflow-checks=y -C debug-assertions=y -D warnings` |
| Owning subsystem | ci |
| Purpose | The "NDEBUG question" as CI: release-optimized codepath with overflow checks and debug assertions forced on (tooling-evaluation §8) |
| Implementation | apt + `libkyotocabinet-dev libglib2.0-dev libc6-dev`; bootstrap `safe-directory rustup rust-cache` |
| Inputs | `cargo test --locked --workspace --release --no-default-features --features kyotocabinet` |
| Outputs | Exit code |
| Platform | Linux container |
| Consumers | Nightly only |
| State | rust-cache (RUSTFLAGS hashed into key) |
| Overlap | nextest/coverage share the kyotocabinet choice |
| Constraints | Enforcement matrix §C "Latent overflow in optimized builds surfaces — SCHED" |

### CI-22 — .github/workflows/verify-nightly.yml (job: coverage)

| Field | Value |
|---|---|
| Entry point | job `coverage`: `uses: ./.github/workflows/coverage.yml` (workflow_call) |
| Owning subsystem | ci |
| Purpose | Measures the whole-tree coverage number nightly regardless of PR paths |
| Implementation | Reusable workflow call; see CI-10 |
| Inputs | none passed |
| Outputs | CI-10's artifact and floor |
| Platform | per CI-10 |
| Consumers | Nightly |
| State | per CI-10 |
| Overlap | CI-10 |
| Constraints | "The floor and its history live in that file" |

### CI-23 — .github/workflows/verify-nightly.yml (job: public-api)

| Field | Value |
|---|---|
| Entry point | job `public-api`; `container: debian:testing`; no timeout |
| Owning subsystem | ci |
| Purpose | Public-API snapshot ratchet for the supported Rust surface (oxpinyin-engine); any diff fails |
| Implementation | apt essentials + `gcc g++ make libc6-dev pkg-config libssl-dev` (cargo-public-api links openssl-sys); bootstrap `safe-directory`; inline rustup `nightly-2026-08-01`; `cargo install cargo-public-api --version 0.52.0 --locked` |
| Inputs | `cargo +nightly-2026-08-01 public-api -p oxpinyin-engine --simplified > public-api.txt`; `diff -u docs/api/oxpinyin-engine.public-api.txt public-api.txt` |
| Outputs | diff exit code |
| Platform | Linux container (nightly rustdoc JSON) |
| Consumers | Nightly; goldens-and-pins.md §1 row "the engine's public-API snapshot" (regeneration command = this exact pair) |
| State | none |
| Overlap | None |
| Constraints | "A deliberate surface change regenerates the file in the same PR"; the engine "has no features and no store dependency, so no C library is needed" |

### CI-24 — .github/workflows/verify-nightly.yml (job: nextest)

| Field | Value |
|---|---|
| Entry point | job `nextest`; `container: debian:testing`; no timeout |
| Owning subsystem | ci |
| Purpose | Proves nextest equivalence on the schedule so flipping the PR unit lane is a one-liner |
| Implementation | apt + `libkyotocabinet-dev libglib2.0-dev libc6-dev`; bootstrap `safe-directory rustup rust-cache`; cargo-nextest **0.9.143** curl'd (musl), `sha256sum -c` vs `6d891c18…3389`, `~/.local/bin` |
| Inputs | `cargo nextest run --locked --workspace --no-default-features --features kyotocabinet` · `cargo test --locked --workspace --doc --no-default-features --features kyotocabinet` |
| Outputs | Exit codes |
| Platform | Linux container |
| Consumers | Nightly |
| State | rust-cache |
| Overlap | ci.yml `test` (cargo test, bdb) |
| Constraints | "doctests stay on cargo test (nextest does not run them)" |

### CI-25 — .github/workflows/verify-nightly.yml (job: perf-snapshot)

| Field | Value |
|---|---|
| Entry point | job `perf-snapshot`; `container: debian:testing`; `timeout-minutes: 60`; `fetch-depth: 0`; env `DEBIAN_FRONTEND` |
| Owning subsystem | perf |
| Purpose | NOT a gate: measures the tree once a night (size, instruction counts via valgrind, RSS), compares to the previous night, flags "a human should look"; only the schedule writes the series |
| Implementation | apt + `gcc g++ make pkg-config libclang-dev libtkrzw-dev liblzma-dev liblz4-dev libzstd-dev zlib1g-dev libglib2.0-dev libssl-dev binutils valgrind python3 util-linux`; bootstrap `safe-directory rustup` (no cache); `cargo install cargo-c --version 0.10.25 --locked`; `actions/cache/restore` + `actions/cache/save` split (`path: .perf-series`, key `perf-snapshot-${{ github.run_id }}`, restore-keys `perf-snapshot-`) |
| Inputs | `tools/perf-gate/snapshot.sh --out snapshot.json --backend tkrzw` (reads `PERF_CPU`, `PERF_GATE_CYCLES`, `PERF_GATE_CG_CYCLES`, `PERF_GATE_RSS_PROCESSES`; backends tkrzw→`tkt`, kyotocabinet→`kct` only; builds `tools/bisection/bisect.c -ldl`; `nm -D` on the shipped .so); `tools/perf-gate/series.py --snapshot snapshot.json --series-dir .perf-series --summary "$GITHUB_STEP_SUMMARY" [--append iff GITHUB_EVENT_NAME == schedule]` (exit 0 reported / 1 flagged / 2 malformed) |
| Outputs | step output `status`; save `if: always() && github.event_name == 'schedule'`; "Re-raise a flagged comparison" exits 1 when status==1 (`::warning::`); `actions/upload-artifact` `perf-snapshot-<run_id>` of `snapshot.json`, `retention-days: 90`, `if-no-files-found: ignore` |
| Platform | Linux container (valgrind, `-ldl`, cargo-c, `nm -D`) |
| Consumers | Nightly; docs/perf/ci-perf-size-gate-proposal-2026-09-09.md |
| State | `.perf-series` cache (latest-wins; "a miss is normal") + 90-day artifacts ("The cache is evictable; artifacts are not") |
| Overlap | `tools/perf-gate/series.test.sh` (30 cases, **not run by any workflow**); benches.md tiers are "none run by CI" — this lane is explicitly non-gate |
| Constraints | "Only the schedule writes the series"; "Wall clock is deliberately absent"; "A flag means 'a human should look', never 'this merge is bad'"; valgrind absence records `null` not zero. Note: benches.md says "`verify-nightly` has no perf lane" and ci-strategy's table has no perf-snapshot row — both predate/ignore this non-gate lane |

---

## Composite action, scripts, hooks, registries

### CI-26 — .github/actions/bootstrap/action.yml

| Field | Value |
|---|---|
| Entry point | `uses: ./.github/actions/bootstrap` with string inputs `safe-directory`, `rustup`, `rustup-components`, `rust-cache`, `rust-cache-key` (all default off/empty; "a bare call is a no-op"); must be called **after** the inline `actions/checkout` |
| Owning subsystem | ci |
| Purpose | Shared post-checkout cluster: `git config --global --add safe.directory "$GITHUB_WORKSPACE"` (root container over root-owned checkout); pinned rustup install; Swatinem/rust-cache restore/save |
| Implementation | Composite action, 85 lines, bash steps; `Swatinem/rust-cache@6323deb1…` (v2.9.2, "do not bump without re-verifying") |
| Inputs | `rust-toolchain.toml` (`channel` awk'd: `awk -F '"' '/^[[:space:]]*channel[[:space:]]*=/{print $2; exit}'`), env `RUSTUP_COMPONENTS` (space-split), `curl --fail --location --retry 3 --show-error https://sh.rustup.rs \| sh -s -- -y --profile minimal --default-toolchain "$channel" --component …`; `GITHUB_PATH += $HOME/.cargo/bin` |
| Outputs | none |
| Platform | Portable (runs on macOS/Windows jobs with only `rust-cache`) |
| Consumers | Every job in ci.yml (except changes/ci-aggregate), commit-trailers.yml, coverage.yml, store-backends.yml, verify-nightly.yml, release-packages.yml `package`. Not used by oracle-image.yml, `attach`, `store-backends-gate` |
| State | rust-cache post step "runs with the composite's completion" |
| Overlap | The same `curl https://sh.rustup.rs … --profile minimal --default-toolchain` line is duplicated inline in tkrzw-sanitizers, bdb-sanitizers, fuzz-soak and public-api (nightly date pins instead of the channel) |
| Constraints | "The actions/checkout step stays inline in every calling job. A local `./` action introduced by a PR cannot wrap the checkout that would make it visible" (verify-nightly.md §4 records the failed attempt) |

### CI-27 — .github/scripts/lint-commits.sh

| Field | Value |
|---|---|
| Entry point | `lint-commits.sh <base-sha> <head-sha>` (CI mode, non-merge commits in range) or `lint-commits.sh --hook <message-file>` (hook mode); executed |
| Owning subsystem | ci |
| Purpose | Single source of truth for commit-trailer rule R2 (Assisted-by house form) so CI and the hook can never disagree |
| Implementation | POSIX sh, 205 lines; git ≥ 2.32 (`%(trailers:only,unfold)`), awk, grep -E, sed, sort/uniq |
| Inputs | argv; `GITHUB_STEP_SUMMARY` (optional); regexes `ASSISTED_SHAPE_RE='^Assisted-by: [[:alnum:]][[:alnum:]._-]*:[[:alnum:]][[:alnum:].+_-]*$'`, `ASSISTED_PLACEHOLDER_RE='^Assisted-by: (AGENT\|AGENT_NAME):\|:(MODEL\|MODEL_VERSION)([[:space:]]\|$)\|\[\|\]'`; condition 2 = MODEL token has ≥1 ASCII letter; condition 4 = no duplicate identical lines |
| Outputs | Exit `0 = pass, 1 = lint failure, 2 = usage/environment error`; `::error::<sha> <subject>: R2 — …` lines; `::notice::` on empty range; markdown summary table (`commit \| subject \| R2`) to step summary or stdout |
| Platform | Portable POSIX sh + git |
| Consumers | `trailer-lint` (CI-08), `.githooks/commit-msg` (CI-29), `tests/lint-commits.test.sh` (CI-09), `.kiro/steering/agents-protocol.md` (quotes the regex) |
| State | none |
| Overlap | none |
| Constraints | "Conditions 1, 3, 4 are lifted verbatim from the repo's former `.githooks/commit-msg`"; "Semantics beyond shape remain human attestation"; only R2 exists ("no R3 was ever adopted", enforcement-matrix §J) |

### CI-28 — .github/scripts/lint-crate-cfg-placement.sh

| Field | Value |
|---|---|
| Entry point | `lint-crate-cfg-placement.sh` (lints every `git ls-files -- '*.rs'`) or `lint-crate-cfg-placement.sh <file>...`; executed |
| Owning subsystem | ci |
| Purpose | Rejects a crate-level `#![cfg(...)]` written ABOVE its `//!` docs (a false gate discards following crate attributes, so `missing_docs` fires on the excluded platform — invisible to every Linux job and the portable-only mac/win jobs) |
| Implementation | POSIX sh + awk + git, 115 lines; awk scan for column-0 `^#!\[cfg\(` followed by column-0 `^//!` |
| Inputs | argv paths or git index; `-h/--help` |
| Outputs | `0 = pass, 1 = violation, 2 = usage/environment error`; `<file>:<doc-line>: \`//!\` doc comment below the crate-level \`#![cfg(...)]\` on line N` to stderr; "crate-cfg-placement: N file(s) checked, no violations" |
| Platform | Portable |
| Consumers | ci.yml `lint` (after its harness `tests/lint-crate-cfg-placement.test.sh`); gated path in `changes` |
| State | none |
| Overlap | none |
| Constraints | `#![cfg_attr(...)]` deliberately not matched; a named path that is not a regular file is exit 2, never a silent skip ("the one answer a linter must never give"); the tracked listing tolerates missing files (mid-rebase) |

### CI-29 — .githooks/commit-msg

| Field | Value |
|---|---|
| Entry point | git `commit-msg` hook (`$1` = message file); activated per clone by `git config core.hooksPath .githooks` (CONTRIBUTING.md) |
| Owning subsystem | ci |
| Purpose | Local pre-flight of R2 so the hook and CI can never disagree |
| Implementation | POSIX sh, 6 lines: `exec "$(git rev-parse --show-toplevel)/.github/scripts/lint-commits.sh" --hook "$1"` |
| Inputs | message file |
| Outputs | lint-commits.sh hook-mode exit (0/1/2); `commit-msg: N trailer violation(s)` |
| Platform | Portable |
| Consumers | Developers who opt in; `tests/lint-commits.test.sh` references `$HOOK` |
| State | none |
| Overlap | CI-08 |
| Constraints | Opt-in only; "Not a security boundary; CI remains authoritative" applies (proposed-config-diffs.md §8 for the dropped pre-commit hook). The only hook in `.githooks/` (the fmt pre-commit hook was "dropped in review (PR-1g)") |

### CI-30 — clippy.toml, deny.toml, Cargo.toml `[workspace.lints]`, rust-toolchain.toml (quality-gate configuration)

| Field | Value |
|---|---|
| Entry point | Read implicitly by `cargo clippy`, `cargo deny`, every `cargo` invocation, and rustup |
| Owning subsystem | ci |
| Purpose | The lint/supply-chain/toolchain policy the jobs enforce |
| Implementation | `clippy.toml` (4 lines): `msrv = "1.97.1"` only. `deny.toml` (71 lines): `[graph] all-features = false`; `[advisories] version = 2, yanked = "deny", ignore = []`; `[bans] multiple-versions = "warn", highlight = "simplest-path", deny = [], skip = [], skip-tree = []`; `[licenses] version = 2, private = { ignore = true }, confidence-threshold = 0.93`, allow list (GPL-3.0-or-later, Apache-2.0, Apache-2.0 WITH LLVM-exception, MIT, MIT-0, BSD-2/3-Clause, ISC, Zlib, Unicode-3.0, Unicode-DFS-2016, CC0-1.0, MPL-2.0, BSL-1.0, OpenSSL, CDLA-Permissive-2.0), `[[licenses.exceptions]] allow = ["NCSA"], crate = "libfuzzer-sys"`; `[sources] unknown-registry = "deny", unknown-git = "deny", allow-registry = [crates.io], allow-git = []`. `Cargo.toml [workspace.lints.rust]`: `unsafe_code = "deny"`, `unexpected_cfgs = "deny"`, `missing_docs = "warn"`, `unused_must_use = "deny"`, `unsafe_op_in_unsafe_fn = "deny"`; `[workspace.lints.clippy]`: `all = { level = "deny", priority = -1 }`, `must_use_candidate = "warn"` (per-crate `undocumented_unsafe_blocks`, `missing_safety_doc`, `unwrap_used`/`expect_used`/`panic`/`panic_in_result_fn` denies live in crate roots per enforcement-matrix §A/§B). `rust-toolchain.toml`: `channel = "1.97.1"`, components clippy, rustfmt. `fuzz/Cargo.toml [lints.rust] unsafe_code = "deny"` (separate workspace, does not inherit) |
| Inputs | — |
| Outputs | — |
| Platform | Portable |
| Consumers | lint (clippy.toml), deny (deny.toml ×3 graphs), every cargo job (`-D warnings` promotes `missing_docs`/`must_use_candidate` warns to errors in CI), bootstrap (channel) |
| State | — |
| Overlap | No `rustfmt.toml` by decision ("stable defaults only", enforcement-matrix §I) |
| Constraints | deny.toml: "every ignore carries its justification and a review-by date inside the reason string" (schema has no expiry field). clippy.toml msrv must "never disagree with what CI enforces". `clippy.toml` and `deny.toml` are in the `changes` gate pathspec |

### CI-31 — .github/dependabot.yml

| Field | Value |
|---|---|
| Entry point | GitHub Dependabot config (declarative) |
| Owning subsystem | ci |
| Purpose | Weekly updates for the root cargo graph (`/`), the fuzz graph (`/fuzz`), and GitHub Actions (`/`); `phf*` grouped |
| Implementation | YAML, 22 lines |
| Inputs | — |
| Outputs | Dependabot PRs |
| Platform | — |
| Consumers | deny job relies on it ("--locked pins both root graphs to the committed Cargo.lock, which Dependabot maintains"); action SHA pins are bumped by it but comments say "do not bump without re-verifying" |
| State | — |
| Overlap | — |
| Constraints | AGENTS.md hard forbid "Add/upgrade deps without ask" is a process rule above this automation |

---

## Quality-gate tooling summary (where each gate's config and threshold live)

| Gate | Tool / version | Config / threshold | Where it runs | Pinning method |
|---|---|---|---|---|
| rustfmt | toolchain 1.97.1 | no `rustfmt.toml` (stable defaults) | ci.yml lint (root + fuzz workspace) | rust-toolchain.toml |
| Clippy | 1.97.1 | `clippy.toml msrv=1.97.1`; workspace `clippy::all = deny`; `RUSTFLAGS=-D warnings` | ci.yml lint (`--workspace --all-targets`) | toolchain |
| Lizard CCN ratchet | lizard 1.24.0 (pip) | **no config file**; `lizard crates/ -l rust -C 40`; max measured 38 | ci.yml lint (PR, since 2026-09-08) | `pip install --break-system-packages lizard==1.24.0` |
| crate-cfg placement | own sh script + harness | — | ci.yml lint | in-tree |
| pc-version | own sh | `oracle-pin.txt libpinyin_tag` vs capi manifests | ci.yml lint | in-tree |
| cargo-deny | 0.20.2 | `deny.toml` | ci.yml deny (3 graphs) | curl + sha256 |
| cargo-llvm-cov | 0.9.0 | `--fail-under-lines 77`; kyotocabinet feature | coverage.yml (PR path-filtered + nightly call) | curl + sha256 |
| cargo-nextest | 0.9.143 | kyotocabinet feature; doctests via cargo test | verify-nightly nextest | curl + sha256 |
| cargo-fuzz | 0.13.2 on nightly-2026-08-01 + rust-src | 10 targets × `-max_total_time=180 -timeout=15 -max_len=4096`; ASan+LSan default; `setarch -R` | verify-nightly fuzz-soak | `cargo install --locked` |
| cargo-public-api | 0.52.0 on nightly-2026-08-01 | diff vs `docs/api/oxpinyin-engine.public-api.txt` | verify-nightly public-api | `cargo install --locked` |
| cargo-c | 0.10.25 (nightly perf) / distro package (release) | — | perf-snapshot; release package | `cargo install --locked` / apt/dnf/pacman |
| Sanitizers | nightly-2026-09-07; gcc libasan/libubsan | tkrzw: ASan (`detect_leaks=0:abort_on_error=1`) + UBSan link arm; bdb: ASan Rust + ASan/UBSan C harnesses | store-backends | inline rustup |
| ABI export gate | `tools/abi/check-exports.sh` (nm -D) | `libpinyin.ver` 79 / `libzhuyin.ver` 52; dev and `--shipped` | ci.yml test | in-tree |
| Allocator pairing | `tools/abi/check-alloc-pairing.sh` (g++ ASan/LSan) | `libpinyin.alloc` / `libzhuyin.alloc`; negative control | ci.yml test | in-tree |
| Overflow lane | stable, `--release` | `RUSTFLAGS=-C overflow-checks=y -C debug-assertions=y -D warnings` | verify-nightly | — |
| w3 fixture identity | `sha256sum -c fixtures/w3/fixtures.sha256` | — | ci.yml test | — |
| Trailer lint | `lint-commits.sh` | R2 regexes | commit-trailers (required) + hook | in-tree |

Portable lanes and their exclusions (both jobs): `--exclude oxpinyin-capi
--exclude oxpinyin-zhuyin-capi` (glib + cargo-c, Linux-only C ABIs; pinyin-oracle's
`oracle-ffi` is kept off by `--no-default-features` on macOS, and its `glib-sys`
dependency is under `[target.'cfg(target_os = "linux")'.dependencies]`),
`--exclude oxpinyin-corpus --exclude oxpinyin-kmm --exclude oxpinyin-punct
--exclude oxpinyin-word` (backend-independent, "run once per PR in ci.yml's
full test job"). `oxpinyin-training` and `oxpinyin-dictool` stay in.

---

## Policy sentences a consolidation must preserve

Quoted verbatim (short) with file path and the nearest heading.

**PASS / SKIPPED / FAIL semantics**
- docs/runbooks/parity-gates.md (title "Live parity gates", body): "The runner protocol is exit 0 PASS, 77 SKIPPED, any other status FAIL. A missing unpatched oracle is SKIPPED; a required patched oracle is FAIL when absent. An off-pin or wrong-cell manifest is FAIL."
- same: "The aggregate prints one `runner | cell | status | seconds | pin_ref | reason` table and exits nonzero on any FAIL or unapproved SKIPPED." … "Allowances never turn a failure into success. Skipped rows remain in the table with their reason."
- same: "An absent patched prefix is FAIL, never an allowable skip."
- same: "A deadline kills the whole process group and produces FAIL, including when a child ignores termination."
- same: "The aggregate changes no CI policy; direct `run-live-typing-diff.sh` keeps its legacy missing-oracle exit behavior for CI."
- tools/bisection/run-live-typing-diff.sh (header): "Exit codes: 0 = identical or skipped; 1 = build/run failure; 2 = divergence."
- tools/perf-gate/series.py (docstring "Exit codes"): "0 reported … 1 a deterministic metric moved more than the attention threshold with the environment unchanged … 2 the snapshot is malformed".
- .github/scripts/lint-commits.sh / lint-crate-cfg-placement.sh: "Exit codes: 0 = pass, 1 = lint failure, 2 = usage/environment error."

**Tests must fail rather than skip on missing inputs**
- docs/testing/README.md "## Tests that need inputs CI never has": "A test that needs the model20 cache, the system-table export, a pin-built libpinyin tool or data dir, or opencc is `#[ignore = "needs …; run with --include-ignored"]`, and once run it panics on a missing input instead of printing "skipping" and passing" … "A committed golden or fixture that is missing is a failure too." … "Do not add a new self-skipping test".
- AGENTS.md "## Operational rules that live with their procedures": "are `#[ignore = "needs …"]` and fail on a missing input, never skip … Do not add a self-skipping test."
- docs/runbooks/goldens-and-pins.md "## 1. Committed fixtures and goldens": "A missing golden is a test failure, never a skip (#373)."
- .github/scripts/lint-crate-cfg-placement.sh (comment): "Skipping a typo silently would report "no violations" for a file nobody read, which is the one answer a linter must never give."
- tools/bdb/run-sanitizers.sh: "ERROR: %s is not installed; sanitizer gate did NOT run." (exit 1). Counter-example to keep visible: tools/kc/run-sanitizers.sh has `OXPINYIN_SANITIZER_ALLOW_SKIP` → "SKIP … the gate did NOT run" exit 0.

**Which cells run when**
- AGENTS.md "## Verification cells": "1. The reference cell is Berkeley DB (`PINYIN_ORACLE_DBM=bdb`, the default). Baseline, differential, revert-and-check and the gate run there and nowhere else by default." "2. Run all three cells (bdb, kc, tkrzw) only when the diff touches backend code. Decide by path check, not by judgement" (the three checks: `crates/oxpinyin-store/` in `git diff --name-only origin/main...HEAD`; a changed file containing `feature = "bdb|tkrzw|kyotocabinet"`; a Cargo.toml diff line naming a backend). "State the check's output in the PR body." "3. Every differential runs on all three cells once at the start of each sweep and once before a release." "4. Revert-and-check runs on the reference cell only."
- docs/runbooks/parity-gates.md: "Use Berkeley DB for a lane's ordinary before/after verification. Select all three cells for the start-of-sweep and pre-release differential run."
- AGENTS.md "## Verification cells" 5: "The local test gate is `cargo test -p <crate>` for each crate the diff touches, plus fmt and clippy on those crates. The workspace sweep is CI's job; do not run it locally as a gate." 6: "No new bespoke driver plus runner per issue where an existing driver can take a case file." 7: "Re-verification is proportional to what changed since the last green gate" (docs-only: fmt then push; review fix: fmt, clippy, `cargo test -p`, only that issue's differential on the reference cell; conflict-free rebase touching no lane file: no local re-gate; stack: re-gate only the changed commit and those above touching the same files). "The report states which of these cases applied and why".

**Same-run measurement**
- AGENTS.md "## Measurement and lane state": "**Measure in one run.** A measuring PR runs base, PR and pin in one job on one machine and reports only those numbers and their ratios. No absolute figure from another run, host or CPU is ever a baseline".
- docs/runbooks/benches.md "### Measure base, PR and pin in one run": "**A measurement is the ratio of artifacts timed in the same job on the same machine, never a number carried over from another run.**" … "**interleaving** the base, PR and pin artifacts run by run" … "Earlier snapshots in this repository remain records of what was measured where; they are not baselines to subtract from."
- docs/runbooks/benches.md (intro): "Four tiers, none run by CI (`verify-nightly` has no perf lane)" … "Numbers are only comparable within one container on one host".
- AGENTS.md "## Rebase discipline": "Re-run pins after any rebase that changes the engine, capi, or data crates. Whoever merges later re-measures those pins rather than assuming the pre-rebase numbers still hold."
- verify-nightly.yml perf-snapshot comment: "Record freely, threshold almost nothing. A flag means "a human should look", never "this merge is bad"." … "Only the schedule writes the series."

**Golden refresh procedure**
- docs/runbooks/goldens-and-pins.md (intro): "All three are STOP-class under AGENTS.md: an ask, a measured differential, and a maintainer ruling recorded in the findings."
- "## 1. Committed fixtures and goldens": "Regenerate, diff, and commit the golden in the same change as the code that moved it, with the before/after numbers in the message." Public API row: "A diff is a supported-surface change, so it is an ask; commit the regenerated file in the same change with the additions and removals named in the message".
- "## 2. The frozen parity numbers": steps "1. Run the parity suite against the oracle … 2. Classify the delta under `compatibility-policy.md` … 3. Ask. The re-freeze is the maintainer's ruling … 4. Update the asserted numbers in the same commit as the ruling."
- ci.yml test comment (w3 manifest): "running that in CI would overwrite the very drift this must catch, so this is a check".

**Pin moves**
- docs/runbooks/goldens-and-pins.md "## 3. The oracle pin": "`tools/oracle/oracle-pin.txt` plus the constants in `tools/oracle/build-oracle.sh` (libpinyin commit, ibus-libpinyin commit, model SHA-256, DBM) are one identity; change every field together." Steps 1–4 (bump + rebuild, "the build verifies the fetched commit by `git rev-parse`"; re-run W2 surface etc.; record in a verification finding; "the bump edits both capi `Cargo.toml` files in the same commit; then run `tools/packaging/check-pc-metadata.sh`").
- tools/oracle/oracle-pin.txt (header): "When a pin component changes, update it here and in its owner file in the same commit — a stale prefix must be rebuilt from scratch." v4: "A pin component, the base image, the apt snapshot or the recipe changing changes them: update them in the same commit."
- docs/runbooks/cloud.md "## Failure modes": "`MISMATCH` and exit 3: the libraries built here do not hash to `oracle-pin.txt`. … Do not edit the recorded values to match."
- oracle-image.yml (push step): "The tag names its recipe, so a tag that already exists was built from this recipe: keep it, never overwrite."

**What CI never does**
- store-backends.yml (header): "The pin-built oracle and the model20 archive are deliberately NOT CI concerns: provisioning them … is a LOCAL workflow, and no CI job may download model20.text.tar.gz or anything like it."
- docs/runbooks/oracle.md (intro): "Linux only. The model20 archive is not redistributable …: no CI job fetches it, and nothing below that needs it runs on GitHub-hosted runners. The one hosted job that touches the oracle, `.github/workflows/oracle-image.yml`, builds the libraries only and ships no model-derived data".
- ci.yml lint comment: "The full installed-tree diff, tools/packaging/check-pc-metadata.sh, needs the pin-built libpinyin, which CI never has."
- ci.yml test comment: "the model20 archive is non-redistributable and never enters CI, so this and every model20-dependent check … run on a provisioned local machine, never on GitHub-hosted runners."
- tools/model/fetch-model.sh (header): "This is a LOCAL-ONLY workflow: no CI job downloads the archive".
- oracle-image.yml (header): "The image carries NO model20-derived data … and no job here fetches it."
- docs/runbooks/parity-gates.md: "`tools/bisection/run-all.sh` … does not provision or patch the oracle, cache oracle output, run a workspace test sweep, or execute performance recipes." … "Provision separate prefixes; never patch the original installations." … "Do not modify the reference installation to satisfy a runner."

**Debug vs release for differentials; target directory retention**
- AGENTS.md "## Verification cells" 8: "Differentials build debug, never release. Keep one target directory per cell and reuse it; do not delete target directories between issues. Clean once, at the end of the lane."
- docs/runbooks/parity-gates.md: "`oracle-cell.sh` owns backend selection and target resolution. It exports `CARGO_PROFILE_DEV_OPT_LEVEL=1`; no Cargo manifest or shipped source is changed." … "Each cell has a separate retained Cargo target under `--target-root` (default `target/cells`). … Keep these targets between issues. No runner should rebuild a supplied library".
- docs/findings/tooling-lane-gates.md (intro): "Parity artifacts use the dev profile with `CARGO_PROFILE_DEV_OPT_LEVEL=1`, one retained target per cell."

**Frozen SPEC / golden / CI-policy edit rules**
- AGENTS.md "## Hard forbids": "Add/upgrade deps without ask · edit frozen SPECs/goldens/CI policy without ask · `unsafe` outside allowlisted crates · silence lints."
- AGENTS.md "## STOP → do not improvise": "implementation would require changing a frozen SPEC without an ask." "A STOP clears **only on a human turn**."
- .kiro/steering/agents-protocol.md: "Implement from frozen SPECs/fixtures only."
- docs/safety/enforcement-matrix.md "## J. Process & meta rules": "Freeze discipline (SPECs, scorer API, path-set parity) | already enforced by review + oracle differentials; linting "don't change frozen semantics" is not mechanizable".
- Ratchets: coverage.yml comment "Raise it when the number rises, never lower it to pass."; docs/safety/ci-strategy.md "## Tier 3" Failure policy: "they do not auto-block unless a ratchet exists (coverage/Lizard/complexity ratchets only ever tighten)."
- Images/pins: store-backends.yml header "Do NOT pin this image by digest or let a later change "stabilise" it." (repeated in every container job: "deliberately unpinned"); store-backends.yml tkrzw-sanitizers: "bump the date deliberately, in its own commit"; commit-trailers.yml/bootstrap: "(do not bump without re-verifying)".
- ci.yml `changes` comment: "The pathspec list is the one the header documents; keep the two in lockstep."
- AGENTS.md "## Attribution" / commit-trailers: "`Assisted-by: AGENT:MODEL` shape with nothing after the model … no placeholder text; no duplicate lines (set semantics)."

---

## Platform matrix as implemented

| Lane (workflow/job) | OS / image | Backend | Builds / tests | Gate? |
|---|---|---|---|---|
| ci / lint | debian:testing container | bdb (link deps only) | fmt ×2, cfg-placement, pc-version, clippy workspace all-targets, Lizard | required via ci-aggregate |
| ci / test | debian:testing | **bdb** (default) | `cargo test --workspace` (all 25 crates), C++ smoke, ABI exports (dev + shipped), alloc pairing (ASan/LSan), live-typing diff (SKIPs) | required |
| ci / test-macos | macos-latest | **tkrzw** (Homebrew 1.x) | workspace minus capi, zhuyin-capi, corpus, kmm, punct, word | required |
| ci / test-windows | windows-latest | **bdb 4.8** (vcpkg; unsurveyed-version override) | same exclusions | required |
| ci / deny | debian:testing | n/a | cargo-deny: default, --all-features, fuzz | required |
| commit-trailers / trailer-lint, trailer-test | debian:testing | n/a | lint-commits.sh; its harness | required |
| coverage / coverage | debian:testing | **kyotocabinet** | llvm-cov workspace, 77% floor | PR path-filtered + nightly |
| store-backends / store-features ×3 | debian:testing | kc / tkrzw / bdb | workspace minus corpus, kmm, punct, word | non-required aggregate |
| store-backends / backend-matrix | debian:testing | all combos (`cargo check`) | exactly-one-backend | non-required |
| store-backends / tkrzw-sanitizers ×2 | debian:testing, seccomp unconfined, nightly-2026-09-07 | tkrzw | `-p oxpinyin-store` under ASan / UBSan-link | non-required |
| store-backends / bdb-sanitizers | debian:testing, seccomp unconfined, nightly-2026-09-07 | bdb | store crate ASan + C harnesses ASan/UBSan | non-required |
| verify-nightly / fuzz-soak | debian:testing, seccomp unconfined, nightly-2026-08-01 | bdb (default via path deps) | 10 fuzz targets × 180 s | nightly, must be green |
| verify-nightly / differentials | debian:testing | **tkrzw** (pin built `--with-dbm=Tkrzw`; Rust `--features tkrzw`) | builds pinned libpinyin; fixture-tier differentials | nightly |
| verify-nightly / overflow-lane | debian:testing | kyotocabinet | `cargo test --release` + overflow/debug-assert flags | nightly |
| verify-nightly / coverage | (coverage.yml) | kyotocabinet | as above | nightly |
| verify-nightly / public-api | debian:testing, nightly-2026-08-01 | none (engine only) | cargo-public-api diff | nightly |
| verify-nightly / nextest | debian:testing | kyotocabinet | nextest workspace + doctests | nightly |
| verify-nightly / perf-snapshot | debian:testing | **tkrzw** (`fixtures/w3/tkt`) | cargo-c shipped build, valgrind, series compare | nightly, non-gate (exit 1 on flag) |
| oracle-image / oracle-image | ubuntu-latest + Docker (`linux/amd64`) | bdb, kc, tkrzw (pin libs) | Dockerfile.oracle build, hash verify, GHCR push | path-triggered |
| release-packages / package ×3 | debian:testing / fedora:latest / archlinux:latest | tkrzw / kc / kc | cargo cinstall via distro cargo-c; deb/rpm/pkg; install + init on system model | release only |
| release-packages / attach | ubuntu-latest | n/a | SHA256SUMS, gh release upload | release only |

Backend coverage per crate set: the four backend-independent crates
(corpus, kmm, punct, word) are tested only in ci/test (bdb). The two C-ABI
crates are tested only on Linux (ci/test bdb; store-features kc/tkrzw/bdb;
overflow/nextest/coverage kc). No lane tests tkrzw on Linux with the full
workspace except store-features (minus the four crates).

### Linux-only places and why

- `.github/workflows/ci.yml` lint/test/deny: `container:` (containers run on Linux runners only); apt; `libdb-dev`/`libglib2.0-dev`; `tools/abi/check-exports.sh` (`nm -D --defined-only` arm; Darwin `nm -gU` supported for local runs only; other hosts exit 2); `tools/abi/check-alloc-pairing.sh` ("Linux only: LeakSanitizer's ptrace-based stop-the-world does not run under macOS's sandbox, and the C ABIs are Linux-first"); `tools/bisection/run-cpp-smoke.sh` (g++ against `libpinyin_capi.so`, glib); `tools/bisection/run-live-typing-diff.sh` (`gcc … -ldl`, dlopen of `.so` in `live-typing-diff.c`; `PINYIN_ORACLE_PREFIX`).
- The two C-ABI crates: `oxpinyin-capi` / `oxpinyin-zhuyin-capi` need `libglib2.0-dev` + `g++` + cargo-c ("Linux-first", backends.md); `pinyin-oracle`'s `glib-sys` is `[target.'cfg(target_os = "linux")'.dependencies]` and its `oracle-ffi` feature is kept off on macOS. `fuzz/Cargo.toml`: capi-commands "Linux-only, like the nightly fuzz soak — as is capi".
- Sanitizer lanes and fuzz-soak: `setarch "$(uname -m)" -R` (util-linux) + `--security-opt seccomp=unconfined` for `personality(2)`; gcc libasan/libubsan; `-Zsanitizer=address` with explicit `--target` host triple.
- `verify-nightly/differentials`: autotools build of libpinyin, `LD_LIBRARY_PATH`, install to `/opt/libpinyin-tkrzw`, `nproc`, `::warning::` retry semantics; `tools/datagen/libpinyin-drop-in-differential.sh` hard-codes `/opt/libpinyin-{kc,tkrzw}` (perf-matrix container convention).
- `verify-nightly/perf-snapshot`: `valgrind` (callgrind Ir), `binutils` (`nm -D`), `gcc … bisect.c -ldl`, cargo-c shipped `.so`.
- `oracle-image`: Docker buildx, `linux/amd64` only ("native shared objects differ across architectures"); `tools/oracle/verify-oracle-image.sh` needs docker or podman; `tools/oracle/image-tag.sh` needs `sha256sum`.
- `release-packages`: `dpkg-deb`, `dpkg --print-architecture`, `dpkg -S`, `ldconfig -p` (release-deb.sh); `rpmbuild` (release-rpm.sh); `makepkg`/`fakeroot`/build-user creation (release-arch.sh, "makepkg refuses to run as root"); `pacman -Sw/-Sp`; `gcc -print-multiarch`; `ldconfig`; `cc` + pkg-config smoke.
- Developer-side tools referenced by runbooks (not CI): `db5.3_dump`, `kchashmgr`, `tkrzw_dbm_util` for the requested-row training gates (parity-gates.md: "installing a backend development library alone does not provide those command-line tools"); `/proc/cpuinfo|meminfo|loadavg` in `run-tree-recipe-control.sh`; `/proc/sys/kernel/perf_event_paranoid` / samply / callgrind in `run-w8-cycle.sh`; `uname -s != Linux` refusal in `tools/packaging/relink-versioned.sh`; docker/podman in `tools/cloud/setup.sh` (also starts `dockerd`, trusts a proxy CA under `/root/.ccr`).
- Bash-isms: every workflow `run:` is bash (`defaults.run.shell: bash` set explicitly on container jobs and on windows; `mapfile`, arrays, `$EPOCHSECONDS`, `[[ ]]`, `BASH_REMATCH`, `<<<`, `read -r … < <(…)`); only `lint-commits.sh`, `lint-crate-cfg-placement.sh`, their harnesses and the hook are POSIX sh.

---

## Count

31 records: CI-01, CI-02, CI-03, CI-04, CI-05, CI-06, CI-07, CI-08, CI-09,
CI-10, CI-11, CI-12, CI-13, CI-14, CI-15, CI-16, CI-17, CI-18, CI-19, CI-20,
CI-21, CI-22, CI-23, CI-24, CI-25, CI-26, CI-27, CI-28, CI-29, CI-30, CI-31.

## Architecture notes

Observations (what the files do) unless marked *interpretation*.

- **Duplicated bootstrap step.** The identical inline `apt-get update -qq && apt-get install -y --no-install-recommends bash tar gzip zstd git ca-certificates curl` block appears in 15 jobs (ci lint/test/deny; trailer-lint/trailer-test; coverage; store-features/backend-matrix/tkrzw-sanitizers/bdb-sanitizers; fuzz-soak/differentials/overflow-lane/public-api/nextest/perf-snapshot). It cannot move into the composite action because checkout needs git first (bootstrap/action.yml description; verify-nightly.md §4). The `debian:testing # deliberately unpinned` comment is repeated at each of those `container:` lines.
- **Duplicated rustup line.** `curl --fail --location --retry 3 --show-error https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain …` exists in bootstrap/action.yml (channel from rust-toolchain.toml) and inline in tkrzw-sanitizers, bdb-sanitizers (nightly-2026-09-07), fuzz-soak (+`--component rust-src`), public-api (nightly-2026-08-01). The bootstrap action has no "toolchain override" input, so nightly jobs bypass it. Two different nightly dates are live: 2026-08-01 (fuzz, public-api, `tools/kc/run-sanitizers.sh` default) and 2026-09-07 (store sanitizers, `tools/bdb/run-sanitizers.sh` default).
- **Duplicated "pinned binary by curl + sha256sum -c" recipe.** cargo-deny 0.20.2 (ci.yml), cargo-llvm-cov 0.9.0 (coverage.yml), cargo-nextest 0.9.143 (verify-nightly) use the same 8-line pattern (`ver=`, `sum=`, `curl -sSfL`, `sha256sum -c`, `tar xzf`, `install -m755 … ~/.local/bin`, `GITHUB_PATH`). cargo-fuzz 0.13.2, cargo-public-api 0.52.0 and cargo-c 0.10.25 use `cargo install --version … --locked` instead (no checksum). *Interpretation:* two install conventions for the same purpose.
- **Duplicated apt dependency lists.** The backend dev-package line (`gcc g++ make pkg-config libclang-dev libkyotocabinet-dev libtkrzw-dev libdb-dev liblzma-dev liblz4-dev libzstd-dev zlib1g-dev libglib2.0-dev`) appears in store-features and backend-matrix; subsets appear in ci lint/test (bdb), coverage/overflow/nextest (kc), differentials/perf-snapshot (tkrzw). `libc6-dev` is named explicitly in coverage/fuzz-soak/overflow/public-api/nextest but not in ci lint/test or store-backends (verify-nightly.md §9 explains why it is deliberate where present).
- **Aggregate-job pattern ×2.** `ci-aggregate` and `store-backends-gate` both bind `needs.*.result` through env and `exit 1` on non-success; only ci-aggregate has the path-gate "skipped is fine iff GATED=false" clause. Only ci-aggregate is a required check.
- **Latest-wins cache pattern ×2.** fuzz-soak (`actions/cache`, key `fuzz-corpus-<run_id>`, restore `fuzz-corpus-`) and perf-snapshot (`actions/cache/restore` + `/save`, `perf-snapshot-<run_id>`, save only on `schedule`). Same `@55cc8345…` v6.1.0 pin.
- **`setarch -R` + seccomp unconfined ×3** (tkrzw-sanitizers address arm, bdb-sanitizers job options, fuzz-soak). The bdb sanitizer *script* does not use setarch; the job only unconfines seccomp.
- **Step-summary report pattern.** store-features, backend-matrix, tkrzw-sanitizers, bdb-sanitizers all have an `if: always()` Report step writing a markdown table with `elapsed` via `$EPOCHSECONDS`; the sanitizer ones read `steps.<id>.outcome` while the others read `job.status`.
- **Parsers of `tools/oracle/oracle-pin.txt`** (`sed -n 's/^KEY=//p'`): oracle-image.yml meta step (`libpinyin_commit`, `base_image`, `lib{pinyin,zhuyin}_sha256_<dbm>`), `tools/oracle/image-tag.sh` (`libpinyin_commit`, `base_image`, `apt_snapshot`), `tools/oracle/verify-oracle-image.sh` (`lib*_sha256_<dbm>`), `tools/packaging/pc-version.sh` (`libpinyin_tag`; consumed by check-pc-version.sh, release-stage/deb/rpm/arch.sh and the release install gate), `tools/cloud/setup.sh` (`pinval` for sha256 and `data_manifest_sha256_<dbm>`), and `crates/oxpinyin-capi/build_pin_version.rs` (per pc-version.sh header). The runbook says the record "mirrors" constants in `build-oracle.sh`.
- **Parsers of `tools/oracle/build-oracle.sh` constants** (`LIBPINYIN_VERSION`, `LIBPINYIN_SHA`, `LIBPINYIN_GIT_URL`, `MODEL_SHA256`): `tools/bisection/oracle-cell.sh` (builds `EXPECTED_PIN_REF=pin_ref=libpinyin-<ver>-<sha>+model20-<sha>+dbm-<cell>`), `tools/oracle/nightly-fixture-differentials.sh`. So the pin identity is held in two files with different readers; goldens-and-pins.md §3 says "change every field together".
- **Parsers of `datagen-manifest.txt`:** nightly-fixture-differentials.sh (`grep -q "^backend=tkt$"`) and the drop-in differential path. **`table.conf`** is only existence-checked (release install gate, nightly differentials, Arch data extraction). **`pin_ref`** is produced by oracle-cell.sh and checked by `run-all.py` ("the manifest must match it before any oracle gate executes") — no workflow reads it.
- **Hard-coded backend names / extensions.** Feature names `kyotocabinet|tkrzw|bdb`: ci.yml (tkrzw macOS; bdb vcpkg env), store-backends matrix + `backend-matrix.sh` combos, coverage/overflow/nextest (`kyotocabinet`), differentials/perf-snapshot (`tkrzw`), release matrix (`tkrzw`, `kyotocabinet` + `dbfmt` `Tkrzw|KyotoCabinet`), `oracle-cell.sh` (`bdb|kc|tkrzw` → `BerkeleyDB|KyotoCabinet|Tkrzw` → `bdb|kyotocabinet|tkrzw` → `db|kct|tkt`), `run-cpp-smoke.sh` (`kct|tkt|db`, prefers `db`), `snapshot.sh` (`tkrzw→tkt`, `kyotocabinet→kct`, no bdb), `libpinyin-drop-in-differential.sh` (`kc|tkrzw` only, no bdb), `verify-oracle-image.sh`/`image-tag.sh`/`oracle-image.yml` (`bdb kc tkrzw`). *Interpretation:* at least four distinct spellings of the same three peers (feature, cell short name, DBM display name, file extension), mapped in oracle-cell.sh but re-derived by hand elsewhere.
- **Every place that spawns cargo.** Workflows: lint (fmt ×2, clippy), test (`cargo test --workspace`), test-macos/test-windows, deny (`cargo deny` ×3 → `cargo metadata`), coverage (`cargo llvm-cov` ×2), store-features (`cargo test`), tkrzw-sanitizers (`cargo test --target`), fuzz-soak (`cargo install`, `cargo fuzz list/run`), overflow-lane, public-api (`cargo install`, `cargo public-api`), nextest (`cargo nextest run`, `cargo test --doc`), perf-snapshot (`cargo install cargo-c`, then snapshot.sh). Scripts: `oracle-cell.sh::oracle_cell_artifact` (`cargo build --locked -p <pkg> --no-default-features --features $CAPI_FEATURE`, skipped when `OXPINYIN_CAPI_SO`/`OXPINYIN_ZHUYIN_SO` are supplied), `backend-matrix.sh` (`cargo check --locked -p oxpinyin-store` ×8), `tools/bdb/run-sanitizers.sh` (`cargo +nightly test/build`), `nightly-fixture-differentials.sh` (`cargo run -p oxpinyin-datagen`, `cargo test … --include-ignored` ×6), `release-stage.sh` → `install.sh` (`cargo cinstall`), `snapshot.sh` (cargo-c build), `check-exports.sh`/`check-alloc-pairing.sh`/`run-cpp-smoke.sh`/`run-live-typing-diff.sh` (via oracle_cell_artifact).
- **Exit-code conventions in play.** `0/1/2` (pass/lint-fail/usage) for the two lint scripts and the hook; `0 (incl. SKIP)/1/2 (divergence)` for run-live-typing-diff.sh; `0/77/other` for registry runners (run-all.py maps `rc==77 → SKIPPED`, deadline → FAIL); `0/1/2` for series.py (re-raised as job failure only on 1); `exit 3` for `oracle-cell.sh` bad `PINYIN_ORACLE_DBM` and for `tools/cloud/setup.sh` hash MISMATCH; `exit 2` for closed-set `case` defaults in workflows (unknown matrix value, unknown distro). `::error::`/`::warning::`/`::notice::` annotations are emitted by workflows and by lint-commits.sh.
- **Process control / timeouts / locks.** `timeout-minutes`: 30 (lint, test, trailer-*, backend-matrix), 45 (test-macos, test-windows, store-features, both sanitizers), 60 (differentials, perf-snapshot), 90 (oracle-image, package); **none** on changes, deny, ci-aggregate, coverage, fuzz-soak, overflow-lane, public-api, nextest, attach, store-backends-gate (GitHub default 360). Concurrency groups: `coverage-<workflow>-<ref>` (cancel), `store-backends-<ref>` (cancel), `oracle-image-<ref>` (no cancel), `release-packages-<release id>` (no cancel), `verify-nightly` (no cancel); ci.yml and commit-trailers.yml have none. Matrix `fail-fast: false` (store-features, package), `true` (tkrzw-sanitizers). fuzz: `-timeout=15` per input, `-max_total_time=180` per target. No file locks in workflows; `run-all.py` registry locks and process-group kills exist only in the local aggregate (parity-gates.md).
- **Frozen / "do not change" markers.** `debian:testing` unpinned ("do NOT pin … or let a later change 'stabilise' it"); action SHAs "do not bump without re-verifying" (checkout v7.0.1, rust-cache v2.9.2); coverage floor "never lower it to pass"; Lizard `-C 40` "never allowed to grow past the limit"; nightly dates "bump the date deliberately, in its own commit"; published oracle tag "never overwrite"; `verify-nightly.yml` "There are no continue-on-error lanes"; oracle-pin.txt "update it here and in its owner file in the same commit"; `OXPINYIN_BDB_ALLOW_UNSURVEYED_VERSION` "No other lane sets it"; w3 manifest check "not regenerate"; ci.yml pathspec "keep the two in lockstep".
- **Self-skip inventory (observation).** Workflow-run scripts that exit 0 without doing their work: `run-live-typing-diff.sh` (always in CI — oracle absent) and `tools/kc/run-sanitizers.sh` (`OXPINYIN_SANITIZER_ALLOW_SKIP`, not run by any job). `run-cpp-smoke.sh`, `check-exports.sh`, `check-alloc-pairing.sh`, `backend-matrix.sh`, `tools/bdb/run-sanitizers.sh`, `nightly-fixture-differentials.sh` have no skip path.
- **Doc/workflow drift noticed while reading (observation, for the consolidator):** (a) store-backends.yml path-gate comment says the sanitizer jobs run `tools/kc/run-sanitizers.sh`; no job does. (b) verify-nightly.yml header's lane list omits `perf-snapshot`; benches.md says "verify-nightly has no perf lane"; ci-strategy.md's matrix has no perf row. (c) docs/runbooks/backends.md opens with "tkrzw is the workspace default" while its own table and every workflow say bdb (since 2026-09-20). (d) ci-strategy.md "Jobs as of 2026-09-12" still lists a `fuzz` job and names `segment` in the coverage path list (workflow has `oxpinyin-training`). (e) tooling-evaluation.md §12 says the Lizard ratchet is "never a PR blocker"; it has been in the PR `lint` job since 2026-09-08 (enforcement-matrix §E agrees with the workflow). (f) verify-nightly.md §6 describes a ci.yml fuzz lane that was removed 2026-09-23 (historical record). (g) enforcement-matrix §G lists "Coverage floor | yes (nightly)" though coverage.yml is also a PR path-filtered gate. (h) backends.md "The exactly-one-backend gate" says "each of the four explicit ones"; the script checks three explicit peers plus the default.
- **Oracle identity verification, by job.** differentials: commit SHA (`git rev-parse FETCH_HEAD` vs `LIBPINYIN_SHA`), no library hash. oracle-image: `.so` sha256 per cell vs `oracle-pin.txt` + absence-of-model-data `find`; the tag itself encodes commit/base digest/apt snapshot/recipe hash. No PR-path job verifies or needs any oracle identity; `tools/cloud/setup.sh` (not CI) checks `.so` hashes and the per-cell `data_manifest_sha256_<dbm>`.


---

# RS — Rust test infrastructure inventory

Tree read: detached worktree at origin/main (`e67f19d1`), never modified. Every
file below was read in full. Line counts are `wc -l` of the tree.

Conventions used in this file:

- "oracle" = the pin-built libpinyin prefix produced by
  `tools/oracle/build-oracle.sh`, verified via `oracle-pin.txt`.
- "portable tier" = compiled and run with the `oracle-ffi` feature off, on
  Linux/macOS/Windows CI (`ci.yml` `test`, `test-macos`, `test-windows`).
- "FAIL on missing input" = the `#[ignore]` policy of `docs/testing/README.md`:
  an ignored test, once run with `--include-ignored`, panics when its input is
  absent; it never prints "skipping" and passes.
- Exit codes for binaries: Rust `ExitCode::SUCCESS`=0, `FAILURE`=1,
  `ExitCode::from(2)`=2 (usage / feature-off / missing tables). No binary uses
  77 (SKIP); the skip convention lives in the shell runners, not here.

---

## pinyin-oracle crate (`crates/pinyin-oracle`)

### RS-01 — crates/pinyin-oracle/Cargo.toml

| Field | Value |
|---|---|
| Entry point | Cargo package `pinyin-oracle` (name deliberately kept: "the frozen differential harness earns one legacy prefix. Do not 'fix' this to oxpinyin"). Lib + 9 `[[bin]]` + 3 `[[bench]]` (`harness = false`). |
| Owning subsystem | pinyin-oracle |
| Purpose | Dev-only differential harness against the pin-built libpinyin; `publish = false`, never ships. |
| Implementation | TOML, 123 lines. Deps: `oxpinyin-core`, `oxpinyin-data` (default-features=false, backend forwarded), `oxpinyin-engine`, `oxpinyin-testsupport`; dev-deps `criterion 0.8`, `dhat 0.3`, `oxpinyin-user` (default-features=false); Linux-only optional `glib-sys` (workspace) behind `oracle-ffi`. |
| Inputs | Features: `default = ["bdb"]`; `oracle-ffi = ["dep:glib-sys"]`; `kyotocabinet`/`bdb`/`tkrzw` forwarded to `oxpinyin-data/*`. `build = "build.rs"`. |
| Outputs | Lib `pinyin_oracle`; bins `parity-corpus`, `parity-diff`, `oracle-paths`, `oracle-candidates`, `oracle-candidate-structure`, `parity-sweep`, `parity-worst`, `corpus-tail`, `sentence-tail` (note: `oracle_sentence_surface` has NO `[[bin]]` entry — auto-discovered from `src/bin/`, so its cargo name is the underscore form `oracle_sentence_surface`); benches `scan_perf`, `alloc_profile`, `dbm_bench` (`required-features = ["oracle-ffi"]`). |
| Platform | Portable with `oracle-ffi` off; Linux-first with it on (glib-sys dep is `[target.'cfg(target_os = "linux")']`). |
| Consumers | Workspace member (`Cargo.toml:14`); `ci.yml` `cargo test --locked --workspace` (feature off); macOS lane runs `--no-default-features --features tkrzw` explicitly to keep `oracle-ffi` off; `tools/bisection/delta_ledger.py` / `render_ledger.py` build `-p pinyin-oracle --test sentence_surface_parity`; `.kiro/steering/structure.md:18` (unsafe allow, Linux, never ships). |
| State | none |
| Overlap | `oxpinyin-testsupport/Cargo.toml` has the same backend-forwarding `[features]` block (kyotocabinet/bdb/tkrzw → `oxpinyin-store/*`), with `default = []` instead of `["bdb"]`. |
| Constraints | Lints: `unsafe_code = "allow"` (only crate besides capi/store with FFI), `undocumented_unsafe_blocks = "deny"`, `missing_safety_doc = "deny"`, `clippy::all = deny`. Comment: the package name, "wire/pin schemas and the default oracle prefix are behavior, not names". `dbm_bench` must keep `required-features` so plain `cargo bench -p pinyin-oracle` on oracle-less hosts skips rather than fails to link. |

### RS-02 — crates/pinyin-oracle/build.rs

| Field | Value |
|---|---|
| Entry point | Cargo build script; runs on every build, does nothing unless `CARGO_FEATURE_ORACLE_FFI` is set. |
| Owning subsystem | pinyin-oracle (oracle provisioning / link discovery) |
| Purpose | Locates a pin-verified oracle prefix and emits link flags for `libpinyin` + `glib-2.0`, refusing any prefix whose `oracle-pin.txt` `pin_ref=` is off-pin. |
| Implementation | Rust, 191 lines; `include!("src/pin_ref.rs")` so the link-time check shares `EXPECTED_PIN_REF`/`bench_pin_ref` with the runtime. No pkg-config crate, no dlopen: it reads `PKG_CONFIG_PATH` *as a path list* and walks `…/lib{,64}/pkgconfig` up two levels to the prefix root. |
| Inputs | Env: `CARGO_FEATURE_ORACLE_FFI`, `PINYIN_ORACLE_PREFIX`, `PKG_CONFIG_PATH`, `HOME`, `PINYIN_BENCH_DBM`, `CI`. Files: `<prefix>/oracle-pin.txt` (only `pin_ref=` line is parsed here), `<prefix>/lib{,64}/libpinyin.so*`. |
| Outputs | `cargo::rerun-if-env-changed={PINYIN_ORACLE_PREFIX,PINYIN_BENCH_DBM,PKG_CONFIG_PATH}`, `rerun-if-changed=<prefix>/oracle-pin.txt`, `rustc-link-search=native=<libdir>`, `rustc-link-lib=dylib=pinyin`, `rustc-link-lib=dylib=glib-2.0`, `rustc-link-arg=-Wl,-rpath,<libdir>` (so no `LD_LIBRARY_PATH` needed). On failure: `eprintln!` of the build-oracle.sh recipe and `exit(1)`. Asserts (panics) if `CI` and `PINYIN_BENCH_DBM` are both set, or `PINYIN_BENCH_DBM` ∉ {kc,bdb}. |
| Platform | Linux-only when active (links `libpinyin.so`, `glib-2.0`). |
| Consumers | Every `--features oracle-ffi` build: `live_smoke`, `differential_live`, `export_seam`, `real_tables_integration`, `candidate_structure` freshness, `dbm_bench`, the `parity-diff`/`oracle-candidates`/`oracle-candidate-structure`/`oracle_sentence_surface` bins. |
| State | none (reads env + prefix files). |
| Overlap | Search order and `verify` are duplicated in `pin.rs` (`OraclePrefix::locate`, `PinManifest::verify_pin_ref_and_dbm`) — by design, "so link time and run time agree". `DEFAULT_PREFIX_SUFFIX = ".local/opt/pinyin-oracle"` is defined here AND in `lib.rs` ("mirrored from the crate root"). `tools/capture/run-capture.sh` parses the same `oracle-pin.txt` in bash. |
| Constraints | "CI must never set `PINYIN_BENCH_DBM`: parity and capture always link the tkrzw oracle under the full frozen-ref check" (enforced by assert). Bench relaxation is an exact-match swap of the `+dbm-` suffix, never a containment check. |

### RS-03 — crates/pinyin-oracle/src/lib.rs

| Field | Value |
|---|---|
| Entry point | Crate root of `pinyin_oracle`. |
| Owning subsystem | pinyin-oracle |
| Purpose | Declares the module layering (pure/always-compiled vs feature-gated `ffi`+`live`) and the public re-exports. |
| Implementation | Rust, 94 lines. Crate-level lints `deny(clippy::unwrap_used, expect_used, panic, panic_in_result_fn)` for non-test builds (constitution §4 mechanised); `warn(missing_docs)`. |
| Inputs | `cfg!(feature = "oracle-ffi")`. |
| Outputs | `pub mod` capture, corpus, differential, flags, model_cache, observation, pin, sentence_tail, taxonomy; private `error`, `pin_ref`; `#[cfg(feature="oracle-ffi")] mod ffi; pub mod live`. Re-exports: `OracleError`, `OracleFlags`, model_cache items (`EXPECTED_MODEL_FILES, MODEL_CACHE_ENV, MODEL_DIR_ENV, MODEL20_SHA256, MODEL20_URL, ModelDirError, default_extracted_dir, locate_model_dir, model_dir_is_complete, verified_model_dir`), observation items (`CandidateInfo, MAX_CAPTURED_CANDIDATES, OracleCandidateType, OracleCompleteness, OracleObservation, OracleSegment`), pin items (`EXPECTED_PIN_REF, OraclePrefix, PinManifest, VerifiedPin, bench_pin_ref`), taxonomy (`BudgetVerdict, DivergenceClass, Taxonomy`), and with ffi `live::{ExportedPhrase, LiveSource, Oracle, Session}`. Consts `DEFAULT_PREFIX_SUFFIX`, `PREFIX_ENV = "PINYIN_ORACLE_PREFIX"`; `const fn ffi_available()`. |
| Platform | portable |
| Consumers | all bins/tests/benches of the crate; `oxpinyin-capi` must NOT depend on it (comment in model_cache.rs). |
| State | none |
| Overlap | `DEFAULT_PREFIX_SUFFIX` duplicated in build.rs. |
| Constraints | Doc comment: "Links the pin-built libpinyin at the P0-0 pin; meaningless off-pin. Fresh state + learning off is the parity protocol." |

### RS-04 — crates/pinyin-oracle/src/ffi.rs

| Field | Value |
|---|---|
| Entry point | `#[cfg(feature = "oracle-ffi")] mod ffi` (private; items `pub(crate)`). |
| Owning subsystem | pinyin-oracle |
| Purpose | Hand-written `unsafe extern "C"` declarations for the subset of the pinned public `pinyin.h` the harness drives, plus six opaque handle types. |
| Implementation | Rust, 242 lines. Two `unsafe extern "C"` blocks (23 + 6 = **29 libpinyin symbols**), `opaque_handle!` macro (zero-size `#[repr(C)]` struct with `PhantomData<(*mut u8, PhantomPinned)>` → !Send/!Sync/!Unpin). GLib reached via `glib_sys::{g_free, GArray, g_array_free, g_array_new}` re-exports. `type PinyinOption = u32`. |
| Inputs | none at runtime; linked against the prefix chosen by build.rs. |
| Outputs | Symbols (29): `pinyin_init, pinyin_fini, pinyin_set_options, pinyin_alloc_instance, pinyin_free_instance, pinyin_reset, pinyin_parse_more_full_pinyins, pinyin_get_parsed_input_length, pinyin_guess_sentence, pinyin_get_sentence, pinyin_guess_candidates, pinyin_get_n_candidate, pinyin_get_candidate, pinyin_get_candidate_string, pinyin_get_candidate_type, pinyin_get_candidate_nbest_index, pinyin_get_pinyin_key, pinyin_get_pinyin_key_rest, pinyin_get_pinyin_key_rest_positions, pinyin_get_pinyin_string, pinyin_get_pinyin_is_incomplete, pinyin_train, pinyin_save` (block 1) and `pinyin_begin_get_phrases, pinyin_iterator_has_next_phrase, pinyin_iterator_get_next_phrase, pinyin_end_get_phrases, pinyin_lookup_tokens, pinyin_token_get_phrase` (block 2, "export/token functions … frozen in docs/findings/data-layer-export.md"). Opaque types (6): `PinyinContext, PinyinInstance, LookupCandidate, ChewingKey, ChewingKeyRest, ExportIterator`. |
| Platform | Linux (glib-sys only on linux target). |
| Consumers | `live.rs` only. |
| State | none |
| Overlap | `oxpinyin-capi/src/fuzz_api` + `fuzz/fuzz_targets/capi_commands.rs` declare the *same-named* symbols against oxpinyin's own capi (different library). `tools/bisection` C drivers declare these in C. |
| Constraints | Header doc: frozen in `docs/testing/oracle-ffi-seam.md` against `include/libpinyin-2.11.92/pinyin.h` SHA-256 `e1138482…312d47`. "Every function below returns C `bool` … **not** GLib's `gboolean` … Do not 'simplify' these signatures." `pinyin_get_candidate_type` out-param is `c_int`, decoded by checked `OracleCandidateType::from_raw`, never transmuted. Adding a symbol means updating the seam finding. |

### RS-05 — crates/pinyin-oracle/src/live.rs

| Field | Value |
|---|---|
| Entry point | `#[cfg(feature = "oracle-ffi")] pub mod live`. |
| Owning subsystem | pinyin-oracle |
| Purpose | Safe wrapper over the FFI: `Oracle` (context over a verified prefix) and `Session` (instance), producing `OracleObservation`s under the frozen capture protocol (fresh user dir, learning off, DYNAMIC_ADJUST rejected, candidates capped at 10 with uncapped total). |
| Implementation | Rust, 1160 lines. `// SAFETY:` on every unsafe block. Public items: `Oracle` (`open`, `open_with_user_dir`, `open_with_temp_user_dir`, `prefix`, `pin_ref`, `save_user_data`, `user_dir`, `observe`, `session`, `export_phrases`), `Session<'oracle>` (`reset`, `observe`, `train_top`, `observe_candidate_infos`, `observe_sentence_surface`, `lookup_tokens`, `token_text`), `SentenceSurface`, `ExportedPhrase`, `LiveSource<'oracle>` (impl `ObservationSource`). Private: `FreshUserDir` (removed on drop), `UserDir` enum, `ORACLE_LOCK`, `path_to_cstring`, `collect_candidates`, `collect_segments`, `parse_input`, `get_sentence`. |
| Inputs | `OraclePrefix` (proof of verification), `OracleFlags`, input bytes; `std::env::temp_dir()` for `open_with_temp_user_dir`. No env vars read directly. |
| Outputs | `Result<OracleObservation, OracleError>`, `Vec<CandidateInfo>`, `SentenceSurface`, `Vec<ExportedPhrase>`, `Vec<u32>` tokens. |
| Platform | Linux (feature-gated). |
| Consumers | bins `parity-diff`, `oracle-candidates`, `oracle-candidate-structure`, `oracle_sentence_surface`; tests `live_smoke`, `differential_live`, `export_seam`, `real_tables_integration`, `candidate_structure`; bench `dbm_bench`. Runbook `oracle.md` §4 mandates `Session::train_top` over raw `pinyin_train`. |
| State | **Process-wide `static ORACLE_LOCK: Mutex<()>`** held for an `Oracle`'s whole lifetime ("the pinned libpinyin does not tolerate two live contexts … `pinyin.cpp:_check_offset: Assertion 'zero_key != key' failed`"); poison ignored. Fresh user dir `<parent>/pinyin-oracle-user-<pid>-<counter>` (static `AtomicU64` counter), `remove_dir_all` before create and on drop. `user_data_dirty: Arc<AtomicBool>` shared with sessions so `pinyin_save` returning `false` on an unmodified store (`pinyin.cpp:1136`) is a no-op not a failure. Drop order: `pinyin_fini` then user-dir removal. |
| Overlap | Segment walk mirrors `tools/capture/capture.c`; abort guard duplicates the knowledge in `docs/testing/oracle-apostrophe-abort.md`; `train_top` precondition mirrors `tools/bisection` train drivers. |
| Constraints | Abort guards (documented, not from upstream source): (1) parsed_len>0 with no lowercase letter → `NoKeyColumns` sentinel instead of key walk; (2) `pinyin_get_candidate_nbest_index` called only for `NbestMatch` (accessor asserts/aborts otherwise); (3) `pinyin_get_sentence(i)` read only for indices a surviving NBEST row proves, clamped to `max_index`. `train_top` requires full parse + successful `guess_sentence` (`pinyin.cpp:2676`). Determinism test `session_reset_agrees_with_a_fresh_instance` pins reset ≡ fresh instance. |

### RS-06 — crates/pinyin-oracle/src/capture.rs

| Field | Value |
|---|---|
| Entry point | `pub mod capture` (always compiled). |
| Owning subsystem | pinyin-oracle / capture |
| Purpose | Reader (and segment writer) for the `pinyin-capture-v1` line format so frozen fixtures replay as `OracleObservation`s in portable CI. |
| Implementation | Rust, 504 lines (incl. 9 unit tests). Pure, no I/O. Items: `CAPTURE_SCHEMA = "pinyin-capture-v1"`, `escape`, `unescape`, `CaptureRecord` (`parse_line`, `field`, `to_observation`), `read_fixture`, `segment_to_wire`, `segments_to_wire`; private `parse_segments`, `parse_segment`, `parse_candidates`. |
| Inputs | Fixture text (TAB-separated `key=value`, one record per LF line). Required fields: `schema, flags (0x-hex), input, parsed_input_length, pin_ref, parse_return, segments, candidate_total, candidates_hex, remainder`; optional `family, case`. |
| Outputs | `Vec<OracleObservation>`; errors `CaptureFieldMissing/Malformed`, `CaptureRecordInvalid{line}`, `DynamicAdjustRejected` (via `OracleFlags::new`). Schema tag mismatch is an error; **pin_ref is NOT validated here** ("an off-pin record must reach the comparison so it can be logged as a divergence"). |
| Platform | portable |
| Consumers | `differential.rs` (escape, segments_to_wire), `oracle-paths` bin (escape/unescape), tests `capture_replay`, `differential_replay`, `taxonomy_replay`, `decode_differential`, `graph_paths` (unescape). |
| State | none |
| Overlap | Escaping rule is a stricter sibling of `tools/capture/capture.c` (bytes ≥0x80 escaped here, raw there; reader tolerates both). Segment notation `text@begin:end:complete|partial` is re-parsed by hand in `tests/graph_paths.rs::parse_path` and `tests/decode_differential.rs::corpus` rather than through this module. `CaptureRecord::parse_line` ≈ `oracle_paths.rs::fields` ≈ `decode_differential.rs::field` ≈ `testsupport::fixture::record_fields` (four hand-rolled TAB `key=value` splitters). |
| Constraints | Format frozen in `docs/testing/capture-fixtures.md`. Sentinel spellings `<missing-rest>@n`, `<missing-position>@n`, `<missing-pinyin>@n`, `<no-key-columns>@n`; `-` = no segments / no candidates. `candidates_hex` is comma-separated lowercase hex of UTF-8. Malformed escapes preserved literally, never fatal. |

### RS-07 — crates/pinyin-oracle/src/observation.rs

| Field | Value |
|---|---|
| Entry point | `pub mod observation` (always compiled). |
| Owning subsystem | pinyin-oracle |
| Purpose | The value both producers (live FFI, replayed fixture) yield for one input: `OracleObservation` and its parts. |
| Implementation | Rust, 407 lines (incl. 5 unit tests). Items: `OracleCompleteness {Complete, Partial}` (+`as_wire`), `OracleCandidateType` (8 variants = `lookup_candidate_type_t` 1..=8; `from_raw`, `as_wire`, `from_wire`), `CandidateInfo {text, candidate_type, nbest_index: Option<u8>}`, `OracleSegment {Syllable{syllable,begin:u16,end:u16,completeness}, MissingKeyRest{offset}, MissingPosition{offset}, MissingPinyinString{offset}, NoKeyColumns{parsed_len}}` (+`is_sentinel`, `syllable`, `range`), `OracleObservation {pin_ref, family, case, input: Vec<u8>, flags: OracleFlags, parse_return, parsed_input_length, segments, candidate_total: u32, candidates: Vec<String>, remainder}` (+`fully_consumed`, `has_sentinel_segment`, `selected_path`, `check_self_consistent`), `MAX_CAPTURED_CANDIDATES = 10`. |
| Inputs | none |
| Outputs | types only |
| Platform | portable |
| Consumers | capture, differential, live, taxonomy, all tests. |
| State | none |
| Overlap | Wire names of candidate types (`NBEST_MATCH_CANDIDATE` …) duplicated as single letters `n/N/a/?` in `oracle_sentence_surface.rs::render_row`, `real_tables_integration.rs`, and port-side `sentence_tail.rs::render_row`. |
| Constraints | "mirror the `pinyin-capture-v1` record fields frozen in capture-fixtures.md". Candidate type discriminants frozen against pinned `pinyin.h`; no `begin/end/tokens` on `CandidateInfo` because the public header exposes none (`candidate-construction.md` §1.6). Sentinels are the F-E-01 (#566) shape, not padding. |

### RS-08 — crates/pinyin-oracle/src/differential.rs

| Field | Value |
|---|---|
| Entry point | `pub mod differential` (always compiled). |
| Owning subsystem | pinyin-oracle |
| Purpose | Parse-level comparison of one `OracleObservation` against `oxpinyin_core::FullPinyinParser`'s path set, the `ObservationSource` seam, the two log schemas and the runner. |
| Implementation | Rust, 911 lines (incl. 14 unit tests). Items: consts `DIFF_SCHEMA="pinyin-diff-v1"`, `DIVERGENCE_SCHEMA="pinyin-divergence-v1"`, `MAX_LOGGED_PATHS=8`, `UNCLASSIFIED="unclassified"`; `Source {Live, Replay}`; `Reason` (8, ordered by precedence: OffPin, OracleInconsistent, PositionOverflow, OracleSentinel, OursError, ConsumedLength, Remainder, PathAbsent); `Outcome {Agreement, Divergence(Reason)}`; `OurParse {Paths(Vec<ParseResult>), Failed(ParseError)}`; trait `Classifier`; `Unclassified`; `Comparison`; `compare()`; trait `ObservationSource`; `ReplaySource`; `RunReport` (+`to_summary`); `comparison_line`, `divergence_line`; `Case`; `RunOutput`; `run()`. Private `decide` (the frozen precedence), `our_path`, `oracle_path`, `render_our_path`. |
| Inputs | `OracleObservation`; uses `oxpinyin_core::{Completeness, FullPinyinParser, InputParser, ParseError, ParseResult}`; `EXPECTED_PIN_REF`. |
| Outputs | `pinyin-diff-v1` line (14 fields, frozen order: schema, pin_ref, source, corpus, case, input, flags, outcome, class, rank, ours_paths, ours_consumed, theirs_consumed, candidate_total) and `pinyin-divergence-v1` line (17 fields incl. theirs_path, theirs_remainder, ours_shown, ours_path_set (≤8 paths `;`-joined), ours_remainder). Summary text (not a schema). |
| Platform | portable |
| Consumers | `live.rs` (LiveSource impl), `taxonomy.rs`, bins `parity-diff`, `oracle-paths` (reads the TSVs it writes), tests `differential_live`, `differential_replay`, `taxonomy_replay`. |
| State | none; runner is sequential so logs are byte-identical between runs. |
| Overlap | `render_our_path` duplicated verbatim in `oracle_paths.rs::render`. Comparison primitives: (a) `pin_ref == EXPECTED_PIN_REF`, (b) `check_self_consistent` (parsed ≤ input len), (c) input len fits u16, (d) no sentinel segment, (e) our parser returned Ok, (f) consumed length equal, (g) remainder bytes equal, (h) oracle path ∈ our ordered path set (rank = position). |
| Constraints | "Schemas, the agreement predicate and the reason precedence are frozen in `docs/findings/differential-log.md`." Candidate lists recorded, never asserted at this level. `UNCLASSIFIED` placeholder retained for `Unclassified` classifier (tests assert it never appears once `Taxonomy` is wired). |

### RS-09 — crates/pinyin-oracle/src/corpus.rs

| Field | Value |
|---|---|
| Entry point | `pub mod corpus` (always compiled). |
| Owning subsystem | pinyin-oracle / fixtures |
| Purpose | Deterministic generation of the 10,465-input W2 parity corpus from `oxpinyin_core::FULL_PINYIN_SYLLABLES` and a fixed seed. |
| Implementation | Rust, 562 lines (incl. 10 unit tests). Inline `SplitMix64` (`SEED = 0x5061_7269_7479_3031` "Parity01", rejection sampling in `below`). Items: `SEED`, `CORPUS_DIR = "tests/parity/corpus/inputs"`, `MAX_APOSTROPHE_GROUPS = 8`, `DOMAIN_LOW = 0x21`, `DOMAIN_HIGH = 0x7e`, `Stratum {file_name, description, inputs}` (+`to_file_bytes`, `parse_file_bytes`), `generate()`. Private: `junk_alphabet`, `partial_prefixes`, `ambiguous_syllables` (derived by running the parser), `within_result_bound`, `accept`, `fill` (attempt budget `count*64+1024`), `join_syllables`, `junk_input`, `edge_cases`. |
| Inputs | none at runtime (pure); depends on `oxpinyin_core::{FULL_PINYIN_SYLLABLES, FullPinyinParser, InputParser}`. |
| Outputs | 9 strata: 01-single-syllable (405, exhaustive), 02-syllable-pairs (2000), 03-short-utterances (2500), 04-long-utterances (1500), 05-apostrophe (1200), 06-partial-tails (1200), 07-ambiguity (800), 08-junk (800), 09-edge (60, fixed list, empty input first). |
| Platform | portable |
| Consumers | bins `parity-corpus`, `parity-diff`, `oracle-candidates`, `oracle-candidate-structure`, `oracle_sentence_surface`, `corpus-tail`, `parity-sweep`, `parity-worst`; tests `parity_corpus`, `differential_live`, `real_tables_integration`, `candidate_structure`; `benches/support` (`CORPUS_DIR`, `parse_file_bytes`). |
| State | none |
| Overlap | Every bin that needs corpus inputs re-reads the committed files via `generate()`+`parse_file_bytes` (parity_sweep, parity_worst, corpus_tail, oracle_sentence_surface, real_tables_integration, benches/support::load_w2_inputs) — six copies of the same load loop. |
| Constraints | Methodology frozen in `docs/testing/parity-corpus.md`; the committed corpus must be byte-identical (test RS-30). Space 0x20 excluded from the domain so lines carry no edge whitespace. Corpus holds inputs only — "a committed expectation would be a second, unverified oracle". |

### RS-10 — crates/pinyin-oracle/src/taxonomy.rs

| Field | Value |
|---|---|
| Entry point | `pub mod taxonomy` (always compiled). |
| Owning subsystem | pinyin-oracle |
| Purpose | Maps a `Comparison` (outcome + reason + rank + flags + pin_ref) to one of eight `DivergenceClass`es and computes the 0.5% auto-accept budget verdict. |
| Implementation | Rust, 584 lines (incl. 15 unit tests). Items: `AUTO_ACCEPT_BUDGET_DENOMINATOR = 200`, `DivergenceClass` (OutputIdentical, TieSwap, PathSet, FlagSemantics, OursBug, TheirsBug, DataVersion, DistroDelta; `ALL`, `as_wire`, `is_auto_accepted`, `is_divergence`, `gates_parity`, `Display`), `Taxonomy {parity_profile: OracleFlags}` (`new`, `parity_profile`, `class_of`; impl `Classifier`), `BudgetVerdict` (`auto_accepted`, `literal_pass`, `divergence_only_pass`, `to_summary`), `budget()`. Private `classify_off_pin` (splits `code+model+dbm` on `+`). |
| Inputs | `Comparison`, `EXPECTED_PIN_REF`, `OracleFlags::DEFAULT`. |
| Outputs | class wire tokens `output-identical, tie-swap, path-set, flag-semantics, ours-bug, theirs-bug, data-version, distro-delta`. |
| Platform | portable |
| Consumers | bins `parity-diff`; tests `differential_live`, `taxonomy_replay`. |
| State | none |
| Overlap | Pin-ref shape `libpinyin-<tag>-<sha>+model20-<digest>+dbm-<backend>` is parsed here by splitting on `+`, and in `pin_ref.rs::bench_pin_ref` by `strip_suffix("+dbm-tkrzw")`. |
| Constraints | Rules per `docs/findings/divergence-taxonomy.md` (frozen 2026-08-09): agreement rank0→output-identical, rank>0→tie-swap; off-pin→data-version iff only the model component differs else distro-delta; sentinel/inconsistent/overflow→theirs-bug; ours-error→ours-bug; consumed/remainder/path-absent→path-set iff `flags == parity_profile` else flag-semantics. Only `path-set` and `ours-bug` gate. Budget "reported, not gating until a decoder exists (W4)" — both readings computed; maintainer decision: divergence-only reading is the meaning. |

### RS-11 — crates/pinyin-oracle/src/pin.rs

| Field | Value |
|---|---|
| Entry point | `pub mod pin` (always compiled). |
| Owning subsystem | pinyin-oracle / oracle provisioning |
| Purpose | Runtime gate: parse `oracle-pin.txt`, verify it is the frozen pin, locate a prefix from the environment. |
| Implementation | Rust, 473 lines (incl. 8 unit tests). Items: re-exports `EXPECTED_PIN_REF, MANIFEST_FILE_NAME, bench_pin_ref`; consts `DATA_MANIFEST_FILE_NAME="oracle-data.sha256"`, `DATA_UNSTABLE_MANIFEST_FILE_NAME="oracle-data-unstable.sha256"`, `MANIFEST_SCHEMA="pinyin-oracle-v1"`, `EXPECTED_DBM="Tkrzw"`, `EXPECTED_LIBPINYIN_TAG="2.11.92"`, `DATA_SUBDIR="lib/libpinyin/data"`; `PinManifest` (`parse`, `read_from_prefix`, `field`, `verify`); `VerifiedPin` (getters `pin_ref, libpinyin_commit, model_sha256, header_sha256, shared_object_sha256, data_manifest_sha256, data_unstable_manifest_sha256: Option`); `OraclePrefix` (`open`, `root`, `data_dir`, `pin`, `locate`). |
| Inputs | Env: `PINYIN_BENCH_DBM` (in `verify`), `PINYIN_ORACLE_PREFIX` (via `crate::PREFIX_ENV`), `PKG_CONFIG_PATH`, `HOME`. Files: `<prefix>/oracle-pin.txt` (`key=value` lines; blank ignored; no-`=` lines ignored; repeated key fatal), `<prefix>/oracle-data.sha256` (existence only), `<prefix>/lib/libpinyin/data/` (is_dir). |
| Outputs | `Result<_, OracleError>`; `verify` requires `schema`, `pin_ref`, `dbm`, `libpinyin_tag` to match and `libpinyin_commit, model_sha256, header_sha256, shared_object_sha256, data_manifest_sha256` to be present (values not re-hashed — "re-hashing the payload is the build recipe's and run-capture.sh's job"). |
| Platform | portable (pure file reads). |
| Consumers | `live.rs`, all live bins/tests, `dbm_bench` (`OraclePrefix::open` from `PINYIN_ORACLE_PREFIX` only). |
| State | none |
| Overlap | Duplicate of `build.rs::locate_prefix`/`verify` (intentional). `oracle-pin.txt` is also parsed by `tools/capture/run-capture.sh` (bash `manifest_value`) and by `tools/bisection` scripts / `cloud/setup.sh`. |
| Constraints | Field order preserved, repeats rejected "so a manifest cannot smuggle a second value". `data_unstable_manifest_sha256` is informational and optional (prefixes built before the manifest split lack it). Bench relaxation mirrors build.rs exactly. |

### RS-12 — crates/pinyin-oracle/src/pin_ref.rs

| Field | Value |
|---|---|
| Entry point | `mod pin_ref` (private) AND `include!`d textually by build.rs. |
| Owning subsystem | pinyin-oracle / oracle provisioning |
| Purpose | Single source of truth for the frozen pin reference string and its bench variants. |
| Implementation | Rust, 36 lines; no `use`, no crate items (so it can be `include!`d). `EXPECTED_PIN_REF = "libpinyin-2.11.92-074a2219c90feaf962d0d24f034514033ece5f99+model20-59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155+dbm-tkrzw"`, `MANIFEST_FILE_NAME = "oracle-pin.txt"`, `bench_pin_ref(dbm) -> Option<(String, &'static str)>` (`"kc"`→`+dbm-kc`,`"KyotoCabinet"`; `"bdb"`→`+dbm-bdb`,`"BerkeleyDB"`; else None). |
| Inputs | none |
| Outputs | constants |
| Platform | portable |
| Consumers | build.rs, pin.rs, taxonomy.rs, differential.rs, every fixture header check (`# pin_ref=` in w4 goldens, `pin_ref=` in foundation records). |
| State | none |
| Overlap | Same literal lives in `tools/capture/run-capture.sh` (`EXPECTED_PIN_REF`, per the doc comment), `fixtures/w3/pin-ref.txt`, `fixtures/w4/oracle-*.txt` headers, `fixtures/foundation/*.txt` records, `docs/testing/oracle-environment.md`. `MODEL20_SHA256` in testsupport is the model component of this string, stored separately. |
| Constraints | "Keep it free of `use` statements". A pin change edits this file plus every fixture that embeds the ref. |

### RS-13 — crates/pinyin-oracle/src/flags.rs

| Field | Value |
|---|---|
| Entry point | `pub mod flags`. |
| Owning subsystem | pinyin-oracle |
| Purpose | Option-word bits and the `OracleFlags` newtype that proves `DYNAMIC_ADJUST` is clear. |
| Implementation | Rust, 157 lines (5 unit tests). Consts `IS_PINYIN=1<<1, IS_ZHUYIN=1<<2, PINYIN_INCOMPLETE=1<<3, ZHUYIN_INCOMPLETE=1<<4, USE_TONE=1<<5, FORCE_TONE=1<<6, USE_DIVIDED_TABLE=1<<7, USE_RESPLIT_TABLE=1<<8, DYNAMIC_ADJUST=1<<9, PINYIN_AMB_ALL=0x3ff<<10, SORT_BY_PHRASE_LENGTH_AND_PINYIN_LENGTH_AND_FREQUENCY=0x1e, CAPTURE_DEFAULT=0x18a, CAPTURE_BASELINE=IS_PINYIN`; `OracleFlags(u32)` with `DEFAULT`, `BASELINE`, `const fn new` (Err on DYNAMIC_ADJUST), `bits`, `contains`, `Display` as `{:#010x}`. |
| Inputs | none |
| Outputs | types/consts |
| Platform | portable |
| Consumers | capture (parses `flags=0x…`), live (`pinyin_set_options`, sort word), taxonomy (parity profile), observation, all tests. |
| State | none |
| Overlap | `oxpinyin_core::OptionBits` and `PINYIN_INCOMPLETE`/`PINYIN_CORRECT_*` consts are the engine-side twins (used in `corpus_tail.rs`, `sentence_surface_parity.rs` with literal `0x18a` and `0x1e`). |
| Constraints | Values frozen in `docs/testing/oracle-ffi-seam.md`; `CAPTURE_DEFAULT` cross-checked against `flags=0x0000018a` in every f-a.txt record. DYNAMIC_ADJUST is rejected, never masked. |

### RS-14 — crates/pinyin-oracle/src/error.rs

| Field | Value |
|---|---|
| Entry point | `mod error` (private), `pub use error::OracleError`. |
| Owning subsystem | pinyin-oracle |
| Purpose | The single `#[non_exhaustive]` error enum for pin, FFI, capture and replay failures. |
| Implementation | Rust, 244 lines. 21 variants: `ManifestUnreadable{path,source}`, `ManifestNotUtf8`, `ManifestFieldMissing`, `ManifestFieldMismatch{field,expected,found}`, `ManifestFieldRepeated`, `PrefixArtefactMissing`, `PrefixNotFound{tried}`, `InteriorNul{what}`, `PathNotRepresentable`, `ContextInitFailed{system_dir,user_dir}`, `InstanceAllocFailed`, `DynamicAdjustRejected{flags}`, `Call{function}`, `TrainPreconditionFailed{parsed,input_len,candidates}`, `NonUtf8{function}`, `UnknownCandidateType{value}`, `ParsedLengthOutOfRange`, `CaptureFieldMissing`, `CaptureFieldMalformed{field,value}`, `ReplayInputMissing{input}`, `CaptureRecordInvalid{line,source:Box<_>}`. `Display` + `std::error::Error` (source only for ManifestUnreadable). |
| Inputs | none |
| Outputs | — |
| Platform | portable |
| Consumers | whole crate. |
| State | none |
| Overlap | `live.rs::FreshUserDir::create` reuses `ManifestUnreadable` for a `create_dir_all` failure (variant name misleading there). |
| Constraints | "Every variant is a reported failure rather than a panic." `PrefixNotFound` Display embeds the build-oracle.sh hint. |

### RS-15 — crates/pinyin-oracle/src/model_cache.rs

| Field | Value |
|---|---|
| Entry point | `pub mod model_cache` — `pub use oxpinyin_testsupport::model_cache::*;` |
| Owning subsystem | pinyin-oracle (re-export shim) |
| Purpose | Keeps `pinyin_oracle::model_cache` stable for bins/tests/benches while the canonical implementation lives in testsupport (so `oxpinyin-capi` benches can use it without a `capi → pinyin-oracle` edge). |
| Implementation | Rust, 11 lines. |
| Inputs | — |
| Outputs | re-export |
| Platform | portable |
| Consumers | `benches/support/mod.rs` (`model_cache::locate_model_dir`), `tests/model_fetch.rs`, crate-root re-exports. |
| State | — |
| Overlap | Pure alias of RS-45. |
| Constraints | Comment: "without the forbidden `oxpinyin-capi` → `pinyin-oracle` edge". |

### RS-16 — crates/pinyin-oracle/src/sentence_tail.rs

| Field | Value |
|---|---|
| Entry point | `pub mod sentence_tail` (always compiled, no FFI). |
| Owning subsystem | pinyin-oracle |
| Purpose | Port-vs-pin sentence-surface measurement over `fixtures/w4/oracle-sentence-surface.txt`, shared by the `sentence-tail` bin and the `sentence_surface_parity` test "so the reported numbers and the asserted numbers cannot drift apart". |
| Implementation | Rust, 393 lines. Items: `PortSession = Session<SystemDictionary, BigramLanguageModel>`, `SENTENCE_FIXTURE`, `repo_root()` (CARGO_MANIFEST_DIR/../..), `export_dir() -> Option<PathBuf>`, `open_session_from_env() -> Result<Option<PortSession>, String>`, `SentenceTailReport` (16 counters + 3 diff-line vecs; `distinct_set_match`), `measure()`. Private fixture parser `load_fixture` (TAB fields: input, guessed `true`, `\x01`-joined sentences, `\x01`-joined rows `letter/nbest/text`), `observe_port` (reset → type_pinyin → guess_sentence → read sentences 0..=proven clamped to `MAX_INDEX=2` → first `ROW_DEPTH=6` rows), `render_row`, `as_multiset`, `distinct_set`, `first_divergent_rank`, `sentence_rows`, `phrase_rows`. Uses `oxpinyin_data::{BigramLanguageModel, SystemDbm, SystemDictionary}`, `oxpinyin_engine::{Candidate, CandidateKind, EmptyConfigSource, Session, StoragePaths}`. |
| Inputs | Env via testsupport: `PINYIN_EXPORT_DIR` (default `/tmp/oxpinyin-export`), `PINYIN_MODEL_DIR`/`PINYIN_MODEL_CACHE`/`CARGO_TARGET_DIR` (doc says model dir must be complete, though the code only opens the export dir: `SystemDictionary::open`, `BigramLanguageModel::open(<dir>/bigram.db)`, `set_lambda_from_table_conf(<dir>/table.conf)`). Fixture file. |
| Outputs | `SentenceTailReport`; `Err(String)` on unreadable fixture / port call failure; `Ok(None)` when export tables absent. |
| Platform | portable code; needs real tables (Linux hosts in practice). |
| Consumers | bin `sentence-tail`, test `sentence_surface_parity`, `tools/bisection/gcov_measure.py` (builds/runs the test). |
| State | `StoragePaths::new("user")` relative user path; read-only. |
| Overlap | `open_session_from_env`'s dict+lm+table.conf open sequence is repeated in `corpus_tail.rs::open_env`, `parity_sweep.rs::main`, `parity_worst.rs::main`, `benches/support::load_real_tables`, `scan_perf::load_scoring_tables` (six copies). `export_dir()` three-table existence check repeated in `corpus_tail.rs::export_dir`, `parity_sweep.rs`, and `testsupport::model_cache::export_dir` (which panics instead). |
| Constraints | `docs/findings/sentence-surface.md` §12 is the write-up; "this module is its reproduction and its gate". Row letters `n`=Sentence, `N`=Phrase, `a`=Addon, `f`=Fallback, `?`=other. Hard-codes `table.conf` file name and `SystemDbm::Bigram.file_name()`. |

## pinyin-oracle binaries (`crates/pinyin-oracle/src/bin`)

### RS-17 — crates/pinyin-oracle/src/bin/parity_corpus.rs

| Field | Value |
|---|---|
| Entry point | `cargo run -p pinyin-oracle --bin parity-corpus -- [OUTPUT_DIR]` (default `tests/parity/corpus/inputs`); >1 arg → usage, exit 2. |
| Owning subsystem | fixtures / pinyin-oracle |
| Purpose | I/O shell that writes `corpus::generate()` strata to disk; regeneration recipe is `cargo run … -- tests/parity/corpus/inputs && git diff --exit-code tests/parity/corpus/inputs`. |
| Implementation | Rust, 54 lines; std only + `pinyin_oracle::corpus`. |
| Inputs | argv[1]. No env. |
| Outputs | 9 stratum files; stdout `count  file  description` per stratum + total line; exit 0 / 1 (I/O error) / 2 (usage). |
| Platform | portable |
| Consumers | `docs/testing/parity-corpus.md`, `tests/parity_corpus.rs` error messages, `docs/runbooks/goldens-and-pins.md`. No script invokes it. |
| State | creates OUTPUT_DIR. |
| Overlap | none |
| Constraints | A corpus change "moves the W2-T3 divergence baseline, so it belongs in its own commit". |

### RS-18 — crates/pinyin-oracle/src/bin/parity_diff.rs

| Field | Value |
|---|---|
| Entry point | `cargo run -p pinyin-oracle --features oracle-ffi --bin parity-diff -- [OUTPUT_DIR]` (default `target/parity`). Without the feature prints the recipe and exits 2. |
| Owning subsystem | pinyin-oracle |
| Purpose | Runs the whole corpus through the live oracle (`LiveSource` over one `Session`) with `Taxonomy::new(DEFAULT)` and writes the three W2-T3 artifacts. |
| Implementation | Rust, 113 lines. |
| Inputs | `OraclePrefix::locate()` env chain (`PINYIN_ORACLE_PREFIX`, `PKG_CONFIG_PATH`, `HOME`, `PINYIN_BENCH_DBM`); `corpus::generate()` (in-memory, not the committed files). |
| Outputs | `<OUT>/comparisons.tsv` (pinyin-diff-v1), `<OUT>/divergences.tsv` (pinyin-divergence-v1), `<OUT>/summary.txt` (RunReport + BudgetVerdict summaries); summary also on stdout; stderr progress (`oracle <pin_ref>`, `corpus N inputs`, `wrote …`). Exit 0/1/2. |
| Platform | Linux (oracle-ffi). |
| Consumers | `oracle-paths` bin consumes its TSVs; "W2-T4 is the human triage pass over the divergence log". |
| State | temp user dir via `open_with_temp_user_dir`; `ORACLE_LOCK`. |
| Overlap | Case construction (`"{file}:{index}"`) duplicated in `tests/differential_live.rs::corpus_cases`. |
| Constraints | Case ids are `<stratum file>:<index>`; corpus field = stratum file name. |

### RS-19 — crates/pinyin-oracle/src/bin/oracle_paths.rs

| Field | Value |
|---|---|
| Entry point | `cargo run -p pinyin-oracle --bin oracle-paths -- [RUN_DIR] [OUTPUT]` (defaults `target/parity`, `fixtures/w4/oracle-paths.txt`). |
| Owning subsystem | fixtures / pinyin-oracle |
| Purpose | Recovers the oracle's selected segmentation for every corpus input from a `parity-diff` run: `theirs_path` verbatim from `divergences.tsv`, else our parser's path at the recorded `rank` from `comparisons.tsv` ("our parser is frozen and deterministic, so path `rank` *is* the oracle's path"). Produces the portable `pinyin-oracle-path-v1` fixture. |
| Implementation | Rust, 177 lines; uses `oxpinyin_core::{Completeness, FullPinyinParser, InputParser, ParseResult}`, `capture::{escape, unescape}`, `EXPECTED_PIN_REF`, `OracleFlags`. Own `fields()` TAB splitter and `render()`. |
| Inputs | `<RUN_DIR>/comparisons.tsv`, `<RUN_DIR>/divergences.tsv`. Every record's `pin_ref` must equal `EXPECTED_PIN_REF` (else error). |
| Outputs | Fixture with 7 header lines (`# pinyin-oracle-path-v1`, `# pin_ref=…`, `# flags=0x0000018a`, 4 comment lines) then `input\tconsumed\tpath` (escaped input; `-` for empty path); stderr summary `compared/from rank/from log/sentinels/wrote`. Exit 0/1. |
| Platform | portable (no FFI). |
| Consumers | `tests/graph_paths.rs`, `tests/decode_differential.rs` (`include_str!` the output). |
| State | none |
| Overlap | `render` == `differential::render_our_path`; `fields` == `CaptureRecord::parse_line` minus owned strings. |
| Constraints | Sentinel divergences (`reason=oracle-sentinel`) are skipped → 10,459 of 10,465 lines. Fixture is tracked; freshness checked only indirectly (graph_paths asserts count 10,459 and pin_ref presence). |

### RS-20 — crates/pinyin-oracle/src/bin/oracle_candidates.rs

| Field | Value |
|---|---|
| Entry point | `cargo run -p pinyin-oracle --features oracle-ffi --bin oracle-candidates -- [OUTPUT]` (default `fixtures/w4/oracle-candidates.txt`); feature-off → exit 2. |
| Owning subsystem | fixtures / pinyin-oracle |
| Purpose | One-time (~3 h) live run freezing depth-10 candidate lists for every distinct corpus input (10,312 of 10,465). |
| Implementation | Rust, 149 lines. `SCHEMA = "pinyin-oracle-candidates-v1"`, `DEPTH = 10`. |
| Inputs | oracle prefix env chain; `corpus::generate()`. |
| Outputs | Header `# pinyin-oracle-candidates-v1`, `# pin_ref=`, `# corpus=`, `# total_inputs=N (distinct M)`, `# depth=10`, `# total_triples=T (distinct inputs with candidates C)`, 4 format comments; then `input\trank\tcandidate_text` sorted bytewise by input then rank. stderr progress/summary. Exit 0/1/2. |
| Platform | Linux. |
| Consumers | `real_tables_integration::oracle_candidates_fixture_is_fresh`, `candidate_structure` test (sister), bins `corpus-tail`, `parity-sweep`, `parity-worst`; `docs/runbooks/goldens-and-pins.md`. |
| State | temp user dir, `ORACLE_LOCK`. |
| Overlap | Near-identical to RS-21 (same loop, 2 fewer columns). The fixture parser `input\trank\ttext → BTreeMap<String, Vec<String>>` is re-implemented in `corpus_tail.rs::load_fixture`, `parity_sweep.rs::load_fixture`, `parity_worst.rs::main`, `real_tables_integration.rs::load_candidate_fixture` (four copies). |
| Constraints | "input and cand contain no TAB or LF (corpus domain / candidate text)". Comment lines start with `#`, which collides with corpus input `#nishaozhan`-style junk inputs — readers filter by `starts_with('#')` so such inputs are dropped (see graph_paths.rs note; `oracle_sentence_surface.rs` counts `#yaguai` as unreadable). |

### RS-21 — crates/pinyin-oracle/src/bin/oracle_candidate_structure.rs

| Field | Value |
|---|---|
| Entry point | `cargo run -p pinyin-oracle --features oracle-ffi --bin oracle-candidate-structure -- [OUTPUT]` (default `fixtures/w4/oracle-candidate-structure.txt`). |
| Owning subsystem | fixtures / pinyin-oracle |
| Purpose | W2-CAND: sister of RS-20 adding per-candidate `type` (public `lookup_candidate_type_t` name) and `nbest_index` (`-` unless NBEST_MATCH). |
| Implementation | Rust, 184 lines; `Session::observe_candidate_infos`; progress every 500 distinct inputs with elapsed time. `SCHEMA = "pinyin-oracle-candidate-structure-v1"`. |
| Inputs | as RS-20. |
| Outputs | Header (13 lines) then `input\trank\ttext\ttype\tnbest_index`. Exit 0/1/2. |
| Platform | Linux. |
| Consumers | `tests/candidate_structure.rs` (portable column check vs sister; ignored freshness). |
| State | as RS-20. |
| Overlap | RS-20. |
| Constraints | `(input, rank, text)` columns must be byte-identical to RS-20's output ("both come from the same `collect_candidates` under the same `0x1e` sort"). |

### RS-22 — crates/pinyin-oracle/src/bin/oracle_sentence_surface.rs

| Field | Value |
|---|---|
| Entry point | `cargo run -p pinyin-oracle --features oracle-ffi --bin oracle_sentence_surface` (underscore name: no `[[bin]]` entry, auto-discovered). No args. Feature-off → exit 2. |
| Owning subsystem | fixtures / pinyin-oracle |
| Purpose | W14 fixture generator: ibus call order (`guess_sentence` → `guess_candidates` → `get_sentence`) over a deterministic sample of the corpus (every `stride`-th input, `SAMPLE = 500`, plus 8 `ANCHORS`). |
| Implementation | Rust, 148 lines. Writes to the path baked in with `concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/w4/oracle-sentence-surface.txt")`; reads corpus from `…/../../tests/parity/corpus/inputs` (committed files, not `generate()` output directly, though it uses `generate()` for file names). |
| Inputs | oracle prefix env chain; committed corpus files. |
| Outputs | `# pin_ref=…`, `# sample=N` (N counts rows not starting with `#`), then `input\tguessed\tsentences(\x01-joined, "-" for None)\trows(\x01-joined letter/nbest/text, first 6)`. stderr `wrote … N inputs`. Exit 0 / 1 / 2. |
| Platform | Linux. |
| Consumers | `sentence_tail.rs`, `real_tables_integration::sentence_surface_fixture_is_fresh`, `tools/bisection/gcov_measure.py`, `tools/bisection/axis-c-replacement.json` (sha256 of the fixture), `docs/runbooks/benches.md` (`--sentence-fixture`/`--train-fixture` for the C harness). |
| State | temp user dir, `ORACLE_LOCK`. |
| Overlap | `render_row` letter mapping duplicated in `sentence_tail.rs` and `real_tables_integration.rs`. |
| Constraints | `observe_sentence_surface(input, 2)` — `MAX_INDEX` 2 is also hard-coded in `sentence_tail.rs`. Committed file says `# sample=507` while the test pins `comparable == 500`. The `#yaguai` input renders as a comment for every reader. |

### RS-23 — crates/pinyin-oracle/src/bin/parity_sweep.rs

| Field | Value |
|---|---|
| Entry point | `cargo run -p pinyin-oracle --release --bin parity-sweep`; env `PARITY_SWEEP_FULL=1` re-scores the winner on the full corpus. |
| Owning subsystem | perf / pinyin-oracle (scoring-constant exploration) |
| Purpose | Grid-sweeps `ScoringConfig{segmentation_penalty, incomplete_penalty, phrase_key_bonus}` over a stratified 2,000-input sample against `oracle-candidates.txt`, printing top-1/top-5/absent per trial. |
| Implementation | Rust, 273 lines; `oxpinyin_core::scoring::ScoringConfig`, `oxpinyin_data`, `oxpinyin_engine::Session::set_scoring_config`. |
| Inputs | Env `PINYIN_EXPORT_DIR` (via `resolve_export_dir`), `PARITY_SWEEP_FULL`; files `<export>/{pinyin_index.bin,phrase_index.bin,bigram.db,table.conf}`, `fixtures/w4/oracle-candidates.txt`, committed corpus. |
| Outputs | stderr table per trial, `best sample:` line, optional full-corpus line. Exit 0; 2 when tables missing. |
| Platform | portable code; needs export tables. |
| Consumers | none in tree (manual; results recorded in `docs/findings/scoring-constant-sweep.md`). |
| State | read-only; `StoragePaths::new("user")`. |
| Overlap | fixture loader + corpus loader + table open sequence (see RS-16/RS-20). |
| Constraints | Baseline is the frozen default `seg=750 inc=999 bonus=1000`; grid skips configs with `incomplete_penalty >= phrase_key_bonus`. |

### RS-24 — crates/pinyin-oracle/src/bin/parity_worst.rs

| Field | Value |
|---|---|
| Entry point | `cargo run -p pinyin-oracle --release --bin parity-worst`. |
| Owning subsystem | pinyin-oracle (analysis) |
| Purpose | Lists the 50 worst top-1 misses (absent first, then our rank descending) with a heuristic category, plus a rank histogram over all misses. |
| Implementation | Rust, 208 lines. |
| Inputs | `PINYIN_EXPORT_DIR`; `fixtures/w4/oracle-candidates.txt`; committed corpus. |
| Outputs | stderr only. Exit 0; 2 if `pinyin_index.bin` missing. |
| Platform | portable code; needs export tables. |
| Consumers | none in tree (manual analysis). |
| State | read-only. |
| Overlap | Same loaders as RS-23/RS-25; categorisation heuristics differ from `corpus_tail.rs`. |
| Constraints | Checks only `SystemDbm::PinyinIndex` existence (weaker than the other bins' three-table check). |

### RS-25 — crates/pinyin-oracle/src/bin/corpus_tail.rs

| Field | Value |
|---|---|
| Entry point | `cargo run -p pinyin-oracle --release --bin corpus-tail [--all-off-tails]`; any other arg → exit 1. |
| Owning subsystem | pinyin-oracle (W12 corpus tail) |
| Purpose | Enumerates the residual inputs behind the `real_tables_session_reports_parity` pins (top1-miss, top5-miss, absent, order-only, prefix-10 gap); with `--all-off-tails` reproduces the six W12 ALL-BITS-OFF TEXT-set tails plus the correction-alias equivalence (`lue`+UE_VE→`lve`, `agn`+GN_NG→`ang`, `amg`+MG_NG→`ang`) and option-word invariance, all without FFI. |
| Implementation | Rust, 409 lines; `oxpinyin_core::{OptionBits, PINYIN_CORRECT_GN_NG, PINYIN_CORRECT_MG_NG, PINYIN_CORRECT_UE_VE}`, `Session::{set_options, composition_keys, parsed_prefix_len}`. |
| Inputs | `PINYIN_EXPORT_DIR` (+ doc says `PINYIN_MODEL_DIR`, not actually read); `fixtures/w4/oracle-candidates.txt`; committed corpus. |
| Outputs | stdout report (`== W2 corpus tail residuals ==` … / `== W12 all-off TEXT-set tails ==` …) "for capture into `docs/testing/corpus-tail.md`"; stderr on error. Exit 0/1. |
| Platform | portable code; needs export tables. |
| Consumers | `docs/testing/corpus-tail.md`, `docs/findings/all-off-tails.md`, `tools/bisection/run-option-sweep.sh:184` comment. |
| State | read-only. |
| Overlap | `export_dir()` duplicates `sentence_tail::export_dir` almost verbatim (Result vs Option). |
| Constraints | Argues the `0x18a` fixture row is the `0x0` answer for single-syllable entries (`all-off-tails.md`). |

### RS-26 — crates/pinyin-oracle/src/bin/sentence_tail.rs

| Field | Value |
|---|---|
| Entry point | `cargo run -p pinyin-oracle --release --bin sentence-tail` with `PINYIN_EXPORT_DIR`, `PINYIN_MODEL_DIR`. |
| Owning subsystem | pinyin-oracle (W14 sentence tail) |
| Purpose | Prints the `sentence_tail::measure` report (three strictnesses + residual breakdown + per-input diff dumps). "This binary opens the session from the environment and prints; the test asserts." |
| Implementation | Rust, 108 lines; thin shell over RS-16. |
| Inputs | via `sentence_tail::open_session_from_env`. |
| Outputs | stdout report; stderr message + exit 1 when tables absent. |
| Platform | portable code; needs export tables. |
| Consumers | `docs/findings/sentence-surface.md` §12. |
| State | read-only. |
| Overlap | none beyond RS-16. |
| Constraints | Doc: "reports it — it does not assert a pin". Doc-comment numbers (496 comparable, 488/385/379) are stale relative to the test (500/500/500). |

## pinyin-oracle integration tests (`crates/pinyin-oracle/tests`)

Portable = runs with `oracle-ffi` off on all three CI OSes. "Needs oracle" = `#![cfg(feature = "oracle-ffi")]` or `#[cfg(feature="oracle-ffi")]` per test.

### RS-27 — crates/pinyin-oracle/tests/capture_replay.rs

| Field | Value |
|---|---|
| Entry point | `cargo test -p pinyin-oracle --test capture_replay`. 4 tests, none ignored, portable. |
| Owning subsystem | pinyin-oracle / capture |
| Purpose | The capture reader against the real frozen fixtures: F-A has 15 records, F-C 46, all on-pin, flags `0x18a` (F-A) / contain IS_PINYIN and not DYNAMIC_ADJUST (F-C), self-consistent, remainder length correct; spot-checks frozen field values (nihao=126 candidates, 你好 first; fangan `[fan,gan]`; nih partial; chang'an gap; junk-prefix; empty; 4096 junk); no record carries a sentinel. |
| Implementation | Rust, 155 lines; reads `fixtures/foundation/{f-a,f-c}.txt` via `CARGO_MANIFEST_DIR/../../fixtures/foundation`. |
| Inputs | the two fixture files (missing → panic). |
| Outputs | libtest. |
| Platform | portable |
| Consumers | ci.yml `test` job. |
| State | none |
| Overlap | `fixture()` helper duplicated in `differential_replay.rs`, `taxonomy_replay.rs`. |
| Constraints | Pins record counts from `capture-fixtures.md` (15/46). |

### RS-28 — crates/pinyin-oracle/tests/differential_replay.rs

| Field | Value |
|---|---|
| Entry point | `--test differential_replay`. 9 tests, portable, none ignored. |
| Owning subsystem | pinyin-oracle |
| Purpose | W2-T3 acceptance on the portable tier: runner over F-A (15 agreements, 0 divergences, exactly one rank-1 = fangan with `ours_paths=3`), log schema/field order, single-line records incl. the 4096-byte case, byte-identical reruns, F-C comparable (divergences carry `class=unclassified` under `Unclassified`), tampered pin ref → `reason=off-pin`, summary text, wire spellings of Outcome/Reason. |
| Implementation | Rust, 239 lines. |
| Inputs | foundation fixtures. |
| Outputs | libtest; prints F-C summary. |
| Platform | portable |
| Consumers | ci.yml `test`. |
| State | none |
| Overlap | see RS-27. |
| Constraints | — |

### RS-29 — crates/pinyin-oracle/tests/taxonomy_replay.rs

| Field | Value |
|---|---|
| Entry point | `--test taxonomy_replay`. 6 tests, portable. |
| Owning subsystem | pinyin-oracle |
| Purpose | W2-T5 on the portable tier: F-A → 14 output-identical + 1 tie-swap; F-C → 10 flag-semantics, 36 agreements, no path-set/ours-bug; re-classifying F-C under `Taxonomy::new(BASELINE)` yields path-set; budget over F-A (limit 0); class set has 8 distinct wire names; no `class=unclassified` once `Taxonomy` is wired. |
| Implementation | Rust, 170 lines. |
| Inputs | foundation fixtures. |
| Platform | portable |
| Consumers | ci.yml `test`. |
| State | none |
| Overlap | RS-27 helper. |
| Constraints | Counts frozen in `divergence-taxonomy.md`. |

### RS-30 — crates/pinyin-oracle/tests/parity_corpus.rs

| Field | Value |
|---|---|
| Entry point | `--test parity_corpus`. 4 tests, portable. |
| Owning subsystem | fixtures |
| Purpose | W2-T2 acceptance: committed corpus files are byte-identical to `corpus::generate()`; directory holds exactly the 9 strata; per-stratum counts (405/2000/2500/1500/1200/1200/800/800/60, total 10,465); every file ends with LF, no blank line except in 09-edge, no edge whitespace/CR/TAB. |
| Implementation | Rust, 144 lines. |
| Inputs | `tests/parity/corpus/inputs/*`. |
| Platform | portable |
| Consumers | ci.yml (path filter `tests/parity/**` triggers it). |
| State | none |
| Overlap | Count table duplicated from `corpus.rs` unit test `strata_reach_their_documented_counts`. |
| Constraints | Regenerate command in panic message. |

### RS-31 — crates/pinyin-oracle/tests/graph_paths.rs

| Field | Value |
|---|---|
| Entry point | `--test graph_paths`. 5 tests, portable. |
| Owning subsystem | pinyin-oracle (W4-T1) |
| Purpose | Every path the pin selected (10,459 records in `oracle-paths.txt`) is a walkable path through `oxpinyin_core::graph::SegmentGraph`, with `consumed()` agreeing; ≥481 mid-partial records all walk; `OPEN_APOSTROPHE_CASES: [&str; 0]` (both apostrophe cases closed 2026-08-21). |
| Implementation | Rust, 233 lines; `include_str!` of the fixture; own `parse_path`. |
| Inputs | `fixtures/w4/oracle-paths.txt` (compile-time). |
| Platform | portable |
| Consumers | ci.yml `test`. |
| State | none |
| Overlap | Segment-notation parser duplicated from `capture::parse_segment`. |
| Constraints | Asserts fixture contains `pinyin-oracle-path-v1` and `EXPECTED_PIN_REF`; data lines identified by containing TAB (so `#`-leading junk inputs are kept). |

### RS-32 — crates/pinyin-oracle/tests/decode_differential.rs

| Field | Value |
|---|---|
| Entry point | `--test decode_differential [-- --nocapture]`. 2 tests, portable. |
| Owning subsystem | pinyin-oracle (W4-T4) |
| Purpose | Decode-level differential: (1) segmentation over 10,459 recovered paths via `SegmentGraph::build` + `kbest::k_best(K=8)` with `Scorer` over `FixtureDictionary`/`FixtureLanguageModel` — asserts total 10,459, absent == `[]`, top-1 ≥ 80%; (2) candidates over the F-A/F-C records with a candidate list through `oxpinyin_engine::Session` (fixture doubles) — asserts covered ≥ 8, top-5-set ≥ 80%. |
| Implementation | Rust, 270 lines; `include_str!` of oracle-paths, f-a, f-c, mini-vocab, mini-bigram. |
| Inputs | the five fixtures (compile-time). |
| Outputs | prints the two report blocks defined in `docs/findings/decode-differential.md`. |
| Platform | portable |
| Consumers | ci.yml `test`; reproduction command in the finding. |
| State | none |
| Overlap | Own `field()` TAB splitter and hex decoder (vs `capture::parse_candidates`); path parser vs `capture::parse_segment`. |
| Constraints | Candidate numbers "must not be quoted as parity" (90-phrase vocab). Gate: unreachable set must be exactly the open cases (now none) and top-1 > 80%. |

### RS-33 — crates/pinyin-oracle/tests/candidate_structure.rs

| Field | Value |
|---|---|
| Entry point | `--test candidate_structure`. 1 portable test + 1 `#[cfg(feature="oracle-ffi")] #[ignore = "full-corpus live oracle run; regenerate with bin oracle-candidate-structure"]`. |
| Owning subsystem | fixtures / pinyin-oracle |
| Purpose | Portable: structure fixture's `(input,rank,text)` triples equal the sister fixture line-for-line, both stamped with `EXPECTED_PIN_REF`; `type` is a known name, never `LONGER_CANDIDATE` or `PREDICTED_*`; `nbest_index` numeric iff NBEST_MATCH. Freshness: re-captures all distinct corpus inputs and compares payload verbatim. |
| Implementation | Rust, 228 lines. |
| Inputs | `fixtures/w4/oracle-candidates.txt`, `oracle-candidate-structure.txt` (missing → panic, "Absence is a failure, not a skip"); oracle prefix for the ignored test. |
| Platform | portable / Linux (ignored). |
| Consumers | ci.yml `test`; freshness manual. |
| State | none / temp user dir. |
| Overlap | `load`/`payload`/`pin_ref` helpers vs `real_tables_integration.rs` loaders. |
| Constraints | Needs oracle: freshness test only. |

### RS-34 — crates/pinyin-oracle/tests/model_fetch.rs

| Field | Value |
|---|---|
| Entry point | `--test model_fetch`. 2 portable tests + `#[cfg(unix)] mod fetch_script` with 5 tests that spawn `bash tools/model/fetch-model.sh`. None `#[ignore]`d. |
| Owning subsystem | oracle provisioning (model fetch contract) |
| Purpose | Pins the locator (`verified_model_dir` accepts 18 files, rejects a partial export naming the missing file) and the script's failure contract: checksum mismatch names expected+actual digests; unreachable URL (`https://127.0.0.1:1/…`) fails cleanly; a repo-internal `--cache-dir` is refused with "refusing to write model bytes" (git-present branch) and "without git ignore checks" (git shimmed to `exit 1` on PATH); pinned archive extracts and second run prints "skipping fetch". |
| Implementation | Rust, 358 lines; `std::process::Command`, scratch dirs `<tmp>/oxpinyin-model-<label>-<pid>-<nanos>-<seq>` removed on drop. |
| Inputs | Env `PINYIN_MODEL_ARCHIVE` (optional local archive), `PATH` (shimmed), `CARGO_MANIFEST_DIR`; probes `target/{model20,oracle-pin-build,oracle-tkrzw-build,oracle,oracle-w2}/downloads/model20.text.tar.gz`. Creates/removes `crates/pinyin-oracle/__must_not_vendor_model20{,_nogit}`. |
| Outputs | libtest. **`pinned_archive_extracts_and_second_run_skips` self-skips** (eprintln + return) when no local archive is present — a surviving exception to the README's no-self-skip rule. |
| Platform | portable for 2 tests; unix for the script tests (needs bash, sha256sum, tar). The unreachable-URL test performs a real network attempt to localhost:1. |
| Consumers | ci.yml `test`. |
| State | temp scratch dirs; writes inside the crate dir for the refusal tests (asserted removed). |
| Overlap | `workspace_root()` walk duplicated from `testsupport::model_cache::workspace_root_from` with a different sentinel file. |
| Constraints | "Keep in lockstep with tools/model/fetch-model.sh" (EXPECTED_MODEL_FILES). |

### RS-35 — crates/pinyin-oracle/tests/live_smoke.rs

| Field | Value |
|---|---|
| Entry point | `cargo test -p pinyin-oracle --features oracle-ffi --test live_smoke`. 10 tests, whole file `#![cfg(feature = "oracle-ffi")]`; **needs oracle**; none ignored. |
| Owning subsystem | pinyin-oracle |
| Purpose | W2-T1 acceptance against frozen F-A values: nihao (5/5 parsed, `[ni,hao]`, 126 candidates, first ten listed), ni/zhongguoren/fangan/nih/chang'an, junk & empty & 4096 junk, reset ≡ fresh instance, interior NUL rejected before FFI, DYNAMIC_ADJUST rejected, located prefix has header sha `e1138482…`, commit `074a2219…`, fresh user dir created and removed, `save_user_data` no-op twice. |
| Implementation | Rust, 284 lines; `expected_ref()` honours `PINYIN_BENCH_DBM` (kc/bdb) via `bench_pin_ref`. |
| Inputs | oracle prefix env chain; `PINYIN_BENCH_DBM`. |
| Platform | Linux. |
| Consumers | runbook `oracle.md`; no CI job (oracle never in CI). |
| State | `ORACLE_LOCK` serialises the 10 tests; temp user dirs. |
| Overlap | `oracle()` helper duplicated in `differential_live.rs`. |
| Constraints | "assertions are not invented: they are the values frozen in f-a.txt". |

### RS-36 — crates/pinyin-oracle/tests/differential_live.rs

| Field | Value |
|---|---|
| Entry point | `--features oracle-ffi --test differential_live`. 10 tests, whole file gated; **needs oracle**; none ignored. |
| Owning subsystem | pinyin-oracle |
| Purpose | W2-T3 live acceptance over the full corpus (one shared `OnceLock` run; determinism test runs it twice): total 10,465, unobservable 0; pinned counts agreements 9,976 / divergences 489 (path-absent 481, consumed-length 2, oracle-sentinel 6; no off-pin/inconsistent/overflow/ours-error/remainder); rank-0 9,508, positive rank 468; abort guard fires exactly on `'`,`''`,`'''`; F-E-01 shape `ni@0:2:complete,<missing-pinyin>@2` for `ni'`,`ni'!`,`ni'i`; spec-contradiction examples (`yingchon`, `fanrufe`, `ngannve`, `zzzzzzzz`); class roll-up 9,508/468/483/6, others 0; gating = 483; budget limit 52, both readings over; no unclassified. |
| Implementation | Rust, 348 lines. |
| Inputs | oracle prefix env chain; `corpus::generate()`. |
| Platform | Linux. |
| Consumers | runbook `oracle.md`; manual. |
| State | `ORACLE_LOCK`; `static RUN: OnceLock<RunOutput>`. |
| Overlap | `corpus_cases()` duplicates `parity_diff.rs`. |
| Constraints | "These assertions deliberately pin *measured* numbers … update it in the same commit as the reason." Note: `divergence-taxonomy.md` frozen table says path-set 483/divergences 489 at pin 0c5e80e; the test tracks the live post-2026-08-27 numbers. |

### RS-37 — crates/pinyin-oracle/tests/export_seam.rs

| Field | Value |
|---|---|
| Entry point | `--features oracle-ffi --test export_seam`. 1 test, gated; **needs oracle**. |
| Owning subsystem | pinyin-oracle (W6 export seam) |
| Purpose | Smoke: `export_phrases(1)` (gb_char) returns >90,000 tuples incl. `你好|ni'hao` with count>0; `lookup_tokens("你好")` yields a token with top byte 1; `token_text` round-trips; junk token `0x00ff_ffff` → `None`. |
| Implementation | Rust, 48 lines. |
| Inputs | oracle prefix env chain. |
| Platform | Linux. |
| Consumers | manual; "full table verification lives in `export_verification.rs`" (not in this crate). |
| State | `ORACLE_LOCK`, temp user dir. |
| Overlap | — |
| Constraints | Library index 1 = gb_char hard-coded. |

### RS-38 — crates/pinyin-oracle/tests/real_tables_integration.rs

| Field | Value |
|---|---|
| Entry point | `--features oracle-ffi --test real_tables_integration -- --ignored --nocapture`. 2 tests, both `#[cfg(feature="oracle-ffi")]` + `#[ignore]`: `oracle_candidates_fixture_is_fresh` ("full-corpus live oracle run (~3h); regenerate with bin oracle-candidates") and `sentence_surface_fixture_is_fresh` ("live oracle run (~1 min); regenerate with bin oracle_sentence_surface"). With the feature off the file compiles to zero tests. **Needs oracle.** |
| Owning subsystem | fixtures / pinyin-oracle |
| Purpose | Freshness of `oracle-candidates.txt` (pin_ref, header `total_triples` vs payload, per-input live-vs-frozen lists, payload regeneration) and of `oracle-sentence-surface.txt` (guessed, sentences, first-6 rows as `(char, Option<u8>, String)`). |
| Implementation | Rust, 437 lines; own loaders `load_w2_corpus` (returns `None` + eprintln on missing corpus — a self-skip inside an ignored test), `load_candidate_fixture`, `load_sentence_fixture`. |
| Inputs | oracle prefix env chain; the two fixtures; committed corpus. |
| Platform | Linux. |
| Consumers | `docs/runbooks/goldens-and-pins.md`. |
| State | `ORACLE_LOCK`, temp user dir. |
| Overlap | Loaders duplicate RS-20/RS-22 parsers; `repo_root()` appears in 8 files of this crate. |
| Constraints | File docs name the regen bins. |

### RS-39 — crates/pinyin-oracle/tests/sentence_surface_parity.rs

| Field | Value |
|---|---|
| Entry point | `cargo test -p pinyin-oracle --test sentence_surface_parity -- --include-ignored` with `PINYIN_EXPORT_DIR`, `PINYIN_MODEL_DIR`. 4 tests: `sentence_surface_matches_the_declared_residual` (ignored: "needs the system-table export and the model20 cache (PINYIN_EXPORT_DIR, PINYIN_MODEL_DIR)"), `sentence_surface_detects_per_step_cost_shift` (same ignore), `sentence_tie_fixture_uses_only_the_declared_residuals` (portable, not ignored), `sentence_surface_matches_supplementary_tie_order` (ignored: "needs the matching system-table export (PINYIN_EXPORT_DIR)"). **No oracle FFI**; needs real tables (the bdb export per tie fixture's `dbm-bdb` pin_ref). |
| Owning subsystem | pinyin-oracle (W14 / #747 gate) |
| Purpose | Asserts §12 numbers: comparable 500, guessed_disagree 0, row0 500, distinct-set 500, ordered 500, rows 500, order-only 0, distinct-extra 0; native trellis costs for four #574 inputs (32746/46792/59752/17763); the tie fixture (`sentence-tie-order-v1`, 8,592 cases, 8,744 occurrences, 22,038 NBEST rows) uses only the six residuals declared in `docs/findings/sentence-surface.md` §12's table (parsed from the markdown at test time); every other input matches the pin row-for-row via `Session::replace_raw` with options `0x18a` and sort `0x1e` (pin-equal 8,586, residuals 6). |
| Implementation | Rust, 333 lines; parses a markdown table from the docs tree (`split_once("\n## 12. ")` …). |
| Inputs | `fixtures/w4/oracle-sentence-surface.txt`, `fixtures/w4/oracle-sentence-tie-order.txt`, `docs/findings/sentence-surface.md`, export tables. |
| Platform | portable code; ignored tests need Linux-hosted tables. |
| Consumers | `tools/bisection/gcov_measure.py` (builds `-p pinyin-oracle --test sentence_surface_parity` once in an isolated target, runs the four tests), `delta_ledger.py`, `render_ledger.py`. |
| State | read-only. |
| Overlap | tie-fixture decoder is unique; residual membership deliberately sourced from the markdown ("this supplementary gate must not introduce another list of names"). |
| Constraints | "Re-measure and obtain approval before updating these numbers and §12 together". Ignore reason wording differs from the README's canonical "needs …; run with --include-ignored" only slightly. |

## pinyin-oracle benches (`crates/pinyin-oracle/benches`)

### RS-40 — crates/pinyin-oracle/benches/support/mod.rs

| Field | Value |
|---|---|
| Entry point | `#[path = "support/mod.rs"] mod harness;` from `scan_perf.rs` and `alloc_profile.rs` (textual include, not a crate). `#![allow(dead_code)]`. |
| Owning subsystem | perf / pinyin-oracle |
| Purpose | Shared loaders for the window-scan benches: real tables + real-frequency LM, counting dictionary, session constructors, keystroke/batch typing, W2 inputs from committed files. |
| Implementation | Rust, 200 lines. Items: `CYCLE_INPUTS` (20 inputs), `model_dir()` (panics if cache absent), `load_real_tables()` (asserts `has_real_unigrams`), `CountingDict<'a>` + `ProbeStats` (AtomicU64 probes/probe_ns/lookups/lookup_entries; impl `Dictionary`), `counting_session`, `real_session`, `type_keystrokes`, `type_batch`, `load_w2_inputs()` (reads `CORPUS_DIR` files directly, sorted by name), `interpolation2_path()`; `pub use oxpinyin_testsupport::model_cache::export_dir`. |
| Inputs | Env via testsupport: `PINYIN_EXPORT_DIR`, `PINYIN_MODEL_DIR`, `PINYIN_MODEL_CACHE`, `CARGO_TARGET_DIR`; files `<export>/{pinyin_index.bin,phrase_index.bin,bigram.db,table.conf}`, `<model>/interpolation2.text`, corpus files. |
| Outputs | — |
| Platform | portable code; needs export + model20 cache. |
| Consumers | `scan_perf.rs`, `alloc_profile.rs`. |
| State | `StoragePaths::new("user")`. |
| Overlap | Mirrors `crates/oxpinyin-capi/benches/support/mod.rs` (which also `pub use`s `export_dir`, `MODEL20_SHA256`, `missing_model_files`, `system_dbm_names` from testsupport). `load_real_tables` is the seventh copy of the dict+lm+table.conf open. |
| Constraints | "Benches refuse to start without the exported tables and the fetched model20 cache: flat-100 fallback numbers would not describe the pinned construction." `load_w2_inputs` deliberately avoids `corpus::generate()` so its allocations stay out of dhat profiles. |

### RS-41 — crates/pinyin-oracle/benches/scan_perf.rs

| Field | Value |
|---|---|
| Entry point | `cargo bench -p pinyin-oracle --bench scan_perf` (criterion, `harness = false`). Groups: `keystroke_cycle_20_inputs`, `parse_interpolation2`, `decode_pass_user_store/{no_store_attached,empty_store_attached,populated_store_attached}`. |
| Owning subsystem | perf |
| Purpose | Criterion baseline for the window-scan construction; the `decode_pass_user_store` group rebuilds capi's `SharedLm` overlay (`BenchLm` over `UserStore::count_delta`/`unigram_delta`) to isolate the user-store cost. |
| Implementation | Rust, 216 lines; `criterion`, `oxpinyin_user::{SENTENCE_START, UserStore}`, `oxpinyin_data::{parse_interpolation2, DEFAULT_STORE_EXT, LmError}`. Config: sample_size 20, warm-up 500 ms, measurement 4 s. |
| Inputs | as RS-40; `load_scoring_tables()` (no model20) for the store group. |
| Outputs | criterion reports under `target/criterion`. |
| Platform | portable code; needs export (+ model20 for keystroke/parse groups). |
| Consumers | `docs/runbooks/benches.md` table row; `docs/perf/*`. |
| State | Temp store files `<tmp>/pinyin-oracle-decode-pass-{empty,populated}-<pid>.<DEFAULT_STORE_EXT>` removed on drop (`TempStorePath`). Populated store gets two `observe_selection` writes with hard-coded tokens `0x0100_1225`, `0x0100_05db`. |
| Overlap | `BenchLm` duplicates `oxpinyin-capi`'s `SharedLm`; `load_scoring_tables` duplicates `support::load_real_tables` minus the unigram assert. |
| Constraints | "Read the arms against each other, not against absolute numbers … ±10% band". Store extension hard-coded via `oxpinyin_data::DEFAULT_STORE_EXT` (comment still says "`.kct` under the KC default"). |

### RS-42 — crates/pinyin-oracle/benches/alloc_profile.rs

| Field | Value |
|---|---|
| Entry point | `cargo bench -p pinyin-oracle --bench alloc_profile -- [--dhat [--keystroke-only]] | [--parity-timed] | [--keystroke-only [--repeats N]]`; custom `main`, not criterion. |
| Owning subsystem | perf / profile |
| Purpose | One-shot profiles for `docs/perf/perf-exploration.md`: dhat heap dump of the W2 batch (or keystroke cycle) with setup excluded; otherwise prints interpolation2 size/parse timings, prefix-probe counts/share via `CountingDict`, and uninstrumented wall times. |
| Implementation | Rust, 198 lines; `#[global_allocator] static ALLOC: dhat::Alloc` (always installed, even without `--dhat`). |
| Inputs | argv flags; env as RS-40. |
| Outputs | `/tmp/dhat-heap-parity.json` or `/tmp/dhat-heap-keystroke.json` (hard-coded paths, written on `Profiler` drop); stdout key/value lines (`dhat_scope`, `dhat_dump`, `wall_ms`, `ops` (=123 for keystroke), `cycle_ms`, `interpolation2 …`, `parity_batch_w2 …`, `parity_uninstrumented_ms`, `keystroke_20_ms`). |
| Platform | portable code; needs export + model20. Writes to `/tmp` literally (not `temp_dir()`). |
| Consumers | `docs/perf/perf-exploration.md`; `docs/runbooks/benches.md`. |
| State | none beyond the dump files. |
| Overlap | timing loops duplicate `scan_perf` groups. |
| Constraints | Profiler must wrap only the decode loop. |

### RS-43 — crates/pinyin-oracle/benches/dbm_bench.rs

| Field | Value |
|---|---|
| Entry point | `PINYIN_ORACLE_PREFIX=<prefix> cargo bench -p pinyin-oracle --features oracle-ffi --bench dbm_bench -- [--save-baseline <name>]` (criterion); or `-- --vmhwm` (parent spawning `--vmhwm-populate <dir>` and `--vmhwm-child <op>` children of `current_exe()`). `required-features = ["oracle-ffi"]`, file `#![cfg(feature = "oracle-ffi")]`. |
| Owning subsystem | perf (libpinyin DBM backend comparison) |
| Purpose | Wall-clock and child-process VmHWM for libpinyin's user-data path (`pinyin_init`, `pinyin_train` via `train_top`, `pinyin_save`) per DBM backend: ops `libpinyin_dbm/init_load`, `train_write/64`, `train_write/256`, `user_db_open` (over a copy of a 256-train populated dir). |
| Implementation | Rust, 380 lines; `Oracle::{open, open_with_user_dir}`, `Session::train_top`, `TRAIN_INPUTS` (16 phrases), `BenchRoot` temp dirs `<tmp>/dbm-bench-<tag>-<pid>` removed on drop, `copy_dir`, `/proc/self/status` `VmHWM:` parse. |
| Inputs | Env `PINYIN_ORACLE_PREFIX` (required; exits 2 otherwise — does NOT use the `locate()` chain), `PINYIN_BENCH_DBM` (through `OraclePrefix::open`/build.rs for kc/bdb prefixes), `DBM_BENCH_POPULATED` (child only). |
| Outputs | criterion results; `--vmhwm` prints `op  vmhwm_kib` table; children print `key=value` lines. Exit 2 on missing prefix / unknown op; 1 on child failure. |
| Platform | Linux (FFI, `/proc/self/status`). |
| Consumers | `docs/runbooks/benches.md` (row + "run one with --bench"), `docs/testing/oracle-environment.md`. |
| State | Temp roots per bench; populated user dir copied per iteration outside the timed window; `ORACLE_LOCK` serialises contexts. |
| Overlap | `--vmhwm` child-per-op pattern is "the `backend_bench.rs` pattern" from oxpinyin-store. |
| Constraints | "Do not mix the two axes in one table." `train_top` precondition documented inline (`pinyin.cpp:2676`). Absolute numbers are facade+DBM, "not comparable with oxpinyin's store-tier benches". |

## oxpinyin-testsupport (`crates/oxpinyin-testsupport`)

### RS-44 — crates/oxpinyin-testsupport/src/lib.rs (+ Cargo.toml)

| Field | Value |
|---|---|
| Entry point | Crate `oxpinyin-testsupport`; `#![forbid(unsafe_code)]`, `#![warn(missing_docs)]`. |
| Owning subsystem | testsupport |
| Purpose | "Shared test support for the oxpinyin workspace: fixture-backed trait doubles. Never a dependency of shipping code." Workspace member so `cargo test --workspace` always runs its own tests. |
| Implementation | Rust, 22 lines; `pub mod differential, fixture, model_cache`; re-exports `Manifest, PinDir, fnv1a64, locate_bin, locate_data, parse_estimate_stdout, parse_manifest` and `FixtureDictionary, FixtureError, FixtureLanguageModel, FrequencyFixtureModel`. Cargo.toml (27 lines): deps `oxpinyin-core`, `oxpinyin-store`; features `default = []`, `kyotocabinet`/`bdb`/`tkrzw` → `oxpinyin-store/*`. |
| Inputs | — |
| Outputs | — |
| Platform | portable |
| Consumers | dev-deps of oxpinyin-core, -engine, -capi (benches), -corpus, -training, -datagen (the one non-dev consumer, "which itself never ships"), pinyin-oracle (normal dep), fuzz crate. 41 files reference it (see grep in Architecture notes). |
| State | — |
| Overlap | — |
| Constraints | "No default backend: … a hard default would re-enable kyotocabinet through the path dev-dependencies on macOS." `.kiro/steering/structure.md:24`: forbid unsafe, portable, never ships. |

### RS-45 — crates/oxpinyin-testsupport/src/model_cache.rs

| Field | Value |
|---|---|
| Entry point | `oxpinyin_testsupport::model_cache` (re-exported as `pinyin_oracle::model_cache`). |
| Owning subsystem | testsupport / oracle provisioning |
| Purpose | Locates the checksum-pinned model20 extraction and the system-table export directory; holds the model provenance constants. |
| Implementation | Rust, 337 lines (incl. 4 unit tests). Consts `MODEL20_URL`, `MODEL20_SHA256 = 59c68e89…1155`, `MODEL_DIR_ENV="PINYIN_MODEL_DIR"`, `MODEL_CACHE_ENV="PINYIN_MODEL_CACHE"`, `DEFAULT_CACHE_DIR_NAME="model20"`, `EXTRACTED_SUBDIR="extracted"`, `EXPECTED_MODEL_FILES` (18), `EXPORT_DIR_ENV="PINYIN_EXPORT_DIR"`, `DEFAULT_EXPORT_DIR="/tmp/oxpinyin-export"`; `ModelDirError {NotADirectory, Incomplete{missing}}`; fns `missing_model_files`, `model_dir_is_complete`, `verified_model_dir`, `default_extracted_dir`, `locate_model_dir` (order: PINYIN_MODEL_DIR (must be complete) → PINYIN_MODEL_CACHE/extracted → $CARGO_TARGET_DIR/model20/extracted → <workspace>/target/model20/extracted; `Ok(None)` when nothing found), `resolve_export_dir` (no existence check), `export_dir` (panics if the three DBMs are missing), `system_dbm_names` (libpinyin names `pinyin_index.bin, phrase_index.bin, bigram.db` when `oxpinyin_store::DEFAULT_STORE_IS_LIBPINYIN_DBM`, else `oxpinyin_store::default_store_file(...)`); private `workspace_root_from` (sentinels `crates/pinyin-oracle/Cargo.toml` + `docs/findings/model-provenance.md`). |
| Inputs | Env: `PINYIN_MODEL_DIR`, `PINYIN_MODEL_CACHE`, `CARGO_TARGET_DIR`, `PINYIN_EXPORT_DIR`; compile-time `CARGO_MANIFEST_DIR`. |
| Outputs | paths / `ModelDirError`. |
| Platform | portable (default export dir `/tmp/…` is a Unix-ism). |
| Consumers | pinyin-oracle (bins, benches, sentence_tail, model_fetch test), `oxpinyin-capi/benches/support`, `oxpinyin-datagen` (lib + bin), `oxpinyin-training`, `tools/model/fetch-model.sh` (must stay in lockstep). |
| State | none |
| Overlap | `DEFAULT_EXPORT_DIR` "`oxpinyin-segment` carries its own equal copy because it must not depend on this crate". `MODEL20_SHA256` equals the model component of `EXPECTED_PIN_REF` (RS-12) but is not derived from it. Backend file naming is one of the places a backend name/extension is decided (via `oxpinyin_store`). |
| Constraints | "does not read frequencies, score, or change what the decoder consumes." "Keep in lockstep with `tools/model/fetch-model.sh`." |

### RS-46 — crates/oxpinyin-testsupport/src/fixture.rs

| Field | Value |
|---|---|
| Entry point | `oxpinyin_testsupport::{FixtureDictionary, FixtureLanguageModel, FrequencyFixtureModel, FixtureError}`. |
| Owning subsystem | testsupport / fixtures |
| Purpose | In-memory `Dictionary`/`LanguageModel` doubles parsed from `fixtures/w4/mini-vocab.txt` and `mini-bigram.txt` (the F-B stand-in). `FrequencyFixtureModel` reports `has_real_unigrams() == true` to route the engine through the production window-scan path. |
| Implementation | Rust, 750 lines (incl. 10 unit tests that `include_str!` the w4 fixtures). Parser `record_fields` (TAB `key=value`, `#` comments incl. trailing field), `parse_vocab` (fields `token,keys,text,unigram`; dup token error; keys via `SyllableKey::from_text`), bigram fields `prev,next,count` (unknown token error; saturating totals). `model_cost` = exact-integer interpolation λ=1/2 via `oxpinyin_core::cost::{reduce_ratio, surprisal, UNKNOWN_COST}` with u128 `checked_*` degrading to `UNKNOWN_COST` (audit F-1/F-2). `Dictionary` impl adds `suggest_extension_tokens` (prefix walk) and `phrase_text_for_token`. |
| Inputs | `&str` only — "oxpinyin-core performs no I/O. Callers `include_str!`". |
| Outputs | — |
| Platform | portable |
| Consumers | pinyin-oracle `decode_differential`; oxpinyin-engine tests (`session_replay`, `decoding`, `session_lifecycle`, `src/session/tests.rs`, `src/nbest.rs`), engine bench `session_keystroke`; oxpinyin-core tests `scoring`, `scoring_fixture`; fuzz `fixture-model`, `session-keys`; training `eval/decode.rs`. |
| State | none |
| Overlap | Fourth TAB `key=value` splitter in the inventory (see capture.rs). |
| Constraints | Header: "Originally `oxpinyin-core/src/fixture.rs`; moved … so the production build graph carries no test doubles. See fixture-adapters.md." λ=½ is "authored, deliberately neutral", not the pinned 0.312699. Pins: vocab 90 entries, max_keys 3, 8 bigrams, `ni` order `你 尼 呢 泥 妮 拟 逆 倪 腻 溺`. Provenance enforced by `crates/oxpinyin-core/tests/fixture_provenance.rs`. |

### RS-47 — crates/oxpinyin-testsupport/src/differential.rs

| Field | Value |
|---|---|
| Entry point | `oxpinyin_testsupport::differential::{fnv1a64, Manifest, parse_manifest, locate_bin, locate_data, PinDir, parse_estimate_stdout}`. |
| Owning subsystem | testsupport (W9 trainer differentials) |
| Purpose | Shared mechanics for the env-gated differential suites against pin-built libpinyin trainer binaries (counter, emitter, lambda, segment, corpus, kmm). |
| Implementation | Rust, 199 lines. `fnv1a64` (offset `0xcbf29ce484222325`, prime `0x100000001b3`); `parse_manifest` (whitespace `key value` lines: `unigrams`, `bigrams`, `fnv1a64` hex); `locate_bin(name)` → `Some(path)` iff env var names an existing file; `locate_data(name)` → iff dir has `table.conf` and is non-empty; `PinDir::fresh(data, tag)` copies only `table.conf` + `*.table` into `<tmp>/oxpinyin-pin-<tag>-<pid>` (removed on drop), `PinDir::run(bin, args, stdin)` spawns with that cwd capturing stdout, Err on non-zero exit naming binary/status/stderr; `parse_estimate_stdout` (`token:%d lambda:%f`, `average lambda:%f`). |
| Inputs | Env var *names* are passed by callers (the `PINYIN_<TOOL>` / `PINYIN_*_DATA` family, e.g. `PINYIN_SPSEG`, `PINYIN_MERGESEQ` per w9 README). |
| Outputs | — |
| Platform | portable code; spawns Linux pin binaries. |
| Consumers | `oxpinyin-training/tests/{counter,emitter,lambda,segment}-differential.rs`, `segment-spseg-mergeseq.rs`, `oxpinyin-corpus/tests/differential.rs`, `oxpinyin-kmm/tests/differential.rs`, `oxpinyin-datagen`. |
| State | temp pin dirs; spawns child processes (no timeout, no process group). |
| Overlap | FNV-1a 64 is also what `oxpinyin-datagen` writes into `fixtures/w3/*/datagen-manifest.txt` (`fnv1a64=` per table) — same construction, separate implementation in datagen. |
| Constraints | Doc: "Everything here is inert without the `PINYIN_*` environment variables: the locators return `None` and the suites skip rather than fail" — this contradicts `docs/testing/README.md`'s no-self-skip rule as written; the suites themselves are `#[ignore]`d and panic (per README), the locator doc is stale. Copy only raw tables: "`.bin`/`.db` files in a live data dir are outputs of earlier runs". |

## Fixture sets

### RS-48 — fixtures/foundation (f-a.txt, f-c.txt)

| Field | Value |
|---|---|
| Entry point | Data; read by `capture::read_fixture` (path `CARGO_MANIFEST_DIR/../../fixtures/foundation/<name>`) and `include_str!` in `decode_differential.rs`. |
| Owning subsystem | fixtures / capture |
| Purpose | The frozen Foundation captures: F-A (15 records, parity profile `0x18a`) and F-C (46 records, one option bit varied at a time off/on against `IS_PINYIN`). The oracle side of every portable differential/taxonomy test and the provenance source of `mini-vocab.txt`. |
| Implementation | `pinyin-capture-v1` text, one LF-terminated record per line; f-a.txt 15 lines / 19,365 bytes; f-c.txt 46 lines / 35,843 bytes. Fields: `schema, pin_ref, family, case, api_sequence, input, flags, parse_return, parsed_input_length, segments, candidate_total, candidates_hex, remainder`. |
| Inputs | Produced by `tools/capture/run-capture.sh` + `tools/capture/capture.c` over the pin prefix. |
| Outputs | — |
| Platform | — |
| Consumers | `capture_replay`, `differential_replay`, `taxonomy_replay`, `decode_differential`, `live_smoke` (values quoted), `crates/oxpinyin-core/tests/fixture_provenance.rs`, `flags.rs` unit test (0x18a). |
| State | — |
| Overlap | `fixtures/w4/oracle-zhuyin-parse.txt` uses the same schema tag with extra fields (`scheme`, `zhuyin`, `api_sequence` with `pinyin_parse_more_chewings`) and segments without a completeness suffix — `capture::read_fixture` would reject those segments; it is read by `crates/oxpinyin-core/tests/zhuyin_parse_parity.rs` instead. |
| Constraints | Frozen: `docs/testing/capture-fixtures.md` (SHA-256 at pin 074a2219: f-a `6690f849…`, f-c `1712555f…`). `api_sequence` field is ignored by the Rust reader. F-C's `force-tone` pair "deliberately records no observed output change". |

### RS-49 — fixtures/w3 (README.md, pin-ref.txt, fixtures.sha256, db/, kct/, tkt/, content .bin)

| Field | Value |
|---|---|
| Entry point | Data directory; runtime/C-ABI suites open `fixtures/w3/<DEFAULT_STORE_EXT>` (`db`, `kct`, or `tkt`) as a complete libpinyin system data directory. |
| Owning subsystem | fixtures |
| Purpose | The `--mini` compile of the pinned model20 per storage backend, plus truncated custom-content `.bin` tables (art…technology) at the top level for the custom-content loader tests. |
| Implementation | Per backend dir: `pinyin_index.bin`, `phrase_index.bin`, `bigram.db`, `punct.bin`, `addon_{pinyin,phrase}_index.bin` (DBM containers; sizes differ wildly: db 8–36 KB, kct 400 KB–6.3 MB, tkt 529 KB–4.2 MB), chunk files `gb_char.bin` (167,681 B), `gbk_char.bin` (70,241 B), `opengram.bin`, `merged.bin` (27 B), `art.bin` (151 B) byte-identical across backends, `table.conf` (29 lines; `binary format version:7`, `model data version:14`, `lambda parameter:0.312699`, `database format:BerkeleyDB|KyotoCabinet|Tkrzw`, default + 12 addon rows), `datagen-manifest.txt` (`oxpinyin-datagen-manifest-v1`, `pin_ref=model20-59c68e89…`, `backend=db|kct|tkt`, `producer=oxpinyin-datagen@0.1.0`, `table=<file> records=<n> fnv1a64=<hex>` × 12; chunk hashes equal across backends, DBM hashes differ). `pin-ref.txt`: one line `pin_ref=<EXPECTED_PIN_REF>` (tkrzw). `fixtures.sha256`: 53 entries (`sha256sum` format with `fixtures/w3/` prefix); regen one-liner in README. 13 top-level content `.bin` (1–3 KB each). |
| Inputs | Generated by `oxpinyin-datagen compile --mini --backend <peer> --out-dir fixtures/w3/<ext>` from the model cache (`system::MINI_KEYS`, `addon::MINI_ART_KEYS`); content bins by `python3 tools/generate_w3_fixtures.py` (default data `~/.local/opt/pinyin-oracle/lib/libpinyin/data`). |
| Outputs | — |
| Platform | — |
| Consumers | `oxpinyin-capi/src/test_support.rs`, `tests/abi/common.rs`, `fuzz/capi_commands.rs` (system dir = `fixtures/w3/<DEFAULT_STORE_EXT>`), `oxpinyin-datagen/tests/fixtures_identity.rs` (ignored; reproduces the compiled backend's dir), `crates/oxpinyin-training/tests/segment-spseg-mergeseq.rs` (W3 phrase_index), `verify-nightly.yml` fuzz-soak seeds `dict-loader` corpus from `fixtures/w3/*.bin`; `.gitignore` excludes `fixtures/w3/*/interpolation2.text`. |
| State | — |
| Overlap | `table.conf` is the third `table.conf` reader family (datagen writes it; `oxpinyin_data::parse_table_conf_lambda` / `set_lambda_from_table_conf` read `lambda parameter`; `testsupport::locate_data` only checks existence). |
| Constraints | DBM container bytes "depend on the writing library's version" (so only records/chunk bytes are identity-checked). `db/` is the default build's flavour since 2026-09-20. `fixtures.sha256` is not consumed by any Rust test found (it is a manual/regen aid; README gives the command). |

### RS-50 — fixtures/w4/f-d-session.txt (session replay scenarios)

| Field | Value |
|---|---|
| Entry point | `include_str!("../../../fixtures/w4/f-d-session.txt")` in `crates/oxpinyin-engine/tests/session_replay.rs` (`SCENARIOS`). Not read by pinyin-oracle. |
| Owning subsystem | fixtures / engine |
| Purpose | The authored F-D stand-in: 9 recorded key-sequence scenarios whose expected candidates are the pin's real F-A lists restricted to the mini vocabulary. |
| Implementation | 84 lines, `# pinyin-session-replay-v1`. **Grammar (exact, as parsed by `session_replay.rs::scenarios`/`run`):** line-oriented UTF-8; a line is trimmed at the end (`trim_end`), then skipped if empty or starting with `#`; otherwise split on TAB into `key=value` fields (fields without `=` are dropped by `filter_map(split_once('='))`). The **first field** decides: `scenario=<name>` opens a new scenario; `step=<kind>` appends the whole field list to the current scenario (a step before any scenario panics "a step follows its scenario"); any other first key panics "unrecognised replay line". Step kinds and their required fields (looked up by name with `value(step, name)`, extra fields ignored): `type text=<ascii>` → one `Session::process_key(KeyInput::character(c))` per char; `key name=<enter|escape|space|backspace>` → `process_key(KeyInput::plain(LogicalKey::…))`, other names panic; `select index=<usize>` → `Session::select(index)`; `select-text text=<phrase>` → position of the first candidate whose text equals `<phrase>` (panic listing offers if absent) then `select`; `expect-preedit text=<text>` → `assert_eq!(preedit().text(), text)` **plus** the span law (spans contiguous from 0, covering `text.len()` exactly); `expect-candidate index=<n> text=<phrase>` → `candidates()[n].text() == phrase` (message dumps offers); `expect-absent text=<phrase>` → no candidate has that text; `expect-empty` (no fields) → `!is_composing() && preedit().is_empty()`; `commit text=<text>` → `Session::commit()? == text`. Any other kind panics. Expectations are plain `assert`/`assert_eq` with `"{scenario} step {n} ({kind})"` in the message; process_key/select/commit `Err`s panic with the same prefix. Each scenario runs in a **fresh** session (`session()` builds `Session<FixtureDictionary, FixtureLanguageModel>` from mini-vocab/mini-bigram with `EmptyConfigSource`, `StoragePaths::new("user")`, after asserting the vocab holds `zhuan, ni, ni,hao, xi,an, chang,an, zhong,guo`). Tests: `the_scenario_file_is_readable` (≥8 scenarios, none empty), `every_recorded_scenario_replays`, `replaying_twice_gives_the_same_answer` (each scenario twice), plus F-E-02, F-E-12, `arbitrary_characters_never_panic` which are hand-written, not fixture-driven. Scenarios: `guess-choose-commit`, `guess-choose-reguess`, `cross-segmentation-choice`, `incomplete-tail`, `apostrophe-boundary`, `backspace-undoes-a-choice`, `space-accepts-the-first-candidate`, `f-e-12-zhuan-totality`, `unknown-input-falls-back`. |
| Inputs | — |
| Outputs | — |
| Platform | portable (runs on Linux/macOS/Windows per `session-replay.md`). |
| Consumers | `oxpinyin-engine/tests/session_replay.rs`; `docs/findings/session-replay.md` (frozen SPEC for W4-T4b); `docs/testing/upstream-test-coverage.md:83` (the `simulate.c` analog). |
| State | none |
| Overlap | Format is the same TAB `key=value` family as capture/vocab fixtures; the parser is a fifth independent splitter. `select` vs `select-text`: "a candidate's index is a property of the current ranking". |
| Constraints | Header: "AUTHORED, not oracle output". Comment in `guess-choose-reguess`: "Exact ranks move with ScoringConfig" (hence `select-text`). A real F-D capture "would replace the file without changing the harness". |

### RS-51 — fixtures/w4 oracle goldens (oracle-candidates.txt, oracle-candidate-structure.txt, oracle-paths.txt)

| Field | Value |
|---|---|
| Entry point | Data; read by path (`repo_root()/fixtures/w4/…`) or `include_str!` (oracle-paths). |
| Owning subsystem | fixtures |
| Purpose | Portable copies of the pin's answers over the W2 corpus: depth-10 candidate lists (97,442 triples over 10,037 inputs with candidates; 10,312 distinct inputs), the same with `type`/`nbest_index`, and the selected segmentation for 10,459 inputs. |
| Implementation | Text, `#` header lines then TAB rows. oracle-candidates.txt 97,452 lines / 2.07 MB (`input\trank\ttext`); oracle-candidate-structure.txt 97,455 lines / 3.92 MB (`…\ttype\tnbest_index`); oracle-paths.txt 10,466 lines / 950 KB (`escaped_input\tconsumed\tpath`). Schema tags `pinyin-oracle-candidates-v1`, `pinyin-oracle-candidate-structure-v1`, `pinyin-oracle-path-v1`; all stamped `# pin_ref=<EXPECTED_PIN_REF>` (tkrzw). |
| Inputs | Generated by RS-20, RS-21, RS-19. |
| Outputs | — |
| Platform | — |
| Consumers | `candidate_structure`, `graph_paths`, `decode_differential`, `real_tables_integration` (ignored freshness), bins `corpus-tail`, `parity-sweep`, `parity-worst`; docs `goldens-and-pins.md`. |
| State | — |
| Overlap | Three readers of the candidates file in bins + one in tests. |
| Constraints | Regeneration ~3 h for candidates. Readers drop lines starting with `#`, so corpus inputs beginning with `#` (junk stratum) are invisible to them; `graph_paths` uses TAB presence instead. |

### RS-52 — fixtures/w4 sentence-surface goldens (oracle-sentence-surface.txt, oracle-sentence-tie-order.txt)

| Field | Value |
|---|---|
| Entry point | Data; `sentence_tail::load_fixture`, `real_tables_integration::load_sentence_fixture`, `sentence_surface_parity::load_tie_cases`, `tools/bisection/gcov_measure.py`. |
| Owning subsystem | fixtures |
| Purpose | W14 sentence surface for a 500-input sample (+anchors) and the #747 supplementary native-tail tie fixture over 8,592 distinct inputs. |
| Implementation | oracle-sentence-surface.txt: 510 lines / 67 KB; `# pin_ref=` (tkrzw), `# sample=507`; rows `input\ttrue|false\t<\x01-joined sentences>\t<\x01-joined n|N|a/?/nbest/text rows>`. oracle-sentence-tie-order.txt: 8,614 lines / 784 KB; 22 `#` header lines incl. `# pin_ref=…+dbm-bdb`, `# format=sentence-tie-order-v1`, `# selection_nbest_rs_sha256=…`, `# selection_main=3391eda…`, counts, profile (`options=0x18a; sort=0x1e; offset=0`), pin source citation (`phonetic_lookup.h:174-178`), regen command (`python3 tools/bisection/contract-diff.py bdb "$PINYIN_ORACLE_PREFIX" … --regenerate-sentence-ties …`); 9 TAB columns `input, occurrences, native_costs(csv), guessed(0|1), pool(\x01-joined), pin_indexed(csv pool ids), pin_NBEST(rank:id csv), residual_indexed, residual_NBEST`. |
| Inputs | Surface file by RS-22; tie file by `tools/bisection/contract-diff.py` (Python, GDB-read native costs). |
| Outputs | — |
| Platform | — |
| Consumers | RS-16, RS-26, RS-38, RS-39; `tools/bisection/axis-c-replacement.json` pins the surface file's sha256 `d119f3a7…`; `docs/runbooks/benches.md` valgrind recipes pass it as `--sentence-fixture`/`--train-fixture`. |
| State | — |
| Overlap | Two pin_refs in play: surface = tkrzw, tie-order = **bdb** (the reference cell). |
| Constraints | Tie file: "Regeneration preserves the historical selection; it does not reselect inputs on a changed main." Residual expectations only for the six §12 rows; test enforces that no new ones appear. |

### RS-53 — fixtures/w4 fixture-double data and config layers (mini-vocab.txt, mini-bigram.txt, config-system.txt, config-user.txt, oracle-zhuyin-parse.txt)

| Field | Value |
|---|---|
| Entry point | `include_str!` from testsupport `fixture.rs` tests, engine tests/benches, core tests, fuzz targets; config files read by `oxpinyin-engine/src/config.rs` tests; zhuyin file by `oxpinyin-core/tests/zhuyin_parse_parity.rs`. |
| Owning subsystem | fixtures / testsupport |
| Purpose | mini-vocab (`pinyin-fixture-vocab-v1`, 90 records `token,keys,text,unigram`; header notes derivation from f-a/f-c at the *old* pin 0c5e80e, `unigram = 1000 - 100*rank`), mini-bigram (`pinyin-fixture-bigram-v1`, 8 authored `prev,next,count` rows), config-system/config-user (`key=… type=b|i|x|s value=…` drop-in layers per `docs/findings/config-layering.md`), oracle-zhuyin-parse (26 `pinyin-capture-v1` records, family `ZH`, with `scheme`, `zhuyin`, segments `ni3@0:3` without completeness). |
| Implementation | mini-vocab 97 lines / 4.2 KB; mini-bigram 16 lines; config-system 5 lines; config-user 7 lines; oracle-zhuyin-parse 26 lines / 22 KB. |
| Inputs | — |
| Outputs | — |
| Platform | — |
| Consumers | 16 files (grep list in Architecture notes), incl. `fuzz/session_keys.rs`, `pinyin-oracle/tests/decode_differential.rs`, `oxpinyin-core/tests/fixture_provenance.rs` (enforces vocab provenance). |
| State | — |
| Overlap | — |
| Constraints | "carry no parity claim" (bigram header). mini-vocab's header still cites pin `2.11.91-0c5e80e` though content is unchanged across the re-freeze. |

### RS-54 — fixtures/w9 (training fixtures)

| Field | Value |
|---|---|
| Entry point | Data for the oxpinyin-training/-corpus/-kmm differential suites (not pinyin-oracle). |
| Owning subsystem | fixtures / training |
| Purpose | `segmenter-han.txt` (111 synthetic Han lines), `segmenter-ngseg.txt` (233 lines, pin `ngseg` stdout `token text`), `segmenter-spseg-w3.txt` (144), `mergeseq-input.txt` (5) / `segmenter-mergeseq-w3.txt` (5), `counter-ngram.manifest` / `interpolation2.manifest` / `lambda-estimate.manifest` (`unigrams N`, `bigrams N`, `fnv1a64 hex` goldens parsed by `testsupport::parse_manifest`), `corpus-xml.xml` (165 lines, 3 zhwiki pages, CC BY-SA 4.0) / `corpus-sample.txt` (16 lines). |
| Implementation | text; README 70 lines. |
| Inputs | produced by pin tools `ngseg`, `spseg`, `mergeseq`, `gen_ngram`, `estimate_interpolation`, `export_interpolation` and by the Rust ports. |
| Outputs | — |
| Platform | — |
| Consumers | `oxpinyin-training/tests/{counter,emitter,lambda,segment}-differential.rs`, `segment-spseg-mergeseq.rs`, `oxpinyin-corpus/tests/{differential,invariants}.rs`, `oxpinyin-kmm/tests/{differential,kmm_pipeline}.rs`, `oxpinyin-datagen/src/lib.rs`, `tools/oracle/run-differentials.sh`, `nightly-fixture-differentials.sh`, `docs/runbooks/goldens-and-pins.md`. |
| State | — |
| Overlap | FNV-1a manifests share the construction with w3 `datagen-manifest.txt`. |
| Constraints | "No model bytes are tracked here." Live cross-checks need `PINYIN_SPSEG`, `PINYIN_MERGESEQ` and the full oracle data dir. |

### RS-55 — fixtures/datagen-toned (toned mini model)

| Field | Value |
|---|---|
| Entry point | Model directory (18 files: 17 `.table` + `interpolation2.text`) in the model20 layout. |
| Owning subsystem | fixtures / packaging (datagen drop-in) |
| Purpose | A by-eye-sized model carrying tone digits (`ba`,`ba1`,`ba3`,`ba4`; `ni3'hao3`/`ni'hao` both token 16777224; `a1`; `lv4`; `er4'huang2`), so libpinyin's `gen_binary_files`/`import_interpolation`/`gen_unigram` and `oxpinyin-datagen` can be compared field by field. |
| Implementation | README 22 lines; tables of 2–5 lines (e.g. art.table 2 lines, punct.table 3 lines, interpolation2.text 13 lines). |
| Inputs | hand-authored. |
| Outputs | — |
| Platform | — |
| Consumers | `tools/datagen/libpinyin-drop-in-differential.sh`, `tools/oracle/nightly-fixture-differentials.sh` (`model=$repo/fixtures/datagen-toned`, GREEN toned tier in `verify-nightly.yml`), `crates/oxpinyin-datagen/tests/libpinyin_parity.rs`, `oxpinyin-datagen/src/lib.rs`. |
| State | — |
| Overlap | Inventory checked by `EXPECTED_MODEL_FILES` (RS-45). |
| Constraints | "Neither side reads the other's output." |

### RS-56 — tests/parity/corpus/inputs (the W2 parity corpus)

| Field | Value |
|---|---|
| Entry point | Nine files `01-single-syllable.txt` … `09-edge.txt`, one input per LF line, read via `corpus::Stratum::parse_file_bytes` (preserves the empty first line of 09-edge). |
| Owning subsystem | fixtures |
| Purpose | Inputs only (no expectations) for every differential: 10,465 lines (405/2000/2500/1500/1200/1200/800/800/60), printable ASCII `0x21..=0x7e`. |
| Implementation | text, 158 KB total; generated by `corpus::generate()` via `parity-corpus`. |
| Inputs | — |
| Outputs | — |
| Platform | — |
| Consumers | RS-30 (byte-identity), RS-22/RS-23/RS-24/RS-25/RS-38, `benches/support::load_w2_inputs`, `tools/bisection/gcov_measure.py`, `tools/bisection/axis-c-replacement.json`, ci.yml path filter `tests/parity/**`. |
| State | — |
| Overlap | — |
| Constraints | Frozen in `docs/testing/parity-corpus.md` (SHA-256 per file). "Readers must therefore preserve an empty first line rather than skipping blanks." Cross-strata duplicates intentional (10,312 distinct). |

## Fuzz workspace (`fuzz/`)

### RS-57 — fuzz/Cargo.toml (+ Cargo.lock, corpus/)

| Field | Value |
|---|---|
| Entry point | Separate cargo workspace (`[workspace] members = ["."]`), package `oxpinyin-fuzz` 0.0.0, edition 2021, `cargo-fuzz = true`. Run: `cargo +nightly-2026-08-01 fuzz run <target> -- -max_total_time=180 -timeout=15 -max_len=4096 -print_final_stats=1` (verify-nightly). |
| Owning subsystem | fuzz |
| Purpose | Ten libFuzzer targets over core/data/engine/user/capi/testsupport. |
| Implementation | TOML 106 lines; deps `libfuzzer-sys = "=0.4.13"`, path deps `oxpinyin-core`, `oxpinyin-data`, `oxpinyin-capi` (`features = ["fuzz-api"]`), `oxpinyin-testsupport`, `oxpinyin-engine`, `oxpinyin-user`. `[lints.rust] unsafe_code = "deny"` (not forbid: capi-commands carries scoped unsafe). Ten `[[bin]]` entries with `test = false, doc = false, bench = false`: `parser, dict-loader, fixture-model, double-pinyin, capi-commands, phrase-library, table-conf, chewing-parser, session-keys, user-store-ops`. `Cargo.lock` 650 lines (Dependabot `directory: "/fuzz"`). `fuzz/corpus/`: only `parser/zhuan` (5 B) and `session-keys/913164e4…` (54 B) are tracked (`.gitignore`: `/fuzz/corpus/*` with `!parser/**`, `!session-keys/**`); CI restores/saves the rest via actions/cache and seeds `dict-loader` from `fixtures/w3/*.bin`. |
| Inputs | Backend rides the workspace default (bdb) via path deps. |
| Outputs | `fuzz/artifacts/<target>/crash-*` on failure (uploaded as `fuzz-crash-artifacts`). |
| Platform | Linux (ASan/LSan, `setarch -R`), nightly toolchain. |
| Consumers | `verify-nightly.yml` `fuzz-soak` (list from `cargo fuzz list`); `ci.yml` `lint` (`cargo fmt --manifest-path fuzz/Cargo.toml --check`) and `deny` (`cargo deny --locked --manifest-path fuzz/Cargo.toml check`); `docs/findings/verify-nightly.md`. |
| State | per-process temp dirs/files per target (see below). |
| Overlap | — |
| Constraints | Workspace does not inherit root lints. "`--locked` pins both root graphs … A root bump that … does not refresh fuzz/Cargo.lock fails --locked. That failure is the gate." |

### RS-58 — fuzz/fuzz_targets/parser.rs

| Field | Value |
|---|---|
| Entry point | `cargo fuzz run parser`. |
| Owning subsystem | fuzz |
| Purpose | `FullPinyinParser::parse` is deterministic on arbitrary bytes; `TooManyAlternatives{limit}` always reports `MAX_PARSE_RESULTS`. |
| Implementation | Rust, 16 lines; `oxpinyin_core`. |
| Inputs | bytes; seed corpus `fuzz/corpus/parser/zhuan`. |
| Outputs | libFuzzer. |
| Platform | Linux nightly. |
| Consumers | fuzz-soak; cited by `docs/testing/upstream-test-coverage.md`, `.kiro/specs/foundation/tasks.md`. |
| State | none |
| Overlap | — |
| Constraints | — |

### RS-59 — fuzz/fuzz_targets/dict_loader.rs

| Field | Value |
|---|---|
| Entry point | `cargo fuzz run dict-loader`. |
| Owning subsystem | fuzz |
| Purpose | `oxpinyin_data::ContentTable::load` returns on every input and is deterministic (len/magic/version/every record) — audit F-3 class. |
| Implementation | Rust, 30 lines. |
| Inputs | bytes; CI seeds from `fixtures/w3/*.bin`. |
| Platform | Linux nightly. |
| Consumers | fuzz-soak. |
| State | none |
| Overlap | — |
| Constraints | — |

### RS-60 — fuzz/fuzz_targets/fixture_model.rs

| Field | Value |
|---|---|
| Entry point | `cargo fuzz run fixture-model`. |
| Owning subsystem | fuzz / testsupport |
| Purpose | `FixtureDictionary::parse` / `FixtureLanguageModel::parse` are total and deterministic on hostile UTF-8 (first half = vocab, second = bigrams); audit F-1/F-2 saturation ingress. |
| Implementation | Rust, 35 lines; returns early on non-UTF-8. |
| Platform | Linux nightly. |
| Consumers | fuzz-soak. |
| State | none |
| Overlap | — |
| Constraints | Only `is_ok()` equality is asserted, not value equality. |

### RS-61 — fuzz/fuzz_targets/double_pinyin.rs

| Field | Value |
|---|---|
| Entry point | `cargo fuzz run double-pinyin`. |
| Owning subsystem | fuzz |
| Purpose | Every `DoublePinyinScheme` (Zrm, Ms, Ziguang, Abc, Pyjj, Xhe, Customized) parses totally and deterministically; `set_scheme(Customized)` is rejected with state preserved (contract shared with the C ABI). |
| Implementation | Rust, 38 lines; `allow_incomplete` from bit 0 of the first byte. |
| Platform | Linux nightly. |
| Consumers | fuzz-soak. |
| State | none |
| Overlap | — |
| Constraints | — |

### RS-62 — fuzz/fuzz_targets/chewing_parser.rs

| Field | Value |
|---|---|
| Entry point | `cargo fuzz run chewing-parser`. |
| Owning subsystem | fuzz |
| Purpose | G1 gap: `ZhuyinParser::with_scheme(s).parse(data, use_tone, allow_incomplete)` over 8 schemes × 2 × 2 is deterministic and obeys the `test_parser2` span law (ascending non-overlapping non-empty spans within input, tone ≤ 5, `consumed` between last span end and input length). `StandardDvorak` excluded (upstream setter aborts). |
| Implementation | Rust, 79 lines. |
| Platform | Linux nightly. |
| Consumers | fuzz-soak. |
| State | none |
| Overlap | Same law as `crates/oxpinyin-core/tests/scheme_parsers.rs`. |
| Constraints | — |

### RS-63 — fuzz/fuzz_targets/phrase_library.rs

| Field | Value |
|---|---|
| Entry point | `cargo fuzz run phrase-library`. |
| Owning subsystem | fuzz |
| Purpose | `PhraseLibrary::open` over a chunk file whose header (`len` LE u32 + `chunk_checksum(payload)` LE) is synthesised around the fuzz payload, so the fuzzer reaches the sub-index/item decoders; every accessor walk is in-bounds and deterministic. |
| Implementation | Rust, 66 lines; writes `<tmp>/oxpinyin-fuzz-phrase-library-<pid>` per input (overwritten, never removed). |
| Platform | Linux nightly. |
| Consumers | fuzz-soak. |
| State | one scratch file per process. |
| Overlap | — |
| Constraints | `payload.len()` must fit u32 (`expect`). |

### RS-64 — fuzz/fuzz_targets/table_conf.rs

| Field | Value |
|---|---|
| Entry point | `cargo fuzz run table-conf`. |
| Owning subsystem | fuzz |
| Purpose | `oxpinyin_data::parse_table_conf_lambda` is total/deterministic on UTF-8 text; a parsed λ is in `[0,1]`. |
| Implementation | Rust, 23 lines. |
| Platform | Linux nightly. |
| Consumers | fuzz-soak. |
| State | none |
| Overlap | — |
| Constraints | `None` is a legitimate answer (decoder falls back to pinned λ). |

### RS-65 — fuzz/fuzz_targets/session_keys.rs

| Field | Value |
|---|---|
| Entry point | `cargo fuzz run session-keys`. |
| Owning subsystem | fuzz / engine |
| Purpose | G5 gap: command stream (`byte % 7`: character key from payload byte (Latin-1), plain `LogicalKey` from `command >> 3 % 11`, `type_pinyin` of ≤64 bytes, `candidates_at(min(offset,len))`+`select`, `guess_sentence`+`sentence_text(0)`, `candidates_ending_at`, `reset` with initial-state law) over a fresh `Session<FixtureDictionary, FixtureLanguageModel>` per input; after every command the `Preedit` span law holds; `candidates_at(len+64)` must be `Err(EngineError::LookupOffsetOutOfRange)`. |
| Implementation | Rust, 184 lines; `include_str!` of mini-vocab/mini-bigram. |
| Inputs | bytes; tracked seed `fuzz/corpus/session-keys/913164e4…`. |
| Platform | Linux nightly. |
| Consumers | fuzz-soak. |
| State | none |
| Overlap | Command-stream shape mirrors `capi_commands.rs`. |
| Constraints | — |

### RS-66 — fuzz/fuzz_targets/user_store_ops.rs

| Field | Value |
|---|---|
| Entry point | `cargo fuzz run user-store-ops`. |
| Owning subsystem | fuzz / user |
| Purpose | Arbitrary mutation interleavings over `oxpinyin_user::UserStore::create_standalone` (`byte % 8`: add_phrase (1–15 chars, keys from a 380-syllable table), add with count, the typed `InvalidPhrase` refusals (16 chars / key mismatch), remove tracked + absurd token, `set_bigram_count`+`observe_selection`+`observe_predicted`, `mask_out`, `save` clears `is_modified`); after every op the totals law (`bigram_total(prev) == Σ bigram_successors`) and the phrase-index inverse law hold. |
| Implementation | Rust, 278 lines; `static LAST_PATH: Mutex<Option<PathBuf>>`, `PATH_SERIAL: AtomicU64`; store at `<tmp>/oxpinyin-fuzz-user-store-<pid>-<n>.store`, previous input's store (and `-lock` sidecar, file or dir) removed at the start of the next input — "at most one orphan remains per process". |
| Platform | Linux nightly; backend = workspace default (bdb). |
| Consumers | fuzz-soak. |
| State | per-input temp store. |
| Overlap | — |
| Constraints | Removal semantics cite `pinyin.cpp:3759` (only fully indexed readings removable). |

### RS-67 — fuzz/fuzz_targets/capi_commands.rs

| Field | Value |
|---|---|
| Entry point | `cargo fuzz run capi-commands`. `#![allow(unsafe_code)]` (the documented exception). |
| Owning subsystem | fuzz / abi |
| Purpose | Stateful session fuzzer over oxpinyin's C ABI (`pinyin_capi::fuzz_api`, the libchewing `fuzzer.rs` shape): one process-wide context+instance over `fixtures/w3/<DEFAULT_STORE_EXT>` and a per-process temp user dir; per input `pinyin_reset` then `byte % 11` commands: reset, `parse_more_full_pinyins`, `guess_sentence`+`get_sentence`(free), candidate walk (`get_n_candidate`, ≤8 `get_candidate`/`get_candidate_string`/`choose_candidate`), `get_parsed_input_length`, `train(0)`, `set_double_pinyin_scheme` (incl. invalid), import iterator trio (begin/add/end paired), export iterator trio (begin/walk+free/end), `clear_constraint`, second parse. No per-call return assertions; a finding is any abort/panic/sanitizer report. |
| Implementation | Rust, 298 lines; declares libc `free` and `atexit`; `teardown_at_exit` frees instance + context so LSan (registered before main, run in reverse atexit order) sees no leak — long comment on why `__lsan_do_leak_check()` was declined. |
| Inputs | `fixtures/w3/<DEFAULT_STORE_EXT>` (root dir yields NULL init — "2026-09-04 soak"); `<tmp>/oxpinyin-fuzz-capi-<pid>.d`. |
| Platform | Linux nightly (ASan+LSan). |
| Consumers | fuzz-soak; `docs/testing/upstream-test-strategies.md:138`. |
| State | `static SESSION: OnceLock<Session>`, `static mut TEARDOWN_*` raw pointers; temp user dir never removed. |
| Overlap | Same symbol names as pinyin-oracle `ffi.rs` but a different library (oxpinyin-capi, not libpinyin). Fix placement precedent in AGENTS.md ("capi-commands fuzz leak fix … belonged on #502"). |
| Constraints | Mispaired iterator lifecycles are UB by contract (audit F-6), so begin/end always pair within one command. Strings capped at 64 bytes, NUL-truncated. |

## Count

67 records: RS-01 RS-02 RS-03 RS-04 RS-05 RS-06 RS-07 RS-08 RS-09 RS-10 RS-11 RS-12 RS-13 RS-14 RS-15 RS-16 RS-17 RS-18 RS-19 RS-20 RS-21 RS-22 RS-23 RS-24 RS-25 RS-26 RS-27 RS-28 RS-29 RS-30 RS-31 RS-32 RS-33 RS-34 RS-35 RS-36 RS-37 RS-38 RS-39 RS-40 RS-41 RS-42 RS-43 RS-44 RS-45 RS-46 RS-47 RS-48 RS-49 RS-50 RS-51 RS-52 RS-53 RS-54 RS-55 RS-56 RS-57 RS-58 RS-59 RS-60 RS-61 RS-62 RS-63 RS-64 RS-65 RS-66 RS-67

## Architecture notes

Observation (what the code does) unless marked *interpretation*.

**Duplicated logic**

- Oracle prefix search + `oracle-pin.txt` `pin_ref` check: `build.rs::{locate_prefix,verify,library_dir}` and `pin.rs::{OraclePrefix::locate, PinManifest::verify_pin_ref_and_dbm}` — same env order (`PINYIN_ORACLE_PREFIX` → `PKG_CONFIG_PATH` walked up two parents → `$HOME/.local/opt/pinyin-oracle`), same `PINYIN_BENCH_DBM` relaxation via the shared `include!`d `pin_ref.rs::bench_pin_ref`. Intentional ("so link time and run time agree"); `DEFAULT_PREFIX_SUFFIX` literal exists in both build.rs and lib.rs. `dbm_bench.rs::bench_prefix` bypasses the chain and requires `PINYIN_ORACLE_PREFIX` alone.
- `oracle-pin.txt` parsers: `build.rs::verify` (first `pin_ref=` line), `pin.rs::PinManifest::parse` (full key=value, repeated-key fatal), plus bash `tools/capture/run-capture.sh::manifest_value` (schema check `pinyin-oracle-v1`).
- Real-table session open (`SystemDictionary::open(dir)` → `BigramLanguageModel::open(dir/bigram.db, libraries)` → `set_lambda_from_table_conf(dir/table.conf)` → `Session::new(&EmptyConfigSource, StoragePaths::new("user"), …)`): `sentence_tail.rs::open_session_from_env`, `corpus_tail.rs::open_env`, `parity_sweep.rs::main`, `parity_worst.rs::main`, `benches/support::load_real_tables`, `scan_perf.rs::load_scoring_tables` — six copies.
- Export-dir three-table existence check (`SystemDbm::{PinyinIndex,PhraseIndex,Bigram}.file_name()`): `sentence_tail::export_dir` (Option), `corpus_tail::export_dir` (Result), `parity_sweep::main` (lists missing, exit 2), `parity_worst::main` (PinyinIndex only), `testsupport::model_cache::export_dir` (panics; uses `system_dbm_names()` instead of `SystemDbm`).
- `oracle-candidates.txt` → `BTreeMap<input, Vec<text>>` loader: `corpus_tail::load_fixture`, `parity_sweep::load_fixture`, `parity_worst::main`, `real_tables_integration::load_candidate_fixture` (the last also validates rank continuity and the `# pin_ref=` header).
- Corpus load from committed files (`corpus::generate()` for names + `Stratum::parse_file_bytes`): `corpus_tail::run`, `parity_sweep::load_corpus`, `parity_worst::main`, `oracle_sentence_surface::real_main`, `real_tables_integration::load_w2_corpus`, `benches/support::load_w2_inputs` (sorts `read_dir` instead).
- TAB `key=value` line splitters: `capture::CaptureRecord::parse_line`, `oracle_paths::fields`, `decode_differential::field`, `testsupport::fixture::record_fields`, `oxpinyin-engine/tests/session_replay.rs::scenarios`.
- Segment-notation (`text@begin:end:complete|partial`) parsers: `capture::parse_segment` (canonical, sentinel-aware), `graph_paths::parse_path`, `decode_differential::corpus` (keeps key + begin only).
- Our-path renderer: `differential::render_our_path` ≡ `oracle_paths::render`.
- Candidate-type letter mapping (`n/N/a/?`, port side adds `f`): `oracle_sentence_surface::render_row`, `real_tables_integration` sentence loader, `sentence_tail::render_row`.
- `repo_root()` = `CARGO_MANIFEST_DIR/../..`: `sentence_tail.rs`, `corpus_tail.rs`, `parity_sweep.rs`, `parity_worst.rs`, `candidate_structure.rs`, `parity_corpus.rs`, `real_tables_integration.rs`, `benches/support`; `model_fetch.rs` and `testsupport::model_cache` walk up to a sentinel file instead (different sentinels: `tools/model/fetch-model.sh` vs `docs/findings/model-provenance.md`).
- `corpus_cases()` (case id `"{file}:{index}"`): `parity_diff.rs` and `differential_live.rs`.
- `oracle()` panicking locator helper: `live_smoke.rs`, `differential_live.rs`.
- Fresh-temp-dir guards with `remove_dir_all` on drop: `live::FreshUserDir`, `dbm_bench::BenchRoot`, `model_fetch::Scratch`, `testsupport::differential::PinDir`, `scan_perf::TempStorePath` (file).
- Stratum count table: `corpus.rs` unit test and `tests/parity_corpus.rs`.

**Environment variables (name → readers)**

- `PINYIN_ORACLE_PREFIX` — build.rs, pin.rs (`PREFIX_ENV`), dbm_bench.rs (required); runbooks/tools default `$HOME/.local/opt/pinyin-oracle`; `tools/bisection/run-all.py` uses alias `OXPINYIN_ORACLE_ROOT` for the same root.
- `PKG_CONFIG_PATH`, `HOME` — build.rs, pin.rs (prefix discovery).
- `PINYIN_BENCH_DBM` (`kc|bdb`) — build.rs, pin.rs, live_smoke.rs; forbidden with `CI`.
- `CI` — build.rs only.
- `CARGO_FEATURE_ORACLE_FFI` — build.rs.
- `PINYIN_MODEL_DIR`, `PINYIN_MODEL_CACHE`, `CARGO_TARGET_DIR` — testsupport::model_cache (`locate_model_dir`); `PINYIN_MODEL_DIR` is *documented* as required by corpus-tail/sentence-tail/sentence_surface_parity but those code paths only open the export dir.
- `PINYIN_EXPORT_DIR` (default `/tmp/oxpinyin-export`) — testsupport::model_cache (`resolve_export_dir`, `export_dir`); every real-table bin/test/bench; `tools/oracle/run-differentials.sh` exports it.
- `PINYIN_MODEL_ARCHIVE` — tests/model_fetch.rs (optional local archive).
- `PARITY_SWEEP_FULL` — parity_sweep.rs.
- `DBM_BENCH_POPULATED` — dbm_bench.rs child process.
- `PATH` — shimmed by model_fetch.rs (`run_without_git`).
- `PINYIN_<TOOL>` / `PINYIN_*_DATA` (e.g. `PINYIN_SPSEG`, `PINYIN_MERGESEQ`) — passed by name to `testsupport::differential::{locate_bin, locate_data}` from the training suites.
- `PINYIN_ORACLE_DBM` — not read by any Rust file here (shell cell selector).

**Exit-code / pass-skip conventions**

- Bins: 0 success, 1 runtime error, 2 usage or feature-off or missing tables (`parity-corpus`, `parity-diff`, `oracle-candidates`, `oracle-candidate-structure`, `oracle_sentence_surface`, `parity-sweep`, `parity-worst`); `corpus-tail`/`sentence-tail`/`oracle-paths` use 0/1 only; `dbm_bench` 2 on missing prefix/unknown op, 1 on child failure. No 77.
- Tests: libtest. Ignored-with-reason tests: `candidate_structure::structure_fixture_is_fresh`, `real_tables_integration::{oracle_candidates_fixture_is_fresh, sentence_surface_fixture_is_fresh}` (all three also feature-gated; need oracle), `sentence_surface_parity::{sentence_surface_matches_the_declared_residual, sentence_surface_detects_per_step_cost_shift, sentence_surface_matches_supplementary_tie_order}` (need export tables, no FFI). Whole files gated on `oracle-ffi` (need oracle, not ignored): `live_smoke`, `differential_live`, `export_seam`. Residual self-skips despite the README rule: `model_fetch::pinned_archive_extracts_and_second_run_skips` (returns when no archive), `real_tables_integration::load_w2_corpus` (returns None inside an ignored test), `testsupport::differential` doc ("the suites skip rather than fail").

**Places that parse pinned artefacts**

- `oracle-pin.txt`: build.rs, pin.rs (Rust); run-capture.sh, build-oracle.sh (writes), cloud/setup.sh, bisection runners (shell).
- `table.conf`: `oxpinyin_data::set_lambda_from_table_conf` (called from six open sequences), `oxpinyin_data::parse_table_conf_lambda` (fuzzed by RS-64), `testsupport::locate_data` (existence), `testsupport::PinDir::fresh` (copies it), w3 `datagen-manifest.txt` lists it as a table.
- `datagen-manifest.txt`: not parsed by anything in this assignment (written by oxpinyin-datagen; read by its own tests).
- `pin_ref` strings: `pin_ref.rs` (constant), `pin.rs` (manifest equality), `differential::decide` (observation equality), `taxonomy::classify_off_pin` (split on `+`), `capture` (passes through), `oracle_paths::check_pin`, `candidate_structure`/`real_tables_integration`/`graph_paths` (`# pin_ref=` header equality), `fixtures/w3/pin-ref.txt` (file, not read by Rust in this set).

**Backend names / extensions hard-coded**

- `pin.rs::EXPECTED_DBM = "Tkrzw"`; `pin_ref.rs` `+dbm-tkrzw`, `+dbm-kc`, `+dbm-bdb`, display names `KyotoCabinet`, `BerkeleyDB`.
- `taxonomy.rs` tests use `dbm-berkeleydb` (a spelling no manifest produces).
- `testsupport::model_cache::system_dbm_names` → `pinyin_index.bin`, `phrase_index.bin`, `bigram.db` when `oxpinyin_store::DEFAULT_STORE_IS_LIBPINYIN_DBM`; otherwise `oxpinyin_store::default_store_file`.
- `scan_perf.rs` uses `oxpinyin_data::DEFAULT_STORE_EXT` for the temp store suffix; `capi_commands.rs` uses it to pick `fixtures/w3/<ext>`.
- `fixtures/w3/{db,kct,tkt}` directory names and `database format:` lines; `oracle-sentence-tie-order.txt` is pinned to `dbm-bdb` while every other golden is `dbm-tkrzw`.
- `sentence_tail.rs`, `corpus_tail.rs`, `parity_sweep.rs`, `parity_worst.rs`, `benches/support`, `scan_perf.rs` hard-code the `table.conf` file name and `SystemDbm::Bigram.file_name()`.

**Process control / locks / timeouts**

- `live::ORACLE_LOCK` — process-wide mutex held for an `Oracle`'s lifetime (abort avoidance), poison ignored.
- `fuzz/capi_commands.rs` — `OnceLock` session + `atexit` teardown ordered before LSan.
- `fuzz/user_store_ops.rs` — `Mutex<Option<PathBuf>>` for last-store cleanup.
- Spawned processes: `tests/model_fetch.rs` (`bash tools/model/fetch-model.sh`, with a PATH `git` shim), `dbm_bench.rs` (`current_exe()` children for VmHWM), `testsupport::differential::PinDir::run` (pin trainer binaries, cwd = fresh copy). None sets a timeout or process group; libFuzzer timeouts come from CI flags (`-timeout=15`, `-max_total_time=180`).
- Cargo is spawned by none of these Rust files (the shell/Python tools do that).
- `differential_live.rs` shares one corpus run via `OnceLock` to avoid repeating a 10,465-input oracle pass per test.

**Frozen / "do not change"**

- `ffi.rs`: `bool` returns ("Do not 'simplify' these signatures"); symbol list frozen in `oracle-ffi-seam.md`, "adding a symbol still means updating this finding".
- `Cargo.toml`: package name `pinyin-oracle` ("Do not 'fix' this to oxpinyin").
- `pin_ref.rs`: must stay `use`-free (textually included by build.rs).
- Schemas: `pinyin-capture-v1`, `pinyin-diff-v1` (14-field order asserted by test), `pinyin-divergence-v1`, `pinyin-oracle-v1` (manifest), `pinyin-oracle-candidates-v1`, `pinyin-oracle-candidate-structure-v1`, `pinyin-oracle-path-v1`, `sentence-tie-order-v1`, `pinyin-session-replay-v1`, `pinyin-fixture-vocab-v1`, `pinyin-fixture-bigram-v1`, `oxpinyin-datagen-manifest-v1`.
- Pinned measured numbers: `differential_live.rs` (9,976/489/481/2/6/9,508/468/483/52), `sentence_surface_parity.rs` (500×, native costs, 8,592/8,744/22,038/8,586/6), `decode_differential.rs` (10,459, ≥80%), `parity_corpus.rs` (stratum counts, 10,465), `capture_replay.rs` (15/46), `taxonomy_replay.rs` (14/1, 10/36).
- *Interpretation:* the `#`-comment convention on golden files collides with the junk stratum's `#`-leading inputs; only `graph_paths.rs` (TAB-presence) and the capture reader (no comment syntax) are immune. `oracle_sentence_surface.rs` acknowledges this ("`#yaguai`") rather than fixing it.
- *Interpretation:* doc-comment drift — `sentence_tail.rs` bin doc (496/488/385/379, 117 residual) and `goldens-and-pins.md` (491/396/390 of 496) disagree with the test's 500/500/500; `corpus_tail.rs` header still cites pin `0c5e80e`; `mini-vocab.txt` header cites `2.11.91-0c5e80e`; `testsupport::differential` doc still describes skip-not-fail.

## pinyin-oracle: generic vs pinned vs oxpinyin-specific

Classes: **(G)** depends only on the public libpinyin C API + glib; **(P)** depends on the 074a2219 pin, `oracle-pin.txt`, model20 SHA, the build-oracle.sh prefix layout, or the patch/data manifests; **(X)** depends on oxpinyin-core/engine/data/testsupport types or the divergence register. Dependency edges name the `use` line or type that forces the class.

### ffi.rs — all (G)

| Item | Class | Edges |
|---|---|---|
| `PinyinContext`, `PinyinInstance`, `LookupCandidate`, `ChewingKey`, `ChewingKeyRest`, `ExportIterator` | G | `core::marker::{PhantomData, PhantomPinned}` only. |
| `PinyinOption = u32` | G | `novel_types.h` `guint32` (any 2.x revision). |
| `g_free`, `GArray`, `g_array_free`, `g_array_new` re-exports | G | `glib_sys`. |
| 29 `extern "C"` fns | G (with a caveat) | Signatures are those of `pinyin.h`; `pinyin_get_candidate_type`/`_nbest_index`, `pinyin_lookup_tokens`, `pinyin_token_get_phrase`, `pinyin_begin/end_get_phrases` exist only in libpinyin ≥ the revisions that added them — generic over revisions that declare them, but the module doc pins the header SHA, which is documentation not code. |

### live.rs

| Item | Class | Edges |
|---|---|---|
| `Oracle::open`, `open_with_user_dir`, `open_with_temp_user_dir` | **P** | take `OraclePrefix` (pin.rs) — "Holding this value implies the prefix matched the frozen pin"; `prefix.data_dir()` = `lib/libpinyin/data` (build-oracle.sh layout). Mechanically G apart from that parameter. |
| `Oracle::prefix`, `pin_ref` | P | return `OraclePrefix` / `VerifiedPin::pin_ref`. |
| `Oracle::save_user_data`, `user_dir`, `observe`, `session`, `export_phrases` | G | FFI + `OracleFlags`/`OracleObservation` (both G types). `save_user_data`'s "pinyin_save returns false for an unmodified store (`pinyin.cpp:1136`)" is a pinned behavioural note, code is generic. |
| `Session::reset`, `observe`, `train_top`, `observe_candidate_infos`, `observe_sentence_surface`, `lookup_tokens`, `token_text` | G | FFI + observation types. Abort guards (`collect_segments` lowercase check, nbest-only index read, proven-depth sentence read) encode *pinned defects* (`oracle-apostrophe-abort.md`, F-E-01) but are safe no-ops on other revisions. `pin_ref: String` field is just carried. |
| `SentenceSurface`, `ExportedPhrase` | G | `CandidateInfo` (G). |
| `LiveSource` + `impl ObservationSource` | G | `differential::{ObservationSource, Source}` — the trait itself is G (see below). |
| private `ORACLE_LOCK`, `FreshUserDir`, `UserDir`, `path_to_cstring` | G | std only; the lock is justified by a pinned assertion but is revision-agnostic. |

### observation.rs — all (G)

| Item | Class | Edges |
|---|---|---|
| `OracleCompleteness`, `OracleCandidateType` (+`from_raw`/`as_wire`/`from_wire`), `CandidateInfo`, `OracleSegment`, `OracleObservation`, `MAX_CAPTURED_CANDIDATES` | G | `crate::OracleFlags` (G), `crate::OracleError` (G). `OracleCandidateType` discriminants 1..=8 are `lookup_candidate_type_t` of `pinyin.h` (any revision with eight values). `pin_ref: String` is an opaque stamp here. |

### capture.rs — all (G)

| Item | Class | Edges |
|---|---|---|
| `CAPTURE_SCHEMA`, `escape`, `unescape`, `CaptureRecord`, `read_fixture`, `segment_to_wire`, `segments_to_wire` | G | observation + flags + error only. Format is oxpinyin's own `pinyin-capture-v1` wire protocol but encodes nothing pin- or crate-specific. |

### differential.rs

| Item | Class | Edges |
|---|---|---|
| `DIFF_SCHEMA`, `DIVERGENCE_SCHEMA`, `MAX_LOGGED_PATHS`, `UNCLASSIFIED`, `Source`, `Reason`, `Outcome`, `Classifier`, `Unclassified`, `ObservationSource`, `ReplaySource`, `Case`, `RunOutput`, `RunReport` | G | std + observation/capture/error. (`Reason::OffPin` exists because of pin policy but the enum is data.) |
| `OurParse` | **X** | `oxpinyin_core::{ParseResult, ParseError}` in the variants. |
| `Comparison` | X | field `ours: OurParse`. |
| `compare` | **X + P** | calls `oxpinyin_core::FullPinyinParser.parse` (X); `decide` step 1 compares `observation.pin_ref != EXPECTED_PIN_REF` (P). |
| `comparison_line`, `divergence_line` | X | take `&Comparison`; `render_our_path` uses `oxpinyin_core::{ParseResult, Completeness}`. |
| `run` | X | via `compare`. |

### pin.rs — all (P)

| Item | Class | Edges |
|---|---|---|
| `DATA_MANIFEST_FILE_NAME`, `DATA_UNSTABLE_MANIFEST_FILE_NAME`, `MANIFEST_SCHEMA`, `EXPECTED_DBM`, `EXPECTED_LIBPINYIN_TAG`, `DATA_SUBDIR` | P | build-oracle.sh prefix/manifest layout. |
| `PinManifest::parse`, `read_from_prefix`, `field` | P (mechanically G) | generic `key=value` parser of `oracle-pin.txt`; only the file name is layout-specific. |
| `PinManifest::verify`, `VerifiedPin` | P | `EXPECTED_PIN_REF`, `PINYIN_BENCH_DBM`, required digest fields (`model_sha256`, `header_sha256`, …). |
| `OraclePrefix::{open, locate, root, data_dir, pin}` | P | `DATA_SUBDIR`, `DATA_MANIFEST_FILE_NAME`, `crate::DEFAULT_PREFIX_SUFFIX`, `PREFIX_ENV`. |

### pin_ref.rs — all (P)

| Item | Class | Edges |
|---|---|---|
| `EXPECTED_PIN_REF`, `MANIFEST_FILE_NAME`, `bench_pin_ref` | P | the literal pin string (commit 074a2219, model20 SHA, `dbm-tkrzw`). |

### taxonomy.rs

| Item | Class | Edges |
|---|---|---|
| `AUTO_ACCEPT_BUDGET_DENOMINATOR`, `DivergenceClass` (+ all methods), `BudgetVerdict`, `budget` | G | std + `differential::{Classifier, Comparison, Outcome, Reason}` names; but note `Comparison` is X, so `Classifier::classify(&Comparison)` drags X in through the parameter type. `DivergenceClass` wire names are oxpinyin's divergence register vocabulary (*interpretation: X by provenance, G by code*). |
| `Taxonomy::{new, parity_profile, class_of}` | **X + P** | reads `comparison.observation.flags` and `.pin_ref`; `classify_off_pin` splits `EXPECTED_PIN_REF` (P); takes `&Comparison` (X). Rules come from `divergence-taxonomy.md` (register). |
| `impl Classifier for Taxonomy` | X | as above. |

### corpus.rs — all (X)

| Item | Class | Edges |
|---|---|---|
| `SEED`, `CORPUS_DIR`, `MAX_APOSTROPHE_GROUPS`, `DOMAIN_LOW`, `DOMAIN_HIGH`, `Stratum` | G | std only (constants and a struct). |
| `generate` (and private `partial_prefixes`, `ambiguous_syllables`, `within_result_bound`, `accept`, `fill`, `join_syllables`, `junk_input`) | **X** | `use oxpinyin_core::{FULL_PINYIN_SYLLABLES, FullPinyinParser, InputParser}` — the inventory and the result-bound filter are oxpinyin's parser. `edge_cases` is G. |

### Other public modules (for completeness)

- `flags.rs` — G (values from `pinyin_custom2.h`; `CAPTURE_DEFAULT` is a profile choice, not a pin).
- `error.rs` — G.
- `model_cache` — P (model20 SHA/URL) + X (`oxpinyin_store` for `system_dbm_names`).
- `sentence_tail.rs` — X (`oxpinyin_data`, `oxpinyin_engine`, `oxpinyin_testsupport`).
- `lib.rs` consts `DEFAULT_PREFIX_SUFFIX`, `PREFIX_ENV` — P.

### How the FFI is linked

Static `extern "C"` declarations linked at build time, **no pkg-config crate, no bindgen, no dlopen**: `build.rs` emits `rustc-link-search=native=<prefix>/lib{,64}`, `rustc-link-lib=dylib=pinyin`, `rustc-link-lib=dylib=glib-2.0`, and `rustc-link-arg=-Wl,-rpath,<libdir>`; `glib-sys` (workspace dep, Linux target only) supplies the four GLib symbols. The prefix is found via `PINYIN_ORACLE_PREFIX` → `PKG_CONFIG_PATH` entries (walked up from `lib/pkgconfig`) → `$HOME/.local/opt/pinyin-oracle`, and is **refused unless `oracle-pin.txt` `pin_ref=` equals `EXPECTED_PIN_REF`** (or its `kc`/`bdb` bench variant under `PINYIN_BENCH_DBM`). Everything is behind the non-default `oracle-ffi` feature; with it off no `extern` block compiles.

### Symbols declared: 29 (+4 GLib)

`pinyin_init, pinyin_fini, pinyin_set_options, pinyin_alloc_instance, pinyin_free_instance, pinyin_reset, pinyin_parse_more_full_pinyins, pinyin_get_parsed_input_length, pinyin_guess_sentence, pinyin_get_sentence, pinyin_guess_candidates, pinyin_get_n_candidate, pinyin_get_candidate, pinyin_get_candidate_string, pinyin_get_candidate_type, pinyin_get_candidate_nbest_index, pinyin_get_pinyin_key, pinyin_get_pinyin_key_rest, pinyin_get_pinyin_key_rest_positions, pinyin_get_pinyin_string, pinyin_get_pinyin_is_incomplete, pinyin_train, pinyin_save, pinyin_begin_get_phrases, pinyin_iterator_has_next_phrase, pinyin_iterator_get_next_phrase, pinyin_end_get_phrases, pinyin_lookup_tokens, pinyin_token_get_phrase`; GLib: `g_free, g_array_new, g_array_free` (+ `GArray` type). Six opaque types.

### Observation types and serialized formats

- `OracleObservation {pin_ref, family?, case?, input: Vec<u8>, flags: OracleFlags, parse_return, parsed_input_length, segments: Vec<OracleSegment>, candidate_total: u32, candidates: Vec<String> (≤10), remainder: Vec<u8>}`; `OracleSegment::{Syllable{syllable,begin:u16,end:u16,completeness}, MissingKeyRest, MissingPosition, MissingPinyinString, NoKeyColumns}`; `CandidateInfo {text, candidate_type: OracleCandidateType, nbest_index: Option<u8>}`; `SentenceSurface {guessed, sentences: Vec<Option<String>>, candidate_total, candidates: Vec<CandidateInfo>}`; `ExportedPhrase {phrase, pinyin (apostrophe-joined), count: i32}`.
- Wire `pinyin-capture-v1` (read; written only by the C harness): TAB `key=value`, escaping `\\ \t \n \r \xNN`; `flags=0x%08x`; `segments=-|text@b:e:complete|partial[,…]` with sentinels `<missing-rest>@n`, `<missing-position>@n`, `<missing-pinyin>@n`, `<no-key-columns>@n`; `candidates_hex=-|hex[,hex…]`.
- `pinyin-diff-v1` (written): `schema, pin_ref, source=live|replay, corpus, case, input, flags, outcome=agreement|divergence, class, rank|-, ours_paths, ours_consumed|-, theirs_consumed, candidate_total`.
- `pinyin-divergence-v1` (written): `schema, pin_ref, source, corpus, case, input, flags, class, reason, theirs_path, theirs_consumed, theirs_remainder, ours_paths, ours_shown, ours_path_set (≤8 paths ';'-joined, or <error:…>), ours_consumed, ours_remainder`.
- Golden formats (bins): `pinyin-oracle-candidates-v1`, `pinyin-oracle-candidate-structure-v1`, `pinyin-oracle-path-v1` (headers `# …`), the headerless-schema sentence-surface file (`# pin_ref`, `# sample`), `sentence-tie-order-v1` (Python-generated).

### Differential comparison primitives

`differential::decide`, in frozen precedence: (1) `pin_ref == EXPECTED_PIN_REF` else `off-pin`; (2) `parsed_input_length <= input.len()` else `oracle-inconsistent`; (3) `input.len()` fits `u16` else `position-overflow`; (4) no sentinel segment else `oracle-sentinel`; (5) our parser `Ok` else `ours-error`; (6) our consumed (= `input.len() - first_path.remainder.len()`) == `parsed_input_length` else `consumed-length`; (7) our remainder bytes == oracle remainder else `remainder`; (8) oracle's `(syllable, begin, end, completeness)` tuple list ∈ our ordered path set (rank = index) else `path-absent`; membership at any rank is `agreement`. Candidates are recorded (`candidate_total` in the diff line), never compared here; candidate/sentence comparison lives in `decode_differential.rs` (top-1/top-5-set/prefix-10 via engine `Session`), `sentence_tail.rs` (row-0, ordered list, multiset, distinct-set, first-divergent rank, first-6 rows split into `n/` vs phrase rows), and the bins (`corpus_tail`: top1/top5/absent/order-only/prefix-10 gap).

### Taxonomy mapping (reason → class)

`Agreement` rank 0 → `output-identical`; rank>0 → `tie-swap`; `off-pin` → `data-version` if `code` and `dbm` components of `pin_ref` match `EXPECTED_PIN_REF` and only `model` differs, else `distro-delta`; `oracle-inconsistent | position-overflow | oracle-sentinel` → `theirs-bug`; `ours-error` → `ours-bug`; `consumed-length | remainder | path-absent` → `path-set` if `observation.flags == parity_profile` (default `0x18a`) else `flag-semantics`. Gating: `path-set`, `ours-bug`. Auto-accepted: `output-identical`, `tie-swap`. Budget: `limit = corpus_size / 200`; `divergence_only_pass = tie_swap <= limit` (the ruled meaning), `literal_pass = output_identical + tie_swap <= limit` (reported, rejected).

### What could become a standalone crate (no oxpinyin dependency)

**Extractable as-is (pure G):** `ffi.rs`, `observation.rs`, `capture.rs`, `flags.rs`, `error.rs`, and `live.rs` minus its `OraclePrefix` parameter; from `differential.rs` the `Source`, `Reason`, `Outcome`, `Classifier`, `ObservationSource`, `ReplaySource`, `Case`, `RunReport`, `RunOutput` types and the two log writers *if* `Comparison` is re-typed; from `taxonomy.rs` `DivergenceClass`, `BudgetVerdict`, `budget`. That is a "libpinyin observation + capture-replay" crate: open any libpinyin (given a data dir and user dir), observe parse/candidates/sentences/exports, read and write the capture/diff/divergence wire formats, serialise contexts process-wide.

**What must be cut or re-homed to get there:**

1. **Pin policy out of the core.** `OraclePrefix` is the only argument to `Oracle::open*`; replace with `(system_data_dir: &Path, pin_ref: String)` and move `pin.rs`, `pin_ref.rs`, `build.rs`'s pin check, `lib.rs::{DEFAULT_PREFIX_SUFFIX, PREFIX_ENV}` into a thin "pinned-oracle" layer (or keep them in oxpinyin's harness). `build.rs` would then be plain link discovery (pkg-config or `PINYIN_ORACLE_PREFIX`) without the manifest gate. `differential::decide` step 1 and `taxonomy::classify_off_pin` need the expected ref passed in as data (`compare(obs, source, corpus, case, expected_pin_ref: &str)`) instead of reading `EXPECTED_PIN_REF`.
2. **Our-side parser out of `differential.rs`.** `OurParse`, `Comparison.ours`, `compare`, `render_our_path`, `our_path` import `oxpinyin_core::{FullPinyinParser, InputParser, ParseResult, ParseError, Completeness}`. Re-home behind a trait, e.g. `trait SubjectParser { fn paths(&self, input: &[u8]) -> Result<Vec<ComparablePath>, String> }` with `ComparablePath = Vec<(String, u16, u16, Completeness)>` defined locally (an `OracleCompleteness`-typed tuple, not `oxpinyin_core::Completeness`). `Comparison` then holds the subject's paths/consumed/remainder in neutral types; oxpinyin supplies the impl in its own test crate. `Taxonomy` follows for free once `Comparison` is neutral.
3. **Corpus generator stays in oxpinyin.** `corpus::generate` is built on `FULL_PINYIN_SYLLABLES` and `FullPinyinParser` (result-bound exclusion); only `Stratum`/`CORPUS_DIR`/`SEED` and `SplitMix64` are neutral. Either leave `corpus.rs` in oxpinyin or parametrise `generate(inventory: &[&str], within_bound: impl Fn(&str)->bool)`.
4. **Everything that opens oxpinyin tables is X and stays:** `sentence_tail.rs`, `model_cache` re-export (`oxpinyin_store`), bins `corpus-tail`, `parity-sweep`, `parity-worst`, `sentence-tail`, benches `scan_perf`/`alloc_profile`/`support` (`oxpinyin_data`, `oxpinyin_engine`, `oxpinyin_user`), tests `decode_differential`, `graph_paths`, `sentence_surface_parity`. `dbm_bench.rs` is G+P (only `Oracle`/`OraclePrefix`) and could move with the core once (1) is done. `oracle-candidates`, `oracle-candidate-structure`, `oracle_sentence_surface`, `parity-diff`, `oracle-paths` are G+P except for `corpus::generate()` and (oracle-paths) `FullPinyinParser` — they would take an input file instead.
5. **Fixture doubles:** `oxpinyin-testsupport::fixture` is X by nature (implements `oxpinyin_core::{Dictionary, LanguageModel}`); `testsupport::differential` and `model_cache` (minus `system_dbm_names`'s `oxpinyin_store` edge) are generic pin-tool/model-cache helpers and could join the standalone crate's "provisioning" module.
6. **Documentation pins to leave behind:** header SHA, `pinyin.cpp:NNNN` citations, abort-guard rationale (keep the guards, drop the "pinned defect" claims into oxpinyin's divergence register).

