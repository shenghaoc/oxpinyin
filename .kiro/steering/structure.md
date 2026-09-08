---
inclusion: always
---
# Structure

| Crate | Role | unsafe | Portable | Ships |
|---|---|---|---|---|
| oxpinyin-core | parser, SegmentGraph, k-best, scoring traits | forbid | yes | via engine |
| oxpinyin-chewing | chewing/zhuyin layer: the packed chewing key, its renderers, and the frozen content tables — a dependency-free leaf *under* core, not a module over it (core depends on chewing) | forbid | yes | via capi, engine |
| oxpinyin-data | load libpinyin-format tables (D3 route); drop-in readers for installed libpinyin data | deny (+mmap) | yes | via engine |
| oxpinyin-user | ACID store over DefaultStore; no format-version row, matching libpinyin's unversioned user files (ruling 2026-09-08, `docs/findings/user-store.md` §4) | forbid | yes | via engine |
| oxpinyin-engine | session API — the supported Rust surface | forbid | yes | yes |
| oxpinyin-facade | shared facade-orchestration layer (instance/context state machines, parse seams, cursor laws, the §9 user-data export materialization) consumed by both C-ABI facades and the Python binding; depends on core, engine, runtime and user — it holds the runtime's concrete dict/lm/user handles by value, so it forwards the whole backend feature matrix rather than staying generic over the engine traits | forbid | yes | via capi |
| oxpinyin-capi | the libpinyin C ABI — `libpinyin.so.15`, all 79 `pinyin_*` exports, libpinyin's SONAME/header/pkg-config via cargo-c | allow | Linux | yes |
| oxpinyin-zhuyin-capi | C ABI of libpinyin's zhuyin facade — `libzhuyin.so.15`, the `--enable-libzhuyin` counterpart (52 symbols, own SONAME); delegates to the same engine/chewing surface as oxpinyin-capi | allow | Linux | yes |
| oxpinyin-python | PyO3 binding over the engine session API (Python consumers) | forbid | yes | wheel only |
| oxpinyin-runtime | concrete assembly shared by consumers (tables+model+user wiring → Session) | forbid | yes | via capi/python |
| pinyin-oracle | differential harness vs pinned libpinyin | allow | Linux | never |
| oxpinyin-dictool | conversions; standalone vocab exporter over the facade's §9 import/export machinery — pure Rust, no C-ABI dependency (portable for real since that edge was cut) | forbid | yes | yes |
| oxpinyin-store | ordered byte-KV seam; Tkrzw (default since 2026-09-05), Kyoto Cabinet, LMDB, redb backends — one per binary, compile-time selected; KC/tkrzw/lmdb are Linux-only C deps, redb is the pure-Rust portability fallback (macOS/Windows CI runs --no-default-features) | deny | yes | via engine |
| oxpinyin-datagen | model20 → runtime data compiler for every backend, writing libpinyin's own file formats (libpinyin's names on KC/tkrzw, `<stem>.<ext>` on redb/LMDB); takes the MemoryChunk chunk format from `oxpinyin-data::chunk_format` | forbid | yes | never |
| oxpinyin-corpus | training corpus front-end (zhwiki dump → ngseg raw text) | forbid | yes | never |
| oxpinyin-testsupport | shared test doubles (fixture Dictionary/LanguageModel) plus the model20 cache locator; dev-lane only — its one non-dev consumer is datagen, which itself never ships | forbid | yes | never |
| oxpinyin-segment | training segmenter (`ngseg`; `spseg`/`mergeseq` per W9 re-audit) | forbid | yes | never |
| oxpinyin-counter | legacy interpolation utility: `gen_ngram`, a libpinyin util the trainer never invokes (trainer-parity-audit §4) — kept because its counting machinery is shared by corpus, lambda, eval and train | forbid | yes | never |
| oxpinyin-lambda | training λ estimator (`estimate_interpolation` EM — on the trainer path via `evaluate.py`; `gen_deleted_ngram` held-out) | forbid | yes | never |
| oxpinyin-emitter | legacy interpolation utility: `export_interpolation` → `interpolation2.text`, a libpinyin util the trainer never invokes (trainer-parity-audit §4) — kept; corpus and train emit through it | forbid | yes | never |
| oxpinyin-kmm | K-mixture-model pipeline (generate/estimate/merge/validate/prune/export/import/→interpolation) — W9 | forbid | yes | never |
| oxpinyin-punct | punctuation-table generator (`genpunct.py` reproduction) — W9 | forbid | yes | never |
| oxpinyin-word | word-recognition pipeline (populate/partialword/newword/markpinyin) — W9 | forbid | yes | never |
| oxpinyin-eval | training correction-rate evaluator (`evaluate.py` + `eval_correction_rate` reproduction) — W9 | forbid | yes | never |
| oxpinyin-train | native trainer orchestrator (config/status/epoch, segment → KMM → interpolation → λ → correction rate) — W9 | forbid | yes | never |

**Centralized assembly:** the concrete construction of a decodable engine
(system tables + unigram model + λ + optional user store + addon/punct
wiring) lives in exactly one place, `oxpinyin-runtime`; the facades
(through `oxpinyin-facade`), capi, python, and future adapters consume it
rather than assembling equivalents. This is deliberate so native and
language-binding paths cannot silently diverge. It is wiring over `oxpinyin-data`/`-user`/`-engine` public APIs;
what algorithm it does hold is deliberate and pinned — the user-count
overlay feed (the arithmetic itself lives in `oxpinyin-data`'s
`*_with_user_delta` methods) and the key-cost cache's seqlock — both
cited against the pin in its source. (The review that corrected this
paragraph also noted the bigram-merge arithmetic there duplicates
`merge_bigram` already exported by data; folding that in is an open
follow-up.)

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
stability promise. There is no cargo-public-api snapshot tooling in the
tree; the supported surface's stability is review-enforced
(docs/safety/enforcement-matrix.md records this). Extension traits (`Dictionary`, `UserModel`,
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
