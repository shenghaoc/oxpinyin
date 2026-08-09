# Findings — Specification by Execution

Date: 2026-08-07 · Method document for Phase 0. Human-led; precedes fleet
activity. Placement of agent-consumed documents follows Kiro conventions.

## Principle

The poorly documented C++ original is not primarily a text to read — it is a
program to run. The checksum-verified, source-built libpinyin reference freeze
is the executable specification. Tests are derived by execution
(characterisation / golden-master testing); prose SPECs are derived by
black-box probing first and source reading only where behaviour is
unobservable.

Consequences:

- Golden fixtures are behavioural facts emitted by the pinned program.
- The fleet implements against a frozen, pre-existing suite it cannot game.
- The suite is independently useful upstream regardless of our project.

## Capture harness

`tools/capture/` is a small C program compiled against the pin-built
`pinyin.h`, emitting line-oriented text stamped with the oracle pin. Protocol: fresh instance
(or documented reset), learning and dynamic adjustment off, default flags
unless stated otherwise, and the exact pin ref stamped into every fixture.

Each UTF-8 fixture record occupies one LF-terminated line. Fields are
TAB-separated `key=value` pairs; backslash, TAB, CR, LF and control bytes in
values use C-style escapes. Required semantic fields are `schema`, `pin_ref`,
`family`, `case`, `api_sequence`, `input`, `flags` and output fields. The pin
ref contains the source revision, model digest and DBM backend. `schema` is
present from the first capture.

## Fixture families

| ID | Family | Captures | Feeds |
|---|---|---|---|
| F-A | Parser | valid and ambiguous pinyin, apostrophes, incomplete tails, junk, empty and very long input | W1 acceptance; parser SPEC |
| F-B | Conversion | corpus utterances → candidate lists to depth 10 | W4 acceptance; W2 corpus |
| F-C | Flag matrix | each option bit singly against baseline | parser/scoring SPECs |
| F-D | Session | guess → choose → re-guess remainder | engine API; capi templates |
| F-E | Robustness | the 13 memory/access-safety rows below | scoped demonstrations and regression gates |
| F-F | Shuangpin/Bopomofo | parse behaviour captured while the harness exists | Stage 2 |

### F-E robustness tracker

The source catalogue is `reference/memory-safety-bugs.md` §7. A row may be a
runtime fixture, compile-time invariant, architecture test or policy/bench
gate; the language does not prevent every class.

| ID | Catalogue row | Owning evidence |
|---|---|---|
| F-E-01 | 1.1 #566 NULL key-rest | `nih` + prefix select; capi oracle test |
| F-E-02 | 1.2 candidate invalid access | session replay + fuzz |
| F-E-03 | 1.3 save-path race | hard-kill + replay |
| F-E-04 | 2.1 async `user_data` leak | owned-task review rule; provider tests |
| F-E-05 | 2.2 `FILE*`/path leak | safe-Rust RAII pattern gate |
| F-E-06 | 2.3 data-tool leaks | safe-Rust RAII pattern gate |
| F-E-07 | 2.4 unbounded growth | bounded-cache bench figures |
| F-E-08 | 3.1 English-mode UAF | safe-Rust compile-time ownership |
| F-E-09 | 4.1 i686 invalid access | loader fixture cross-check |
| F-E-10 | 4.2 sparc64 unaligned access | checked parsing + advisory-target CI |
| F-E-11 | 5.1 #179 stale-lock hang | user-store hard-kill gate |
| F-E-12 | 6.1 #542 assertion | `zhuan` totality fixture + proptest + fuzz seed |
| F-E-13 | 6.2 cloud/proxy crash | ASan across remaining FFI |

## What still requires reading

| Item | Why unobservable | Output |
|---|---|---|
| Interpolation formula + constants | internal arithmetic | scoring SPEC, triple-checked against F-B |
| Binary table formats | on-disk layout | data-route decision |
| Learn-delta semantics | partly probe-able | scoring SPEC §learning |
| Frontend key semantics | behind IBus | per-key checklist |

## Human/AI boundary and freeze

Architect agents write the capture harness, run captures and draft SPECs. The
human verifies the protocol, re-derives sampled constants, explains back and
freezes. Each scoped derivation PR merges before its separate implementation
PR. Implementers never read upstream implementation source.

Goldens regenerate only against a changed reference freeze, in a dedicated
human-reviewed data PR citing old and new refs. Implementers never modify a
golden; a failure means the implementation is wrong or an Architect must
correct the SPEC.
