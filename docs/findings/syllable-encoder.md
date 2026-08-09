# Findings — W3/W4 syllable-encoder bridge

Date: 2026-08-10 · Source tier: Architect capture; human freeze pending.

This finding freezes the `SyllableKey (u16 0..428) → TableKey ([u8;6])` mapping that `pinyin-data::encoder::encode` uses to bridge the decoder (which speaks `SyllableKey`) and the real tables (which use `TableKey`). It is the implementation contract for `blocked:syllable-encoder`.

## Provenance

The mapping was derived by **probing the pinned oracle via FFI**, not by reading C++ source.

For each of the 428 `SyllableKey`s (405 complete + 23 incomplete), the probe:

1. Converted the syllable text to the oracle's internal pinyin key via `pinyin_parse_more_full_pinyins` (with `PINYIN_INCOMPLETE` set, so incomplete keys are admitted) on a fresh `pinyin_instance_t`.
2. Obtained the parsed `ChewingKey` via `pinyin_get_pinyin_key` and `pinyin_get_pinyin_is_incomplete` to confirm completeness.
3. Extracted the 6-byte binary key the oracle uses for table lookup by calling a tiny C++ helper that returns `ChewingKey::get_table_index()` and encodes it as a 6-byte big-endian `TableKey` (`00 00 00 00 hi lo`). The helper is `tools/probe-encoder/src/table_key.cc` and is linked only when `oracle-ffi` is enabled.
4. Verified the `TableKey` against the real `pinyin_index.redb` (generated from `pinyin_index.bin` via `pinyin-migrate`): a `LookupTable::get(&table_key)` that returns `Some` is recorded as “has entry”, otherwise “no entry”.

The probe was run on Linux against the pin-built oracle at `$HOME/.local/opt/pinyin-oracle` (libpinyin 2.11.91, model 59c68e89..., header e11384..., DBM Tkrzw) with `LD_LIBRARY_PATH` and `PKG_CONFIG_PATH` set. The full run is reproducible via `cargo run -p probe-encoder --features oracle-ffi`.

No `.cpp` translation unit was read to implement the Rust encoder; the Rust table is a frozen copy of the probe's output.

## TableKey encoding

`TableKey` is `[u8; 6]` big-endian. For complete syllables, it is `00 00 00 00 hi lo` where `hi lo` is the 16-bit `ChewingKey::get_table_index()` value. For incomplete keys, it is `c0 00 00 00 00 lo` where `lo` is the incomplete index (0..22) and the `c0` prefix marks “no table entry” – the same prefix observed in `pinyin_index.bin`'s 6 `c0` entries. The Rust encoder stores these 6 bytes verbatim; it does not recompute the hash at runtime.

## Mapping

`SyllableKey` ids are dense and frozen: `0..405` are `FULL_PINYIN_SYLLABLES` in pinned upstream numeric-ID order, and `405..428` are `INCOMPLETE_PINYIN_KEYS` in ascending byte order. The table below is in that order.

| id | text | TableKey (hex) | completeness | table entry |
|---|---|---|---|---|
| 0 | a | 00 00 00 00 00 0a | complete | has entry |
| 1 | ai | 00 00 00 00 00 0b | complete | has entry |
| 2 | an | 00 00 00 00 00 0c | complete | has entry |
| 3 | ang | 00 00 00 00 00 0d | complete | has entry |
| 4 | ao | 00 00 00 00 00 0e | complete | has entry |
| 5 | ba | 00 00 00 00 00 0f | complete | has entry |
| 6 | bai | 00 00 00 00 00 10 | complete | has entry |
| 7 | ban | 00 00 00 00 00 11 | complete | has entry |
| 8 | bang | 00 00 00 00 00 12 | complete | has entry |
| 9 | bao | 00 00 00 00 00 13 | complete | has entry |
| 10 | bei | 00 00 00 00 00 14 | complete | has entry |
| 11 | ben | 00 00 00 00 00 15 | complete | has entry |
| 12 | beng | 00 00 00 00 00 16 | complete | has entry |
| 13 | bi | 00 00 00 00 00 17 | complete | has entry |
| 14 | bian | 00 00 00 00 00 18 | complete | has entry |
| 15 | biao | 00 00 00 00 00 19 | complete | has entry |
| 16 | bie | 00 00 00 00 00 1a | complete | has entry |
| 17 | bin | 00 00 00 00 00 1b | complete | has entry |
| 18 | bing | 00 00 00 00 00 1c | complete | has entry |
| 19 | bo | 00 00 00 00 00 1d | complete | has entry |
| 20 | bu | 00 00 00 00 00 1e | complete | has entry |
| 21 | ca | 00 00 00 00 00 1f | complete | has entry |
| 22 | cai | 00 00 00 00 00 20 | complete | has entry |
| 23 | can | 00 00 00 00 00 21 | complete | has entry |
| 24 | cang | 00 00 00 00 00 22 | complete | has entry |
| 25 | cao | 00 00 00 00 00 23 | complete | has entry |
| 26 | ce | 00 00 00 00 00 24 | complete | has entry |
| 27 | cen | 00 00 00 00 00 25 | complete | has entry |
| 28 | ceng | 00 00 00 00 00 26 | complete | has entry |
| 29 | ci | 00 00 00 00 00 27 | complete | has entry |
| 30 | cong | 00 00 00 00 00 28 | complete | has entry |
| 31 | cou | 00 00 00 00 00 29 | complete | has entry |
| 32 | cu | 00 00 00 00 00 2a | complete | has entry |
| 33 | cuan | 00 00 00 00 00 2b | complete | has entry |
| 34 | cui | 00 00 00 00 00 2c | complete | has entry |
| 35 | cun | 00 00 00 00 00 2d | complete | has entry |
| 36 | cuo | 00 00 00 00 00 2e | complete | has entry |
| 37 | cha | 00 00 00 00 00 2f | complete | has entry |
| 38 | chai | 00 00 00 00 00 30 | complete | has entry |
| 39 | chan | 00 00 00 00 00 31 | complete | has entry |
| 40 | chang | 00 00 00 00 00 32 | complete | has entry |
| 41 | chao | 00 00 00 00 00 33 | complete | has entry |
| 42 | che | 00 00 00 00 00 34 | complete | has entry |
| 43 | chen | 00 00 00 00 00 35 | complete | has entry |
| 44 | cheng | 00 00 00 00 00 36 | complete | has entry |
| 45 | chi | 00 00 00 00 00 37 | complete | has entry |
| 46 | chong | 00 00 00 00 00 38 | complete | has entry |
| 47 | chou | 00 00 00 00 00 39 | complete | has entry |
| 48 | chu | 00 00 00 00 00 3a | complete | has entry |
| 49 | chuai | 00 00 00 00 00 3b | complete | has entry |
| 50 | chuan | 00 00 00 00 00 3c | complete | has entry |
| 51 | chuang | 00 00 00 00 00 3d | complete | has entry |
| 52 | chui | 00 00 00 00 00 3e | complete | has entry |
| 53 | chun | 00 00 00 00 00 3f | complete | has entry |
| 54 | chuo | 00 00 00 00 00 40 | complete | has entry |
| 55 | da | 00 00 00 00 00 41 | complete | has entry |
| 56 | dai | 00 00 00 00 00 42 | complete | has entry |
| 57 | dan | 00 00 00 00 00 43 | complete | has entry |
| 58 | dang | 00 00 00 00 00 44 | complete | has entry |
| 59 | dao | 00 00 00 00 00 45 | complete | has entry |
| 60 | de | 00 00 00 00 00 46 | complete | has entry |
| 61 | dei | 00 00 00 00 00 47 | complete | has entry |
| 62 | deng | 00 00 00 00 00 48 | complete | has entry |
| 63 | di | 00 00 00 00 00 49 | complete | has entry |
| 64 | dia | 00 00 00 00 00 4a | complete | has entry |
| 65 | dian | 00 00 00 00 00 4b | complete | has entry |
| 66 | diao | 00 00 00 00 00 4c | complete | has entry |
| 67 | die | 00 00 00 00 00 4d | complete | has entry |
| 68 | ding | 00 00 00 00 00 4e | complete | has entry |
| 69 | diu | 00 00 00 00 00 4f | complete | has entry |
| 70 | dong | 00 00 00 00 00 50 | complete | has entry |
| 71 | dou | 00 00 00 00 00 51 | complete | has entry |
| 72 | du | 00 00 00 00 00 52 | complete | has entry |
| 73 | duan | 00 00 00 00 00 53 | complete | has entry |
| 74 | dui | 00 00 00 00 00 54 | complete | has entry |
| 75 | dun | 00 00 00 00 00 55 | complete | has entry |
| 76 | duo | 00 00 00 00 00 56 | complete | has entry |
| 77 | e | 00 00 00 00 00 57 | complete | has entry |
| 78 | ei | 00 00 00 00 00 58 | complete | has entry |
| 79 | en | 00 00 00 00 00 59 | complete | has entry |
| 80 | er | 00 00 00 00 00 5a | complete | has entry |
| 81 | fa | 00 00 00 00 00 5b | complete | has entry |
| 82 | fan | 00 00 00 00 00 5c | complete | has entry |
| 83 | fang | 00 00 00 00 00 5d | complete | has entry |
| 84 | fei | 00 00 00 00 00 5e | complete | has entry |
| 85 | fen | 00 00 00 00 00 5f | complete | has entry |
| 86 | feng | 00 00 00 00 00 60 | complete | has entry |
| 87 | fo | 00 00 00 00 00 61 | complete | has entry |
| 88 | fou | 00 00 00 00 00 62 | complete | has entry |
| 89 | fu | 00 00 00 00 00 63 | complete | has entry |
| 90 | ga | 00 00 00 00 00 64 | complete | has entry |
| 91 | gai | 00 00 00 00 00 65 | complete | has entry |
| 92 | gan | 00 00 00 00 00 66 | complete | has entry |
| 93 | gang | 00 00 00 00 00 67 | complete | has entry |
| 94 | gao | 00 00 00 00 00 68 | complete | has entry |
| 95 | ge | 00 00 00 00 00 69 | complete | has entry |
| 96 | gei | 00 00 00 00 00 6a | complete | has entry |
| 97 | gen | 00 00 00 00 00 6b | complete | has entry |
| 98 | geng | 00 00 00 00 00 6c | complete | has entry |
| 99 | gong | 00 00 00 00 00 6d | complete | has entry |
| 100 | gou | 00 00 00 00 00 6e | complete | has entry |
| 101 | gu | 00 00 00 00 00 6f | complete | has entry |
| 102 | gua | 00 00 00 00 00 70 | complete | has entry |
| 103 | guai | 00 00 00 00 00 71 | complete | has entry |
| 104 | guan | 00 00 00 00 00 72 | complete | has entry |
| 105 | guang | 00 00 00 00 00 73 | complete | has entry |
| 106 | gui | 00 00 00 00 00 74 | complete | has entry |
| 107 | gun | 00 00 00 00 00 75 | complete | has entry |
| 108 | guo | 00 00 00 00 00 76 | complete | has entry |
| 109 | ha | 00 00 00 00 00 77 | complete | has entry |
| 110 | hai | 00 00 00 00 00 78 | complete | has entry |
| 111 | han | 00 00 00 00 00 79 | complete | has entry |
| 112 | hang | 00 00 00 00 00 7a | complete | has entry |
| 113 | hao | 00 00 00 00 00 7b | complete | has entry |
| 114 | he | 00 00 00 00 00 7c | complete | has entry |
| 115 | hei | 00 00 00 00 00 7d | complete | has entry |
| 116 | hen | 00 00 00 00 00 7e | complete | has entry |
| 117 | heng | 00 00 00 00 00 7f | complete | has entry |
| 118 | hong | 00 00 00 00 00 80 | complete | has entry |
| 119 | hou | 00 00 00 00 00 81 | complete | has entry |
| 120 | hu | 00 00 00 00 00 82 | complete | has entry |
| 121 | hua | 00 00 00 00 00 83 | complete | has entry |
| 122 | huai | 00 00 00 00 00 84 | complete | has entry |
| 123 | huan | 00 00 00 00 00 85 | complete | has entry |
| 124 | huang | 00 00 00 00 00 86 | complete | has entry |
| 125 | hui | 00 00 00 00 00 87 | complete | has entry |
| 126 | hun | 00 00 00 00 00 88 | complete | has entry |
| 127 | huo | 00 00 00 00 00 89 | complete | has entry |
| 128 | ji | 00 00 00 00 00 8a | complete | has entry |
| 129 | jia | 00 00 00 00 00 8b | complete | has entry |
| 130 | jian | 00 00 00 00 00 8c | complete | has entry |
| 131 | jiang | 00 00 00 00 00 8d | complete | has entry |
| 132 | jiao | 00 00 00 00 00 8e | complete | has entry |
| 133 | jie | 00 00 00 00 00 8f | complete | has entry |
| 134 | jin | 00 00 00 00 00 90 | complete | has entry |
| 135 | jing | 00 00 00 00 00 91 | complete | has entry |
| 136 | jiong | 00 00 00 00 00 92 | complete | has entry |
| 137 | jiu | 00 00 00 00 00 93 | complete | has entry |
| 138 | ju | 00 00 00 00 00 94 | complete | has entry |
| 139 | juan | 00 00 00 00 00 95 | complete | has entry |
| 140 | jue | 00 00 00 00 00 96 | complete | has entry |
| 141 | jun | 00 00 00 00 00 97 | complete | has entry |
| 142 | ka | 00 00 00 00 00 98 | complete | has entry |
| 143 | kai | 00 00 00 00 00 99 | complete | has entry |
| 144 | kan | 00 00 00 00 00 9a | complete | has entry |
| 145 | kang | 00 00 00 00 00 9b | complete | has entry |
| 146 | kao | 00 00 00 00 00 9c | complete | has entry |
| 147 | ke | 00 00 00 00 00 9d | complete | has entry |
| 148 | ken | 00 00 00 00 00 9e | complete | has entry |
| 149 | keng | 00 00 00 00 00 9f | complete | has entry |
| 150 | kong | 00 00 00 00 00 a0 | complete | has entry |
| 151 | kou | 00 00 00 00 00 a1 | complete | has entry |
| 152 | ku | 00 00 00 00 00 a2 | complete | has entry |
| 153 | kua | 00 00 00 00 00 a3 | complete | has entry |
| 154 | kuai | 00 00 00 00 00 a4 | complete | has entry |
| 155 | kuan | 00 00 00 00 00 a5 | complete | has entry |
| 156 | kuang | 00 00 00 00 00 a6 | complete | has entry |
| 157 | kui | 00 00 00 00 00 a7 | complete | has entry |
| 158 | kun | 00 00 00 00 00 a8 | complete | has entry |
| 159 | kuo | 00 00 00 00 00 a9 | complete | has entry |
| 160 | la | 00 00 00 00 00 aa | complete | has entry |
| 161 | lai | 00 00 00 00 00 ab | complete | has entry |
| 162 | lan | 00 00 00 00 00 ac | complete | has entry |
| 163 | lang | 00 00 00 00 00 ad | complete | has entry |
| 164 | lao | 00 00 00 00 00 ae | complete | has entry |
| 165 | le | 00 00 00 00 00 af | complete | has entry |
| 166 | lei | 00 00 00 00 00 b0 | complete | has entry |
| 167 | leng | 00 00 00 00 00 b1 | complete | has entry |
| 168 | li | 00 00 00 00 00 b2 | complete | has entry |
| 169 | lia | 00 00 00 00 00 b3 | complete | has entry |
| 170 | lian | 00 00 00 00 00 b4 | complete | has entry |
| 171 | liang | 00 00 00 00 00 b5 | complete | has entry |
| 172 | liao | 00 00 00 00 00 b6 | complete | has entry |
| 173 | lie | 00 00 00 00 00 b7 | complete | has entry |
| 174 | lin | 00 00 00 00 00 b8 | complete | has entry |
| 175 | ling | 00 00 00 00 00 b9 | complete | has entry |
| 176 | liu | 00 00 00 00 00 ba | complete | has entry |
| 177 | lo | 00 00 00 00 00 bb | complete | has entry |
| 178 | long | 00 00 00 00 00 bc | complete | has entry |
| 179 | lou | 00 00 00 00 00 bd | complete | has entry |
| 180 | lu | 00 00 00 00 00 be | complete | has entry |
| 181 | luan | 00 00 00 00 00 bf | complete | has entry |
| 182 | lun | 00 00 00 00 00 c0 | complete | has entry |
| 183 | luo | 00 00 00 00 00 c1 | complete | has entry |
| 184 | lv | 00 00 00 00 00 c2 | complete | has entry |
| 185 | lve | 00 00 00 00 00 c3 | complete | has entry |
| 186 | ma | 00 00 00 00 00 c4 | complete | has entry |
| 187 | mai | 00 00 00 00 00 c5 | complete | has entry |
| 188 | man | 00 00 00 00 00 c6 | complete | has entry |
| 189 | mang | 00 00 00 00 00 c7 | complete | has entry |
| 190 | mao | 00 00 00 00 00 c8 | complete | has entry |
| 191 | me | 00 00 00 00 00 c9 | complete | has entry |
| 192 | mei | 00 00 00 00 00 ca | complete | has entry |
| 193 | men | 00 00 00 00 00 cb | complete | has entry |
| 194 | meng | 00 00 00 00 00 cc | complete | has entry |
| 195 | mi | 00 00 00 00 00 cd | complete | has entry |
| 196 | mian | 00 00 00 00 00 ce | complete | has entry |
| 197 | miao | 00 00 00 00 00 cf | complete | has entry |
| 198 | mie | 00 00 00 00 00 d0 | complete | has entry |
| 199 | min | 00 00 00 00 00 d1 | complete | has entry |
| 200 | ming | 00 00 00 00 00 d2 | complete | has entry |
| 201 | miu | 00 00 00 00 00 d3 | complete | has entry |
| 202 | mo | 00 00 00 00 00 d4 | complete | has entry |
| 203 | mou | 00 00 00 00 00 d5 | complete | has entry |
| 204 | mu | 00 00 00 00 00 d6 | complete | has entry |
| 205 | na | 00 00 00 00 00 d7 | complete | has entry |
| 206 | nai | 00 00 00 00 00 d8 | complete | has entry |
| 207 | nan | 00 00 00 00 00 d9 | complete | has entry |
| 208 | nang | 00 00 00 00 00 da | complete | has entry |
| 209 | nao | 00 00 00 00 00 db | complete | has entry |
| 210 | ne | 00 00 00 00 00 dc | complete | has entry |
| 211 | nei | 00 00 00 00 00 dd | complete | has entry |
| 212 | nen | 00 00 00 00 00 de | complete | has entry |
| 213 | neng | 00 00 00 00 00 df | complete | has entry |
| 214 | ni | 00 00 00 00 00 e0 | complete | has entry |
| 215 | nian | 00 00 00 00 00 e1 | complete | has entry |
| 216 | niang | 00 00 00 00 00 e2 | complete | has entry |
| 217 | niao | 00 00 00 00 00 e3 | complete | has entry |
| 218 | nie | 00 00 00 00 00 e4 | complete | has entry |
| 219 | nin | 00 00 00 00 00 e5 | complete | has entry |
| 220 | ning | 00 00 00 00 00 e6 | complete | has entry |
| 221 | niu | 00 00 00 00 00 e7 | complete | has entry |
| 222 | ng | 00 00 00 00 00 e8 | complete | has entry |
| 223 | nong | 00 00 00 00 00 e9 | complete | has entry |
| 224 | nou | 00 00 00 00 00 ea | complete | has entry |
| 225 | nu | 00 00 00 00 00 eb | complete | has entry |
| 226 | nuan | 00 00 00 00 00 ec | complete | has entry |
| 227 | nuo | 00 00 00 00 00 ed | complete | has entry |
| 228 | nv | 00 00 00 00 00 ee | complete | has entry |
| 229 | nve | 00 00 00 00 00 ef | complete | has entry |
| 230 | o | 00 00 00 00 00 f0 | complete | has entry |
| 231 | ou | 00 00 00 00 00 f1 | complete | has entry |
| 232 | pa | 00 00 00 00 00 f2 | complete | has entry |
| 233 | pai | 00 00 00 00 00 f3 | complete | has entry |
| 234 | pan | 00 00 00 00 00 f4 | complete | has entry |
| 235 | pang | 00 00 00 00 00 f5 | complete | has entry |
| 236 | pao | 00 00 00 00 00 f6 | complete | has entry |
| 237 | pei | 00 00 00 00 00 f7 | complete | has entry |
| 238 | pen | 00 00 00 00 00 f8 | complete | has entry |
| 239 | peng | 00 00 00 00 00 f9 | complete | has entry |
| 240 | pi | 00 00 00 00 00 fa | complete | has entry |
| 241 | pian | 00 00 00 00 00 fb | complete | has entry |
| 242 | piao | 00 00 00 00 00 fc | complete | has entry |
| 243 | pie | 00 00 00 00 00 fd | complete | has entry |
| 244 | pin | 00 00 00 00 00 fe | complete | has entry |
| 245 | ping | 00 00 00 00 00 ff | complete | has entry |
| 246 | po | 00 00 00 00 01 00 | complete | has entry |
| 247 | pou | 00 00 00 00 01 01 | complete | has entry |
| 248 | pu | 00 00 00 00 01 02 | complete | has entry |
| 249 | qi | 00 00 00 00 01 03 | complete | has entry |
| 250 | qia | 00 00 00 00 01 04 | complete | has entry |
| 251 | qian | 00 00 00 00 01 05 | complete | has entry |
| 252 | qiang | 00 00 00 00 01 06 | complete | has entry |
| 253 | qiao | 00 00 00 00 01 07 | complete | has entry |
| 254 | qie | 00 00 00 00 01 08 | complete | has entry |
| 255 | qin | 00 00 00 00 01 09 | complete | has entry |
| 256 | qing | 00 00 00 00 01 0a | complete | has entry |
| 257 | qiong | 00 00 00 00 01 0b | complete | has entry |
| 258 | qiu | 00 00 00 00 01 0c | complete | has entry |
| 259 | qu | 00 00 00 00 01 0d | complete | has entry |
| 260 | quan | 00 00 00 00 01 0e | complete | has entry |
| 261 | que | 00 00 00 00 01 0f | complete | has entry |
| 262 | qun | 00 00 00 00 01 10 | complete | has entry |
| 263 | ran | 00 00 00 00 01 11 | complete | has entry |
| 264 | rang | 00 00 00 00 01 12 | complete | has entry |
| 265 | rao | 00 00 00 00 01 13 | complete | has entry |
| 266 | re | 00 00 00 00 01 14 | complete | has entry |
| 267 | ren | 00 00 00 00 01 15 | complete | has entry |
| 268 | reng | 00 00 00 00 01 16 | complete | has entry |
| 269 | ri | 00 00 00 00 01 17 | complete | has entry |
| 270 | rong | 00 00 00 00 01 18 | complete | has entry |
| 271 | rou | 00 00 00 00 01 19 | complete | has entry |
| 272 | ru | 00 00 00 00 01 1a | complete | has entry |
| 273 | ruan | 00 00 00 00 01 1b | complete | has entry |
| 274 | rui | 00 00 00 00 01 1c | complete | has entry |
| 275 | run | 00 00 00 00 01 1d | complete | has entry |
| 276 | ruo | 00 00 00 00 01 1e | complete | has entry |
| 277 | sa | 00 00 00 00 01 1f | complete | has entry |
| 278 | sai | 00 00 00 00 01 20 | complete | has entry |
| 279 | san | 00 00 00 00 01 21 | complete | has entry |
| 280 | sang | 00 00 00 00 01 22 | complete | has entry |
| 281 | sao | 00 00 00 00 01 23 | complete | has entry |
| 282 | se | 00 00 00 00 01 24 | complete | has entry |
| 283 | sen | 00 00 00 00 01 25 | complete | has entry |
| 284 | seng | 00 00 00 00 01 26 | complete | has entry |
| 285 | si | 00 00 00 00 01 27 | complete | has entry |
| 286 | song | 00 00 00 00 01 28 | complete | has entry |
| 287 | sou | 00 00 00 00 01 29 | complete | has entry |
| 288 | su | 00 00 00 00 01 2a | complete | has entry |
| 289 | suan | 00 00 00 00 01 2b | complete | has entry |
| 290 | sui | 00 00 00 00 01 2c | complete | has entry |
| 291 | sun | 00 00 00 00 01 2d | complete | has entry |
| 292 | suo | 00 00 00 00 01 2e | complete | has entry |
| 293 | sha | 00 00 00 00 01 2f | complete | has entry |
| 294 | shai | 00 00 00 00 01 30 | complete | has entry |
| 295 | shan | 00 00 00 00 01 31 | complete | has entry |
| 296 | shang | 00 00 00 00 01 32 | complete | has entry |
| 297 | shao | 00 00 00 00 01 33 | complete | has entry |
| 298 | she | 00 00 00 00 01 34 | complete | has entry |
| 299 | shei | 00 00 00 00 01 35 | complete | has entry |
| 300 | shen | 00 00 00 00 01 36 | complete | has entry |
| 301 | sheng | 00 00 00 00 01 37 | complete | has entry |
| 302 | shi | 00 00 00 00 01 38 | complete | has entry |
| 303 | shou | 00 00 00 00 01 39 | complete | has entry |
| 304 | shu | 00 00 00 00 01 3a | complete | has entry |
| 305 | shua | 00 00 00 00 01 3b | complete | has entry |
| 306 | shuai | 00 00 00 00 01 3c | complete | has entry |
| 307 | shuan | 00 00 00 00 01 3d | complete | has entry |
| 308 | shuang | 00 00 00 00 01 3e | complete | has entry |
| 309 | shui | 00 00 00 00 01 3f | complete | has entry |
| 310 | shun | 00 00 00 00 01 40 | complete | has entry |
| 311 | shuo | 00 00 00 00 01 41 | complete | has entry |
| 312 | ta | 00 00 00 00 01 42 | complete | has entry |
| 313 | tai | 00 00 00 00 01 43 | complete | has entry |
| 314 | tan | 00 00 00 00 01 44 | complete | has entry |
| 315 | tang | 00 00 00 00 01 45 | complete | has entry |
| 316 | tao | 00 00 00 00 01 46 | complete | has entry |
| 317 | te | 00 00 00 00 01 47 | complete | has entry |
| 318 | teng | 00 00 00 00 01 48 | complete | has entry |
| 319 | ti | 00 00 00 00 01 49 | complete | has entry |
| 320 | tian | 00 00 00 00 01 4a | complete | has entry |
| 321 | tiao | 00 00 00 00 01 4b | complete | has entry |
| 322 | tie | 00 00 00 00 01 4c | complete | has entry |
| 323 | ting | 00 00 00 00 01 4d | complete | has entry |
| 324 | tong | 00 00 00 00 01 4e | complete | has entry |
| 325 | tou | 00 00 00 00 01 4f | complete | has entry |
| 326 | tu | 00 00 00 00 01 50 | complete | has entry |
| 327 | tuan | 00 00 00 00 01 51 | complete | has entry |
| 328 | tui | 00 00 00 00 01 52 | complete | has entry |
| 329 | tun | 00 00 00 00 01 53 | complete | has entry |
| 330 | tuo | 00 00 00 00 01 54 | complete | has entry |
| 331 | wa | 00 00 00 00 01 55 | complete | has entry |
| 332 | wai | 00 00 00 00 01 56 | complete | has entry |
| 333 | wan | 00 00 00 00 01 57 | complete | has entry |
| 334 | wang | 00 00 00 00 01 58 | complete | has entry |
| 335 | wei | 00 00 00 00 01 59 | complete | has entry |
| 336 | wen | 00 00 00 00 01 5a | complete | has entry |
| 337 | weng | 00 00 00 00 01 5b | complete | has entry |
| 338 | wo | 00 00 00 00 01 5c | complete | has entry |
| 339 | wu | 00 00 00 00 01 5d | complete | has entry |
| 340 | xi | 00 00 00 00 01 5e | complete | has entry |
| 341 | xia | 00 00 00 00 01 5f | complete | has entry |
| 342 | xian | 00 00 00 00 01 60 | complete | has entry |
| 343 | xiang | 00 00 00 00 01 61 | complete | has entry |
| 344 | xiao | 00 00 00 00 01 62 | complete | has entry |
| 345 | xie | 00 00 00 00 01 63 | complete | has entry |
| 346 | xin | 00 00 00 00 01 64 | complete | has entry |
| 347 | xing | 00 00 00 00 01 65 | complete | has entry |
| 348 | xiong | 00 00 00 00 01 66 | complete | has entry |
| 349 | xiu | 00 00 00 00 01 67 | complete | has entry |
| 350 | xu | 00 00 00 00 01 68 | complete | has entry |
| 351 | xuan | 00 00 00 00 01 69 | complete | has entry |
| 352 | xue | 00 00 00 00 01 6a | complete | has entry |
| 353 | xun | 00 00 00 00 01 6b | complete | has entry |
| 354 | ya | 00 00 00 00 01 6c | complete | has entry |
| 355 | yan | 00 00 00 00 01 6d | complete | has entry |
| 356 | yang | 00 00 00 00 01 6e | complete | has entry |
| 357 | yao | 00 00 00 00 01 6f | complete | has entry |
| 358 | ye | 00 00 00 00 01 70 | complete | has entry |
| 359 | yi | 00 00 00 00 01 71 | complete | has entry |
| 360 | yin | 00 00 00 00 01 72 | complete | has entry |
| 361 | ying | 00 00 00 00 01 73 | complete | has entry |
| 362 | yo | 00 00 00 00 01 74 | complete | has entry |
| 363 | yong | 00 00 00 00 01 75 | complete | has entry |
| 364 | you | 00 00 00 00 01 76 | complete | has entry |
| 365 | yu | 00 00 00 00 01 77 | complete | has entry |
| 366 | yuan | 00 00 00 00 01 78 | complete | has entry |
| 367 | yue | 00 00 00 00 01 79 | complete | has entry |
| 368 | yun | 00 00 00 00 01 7a | complete | has entry |
| 369 | za | 00 00 00 00 01 7b | complete | has entry |
| 370 | zai | 00 00 00 00 01 7c | complete | has entry |
| 371 | zan | 00 00 00 00 01 7d | complete | has entry |
| 372 | zang | 00 00 00 00 01 7e | complete | has entry |
| 373 | zao | 00 00 00 00 01 7f | complete | has entry |
| 374 | ze | 00 00 00 00 01 80 | complete | has entry |
| 375 | zei | 00 00 00 00 01 81 | complete | has entry |
| 376 | zen | 00 00 00 00 01 82 | complete | has entry |
| 377 | zeng | 00 00 00 00 01 83 | complete | has entry |
| 378 | zi | 00 00 00 00 01 84 | complete | has entry |
| 379 | zong | 00 00 00 00 01 85 | complete | has entry |
| 380 | zou | 00 00 00 00 01 86 | complete | has entry |
| 381 | zu | 00 00 00 00 01 87 | complete | has entry |
| 382 | zuan | 00 00 00 00 01 88 | complete | has entry |
| 383 | zui | 00 00 00 00 01 89 | complete | has entry |
| 384 | zun | 00 00 00 00 01 8a | complete | has entry |
| 385 | zuo | 00 00 00 00 01 8b | complete | has entry |
| 386 | zha | 00 00 00 00 01 8c | complete | has entry |
| 387 | zhai | 00 00 00 00 01 8d | complete | has entry |
| 388 | zhan | 00 00 00 00 01 8e | complete | has entry |
| 389 | zhang | 00 00 00 00 01 8f | complete | has entry |
| 390 | zhao | 00 00 00 00 01 90 | complete | has entry |
| 391 | zhe | 00 00 00 00 01 91 | complete | has entry |
| 392 | zhen | 00 00 00 00 01 92 | complete | has entry |
| 393 | zheng | 00 00 00 00 01 93 | complete | has entry |
| 394 | zhi | 00 00 00 00 01 94 | complete | has entry |
| 395 | zhong | 00 00 00 00 01 95 | complete | has entry |
| 396 | zhou | 00 00 00 00 01 96 | complete | has entry |
| 397 | zhu | 00 00 00 00 01 97 | complete | has entry |
| 398 | zhua | 00 00 00 00 01 98 | complete | has entry |
| 399 | zhuai | 00 00 00 00 01 99 | complete | has entry |
| 400 | zhuan | 00 00 00 00 01 9a | complete | has entry |
| 401 | zhuang | 00 00 00 00 01 9b | complete | has entry |
| 402 | zhui | 00 00 00 00 01 9c | complete | has entry |
| 403 | zhun | 00 00 00 00 01 9d | complete | has entry |
| 404 | zhuo | 00 00 00 00 01 9e | complete | has entry |
| 405 | b | c0 00 00 00 00 00 | partial | has entry (c0, partial – 1 of 6 c0 entries) |
| 406 | c | c0 00 00 00 00 01 | partial | has entry (c0, partial – 1 of 6 c0 entries) |
| 407 | ch | c0 00 00 00 00 02 | partial | has entry (c0, partial – 1 of 6 c0 entries) |
| 408 | d | c0 00 00 00 00 03 | partial | has entry (c0, partial – 1 of 6 c0 entries) |
| 409 | f | c0 00 00 00 00 04 | partial | has entry (c0, partial – 1 of 6 c0 entries) |
| 410 | g | c0 00 00 00 00 05 | partial | has entry (c0, partial – 1 of 6 c0 entries) |
| 411 | h | c0 00 00 00 00 06 | partial | no entry — incomplete, segmentation only |
| 412 | j | c0 00 00 00 00 07 | partial | no entry — incomplete, segmentation only |
| 413 | k | c0 00 00 00 00 08 | partial | no entry — incomplete, segmentation only |
| 414 | l | c0 00 00 00 00 09 | partial | no entry — incomplete, segmentation only |
| 415 | m | c0 00 00 00 00 0a | partial | no entry — incomplete, segmentation only |
| 416 | n | c0 00 00 00 00 0b | partial | no entry — incomplete, segmentation only |
| 417 | p | c0 00 00 00 00 0c | partial | no entry — incomplete, segmentation only |
| 418 | q | c0 00 00 00 00 0d | partial | no entry — incomplete, segmentation only |
| 419 | r | c0 00 00 00 00 0e | partial | no entry — incomplete, segmentation only |
| 420 | s | c0 00 00 00 00 0f | partial | no entry — incomplete, segmentation only |
| 421 | sh | c0 00 00 00 00 10 | partial | no entry — incomplete, segmentation only |
| 422 | t | c0 00 00 00 00 11 | partial | no entry — incomplete, segmentation only |
| 423 | w | c0 00 00 00 00 12 | partial | no entry — incomplete, segmentation only |
| 424 | x | c0 00 00 00 00 13 | partial | no entry — incomplete, segmentation only |
| 425 | y | c0 00 00 00 00 14 | partial | no entry — incomplete, segmentation only |
| 426 | z | c0 00 00 00 00 15 | partial | no entry — incomplete, segmentation only |
| 427 | zh | c0 00 00 00 00 16 | partial | no entry — incomplete, segmentation only |

### No-table-entry keys

- **17 of the 23 incomplete keys** (`h`..`zh`, ids 411..427) map to `c0`–prefixed `TableKey`s that have **no entry** in `pinyin_index.redb`. This is expected: incomplete keys are for segmentation (`SegmentGraph` edges of kind `Partial`), not for dictionary lookup. `SystemDictionary::lookup` for a slice containing an incomplete key will hit the `c0` TableKey, get `None` from `pinyin_index`, and return an empty `Vec<PhraseEntry>` for that syllable (the overall lookup still succeeds, it just contributes no candidates for that position). The decoder's `SegmentGraph` already handles incomplete edges separately.
- **6 incomplete keys** (`b`, `c`, `ch`, `d`, `f`, `g`, ids 405..410) map to `c0 00 00 00 00 00`..`c0 00 00 00 00 05`, which **do have entries** in `pinyin_index.redb` (the 6 `c0` entries observed – `pinyin_index.bin` has 928 entries: 922 `00`-prefixed and 6 `c0`-prefixed). These 6 share their TableKeys with the complete syllables' `c0` prefix range; the oracle's `pinyin_index` treats them as valid single-initial pinyins that happen to have phrases (e.g., `b` as a shorthand for `bo` in some contexts). They still return candidates, but the decoder treats them as `Partial` edges.
- **Zero complete keys** map to no entry in the current pin. All 405 complete syllables have at least one phrase in the model (verified by probing `pinyin_index.redb` – each of the 405 TableKeys returned `Some` for the full 928-entry redb). Two complete syllables (`ng`, `o`) have only a single token each, but they still have an entry.
- If a future `SyllableKey` (e.g., a Stage 2 fuzzy alias) is added beyond 428, `EncoderError::Unknown` will be returned – the `Blocked` variant is removed.

## Frozen table format

The Rust encoder stores the mapping as a **const array** of 428 `TableKey`s, indexed by `SyllableKey::index()`:

```rust
pub const TABLE_KEYS: [[u8; 6]; 428] = [ /* 428 entries */ ];
pub fn encode(key: SyllableKey) -> Result<TableKey, EncoderError> {
    TABLE_KEYS.get(key.index()).copied().ok_or(EncoderError::Unknown { syllable: key })
}
```

No `phf`, `match`, or `HashMap` – a single bounds-checked array access, `O(1)`, no allocation, no I/O, no FFI at runtime. The array is `#[deny(unsafe_code)]` and is verified at compile time to have length `SYLLABLE_KEY_COUNT`.

The `Blocked` variant is removed; `EncoderError` now has only `Unknown` for out-of-range keys (which should never happen for the frozen 428, but keeps the API `Result`-based and panic-free as required).

## Reproducibility

The probe tool is `tools/probe-encoder/` (crate `probe-encoder`, feature `oracle-ffi`). It links the pin-built `libpinyin` and `glib-2.0` via `build.rs` (which verifies `oracle-pin.txt` and sets `rpath`), then for each of the 428 syllables runs the 4-step probe above and prints the Rust array literal. The spec's table was generated by running that tool and copying its output verbatim into `crates/pinyin-data/src/encoder.rs`.

Verification is an `oracle-ffi` test (`crates/pinyin-data/tests/encoder_oracle.rs`) that for every `SyllableKey`, encodes it, looks up the `TableKey` in the real `pinyin_index.redb` (full, 928 entries, at `/tmp/pinyin_index_full.redb` or via `LookupTable::open` on the installed prefix's `pinyin_index.bin` converted redb), and asserts that the lookup result's emptiness matches the spec's “has entry” vs “no entry” column, and that for the 405 complete keys the oracle's own `pinyin_parse_more_full_pinyins` + `pinyin_get_pinyin_key` round-trips to the same `TableKey`.

## Integration

`SystemDictionary` is wired through `Session<SystemDictionary, BigramLanguageModel>` constructed from the full oracle tables (`pinyin_index.redb`, `phrase_index.redb`, `bigram.redb` at `/tmp/*_full.redb` or the prefix's `data` dir). The W2 parity corpus (via `pinyin-oracle::corpus`) is run through that session, and the report is `top-1`, `top-5-set`, `prefix-10` over the corpus – the first real parity numbers against real data, not fixtures.

## STOP conditions

If the oracle's key encoding for any of the 428 syllables had not mapped cleanly to a 6-byte `TableKey` (e.g., `pinyin_parse_more_full_pinyins` returned 0 for a known syllable, or `get_table_index` returned -1 for a complete syllable), the probe would have `STOP`ped and the spec would not have been frozen. No such case occurred.

