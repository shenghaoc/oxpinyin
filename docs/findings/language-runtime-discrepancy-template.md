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

The block is part of the reviewed discrepancy record, but it holds no
observation. Each side keeps the SHA-256 of its complete stable observation
(exit, result with its logs, stderr in the driver's scratch-path
normalization, and userfiles for a user-directory case), a 16-hex digest per
top-level field for diagnosis, and the pin-to-subject divergence as text:
one line per moved field, with list edits in the pin's indices. The serialized
divergence field is limited to 4,096 UTF-8 bytes per case/backend pair,
including JSON escaping, indentation and an omission marker. Bounded previews
show affected paths, lengths and representative changes; rendering stops when
the budget is exhausted. Full observations still determine acceptance. An entry
escapes surrogate code points in values and paths for UTF-8 output while
keeping ordinary non-ASCII text. Digests use a single canonical JSON encoding
(sorted keys, no spaces, ASCII escapes), including invalid UTF-8 stderr bytes
preserved by the driver's `surrogateescape` decoding. An entry
under `cells` applies to that DBM cell; an entry without `cells` applies to
every cell. A POLICY-ACCEPTED verdict needs both complete observations to
digest to the recorded values: there are no wildcards, ignored `~` fields or
partial matches. Both sides must terminate normally (or the pin as a
registered abort) and match their recorded digests exactly; a mismatch is
DIFFER and the report names the side and the fields whose digests moved. A
missing, unregistered, superseded, schema-1 or malformed record is INVALID;
an unexpected observation is DIFFER and fails even under `--expect-parent`.
Equality cannot turn a marked case into MATCH. Record the case again with
`just policy-record` when its accepted observation legitimately changes; the
command replaces only that case's entry for the captured cell.
The accepted case reports its record and both observations, and contributes
only to the separate POLICY-ACCEPTED count. An existing `abort=` case may
retain its pin SIGABRT with a null result and exact stderr; the subject
must terminate normally. A different pin signal remains invalid. Other
crash and host-dependent markers cannot be combined with this marker;
those cases keep their existing class-specific comparison rules.
