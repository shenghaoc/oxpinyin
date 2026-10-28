# Stable raw-cost tail ordering — S3

Date: 2026-10-28 UTC (`date -u`). Base: `b02f85b1ff251fdb67532dd39637cda601a427f8`.
Previous comparator repair: #1352, S1/S2. This is PR 2 of 4.
Policy: [witnessed algorithm-precondition violations, S3](compatibility-policy.md#amendment--witnessed-algorithm-precondition-violations-2026-10-28-utc),
including its explicit special-cost completion. This is the registered
product-level ruling, not a language-artefact exception.

## Source and scope

The libpinyin Git objects read were at
`074a2219c90feaf962d0d24f034514033ece5f99`; the three cited source blobs
were hashed against that commit. `src/lookup/phonetic_lookup.h:174-178`
says “in descending order” and compares only `m_poss`, returning the
negated `gfloat` difference as `gint`. Its shared template's `get_tails`
selects `nbest` entries, then sorts them (`:330-339`). Pinyin instantiates
`PhoneticLookup<2,3>` (`src/pinyin.cpp:55,406`); zhuyin instantiates
`PhoneticLookup<1,1>` (`src/zhuyin.cpp:50,348`).

Oxpinyin's `Trellis::tails` retains its existing selection and calls
`tail_merge`, which now uses Rust's stable `slice::sort_by` with
`ordered_cost_compare(left.cost, right.cost)`. This orders raw possibility
descending without integer truncation: `NEG_INF_COST`, finite costs
ascending, `POS_INF_COST`, `NAN_COST`. Equal costs retain the selected
array's input order. Length and batch position do not break a tail tie;
adjusted S remains confined to S1/S2. Zhuyin shares this final sort but
selects at most one tail, so its output cannot move through S3 alone.

Selection, node insertion and front eviction, backtracking, score
arithmetic, external calls, persistent data and the C ABI are unchanged.
The tail sort's worst-case bounds remain O(n log n) time and O(n) scratch;
here n is at most three for pinyin and one for zhuyin. No runtime saving
is claimed.

Before removing `poss`, `tail_compare` and `c_float_to_gint`, a tracked
workspace-wide search found code references only in `nbest.rs`'s tail
implementation and its obsolete NaN-conversion test. Neither
`session/mod.rs::glib_rank_sort` / `RankKey::pin_compare` nor
`capi/src/predict.rs::sort_predicted` uses them: those paths keep their
wrapping frequency comparisons and `glib_merge_sort`, as row 83 requires.
The merge implementation and export remain unchanged.

## Tests

The former tail-conversion/NaN test and near-equal truncated-tail test
are replaced by tests of the production `tail_merge`: sub-one-log-unit
score differences order by raw cost, equal costs remain stable despite
length/identity, special costs order explicitly with NaN last, and all
permutations of the qian witness order A, B, C. A raw-cost versus adjusted-S
counterexample prevents accidentally reusing the selection criterion.

The qian witness has m_poss A=`-0x1.c9dc3cp+2`, B=`-0x1.dda91p+2`,
C=`-0x1.0a68dp+3`, from the [registered capture](algorithm-precondition-witnesses-2026-10-28.md).
Its native-scale test costs are `round(-m_poss * 1000 / ln(2))`:
10321, 10767 and 12011. The capture licenses S3, while the shuffled arrays
also discriminate the old comparator. The captured original order already
is A, B, C; its existence does not predict an outward movement.

The GLib merge reference test now exercises the retained wrapping
frequency comparator, including cycles and ties. Its equal-key and
wrapping-cycle expectations remain unchanged. Existing S1/S2 and zhuyin
selection tests retain their expectations. No frozen fixture or golden
is edited.

## Focused BDB results

Both base and repair report **4 MATCH and 4 POLICY-ACCEPTED**, with all
complete observations identical between base and repair: exit, result,
logs and normalized stderr. No case in this selection moves under S3,
so none receives a new policy exemption.

| Cases | Base and repair |
| --- | --- |
| `glib-sort-tails-pinyin`, `glib-sort-tails-zhuyin` | MATCH |
| `trellis-tail-sort/qian-on`, `trellis-tail-sort/qian-off` | MATCH; both return three sentences: 前, 前, 千 |
| `nbest-user-table-order-pinyin` | POLICY-ACCEPTED against its existing S2 record; no S3 movement |
| `table-conf-lambda-one-and-a-half`, `table-conf-lambda-two`, `table-conf-lambda-million` | POLICY-ACCEPTED against their existing S1/S2 record; no S3 movement |

The four pre-existing cases were re-run on both revisions after #1352's
rebase onto the host-libc lambda reader. Their complete pin and subject
observations still equal [the original S1/S2 record](trellis-adjusted-cost-s1-s2.md),
which is preserved unchanged. The qian table-on/off registrations reuse
`contract-diff.py::glib_sort_tails` and the existing complete sentence-rank
observer; no driver was added. On this base these registrations, tail-sort
cases and lambda cases remain in `contract-diff.py`; the user-table case's
declaration is in `contract_combined_import_nbest.py`.

Any future observed S3 movement needs an exact BDB pin/subject pair in a
registered current `contract-diff-policy` record and `policy_record` on its
case. A lambda case moved again by S3 must cite both S1/S2 and S3 and identify
the added difference. These passing controls grant no wildcard exemption.
This selection is not a corpus campaign.

Engine checks pass: 198 unit tests, 39 integration tests and four doctests;
formatting and engine Clippy pass. The focused contract-driver and policy-verdict
harnesses pass 163 and 35 tests. The six-permutation qian regression passes.

## Reproduction

Native Ubuntu x86_64, repository Rust toolchain, managed Python and the
verified BDB pin/data from `tools/cloud/setup.sh`. Source that setup's
environment file and use disk-backed scratch. The original runs passed
`--observations` to the existing runner, writing separate
`base-observations.jsonl` and `pr-observations.jsonl` files. The claim that observations are unchanged comes from comparing those captures,
not the verdict output.

Set `BASE_WORKTREE` and `PR_WORKTREE` to the two checkouts, `BASE_TARGET` and
`PR_TARGET` to their separate target directories, and `OBS_DIR` to a writable
local directory; all five paths are absolute. These commands preserve the original runner options, capture filenames and
case selection, with variables for checkout and artifact locations. Run once
per revision:

```sh
CASES=glib-sort-tails-pinyin,glib-sort-tails-zhuyin,trellis-tail-sort/qian-on,trellis-tail-sort/qian-off,nbest-user-table-order-pinyin,table-conf-lambda-one-and-a-half,table-conf-lambda-two,table-conf-lambda-million
mkdir -p "$OBS_DIR"
(
  cd "$BASE_WORKTREE"
  export CARGO_TARGET_DIR="$BASE_TARGET"
  just heavy -- just cell-artifacts --artifacts pinyin-capi,zhuyin-capi
  just heavy -- just runner contract-diff bdb "$PINYIN_ORACLE_PREFIX" \
    "$CARGO_TARGET_DIR/debug/libpinyin_capi.so" \
    "$CARGO_TARGET_DIR/debug/libzhuyin_capi.so" -- \
    --cases "$CASES" --observations "$OBS_DIR/base-observations.jsonl"
)
(
  cd "$PR_WORKTREE"
  export CARGO_TARGET_DIR="$PR_TARGET"
  just heavy -- just cell-artifacts --artifacts pinyin-capi,zhuyin-capi
  just heavy -- just runner contract-diff bdb "$PINYIN_ORACLE_PREFIX" \
    "$CARGO_TARGET_DIR/debug/libpinyin_capi.so" \
    "$CARGO_TARGET_DIR/debug/libzhuyin_capi.so" -- \
    --cases "$CASES" --observations "$OBS_DIR/pr-observations.jsonl"
)
```

The comparison used each side's `exit`, complete `result` (including logs),
and normalized `stderr`, the stable policy observation for these eight cases
(`userdir=False`). `contract_runner.py::Runner.run_worker` already
replaces the generated scratch root with `SCRATCH` and generated user/sys
suffixes with `X` before capture. No further normalization or result-field
filtering was applied; capture bookkeeping is outside this comparison.
The original inline comparison ran with `python3`. The command below runs
that same field-comparison logic with the repository's managed interpreter
and parameterizes the capture directory. It prints the unchanged-observations
line for each of the eight cases and exits successfully:

```sh
uv run --no-project --managed-python python -X lazy_imports=normal - "$OBS_DIR" <<'PYTHON'
import json
import sys
from pathlib import Path
r = Path(sys.argv[1])
a = {x['case']: x for x in map(json.loads, (r / 'base-observations.jsonl').read_text().splitlines())}
b = {x['case']: x for x in map(json.loads, (r / 'pr-observations.jsonl').read_text().splitlines())}
for name in a:
    for side in ('pin', 'subject'):
        for field in ('exit', 'result', 'stderr'):
            assert a[name][side][field] == b[name][side][field], (name, side, field)
    print(name, 'complete pin/subject observations unchanged')
PYTHON
```

From the repair worktree, the completion checks are:

```sh
just fmt --all --check
just clippy --locked -p oxpinyin-engine --all-targets -- -D warnings
just heavy -- cargo test --locked -p oxpinyin-engine
just test-tools tools/bisection/contract-diff.test.py tools/bisection/contract_verdict.test.py
```

For base production code, apply only the two qian registrations and the
existing tail cases' stderr comparison, without changing its Rust code or
policy markers. The four old cases are already present on base. The record
checks compare complete observations, not only sentence text.


## Shipping libraries and policies

Stripped BDB regular shipping files, counted once each under #864, built on
the same Ubuntu x86_64 host with repository Rust and equivalent source-path
remapping. Both release-stage gates pass; `file` identifies all four objects
as stripped.

| Library | Base `b02f85b1` bytes | S3 repair bytes | Increase |
| --- | ---: | ---: | ---: |
| `libpinyin.so.15.0.0` | 2,462,448 | 2,470,640 | 8,192 |
| `libzhuyin.so.15.0.0` | 2,208,496 | 2,220,784 | 12,288 |

```sh
RUSTFLAGS="--remap-path-prefix=$PWD=/oxpinyin-s3" \
  just heavy -- just packaging release-stage bdb \
  --prefix=/usr --libdir=/usr/lib --dest="$STAGE"
file "$STAGE/usr/lib/libpinyin.so.15.0.0" "$STAGE/usr/lib/libzhuyin.so.15.0.0"
wc -c "$STAGE/usr/lib/libpinyin.so.15.0.0" "$STAGE/usr/lib/libzhuyin.so.15.0.0"
```

All six checked-in policies resolve against the PR's shipping staticlibs;
none needs regeneration. Build inputs use `just packaging install`, with
`--locked --release --no-default-features --features <backend>,shipped`
for pinyin and `<backend>` for zhuyin (`kc` maps to `kyotocabinet`).

| Backend | libpinyin resolved/total | libzhuyin resolved/total |
| --- | ---: | ---: |
| bdb | 266/266 | 244/244 |
| kc | 343/343 | 327/327 |
| tkrzw | 270/270 | 257/257 |

No C++ policy path is skipped (KC resolves all 53 C++ paths per facade).
The tkrzw health probe passes, and both pin and both subject facades resolve
the same setup-verified sound libtkrzw. Behavioral differentials remain BDB-only;
the three-backend shipping checks follow verification rule 2's policy requirement.
Reproduction uses [the shipping-build and policy procedures](../packaging.md#regeneration)
and, for each facade/backend archive:

```sh
just packaging function-order resolve \
  "$CARGO_TARGET_DIR/$(rustc -vV | sed -n 's/^host: //p')/release/$FACADE.a" \
  "tools/packaging/function-order/$CELL/$FACADE.order" "$FACADE.so.15"
```
