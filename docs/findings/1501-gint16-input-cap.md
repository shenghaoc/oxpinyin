# 1501-gint16-input-cap — the full-pinyin parse's 16-bit steps

- **Status:** accepted · 2026-11-01 UTC · issue #1501 · PR to follow
- **Disposition:** POLICY-ACCEPTED (never MATCH)
- **Policy limb:** language artefact (decision-order limb 3, 2026-10-26 UTC
  and the 2026-10-29 UTC "Contract, not implementation" ruling). Not class
  (a): no arithmetic result is wrong, only the width of a field. Not (b):
  every write stays in bounds (#1501, second comment). Not (c): retired
  2026-10-31 UTC, and nothing aborts. A language-neutral requirement for
  "parse the longest prefix with the fewest keys" would not mention a
  32767-byte stop: it comes from "an integer width chosen for storage".
  Maintainer decision, 2026-11-01 UTC, recorded in #1501. Policy row 59
  (#540) reproduced the bound under reasoning that predates the
  2026-10-26 and 2026-10-29 UTC rulings; it was a lane closure, not a
  maintainer ruling, so limb 6 does not protect it.
- **Pin:** libpinyin 2.11.92 at 074a2219; tree read: a fresh clone at that
  commit, blobs hashed against the pin (`src/storage/pinyin_parser2.cpp`
  `47c58761`, `src/storage/pinyin_parser2.h` `790c0cbf`, `src/pinyin.cpp`
  `f27f7cf7`, `src/zhuyin.cpp` `d1520c18`, `src/storage/zhuyin_parser2.cpp`
  `4ef4500b`, `src/storage/chewing_key.h` `7ec4dfb4`,
  `src/storage/phonetic_key_matrix.cpp` `ab7e8796`,
  `utils/storage/gen_pinyin_table.cpp` `fccc8c3f`); GLib/libc of the
  reference image: not involved; facade: both; backend: all.
- **Pin derivation:** `parse_value_t` keeps `m_num_keys`, `m_parsed_len`,
  `m_distance` and `m_last_step` as `gint16` (`pinyin_parser2.cpp:67-70`).
  `FullPinyinParser2::parse` fills `len + 1` steps with `int` indices
  (`:226-231`); the apostrophe branch stores `m_parsed_len = … + 1` and
  `m_last_step = i` (`:246`, `:248`), the key branch `m_parsed_len = … +
  onepinyinlen` and `m_last_step = m` (`:290`, `:292`). Past byte 32767
  these values do not fit, and the conversion is implementation-defined.
  `final_step` scans back from step `len` and stops at the first step
  whose sign-extended `m_parsed_len` equals its index (`:353-357`;
  `movswl 0x8(%rax),%r13d` at `0x5caac`, `cmp %esi,%r13d` at `0x5cab4` in
  the pin-built object), which is never above 32767; `parse` returns that
  `gint16` (`:333`). So no full-pinyin parse consumes more than 32767
  bytes.
- **Rust counterpart:** `oxpinyin_core::graph::SegmentGraph` (positions in
  `u32`, `MAX_GRAPH_POSITION`), `oxpinyin_engine::Session` (the raw input,
  `session/buffer.rs`), `parse_import_pinyin` in
  `crates/oxpinyin-capi/src/iterators.rs`; C entry points below.
- **Forward trace:** `pinyin_parse_more_full_pinyins` /
  `zhuyin_parse_more_full_pinyins` → `InstanceCore::parse_full_more`
  (`crates/oxpinyin-facade/src/parse.rs`) → `Session::replace_raw` (HANYU)
  or the scheme index parse plus `replace_raw_full_pinyin` (LUOMA,
  SECONDARY_ZHUYIN) → the segment graph → `full_parsed_len` → the return
  value, `pinyin_get_parsed_input_length`, the scan matrix, the key and
  offset getters, the auxiliary text, candidates and sentences.
  `pinyin_iterator_add_phrase` → `parse_import_pinyin` → the graph's
  fewest-keys path → the phrase-length gate (`pinyin.cpp:640`) →
  `_add_phrase` (`:541`).
- **Backward trace:** every producer of a parse length or key list:

  | Entry point | Pin parser and step width | oxpinyin before | oxpinyin after |
  | --- | --- | --- | --- |
  | `pinyin_parse_more_full_pinyins` (`pinyin.cpp:1507-1509`) | `FullPinyinParser2`, `gint16` steps: stops at 32767 | session raw clamped to 32767 bytes (`MAX_INPUT_BYTES`) | whole input |
  | `zhuyin_parse_more_full_pinyins` (`zhuyin.cpp:1030-1032`) | `FullPinyinParser2`, `gint16` steps: stops at 32767 | same session clamp | whole input |
  | LUOMA and SECONDARY_ZHUYIN schemes of the two above (`set_scheme`, `pinyin_parser2.cpp:384-403`) | `FullPinyinParser2`, `gint16` steps: stops at 32767 | the scheme index parse was unbounded, but the session clamped its canonical text to 32767 bytes and dropped later segments | whole input |
  | `pinyin_iterator_add_phrase` (`pinyin.cpp:631-637`) | `FullPinyinParser2`, `gint16` steps: stops at 32767 | first 32767 bytes (`parse_import_pinyin`) | whole reading |
  | `gen_pinyin_table` (`gen_pinyin_table.cpp:176-181`) | `FullPinyinParser2`, `gint16` steps: stops at 32767 | whole line up to 65535 bytes, no keys past it (`MAX_GRAPH_INPUT`) | whole line |
  | `pinyin_parse_full_pinyin`, `zhuyin_parse_full_pinyin` (`pinyin.cpp:1492`, `zhuyin.cpp:1012`) | `parse_one_key`, one key, no steps | one key | unchanged |
  | `pinyin_parse_more_double_pinyins` (`pinyin.cpp:1549-1553`) | `DoublePinyinParser2`, `int` (`pinyin_parser2.cpp:538-572`): unbounded | parser unbounded, session clamped the joined text to 32767 bytes | whole input |
  | `pinyin_parse_more_chewings`, `zhuyin_parse_more_chewings` | `ZhuyinSimpleParser2`, `ZhuyinDiscreteParser2`, `ZhuyinDaChenCP26Parser2`, `int` (`zhuyin_parser2.cpp:216-260`, `:408-452`, `:718-790`): unbounded | parser unbounded, session clamped the joined text | whole input |
  | `zhuyin_iterator_add_phrase` (`zhuyin.cpp:516-523`), `gen_zhuyin_table` | `ZhuyinDirectParser2`, `int` (`zhuyin_parser2.cpp:914-961`): unbounded | unbounded | unchanged |
  | auxiliary text, cursor offsets, lookups, `composition_keys` | the matrix of the parse above | graphs over the clamped raw input | graphs over the whole input |

  Only `FullPinyinParser2` keeps its steps in 16 bits. In oxpinyin, the
  32767 bound was `oxpinyin_engine::MAX_INPUT_BYTES` (every session parse,
  so it also clamped the double-pinyin and chewing sessions, whose pin
  parsers have no bound) and `parse_import_pinyin`. The graph's 65535-byte
  refusal (`MAX_GRAPH_INPUT = 65_535`) had no pin counterpart and no
  product rule; the 32767 clamp made it unreachable except from
  `gen_pinyin_table`.
- **Input partition:**
  - Valid supported input: any reading or composition up to 32767 bytes,
    on which both sides parse the same prefix (unchanged; controls below).
  - Malformed / non-scalar / host-dependent input: not involved; the
    bound is on length only.
  - Other (over-long): a full-pinyin input or reading longer than 32767
    bytes. This is the only partition that differs.
- **Difference:** for a full-pinyin input longer than 32767 bytes the pin
  parses at most its first 32767 bytes; oxpinyin parses all of it. `ni`
  repeated to 32768, 65535, 65536 and 65537 bytes: the pin's
  `pinyin_parse_more_full_pinyins` and `pinyin_get_parsed_input_length`
  answer 32767 each time, oxpinyin the input length. A two-character
  import reading `ni3` + 40000 apostrophes + `hao3` gives the pin one key
  (the add refuses and leaks, `pinyin.cpp:639-652`) and oxpinyin two keys
  (the add runs). The mechanism is the `gint16` fields and the
  implementation-defined narrowing above.
- **Why accepted:** the stop is the width of a C++ storage field, not a
  rule of the parse. oxpinyin's graph keeps positions in `u32`, wider than
  the pin's own `int` lengths (`pinyin_parser2.h:131`), and its candidate
  span counts in `u32` (`crates/oxpinyin-engine/src/candidate.rs`, widened
  from `u16`). No new limit replaces the old ones: the graph refuses only
  an input past `MAX_GRAPH_POSITION` (`u32::MAX - 1` bytes), whose
  positions an edge cannot hold.
- **Contract preserved:** C ABI and layout unchanged, including
  `ChewingKeyRest`'s `guint16` positions (`chewing_key.h:102-103`): a key
  position past 65535 reaches the caller through
  `pinyin_get_pinyin_key_rest` and `pinyin_get_pinyin_key_rest_positions`
  narrowed to that field mod 2^16, as the pin's are
  (`crates/oxpinyin-capi/src/cursor.rs`; the C adapters saturated at 65535
  until the maintainer's 2026-11-01 UTC ruling recorded in
  [1501-guint16-key-positions](1501-guint16-key-positions.md)). The pin's
  full-pinyin parse never produces such a position; its double-pinyin and
  chewing parses do, and that record holds what follows. Ownership and lifetime unchanged. Every export
  unchanged; the affected ones answer for the whole input. On-disk system
  and user files: unchanged format; an over-long import reading now stores
  its full key list where the pin refused or stored the 32767-byte prefix's
  keys. Data ownership and process-wide side effects unchanged.
  Diagnostics: none involved. The pin's leak on a refused import
  (`pinyin.cpp:639-652`) is not reproduced on readings that now parse in
  full.
- **Affected inputs:** full-pinyin input longer than 32767 bytes, on both
  facades and every scheme, and pinyin import readings and
  `gen_pinyin_table` lines of that length. Double-pinyin and chewing
  sessions longer than 32767 joined bytes now keep every key, as their pin
  parsers do. Up to 65535 bytes they then match the pin. Past 65535 bytes
  they do not: the pin's matrix places each key at its wrapped `guint16`
  begin, so its offsets, its key lookups at offsets 65536 and above, its
  candidates and its sentence differ from oxpinyin's, POLICY-ACCEPTED in
  [1501-guint16-key-positions](1501-guint16-key-positions.md). Products of the change: every product rule applies to the
  whole input on oxpinyin's own path. The validating `reduce_tokens`
  asserts (`phrase_large_table3.h:95`, policy rows 74 and 93) abort
  wherever that path meets them. In
  `combined-library-sixth-add-key-past-parse-bound-pinyin` oxpinyin now
  aborts in the sixth add (`pinyin.cpp:541`) and the pin in the next read
  (`:2664`); both die of SIGABRT after the same earlier work and with the
  same stderr, so that case's observations are unchanged.
- **Tests:** `contract-diff.py` cases, existing driver:
  - POLICY-ACCEPTED: `input-length-cap` (4096, 4097, 32766 and 32767
    agree; 32768 and 65537 differ), `d18-input-boundaries-pinyin`,
    `d18-input-boundaries-zhuyin`, `input-length-boundary-32768`,
    `input-length-boundary-65535`, `input-length-boundary-65536`.
  - Controls that MATCH: `input-length-boundary-32767`,
    `long-input-sentence` (4800 bytes).
  - Import cases, unchanged comparison (`sigabrt=True`):
    `combined-library-sixth-add-long-reading-pinyin` (70008 bytes; two
    keys on both sides, both abort in the add),
    `combined-library-sixth-add-key-past-parse-bound-pinyin` (as above),
    `combined-library-sixth-add-long-reading-zhuyin` (unbounded on both).
  - Unit tests: `an_input_past_the_pins_gint16_steps_is_parsed_whole`
    (`oxpinyin-core`), `input_past_the_pins_gint16_bound_is_kept_and_parsed_whole`
    (`oxpinyin-engine`).
- **Prior rulings:** policy row 59 (#540) is reopened by the maintainer's
  2026-11-01 UTC decision in #1501, not reversed by this record.
- **Evidence:** the digests below come from `just policy-record` over a
  tkrzw, bdb and kc `--observations` capture of the cases above, made on an
  ephemeral cloud host provisioned with `tools/cloud/setup.sh`:

  ```sh
  just runner contract-diff <cell> "$PINYIN_ORACLE_PREFIX" <capi.so> <zhuyin.so> -- --cases <case> --observations obs.jsonl
  just policy-record docs/findings/1501-gint16-input-cap.md obs.jsonl
  ```
- **Conclusion:** accepted. Maintainer decision 2026-11-01 UTC (#1501).

```contract-diff-policy
{
  "schema": 2,
  "record": "1501-gint16-input-cap",
  "status": "current",
  "disposition": "POLICY-ACCEPTED",
  "cases": {}
}
```
