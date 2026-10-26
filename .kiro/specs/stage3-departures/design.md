# Design — Stage 3: opt-in departures from the pin

## Candidates

Each candidate changes observable output or the files on disk, so each
needs its own cargo feature (Requirement 1).

- **Compile the text model at data-prep time.** Changes the system data
  files. It attacks init time, RAM and disk at once (direction decided
  2026-09).
- **System bigram out of the backend's `bigram.db`** into a compiled
  blob. Changes the system data files.
- **Model upgrades:** trigram or Kneser-Ney smoothing, typo edges
  (`EdgeKind` Fuzzy/Typo/Abbrev), oxpinyin's own data. Change output.

## Not Stage 3

The internal items of the 2026-09 direction keep compatibility, so they
are Stage 2 work (`.kiro/specs/bug-for-bug/design.md`, "Internal
improvement"): the dictionary cursor walk, the incremental per-keystroke
lattice, the per-session arena, ordered atomics off the steady cycle,
and host-independent cost derivation if output stays identical. A
difference that is only a language artefact is also Stage 2: it is
recorded, not opted into (`.kiro/specs/bug-for-bug/`, Requirement 6).
