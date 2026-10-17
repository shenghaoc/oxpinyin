# Cloud sessions — provisioning a Claude Code cloud host

A Claude Code cloud session is a supported host for every gate in this
repository (`AGENTS.md`), and so is any Linux host with rootless podman
(below). A cloud session is a root container with Docker installed
but not necessarily running, outbound HTTPS only through a session proxy
that re-terminates TLS, and a network policy that allows a fixed list of
domains. `tools/cloud/setup.sh` brings it to the state the gates expect;
this page is what it does, what the environment must allow, and what it
costs.

## Setup command

**When to run it.** The environment's setup script stays empty. A lane runs
the setup only when its task needs the pin oracle (a differential, a
same-run measurement against the pin, a data check); a lane that does not
touch the oracle never pays for it. From the checkout:

```sh
tools/cloud/setup.sh
```

It is idempotent: every step checks its result and skips when it holds,
so a second run only re-verifies (under a second here). Options:
`--cache-dir DIR` (oracle prefixes, the model cache and the tkrzw build;
default `~/.cache/oxpinyin`), `--cells`, `--jobs`, `--no-pull` (build the
libraries even if the published image exists). `OXPINYIN_CONTAINER`
names the container command (see step 1). Progress goes to stderr;
stdout is one line per cell:

```
bdb prefix=<dir> libpinyin=<sha256> libzhuyin=<sha256> data manifest OK
```

### The environment file

On success the setup writes `~/.oxpinyin-oracle.env` (`OXPINYIN_ORACLE_ENV`
overrides the path), rewritten on every run. A lane sources it before a
runner:

```sh
. ~/.oxpinyin-oracle.env                       # reference cell, bdb
PINYIN_ORACLE_DBM=kc . ~/.oxpinyin-oracle.env  # another cell
```

It exports, for each provisioned cell `<C>` in `BDB`, `KC`, `TKRZW`:
`OXPINYIN_ORACLE_PREFIX_<C>` (the oracle prefix) and
`OXPINYIN_ORACLE_DATA_<C>` (its `lib/libpinyin/data`); `PINYIN_MODEL_DIR`
(the extracted model20 cache); and, for the cell named by
`PINYIN_ORACLE_DBM` (the caller's value, else `bdb`, else the first
provisioned cell), `PINYIN_ORACLE_PREFIX` and `OXPINYIN_LIBPINYIN_DATA_DIR`,
recomputed each time the file is sourced. Naming a cell the setup did not
provision stops the `source` with an error. The file needs bash.

## What it does

0. Installs Valgrind with `apt-get install -y valgrind` (through `sudo`
   when not root) if `valgrind --version` fails, and prints the version in
   the summary; `benches.md` requires every measurement report to state it.
   Then requires the backend dump utilities the stateful differentials
   call, for the cells requested: `db5.3_dump` (package `db5.3-util`, bdb)
   and `kchashmgr` (`kyotocabinet-utils`, kc). A missing one is installed
   with `apt-get install -y` when running as root; otherwise the script
   prints `run as root: apt-get install -y <packages>` and exits 1. It never
   runs `sudo` for these. `tkrzw_dbm_util` (tkrzw) comes from step 3b.
1. Chooses the container command: `$OXPINYIN_CONTAINER` if set, else
   `podman` if it is on `PATH`, else `docker`; every container call goes
   through it. Only the docker path starts `dockerd` (detached, `setsid
   nohup dockerd`) unless `docker info` already answers, and waits for it.
   When it starts the daemon it first installs
   `/root/.ccr/agent-proxy-ca.crt` into the system trust store, so the
   daemon's own pulls trust the session proxy. podman needs no daemon, no
   root and no `sudo`: `podman info` must answer, and the pulls, the
   `deps` build and the data runs are the same commands rootless.
2. Pulls the `base_image` digest of `tools/oracle/oracle-pin.txt`
   (`debian:testing@sha256:dab11cdb…`), retrying 429s from the registry
   at 2, 4, 8 and 16 s.
3. Gets the libraries for bdb, kc and tkrzw (linux/amd64, the platform
   the recorded hashes are for). If
   `ghcr.io/shenghaoc/oxpinyin-oracle:<tag>` exists, with `<tag>` printed by
   `tools/oracle/image-tag.sh` (pin, base digest, apt snapshot and a hash
   of the recipe), it is pulled and the cells are copied out of it. A cell
   whose pulled hashes do not match `oracle-pin.txt` is discarded and built
   instead, as is every cell when no image exists: the pinned image's
   `deps` stage is built and `tools/oracle/build-oracle.sh --libs-only
   --enable-libzhuyin --dbm <cell>` runs inside it, at the same in-image
   paths as the Dockerfile (`/build/<cell>`, `/opt/pinyin-oracle/<cell>`).
   Whichever way the libraries arrived, the script then compares
   `libpinyin.so` and `libzhuyin.so` with `libpinyin_sha256_<cell>` and
   `libzhuyin_sha256_<cell>` in `oracle-pin.txt` and **stops with exit 3
   after printing both values** if any differs: a library that was *built*
   here and differs means the recipe is not reproducible on this host.
   3b. The tkrzw cell's host library. `tools/tkrzw/distro-probe.sh` is run
   against the host's `libtkrzw` (`tkrzw_dbm_util` on `PATH`). If it
   exits 0 nothing more happens. If it reports a defect, or finds no
   tkrzw, upstream tkrzw is built from source at
   `9ee84164cf45e441aa8186418c03a02c9b89f08b` (1.0.34: its sentinel fix
   makes defect 1 immune to LTO; 1.0.32 is healthy only without LTO,
   `docs/findings/tkrzw-distro-compat.md`) with `env -u CFLAGS -u CXXFLAGS
   -u LDFLAGS ./configure --prefix=<cache>/tkrzw && make && make install`,
   so no LTO or `-Bsymbolic-functions` can come in from the environment.
   The probe is then run against that build and the build is refused
   unless it prints `RESULT : healthy` and resolves to the cache. The
   environment file puts `<cache>/tkrzw` first on `PKG_CONFIG_PATH`,
   `LD_LIBRARY_PATH` and `PATH` (so `tkrzw_dbm_util` is the built one), and
   the script checks with `ldd` that the tkrzw oracle `libpinyin.so`
   resolves `libtkrzw.so.1` to `<cache>/tkrzw/lib`. A rerun skips the
   build when the cached revision is present and its probe passes.
4. Fetches model20 with `tools/model/fetch-model.sh` into
   `<cache>/model20`, runs `build-oracle.sh --regen-data` per cell inside
   the image, and re-verifies the installed files against
   `oracle-data.sha256`. The data is generated in a staging area and swapped
   in only if the regenerated stable manifest equals
   `data_manifest_sha256_<cell>`; otherwise the step fails and the prefix is
   untouched.
5. Rewrites the prefixes' `.pc` files to the host location, writes the
   environment file above and prints the per-cell lines.

### How containers get out

Processes in a container cannot reach the proxy at `127.0.0.1` and do not
trust its CA (`/root/.ccr/README.md`, "docker build / docker run"), so
every `docker run` here uses `--network host`, passes `https_proxy` and
the CA (`GIT_SSL_CAINFO`, `SSL_CERT_FILE`, `CURL_CA_BUNDLE`) and mounts
`/root/.ccr/ca-bundle.crt`. The `deps` build passes the proxy as a build
arg and the CA as the optional BuildKit secret `cacert`
(`tools/oracle/Dockerfile.oracle`); with the secret, apt reads the same
dated snapshot over https instead of http. Without it (CI, a workstation)
nothing changes, and the installed packages, hence the toolchain, are the
same.

## Allowlist domains

Network access must be Limited-with-allowed-domains or broader. Hosts the
setup contacts, and what a session showed on 2026-10-16 UTC:

| Host | Used for | Seen |
| --- | --- | --- |
| `registry-1.docker.io`, `auth.docker.io`, `production.cloudflare.docker.com` | the pinned `debian:testing` digest | reachable (429 under load; retried) |
| `snapshot.debian.org` | the apt snapshot the toolchain is installed from | reachable once allowed (403 before) |
| `github.com` | libpinyin at the pinned commit (`git fetch --depth=1 <url> <sha>`) | reachable |
| `downloads.sourceforge.net` and its redirect target, a rotating `*.dl.sourceforge.net` mirror | model20 archive | reachable (redirected to `cytranet-dal.dl.sourceforge.net`) |
| `ghcr.io` (and the blob host it redirects to) | the published oracle image, when it exists | token endpoint answers; no image published yet, blob host untested |
| `index.crates.io`, `static.crates.io` | cargo | reachable |

`snapshot.debian.org` is the one that is easy to miss: the pinned
toolchain is installed from it, and `--regen-data` compiles the pin's data
build, so both the library build and the data step need the `deps` image.
A published image would save only the library compile; it does not remove
the apt dependency.

## Setup time and disk use

Measured on 2026-10-16 UTC in a 4-core, 15 GiB cloud session (x86_64,
`/dev/vda`, `docker` 29.8.2). No oracle image was reachable in that run, so
only the build path ("without the published image") was measured here; the
"with" column is measured end to end in "With the published image, from
scratch" below, not step by step.

**Starting state of the from-scratch figures.** Docker emptied and stopped,
and the cache removed, then `df` read before and after:

```sh
docker rm -f $(docker ps -aq); docker system prune -af; docker builder prune -af
pkill -x dockerd; sleep 3; rm -rf /var/lib/docker/* ~/.cache/oxpinyin
df --output=used -k / | tail -1        # before: 12716192
SECONDS=0; tools/cloud/setup.sh > /dev/null 2> setup.err
echo "total=${SECONDS}s"               # bash's elapsed seconds: 353
df --output=used -k / | tail -1        # after: 14773784
```

Each step's time is the difference between consecutive `[ Ns ]` offsets
that `setup.sh` prints on stderr (the offset is bash's `$SECONDS` since the
script started); a step ends where the next one starts.

| Step (stderr offsets) | Without the published image | With the published image |
| --- | --- | --- |
| `dockerd` start and pinned-digest pull (`0` to `6`) | 6 s | same |
| `deps` image build (`6` to `68`) | 62 s | same (the data step needs it) |
| libraries bdb, kc, tkrzw (`68` to `96`, `96` to `148`, `148` to `180`) | 28 s, 52 s, 32 s (112 s) | not measured |
| model20 fetch, once, and `--regen-data` bdb, kc, tkrzw (`180` to `224`, `224` to `301`, `301` to `353`) | 44 s, 77 s, 52 s (173 s) | not measured |
| setup, end to end (`total=` above) | **353 s** | not measured |

The model fetch (`tools/model/fetch-model.sh --cache-dir
~/.cache/oxpinyin/model20`) runs inside the bdb data step and is not timed
on its own.

**Second run, everything present.** Run immediately after the first, with
nothing removed: `time tools/cloud/setup.sh > /dev/null 2>&1` reported
`real 0m0.332s`.

**Disk.** `df` used space rose by 2,057,592 KiB (about 2.0 GB: 14773784
minus 12716192 above). Its parts, read after the run:

```sh
du -sh ~/.cache/oxpinyin/model20                  # 105M (archive and extraction)
du -sh ~/.cache/oxpinyin/oracle/{bdb,kc,tkrzw}    # 63M, 78M, 54M (193M together, with data)
docker system df                                  # images 1.03GB, build cache 1.028GB
```

The build cache shares its layers with the images (`docker system df`
reports 12.29 kB of it reclaimable), so the two Docker figures are not
additive. `docker builder prune -af` after the setup drops the cache copy.

### tkrzw step and the CI child

CI's tkrzw child uses Debian's `libtkrzw` 1.0.32, built without LTO; the
cloud setup builds upstream 1.0.34 (`9ee8416`) when the host's library fails
the probe. The two differ in version, and both pass
`tools/tkrzw/distro-probe.sh` (`RESULT : healthy`), which is the criterion
the setup enforces.

### Rootless podman on a workstation

Measured 2026-10-16 UTC on the maintainer's own host (Ubuntu 26.10, 12
cores, 15 GiB, podman 5.8.6 rootless, no `sudo`, no Docker; the host's
`libtkrzw` 1.0.32 is Ubuntu's and fails the probe). Not a cloud session, so
these figures are **not** the cloud figures above. The podman image store
already held the pinned base image and the `deps` image, so neither was
pulled or built; the cache directory was new and empty. `db5.3_dump` and
`kchashmgr` were unpacked from the distribution `.deb`s onto `PATH`
(`apt-get download`, `dpkg -x`) because the host user cannot install
packages. The published oracle image existed on this host's registry
path and was pulled, so the libraries came from it, not from a build.

`SECONDS` offsets from the stderr of `tools/cloud/setup.sh --cache-dir
<new>`:

| Step | Seconds |
| --- | --- |
| podman check, base image present (`0` to `1`) | 1 |
| published image pull, three cells copied out, hashes match (`1` to `7`) | 6 |
| tkrzw: probe, source fetch, configure, `make -j12`, install, probe, `ldd` (`7` to `45`) | 38 |
| model20 fetch and `--regen-data` bdb, kc, tkrzw (`45` to `114`, `114` to `205`, `205` to `279`) | 69 (including the 20 s fetch), 91, 74 |
| setup, end to end (`time`) | **278.6 s** |

Rerun with nothing removed: `real 0m1.959s`; it re-probes the host
library, probes the cached build, and re-runs the `ldd` check. The cached
build is `120M`.

After sourcing the generated environment file, the tkrzw oracle
`libpinyin.so`, the `oxpinyin-capi` tkrzw `libpinyin_capi.so` and an
`oxpinyin-store` test binary built with `--features tkrzw`
(`cargo test -p oxpinyin-store --no-default-features --features tkrzw
--no-run`) all resolve `libtkrzw.so.1` to `<cache>/tkrzw/lib`; the store
test binary also carries that directory as its `RUNPATH`. On this host the
`issue784-index-keys-*` contract-diff cases (26, pinyin and zhuyin) ran
`MATCH` 26 of 26 on bdb, kc and tkrzw.

### This change in a Claude Code cloud session

Measured 2026-10-16 UTC in a Claude Code cloud session with this change at
`c270ad8`: 4 cores, x86_64, root, `docker` 29.8.2, no `podman`. This is the
session's own host, not the maintainer's workstation above, and not the
earlier 15 GiB session in "Setup time and disk use". `~/.cache/oxpinyin`
did not exist before the first run.

**Image and tkrzw.** In this run no oracle image was found at the tag the script
tries (`ghcr.io/shenghaoc/oxpinyin-oracle:074a2219c90f-…`), so the libraries
were **built**, not pulled, and the `deps` image was built. The host
`libtkrzw` failed the probe, so the **tkrzw source build ran** (upstream
`9ee8416`). `db5.3-util` and `kyotocabinet-utils` were installed with
`apt-get` (root).

`SECONDS` offsets from the stderr of `bash tools/cloud/setup.sh`:

| Step | Seconds |
| --- | --- |
| dump utilities (`1` to `4`, including the `dockerd` start) | 3 |
| base image pull and the published-image attempt (`4` to `8`) | 4 |
| `deps` image build (`8` to `94`) | 86 |
| libraries bdb, kc, tkrzw (`94` to `142`, `142` to `212`, `212` to `267`) | 48, 70, 55 |
| tkrzw: probe, source build, install, probe, `ldd` (`267` to `329`) | 62 |
| model20 fetch and `--regen-data` bdb, kc, tkrzw (`329` to `398`, `398` to `516`, `516` to `596`) | 69, 118, 80 |
| setup, end to end (`time`) | **595.98 s** |

Rerun with nothing removed: `real 0m1.164s`.

**Capi and contract diff.** `oxpinyin-capi` and `oxpinyin-zhuyin-capi` debug
builds, one target directory per cell (`--no-default-features --features
bdb|kyotocabinet|tkrzw`), then `tools/bisection/run-contract-diff.sh` with
`--cases` set to the 26 `issue784-index-keys-*` cases (pinyin and zhuyin),
after `source ~/.oxpinyin-oracle.env`:

| Cell | Capi build (`oxpinyin-capi`) | `issue784-index-keys-*` |
| --- | --- | --- |
| bdb | 24.3 s | 26 of 26 `MATCH` |
| kc | 20.0 s | 26 of 26 `MATCH` |
| tkrzw | 21.1 s | 26 of 26 `MATCH` |

`ldd` on the tkrzw `libpinyin_capi.so` resolves `libtkrzw.so.1` to
`~/.cache/oxpinyin/tkrzw/lib/libtkrzw.so.1`; the bdb and kc ones resolve
`libdb-5.3.so` and `libkyotocabinet.so.16` from `/lib/x86_64-linux-gnu`.
`issue785` has no cases of its own in `contract-diff.py`; its mask behaviour
is inside the 784 mask cases.

### With the published image, from scratch

Measured 2026-10-16 UTC on the same 4-core cloud host as above, after the
session's network policy allowed `ghcr.io` and
`pkg-containers.githubusercontent.com`. The container was restarted, so
`db5.3-util` and `kyotocabinet-utils` were still installed from the earlier
run (no install step), and `~/.cache/oxpinyin` and `/var/lib/docker` were
emptied before the run, so everything else started from nothing.

The published oracle image **was pulled** (`ghcr.io/shenghaoc/oxpinyin-oracle:074a2219c90f-…`),
and all three cells' libraries matched `oracle-pin.txt`, so no library was
built. The tkrzw source build still ran (the host library failed the probe),
and the `deps` image was still built, because `--regen-data` needs it.

`SECONDS` offsets from the stderr of `bash tools/cloud/setup.sh`:

| Step | Seconds |
| --- | --- |
| `dockerd` start and pinned-digest pull, two retries (`1` to `17`) | 16 |
| published image pull, three cells copied out, hashes match (`17` to `21`) | 4 |
| tkrzw: probe, source build, install, probe, `ldd` (`21` to `86`) | 65 |
| `deps` image build (starts at `89`) and model20 fetch, inside the bdb data step (`86` to `244`) | 158 for the step |
| `--regen-data` kc, tkrzw (`244` to `359`, `359` to `441`) | 115, 82 |
| setup, end to end (`time`) | **440.71 s** |

Against the 595.98 s build-only run above, the image saves the three
library builds (173 s there) and adds a 4 s pull; the other steps differ
between runs by amounts the two runs do not separate.

### When the published image cannot be pulled

`setup.sh` keeps the pull's stderr in `<cache>/pull-published.err`, logs its
last five lines, and says which case it is before building the libraries:
`manifest unknown / not found` (the tag is not published) or `pull FAILED
(network, auth or proxy, not "not found")` for anything else. Both fall
back to the build.

### Not measured here

Disk use in this session and whether `podman info` or `docker info` works
in a Codex cloud environment were not measured.

## Gates in a cloud session

Everything in `AGENTS.md` ("Verification cells") runs as on any Linux
host; the cells are the three prefixes the setup prints. The Rust toolchain
(`rust-toolchain.toml`) is the one the session already carries, and
`cargo`, `valgrind` and `cc` are present, so same-run callgrind
measurements (`benches.md`) need nothing more than the oracle for the pin
side. A main-against-main control (two builds of one commit, measured in
one run, spread 0) needs only the Rust side.

## Failure modes

- `429 Too Many Requests` from the registry: the script retries; rerun it.
- `403` from the proxy: the host is not allowed for the environment. Do
  not route around it; add the host (table above) to Allowed domains.
- `MISMATCH` and exit 3: the libraries built here do not hash to
  `oracle-pin.txt`. They are left in the oracle root; compare them with
  `readelf -S -W` and `cmp -l` against a reference build and report the
  difference. Do not edit the recorded values to match.
