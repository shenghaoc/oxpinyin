# Release — cutting one and what the pipeline gates

This is the procedure to cut a release.
[`docs/packaging.md`](../packaging.md) is the design record: cargo-c, the installed tree,
the four version streams and the distro recipes.
See [Installing uv and cargo-c](setup.md#installing-uv-and-cargo-c) for host setup.

## Trigger

When you publish a GitHub release (a stable tag), `.github/workflows/release-packages.yml` runs.
It builds drop-in packages for Debian (tkrzw), Fedora (kyotocabinet) and Arch (kyotocabinet).
Each package uses the backend whose format the libpinyin data of that distro uses.
The workflow attaches the packages and a `SHA256SUMS` to the release.

## Before tagging

Make sure that each condition holds before you tag:

1. `main` is green on `CI` and `Store backends`, and on every job of the last
   `verify-nightly`.
2. Dispatch `verify-nightly.yml` manually on the release commit with
   `tools_harness` set. Publish only if its macOS, Windows, fuzz soak and
   overflow lane jobs pass on that commit; a green run of another commit
   does not count. For the merge rule, see
   [verification rule 10](verification.md#10-ci-merge-conditions).
3. The ABI export gate passed on the tip (`just abi exports
   --shipped`; CI runs it on every change).
4. The allocator-pairing gate passed on the tip
   (`just abi alloc-pairing`; CI runs it on every change).
   This gate is Linux only; `--static-only` is the portable half.
5. The frozen pins are unchanged since the last verified run, or they are re-verified
   ([goldens and pins](goldens-and-pins.md)).
6. The version streams agree ([the four version
   streams](../packaging.md#the-four-version-streams)):
   - the drop-in identity is the libpinyin version of the pin
     (`just packaging pc-version`; CI's `python -m tools.dev.packaging
     check-pc-version` holds the manifests to it);
   - the crate version is the `0.x` lockstep of the workspace.

## What each lane gates

`just packaging release-stage <backend>` does these steps:

1. It builds both cdylibs through
   `just packaging install` (`cargo cinstall` plus the complete `.pc`),
   with `--features shipped` on the pinyin crate.
2. It strips the libraries.
3. It exits 0 only after these checks pass: the SONAME, the five pkg-config reads that real consumers perform, and a C
   compile/link/run smoke test against the staged tree.

The workflow then
installs the package in a fresh container and runs `pinyin_init` /
`zhuyin_init` against the data of the distro. A null context fails the
lane, because a package whose init fails against the system data must
not ship.
Before the install, the Debian lane runs lintian's md5sums tags on both
`.deb` files. After the install, each lane requires mode 0755 on the two
installed shared libraries.

A pull request that changes a packaging path runs the same workflow as a
dry run, with the version 0.0.0. The dry run builds, installs and gates
the packages of the three lanes. It resolves the release repository
through gh, as the upload does, and it stops before the upload.

## Locally

```sh
just packaging release-stage tkrzw --prefix=/usr --libdir=/usr/lib/x86_64-linux-gnu \
    --dest="$PWD/target/release-stage/tkrzw"
just packaging release-deb  tkrzw <version> target/release-stage/tkrzw /usr/lib/x86_64-linux-gnu <outdir>
just packaging release-rpm  kyotocabinet <version> target/release-stage/kyotocabinet <outdir>
just packaging release-arch kyotocabinet <version> target/release-stage/kyotocabinet <outdir> --data=<arch data dir> --data-version=<ver>  # without --data the package cannot init on the system it replaced
```

`tools/dev/packaging.py` states the arguments of each maker. The workflow is the
reference invocation.

The staged tree is the complete install (runtime + dev). The makers
split it in the same way that each distro splits libpinyin.

## After

Confirm the release assets and checksums. Then record anything that the
lanes found (a distro data change, a new package dependency) in
`docs/findings/tkrzw-distro-compat.md` or `docs/packaging.md`.
