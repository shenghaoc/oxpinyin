# Oracle reference freeze

Date: 2026-08-07 · Status: recorded; human review required before freeze.

This is the **reference freeze for reproducibility (upstream release state
as of 2026-07-31)**. The oracle is built from the recorded source and data
archives, so parity runs are reproducible on any supported build host. An
authoritative run uses only binaries produced by this recipe and aborts if
any archive checksum or resulting source revision differs.

## Pin

| Component | Release tag | Commit SHA | Source archive | SHA-256 |
|---|---|---|---|---|
| libpinyin | `2.11.91` | `0c5e80e1200f84fab185d1c5bde458b770a0636c` | `https://codeload.github.com/libpinyin/libpinyin/tar.gz/refs/tags/2.11.91` | `eb25890dab0072eb0744c9ee1bc152051143b7bc23aea2a424792a9b1b84bdcb` |
| ibus-libpinyin | `1.16.5` | `2d2cdac0187101aa0cd7ac06694a8340721ddfbb` | `https://codeload.github.com/libpinyin/ibus-libpinyin/tar.gz/refs/tags/1.16.5` | `ab6d6cc371e4ec0cda1471ef968e9545de69a404958ecfb4e68545ef4b328646` |

Both are the latest release tags present in their upstream repositories on
or before the **reference freeze for reproducibility (upstream release state
as of 2026-07-31)**. libpinyin is the backend parity source.
ibus-libpinyin is used only to derive the frontend-called ABI subset and
GSettings schema, and to prove that the pinned frontend builds against the
pinned backend.

## Data artefacts

| Name | URL | SHA-256 |
|---|---|---|
| `model20.text.tar.gz` | `https://downloads.sourceforge.net/libpinyin/models/model20.text.tar.gz` | `59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155` |

The archive expands to `interpolation2.text` and the upstream table files
consumed by libpinyin's own data-generation targets.

## Build recipe

Canonical recipe: `tools/oracle/build-oracle.sh`. Its header comment lists
the build dependencies a host needs.

The script verifies every SHA-256 before extraction, builds both components
from their pinned source archives with autotools, and prints the absolute
path to the resulting `libpinyin` shared object. The libpinyin DBM backend is
pinned to Tkrzw; the deprecated Berkeley DB backend is not used. Its default
installation prefix is `WORK_DIR/prefix`, resolved after option parsing. The prefix must be
absent or empty so stale output cannot enter the oracle. The script resets
`PKG_CONFIG_PATH` and `LD_LIBRARY_PATH` to prefix-local paths rather than
inheriting caller search paths; explicit `CC`, `CXX`, `CFLAGS`, `CXXFLAGS` and
`LDFLAGS` overrides remain supported. A successful build writes
`oracle-pin.txt` plus `oracle-data.sha256` into the prefix, binding the pin ref
to checksums of the public header, shared object and every generated data
file that libpinyin produces reproducibly. The six files it does not
(`addon_phrase_index.bin`, `addon_pinyin_index.bin`, `bigram.db`,
`phrase_index.bin`, `pinyin_index.bin`, `punct.bin` — the DBM-backed
generation path) are listed in `oracle-data-unstable.sha256` instead and
recorded as `data_unstable_manifest_sha256`; that manifest is tamper-evident
within a prefix but two clean builds of the same pin legitimately disagree on
it, so only `data_manifest_sha256` is comparable across prefixes. See
`docs/findings/oracle-data-reproducibility.md`. Run
`tools/oracle/build-oracle.sh --help` for flags.

## Oracle boundary

- W2-T3 and every S1b parity run load only the pin-built shared object.
- A distribution-provided build may be compared as the advisory
  `distro-delta` class, but it never gates S1b and never becomes the oracle.
- The frontend source is not a backend implementation reference.
- A pin change requires a dedicated human-reviewed PR that updates tags,
  commits, archive hashes, fixtures, and the divergence baseline together.

## Multi-backend bench oracles

Added 2026-09-06 (append-only; everything above is unchanged). For
backend-comparison benchmarking only, `tools/oracle/build-oracle.sh`
accepts `--dbm <tkrzw|kc|bdb>` and configures libpinyin with the
corresponding DBM (`Tkrzw`, `KyotoCabinet`, `BerkeleyDB`). The choice is
recorded in the pin ref (`+dbm-<name>`) and in the `dbm=` field of
`oracle-pin.txt`; each backend gets its own explicit `--prefix`. The
default path — no flag — is unchanged: the parity oracle remains tkrzw
and nothing in this section applies to it.

A bench prefix is linked by setting `PINYIN_ORACLE_PREFIX` plus
`PINYIN_BENCH_DBM=kc|bdb`, which relaxes the frozen pin-ref comparison
in `crates/pinyin-oracle/build.rs` to a `dbm-<name>` containment check
(the manifest still records the full pin, hashes included). The
relaxation is CI-guarded — `PINYIN_BENCH_DBM` set under `CI` fails the
build — and `tools/capture/run-capture.sh` separately refuses any
prefix whose manifest `dbm=` is not `Tkrzw`, so parity and capture can
link only the tkrzw oracle regardless.

Berkeley DB is measured deliberately: it is libpinyin's upstream
default DBM and the configuration distributions ship, even though
libpinyin's own development deprecates it. Bench-only prefixes never
gate parity or capture.

## Amendment — pin 074a2219 (2026-09-06 UTC)

The oracle pin moved from `2.11.91`/`0c5e80e1200f84fab185d1c5bde458b770a0636c`
to `2.11.92`/`074a2219c90feaf962d0d24f034514033ece5f99` (libpinyin main
HEAD; nine commits, no release tag — `2.11.92` is untagged upstream).
Rows above are the historical 2.11.91 freeze and stand unedited.

- **Fetch form:** no tag tarball carries this pin, so the recipe now
  fetches by commit SHA (`git fetch --depth=1 <repo> <sha>`,
  `git checkout FETCH_HEAD`) and verifies by `git rev-parse HEAD`
  equality — an archive SHA-256 cannot pin a GitHub-regenerated
  tarball. ibus-libpinyin stays the tagged `1.16.5` archive.
- **Verification (2026-09-06 UTC, debian:testing container):** the oracle
  builds unpatched; the W2 candidate surface is byte-identical for all
  10,312 distinct corpus inputs (97,442 triples, 10,037 inputs with
  candidates — the frozen fixture's exact counts); the sentence
  surface re-freeze §12 counts hold at 491/396/390 with every
  mechanism invariant intact. See
  `docs/findings/oracle-pin-074a221-verification.md`.
- **Known upstream changes re-verified:** `libpinyin.so.15.0.0`
  unchanged (`libpinyin_abi_current=15`, revision 0, both pins); the
  public `pinyin.h` is byte-identical (same `header_sha256`); the six
  DBM-backed data files are build-nondeterministic at ANY pin (issue
  #358), so no between-pins data claim is made for them; the 17
  reproducible data files are byte-identical between pins.
- **oxpinyin's drop-in identity stays 2.11.91** (cargo-c header
  subdirectory, package version, `libpinyin.pc` version): distros ship
  2.11.91 and consumers build against `include/libpinyin-2.11.91/`;
  claiming an unreleased version would break drop-in. It moves only
  when upstream tags a release.

## Amendment — ibus-libpinyin by commit SHA (2026-09-07 UTC, #369)

The pin itself is unchanged: ibus-libpinyin stays `1.16.5` /
`2d2cdac0187101aa0cd7ac06694a8340721ddfbb`. Only its fetch and
verification form moves, to match libpinyin's:

- **Fetch form:** `tools/oracle/build-oracle.sh` now fetches
  ibus-libpinyin by commit SHA (`git fetch --depth=1 <repo> <sha>`,
  `git checkout FETCH_HEAD`, verified by `git rev-parse HEAD` equality)
  instead of downloading the `1.16.5` tag tarball and checking its
  SHA-256. The archive URL and archive hash in the row above are
  historical and no longer consulted by the recipe. `1.16.5` is a
  lightweight tag upstream and resolves to exactly that commit
  (`git ls-remote`, 2026-09-07 UTC).
- **Provisioning mirror:** `tools/oracle/oracle-pin.txt` is schema
  `oracle-provisioning-pin-v3`; `ibus_libpinyin_archive_sha256` is
  dropped and both upstreams are verified symmetrically by commit SHA.
  The prefix manifest (`schema=pinyin-oracle-v1`) is unchanged: it never
  carried an archive hash for either upstream, so the pin ref, every
  `oracle-pin.txt` a built prefix writes, and every fixture stamp are
  byte-identical before and after this change.
- **Rationale:** GitHub regenerates archive tarballs and has changed
  their compression before, so an archive hash can drift while a commit
  SHA cannot; a tag can also be moved, which a commit SHA pin ignores.
