# Framework-neutral session API SPEC

Date: 2026-08-09 · Status: **frozen for W4-T0**

This SPEC freezes the `oxpinyin-engine` session surface before any decoder work
lands on top of it. Every later W4 task fills these signatures in; none of them
may change one. A signature change after this branch is an Architect+human
correction, recorded in the log at the end of this file.

The rules it implements are already stated in `.kiro/steering/structure.md`
under **Portability seam**. This finding turns them into a named, frozen API so
that a shell author — IBus today, TSF/IMK/ArkTS later — can read one document
and know exactly what they consume.

## What this seam is for

`oxpinyin-engine` is one of the two supported surfaces (the other is
`oxpinyin-capi`'s C ABI). Everything below it — `oxpinyin-core`, `oxpinyin-data`,
`oxpinyin-user` — is internal and carries no stability promise.

A shell supplies platform facts as **data** and receives platform-free results:

| Shell supplies | Engine returns |
|---|---|
| `KeyInput` — logical key, modifiers, committed text | `KeyOutcome` |
| `StoragePaths` — where user and system data live | preedit spans + candidates |
| a `ConfigSource` — typed settings by key | deterministic commits |

## Deliberately absent

Naming these matters as much as naming what is present, because each one is a
place a portable API usually leaks:

- **No keysyms.** `LogicalKey` is an abstract key, never an IBus/X11 keyval.
  Translation from `IBUS_KEY_*` lives in `oxpinyin-capi` and nowhere else.
- **No GSettings, no dconf, no registry.** Configuration arrives through
  `ConfigSource`. The layered `Config` that satisfies it is W4-T0c; a GSettings
  backend is a shell concern outside both.
- **No path discovery.** The engine never calls `dirs`, `XDG_*`, `%APPDATA%`
  or `NSSearchPathForDirectoriesInDomains`. It uses the `StoragePaths` it is
  handed.
- **No `cfg(target_os)`, no platform types, anywhere in the portable crates.**
- **No threading contract.** `Session` is not required to be `Send` or `Sync`.
  Sessions are instance-per-context and main-thread-friendly, which is what the
  TSF, IMK and ArkTS models want.
- **No clock, no locale, no environment reads.** Output stays a pure function
  of (input, user state, config), per constitution item 6.

## Key input

```rust
pub struct Modifiers(u8);          // SHIFT | CONTROL | ALT | SUPER
pub enum LogicalKey {              // #[non_exhaustive]
    Character(char),
    Backspace, Delete, Enter, Escape, Space, Tab,
    Left, Right, Up, Down, Home, End, PageUp, PageDown,
    Unknown,
}
pub struct KeyInput { /* private */ }
```

`KeyInput` carries the logical key, the modifier set, and the **text** the key
would commit if the engine ignored it (empty for non-text keys). Fields are
private with constructors and accessors so the struct can grow without breaking
the freeze; `LogicalKey` is `#[non_exhaustive]` for the same reason.

A shell that only knows how to send characters can use
`KeyInput::character(ch)`. `Unknown` exists so a shell never has to invent a
key: an unmapped key is reported as unmapped, and the engine leaves it alone.

## Preedit

```rust
pub enum SpanStyle { Raw, Converted, Selected }   // #[non_exhaustive]
pub struct PreeditSpan { /* byte range + style */ }
pub struct Preedit { /* text, spans, cursor */ }
```

The preedit is **text plus spans**, never markup and never platform attribute
objects. Spans are non-overlapping half-open byte ranges into the preedit text,
in ascending order, and together they cover it exactly. `cursor` is a byte
offset into the same text and always lands on a character boundary.

`Raw` is un-converted input the user typed, `Converted` is engine output not
yet chosen, `Selected` is text the user has already picked in this composition.
A shell maps the three to its own underline/highlight conventions.

## Candidates

```rust
pub enum CandidateKind { Phrase, Sentence, Fallback }   // #[non_exhaustive]
pub struct Candidate { /* text, kind, consumed keys and bytes, cost */ }
pub struct CandidateList { /* ordered, deterministic */ }
```

`CandidateList` supports `len`, `is_empty`, `get` and `iter`. Indexing is
always checked: `get` returns `Option`, and `Session::select` on an
out-of-range index returns `Err`, never a panic. That is the standing
requirement behind F-E-02 (`robustness-evidence.md`), where the upstream defect
is precisely an unchecked candidate index after the list was regenerated.

`consumed_keys` and `consumed_bytes` say how much of the composition a
candidate would absorb, so a shell can render partial acceptance without
re-deriving it.

## Storage and configuration

```rust
pub struct StoragePaths { user_data_dir: PathBuf, system_data_dirs: Vec<PathBuf> }

pub enum ConfigValue { Bool(bool), Int(i32), Int64(i64), Text(String) }  // #[non_exhaustive]
pub trait ConfigSource {
    fn get(&self, key: &str) -> Option<&ConfigValue>;
    // defaulted typed readers: bool / int / int64 / text
}
```

The four `ConfigValue` variants mirror the GSettings types actually used by the
upstream schema frozen in `docs/findings/upstream-schema.md` (`b`, `i`, `x`,
`s`). `ConfigSource` is object-safe and grows only by defaulted methods.

W4-T0c supplies the concrete layered `Config`, its captured upstream defaults
and the pure merge function. The replay harness (W4-T4b) is the second
consumer, with a file-backed source for test scenarios.

## Session

```rust
pub struct Session<D, L> { /* private */ }

impl<D, L> Session<D, L>
where
    D: Dictionary<Syllable = SyllableKey, Entry = PhraseEntry>,
    D::Error: Display,
    L: LanguageModel<Token = PhraseToken>,
    L::Error: Display,
{
    pub fn new(config: &dyn ConfigSource, paths: StoragePaths, dictionary: D, model: L)
        -> Result<Self, EngineError>;

    pub fn process_key(&mut self, input: &KeyInput) -> Result<KeyOutcome, EngineError>;
    pub fn select(&mut self, index: usize) -> Result<Selection, EngineError>;
    pub fn commit(&mut self) -> Result<String, EngineError>;
    pub fn reset(&mut self);

    pub fn preedit(&self) -> Preedit;
    pub fn candidates(&self) -> &CandidateList;
    pub fn raw_input(&self) -> &str;
    pub fn is_composing(&self) -> bool;
    pub fn page_size(&self) -> usize;
    pub fn paths(&self) -> &StoragePaths;
    pub fn dictionary(&self) -> &D;
    pub fn language_model(&self) -> &L;

    // W6-T3 added the training surface. The frozen W4-T0 methods above
    // did not change; these are additions. `train` is method-generic over
    // `UserModel` so the engine stays user-agnostic.
    pub fn train<U>(&self, user: &mut U) -> Result<(), EngineError>
    where
        U: UserModel<Token = PhraseToken>,
        U::Error: Display;
    pub fn selected_tokens(&self) -> &[PhraseToken];
    pub fn composition_keys(&self) -> Result<Vec<SyllableKey>, EngineError>;

    // W14 added the sentence surface (docs/findings/sentence-surface.md).
    // Additions again; nothing frozen above changed. `guess_sentence` is
    // the m_nbest_results gate: rows live until the next guess or reset,
    // and `candidates` prepends them as NBEST rows while they do.
    pub fn guess_sentence(&mut self) -> Result<bool, EngineError>;
    pub fn sentence_lookup_active(&self) -> bool;
    pub fn sentence_text(&self, index: u8) -> Option<&str>;
}

pub enum KeyOutcome { Ignored, Consumed, Commit(String) }   // #[non_exhaustive]
pub enum Selection { Continued, Completed }                 // #[non_exhaustive]
```

The bounds are part of the freeze. They name concrete `oxpinyin-core` types
(`SyllableKey`, `PhraseEntry`, `PhraseToken`) precisely so the decoder can be
added later without widening them, and the `Display` bounds exist because the
frozen `core-trait-seam.md` traits leave `Error` unbounded — the engine
type-erases backend failures into `EngineError` rather than leaking an
associated type into its public surface.

`Session` owns its dictionary and language model. Swapping W4's fixture
adapters for W3's table-backed loaders is a change of the two type arguments
and nothing else.

### Behavioural contract

- `process_key` appends characters the parser has syntax for (ASCII lowercase
  and apostrophe) to the raw input buffer; `Backspace` removes the last
  character, or undoes a selection when nothing else remains; `Escape` clears
  the composition; `Enter` commits it; `Space` chooses the first candidate, and
  commits when that finishes the composition.
- A key the session does not use is `KeyOutcome::Ignored`, and the session is
  unchanged. Any key held with Control, Alt or Super is ignored: the shell is
  invoking a command. Shift is not such a modifier — `Shift`+letter is ordinary
  typing. A full input buffer also reports `Ignored` — refusing further input
  is not an error condition.
- `MAX_INPUT_BYTES` is 4,096, matching the largest input the frozen F-A and
  parity-corpus fixtures carry.
- `select` appends the chosen candidate to the selected prefix and advances
  over the bytes it consumed, returning `Completed` once nothing is left.
- `commit` returns the committed text and resets the session. It never fails on
  an empty composition; it returns an empty string.
- A composition with no dictionary result still offers one candidate: the
  remaining raw input, as `CandidateKind::Fallback`. Before the decoder exists
  that is the only candidate a session can honestly produce, and it keeps
  `Space` and `select` meaningful at every stage.
- Every method is deterministic for the same (input, state, config).

`EmptyConfigSource` is supplied for callers that have no configuration yet:
every key is absent and every session setting falls back to its documented
default.

## Errors

```rust
pub enum EngineError {                    // #[non_exhaustive]
    CandidateIndexOutOfRange { index: usize, len: usize },
    Dictionary(String),
    LanguageModel(String),
    UserModel(String),                    // W6-T3
    Graph(GraphError),
    Decode(DecodeError),
    Scoring(ScoringError),
}
```

`#[non_exhaustive]`, `Display` and `std::error::Error`. Later tasks add
variants; adding a variant to a `#[non_exhaustive]` enum is the documented
growth path and is not a freeze violation. Removing or renaming one is.

## Decoder vocabulary types

`oxpinyin-core` gains the types the bounds above name:

```rust
pub struct SyllableKey(u16);       // dense id over the frozen key inventory
pub struct PhraseToken(u32);
pub struct PhraseEntry { token: PhraseToken, text: String }
```

The key inventory is the 405 complete syllables of `parser-spec.md` in their
frozen numeric-ID order, followed by the initial-only keys in ascending byte
order. An initial-only key is a non-empty proper prefix of a complete syllable
that contains no vowel byte (`a`, `e`, `i`, `o`, `u`, `v`) and is not itself a
complete syllable; that rule yields exactly 23 keys and is asserted against the
frozen inventory by test. W4-T1 records the oracle evidence that these 23 are
the pin's incomplete-key set and makes them graph edges.

## Acceptance

- Compiles on Linux, macOS and Windows; no `cfg(target_os)` and no platform
  dependency anywhere in `oxpinyin-engine`.
- Every public item carries a doc comment.
- Candidate indexing, preedit span coverage and buffer bounds are unit-tested.

## Architect correction log

**2026-08-15 — W6-T3 training surface.** `Session` gained `train`,
`selected_tokens`, and `composition_keys`. `EngineError` gained
`UserModel`. None of the frozen W4-T0 signatures changed; these are
additions. `train` is method-generic over `UserModel` so the engine
stays user-agnostic. Recorded by W6-T4 so the SPEC and the code agree.
