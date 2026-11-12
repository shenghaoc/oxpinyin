# Transformed parse option sweep

This sweep compares the pin and the subject over transformed parse option words.
It replaces the unavailable `/home/sheng/audit-r2-scratch` audit harness.
Its counts are **not comparable** with the 104 single words,
435 pair words, or 496/149-input corpora of that harness. Nothing here claims that those inputs
or their coverage were recovered.

## Exact lists

`transformed-options-words.txt` lists 470 distinct explicit `uint32` words:

- 34 single words: zero, `0xffffffff`, and each `1 << bit` for bits 0–31.
- 435 pairs: every two-bit combination of bits 1–19 and 21–31. Bits 0
  and 20 are reserved and excluded from pairs, as in the declared option inventory of the replacement
  sweep.
- One extra word: `0x0000018a`, the parity profile.

All lists are sorted numerically and deduplicated.
`transformed-options-inputs.json` lists the 11 inputs and the
scheme numbers:

- Hanyu: `sh`, `lishbakua`;
- Luoma: `lishihbakua`, `chih`, `rih`, `sih`, `zih`, `shih`;
- MS double: `nihk`;
- STANDARD chewing: `su3cl3`;
- secondary zhuyin: `tsz`.

There are 5,170 paired observations per cell.

## Run

Build the pin with `python3 tools/oracle/build.py --dbm CELL
--enable-libzhuyin`, and the subject with the matching feature (`bdb`,
`kyotocabinet`, or `tkrzw`). Both sides read the pin data of that cell:

```sh
uv run --no-project --managed-python python -X utf8 -m tools.oracle.transformed_options_sweep \
  --oracle "$PREFIX/lib/libpinyin.so" \
  --subject "$TARGET/debug/libpinyin_capi.so" \
  --data "$PREFIX/lib/libpinyin/data" --expect fixed
```

The `--expect` modes assert these counts:

- `--expect parent` asserts the pre-fix 29/31 ordinary-candidate differing
  words for double/chewing.
- `--expect fixed` asserts zero for both.
- Both modes
  assert the unchanged Hanyu/Luoma/secondary ordinary counts (0/0/0) and the
  complete-protocol counts (0/0/0).

Each word and library runs in an isolated subprocess, with a fresh user
profile, no training, and a fresh instance for each input. The options are always
set explicitly. The call order is:

1. parse;
2. parsed length;
3. guess sentence;
4. get the rank-0 sentence when guessing succeeds;
5. guess candidates at offset
   zero with sort word `0x1e`;
6. enumerate every candidate.

Comparisons use
canonical UTF-8 JSON bytes. These include return statuses, consumed and parsed
lengths, the rank-0 sentence, and the complete ordered candidate vector
(getter statuses, type, text, and n-best index for n-best candidates).
The sweep uses no hash-only comparison, no prefix truncation and no capture files.
Diagnostics on stderr are outside this comparison (#545).

A word differs for a scheme when any input of that scheme differs.
The ordinary-candidate count compares every ordered row except type-1
n-best rows. The complete-protocol counts include those rows and all observed
returns. This distinction shows the known residuals, so that they are not
silently treated as parity.

After the sweep, targeted checks cover:

- `nihk` / `su3cl3` at `0x8002`;
- MS double `n` at `0x800a`;
- chewing `su` at `0x8002`;
- Hanyu `sh` / `lishbakua` at `0x2`, `0xa`, `0x20`, `0x28`.

Fixed transformed cases and
every Hanyu control, including the empty-key `sh`, must match byte for byte on
the observed surface. Since #651, `pinyin_guess_sentence` returns false on a parse
that placed no key, as the pin does (`phonetic_lookup.h:743-745`). The
`guess_sentence` difference that #542 owned is therefore gone, and the Hanyu complete count is
0. Recorded evidence: before #651, it was 437 (all from `sh`; `lishbakua` already matched).

Fresh profile directories are removed on normal completion. A worker
crash or timeout fails the run; it does not count as a candidate mismatch.
The subprocess timeout is 600 seconds per word/library. The runner writes only
stdout summaries and keeps no logs.

## Lane J parsed-key intake and cursor review

With #691, the observed Luoma and secondary-zhuyin differing-word counts are
0 ordinary / 0 complete-protocol each. #690 leaves two residual words
(`0x60` and `0xffffffff`), which #691 fixes.
Hanyu stays 0/0, as #688 established. Fixed double and chewing stay
0/0. To enforce these counts on this stack member, run `--expect fixed`.
`--lane-j-session` checks the seven repaired inputs at 0x2/0x20/0x28.

`--cursor-family pinyin` (or `zhuyin`, with the corresponding library)
checks all offsets from 0 through the input length for the same seven inputs at
0x2/0x20/0x28. It checks cursor normalization, left/right, the packed key, the key-rest
positions and length, and the character offset. Each family checks 38 positions
per option word. Cursor calls are isolated, so that the right-offset
asserts of the pin are recorded. Each such assert requires the subject to abort (SIGABRT) too, with no warning that the pin does not print.
Other results and out-parameters
must match. The output includes complete observations; keep it outside the
repository as evidence.
