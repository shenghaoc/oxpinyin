# Language/runtime discrepancy record — template

Status: **current template** (policy, 2026-10-24 UTC; #915/#916;
amended 2026-10-26 UTC, #1303) · Not
itself a register entry. Canonical policy:
`docs/findings/compatibility-policy.md`, "The language/runtime-quirk
exception". Owner issue: #915.

Use this record for a proposed difference from the pinned libpinyin that
is a GLib, glibc or C++ language/runtime quirk rather than class (a)
math, (b) memory safety or (c) availability: a behaviour of another
language or of a library written for one, which oxpinyin does not
reproduce (decision-order limb 3, 2026-10-26 UTC). No approval is
needed. One record per independent
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
- **Policy limb:** language artefact. Why not (a), (b) or (c), and why
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
   ABI, ownership, an on-disk format or data ownership, it is not
   eligible. If it changes product behaviour (a ranking rule, a limit, a
   logic defect), it is not an artefact: record `NEEDS-RULING` and stop.
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

## Existing driver's case marker (#894)

Register an ordinary surviving case with `policy_record="<record>.md"`
in `tools/bisection/contract-diff.py`. The filename is relative to
`docs/findings`; the register must link it. The record contains exactly
one `contract-diff-policy` fenced JSON block, which `just policy-record`
writes from the differential's `--observations` capture:

```json
{
  "schema": 2,
  "record": "<record>",
  "status": "current",
  "disposition": "POLICY-ACCEPTED",
  "cases": {
    "<exact case ID>": {
      "cells": {
        "bdb": {
          "pin": {"sha256": "<64 hex>", "fields": {"exit": "<16 hex>", "result.logs": "<16 hex>", "result.ret": "<16 hex>", "stderr": "<16 hex>"}},
          "subject": {"sha256": "<64 hex>", "fields": {"exit": "<16 hex>", "result.logs": "<16 hex>", "result.ret": "<16 hex>", "stderr": "<16 hex>"}},
          "divergence": ["result.ret: pin true; subject false"]
        }
      }
    }
  }
}
```

Derive the complete observations from source before executing the case,
then record the capture:

```sh
just runner contract-diff bdb "$PINYIN_ORACLE_PREFIX" <capi.so> <zhuyin.so> -- --cases <case> --observations obs.jsonl
just policy-record docs/findings/<record>.md obs.jsonl
```

The schema-3 block holds no complete observation. For each case and cell,
`accepted` names exactly the fields whose stored pin and subject digests differ.
The fields are top-level observation members, except a result document is split
into its individual members. For each accepted field, each side must match its
stored 16-hex SHA-256 digest, or its recorded absence. Every other field must
be equal between the current pin and subject, including its presence and value
type; it need not match the historical digest. The complete-observation hashes
and other field digests are retained for diagnosis and do not decide the
verdict. This applies also when every field is accepted. An observation must
still satisfy the driver's validity rules. An unexpected pair is DIFFER, even
under `--expect-parent`; a missing, unregistered, superseded or malformed
record is INVALID. A marked case never becomes MATCH merely through equality.

Digests use canonical JSON (sorted keys, no spaces, ASCII escapes), including
surrogateescaped invalid UTF-8. Divergence text describes the recorded pair,
with bounded previews and a 4,096-byte serialized limit. An entry under `cells`
applies to that cell; `database_free` applies to every cell. Use `just
policy-record --migrate` to derive schema 3 from schema 2's stored field
digests without a new capture, or record a legitimate changed accepted pair
from a capture.
The accepted case reports its record and both observations, and contributes
only to the separate POLICY-ACCEPTED count. An existing `abort=` case may
retain its pin SIGABRT with a null result and exact stderr; the subject
must terminate normally. A different pin signal remains invalid. Other
crash and host-dependent markers cannot be combined with this marker;
those cases keep their existing class-specific comparison rules.
