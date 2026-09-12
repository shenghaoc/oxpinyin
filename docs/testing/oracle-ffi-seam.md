# Findings — W2-T1 oracle FFI seam

Date: 2026-08-09 · Source tier: Architect capture; human freeze pending.

This finding freezes the FFI surface `pinyin-oracle` uses to drive the
pin-built libpinyin as the W2 differential subject. It is the implementation
contract for W2-T1 and the live producer for W2-T3.

`pinyin-oracle` is `publish = false` and never ships. Nothing in this finding
expands the supported `oxpinyin-capi` surface.

## Source identity and provenance

ABI declarations were read from the **public header only** of the oracle built
by `tools/oracle/build-oracle.sh`. That prefix is the sole subject; no
distribution or system-packaged libpinyin qualifies, is referenced, or is
compared against.

The authority for the header identity is the `header_sha256` field of
`oracle-pin.txt`, which the recipe writes into the prefix:

- libpinyin version `2.11.92`, commit
  `074a2219c90feaf962d0d24f034514033ece5f99`;
- `include/libpinyin-2.11.92/pinyin.h` SHA-256
  `e1138482d06766163608406fe1083539b21ff8c44ea04f329f3db0c78a312d47`,
  equal to `header_sha256` in the prefix manifest;
- scalar and flag definitions from the headers that public header includes:
  `include/novel_types.h` and `storage/pinyin_custom2.h`;
- data payload verified by `oracle-data.sha256` (the 17 reproducible
  generated files) plus `oracle-data-unstable.sha256` (the 6 files
  libpinyin does not generate reproducibly; informational, see
  `docs/findings/oracle-data-reproducibility.md`).

This is the same method `docs/findings/abi-subset.md` used to derive the
frontend-called subset: derive it from the declared public interface. The
parity *behaviour* contract remains the executable oracle plus frozen
fixtures, per `docs/findings/spec-derivation.md`.

Independent cross-check: the flag word this finding derives for the F-A capture
profile, `IS_PINYIN | PINYIN_INCOMPLETE | USE_DIVIDED_TABLE |
USE_RESPLIT_TABLE`, evaluates to `0x0000018a`, which equals the `flags` field
recorded in every `fixtures/foundation/f-a.txt` record.

## Bindgen decision

Hand-written declarations, not `bindgen`.

`bindgen` would add a build-dependency tree to the workspace, and adding
dependencies without an explicit ask is a hard forbid in `AGENTS.md`. The
required surface was 17 functions over five opaque types at W2-T1 (the
original text said four; the binding's first revision and the list below
both had five) and is 29 over six today, so a hand-written
`extern "C"` block is smaller than the tooling it replaces, is reviewable
against the hashes above, and keeps `Cargo.lock` unchanged. The W2-T1 card
permits either choice.

## Scalar mapping

| C spelling | Definition site | Rust type |
|---|---|---|
| `bool` | C++ / C `_Bool` | `bool` |
| `size_t` | libc | `usize` |
| `guint` | GLib | `c_uint` |
| `guint16` | GLib | `u16` |
| `guint32` | GLib | `u32` |
| `gchar` | GLib | `c_char` |
| `pinyin_option_t` | `novel_types.h:143` (`guint32`) | `u32` |
| `sort_option_t` | `pinyin.h` enum | `c_uint` |

Every function in this subset returns C `bool`, **not** `gboolean`. GLib's
`gboolean` is `gint` (4 bytes); C `bool` is `_Bool` (1 byte, values 0 and 1).
Rust's `bool` is FFI-compatible with `_Bool`, so `bool` is the correct return
mapping. Declaring these returns as `c_int` would read undefined upper bits on
the SysV AMD64 ABI, where `_Bool` is returned in `AL`. This distinction is
load-bearing and must not be "simplified" later.

## Opaque types

`pinyin_context_t`, `pinyin_instance_t`, `lookup_candidate_t`, `ChewingKey`,
`ChewingKeyRest` and, since the W6 differential, `export_iterator_t`
(`ExportIterator`) are declared as opaque `extern type`-style zero-field structs
with private fields. Their layout is never assumed, never constructed on the
Rust side, and never dereferenced except by passing the pointer back to
libpinyin.

## Function subset

The W2-T1 card scoped the subset to: init context, parse, candidates to
depth 10, reset and free — 17 symbols. The seam has since grown with the
harness: sentence conversion (W14), candidate type and n-best index, the
key-rest positions, training and saving (W6), the phrase export iterator
and the token lookups (the W6 differential). The authoritative list is
the `extern "C"` block in `crates/pinyin-oracle/src/ffi.rs`; as of
2026-09-12 it declares these 29 symbols (Rust spellings; every `bool`
return is C `_Bool`, see Scalar mapping):

```text
pinyin_init(systemdir: *const c_char, userdir: *const c_char) -> *mut PinyinContext
pinyin_fini(context: *mut PinyinContext)
pinyin_set_options(context: *mut PinyinContext, options: PinyinOption) -> bool
pinyin_alloc_instance(context: *mut PinyinContext) -> *mut PinyinInstance
pinyin_free_instance(instance: *mut PinyinInstance)
pinyin_reset(instance: *mut PinyinInstance) -> bool
pinyin_parse_more_full_pinyins(instance: *mut PinyinInstance, pinyins: *const c_char) -> usize
pinyin_get_parsed_input_length(instance: *mut PinyinInstance) -> usize
pinyin_guess_sentence(instance: *mut PinyinInstance) -> bool
pinyin_get_sentence(instance: *mut PinyinInstance, index: u8, sentence: *mut *mut c_char) -> bool
pinyin_guess_candidates(instance: *mut PinyinInstance, offset: usize, sort_option: c_uint) -> bool
pinyin_get_n_candidate(instance: *mut PinyinInstance, num: *mut c_uint) -> bool
pinyin_get_candidate(instance: *mut PinyinInstance, index: c_uint, candidate: *mut *mut LookupCandidate) -> bool
pinyin_get_candidate_string(instance: *mut PinyinInstance, candidate: *mut LookupCandidate, utf8_str: *mut *const c_char) -> bool
pinyin_get_candidate_type(instance: *mut PinyinInstance, candidate: *mut LookupCandidate, candidate_type: *mut c_int) -> bool
pinyin_get_candidate_nbest_index(instance: *mut PinyinInstance, candidate: *mut LookupCandidate, index: *mut u8) -> bool
pinyin_get_pinyin_key(instance: *mut PinyinInstance, offset: usize, key: *mut *mut ChewingKey) -> bool
pinyin_get_pinyin_key_rest(instance: *mut PinyinInstance, offset: usize, key_rest: *mut *mut ChewingKeyRest) -> bool
pinyin_get_pinyin_key_rest_positions(instance: *mut PinyinInstance, key_rest: *mut ChewingKeyRest, begin: *mut u16, end: *mut u16) -> bool
pinyin_get_pinyin_string(instance: *mut PinyinInstance, key: *mut ChewingKey, utf8_str: *mut *mut c_char) -> bool
pinyin_get_pinyin_is_incomplete(instance: *mut PinyinInstance, key: *mut ChewingKey) -> bool
pinyin_train(instance: *mut PinyinInstance, index: u8) -> bool
pinyin_save(context: *mut PinyinContext) -> bool
pinyin_begin_get_phrases(context: *mut PinyinContext, index: c_uint) -> *mut ExportIterator
pinyin_iterator_has_next_phrase(iter: *mut ExportIterator) -> bool
pinyin_iterator_get_next_phrase(iter: *mut ExportIterator, phrase: *mut *mut c_char, pinyin: *mut *mut c_char, count: *mut c_int) -> bool
pinyin_end_get_phrases(iter: *mut ExportIterator)
pinyin_lookup_tokens(instance: *mut PinyinInstance, phrase: *const c_char, tokenarray: *mut GArray) -> bool
pinyin_token_get_phrase(instance: *mut PinyinInstance, token: u32, len: *mut c_uint, utf8_str: *mut *mut c_char) -> bool
```

GLib, not libpinyin: `g_free` (owned `gchar*` returns), `g_array_new` /
`g_array_free` (the token array `pinyin_lookup_tokens` fills).

23 of these carry a row in the live frontend-called list of
`docs/findings/abi-subset.md` §1 (50 symbols); the other 6
(`pinyin_get_parsed_input_length`, `pinyin_get_pinyin_key`, `pinyin_get_pinyin_string`, `pinyin_get_pinyin_is_incomplete`, `pinyin_lookup_tokens`, `pinyin_token_get_phrase`) are harness-only, which `abi-subset.md` explicitly permits in
`pinyin-oracle` without expanding the `oxpinyin-capi` surface.

## Constants

| Name | Value | Use |
|---|---|---|
| `IS_PINYIN` | `1 << 1` = `0x002` | base flag |
| `PINYIN_INCOMPLETE` | `1 << 3` = `0x008` | partial tails |
| `USE_DIVIDED_TABLE` | `1 << 7` = `0x080` | divided table |
| `USE_RESPLIT_TABLE` | `1 << 8` = `0x100` | resplit table |
| `DYNAMIC_ADJUST` | `1 << 9` = `0x200` | **rejected** by the protocol |
| `SORT_BY_PHRASE_LENGTH_AND_PINYIN_LENGTH_AND_FREQUENCY` | `0x1e` | candidate order |

The F-A capture profile is `0x18a`. The F-C baseline is `IS_PINYIN` alone.

## Ownership and lifetimes

Observed from `tools/capture/capture.c`, our own `-Werror`-clean harness
against this header:

| Returned pointer | Owner | Release |
|---|---|---|
| `pinyin_context_t *` from `pinyin_init` | caller | `pinyin_fini` |
| `pinyin_instance_t *` from `pinyin_alloc_instance` | caller | `pinyin_free_instance` |
| `gchar *` from `pinyin_get_pinyin_string` | caller | `g_free` |
| `const gchar *` from `pinyin_get_candidate_string` | instance | never freed; copy before reuse |
| `lookup_candidate_t *` from `pinyin_get_candidate` | instance | never freed |
| `ChewingKey *`, `ChewingKeyRest *` | instance | never freed |

Every instance-borrowed pointer is invalidated by the next mutating call on
that instance (`pinyin_reset`, a further parse, or a further
`pinyin_guess_candidates`) and by `pinyin_free_instance`. The Rust wrapper
therefore copies borrowed strings into owned `String`/`Vec<u8>` before
returning, and never lets a raw borrowed pointer escape a method body.

An instance must not outlive its context. The wrapper enforces this with a
lifetime parameter tying the instance handle to a `&Context` borrow, so the
ordering is a compile-time property rather than a review rule.

## Parity protocol

Carried over verbatim from `docs/testing/capture-fixtures.md` and
`docs/testing/oracle-environment.md`:

- fresh, empty user directory per run; the harness never calls training,
  remembering, choosing or saving APIs;
- learning off; `DYNAMIC_ADJUST` is **rejected**, not merely unset — a request
  containing that bit is an error, never a silent mask;
- candidates are capped at the first 10 while the uncapped total is retained;
- the pin is verified before any observation is accepted.

## Pin verification

`oracle-environment.md` requires that W2-T3 and every S1b parity run load only
the pin-built shared object. The wrapper therefore reads `oracle-pin.txt` from
the prefix and refuses to open a context unless:

- `schema` is `pinyin-oracle-v1`;
- `pin_ref` equals the frozen reference string;
- `dbm` is `Tkrzw`.

A prefix that fails any check yields an error. A distribution-provided
libpinyin can never satisfy `pin_ref`, so it cannot be mistaken for the
oracle; it is reachable only as the advisory `distro-delta` class, which never
gates S1b.

## Build and link discovery

Linking is opt-in through the non-default cargo feature `oracle-ffi`.

- Feature off (the default, and what portable CI builds): no `extern` block is
  compiled, no link flags are emitted, and the crate builds on every supported
  host. `cargo check --workspace` and `cargo test --locked` stay green without
  an oracle present.
- Feature on: `build.rs` resolves the oracle prefix and emits
  `rustc-link-search` and `rustc-link-lib` for `pinyin` and `glib-2.0`, plus a
  rerun-if-changed on the prefix manifest. A prefix that is missing, off-pin,
  or lacking the shared object fails the build with a message naming the
  recipe.

The prefix is located, in order:

1. `PINYIN_ORACLE_PREFIX`, if set;
2. `PKG_CONFIG_PATH`, by walking each entry up from `lib/pkgconfig` or
   `lib64/pkgconfig` to the prefix root that holds `oracle-pin.txt`;
3. `$HOME/.local/opt/pinyin-oracle`.

Step 2 makes the invocation from the W2-T1 card work unchanged:

```bash
PKG_CONFIG_PATH=$HOME/.local/opt/pinyin-oracle/lib/pkgconfig \
LD_LIBRARY_PATH=$HOME/.local/opt/pinyin-oracle/lib \
cargo test -p pinyin-oracle --features oracle-ffi
```

`build.rs` accepts either `lib/` or `lib64/`, since the recipe's own
`find` step and `PKG_CONFIG_PATH` export cover both layouts. It rejects any
candidate whose `oracle-pin.txt` does not match the frozen pin, so a stray
prefix on the search path cannot be linked by accident.

This keeps the crate Linux-first without a single `cfg(target_os)` in a
portable crate, and satisfies the W2-T1 note that the FFI only builds on Linux
with the oracle installed.

## Non-goals

Candidate selection (`pinyin_choose_candidate`), prediction, the addon and
import surfaces, and the zhuyin facade are outside this subset — the
differentials that need them drive the built libraries through the C
drivers under `tools/bisection/`, not through this binding. Sentence
conversion, training, saving and the phrase export iterator, listed as
non-goals at W2-T1, are in the subset now (above). Adding a symbol still
means updating this finding. Depth beyond 10 candidates is out of scope:
the capture protocol and W2-T3 comparison both stop at 10.
