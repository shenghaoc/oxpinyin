---
inclusion: always
---
# Structure

| Crate | Role | unsafe | Portable | Ships |
|---|---|---|---|---|
| pinyin-core | parser, SegmentGraph, k-best, scoring traits | forbid | yes | via engine |
| pinyin-data | load libpinyin-format tables (D3 route) | deny (+mmap) | yes | via engine |
| pinyin-user | redb ACID store; format-version from day one | deny | yes | via engine |
| pinyin-engine | session API — the supported Rust surface | deny | yes | yes |
| pinyin-capi | C ABI subset for the borrowed frontend | allow | Linux | yes |
| pinyin-oracle | differential harness vs pinned libpinyin | allow | Linux | never |
| pinyin-dictool | conversions; standalone vocab exporter | deny | yes | yes |
| pinyin-migrate | settings + vocab import; FFI fallback | allow | Linux | yes |
| pinyin-segment | training segmenter (`ngseg` reproduction) | deny | yes | never |
| pinyin-counter | training n-gram counter (`gen_ngram` reproduction) | deny | yes | never |
| pinyin-lambda | training λ estimator (`gen_deleted_ngram` + `estimate_interpolation`) | deny | yes | never |
| pinyin-emitter | training `interpolation2.text` writer (`export_interpolation` reproduction) | deny | yes | never |

**Portability seam:** `pinyin-engine`'s session API is framework-neutral —
abstract `KeyInput`, preedit spans + style enum, candidate iteration;
config and storage paths injected as data; no platform services and no
`cfg(target_os)` in the portable crates. IBus keysym translation lives in
pinyin-capi, never in the engine. Sessions are instance-per-context and
main-thread-friendly (TSF/IMK/ArkTS models).

**Supported surface:** `pinyin-engine` (Rust) and `pinyin-capi` (C ABI).
core/data/user are published to hold names but are internal — no
stability promise; cargo-public-api snapshots apply to the supported
surface only. Extension traits (`Dictionary`, `UserModel`,
`LanguageModel`) are unsealed and grow only by defaulted methods; public
error enums are `#[non_exhaustive]`.

**Configuration model:** layered — pinned upstream defaults (P0-5,
verbatim) → system drop-ins → user (GSettings on Linux; file backend
elsewhere via the same trait). Merge is a pure core function.
`Config::default()` must equal the captured upstream defaults — the sane
default *is* the parity configuration, and S1b runs under it.
Customisation is data overlays (rules, maps, schemes) and live
preferences; engine weights and LM order are never user configuration.

**Decoder:** a weighted SegmentGraph from day one (index-based arenas, not
references). EdgeKind: Exact + Segmentation now; Fuzzy/Typo/Abbrev at
Stage 2. **The scorer API accepts edge costs from the first
implementation — hard freeze.** Parity mode mirrors upstream's
path-enumeration policy per the path-set SPEC — neither more paths nor
fewer.
