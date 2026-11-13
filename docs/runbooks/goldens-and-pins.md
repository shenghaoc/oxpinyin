# Goldens and pins — refreshing what is frozen

This page gives the procedures that refresh frozen content.
Three different kinds of content are frozen here, and each has its own procedure.
All three are STOP-class under the [approval conditions](../../CONTRIBUTING.md#approval-conditions).
Each needs an ask, a measured differential, and a maintainer ruling recorded in the findings.

## 1. Committed fixtures and goldens

| what | where | regenerate with |
| --- | --- | --- |
| the mini data set, per backend | `fixtures/w3/<ext>/` | frozen output of the model subset and the pin program sequence; see `fixtures/w3/README.md`. The generation/check CLI is retired. |
| W4 oracle goldens (candidate structure, sentence surface, paths, tie order) | `fixtures/w4/oracle-*.txt` | no regenerator, except for the tie order ([archived contract sentence ties source](https://github.com/shenghaoc/oxpinyin/blob/1b91c898795b6edd8b92dd1fecf65ec6f4d1a434/tools/bisection/contract_sentence_ties.py)); see [the next pin capture](#the-next-pin-capture). `oracle-paths.txt` comes from W2 logs that no longer exist, and it stays frozen |
| trainer manifests and goldens | `fixtures/w9/` (`segmenter-han.txt`, `segmenter-spseg-w3.txt`, `mergeseq-input.txt`) | the segmentation tests of `oxpinyin-utils` compare against these; regenerate with the pin tool that the test names, and commit only when it agrees |

A missing golden is a test failure, never a skip (#373). To refresh a golden:

1. Regenerate it.
2. Diff it.
3. Commit the golden in the same change as the code that moved
   it, with the before and after numbers in the message.

### The next pin capture

The Rust capture programs are deleted; Python is the capture side. Each capture below needs a Python regenerator in the next pin capture,
together with the [user-profile interchange captures](oracle.md#user-profile-interchange).
The behaviours that only the retired C drivers in `tools/bisection/` covered are observations
of the [pin capture](pin-captures.md). None of these regenerators exists yet:

| capture | file |
| --- | --- |
| candidate structure | `fixtures/w4/oracle-candidate-structure.txt` |
| sentence surface | `fixtures/w4/oracle-sentence-surface.txt` |
| F-A | `fixtures/foundation/f-a.txt` |
| F-C | `fixtures/foundation/f-c.txt` |
| ZH | `fixtures/w4/oracle-zhuyin-parse.txt` |

The live utility differentials in `crates/oxpinyin-utils/tests/` are deleted. The frozen utility cases cover what they compared, except for the
observations below. Each observation runs the pin-built utility in a private copy of
`fixtures/w3/<ext>/` on the bdb, kc and tkrzw cells. They are the `utility/<cell>/kmm-import-unigram`,
`utility/<cell>/gen-ngram` and `utility/<cell>/gen-deleted-ngram` observations of the
[pin capture](pin-captures.md):

| capture | records | becomes |
| --- | --- | --- |
| KMM import, 1-gram only | `import_k_mixture_model` with no arguments and this stdin: `\data model "k mixture model" count 3 N 1 total_freq 3`, `\1-gram`, `\item 1 <start> count 2 freq 0`, `\item 16817937 中国 count 1 freq 1`, `\item 16802309 你好 count 0 freq 2`, `\end`, each line ending in `\n`; the exit status, stdout, stderr and whether the directory tree changed | the frozen case `kmm_import_unigram_only` in `kmm_tools.rs` |
| training bigram rows | `gen_ngram` with no arguments, writing the default `bigram.db`, and `gen_deleted_ngram --deleted-bigram-file deleted.db`, each with stdin `16777219 阿`, `16777220 啊`, an empty line, `16777219 阿`; for each file, which of `START` (1), `阿` (16777219) and `啊` (16777220) has a row at all, and for each present row its total and every successor with its count. An absent row is recorded as absent, not as a zero-valued row: on this input `啊` has no row in `deleted.db` | row assertions in the frozen `gen_ngram` and `gen_deleted_ngram` cases in `training.rs` |

The cases below are registered for the bdb cell only. They hold no frozen pin observation
and no policy digest. Each case is captured on the bdb cell through
`tools/oracle/pin_capture.py` staging and then converted to a C-boundary test in the
`src` test modules of the facade crate:

| case | facade |
| --- | --- |
| `removed-predict-stale-skip-pinyin` | pinyin |

The six apostrophe inputs `'`, `''`, `'''`, `ni'`, `ni'!` and `ni'i` have their own
capture step, which stages proposed observations for review. They also have native C-API tests in
the two capi crates: see [pin captures](pin-captures.md).
`oracle-paths.txt` excludes all six. The `graph_paths` tests of core keep the
recorded expectations of the deleted `live_sentinels` tests as native regressions:

- apostrophe-only inputs consume every byte without key edges;
- the other three inputs consume three bytes and admit `ni@0:2:complete` (the recorded pin segments were
  `ni@0:2:complete,<missing-pinyin>@2`).

No new pin run was used for these expectations.

## 2. The frozen parity numbers

The `candidate_surface` test of `oxpinyin-runtime` asserts the candidate surface:
all 4,572 captured rows over 750 inputs, identical to the pin. It reads
the pinned libpinyin-data package (`just fetch-contract-data`). The
`sentence_surface_parity` test asserts the sentence-surface
residual (496 1-best, 492 distinct-set and 491 ordered of 500) over the same package.
`docs/findings/sentence-surface.md` §12 records it. For a change that moves any of these numbers:

1. Run those tests and capture the new numbers.
2. Classify the delta under `compatibility-policy.md`. A change toward
   the pin is a closure; a change away from the pin needs a class or a revert.
3. Ask. The re-freeze is the ruling of the maintainer, recorded as an
   amendment in the finding with the commit that moved it.
4. Update the asserted numbers in the same commit as the ruling.

## 3. The oracle pin

This is the only procedure to change the oracle pin.
[`tools/oracle/oracle-pin.txt`](../../tools/oracle/oracle-pin.txt) is the
authoritative pin record (libpinyin version and commit, ibus-libpinyin tag
and commit, model SHA-256, DBM). The builder and host model fetcher both read
the model archive digest there. A change to the record changes the recipe hash
and invalidates prefixes from the old recipe.
[Pin identity](oracle.md#pin-identity) explains each field. Change every
field of one pin move together, in one commit:

1. Edit the pin fields in `oracle-pin.txt`. Rebuild
   every cell from an empty prefix ([oracle](oracle.md#2-build-the-oracle)).
   The build verifies each fetched commit by `git rev-parse`.
2. Run these checks again ([oracle](oracle.md#the-c-boundary-tests)):
   - the W2 candidate surface (it must stay byte-identical unless the
     upstream change is the reason for the bump);
   - the sentence-surface counts;
   - the `_check_offset` and abort families;
   - the C-boundary tests.
3. In the pull request, record what moved, what did not move, and what is
   build-nondeterministic. The six DBM-backed data files are
   build-nondeterministic at any pin
   ([regenerate the data](oracle.md#regenerate-the-data-after-every-pull)).
4. Move the drop-in identity (`libpinyin-<version>` header directory, `.pc`
   version) with the pin. `just packaging install` and
   `just packaging check-pc-version` read `libpinyin_tag` from
   `oracle-pin.txt` and refuse a `[package.metadata.capi]` table that
   disagrees, so edit both capi `Cargo.toml` files in the same commit.
   Then run `just packaging check-pc-metadata` against the rebuilt pin
   (issue #537).

After any rebase that touches engine, capi or data, run the pins again. The person who merges later runs them again.

A conflict-free rebase does not verify a changed generation scheme: a dependency can change
manifest layouts, fixture formats or builder recipes while old generated artifacts survive the
textual rebase. Record the base with `git merge-base HEAD origin/main` before rebasing.
After rebasing, list the committed artifacts whose generators changed between the recorded base and the new base on main, regenerate them, and re-verify under
[verification scope and precedence](verification.md#verification-scope-and-precedence).
