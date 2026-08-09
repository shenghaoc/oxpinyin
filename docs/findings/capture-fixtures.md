# F-A and F-C capture fixture freeze

Date: 2026-08-09 · Status: frozen Foundation capture

The Task 4 execution amendment selects line-oriented text rather than JSON.
The authored Foundation spec files remain unchanged; this finding freezes the
wire format used by the task output.

## Reference and reproduction

Every record carries this name-version-revision stamp:

```text
libpinyin-2.11.91-0c5e80e1200f84fab185d1c5bde458b770a0636c+model20-59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155+dbm-tkrzw
```

Reproduce from a clean ignored build directory:

```bash
tools/oracle/build-oracle.sh --work-dir target/oracle --jobs 4
tools/capture/run-capture.sh target/oracle/prefix fixtures/foundation
sha256sum fixtures/foundation/f-a.txt fixtures/foundation/f-c.txt
```

The runner validates `oracle-pin.txt`, the public header, shared object, data
manifest, and every generated data file before accepting the pin ref.

Frozen outputs:

| Family | Records | SHA-256 |
|---|---:|---|
| `fixtures/foundation/f-a.txt` | 15 | `8a82f2195b80e7596cb0d4069d096dbc5064eecea9e2dcd78b2fff144a81c858` |
| `fixtures/foundation/f-c.txt` | 46 | `e24aa79f0beb60f99924606962eaeec0941c0335a29335806bed25533033bfb5` |

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
- the full `pin_ref` above;
- `family` and stable `case` identifiers;
- the per-record public `pinyin.h` `api_sequence` affecting the observation;
- escaped `input` and hexadecimal `flags`;
- `parse_return`, `parsed_input_length`, `segments`, `candidate_total`,
  `candidates_hex`, and `remainder` outputs.

`pinyin_init` and `pinyin_fini` are family-scoped setup/teardown and are not
misreported as per-record calls.

A segment is `canonical@begin:end:complete|partial`. Byte positions are
half-open offsets into `input`; apostrophe separator bytes therefore appear as
gaps between adjacent segments. `segments=-` represents no parsed segment.
Candidates are capped at the first ten deterministic results and encoded as
comma-separated UTF-8 byte strings in lowercase hexadecimal; `candidate_total`
retains the uncapped count.

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

F-C uses `IS_PINYIN` as the baseline and captures an off/on pair with
identical input for each additional full-pinyin parser bit. It covers
incomplete parsing, tone handling, divided and resplit tables, all ten pinyin
ambiguity bits, and all eight pinyin correction bits. Candidate totals and the
first ten candidates expose matching/scoring effects even when the selected
segmentation is unchanged. Zhuyin-only bits are outside this full-pinyin
family; `DYNAMIC_ADJUST` is excluded by the capture protocol. The `force-tone`
pair deliberately records no observed output change at this public API
surface rather than inferring unobserved behavior.
