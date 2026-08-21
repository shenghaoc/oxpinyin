# Full-pinyin parser path-set SPEC

Date: 2026-08-09 · Status: frozen for Foundation Tasks 6–9

This finding freezes which alternatives the parser returns and their order.
It is normative together with `docs/findings/parser-spec.md`. F-A records the
oracle-selected path only; this path set deliberately returns every valid
complete alternative and leaves selection to the decoder.

## Complete group enumeration

A valid segmentation contains only complete table syllables and consumes its
whole lowercase group. For one group, enumerate valid segmentations with
depth-first backtracking from byte zero:

1. At each cursor, consider every complete table syllable matching the input
   prefix at that cursor.
2. Visit matches by descending byte length (greedy longest match first).
3. Recurse or iterate from the end of each match.
4. Emit a path only when it consumes the whole group.

A fixed input prefix identifies at most one syllable of a given length, so no
secondary tie-break is normally needed. If the inventory ever changes to
permit a tie, compare syllable bytes ascending. Remove duplicate paths while
preserving first occurrence.

Under this exhaustive rule, 164 of the 405 complete table entries admit more
than one complete segmentation of the entry alone; round-trip tests assert
that the greedy identity path is present as the first path, not that it is
the only path.

For multiple apostrophe-separated groups, preserve each group’s order and
form the Cartesian product left-to-right: for each left path in order, append
each right path in order. Apostrophes are consumed separators and never
segments. A separator is consumed only if the right side consumes at least one
byte as a complete or terminal partial segment.

## Partial fallback

Partial-tail results satisfy R3.3/R3.4 but are not valid segmentations under
R3.7. Consider them only when a group has no complete path. Enumerate paths
consisting of zero or more complete syllables followed by one non-empty proper
prefix of a table syllable. Keep only paths whose final end offset is maximal.
Order retained paths by the same greedy depth-first order and remove
duplicates preserving first occurrence.

If neither a complete nor a partial continuation exists at a cursor, retain
the complete prefix already obtained and start the remainder at that cursor.
When this happens before an apostrophe, retain the group’s maximal partial if
one exists, stop before the apostrophe, and leave the apostrophe plus all
following bytes in the remainder.

Complete-path precedence is required: partial alternatives are not returned
when any complete segmentation consumes the group. Thus `xian` does not also
return `[xi, a, n(partial)]`; the two all-complete paths are its complete path
set.

## Result bound

Count the final deduplicated path set before materialization, saturating at
4,097. A count over `MAX_PARSE_RESULTS` (4,096) returns
`ParseError::TooManyAlternatives { limit: 4_096 }` and no partial vector. A
count at or below the limit is emitted exhaustively in the order above. The
bound applies after Cartesian products and after maximal-partial filtering.

## Frozen examples

Notation is `text@start:end:C` for complete and `text@start:end:P` for
partial. Paths appear below in required return order.

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

The empty input has one empty segmentation rather than zero paths. Junk-prefix
input likewise has one result with no segments and the entire input as the
remainder. These rules guarantee that every non-over-limit input has a
representable parse without conflating “no consumed prefix” with parser
failure.

## Acceptance relationship

Task 7 unit tests pin this ordered path set, including both `xian` paths and
all three `fangan` paths. Task 8 maps each F-A record to a compatible portable
path while separately asserting all alternatives. Oracle candidate ranking
and its selected segmentation do not reorder or remove portable parser paths.

## Architect correction log

- 2026-08-09, Task 7: the `chang'an` example now includes the complete
  `[cha, ng, an]` alternative. The original row omitted that table-valid path
  even though the exhaustive rule above already required it.
- 2026-08-21, W12 Class B: the `ni''hao` example now shows both `[ni, hao]`
  and `[ni, ha, o]` with an empty remainder. Consecutive apostrophes are a
  single separator when the right group consumes at least one byte; matches
  the pin (`fixtures/w4/oracle-candidates.txt` `ni''hao` top-1 `你好`).
  Cross-referenced in `parser-spec.md` architect correction log
  (2026-08-21) and `parser-spec-contradiction-incomplete-keys.md`
  (decision 3, doubled half now closed).
- 2026-08-21, W12 decision 3 leading half: `'ni` and `''ni` added.
  A leading run of consecutive apostrophes is consumed when the group
  after it consumes at least one byte, matching the pin's zero-width
  step-propagation at position 0. Cross-referenced in `parser-spec.md`
  architect correction log (2026-08-21, leading apostrophe) and
  `parser-spec-contradiction-incomplete-keys.md` (decision 3, leading
  half now closed).
