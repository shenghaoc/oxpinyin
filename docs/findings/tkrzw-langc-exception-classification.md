# Findings — tkrzw C-API exception classification divergence

Date: 2026-08-28 · Source tier: binding-migration audit (cxx → bindgen,
`feat/tkrzw-bindgen-migration`).
Status: **ruled accepted** (maintainer decision 2026-08-28). Registered as
a standing divergence — binding-ABI error-origin collapse — rather than a
defect, because it is externally observable in principle and fits none of
the previously accepted divergence buckets.

## What diverges

Through `tkrzw_langc.h`, every entry point catches every C++ exception
and reports it as `TKRZW_STATUS_SYSTEM_ERROR`. The retired cxx shim
caught the same exceptions at its own boundary and reported
`UNKNOWN_ERROR`. The store maps `SYSTEM_ERROR` to [`StoreError::Io`] and
every other code to [`StoreError::Backend`], so the binding switch
reclassifies the exception-origin error path:

| error origin | cxx shim (≤ `fea676f`) | bindgen C API |
| --- | --- | --- |
| C++ exception (allocation failure) | `UNKNOWN_ERROR` → `Backend` | `SYSTEM_ERROR` → `Io` |
| operating-system error | `SYSTEM_ERROR` → `Io` | `SYSTEM_ERROR` → `Io` (unchanged) |

- **Upstream cite:** `tkrzw_langc.cc` at 1.0.32 (upstream `bcaa0fb`): the
  `catch (const std::exception& e)` clause ending every wrapper — 135
  sites, e.g. lines 163–165 — each calling
  `tkrzw_set_last_status(TKRZW_STATUS_SYSTEM_ERROR, e.what())`. Old side:
  `crates/oxpinyin-store/src/tkrzw/shim.cc` at `fea676f:61-67`, whose
  `caught()` wrapped the same exceptions as
  `Status(UNKNOWN_ERROR, e.what())`.
- **Mechanism:** tkrzw's C++ core reports operating-system failures
  through `Status` returns, not exceptions; exceptions arise from
  allocation failure (`std::bad_alloc`, `std::length_error`) during
  container growth inside tkrzw or the wrapper's message copy. Both
  binding generations catch them; they disagree on the code they report.
- **What oxpinyin does instead:** nothing distinguishable — see below.
- **Reachability:** only under memory exhaustion. No test or differential
  corpus can produce the path without it.
- **Why exact preservation is impossible:** through the C ABI the two
  `SYSTEM_ERROR` origins share one code and one channel, and the message
  (`e.what()` vs an errno string) is not a reliable discriminator. The
  alternatives were ruled out: mapping `SYSTEM_ERROR` to `Backend`
  unconditionally preserves the exception class but regresses the
  classification of genuine I/O errors — an observed, branchable contract
  ([`lib.rs`](../../crates/oxpinyin-store/src/lib.rs) `StoreError`
  docs) — and keeping a C++ shim solely to re-synthesize `UNKNOWN_ERROR`
  would defeat the migration's single-binding-path goal.
- **Externally observable:** yes, in principle — a caller branching on
  `StoreError::Io` sees allocation-failure statuses after the migration
  that it would have seen as `Backend` before. Not observable by any
  current test or differential surface.

## Ruling and process note

STOP condition #2 of the migration task fired on this difference during
the Step 1 source audit; the migration nevertheless proceeded to a commit
on an agent materiality judgment ("only allocation failure, unobserved"),
which was not the agent's call to make. The 2026-08-28 maintainer decision
accepts the reclassification and requires this register entry, so the
class carries a paper trail instead of a quiet absorption.

## Upstream feedback (deferred)

Worth reporting to tkrzw: `UNKNOWN_ERROR` ("generic error whose cause is
unknown") arguably fits a caught exception better than `SYSTEM_ERROR`
("generic error from underlying systems") does in the langc wrappers. If
upstream ever changes the mapping, revisit this entry and the backend's
status mapping together.

[`StoreError::Io`]: ../../crates/oxpinyin-store/src/lib.rs
[`StoreError::Backend`]: ../../crates/oxpinyin-store/src/lib.rs
