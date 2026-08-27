# Native model20 data production — the canonical-source invariant

Date: 2026-08-27 · Status: **recorded / implemented** (`crates/oxpinyin-datagen`)

## The invariant

> The canonical linguistic source is the source of truth. No oxpinyin
> backend may require libpinyin-generated runtime data as its input.

The canonical source is the pinned `model20.text.tar.gz`
(`docs/findings/model-provenance.md`; SHA-256
`59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155`,
fetched and verified by `tools/model/fetch-model.sh`). Every runtime-data
producer consumes that archive directly:

```text
                     pinned model20.text.tar.gz
                                 │
        ┌────────────────────────┼────────────────────────┐
        │                        │                        │
        ▼                        ▼                        ▼
  libpinyin's own build   oxpinyin-datagen redb    oxpinyin-datagen lmdb
  (data/Makefile.am)      (also lmdb, tkrzw)              │
        │                        │                        ▼
        ▼                        │                 oxpinyin-datagen tkrzw
  libpinyin tables              ▼                        ▼
  (Tkrzw .db/.bin)        redb tables ── LMDB tables ── Tkrzw tables
        │                        │        (key/value-identical across
        │                        │         backends, proven by test)
        ▼                        ▼
   libpinyin  ◄────── differential ──────►  oxpinyin
```

This replaces the retired `oxpinyin-migrate` route, which was the opposite
architecture: `export` drove the pin-built oracle's C ABI (consuming
libpinyin-compiled runtime data) and `convert` copied the oracle's
`bigram.db` verbatim (`docs/findings/data-layer-export.md`). That route
tested migration compatibility, not implementation parity. Its removal on
2026-08-23 left no in-tree producer, which parked the five differentials
that need a full system dir.

## The derivation

libpinyin's `data/Makefile.am` compiles the same archive with three tools;
`oxpinyin-datagen` reproduces the arithmetic natively:

| oxpinyin table | model20 source | upstream equivalent | match |
|---|---|---|---|
| `pinyin_index` | `gb_char/gbk_char/opengram/merged.table` rows `pinyin phrase token count` | `gen_binary_files` + `FacadePhraseIndex::load_text` + the ABI export iterator | byte-identical values |
| `phrase_index` | same rows, token column verbatim | same (tokens are never renumbered; `load_text` asserts the top byte) | byte-identical values |
| `bigram` | `\2-gram` section of `interpolation2.text`, grouped by first token, `total == Σ count` | `import_interpolation`'s `parse_bigram` against a fresh DB | byte-identical values |
| `addon_{4..15}_{pinyin,phrase}_index` | the twelve topic `.table` files | `gen_binary_files` over `get_addon_tables()` | restored from the removed `export-addon` |
| `punct` | `punct.table` | `gen_binary_files --gen-punct-table` | restored from the removed `export-punct` |

Details that make the equivalence exact (all measured on the pin):

- `pinyin_index` freq is the `.table` count column **verbatim**: the export
  iterator reads `PhraseItem::get_nth_pronunciation`'s per-pronunciation
  freq, and `import_interpolation`/`gen_unigram` only touch the separate
  phrase-index unigram field (`add_unigram_frequency` writes a different
  offset of the item), so the 1-gram additions and the +1 sweep are
  invisible to these tables. (oxpinyin's runtime loads its unigrams from
  `interpolation2.text` directly.)
- `load_text`'s one filter — pinyin must parse to exactly as many keys as
  the phrase has characters — drops **0** rows in the pin; same-pinyin
  duplicate rows of one token sum (0 occurrences); tokens never regroup
  (0 occurrences). The compiler enforces all three as errors rather than
  silently diverging.
- `interpolation2.text` `(token, word)` pairs validate against the
  system tables for all but the 49,737 `<start>` references (token top
  byte 0 — the special-token table upstream, accepted as-is); any real
  mismatch is a compile error, not a drop.

## Equivalence proof (measured 2026-08-27)

Against the frozen oracle-derived export at `/tmp/oxpinyin-export`
(produced by the retired ABI/convert route, the artifact the parked
differentials used to consume):

- `pinyin_index`: **93,349** keys, all entries identical
- `phrase_index`: **138,096** tokens, all entries identical
- `bigram`: **56,359** entries / **1,849,609** successor records, all
  identical

The `--mini` subset reproduces the committed `fixtures/w3/` tables
row-for-row through the store API. Raw container bytes of the frozen
fixtures are a property of the writing redb version (4.1.0 then, 4.2.0 in
the lockfile now); the committed files stay pinned by
`fixtures/w3/fixtures.sha256`, and `datagen --mini` is the regeneration
recipe should they ever be re-frozen.

Automated as three tests in `crates/oxpinyin-datagen/tests/`:
`export_reference` (the frozen-export comparison; env-gated on
`OXPINYIN_DATAGEN_REF_DIR`), `fixtures_identity` (mini vs `fixtures/w3`),
`cross_backend` (all compiled-in backends emit identical key/value
streams, verified again through `oxpinyin-data`'s real loader). All
respect `OXPINYIN_DATAGEN_STRICT=1`: absent data is a failure, never a
skip.

**Verification is local-only, never CI.** The model20 archive is
non-redistributable (`docs/findings/model-provenance.md`) and lives
behind a flaky SourceForge mirror — neither belongs on a GitHub-hosted
runner, and the first CI attempt that fetched it failed on exactly that
flakiness. CI stays free of model data entirely. The local recipe, on
a provisioned machine:

```sh
tools/model/fetch-model.sh                      # SHA-verified extract
export PINYIN_MODEL_DIR="$PWD/target/model20/extracted"
OXPINYIN_DATAGEN_STRICT=1 cargo test -p oxpinyin-datagen            # redb
OXPINYIN_DATAGEN_STRICT=1 cargo test -p oxpinyin-datagen --features lmdb
sudo make install  # libtkrzw once, per .github history; then:
OXPINYIN_DATAGEN_STRICT=1 cargo test -p oxpinyin-datagen --features tkrzw
cargo run -p oxpinyin-datagen -- compile --out-dir target/datagen/redb
# sentence-surface parity over independently produced tables:
PINYIN_EXPORT_DIR="$PWD/target/datagen/redb" \
PINYIN_MODEL_DIR="$PWD/target/model20/extracted" \
cargo test -p pinyin-oracle --test sentence_surface_parity -- --nocapture
# a skip marker in that output means the data was absent — re-provision.
```

## Backend matrix

| Source | Backend | Producer | Output | Oracle comparison |
|---|---|---|---|---|
| model20 | redb | `oxpinyin-datagen compile --backend redb` | `*.redb` (engine's default backend; the full runtime path) | libpinyin, behavioral |
| model20 | LMDB | `oxpinyin-datagen compile --backend lmdb` | `*.lmdb` | libpinyin, via proven-identical tables |
| model20 | Tkrzw | `oxpinyin-datagen compile --backend tkrzw` | `*.tkt` | libpinyin, via proven-identical tables |

The LMDB/Tkrzw rows are proven at the store + loader level
(`cross_backend` plus the store crate's three-way ordering conformance):
their key/value streams are identical to redb's by test, so engine
behavior over them is identical by construction. Running the C ABI itself
over non-default backends would require switching `DefaultStore` (a
shipping-interface decision, deferred to Stage 2).

## Reproducibility

- Same archive (SHA-pinned) + same producer commit + same redb version →
  byte-identical redb tables. LMDB/Tkrzw outputs are key/value-identical
  by test; their container bytes are not a contract.
- Every compile writes `datagen-manifest.txt`:
  `schema`, `pin_ref=model20-<sha>`, `backend`,
  `producer=oxpinyin-datagen@<version>`, and per-table `records` +
  `fnv1a64` fingerprints. A differential run can name the exact
  source/producer/backend triple behind its data.
- Data production is separated from differential execution, and a
  skipped producer test cannot read as green — via the strict local
  recipe above, not via CI (see the policy note).

## The five parked differentials — restored

All five ran on 2026-08-27 against `oxpinyin-datagen` output (system dir =
compile out-dir; the four C-ABI runners also used the local pin-built
oracle):

| Differential | Data env | Result |
|---|---|---|
| `live-typing` | `LIVETYPING_SYSTEM` | **IDENTICAL** |
| `nbest-train` | `NBEST_CAPI_SYSTEM` | **IDENTICAL** |
| `train-dynamic-off` populated phase | `OPTION_SWEEP_CAPI_DATA` | **PASS** (candidate dumps engaged) |
| `sentence-surface` parity | `PINYIN_EXPORT_DIR` + model | **PASS** (frozen §12 residual) — local run; grep the output for the skip marker |
| `scheme` (double/bopomofo/full) | `W13_CAPI_SYSTEM` | runs; reports the **documented pre-existing divergence** |

The scheme result is the known tie-order class, not a data artifact: the
divergence sets are identical against the old oracle-derived export
(verified line-for-line on 2026-08-27), consistent with the tables being
byte-identical. This is the `docs/findings/sentence-surface.md` §6–§7/§12
residual (near-tie tail ordering from upstream's per-step `gfloat`
accumulation), pending the same maintainer freeze decision; it is reported
as the divergence it is, not masked.

## libpinyin capability map (data pipeline)

The acceptance lens for datagen is **libpinyin parity**, not crate
elegance: map every capability libpinyin provides to its oxpinyin
equivalent, reuse existing crates where their responsibility already
matches, and only add a crate where no existing boundary fits.

| libpinyin capability | where libpinyin puts it | oxpinyin equivalent | status |
|---|---|---|---|
| compile published `.table`s + `punct.table` into runtime format | build-time tool `utils/storage/gen_binary_files` (same source tree, not part of the shipped library) | `oxpinyin-datagen compile` (dict, addon, punct) | entry-for-entry identical to the frozen export |
| `interpolation2.text` → system bigram | build-time tool `utils/storage/import_interpolation` | same compile, bigram half | identical; strict validation instead of upstream's parse-abort |
| +1 unigram floor sweep | build-time tool `utils/training/gen_unigram` | no separate step | the sweep bumps a phrase-index unigram field that neither the exported tables nor oxpinyin's runtime reads (runtime unigrams load from `interpolation2.text` directly); every differential surface measures identical with the sweep absent — verified by measurement, not assumed |
| the data-make recipe | `data/Makefile.am` | `oxpinyin-datagen compile` (local) | equivalent, plus the provenance manifest |
| runtime λ source | `table.conf` | `oxpinyin-data::table_conf` + `PINNED_LAMBDA` | existing (W3) |
| train **new** models from a raw corpus | separate `libpinyin/trainer` repo (ngseg, gen_ngram, gen_deleted_ngram, estimate_interpolation, export_interpolation) | W9 crates: corpus / segment / counter / lambda / emitter | algorithmic parity on the legacy chain; KMM path deliberately out of scope (`docs/findings/training-algorithm.md`) |

**Crate placement, decided by that map.** libpinyin splits three ways —
runtime library (reads the compiled DB), build-time data tools (compiled
from the source tree, not shipped inside the library), and the trainer
(new-model derivation). oxpinyin mirrors each: `oxpinyin-data` (runtime
loader, ships), `oxpinyin-datagen` (build-time compiler, never ships),
W9 crates (trainer, never ships). The alternatives fail the
"where does libpinyin put it" test: `oxpinyin-store` ships via the engine
(the KV seam must not parse model text), `oxpinyin-dictool` ships, the
W9 crates own a different pipeline (corpus → new model, not published
model → tables), and `pinyin-oracle` is the differential harness. Reuse
went where responsibility matched: the three backends are
`oxpinyin-store`'s own `WriteStore` instantiations — one linguistic
model, three storage containers, zero algorithms duplicated — and the
model20 pin/discovery constants come from `pinyin-oracle`'s model cache
(no FFI). Likewise `oxpinyin-data::interp` (the runtime 1-gram reader)
was intentionally left 1-gram-only: extending a shipping crate's parser
for a build tool would be the coupling libpinyin also avoids
(`import_interpolation` parses the text; the library reads the DB).

The remaining engine-side gaps are the ones the workstreams already
track: the sentence-surface §12 tie-order residual (gfloat family,
freeze decision pending) and the trainer's KMM path. Nothing in the
data pipeline is outstanding.

## `oxpinyin-migrate` verdict

The removal stands. Its `convert`/`export` halves were migration
infrastructure by construction (inputs were libpinyin runtime artifacts),
so they cannot be part of this architecture; its `export-addon`/
`export-punct` halves were already model20-native and have been restored
verbatim inside `oxpinyin-datagen`. Nothing in the tree depends on the
crate; the frozen `fixtures/w3/` tables it produced remain the committed
mini baseline, now reproducible again via `datagen --mini`.
