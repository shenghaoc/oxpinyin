# A SYSTEM_FILE above the stock system indices

The default table can name a `SYSTEM_FILE` above index 4. The pin selects the
load path by `m_file_type`, not by a fixed index (`src/pinyin.cpp:234-277`,
`src/zhuyin.cpp:178-221` at `074a2219`). Import selects the requested
sub-index (`pinyin.cpp:532-568`, `zhuyin.cpp:419-455`).

On the tkrzw reference cell, `high-system-library-{pinyin,zhuyin}` replace
the NETWORK_DICTIONARY row with a `SYSTEM_FILE` at index 6 backed by `art.bin`.
Against `origin/main` `5755bb48`, both cases DIFFER: the pin reads the first
item and accepts an import into the library; main answers no item and refuses
the import. Training and save/reopen of that imported item cannot be reached
on main. This difference predates the stage 2 mapping stack.

Member B moves the library's mapping into the live sub-index and selects its
consumers by the configured file type. The same cases cover item and unigram
lookup, import, training, a second import, save and reopen in both facades;
pinyin additionally covers phrase export. They MATCH on tkrzw. The module-local test
`oxpinyin_user::live_system::tests::a_nonstock_loaded_library_is_a_system_library`
holds configured-library classification and reads outside the stock range.
