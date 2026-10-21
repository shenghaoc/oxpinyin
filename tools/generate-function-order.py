#!/usr/bin/env -S uv run --no-project --managed-python --python cpython@3.15.0+gil python -X lazy_imports=normal
"""Derive optional shipping-layout policies from measured Callgrind streams.

Reads captures only; does not build, profile, write a capture, or run a
workload. Each init stream must stop after init (including load); each steady
stream must collect only the existing steady keystroke/guess workload.
Use --demangle=no for Callgrind. nm supplies canonical demangled identity.
Both facades and both startup profiles are required, with no default policy.
"""

import argparse
from collections import defaultdict
from contextlib import nullcontext
import os
from pathlib import Path
import re
import subprocess
import sys


FACADES = ("pinyin", "zhuyin")
PROFILES = ("phrases", "journals")
HERE = Path(__file__).resolve().parent


def canonical(name):
    """Same normalization as packaging/function-order.awk; no fuzzy matches."""
    name = re.sub(r"\.llvm\.[0-9a-fA-F]+$", "", name)
    return re.sub(r"::h[0-9a-fA-F]{16}$", "", name)


def run(command):
    result = subprocess.run(command, capture_output=True, text=True, check=False,
                            env={**os.environ, "LC_ALL": "C"})
    if result.returncode:
        raise ValueError(f"{command[0]} failed: {result.stderr.strip()}")
    return result.stdout


def symbols(text):
    """Parse nm sysv records from the right, preserving bars inside names."""
    rows = []
    for line in text.splitlines():
        fields = line.rsplit("|", 6)
        if len(fields) == 7 and fields[3].strip() == "FUNC":
            rows.append(tuple(field.strip() for field in fields))
    return rows


def library_symbols(library):
    command = ["nm", "--format=sysv", "--defined-only", "--no-sort"]
    raw = symbols(run([*command, str(library)]))
    demangled = symbols(run([*command, "--demangle=rust", str(library)]))
    if len(raw) != len(demangled):
        raise ValueError("raw/demangled symbol-table lengths differ")
    result = {}
    for original, decoded in zip(raw, demangled, strict=True):
        if original[1:] != decoded[1:]:
            raise ValueError("raw/demangled symbol-table records disagree")
        identity = canonical(decoded[0])
        value = identity, decoded[6]
        if original[0] in result and result[original[0]] != value:
            raise ValueError(f"ambiguous library symbol: {original[0]}")
        result[original[0]] = value
    return result


def archive_paths(archive):
    result = defaultdict(set)
    text = run(["bash", str(HERE / "packaging/function-order.sh"), "inventory", str(archive)])
    for line in text.splitlines():
        name, section = line.split("\t")
        result[name].add(section)
    return result


def callgrind_functions(lines, library):
    """Positive exclusive Ir only, scoped to the exact measured object.

    Name-compression dictionaries are shared by current/callee records.
    Inclusive call-edge costs never imply that an ancestor executed code.
    Reconcile all exclusive costs with totals to reject truncated/misparsed
    input instead of generating an incomplete policy.
    """
    tables = {"ob": {}, "fn": {}}
    current_object = current_function = ""
    events = positions = None
    skip_cost = False
    total = 0
    expected = None
    functions = set()

    def decode(kind, body):
        match = re.fullmatch(r"\((\d+)\)(?:\s+(.*))?", body)
        if not match:
            return body
        number, name = match.groups()
        if name is not None:
            tables[kind][number] = name
        if number not in tables[kind]:
            raise ValueError(f"undefined Callgrind {kind} name: {number}")
        return tables[kind][number]

    for raw in lines:
        line = raw.rstrip("\n")
        if line.startswith("events:"):
            if events is not None:
                raise ValueError("multiple Callgrind event headers; provide one dump per stream")
            events = line.split()[1:]
            if "Ir" not in events:
                raise ValueError("Callgrind events lack Ir")
        elif line.startswith("positions:"):
            positions = len(line.split()[1:])
            if not positions:
                raise ValueError("Callgrind positions are empty")
        elif match := re.match(r"^(c?)(ob|fn)=(.*)$", line):
            callee, kind, body = match.groups()
            value = decode(kind, body)
            if not callee:
                if kind == "ob":
                    current_object = value
                else:
                    current_function = value
        elif line.startswith("calls="):
            if skip_cost:
                raise ValueError("Callgrind call association lacks a cost")
            skip_cost = True
        elif line.startswith("totals:"):
            if events is None or expected is not None:
                raise ValueError("invalid or repeated Callgrind totals")
            counts = line.split()[1:]
            if len(counts) != len(events):
                raise ValueError("Callgrind totals do not match events")
            expected = int(counts[events.index("Ir")])
        elif line and line[0] in "0123456789+-*":
            if events is None or positions is None:
                raise ValueError("Callgrind cost before events/positions")
            if expected is not None:
                raise ValueError("Callgrind cost after totals")
            fields = line.split()
            if len(fields) <= positions:
                raise ValueError("Callgrind cost lacks event counts")
            counts = [int(value) for value in fields[positions:]]
            if len(counts) > len(events) or any(value < 0 for value in counts):
                raise ValueError("invalid Callgrind event counts")
            counts.extend([0] * (len(events) - len(counts)))
            if skip_cost:
                skip_cost = False
                continue
            ir = counts[events.index("Ir")]
            total += ir
            if ir and current_object and Path(current_object).resolve() == library.resolve():
                if not current_function:
                    raise ValueError("library cost lacks function identity")
                functions.add(current_function)
    if skip_cost or expected is None or expected != total:
        raise ValueError(f"Callgrind exclusive Ir does not reconcile: {total} vs {expected}")
    if not functions:
        raise ValueError(f"no executed functions for exact library object {library}")
    return functions


def measured_paths(stream, library, symbol_table, available):
    with (nullcontext(sys.stdin) if str(stream) == "-" else stream.open()) as lines:
        functions = callgrind_functions(lines, library)
    result = set()
    fixed = set()
    for function in sorted(functions):
        if function not in symbol_table:
            raise ValueError(f"unresolved measured symbol {function!r}; collect with --demangle=no")
        name, section = symbol_table[function]
        if name not in available:
            # The loader's .init/.fini and CRT .text functions are not
            # individually orderable staticlib sections. Rust must resolve.
            if function.startswith(("_R", "_ZN")):
                raise ValueError(f"Rust function has no orderable archive section: {name}")
            fixed.add(name)
            continue
        if len(available[name]) != 1:
            raise ValueError(f"ambiguous stable path: {name}")
        if section != ".text":
            raise ValueError(f"archive function resolved outside library .text: {name}")
        if (name != canonical(name) or re.match(r"^(_R|_ZN[0-9])", name)
                or any(ord(c) < 32 or ord(c) == 127 for c in name)):
            raise ValueError(f"invalid stable path: {name!r}")
        result.add(name)
    print(f"{library.name}: {len(result)} orderable paths; {len(fixed)} fixed/linker paths", file=sys.stderr)
    if not result:
        raise ValueError("measured stream has no orderable functions")
    return result


def pairs(values, expected, option):
    result = {}
    for value in values:
        key, separator, path = value.partition("=")
        if not separator or not path or key not in expected or key in result:
            raise ValueError(f"invalid/duplicate {option}: {value!r}")
        result[key] = Path(path)
    if result.keys() != set(expected):
        raise ValueError(f"{option} requires exactly: {', '.join(expected)}")
    return result


def policy(facade, startup, steady):
    rows = [f"oxpinyin-function-order-v1\tlib{facade}.so.15"]
    for tier, names in (("init", startup), ("steady", steady - startup)):
        rows.extend(f"{tier}\t{name}" for name in sorted(names))
    return "\n".join(rows) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--library", action="append", default=[], metavar="FACADE=SO")
    parser.add_argument("--staticlib", action="append", default=[], metavar="FACADE=ARCHIVE")
    parser.add_argument("--init", action="append", default=[], metavar="FACADE:PROFILE=STREAM")
    parser.add_argument("--steady", action="append", default=[], metavar="FACADE=STREAM")
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    try:
        libraries = pairs(args.library, FACADES, "--library")
        archives = pairs(args.staticlib, FACADES, "--staticlib")
        startup = pairs(args.init, tuple(f"{f}:{p}" for f in FACADES for p in PROFILES), "--init")
        steady = pairs(args.steady, FACADES, "--steady")
        if sum(str(path) == "-" for path in (*startup.values(), *steady.values())) > 1:
            raise ValueError("stdin can supply only one stream")
        outputs = {}
        for facade in FACADES:
            library = libraries[facade]
            symbol_table = library_symbols(library)
            available = archive_paths(archives[facade])
            init = set()
            for profile in PROFILES:
                init |= measured_paths(startup[f"{facade}:{profile}"], library, symbol_table, available)
            hot = measured_paths(steady[facade], library, symbol_table, available)
            outputs[f"lib{facade}.order"] = policy(facade, init, hot)
        # Validate all streams before writing either policy. No raw data is
        # copied; the only outputs are the production-layout policy inputs.
        args.output_dir.mkdir(parents=True, exist_ok=True)
        for name, contents in outputs.items():
            (args.output_dir / name).write_text(contents, encoding="utf-8", newline="\n")
    except (OSError, ValueError) as error:
        parser.exit(1, f"function ordering: {error}\n")


if __name__ == "__main__":
    main()
