# Indexed full-pinyin parsing

`oxpinyin_core::parse_full_pinyin_index_with_options(input, options, index)`
accepts `&[u8]`, `OptionBits`, and the scheme index and returns
`Result<FullPinyinIndexParse, core::convert::Infallible>`. A rejected key
ends the parsed prefix; there are currently no error cases. `FORCE_TONE` rejects toneless keys only inside
`USE_TONE`, matching libpinyin 074a2219 `pinyin_parser2.cpp:176-190`.
The existing boolean `parse_full_pinyin_index` remains available with its
original signature and behavior, returning `FullPinyinIndexParse` directly. The facade forwards its complete option word.
