# Model and table provenance

Date: 2026-08-09 · Decision: **Branch B**

This finding records the project shipping decision for the data pinned by
`docs/findings/oracle-environment.md`. It is a conservative redistribution
classification for this project, not a legal opinion.

## Examined artefacts

The reference data is `model20.text.tar.gz`, downloaded from the
[libpinyin SourceForge model directory](https://sourceforge.net/projects/libpinyin/files/models/)
and pinned as:

- URL: `https://downloads.sourceforge.net/libpinyin/models/model20.text.tar.gz`
- SHA-256: `59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155`
- Source listing date: 2024-09-27

A checksum-verified inspection found `interpolation2.text` and eighteen
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

The pinned libpinyin source tag carries a
[GPL-3.0 licence text](https://raw.githubusercontent.com/libpinyin/libpinyin/2.11.91/COPYING).
That notice establishes terms for the source release, but the separately
published model archive does not say that its trained model, tables, source
corpora, or generated contents are covered by those terms. The SourceForge
project-level licence label likewise does not identify the copyright holders
or grant terms for each data input.

## Redistribution status

| Artefact class | Evidence | Project status |
|---|---|---|
| `interpolation2.text` model | No embedded licence, copyright, training-corpus provenance, or model-generation manifest | **Not redistributable** |
| Eighteen `.table` files | No embedded licence, copyright, source-data provenance, or generation manifest | **Not redistributable** |
| Converted or compiled forms of either class | A mechanical conversion does not cure missing permission or provenance in its input | **Not redistributable** |

The required two-week fallback is therefore resolved conservatively now:
there is no need to wait for the deadline to treat inconclusive artefacts as
non-redistributable. New primary evidence from the relevant rightsholders can
reopen the finding in a dedicated provenance change, but absence of contrary
evidence does not change this decision.

## Branch declaration

The project routes considered here are:

- **Branch A:** redistribute the pinned libpinyin model and tables.
- **Branch A′:** replace them with the Android PinyinIME raw dictionary and
  derive any additional data under documented, compatible terms.
- **Branch B:** build tables and models only from inputs whose provenance and
  redistribution terms are recorded by this project.

**Branch B is selected.** Branch A is rejected because neither the model nor
the tables have an artefact-specific licence and provenance chain. Branch A′
is not selected as the complete route: AOSP publishes
[`rawdict_utf16_65105_freq.txt`](https://android.googlesource.com/platform/packages/inputmethods/PinyinIME/+/refs/heads/master/jni/data/rawdict_utf16_65105_freq.txt)
under the PinyinIME
[Apache-2.0 notice](https://android.googlesource.com/platform/packages/inputmethods/PinyinIME/+/refs/heads/master/NOTICE),
but that dictionary is not a provenance record or replacement for the pinned
language model. It may be evaluated later as one documented Branch B input;
this finding does not approve a derived model or bundled table from it.

## Shipping consequence

- Stage 1 may read the checksum-pinned external archive for local use and may
  use it as the non-shipping oracle subject.
- The repository, release archives, packages, installers, and generated
  shipping artefacts must not contain `interpolation2.text`, any listed
  `.table` file, or converted/compiled derivatives of them.
- Standalone data shipping and Stage 2 remain gated until Branch B produces a
  separately reviewed source manifest, licence record, reproducible build,
  and redistributable outputs.
- A future provenance change must identify model and tables separately; a
  licence conclusion for one class must not be inferred for the other.
