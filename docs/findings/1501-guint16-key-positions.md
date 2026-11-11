# 1501-guint16-key-positions — key positions past byte 65535

- **Status:** accepted · 2026-11-01 UTC · issue #1501 · PR #1502
- **Disposition:** MATCH for the parsed length and the key-rest positions and length; POLICY-ACCEPTED for matrix placement and what follows from it.
- **Policy limb:** the maintainer's 2026-11-01 UTC ruling ([compatibility-policy.md](compatibility-policy.md#key-positions-past-byte-65535)). The `guint16` wrap is contract (limb 1). The placement of keys at the wrapped begin is an explicit ruling exception, not (a), (b) or (c).
- **Pin:** libpinyin 2.11.92 at 074a2219 (fresh clone; `chewing_key.h` `7ec4dfb4`, `phonetic_key_matrix.cpp` `ab7e8796`); GLib/libc: not involved; facade: both; backend: all.
- **Pin derivation:** the double-pinyin and keyboard zhuyin parsers count in `int`. They store positions into the `guint16`s of `ChewingKeyRest` (`pinyin_parser2.cpp:565`, `zhuyin_parser2.cpp:259`, `chewing_key.h:102-103`). `fill_matrix` appends each key at its wrapped begin (`phonetic_key_matrix.cpp:51-55`). Thus columns 65536 and above are empty, and the getters return `false` there (`pinyin.cpp:2930-2931`).
- **Rust counterpart:** the C adapters store real positions `as u16` (`crates/oxpinyin-capi/src/cursor.rs`, `crates/oxpinyin-zhuyin-capi/src/cursor.rs`). The columns of the facade stay at real offsets.
- **Forward trace:** `pinyin_parse_more_double_pinyins` / `*_parse_more_chewings` → session key spans → `*_get_*_key_rest` and `*_key_rest_positions`/`_length`.
- **Backward trace:** only double-pinyin and keyboard chewing produce positions past 65535 at the pin. Full pinyin stops at 32767 ([1501-gint16-input-cap](1501-gint16-input-cap.md)).
- **Input partition:** compositions up to 65535 bytes agree. Only double-pinyin or chewing compositions longer than 65535 bytes differ.
- **Difference:** the input is `ni3` + 32769 × `ni` (double pinyin) or `su3` + 32769 × `su` (chewing). On both sides, the crossing key at 65535 returns `(65535, 1)`, length 2. At 65537, the pin returns `false` and oxpinyin returns `(1, 3)`, length 2.
- **Why accepted:** the ruling. A ring of 65536 columns puts syllables 65536 bytes apart in one column, and no IME composition reaches that length.
- **Contract preserved:** C ABI and layout are unchanged. Positions past 65535 reach the caller wrapped, as the pin's positions do.
- **Affected inputs:** double-pinyin and chewing compositions longer than 65535 bytes, on both facades.
- **Tests:** unit tests `oxpinyin_capi::cursor::preedit_key_tests::key_rest_positions_past_byte_65535_wrap` and `oxpinyin_zhuyin_capi::e2e_tests::key_rest_positions_past_byte_65535_wrap`. Recorded [archived contract diff source](https://github.com/shenghaoc/oxpinyin/blob/1b91c898795b6edd8b92dd1fecf65ec6f4d1a434/tools/bisection/contract-diff.py) evidence: MATCH `key-position-past-65535/{double-pinyin,chewing-zhuyin}-crossing-key` and POLICY-ACCEPTED `key-position-past-65535/{double-pinyin,chewing-zhuyin}-past-key`.
- **Prior rulings:** none reversed; the 2026-11-01 UTC `gint16` decision stands.
- **Evidence:** the pin derivation above. On bdb, kc and tkrzw, at key position 65537 the pin answers `false` and oxpinyin answers `true` with the real key rest. The unit tests above hold oxpinyin's side.
- **Conclusion:** accepted. Maintainer ruling 2026-11-01 UTC (#1501).
