# Release — cutting one and what the pipeline gates

`docs/packaging.md` is the design record (cargo-c, the installed tree,
the four version streams, the distro recipes). This is the procedure.

## Trigger

Publishing a GitHub release (a stable tag) runs
`.github/workflows/release-packages.yml`, which builds drop-in packages
for Debian (tkrzw), Fedora (kyotocabinet) and Arch (kyotocabinet) — each
under the backend whose format that distro's own libpinyin data uses —
and attaches them with a `SHA256SUMS` to the release.

## Before tagging

1. `main` green on `CI`, `Store backends` and the last `verify-nightly`.
2. The ABI export gate passed on the tip (`tools/abi/check-exports.sh
   --shipped`; CI runs it on every change).
3. The allocator-pairing gate passed on the tip
   (`tools/abi/check-alloc-pairing.sh`; CI runs it on every change).
   Linux only — `--static-only` is the portable half.
4. Frozen pins unchanged since the last measured run, or re-measured
   (`goldens-and-pins.md`).
5. Version streams agree (`docs/packaging.md`, "The four version
   streams"): the drop-in identity stays `2.11.91` until upstream tags a
   release; the crate version is the workspace's `0.x` lockstep.

## What each lane gates

`tools/packaging/release-stage.sh <backend>` builds both cdylibs through
`tools/packaging/install.sh` (`cargo cinstall` plus the complete `.pc`),
`--features shipped` on the pinyin crate, strips, and exits 0 only after:
the SONAME, the five pkg-config reads real consumers perform, and a C
compile/link/run smoke against the staged tree. The workflow then
installs the package in a fresh container and runs `pinyin_init` /
`zhuyin_init` against the distro's own data — a null context fails the
lane, because a package whose init fails against the system data must
not ship.

## Locally

```sh
tools/packaging/release-stage.sh tkrzw --prefix=/usr --libdir=/usr/lib/x86_64-linux-gnu \
    --dest=target/release-stage/tkrzw
tools/packaging/release-deb.sh  tkrzw <version> target/release-stage/tkrzw /usr/lib/x86_64-linux-gnu <outdir>
tools/packaging/release-rpm.sh  kyotocabinet <version> target/release-stage/kyotocabinet <outdir>
tools/packaging/release-arch.sh kyotocabinet <version> target/release-stage/kyotocabinet <outdir> --data=<arch data dir> --data-version=<ver>  # without --data the package cannot init on the system it replaced
```

Each maker's header states its arguments; the workflow is the reference
invocation.

The staged tree is the complete install (runtime + dev); the makers
split it the way each distro splits libpinyin.

## After

Confirm the release assets and checksums, then record anything the
lanes surfaced (a distro data change, a new package dependency) in
`docs/findings/tkrzw-distro-compat.md` or `docs/packaging.md`.
