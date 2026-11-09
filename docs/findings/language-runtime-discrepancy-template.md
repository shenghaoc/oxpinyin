# Language/runtime discrepancy record — template

Status: **current template** (policy; #915/#916, #1303) · Not itself a
register entry. Canonical policy:
[compatibility policy, "Internal language artefacts"](compatibility-policy.md#internal-language-artefacts)
and [dispositions and discrepancy records](compatibility-policy.md#dispositions-and-discrepancy-records).
Owner issue: #915.

Use this record for a GLib, glibc or C++ language/runtime quirk under
[decision-order limb 3](compatibility-policy.md#decision-order).
It is neither class (a) maths nor class (b) memory safety. No approval is needed.

- Write one record for each independent semantic difference.
- Name it `docs/findings/<issue-or-topic>-<slug>.md`.
- Link it from the register entry in `docs/findings/upstream-divergences.md`.
- Name its cases in the record.

The record must be complete before code changes. Do not use it to record a
result found by running the oracle. Derive the pin's behaviour from its
source, and from its disassembly where the source does not settle it.

## Record

```markdown
# <record id> — <short name>

- **Status:** proposed | accepted | superseded · <YYYY-MM-DD> UTC · issue #N · PR #N
- **Disposition:** POLICY-ACCEPTED (never MATCH)
- **Policy limb:** language artefact. Why not (a) or (b), and why
  a language-neutral requirement would not mention it: <one or two
  sentences>.
- **Pin:** libpinyin 2.11.92 at 074a2219; tree read: <checkout path,
  or clone plus blob hashes>; GLib/libc version of the reference image:
  <version>; facade: pinyin | zhuyin | both; backend: bdb | kc | tkrzw |
  all.
- **Pin derivation:** `path:line` (and symbol/instruction offsets where
  disassembly was read).
- **Rust counterpart:** `path`, module/item, C entry point.
- **Forward trace:** C entry -> validation -> parse/decode -> lookup or
  store operation -> candidate/sentence/export/save -> out-params and
  diagnostics.
- **Backward trace:** every producer of the observable, including
  alternate paths, bypasses, both facades and the three backends; name
  what is *not* reachable and why.
- **Input partition:**
  - Valid supported input: <definition and examples>
  - Malformed / non-scalar / host-dependent input: <definition and
    examples>
  - Other (empty, over-long, crafted file): <definition>
- **Difference:** exact triggering input; pin behaviour; oxpinyin
  behaviour; the language/runtime mechanism that causes it.
- **Why accepted:** <the language or library behaviour that causes the
  difference, and the idiomatic Rust that replaces it>.
- **Contract preserved:** C ABI/layout; ownership and lifetime (real
  `GArray`, `g_free`, other GLib-owned structures); every affected and
  unaffected export; on-disk system/user files; data ownership;
  process-wide side effects; diagnostics. State each as unchanged or
  name the documented difference.
- **Affected inputs:** the class of valid or malformed input the
  artefact reaches, characterised as precisely as the source permits,
  with representative pin and oxpinyin results and controls showing
  that product behaviour elsewhere is unchanged.
- **Cases:** the case IDs and their disposition (`MATCH` or
  `POLICY-ACCEPTED`); the record lists the `POLICY-ACCEPTED` cases.
- **Tests:** the Rust tests that hold the accepted behaviour and its
  controls, by crate, target and test name; malformed-input witnesses.
- **Prior rulings:** none reversed. If one is involved, STOP and cite
  it.
- **Evidence:** the pin citation, the exact inputs, the pin and oxpinyin
  observations, the `POLICY-ACCEPTED` cases and the Rust tests above. No
  captures or logs are committed.
- **Conclusion:** accepted | retained as normal parity | needs a
  ruling. Reviewer and date.
```

## Rules the record enforces

1. **The contract is never the quirk.** If the difference changes the C
   ABI, ownership, an on-disk format or data ownership, it is not
   eligible. If it changes product behaviour (a ranking rule, a limit, a
   logic defect), it is not a language artefact. Record `NEEDS-RULING` and stop.
2. **Trace both directions.** A forwards-only trace does not establish
   that another producer cannot reach the same observable.
3. **Partition the input.** "Malformed only" is a claim to prove, not a
   label to apply. The record names the exact bytes or state that reach
   the difference.
4. **Never call it MATCH.** The case keeps its pin and subject evidence
   and has the `POLICY-ACCEPTED` disposition. If you delete or relabel the
   case, you erase the evidence of the record.
5. **One record, one difference.** A shared mechanism does not merge
   independent observable behaviours into one record.
6. **No dependency for appearance.** The purpose is to replace a GLib helper
   with normal Rust. To add a crate that avoids a documented difference is
   a dependency change, and it stops for approval.

## Tests of an accepted case

The record states the pin's and oxpinyin's results and their source
derivation. The test that holds oxpinyin's side names the record in a
comment, with the case's original ID and the pin source lines. Where the
pin is not run (`pin_not_run`, class (b)), the comment says so, and the
expectation is the source-derived result.
