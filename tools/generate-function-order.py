#!/usr/bin/env -S uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal
"""Derive optional shipping-layout policies from measured Callgrind streams.

By default reads streams without running a workload. --bisect collects them
in memory with the in-tree teardown worker; run that mode through just heavy.
Neither mode writes a capture or builds a driver.
Each facade's startup set is the load constructors plus the measured
pinyin_init/zhuyin_init subtree and the exports the worker calls from there
to its M1 checkpoint, from the in-tree bisect worker. The steady set is the
existing bisect perf workload. Use --demangle=no for Callgrind.
nm supplies canonical demangled identity. Both facades and both startup
profiles are required, with no default policy.
"""

import argparse
from collections import defaultdict
from concurrent.futures import ThreadPoolExecutor, as_completed
from copy import copy
from queue import Queue
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
import threading
import time


FACADES = ("pinyin", "zhuyin")
PROFILES = ("phrases", "journals")
HERE = Path(__file__).resolve().parent
COLLECTION_STATE = threading.local()
# The startup-profile manifest, shared with the teardown provisioner that
# writes it; also the system-data identity.
STARTUP_PROFILE = runpy.run_path(str(HERE / "bisection/startup_profile.py"))
manifest = STARTUP_PROFILE["manifest"]


def canonical(name):
    """Same normalization as packaging/function-order.awk; no fuzzy matches."""
    name = re.sub(r"\.llvm\.[0-9a-fA-F]+$", "", name)
    return re.sub(r"::h[0-9a-fA-F]{16}$", "", name)


def rust_mangled(name):
    """A raw Rust symbol is never a stable path: v0 (_R) carries crate
    disambiguators and legacy (_ZN...17h<hash>E) a hash. nm demangles both."""
    name = re.sub(r"\.llvm\.[0-9a-fA-F]+$", "", name)
    return name.startswith("_R") or re.fullmatch(r"_ZN.*17h[0-9a-f]{16}E", name) is not None


def cxx_mangled(name):
    """An Itanium C++ name (the Kyoto Cabinet shim) is its own stable path:
    the mangling carries no hash. Which C++ functions a build contains is the
    system C++ compiler's choice (inlining, GCC clones), so the resolver and
    the verifier skip a C++ path that a build lacks; Rust and C paths stay
    exact (packaging/function-order.awk)."""
    return name.startswith("_Z") and not rust_mangled(name)


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
        cancelled = getattr(COLLECTION_STATE, 'cancelled', None)
        if cancelled is None:
            result = subprocess.run(command, capture_output=True, check=False,
                                    env=environment, timeout=1800)
        else:
            if cancelled.is_set():
                raise ValueError('collection cancelled')
            with subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                  env=environment, pass_fds=COLLECTION_STATE.fds) as child:
                deadline = time.monotonic() + 1800
                try:
                    while True:
                        if cancelled.is_set():
                            raise ValueError('collection cancelled')
                        remaining = deadline - time.monotonic()
                        if remaining <= 0:
                            raise subprocess.TimeoutExpired(command, 1800)
                        try:
                            out, err = child.communicate(timeout=min(.1, remaining))
                            break
                        except subprocess.TimeoutExpired:
                            continue
                except BaseException:
                    child.kill()
                    child.wait()
                    raise
                result = subprocess.CompletedProcess(command, child.returncode, out, err)
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
            if rust_mangled(function):
                raise ValueError(f"Rust function has no orderable archive section: {name}")
            fixed.add(name)
            continue
        if len({section_family(section) for section in available[name]}) != 1:
            raise ValueError(f"ambiguous stable path: {name}")
        if section != ".text":
            raise ValueError(f"archive function resolved outside library .text: {name}")
        if (name != canonical(name) or rust_mangled(name)
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


check_tkrzw = runpy.run_path(str(Path(__file__).parent / "tkrzw/runtime.py"))["check"]

def collect_toggle(args, facade, library, user, toggle, environment):
    check_tkrzw("auto", "generate-function-order.py", "subject", library, env=environment)
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


# The exports tools/bisection/teardown.h calls after <facade>_init and before
# its M1 checkpoint: options, one instance and the first candidate. Code they
# reach before M1 is startup code too; left out of the ordered block, it is
# faulted in 64 KiB fault-around windows scattered over .text (#1032).
FIRST_CANDIDATE_COMMON = ("set_options", "alloc_instance", "parse_more_full_pinyins",
                          "get_n_candidate", "get_candidate", "get_candidate_type",
                          "get_candidate_string")
# The exports teardown.h calls from <facade>_init to M1: set_options,
# alloc_instance, td_first and td_read_candidate.
FIRST_CANDIDATE = {
    "pinyin": (*FIRST_CANDIDATE_COMMON, "get_parsed_input_length", "guess_candidates",
               "get_candidate_nbest_index"),
    "zhuyin": (*FIRST_CANDIDATE_COMMON, "guess_candidates_after_cursor"),
}


def first_candidate_toggles(facade, exports):
    """The facade's FIRST_CANDIDATE exports; each one toggles alone."""
    available = set(exports.split(","))
    toggles = [f"{facade}_{name}" for name in FIRST_CANDIDATE[facade]]
    missing = [toggle for toggle in toggles if toggle not in available]
    if missing:
        raise ValueError(f"{facade} lacks the first-candidate exports {', '.join(missing)}")
    return toggles


def startup_toggles(facade, exports):
    """Each profile's init-to-M1 toggles, after the load constructors."""
    return (f"{facade}_init", *first_candidate_toggles(facade, exports))


def startup_collections(facade, library, exports):
    """Independent (profile, toggle) inputs, including load constructors."""
    return [(PROFILES[0], toggle) for toggle in init_array_symbols(library)] + [
        (profile, toggle) for profile in PROFILES
        for toggle in startup_toggles(facade, exports)]


def collection_budget(env, resources, *, runner=False):
    """Reserve each profiler worker under the shared slot's memory accounting."""
    from tools.dev import host
    child = dict(env)
    ceiling = min(resources['cpus'], int(env['OXPINYIN_ORDER_CPU_COUNT']))
    available = resources['available_mib']
    headroom = host.memory_headroom(env, resources)
    # Use the existing heavy-runner allowance, including captured output and
    # private fixtures, rather than treating a profiler as a rustc process.
    peak = host.RUNNER_PEAK_MIB
    slots = host.heavy_slot_count(resources.get('total_mib'))
    workers = 0 if available is None else min(ceiling, max(0, available - headroom) // slots // peak)
    if not workers and available is not None:
        workers = host.fit(available, peak, ceiling, headroom)
        workers = min(workers, 1)
    child['OXPINYIN_ORDER_WORKERS'] = str(max(1, workers))
    reason = f'{available} MiB available; {headroom} MiB headroom; {peak} MiB per collection; CPU ceiling {ceiling}'
    return child, {'OXPINYIN_ORDER_WORKERS': (workers, reason, False)}, max(1, workers) * peak


def collect_parallel(args, libraries, work, environment, cpus, *, passes=1, pass_fds=()):
    """Union independent collections; private copies and CPU leases never overlap."""
    jobs = []
    for facade in FACADES:
        library = libraries[facade].resolve()
        exports = teardown_exports(library, facade)
        env = dict(environment, OXPINYIN_TEARDOWN_EXPORTS=exports)
        jobs.extend((facade, library, profile, toggle, env)
                    for profile, toggle in startup_collections(facade, library, exports))
        jobs.append((facade, library, None, None, env))
    jobs = [(iteration, job) for job in jobs for iteration in range(passes)]
    cancelled = threading.Event()
    leases = Queue()
    for cpu in cpus:
        leases.put(cpu)

    def collect(job):
        """Keep temporary files until the child has exited and streams are parsed."""
        iteration, job = job
        facade, library, profile, toggle, env = job
        COLLECTION_STATE.cancelled = cancelled
        COLLECTION_STATE.fds = pass_fds
        local = copy(args)
        local.cpu = leases.get()
        try:
            with tempfile.TemporaryDirectory(prefix=f'{facade}-', dir=work) as directory:
                scratch = Path(directory)
                env = dict(env, TMPDIR=str(scratch))
                if profile is None:
                    functions = collect_steady(local, facade, library, scratch, env)
                else:
                    user = scratch / 'user'
                    shutil.copytree(args.profiles_dir / profile / 'user', user)
                    functions = collect_toggle(local, facade, library, user, toggle, env)
                return iteration, facade, 'steady' if profile is None else 'startup', functions
        finally:
            leases.put(local.cpu)

    result = [{facade: {'startup': set(), 'steady': set()} for facade in FACADES}
              for _ in range(passes)]
    with ThreadPoolExecutor(max_workers=len(cpus)) as pool:
        futures = []
        try:
            for job in jobs:
                futures.append(pool.submit(collect, job))
            for future in as_completed(futures):
                iteration, facade, tier, functions = future.result()
                result[iteration][facade][tier].update(functions)
        finally:
            # Do not launch queued work after failure/cancellation. Running
            # children are reaped before the pool and scratch context exit.
            cancelled.set()
            for future in futures:
                future.cancel()
    return result


def collect_steady(args, facade, library, work, environment):
    command = [*callgrind_prefix(args.bisect, args.cpu, "bisect_perf_steady_unit"),
               "--perf", str(library), str(args.system_dir)]
    run_environment = {**environment, "PERF_FACADE": facade, "PERF_MODE": "speed",
                       "PERF_CYCLES": "4", "PERF_REPEATS": "1",
                       "PERF_WORKLOAD_CHECKSUM": "1"}
    check_tkrzw("auto", "generate-function-order.py", "subject", library, env=run_environment)
    output, error = capture(command, run_environment)
    row = json.loads(output)
    if len(row.get("cycles_ns", [])) != 4 or "workload_checksum" not in row:
        raise ValueError("steady driver lacks its cycle/fingerprint result")
    functions = callgrind_functions(io.StringIO(error.decode("utf-8", errors="strict")), library)
    return functions


def collect_policies(args, libraries, archives):
    required = (args.bisect, args.system_dir, args.profiles_dir, args.work_dir,
                args.system_identity)
    if not all(required) or args.init or args.steady:
        raise ValueError("collection needs the bisect worker, system/profile/work directories and identity; no stream inputs")
    identity = STARTUP_PROFILE["identity"](args.system_dir)
    if identity != args.system_identity:
        raise ValueError(f"system identity differs: {identity}")
    for profile in PROFILES:
        fixture = args.profiles_dir / profile
        STARTUP_PROFILE["check"](fixture / "user", fixture / "profile.manifest")
    passes = 2 if getattr(args, "repeat_output_dir", None) else 1
    outputs = [{} for _ in range(passes)]
    with collection_scratch(args.work_dir) as work:
        cpus = sorted(os.sched_getaffinity(0))
        if args.cpu is not None:
            if args.cpu not in cpus:
                raise ValueError(f'CPU {args.cpu} is outside the allowed affinity set')
            cpus = [args.cpu]
        root = str(HERE.parent)
        if root not in sys.path:
            sys.path.insert(0, root)
        from tools.dev import slot
        environment = dict(os.environ, LC_ALL="C", TMPDIR=str(work),
                           OXPINYIN_ORDER_CPU_COUNT=str(len(cpus)))
        with slot.HeavyAdmission('function-order collections', environment,
                                 prepare_heavy_env=collection_budget) as admission:
            cpus = cpus[:int(admission.env['OXPINYIN_ORDER_WORKERS'])]
            print(f'function-order workers: {len(cpus)}; CPUs: {cpus}', file=sys.stderr, flush=True)
            collected = collect_parallel(args, libraries, work, admission.env, cpus, passes=passes,
                                         pass_fds=admission.slot.pass_fds())
            for facade in FACADES:
                library = libraries[facade].resolve()
                table, available = library_symbols(library), archive_paths(archives[facade])
                for iteration, streams in enumerate(collected):
                    startup = resolved_paths(streams[facade]['startup'], library, table, available)
                    hot = resolved_paths(streams[facade]['steady'], library, table, available)
                    outputs[iteration][f"lib{facade}.order"] = policy(facade, startup, hot)
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
    parser.add_argument("--repeat-output-dir", type=Path,
                        help="collect a second independent pass in the same admitted pool")
    parser.add_argument("--print-identity", type=Path, metavar="DIR",
                        help="print DIR's --system-identity value and exit")
    parser.add_argument("--bisect", type=Path, help="in-tree bisect worker; collects instead of reading streams")
    parser.add_argument("--system-dir", type=Path)
    parser.add_argument("--system-identity")
    parser.add_argument("--profiles-dir", type=Path)
    parser.add_argument("--work-dir", type=Path)
    parser.add_argument("--cpu", type=int, help="restrict collection to one allowed CPU (default: admitted pool)")
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
            if args.repeat_output_dir:
                raise ValueError("--repeat-output-dir requires --bisect")
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
        if not args.bisect:
            outputs = [outputs]
        destinations = [args.output_dir]
        if args.repeat_output_dir:
            if args.repeat_output_dir.resolve() == args.output_dir.resolve():
                raise ValueError("repeat output directory must differ from output directory")
            destinations.append(args.repeat_output_dir)
        for destination, policies in zip(destinations, outputs, strict=True):
            destination.mkdir(parents=True, exist_ok=True)
            for name, contents in policies.items():
                (destination / name).write_text(contents, encoding="utf-8", newline="\n")
    except (OSError, ValueError) as error:
        parser.exit(1, f"function ordering: {error}\n")


if __name__ == "__main__":
    main()
