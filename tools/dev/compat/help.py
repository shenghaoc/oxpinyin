"""Preserved help streams for migrated oracle command grammars."""

TEXT = {
    'verify-image': r'''. "${0%/*}/../lib/bash-floor.sh"

set -euo pipefail

# Check a built oracle image against tools/oracle/oracle-pin.txt (issue #818).
#
# For each cell (bdb, kc, tkrzw) the image's libpinyin.so and libzhuyin.so
# must hash to the recorded recipe values, and the image must carry NO
# model-derived data (docs/findings/model-provenance.md): no
# lib/libpinyin/data directory and no *.bin, *.table, *.text or bigram.db
# file anywhere under /opt/pinyin-oracle.
''',
    'reproducible': r'''. "${0%/*}/../lib/bash-floor.sh"

set -euo pipefail

# Reproducibility gate for the pin-built oracle libraries (issue #818).
#
# For each DBM backend (bdb, kc, tkrzw) this builds libpinyin.so and
# libzhuyin.so twice, in the pinned build image, from clean, DIFFERENT work
# directories and DIFFERENT install prefixes, and fails unless both builds
# hash identically, and equally to the values tools/oracle/oracle-pin.txt
# records. Prints one line per backend and library:
#
#   <dbm> <library> <sha256>
#
# Usage: check-reproducible.sh [--engine docker|podman|container] [--image IMAGE] [--jobs N]
#                              [--platform linux/amd64|linux/arm64]
#
# Without --image the `deps` stage of tools/oracle/Dockerfile.oracle is built
# first (the Debian trixie pin-reference environment). The builds fetch the
''',
    'differentials': r'''!/usr/bin/env bash
Not `set -e`: a gated differential that diverges (e.g. ngseg when the system
bigram is unavailable) should be reported, not abort the whole run. Every
suite's exit status is accumulated instead, and the script exits non-zero
when any suite failed, so a later passing suite never masks an earlier
failure.
shellcheck source=../lib/bash-floor.sh
run-differentials.sh — run the oxpinyin↔libpinyin oracle differentials
against a built pinned oracle (libpinyin 2.11.92, Tkrzw backend, verified
model20 data). Wires the PINYIN_* env vars the env-gated differential tests
read, then runs them with the tkrzw backend EXPLICITLY — the flavour the
pin-built oracle itself is (the workspace default moved to Berkeley DB on
2026-09-20, so this is now a pin, not a ride). Every side of these
differentials is tkrzw: the oracle, the oxpinyin export below, and the
backend-forwarding crates.

Prerequisites (see docs/testing/oracle-environment.md and the build recipe
below):
  1. A built libpinyin 2.11.92 tree (Tkrzw backend). Its utils and shared
     object are used directly from the build tree — no `make install`.
  2. A built system data dir: the model20 tables + table.conf plus the
     compiled phrase_index.bin / pinyin_index.bin, built by the three
     steps of libpinyin's data/Makefile.am — gen_binary_files,
     import_interpolation, then gen_unigram (the freq-1 floor; without it
     every zero-count phrase is unreachable and the pin's ngseg diverges
     from the committed golden). The KMM/segment utils load these from
     the cwd.
     The two bigram-backed gates (ngseg-live, eval_correction_rate) also
     need the compiled bigram.db there, and the evaluator gate needs an
     evals2.text (a segmented, null_token-separated corpus in the system
     token space — e.g. the pin's own `ngseg` over raw text) in that dir;
     without them the evaluator gate is reported as skipped, not failed.
  3. The model export (tkrzw), produced by libpinyin's own sequence with
     oxpinyin-utils' pinned binaries — the pin's configure writes table.conf
     from data/table.conf.in, so the export starts from the pin's own
     data/table.conf (an installed one, or fixtures/w3/tkt/table.conf):
       cd <export dir> && cp <pin data>/table.conf .
       cargo run -p oxpinyin-utils --no-default-features --features tkrzw \
           --bin gen_binary_files -- --gen-punct-table --table-dir <model20 dir>
       cargo run -p oxpinyin-utils --no-default-features --features tkrzw \
           --bin import_interpolation -- --table-dir <model20 dir> < <model20 dir>/interpolation2.text
       cargo run -p oxpinyin-utils --no-default-features --features tkrzw \
           --bin gen_unigram -- --table-dir <model20 dir>
  4. The extracted model20 directory (--model), also used by the
     cross_backend read-back test. The runtime data-directory test reads
     --data unchanged; both new suites use the same tkrzw cell.

Usage:
  just heavy -- tools/oracle/run-differentials.sh --libpinyin DIR --data DIR --export DIR --model DIR

Every required path is validated before a single gate runs. Explicitly
selected ignored tests fail on missing inputs (docs/testing/README.md).
The pre-release checks for BDB and KC live in docs/runbooks/oracle.md.

The build recipe used to produce (1)+(2) from the pinned source (when the
release tarball is reachable, tools/oracle/build-oracle.sh is canonical; in a
network-restricted host build from a local checkout of the pinned commit):
  cd libpinyin && ./autogen.sh && \
    ./configure --with-dbm=Tkrzw --prefix=$PWD/../prefix && make -j
  cd data && cp <model20>/*.table <model20>/interpolation2.text . && \
    LD_LIBRARY_PATH=../src/.libs ../utils/storage/gen_binary_files \
        --gen-punct-table --table-dir . && \
    LD_LIBRARY_PATH=../src/.libs ../utils/storage/import_interpolation \
        --table-dir . < interpolation2.text && \
    LD_LIBRARY_PATH=../src/.libs ../utils/training/gen_unigram --table-dir .
`import_interpolation` has been observed to trip its `insert_freq`
assertion intermittently on the full model20 (in both a Tkrzw and a Kyoto
Cabinet build of the pin); a re-run on a fresh bigram.db completed. Delete
a partial bigram.db before retrying.
---- prerequisite validation ------------------------------------------------
Flavour assertion: KC, tkrzw and BDB all name their tables identically
(pinyin_index.bin, bigram.db, ...), so a wrong-flavour export would NOT
announce itself as a missing file — it would open as nonsense. The
manifest's backend token is the only thing that names the container.
Segment utils.
KMM utils.
Legacy counting / interpolation utils (lambda + counter differentials).
eval_correction_rate: needs the compiled system bigram.db + an evals2.text.
The built system data dir (cwd for KMM/segment utils) and the oxpinyin export.
The evaluator gate: wired only when its two extra inputs exist. The pin
side runs in the data dir (its .bin indexes, bigram.db, evals2.text); the
native SystemDictionary reads the oxpinyin export of the same model, so
the index paths point at the tkrzw export, not the pin's .bin files.
---- the suites -------------------------------------------------------------
The backend-forwarding crates run tkrzw explicitly, matching the pin;
oxpinyin-kmm follows the selected store backend, as its seven utilities do.

''',
}
