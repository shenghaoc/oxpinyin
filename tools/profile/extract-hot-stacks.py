#!/usr/bin/env -S uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal
"""Print the hottest stacks from a samply / Firefox-profiler JSON dump.

Reads `target/profile/w8-cycle.profile.json.gz` (or a path argument) and
prints the top frames by sample self-count. A samply symbol sidecar
(`<profile>.syms.json`, written by
`samply record --unstable-presymbolicate`) supplies function names for
save-only profiles. The report opens with the profile's sample total. A total
below MIN_SAMPLES also draws a warning on stderr, because the order of the
stacks is then mostly sampling noise. `--total` prints only that line, which
is what `run-w8-cycle.sh` shows after a record. Used to fill the Stage-2
findings note; not a CI gate.
"""

from __future__ import annotations

import argparse
import gzip
import json
import re
import sys
import zlib
from collections import Counter
from pathlib import Path

RAW_ADDRESS = re.compile(r"^0x[0-9a-fA-F]+$")
UNRESOLVED = "unresolved"
DEFAULT_PROFILE = Path("target/profile/w8-cycle.profile.json.gz")
# Samples needed to rank hot stacks: about twice the size from which the top two
# stacks kept their places in every repeated run. The basis is in
# docs/runbooks/benches.md, "Profiles".
MIN_SAMPLES = 5000


def load_profile(path: Path) -> dict:
    if path.suffix == ".gz" or str(path).endswith(".json.gz"):
        with gzip.open(path, "rt", encoding="utf-8") as handle:
            profile = json.load(handle)
    else:
        with path.open(encoding="utf-8") as handle:
            profile = json.load(handle)
    if not isinstance(profile, dict):
        raise ValueError("the top-level JSON value is not an object")
    return profile


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


def _column_value(table: object, name: str, index: int):
    values = column(table, name)
    if 0 <= index < len(values):
        return values[index]
    return None


def _index(value: object) -> bool:
    return isinstance(value, int) and not isinstance(value, bool) and value >= 0


def _int_at(table: object, name: str, index: int) -> int | None:
    value = _column_value(table, name, index)
    return value if _index(value) else None


def sidecar_path(profile_path: Path) -> Path:
    """The sidecar path samply derives with `Path::with_extension`."""
    return profile_path.with_suffix(".syms.json")


def load_sidecar(profile_path: Path) -> dict | None:
    """Load the JSON symbol sidecar beside a profile, or None when absent."""
    path = sidecar_path(profile_path)
    if not path.is_file():
        return None
    try:
        with path.open(encoding="utf-8") as handle:
            data = json.load(handle)
    except (OSError, json.JSONDecodeError):
        return None
    return data if isinstance(data, dict) else None


def _sidecar_entries(sidecar: dict) -> list[dict]:
    """Unpack `string_table` + per-library `known_addresses` into lookups."""
    strings = sidecar.get("string_table")
    data = sidecar.get("data")
    if not isinstance(strings, list) or not isinstance(data, list):
        return []
    entries: list[dict] = []
    for raw in data:
        if not isinstance(raw, dict):
            continue
        symbols = raw.get("symbol_table")
        known = raw.get("known_addresses")
        if not isinstance(symbols, list) or not isinstance(known, list):
            continue
        by_rva: dict[int, str] = {}
        for pair in known:
            if not isinstance(pair, list) or len(pair) != 2:
                continue
            rva, symbol_index = pair
            if (
                not _index(rva)
                or not _index(symbol_index)
                or symbol_index >= len(symbols)
            ):
                continue
            symbol = symbols[symbol_index]
            name_index = symbol.get("symbol") if isinstance(symbol, dict) else None
            if not _index(name_index) or name_index >= len(strings):
                continue
            name = strings[name_index]
            if (
                isinstance(name, str)
                and name
                and name != "UNKNOWN"
                and not RAW_ADDRESS.match(name)
            ):
                by_rva[rva] = name
        entries.append(
            {
                "debug_name": raw.get("debug_name"),
                "code_id": raw.get("code_id"),
                "by_rva": by_rva,
            }
        )
    return entries


def _lib_symbols(profile: dict, entries: list[dict]) -> dict[int, dict]:
    """Map profile lib indexes to sidecar entries by debug name and code ID."""
    libs = profile.get("libs")
    if not isinstance(libs, list):
        return {}
    by_name: dict[str, list[dict]] = {}
    by_key: dict[tuple[str, str], list[dict]] = {}
    for entry in entries:
        name = entry["debug_name"]
        if not isinstance(name, str):
            continue
        by_name.setdefault(name, []).append(entry)
        code = entry["code_id"]
        if isinstance(code, str):
            by_key.setdefault((name, code.lower()), []).append(entry)
    mapped: dict[int, dict] = {}
    for index, lib in enumerate(libs):
        if not isinstance(lib, dict):
            continue
        name = lib.get("debugName") or lib.get("name")
        if not isinstance(name, str):
            continue
        code = lib.get("codeId")
        matches = (
            by_key.get((name, code.lower())) if isinstance(code, str) else None
        )
        if not matches:
            matches = by_name.get(name)
        if matches and len(matches) == 1:
            mapped[index] = matches[0]
    return mapped


def _lib_for_frame(thread: dict, frame_index: int) -> int | None:
    func_index = _int_at(thread.get("frameTable") or {}, "func", frame_index)
    if func_index is None:
        return None
    resource_index = _int_at(
        thread.get("funcTable") or {}, "resource", func_index
    )
    if resource_index is None:
        return None
    return _int_at(thread.get("resourceTable") or {}, "lib", resource_index)


class SymbolResolver:
    """Resolve a frame's library-relative address through a samply sidecar."""

    def __init__(self, profile: dict, sidecar: dict | None = None):
        entries = _sidecar_entries(sidecar) if isinstance(sidecar, dict) else []
        self._libs = _lib_symbols(profile, entries)
        libs = profile.get("libs")
        self._profile_libs = libs if isinstance(libs, list) else []

    def lib_name(self, lib_index: int) -> str | None:
        if 0 <= lib_index < len(self._profile_libs):
            lib = self._profile_libs[lib_index]
            if isinstance(lib, dict):
                name = lib.get("debugName") or lib.get("name")
                if isinstance(name, str) and name:
                    return name
        return None

    def resolve(self, thread: dict, frame_index: int) -> str | None:
        lib_index = _lib_for_frame(thread, frame_index)
        address = _int_at(
            thread.get("frameTable") or {}, "address", frame_index
        )
        if lib_index is None or address is None:
            return None
        entry = self._libs.get(lib_index)
        return entry["by_rva"].get(address) if entry else None


def _profile_name(thread: dict, frame_index: int) -> str | None:
    func_index = _int_at(thread.get("frameTable") or {}, "func", frame_index)
    if func_index is None:
        return None
    name_index = _int_at(thread.get("funcTable") or {}, "name", func_index)
    if name_index is None:
        return None
    name = string_of(thread, name_index)
    return name if isinstance(name, str) else None


def _valid_name(name: str | None) -> bool:
    return bool(
        name
        and not RAW_ADDRESS.match(name)
        and not name.startswith(("frame:", "func:", "?"))
    )


Frame = tuple[tuple, str]  # (aggregation identity, rendered label)


def frame_identity(
    thread: dict,
    frame_index: int,
    symbols: SymbolResolver | None = None,
    thread_index: int = 0,
) -> Frame:
    """Return a frame's aggregation identity and its display label.

    A resolved name is its own identity. An unresolved frame keeps its
    library and RVA, or its (thread, frame) index when those are
    unavailable, so distinct frames never share a counter key.
    """
    if symbols is not None:
        resolved = symbols.resolve(thread, frame_index)
        if resolved:
            return ("name", resolved), resolved
    name = _profile_name(thread, frame_index)
    if _valid_name(name):
        return ("name", name), name
    lib_index = _lib_for_frame(thread, frame_index)
    address = _int_at(thread.get("frameTable") or {}, "address", frame_index)
    if lib_index is not None and address is not None:
        library = symbols.lib_name(lib_index) if symbols is not None else None
        library = library or f"lib#{lib_index}"
        return (
            ("unresolved", lib_index, address),
            f"{UNRESOLVED} ({library}+0x{address:x})",
        )
    return (
        ("frame", thread_index, frame_index),
        f"{UNRESOLVED} (thread {thread_index} frame {frame_index})",
    )


def frame_name(
    thread: dict,
    frame_index: int,
    symbols: SymbolResolver | None = None,
    thread_index: int = 0,
) -> str:
    return frame_identity(thread, frame_index, symbols, thread_index)[1]


def walk_stack(
    thread: dict,
    stack_index: int,
    symbols: SymbolResolver | None = None,
    limit: int = 12,
    thread_index: int = 0,
) -> tuple[Frame, ...]:
    stacks = thread.get("stackTable") or {}
    prefix_col = column(stacks, "prefix")
    frame_col = column(stacks, "frame")
    frames: list[Frame] = []
    seen: set[int] = set()
    current = stack_index
    while isinstance(current, int) and current >= 0 and current not in seen:
        seen.add(current)
        if current >= len(frame_col):
            break
        frame_index = frame_col[current]
        if not isinstance(frame_index, int) or frame_index < 0:
            break
        frames.append(
            frame_identity(thread, frame_index, symbols, thread_index)
        )
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
    profile: dict,
    top_n: int = 12,
    symbols: SymbolResolver | None = None,
) -> tuple[int, list[tuple[tuple[str, ...], int]]]:
    if symbols is None:
        symbols = SymbolResolver(profile)
    counts: Counter[tuple] = Counter()
    labels: dict[tuple, tuple[str, ...]] = {}
    for thread_index, thread in enumerate(profile.get("threads") or []):
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
            stack = walk_stack(
                thread, stack_index, symbols, thread_index=thread_index
            )
            if not stack:
                continue
            identity = tuple(frame[0] for frame in stack)
            counts[identity] += 1
            labels.setdefault(identity, tuple(frame[1] for frame in stack))
    total = sum(counts.values())
    rows = [
        (labels[identity], count)
        for identity, count in counts.most_common(top_n)
    ]
    return total, rows


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
    # Name index is non-int → unresolved by thread/frame index, no TypeError.
    assert bad_total == 2, bad_total
    assert frame_name(bad["threads"][0], 0) == (
        f"{UNRESOLVED} (thread 0 frame 0)"
    )
    print("self-test ok")
    return 0


def report_samples(total: int, level: int, prefix: str = "") -> None:
    """Print the sample total on stdout; below the level, warn on stderr.

    Too few samples is a warning, not a failure: the profile is valid, only
    too short for the order of its stacks to mean anything.
    """
    if total >= level:
        print(
            f"{prefix}samples: {total} (at least the {level} needed to rank "
            "hot stacks)",
            flush=True,
        )
        return
    print(
        f"{prefix}samples: {total} (fewer than the {level} needed to rank "
        "hot stacks)",
        flush=True,
    )
    print(
        f"warning: the profile has {total} of the {level} samples needed to "
        "rank hot stacks; their order is mostly sampling noise. Raise "
        "PERF_CYCLES and record again.",
        file=sys.stderr,
        flush=True,
    )


def _non_negative(text: str) -> int:
    value = int(text)
    if value < 0:
        raise argparse.ArgumentTypeError("must not be negative")
    return value


def _parse_args(argv: list[str] | None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Print the hottest stacks from a samply profile."
    )
    parser.add_argument(
        "profile",
        nargs="?",
        type=Path,
        default=DEFAULT_PROFILE,
        help=f"profile to read (default {DEFAULT_PROFILE})",
    )
    parser.add_argument(
        "--total",
        action="store_true",
        help="print only the sample total, warning when it is below the level",
    )
    parser.add_argument(
        "--min-samples",
        type=_non_negative,
        default=MIN_SAMPLES,
        metavar="N",
        help=f"samples needed to rank hot stacks (default {MIN_SAMPLES})",
    )
    parser.add_argument("--self-test", action="store_true", help=argparse.SUPPRESS)
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = _parse_args(argv)
    if args.self_test:
        return _self_test()
    path = args.profile
    if not path.is_file():
        print(f"fatal: profile not found at {path}", file=sys.stderr)
        return 1
    try:
        profile = load_profile(path)
    except (OSError, EOFError, ValueError, zlib.error) as error:
        print(f"fatal: cannot read the profile at {path}: {error}", file=sys.stderr)
        return 1
    if args.total:
        # The total does not depend on names, so skip the symbol sidecar.
        total, _ = hottest(profile, top_n=0)
        report_samples(total, args.min_samples)
        return 0
    sidecar = load_sidecar(path)
    symbols = SymbolResolver(profile, sidecar)
    total, rows = hottest(profile, symbols=symbols)
    if not rows:
        print("no samples")
        return 1
    print(f"# hottest stacks in {path}")
    sidecar_file = sidecar_path(path)
    if sidecar is None:
        state = "unusable" if sidecar_file.is_file() else "missing"
        print(
            f"# symbol sidecar: {state} at {sidecar_file}; "
            "frames without an in-profile name print as unresolved"
        )
    else:
        print(f"# symbol sidecar: {sidecar_file}")
    report_samples(total, args.min_samples, prefix="# ")
    for rank, (stack, count) in enumerate(rows, start=1):
        pct = 100.0 * count / total if total else 0.0
        leaf = stack[-1] if stack else "?"
        print(f"{rank:2d}. {count:6d}  ({pct:5.1f}%)  {leaf}")
        for frame in stack[-8:]:
            print(f"       {frame}")
        print()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
