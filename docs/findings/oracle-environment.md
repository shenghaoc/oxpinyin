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
file. Run `tools/oracle/build-oracle.sh --help` for flags.

## Oracle boundary

- W2-T3 and every S1b parity run load only the pin-built shared object.
- A distribution-provided build may be compared as the advisory
  `distro-delta` class, but it never gates S1b and never becomes the oracle.
- The frontend source is not a backend implementation reference.
- A pin change requires a dedicated human-reviewed PR that updates tags,
  commits, archive hashes, fixtures, and the divergence baseline together.
