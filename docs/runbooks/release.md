# Release — cutting one and what the pipeline gates

This is the procedure to cut a release.
Publishing a GitHub release (a stable tag) runs `.github/workflows/release-packages.yml`.
[Release artifacts](../packaging.md#release-artifacts-release-packagesyml) states
what each distribution lane builds and gates; [`docs/packaging.md`](../packaging.md)
is the design record. The Debian lane checks the runtime and development packages with lintian’s md5sums tags before installing them. Packaging pull requests run the build, install and gates with the dry-run version and stop before upload.

## Before tagging

Each of these conditions holds before a release is tagged:

1. `main` is green on `CI` and `Store backends`, and on every job of the last
   `verify-nightly`.
2. `verify-nightly.yml` was dispatched manually on the release commit with
   `tools_harness` set ([verification rule 10](verification.md#10-ci-merge-conditions)).
   A release is published only if its macOS, Windows, fuzz soak and overflow lane jobs pass on
   that commit; a green run of another commit does not count.
3. The ABI export gate passed on the tip (`just abi exports
   --shipped`; CI runs it on every change).
4. The allocator-pairing gate passed on the tip
   (`just abi alloc-pairing`; CI runs it on every change).
   This gate is Linux only; `--static-only` is the portable half.
5. The frozen pins are unchanged since the last verified run, or they are re-verified
   ([goldens and pins](goldens-and-pins.md)).
6. The [version streams](../packaging.md#the-four-version-streams) agree:
   the drop-in identity is the libpinyin version of the pin, and the crate
   version is the `0.x` lockstep of the workspace.

## Locally

```sh
just packaging release-stage tkrzw --prefix=/usr --libdir=/usr/lib/x86_64-linux-gnu \
    --dest="$PWD/target/release-stage/tkrzw"
just packaging release-deb  tkrzw <version> target/release-stage/tkrzw /usr/lib/x86_64-linux-gnu <outdir>
just packaging release-rpm  kyotocabinet <version> target/release-stage/kyotocabinet <outdir>
just packaging release-arch kyotocabinet <version> target/release-stage/kyotocabinet <outdir> --data=<arch data dir> --data-version=<ver>  # without --data the package cannot init on the system it replaced
```

`tools/dev/packaging.py` states the arguments of each maker. The workflow is the
reference invocation. The staged tree is the complete install (runtime + dev);
the makers split it in the same way that each distro splits libpinyin.

## After

Confirm the release assets and checksums. Then record anything that the
lanes found (a distro data change, a new package dependency) in
`docs/findings/tkrzw-distro-compat.md` or `docs/packaging.md`.
