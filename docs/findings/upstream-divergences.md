# Upstream divergences

Purpose: a register of behaviours that oxpinyin cannot or deliberately does
not reproduce because of a Rust language mechanism or a policy-accepted
GLib/glibc/C++ runtime quirk. Source policy permits
reading and copying upstream; this file is for the residue. Once the rewrite
is complete, these notes are collected to report back to libpinyin.

A new language/runtime-quirk entry also needs the full record from
`docs/findings/language-runtime-discrepancy-template.md`; this register
keeps the short entry and links the record. Class (a)–(b) entries keep
their existing shape. No existing entry is rewritten by a later policy
change. An entry whose own issue reproduces the behaviour is closed in
place with a dated status; the historical finding stays as the record of
what used to differ.

## Entry template

```markdown
### <short name>

- **Upstream source cite:** `path:lines` in the pinned libpinyin source.
- **Mechanism:** what the C++ does.
- **What oxpinyin does instead:** the Rust behaviour.
- **Externally observable:** yes/no and how a caller would see it.
- **Record and disposition (quirk entries only):**
  `docs/findings/<record>.md`; contract-diff case `<id>`, disposition
  `POLICY-ACCEPTED`.
```

## Register

### Unloaded-token pronunciation uses no cached item

- **Upstream source cite:** `phonetic_lookup.h:924–929` at `074a2219`.
- **Mechanism:** a failed item fetch leaves the context's shared lookup cache
  in place; training can mutate a different loaded item or access freed bytes.
- **What oxpinyin does instead:** skips the unloaded token's pronunciation
  step, preserving bigram training, the constraint walk, modified flag and result.
- **Externally observable:** yes; the defined witness saves 甲's pronunciation
  count as 79 in the pin and leaves it at 10 in oxpinyin.
- **Record and disposition:** [compatibility policy](compatibility-policy.md),
  amendment “Unloaded-token pronunciation cache”, ruling 2026-11-05 UTC;
  `unloaded-cached-pronunciation-pinyin` and
  `unloaded-cached-pronunciation-zhuyin`, `POLICY-ACCEPTED`.

### Non-scalar stored phrase text — CLOSED, ordinary MATCH (#894)

- **Record:** `894-non-scalar-phrase-text`
  (dated superseded note, 2026-10-24 UTC).
- **Former classification:** decision-order limb 3, not class (b) merely
  because Rust `str` cannot contain a non-scalar. GLib 2.84.4-3~deb13u5
  (trixie) emits extended UTF-8 for surrogates and U+110000, as the
  2.89.4-2 testing build did; the old Rust path refused the item.
  Re-read 2026-10-25 UTC.
- **Status:** **CLOSED (2026-10-24 UTC, #1182).** The stored UCS-4 words
  are carried as words (`PhraseText` in
  `crates/oxpinyin-core/src/ucs4.rs`; the chunk reader in
  `crates/oxpinyin-data/src/phrase_library.rs`; the candidate, sentence
  and export paths) and encoded with GLib's rule in natural Rust at the
  C boundary (`phrase_text_bytes` in `crates/oxpinyin-capi-marshal`).
  The former POLICY-ACCEPTED marker is removed; the ten cases are
  ordinary MATCH cases. The zhuyin export-symbol absence checks remain
  MATCH. The cases use system data; malformed user-library items are
  tracked in #1198.

### Embedded zero in stored phrase text — CLOSED, ordinary MATCH (#894)

- **Record:** `894-nul-phrase-text` (dated
  superseded note, 2026-10-24 UTC).
- **Former classification:** decision-order limb 3. GLib truncates each
  phrase at zero; the old Rust path retained the full internal text.
- **Status:** **CLOSED (2026-10-24 UTC, #1182).** The outward text ends
  at the first zero word, exactly as `g_ucs4_to_utf8` does, while the
  stored length still prices the item; candidate, sentence and export
  bytes match. The former POLICY-ACCEPTED marker is removed; the five
  cases are ordinary MATCH cases. The cases use system data; malformed
  user-library items are tracked in #1198.

### Bigram export iterator's pinyin buffer

- **Restated 2026-09-27 UTC (#530, audit D-20; policy row 1).** The
  original record below called this a stale-buffer reuse that crashes on
  a *repeated* export cycle. Re-read at `074a2219`, the defect is an
  unterminated string vector, and it crashes the *first* export cycle
  after a train.
- **Upstream source cite:** `src/pinyin.cpp:842-872`
  (`pinyin_bigram_iterator_has_next_phrase`): `:844-850` and `:857-863`
  build each pronunciation with `g_ptr_array_new`, free the array with
  `g_ptr_array_free(…, FALSE)` and pass the result to `g_strjoinv` and
  `g_strfreev`; nothing adds the terminating NULL. The unigram export
  does add it (`:726-734`, `g_ptr_array_add(array, NULL)` at `:730`).
- **Mechanism:** `g_strjoinv`/`g_strfreev` walk a `gchar **` until a
  NULL; the array has none, so both read past its end into whatever
  the heap holds there — an out-of-bounds read whose outcome depends on
  the allocation's neighbourhood. Any export that reaches a row with a
  pronunciation takes the path.
- **What oxpinyin does instead:** `CapiContext::export_bigram_rows`
  renders every row into owned Rust strings when the iterator is created
  (`crates/oxpinyin-capi/src/iterators.rs:576`), so no join over a C
  array exists to over-read, on the first cycle or any later one.
- **Externally observable:** yes — the audit's execution
  (`bigram-before-save.c`): after one train, the first export cycle
  SIGSEGVs in `pinyin_bigram_iterator_has_next_phrase` on tkrzw and kc
  whether or not the user dir was saved, and on bdb unsaved (4 of 4
  runs); oxpinyin completes the cycle. Class (b): the pin's behaviour is
  undefined, and no safe construction reproduces a read past an
  allocation. Also cross-indexed in `reference/memory-safety-bugs.md`.
- **Original record (2026-08, superseded in mechanism and scope by the
  restatement above):** the export iterator keeps C pointers into
  reused pronunciation/join buffers; repeating an export cycle inside
  one context reuses stale storage and the pinned oracle segfaults.
  The per-round train differential runs one export per fresh context
  for the oracle and compares those rows to oxpinyin
  (`tools/bisection/run-train-diff.sh`).

### Chewing auxiliary substring over-read

- **Upstream source cite:** `src/pinyin.cpp:3555-3560`
  (`pinyin_get_chewing_auxiliary_text`): after `len = cursor - begin`
  (`:3556`), `g_utf8_substring(zhuyin, len, end)` (`:3558`) passes the raw
  byte delta as `start_pos` and `m_raw_end`, an **absolute** raw-input
  offset, as `end_pos`. GLib 2.89.4-2 `g_utf8_substring`
  (`glib/gutf8.c:298-322`) resolves
  `start = g_utf8_offset_to_pointer(str, len)` and
  `end = g_utf8_offset_to_pointer(start, end_pos - len)`;
  `g_utf8_offset_to_pointer` (`glib/gutf8.c:376-386`) steps
  `g_utf8_next_char` `end_pos - start_pos` times with no terminator check.
- **Mechanism:** the second call asks for `m_raw_end - len` scalars from
  scalar `len`, not for the `C - len` scalars remaining. `m_raw_end > C` for
  essentially every interior cursor (a later key's absolute end exceeds `C`;
  a first key's end is its raw span, at least `C` for every splittable
  content row and usually larger), so the call is outside GLib's documented
  domain and the walk passes the NUL. The copy reads beyond the allocation
  once the end pointer is at least one byte past the NUL
  (`m_raw_end >= C + 2`); for `nihao@3` (`hao`, `C = 2`, `len = 1`,
  `m_raw_end = 5`) it walks three scalars past and copies the NUL plus the
  bytes after it. While the pin returns, `g_strconcat` sees the copied NUL
  and the visible right half is the spelling from `len` to `C` (empty at
  `len == C`). For `len > C` the copy starts past the NUL, so the visible
  right half is heap bytes. The over-read can fault.
- **What oxpinyin does instead:** `crates/oxpinyin-capi/src/aux_matrix.rs`
  `split_zhuyin` cuts the spelling at scalar `len` and returns an empty
  right half when `len` is at or past the scalar count; the zhuyin-parser
  branch in `crates/oxpinyin-capi/src/text.rs` `chewing_aux_text` uses the
  same cut. For `len <= C` its bytes are the pin's visible right half.
- **Externally observable:** for an interior cursor the visible bytes match
  only when the pin returns; the difference is the absent out-of-allocation
  read and possible fault, plus the out-of-domain walk at `m_raw_end == C + 1`.
  For `len > C` the visible bytes differ (heap vs empty). Class (b): no safe,
  deterministic reproduction; no differential case reproduces the read.
- **Record:** [G2 closure](glib-libc-verification.md#g2--c-strings-joins-and-vectors);
  verified 2026-10-25 UTC on bdb.

### Unterminated punctuation value over-read

- **Upstream source cite:** `src/storage/punct_table.cpp:57-72`
  (`PunctTableEntry::unescape`) and `:74-94` (`get_all_punctuations`),
  reached from `pinyin_guess_predicted_candidates_with_punctuations`
  (`src/pinyin.cpp:2474`). The disassembly of the bdb reference build
  (`unescape` at `0x72020`, `get_all_punctuations` at `0x72080`) has the
  same control flow.
- **Mechanism:** `unescape` appends `punct[index]`, increments `index`,
  then reads `punct[index]` to test for the terminator before the loop
  checks `index < maxlen`. When a run reaches the value's last whole word,
  that read is one `ucs4_t` past the value. A zero breaks the loop and any
  other word ends it at the loop test, so both outcomes return
  `index + 1`: the run, and so the visible punctuation, does not depend
  on the word read. `get_all_punctuations` passes `end - begin` as
  `maxlen`, an arithmetic right shift of the byte difference, and
  compares `begin < end` on bytes, so a value whose length is not a
  multiple of four is walked as its whole words, with one more empty
  punctuation (`maxlen` zero) when 1–3 bytes remain after a terminated run.
- **What oxpinyin does instead:** `crates/oxpinyin-data/src/row_format.rs`
  `punct::decode_puncts` ends the run at the value's last whole word without
  reading past it, and walks a misaligned tail as the pin's compiled walk
  does. Its result is the pin's visible result.
- **Externally observable:** only when the pin's read faults, which needs
  the copied value to end where mapped memory ends. Otherwise the rows
  are identical. Class (b): the out-of-bounds read and its possible fault
  are not reproduced.
- **Cases:** `punct-decode/unterminated`, `punct-decode/unterminated-zero`,
  `punct-decode/misaligned-terminated`,
  `punct-decode/misaligned-unterminated` and `punct-decode/misaligned-short`
  of `contract-diff.py` MATCH on bdb (#1144, 2026-10-26 UTC).

### Aliased prediction prefix use-after-free

- **Upstream source cite:** `src/pinyin.cpp:2413-2427`
  (`pinyin_guess_predicted_candidates`): `_free_candidates` (`:1335-1344`)
  `g_free`s every candidate string and empties the list (`:2420`),
  `m_prefixes` is set to size 0 (`:2423`), then `_compute_prefixes` reads
  `prefix` with `g_utf8_to_ucs4` (`:2424`, `:1389-1398`).
  `pinyin_guess_predicted_candidates_with_punctuations` makes that call
  first and ignores its answer (`:2459`), walks `m_prefixes` (`:2463-2486`)
  and returns `true` (`:2498`).
- **Mechanism:** when `prefix` is one of the instance's candidate strings
  (`pinyin_get_candidate_string`, `:2876`), `_compute_prefixes` reads
  freed storage. Defined: the emptied list, the emptied `m_prefixes`
  before the read, untouched sentence rows, and the punctuation entry's
  `true`. Undefined: the prefixes computed from the freed bytes, and so
  the predicted and punctuation rows.
- **What oxpinyin does instead:** `crates/oxpinyin-capi/src/predict.rs`
  `clear_borrowed_prefix` finds the alias by pointer identity before
  reading it, empties the candidates and prefixes, and gives #542's
  no-prefix outcome. The plain entry answers `false`; the punctuation
  entry (`crates/oxpinyin-capi/src/sentence.rs`) answers `true` with an
  empty list, since an empty `m_prefixes` prepends nothing.
- **Externally observable:** only where the pin's freed bytes would decode
  to a prefix with phrase-table tokens, or the read faults. Class (b): the
  read is not reproduced. The punctuation entry used to predict from the
  intact candidate text, which neither #542 nor the pin's defined parts
  give.
- **Cases:** `punct-alias/borrowed-candidates` and
  `punct-alias/borrowed-predicted` of `contract-diff.py` (`pin_not_run`:
  the pin is not driven into the read; the subject is held to the defined
  parts) pass on bdb and differ against the parent build;
  `punct-alias/owned-*` pass a copy of the same text and MATCH the pin
  (#1272, 2026-10-26 UTC).

### NULL prediction prefix

- **Upstream source cite:** `src/pinyin.cpp:1389-1398` (`_compute_prefixes`)
  passes `prefix` to `g_utf8_to_ucs4`, whose first act is `*in`
  (GLib 2.84.4 `gutf8.c:950`); there is no NULL guard. Its callers reset
  first: `pinyin_guess_predicted_candidates` frees the candidates and
  empties `m_prefixes` (`:2420-2423`), the punctuation entry makes that
  call (`:2459`), and `pinyin_guess_sentence_with_prefix` sets
  `m_prefixes` to `[sentence_start]` (`:1431-1432`).
  `zhuyin_guess_sentence_with_prefix` has the same shape
  (`src/zhuyin.cpp:912-918`, `:949-953`).
- **Mechanism:** a NULL `prefix` with a live instance passes those defined
  resets, then the conversion dereferences NULL and the pin dies of
  SIGSEGV. Row 41's guard covers a NULL instance only, where nothing
  defined precedes the fault.
- **What oxpinyin does instead:** by maintainer ruling (2026-10-26 UTC,
  #1277), a NULL `prefix` is a prefix that adds no token, like one
  `g_utf8_to_ucs4` rejects or an empty one.
  `oxpinyin-capi-marshal`'s `ucs4::prefix_text` turns all three into
  `None` at the C boundary, and the prediction and prefixed-sentence paths
  take that `Option`. The plain entry answers `false` with the candidates
  and prefixes emptied, the punctuation entry answers `true` with an empty
  list, and both sentence entries guess on `[sentence_start]` and answer
  the n-best result.
- **Externally observable:** yes. The pin faults; oxpinyin answers the
  no-prefix outcome. Class (b): the NULL dereference is not reproduced.
- **Cases:** `prefix-convert/{predicted,punctuation,pinyin-sentence,zhuyin-sentence}-null`
  of `contract-diff.py` (`crash=`: the pin dies of SIGSEGV after the
  subject's stderr, and the subject answers the outcome above) pass on bdb.
  Against the #1276 build, all but `punctuation-null` differ; that entry
  already gave the outcome (2026-10-26 UTC).

### Public bigram export is a rendering surface

- **Upstream source cite:** `src/pinyin.cpp:775-918`.
- **Mechanism:** the public bigram iterators render the store: sentence-start
  predecessors are dropped, counts are doubled, below-threshold rows are
  hidden, pronunciations are expanded as a Cartesian product, and
  per-predecessor totals are unreachable.
- **What oxpinyin does instead:** the C ABI reproduces that rendering for
  compatibility. The full value surface is reachable only below the public
  iterators; the one-time migration tool that would have read it (linking
  the pinned `libstorage.a` through a dump shim,
  `legacy-migration` §3) was cancelled — that document is
  SHELVED and the implementation stays on the parked branch
  `feat/w7-t2-legacy-migrate`.
- **Externally observable:** yes — the C ABI surface matches the pin's
  rendering; nothing in-tree reads the raw store.

### HANYU full pinyin ignores tone digits under USE_TONE — CLOSED

- **Upstream source cite:** `FullPinyinParser2::parse_one_key`
  (`src/storage/pinyin_parser2.cpp:164-214`): under `USE_TONE` a
  trailing digit 1–5 is the tone and is consumed with the match
  (`zai4` consumes 4; aux renders `zai4` through
  `ChewingKey::get_pinyin_string`, `chewing_key.cpp:47-58`).
- **Mechanism:** the scan reads only the span's last byte; the
  digit-stripped core goes through the ordinary option-gated index
  lookup, so an initial-only key carries a tone like a complete one,
  and the DP window is `max_full_pinyin_length = 7` "include tone"
  (`pinyin_parser2.cpp:82`). `0` and `6`–`9` are not tones: they stay
  in the core, fail the lookup, and the shorter toneless parse wins.
- **Status:** closed by the HANYU `USE_TONE` port. The graph's
  `emit_edges` strips a trailing `1..=5` only under the bit (window 7
  then, 6 otherwise), `Edge` carries the tone, the capi aux renders
  canonical + digit, fuzzy alternates inherit the tone, and the
  resplit/divided tables never match a toned key (`ChewingKey`
  `operator==` includes `m_tone`, `chewing_key.h:81-91`). Measured:
  `SCHEME_DIFF_TONE=1 SCHEME_DIFF_PARSE_AUX_ONLY=1
  run-scheme-diff.sh full 1` → PARSE_AUX_IDENTICAL, with the tone-less
  full-1 sweep staying PARSE_AUX_IDENTICAL over its unchanged corpus.
- **Back-reference:** distinct from #130's aux over-read (a buffer
  split in the aux renderer, not parser consumption) — carrying the
  digit here is exactly what keeps that over-read closed on HANYU.

### validate_constraint's drop test is an integer zero test — CLOSED (academic: equivalent on model20)

- **Upstream source cite:**
  `src/lookup/phonetic_lookup.cpp:142-168` (`validate_constraint`);
  `src/storage/phonetic_key_matrix.cpp:534-601`
  (`compute_pronunciation_possibility`); `src/storage/phrase_index.h:136-164`
  (`get_pronunciation_possibility`); `src/storage/pinyin_phrase3.h:68-144`
  (the loose compare).
- **Mechanism:** a forcing is dropped when
  `compute_pronunciation_possibility` of the forced token over its span
  falls below `FLT_EPSILON` (2^-23) under the current matrix. The
  quantity is a sum over every matrix path that spells the phrase: each
  complete path contributes the item's matched/total pronunciation
  share under the loose compare — initials exact, initial-only keys
  match any middle/final, zero tone matches any tone, fuzzy not
  handled.
- **What oxpinyin does instead:** the same walk and compare, with the
  drop test as an integer zero test. `ConstraintSpelling::spells`
  (`nbest.rs`) reads the forced token's phrase item through
  `Dictionary::phrase_item_pronunciations`, walks the matrix columns
  from the forcing's start to its end exactly as
  `compute_pronunciation_possibility` does (an empty start or end
  column answers zero, a path is as long as the item's phrase length),
  and compares each path with the item's stored pronunciations by
  `pinyin_compare_with_tones` (`pinyin_phrase3.h:108-144`; an incomplete
  key matches any final, `:80-86`). It drops the forcing when no path
  matches a stored pronunciation with a nonzero frequency, where
  upstream drops when the f32 sum of the matched/total shares is below
  `FLT_EPSILON`. There is no expansion of incomplete keys and no cap, so
  a forcing over an abbreviation of any width survives as it does at the
  pin (#825). Like the pin's single `PhraseItem item`
  (`phonetic_lookup.cpp:141`), the last item read successfully is carried
  across the forcings of one pass, so a token whose library is unloaded
  is checked against the previous forcing's item
  (`phrase_index.h:649-650`). A dictionary that keeps no phrase items (the test doubles)
  is revalidated by searching the path instead. The cells also carry the
  chosen phrase's display text where upstream re-fetches by token from
  the phrase index, so the selection record rebuilds from the store
  alone.
- **History (#825):** the revalidation used to search the span's key
  paths through the dictionary, expanding every incomplete key into its
  completions (`SCAN_EXPANSION_LIMIT`, 4,096 combinations). Past the
  limit nothing was searched, the forced token read as absent, and the
  forcing was dropped where the pin keeps it. The zero-test equivalence
  below holds for the read the pin makes and now so does the port.
- **Status:** closed as **academic — equivalent on model20**. The
  arithmetic differs (upstream sums f32 per-path loose-compare terms;
  oxpinyin guards on integer zero before any division), but the drop
  boundary is unreachable on the pinned data, so the two tests produce
  identical observable behavior. Proof: the threshold implies a
  per-token pronunciation total above 2^23 = 8,388,608 for any
  nonzero-but-below-ε share. Scanning the model20 tables: the max
  per-token total is 2,945,481 (的 — 2,224,855 across its pinyin
  records plus 720,626 across its punctuated-variant records; the
  per-library max alone is 2,224,855) and every record frequency is
  ≥ 1, so every nonzero sum is ≥ 1/2,945,481 ≈ 3.4e-7 > FLT_EPSILON ≈
  1.19e-7 — a 2.8× margin. Below-ε-but-nonzero cannot occur; the test
  is equivalently a zero test on both sides. The zero case agrees too:
  every stored pronunciation in model20 carries zero tone (no tone
  digits in any table), so the compare's tone rule never blocks; the
  loose compare's initial-only lenience is the same compare on both
  sides (both initial-exact); and fuzzy
  alternates are explicit matrix-column keys on both sides, so the
  all-exact path always matches. No model20 input exists where the pin
  drops a forcing and oxpinyin keeps it — the E2E I/O rule is
  satisfied vacuously, and no non-vacuity case is constructible.
- **Re-opening condition:** two corpus changes reopen this entry, with
  different fixes. A per-token pronunciation total large enough to push
  a forced token's nonzero share below FLT_EPSILON (a matched count of
  1 over a total above 2^23 = 8,388,608) makes the threshold — not
  zero — the drop boundary while a span entry still lists the token:
  this entry becomes a revert target, and the fix is the threshold port
  over the matched/total sums `ConstraintSpelling::spells` already forms.
  Separate, and not addressed by that port: a corpus storing nonzero
  pronunciation tones needs no separate fix: the compare is the pin's
  tone-aware one, and the matrix keys carry their tones into it.
- **Externally observable:** was only ever reachable on edits that
  leave a span marginally spellable — the same inputs where the §3
  possibility divergence is already observable; on model20, not
  reachable at all (see Status).

### Constraints survive every re-parse — CLOSED

- **Upstream source cite:** `src/pinyin.cpp:1497-1517`
  (`pinyin_parse_more_full_pinyins` never touches `m_constraints`);
  `src/pinyin.cpp:2693-2704` (`pinyin_reset` clears them,
  `m_constraints->clear()` at :2699).
- **Mechanism:** upstream's constraints are instance state that survives
  every re-parse — extension, backspace, edit, and a re-parse after the
  composition completed — with `validate_constraint` dropping whatever
  no longer spells at the next guess. There is no engine-visible
  "completed" notion: the cursor is the frontend's own state.
- **What oxpinyin did (pre-revert — historical, superseded by the
  Closed bullet below):** the parse continued an OPEN composition's
  re-parse when the buffer evolved from the stored one — extension,
  shrink, or re-send kept the store, the selection record, and the
  clamped cursor; validate dropped what stops spelling, and the record
  followed. Two shapes started fresh: a composition a SELECTION
  consumed (an engine-level emulation of the frontend's
  reset-on-commit contract the #141 cursor flows pinned), and a
  divergent buffer. Only the divergent-buffer half survives today.
- **Audit note (2026-08-29):** an early work order framed this entry as
  a `pinyin_reset` scope/order question. It never was: the pin's
  `pinyin_reset` and oxpinyin's `pinyin_reset` (`reset_parse_state` +
  `Session::reset`) produce identical post-state field for field — both
  clear the constraint store, and upstream's parse path leaves it
  untouched. The divergence lived only in `parse_continues`'s
  selection-committed rule.
- **Closed** (`fix/revert-r5-constraint-reset`): a selection-committed
  composition whose buffer evolved from the stored one now CONTINUES —
  `Session::committed_parse_continues` joins `parse_continues`, and
  `begin_parse` takes only the composition reset (`reset_composition`),
  keeping the store and the selection record into the next guess where
  validate drops what stops spelling. `pinyin_reset` alone clears the
  store now, exactly upstream. A DIVERGENT buffer still starts fresh:
  a different string is a different composition, and a stale
  selection-derived cursor must not mis-anchor the new composition's
  window before validate could drop the mismatched forcings — a
  deliberate boundary upstream has no analogue for (its cursor is the
  frontend's own), not a recorded divergence.
- **Evidence:** the live-typing differential gained a committed-reparse
  phase (choose the whole input's phrase — the commit branch — then
  re-parse an extension with no reset): `pinyin_clear_constraint(0)`
  answers 1 on both sides post-revert, 0 on the engine pre-revert
  (DIVERGENT), so the probe flips to IDENTICAL with the change
  (`live-typing`). The frozen pins held bit-identical (candidates
  10,190/10,190/absent 0/order-only 0/prefix-10 98,930; sentence
  488/385/379), the scheme sweep is byte-identical to the pre-change
  baseline (its standing §5 tie class), and the backspace ladder is
  unchanged (`live-typing` §"Backspace-after-choose"). The three
  #141 cursor flows that pinned the fresh start were re-based onto the
  continued-store contract (the committed-reparse store test, the
  post-separator choose law, the Luoma offset law); the
  training-through-the-ABI flow now takes the explicit `pinyin_reset`
  a frontend performs.

### The n-best row-choose cursor answers the whole parse end — CLOSED

- **Upstream source cite:** `src/pinyin.cpp:2511-2519`
  (`pinyin_choose_candidate`'s NBEST branch returns
  `matrix.size() - 1` unconditionally).
- **Mechanism:** choosing any n-best row answers the whole input's parse
  length as the new cursor, whatever span the row's own path covered.
- **What oxpinyin did instead:** the row candidate's absolute end —
  the composition offset it actually advanced to. The two agreed whenever
  the row's path reached the parse bound (every real-table surface,
  including the live-typing differential); a degenerate row that stops
  early answered its own shorter end.
- **Status:** closed by answering `parsed_len` for every
  `NBEST_MATCH_CANDIDATE` selection — upstream's own value of
  `matrix.size() - 1`, since `fill_matrix` sizes the matrix to
  `parsed_len + 1` and no split/fuzzy step resizes it, carried here in
  the active parse mode's own coordinates (`m_parsed_len`,
  `pinyin_get_parsed_input_length`) — whatever span the row's path
  covers (`crates/oxpinyin-capi/src/candidates.rs`). Only the answered
  cursor changed; the engine's composition state is untouched. Measured:
  `union-diff.c` grew an NBEST-row section that chooses the fixture's
  degenerate single-phrase row (imported user phrase 测测 for "cece",
  pristine train state — the train section's unigram deltas enrich the
  decode past it, so the section runs first), prints the cursor, and
  follows the corrected post-NBEST flow — `pinyin_guess_sentence`, then
  `pinyin_train`, never `pinyin_guess_candidates` at the cursor (the
  tail slot starts no span on either engine; the old draft's
  guess-at-the-cursor chain only passed because the old row-own-end
  answer happened to equal the chosen phrase's extent there). Old answer
  `nbest-cursor: 4` vs the pin's 9 → `run-union-diff.sh` DIVERGENCE;
  new answer 9 = 9 → IDENTICAL end to end. live-typing differential
  IDENTICAL; the frozen candidate and sentence-surface pins
  bit-identical.
- **Note:** upstream has no `m_last_index` member (grep over the pinned
  2.11.91 tree: none) — the returned cursor is the only cursor channel,
  and the engine session's composition offset keeps its existing
  internal semantics.
- **Externally observable:** was — only through a row whose path ends
  before the parsed input does; the union fixture's imported-phrase
  composition drives exactly one such single-phrase row on the mini
  tables, and none of the frozen pin surfaces constructs one on real
  tables.

### One bigram-prediction row differs on the pin's own data (trellis residual)

- **Closed 2026-10-03 with row 33 (#614, merge commit `33e2e54d`):** `union-diff` is
  pin-identical on `main` (`IDENTICAL (11 log lines)` on tkrzw, bdb and kc);
  the extra line described here is gone.
- **Reattributed 2026-09-27 UTC (#550, audit D-05; policy row 20 → row
  33).** The line is not a trellis residual. The union driver's
  whole-row NBEST choose installs no `CONSTRAINT_ONESTEP` on the pin
  (`src/pinyin.cpp:2515-2520`), so `train_result3` trains nothing
  (`src/lookup/phonetic_lookup.h:866`) and the pin writes no `测测 → 你`
  at all; oxpinyin's selection-history fallback
  (`crates/oxpinyin-engine/src/session/selection.rs:298-339`) writes it
  with count 138 and predicts 你. That is row 33's mechanism, and the
  audit measured it on every cell, not only on Kyoto Cabinet. The
  bullets below are the original record; their "straddle the
  `m_count ≥ 10` filter" explanation is superseded.

- **Where:** `tools/bisection/run-same-data-dir-diff.sh union-diff` on a
  `--with-dbm=KyotoCabinet` libpinyin install's own `data/` — one line,
  `pred: type=4 text=你` (a `PREDICTED_BIGRAM_CANDIDATE`), present on
  oxpinyin's C ABI and not the pin's, after the union differential's
  train-then-predict sequence for `测测`.
- **Mechanism:** downstream of the registered n-best trellis divergence
  (below). The union driver's step 3 chooses an n-best row for
  `cecenihao` and trains the constrained decode; the first decoded phrase
  after `测测` differs by the trellis's gfloat-vs-fixed-point residual, so
  the two engines write `测测 → 你` into the user bigram with counts that
  straddle `_compute_predicted_bigram_candidates`'s `m_count ≥ 10` filter
  (`pinyin.cpp:2340-2366`). At that one prefix oxpinyin's trained count
  clears 10 and the pin's does not, so oxpinyin emits the extra
  single-character bigram prediction.
- **Independent of the P6 runtime switch:** the user-store training code
  (`oxpinyin-user`) is unchanged by P6, and the divergence is stable
  across the scoring change; the standard `run-union-diff.sh` (oxpinyin on
  its own generated data vs the pin on its own) stays green, because there
  the two decodes settle the same way. It surfaces only when oxpinyin
  reads the pin's exact bytes and the trellis residual tips this filter.
- **Externally observable:** yes — one predicted-bigram row on this one
  trained-prediction edge. Not reducible without matching the pin's float
  trellis bit-for-bit, which the fixed-point decoder deliberately does not
  (see below). Every other `union-diff` line, and the whole
  `dict-surface` / `pred-order` / `live-typing` / `nbest-train` /
  `phrase-surface` / `predict` / `punct` / `addon` / `user-candidate` /
  `import` / `key-surface` differential, is identical on the pin's data.

### Candidate sorting guards the wrapping frequency order (#1297 item A)

The maintainer decision dated 2026-10-30 preserves libpinyin's frequency
comparison as `rhs.wrapping_sub(lhs) as i32`. Stable std sorting replaces
GLib's merge after guarding each enabled length/span tie group. Only cyclic
groups may use stable numeric-descending fallback; equal complete keys keep
collection order, and dedup-only `m_freq` never breaks ties. The guard proof,
source derivation and verification are in
[1297-candidate-numeric-sort](1297-candidate-numeric-sort.md). The four
`row58-signed-{score,duplicate}-{off,on}` cases must MATCH and are not
accepted by this registration.

### Zhuyin selection uses length, raw score and batch position (#1378)

The 2026-10-29 “No ported external-library internals” ruling orders zhuyin
selection by length, descending raw possibility (NaN last, signed zeros
equal), then earlier batch position. The equal-count user-phrase case
`nbest-user-table-order-zhuyin` and the three NaN-producing λ cases are
POLICY-ACCEPTED; complete observations
and derivation: [1378-zhuyin-selection](1378-zhuyin-selection.md).
Single-slot admission stays unchanged.

### N-best trellis accumulates gfloat log costs — not reproducible in fixed point, FROZEN as a permanent Stage-1 divergence

**Superseded by #1378 PR B (maintainer approval 2026-10-29 UTC;
recorded 2026-10-29T08:32:25Z, `date -u`).** Real-table trellis and phrase scores
now accumulate in f32 after Rust std `f64::ln`. The permanent fixed-point
divergence and its cross-platform premise are retired; the historical
record below remains. Only a demonstrated std-math precision difference
remains class (a). Compiler contraction is a separate internal language
artefact, never imitated with `mul_add`. §12 records the PR B re-measurement: seven supplementary surfaces become
pin-equal, and 156 exact selection residuals are unchanged from main.

**Current scope (2026-10-04 UTC, lane D):** selection is ported. Only bo, quguan, kaliantai, huilianpei, yunhoulvlvlunqianaonaoruoqubeiji and nou'y are class (a); the four separator residuals remain unattributed by name in sentence-surface.md §12's lane-D amendment. Current gate: 500/500/500 of 500 after the #639 resplit repair (495/495/495 of 496 at the port; corpus 14/10,465 before the repair). The historical record below is superseded for current scope and counts.

- **Scope shrinks, pending lane D (2026-09-27 UTC, #550, #535, audit
  D-13; policy row 11).** Two rules inside the frozen residual are
  selection logic, not arithmetic, and are not class (a): the node keep
  rule (the pin's `trellis_node::eval_item` heap front is the best
  value, replaced when a newcomer beats it —
  `src/lookup/phonetic_lookup_heap.h:25-29`, `:56-81`; oxpinyin replaces
  its worst, `crates/oxpinyin-engine/src/nbest.rs:241-267`) and the
  comparator's longer-by-one clause (dead at the pin, `:75-77` being
  subsumed by `:87-88` of `phonetic_lookup.h`; live in oxpinyin,
  `nbest.rs:194-197`). Selection-logic specimen D-26: ZiGuang
  `zhrgguor`, candidate[2] 宗人光卓然 on the pin, 总人光卓然 on
  oxpinyin, rows 0–1 identical (#535). Class (a) keeps the `gfloat`
  `log` accumulation only.

- **Upstream source cite:** `src/lookup/phonetic_lookup.h:663, 692`
  (`m_poss += log(...)` per step, a `gfloat` accumulator rounded at
  every node); comparator `trellis_value_less_than` (`:66-91`), used by
  the node store, the beam-32, and the tail-3 selections; final tail
  sort `trellis_value_compare` (`:174-178`), which truncates the float
  poss difference to `gint` so two tails within 1.0 nat tie and keep
  heap-pop order.
- **Mechanism:** the sentence n-best selection is a pure function of the
  accumulated `gfloat` log-probabilities. Each step adds a natural `log`
  (computed in `double`, stored back to `f32`), and near-ties among the
  top-3 survivors — which 1st/2nd/3rd hypotheses live, and their rank
  order — are decided by those exact float values, down to the ULP.
- **What oxpinyin does instead:** the core integer fixed-point surprisal
  scale (negative log₂ × 1000; `crates/oxpinyin-core/src/cost.rs`),
  accumulated exactly, with an insertion-order tiebreak. Reproducing the
  `gfloat` values would require a floating-point natural `log` per step;
  `f64::ln` delegates to the platform libm with no cross-platform
  bit-exactness guarantee, the build forbids `-march=native` for exactly
  this reason, and constitution item 6 requires output to be a pure
  function of (input, user state, config) on every OS. The float
  dependency also reaches the tiebreak (heap-pop order is seeded by the
  exact-float comparator), so it cannot be recovered in fixed point
  either. Contrast the candidate frequency `amplified_frequency`
  (`(1−λ)·unigram/total·2²⁴` — IEEE-754 basic ops only, no
  transcendental), which *is* bit-reproducible and ported to 100%.
- **Externally observable:** yes, on the sentence surface only. Against
  the pinned oracle over a 496-input W2 sample
  (`fixtures/w4/oracle-sentence-surface.txt`): 1-best 491/496, n-best
  distinct-set 396/496, n-best ordered / first-6 rows 390/496; the 106
  ordered misses are all trellis-side (0 candidate-surface leaks). The
  candidate surface, which does not share this arithmetic, is
  bit-identical. Frozen as a permanent Stage-1 divergence by maintainer
  ruling 2026-09-02 at 488/385/379 and re-frozen 2026-09-04 at 491/396/390
  after P6 (`345af16d`) moved the trellis's P_unigram source —
  `sentence-surface.md` §12, asserted by
  `crates/pinyin-oracle/tests/sentence_surface_parity.rs`; enumerate with
  the read-only `pinyin-oracle` `sentence-tail` binary.

### Predicted-candidate tie order is the Tkrzw HashDBM bucket walk

- **Upstream source cite:** `src/storage/phrase_large_table3_tkrzwdb.cpp:155-190`
  (`PhraseLargeTable3::search_suggestion`: `MakeIterator`/`Jump(prefix)`/
  `Next` over `phrase_index.bin`, a `TkrzwHDB` file); consumed verbatim by
  `_compute_predicted_prefix_candidates` (`src/pinyin.cpp:2380-2405`) and
  left in place by `g_array_sort_with_data` — measured on a 178-element
  array with grouped ties, glib's sort preserves within-tie insertion order
  (0 inversions).
- **Mechanism:** the system suggestion phrases are baked with uniform
  phrase-index counts (measured on model20: 好 177×100+1×200, 的
  281×100+2×99, 一 587×100+2×99+2×200, 我 167×100+1×200), so the
  `(length desc, amplified-freq desc)` comparator ties across the whole
  list and the row order a caller sees is exactly the store's iteration
  order — the Tkrzw hash bucket walk, one physical file holding all
  libraries' tokens (27 library switches observed in one prefix's list).
  Deterministic for a given file and tkrzw version; not expressible as a
  sort key over (text, token, library).
- **What oxpinyin does instead:** the prediction pipeline has three
  stages (pre-P6 description, kept as the record; the P6 amendment below
  carries the current mechanism). (1) Collection —
  `SystemDictionary::suggest_after`
  (then `crates/oxpinyin-data/src/dict.rs:196-217`) walked a
  `BTreeMap<String, Vec<u32>>` in text order, collecting every phrase that
  starts with the prefix, then sorts that collection by token ascending.
  (2) Ranking — `guess_predicted` (`crates/oxpinyin-capi/src/predict.rs`)
  applies a stable sort whose primary comparator is **phrase length,
  descending**, tie-broken by **amplified frequency, descending**, so the
  final order is determined by that comparator (and the stable collection
  order within a full tie), NOT directly by the `BTreeMap` walk. (3)
  Deduplication drops repeats by phrase text. The hash bucket order of a
  foreign DBM layout is not derivable from any key, so matching the pin's
  order exactly would mean replicating the Tkrzw hash layout or freezing
  per-prefix orders as fixture data.
- **Externally observable:** yes — the row order of `PREDICTED_PREFIX`
  candidates from `pinyin_guess_predicted_candidates[_with_punctuations]`.
  The sets are identical after the prefix slice (closed by the B1 fix —
  the slice lands in `predict.rs`), and every response is divergent on
  position only. Position mismatches vs the pin, matched model20 tables:
  **177/178 on 好, 1557/1571 across the eight measurement prefixes**
  (the text-ascending order; the pre-switch token-ascending order was
  174/178 and 1541/1571). The gate is `tools/bisection/pred-order-diff.c`
  on the measurement branch.

**Decision (maintainer, 2026-08-25): a defined order, not fixture-frozen
parity.** The pin's order is a compile-time artifact of its DBM choice
with no semantic content, a frozen fixture would re-freeze whenever the
pin's storage changes, and it would hand frontends an order that
carries no meaning. The defined order is **text-ascending** — stable
across builds, what the `BTreeMap` walk already yields, reproducible by
anyone — joining the trellis-float entry as "upstream deterministic but
not reproducibly so." Two consequences, stated explicitly:

1. **Superseded by P6 (2026-09-02).** Once the runtime reads the phrase
   DBM directly, the suggestion order is reproduced from the *same* file
   the pin walks: `SystemDictionary::suggest_after` +
   `resolve_suggestions` (`crates/oxpinyin-data/src/dict.rs`) group the
   tokens by library nibble ascending (the `reduce_tokens` concatenation)
   and, within a group, in the DBM's byte-lexical UCS-4 cursor order —
   exactly `PhraseLargeTable3::search_suggestion`'s walk. On the pin's own
   KC `data/`, `pred-order-diff` is now **IDENTICAL** (1588 log lines, 0
   position mismatches), not the text-ascending near-miss below. The
   text-ascending fallback remains the *defined* order for oxpinyin-native
   backends (redb/LMDB), whose container is not the pin's; on KC and tkrzw
   the pin's own order is reproduced because the file is the pin's own.
   The historical text-ascending decision is kept below for the record.
2. The pred-order gate therefore **changes meaning**: from a parity
   assertion (drive to zero) to a **defined-order assertion** — the
   emitted list equals its own defined text-ascending order
   (within the comparator's `(char count, amplified frequency)`
   tie groups). Implemented in-tree: the capi e2e test
   `predicted_tie_groups_are_text_ascending_including_user_rows`;
   the runner comparison against the pin stays as the
   recorded-divergence constant.

**Completed (fix/predicted-text-order):** the token pre-sorts are gone —
three sites, not two: `SystemDictionary::suggest_after`
(`dict.rs`; since P6 `search_suggestion` + `resolve_suggestions`), `append_predicted_prefix` (`predict.rs`), and the user
seam `UserLookup::suggest_after` (`oxpinyin-user/src/lookup.rs`). The
`BTreeMap` text-ascending walk (token-ascending within one text) now
survives the stable sort's tie groups, on the system and user seams
alike — measured new drift constants 177/178 (好) and 1557/1571
eight prefixes. The defined-order predicate is asserted by the capi
e2e test `predicted_tie_groups_are_text_ascending_including_user_rows`
(grouping by the comparator's exact `(char count, amplified frequency)`
key, populated user store included); the oracle comparison remains the
recorded drift constant, *not* a target of zero.

**Homograph nuance (frontend-invisible):** within one text the
per-text token vector stays token-ascending (`build_text_tokens` then;
`resolve_suggestions` in `crates/oxpinyin-data/src/dict.rs` since P6), so
a homograph row keeps the same surviving token under the defined order;
the one case that can differ is a system-vs-user text duplicate — the
system row now always precedes the user row, so with a populated user
store the **token recorded on the surviving dedup row** can differ.
Text, candidate type and counts cannot.

### Mid-syllable candidate-lookup offset: closed — the pin's empty-column law, not the suffix re-parse

- **Upstream source cite:** `src/pinyin.cpp:2224-2262`
  (`pinyin_guess_candidates` anchors `start = offset` and runs
  `search_matrix(matrix, start, end, ...)` over the whole-composition
  `PhoneticKeyMatrix`); `src/pinyin.cpp:2163-2180` (`_check_offset` asserts
  only on a lone zero-key column — one past an apostrophe run — never on an
  ordinary mid-syllable offset); `src/pinyin.cpp:3006-3027`
  (`pinyin_get_pinyin_offset` walks the cursor back to the nearest non-empty
  column before any caller reaches the guess); `src/storage/phonetic_key_matrix.cpp`
  (`fill_matrix` puts each chosen key at its raw begin, and `resplit_step` /
  `inner_split_step` append split keys at interior positions, so a divided
  syllable's boundary is a live column too); `src/storage/special_table.h`
  (the frozen divided/resplit pair lists).
- **Mechanism:** the pin's matrix has one column per input byte. A column
  carries a key where the chosen parse's syllable begins, plus the split-key
  halves the two table steps add (`jie` in `nihaoshijie` also carries
  `ji` + `e`, so byte 10 answers the `e` window — measured fresh `n=190`,
  阿 first — while mid-chunk bytes 3/4/6 of the same input stay empty), and a
  zero key at every apostrophe, which `search_matrix` steps over (measured:
  `ni'hao@2` answers the full `hao` window, `n=93`). `search_matrix` from an
  empty column matches nothing, so the window there is only the prepended
  n-best sentence rows over the raw-suffix fallback — measured on `nihao`
  (parity word `0x18a`): fresh offsets 1/3/4/5 → `true` with `n=0`; after
  `pinyin_guess_sentence` → `true` with `n=1` (你好 alone); `nihaoshijie`
  after the sentence lookup → `true` with `n=3` (你好世界/你好时节/你好是届).
  The pin never *aborts* at a mid-syllable offset; only one past a lone
  apostrophe column trips the `_check_offset` assert (`ni'hao@3` — the
  recorded no-pin-behaviour landmine, not a comparable surface). In normal
  use the pin is never handed a mid-syllable offset — the frontend routes
  every cursor through `pinyin_get_pinyin_offset`, which snaps it to the
  syllable start.
- **What oxpinyin did instead (pre-fix):** the candidate window was rebuilt
  from the raw byte suffix `&raw[offset..]` (`Session::candidates_at` →
  `Session::scan_window`), so a mid-syllable offset re-parsed the tail as a
  fresh composition and returned that tail's phrases — measured on `nihao`:
  offset 3 → `n=105` phrase rows (`奥/澳/凹/…`, the `ao` re-parse), offset 4 →
  `n=5` (the `o` re-parse) — where the pin shows the empty-column window.
  Offsets whose suffix cannot begin a parse (`i`-initial tails: `nihao@1`,
  `nihaoshijie@1/7/9`) already agreed, because the re-parse found nothing and
  the C ABI skips the raw-fallback row.
- **The fix:** `Session::candidates_at` now classifies the anchor before
  scanning. An anchor a scan-matrix key's syllable starts on — or an
  apostrophe byte the parse reached — keeps the offset-anchored scan; any
  other byte (a mid-syllable position, or an apostrophe past a stop byte,
  outside the matrix) is the pin's empty column, answered as the raw-suffix
  fallback under the prepended
  n-best rows with no phrase scan. The classification reuses
  `build_scan_matrix` — the same matrix model the window scan itself reads
  (`docs/findings/matrix-split-tables.md`) — so the boundary law and the
  candidate construction cannot disagree about what the matrix holds.
  Measured byte-identical against the pin over every byte offset of `nihao`
  and `nihaoshijie`, fresh and post-sentence, on the compared surface — the
  guess bool, the window count, and each window's first four rows (the
  driver's
  phase E: `tools/bisection/uncovered-surface-diff.c`, labels `raw:`) — and
  over the exotic classes: `ni'hao@2` (transparent apostrophe, `n=93`/`94`
  both sides), `nihaozh@6` and `nihaozhu@6/7` (incomplete `zh`/`zhu` stay one
  matrix key — empty columns both sides), `shon` under
  `PINYIN_CORRECT_ON_ONG` (the parse is `s|hong`; bytes 2/3 empty both
  sides), and `nang`/`shuo` under `PINYIN_AMB_AN_ANG` (single-key parses;
  every interior byte empty both sides). The prior suffix-re-parse windows
  (`nihao@3` → `n=105`) are gone. At every syllable-aligned offset — the only
  kind a correct caller produces — the two engines keep agreeing bit-for-bit.
  Where the pin's `_check_offset` aborts (one past an apostrophe run), oxpinyin
  still normalizes or refuses via
  `CapiInstance::validate_lookup_offset`
  (`Session::normalized_lookup_offset` → `EngineError::LookupOffsetPastSeparator`).
- **Externally observable:** not in practice — no known frontend passes a
  mid-syllable offset to `pinyin_guess_candidates`; fcitx5-oxpinyin and
  ibus-libpinyin both snap the cursor with `pinyin_get_pinyin_offset` first,
  and at a snapped (syllable-aligned) offset the windows are identical. A
  caller that bypasses the snap now sees the pin's empty-column window on
  both engines. This closes the residue the 2026-08-27 amendment of
  `uncovered-surface-differentials` recorded as not chased.

### Apostrophe-only input: the pin consumes every byte, and so does the engine — CLOSED

- **Status (amended 2026-11-07 UTC):** closed by 678f3259 (2026-08-26, the
  B2 parser-termination class): `SegmentGraph` propagates each
  apostrophe one byte, counted, so `'` → 1, `''` → 2, `'''` → 3 match
  the pin (pinned in `crates/oxpinyin-core/src/graph.rs`).
  Since #902, the zhuyin facade's `guess_sentence`,
  `guess_sentence_with_prefix`, `guess_candidates_after_cursor` and
  `guess_candidates_before_cursor` return `false` on a zero-key matrix,
  as the pin does, and `oxpinyin-zhuyin-capi`'s
  `boundary_tests::a_keyless_parse_refuses_the_guess_and_keeps_the_sentence`
  holds that behaviour.

- **Upstream source cite:** at `074a2219`,
  `src/storage/pinyin_parser2.cpp:238-250` propagates each `'` with an
  unchanged key count and an incremented parsed length; `:360-378`
  exports only real keys. Thus `'` → parse_return 1, `''` → 2,
  `'''` → 3, but each exports zero keys.
- **Mechanism (corrected for #902):** a separator-only input consumes
  every byte but leaves an empty matrix:
  `src/storage/phonetic_key_matrix.cpp:34-38` clears it and returns
  before creating separator columns when there are no real keys.
  Sentence guessing therefore returns false before clearing prior
  results (`src/lookup/phonetic_lookup.h:743-748`); candidate guessing
  clears candidates and returns false (`src/pinyin.cpp:2193-2196`,
  `src/zhuyin.cpp:1468-1471,1548-1551`). This corrects the former
  non-empty-matrix explanation; the consumed-length fix stays closed.
- **What oxpinyin does:** `SegmentGraph` propagates each apostrophe, so a
  separator-only input consumes every byte (`pinyin_parse_more_full_pinyins`
  and `pinyin_get_parsed_input_length` answer 1, 2 or 3) and leaves an empty
  matrix. Over that matrix `pinyin_get_pinyin_offset` answers `true` with
  offset 0 at cursor 0 and aborts at every cursor of 1 or more, where the
  pin's first `get_column_size` asserts (`phonetic_key_matrix.h:103`);
  `pinyin_get_left_pinyin_offset` answers `true` with offset 0 at offset 0
  and aborts at any other; `pinyin_get_right_pinyin_offset` and
  `pinyin_get_pinyin_key` abort (register row 66, F-E-14 in
  `docs/testing/oracle-apostrophe-abort.md`).
- **Externally observable:** no. The parse return, the parsed length and
  every cursor helper answer as the pin does on this input.

### FORCE_TONE — scheme-specific: every seam honours its scheme law except the pinyin facade's chewing batch (its own entry below); zhuyin batch closed (1671954); double-pinyin batch closed (5ec782ea)

- **Upstream source cite:** `src/storage/pinyin_parser2.cpp:412` and
  `:448` (`DoublePinyinParser2::parse_one_key`: `if (options & FORCE_TONE
  && 3 != len) return false;` — NOT nested under `USE_TONE`, and a
  length-3 requirement the full-pinyin parser does not have — plus an
  inner check at `:448` that is unreachable, the digit parse above it
  already refuses every non-tone byte); `PinyinDirectParser2::parse_one_key`
  carries the full-pinyin-shaped check (`:645`).
- **Mechanism:** the pin gives each scheme parser its own FORCE_TONE
  semantics; the double-pinyin one is a genuinely different law (a
  two-key-plus-tone length gate).
- **What oxpinyin does instead:** implements the measured surface — the
  full-pinyin law, nested inside `USE_TONE` exactly like the pin
  (`pinyin_parser2.cpp:176-190` ported to `graph.rs::tone_split`) — and
  originally left the BATCH double/zhuyin parsers untouched. The measured C1 surface of
  the uncovered-surface differential is full-pinyin only; porting the
  scheme-parser shapes unmeasured is exactly what would perturb the
  frozen double/zhuyin scheme sweeps.
- **Tier-A amendment (2026-08-29, the one-key seams).** The ABI
  single-key entries DO carry their scheme laws now:
  `pinyin_parse_double_pinyin` implements `DoublePinyinParser2::
  parse_one_key`'s law — the length-3 `FORCE_TONE` gate at
  `pinyin_parser2.cpp:412` (whose inner zero-tone check at `:448` is
  dead: the digit parse above it already refuses every non-tone byte) —
  and `pinyin_parse_chewing` implements the Simple/Discrete/CP26
  `FORCE_TONE` placements (`zhuyin_parser2.cpp:178` nested under
  `USE_TONE`; `:373`+`:387` unconditional for Discrete; `:602` nested).
  Measured: `tools/bisection/run-key-surface-diff.sh` is IDENTICAL
  against the pin (2,131 probe lines) across double schemes 1–6 and
  chewing keyboards 1–6, 8, 9. The FORCE_TONE law parity itself is
  the `0x1ea` (`USE_TONE|FORCE_TONE`) and `0x1ca` (FORCE_TONE alone)
  profiles; `0x1aa` (`USE_TONE` alone) and the `0x18a` baseline run for
  regression coverage of the neighbouring seams, not FORCE_TONE. The
  batch double-pinyin `parse` surface keeps the original scope boundary
  above (its builder is the frozen scheme sweep); the batch zhuyin surface
  was subsequently closed — see Zhuyin batch amendment below.
- **Zhuyin batch amendment (1671954, 2026-08-31).** The batch zhuyin
  `parse` surface (`zhuyin_parse_more_chewings` via `oxpinyin-zhuyin-capi`)
  now honours `FORCE_TONE` per keyboard family: nested under `USE_TONE`
  for Simple/CP26, unconditional for Discrete — matching
  `zhuyin_parser2.cpp:176-180, :373, :387, :602`. Measured: the
  `tools/bisection/zhuyin-diff.c` differential converges on the batch parse.
  This closes the zhuyin batch seam. The double-pinyin batch seam
  (`pinyin_parse_more_double_pinyins`) was closed subsequently — see the
  Double-pinyin batch closure amendment below.
- **Externally observable:** no longer on the one-key seams, the
  libzhuyin batch seam and the double-pinyin batch seam — all answer
  identically to the pin under every FORCE_TONE profile (one-key seams:
  D3 gate; zhuyin batch: 1671954; double-pinyin batch: 5ec782ea). Still
  on the pinyin facade's chewing batch seam (`pinyin_parse_more_chewings`,
  `ToneForwarding::PinFacade` does not forward FORCE_TONE) — its own
  entry below, row 30 of the policy table. The
  full-pinyin seam itself matches the pin (capi e2e `parse_termination`
  module, harness phase-C 0x60 probes closed).
- **Freeze correction (2026-09-02, historical).** Before the batch
  closure, the freeze-time observable-shape sentence read as if the batch
  seam applied the full-pinyin FORCE_TONE law; the batch parser of that
  time was more precisely option-blind: it ran the tone-less profile
  whatever the caller's option word (the greedy walk rejected every
  three-byte key and retried length 2). The divergence was the same in
  every FORCE_TONE profile — oxpinyin observably less restrictive than
  the pin (which consumes nothing at all under FORCE_TONE without
  USE_TONE, and three-byte toned keys under USE_TONE|FORCE_TONE) — but
  the mechanism was absence of the law, not the full-pinyin law. The
  frozen SPEC fixed the law; the Double-pinyin batch closure amendment
  below implemented it.
- **Double-pinyin batch closure (5ec782ea, 2026-09-02).** The batch
  double-pinyin `parse` surface (`pinyin_parse_more_double_pinyins`) now
  honours the frozen SPEC's Tone law: the caller's full option word
  crosses the seam (the pin's `options = context->m_options`,
  `src/pinyin.cpp:1543`) and drives `DoublePinyinParser::parse_with_options`
  — the additive option-word seam the zhuyin batch closure established.
  `FORCE_TONE` rejects any key that is not exactly three bytes
  (`pinyin_parser2.cpp:412`); a three-byte key carries its trailing
  `1`..`5` digit as the tone only under `USE_TONE` (`:439-451`), so
  FORCE_TONE without USE_TONE consumes nothing at all. The parsed tone
  rides the key into the exact segments. Measured in the debian-testing
  gate container against the pinned tkrzw oracle over full model20 KC
  tables: the scheme differential is byte-identical for double schemes
  1, 2, 4, 5, 6 including the new `tonelaw` probe section (142 lines
  each: three FORCE_TONE/USE_TONE profiles over thirteen tone-digit
  inputs) and for all eight bopomofo keyboards; `run-key-surface-diff.sh`
  stays IDENTICAL (2,131 probe lines). Revert-and-check: the pristine
  parser diverges from the pin on 130 of the new probe lines under the
  same driver. The one residual in the comparison — scheme 3 (Ziguang)
  NBEST row 2 on `zhrgguor` (pin 宗人光卓然 / oxpinyin 总人光卓然; the
  1-best and 2-best rows agree) — is the pre-existing §12 trellis
  hypothesis-selection class, unchanged by this closure (the same
  revert-check reproduces it) and first surfaced by the full-model
  scheme sweep.

## Sanitizer scope on the tkrzw shim CI (2026-08-27)

- **Where:** `.github/workflows/store-backends.yml`, `tkrzw-sanitizers` job.
- **libpinyin behaviour:** its make-check CI (and any `-fsanitize=address,undefined`
  build of a C/C++ tree) instruments every translation unit, Rust has no
  equivalent because there is no Rust in the pin.
- **oxpinyin behaviour:** the ASan arm runs FULL `-Zsanitizer=address`
  instrumentation over the target graph (Rust units plus the GCC-instrumented
  C++ shim), made possible by passing cargo an explicit `--target` so
  RUSTFLAGS never reaches host build scripts or proc macros — the mechanism
  that previously made sanitized proc-macro dylibs unloadable (E0463 on
  `cxxbridge_macro`). The UBSan arm instruments the shim translation units
  and injects libubsan at the final link, because rustc's `-Zsanitizer` list
  has never included an `undefined` value. In both arms the prebuilt standard
  library is uninstrumented (no `-Zbuild-std`).
- **Externally observable:** none for shipped behavior; this widens or narrows
  no differential. The residual gaps are toolchain-bound: no
  `-Zsanitizer=undefined` exists, and std is prebuilt. Revisit when either
  changes; per source policy, recorded and not chased.
- **Amended 2026-10-22 UTC (#1014):** the UBSan arm described above is
  removed in this PR. tkrzw is bound through bindgen to the prebuilt system
  library and no C or C++ is compiled, so the UBSan flags instrumented
  nothing. The ASan arm remains and instruments the Rust code only; the C++
  shim and the cxxbridge described above no longer exist.
  Correction to the libpinyin behaviour above: libpinyin's CI builds
  without any sanitizer. At `074a2219`, `make-check.yml` runs autotools with
  `--with-dbm=BerkeleyDB`, then `make check` and `make distcheck`, and
  `cmake.yml` runs a Kyoto Cabinet `Release` build with `ctest`. No
  `-fsanitize` (nor any `sanitize`) appears in that tree's
  `.github/workflows/`, `configure.ac`, `Makefile.am` files or CMake files
  (`CMakeLists.txt`, `cmake/*.cmake`).

## Native data-file naming under the compile-time backend (2026-08-29)

- **Where:** `oxpinyin-store` (`DefaultStore`/`DEFAULT_STORE_EXT`),
  `oxpinyin-runtime`'s system/user file names, `oxpinyin-datagen`
  `Backend::extension`.
- **libpinyin behaviour:** the DBM backend is chosen at configure time
  (`--with-dbm`, `if BERKELEYDB/KYOTOCABINET/TKRZW` in
  `src/storage/Makefile.am`), and the data filenames are backend-INDEPENDENT
  compile-time constants (`SYSTEM_BIGRAM "bigram.db"`,
  `SYSTEM_PINYIN_INDEX "pinyin_index.bin"`, … `src/pinyin_internal.h`), so a
  Kyoto-Cabinet-built libpinyin still writes `bigram.db`.
- **oxpinyin behaviour:** the same one-backend-per-binary compile-time
  selection (the `DefaultStore` cfg chain, exactly one backend feature per
  build), but the file names follow the backend family since P6
  (`345af16d`): on Kyoto Cabinet, tkrzw and Berkeley DB — the three DBMs
  libpinyin itself builds against — the files carry libpinyin's own
  constants (`pinyin_index.bin`, `bigram.db`, …), so an install is name-
  and byte-compatible; only the oxpinyin-only containers (redb, LMDB)
  carry `<stem>.<ext>` (`pinyin_index.redb`/`.lmdb`).
  `DEFAULT_STORE_IS_LIBPINYIN_DBM` (`crates/oxpinyin-store/src/lib.rs`) is
  the switch.
- **Externally observable:** only in redb and LMDB data directories,
  which no libpinyin build can open anyway; on Kyoto Cabinet, tkrzw and
  Berkeley DB the files are libpinyin's own fixed names (`bigram.db`,
  `*.bin`), so no libpinyin consumer sees a difference; recorded because
  the naming intentionally diverges from the pin's constants rather than
  mirroring them.
- **Amendment (2026-09-13, drop-in task 10).** Berkeley DB landed as the
  fifth peer and the third libpinyin DBM, so "the two DBMs" this entry
  named is now three and `bdb` sits inside the name-compatible set — it
  carries libpinyin's own constants, not `<stem>.<ext>`. The precedence
  chain the entry used to cite (`kyotocabinet > tkrzw > lmdb > redb`) is
  also gone: an exactly-one-backend `compile_error!` guard refuses any
  build naming none or more than one backend feature, so no order
  survives to fall back on, and the default selection has been tkrzw
  since 2026-09-05. The naming decision itself is unchanged.
- **Amendment (2026-09-27 UTC, Q1 ruling).** The last clause above is
  history: the default selection was tkrzw from 2026-09-05 to
  2026-09-20 and has been Berkeley DB since (`default = ["bdb"]`, e.g.
  `crates/oxpinyin-capi/Cargo.toml:32`), matching the reference build's
  bare `./configure` (`configure.ac:94` at `074a2219`). Neither tkrzw nor
  Kyoto Cabinet is the default; `compatibility-policy.md`, "Amendment —
  rulings recorded", item 1.

## R1 measured on the drop-in compat paths — order-only, sets identical (2026-08-30)

> **SUPERSEDED (see architecture correction).** The libpinyin drop-in /
> compat loader described in this section has been removed. oxpinyin
> reads only its own peer-backend tables (KC, redb, LMDB, tkrzw); it does
> not detect or read libpinyin's on-disk DBM files. The measurements
> below are preserved as a historical record of what the (removed)
> compat path did.
>
> **Amended 2026-09-06:** the banner above was true between f8b81d61
> (2026-08-30) and P6 (345af16d, 2026-09-02) only. Since P6 the runtime
> reads an unmodified libpinyin install's `data/` directly on KC and
> tkrzw through the same readers it uses for its own output — there is
> no compat *layer*, but the drop-in read is back and is the shipped
> shape. The R1 measurement below is reproduced on that path:
> `pred-order-diff` is IDENTICAL on the pin's own KC `data/` (see the
> predicted-candidate entry's P6 amendment).
>
> Everything below this banner — the harnesses, the 2,562 reorder lines,
> the DIVERGE/order-only verdicts — is the 2026-08-30 measurement of the
> removed compat path, kept verbatim as the historical record. It is not
> the current result; the current result is the IDENTICAL above.

- **Where:** the removed `oxpinyin-data/src/compat` module (libpinyin
  drop-in loader) and its removed
  `tools/bisection/run-pred-order-dropin.sh` /
  `run-dropin-fedora-kc.sh` / `run-dropin-debian-tkrzw.sh` container
  harnesses.
- **The measurement:** dual-dlopen differential — the distro's own
  libpinyin (the oracle) and oxpinyin's `libpinyin_capi.so` (the subject)
  each run the eight predicted-prefix probes over the SAME installed
  `libpinyin-data` directory; PREDICTED_PREFIX rows are compared in order
  (absolute indices stripped: the subject's `_with_punctuations` API
  prepends punctuation rows of a different type; the driver falls back to
  plain `pinyin_guess_predicted_candidates` on libpinyin < 2.11, whose
  enum is a prefix of 2.11.91's — `PREDICTED_PREFIX_CANDIDATE` sits at the
  same ordinal). The driver also dumps the PREDICTED_PUNCTUATION rows
  (type 8, from the install's `punct.bin`, present from 2.11 on) as
  `punct-*` lines, which makes the differential the compat punct reader's
  gate: those rows appear on the subject only when it reads that file.
- **Kyoto Cabinet path** (Fedora rawhide container: libpinyin 2.11.91,
  kyotocabinet 1.2.80): oracle 1,571 rows, subject 1,571 rows; sorted row
  SETS byte-identical; 2,562 diff lines of pure reordering. **DIVERGE,
  order-only** — exactly the registered "predicted-candidate tie order"
  divergence below: the pin walks its DBM's physical bucket order, oxpinyin
  emits the defined text-ascending order. Zero content divergence.
- **tkrzw path** (Debian testing container: libpinyin 2.11.91-1, the
  tkrzw build Debian switched to in 2.11.91-1): the same shape — 1,571 =
  1,571 rows, sets identical, order-only. First measurement on this
  backend; no prior target existed.
- **Kyoto Cabinet path, NixOS packaging** (nixos/nix container:
  nixpkgs-unstable libpinyin 2.11.91, kyotocabinet 1.2.80, oracle at
  `/nix/store/…-libpinyin-2.11.91/lib/libpinyin.so.15`): identical to the
  Fedora measurement — 1,571 = 1,571 rows, sets identical, order-only,
  the same 2,562 reordered lines. Confirms the compat path is layout-
  portable (profile-symlinked `/nix/store` paths, no `/usr/lib`), not
  just RPM-shaped. Built with nixpkgs' rustc (1.95) under
  `--ignore-rust-version` — rustup toolchains cannot run on a pure Nix
  image — with the differential gating the artifact.
- **Punct rows, after the compat `punct.bin` reader** (2026-08-30, same
  differential with the driver's `punct-*` dump): every total rises
  1,571 → 1,588 on BOTH sides — 17 PREDICTED_PUNCTUATION rows per run
  (好 ，。; 是 “，：; 了 。，“！; …) — and the punct rows are identical
  between oracle and subject, order included, on the NixOS KC path
  (nixpkgs's 405 KB `punct.bin`, a TreeDB) and on the Debian tkrzw path
  (its own punct.bin, a TreeDBM). The reorder residual stays 2,562
  pred-row lines; the punct rows add zero divergence. Before the reader,
  the subject's punct table was always empty on these paths, so these
  rows could not have appeared at all.
- **libpinyin behaviour:** predicted candidates come back in the DBM's
  iteration order — backend-dependent, semantically arbitrary, different
  between the KC and tkrzw oracles themselves.
- **oxpinyin behaviour:** the `(length desc, amplified-freq desc)`
  ranking governs the emitted list; the text-ascending collection order
  (token-ascending within one text) resolves only the ties that ranking
  leaves — build-stable on every backend and on the compat paths.
- **Externally observable:** candidate POSITIONS in the predicted list
  differ; the candidate set does not. Ruled intentional by the
  "Predicted-candidate tie order" entry; these measurements close R1's
  open question by attributing the whole drop-in divergence to that one
  rule, on both real-data backends.

## zhuyin batch `FORCE_TONE` law — CLOSED (implemented in oxpinyin-core)

Initially reported as "parse restrictiveness" by the libzhuyin differential:
`zhuyin_parse_more_chewings` reported a non-zero `consumed` for toneless
syllables (`ta`=1, `li`=2, `ju`=2) where the pin reported 0. Root cause was
**not** the syllable-validation gate — it was the **batch parser ignoring
`FORCE_TONE`**.

- **Upstream source cite:** `src/zhuyin.cpp:1061` (batch chews pass
  `context->m_options`), `src/zhuyin.cpp:273` (`zhuyin_init` seeds
  `USE_TONE | FORCE_TONE`), `src/storage/zhuyin_parser2.cpp:176-180` (Simple
  `parse_one_key` rejects a toneless syllable under `FORCE_TONE`, nested
  under `USE_TONE`; `:373,:387` for Discrete; `:602` for CP26).
- **Mechanism:** with `FORCE_TONE` (part of the pin's zhuyin default), the
  batch parse rejects a syllable that carries no tone.
- **What oxpinyin now does:** `oxpinyin_core::ZhuyinParser::parse_with_options`
  was added (additive — the existing three-argument `parse`, used by the
  pinyin facade's `pinyin_parse_more_chewings`, is unchanged) to model the
  pin's option-word batch law. `oxpinyin-zhuyin-capi`'s
  `zhuyin_parse_more_chewings` passes the caller's full option word, so
  `FORCE_TONE` is honoured. `KeyProbe` now carries `force_tone` and rejects a
  toneless match per keyboard family (nested under `USE_TONE` for Simple and
  CP26, unconditional for Discrete).
- **Externally observable:** the differential now converges on the batch
  parse: with `USE_TONE | FORCE_TONE` both sides report `ta`=0, `li`=0,
  `ju`=0, `su3`=3, `ke3`=3 for the STANDARD keyboard.

## zhuyin candidate-tag grouping + `after(consumed)` terminal offset — CLOSED (display-law collapse + builder terminal mapping)

- **Re-closed 2026-10-03 (#577; #623, merge commit `cb824981`, code
  `7bea86d1`; policy rows 25 and 26):** a mid-key after-cursor lookup
  answers the sentence rows alone (`zhuyin.cpp:1498-1512`, `:1624-1626`);
  `zhuyin-diff` now drives offsets 1, 2, 4 and 5 of both families and a
  choose battery, 4558 lines identical on tkrzw, bdb and kc; the
  choose-then-guess half is #609 (`563ed9b1`).
- **Contradicted by #577 (2026-09-27 UTC; audit Z-3; policy rows 25
  and 26), pending investigation — the record this closure answers.** On all three cells, for the chewing
  input `su3cl3` after `zhuyin_guess_sentence`,
  `zhuyin_guess_candidates_after_cursor` at offsets 1, 2 and 5 answers
  n=1 on the pin and n=126/126/94 on oxpinyin, and after a choose
  `zhuyin_guess_candidates_before_cursor` answers 94/126 on the pin and
  1/94 on oxpinyin (pin `src/zhuyin.cpp:1460-1541`, `:1542-1600`;
  oxpinyin `crates/oxpinyin-zhuyin-capi/src/sentence.rs:181`, `:198`).
  `zhuyin-diff` drives no mid-key offset. The CLOSED status below is
  contradicted until #577 is investigated.

- **Upstream source cite:** `src/zhuyin.cpp:1272-1291`
  (`_prepend_sentence_candidates` prepends `m_nbest_results.size()`
  `BEST_MATCH_CANDIDATE` rows), `src/zhuyin.cpp:1460-1540`
  (`zhuyin_guess_candidates_after_cursor` returns `true` for a valid lookup
  into a non-empty matrix even with no candidate spanning the offset),
  `src/zhuyin.cpp:1542` (`zhuyin_guess_candidates_before_cursor`, same rule).
- **Mechanism:** (1) the pin prepends exactly `m_nbest_results.size()`
  sentence rows as `BEST_MATCH_CANDIDATE`, so on `su3` it tags one row
  `BEST_MATCH` and the rest `AFTER`; the engine's `candidates_at` emits a
  `Sentence` row per n-best sentence (two for `su3`), so the facade tags the
  second row `BEST_MATCH` too — candidate set and count identical (125), one
  label differs (ORDER-ONLY-like). (2) for an offset equal to the consumed
  length (`after(consumed)`), the pin returns `true` with 0 candidates (the
  matrix is non-empty); the facade returns `false` because the normalized
  offset is in original zhuyin-input coordinates while `candidates_at`
  expects session raw-buffer coordinates.
- **What oxpinyin does instead:** the facade's 4-value-enum tagging is
  faithful to the pin's prepend law; the row-count difference is the engine's
  n-best construction. The `after(consumed)` terminal-offset `false` is class
  (c) — the pin returns `true`, oxpinyin `false` — and stems from the same
  candidate-construction gap (coordinate mismatch between the original
  zhuyin input offset and the session's `'`-joined raw buffer).
- **Externally observable:** yes — `zhuyin_get_candidate_type` differs on the
  row after the top match, and `zhuyin_guess_candidates_after_cursor`
  answers `false` at `offset == consumed` where the pin answers `true`/0.
- **Classification:** engine workstream (n-best row count) + terminal-offset
  availability. Neither is a facade defect: the candidate
  set is identical, and the terminal-offset case is not exercised by the
  pinned differential driver. The `after(consumed)` coordinate gap and the
  multi-syllable before-cursor and candidate-construction gaps below share the
  same root cause: `session.candidates()` is forward-anchored and the facade
  cannot UNION multiple `candidates_at` windows. One implementation direction
  covers all three: the backward-anchored window builder; see the next entry.

  (Amended 2026-08-31: the tag-grouping half is CLOSED — the row-count
  divergence was the **string-fill law**, not the n-best constants. Upstream
  zhuyin fills every `BEST_MATCH_CANDIDATE` row through `zhuyin_get_sentence`,
  which always reads `get_result(0)` (`zhuyin.cpp:1327-1330`, `:990-995`),
  unlike the pinyin surface's per-index `pinyin_get_sentence`
  (`pinyin.cpp:2004-2007`); identical strings collide in
  `_remove_duplicated_items_by_phrase_string`, which physically removes the
  duplicates (`zhuyin.cpp:1425-1438`), so exactly one sentence row is
  observable regardless of the n-best count. oxpinyin was applying the pinyin
  per-row law on the zhuyin surface. Fixed scheme-locally: the zhuyin facade
  sets `Session::set_collapse_sentence_rows_to_best(true)` and the prepend
  rides only the 1-best row — leaving `NSTORE`/`NBEST_ROWS` (and the shared
  trellis) untouched. Measured on the full-row differential
  (`tools/bisection/zhuyin-diff.c` now dumps every row): before the fix 253
  oracle rows vs 254 oxpinyin rows with `su3` candidate[1] `AFTER`/尼 vs
  `BEST_MATCH`/尼 and `su3u3` `after(0)` 128 vs 129 with candidate[1]
  `AFTER`/拟议 vs `BEST_MATCH`/你以 — after the fix the driver is byte-identical
  (revert-and-check: reverting the two source edits reproduces the 259-line
  diff). The `after(consumed)` terminal-offset half stays open for the
  backward-anchored window builder.)

  (Amended 2026-08-31, second half: the `after(consumed)` terminal offset is
  CLOSED by the same builder change — the original-offset mapping is now
  direction- and terminal-aware (`zhuyin_lookup_session_offset`): the
  terminal lookup answers the session buffer's one-past-end (the pin's
  reserved slot, where the span walk yields nothing and only the prepended
  sentence rows answer — `true` with the BEST_MATCH row, measured identical
  on the extended differential at after(consumed) for every corpus input),
  closing the coordinate gap. The tag-grouping and terminal-offset
  halves of this entry are both closed; see the before-cursor entry for the
  builder's full measured numbers.)

## zhuyin before-cursor candidate window — CLOSED (backward-anchored window builder)

- **Re-closed 2026-10-03 (#577; #623, merge commit `cb824981`, code
  `7bea86d1`; policy rows 25 and 26):** a mid-key after-cursor lookup
  answers the sentence rows alone (`zhuyin.cpp:1498-1512`, `:1624-1626`);
  `zhuyin-diff` now drives offsets 1, 2, 4 and 5 of both families and a
  choose battery, 4558 lines identical on tkrzw, bdb and kc; the
  choose-then-guess half is #609 (`563ed9b1`).
- **Contradicted by #577 (2026-09-27 UTC; audit Z-3; policy rows 25
  and 26), pending investigation — the record this closure answers.** On all three cells, for the chewing
  input `su3cl3` after `zhuyin_guess_sentence`,
  `zhuyin_guess_candidates_after_cursor` at offsets 1, 2 and 5 answers
  n=1 on the pin and n=126/126/94 on oxpinyin, and after a choose
  `zhuyin_guess_candidates_before_cursor` answers 94/126 on the pin and
  1/94 on oxpinyin (pin `src/zhuyin.cpp:1460-1541`, `:1542-1600`;
  oxpinyin `crates/oxpinyin-zhuyin-capi/src/sentence.rs:181`, `:198`).
  `zhuyin-diff` drives no mid-key offset. The CLOSED status below is
  contradicted until #577 is investigated.

The facade's `zhuyin_guess_candidates_before_cursor` originally reused the
composition-anchored cached candidate window, so `before(0)` wrongly returned
125 word candidates where the pin returns 0 (nothing precedes the first key).
That facade bug is fixed, but the fix is **correct only for a single-syllable
composition**; multi-syllable before-cursor is a genuine engine gap.

- **What oxpinyin now does (single-syllable):** the before-cursor path takes
  the composition window and filters to candidates whose consumed span ENDS
  at the requested original-offset (`snapshot_candidates`'s `before_end`). At
  offset 0 no span ends there (empty window, matching the pin); at the
  terminal offset the syllable's candidates are returned. `before(0)`=0 and
  `before(consumed)` match the pin on the single-syllable differential corpus.
- **Multi-syllable (measured on the two-syllable `su3u3`):** `before(3)`
  (first key boundary) matches the pin (125 on both), but `before(consumed)=5`
  does NOT — the pin returns 600 (the last key's 597 candidates plus the
  whole-composition sentence rows), oxpinyin returns 3. Root cause: the
  engine's forward-anchored `session.candidates()` does not enumerate the
  trailing keys' candidates, and the facade cannot UNION multiple
  `candidates_at` windows into one `CandidateList` (the engine does not
  expose `Candidate`/`CandidateList` construction). Fixing it requires an
  engine change: a backward-anchored window builder (the pin's
  `search_matrix` walk over spans ending at the offset).
- **Externally observable:** yes — `zhuyin_get_n_candidate` differs for
  `before(consumed)` on a multi-syllable composition. Registered as engine
  workstream, not a facade defect (the single-syllable ABI surface is
  correct).

  (Amended 2026-08-31: CLOSED — the engine gained the backward-anchored
  window builder `Session::candidates_ending_at(offset)` (additive, the
  `parse_with_options` seam pattern): the prefix graph's scan matrix walked
  per start `0..offset` ascending — the pin's longest-span-first `len` loop
  (`zhuyin.cpp:1575-1631`) — each span's slice ranked by the three-key order
  with its own previous-token gram, groups concatenated, the sentence rows
  prepended, one text dedup. The facade maps the original offset
  directionally (`zhuyin_lookup_session_offset`): the after family takes the
  right-key start, the before family the LEFT-KEY END — a mid-composition
  boundary is the apostrophe byte in the `'`-joined buffer, and upstream's
  walk answers the left syllable's candidates there — and the terminal
  offset answers the buffer's one-past-end. Sentence rows are exempt from
  the facade's before-end filter (the prepend law has no offset condition).
  Decomposition first, per review — measured on the instrumented oracle at
  0c5e80e1: `before(5)` on su3u3 = span (0,5) 3 phrase candidates + span
  (3,5) 597 + mid-syllable starts (1,2,4) no match (empty columns,
  `SEARCH_NONE`) = 600 phrases, +1 sentence row, −1 string-duplicate → 600;
  the register's earlier "597 + sentence rows" arithmetic was a
  simplification, as suspected. Measured differential (the driver now dumps
  n/TEXT/TYPE in full at after(0), after(consumed), before(0), before(3),
  before(consumed), over 11 inputs including three-syllable `su3u3u3`):
  before the fix `before(consumed)` on su3u3 is 3 (oxpinyin) vs 600 (pin)
  and `before(3)` 1 vs 126; after the fix the extended driver is
  byte-identical (revert-and-check: reverting the builder and facade edits
  reproduces a 1728-line diff).

  (Amended 2026-09-01, **STOP record and protocol split** — supersedes the
  "surface shift" framing the 2026-08-31 amendment closed with. **A STOP
  fired and was overridden.** The work order's STOP read: *"`before(3)` on
  `su3u3` ceasing to be 125 on both sides."* The Phase-2 baseline measured
  **1 (oxpinyin) vs 126 (pin)** on exactly that query, and the work
  continued on a reinterpretation ("agreement preserved") instead of
  stopping and reporting. Neither number in that baseline was the
  register's datapoint, but that is a finding, not an excuse: oxpinyin's 1
  came from a mid-implementation builder state (the exact-segment graph
  bug, since fixed) — not from the pre-change path — and the pin's 126 came
  from the driver's new guess-first sequence, where
  `zhuyin_guess_sentence` populates `m_nbest_results` and the prepended
  BEST_MATCH row rides every before-cursor answer (the prepend law has no
  offset condition). Re-measured on BOTH protocols, both sides, with the
  register-era binary rebuilt at its own base (`1451211`; the driver gained
  a `noguess` 4th argument so the protocol is a flag, not an accident):

  | su3u3 `before(3)` | pin | oxpinyin @ 1451211 | oxpinyin @ this branch |
  |---|---|---|---|
  | parse only (no `guess_sentence`) | 125 | **125** | 125 |
  | `guess_sentence` first | 126 | **125** | 126 |

  What this settles. The register's "125 on both" was a **real measurement
  under the parse-only protocol**, and the boundary agreement it recorded
  was real: at the FIRST key boundary every span ending at the offset also
  starts at the composition anchor, which is precisely the degenerate case
  the composition-anchored filter handles — the entry's single-syllable
  scoping was drawn from a datapoint that existed. But it was
  protocol-bound: under guess-first the register-era path never matched
  (125 vs 126 — its filter applied the span test to the sentence row and
  dropped it, where the pin prepends regardless of offset), so
  `before(3)` agreement at that boundary was always an artefact of the
  unguessed sequence. The final state matches the pin under BOTH protocols
  (125/125 parse-only, 126/126 guess-first), so the boundary now agrees
  for the right reason — the builder — rather than by the filter's
  coincidence.

  **Single-syllable closure re-examined — and one half of its recorded
  reason corrected.** The entry's closure sentence — "`before(0)`=0 and
  `before(consumed)` match the pin on the single-syllable differential
  corpus" — names no protocol, and under the guess-first protocol one half
  of it was false at the base: `before(0)` answered 0 against the pin's 1
  (the prepended row the old filter dropped). The same protocol-bound
  pattern as `before(3)`, a second instance, not a free-standing wrinkle.
  Re-measured on su3, both protocols, both sides:

  | su3 surface | protocol | pin | oxpinyin @ 1451211 | oxpinyin @ this branch |
  |---|---|---|---|---|
  | `before(0)` | parse only | 0 | 0 | 0 |
  | `before(0)` | guess first | 1 | **0** | 1 |
  | `before(consumed)` | parse only | 125 | 125 | 125 |
  | `before(consumed)` | guess first | 125 | 125 | 125 |

  So the closure sentence held in full only under the parse-only protocol;
  under guess-first only its `before(consumed)` half matched (the
  terminal-offset row survived the old filter because the sentence row's
  span maps to the whole parse, which equals the terminal offset).
  The closure itself stands at the final state under BOTH protocols and
  now rests on the general mechanism (the builder) rather than the
  filter's coincidence — but its recorded reason is now protocol-accurate.

  (Amended 2026-09-01, **baseline column re-measured** — the correction
  the before(3) STOP record forced, applied to the whole A2 baseline.
  The A2 commit message's baseline cells came from a mid-implementation
  run and are superseded by this table, measured at the base itself
  (1451211 rebuilt) under the declared guess-first protocol:

  | su3u3 surface | pin | oxpinyin @ 1451211 | oxpinyin @ this branch |
  |---|---|---|---|
  | `after(0)` | true/128 | true/129 | true/128 |
  | `after(consumed)` | true/1 | true/**2** | true/1 |
  | `before(0)` | true/1 | true/**0** | true/1 |
  | `before(3)` | true/126 | true/**125** | true/126 |
  | `before(consumed)` | true/600 | true/**4** | true/600 |

  Corrections this forces on the record:
  - The A2 baseline's "`after(consumed)` false vs true" is wrong twice
    over. At 1451211 the facade answers **true** — with 2 rows, both
    uncollapsed sentence rows riding the empty-column prepend after the
    coordinate mismatch landed the lookup on a mid-syllable column of the
    `'`-joined buffer — not false; and the pin answers true/**1** under
    this protocol, not true/0 (true/0 is the parse-only shape, which is
    what the entry's original "pin returns true with 0 candidates"
    observable measured). The coordinate-mismatch mechanism stands; the
    recorded observed value was true/2 vs true/1, the tag-grouping
    divergence itself.
  - The "3 vs 600" `before(consumed)` cell matches the parse-only shape
    (re-measured: 3 vs 600 there) and agrees with this entry's original
    registration, so that cell stands as recorded — but at the declared
    protocol the base answers **4** (the two uncollapsed sentence rows
    survive the old filter at the terminal offset; dedup then drops one
    against a same-string phrase).
  - The "`before(3)` 1 vs 126" cell was the contaminated one that fired
    the STOP; the base's measured value at the declared protocol is
    125, per the table above.)

  (Amended 2026-09-05, **the choose leg of the same surface** — the window
  this entry closed was displayed correctly but could not be chosen from.
  `zhuyin_choose_candidate` resolved the snapshot row's `source_index`
  through `Session::select`, which indexes the composition-anchored cached
  list, while `snapshot_candidates` had recorded that index against the
  `candidates_ending_at` window the before-cursor guess built. The two
  lists differ in general, so the committed text could be a row the caller
  never displayed. Measured on `fixtures/w3` (redb backend), `su3cl3` —
  `ni3'hao3` in the session's `'`-joined buffer — with `before(6)`:

  | `before(6)` row 1 | displayed | committed | cursor answered |
  |---|---|---|---|
  | before this fix | 好 | **`你'hao`** | **3** |
  | after this fix | 好 | 好 | 6 |

  The cached list's row 1 is 你 (the FIRST key's span, `consumed_bytes` 3);
  the before-cursor window's row 1 is 好 (the SECOND key's span, ending at
  the cursor). Choosing the displayed 好 committed 你 and left the raw
  `hao` tail in the buffer, and answered the first key's end as the new
  cursor.

  **A defect, not a divergence.** Upstream has no equivalent indirection:
  `zhuyin_choose_candidate` receives a `lookup_candidate_t *` carrying the
  row's own token, so no index is resolved against a second list and the
  mismatch is not expressible there. Per this policy's rule that anything
  outside the four exception classes is a defect to be reverted, the fix
  moves toward the pin and needs no class.

  **The fix.** The before-cursor window is retained in
  `InstanceCore::anchored_window` exactly as the after-cursor one is, so
  the later choose resolves through `Session::select_anchored` against the
  same list — in the zhuyin C ABI (`oxpinyin-zhuyin-capi`; a former Python
  `ZhuyinSession` mirror was retired with the binding). The
  anchor is `oxpinyin_facade::BEFORE_CURSOR_ANCHOR` = 0, not the lookup
  offset: `candidates_ending_at` scans the prefix graph `raw[..offset]`,
  whose coordinates are absolute from the buffer start, so every row's
  `consumed_bytes` is the span's absolute END rather than a length
  measured from a start. `select_inner` reads the chosen span as
  `[anchor, anchor + consumed_bytes)`, so 0 is the only anchor that
  reproduces the span end and the consumed advance; reusing the
  after-cursor anchor would read `[offset, 2 * offset)` and walk the
  composition off the end of the buffer.

  **Coverage.** A C-side test pins the property on the zhuyin ABI —
  `oxpinyin-zhuyin-capi`'s
  `choosing_from_a_before_cursor_window_uses_that_window`, the twin of
  `oxpinyin-capi`'s
  `choosing_from_a_reanchored_window_uses_the_anchored_span`. Revert-and-check:
  reverting the facade edit fails the C test at the cursor assertion
  (3 vs 6).

  **Residual — OPEN.** The fix makes the
  committed row the displayed row; it does not make the before-cursor
  choose pin-identical. For a row whose span STARTS after the composition
  offset, the chosen span is recorded as `[0, offset)` rather than
  `[start, offset)`, so the raw bytes before the span are absorbed into the
  chosen text instead of being decoded — `su3cl3` `before(6)` row 1 commits
  `好`, where upstream's constrain-and-re-decode model would keep a
  conversion for the leading key as well. Root cause:
  `oxpinyin_engine::Candidate` carries `consumed_bytes` but no span START,
  so a window whose rows have per-row begins cannot be resolved row by row;
  the pin's rows carry `m_begin` each (`pinyin.cpp:2227`, the `m_begin =
  start` already cited in `Session::select_anchored`'s doc comment) and its
  caller passes that begin as the choose offset, where this facade
  deliberately ignores its `offset` argument. Closing it needs `Candidate`
  to carry its span start plus an end-anchored `Session::select_*` — both
  changes to `oxpinyin-engine`'s supported surface, so the contributor approval
  ("needs interface change") applies and the gap is registered rather than
  improvised. (The paragraph's pin-side claims were written without an
  oracle on the authoring host and are superseded by the measured record
  below.)

  (Amended 2026-09-05, **the residual measured against the pin** — the
  owed differential run, on the pin-built oracle
  (`/tmp/oxpinyin-zhuyin-oracle`, `oracle-pin.txt`: libpinyin 2.11.91 at
  `0c5e80e1`, model20 `59c68e89…`, Tkrzw, `shared_object_sha256`
  `5cb23f8b…`) against this branch (`ab36a43c`); the Rust systemdir was
  recompiled from the same SHA-verified model20 export with
  `oxpinyin-datagen compile --backend tkrzw` — the runtime's
  default-backend switch (`05688575`) had made the session's `.redb` dir
  stale. Pin source re-read at the pin (`git show 0c5e80e1:src/zhuyin.cpp`):
  `zhuyin_choose_candidate` for a `NORMAL_CANDIDATE_BEFORE_CURSOR` row
  does `constraints->add_constraint(candidate->m_begin,
  candidate->m_end, token)` and `offset = candidate->m_begin` — the
  `m_begin` law, now measured, not just cited. The driver gained the
  default-off `choose` battery for it (`tools/bisection/zhuyin-diff.c`:
  parse, `guess_sentence` — the consumer protocol — then
  `guess_before(consumed)`, choose row 1, the answered cursor,
  `get_sentence` after the choose and after the next re-guess, over
  `su3cl3`/`su3u3`/`su3u3u3`); the standing default battery is unchanged
  (byte-identical, 2307 lines both sides). Measured, row 1 of
  `before(consumed)` — the row is the same on both sides, windows
  identical (n 94/600/608, head rows equal):

  | input | row 1 (span, pin) | pin cursor | ox cursor | pin re-guess | ox re-guess |
  |---|---|---|---|---|---|
  | `su3cl3` | 好 `[3,6)` | **3** | 6 | 你好 | 你好 |
  | `su3u3` | 拟议 `[0,5)` | **0** | 5 | 拟议 | 拟议 |
  | `su3u3u3` | 意义 `[3,7)` | **3** | 7 | 你意义 | 你意义 |

  What the measurement settles, and what it corrects:
  - **The cursor divergence is universal, and it is the whole measured
    surface.** The pin answers the row's `m_begin` in every case —
    including a whole-composition row (拟议 `[0,5)` → 0), where this
    facade answers the composition end. This facade's law (the span END
    mapped to original coordinates) is measurable at every row: 6 vs 3,
    5 vs 0, 7 vs 3. Honest note: the fix above traded an accidental
    cursor agreement for the displayed-row property — pre-fix this
    facade answered 3 on `su3cl3` (= the pin, by accident of the
    wrongly-chosen 你's consumed length); post-fix it answers 6. The
    text side moved toward the pin (好, the row the caller actually
    saw); the cursor side moved from accidentally-right to wrong.
  - **The sentence-level claim is corrected.** The superseded
    paragraph's "the raw bytes before the span are absorbed into the
    chosen text instead of being decoded" does not show in the re-decoded
    1-best sentence: the pin's constrain-and-re-decode keeps 你好 AND so
    does this facade, on all three inputs. The `[0, offset)` constraint
    span is structurally real (the source law above), but no corpus
    input here flips the 1-best because of it; a sentence-level
    divergence would need an input where the constraint changes the
    decode, none found among the three.
  - **Classification, unchanged and now grounded.** The pin law is plain
    integer bookkeeping — reproducible in principle — so this is not one
    of the four exception classes; it is a gap the compatibility policy's
    corollary makes mandatory to close ("if implementing a symbol
    correctly requires an engine change, that engine change is
    mandatory"). The STOP stands (an `oxpinyin-engine` interface change
    needs an ask), so the entry stays OPEN, registered rather than
    improvised. Closure proof when the engine work lands: this battery
    must go byte-identical — cursor, sentence after the choose, and
    sentence after the re-guess across all three inputs, on both
    protocols.)


  (Amended 2026-09-06, **the residual CLOSED in code** — maintainer
  approval 2026-09-06 for the engine-interface change, with the
  instruction to copy what libpinyin's source does. `Candidate` now
  carries `span_start`, upstream's `m_begin`: 0 for a window measured
  from its anchor, the absolute span start for a before-cursor window
  (`zhuyin.cpp:1595`, `template_item.m_begin = start; m_end = offset`).
  `Session::select_inner` writes the constraint on
  `[anchor + span_start, end)` — `add_constraint(m_begin, m_end, token)`,
  `zhuyin.cpp:1652,1659` — so `su3cl3` `before(6)` row 1 forces `[3, 6)`
  (好) and leaves the leading key to the decode: the re-guess answers
  你好, the shape the 2026-09-05 measurement recorded on the pin. Two
  observable consequences, both toward the pin:
  1. `zhuyin_choose_candidate` answers `m_begin` for a before-cursor
     row (`offset = candidate->m_begin`, `zhuyin.cpp:1660`) — 3 here,
     where the facade answered the span's end (6). The after-cursor and
     BEST_MATCH returns are unchanged (`m_end`, `matrix.size() - 1`).
  2. A before-cursor choose after an earlier selection no longer
     fails: the old `[0, offset)` span regressed the composition offset
     and hit `SelectionAnchorBeforeComposition`; the row's own start
     does not.
  The engine's `commit()` — a Rust/Python surface with no ABI
  counterpart — keeps the leading key's raw bytes as a typed-but-
  unselected gap (`ni好`), the same law an after-cursor re-anchor
  already applied; the ABI surface commits `zhuyin_get_sentence`'s
  re-decode, which is 你好. Pinned by the engine test
  `choosing_a_before_cursor_row_constrains_its_own_span`, the C test
  `choosing_from_a_before_cursor_window_uses_that_window` (cursor 3,
  free cell at 0, forcing at 3, sentence 你好 after the re-guess) and the
  zhuyin parity corpus.)

  (Amended 2026-09-08, **the owed battery run — IDENTICAL.** In the
  perf-matrix container (`tools/bisection/Dockerfile.perf-matrix`,
  arm64), libzhuyin built at the current pin 074a2219 with
  `--with-dbm=Tkrzw --enable-libzhuyin` over the SHA-verified model20
  export (`libzhuyin.so.15.0.0` sha256 `2109e10c…`, 23 data files built
  by the pin's own `data/` step) against this tree's
  `libzhuyin_capi.so` (tkrzw, then the default) reading an
  `oxpinyin-datagen compile --backend tkrzw` systemdir from the same
  export. `tools/bisection/zhuyin-diff.c`: the standard battery is
  byte-identical (2307 lines a side) and the default-off `choose`
  battery is byte-identical (65 lines a side) — cursor 3 / 0 / 3 on
  `su3cl3` / `su3u3` / `su3u3u3`, `get_sentence` after the choose 你好 /
  你一 / 你意义 and after the re-guess 你好 / 拟议 / 你意义 on both
  sides, windows 94 / 600 / 608 with row 1 好 / 拟议 / 意义. The
  2026-09-05 table's three cursor cells (6/5/7 vs the pin's 3/0/3) are
  the ones #374 closed; nothing else in the battery moved. Build note
  for the next run: libpinyin's `configure` takes the DBM in its own
  spelling (`Tkrzw`, as `tools/oracle/build-oracle.sh` maps it) — the
  lower-case form is accepted silently and leaves every backend type
  undefined.)

## zhuyin multi-syllable candidate construction — CLOSED (the zhuyin display law, not the construction model)

The two-syllable differential input `su3u3` (ㄋㄧˇ ㄧˇ, consumed 5) exposes a
broader divergence than the `after(consumed)`/tag-gap above: the multi-syllable
candidate construction itself differs on the count, the phrase set, and the
tags.

- **Measured (oracle vs oxpinyin on `su3u3`):**
  - `n_candidates` at `after(0)`: **128 (pin) vs 129 (oxpinyin)** — the first
    candidate count differs (oxpinyin emits one extra row).
  - `candidate[1]` TEXT: pin `拟议`/`逆夷` at 1-2, oxpinyin `你以` at 1 — the
    multi-syllable phrase set diverges.
  - `candidate[1]` TYPE: pin `AFTER`, oxpinyin `BEST_MATCH` — oxpinyin carries
    a second `BEST_MATCH` row (`你以`) where the pin has exactly one sentence
    row (`你一` at 0).
  - `before(consumed=5)`: **600 (pin) vs 3 (oxpinyin)** — the before-cursor
    window diverges.
  - `before(3)` (first key boundary): 125 on both — matches.
- **Root cause:** the engine's forward-anchored `session.candidates()` builds
  the composition's candidates from the start; it does not enumerate the
  trailing keys' candidates (per-key at each position the way the pin's
  `search_matrix` walk does), and the facade cannot UNION multiple
  `candidates_at` windows into one `CandidateList` (the engine does not expose
  `Candidate`/`CandidateList` construction). The facade's tagging LAW is
  faithful to the pin's prepend rule, but the engine feeds it two sentence
  rows where the pin's `m_nbest_results` holds one, so the observed TYPE
  values differ as well (`candidate[1]`: `BEST_MATCH`/你以 vs the pin's
  `AFTER`/拟议).
- **Classification:** engine workstream (the candidate-construction model).
  Not a facade defect — the single-syllable facade surface is correct, and the
  facade faithfully translates what the engine provides. The differential
  corpus documents this as the known multi-syllable gap. This entry, the
  before-cursor multi-syllable gap above, and the `after(consumed)` / tag
  grouping entry all share one underlying cause and one implementation
  direction: the backward-anchored window builder mirroring the pin's
  `search_matrix` walk over spans ending at the offset.

  (Amended 2026-08-31: CLOSED — the divergence was the pinyin string-fill law
  riding the zhuyin surface, not the candidate-construction model: every
  symptom (count +1, `candidate[1]` TEXT `你以` vs `拟议`, TYPE
  `BEST_MATCH` vs `AFTER`) traces to the second n-best sentence row
  surviving where upstream zhuyin's display law collapses all sentence rows
  onto the 1-best string. See the candidate-tag grouping entry above for the
  law, the fix shape (`Session::set_collapse_sentence_rows_to_best`, set by
  the zhuyin facade only), and the measured before/after numbers. The full-row
  differential is byte-identical after the fix, revert-and-check proven.
  The `before(consumed)` half of the measured list stays open in the
  before-cursor entry above — that is the backward-anchored window builder's,
  not this entry's.)

## zhuyin n-best trellis constants: `PhoneticLookup<1, 1>` vs the engine's `<2, 3>` port — CLOSED in code (2026-09-06)

- **Upstream source cite:** `src/zhuyin.cpp:50` (`PhoneticLookup<1, 1> *
  m_pinyin_lookup` — `nstore = 1`, `nbest = 1` for libzhuyin) vs
  `src/pinyin.cpp:55` (`PhoneticLookup<2, 3>` for libpinyin); the beam/tail
  selection is `src/lookup/phonetic_lookup.h:330-341` (`get_tails` caps the
  results with `get_top_results<nstore>(nbest, …)`).
- **Mechanism:** the two upstream surfaces instantiate the same beam search
  with different constants. libzhuyin keeps ONE value per
  `(position, token)` trellis node and extracts at most ONE sentence tail;
  libpinyin keeps two and up to three. The candidate-list half of the
  observable difference is masked by the zhuyin display law (see the
  candidate-tag grouping entry — every sentence row displays the 1-best
  string and the dedup collapses them), but the trellis depth itself (which
  `(position, token)` values survive pruning, and with them the constraint
  and training walks' available rows) genuinely differs between the
  surfaces.
- **What oxpinyin does instead:** `crates/oxpinyin-engine/src/nbest.rs`
  hardcodes `NSTORE = 2` / `NBEST_ROWS = 3` — the pinyin instantiation — for
  both surfaces. Surfaced by the Phase-1 work on the candidate-window
  builder (2026-08-31); an earlier reading took the constants for the cause
  of the row-count divergence, which the string-fill law turned out to be.
- **Externally observable:** not through today's libzhuyin candidate
  surface (`zhuyin.h` exposes no per-index sentence getter, and the display
  law collapses the list), so no differential row moves today. It becomes
  observable through any future per-row sentence access on the zhuyin
  surface, and the pruning depth is measurable against the pin's
  constraint/train behaviour. Fix shape when taken: per-surface constants
  through const generics (the `parse_with_options` additive pattern), NOT a
  global constant edit — the full-pinyin corpus pins freeze the `<2, 3>`
  behaviour.

  (Amended 2026-09-06, **CLOSED in code** — maintainer approval
  2026-09-06. The pair is a per-session `oxpinyin_engine::NbestShape`
  — `PINYIN` = `<2, 3>` (`pinyin.cpp:55`), `ZHUYIN` = `<1, 1>`
  (`zhuyin.cpp:50`), `Default` = `PINYIN`, no other constructor so
  upstream's `nstore <= nbest` assert (`phonetic_lookup.h:715`) cannot
  be violated — threaded through `nbest_sentences` into the trellis's
  per-node cap and tail count, and set by both zhuyin facades at
  instance allocation beside the display law
  (`Session::set_nbest_shape(NbestShape::ZHUYIN)`). The fallback DP's
  row cap follows the shape too. Pinned by
  `the_zhuyin_shape_extracts_a_single_tail`: two readings of `ni` give
  the pinyin shape two rows and the zhuyin shape one, the same 1-best.
  Per-surface const generics were the recorded fix shape; a per-session
  value is the same law without changing the `Session` type every
  consumer names. Not observable through today's libzhuyin candidate
  surface, as the entry says; the pruning depth now matches the pin for
  whatever surface exposes it next.)

## zhuyin `FORCE_TONE` / `ZHUYIN_INCOMPLETE` default

- **Upstream source cite:** `src/zhuyin.cpp:273` (`context->m_options =
  USE_TONE | FORCE_TONE`; no `ZHUYIN_INCOMPLETE`).
- **Mechanism:** `zhuyin_init` seeds `USE_TONE | FORCE_TONE` and nothing
  else. `FORCE_TONE` is honoured by the chewing parser nested inside
  `USE_TONE` for the Simple / CP26 keyboards and unconditionally for
  Discrete (`zhuyin_parser2.cpp:178,373,387,602`); `ZHUYIN_INCOMPLETE` is
  OFF by default.
- **What oxpinyin does instead:** `CapiContext::try_open` seeds the same
  `USE_TONE | FORCE_TONE` word (`ZHUYIN_DEFAULT_OPTIONS`,
  `crates/oxpinyin-zhuyin-capi/src/state.rs`) and defaults `incomplete` to `false`
  (matching the pin). The FORCE_TONE law is delegated to
  `oxpinyin_core::ZhuyinParser::parse_with_options`, which honours it in the
  Simple/CP26 (nested) and Discrete (unconditional) shapes — the
  implementation matches the pin's three shapes.
- **Externally observable:** no, with the corrected default — the differential
  was run with the pin's default word (`USE_TONE | FORCE_TONE`, no
  `ZHUYIN_INCOMPLETE`) and the parse-length gap above is the only residual.
  Entry kept so a future consumer that sets the bit finds the law already
  analysed.

## pinyin-facade chewing batch seam does not forward `FORCE_TONE` — CLOSED (the whole word crosses the seam)

Surfaced by the bopomofo SPEC audit (2026-09-03). The FORCE_TONE entry
above enumerates the full-pinyin batch, the double one-key, the zhuyin
one-key, the libzhuyin batch and the double-pinyin batch seams; the pinyin
facade's chewing batch seam — `pinyin_parse_more_chewings`, the function the bopomofo SPEC
freezes — was never in that enumeration. This entry completes it.

- **Upstream source cite:** `src/pinyin.cpp:1582-1609` (at pin `0c5e80e1`)
  (`pinyin_parse_more_chewings` passes `context->m_options` with only
  `ZHUYIN_CORRECT_ALL` stripped, the strip at `:1589`);
  `src/storage/zhuyin_parser2.cpp:171-179` (Simple `parse_one_key`:
  `FORCE_TONE` nested under `USE_TONE`), `:373` and `:387` (Discrete),
  `:595-605` (CP26, nested); `pinyin_init` seeds `USE_TONE` alone
  (`src/pinyin.cpp:329`), so the bit is caller-set on this facade.
- **Mechanism:** the pin forwards the whole option word, so a
  `pinyin_context_t` consumer that sets `FORCE_TONE` gets the per-keyboard
  law — a toneless Simple/CP26 syllable and any Discrete key without a
  tone byte refuse, and the greedy walk stops there.
- **What oxpinyin does instead:** `parse_chewing_more`
  (`crates/oxpinyin-capi/src/parse.rs`) calls the three-argument
  `ZhuyinParser::parse(text, use_tone, allow_incomplete)`, so `FORCE_TONE`
  never crosses and the tone-less profile runs whatever the caller's word.
  The comment that justified forwarding one bit — "`ZHUYIN_INCOMPLETE` is
  the one caller bit the Simple parser consults", citing `pinyin.cpp:1621`
  — was false at the pin on both counts and was the reason the seam was
  built this way; corrected in dea0880 together with the SPEC paragraph it
  was copied into. The libzhuyin facade's batch seam forwards the whole
  word through `ZhuyinParser::parse_with_options` (1671954); the fix shape
  here is the same one-line forward of `inst.options().bits()` plus a
  `FORCE_TONE` profile in `tools/bisection/chewing-diff.c`, whose only
  option word today is `IS_ZHUYIN | ZHUYIN_INCOMPLETE | USE_TONE`
  (`chewing-diff.c:53`).
- **Externally observable:** yes, but only under a caller-set `FORCE_TONE`
  — not the pin's `pinyin_init` default and not the bopomofo differential's
  word, which is why every existing gate is IDENTICAL while the seam is
  unimplemented. Under `USE_TONE | FORCE_TONE` the pin consumes 0 on
  toneless `su` (STANDARD ㄋㄧ) where oxpinyin consumes 2. No class among
  (a)–(d) of `compatibility-policy.md` fits: this is a defect to close,
  registered so the bopomofo SPEC freeze can name it as its open
  implementation item rather than stay silent, exactly as the double-pinyin
  freeze carried its batch gate.

  (Amended 2026-09-14: **CLOSED in code.** The `PinFacade` arm of
  `parse_chewing_more` now forwards `self.options().bits() &
  !ZHUYIN_CORRECT_ALL` through `ZhuyinParser::parse_with_options` — the
  pin's exact `pinyin.cpp:1582-1609` law, re-verified against the pin
  checkout (`src/pinyin.cpp` blob `f27f7cf7` at `074a2219`). The
  capi-level tests (`chewing_batch_force_tone` in
  `crates/oxpinyin-capi/src/e2e_tests.rs`) pin the register's measured
  shape — toneless `su` refuses under `USE_TONE | FORCE_TONE`, `su3`
  parses, the strip changes nothing observable on STANDARD, and
  `FORCE_TONE` without `USE_TONE` stays nested-inert — and
  `tools/bisection/chewing-diff.c` gained the FORCE_TONE profile pass
  the entry prescribed. The live pin-vs-oxpinyin run of that profile was
  taken 2026-09-16 (amendment below); every existing gate stays
  IDENTICAL because none sets the bit, so until that run the closure
  rested on the unit-level law plus the profile, in the same shape the
  NbestShape closure (row 28) rested on engine tests without a moving
  gate.)

  (Amended 2026-09-16: **live differential RUN — IDENTICAL on all eight
  implemented keyboards.** `run-scheme-diff.sh bopomofo N` for N = 1–6,
  8, 9 (STANDARD, HSU, IBM, GINYIEH, ETEN, ETEN26, HSU_DVORAK,
  DACHEN_CP26; 7 STANDARD_DVORAK aborts the pin and is never sent) each
  exited 0 IDENTICAL with no `SKIP:` line and both sides visibly
  executed. Environment: a `debian:testing` container (image
  `docker.io/library/debian@sha256:5056ab8a99336d6d71390d640f72229649f12e3d38e987cf6b24dc8675325d73`,
  container `a15849ef7dd2`), the pin oracle prefix mounted read-only,
  both sides on tkrzw — the oracle is the `dbm=tkrzw` parity build and
  the default `cargo build -p oxpinyin-capi` then resolved the tkrzw backend (Berkeley DB since 2026-09-20) —
  with the capi on a P6-native data directory (libpinyin-native
  `.bin`/`.db` naming). The measurement tree was `main` @ `93a462f`
  overlayed with PR #476's `system-dir.sh` (the native-layout
  acceptance; these records change no runner or driver). The
  full both-side driver logs for `bopomofo 1` run 1512 lines each; the
  FORCE_TONE profile section (log line 889) carries the toneless `su`
  on STANDARD identically on both sides: `parse_chewing: consumed=0`,
  `parsed_input_length: 0`, `guess_sentence: false`,
  `n_candidates: 0`. Non-vacuity: reverting the seam alone in a
  throwaway worktree (`crates/oxpinyin-facade/src/parse.rs` and
  `crates/oxpinyin-core/src/scheme.rs` at their pre-`6616fb08` state,
  the driver's FORCE_TONE profile kept) turns `bopomofo 1` into exit 2
  with exactly the row-30 hunk — oracle `consumed=0`, reverted capi
  `consumed=2`, `n_candidates=125`. Logs:
  `~/.local/share/oxpinyin-evidence/2026-09-16/w1/`.)

### The pinyin index DBMs carry uninitialized struct padding

- **Upstream source cite:** `src/storage/chewing_large_table2.h:217-231`
  (`ChewingTableEntry::add_index` inserts a whole `PinyinIndexItem2<L>`
  struct with `m_chunk.insert_content(offset, &add_elem, sizeof(IndexItem))`)
  and `pinyin_phrase3.h:181-195` (`PinyinIndexItem2<L>` = `{u32 m_token,
  ChewingKey m_keys[L]}` — for odd L the struct carries 2 bytes of
  tail padding to the token's 4-byte alignment).
- **Mechanism:** `add_elem` is a stack-allocated `PinyinIndexItem2<L>`
  whose constructor `memmove`s only the keys and never touches the tail
  padding; the whole `sizeof` bytes — padding included — are copied into
  the DBM value. The padding bytes are whatever happened to sit on that
  stack region during `gen_binary_files` — unreproducible garbage
  (observed `17, 236` and `90, 237` in the pin's real files).
- **What oxpinyin does instead:** datagen zeroes the padding
  (`oxpinyin_data::row_format::pinyin_index::encode_item`, written from
  `crates/oxpinyin-data/src/table_entries.rs`), and the runtime reader
  (`pinyin_index::decode_items` via `ChewingTable::search`,
  `crates/oxpinyin-data/src/chewing_table.rs`) reads only the token and
  key fields within each stride, never the padding.
- **Externally observable:** no — the reader never touches the padding
  bytes, so a libpinyin runtime consuming either file decodes identical
  index items; only the file bytes differ, in two bytes per odd-L value
  record. Class (b): upstream's stored bytes *are* uninitialized memory;
  no safe construction reproduces specific garbage, and none should.


## redb write-side emptiness probe creates the table it probes (2026-09-05)

redb `WriteTxn::is_empty` (`RedbWriteTxn` in `crates/oxpinyin-store/src/lib.rs`)
creates the probed table if absent — redb API constraint (4.2.0: `open_table`
creates, `list_tables` is the only lookup), no non-creating write-side
existence probe exists; the store traits cannot tell an empty table from an
absent one, so nothing above them observes it and the only trace is an empty
table in the file.

## Entries registered 2026-09-27 UTC (lane G — round-2 audit reconciliation)

The entries below were registered by the round-2 register
reconciliation (#573; the audit report is
`bug-for-bug-audit-r2-2026-09-23`). Each carries its
`compatibility-policy.md` row number. Pin cites are at `074a2219`,
oxpinyin cites at `main` @ `e1d915d0`; both were re-read for this
section.

### `pinyin_init`/`zhuyin_init` leave the process `LC_NUMERIC` at "C" (policy row 39)

- **Upstream source cite:** `src/storage/table_info.cpp:328` and `:372`
  (`UserTableInfo::load`), `:197` and `:291` (`SystemTableInfo2::load`),
  `:378` and `:394` (`UserTableInfo::save`).
- **Mechanism:** each function saves `setlocale(LC_NUMERIC, "C")`'s
  return value and restores it at the end. `setlocale` returns the name
  of the *new* locale (C11 7.11.1.1p7), so the "restore" re-applies
  `"C"`; and every early return — a failed `fopen` (`:330-333`,
  `:199-202`), a failed version directive (`:339-348`) — skips the
  restore altogether. Either way the process's `LC_NUMERIC` is `"C"`
  after `pinyin_init` or `zhuyin_init`, successful or not.
- **What oxpinyin does instead:** since external-I/O stage 1
  (2026-10-28 UTC) the `setlocale` calls are the pin's own, made inside
  `oxpinyin-input`'s `table.conf`/`user.conf` reader and writer at the
  pin's sites and in the pin's number: before `fopen`, and again after
  `fclose` on the success path only. From #618 (merge commit `a3ef00f5`,
  code `e5400598`, 2026-10-01 UTC) to stage 1 the two C-ABI crates
  installed `"C"` once at each facade entry instead. Before #618 — the
  state this section was written against — there was no `setlocale`
  call anywhere in `crates/`, so the host's `LC_NUMERIC` survived init.
- **Externally observable:** yes — audit D-17 (#539): with
  `LC_ALL=zh_CN.UTF-8` set by the host, `setlocale(LC_NUMERIC, NULL)`
  reads `C` after the pin's init and read `zh_CN.UTF-8` after
  oxpinyin's, before #618. A consumer that formats numbers after init
  saw the difference; `tools/bisection/run-locale-diff.sh` now diffs it
  byte for byte.
- **Status:** ruled bug-for-bug (recorded 2026-09-27 UTC,
  `compatibility-policy.md` "Amendment — rulings recorded" item 4):
  oxpinyin is to reproduce the pin's end state, and the defect is
  drafted for the upstream report (`upstream-report-drafts.md` item 6,
  not filed). **CLOSED** in code: reproduced by #618 (merge commit
  `a3ef00f5`, code `e5400598`, merged 2026-10-01 UTC; recorded
  2026-10-02 UTC); it was a REVERT TARGET until then (#539).

### tkrzw binding: exception-origin `SYSTEM_ERROR` and OS `SYSTEM_ERROR` share one error class (policy row 40)

> **Retired 2026-11-02 UTC** (tkrzw cxx rebinding): the collapse was `tkrzw_langc.h`'s; the backend now calls the concrete classes through cxx and catches no exception, so a C++ exception terminates the process as the pin's does and a status keeps its own class. Row 40 in `compatibility-policy.md` records the retirement; the entry below is kept as written.

- **Registered as:** a standing divergence (ruled accepted 2026-08-28;
  a Rust/C++ language-boundary consequence under the language-quirk
  exception, maintainer ruling 2026-10-03), not a numbered exception
  class — `compatibility-policy.md`, "Registered standing divergences". Registered here 2026-09-27 UTC (#551); the full
  record, with the ruling's process note, is
  `tkrzw-langc-exception-classification`.
- **Upstream source cite:** none in libpinyin: the pin's tkrzw backend
  (`src/storage/*_tkrzwdb.cpp`) calls tkrzw's C++ API. The cite is
  tkrzw's `tkrzw_langc.cc` at 1.0.32 (every wrapper's
  `catch (const std::exception& e)` sets `TKRZW_STATUS_SYSTEM_ERROR`,
  e.g. `:163-165`, as the finding cites it).
- **Mechanism:** a C++ exception inside tkrzw (allocation failure) and
  an operating-system error both surface as `SYSTEM_ERROR` through the
  C API; the retired cxx shim reported the first as `UNKNOWN_ERROR`.
  Rust cannot catch a C++ exception, so the C API is the only way in,
  and it merges the two before oxpinyin sees either.
- **What oxpinyin does instead:** maps `SYSTEM_ERROR` to
  `StoreError::Io` and every other code to `StoreError::Backend`
  (`crates/oxpinyin-store/src/tkrzw/mod.rs:149`, `:214`), so the
  exception origin is indistinguishable from an I/O error.
- **Externally observable:** only to a Rust caller of `oxpinyin-store`
  branching on `StoreError::Io`, and only under memory exhaustion; no
  test or differential surface reaches it, and nothing crosses the
  libpinyin C ABI.

### Literal `0x0` option gating — CLOSED in code (policy row 17)

- **Upstream source cite:** `src/storage/phonetic_key_matrix.cpp:86-89`
  (`resplit_step` returns without `USE_RESPLIT_TABLE`), `:168-171`
  (`inner_split_step` without `USE_DIVIDED_TABLE`); `src/pinyin.cpp:2194`
  (`pinyin_guess_candidates` answers `false` on an empty matrix).
- **Mechanism:** at the literal option word `0x0` the correction
  aliases (`jv`, `zon`) produce no key and the divided/resplit tables
  add no alternatives, so the guess is empty and `xian`'s inventory
  shrinks.
- **What oxpinyin does:** the same gates (`crates/oxpinyin-engine/src/session/mod.rs:609`,
  `:626`), and no raw-text fallback when the parse consumed nothing
  (`8ec75085`).
- **Status:** closed; `run-option-sweep.sh` with the `0x0` and
  divided-contrast cases ran 2026-09-16, 24/24 PASS. Registered here
  2026-09-27 UTC (#548): until then the row's only record was the
  parked paragraph in `all-off-tails`.

### Sort-option input of `pinyin_guess_candidates` — CLOSED in code, reopened 2026-09-27 (#582) and closed 2026-10-03 (policy row 32)

- **Upstream source cite:** `src/pinyin.cpp:2292-2300` (the LONGER and
  sentence prepends gated on `sort_option`, then
  `_remove_duplicated_items_by_phrase_string`); `:1678-1709`
  (`compare_item_with_sort_option`); `:1870-1933`
  (`_prepend_longer_candidates`); `src/pinyin.h:56`
  (`SORT_WITHOUT_SENTENCE_CANDIDATE = 0x1`).
- **Mechanism:** with bit `0x1` set the pin never prepends the n-best
  rows, so its dedup never sees them and every NORMAL row survives.
- **What oxpinyin does instead:** bits `0x2`/`0x4`/`0x8`/`0x10` are
  ported (PR #496, 2026-09-20). Bit `0x1` is applied after an engine
  dedup that already ran with the n-best rows present
  (`crates/oxpinyin-engine/src/session/lookup.rs:754`, then
  `crates/oxpinyin-capi/src/sentence.rs:359`), so a NORMAL row whose
  text equals an n-best string is lost with the sentence row.
- **Externally observable:** yes (#582, all three cells): `li'shi`
  after `guess_sentence` at `0x1f` — pin n=385 headed by 历史, 理事;
  oxpinyin n=383, neither present; `0x1d` the same; `0x1c` identical.
  The 2026-09-20 closure measured `0x1e`/`0x1c`/`0x14` only.
- **Status:** **CLOSED** in code 2026-10-03: #596 (merge commit `45cc6032`) and
  #598 (`efa8337a`); every `0x1`/`0x1f` case of `candidate-assembly-diff` is
  identical on tkrzw, bdb and kc. Reopened 2026-09-27 UTC (#582).

### Whole-row NBEST choose + train writes the user bigram (policy row 33)

- **Upstream source cite:** `src/pinyin.cpp:2515-2520` (a row choose
  installs no `CONSTRAINT_ONESTEP`); `src/lookup/phonetic_lookup.h:866`
  (`train_result3` trains only past a ONESTEP constraint).
- **Mechanism:** after a whole-row n-best choose, `pinyin_train` walks a
  constraint-free result and writes nothing.
- **What oxpinyin does instead:** `Session::train`
  (`crates/oxpinyin-engine/src/session/selection.rs:298-339`) falls back
  to the selection record when no OneStep cell is present and seeds
  `sentence_start → phrase`.
- **Externally observable:** yes — audit D-05 (#527): after the union
  driver's choose and train, oxpinyin writes `测测→你` (count 138) and
  predicts 你; the pin writes and predicts nothing. Row 20's line is
  this mechanism (see row 20).
- **Status:** **CLOSED** in code 2026-10-03: #614 (merge commit `33e2e54d`, code
  `750b8cb6`) and #616 (`fd293e53`); the choose-every-row differential is
  identical on tkrzw, bdb and kc. Work order `revert-plan`.

### Imported user phrase lost after `guess_sentence` (policy row 34)

- **Upstream source cite:** `src/pinyin.cpp:2693-2704` (`pinyin_reset`
  is the only clear of `m_nbest_results`); `:2184-2300`
  (`pinyin_guess_candidates` rebuilds from scratch).
- **Mechanism:** the pin keeps n-best results across a parse and, under
  `0x1`, never lets them into the dedup.
- **What oxpinyin does instead:** clears n-best on every parse and
  dedups NBEST-first before the `0x1` filter (row 32's mechanism).
- **Externally observable:** yes — an imported user phrase is offered at
  `0x1f` after a sentence guess on the pin and not on oxpinyin
  (`probe-coverage-abi.md` C); #582 shows the dedup half without any
  import.
- **Status:** **CLOSED** in code 2026-10-03: #598 (merge commit `efa8337a`) with
  #596 (`45cc6032`); phases A and N at `0x1`/`0x1f` are identical on
  tkrzw, bdb and kc.

### User-library tokens refused an n-best step cost — CLOSED in code (policy row 35)

- **Upstream source cite:** `src/lookup/phonetic_lookup.h:643-668`
  (`unigram_gen_next_step` prices any loaded sub-index's item);
  `src/pinyin.cpp:597-605` (an import writes the item with
  `count × unigram_factor`).
- **Mechanism:** a user-dictionary token is a priced trellis step.
- **What oxpinyin does:** since `7c9a6923` (2026-09-20),
  `nbest_step_costs_with_user_delta` prices a visible `USER_FILE` token
  with no system unigram from its user delta
  (`crates/oxpinyin-data/src/lm/mod.rs:570-576`); a masked library's
  token and a missing item keep the default, as the pin's failing
  `get_phrase_item` does.
- **Status:** closed; `7c9a6923`'s same-dir measurement on the pin's
  `data/` (tkrzw): `residue-a-tail-diff` phases A and X byte-identical.
  #548 found the policy still counting it open; corrected 2026-09-27
  UTC.

### Bigram export iterator: `get_next_phrase`'s return value on the last row (policy row 36)

- **Upstream source cite:** `src/pinyin.cpp:894-911`
  (`pinyin_bigram_iterator_get_next_phrase` returns
  `pinyin_bigram_iterator_has_next_phrase(iter)`).
- **Mechanism:** the pin answers `false` on the last row.
- **What oxpinyin did instead (before lane B):** answered `true`
  whenever a row was fetched (`crates/oxpinyin-capi/src/iterators.rs:406-407`
  at `e1d915d0`); #607 returns `has_next` after the increment.
- **Externally observable:** yes — audit D-19 (#541):
  `你好|ni'hao|138|false` on the pin, `…|true` on oxpinyin. A debug
  ibus-libpinyin build wraps the call in `check_result`
  (`PYLibPinyin.cc:321`) and aborts on the pin's last row.
- **Status:** **CLOSED** in code (recorded 2026-10-02 UTC at `a3ef00f5`):
  lane B's #607 (merge commit `6c9bc75d`, code `3415b232`) answers
  `has_next` after the increment, and #608 (merge commit `569420c1`, code
  `d0d849af`) exports from the pin's own in-memory user-bigram container —
  its walk order, its skipped last key and its `sentence_start`
  attribution (#541). Both merged 2026-10-01 UTC; measured by those PRs'
  `bigram-export-diff` differential.

### Candidate window behind the composition offset after a choose (policy row 37)

- **Upstream source cite:** `src/pinyin.cpp:2184-2262`
  (`pinyin_guess_candidates` rebuilds from `offset` over the whole
  matrix); `:2501-2590` (a choose writes a constraint and answers a
  cursor; the instance keeps no composition offset).
- **What oxpinyin does instead:** advances a composition offset on every
  choose (`crates/oxpinyin-engine/src/session/selection.rs`) and serves
  its cached list for any lookup offset at or behind it
  (`crates/oxpinyin-capi/src/sentence.rs`, the re-anchor test).
- **Externally observable:** yes — `probe-coverage-abi.md` E: at
  `guess_candidates(0, 0x1f)` after a whole-composition choose the pin
  answers 127 candidates and oxpinyin 0; ibus-libpinyin's preset 2
  takes that path after every partial choose.
- **Status:** **CLOSED** in code 2026-10-03: #604 (merge commit `06ede4dc`), #609
  (`563ed9b1`) and, for the transformed input schemes, #625 (`c6a61be8`);
  phases C, M, S, T and K are identical on tkrzw, bdb and kc.

### `pinyin_train`/`zhuyin_train` gate (policy row 38)

Updated 2026-10-04 UTC for #524. Valid-index and both empty-result defects
closed in code; the bounds assert is reproduced as an abort.

The pin tree read and built is `074a2219c90feaf962d0d24f034514033ece5f99`.
`src/pinyin.cpp:2678-2688` refuses empty decoded results, asserts the index
bound, marks the context modified and trains the requested result.
`src/zhuyin.cpp:1704-1713` has the same empty gate and trains row zero.
Selection history alone is insufficient on both facades; the previous
claim that these refuse arms were closed was wrong.

The fix adds `Session::train_nbest(index, user)` using each existing row's
spans (`crates/oxpinyin-engine/src/session/selection.rs:361-376`), retaining
the pin's OneStep/train-next observation gate in `train_spans` (:379-424).
`InstanceCore::train(index)` returns `Result<bool, EngineError>` and leaves
empty and bounds calls unmodified (`crates/oxpinyin-facade/src/instance.rs:245-259`).
The pinyin C adapter aborts on a bounds miss, as the pin's assert does
(`crates/oxpinyin-capi/src/candidates.rs`, `train_answer`).
Empty results return false quietly, including zhuyin
(`crates/oxpinyin-zhuyin-capi/src/candidates.rs:272`).

Measured on linux/amd64, debian:testing digest
`sha256:16faa8d1cd99fcb2d30eebe90454e26b20499055fa7094e9b55d32d1a7666f08`:
`python3 tools/bisection/train-index-diff.py bdb PREFIX PINYIN_SO ZHUYIN_SO`
runs 10 cases; `kc` and `tkrzw` run the same 10. Identical pre-training pinyin rows are 今天/今田/今添;
zhuyin row zero is 你好. Valid calls match returns, native exported records
and complete saved `.dbin` bytes. Pinyin rows 0/1/2 train 今 followed by
天/田/添 respectively, each credited bigram +69, each unigram +483,
each pronunciation +69. Bounds len=3 and 255 are the pin's SIGABRT; the
pinyin adapter aborts at the same inputs, before any training write, and
prints no warning the pin does not. Both facades' fresh and
parse/choose-without-guess cases are quiet false.

Bdb and tkrzw raw user index-container bytes differ after valid saves while exported
records match; no register row covers that container-byte difference.
Row 18 covers padding in index values, a different issue. No container-byte
fix is included. Kc raw index-container bytes also match. Captures are
not committed; temporary profiles are discarded by the driver.

### An empty-string user dir is the working directory on the pin (policy row 46)

Registered 2026-10-04 UTC with its fix (#619). Pin cites are at
`074a2219`; the measurements were taken on freshly built bdb, kc and
tkrzw oracles (linux/amd64) against `main` @ `8cd06566`.

- **Upstream source cite:** `src/pinyin.cpp:332` and `src/zhuyin.cpp:276`
  (`m_user_dir = g_strdup(userdir)`); `pinyin.cpp:1133` and `:2671`,
  `zhuyin.cpp:548` and `:1697` (the only guards, both on the pointer);
  `pinyin.cpp:176-177`, `:220-232`, `:922-1130` and `zhuyin.cpp:130-131`,
  `:164-176`, `:547-693` (every user file's path is
  `g_build_filename(m_user_dir, <name>, NULL)`, built when the file is
  touched).
- **Mechanism:** `g_build_filename` drops an empty element and stops at
  a NULL one. With `userdir ""` every path is a bare file name, resolved
  against the directory that is current at that file operation — not the
  one current at init — and `m_user_dir` is non-NULL, so the context
  trains and a dirty save writes the profile. With a NULL `userdir` the
  same calls build the empty string: train and save answer `false` and
  nothing is written. The behaviour is defined, has no float in it and
  aborts nothing, so no exception class fits.
- **What oxpinyin does instead:** since #619's fix it reproduces it.
  `pinyin_init`/`zhuyin_init` keep NULL and `""` apart
  (`crates/oxpinyin-capi/src/context.rs`,
  `crates/oxpinyin-zhuyin-capi/src/context.rs`), `ContextCore::try_open`
  carries the difference as `Option<&str>`, and `Runtime::open_with_law`
  opens the store on the empty path and keeps it relative, so each file
  resolves against the directory current when it is opened. Before that
  — the state #619 measured — the runtime dropped an empty path
  (`crates/oxpinyin-runtime/src/lib.rs:1207` at `8cd06566`) after the
  two inits had turned NULL into `""`, so both arguments meant "no user
  dir".
- **Externally observable:** yes — #619. In an empty working directory
  the pin's `pinyin_init(data, "")` writes `user.conf` there, a train
  answers `true`, and the save answers `true`, writes the 11-file
  profile into the working directory and leaves `LC_NUMERIC` at `"C"`
  (row 39's site); a second process reads the training back (the trained
  word's unigram frequency 854 → 1337). `zhuyin_init(data, "")` is the
  same from the train on. Before the fix oxpinyin answered `false` to
  the train and the save, wrote nothing and left `LC_NUMERIC` alone at
  the save. With a chdir after init the pin's save and fini write into
  the new directory and the first keeps the raised open counter.
  `tools/bisection/run-locale-diff.sh` — row 39's differential,
  extended — diffs the two libraries on five forms of the argument — an
  absolute directory, `""`, `""` with a chdir after init, NULL and `"."`
  — two consecutive processes each, byte for byte on every step's
  return value, `LC_NUMERIC`, each directory's inventory and
  `user.conf`, and the trained word's unigram frequency; stderr is
  captured and not compared (#545).
- **Status:** **CLOSED** in code (#619); it was never a registered
  divergence, only an unregistered defect. Two neighbours are separate
  issues and are not reproduced here: a NULL user dir still has
  in-memory user tables on the pin, so an import succeeds there (#642),
  and an empty-string *system* dir is the working directory too (#643).

### NULL pointer arguments: the pin dereferences, oxpinyin null-guards (policy row 41)

- **Upstream source cite:** 68 `pinyin_*` exports, each dereferencing a
  NULL argument before any check at `074a2219`; the per-export first
  dereference is tabulated in #526's body, by pattern: the instance
  first (`src/pinyin.cpp:1312`); the context or iterator first (`:509`,
  `:665`, `:777`, `:1196`); an out-parameter written before any check
  (`:2847`, `:2876`, `:2982`); a NULL candidate inside a live `assert`
  (`:2507`, `:2593`); a NULL key or key-rest (`:2711`, `:2722`,
  `:2733`, `:2744`).
- **Mechanism:** no export checks its pointer arguments; a NULL one is
  dereferenced — undefined behaviour, SIGSEGV in practice.
- **What oxpinyin does instead:** every export (or the helper it
  delegates to) opens with an `is_null()` guard and answers
  `false`/`0`/NULL/void; #526 lists the guard line per export (e.g.
  `crates/oxpinyin-capi/src/instance.rs:19`, `iterators.rs:64`,
  `candidates.rs:309`, `cursor.rs:241`, `keys.rs:219`).
- **Externally observable:** yes — audit D-04: of 83 NULL-class probes,
  70 crash the pin (68 distinct exports, all `signal=11`) and all 83
  return on oxpinyin; the libzhuyin exports show the same shape (#526,
  comment of 2026-09-25). Class (b).

### A guess on an instance whose context was finalised (policy row 42)

- **Upstream source cite:** `src/pinyin.cpp:1194-1222` (`pinyin_fini`
  deletes the context and its members, not its instances);
  `:1372-1380` (`pinyin_guess_sentence` reads
  `instance->m_context->m_pinyin_lookup`).
- **Mechanism:** a use after free — the instance keeps a pointer to the
  deleted context.
- **What oxpinyin does instead:** each instance holds its own handles to
  the shared engine state (`crates/oxpinyin-capi/src/state.rs:69-77`),
  so the guess completes and answers `true`.
- **Externally observable:** yes — audit D-06, probe
  `instance_outlives_context`: SIGSEGV on the pin, `true` on oxpinyin.
  Class (b). The inverse probe (`alloc_after_fini`) is the next entry
  (policy row 60). The orphan guess itself needs no change (ruled
  2026-10-04): the pin's use after free stays unreproducible.

### Zhuyin import into library index 16 (policy row 43)

- **Upstream source cite:** `src/zhuyin.cpp:392-398`
  (`zhuyin_begin_add_phrases` stores any index), `:475` (`_add_phrase`
  calls `phrase_index->get_range(index, …)`),
  `src/storage/phrase_index.cpp:611` (`m_sub_phrase_indices[phrase_index]`
  with no bound), `src/storage/phrase_index.h:441` (the array has
  `PHRASE_INDEX_LIBRARY_COUNT` = 16 entries).
- **Mechanism:** index 16 reads one element past the array and calls
  through the value found there — an out-of-bounds read.
- **What oxpinyin does instead:** `zhuyin_begin_add_phrases` stores any index
  as the pin does (`crates/oxpinyin-zhuyin-capi/src/iterators.rs:33`); the
  following `zhuyin_iterator_add_phrase` takes the `USER_FILE` libraries and
  the loaded system libraries 1..=4, and refuses every other index — library
  16 included — with `false`
  (`crates/oxpinyin-zhuyin-capi/src/iterators.rs:132`).
- **Externally observable:** yes — the audit's libzhuyin battery
  (#526, comment of 2026-09-25): SIGSEGV on the pin on all three cells,
  `false` on oxpinyin. Class (b). The pinyin facade's `_add_phrase`
  carries the same unchecked index (`src/pinyin.cpp:589`), but the
  audit recorded library 255 *accepted* there (D-12), so that side is
  not a stable crash and is not part of this entry.
- **Held by:** `oxpinyin-zhuyin-capi`'s
  `boundary_tests::null_arguments_answer_the_declared_value`. The pin
  observation `zhuyin-add-phrase-library-16` of `contract-diff.py`
  (added 2026-10-11 UTC, #563): on bdb the pin exits -11 (SIGSEGV) at
  library 16 after writing the same stderr bytes the subject writes, and
  the subject answers `false` with no warning. The case builds no fixture,
  so a sweep runs it in kc and tkrzw as well; measured on bdb here (this
  lane is bdb-only; #562 and #564 cover the other cells).

### `pinyin_token_get_nth_pronunciation` past the last reading (policy row 51)

- **Upstream source cite:** `src/pinyin.cpp:2801-2819`
  (`ChewingKey buffer[MAX_PHRASE_LENGTH]` at `:2808`, never initialised;
  the return value of `item.get_nth_pronunciation(nth, buffer, freq)` at
  `:2815` ignored; `g_array_append_vals(keys, buffer, len)` at `:2817`);
  `src/storage/phrase_index.cpp:33-44` (`get_nth_pronunciation` refuses an
  out-of-range read through `MemoryChunk::get_content` and writes nothing).
- **Mechanism:** an `nth` past the last reading leaves `buffer` as it was,
  and the function appends `phrase_length` keys of it and answers `true`:
  an uninitialised read. What the stack held depends on earlier calls
  (bdb, 2026-10-04: `0000 0000` for `nth` 1 and 2, `604f0000` for
  `nth` 2^32−1).
- **What oxpinyin does instead:** answers `true` and appends
  `phrase_length` zeroed keys (`crates/oxpinyin-capi/src/dict.rs`,
  `pinyin_token_get_nth_pronunciation`). The return value and the number of
  keys are the pin's.
- **Externally observable:** the content only. Class (b).

### `zhuyin_token_get_nth_pronunciation` past the last reading (policy row 70)

- **Upstream source cite:** `src/zhuyin.cpp:1793-1811`
  (`ChewingKey buffer[MAX_PHRASE_LENGTH]` at `:1800`, never initialised;
  the return value of `item.get_nth_pronunciation(nth, buffer, freq)` at
  `:1807` ignored; `g_array_append_vals(keys, buffer, len)` at `:1809`);
  `src/storage/phrase_index.cpp:33-44`, the same refusing read as the
  pinyin entry above.
- **Mechanism:** as the pinyin entry: an `nth` past the last reading leaves
  `buffer` as it was, and the function appends `phrase_length` keys of it
  and answers `true`. bdb, 2026-10-04: `nth` 1, 2 and 2^32−1 on `你好`
  answer `true` with two keys.
- **What oxpinyin does instead:** answers `true` and appends
  `phrase_length` zeroed keys (`crates/oxpinyin-zhuyin-capi/src/dict.rs`).
- **Externally observable:** the content only. Class (b).

### A save whose write fails for one file (policy rows 68 and 69)

- **Upstream source cite:** `src/pinyin.cpp:940-1147` (`_write_files`
  ignores each file's write result; `_rename_files` renames every file of
  the set and prints `rename %s to %s failed.` for the ones that fail;
  `pinyin_save` answers `_write_files(...) && _rename_files(...)`, `true`),
  `src/storage/table_info.cpp:377-397` (`write %s failed.` for the marker),
  `src/zhuyin.cpp` for the twin.
- **Mechanism:** the files are written and renamed one by one. Blocking
  `user_pinyin_index.bin.tmp` leaves the first `user_pinyin_index.bin` at
  its active final path while the other files take the second save. In the
  final-blocked fixture, the harness first moves `user_phrase_index.bin`
  to `user_phrase_index.bin.moved`; that backup keeps the first save, the
  active final path is a blocking directory, and the other files take the
  second save. Both print one failure line and answer `true`: distinct
  mixed profiles.
- **What oxpinyin does:** the same, since lane C PR 15
  (`persistence::save_with_bigram_reporting`). It had removed what it
  staged and reported all ten renames, leaving the previous profile whole;
  that was a REVERT TARGET (row 69) for a day and is reverted.
  `UserStore::save()` keeps its all-or-nothing commit; only the reporting
  save, which the C `save` entry points use, follows the pin.
- **Externally observable:** yes — the stderr lines, the files left in the
  user dir. Closed: `stderr-save-one-tmp-blocked`,
  `stderr-save-one-final-blocked`, `stderr-save-one-tmp-blocked-zhuyin`
  MATCH the pin.

### `pinyin_alloc_instance` on a finalised context (policy row 60)

- **Upstream source cite:** `src/pinyin.cpp:1194-1222` (`pinyin_fini`
  deletes the context), `:1310-1333` (`pinyin_alloc_instance` reads
  `context->m_phrase_index` at `:1322` to build the instance's
  constraints, `phonetic_lookup.h:427`).
- **Mechanism:** a use after free. valgrind (2026-10-04, bdb): "Invalid
  read of size 8 at `pinyin_alloc_instance (pinyin.cpp:1322)`, address 48
  bytes inside a block of size 1,464 free'd". The call returns a non-NULL
  instance whose constraints hold a dangling pointer, and survives because
  the freed block is still mapped.
- **What oxpinyin does instead:** `pinyin_init` records each context's
  address and `pinyin_fini` forgets it (`crates/oxpinyin-capi/src/live.rs`);
  `pinyin_alloc_instance` answers NULL for an address that is not live, and
  logs nothing, because the pin does not abort. Before this entry the call
  dereferenced the freed Rust context and crashed (valgrind: invalid read in
  `ContextCore::alloc_instance`; SIGSEGV).
- **Externally observable:** yes — `alloc-instance-after-fini` in
  `contract-diff.py`: the pin returns an instance and exits 0, the parent
  build exits with SIGSEGV, this change returns NULL and exits 0. Class (b).
  A new context at a reused address is live again, which the registry cannot
  tell from the old one.
- **The zhuyin twin (lane C, PR 14):** `zhuyin_alloc_instance` reads the
  freed context the same way (`zhuyin.cpp:845-857`). `zhuyin_init` and
  `zhuyin_fini` keep their own registry (`crates/oxpinyin-zhuyin-capi/src/live.rs`)
  and the call answers NULL, silently (the pin observation
  `zhuyin-alloc-instance-after-fini`); `oxpinyin-zhuyin-capi`'s
  `live::tests::a_context_is_live_from_init_to_fini` holds the answer.


### Empty system directory (#643; row 71, closed 2026-10-08 UTC)

This was a port defect, not an exception: at pin 074a2219,
`pinyin.cpp:331-338` / `zhuyin.cpp:275-282` resolve an empty system
directory in cwd; NULL instead fails on the empty filename. Both C
facades now preserve that distinction and the NULL failure's exact raw
stderr. `contract-diff.py`'s ten `init-system-*` cases cover both facades,
including parse/guess after a successful open and dot/absolute controls.
The pin tree read was `/home/sheng/work/libpinyin` at 074a2219.

### Remembered pronunciation tones (#699; row 72, closed 2026-10-08 UTC)

Ordinary userdir, library 7, import 你好你好 / ni3'hao3'ni3'hao3 / 100000,
parse ni3hao3ni3hao3, guess, remember count 3: the pin exports the toned
reading at 100003; the pre-fix subject exported toned 100000 plus toneless 3.
`src/pinyin.cpp:3578-3668` at 074a2219 carries complete matrix keys;
`capi/src/user_data.rs` dropped tone while projecting selected syllables.
The fix retains parsed tones without changing the selected syllable sequence.
No public interface, ABI or dependency changes. Both Zhuyin libraries lack
remember/export symbols. The contract driver cases remember-tones-pinyin
and remember-api-absent-zhuyin showed this distinction on BDB.
`oxpinyin-capi`'s `boundary_tests::remember_keeps_the_parse_time_tones` holds
the pinyin side.
Source read from /home/sheng/work/libpinyin at the pin, pinyin.cpp blob
f27f7cf776724ead8d55e14b4af390e790054a91. This is plain parity.

### NULL user sessions (#642; row 73, closed 2026-10-08 UTC)

At 074a2219, pinyin.cpp:326-444 / zhuyin.cpp:269-358 allocate user
indexes even with NULL userdir. Imports (pinyin.cpp:514-654,
zhuyin.cpp:392-545) modify those indexes. Train (2670-2691 / 1696-1715)
and save (1132-1147 / 547-552) first refuse NULL, without writes.
Pinyin fini attempts an empty marker filename (1194-1200); Zhuyin
fini (741-757) does not write. Source: clean /home/sheng/work/libpinyin
at the pin. Runtime previously passed no UserStore, so imports refused.

The transient store preserves the session without a filesystem target.
Import 你好你好 at toned count 100000 now succeeds and exposes library-7
token 117440513 on both facades; Pinyin candidate zero is the user phrase.
Train/save remain false. Cwd and TMPDIR snapshots remain unchanged after
init, import, lookup, train, save and fini. Exact failed-empty-filename
stderr matches, including Pinyin fini's diagnostic. Ordinary and NULL
fresh-context imports to each library 1–7 are separate contract cases.
The ordinary imports already matched at main 3b016f48; the historical
#525 refusal claim for 1–4 is stale. Zhuyin has no phrase-export API.
Approved additive internal Rust APIs are documented in
../api/transient-user-store.md; C ABI and dependencies are unchanged.

### `pinyin_bigram_iterator_get_next_phrase` after a walk that ends on a real predecessor (policy row 75)

- **Upstream source cite:** `src/pinyin.cpp:896-911` (`get_next`: the
  assert at `:902`, then `g_strdup(iter->m_phrase)` at `:904` and
  `g_strdup((gchar *) g_ptr_array_index(iter->m_pinyins,
  iter->m_pinyin_index))` at `:905`, then `return
  pinyin_bigram_iterator_has_next_phrase(iter)` at `:910`); `:789-894`
  (`has_next` frees `m_pinyins` and replaces it with an empty array at
  `:798-801` before it looks for the next row, and leaves `m_index_token` at
  the predecessor it loaded last, `:885-886`); `:779` and
  `src/storage/ngram_bdb.cpp:178-200` (the predecessors come in the user
  bigram's hash order, `DB_HASH`).
- **Mechanism:** a walk's `false` row leaves `m_pinyins` empty. If the
  predecessor loaded last is `null_token` or `sentence_start`, a further
  `get_next` asserts (policy row 62). If it is a real phrase token the assert
  passes and `:905` indexes the empty array: on GLib 2.90 `g_ptr_array_new`
  allocates no storage, so the read goes through a NULL pointer and the pin
  dies of SIGSEGV. Undefined behaviour.
- **What oxpinyin does instead:** answers `false` with `phrase`, `pinyin`
  and `count` untouched and logs nothing
  (`crates/oxpinyin-capi/src/iterators.rs:641`; the walk names the state
  `BigramStep::Undefined`, `crates/oxpinyin-facade/src/export_rows.rs:1036-1038`).
- **Externally observable:** yes: the pin's process dies, and oxpinyin
  answers `false`. Class (b). Not a `contract-diff.py` case, because which
  predecessor a walk loads last depends on the backend's hash order.
- **Measured** on bdb, linux/amd64 under Rosetta (native amd64 not run),
  2026-10-08 UTC, from the repository root at `3b016f48`, with `TMPDIR` on the
  container's own filesystem:

  ```
  gcc -shared -fPIC -o segv-report.so segv-report.c
  LD_PRELOAD=$PWD/segv-report.so python3 bigram-905.py \
      <prefix>/lib/libpinyin.so <prefix>/lib/libpinyin/data
  addr2line -f -i -C -e <prefix>/lib/libpinyin.so.15.0.0 0x97748
  python3 bigram-905.py <target>/debug/libpinyin_capi.so <prefix>/lib/libpinyin/data
  ```

  The pin prints `has_next True` and `get_next False 你好`, then
  `SIGSEGV addr=(nil) pc=<prefix>/lib/libpinyin.so+0x97748` and exits 139;
  `addr2line` names `pinyin_bigram_iterator_get_next_phrase` at
  `pinyin.cpp:905`. The pin built with the `bigram-export-strjoinv` patch,
  whose walk cannot over-read (row 1), prints the same two lines and faults
  at `+0x97768`, its `pinyin.cpp:907`: the same statement, two patch lines
  down. oxpinyin prints the same two lines, then
  `get_next again False [True, True, True] []`: `false`, the three
  out-params untouched, no log record. The raw capture is not retained.

  `bigram-905.py`:

  ```python
  # Run from the repository root: python3 bigram-905.py <libpinyin.so> <data-dir>
  import ctypes as C, runpy, sys, tempfile
  h = runpy.run_path('tools/bisection/contract-diff.py')
  P, B, I, S, Z = h['P'], h['B'], h['I'], h['S'], h['Z']
  k = h['Kit']('pinyin', sys.argv[1], sys.argv[2], tempfile.mkdtemp())
  for text, first, second in ((b'nihao', '你', '好'), (b'shijie', '世', '界')):
      inst = k.alloc()
      k.fn('parse_more_full_pinyins', Z, P, S)(inst, text)
      k.fn('guess_sentence', B, P)(inst)
      h['choose_text'](k, inst, h['choose_text'](k, inst, 0, first), second)
      k.fn('guess_sentence', B, P)(inst)
      k.fn('train', B, P, C.c_ubyte)(inst, 0)
  it = k.fn('begin_get_bigram_phrases', P, P)(k.ctx)
  get_next = k.fn('bigram_iterator_get_next_phrase', B, P, C.POINTER(P), C.POINTER(P), C.POINTER(I))
  out = [P(h['UNTOUCHED']), P(h['UNTOUCHED']), I(h['UNTOUCHED'])]
  print('has_next', k.fn('bigram_iterator_has_next_phrase', B, P)(it), flush=True)
  print('get_next', get_next(it, *map(C.byref, out)), k.text(out[0].value), flush=True)
  out = [P(h['UNTOUCHED']), P(h['UNTOUCHED']), I(h['UNTOUCHED'])]
  print('get_next again', get_next(it, *map(C.byref, out)),
        [o.value == h['UNTOUCHED'] for o in out], k.logs, flush=True)
  ```

  `segv-report.c` (gdb cannot ptrace under Rosetta):

  ```c
  /* LD_PRELOAD: on SIGSEGV print the fault address and the faulting PC as a
   * module offset for addr2line (gdb cannot ptrace under Rosetta). */
  #define _GNU_SOURCE
  #include <dlfcn.h>
  #include <signal.h>
  #include <stdio.h>
  #include <string.h>
  #include <ucontext.h>

  static void report(int sig, siginfo_t *si, void *ctx) {
      void *pc = (void *)((ucontext_t *)ctx)->uc_mcontext.gregs[REG_RIP];
      Dl_info d;
      memset(&d, 0, sizeof d);
      dladdr(pc, &d);
      fprintf(stderr, "SIGSEGV addr=%p pc=%s+0x%lx\n", si->si_addr,
              d.dli_fname ? d.dli_fname : "?",
              (unsigned long)((char *)pc - (char *)d.dli_fbase));
      signal(sig, SIG_DFL);
      raise(sig);
  }

  __attribute__((constructor)) static void install(void) {
      struct sigaction sa;
      memset(&sa, 0, sizeof sa);
      sa.sa_sigaction = report;
      sa.sa_flags = SA_SIGINFO;
      sigaction(SIGSEGV, &sa, NULL);
  }
  ```

### `mmap %s failed!`: a system library the pin cannot map, and the empty chunk it dies on (#545; policy row 76)

- **Upstream source cite:** `src/pinyin.cpp:256`, `:290`, `:956`, `:1265` and
  `src/zhuyin.cpp:200`, `:234`, `:589`, `:800` (`if (!chunk->mmap(chunkfilename))
  fprintf(stderr, "mmap %s failed!\n", chunkfilename);`, the `open %s failed!`
  branches beside them being the `#else` of `LIBPINYIN_USE_MMAP`, defined by
  `src/include/memory_chunk.h:31-34` whenever `HAVE_MMAP` is, and
  `config.h:42` of the pin's build defines it); `src/include/memory_chunk.h:470-520`
  (`MemoryChunk::mmap` resets the chunk, then answers `false` for a file that
  will not open, one shorter than the 8-byte header, a length word that is not
  the payload length, or a checksum that does not verify);
  `src/storage/phrase_index.cpp:244` and `:278` (`FacadePhraseIndex::load` and
  `diff` hand the chunk to `SubPhraseIndex::load`, `:335-363`, whose
  `g_return_val_if_fail(*(buf_begin + offset) == c_separate, FALSE)` at `:354`
  reads `buf_begin + 16`). Also `src/storage/facade_chewing_table.h:115` and
  `src/storage/facade_phrase_table2.h:96`, which no public call reaches (below).
- **Mechanism:** every site prints the line and carries on with the chunk
  `mmap` reset, which is empty: `begin()` is NULL and `size()` is 0, so
  `SubPhraseIndex::load` reads the separator byte at `NULL + 16` before its own
  bounds check can refuse the chunk. The pin dies of SIGSEGV. The sites are
  reached by `pinyin_init` / `zhuyin_init` (each default `SYSTEM_FILE`
  library, in index order), `pinyin_load_phrase_library` /
  `zhuyin_load_phrase_library` (a library that was unloaded),
  `pinyin_load_addon_phrase_library` (`:290`, the only entry to a `DICTIONARY`
  library), a modified `pinyin_save` / `zhuyin_save` (every loaded system
  library is mapped again to `diff` it) and `pinyin_mask_out` /
  `zhuyin_mask_out` (every loaded system library is reloaded). A library
  already loaded, an unloaded one in a save or mask-out, an unmodified save
  and a reload of a whole file stay silent. `zhuyin.cpp:234` is reached by no
  call: `zhuyin_init` and `zhuyin_load_phrase_library` `assert` that the default
  table is not a `DICTIONARY` (`:330`, `:372`; the pin is built without
  `NDEBUG`) and the zhuyin facade has no addon entry point.
  `FacadeChewingTable` and `FacadePhraseTable2` are included by
  `src/pinyin_internal.cpp` into the uninstalled `libpinyin_internal.a` and
  instantiated nowhere in `src/`, `tests/` or `utils/`; `libpinyin.so.15` and
  `libzhuyin.so.15` define no symbol of either class.
- **What oxpinyin does instead:** writes the pin's `mmap <path> failed!` line,
  the path as its bytes, at each reachable site on both facades, with the
  pin's own `fprintf(stderr, …)` through `oxpinyin_data::c_stderr` (stage 4
  of `external-io-boundary.md`), and answers what it answered before: NULL with
  one GLib warning from an init whose library is short or corrupt, non-NULL
  from one whose library is absent (the library stays unloaded), `true` from a
  reload, a modified save and a mask-out, `false` from the addon load. It
  never reads through the empty chunk. The failure is the typed
  `LibraryError::Unmappable` the chunk reader raises for exactly what
  `MemoryChunk::mmap` refuses; a separator or index outside the payload
  is `LibraryError::Format` and has no line; a payload shorter than 17
  bytes is `LibraryError::Layout`, and the caller loads the empty
  sub-index and logs one critical, as `SubPhraseIndex::load`'s
  `g_return_val_if_fail` does. With several libraries broken it writes one
  line for each, in library order; the pin's bytes are the first. An init that
  fails on a library judges the user profile first, as the pin does, so the pin's
  `open <user>/user.conf failed.`, wipe and marker write precede its line and its NULL.
- **Externally observable:** yes: the pin's process dies, with the line as its
  last output; oxpinyin writes the line and goes on. Class (b).
- **Measured** on bdb, native amd64, 2026-10-09 UTC, in a Debian testing
  container over `1be4e35d` with this change: the `stderr-library-*` cases
  of `tools/bisection/contract-diff.py` break `merged.bin`, `gbk_char.bin` or
  `art.bin` in a private copy of the system directory (absent, cut to four
  bytes, last payload byte flipped) and compare stderr up to the crash. 39
  crash cases (21 pinyin, 18 zhuyin, fresh and non-conforming user dirs among them) match:
  the pin dies of SIGSEGV after the same bytes, and oxpinyin's answers are
  the ones listed above; the 11 controls match. Against the parent build the
  39 differ and the 11 match. Fault address, with a preload that prints
  `si_addr` and re-raises:

  ```
  gcc -shared -fPIC -o segv-addr.so segv-addr.c
  LD_PRELOAD=$PWD/segv-addr.so tools/bisection/run-contract-diff.sh bdb <prefix> \
      <target>/debug/libpinyin_capi.so <target>/debug/libzhuyin_capi.so \
      -- --cases stderr-library-save-missing --observations obs.jsonl
  ```

  The pin's stderr in `obs.jsonl` ends `SEGV si_addr=0x10` for `-init-`,
  `-load-`, `-save-`, `-mask-out-` and `-addon-` alike, pinyin and zhuyin.

  `segv-addr.c`:

  ```c
  #define _GNU_SOURCE
  #include <signal.h>
  #include <stdio.h>
  #include <string.h>
  #include <unistd.h>

  static void on_segv(int sig, siginfo_t *info, void *ctx) {
      char line[64];
      int n = snprintf(line, sizeof line, "SEGV si_addr=%p\n", info->si_addr);
      (void)!write(2, line, n);
      signal(sig, SIG_DFL);
      raise(sig);
  }

  __attribute__((constructor)) static void install(void) {
      struct sigaction sa;
      memset(&sa, 0, sizeof sa);
      sa.sa_sigaction = on_segv;
      sa.sa_flags = SA_SIGINFO | SA_NODEFER;
      sigaction(SIGSEGV, &sa, NULL);
  }
  ```

**Extended 2026-10-18 UTC (#865):** the same `SubPhraseIndex::load`
separator/index reads at `phrase_index.cpp:354-356` also run past the chunk
for an in-bounds header whose separator/index lies outside the payload, or for
a payload shorter than the four-word header. The pin reads the separator/index
outside the mapped chunk; on the first shape it dies of SIGSEGV before any
line. oxpinyin's `PhraseLibrary::open` refuses a separator or index outside the
payload as `LibraryError::Format` without the read and without a critical,
and a payload shorter than 17 bytes as `LibraryError::Layout`; the caller
loads the empty sub-index and logs one critical. Both are class (b), the same
site as row 76. Held by `crash-oob-layout-{pinyin,zhuyin}` (pin SIGSEGV,
subject `init` false; no critical) and `short-chunk-layout-{pinyin,zhuyin}`
(the pin's empty sub-index + one critical matched by the subject's Layout
empty sub-index + one critical; the out-of-object read itself is not
performed) of `contract-diff.py`, all MATCH on bdb; the OOB cases are controls
that also match the parent build.

### A sparse chunk's whole mapping is retained after `fini` (#865; policy row 109)

**2026-10-28 UTC:** the mapping call is a revert target under the
external-I/O ruling. The post-fini residual is a separate open question;
the historical observation below does not decide it. See
[external-io-boundary.md](external-io-boundary.md).

- **Upstream source cite:** `src/include/memory_chunk.h:470-520`
  (`MemoryChunk::mmap` maps the whole file) and `:72-78`
  (`MemoryChunk::freemem` unmaps only `header + capacity()`).
- **Mechanism:** a 2^32+8 file whose length word is 0 makes `mmap` read a
  `guint32` payload length of 0; the mapping is the whole 4 GiB file, while
  `freemem` releases only the 8-byte header extent. The remaining ~4 GiB stays
  mapped after `pinyin_fini`/`zhuyin_fini`; measured on bdb in the #865 report
  at 4,242,620 KiB VmSize after init and 4,228,488 KiB after fini, against
  51,412/37,284 KiB for the subject process.
- **What oxpinyin does instead:** `map_chunk`
  (`crates/oxpinyin-data/src/phrase_library.rs`) maps only
  `header + data_len` — 8 bytes for this chunk — and `MappedFile::drop`
  releases that whole mapping. The ~4 GiB virtual-address retention is a pin
  resource defect, not an API answer, and is not reproduced.
- **Status:** **REVERT TARGET** for the mapping call (external-I/O
  ruling, 2026-10-28 UTC); **OPEN QUESTION** for the post-fini residual
  mapping. The historical reading — pin resource defect, not reproduced; no
  compatibility exception; the safe construction releases the map and is not
  class (b) UB — does not decide either.
- **Held by:** `a_sparse_chunk_maps_only_its_chunk_extent`
  (`crates/oxpinyin-data/src/phrase_library.rs`, the mapping is the chunk
  extent) and the #865 sparse measurement above.

### `table.conf` follows its rows; malformed ones fail the triggering call (#694 and #525 batch B; policy rows 78, 79, 80 and 83)

Registered 2026-10-10 UTC. Pin source read at `074a2219c90feaf962d0d24f034514033ece5f99`
(`src/storage/table_info.cpp`, `src/pinyin.cpp`, `src/zhuyin.cpp`,
`src/include/memory_chunk.h`, `src/lookup/phonetic_lookup.h`).

- **What the pin does:** `SystemTableInfo2::load` (`table_info.cpp:194-294`)
  reads `binary format version:`, `model data version:`, `lambda parameter:`,
  `source table format:` and `database format:` with five `fscanf`s over one
  stream, then rows of six words (`default <enum name>` or `addon <number>`,
  table, system and user file names, file type) until `feof`. The rows fill
  two arrays of sixteen `pinyin_table_info_t` (default and addon). The init
  loops (`pinyin.cpp:377-392`, `zhuyin.cpp:318-334`) load every default row
  that is not `NOT_USED`; `pinyin_load_addon_phrase_library` loads one addon
  row; `_write_files`/`_rename_files`/`_clean_user_files` write, rename and
  unlink the files the rows name.
- **What oxpinyin did:** read the lambda and version lines only, and
  hard-coded the library layout. A valid but non-stock `table.conf` was
  ignored; a malformed one was accepted without a word.
- **What it does now:** `oxpinyin_data::table_info::TableConf::load` is the one
  reader, over the pin's own `fscanf` stream since external-I/O stage 1
  (2026-10-28 UTC; `TableConf::parse` over the file's bytes before it). The
  decoder's λ, the prediction λ, the user marker's versions and
  database format and the library layout all come from it; the layout reaches
  the system dictionary, the addon dictionary and the user store
  (`oxpinyin_user::SystemOriginals`, `UserStore::has_user_library`), and the
  two C ABIs' library calls follow the rows. Malformed files fail the call that
  triggers the pin's abort, with exactly one warning (policy row 79). The
  header's ordinary `false` returns answer NULL with the raw `load %s failed!`
  line and no warning; a missing `source table format:` line is an
  uninitialised read at the pin (row 80). λ is read as `%f` reads it; and
  outside `[0, 1]` the costs go negative or NaN, reproduced with GLib's merge sorts ported (row 83).
- **Evidence:** `tools/bisection/run-contract-diff.sh bdb <oracle prefix>
  <libpinyin_capi.so> <libzhuyin_capi.so> -- --cases <the table-conf-* names>`
  (141 cases; the abort cases hold the pin's SIGABRT, the crash cases its
  SIGSEGV, and the init aborts compare the user directory each side leaves).
  The SIGSEGV shapes were traced with `gdb -batch -ex run -ex bt --args python3
  <script that opens the pin's libpinyin.so on the private system directory,
  parses nihao, calls pinyin_guess_sentence and pinyin_guess_candidates>` in the
  `debian:testing` container on Linux x86-64.
- **Not held:** (1) a `NULL` user-file name on libzhuyin, where the pin's answer
  depends on the file system (row 79); (2) a default row retyped across the
  fixed roles of sub-indices 5–7 beyond the cases measured (row 78).
- **Found on the way:** the `SystemVersions` documentation said upstream reads
  `table.conf` through GLib's key file; it reads it with `fscanf`, as above.
  `UPSTREAM_DB_FORMATS`' comment said an unknown token is `UNKNOWN_FORMAT`; it
  reaches the `abort()` at `table_info.cpp:132`.

### Bigram export iterator reads past a partial user-gram item (#677; policy row 98, class (b), 2026-10-13 UTC)

- **Upstream source cite:** `src/storage/ngram.cpp:133-143`
  (`SingleGram::retrieve_all`) at
  `074a2219c90feaf962d0d24f034514033ece5f99`, read from the pinned
  libpinyin source; `src/pinyin.cpp:789-889` reaches the walk.
- **Mechanism:** a value with a four-byte total, `k` complete eight-byte
  items and 1–7 trailing bytes has a partial `SingleGramItem` at its end.
  `retrieve_all` walks with `cur != end`, then reads an entire item. This
  can over-read the value on any backend. The partial tail is distinct
  from the complete-item count that `get_length` and indexed reads use.
- **What oxpinyin does instead:** the user loader decodes the total and
  complete items for defined reads, and the raw DBM keeps the original
  value for save. The export iterator refuses a predecessor with a
  partial item tail, returning NULL and emitting one `libpinyin` warning.
  A mask or candidate removal refuses with `false` and one warning in the
  facade domain when a surviving predecessor has such a tail; a mask
  deleting that predecessor wholesale never reads its value.
  The raw tkrzw store returns libtkrzw's actual results, including a
  retained removal record if the linked library returns one.
- **Externally observable:** a caller sees NULL and one warning where
  upstream can read outside the value. The source guard and ordinary
  backend-independent valid-row differential are prepared; no malformed
  crash probe was executed in this source-maintenance phase.

### Training on a malformed user-gram value reads past a partial item (#677; policy row 100, class (b), 2026-10-14 UTC)

`SingleGram::insert_freq` (`src/storage/ngram.cpp:178`, loop `:190` at
`074a2219c90feaf962d0d24f034514033ece5f99`), `get_freq` (`:231`, loop
`:241`) and `set_freq` (`:252`, loop `:261`) walk the existing items with
`cur_item != end`. `pinyin_train`/`zhuyin_train` reach them through
`train_result3` (`pinyin_lookup2.cpp:611`, `phonetic_lookup.h:889`). A value
of length `4 + 8k + r`, `1 <= r <= 7`, leaves a partial eight-byte item at
its end that these loops read past: the same out-of-bounds read as row 98,
on the training path. The maintainer accepted this source-based class (b)
classification; no crash probe was run.

`pinyin_choose_predicted_candidate` reaches the same walks
(`pinyin.cpp:2630-2633`: `get_total_freq`, `get_freq`, `insert_freq` on the
previous token's gram).

- **What oxpinyin does instead:** `pinyin_train`/`zhuyin_train` return
  `false` with exactly one facade-domain warning, and observe nothing,
  whenever the training step would load a predecessor for which
  `has_oob_bigram_value_for` is true; `pinyin_choose_predicted_candidate`
  answers `false` with one warning under the same condition. The check
  reads the live raw value at each call. The raw value is untouched and
  survives save and reopen byte for byte. The previous five-byte
  sentinel-specific attribution and guard are withdrawn.
- **Held by:** `training_over_a_partial_tail_refuses_and_keeps_the_raw_value`
  and `a_partial_tail_created_in_session_refuses_training`
  (`crates/oxpinyin-user/src/store_libpinyin.rs`), at store level.

### A partial phrase-table token tail cannot terminate the entry walk (#799; policy row 101, class (b), 2026-10-15 UTC)

- **Upstream source cite:** `src/storage/phrase_large_table3.cpp:32-39`
  (`PhraseTableEntry::search`), `:57-65` (`add_index`), `:74-81`
  (`remove_index`) and `:103-117` (`mask_out`) at
  `074a2219c90feaf962d0d24f034514033ece5f99`, read with `git show`
  from the libpinyin clone.
- **Mechanism:** the entry casts its raw byte end to a token pointer and
  walks four-byte tokens with `iter != end`. For a value of length
  `4k + r`, `1 <= r <= 3`, the walk cannot reach that end by token
  increments and can read beyond the record. This is an out-of-bounds
  read, not a defined instruction to ignore the trailing bytes. Early
  exits in add/remove may avoid the tail on a particular call; search
  walks it. The source-based class (b) safe answer is user-authorized;
  this entry does not claim a measured crash or a stable pin output.
- **What oxpinyin does instead:** `phrase_table.rs`, `validate` and
  `PhraseTableEntry::from_bytes`, reject the whole incomplete record with
  `StoreError::InvalidInput`; entry operations do not process its complete
  tokens while discarding its tail. `lookup.rs`, `UserLookup::from_store`,
  substitutes an empty entry in the lookup cache, retaining the key and
  exposing no tokens from that record while unrelated lookups remain
  available. `persistence.rs`, `load_user_phrase_index` and `stage_rest`,
  retain and save the raw key/value bytes without normalization. The
  refusal/cache policy does not repair or truncate the stored value.
- **Externally observable:** direct entry consumers receive an error;
  cached lookup exposes no tokens from the malformed record. This records
  those existing behaviors, not a uniform C ABI return or new warning
  guarantee. No malformed pin crash probe was executed. Rows 98 and 100
  concern `SingleGram` tails and do not cover this distinct phrase-table
  record layout.

### A reloaded library's dropped item stays in the user index tables, and a candidate walk dereferences it (#805; policy row 102, class (b), 2026-10-16 UTC)

- **Upstream source cite:** `src/storage/phrase_index.cpp:261-268`
  (`FacadePhraseIndex::unload` deletes the sub-index and takes its total out
  of the facade), `src/pinyin.cpp:264-277` (`_load_phrase_library` maps the
  system chunk and merges only the saved `.dbin`), `src/zhuyin.cpp:298-312`
  (the same path), at `074a2219c90feaf962d0d24f034514033ece5f99`, read from
  the libpinyin checkout at that commit.
- **Mechanism:** the unload frees every item the session had added to the
  library, but the user phrase and pinyin index tables are not unloaded. An
  imported item that was never saved therefore keeps its token in the index,
  and after the reload `pinyin_lookup_tokens` still returns it while the
  sub-index holds no item for it. `pinyin_guess_candidates` (and
  `zhuyin_guess_candidates_after_cursor`) read the item through that token and
  die of SIGSEGV. Measured on the bdb cell: library 2 (GBK), `龘龘` imported with
  count 50, unload, load, then `dada` (pinyin) or `283283` (zhuyin). A dereference
  of an item the sub-index no longer owns has no safe construction.
- **What oxpinyin does instead:** the reload and the unload reproduce the
  pin's state (`GenericUserStore::load_system_libraries` and
  `unload_system_libraries`): the library's rows, overrides and share of the
  total revert to the system chunk plus the saved log, and the user index
  tables are left as they are. `lookup_tokens` answers the dropped token, the
  item reads answer false, and a candidate walk treats the token as an item
  that does not exist: it adds no candidate and the walk completes. The
  candidate list is the one the same input gives before the import.
- **Externally observable:** yes. The pin dies of SIGSEGV in the candidate
  call; oxpinyin returns the list without the dropped item. Every other
  observation of the path (frequencies, readings, ranks, the facade total while
  the library is unloaded, library 1 staying loaded) matches the pin.
- **Held by:** the `reload-*` cases in `tools/bisection/contract-diff.py`
  (`reload-dangling-token-candidates` for each facade as the class (b) case;
  the rest as plain differentials with their saved-first controls), and
  `reloading_a_system_library_drops_the_unsaved_session`
  (`crates/oxpinyin-user/src/store_libpinyin.rs`) at store level.

### A second `pinyin_fini` / `zhuyin_fini` on one context (#573; policy row 105, class (b), 2026-10-17 UTC)

- **Upstream source cite:** `src/pinyin.cpp:1194-1222` (`pinyin_fini`),
  `src/zhuyin.cpp:741-757` (`zhuyin_fini`), at
  `074a2219c90feaf962d0d24f034514033ece5f99`.
- **Mechanism:** a double free. The second call reads the freed context
  (`pinyin_fini` first reads `m_user_table_info`) and deletes its members and
  the context again.
- **What oxpinyin does instead:** both entry points return at once for a
  context the live-context registry does not hold (row 60's registries), with
  no log, since the pin has no path to mirror.
- **Externally observable:** yes. Bdb, `fini-twice` and `zhuyin-fini-twice`
  in `contract-diff.py`, six runs (command in policy row 105): the pin dies of
  SIGSEGV every run on both facades and the subject returns. The cases hold
  the exit status and leave stderr out, since the pin reads freed memory.
- **Left as is:** a second `pinyin_free_instance` / `zhuyin_free_instance`
  (`pinyin.cpp:1347-1356`, `zhuyin.cpp:876-884`) is caller UB on both sides
  (the audit saw SIGSEGV on one pin run and SIGABRT on another). A
  registry of live instances would worsen time and space on every
  alloc/free, so there is no case and no change.

### A prediction candidate whose phrase item a journal REMOVE took out (#1231; policy row 106a, class (b), 2026-10-25 UTC)

- **Upstream source cite:** `src/pinyin.cpp:2389-2405` (prefix filter),
  `:2018-2022` and `:1635-1637` (NULL phrase string),
  `:2041-2068` and `:2082` (dedup `strcmp`), `:2071-2080` (singleton
  dedup), `:2608-2616` (prefix choose), and
  `src/storage/phrase_index.h:628-634` (facade-total increment), at
  `074a2219c90feaf962d0d24f034514033ece5f99`.
- **Mechanism:** `_compute_predicted_prefix_candidates` ignores
  `get_phrase_item`'s result. The loop-wide `PhraseItem` keeps the last
  successful read's length (or the default zero), so the
  `2 * prefix_len + 1` gate uses a stale length: every success replaces it
  before the check, including one the check then skips. An appended missing
  token keeps `m_phrase_string = NULL`. With two or more pre-dedup rows
  the dedup sort's `strcmp` reads it and the pin dies of SIGSEGV; with one
  row neither comparison runs, so the NULL row survives and the getters
  expose count 1, prefix type, a successful string getter writing NULL, a
  failed `token_get_phrase` with untouched outputs, and `false` from
  `is_user_candidate`. Choosing that row calls
  `add_unigram_frequency(token, 483)`: the loaded facade total advances by
  483 modulo 2^32, the missing sub-index write fails, and the prefix
  branch still returns true without a bigram; an unloaded library returns
  before the total moves and still returns true.
- **What oxpinyin does instead:** `select_prefix_rows`
  (`crates/oxpinyin-capi/src/predict.rs`) keeps one plain
  `last_read_length` with the pin's update-before-check order. It keeps a
  sole missing row in the snapshot with `TextSpan::NULL`; with any other
  pre-dedup row it drops missing rows under row 106's class (b) answer
  (no candidate, normal true, no warning). `pinyin_choose_predicted_candidate`
  (`crates/oxpinyin-capi/src/candidates.rs`) advances the loaded facade
  total through `RuntimeDict::add_unigram_delta` and returns true, or
  leaves it unchanged when the library is unloaded. The former
  `.or(fallback)` phrase-table key text and #1190's system-text carry are
  removed; an absent item reaches the adapter as `None`.
- **Externally observable:** yes. `removed-predict-sole-pinyin`,
  `removed-predict-choose-pinyin`, `removed-predict-class-b-pinyin`
  (crash: pin SIGSEGV, subject true with the live candidate) and
  `removed-predict-stale-skip-pinyin` in `contract-diff.py`; the stale
  case is bdb-only by registration, and the other three run on bdb, kc and
  tkrzw and MATCH on all three. The C ABI has no facade-total getter; the 483-total and
  unloaded-unchanged effects are also held by the in-crate test
  `choosing_a_sole_missing_prefix_row_advances_the_facade_total`.
- **Separate pre-existing defect:** the same implementation applies the
  pin's `pinyin_is_user_candidate` type gate (`src/pinyin.cpp:3713-3715`).
  Before it, a predicted-bigram or predicted-prefix row whose token was in
  `USER_DICTIONARY` answered true; `oxpinyin-capi`'s
  `boundary_tests::a_predicted_bigram_row_is_not_a_user_candidate` holds the
  pin's false.
- **Not changed:** the zhuyin facade has no predicted-candidate path.

## Pin portability defects found on arm64 (#1137, 2026-10-24 UTC; amended 2026-10-30 UTC, #1416)

On arm64 the pin departs from its own amd64 reference because it
assigns an out-of-range `float` expression to a `guint32`, a conversion
C leaves undefined. Until #1416 oxpinyin reproduced the amd64
`cvttss2si` store on both architectures (`score as i64 as u32`) and
these entries recorded no oxpinyin divergence. #1416 removed that
imitation of processor behaviour (decision-order limb 3 ([source policy](../source-policy.md#internal-language-artefacts))): the score is
stored with Rust's `as u32`, which in range truncates like both pins
and out of range stores what the arm64 pin's `fcvtzu` stores. Against
the amd64 pin the out-of-range cases are therefore class (b)
divergences, `POLICY-ACCEPTED` and held by the cases named in each
entry; in range nothing changes. They remain pin portability defects
for the upstream report-back batch, one entry per independent defect.
The full diagnosis, the instruction sequences and the proposed issue
texts are in
[`arm64-pin-portability-1137.md`](arm64-pin-portability-1137.md).

### Pinyin candidate frequency: out-of-range `float`-to-`guint32` store (#1137, #1416; class (b))

- **Upstream source cite:** `src/pinyin.cpp:1813-1826` (predicted-prefix
  branch), `:1829-1843` (addon branch) and `:1855-1868` (ordinary branch)
  at `074a2219c90feaf962d0d24f034514033ece5f99`; the conversions are the
  `guint32 freq = ...` initializations at `:1821-1823`, `:1838-1840` and
  `:1862-1865`, and the stores are `item->m_freq = freq` at `:1824`,
  `:1841` and `:1866`.
- **Mechanism:** each branch computes a `gfloat` candidate frequency and
  assigns it to `guint32`. When the value is negative or at least 2³²,
  the conversion is undefined (C++ [conv.fpint]/1). GCC 15.3.0 emits
  `cvttss2si` to a 64-bit register plus a 32-bit store for x86-64 and
  `fcvtzu` for arm64; the latter saturates, so a negative value becomes 0
  and a value at least 2³² becomes `0xffffffff`. For the observed case
  the arm64 conversion is `libpinyin.so.15.0.0+0x72644`
  (predicted-prefix) and the shared store is `+0x72530`; the amd64
  conversion is `+0x8fcce` and the store is `+0x8fb6e`.
- **What oxpinyin does instead:** since #1416,
  `oxpinyin_engine::session::amplified_frequency_with_bigram`, which the
  predicted rows (`oxpinyin_capi::predict::predicted_frequency`) and the
  ordinary candidate lists share, stores the `f32` score with Rust's
  `as u32`: in range it truncates like the pin; a negative or NaN score
  is 0 and a score at or above 2³² is `0xffffffff`. The earlier
  `score as i64 as u32` imitation of the amd64 `cvttss2si` result (the
  `predict.rs` copy and `oxpinyin_engine::session::guint32_of_score`)
  is removed under decision-order limb 3. The rows are still ordered with the
  pin's `guint32` comparator (`oxpinyin_engine::candidate::rank_by_frequency`).
- **Externally observable:** yes, in the candidate order, only when a
  score is out of range. `row58-unsigned-score-off` and
  `row58-unsigned-score-on`: import `我甲甲` with
  count 1431655631, bring the facade total to 10000000 through the
  absent-token delta, then call
  `pinyin_guess_predicted_candidates("我")`. The frequency is
  `4952525312.0f`; the amd64 pin stores `0x27318a00 = 657558016` and
  lists `甲甲` first; oxpinyin stores `0xffffffff` and lists it last
  among the two-character rows, as the arm64 pin does. The other 167
  rows are in the same order on both sides.
- **Classification:** class (b). C leaves the conversion undefined and
  the pin's own result differs between x86-64 and AArch64; oxpinyin's
  saturating store is the language's defined result and is not tuned to
  either target. Also a pin portability defect to report upstream.
- **Held by:** `oxpinyin-engine`'s
  `session::tests::candidate_scores_follow_rusts_saturating_conversion`,
  which holds oxpinyin's value. The pin observations `row58-unsigned-score-off`
  and `row58-unsigned-score-on` were `POLICY-ACCEPTED` on bdb, kc and sound tkrzw against the amd64 pin,
  with the digests in
  [`arm64-pin-portability-1137.md`](arm64-pin-portability-1137.md)
  (recorded 2026-10-30 UTC, #1416; before #1416 they were MATCH on the
  three cells and arm64-excluded by #1137).
- **Proposed upstream report:** issue A in
  `arm64-pin-portability-1137.md` ("pinyin candidate frequency:
  undefined float-to-`guint32` conversion is target-dependent").

### Zhuyin candidate frequency: negative `float`-to-`guint32` store at λ > 1 (#1137, #1416; class (b))

- **Upstream source cite:** `src/zhuyin.cpp:1229-1270`
  (`_compute_frequency_of_items`), the conversion in the
  `guint32 freq = ...` initialization at `:1264-1267` and the store at
  `:1268`, at `074a2219c90feaf962d0d24f034514033ece5f99`.
- **Mechanism:** with the default options `DYNAMIC_ADJUST` is clear, so
  the frequency is `(1 - lambda) * unigram / (gfloat) total_freq *
  256 * 256 * 256`. For λ > 1 the coefficient is negative, every
  positive-unigram candidate gets a negative `float`, and the assignment
  to `guint32` is undefined. GCC 15.3.0 emits `cvttss2si` plus a 32-bit
  store for x86-64 (`libzhuyin.so.15.0.0+0x8e06d`, store `+0x8e072`) and
  `fcvtzu` for arm64 (`+0x70938`, store `+0x7093c`). amd64 keeps the
  low 32 bits of `-k` (for `0 < k < 2³²`, that is `2³² - k`); arm64
  stores 0, so every negative key is equal.
- **What oxpinyin does instead:** since #1416 the shared engine law
  `oxpinyin_engine::session::amplified_frequency_with_bigram` stores a
  negative score as 0 (Rust's `as u32`), so for λ > 1 every
  positive-unigram candidate has the key 0 and the rows keep their
  collection order under the pin's comparator, as on the arm64 pin.
  Before #1416 `guint32_of_score` produced the amd64 low-32 key on both
  architectures; that imitation is removed under decision-order limb 3.
- **Externally observable:** yes, on arm64 only, in
  `zhuyin_guess_candidates_after_cursor`. With
  `lambda parameter:1.5`, `2` and `1e6` in `table.conf`, the arm64 pin's
  candidate rows 3–12 are the collection order within the length-1 group
  `疒 祢 禰 妳 呢 堄 逆 猊 霓 儗` and are identical for all three λ
  values; the amd64 pin orders them by the wrapped key and differs per
  λ. The sentences are identical on both targets. The λ < 0, `inf` and
  `nan` cases match because their keys are in range or equal on both
  architectures.
- **Classification:** class (b). C leaves the conversion undefined and
  the pin's own result differs between x86-64 and AArch64 (policy row
  83 keeps λ outside `[0, 1]` itself as reproduced input handling). Also
  a pin portability defect to report upstream.
- **Held by:** `table-conf-lambda-million-zhuyin`,
  `table-conf-lambda-one-and-a-half-zhuyin` and
  `table-conf-lambda-two-zhuyin`, `POLICY-ACCEPTED` on bdb, kc and
  sound tkrzw against the amd64 pin with the digests in
  [`1378-zhuyin-selection.md`](1378-zhuyin-selection.md), whose entries
  already held the #1378 selection difference in the same cases
  (re-recorded 2026-10-30 UTC, #1416, on the three cells).
- **Proposed upstream report:** issue B in
  `arm64-pin-portability-1137.md` ("libzhuyin candidate frequency: λ > 1
  orders candidates differently on arm64").

### NULL string conversion after defined prior work (#1279)

Class (b), following the #1272/#1277 no-input ruling. At 074a2219
`src/pinyin.cpp:1450-1452` and `src/zhuyin.cpp:969-971`, NULL sentence
first calls `g_utf8_strlen`, which emits its precondition critical
(GLib 2.84.4 `glib/gutf8.c:240`, GLib domain from `glib/meson.build:404`),
then `g_utf8_to_ucs4` dereferences NULL (`gutf8.c:950`). Both adapters'
`phrase_segment` entries preserve that critical, then segment no input:
true with the prior phrase-result list cleared (`lookup/phrase_lookup.cpp:36-52,119-157,382-428`).

NULL lookup phrase faults before `reduce_tokens` at
`src/pinyin.cpp:2658` / `src/zhuyin.cpp:1684`; the adapters' `lookup_tokens`
entries take the no-input lookup, return false and clear the caller array.
Batch parse NULL strings fault at strlen before instance mutation
(`pinyin.cpp:1509,1552,1599`; `zhuyin.cpp:1032,1070`); their existing
no-input path returns zero and clears parse state. Single-key NULL strings
fault at strlen before key mutation (`pinyin.cpp:1491,1534,1575`;
`zhuyin.cpp:1011,1052`); the existing guard returns false and leaves the
key untouched. NULL `parse_full_pinyin` is the single-key case in point:
the pin faults at strlen (`pinyin.cpp:1491`, `zhuyin.cpp:1011`) before
`parse_one_key` touches the key, so the key stays untouched. No UB is
reproduced. Existing row 41 remains the guard classification; this entry
records the state-sensitive string outcomes.

Cases: `c-string-input/*-segment-state-null`, `*-lookup-state-null`,
`*-parse*-null`, `*-character-offset-null`, `*-import-null` and
`*-remember-null`, with the pin not driven into UB (`pin_not_run`), plus
non-empty valid controls.

## Native scanner and tool parsing (#1297 Phase 2)

The 2026-10-28 UTC external-I/O ruling makes the ten scanner, writer and
tool entries below **revert targets**. Their dated findings preserve the
earlier evidence. Stage 1 of [external-io-boundary.md](external-io-boundary.md)
(2026-10-28 UTC) reverted the seven `table.conf` and `user.conf` entries:
`oxpinyin-input` makes the pin's own calls, their cases run their original
probes as MATCH, and their records are no longer registered by any case.
The punctuation record's undefined-read safety treatment stands under
class (b); do not reproduce the pin's unsafe surrounding defect.

- Decimal lambda grammar: reverted at stage 1, host `fscanf` `%f` on the pin stream.
- Representable version fields: reverted at stage 1, host `fscanf` `%d`.
- Configuration line boundaries: reverted at stage 1, pin's ordered stream formats.
- Addon indexes in the library range: reverted at stage 1, host `atoi` and pin conversion.
- Nonconforming decimal markers reset profiles: reverted at stage 1, marker `fscanf` `%d`.
- Exact database marker tokens: reverted at stage 1, `%255s` and `strcmp`.
- [Punctuation values contain whole words](1297-punct-words.md): not superseded; the existing class (b) safety treatment stands under the 2026-10-28 UTC ruling.
- Complete chunk payload writes: superseded (2026-10-29 UTC) by the 2026-10-28 UTC external-I/O ruling; revert target at stage 2, one host `write`.
- Complete tool decimal fields: superseded (2026-10-29 UTC) by the 2026-10-28 UTC external-I/O ruling and the tools drop-in ruling; revert target at stage 5, host `atoi`/`atol`.
- Native nonfinite tool formatting: superseded (2026-10-29 UTC) by the 2026-10-28 UTC external-I/O ruling and the tools drop-in ruling; revert target at stage 5, pin's formatted call.
- Complete table words and exact enum tokens: reverted at stage 1, `%255s` and C-string comparisons.

## Pinyin trellis adjusted cost — S1/S2 repair (2026-10-28 UTC)

[Pinyin trellis adjusted-cost comparison](trellis-adjusted-cost-s1-s2.md)
records the repair authorized by the witnessed algorithm-precondition amendment:
ordered adjusted cost for node heaps and preselection batch-position ties for
beam/final-tail selection. Four exact BDB case pairs report `POLICY-ACCEPTED`,
each attributed in that record; since #1400, the two gb table.conf cases
moved by the S2 tie rule hold their bdb, kc and tkrzw pairs there too, as do the
two negative-lambda cases moved by the special-cost completion, and the
three lambda cases' kc and tkrzw pairs. This is the amendment's product-level ruling,
not a new exception class. Row 83 remains historical; zhuyin, tail ordering,
node front eviction, frequency wrap and lambda validation are unchanged.

## Stable raw-cost tail ordering — S3 repair (2026-10-28 UTC)

Stable raw-cost tail ordering records the S3
repair authorized by the witnessed algorithm-precondition amendment:
stable raw possibility descending, equal-cost input order preserved and
NaN last. Both facades share the final sort; zhuyin selects at most one tail.
The focused BDB selection has no new movement: four MATCH controls and the
four existing S1/S2 POLICY-ACCEPTED cases retain their complete observations.
The S1/S2 record remains unchanged. No new case exemption is registered.
Selection, node front eviction, candidate/prediction frequency wrap,
lambda validation, external calls, persistent formats and C ABI are unchanged.

## Standard-library searches of stored records (#1297)

### Stored gram partitions: phrase segmentation — inventory C

- **Record:** [1297-gram-partition-segmentation](1297-gram-partition-segmentation.md).
- **Status:** accepted. `user-gram-unsorted-phrase-segment` is
  POLICY-ACCEPTED on bdb, kc and sound tkrzw, with its complete-observation
  digests recorded separately for each cell.
- **Upstream source cite:** `src/storage/ngram.cpp:231–250,308–349`
  and `src/lookup/phrase_lookup.cpp:268–290` at `074a2219`.
- **Mechanism:** the stored-order merge of an out-of-order gram violates
  the search predicate's partition requirement. `single_gram::lower_bound_records`
  uses `slice::partition_point` on whole stored records without sorting or cleaning.
- **Externally observable:** the pin segments 今天天气 as one phrase; Rust
  selects 今天 and 天气 separately after reaching the boosted 今天 record.
  Both segmenters return true. Ordered duplicates retain first-not-less
  semantics; existing refusals, persistent formats and C ABI are unchanged.

### Stored gram partitions: training — inventory C

- **Record:** [1297-gram-partition-training](1297-gram-partition-training.md).
- **Status:** accepted. `abort-train-unsorted-set-freq-miss-pinyin` and
  `abort-train-unsorted-set-freq-miss-zhuyin` are POLICY-ACCEPTED on bdb,
  kc and sound tkrzw, each with complete-observation digests recorded
  separately for each cell.
- **Upstream source cite:** `src/storage/ngram.cpp:178–205,231–271`
  and `src/lookup/phonetic_lookup.h:888–903` at `074a2219`.
- **Mechanism:** an out-of-order user gram violates the search predicate's
  partition requirement. `single_gram::lower_bound_records` uses
  `slice::partition_point` on whole stored records without sorting or cleaning.
- **Externally observable:** the pin's final set-freq search misses after
  insertion and training aborts; Rust reaches the inserted item and training
  returns true with no warning. An actual post-insertion miss still uses the
  existing refusal. Persistent formats and C ABI are unchanged.

## libpinyin utilities (external-I/O boundary stage 5)

`oxpinyin-utils` ports the pin's utilities under their own names, with every
edge call made through `oxpinyin-input`'s `tools` boundary
([external-io-boundary.md](external-io-boundary.md), stage 5). Pin citations
are `utils/storage/gen_pinyin_table.cpp` at
`074a2219c90feaf962d0d24f034514033ece5f99`.

### gen_pinyin_table invalid UTF-8 phrase (class (b), 2026-10-30 UTC)

- **Upstream source cite:** `main` (`:97-131`), `feed_line` (`:160-232`),
  `phrase_item_compare` (`:85-94`), `gen_phrase_file` (`:259-322`).
- **Mechanism:** a phrase that is not valid UTF-8 can pass both of
  `feed_line`'s checks (`g_utf8_strlen` counts lead bytes, and a pinyin of
  that many keys parses), while `g_utf8_to_ucs4` answers NULL (`:174`). The
  item enters the tree with that NULL `uniphrase`. `main` reads every input
  file (`:110-112`), prints `nnodes` (`:114`), stores and sorts
  (`:116-127`), and `gen_phrase_file` opens the output with `fopen(outputfile,
  "w")` (`:261`) before `g_ucs4_to_utf8` reads through the NULL
  (`:280-281`): undefined behaviour, a SIGSEGV on the reference host. The
  buffered `nnodes` line and any rows already printed are lost with the
  process. Where the tree compares the item with another phrase of its
  length, `phrase_item_compare`'s `memcmp` reads through the NULL earlier,
  before the output is opened, at a point GLib's tree shape decides.
- **What oxpinyin does instead:** it keeps the item unconverted, reads
  every input and opens the output at the pin's point (`:261`). At the row
  where the pin reads through the NULL (`:280-281`) it writes
  `gen_pinyin_table: invalid UTF-8 phrase: <phrase bytes>` on `stderr` and
  exits 1 through `exit`, which flushes what it wrote. The pin crashes
  there (a SIGSEGV on the reference host); oxpinyin exits 1 with that
  message. stdout and any partial output may differ, because the pin's
  crash discards whatever its C streams still buffer (the `nnodes` line
  and rows already printed, depending on whether stdout is a pipe or a
  terminal); that loss is the effect of the undefined behaviour and libc
  buffering, not behaviour oxpinyin reproduces. An unconverted phrase
  sorts before the converted phrases of its length, so the earlier
  `memcmp` death is not reproduced.
- **Externally observable:** the exit status, oxpinyin's message, and
  stdout and the output file where the pin's buffers were lost. Held by
  `an_unconvertible_phrase_is_refused_at_the_pins_row_after_the_output_opens`
  (`crates/oxpinyin-utils/tests/gen_pinyin_table.rs`). Against the pin-built
  utility (#1436) the two invalid-UTF-8 cases are POLICY-ACCEPTED under
  this record.
- **Also class (b), no observable choice made:** `g_utf8_strlen` over a
  truncated sequence at the end of a `%1023s` word steps past its NUL into
  the indeterminate bytes of the pin's `char phrase[1024]` (`:136`, `:162`);
  oxpinyin's buffer keeps the earlier words' bytes as the pin's frame does,
  starts zeroed and has eight zero bytes past the 1024. A `-t` index of 128
  or more, or a negative one, overflows the pin's `int` shift in
  `PHRASE_INDEX_MAKE_TOKEN` (`src/include/novel_types.h:45-46`); oxpinyin
  wraps the shift and keeps the masked bits.

### gen_pinyin_table phrase order is a native-host result (2026-10-30 UTC)

Not a divergence. `phrase_item_compare` orders equal-length phrases by
`memcmp` over their UCS-4 words in host byte order, and the stable
`g_array_sort_with_data` (`:124-126`; GLib's documented guarantee since 2.32)
keeps that order among phrases whose first readings are equal. Tokens are
numbered in that order, so a little-endian and a big-endian host number
such phrases differently. oxpinyin compares the words' native-endian bytes
and reproduces the pin on each host (constitution §6, #1303). Held by
`rows_follow_length_then_first_reading_then_phrase_bytes`.

The shared `tool-diff` runner binds the pinyin and zhuyin invalid-UTF-8
cases to [eio-table-generator-invalid-utf8.md](eio-table-generator-invalid-utf8.md).
The zhuyin utility's earlier missing-frequency null read is separately
recorded in [eio-zhuyin-missing-frequency.md](eio-zhuyin-missing-frequency.md).

### k_mixture_model_to_interpolation undefined lines (2026-10-31 UTC)

**Class (b).** The pin's `taglib_read` hands `strcmp` the NULL `tokens[0]`
of a line without a token, and reads through the NULL `split_line`
answers for an escape ending the line (`src/storage/tag_utility.cpp:137`,
`:181-190`); the reference pin dies with SIGSEGV. oxpinyin's utility makes
the pin's calls up to that read, GLib's criticals included, then refuses
with exit 1. Recorded and bound in
[eio-kmm-to-interpolation-undefined-lines.md](eio-kmm-to-interpolation-undefined-lines.md).
A truncated UTF-8 sequence at the line's end, whose next `*cur` reads
whatever follows the string, is refused the same way; the pin has no
reproducible outcome there to bind, and
`undefined_lines_are_refused_normally` holds the refusal.
The utility's `abort()` calls are not divergences: both sides abort, and
`tool-diff` compares them as ordinary cases.

### g_utf8_next_char from a byte that leads no sequence (2026-10-31 UTC)

**POLICY-ACCEPTED.** `split_line` steps with `g_utf8_next_char`, which
GLib documents for valid UTF-8 and implements with its internal
`g_utf8_skip` table. oxpinyin steps the length the lead byte gives, and 1
for a byte that leads no sequence. Only `0xF8..=0xFD`, which the table
steps 5 or 6, differ. Recorded with its cases in
[eio-utf8-next-char-invalid-lead.md](eio-utf8-next-char-invalid-lead.md).

### Utility check macros write no message (2026-10-31 UTC)

**Language artefact.** Under the maintainer ruling of 2026-10-31 UTC the
utilities' `assert` (`check_result`) and `g_return_val_if_fail` checks are
plain Rust: on the pin's condition oxpinyin answers the pin's value or
aborts where the pin aborts, without glibc's assertion line or GLib's
critical. Stdout and the SIGABRT exit are the pin's. Recorded and bound in
[eio-tool-check-messages.md](eio-tool-check-messages.md).

The same ruling covers the seven KMM utilities' asserts:
`merge`'s missing result attach, `estimate`'s zero magic total and `gen`'s
missing container header abort with SIGABRT on both sides and no
subject assertion line. Bound per cell in
[eio-kmm-tools.md](eio-kmm-tools.md).

### Pinyin-index item padding is indeterminate in the pin (class (b), 2026-11-01 UTC)

`ChewingTableEntry<N>::add_index` (`src/storage/chewing_large_table2.h:
208-232`) copies a stack `PinyinIndexItem2<N>` (`src/storage/pinyin_phrase3.h:
181-196`) into the DBM value whole, and for an odd `N` the object's last two
bytes are tail padding no constructor sets. The subject's
`row_format::pinyin_index::encode_item` writes zeros there. Tokens and keys
are identical and no reader interprets the padding. Recorded and bound for
the `gbf-toned-model` tool-diff case in
[eio-pinyin-index-padding.md](eio-pinyin-index-padding.md); the case stays
DIFFER on a cell whose DBM library also writes its own container header
until the comparator ignores that metadata.

### Library `g_return_val_if_fail` criticals are not reproduced (2026-10-31 UTC)

**Maintainer decision D1 of #1297**, a narrow exception to the library
stderr ruling (#545) under the 2026-10-31 UTC no-shims ruling. Both
facades' `phrase_segment` on invalid UTF-8 (`pinyin.cpp:1454`,
`zhuyin.cpp:973`) and `SubPhraseIndex::load`'s layout checks
(`phrase_index.cpp:354-356`, `:361`) keep the pin's return values, state
and continuation and log no critical. Lost: the critical a caller's log
handler or stderr receives, and the pin's abort under GLib fatal-critical
settings. Recorded and bound in
[1297-segment-critical.md](1297-segment-critical.md) and
[1297-layout-criticals.md](1297-layout-criticals.md).

### Reproduced library aborts write no assertion line (2026-10-31 UTC)

**POLICY-ACCEPTED, stderr only.** Since class (c) was retired, oxpinyin
reproduces each pin `assert` site with `std::process::abort()` and does
not rebuild glibc's `Assertion '…' failed.` line; the SIGABRT status, the
earlier stderr bytes and the files left are MATCH. Recorded in
[pin-abort-assert-line.md](pin-abort-assert-line.md).

### Pin aborts reproduced in the pinyin C facade (2026-10-31 UTC)

**Pin behaviour reproduced.** Member 3 of the class (c) retirement: the
pinyin C facade aborts the process with `std::process::abort()` at the
pin's `assert` and `abort()` sites of rows 4, 5a, 10, 14, 38, 45, 63, 64,
81, 88, 93 and 95, and on the pinyin arm of rows 5c, 5d, 6, 19, 21, 44,
61, 62, 66, 74, 77, 79, 82, 84, 86, 87 and 94. `abort()` sites are MATCH;
`assert` sites are POLICY-ACCEPTED for the missing assertion line only.
Recorded, with the pin sites, items and cases, in
[pin-aborts-pinyin-facade.md](pin-aborts-pinyin-facade.md).

### Pin aborts reproduced in the zhuyin C facade (2026-10-31 UTC)

**Pin behaviour reproduced.** Member 4 of the class (c) retirement: the
zhuyin C facade aborts the process with `std::process::abort()` at the
pin's `assert` and `abort()` sites of rows 89, 90 and 104, and on the
zhuyin arm of rows 5c, 5d, 6, 19, 21, 44, 61, 66, 74, 77, 79, 82, 84, 86,
87 and 94. `abort()` sites are MATCH; `assert` sites are POLICY-ACCEPTED
for the missing assertion line only. Recorded, with the pin sites, items
and cases, in [pin-aborts-zhuyin-facade.md](pin-aborts-zhuyin-facade.md).

### Pin aborts reproduced in the journal replay (2026-10-31 UTC)

**Pin behaviour reproduced.** Member 5a of the class (c) retirement: a
system library's user `.dbin` is replayed in the pin's order at init, at a
library reload and at `mask_out`, and the assertions the pin's walk reaches
(rows 91, 92, 97 and 108: `phrase_index_logger.h:202`, `:245`, `:257`,
`phrase_index.cpp:745`, `memory_chunk.h:390`) return a typed fault on which
both C facades call `std::process::abort()`. The up-front log validation
and its refusals are deleted. All sites are `assert`s: POLICY-ACCEPTED for
the missing assertion line only. Recorded, with the pin sites, items and
cases, in [pin-aborts-journal-replay.md](pin-aborts-journal-replay.md).

### Library transitions in memory (2026-10-31 UTC)

**Pin behaviour reproduced.** Member 5b of the class (c) retirement: a
library unload or reload changes only the session's in-memory state and
the runtime's visibility mask, as `FacadePhraseIndex::unload` and
`_load_phrase_library` do (row 103), and the phrase export iterator's
read of the session phrases cannot fail (row 62's store-read sub-shape).
Neither row has a pin abort; the refusals and their warnings are deleted.
A save now skips an unloaded library, as `_write_files` and
`_rename_files` do. Recorded, with the pin lines, items and cases, in
[library-transitions-in-memory.md](library-transitions-in-memory.md).

### Pin aborts reproduced: the short user-bigram total and the directory library (2026-10-31 UTC)

**Pin behaviour reproduced.** Member 5c of the class (c) retirement, the
last two rows. Row 85: a user-bigram value shorter than its `guint32`
total (`memory_chunk.h:390`) is a typed fault at mask-out, candidate
removal, predicted guessing and training, `*_train`, the lookups and the
bigram export walk, on which both C facades abort; the export now aborts
where `has_next` loads that predecessor, after the earlier rows, instead of
refusing at begin. The partial item tail of rows 98 and 100 is split from it
and keeps its class (b) refusals. Row 96: a system library that is a
directory (`memory_chunk.h:493`) is the fault where init's, the save's and
the mask's library loops map it, after the earlier libraries' work (the
save's earlier `.tmp` files, renamed by libzhuyin), and at the reload and
the addon load. All sites are `assert`s: POLICY-ACCEPTED for the missing
assertion line only. Every former class (c) row is now reproduced.
Recorded, with the pin sites, items and cases, in
[pin-aborts-remaining.md](pin-aborts-remaining.md).

### The full-pinyin parse's 16-bit steps (2026-11-01 UTC, #1501)

**Language artefact, POLICY-ACCEPTED.** The pin's full-pinyin parse stops at byte 32767 because its steps are `gint16`s (`pinyin_parser2.cpp:67-70`); oxpinyin parses the whole input, and policy row 59 is reopened: [1501-gint16-input-cap.md](1501-gint16-input-cap.md).

### Key positions past byte 65535 (2026-11-01 UTC, #1501)

**Contract for the getters, POLICY-ACCEPTED for the matrix.** The C getters wrap key-rest positions past 65535 mod 2^16 as the pin's `guint16` fields do; oxpinyin keeps real positions where the pin's `fill_matrix` places keys at the wrapped begin (`phonetic_key_matrix.cpp:51-55`): [1501-guint16-key-positions.md](1501-guint16-key-positions.md).

### FlexibleBigram malformed record lengths

- **Upstream source cite:** `src/storage/flexible_ngram_bdb.h:165-187`
  and its Kyoto Cabinet and tkrzw peers at 074a2219;
  `src/storage/flexible_single_gram.h:88-105,151-181`.
- **Mechanism:** the pin wraps the whole stored value in a
  `FlexibleSingleGram` and reads the header and item tail without
  checking that the value holds whole records.
- **What oxpinyin does instead:** `SpecGram::decode` answers
  `StoreError` for a value shorter than its header or with a partial
  item tail.
- **Externally observable:** only for a corrupted or hand-written
  container; a pin-written file never takes this path.
- **Record:** [eio-flexible-bigram-malformed-records](eio-flexible-bigram-malformed-records.md),
  class (b), 2026-11-01 UTC.

### FlexibleBigram unsorted-record search (2026-11-01 UTC)

[eio-flexible-bigram-unsorted-records](eio-flexible-bigram-unsorted-records.md)
records the standard-library search contract and the malformed KC KMM
fixture's provenance. The implementation already uses Rust partitioning;
there is no copied external probe sequence. Sorted results remain exact.
Any distinguishing unsorted result is a language artefact under the
2026-10-29 ruling, requiring concrete POLICY-ACCEPTED evidence.

### Berkeley DB handles whose open failed (2026-11-02 UTC, #1516)

[bdb-failed-handle](bdb-failed-handle.md) holds the accepted digests.
The explanation is [compatibility register, row 113](compatibility-policy.md#bdb-failed-handles).
