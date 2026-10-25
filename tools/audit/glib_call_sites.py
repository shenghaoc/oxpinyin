#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Regenerate the GLib/libc call-site TSV from a pinned libpinyin checkout.

The pin checkout must be at 074a2219c90feaf962d0d24f034514033ece5f99.
The scanner yields the mechanical columns (library, pin_site, pin_call,
call and a reach it can derive); the committed classification file adds
family, facade, backend, child and any reach the scan cannot derive.
"""
from __future__ import annotations

import argparse
import csv
import io
import re
import subprocess
import sys
from pathlib import Path

PIN = "074a2219c90feaf962d0d24f034514033ece5f99"
ROOT = Path(__file__).resolve().parents[2]
CLASSIFICATION = ROOT / "tools/audit/glib_call_sites.classification.tsv"
OUTPUT = ROOT / "docs/findings/glib-libc-call-sites.tsv"
HEADER = ("family", "library", "facade", "backend", "pin_site",
          "pin_call", "child", "reach", "call")
CLASSIFICATION_HEADER = ("pin_site", "pin_call", "family", "facade",
                         "backend", "child", "reach")

GLIB = re.compile(r"\bg_[A-Za-z0-9_]+\b")
LIBC = re.compile(
    r"\b(?:fscanf|sscanf|scanf|atoi|strtol|strtoul|strtoull|setlocale|"
    r"fopen|fclose|fprintf|printf|snprintf|vsnprintf|rename|fsync|"
    r"mmap|munmap|lseek|open|close|read|write|unlink|malloc|calloc|"
    r"realloc|free|qsort|bsearch|strlen|strcmp|strncmp|strcpy|strncpy|"
    r"strcat|strncat|strchr|strrchr|strstr|strspn|strcspn|memcpy|"
    r"memmove|memset|memcmp|log|logf|exp|expf|pow|powf|sqrt|sqrtf|"
    r"floor|ceil|round|fabs|hypot|sin|cos|tan|asin|acos|atan|atan2|"
    r"sinh|cosh|tanh)\b")
CALL = re.compile(GLIB.pattern + "|" + LIBC.pattern)

TOOL_FILES = {
    "src/storage/tag_utility.cpp",
    "src/storage/table_text_parser.h",
}
TOOL_RANGES = {
    "src/storage/chewing_large_table2.cpp": ((112, 155),),
    "src/storage/phrase_large_table3.cpp": ((123, 140),),
    "src/storage/phrase_index.cpp": ((522, 590),),
    "src/storage/punct_table.cpp": ((180, 200),),
}
DEAD_FILES = {
    "src/lookup/pinyin_lookup2.cpp",
    "src/storage/chewing_large_table.cpp",
    "src/storage/phrase_large_table2.cpp",
    "src/storage/facade_chewing_table.h",
    "src/storage/facade_phrase_table2.h",
}
DEAD_RANGES = {
    "src/lookup/phonetic_lookup.cpp": ((25, 64),),
    "src/storage/phonetic_key_matrix.cpp": ((319, 377),),
    "src/storage/punct_table.cpp": ((40, 54), (97, 110), (117, 145)),
}
MEMORY_CHUNK = "src/include/memory_chunk.h"


def check_pin(pin: Path) -> None:
    """Require the checkout to report the pinned commit."""
    try:
        head = subprocess.check_output(
            ["git", "-C", str(pin), "rev-parse", "HEAD"], text=True,
            stderr=subprocess.PIPE).strip()
    except (OSError, subprocess.CalledProcessError) as error:
        raise ValueError(f"{pin} is not a git checkout: {error}") from error
    if head != PIN:
        raise ValueError(f"{pin} is at {head}, expected {PIN}")


def if_zero_lines(text: str) -> set[int]:
    """Line numbers inside an outermost `#if 0` block."""
    lines: set[int] = set()
    stack: list[int] = []
    for number, line in enumerate(text.splitlines(), 1):
        if re.match(r"\s*#\s*if\s+0\b", line):
            stack.append(number)
        elif re.match(r"\s*#\s*endif\b", line) and stack:
            start = stack.pop()
            if not stack:
                lines.update(range(start, number + 1))
    return lines


def derive_reach(relative: str, line: int, dead_lines: set[int]) -> str:
    """The reach the scanner can establish without the classification file."""
    if relative in TOOL_FILES:
        return "tool-only"
    if relative in DEAD_FILES or line in dead_lines:
        return "dead"
    for start, end in TOOL_RANGES.get(relative, ()):
        if start <= line <= end:
            return "tool-only"
    for start, end in DEAD_RANGES.get(relative, ()):
        if start <= line <= end:
            return "dead"
    return "exported"


def call_text(text: str, start: int) -> str:
    """The call from its name through its matching close parenthesis."""
    depth = 0
    quoted = False
    escaped = False
    index = start
    while index < len(text):
        character = text[index]
        if quoted:
            if escaped:
                escaped = False
            elif character == "\\":
                escaped = True
            elif character == '"':
                quoted = False
        elif character == '"':
            quoted = True
        elif character == "(":
            depth += 1
        elif character == ")":
            depth -= 1
            if depth == 0:
                index += 1
                break
        index += 1
    return " ".join(text[start:index].split())


def scan(pin: Path) -> list[dict[str, str]]:
    """Scan the pin's source for the mechanical columns."""
    files = sorted(pin.glob("src/**/*.cpp")) + sorted(pin.glob("src/**/*.h"))
    rows: list[dict[str, str]] = []
    seen: set[tuple[str, int, str]] = set()
    for path in files:
        relative = path.relative_to(pin).as_posix()
        text = path.read_text(errors="ignore")
        dead_lines = if_zero_lines(text)
        for match in CALL.finditer(text):
            name = match.group(0)
            following = match.end()
            while following < len(text) and text[following] in " \t\r\n":
                following += 1
            if following >= len(text) or text[following] != "(":
                continue
            line = text.count("\n", 0, match.start()) + 1
            prefix = text[text.rfind("\n", 0, match.start()) + 1:match.start()]
            if "//" in prefix or prefix.strip().startswith(("*", "/*")):
                continue
            if name in ("open", "close", "read", "write") and relative != MEMORY_CHUNK:
                continue
            key = (relative, line, name)
            if key in seen:
                continue
            seen.add(key)
            rows.append({
                "library": "glib" if name.startswith("g_") else "libc",
                "pin_site": f"{relative}:{line}",
                "pin_call": name,
                "call": call_text(text, match.start()),
                "reach": derive_reach(relative, line, dead_lines),
            })
    return rows


def read_classification(path: Path) -> dict[tuple[str, str], dict[str, str]]:
    """Read the hand-maintained classification, keyed by site and call."""
    with path.open(newline="") as stream:
        reader = csv.DictReader(stream, delimiter="\t")
        if tuple(reader.fieldnames or ()) != CLASSIFICATION_HEADER:
            raise ValueError(
                f"{path}: header must be {CLASSIFICATION_HEADER}")
        entries: dict[tuple[str, str], dict[str, str]] = {}
        for row in reader:
            if set(row) != set(CLASSIFICATION_HEADER):
                raise ValueError(f"{path}: row has the wrong fields")
            key = (row["pin_site"], row["pin_call"])
            if key in entries:
                raise ValueError(f"{path}: duplicate key {key}")
            entries[key] = row
    return entries


def build_rows(pin: Path,
               classification: dict[tuple[str, str], dict[str, str]]
               ) -> list[dict[str, str]]:
    """Join the scanned mechanical columns to the classification."""
    rows = scan(pin)
    seen: set[tuple[str, str]] = set()
    result: list[dict[str, str]] = []
    for row in rows:
        key = (row["pin_site"], row["pin_call"])
        seen.add(key)
        entry = classification.get(key)
        if entry is None:
            raise ValueError(f"no classification for {key[0]} {key[1]}")
        reach = entry["reach"] or row["reach"]
        result.append({
            "family": entry["family"],
            "library": row["library"],
            "facade": entry["facade"],
            "backend": entry["backend"],
            "pin_site": row["pin_site"],
            "pin_call": row["pin_call"],
            "child": entry["child"],
            "reach": reach,
            "call": row["call"],
        })
    extra = sorted(set(classification) - seen)
    if extra:
        raise ValueError(f"classification has {len(extra)} stale rows, "
                         f"first {extra[0][0]} {extra[0][1]}")
    result.sort(key=lambda row: (
        row["family"], row["pin_site"].rsplit(":", 1)[0],
        int(row["pin_site"].rsplit(":", 1)[1]), row["pin_call"], row["call"]))
    return result


def render(rows: list[dict[str, str]]) -> str:
    """Render the TSV with the repository's CSV dialect."""
    buffer = io.StringIO()
    writer = csv.writer(buffer, delimiter="\t", quoting=csv.QUOTE_MINIMAL,
                        lineterminator="\n")
    writer.writerow(HEADER)
    for row in rows:
        writer.writerow([row[column] for column in HEADER])
    return buffer.getvalue()


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("pin", type=Path, help="libpinyin checkout at the pin")
    parser.add_argument("--classification", type=Path, default=CLASSIFICATION)
    parser.add_argument("--output", type=Path, default=OUTPUT)
    parser.add_argument("--check", action="store_true",
                        help="compare with --output instead of writing it")
    args = parser.parse_args(argv)
    try:
        check_pin(args.pin)
        entries = read_classification(args.classification)
        text = render(build_rows(args.pin, entries))
    except (OSError, ValueError) as error:
        print(f"glib_call_sites: {error}", file=sys.stderr)
        return 1
    if args.check:
        current = args.output.read_text() if args.output.exists() else ""
        if current != text:
            print(f"glib_call_sites: {args.output} is stale", file=sys.stderr)
            return 1
        return 0
    args.output.write_text(text)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
