# 1501-gint16-input-cap — the full-pinyin parse's 16-bit steps

- **Status:** accepted · 2026-11-01 UTC · issue #1501 · PR #1502
- **Disposition:** POLICY-ACCEPTED (never MATCH)
- **Policy limb:** language artefact (limb 3; the 2026-10-29 UTC "Contract, not implementation" ruling): the stop is a storage width; not (a), (b) or (c). Maintainer decision 2026-11-01 UTC in #1501.
- **Pin:** libpinyin 2.11.92 at 074a2219 (fresh clone; `pinyin_parser2.cpp` blob `47c58761`); GLib/libc: not involved; facade: both; backend: all.
- **Pin derivation:** `parse_value_t` keeps its steps in `gint16` (`pinyin_parser2.cpp:67-70`). `final_step` returns the last step whose sign-extended `m_parsed_len` equals its index (`:353-357`). That step is never above 32767. `parse` returns it (`:333`).
- **Rust counterpart:** `oxpinyin_core::graph::SegmentGraph`, `oxpinyin_engine::Session` (`session/buffer.rs`), `parse_import_pinyin` (`crates/oxpinyin-capi/src/iterators.rs`).
- **Forward trace:** `*_parse_more_full_pinyins` → `Session::replace_raw` → the graph → the parsed length, keys, candidates; `pinyin_iterator_add_phrase` → `parse_import_pinyin` → `_add_phrase` (`pinyin.cpp:541`).
- **Backward trace:** only `FullPinyinParser2` has 16-bit steps (both `parse_more_full_pinyins` entries, `pinyin.cpp:631-637`, `gen_pinyin_table.cpp:176-181`). The double-pinyin and zhuyin parsers count in `int`.
- **Input partition:** input up to 32767 bytes agrees. Only full-pinyin input or import readings longer than 32767 bytes differ.
- **Difference:** past 32767 bytes, the pin parses the first 32767 and oxpinyin parses the whole input. oxpinyin has no `MAX_INPUT_BYTES` clamp, import cut or `MAX_GRAPH_INPUT` refusal.
- **Why accepted:** the stop is a C++ storage width. oxpinyin keeps graph positions and candidate spans in `u32`, and refuses only past `MAX_GRAPH_POSITION` (`u32::MAX - 1` bytes).
- **Contract preserved:** C ABI, layout, ownership and on-disk formats are unchanged. An over-long import reading stores its full key list.
- **Affected inputs:** full-pinyin input, pinyin import readings and `gen_pinyin_table` lines longer than 32767 bytes, on both facades and every scheme.
- **Tests:** unit tests `oxpinyin_engine::session::tests::input_past_the_pins_gint16_bound_is_kept_and_parsed_whole`, `oxpinyin_engine::session::tests::candidates_come_from_keys_past_the_pins_gint16_bound` and `oxpinyin_capi::boundary_tests::an_import_reading_parses_past_long_separator_runs`. Recorded [archived contract diff source](https://github.com/shenghaoc/oxpinyin/blob/1b91c898795b6edd8b92dd1fecf65ec6f4d1a434/tools/bisection/contract-diff.py) evidence: POLICY-ACCEPTED `input-length-boundary-32768` and `input-length-boundary-65536`; MATCH control `input-length-boundary-32767`; `combined-library-sixth-add-long-reading-pinyin` unchanged.
- **Prior rulings:** none reversed; policy row 59 (#540) is reopened.
- **Evidence:** the pin derivation above. On bdb, kc and tkrzw, `input-length-boundary-32768` parses 32767 bytes at the pin and 32768 in oxpinyin, and `input-length-boundary-65536` parses 32767 and 65536. The unit tests above hold oxpinyin's side.
- **Conclusion:** accepted. Maintainer decision 2026-11-01 UTC (#1501).
