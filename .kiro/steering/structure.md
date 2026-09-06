---
inclusion: always
---
# Structure

| Crate | Role | unsafe | Portable | Ships |
|---|---|---|---|---|
| oxpinyin-core | parser, SegmentGraph, k-best, scoring traits | forbid | yes | via engine |
| oxpinyin-chewing | chewing/zhuyin layer; excisable module over core (D6 modularity) | deny | yes | via capi, engine |
| oxpinyin-data | load libpinyin-format tables (D3 route); drop-in readers for installed libpinyin data | deny (+mmap) | yes | via engine |
| oxpinyin-user | ACID store over DefaultStore; format-version from day one | deny | yes | via engine |
| oxpinyin-engine | session API — the supported Rust surface | deny | yes | yes |
| oxpinyin-facade | shared facade-orchestration layer (instance/context state machines, parse seams, cursor laws) consumed by both C-ABI facades | forbid | yes | via capi |
| oxpinyin-capi | the libpinyin C ABI — `libpinyin.so.15`, all 79 `pinyin_*` exports, libpinyin's SONAME/header/pkg-config via cargo-c | allow | Linux | yes |
| oxpinyin-zhuyin-capi | C ABI of libpinyin's zhuyin facade — `libzhuyin.so.15`, the `--enable-libzhuyin` counterpart (52 symbols, own SONAME); delegates to the same engine/chewing surface as oxpinyin-capi | allow | Linux | yes |
| oxpinyin-python | PyO3 binding over the engine session API (Python consumers) | forbid | yes | wheel only |
| oxpinyin-runtime | concrete assembly shared by consumers (tables+model+user wiring → Session) | forbid | yes | via capi/python |
| pinyin-oracle | differential harness vs pinned libpinyin | allow | Linux | never |
| oxpinyin-dictool | conversions; standalone vocab exporter | deny | yes | yes |
| oxpinyin-store | ordered byte-KV seam; Tkrzw (default since 2026-09-05), Kyoto Cabinet, LMDB, redb backends — one per binary, compile-time selected; KC/tkrzw/lmdb are Linux-only C deps, redb is the pure-Rust portability fallback (macOS/Windows CI runs --no-default-features) | deny | yes | via engine |
| oxpinyin-datagen | model20 → runtime data compiler for every backend, writing libpinyin's own file formats (libpinyin's names on KC/tkrzw, `<stem>.<ext>` on redb/LMDB) | deny | yes | never |
| oxpinyin-corpus | training corpus front-end (zhwiki dump → ngseg raw text) | deny | yes | never |
| oxpinyin-testsupport | shared test doubles (fixture Dictionary/LanguageModel); dev-only | forbid | yes | never |
| oxpinyin-segment | training segmenter (`ngseg`; `spseg`/`mergeseq` per W9 re-audit) | deny | yes | never |
| oxpinyin-counter | legacy n-gram counter (`gen_ngram`; off the trainer path — see trainer-parity-audit §4) | deny | yes | never |
| oxpinyin-lambda | training λ estimator (`estimate_interpolation` EM — on the trainer path via `evaluate.py`; `gen_deleted_ngram` held-out) | deny | yes | never |
| oxpinyin-emitter | legacy `interpolation2.text` writer (`export_interpolation`; off the trainer path — see trainer-parity-audit §4) | deny | yes | never |
| oxpinyin-kmm | K-mixture-model pipeline (generate/estimate/merge/validate/prune/export/import/→interpolation) — W9 | deny | yes | never |
| oxpinyin-punct | punctuation-table generator (`genpunct.py` reproduction) — W9 | deny | yes | never |
| oxpinyin-word | word-recognition pipeline (populate/partialword/newword/markpinyin) — W9 | deny | yes | never |
| oxpinyin-train | native trainer orchestrator (config/status/epoch, segment → KMM → interpolation → λ → correction rate) — W9 | deny | yes | never |

**Centralized assembly:** the concrete construction of a decodable engine
(system tables + unigram model + λ + optional user store + addon/punct
wiring) lives in exactly one place, `oxpinyin-runtime`; capi,
python, and future adapters consume it rather than assembling equivalents.
This is deliberate so native and language-binding paths cannot silently
diverge. It is glue over `oxpinyin-data`/`-user`/`-engine` public APIs — no
algorithm belongs there.

**Drop-in data path (P6, 2026-09-02):** there is no compatibility layer.
`oxpinyin-data` reads libpinyin's own files through lazy readers — the
pinyin and phrase DBMs, the per-library `MemoryChunk` files (mmap,
checksummed), `bigram.db`, `punct.bin`, the addon DBM pair, λ from
`table.conf` — a handle plus a point read each, nothing scanned at open.
`oxpinyin-runtime` opens a system directory the way `pinyin_init` does;
on Kyoto Cabinet and tkrzw that directory can be an unmodified libpinyin
install's `data/`, and on every backend it is what `oxpinyin-datagen
compile` writes. The caller supplies the directory (`StoragePaths`); no
distro layout is auto-detected. A redb or LMDB build reads the same
records from its own container (`<stem>.<ext>`) and cannot open a
libpinyin install directly.

**Portability seam:** `oxpinyin-engine`'s session API is framework-neutral —
abstract `KeyInput`, preedit spans + style enum, candidate iteration;
config and storage paths injected as data; no platform services and no
`cfg(target_os)` in the portable crates. IBus keysym translation lives in
oxpinyin-capi, never in the engine. Sessions are instance-per-context and
main-thread-friendly (TSF/IMK/ArkTS models).

**Supported surface:** `oxpinyin-engine` (Rust), `oxpinyin-capi` (C ABI), and `oxpinyin-python` (PyO3: `Engine`/`Candidate` over the same session API; unsafe is forbidden even at the FFI boundary).
core/data/user are published to hold names but are internal — no
stability promise; cargo-public-api snapshots apply to the supported
surface only. Extension traits (`Dictionary`, `UserModel`,
`LanguageModel`) are unsealed and grow only by defaulted methods; public
error enums are `#[non_exhaustive]`.

**Configuration model:** layered — the frozen upstream defaults
(`docs/findings/upstream-schema.md`, verbatim) → system drop-ins → a user
layer the shell supplies as data (a `ConfigLayer` file, or one built in
memory from whatever settings store the shell owns; libpinyin itself has
no settings store, and neither does the engine). Merge is a pure core
function. `Config::default()` must equal the captured upstream defaults —
the sane default *is* the parity configuration, and S1b runs under it.
Customisation is data overlays (rules, maps, schemes) and live
preferences; engine weights and LM order are never user configuration.

**Decoder:** a weighted SegmentGraph from day one (index-based arenas, not
references). EdgeKind: Exact + Segmentation now; Fuzzy/Typo/Abbrev at
Stage 2. **The scorer API accepts edge costs from the first
implementation — hard freeze.** Parity mode mirrors upstream's
path-enumeration policy per the path-set SPEC — neither more paths nor
fewer.
