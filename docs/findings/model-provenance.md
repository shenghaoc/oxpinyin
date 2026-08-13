# Model and table provenance

Date: 2026-08-14 · Decision: **no redistribution — build-time fetch permitted** (Branch B: optional vendoring route, not a shipping gate)

This finding records the project shipping decision for the data pinned by
`docs/findings/oracle-environment.md`. It is a conservative redistribution
classification for this project, not a legal opinion, and it draws the line
between redistribution and build-time fetch.

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
[GPL licence text](https://raw.githubusercontent.com/libpinyin/libpinyin/2.11.91/COPYING).
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

## Redistribution vs build-time fetch

Two distinct acts must not be conflated:

- **Redistributing** the model — placing `interpolation2.text`, any listed
  `.table` file, or a converted/compiled/derived form of either inside this
  repository, a release archive, a package, or an installer — requires a
  grant this project does not have, and remains prohibited.
- **Fetching** the checksum-pinned archive from its upstream origin at build
  time, on the user's machine, does not place this project in the
  redistribution chain. The user obtains the data from the same SourceForge
  models directory that upstream libpinyin directs them to; the project ships
  code plus a URL and a checksum, not data, and is not the distributor in
  that path.

Upstream practice is consistency evidence, not proof. libpinyin (pinned tag
2.11.91) [downloads `model20.text.tar.gz` from its SourceForge models
directory](https://sourceforge.net/projects/libpinyin/files/models/) during
its own build rather than vendoring the archive, and a build-time fetch in
the Rust reimplementation mirrors that mechanism exactly. This project
classifies the act the same conservative way: the project ships code plus a
URL and a checksum, not data. Upstream's own build carrying the identical
mechanism supports that classification by consistency — were the mechanism
itself redistribution, upstream would sit under the same exposure — but
upstream's practice is not by itself dispositive; this file is a conservative
policy, not a legal opinion.

## Branch declaration

The project routes considered here are:

- **Branch A:** redistribute the pinned libpinyin model and tables.
- **Branch A′:** replace them with the Android PinyinIME raw dictionary and
  derive any additional data under documented, compatible terms.
- **Branch B:** build tables and models only from inputs whose provenance and
  redistribution terms are recorded by this project.

**Branch A is rejected** because neither the model nor the tables have an
artefact-specific licence and provenance chain. **Branch B is recorded as an
optional future route, not a shipping prerequisite**: it is the path to a
model this project could *vendor*, and it is only needed if the project ever
wants one. Its mechanism is the
[libpinyin trainer](https://github.com/libpinyin/trainer) (GPL-2.0, build-time
tooling that produces the `interpolation2.text`-format model from a training
corpus); taking it relocates the licensing question to the training corpus the
project would supply, a question deferred until/unless Branch B is pursued.
Branch A′ is not selected as the complete route: the AOSP PinyinIME tree
carries an Apache-2.0
[NOTICE](https://android.googlesource.com/platform/packages/inputmethods/PinyinIME/+/refs/heads/master/NOTICE)
next to
[`rawdict_utf16_65105_freq.txt`](https://android.googlesource.com/platform/packages/inputmethods/PinyinIME/+/refs/heads/master/jni/data/rawdict_utf16_65105_freq.txt);
whether and how that notice covers the raw dictionary was not verified for
this finding, and the dictionary is not a provenance record or replacement
for the pinned language model. It may be evaluated later as one documented
Branch B input; this finding does not approve a derived model or bundled
table from it.

## Shipping consequence

- Stage 1 may read the checksum-pinned external archive for local use and may
  use it as the non-shipping oracle subject.
- The repository, release archives, packages, installers, and generated
  shipping artefacts must not contain `interpolation2.text`, any listed
  `.table` file, or converted/compiled derivatives of them.
- Shipping the reimplementation — including Stage 2 — is gated on exactly
  that: not vendoring the model or its derivatives, which a build-time fetch
  of the checksum-pinned archive satisfies. It is **not** gated on Branch B
  producing a replacement model. The code ships under GPL-3.0-or-later (this
  project's declared license; libpinyin's licence is GPL-compatible); the
  model is fetched from upstream at build time; no model bytes are
  redistributed by this project.
- A build-time download makes the model a build-time-fetched dependency: the
  build must fail cleanly on an unreachable URL, a moved artifact, or a
  checksum mismatch. This is the same operational fragility upstream's own
  build carries; it is an engineering consequence, not a licensing one.
- A future provenance change must identify model and tables separately; a
  licence conclusion for one class must not be inferred for the other.
