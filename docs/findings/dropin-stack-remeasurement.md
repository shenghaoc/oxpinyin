# The drop-in stack, measured against a live oracle

Date: 2026-08-28 · Status: **measured; no pin moved, no differential
regressed** · Branch: `measure/dropin-stack-oracle` (a measurement
branch off the stack tip, not a commit to any stack branch).

Five sessions of drop-in work shipped with the same caveat: *the frozen
pins could not be re-measured, because the oracle could not be
provisioned.* `ci/oracle-provisioning-ubuntu-source` fixed the fetch.
This is the measurement those caveats asked for.

## What was measured, and against what

| | |
|---|---|
| Stack tip | `claude/pr5-revert-incompatible-divergences` (`70edf8f`) |
| Harness | plus `fix/runner-system-dir-guard` (PR #199) cherry-picked |
| Oracle `pin_ref` | `libpinyin-2.11.91-0c5e80e1200f84fab185d1c5bde458b770a0636c+model20-59c68e89…+dbm-tkrzw` |
| libpinyin archive | `ff3047b1…788c` — the dist tarball, from `archive.ubuntu.com` |
| ibus archive | `cc652d48…ce31` — same route |
| libtkrzw | 1.0.32 built from source (Ubuntu's package is the broken build) |
| System data | `oxpinyin-datagen` from pinned model20 (`59c68e89…`) |

The stack tip carries the real DYNAMIC_ADJUST
(`dynamic_adjust_bigram_possibility`), not main's
`dynamic_adjust_bigram_term` stub — verified in the built tree before
measuring, because measuring the stub would have proved nothing.

## Frozen pins: every one holds

| Pin | Established | main (`a696e08` baseline) | **Stack tip** | Δ |
|---|---|---|---|---|
| top-1 | 10,190 / 10,190 | 10,190 / 10,190 | **10,190 / 10,190** | none |
| top-5-set | 10,190 | 10,190 | **10,190** | none |
| absent | 0 | 0 | **0** | none |
| order-only | 0 | 0 | **0** | none |
| prefix-10 | 98,930 / 98,930 | 98,930 / 98,930 | **98,930 / 98,930** | none |
| sentence 1-best / distinct-set / ordered | 488 / 385 / 379 | 488 / 385 / 379 | **488 / 385 / 379** | none |

`sentence_surface_matches_the_declared_residual` ran and passed. Both
fixture-freshness tests pass against the live oracle (10,312 distinct
inputs, 97,442 live triples), so the fixtures the pins are scored
against are themselves current.

**DYNAMIC_ADJUST's safety argument is now a measurement.** It shipped
argued rather than measured: the bit is clear in every frozen option
word, and with `bigram_poss == 0.0` the amplified arithmetic is
bit-identical to the unigram-only law by construction. Both halves now
hold against a live pin over all 10,190 corpus inputs.

## Differentials: twelve clean, two documented

| Differential | main | **Stack tip** | Δ |
|---|---|---|---|
| live-typing (the §3 gate) | IDENTICAL | **IDENTICAL** | none |
| import | IDENTICAL | **IDENTICAL** | none |
| import — classic frontend interop | *skipped* | **IDENTICAL (both directions)** | now runs |
| train | IDENTICAL | **IDENTICAL** | none |
| nbest-train | IDENTICAL | **IDENTICAL** | none |
| predict | IDENTICAL | **IDENTICAL** | none |
| punct | IDENTICAL | **IDENTICAL** | none |
| scheme | IDENTICAL | **IDENTICAL** | none |
| addon-candidate | IDENTICAL | **IDENTICAL** | none |
| user-candidate | IDENTICAL | **IDENTICAL** | none |
| union | IDENTICAL | **IDENTICAL** | none |
| option-sweep | PASS | **PASS** | none |
| uncovered-surface | exit 2, all PRED_PREFIX | **exit 2, 152 lines, all PRED_PREFIX, 0 others** | none |
| pred-order | exit 2, 1557/1571 | **exit 2, 1557/1571** | none |

**live-typing deserves its own line** (Step 3d): it drives parse → guess
sentence → choose → guess at the advanced offset, which is exactly the
path DYNAMIC_ADJUST's Gate 1 now touches, and it is IDENTICAL with the
implementation in place.

The two exit-2 rows are the documented classes, unchanged from main:

- **uncovered-surface** — `datagen-model20.md:204` records the expected
  result as "exit 2 with zero non-PRED_PREFIX diverging lines". Measured
  on the stack tip: 152 diverging lines, **152 PRED_PREFIX, zero
  others** — byte-for-byte the shape main produces.
- **pred-order** — a 2026-08-25 maintainer decision defines this as "a
  defined order, not fixture-frozen parity".

## The new pred-order constant

Recorded as asked (Step 3e). It measures **identically on main and on
the stack tip**, so it is not stack-dependent:

```
hao 177/178   de 280/283   yi 589/591   ni 69/71
zhongguo 124/126   wo 168/168   shi 97/98   le 53/56
DIVERGENCE: 1557/1571 rows at different positions
```

The previously documented figures were **1541/1571** (and `hao`
174/178). The delta is not the stack: it is the oracle's Tkrzw build.
The class is documented as "a compile-time artifact of its DBM choice" —
a bucket walk — and this oracle links a from-source **1.0.32**, where
the recorded figures came from a different build. **1557/1571 is the
constant for an oracle built by `build-oracle.sh` as it now stands**,
and it should be re-recorded whenever the oracle's tkrzw changes.

## Step 4: the delta table, and why each is what was predicted

| Stack change | Predicted impact | Measured |
|---|---|---|
| DYNAMIC_ADJUST | ordering may change at non-zero offsets *with the bit set*; unchanged under frozen profiles, where the bit is always clear | **unchanged** — every pin and differential identical to main |
| Preedit key family | outside the decode path, no pin impact | **unchanged** |
| KC backend | off-by-default feature, no decode path | **unchanged** |

Zero unexpected deltas. Nothing to re-freeze.

## The harness fix this depended on

The previous sweep read `scheme` and `option-sweep` as DIVERGENT because
their system-dir variables were unset and the runners silently used the
mini fixture. PR #199 makes that fatal. This sweep set **only**
`OXPINYIN_SYSTEM_DIR` — no per-runner name — and scheme and option-sweep
came back IDENTICAL/PASS on the first attempt, which is the
standardisation working end to end.

One genuine bug it fixed: `run-w11-diff.sh` reads
`CAPI_W11_SYSTEM_DIR`, was undocumented, and the guessable
`CAPI_W11_SYSTEM` did nothing at all.

## Reproducing this

```sh
# 1. the oracle (archive.ubuntu.com primary)
PKG_CONFIG_PATH=/usr/local/lib/pkgconfig \
  tools/oracle/build-oracle.sh --prefix /tmp/oracle-prefix --jobs "$(nproc)"

# 2. the system data
tools/model/fetch-model.sh
export PINYIN_MODEL_DIR="$PWD/target/model20/extracted"
cargo run --release -p oxpinyin-datagen -- compile --out-dir target/datagen/redb
cp "$PINYIN_MODEL_DIR/interpolation2.text" target/datagen/redb/

# 3. one variable for the whole sweep (PR #199)
export PINYIN_ORACLE_PREFIX=/tmp/oracle-prefix
export PINYIN_EXPORT_DIR="$PWD/target/datagen/redb"
export OXPINYIN_SYSTEM_DIR="$PWD/target/datagen/redb"
export PINYIN_IBUS_BUILD_DIR=/tmp/oracle-work   # the work ROOT, not src/

cargo run --release -p pinyin-oracle --bin corpus-tail
cargo test --release -p pinyin-oracle --features oracle-ffi \
  --test sentence_surface_parity --test real_tables_integration -- --include-ignored
for r in tools/bisection/run-*-diff.sh tools/bisection/run-option-sweep.sh; do bash "$r"; done
```

`PINYIN_IBUS_BUILD_DIR` wants the oracle **work root** (holding
`src/ibus-libpinyin-1.16.5/src/*.o` and `prefix/oracle-pin.txt`), not the
ibus source directory — pointing it at the source dir is why the classic
frontend interop check skipped in the previous sweep.
