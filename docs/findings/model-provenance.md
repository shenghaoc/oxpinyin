# Model and table provenance

## 2026-11-10 UTC ruling for issue #1764

The project may commit and redistribute the minimal W3 test subset derived
from model20 under GPL-3.0-or-later. Distributors must comply with the
licence, including its notice and corresponding source requirements. This
ruling replaces the 2026-08-14 redistribution prohibition only for the
files in this scope.

### Scope

- Model-derived records in `fixtures/w3/{db,kct,tkt}/`: the chunk files,
  the index files, `bigram.db` and `punct.bin`.
- Expected-output files that reproduce those records:
  `crates/oxpinyin-utils/tests/fixtures/export_interpolation/w3-model-*.txt`.

All other files are outside this ruling. The full model20 archive remains
subject to the build-time fetch policy.

### Evidence

- At libpinyin commit `074a2219`, `EXTRA_DIST` includes
  `interpolation2.text` and the 17 `.table` files through
  `textual_model_data`.
  [Pinned data/Makefile.am](https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/data/Makefile.am)
- Upstream author Peng Wu published `libpinyin-2.11.92.tar.gz`
  (20,636,687 bytes). The release asset contains all 18 files in `data/`.
  [Upstream release](https://github.com/libpinyin/libpinyin/releases/tag/v2.11.92)
- Debian's libpinyin 2.11.92-1 source package in `main` contains
  `data/interpolation2.text` and the same 17 tables. Its
  `debian/copyright` has two `Files` stanzas: `*` and `debian/*`.
  The `*` stanza lists Peng Wu (2006–2024) and James Su (2002, 2003,
  2006) under `GPL-3+`. The file has no separate `data/` stanza or
  `Files-Excluded` field.
  [Debian source descriptor](https://deb.debian.org/debian/pool/main/libp/libpinyin/libpinyin_2.11.92-1.dsc),
  [Debian packaging source](https://deb.debian.org/debian/pool/main/libp/libpinyin/libpinyin_2.11.92-1.debian.tar.xz)
- Fedora lists `libpinyin-data` under GPL-3.0-or-later.
  [Fedora package metadata](https://packages.fedoraproject.org/pkgs/libpinyin/libpinyin-data/)

### Remaining uncertainty

No rightsholder statement covers the training corpus behind the counts.
The provenance and licence terms of that corpus remain unresolved.

## 2026-08-14 archive inspection and build-time fetch ruling

The original ruling prohibited redistribution and permitted a build-time
fetch of the archive pinned by `model_archive_sha256` in
[`tools/oracle/oracle-pin.txt`](../../tools/oracle/oracle-pin.txt).
The inspection record follows.

## Examined artefacts

The reference data is `model20.text.tar.gz`, downloaded from the
[libpinyin SourceForge model directory](https://sourceforge.net/projects/libpinyin/files/models/)
and pinned as:

- URL: `https://downloads.sourceforge.net/libpinyin/models/model20.text.tar.gz`
- SHA-256: `59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155`
- Source listing date: 2024-09-27

A checksum-verified inspection found `interpolation2.text` and seventeen
`.table` files. The archive contains no `COPYING`, `LICENSE`, `NOTICE`,
README, copyright statement, provenance manifest, or source-corpus record.
The archive inventory is:

```text
art.table
culture.table
economy.table
gb_char.table
gbk_char.table
geology.table
history.table
interpolation2.text
life.table
merged.table
nature.table
opengram.table
people.table
punct.table
science.table
society.table
sport.table
technology.table
```

The pinned libpinyin source carries a
[GPL licence text](https://raw.githubusercontent.com/libpinyin/libpinyin/074a2219c90feaf962d0d24f034514033ece5f99/COPYING).
That notice sets terms for the source release. The model archive is
published separately. It does not say that those terms cover its trained
model, tables, source corpora or generated contents. The SourceForge
project-level licence label also does not identify the copyright holders or
grant terms for each data input.

## Redistribution status

| Artefact | Project status |
|---|---|
| Full model20 archive, `interpolation2.text` and its 17 `.table` files | Redistribution prohibited |
| Model-derived W3 records and expected outputs in the scope above | Redistribution permitted under GPL-3.0-or-later |
| Other converted, compiled or derived model20 data | Redistribution prohibited |

## Redistribution vs build-time fetch

Redistribution includes putting data in the repository, a release archive,
a package or an installer. Apply the status table to each of those routes.

For a build-time fetch, the user obtains the archive directly from the
upstream SourceForge models directory. The project supplies the URL and
the checksum. The upstream build uses the same fetch method. The build
must report a download or checksum error and stop if the fetch fails.

## Branch declaration

The project routes considered here are:

- **Branch A:** redistribute the pinned libpinyin model and tables.
- **Branch A′:** replace them with the Android PinyinIME raw dictionary and
  derive any additional data under documented, compatible terms.
- **Branch B:** build tables and models only from inputs whose provenance and
  redistribution terms are recorded by this project.

**Branch A remains rejected for the full model and tables.** The W3
ruling permits only the listed test files.

**Branch B is recorded as an optional future route, not a shipping prerequisite.** It is
the path to a model that this project could *vendor*. The project needs it only
if it ever wants such a model. Its mechanism is the
[libpinyin trainer](https://github.com/libpinyin/trainer): GPL-2.0
build-time tooling that produces the `interpolation2.text`-format model from
a training corpus. This route moves the licensing question to the training
corpus that the project would supply. That question is deferred until the
project pursues Branch B.

**Branch A′ is not selected as the complete route.** The AOSP PinyinIME tree
carries an Apache-2.0
[NOTICE](https://android.googlesource.com/platform/packages/inputmethods/PinyinIME/+/refs/heads/master/NOTICE)
next to
[`rawdict_utf16_65105_freq.txt`](https://android.googlesource.com/platform/packages/inputmethods/PinyinIME/+/refs/heads/master/jni/data/rawdict_utf16_65105_freq.txt).
This finding did not verify whether or how that notice covers the raw
dictionary. The dictionary is not a provenance record and does not replace
the pinned language model. It may be evaluated later as one documented
Branch B input. This finding does not approve a derived model or bundled
table from it.

## Shipping consequence

- Stage 1 may use the checksum-pinned external archive locally and as
  input to the oracle.
- The W3 ruling permits redistribution only for the listed test files.
  Builds that need the full model fetch the archive from upstream.
- Shipping the reimplementation, including Stage 2, can use the build-time
  fetch route. Branch B remains an optional route to a replacement model.
- A future provenance change must assess the model and the tables
  separately.
