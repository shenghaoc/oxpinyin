# Backend function-order policies (#1076)

Recorded 2026-10-23 08:22:01 UTC. All three contract and same-data gates completed. Results are text summaries; no captures or logs are retained.

## Summary

Kyoto Cabinet and tkrzw shipping builds previously had no measured function-order policy and linked unordered. Give each backend and facade its own reproducible policy, select it during the final packaging link, and verify the selected backend independently from the installed object's `DT_NEEDED`.

## Changes

- Store policies under `tools/packaging/function-order/{bdb,kc,tkrzw}/`. Move the BDB policy bytes unchanged. Retain the two old paths as BDB symlinks for #1082's existing nightly check; that workflow still checks BDB only.
- Reject missing registered policies, stale paths and named policies belonging to another backend, including symlink aliases. Preserve explicit opt-out and unsupported-linker fallback from #1075.
- Allow only teardown profile provisioning on KC/tkrzw. Later teardown stages remain BDB-only. Document all three regeneration recipes, same-cell profiles, one target directory per backend, tkrzw dependency/runtime selection and invalid instruction controls. No new driver or workflow change.

The policies are in their own commit. No production algorithm, C ABI, persistent data format, dependency or frozen input changes.

## Verification

Verified shipping source: BDB `7778c966` on main `1cdb810c`; KC/tkrzw `e9fd97e1` on main `13347dbf`. The latter integrates #1068's runtime preflight; it changes no BDB shipping/link input, and retained BDB artifact and policy hashes were verified unchanged. Publication integrates main `e58d2e26`. Commit organization and the unrelated documentation/admission-test rebase preserve the measured implementation and all six policy files. The tkrzw gate completed on the same implementation bytes; the older measurement-head identifiers record provenance, not final commit ancestry. Results below belong to those named source revisions, not to historical builds.

Native Ubuntu 26.10 x86-64, i7-9750H, 4-KiB pages, home-disk ext4 worktree and `TMPDIR`; GNU BFD ld 2.47, cc 15.3.0, Rust 1.97.1, cargo-c 0.10.24, Valgrind 3.27.1. All campaigns used normal serialized `just heavy` admission; no cache clearing. macOS/Windows/arm64: not run here.

Setup verified BDB/KC/tkrzw oracle prefixes. In each cell, with its verified environment sourced:

```sh
# FEATURE=bdb|kyotocabinet|tkrzw; separate CARGO_TARGET_DIR per cell.
OXPINYIN_FUNCTION_ORDER_POLICY= just heavy -- tools/packaging/install.sh libpinyin --prefix="$BASE" -- --locked --release --no-default-features --features "$FEATURE,shipped"
OXPINYIN_FUNCTION_ORDER_POLICY= just heavy -- tools/packaging/install.sh libzhuyin --prefix="$BASE" -- --locked --release --no-default-features --features "$FEATURE"
just heavy -- uv run --no-project --managed-python --python cpython@3.15+gil python -B -X lazy_imports=normal tools/bisection/contract-diff.py "$CELL" "$PIN" "$BASE/lib/libpinyin.so.15.0.0" --zhuyin-so "$BASE/lib/libzhuyin.so.15.0.0" --unordered-arm ox --unordered-arm control --teardown --teardown-stage provision --teardown-work-dir "$TEARDOWN"
```

`tools/generate-function-order.py` ran twice per backend with both same-build `--library`/`--staticlib` pairs, `--bisect`, that cell's `--system-dir`/`--system-identity`, its provisioned `--profiles-dir`, `--cpu 2`, and separate output directories. `cmp` passed for both facades in every cell. Ordered release-recipe installs then ran with the policy environment variable unset. `tools/packaging/function_order.py` reported **ordered** for both PR/control objects in all cells; base was explicitly intentional-unordered, pin reference. Exports were **79 pinyin / 52 zhuyin** on every subject arm.

Source tests at `e9fd97e1`:

```sh
uv run --no-project --managed-python --python cpython@3.15+gil python -B -X lazy_imports=normal tools/packaging/function-order.test.py  # 38 passed
uv run --no-project --managed-python --python cpython@3.15+gil python -B -X lazy_imports=normal tools/generate-function-order.test.py  # 39 passed
uv run --no-project --managed-python --python cpython@3.15+gil python -B -X lazy_imports=normal tools/bisection/teardown.test.py  # 33 passed
uv run --no-project --managed-python --python cpython@3.15+gil python -B -X lazy_imports=normal tools/bisection/contract-diff.test.py  # 86 passed
uv run --no-project --managed-python --python cpython@3.15+gil python -B -X lazy_imports=normal tools/tkrzw/runtime.test.py  # 24 passed
bash -n tools/packaging/relink-versioned.sh
cargo fmt --all --check
git diff --check
```

Publication static checks passed after commit organization and a conflict-free rebase onto main `e58d2e26`: `bash -n`, `cargo fmt --all --check`, and `git diff --check`. All three rule 2 outputs remained empty. The implementation and test files match measured source `e9fd97e1`; no completed campaign was repeated for unrelated documentation/admission-test changes.

All three rule 2 path checks produced empty output: no `crates/oxpinyin-store/` path; no changed non-Markdown file containing `feature = "bdb"|"tkrzw"|"kyotocabinet"`; no backend-naming changed Cargo.toml line. The checks did not trigger the three-cell rule. All three cells were explicitly requested for this backend-policy task.

Rule 7: this is the initial coherent tooling/policy completion, with full requested shipping integration gates. After #1068 merged, its guard and the provisioner dispatch integration were checked by the named harness tests and KC/tkrzw campaigns. No Rust crate is modified, so crate tests/clippy were not applicable. The final unrelated documentation/admission-test rebase uses rule 7's no-conflict/no-touched-file case; it does not turn earlier-source measurements into new-source measurements. No completed campaign was repeated solely for main movement.

Full `just heavy -- ... tools/bisection/contract-diff.py CELL PIN PR_PY --zhuyin-so PR_ZHU` and `just heavy -- tools/bisection/run-same-data-dir-diff.sh PIN_PY PR_PY DATA` results:

| Cell | Contract MATCH / SKIP / DIFFER | Same-data default drivers |
|---|---|---|
| BDB | 2888 / 1 / 0 | 15 IDENTICAL |
| KC | 2876 / 13 / 0 | 15 IDENTICAL |
| tkrzw | 2876 / 13 / 0 | 15 IDENTICAL |

BDB's one SKIP is `issue785-pinyin-tails-kept-entry-zhuyin`, driver `tools/bisection/contract-diff.py`, registered only for KC/tkrzw. The emitted row has no reason field. The verbatim source explanation is: "The case crafts a fixture in another cell's storage format (e.g. a Berkeley DB btree); neither side can reach the site here, so there is nothing to compare." This exclusion occurs before worker/library selection. It is not a missing-input skip. Whether an observed base contract run skips this case is **Unknown**: no such retained run exists. Source exclusion of a BDB subject is not observed-base evidence.

KC and tkrzw each excluded the same 13 BDB-fixture cases: `abort-add-phrase-duplicate-library-token`, `abort-add-phrase-index-item-missing`, `abort-add-phrase-index-length-mismatch`, `abort-add-phrase-index-text-mismatch`, `abort-choose-candidate-predicted-bigram-row`, `issue542-stale-user_phrase_index.bin-{pinyin,zhuyin}`, `issue542-stale-user_pinyin_index.bin-{pinyin,zhuyin}`, `issue762-token-reuse-stale-bigram-no-mask-{pinyin,zhuyin}`, and `issue762-token-reuse-stale-bigram-{pinyin,zhuyin}`. The retained terminal reports were compared case by case: both cells name exactly this set. Tkrzw contract completed with 5752 workers in 1713.3743 seconds; its 15 same-data drivers and the serial sequence exited 0.

For tkrzw, manual `ldd` checks required exactly one canonical resolution for both pins, installed subjects and every copied arm; discarded live `LD_DEBUG=libs` traces checked both sides. All 1310 measurement-copy checks resolved to `/home/sheng/Documents/Codex/2026-10-23/task-10/1076-op.wtX6BX/cache/tkrzw/lib/libtkrzw.so.1.77.0`, SHA-256 `36c6aa4d77edb3bd6a628f02fc456f0a67096bdd451c04999141d7144f144d0e`, built by setup from tkrzw 1.0.34. The pin and subject paths were identical to this cache path. The no-`LD_LIBRARY_PATH` contrast resolved `/usr/lib/x86_64-linux-gnu/libtkrzw.so.1.75.0`; no campaign used it. Both completed generation passes also passed their manual runtime checks. #1068's merged guard additionally checks actual gate workers.

An executor disconnect left the earlier tkrzw generation's final exit status unknown. Recovery established no surviving owned process and no completed policy output, and verified retained unordered builds, profiles and identities. Only unfinished generation and later stages resumed; completed BDB/KC/tkrzw provisioning was not repeated. No captures, logs or evidence files are attached or committed; results are text only.

## Compatibility and impact

Each backend's measurement job compared unordered base, ordered PR, a same-bytes PR control and the same-cell pin together, with equal-length arm directories, both facades, `phrases`/`journals`, CPU 2 and 20 native rounds. Invocation: `just heavy -- ... tools/measure-function-order.py --subject base=BASE --subject pr=PR --subject control=CONTROL --subject pin=PIN --unordered-arm base --bisect BISECT --system-dir DATA --system-identity ID --profiles-dir PROFILES --work-dir WORK --cpu 2 --rounds 20`, with explicit `--function-order pr/FACADE=.../function-order/CELL/libFACADE.order` and matching control overrides. No absolute figure from another job is used as a baseline.

M1 medians below use **base / PR / pin**. Pss is KiB; minor faults are counts. PP/PJ/ZP/ZJ denote pinyin phrases/journals and zhuyin phrases/journals.

| Cell/workload | Text Pss | Total Pss | Minor faults | Text PR/base; PR/pin |
|---|---|---|---|---|
| BDB PP | 1800 / 512 / 452 | 9089 / 7801 / 8010 | 334 / 313 / 291.5 | .2844; 1.1327 |
| BDB PJ | 1800 / 512 / 452 | 9133 / 7845 / 8025 | 345.5 / 324 / 302 | .2844; 1.1327 |
| BDB ZP | 1604 / 520 / 436 | 8709 / 7625 / 7711 | 324 / 306 / 282 | .3242; 1.1927 |
| BDB ZJ | 1604 / 520 / 436 | 8754 / 7670 / 7717 | 332.5 / 314 / 291 | .3242; 1.1927 |
| KC PP | 1700 / 508 / 716 | 15924 / 14736 / 14407.5 | 1754 / 1734 / 1698 | .2988; .7095 |
| KC PJ | 1700 / 508 / 716 | 15935.5 / 14711 / 14405 | 1760 / 1740 / 1703 | .2988; .7095 |
| KC ZP | 1632 / 520 / 676 | 11543 / 10432 / 9965 | 905 / 885 / 842 | .3186; .7692 |
| KC ZJ | 1632 / 520 / 694 | 11569.5 / 10458 / 9933 | 910 / 892 / 847 | .3186; .7493 |
| tkrzw PP | 1768 / 508 / 464 | 10661 / 9401 / 9137 | 611 / 591 / 561 | .2873; 1.0948 |
| tkrzw PJ | 1768 / 508 / 464 | 10684.5 / 9422.5 / 9126 | 620 / 600 / 568 | .2873; 1.0948 |
| tkrzw ZP | 1576 / 520 / 448 | 9797 / 8741.5 / 8430.5 | 498 / 480 / 449.5 | .3299; 1.1607 |
| tkrzw ZJ | 1576 / 520 / 448 | 9864 / 8808 / 8428 | 515 / 497 / 458 | .3299; 1.1607 |

Text same-bytes controls were exact. The required fall holds on KC (1192/1112 KiB pinyin/zhuyin) and tkrzw (1260/1056 KiB). The maintainer's requested pin band is **±64 KiB**. Measured PR-minus-pin text Pss is:

| Backend/facade | Phrases difference, KiB | Journals difference, KiB | Pin-band category |
|---|---:|---:|---|
| BDB pinyin | +60 | +60 | within ±64 KiB |
| BDB zhuyin | +84 | +84 | above the pin |
| KC pinyin | -208 | -208 | below the pin |
| KC zhuyin | -156 | -174 | below the pin |
| tkrzw pinyin | +44 | +44 | within ±64 KiB |
| tkrzw zhuyin | **+72** | **+72** | **above the pin** |

Categories are below the pin for differences less than -64 KiB, within ±64 KiB for the inclusive band, and above the pin for differences greater than +64 KiB. The required base-to-PR fall and the pin-band category are distinct. BDB policy bytes are unchanged, so BDB zhuyin's +84 KiB predates #1076.

For total Pss, conservative same-bytes difference bounds (control minimum minus PR maximum through control maximum minus PR minimum) are, in PP/PJ/ZP/ZJ order: BDB `[-9,12],[-9,8],[-10,12],[-12,11]`; KC `[-85,153],[-105,151],[-63,80],[-30,65]`; tkrzw `[-19,10],[-13,22],[-9,22],[-17,12]` KiB. Bounds entirely inside ±max(64 KiB, 1% of PR median) establish containment for any paired-bootstrap interval on those samples. The two KC pinyin bounds exceed that band, so their total-Pss control validity is **unestablished** and accepted total-Pss pin ratios are suppressed. No actual paired-bootstrap interval was computed or retained. Other total PR/base; PR/pin ratios: BDB PP `.8583;.9739`, PJ `.8590;.9776`, ZP `.8755;.9888`, ZJ `.8762;.9939`; KC ZP `.9038;1.0469`, ZJ `.9039;1.0529`; tkrzw PP `.8818;1.0289`, PJ `.8819;1.0325`, ZP `.8923;1.0369`, ZJ `.8929;1.0451`.

Exact init Ir (base = PR = control; then pin) is BDB PP `9631681;10284337`, BDB ZJ `9592003;10179485`; KC PP `15179124;15601534`, PJ `15207461;15673812`, ZP `11711711;12147571`, ZJ `11735191;12206174`; tkrzw PP `338691482;338900015`, PJ `338681725;338967207`, ZP `338290775;338555087`, ZJ `338285132;338623206`. All PR/base ratios are 1. PR/pin ratios are BDB PP `.936539`, ZJ `.942288`; KC PP/PJ/ZP/ZJ `.972925/.970246/.964120/.961414`; tkrzw `.999385/.999158/.999219/.999002`. BDB PJ/ZP init controls differ by 1 Ir and are **INVALID**; their absolute comparisons are suppressed.

All first-candidate, startup and steady Ir controls are nonexact: **INVALID**, not gains or regressions. Diagnostic PR-minus-base vectors and control-minus-PR vectors, in PP/PJ/ZP/ZJ order:

| Cell/phase | PR-base | Control-PR | Pooled symmetric range |
|---|---|---|---|
| BDB first-candidate | -93,259,74,-52 | 30,144,-138,102 | ±144 |
| BDB startup | -93,259,73,-52 | 30,145,-137,102 | ±145 |
| BDB steady | 11377,6724,-3729,6145 | -6556,7968,10394,-3943 | ±10394 |
| KC first-candidate/startup | -30,7,-124,125 | 52,-151,-104,224 | ±224 |
| KC steady | 9576,-13467,2672,3456 | -4674,15928,-6901,504 | ±15928 |
| tkrzw first-candidate/startup | -106,-204,243,-69 | -56,251,-230,-116 | ±251 |
| tkrzw steady | 12673,12441,2791,6561 | -2785,243,13068,-259 | ±13068 |

Steady Ir is limited by nonexact `RandomState` controls until #1031's hasher lands. These diagnostic differences do not pass the runbook's exact-count rule. Native measurement tools exited 0. The specific human overlap rulings, rather than an invented tolerance, cleared continuation of the invalid observations below.

Requested fresh-process diagnostics, not replacement current-job baselines:

- Historical KC zhuyin/journals steady at `99e83dcd` / main `697ac2ec`: base `468081585,468088027,468082220` (range `468081585–468088027`); PR `468076938,468087009,468084808` (range `468076938–468087009`). Ranges overlap, PR not above base in all pairs. An earlier collector rejected a workload checksum mismatch before producing Ir; that attempt was excluded.
- Retained current BDB pinyin/journals first-candidate at `7778c966`: base `555815,555579,555673` (range `555579–555815`); PR `555702,555680,555611` (range `555611–555702`). Startup: base `10228823,10228586,10228680` (range `10228586–10228823`); PR `10228709,10228687,10228618` (range `10228618–10228709`). Both ranges overlap; paired deltas have mixed signs. The repeated-all-pairs attribution/STOP condition did not occur.

Stripped shipping sizes, pinyin/zhuyin bytes, **base = PR = control**: BDB `2441968/2200304`; KC `2482920/2233064`; tkrzw `2487016/2245352`. Pin sizes respectively `846600/826096`, `1209592/1189080`, `867216/838520`. No increase over base.

Final policy comparison (BDB/KC/tkrzw): pinyin has `265/270/266` paths, 235 shared by all three (224 init, 11 steady); zhuyin `249/255/251`, 222 shared (211 init, 11 steady). Pairwise intersections BDB–KC/BDB–tkrzw/KC–tkrzw are `237/242/242` for pinyin and `222/229/230` for zhuyin. Only one leading ordered row is identical across all three for either facade. Proposal: keep policies separate; sharing sets would discard their measured backend-specific order. No shared prefix implemented.

## Related work

Fixes #1076. Builds on #1067 and #1075; integrates the merged #1068 runtime guard and #1083 shipping-source changes. Preserves #1082's BDB policy aliases without extending its workflow. #1031 remains necessary for exact steady instruction controls.
