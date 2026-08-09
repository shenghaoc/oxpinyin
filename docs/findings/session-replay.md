# Session replay SPEC

Date: 2026-08-09 · Status: **frozen for W4-T4b**

Recorded `KeyInput` sequences driven through the session API, asserting what
comes out. It is the first cross-platform consumer of the seam
`session-api.md` froze, and it is what turns "the API is portable" from a
design claim into a run on three operating systems.

## What this stands in for

`spec-derivation.md` reserves family **F-D** for "guess → choose → re-guess
remainder", feeding the engine API and the capi templates. F-D was never
captured; Phase 0 produced F-A and F-C only.

These scenarios are therefore **authored**, and the file says so in its own
header. What is not authored is what they expect: every candidate asserted
here is one the pinned oracle really emitted, taken from the candidate lists
in `fixtures/foundation/f-a.txt`, restricted to what the mini vocabulary can
reach. So the scenarios are ours and the expectations are the pin's.

A real F-D capture would replace the file without changing the harness. The
scenario format below is what such a capture would have to produce.

## Format

`fixtures/w4/f-d-session.txt`, line-oriented UTF-8, TAB-separated `key=value`
fields — the shape used by every other fixture here. Blank lines and lines
starting with `#` are comments. A `scenario=` line starts a scenario; `step=`
lines belong to the scenario above them and run in order.

| Step | Fields | Meaning |
|---|---|---|
| `type` | `text` | one `process_key` per character |
| `key` | `name` | `enter`, `escape`, `space`, `backspace` |
| `select` | `index` | choose by index |
| `select-text` | `text` | choose the candidate with this text |
| `expect-preedit` | `text` | the whole preedit text |
| `expect-candidate` | `index`, `text` | that position holds that text |
| `expect-absent` | `text` | that text is not offered |
| `expect-empty` | — | nothing is composing |
| `commit` | `text` | commit, and assert what came out |

`select-text` exists because a candidate's *index* is a property of the
current ranking and a scenario that hard-codes one is asserting the ranking by
accident. Where the index is the point — F-E-02 — the scenario says `select`.

Every `expect-preedit` also checks the spans: contiguous, ascending, and
covering the text exactly. That invariant is easy to break and invisible from
the text alone.

## Scenarios

Nine, covering the F-D shape and the shapes W4 added:

| Scenario | What it holds |
|---|---|
| `guess-choose-commit` | the basic loop |
| `guess-choose-reguess` | choose a phrase, decode the remainder, choose again |
| `cross-segmentation-choice` | `xian` offers `西安` from a segmentation the pin did not select |
| `incomplete-tail` | `nih` reaches `你好` through an initial-only key |
| `apostrophe-boundary` | `chang'an` |
| `backspace-undoes-a-choice` | a selection is undone, not just the last letter |
| `space-accepts-the-first-candidate` | |
| `f-e-12-zhuan-totality` | the registered `zhuan` trigger, at session level |
| `unknown-input-falls-back` | `qqq` offers itself back |

Each is replayed twice in a fresh session and must give the same answer, which
is constitution item 6 at the level a shell sees.

## Registered robustness evidence made executable

`docs/findings/robustness-evidence.md` registers two rows against this lane.
Both now have the artifact it names:

- **F-E-02**, `f_e_02_candidate_replay`. Take a candidate snapshot,
  regenerate the list, then replay every stale index including `len`, `len+1`
  and `usize::MAX`. Upstream's defect is an unchecked candidate index; here
  every one is `EngineError::CandidateIndexOutOfRange`, and the session serves
  a second request afterwards. That last part is the claim — not that an index
  is rejected, but that rejecting it leaves the session usable.
- **F-E-12**, `f_e_12_zhuan_session_replay`. The register asks explicitly for
  the full frontend-equivalent sequence rather than a parse in isolation
  before any session-level claim is made: parse `zhuan`, take a candidate,
  commit, and keep going on the same session.

A third test feeds every character in `U+0000..=U+00FF` plus `你` and
`U+10FFFF`, then every logical key, and requires no panic and no error — the
totality property at session level.

## Portability

The harness discovers no path, reads no environment, uses no clock, and
contains no `cfg(target_os)`; neither does anything beneath it. CI runs it on
Linux, macOS and Windows.

The `test-portable` job covers `pinyin-core`, `pinyin-data`, `pinyin-user`,
`pinyin-engine` and `pinyin-oracle` — the last with its `oracle-ffi` feature
off, which is how the graph and decode-level differentials also run on all
three. `pinyin-capi` and `pinyin-migrate` stay Linux-first per
`.kiro/steering/structure.md` and are covered by the existing job.

Extending CI was an explicit maintainer decision: `AGENTS.md` lists editing CI
policy among the hard forbids without an ask, and this is the recorded ask.

## Acceptance

- Every scenario replays, twice, identically.
- F-E-02 and F-E-12 pass.
- All of it on Linux, macOS and Windows.
