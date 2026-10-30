# Pin-built libpinyin on arm64: five non-crash contract cases, two out-of-range float-to-`guint32` stores (#1137)

Recorded 2026-10-24 UTC. No captures or logs are committed; the commands
below reproduce every figure.

This document diagnoses the five #1132 contract cases that exit 0 and
are byte-identical between the amd64 pin and oxpinyin, but return a
different candidate order from the arm64 pin:

- `row58-unsigned-score-off`
- `row58-unsigned-score-on`
- `table-conf-lambda-million-zhuyin`
- `table-conf-lambda-one-and-a-half-zhuyin`
- `table-conf-lambda-two-zhuyin`

They reduce to two independent defects, both the same C++ undefined
behaviour at two separate sites: a `float` expression assigned to a
`guint32` when the value is negative or at least 2³². Neither plain
`char` signedness nor an uninitialised read is involved. The eight crash
cases are covered by a short note at the end; their different arm64
signals do not require a second source defect.

## Result

| case | defect | arm64 pin | amd64 pin = arm64 subject |
| --- | --- | --- | --- |
| `row58-unsigned-score-off` | pinyin predicted-prefix candidate frequency, `src/pinyin.cpp:1821-1824` | `甲甲` is last of the 65 two-character rows (index 64, after `们的`) | `甲甲` first (index 0) |
| `row58-unsigned-score-on` | the same site; the option bit does not reach this branch | identical to `-off` | identical to `-off` |
| `table-conf-lambda-million-zhuyin` | zhuyin candidate frequency, `src/zhuyin.cpp:1264-1268` | rows 3–12 are the collection order (all length 1) `疒 祢 禰 妳 呢 堄 逆 猊 霓 儗` | rows 3–12 are frequency-ordered `尼 禰 妳 堄 輗 鉨 齯 鶂 貎 膩` |
| `table-conf-lambda-one-and-a-half-zhuyin` | the same site | the same arm64 pin rows as `million` | `禰 妳 堄 儗 輗 蜺 鉨 齯 鶂 貎` |
| `table-conf-lambda-two-zhuyin` | the same site | the same arm64 pin rows as `million` | `禰 妳 堄 儗 輗 鉨 齯 鶂 貎 膩` |

The arm64 pin's three zhuyin observations are byte-identical to one
another (bdb canonical hash `d4a2282014a0`), while the three amd64-pin
observations differ (`ce9baa647ac0`, `7fd8ff5fcf7f`,
`49e68e3f23f7`). That is the signature of every negative candidate
frequency becoming the same value on arm64, not of a scanner difference
or a heap-dependent read. The two `row58` arm64-pin observations are
likewise identical (`5e04d6ce4a92`), as are the two amd64-pin
observations (`6f99d0e60bdb`); the `DYNAMIC_ADJUST` bit cannot change
the predicted-prefix branch, which reads no bigram.

## Artifacts and revision

- Pin: libpinyin 2.11.92 at
  `074a2219c90feaf962d0d24f034514033ece5f99`, model20
  `59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155`.
  Source is read at the pin from a clone of
  `https://github.com/libpinyin/libpinyin.git` checked out at that
  commit.
- Compiler: GCC 15.3.0 (`GCC: (Debian 15.3.0-2) 15.3.0`) for both
  architectures. The `.comment` sections of the four objects carry the
  same string, so the instruction difference below is target code
  generation, not compiler version.
- amd64 objects: the published reproducible oracle image
  `ghcr.io/shenghaoc/oxpinyin-oracle:074a2219c90f-dab11cdb0a9d-20260831T000000Z-fb1afc111b20`,
  cell bdb. The disassembled files are
  `opt/pinyin-oracle/bdb/lib/libpinyin.so.15.0.0` and
  `libzhuyin.so.15.0.0`, SHA-256
  `754499651c302efd91fe7f72dd6a791978400b268cb6206d2ebe644015721f30`
  and
  `cf4ccbce9f70f6c7e412b92a52a243ba64fdea1ff197cbc44019c6b85650d53b`,
  equal to `tools/oracle/oracle-pin.txt`.
- arm64 objects: the #1132 oracle, same recipe and pin. The
  disassembled files are
  `<arm64-prefix>/bdb/lib/libpinyin.so.15.0.0` and
  `libzhuyin.so.15.0.0` from the arm64 build, SHA-256
  `473187d8d040b8a5763a188509d6be031e224a712ad9b7716defdb4ff1293a2f`
  and
  `e90429560584cf9dfee345d41e608092032d3a1c7248e04feeedc899dcfd24b5`,
  equal to the `_linux_arm64` rows of `tools/oracle/oracle-pin.txt`.
  Build and host details are in
  [`oracle-arm64-1132.md`](oracle-arm64-1132.md).
- Observations: the arm64 side is #1132's `--observations` JSONL for the
  13 excluded cases (`arm64-13-{bdb,kc,tkrzw}.jsonl` in that lane). The
  amd64 side is the #1132 amd64 capture recorded in issue comment
  [6380711257](https://github.com/shenghaoc/oxpinyin/issues/1132#issuecomment-6380711257).
  The five amd64 pin hashes in that comment equal the five arm64
  subject hashes. The arm64 pin and subject hashes quoted above were
  recomputed from the JSONL with

  ```sh
  python3 - <<'EOF'
  import json, hashlib
  cases = {'row58-unsigned-score-off', 'row58-unsigned-score-on',
           'table-conf-lambda-million-zhuyin',
           'table-conf-lambda-one-and-a-half-zhuyin',
           'table-conf-lambda-two-zhuyin'}
  h = lambda x: hashlib.sha256(json.dumps(
      x, sort_keys=True, separators=(',', ':'), ensure_ascii=True
  ).encode()).hexdigest()[:12]
  for line in open('arm64-13-bdb.jsonl'):
      o = json.loads(line)
      if o['case'] in cases:
          print(o['case'], 'pin', h(o['pin']), 'subject', h(o['subject']))
  EOF
  ```

  The kc and tkrzw observations show the same five candidate-order
  differences; only the `user.conf` backend word differs, as #1132
  records. The bdb objects are cited for disassembly; the source is the
  same at the pin and the observed candidate rows are the same on all
  three cells.

## Method

The pin's source is read first. Where the source leaves the generated
code open — which instruction performs the `guint32` assignment — the
two pin-built objects are disassembled with

```sh
objdump -dlS -C --no-show-raw-insn \
  --prefix-strip=3 --prefix=/tmp/libpinyin-074a2219 \
  --start-address=0x... --stop-address=0x... \
  <prefix>/lib/libpinyin.so.15.0.0
```

The host's `/usr/bin/objdump` is Apple LLVM 21.0.0; its
`--prefix-strip` is a directory-count. GNU objdump takes the same
numeric level (`--prefix-strip=3`), not a pathname. Level 3 strips the
three directory names `usr`, `src` and `libpinyin` from the recorded
`/usr/src/libpinyin/src/pinyin.cpp`, leaving `src/pinyin.cpp`;
`--prefix` then supplies the pin checkout. The check

```sh
test -f /tmp/libpinyin-074a2219/src/pinyin.cpp
test -f /tmp/libpinyin-074a2219/src/zhuyin.cpp
```

confirms the stripped result, and the interleaved source line numbers
agree with the pin clone. No pin run, sanitiser build or probe produced
any figure below.

## Defect 1 — pinyin candidate frequency, `src/pinyin.cpp:1821-1824`

### Case input

Both `row58-unsigned-score-*` cases do the same thing; `-on` sets the
`DYNAMIC_ADJUST` bit and `-off` clears it. The probe is
`row58_signed_score` in `tools/bisection/contract-diff.py`:

1. Import one phrase `我甲甲` (`wo3'jia3'jia3`) with count
   `1431655631`. `_add_phrase` stores `count * 3`
   (`src/pinyin.cpp:514-520`, `:604-605`), so the item's unigram is
   `4294966893`.
2. Call `token_add_unigram_frequency(inst, 0x01ffffff, delta)` with
   `delta` chosen so the facade total wraps to exactly `10000000`.
   `FacadePhraseIndex::add_unigram_frequency`
   (`src/storage/phrase_index.h:628-635`) adds `delta` to the facade
   total before the absent token refuses; the probe records
   `absent_add = false`.
3. Call `pinyin_guess_predicted_candidates(inst, "我")` and print the
   candidate list.

The phrase `我甲甲` reaches the list as a `PREDICTED_PREFIX_CANDIDATE`:
`m_begin = 1`, phrase length 2, text `甲甲`. Its unigram is now larger
than the facade total, violating the source's `possibility value <=
1.0` comment.

### Source

`_compute_frequency_of_items` (`src/pinyin.cpp:1794-1868`) has three
assignments of a `gfloat` expression to `guint32`:

```c
/* predicted-prefix branch, :1813-1826 */
guint32 freq = ((1 - lambda) *
                cached_item.get_unigram_frequency() /
                (gfloat) total_freq) * 256 * 256 * 256;
item->m_freq = freq;                                  /* :1824 */

/* addon branch, :1829-1843 */
guint32 freq = ((1 - lambda) *
                cached_item.get_unigram_frequency() /
                (gfloat) total_freq) * 256 * 256 * 256;
item->m_freq = freq;                                  /* :1841 */

/* ordinary candidate branch, :1855-1868 */
guint32 freq = (lambda * bigram_poss * BIGRAM_FREQUENCY_DISCOUNT +
                (1 - lambda) *
                cached_item.get_unigram_frequency() /
                (gfloat) total_freq) * 256 * 256 * 256;
item->m_freq = freq;                                  /* :1866 */
```

`lambda` is `gfloat`, `get_unigram_frequency()` is `guint32` promoted to
`float`, `total_freq` is cast to `gfloat`, and the `256` factors are
`int` promoted to `float`. The expression is therefore single
precision, and the assignment is a `float`-to-`guint32` conversion. When
the value is negative or at least 2³², C++ [conv.fpint]/1 (the same rule
as C11 6.3.1.4p1) makes the conversion undefined. The source's
`possibility value <= 1.0` invariant is what normally keeps the value in
range; a `table.conf` λ outside `[0, 1]`, or an imported unigram larger
than the facade total, can break it.

The conversion is the initialization of `freq` at `:1821-1823`; the
32-bit store is `item->m_freq = freq` at `:1824`. The `row58` cases
exercise the first site. The other two are the same defect in the same
function; the proposed upstream report covers all three.

### Arithmetic

With `lambda = 0.312699` (the stock `table.conf` value, stored as
`gfloat`), `1 - lambda = 0.68730104`, `unigram` = `4294966893` rounded
to `gfloat` = `4294966784`, and `total_freq = 10000000`:

```text
((1 - lambda) * unigram / total) * 256 * 256 * 256
  = 4952525312.0f
  = 0x1_2731_8a00
```

The low 32 bits are `0x27318a00 = 657558016`, which is 657558016 above
2³² = 4294967296.

```sh
python3 - <<'EOF'
import struct
f32 = lambda x: struct.unpack('f', struct.pack('f', x))[0]
lam = f32(0.312699)
x = f32(f32(f32(1.0 - lam) * f32(1431655631 * 3)) / f32(10000000.0))
for _ in range(3):
    x = f32(x * f32(256.0))
print(repr(x), int(x) % 2**32, hex(int(x) % 2**32))
EOF
# 4952525312.0 657558016 0x27318a00
```

### Instructions

amd64, `_compute_frequency_of_items` at
`libpinyin.so.15.0.0+0x8fa20`; the predicted-prefix and addon branches
share the conversion at `+0x8fcce`:

```text
; shared prefix/addon frequency code; the line table attributes it to
; :1840 (the addon branch), and the prefix branch reaches it from :1821-1823
   8fcca:  mulss  <256.0>, %xmm1
   8fcce:  cvttss2si %xmm1, %rax
; the store is shared with :1866
   8fcd3:  jmp    0x8fb6e
   8fb6e:  movl   %eax, 0x1c(%r15)
```

`cvttss2si` converts to a signed 64-bit integer. `4952525312` fits in
64 bits, so `%rax = 0x1_2731_8a00` and the 32-bit store keeps
`0x27318a00`.

arm64, `_compute_frequency_of_items` at `libpinyin.so.15.0.0+0x72408`;
the predicted-prefix branch converts at `+0x72644`:

```text
; src/pinyin.cpp:1823
   72638:  fmul   s31, s31, s15
   7263c:  fmul   s31, s31, s15
   72640:  fmul   s31, s31, s15
   72644:  fcvtzu s31, s31
; src/pinyin.cpp:1825
   72648:  b      0x72528
   72530:  str    s31, [x20, #0x1c]
```

`fcvtzu` converts to an unsigned 32-bit integer with saturation. The
value is above `UINT32_MAX`, so the stored word is `0xffffffff`. The
addon branch has its own `fcvtzu` at `+0x726b8`; the ordinary branch
converts at `+0x72524`. All three store through `+0x72530`.

### Why the observed positions follow

The comparator is `compare_item_with_sort_option`
(`src/pinyin.cpp:1678-1709`), frequency key at `:1700-1706`:

```c
guint32 freq_lhs = item_lhs->m_freq;
guint32 freq_rhs = item_rhs->m_freq;
if (freq_lhs != freq_rhs)
    return -(freq_lhs - freq_rhs); /* in descendant order */
```

The subtraction is `guint32` arithmetic; the negation and the conversion
to `gint` wrap.

- amd64 stores `0x27318a00 = 657558016`. Every other item's unigram is
  bounded by the pre-import system total, `51051831`, so every other
  frequency is at most
  `(1-λ)·51051831/10000000·2²⁴ = 58867856` (computed in `f32`).
  `657558016 > 58867856`, so `甲甲` sorts first among the two-character
  rows. The observed amd64/subject index is 0.
- arm64 stores `0xffffffff`. For any other two-character row with
  frequency `f < 2³¹`, `-(0xffffffff - f) = f + 1 > 0`, so `甲甲` sorts
  after it. Every other frequency is at most `58867856`, well below
  2³¹, so `甲甲` sorts last among the two-character rows and before the
  one-character rows, whose phrase-length key dominates. The observed arm64 pin index is 64,
  immediately after `们的` and immediately before `们`; the amd64/subject
  list has `甲甲` at 0 and `们的` at 64. This is the exact predicted
  permutation, not a general reordering.

The two option states produce the same list because the
`PREDICTED_PREFIX_CANDIDATE` branch at `:1813-1826` returns before the
`DYNAMIC_ADJUST` test at `:1845`. The `-off`/`-on` equality is therefore
a control that rules out the bigram term.

### Ruled out

- **Plain `char` signedness.** No plain `char` participates in the
  frequency. The phrase string is bytes compared by `strcmp`; the
  candidate frequency is `guint32`. A signedness change would not move
  one candidate by exactly the sign boundary of the comparator.
- **An uninitialised read.** `_append_items` (`src/pinyin.cpp:1769-1792`)
  fills every candidate's `m_freq` from the template and
  `_compute_frequency_of_items` overwrites it. The same input produces
  the same list on both architectures except for this one key.
- **The tail comparator.** `trellis_value_compare`
  (`src/lookup/phonetic_lookup.h:174-178`) is not reached by the
  predicted-candidate sort, and the five cases' sentences are identical
  on both architectures. Its own conversion is discussed under defect 2.

## Defect 2 — zhuyin candidate frequency, `src/zhuyin.cpp:1264-1268`

### Case input

`conf_case('lambda-…', ('lambda parameter:0.312699',
'lambda parameter:' + value))` rewrites a private copy of `table.conf`
and runs `conf_session`. `conf_view` allocates an instance, parses
`ni3hao3` (and `yi4shu4`), calls `zhuyin_guess_sentence`, then
`zhuyin_guess_candidates_after_cursor(inst, 0)` and prints the first 12
candidate texts. The λ values are the ordinary decimal spellings `1e6`,
`1.5` and `2`; both the oracle image's scanner and the Debian 13 host's
accept them, and the `host_scanf` marker applies only to the malformed
exponent-without-digits spellings, not these.

### Source

`_compute_frequency_of_items` (`src/zhuyin.cpp:1229-1270`):

```c
gfloat lambda = context->m_system_table_info.get_lambda();

if (options & DYNAMIC_ADJUST) { ... }              /* :1247-1255 */

guint32 freq = (lambda * bigram_poss +
                (1 - lambda) *
                cached_item.get_unigram_frequency() /
                (gfloat) total_freq) * 256 * 256 * 256;
item->m_freq = freq;                               /* :1268 */
```

The conversion is the initialization of `freq` at `:1264-1267`; the
32-bit store is `item->m_freq = freq` at `:1268`. The default zhuyin
options are `USE_TONE | FORCE_TONE` (`src/zhuyin.cpp:273`), so
`DYNAMIC_ADJUST` is clear and `bigram_poss` stays `0`. The expression reduces to
`(1 - lambda) * unigram / total * 2²⁴`. For λ > 1 the coefficient is
negative, so every candidate with a positive unigram gets a negative
`float` and the assignment to `guint32` is undefined.

### Instructions

amd64, `_compute_frequency_of_items` at
`libzhuyin.so.15.0.0+0x8dee0`:

```text
; src/zhuyin.cpp:1267
   8e055:  mulss  <256.0>, %xmm1
   8e05d:  mulss  <256.0>, %xmm1
   8e065:  mulss  <256.0>, %xmm1
   8e06d:  cvttss2si %xmm1, %rax
   8e072:  movl   %eax, 0x1c(%rbx)
```

For a negative score of magnitude `k`, `%rax = -k` and the 32-bit store
keeps `2³² - k` (mod 2³²). The comparator
`compare_item_with_length_and_frequency` (`src/zhuyin.cpp:1129-1144`)
then computes `-(freq_lhs - freq_rhs)` in `guint32`.

arm64, `_compute_frequency_of_items` at
`libzhuyin.so.15.0.0+0x70804`:

```text
; src/zhuyin.cpp:1267
   7092c:  fmul   s31, s31, s15
   70930:  fmul   s31, s31, s15
   70934:  fmul   s31, s31, s15
   70938:  fcvtzu s31, s31
   7093c:  str    s31, [x19, #0x1c]
```

`fcvtzu` of a negative finite value is 0. Every negative candidate
frequency is therefore the same key on arm64.

### Why the observed order follows

For λ = 1.5, `k = trunc(0.5 · unigram/total · 2²⁴)`; for λ = 2,
`k = trunc(unigram/total · 2²⁴)`. Because `unigram ≤ total`,
`0 ≤ k < 2³¹` for both. amd64's wrapped keys are all in the upper half
`(2³¹, 2³²)`, and the comparator is consistent there: a larger `k`
gives a smaller wrapped key and sorts later. The amd64 order is
therefore ascending `k` (before truncation ties, ascending unigram).
arm64's keys are all 0, so GLib's stable merge keeps the collection
order within each phrase-length group; every normal candidate in this
window has phrase length 1, so the rows are the collection order. The
amd64 pin reorders them by the unigram key. The first two
rows, `儞䝥` and `你好`, are prepended sentence candidates
(`_prepend_sentence_candidates`, `src/zhuyin.cpp:1272-1288`, called
after the sort at `:1533`), so they are outside the frequency sort and
are identical on both architectures.

For λ = 1e6, `k` exceeds 2³²; amd64 keeps `2³² - (k mod 2³²)` (or 0 when
`k` is a multiple of 2³²), so the order is the low-32 residue rather
than a monotone unigram order. It is still a varying key, while arm64
stays all 0. This is why the three arm64 pin observations are identical
and the three amd64-pin ones are not.

The cases with λ below 0, `inf` and `nan` do not appear in the #1132
exclusions because their keys do not split the two architectures:

- λ < 0 makes `1 - λ > 1`; every candidate frequency is positive and in
  range (the unigram term is at most `(1 - λ) * 2²⁴`, well below 2³² for
  these cases, and the bigram term is zero), so both architectures store
  the same keys.
- λ = `inf` or `nan` makes every candidate frequency NaN: the first
  term is `inf * 0` (NaN) and the second is `-inf`, and `nan` is
  already NaN. On amd64 `cvttss2si` of a non-finite value returns
  `INT64_MIN`, whose low 32 bits are 0; on arm64 `fcvtzu` of NaN is 0. Every key is equal within each build, so both
  keep the collection order and the observations agree. The same
  applies to the trellis's non-finite `m_poss` differences.

### Ruled out

- **The host scanner.** The inputs are `1e6`, `1.5` and `2`, all
  ordinary decimal `%f` spellings accepted by any conforming C library.
  The `host_scanf` marker in `contract-diff.py` covers only the malformed
  exponent-without-digits spellings (`1e` and `0x1p`); it does not cover
  these cases. A scanner that rejected them would fail init, not reorder
  candidates; the pin exits 0 and returns the full list.
- **Uninitialised memory.** `_append_items` initialises every candidate
  and `_compute_frequency_of_items` assigns every `m_freq` before the
  sort. The arm64 pin's three λ observations being byte-identical is
  the opposite of a heap-dependent read; it is what the same saturated
  key predicts.
- **The tail comparator.** `trellis_value_compare`
  (`src/lookup/phonetic_lookup.h:174-178`) compiles to `fcvtzs w0,s30`
  on arm64 and `cvttss2si %xmm0,%eax` on amd64. For a NaN difference,
  arm64 gives 0 and amd64 `INT_MIN`; as policy row 83 records, both make
  GLib's merge take the left element on `<= 0`, so the resulting order is
  the same. The sentences in these cases are identical on both
  architectures (`儞䝥` for `ni3hao3`, `苅襩` for `yi4shu4`); only the
  candidate rows differ. The tail comparator is not this defect.

## The eight crash cases

The eight cases are `fini-twice`, `zhuyin-fini-twice` and the six
`replay-modify-header-overlong-*` / `replay-touched-*-invalid-*` cases.
They crash on amd64 too; the #1132 harness expects that, so they MATCH
there.

| cases | known defect | amd64 pin signal | arm64 pin signal |
| --- | --- | --- | --- |
| `fini-twice`, `zhuyin-fini-twice` | second `fini` double free, policy row 105 (`src/pinyin.cpp:1194-1222`, `src/zhuyin.cpp:741-757`) | SIGSEGV (-11) | SIGBUS (-7) |
| six `replay-*` | overlong item header read past the item, policy row 107 (`src/storage/phrase_index.cpp:439-517` and the `PhraseItem` header getters) | SIGSEGV (-11) | SIGTRAP (-5) |

Reading the pin source and the two objects does not produce a second
source defect for either signal change:

- Both groups reach the same C++ defects at the same source lines on
  both architectures. The double free and the over-read are
  architecture-independent.
- The signal is the runtime's response to the same invalid memory
  operation. The double-free fault is in allocator metadata whose state
  depends on the allocation history and the host glibc; the replay fault
  is an over-read whose address depends on the mapping and the item
  copy. Neither is a pin source path that exists only on arm64.
- The run-to-run variation is the same dependence. The over-read's
  target and the double free's allocator state are heap-layout
  dependent, so which run faults at the first invalid access is not
  stable. `fini-twice`'s stderr already reads freed bytes and differs
  between amd64 runs (the #1132 correction notes the uninitialised
  filename bytes), so the pin side of that case is not stable on either
  architecture.
- The arm64 runs use the Debian 13 host's glibc against objects built
  in the pinned `debian:testing` image; the amd64 capture's host libc is
  not separately recorded in #1132. The capture does not separate the
  host libc from the architecture, and this note does not claim it
  does. No arm64-only pin source defect is needed either way.

The precise arm64 first-fault instruction for these eight is not claimed:
settling it needs an instrumented pin or a core dump, which #1137 puts
out of scope. The question it asks — whether a cause beyond the known
amd64 crash exists — is answered no on the source and disassembly read
here.

## Oxpinyin's position

**Superseded 2026-10-30 UTC (#1416).** The low-32-bit imitation
described below was removed under decision-order limb 3 (processor and
compiler behaviour is not reproduced; [source policy](../source-policy.md#internal-language-artefacts)). `oxpinyin_engine::session::
amplified_frequency_with_bigram`, which the predicted rows now call too,
stores the score with Rust's `as u32`: in range it truncates like both
pins; a negative or NaN score is 0 and a score at or above 2³² is
`0xffffffff`, which is what the arm64 pin's `fcvtzu` stores. The
amd64-only wrap is class (b) (C leaves the conversion undefined), held
POLICY-ACCEPTED by the cases named in `upstream-divergences.md`. The
text below records the position at the time of the #1137 reading.

Oxpinyin reproduced the amd64 pin, not the arm64 pin, on both
architectures, and that was the Stage 2 target (the contributor rules, "Stage 2";
`docs/findings/compatibility-policy.md` row 83). The relevant code was:

- Predicted candidates: `oxpinyin_capi::predict::amplified_frequency`
  computes the same `f32` expression and converts with
  `u64::from(score as i64 as u32)`, which keeps the low 32 bits of the
  x86-64 `cvttss2si` result for finite in-range scores. It is the same
  law as `oxpinyin_engine::session::guint32_of_score`, which the engine
  uses for ordinary candidate lists.
- Ordinary candidates, both facades:
  `oxpinyin_engine::session::amplified_frequency_with_bigram` and
  `guint32_of_score` (`crates/oxpinyin-engine/src/session/mod.rs`). The
  latter returns 0 for non-finite or 64-bit-out-of-range scores, which
  is the low 32 bits of `cvttss2si`'s `INT64_MIN`.
- Candidate order: `oxpinyin_engine::session::glib_rank_sort` and
  `RankKey::pin_compare` reproduce the pin's `guint32` subtraction and
  GLib's merge, as row 83 records.

Both paths therefore produce `0x27318a00` for the `row58` candidate and
`2³² - k` for the zhuyin negative frequencies on arm64 as well as amd64.
That is why the arm64 subject hashes equal the amd64 pin hashes in the
#1132 table. No oxpinyin change is owed. No case shows oxpinyin
depending on pin undefined behaviour beyond the deliberate
`cvttss2si` reproduction that row 83 already records.

## Proposed upstream issues

Both are drafts. Nothing has been filed.

### Issue A — pinyin candidate frequency: undefined float-to-`guint32` conversion is target-dependent

> **Title:** Candidate frequency: out-of-range float-to-guint32 conversion differs on arm64
>
> **Version:** libpinyin 2.11.92, commit
> `074a2219c90feaf962d0d24f034514033ece5f99`, built with GCC 15.3.0
> by `tools/oracle/build-oracle.sh` (autoconf `-g -O2`, assertions
> live).
>
> **Summary:** `_compute_frequency_of_items` (`src/pinyin.cpp:1794-1868`)
> assigns the candidate frequency to `guint32` in the
> predicted-prefix branch (`:1821-1824`), the addon branch
> (`:1838-1841`) and the ordinary branch (`:1862-1866`). When the
> `float` value is negative or at least 2³², the conversion is
> undefined (C++ [conv.fpint]/1). This is reachable from `table.conf`
> (λ values outside `[0, 1]`) and from a user import whose unigram exceeds
> the facade total.
>
> **Observed:** On x86-64 the assignment compiles to `cvttss2si` to a
> 64-bit register followed by a 32-bit store (`libpinyin.so.15.0.0`
> offsets `+0x8fb69` and `+0x8fcce`); the low 32 bits are kept. On
> arm64 it compiles to `fcvtzu` (`+0x72524`, `+0x72644`, `+0x726b8`),
> which saturates: a negative value becomes 0 and a value at least 2³²
> becomes `0xffffffff`.
>
> **Reproduction:** import `我甲甲` (`wo3'jia3'jia3`) with count
> 1431655631, add the delta that brings the facade total to 10000000
> through `pinyin_token_add_unigram_frequency` on `0x01ffffff`, then
> call `pinyin_guess_predicted_candidates(inst, "我")` with the stock
> λ = 0.312699. The frequency is `4952525312.0f`. On x86-64 the
> candidate `甲甲` is first (stored key `657558016`); on arm64 it is
> last among the two-character candidates (stored key `0xffffffff`).
>
> **Suggested fix:** make the conversion defined. The x86-64 low-32
> result is what consumers see today, so a helper that checks
> finiteness and the 64-bit range and then stores `(guint32)(gint64)
> score` preserves it; a saturating clamp is the alternative. Apply the
> same helper to all three assignments.

### Issue B — libzhuyin candidate frequency: negative frequency saturates to 0 on arm64 for λ > 1

> **Title:** libzhuyin candidate frequency: λ > 1 orders candidates differently on arm64
>
> **Version:** libpinyin 2.11.92 / libzhuyin at commit
> `074a2219c90feaf962d0d24f034514033ece5f99`, GCC 15.3.0, the same
> build configuration as issue A.
>
> **Summary:** `_compute_frequency_of_items` in `src/zhuyin.cpp:1229-1270`
> assigns `(lambda * bigram_poss + (1 - lambda) * unigram /
> (gfloat) total_freq) * 256 * 256 * 256` to `guint32` at `:1264-1267`
> and stores it at `:1268`. With
> the default options `DYNAMIC_ADJUST` is clear, so the value is
> `(1 - lambda) * unigram / total * 2²⁴`. For λ > 1 the coefficient is
> negative and every positive-unigram candidate gets an undefined
> `float`-to-`guint32` conversion.
>
> **Observed:** x86-64 converts with `cvttss2si` and stores the low 32
> bits (`libzhuyin.so.15.0.0+0x8e06d`, `+0x8e072`), so a score of `-k`
> becomes `2³² - k` for `0 < k < 2³²` (and the low 32 bits of `-k`
> otherwise). arm64 converts with `fcvtzu` (`+0x70938`,
> `+0x7093c`) and stores 0 for every negative score. In
> `zhuyin_guess_candidates_after_cursor` the x86-64 list is therefore
> ordered by the wrapped key and the arm64 list keeps the collection
> order. With `lambda parameter:1.5`, `2` and `1e6` in `table.conf`,
> the arm64 pin's candidate rows 3–12 are identical for all three
> values, while the x86-64 rows differ. The chosen sentence is the same
> on both targets.
>
> **Reproduction:** a private `table.conf` with
> `lambda parameter:1.5`, parse `ni3hao3`, call
> `zhuyin_guess_sentence`, then `zhuyin_guess_candidates_after_cursor
> (inst, 0)` and print the first 12 candidate strings. On x86-64 the
> rows after `儞䝥` and `你好` are
> `禰 妳 堄 儗 輗 蜺 鉨 齯 鶂 貎`; on arm64 they are
> `疒 祢 禰 妳 呢 堄 逆 猊 霓 儗`, the collection order. λ = 2 and
> λ = 1e6 behave the same way; λ below 0, `inf` and `nan` do not,
> because all their keys are equal or in range.
>
> **Suggested fix:** the same defined conversion helper as issue A,
> applied at `src/zhuyin.cpp:1268`.

## Reproduction commands

Pin source:

```sh
git clone https://github.com/libpinyin/libpinyin.git /tmp/libpinyin-074a2219
git -C /tmp/libpinyin-074a2219 checkout \
  074a2219c90feaf962d0d24f034514033ece5f99
```

amd64 objects (the published image, bdb cell):

```sh
TAG=$(tools/oracle/image-tag.sh)
TOKEN=$(curl -fsSL \
  "https://ghcr.io/token?scope=repository:shenghaoc/oxpinyin-oracle:pull" \
  | jq -r .token)
DIGEST=$(curl -fsSL -H "Authorization: Bearer $TOKEN" \
  -H 'Accept: application/vnd.docker.distribution.manifest.v2+json' \
  "https://ghcr.io/v2/shenghaoc/oxpinyin-oracle/manifests/$TAG" \
  | jq -r '.layers[0].digest')
curl -fsSL -H "Authorization: Bearer $TOKEN" \
  "https://ghcr.io/v2/shenghaoc/oxpinyin-oracle/blobs/$DIGEST" \
  -o /tmp/oracle-amd64.tgz
mkdir -p /tmp/oracle-amd64 && tar xzf /tmp/oracle-amd64.tgz \
  -C /tmp/oracle-amd64
shasum -a 256 \
  /tmp/oracle-amd64/opt/pinyin-oracle/bdb/lib/libpinyin.so.15.0.0 \
  /tmp/oracle-amd64/opt/pinyin-oracle/bdb/lib/libzhuyin.so.15.0.0
# 754499651c302efd91fe7f72dd6a791978400b268cb6206d2ebe644015721f30
# cf4ccbce9f70f6c7e412b92a52a243ba64fdea1ff197cbc44019c6b85650d53b
```

arm64 objects: build with the recipe in
[`oracle-arm64-1132.md`](oracle-arm64-1132.md), then verify

```sh
shasum -a 256 \
  <prefix>/bdb/lib/libpinyin.so.15.0.0 \
  <prefix>/bdb/lib/libzhuyin.so.15.0.0
# 473187d8d040b8a5763a188509d6be031e224a712ad9b7716defdb4ff1293a2f
# e90429560584cf9dfee345d41e608092032d3a1c7248e04feeedc899dcfd24b5
```

The conversion sites (LLVM objdump; GNU objdump uses the same numeric
`--prefix-strip=3`):

```sh
P=/tmp/libpinyin-074a2219
A=/tmp/oracle-amd64/opt/pinyin-oracle/bdb/lib
R=<arm64-prefix>/bdb/lib

# pinyin _compute_frequency_of_items, both targets
objdump -dlS -C --no-show-raw-insn --prefix-strip=3 --prefix=$P \
  --start-address=0x8fa20 --stop-address=0x8fd10 $A/libpinyin.so.15.0.0
objdump -dlS -C --no-show-raw-insn --prefix-strip=3 --prefix=$P \
  --start-address=0x72408 --stop-address=0x72720 $R/libpinyin.so.15.0.0

# zhuyin _compute_frequency_of_items, both targets
objdump -dlS -C --no-show-raw-insn --prefix-strip=3 --prefix=$P \
  --start-address=0x8dee0 --stop-address=0x8e15a $A/libzhuyin.so.15.0.0
objdump -dlS -C --no-show-raw-insn --prefix-strip=3 --prefix=$P \
  --start-address=0x70804 --stop-address=0x70a68 $R/libzhuyin.so.15.0.0

# the two candidate comparators
objdump -dlS -C --no-show-raw-insn --prefix-strip=3 --prefix=$P \
  --start-address=0x8dec0 --stop-address=0x8df20 $A/libpinyin.so.15.0.0
objdump -dlS -C --no-show-raw-insn --prefix-strip=3 --prefix=$P \
  --start-address=0x706a0 --stop-address=0x70710 $R/libpinyin.so.15.0.0
objdump -dlS -C --no-show-raw-insn --prefix-strip=3 --prefix=$P \
  --start-address=0x8d740 --stop-address=0x8d758 $A/libzhuyin.so.15.0.0
objdump -dlS -C --no-show-raw-insn --prefix-strip=3 --prefix=$P \
  --start-address=0x6fee0 --stop-address=0x6ff08 $R/libzhuyin.so.15.0.0

# the tail comparator, for the negative control
objdump -dlS -C --no-show-raw-insn --prefix-strip=3 --prefix=$P \
  --start-address=0x96730 --stop-address=0x96750 $A/libpinyin.so.15.0.0
objdump -dlS -C --no-show-raw-insn --prefix-strip=3 --prefix=$P \
  --start-address=0x79880 --stop-address=0x798a0 $R/libpinyin.so.15.0.0
```

The five contract cases reproduce with the maintained driver command:

```sh
tools/bisection/run-contract-diff.sh bdb $PINYIN_ORACLE_PREFIX \
  target-bdb/debug/libpinyin_capi.so target-bdb/debug/libzhuyin_capi.so \
  -- --run-platform-excluded \
  --cases row58-unsigned-score-off,row58-unsigned-score-on,table-conf-lambda-million-zhuyin,table-conf-lambda-one-and-a-half-zhuyin,table-conf-lambda-two-zhuyin \
  --observations obs-bdb.jsonl
```

`--run-platform-excluded` is required on arm64: without it the five
cases are SKIPs and `obs-bdb.jsonl` stays empty. With it, each case runs
and a JSON line on stderr states `override: "platform-excluded"`, the
registry reason and `note: "not an ordinary gate"`; the stdout verdict is
then the ordinary MATCH/DIFFER.
The flag does not bypass the backend `cells=` skip or any other skip
kind, so a case whose fixture belongs to another cell still prints SKIP.
On amd64 the flag changes nothing because the platform check does not
fire. On arm64 this command exits 1 by design: the five selected cases
are the differences this document diagnoses, so the driver reports five
`DIFFER` verdicts; it is the reproduction, not a passing gate.

The arm64 side needs the arm64 oracle and subject from #1132; the amd64
side needs the amd64 oracle and subject. The #1132 comment records the
amd64 command; `oracle-arm64-1132.md` records the arm64 one and the
13-case hashes. The five arm64-pin rows quoted above are in
`arm64-13-bdb.jsonl`.

## Limits

- The five diagnoses rest on the source and the two generated instruction
  sequences. The candidate rows are the observed ones from the #1132
  capture, not a rerun.
- The arm64 signals of the eight crash cases are recorded; their precise
  first-fault instructions are not. That is the short note #1137 asks
  for, not a second diagnosis of a crash amd64 already shows.
- The arm64 runs use the Debian 13 host's glibc against objects built in
  the pinned `debian:testing` image; the amd64 capture's host libc is not
  separately recorded in #1132. The five conversion differences do not
  depend on libc (they are in the generated code), but the eight crash
  signals may.

```contract-diff-policy
{
  "schema": 2,
  "record": "arm64-pin-portability-1137",
  "status": "current",
  "disposition": "POLICY-ACCEPTED",
  "cases": {
    "row58-unsigned-score-off": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "a22dc0b88ecc7679d817dceefd888a6c2aa12a68d287b6520726e4f6e69c5f55",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.absent_add": "fcbcf165908dd18a",
              "result.logs": "4f53cda18c2baa0c",
              "result.prediction": "64e70f03c9bab24b",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "ac3a8cae5af2b755c085e44cdae61940e141f7eb821c4eae5702e7c4507d0858",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.absent_add": "fcbcf165908dd18a",
              "result.logs": "4f53cda18c2baa0c",
              "result.prediction": "7b6fc3478e1f7c17",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.prediction.rows (list lengths pin 169, subject 169; first change [0]): pin [5, \"甲甲\"]; subject [5, \"爱你\"]",
            "… further divergence omitted (4096-byte budget)"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "a22dc0b88ecc7679d817dceefd888a6c2aa12a68d287b6520726e4f6e69c5f55",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.absent_add": "fcbcf165908dd18a",
              "result.logs": "4f53cda18c2baa0c",
              "result.prediction": "64e70f03c9bab24b",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "ac3a8cae5af2b755c085e44cdae61940e141f7eb821c4eae5702e7c4507d0858",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.absent_add": "fcbcf165908dd18a",
              "result.logs": "4f53cda18c2baa0c",
              "result.prediction": "7b6fc3478e1f7c17",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.prediction.rows (list lengths pin 169, subject 169; first change [0]): pin [5, \"甲甲\"]; subject [5, \"爱你\"]",
            "… further divergence omitted (4096-byte budget)"
          ]
        },
        "tkrzw": {
          "pin": {
            "sha256": "a22dc0b88ecc7679d817dceefd888a6c2aa12a68d287b6520726e4f6e69c5f55",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.absent_add": "fcbcf165908dd18a",
              "result.logs": "4f53cda18c2baa0c",
              "result.prediction": "64e70f03c9bab24b",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "ac3a8cae5af2b755c085e44cdae61940e141f7eb821c4eae5702e7c4507d0858",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.absent_add": "fcbcf165908dd18a",
              "result.logs": "4f53cda18c2baa0c",
              "result.prediction": "7b6fc3478e1f7c17",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.prediction.rows (list lengths pin 169, subject 169; first change [0]): pin [5, \"甲甲\"]; subject [5, \"爱你\"]",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "row58-unsigned-score-on": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "a22dc0b88ecc7679d817dceefd888a6c2aa12a68d287b6520726e4f6e69c5f55",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.absent_add": "fcbcf165908dd18a",
              "result.logs": "4f53cda18c2baa0c",
              "result.prediction": "64e70f03c9bab24b",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "ac3a8cae5af2b755c085e44cdae61940e141f7eb821c4eae5702e7c4507d0858",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.absent_add": "fcbcf165908dd18a",
              "result.logs": "4f53cda18c2baa0c",
              "result.prediction": "7b6fc3478e1f7c17",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.prediction.rows (list lengths pin 169, subject 169; first change [0]): pin [5, \"甲甲\"]; subject [5, \"爱你\"]",
            "… further divergence omitted (4096-byte budget)"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "a22dc0b88ecc7679d817dceefd888a6c2aa12a68d287b6520726e4f6e69c5f55",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.absent_add": "fcbcf165908dd18a",
              "result.logs": "4f53cda18c2baa0c",
              "result.prediction": "64e70f03c9bab24b",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "ac3a8cae5af2b755c085e44cdae61940e141f7eb821c4eae5702e7c4507d0858",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.absent_add": "fcbcf165908dd18a",
              "result.logs": "4f53cda18c2baa0c",
              "result.prediction": "7b6fc3478e1f7c17",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.prediction.rows (list lengths pin 169, subject 169; first change [0]): pin [5, \"甲甲\"]; subject [5, \"爱你\"]",
            "… further divergence omitted (4096-byte budget)"
          ]
        },
        "tkrzw": {
          "pin": {
            "sha256": "a22dc0b88ecc7679d817dceefd888a6c2aa12a68d287b6520726e4f6e69c5f55",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.absent_add": "fcbcf165908dd18a",
              "result.logs": "4f53cda18c2baa0c",
              "result.prediction": "64e70f03c9bab24b",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "ac3a8cae5af2b755c085e44cdae61940e141f7eb821c4eae5702e7c4507d0858",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.absent_add": "fcbcf165908dd18a",
              "result.logs": "4f53cda18c2baa0c",
              "result.prediction": "7b6fc3478e1f7c17",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.prediction.rows (list lengths pin 169, subject 169; first change [0]): pin [5, \"甲甲\"]; subject [5, \"爱你\"]",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    }
  }
}
```
