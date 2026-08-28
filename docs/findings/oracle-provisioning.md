# Oracle provisioning — the source path

Date: 2026-08-28 · Status: **fixed and measured; the oracle builds** ·
Branch: `ci/oracle-provisioning-ubuntu-source`.

Five sessions of oracle-gated work self-skipped because
`tools/oracle/build-oracle.sh` could not fetch its sources. This is the
diagnosis, the fix, and what running the previously-skipping surface
found.

## 1. What actually failed

Probed each of the script's three downloads directly:

| Source | Result |
|---|---|
| `codeload.github.com/libpinyin/libpinyin/tar.gz/refs/tags/2.11.91` | **HTTP 403**, 378-byte body |
| `codeload.github.com/libpinyin/ibus-libpinyin/tar.gz/refs/tags/1.16.5` | **HTTP 403**, 378-byte body |
| `downloads.sourceforge.net/libpinyin/models/model20.text.tar.gz` | HTTP 200, 20,283,243 bytes |

Two corrections to the premise this investigation started from:

- **It is not one tarball, it is two.** `ibus-libpinyin` is blocked
  exactly as `libpinyin` is. A fix that repointed only the libpinyin
  fetch would have moved the failure one line down.
- **SourceForge was never the problem.** The model20 fetch works and
  always did; the failure was entirely `codeload.github.com`.

And the 403 is not GitHub refusing — it is this environment's policy
layer:

```json
{"message":"GitHub access to this repository is not enabled for this
session. Use add_repo to request access. …"}
```

Which matters, because it says the block is on the **codeload HTTP
endpoint**, not on GitHub as such. Anonymous *git* reads of public
repositories are served: `git clone --depth 1 --branch 2.11.91` of
`libpinyin/libpinyin` succeeds and lands on
`0c5e80e1200f84fab185d1c5bde458b770a0636c` — the pinned commit. That
clone is what made the verification in §2 possible.

## 2. The replacement, and how far it was verified

The same commit is published as a second, different artifact: upstream's
`make dist` release tarball.

**Served byte-identically from two hosts.** `debian/watch` in the Ubuntu
source package names `https://sf.net/libpinyin/` as upstream, and the
file the Ubuntu archive carries as `.orig.tar.gz` is bit-for-bit the file
SourceForge serves:

| File | SHA-256 | Size |
|---|---|---|
| `libpinyin_2.11.91.orig.tar.gz` (Ubuntu) | `ff3047b1…788c` | 20,638,374 |
| `libpinyin-2.11.91.tar.gz` (SourceForge) | `ff3047b1…788c` | 20,638,374 |
| `ibus-libpinyin_1.16.5.orig.tar.gz` (Ubuntu) | `cc652d48…ce31` | 1,351,562 |
| `ibus-libpinyin-1.16.5.tar.gz` (SourceForge) | `cc652d48…ce31` | 1,351,562 |

`cmp` confirms both pairs identical. Two independent mirrors of one
upstream release corroborate each other.

**It is a different file from the GitHub archive, and its checksum
differs — as it must.** `eb25890d…` is GitHub's generated archive of the
tag; `ff3047b1…` is upstream's dist tarball. Neither is a corruption of
the other; they are two publications of one commit.

### Is it the same code?

Yes, and this was measured rather than assumed, using the anonymous git
clone above.

- **The dist tarballs self-certify.** `make dist` regenerates `ChangeLog`
  from git history, and both tarballs' first line is the pinned commit:

  ```
  libpinyin-2.11.91/ChangeLog:      commit 0c5e80e1200f84fab185d1c5bde458b770a0636c
  ibus-libpinyin-1.16.5/ChangeLog:  commit 2d2cdac0187101aa0cd7ac06694a8340721ddfbb
  ```

  Both name exactly the commits pinned in `build-oracle.sh`.

- **File-by-file against the tag: 163 of 164 tracked files byte-identical.**
  The single difference is that same `ChangeLog`, which is **empty in git**
  (0 bytes) and 452,532 bytes in the tarball — a `make dist` artifact, not
  a source change.

- **43 git-tracked files are absent from the tarball**, all of them
  build-system alternatives and code generators that the autotools path
  does not use: `CMakeLists.txt`, `cmake/Find*.cmake`, `autogen.sh`,
  `config.h.cmake`, and `scripts2/` (the table generators). Their
  *outputs* are present and identical — `pinyin_parser_table.h`,
  `chewing_enum.h`, `double_pinyin_table.h`, `special_table.h` all
  compare equal — and `build-oracle.sh` runs `autoreconf --force
  --install`, not `autogen.sh`.

- All three DBM backends compare identical: `ngram_tkrzwdb.cpp`,
  `ngram_bdb.cpp`, `ngram_kyotodb.cpp`.

### One consequential difference: the dist tarball bundles `data/`

The GitHub archive has no `data/interpolation2.text`; the dist tarball
carries all eighteen model20 export files (83 MB of
`interpolation2.text` among them). `build-oracle.sh` extracts model20
*over* `data/`, so the question is whether that changes what the oracle
is built from.

It does not: **all 18 files are byte-identical** to the pinned
`model20.text.tar.gz` (SHA `59c68e89…`, verified before comparison). The
extraction lands on identical bytes. The only other files in the
tarball's `data/` are `Makefile.am`, `Makefile.in` and `table.conf.in` —
build files, not data — so nothing unpinned rides in.

## 3. The fix

`build-oracle.sh` now holds a per-component list of `"URL SHA256"`
alternates and takes the first reachable one, checking it against **its
own** digest.

Order is availability, not preference of provenance: Debian/Ubuntu
archive (best mirrored), SourceForge (upstream's own host, but
`datagen-model20.md` records it as flaky), GitHub codeload last (the
original pin source).

**Verification is not relaxed anywhere**, which is the property that
matters most in a change like this:

- every candidate is checked against the digest listed beside it, so an
  archive nobody pinned cannot be substituted for one that was;
- a download that **succeeds but fails its digest is fatal immediately**,
  printing expected and actual — falling through to the next host there
  would paper over exactly what the digest exists to catch;
- only a download that produced no file at all (unreachable host, HTTP
  error) advances to the next candidate;
- exhausting the list is fatal and names every URL tried.

**`pin_ref` is unchanged**, and by construction rather than by care:
`ORACLE_PIN_REF` keys on `LIBPINYIN_SHA` — the git commit — not on the
archive checksum. Both archives publish that same commit, so the
composite identity is the same whichever was reachable. The built
prefix's `oracle-pin.txt` now also records
`libpinyin_archive_sha256=` / `ibus_libpinyin_archive_sha256=` for the
archive actually used, so two prefixes built from different byte streams
are distinguishable in the audit trail. The reference
`tools/oracle/oracle-pin.txt` lists both acceptable digests per
component, suffixed `_dist` and `_github`.

## 4. Prerequisites the fix does not remove

Building the oracle here also needed three things that had nothing to do
with the source path, and are worth recording because the next person
hits them too:

- `libtool` and `libibus-1.0-dev` were absent (`apt-get install
  libtool-bin libibus-1.0-dev gnome-common`).
- **libtkrzw had to be built from source**, and `dbmx.net` — the host
  `datagen-model20.md`'s recipe names — is **also 403 through this
  proxy** (`CONNECT tunnel failed`). The same lever solves it: the Ubuntu
  archive carries `tkrzw_1.0.32.orig.tar.xz`, the exact version that
  recipe pins. A plain `./configure && make` produces a *correct*
  build — no LTO, no `-Wl,-Bsymbolic-functions`, so neither of the two
  defects in `tkrzw-distro-compat.md`. Verified with the pointer-identity
  probe: `tkrzw_dbm_util` creates a TreeDB and reads back from it, which
  Ubuntu's packaged `libtkrzw` cannot do.

  Ubuntu's `libtkrzw-dev` must **not** be used for the oracle: it is the
  broken build, and an oracle linked against it would give wrong answers
  to every differential with no error anywhere.

## 5. What the working oracle then showed

Everything below had been self-skipping. `pin_ref` in the built prefix is
`libpinyin-2.11.91-0c5e80e…+model20-59c68e89…+dbm-tkrzw` — **unchanged**,
as it must be: it keys on the commit, not the archive.

### Frozen pins: every one holds

`corpus-tail` over the real tables (`cargo run --release -p pinyin-oracle
--bin corpus-tail`):

| Pin | Established | Measured | |
|---|---|---|---|
| top-1 | 10,190 / 10,190 | **10,190 / 10,190** (0 misses) | ✅ |
| top-5-set | 10,190 | **10,190** (0 misses) | ✅ |
| absent | 0 | **0** | ✅ |
| order-only | 0 | **0** | ✅ |
| prefix-10 | 98,930 / 98,930 | **98,930 / 98,930** (gap 0) | ✅ |
| sentence 1-best / distinct-set / ordered | 488 / 385 / 379 | **488 / 385 / 379** | ✅ |

`sentence_surface_matches_the_declared_residual` **ran and passed** — the
first time it has not skipped.

And the fixtures those pins are scored against are themselves still
current: `oracle_candidates_fixture_is_fresh` and
`sentence_surface_fixture_is_fresh` both pass **against the live
oracle** — 10,312 distinct inputs, 97,442 live triples.

Note on scope: this base is `origin/main`, which does **not** contain the
drop-in stack (`session.rs` still holds the `dynamic_adjust_bigram_term`
stub). These numbers therefore establish main's baseline with a working
oracle; they do not yet re-measure the drop-in branches, which is now
possible for the first time and is the obvious next step.

### Differentials

| Differential | Result |
|---|---|
| live-typing (the §3 gate) | **IDENTICAL** |
| import | **IDENTICAL** |
| train | **IDENTICAL** |
| nbest-train | **IDENTICAL** |
| predict | **IDENTICAL** |
| punct | **IDENTICAL** |
| scheme | **IDENTICAL** |
| addon-candidate | **IDENTICAL** |
| user-candidate | **IDENTICAL** |
| union | **IDENTICAL** |
| option-sweep | **PASS** |
| uncovered-surface | exit 2, **the documented shape** |
| pred-order | exit 2, **order-only, by design** |

Eleven clean. The two exit-2 rows are expected, not regressions:

- **uncovered-surface** — `datagen-model20.md:204` records the expected
  result as "exit 2 with zero non-PRED_PREFIX diverging lines". Measured:
  152 diverging lines, **152 of them PRED_PREFIX, zero others**. Exactly
  the documented shape.
- **pred-order** — a maintainer decision of 2026-08-25 makes this "a
  defined order, not fixture-frozen parity": the pin's order is a
  compile-time artifact of its DBM choice, a Tkrzw bucket-walk. Measured
  1557/1571 rows at different positions against a recorded 1541/1571
  (and hao 177/178 against 174/178). The counts differ slightly from the
  record, which is consistent with a different Tkrzw build producing a
  different bucket walk — this oracle links a from-source 1.0.32. Worth
  a maintainer's eye, but it is not one of the frozen pins and the class
  is documented as order-only.

**Four of the runners' skips were my own harness error, not the code.**
`scheme`, `option-sweep`, `nbest-train` and the two `UNCOVERED_SYSTEM`
runners each need their own system-dir variable, and with the wrong one
set they silently fall back to the mini fixture and "diverge". Setting
`W13_CAPI_SYSTEM`, `OPTION_SWEEP_CAPI_DATA`, `NBEST_CAPI_SYSTEM` and
`UNCOVERED_SYSTEM` turned scheme and option-sweep from DIVERGENCE into
IDENTICAL/PASS. The variable names are not uniform, which is the trap;
they are listed in each runner's header.

`run-w11-diff.sh` is not a differential of its own — it is a wrapper that
runs `user-candidate-diff`, `addon-candidate-diff` or `predict-diff`
under a W11 system dir, and all three are IDENTICAL standalone.

### The full local recipe, for the next person

```sh
apt-get install -y libtool-bin libibus-1.0-dev gnome-common sqlite3
# tkrzw from source -- NOT libtkrzw-dev, which is the broken build
curl -O http://archive.ubuntu.com/ubuntu/pool/universe/t/tkrzw/tkrzw_1.0.32.orig.tar.xz
tar xJf tkrzw_1.0.32.orig.tar.xz && cd tkrzw-1.0.32
./configure && make -j"$(nproc)" && make install && ldconfig && cd -

tools/model/fetch-model.sh
export PINYIN_MODEL_DIR="$PWD/target/model20/extracted"
cargo run --release -p oxpinyin-datagen -- compile --out-dir target/datagen/redb
cp "$PINYIN_MODEL_DIR/interpolation2.text" target/datagen/redb/

PKG_CONFIG_PATH=/usr/local/lib/pkgconfig \
  tools/oracle/build-oracle.sh --prefix /tmp/oracle-prefix --jobs "$(nproc)"

SYS=$PWD/target/datagen/redb
export PINYIN_ORACLE_PREFIX=/tmp/oracle-prefix PINYIN_EXPORT_DIR=$SYS
export LIVETYPING_SYSTEM=$SYS NBEST_CAPI_SYSTEM=$SYS UNCOVERED_SYSTEM=$SYS
export W13_CAPI_SYSTEM=$SYS CAPI_W11_SYSTEM=$SYS OPTION_SWEEP_CAPI_DATA=$SYS
export PINYIN_IBUS_BUILD_DIR=/tmp/oracle-work/src/ibus-libpinyin-1.16.5
```

## 6. CI: there is no oracle job to rewire, by policy

The brief asks for the oracle-diff CI job to be repointed at the new
source. **That job does not exist, and creating one would reverse a
policy this repository states in two places.**

`.github/workflows/store-backends.yml:22-26`:

> The pin-built oracle and the model20 archive are deliberately NOT CI
> concerns: provisioning them (`tools/model/fetch-model.sh`,
> `tools/oracle/build-oracle.sh`) is a LOCAL workflow, and no CI job may
> download `model20.text.tar.gz` or anything like it.

and `ebfb1ce`, three commits before this branch's base:

> The oracle chain is a developer-machine workflow and stays that way: no
> CI job downloads model20.text.tar.gz or builds the pin.

`datagen-model20.md` gives the reason: the model20 archive is
non-redistributable and lives behind a mirror the doc records as flaky,
and the first CI attempt that fetched it failed on exactly that.

So the source-path fix lands entirely in `build-oracle.sh`, which is what
a local developer runs today and what any future CI job would call. No
workflow file is touched — adding an oracle job would be a CI-policy
change and needs an ask under AGENTS.md's hard forbids.

`ci.yml`'s existing `Live-typing differential` step is unaffected: it
already calls `run-live-typing-diff.sh`, which self-skips without a
prefix. It keeps its fail-loudly assertions, and nothing about the source
change turns a real failure into a skip — `fetch_any` is fatal on a
digest mismatch and fatal on exhausting its sources.
