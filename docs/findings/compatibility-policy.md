# Compatibility policy — what oxpinyin may diverge on, and what it may not

Date: 2026-08-28 · Status: **policy** (maintainer-decided; this document
records the decision and classifies the existing register against it) ·
Branch: `claude/pr2-compatibility-policy` (#203). **Amended 2026-09-06:**
the classification table re-synced with `upstream-divergences.md` at
`87f25055` — every register entry now has a row, closures since
2026-08-28 are recorded, and the "PR 5" the original text named is
`docs/findings/revert-plan.md` (#209, the work order), which never
became a PR of its own; the reverts landed one by one (see the table).
**Amended 2026-09-12 (maintainer):** the E2E rule's pin is libpinyin
2.11.92 at `074a2219` — the repository is feature-complete, at measured
parity under this policy's exceptions with one open defect (row 30, the
pinyin facade's chewing batch `FORCE_TONE` seam), and the pin was moved
to libpinyin's latest commit on 2026-09-06
(`docs/testing/oracle-environment.md`); and the goal amendment's task-9
gap closed on 2026-09-09, and row 2's basis no longer cites the
cancelled migration tool. **Amended 2026-09-09:** the goal's byte-level
guarantee is per KV
backend family — same-backend pairs (Kyoto Cabinet↔Kyoto Cabinet,
tkrzw↔tkrzw, oxpinyin↔libpinyin either direction) interoperate
seamlessly, user data included; data loss when the KV database backend
actually changes is taken for granted (maintainer ruling; recorded in
place under "The goal this policy serves"). **Amended 2026-09-14:** the
one defect the 2026-09-12 amendment carried — row 30 — is closed in
code (the table and totals below carry the state; the live FORCE_TONE
differential ran 2026-09-16 — all eight implemented keyboards
IDENTICAL, register amendment in `docs/findings/upstream-divergences.md`).
**Amended 2026-09-27 UTC (lane G, register integrity after the round-2
audit, #573):** five human rulings are recorded with their sources in
"Amendment — rulings recorded" at the end of this document; the
reference build, the drop-in version, class (c)'s obligations, the
`LC_NUMERIC` side effect and the evidence standard follow from them.

## The goal this policy serves

oxpinyin is a **drop-in replacement**: rename the built shared object to
`libpinyin.so.15`, put it on the library path, and existing consumers
work against the data already on the system. Not a compatible
reimplementation a consumer is ported to — the same binary interface,
the same file formats, the same observable behaviour, with the consumer
unchanged and unaware.

> **Amended 2026-09-09 — same-backend user data is seamless (maintainer
> ruling).** The guarantee is per KV database backend family: an
> oxpinyin built on Kyoto Cabinet and a libpinyin built on Kyoto
> Cabinet — likewise the tkrzw pair — read and write the same user
> dir, and a swap in either direction carries the learned data with
> it. oxpinyin reads the user state libpinyin left (`user_bigram.db`,
> `user_pinyin_index.bin`, `user_phrase_index.bin`, `user.bin`, the
> `*.dbin` diff logs, `user.conf` — characterized at the pin
> `074a2219`: `pinyin_internal.h:55-66`, `_write_files`/
> `_rename_files` `pinyin.cpp:922-1130`, the per-library user
> filenames in `data/table.conf.in`), and `pinyin_save` writes back
> what a same-backend libpinyin picks up (drop-in task 9, reopened
> with exactly this scope; landed 2026-09-09 —
> `docs/findings/user-store.md` §11, measured both ways on Kyoto
> Cabinet and tkrzw by `tools/oracle/user-dir-round-trip.sh`; until
> then the runtime opened its own `user_store.<ext>` and left those
> files untouched, and the maintainer confirmed the closure 2026-09-12). **Data loss when the KV database backend actually
> changes is taken for granted** — a BDB-built libpinyin (Debian
> stable, Ubuntu) against oxpinyin's KC/tkrzw builds, redb/LMDB builds
> with no libpinyin counterpart, KC↔tkrzw transitions. *(Historical as
> of 2026-09-20: the redb and LMDB builds named here no longer
> exist — LMDB was removed 2026-09-20 in `refactor/drop-redb-lmdb-backends`,
> redb the same day in `refactor/drop-redb-backend`; the clause stands as the ruling's
> record. The BDB-vs-KC/tkrzw and KC↔tkrzw examples remain live.)*
> **(Default-backend transition, 2026-09-20 UTC, branch
> `refactor/default-backend-berkeleydb`.)** The workspace default feature
> moved tkrzw → Berkeley DB, so a from-source build that takes the
> default now produces a different backend family: `user.conf`'s
> `database format` line changes `Tkrzw` → `BerkeleyDB` and the
> `DEFAULT_STORE_EXT` constant `tkt` → `db` (the session-scratch /
> datagen-native container extension; the DBM tables themselves keep
> libpinyin's own names on both). This is the same-family change the
> paragraph above takes for granted: a profile written by a
> pre-2026-09-20 default build is non-conform under the new default and
> is wiped on open, exactly as libpinyin wipes across its own backend
> switches. **Affected set, stated honestly: from-source developer
> stores only.** Nothing released carries the old default — no wheel,
> no package, no published artifact; the per-distro release lanes pin
> their backend explicitly (debian/tkrzw, fedora and arch/kyotocabinet)
> and always have. `DEFAULT_STORE_IS_LIBPINYIN_DBM` is `true` either
> way, and the drop-in on-disk surface — libpinyin's own file names —
> is unchanged on both sides of the transition. There the
> ecosystem's own fresh-start norm applies: libpinyin discards user
> data across its own backend switches (Debian's 2.11.91-1
> BerkeleyDB→Tkrzw carried a `debian/NEWS` warning: "all previous
> user data will be lost after the upgrade") and across
> model-version bumps (`check_format` → `_clean_user_files`,
> `pinyin.cpp:172-199`), and it migrated nothing from its own
> predecessors (W7, `legacy-migration.md`). Across a genuine backend
> change the migration path is the library's value-level interchange —
> the `pinyin_*_add_phrase*` import trio and the
> `pinyin_*_get_(bigram_)phrases` export iterators (W6-T7) — which
> carries the user *phrase dictionary* but not the trained *bigram*
> (upstream's ABI has no bigram import); that residual asymmetry is a
> cross-backend concern only. **Consequence for the E2E rule below:**
> "state" includes the on-disk user state of a same-backend user dir;
> a user dir in another backend's format is outside the compared
> state, and a fresh start there is not a divergence. The BerkeleyDB
> path (task 10) stays shelved behind its existing consumer-need gate
> — a BDB revival would bring system and user files at once, on a
> distro set whose own backend switch already discarded the user
> data. (Amended 2026-09-12: the consumer ask arrived and task 10
> landed — the Berkeley DB backend is the fifth store peer
> (`docs/findings/berkeleydb-backend.md`); this paragraph records the
> pre-landing state.)
> **(Reference build, ruling 2026-09-26 UTC, recorded 2026-09-27 UTC.)**
> The reference build is the pin's autoconf build, and the reference
> default is a bare `./configure`, which selects Berkeley DB
> (`configure.ac:94` at `074a2219`: `DBM="BerkeleyDB"`, overridden only
> by `--with-dbm`). oxpinyin's default matches it: every crate that
> selects a store backend carries `default = ["bdb"]` (e.g.
> `crates/oxpinyin-capi/Cargo.toml:32`). Neither tkrzw nor Kyoto Cabinet
> is "the default" any longer, in oxpinyin or in the reference; both
> are peers selected explicitly, as the pin selects them with
> `--with-dbm`. Parity coverage always includes all three cells —
> tkrzw, bdb and kc — whichever one a given release lane pins. Earlier
> prose in this document that calls tkrzw or Kyoto Cabinet the default
> is history, dated where it stands.

That goal sets the default: **oxpinyin reproduces the pin.** Divergence
is not a design freedom to be exercised where the Rust is nicer. It is
an exception that has to be argued into one of the three live classes
below, (a)–(c), and everything outside them is a defect to be reverted.
Class (d) was retired on 2026-09-06; its text is kept below as a record
and no entry may be classified under it.

## The exception classes — three live, (a)–(c)

There are three live classes, (a) math, (b) memory safety and (c)
availability, and no others. A fourth, (d) consumer scope, was retired
by maintainer decision on 2026-09-06 (banner under its heading below).
A **registered standing divergence** — "Registered standing
divergences" below — is not a class: it records a ruled-accepted
difference that no class covers, one by one, and never admits a new
entry by analogy.

### (a) MATH — platform-dependent floating-point accumulation

Upstream's result is a pure function of `gfloat` values accumulated
through a transcendental (`log`), and no fixed-point or integer form
reproduces it. Reproducing it would require delegating to the platform
libm, which carries no cross-platform bit-exactness guarantee.

**Cited against constitution item 6:** *"Determinism: output is a pure
function of (input, user state, config)."* Output that depends on which
libm the build linked is not a function of those three, so reproducing
upstream here would violate the constitution rather than serve it. The
build's refusal of `-march=native` is the same rule applied earlier.

**The class is narrow.** Basic IEEE-754 operations — add, subtract,
multiply, divide, compare — are bit-reproducible across platforms and
are **not** covered. `amplified_frequency` ((1−λ)·unigram/total·2²⁴) is
basic-ops-only and is ported to 100%; that is the standard. A float in
the call graph does not make an entry class (a). A transcendental in
the accumulation does.

**Example:** the n-best trellis (`phonetic_lookup.h:663,692`) does
`m_poss += log(...)` per step into a `gfloat`, and near-ties among the
top-3 survivors are decided down to the ULP.

### (b) MEMORY SAFETY — upstream is UB and Rust structurally prevents it

Upstream's observable behaviour *is* undefined behaviour — a
use-after-free, an out-of-bounds read — and the Rust construction that
replaces it cannot express the bug. The divergence is not a decision;
there is no safe Rust that reproduces it.

**Scope note:** this class covers only cases where reproduction is
structurally impossible. Where upstream's UB happens to produce a
*stable, reproducible* value that a safe construction could also
produce, it is not class (b).

**Examples:** the bigram export iterator's unterminated pronunciation
vector (`pinyin.cpp:844-850`, `:857-863`), a heap over-read the pin
segfaults on in the first export cycle after a train (row 1; this
example called it a use-after-free on a repeated cycle until
2026-09-27 UTC, #530/#550); the aux-text heap over-read.

### (c) AVAILABILITY — upstream aborts on caller input

Upstream `assert()`s or `abort()`s on input a caller can supply.
oxpinyin returns `false` / `Err` instead and logs the point.

**Rust does not prevent aborting.** `panic!`, `abort()`, `unwrap()` and
`assert!` are all available and would reproduce upstream faithfully.
This class is therefore **not** a language-mechanism residue — it is a
deliberate product decision, and it must be labelled as one rather than
smuggled in as something Rust forced.

**Justification — MISRA C:2025 guideline D.4.1 (Required), assessed for
Rust.** MISRA C Directive 4.1, "Run-time failures shall be minimized",
is category Required. MISRA C:2025 Addendum 6 ("Applicability of MISRA
C:2025 to the Rust Programming Language", March 2025 —
[`MISRA-C-2025-ADD6`](https://misra.org.uk/app/uploads/2025/03/MISRA-C-2025-ADD6.pdf))
assesses D.4.1 as applicable to Rust, keeps its adjusted category
Required, and records the run-time failure as "often in the form of
panics". A library loaded into a long-lived input-method process must
not take the process down on caller error: an IME abort loses the user's
session, not just the call. Constitution item 4 ("nothing panics on any
input; public APIs return `Result`") is the in-house statement of the
same rule.

**Obligations on every (c) site:** return `false` (C ABI) or `Err`
(Rust), *and log the point*. A silently-swallowed abort is not class
(c) — it is a behaviour change with no record.

**Both halves, exactly (ruling recorded 2026-09-27
UTC).** A site is class (c) only when oxpinyin answers `false`/`Err`
**and** emits exactly one GLib `g_warning`-level log line for that
site per occurrence — the channel `crates/oxpinyin-capi/src/ffi.rs:158-173`
(`log_warning`, domain `libpinyin`) and its zhuyin twin already use.
A site that answers `true`, data or a store write where the pin
aborts meets neither half and needs a guard, not just a log. The
bug-for-bug target is the **asserts-live** pin build: the reference
autoconf build passes no `-DNDEBUG` (its default `-g -O2`), so every
`assert` at the pin is an abort site exactly as every `abort()` is,
and `check_result` (`include/pinyin_utils.h:27-31`, an `assert`
unless `NDEBUG`) is one too. Every class-(c) row states, per site,
whether the pin's construct is an **`assert`** or an **`abort()`** —
an `assert` would vanish under `-DNDEBUG`, an `abort()` would not, and
the upstream report differs accordingly.

**Boundary:** (c) covers *aborts*. It does not cover upstream returning
a wrong-but-defined answer. Where upstream half-mutates and reports
success, reproducing it is possible and the divergence is a revert
target, not an availability exception. See the double-pinyin
out-of-enum row in the table below, which sits on exactly that line.

### (d) CONSUMER SCOPE — only what the two reference consumers call

> **RETIRED (maintainer decision, 2026-09-06).** This class was written
> for the 51/58-symbol consumer-union contract and became moot the
> moment the target changed to the full ABI with `pinyin.h` and
> `zhuyin.h` copied verbatim (W8, 79/79; libzhuyin, 52/52): every
> exported symbol and every option bit is in scope, whether or not a
> known consumer reaches it. The text below is kept as the record of
> what (d) meant; no new entry may be classified (d), and the two rows
> that were (16, 17) are re-dispositioned in the table. "No consumer
> calls it" remains useful as a *priority* signal, never as an
> exception.

**The scope is every exported symbol**: all 79 `pinyin_*` symbols in
`libpinyin.ver` and all 52 `zhuyin_*` symbols in `libzhuyin.ver` — the
same sets the pin exports (verified identical 2026-09-17). Nothing is
in or out of scope by which consumer calls it, and no count of
"the symbols consumers call" bounds anything; the shipped object has
exported the full set live since 2026-08-30.

**Consumer behaviour is reachability evidence, never scope.** A named
consumer's call sites may be cited to show that one specific behaviour
is reachable in practice (for example ibus-libpinyin 1.16.5's default
sort word, `PYPConfig.cc:151`); they never define, count or bound the
work. Dead code in a consumer is not evidence of anything: it is not
a call site (`pinyin_get_raw_full_pinyin`, fcitx's `eim.cpp:377-391`
inside `#if 0`, is not even an upstream export, so a live call would
not link). The reference for the exported set and per-symbol
signatures is `docs/findings/abi-reference.md`.

## Registered standing divergences — outside the classes

A standing divergence is a difference the maintainer ruled accepted
under the language-quirk exception: a consequence of the Rust/C++
language boundary that no (a)–(c) class describes (maintainer ruling,
2026-10-03). It is registered by name, with its ruling; it is not a
numbered class, it admits nothing by analogy, and it is counted apart
from the classes in the totals.

- **tkrzw `SYSTEM_ERROR`/`UNKNOWN_ERROR` collapse** (row 40; ruled
  accepted 2026-08-28, `tkrzw-langc-exception-classification.md`).
  Through tkrzw's C API every wrapper catches a C++ exception and
  reports `TKRZW_STATUS_SYSTEM_ERROR` (`tkrzw_langc.cc` at 1.0.32,
  e.g. `:163-165`, as that finding cites it), where the retired cxx shim reported
  `UNKNOWN_ERROR`; the store maps `SYSTEM_ERROR` to `StoreError::Io`
  (`crates/oxpinyin-store/src/tkrzw/mod.rs:214`), so an
  allocation-failure exception and an operating-system error now share
  one error class. Reachable only under memory exhaustion; no
  differential surface observes it. **Ruling (2026-10-03):** this is a
  Rust/C++ language-boundary consequence and falls under the
  language-quirk exception. Rust cannot catch a C++ exception, so oxpinyin
  can only use tkrzw through its C API, and that API merges the
  exception-origin and operating-system error codes into one status
  before the store sees either. The pin's tkrzw backend calls tkrzw's
  C++ API and can tell them apart; oxpinyin cannot, short of a C++ shim
  of its own. It is not observable through libpinyin's C ABI, and it is
  not UB, an abort or a transcendental, which is why none of (a)–(c)
  applies.

## (e) The E2E I/O compatibility rule

The three live exception classes say when divergence is permitted. This
says what compliance *means* everywhere else, and it is the rule those
classes are exceptions to.

> **E2E I/O COMPATIBILITY RULE:** For every exported symbol, given
> the same inputs and state, oxpinyin MUST return
> byte-identical outputs to the pinned libpinyin 2.11.92 at `074a2219`
> (the pin since 2026-09-06; the rule was written at 2.11.91 / `0c5e80e`,
> against which the candidate surface is byte-identical —
> `docs/testing/oracle-environment.md`) —
> except where one of the named exceptions (a)/(b)/(c) ((d) retired 2026-09-06)
> explicitly applies. Exporting a symbol that returns a wrong value is
> worse than not exporting it: the consumer gets a silent wrong answer
> instead of a link error. **A stub returning `false` is not compliance —
> it is a defect.**
>
> **Corollary:** if implementing a symbol correctly requires an engine
> change, that engine change is mandatory. The engine serves the ABI, not
> the other way around.
>
> **Verification:** every exported symbol must have a
> differential probe that drives it with the same input on both libraries
> and asserts byte-identical output — where *output* is the whole
> observable surface, not just the scalar return: return status,
> out-parameters and the data they point to, written lengths, and any
> state transition on the handle. A symbol with no probe is unverified,
> not compliant. The probe runs on all three backend cells — tkrzw,
> bdb and kc — against the pin built for the same cell (ruling
> 2026-09-26 UTC; see "Amendment — rulings recorded").
>
> **Evidence standard (ruling recorded 2026-09-27 UTC).** A finding —
> a divergence, a closure, a class — rests on code plus data: the
> citation of both sides' code at the named SHAs and the input and
> output needed to see the difference. Logs are not evidence; a run
> log or a local working file may point at where the data came from,
> never stand in for it.

Three consequences worth stating, because each is currently unmet
somewhere:

1. **The version script is not a compliance mechanism.** The export
   list decides which symbols exist; this rule decides what they must
   *do*. A symbol may be legitimately divergent (a named exception). It
   may not be present and wrong.
2. **`pinyin_get_pinyin_key_rest` and `pinyin_get_pinyin_key_rest_positions`
   were defects when this was written** — exported, returning `false`
   unconditionally. Closed with the W8 79/79 work: both are implemented
   against a per-instance key-rest slot
   (`crates/oxpinyin-capi/src/cursor.rs`). The rule they illustrated
   stands: a linker error is a diagnosis and a `false` is not.
3. **Probe coverage is itself a deliverable.** The exported set is
   79 `pinyin_*` symbols (and 52 `zhuyin_*`); the differential suite
   does not drive all of them. The uncovered ones are unverified rather
   than compliant, and closing that gap is work, not bookkeeping. The
   live matrix is `docs/findings/probe-coverage-abi.md` (PR #480).

## The classification table

Every entry in `upstream-divergences.md`, and the one parked entry in
`all-off-tails.md`, classified against (a)/(b)/(c) ((d) retired
2026-09-06; rows 16 and 17 carry its history), **STANDING** (a
registered standing divergence, not a class — see below), **REVERT
TARGET** (reproducible, not yet reproduced, no blocker but the work),
**OPEN DEFECT** (reproducible, blocked on a STOP — an engine-interface
ask), **CLOSED** (reproduced, or proven equivalent on the pinned data),
**CONTRADICTED** (a closure a later measurement contradicts, pending
investigation),
or **no ABI divergence**. Rows 1–18 are the 2026-08-28 table with their
status brought to `87f25055`; rows 19–31 are the entries the register
gained or that the original table skipped (row 19 landed with the
074a2219 pin bump); rows 32–38 were registered 2026-09-18 to 2026-09-20,
and rows 39–44 on 2026-09-27 by the round-2 reconciliation (#573).
Every row from 17 on now has a register entry (`upstream-divergences.md`,
"Entries registered 2026-09-27 UTC"). The work order for the
revert targets is `revert-plan.md`.

| # | Entry | Class | Basis |
| --- | --- | --- | --- |
| 1 | Bigram export iterator's pinyin buffer | **(b)** | **restated 2026-09-27 UTC (#530):** the pin crashes on the **first** export cycle after a train, saved or not — not only on a repeated cycle. `pinyin_bigram_iterator_has_next_phrase` builds each pronunciation string from a `GPtrArray` it never NULL-terminates (`g_ptr_array_new` → `g_ptr_array_free(…, FALSE)` → `g_strjoinv`/`g_strfreev`, `pinyin.cpp:844-850` and `:857-863`; the unigram export adds the terminator, `:726-734`), so the join walks past the array's end: a heap over-read whose outcome depends on what follows the allocation. Executed (audit D-20): tkrzw and kc crash with the user dir saved and unsaved; bdb crashes unsaved, 4/4. oxpinyin renders every row into owned strings when the iterator is created (`crates/oxpinyin-capi/src/iterators.rs:336`), so no join over a C array exists to over-read |
| 2 | Public bigram export is a rendering surface | **no ABI divergence** | the C ABI reproduces the rendering; nothing in-tree reads the raw store — the internal migration tool that would have was cancelled (`legacy-migration.md`, SHELVED on `feat/w7-t2-legacy-migrate`; maintainer confirmation 2026-09-12) |
| 3 | HANYU full pinyin ignores tone digits under `USE_TONE` | **CLOSED** | ported; `PARSE_AUX_IDENTICAL` |
| 4 | Tone digit on an initial-only key aborts the phrase search | **(c)** — both halves met (lane C, #525) | pin SIGABRTs on `n4` under `USE_TONE\|PINYIN_INCOMPLETE`. Sites: `storage/pinyin_phrase3.h:152` **`assert`** (`CHEWING_ZERO_TONE == key.m_tone` in `contains_incomplete_pinyin`, `:146-156`) on every phrase search over the key; `pinyin.cpp:2769` **`assert`** (the same test in `pinyin_get_pinyin_is_incomplete`). oxpinyin searched the key and returned candidates (`contains_incomplete` ignored the tone) and answered `true` from `pinyin_get_pinyin_is_incomplete`, with no log. Since lane C, PR 12e, `pinyin_guess_candidates`, `pinyin_guess_sentence`, `pinyin_guess_sentence_with_prefix` and `pinyin_get_pinyin_is_incomplete` answer `false` with one `g_warning` in `libpinyin` when the matrix (or the key) holds a toned initial; held by `abort-guess-candidates-toned-initial`, `abort-guess-sentence-toned-initial`, `abort-guess-sentence-toned-initial-after-key`, `abort-is-incomplete-toned-initial` and `abort-guess-candidates-toned-initial-before-offset` (`n4ni` at offset 2, where the pin dies as well, so the guard needs no offset window), with `toned-initial-neighbours` as the control. |
| 5a | Scheme setters — double `CUSTOMIZED` (30) | **(c)** — both halves met (lane C, #525) | aborts mid-call: `storage/pinyin_parser2.cpp:611` **`abort()`** (after the fallback clear at `:580`). oxpinyin answers `false`, scheme unchanged (`crates/oxpinyin-capi/src/config.rs`), and since lane C, PR 12a emits one `g_warning` in `libpinyin`; held by `abort-set-double-pinyin-scheme-30` of `contract-diff.py`. |
| 5b | Scheme setters — double out-of-enum (negatives, 0, 7–29, 31+) | **CLOSED** in code (2026-09-15) | reproduced: CAPI returns `true`, fallback cleared, shengmu/yunmu intact — the pin's half-mutation; the contract test (`contract.rs::double_out_of_enum_reproduces_the_half_mutation`) pins the cleared fallback as observable; the `run-scheme-diff.sh` oracle differential with out-of-enum values ran 2026-09-16 in a debian:testing container — IDENTICAL, the twelve `halfmut` probe rows byte-equal on both sides |
| 5c | Scheme setters — zhuyin `STANDARD_DVORAK` (7) | **(c)** — both halves met (lane C, #525) | the dvorak arm falls through to `default:` — `storage/zhuyin_parser2.cpp:295` **`abort()`** (arm at `:291`). oxpinyin answers `false`, scheme unchanged, and since lane C, PR 12f `zhuyin_set_chewing_scheme(7)` emits one `g_warning` in `libzhuyin` (`pinyin_set_zhuyin_scheme(7)` has logged in `libpinyin` since PR 12a); held by `abort-zhuyin-chewing-scheme-7` of `contract-diff.py`. |
| 5d | Scheme setters — zhuyin / full-pinyin out-of-enum | **(c)** — both halves met (lane C, #525) | zhuyin: `pinyin.cpp:1189` **`abort()`** (the API's `default:`; the register's old `:1188` was one line off) and its libzhuyin twin `zhuyin.cpp:736` **`abort()`**; full pinyin: `storage/pinyin_parser2.cpp:398` **`abort()`**. oxpinyin answers `false`, scheme unchanged, and now emits one `g_warning` per call: `libpinyin` for the pinyin setters (PR 12a), `libzhuyin` for `zhuyin_set_chewing_scheme` out of the enum and `zhuyin_set_full_pinyin_scheme(0 or 4)` (PR 12f; `abort-zhuyin-chewing-scheme-0`, `-10`, `-minus-1`, `abort-zhuyin-full-pinyin-scheme-0`, `-4`). |
| 6 | Constraint-aware train without the consistency assert | **(c)** — both halves met (lane C, #525) | `train_result3` aborts on a stale result: `lookup/phonetic_lookup.h:868` **`assert`** (`token == constraint->m_token` at a `CONSTRAINT_ONESTEP` position). `Session::train_nbest` checks every forced phrase against the decoded row before observing anything and answers `EngineError::StaleTrainingConstraint`; `InstanceCore::train` leaves the context unmodified. Both C facades turn that error into `false` and exactly one warning, in `libpinyin` for `pinyin_train` and `libzhuyin` for `zhuyin_train` (lane C PR 12d). `abort-train-stale-forcing` and `abort-zhuyin-train-stale-forcing` show the pin dying of SIGABRT and the guarded answers; `train-forcing-after-lookup` is the successful control. |
| 7 | `validate_constraint`'s drop test is the span-search shape | **CLOSED** (was REVERT TARGET) | 4c2fe02b, 2026-08-29: the `FLT_EPSILON` drop boundary is unreachable on model20 (max per-token total 2,945,481 < 2²³), so the two tests are observably equivalent on the pinned data; the arithmetic still differs (see the note below) |
| 8 | Constraints survive every re-parse except the selection-committed one | **CLOSED** (was REVERT TARGET) | #217 (`fix/revert-r5-constraint-reset`): constraints survive a selection-committed re-parse; frozen pins bit-identical |
| 9 | The n-best row-choose cursor is the row's own end | **CLOSED** (was REVERT TARGET) | eca8d43b: every `NBEST_MATCH_CANDIDATE` choose answers `parsed_len` — upstream's `matrix.size()-1` in the active parse mode's coordinates |
| 10 | `pinyin_get_sentence` asserts a past-the-rows index | **(c)** — both halves met (lane C, #525) | SIGABRTs on a non-empty result set: `pinyin.cpp:1474` **`assert`** (`index < results.size()`; function `:1464-1482`). oxpinyin answers `false` with `*sentence` untouched (row 53) and, since lane C, PR 12b, emits one `g_warning` in `libpinyin` when rows exist (no rows is the pin's own silent `false`); held by `abort-get-sentence-past-rows` of `contract-diff.py`. |
| 11 | N-best native costs versus pin gfloat log accumulation | **(a)** — six shared-comparison rows only (2026-10-04 UTC, lane D, #535/#594/#574) | The seven selection rules are ported bug for bug. Port gate: 495/495/495 of 496; #574 coverage extends it to 499/499/499 of 500, and the #639 resplit repair to 500/500/500 of 500; corpus residual 14/10,465 before that repair (not re-measured). Only bo, quguan, kaliantai, huilianpei, yunhoulvlvlunqianaonaoruoqubeiji and nou'y are (a); deciding pin float bits and native subject costs are recorded in sentence-surface.md §12's lane-D amendment. The four different-generation-path rows were the #639 resplit defect and now match; four pre-existing separator rows remain **unattributed**, named there. All 166 candidate-assembly cases (155 pinyin, 11 zhuyin; 144 at origin/main, 7 added by #639, 15 by #645), including the 14 #594 cases, match. The earlier "129" was the port-time suite before its 14 toned and one forced-token case. No remaining row is presumed arithmetic. |
| 12 | Predicted-candidate tie order | **CLOSED** (was REVERT TARGET) | superseded by P6 (345af16d, 2026-09-02): on KC and tkrzw the runtime walks the pin's own phrase DBM, so `pred-order-diff` is IDENTICAL on the pin's `data/` (1588 lines, 0 mismatches) — the KC hash-walk experiment the original row asked for is moot. On redb/LMDB the text-ascending *defined* order stood (maintainer decision 2026-08-25); those containers were not the pin's and outside the drop-in surface *(both backends removed 2026-09-20, branches `refactor/drop-redb-lmdb-backends` and `refactor/drop-redb-backend` — historical clause, retained by ruling)*. `ROADMAP.md` records the same disposition |
| 13 | Mid-syllable candidate-lookup offset | **CLOSED** (was REVERT TARGET) | the pin's empty-column law is reproduced (register entry re-titled "closed"; the C2 residue closed 2026-08-29 per `uncovered-surface-differentials.md` phase E) |
| 14 | Cursor helpers' `_check_offset` aborts answer `false` | **(c)** — both halves met (lane C, #525) | at `074a2219` `_check_offset` itself returns `false` (`pinyin.cpp:2163-2182`); the aborts are its **`assert`**-wrapped calls — `pinyin.cpp:3035` and `:3057` (`get_left_pinyin_offset`), `:3067` and `:3092` (`get_right_pinyin_offset`); the `:2175` the register and the runners' comments cite was the `0c5e80e1` pin's assert. Executed abort: `get_right_pinyin_offset(inst, 5, &r)` on `nihao` fires `:3092` (#549). oxpinyin answers `false` via `EngineError::ZeroKeyOffsetCheck` (`crates/oxpinyin-engine/src/cursor.rs:142`) and, since lane C, PR 12b, emits one `g_warning` in `libpinyin`; held by `abort-get-left-pinyin-offset-after-separator`, `abort-get-right-pinyin-offset-after-separator` and `-nihao-5`. |
| 15 | Apostrophe-only input: pin consumes every byte, engine none | **CLOSED** (was REVERT TARGET) | 678f3259, 2026-08-26 (B2, the parser-termination class): `SegmentGraph` propagates each apostrophe one byte, counted — `'''` consumes 3, pinned in `graph.rs`. Closed two days *before* this policy was written; the register entry had not been updated (amended 2026-09-06) |
| 16 | `FORCE_TONE` honoured on the full-pinyin seam only | **CLOSED** (was (d); (d) retired 2026-09-06) | the zhuyin batch (1671954), double-pinyin batch (#289) and one-key seams have since been ported; the last seam, the pinyin facade's chewing batch, is row 30 |
| 17 | Literal `0x0` option gating (`jv`/`zon`; `xian` divided-table) | **CLOSED** in code (`8ec75085`; live sweep 2026-09-16) | closed (#548 corrected this cell, which still read REVERT TARGET): the pin's gating at the literal `0x0` word is ported — resplit and divided additions are gated on `USE_RESPLIT_TABLE`/`USE_DIVIDED_TABLE` (`crates/oxpinyin-engine/src/session/mod.rs:609`, `:626`; pin `storage/phonetic_key_matrix.cpp:88`, `:171`) and an empty matrix answers an empty guess (pin `pinyin.cpp:2194`). `run-option-sweep.sh` with the all-off (`0x0`) and divided-contrast cases ran 2026-09-16, 24/24 PASS. Original basis: not a register entry, the parked paragraph in `all-off-tails.md`; with (d) retired the consumer-unreachability argument was moot. Register entry added 2026-09-27 UTC |
| 18 | The pinyin index DBMs carry uninitialized struct padding | **(b)** | upstream copies a stack struct's tail padding into the DBM; datagen zeroes it and the reader never touches it |
| 19 | `pinyin_get_character_offset`'s recursion asserts answer `false` | **(c)** — both halves met (lane C, #525) | sites, all **`assert`**: `pinyin.cpp:3147` (`size > 0`, the recursion's column), `:3161` (`1 == size`), `:3203` (`offset < matrix.size()`), `:3204` (`_check_offset`); libzhuyin twins `zhuyin.cpp:2110`, `:2158`. The register's `:3152`/`:3166` were stale by five lines. The unbounded `cached_tokens` read (`:3168`) is a (b) sub-shape answered as a deterministic miss. oxpinyin answers `false` via `EngineError::MatrixColumnAssert`/`LookupOffsetOutOfRange`/`ZeroKeyOffsetCheck` (`crates/oxpinyin-engine/src/char_offset.rs:149,152,157,219,226`) — the false half held. Since lane C the pinyin arm (`pinyin_get_character_offset`, `:3203`, `:3204`; PR 12b: `abort-get-character-offset-past-matrix`, `-after-separator`) emits one `g_warning` in `libpinyin` and the libzhuyin twins (`zhuyin.cpp:2110`, `:2158`; PR 12f: `abort-zhuyin-get-character-offset-past-matrix`) one in `libzhuyin`. |
| 20 | One bigram-prediction row differs on the pin's own data (trellis residual) | **→ row 33** (reattributed 2026-09-27 UTC, #550; counted under row 33) — closed with row 33, #614 (merge commit `33e2e54d`) | **closed with row 33 (2026-10-03, #614 merge commit `33e2e54d`, code `750b8cb6`):** `union-diff` is pin-identical on `main` — `IDENTICAL (11 log lines)` on tkrzw, bdb and kc at the tree of `main` `c6a61be8`; the extra `pred: type=4 text=你` line is gone, and no "byte-identical to `main`" claim about it stands any more. Original record, kept as written: **falsified as (a) (audit D-05, #550):** the one `union-diff` line — oxpinyin's extra `pred: type=4 text=你` after the train on `测测` — is row 33's mechanism, not the trellis residual: the union driver's whole-row NBEST choose installs no `CONSTRAINT_ONESTEP` on the pin (`pinyin.cpp:2515-2520`), so `train_result3` writes nothing (`lookup/phonetic_lookup.h:866`) and the pin predicts nothing, while oxpinyin's history fallback (`crates/oxpinyin-engine/src/session/selection.rs:298-339`) writes `测测→你` with count 138 and predicts 你. It occurs on every cell, not on Kyoto Cabinet only. It closes when row 33 closes. Original basis, kept as the record: downstream of row 11: the trained count for `测测 → 你` straddles the `m_count ≥ 10` filter because the trellis residual picks a different phrase; one `union-diff` line, everything else identical |
| 21 | The single-key surface aborts the pin where oxpinyin answers `false` | **(c)** — both halves met (lane C, #525) | `storage/pinyin_parser2.cpp:170` **`assert`** (apostrophe in `parse_one_key`); `pinyin.cpp:499` **`assert`** (`index < PHRASE_INDEX_LIBRARY_COUNT`, addon unload) and its sibling `:466` **`assert`** (phrase-library unload). The empty-input reads under `USE_TONE` (`pinyin_parser2.cpp:178`, `zhuyin_parser2.cpp:171`) are over-reads, neither `assert` nor `abort()` — the (b) shape. oxpinyin answers `false` on all of them (pinned in `crates/oxpinyin-capi/tests/abi/keys.rs`) — the false half held. Since lane C: `pinyin_unload_phrase_library(16)` (`:466`) and `pinyin_unload_addon_phrase_library(16)` (`:499`) warn (PR 12a), `pinyin_parse_full_pinyin` and `zhuyin_parse_full_pinyin` with an apostrophe (`pinyin_parser2.cpp:170`, one parser) warn (PR 12b, PR 12f; `abort-parse-full-pinyin-apostrophe`, `abort-zhuyin-parse-full-pinyin-apostrophe`). |
| 22 | Empty-string phrase lookup crashes the pin | **WITHDRAWN** — not reproduced at `074a2219` (2026-10-03 UTC); original reproduction unavailable; claim withdrawn (maintainer ruling) | **Withdrawn 2026-10-03 UTC:** not reproduced at `074a2219`: no crash and valgrind clean on tkrzw, bdb and kc (2026-10-03); the audit's original reproduction is unavailable (its evidence archive no longer exists); claim withdrawn. `pinyin_lookup_tokens(instance, "", array)` was run on freshly built pin oracles (tkrzw, bdb, kc), plain and under valgrind, and answered `false` with an empty array every time; it was also run with an empty and a populated user dir, fresh and after `guess_sentence`, with a user-imported phrase present, on all three cells without a fault. The row is no longer class (c) and has no log obligation. Original record, kept as written (the claim it makes is the one withdrawn): **symbol corrected (#549):** the crash is `pinyin_lookup_tokens(instance, "", …)`, which passes a zero length straight to `m_phrase_table->search` (`pinyin.cpp:2652-2667`); `pinyin_phrase_segment(instance, "")` does not crash — `get_best_match` with length 0 runs no search (`lookup/phrase_lookup.cpp:119-149`, `nstep - 1 == 0`) — and answers `true` (audit R-2), as oxpinyin does (`crates/oxpinyin-engine/src/phrase.rs:124-130`, `:181-201`). Site kind: a fault signal, neither `assert` nor `abort()`; a crash on caller input is the (c) shape whatever the signal. oxpinyin answers `false` (`crates/oxpinyin-capi/src/dict.rs:86`) — the false half holds; no log. **Re-measurement 2026-10-03 (the basis of the withdrawal above):** `pinyin_lookup_tokens(instance, "", array)` was run on freshly built pin oracles at `074a2219` — tkrzw, bdb and kc, plain and under valgrind, with an empty and a populated user dir, fresh and after `guess_sentence`, with a user-imported phrase present — and answered `false` with an empty array on every run; no SIGFPE, no fault, so no backtrace exists to cite. The source shows no `assert`/`abort()` and no division on the path (`pinyin.cpp:2652-2667` passes `ucs4_len` 0 to the backend's key get). Under the 2026-10-03 ruling a non-assert, non-`abort()` pin crash is class (b) when it is undefined behaviour; that classification would need a reproduced fault, and none exists, so the claim is withdrawn rather than reclassified |
| 23 | Sanitizer scope on the tkrzw shim CI · native data-file naming · R1 measured on the compat paths | **no ABI divergence** | three dated records, not behaviour entries: a CI-instrumentation note, a file-naming decision (`installed-naming.md`), and a measurement whose subject was removed (its SUPERSEDED banner is itself amended 2026-09-06 — P6 restored direct reads of libpinyin's files) |
| 24 | zhuyin batch `FORCE_TONE` law | **CLOSED** | 1671954: `ZhuyinParser::parse_with_options` honours the three per-keyboard shapes |
| 25 | zhuyin candidate-tag grouping + `after(consumed)` terminal offset | **CLOSED** — re-closed by #623 (merge commit `cb824981`, code `7bea86d1`) after #577 (was CONTRADICTED) | **Re-closed (2026-10-03, #577; #623 merge commit `cb824981`, code `7bea86d1`):** what #577 contradicted was the mid-key after-cursor window. At an offset strictly inside a key the pin's matrix column is empty, so `zhuyin_guess_candidates_after_cursor` answers the prepended sentence rows alone (`zhuyin.cpp:1498-1512`, `:1624-1626`); #623 snapshots only those rows there (`zhuyin_offset_is_mid_key`), and the `zhuyin-diff` battery now drives offsets 1, 2, 4 and 5 of both families, `su3cl3` in the standard corpus and a choose battery: 4558 log lines identical to the pin on tkrzw, bdb and kc, re-measured on the tree of `main` `c6a61be8`. The choose-then-guess half (#602) is #609 (merge commit `563ed9b1`, code `47eea9aa`). Original record, kept as written: **Contradicted by #577 (2026-09-27 UTC; audit Z-3), pending investigation.** The audit's libzhuyin battery, on all three cells: `zhuyin_parse_more_chewings(inst, "su3cl3")`, `zhuyin_guess_sentence`, then `zhuyin_guess_candidates_after_cursor` at offsets 1, 2 and 5 answers n=1 on the pin and n=126/126/94 on oxpinyin; after a choose, `zhuyin_guess_candidates_before_cursor` answers 94/126 on the pin and 1/94 on oxpinyin (pin `zhuyin.cpp:1460-1541`, `:1542-1600`; oxpinyin `crates/oxpinyin-zhuyin-capi/src/sentence.rs:181`, `:198`). The repo's `zhuyin-diff` drives no mid-key offset, which is how the closure below missed it. Which half of this row the mid-key windows belong to is the open question; the closure stands contradicted until it is answered. Original closure record: both halves closed by the display-law collapse and the builder terminal mapping (amended 2026-08-31) |
| 26 | zhuyin before-cursor candidate window | **CLOSED** — re-closed by #623 (merge commit `cb824981`, code `7bea86d1`) after #577 (was CONTRADICTED) | **Re-closed (2026-10-03, #577; #623 merge commit `cb824981`, code `7bea86d1`):** what #577 contradicted was the mid-key after-cursor window. At an offset strictly inside a key the pin's matrix column is empty, so `zhuyin_guess_candidates_after_cursor` answers the prepended sentence rows alone (`zhuyin.cpp:1498-1512`, `:1624-1626`); #623 snapshots only those rows there (`zhuyin_offset_is_mid_key`), and the `zhuyin-diff` battery now drives offsets 1, 2, 4 and 5 of both families, `su3cl3` in the standard corpus and a choose battery: 4558 log lines identical to the pin on tkrzw, bdb and kc, re-measured on the tree of `main` `c6a61be8`. The choose-then-guess half (#602) is #609 (merge commit `563ed9b1`, code `47eea9aa`). **Before-cursor correction (2026-10-08 UTC, #698; before-cursor half closed by #707, 2026-10-09 UTC):** #623's after-cursor closure did not fix reparsing a truncated prefix before the cursor. The window now searches the complete retained parse at the requested fixed end (zhuyin.cpp:1562-1588, phonetic_key_matrix.cpp:416-423 at 074a2219). Twenty-four separator-free full-pinyin inputs, including tones and incomplete suffixes, match all 177 offset inventories; [separator matrix findings](separator-matrix.md) records the reproduction and controls. Original record, kept as written: **Contradicted by #577 (2026-09-27 UTC; audit Z-3), pending investigation.** The audit's libzhuyin battery, on all three cells: `zhuyin_parse_more_chewings(inst, "su3cl3")`, `zhuyin_guess_sentence`, then `zhuyin_guess_candidates_after_cursor` at offsets 1, 2 and 5 answers n=1 on the pin and n=126/126/94 on oxpinyin; after a choose, `zhuyin_guess_candidates_before_cursor` answers 94/126 on the pin and 1/94 on oxpinyin (pin `zhuyin.cpp:1460-1541`, `:1542-1600`; oxpinyin `crates/oxpinyin-zhuyin-capi/src/sentence.rs:181`, `:198`). The repo's `zhuyin-diff` drives no mid-key offset, which is how the closure below missed it. Which half of this row the mid-key windows belong to is the open question; the closure stands contradicted until it is answered. Original closure record: window builder closed (c2ad5925); the residual — a row whose span starts after the offset was constrained as `[0, offset)` — closed by #374 (`Candidate::span_start` = upstream's `m_begin`, constraint `[m_begin, m_end)`, `zhuyin_choose_candidate` answers `m_begin`, `zhuyin.cpp:1660`) and **measured IDENTICAL on the pin-built libzhuyin at 074a2219** on 2026-09-08: the three-input choose battery byte-identical (register entry, second amendment) |
| 27 | zhuyin multi-syllable candidate construction | **CLOSED** | the divergence was the pinyin string-fill law, not the construction model (amended 2026-08-31) |
| 28 | zhuyin n-best trellis constants `<1, 1>` vs the engine's `<2, 3>` | **CLOSED** in code (#374) | per-session `NbestShape` (`PINYIN` = `<2, 3>`, `ZHUYIN` = `<1, 1>`), set by both zhuyin facades at instance allocation; not observable through today's libzhuyin candidate surface, so no gate moves |
| 29 | zhuyin `FORCE_TONE` / `ZHUYIN_INCOMPLETE` default | **no ABI divergence** | `CapiContext::try_open` seeds the pin's `USE_TONE \| FORCE_TONE`; entry kept as analysis for a future consumer |
| 30 | pinyin-facade chewing batch seam does not forward `FORCE_TONE` | **CLOSED** in code (2026-09-14) | the register says it: no class fits, a defect to close; closed by the prescribed shape — the seam forwards `options().bits() & !ZHUYIN_CORRECT_ALL` through `parse_with_options`, capi tests pin the measured shape (toneless `su` refuses under `USE_TONE \| FORCE_TONE`), and `chewing-diff.c` carries the FORCE_TONE profile; the live differential ran 2026-09-16 — `run-scheme-diff.sh bopomofo` 1–6, 8, 9 all IDENTICAL in a debian:testing container, non-vacuity shown by the reverted seam exiting 2 (register amendment) |
| 31 | redb write-side emptiness probe creates the table it probes | **no ABI divergence** | a redb API constraint below the store traits; nothing above them observes it. Stage-2 store-trait note, not a compatibility entry. *(The probe was removed with the redb backend on 2026-09-20, `refactor/drop-redb-backend`; the row stands as history so the class totals below stay arithmetically true.)* |
| 32 | Sort-option input of `pinyin_guess_candidates` | **CLOSED** in code (2026-10-03): #596 (merge commit `45cc6032`, code `71d1b699`) and #598 (`efa8337a`) | **closed (2026-10-03):** #596 (merge commit `45cc6032`, code `71d1b699`) skips the n-best prepend when bit `0x1` is set (`pinyin.cpp:2295-2296`) so the phrase-string dedup keeps the NORMAL rows that repeat a sentence (`:2298-2300`); #598 (merge commit `efa8337a`) ports the `0x1` choose leg (`:2565-2576`). `candidate-assembly-diff` runs the words `0x0`, `0x1`, `0x1e` and `0x1f`: every case at `0x1` and `0x1f` (20 pin cases per run, import on and off) is identical on tkrzw, bdb and kc on the tree of `main` `c6a61be8`, and none is declared; the `li'shi` / `0x1f` specimen is n=385 on both sides. Original record, kept as written: **Reopened (#582):** the closure below was measured only at `0x1e`, `0x1c` and `0x14`, none of which sets bit `0x1` (`SORT_WITHOUT_SENTENCE_CANDIDATE`, `pinyin.h:56`). With `0x1` set the pin never prepends the sentence rows (`pinyin.cpp:2295-2296`), so its phrase-string dedup (`:2300`) keeps the NORMAL rows that repeat an n-best string; oxpinyin dedups keep-first *with* the n-best rows present (`crates/oxpinyin-engine/src/session/lookup.rs:754`) and the capi then drops the sentence rows (`crates/oxpinyin-capi/src/sentence.rs:298-299`, `:359`), so both copies go. Executed on all three cells (#582): `li'shi` after `guess_sentence`, option `0x1f` — pin n=385 headed by 历史, 理事; oxpinyin n=383 with neither; the `0x1c` control is identical. Original closure record: the original gap: only `SORT_WITHOUT_SENTENCE_CANDIDATE` (0x1) was honoured; 0x2 and the three sort keys (0x4/0x8/0x10) were ignored and no LONGER row was ever produced, consumer-reachable through ibus presets 0x14/0x1c (0x1c the GSettings default, `PYPConfig.cc:151`). Closed by the §9 port (PR #496): the whole word reaches the engine (`Session::set_sort_options`), bit 0x2 clear builds and prepends the pin's LONGER row (`Session::longer_candidate` — `_prepend_longer_candidates`, `pinyin.cpp:1870-1933`, the two length caps and the max-unigram winner), and the three keys order the list by `compare_item_with_sort_option`'s zeroed-disabled-key law (`:1678-1709`); the choose-a-LONGER-row flow trains `+483` unigram and answers cursor 1 (`:2521-2530`). Measured 2026-09-20 (host, oracle `~/.local/opt/pinyin-oracle` read-only, both sides tkrzw): the parameterised sweep passes 24/24 at `0x1e`, `0x1c` and `0x14` (1c/14 flipped from STOP-on-every-case); the ABI probe at `0x1c`/`0x14` drops to the exact 26-line `0x1e` residue set (sorted ± sets identical); the probe's longer-choose phase runs IDENTICAL. Work order record `revert-plan.md` §9 |
| 33 | Whole-row NBEST choose + train writes the user bigram | **CLOSED** in code — #614 (merge commit `33e2e54d`, code `750b8cb6`), #616 (`fd293e53`) | **closed (2026-10-03):** #614 (merge commit `33e2e54d`, code `750b8cb6`) makes a decoded constraint-free result train nothing (`phonetic_lookup.h:866`) and keeps `diff_result` training for nonzero rows (`pinyin.cpp:2515-2520`); #616 (`fd293e53`) trains pronunciation counts through the selected matrix span. The choose-every-row saved-state differential (`run-nbest-choose-train-diff.sh`: 11 profiles — grams, unigrams, pronunciation counts, complete saved logs) is identical on tkrzw, bdb and kc on the tree of `main` `c6a61be8`, and `union-diff` no longer prints `测测→你` (row 20). Original record, kept as written: still present at `e1d915d0` (audit D-05, #527: after a whole-row NBEST choose and train oxpinyin writes `测测→你` with count 138 and predicts 你; the pin writes nothing). no exception class fits; a row-0 `NBEST_MATCH_CANDIDATE` choose runs `diff_result(best, best)` and installs no `CONSTRAINT_ONESTEP` (pin `pinyin.cpp:2515-2520` / `phonetic_lookup.cpp:172-205`; oxpinyin `constraint.rs:193-214`, `selection.rs:221-241`). Pin `pinyin_train` → `train_result3` trains only when `train_next` or `constraint.m_type == CONSTRAINT_ONESTEP` (`phonetic_lookup.h:866`), so a constraint-free whole-row train writes nothing. Oxpinyin's `Session::train` (`selection.rs:298-338`) falls through to the selection-history record when no OneStep cell is present (`:314-318`, `:333-337`) and seeds `sentence_start → phrase`. Same-dir re-measure 2026-09-19 (`probe-coverage-abi.md` B): pin bigram export stays empty through two `train(0)` calls; capi exports counts 138 then 414 for `你好世界` / `ni'hao'shi'jie`. User-visible: DYNAMIC_ADJUST / prediction boost the phrase on oxpinyin and not on the pin; the only residue that corrupts stored user state and compounds with use |
| 34 | Imported user phrase lost after `guess_sentence` | **CLOSED** in code — #598 (merge commit `efa8337a`) with #596 (`45cc6032`) | **closed (2026-10-03):** #598 (merge commit `efa8337a`) keeps the n-best rows across a parse (`pinyin_parse_more_full_pinyins` never touches `m_nbest_results`, `pinyin.cpp:1497-1524`; only `pinyin_reset` clears it, `:2693-2704`) via `Session::discard_composition`; #596 (`45cc6032`) fixes the dedup half. The imported-phrase and `guess_sentence`-then-parse cases (phases A and N) at `0x1` and `0x1f` are identical on tkrzw, bdb and kc on the tree of `main` `c6a61be8`. Original record, kept as written: #582 measures the dedup half at its full width — no import needed, the first `guess_candidates` after the first `guess_sentence` at `0x1f` (row 32); the nbest-across-parse half stands as below. no exception class fits; pin keeps `m_nbest_results` across parse (cleared only by `pinyin_reset`, `pinyin.cpp:2693-2704` vs `:1497-1524`) and rebuilds candidates from scratch each `guess_candidates` (`:2184-2300`), so at `0x1f` (`SORT_WITHOUT_SENTENCE_CANDIDATE`) the imported user NORMAL remains when the NBEST prepend is skipped (`:2295-2296`). Oxpinyin clears nbest on every `begin_parse` (`reset_parse_state` → `sentence.reset()`, `instance.rs:145-194`; `state.rs:170-172`) and applies NBEST-wins text dedup before the `0x1` filter (`guess.rs:45-103`; `lookup.rs:558-580`; `sentence.rs:264-389`), so at `0x1f` the user row is gone with the sentences. User-visible: sequence-dependent presentation — after a sentence guess, ibus-style `0x1f` still offers the imported phrase on the pin and may not on oxpinyin (`probe-coverage-abi.md` C) |
| 35 | User-library tokens refused an n-best step cost (the missing user-phrase sentence path) | **CLOSED** in code (2026-09-20, `7c9a6923`) | **closed (#548 corrected this cell and the totals):** `nbest_step_costs_with_user_delta` now mirrors the pin's presence gate — a visible `USER_FILE` token with no system unigram is priced from its user delta (`crates/oxpinyin-data/src/lm/mod.rs:570-576`), a masked library's token or a missing item keeps the default; measured by `7c9a6923`, same-dir on the pin's `data/` (tkrzw): `residue-a-tail-diff` phases A and X byte-identical (`A-1e:n=128`; `clear_constraint(0)=true`; 你好 161 → 644 after the train) and `step_costs(sentence_start → 0x07000002)` pricing the token at 14.8278 nats. Original record: no exception class fits; `BigramLanguageModel::nbest_step_costs_with_user_delta` (`crates/oxpinyin-data/src/lm/mod.rs:532-548`) destructures `unigram_count(token)` — the system chunk libraries only (`:340`; `phrase_libraries.rs:179`) — and returns no cost when it is `None`, before merging the user delta it already holds (`:545`), so **every USER_DICTIONARY / NETWORK_DICTIONARY token is refused an n-best step cost and no imported or learned phrase can enter a sentence path**; the expansion in `crate::nbest::expand_entry` (`nbest.rs:677`) pushes nothing for such an entry although the widen probe reaches the span and the merged dictionary returns the token. The pin prices any loaded sub-index's item, the user library included: `unigram_gen_next_step` (`phonetic_lookup.h:643-668`) over `get_phrase_item`, `_add_phrase` having written the phrase into the pinyin table and the phrase index with `count × 3` (`pinyin.cpp:597-605`). Measured 2026-09-19 (`debian:testing` container `7cfeefdaf53f`, image `sha256:dab11cdb0a9d…`, same-dir on the pin's `data/`; `probe-coverage-abi.md` A): after importing 你好世界/9, the pin's rank-0 tail for `nihaoshijie` is the single user token at `m_poss = −14.8274994` and oxpinyin's n-best is the pin's list shifted up by one; `step_costs(sentence_start → 0x07000002)` answers `None/None` on oxpinyin; a counterfactual build pricing the token from its user delta lands it at 14.827804 nats (`\|Δ\| = 0.0003`, the row-11 band) and closes the residue with every consequence of the absent path. User-visible: the 1-best after an import is the imported phrase itself on the pin and the same text assembled from system phrases on oxpinyin; a non-best row choose forces fewer phrases (`clear_constraint(0)` false vs true), the train after it moves fewer unigrams, every `0x1e` window carries one extra sentence row, and a phrase the user imported or learned as a unit never shapes a sentence. Independent of rows 33/34 (the common-root experiment, `probe-coverage-abi.md`); fix shape and pre-registered differential there; work order `revert-plan.md` §12 |
| 36 | Bigram export iterator: `pinyin_bigram_iterator_get_next_phrase`'s return value on the last row | **CLOSED** in code — lane B (#607 `6c9bc75d`, #608 `569420c1`; merged 2026-10-01 UTC) | **closed (lane B, #541; recorded 2026-10-02 UTC at `a3ef00f5`):** #607 (merge commit `6c9bc75d`, code `3415b232`) makes `pinyin_bigram_iterator_get_next_phrase` answer `pinyin_bigram_iterator_has_next_phrase` after the increment (`pinyin.cpp:896-911`), `false` on the last row; #608 (merge commit `569420c1`, code `d0d849af`) exports from the pin's own in-memory user-bigram container — its walk order, its skipped last key and its `sentence_start` attribution. Both are measured by those PRs' `bigram-export-diff` differential (`tools/bisection/run-bigram-export-diff.sh`). Original record, kept as written: still present at `e1d915d0`: `pinyin_bigram_iterator_get_next_phrase` answers `true` after the increment (`crates/oxpinyin-capi/src/iterators.rs:406-407`), audit D-19 (`你好\|ni'hao\|138\|false` on the pin, `…\|true` on oxpinyin); #541 adds the DB-walk export order, the pin's skipped last key and its `sentence_start` attribution on the same surface. no exception class fits; a plain ABI return-value divergence. The pin fills the out-params, advances, and returns `pinyin_bigram_iterator_has_next_phrase(iter)` (`pinyin.cpp:896-911`) — `false` on the last row; oxpinyin returns `true` whenever a row was fetched and `false` only once exhausted (`crates/oxpinyin-capi/src/iterators.rs:373-410`). The unigram export iterator is not affected: the pin's `pinyin_iterator_get_next_phrase` returns `true` on every row (`pinyin.cpp:698-769`), as oxpinyin's does. Measured 2026-09-19 (`debian:testing` container `7cfeefdaf53f`, same-dir on the pin's `data/`; `residue-a-tail-diff` phase X2 after a constrained train): `X2-train1:bigram[0]=false 你好时节\|ni'hao'shi'jie\|138` on the pin, `true` on oxpinyin, one row exported on both. Invisible to the union probe while the pin's export was empty in every probed state (row 33). Consumer-visible in exactly one place: ibus-libpinyin 1.16.5 wraps the call in `check_result` (`PYLibPinyin.cc:321`), `assert` outside `NDEBUG`/`G_DISABLE_ASSERT` builds (`PYUtil.h:49-53`) — a debug ibus exporting a user dictionary with at least one bigram row aborts on the pin's last row and completes on oxpinyin; release builds discard the value. Fix: return `handle.index < handle.rows.len()` after the increment (the pin's value, since `has_next_phrase` is a pure read of the remaining rows on oxpinyin's pre-rendered list); differential: phase X2's `bigram[0]` line identical, plus a two-row export (import two pairs, train each) asserting `true` then `false` on both sides. Work order `revert-plan.md` §13 |
| 37 | Candidate window behind the composition offset after a choose | **CLOSED** in code — #604 (merge commit `06ede4dc`), #609 (`563ed9b1`), #625 (`c6a61be8`) | **closed (2026-10-03):** #604 (merge commit `06ede4dc`) builds the window from `start = offset` and lets a choose behind the composition move the record back (`pinyin.cpp:2184-2262`, `:2578-2590`); #609 (`563ed9b1`, code `47eea9aa`) maps a key boundary to the matrix column the key sits at; #625 (`c6a61be8`, code `f3053ac3`) does the same for double pinyin, Luoma and secondary zhuyin, including the empty mid-key column. Phases C, M and S, and the transformed-scheme phase T and mid-key phase K of `candidate-assembly-diff`, are identical on tkrzw, bdb and kc on the tree of `main` `c6a61be8`. Original record, kept as written: no exception class fits; the pin's `pinyin_guess_candidates` rebuilds the window from `start = offset` over the whole-composition matrix on every call (`pinyin.cpp:2184-2262`) and its instance carries no composition offset — a choose writes a constraint and answers a cursor (`:2501-2590`) — where oxpinyin advances a composition offset on every choose (`session/selection.rs:229,252`), rebuilds its cached list there (`scan_window(anchor = consumed)`, `session/lookup.rs:102-110`; a fully-consumed anchor yields the n-best rows alone), and the C ABI re-anchors only for a lookup offset strictly past the composition offset (`crates/oxpinyin-capi/src/sentence.rs:319-339`), serving the cached list for any offset at or behind it. Measured 2026-09-19 (`debian:testing` container `70d00b22eee3`, image `sha256:dab11cdb0a9d…`, same-dir on the pin's `data/`; `residue-a-tail-diff` phase E, `probe-coverage-abi.md` E). **At its worst measured point** — `guess_candidates(0, 0x1f)` after a whole-composition NBEST choose and re-guess — the pin answers 127 candidates headed by the imported user phrase and oxpinyin answers **0, an empty list**; at the ordinary partial choose (E2: 你好 chosen for `nihaoshijie`, cursor 5) `guess_candidates(0, …)` on oxpinyin answers the offset-5 list (世界 时节 …, 301 phrases) where the pin answers the offset-0 one (你好世界 你好 你 …, 127). At the choose's own offset both sides agree. Consumer routes: ibus-libpinyin 1.16.5 under preset 2 forces `lookup_cursor = 0` (`PYPPhoneticEditor.cc:352-355`) and calls `guess_candidates(0, 0x1f)` after every partial choose, so offset 0 behind a choose is that preset's normal path; and `moveCursorLeft` (`:595-604`) puts the lookup offset behind a choose under every preset. Independent of rows 34 and 35 (unchanged under the row-35 counterfactual build; no parse between the choose and the guess, n-best rows present on both sides). Fix shape and pre-registered differential in `probe-coverage-abi.md` E; work order `revert-plan.md` §14, executing second |
| 38 | `pinyin_train` requested n-best row and both facades’ empty-result gate | **(c)** — bounds, both halves met; valid-index and empty-result defects **CLOSED** in code (#524) | Measured 2026-10-04 UTC on bdb, kc and tkrzw, linux/amd64: `Session::train_nbest` reads the requested row’s existing spans (`crates/oxpinyin-engine/src/session/selection.rs:361-376`); `InstanceCore::train(index)` rejects empty decoded results before marking user data modified (`crates/oxpinyin-facade/src/instance.rs:245-259`). Both fresh instances and parse → NORMAL choose → train without `guess_sentence` return quiet false and write no training data, including zhuyin (`crates/oxpinyin-zhuyin-capi/src/candidates.rs:272`). Valid pinyin indices 0/1/2 train distinct rows; all three cells match pin `074a2219` return values, exported native records and complete `.dbin` bytes (`src/pinyin.cpp:2678-2688`; zhuyin `src/zhuyin.cpp:1704-1713`). Bounds on nonempty results are approved class (c): false, no training writes, exactly one `log_warning` in domain `libpinyin`, level 16 (`crates/oxpinyin-capi/src/candidates.rs:600-606`), where pin asserts at `:2684`. `python3 tools/bisection/train-index-diff.py CELL PREFIX PINYIN_SO ZHUYIN_SO`: 10 cases per cell, 30 passed; `--expect-parent` reproduces ignored-index, both selection-only empty-result defects and bounds true/write/no-warning. §12 remains 491/396/390 on every cell. |
| 39 | `pinyin_init`/`zhuyin_init` leave the process `LC_NUMERIC` at `"C"` | **CLOSED** in code — #618 (merge commit `a3ef00f5`, code `e5400598`; merged 2026-10-01 UTC); ruled bug-for-bug 2026-09-27 UTC (#539) | **closed (#539, #618; recorded 2026-10-02 UTC at `a3ef00f5`):** `pinyin_init`/`zhuyin_init`, `pinyin_save`/`zhuyin_save` past their guards and `pinyin_fini` now install `LC_NUMERIC` `"C"` through the C library's own `setlocale` at the points the pin does (the two C-ABI crates, `oxpinyin-capi` and `oxpinyin-zhuyin-capi`); `tools/bisection/run-locale-diff.sh` diffs the two libraries' logs byte for byte on the success path and six failure forms of init. Original record, kept as written (its "oxpinyin never calls `setlocale`" is the state before #618): no exception class fit, and the ruling was to reproduce it: the pin's `UserTableInfo::load` and `SystemTableInfo2::load` save `setlocale(LC_NUMERIC, "C")`'s return — the *new* locale's name, not the old one — and restore it on success (`storage/table_info.cpp:328,372` and `:197,291` at `074a2219`; `save` the same, `:378,394`), while every early return skips the restore (e.g. `:330-333`, `:339-348`, `:199-202`), so after any init, successful or not, `LC_NUMERIC` is `"C"`. oxpinyin never calls `setlocale` (no hit in `crates/`), so a host that set `LC_ALL=zh_CN.UTF-8` still reads `zh_CN.UTF-8` after init (audit D-17, `bug-for-bug-audit-r2-2026-09-23.md` §4.1). Recorded as an upstream defect (`upstream-report-drafts.md` item 6) |
| 40 | tkrzw binding: `SYSTEM_ERROR`/`UNKNOWN_ERROR` collapse | **STANDING** (registered standing divergence, ruled accepted 2026-08-28; language-quirk exception per the 2026-10-03 ruling; not a numbered class) | registered 2026-09-27 UTC (#551): the ruling existed only in `tkrzw-langc-exception-classification.md`. tkrzw's C API reports every caught C++ exception as `TKRZW_STATUS_SYSTEM_ERROR`, which `crates/oxpinyin-store/src/tkrzw/mod.rs:214` maps to `StoreError::Io`, the same class as an operating-system error; the retired cxx shim reported `UNKNOWN_ERROR` → `Backend`. Reachable only under memory exhaustion; not observable through the C ABI. Language-boundary consequence (ruling 2026-10-03): Rust cannot catch C++ exceptions and the tkrzw C API merges them into `SYSTEM_ERROR`. See "Registered standing divergences" |
| 41 | NULL pointer arguments across 68 exports | **(b)** | registered 2026-09-27 UTC (#526, audit D-04). At `074a2219` each of 68 `pinyin_*` exports dereferences a NULL argument before any check, and the pin receives SIGSEGV (70 of 83 NULL-class probes; the 13 that do not crash are excluded). #526's per-export table cites the first dereference and oxpinyin's guard for each; the patterns are: the instance first (`pinyin.cpp:1312`, `pinyin_alloc_instance`); the context or iterator first (`:509`, `:665`, `:777` at iterator creation; `:1196` in `pinyin_fini`); an out-parameter written before any check (`:2847` `*num = instance->m_candidates->len`, `:2876`, `:2982`); a NULL candidate inside a live `assert` (`:2507`, `:2593`); a NULL key or key-rest (`:2711`, `:2722`, `:2733`, `:2744`). Dereferencing NULL is undefined behaviour; oxpinyin opens every export with an `is_null()` guard and answers `false`/`0`/NULL/void (e.g. `crates/oxpinyin-capi/src/instance.rs:19`, `iterators.rs:63`, `candidates.rs:309`, `cursor.rs:241`, `keys.rs:219`). The libzhuyin exports show the same shape (#526, comment of 2026-09-25) |
| 42 | A guess on an instance whose context was finalised | **(b)** | registered 2026-09-27 UTC (#528, audit D-06, probe `instance_outlives_context`). `pinyin_fini` deletes the context and its members (`pinyin.cpp:1194-1221`) but not the instances allocated from it; `pinyin_guess_sentence` on such an instance reads `instance->m_context->m_pinyin_lookup` (`:1373-1380`) — a use after free, and the pin receives SIGSEGV. oxpinyin's instance holds its own handles to the shared state (`crates/oxpinyin-capi/src/state.rs:69-77`, `core.alloc_instance()`), so the guess completes and answers `true`. Only this half is (b): the inverse probe `alloc_after_fini` crashes oxpinyin (`crates/oxpinyin-capi/src/instance.rs:18-27` dereferences the freed context) and survives on the pin by chance — caller UB on both sides; the oxpinyin crash is closed by row 60 (a liveness registry answers NULL), and the orphan guess itself needs no change (ruled 2026-10-04) |
| 43 | Zhuyin import into library index 16 | **(b)** | registered 2026-09-27 UTC (#526, comment of 2026-09-25). `zhuyin_begin_add_phrases` accepts any `guint8` (`zhuyin.cpp:392-398`); a following `zhuyin_iterator_add_phrase` reaches `_add_phrase`'s `phrase_index->get_range(index, …)` (`zhuyin.cpp:475`), which reads `m_sub_phrase_indices[16]` (`storage/phrase_index.cpp:611`) one past the 16-entry array (`storage/phrase_index.h:441`) and calls through what it finds: an out-of-bounds read, SIGSEGV on the pin on all three cells. oxpinyin answers `false` for every library that is neither a `USER_FILE` library nor a loaded system library (`crates/oxpinyin-zhuyin-capi/src/iterators.rs:90`; since #612, merge commit `c99ffb57`, the system libraries 1-4 are accepted as on the pin), library 16 and 16..=255 included. The pinyin facade's `_add_phrase` has the same unchecked index (`pinyin.cpp:589`), but the audit's D-12 recorded library 255 *accepted* on the pin, so that side's outcome is not a stable crash and is not part of this row |
| 44 | Unknown `database format:` in `user.conf` aborts the pin; oxpinyin refuses the open | **(c)** — both halves met (#591, merged 2026-09-26) | registered 2026-09-27 UTC from #591's "Register impact". Site: `storage/table_info.cpp:132` **`abort()`** — the fall-through of `to_table_database_format_type` (`:122-133`), called at `:353-354` from `UserTableInfo::load` (`:351-352`: `char str[256]`, `fscanf(input, "database format:%255s\n", str)`), which `check_format` calls (`pinyin.cpp:172-178`, `zhuyin.cpp:126-132`) and whose result `pinyin_init`/`zhuyin_init` ignore (`pinyin.cpp:344`, `zhuyin.cpp:288`), so the `abort()` is what stops them. Condition: the third `fscanf` converts a token (it returns `1`) that is not `BerkeleyDB`, `KyotoCabinet` or `Tkrzw` — the source guarantees the `abort()` there. When the directive's literal fails to match (a missing `database format:` line, or junk left behind by the line before it) `fscanf` returns `0`, not `EOF`, and writes no `str`: `strcmp` then reads an indeterminate stack buffer (undefined behaviour, not a source-guaranteed abort). Execution data for those two shapes: `tools/bisection/run-open-counter-diff.sh`'s `abort-modelver-junk` and `abort-no-dbformat` cases, oracle exit 134 on every cell in #591's runs. Trigger: an edited, foreign or torn `user.conf` (`UserTableInfo::save` is `fopen`/`fprintf`/`fclose` with no atomic rename, `table_info.cpp:377-395`); #591 executed 24 runs across both facades and all three cells, every one SIGABRT with the user dir untouched. oxpinyin: `UserTableInfo::parse` answers `Err(UserConfError::UnknownDatabaseFormat)`, `persistence::load` propagates it before any conformance judgement (`crates/oxpinyin-user/src/persistence.rs:66-72`), `Runtime::open` turns it into `OpenError::UnknownDatabaseFormat`, and `pinyin_init`/`zhuyin_init` answer NULL with nothing cleaned and no marker written, logging exactly one line through GLib at warning level, `check_format: unknown database format in user.conf` (`oxpinyin_facade::UNKNOWN_DATABASE_FORMAT_WARNING`, `crates/oxpinyin-facade/src/context.rs:49-50`; emitted at `crates/oxpinyin-capi/src/context.rs:25`, `crates/oxpinyin-zhuyin-capi/src/context.rs:48`). The previous behaviour (unknown token → wipe) was itself a divergence, which this row replaces. No byte differential is possible at the abort; `tools/bisection/run-open-counter-diff.sh`'s `abort-*` channel asserts the pair (oracle exit, NULL, one log line, untouched dir). The `table.conf` path to the same `abort()` (`:232-233`) stays silent (#525 ledger, group B) |
| 45 | `pinyin_choose_candidate` under `SORT_WITHOUT_SENTENCE_CANDIDATE` with a nonzero offset | **(c)** — both halves met (#598, merged 2026-10-01) | registered 2026-10-03 from #598's "Register impact" (merge commit `efa8337a`). Site: `src/pinyin.cpp:2566` **`assert`** (`0 == offset`), the first statement of the `0x1` choose leg (`:2565-2576`); ibus-libpinyin 1.16.5 always passes 0 on that path (`PYPLibPinyinCandidates.cc:139-145`), so only a caller that passes another offset reaches it. Executed on freshly built pin oracles (asserts live), tkrzw, bdb and kc: a NORMAL row chosen at offset 1 after `pinyin_guess_candidates(0, 0x1)` aborts with `Assertion '0 == offset' failed` (SIGABRT, exit 134); at offset 0 it answers `1`. oxpinyin answers `0` and logs exactly one `g_warning` in the `libpinyin` domain, `pinyin_choose_candidate: offset must be 0 under SORT_WITHOUT_SENTENCE_CANDIDATE` (`crates/oxpinyin-capi/src/candidates.rs:362-371`) |
| 46 | Empty-string user dir: `pinyin_init`/`zhuyin_init` given `userdir ""` keep the profile in the working directory | **CLOSED** in code (#619) | registered 2026-10-04 UTC with its fix (#619, verdict DIVERGENT-UNREGISTERED; no exception class fits — the behaviour is defined, has no float in it and aborts nothing — so it was a defect to revert, not a divergence to keep). The pin stores `g_strdup(userdir)` (`pinyin.cpp:332`, `zhuyin.cpp:276`), its only guards test the pointer (`pinyin.cpp:1133`, `:2671`; `zhuyin.cpp:548`, `:1697`) and `g_build_filename` drops an empty element, so with `""` every user file is a bare name, resolved against the directory current at that file operation: the context trains, a dirty save writes the profile there and resets `LC_NUMERIC` (row 39), and a consumer that changes directory after init saves into the new one. A NULL `userdir` builds the empty string instead: train and save answer `false` and nothing is written. oxpinyin folded `""` into "no user dir" (`crates/oxpinyin-runtime/src/lib.rs:1207` at `8cd06566`, after the two inits had folded NULL into `""`): `pinyin_train`/`zhuyin_train` and the saves answered `false` and nothing was written. Measured 2026-10-04 UTC, linux/amd64, on bdb, kc and tkrzw, both facades (#619). The fix keeps the two apart from the C boundary down (`crates/oxpinyin-capi/src/context.rs`, `crates/oxpinyin-zhuyin-capi/src/context.rs`, `ContextCore::try_open`'s `Option<&str>`) and opens the store on the empty path, kept relative (`crates/oxpinyin-runtime/src/lib.rs`, `Runtime::open_with_law`). `tools/bisection/run-locale-diff.sh` (row 39's differential, extended) holds five forms of the argument — an absolute directory, `""`, `""` with a chdir after init, NULL and `"."` — as two consecutive processes each, byte for byte on return values, `LC_NUMERIC`, each directory's inventory and `user.conf`, and the trained word's unigram frequency: identical on bdb, where the fix was gated. Not this row: a NULL user dir's in-memory imports (#642) and the empty-string system dir (#643) |
| 47 | `pinyin_init`'s default option word | **CLOSED** in code (#532, lane C) | the pin seeds `context->m_options = USE_TONE` (`pinyin.cpp:329`, bit 5) and nothing else; oxpinyin seeded `PINYIN_INCOMPLETE` (bit 3), so a caller that never called `pinyin_set_options` got incomplete keys and no tone digits. Measured on bdb at `b04ace57` (`contract-diff.py`, cases `default-option-parse`, `default-option-single-key`): bytes consumed for `su3cl3` / `ni2hao` / `n` / `zzz` / `ni3` were pin 3 / 6 / 0 / 0 / 3 against oxpinyin 2 / 2 / 1 / 3 / 2, and `parse_full_pinyin` answered `n` true (`0x0b`) and `ni3` false where the pin answers false and true (`0x302b`). `PINYIN_DEFAULT_OPTION_WORD` is now `USE_TONE` (`crates/oxpinyin-facade/src/lib.rs:58-61`; read at `crates/oxpinyin-capi/src/state.rs:62`); the zhuyin word is untouched. Churn: the 8 tests that had encoded the old default now set `PINYIN_INCOMPLETE` explicitly (5 in `crates/oxpinyin-capi/src`, 3 in `crates/oxpinyin-capi/tests/abi/keys.rs` through `Fixture::new`); no fixture changes — `fixtures/` and `tests/parity` are captured with explicit words — and the 11 `tools/bisection` drivers that never call `pinyin_set_options` and take `<so> <systemdir>` are IDENTICAL to the pin at both defaults. Verified on bdb: all four cases MATCH, and against the parent build the two default cases DIFFER while the two explicit-word controls MATCH. |
| 48 | Invalid UTF-8 after valid pinyin in the multi-key parse entries | **CLOSED** in code (#587, lane C) | the pin hands the parser the raw bytes and `strlen` (`pinyin.cpp:1498-1509`; the reading of `pinyin_iterator_add_phrase`, `:638`), and a parser stops at the first byte that is no part of a key, so valid pinyin before an invalid sequence is consumed and counted. oxpinyin's `cstr_to_string` (`crates/oxpinyin-capi/src/ffi.rs`) mapped the whole argument to the empty string. Measured on bdb at the parent: `ni\xffhao` / `nihao\xff` / `ni\xe4\xbd` consumed 2 / 5 / 2 at the pin and 0 / 0 / 0 in `pinyin_parse_more_full_pinyins`, `_double_pinyins` and `_chewings` alike (and the same for `zhuyin_parse_more_chewings`), and `pinyin_iterator_add_phrase(你好, "ni'ha\xffo")` answered true at the pin and false in oxpinyin. The multi-key parse entries and the add-phrase reading now take the longest valid UTF-8 prefix (`cstr_to_parsed_prefix`, `crates/oxpinyin-capi/src/ffi.rs` and its zhuyin twin). Left alone on purpose because the pin answers the same: the single-key parsers (`pinyin_parse_full_pinyin` gets the whole argument, `ni\xff` fails at both), `lookup_tokens`, `remember_user_input`, the phrase argument of `iterator_add_phrase` (`g_utf8_to_ucs4` refuses it), and the zhuyin add-phrase reading (the direct parser splits on spaces, so a last token with an invalid byte is refused whole). Verified on bdb with `contract-diff.py` cases `bytes-*`: all MATCH, and against the parent build the six fixed cases DIFFER while the two controls MATCH. |
| 49 | `pinyin_init` and `zhuyin_init` with a non-UTF-8 directory path | **CLOSED** in code (#587, lane C) | the pin passes the `char *` paths to `g_build_filename` and `fopen` as bytes (`pinyin.cpp:331-336`, `zhuyin.cpp`), so a system directory whose name contains an invalid UTF-8 byte opens. Before the path-based fix, oxpinyin converted non-UTF-8 system paths to an empty string and non-UTF-8 user paths to no user directory. Measured on bdb at `b04ace57`: a symlink named `bad\xffdir` to the data directory opened at the pin (both facades) and failed in oxpinyin. **Closed (2026-10-04, lane C, PR 13):** `ContextCore::open` and `try_open` take the system directory as a `&Path` and the user directory as an `Option<&Path>` (`None` is a NULL user dir, `Some("")` the working directory, as in #646), and both C facades hand them the caller's bytes as they are (`ffi::cstr_to_path`). A non-UTF-8 user directory is a user directory again, no longer "no user dir". Verified on bdb with `non-utf8-directory-system`, `-user`, `-both` and `-zhuyin-both` of `contract-diff.py`: init, a sentence, a training and a save, and the files left in the user directory, match the pin; against the parent build the four fail. |
| 50 | Out-params the pin leaves alone on failure, and writes on a failed walk | **CLOSED** in code (#542, lane C) | four contracts of rows 2, 6, 7 and 20 of the round-2 contract battery. (1) `pinyin_get_sentence` with no row to answer returns `false` with `*sentence` untouched (`pinyin.cpp:1470-1471`); oxpinyin wrote NULL (`crates/oxpinyin-capi/src/sentence.rs`). (2) `pinyin_get_character_offset` whose walk fails stores the length it reached, `*plength = length` (`pinyin.cpp:3238`), 0; the earlier `false` returns (empty matrix, empty phrase, a character without a token: `:3200-3228`) leave it untouched. oxpinyin left it untouched on every `false`; the engine answers all of them with `Ok(None)`, so the capi layer now tells them apart from the matrix, the phrase and the dictionary (`character_walk_ran`). (3) `pinyin_token_get_phrase` on an unknown token returns `false` with both out-params untouched (`_token_get_phrase`, `:1627`); oxpinyin wrote NULL into the string. (4) `pinyin_token_get_nth_pronunciation` empties the key array first (`:2805`), so an unknown token leaves it empty (oxpinyin left the caller's content), and an `nth` past the last reading answers `true` with `phrase_length` keys (`:2808-2817`; oxpinyin answered `false`). The content of those keys is registered as row 51. Measured on bdb at the parent: pin `[false, untouched]` against oxpinyin `[false, NULL]`, and `[false, 0]` against `[false, untouched]` for `啊`@3, `你`@5 and `好`@5 on `nihao`. Verified on bdb with the `empty-parse-sentence`, `character-offset-out`, `token-get-phrase-out` and `nth-pronunciation-range` cases of `contract-diff.py`: all MATCH, and all four DIFFER against the parent build. The zhuyin twins (`zhuyin_get_sentence` writes NULL the same way) are not touched. |
| 51 | `pinyin_token_get_nth_pronunciation` keys past the last reading | **(b)** | an `nth` past the last reading appends `phrase_length` keys read from `ChewingKey buffer[MAX_PHRASE_LENGTH]`, a stack array that is never initialised, because the return value of `PhraseItem::get_nth_pronunciation` is ignored (`pinyin.cpp:2808-2817`; `MemoryChunk::get_content` refuses the out-of-range read and writes nothing). The content is whatever the stack held: `0000 0000` for `nth` 1 and 2 and `604f0000` for `nth` 2^32−1 on bdb, depending on earlier calls, so no safe construction can reproduce it. Ruled 2026-10-04: `true` with zeroed keys (`crates/oxpinyin-capi/src/dict.rs`, `pinyin_token_get_nth_pronunciation`). The return value and the key count are the pin's and are held byte for byte by `nth-pronunciation-range`; the content is recorded in the case under a `~` key and left out of the comparison. |
| 52 | The `ChewingKey` and `ChewingKeyRest` slots are process-wide, and the key-rest length is a `guint16` difference | **CLOSED** in code (#542, lane C) | the pin hands out `&` of function-local `static`s (`static ChewingKey key`, `pinyin.cpp:2936`; `static ChewingKeyRest key_rest`, `:2960`): one slot for every instance, overwritten by the next call from any instance, valid after the instance is freed. oxpinyin kept a slot per instance (`CapiInstance::key_slot`, `key_rest_slot`), so two instances got different pointers, the first instance's key survived the second's call, and the pointer dangled once the instance was freed. `pinyin_get_pinyin_key_rest_length` is `key_rest->length()`, `m_raw_end - m_raw_begin` in `guint16` (`chewing_key.h:111-113`, `pinyin.cpp:2979-2984`), 65533 for a rest of (5, 2) and 1 for (65535, 0); oxpinyin saturated at 0. Measured on bdb at the parent: pin `key same pointer` true and the first instance's key `2b00` → `8602` after the second call, oxpinyin false and unchanged; pin 65533 and 1, oxpinyin 0 and 0. Both slots are now `static`s of `AtomicU16` (`crates/oxpinyin-capi/src/cursor.rs`), so the Rust side has no data race and the C side reads through the pointer as it reads the pin's static; the length uses `wrapping_sub`. The per-instance fields are gone from `CapiInstance`. The zhuyin twins keep their per-instance slots. Verified on bdb with `static-key-slots` and `key-rest-length` of `contract-diff.py`: MATCH, and both DIFFER against the parent build. |
| 53 | Sentence, candidate and unload return values on the pin's empty-state paths | **CLOSED** in code (#542, lane C) | five return-value contracts of rows 1, 3, 10 and 21 of the round-2 battery, each measured on bdb at the parent. (1) `pinyin_get_sentence` with no row — before any `pinyin_guess_sentence`, after a reset, after a double or chewing parse — answers `false` with `*sentence` untouched (`pinyin.cpp:1470-1471`); oxpinyin answered the raw preedit (`nihao`, `nihk`, `su3cl3`) with `true`, the W14 remnant (`docs/findings/sentence-surface.md`). The raw branch is gone from `crates/oxpinyin-capi/src/sentence.rs`. (2) `pinyin_guess_sentence` and `pinyin_guess_sentence_with_prefix` on a parse that placed no key (`'`, `''`, `!`, or an empty string on a fresh instance; for an empty reparse on a reused instance, see row 54) answer `false` before they clear anything (`get_nbest_match`, `phonetic_lookup.h:743-745`; `fill_matrix` leaves the matrix empty for a keyless parse, `phonetic_key_matrix.cpp:34-38`), so the rows of an earlier guess stay readable; oxpinyin answered `true` for `'` and `''` and dropped the rows. `e2e_tests::parse_termination::stop_bytes_terminate_the_parse` had recorded "the pin answers true" there; at 074a2219 the pin answers `false` for one, two and three apostrophes under the default and the parity word. (3) `pinyin_guess_candidates` on the same matrix frees the list and answers `false` (`pinyin.cpp:2193-2196`); it now does, which keeps (2) from showing stale sentence rows as candidates. (4) `pinyin_unload_phrase_library(GBK)` answers `true` on every call, because the pin ignores what `unload` returns (`pinyin.cpp:473-474`); oxpinyin answered `false` on a repeat (`RuntimeDict::unload_library`, which the zhuyin facade shares, so `zhuyin_unload_phrase_library(2)` repeats `true` as well). (5) `pinyin_guess_sentence_with_prefix` with an invalid UTF-8 prefix decodes as if the prefix were empty (`g_utf8_to_ucs4` gives NULL and `_compute_prefixes` adds no token, `:1397-1403`); oxpinyin refused with `false` and left no row. Verified on bdb with the `sentence-before-guess`, `sentence-before-guess-schemes`, `guess-sentence-keyless`, `guess-candidates-keyless`, `unload-phrase-library-repeat` and `sentence-with-prefix-bytes` cases of `contract-diff.py`: all MATCH, and all six DIFFER against the parent build. |
| 54 | Three parse and lookup divergences seen beside #542 | **REVERT TARGET** for items 2 and 3; item 1 **CLOSED** in code (2026-10-08 UTC) | found while building the row 53 cases, not fixed there because each lives in the engine session. Measured on bdb at the parent of this change. (1) **Closed:** original-coordinate leading empty columns are retained for generation and candidate lookup; key/rest accessors preserve facade-specific zero-key laws, and pin assertions return false with exactly one warning. The six-input, both-facade ledger covers every offset, including the 13 previously incomplete zhuyin class-(c) sites (all already cited by rows 19 and 66, mapped in the findings); see [separator matrix findings](separator-matrix.md), #640 and #698. Original observation: a leading apostrophe: after `parse("'nihao")` the pin's `pinyin_guess_sentence` answers `true` and `pinyin_get_sentence(0)` answers `false` (no n-best row, `0 == results.size()`, `pinyin.cpp:1470`); oxpinyin decodes `你好` and answers `true`. (2) A parse of the empty string after an earlier parse: the pin consumes 0, clears the matrix and the next `pinyin_guess_sentence` answers `false`; oxpinyin keeps the earlier composition and answers `true`. (3) `pinyin_guess_candidates` repeated after `pinyin_unload_phrase_library(2)` without a re-parse: the pin rebuilds the list (126 → 47 candidates for `nihao` at the sort word `0x1e`), oxpinyin answers its cached list (126 → 126); with a re-parse both give 47. |
| 55 | The three auxiliary-text functions read one shared matrix | **CLOSED** in code (#542, lane C) | the pin fills one `PhoneticKeyMatrix` per instance whichever parser ran, and `pinyin_get_full_pinyin_auxiliary_text`, `_double_pinyin_` and `_chewing_auxiliary_text` all read it (`pinyin.cpp:3295-3576`): they differ only in how they cut a key the cursor splits (pinyin string, shengmu and yunmu one or two bytes in, zhuyin characters) and in which spelling the other keys take. So after a full parse the double and chewing functions answer `true` (`\|ni hao `, `\|ㄋㄧ ㄏㄠ `), and after a double or chewing parse the full one does. oxpinyin kept one rendering per parse mode and answered `false` with an empty string for the other two functions, and its full renderer rebuilt a segment graph from the double or chewing input (`ni h` for `nihk`). Its double renderer also left the tone digit out. Measured on bdb at the parent: 9 of 9 (parse mode × function) combinations differ across the `aux-text-*` cases, tones included (`\|ni3 hao4 ` at the pin against `\|ni hao `). `crates/oxpinyin-capi/src/aux_matrix.rs` rebuilds the matrix from the instance's current parse (a key at the column where it begins, a zero key at each `'` and at the last column, `fill_matrix`, `phonetic_key_matrix.cpp:27-80`) and ports the three loops; the double function now always uses it, and the full and chewing functions use it when another parser filled the matrix. A parse that placed no key answers `false` with an empty string from all three (`fill_matrix` clears the matrix, `:34-38`). The pin's assertion shapes (`pinyin.cpp:3311` and the column reads at `:3353`, `:3410`, `:3467`, `:3545`, a double cursor three bytes into a key at `:3488`) stay silent `false`; the class (c) log is the later #525 change. Verified on bdb with the seven `aux-text-*` cases of `contract-diff.py`: all MATCH, and the six that exercise the fix DIFFER against the parent build while `aux-text-no-parse` MATCHes. Review (2026-10-04): with several keys beginning at one byte (the divided, resplit and fuzzy alternatives) the port kept the last; the pin's renderers read the column's first item (`get_item(column, 0)`), so the first now stays. Held by `aux-text-divided-table`, `-resplit-table`, `-divided-and-resplit` and `-fuzzy-keys` (`xian`, `changan`, `jiangnan`… under each option), which differ against the parent build. A leading `'` (`'nihao`) was also asked about: the pin leaves column 0 empty and **aborts** at `pinyin.cpp:3311` on every cursor of all three functions, so there is no zero key to render and the `false` stands; the log is the class (c) change that follows (#525). |
| 56 | Unigram adds stop at the sub-index `guint32` total, in training and in predicted candidates | **CLOSED** in code (#540, lane C) | `FacadePhraseIndex::add_unigram_frequency` bumps the facade total unconditionally and the sub-index refuses the item's delta when `m_total_freq > m_total_freq + delta` (`phrase_index.h:628-635`, `phrase_index.cpp:169-171`). `train_result3` ignores the refusal (`phonetic_lookup.h:928-929`), so a trained item freezes once its library's total is full; `pinyin_choose_predicted_candidate` adds the unigram first and answers `false` on `ERROR_INTEGER_OVERFLOW`, before it trains the bigram (`pinyin.cpp:2609-2612`). oxpinyin guarded only the import path (`add_unigram_frequency`, `crates/oxpinyin-user/src/store.rs`) and the ABI add (`RuntimeDict::add_unigram_delta`); `update()` added the training and predicted deltas with a bare `saturating_add`. Measured on bdb at the parent: 你 after 10000 / 20000 / 30000 trainings was `1544842896` / `3090442896` / `4294967295` against the pin's `1544842896` / `2122897296` / `2122897296`; and after the trainings filled library 1, the pin accepted 264 predicted candidates before refusing, oxpinyin never refused. `update()` now routes both policies through the guarded add, training ignoring the refusal and the predicted policy returning the new `UserStoreError::UnigramTotalOverflow` (approved, ruling 11) before it touches the bigram; the facade total still takes the delta, as at the pin. Verified on bdb with `train-unigram-total` and `predicted-unigram-overflow` of `contract-diff.py`: MATCH, and both DIFFER against the parent build. Open edge, not part of this row: the ABI add (`pinyin_token_add_unigram_frequency`) keeps its accepted deltas in the runtime overlay and training keeps its deltas in the user store, so each guard sees the other's contribution only through the other's own bound; a mix of the two near a full library is not measured. |
| 57 | `pinyin_choose_predicted_candidate`'s return table, its predecessor, and `remember_user_input` counts below -1 | **CLOSED** in code (#542, #540, lane C) | (1) A punctuation row has no frequency and the pin answers `true` without training (`pinyin.cpp:2604-2605`); oxpinyin answered `false` (no token) on every prefix tried (我, 你好, 是, 好). (2) The bigram predecessor is `_get_previous_token(instance, 0)`: the longest token of `m_prefixes`, which `pinyin_guess_predicted_candidates` and `pinyin_guess_sentence_with_prefix` fill from their prefix text and `pinyin_guess_sentence` and `pinyin_reset` empty, `sentence_start` when empty (`pinyin.cpp:1711-1740`, `:2618`); oxpinyin used the last *selected* token (`crates/oxpinyin-capi/src/candidates.rs`). `CapiInstance::prefixes` now mirrors `m_prefixes`. (3) `pinyin_remember_user_input` takes any `gint` count other than -1 as the `guint32` it becomes (`pinyin.cpp:520-524`): -2, -5 and `INT_MIN` are accepted and exported as they went in; oxpinyin returned `false` for a negative count (`crates/oxpinyin-capi/src/user_data.rs`). The overflow answer of the same function (`false` before the bigram) is row 56. Verified on bdb with `choose-predicted-types` and `remember-count-*` of `contract-diff.py` (five fixed cases DIFFER against the parent build, two controls MATCH); the predecessor is pinned by `e2e_tests::a_predicted_bigram_trains_from_the_prediction_prefix`, because the pin orders bigram rows by unigram probability unless `DYNAMIC_ADJUST` is on, so no ordering shows it (see row 58). The e2e test that asserted the old "last selected token" predecessor now records the pin's. |
| 58 | Predicted rows: a prefix row trains a bigram, and the ranking ignores the user store | **REVERT TARGET** (#540, lane C) | found while building row 57's cases, not fixed. (1) For a `PREDICTED_PREFIX_CANDIDATE` the pin trains the unigram only and returns (`pinyin.cpp:2615-2616`); `UserStore::observe_predicted` always trains the bigram too, so a chosen prefix row later appears as a bigram row. The fix needs a unigram-only entry on `UserStore`, a public API addition the rulings do not cover. (2) Predicted prefix rows are ranked by the live unigram counter; oxpinyin's `append_predicted_prefix` reads the system count plus the runtime overlay only (`crates/oxpinyin-capi/src/predict.rs`), not the user store's deltas, so 21 accepted prefix rows move 是谁 from third to first at the pin and leave the list unchanged in oxpinyin. Adding the deltas moves it to first but leaves three rows (的心, 只想, 最爱) in a different order than the pin, a near-tie reshuffle, so it is not done. (3) Predicted bigram rows are ordered at the pin by the amplified unigram probability (the bigram term needs `DYNAMIC_ADJUST`), so 200 accepted choices of one row were needed to flip two rows; oxpinyin orders them by bigram count and flips after one. |
| 59 | The session's input cap: 4096 bytes against the pin's `gint16` | **CLOSED** in code (#540, lane C) | the pin's parser keeps `parsed_len`, `m_last_step` and the step values in `gint16`s (`pinyin_parser2.cpp:274-276`, `:290-291`, `:333`, `:349-360`), so no parse consumes more than 32767 bytes: `ni` repeated 4096 / 4097 / 32766 / 32767 / 32768 / 65537 times consumes 4096 / 4097 / 32766 / 32767 / 32767 / 32767, with no fault (`m_parsed_len` wrapping past 32767 never equals the step index, so `final_step` keeps the last step that did). oxpinyin's session accepted 4096 bytes (`oxpinyin_engine::MAX_INPUT_BYTES`, "the largest input the frozen F-A fixtures carry") and consumed 4096 for every longer input. The constant is now `32_767`; the narrowing arithmetic is implementation-defined, not undefined, in the pin's C++, so no class applies. Measured on bdb with `input-length-cap` (all six lengths, `parsed_input_length` too) and `long-input-sentence` (4800 bytes decoded to the same sentence) of `contract-diff.py`: MATCH, and both DIFFER against the parent build. Cost, debug build: parse of 32767 bytes 0.7 s, `guess_sentence` on it 115.8 s against the pin's 1.7 s (release); that call is not in the differential. The CI fuzz lanes keep `-max_len=4096` (`verify-nightly.yml`) and were left alone. |
| 60 | `pinyin_alloc_instance` on a finalised context | **(b)** | ruled 2026-10-04 (#528): the pin reads the freed context in `pinyin_alloc_instance` (`context->m_phrase_index`, `pinyin.cpp:1322`; valgrind on bdb: invalid read of size 8, 48 bytes inside a freed block of 1,464) and survives by chance, returning a non-NULL instance with a dangling constructor argument; no safe construction reproduces a read of freed memory. oxpinyin dereferenced the freed Rust context the same way and crashed (SIGSEGV at the parent; valgrind: invalid read in `ContextCore::alloc_instance`). A liveness registry (`crates/oxpinyin-capi/src/live.rs`) records the contexts `pinyin_init` hands out and `pinyin_fini` takes back; `pinyin_alloc_instance` answers NULL for an address that is not live, with no log, since the pin does not abort. Verified on bdb with `alloc-instance-after-fini` of `contract-diff.py` (the answer itself is a `~` field; the case holds the exit status: pin 0, parent -11, this change 0). Row 42's inverse probe is closed by this row. Entry in `upstream-divergences.md`. *Lane C, PR 14:* `zhuyin_alloc_instance` after `zhuyin_fini` reads the freed context at `zhuyin.cpp:845-857` the same way; it has its own liveness registry (`crates/oxpinyin-zhuyin-capi/src/live.rs`) and answers NULL silently (`zhuyin-alloc-instance-after-fini`). |
| 61 | The facade unigram total is a `guint32` that wraps, and a total of zero asserts | **(c)** — both halves met (#540, lane C) | `FacadePhraseIndex::add_unigram_frequency` does `m_total_freq += delta` on a `guint32` before the item is touched (`phrase_index.h:633`), so adds wrap the total; `_compute_frequency_of_items` then asserts `0 < total_freq` for every ranked non-prefix, non-addon candidate (`pinyin.cpp:1859`, **`assert`**; the libzhuyin twin is `zhuyin.cpp:1261`). Measured on bdb: after `pinyin_token_add_unigram_frequency` of `(2^32 − 51051831) mod 2^32` (51051831 is the loaded libraries' sum), `pinyin_guess_candidates("nihao")` aborts the pin (SIGABRT, `Assertion '0 < total_freq' failed`); oxpinyin summed in `u64` with saturation and answered `true`. The overlay total is now a `u32` that wraps (`RuntimeDict::unigram_total_delta() -> u32`, approved 2026-10-04), the language model's total wraps (`crates/oxpinyin-runtime/src/lib.rs`), and `pinyin_guess_candidates` answers `false`, empties its list and emits exactly one `g_warning` in domain `libpinyin` (level 16, `pinyin_guess_candidates: assertion '0 < total_freq' failed`, `crates/oxpinyin-capi/src/sentence.rs`) when the wrapped total is zero for a model with real unigrams. Verified on bdb with `facade-total-zero` of `contract-diff.py` (the pin aborts, the subject answers `false` with one warning); against the parent build the case fails. The zhuyin twin is not wired here (the later zhuyin class (c) change). Not part of this row, not measured: the candidate ranking's denominator (the library sum plus the user store's deltas, `RuntimeLm::unigram_total`) does not include the ABI add's overlay, which only the predicted-row denominator reads (`crates/oxpinyin-capi/src/predict.rs`). |
| 62 | Library-index and iterator asserts: `pinyin_load_phrase_library` of an unused slot, `pinyin_iterator_get_next_phrase` without a next row, `pinyin_bigram_iterator_get_next_phrase` without a pending row | **(c)** — both halves met (#525, lane C; the bigram twin #681) | two sites that had no register row. `pinyin.cpp:457` **`assert`** (`SYSTEM_FILE == m_file_type \|\| USER_FILE == m_file_type`): the stock `table.conf` leaves library 0 (reserved) and 8–15 unused, so `pinyin_load_phrase_library(ctx, 0)` and `(ctx, 8)` abort the pin (an index of 16 or more answers `false` first, `:448`). `pinyin.cpp:709` **`assert`** (`ERROR_OK == retval`): `pinyin_iterator_get_next_phrase` on an exhausted or empty iterator (an empty user dictionary at `pinyin_begin_get_phrases(ctx, 7)`) asks for the item of `null_token`. oxpinyin answered `false` at both and said nothing; each now emits exactly one `g_warning` in domain `libpinyin` (`crates/oxpinyin-capi/src/config.rs`, `iterators.rs`). Verified on bdb with `abort-load-phrase-library-0`, `-8` and `abort-iterator-get-next-phrase` of `contract-diff.py`: the pin dies of SIGABRT, the subject answers `false` with one warning; against the parent build the three fail. The in-range neighbours (`scheme-and-library-neighbours`) answer without a warning at both sides. **Extended 2026-10-08 UTC (#681)** to the bigram twin, which had no row either: `pinyin.cpp:902` **`assert`** (`iter->m_index_token != null_token && iter->m_index_token != sentence_start`) — `pinyin_bigram_iterator_get_next_phrase` called while the iterator's predecessor is `null_token`, as on a fresh iterator over an empty user bigram (with or without a `has_next` first, which answers `false`), or `sentence_start`, where a walk whose last-loaded predecessor is `sentence_start` leaves it after its `false` row. oxpinyin already answered `false` there with `phrase`, `pinyin` and `count` untouched and exactly one `g_warning` in domain `libpinyin` (`crates/oxpinyin-capi/src/iterators.rs:434-442`). Verified on bdb with `abort-bigram-iterator-get-next-phrase` of `contract-diff.py` (the fresh iterator): the pin dies of SIGABRT, the subject answers `false` with one warning and the three out-params untouched. The call after a walk is not a case: which predecessor the walk loads last follows the backend's hash order, and the other outcome is row 75's read. libzhuyin has no export iterator (`zhuyin.h` declares only the import iterator), so there is no zhuyin twin. |
| 63 | The key accessors on an empty matrix, and the double-pinyin aux text cut three bytes in | **(c)** — both halves met (#525, lane C) | two sites that had no register row. `storage/phonetic_key_matrix.h:103` **`assert`** (`index < m_table_content->len`): `pinyin_get_pinyin_key` and `pinyin_get_pinyin_key_rest` test `offset >= matrix.size() - 1` first, which wraps on a matrix of size 0 (a fresh instance, or one whose parse placed no key), so the test passes and `get_column_size` asserts; on a matrix with keys they answer `false` as before and stay silent. `pinyin.cpp:3488` **`abort()`** (`default:` of the `switch(len)` in `pinyin_get_double_pinyin_auxiliary_text`): a cursor three or more bytes into a key the shared matrix holds, e.g. `zhong` cut at 3 (the matrix is shared by every parser, row 55); the `assert` at `:3311` fires on every cursor of all three functions after a parse with a leading `'` (`'nihao`, `''ni`: column 0 stays empty), and each now warns once (`abort-*-auxiliary-text-leading-apostrophe`). oxpinyin answered `false` at all of them and said nothing; each now emits exactly one `g_warning` in domain `libpinyin` (`crates/oxpinyin-capi/src/cursor.rs`, `text.rs`). Verified on bdb with `abort-get-pinyin-key-empty-matrix`, `abort-get-pinyin-key-rest-empty-matrix` and `abort-double-auxiliary-text-cut-three-bytes-in` of `contract-diff.py`. |
| 64 | Candidate-type asserts: `pinyin_choose_candidate` on a predicted row, `pinyin_choose_predicted_candidate` on any other row, `pinyin_get_candidate_nbest_index` on a non-sentence row, `pinyin_remove_user_candidate` on a non-`NORMAL` or non-user row | **(c)** — both halves met (#525, lane C) | sites that had no register row, all **`assert`**: `pinyin.cpp:2507` (`PREDICTED_BIGRAM_CANDIDATE != type && PREDICTED_PREFIX_CANDIDATE != type`), `:2593` (the three predicted types), `:2883` (`NBEST_MATCH_CANDIDATE == type`), `:3734` (`NORMAL_CANDIDATE == type`) and `:3738` (`USER_DICTIONARY == index`). oxpinyin took a candidate of the wrong type without a check: `pinyin_choose_candidate` selected a predicted row, `pinyin_choose_predicted_candidate` trained a normal row, `pinyin_get_candidate_nbest_index` answered `true` and wrote `0`. Each now answers `false` (`0` for `pinyin_choose_candidate`, as row 45) and emits exactly one `g_warning` in domain `libpinyin` (`crates/oxpinyin-capi/src/candidates.rs`); `pinyin_get_candidate_nbest_index` no longer writes its out-param, as the pin dies before it does. Verified on bdb with the `abort-choose-candidate-predicted-prefix-row`, `abort-choose-predicted-candidate-normal-row`, `-sentence-row`, `abort-get-candidate-nbest-index-normal-row` and `abort-remove-user-candidate-*` cases of `contract-diff.py`. The `PREDICTED_BIGRAM_CANDIDATE` arm of `:2507` shares the assertion but is not executed: no bigram row could be produced at the pin (the harness's training sequences left every prediction without one), so the arm is held by the prefix row alone. `training_through_the_abi_records_the_pinned_counts` called `pinyin_choose_predicted_candidate` on a `NORMAL_CANDIDATE`, which the pin aborts on; it now takes a predicted row. |
| 65 | `pinyin_guess_candidates` at an offset with no span: the pin still lists the `LONGER_CANDIDATE` row | **REVERT TARGET** (#542, lane C) | found while probing the offsets row 66 guards, not fixed. On `nihao` with no earlier lookup, `pinyin_guess_candidates(inst, offset, 0)` at offsets 3, 5 and 6 answers `true` with one candidate at the pin — the `LONGER_CANDIDATE` `你好吗` (type 7) — and `true` with none at oxpinyin. Measured with the sort word 0 on `nihao` only; the cause is not traced. Offset 0 was not compared. The harness control `guess-candidates-past-the-reserved-slot` therefore compares the returns only. |
| 66 | The zhuyin twins and the offset probes: `zhuyin_set_chewing_scheme`, `zhuyin_set_full_pinyin_scheme`, `zhuyin_load_phrase_library`, `zhuyin_unload_phrase_library`, the zhuyin offset, key and candidate lookups past the matrix, and their pinyin siblings | **(c)** — both halves met (#525, lane C) | sites that had no register row except where noted; all **`assert`** unless marked. `zhuyin_parser2.cpp:295` (**`abort()`**, dvorak, row 5c) and `zhuyin.cpp:736` (**`abort()`**, out-of-enum, row 5d) through `zhuyin_set_chewing_scheme`; `pinyin_parser2.cpp:398` (**`abort()`**, row 5d) through `zhuyin_set_full_pinyin_scheme(0 or 4)`; `zhuyin.cpp:372` (library 0 and 8–15 unused) and `:381` (index 16 or more); `zhuyin.cpp:1453` (`_check_offset`'s `assert(zero_key != key)`, which also fires one past the reserved slot — the slot holds a lone zero key) through `zhuyin_guess_candidates_after_cursor` and `_before_cursor` at `parsed_len + 1` and beyond, and (2026-10-09 UTC, #707, [separator-matrix.md](separator-matrix.md)) also at an offset inside the parse where a leading separator left a zero-key column (`'ni'hao`, `'ni'`): `zhuyin_guess_candidates_after_cursor` and `_before_cursor`, `zhuyin_get_zhuyin_key` and `_key_rest`, and `zhuyin_get_left_zhuyin_offset` (the seven data-returning calls), and `zhuyin_get_character_offset` at the input length of `'ni'`; `zhuyin.cpp:2158` through `zhuyin_get_character_offset`; `zhuyin_get_left_zhuyin_offset` and `zhuyin_get_right_zhuyin_offset` past the matrix, the latter also on an empty one; `zhuyin_get_zhuyin_key` and `_key_rest` on an empty matrix (`phonetic_key_matrix.h:103`); `zhuyin.cpp:1261` (`0 < total_freq`, the zhuyin twin of `pinyin.cpp:1859`) with the facade total wrapped to zero through `zhuyin_token_add_unigram_frequency`; (the zhuyin arm of row 6, `zhuyin_train`, is logged since PR 12d). The zhuyin keyboards bind no `'`, so `_check_offset`'s zero-key arm is unreachable except at the reserved slot; and `zhuyin.cpp:330`, `:440` and `:457` are file-triggered, not part of this change (`:454` is refuted). The probes also found two pinyin siblings that were still silent: `pinyin_get_right_pinyin_offset` on an empty matrix (`pinyin.cpp:3085`'s loop asserts first) and `pinyin_guess_candidates` two or more past the reserved slot; and one wrong-but-defined answer, reverted: `pinyin_guess_candidates` one past the reserved slot (`parsed_len + 1`) answers `true` at the pin (`_check_offset`'s result is discarded, `pinyin.cpp:2226`) and `false` here, now `true`. oxpinyin answered `false` at every abort and said nothing; each now emits exactly one `g_warning` — in the zhuyin facade's own domain `libzhuyin` (`crates/oxpinyin-zhuyin-capi/src/ffi.rs`, as the facade's `zhuyin_init` warning already does), in `libpinyin` for the pinyin siblings. Verified on bdb with the `abort-zhuyin-*` cases of `contract-diff.py`, which now compare the domain per facade. |
| 67 | The zhuyin twins of rows 50, 52 and 53: `zhuyin_get_sentence`, `zhuyin_get_character_offset`'s out-param and reserved slot, `zhuyin_token_get_phrase`, `zhuyin_token_get_nth_pronunciation`, the key slots, `zhuyin_get_zhuyin_key_rest_length`, `zhuyin_unload_phrase_library` | **CLOSED** in code (#542, lane C) | measured on bdb against the libzhuyin pin with the `zhuyin-*` cases of `contract-diff.py`; every one differed. (1) `zhuyin_get_sentence` answers from `m_nbest_results` alone: `false` with `*sentence` untouched when there is no row (`zhuyin.cpp:988-989`), including before any guess and after a keyless one; oxpinyin wrote NULL and answered the raw preedit with `true` after a parse. (2) `zhuyin_get_character_offset`'s failed walk stores the length it reached, 0 (`*plength = length`, `zhuyin.cpp:2192`); the earlier returns leave it alone. At the reserved slot (`offset == parsed_len`) the zhuyin walk has no zero-key skip (`zhuyin.cpp:2095-2140`; the pinyin one has, `pinyin.cpp:3140`), so it fails there where the pinyin walk succeeds; oxpinyin answered `true, 2`. (3) `zhuyin_token_get_phrase` on an unknown token leaves both out-params untouched; oxpinyin wrote NULL. (4) `zhuyin_token_get_nth_pronunciation` past the last reading answers `true` with `phrase_length` keys, zeroed here (class (b) for the content, row 70, the twin of row 51). (5) The key and key-rest slots are `static`s of libzhuyin (`zhuyin.cpp:1898`, `:1921`): one for every instance, valid after the instance is freed; oxpinyin kept one per instance. (6) `zhuyin_get_zhuyin_key_rest_length` is `m_raw_end - m_raw_begin` in `guint16`, wrapping; oxpinyin saturated at 0. (7) `zhuyin_unload_phrase_library` refuses only `TSI_DICTIONARY` (1), unloads any other library and answers `true` (`zhuyin.cpp:378-388`), where `pinyin_unload_phrase_library` refuses everything but GBK; library 3 holds phrases in the oracle data (unloading it takes `su3cl3`'s list from 126 to 124 candidates). `RuntimeDict::unload_library` and `load_library` widen from GBK only to any of the sixteen sub-indices, signatures unchanged, and the pinyin ABI keeps its GBK-only gate in `pinyin_unload_phrase_library`; `zhuyin-unload-phrase-library-effect` holds the answers and the candidate counts through an unload and a reload. Against the parent build the nine cases fail. Review (2026-10-06 UTC): populated USER_FILE index 7 is hidden from phrase and pronunciation reads after a zhuyin unload, and visible after reload; the pinyin facade refuses that unload and keeps the token visible. `user-library-token-unload-zhuyin` and the pinyin control hold both measured contracts. |
| 68 | Diagnostics on stderr: a fresh user dir, a failed init, a failed save, and a failed fini | **CLOSED** in code (#545, lane C) | the pin writes raw `fprintf(stderr, …)` lines on its failure paths, and oxpinyin printed a note of its own on the first and nothing on the others. Measured on bdb, both facades (`stderr-*` cases of `contract-diff.py`, which now compare the library's stderr with the scratch directory names normalised). (1) A user dir whose `user.conf` cannot be opened: `open <dir>/user.conf failed.` with **no newline** (`table_info.cpp:332`); oxpinyin printed `oxpinyin: non-conforming user profile wiped (user dir …)`, also when `user.conf` existed and merely did not conform, where the pin is silent. (2) An init whose `table.conf` cannot be opened: `open <sys>/table.conf failed.` and `load <sys>/table.conf failed!` (`table_info.cpp:201`, `pinyin.cpp:338`, `zhuyin.cpp:282`), the file named as `g_build_filename` spells it; oxpinyin logged its own GLib `libpinyin` warning (`pinyin_init: missing file: …`) and no raw line. The descriptive warning stays for every other failure, which the pin reports with lines this change did not measure. (3) A save with the user dir removed: ten `rename <tmp> to <final> failed.` lines — the seven libraries by index, then `user_pinyin_index.bin`, `user_phrase_index.bin`, `user_bigram.db` (`pinyin.cpp:1061`…`:1123`) — then `write <dir>/user.conf failed.` (`table_info.cpp:382`), and the save **answers `true`** (`_write_files(context) && _rename_files(context)`, `pinyin.cpp:1140`); oxpinyin answered `false`, printed nothing of the pin's, and bdb leaked a libdb line (`BDB1565 DB->sync: method not permitted before handle's open method`) from the `Drop` of a handle whose `open` had failed. (4) the paths in all of these are written as their bytes (`stderr-non-utf8-user-dir`); (5) `pinyin_fini` with the user dir removed: `write <dir>/user.conf failed.`; libzhuyin's fini writes nothing. `UserStore::save_reporting` and `SaveReport { renames_failed, user_conf_write_failed }` carry the failures (`oxpinyin-user`, approved 2026-10-04); `ContextCore::save_user` prints them and answers `true`; `oxpinyin-store`'s bdb `Db` no longer syncs a handle that never opened. One failing file does not stop the save: the others are written and renamed, as the pin does (row 69). A save that works, an unmodified one, and a reopen of the profile it wrote print nothing at either side. **Not fixed, recorded:** `errno` after `pinyin_init` on a fresh dir is 2 (`ENOENT`, left by the failed `fopen`) at the pin on all three cells and 0 here on bdb and tkrzw, 2 on kc; the other raw writers the pin has (`mmap %s failed!` / `open %s failed!`, `pinyin.cpp:256`…`:1268`, `zhuyin.cpp:200`…`:973`, `facade_chewing_table.h:115`, `facade_phrase_table2.h:96`) were not measured. |
| 69 | A save whose write fails for one file: the pin updates the rest | **CLOSED** in code (#545, lane C) | measured on bdb, both facades: with `user_pinyin_index.bin.tmp` a non-empty directory (or `user_phrase_index.bin` itself one) a second, modified `pinyin_save` / `zhuyin_save` at the pin ignores the failed write (`_write_files`, `pinyin.cpp:940-1020`), renames every other file, prints one line — `rename <dir>/user_pinyin_index.bin.tmp to <dir>/user_pinyin_index.bin failed.` (`:1096`; `user_phrase_index.bin` for the blocked final, `:1110`) — rewrites `user.conf` and answers `true`. In the temporary-blocked fixture, the first `user_pinyin_index.bin` stays at its active final path while the other files take the second save. In the final-blocked fixture, the harness first moves `user_phrase_index.bin` to `user_phrase_index.bin.moved`: that backup keeps the first save, the active final path is the blocking directory, and the other files take the second save. These are two distinct **mixed profiles**. oxpinyin first removed what it had staged and reported all ten renames, leaving the previous profile whole (ten lines against one; ruled a REVERT TARGET 2026-10-04 and reverted the same day). `save_with_bigram_reporting` (`crates/oxpinyin-user/src/persistence.rs`) now writes each file on its own, tolerating an I/O or store failure of that file, then renames every file of the set in the pin's order and reports the failures; only an encoding failure stops it. `save()` keeps its all-or-nothing `finish_save`. Held by `stderr-save-one-tmp-blocked`, `stderr-save-one-final-blocked` and `stderr-save-one-tmp-blocked-zhuyin` of `contract-diff.py` (stderr text, return value and which files changed, all MATCH), with `stderr-save-dir-removed` still the all-fail case. |
| 70 | `zhuyin_token_get_nth_pronunciation` keys past the last reading | **(b)** | the zhuyin twin of row 51, ruled with it: an `nth` past the last reading appends `phrase_length` keys read from `ChewingKey buffer[MAX_PHRASE_LENGTH]`, a stack array that is never initialised, because the return value of `PhraseItem::get_nth_pronunciation` is ignored (`zhuyin.cpp:1800-1810`; the out-of-range read writes nothing). No safe construction reproduces a read of uninitialised stack. The answer (`true`) and the number of keys (`phrase_length`) are the pin's and are held by `zhuyin-nth-pronunciation-range` of `contract-diff.py`, which leaves the content out of the comparison; the content is zeroed (`crates/oxpinyin-zhuyin-capi/src/dict.rs`). Entry in `docs/findings/upstream-divergences.md`. |
| 75 | `pinyin_bigram_iterator_get_next_phrase` after a walk that ends on a real predecessor | **(b)** | the other outcome of row 62's bigram call. `get_next` answers `has_next` after taking a row (`pinyin.cpp:910`), and `has_next` empties `m_pinyins` before it looks for the next row (`:798-801`) and leaves `m_index_token` at the predecessor it loaded last (`:885-886`). When that is a real phrase token the `:902` assert passes, so a `get_next` called after the walk's `false` row, with no `has_next` in between, indexes the empty array at `:905`: on GLib 2.90 the array has no storage, and the pin dies of SIGSEGV at fault address 0. Measured on bdb (amd64 under Rosetta; native amd64 not run) after training 你→好 and 世→界: one row (`你好`, `get=false`), then the call faults at `pinyin_bigram_iterator_get_next_phrase+0x38`, offset `0x97748` of the prefix's `libpinyin.so.15.0.0`, which `addr2line` maps to `pinyin.cpp:905`; the pin built with row 1's `bigram-export-strjoinv` patch faults at the same statement (its `:907`). Undefined behaviour, not an assert: no safe construction reproduces the read. oxpinyin already answered `false` with the three out-params untouched and no warning (`crates/oxpinyin-capi/src/iterators.rs:443-444`, `crates/oxpinyin-facade/src/export_rows.rs:369-371`). No `contract-diff.py` case: the predecessors come in the user bigram's hash order (`pinyin.cpp:779`; `storage/ngram_bdb.cpp:178-200`, `DB_HASH`), so whether a given training reaches this read or row 62's assert depends on the backend. Entry in `docs/findings/upstream-divergences.md`, with the commands. |

| 71 | Empty system directory and NULL system directory (#643) | **CLOSED** in code (2026-10-08 UTC) | At 074a2219, `pinyin.cpp:331-338` and `zhuyin.cpp:275-282` keep the argument then call `g_build_filename`: empty names cwd/table.conf, NULL names the empty filename and fails with two raw stderr lines. The facade rejected both. Both C boundaries now preserve NULL, including its exact failed-open stderr, and the facade forwards an empty Path unchanged. `contract-diff.py` `init-system-{empty,empty-missing,null,dot,absolute}-{pinyin,zhuyin}` compares init, parse, guess, user files and stderr on BDB. Source read from `/home/sheng/work/libpinyin` at the pin (pinyin.cpp blob f27f7cf7, zhuyin.cpp d1520c18); no exception class applies. |
| 72 | Remembered phrase pronunciation tones (#699) | **CLOSED** in code (2026-10-08 UTC) | `pinyin.cpp:3578-3668` at 074a2219 retains ChewingKey tones; the C remember path projected to syllable IDs. It now combines the existing selected syllables with their parsed tones. Ordinary-userdir import/remember/export is covered by `remember-tones-pinyin`; Zhuyin has no equivalent API. Plain match, no interface change. |
| 73 | NULL user directory retains in-memory user state (#642) | **CLOSED** in code (2026-10-08 UTC) | Pin init creates user tables even with NULL userdir; imports and lookup work, but train/save refuse. Runtime formerly omitted the UserStore. A transient store now retains system originals and mutable indexes without a filesystem target. Existing contract driver covers both facades, each library 1–7 in a fresh context, exact stderr and cwd/TMPDIR snapshots through fini. Plain match. |
| 74 | Import with five existing same-text tokens (#525) | **(c)**, both halves met (2026-10-08 UTC) | `phrase_large_table3.h:95` at 074a2219 asserts `0 <= num && num <= 4` during the sixth iterator add after additions to libraries 1–5. Both facades refuse before mutation with exactly one domain warning. The fifth add remains successful. Existing `combined-library-sixth-add-*` cases prove SIGABRT versus false and unchanged state; `combined-library-fifth-add-*` controls match. |

Totals at `e1d915d0` with this change (2026-09-27 UTC, recounted from
the class column; #548): **(a)** 1 (row 11, scope shrinking, #535) · **(b)** 8 (1, 18,
41, 42, 43, 51, 60, 70) · **(c)** 18 (38, 44, 45, 61, 5a, 62, 10, 14, 63, 64, 6, 4, 5c, 5d, 19, 21, 66 and 74, both halves met) · **(d)** 0 (retired 2026-09-06) ·
**REVERT TARGET** 3 ·
**OPEN DEFECT** 0 · **CLOSED** 40 (3, 5b, 7, 8, 9, 12, 13, 15, 16, 17,
24, 25, 26, 27, 28, 30, 32, 33, 34, 35, 36, 37, 39, 46, 47, 48, 50, 52, 53, 55, 56, 57, 59, 49, 67, 68, 69, 71, 72, 73) · **CONTRADICTED** 0 · **STANDING** 1 (40) · **reattributed** 1 (20 → 33) ·
**no ABI divergence** 4 (2, 23, 29, 31) · **WITHDRAWN** 1 (22) — 77 rows (amended the same
day for #550: row 20 reattributed, row 38's index arm opened; for
#577: rows 25 and 26 contradicted; and for #591: row 44 registered; amended
2026-10-02 UTC at `a3ef00f5`: rows 36 and 39 closed in code by lane B, #607/#608,
and #618, so REVERT TARGET 7 → 5 and CLOSED 15 → 17; amended 2026-10-03 UTC: row 22's crash claim withdrawn, not reproduced, so (c) 11 → 10; amended 2026-10-03 UTC at `c6a61be8`, lanes I and H merged: rows 32, 33, 34 and 37 closed in code and rows 25 and 26 re-closed after #577, so REVERT TARGET 5 → 1, CLOSED 17 → 23, CONTRADICTED 2 → 0; row 45 registered, #598's class-(c) row, so (c) 10 → 11 and 47 → 48 rows; amended 2026-10-04 UTC for #524: row 38’s valid-index and empty-result defects closed, bounds registered as (c), so REVERT TARGET 1 → 0 and (c) 11 → 12, CLOSED stays 23; amended 2026-10-04 UTC for #619: row 46 registered closed in code with its fix, so CLOSED 23 → 24 and 48 → 49 rows; amended 2026-10-04 UTC, lane C: row 47 registered and closed in code (#532), so CLOSED 24 → 25 and 49 → 50 rows; amended 2026-10-04 UTC, lane C: row 48 registered and closed in code (#587) and row 49 registered as a REVERT TARGET (#587), so CLOSED 25 → 26, REVERT TARGET 0 → 1 and 50 → 52 rows; amended 2026-10-04 UTC, lane C: row 50 registered and closed in code (#542) and row 51 registered as class (b) (#542), so CLOSED 26 → 27, (b) 5 → 6 and 52 → 54 rows; amended 2026-10-04 UTC, lane C: row 52 registered and closed in code (#542), so CLOSED 27 → 28 and 54 → 55 rows; amended 2026-10-04 UTC, lane C: row 53 registered and closed in code (#542) and row 54 registered as a REVERT TARGET (#542), so CLOSED 28 → 29, REVERT TARGET 1 → 2 and 55 → 57 rows; amended 2026-10-04 UTC, lane C: row 55 registered and closed in code (#542), so CLOSED 29 → 30 and 57 → 58 rows; amended 2026-10-04 UTC, lane C: row 56 registered and closed in code (#540), so CLOSED 30 → 31 and 58 → 59 rows; amended 2026-10-04 UTC, lane C: row 57 registered and closed in code (#542, #540) and row 58 registered as a REVERT TARGET (#540), so CLOSED 31 → 32, REVERT TARGET 2 → 3 and 59 → 61 rows; amended 2026-10-04 UTC, lane C: row 59 registered and closed in code (#540), so CLOSED 32 → 33 and 61 → 62 rows; amended 2026-10-04 UTC, lane C: row 60 registered as class (b) (#528), so (b) 6 → 7 and 62 → 63 rows; amended 2026-10-04 UTC, lane C: row 61 registered as class (c) with both halves met (#540), so (c) 12 → 13 and 63 → 64 rows; amended 2026-10-04 UTC, lane C: row 62 registered as class (c) with both halves met (#525) and row 5a promoted to both halves met, so (c) 13 → 14 and 64 → 65 rows; lane C, PR 12b: rows 10, 14, 19 and 21 are both halves met (`pinyin_get_sentence` past the rows, the cursor-offset and character-offset asserts, the apostrophe in `pinyin_parse_full_pinyin`), and row 63 is registered; lane C, PR 12c: row 64 is registered (the candidate-type asserts); lane C, PR 12e: row 4 is both halves met (a toned initial-only key refused at the lookups and at `pinyin_get_pinyin_is_incomplete`); lane C, PR 12f: row 65 is registered (the pin lists the longer row at offsets that start no span; REVERT TARGET), rows 5c, 5d, 19 and 21 are both halves met, and row 66 is registered (the zhuyin twins); lane C, PR 12f: row 65 is registered (the pin lists the longer row at offsets that start no span; REVERT TARGET); lane C, PR 12f: rows 5c, 5d and 6 are both halves met, and row 66 is registered (the zhuyin twins); lane C, PR 13: row 49 is closed in code (#587), so REVERT TARGET 4 → 3 and CLOSED 33 → 34; lane C, PR 14: row 67 is registered and closed in code (the zhuyin twins of rows 50, 52 and 53); row 60 amended for the zhuyin liveness twin; lane C, PR 14: row 70 is registered as class (b) (the zhuyin twin of row 51, uninitialised keys past the last reading); lane C, PR 15: row 68 is registered and closed in code (#545, the pin's stderr on a fresh dir, a failed init, a failed save and a failed fini); lane C, PR 15: row 69 is registered and closed in code (a save whose write fails for one file leaves the pin's mixed profile, and now oxpinyin's); registered as a REVERT TARGET on 2026-10-04 and reverted the same day, so REVERT TARGET and CLOSED are as if it had been closed on registration; amended 2026-10-08 UTC for #681: row 62 extended to the bigram iterator's `get_next` assert, still (c), and row 74 registered as class (b), the NULL read the same call makes after a walk that ends on a real predecessor, so (b) 8 → 9 and 73 → 74 rows). The line this replaces (below) no longer matched
its own table: it said REVERT TARGET 5 and CLOSED 18 while the class
column counted 6 and 17, because row 17's cell still read REVERT
TARGET after `8ec75085` closed it, and row 35, closed in code by
`7c9a6923` (an ancestor of the audited `18d78208`), was still counted
open.

*Superseded totals line, kept as history:* Totals at `2a99761a` (2026-09-06, oracle pin 074a2219), amended
2026-09-16 for rows 5b, 17 and 30 — each closed in code, each with its
live differential run taken 2026-09-16 in a `debian:testing` container
(the oracle environment `docs/testing/oracle-environment.md` records);
amended 2026-09-18 for row 32; amended 2026-09-19 for rows 33–34
and again for rows 35–37; amended 2026-09-20 for row 32 (closed in
code by the §9 port, PR #496) and row 38 (registered closed — the §9
train-gate law):
**(a)** 2 · **(b)** 2 · **(c)** 10 · **(d)** 0 (class retired, see
below) · **REVERT TARGET** 5 (rows 33, 34, 35, 36, 37) ·
**OPEN DEFECT** 0 · **CLOSED** 18
(rows 3, 5b, 7, 8, 9, 12, 13, 15, 16, 17, 24, 25, 26, 27, 28, 30, 32,
38 — 26
measured identical on the pin 2026-09-08; 28 in code via #374, not
observable through today's surface; 30 in code 2026-09-14, live
FORCE_TONE profile run 2026-09-16 — `run-scheme-diff.sh bopomofo` 1–6,
8, 9 all IDENTICAL; 5b in code 2026-09-15, contract test pinned,
`run-scheme-diff.sh` oracle differential with out-of-enum values run
2026-09-16 — IDENTICAL; 17 in code 2026-09-16, `run-option-sweep.sh`
with the all-off (`0x0`) and divided-contrast (`0x8`/`0x88`) cases run
2026-09-16 — 24/24 PASS; 32 in code 2026-09-20 with the §9 port —
option sweep 24/24 at `0x1e`/`0x1c`/`0x14` and the ABI probe's
longer-choose phase IDENTICAL, host measurement per the row above; 38
in code 2026-09-20, landed with §9, its pinyin-side evidence the same
longer-choose phase, its zhuyin differential owed per the row above) ·
**no ABI divergence** 4 rows (2, 23,
29, 31).

The 2026-08-28 totals were (a) 1 · (b) 2 · (c) 6 · (d) 1 · REVERT
TARGET 7 · closed or not a divergence 2. Of the seven revert targets,
five closed by reproduction or proven equivalence (7, 8, 9, 13, 15) and
one was superseded by P6 (12); 5b and 17 closed in code (2026-09-15,
2026-09-16), their live differential runs taken 2026-09-16.

### What is still owed, in order

1. **Row 30** — closed in code 2026-09-14 (seam forward + capi tests +
   the `chewing-diff.c` FORCE_TONE profile); the profile's live run was
   taken 2026-09-16 — `run-scheme-diff.sh bopomofo` 1–6, 8, 9 all
   IDENTICAL, non-vacuity shown by the seam-only revert exiting 2
   (register amendment, `upstream-divergences.md`). Nothing is owed.
2. **Row 5b** — the double out-of-enum half-mutation, reproduced: the
   CAPI returns `true` and clears the fallback, matching the pin;
   shengmu/yunmu tables stay intact (closed in code 2026-09-15). The
   `run-scheme-diff.sh` oracle differential with out-of-enum values
   (99, −1 under ZRM) ran 2026-09-16 — IDENTICAL, the twelve `halfmut`
   probe rows byte-equal on both sides. Nothing is owed.
3. **Row 17** — the pin's `0x0` gating is ported (closed in code
   2026-09-16); `run-option-sweep.sh` with the all-off (`0x0`) and
   divided-contrast (`0x8`/`0x88`) cases ran 2026-09-16 — 24/24 PASS,
   and per-word driver logs at `0x0`/`0x2`/`0x8`/`0x82`/`0x88`/`0x188`/
   `0x18a` are byte-identical on both sides, the `n=` inventory lines
   included (`xian` 337 tables-off / 756 tables-on). Nothing is owed.
4. **Rows 26 and 28** — landed in code (#374) under the maintainer's
   2026-09-06 approval, copying libpinyin's source; row 26's
   three-input oracle battery ran on 2026-09-08 and is byte-identical.
   Nothing is owed on either.
5. **Row 32** — the sort-option input of `pinyin_guess_candidates`
   (bits 0x2/0x4/0x8/0x10; row text above). **Landed 2026-09-20** with
   the §9 port (PR #496): all three pre-registered probes flipped as
   specified (sweep STOP → 24/24 at 0x1c and 0x14; ABI probe down to
   the 0x1e residues at both; the choose-a-LONGER-row flow IDENTICAL).
   **Reopened 2026-09-27 UTC (#582):** those probes never set bit
   `0x1`; at `0x1d`/`0x1f` the NORMAL rows that repeat an n-best string
   are lost (row text above). Owed at the time: the fix (#582), and
   a sweep that includes `0x1d` and `0x1f`. **Landed 2026-10-03:** #596 (merge commit `45cc6032`) and #598 (`efa8337a`); `candidate-assembly-diff` covers `0x1` and `0x1f` and is identical there on all three cells (row 32).
6. **Row 33** — whole-row NBEST choose + train (probe residue B). Drop
   the history fallback whenever no OneStep cell is present so
   `Session::train` observes nothing, matching `train_result3`. Probe:
   phase B of `residue-mechanism-diff` must print empty bigram rows on
   both sides after `train(0)` and `train(0)again`. Registered
   2026-09-19; executes before row 34 (corrupts stored state).
7. **Row 34** — imported user phrase after `guess_sentence` (probe
   residue C). Keep nbest across parse; when `SORT_WITHOUT_SENTENCE` is
   set, rebuild the phrase window without the prior NBEST-wins dedup (or
   rebuild from scratch every `guess_candidates`). Probe: import 你好世界
   → parse → `guess_sentence` → `guess_candidates(0, 0x1f)` → assert
   NORMAL + `is_user` on both; then re-parse → `guess_candidates(0, 0x1e)`
   → assert nbest rows still present on both. Registered 2026-09-19.
8. **Row 38** — the `pinyin_train`/`zhuyin_train` gate (row text
   above). The code landed with §9 (PR #496); what is owed is the
   zhuyin-side differential: `zhuyin-diff.c` drives no train call, so
   the widened gate's zhuyin behaviour is unmeasured against the pin.
   The differential must exercise, on both sides: guess-sentence-then-
   train with no choose (the widened arm — the pin answers `true` and
   writes nothing), choose-then-train (unchanged by the gate), and the
   no-guess fresh-instance train (still `false` on both). Registered
   2026-09-20.

### Notes on the three entries whose class was not obvious

**#7 — why `validate_constraint` is not class (a), and how it closed
anyway.** The drop test is
`compute_pronunciation_possibility(...) < FLT_EPSILON`
(`phonetic_lookup.cpp:161-164`). Reading the function first-hand
(`phonetic_key_matrix.cpp:534-600`), it is a recursive **sum over every
path** of `PhraseItem::get_pronunciation_possibility` — `gfloat`
addition and a frequency ratio, no transcendental anywhere. That is the
`amplified_frequency` standard, which is ported to 100%, so the
threshold is bit-reproducible and the entry does not qualify under (a).

Reverting it would be real work rather than a flag flip: oxpinyin's
`span_finds_token` follows the §3 step-cost model (first path per
token), while upstream sums **all** paths. It closed without the port
(4c2fe02b): the threshold can only bite when a token's pronunciation
total exceeds 2²³, and the largest total in model20 is 2,945,481, so on
the pinned data the drop boundary is unreachable and the two tests are
observably equivalent. The register entry carries the scan. A different
model with a total above 2²³ would reopen it.

**#16 — why `FORCE_TONE` is class (d), and why that is not a criticism
of the port.** The double-pinyin parser has a genuinely different
`FORCE_TONE` law (a length-3 gate not nested under `USE_TONE`,
`pinyin_parser2.cpp:412,448`), and oxpinyin implements only the
full-pinyin shape. Measured: `FORCE_TONE` appears **zero times** in
ibus-libpinyin 1.16.5's `src/` and **zero times** in fcitx-libpinyin's
`src/`. Neither reference consumer can set the bit, so the differing
law is unreachable through the drop-in surface.

Class (d) here meant *correctly out of scope*, not *wrong*. The pin's
`USE_TONE` branch was ported in #178 and the port is correct and
internally consistent; the full-pinyin seam matches the pin, and the
double/zhuyin shapes — unported when this note was written — sat
outside the consumer boundary rather than being an oversight. Since
then the zhuyin batch seam (1671954, row 24) and the double-pinyin
batch seam (#289) were ported anyway, and with (d) retired
(2026-09-06) the last one, the pinyin facade's chewing batch seam, is
row 30's open defect. The note is kept as the record of the original
reasoning; row 16 carries the current status. (Row 30 closed in code
2026-09-14 — see the table.)

**#17 — recorded as a revert target, with the evidence against it
stated.** Both consumers OR the bits unconditionally before every
`pinyin_set_options` call — ibus-libpinyin at `PYLibPinyin.cc:195-196`
and fcitx-libpinyin at `eim.cpp:941` — so **neither can produce the
literal `0x0` option word**, which is the same evidence shape that
puts #16 in class (d). It is classified REVERT TARGET here because the
maintainer's decision names it as one explicitly, and because (d) as
written is scoped to *symbols* rather than to unreachable inputs
generally.

The decision that was open here — whether (d) covers
consumer-unreachable inputs or only uncalled symbols — was overtaken on
2026-09-06 by the retirement of (d) itself (see the banner above): row
17 is a plain revert target and the pin's `0x0` gating gets ported.

## What is not an exception

Stated so the classes are not read as broader than they are:

- **"The Rust is cleaner."** Not a class. Internal structure is free to
  diverge (source policy); externally observable behaviour is not.
- **"Upstream's behaviour is meaningless."** Not a class. #12's
  predicted-candidate order was defended that way; it is a revert
  target.
- **"No frontend does that."** Not a class. It was the basis of (d)
  while the consumer-union contract stood; with (d) retired it is at
  most a reason to schedule a port later, never to skip it.
- **"A float is involved."** Not class (a) unless a transcendental is
  in the accumulation.
- **"Upstream returns something useless."** Not class (c) unless
  upstream *aborts*. A wrong-but-defined answer is reproducible.

## Consequences for the register

`upstream-divergences.md` keeps its stated purpose — the residue of
what a Rust mechanism prevents — but the three live classes are narrower than
what the register accumulated. Class (c) entries in particular are not
language-mechanism residue at all; they are product decisions, and the
register should say so where it currently implies Rust forced them.

The work order (`revert-plan.md`) flips each REVERT TARGET's
differential probe from "recorded divergence" to "must be IDENTICAL" as
it lands. Entry #12's extra step — establishing Kyoto Cabinet's physical
hash walk experimentally — was overtaken by P6: reading the pin's own
DBM reproduces the pin's own walk, and `pred-order-diff` is IDENTICAL on
KC without any order having been modelled.

## Amendment — row 32 registered (2026-09-18 UTC)

The sort-option input of `pinyin_guess_candidates` is registered as row
32, class **REVERT TARGET** (no exception class fits — a defect to
close, the row-30 shape): oxpinyin honours only bit 0x1; bits
0x2/0x4/0x8/0x10 are ignored and no longer-candidate row is ever
produced, while default-settings ibus users see longer candidates on
the pin. The row carries the full measurement identity (the 2026-09-17
`debian:testing` runs, container `a15849ef7dd2`, image
`docker.io/library/debian@sha256:5056ab8a…5d73`). The totals move
REVERT TARGET 0 → 1; CLOSED stays 16. The 0x1f user-row shape is a
separate residue (`docs/findings/probe-coverage-abi.md`), not part of
the row.

## Amendment — rows 33–34 registered; residue A does not fold into row 11 (2026-09-19 UTC)

Probe residues B and C from `docs/findings/probe-coverage-abi.md` are
registered as rows 33 and 34, class **REVERT TARGET** each (no
exception class fits):

- **Row 33 (B)** — whole-row NBEST choose installs no OneStep; pin
  `train_result3` writes nothing; oxpinyin's history fallback trains
  `sentence_start → phrase` (exported counts 138, 414, …). Corrupts
  stored user state and compounds with use.
- **Row 34 (C)** — nbest cleared on every parse and NBEST-wins dedup
  applied before the `SORT_WITHOUT_SENTENCE` filter; sequence-dependent
  loss of an imported user NORMAL at `0x1f`.

Residue **A** does **not** fold into row 11 (basis updated): the
settling `nihaoshijie` dump is a 5.185 nat gap, structural rather than
the 1.0 nat `gfloat` band; classification pending, no new row in this
amendment. (Characterised later the same day: the gap is exactly one
absent tail — the pin's rank-0 single-token user-phrase path — and the
common-root experiment finds no shared cause with rows 33/34; ruled
REVERT TARGET and registered as row 35 in the amendment below.)

## Amendment — row 35 registered (2026-09-19 UTC)

Residue A of `docs/findings/probe-coverage-abi.md` is registered as
row 35, class **REVERT TARGET** (maintainer ruling 2026-09-19; no
exception class fits). The consequence at its true width: every
user-library token is refused an n-best step cost, so no imported or
learned phrase can enter a sentence path — the `nihaoshijie` tail is
one instance. The row carries the mechanism, the `lm/mod.rs` site, the
measurement identity, the user-visible consequence, and points at the
fix shape and the pre-registered differential in the probe record.
The common-root experiment refuting a shared cause with rows 33/34
stands as recorded; §10's gate is amended accordingly. Totals move
REVERT TARGET 3 → 4; CLOSED stays 16. Work order: `revert-plan.md`
§12, executing first (12 → 10 → 11 → 9).

## Amendment — row 36 registered (2026-09-19 UTC)

The bigram export iterator's last-row return value — side observation
(i) of the residue-A record, `probe-coverage-abi.md` F — is registered
as row 36, class **REVERT TARGET** (maintainer instruction 2026-09-19:
a plain ABI return-value divergence; no exception class fits). Scope:
`pinyin_bigram_iterator_get_next_phrase` only; the unigram export
iterator agrees on both sides. Totals move REVERT TARGET 4 → 5; CLOSED
stays 16. Work order: `revert-plan.md` §13, independent of the
sequence. Residue E (the candidate window behind the composition
offset, `probe-coverage-abi.md` E) is registered as row 37 in the
amendment below.

## Amendment — row 37 registered (2026-09-19 UTC)

Residue E of `docs/findings/probe-coverage-abi.md` is registered as
row 37, class **REVERT TARGET** (maintainer ruling 2026-09-19; no
exception class fits). The consequence at its worst measured point: at
`(0, 0x1f)` after a whole-composition choose the pin answers 127
candidates and oxpinyin answers 0 — an empty list — and at the
ordinary partial choose oxpinyin answers the offset-5 list where the
pin answers the offset-0 one. Consumer routes: ibus-libpinyin under
preset 2 forces `lookup_cursor = 0` and calls `guess_candidates(0,
0x1f)` after every partial choose (`PYPPhoneticEditor.cc:352-355`);
cursor-left reaches it under every preset. Totals move REVERT TARGET
5 → 6; CLOSED stays 16. Work order: `revert-plan.md` §14, executing
second (12 → 14 → 10 → 11 → 9). The `tuihui` dump stays as the band illustration (both
sides agree). Residue **D** needs no register row (same-dir
retraction stays in the probe record). Totals move REVERT TARGET 1 →
3; CLOSED stays 16.

## Amendment — rulings recorded (2026-09-27 UTC)

Five human rulings, recorded here with their sources. The recording
date is UTC captured at run time (`date -u`: 2026-09-27T04:00:22Z);
where a ruling was given earlier, its own date is stated too.

1. **Q1 — the reference build and its default.** *Given 2026-09-26
   UTC* (recorded in `bug-for-bug-audit-r2-2026-09-23.md` §9 item 6,
   landed in `2702674f`, PR #522), *restated in the lane-G brief of
   2026-09-27.* The reference build is the pin's autoconf build; the
   reference default is bare `./configure`, i.e. Berkeley DB
   (`configure.ac:94` at `074a2219`), matching the workspace's
   `default = ["bdb"]`. Parity coverage always includes the tkrzw, bdb
   and kc cells. Prose calling tkrzw or Kyoto Cabinet the default is
   retired: current-state claims are corrected, dated history is
   annotated where it could be read as current (the goal section
   above; `upstream-divergences.md`, "Native data-file naming").
   §9 of the audit report also records the provenance gap this ruling
   closes: before it, no written human decision for the Berkeley DB
   default existed.
2. **Version — the drop-in identity follows the pin.** *Given
   2026-09-26 UTC*, landed in `e1d915d0` (PR #592): the drop-in
   version is 2.11.92, derived from `libpinyin_tag` in
   `tools/oracle/oracle-pin.txt`. In this document,
   `upstream-divergences.md`, `divergence-taxonomy.md` and
   `upstream-report-drafts.md` no current-state 2.11.91 claim remains:
   every 2.11.91 left in them is oracle history (the `0c5e80e1` pin
   the E2E rule was written against, the 2026-08-22 report drafts
   verified at that pin, the taxonomy's 2026-08-09 corpus roll-up) or
   a distro package version (Debian's `2.11.91-1`), and stays as
   written.
3. **Class (c) — both halves, asserts live.** *Given in the lane-G
   brief of 2026-09-27 UTC.* A class-(c) site answers `false`/`Err`
   **and** emits exactly one `g_warning` log line; the bug-for-bug
   target is the asserts-live pin build; every class-(c) row says
   whether each site is an `assert` or an `abort()`. The class text
   above carries the rule; rows 4, 5a, 5c, 5d, 6, 10, 14, 19, 21 and
   22 are re-marked against it (#549, #525).
4. **#539 — `LC_NUMERIC`.** *Given in the lane-G brief of 2026-09-27
   UTC.* The pin leaves the process `LC_NUMERIC` at `"C"` after
   `pinyin_init`/`zhuyin_init`; oxpinyin reproduces this bug-for-bug,
   and it is recorded as an upstream defect. Row 39 registered it as a
   REVERT TARGET until the reproduction landed; it landed in #618 (merge
   commit `a3ef00f5`, code `e5400598`, 2026-10-01 UTC) and row 39 is
   CLOSED in code. `upstream-report-drafts.md` item 6 is the upstream
   draft.
5. **Evidence policy.** *Given in the lane-G brief of 2026-09-27 UTC*;
   the round-2 audit already applied it (`bug-for-bug-audit-r2-2026-09-23.md`
   §1, "Method: code and data basis"). Findings rest on code plus
   data; logs are not evidence. The E2E rule's verification clause
   above carries it.


Amended 2026-10-08 UTC, init lane: row 71 (#643) registered closed with its fix; CLOSED 37 → 38, total 73 → 74.
