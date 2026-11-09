# 1501-guint16-key-positions — key positions past byte 65535

- **Status:** accepted · 2026-11-01 UTC · issue #1501 · PR #1502
- **Disposition:** MATCH for the parsed length and the key-rest positions and length; POLICY-ACCEPTED for matrix placement and what follows from it.
- **Policy limb:** the maintainer's 2026-11-01 UTC ruling ([compatibility-policy.md](compatibility-policy.md)): the `guint16` wrap is contract (limb 1); placing keys at the wrapped begin is an explicit ruling exception, not (a), (b) or (c).
- **Pin:** libpinyin 2.11.92 at 074a2219 (fresh clone; `chewing_key.h` `7ec4dfb4`, `phonetic_key_matrix.cpp` `ab7e8796`); GLib/libc: not involved; facade: both; backend: all.
- **Pin derivation:** the double-pinyin and keyboard zhuyin parsers count in `int` and store positions into `ChewingKeyRest`'s `guint16`s (`pinyin_parser2.cpp:565`, `zhuyin_parser2.cpp:259`, `chewing_key.h:102-103`); `fill_matrix` appends each key at its wrapped begin (`phonetic_key_matrix.cpp:51-55`), so columns 65536 and above are empty and the getters answer `false` there (`pinyin.cpp:2930-2931`).
- **Rust counterpart:** the C adapters store real positions `as u16` (`crates/oxpinyin-capi/src/cursor.rs`, `crates/oxpinyin-zhuyin-capi/src/cursor.rs`); the facade's columns stay at real offsets.
- **Forward trace:** `pinyin_parse_more_double_pinyins` / `*_parse_more_chewings` → session key spans → `*_get_*_key_rest` and `*_key_rest_positions`/`_length`.
- **Backward trace:** only double-pinyin and keyboard chewing produce positions past 65535 at the pin; full pinyin stops at 32767 ([1501-gint16-input-cap](1501-gint16-input-cap.md)).
- **Input partition:** compositions up to 65535 bytes agree; only double-pinyin or chewing compositions longer than 65535 bytes differ.
- **Difference:** for `ni3` + 32769 × `ni` (double pinyin) or `su3` + 32769 × `su` (chewing), the crossing key at 65535 answers `(65535, 1)`, length 2, on both sides; at 65537 the pin answers `false` and oxpinyin `(1, 3)`, length 2.
- **Why accepted:** the ruling: a ring of 65536 columns puts syllables 65536 bytes apart in one column, which no IME composition reaches.
- **Contract preserved:** C ABI and layout unchanged; positions past 65535 reach the caller wrapped, as the pin's do.
- **Affected inputs:** double-pinyin and chewing compositions longer than 65535 bytes, on both facades.
- **Tests:** unit tests `oxpinyin_capi::cursor::preedit_key_tests::key_rest_positions_past_byte_65535_wrap` and `oxpinyin_zhuyin_capi::e2e_tests::key_rest_positions_past_byte_65535_wrap`. `contract-diff.py` recorded MATCH `key-position-past-65535/{double-pinyin,chewing-zhuyin}-crossing-key` and POLICY-ACCEPTED `key-position-past-65535/{double-pinyin,chewing-zhuyin}-past-key`.
- **Prior rulings:** none reversed; the 2026-11-01 UTC `gint16` decision stands.
- **Evidence:** the pin derivation above. On bdb, kc and tkrzw, at key position 65537 the pin answers `false` and oxpinyin answers `true` with the real key rest; the unit tests above hold oxpinyin's side.
- **Conclusion:** accepted. Maintainer ruling 2026-11-01 UTC (#1501).
