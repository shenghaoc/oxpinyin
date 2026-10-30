# Design — Stage 3: opt-in departures from the pin

## Candidates

Each candidate changes output or files and therefore requires its own cargo feature under Requirement 1.

- Compile the text model during data preparation.
  This changes system data files and targets init time, RAM and disk together (direction decided 2026-09).
- Move the system bigram from the backend's `bigram.db` into a compiled blob.
  This changes system data files.
- Upgrade the model with trigram or Kneser-Ney smoothing, typo edges, or oxpinyin's own data.
  The EdgeKind candidates are Fuzzy, Typo and Abbrev.
  These change output.

## Not Stage 3

Compatible internal work from the 2026-09 direction remains [Stage 2](../bug-for-bug/design.md#internal-improvement):

- Dictionary cursor walks.
- Incremental per-keystroke lattices.
- Per-session arenas.
- Removing ordered atomics from the steady cycle.
- Host-independent cost derivation when output stays identical.

A language artefact also remains Stage 2 under Requirement 6.
Record it instead of requiring opt-in.
