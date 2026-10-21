# Tooling layer 5: migration and recovered measurement

> Status: partial implementation; remaining migration proposed. Docs only.
> Reconciled: 2026-10-20 UTC (`date -u`). Original split: PR #895,
> 2026-10-19 UTC. Original amendment: 2026-10-18 UTC.
> Preserve the original Phase 1 records and the later d8539e96 corrections.
> This reconciliation changes no code, tool config, workflow or AGENTS.md rule.

## 0. Purpose and layer ownership

Enable reliable setup after a fresh OS install and proportionate feedback
on native Linux, macOS and Windows. Keep pinned compatibility work separate.
Each layer owns one plan. References below import that owner's rule; they do
not create a second implementation or acceptance contract.

| Layer | Plan | Owns |
| --- | --- | --- |
| 1, native | [tooling-python](../plans/tooling-python.md) | Host CPython/uv, tools.dev, doctor and portable slot semantics. |
| 2, native | [tooling-just](../plans/tooling-just.md) | Root native command interface and shell/argument/status transport. |
| 3, native | [tooling-nextest](../plans/tooling-nextest.md) | Native Rust test orchestration, profiles, groups and parity. |
| 4, container | [tooling-container](../plans/tooling-container.md) | Pinned Linux execution, launch, separate state and pin-test profile. |
| 5, native/container migration | This document | Script handoffs, recovered semantics, measurement and historical records. |

The #938 → #939 → #941 → #942 → #943 stack is merged on main at `b034e834`.
Layer 1's interpreter and core services, Layer 2's six current recipes and
Layer 3's native nextest configuration are implemented in #939/#941/#942/#943.
The full environment provisioner, remaining recipe surface, CI cutover and
Python container service remain proposed. See each layer's capability status
and §10's delivery status. Implementation does not establish host acceptance.
Layers 1–3 record the Windows deferral and the bounded macOS checks on
`b034e834` from 2026-10-20 UTC. Their remaining macOS scopes stay unverified.
Historical findings and validation records retain their original provenance.

The 2026-10-19 separation ruling supersedes mixed native/pin execution.
It also supersedes cross-OS or native/container shared-lock acceptance.
Layer 4 owns the environment boundary. Native Linux pin work now belongs
inside its container. The full original-section ownership map is in #895's
PR body. It covers a3ae6f81 and every later addition through d8539e96.

Keep the nine companion-file changes as history. Their counts, findings and
old destination descriptions identify their recorded revisions. They do not
override these five current plan owners. No historical number changes owner
or gains a new artifact identity through this split.

## 1. Superseded revisions, preserved

The three PRs were closed **without merging**, and their source branches were
left in place. The revisions below are the exact heads that were closed.
GitHub's `refs/pull/<n>/head` refs resolve to the same commits, so the
revisions remain reachable even if a branch is later removed. The full
recovery record is `superseded-tooling-prs.md`.

| PR | Branch (retained) | Base | Head | Commits | `refs/pull/<n>/head` |
| --- | --- | --- | --- | --- | --- |
| [#836](https://github.com/shenghaoc/oxpinyin/pull/836) | `abi/extract-foundation` | `main` at `ecd9a7e5` | `ac74b77815fa9cd3e1bd26485daaa28df32ef3f0` | `ac74b778` | `ac74b77815fa9cd3e1bd26485daaa28df32ef3f0` |
| [#837](https://github.com/shenghaoc/oxpinyin/pull/837) | `abi/subject` | `abi/extract-foundation` at `ac74b778` | `8f088191176e8c57e270bacaa6e091bb57f1f1ca` | `a3cad350`, `8f088191` | `8f088191176e8c57e270bacaa6e091bb57f1f1ca` |
| [#842](https://github.com/shenghaoc/oxpinyin/pull/842) | `claude/tooling-language-consolidation-dvahgz` | `main` at `ecd9a7e5` | `a204f9c0e6874eee9d08795fe77a42b7a25da1d6` | `7834d535`, `02c40816`, `a793670d`, `33359825`, `a204f9c0` | `a204f9c0e6874eee9d08795fe77a42b7a25da1d6` |

#834 is already merged. Its report is retained as the Phase 1 historical
record; this amendment is the correction it needs, landed separately as a
documentation-only PR.

---

## 2. Why the correction remains needed

Python already owns the runner, contract families, option sweeps and parsers.
Refactor those assets in Python. Do not add a second Rust orchestration rewrite.
Recover #836/#837's semantics without their Rust ABI foundation.
Recover #842's measurement definitions without its standalone Rust initbench.
Use a compiled worker only where fidelity requires it. Keep Python outside
native timed intervals. Preserve the historical records and frozen inputs.

## 3. Architecture references and retained module work

Layers 1–4 own interpreter, native interface, nextest and execution environment.
Use their plans directly. Do not restate those contracts in migration recipes.
No Python module imports a product Rust crate. A generic two-library profile
has no pin/cell dependency; a pin profile adds Layer 4's environment policy.
Product crates remain separate from orchestration.

This layer owns these module handoffs:

| Module | Migration responsibility | Source | Status |
| --- | --- | --- | --- |
| `registry` | Read one `runners.json`; validate/select issue, row, runner, cell and kind. | `run-all.py`, Appendix A | Partial, #941: read/validate and runner/cell selection. Issue/kind and implementation metadata are proposed. |
| `subject` | Load through ctypes; use facade-specific symbols, lifetimes and process isolation. | §6 and contract-diff's Kit | Proposed recovery. |
| `observation` | Keep one tree and both existing wire formats byte for byte. | Capture and contract rows | Proposed handoff. |
| `compare` | Move plain/abort/crash/either/control rules once. | `contract-diff.py` matches() | Proposed handoff. |
| `runner` | Keep selection, builds, resource locks, timeouts, process cleanup and result protocol. | §5.2, Appendix A BIS-P-02 | Partial, #941: shared slot adapter only; runner handoff is proposed. |
| `report` | Keep tables, JSONL, uncertainty, ratios and provenance. | Existing parsers and §7 | Proposed handoff. |
| `measure` | Order recipes and interleave subjects within the selected environment. | Perf recipes and §7 | Proposed recovery. |
| `cases/` | Keep capture, replay, contract, option and startup grammars. | Existing case families | Proposed handoff. |

Layer 1 owns `cli`, `host` and `slot`. Layer 4 owns `cell`, `pin`, `systemdir`
and `artifacts`. Final names stay an implementation-stage decision.
Discovery adds `kind` values for gate, measurement, provisioning and probe.
The registry's `implementation` field names `legacy:<path>`, `python:<module>`
or `instrument:<path>`. Do not create a second maintained catalog.
Each scenario drives ordered ABI steps with a fresh user directory.
Record each loaded subject's path, full SHA-256 and platform loader metadata
from §7.5. A generic profile makes no pin claim.
Keep the documented prefix, cell, system-dir and subject-artifact overrides,
plus `TARGET_DIR` and `TMPDIR`. Apply Layer 4's environment validation.
Keep scratch work under `TMPDIR` and remove it at exit. Report subject identity
and argv. Distinguish diagnostics from completed verification.

Do not add `libpinyin-abi`, a Rust Subject crate, `oxpinyin-devtool` or standalone
Rust initbench. Do not extract or re-export pinyin-oracle internals for them.
Keep Bash only as a migration shim. The no-new-Bash rule starts at this
amendment's merge. Keep the exact #890 exemptions regardless of merge order:
`tools/oracle/compat.sh`, `tools/oracle/compat-inside.sh`, `tools/dev/check.sh`.
Their exemptions do not remove their handoffs or retirement gates.

## 4. Migration feedback goals

Use named representative hardware. Treat these as engineering goals, not
measured results or universal deadlines:

- Print a verification plan in about 1 second.
- Finish a focused check in about 10 seconds with a warm cache, including
  incremental compilation.
- Recheck a routine small review fix in about 1 minute.

Report cold provisioning and broad campaigns separately, with their reasons.
From tooling stage T1 onward, report before/after edit-to-result times. Split artifact
build, worker startup/registration, fixture/native init, execution and resource
waiting. Mark inapplicable phases. Keep diagnostic runs distinct from gates.
A missing required input must not become a passing result.
Layer 2 owns native recipe names. Layer 4 owns all container-prefixed commands.

## 5. Asset migration contracts

### 5.1 pinyin-oracle

Keep its files, public items, 29-symbol live layer and link-time pin check.
Do not impose generic extraction or re-export obligations. Its pin execution
uses Layer 4. A later native acceleration needs measured justification.

### 5.2 Runner

Refactor `run-all.py` into `runner` and `cli` in the same language.
Use Appendix A BIS-P-02 as the acceptance source. Run old and new on the same
registry, prefixes and `--no-build` artifacts in one container environment.
Keep every current row field, including `runner`, `cell`, `status`, `seconds`,
`build_seconds`, `lock_wait_seconds`, `slot_wait_seconds`,
`build_slot_wait_seconds`, `memory_wait_seconds`, `phases`, `pin_ref` and `reason`.
Require the same field set, field names, types, units and meaning for every
`seconds` and `*_seconds` field, including phase timing values. Every such
value must be valid and non-negative. Their values need not match: `run-all.py`
fills bare `seconds` from measured wall time. Timing values may differ
between live runs. Test the timing calculation separately, with controlled
inputs rather than a live-run comparison.
Require identical values for every other field: `runner`, `cell`, `status`,
`reason`, `pin_ref`, the `phases` structure, the selection and the verdicts.

PR #941 extracted T2N's slot and admission core from `run-all.py` into
Layer 1's `tools.dev.slot`, the one implementation. `run-all.py` imports it
through an adapter and keeps its CLI, its rows, its environment variables and
its cancellation semantics unchanged. Keep independent state in each
environment. `run-all.py` remains authoritative on Linux/macOS for its legacy
behaviour until T5. This does not permit host-native pin execution under the
new plan. Use Layer 4's `just container run` interface for pin work.

### 5.3 Contract families

Move `contract-diff.py` into `cases/contract` and `compare`, one family per PR.
Keep decorators, probes, table loops, five comparison rules and Axis-C JSON.
Require old/new `--observations` JSONL bytes to match for every deterministic
field. A field may differ between two runs of the same harness version only
when its exact key is listed, the change is proved with a same-version control,
and the comparison uses a stated invariant instead. Do not discard a field and
do not widen an accepted difference. Every other byte must match, including
every `~` key, userfile entry and stderr byte not named above.

#898's same-version control (two runs of one harness version) records these
keys:

- `pin.result.~increment-ns` and `subject.result.~increment-ns`: present,
  integer, non-negative. `contract-diff.py` writes it from
  `time.perf_counter_ns()`.
- `pin.result.~save-ns` and `subject.result.~save-ns`: present, integer,
  non-negative, from `time.perf_counter_ns()`.
- `pin.stderr` and `subject.stderr`: present. Compare after the harness's
  existing normalization: strip only GLib's process id and timestamp for a
  fatal run, and replace only a generated scratch directory name; a
  `stderr_varies` site checks presence only.
- `pin.userfiles` and `subject.userfiles`: present. Compare the same key set
  and values after removing only the stale-index entry #898 names; the partial
  `*.tmp` entry keeps its key and a `[size, sha256]` pair.

No other observation key may differ. Keep verdict rows identical except
separately proved intentional corrections such as #896. Use §10's cell policy.
Keep both sides until that family's acceptance passes.

### 5.4 Drivers, instruments and probes

Keep the three classes distinct:

| Class | Migration and acceptance |
| --- | --- |
| Observation drivers | Move the 33 C drivers, bisect surface mode, chewing-diff and union-edge-diff to Python scenarios. Require byte-identical logs and four-way C/Python × pin/subject comparisons. |
| Measured instruments | Keep system-import-profile, bisect perf/rss-diag, plant-oracle-bigram, identity-probe, btree-order, hash-walk and the startup worker compiled. Python builds and orchestrates them. |
| Compile/linkage probes | Keep cpp-smoke, frontend-import and alloc-pairing in C/C++. Integrate through the appropriate environment's command. |

A native executable means compiled code; it does not imply host-native pin work.
Layer 4 places every pin workload. Subject-only probes retain their native
scope from that layer's gate-placement table.

A superseded script may be retired in the same implementation PR that proves
its replacement, if that PR:

- (a) delivers the functional replacement;
- (b) gives old/new acceptance for the affected behaviour;
- (c) moves the registry and every live consumer;
- (d) loses no required test or error handling; and
- (e) lists the retired files by name for the maintainer's approval of that PR.

T14 keeps the final repository-wide reconciliation. Frozen evidence and
historical documents stay protected. This docs-only PR retires no files.
Keep each wrapper until these conditions are met. A historical reference
is not a live consumer.
Keep #767 workloads and every existing golden. Rewrite an instrument only
after a separate decision and same-run evidence.

### 5.5 Provisioning and workflow handoff

Use Layer 4 for frozen provisioning recipes, identity checks and commands.
Move nightly-fixture-differentials and run-differentials through its container
Rust-test path over the same testsupport locators. Each workflow switch needs
its own approval. Run old and new steps side by side for one cycle and require
identical conclusions. Native jobs use Layer 3's native profiles; pin jobs
use Layer 4's separate profile. Do not change workflow files in this stack.

Include Layer 4's full native gate inventory in that comparison.
Keep the crate-configuration checker and its harness, Lizard 1.24.0 with
`lizard crates/ -l rust -C 40`, and the W3 fixture checksum command.
Keep verify-nightly's release Kyotocabinet overflow lane and its exact
`RUSTFLAGS` from Layers 3–4. Compare these gates before any approved CI switch.

## 6. Recovered requirements from #836 and #837 (the ABI foundation)

The Rust crate remains retired. Keep its semantics. The Python `subject`
module must satisfy every rule below, and the named tests (or their Python
equivalents) are the acceptance tests. This is a requirements recovery, not a
port of the Rust code.

### 6.1 Vocabulary and wire format (#836)

The Python core must keep, in one module each:

- `pinyin_option_t` bits and the sort word, with the validated `0x18a`
  capture profile and `0x1e` sort constant; `DYNAMIC_ADJUST` is rejected by
  the oracle policy, never silently masked.
- Segment, completeness and candidate-type vocabularies for both facades.
  The facades are **different types**: libzhuyin's `lookup_candidate_type_t`
  has four values with different meanings from libpinyin's eight, and
  `zhuyin_get_sentence` takes no sentence index.
- The `pinyin-capture-v1` escaping and field notation, byte for byte:
  `CAPTURE_SCHEMA`, `escape`/`unescape`, `CaptureRecord`, `read_fixture`,
  `segment_to_wire`/`segments_to_wire`, and the `CaptureFieldMalformed
  {field, value}` error shape. Existing fixtures and readers must not change.
- The capture test cases that pinned the wire format, including round trips,
  malformed fields, unknown candidate types, non-UTF-8 and the field/value
  error payload. #836's `tests/abi_reexports.rs` and the crate's 13 unit
  tests are the source of the cases; the Python tests must assert the same
  bytes, not merely "parses".

### 6.2 Subject rules (#837)

| Rule | Required Python behaviour |
| --- | --- |
| Open by path | Load a shared object by path; report a missing file, a file the platform loader cannot load, or a failed loader call. Keep non-ELF as the Linux-only negative test. The controller must not crash |
| Symbol resolution | Resolve the facade's symbols once; a missing symbol is a property of that library, not a rejection. `missing_symbols()` lists all absent names |
| Missing symbol scope | Only the operation that needs the absent symbol fails, with the symbol name; a library missing `init` still opens; a pinyin library missing the n-best accessor still lists candidates |
| Facades | Pinyin and zhuyin are separate tables; a session of the other facade is refused, never miscalled; the zhuyin sentence has no index. Its candidate n-best index is "not available on this facade". Use only the public C API |
| Process isolation | Differential runs load **one library per process** by default. A second live subject in the same process is refused before touching the file; an explicit opt-out exists for callers that know the libraries do not interfere |
| Aborts | Do not mask an abort the library raises; run untrusted inputs in a child process, one per input where the pin is known to abort |
| Threads | libpinyin is not thread-safe; subject, context and sessions are never shared across threads. The controller uses processes, not threads, for a live subject |
| Release-or-refuse | Resolve `*_fini` before `*_init` runs and `*_free_instance` before `*_alloc_instance`; if the destructor is absent, create nothing. Every context/instance created is released even when a later call fails |
| Never unload | The library handle is never unloaded; a subject opened after another was dropped sees the library's process-global state. Clean state requires a fresh process, not just a fresh subject |
| Owned vs borrowed | Candidate strings point into the instance: copy before the next call, never free. The sentence is caller-owned and freed with the library's `g_free`; if no `g_free` is found, fail before asking the library, so nothing leaks |
| Bounds | Pass Unix C path arguments as raw bytes, without a UTF-8 check. Reject only an interior NUL in a C path. An interior NUL in input is also an error. On Windows, report an unrepresentable path if the platform cannot supply path bytes. Check parsed length and candidate counts against the call's result |
| Error taxonomy | Every failure is a reported value with the input that caused it: load, missing symbol, already-open, facade mismatch, interior NUL, unrepresentable path, init failure, instance failure, call failure. No panic escapes to the controller |

The recovered Rust tests to reproduce as Python tests, with their names kept
for traceability:

`pinyin_observation_reads_everything_and_releases_everything`,
`nothing_parsed_queries_nothing_further`,
`zhuyin_has_its_own_candidate_types_and_an_unindexed_sentence`,
`a_session_of_the_other_facade_is_refused_not_miscalled`,
`a_missing_symbol_fails_only_the_operation_that_needs_it`,
`without_g_free_a_sentence_is_never_requested`,
`a_revision_without_the_nbest_accessor_still_lists_candidates`,
`a_library_without_init_cannot_open_a_context_but_still_opens`,
`refusals_are_reported_with_their_inputs`,
`a_context_that_could_not_be_released_is_never_created`,
`an_instance_that_could_not_be_freed_is_never_allocated`,
`a_missing_allocator_creates_nothing_either`,
`every_context_created_is_released_even_when_a_session_fails`,
`nothing_derived_from_a_subject_can_cross_threads`,
`a_failed_open_gives_its_slot_back`,
`the_default_open_refuses_a_second_subject_before_touching_the_file`,
`the_first_subject_takes_the_only_slot_and_additional_ones_are_explicit`,
`the_real_loader_reports_a_library_with_none_of_the_symbols`,
`a_file_that_cannot_be_loaded_is_a_reported_failure`.

One adaptation: `nothing_derived_from_a_subject_can_cross_threads` was a
compile-time marker in Rust. In Python the invariant is enforced by design
(the controller never shares a live subject between threads and uses worker
processes for concurrent work) and tested as process/thread-affinity
behaviour; the Rust name is kept for traceability, not as a claim that Python
has the same compile-time check.

The recovered Python integration test requires `LIBPINYIN_ABI_PINYIN_SO`,
`LIBPINYIN_ABI_SYSTEM_DIR` and `LIBPINYIN_ABI_ZHUYIN_SO`. A missing input
fails the test. The #837 original skipped the zhuyin part when its variable
was absent (`crates/libpinyin-abi/tests/real_library.rs` at `8f088191`).
The recovered test does not skip that part. It checks both facades and
prints one stable line per observation. Run one process per library to
compare libraries.

---

## 7. Recovered functionality from #842 (startup measurement)

### 7.1 Scope and acceptance

Layer 4 owns all pin-dependent execution. The optional native macOS path
below stays separate and uses no pin.

The capability measures **where initialization time and memory go**, for a
pin-built and an oxpinyin-built `libpinyin`/`libzhuyin`: library load and
symbol resolution, `*_init`, option setup, `*_alloc_instance`, startup
memory, and the time to the first usable candidate as the outer startup
boundary. It is not steady-state typing, repeated guessing, runtime tuning or
training.

The acceptance rule is unchanged and non-negotiable: **equivalence before
performance.** Timing is reported only for a cell whose subjects first
produced identical complete observations through the same code path as the
timed child. A mismatch is `NOT COMPARABLE` and gets **no ratio**; a timed
sample that contradicts the verified transcript is rejected, not averaged.

### 7.2 Units (the exact workload)

`p` is `pinyin` or `zhuyin`; the sequence is the same except where stated.
Option word `0x18a`; sort `0x1e`; first input `nihao`.

| Unit | Calls, in order | Required state | Expected on the pin | Timed interval |
| --- | --- | --- | --- | --- |
| U1 load | `dlopen`; resolve every facade symbol | none | all symbols present | open and resolution |
| U2 init | `p_init(system, user)` | U1; user directory is a private copy of the profile | non-NULL context | that call |
| U3 options | `p_set_options(ctx, 0x18a)` | U2 | true | that call |
| U4 alloc | `p_alloc_instance(ctx)` | U3 | non-NULL instance | that call |
| U5 first candidate | `p_parse_more_full_pinyins(inst, "nihao")`; pinyin: `get_parsed_input_length`; pinyin: `guess_candidates(inst, 0, 0x1e)`, zhuyin: `guess_candidates_after_cursor(inst, 0)`; `get_n_candidate`; `get_candidate(0)`; `get_candidate_type`; `get_candidate_string` (copied out) | U4, no earlier parse | consumed 5, length 5, true, at least one candidate, non-empty string | parse through string copy |
| U6 teardown | `p_free_instance`, then `p_fini` | U5 | none | each call separately |

State carried: U2's context into U3/U4; U4's instance into U5/U6. The user
directory is a private working copy, so no sample changes what the next one
starts from. **Excluded from every timed interval:** building the trace,
memory reads, serialisation and the verification hook. Diagnostics the library
writes to stderr during the units are captured by the parent and compared in
verification. Sentence guessing, repeated queries, training and saving are not
units.

Stop points for whole-process profilers, in order: `start`, `lib_load`,
`init`, `config`, `alloc`, `first_candidate`, `end`. Stop points `start`
through `first_candidate` skip teardown; `end` runs U6, timing `p_free_instance` then `p_fini` separately,
excluded from `ttfc`.

### 7.3 Equivalence observation

An observation records every answer to the defined script without
normalisation. It includes return values, parsed lengths, sentences, ordered
candidate types and text, stderr diagnostics, user-directory files, and a
save/reopen persistence scenario. Record candidate n-best indexes for pinyin
only. For zhuyin, record "not available on this facade". Use only the public
C API; do not derive this field from other data. The recovered workload
input sets:

- main script (`SCRIPT_INPUTS`, 18, in order, ending with a repeated
  `nihao`): `nihao`, `ni`, `n`, `zh`, `zhongguo`, `xi'an`,
  `bu'tian`, `fan'gan`, `fangan`, `chongke`, `caisho`, `paolen`,
  `waimenggu`, `naoxion`, `liangniejue`, `chuaipengdengzaimiu`, `a`,
  `nihao`;
- chewing inputs (zhuyin facade): `su3cl3`, `ji3`, `5j/\x20`
  (`\x20` denotes the actual trailing U+0020).

The equivalence transcript contains workload inputs only, including profile
witness inputs and the save/reopen scenario. Hostile probes form a separate
safety check. Run each probe in its own process. Report refusal and crash
behaviour for each input. Keep these outcomes outside the transcript that
controls ratios. #842 uses this separation at `a204f9c0`:
`child.rs::verify` calls `observe.rs::observe_script`;
`orchestrate.rs::hostile_cell` emits separate `ProbeRecord` results.

- Hostile inputs (12): `""`, `"'"`, `"''"`, `"'ni"`,
  `"ni'"`, `"nihao'"`, `"zzzz"`, `"12345"`, `"ni hao"`, `"NIHAO"`,
  `"你好"`, and the long `nihao…` repeat.

Only sentence indexes the candidate list proves exist are read. A failed call
that is not a value (missing instance, interior NUL) is an error, not an
observation; a library answer of false/NULL is a value.

### 7.4 Profile fixtures

A fixture is a directory with `user/` exactly as the library left it,
`profile.manifest` (schema `oxpinyin-startup-profile-v1`, one line per file:
size, SHA-256, relative path) and optional `witness.txt`. Every sample gets a
private copy; the manifest is checked before and after. Recovered profiles:

| Profile | Import calls | Distinct phrases | Frequency per row | Libraries | User directory (baseline) |
| --- | ---: | ---: | ---: | --- | ---: |
| `empty` | 0 | 0 | — | — | 0 B |
| `modest` | 80 | 2 (same two in every library) | 5 | all four | 29,233 B |
| `phrases` | 10,000 | 10,000 | 20,000 | almost all `gb_char` | 496,209 B |
| `journals` | 10,000 | 10,000 | 20,000 | all four, quota 3000/3000/3000/1000 | 543,885 B |

20,000 is the **frequency**, not a row count. The profiles have different
shapes and are **not** a controlled size-scaling series. A non-empty profile is
accepted only after witness inputs answer differently from `empty`.
Population is through the public import API, save and close; a refused row or
an unsaved profile is an error.

The count table does not identify the exact workloads. The recovered
profile generator is `crates/oxpinyin-initbench/src/bin/initbench.rs::build_profiles`
at `a204f9c0`. It calls `crates/oxpinyin-initbench/src/fixtures.rs`.
The generator uses no random seed. Its parameters and ordering are:

- `export_system` exports libraries 1 through 4 in order.
- `select_sequential` selects the first pronunciation of each of the first
  10,000 distinct phrases in export order. `phrases` uses count 20,000.
- `select_balanced` scans export order with quotas `[3000, 3000, 3000, 1000]`.
  It keeps globally distinct phrases and joins library groups in order.
  `journals` uses count 20,000.
- `populate` imports consecutive library groups, saves, and closes the context.
- `candidate_inputs(rows, 400)` removes ASCII digits from pronunciations.
  It keeps nonempty inputs of at most 24 bytes and removes duplicates in row
  order. `build_profiles` compares empty and reopened-profile transcripts.
  It keeps the first 40 matching candidate inputs from `discriminating`'s
  ordered tags and writes `witness.txt`.

The source records this count-5 sequential-input SHA-256:
`22637aee560c86761bb6d9d187d9af8b9fea970a6690bc21e21e5b6fc1cbb117`.
This is `REGISTERED_PHRASE_INPUT_SHA256`, not a count-20,000 fixture digest.
The baseline records the system-data identity as
`e18f23beb15c48ed0d86988364af70bdbce6840313fe1399317865cb811a56c2`.

Full output digests for `phrases` and `journals` are unavailable.
The historical tree contains no saved fixtures, manifests or generator output
with those identities. The exact historical witness inputs are also
unavailable: the tree contains no generated `witness.txt` files.
Tooling stage T11 must reproduce these workloads from the recovered generator and
pinned data, not from the count table. Record the resulting full digests and
exact witness inputs. Do not claim byte identity with unavailable historical
fixtures.

### 7.5 Metrics and statistics

Timing metrics (nanoseconds on one monotonic clock, reported in ms):
`lib_load`, `init`, `config`, `alloc`, `first_candidate_phase`, `ttfc`,
`setup_plus_first_candidate`, `drop_instance`, `fini`, `process_wall`.

- **Time to first candidate** (`ttfc`) = end of the first-result phase − start
  of library load, from the same clock, including gaps between phases.
- Memory metrics at each point: RSS delta, peak RSS, anonymous, file-backed,
  private dirty, PSS, virtual size, minor and major faults. Read between
  phases only in a `memory` sample; a memory sample's timings are never
  aggregated as timings, and a timing sample's mid-run memory is not used.
  On macOS the worker reports RSS and virtual size only, as #842's `ps`
  path did (`crates/oxpinyin-initbench/src/memory.rs::{PsProbe, parse_ps}` at
  `a204f9c0`). Every other memory metric is recorded as unavailable, never
  as zero; an unavailable metric is never part of a comparison.
- Summaries: median, 95% confidence interval, range/median spread, `n`;
  ratios only between comparable subjects, with a ratio interval. Warm-up
  rounds are excluded; rejected samples are not aggregated.
- Run shape recovered from the baseline: 60 timed rounds with 3 warm-up; 30
  memory rounds with 2 warm-up; optional `--cache cold` (guest-level
  `drop_caches`, needs root) and `--bind lazy` (`RTLD_LAZY`) at 30 rounds;
  CPU pin; one fresh process per sample; subjects alternate within a round.
- **Always include a same-bytes control** (the same oxpinyin object at another
  path). Its ratio against ox is the noise floor; if it is not near 1, the
  machine is not quiet and the run is not a result.
  **tooling stage T11 threshold — OPEN DECISION.** #842 defines no numeric band or
  acceptance test (`docs/runbooks/initbench.md`, same-bytes control rule;
  `crates/oxpinyin-initbench/src/report.rs::fmt_ratio`). Tooling stage T11 must choose
  a fixed band or a confidence-interval overlap test on repeated rounds.
  This amendment chooses neither. The historical 0.94–1.05 range is not an
  acceptance band.
- Never use an absolute figure from another run, host or CPU as a baseline.
  Report the host line, SHA-256 and size for each timed artifact.
  On Linux, report ELF `NEEDED` from `readelf -d`.
  On macOS, report Mach-O `LC_LOAD_DYLIB` and `LC_ID_DYLIB` from
  `otool -L` / `-D`. Record each field that the platform cannot supply as
  unavailable. Never omit a required field silently.

### 7.6 Recovered findings that remain valid

These are historical results for the artifacts and trees they name, not
current claims about `main`:

- Library load: oxpinyin 0.50× the pin's time; 0.82 M against 4.12 M
  instructions (BDB, one host).
- `empty`: time to first candidate 0.71× pinyin, 0.73× zhuyin.
- `modest`: oxpinyin `*_init` 0.27× the pin's. The pin's own `modest` init
  (6.7 ms, 11.8 M Ir, about 8.3 MiB anonymous) is slower than its `phrases` init
  (3.1 ms, 10.3 M Ir, 0.8 MiB anonymous).
- `phrases`/`journals`: oxpinyin `*_init` 6.8–8.3× the pin (about 21 ms
  against 3 ms; 141 M Ir and 268 k allocation calls against 10 M and 1.3 k).
- Same-bytes control: 0.94–1.05× for init and first candidate — the noise
  floor.
- The first-result phase is about 0.1–0.34 ms and is not where startup goes.
- Pin-only Callgrind attribution: the `modest` extra cost is journal replay
  (`SubPhraseIndex::merge` → `add_phrase_item` →
  `MemoryChunk::set_content`). Why the journals take different paths remains
  a **hypothesis**, not a finding; oxpinyin per-function attribution was not
  captured.
- **Unresolved behavioural difference:** zhuyin `'` and `''` — the pin's
  `zhuyin_guess_sentence` and `zhuyin_guess_candidates_after_cursor` return
  false; oxpinyin returns true with no sentence. Reproduced at no option word,
  `0x20` and `0x18a`. It contradicts `compatibility-policy.md` row 53 item 2,
  which records it as closed in code; it is not fixed, not classified as
  acceptable, and does not affect the timed workload. The separate hostile
  probe reports this difference outside the transcript that controls ratios.
  A maintainer must check the difference against the register.
- Raw captures were not retained (ephemeral container); the baseline document
  says so at the point of use.

### 7.7 Python + native implementation of the recovered capability

- Use only libc and the subject's own libraries for the new startup worker.
  Read Linux process memory through `/proc` inside the container.
  Keep the optional native macOS `ps` memory path.
- Python `measure` owns profiles, verification orchestration, comparability,
  statistics, report and the same-run base/PR/pin interleaving.
- The native C worker (proposed path `tools/workers/startup.c`) has untimed
  verify and timed measure modes. Both use the same native U1–U6 routine
  and record its digest. The native verify path also records the full §7.3
  transcript. It observes all 18 workload inputs, profile witnesses,
  sentences, ordered candidates, stderr, directory state and save/reopen
  persistence. These additional native observations remain outside timed
  mode's U1–U6 workload. Full-transcript comparisons control comparability.
  Run U1–U6 on a fresh instance before any transcript parse.
  Run the full transcript on a separate fresh instance or process.
  Never parse a transcript input on the U5 instance before U5.
  A timed sample is valid only when its U1–U6 digest equals the verify
  digest for the same subject and profile.
  Python orchestrates both modes, checks digest equality and comparability,
  and calculates statistics and reports. A Python `ctypes` observation may
  cross-check the result but never replaces native verification. Python
  remains outside the timed process. The worker reads memory at the
  requested points and emits a line-oriented record. Python serialises the
  existing `oxpinyin-initbench-sample-v1` /
  `oxpinyin-initbench-observation-v1` schemas. Historical records remain
  interpretable.
- Record the worker source hash next to the executable identity.
  Record the full SHA-256, byte size and build command of the worker
  executable that actually ran. Check that executable against its record.
  Refuse a run if an executable identity field is missing or the executable
  does not match its record.
- Run pin comparisons in Layer 4's container on every host, Linux included.
  Use Layer 4's separate base and PR source snapshots and identity checks.
  Build both inside the same container run as the pin comparison.
  Require non-emulated Linux amd64 for pin timing and gates.
  Keep optional native macOS oxpinyin-only base-versus-PR timing.
  It supplies information only. Never gate on it, compare it across operating
  systems or run it under emulation. Native macOS pin rows are unsupported:
  return a nonzero status naming the container requirement. Do not skip them.
  Keep Windows startup measurement unclaimed until its loader and memory
  probe are implemented and tested. A Windows host may launch a supported
  container adapter; that is a separate Linux execution environment.
- The worker returns the same failure/refusal classes: a missing library,
  a file that is not a loadable native shared library, a library without the
  required symbols, a bad invocation or a failed unit is a non-zero exit
  with nothing on stdout; a missing input is a failure, never a skip.
- The capability is accepted only when it reproduces #842's definitions: the
  units, input sets, profiles, equivalence verdicts, metric names, schema
  names, rejection rules and control-subject rule above. It does not need to
  reproduce #842's *numbers* — those are historical and tied to that
  artifact — but on Linux a same-run comparison against the pin must be
  possible with the recovered rules.
- **Tests recovered from #842** and carried forward: the four
  process-level refusals (missing library, a file that is not a loadable
  native shared library (non-ELF is the Linux-specific negative test), real
  library without symbols, bad invocations); the live comparability test (6 verdicts across
  two facades × empty/modest/persistence, transcript > 100 lines, hostile
  probes for both subjects and both facades); the timed-sample reproduction
  test (3 rounds + 1 warm-up × 2 subjects × 2 facades, timed U1–U6 digests
  equal to native verify digests for each subject/profile, fixture untouched); and
  the mutation test (unchanged re-run comparable; changed workload, tampered answer, changed trace digest
  and populated-vs-empty profile each detected). The unit-test families for
  sequencing, timing boundaries, memory probes, profiles, manifests,
  statistics, report ratios, child failure/timeout, and digest are also
  carried forward.
- Add a tooling stage T11 integration case with three distinct synthetic subjects:
  base, PR and pin. Add a same-bytes control for the PR subject.
  Check same-run interleaving order across all four subjects and the expected
  base/pin, PR/pin, PR/base and control/PR ratios. Real pin rows remain
  limited to non-emulated Linux amd64 under Layer 4 and §7.7.

---

## 8. Inventory and history

Keep all 290 inventory records and their factual counts, platform notes and
observed anomalies. Apply the destination translation below; do not renumber
or rewrite the historical inventory.

| Earlier destination | Current owner or handoff |
| --- | --- |
| Shared Rust core | Python module or scenario from §3. |
| libpinyin-abi Subject/Observation/Comparator | §6 subject/observation/compare semantics. |
| oxpinyin-devtool | Layer 1's CLI through Layer 2 or Layer 4. |
| Cell/SystemDir/PinProfile/Artifacts extraction | Layer 4's modules; pinyin-oracle retains its files. |
| Rust-test differential driver | Layer 4's container test path. |
| Frozen data, instrument or probe | Retain; use §5.4 and Layer 4's placement. |
| Retire after proof | Replacement PR under §5.4; T14 keeps final repository-wide reconciliation. |

Keep the three #890 handoffs. Layer 2 owns check.sh's native interface.
Layer 4 owns compat.sh/compat-inside.sh launch acceptance. Tooling stages T2 and T6
schedule those handoffs. Preserve both compatibility scripts until launch
acceptance passes. Their retirement follows §5.4 in the replacement PR.
Absence from CI is not non-use. Check and name documented/manual consumers
before classifying a tool for retirement.
The Phase 1 summary table and recovered records remain history at their
recorded revisions. Use the five layer documents for the current proposal.

### 8.1 Current shell inventory

Count rule at main `b034e834`: count tracked paths below `tools/` whose names
end in `.sh`, including sourced libraries and shell test harnesses. Use
`git ls-files 'tools/*.sh' 'tools/**/*.sh'` and count each path once.
There are 90 files, including 52 below `tools/bisection/`. A separate shebang
scan found no shell files without that suffix. This is a current file count,
not a replacement for the historical 290-record inventory.

The groups below partition those 90 files by primary responsibility.
Counts in parentheses are total / bisection. Paths are relative to `tools/`.
Consumers are current commands, source calls or active instructions; workflow
path filters and historical mentions do not establish live use. A group can
have manual-only members. No caller found is not proof for retirement.

| Responsibility (count) | Shell files | Live consumers | Planned owner |
| --- | --- | --- | --- |
| Dispatch wrappers (38 / 38) | `bisection/run-*.sh` except the ABI, instrumentation and packaging files below. | `bisection/runners.json` commands; `run-all.sh` → `run-all.py`; `oracle/compat-inside.sh`; `ci.yml` live-typing step; oracle and compatibility runbooks. | Layer 5 runner/cases; Layer 4 container dispatch. Subject-only uses retain native scope. |
| Toolchain/artifact preparation (3 / 3) | `bisection/cell-artifacts.sh`, `oracle-cell.sh`, `system-dir.sh`. | `run-all.py` calls cell-artifacts; runner shells source oracle-cell/system-dir; `cell-identity.test.py` checks artifact preparation. | Layer 4 cell/artifacts/systemdir. |
| Fixture/environment (11 / 0) | `bdb/run-sanitizers.sh`, `kc/run-sanitizers.sh`, `capture/run-capture.sh`, `cloud/setup.sh`, `dev/check.sh`, `env-probe/*.sh`, `model/fetch-model.sh`, `store/backend-matrix.sh`, `tkrzw/*.sh`. | `store-backends.yml` backend-matrix/BDB sanitizer steps; cloud setup calls distro-probe and fetch-model; compat-inside calls cloud setup; `dev/check.test.py` runs check.sh; setup/cloud/backends and capture-fixtures instructions. Env probes and libpinyin-backend-probe remain manual entry points without an external live caller found. | Layer 1 native check/host; Layer 4 fixtures/provisioning; Layer 5 diagnostics. |
| ABI comparison (4 / 2) | `abi/*.sh`, `bisection/run-abi-probe-diff.sh`, `run-cpp-smoke.sh`. | Registry abi-probe-diff/cpp-smoke/abi-exports/abi-alloc rows; `ci.yml` exports/allocation/C++ steps; run-bisect calls C++ smoke; packaging instructions. | Layer 5 subject-only ABI probes; Layer 4 pin ABI comparisons. Keep compiled probes. |
| Instrumentation (12 / 8) | `bisection/debuginfo-neutrality-lib.sh`, `run-callgrind-differential.sh`, `run-perf-baseline.sh`, `run-perf-matrix.sh`, `run-perf-same-data.sh`, `run-rss-diag.sh`, `run-system-import-profile.sh`, `run-tree-recipe-control.sh`; `perf-gate/*.sh`; `profile/run-w8-cycle.sh`. | Registry measurement/probe rows; verify-nightly calls snapshot.sh; callgrind runner and debuginfo probe source the library; benches runbook names perf/profile commands. callgrind-ir.test.sh and series.test.sh remain manual parser harnesses. | Layer 5 measure/report and retained instruments; Layer 4 pin execution. |
| Oracle/container (12 / 0) | `oracle/*.sh`, `container/runtime.sh`, `datagen/libpinyin-drop-in-differential.sh`. | compat sources runtime and calls image-tag/compat-inside; cloud setup and oracle Dockerfiles use build-oracle; oracle-image workflow calls image-tag/verify-oracle-image; verify-nightly calls nightly-fixture-differentials, which calls drop-in-differential; registry user-dir-round-trip; oracle/compatibility runbooks. | Layer 4 launcher/pin/provisioning and container Rust tests. |
| Packaging (10 / 1) | `packaging/*.sh`, `bisection/run-shipped-check.sh`. | Registry shipped-check/pc-version/pc-metadata rows; `ci.yml` check-pc-version; release-packages calls release-stage and distro packagers; release-stage calls install/relink helpers; perf snapshot and packaging/benches instructions use install.sh. | Layer 5 packaging orchestration; Layer 4 pinned install-tree comparisons. |

## 9. Decision ownership

This table routes D1–D13. The referenced owner holds the requirement.

| Decision | Owner |
| --- | --- |
| D1 interpreter/dependencies | Layer 1; Layer 2 owns the just prerequisite. Native Cargo dependencies stay unchanged. |
| D2 names/homes | Layer 1 package; Layer 2 native recipes; Layer 4 container prefix. No repository split or publication. |
| D3 goldens | Keep golden bytes. Re-capture remains a STOP. Make harness selection cell-parametric and record backend provenance. |
| D4 instruments | §5.4 and §7; keep measured instruments and #767 workloads. |
| D5 ledgers | Keep revision-pinned history and generators. #793 precedes ledger retirement. |
| D6 registry | §3's one catalog and compatible implementation field. No automatic path move. |
| D7 verdict/status | Layer 2 status transport; §5.2 runner schema and §10 global invariants. |
| D8 CI | §5.5 and tooling stage T13. Each workflow edit needs approval. |
| D9 platforms | Layers 1–3 native; Layer 4 pin execution and runtime limits. |
| D10 pacing | §10. Tooling stage T1 keeps its separate authorization. Keep family handoff and no-narrowing safeguards. |
| D11 retirements | §5.4 permits named script retirement in the approved replacement PR. T14 keeps final reconciliation. Keep closed branches; branch deletion still needs separate approval. |
| D12 startup worker | §7. Keep units, source hash and executed-worker full SHA-256, size and build command. Refuse missing or mismatched identity. |
| D13 Python packaging | Layer 1. No published/installed Python package or new console entry point. |

## 10. Migration stages by environment

Tooling stages use T0–T14 for delivery milestones.
They differ from stack layers and repository roadmap stages.
Each implementation PR needs a coherent scope and its own approval.
T1a and the native T2 core are implemented as recorded below; their full
cross-platform acceptance remains partial. Remaining delivery is proposed.
Tooling stage T1 retains only its independent existing authorization.
A split row below is an acceptance scope, not permission for a partial cutover.
PR #939 delivered T1a as one interpreter PR across both environments.
It did not deliver full environment provisioning. Keep its exact interpreter;
do not restore an older supported interpreter while another scope waits.

| Tooling stage | Environment | Delivery and acceptance | Status |
| --- | --- | --- | --- |
| T0 | Native/container records | This docs-only stack. Review documents; build nothing. | Implemented, original plans and #938 acceptance corrections; this reconciliation is docs only. |
| T1N | Native | Improve existing native script feedback under the authorized lane. Keep selected checks, observations, valid verdicts and required gate breadth. No just prerequisite. | Partial: #905/#906 delivered admission/defaults. The remaining feedback scope is proposed under T1 authorization. |
| T1C | Container | Improve expect-parent validity, exact issue/family selection, phase timing, worker registration and consumed-artifact builds. Check staged-artifact identity before execution. Prove #896 INVALID with old-driver failure/new-driver success. Preserve selected IDs, envelopes, valid verdicts and statuses. | Proposed remaining feedback scope under T1 authorization. |
| T1aN | Native | Deliver Layer 1's exact interpreter cutover, setup and identity acceptance. | Implemented interpreter cutover, #939. Bounded macOS identity passed at `b034e834`; full setup acceptance remains partial. |
| T1aC | Container | Deliver Layer 4's separate interpreter in the same atomic PR. Keep frozen oracle artifacts. Workflow edits need separate approval. | Implemented interpreter cutover, #939. Full environment provisioning is proposed. |
| T2N | Native | Deliver Layers 1–2: package, CLI, registry reader, bootstrap doctor, thin recipes and slot. Extract the slot and admission core from run-all.py into tools.dev.slot, the one implementation; run-all.py imports it through an adapter and keeps its CLI, rows, environment variables and cancellation semantics unchanged. Migrate check.sh with matching selection/status. Run native shell, inheritance, resource and argument acceptance. | Partial: #941 core/adapter and #942 native recipes/check are implemented. Full recipe surface and host acceptance remain open. |
| T2C | Container | Keep legacy scripts authoritative. Apply Layer 4's separate namespaces and host budget contract before any pin handoff. Do not share native slot/cache state. | Proposed environment/budget contract; #890 legacy launch remains implemented. |
| T2T | Native | Deliver Layer 3 nextest after the native interface. Compare every current CI crate/feature scope, doctests and selected ignored tests. No workflow changes without approval. | Partial: #943 native profiles/recipes are implemented. Linux equivalence is historical at its named head; macOS tkrzw equivalence is historical at `b034e834` on 2026-10-20 UTC. The current macOS CI BDB scope passed at `219f420` on 2026-10-20 UTC: 1,218 passed and 22 ignored across 1,240 identities; separate doctests had four passed and two ignored. Identities and verdicts match. Other macOS scopes and Windows remain unverified. CI cutover is proposed. |
| T3N | Native | Add conservative native verification records and invalidation. Keep original provenance on reused results. | Proposed. |
| T3C | Container | Add separate records and an operational full BDB differential backstop with pin/model20. Validate its host and scheduling policy before narrowing mandatory local selection. | Proposed. |
| T4N | Native | Test portable pure core functions without an oracle or pin installation. Use Layer 3 for Rust tests. | Proposed. |
| T4C | Container | Deliver subject/observation/compare/capture/replay. Reproduce all §6 tests, both facades, ownership/abort/missing-symbol rules and fixture bytes. Retain C drivers and contract-diff. | Proposed. |
| T5 | Container | Refactor runner/cli and carry the T2N slot and admission import through the adapter, with container-only state. Apply §5.2's full row/timing comparison. Run one full gate set under the cell rule. The refactored runner/cli takes legacy authority at T5; retain run-all.py and run-all.sh until §5.4 is met. | Proposed. |
| T6 | Container | Deliver cell/systemdir/artifacts/pin and launch handoff. Require byte-equal describe JSON/env dumps, every exit-3 text and record/constant agreement. Apply Layer 4's full side-by-side container acceptance. Retain shims until §5.4 is met. | Proposed. |
| T7 | Container | Unify runtime per-cell pin checks. Use BDB by default. Run original live tkrzw and kc-on-bdb rejection only when rule 2's path trigger fires. Keep EXPECTED_PIN_REF meanwhile. | Proposed. |
| T8 | Container | Move observation families one per PR. Require four-way C/Python × pin/subject checks and original expectations. Retain wrappers and C drivers until replacement acceptance and §5.4 approval. | Proposed. |
| T9 | Container | Move contract families, Axis C and tie regeneration. Keep observation JSONL bytes, verdict rows, difference signatures/counts/stability and byte-identical regenerated fixtures. Retain unported families and old Axis-C/tie implementations until equal. | Proposed. |
| T10N | Native | Finish any native Rust-test family handoff under Layer 3 parity. Keep its crate/feature/ignore scope. | Proposed. |
| T10C | Container | Migrate option sweep and Rust differential tier. Require per-scheme counts/JSON, identical selected ignored tests/pass-fail and the drop-in bdb arm. Use Layer 4's test profile. | Proposed. |
| T11N | Native macOS | Deliver only §7's optional oxpinyin base/PR information. Keep unavailable metrics explicit; no pin or cross-OS gate. | Proposed. |
| T11C | Container | Deliver measurement/report and startup controller/worker. Use Layer 4's base/PR source input contract. Keep parser output on retained Callgrind/Massif/smaps/JSONL inputs and the same dry-run commands. Apply all §7 units, profiles, refusals, mutations, digests and four-subject interleaving/ratio tests. Retain old parsers until equal. | Proposed. |
| T12N | Native | Migrate source/subject-only ABI probes. Require identical symbol verdicts. Retain wrappers until §5.4 is met. | Proposed. |
| T12C | Container | Migrate pin ABI and packaging seams. Require identical symbol verdicts and staged installation trees. Retain wrappers until §5.4 is met. | Proposed. |
| T13N | Native | Switch one approved workflow step at a time to Layers 2–3. Pin uv setup/version, provision managed CPython and verify identity. Keep old/new conclusions equal for one cycle. No permanent Python-version matrix. | Partial: `ci.yml` test (Linux) and test-macos switched to pinned nextest with the ci profile and separate doctests; exact selections preserved. The implementing PR records the one-cycle CI comparison. Windows stays on cargo test (#950). |
| T13C | Container | Switch separately approved pin workflow steps to Layer 4. Keep the same one-cycle comparison and container identities. | Proposed. |
| T14N | Native | Final repository-wide reconciliation of native consumers and replacement proof. Under §5.4, a superseded script may retire in its implementation PR if it (a) delivers the functional replacement, (b) gives old/new acceptance for the affected behaviour, (c) moves the registry and every live consumer, (d) loses no required test or error handling, and (e) lists retired files by name for maintainer approval of that PR. Frozen evidence and historical documents stay protected. | Proposed. |
| T14C | Container | Final repository-wide reconciliation of container consumers and replacement proof. Under §5.4, a superseded script may retire in its implementation PR if it (a) delivers the functional replacement, (b) gives old/new acceptance for the affected behaviour, (c) moves the registry and every live consumer, (d) loses no required test or error handling, and (e) lists retired files by name for maintainer approval of that PR. Frozen evidence and historical documents stay protected. Keep retired PR branches unless deletion receives separate approval. | Proposed. |

Tooling stages T8 and T9 retain `non-utf8-directory-system`, `non-utf8-directory-user`,
`non-utf8-directory-both` and `non-utf8-directory-zhuyin-both`.
They enforce compatibility-policy row 49's raw Unix path bytes.

Use BDB by default for container comparisons. Run all three cells only when
AGENTS.md rule 2's mechanical path check fires. Rule 3 supplies start-of-sweep
and pre-release coverage. Apply this to §§5.3–5.4 and tooling stages T4–T11.
Use sound tkrzw only. Without a path trigger, suspected backend dependence
is a finding, not permission for extra cells.

The tooling stage's changed paths select Windows acceptance. Native scope
means native execution without a POSIX shell. Container scope means Layer 4
launch plus Linux execution; keep unverified adapters/emulation explicit.
Tooling stage T11 retains its worker loader/memory-probe conditions. Tooling stage T13 needs
separate workflow approval. Historical/retirement rows create no new target.

Tooling stage T3 records content digests for every declared input: fixtures, cases,
configuration, executed runner and harness code. Include artifact identity,
Python version, implementation, executable/build identity and uv version.
Changes invalidate retained results. Missing or uncertain digests invalidate
reuse. Keep original provenance. Never exchange records across environments.
Verify invalidation when contents change at unchanged paths.

Keep the full BDB backstop host an open tooling stage T3 decision. It may run on a
self-hosted runner, scheduled cloud session or another named host, but its pin
work runs inside Layer 4's container. It needs pin/model20, absent from current
CI. Validate the host and choose pre-merge prevention or post-merge detection.
These policies differ. This plan chooses neither. Until the backstop works,
do not narrow any mandatory local gate. Keep sweep and release obligations.

Preserve PASS/SKIPPED/FAIL and gate exit codes. Keep pin/patch/cell checks before
builds, data/model provenance, user/worker isolation, ABI outputs, ordering and
ownership. Keep timeouts, process-group cleanup and exclusive resources.
Keep fixture/scenario bytes, scoring rules, accepted divergences and #767
workloads. Keep one-run measurement. Itemize intentional verdict corrections
and prove each against the original driver; preserve all valid observations.

## 11. Review and authorization gate

This reconciliation changes plans only. It authorizes no implementation,
workflow edit, file retirement, merge or branch deletion. Section 5.4 defines
retirement conditions for approval of a specific future replacement PR.
Python/uv, just and cargo-nextest remain approved host-tool directions.
This documentation change installs or configures nothing.

Before implementation, obtain the required maintainer review and explicit
human approval of the named tooling stage and scope. A clean automated review is not
approval. Keep each later tooling stage's own gate. Do not extend tooling stage T1's separate
authorization. Do not enable auto-merge or request automated reviews here.
Keep closed #836/#837/#842 branches and their exact historical revisions.

## 12. Risks and open decisions

Keep Python out of measured process intervals, or declare it in provenance
for a measurement that intentionally includes it. ctypes signatures can cause
undefined behaviour; §6's tables, ownership and worker isolation guard them.
An abort remains a child observation, not a controller crash.

Layers 1–4 own interpreter, shell, nextest and runtime risks. Do not claim
Windows startup support or adapter/emulation support without acceptance.
Keep §7.5's control threshold open. Keep tooling stage T3's backstop host and timing
policy open. Keep one old/new owner per migration family to prevent permanent
duplication. Historical initbench numbers stay tied to a204f9c0.
The zhuyin apostrophe difference stays an open finding. This plan neither
fixes nor classifies it.

## 13. References and historical records

- [Phase 1](tooling-consolidation-phase1.md),
  [inventory](tooling-consolidation-phase1-inventory.md),
  [summary table](tooling-consolidation-phase1-summary-table.md),
  [upstream survey](tooling-consolidation-phase1-upstream-survey.md).
- [Retirement record](superseded-tooling-prs.md),
  [startup audit](startup-initialization-audit.md),
  [startup baseline](../perf/startup-baseline-2026-10.md).
- [AGENTS.md](../../AGENTS.md), [compatibility policy](compatibility-policy.md),
  [bench procedures](../runbooks/benches.md), [testing policy](../testing/README.md).
- [GitHub stacked pull requests](https://docs.github.com/en/pull-requests/how-tos/stacked-pull-requests)
  and [creation guidance](https://docs.github.com/en/pull-requests/how-tos/create-pull-requests/creating-stacked-pull-requests),
  read on 2026-10-19 UTC. Each PR targets the branch immediately below it.
