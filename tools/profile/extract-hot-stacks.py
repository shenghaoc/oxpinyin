#!/usr/bin/env python3
"""Print the hottest stacks from a samply / Firefox-profiler JSON dump.

Reads `target/profile/w8-cycle.profile.json.gz` (or a path argument) and
prints the top frames by sample self-count. Used to fill the Stage-2
findings note; not a CI gate.
"""

from __future__ import annotations

import gzip
import json
import sys
from collections import Counter
from pathlib import Path


def load_profile(path: Path) -> dict:
    if path.suffix == ".gz" or str(path).endswith(".json.gz"):
        with gzip.open(path, "rt", encoding="utf-8") as handle:
            return json.load(handle)
    with path.open(encoding="utf-8") as handle:
        return json.load(handle)


def column(table: object, name: str) -> list:
    """Direct column arrays win; otherwise unpack Firefox `{schema, data}` rows.

    Processed/samply profiles store `stack`/`prefix`/`frame` as parallel
    arrays. Raw Gecko tables store the same fields as `schema` indexes into
    row-oriented `data`. Prefer the arrays when present so existing
    column-oriented counting is unchanged.
    """
    if not isinstance(table, dict):
        return []
    direct = table.get(name)
    if isinstance(direct, list):
        return direct
    schema = table.get("schema")
    index = schema.get(name) if isinstance(schema, dict) else None
    if not isinstance(index, int):
        return []
    rows = table.get("data") or []
    out: list = []
    for row in rows:
        if isinstance(row, list) and 0 <= index < len(row):
            out.append(row[index])
        else:
            out.append(None)
    return out


def string_of(thread: dict, index: int) -> str:
    table = thread.get("stringArray") or thread.get("stringTable") or []
    if 0 <= index < len(table):
        return table[index]
    return f"?{index}"


def frame_name(thread: dict, frame_index: int) -> str:
    frames = thread.get("frameTable") or {}
    funcs = thread.get("funcTable") or {}
    func_col = column(frames, "func")
    if not (0 <= frame_index < len(func_col)):
        return f"frame:{frame_index}"
    func_index = func_col[frame_index]
    name_col = column(funcs, "name")
    if not isinstance(func_index, int) or not (0 <= func_index < len(name_col)):
        return f"func:{func_index}"
    name_index = name_col[func_index]
    if not isinstance(name_index, int):
        return f"func:{func_index}"
    return string_of(thread, name_index)


def walk_stack(thread: dict, stack_index: int, limit: int = 12) -> tuple[str, ...]:
    stacks = thread.get("stackTable") or {}
    prefix_col = column(stacks, "prefix")
    frame_col = column(stacks, "frame")
    frames: list[str] = []
    seen: set[int] = set()
    current = stack_index
    while isinstance(current, int) and current >= 0 and current not in seen:
        seen.add(current)
        if current >= len(frame_col):
            break
        frame_index = frame_col[current]
        if not isinstance(frame_index, int) or frame_index < 0:
            break
        frames.append(frame_name(thread, frame_index))
        if current >= len(prefix_col):
            break
        current = prefix_col[current]
        if not isinstance(current, int) or current < 0:
            break
        if len(frames) >= limit:
            break
    frames.reverse()
    return tuple(frames)


def hottest(
    profile: dict, top_n: int = 12
) -> tuple[int, list[tuple[int, tuple[str, ...]]]]:
    counts: Counter[tuple[str, ...]] = Counter()
    for thread in profile.get("threads") or []:
        stacks = thread.get("stackTable") or {}
        # Bound by the frame column. A prefix-only row has no frame and
        # must not be walked (that would count an empty stack).
        n_frames = len(column(stacks, "frame"))
        stack_col = column(thread.get("samples") or {}, "stack")
        for stack_index in stack_col:
            if (
                not isinstance(stack_index, int)
                or stack_index < 0
                or stack_index >= n_frames
            ):
                continue
            stack = walk_stack(thread, stack_index)
            if not stack:
                continue
            counts[stack] += 1
    return sum(counts.values()), counts.most_common(top_n)


def _self_test() -> int:
    # Column-oriented (processed / samply): unchanged counting.
    column_thread = {
        "stringArray": ["root", "leaf"],
        "samples": {"stack": [1, 1]},
        "stackTable": {"prefix": [None, 0], "frame": [0, 1]},
        "frameTable": {"func": [0, 1]},
        "funcTable": {"name": [0, 1]},
    }
    # Row-oriented (raw Firefox): schema + data.
    row_thread = {
        "stringTable": ["root", "leaf"],
        "samples": {"schema": {"stack": 0}, "data": [[1], [1]]},
        "stackTable": {"schema": {"prefix": 0, "frame": 1}, "data": [[None, 0], [0, 1]]},
        "frameTable": {"schema": {"func": 0}, "data": [[0], [1]]},
        "funcTable": {"schema": {"name": 0}, "data": [[0], [1]]},
    }
    mixed = {
        "threads": [
            column_thread,
            row_thread,
            # Direct columns present: ignore leftover schema/data.
            {
                "stringArray": ["only"],
                "samples": {
                    "stack": [0],
                    "schema": {"stack": 0},
                    "data": [[99], [99]],
                },
                "stackTable": {"prefix": [None], "frame": [0]},
                "frameTable": {"func": [0]},
                "funcTable": {"name": [0]},
            },
        ]
    }
    total, rows = hottest(mixed)
    assert total == 5, total
    assert rows == [
        (("root", "leaf"), 4),
        (("only",), 1),
    ], rows
    prefix_only = {
        "threads": [
            {
                "stringArray": ["ok"],
                "samples": {"stack": [0, 1]},
                "stackTable": {
                    "prefix": [None, None],
                    "frame": [0],
                },
                "frameTable": {"func": [0]},
                "funcTable": {"name": [0]},
            }
        ]
    }
    prefix_total, prefix_rows = hottest(prefix_only)
    # Sample 1 points at a prefix row with no matching frame: skip it,
    # do not count an empty stack.
    assert prefix_total == 1, prefix_total
    assert () not in {stack for stack, _ in prefix_rows}
    assert prefix_rows == [(("ok",), 1)], prefix_rows
    invalid_frame = {
        "threads": [
            {
                "stringArray": ["ok"],
                "samples": {"stack": [0, 1]},
                "stackTable": {
                    "prefix": [None, None],
                    "frame": [0, None],
                },
                "frameTable": {"func": [0]},
                "funcTable": {"name": [0]},
            }
        ]
    }
    invalid_total, invalid_rows = hottest(invalid_frame)
    # Sample 1 is in range of the frame column but the frame value is
    # invalid: walk_stack returns (); do not count it.
    assert invalid_total == 1, invalid_total
    assert () not in {stack for stack, _ in invalid_rows}
    assert invalid_rows == [(("ok",), 1)], invalid_rows
    bad = {
        "threads": [
            {
                "stringArray": ["ok"],
                "samples": {"stack": [0, "x", None, -1, 99, 0]},
                "stackTable": {
                    "prefix": [None, "nope"],
                    "frame": [0, "bad"],
                },
                "frameTable": {"func": [0]},
                "funcTable": {"name": ["not-an-int"]},
            }
        ]
    }
    bad_total, _ = hottest(bad)
    # Two in-range integer samples (the 0s); the rest are skipped.
    # Name index is non-int → func fallback, no TypeError.
    assert bad_total == 2, bad_total
    assert frame_name(bad["threads"][0], 0).startswith("func:")
    print("self-test ok")
    return 0


def main() -> int:
    if len(sys.argv) > 1 and sys.argv[1] == "--self-test":
        return _self_test()
    default = Path("target/profile/w8-cycle.profile.json.gz")
    path = Path(sys.argv[1]) if len(sys.argv) > 1 else default
    if not path.is_file():
        print(f"fatal: profile not found at {path}", file=sys.stderr)
        return 1
    profile = load_profile(path)
    total, rows = hottest(profile)
    if not rows:
        print("no samples")
        return 1
    print(f"# hottest stacks in {path}")
    for rank, (count, stack) in enumerate(rows, start=1):
        pct = 100.0 * count / total if total else 0.0
        leaf = stack[-1] if stack else "?"
        print(f"{rank:2d}. {count:6d}  ({pct:5.1f}%)  {leaf}")
        for frame in stack[-8:]:
            print(f"       {frame}")
        print()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
