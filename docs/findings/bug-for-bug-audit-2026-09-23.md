# Bug-for-bug compatibility audit — oxpinyin vs the libpinyin pin

Date (UTC, captured at run time): **2026-09-23T03:54:40Z** · Phase 1 (read-only
with respect to source; this document is the only file the branch adds) ·
Status: **audit report, no remediation performed**.

This is an independent, evidence-first audit of oxpinyin against the pinned
libpinyin oracle. It follows the mandate's axes A–L. Every behavioural claim
carries the command that produced it; in-repo docs are treated as claims to
verify, not as evidence. Where an axis could not be established to the
mandate's bar in this session it is reported **NOT-ESTABLISHED** (which is not
the same as refuted), never silently as MATCH.

A note on method ordering, stated up front for honesty: the mandate asks that
the pre-registration (§2 below) precede any measurement. In this single session
environment provisioning and measurement proceeded iteratively (the container
build had to be debugged before any differential could run), so §2 was
finalised contemporaneously with this report rather than strictly before the
first measurement. The decision rules in §2 are **not** post-hoc: they are the
repository's pre-existing criteria (`docs/findings/compatibility-policy.md`, the
four exception classes and the E2E I/O rule) plus the mandate's own verdict
vocabulary. No rule was chosen to fit an observed result.

---

## 1. Provenance block

| field | value | how verified |
| --- | --- | --- |
| subject (oxpinyin) HEAD | `656ef0b5b0359efb3f7a233a6572305dbfb076c9` | `git rev-parse HEAD`; equals `origin/main` tip (`git rev-parse origin/main`) |
| subject branch | `docs/bug-for-bug-audit-2026-09-23` (worktree `~/oxpinyin-audit-wt`) | `git branch --show-current` |
| oracle pin (libpinyin) | `2.11.92` / `074a2219c90feaf962d0d24f034514033ece5f99` | **from the tree, not the prompt**: `configure.ac` `m4_define([libpinyin_micro_version],[92])`, `[abi_current],[15]`, `[abi_revision],[0]`; `git show -s 074a2219` → date `2026-09-03 17:26:19 +0800`, subject "use g_rename to replace rename…". Matches the task's expected pin. |
| oracle pin (ibus-libpinyin) | `1.16.5` / `2d2cdac0187101aa0cd7ac06694a8340721ddfbb` | `tools/oracle/oracle-pin.txt` (not built this session — see §8 leads) |
| model20 | sha256 `59c68e89…defcb1155` | `target/model20/verified` marker; 18 files present |
| Rust toolchain | `1.97.1` | `rust-toolchain.toml` `channel = "1.97.1"`; container `rustc 1.97.1 (8bab26f4f 2026-07-14)` |
| cargo-c | `0.10.25` | `Dockerfile.audit` (`cargo install cargo-c@0.10.25 --locked`) |
| container base | `debian:testing@sha256:dab11cdb0a9dcf4bbd68f671635b35f1f726b452b92396875b69bb2c7daa42a9` (forky/sid) | `podman pull` of the digest; matches the image cited in `compatibility-policy.md` rows 35/37 |
| container apt snapshot | `snapshot.debian.org/archive/debian/20260831T000000Z testing main` | `Dockerfile.audit` (same frozen package set as `tools/bisection/Dockerfile.perf-matrix`) |
| audit image | `oxpinyin-audit:latest` id `a864f2a9ffef…`, repo digest `sha256:0d3513f7a81c035432e02dfdb9372b904da10bc975ed7a23f13fbd1e55a80204` | `podman image inspect` |
| store backend (both sides) | **tkrzw** | oracle `./configure --with-dbm=Tkrzw`; subject `--no-default-features --features tkrzw`. Chosen to match the frozen parity pin (`tools/oracle/oracle-pin.txt` `dbm=Tkrzw`) and the §12/row-20 baseline. |
| host | RHEL 10.2, podman 5.8.2, x86_64 | `uname -a`, `podman version` |
| run window (UTC) | 2026-09-23T02:00Z … 2026-09-23T03:54Z | `date -u +%FT%TZ` at each step |

**Backend provenance caveat (recorded, not a defect of this audit):** the
subject's *workspace default* backend is Berkeley DB since 2026-09-20
(`crates/oxpinyin-store/Cargo.toml` `default = ["bdb"]`, commit `ef7f2b41`),
while the frozen parity oracle pin is tkrzw. This audit therefore built **both**
sides on tkrzw (the parity pin) for the primary comparison; the default-backend
mismatch (a from-source `cargo build` is BDB, the parity oracle is tkrzw) is a
provenance fact a reader must keep in mind, and the on-disk-family consequence
is already governed by the maintainer's same-backend ruling
(`compatibility-policy.md`, "The goal this policy serves").

**Container recipe.** `Dockerfile.audit` (scratch, outside the worktree; never
committed — proposed for Phase 2 in §8) is `Dockerfile.perf-matrix` trimmed to
the tkrzw oracle/subject pair plus `abigail-tools`. Two build fixes were needed
against the trimmed recipe and are themselves informative: (i) `liblzma-dev` +
`zlib1g-dev` must be installed explicitly for the tkrzw subject link
(`perf-matrix` pulled them transitively via `libkyotocabinet-dev`, which the
trim dropped) — the raw failure was `rust-lld: error: unable to find library
-llzma`; (ii) the oracle does **not** build libzhuyin unless
`--enable-libzhuyin` is passed (`configure.ac:138-144`, default `no`).

---

## 2. Pre-registered decision rules and falsifier list

### 2.1 Decision rule per axis

Verdict vocabulary (mandate): **MATCH**, **DIVERGENT-REGISTERED**,
**DIVERGENT-UNREGISTERED**, **NOT-ESTABLISHED**.

The registered divergence classes are those of
`docs/findings/compatibility-policy.md` — **(a) MATH** (transcendental `gfloat`
accumulation), **(b) MEMORY SAFETY** (upstream is UB, safe Rust cannot express
it), **(c) AVAILABILITY** (upstream aborts/asserts on caller input; oxpinyin
returns `false`/`Err` and logs), **(d) CONSUMER SCOPE** (RETIRED 2026-09-06; may
not justify anything) — plus one separately-registered **standing divergence**:
the tkrzw **binding-ABI error-origin collapse** (`SYSTEM_ERROR`/`UNKNOWN_ERROR`),
which `docs/findings/tkrzw-langc-exception-classification.md` records as
"fits none of the previously accepted divergence buckets" and the mandate calls
"**ABI information loss**". See §6 for the reconciliation of that naming.

Per-axis rule (what counts as MATCH vs DIVERGENT):

- **A (ABI surface).** MATCH iff: version-script symbol sets are equal both
  directions; the *shipped* (relinked) `.so` exports the same versioned symbol
  set with the same SONAME and version node; `abidiff` exits 0; every installed
  header is byte-identical; the generated `.pc` matches field-for-field
  (modulo install-path values). Any difference is DIVERGENT; classified by
  whether it is in a registered class.
- **B (per-entry-point contract).** MATCH iff for each exported symbol the
  return value, out-params, ownership/free-pair, and error paths equal the pin's
  for the probed inputs. A parameter the pin honours and oxpinyin ignores is
  DIVERGENT.
- **C (output equivalence).** MATCH iff the same-data-dir differential drivers
  and the option sweep are byte-identical, except lines attributable to a
  registered class. A new differing line is DIVERGENT-UNREGISTERED.
- **D (persisted state).** MATCH iff the user dir round-trips both directions
  byte-identically on the same backend, and `pinyin_save`/`load` agree.
- **E (upstream defect preservation).** For each catalogued upstream defect:
  MATCH-if-reproduced (defined-but-wrong behaviour), or DIVERGENT-REGISTERED if
  it is a class (b)/(c) defect (UB/abort) that oxpinyin cannot/does-not
  reproduce, or DIVERGENT-UNREGISTERED if oxpinyin silently fixes a *defined*
  behaviour with no class.
- **F (error/diagnostic).** MATCH iff every upstream abort site reachable from
  caller input is answered `false`/`Err` under class (c) (and logged), and no
  oxpinyin path aborts where upstream returns.
- **G (numerical).** MATCH iff the §12 residual is exactly 491/396/390 and the
  candidate surface is bit-identical; any move either direction is DIVERGENT.
- **H (concurrency).** MATCH iff thread-safety equals upstream *including where
  upstream is unsafe*; oxpinyin being safer under contract-violating use is
  recorded (per mandate) and classified.
- **I (drop-in integration).** MATCH iff the installed file set/paths match, the
  headers compile from C and C++ at the named `-std` levels with `-Werror`, and
  pinned consumers relink and behave identically.
- **J (coverage).** MATCH iff every ABI-reachable upstream function maps to an
  oxpinyin counterpart or is deliberately absent with a cited reason; counts
  regenerable by a committed command.
- **K (register reconciliation).** Produce the three lists; a register entry
  whose fix has landed but whose disposition was not updated is a stale-entry
  finding.
- **L (complexity).** MATCH iff time and space are not both worsened anywhere
  internal structure diverges; cite asymptotics + a measurement.

An axis is reported **MATCH only if** (i) its instrument is non-vacuous (§3) and
(ii) the comparison shows identity. Otherwise it is DIVERGENT (classified) or
NOT-ESTABLISHED.

### 2.2 Falsifier list (what would prove each "no divergence" conclusion wrong)

Each falsifier is paired with its outcome in this session.

| # | "no divergence" conclusion | falsifier (observation that would refute it) | outcome |
| --- | --- | --- | --- |
| Φ1 | libpinyin/libzhuyin export sets are identical | `nm -D --defined-only` set difference non-empty in either direction on the **shipped** objects | **not fired** — 79=79 and 52=52, both directions empty (§5.A) |
| Φ2 | shipped ABI is structurally identical | `abidiff` exit ≠ 0 | **not fired** — exit 0 both libraries (§5.A) |
| Φ3 | installed headers identical | any `diff` of an installed header non-empty | **not fired** — 7/7 byte-identical (§5.A) |
| Φ4 | candidate/parse/key surface identical | any same-data-dir driver other than the registered union-diff line differs | **not fired** — 11/12 IDENTICAL; union-diff = registered row 20 (§5.C) |
| Φ5 | §12 residual stable at 491/396/390 | the gate asserts a different number, or `comparable ≠ 496`, or `guessed_disagree ≠ 0`, or `list_order_only ≠ 0` | **not fired** — gate PASSED (§5.G) |
| Φ6 | same-backend user dir interoperates | round-trip exit ≠ 0, or a phase reports DIVERGENT/unreadable | **not fired** — exit 0, Phase C IDENTICAL, Phase D READABLE (§5.D) |
| Φ7 | every exported symbol is implemented | any of the 131 is STUB or ABSENT | **not fired** — 0 STUB, 0 ABSENT (§7) |
| Φ8 | the differential instrument is non-vacuous | an injected subject divergence is NOT caught | **not fired** — injected `is_incomplete` negation caught by key-surface-diff (§3) |
| Φ9 | the version metadata matches the pin | the generated `.pc` `Version`/include-subdir differs from `2.11.92` | **FIRED** — subject reports `2.11.91` (finding D1, §5.A/§6) |
| Φ10 | `pinyin_train` honours its `index` | oxpinyin trains a different result than the pin for `index≠0` | **FIRED at source** — `_index` unused (finding D2, §5.B); empirical index≠0 repro is a lead |
| Φ11 | the four open revert targets are closed at HEAD | a fix commit for row 33/34/36/37 is an ancestor of HEAD | **not fired** — only registration docs are on main; fixes are not (finding D4–D7, §6) |
| Φ12 | the register/policy table reflects HEAD | a row's disposition contradicts the landed source | **FIRED** — policy row 35 still "REVERT TARGET" though its fix landed on main (finding D13, §6) |

Falsifiers Φ9, Φ10, Φ12 fired and are carried into the ledger as findings. The
rest did not fire, so the corresponding MATCH conclusions stand **subject to the
instrument-validation status in §3** (an axis whose instrument was not mutation-
validated is NOT-ESTABLISHED even if its comparison showed identity).

---

## 3. Instrument-validation table (Step 3)

The mandate: a differential that never fires proves nothing. For each axis where
MATCH is claimed, the instrument's non-vacuity must be shown. "Injected
mutation" = a deliberate divergence introduced into the subject in the container
scratch build (`/repo`, never committed), detected, then reverted. "Real-detection"
= the instrument was observed to fire on a genuine divergence (weaker than an
injected mutation, and labelled as such).

| axis | instrument | validation kind | mutation / divergence | detected? | reverted? | status |
| --- | --- | --- | --- | --- | --- | --- |
| C (output equivalence) | `run-same-data-dir-diff.sh` (key-surface-diff driver) | **injected mutation** | negate `pinyin_get_pinyin_is_incomplete` (`crates/oxpinyin-capi/src/keys.rs:260@656ef0b5`: `core.middle==0 && core.final_==0` → `!(…)`) | **YES** — exit 2, `render\|full\|zhong\|incomplete\|0`→`\|1` (and `zi`, `hao`, …) | **YES** — line 260 restored, confirmed | **PASS** |
| C (output equivalence) | `run-same-data-dir-diff.sh` (union-diff driver) | real-detection | the naturally-occurring row-20 class-(a) line | YES — exit 2, one `pred: type=4 text=你` line | n/a (not a mutation) | corroborates PASS |
| A (ABI surface) | `nm -D --defined-only` + `readelf -V` | real-detection | the **raw** cargo-c cdylib (unversioned `pinyin_*`, extra `oxpinyin_init_for_fixtures`) vs the oracle's versioned `pinyin_*@@LIBPINYIN` | YES — set difference non-empty, version tags absent | n/a (two real artifacts; the relink resolves it) | non-vacuity **shown**; no injected ABI mutation performed |
| A (ABI surface) | `abidiff` | real-detection (depth check) | confirmed both shipped objects carry `.debug_info` (`readelf -S … | grep -c debug_info` = 1 each), so abidiff ran a **deep** comparison, not symbol-only | abidiff exit 0 on identical inputs is meaningful only because it had DWARF; depth confirmed | n/a | deep-comparison **established**; injected type mutation not performed |
| G (numerics) | `sentence_surface_parity` gate | strict-assertion + real-detection | the gate `assert_eq!`s 491/396/390/496/0/0/6 (not a tautology); the union-diff row-20 line is the same class-(a) trellis residual observed live | the gate would fail on any move; it PASSED | n/a | strict-assertion **established**; injected trellis mutation not performed |
| D (persisted state) | `user-dir-round-trip.sh` | real-detection | the round-trip asserts byte-identity of the pin's render of oxpinyin's rewrite (Phase C) and readability (Phase D); it also exercises the `check_format` wipe path | exit 0 with the wipe path printed — the script distinguishes conform/non-conform | n/a | non-vacuity **argued** (it can and does report DIVERGENT/unreadable); injected corrupt-store mutation not performed |
| B, E, F, H, I, L | — | — | — | — | — | **NOT-ESTABLISHED** (no injected mutation this session) |

**Consequence, applied honestly:** only **axis C** has a passing *injected*
mutation check. Axes **A, D, G** have instruments shown non-vacuous by
real-detection / strict-assertion / DWARF-depth, which is strong but is **not**
the injected mutation the mandate prefers; their MATCH claims are recorded with
that qualifier. Axes **B, E, F, H, I, L** have **no** passing mutation check and
are reported **NOT-ESTABLISHED** for the MATCH verdict, regardless of favourable
source-level reading. ("Not established" ≠ "refuted".)

Reproduction (axis C injected mutation, run inside `oxaudit`):

```sh
# in container, /repo = HEAD source (scratch copy; never committed)
sed -i '260s/core.middle == 0 && core.final_ == 0/!(core.middle == 0 \&\& core.final_ == 0)/' \
    crates/oxpinyin-capi/src/keys.rs
cargo build --release --locked -p oxpinyin-capi --no-default-features --features tkrzw   # "Compiling oxpinyin-capi"
tools/bisection/run-same-data-dir-diff.sh /opt/oracle-tkrzw/lib/libpinyin.so \
    target/release/deps/libpinyin_capi.so /opt/oracle-tkrzw/lib/libpinyin/data key-surface-diff
# → DIVERGENCE, exit 2 (incomplete rows flip 0→1). Then restore keys.rs:260.
```

A side observation worth recording: the **first** mutation attempt (parsed_len
`+1` in `pinyin_get_parsed_input_length`) was **not** caught by
key-surface-diff / live-typing-diff / dict-surface-diff because none of those
three drivers call that getter (only `abi-probe-diff`, `bisect`, `chewing-diff`,
`fullpin-diff`, `option-sweep`, `scheme-diff` do). That is a driver-coverage
fact, not a vacuous instrument — and it is why the validated mutation targets a
function the chosen driver provably exercises.

---

## 4. Flat ledger (sorted by verdict severity)

Columns: `ID | axis | claim | upstream evidence | subject evidence | verdict | severity | repro | proposed classification`.
Verdicts: **U**=DIVERGENT-UNREGISTERED, **R**=DIVERGENT-REGISTERED,
**N**=NOT-ESTABLISHED, **M**=MATCH. Severity: 1 high … 4 low / informational.

| ID | axis | claim | upstream evidence | subject evidence | verdict | sev | repro | proposed classification |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| D1 | A/I | The drop-in reports package version **2.11.91** and installs headers under `libpinyin-2.11.91`; the pin is **2.11.92** (`libpinyin-2.11.92`) | `libpinyin.pc` `Version: 2.11.92`, `libpinyinincludedir=…/libpinyin-2.11.92`; `configure.ac` micro_version 92 @074a2219 | generated `.pc` `Version: 2.11.91`, `Cflags: -I…/libpinyin-2.11.91`; `crates/oxpinyin-capi/Cargo.toml` `[package.metadata.capi.pkg_config] version="2.11.91"`, `header.subdirectory="libpinyin-2.11.91"`@656ef0b5 | **U** | 2 | `podman exec oxaudit cat /opt/oracle-tkrzw/lib/pkgconfig/libpinyin.pc` vs `…/subject-tkrzw/stage/usr/lib/x86_64-linux-gnu/pkgconfig/libpinyin.pc` | none of (a)/(b)/(c) — a stale version constant; **defect to revert** (bump metadata to 2.11.92) |
| D2 | B | `pinyin_train(instance, index)` **ignores `index`**; the pin trains the `index`-th n-best result | `assert(index < results.size()); check_result(results.get_result(index, result))` `src/pinyin.cpp:2684-2687@074a2219` | `pub extern "C" fn pinyin_train(instance, _index: u8)` → `inst.core.train()` (index-free); doc: "The `index` n-best parameter is accepted but unused" `crates/oxpinyin-capi/src/candidates.rs:550@656ef0b5` | **U** | 2 | source-cited; empirical `train(index≠0)` repro is a lead (§8). All in-tree drivers call `train(inst,0)` | none — a parameter-contract divergence; **defect** (or register). The comment "C ABI has no n-best sentence results yet" is suspect: `pinyin_get_sentence` honours `index` (ledger row 28) |
| D3 | B/C | `zhuyin_iterator_add_phrase` substitutes `PinyinKey::MAX` for an out-of-range key index where the pinyin twin **rejects** the phrase | `src/zhuyin.cpp:500@074a2219` vs `src/pinyin.cpp:614@074a2219` | `crates/oxpinyin-zhuyin-capi/src/iterators.rs:59@656ef0b5` vs `crates/oxpinyin-capi/src/iterators.rs:114@656ef0b5` | **U** | 3 | source-cited (axis-J ledger flag); needs a targeted add-phrase differential | none — internal inconsistency; **defect to review** |
| D4 | C/E | Row 33 — whole-row NBEST choose + train writes a user bigram the pin does not | pin `train_result3` writes nothing without `CONSTRAINT_ONESTEP` (`phonetic_lookup.h:866`, `pinyin.cpp:2515-2520@074a2219`) | `Session::train` history fallback seeds `sentence_start→phrase` (`selection.rs:298-338@656ef0b5`) | **R** | 2 | `compatibility-policy.md` row 33; `revert-plan.md §10` marked **open** at HEAD; no fix commit on main touches `selection.rs` since 2026-09-19 | **REVERT TARGET** (registered, open at HEAD) |
| D5 | C | Row 34 — imported user phrase lost after `guess_sentence` at `0x1f` | pin keeps `m_nbest_results` across parse (`pinyin.cpp:2693-2704@074a2219`) | oxpinyin clears nbest on every `begin_parse` (`instance.rs:145-194@656ef0b5`) | **R** | 2 | `compatibility-policy.md` row 34; fix NOT on main | **REVERT TARGET** (registered, open at HEAD) |
| D6 | B | Row 36 — `pinyin_bigram_iterator_get_next_phrase` last-row return value | pin returns `has_next_phrase` after advancing (`pinyin.cpp:896-911@074a2219`) → `false` on last row | returns `true` for every fetched row (`crates/oxpinyin-capi/src/iterators.rs:373-410@656ef0b5`) | **R** | 3 | `compatibility-policy.md` row 36; fix NOT on main | **REVERT TARGET** (registered, open at HEAD) |
| D7 | C | Row 37 — candidate window behind the composition offset after a choose | pin rebuilds the window from `start=offset` each call (`pinyin.cpp:2184-2262@074a2219`) | oxpinyin serves the composition-anchored cached list (`crates/oxpinyin-capi/src/sentence.rs:319-339@656ef0b5`) | **R** | 2 | `compatibility-policy.md` row 37; fix NOT on main | **REVERT TARGET** (registered, open at HEAD) |
| D8 | C/G | union-diff emits one extra predicted row `pred: type=4 text=你` after train-then-predict `测测` | pin's trained `测测→你` count stays below the `m_count≥10` filter (`pinyin.cpp:2340-2366@074a2219`) | oxpinyin's fixed-point trellis residual tips the count over 10 | **R** | 3 | `run-same-data-dir-diff.sh … union-diff` → exit 2, one line (§5.C) | **(a) MATH** — registered row 20 (trellis residual) |
| D9 | G | §12 sentence-surface residual 491/396/390 of 496 | pin `m_poss += log(…)` per step into a `gfloat` (`phonetic_lookup.h:663,692@074a2219`) | oxpinyin fixed-point surprisal scale (`crates/oxpinyin-core/src/cost.rs@656ef0b5`) | **R** | 3 | `cargo test --release -p pinyin-oracle --features tkrzw --test sentence_surface_parity -- --include-ignored` → PASS (§5.G) | **(a) MATH** — FROZEN permanent Stage-1 divergence (rows 11/12) |
| D10 | E | class-(b) memory-safety defects not reproduced: aux over-read, bigram-export stale buffer, index struct padding | `src/pinyin.cpp:3414-3419`, `:842-872@074a2219` | safe-Rust `&str` bounds / owned snapshots | **R** | 4 | `reference/memory-safety-bugs.md` §1.4/§3.2; `compatibility-policy.md` rows 1, 18 | **(b) MEMORY SAFETY** |
| D11 | E/F | class-(c) abort-on-caller-input defects answered `false`/`Err`: rows 4, 5a, 5c, 5d, 6, 10, 14, 19, 21, 22 | e.g. `assert(index<results.size())` `pinyin.cpp:1463-1482@074a2219`; `abort()` `zhuyin_parser2.cpp:291-295@074a2219`; `pinyin_parser2.cpp:611-612@074a2219` | oxpinyin returns `false`/`Err` + logs | **R** | 3 | `compatibility-policy.md` rows 4/5a/5c/5d/6/10/14/19/21/22; `upstream-report-drafts.md` items 2–4 | **(c) AVAILABILITY** (MISRA D.4.1; constitution item 4) |
| D12 | F/K | tkrzw C-API collapses C++-exception origin (`UNKNOWN_ERROR`) and OS error (`SYSTEM_ERROR`) into one `SYSTEM_ERROR`→`StoreError::Io` | `tkrzw_langc.cc` 135 `catch` sites → `TKRZW_STATUS_SYSTEM_ERROR` | `crates/oxpinyin-store/src/tkrzw/mod.rs:214@656ef0b5` | **R** | 4 | `docs/findings/tkrzw-langc-exception-classification.md`; reachable only under memory exhaustion | the mandate's "**ABI information loss**" = this registered standing divergence (not a policy (a)/(b)/(c) class) — see §6 |
| D13 | K | policy table row 35 ("user-library tokens refused an n-best step cost") still marked **REVERT TARGET**; totals say "REVERT TARGET 5 (rows 33,34,35,36,37)" | — | the fix landed on main: `7c9a6923` ("price user-file tokens' n-best step cost from the user delta") is an ancestor of HEAD; `crates/oxpinyin-data/src/lm/mod.rs:553 nbest_step_costs_with_user_delta@656ef0b5`; `revert-plan.md §12` says "closed in code (2026-09-19)" | **U** (register integrity) | 3 | `git merge-base --is-ancestor 7c9a6923 HEAD` → true; `git show HEAD:crates/oxpinyin-data/src/lm/mod.rs \| grep nbest_step_costs_with_user_delta` | stale register entry — **row 35 is CLOSED at HEAD; open revert targets are 4 (33,34,36,37), not 5** |
| D14 | A/K | `installed-naming.md` states version `2.11.91` is "the pin oxpinyin targets" | pin is 2.11.92 | `docs/findings/installed-naming.md:179@656ef0b5` | **U** (doc integrity) | 3 | `grep -n "2.11.91" docs/findings/installed-naming.md` | stale doc prose (the mandate's "stale doc" hazard) — do not act on it; it is the same root as D1 |
| D15 | I/K | `Dockerfile.perf-matrix` cell D is labelled "oxpinyin + Kyoto Cabinet (default)" and installs to `/opt/oxpinyin-kc`, but builds with **no** feature flags → now the default **bdb** | — | `tools/bisection/Dockerfile.perf-matrix:116-124@656ef0b5`; `default=["bdb"]` since `ef7f2b41` | **U** (recipe integrity) | 3 | read the Dockerfile cell D vs `crates/oxpinyin-store/Cargo.toml` `default` | stale recipe — silently builds BDB into a KC-named dir |
| D16 | K | the mandate's premise "`upstream-report-drafts.md` (13 documented items)" | — | the file has **5** drafts (items 1–5) | informational | 4 | `grep -c '^## ' docs/findings/upstream-report-drafts.md` | task-premise discrepancy; reported per "verify against the tree, not the prompt" |
| N1 | B | per-entry-point contract battery (NULL/empty/out-of-range/oversized/aliasing/double-free for all 131) | — | coverage ledger maps all 131 (§7) but the full input battery was not run | **N** | 2 | — | needs the abi-probe contract battery + targeted probes (§8) |
| N2 | C | full `pinyin_option_t` matrix sweep (every bit + interactions + reserved bits) | option bits in `src/pinyin.h@074a2219`; `option-bits.md` | `option-sweep.c` exists | **N** | 2 | `run-option-sweep.sh` (builds a debug capi — not run this session) | instrument present, not executed |
| N3 | E | per-defect empirical reproduction of every catalogued upstream defect | `reference/memory-safety-bugs.md` (14 catalogued), `robustness-evidence.md` (F-E-01…14) | class (b)/(c) diverge by design | **N** | 3 | each defect needs its own C repro vs the oracle | catalogue read; empirical repro per defect not run |
| N4 | F | full per-site assert/abort enumeration + mapping | pin `src/`: **280** `assert(`, **61** `abort()`, **0** `g_assert`/`g_error`; **67** assert/abort in `pinyin.cpp`+`zhuyin.cpp` | class (c) register rows | **N** | 2 | `grep -rn "assert(\|abort()" ~/oxpinyin-audit/libpinyin-pin/src` | enumeration started (subagent) but not completed this session |
| N5 | H | fork/signal-safety/env-vars/`setlocale` dependence | — | `send-sync-audit-2026-09-21.md` covers the sharing machinery | **N** | 3 | — | send-sync read; fork/signal/env not empirically probed |
| N6 | I | consumer relink + identical runtime (ibus-libpinyin 1.16.5, fcitx5) | ibus-libpinyin pin `2d2cdac` | — | **N** | 2 | the pinned-frontend relink fixture | not performed (no ibus/fcitx build this session) |
| N7 | L | complexity caveat (time & space not both worsened) | — | — | **N** | 3 | differential baseline capture | not performed |
| N8 | A/D/G | injected-mutation validation for A, D, G | — | real-detection / strict-assertion only (§3) | **N** | 3 | inject an ABI/store/trellis mutation | only axis C had an injected mutation |
| N9 | C | libzhuyin **behavioural** differential (`zhuyin-diff`) | — | libzhuyin ABI built & MATCH (§5.A) | **N** | 3 | `run-zhuyin-diff.sh` | ABI established; behaviour not run |
| M1 | A | version scripts identical | `src/libpinyin.ver` 79, `src/libzhuyin.ver` 52 @074a2219 | `crates/oxpinyin-capi/libpinyin.ver`, `crates/oxpinyin-zhuyin-capi/libzhuyin.ver`@656ef0b5 | **M** | 1 | `comm` of extracted symbol sets — empty both directions (§5.A) | — |
| M2 | A | installed headers byte-identical | `src/pinyin.h`, `src/zhuyin.h`, `src/include/novel_types.h`, `src/storage/{pinyin,zhuyin}_custom2.h`@074a2219 | the 5 capi/zcapi headers @656ef0b5 | **M** | 1 | `diff -q` all 7 pairs → identical (§5.A) | — |
| M3 | A | shipped **libpinyin** ABI identical | oracle `libpinyin.so.15.0.0` (79 versioned `@@LIBPINYIN`, SONAME `libpinyin.so.15`) | relinked subject (79 versioned, same SONAME/node) | **M** | 1 | `nm`/`readelf -V`/`abidiff` exit 0 (§5.A); instrument non-vacuous per §3 | — |
| M4 | A | shipped **libzhuyin** ABI identical | oracle `libzhuyin.so.15.0.0` (52 versioned `@@LIBZHUYIN`, SONAME `libzhuyin.so.15`) | relinked subject (52 versioned) | **M** | 1 | `nm` set-diff empty both ways; `abidiff` exit 0 (§5.A) | — |
| M5 | C | drop-in candidate/parse/key/punct/import/live-typing/nbest-train surface identical | oracle on its own `data/` | subject on the same `data/` | **M** | 1 | `run-same-data-dir-diff.sh` → 11/12 IDENTICAL (union-diff = D8) (§5.C) | — |
| M6 | D | same-backend user dir round-trips both directions | pin-trained profile | oxpinyin reads/rewrites; pin reads oxpinyin's | **M** | 1 | `user-dir-round-trip.sh /opt/oracle-tkrzw` → exit 0, Phase C IDENTICAL, Phase D READABLE (§5.D) | — |
| M7 | G | §12 residual stable | — | — | **M** | 1 | gate PASS (§5.G) | — |
| M8 | J | every exported symbol implemented | 131 in `.ver` | 131 `#[unsafe(no_mangle)]` defs | **M** | 1 | §7 regeneration command → 131/79/52, 0 STUB, 0 ABSENT | — |

---

## 5. Per-axis narrative (anything not MATCH, plus the MATCH evidence)

### A — ABI surface (MATCH for the shipped objects; two unregistered findings)

**What was compared.** The *shipped* drop-in is the cargo-c output **relinked**
by `tools/packaging/relink-versioned.sh` (the raw cdylib has unversioned symbols
— see below). Comparing the relinked objects:

- **Version scripts** (`libpinyin.ver`, `libzhuyin.ver`): symbol sets extracted
  and `comm`-diffed — **79=79 and 52=52, empty both directions**; both use the
  unversioned node names `LIBPINYIN`/`LIBZHUYIN` (`local: *;`).
- **SONAME / symlink chain**: both `libpinyin.so → libpinyin.so.15 →
  libpinyin.so.15.0.0` and `libzhuyin.so → …so.15 → …so.15.0.0`; SONAMEs
  `libpinyin.so.15` / `libzhuyin.so.15` on both sides (`readelf -d`).
- **`nm -D --defined-only`**: shipped subject exports **79 versioned
  `pinyin_*@@LIBPINYIN`** and **52 versioned `zhuyin_*@@LIBZHUYIN`**, set-identical
  to the oracle both directions (version-stripped `comm` empty).
- **Symbol versioning** (`readelf -V`): both carry the `LIBPINYIN`/`LIBZHUYIN`
  version definition with base node = the SONAME; every export is versioned.
- **`abidiff`**: **exit 0** for libpinyin and for libzhuyin. Depth confirmed —
  both shipped objects carry `.debug_info` (`readelf -S | grep -c debug_info` = 1),
  so this is a deep type comparison, not symbol-only.
- **Installed headers**: `pinyin.h`, `zhuyin.h`, `novel_types.h`,
  `pinyin_custom2.h`, `zhuyin_custom2.h` all **byte-identical** (`diff` exit 0).
  Because the public header is identical, every exposed enum value
  (`lookup_candidate_type_t`, `sort_option_t`), typedef width, and opaque-pointer
  semantic is identical by construction. The context/instance/candidate/iterator
  types are opaque forward-declared typedefs (`pinyin.h:35-41@074a2219`), so no
  internal layout crosses the ABI for them. `ChewingKey`/`ChewingKeyRest` are
  also forward-declared in the public header (`pinyin.h:32-33@074a2219`; the
  packed definitions live in the *internal* `src/storage/chewing_key.h:41,97`, not
  installed) — the library fills a caller-provided `ChewingKey*` and the consumer
  reads it back. Their effective layout and semantics are **empirically
  confirmed**: `key-surface-diff.c:49` carries
  `_Static_assert(sizeof(ChewingKey)==2, …)`, and the driver's readback of the
  filled key bits is part of the **2131-line IDENTICAL** key-surface log (§5.C).
  No empirical `pahole`/`offsetof` probe of the installed objects was run beyond
  this; a full struct-layout differential is a lead (§8).

**Raw cargo-c cdylib is NOT the drop-in (methodology + non-vacuity).** The
pre-relink cdylib (`cargo cinstall` output) has **unversioned** `pinyin_*` and
**extra** symbols (`oxpinyin_init_for_fixtures`, `oxpinyin_test_set_user_bigram`,
`oxpinyin_alloc_*` — the fixture/alloc-count hooks, compiled out only under
`--features shipped`). The `nm` instrument caught this (set difference non-empty,
version tags absent), which is the real-detection non-vacuity evidence in §3.
`relink-versioned.sh` applies the `.ver` (`local: *;` hides the hooks) and
produces upstream's versioned shape. **Consequence for the audit:** the ABI MATCH
claims (M3/M4) are for the *shipped relinked* artifact; a consumer that shipped
the raw cdylib would diverge (glibc `no version information available` warning
and a different export shape).

**Finding D1 — version 2.11.91 vs 2.11.92 (DIVERGENT-UNREGISTERED).** The
generated `libpinyin.pc` reports `Version: 2.11.91` and
`Cflags: -I${includedir}/libpinyin-2.11.91`, and the headers install under
`libpinyin-2.11.91`; the pin's `.pc` reports `Version: 2.11.92` and
`libpinyin-2.11.92`. Source: `crates/oxpinyin-capi/Cargo.toml`
`[package.metadata.capi.pkg_config] version = "2.11.91"` and
`[package.metadata.capi.header] subdirectory = "libpinyin-2.11.91"` (mirrored by
`build.rs` and by the zhuyin crate). The pin moved 2.11.91 → 2.11.92 on
2026-09-06; the version metadata was not bumped. Observable surface:
`pkg-config --modversion libpinyin` (2.11.91 vs 2.11.92), the include
subdirectory name, and any consumer doing
`pkg-config --atleast-version=2.11.92 libpinyin` (fails against oxpinyin). The
runtime SONAME (`libpinyin.so.15`) is unaffected, so an already-compiled consumer
that only relinks is unaffected; a consumer that *recompiles* against the
drop-in's `.pc` gets the 2.11.91 include path. **Not registered** in
`upstream-divergences.md` or the policy table; `installed-naming.md:179` even
asserts (stalely) that "2.11.91 [is] the pin oxpinyin targets" (finding D14).
Proposed classification: a plain stale-constant **defect to revert** (bump to
2.11.92), not an exception class.

**Finding D15 — perf-matrix cell D stale recipe.** `Dockerfile.perf-matrix`
cell D runs `cargo cinstall … -p oxpinyin-capi` with no feature flags into
`/opt/oxpinyin-kc`, labelled "Kyoto Cabinet (default)". Since the workspace
default became `bdb` (2026-09-20), that cell silently builds **Berkeley DB**
into a KC-named directory. Informational/recipe-integrity; it does not affect
this audit (which built tkrzw explicitly) but would mislead a perf-matrix run.

### B — Per-entry-point contract (NOT-ESTABLISHED overall; two findings)

The 131-symbol coverage ledger (§7) establishes that every export has a real
counterpart (0 STUB, 0 ABSENT) and records, per symbol, whether it is a thin
WRAPPER or IMPLEMENTED, with upstream and subject citations. That is the
*structural* half of axis B. The *contract* half — NULL / empty / out-of-range /
oversized inputs, out-param write-vs-untouched on failure, ownership and the
exact matching free function, aliasing/overlapping buffers, call-order and
lifecycle violations, idempotence, double-free/use-after-free tolerance — was
**not** run as an empirical battery this session (the `abi-probe-diff` contract
probe and targeted per-symbol drivers were not executed). Axis B is therefore
**NOT-ESTABLISHED** for the contract verdict, with two concrete source-level
findings:

- **D2 — `pinyin_train` ignores `index`.** Upstream trains
  `results.get_result(index)` after `assert(index < results.size())`
  (`src/pinyin.cpp:2684-2687@074a2219`); oxpinyin's wrapper takes `_index: u8`
  and calls the index-free `InstanceCore::train()`
  (`crates/oxpinyin-capi/src/candidates.rs:550@656ef0b5`). For `index≠0`
  (reachable whenever `guess_sentence` produced >1 n-best result) the pin trains
  a different sentence decomposition → different user unigram/bigram writes;
  oxpinyin trains the top/selected one. For `index ≥ results.size()` the pin
  **aborts** (class (c)); oxpinyin does not. No in-tree driver exercises
  `index≠0` (all call `train(inst,0)`), so the divergence is unmeasured but
  source-confirmed. The wrapper's doc rationale ("the C ABI has no n-best
  sentence results yet") is inconsistent with `pinyin_get_sentence` honouring
  `index` (ledger row 28) — flagged as suspect/stale.
- **D3 — `zhuyin_iterator_add_phrase` `PinyinKey::MAX` substitution** for an
  out-of-range key index, where the pinyin twin rejects the phrase
  (`zcapi/iterators.rs:59` vs `capi/iterators.rs:114@656ef0b5`). Minor internal
  inconsistency; needs a targeted add-phrase differential.

### C — Output equivalence (MATCH on 11/12 drivers; one registered divergence)

`run-same-data-dir-diff.sh` (oracle vs subject, both on the pin's own
`/opt/oracle-tkrzw/lib/libpinyin/data`, tkrzw) → **11/12 drivers IDENTICAL**:
key-surface (2131 lines), dict-surface (168), phrase-surface (19), pred-order
(1588), predict (5), punct (17), addon-candidate (4), user-candidate (1), import
(16), live-typing (317), nbest-train (56). The 12th, **union-diff**, differs by
exactly one line — the subject emits `pred: type=4 text=你` after the
train-then-predict `测测` sequence. This is the **registered row 20, class (a)**
trellis residual (`upstream-divergences.md:359-387`): the fixed-point-vs-`gfloat`
trellis picks a different first phrase after `测测`, so the trained `测测→你`
count straddles `_compute_predicted_bigram_candidates`'s `m_count ≥ 10` filter
(`pinyin.cpp:2340-2366@074a2219`) and oxpinyin clears it. The register recorded
it on Kyoto Cabinet data; this session reproduced it on **tkrzw**, consistent
with the backend-independent class-(a) mechanism. Verdict DIVERGENT-REGISTERED
(D8). The **full option matrix sweep** (`option-sweep`, every `pinyin_option_t`
bit + interactions + reserved bits) was **not** run (it builds a debug capi);
axis C is MATCH for the default/`0x1e`-family surface the 12 drivers cover, and
**NOT-ESTABLISHED** for the exhaustive option matrix (N2).

### D — Persisted state and on-disk formats (MATCH for the round-trip)

`tools/oracle/user-dir-round-trip.sh /opt/oracle-tkrzw nihao nisha nihaoshijie`
→ **exit 0**: Phase C "IDENTICAL: the pin renders oxpinyin's rewrite of its own
profile byte-identically (5 rows)"; Phase D "READABLE: the pin loads and renders
oxpinyin's own profile (3 rows, 3 phrase rows, all expected)". The
`check_format`-driven wipe of a non-conforming profile fired and was reported
("non-conforming user profile wiped"), matching libpinyin's own behaviour. This
establishes **bidirectional same-backend (tkrzw) user-dir interop at HEAD**
(M6). Not established: corrupt/truncated/empty/wrong-version/newer-version file
handling beyond the wipe path; save atomicity (temp+rename vs in-place); file
modes/umask; the value-level import/export byte-for-byte formats; and the BDB
and KC backends (only tkrzw was built). Those are leads (§8).

### E — Upstream defect preservation (NOT-ESTABLISHED empirically; catalogue mapped)

The catalogued upstream defects (`reference/memory-safety-bugs.md` §1–6, 14
items; `robustness-evidence.md` F-E-01…14; `upstream-report-drafts.md` 5 drafts)
were read and mapped to oxpinyin's disposition. The library-ABI-relevant ones:
aux over-read (§1.4) and bigram-export stale buffer (§3.2) are **class (b)**
(safe Rust cannot express the UB); `zhuan`/apostrophe asserts (§6.1, F-E-14) and
the scheme-setter/get-sentence/incomplete-key aborts are **class (c)** (answered
`false`/`Err` + logged). The mandate's bug-for-bug question — does oxpinyin
*reproduce*, *silently fix*, or *fix under a registered class* — resolves to
"diverges under (b)/(c)" for the UB/abort defects (registered, permitted) and
"reproduces" for defined-but-wrong behaviour (e.g. the double-out-of-enum
half-mutation, row 5b CLOSED). **No per-defect empirical reproduction was run
this session** (each needs its own C repro against the oracle), so axis E is
**NOT-ESTABLISHED** for the empirical verdict (N3); the catalogue mapping is
recorded and the class-(b)/(c) dispositions are inherited from the register
(which §6 reconciles). Note: the mandate's "13 documented items" in
`upstream-report-drafts.md` is **5** in the tree (D16).

### F — Error and diagnostic behaviour (NOT-ESTABLISHED for the full enumeration)

Raw counts in the pin (`grep -rn` over `src/`, excluding tests): **280
`assert(`**, **61 `abort()`**, **0** `g_assert`/`g_error`/`G_BREAKPOINT`; **67**
assert/abort sites in the two ABI facades `pinyin.cpp`+`zhuyin.cpp`. The
register maps the caller-reachable abort sites to class (c) (rows 4, 5a, 5c, 5d,
6, 10, 14, 19, 21, 22 — D11) and one specific site was confirmed first-hand this
session: `pinyin_train`'s `assert(index < results.size())`
(`src/pinyin.cpp:2685@074a2219`). The **full per-site enumeration and mapping**
(each of the 341 sites: condition, caller-reachability, trigger, oxpinyin
handling, class) was started via a subagent but **not completed**; axis F is
**NOT-ESTABLISHED** for the complete enumeration (N4). The
**SYSTEM_ERROR/UNKNOWN_ERROR collapse** (D12) is confirmed to be exactly the
registered binding-ABI standing divergence: it lives below the libpinyin C ABI
(in oxpinyin's tkrzw store binding), is reachable only under memory exhaustion,
and maps the C++-exception origin to `StoreError::Io` (`tkrzw/mod.rs:214`). It is
**not broader** than the registered entry — no *additional* error-information
loss at the libpinyin C ABI boundary was found beyond it (the C ABI returns
`bool`/`false` exactly where the pin does, per the coverage ledger's "every
`return false` is an input-validation guard").

### G — Numerical semantics (MATCH at the frozen residual)

The §12 gate `sentence_surface_matches_the_declared_residual` **PASSED**
(13.08s, genuinely measured — `PINYIN_EXPORT_DIR`/`PINYIN_MODEL_DIR` present, so
it asserted rather than self-skipped): `comparable=496`, `guessed_disagree=0`,
`row0_match=491`, `distinct_set_match=396`, `list_ordered_match=390`,
`rows_match=390`, `list_order_only=0`, `list_distinct_extra=6`. So the residual
**still sits at 491/396/390** at HEAD on tkrzw — no move in either direction
(M7/D9). Mechanism: the pin accumulates `m_poss += log(…)` into a `gfloat`
(`phonetic_lookup.h:663,692@074a2219`); oxpinyin uses an exact integer
fixed-point surprisal scale (`crates/oxpinyin-core/src/cost.rs@656ef0b5`) — the
registered class-(a) FROZEN divergence. The candidate surface is bit-identical
(`list_order_only=0`, 0 leaks). Float width / accumulation order / `log` source
/ lambda & mixture parameters / pruning thresholds / comparator tie-breaks were
not individually re-derived from source this session; the residual gate is the
authoritative end-to-end check and it holds.

### H — Concurrency and process state (NOT-ESTABLISHED; one recorded judgment)

`send-sync-audit-2026-09-21.md` documents that oxpinyin's C ABI makes **no**
thread-safety promise (matching libpinyin), and that its internal
`Arc`/`Mutex`/`RwLock`/`SeqCst`-atomic machinery exists to let an instance outlive
its context (required even single-threaded) — "stronger than the C contract" but
"not a divergence" because it is below the ABI and invisible under the
caller-serialises contract. **The mandate's axis H explicitly says being safer
than an unsafe upstream is itself a divergence to record.** The two positions
reconcile as: under contract-compliant single-threaded use the synchronisation is
unobservable (MATCH); under contract-*violating* concurrent use the pin races
(UB) while oxpinyin serialises (defined) — a "safer" difference from UB, which is
the class-(b) shape (UB not reproducible in safe Rust). This is recorded as a
judgment point, not asserted as MATCH. fork behaviour, signal safety, environment
variables read, `setlocale` dependence, and init/deinit ordering were **not**
empirically probed (N5).

### I — Drop-in integration (NOT-ESTABLISHED beyond the installed tree)

Established this session: the installed header set is byte-identical (M2) and the
shipped `.so` carries upstream's SONAME, version node, and versioned export set
(M3/M4). **Not** established: header includability from C and C++ at
`-std=c89/c99/c11/c++11` with `-Wall -Wextra -Werror`; `extern "C"` correctness
under a real compile; the generated `.pc` consumed via `pkg-config` end-to-end
(the `.pc` version divergence D1 is the one field confirmed to differ); the
installed file set/paths under `tools/packaging/install.sh`; and the **consumer
relink** of ibus-libpinyin 1.16.5 / fcitx5 against both libraries with identical
runtime behaviour (N6). The `.pc.in` templates differ in one respect: oxpinyin
hardcodes `exec_prefix=${prefix}` / `includedir=${prefix}/include` where the pin
uses configure-substituted `@exec_prefix@`/`@includedir@` — equivalent for a
standard prefix, divergent only under a non-standard `--exec-prefix`/`--includedir`
(which does not apply to a prebuilt drop-in).

### J — Coverage ledger (MATCH; see §7)

### K — Divergence register reconciliation (see §6)

### L — Complexity caveat (NOT-ESTABLISHED)

No differential time/space baseline was captured this session (N7). The internal
structure diverges from upstream substantially (fixed-point trellis, Rust store
backends, owned snapshots), so the "time and space not both worsened" property is
**not established** by this audit; the repository's own perf findings
(`docs/findings/perf-*`) are claims, not evidence, per the mandate.

---

## 6. Divergence-register reconciliation (axis K)

The registered classes are the policy's **(a) MATH**, **(b) MEMORY SAFETY**,
**(c) AVAILABILITY**, **(d) CONSUMER SCOPE — RETIRED** ("may not be used to
justify anything"), plus the separately-registered **standing divergence** the
mandate names "**ABI information loss**" (= the tkrzw binding-ABI error-origin
collapse, `tkrzw-langc-exception-classification.md`). **Reconciliation note:**
"ABI information loss" is **not** one of the policy's four exception classes; the
policy says "There are four, and no others" ((a)/(b)/(c)/(d)). It is a
maintainer-accepted standing divergence that "fits none of the previously
accepted divergence buckets". The mandate's axis-K list therefore conflates the
three live exception classes with this fourth standing divergence; this audit
keeps them distinct and flags the naming mismatch as a register-hygiene item.

**List 1 — unregistered divergences (defects):**
- **D1** version 2.11.91 vs 2.11.92 (`.pc` Version + include subdir). No class
  fits; a stale constant. Defect to revert.
- **D2** `pinyin_train` ignores `index`. No class fits (it is not an abort, not
  UB, not a transcendental); a parameter-contract divergence. Defect/register.
- **D3** `zhuyin_iterator_add_phrase` `PinyinKey::MAX` substitution vs the pinyin
  twin's rejection. Defect to review.
- **D14** `installed-naming.md` stale "2.11.91 = the pin" prose (doc integrity;
  same root as D1).
- **D15** `Dockerfile.perf-matrix` cell D stale recipe (recipe integrity).
- **D16** mandate-premise "13 items" vs the tree's 5 (informational).

**List 2 — divergences broader than their registered class (defects):** none
found. Each registered divergence observed (D8 row 20, D9 §12, D10 class b, D11
class c, D12 binding-ABI collapse) sits **within** its recorded class and cite.
The SYSTEM_ERROR/UNKNOWN_ERROR collapse (D12) was specifically checked to be **no
broader** than the registered entry: it is confined to the tkrzw store binding
under memory exhaustion, and no additional error-information loss was found at
the libpinyin C ABI boundary.

**List 3 — registered classes / entries with no surviving instance (stale):**
- **D13** policy table **row 35** is marked REVERT TARGET and the totals read
  "REVERT TARGET 5 (rows 33,34,35,36,37)", but row 35's fix (`7c9a6923`) is an
  ancestor of HEAD and `revert-plan.md §12` records it "closed in code". **At
  HEAD the open revert targets are 4 (rows 33, 34, 36, 37), not 5.** The policy
  table and its totals are stale.
- **Class (d)** is retired; rows 16/17 were re-dispositioned (16 CLOSED, 17
  CLOSED per the 2026-09-16 amendment). No surviving (d) instance — correctly so.
- (No class (a)/(b)/(c) entry was found to be without a surviving instance; the
  class-(a) residual is live (D8/D9), the class-(b)/(c) entries are live by
  design.)

**Open revert targets at HEAD:** rows 33, 34, 36, 37. Verified two ways: (i)
`docs/findings/revert-plan.md` *at HEAD* marks §10 (#33), §11 (#34), §13 (#36),
§14 (#37) **open** (`git show HEAD:docs/findings/revert-plan.md`); (ii) no commit
on main since the 2026-09-19 registration touches their source sites as a fix —
`git log --since=2026-09-19 HEAD -- crates/oxpinyin-engine/src/session/selection.rs`
(row 33) is empty, and the only post-registration touches of `instance.rs`
(row 34), `iterators.rs` (row 36), `sentence.rs` (row 37) are the redb-removal
refactor and the §9 sort-option port, not the row fixes. The row-registration
commits (`07a132f5`/`1aa801db`/`3301ec53`/`b34102cd`) are docs and *are* on main.
By contrast row 35's fix `7c9a6923` **is** an ancestor of HEAD
(`git merge-base --is-ancestor 7c9a6923 HEAD` → true; D13), and rows 32 (§9) and
38 (§15) are recorded CLOSED in code (2026-09-20, PR #496).

---

## 7. Coverage ledger (axis J) and regeneration command

Full 131-row ledger (symbol → upstream def → oxpinyin counterpart → status) was
produced by a read-only enumeration subagent and is summarised here; the per-row
citations follow the regeneration command's output.

| | pinyin_* | zhuyin_* | total |
| --- | --- | --- | --- |
| exported (`.ver`) | 79 | 52 | **131** |
| IMPLEMENTED | 51 | 32 | **83** |
| WRAPPER (thin delegation to a named, verified internal fn) | 28 | 20 | **48** |
| STUB (unconditional false/0/NULL) | 0 | 0 | **0** |
| ABSENT (no `#[no_mangle]` def) | 0 | 0 | **0** |

Rubric: IMPLEMENTED = body carries the operational logic; WRAPPER = null-guard /
C-string marshalling / out-param writes + delegation to ONE named internal fn
(all 48 delegates verified to exist); every `return false`/`return 0` arm
encountered is an input-validation guard, never an unconditional bail.

**Regeneration command** (from the repo root; outputs verified 131 / 79 / 52):

```sh
grep -rhA2 '#\[unsafe(no_mangle)\]' crates/oxpinyin-capi/src crates/oxpinyin-zhuyin-capi/src --include='*.rs' \
  | grep -oE 'extern "C" fn (pinyin_|zhuyin_)[A-Za-z0-9_]+' \
  | sed 's/extern "C" fn //' | sort -u > /tmp/defined.txt
wc -l < /tmp/defined.txt              # 131
grep -c '^pinyin_' /tmp/defined.txt   # 79
grep -c '^zhuyin_' /tmp/defined.txt   # 52
# ABSENT check (empty output = none absent):
comm -23 <(grep -ohE '(pinyin|zhuyin)_[a-z_0-9]+' \
    ~/oxpinyin-audit/libpinyin-pin/src/libpinyin.ver \
    ~/oxpinyin-audit/libpinyin-pin/src/libzhuyin.ver | sort -u) /tmp/defined.txt
```

Notes carried from the ledger: 7 oxpinyin-specific `#[no_mangle]` symbols exist
outside the `.ver` sets (`oxpinyin_init_for_fixtures`, `oxpinyin_test_set_user_bigram`,
`oxpinyin_alloc_*`) — localised at link time by the `.ver` `local: *;` and
compiled out under `--features shipped`. Upstream's second
`pinyin_get_character_offset` (`src/pinyin.cpp:3243`) and
`zhuyin_get_character_offset` (`src/zhuyin.cpp:2198`) are `#if 0` dead code (live
defs at `:3193`/`:2148`); `pinyin_get_n_pinyin`/`zhuyin_get_n_zhuyin` are `#if 0`
and unexported — correctly absent from oxpinyin. **Internal-function coverage**
(every upstream function transitively reachable from the 131 exports, mapped to
an oxpinyin counterpart) was **not** enumerated; the ledger covers the 131
exported entry points (the ABI contract), and the internal mapping is a lead.

---

## 8. Open leads deliberately not pursued (with scope reason)

1. **Full `pinyin_option_t` matrix sweep** (`run-option-sweep.sh`, every bit +
   interactions + reserved/invalid bits). Reason: the runner builds a *debug*
   capi (full debug dependency build); the 12 same-data-dir drivers already cover
   the `0x1e`/default surface and rows 17/32 are registered. Phase-2: run the
   sweep at `0x0`/`0x2`/`0x8`/`0x1e`/`0x1c`/`0x14`/`0x82`/`0x88`/`0x188`/`0x18a`.
2. **`pinyin_train(index≠0)` empirical differential** (D2). Reason: needs a
   custom driver training a non-top n-best result and comparing user-store
   writes; all in-tree drivers use `train(inst,0)`. Source-confirmed divergence.
3. **Full per-site assert/abort enumeration + mapping** (N4, 341 sites). Reason:
   the enumeration subagent exceeded the session budget and was interrupted; the
   caller-reachable subset is registered as class (c) (D11).
4. **Per-defect empirical reproduction of the memory-safety catalogue** (N3, 14
   items). Reason: each needs its own C repro against the oracle; the catalogue
   was mapped to classes (b)/(c) from source + register.
5. **Consumer relink + identical runtime** for ibus-libpinyin 1.16.5 (`2d2cdac`)
   and fcitx5 (N6). Reason: no consumer build this session; the pinned-frontend
   relink fixture is the instrument.
6. **libzhuyin behavioural differential** (`run-zhuyin-diff.sh`) (N9). Reason:
   the libzhuyin *ABI* was built and MATCHes (§5.A) but the behavioural drivers
   were not run; row 38's zhuyin train-gate differential is also still owed per
   the register.
7. **On-disk format edge cases** (corrupt/truncated/empty/wrong-version/
   newer-version files; save atomicity; file modes/umask; BDB and KC backends;
   value-level import/export byte-for-byte). Reason: only the tkrzw round-trip
   was run (M6); the rest is governed by the same-backend ruling and the
   `data-formats.md`/`user-store.md` claims, which are claims not evidence.
8. **Header includability matrix** (C/C++ × `-std=c89/c99/c11/c++11` ×
   `-Wall -Wextra -Werror`) and `extern "C"` compile (axis I). Reason: not run;
   headers are byte-identical so the risk is low but unproven.
9. **Complexity differential baseline** (axis L, N7). Reason: no time/space
   baseline captured; the perf findings are repo claims.
10. **Injected-mutation validation for axes A, D, G** (N8). Reason: only axis C
    received an injected mutation; A/D/G non-vacuity was shown by real-detection /
    strict-assertion / DWARF-depth (§3), which is strong but is not the injected
    mutation the mandate prefers.
11. **`Dockerfile.audit` retention.** The trimmed audit container recipe
    (oracle+subject tkrzw pair + abigail-tools + the two build fixes) lives in
    scratch (`~/oxpinyin-audit/Dockerfile.audit`), never committed. Proposed for
    Phase 2: fold the `liblzma-dev`/`zlib1g-dev` fix and an `--enable-libzhuyin`
    oracle cell into `tools/bisection/Dockerfile.perf-matrix` (and fix cell D's
    stale KC label, D15) so the zhuyin ABI and a tkrzw-matched drop-in gate are
    reproducible in CI.

---

## Appendix — exact reproduction commands (this session)

All inside the `oxaudit` container (`podman exec oxaudit …`) built from
`oxpinyin-audit:latest`; oracle at `/opt/oracle-tkrzw`, subject stage at
`/opt/subject-tkrzw/stage`, subject data at `/opt/subject-tkrzw/data`, source at
`/repo` (= HEAD `656ef0b5`).

```sh
# Version scripts (M1): extract global: symbols, set-difference
#   → 79=79, 52=52, empty both directions

# Headers (M2):
diff -q /opt/oracle-tkrzw/include/libpinyin-2.11.92/pinyin.h /repo/crates/oxpinyin-capi/pinyin.h   # identical
#   (and zhuyin.h, novel_types.h, pinyin_custom2.h, zhuyin_custom2.h)

# Shipped subject relink (M3):
bash /repo/tools/packaging/relink-versioned.sh \
  --staticlib /repo/target/x86_64-unknown-linux-gnu/release/libpinyin.a \
  --ver /repo/crates/oxpinyin-capi/libpinyin.ver --soname libpinyin.so.15 \
  --dest /audit/out/subject-shipped/libpinyin.so.15.0.0 --features tkrzw
nm -D --defined-only <oracle.so> | awk '{print $NF}' | sed 's/@.*//' | sort -u   # vs subject → empty comm
abidiff --no-unreferenced-symbols <oracle.so> <shipped-subject.so>               # exit 0

# Drop-in gate (M5/D8):
tools/bisection/run-same-data-dir-diff.sh /opt/oracle-tkrzw/lib/libpinyin.so \
  /opt/subject-tkrzw/stage/usr/lib/x86_64-linux-gnu/libpinyin.so.15.0.0 \
  /opt/oracle-tkrzw/lib/libpinyin/data            # exit 2; 11/12 IDENTICAL, union-diff row 20

# §12 residual (M7/D9):
PINYIN_EXPORT_DIR=/opt/subject-tkrzw/data PINYIN_MODEL_DIR=/repo/target/model20/extracted \
  cargo test --locked --release -p pinyin-oracle --no-default-features --features tkrzw \
  --test sentence_surface_parity -- --include-ignored      # PASS (491/396/390)

# User-dir round-trip (M6):
tools/oracle/user-dir-round-trip.sh /opt/oracle-tkrzw nihao nisha nihaoshijie    # exit 0

# libzhuyin ABI (M4): oracle rebuilt with --enable-libzhuyin; subject zhuyin-capi
#   relinked with libzhuyin.ver → 52=52 versioned, abidiff exit 0
```

Raw artifacts retained this session under `~/oxpinyin-audit/out/` (ephemeral
capture environment; not committed — findings docs commit no captures):
`A-soname.txt`, `A-nm-diff.txt`, `A-versioning.txt`, `A-abidiff-shipped.txt`,
`A-headers.txt`, `A-pc.txt`, `A-abidiff-zhuyin.txt`, `same-data-dir-diff.txt`,
`s12-gate.txt`, `user-dir-roundtrip.txt`, `mutation-check.txt`,
`mutation-check2.txt`, `zhuyin-abi.txt`.
