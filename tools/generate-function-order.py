#!/usr/bin/env -S uv run --no-project --managed-python --python cpython@3.15.0+gil python -X lazy_imports=normal
"""Derive optional shipping-layout policies from measured Callgrind streams.

By default reads streams without running a workload. --bisect collects them
in memory with the in-tree teardown worker; run that mode through just heavy.
Neither mode writes a capture or builds a driver.
Each facade's startup set is the load constructors plus the measured
pinyin_init/zhuyin_init subtree, from the in-tree bisect worker. The steady
set is the existing bisect perf workload. Use --demangle=no for Callgrind.
nm supplies canonical demangled identity. Both facades and both startup
profiles are required, with no default policy.
"""

import argparse
from collections import defaultdict
from contextlib import contextmanager, nullcontext
import io
import json
import os
from pathlib import Path
import re
import runpy
import shutil
import signal
import subprocess
import sys
import tempfile


FACADES = ("pinyin", "zhuyin")
PROFILES = ("phrases", "journals")
HERE = Path(__file__).resolve().parent
# The startup-profile manifest, shared with the teardown provisioner that
# writes it; also the system-data identity.
STARTUP_PROFILE = runpy.run_path(str(HERE / "bisection/startup_profile.py"))
manifest = STARTUP_PROFILE["manifest"]


def canonical(name):
    """Same normalization as packaging/function-order.awk; no fuzzy matches."""
    name = re.sub(r"\.llvm\.[0-9a-fA-F]+$", "", name)
    return re.sub(r"::h[0-9a-fA-F]{16}$", "", name)


def section_family(section):
    """Opaque proof of a compiler-clone family, never a policy sort key."""
    raw = re.sub(r"^\.text\.(?:(?:unlikely|hot|startup|exit)\.)?", "", section)
    raw = re.sub(r"\.llvm\.[0-9a-fA-F]+$", "", raw)
    return re.sub(r"\.[0-9]+$", "", raw)


def stable_identity(name, raw):
    """Keep a hash-free instantiating crate when v0 encodes a crate root.

    GNU nm omits this optional demangled component. Two live generic bodies
    can otherwise have the same displayed path. Decode only a validated
    terminal root, never use its disambiguator for identity or ordering.
    Backref/unsupported contexts remain conservative: ambiguity is rejected.
    https://doc.rust-lang.org/rustc/symbol-mangling/v0.html#instantiating-crate
    """
    name = canonical(name)
    raw = re.sub(r"\.llvm\.[0-9a-fA-F]+$", "", raw)
    raw = re.sub(r"\.[0-9]+$", "", raw)
    if raw.startswith("_R"):
        roots = []
        for match in re.finditer(r"(?=Cs[0-9A-Za-z]+_([0-9]+)([A-Za-z_][0-9A-Za-z_]*)$)", raw):
            length, crate = match.groups()
            if int(length) == len(crate):
                roots.append(crate)
        if len(roots) == 1:
            name += f" [instantiating crate: {roots[0]}]"
    return name


def run(command):
    result = subprocess.run(command, capture_output=True, text=True, check=False,
                            env={**os.environ, "LC_ALL": "C"})
    if result.returncode:
        raise ValueError(f"{command[0]} failed: {result.stderr.strip()}")
    return result.stdout


def capture(command, environment):
    """Run a measured child and keep both streams in memory until it succeeds."""
    try:
        result = subprocess.run(command, capture_output=True, check=False,
                                env=environment, timeout=1800)
    except subprocess.TimeoutExpired as error:
        raise ValueError(f"measurement child timed out: {command[0]}") from error
    if result.returncode:
        diagnostic = result.stderr[-4000:].decode("utf-8", errors="backslashreplace")
        raise ValueError(f"measurement child failed ({result.returncode}): {diagnostic}")
    return result.stdout, result.stderr


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
    addresses = defaultdict(set)
    for original, decoded in zip(raw, demangled, strict=True):
        if original[1:] != decoded[1:]:
            raise ValueError("raw/demangled symbol-table records disagree")
        identity = stable_identity(decoded[0], original[0])
        value = identity, decoded[6]
        if original[0] in result and result[original[0]] != value:
            raise ValueError(f"ambiguous library symbol: {original[0]}")
        result[original[0]] = value
        addresses[int(original[1], 16)].add(value)
    # Callgrind can leave zero-size CRT functions unnamed even though nm
    # has their exact start symbol. Resolve only an unambiguous exact start;
    # never guess a containing function or rank policy entries by address.
    for address, identities in addresses.items():
        if len(identities) == 1:
            result[f"0x{address:016x}"] = next(iter(identities))
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
    return resolved_paths(functions, library, symbol_table, available)


def resolved_paths(functions, library, symbol_table, available):
    result = set()
    fixed = set()
    for function in sorted(functions):
        if function not in symbol_table:
            raise ValueError(f"unresolved measured symbol {function!r}; use --demangle=no --separate-recs=1 --separate-callers=0")
        name, section = symbol_table[function]
        if name not in available:
            # The loader's .init/.fini and CRT .text functions are not
            # individually orderable staticlib sections. Rust must resolve.
            if function.startswith(("_R", "_ZN")):
                raise ValueError(f"Rust function has no orderable archive section: {name}")
            fixed.add(name)
            continue
        if len({section_family(section) for section in available[name]}) != 1:
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


@contextmanager
def collection_scratch(root):
    # No profiler output is written here. Only private writable profile
    # copies exist, removed after the child is reaped, including on signals.
    previous = {sig: signal.getsignal(sig) for sig in (signal.SIGINT, signal.SIGTERM)}

    def interrupted(number, _frame):
        raise SystemExit(128 + number)

    with tempfile.TemporaryDirectory(prefix="function-order-", dir=root) as name:
        try:
            for sig in previous:
                signal.signal(sig, interrupted)
            yield Path(name)
        finally:
            for sig, handler in previous.items():
                signal.signal(sig, handler)


def teardown_exports(library, facade):
    """The existing version script's export set, for the teardown resolver."""
    names = []
    for line in run(["nm", "-D", "--defined-only", str(library)]).splitlines():
        fields = line.split()
        if len(fields) == 3 and fields[1] in "TtWw":
            name = fields[2].split("@", 1)[0]
            if name.startswith(facade + "_"):
                names.append(name)
    if not names:
        raise ValueError(f"no {facade}_ exports in {library}")
    return ",".join(names)


def init_array_section(library):
    """The .init_array virtual address and byte size from the section table."""
    for line in run(["readelf", "-SW", str(library)]).splitlines():
        match = re.match(
            r"^\s*\[\s*\d+\]\s+\.init_array\s+\S+\s+([0-9a-fA-F]+)\s+"
            r"[0-9a-fA-F]+\s+([0-9a-fA-F]+)\b", line)
        if match:
            return int(match.group(1), 16), int(match.group(2), 16)
    raise ValueError(f"no .init_array section in {library}")


def init_array_words(library, base, size):
    """Raw slot values, without applying relocations."""
    words = []
    for line in run(["readelf", "-x", ".init_array", str(library)]).splitlines():
        match = re.match(r"^\s*0x[0-9a-fA-F]+\s+(.*)$", line)
        if not match:
            continue
        words.extend(word for word in match.group(1).split()
                     if re.fullmatch(r"[0-9a-fA-F]{8}", word))
    if len(words) % 2:
        raise ValueError("odd .init_array word count")
    little = lambda word: int("".join(word[i:i + 2] for i in range(6, -1, -2)), 16)
    slots = [little(low) | (little(high) << 32)
             for low, high in zip(words[::2], words[1::2])]
    if len(slots) * 8 != size:
        raise ValueError(f".init_array size {size} does not match {len(slots)} slots")
    return slots


def init_array_relocations(library, base, size):
    """Relocation targets in the .init_array address range, by slot address."""
    relocations = {}
    for line in run(["readelf", "-rW", str(library)]).splitlines():
        fields = line.split()
        if len(fields) < 4:
            continue
        try:
            offset = int(fields[0], 16)
        except ValueError:
            continue
        if not base <= offset < base + size:
            continue
        if fields[2].endswith("_RELATIVE"):
            try:
                relocations[offset] = int(fields[-1], 16)
            except ValueError:
                relocations[offset] = None
        else:
            relocations[offset] = None
    return relocations


def init_array_symbols(library):
    """Every load-time constructor, resolved from slots and relocations."""
    base, size = init_array_section(library)
    slots = init_array_words(library, base, size)
    relocations = init_array_relocations(library, base, size)
    table = {}
    for line in run(["nm", "--defined-only", str(library)]).splitlines():
        fields = line.split()
        if len(fields) == 3:
            try:
                table[int(fields[0], 16)] = fields[2]
            except ValueError:
                pass
    result = []
    unresolved = []
    for index, raw in enumerate(slots):
        offset = base + index * 8
        if offset in relocations:
            target = relocations[offset]
            if target is None:
                unresolved.append(f"{offset:#x} (unresolved relocation)")
                continue
        elif raw == 0:
            # A zero placeholder with no relocation is not a constructor.
            continue
        else:
            target = raw
        if target not in table:
            unresolved.append(f"{offset:#x} -> {target:#x}")
            continue
        result.append(table[target])
    if unresolved:
        raise ValueError("unresolved .init_array constructors: " + ", ".join(unresolved))
    return result


def callgrind_prefix(executable, cpu, toggle):
    return ["taskset", "-c", str(cpu), "valgrind", "--tool=callgrind",
            "--cache-sim=no", "--branch-sim=no", "--demangle=no",
            "--separate-recs=1", "--separate-callers=0", "--dump-instr=yes",
            "--collect-atstart=no", f"--toggle-collect={toggle}",
            "--callgrind-out-file=/dev/stderr", str(executable)]


def teardown_command(args, facade, library, user):
    return [str(args.bisect), "--teardown", facade, str(library),
            str(args.system_dir), str(user), "time", "0", "1", "1",
            "first-candidate", "-"]


def collect_toggle(args, facade, library, user, toggle, environment):
    command = [*callgrind_prefix(args.bisect, args.cpu, toggle),
               *teardown_command(args, facade, library, user)[1:]]
    _, error = capture(command, environment)
    try:
        return callgrind_functions(io.StringIO(error.decode("utf-8", errors="strict")), library)
    except ValueError as error_text:
        # A CRT stub such as frame_dummy may execute without touching an
        # orderable library body. Other constructors still contribute.
        if "no executed functions" in str(error_text):
            return set()
        raise


def collect_startup(args, facade, library, work, environment):
    """Load constructors plus each profile's init subtree, from the worker."""
    functions = set()
    first = work / f"{facade}-load"
    shutil.copytree(args.profiles_dir / PROFILES[0] / "user", first)
    try:
        for toggle in init_array_symbols(library):
            functions |= collect_toggle(args, facade, library, first, toggle, environment)
    finally:
        shutil.rmtree(first)
    for profile in PROFILES:
        user = work / f"{facade}-{profile}"
        shutil.copytree(args.profiles_dir / profile / "user", user)
        try:
            functions |= collect_toggle(args, facade, library, user, f"{facade}_init", environment)
        finally:
            shutil.rmtree(user)
    return functions


def collect_steady(args, facade, library, work, environment):
    command = [*callgrind_prefix(args.bisect, args.cpu, "bisect_perf_steady_unit"),
               "--perf", str(library), str(args.system_dir)]
    run_environment = {**environment, "PERF_FACADE": facade, "PERF_MODE": "speed",
                       "PERF_CYCLES": "4", "PERF_REPEATS": "1",
                       "PERF_WORKLOAD_CHECKSUM": "1"}
    output, error = capture(command, run_environment)
    row = json.loads(output)
    if len(row.get("cycles_ns", [])) != 4 or "workload_checksum" not in row:
        raise ValueError("steady driver lacks its cycle/fingerprint result")
    functions = callgrind_functions(io.StringIO(error.decode("utf-8", errors="strict")), library)
    return functions


def collect_policies(args, libraries, archives):
    required = (args.bisect, args.system_dir, args.profiles_dir, args.work_dir,
                args.system_identity, args.cpu is not None)
    if not all(required) or args.init or args.steady:
        raise ValueError("collection needs the bisect worker, system/profile/work directories, identity and CPU; no stream inputs")
    identity = STARTUP_PROFILE["identity"](args.system_dir)
    if identity != args.system_identity:
        raise ValueError(f"system identity differs: {identity}")
    for profile in PROFILES:
        fixture = args.profiles_dir / profile
        STARTUP_PROFILE["check"](fixture / "user", fixture / "profile.manifest")
    outputs = {}
    with collection_scratch(args.work_dir) as work:
        environment = {**os.environ, "LC_ALL": "C", "TMPDIR": str(work)}
        for facade in FACADES:
            library = libraries[facade].resolve()
            table, available = library_symbols(library), archive_paths(archives[facade])
            environment["OXPINYIN_TEARDOWN_EXPORTS"] = teardown_exports(library, facade)
            startup = resolved_paths(collect_startup(args, facade, library, work, environment),
                                     library, table, available)
            hot = resolved_paths(collect_steady(args, facade, library, work, environment),
                                 library, table, available)
            outputs[f"lib{facade}.order"] = policy(facade, startup, hot)
    # The input fixtures are immutable even if a driver's private copy mutates.
    for profile in PROFILES:
        fixture = args.profiles_dir / profile
        try:
            STARTUP_PROFILE["check"](fixture / "user", fixture / "profile.manifest")
        except ValueError as error:
            raise ValueError(f"profile changed during collection: {profile}: {error}") from error
    return outputs


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
    parser.add_argument("--output-dir", type=Path)
    parser.add_argument("--print-identity", type=Path, metavar="DIR",
                        help="print DIR's --system-identity value and exit")
    parser.add_argument("--bisect", type=Path, help="in-tree bisect worker; collects instead of reading streams")
    parser.add_argument("--system-dir", type=Path)
    parser.add_argument("--system-identity")
    parser.add_argument("--profiles-dir", type=Path)
    parser.add_argument("--work-dir", type=Path)
    parser.add_argument("--cpu", type=int)
    args = parser.parse_args()
    if args.print_identity:
        try:
            print(STARTUP_PROFILE["identity"](args.print_identity))
        except (OSError, ValueError) as error:
            parser.exit(1, f"function ordering: {error}\n")
        return
    if not args.output_dir:
        parser.error("--output-dir is required")
    try:
        libraries = pairs(args.library, FACADES, "--library")
        archives = pairs(args.staticlib, FACADES, "--staticlib")
        if args.bisect:
            outputs = collect_policies(args, libraries, archives)
        else:
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
