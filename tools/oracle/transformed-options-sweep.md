# Transformed parse option sweep

This replaces the unavailable `/home/sheng/audit-r2-scratch` audit harness.
Its counts are **not comparable** with that harness's 104 single words,
435 pair words, or 496/149-input corpora. Nothing here claims those inputs
or their coverage were recovered.

## Exact lists

`transformed-options-words.txt` lists 470 distinct explicit `uint32` words:

- 34 single words: zero, `0xffffffff`, and each `1 << bit` for bits 0–31.
- 435 pairs: every two-bit combination of bits 1–19 and 21–31. Bits 0
  and 20 are reserved and excluded from pairs, matching the replacement
  sweep's declared option inventory.
- One extra word: `0x0000018a`, the parity profile.

All lists are sorted numerically and deduplicated. The 11 inputs and
scheme numbers are listed in `transformed-options-inputs.json`: Hanyu
`sh`, `lishbakua`; Luoma `lishihbakua`, `chih`, `rih`, `sih`, `zih`,
`shih`; MS double `nihk`; STANDARD chewing `su3cl3`; secondary zhuyin
`tsz`. There are 5,170 paired observations per cell.

## Run

Build the pin with `tools/oracle/build-oracle.sh --dbm CELL
--enable-libzhuyin` and the subject with the matching feature (`bdb`,
`kyotocabinet`, or `tkrzw`). Both sides read that cell's pin data:

```sh
python3 tools/oracle/transformed-options-sweep.py \
  --oracle "$PREFIX/lib/libpinyin.so" \
  --subject "$TARGET/debug/libpinyin_capi.so" \
  --data "$PREFIX/lib/libpinyin/data" --expect fixed
```

`--expect parent` asserts the pre-fix 29/31 ordinary-candidate differing
words for double/chewing. `--expect fixed` asserts zero for both. Both
assert the unchanged Hanyu/Luoma/secondary ordinary counts (0/0/0) and the
complete-protocol counts (0/0/0).

Each word and library runs in an isolated subprocess, with a fresh user
profile, no training, and a fresh instance per input. Options are always
set explicitly. Call order: parse → parsed length → guess sentence →
get rank-0 sentence when guessing succeeds → guess candidates at offset
zero with sort word `0x1e` → enumerate every candidate. Comparisons use
canonical UTF-8 JSON bytes, including return statuses, consumed/parsed
lengths, rank-0 sentence, and the complete ordered candidate vector
(getter statuses, type, text, and n-best index for n-best candidates).
No hash-only comparison, prefix truncation, or capture files are used.
Diagnostics on stderr are outside this comparison (#545).

A word differs for a scheme when any of that scheme's inputs differs.
The ordinary-candidate count compares every ordered row except type-1
n-best rows; complete-protocol counts include those rows and all observed
returns. This distinction exposes the known residuals rather than
silently treating them as parity.

After the sweep, targeted checks cover `nihk` / `su3cl3` at `0x8002`,
MS double `n` at `0x800a`, chewing `su` at `0x8002`, and Hanyu `sh` /
`lishbakua` at `0x2`, `0xa`, `0x20`, `0x28`. Fixed transformed cases and
every Hanyu control, the empty-key `sh` included, must match byte for byte on
the observed surface. #651 made `pinyin_guess_sentence` return false on a parse
that placed no key, as the pin does (`phonetic_lookup.h:743-745`), so the
`guess_sentence` difference #542 owned is gone and the Hanyu complete count is
0, where it was 437 (all from `sh`; `lishbakua` already matched).

Fresh profile directories are removed on normal completion. A worker
crash or timeout fails the run instead of counting as a candidate mismatch.
Subprocess timeout is 600 seconds per word/library. The runner writes only
stdout summaries; it retains no logs.

## Lane J parsed-key intake and cursor review

After #691, observed Luoma and secondary-zhuyin differing-word counts are
0 ordinary / 0 complete-protocol each. #690 leaves two residual words
(`0x60` and `0xffffffff`), which #691 fixes.
Hanyu remains 0/0 as established by #688; fixed double and chewing remain
0/0. Run `--expect fixed` to enforce these counts on this stack member.
`--lane-j-session` checks the seven repaired inputs at 0x2/0x20/0x28.

`--cursor-family pinyin` (or `zhuyin`, with the corresponding library)
checks all offsets 0 through input length for the same seven inputs at
0x2/0x20/0x28: cursor normalization, left/right, packed key, key-rest
positions/length, and character offset. Each family checks 38 positions
per option word. Cursor calls are isolated so the pin's right-offset
asserts are recorded; each requires the subject's established class-(c)
false return and exactly one warning. Other results and out-parameters
must match. Output includes complete observations; retain it outside the
repository as evidence.
