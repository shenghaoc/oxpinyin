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


def string_of(thread: dict, index: int) -> str:
    table = thread.get("stringArray") or thread.get("stringTable") or []
    if 0 <= index < len(table):
        return table[index]
    return f"?{index}"


def frame_name(thread: dict, frame_index: int) -> str:
    frames = thread.get("frameTable") or {}
    funcs = thread.get("funcTable") or {}
    func_col = frames.get("func") or []
    if not (0 <= frame_index < len(func_col)):
        return f"frame:{frame_index}"
    func_index = func_col[frame_index]
    name_col = funcs.get("name") or []
    if not (0 <= func_index < len(name_col)):
        return f"func:{func_index}"
    return string_of(thread, name_col[func_index])


def walk_stack(thread: dict, stack_index: int, limit: int = 12) -> tuple[str, ...]:
    stacks = thread.get("stackTable") or {}
    prefix_col = stacks.get("prefix") or []
    frame_col = stacks.get("frame") or []
    frames: list[str] = []
    seen: set[int] = set()
    current = stack_index
    while current is not None and current >= 0 and current not in seen:
        seen.add(current)
        if current < len(frame_col):
            frames.append(frame_name(thread, frame_col[current]))
        if current >= len(prefix_col):
            break
        current = prefix_col[current]
        if current is None or current < 0:
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
        samples = thread.get("samples") or {}
        stack_col = samples.get("stack") or []
        for stack_index in stack_col:
            if stack_index is None or stack_index < 0:
                continue
            counts[walk_stack(thread, stack_index)] += 1
    return sum(counts.values()), counts.most_common(top_n)


def main() -> int:
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
