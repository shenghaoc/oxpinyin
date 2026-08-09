# F-A and F-C capture fixture freeze

Date: 2026-08-09 · Status: frozen Foundation capture

## Reference and reproduction

Every record carries this name-version-revision stamp:

```text
libpinyin-2.11.91-0c5e80e1200f84fab185d1c5bde458b770a0636c+model20-59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155+dbm-tkrzw
```

Reproduce from a clean ignored build directory:

```bash
tools/oracle/build-oracle.sh --work-dir target/oracle --jobs 4
tools/capture/run-capture.sh target/oracle/prefix fixtures/foundation
sha256sum fixtures/foundation/f-a.nvr fixtures/foundation/f-c.nvr
```

Frozen outputs:

| Family | Records | SHA-256 |
|---|---:|---|
| `fixtures/foundation/f-a.nvr` | 15 | `d9599903593cda62ae9f60b80ab3140e584592738ed770e1638fff03879ade9b` |
| `fixtures/foundation/f-c.nvr` | 24 | `934761a605b33e775daff43a4c2cbdc42d1a09373c6679cf3535c181197dda5e` |

The runner creates a new empty user directory for each family. The harness
never calls training, remembering, choosing, or saving APIs. The
`DYNAMIC_ADJUST` bit is rejected by the harness and absent from every record.
The benign oracle diagnostic about a missing fresh `user.conf` is emitted on
stderr and is not part of either fixture.

## Line protocol

A fixture is UTF-8 text with exactly one record per LF-terminated line. Each
record is a sequence of TAB-separated `key=value` fields. `\\`, TAB, CR, LF
and other control bytes inside values are escaped as `\\\\`, `\\t`, `\\r`,
`\\n` and `\\xNN`. Records contain:

- `schema=pinyin-capture-v1`;
- the full `nvr` above;
- `family` and stable `case` identifiers;
- the public `pinyin.h` `api_sequence` affecting the observation;
- escaped `input` and hexadecimal `flags`;
- `parse_return`, `parsed_input_length`, `segments`, and `remainder` outputs.

A segment is `canonical@begin:end:complete|partial`. Byte positions are
half-open offsets into `input`; apostrophe separator bytes therefore appear as
gaps between adjacent segments. `segments=-` represents no parsed segment.

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

F-A records the pinned oracle's selected segmentation only. It does not define
the portable parser's path set. The parser SPEC must add every valid
segmentation, including alternatives not selected by the oracle, before Rust
implementation begins.

## F-C coverage

F-C uses `IS_PINYIN` as the baseline and enables one additional full-pinyin
parser bit per record. It covers incomplete parsing, tone handling, divided
and resplit tables, all ten pinyin ambiguity bits, and all eight pinyin
correction bits. Zhuyin-only bits are outside this full-pinyin family;
`DYNAMIC_ADJUST` is excluded by the capture protocol. Flags whose effect is in
candidate matching rather than segmentation may intentionally produce the
same parser output as baseline.
