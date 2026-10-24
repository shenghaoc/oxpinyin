# Language/runtime discrepancy record — template

Status: **current template** (policy, 2026-10-24 UTC; #915/#916) · Not
itself a register entry. Canonical policy:
`docs/findings/compatibility-policy.md`, "The language/runtime-quirk
exception". Owner issue: #915.

Use this record for a proposed difference from the pinned libpinyin that
is a GLib, glibc or C++ language/runtime quirk rather than class (a)
math, (b) memory safety or (c) availability. One record per independent
semantic difference, named `docs/findings/<issue-or-topic>-<slug>.md`.
The record is linked from the register entry in
`docs/findings/upstream-divergences.md` and from the contract-diff case.

The record must be complete before code changes. It is not a place to
record a result discovered by running the oracle; derive the pin's
behaviour from its source (and its disassembly where the source does not
settle it).

## Record

```markdown
# <record id> — <short name>

- **Status:** proposed | accepted | superseded · <YYYY-MM-DD> UTC · issue #N · PR #N
- **Disposition:** POLICY-ACCEPTED (never MATCH)
- **Policy limb:** language/runtime quirk. Why not (a), (b) or (c):
  <one or two sentences>.
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
- **Why accepted:** <why this is an incidental runtime quirk and not an
  application semantic; why no valid input changes>.
- **Contract preserved:** C ABI/layout; ownership and lifetime (real
  `GArray`, `g_free`, other GLib-owned structures); every affected and
  unaffected export; on-disk system/user files; data ownership;
  process-wide side effects; diagnostics. State each as unchanged or
  name the documented difference.
- **Valid-input proof:** source citations showing the quirk path cannot
  produce a valid-input difference, plus the differential controls that
  exercise ordinary valid input.
- **Tests:** contract-diff case IDs and expected disposition
  (`MATCH` or `POLICY-ACCEPTED`); unit/contract tests; malformed-input
  witnesses; controls. Name the existing driver; a new driver needs the
  reason no existing one can take the cases.
- **Prior rulings:** none reversed. If one is involved, STOP and cite
  it.
- **Evidence:** commands that produced the figures; no captures are
  committed. Say where the capture environment is ephemeral.
- **Conclusion:** accepted | retained as normal parity | needs a
  ruling. Reviewer and date.
```

## Rules the record enforces

1. **The contract is never the quirk.** If the difference changes the C
   ABI, ownership, an on-disk format, data ownership or valid-input
   behaviour, it is not eligible. Record `NEEDS-RULING` and stop.
2. **Trace both directions.** A forwards-only trace does not establish
   that another producer cannot reach the same observable.
3. **Partition the input.** "Malformed only" is a claim to prove, not a
   label to apply. The record names the exact bytes or state that reach
   the difference.
4. **Never call it MATCH.** The case keeps its pin and subject evidence
   and carries the `POLICY-ACCEPTED` disposition. Deleting the case or
   relabelling it erases the record's evidence.
5. **One record, one difference.** Shared mechanism does not merge
   independent observable behaviours into one record.
6. **No dependency for aesthetics.** Replacing a GLib helper with normal
   Rust is the point; adding a crate to avoid a documented difference is
   a dependency change and stops for approval.
