# Findings — W2-T1 oracle FFI seam

Date: 2026-08-09 · Source tier: Architect capture; human freeze pending.

This finding freezes the FFI surface `pinyin-oracle` uses to drive the
pin-built libpinyin as the W2 differential subject. It is the implementation
contract for W2-T1 and the live producer for W2-T3.

`pinyin-oracle` is `publish = false` and never ships. Nothing in this finding
expands the supported `pinyin-capi` surface.

## Source identity and provenance

ABI declarations were read from the **public header only** of the oracle built
by `tools/oracle/build-oracle.sh`. That prefix is the sole subject; no
distribution or system-packaged libpinyin qualifies, is referenced, or is
compared against.

The authority for the header identity is the `header_sha256` field of
`oracle-pin.txt`, which the recipe writes into the prefix:

- libpinyin tag `2.11.91`, commit
  `0c5e80e1200f84fab185d1c5bde458b770a0636c`;
- `include/libpinyin-2.11.91/pinyin.h` SHA-256
  `e1138482d06766163608406fe1083539b21ff8c44ea04f329f3db0c78a312d47`,
  equal to `header_sha256` in the prefix manifest;
- scalar and flag definitions from the headers that public header includes:
  `include/novel_types.h` and `storage/pinyin_custom2.h`;
- data payload verified by `oracle-data.sha256` (23 generated files).

This is the same method `docs/findings/abi-subset.md` used to derive the
frontend-called subset: read the declared public interface, not the
implementation. No `.cpp` translation unit was read, and no upstream logic was
transcribed. The parity *behaviour* contract remains the executable oracle plus
frozen fixtures, per `docs/findings/spec-derivation.md`.

Independent cross-check: the flag word this finding derives for the F-A capture
profile, `IS_PINYIN | PINYIN_INCOMPLETE | USE_DIVIDED_TABLE |
USE_RESPLIT_TABLE`, evaluates to `0x0000018a`, which equals the `flags` field
recorded in every `fixtures/foundation/f-a.txt` record.

## Bindgen decision

Hand-written declarations, not `bindgen`.

`bindgen` would add a build-dependency tree to the workspace, and adding
dependencies without an explicit ask is a hard forbid in `AGENTS.md`. The
required surface is 17 functions over four opaque types, so a hand-written
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

`pinyin_context_t`, `pinyin_instance_t`, `lookup_candidate_t`, `ChewingKey` and
`ChewingKeyRest` are declared as opaque `extern type`-style zero-field structs
with private fields. Their layout is never assumed, never constructed on the
Rust side, and never dereferenced except by passing the pointer back to
libpinyin.

## Function subset

The W2-T1 card scopes the subset to: init context, parse, candidates to depth
10, reset and free. That is 17 symbols.

```c
pinyin_context_t * pinyin_init(const char * systemdir, const char * userdir);
void  pinyin_fini(pinyin_context_t * context);
bool  pinyin_set_options(pinyin_context_t * context, pinyin_option_t options);
pinyin_instance_t * pinyin_alloc_instance(pinyin_context_t * context);
void  pinyin_free_instance(pinyin_instance_t * instance);
bool  pinyin_reset(pinyin_instance_t * instance);
size_t pinyin_parse_more_full_pinyins(pinyin_instance_t * instance,
                                      const char * pinyins);
size_t pinyin_get_parsed_input_length(pinyin_instance_t * instance);
bool  pinyin_guess_candidates(pinyin_instance_t * instance, size_t offset,
                              guint sort_option);
bool  pinyin_get_n_candidate(pinyin_instance_t * instance, guint * num);
bool  pinyin_get_candidate(pinyin_instance_t * instance, guint index,
                           lookup_candidate_t ** candidate);
bool  pinyin_get_candidate_string(pinyin_instance_t * instance,
                                  lookup_candidate_t * candidate,
                                  const gchar ** utf8_str);
bool  pinyin_get_pinyin_key(pinyin_instance_t * instance, size_t offset,
                            ChewingKey ** key);
bool  pinyin_get_pinyin_key_rest(pinyin_instance_t * instance, size_t offset,
                                 ChewingKeyRest ** key_rest);
bool  pinyin_get_pinyin_key_rest_positions(pinyin_instance_t * instance,
                                           ChewingKeyRest * key_rest,
                                           guint16 * begin, guint16 * end);
bool  pinyin_get_pinyin_string(pinyin_instance_t * instance, ChewingKey * key,
                              gchar ** utf8_str);
bool  pinyin_get_pinyin_is_incomplete(pinyin_instance_t * instance,
                                      ChewingKey * key);
void  g_free(void * mem);   /* GLib, for owned gchar* returns */
```

15 of these appear in the 52-symbol frontend-called queue in
`docs/findings/abi-subset.md`. `pinyin_get_pinyin_is_incomplete` does not;
`abi-subset.md` explicitly permits harness-only symbols in `pinyin-oracle`
without expanding the `pinyin-capi` surface. `g_free` is GLib, not libpinyin.

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

Carried over verbatim from `docs/findings/capture-fixtures.md` and
`docs/findings/oracle-environment.md`:

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

Sentence conversion, candidate selection, prediction, user-phrase iteration,
training and saving are outside this subset. Adding a symbol requires updating
this finding first. Depth beyond 10 candidates is out of scope: the capture
protocol and W2-T3 comparison both stop at 10.
