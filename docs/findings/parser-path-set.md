# Full-pinyin parser path-set SPEC

Date: 2026-08-09 · Status: **frozen for Foundation Tasks 6–9**

What binds: the ordered set of alternatives that the full-pinyin parser returns for an input, including the frozen examples below, and the result bound, as far as they are derived from the pin and observable through the C APIs ([frozen-SPEC ruling](compatibility-policy.md#frozen-specs)).
A change to it needs a maintainer ruling, recorded in the [correction log](#architect-correction-log).
The names of tests and tasks are current reference.

This SPEC is normative together with [parser-spec.md](parser-spec.md).
F-A records only the path that the oracle selects.
This path set returns every valid complete alternative on purpose and leaves selection to the decoder.
`FullPinyinParser::parse_with_options` returns this path set; no C API returns it.
The path set does not contain the pin's mid-path or repeated initial-only keys.
[segment-graph.md](segment-graph.md) carries those keys, and [parser-spec.md](parser-spec.md) states the pin's rule.

## Complete group enumeration

A valid segmentation contains only complete table syllables and consumes its whole lowercase group.
For one group, enumerate valid segmentations with depth-first backtracking from byte zero:

1. At each cursor, consider every complete table syllable that matches the input prefix at that cursor.
2. Visit matches by descending byte length (greedy longest match first).
3. Recurse or iterate from the end of each match.
4. Emit a path only when it consumes the whole group.

A fixed input prefix identifies at most one syllable of a given length, so a secondary tie-break is not normally necessary.
If the inventory changes to permit a tie, compare syllable bytes ascending.
Remove duplicate paths and keep the first occurrence.

Under this exhaustive rule, 164 of the 405 complete table entries have more than one complete segmentation of the entry alone.
Round-trip tests assert that the greedy identity path is the first path, not that it is the only path.

For more than one apostrophe-separated group, keep each group's order and form the Cartesian product left to right: for each left path in order, append each right path in order.
Apostrophes are consumed separators and never segments.
A separator is consumed only if the right side consumes at least one byte as a complete or terminal partial segment.

## Partial fallback

The partial fallback is a rule of the portable parser; the pin has no partial fallback.
Partial-tail results satisfy R3.3/R3.4, but they are not valid segmentations under R3.7.
Consider them only when a group has no complete path:

1. Enumerate paths of zero or more complete syllables followed by one non-empty proper prefix of a table syllable.
2. Keep only the paths whose final end offset is maximal.
3. Order the kept paths by the same greedy depth-first order.
4. Remove duplicates and keep the first occurrence.

If neither a complete nor a partial continuation exists at a cursor, keep the complete prefix already obtained and start the remainder at that cursor.
When this happens before an apostrophe:

- keep the group's maximal partial, if one exists;
- stop before the apostrophe;
- leave the apostrophe and all following bytes in the remainder.

Complete-path precedence is required: the parser does not return partial alternatives when any complete segmentation consumes the group.
So `xian` does not also return `[xi, a, n(partial)]`; its complete path set is the two all-complete paths.

## Result bound

Count the final deduplicated path set before materialization, saturating at 4,097.
A count over `MAX_PARSE_RESULTS` (4,096) returns `ParseError::TooManyAlternatives { limit: 4_096 }` and no partial vector.
A count at or below the limit is emitted exhaustively in the order above.
The bound applies after Cartesian products and after maximal-partial filtering.

## Frozen examples

The notation is `text@start:end:C` for complete and `text@start:end:P` for partial.
The paths below are in the required return order.

| Input | Ordered paths | Remainder |
|---|---|---|
| empty | `[]` | empty |
| `ni` | `[ni@0:2:C]` | empty |
| `nihao` | `[ni@0:2:C, hao@2:5:C]`; `[ni@0:2:C, ha@2:4:C, o@4:5:C]` | empty |
| `xian` | `[xian@0:4:C]`; `[xi@0:2:C, an@2:4:C]` | empty |
| `fangan` | `[fang@0:4:C, an@4:6:C]`; `[fan@0:3:C, gan@3:6:C]`; `[fa@0:2:C, ng@2:4:C, an@4:6:C]` | empty |
| `xi'an` | `[xi@0:2:C, an@3:5:C]` | empty |
| `chang'an` | `[chang@0:5:C, an@6:8:C]`; `[cha@0:3:C, ng@3:5:C, an@6:8:C]` | empty |
| `nih` | `[ni@0:2:C, h@2:3:P]` | empty |
| `zhongg` | `[zhong@0:5:C, g@5:6:P]` | empty |
| `ni'h` | `[ni@0:2:C, h@3:4:P]` | empty |
| `nih'ao` | `[ni@0:2:C, h@2:3:P]` | `'ao` |
| `!ni` | `[]` | `!ni` |
| `ni!hao` | `[ni@0:2:C]` | `!hao` |
| `ni!` | `[ni@0:2:C]` | `!` |
| `ni'` | `[ni@0:2:C]` | `'` |
| `ni'i` | `[ni@0:2:C]` | `'i` |
| `ni'!` | `[ni@0:2:C]` | `'!` |
| `ni''hao` | `[ni@0:2:C, hao@4:7:C]`; `[ni@0:2:C, ha@4:6:C, o@6:7:C]` | empty |
| `'ni` | `[ni@1:3:C]` | empty |
| `''ni` | `[ni@2:4:C]` | empty |

The empty input has one empty segmentation, not zero paths.
Junk-prefix input also has one result, with no segments and the entire input as the remainder.
These rules make sure that every input under the limit has a representable parse.
"No consumed prefix" is then not the same as parser failure.

## Acceptance relationship

- Unit tests pin this ordered path set, including both `xian` paths and all three `fangan` paths.
- The F-A acceptance test maps each F-A record to a compatible portable path and separately asserts all alternatives.
- Oracle candidate ranking and its selected segmentation do not reorder or remove portable parser paths.

## Architect correction log

- 2026-08-09, Task 7: the `chang'an` example includes the complete `[cha, ng, an]` alternative.
  The exhaustive rule above already required that table-valid path; the first version of the row omitted it.
- 2026-08-21, W12 Class B: the `ni''hao` example shows both `[ni, hao]` and `[ni, ha, o]` with an empty remainder.
  Consecutive apostrophes are a single separator when the right group consumes at least one byte.
  This matches the pin: `fixtures/w4/oracle-paths.txt` records `ni''hao` with consumed length 7 and the path `ni@0:2`, `hao@4:7`.
  Recorded evidence: the pin's top-1 candidate for `ni''hao` is `你好`; the reduced `fixtures/w4/oracle-candidate-structure.txt` does not hold that input.
  Cross-referenced in the [parser-spec.md correction log](parser-spec.md#architect-correction-log) (2026-08-21); this closes the doubled-apostrophe half of maintainer decision 3.
- 2026-08-21, W12 decision 3 leading half: `'ni` and `''ni` added.
  A leading run of consecutive apostrophes is consumed when the group after it consumes at least one byte.
  This matches the pin's zero-width step-propagation at position 0.
  Cross-referenced in the [parser-spec.md correction log](parser-spec.md#architect-correction-log) (2026-08-21, leading apostrophe); this closes the leading-apostrophe half of maintainer decision 3.
