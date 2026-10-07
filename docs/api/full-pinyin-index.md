# Indexed full-pinyin parsing

`oxpinyin_core::parse_full_pinyin_index_with_options(input, options, index)`
accepts `&[u8]`, `OptionBits`, and the scheme index and returns
`FullPinyinIndexParse`. `FORCE_TONE` rejects toneless keys only inside
`USE_TONE`, matching libpinyin 074a2219 `pinyin_parser2.cpp:176-190`.
The existing boolean `parse_full_pinyin_index` remains available with its
original behavior. The facade forwards its complete option word.
