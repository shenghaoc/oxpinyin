# Separator columns and retained parse topology (#640, #698, #681)

Recorded 2026-10-08 UTC. Subject: libpinyin 2.11.92, pin
`074a2219c90feaf962d0d24f034514033ece5f99` (source tree read at that
commit, `HEAD` verified before citing). Reference cell: BDB. Closes
register row 54 item 1 and the before-cursor half of row 26 of
[compatibility-policy.md](compatibility-policy.md). Items 2 and 3 of row 54
are untouched.

## What the pin does

`fill_matrix` places a real key at the column where it begins and a zero
key at each `'` and at the last column (`phonetic_key_matrix.cpp:27-80`). A
leading `'` therefore leaves column 0 **empty**: the pin has no n-best row
for `'nihao` (`pinyin.cpp:1470`), the key/rest getters answer from the
column they are asked for, and several getters abort on an empty column or
a zero key (`_check_offset`, `pinyin.cpp:3092`; `size > 0`, `:3147`;
`zhuyin.cpp:1453` `assert(zero_key != key)`). The before-cursor window
searches the complete retained matrix at a fixed end
(`zhuyin.cpp:1562-1588`, `phonetic_key_matrix.cpp:416-423`); it does not
re-parse a truncated prefix.

## What oxpinyin did

The session kept the compacted key sequence, so a leading separator shifted
every coordinate, and the zhuyin before-cursor window re-parsed the prefix
up to the cursor. #623 closed the mid-key *after*-cursor window only.
Measured before the change (24 separator-free zhuyin inputs, 177 offsets):
108 offsets differed; `ni` at offset 1 answered 788 candidates against the
pin's 1.

## Change

`crates/oxpinyin-engine/src/session/matrix.rs` retains the parsed-column
topology in original input coordinates, distinguishing empty columns,
zero-key separator columns and real keys. Candidate windows use the whole
matrix. Internal facade additions `abi_key_at`, `abi_left_offset` and
`validate_abi_lookup_offset` return `Result`; the existing methods delegate
to them and `normalize_lookup_offset` keeps its signature. No engine public
signature, C ABI or dependency change. Space is O(B+K) for B parsed bytes
and K keys; no sentence or dictionary caching.

## Ledger

All figures come from `contract-diff.py` on BDB (options `0x18a`, sort
`0x1e`), a fresh process and parse per case. Cases were added to the
existing driver; no new driver.

| Group | Cases | Result |
|---|---:|---|
| Six leading-separator inputs (`'ni`, `''ni`, `'nihao`, `'ni'hao`, `'ni'`, `'a`), both facades, every offset, every surface | 504 | no non-abort difference; 91 pin SIGABRTs, subject answers false with exactly one warning (78 already did; the other 13 are zhuyin class (c)) |
| zhuyin before-cursor windows (#698) | 177 | identical inventories |
| zero-key accessors on non-leading separators (`ni'`, `ni'hao`, `ni''hao`, `ni'hao'`) and the #681 cases | 108 | identical except 8 pin aborts, subject false plus one warning |
| allocator-option zero-key cases (`0x1aa`) | 16 | identical except one pin abort |
| separator-free controls | 555 | identical |

#681 (public getters, sentinel `0xABCDEF`): pinyin `xi'` at offset 2 and
`xi''` at offsets 2 and 3 answer true with a zero key and key-rest [3,4] /
[4,5]; on those zero keys the pinyin, zhuyin, luoma and secondary-zhuyin
string getters answer false with NULL, and `pinyin_get_pinyin_strings`
answers false with both outputs untouched. Zhuyin `xi1'an1` and `xi1'` at
offset 3: zero key, key-rest [3,4], both string renderers false with NULL.
Zhuyin `xi1'an1` key getter at offset 4 aborts the pin at `zhuyin.cpp:1453`;
the subject answers false with one warning (class (c)).

The four non-abort differences in the full run are the pre-existing
register entries `alloc-instance-after-fini` and `nth-pronunciation-range`
(each in both facades), unrelated to this change.

## The 13 zhuyin class (c) sites

Thirteen zhuyin cases aborted the pin where the subject answered without
warning (six `character(length)` calls, one per input, and seven calls on
`'ni'hao` and `'ni'` that returned data). They reach two pin assertions,
both already cited by register rows, so no row is added. Row 63 is
pinyin-only (`phonetic_key_matrix.h:103`, `pinyin.cpp:3488`) and does not
apply.

| Cases | Pin assertion at 074a2219 | Register row |
|---|---|---|
| `character(length)` of five inputs (`separator/zhuyin/{0/3,1/4,2/6,3/7,5/2}`) | `zhuyin.cpp:2110`, `assert(size > 0)` in `_get_char_offset_recur` | row 19 (cites the libzhuyin twins `zhuyin.cpp:2110`, `:2158`) |
| `character(length)` of `'ni'` (`4/4`) | `zhuyin.cpp:1453`, `assert(zero_key != key)` via `_check_offset` called at `zhuyin.cpp:2158` | rows 66 (`:1453`) and 19 (`:2158`) |
| `'ni'hao`: after, before at offset 4 and `key`, `rest` at 4, `left` at 7 (`3/{candidates,before,key,rest}/4`, `3/left/7`) | `zhuyin.cpp:1453`, `assert(zero_key != key)` | row 66 (cites `:1453` through after/before-cursor and the offset probes) |
| `'ni'`: after, before at offset 4 (`4/{candidates,before}/4`) | `zhuyin.cpp:1453` | row 66 |

Row 66 cites `zhuyin.cpp:1453` by line and by its entry points (the
candidate windows and the left/right offset probes); the key and key-rest
getters reach the same assertion on a zero key, which row 66 does not
list as an entry point. The assertion site is cited, so this is a
mapping, not a new row.

## Results

BDB, head `aeca0439`, `contract-diff` PASS:

- 504 leading-separator cases: no non-abort difference; 91 pin aborts,
  each answered false with one warning by the subject.
- 177/177 before-cursor inventories identical.
- 108 non-leading zero-key and #681 cases identical except 8 pin aborts.
- 16 allocator-option cases identical except 1 pin abort.
- 555 separator-free controls identical.

The captures are not committed.
