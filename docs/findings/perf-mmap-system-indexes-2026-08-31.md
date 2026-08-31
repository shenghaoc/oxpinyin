# Sysimage: mmap-backed system pinyin/phrase indexes — 2026-08-31

Date: 2026-08-31 · Status: **REJECTED — architecture withdrawn, kept as
a measurement record** · Continues
`perf-backend-matrix-2026-08-31.md` (PR #260's controlled 4-cell matrix).

## Disposition (2026-09-01)

The custom sysimage format was **rejected because it violates the
project's same-file-format / direct-replacement requirement**
(`docs/findings/compatibility-policy.md`: "the same binary interface,
the same file formats, the same observable behaviour"). Performance
cannot justify an oxpinyin-specific on-disk format, and the
investigation of `docs/findings/libpinyin-system-data-formats-2026-09-01.md`
showed the performance goal is reachable on libpinyin's own files.
The implementation was removed from the branch; what this document
keeps is the *measurement* half — the component breakdown, the matrix
rerun, and the RSS-composition evidence — because those facts hold for
any representation that stops reconstructing eagerly.

## Corrected architecture understanding

The premise this PR started from ("libpinyin's system phrase/pinyin
indexes are mmap-backed binary files") was **partly wrong**:

```text
libpinyin:
    pinyin/phrase indexes  → backend DBMs (KC TreeDB / Tkrzw TreeDBM),
                             opened lazily, served with per-lookup gets
    phrase-library content → per-library *.bin MemoryChunk files
                             (the one mmap-backed representation)
    bigram / punct         → backend DBMs (HashDB(M) / TreeDB(M))

oxpinyin:
    must consume those same formats directly (P1–P5 rework)
```

There is no single mmap'ed libpinyin representation standing in for
all of these structures, and an oxpinyin-side replacement for one is
not a compatible substitute.

## What this means for current performance

**Nothing in the shipped runtime changed.** The performance results of
record remain #260's: oxpinyin initializes in ~86 ms (KC) / ~106 ms
(Tkrzw) at ~69–73 MiB RSS, because the runtime still eagerly loads its
own store tables. The numbers below are the *withdrawn* iteration's
measurements, kept only as evidence of where the cost lives. The
follow-up stack replaces them: P1 (PR #269) delivers the
`MemoryChunk`/`SubPhraseIndex` reader over libpinyin's real files;
P2/P3 rewire the system dictionary onto libpinyin's DBMs and chunk
files; only then is initialization re-measured.

## 1. The baseline this starts from

PR #260 established that oxpinyin's ~100× initialization gap and 4–5×
RSS gap are properties of the implementation architecture, not of the
database backend: libpinyin initializes in under a millisecond with
either backend by **never eagerly reconstructing its immutable system
indexes**, while oxpinyin walked every store table and rebuilt the
whole dictionary on the heap at every open.

| Implementation | Backend | Init | Steady cycle | Post-init RSS |
| --- | --- | ---: | ---: | ---: |
| libpinyin | Tkrzw | 0.854 ms | 8.035 ms | 13,328 KiB |
| libpinyin | KC | 0.920 ms | 8.002 ms | 17,972 KiB |
| oxpinyin | KC | 86.188 ms | 8.473 ms | 72,910 KiB |
| oxpinyin | Tkrzw | 106.383 ms | 8.932 ms | 69,256 KiB |

This PR is the first Stage-2 optimization on that finding: the system
pinyin/phrase index path stops reconstructing and adopts libpinyin's
proven architecture — compact immutable files, mapped at startup,
served through offset views.

## 2. What oxpinyin rebuilt (the before-path)

`SystemDictionary::open` (crates/oxpinyin-data/src/dict.rs, pre-PR):

1. `load_phrase_index` — backend walk of 138,096 phrase rows into
   `Vec<(LeByteKey, CompactString)>` (one heap string per phrase).
2. `load_pinyin_index` — backend walk of 93,349 keys, parsing each
   value's `{token, freq}` records into per-key `Vec`s.
3. `derive_pinyin` — the eager derivation pass:
   - `InitialAlphabet` pack/sort/unpack of 93,349 keys into
     `Box<[String]>` (45,404 initial keys, one `String` each);
   - `BTreeMap<u32, u64>` unigram aggregation over every record;
   - `resolve_hits` — a `PhraseEntry` per record (411,679 records ×
     ~48 B) into one entry arena, each with the phrase text re-cloned
     inline and the token's aggregate total binary-searched and
     attached;
   - `Vec<(Box<str>, Range)>` row table (one boxed string per key).
4. The reverse text→tokens map stayed lazy (`OnceLock`, unchanged).

That is four derived representations of one logical dataset (store
rows + decoded rows + derived indexes + resolved entries) — the
duplication the prompt's §11 audit predicted, and the ~66.8 MiB
anonymous post-init RSS of the matrix run.

## 3. The libpinyin reference (verified against the pinned source)

Read directly from `libpinyin-2.11.91` (pin of
`docs/testing/oracle-environment.md`):

- `pinyin.cpp` `_load_phrase_library`: the four system phrase
  libraries are `MemoryChunk::mmap(system_dir/<name>.bin)` —
  `src/include/memory_chunk.h` maps the whole file
  `PROT_READ|MAP_PRIVATE`, validates a `{length, checksum}` header,
  and hands out a pointer.
- `phrase_index.cpp` `SubPhraseIndex::load` wraps **three sub-chunk
  views** over the mapped bytes (indirect offset array, phrase
  content) — no per-record construction. `get_phrase_item` is one
  offset read plus a `set_chunk` view; `get_range` reads the offset
  array's tail.
- The chewing/pinyin index and the phrase *table* are DBMs
  (`pinyin_index.bin`/`phrase_index.bin` are TreeDB/TreeDBM files in
  2.11.91, opened lazily — `open()` never walks the tree), and
  `bigram.db` is the one backend database. That is why libpinyin init
  is sub-millisecond with KC: **no init-time walk of any table**, and
  the mapped libraries page in lazily behind a one-pass checksum.

The architectural transfer to oxpinyin is the phrase-library pattern:
**an immutable compiled file, mapped once, validated once, read by
offset forever** — applied to both of oxpinyin's system indexes
(whose data model is two tables rather than per-library items).

## 4. What changed

New format ("sysimage", normative spec:
`crates/oxpinyin-core/src/sysformat.rs`):

- `phrase_index.bin` — header (magic/version/n_tokens/
  `unigram_total`/section offsets/`pair_hash`), ascending `u32` token
  array, parallel entry-offset array, then the entry area:
  `{text_len: u32, unigram: u64, text: UTF-8}` per token. The
  per-token aggregate and the total are exactly the eager loader's
  `BTreeMap<u32, u64>` / `unigram_total`, precomputed.
- `pinyin_index.bin` — header, key-offset array into one key blob
  (strictly ascending, `'`-joined spellings), record-offset array into
  one record area of 12-byte `{token, freq, entry_off}` records
  (`entry_off` points **into the paired phrase image's entry area**),
  and the sorted initial-projection key list. The initial keys are the
  `InitialAlphabet` projection, precomputed.
- `pair_hash` (FNV-1a 64 over the canonical phrase content) is stored
  in both headers; `open` refuses a mismatched pair.

New code:

- `oxpinyin-data/src/image.rs` — the reader. `MappedFile` (Unix
  `mmap` via two `extern "C"` declarations — no new dependency; the
  constitution's documented mmap exception, the only `unsafe` in the
  crate, isolated in `image::map` with `// SAFETY:` blocks; non-Unix
  builds read once into a heap buffer behind the same views), header +
  section + ordering validation at open, then O(1) `trusted()` views
  per access. Per-record accesses re-check bounds and UTF-8; a
  malformed image answers "no hit", never panics.
- `oxpinyin-datagen/src/image.rs` — the writer. A pure function of the
  same compiled `Entries` the store writers receive (both
  representations from one row set — they cannot drift), emitting for
  the system pair and every addon library. Deterministic and
  input-order-independent (canonicalised).
- `SystemDictionary` now holds the two mappings and answers through
  the views: `lookup` is one byte-compare binary search for the row +
  per-record `{entry_off → text, unigram}` resolution into
  `PhraseEntry` (the API contract — `Vec<PhraseEntry>` — is
  unchanged); the `SEARCH_CONTINUED` probes binary-search the row keys
  and the initial list on raw bytes (byte order = UTF-8 `str` order,
  so probes need no decode). The lazy reverse map and its
  `suggest_after` order are unchanged. `unigram_map()` (a
  `&BTreeMap`) became `unigram_len()` + `unigrams()` over the image —
  the three call sites (fixture-mode LM seed, visibility scan, one
  e2e test) moved with it.
- `InitialAlphabet` moved from `oxpinyin-data` to `oxpinyin-datagen`
  (initials.rs): the projection is now a compile-time derivation, the
  same class of move libpinyin made when it compiled the incomplete
  index into its DBM.
- Runtime/addon loading opens `pinyin_index.bin`/`phrase_index.bin`
  (backend-independent); the bigram, punct, user store, and
  interpolation2.text are deliberately untouched (follow-up scope).
- The store `pinyin_index`/`phrase_index` tables are still emitted for
  the trainer tooling that reads them (`oxpinyin-segment`,
  `oxpinyin-emitter`, the parity harness's export dirs); installs
  carry both until that tooling migrates (named as the follow-up).
  Installed-size effect (matrix, same measurement as #260): oxpinyin-KC
  runtime data 112.12 → 120.82 MiB, total 140.34 → 148.93 MiB (+8.7 MiB
  = the `.bin` images, system + addons; the store pair it replaces in
  the runtime's eyes stays on disk until the trainer migration).

## 5. On-disk/runtime representation

Full model, system pair: `pinyin_index.bin` 4,145,804 B +
`phrase_index.bin` 3,856,044 B ≈ **7.6 MiB**, shared page cache,
file-backed. The four heap structures of §2 (row table, entry arena,
unigram BTreeMap, initial-key `Box<[String]>`) are gone; the resident
state is the two mappings plus the lazy reverse map the prediction
surface builds on demand (unchanged behavior).

## 6. Initialization before/after

Component breakdown, `load_profile` (same host, release, redb, full
frozen export at `/tmp/oxpinyin-export`; median of 5 / three fresh
process runs):

| Step | before | after |
| --- | ---: | ---: |
| `SystemDictionary::open` (isolated median) | 31.8 ms | **0.5 ms** |
| `SystemDictionary::open` (cumulative, fresh process) | 59.3 / 63.6 / 70.8 ms | **0.3 / 0.2 / 0.2 ms** |
| `BigramLanguageModel::open` (untouched) | 13.4–25.3 ms | 9.2–10.4 ms (noise) |
| `set_unigrams_from_interpolation2` (untouched) | ~8 ms | ~6.5 ms |

The dictionary side of init is now the two `open(2)`+`mmap(2)` pairs
plus header/section/ordering validation — the libpinyin shape. What
remits of oxpinyin's init is the bigram slurp and the
`interpolation2.text` parse, both explicitly out of scope here and
both already named by the matrix doc as the next targets.

Docker matrix (the #260 harness, same container image recipe, same
CPU pinning, PERF_RUNS=20 / PERF_CYCLES=8 / PERF_RAM_RUNS=10; the
libpinyin cells re-measured as controls: init 0.864/1.108 ms against
#260's 0.854/0.920 ms — same sub-millisecond regime, comparison
valid):

| Cell | init before (#260) | init after | reduction | gap vs libpinyin |
| --- | ---: | ---: | ---: | ---: |
| oxpinyin + KC | 86.188 ms | **21.669 ms** [20.804, 68.362] | **4.0×** | 93.7× → **19.6×** |
| oxpinyin + Tkrzw | 106.383 ms | **23.472 ms** [22.267, 49.818] | **4.5×** | 124.5× → **27.2×** |

The 19–27× that remains is no longer the dictionary: §11's two
untouched eager loads (bigram slurp, `interpolation2.text`) now
dominate `pinyin_init`.

## 7. RSS before/after

Local process RSS is unavailable on macOS (`/proc`); the Docker
matrix's `ram-init` capture (smaps-derived RssAnon/RssFile) is the
evidence of record. Expected and measured direction: anonymous RSS
falls by the §2 structures (the matrix's 66.8 MiB RssAnon was
dictionary-dominated), file-backed RSS rises by the mapped pair as
pages fault in (bounded by 7.6 MiB), and the total lands near
libpinyin's profile rather than 4–5× it.

| Cell | RssAnon before | RssFile before | RssAnon after | RssFile after |
| --- | ---: | ---: | ---: | ---: |
| oxpinyin + KC | 66,798 KiB | 6,112 KiB | **38,302 KiB** | **10,840 KiB** |
| oxpinyin + Tkrzw | 62,200 KiB | 7,056 KiB | **37,012 KiB** | **11,716 KiB** |

Post-init RSS: KC **72,910 → 49,136 KiB** (−32.6%), Tkrzw
**69,256 → 48,728 KiB** (−29.6%). The composition moved exactly the
way the architecture predicts: ~24 MiB of anonymous heap became
file-backed pages (RssFile +4.7 MiB — the mapped pair and the pages
the first probes touched, shared with the page cache), and anonymous
RSS dropped 40–43%. The remaining ~37 MiB of RssAnon is the bigram
slurp (~15 MB of value bytes plus its typed map) and the
`interpolation2.text` unigram table — §11's targets.

## 8. Steady state before/after

`load_profile lookups` (same host/build as §6; 5000 lookups over five
hot shapes — 125/1/1/337/27-hit rows — plus 5000 `SEARCH_CONTINUED`
probes, median):

| | before | after |
| --- | ---: | ---: |
| 5000 lookups + 5000 probes | 2.3 ms | 3.8 ms |

The hit loop pays for the offset-resolved text/unigram reads that the
pre-resolved arena used to memcpy (~3 ns per hit at this scale). The
probes are byte-compare binary searches, the same cost class as
before. The full keystroke cycle (the #260 metric) is dominated by
parse/n-best/scoring — the matrix rerun is the acceptance evidence:

| Cell | steady before (#260) | steady after | vs libpinyin after |
| --- | ---: | ---: | ---: |
| oxpinyin + KC | 8.473 ms (1.059×) | **9.051 ms** | 1.099× |
| oxpinyin + Tkrzw | 8.932 ms (1.112×) | **9.153 ms** | 1.107× |

The KC cycle carries a **+6.8%** regression and Tkrzw **+2.5%** — the
cost of resolving hit texts and unigrams through the mapped phrase
image instead of memcpy'ing the pre-resolved arena (§8's microbench
isolates it at ~3 ns/hit; the cycle amplifies it across every prefix
of every path of every keystroke). Disclosed as the accepted trade: a
4–4.5× init reduction and −24 MiB anonymous RSS against ≤7% on one
cell's steady cycle. If the maintainer wants the arena's cycle back
without the heap, the contained follow-up is an on-demand per-row
resolved cache (session-scoped, bounded by the touched rows) — noted,
not attempted here.

## 9. Correctness validation

- Reader unit tests (image.rs): empty image, single/multiple rows,
  exact + prefix + initial probes, truncation at every section
  boundary, bad magic/version, non-back-to-back sections, ascending
  violations, out-of-bounds and mid-UTF-8 entry damage (answers None,
  never panics), sentinel offsets, `usize::MAX` record ranges, and
  `MappedFile` round-trip/empty/missing files.
- Emission tests (datagen `image_emission.rs`): round-trip through the
  real `SystemDictionary::open` (contents, order, pronunciation
  possibility `(8, 8)`/`(5, 5)`, unigram view/total, all three probe
  shapes), byte-determinism and input-order independence, the
  eager-loader drop rule (a record whose token has no phrase entry is
  invisible; its frequency still aggregates), mismatched-pair refusal,
  and schema violations as emission errors.
- `fixtures_identity` (strict, model20): a fresh mini compile's
  sysimage pair is **byte-identical** to the committed
  `fixtures/w3/*.bin`; the store tables still match row-for-row.
- `export_reference` (strict): the full compile still reproduces the
  frozen oracle-derived export 93,349/138,096/56,359 row-for-row, so
  the emitted images derive from proven-identical rows.
- Whole-workspace suites green under redb (macOS) and the default KC
  build + capi e2e in the matrix container (Linux); the C-ABI parity
  differentials are the maintainer's local gates as before (they run
  against datagen output, which now carries the images).

Behavioral notes (no observable change for datagen-produced data —
both are invariants of the shared row set; hand-built images can
differ only under corruption, where the rule is "no hit, no panic"):

- a token present in `phrase_index` but referenced by no pinyin record
  reads `unigram_count == 0` (the eager map had no entry → `None`);
  datagen cannot produce this (phrases are built from index rows).
- key UTF-8 validity is checked where keys become `&str`
  (`pronunciations`, reverse map); the probe paths compare bytes, so a
  corrupt non-UTF-8 key can only ever match a byte-identical probe.

## 10. Trade-offs

- The runtime now requires the `.bin` pair next to the store tables
  (datagen emits both; `Runtime::open` refuses a dir without them).
- Installs carry the store `pinyin_index`/`phrase_index` alongside the
  images until the trainer tooling migrates (a few MiB of dead weight,
  disclosed in the size table; the migration is a contained follow-up).
- Steady-state hit resolution is ~1.7× the pre-resolved arena on a
  synthetic hit-heavy loop; the full-cycle cost is the matrix's
  +6.8% (KC) / +2.5% (Tkrzw) — accepted against the 4–4.5× init win
  and the RSS drop, with the bounded per-row cache named as the
  recovery if the maintainer wants it.

## 11. Remaining bottlenecks (in measured order)

1. **`BigramLanguageModel::open`** — the remaining eager store walk
   (15 MB of value bytes, ~10–25 ms). The same sysimage treatment
   applies; it is the natural next PR.
2. **`interpolation2.text`** — 79.6 MiB shipped + ~6.5 ms parse at
   init; compiling the 1-gram section into a sidecar at datagen time
   (as the matrix doc already recommends) removes both.
3. **Store-table dual emission** — migrate `oxpinyin-segment` /
   `oxpinyin-emitter` to read the phrase image, then stop emitting the
   store pair (install size win).

## 12. Next optimization candidate

The bigram sysimage: same architecture, one more file, no behavior
change — and by §6 it is now the dominant term of what remains.
