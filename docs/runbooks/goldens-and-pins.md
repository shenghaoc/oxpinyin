# Goldens and pins — refreshing what is frozen

Three different things are "frozen" here, with three different
procedures. All three are STOP-class under AGENTS.md: an ask, a measured
differential, and a maintainer ruling recorded in the findings.

## 1. Committed fixtures and goldens

| what | where | regenerate with |
| --- | --- | --- |
| the mini data set, per backend | `fixtures/w3/<backend>/` | `oxpinyin-datagen compile --mini --backend <b> --model-dir $PINYIN_MODEL_DIR --out-dir fixtures/w3/<b>`; `fixtures_identity` (ignored) proves the compile reproduces the tree byte for byte |
| W4 oracle goldens (candidates, structure, paths) | `fixtures/w4/oracle-*.txt` | the `pinyin-oracle` bins `oracle-candidates` (~3 h, full corpus), `oracle-candidate-structure`, `oracle_sentence_surface` (~1 min); the ignored `*_fixture_is_fresh` tests re-derive and compare |
| trainer manifests and goldens | `crates/oxpinyin-{counter,emitter,lambda,segment}/tests/fixtures/` | the ignored differential tests print the Rust side; commit only after the live pin tool agrees |

A missing golden is a test failure, never a skip (#373). Regenerate,
diff, and commit the golden in the same change as the code that moved
it, with the before/after numbers in the message.

## 2. The frozen parity numbers

The candidate surface (top-1, top-5-set, prefix-10, absent, order-only)
and the sentence-surface residual (491/396/390 of 496, re-frozen
2026-09-04) are asserted by `pinyin-oracle`'s parity tests and recorded
in `docs/findings/pin-refreeze-2026-08.md` and
`docs/findings/sentence-surface.md` §12. A change that moves any of them:

1. Run the parity suite against the oracle (`oracle.md`) and capture the
   new numbers.
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
(`docs/findings/oracle-pin-074a221-verification.md`,
`docs/testing/oracle-environment.md`, "Amendment — pin 074a2219"):

1. Bump the pin fields; rebuild the oracle (`oracle.md`); the build
   verifies the fetched commit by `git rev-parse`.
2. Re-run the W2 candidate surface (must stay byte-identical unless the
   upstream change is the point), the sentence-surface counts, the
   `_check_offset` and abort families, and `run-same-data-dir-diff.sh`.
3. Record what moved, what did not, and what is build-nondeterministic
   (the six DBM-backed data files are; issue #358) in a verification
   finding; amend `oracle-environment.md`.
4. The drop-in identity (`libpinyin-2.11.91` header dir, `.pc` version)
   moves only when upstream tags a release, never with the pin.

Re-run the pins after any rebase that touches engine, capi or data
(AGENTS.md, "Rebase discipline"); whoever merges later re-measures.
