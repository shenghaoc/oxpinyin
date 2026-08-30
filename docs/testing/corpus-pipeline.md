# Corpus pipeline — zhwiki dump → ngseg raw text (W9-T4b)

Date: 2026-08-15 · Status: **tier-1 invariants green; tier-2 end-to-end
differential value-identical against the pin-built 2.11.91 trainer
chain** · Crate: `oxpinyin-corpus` (never ships).

This is the corpus front-end of the W9 trainer. It converts a
Chinese-Wikipedia XML dump into the line-oriented raw text that
`oxpinyin-segment` (the `ngseg` reproduction, W9-T1) consumes — every line
contains Han and the output is predominantly Han text with standard
Chinese punctuation, but Latin names and digits pass through too — so the
full chain

```text
zhwiki dump → [T4b clean] → T1 segment → T2 count → T3 λ → T4a emit
                                                        interpolation2.text
```

runs end to end with zero glue. **This stage has no libpinyin
differential oracle.** libpinyin's corpus preparation was Peng Wu's
private, undocumented pipeline (established at W9-T0:
`interpolation2.text` carries no corpus attribution), and `ngseg`
consumes raw text without producing it from a dump. The cleaning step is
ordinary data engineering and is verified as such — by well-formedness
invariants (tier 1) and by the end-to-end differential at the far end of
the chain (tier 2). No oracle is fabricated for the cleaning itself.

## Input model

- Source: a real zhwiki export — `Special:Export` output or a
  `dumps.wikimedia.org` pages-articles slice after `bzip2 -dc`. The
  input is decompressed XML (`export-0.11`), streamed one `<page>` at a
  time (memory bounded by the largest page, not the dump size).
- The pipeline is committed; the corpus is not. No dump, no large
  corpus, no model bytes, no `table.conf` are vendored. The committed
  test sample is 818 bytes of cleaned raw text derived from three real
  articles, CC BY-SA 4.0 attributed (see `fixtures/w9/README.md`).
- `--pages N` produces a bounded sample of a large dump.

## Crate placement

New crate `crates/oxpinyin-corpus`, sibling to the four W9 trainer crates
(`oxpinyin-segment` / `oxpinyin-counter` / `oxpinyin-lambda` /
`oxpinyin-emitter`). W9 maps each trainer stage to its own crate; the
corpus front-end is the first stage, and a dedicated crate keeps the
seam small. `unsafe`: deny. Portable. `publish = false`. Never ships
with the engine.

## Dependencies

**Zero Cargo dependencies.** Each stage was weighed against a crate and
a hand-roll or a binary won:

| Stage | Choice | Why |
|---|---|---|
| XML `<page>` extraction | hand-rolled two-tag byte scanner (`xml.rs`) | a `<page>`/`<text>` scan does not justify a parser dependency |
| markup stripping | hand-rolled two-pass cleaner (`markup.rs`) | rules are small and testable; no extractor crate needed |
| trad → simp | shell to the **`opencc` binary** (Apache-2.0), config `t2s` | OpenCC is the standard, permissively licensed converter; shelling adds zero Cargo dependencies and keeps the crate portable. The runtime requirement is one documented executable (`--opencc` overrides, `--no-convert` skips) |
| sentence split | hand-rolled (`split.rs`) | a punctuation scan |

## What the cleaner strips / converts / splits

### XML layer (`xml.rs`)

Streams `<page>` blocks; per page extracts `<title>`, `<ns>`, `<text>`
with XML entity decoding (`lt gt amp quot apos` + `&#N;`/`&#xH;`).
Pages are skipped when: `ns != 0`, the title contains `列表` (list
articles are tables end to end), the text is missing, the text starts
with a redirect marker (`#REDIRECT` / `#重定向` / `#重新導向`), or the
text carries a disambiguation template (`{{disambig…}}` /
`{{消歧义…}}`).

### Markup layer (`markup.rs`)

Pass 1 deletes `{| … |}` table blocks wholesale, tracking template
depth so `{|`/`|}` inside `{{…}}` arguments (including `|}}` tails)
cannot be mistaken for table delimiters. Pass 2 strips, on the char
stream:

- HTML comments `<!-- -->`.
- Paired tags with **dropped content**: `ref`, `references`, `gallery`,
  `math`, `chem`, `score`, `timeline`, `mapframe`, `maplink`, `graph`,
  `hiero`, `inputbox`, `categorytree`, `includeonly`, `noinclude`,
  `onlyinclude`, `pre`, `code`, `tt`, `syntaxhighlight`, `source`,
  `imagemap`, `choose`, `quiz`, `poll`. Self-closing forms are dropped
  too; `br` becomes a line break; other tags keep their content.
- Templates `{{…}}` (nested-aware): dropped, except the **keep-list**
  `lang` / `lang-*`, whose last `|`-separated argument is the displayed
  text and is kept (and re-stripped). `{{PAGENAMEBASE}}` /
  `{{PAGENAME}}` resolve to the page title so lead sentences stay
  complete.
- Links `[[target|display]]` → display text (target when no `|`); links
  to `File:`/`Image:`/`Category:` (and `文件:`/`檔案:`/`分类:`/
  `分類:`) are dropped.
- External links `[url display]` → display text; bare `[url]` and
  citation markers `[12]` dropped; stray `[` stays literal.
- `'''`/`''` emphasis markers (any run ≥ 2 quotes).
- Heading `=+` markers (leading and trailing), line-start list markers
  `* # : ;`, `__MAGIC__` words.
- Named/numeric HTML entities (the XML layer has already decoded one
  level; wikitext `&nbsp;` etc. is decoded here).
- Empty parenthesis pairs left behind by dropped templates (`（）`,
  `（ ，）`).

The stripper consumes *balanced* markup. Source pages with unbalanced
braces are real (a few in the 300-article slice); the sentence layer's
`MARKUP_RESIDUE` net (`{{ }} [[ ]] {| |} <ref`) drops any fragment that
still carries such syntax, so `ngseg` never sees it.

### Conversion layer (`convert.rs`)

`opencc -c t2s`, stdin→stdout, per page. The child's stderr is
**inherited, never piped**: a converter writing more than a pipe buffer
before exit would deadlock the parent's `wait`, so diagnostics go to the
terminal and errors report only the exit status. The stdin writer runs on
a thread so a large page cannot deadlock against a full stdin buffer.
Missing binary or nonzero exit → `CorpusError::Convert`
(never a silent skip). `--no-convert` skips the stage for
already-simplified corpora. The committed XML fixture is
all-simplified, so CI runs without opencc and the conversion leg is
tested separately when an `opencc` binary exists
(`PINYIN_OPENCC` / `PATH`).

### Sentence layer (`split.rs`)

Splits on `。！？；…` and newlines; runs of boundaries emit nothing.
Each fragment is trimmed (whitespace and a leading/trailing punctuation
set), must contain at least one Han character (BMP unified CJK,
extension A, compatibility, and supplementary planes), must not contain
markup residue, and is written as one `\n`-terminated line — exactly
the shape T1's `getline` reads (one `\n` stripped per line, never
`\r`).

## The join: T4b → T1 with zero glue

Confirmed via the LSP (rust-analyzer hover on
`oxpinyin_segment::Segmenter::segment_bytes`):
`pub fn segment_bytes(&self, input: &[u8], extra_enter: bool) -> Result<String, SegmentError>`
— line-oriented UTF-8, one sentence per line. T4b's writer emits
exactly that byte shape (asserted in `tests/invariants.rs`), and the
tier-2 test feeds the committed sample bytes into `segment_bytes` with
no transformation — the same bytes are fed to pin `ngseg`, and the two
segmentations are asserted byte-identical before any counting.

## Verification

### Tier 1 — cleaning-stage invariants (no oracle, CI-unconditional)

`crates/oxpinyin-corpus/tests/invariants.rs` asserts on the committed
sample:

- every line non-empty, trimmed, `\n`-only line endings, no `\r`;
- every line contains ≥ 1 Han character;
- no residual markup syntax (`[[ {{ {| }} ]] |} <ref <!-- & http ==`);
- simplified spot-check — an unambiguous-traditional character list
  (`國 學 體 機 發 …`) flags any retained traditional character, which
  is the spot-check the task asks for (the exhaustive check would need
  the OpenCC dictionary);
- the cleaner reproduces the sample from the committed XML fixture
  **bit-exactly** (3 articles → 16 sentences, pinned), so the strip
  rules cannot drift silently.

These are internal correctness checks, not a differential — libpinyin
has no tool to diff against. Stated plainly, not papered over.

### Tier 2 — end-to-end differential (the real oracle, env-gated)

`crates/oxpinyin-corpus/tests/differential.rs`:

- `rust_chain_consumes_t4b_sample_with_zero_glue` — T4b sample → T1 →
  T2 → T3 → T4a in-process (skips only when the migrate export /
  model20 cache are absent).
- `end_to_end_matches_live_libpinyin_pipeline` — gated on
  `PINYIN_NGSEG` + `PINYIN_NGSEG_DATA` + `PINYIN_GEN_BINARY_FILES` +
  `PINYIN_GEN_UNIGRAM` + `PINYIN_GEN_NGRAM` + `PINYIN_GEN_DELETED_NGRAM`
  + `PINYIN_ESTIMATE_INTERPOLATION` + `PINYIN_EXPORT_INTERPOLATION` +
  `PINYIN_GEN_NGRAM_DATA` (the same gates the T1–T4a tasks use); runs
  `ngseg → gen_binary_files → gen_unigram → gen_ngram →
  gen_deleted_ngram → estimate_interpolation → export_interpolation`
  and asserts:
  - T1(`segment_bytes`) ≡ pin `ngseg` **byte-identical** on the sample;
  - `interpolation2.text` **value-identical** (both n-gram sections,
    `parse_interpolation_dump` comparison — T4a's documented
    value-level convention, since the pin's tkrzw walk order differs);
  - λ per-context **byte-identical at the six decimals
    `estimate_interpolation` prints**, average `|Δ| < 1e-6` (T3's
    stated tolerance, `lambda-port.md`).

**Live result (pinned 2.11.91, built by `tools/oracle/build-oracle.sh`
from the verified archives):** ngseg bit-identical; 138 096 unigrams +
132 bigrams value-identical; 86 λ contexts byte-identical at 6 dp,
average λ 1.000000, `|Δ| = 2.35e-9`.

**Wikipedia-scale run (local evidence, same pinned build, not
committed):** the 300-article slice (1.3 MB of wikitext) cleaned to
4 431 lines / 443 KB, then both chains: T1 96 995 segmentation records
byte-identical to `ngseg`; 138 096 unigrams + 53 864 bigrams
value-identical; λ stdout byte-identical (average 0.999999).

### λ saturation note

The e2e configuration feeds the **same** stream to the system bigram
and the held-out (`gen_deleted_ngram`) side — the maximal-overlap
held-out configuration, `oxpinyin-lambda`'s own default. Every held-out
bigram was therefore seen in training, so per-context λ saturates at
1.000000 on both sides; the λ check is exact but degenerate. The
non-degenerate λ arithmetic (mixed hit/miss contexts, λ across (0, 1))
is already differentially verified by W9-T3's own Config X gate
(`crates/oxpinyin-lambda/tests/differential.rs`); this stage must not
re-invent it.

## Licensing

The committed `fixtures/w9/corpus-xml.xml` (three real article page
blocks, verbatim) and `fixtures/w9/corpus-sample.txt` (its cleaned
output) are derived from zh.wikipedia.org text, licensed **CC BY-SA
4.0**; attribution is recorded in `fixtures/w9/README.md`. Sizes (6 KB
/ 0.8 KB) are insubstantial excerpts kept only for the test. No dump,
no corpus, no model bytes are committed.

## Known limits (accepted, documented)

- Heading-only fragments (`参考资料`, `行政区划`…) survive as
  one-word lines; they are Han text and harmless to n-gram counting.
- Unbalanced source markup is dropped fragment-wise by the residue net;
  a bare `|` from mangled table syntax can survive on pathological list
  pages (3 lines in the 4 431-line scale run; list pages are skipped
  by title where the title says so).
- `{{lang|…}}` keep-list resolution keeps the *last* argument; exotic
  inline templates are simply dropped.
- Horizontal-rule runs (`----`) are not stripped by the markup layer: a
  lone rule line vanishes in the sentence layer (no Han character), but
  dashes embedded in Han text (`前----后`) survive into the output.
- `t2s` conversion quality is OpenCC's, not this crate's; the pipeline
  only guarantees that whatever `opencc` emits is what goes forward.

## Source index

| Claim | Source | Tag |
|---|---|---|
| T1 input type `segment_bytes(&[u8], bool)` | `crates/oxpinyin-segment/src/lib.rs:101` (LSP hover) | SHOWN |
| T1 line rule: one `\n` stripped, never `\r` | `crates/oxpinyin-segment/src/driver.rs:143-164` | SHOWN |
| T2 consumes T1's stdout text | `oxpinyin_counter::count_ngseg` (LSP hover) | SHOWN |
| T4a renders `interpolation2.text` | `oxpinyin_emitter::emit_interpolation2` (LSP hover) | SHOWN |
| λ tolerance: 6 dp byte-identical, `\|Δaverage\| < 1e-6` | `docs/findings/lambda-port.md` §4.3 | SHOWN |
| libpinyin corpus prep undocumented; no oracle | W9-T0 `training-algorithm.md` | SHOWN |
