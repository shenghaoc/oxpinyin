# Goldens and pins — refreshing what is frozen

This page gives the procedures that refresh frozen content.
Three different kinds of content are frozen here, and each has its own procedure.
A refresh of any of the three needs the maintainer's approval under the [approval conditions](../../CONTRIBUTING.md#approval-conditions).
Each needs a request to the maintainer, a measured differential, and a maintainer ruling recorded in the findings.

## 1. Committed fixtures and goldens

| what | where | regenerate with |
| --- | --- | --- |
| the mini data set, per backend | `fixtures/w3/<ext>/` | frozen output of the model subset and the pin program sequence; see `fixtures/w3/README.md` |
| W4 oracle goldens (candidate structure, sentence surface, paths, tie order) | `fixtures/w4/oracle-*.txt` | no regenerator; see [the next pin capture](#the-next-pin-capture). `oracle-paths.txt` has no source to regenerate from and stays frozen |
| trainer manifests and goldens | `fixtures/w9/` (`segmenter-han.txt`, `segmenter-spseg-w3.txt`, `mergeseq-input.txt`) | the segmentation tests of `oxpinyin-utils` compare against these; regenerate with the pin tool that the test names, and commit only when it agrees |

A missing golden is a test failure, never a skip ([test inputs](../testing/README.md#test-inputs)). To refresh a golden:

1. Regenerate it.
2. Diff it.
3. Commit the golden in the same change as the code that moved
   it, with the before and after numbers in the message.

### The next pin capture

Each capture below needs a Python regenerator in the next pin capture,
together with the [user-profile interchange captures](oracle.md#user-profile-interchange).
None of these regenerators exists yet:

| capture | file |
| --- | --- |
| candidate structure | `fixtures/w4/oracle-candidate-structure.txt` |
| sentence surface | `fixtures/w4/oracle-sentence-surface.txt` |
| F-A | `fixtures/foundation/f-a.txt` |
| F-C | `fixtures/foundation/f-c.txt` |
| ZH | `fixtures/w4/oracle-zhuyin-parse.txt` |

The cases below are registered for the bdb cell only. They hold no frozen pin observation
and no policy digest. Each case is captured on the bdb cell through
`tools/oracle/pin_capture.py` staging and then converted to a C-boundary test in the
`src` test modules of the facade crate:

| case | facade |
| --- | --- |
| `removed-predict-stale-skip-pinyin` | pinyin |

## 2. The frozen parity numbers

These are the parity numbers. The tests of `oxpinyin-runtime` assert them over
the pinned libpinyin-data package (`just fetch-contract-data`):

- `candidate_surface` (`candidates_match_the_frozen_capture`): the candidate surface
  is identical to the pin on all 4,572 rows over 750 inputs of
  `fixtures/w4/oracle-candidate-structure.txt`, and the header totals agree.
- `sentence_surface_parity` (`sentence_surface_matches_the_declared_residual`): over the
  500 comparable inputs of `fixtures/w4/oracle-sentence-surface.txt`, 496 1-best,
  492 distinct-set and 491 ordered-list matches, 9 declared residuals and no
  order-only residual. `docs/findings/sentence-surface.md` §12 records them.

For a change that moves any of these numbers:

1. Run those tests and capture the new numbers.
2. Classify the delta under `compatibility-policy.md`. A change toward
   the pin is a closure; a change away from the pin needs a class or a revert.
3. Ask the maintainer. The re-freeze is the ruling of the maintainer, recorded as an
   amendment in the finding with the commit that moved it.
4. Update the asserted numbers in the same commit as the ruling.

## 3. The oracle pin

This is the only procedure to change the oracle pin.
[`tools/oracle/oracle-pin.txt`](../../tools/oracle/oracle-pin.txt) is the
authoritative pin record (libpinyin version and commit, ibus-libpinyin tag
and commit, model SHA-256, DBM). The builder and host model fetcher both read
the model archive digest there. A change to the record changes the recipe hash
and invalidates prefixes from the old recipe.
[Pin identity](oracle.md#pin-identity) explains each field. Every
field of one pin move changes together, in one commit:

1. Edit the pin fields in `oracle-pin.txt`. Rebuild
   every cell from an empty prefix ([oracle](oracle.md#2-build-the-oracle)).
   The build verifies each fetched commit by `git rev-parse`.
2. Run these checks again ([oracle](oracle.md#the-c-boundary-tests)):
   - the [parity numbers](#2-the-frozen-parity-numbers) (the candidate surface must stay
     byte-identical unless the upstream change is the reason for the bump);
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
   Then run `just packaging check-pc-metadata` against the rebuilt pin.

A changed C-boundary expectation is derived from the source diff of the pin.
A [pin capture](pin-captures.md) observation is used only where the source does not settle the result;
`pin_capture.py` is the only capture tool. The frozen utility cases have no recapture tooling:
a pin bump must provide and document that step, with repeat-run stability checks and a review of
architecture, OS and policy variants ([utility cases](tool-diff.md#pin-bumps-and-recapture)).

After any rebase that touches engine, capi or data, the pins run again
([verification scope and precedence](verification.md#verification-scope-and-precedence)).
The person who merges later runs them again.

A conflict-free rebase does not verify a changed generation scheme: a dependency can change
manifest layouts, fixture formats or builder recipes while old generated artifacts survive the
textual rebase. Record the base with `git merge-base HEAD origin/main` before rebasing.
After rebasing, list the committed artifacts whose generators changed between the recorded base and the new base on main, regenerate them, and re-verify under
[verification scope and precedence](verification.md#verification-scope-and-precedence).
