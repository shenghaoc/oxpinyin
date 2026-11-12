# F-E cross-lane robustness evidence register

Date: 2026-08-09 · Status: all 14 cases registered

This register maps each scoped claim to a reproducible trigger and a named
passing artifact. The claims are rows F-E-01–F-E-13 of
`reference/memory-safety-bugs.md` and the W2-T3 oracle abort row F-E-14.

A `registered` entry is evidence-ready. It may not be executable yet,
because its owning crate or lane belongs to later work. It does not count
as a working exploit or a passing regression until the named artifact
exists.

Oracle-backed entries use the pin ref frozen in
`docs/testing/capture-fixtures.md`. Each claim is limited to its listed
trigger. No entry supports a general claim that Rust code cannot crash.

## Summary

| ID | Trigger or invariant | Owner | State at Foundation |
|---|---|---|---|
| F-E-01 | `nih`, then select valid `ni` prefix | C API oracle test | captured seed; regression registered |
| F-E-02 | stale/invalid candidate access | C ABI choose + fuzz | registered |
| F-E-03 | interruption during save/config change | user store hard-kill + replay | registered |
| F-E-04 | cancelled/superseded async request | provider ownership tests | registered |
| F-E-05 | table I/O early return | none: test not written, `oxpinyin-migrate` is deleted | registered |
| F-E-06 | data-tool early return | none: test not written, `oxpinyin-dictool` is deleted | registered |
| F-E-07 | prolonged unique workload | bounded-cache benchmark | registered |
| F-E-08 | English-mode dangling lifetime | compile-time ownership | registered |
| F-E-09 | i686 table generation/load | loader cross-check | registered |
| F-E-10 | strict-alignment table load | checked parser + advisory CI | registered |
| F-E-11 | stale Berkeley DB sidecar lock | user-store hard-kill gate | legacy trigger registered |
| F-E-12 | `zhuan` user input | parser totality + fuzz | F-A seed; proptest + cargo-fuzz |
| F-E-13 | cloud request through system proxy | provider/FFI ASan | registered |
| F-E-14 | lone apostrophe `'` (oracle abort) | capi parse and abort tests | registered; same root cause as #570 |

## Evidence entries

### F-E-01 — #566 NULL key-rest

- **Source evidence:** `reference/memory-safety-bugs.md` §1.1 and
  [ibus-libpinyin #566](https://github.com/libpinyin/ibus-libpinyin/issues/566).
- **Trigger:** fresh state; parse `nih`; expose candidates for the valid `ni`
  prefix; select one while the trailing `h` remains.
- **Foundation artifact:** F-A case `incomplete-nih` records `ni@0:2:complete`
  and `h@2:3:partial` at the frozen oracle pin.
- **Passing artifact:** the exact C API/oracle regression
  `f_e_01_null_key_rest`. It must:
  - validate a failed or missing key-rest before use;
  - return the ABI error convention;
  - keep the process usable for a second request.

### F-E-02 — candidate-processing invalid access

- **Source evidence:** `reference/memory-safety-bugs.md` §1.2.
- **Trigger:** create a candidate snapshot. Regenerate candidates after an
  input change. Then choose with a candidate pointer from the stale snapshot
  that the regenerated list no longer holds.
- **Production path:** the C ABI choose. A choose names a `lookup_candidate_t`
  pointer, which `pinyin_choose_candidate` and `zhuyin_choose_candidate`
  resolve by address against the instance's current snapshot. The session
  has no index-based selection: the engine index replay
  (`f_e_02_candidate_replay`) was retired with `Session::select`, and
  `CandidateList::get`, which had only test callers, was deleted with it.
- **Reproduction command:** `cargo nextest run -p oxpinyin-capi -p
  oxpinyin-zhuyin-capi a_stale_candidate_pointer_after_regeneration_is_refused`
  (`oxpinyin_capi::e2e_tests` and `oxpinyin_zhuyin_capi::e2e_tests`).
  Seeding the same sequence into the `capi_commands` fuzz target is open.
- **Passing artifact:** a stale pointer whose address holds no current row
  answers `-1` on both facades, the composition is unchanged, and a second
  request chooses a live row. The snapshot keeps its buffer across a
  rebuild, so a stale pointer to a slot the new list still fills addresses
  that new row, as the pin's reused `GArray` slot does. No fuzz corpus seed
  is retained yet.

### F-E-03 — historical save-path race

- **Source evidence:** `reference/memory-safety-bugs.md` §1.3.
- **Upstream-path trigger:** in the frontend compatibility lane, synchronize a
  configuration callback with `pinyin_save` at a barrier. Run the interleave
  under TSAN. This is the scoped reproduction of the cited callback race.
- **Replacement invariant:** as a separate test, kill the Rust user-store
  process at each persisted write boundary, reopen, and replay the last
  input.
- **Passing artifacts:**
  - frontend test `f_e_03_config_save_race` reports no race or invalid
    lifetime;
  - user-store test `f_e_03_hard_kill_replay` reopens within timeout and
    exposes no partially committed generation.

### F-E-04 — asynchronous cloud `user_data` lifetime leak

- **Source evidence:** `reference/memory-safety-bugs.md` §2.1.
- **Trigger:** repeatedly supersede and cancel an in-flight provider request,
  then drop the session before completion.
- **Reproduction command:** provider test `f_e_04_owned_task_cancellation`
  under the leak-checking job when an async provider exists.
- **Passing artifact:** every task handle is owned, and is joined or aborted
  on drop. Completion and cancellation counters balance. A detached task is
  a review failure.

### F-E-05 — `FILE*`/path early-return leak

- **Source evidence:** `reference/memory-safety-bugs.md` §2.2.
- **Trigger:** import/export a missing, truncated, permission-denied, and
  malformed table repeatedly while sampling open descriptors.
- **Reproduction command:** data/migration test `f_e_05_table_io_raii`
  (not written; the `oxpinyin-migrate` crate is deleted).
- **Passing artifact:** each operation returns `Err`. The descriptor count
  returns to baseline after every iteration. Shipping paths use owned Rust
  files and paths, not raw `FILE*`.

### F-E-06 — libpinyin data-tool leaks

- **Source evidence:** `reference/memory-safety-bugs.md` §2.3.
- **Trigger:** run every dictool conversion against valid, truncated, and
  malformed input in a loop under the leak-checking job.
- **Reproduction command:** dictool test `f_e_06_tool_raii` (not written; the
  `oxpinyin-dictool` crate is deleted) plus the same
  corpus under ASan/LSan if any foreign parser remains.
- **Passing artifact:** deterministic `Err` output and stable live allocation
  and descriptor counts after warm-up.

### F-E-07 — high or unbounded memory growth

- **Source evidence:** `reference/memory-safety-bugs.md` §2.4.
- **Trigger:** configure a 4,096-entry cache. Process one million unique
  32-byte keys with 128-byte values. Then repeat the first 4,096 keys.
- **Reproduction command:** Linux benchmark `f_e_07_bounded_cache`, sampled
  from `/proc/self/status` after warm-up and after each 10,000 operations.
- **Passing artifact:** the entry count never exceeds 4,096. Peak
  post-warm-up RSS is at most baseline plus 64 MiB. The bench page records
  toolchain, allocator, kernel and raw samples. Other platforms are
  advisory.

### F-E-08 — English-mode use-after-free

- **Source evidence:** `reference/memory-safety-bugs.md` §3.1.
- **Trigger:** construct a mode result. Replace or drop the originating
  session state. Keep the result for later rendering.
- **Reproduction command:** compile-time ownership test
  `f_e_08_owned_mode_result` plus normal mode-switch replay.
- **Passing artifact:** the result owns its render data or is lifetime-bound to
  the session. A dangling borrowing construction does not type-check.

### F-E-09 — i686 binary-generation invalid access

- **Source evidence:** `reference/memory-safety-bugs.md` §4.1 and
  [libpinyin #120](https://github.com/libpinyin/libpinyin/issues/120).
- **Scope:** the cited upstream failure occurs during binary generation. The
  Rust lane does not reproduce that generator. It proves the replacement
  loader invariant against the generated format.
- **Trigger:** load the same frozen little-endian table fixture on x86_64 and
  i686. Compare decoded records, and errors for truncated offsets.
- **Reproduction command:** loader test `f_e_09_i686_cross_check` in the
  advisory 32-bit lane.
- **Passing artifact:** identical logical output on both targets. Malformed
  offsets return a checked error, with no pointer casts or unchecked
  indexing.

### F-E-10 — sparc64 unaligned access

- **Source evidence:** `reference/memory-safety-bugs.md` §4.2 and
  [libpinyin #170](https://github.com/libpinyin/libpinyin/issues/170).
- **Scope:** the cited upstream bus error occurs in unigram generation. The
  applicable Rust evidence is a checked replacement parser, not a claim that
  the historical generator path was reproduced.
- **Trigger:** decode fixtures whose multi-byte fields begin at every byte
  alignment, including truncation at each field boundary.
- **Reproduction command:** loader test `f_e_10_unaligned_bytes` on the normal
  lane and advisory strict-alignment/sparc64 lane when available.
- **Passing artifact:** byte-wise checked decoding matches the aligned
  fixture, and every truncation returns `Err`. No reference is formed from
  packed data.

### F-E-11 — #179 stale Berkeley DB lock

- **Source evidence:** `reference/memory-safety-bugs.md` §5.1 and
  [libpinyin #179](https://github.com/libpinyin/libpinyin/issues/179).
- **Legacy trigger:** kill a Berkeley DB writer, keep its `__db.*` sidecars,
  and reopen. This is an upstream demonstration only. Berkeley DB is a supported
  backend of `oxpinyin-store`; tkrzw is the default.
- **Reproduction command:** `cargo test -p oxpinyin-user
  f_e_11_hard_kill_reopen -- --exact` (not written).
- **Passing artifact:** hard-kill/reopen completes within timeout with a valid
  store and no `__db.*` sidecar files.

### F-E-12 — #542 assertion on `zhuan`

- **Source evidence:** `reference/memory-safety-bugs.md` §6.1 and
  [ibus-libpinyin #542](https://github.com/libpinyin/ibus-libpinyin/issues/542).
- **Foundation trigger:** parse `zhuan` in fresh state and continue with a
  second parse. This proves only backend parser totality. It does not
  reproduce the cited frontend/session assertion path.
- **Foundation artifact:** F-A case `robustness-zhuan` records a complete
  `zhuan@0:5:complete` parse at the frozen oracle pin.
- **Passing artifacts:** F-A case `robustness-zhuan`, the proptest totality
  property (`arbitrary_bytes_never_panic`) and the cargo-fuzz determinism
  target. When the session lane exists, `f_e_12_zhuan_session_replay` must
  exercise the full frontend-equivalent sequence before any session-level
  claim is made.

### F-E-13 — #518 cloud/proxy foreign-library crash

- **Source evidence:** `reference/memory-safety-bugs.md` §6.2 and
  [ibus-libpinyin #518](https://github.com/libpinyin/ibus-libpinyin/issues/518).
- **Trigger:** cancel and replace cloud requests through a deterministic local
  HTTP proxy while responses arrive before, during, and after session drop.
- **Reproduction command:** provider test `f_e_13_proxy_lifetime` under ASan
  for every remaining FFI boundary. The canonical Foundation oracle build has
  cloud input disabled, so this entry claims no cloud exploit.
- **Passing artifact:** all request, message and cancellation ownership is
  balanced. The session stays usable. ASan reports no boundary violation.

### F-E-14 — pinned oracle aborts on apostrophe-only input

- **Source evidence:** live measurement on the pin (recorded evidence). It
  is the same class as F-E-12 (`assert()` on user input) on a different
  path. It is not a catalogue row in `reference/memory-safety-bugs.md`. It
  has the same root cause as
  [ibus-libpinyin #570](https://github.com/libpinyin/ibus-libpinyin/issues/570)
  (reported 2026-08-06 through the frontend path). The API-level repro
  (lone apostrophe) is a simpler trigger.
- **Trigger:** use a fresh pin-built oracle. Parse a lone `'` (also `''`,
  `'''`). Call `pinyin_get_pinyin_key(instance, 0)`. The parse and
  `pinyin_get_parsed_input_length` answer the input length. The matrix has
  no key column, so the key query asserts and the process aborts. A
  lowercase letter in the parsed prefix gives a key column, so `ni'`, `'ni`,
  `a'a` and `ni''hao` do not abort.
- **One-character API repro:** `'` → `assert()` → `abort()`.
- **Passing artifact:** oxpinyin reproduces the pin's parse and abort.
  `crates/oxpinyin-capi/src/parse.rs` holds
  `an_apostrophe_only_parse_matches_the_pin` (parse and parsed lengths of
  `'`, `''`, `'''`) and `a_key_query_on_an_apostrophe_only_parse_aborts`
  (pin observations `key-abort/'`, `key-abort/''`, `key-abort/'''`).
  `crates/oxpinyin-zhuyin-capi/src/parse.rs` holds the zhuyin parse. The
  three apostrophe-only inputs were in stratum `09-edge` of the retired W2 corpus.
