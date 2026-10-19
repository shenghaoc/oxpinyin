# Superseded tooling PRs #836, #837 and #842 — preserved revisions and recovered work

> **Status: historical record.** These three PRs were closed unmerged on
> 2026-10-18 UTC under the maintainer's decision to adopt Python 3 for
> developer-tool orchestration and `just` as the cross-platform task
> interface. Their branches were not deleted and their exact head revisions
> remain reachable through both the branches and GitHub's
> `refs/pull/<n>/head` refs. Their reusable requirements, test cases,
> measurement definitions and verified findings are recorded here and in
> `tooling-consolidation-python-just.md`. This document is not a description
> of current `main` and authorises no implementation.

Dates are UTC, captured from the executing host with `date -u`. The closure
times below are GitHub's `closedAt` values.

---

## 1. Closure state

| PR | Title | Branch (retained) | Base | Exact head | State / closed |
| --- | --- | --- | --- | --- | --- |
| [#836](https://github.com/shenghaoc/oxpinyin/pull/836) | `refactor(abi): extract the generic libpinyin vocabulary into libpinyin-abi` | `abi/extract-foundation` | `main` at `ecd9a7e5b70ec2b4bc0ec0535b69250e1059bc81` | `ac74b77815fa9cd3e1bd26485daaa28df32ef3f0` | CLOSED 2026-10-18T20:15:55Z, not merged |
| [#837](https://github.com/shenghaoc/oxpinyin/pull/837) | `feat(abi): add a dynamically loaded Subject for libpinyin and libzhuyin` | `abi/subject` | `abi/extract-foundation` at `ac74b778` | `8f088191176e8c57e270bacaa6e091bb57f1f1ca` | CLOSED 2026-10-18T20:15:52Z, not merged |
| [#842](https://github.com/shenghaoc/oxpinyin/pull/842) | `initbench: independent init/startup benchmark vs pinned libpinyin` | `claude/tooling-language-consolidation-dvahgz` | `main` at `ecd9a7e5` | `a204f9c0e6874eee9d08795fe77a42b7a25da1d6` | CLOSED 2026-10-18T20:15:58Z, not merged |

GitHub's pull-request refs resolve to the same commits:

| PR | `refs/pull/<n>/head` |
| --- | --- |
| #836 | `ac74b77815fa9cd3e1bd26485daaa28df32ef3f0` |
| #837 | `8f088191176e8c57e270bacaa6e091bb57f1f1ca` |
| #842 | `a204f9c0e6874eee9d08795fe77a42b7a25da1d6` |

Closure was performed without `gh pr merge`, without `--delete-branch`,
without enabling auto-merge, and without a CodeRabbit review request. The
branches are therefore still on `origin`; the pull-request refs are the
second, independent preservation path.

### 1.1 Commits preserved

| PR | Commit | Subject |
| --- | --- | --- |
| #836 | `ac74b77815fa9cd3e1bd26485daaa28df32ef3f0` | `refactor(abi): extract the generic libpinyin vocabulary into libpinyin-abi` |
| #837 | `a3cad350aa344337fae00d8cb9a3423e144481d9` | `feat(abi): add a dynamically loaded Subject for libpinyin and libzhuyin` |
| #837 | `8f088191176e8c57e270bacaa6e091bb57f1f1ca` | `fix(abi): release-or-refuse handle lifecycle, !Send subject, unsafe allowlist` |
| #842 | `7834d535ab1581bb5221d4e122e51941741ac3a2` | `initbench: independent init/startup benchmark of two built libpinyin artifacts` |
| #842 | `02c40816528f157c30ebd1a9e017c8343b7e4ee3` | `initbench: split run and run_units to satisfy the complexity gate` |
| #842 | `a793670d66d4fb34be0a93db6af2e9ef6b16435a` | `initbench: accept the documented refusal on hosts without a memory probe` |
| #842 | `333598252238457863346e696ef74ad49a3a4da1` | `initbench: address review findings` |
| #842 | `a204f9c0e6874eee9d08795fe77a42b7a25da1d6` | `initbench: correct fixture terminology, scope wording and the zhuyin finding` |

The earlier #837 head `a3cad350` is not an orphan: it is the first commit of
the retained `abi/subject` branch, followed by the refinement `8f088191`.
The #842 branch is a self-contained replacement of an earlier stacked
implementation; the five commits above are the final series.

---

## 2. Why these PRs are superseded

The maintainer adopted a different implementation language for the **developer
tooling**, not a different understanding of the problem:

- Python 3 owns orchestration, discovery, subject driving, observation,
  comparison, reporting and measurement control.
- `just` is the single cross-platform task interface.
- The Rust ABI foundation (`libpinyin-abi`, the dynamic `Subject` on top of
  it) is **not** a required component. Its semantics become Python
  requirements.
- The standalone Rust `initbench` is **not** a required component. Its
  measurement definitions and findings become the specification for a Python
  controller plus a small native worker.
- Native Rust/C workers are retained or added only where their functionality
  and measurement fidelity justify that worker.

This does **not** invalidate the historical findings. A finding remains true
for the tree and artifact it names even when the tool that produced it is no
longer the chosen tool. The closed PRs remain the evidence.

---

## 3. #836 — generic vocabulary and capture wire format

### 3.1 What it delivered

A workspace crate `libpinyin-abi` with no dependencies and no oxpinyin crate,
holding the parts of `pinyin-oracle` that are generic over any libpinyin:

- `pinyin_option_t` bits and the sort word, including the validated capture
  profile `0x18a` and the `0x1e` sort constant, with `DYNAMIC_ADJUST`
  rejected rather than masked.
- Segment, completeness and candidate-type vocabulary, with the two facades
  kept as different types (libzhuyin's four candidate-type values are not
  libpinyin's eight).
- The `pinyin-capture-v1` escaping and field notation: `CAPTURE_SCHEMA`,
  `escape`/`unescape`, `CaptureRecord`, `read_fixture`,
  `segment_to_wire`/`segments_to_wire`, and the
  `CaptureFieldMalformed { field, value }` error shape.
- A `pinyin-oracle` re-export of every moved item under its old name, with
  unchanged public paths, variants, signatures and `Debug` output.

It added 13 crate unit tests and `tests/abi_reexports.rs` (re-exports are the
same types; the error shape is preserved). No fixture, golden, wire format,
CI workflow or tool changed. The crate was `#![forbid(unsafe_code)]`.

### 3.2 Recovered requirements

The Python core must keep these, in one module each:

1. The option bits, the sort word and the `0x18a` capture profile; the oracle
   policy still rejects `DYNAMIC_ADJUST`, never silently masks it.
2. The segment/completeness/candidate-type vocabulary, with the facades not
   unified into one enum.
3. The capture wire format byte for byte, including escaping, field notation,
   fixture reading and the field/value error payload.
4. Existing `pinyin-oracle` public names remain valid if a future Python
   module is used alongside it; no consumer is asked to change.
5. Tests assert the same **bytes**, not merely that parsing succeeds.

### 3.3 Recovered test cases

- `tests/abi_reexports.rs` — the re-exported items are the same types and the
  generic parse failure still maps to the same field and value.
- The 13 crate unit tests, covering constant values, candidate-type mapping,
  escape/unescape round trips, capture record parsing, segment rendering,
  malformed input and unknown candidate values.
- The `pinyin-oracle` suite is unchanged by the extraction: 159 result lines,
  107 passed / 3 ignored on pristine `main` and on the branch, plus the 3 new
  tests (110 passed).

### 3.4 Disposition

The crate is not adopted. Its vocabulary and wire-format requirements move
into the Python `subject`/`observation` core; the Rust re-export exercise is
not repeated. The exact revision remains available for reference at
`ac74b77815fa9cd3e1bd26485daaa28df32ef3f0`.

---

## 4. #837 — the dynamic `Subject` semantics

### 4.1 What it delivered

A reusable, oxpinyin-independent Rust ABI foundation in `libpinyin-abi`:
open a libpinyin/libzhuyin shared object by path with `libloading`, then a
checked first slice per facade (context lifecycle, options, parsing, sentence
guess, candidate list). It was a library only — no CLI, comparison, reporting
or 131-symbol rewrite.

Its refinements in the second commit are the important ones:

1. **Unsafe policy** narrowly allowed the `subject` module; every block had a
   `// SAFETY:` comment and the two enforcing lints were denied.
2. **Owned-handle lifecycle:** resolve `*_fini` before `*_init` and
   `*_free_instance` before `*_alloc_instance`; a missing destructor means
   nothing is created. Every created handle is released even when a later call
   fails.
3. **Loader and lifetimes:** the sentence is caller-owned and freed with
   `g_free`; candidate strings are copied, never freed; a missing `g_free`
   fails before the library is asked; `Subject` is not `Send`/`Sync`.
4. **Guard:** a failed load releases its slot; the default open refuses a
   second subject before touching the file; permanent loading is documented
   with its cost and the process-global-state consequence.
5. **Wording:** Stage 1 is a reusable library foundation, not a tool.

### 4.2 Recovered requirements

The corrected requirements follow `tooling-consolidation-python-just.md`
section 6.2. They preserve the recovered semantics with the amendments below:

- open by path; report, never crash;
- per-symbol presence; a missing symbol fails only the operation needing it,
  and a library is never rejected for lacking another revision's symbol;
- distinct pinyin/zhuyin facades and candidate types; the zhuyin sentence has
  no index;
- one library per process by default; a second live subject is refused before
  touching the file; an explicit opt-out exists;
- do not mask aborts; use a child process for untrusted input;
- no sharing across threads; use processes for live subjects;
- release-or-refuse lifecycle; every created handle released;
- borrowed strings copied, owned sentence freed with the library's `g_free`,
  fail before asking if it is absent;
- never unload; process-global state needs a fresh process to reset;
- pass Unix C path arguments as raw bytes. Refuse only an interior NUL
  in a C path. Check lengths and counts. On Windows, report an
  unrepresentable path if the platform cannot supply path bytes;
- the refusal taxonomy carries the input that caused it.

### 4.3 Recovered test cases

The full list is in `tooling-consolidation-python-just.md` section 6.2. The
source tests are, in the crate:

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

The recovered real-library integration test requires
`LIBPINYIN_ABI_PINYIN_SO`, `LIBPINYIN_ABI_SYSTEM_DIR` and
`LIBPINYIN_ABI_ZHUYIN_SO`. A missing input fails the test. The #837 original
skipped zhuyin when its variable was absent; the recovered test does not.
It checks both facades and prints one stable line per observation.
Run one process per library to compare libraries.

### 4.4 Verified findings from this PR

- The mock-based suite passed 33 unit tests; the real-loader tests ran against
  a system library per OS.
- With the real pin (BDB, libpinyin + libzhuyin 2.11.92) and oxpinyin's capi
  objects, each in its own process and on the same data dir, the 11-line log
  was byte-identical between the two and identical to the pre-refinement run.
  This is slice-level evidence on nine inputs, not an equivalence claim.
- `cargo tree -p libpinyin-abi` was `libloading` → `cfg-if` with zero
  oxpinyin crates; the crate also built and passed all 33 tests when copied
  out of the workspace.
- macOS and Windows were not run locally; the portable lanes built and tested
  the workspace. GLib fallback names were untested on macOS/Windows.
- Out of scope by design: user-data/export iterators, double-pinyin/zhuyin
  schemes, key/rest queries, training, addon libraries and any comparison,
  reporting or CLI layer.

### 4.5 Disposition

The crate and its unsafe allowlist are not adopted. The semantic rules and
the test names are recovered as requirements for the Python `subject` module.
The exact revision remains at `8f088191176e8c57e270bacaa6e091bb57f1f1ca`.

---

## 5. #842 — startup and initialization measurement

### 5.1 What it delivered

A self-contained Rust tool, `oxpinyin-initbench`, that `dlopen`s a pin-built
and an oxpinyin-built `libpinyin`/`libzhuyin` and measures where
initialization time and memory go: library load and symbol resolution,
`*_init`, option setup, `*_alloc_instance`, startup memory, and the time to
the first usable candidate as the outer startup boundary. It did not measure
typing, steady state or training and did not modify production code.

The final PR also added three documents:

- `docs/findings/startup-initialization-audit.md` — the source-and-binary
  audit of the initialization path, the pin's `modest` anomaly and
  hypotheses, candidates for reducing init cost, and the unresolved zhuyin
  finding;
- `docs/perf/startup-baseline-2026-10.md` — the Linux/BDB one-host
  baseline with equivalence, timing, instructions/allocations and memory;
- `docs/runbooks/initbench.md` — the procedure, units, refusal rules and
  tests.

Those documents are recovered into this repository by the corrective PR
(with a banner saying the Rust tool is not on `main`). The runbook is not
copied as a procedure for a tool that does not exist; its content is folded
into the recovered capability in the amendment.

### 5.2 Recovered requirements and measurement definitions

The full specification is in `tooling-consolidation-python-just.md` section 7.
The essentials:

- **Scope:** initialization and startup only, with the first usable candidate
  as the outer boundary; no steady state, typing or training.
- **Equivalence before performance:** the subjects must first produce
  identical complete observations through the same code path as the timed
  child. A mismatch is `NOT COMPARABLE` and gets no ratio; a timed sample
  that contradicts the verified transcript is rejected, not averaged.
- **Units U1–U6:** load/resolve; `p_init`; `p_set_options(0x18a)`;
  `p_alloc_instance`; first candidate (`p_parse_more_full_pinyins("nihao")`,
  pinyin also `get_parsed_input_length` and
  `guess_candidates(inst, 0, 0x1e)`, zhuyin
  `guess_candidates_after_cursor(inst, 0)`, then count/candidate/type/string);
  teardown `p_free_instance` then `p_fini`.
- **Stop points:** `start`, `lib_load`, `init`, `config`, `alloc`,
  `first_candidate` skip teardown; `end` runs U6 (`p_free_instance` then
  `p_fini`), as `workload.rs::run_units` does at #842 `a204f9c0`.
- **Timing:** one monotonic clock; phase times; cumulative timeline; `ttfc`
  = first-result end − load start, including inter-phase gaps; the two U6
  calls are timed separately and excluded from `ttfc`.
- **Memory:** RSS delta, peak RSS, anonymous, file-backed, private dirty, PSS,
  virtual size, minor/major faults; read between phases only in a memory
  sample; a memory sample's timings are not aggregated as timings.
  On macOS, report RSS and virtual size only. Record other memory metrics
  as unavailable, never zero. Exclude unavailable metrics from comparisons.
- **Profiles:** `empty`, `modest` (80 import calls, 2 distinct phrases,
  frequency 5), `phrases` and `journals` (10,000 distinct phrases at
  frequency 20,000; `journals` spreads them over all four libraries with
  quota 3000/3000/3000/1000). They are different shapes, not a size-scaling
  series. A fixture has `user/`, `profile.manifest`
  (`oxpinyin-startup-profile-v1`) and optional `witness.txt`; every sample
  gets a private copy; the manifest is checked before and after.
- **Inputs:** 18 main-script inputs (ending with a repeated `nihao`), 3
  chewing inputs. Run the 12 hostile inputs as separate safety probes,
  each in its own process. Their outcomes remain outside the transcript
  that controls ratios.
- **Observation:** every return value, parsed length, sentence, ordered
  candidate types and text, stderr, user-directory files, and save/reopen
  persistence. Record candidate n-best indexes for pinyin only.
  For zhuyin, record "not available on this facade". Use only the public
  C API; do not derive this field from other data.
- **Statistics:** median, 95% CI, range/median spread, `n`; ratios only
  between comparable subjects; warm-up excluded; rejected samples excluded.
- **Run shape:** 60 timed rounds with 3 warm-up; 30 memory rounds with 2
  warm-up; optional cold cache and lazy binding at 30 rounds; CPU pin; one
  process per sample; alternate subjects; always include a same-bytes control
  and report its ratio as the noise floor.
- **Provenance:** record each timed object's full SHA-256, size, host and CPU.
  On Linux, record ELF `NEEDED` from `readelf -d`.
  On macOS, record Mach-O `LC_LOAD_DYLIB` and `LC_ID_DYLIB` from
  `otool -L` / `-D`. Record unavailable fields explicitly.
  Never use another run's or host's absolute figures as a baseline.

### 5.3 Recovered test cases

- Process-level refusals: missing library, a file that is not a shared
  object, a real library without the facade's symbols (named, not silently
  accepted), and bad invocations (no subcommand, unknown subcommand, missing
  or unknown options, invalid facade/stop/kind/bind, malformed subject,
  missing report input, missing profiles).
- Live comparability: six verdicts (two facades × empty, modest and the
  persistence scenario), each comparable, transcript > 100 lines, hostile
  probes for both subjects and both facades.
- Timed-sample reproduction: 3 rounds + 1 warm-up × 2 subjects × 2 facades;
  every sample has `ttfc` and a first candidate; no timed run contradicts
  its verification; the fixture is untouched.
- Mutation detection: an unchanged re-run compares equal; a changed workload,
  a tampered answer (the difference names the line), a changed trace digest
  and a populated-vs-empty profile are each detected.
- Unit families: sequencing, timing boundaries, validation and errors,
  comparison and mutation detection, profile manifests and copies, memory
  probes, statistics, report ratios and labels, child failure/timeout/digest.

### 5.4 Verified findings

The recovered baseline documents contain medians and limitations.
The confidence intervals and spreads are unavailable. The headline results
apply to the named artifacts and host:

- Library load: oxpinyin 0.50× the pin in time; 0.82 M against 4.12 M
  instructions.
- `empty`: time to first candidate 0.71× pinyin, 0.73× zhuyin.
- `modest`: oxpinyin `*_init` 0.27× the pin; the pin's own `modest` init
  is slower than its `phrases` init (6.7 ms / 11.8 M Ir / about 8.3 MiB anonymous
  against 3.1 ms / 10.3 M Ir / 0.8 MiB).
- `phrases`/`journals`: oxpinyin `*_init` 6.8–8.3× the pin (about 21 ms
  against 3 ms; 141 M Ir and 268 k allocation calls against 10 M and 1.3 k).
- Same-bytes control: 0.94–1.05× for init and first candidate, the noise
  floor.
- Pin-only Callgrind: the `modest` extra cost is journal replay
  (`SubPhraseIndex::merge` → `add_phrase_item` →
  `MemoryChunk::set_content`); why the journals take different paths is a
  hypothesis. Oxpinyin per-function attribution was not captured.
- **Unresolved:** zhuyin `'` and `''` — pin false, oxpinyin true with no
  sentence, at no option word, `0x20` and `0x18a`. It contradicts
  `compatibility-policy.md` row 53 item 2 (recorded as closed in code), is
  not fixed, not classified acceptable, and is outside the timed workload.
  It must be checked against the register by a maintainer.
- Raw captures were not retained; the capture environment was an ephemeral
  container, and the baseline says so at the point of use.

### 5.5 Disposition

The standalone Rust tool is not adopted. The recovered capability is a
Python controller plus a small C worker that keeps Python out of the timed
process; the recovered definitions and findings are the acceptance
specification. The exact revision remains at
`a204f9c0e6874eee9d08795fe77a42b7a25da1d6`.

---

## 6. What is not adopted

- `crates/libpinyin-abi` as a required crate, its `libloading` dependency,
  its unsafe allowlist, and its re-export obligation for `pinyin-oracle`.
- The dynamic Rust `Subject` as a required component.
- `crates/oxpinyin-initbench` as a required crate or standalone tool.
- Any claim that the Rust implementation is the only way to state or test the
  recovered semantics.

## 7. What is preserved

- The three branches and the three `refs/pull/<n>/head` revisions.
- The commit history listed in section 1.1.
- The requirements, test cases, measurement definitions and findings in this
  record and `tooling-consolidation-python-just.md`.
- The recovered audit and baseline documents in this repository.
- The historical numbers in `docs/perf/perf-init-user-profile-2026-10.md`,
  which used the #842 tool at `a204f9c0`; they remain valid for that artifact
  and are not reattributed.

## 8. References

- `tooling-consolidation-python-just.md` — the corrected architecture,
  decisions and stages.
- `startup-initialization-audit.md` — recovered from #842.
- `../perf/startup-baseline-2026-10.md` — recovered from #842.
- `tooling-consolidation-phase1.md` — the historical Phase 1 report.
- `compatibility-policy.md` — row 53 item 2, the unresolved zhuyin
  difference.
