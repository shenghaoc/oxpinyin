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

`crates/oxpinyin-engine/tests/session_replay.rs` holds the scenarios inline and reads no scenario file.
It opens each session with `Session::new` and `SessionSettings::default()`.
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

The test `recorded_candidate_scenarios_use_direct_operations` runs seven scenarios, each in a new session:

| Input | What it holds |
|---|---|
| `nihao` | the basic loop: first candidate `你好`, and `select(0)` completes the composition |
| `nihaozhongguo` | choose `你好`, decode the remainder `zhongguo`, choose `中国` |
| `xian` | offers `西安` first and `西岸` second |
| `nih` | reaches `你好` through an initial-only key |
| `chang'an` | the apostrophe boundary: `长安` |
| `zhuan` | the F-E-12 trigger: `传` |
| `qqq` | offers itself back and does not offer `你好` |

## Robustness tests

The test doc comments state each upstream defect.

- **F-E-12**, `f_e_12_zhuan_session_replay`.
  The test runs the full frontend-equivalent sequence, not a parse in isolation: it parses `zhuan`, takes a candidate, reads the retained text, resets, and continues on the same session.

A third test, `arbitrary_characters_never_panic`, appends every character in `U+0000..=U+00FF`, then `你` and `U+10FFFF`, and calls `replace_raw` after each one.
It requires no panic and no error: the totality property at session level.

## Portability

The harness discovers no path, reads no environment, uses no clock, and contains no `cfg(target_os)`.
Nothing beneath it does these things either.
CI runs it on Linux, macOS and Windows:

- the PR `test` job runs the workspace tests, this harness and the graph differential on Linux;
- the nightly `test-windows` and `test-macos` jobs (`verify-nightly.yml`) run them on Windows and macOS.
