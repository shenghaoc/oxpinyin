# F-A and F-C capture fixture freeze

Status: frozen capture

This finding freezes the wire format of the F-A and F-C capture output: line-oriented text, not JSON.

## Reference

Every record carries this name-version-revision stamp:

```text
libpinyin-2.11.92-074a2219c90feaf962d0d24f034514033ece5f99+model20-59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155+dbm-tkrzw
```

Frozen outputs:

| Family | Records | SHA-256 |
|---|---:|---|
| `fixtures/foundation/f-a.txt` | 15 | `6690f849f6361f97f6b26b11159b67d6737924534ebfb0a69d7722040ea76a40` |
| `fixtures/foundation/f-c.txt` | 46 | `1712555f148952fbc624976b8e11703f1af93e30c9f8ab32833eab0fc6735307` |

No recipe in the tree regenerates these fixtures; the regenerator belongs to
[the next pin capture](../runbooks/goldens-and-pins.md#the-next-pin-capture).

The runner creates a new empty user directory for each family. The harness
never calls training, remembering, choosing or saving APIs. The harness rejects the
`DYNAMIC_ADJUST` bit, and no record contains it.
The benign oracle diagnostic about a missing fresh `user.conf` goes to
stderr and is not part of either fixture.

## Line protocol

A fixture is UTF-8 text with exactly one record per LF-terminated line. Each
record is a sequence of TAB-separated `key=value` fields. Inside values, `\\`, TAB, CR, LF
and other control bytes are escaped as `\\\\`, `\\t`, `\\r`,
`\\n` and `\\xNN`. Records contain:

- `schema=pinyin-capture-v1`;
- the full `pin_ref` above;
- `family` and stable `case` identifiers;
- the per-record public `pinyin.h` `api_sequence` that affects the observation;
- escaped `input` and hexadecimal `flags`;
- the outputs `parse_return`, `parsed_input_length`, `segments`, `candidate_total`,
  `candidates_hex` and `remainder`.

`pinyin_init` and `pinyin_fini` are family-scoped setup and teardown. They are not
reported as per-record calls.

A segment is `canonical@begin:end:complete|partial`. Byte positions are
half-open offsets into `input`. Apostrophe separator bytes therefore appear as
gaps between adjacent segments. `segments=-` means no parsed segment.
Candidates are limited to the first ten deterministic results. They are encoded as
comma-separated UTF-8 byte strings in lowercase hexadecimal. `candidate_total`
keeps the full count.

## F-A coverage

F-A uses `IS_PINYIN | PINYIN_INCOMPLETE | USE_DIVIDED_TABLE |
USE_RESPLIT_TABLE`, with dynamic adjustment off. It freezes:

- valid single and multi-syllable input: `ni`, `nihao`, `zhongguoren`;
- the F-E-12 totality seed `zhuan`;
- ambiguous input: `xian`, `fangan`;
- hard apostrophe boundaries: `xi'an`, `chang'an`;
- incomplete tails: `nih`, `zhongg`;
- junk at prefix, middle and suffix positions;
- empty input;
- a 4,096-byte junk input.

F-A records only the segmentation that the pinned oracle selected. It does not define
the path set of the portable parser.

## F-C coverage

F-C uses `IS_PINYIN` as the baseline. For each additional full-pinyin parser bit, it captures an off/on pair with
identical input. It covers
incomplete parsing, tone handling, divided and resplit tables, all ten pinyin
ambiguity bits, and all eight pinyin correction bits. Candidate totals and the
first ten candidates show matching and scoring effects, also when the selected
segmentation is unchanged. Zhuyin-only bits are outside this full-pinyin
family. The capture protocol excludes `DYNAMIC_ADJUST`. The `force-tone`
pair deliberately records no observed output change at this public API
surface; it does not infer unobserved behavior.
