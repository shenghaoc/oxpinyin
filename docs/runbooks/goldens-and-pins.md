# Goldens and pins — refreshing what is frozen

Three different things are "frozen" here, with three different
procedures. All three are STOP-class under the [approval conditions](../../CONTRIBUTING.md#approval-conditions): an ask, a measured
differential, and a maintainer ruling recorded in the findings.

## 1. Committed fixtures and goldens

| what | where | regenerate with |
| --- | --- | --- |
| the mini data set, per backend | `fixtures/w3/<ext>/` | `just w3-fixtures generate --model-dir <model20 extraction>` (`tools/dev/w3_fixtures.py`): the Python subset of the model20 text, then `oxpinyin-utils`' `gen_binary_files` / `import_interpolation` / `gen_unigram` / `gen_deleted_ngram` built for each backend. `just w3-fixtures check` regenerates into a temporary directory and compares records and chunk bytes with the committed tree; `fixtures/w3/README.md` has the details |
| W4 oracle goldens (candidate structure, sentence surface, paths, tie order) | `fixtures/w4/oracle-*.txt` | no regenerator today, except the tie order (`contract_sentence_ties.py`); see [the next pin capture](#the-next-pin-capture). `oracle-paths.txt` came from W2 logs that no longer exist and stays frozen |
| parity corpus inputs | `tests/parity/corpus/inputs/` | frozen input files; a change is a deliberate edit, with the digests in [the corpus record](../testing/parity-corpus.md) updated with it |
| trainer manifests and goldens | `fixtures/w9/` (`segmenter-han.txt`, `segmenter-spseg-w3.txt`, `mergeseq-input.txt`, `segmenter-mergeseq-w3.txt`) | `oxpinyin-utils`' segmentation tests compare against these; regenerate with the pin tool the test names and commit only when it agrees |

A missing golden is a test failure, never a skip (#373). Regenerate,
diff, and commit the golden in the same change as the code that moved
it, with the before/after numbers in the message.

### The next pin capture

The Rust capture programs were deleted on 2026-11-06 UTC, and Python is the
capture side. Each of these needs a Python regenerator in the next pin capture,
alongside the [user-profile interchange captures](oracle.md#user-profile-interchange).
The behaviours the live C drivers in `tools/bisection/` still cover alone are observations
of the [pin capture](pin-captures.md). None of these regenerators exists yet:

| capture | file |
| --- | --- |
| candidate structure | `fixtures/w4/oracle-candidate-structure.txt` |
| sentence surface | `fixtures/w4/oracle-sentence-surface.txt` |
| F-A | `fixtures/foundation/f-a.txt` |
| F-C | `fixtures/foundation/f-c.txt` |
| ZH | `fixtures/w4/oracle-zhuyin-parse.txt` |

The live utility differentials in `crates/oxpinyin-utils/tests/` were deleted on
2026-11-07 UTC. The frozen utility cases cover what they compared, except for the
observations below. Each runs the pin-built utility in a private copy of
`fixtures/w3/<ext>/` on the bdb, kc and tkrzw cells, as the `utility/<cell>/kmm-import-unigram`,
`utility/<cell>/gen-ngram` and `utility/<cell>/gen-deleted-ngram` observations of the
[pin capture](pin-captures.md):

| capture | records | becomes |
| --- | --- | --- |
| KMM import, 1-gram only | `import_k_mixture_model` with no arguments and this stdin: `\data model "k mixture model" count 3 N 1 total_freq 3`, `\1-gram`, `\item 1 <start> count 2 freq 0`, `\item 16817937 中国 count 1 freq 1`, `\item 16802309 你好 count 0 freq 2`, `\end`, each line ending in `\n`; the exit status, stdout, stderr and whether the directory tree changed | the frozen case `kmm_import_unigram_only` in `kmm_tools.rs` |
| training bigram rows | `gen_ngram` with no arguments, writing the default `bigram.db`, and `gen_deleted_ngram --deleted-bigram-file deleted.db`, each with stdin `16777219 阿`, `16777220 啊`, an empty line, `16777219 阿`; for each file, which of `START` (1), `阿` (16777219) and `啊` (16777220) has a row at all, and for each present row its total and every successor with its count. An absent row is recorded as absent, not as a zero-valued row: on this input `啊` has no row in `deleted.db` | row assertions in the frozen `gen_ngram` and `gen_deleted_ngram` cases in `training.rs` |

These contract cases are registered for the bdb cell only and hold no frozen pin
observation in `fixtures/contract-refs/bdb/` and no policy digest. Each is captured on the
bdb cell through `tools/oracle/pin_capture.py` staging and then converted to a Rust test:

| case | family |
| --- | --- |
| `removed-predict-stale-skip-pinyin` | `journal_replay` |

The six apostrophe inputs `'`, `''`, `'''`, `ni'`, `ni'!` and `ni'i` have their own
capture step, which stages proposed observations for review, and native C-API tests in
the two capi crates: see [pin captures](pin-captures.md).
`oracle-paths.txt` excludes all six, but core's `graph_paths` tests retain the
deleted `live_sentinels` tests' recorded expectations as native regressions:
apostrophe-only inputs consume every byte without key edges; the other three
consume three bytes and admit `ni@0:2:complete` (the recorded pin segments were
`ni@0:2:complete,<missing-pinyin>@2`). No new pin run was used.

## 2. The frozen parity numbers

The candidate surface is asserted by `oxpinyin-runtime`'s `candidate_surface`
test: all 97,442 captured rows over 10,037 inputs, identical to the pin. It reads
the pinned libpinyin-data package (`just fetch-contract-data`). The sentence-surface
residual (496 1-best, 492 distinct-set and 491 ordered of 500) is asserted by the
`sentence_surface_parity` test, over the same package, and recorded in
`docs/findings/sentence-surface.md` §12. A change that moves any of them:

1. Run those tests and capture the new numbers.
2. Classify the delta under `compatibility-policy.md`; a change toward
   the pin is a closure, a change away needs a class or a revert.
3. Ask. The re-freeze is the maintainer's ruling, recorded as an
   amendment in the finding with the commit that moved it.
4. Update the asserted numbers in the same commit as the ruling.

## 3. The oracle pin

`tools/oracle/oracle-pin.txt` plus the constants in
`tools/oracle/build-oracle.sh` (libpinyin commit, ibus-libpinyin commit,
model SHA-256, DBM) are one identity; change every field together. The
2026-09-06 move to 074a2219 is the worked example
(`oracle-pin-074a221-verification`,
`docs/testing/oracle-environment.md`, "Amendment — pin 074a2219"):

1. Bump the pin fields; rebuild the oracle (`oracle.md`); the build
   verifies the fetched commit by `git rev-parse`.
2. Re-run the W2 candidate surface (must stay byte-identical unless the
   upstream change is the point), the sentence-surface counts, the
   `_check_offset` and abort families, and `just runner same-data-dir-diff`.
3. Record what moved, what did not, and what is build-nondeterministic
   (the six DBM-backed data files are; issue #358) in a verification
   finding; amend `oracle-environment.md`.
4. The drop-in identity (`libpinyin-<version>` header dir, `.pc` version)
   moves with the pin: the capi builds read `libpinyin_tag` from
   `oracle-pin.txt` and refuse a `[package.metadata.capi]` table that
   disagrees, so the bump edits both capi `Cargo.toml` files in the same
   commit; then run `just packaging check-pc-metadata` against the
   rebuilt pin (issue #537).

Re-run the pins after any rebase that touches engine, capi or data
([rebase procedure](../../CONTRIBUTING.md#checkpoints-and-rebases)); whoever merges later re-runs them.

2026-09-27: #612 corrects oxpinyin-generated `RECORDS_GOLDEN`’s `.dbin` MODIFY token from `0x01000001` to local slot `0x00000001`; pin `074a2219` (`phrase_index.cpp:394–442`) reproduces all 62 payload bytes with only offset 25 changing `01` → `00` (maintainer-approved).
