# Matrix split tables SPEC

Date: 2026-08-14 · Status: **frozen** (amended by #716, maintainer approval 2026-10-10)

What binds: the two split lists below and the semantics by which they add keys to the matrix, as derived from the pin and observable through the C APIs ([frozen-SPEC ruling](compatibility-policy.md#frozen-specs)).
A change to them needs a maintainer ruling.
The oxpinyin paths and constant names are current reference.

The candidate window scan ([candidate-construction.md](candidate-construction.md#8-the-frozen-construction-contract) §8) walks the key set that the pinned oracle's matrix admits at each byte position.
That key set is the selected parse's keys plus two frozen lists of alternative splits.
This SPEC freezes those lists and their semantics.
It is the data half of the scan.
The scan itself is in `crates/oxpinyin-engine/src/session/` (`RESPLIT_TABLE` and `DIVIDED_TABLE` in `split_tables.rs`).

## Provenance

The lists come from the maintainer's source-verified audit of the pinned libpinyin 2.11.91 parser and its generated ambiguity tables.
That audit produced the scan specification.
The lists are frozen here as project data: the scan implementation cites this SPEC, not upstream paths.
The lists are closed: an oracle pin change re-verifies them and does not extend them.

## Resplit pairs (85)

A pair `(first, second)`, whose first key ends at the second key's column, admits the alternative `(left, right)` at the same byte positions.
`left` occupies the start of `first`, and `right` runs from its end to the end of `second`.
The pair may include keys that the same resplit pass added earlier.
A pair never resplits across an apostrophe separator; a separator before the first key is allowed.

- `a + nan` → `an + an`
- `an + gang` → `ang + ang`
- `ba + nan` → `ban + an`
- `ca + nan` → `can + an`
- `chan + gan` → `chang + an`
- `chan + ge` → `chang + e`
- `che + nai` → `chen + ai`
- `chen + gan` → `cheng + an`
- `chu + nan` → `chun + an`
- `dan + gan` → `dang + an`
- `e + nai` → `en + ai`
- `e + nen` → `en + en`
- `fa + nan` → `fan + an`
- `fan + gai` → `fang + ai`
- `fan + gan` → `fang + an`
- `fan + ge` → `fang + e`
- `ga + nai` → `gan + ai`
- `ga + nen` → `gan + en`
- `gan + gao` → `gang + ao`
- `guan + gan` → `guang + an`
- `hu + nan` → `hun + an`
- `huan + gan` → `huang + an`
- `ji + ne` → `jin + e`
- `ji + nou` → `jin + ou`
- `jia + nai` → `jian + ai`
- `jia + nan` → `jian + an`
- `jia + nao` → `jian + ao`
- `jia + ne` → `jian + e`
- `jia + nou` → `jian + ou`
- `jian + gan` → `jiang + an`
- `jin + gai` → `jing + ai`
- `jin + gan` → `jing + an`
- `jin + ge` → `jing + e`
- `kuan + gao` → `kuang + ao`
- `li + nan` → `lin + an`
- `lia + nai` → `lian + ai`
- `lia + ne` → `lian + e`
- `lian + gan` → `liang + an`
- `ma + ne` → `man + e`
- `men + gen` → `meng + en`
- `min + gan` → `ming + an`
- `min + ge` → `ming + e`
- `na + nai` → `nan + ai`
- `na + nan` → `nan + an`
- `na + nao` → `nan + ao`
- `na + nou` → `nan + ou`
- `nin + gan` → `ning + an`
- `pa + nan` → `pan + an`
- `pen + gan` → `peng + an`
- `pin + gan` → `ping + an`
- `qi + nai` → `qin + ai`
- `qi + nan` → `qin + an`
- `qia + nan` → `qian + an`
- `qia + ne` → `qian + e`
- `qin + gai` → `qing + ai`
- `qin + gan` → `qing + an`
- `qu + na` → `qun + a`
- `re + nai` → `ren + ai`
- `re + nan` → `ren + an`
- `san + gou` → `sang + ou`
- `shan + gan` → `shang + an`
- `she + nai` → `shen + ai`
- `she + nao` → `shen + ao`
- `wa + nan` → `wan + an`
- `wa + ne` → `wan + e`
- `wa + nou` → `wan + ou`
- `wen + gan` → `weng + an`
- `xi + nai` → `xin + ai`
- `xi + nan` → `xin + an`
- `xia + nai` → `xian + ai`
- `xia + nan` → `xian + an`
- `xia + ne` → `xian + e`
- `xian + gai` → `xiang + ai`
- `xian + gan` → `xiang + an`
- `xian + ge` → `xiang + e`
- `xin + gai` → `xing + ai`
- `xin + gan` → `xing + an`
- `ya + nan` → `yan + an`
- `yi + nan` → `yin + an`
- `yi + ne` → `yin + e`
- `zhan + gai` → `zhang + ai`
- `zhe + nai` → `zhen + ai`
- `zhe + nan` → `zhen + an`
- `zhen + gan` → `zheng + an`
- `zhua + nan` → `zhuan + an`

## Divided syllables (20)

A matrix key `syllable` also splits into `left + right`, where `left` ends inside the syllable.
The parts run from the syllable's own text start.

- `bian` → `bi + an`
- `bie` → `bi + e`
- `dian` → `di + an`
- `jian` → `ji + an`
- `jiang` → `ji + ang`
- `jie` → `ji + e`
- `jue` → `ju + e`
- `kuai` → `ku + ai`
- `lian` → `li + an`
- `liang` → `li + ang`
- `liao` → `li + ao`
- `luan` → `lu + an`
- `qian` → `qi + an`
- `qie` → `qi + e`
- `shuan` → `shu + an`
- `tian` → `ti + an`
- `tuan` → `tu + an`
- `xian` → `xi + an`
- `yuan` → `yu + an`
- `zuan` → `zu + an`

## Semantics

- **Resplit** walks columns in increasing byte order and excludes the reserved final column.
  - It snapshots the current column once and visits every key in that snapshot.
    For each key, it snapshots the column at the key's raw end.
  - Every matching pair appends the left key to the current column.
    It appends the right key at the current byte index plus the text length of the left syllable.
  - Keys added to the current column do not extend its existing snapshot.
  - Right halves that earlier columns appended are present when the pass visits their later column.
    So resplits chain: `ba + nan + gang` admits `ban + an + gang`, then `ban + ang + ang`.
  - This follows [`resplit_step` at pin `074a2219`, lines 113–157](https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/phonetic_key_matrix.cpp#L113-L157).
- **Separators** prevent a resplit across an apostrophe.
  [`fill_matrix` inserts zero keys in the gap](https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/phonetic_key_matrix.cpp#L68-L78), and the resplit table cannot match that zero key.
  An apostrophe before the first real key of a pair does not prevent its resplit.
  `chui'lianai` still admits `lia + nai` → `lian + ai`, positioned from the raw begin of `lia`.
- **Divided** runs after resplit, and it also walks forward with a snapshot per column.
  - It applies to every matching matrix key, including keys that resplit or earlier divided columns added.
  - A preceding apostrophe does not block it.
    The parts start at the syllable's own raw begin, so `bu'tian` still offers `补体` from the divided `ti`.
  - See [`inner_split_step` at the same pin](https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/phonetic_key_matrix.cpp#L169-L228).
- Both tables compare the full key, including tone, against zero-tone table entries.
- Both passes only add keys and never remove one, so the selected parse's own keys stay available.
- The pinyin full-parser order is [fill → resplit → divided → fuzzy](https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/pinyin.cpp#L1497-L1524).
