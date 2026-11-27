# Session replay SPEC

Status: **reference**

The replay scenarios are authored, not captured, so they are not a compatibility contract ([approval conditions](../../CONTRIBUTING.md#approval-conditions)).
What binds is the pin behaviour that they check: every expected candidate is one that the pin emitted.
The scenarios, the harness, the test names and the CI jobs are reference and change with the code.

Session replay asserts the results of recorded input sequences through the session API.

## What this stands in for

Fixture family **F-D** ("guess → choose → re-guess remainder") has no capture, so these scenarios are **authored**.
Their expectations are not authored.
Every candidate asserted here is one that the pinned oracle emitted, taken from the candidate lists in `fixtures/foundation/f-a.txt` and limited to what the mini vocabulary can reach.

## Harness

The `session_replay` module of `src/engine/session/selection/tests.rs` holds the scenarios inline and reads no scenario file; the robustness tests are in the `session_replay` module of `src/engine/session/tests.rs`.
It opens each session with `Session::new` under the tests' option word: incomplete pinyin and both split tables.
The dictionary and the language model are fixture doubles over `fixtures/w4/mini-vocab.txt` and `fixtures/w4/mini-bigram.txt`.

| Operation | Use in the test |
|---|---|
| `replace_raw` | supplies the whole raw input as text |
| `candidates` | gives the list that the test checks by index and text |
| `select` | chooses by index and returns `Selection::Continued` or `Selection::Completed` |
| `selected_text`, `composition_offset`, `raw_input` | give the retained text, the consumed byte count and the raw input |
| `reset`, `is_composing` | discard the composition and confirm that nothing is composing |

The session has no key, preedit or commit operation, so no scenario uses one.

## Scenarios

The test `recorded_candidate_scenarios_use_direct_operations` runs two scenarios, each in a new session.

| Input | What it holds |
|---|---|
| `nihaozhongguo` | choose `你好`, decode the remainder `zhongguo`, choose `中国` |
| `zhuan` | the F-E-12 trigger: choose `传` and reset |

`candidates_come_from_every_segmentation_not_only_the_selected_one`, `an_initial_only_tail_reaches_the_phrases_it_prefixes`, and `an_apostrophe_is_a_boundary_the_decoder_honours` hold the candidate construction cases. `an_input_no_phrase_can_reach_falls_back_to_itself` holds the raw fallback.

## Robustness tests

The test doc comments state each upstream defect. F-E-12 is an `assert()` on the user input `zhuan` (ibus-libpinyin #542).

- **F-E-12**, the `zhuan` row of `recorded_candidate_scenarios_use_direct_operations` and `reset_clears_the_composition_but_not_the_configuration`.
  The row runs the full frontend-equivalent sequence, not a parse in isolation: it parses `zhuan`, takes `传`, reads the retained text and resets.
  `reset_clears_the_composition_but_not_the_configuration` (the `session_lifecycle` module of `src/engine/session/tests.rs`) continues on a session after a choice and a reset.

The test `arbitrary_characters_never_panic` appends every character in `U+0000..=U+00FF`, then `你` and `U+10FFFF`, and calls `replace_raw` after each one.
It requires no panic and no error: the totality property at session level.

## Portability

The harness discovers no path, reads no environment, uses no clock, and contains no `cfg(target_os)`.
Nothing beneath it does these things either.
CI runs it on Linux, macOS and Windows:

- the PR `test` job runs the workspace tests, this harness and the graph differential on Linux;
- the nightly `test-windows` and `test-macos` jobs (`verify-nightly.yml`) run them on Windows and macOS.
