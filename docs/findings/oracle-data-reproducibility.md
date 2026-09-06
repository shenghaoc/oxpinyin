# Oracle data reproducibility

Status: recorded 2026-09-06 (issue #358). Upstream report-back candidate.

## Observation

A determinism control — two clean builds of libpinyin 2.11.91
(`0c5e80e1200f84fab185d1c5bde458b770a0636c`) in separate work dirs, the
same model20 export, the same container image, an unmodified
`tools/oracle/build-oracle.sh` — shows that 6 of the 23 files
`make install` ships to `lib/libpinyin/data` are **not reproducible at a
fixed pin**:

- `addon_phrase_index.bin`
- `addon_pinyin_index.bin`
- `bigram.db`
- `phrase_index.bin`
- `pinyin_index.bin`
- `punct.bin`

The other 17 — the domain phrase tables and the gb/gbk character tables —
are byte-identical across clean builds. The 6 are exactly the files
produced through libpinyin's DBM-backed generation path (`gen_binary_files`
and `gen_unigram`/`gen_ngram` importing through the DBM backend), so the
variance is upstream's, not the recipe's.

## Consequence for the harness

Before this note, `oracle-data.sha256` covered all 23 files, so its digest
(`data_manifest_sha256` in `oracle-pin.txt`) differed between two builds of
the *same* pin. Anything comparing a fresh prefix's manifest against a
recorded one failed for reasons that were not a pin change, and any
between-pins byte diff on those 6 files could not be attributed to the pin
change — the 074a221 verification had to withdraw exactly such a delta as
build noise.

## Resolution

The build recipe splits the payload manifest:

- `oracle-data.sha256` — the **reproducible gate**: the 17 stable files.
  Its digest, `data_manifest_sha256`, is a pure function of the pin and is
  the only data digest comparable across prefixes.
- `oracle-data-unstable.sha256` — the **informational** manifest: the 6
  files above, digest recorded as `data_unstable_manifest_sha256`. It keeps
  the audit trail and the tamper-evidence `run-capture.sh` relies on
  (both manifests are re-checked against the prefix before a capture), but
  it does not pretend to reproducibility and nothing compares it across
  builds. The harness accepts a manifest without the field, so a prefix
  built before the split still verifies.

The unstable list is fixed in `build-oracle.sh` (`DATA_UNSTABLE_FILES`);
a file that moves from one class to the other is a recipe change, made
deliberately, not something the split infers per build.

## Upstream angle

libpinyin's generated data is not reproducible, which matters for distro
reproducible-builds work. The affected files are those written through the
DBM path; the flat tables produced without it are stable. To be reported
back with the rest of the rewrite's findings.
