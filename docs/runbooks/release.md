# Release — cutting one and what the pipeline gates

This is the procedure to cut a release.
Publishing a GitHub release runs `.github/workflows/release-packages.yml`.
[Release artifacts](../packaging.md#release-artifacts-release-packagesyml) states
what each distribution lane builds and gates; [`docs/packaging.md`](../packaging.md)
is the design record. The Debian lane checks the runtime and development packages with lintian’s md5sums tags before installing them. Every pull request to `main` runs the build, install and gates with the dry-run version and stops before upload.

## Before tagging

Each of these conditions holds before a release is tagged:

1. `main` is green on `CI`, and on every job of the last
   `verify-nightly`.
2. `verify-nightly.yml` was dispatched manually on the release commit ([verification rule 10](verification.md#10-ci-merge-conditions)).
   A release is published only if its macOS, Windows, fuzz soak and overflow lane jobs pass on
   that commit; a green run of another commit does not count.
3. The ABI gate passed on the tip. The Linux sanitizer job runs `just abi` to check exports, C++ header consumers and the allocation registers.
4. The frozen pins are unchanged since the last verified run, or they are re-verified
   ([goldens and pins](goldens-and-pins.md)).
5. The [version streams](../packaging.md#the-four-version-streams) agree:
   the drop-in identity is the libpinyin version of the pin, and the crate
   version is the `0.x` lockstep of the workspace.

## The Cargo pre-release

The crates.io pre-release is a separate step from the distro release:

1. Merge the release commit to `main`.
2. From the merged commit, run `cargo publish --dry-run -p oxpinyin -p oxpinyin-capi -p oxzhuyin-capi`.
3. When the dry run passes, run `cargo publish -p oxpinyin -p oxpinyin-capi -p oxzhuyin-capi` from the same commit.

Tag the commit from which you published the crates and publish a GitHub release for that tag.
Mark the release as a pre-release if the tag has a pre-release suffix.
`release-packages.yml` builds and attaches the distro packages for every release.

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
