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
  --subject "$TARGET/release/libpinyin_capi.so" \
  --data "$PREFIX/lib/libpinyin/data" --expect fixed
```

`--expect parent` asserts the pre-fix 29/31 ordinary-candidate differing
words for double/chewing. `--expect fixed` asserts zero for both. Both
assert the unchanged Hanyu/Luoma/secondary counts (0/437/437).

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
nonempty Hanyu controls must match byte for byte on the observed surface.
Empty-key Hanyu controls retain the declared `guess_sentence` difference
owned by #542, lane C; conditional sentence-get calls are reported too.

Fresh profile directories are removed on normal completion. A worker
crash or timeout fails the run instead of counting as a candidate mismatch.
Subprocess timeout is 90 seconds per word/library. The runner writes only
stdout summaries; it retains no logs.
