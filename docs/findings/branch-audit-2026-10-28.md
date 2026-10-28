# Remote branch audit, 2026-10-28 UTC

Status: audit. The audit was read-only: no branch was pushed or deleted.
The maintainer decides which proposed tiers to delete.

## Scope and revision

- Remote: `origin` (`shenghaoc/oxpinyin` on GitHub), fetched with
  `git fetch --prune origin` at about 16:45 UTC on 2026-10-28.
- main: `8cf8e23533c38f830afdb345c0210fabca044c71`.
- Excluded: `main` and the heads of the seven open PRs, #1366, #1367,
  #1369 to #1372 and #1374 (`wip/1175-10` to `wip/1175-16`).
- Audited: 644 branches. PR and issue states were read with `gh` in
  the same session.

## Method

- `git cherry` compares each branch head with main. All `-` means every
  commit on the branch has a patch-equivalent commit on main.
- Ancestor is `git merge-base --is-ancestor <head> <main>`. No branch is
  an ancestor of main, because main takes rebase merges; landed work never
  shares commit objects with its branch.
- "PRs with this head" lists the PRs whose head ref is the branch name,
  newest first. "Number in name" is an issue or PR number parsed from the
  branch name, with its state.
- "Same subject on main" counts unlanded commits whose subject matches a
  commit on main: the work landed in a revised form.
- Committed dates are the head commit's committer date in UTC.

Groups:

1. Fully landed (547): `git cherry` is all `-`.
2. Closed PR with unlanded content (38).
3. No PR (39): no PR has this branch as its head.
4. Unclear (20): the newest PR is merged but `+` commits remain, or
   every `+` commit has a same-subject commit on main.

Proposal: T1 = fully landed, delete; T2 = superseded or disposable,
delete after a look; keep = everything else. Counts: T1 547, T2 42,
keep 55.

## Findings

- PR #372 (`chore/oracle-pin-followup`) is merged, but into its stack
  parent `chore/oracle-pin-074a221` after #363 had already landed that
  parent on main. Its change never reached main: main's
  `tools/oracle/patches/fullpin-aux-overread.patch` still carries the
  `@@ -3415` hunk header and the "Re-verified to apply cleanly" line
  that #372 replaced. Both branches are marked keep.
- The other group 4 branches with a merged PR (#47, #225, #273 to #276,
  #296) were also merged into stack parents; their commits reached main
  later in a revised form, under the same subjects.
- `wip/contract-docs` holds an earlier version of PR #1329, which merged
  from `wip/contract-docs-2` as `2c1db998` with revised text.
- `claude/dynamic-adjust-bigram-ranking-yxbu5z` adds 13 lines to
  `docs/findings/option-bits.md` re-checking the fork at `612004e`. The
  text is not on main, but main states the same conclusion ("The fork
  masks `DYNAMIC_ADJUST` out entirely"), the branch has no PR, and issue
  #99 is closed.

## Reproduction

```bash
git fetch --prune origin
git cherry -v 8cf8e23533c38f830afdb345c0210fabca044c71 <head SHA>
git merge-base --is-ancestor <head SHA> 8cf8e23533c38f830afdb345c0210fabca044c71
gh pr list -R shenghaoc/oxpinyin --state all --limit 3000 --json number,headRefName,state
gh issue list -R shenghaoc/oxpinyin --state all --limit 3000 --json number,state
```

Branch heads move and branches are deleted; the head SHA column fixes
each row's input.

## Table

| Group | Branch | Head SHA | Committed (UTC) | Author | `git cherry` | Ancestor | PRs with this head | Number in name | Proposal | First unlanded commit |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 landed | `abi-parity-79-tier-b` | `fe7a80bc11ef9d0b8667caad99f020f91e1cddab` | 2026-08-30 | Shenghao Chen | all `-` (8) | no | #231 merged | #79 PR merged | T1 |  |
| 1 landed | `abi-parity-79-tier-c` | `c08cc5a671bae999874d62a1d5a620712ecea736` | 2026-08-30 | shenghaoc | all `-` (11) | no | #234 merged | #79 PR merged | T1 |  |
| 1 landed | `abi-parity-tier-a` | `c93e138e883c7b47c97f0f077761d9a58e5d8b17` | 2026-08-30 | Shenghao Chen | all `-` (1) | no | #223 merged | — | T1 |  |
| 1 landed | `abi-parity-tier-b` | `9e0d909ae4146e7d489241e7b0d96c92b2f51bb8` | 2026-08-30 | Shenghao Chen | all `-` (3) | no | #230 merged, #227 closed, #224 closed | — | T1 |  |
| 1 landed | `abi-parity-tier-d` | `13ce6401e06851e5bda48986e1cfe91cf572f40d` | 2026-08-30 | Shenghao Chen | all `-` (5) | no | #226 merged | — | T1 |  |
| 1 landed | `audit/560-replacement` | `f72ae9fda9ae1cec57b0643154d6b474e3076b3c` | 2026-10-12 | Shenghao Chen | all `-` (9) | no | #753 merged | #560 issue closed | T1 |  |
| 1 landed | `audit/store-key-ordering` | `6e971edce66816dc0554c329a1db14371b78cda8` | 2026-08-25 | Shenghao Chen | all `-` (1) | no | #163 merged | — | T1 |  |
| 1 landed | `bench/lmdb-bulk-load-isolation` | `25595932ddaf70215e6726714a8235cd6d469676` | 2026-09-06 | Shenghao Chen | all `-` (1) | no | #345 merged | — | T1 |  |
| 1 landed | `bench/redb-is-empty-isolation` | `955bc301aa0c93f94c48ef40cbee2a820ece2486` | 2026-09-06 | Shenghao Chen | all `-` (2) | no | #348 merged | — | T1 |  |
| 1 landed | `bench/store-opt-benches` | `98f4575d0bbfc0c997fcebc39f3a53b2c9798602` | 2026-09-06 | Shenghao Chen | all `-` (4) | no | #344 merged | — | T1 |  |
| 1 landed | `bindgen-0.73` | `3f78356983d66648f98d436e913a69b24e9659f4` | 2026-09-13 | Shenghao Chen | all `-` (1) | no | #446 merged | — | T1 |  |
| 1 landed | `build/default-backend-tkrzw` | `d27577544a17b92384d64eb641d6c3aad790ada3` | 2026-09-05 | Shenghao Chen | all `-` (4) | no | #320 merged | — | T1 |  |
| 1 landed | `build/version-script-relink` | `53176b69c5a2f2dd04479e6648bea79366362c58` | 2026-09-14 | Shenghao Chen | all `-` (1) | no | #462 merged | — | T1 |  |
| 1 landed | `chore/bisection-binary-and-paths` | `d41d3630390c6dae95b1c00aa38568431819665b` | 2026-09-07 | Shenghao Chen | all `-` (1) | no | #364 merged | — | T1 |  |
| 1 landed | `chore/drop-python-wrappers` | `cb8e09244ba293f26568ca2dd044fea4c767c6e4` | 2026-09-21 | Shenghao Chen | all `-` (3) | no | #503 merged | — | T1 |  |
| 1 landed | `chore/fuzz-comment-rot` | `10f36223237765fb93e4d0a121224c692ae12eca` | 2026-09-14 | Shenghao Chen | all `-` (1) | no | #461 merged | — | T1 |  |
| 1 landed | `chore/lint-allow-to-expect` | `fb3fb54b9bbb25586e1f341c7ef6432d7a0302a8` | 2026-09-07 | Shenghao Chen | all `-` (1) | no | #365 merged | — | T1 |  |
| 1 landed | `chore/remove-bdb-claims` | `33c5a3e5a9f143d3f9cb95002081b44e0c32b716` | 2026-08-30 | shenghaoc | all `-` (2) | no | #233 merged | — | T1 |  |
| 1 landed | `chore/remove-oxpinyin-migrate` | `58d899f6f8ff4d5b387b261fbc8c41bd423b9802` | 2026-08-23 | Shenghao Chen | all `-` (6) | no | #161 merged | — | T1 |  |
| 1 landed | `chore/rename-to-oxpinyin` | `2a6a090b38145791828e9e47458cacd7eb771a2b` | 2026-08-16 | Shenghao Chen | all `-` (2) | no | #77 merged | — | T1 |  |
| 1 landed | `chore/restore-stale-bisection-runners` | `b71039b657c9e1372b2b128815cc161cf149ccbb` | 2026-09-20 | Shenghao Chen | all `-` (1) | no | #495 merged | — | T1 |  |
| 1 landed | `ci-store-parallel-matrix` | `161d1d54c4783699c7dafe9c7aa1c57123280a5b` | 2026-08-30 | Shenghao Chen | all `-` (1) | no | #232 merged | — | T1 |  |
| 1 landed | `ci/abi-export-gate` | `9f0ccc6af9b493197ccace58aa571ba2d59cca57` | 2026-09-07 | Shenghao Chen | all `-` (1) | no | #384 merged | — | T1 |  |
| 1 landed | `ci/coverage-expansion` | `557c4ab72d9d7ce918c39038f51dee2fbfcac650` | 2026-09-15 | Shenghao Chen | all `-` (1) | no | #466 merged | — | T1 |  |
| 1 landed | `ci/fuzz-lockfile-locked` | `109de53f1e5e7efd7f40abcd48f3bc38156bd506` | 2026-09-21 | Shenghao Chen | all `-` (2) | no | #507 merged | — | T1 |  |
| 1 landed | `ci/fuzz-to-nightly` | `c7170865d93d4625b5cc3bd8814f5855dff98c4e` | 2026-09-23 | Shenghao Chen | all `-` (3) | no | #514 merged | — | T1 |  |
| 1 landed | `ci/matrix-widen-sanitizer-gate-coverage-floor` | `760901712ced746dba5768c44f1dd6a386ca79fe` | 2026-09-07 | Shenghao Chen | all `-` (2) | no | #385 merged | — | T1 |  |
| 1 landed | `ci/nightly-fuzz-bdb` | `61d51abd5a9f9778cb249557f243b70822581e22` | 2026-09-23 | Shenghao Chen | all `-` (1) | no | #515 merged | — | T1 |  |
| 1 landed | `ci/oracle-provisioning` | `7b0f17503c02e40e55ef9c49729d2efad947212c` | 2026-08-28 | Shenghao Chen | all `-` (4) | no | #172 merged | — | T1 |  |
| 1 landed | `ci/path-filters-cover-inputs` | `bac2dd8c2a2c029f6290455e57689bdd06ae6819` | 2026-09-07 | Shenghao Chen | all `-` (1) | no | #362 merged | — | T1 |  |
| 1 landed | `ci/path-gates` | `51b389844a22d00591a5d7d42b360973ca475284` | 2026-08-30 | Shenghao Chen | all `-` (1) | no | #236 merged | — | T1 |  |
| 1 landed | `ci/portable-lanes-on-a-c-backend` | `181ae31963adefdfd67b99b2ae1aaa391c2198e7` | 2026-09-22 | Shenghao Chen | all `-` (9) | no | #500 merged | — | T1 |  |
| 1 landed | `ci/python-job-debian-container` | `83190d8adbea6c53f50a428edb84c6f2e17ab37e` | 2026-09-04 | Shenghao Chen | all `-` (1) | no | #322 merged | — | T1 |  |
| 1 landed | `ci/python-trixie-everywhere` | `358cf0d9df6990c1cc8730fce411114e86411993` | 2026-09-05 | Shenghao Chen | all `-` (1) | no | #323 merged | — | T1 |  |
| 1 landed | `ci/redundancy-caching` | `0418f8436a0454bea7626638977a2caecae4f0b6` | 2026-09-01 | Shenghao Chen | all `-` (4) | no | #244 merged | — | T1 |  |
| 1 landed | `ci/release-packages` | `5942b3c217a41b6d79a533774ff528b4af27ed76` | 2026-09-06 | Shenghao Chen | all `-` (7) | no | #368 merged | — | T1 |  |
| 1 landed | `ci/trailer-job-names` | `8d1dcf91d1fa7f60184e8000cf8cb03bc126a1ee` | 2026-08-17 | Shenghao Chen | all `-` (1) | no | #108 merged | — | T1 |  |
| 1 landed | `ci/vn-fix-apt` | `2f37f4b73a23f079a220bb8efb81851caa7188bd` | 2026-09-02 | Shenghao Chen | all `-` (1) | no | #277 merged | — | T1 |  |
| 1 landed | `claude/681-alloc-register` | `0522575d350adef7f15a843e791f965c09530737` | 2026-10-09 | Shenghao Chen | all `-` (4) | no | #704 merged | #681 issue closed | T1 |  |
| 1 landed | `claude/681-bigram-iterator-rows` | `b0daaa2235746efd835e827b6b8b6c115ca87efc` | 2026-10-09 | Shenghao Chen | all `-` (8) | no | #705 merged | #681 issue closed | T1 |  |
| 1 landed | `claude/681-string-getter-false` | `ea9dc79b49f2aa6f0297d16a8c2af679fb580d64` | 2026-10-09 | Shenghao Chen | all `-` (9) | no | #708 merged | #681 issue closed | T1 |  |
| 1 landed | `claude/agents-verification-cells` | `ee62835abc2c9e73821463d1b9cbbbbe6206a168` | 2026-10-05 | Shenghao Chen | all `-` (3) | no | #644 merged | — | T1 |  |
| 1 landed | `claude/angry-pascal-41cdd0` | `82c3d39d269a34b097a43219116d35d36c5b1115` | 2026-09-13 | Shenghao Chen | all `-` (2) | no | #454 merged | — | T1 |  |
| 1 landed | `claude/axis-j-kc-tkrzw-ledgers-2d8494` | `c1bc1c74fd9f9ccf3c6f772e0aa4d70ef9bf7f67` | 2026-10-15 | Shenghao Chen | all `-` (3) | no | #789 merged | — | T1 |  |
| 1 landed | `claude/bold-sagan-kain8m` | `18121e08be6369b652f6fa7f611ad485b18ad0e0` | 2026-10-03 | Shenghao Chen | all `-` (20) | no | #593 merged | — | T1 |  |
| 1 landed | `claude/bold-snyder-b64f23` | `090d5592205f9885b8f856bdcaf22b2639e0fe54` | 2026-09-13 | Shenghao Chen | all `-` (1) | no | #459 merged | — | T1 |  |
| 1 landed | `claude/bopomofo-spec-audit-55f56c` | `8752946aa1a609f4bae91804f7166c302cd1cf38` | 2026-09-03 | Shenghao Chen | all `-` (5) | no | #294 merged | — | T1 |  |
| 1 landed | `claude/candidate-list-assembly-gzhlh1` | `3bd8f4b3430161df9e4c00fec857d8f6fc48155c` | 2026-10-02 | Shenghao Chen | all `-` (2) | no | #596 merged | — | T1 |  |
| 1 landed | `claude/candidate-list-assembly-gzhlh1-602` | `ffea3936f192cdd7fd9564a8edcb8407a7288df3` | 2026-10-02 | Shenghao Chen | all `-` (6) | no | #609 merged | #602 issue closed | T1 |  |
| 1 landed | `claude/candidate-list-assembly-gzhlh1-row34` | `1830c94c992019883888af9559bcdf711c3b9db7` | 2026-10-02 | Claude | all `-` (3) | no | #598 merged | — | T1 |  |
| 1 landed | `claude/candidate-list-assembly-gzhlh1-row37` | `32342ec471302200288387dda85a6a7c3b7d7077` | 2026-10-02 | Claude | all `-` (4) | no | #604 merged | — | T1 |  |
| 1 landed | `claude/capi-twins-marshalling-c88d06` | `13c103a72d14443de31d74c45384a5a3a6856b1f` | 2026-09-10 | Shenghao Chen | all `-` (1) | no | #412 merged | — | T1 |  |
| 1 landed | `claude/cool-hertz-600ca5` | `26185f583f4fe9adb77cec7ac88dfa97d9621cc5` | 2026-09-03 | Shenghao Chen | all `-` (1) | no | #306 merged | — | T1 |  |
| 1 landed | `claude/dependency-trim-audit-1de972` | `c11d8730f8e9b9c6fc1e79c5913d3641f76ce0ab` | 2026-09-05 | Shenghao Chen | all `-` (7) | no | #338 merged | — | T1 |  |
| 1 landed | `claude/docs-fullpin-aux-overread` | `1ee13f7288975820f9118f0c205555e975cf70d6` | 2026-08-20 | Shenghao Chen | all `-` (2) | no | #130 merged | — | T1 |  |
| 1 landed | `claude/docs-oracle-bisect-assertion-3o5lkq` | `47377dcdbb1c3c7eb34414de5bb0c04609a20558` | 2026-08-18 | Shenghao Chen | all `-` (2) | no | #114 merged | — | T1 |  |
| 1 landed | `claude/eager-cray-uwgq0n` | `8ede10aa3cbb76a620fe93308568cc76903c7140` | 2026-10-03 | Shenghao Chen | all `-` (2) | no | #621 merged | — | T1 |  |
| 1 landed | `claude/eager-cray-uwgq0n-536` | `d4bb745509da92004bcc7bd560e31200ad545621` | 2026-10-03 | Shenghao Chen | all `-` (7) | no | #624 merged | #536 issue closed | T1 |  |
| 1 landed | `claude/eager-cray-uwgq0n-547` | `f784859c66db558bc78668b801e437d3f1bc0577` | 2026-10-03 | Claude | all `-` (3) | no | #622 merged | #547 issue closed | T1 |  |
| 1 landed | `claude/eager-cray-uwgq0n-577` | `d5e0056c567ec6a6c105a5ee97724eeefb4d8b07` | 2026-10-03 | Shenghao Chen | all `-` (5) | no | #623 merged | #577 issue closed | T1 |  |
| 1 landed | `claude/engineering-architecture-c20f85` | `ddd3a3c567c953f83c3faf4ad9bf9d8e75cc3202` | 2026-09-08 | Shenghao Chen | all `-` (9) | no | #396 merged | — | T1 |  |
| 1 landed | `claude/engineering-documentation-4c4d18` | `ebe641c4c0a10bc3b0a84f6eee12517d0c88a8a6` | 2026-09-12 | Shenghao Chen | all `-` (7) | no | #435 merged | — | T1 |  |
| 1 landed | `claude/exciting-hopper-a6442f` | `b0ecf1b4e35fa67243aee656cf7a849c9c335059` | 2026-09-10 | Shenghao Chen | all `-` (3) | no | #413 merged | — | T1 |  |
| 1 landed | `claude/frozen-allocator-registers-ad750e` | `49a1c46e9ac466a77006eb6f21a47a8e56710eec` | 2026-10-14 | Claude | all `-` (2) | no | #782 merged | — | T1 |  |
| 1 landed | `claude/fuzz-target-build-coverage-653f30` | `401aaada107ad771804ba686240644f77394b080` | 2026-09-03 | Shenghao Chen | all `-` (1) | no | #305 merged | — | T1 |  |
| 1 landed | `claude/glib-ffi-glib-sys-9kaiii` | `0b8f63b252baa8b8f9767d259e1c62f122a4be49` | 2026-09-03 | Shenghao Chen | all `-` (1) | no | #298 merged | — | T1 |  |
| 1 landed | `claude/harness-q1-backend-default` | `903c8718726420633af1220c3ec2b56ae33e241c` | 2026-10-03 | Shenghao Chen | all `-` (30) | no | #597 merged | — | T1 |  |
| 1 landed | `claude/infallible-wozniak-38ea41` | `fbce254c437d8b6dabc61dfad64fa8674b01269d` | 2026-09-02 | Shenghao Chen | all `-` (2) | no | #290 merged | — | T1 |  |
| 1 landed | `claude/inspiring-elbakyan-e0b4d5` | `cd36f92444f41f06cc7df6d621a7a138d38e06e4` | 2026-09-05 | Shenghao Chen | all `-` (3) | no | #326 merged | — | T1 |  |
| 1 landed | `claude/issue-619-empty-user-dir-bbccc8` | `168a33b31f71a8dba015795bf66431a2bfcb4994` | 2026-10-04 | Shenghao Chen | all `-` (2) | no | #646 merged | #619 issue closed | T1 |  |
| 1 landed | `claude/kc-perf-re-baseline-b69da4` | `74d6f4be1a2c38d7c973bad72d20b0d4feb3b487` | 2026-09-04 | Shenghao Chen | all `-` (4) | no | #311 merged | — | T1 |  |
| 1 landed | `claude/keen-khayyam-a92609` | `631ad16faa9898e0a6b9f12d9254d90866360168` | 2026-09-05 | Shenghao Chen | all `-` (2) | no | #325 merged | — | T1 |  |
| 1 landed | `claude/kyoto-cabinet-default-nlgdug` | `204c061017fa562a60037b495a03c1ed847202df` | 2026-08-30 | Shenghao Chen | all `-` (10) | no | #243 merged | — | T1 |  |
| 1 landed | `claude/lane-c-525-guards-candidates` | `821071dc2b36c33aed25dbb8217940e4a899ecfb` | 2026-10-06 | Shenghao Chen | all `-` (14) | no | #660 merged | #525 issue closed | T1 |  |
| 1 landed | `claude/lane-c-525-guards-config` | `a21faf722766d6547365991c4229b5748f2eb606` | 2026-10-06 | Shenghao Chen | all `-` (12) | no | #658 merged | #525 issue closed | T1 |  |
| 1 landed | `claude/lane-c-525-guards-cursor` | `4ab6110052715e3fa04480c63ca0c6a3b8f894c1` | 2026-10-06 | Shenghao Chen | all `-` (13) | no | #659 merged | #525 issue closed | T1 |  |
| 1 landed | `claude/lane-c-525-guards-tone` | `08a077448c4b7c59031fc61e3aa644b0471d47ca` | 2026-10-06 | Shenghao Chen | all `-` (16) | no | #662 merged | #525 issue closed | T1 |  |
| 1 landed | `claude/lane-c-525-guards-train` | `505c9340331e1b44530afee7e59f9f318b84fed3` | 2026-10-06 | Shenghao Chen | all `-` (15) | no | #661 merged | #525 issue closed | T1 |  |
| 1 landed | `claude/lane-c-525-guards-zhuyin` | `850fc5d0a7a175b05dafaaa073a80b9754c02a49` | 2026-10-06 | Shenghao Chen | all `-` (17) | no | #663 merged | #525 issue closed | T1 |  |
| 1 landed | `claude/lane-c-528-alloc-after-fini` | `037c9f18a2d096d8716d2ef738b874e8140848f2` | 2026-10-06 | Shenghao Chen | all `-` (10) | no | #656 merged | #528 issue closed | T1 |  |
| 1 landed | `claude/lane-c-532-use-tone` | `70140d2cbaaa64d59bd921fa1cf61bd7717118eb` | 2026-10-06 | Shenghao Chen | all `-` (1) | no | #647 merged | #532 issue closed | T1 |  |
| 1 landed | `claude/lane-c-540-choose-predicted` | `1546113beff5c8858777435167d38f96e36e8084` | 2026-10-06 | Shenghao Chen | all `-` (8) | no | #654 merged | #540 issue closed | T1 |  |
| 1 landed | `claude/lane-c-540-input-cap` | `05206e3991ab1b151b1f7988cc5ef9882405f6fb` | 2026-10-06 | Shenghao Chen | all `-` (9) | no | #655 merged | #540 issue closed | T1 |  |
| 1 landed | `claude/lane-c-540-total-wrap` | `f31cc314aa0bfc54c0174f3a7657ed01e1abe7c8` | 2026-10-06 | Shenghao Chen | all `-` (11) | no | #657 merged | #540 issue closed | T1 |  |
| 1 landed | `claude/lane-c-540-unigram-total` | `bd9b05624ab2bc596b07867a85967c0483ef45b5` | 2026-10-06 | Shenghao Chen | all `-` (7) | no | #653 merged | #540 issue closed | T1 |  |
| 1 landed | `claude/lane-c-542-aux-text` | `8f6820c6c67d42e5556dd7edcb0b3a490a9ba3cb` | 2026-10-06 | Shenghao Chen | all `-` (6) | no | #652 merged | #542 issue closed | T1 |  |
| 1 landed | `claude/lane-c-542-key-slots` | `77aa2e8a673f5d2ec33108fe2e48ac603fede109` | 2026-10-06 | Shenghao Chen | all `-` (4) | no | #650 merged | #542 issue closed | T1 |  |
| 1 landed | `claude/lane-c-542-out-params` | `d7c7aaf1d54e8d5f4744aabba74a2a0e287b4907` | 2026-10-06 | Shenghao Chen | all `-` (3) | no | #649 merged | #542 issue closed | T1 |  |
| 1 landed | `claude/lane-c-542-sentence-unload` | `15c8485031f07f88370a45b8d71c479318356ff6` | 2026-10-06 | Shenghao Chen | all `-` (5) | no | #651 merged | #542 issue closed | T1 |  |
| 1 landed | `claude/lane-c-542-zhuyin-twins` | `7827efe766f898cb45e546f2acaf0ce6e09b993c` | 2026-10-06 | Shenghao Chen | all `-` (19) | no | #665 merged | #542 issue closed | T1 |  |
| 1 landed | `claude/lane-c-545-save-diagnostics` | `23ab42062b9fb8103271b96d9c6f017c82d5ba27` | 2026-10-06 | Shenghao Chen | all `-` (20) | no | #666 merged | #545 issue closed | T1 |  |
| 1 landed | `claude/lane-c-587-invalid-utf8` | `8c021f9122a0975c26ca4869db94317e993190cf` | 2026-10-06 | Shenghao Chen | all `-` (2) | no | #648 merged | #587 issue closed | T1 |  |
| 1 landed | `claude/lane-c-587-raw-paths` | `0baeb13104d8ac94c0eeb6a3765d3b209722e20a` | 2026-10-06 | Shenghao Chen | all `-` (18) | no | #664 merged | #587 issue closed | T1 |  |
| 1 landed | `claude/leading-apostrophe-pinyin-px5hw5` | `9989512dec4596ce2635d222defb43c15fb8301b` | 2026-08-21 | Shenghao Chen | all `-` (2) | no | #137 merged | — | T1 |  |
| 1 landed | `claude/lucid-gates-p7kfq9` | `d198f5e13fed228254fc5da3ab2b9d007a60602a` | 2026-10-01 | Claude | all `-` (2) | no | #618 merged | — | T1 |  |
| 1 landed | `claude/measure-fsync-cost-ac6300` | `72d1855cbb30b32f2c3e8efce24f996df174d840` | 2026-09-10 | Shenghao Chen | all `-` (6) | no | #411 merged | — | T1 |  |
| 1 landed | `claude/modest-zhukovsky-e8c84f` | `556b49c8a2c158a204f15334e1a614d44cab1543` | 2026-09-05 | Shenghao Chen | all `-` (1) | no | #329 merged | — | T1 |  |
| 1 landed | `claude/nifty-goodall-ca3kai` | `0d30e037713ba7b31e8b14b81d8019a1ce32e45f` | 2026-09-30 | shenghaoc | all `-` (4) | no | #595 merged | — | T1 |  |
| 1 landed | `claude/nifty-goodall-ca3kai-534` | `ef47140bba1d468a707e3caff17a7bd26e4ec588` | 2026-09-30 | shenghaoc | all `-` (6) | no | #605 merged | #534 issue closed | T1 |  |
| 1 landed | `claude/nifty-goodall-ca3kai-541-order` | `4c17a11c786e1b1d8ca4eabb2da726ea540aefe5` | 2026-09-30 | shenghaoc | all `-` (13) | no | #608 merged | #541 issue closed | T1 |  |
| 1 landed | `claude/nifty-goodall-ca3kai-541-row36` | `c71b32d6b7ad976603ad2079c0a10b521819b208` | 2026-09-30 | shenghaoc | all `-` (10) | no | #607 merged | #541 issue closed | T1 |  |
| 1 landed | `claude/nifty-goodall-ca3kai-575` | `5cb6c01af167164e90b245b46c66519cae221d23` | 2026-09-30 | Claude | all `-` (7) | no | #606 merged | #575 issue closed | T1 |  |
| 1 landed | `claude/nifty-goodall-ca3kai-599` | `9998ced9c71d5ef1544825bbd7e964158367d790` | 2026-09-30 | shenghaoc | all `-` (22) | no | #612 merged | #599 issue closed | T1 |  |
| 1 landed | `claude/nifty-goodall-ca3kai-600` | `33fdbb44a7ffc3467080af37712a8a20a28bdc29` | 2026-09-30 | shenghaoc | all `-` (16) | no | #610 merged | #600 issue closed | T1 |  |
| 1 landed | `claude/nifty-goodall-ca3kai-601` | `4680a384744eda94d7ceaee366f5c650cec72bf2` | 2026-09-30 | Shenghao Chen | all `-` (17) | no | #611 merged | #601 issue closed | T1 |  |
| 1 landed | `claude/offset-decode-context-xdrlsb` | `b8ceb020d2f2904e4a7a72737f8edf892d688592` | 2026-08-21 | Shenghao Chen | all `-` (3) | no | #138 merged | — | T1 |  |
| 1 landed | `claude/optimistic-nobel-adf2ab` | `00fdafd505050f3a68d2d7ea15ec48994697a50c` | 2026-09-11 | Shenghao Chen | all `-` (2) | no | #419 merged | — | T1 |  |
| 1 landed | `claude/oxpinyin-allocator-pairing-gate-64a1f8` | `6b881866f649bfb22b1168be64aea2817beecce3` | 2026-09-10 | Shenghao Chen | all `-` (2) | no | #410 merged | — | T1 |  |
| 1 landed | `claude/oxpinyin-issue-356-liyacd` | `12b4f0f1a810ca357ab9e0a8e5141d62bd5ed509` | 2026-09-06 | Shenghao Chen | all `-` (2) | no | #367 merged | #356 issue closed | T1 |  |
| 1 landed | `claude/oxpinyin-issue-357-dbgdxi` | `181ea2893f06a7baac540fe8ab0b3b0fe7aeeeb3` | 2026-09-07 | Shenghao Chen | all `-` (2) | no | #378 merged | #357 issue closed | T1 |  |
| 1 landed | `claude/oxpinyin-issue-358-0hcjoh` | `4cbeee5b97dd9d4aeaafc803dd618f917fc0bb0e` | 2026-09-06 | Shenghao Chen | all `-` (3) | no | #366 merged | #358 issue closed | T1 |  |
| 1 landed | `claude/oxpinyin-issue-369-8qd429` | `7554ca10da30206e8948ae6204b0e3540435bb43` | 2026-09-07 | Shenghao Chen | all `-` (2) | no | #377 merged | #369 issue closed | T1 |  |
| 1 landed | `claude/oxpinyin-issue-370-v9rb6j` | `1d0884a7850cb83121297cdb072cf4a2447959c3` | 2026-09-07 | Shenghao Chen | all `-` (1) | no | #376 merged | #370 issue closed | T1 |  |
| 1 landed | `claude/oxpinyin-issue-401-a16b4e` | `5c0d53ed31241a4e07c4a974586f478d7e1e1265` | 2026-09-11 | Shenghao Chen | all `-` (3) | no | #417 merged | #401 issue closed | T1 |  |
| 1 landed | `claude/oxpinyin-issue-402-ea9b12` | `5b7c5027f19e64182c66e3482484846ca6a58304` | 2026-09-11 | Shenghao Chen | all `-` (2) | no | #418 merged | #402 issue closed | T1 |  |
| 1 landed | `claude/oxpinyin-issue-403-915089` | `e8d7d6fda49d86d376d969ed2d8f99d34d47b20e` | 2026-09-11 | Shenghao Chen | all `-` (3) | no | #416 merged | #403 issue closed | T1 |  |
| 1 landed | `claude/oxpinyin-issue-403-follow-up` | `9911ebc793d89c89357e1e1a2c48dc8fb0429ff0` | 2026-09-11 | Shenghao Chen | all `-` (6) | no | #427 merged | #403 issue closed | T1 |  |
| 1 landed | `claude/oxpinyin-shared-row-schema-410b86` | `e16bdf378a688ef4425dcf564e33bb5a5f0bedc7` | 2026-09-10 | Shenghao Chen | all `-` (2) | no | #409 merged | — | T1 |  |
| 1 landed | `claude/perf-gate-rejected` | `da7b8419c9565c24cc65baed2d98e72fdfb58d18` | 2026-09-12 | Shenghao Chen | all `-` (2) | no | #432 merged | — | T1 |  |
| 1 landed | `claude/perf-nightly-lane` | `4127645b8ccccc3e9b896ea3c626d2af2ab2144b` | 2026-09-12 | Shenghao Chen | all `-` (5) | no | #433 merged | — | T1 |  |
| 1 landed | `claude/perf-size-ci-gate-294dc1` | `d35704e56fb52fed2345704a30c4e66953702ebb` | 2026-09-09 | Shenghao Chen | all `-` (2) | no | #407 merged | — | T1 |  |
| 1 landed | `claude/pr-245-readme-updates-bfr0xk` | `9f348d11733e64bc2246a4585d0a5038ab3c4ec6` | 2026-08-31 | Shenghao Chen | all `-` (4) | no | — | #245 PR merged | T1 |  |
| 1 landed | `claude/pr-stack-277-279-ygg9e9` | `c6bc47345b534422d178431aa7361ef28e571055` | 2026-09-02 | Shenghao Chen | all `-` (1) | no | #292 merged, #282 closed | #277 PR merged; #279 PR merged | T1 |  |
| 1 landed | `claude/pr172-debian-testing-rewrite-xm0rnc` | `7b0f17503c02e40e55ef9c49729d2efad947212c` | 2026-08-28 | Shenghao Chen | all `-` (4) | no | — | — | T1 |  |
| 1 landed | `claude/pr2-compatibility-policy` | `f99501117877fb45f59285eb0873ae729c43fdbf` | 2026-08-29 | Shenghao Chen | all `-` (16) | no | #203 merged | — | T1 |  |
| 1 landed | `claude/pr3-soname-consumer-union` | `29bc4cc583e10c5260f0b805a8a697619dd3f65e` | 2026-08-29 | Shenghao Chen | all `-` (25) | no | #206 merged | — | T1 |  |
| 1 landed | `claude/pr4-berkeleydb-compat` | `a6d0a7067e233762d4a924bfc8de6525350af26f` | 2026-08-29 | Shenghao Chen | all `-` (38) | no | #207 merged | — | T1 |  |
| 1 landed | `claude/pr5-revert-incompatible-divergences` | `0c28ac673d9e09cdba918fdd31d360c6ebcece76` | 2026-08-29 | Shenghao Chen | all `-` (39) | no | #209 merged | — | T1 |  |
| 1 landed | `claude/practical-gagarin-e819a8` | `c78a3284b825ec1312a746f5fe8fa06c29dc2e59` | 2026-09-05 | Shenghao Chen | all `-` (2) | no | #327 merged | — | T1 |  |
| 1 landed | `claude/prs-252-258-review-gbqq5e` | `a41645ca2c1e5a46fdbc0a8a3f65945de588235a` | 2026-09-02 | Shenghao Chen | all `-` (24) | no | — | #252 PR merged; #258 PR merged | T1 |  |
| 1 landed | `claude/replace-g-free-glib-sys-pa4xbu` | `27f9137627b6c805306250beec8fcf923009c183` | 2026-09-03 | Shenghao Chen | all `-` (1) | no | #299 merged | — | T1 |  |
| 1 landed | `claude/review-followups-2026-09-08` | `6c08f3b5169f4c459174ffb6120a2fdee202a2a7` | 2026-09-08 | Shenghao Chen | all `-` (4) | no | #398 merged | #9 PR merged | T1 |  |
| 1 landed | `claude/roadmap-register-sync-h961wh` | `3e5929233f83eb4591ede0302b71267610cd111b` | 2026-08-31 | Shenghao Chen | all `-` (2) | no | #259 merged | — | T1 |  |
| 1 landed | `claude/rust-libpinyin-trainer-parity-46tiht` | `a41645ca2c1e5a46fdbc0a8a3f65945de588235a` | 2026-09-02 | Shenghao Chen | all `-` (24) | no | #258 merged | — | T1 |  |
| 1 landed | `claude/rust-libpinyin-trainer-parity-46tiht-1-audit` | `ba82981979756167a9b4f7093fba25a8528c50d3` | 2026-09-02 | Shenghao Chen | all `-` (2) | no | #252 merged | — | T1 |  |
| 1 landed | `claude/rust-libpinyin-trainer-parity-46tiht-2-segment` | `2d42577fd073185f68572e184f3ca3e164081663` | 2026-09-02 | Shenghao Chen | all `-` (3) | no | #253 merged | — | T1 |  |
| 1 landed | `claude/rust-libpinyin-trainer-parity-46tiht-3-kmm` | `e6f1f06acd6e3b3971489559b2591684628b312b` | 2026-09-02 | Shenghao Chen | all `-` (10) | no | #254 merged | — | T1 |  |
| 1 landed | `claude/rust-libpinyin-trainer-parity-46tiht-4-punct-word` | `315b3069163e1be22de7045332bc021f2920e274` | 2026-09-02 | Shenghao Chen | all `-` (13) | no | #255 merged | — | T1 |  |
| 1 landed | `claude/rust-libpinyin-trainer-parity-46tiht-5-eval` | `49f3971dfd7e0e13a2623b974617131282d15d88` | 2026-09-02 | Shenghao Chen | all `-` (16) | no | #256 merged | — | T1 |  |
| 1 landed | `claude/rust-libpinyin-trainer-parity-46tiht-6-train` | `b57de5fa870efecd118694d31fb2a1d536d00da1` | 2026-09-02 | Shenghao Chen | all `-` (19) | no | #257 merged | — | T1 |  |
| 1 landed | `claude/separate-libpinyin-libzhuyin-7acdbb` | `44fc35f33429e4a08a7a81119b78bdaffde874de` | 2026-09-03 | Shenghao Chen | all `-` (1) | no | #293 merged | — | T1 |  |
| 1 landed | `claude/session-state-decomposition-c46010` | `68aa4aa567d58077e53d2335e56359fa984b26e5` | 2026-09-10 | Shenghao Chen | all `-` (1) | no | #414 merged | — | T1 |  |
| 1 landed | `claude/sharp-noether-0yzmwh` | `5a94a4823178b0121e42fbf62f9c80b9d521e7ba` | 2026-08-30 | Shenghao Chen | all `-` (8) | no | #241 merged | — | T1 |  |
| 1 landed | `claude/tkrzw-134-sentinel-finding` | `8db32913faff862cef18bf028ff7e35778de903c` | 2026-10-05 | Shenghao Chen | all `-` (3) | no | #678 merged | #134 PR merged | T1 |  |
| 1 landed | `claude/tkrzw-distro-compat-9o3k8h` | `506677434dd146e5d209272be27900e5bc8c9eb3` | 2026-08-29 | Shenghao Chen | all `-` (12) | no | #202 merged | — | T1 |  |
| 1 landed | `claude/transformed-reanchor` | `354d29b533cb4e0d44e6a20127639ba0ada7a0ba` | 2026-10-03 | Shenghao Chen | all `-` (9) | no | #625 merged | — | T1 |  |
| 1 landed | `claude/vigilant-planck-nwa9p7` | `9c0892f7283372fb7cf9efcda9220b7f03099361` | 2026-09-26 | Claude | all `-` (2) | no | #592 merged | — | T1 |  |
| 1 landed | `claude/w5-t0-abi-subset-h5aesd` | `ba25ff7f4da8456ad2a0404e2bdb1d3206afb37d` | 2026-08-15 | Shenghao Chen | all `-` (3) | no | #41 merged | — | T1 |  |
| 1 landed | `codex/747-sentence-tie-order` | `bc5b290370e0cb088042f55e2eed4cacbdf4898d` | 2026-10-12 | Shenghao Chen | all `-` (1) | no | #749 merged | #747 issue closed | T1 |  |
| 1 landed | `codex/fix-588-lazy-originals` | `abd2000b56e14902c0ec9f19461b9ff4f3f9d2cb` | 2026-10-11 | Shenghao Chen | all `-` (2) | no | #742 merged | #588 issue closed | T1 |  |
| 1 landed | `codex/import-token-bound` | `74a1193ade95ba1ee6558e5e08b46514b94ecd0a` | 2026-10-08 | Shenghao Chen | all `-` (4) | no | #702 merged | — | T1 |  |
| 1 landed | `codex/init-empty-system` | `6ca857d24c5c054242d9a2a69e5793e4bf64b27e` | 2026-10-08 | Shenghao Chen | all `-` (1) | no | #693 merged | — | T1 |  |
| 1 landed | `codex/init-null-user` | `429a840cd6b5dd651765c84cd98e4c4865222f85` | 2026-10-08 | Shenghao Chen | all `-` (3) | no | #701 merged | — | T1 |  |
| 1 landed | `codex/issue677-phase2` | `7dbab351280301cd62d46a415885be1830128fdc` | 2026-10-14 | Shenghao Chen | all `-` (1) | no | #763 merged | — | T1 |  |
| 1 landed | `codex/lane-594-final-step` | `5a09440dbc964642492b085d628cd6f1c52f8bcc` | 2026-10-10 | Shenghao Chen | all `-` (4) | no | #721 merged | #594 issue closed | T1 |  |
| 1 landed | `codex/lane-710-parse-constraints` | `4fd27a94fdd723aba1ca23f9fef3c9de01e9d7a9` | 2026-10-09 | Shenghao Chen | all `-` (2) | no | #714 merged | #710 issue closed | T1 |  |
| 1 landed | `codex/lane-744-bdb-mutex` | `8da09e51baa72628e294a7a65a522431cbdfb3a7` | 2026-10-11 | Shenghao Chen | all `-` (2) | no | #745 merged | #744 issue closed | T1 |  |
| 1 landed | `codex/lane-a-pinyin-train-index-524` | `c883b7e57e351e90e45addbd082f88044e6f6447` | 2026-10-04 | Shenghao Chen | all `-` (2) | no | #633 merged | #524 issue closed | T1 |  |
| 1 landed | `codex/lane-d-probe-repairs` | `5aa35c38348ffd813e92d0351c79dcb9b912717f` | 2026-10-04 | Shenghao Chen | all `-` (3) | no | #632 merged | — | T1 |  |
| 1 landed | `codex/lane-d-sentence-gate-coverage` | `6d925e16af3eeab12ffdb505006d45d5568a4daa` | 2026-10-04 | Shenghao Chen | all `-` (5) | no | #637 merged | — | T1 |  |
| 1 landed | `codex/lane-d-trellis-records` | `5e56e76176de23e8d96496883af14abc90cf94ee` | 2026-10-04 | Shenghao Chen | all `-` (1) | no | #635 merged | — | T1 |  |
| 1 landed | `codex/lane-d-trellis-selection` | `bbc8340bc50154f17984551da85d1ae0c93ad8d3` | 2026-10-04 | Shenghao Chen | all `-` (4) | no | #636 merged | — | T1 |  |
| 1 landed | `codex/lane-i-key-aware-train` | `6b6023562ff42e52a2d96c9c22b52a35e87eb8b6` | 2026-10-02 | Shenghao Chen | all `-` (9) | no | #616 merged | — | T1 |  |
| 1 landed | `codex/lane-i-nbest-train` | `dbf8474097b16f213984aa4d0b0bf74329016ddc` | 2026-10-02 | Shenghao Chen | all `-` (8) | no | #614 merged | — | T1 |  |
| 1 landed | `codex/lane-j-transformed-fuzzy-matrix` | `5d0e17baf328b4058257e61280273430f4796153` | 2026-10-04 | Shenghao Chen | all `-` (4) | no | #634 merged | — | T1 |  |
| 1 landed | `codex/remember-tones` | `45f0a2eccf19423185f228276fad83cb80ae5c35` | 2026-10-08 | Shenghao Chen | all `-` (2) | no | #700 merged | — | T1 |  |
| 1 landed | `codex/row58-predicted-candidates` | `66a1e42d18f8ced92388c1eac481c1b65efd6216` | 2026-10-09 | Shenghao Chen | all `-` (3) | no | #712 merged | — | T1 |  |
| 1 landed | `codex/tools-01-fixtures` | `e12b8d48fea0d094886e886929aada1f4e64bca2` | 2026-10-07 | Shenghao Chen | all `-` (2) | no | #667 merged | #1 PR merged | T1 |  |
| 1 landed | `codex/tools-02-cell-targets` | `151063e6c31c4015b25f6c626117af636326fd61` | 2026-10-07 | Shenghao Chen | all `-` (3) | no | #668 merged | #2 PR merged | T1 |  |
| 1 landed | `codex/tools-03-surface-dev` | `d64b67063dbde0ff50efda8a400a758f00a69e69` | 2026-10-07 | Shenghao Chen | all `-` (4) | no | #670 merged | #3 PR merged | T1 |  |
| 1 landed | `codex/tools-04-w11-contract` | `c35cdd54ec0666af41ca74602e279b889801dde1` | 2026-10-07 | Shenghao Chen | all `-` (6) | no | #671 merged | #4 PR merged | T1 |  |
| 1 landed | `codex/tools-05-prebuilt-parity` | `2b967546aa929e56af7a1fb9d6e836a55ec64a6c` | 2026-10-07 | Shenghao Chen | all `-` (9) | no | #672 merged | #5 PR merged | T1 |  |
| 1 landed | `codex/tools-06-isolate-gates` | `87a478d8b54c4dd493d8fae8674b7ea32f039a84` | 2026-10-07 | Shenghao Chen | all `-` (12) | no | #673 merged | #6 PR merged | T1 |  |
| 1 landed | `codex/tools-07-aggregate` | `cd776c354d35dcf4a5f5adfc446b89ba8a77ca6a` | 2026-10-07 | Shenghao Chen | all `-` (18) | no | #674 merged | #7 PR merged | T1 |  |
| 1 landed | `codex/tools-08-gate-docs` | `9a6ec60697c04f40033daa1dd11c6dfa6d09a8c8` | 2026-10-08 | Shenghao Chen | all `-` (22) | no | #675 merged | — | T1 |  |
| 1 landed | `codex/training-tooling-consolidation` | `21f15f35e99686ac75eeeb391b083fd9e0fc0dc8` | 2026-10-05 | Shenghao Chen | all `-` (1) | no | #679 merged | — | T1 |  |
| 1 landed | `config/dependabot-groups` | `03162e902b7691098a61416e502a9dacb4267a2e` | 2026-09-06 | Shenghao Chen | all `-` (1) | no | #347 merged | — | T1 |  |
| 1 landed | `cursor/classify-probe-residues-ce63` | `b0f4a40dd270d20ca979f56d7c4b9f66277d07f6` | 2026-09-19 | Shenghao Chen | all `-` (1) | no | #486 closed | — | T1 |  |
| 1 landed | `cursor/decode-path-bench-baseline-4469` | `408da7290d328ff72231dda164ab1943a33d2da9` | 2026-09-19 | Shenghao Chen | all `-` (4) | no | #487 merged | — | T1 |  |
| 1 landed | `cursor/lint-commits-drop-r1-a24a` | `d3d0a1a12e7ff1838c5d00ded1a04e23cd5e49a9` | 2026-09-21 | Shenghao Chen | all `-` (1) | no | #511 merged | — | T1 |  |
| 1 landed | `cursor/register-probe-residues-ce63` | `9f1b7a0ca6cd91a6bc00d9e6b4914928025a07f7` | 2026-09-19 | Shenghao Chen | all `-` (5) | no | #488 merged | — | T1 |  |
| 1 landed | `data-pipeline-parity` | `b67f1b03b50ff5645cac3316b16536bbf9410462` | 2026-08-27 | Shenghao Chen | all `-` (6) | no | #180 merged | — | T1 |  |
| 1 landed | `datagen-kc-conformance` | `66a69deb9c0a078253534379b910ac37f4052b2a` | 2026-08-30 | Shenghao Chen | all `-` (1) | no | #235 merged | — | T1 |  |
| 1 landed | `deps/phf-0.14` | `580fd2708b3058b81355075c48f49d09eb2d58f0` | 2026-09-06 | Shenghao Chen | all `-` (1) | no | #350 merged | — | T1 |  |
| 1 landed | `diag/residue-a-root` | `696561eab9086f69de5daa430a6d0e63558e9146` | 2026-09-19 | Shenghao Chen | all `-` (6) | no | #489 merged | — | T1 |  |
| 1 landed | `docs/525-ledger-rows-93-95` | `1ab099e338a4f3475e50ce9edd1025c23011000d` | 2026-10-15 | Shenghao Chen | all `-` (1) | no | #795 merged | #525 issue closed; #93 PR merged; #95 PR merged | T1 |  |
| 1 landed | `docs/agents-bench-targets` | `005d0d9a9d9a5e09b80fbaecc22c42b17ee1286b` | 2026-09-06 | Shenghao Chen | all `-` (1) | no | #346 merged | — | T1 |  |
| 1 landed | `docs/agents-rebase-scheme-staleness` | `b48089e0e38bd04376c64b29f8b5e8d3ff04a59b` | 2026-09-07 | Shenghao Chen | all `-` (2) | no | #371 merged | — | T1 |  |
| 1 landed | `docs/agents-stack-placement-rule` | `046d6b9784be0719aa7646e8b4afec8c4ae5f59d` | 2026-09-23 | Shenghao Chen | all `-` (2) | no | #513 merged | — | T1 |  |
| 1 landed | `docs/agents-stop-clearance-rule` | `f1b95bab1ee288f2b618ce0489be9a6276357387` | 2026-09-05 | Shenghao Chen | all `-` (1) | no | #328 merged | — | T1 |  |
| 1 landed | `docs/audit-policy-548-550` | `b4ebd2bd503c8b6ba1cff7a90db12814fd5427d4` | 2026-10-11 | Shenghao Chen | all `-` (2) | no | #743 merged | #548 issue closed; #550 issue closed | T1 |  |
| 1 landed | `docs/audit-stale-records` | `d5e5520e1680de326eff7914b7aea3d251dc828c` | 2026-09-14 | Shenghao Chen | all `-` (2) | no | #464 merged | — | T1 |  |
| 1 landed | `docs/axis-j-bdb-566` | `0cd51cd1424bb1a2a566fbe7ca94f83218685747` | 2026-10-13 | Shenghao Chen | all `-` (2) | no | #750 merged | #566 issue closed | T1 |  |
| 1 landed | `docs/bopomofo-spec-freeze` | `fa2755cb61782e4a3da5443dbb8344f684b58ee1` | 2026-09-06 | Shenghao Chen | all `-` (1) | no | #353 merged | — | T1 |  |
| 1 landed | `docs/bug-for-bug-audit-r2-2026-09-23` | `dc3ec1e14432f9179cef66e14fc248dc0818d89b` | 2026-09-26 | Shenghao Chen | all `-` (2) | no | #522 merged | #9 PR merged; #23 PR merged | T1 |  |
| 1 landed | `docs/candidate-construction-spec` | `590d5f89925cb6ff09482299796aadcb3bb1a3b3` | 2026-08-12 | Shenghao Chen | all `-` (1) | no | #33 merged | — | T1 |  |
| 1 landed | `docs/ci-path-gate-comment` | `b0312d5c69673ed2011e43a18558ff512cd65dd1` | 2026-09-20 | Shenghao Chen | all `-` (2) | no | #494 merged | — | T1 |  |
| 1 landed | `docs/ci-required-three-checks` | `f8910eaf35886f48dea9e8c25a78355f9f75d61d` | 2026-09-21 | Shenghao Chen | all `-` (1) | no | #506 merged | — | T1 |  |
| 1 landed | `docs/commit-hygiene-failures` | `15d5f421b54ced1dcfe3c4c0b0d3c089a5e0ac94` | 2026-08-17 | Shenghao Chen | all `-` (2) | no | #106 merged | — | T1 |  |
| 1 landed | `docs/crumbs-post-w11` | `b18df609d31be43db09ce21144cd0c041266b3ce` | 2026-08-17 | Shenghao Chen | all `-` (2) | no | #110 merged | — | T1 |  |
| 1 landed | `docs/drop-gsettings-refs` | `5c741a8c9c0b8b946bffd97604219d3ccfd1f5d2` | 2026-09-06 | Shenghao Chen | all `-` (1) | no | #352 merged | — | T1 |  |
| 1 landed | `docs/drop-in-user-data-scope` | `2992f11ba084c7b89da566c80f7a53cde2540ff5` | 2026-09-11 | Shenghao Chen | all `-` (2) | no | #400 merged | — | T1 |  |
| 1 landed | `docs/error-handling-audit` | `2d31ac3607ae456705b03961a2ddaac63693b1c1` | 2026-08-28 | Shenghao Chen | all `-` (5) | no | #196 merged | — | T1 |  |
| 1 landed | `docs/fifth-peer-drop-in-naming` | `2893ebe1500cc2cdbe45e907671c86fbd28e7e88` | 2026-09-13 | Shenghao Chen | all `-` (3) | no | #453 merged | — | T1 |  |
| 1 landed | `docs/findings-no-committed-captures` | `80fd9ff7e891f0600c67ff25fdaa0d97cf7a95c6` | 2026-09-09 | Shenghao Chen | all `-` (1) | no | #405 merged | — | T1 |  |
| 1 landed | `docs/findings-send-sync-audit` | `eff8d2e6c3da193c10023786b6042ec618e24d04` | 2026-09-21 | Shenghao Chen | all `-` (1) | no | #505 merged | — | T1 |  |
| 1 landed | `docs/freeze-candidate-construction-contract` | `87d6e3eb2eeef3d1a004fea711845b6c2d6d7616` | 2026-08-13 | Shenghao Chen | all `-` (1) | no | #35 merged | — | T1 |  |
| 1 landed | `docs/freeze-double-pinyin-spec` | `16efb618dc1ada165669cb5cb923171acd3250ad` | 2026-09-03 | Shenghao Chen | all `-` (1) | no | #287 merged | — | T1 |  |
| 1 landed | `docs/indexes-and-runbooks` | `ab98ace301e83bdf74da4f531d27ac8b2da0a8a7` | 2026-09-07 | Shenghao Chen | all `-` (1) | no | #386 merged | — | T1 |  |
| 1 landed | `docs/lane-c-container-scratch` | `e1db8ddd1f0eaf5158c6aef307bb596f4f9269c2` | 2026-10-07 | Shenghao Chen | all `-` (21) | no | #680 merged | — | T1 |  |
| 1 landed | `docs/model-provenance-fetch-correction` | `d05d3a85e2b751f01fd7d2f2a9a86410e3067a34` | 2026-08-13 | Shenghao Chen | all `-` (1) | no | #38 merged | — | T1 |  |
| 1 landed | `docs/perf-baseline-kc-2026-09-amendment` | `e2c4878014fc46d4c5370fda59a4723796cbf60f` | 2026-09-05 | Shenghao Chen | all `-` (1) | no | #330 merged | #9 PR merged | T1 |  |
| 1 landed | `docs/perf-provenance-audit` | `3fc61e96ec5d3c3e96631ee541ece129c4fa44ff` | 2026-09-08 | Shenghao Chen | all `-` (3) | no | #382 merged | — | T1 |  |
| 1 landed | `docs/perf-recipe-drop-python-row` | `0fc5ff0fff3134c7402fce3935940576a77a6986` | 2026-09-21 | Shenghao Chen | all `-` (1) | no | #509 merged | — | T1 |  |
| 1 landed | `docs/policy-table-sync` | `6d4276e300fc1d7293b2b8a6314d177d4cd78968` | 2026-09-07 | Shenghao Chen | all `-` (3) | no | #360 merged | — | T1 |  |
| 1 landed | `docs/r3-no-self-tag` | `2c8c49bf3728a07e2e84dcfba623e62953fca033` | 2026-08-17 | Shenghao Chen | all `-` (2) | no | #95 merged | — | T1 |  |
| 1 landed | `docs/readme-accuracy` | `1a9e95fcfb034a3266e73738dedb3c7c84ff0be9` | 2026-08-16 | Shenghao Chen | all `-` (3) | no | #79 merged | — | T1 |  |
| 1 landed | `docs/readme-status-crates` | `9f348d11733e64bc2246a4585d0a5038ab3c4ec6` | 2026-08-31 | Shenghao Chen | all `-` (4) | no | #245 merged | — | T1 |  |
| 1 landed | `docs/register-sort-option` | `640e671df477520c614eee7746c9929682efc07a` | 2026-09-18 | Shenghao Chen | all `-` (1) | no | #485 merged | — | T1 |  |
| 1 landed | `docs/reorg-perf-testing` | `b65c4f412111766c21b83c14cb934784ea1b0cf5` | 2026-08-30 | Shenghao Chen | all `-` (3) | no | #242 merged | — | T1 |  |
| 1 landed | `docs/residual-after-construction-freeze` | `67b4519ffe84dc3a1abc73e39821fde563137201` | 2026-09-04 | Shenghao Chen | all `-` (1) | no | #316 merged, #36 closed | — | T1 |  |
| 1 landed | `docs/roadmap-amendment-w7-w8-w9` | `d940fbdd5b197317f10c7056c9b6f530e4fd6e41` | 2026-08-16 | Shenghao Chen | all `-` (1) | no | #75 merged | — | T1 |  |
| 1 landed | `docs/roadmap-stage1-status` | `c6839fd7ed0cd53a120d72713f232951a3f84e99` | 2026-09-06 | Shenghao Chen | all `-` (2) | no | #351 merged | — | T1 |  |
| 1 landed | `docs/roadmap-w10-w11-w12` | `2fc7d480f8d990fb2db73bc87aa3f667a2734f99` | 2026-08-16 | Shenghao Chen | all `-` (1) | no | #90 merged | — | T1 |  |
| 1 landed | `docs/roadmap-w14-sentence-surface` | `894650703750a6abf297eaf7a7a60e8e5bdad41e` | 2026-08-16 | Shenghao Chen | all `-` (1) | no | #102 merged | — | T1 |  |
| 1 landed | `docs/self-audit-fixes` | `51b005cc7ccd3d7e15d1e10971e85239c1ac8d33` | 2026-09-08 | Shenghao Chen | all `-` (2) | no | #392 merged | — | T1 |  |
| 1 landed | `docs/send-sync-audit-wording` | `671ee022141cbde69af7c729d3ff30c62a7103af` | 2026-09-21 | Shenghao Chen | all `-` (1) | no | #510 merged | — | T1 |  |
| 1 landed | `docs/sentence-surface-refreeze-491` | `67f0184777cbfb59e9cb9fcd673ed722fd67dcef` | 2026-09-04 | Shenghao Chen | all `-` (1) | no | #308 merged | #491 PR merged | T1 |  |
| 1 landed | `docs/so-size-facade-tip-correction` | `0ce8755fe96efe3fcaa6cfe199a7a7cf621f7fbf` | 2026-09-05 | Shenghao Chen | all `-` (1) | no | #331 merged | — | T1 |  |
| 1 landed | `docs/stale-source-citations` | `39d1d98e53e365b67f83d3a406d390fd0ddc1f2d` | 2026-10-15 | Shenghao Chen | all `-` (1) | no | #794 merged | — | T1 |  |
| 1 landed | `docs/steady-cycle-cross-host-data` | `2eebc0fabba79cfd2e6544a265838a10f4eb449d` | 2026-09-07 | Shenghao Chen | all `-` (10) | no | #380 merged | — | T1 |  |
| 1 landed | `docs/steering-sync-2026-09-06` | `06cf8e19d7ddadd0e39bb8dcf699860ee6e90952` | 2026-09-06 | Shenghao Chen | all `-` (1) | no | #359 merged | #9 PR merged; #6 PR merged | T1 |  |
| 1 landed | `docs/store-durability-and-divergence-fifth-peer` | `3dffa39e9097c76b9c9ec001afd7015bd940f0ee` | 2026-09-13 | Shenghao Chen | all `-` (2) | no | #456 merged | — | T1 |  |
| 1 landed | `docs/supersede-steady-cross-host` | `7ceebdacded95723124d92ee9e276a24e7a800b8` | 2026-09-09 | Shenghao Chen | all `-` (1) | no | #399 merged | — | T1 |  |
| 1 landed | `docs/tooling-consolidation-phase1` | `7cd046c21f76519f312ae2f207d9bb16724a086e` | 2026-10-17 | Shenghao Chen | all `-` (1) | no | #834 merged | — | T1 |  |
| 1 landed | `docs/uncovered-surface-aggregate` | `d49ddc9c3cf144a813eb80ce9f28115d2f305b5f` | 2026-08-28 | Shenghao Chen | all `-` (3) | no | #191 merged | — | T1 |  |
| 1 landed | `docs/upstream-report-drafts` | `0f3f871a4be4e9693dba5c1be9f7a86d6259b271` | 2026-08-22 | Shenghao Chen | all `-` (3) | no | #148 merged | — | T1 |  |
| 1 landed | `docs/vn-findings` | `e3f1dbf47b06d45ddda97d9f2ecf3665a4a14aa8` | 2026-09-02 | Shenghao Chen | all `-` (3) | no | #279 merged | — | T1 |  |
| 1 landed | `docs/w12-all-off-tails` | `d13c45c4157af90c01a552ac4a44cf284ceb5b2c` | 2026-08-22 | Shenghao Chen | all `-` (1) | no | #139 merged | — | T1 |  |
| 1 landed | `docs/w14-sentence-residual-freeze` | `17bd500024f02f78610efdfd14e092558cba4ed8` | 2026-09-03 | Shenghao Chen | all `-` (2) | no | #288 merged | — | T1 |  |
| 1 landed | `docs/w8-report` | `14e801cfedb470cc9d70dedee823a628b8cc0894` | 2026-08-16 | Shenghao Chen | all `-` (2) | no | #93 merged | — | T1 |  |
| 1 landed | `engine/pooling-dp-no-clone` | `b0192e09e1f13e8c122100dfdc9c23f71536f6e7` | 2026-09-08 | Shenghao Chen | all `-` (1) | no | #389 merged | — | T1 |  |
| 1 landed | `engine/session-split` | `52a6316fbac7d9628807e89e1c2bf211af81b058` | 2026-09-07 | Shenghao Chen | all `-` (1) | no | #388 merged | — | T1 |  |
| 1 landed | `engine/zhuyin-span-start-and-nbest-shape` | `acfce4d593dfe6da98408790f45dbdb85647ecd6` | 2026-09-07 | Shenghao Chen | all `-` (2) | no | #374 merged | — | T1 |  |
| 1 landed | `facade/tests` | `e33154068c8623a7ed4dd459e0e8d7a0453ec4d3` | 2026-09-08 | Shenghao Chen | all `-` (1) | no | #390 merged | — | T1 |  |
| 1 landed | `feat/backend-benchmarks` | `88f43b9937b9883f9eea26717d64a37804a7d818` | 2026-08-23 | Shenghao Chen | all `-` (25) | no | #158 merged | — | T1 |  |
| 1 landed | `feat/backend-parity` | `6cffa8c4fe20055f179cfd6c8078f37174870c48` | 2026-08-30 | Shenghao Chen | all `-` (16) | no | #220 merged | — | T1 |  |
| 1 landed | `feat/backend-parity-p1` | `76a64d3cade1164790ebf1bb2fe13ec4612f0db5` | 2026-08-30 | Shenghao Chen | all `-` (8) | no | #222 merged | — | T1 |  |
| 1 landed | `feat/bench-disk-alloc` | `5d53103057d3e4321c06a0bb45c6e123e8897a96` | 2026-08-23 | Shenghao Chen | all `-` (27) | no | #159 merged | — | T1 |  |
| 1 landed | `feat/berkeleydb-backend` | `689b69409c20cd2b79547beba395ed78d3e1c56f` | 2026-09-12 | Shenghao Chen | all `-` (7) | no | #445 merged | — | T1 |  |
| 1 landed | `feat/candidate-collection-window-scan` | `017a610293e6546c69877c8be1ecbc2e89690c67` | 2026-08-14 | Shenghao Chen | all `-` (2) | no | #46 merged | — | T1 |  |
| 1 landed | `feat/capi-decode-incomplete-option` | `ed7226d5f6526bcc35c93b3b5db886350d3bea00` | 2026-08-16 | Shenghao Chen | all `-` (10) | no | #83 merged | — | T1 |  |
| 1 landed | `feat/capi-full-aux-text` | `c447d475c718e8bbcbedd0d4ea80b5d7ec6fe446` | 2026-08-16 | Shenghao Chen | all `-` (13) | no | #86 merged | — | T1 |  |
| 1 landed | `feat/capi-interpolation2-unigrams` | `bb33c4907ad1ed0fc48926fd23e89ecd019867d6` | 2026-08-16 | Shenghao Chen | all `-` (11) | no | #84 merged | — | T1 |  |
| 1 landed | `feat/constraint-machinery` | `4300452540c64c1896bb40abd981457d6c2519a2` | 2026-08-22 | Shenghao Chen | all `-` (8) | no | #145 merged | — | T1 |  |
| 1 landed | `feat/data-layer-rebuild` | `4d2fea1cd0cca10bc71b5948811fa8963e9ce1ae` | 2026-08-10 | Shenghao Chen | all `-` (6) | no | #29 merged | — | T1 |  |
| 1 landed | `feat/decoder-lambda-from-config` | `1beeff3ec91cee7c1264bac056efa920679436b2` | 2026-08-15 | Shenghao Chen | all `-` (1) | no | #55 merged | — | T1 |  |
| 1 landed | `feat/double-force-tone-batch` | `02676d5a3e8cd7b86142aa6f008476006c2314c0` | 2026-09-03 | Shenghao Chen | all `-` (4) | no | #289 merged | — | T1 |  |
| 1 landed | `feat/drop-snapshot-mvcc` | `6bf3b285e615f1641e0ac626f246d8c0f5b4eee4` | 2026-08-24 | Shenghao Chen | all `-` (8) | no | #166 merged | — | T1 |  |
| 1 landed | `feat/dynamic-adjust` | `da3fe9ea24efbfadb9affff5f00aed1e4ebea1c8` | 2026-08-29 | Shenghao Chen | all `-` (20) | no | #204 merged | — | T1 |  |
| 1 landed | `feat/five-differential-surfaces` | `6b98aa6ed1445a4631f9a6510cb134897687d4dd` | 2026-08-28 | Shenghao Chen | all `-` (2) | no | #190 merged | — | T1 |  |
| 1 landed | `feat/foundation-t2-provenance` | `a0df9cdc692a2262d92432524e8c3ba54ca1c4c6` | 2026-08-08 | Shenghao Chen | all `-` (1) | no | #1 merged | — | T1 |  |
| 1 landed | `feat/foundation-t4-capture-harness` | `810285083f1b2d793ef92eb9dba986c64d1c3740` | 2026-08-09 | Shenghao Chen | all `-` (9) | no | #2 merged | — | T1 |  |
| 1 landed | `feat/foundation-t5-core-traits` | `cc7408e8cdfff806f4bae725bf97928942a8e863` | 2026-08-09 | Shenghao Chen | all `-` (12) | no | #3 merged | — | T1 |  |
| 1 landed | `feat/foundation-t6-syllable-table` | `129e954026678f81755cc816cefd5c9d57903506` | 2026-08-09 | Shenghao Chen | all `-` (15) | no | #4 merged | — | T1 |  |
| 1 landed | `feat/foundation-t7-parser` | `acae9ac52a911974e86c11e2b794f3f12867d65f` | 2026-08-09 | Shenghao Chen | all `-` (18) | no | #5 merged | — | T1 |  |
| 1 landed | `feat/foundation-t8-parser-acceptance` | `37217b7619437417a32848dbbd4e94c5dd502987` | 2026-08-09 | Shenghao Chen | all `-` (20) | no | #6 merged | — | T1 |  |
| 1 landed | `feat/foundation-t9-parser-fuzzing` | `18a444359882115e500b534f5de80909639bdf3e` | 2026-08-09 | Shenghao Chen | all `-` (23) | no | #7 merged | — | T1 |  |
| 1 landed | `feat/gfloat-accumulation-parity` | `b79e478a71026ce11cdb8e24888546a4fe3fb5fc` | 2026-08-24 | Shenghao Chen | all `-` (4) | no | #171 merged | — | T1 |  |
| 1 landed | `feat/guess-candidates-sort-options` | `014700c52e8bba16fc09bd3bc7c134e31522867e` | 2026-09-20 | Shenghao Chen | all `-` (4) | no | #496 merged | — | T1 |  |
| 1 landed | `feat/guess-offset-zero-start` | `ad9a6dfafdd2332100fa5a9aa3f1a6e3cd959062` | 2026-08-22 | Shenghao Chen | all `-` (5) | no | #141 merged | — | T1 |  |
| 1 landed | `feat/hanyu-use-tone-digits` | `860e122f361fc7ce689d8648c3758284878d4289` | 2026-08-20 | Shenghao Chen | all `-` (7) | no | #131 merged | — | T1 |  |
| 1 landed | `feat/kc-dropin` | `452593882a658aca38b09b50ef8d9132e020914f` | 2026-08-30 | Shenghao Chen | all `-` (10) | no | #228 merged | — | T1 |  |
| 1 landed | `feat/libpinyin-installed-naming` | `b2d60fa268db018303a1fc2272d04c19198486ae` | 2026-08-29 | Shenghao Chen | all `-` (31) | no | #192 merged | — | T1 |  |
| 1 landed | `feat/libzhuyin-engine` | `d6a74ef90c89e78cd750b796c5c21e45cdc0ec44` | 2026-08-31 | Shenghao Chen | all `-` (6) | no | #247 merged | — | T1 |  |
| 1 landed | `feat/libzhuyin-facade` | `98da056669380d93926e7ff7fe2cb4df30bdaae5` | 2026-08-30 | Shenghao Chen | all `-` (2) | no | #246 merged | — | T1 |  |
| 1 landed | `feat/live-typing-differential` | `15496443f474f619c37d4ee001b4c99de505f3ea` | 2026-08-22 | Shenghao Chen | all `-` (4) | no | #144 merged | — | T1 |  |
| 1 landed | `feat/lmdb-backend` | `efe2c1b4a20510811cbe0b2304f478837e31c5a5` | 2026-08-23 | Shenghao Chen | all `-` (18) | no | #155 merged | — | T1 |  |
| 1 landed | `feat/lmdb-system-library` | `b66c5806a1a42368fff517f96c7ee23dd2fbef5a` | 2026-09-11 | Shenghao Chen | all `-` (2) | no | #420 merged | — | T1 |  |
| 1 landed | `feat/model-build-fetch` | `ab3624e9b867a8fb021100b573088de9e09f8c15` | 2026-08-13 | Shenghao Chen | all `-` (1) | no | #39 merged | — | T1 |  |
| 1 landed | `feat/ordered-store-rw` | `e676b56c71efa38d8acb2aec3e06af8cfa87e5ef` | 2026-08-23 | Shenghao Chen | all `-` (6) | no | #152 merged | — | T1 |  |
| 1 landed | `feat/oxpinyin-store` | `a08ec10cc7b92d4628d0b87bfbd72abd0991b1da` | 2026-08-23 | Shenghao Chen | all `-` (2) | no | #149 merged | — | T1 |  |
| 1 landed | `feat/parity-climb` | `0505b234a2ab3799adbb5a1940008af3130cb37b` | 2026-08-10 | Shenghao Chen | all `-` (4) | no | #30 merged | — | T1 |  |
| 1 landed | `feat/parity-climb-2` | `75c6096c8719e3f2317b8df3031b9b4e4c7c5099` | 2026-08-11 | Shenghao Chen | all `-` (4) | no | #31 merged | — | T1 |  |
| 1 landed | `feat/parity-test-parallel` | `83b4c71dc1c105f7dca80766b69f9450b00901af` | 2026-08-12 | Shenghao Chen | all `-` (2) | no | #32 merged | — | T1 |  |
| 1 landed | `feat/perf-exploration` | `10cd352551e71b3d2ab9d52a708841b05fcddf78` | 2026-08-14 | Shenghao Chen | all `-` (4) | no | #48 merged | — | T1 |  |
| 1 landed | `feat/pinyin-header-verbatim` | `93508dbc7c4c7fcd5636137becd83a87992ca8db` | 2026-08-31 | Shenghao Chen | all `-` (1) | no | #261 merged | — | T1 |  |
| 1 landed | `feat/preedit-key-accessor` | `6db9e4a8c970a33f6c3d56abe81a9dc108173665` | 2026-08-29 | Shenghao Chen | all `-` (23) | no | #205 merged | — | T1 |  |
| 1 landed | `feat/python-api` | `533d4a432518d6fed89a29cef1879f340cf70999` | 2026-09-04 | Shenghao Chen | all `-` (1) | no | #317 merged, #179 closed | — | T1 |  |
| 1 landed | `feat/python-api-docs` | `89cd6a5a7f354636c78e6cdee016a8eb79a12ab2` | 2026-08-28 | Shenghao Chen | all `-` (8) | no | #188 merged | — | T1 |  |
| 1 landed | `feat/python-bindings` | `c15230372646f882e66a1e010bed2797b420913b` | 2026-08-28 | Shenghao Chen | all `-` (4) | no | #187 merged | — | T1 |  |
| 1 landed | `feat/runtime-assembly` | `08e06c9ba05661618f46a5ff15a4b20a4f78526a` | 2026-08-27 | Shenghao Chen | all `-` (2) | no | #186 merged | — | T1 |  |
| 1 landed | `feat/stage2-measurement-harness` | `f84e4abd459724abb0e55c7b02e1bbf1d1b2db47` | 2026-08-20 | Shenghao Chen | all `-` (6) | no | #121 merged | — | T1 |  |
| 1 landed | `feat/store-capability-tiers` | `81cfe66af99b4ff3f806d779aff181e35d51e1a2` | 2026-08-24 | Shenghao Chen | all `-` (2) | no | #164 merged | — | T1 |  |
| 1 landed | `feat/store-read-snapshot` | `6d21e96b0dea271a2c8dd6e355b873e493cb915a` | 2026-08-23 | Shenghao Chen | all `-` (8) | no | #153 merged | — | T1 |  |
| 1 landed | `feat/store-tier-tests` | `00b6ab4e442a857b4fac8ede6af9c1a0b14b17c8` | 2026-08-24 | Shenghao Chen | all `-` (4) | no | #165 merged | — | T1 |  |
| 1 landed | `feat/test-architecture` | `bc57fbd6b4cd83a7dc005d36ddc2d665f5d24376` | 2026-08-29 | Shenghao Chen | all `-` (7) | no | #210 merged | — | T1 |  |
| 1 landed | `feat/tkrzw-backend` | `446c51f30a890085c08b3ba9311867bce392f587` | 2026-08-24 | Shenghao Chen | all `-` (11) | no | #167 merged | — | T1 |  |
| 1 landed | `feat/tkrzw-bench-column` | `f97bcf8a166f9f603f18f4c3d3efb26dd57c3456` | 2026-08-24 | Shenghao Chen | all `-` (12) | no | #168 merged | — | T1 |  |
| 1 landed | `feat/tkrzw-bindgen-migration` | `940a5213e9321d030850d2403d7749ac161b7245` | 2026-08-28 | Shenghao Chen | all `-` (3) | no | #197 merged | — | T1 |  |
| 1 landed | `feat/tkrzw-zero-copy` | `2a4cd49f8931f8d2af979176361cf9c3a22239c7` | 2026-08-25 | Shenghao Chen | all `-` (14) | no | #169 merged | — | T1 |  |
| 1 landed | `feat/uncovered-surface-differentials` | `1d8bd72cce53cd180a3d77e63f120fa63d50a45c` | 2026-08-26 | Shenghao Chen | all `-` (10) | no | #174 merged | — | T1 |  |
| 1 landed | `feat/user-codec-fallible` | `8d6366f88a92c9786faa952ebb44d3077b18108f` | 2026-08-23 | Shenghao Chen | all `-` (4) | no | #151 merged | — | T1 |  |
| 1 landed | `feat/user-key-codec` | `2c6de1dd09245ed477d153146f0f661ba3ef768e` | 2026-08-23 | Shenghao Chen | all `-` (3) | no | #150 merged | — | T1 |  |
| 1 landed | `feat/user-store-generic` | `3af760e594a608ee9b4174f1e07db58cffb006db` | 2026-08-23 | Shenghao Chen | all `-` (19) | no | #156 merged | — | T1 |  |
| 1 landed | `feat/user-store-on-trait` | `33c59f10717d6f2e1100fba0f45ecc9981858584` | 2026-08-23 | Shenghao Chen | all `-` (12) | no | #154 merged | — | T1 |  |
| 1 landed | `feat/user-store-public-ctor` | `026a85b7ab74108a2409b2bd18d26b83921ad735` | 2026-08-23 | Shenghao Chen | all `-` (23) | no | #157 merged | — | T1 |  |
| 1 landed | `feat/w10-option-bits` | `529a1226057f3d7980d7e93c9de4f1756634f818` | 2026-08-17 | Shenghao Chen | all `-` (5) | no | #98 merged | — | T1 |  |
| 1 landed | `feat/w11-addon-choose-promotion` | `6dca19876a5a73c8f74572e2ea3a803a99bbb30f` | 2026-08-17 | Shenghao Chen | all `-` (1) | no | #112 merged | — | T1 |  |
| 1 landed | `feat/w11-phrase-union` | `47b0c9f7087d4ea6e76896c477fcc8148817fd04` | 2026-08-17 | Shenghao Chen | all `-` (5) | no | #91 merged | — | T1 |  |
| 1 landed | `feat/w11-phrase-union-impl` | `0058e33204cdfc972f07f1bb6a036a2fca23f10d` | 2026-08-17 | Shenghao Chen | all `-` (11) | no | #96 merged | — | T1 |  |
| 1 landed | `feat/w11-prediction-punct` | `a42cc22c5a3a234b49f9d2d6bb6a6036afa60ffe` | 2026-08-17 | Shenghao Chen | all `-` (2) | no | #111 merged | — | T1 |  |
| 1 landed | `feat/w12-class-a-comparator` | `0466da9f11a6fbae8cd26ae574c247c925cbde4f` | 2026-08-22 | Shenghao Chen | all `-` (6) | no | #140 merged | — | T1 |  |
| 1 landed | `feat/w12-corpus-tail` | `d699ddf156c73a09756988fb9ea5b4dddffd8236` | 2026-08-20 | Shenghao Chen | all `-` (2) | no | #133 merged | — | T1 |  |
| 1 landed | `feat/w13-schemes` | `efd8f62a3ab1b1b4c707c7a1a6cc55a2b4131419` | 2026-08-16 | Shenghao Chen | all `-` (2) | no | #97 merged | — | T1 |  |
| 1 landed | `feat/w14-nbest-user-delta` | `7fc776edfcb0d920424fcf46192fe77398624c21` | 2026-08-19 | Shenghao Chen | all `-` (4) | no | #119 merged | — | T1 |  |
| 1 landed | `feat/w14-sentence-surface` | `5ec6574d24bae5ffd3c135d67087d5c2c54878d6` | 2026-08-18 | Shenghao Chen | all `-` (5) | no | #113 merged | — | T1 |  |
| 1 landed | `feat/w2-cand-capture` | `c90d2de3927e1d15d4ec7c1dd90ccda62d0ceec8` | 2026-08-13 | Shenghao Chen | all `-` (2) | no | #34 merged | — | T1 |  |
| 1 landed | `feat/w2-t1-oracle-ffi` | `fcaaef893051fa7100206eda2eb01ef7ce4b36b9` | 2026-08-09 | Shenghao Chen | all `-` (3) | no | #9 merged | — | T1 |  |
| 1 landed | `feat/w2-t2-parity-corpus` | `13b9c248070ec6d2bfa7f764e42e1f93c6172ffa` | 2026-08-09 | Shenghao Chen | all `-` (6) | no | #10 merged | — | T1 |  |
| 1 landed | `feat/w2-t3-differential-runner` | `3e23727088e1eb585c6864a32b8e44b7216e31df` | 2026-08-09 | Shenghao Chen | all `-` (11) | no | #11 merged | — | T1 |  |
| 1 landed | `feat/w2-t5-divergence-taxonomy` | `d34ee64e97b2228d9f2d9fd3afe6e22945d450ea` | 2026-08-09 | Shenghao Chen | all `-` (19) | no | #12 merged | — | T1 |  |
| 1 landed | `feat/w3-t0-mini-fixtures` | `3f01c012414509ee007bff7fbf5b5bd63545ad6d` | 2026-08-09 | Shenghao Chen | all `-` (6) | no | #16 merged | — | T1 |  |
| 1 landed | `feat/w3-t1-schema-spec` | `91f41685ac1f13ef6d75a897abdb8d2de1fd0bee` | 2026-08-09 | Shenghao Chen | all `-` (3) | no | #14 merged | — | T1 |  |
| 1 landed | `feat/w3-t2-converter` | `208e2766e93b9e8da250a234e806aff280a61005` | 2026-08-09 | Shenghao Chen | all `-` (4) | no | #15 merged | — | T1 |  |
| 1 landed | `feat/w3-t3-dict-lm-impls` | `67e245bc4fe36f99f1b3b0655723df4306c204ba` | 2026-08-09 | Shenghao Chen | all `-` (9) | no | #17 merged | — | T1 |  |
| 1 landed | `feat/w4-t0-session-api` | `cf14b20513ba56d54a0ad5c55ce7e6301efbb6e4` | 2026-08-09 | Shenghao Chen | all `-` (3) | no | #19 merged | — | T1 |  |
| 1 landed | `feat/w4-t0b-fixture-adapters` | `60d7e6bb51d87ef226a65c891f64bb9cf0a78463` | 2026-08-09 | Shenghao Chen | all `-` (5) | no | #20 merged | — | T1 |  |
| 1 landed | `feat/w4-t0c-config` | `132f418451ed38a1942d9167165613894c567393` | 2026-08-09 | Shenghao Chen | all `-` (7) | no | #21 merged | — | T1 |  |
| 1 landed | `feat/w4-t1-segment-graph` | `8e1fc97000b574f432e52f94bf818fa5d27130da` | 2026-08-09 | Shenghao Chen | all `-` (11) | no | #22 merged | — | T1 |  |
| 1 landed | `feat/w4-t2-kbest` | `55af9a5ada34c1f66062754a769a1b6c12780612` | 2026-08-09 | Shenghao Chen | all `-` (13) | no | #23 merged | — | T1 |  |
| 1 landed | `feat/w4-t3-scoring` | `f260cdf1566482fcbc1733f36d50e6782e29206c` | 2026-08-09 | Shenghao Chen | all `-` (15) | no | #24 merged | — | T1 |  |
| 1 landed | `feat/w4-t4-engine-compose` | `1abef31d5bc2f5e8343d13c6e439f5123f5fb35d` | 2026-08-09 | Shenghao Chen | all `-` (18) | no | #25 merged | — | T1 |  |
| 1 landed | `feat/w4-t4b-session-replay` | `168f393e9e1afb33301191987604fbe6ae71abd1` | 2026-08-09 | Shenghao Chen | all `-` (21) | no | #26 merged | — | T1 |  |
| 1 landed | `feat/w5-t1-capi-skeleton` | `85265f24c02b1cfca0053bc947aacb37ecf6a659` | 2026-08-15 | Shenghao Chen | all `-` (2) | no | #42 merged | — | T1 |  |
| 1 landed | `feat/w5-t2-lifecycle` | `6614fd9eb44a0be991285592c7491ed8114da7c0` | 2026-08-15 | Shenghao Chen | all `-` (4) | no | #43 merged | — | T1 |  |
| 1 landed | `feat/w5-t3-keystroke` | `41fe5394e96d7cd1c6f6dfbcfc0e46b5f8b36555` | 2026-08-15 | Shenghao Chen | all `-` (6) | no | #44 merged | — | T1 |  |
| 1 landed | `feat/w5-t4-select-save-config` | `04d2d3282f2fc92fe088be3ad4ef2743b59821fd` | 2026-08-15 | Shenghao Chen | all `-` (8) | no | #45 merged | — | T1 |  |
| 1 landed | `feat/w6-t0-user-store-characterization` | `67cd3fdd50604916d809233cb7b20c581fc59b5f` | 2026-08-15 | Shenghao Chen | all `-` (1) | no | #61 merged | — | T1 |  |
| 1 landed | `feat/w6-t1-user-store-schema` | `b9eeaac75492f18d923df1d59983b7a7afdf7c9a` | 2026-08-15 | Shenghao Chen | all `-` (2) | no | #62 merged | — | T1 |  |
| 1 landed | `feat/w6-t2-user-phrase-index` | `e82dabba5a8d75ac12007dae3d6124c326b2d0c2` | 2026-08-15 | Shenghao Chen | all `-` (3) | no | #63 merged | — | T1 |  |
| 1 landed | `feat/w6-t3-training-wiring` | `54f7bd435104818fac9931500e3c6064af7a3f5e` | 2026-08-15 | Shenghao Chen | all `-` (4) | no | #64 merged | — | T1 |  |
| 1 landed | `feat/w6-t4-decode-merge` | `34603e52c67b256386f00a45ffa1d4802b3942f0` | 2026-08-15 | Shenghao Chen | all `-` (5) | no | #65 merged | — | T1 |  |
| 1 landed | `feat/w6-t5-persistence` | `434301ec08bd793f8eeb6cbc1d1e406ec81716c4` | 2026-08-15 | Shenghao Chen | all `-` (6) | no | #66 merged | — | T1 |  |
| 1 landed | `feat/w6-t6-masking` | `cc4a5362e22f23e0324196c98f92639742619370` | 2026-08-15 | Shenghao Chen | all `-` (10) | no | #68 merged | — | T1 |  |
| 1 landed | `feat/w6-t7-export-differential` | `9cf95fc3ee85f67653e94cd4daf0f92c35b8b583` | 2026-08-15 | Shenghao Chen | all `-` (7) | no | #67 merged | — | T1 |  |
| 1 landed | `feat/w7-t1-user-import` | `2bdcd2aa763e44767df293c1e3320ca9d65e5ae1` | 2026-08-16 | Shenghao Chen | all `-` (2) | no | #73 merged | — | T1 |  |
| 1 landed | `feat/w8-fork-bootstrap-surface` | `42d9ed9ba54e8418fd2844f97071f317b0d74dee` | 2026-08-16 | Shenghao Chen | all `-` (7) | no | #81 merged | — | T1 |  |
| 1 landed | `feat/w8-perf-baseline` | `d794282fef70ec25ce7254ea3bd2e1dec4fe0b00` | 2026-08-16 | Shenghao Chen | all `-` (1) | no | #92 merged | — | T1 |  |
| 1 landed | `feat/w9-t4a-emitter` | `5ea42eb585847ff2de6fbeb6c76d55278d42fae9` | 2026-08-15 | Shenghao Chen | all `-` (5) | no | #57 merged | — | T1 |  |
| 1 landed | `feat/w9-t4b-corpus` | `554ab89316378070cf9027dd58e44bce0defeee3` | 2026-08-15 | Shenghao Chen | all `-` (6) | no | #58 merged | — | T1 |  |
| 1 landed | `fix/1336-profile-linux-stage-2` | `37658dd7df76d7d358d735fff610b65aee5bb27f` | 2026-10-28 | Shenghao Chen | all `-` (1) | no | — | #1336 issue closed | T1 |  |
| 1 landed | `fix/540-integer-edge-parity` | `bc04f3f61ab476cc7dc170a3ee55d673d9077537` | 2026-10-11 | Shenghao Chen | all `-` (2) | no | #735 merged | #540 issue closed | T1 |  |
| 1 landed | `fix/542-contract-ruling` | `85b690fac18445d65f083551e12c5804b0e71df5` | 2026-10-11 | Shenghao Chen | all `-` (1) | no | #739 merged | #542 issue closed | T1 |  |
| 1 landed | `fix/545-mmap-failed-lines` | `865b5fcfcccb2b9d94b17f71f8f10fb4c461da77` | 2026-10-09 | Shenghao Chen | all `-` (3) | no | #711 merged | #545 issue closed | T1 |  |
| 1 landed | `fix/554-guess-step-rows` | `c1b81643f491570e58d4ab742d991fe4b3d17c68` | 2026-10-13 | Shenghao Chen | all `-` (2) | no | #756 merged | #554 issue closed | T1 |  |
| 1 landed | `fix/554-sparse-successor-walk` | `5371359bfdd665aa2d1e9a8011f3079b4a91ef26` | 2026-10-13 | Shenghao Chen | all `-` (4) | no | #760 merged | #554 issue closed | T1 |  |
| 1 landed | `fix/569-pred-driver-observability` | `b0afcda7d97df5a121f0449fceddc93e97bb6416` | 2026-10-11 | Shenghao Chen | all `-` (2) | no | #746 merged | #569 issue closed | T1 |  |
| 1 landed | `fix/728-mask-out-reload` | `6817cc5fe96524b31092543f23370821c33a2266` | 2026-10-11 | Shenghao Chen | all `-` (2) | no | #734 merged | #728 issue closed | T1 |  |
| 1 landed | `fix/736-shared-segment-graph` | `2d6875707ccf9a46513c3b716bd01509dd23fa55` | 2026-10-11 | Shenghao Chen | all `-` (1) | no | #738 merged | #736 issue closed | T1 |  |
| 1 landed | `fix/740-stored-bigram-total` | `336730891667e9a80bf3c7233aeaf5d04131135e` | 2026-10-11 | Shenghao Chen | all `-` (1) | no | #741 merged | #740 issue closed | T1 |  |
| 1 landed | `fix/751-surrogate-segmentation` | `ca22ddb1402a10a36b0f526014aedf2a890997e9` | 2026-10-12 | Shenghao Chen | all `-` (1) | no | #754 merged | #751 issue closed | T1 |  |
| 1 landed | `fix/752-remember-character-eligibility` | `d634f60d6661bccd6b85842c33bb5ec2b524c83e` | 2026-10-12 | Shenghao Chen | all `-` (1) | no | #755 merged | #752 issue closed | T1 |  |
| 1 landed | `fix/785-phrase-table-mask-carry` | `63d727b4f6bb10ce389f46e39dfea46b11064e6d` | 2026-10-15 | Shenghao Chen | all `-` (1) | no | #807 merged | #785 issue closed | T1 |  |
| 1 landed | `fix/790-live-pronunciation-export` | `085c88e4e4399889aaa7e94babcc8e11515960e6` | 2026-10-15 | Shenghao Chen | all `-` (1) | no | #803 merged | #790 issue closed | T1 |  |
| 1 landed | `fix/791-nbest-blend-cost-above-one` | `e416b01a73ef498b242268c65a034887e19286c0` | 2026-10-15 | Claude | all `-` (2) | no | #796 merged | #791 issue closed | T1 |  |
| 1 landed | `fix/798-wrapping-facade-total` | `2fbb38e5e1e6dd25ca44bfe514b9de3c24fcea9e` | 2026-10-15 | Shenghao Chen | all `-` (1) | no | #801 merged | #798 issue closed | T1 |  |
| 1 landed | `fix/802-preflight-stored-total` | `07acbcc64602d6fb38e15eaa6cd75f562dfc3e54` | 2026-10-16 | Shenghao Chen | all `-` (1) | no | #817 merged | #802 issue closed | T1 |  |
| 1 landed | `fix/804-wrapped-pronunciation-count` | `751e5fc8efdfc85dd4fbbc117067e63486fd5dd1` | 2026-10-16 | Shenghao Chen | all `-` (1) | no | #806 merged | #804 issue closed | T1 |  |
| 1 landed | `fix/805-reload-drops-session` | `9f366ddfebad9379f57f44228c255b5b67e639c5` | 2026-10-16 | Shenghao Chen | all `-` (1) | no | #812 merged | #805 issue closed | T1 |  |
| 1 landed | `fix/808-pinyin-unigram-user-libs` | `b08922ab6aa9da452388348ea18e3b79c3a68c6f` | 2026-10-16 | Shenghao Chen | all `-` (1) | no | #813 merged | #808 issue closed | T1 |  |
| 1 landed | `fix/809-system-zhuyin-only-readings` | `c74990cdbae6bbaac78852db74ed1a1a6c0c7674` | 2026-10-16 | Shenghao Chen | all `-` (1) | no | #815 merged | #809 issue closed | T1 |  |
| 1 landed | `fix/811-runner-system-dir` | `6e2574de593b9d7e15556bf9792af6790208409c` | 2026-10-16 | Shenghao Chen | all `-` (1) | no | #816 merged | #811 issue closed | T1 |  |
| 1 landed | `fix/all-off-tails` | `8c6e33e0bf407a255b6a920f2561b02a205cc8df` | 2026-08-24 | Shenghao Chen | all `-` (1) | no | #162 merged | — | T1 |  |
| 1 landed | `fix/bisect-nbest-guard` | `1d2b382a45f7a0c5d77f5c0e991a3bfe4704c6f4` | 2026-08-16 | Shenghao Chen | all `-` (8) | no | #82 merged | — | T1 |  |
| 1 landed | `fix/capi-database-format-bdb` | `8246e3fea95d26f34de217ea08486a6c1d6f9195` | 2026-09-21 | Shenghao Chen | all `-` (7) | no | #498 merged | — | T1 |  |
| 1 landed | `fix/capi-live-registry-release` | `72e5678b50397ba5b49b12a1269aeeb3a7902542` | 2026-10-07 | Shenghao Chen | all `-` (1) | no | #687 merged | — | T1 |  |
| 1 landed | `fix/constraint-shrink-survival` | `1e03dfb3711e5833e5a76fc646bd8e4133569fa1` | 2026-08-22 | Shenghao Chen | all `-` (10) | no | #146 merged | — | T1 |  |
| 1 landed | `fix/core-incomplete-phonetic-initial` | `ef68651440ed4ced3dc138abfde58c60a73133bd` | 2026-08-16 | Shenghao Chen | all `-` (12) | no | #85 merged | — | T1 |  |
| 1 landed | `fix/cursor-offset-normalization` | `ff1cc6b29d6f215dec8425a3a9926c15e15e3bb2` | 2026-08-26 | Shenghao Chen | all `-` (4) | no | #177 merged | — | T1 |  |
| 1 landed | `fix/engine-remove-candidate-cap` | `b650f33b853db0df8264eedae7d78dbb16dbf5bb` | 2026-08-16 | Shenghao Chen | all `-` (15) | no | #87 merged | — | T1 |  |
| 1 landed | `fix/error-taxonomy-io-misclassification` | `476696e135a1c80a589418a907187313fd059876` | 2026-09-05 | Shenghao Chen | all `-` (4) | no | #324 merged | — | T1 |  |
| 1 landed | `fix/expand-keys-zero-limit-521` | `8bcdc9b23eb168659d1902aaa1632da632aa34b1` | 2026-10-11 | Shenghao Chen | all `-` (1) | no | #724 merged | #521 issue closed | T1 |  |
| 1 landed | `fix/fmt-import-order` | `a82a66c146c674cdc9ccd7ffbde7087850a5d087` | 2026-08-17 | Shenghao Chen | all `-` (1) | no | #107 merged | — | T1 |  |
| 1 landed | `fix/fuzz-capi-fixture-path` | `dded4caff49755bd6bee08f17769da1c7db12519` | 2026-09-04 | Shenghao Chen | all `-` (1) | no | #315 merged | — | T1 |  |
| 1 landed | `fix/fuzz-session-keys-timeout` | `2ddc64b5176f073c87d56ee4d298634d16b4c969` | 2026-09-23 | Shenghao Chen | all `-` (1) | no | #518 merged | — | T1 |  |
| 1 landed | `fix/independent-contexts` | `f8d4fcdc20ccc56aa8956c048a2e2dab9c5ef0e7` | 2026-09-26 | Shenghao Chen | all `-` (8) | no | #581 merged | — | T1 |  |
| 1 landed | `fix/issue-250-libzhuyin-pc` | `215d14443866bb550113d367b8428754e1848d7c` | 2026-09-02 | Shenghao Chen | all `-` (1) | no | #285 merged | #250 issue closed | T1 |  |
| 1 landed | `fix/key-costs-rwlock-fast-path` | `004f251e049099b0a1c97ccadb64b2a6cd018853` | 2026-09-04 | Shenghao Chen | all `-` (2) | no | #314 merged | — | T1 |  |
| 1 landed | `fix/keycost-doc-and-gate-followups` | `9bd572fe983b66694b53f430d865f5ad2024958d` | 2026-09-08 | Shenghao Chen | all `-` (6) | no | #381 merged | — | T1 |  |
| 1 landed | `fix/leading-separator-columns` | `2dcb5bc5c3fa8b9d47b728139f5d7fed57f8229f` | 2026-10-09 | Shenghao Chen | all `-` (1) | no | #707 merged | — | T1 |  |
| 1 landed | `fix/lmdb-lock-sidecars` | `d784c29c4406593fdd685c06a8104e3951d263d5` | 2026-09-07 | Shenghao Chen | all `-` (1) | no | #361 merged | — | T1 |  |
| 1 landed | `fix/mid-offset-window` | `805f5a993794b5ca3da9bfdc78468fe7de034d92` | 2026-08-26 | Shenghao Chen | all `-` (4) | no | #176 merged | — | T1 |  |
| 1 landed | `fix/nbest-step-cost-user-token` | `153ebf488059232d78d8d19835daca6e8d6f99d9` | 2026-09-19 | Shenghao Chen | all `-` (1) | no | #493 merged | — | T1 |  |
| 1 landed | `fix/nightly-differentials-w3` | `159389d3cb517a9c73e2eaf5694228ddda5dbbfa` | 2026-09-23 | Shenghao Chen | all `-` (1) | no | #517 merged | — | T1 |  |
| 1 landed | `fix/oracle-driver-trains-a-selection` | `05054aee4128a402cd1898597da9f6b342c8ee11` | 2026-09-13 | Shenghao Chen | all `-` (2) | no | #457 merged | — | T1 |  |
| 1 landed | `fix/p3-p8-land-on-main` | `fd981eedbc1ddfb1976e1397d6ee322bd32a0146` | 2026-09-03 | Shenghao Chen | all `-` (25) | no | #304 merged | — | T1 |  |
| 1 landed | `fix/parse-termination` | `5a062026ba8387975532f672e30fdac6d9822efb` | 2026-08-26 | Shenghao Chen | all `-` (5) | no | #178 merged | — | T1 |  |
| 1 landed | `fix/perf-explicit-kc-backend` | `e35ac93425172f8535036eb5f383b46ce0146da7` | 2026-09-07 | Shenghao Chen | all `-` (3) | no | #375 merged | — | T1 |  |
| 1 landed | `fix/pr302-deferred-findings` | `255489c189c86aafe2cbff806463340dd2c9e4ce` | 2026-09-03 | Shenghao Chen | all `-` (5) | no | #307 merged | — | T1 |  |
| 1 landed | `fix/predicted-prefix-slice` | `6de58adb8fd724d8215543574f07861e46f62f96` | 2026-08-26 | Shenghao Chen | all `-` (2) | no | #173 merged | — | T1 |  |
| 1 landed | `fix/predicted-text-order` | `4b70f93d0bdcdb9e1c3a5df2835909c79f97ab96` | 2026-08-26 | Shenghao Chen | all `-` (2) | no | #175 merged | — | T1 |  |
| 1 landed | `fix/public-api-snapshot-const-drift` | `8afc6bfbae3f01f26a255897c7ca2aa3c8ffe010` | 2026-09-19 | Shenghao Chen | all `-` (1) | no | #490 merged | — | T1 |  |
| 1 landed | `fix/resplit-after-separator` | `1a5409bfeb06a260b4563b7ed6affea6fd0c3f86` | 2026-10-07 | Shenghao Chen | all `-` (1) | no | #684 merged | — | T1 |  |
| 1 landed | `fix/resplit-matrix-682-683` | `fee64c03d43ae7972c77ab59bcc3bd5f75c1ddab` | 2026-10-10 | Shenghao Chen | all `-` (2) | no | #716 merged | #682 issue closed; #683 issue closed | T1 |  |
| 1 landed | `fix/revert-r2-mid-syllable` | `8b80611b3655536523d0a424b1989b9e4fda84fd` | 2026-08-29 | Shenghao Chen | all `-` (4) | no | #218 merged | — | T1 |  |
| 1 landed | `fix/revert-r4-validate-constraint` | `9788ab6db69cba7a304912ded286ce1400cd7758` | 2026-08-29 | Shenghao Chen | all `-` (4) | no | #215 merged | — | T1 |  |
| 1 landed | `fix/revert-r5-constraint-reset` | `5daa0c20cd280dd2bf112f32b2ae02c75181c660` | 2026-08-29 | Shenghao Chen | all `-` (6) | no | #217 merged | — | T1 |  |
| 1 landed | `fix/revert-r6-nbest-cursor` | `cbd8904859c0c9eb796707f0df785eac93d6fb1c` | 2026-08-29 | Shenghao Chen | all `-` (3) | no | #216 merged | — | T1 |  |
| 1 landed | `fix/row30-chewing-force-tone` | `22a4fdde72d86ac975745c95c835d5f8151b0b5a` | 2026-09-14 | Shenghao Chen | all `-` (1) | no | #460 merged | — | T1 |  |
| 1 landed | `fix/row54-empty-parse-unload` | `4ea3423b797839480ff139088b9753fe0f816fcd` | 2026-10-09 | Shenghao Chen | all `-` (3) | no | #709 merged | — | T1 |  |
| 1 landed | `fix/runner-system-dir-guard` | `7bd720e16cc5cbce83c47804fc5ffa6c076a0f43` | 2026-08-29 | Shenghao Chen | all `-` (5) | no | #199 merged | — | T1 |  |
| 1 landed | `fix/rust-analyzer-diagnostics` | `e506a00edc348f799e4267f49bda627887ac14b3` | 2026-09-12 | Shenghao Chen | all `-` (2) | no | #442 merged | — | T1 |  |
| 1 landed | `fix/serialize-locale-tests` | `1a97d2332d931dc2de3e805751e36cf9f569dcb4` | 2026-10-03 | Shenghao Chen | all `-` (1) | no | #631 merged | — | T1 |  |
| 1 landed | `fix/shared-fewest-keys-walk` | `71b3dcd19ad7a4cedbf249f868f880fbf0b17adc` | 2026-08-16 | Shenghao Chen | all `-` (3) | no | #80 merged | — | T1 |  |
| 1 landed | `fix/store-unsafe-waiver-narrowing` | `574d898c8ae02a1c17ce005b0e0106897ffd6018` | 2026-09-13 | Shenghao Chen | all `-` (3) | no | #455 merged | — | T1 |  |
| 1 landed | `fix/tkrzw-build-overrides` | `05a69d3465f122073d416b03b8f8e136800b3cc4` | 2026-09-23 | Shenghao Chen | all `-` (21) | no | #512 merged | — | T1 |  |
| 1 landed | `fix/user-conf-counter-parse` | `5f014c8c318adaf503248c68d8642f8498d866df` | 2026-09-26 | Shenghao Chen | all `-` (10) | no | #584 merged | — | T1 |  |
| 1 landed | `fix/user-conf-fscanf-sequence` | `4059cde8c157110f746e7e8f8f61cb2e142a70a8` | 2026-09-26 | Shenghao Chen | all `-` (13) | no | #591 merged | — | T1 |  |
| 1 landed | `fix/user-conf-open-counter` | `25d69d0a491fdaad60482f7db4bc1e66f10d1a66` | 2026-09-26 | Shenghao Chen | all `-` (2) | no | #578 merged | — | T1 |  |
| 1 landed | `fix/user-file-modes` | `a674529815392db1955fbff9a600bab44c6aacee` | 2026-09-26 | Shenghao Chen | all `-` (4) | no | #579 merged | — | T1 |  |
| 1 landed | `fix/user-index-keep-empty-keys` | `eaea783d246bce4ca26813f9f074d5ba043ba5ac` | 2026-10-14 | Shenghao Chen | all `-` (1) | no | #786 merged | — | T1 |  |
| 1 landed | `fix/user-phrase-tones` | `b912ed8f208e3563ec12376344e5934cbd078a8f` | 2026-10-07 | Shenghao Chen | all `-` (2) | no | #685 merged | — | T1 |  |
| 1 landed | `fix/user-store-read-path` | `1950ee24d2c23455fe99453590b7c90cbe5f1786` | 2026-08-15 | Shenghao Chen | all `-` (1) | no | #74 merged | — | T1 |  |
| 1 landed | `fix/user-store-registry-drain` | `240947227ce5fdcee6fe7011974619310abf9c26` | 2026-08-15 | Shenghao Chen | all `-` (2) | no | #72 merged | — | T1 |  |
| 1 landed | `fix/user-token-reuse-after-mask` | `1d90b1bc2a2cf14d2fcdc6088e3d027990e6e7c4` | 2026-10-14 | Shenghao Chen | all `-` (2) | no | #783 merged | — | T1 |  |
| 1 landed | `fix/w12-ni-double-apostrophe` | `3227231e1d80fbcebc017021b10baa683764b2a4` | 2026-08-20 | Shenghao Chen | all `-` (5) | no | #134 merged | — | T1 |  |
| 1 landed | `fix/w14-nbest-select-path` | `f2d524e6d922af9a38cd34b411ef98da1f70b6ce` | 2026-08-19 | Shenghao Chen | all `-` (6) | no | #117 merged | — | T1 |  |
| 1 landed | `fix/w14-shared-lm-nbest-costs` | `c276a305a542cab0caa71e6b13f42093016a1dce` | 2026-08-19 | Shenghao Chen | all `-` (2) | no | #116 merged | — | T1 |  |
| 1 landed | `fix/zhuyin-unigram-576` | `282f70e760e6fad4f9d01bdf7e80d7ebb5f05640` | 2026-10-11 | Shenghao Chen | all `-` (2) | no | #725 merged | #576 issue closed | T1 |  |
| 1 landed | `full-pinyin-schemes` | `085e88c7e1f0c528246a93644d2e292e833e8c52` | 2026-08-20 | Shenghao Chen | all `-` (25) | no | #126 merged | — | T1 |  |
| 1 landed | `fuzz/session-keys-timeout-seed` | `f594531040445d90b347ba3816a1aba2d6fae215` | 2026-09-24 | Shenghao Chen | all `-` (2) | no | #519 merged | — | T1 |  |
| 1 landed | `fuzz/vn-fixture-model-import` | `33595b7e70047aff0dba9d3ec8b742f0175305c8` | 2026-09-02 | Shenghao Chen | all `-` (2) | no | #278 merged | — | T1 |  |
| 1 landed | `kiro/specs-update` | `d1feb84e85c3328509f9cdbebce3613e675c0c82` | 2026-08-31 | Shenghao Chen | all `-` (5) | no | #239 merged | — | T1 |  |
| 1 landed | `kiro/steering-refresh` | `08a36fb723ce16d6bf75c94a985ce6f34a835067` | 2026-08-31 | Shenghao Chen | all `-` (8) | no | #238 merged | — | T1 |  |
| 1 landed | `lane-525-batch-a` | `785705aa76fe1f2ffa075d02df40fc4588e8dc69` | 2026-10-10 | Shenghao Chen | all `-` (2) | no | #715 merged | #525 issue closed | T1 |  |
| 1 landed | `lane-525-batch-c` | `1fb14fb112f857ee8505dc81326a4f837f8b15b1` | 2026-10-10 | Shenghao Chen | all `-` (5) | no | #717 merged | #525 issue closed | T1 |  |
| 1 landed | `lane-525-batch-d` | `eeef0a7efb335c3ef16705da5797141f9814097c` | 2026-10-10 | Shenghao Chen | all `-` (11) | no | #723 merged | #525 issue closed | T1 |  |
| 1 landed | `lane-525-batch-e` | `3f9fb77abafc7f23a633dc01f6a4ab34f4c610ce` | 2026-10-10 | Shenghao Chen | all `-` (9) | no | #722 merged | #525 issue closed | T1 |  |
| 1 landed | `lane-525-batch-f` | `3c9e4b26a55779d34cfa4eb8de78d2a5d1104260` | 2026-10-10 | Shenghao Chen | all `-` (12) | no | #729 merged | #525 issue closed | T1 |  |
| 1 landed | `lane-525-batch-g` | `53442897b86a72bc43b8f1e5bf178b85108727c9` | 2026-10-10 | Shenghao Chen | all `-` (7) | no | #719 merged | #525 issue closed | T1 |  |
| 1 landed | `lane-563-axis-b-bdb` | `82be7a3b0f9f1a8e7c2e2d0b519572c1f34771a2` | 2026-10-11 | Shenghao Chen | all `-` (3) | no | #748 merged | #563 issue closed | T1 |  |
| 1 landed | `lane-694-table-conf` | `4cb809f44c26cf622b64dd94e042709fcb0420e4` | 2026-10-10 | Shenghao Chen | all `-` (3) | no | #720 merged | #694 issue closed | T1 |  |
| 1 landed | `lane-j/force-tone-641` | `4398bd9d6b7ca2c58c4a22551f484868d3402e9b` | 2026-10-07 | Shenghao Chen | all `-` (4) | no | #691 merged | #641 issue closed | T1 |  |
| 1 landed | `lane-j/session-585-626` | `60cbd41de5f84caaf1ed790fe8f6a96566f46df9` | 2026-10-07 | Shenghao Chen | all `-` (2) | no | #690 merged | #585 issue closed; #626 issue closed | T1 |  |
| 1 landed | `measure/backend-matrix-2026-09` | `14396cee6351b98acbc175eab3f57758e4ddd9a2` | 2026-09-05 | Shenghao Chen | all `-` (2) | no | #339 merged | #9 PR merged | T1 |  |
| 1 landed | `measure/backend-matrix-bdb-store-2026-09` | `b81b07ddeac7963fac7566fd115959f5bb94cb18` | 2026-09-06 | Shenghao Chen | all `-` (5) | no | #354 merged | #9 PR merged | T1 |  |
| 1 landed | `measure/perf-backend-matrix-2026-08-31` | `ea1d0bc817f139894b009b5b7f7bbdab57b4ed7c` | 2026-08-31 | shenghaoc | all `-` (6) | no | #260 merged | #31 PR merged | T1 |  |
| 1 landed | `measure/perf-baseline-kc-validation-2026-08-31` | `d2463126d82327987d83b645eb2c6a463ca38e81` | 2026-08-31 | shenghaoc | all `-` (8) | no | #251 merged | #31 PR merged | T1 |  |
| 1 landed | `opt/lmdb-backend` | `3ca43ba1fc6e45d9ef224b2372cdbc652f9778f3` | 2026-09-05 | Shenghao Chen | all `-` (4) | no | #341 merged | — | T1 |  |
| 1 landed | `opt/redb-alloc-reduction` | `1ba5aa097bbcc4c2683b3e7903d6d38dda2e4e29` | 2026-09-05 | Shenghao Chen | all `-` (4) | no | #340 merged | — | T1 |  |
| 1 landed | `parity/five-surfaces` | `04bec320f2a685b97b93ccb3b462f07c2192f35e` | 2026-08-27 | Shenghao Chen | all `-` (12) | no | #181 merged | — | T1 |  |
| 1 landed | `perf/757-758-lookup` | `c0909a5fc1b5b470e4394a4ffd8184da137626d1` | 2026-10-14 | Shenghao Chen | all `-` (1) | no | #765 merged | #757 issue closed; #758 issue closed | T1 |  |
| 1 landed | `perf/757-758-trellis-reset` | `bce1180c0ab85a6aac471752f057da30c50d4d84` | 2026-10-14 | Shenghao Chen | all `-` (1) | no | #781 merged | #757 issue closed; #758 issue closed | T1 |  |
| 1 landed | `perf/757-759-harness` | `56affabfccdaf48e62ab9643c90e9789c8d45357` | 2026-10-14 | Shenghao Chen | all `-` (2) | no | #767 merged | #757 issue closed; #759 issue closed | T1 |  |
| 1 landed | `perf/757-nbest-step-pricing` | `fc6288df5371f1162e47051a152fee5c4d410a02` | 2026-10-16 | Shenghao Chen | all `-` (1) | no | #810 merged | #757 issue closed | T1 |  |
| 1 landed | `perf/757-pr0-phrase-index-ranges` | `633fbde01ff5a11911dcab3c9937adfec08d99cb` | 2026-10-15 | Shenghao Chen | all `-` (2) | no | #787 merged | #757 issue closed | T1 |  |
| 1 landed | `perf/757-pr2-raw-user-bigram` | `cb5c6919da1558b56c3d981755a2f6c57167bf98` | 2026-10-15 | Shenghao Chen | all `-` (2) | no | #792 merged | #757 issue closed | T1 |  |
| 1 landed | `perf/757-pr3-merged-lookup` | `24e1f27e035a6165b4aa797c806490ac305fee9b` | 2026-10-15 | Shenghao Chen | all `-` (2) | no | #797 merged | #757 issue closed | T1 |  |
| 1 landed | `perf/757-trellis-layout` | `af33c8baad28672cdedbd535bb72e1086312918e` | 2026-10-17 | Shenghao Chen | all `-` (1) | no | #835 merged | #757 issue closed | T1 |  |
| 1 landed | `perf/759-export-iterator` | `b72446ccb578b6cde71bdf156d511653c7d7076e` | 2026-10-15 | Shenghao Chen | all `-` (3) | no | #788 merged | #759 issue closed | T1 |  |
| 1 landed | `perf/alloc-instance-key-cost-cache` | `31a75ee9a10e7023de8172fef3841e0d8ce2c717` | 2026-09-03 | Shenghao Chen | all `-` (3) | no | #303 merged | — | T1 |  |
| 1 landed | `perf/bdb-point-buffer` | `a4369f047fffcee018d5b4b829cdb93758688fbc` | 2026-10-14 | Shenghao Chen | all `-` (1) | no | #766 merged | — | T1 |  |
| 1 landed | `perf/defer-key-cost-table` | `fa1ec3bfa5eb3ae8a97c2a3a382931088cab494f` | 2026-09-04 | Shenghao Chen | all `-` (3) | no | #310 merged | — | T1 |  |
| 1 landed | `perf/dict-lm-load` | `9327379e0e6142313a583c4cf8ad97676ef6d1c5` | 2026-08-19 | Shenghao Chen | all `-` (5) | no | #118 merged | — | T1 |  |
| 1 landed | `perf/ds-algo-packages` | `cee74d73662b33673217610e55e0c16f7af8b3ca` | 2026-09-01 | shenghaoc | all `-` (6) | no | #270 merged | — | T1 |  |
| 1 landed | `perf/fill-lookup` | `5dac940fa82d9d3efa5a86727fb5e50b31d164d1` | 2026-08-20 | Shenghao Chen | all `-` (1) | no | #129 merged | — | T1 |  |
| 1 landed | `perf/init-text-slurp` | `464bd0648a89d210d7e3a7b3efa3a802b87cbe89` | 2026-08-21 | Shenghao Chen | all `-` (4) | no | #136 merged | — | T1 |  |
| 1 landed | `perf/init-typed-map` | `1a3a1f13f4997bba839110a8dad46f22ec71452b` | 2026-08-20 | Shenghao Chen | all `-` (2) | no | #132 merged | — | T1 |  |
| 1 landed | `perf/keystroke-heap-alloc` | `b0b23f0f46e7866548164c8527877ce3ae437143` | 2026-08-19 | Shenghao Chen | all `-` (3) | no | #120 merged | — | T1 |  |
| 1 landed | `perf/mmap-system-indexes-2026-08-31` | `f8722c06ff829b11cb24f34c3053f6300916717a` | 2026-09-03 | Shenghao Chen | all `-` (7) | no | #269 merged | #31 PR merged | T1 |  |
| 1 landed | `perf/oxpinyin-rss-diagnosis-qvjzud` | `77e33058af9a9f91dc9e868715f43859c1e7d7c3` | 2026-09-09 | Shenghao Chen | all `-` (4) | no | #404 merged | — | T1 |  |
| 1 landed | `perf/p2-chewing-table-2026-09-01` | `bd6aeb7f87c3bf2a9787289a993491c3d3c180ac` | 2026-09-03 | Shenghao Chen | all `-` (12) | no | #272 merged | #9 PR merged; #1 PR merged | T1 |  |
| 1 landed | `perf/remove-ffi-catch-panic-abort` | `3133ae5989df52124ed17034e54081430f8d3b95` | 2026-09-04 | Shenghao Chen | all `-` (6) | no | #319 merged | — | T1 |  |
| 1 landed | `perf/revert-release-panic-abort` | `5e630ed084db1e33f0eed3d58b7e070a1f7954c3` | 2026-09-05 | Shenghao Chen | all `-` (1) | no | #337 merged | — | T1 |  |
| 1 landed | `perf/so-size-lto` | `81cde2f4142b2357cbf16012b8b3f6f9fa8efabd` | 2026-09-03 | Shenghao Chen | all `-` (4) | no | #301 merged | — | T1 |  |
| 1 landed | `perf/steady-cycle-constant-factor-mkeg4k` | `c2ff079cb65958dd47e3120575012a427926bf76` | 2026-09-09 | Shenghao Chen | all `-` (3) | no | #394 merged | — | T1 |  |
| 1 landed | `perf/steady-cycle-workload-knob` | `df6c311855a7977acf6ec346d2fef4471d3a7662` | 2026-09-07 | Shenghao Chen | all `-` (3) | no | #379 merged | — | T1 |  |
| 1 landed | `perf/trim-interpolation2-1gram` | `7b2de5ab5a752361b0e5f53e138d970e9e9e24a5` | 2026-09-03 | Shenghao Chen | all `-` (2) | no | #300 merged | — | T1 |  |
| 1 landed | `port/759p1-phrase-table-1015` | `80be5f41cbd28119ed2e2644181fdd3c80947efe` | 2026-10-15 | Shenghao Chen | all `-` (1) | no | #799 merged | #1015 PR merged | T1 |  |
| 1 landed | `pr1-525-contract-cases` | `d0ead6f4b1a73a721a320c226e5d6e4229a035a9` | 2026-10-11 | Shenghao Chen | all `-` (1) | no | #730 merged | #525 issue closed | T1 |  |
| 1 landed | `pr2-525-missing-refusals` | `d30124a213a5ef10dfaa0705b77fb79236a09bc8` | 2026-10-11 | Shenghao Chen | all `-` (2) | no | #731 merged | #525 issue closed | T1 |  |
| 1 landed | `pr3-525-native-fixtures` | `ef1b5f7a2341c81417c61c48026c6f4603b11a96` | 2026-10-11 | Shenghao Chen | all `-` (1) | no | #732 merged | #525 issue closed | T1 |  |
| 1 landed | `pr4-525-chunk-header-read` | `e1437417868a61c9694f592d7df1f38f34fbf26a` | 2026-10-11 | Shenghao Chen | all `-` (4) | no | #733 merged | #525 issue closed | T1 |  |
| 1 landed | `python/portable-wheel-lanes` | `1d221e476b7d01949bf820a87f1debd0a207b5dc` | 2026-09-08 | Shenghao Chen | all `-` (2) | no | #391 merged | — | T1 |  |
| 1 landed | `re-pin-live-178` | `24b8611168d49f22172eaa276d67253c302be794` | 2026-08-27 | Shenghao Chen | all `-` (2) | no | #183 merged | #178 PR merged | T1 |  |
| 1 landed | `refactor/code-sharing-lsp-quality` | `a58abac15cbcac5574121c99a006ea1b533a2247` | 2026-09-01 | Shenghao Chen | all `-` (32) | no | #263 merged | — | T1 |  |
| 1 landed | `refactor/dead-error-variants` | `183db34e0d92ae046d590bf64dc11c154ab6caca` | 2026-09-14 | Shenghao Chen | all `-` (2) | no | #463 merged | — | T1 |  |
| 1 landed | `refactor/default-backend-berkeleydb` | `eaffba5dd71d3acb45e3f67a9c028c6457522b28` | 2026-09-23 | Shenghao Chen | all `-` (20) | no | #502 merged | — | T1 |  |
| 1 landed | `refactor/drop-redb-backend` | `cb24f785f61a1ff996888c69d0991d8f6d1c5243` | 2026-09-22 | Shenghao Chen | all `-` (15) | no | #501 merged | — | T1 |  |
| 1 landed | `refactor/drop-redb-lmdb-backends` | `b59b9b91e99ba31fab088ff4589e185a2001893b` | 2026-09-21 | Shenghao Chen | all `-` (5) | no | #497 merged | — | T1 |  |
| 1 landed | `refactor/facade-pub-crate-ctors` | `04927179dda621e5b1c0d2552f263653fcc6bf80` | 2026-09-21 | Shenghao Chen | all `-` (1) | no | #504 merged | — | T1 |  |
| 1 landed | `refactor/structure-followups` | `4e9c9e641bf1545d8efe1b88e30605c91d96fe23` | 2026-09-12 | Shenghao Chen | all `-` (2) | no | #443 merged | — | T1 |  |
| 1 landed | `review/backend-matrix-followup-2026-09` | `c25812ff0313bf3fcc76027aca9f2577ca024cae` | 2026-09-06 | Shenghao Chen | all `-` (1) | no | #355 merged | #9 PR merged | T1 |  |
| 1 landed | `row65-longer-candidate-offset` | `277cc616cc6ef1e5b6c78c785193209c713694aa` | 2026-10-09 | Shenghao Chen | all `-` (2) | no | #713 merged | — | T1 |  |
| 1 landed | `safety-pr1-mechanics` | `cea24f3ce0eede3443eee995aba4d4dddce32ea1` | 2026-08-29 | Shenghao Chen | all `-` (14) | no | #200 merged | — | T1 |  |
| 1 landed | `safety-pr2-fixes` | `dd6ac2b65f7aadbb7dff26da234da3b9179f2c18` | 2026-08-29 | Shenghao Chen | all `-` (20) | no | #201 merged | — | T1 |  |
| 1 landed | `safety-pr3-lanes` | `2b2d7018f10bec315643133ad66667775b7faafb` | 2026-08-29 | Shenghao Chen | all `-` (24) | no | #211 merged | — | T1 |  |
| 1 landed | `safety-pr4-trials` | `6e01c9a104a37fab62ba8253034dcbc302cf31f9` | 2026-08-29 | Shenghao Chen | all `-` (36) | no | #212 merged | — | T1 |  |
| 1 landed | `scheme-contract-lock` | `9e6779c1c111b28402ad5c535a02aebb3b0c190b` | 2026-08-20 | Shenghao Chen | all `-` (27) | no | #127 merged | — | T1 |  |
| 1 landed | `shenghaoc-commit-trailer-ci-lint` | `4b090c77ef59a64483d704bf78aa23e7cc87a79f` | 2026-08-15 | Shenghao Chen | all `-` (4) | no | #56 closed | — | T1 |  |
| 1 landed | `shenghaoc-facade-extract-pinyin` | `d5fcdf900d692c43738e864967790cff1ba86f8d` | 2026-09-04 | Shenghao Chen | all `-` (5) | no | #313 merged | — | T1 |  |
| 1 landed | `shenghaoc-facade-extract-pinyin-1` | `92e8cfb3497c2c04dd882baf47f8dc1577a171b9` | 2026-09-04 | Shenghao Chen | all `-` (3) | no | #312 merged | — | T1 |  |
| 1 landed | `shenghaoc-w8-packaging` | `63412940e0ce3fb9b8ca351bc50285642443fdd0` | 2026-08-16 | Shenghao Chen | all `-` (1) | no | #89 merged | — | T1 |  |
| 1 landed | `shenghaoc-w9-t0-training-characterization` | `61b1f42d56aeb1152d393029d0e837384a3d7490` | 2026-08-15 | Shenghao Chen | all `-` (1) | no | #51 merged | — | T1 |  |
| 1 landed | `shenghaoc-w9-t1-segmenter` | `e487288e647fc44c7ac757947c22d87d7996040c` | 2026-08-15 | Shenghao Chen | all `-` (2) | no | #52 merged | — | T1 |  |
| 1 landed | `shenghaoc-w9-t2-counter` | `9a3e4bb41557bca01c5a7c6657b8c33771b337b1` | 2026-08-15 | Shenghao Chen | all `-` (3) | no | #53 merged | — | T1 |  |
| 1 landed | `shenghaoc-w9-t3-lambda-estimator` | `8977ca211503606593419c7a427a8485ce7d08b2` | 2026-08-15 | Shenghao Chen | all `-` (4) | no | #54 merged | — | T1 |  |
| 1 landed | `shenghaoc-zhuyin-py-facade` | `e091a3bb40b453d74b17ea4d30f1a3ede57fe1d7` | 2026-09-05 | Shenghao Chen | all `-` (3) | no | #321 merged | — | T1 |  |
| 1 landed | `split/s1-storage-seam` | `32ec7b80144b5cf4004ab4ed05bbee2dafd5c9eb` | 2026-09-13 | Shenghao Chen | all `-` (2) | no | #447 merged | — | T1 |  |
| 1 landed | `split/s2-data-readers` | `dba75c6303a37dcf2cf03996af2a6ba5e29e9184` | 2026-09-13 | Shenghao Chen | all `-` (4) | no | #448 merged | — | T1 |  |
| 1 landed | `split/s3-session-seam` | `fd7d80ec124a56ad09c0f8a3208d30251d793396` | 2026-09-13 | Shenghao Chen | all `-` (5) | no | #449 merged | — | T1 |  |
| 1 landed | `split/s4-capi-pair` | `9f054c44c11dc63eba762bcfad018ce5c54d6ffb` | 2026-09-13 | Shenghao Chen | all `-` (6) | no | #450 merged | — | T1 |  |
| 1 landed | `split/s5-training` | `92b95b8f160f177a2a6b7aa262b69c42a2309fed` | 2026-09-13 | Shenghao Chen | all `-` (7) | no | #451 merged | — | T1 |  |
| 1 landed | `split/s6-tooling` | `9ff2d3d04bcacdde75bec50a57420b5e2cb294f4` | 2026-09-13 | Shenghao Chen | all `-` (9) | no | #452 merged | — | T1 |  |
| 1 landed | `store/common-helpers` | `c81ef9d84a24b729400a9e487c827875ae54e7c5` | 2026-09-07 | Shenghao Chen | all `-` (1) | no | #387 merged | — | T1 |  |
| 1 landed | `style/engine-user-idiomatic` | `0f7fde7f661e05d6c096493ddabff064ade309f7` | 2026-08-28 | Shenghao Chen | all `-` (2) | no | #195 merged | — | T1 |  |
| 1 landed | `task9/1-data-codecs` | `e152c32327d48866c94775dc07cd0077cccb8c11` | 2026-09-11 | Shenghao Chen | all `-` (3) | no | #421 merged | — | T1 |  |
| 1 landed | `task9/2-store-containers` | `815aa71408a298121bca04f1ff20a61415b16752` | 2026-09-11 | Shenghao Chen | all `-` (4) | no | #422 merged | — | T1 |  |
| 1 landed | `task9/3-user-persistence` | `5ccbff73b342f7b2622e1e956ba2484feab79e52` | 2026-09-11 | Shenghao Chen | all `-` (5) | no | #423 merged | — | T1 |  |
| 1 landed | `task9/4-session-wiring` | `627b40a377f4542b1e1bd3865bd1a18b3ae9e5ca` | 2026-09-11 | Shenghao Chen | all `-` (6) | no | #424 merged | — | T1 |  |
| 1 landed | `task9/5-oracle-differential` | `a85e584d428fafaed37b8bcae993bcb9208cb28d` | 2026-09-11 | Shenghao Chen | all `-` (7) | no | #425 merged | — | T1 |  |
| 1 landed | `test-525-native-empty-system-index` | `4b126177a89ac8b9be158223274f05b9dd797e28` | 2026-10-11 | Shenghao Chen | all `-` (1) | no | #737 merged | #525 issue closed | T1 |  |
| 1 landed | `test/oracle-transformed-options-651` | `b93ff32224f679ffc3ebd6bd7669b570bd72b96a` | 2026-10-07 | Shenghao Chen | all `-` (2) | no | #688 merged | #651 PR merged | T1 |  |
| 1 landed | `test/skips-become-ignores` | `b848a66b8c31d248418d00cfa0ab88e5ba874755` | 2026-09-07 | Shenghao Chen | all `-` (2) | no | #373 merged | — | T1 |  |
| 1 landed | `test/w13-scheme-remeasure` | `5193e8bc1ad9cb1129619e61d2c13475f8049e6a` | 2026-08-19 | Shenghao Chen | all `-` (2) | no | #115 merged | — | T1 |  |
| 1 landed | `wip/1297` | `5015c289528f20e885c9fefa545d10185b37b83b` | 2026-10-26 | Shenghao Chen | all `-` (1) | no | — | #1297 issue open | T1 |  |
| 1 landed | `wip/757-fused` | `bc642d89925ebca47d1b790abe6a2abad4adc828` | 2026-10-16 | Shenghao Chen | all `-` (2) | no | #821 merged | #757 issue closed | T1 |  |
| 1 landed | `wip/757-lazy-candidates` | `0817632195da44e5c745a0bc938540df92d50f64` | 2026-10-16 | Shenghao Chen | all `-` (1) | no | #829 merged | #757 issue closed | T1 |  |
| 1 landed | `wip/757-log-cache` | `5cea5b8d571b48a937d6bbf61cff1b84577483f2` | 2026-10-16 | Shenghao Chen | all `-` (1) | no | #833 merged | #757 issue closed | T1 |  |
| 1 landed | `wip/757-priced-token` | `a3e883986d3dfa9926925b1eb3739581df281bcb` | 2026-10-16 | Shenghao Chen | all `-` (1) | no | #831 merged | #757 issue closed | T1 |  |
| 1 landed | `wip/757-trellis-layout` | `af33c8baad28672cdedbd535bb72e1086312918e` | 2026-10-17 | Shenghao Chen | all `-` (1) | no | — | #757 issue closed | T1 |  |
| 1 landed | `wip/785` | `c5fd63a1e231f7754a47913525a2b1e7ef62ff14` | 2026-10-16 | Shenghao Chen | all `-` (1) | no | #824 merged | #785 issue closed | T1 |  |
| 1 landed | `wip/793` | `d490f96c14db6f36f5561e3148800d253b792348` | 2026-10-17 | Shenghao Chen | all `-` (1) | no | #822 merged | #793 issue closed | T1 |  |
| 1 landed | `wip/814` | `c928b83452f6b56bf1c665a7a84797cdd3f38812` | 2026-10-16 | Shenghao Chen | all `-` (1) | no | #820 merged | #814 issue closed | T1 |  |
| 1 landed | `wip/818` | `d8b8bbfe04297b1f01e2ffb94a8e18da05336fbb` | 2026-10-16 | Claude | all `-` (3) | no | #819 merged | #818 issue closed | T1 |  |
| 1 landed | `wip/825` | `1034df8a21d6a1169920c08fdfdd21d920791795` | 2026-10-16 | Shenghao Chen | all `-` (1) | no | #828 merged | #825 issue closed | T1 |  |
| 1 landed | `wip/826` | `36bf13ed338b254d90bbf53dd54c60231e413a6a` | 2026-10-17 | Claude | all `-` (2) | no | #827 merged | #826 issue closed | T1 |  |
| 1 landed | `wip/830` | `2142b88cc7626e223ed039323de20e2a425ef8b7` | 2026-10-17 | Shenghao Chen | all `-` (1) | no | #832 merged | #830 issue closed | T1 |  |
| 1 landed | `wip/926` | `1828cadd385ad605af232950054e4b487c8d4e1e` | 2026-10-26 | Shenghao Chen | all `-` (1) | no | — | #926 issue closed | T1 |  |
| 1 landed | `wip/971-measured` | `3e1c045031597f631b849d3746727119069e2e72` | 2026-10-21 | Shenghao Chen | all `-` (1) | no | — | #971 issue closed | T1 |  |
| 1 landed | `wip/999-repro` | `c2d3641c4aa0f037f8871642af1a158548133256` | 2026-10-21 | Shenghao Chen | all `-` (2) | no | — | #999 PR merged | T1 |  |
| 1 landed | `zhuyin-dachen-cp26` | `2d1642890fd07d5a4cdd79818649311742bb5b0c` | 2026-08-20 | Shenghao Chen | all `-` (18) | no | #125 merged | — | T1 |  |
| 1 landed | `zhuyin-discrete` | `35026a05ff7022f5d9de1f85e4bd8f1790bfd526` | 2026-08-20 | Shenghao Chen | all `-` (13) | no | #124 merged | — | T1 |  |
| 1 landed | `zhuyin-index-fidelity` | `0e207e3c0a77ad32e99fd4e453e3c1940004593a` | 2026-08-20 | Shenghao Chen | all `-` (4) | no | #122 merged | — | T1 |  |
| 1 landed | `zhuyin-simple-keyboards` | `387483e812a852c965c3e3850758a08c2010cb02` | 2026-08-20 | Shenghao Chen | all `-` (9) | no | #123 merged | — | T1 |  |
| 1 landed | `zhuyin/candidate-window` | `4be4588ecec165fcb68899c9dddc898f0a3088fb` | 2026-09-02 | Shenghao Chen | all `-` (5) | no | #286 merged | — | T1 |  |
| 2 closed PR | `abi/extract-foundation` | `ac74b77815fa9cd3e1bd26485daaa28df32ef3f0` | 2026-10-17 | Shenghao Chen | 1 `+`, 0 `-` | no | #836 closed | — | keep | refactor(abi): extract the generic libpinyin vocabulary into libpinyin-a |
| 2 closed PR | `abi/subject` | `8f088191176e8c57e270bacaa6e091bb57f1f1ca` | 2026-10-17 | Shenghao Chen | 3 `+`, 0 `-` | no | #837 closed | — | keep | refactor(abi): extract the generic libpinyin vocabulary into libpinyin-a |
| 2 closed PR | `calib/c1-phrase-key-bonus-sweep` | `bc242d0f630dff74893daa469dceae77c78695fa` | 2026-08-13 | Shenghao Chen | 1 `+`, 0 `-` | no | #37 closed | — | T2 | C1: phrase_key_bonus re-sweep — 1,000 remains optimal (negative result) |
| 2 closed PR | `ci/verify-nightly-apt` | `09434ff7bd5e6a6fe158bcb5d6e1d682533e7bfb` | 2026-08-31 | Shenghao Chen | 1 `+`, 0 `-` | no | #264 closed | — | T2 | ci(verify-nightly): fix apt deps and miri --component syntax |
| 2 closed PR | `ci/verify-nightly-apt-fix` | `de7c8eeb61311ac2de99d283d9f12301e5e3f99c` | 2026-08-31 | Shenghao Chen | 1 `+`, 0 `-` | no | #262 closed | — | T2 | ci(verify-nightly): fix apt deps and miri --component syntax |
| 2 closed PR | `ci/verify-nightly-checkout-git` | `eed982bbdc5de27feec531808f8c4c9033ef3c29` | 2026-08-31 | Shenghao Chen | 2 `+`, 0 `-` | no | #265 closed | — | T2 | ci(verify-nightly): fix apt deps and miri --component syntax |
| 2 closed PR | `ci/verify-nightly-fix` | `b34bada95749c46d5895660a5738b284ade5edf1` | 2026-09-01 | Shenghao Chen | 2 `+`, 0 `-` | no | #271 closed | — | T2 | ci(verify-nightly): fix the four root causes that kept every job red |
| 2 closed PR | `claude/great-franklin-hilcey` | `173677ca6ff14ee45fee74e78477f63e7dd0f758` | 2026-10-22 | Shenghao Chen | 3 `+`, 0 `-`; 1 same subject on main | no | #1039 closed | — | keep | perf(data,user): replay each system phrase journal in its own mapping |
| 2 closed PR | `claude/initbench-independent` | `333598252238457863346e696ef74ad49a3a4da1` | 2026-10-17 | Claude | 4 `+`, 0 `-` | no | #843 closed | — | keep | initbench: independent init/startup benchmark of two built libpinyin art |
| 2 closed PR | `claude/oxpinyin-dbm-fuzzing-6cdb7b` | `f258a8edbad66a0c3d356d3d98e6af1e66d4cb50` | 2026-09-10 | Shenghao Chen | 2 `+`, 0 `-` | no | #408 closed | — | keep | fuzz: hostile database files through the store seam |
| 2 closed PR | `claude/perf-gate-capture` | `cd2e00d376021c3c25fe32c0f245eb6eda2a9dce` | 2026-09-12 | Shenghao Chen | 4 `+`, 0 `-` | no | #431 closed | — | keep | docs(perf): finish the gate proposal -- recipe term, schema, lane |
| 2 closed PR | `claude/perf-gate-checker` | `bdfef15be11bc6a6c856593a1975ef63973e7d17` | 2026-09-12 | Shenghao Chen | 3 `+`, 0 `-` | no | #430 closed | — | keep | docs(perf): finish the gate proposal -- recipe term, schema, lane |
| 2 closed PR | `claude/perf-gate-proposal-finish` | `e3dc6da301dbec95934c0bce4f06adfe5a6fe5ca` | 2026-09-12 | Shenghao Chen | 2 `+`, 0 `-` | no | #429 closed | — | keep | docs(perf): finish the gate proposal -- recipe term, schema, lane |
| 2 closed PR | `claude/phrase-text-glib-compat-gngi60` | `dd030e36f9954464609e471af0148c7e0d5227ea` | 2026-10-19 | Claude | 2 `+`, 0 `-` | no | #904 closed | — | keep | test(contract): cases for phrase text that g_ucs4_to_utf8 does not turn  |
| 2 closed PR | `claude/phrase-text-glib-compat-gngi60-b` | `0cd14caad8ecf232b18fa3723f8dbe81435bf803` | 2026-10-19 | Claude | 4 `+`, 0 `-` | no | #912 closed, #910 closed | — | keep | test(contract): cases for phrase text that g_ucs4_to_utf8 does not turn  |
| 2 closed PR | `claude/tooling-1234-image-builds` | `c589a19ecbc0fdbb5bdb025ed5189a049406554a` | 2026-10-25 | Claude | 2 `+`, 0 `-`; 1 same subject on main | no | #1263 closed | #1234 issue closed | keep | tooling: bound concurrent setup cell builds under one CPU and memory bud |
| 2 closed PR | `claude/tooling-language-consolidation-dvahgz` | `a204f9c0e6874eee9d08795fe77a42b7a25da1d6` | 2026-10-17 | Claude | 5 `+`, 0 `-` | no | #842 closed | — | keep | initbench: independent init/startup benchmark of two built libpinyin art |
| 2 closed PR | `coderabbit/add-comprehensive-pr-tests/3dd68af2` | `b868056acaa21d0519748f1c3bdcac3f1287e263` | 2026-08-28 | coderabbitai[bot] | 1 `+`, 7 `-` | no | #194 closed | — | T2 | Generate unit tests for PR #188 |
| 2 closed PR | `coderabbit/add-pull-request-tests/282701c5` | `f828deed0e102df5afc06151572998e9ed6cfa97` | 2026-08-28 | coderabbitai[bot] | 1 `+`, 4 `-` | no | #193 closed | — | T2 | Generate unit tests for PR #187 |
| 2 closed PR | `coderabbitai/docstrings/17b5963` | `baf3f0706cc6c429144001879c3ec433fccdd3db` | 2026-08-27 | coderabbitai[bot] | 1 `+`, 5 `-` | no | #185 closed | — | T2 | Add docstrings to 'data-pipeline-parity' |
| 2 closed PR | `coderabbitai/docstrings/48f7a9d` | `7debc3d498eebc0f0868a7c9d00abf4fe90dbc7e` | 2026-08-27 | coderabbitai[bot] | 1 `+`, 7 `-` | no | #184 closed | — | T2 | Add docstrings to 'parity/five-surfaces' |
| 2 closed PR | `docs/bug-for-bug-audit-2026-09-23` | `4a9fddb57e273d18d63e985c17425a59f1163015` | 2026-09-23 | Shenghao Chen | 1 `+`, 0 `-` | no | #516 closed | #9 PR merged; #23 PR merged | keep | docs(findings): bug-for-bug compatibility audit vs the libpinyin pin |
| 2 closed PR | `docs/fcitx-libpinyin-pin` | `47c0c58d24c92b84b1d49d407d4413211afc7c8c` | 2026-09-16 | Shenghao Chen | 1 `+`, 0 `-` | no | #477 closed | — | keep | docs: freeze the fcitx-libpinyin 0.5.4 source identity and manifest |
| 2 closed PR | `docs/findings/verify-nightly-overrides` | `9e0070bee267749cc40e5a9c62fd0fbbb7a9fa9d` | 2026-08-31 | Shenghao Chen | 4 `+`, 0 `-` | no | #267 closed | — | T2 | ci(verify-nightly): fix apt deps and miri --component syntax |
| 2 closed PR | `docs/hygiene-2026-09-pr-rebase` | `a9fcfb43ff82f668b3f167e39ea2f05066000150` | 2026-09-17 | Shenghao Chen | 1 `+`, 0 `-` | no | #475 closed | #9 PR merged | keep | docs(findings): sections 5-6 — the unauthorized Phase 2 rebase and the h |
| 2 closed PR | `feat/candidate-construction` | `db2490eff7fe1f759dba83101dc81d0babdac4b5` | 2026-08-13 | Shenghao Chen | 1 `+`, 0 `-` | no | #40 closed | — | keep | feat(engine): reproduce the pin's candidate construction |
| 2 closed PR | `feat/integration-syllable-encoder` | `c3f362d7c29960043de70a4a4c10cdbee7e13188` | 2026-08-10 | Shenghao Chen | 6 `+`, 0 `-` | no | #28 closed | — | keep | docs(findings): freeze syllable-encoder mapping |
| 2 closed PR | `feat/user-files-libpinyin-format` | `d31f46ea763924fac7509c633068023233bef115` | 2026-09-10 | Shenghao Chen | 15 `+`, 0 `-`; 2 same subject on main | no | #406 closed | — | keep | data: the libpinyin user-file codecs move in beside their readers |
| 2 closed PR | `followup/pedantic-linux-sweep` | `2c1920bd998f3f33f7fd3abf3a34417e88efbb60` | 2026-09-12 | Shenghao Chen | 1 `+`, 0 `-` | no | #444 closed | — | keep | chore(lints): the #263 follow-up sweep — clear the re-emitted audited su |
| 2 closed PR | `fuzz/fixture-model-import` | `6397ec89e42a00741980f934f1ecd3a080f0ebb3` | 2026-08-31 | Shenghao Chen | 3 `+`, 0 `-` | no | #266 closed | — | T2 | ci(verify-nightly): fix apt deps and miri --component syntax |
| 2 closed PR | `kiro/hooks-and-agents` | `0cb3de75b43e1d2276289514ce007ad325146a96` | 2026-08-30 | Shenghao Chen | 1 `+`, 3 `-` | no | #240 closed | — | keep | docs(kiro): add hooks and agent configs in the KiroCrew formats |
| 2 closed PR | `perf/kc-cxx-bridge` | `598443806ded775674dcf97c7b3f35956bbb7f44` | 2026-10-22 | Shenghao Chen | 1 `+`, 5 `-` | no | #1043 closed | — | keep | refactor(store): bind Kyoto Cabinet through a cxx bridge to its classes |
| 2 closed PR | `perf/user-journal-old-item-borrowed` | `c1a238eb086daa75474119b83eb922575bc67f10` | 2026-10-17 | Claude | 1 `+`, 0 `-` | no | #846 closed | — | keep | perf(user): compare journal old items without materializing them |
| 2 closed PR | `repository-analysis-3214107716791942969` | `1580553754137e0fe1a1f8ebd90557de8eb043ea` | 2026-10-28 | google-labs-jules[bot] | 1 `+`, 0 `-` | no | #1335 closed | — | T2 | Analyse repository architecture, policy, and diagnostics |
| 2 closed PR | `shenghaoc-zhuyin-python-bindings` | `5b7e9ef1eb0a9bd0c0ca7c2d4440089ece2325ae` | 2026-09-04 | Shenghao Chen | 1 `+`, 0 `-` | no | #309 closed | — | T2 | feat(python): bind the zhuyin/chewing engine as oxpinyin.zhuyin |
| 2 closed PR | `test/1241-skip-proof` | `33a1a1cc2d25547f46b7a7ff8b4380f90a038c8c` | 2026-10-25 | Claude | 2 `+`, 1 `-` | no | #1249 closed | #1241 issue closed | T2 | docs: proof push 2 (Markdown only, image tag unchanged) |
| 2 closed PR | `wip/1031-hasher` | `41326a235c8bf008ec26d1bd69da3fac8920fed1` | 2026-10-23 | Shenghao Chen | 4 `+`, 0 `-` | no | #1115 closed | #1031 issue open | keep | perf: share fixed-seed hashing on the guess path |
| 2 closed PR | `wip/1297-ranking` | `03b69118ff061ffa0f580d3c51c559f691276f67` | 2026-10-28 | Shenghao Chen | 3 `+`, 0 `-` | no | #1314 closed | #1297 issue open | keep | refactor(nbest): simplify the directed trellis predicate |
| 3 no PR | `ci/oracle-provisioning-ubuntu-source` | `a696e080c19fe18223e91ca62917f0a4489df24f` | 2026-08-28 | Shenghao Chen | 1 `+`, 0 `-` | no | — | — | T2 | tools(oracle): fetch the pin from sources this network can reach |
| 3 no PR | `claude/dynamic-adjust-bigram-ranking-yxbu5z` | `dade7192018f4729e4229cbc58c3f2b4bc1669f0` | 2026-08-18 | Claude | 1 `+`, 0 `-` | no | — | — | T2 | docs(w10): revalidate DYNAMIC_ADJUST set-state stays unreachable (#99) |
| 3 no PR | `claude/elegant-davinci-ua3i7o` | `9eb9811396e8a5edf87cbc2e336d185b0567bff3` | 2026-08-30 | Claude | 5 `+`, 7 `-`; 4 same subject on main | no | — | — | keep | test(bisection): add the key-surface differential against the pinned ora |
| 3 no PR | `claude/init-gap-1090-analysis-7it4g0` | `b1460c4c996070f581f579aa1587bfbc1ccf4a3d` | 2026-10-23 | Shenghao Chen | 18 `+`, 2 `-`; 4 same subject on main | no | — | #1090 issue closed | keep | tools(teardown): verify per-file cold cache eviction |
| 3 no PR | `claude/pr-179-review-fixes-9jzpny` | `6e9375730eae0680d8ee4ab2cb5efd884f1a9e14` | 2026-08-27 | Shenghao Chen | 21 `+`, 0 `-` | no | — | #179 PR closed | T2 | feat(python): bind the engine session API through PyO3 |
| 3 no PR | `docs/post-audit-cleanup` | `0462ff70bbf246bb92353d1bbcf44ef3f10dbbd5` | 2026-09-06 | Shenghao Chen | 3 `+`, 0 `-` | no | — | — | keep | docs(kiro): drop the pre-P6 compat modules and the 58-symbol surface fro |
| 3 no PR | `feat/bdb-backend` | `468f473380e9521a2c49d13a8482901a446b1663` | 2026-08-28 | Shenghao Chen | 7 `+`, 11 `-`; 6 same subject on main | no | — | — | keep | docs(store): bisect the Ubuntu libtkrzw pointer-identity breakage |
| 3 no PR | `feat/revert-incompatible-divergences` | `6538cd7b875ff784de2fec2c832d546dd55e9b23` | 2026-08-28 | Shenghao Chen | 3 `+`, 17 `-`; 2 same subject on main | no | — | — | keep | feat(capi): SONAME libpinyin.so.15, consumer-union scope, and the loader |
| 3 no PR | `measure/dropin-stack-oracle` | `31182159f2232bb89b29297b0d2f13e8d23ff010` | 2026-08-28 | Shenghao Chen | 7 `+`, 13 `-`; 6 same subject on main | no | — | — | keep | docs(store): bisect the Ubuntu libtkrzw pointer-identity breakage |
| 3 no PR | `tooling/perf-snapshot-compare` | `5f00b72caa63ea68ab516f42548d5a828551f892` | 2026-10-25 | Shenghao Chen | 1 `+`, 0 `-` | no | — | — | keep | tools: compare fixed revisions on the nightly snapshot workload |
| 3 no PR | `wip/1031-b` | `11f24058b2d2ebb468c87439e38d4bf191ae5ab1` | 2026-10-22 | Shenghao Chen | 3 `+`, 0 `-`; 1 same subject on main | no | — | #1031 issue open | keep | perf(user): memoise absent user-unigram rows |
| 3 no PR | `wip/1031-first-use` | `05053c8042171cb7e7956d35b329b4cfc53c6afe` | 2026-10-27 | Shenghao Chen | 1 `+`, 1 `-` | no | — | #1031 issue open | keep | perf: price replayed system pronunciations without payload decoding |
| 3 no PR | `wip/1031-memo` | `52125a7332b449b9f7bc483c5a9dc9efb2c21502` | 2026-10-23 | Shenghao Chen | 6 `+`, 0 `-` | no | — | #1031 issue open | keep | perf: share fixed-seed hashing on the guess path |
| 3 no PR | `wip/1031-resolve-utf8` | `11e32fd39c7fbaa66b0b3cda72edc0f0221442e1` | 2026-10-27 | Shenghao Chen | 1 `+`, 0 `-` | no | — | #1031 issue open | keep | perf(data): bulk-encode mapped scalar phrase text |
| 3 no PR | `wip/1031-snapshot-append` | `adbf1574fb488cf4b2db4fce43f0d980ec810629` | 2026-10-27 | Shenghao Chen | 2 `+`, 0 `-` | no | — | #1031 issue open | keep | perf(capi): append typed scalar snapshot text without rescanning |
| 3 no PR | `wip/1031-snapshot-append-2` | `3cfb12487222d0a8b8498efdf57035f480b103ff` | 2026-10-27 | Shenghao Chen | 2 `+`, 0 `-` | no | — | #1031 issue open | keep | perf(capi): append typed scalar snapshot text without rescanning |
| 3 no PR | `wip/1071-probe-current` | `bce98357cc04732802682649ed68f395ff45e5ac` | 2026-10-23 | Shenghao Chen | 1 `+`, 1 `-` | no | — | #1071 issue closed | T2 | probe(1071): run only function-order-policy on dispatch |
| 3 no PR | `wip/1071-probe-stale` | `ad1f02a8b96884893a7e6737061fd4d6396b03c0` | 2026-10-23 | Shenghao Chen | 2 `+`, 1 `-` | no | — | #1071 issue closed | T2 | probe(1071): run only function-order-policy on dispatch |
| 3 no PR | `wip/1091-cold-cache` | `12938b23748839a4b54418e944ea79c31c321001` | 2026-10-23 | Shenghao Chen | 2 `+`, 0 `-`; 1 same subject on main | no | — | #1091 issue closed | keep | tools(teardown): verify per-file cold cache eviction |
| 3 no PR | `wip/1099-tkrzw-init` | `b764e197735d023270f365266575fcfda9dea8db` | 2026-10-23 | Shenghao Chen | 1 `+`, 0 `-` | no | — | #1099 issue closed | keep | docs(1099): prepare sequential Ubuntu init attribution |
| 3 no PR | `wip/1124-kc-include-order` | `63a45669b42f1be37a2af8759e147be512e6cc98` | 2026-10-24 | Shenghao Chen | 2 `+`, 0 `-` | no | — | #1124 issue closed | keep | fix(store): prioritize the Kyoto Cabinet include override |
| 3 no PR | `wip/1125-tkrzw-shim` | `fccb87416fa58b7fdf08660735c95553da5f267a` | 2026-10-24 | Shenghao Chen | 4 `+`, 0 `-` | no | — | #1125 issue closed | keep | perf(store): construct concrete tkrzw DBMs through a C++ shim |
| 3 no PR | `wip/1190` | `776de43e8d0248baa94486f79b9143a2f7b75a75` | 2026-10-25 | Shenghao Chen | 16 `+`, 0 `-`; 1 same subject on main | no | — | #1190 PR merged | T2 | docs: require natural-Rust reproduction in limb 3 |
| 3 no PR | `wip/1229-hex-dot` | `998330d4f52d7d28604c5299c0bae85e206cb669` | 2026-10-25 | Shenghao Chen | 5 `+`, 0 `-` | no | — | #1229 PR merged | T2 | chore(images): move the oracle to trixie and testing images off the snap |
| 3 no PR | `wip/1229-pre-squash` | `795f2c04a9e2b5b9dd8434fd4ebf54c591683acb` | 2026-10-25 | Shenghao Chen | 5 `+`, 0 `-` | no | — | #1229 PR merged | T2 | chore(images): move the oracle to trixie and testing images off the snap |
| 3 no PR | `wip/1297-ranking-doc-complexity-20261028` | `03b69118ff061ffa0f580d3c51c559f691276f67` | 2026-10-28 | Shenghao Chen | 3 `+`, 0 `-` | no | — | #1297 issue open | keep | refactor(nbest): simplify the directed trellis predicate |
| 3 no PR | `wip/1297-ranking-final-20261027` | `60ecb96952749b255123bd11e55a1d45af4a6419` | 2026-10-27 | Shenghao Chen | 3 `+`, 0 `-` | no | — | #1297 issue open | keep | refactor(nbest): simplify the directed trellis predicate |
| 3 no PR | `wip/1297-ranking-final-20261027-stop` | `291d4b34726aa5b907dd015fdaef45814e49e3d9` | 2026-10-27 | Shenghao Chen | 3 `+`, 0 `-` | no | — | #1297 issue open | keep | refactor(nbest): simplify the directed trellis predicate |
| 3 no PR | `wip/1297-ranking-final-20261028-composition` | `3523dedd9bada1c67ed5d8a470388b06886620b1` | 2026-10-28 | Shenghao Chen | 3 `+`, 0 `-` | no | — | #1297 issue open | keep | refactor(nbest): simplify the directed trellis predicate |
| 3 no PR | `wip/1297-ranking-final-20261028-measure` | `62fe1e947c374af400b1e11ae397f9b8edf788a1` | 2026-10-28 | Shenghao Chen | 3 `+`, 0 `-` | no | — | #1297 issue open | keep | refactor(nbest): simplify the directed trellis predicate |
| 3 no PR | `wip/853` | `5e934f9208a555b652d852cef7e45df8b54fe913` | 2026-10-17 | Claude | 1 `+`, 0 `-` | no | — | #853 issue closed | keep | test(contract-diff): a unigram added to a live system item reaches the s |
| 3 no PR | `wip/894` | `167497e032af71166469a5db3668b5188186a062` | 2026-10-19 | Claude | 4 `+`, 0 `-` | no | — | #894 issue closed | keep | test(contract): cases for phrase text that g_ucs4_to_utf8 does not turn  |
| 3 no PR | `wip/926-pre-squash` | `3afb7368bbde7f38886b0e4419e023179b855245` | 2026-10-25 | Shenghao Chen | 4 `+`, 0 `-` | no | — | #926 issue closed | T2 | fix(tools): validate phrase records and honor NUL byte boundaries |
| 3 no PR | `wip/929-pre-squash` | `4555421a0045a8c333e08e4ce2f9e3651ce4d901` | 2026-10-25 | Shenghao Chen | 3 `+`, 1 `-`; 1 same subject on main | no | — | #929 issue closed | T2 | test(contract): observe late save interruption and physical bytes (refs  |
| 3 no PR | `wip/985` | `abf05208cd05f4f28fb36ccbbb41d7bbb7eaea3b` | 2026-10-22 | Shenghao Chen | 3 `+`, 0 `-`; 1 same subject on main | no | — | #985 issue open | keep | perf(data,user): replay each system phrase journal in its own mapping |
| 3 no PR | `wip/986-phase2a-checkpoint-93986ae1` | `93986ae122e74393cc58b7a3fc545fbde1f4d831` | 2026-10-21 | Shenghao Chen | 2 `+`, 0 `-` | no | — | #986 issue closed | keep | docs(packaging): plan optional measured function ordering |
| 3 no PR | `wip/994-scan-scratch` | `0c58df3fa13cfb9828f2e1d22822a28eabd33e63` | 2026-10-22 | Shenghao Chen | 4 `+`, 2 `-`; 1 same subject on main | no | — | #994 issue closed | keep | perf(engine): append window-scan hits straight to the window |
| 3 no PR | `wip/compact-zhuyin` | `4797d21366f24dbccd3794775c2474aca6c23ffd` | 2026-10-18 | Shenghao Chen | 4 `+`, 0 `-` | no | — | — | keep | wip(core): prototype pooled global Zhuyin index |
| 3 no PR | `wip/docstrings-audit` | `5934978445d4aaa886695b8f259a2b8942f62ac8` | 2026-10-25 | Shenghao Chen | 1 `+`, 0 `-` | no | — | — | keep | docs(audit): document call-site generator entry point |
| 4 unclear | `abi-parity-tier-c` | `d5a01904be35f35763aba4eb5ecfd4a2b3a3ad06` | 2026-08-30 | Shenghao Chen | 2 `+`, 2 `-`; 2 same subject on main | no | #225 merged | — | T2 | feat(capi,core): consume oxpinyin-chewing; land the Tier-A ABI surface ( |
| 4 unclear | `chore/oracle-pin-074a221` | `bedff251d77dfbf90c3cdca24ed6b8fa39cb31e8` | 2026-09-07 | Shenghao Chen | 1 `+`, 3 `-` | no | #363 merged | — | keep | chore(oracle): tidy the 074a2219 pin follow-ups |
| 4 unclear | `chore/oracle-pin-followup` | `c48ca383b3c27b35db60f55ea76e8fe777a7308a` | 2026-09-06 | Shenghao Chen | 1 `+`, 3 `-` | no | #372 merged | — | keep | chore(oracle): tidy the 074a2219 pin follow-ups |
| 4 unclear | `claude/cache-key-cost-table-0qixdj` | `be3f96f74fd4c57d6cc42c2e85ec82518b50892a` | 2026-09-03 | Claude | 1 `+`, 0 `-`; 1 same subject on main | no | — | — | T2 | perf(runtime): cache key_cost_table on Runtime; skip per-session recompu |
| 4 unclear | `claude/journal-replay-item-compare-lvhg0s` | `01698192bb7231015e1de766a600591c9a0f38cc` | 2026-10-17 | Claude | 1 `+`, 0 `-`; 1 same subject on main | no | — | — | T2 | fix(user): journal replay compares and sizes items as the pin does (#850 |
| 4 unclear | `claude/store-capability-tiers-xsyjsr` | `f71d5966f22d0e0beaa1dcd9aa7b6f5f313fd6d5` | 2026-08-24 | Shenghao Chen | 2 `+`, 5 `-`; 2 same subject on main | no | — | — | T2 | bench(store): add tkrzw as a third backend_bench column |
| 4 unclear | `claude/trim-interpolation2-2gram-bdsqll` | `48c810dc902e668ae1463c24f719cba2bd19bd3a` | 2026-09-03 | Claude | 1 `+`, 0 `-`; 1 same subject on main | no | — | — | T2 | perf(datagen): emit 1-gram-only interpolation2.text; drop 77.6 MiB dead  |
| 4 unclear | `feat/kyotocabinet-backend` | `ab8e7d793435ac0281fa2897b7fb0082c67359ea` | 2026-08-29 | Shenghao Chen | 3 `+`, 38 `-`; 3 same subject on main | no | #208 closed | — | T2 | feat(capi): SONAME libpinyin.so.15, consumer-union scope, and the loader |
| 4 unclear | `feat/w5-t5-differential-integration` | `ff4e609f192eeee092a954e71eca79406d6fdebd` | 2026-08-15 | Shenghao Chen | 1 `+`, 13 `-`; 1 same subject on main | no | #47 merged | — | T2 | fix(capi): share T4 backends via Arc instead of 'static borrows |
| 4 unclear | `fix/datagen-compat-doc-2026-09-03` | `f7033deb4e51ce487b95133e735ebf2f03d3f5b4` | 2026-09-03 | Shenghao Chen | 5 `+`, 39 `-`; 5 same subject on main | no | #302 closed | #9 PR merged; #3 PR merged | T2 | feat(data): P2 — add chewing/chewing-lookups modes to load_profile |
| 4 unclear | `perf/p3-phrase-table-2026-09-01` | `67786bef8b8915e711352b22b85a7156168db623` | 2026-09-03 | Shenghao Chen | 1 `+`, 21 `-`; 1 same subject on main | no | #273 merged | #9 PR merged; #1 PR merged | T2 | feat(data): P2 — add chewing/chewing-lookups modes to load_profile |
| 4 unclear | `perf/p4-bigram-punct-2026-09-01` | `fd625b9d3bfbf5571be4b8ffd95f82f41f54588f` | 2026-09-03 | Shenghao Chen | 1 `+`, 23 `-`; 1 same subject on main | no | #274 merged | #9 PR merged; #1 PR merged | T2 | feat(data): P2 — add chewing/chewing-lookups modes to load_profile |
| 4 unclear | `perf/p5-datagen-compat-2026-09-01` | `6432267970897a7971e5c286fb10ca12c9021ffd` | 2026-09-03 | Shenghao Chen | 3 `+`, 32 `-`; 3 same subject on main | no | #275 merged | #9 PR merged; #1 PR merged | T2 | feat(data): P2 — add chewing/chewing-lookups modes to load_profile |
| 4 unclear | `perf/runtime-direct-libpinyin-data-2026-09-01` | `d4de84f80e53ffebc38414d67f2586749edd157f` | 2026-09-03 | Shenghao Chen | 4 `+`, 37 `-`; 4 same subject on main | no | #276 merged | #9 PR merged; #1 PR merged | T2 | feat(data): P2 — add chewing/chewing-lookups modes to load_profile |
| 4 unclear | `perf/same-data-differential-tooling-2026-09-02` | `e4070a1fd7b25d87607e3f49218c2f10f0fe0950` | 2026-09-03 | Shenghao Chen | 5 `+`, 37 `-`; 5 same subject on main | no | #296 merged, #284 closed | #9 PR merged; #2 PR merged | T2 | feat(data): P2 — add chewing/chewing-lookups modes to load_profile |
| 4 unclear | `perf/stack-base-p1-p2-2026-09-03` | `963b7dae7513533bea70cd4e088b86603bf9d3a9` | 2026-09-03 | Shenghao Chen | 5 `+`, 37 `-`; 5 same subject on main | no | — | #9 PR merged; #3 PR merged | T2 | feat(data): P2 — add chewing/chewing-lookups modes to load_profile |
| 4 unclear | `wip/1227-pre-squash` | `f369b0a25af06a9ac86a5e1ad916f77b861d841e` | 2026-10-25 | Shenghao Chen | 2 `+`, 0 `-`; 2 same subject on main | no | — | #1227 issue closed | T2 | fix(capi): log final allocation refusals and return failure (#1227) |
| 4 unclear | `wip/1294` | `868326de10c59db48dde3df1ddbb6d964eb70e1c` | 2026-10-27 | Shenghao Chen | 1 `+`, 0 `-`; 1 same subject on main | no | — | #1294 issue closed | T2 | wip: move cloud provisioning behind the managed interpreter |
| 4 unclear | `wip/921` | `f35babfff3f411bb0f8e2cc67dbd343f665df49f` | 2026-10-26 | Shenghao Chen | 1 `+`, 1 `-`; 1 same subject on main | no | — | #921 issue closed | T2 | docs(findings): record why the GLib sorts stay; add tie differential cas |
| 4 unclear | `wip/contract-docs` | `9eca45a12d07ae6ec6fdfd0f86eb7981b0dbd34e` | 2026-10-27 | Shenghao Chen | 1 `+`, 0 `-`; 1 same subject on main | no | — | — | T2 | docs: correct transaction, save, panic and static-link contracts |

## Proposed delete list

The maintainer decides which tiers to delete. Nothing has been deleted.

### T1: fully landed (547)

```text
abi-parity-79-tier-b
abi-parity-79-tier-c
abi-parity-tier-a
abi-parity-tier-b
abi-parity-tier-d
audit/560-replacement
audit/store-key-ordering
bench/lmdb-bulk-load-isolation
bench/redb-is-empty-isolation
bench/store-opt-benches
bindgen-0.73
build/default-backend-tkrzw
build/version-script-relink
chore/bisection-binary-and-paths
chore/drop-python-wrappers
chore/fuzz-comment-rot
chore/lint-allow-to-expect
chore/remove-bdb-claims
chore/remove-oxpinyin-migrate
chore/rename-to-oxpinyin
chore/restore-stale-bisection-runners
ci-store-parallel-matrix
ci/abi-export-gate
ci/coverage-expansion
ci/fuzz-lockfile-locked
ci/fuzz-to-nightly
ci/matrix-widen-sanitizer-gate-coverage-floor
ci/nightly-fuzz-bdb
ci/oracle-provisioning
ci/path-filters-cover-inputs
ci/path-gates
ci/portable-lanes-on-a-c-backend
ci/python-job-debian-container
ci/python-trixie-everywhere
ci/redundancy-caching
ci/release-packages
ci/trailer-job-names
ci/vn-fix-apt
claude/681-alloc-register
claude/681-bigram-iterator-rows
claude/681-string-getter-false
claude/agents-verification-cells
claude/angry-pascal-41cdd0
claude/axis-j-kc-tkrzw-ledgers-2d8494
claude/bold-sagan-kain8m
claude/bold-snyder-b64f23
claude/bopomofo-spec-audit-55f56c
claude/candidate-list-assembly-gzhlh1
claude/candidate-list-assembly-gzhlh1-602
claude/candidate-list-assembly-gzhlh1-row34
claude/candidate-list-assembly-gzhlh1-row37
claude/capi-twins-marshalling-c88d06
claude/cool-hertz-600ca5
claude/dependency-trim-audit-1de972
claude/docs-fullpin-aux-overread
claude/docs-oracle-bisect-assertion-3o5lkq
claude/eager-cray-uwgq0n
claude/eager-cray-uwgq0n-536
claude/eager-cray-uwgq0n-547
claude/eager-cray-uwgq0n-577
claude/engineering-architecture-c20f85
claude/engineering-documentation-4c4d18
claude/exciting-hopper-a6442f
claude/frozen-allocator-registers-ad750e
claude/fuzz-target-build-coverage-653f30
claude/glib-ffi-glib-sys-9kaiii
claude/harness-q1-backend-default
claude/infallible-wozniak-38ea41
claude/inspiring-elbakyan-e0b4d5
claude/issue-619-empty-user-dir-bbccc8
claude/kc-perf-re-baseline-b69da4
claude/keen-khayyam-a92609
claude/kyoto-cabinet-default-nlgdug
claude/lane-c-525-guards-candidates
claude/lane-c-525-guards-config
claude/lane-c-525-guards-cursor
claude/lane-c-525-guards-tone
claude/lane-c-525-guards-train
claude/lane-c-525-guards-zhuyin
claude/lane-c-528-alloc-after-fini
claude/lane-c-532-use-tone
claude/lane-c-540-choose-predicted
claude/lane-c-540-input-cap
claude/lane-c-540-total-wrap
claude/lane-c-540-unigram-total
claude/lane-c-542-aux-text
claude/lane-c-542-key-slots
claude/lane-c-542-out-params
claude/lane-c-542-sentence-unload
claude/lane-c-542-zhuyin-twins
claude/lane-c-545-save-diagnostics
claude/lane-c-587-invalid-utf8
claude/lane-c-587-raw-paths
claude/leading-apostrophe-pinyin-px5hw5
claude/lucid-gates-p7kfq9
claude/measure-fsync-cost-ac6300
claude/modest-zhukovsky-e8c84f
claude/nifty-goodall-ca3kai
claude/nifty-goodall-ca3kai-534
claude/nifty-goodall-ca3kai-541-order
claude/nifty-goodall-ca3kai-541-row36
claude/nifty-goodall-ca3kai-575
claude/nifty-goodall-ca3kai-599
claude/nifty-goodall-ca3kai-600
claude/nifty-goodall-ca3kai-601
claude/offset-decode-context-xdrlsb
claude/optimistic-nobel-adf2ab
claude/oxpinyin-allocator-pairing-gate-64a1f8
claude/oxpinyin-issue-356-liyacd
claude/oxpinyin-issue-357-dbgdxi
claude/oxpinyin-issue-358-0hcjoh
claude/oxpinyin-issue-369-8qd429
claude/oxpinyin-issue-370-v9rb6j
claude/oxpinyin-issue-401-a16b4e
claude/oxpinyin-issue-402-ea9b12
claude/oxpinyin-issue-403-915089
claude/oxpinyin-issue-403-follow-up
claude/oxpinyin-shared-row-schema-410b86
claude/perf-gate-rejected
claude/perf-nightly-lane
claude/perf-size-ci-gate-294dc1
claude/pr-245-readme-updates-bfr0xk
claude/pr-stack-277-279-ygg9e9
claude/pr172-debian-testing-rewrite-xm0rnc
claude/pr2-compatibility-policy
claude/pr3-soname-consumer-union
claude/pr4-berkeleydb-compat
claude/pr5-revert-incompatible-divergences
claude/practical-gagarin-e819a8
claude/prs-252-258-review-gbqq5e
claude/replace-g-free-glib-sys-pa4xbu
claude/review-followups-2026-09-08
claude/roadmap-register-sync-h961wh
claude/rust-libpinyin-trainer-parity-46tiht
claude/rust-libpinyin-trainer-parity-46tiht-1-audit
claude/rust-libpinyin-trainer-parity-46tiht-2-segment
claude/rust-libpinyin-trainer-parity-46tiht-3-kmm
claude/rust-libpinyin-trainer-parity-46tiht-4-punct-word
claude/rust-libpinyin-trainer-parity-46tiht-5-eval
claude/rust-libpinyin-trainer-parity-46tiht-6-train
claude/separate-libpinyin-libzhuyin-7acdbb
claude/session-state-decomposition-c46010
claude/sharp-noether-0yzmwh
claude/tkrzw-134-sentinel-finding
claude/tkrzw-distro-compat-9o3k8h
claude/transformed-reanchor
claude/vigilant-planck-nwa9p7
claude/w5-t0-abi-subset-h5aesd
codex/747-sentence-tie-order
codex/fix-588-lazy-originals
codex/import-token-bound
codex/init-empty-system
codex/init-null-user
codex/issue677-phase2
codex/lane-594-final-step
codex/lane-710-parse-constraints
codex/lane-744-bdb-mutex
codex/lane-a-pinyin-train-index-524
codex/lane-d-probe-repairs
codex/lane-d-sentence-gate-coverage
codex/lane-d-trellis-records
codex/lane-d-trellis-selection
codex/lane-i-key-aware-train
codex/lane-i-nbest-train
codex/lane-j-transformed-fuzzy-matrix
codex/remember-tones
codex/row58-predicted-candidates
codex/tools-01-fixtures
codex/tools-02-cell-targets
codex/tools-03-surface-dev
codex/tools-04-w11-contract
codex/tools-05-prebuilt-parity
codex/tools-06-isolate-gates
codex/tools-07-aggregate
codex/tools-08-gate-docs
codex/training-tooling-consolidation
config/dependabot-groups
cursor/classify-probe-residues-ce63
cursor/decode-path-bench-baseline-4469
cursor/lint-commits-drop-r1-a24a
cursor/register-probe-residues-ce63
data-pipeline-parity
datagen-kc-conformance
deps/phf-0.14
diag/residue-a-root
docs/525-ledger-rows-93-95
docs/agents-bench-targets
docs/agents-rebase-scheme-staleness
docs/agents-stack-placement-rule
docs/agents-stop-clearance-rule
docs/audit-policy-548-550
docs/audit-stale-records
docs/axis-j-bdb-566
docs/bopomofo-spec-freeze
docs/bug-for-bug-audit-r2-2026-09-23
docs/candidate-construction-spec
docs/ci-path-gate-comment
docs/ci-required-three-checks
docs/commit-hygiene-failures
docs/crumbs-post-w11
docs/drop-gsettings-refs
docs/drop-in-user-data-scope
docs/error-handling-audit
docs/fifth-peer-drop-in-naming
docs/findings-no-committed-captures
docs/findings-send-sync-audit
docs/freeze-candidate-construction-contract
docs/freeze-double-pinyin-spec
docs/indexes-and-runbooks
docs/lane-c-container-scratch
docs/model-provenance-fetch-correction
docs/perf-baseline-kc-2026-09-amendment
docs/perf-provenance-audit
docs/perf-recipe-drop-python-row
docs/policy-table-sync
docs/r3-no-self-tag
docs/readme-accuracy
docs/readme-status-crates
docs/register-sort-option
docs/reorg-perf-testing
docs/residual-after-construction-freeze
docs/roadmap-amendment-w7-w8-w9
docs/roadmap-stage1-status
docs/roadmap-w10-w11-w12
docs/roadmap-w14-sentence-surface
docs/self-audit-fixes
docs/send-sync-audit-wording
docs/sentence-surface-refreeze-491
docs/so-size-facade-tip-correction
docs/stale-source-citations
docs/steady-cycle-cross-host-data
docs/steering-sync-2026-09-06
docs/store-durability-and-divergence-fifth-peer
docs/supersede-steady-cross-host
docs/tooling-consolidation-phase1
docs/uncovered-surface-aggregate
docs/upstream-report-drafts
docs/vn-findings
docs/w12-all-off-tails
docs/w14-sentence-residual-freeze
docs/w8-report
engine/pooling-dp-no-clone
engine/session-split
engine/zhuyin-span-start-and-nbest-shape
facade/tests
feat/backend-benchmarks
feat/backend-parity
feat/backend-parity-p1
feat/bench-disk-alloc
feat/berkeleydb-backend
feat/candidate-collection-window-scan
feat/capi-decode-incomplete-option
feat/capi-full-aux-text
feat/capi-interpolation2-unigrams
feat/constraint-machinery
feat/data-layer-rebuild
feat/decoder-lambda-from-config
feat/double-force-tone-batch
feat/drop-snapshot-mvcc
feat/dynamic-adjust
feat/five-differential-surfaces
feat/foundation-t2-provenance
feat/foundation-t4-capture-harness
feat/foundation-t5-core-traits
feat/foundation-t6-syllable-table
feat/foundation-t7-parser
feat/foundation-t8-parser-acceptance
feat/foundation-t9-parser-fuzzing
feat/gfloat-accumulation-parity
feat/guess-candidates-sort-options
feat/guess-offset-zero-start
feat/hanyu-use-tone-digits
feat/kc-dropin
feat/libpinyin-installed-naming
feat/libzhuyin-engine
feat/libzhuyin-facade
feat/live-typing-differential
feat/lmdb-backend
feat/lmdb-system-library
feat/model-build-fetch
feat/ordered-store-rw
feat/oxpinyin-store
feat/parity-climb
feat/parity-climb-2
feat/parity-test-parallel
feat/perf-exploration
feat/pinyin-header-verbatim
feat/preedit-key-accessor
feat/python-api
feat/python-api-docs
feat/python-bindings
feat/runtime-assembly
feat/stage2-measurement-harness
feat/store-capability-tiers
feat/store-read-snapshot
feat/store-tier-tests
feat/test-architecture
feat/tkrzw-backend
feat/tkrzw-bench-column
feat/tkrzw-bindgen-migration
feat/tkrzw-zero-copy
feat/uncovered-surface-differentials
feat/user-codec-fallible
feat/user-key-codec
feat/user-store-generic
feat/user-store-on-trait
feat/user-store-public-ctor
feat/w10-option-bits
feat/w11-addon-choose-promotion
feat/w11-phrase-union
feat/w11-phrase-union-impl
feat/w11-prediction-punct
feat/w12-class-a-comparator
feat/w12-corpus-tail
feat/w13-schemes
feat/w14-nbest-user-delta
feat/w14-sentence-surface
feat/w2-cand-capture
feat/w2-t1-oracle-ffi
feat/w2-t2-parity-corpus
feat/w2-t3-differential-runner
feat/w2-t5-divergence-taxonomy
feat/w3-t0-mini-fixtures
feat/w3-t1-schema-spec
feat/w3-t2-converter
feat/w3-t3-dict-lm-impls
feat/w4-t0-session-api
feat/w4-t0b-fixture-adapters
feat/w4-t0c-config
feat/w4-t1-segment-graph
feat/w4-t2-kbest
feat/w4-t3-scoring
feat/w4-t4-engine-compose
feat/w4-t4b-session-replay
feat/w5-t1-capi-skeleton
feat/w5-t2-lifecycle
feat/w5-t3-keystroke
feat/w5-t4-select-save-config
feat/w6-t0-user-store-characterization
feat/w6-t1-user-store-schema
feat/w6-t2-user-phrase-index
feat/w6-t3-training-wiring
feat/w6-t4-decode-merge
feat/w6-t5-persistence
feat/w6-t6-masking
feat/w6-t7-export-differential
feat/w7-t1-user-import
feat/w8-fork-bootstrap-surface
feat/w8-perf-baseline
feat/w9-t4a-emitter
feat/w9-t4b-corpus
fix/1336-profile-linux-stage-2
fix/540-integer-edge-parity
fix/542-contract-ruling
fix/545-mmap-failed-lines
fix/554-guess-step-rows
fix/554-sparse-successor-walk
fix/569-pred-driver-observability
fix/728-mask-out-reload
fix/736-shared-segment-graph
fix/740-stored-bigram-total
fix/751-surrogate-segmentation
fix/752-remember-character-eligibility
fix/785-phrase-table-mask-carry
fix/790-live-pronunciation-export
fix/791-nbest-blend-cost-above-one
fix/798-wrapping-facade-total
fix/802-preflight-stored-total
fix/804-wrapped-pronunciation-count
fix/805-reload-drops-session
fix/808-pinyin-unigram-user-libs
fix/809-system-zhuyin-only-readings
fix/811-runner-system-dir
fix/all-off-tails
fix/bisect-nbest-guard
fix/capi-database-format-bdb
fix/capi-live-registry-release
fix/constraint-shrink-survival
fix/core-incomplete-phonetic-initial
fix/cursor-offset-normalization
fix/engine-remove-candidate-cap
fix/error-taxonomy-io-misclassification
fix/expand-keys-zero-limit-521
fix/fmt-import-order
fix/fuzz-capi-fixture-path
fix/fuzz-session-keys-timeout
fix/independent-contexts
fix/issue-250-libzhuyin-pc
fix/key-costs-rwlock-fast-path
fix/keycost-doc-and-gate-followups
fix/leading-separator-columns
fix/lmdb-lock-sidecars
fix/mid-offset-window
fix/nbest-step-cost-user-token
fix/nightly-differentials-w3
fix/oracle-driver-trains-a-selection
fix/p3-p8-land-on-main
fix/parse-termination
fix/perf-explicit-kc-backend
fix/pr302-deferred-findings
fix/predicted-prefix-slice
fix/predicted-text-order
fix/public-api-snapshot-const-drift
fix/resplit-after-separator
fix/resplit-matrix-682-683
fix/revert-r2-mid-syllable
fix/revert-r4-validate-constraint
fix/revert-r5-constraint-reset
fix/revert-r6-nbest-cursor
fix/row30-chewing-force-tone
fix/row54-empty-parse-unload
fix/runner-system-dir-guard
fix/rust-analyzer-diagnostics
fix/serialize-locale-tests
fix/shared-fewest-keys-walk
fix/store-unsafe-waiver-narrowing
fix/tkrzw-build-overrides
fix/user-conf-counter-parse
fix/user-conf-fscanf-sequence
fix/user-conf-open-counter
fix/user-file-modes
fix/user-index-keep-empty-keys
fix/user-phrase-tones
fix/user-store-read-path
fix/user-store-registry-drain
fix/user-token-reuse-after-mask
fix/w12-ni-double-apostrophe
fix/w14-nbest-select-path
fix/w14-shared-lm-nbest-costs
fix/zhuyin-unigram-576
full-pinyin-schemes
fuzz/session-keys-timeout-seed
fuzz/vn-fixture-model-import
kiro/specs-update
kiro/steering-refresh
lane-525-batch-a
lane-525-batch-c
lane-525-batch-d
lane-525-batch-e
lane-525-batch-f
lane-525-batch-g
lane-563-axis-b-bdb
lane-694-table-conf
lane-j/force-tone-641
lane-j/session-585-626
measure/backend-matrix-2026-09
measure/backend-matrix-bdb-store-2026-09
measure/perf-backend-matrix-2026-08-31
measure/perf-baseline-kc-validation-2026-08-31
opt/lmdb-backend
opt/redb-alloc-reduction
parity/five-surfaces
perf/757-758-lookup
perf/757-758-trellis-reset
perf/757-759-harness
perf/757-nbest-step-pricing
perf/757-pr0-phrase-index-ranges
perf/757-pr2-raw-user-bigram
perf/757-pr3-merged-lookup
perf/757-trellis-layout
perf/759-export-iterator
perf/alloc-instance-key-cost-cache
perf/bdb-point-buffer
perf/defer-key-cost-table
perf/dict-lm-load
perf/ds-algo-packages
perf/fill-lookup
perf/init-text-slurp
perf/init-typed-map
perf/keystroke-heap-alloc
perf/mmap-system-indexes-2026-08-31
perf/oxpinyin-rss-diagnosis-qvjzud
perf/p2-chewing-table-2026-09-01
perf/remove-ffi-catch-panic-abort
perf/revert-release-panic-abort
perf/so-size-lto
perf/steady-cycle-constant-factor-mkeg4k
perf/steady-cycle-workload-knob
perf/trim-interpolation2-1gram
port/759p1-phrase-table-1015
pr1-525-contract-cases
pr2-525-missing-refusals
pr3-525-native-fixtures
pr4-525-chunk-header-read
python/portable-wheel-lanes
re-pin-live-178
refactor/code-sharing-lsp-quality
refactor/dead-error-variants
refactor/default-backend-berkeleydb
refactor/drop-redb-backend
refactor/drop-redb-lmdb-backends
refactor/facade-pub-crate-ctors
refactor/structure-followups
review/backend-matrix-followup-2026-09
row65-longer-candidate-offset
safety-pr1-mechanics
safety-pr2-fixes
safety-pr3-lanes
safety-pr4-trials
scheme-contract-lock
shenghaoc-commit-trailer-ci-lint
shenghaoc-facade-extract-pinyin
shenghaoc-facade-extract-pinyin-1
shenghaoc-w8-packaging
shenghaoc-w9-t0-training-characterization
shenghaoc-w9-t1-segmenter
shenghaoc-w9-t2-counter
shenghaoc-w9-t3-lambda-estimator
shenghaoc-zhuyin-py-facade
split/s1-storage-seam
split/s2-data-readers
split/s3-session-seam
split/s4-capi-pair
split/s5-training
split/s6-tooling
store/common-helpers
style/engine-user-idiomatic
task9/1-data-codecs
task9/2-store-containers
task9/3-user-persistence
task9/4-session-wiring
task9/5-oracle-differential
test-525-native-empty-system-index
test/oracle-transformed-options-651
test/skips-become-ignores
test/w13-scheme-remeasure
wip/1297
wip/757-fused
wip/757-lazy-candidates
wip/757-log-cache
wip/757-priced-token
wip/757-trellis-layout
wip/785
wip/793
wip/814
wip/818
wip/825
wip/826
wip/830
wip/926
wip/971-measured
wip/999-repro
zhuyin-dachen-cp26
zhuyin-discrete
zhuyin-index-fidelity
zhuyin-simple-keyboards
zhuyin/candidate-window
```

### T2: superseded or disposable (42)

```text
calib/c1-phrase-key-bonus-sweep
ci/verify-nightly-apt
ci/verify-nightly-apt-fix
ci/verify-nightly-checkout-git
ci/verify-nightly-fix
coderabbit/add-comprehensive-pr-tests/3dd68af2
coderabbit/add-pull-request-tests/282701c5
coderabbitai/docstrings/17b5963
coderabbitai/docstrings/48f7a9d
docs/findings/verify-nightly-overrides
fuzz/fixture-model-import
repository-analysis-3214107716791942969
shenghaoc-zhuyin-python-bindings
test/1241-skip-proof
ci/oracle-provisioning-ubuntu-source
claude/dynamic-adjust-bigram-ranking-yxbu5z
claude/pr-179-review-fixes-9jzpny
wip/1071-probe-current
wip/1071-probe-stale
wip/1190
wip/1229-hex-dot
wip/1229-pre-squash
wip/926-pre-squash
wip/929-pre-squash
abi-parity-tier-c
claude/cache-key-cost-table-0qixdj
claude/journal-replay-item-compare-lvhg0s
claude/store-capability-tiers-xsyjsr
claude/trim-interpolation2-2gram-bdsqll
feat/kyotocabinet-backend
feat/w5-t5-differential-integration
fix/datagen-compat-doc-2026-09-03
perf/p3-phrase-table-2026-09-01
perf/p4-bigram-punct-2026-09-01
perf/p5-datagen-compat-2026-09-01
perf/runtime-direct-libpinyin-data-2026-09-01
perf/same-data-differential-tooling-2026-09-02
perf/stack-base-p1-p2-2026-09-03
wip/1227-pre-squash
wip/1294
wip/921
wip/contract-docs
```
