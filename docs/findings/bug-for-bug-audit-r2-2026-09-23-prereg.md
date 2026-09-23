# Bug-for-bug audit, round 2 — pre-registration

Written 2026-09-23T10:52:43Z (UTC, `date -u +%FT%TZ` at authoring time),
before any round-2 measurement. This file is committed as the first
commit on `docs/bug-for-bug-audit-r2-2026-09-23` and is **never edited
after that commit**; the report
(`docs/findings/bug-for-bug-audit-r2-2026-09-23.md`) cites it by commit
SHA and records every deviation from it as a deviation, not as an edit.

## 0. Fixed identities

| item | value | how resolved |
|---|---|---|
| subject tree | `origin/main` = `18d782089bd1dff1eec95d92a9897269b011a035` | `git fetch --all --prune` then `git rev-parse origin/main`, 2026-09-23T10:36:29Z |
| libpinyin pin | 2.11.92 at `074a2219c90feaf962d0d24f034514033ece5f99` | `tools/oracle/oracle-pin.txt` and `tools/oracle/build-oracle.sh:27`, which agree |
| ibus-libpinyin pin | 1.16.5 at `2d2cdac0187101aa0cd7ac06694a8340721ddfbb` | same two files, `build-oracle.sh:29-30` |
| toolchain | Rust 1.97.1 | `rust-toolchain.toml` |

Any measurement taken against a different subject SHA, or an oracle
whose `git rev-parse` is not `074a2219…`, is invalid and is discarded,
not reported.

### Divergence classes, as read from the tree before measuring

`docs/findings/compatibility-policy.md` defines four exception classes,
(a) MATH, (b) MEMORY SAFETY, (c) AVAILABILITY, and (d) CONSUMER SCOPE,
and marks (d) RETIRED (2026-09-06; "no new entry may be classified
(d)"). There are therefore **three live classes**. AGENTS.md says the
same. The policy's own lead-in (lines 107-110) still says "one of four
classes" and "There are four, and no others". That wording is an axis-K
candidate, recorded here only as a prediction to be verified.

The **standing divergence** (tkrzw binding-ABI error-origin collapse,
`SYSTEM_ERROR`/`UNKNOWN_ERROR`) is defined in
`docs/findings/tkrzw-langc-exception-classification.md` ("Registered as
a standing divergence … ruled accepted 2026-08-28"). A grep of
`compatibility-policy.md` and `AGENTS.md` for `SYSTEM_ERROR`,
`tkrzw-langc`, `standing` and `information loss` finds no mention of it
in either. Whether the policy register should carry it is an axis-K
question. It is **not** a class, and it justifies only the exact
error-origin collapse it names.

### Anchoring disclosure

This is a blind re-sweep, but it cannot be perfectly blind. Before
Step 3 began, the auditor's session context held a third-party review
summary of round 1 (PR #516). That summary named D1 (the capi pkg-config
Version/subdirectory 2.11.91), a `train(index≠0)` item (D2), D3,
D12–D16, the capi crate `description` prose, the policy's
"four classes" heading, policy row 35 and a `Dockerfile.perf-matrix`
cell label. The mandate itself names several of these as candidates.
Mitigation:
- none of these enters the report without this round's own source or
  execution evidence;
- the round-1 report file (`docs/findings/bug-for-bug-audit-2026-09-23.md`)
  is not on `origin/main`, and neither the auditor nor any subagent
  opens it before Step 3 is complete. Subagent prompts state that
  prohibition.

## 1. Cells

Every behavioural axis runs on each of the three cells, and a result is
reported for the cell that produced it only.

| cell | oracle configure | subject cargo features | data dir |
|---|---|---|---|
| `tkrzw` | `--with-dbm=Tkrzw --enable-libzhuyin` | `--no-default-features --features tkrzw,shipped` | that oracle's installed `lib/libpinyin/data` |
| `bdb` | bare `./configure --enable-libzhuyin` | default features + `shipped` | same |
| `kc` | `--with-dbm=KyotoCabinet --enable-libzhuyin` | `--no-default-features --features kyotocabinet,shipped` | same |

- The subject is the **relinked** object produced by
  `tools/packaging/relink-versioned.sh` from the cargo staticlib, with
  `--features` passed through. The raw cargo-c cdylib is measured only
  on axis A, for the raw-versus-relinked comparison.
- Both `libpinyin` (oxpinyin-capi) and `libzhuyin` (oxpinyin-zhuyin-capi)
  are built and relinked for every cell.
- The environment is a `debian:testing` image pinned by digest, with
  apt pinned to one `snapshot.debian.org` timestamp. Both are recorded
  in the report.
- **bdb cell validity falsifier:** the bare configure must print a
  `BerkeleyDB` DBM selection in `config.log`/configure output, and the
  built `libpinyin.so` must link `libdb`. If either is missing, the bdb
  cell is invalid, which is itself reported as a finding against the
  `ef7f2b41` rationale.

## 2. Verdict vocabulary and decision rules (all axes)

Per (axis, cell, claim), one verdict:

- **MATCH**. All of:
  - the instrument ran to completion on that cell;
  - the output is identical between oracle and subject after the
    normalisations this file names, and there are no others;
  - a pristine re-run on the same cell reproduced the first run byte for
    byte;
  - the axis's injected mutation was **detected** on that cell and its
    revert **confirmed** (section 3).

  Exception: a *pure static byte or set comparison* (header bytes, the
  exported-symbol set, the installed-file set, version-definition
  names) may be MATCH without a mutation. The report must state that
  basis on the row.
- **DIVERGENT-REGISTERED**. A difference is observed, and a register
  row (policy table or `upstream-divergences.md`) covers it. The
  observed scope must be no broader than the row's text, and the row's
  class must be one of (a)/(b)/(c), or the row must be the tkrzw
  standing divergence.
- **DIVERGENT-BROADER**. A registered row covers part of the
  difference, but the observed behaviour exceeds the row's stated
  scope (more symbols, inputs, backends or effects).
- **DIVERGENT-UNREGISTERED**. A difference with no covering row, or
  whose only covering row is class (d), or a row marked REVERT TARGET
  whose defect is still present. Being *better* than the pin is a
  divergence like any other.
- **NOT-ESTABLISHED**. Anything that fails a MATCH precondition:
  - the instrument did not run;
  - the run was nondeterministic;
  - the mutation was not detected;
  - the revert was not confirmed;
  - the cell could not be built;
  - the coverage is incomplete.

  It is never rounded up to MATCH.

Normalisations permitted in diffs, and no others:
1. absolute paths replaced by a cell-relative token;
2. pointer values replaced by `PTR`;
3. process IDs and wall-clock timestamps removed;
4. for axis C only, the registered predicted-candidate tie order,
   compared as a multiset **and** separately as an ordered list. Both
   are reported, and the ordered difference is attributed to its
   register row only if its text covers it.

### Severity (pre-registered)

- **1**: loses or corrupts persisted user state, or causes a process
  crash or abort on one side and not the other along a reachable path;
  or an ABI or packaging difference that makes an unmodified consumer
  fail to load, link or build.
- **2**: a consumer-visible difference on a normal path, such as
  candidate text or order, a return value, an out-param, or exported
  text formats.
- **3**: a difference only on an error or edge path, or only in
  diagnostics; or install or metadata that does not break load, link
  or build.
- **4**: repository prose, register integrity or documentation, with no
  runtime effect.

### Class assignment (pre-registered)

A proposed class must quote the policy's own condition:
- (a) needs a transcendental `gfloat` accumulation in the pinned call
  path;
- (b) needs upstream UB that safe Rust cannot express, and a stable UB
  value is not (b);
- (c) needs an upstream `assert`/`abort` reachable from caller input,
  **and** oxpinyin returning `false`/`Err` **with a log line**. A silent
  swallow is not (c).

Anything else is unregistered.

## 3. Mutation protocol (all axes)

- **Where:**
  - a scratch copy of the subject tree at `18d78208`, at
    `/home/sheng/audit-r2-scratch/mut-src`;
  - scratch copies of installed artifacts (headers, `.pc`, `.ver`)
    under `/home/sheng/audit-r2-scratch/mut-art`.

  Never the worktree, and never the oracle source.
- **Form:** each mutation is one `.patch` file with an ID (`A-m1` …).
  - Source mutations to the Rust subject may be *env-gated*: the patched
    code executes the mutated branch only when
    `OXPINYIN_AUDIT_MUT=<ID>` is set. All gated mutations for a cell are
    then built into one mutant object per cell, so only one rebuild is
    needed.
  - Ungated mutations use one build each.
- **Detection:** the axis instrument, pointed at the mutant (gated
  mutations: with that ID set), must report a difference that the
  pristine run did not. The report records the instrument's exit status
  and the first differing line.
- **Revert confirmation** needs all three:
  1. gated form only: the mutant object run with `OXPINYIN_AUDIT_MUT`
     unset produces output byte-identical to the pristine object;
  2. `git apply -R` of every patch leaves `git -C mut-src diff`
     empty;
  3. the pristine object's SHA-256 is unchanged from the one the
     baseline run used.
- A mutation that is not detected makes that axis NOT-ESTABLISHED on
  that cell. The auditor does not swap in a different mutation after
  the fact to rescue a MATCH. A substitute is allowed only when the
  pre-registered one cannot be applied because the named code does not
  exist, and it is logged as a deviation with the reason.

## 4. Per-axis pre-registration

For each axis: the instrument(s), the MATCH/DIVERGENT rule over and
above section 2, the mutation(s), and the falsifiers. A falsifier is an
observation that, if seen, refutes the conclusion stated with it.

### A. ABI surface

**Instruments.**
- `readelf -d` for SONAME and `ls -l` for the symlink chain;
- `nm -D --defined-only` sets, diffed both ways;
- `readelf -V`, for version definitions and per-symbol version;
- `abidiff` with DWARF on both sides (oracle built `-g`; subject
  staticlib built with `debug = true` for the relink);
- `cmp` of the installed header sets and per-header bytes;
- a C probe `abi-layout.c` that prints `sizeof`/`offsetof`/`alignof`
  for `ChewingKey`, `ChewingKeyRest`, every enum constant's value, every
  typedef width and every object-like macro value. It is compiled
  twice, once against each side's installed headers, and run;
- a field-by-field diff of `libpinyin.pc`/`libzhuyin.pc`, including
  `Version` and the `Cflags` include subdirectory;
- `nm -D` and `readelf -V` of the raw cdylib against the relinked
  object.

**Rules.**
- The symbol-set, version-name, header-byte and `.pc` comparisons are
  static and may be MATCH without a mutation.
- `abidiff` compares a C++ build with a Rust build. Its type-level
  report is expected to be dominated by language differences. It is
  used only for function-symbol and ELF-symbol changes; its type
  section is reported verbatim and is NOT-ESTABLISHED as a parity
  statement.
- The layout probe is the operative layout check.

**Mutations.**
- `A-m1`: relink the subject with a scratch `.ver` that omits
  `pinyin_get_n_phrase`. The `nm` diff must report it missing.
- `A-m2`: change one byte in a scratch copy of the subject's
  `pinyin.h`. The header `cmp` must report it.
- `A-m3`: swap the declaration order of two `ChewingKey` bit-fields in
  a scratch header, then compile the probe against it. The probe diff
  must report changed offsets or layout.
- `A-m4`: set `Version: 2.11.91` in a scratch `libpinyin.pc`. The `.pc`
  diff must report it.

**Falsifiers.**
- The two sides' `.ver` files are not byte-identical to the pin's
  `src/libpinyin.ver`/`libzhuyin.ver` → A is DIVERGENT.
- Any export on one side only → DIVERGENT.
- SONAME ≠ `libpinyin.so.15`/`libzhuyin.so.15` → DIVERGENT.
- The raw cdylib carries `@@LIBPINYIN` versions → the premise of the
  relink step is false, recorded as a finding.

### B. Per-entry-point contract (all exports)

**Instrument.** A new C harness, `contract-battery.c`, kept in scratch.
- It `dlopen`s one side's `libpinyin.so.15` and `libzhuyin.so.15`.
- Every probe runs in a **forked child**, so an abort or signal is
  recorded as a result (exit status or signal number) and does not
  crash the harness.
- Each probe logs:
  - the return value;
  - every out-param's bytes, pre-filled with a `0xA5` sentinel so
    "untouched" can be distinguished from "written";
  - `errno`;
  - the stderr/glib log text.
- The probe classes are those the mandate lists: NULL, empty,
  out-of-range, oversized, error-path return, out-param
  write-on-failure, ownership/free function, aliasing, call-order and
  lifecycle, idempotence, and double-free/UAF.
- Deallocator identity is checked by `dladdr` on the free function the
  API documents, and by an `LD_PRELOAD` malloc/free interposer that
  records which allocator family freed each returned pointer.
- Every row of the export ledger (all exports; the count comes from
  `nm`, not from this file) gets at least one probe, or an N/A citing
  the upstream line that makes the class inapplicable.

**Required differentials.**
- `pinyin_train(index≠0)`: an in-range index and an out-of-range
  index, each preceded by `pinyin_guess_sentence` on both sides. The
  compared object is the user-store export (unigram and bigram
  iterators, and the raw user files' bytes) after save.
- `zhuyin_iterator_add_phrase` with an out-of-range key, compared with
  the same probe on `pinyin_iterator_add_phrase`.

**Rules.**
- Per export, MATCH requires identical probe logs on the cell.
- An abort on the oracle answered by `false` + log on the subject is
  DIVERGENT-REGISTERED (c) only if the log line exists (F cross-check).
  If the subject returns silently, it is unregistered.

**Mutations.**
- `B-m1`: gated. In `pinyin_get_candidate`, the out-of-range-index
  branch returns `TRUE` instead of `FALSE`.
- `B-m2`: gated. `pinyin_train` skips the user-bigram write for
  `index≠0`.

**Falsifiers.**
- Any export with no row → B incomplete, NOT-ESTABLISHED.
- A probe whose oracle result differs across two runs →
  nondeterministic, NOT-ESTABLISHED for that probe.

### C. Output equivalence

**Instruments.**
- `tools/bisection/run-same-data-dir-diff.sh` with every default
  driver, each cell's own oracle data dir, and
  `OXPINYIN_ALLOW_MINI_FIXTURE` unset (a run that falls back to the
  mini fixture is invalid);
- `run-option-sweep.sh` / `option-sweep.c`, extended in scratch to
  cover every `pinyin_option_t` bit, all pairwise combinations,
  reserved bits and invalid bits;
- `run-scheme-diff.sh` (double-pinyin and zhuyin schemes);
- `run-zhuyin-diff.sh`;
- `run-punct-diff.sh`;
- `run-live-typing-diff.sh` (incremental parsing);
- a scratch encoding battery: invalid UTF-8, NFC vs NFD, full-width
  vs half-width, embedded NUL, and maximum lengths;
- a ü/v/`u:`/tone/abbreviation/auto-correction battery;
- a libzhuyin behavioural differential over every keyboard layout.

**Rules.** Section 2.

**Mutations.**
- `C-m1`: gated. `PINYIN_CORRECT_V_U` is ignored. The option sweep
  must detect it.
- `C-m2`: gated. The first two non-sentence candidates of every
  `pinyin_guess_candidates` are swapped. The same-data-dir drivers must
  detect it.
- `C-m3`: gated. One zhuyin keyboard-layout key maps to a different
  symbol. The zhuyin differential must detect it.

**Falsifiers.**
- A runner exits 0 while its log says SKIP or mini fixture → invalid,
  NOT-ESTABLISHED.
- Any bit in `pinyin_option_t` that the sweep does not reach → C
  incomplete.

### D. Persisted state

**Instruments.**
- `tools/oracle/user-dir-round-trip.sh` in both directions (oracle
  writes, subject reads and continues; and the reverse), run per cell;
- a scratch corruption battery, which runs each user file in each
  state through both sides and compares the open result, the
  diagnostics and whether the file is rewritten. The states are:
  corrupt, truncated at every 1/8 boundary, empty, wrong-version
  header and newer-version header;
- `strace -f -e trace=%file,%desc` on save, to compare temp+rename
  against in-place writes, `fsync`, and the exact sequence of file
  syscalls;
- `stat -c %a` of every created file and directory, under umask 022
  and 077;
- directory-creation behaviour when the user dir is missing, or its
  parent is missing;
- path derivation and every environment override read (cross-checked
  with H);
- import and export text formats, compared byte for byte.

**Mutations.**
- `D-m1`: gated. The user unigram count is written +1 on save. The
  round-trip must detect it.
- `D-m2`: gated. Created user files get mode 0600. The mode comparison
  must detect it.
- `D-m3`: gated. The export writer emits a trailing space on each row.
  The byte comparison must detect it.

**Falsifiers.**
- The round-trip is run only one way → incomplete.
- The oracle and subject read different directories → invalid.

### E. Upstream defect preservation

**Catalogue.**
- `docs/findings/upstream-report-drafts.md` (items counted from the
  tree, by a recorded command);
- `reference/memory-safety-bugs.md`;
- `docs/findings/robustness-evidence.md`;
- the trainer report (`docs/findings/trainer-replacement-report.md` and
  `trainer-parity-audit.md`);
- upstream issues #566, #542 and #518, read from GitHub at the pin's
  repository;
- the general classes: overflow and wraparound, signed/unsigned
  confusion, off-by-one, iteration-order leakage, sort instability,
  locale dependence, and silent success on failure.

**Rules.**
- Each defect gets exactly one verdict:
  - **reproduced**: the subject shows the same observable behaviour;
  - **silently fixed**: DIVERGENT-UNREGISTERED;
  - **fixed under a cited class**, which must satisfy section 2's class
    rules.
- A defect with no executable reproduction against the oracle is
  NOT-ESTABLISHED, not "reproduced".

**Mutation.**
- `E-m1`: selection rule fixed now. Take the first catalogue item, in
  catalogue order, that the harness reports as **reproduced** on the
  subject. Implement its "fix" as a gated mutation. The harness must
  then report it as silently fixed.

**Falsifier.** Any catalogue item without a verdict → E incomplete.

### F. Errors and diagnostics

**Instruments.**
- A recount of every `assert(` and `abort()` site in the pin's `src/`,
  by a recorded command (`grep -rnE '\bassert\s*\(|\babort\s*\(\s*\)'`
  over `src/`, and then separately excluding `g_assert`/`static_assert`
  and comments, with both counts recorded).
- For each site: its condition, whether caller input reaches it (by a
  cited call path, or by an executed trigger), the trigger, oxpinyin's
  handling and the class.
- The glib log domain, log level and message text of every warning
  on both sides, captured through `G_MESSAGES_DEBUG=all` and a
  `g_log_set_default_handler` shim for the oracle, and the subject's
  own logging sink.
- OOM paths, exercised through an `LD_PRELOAD` failing-malloc
  interposer at a fixed N-th allocation.
- Error-information loss at the C boundary, **per symbol**: for each
  export, enumerate the distinct failure causes on the oracle side and
  whether the subject distinguishes each one the same way (return value,
  out-param, log). The tkrzw standing divergence is the only
  pre-authorised loss, and only at the exact error origin it names.

**Mutations.**
- `F-m1`: gated. The log line at one class-(c) site is removed. The
  diagnostics diff must detect it.
- `F-m2`: gated. The same site returns `TRUE`. The contract battery
  must detect it.

**Falsifiers.**
- The site count differs between two runs of the recorded command →
  instrument error.
- Any site left unclassified → F incomplete.

### G. Numerics

**Instruments.**
- The §12 gate: `crates/pinyin-oracle/tests/sentence_surface_parity.rs`
  `sentence_surface_matches_the_declared_residual`, run per cell with
  `--include-ignored`. It needs `PINYIN_EXPORT_DIR` from that cell's
  oracle and the verified model20 cache.
- A re-derivation from source of each of these on both sides, each
  with file:line cites:
  - float width and accumulation order;
  - the `log` source (which libm symbol, and which crate or function
    on the subject);
  - the lambda and mixture parameters;
  - pruning thresholds and beam width;
  - comparator tie-breaks.

**Rule.** The expected residual is 491/396/390 of 496 on every cell.
Any other value on any cell is a finding on that cell, in either
direction.

**Mutations.**
- `G-m1`: gated. Every n-best step cost is increased by 1e-3 nats. The
  gate must report a moved residual.
- `G-m2`: gated. The trellis comparator's tie-break order is reversed.

**Falsifier.** The gate reports "skipped" or passes with no oracle
input → invalid.

### H. Concurrency and process state

**Instruments.**
- A static inventory of synchronisation primitives and process-global
  state on both sides:
  - Rust: `static`, `Mutex`, `OnceLock`, `thread_local!` and atomics
    in every shipped crate;
  - C++: `static` data and globals in the pin's `src/`.
- A two-thread harness that shares one context and one instance
  (upstream-unsafe patterns included), run under the oracle and the
  subject, comparing the result, crash or abort, and TSan/helgrind
  output.
- A fork harness: the parent initialises, the child uses and saves,
  and the parent then saves.
- An async-signal context probe; this is documentation-level if upstream
  makes no claim.
- A `getenv`/`secure_getenv` `LD_PRELOAD` interposer, which logs every
  environment variable read during a scripted session on both sides.
- `setlocale` dependence: the same session under `LC_ALL=C`,
  `C.UTF-8`, `zh_CN.UTF-8` and `tr_TR.UTF-8`.
- An init/deinit ordering probe: fini before init, double fini,
  instance outliving context, and so on.

**Rule.** Being safer than upstream (for example, an internal lock
where the pin races) is a divergence and must be classified. It is
never an automatic MATCH.

**Mutations.**
- `H-m1`: gated. One extra `getenv("OXPINYIN_AUDIT_H")` call on
  `pinyin_init`. The interposer inventory must detect it.
- `H-m2`: a scratch-only static `Mutex` added to the facade. The static
  inventory must detect it.

**Falsifier.** The interposer sees zero reads on the oracle → the
interposer is not wired, invalid.

### I. Drop-in integration

**Instruments.**
- The installed file set and paths: `make install DESTDIR` on the
  oracle, compared with the release staging on the subject
  (`tools/packaging/install.sh` / `release-stage.sh`);
- a header compile matrix: C `-std=c89/c99/c11/c17` and C++
  `-std=c++11/14/17/20`, each with `-Wall -Wextra -Werror`, compiling
  a TU that includes each installed public header, on both sides;
- `pkg-config --modversion/--cflags/--libs` and
  `--atleast-version=2.11.92`, end to end;
- a relink and `LD_PRELOAD` replay of consumers:
  - the pinned-frontend fixture (`tools/bisection/frontend-import.cc`
    over the ibus-libpinyin 1.16.5 objects);
  - ibus-libpinyin at the pin, driven headless with the same key script
    on both sides where it can be driven;
  - fcitx5-oxpinyin, if its source is available on the host.

  Runtime behaviour is compared, not just a successful link.

**Mutations.**
- `I-m1`: a `//` comment injected into a scratch copy of `pinyin.h`.
  The c89 cell of the matrix must fail.
- `I-m2`: one installed file removed from the scratch staging. The
  file-set diff must detect it.
- `I-m3`: the gated `C-m2` rank swap. The consumer replay must detect
  it.

**Falsifier.** A consumer that cannot be driven headless → that consumer's
row is NOT-ESTABLISHED, with the blocker stated.

### J. Coverage

**Instruments.**
- An export ledger generated from `nm -D` of the oracle's `.so` files,
  with every row mapped to the B, C and D evidence that covers it.
- An internal ledger:
  - every upstream function transitively reachable from an export,
    taken from a call graph of the pin (`clang -emit-llvm` + `opt
    -passes=print-callgraph`, or `cg-calls.py` over the pin's objects;
    the command is recorded);
  - each function mapped to its oxpinyin counterpart (`path:line`) or
    marked deliberately absent with a cited reason.

**Rules.**
- The export ledger is a static set comparison, and MATCH is allowed
  when the set is complete.
- The internal ledger is COMPLETE, PARTIAL (with the unmapped count) or
  NOT-ESTABLISHED.

**Mutation.** `J-m1`: one export row deleted from a scratch ledger copy.
The completeness check must flag it.

**Falsifier.** A ledger count that the recorded command cannot
regenerate → invalid.

### K. Register reconciliation

**Instrument.** Every row of the policy table and of
`upstream-divergences.md`, set against the divergences this round
observes. The output is three lists:
- unregistered;
- broader than registered;
- stale, meaning the row asserts a state the code no longer has.

Also:
- register-integrity checks: class totals against row counts, row
  status against the code, and cross-references resolving;
- a repo-wide sweep of behaviour-asserting prose (crate `description`s,
  `.pc.in`, workflow comments, Dockerfile labels, and docs) compared
  with the observed behaviour.

**Rule.** K produces lists and not MATCH. Every list entry is backed by a
source cite or an executed observation. **Mutation:** none, because K is
static reconciliation.

**Falsifier.** A register row cited in a verdict elsewhere and missing
from K's walk → K incomplete.

### L. Complexity

**Instrument.** For each internal structural divergence identified in J
and in the source read, a differential baseline on each cell. The
baseline measures:
- wall time (median of 11 runs, pinned to a single CPU with `taskset`);
- peak RSS (`/usr/bin/time -v`);

over these workloads:
- the §12 input set through guess plus the sentence;
- training on 200 inputs;
- import/export of 10k phrases;
- a cold open.

It also cites the asymptotics on each side.

**Rule.** "Both worsened" means the subject median time is above
1.10× the oracle's **and** the subject peak RSS is above 1.10× the
oracle's on the same workload and cell, outside the run-to-run spread.
That is a finding. Either one alone is recorded as a trade.

**Mutations.**
- `L-m1`: gated. A busy loop proportional to the input length squared
  in `pinyin_guess_candidates`. Time must flag it.
- `L-m2`: gated. A retained 64 MiB allocation on `pinyin_init`. RSS
  must flag it.

**Falsifier.** A spread above 10% on the oracle's own repeated runs →
that workload is NOT-ESTABLISHED.

## 5. Global falsifiers

1. The pristine re-run on a cell is not byte-identical to its first run
   → every axis that used that instrument is NOT-ESTABLISHED on that
   cell.
2. Any measurement artifact carries a timestamp earlier than this
   file's commit → the pre-registration was breached, and the report
   logs it as a hygiene failure.
3. The subject's `libpinyin.so.15` is not the relinked object with
   `shipped` → invalid for that cell.
4. A subagent's enumeration enters the report without a recorded
   spot-check against source → invalid, and the claim is removed.

## 6. Budget rule

If the session ends before an axis × cell is closed:
- the partial report is pushed;
- one `verdict:not-established` issue is filed per unfinished axis ×
  cell, naming where the work stopped and the next command to run;
- the report's first section says so plainly.
