# Upstream IME test-strategy study — libpinyin and libchewing

Study input requested during the safety-profile review: how the two closest
upstream projects structure tests and fuzzing, and what oxpinyin should
import. Sources inspected (2026-08-27):

- `github.com/libpinyin/libpinyin` — `tests/` (and its CI: `cmake.yml`,
  `make-check.yml`)
- `codeberg.org/chewing/libchewing` — `tests/` and `fuzzer/` (and its CI)

## libpinyin `tests/`

Layout: four top-level interactive drivers (`test_pinyin.cpp`,
`test_zhuyin.cpp`, `test_chewing.cpp`, `test_phrase.cpp`) — stdin-driven
REPLs ("prefix:" / "pinyin:" prompts, candidates printed to stdout), i.e.
manual oracle tools rather than assertions; `tests_helper.h` provides
`load_phrase_index` and `check_result`; and two real unit-test directories:

- `storage/` (12 tests, closest analogue to oxpinyin's data/user/store):
  `test_ngram.cpp`, `test_matrix.cpp`, `test_flexible_ngram.cpp`,
  `test_phrase_index.cpp`, `test_phrase_index_logger.cpp`,
  `test_phrase_table.cpp`, `test_punct_table.cpp`, `test_table_info.cpp`,
  `test_chewing_table.cpp`, `test_parser2.cpp`, … Style: imperative
  scenario scripts with raw `assert()` plus printf output a human is
  expected to eyeball (e.g. bigram store/load/search roundtrips in
  `test_ngram.cpp` assert only some outcomes and print the rest).
- `lookup/`: `test_phrase_lookup.cpp`, `test_pinyin_lookup.cpp` (lookup
  path scenarios over a loaded phrase index).

CI: `make check` runs these under both autotools and CMake builds; nothing
else (no fuzzing, no sanitizers in-tree).

**What oxpinyin already does better**: assertions instead of eyeballing,
the pin-built oracle as a *differential* oracle, fixtures with hashes. The
importable artifact is the **coverage checklist**: libpinyin unit-tests each
storage class in isolation (ngram, matrix, phrase index/table, punct,
table-info, parser). Mapped onto oxpinyin that is: `user/store.rs` bigram
merge paths, `data/lm` ngram math, `data/dict`+`table` phrase storage,
`data/punct`, `data/table_conf`, `core/parser`. Cross-checking against the
complexity audit: the `user/store.rs` quartet (CCN 38/33/31/22) is exactly
where libpinyin has the most granular per-storage tests and oxpinyin has
the fewest per-function ones — that correlation drives the coverage/mutation
priority in `ci-strategy.md`.

## libchewing `tests/`

The most instructive of the three directories. Components:

- **`testchewing.c` + `materials.txt`** — a keystroke-script interpreter
  (escape syntax like `<E>`, `<D>`, `<C1>`) replayed against a real session,
  printing commit strings; `materials.txt` is a golden table of
  `keystrokes → expected Chinese output` drawn from real-world sentences.
  A human/script diff closes the loop (`simulate.sh`).
- **`test-regression.c`** — one minimal repro function per historical bug,
  named for its tracker issue (`test_libchewing_googlecode_issue_472` …).
  Most just assert "no longer crashes" on a gnarly keystroke string. This
  is where every fuzz/stress finding gets parked forever.
- **`randkeystroke.c` / `genkeystroke.c`** — keystroke generators with two
  modes: domain-plausible (bopomofo key tables weighted by phonetics) and
  `-r` totally random; `-s` seeds, `-n` counts. Output feeds
  `testchewing`/`simulate` for unattended soaks.
- **`stress.c` / `stresstest.py` / `simulate.c`** — long-running stress and
  valgrind-wrapped simulation.
- **Per-feature unit tests** — `test-bopomofo`, `test-config`,
  `test-error-handling`, `test-fullshape`, `test-keyboard`,
  `test-special-symbol`, `test-symbol`, `test-userphrase`, `test-reset`,
  `test-logger`, and notably **`test-struct-size.c`** — an ABI guard that
  pins public struct sizes/offsets.

**Imports for oxpinyin**:

1. **Issue-named regression convention** (adopt formally): every crash,
   fuzz finding, or parity bug becomes a test named for its PR/issue and is
   never deleted. oxpinyin's recent review-driven fixes (#176 anchor/window
   fixes) are exactly the candidate class; make the convention explicit in
   the profile rather than folklore.
2. **Domain-plausible seed generation**: oxpinyin's parity corpus tools
   already emit realistic pinyin streams — wire them as fuzz corpus seeds
   (the `-r` random mode remains libFuzzer's job).
3. **ABI guard precedent** (`test-struct-size.c`) confirms oxpinyin's
   checked-in `pinyin.h` + C++ smoke gate is the standard trick; nothing
   new needed.

## libchewing `fuzzer/` — the key import

A Rust workspace member crate (`fuzzer`, plain `[[bin]]`s, AFL++ via
`cargo afl`, not cargo-fuzz) with harnesses:

Toolchain note: the AFL++ dependency itself is expected from the
distribution, not crates.io — the README installs
`sudo dnf install american-fuzzy-lop`
([Fedora package](https://packages.fedoraproject.org/pkgs/american-fuzzy-lop/american-fuzzy-lop/))
or `sudo apt install afl++`, and only `cargo-afl` (the cargo wrapper) comes
from `cargo install`. For oxpinyin the analogous stance is already in force:
the fuzz job pins `cargo-fuzz 0.13.2 --locked` on a pinned nightly; if an
AFL++ lane were ever added, prefer the distro package + version-pinned
`cargo-afl` over ad-hoc builds, and keep it out of the default toolchain.

- **`fuzzer.rs` (453 lines)**: a *stateful command interpreter over the C
  API*. Each input byte maps (`value % 25`) to one of 25 "handles" — keys
  (space, arrows, home/end, page keys) *and config mutators*
  (`set_maxChiSymbolLen`, `set_autoShiftCur`, KB-type switch, mode
  toggles). The loop drives `chewing_new2` … key … enumerate candidates …
  `chewing_delete`, i.e. a whole-session fuzzer through `unsafe` FFI, with
  a `GEN` mode to dump repro keystroke scripts. Runs against an in-memory
  user DB (`:memory:`).
- **`trieloader.rs`**: treats the input *file* as a hostile dictionary —
  load as trie (plus a CDB variant), query metadata, look up phrases. This
  is precisely the "untrusted data file" boundary.
- README documents AFL++ usage, seed dirs, per-harness args; fuzzing is a
  local/manual activity, not a CI gate.

**Why this matters for oxpinyin**: the current single fuzz target covers
the safe parser only. The libchewing precedent shows the two harness shapes
with the best historical yield for an IME:

1. **`capi-commands`** — a stateful session fuzzer through
   `oxpinyin-capi`'s own 55-symbol ABI (bytes → `process_key`-equivalents,
   guess, candidate walks, config setters, iterator begin/end pairings in
   adversarial orders — double-`end` and use-after-`end` are exactly the
   trust boundary documented in `oxpinyin-audit.md` F-6). This exercises
   the FFI conversion/ownership layer that the parser target cannot
   reach. Implementation note: lives in the existing nightly `fuzz/`
   workspace (Linux-only matches the CI fuzz job); the harness itself is
   the one place where fuzz code legitimately links capi. Sanitizers:
   the pinned cargo-fuzz 0.13.2 builds targets with its default
   instrumentation, selected as `-s address` (Sanitizer::Address) — the
   Rust-side spelling of what lands in RUSTFLAGS — and on Linux the
   AddressSanitizer build includes LeakSanitizer by default, so leaks and
   heap errors surface without extra flags. Native FFI coverage is
   **planned, not current**: it exists only once this target and its CI
   command land — today's `parser` target never links the C ABI.
   
   Semantic postconditions to assert per command, **for valid command
   sequences only** (define before implementing; reuse the contract
   expectations in `crates/oxpinyin-capi/src/contract_tests.rs`, which
   already pins the scheme-setter behavior):
   - every entry point returns, never aborts (ffi_catch turns panics into
     `false`/`NULL` fallbacks — an abort is a finding);
   - rejected config setters return `false` **and leave instance state
     unchanged** (query a getter before/after to compare);
   - candidate walks after a rejected/failed guess return the pre-failure
     snapshot or an empty list, never garbage pointers;
   - fallback parsing: junk bytes into `pinyin_parse_more_*` return the
     parsed-length accounting (0 for nothing consumed) and leave the
     preedit consistent with `pinyin_get_parsed_input_length`.
   
   Double-`end` and use-after-`end` are different: the contract makes
   them UB (the audit's F-6 trust surface), so they are **intentional
   negative tests**, not postcondition subjects — drive them and judge
   the outcome through sanitizer observations (any ASan/LSan report is a
   finding; a clean run under sanitizers is the best available signal,
   not a proof). Changing the API to require safe fallbacks for those
   sequences would be a behavioral change to the pinned ABI and is out
   of scope unless explicitly taken.
2. **`dict-loader`** — bytes as a `.text`/`.bin` table through
   `oxpinyin-data`'s decode path (the F-3 class), the trieloader
   translated to oxpinyin's formats.

Both were already proposed in `tooling-evaluation.md` §16 on trust-boundary
grounds; the libchewing study adds empirical weight (these are the shapes a
decade of IME fuzzing converged on) and the config-mutation trick (fold
`pinyin_set_*` setters into the command alphabet).

## Synthesis

| Practice | libpinyin | libchewing | oxpinyin today | oxpinyin action |
|---|---|---|---|---|
| Golden keystroke→output table | — | materials.txt | parity fixtures + oracle differential (stronger) | none — keep |
| Per-storage-class unit tests | yes (12) | partial | partial | coverage priority on user/store quartet |
| Issue-named regression tests | — | yes | informal | adopt convention explicitly |
| Random keystroke soak | — | randkeystroke + simulate | 10s libFuzzer smoke | nightly soak with corpus |
| Valgrind/ASan stress | — | stresstest.py | — | nightly fuzz soak (ASan default) |
| ABI size/pin guard | — | test-struct-size.c | checked-in pinyin.h + C++ smoke gate | none — keep |
| Stateful C-API fuzzer | — | fuzzer.rs (AFL++) | — | **add `capi-commands` target** |
| Hostile data-file fuzzer | — | trieloader/cdbloader | — | **add `dict-loader` target** |
| Interactive oracle tools | 4 REPL drivers | debug-chewing-shell.sh | oracle harness bins | none — equivalent exists |
