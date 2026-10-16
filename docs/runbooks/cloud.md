# Cloud sessions — provisioning a Claude Code cloud host

A Claude Code cloud session is a supported host for every gate in this
repository (`AGENTS.md`). It is a root container with Docker installed
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
`--cache-dir DIR` (oracle prefixes and the model cache; default
`~/.cache/oxpinyin`), `--cells`, `--jobs`, `--no-pull` (build the
libraries even if the published image exists). Progress goes to stderr;
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
1. Starts `dockerd` detached (`setsid nohup dockerd`) unless
   `docker info` already answers, and waits for it. When it starts the
   daemon it first installs `/root/.ccr/agent-proxy-ca.crt` into the
   system trust store, so the daemon's own pulls trust the session proxy.
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
`/dev/vda`, `docker` 29.8.2). No oracle image is published yet, so only
the build path ("without the published image") was measured; the "with"
column is not measured and is not estimated.

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
