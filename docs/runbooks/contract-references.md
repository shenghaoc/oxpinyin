# Contract references: capture and verify

`contract-diff.py` has two operations. `verify` is the routine one: it runs the selected
cases on oxpinyin only and compares each result with the pin's observation, frozen in
`fixtures/contract-refs/`. `capture` writes those references from the pin. An investigative
live comparison is a capture into a temporary directory followed by a verify against it.

## verify

```sh
just fetch-contract-data                       # once: the pinned system data
just runner contract-diff verify tkrzw target/debug/libpinyin_capi.so target/debug/libzhuyin_capi.so
just runner contract-diff verify tkrzw ... -- --cases save-929-logical-cross-open-pinyin
just runner contract-diff verify tkrzw ... -- --expect-parent        # a parent build
```

`verify` needs the subject libraries and the references. It needs no pin prefix, pin
library or container, and it never captures. A missing, incomplete or incompatible
reference fails its case as `NO-REFERENCE` with the reason (the file, the field that
differs, and the capture command). Nothing is skipped.

The full tkrzw `verify` runs in `verify-nightly.yml` on its schedule and by manual
dispatch before a release. KC and BDB `verify` run in the store-backends jobs when
KV database code changes.

`--expect-parent` verifies the parent build against the same references: every case marked
`control` must still pass and every other case must fail (revert-and-check).

## What a reference holds

One JSON line per case and cell in `fixtures/contract-refs/<cell>/<case module>.jsonl`:
the pin's complete observation exactly as the driver observes it (exit status, result
document, untouched output parameters, byte sequences, ordering, diagnostics, user-directory
effects), never a verdict, and the identity that makes it valid:

| identity | content |
| --- | --- |
| case and inputs | the case ID and a digest of the case function's source, bound arguments and closure values, and registration |
| pin | commit `074a2219…` and the SHA-256 of the pin library captured from |
| backend | `bdb`, `kc` or `tkrzw` |
| system data | `package` with the digest of the data directory, or `synthetic` |
| capture environment | image identity, GLib and glibc versions, architecture |
| libc scanner | each lambda `%f` case's capture-host acceptance and parsed float bits |

The oxpinyin commit is not part of it. A policy-accepted departure is not stored in the
reference: its digests stay in its discrepancy record and are applied at verification, so it
is never MATCH and the pin's output is never replaced. A case the pin must not run keeps
its source-derived expectation, with `provenance: pin_not_run`.

A handoff case (`save-929-logical-cross-open-*`, `issue1114-collision-load-save-*`) also
stores the profile the pin wrote, compressed. Verification opens it with oxpinyin as an
ordinary fixture, so pin -> oxpinyin is checked on every run. The profile oxpinyin writes is
compared exactly where the format is deterministic. The other direction, whether the pin can
read a file oxpinyin wrote, cannot come from a frozen reference; see the next section.

## Reverse cross-open (local only)

```sh
just runner contract-diff verify tkrzw target/debug/libpinyin_capi.so target/debug/libzhuyin_capi.so \
    -- --reverse-cross-open PIN_PREFIX
```

`PIN_PREFIX` is a pin build for the cell (`PREFIX/lib/libpinyin.so` and `libzhuyin.so`, as for
`capture`). It runs these four cases live, pin and oxpinyin, each saving a profile that the other
library then reads, and prints one row per case:

- `issue1114-collision-load-save-pinyin` and `-zhuyin`
- `save-929-logical-cross-open-pinyin` and `-zhuyin`

Run it after any change to a persistence writer, to serialisation or to a database format. It
uses the same implementation as the earlier live comparison (`Runner.run_case`); it is not a
third operation and adds no runner or test interface. It starts the pin, so it refuses to run
when `GITHUB_ACTIONS` is set and no workflow calls it.

## Backend coverage

The three backends do not have equal contract coverage. tkrzw has the complete default
contract suite. Kyoto Cabinet and Berkeley DB have data-independent contract cases (the
cases whose tkrzw references are `synthetic`, which build their own data and read no package
data), plus the native backend tests in the crates that own each backend. Do not read a KC or
BDB verify run as covering the suite. Use `--plan-only` with the verify command
to print the selected case count; a completed verify prints `verdict_counts`.

## Host-dependent results

`verify` compares first. An equal observation is `MATCH` (or `POLICY-ACCEPTED`) on any host.
For each lambda `%f` input, capture stores whether libc accepts it and the parsed float's
32-bit value. Verification calls the host libc's `sscanf` once for the same text and reports
both verdicts. If they agree, the complete observation must compare normally, regardless of
glibc version. If they differ, any changed observation field is `HOST-DEPENDENT`. The two
cases that remove the lambda line have no scanner input and compare normally. GLib
fatal-critical cases still allow only a changed termination signal when the capture and host
GLib versions differ. Reports count `HOST-DEPENDENT` separately from `MATCH`.
For a registered directory-chunk case, capture also stores the pin scratch directory's
size-gate and header-read verdict. Verification probes the same operations in its own
scratch directory. Matching verdicts require an exact observation; differing verdicts
are `HOST-DEPENDENT` and both verdicts appear in the report row. A missing recorded
verdict is `NO-REFERENCE`.

## System data

The data is Debian forky's `libpinyin-data` at one exact version
(`tools/oracle/libpinyin-data.json`), from snapshot.debian.org, verified by SHA-256 and by
the digest of its files. `just fetch-contract-data` puts it under `target/contract-data/`;
`capture` and `verify` take it with `--data`. model20 is not used. The package's `bigram.db`
is a tkrzw file, so the cases that read it run on the tkrzw cell only. KC and BDB run the
cases that build their own data; the tkrzw references say which those are
(`system_data.kind`).

## capture

```sh
bash tools/oracle/capture-contract.sh tkrzw                       # the default set
bash tools/oracle/capture-contract.sh kc --cases NAME[,NAME]
```

On x86_64 it uses the oracle tag from `just compat image-tag`; on aarch64 it uses that tag
with `-arm64`. It refuses other architectures and runs no image under emulation. If the
expected image exists locally, it uses it; otherwise it pulls the published image. If neither
is available, build the image locally with:

```sh
bash tools/oracle/build-contract-image.sh
```

The local command builds `tools/oracle/Dockerfile.oracle` for the host architecture and
checks the resulting libraries against `tools/oracle/oracle-pin.txt` using `compat verify-image`.
The Dockerfile runs the existing `build-oracle.sh`. No GitHub Actions job participates in
this path. Capture builds the small capture image
on Debian stable (`Dockerfile.capture`: the pin's libraries, GLib, the DBM libraries and the
managed Python), mounts the checkout read-only and the data, and runs
`contract-diff.py capture` inside. The checkout is not changed
except for `fixtures/contract-refs/`. Each case is captured twice and a case whose two
captures differ is refused (`UNSTABLE`). Capture tkrzw before KC and BDB, which read its
references to know which cases to run.
