# Pin-built oracle on native arm64 Linux (#1132)

Recorded 2026-10-24 UTC. No capture or log file is committed; the commands
below reproduce every figure.

The gate ran at `d7788072`. The branch was later rebased onto `c86ec0dd`,
whose difference is backend documentation links and the new cell-selection
tooling, not subject or contract behavior. Rule 7 applies: a conflict-free
rebase that brings no file this lane touches needs no local re-gate; CI
covers the rebase. The arm64 oracle recipe, setup and the 13 #1137
exclusions are part of this change.

## Host

Apple container 1.5.0 on macOS 27.0.1. The verification container is
Debian 13.7 (trixie) arm64, allocated 4 CPUs (`cpu.max=400000 100000`) and
16 GiB (`memory.max=17179869184`), with `/work` on ext4 (`/dev/vdb`). No
emulation: every image, library and subject binary is arm64.

The oracle recipe itself runs in the pinned
`debian:testing@sha256:dab11cdb0a9dcf4bbd68f671635b35f1f726b452b92396875b69bb2c7daa42a9`
arm64 image from `tools/oracle/Dockerfile.oracle` (Debian forky/sid), because
that base image, its dated apt snapshot and its compiler are part of the pin.
The subject and the gates run on the Debian 13 host.

## Recipe and reproducibility

The pin is unchanged: libpinyin 2.11.92 at
`074a2219c90feaf962d0d24f034514033ece5f99`, model20
`59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155`,
and the bdb, kc and tkrzw backends from the pinned `debian:testing` image
and apt snapshot `20260831T000000Z`.

Each cell was built twice with `build-oracle.sh --libs-only
--enable-libzhuyin` from different work directories and prefixes:

```sh
tools/oracle/build-oracle.sh --libs-only --enable-libzhuyin --dbm bdb \
  --work-dir /build/a/bdb --prefix /opt/oracle-a/bdb --jobs 4
tools/oracle/build-oracle.sh --libs-only --enable-libzhuyin --dbm bdb \
  --work-dir /build/elsewhere/b/bdb --prefix /opt/oracle-b/bdb --jobs 4
```

The kc and tkrzw pairs used `/build/kc-a` and `/build/elsewhere/kc-b`, and
`/build/tkrzw-a` and `/build/elsewhere/tkrzw-b`, with two make jobs each.
The two builds of every cell produced identical hashes:

| cell | libpinyin | libzhuyin |
| --- | --- | --- |
| bdb | `473187d8d040b8a5763a188509d6be031e224a712ad9b7716defdb4ff1293a2f` | `e90429560584cf9dfee345d41e608092032d3a1c7248e04feeedc899dcfd24b5` |
| kc | `d2672b94ec8fe15c86062ca9e400a0df8b52b9b1f8be196a71e27e545de50e2d` | `644cc0edb5080925bb574445deac5425c7fca729bcd9032d8b4858ffe22e6df7` |
| tkrzw | `062caa79982cb2024666666e5c0da1914808b9d2eba55c9ae95acc6827d8d32a` | `43b884cbaa74b170a3a4b4cd254fff7d2939b9442d07c6190f51a643610c67c3` |

The same hashes came out of the Dockerfile image built natively by Apple's
builder (`container build --platform linux/arm64`). The arm64 compatibility
image also built natively from those deps and reference images; its managed
Python and Rust 1.97.1 checks passed. The arm64 rows are in
`tools/oracle/oracle-pin.txt`; the amd64 field lines are unchanged.

## Provisioning

The Apple container has no nested container runtime, so setup used its
existing in-container adapter with the pinned image's contents at
`/opt/reference`:

```sh
export OXPINYIN_COMPAT_CONTAINER=1
tools/cloud/setup.sh --in-container --cells "bdb kc tkrzw"
```

All three cells passed the frozen arm64 library hashes and their data
manifests. The host's packaged libtkrzw is the known broken build, so setup
used the cached upstream 9ee8416 build. The probe then reported:

```
RESULT : healthy
```

and `ldd` resolved the oracle's `libtkrzw.so.1` to
`/work/cache/tkrzw/lib/libtkrzw.so.1`. The setup stdout ended with one line
per cell naming the arm64 library hashes and `data manifest OK`.

## Gates

```sh
source /work/oracle.env
tools/bisection/run-all.py \
  --runners contract-diff,same-data-dir-diff \
  --cells bdb,kc,tkrzw --jobs 1 \
  --oracle-root /work/cache/oracle --target-root /work/targets-final \
  --log-dir /work/gates-final/logs --json /work/gates-final/results.json
```

| cell | contracts (2,899 cases) | same-data |
| --- | --- | --- |
| bdb | 2,880 MATCH, 15 SKIP, 4 HOST-DEPENDENT, 0 DIFFER | IDENTICAL on 15 drivers |
| kc | 2,875 MATCH, 20 SKIP, 4 HOST-DEPENDENT, 0 DIFFER | IDENTICAL on 15 drivers |
| tkrzw | 2,877 MATCH, 18 SKIP, 4 HOST-DEPENDENT, 0 DIFFER | IDENTICAL on 15 drivers |

Thirteen of the bdb SKIPs, thirteen of the kc SKIPs and thirteen of the
tkrzw SKIPs are the arm64-only #1137 exclusions; the other SKIPs are the
registry's cell-scoped cases. Each excluded verdict carries the reason:

```
#1137: the pin crashes or orders candidates differently on arm64; excluded
there only, amd64 still runs the case
```

On arm64 `fini-twice`, `zhuyin-fini-twice`, the six
`replay-modify-header-overlong-*` / `replay-touched-*` cases, the two
`row58-unsigned-score-*` cases and the three
`table-conf-lambda-*-zhuyin` cases are registered `platforms=('linux/amd64',)`.
The amd64 run is unchanged.

## amd64 comparison

No full amd64 contract run is recorded at `d7788072`. The latest complete
three-cell lane report is PR #1130 at `838dabc2` (2,899 cases): BDB 2,893
MATCH / 2 SKIP / 4 HOST-DEPENDENT / 0 DIFFER; KC 2,888 / 7 / 4 / 0; tkrzw
2,890 / 5 / 4 / 0. After the 13 arm64-only exclusions, the arm64 gate has
0 DIFFER on every cell.

## Cross-architecture observations

A native amd64 (x86_64) Claude Code cloud session ran the same 13 cases at
`c9067c80` with the existing driver and recorded both complete observations.
Its hash table is issue comment
[6380711257](https://github.com/shenghaoc/oxpinyin/issues/1132#issuecomment-6380711257);
the raw observations were not attached and are not committed.

The arm64 side used the driver's `--observations` for each cell. The arm64
subject observations were hashed as
`sha256(json.dumps(observation, sort_keys=True, separators=(',', ':'),
ensure_ascii=True))[:12]`, the same canonical form as the amd64 table. The
maintained way to reproduce that capture uses `--run-platform-excluded`,
which runs the cases the host registry would skip and prints each case's
reason on stderr; it replaces the session-only wrapper that overrode
`HOST_ORACLE_PLATFORM`:

```sh
tools/bisection/run-contract-diff.sh "$cell" /work/cache/oracle/$cell \
  /work/targets-final/$cell/debug/libpinyin_capi.so \
  /work/targets-final/$cell/debug/libzhuyin_capi.so \
  -- --run-platform-excluded \
  --cases fini-twice,... --observations /work/arm64-13-$cell.jsonl
```

For the eight crash cases the reference is the amd64 **subject** observation
(the amd64 pin crashes with SIGSEGV but the harness records MATCH, as the
case expects). For the five cases whose amd64 pin exits normally, the
reference is the amd64 **pin** observation. All 39 per-cell comparisons are
identical:

| case | group | amd64 pin exit | arm64 subject = amd64 reference, sha256[:12] bdb/kc/tkrzw |
| --- | --- | ---: | --- |
| `fini-twice` | crash | -11 | `796af921bf0d` / `f3287a92ad43` / `cd0d059df947` |
| `zhuyin-fini-twice` | crash | -11 | `d877e15b727c` / `d877e15b727c` / `d877e15b727c` |
| `replay-modify-header-overlong-pinyin` | crash | -11 | `5a1c30b47e65` / `cc9088a4f03f` / `c17336526b9e` |
| `replay-modify-header-overlong-zhuyin` | crash | -11 | `396b1e1530d9` / `37a882e7f15f` / `4aba0b24e509` |
| `replay-touched-duplicate-invalid-pinyin` | crash | -11 | `5a1c30b47e65` / `cc9088a4f03f` / `c17336526b9e` |
| `replay-touched-duplicate-invalid-zhuyin` | crash | -11 | `396b1e1530d9` / `37a882e7f15f` / `4aba0b24e509` |
| `replay-touched-many-invalid-pinyin` | crash | -11 | `5a1c30b47e65` / `cc9088a4f03f` / `c17336526b9e` |
| `replay-touched-many-invalid-zhuyin` | crash | -11 | `396b1e1530d9` / `37a882e7f15f` / `4aba0b24e509` |
| `row58-unsigned-score-off` | normal | 0 | `6f99d0e60bdb` / `6500edb76419` / `44c889deeb00` |
| `row58-unsigned-score-on` | normal | 0 | `6f99d0e60bdb` / `6500edb76419` / `44c889deeb00` |
| `table-conf-lambda-million-zhuyin` | normal | 0 | `ce9baa647ac0` / `095bd2a8c48b` / `5bca8a93d778` |
| `table-conf-lambda-one-and-a-half-zhuyin` | normal | 0 | `7fd8ff5fcf7f` / `36d33f2179d6` / `2e08f5071daa` |
| `table-conf-lambda-two-zhuyin` | normal | 0 | `49e68e3f23f7` / `95c8b09f94ca` / `195653d75475` |

`fini-twice`'s pin side contains uninitialised bytes and is not compared;
its subject side is, per the crash-case rule above.
