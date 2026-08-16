# Memory & Access Safety Bugs in libpinyin / ibus-libpinyin

A curated reference of bugs that are primarily caused by (or closely related to) **memory management**, **null / invalid access**, **resource lifetime**, **alignment**, or **use-after-free** issues.

These are the class of defects that a careful Rust rewrite is expected to eliminate or make significantly harder by construction (ownership, `Option`/`Result`, RAII, borrow checker, no unchecked pointer arithmetic, etc.).

**Out of scope** (intentionally excluded):

- Compiler version / C++ standard / MinGW portability issues
- Pure behavioural / UX issues (e.g. “baidu.com” punctuation stripping, mode-switch quirks, Wayland candidate positioning, missing features)

Sources: upstream GitHub issues, Red Hat ABRT, SUSE Bugzilla, Launchpad, commit history, and distro crash reports (as of mid-2026).

---

## 1. Null-pointer / Invalid Access (SIGSEGV)

### 1.1 Segfault in `selectCandidate()` when `pinyin_get_pinyin_key_rest()` returns NULL

| Field | Value |
|-------|-------|
| **Tracker** | [ibus-libpinyin #566](https://github.com/libpinyin/ibus-libpinyin/issues/566) |
| **Also seen** | Red Hat Bugzilla 2476461 |
| **Component** | ibus-libpinyin (`PYPLibPinyinCandidates.cc`) + libpinyin (`pinyin.cpp`) |
| **Status** | Open (reported 2026) |
| **Trigger** | Type a pinyin string that ends with an invalid syllable (e.g. `nih`). Select a candidate for the valid prefix (`ni`). |
| **Root cause** | `pinyin_get_pinyin_key_rest()` returns `false` and leaves `pos = NULL`. Code then calls `pinyin_get_pinyin_key_rest_positions(instance, pos, ...)` which dereferences `pos->m_raw_begin`. |
| **Simplified crash path** | ```cpp<br>PinyinKeyPos *pos = NULL;<br>pinyin_get_pinyin_key_rest(instance, lookup_cursor, &pos); // fails → pos stays NULL<br>pinyin_get_pinyin_key_rest_positions(instance, pos, &begin, NULL); // SIGSEGV<br>``` |
| **Why Rust helps** | `Option<&PinyinKeyPos>` (or equivalent) forces the null case to be handled; no unchecked dereference possible. |

### 1.2 Random SIGSEGVs in candidate processing

| Field | Value |
|-------|-------|
| **Trackers** | Launchpad 2008451, SUSE 1257531, multiple ABRT reports |
| **Component** | ibus-libpinyin (`EnglishCandidates::processCandidates`, `EnhancedCandidate` vector operations, `PhoneticEditor::updateCandidates`) |
| **Symptoms** | Crash after screen lock, mode switch, or during normal candidate update. Backtraces frequently show invalid memory access inside `std::vector` reallocation or iterator use. |
| **Why Rust helps** | Bounds-checked containers + ownership of candidate lists prevent use of invalid iterators / dangling references. |

### 1.3 Historical `pinyin_save` SIGSEGV

| Field | Value |
|-------|-------|
| **Trackers** | Red Hat 1689745 and duplicates |
| **Component** | libpinyin / ibus-libpinyin |
| **Symptoms** | Crash in save path under certain GSettings / config-change races. |
| **Status** | Fixed years ago |
| **Why Rust helps** | Clearer lifetime of user-data structures; no manual pointer management across async/config callbacks. |

---

## 2. Memory / Resource Leaks

### 2.1 Async cloud-request `user_data` leak

| Field | Value |
|-------|-------|
| **Source** | Commit “Avoid memory leak of user_data” (czxdev, ~2025-04) |
| **File** | `src/PYPCloudCandidates.cc` |
| **Root cause** | `g_timeout_add` without a proper destroy-notify in some cancellation / superseding-timer paths. |
| **Fix applied** | Switched to `g_timeout_add_full` + explicit `releaseUserData` destroy function. |
| **Why Rust helps** | Ownership + drop glue (or `Box` + async cancel) makes the leak class a non-issue. |

### 2.2 `FILE*` / path resource leak in table import/export

| Field | Value |
|-------|-------|
| **Source** | Commit “Fix resource leak” (Peng Wu, 2024-09) |
| **File** | `src/PYTableDatabase.cc` |
| **Root cause** | Early `return FALSE` paths after `fopen` / `sqlite3_prepare` did not always `fclose` / `g_free`. |
| **Why Rust helps** | RAII (`std::fs::File`, owned `String`) guarantees cleanup on every exit path. |

### 2.3 libpinyin internal memory leaks

| Field | Value |
|-------|-------|
| **Source** | libpinyin 2.10.2 release notes (“fix memory leaks”); older commits in `export_interpolation.cpp` etc. |
| **Symptoms** | Leaked `SingleGram` objects, `GArray`s, and similar in data-generation tools. |
| **Why Rust helps** | Ownership model eliminates the majority of manual `delete` / `g_free` mistakes. |

### 2.4 High / unbounded memory growth reports

| Field | Value |
|-------|-------|
| **Source** | openSUSE forum reports, user observations |
| **Symptoms** | `ibus-engine-libpinyin` growing to multiple GB under prolonged use (sometimes correlated with cloud input or desktop extensions). |
| **Note** | Not always a pure leak (could be cache growth), but the absence of ownership boundaries makes diagnosis and prevention harder. |

---

## 3. Use-After-Free

### 3.1 Temporary UAF in English mode

| Field | Value |
|-------|-------|
| **Tracker** | Red Hat 2359375 |
| **Component** | ibus-libpinyin |
| **Symptoms** | After a Coverity-driven change, a use-after-free caused random files to appear in `$HOME` after re-login. |
| **Status** | Fixed in the 1.16.1 timeframe |
| **Why Rust helps** | The borrow checker turns this entire class of bug into a compile-time error. |

---

## 4. Unaligned / Architecture-Specific Access

### 4.1 Segmentation fault on 32-bit x86 during binary generation

| Field | Value |
|-------|-------|
| **Tracker** | [libpinyin #120](https://github.com/libpinyin/libpinyin/issues/120) |
| **Component** | libpinyin (`gen_binary_files` / bigram generation) |
| **Symptoms** | Consistent SIGSEGV on i686 (Alpine + GCC 9.x reported). |
| **Status** | Closed |
| **Why Rust helps** | Explicit layout control (`#[repr(C)]`, alignment attributes) and safer binary data handling reduce the chance of silent misalignment. |

### 4.2 Bus error (unaligned access) on sparc64

| Field | Value |
|-------|-------|
| **Tracker** | [libpinyin #170](https://github.com/libpinyin/libpinyin/issues/170) / Debian #889596 |
| **Component** | libpinyin (`gen_unigram`) |
| **Symptoms** | Bus error while generating unigram data on sparc64. |
| **Status** | Fixed (libpinyin 2.10.3 era) |
| **Why Rust helps** | Same as above; alignment violations become harder to introduce accidentally. |

---

## 5. Resource / Lock-File Access Hangs

### 5.1 Hang in `PhraseLargeTable3::store_db` / `save_db`

| Field | Value |
|-------|-------|
| **Tracker** | [libpinyin #179](https://github.com/libpinyin/libpinyin/issues/179) |
| **Component** | libpinyin (Berkeley DB backend) |
| **Symptoms** | Engine freezes forever. Leftover `__db.user_phrase_index.bin.tmp` lock files (from previous crash/kill) cause Berkeley DB to wait for a non-existent lock holder. |
| **Current behaviour** | Code only tries to remove the non-`__db` temporary name. |
| **Status** | Open (reported 2026) |
| **Why Rust helps** | Better temp-file & lock hygiene is natural with crates such as `tempfile`; or the store can be replaced with a pure-Rust embedded database that does not leave stale lock files. |

---

## 6. Assert / Brittle Internal Invariants

### 6.1 Crash on specific input (`__assert_perror_fail`)

| Field | Value |
|-------|-------|
| **Tracker** | [ibus-libpinyin #542](https://github.com/libpinyin/ibus-libpinyin/issues/542) |
| **Component** | ibus-libpinyin |
| **Symptoms** | Typing certain syllables (e.g. `zhuan`) on Fedora 43 / 1.16.5 triggers an assertion failure and process exit. |
| **Status** | Closed |
| **Why Rust helps** | Prefer `Result` / recoverable error paths over `assert` on user-controlled input. |

### 6.2 Cloud input + system proxy segfaults

| Field | Value |
|-------|-------|
| **Tracker** | [ibus-libpinyin #518](https://github.com/libpinyin/ibus-libpinyin/issues/518) |
| **Component** | ibus-libpinyin cloud path + libsoup |
| **Symptoms** | Segfault (sometimes landing inside libsoup) when cloud input is enabled together with certain system proxies (Clash / Mihomo). |
| **Related fixes** | “Fix segmentation fault in processCloudResponse”, cancellation / message lifetime hardening. |
| **Why Rust helps** | Clearer ownership of async messages, streams and cancellation tokens. |

---

## Summary – Why these bugs benefit from a Rust rewrite

| Bug class | Typical C/C++ failure mode | Rust mitigation |
|-----------|----------------------------|-----------------|
| Null / invalid access | Missing check after fallible API | `Option` / `Result` force handling |
| Memory / resource leaks | Forgotten `g_free` / `fclose` / `delete` on early return | RAII + ownership |
| Use-after-free | Manual lifetime across callbacks / async | Borrow checker |
| Unaligned access | Packed binary structures, architecture assumptions | Explicit layout + safer data handling |
| Stale lock / resource hangs | Incomplete cleanup of external DB lock files | Better temp/lock APIs or pure-Rust store |
| Assert on user input | Brittle internal invariants | Recoverable error types |

---

## 7. Eliminated by construction — a precise accounting

"Rust prevents this" is three different claims, and conflating them is how
rewrite projects end up making promises their first release breaks. The
tiers below sort every catalogued bug by *which mechanism* actually holds
the guarantee in oxpinyin — the type system, the ownership model with its
named escape hatches, or policy-plus-tests where the language holds
nothing at all. Scope note: Tier A guarantees apply to the
`#![forbid(unsafe_code)]` core crates; `pinyin-capi` is the one surface
where C-shaped risk re-enters, governed by contract (`// SAFETY:` per
block, ASan across the FFI, the NULL-tolerance template rule below)
rather than by the compiler.

### Tier A — unrepresentable in safe Rust

- **Null dereference** (1.1, parts of 1.2, 1.3): fallible lookups return
  `Option`/`Result`; the deref-of-NULL cannot be written. *Stage-1
  nuance:* the borrowed C++ frontend still performs the upstream call
  sequence and will pass NULL into our shim — bug 1.1 is prevented at
  `pinyin-capi` by contract (validate, return the upstream error
  convention), not by the borrow checker. The language protects the
  core; the contract protects the seam.
- **Use-after-free** (3.1): lifetimes make the dangling reference a
  compile error. Full stop.
- **Iterator/reallocation UB** (1.2): owned candidate lists and
  bounds-checked indexing make invalidated-iterator UB inexpressible.
  (A *logically stale* snapshot remains possible — that is a correctness
  question for session-replay tests, not a memory-safety one.)
- **Unaligned-access UB** (4.1, 4.2): safe Rust cannot perform a
  misaligned dereference; packed on-disk data goes through explicit byte
  parsing, so the failure mode becomes a checked parse error, never a
  bus error.
- **Early-return resource leaks** (2.2, 2.3): `Drop` runs on every exit
  path; the forgot-to-`fclose`-before-`return` pattern is unwritable.

### Tier B — structurally prevented, escape hatches named

- **Async lifetime leaks** (2.1): ownership plus cancel-on-`Drop` erases
  the `g_timeout` destroy-notify class — but Rust async has its own
  hatch: a detached task whose `JoinHandle` is dropped leaks by design.
  Design rule: no fire-and-forget; every spawned task is owned or
  aborted.
- **Deliberate-leak APIs** (`Box::leak`, `mem::forget`, `Rc` cycles)
  exist on purpose. The core crates have no use for them, and their
  appearance in review is treated as a defect requiring written
  justification.

Tier B is honest about the hatch: the language closes the accidental
path and leaves the deliberate one; policy closes the remainder.

### Tier C — the language does not prevent these; policy, architecture and tests do

- **Panic on user input** (6.1): `assert!` and `unwrap` abort exactly
  like `__assert_perror_fail`. Prevention is constitution rule 4 (every
  public API returns `Result`; a panic is a defect of the same severity
  as data loss), enforced by proptest totality, cargo-fuzz from W1, and
  Kani bounded proofs. This tier exists in writing because libchewing —
  a competent Rust rewrite by the original author — still shipped
  "panic when selecting phrases backwards at the end of buffer" in
  v0.8.0. Memory safety eliminates one class of crash, not crashes.
- **Stale-lock hangs** (5.1): no borrow checker deletes a leftover
  `__db.*` file. Eliminated by architecture — redb keeps state in a
  single file with no sidecar lock artefacts — and locked by the
  hard-kill gate (terminate mid-write, reopen, integrity asserted).
- **Unbounded growth** (2.4): caches grow in any language. Bounded-cache
  policy plus published memory figures; ownership makes the accounting
  *legible*, not automatic.
- **Foreign-library interop crashes** (6.2): shifted, not solved. Any
  future cloud path uses a Rust HTTP stack behind the opt-in provider
  trait; where FFI remains (capi, oracle), ASan/LSan across the boundary
  is the guard, because Miri cannot follow.

### Per-bug verdict

| Bug | Tier | Where the guarantee lives | Locked by |
|---|---|---|---|
| 1.1 #566 null key-rest | A (core) / contract (seam) | `Option`-returning lookups; capi NULL-tolerance rule | F-E fixture (`nih` + select); per-symbol oracle test |
| 1.2 candidate SIGSEGVs | A | owned candidate lists, bounds checks | session replay (F-D) + fuzz |
| 1.3 save-path race | A/B | ownership across config callbacks | hard-kill + replay |
| 2.1 async `user_data` | B | task ownership, cancel-on-`Drop`, no detached spawns | review rule; Stage-2 provider tests |
| 2.2 `FILE*`/path leaks | A | `Drop` on every path | — (pattern unwritable) |
| 2.3 tool leaks | A | same | — |
| 2.4 multi-GB growth | C | bounded caches | memory figures on the bench page |
| 3.1 English-mode UAF | A | borrow checker | — (compile error) |
| 4.1 i686 segfault | A | checked byte parsing of tables | loader fixture cross-check (W3-T0) |
| 4.2 sparc64 bus error | A | same | 3-OS + advisory-target CI |
| 5.1 BDB lock hang | C | redb single-file store, no sidecar locks | hard-kill gate (W6-T3) |
| 6.1 assert on `zhuan` | C | rule 4: `Result`, never `assert` on input | proptest, fuzz, Kani; F-E |
| 6.2 proxy/libsoup | C | Rust stack for any cloud; FFI audited | ASan jobs (verification stack) |

### Claims discipline

Every Tier A and C row is claimable only as a scoped, reproducible
demonstration — "on input `nih` + candidate select, upstream
`<pinned ref>` crashes (ibus-libpinyin #566); oxpinyin returns an error
and continues" — never as a blanket "memory-safe, therefore crash-free".
Tier C is the standing proof the blanket form would be false.
Operationally: each repro above is registered in the cross-lane evidence family
**F-E** at foundation time; reproducible evidence is attached when the
applicable lane and tooling exist. Upstream-reproducible bugs remain filed
upstream regardless of our fix; and the open, user-visible ones (#566 leading,
#179's hang second) form the demonstration set for the first typing-session
milestone.

The beauty is real. The accounting is what keeps it honest.

---

## Suggested repository layout

```text
reference/
├── memory-safety-bugs.md          # this file
└── memory-safety-bugs/
    ├── 566-null-key-rest.md       # deeper write-up + minimal repro
    ├── 179-berkeley-db-lock.md
    ├── cloud-async-lifetime.md
    └── ...
```

This catalogue is intended as living reference material for the Rust rewrite effort. New memory/access bugs should be added here with the same structure.
