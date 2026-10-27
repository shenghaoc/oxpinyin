#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Derive optional shipping-layout policies from measured Callgrind streams.

By default reads streams without running a workload. --bisect collects them
in memory with the in-tree teardown worker; run that mode through just heavy.
Neither mode writes a capture or builds a driver.
Each facade's startup set is the load constructors plus the measured
pinyin_init/zhuyin_init subtree and the exports the worker calls from there
to its M1 checkpoint, from the in-tree bisect worker. The steady set is the
existing bisect perf workload. Use --demangle=no for Callgrind.
nm supplies canonical demangled identity. Both facades and all three startup
profiles are required, with no default policy. The populated profiles' init
path leads the init tier; functions only the empty profile runs follow it.
"""

import argparse
from collections import defaultdict
from copy import copy
from contextlib import ExitStack, contextmanager, nullcontext
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
import time
from dataclasses import replace
from itertools import zip_longest


FACADES = ("pinyin", "zhuyin")
# The populated profiles' startup set leads the init tier. The empty user
# directory takes init's missing-profile path (check_format's absent marker,
# the wipe of user files); the functions only it runs follow that set in the
# init tier (#1090).
POPULATED_PROFILES = ("phrases", "journals")
EMPTY_PROFILE = "empty"
PROFILES = (*POPULATED_PROFILES, EMPTY_PROFILE)
HERE = Path(__file__).resolve().parent
if str(HERE.parent) not in sys.path:
    sys.path.insert(0, str(HERE.parent))
from tools.dev import host, pool, slot
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


def capture(command, environment, context):
    """Capture through the shared unit launcher, preserving the 1800-second bound."""
    deadline = time.monotonic() + 1800

    def stopped():
        context.check()
        if time.monotonic() >= deadline:
            raise ValueError(f"measurement child timed out: {command[0]}")
        return False

    # UnitContext.launch checks this deadline at every wait and owns all
    # process-group termination, descriptor inheritance and reaping.
    result = replace(context, cancelled=stopped).launch(command, env=environment)
    if result.status:
        diagnostic = result.stderr[-4000:].decode("utf-8", errors="backslashreplace")
        raise ValueError(f"measurement child failed ({result.status}): {diagnostic}")
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
    text = run([sys.executable, "-X", "lazy_imports=normal", str(HERE / "dev/__main__.py"), "packaging", "function-order", "inventory", str(archive)])
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
    _, error = capture(command, environment, args.context)
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


def collection_units(args, libraries, work, backend, campaign):
    """Describe one backend's independent collections, with interleaved passes."""
    jobs = []
    for facade in FACADES:
        library = libraries[facade].resolve()
        exports = teardown_exports(library, facade)
        jobs.extend((facade, library, profile, toggle, exports)
                    for profile, toggle in startup_collections(facade, library, exports))
        jobs.append((facade, library, None, None, exports))

    def unit(job, iteration):
        def collect(context):
            facade, library, profile, toggle, exports = job
            context.check()
            local = copy(args)
            local.cpu, local.context = context.cpu, context
            with tempfile.TemporaryDirectory(prefix=f'{facade}-', dir=work) as directory:
                scratch = Path(directory)
                environment = dict(context.environment, LC_ALL='C', TMPDIR=str(scratch),
                                   OXPINYIN_TEARDOWN_EXPORTS=exports)
                if profile is None:
                    functions = collect_steady(local, facade, library, scratch, environment)
                else:
                    user = scratch / 'user'
                    shutil.copytree(args.profiles_dir / profile / 'user', user)
                    context.check()
                    functions = collect_toggle(local, facade, library, user, toggle, environment)
                tier = 'steady' if profile is None else 'startup' if profile in POPULATED_PROFILES else 'empty'
                return (campaign, iteration, facade, tier, functions,
                        context.environment.get(pool.POOL_WORKERS))
        return collect

    passes = 2 if args.repeat_output_dir else 1
    return [(f'{backend}-pass{iteration + 1}-{index}', unit(job, iteration))
            for index, job in enumerate(jobs) for iteration in range(passes)]


def collect_steady(args, facade, library, work, environment):
    command = [*callgrind_prefix(args.bisect, args.cpu, "bisect_perf_steady_unit"),
               "--perf", str(library), str(args.system_dir)]
    run_environment = {**environment, "PERF_FACADE": facade, "PERF_MODE": "speed",
                       "PERF_CYCLES": "4", "PERF_REPEATS": "1",
                       "PERF_WORKLOAD_CHECKSUM": "1"}
    check_tkrzw("auto", "generate-function-order.py", "subject", library, env=run_environment)
    output, error = capture(command, run_environment, args.context)
    row = json.loads(output)
    if len(row.get("cycles_ns", [])) != 4 or "workload_checksum" not in row:
        raise ValueError("steady driver lacks its cycle/fingerprint result")
    functions = callgrind_functions(io.StringIO(error.decode("utf-8", errors="strict")), library)
    return functions


def collect_campaigns(campaigns, *, cpu=None, jobs=None, report=None):
    """Run all backend/pass units under one shared admission, then resolve every stream."""
    cpus = sorted(host.allowed_cpus())
    if cpu is not None:
        if cpu not in cpus:
            raise ValueError(f'CPU {cpu} is outside the allowed affinity set')
        cpus = [cpu]
    outputs, batches = [], []
    with ExitStack() as scratch:
        for index, (backend, args, libraries, archives) in enumerate(campaigns):
            required = (args.bisect, args.system_dir, args.profiles_dir, args.work_dir, args.system_identity)
            if not all(required) or args.init or args.steady:
                raise ValueError('collection needs the bisect worker, system/profile/work directories and identity; no stream inputs')
            identity = STARTUP_PROFILE['identity'](args.system_dir)
            if identity != args.system_identity:
                raise ValueError(f'system identity differs: {identity}')
            for profile in PROFILES:
                fixture = args.profiles_dir / profile
                STARTUP_PROFILE['check'](fixture / 'user', fixture / 'profile.manifest')
            work = scratch.enter_context(collection_scratch(args.work_dir))
            batches.append(collection_units(args, libraries, work, backend, index))
            outputs.append([{facade: {'startup': set(), 'empty': set(), 'steady': set()} for facade in FACADES}
                            for _ in range(2 if args.repeat_output_dir else 1)])
        # Round-robin the backends as well as the passes so capacity can serve
        # all campaigns immediately instead of draining the first backend first.
        units = [unit for row in zip_longest(*batches) for unit in row if unit is not None]
        with slot.cancellation_handlers():
            results = pool.run_admitted(units, 'function-order collections',
                                        cancelled=slot.CANCELLED.is_set, cpus=cpus,
                                        max_workers=jobs, cancel_error=slot.Cancelled)
        for campaign, iteration, facade, tier, functions, workers in results:
            if report is not None and workers is not None:
                report['workers'] = int(workers)
            outputs[campaign][iteration][facade][tier].update(functions)
        for index, (_, args, libraries, archives) in enumerate(campaigns):
            for facade in FACADES:
                library = libraries[facade].resolve()
                table, available = library_symbols(library), archive_paths(archives[facade])
                for streams in outputs[index]:
                    startup = resolved_paths(streams[facade]['startup'], library, table, available)
                    empty = resolved_paths(streams[facade]['empty'], library, table, available)
                    hot = resolved_paths(streams[facade]['steady'], library, table, available)
                    streams[f'lib{facade}.order'] = policy(facade, startup, hot, empty)
                    del streams[facade]
            for profile in PROFILES:
                fixture = args.profiles_dir / profile
                try:
                    STARTUP_PROFILE['check'](fixture / 'user', fixture / 'profile.manifest')
                except ValueError as error:
                    raise ValueError(f'profile changed during collection: {profile}: {error}') from error
    return outputs


def collect_policies(args, libraries, archives):
    """The single-backend CLI uses the same admitted campaign path."""
    return collect_campaigns([('backend', args, libraries, archives)], cpu=args.cpu,
                             jobs=getattr(args, 'jobs', None))[0]


def positive(text):
    value = int(text)
    if value < 1:
        raise argparse.ArgumentTypeError('jobs must be positive')
    return value


def read_campaign(path):
    """Load backend inputs; paths are relative to the campaign file."""
    rows = json.loads(path.read_text())
    if not isinstance(rows, list) or not rows:
        raise ValueError('campaign must be a nonempty JSON array')
    required = {'backend', 'library', 'staticlib', 'bisect', 'system_dir', 'system_identity',
                'profiles_dir', 'work_dir', 'output_dir'}
    campaigns, backends, destinations = [], set(), set()
    def resolve(value):
        return (path.parent / value).resolve()
    for row in rows:
        if not isinstance(row, dict) or not required <= row.keys() or row.keys() - required - {'repeat_output_dir'}:
            raise ValueError('campaign entry has missing or unknown fields')
        backend = row['backend']
        if backend not in ('bdb', 'kc', 'tkrzw') or backend in backends:
            raise ValueError(f'invalid or duplicate campaign backend: {backend}')
        backends.add(backend)
        mappings = []
        for field in ('library', 'staticlib'):
            if not isinstance(row[field], dict) or row[field].keys() != set(FACADES):
                raise ValueError(f'campaign {field} requires exactly pinyin and zhuyin')
            mappings.append({facade: resolve(value) for facade, value in row[field].items()})
        fields = {key: resolve(row[key]) for key in
                  ('bisect', 'system_dir', 'profiles_dir', 'work_dir', 'output_dir')}
        fields['repeat_output_dir'] = resolve(row['repeat_output_dir']) if row.get('repeat_output_dir') else None
        args = argparse.Namespace(**fields, system_identity=row['system_identity'], init=[], steady=[])
        for destination in (args.output_dir, args.repeat_output_dir):
            if destination is not None:
                if destination in destinations:
                    raise ValueError('campaign output directories must be distinct')
                destinations.add(destination)
        campaigns.append((backend, args, *mappings))
    return campaigns


def write_outputs(args, outputs):
    """Publish policies only after every campaign and immutable fixture check succeeds."""
    destinations = [args.output_dir]
    if args.repeat_output_dir:
        if args.repeat_output_dir.resolve() == args.output_dir.resolve():
            raise ValueError('repeat output directory must differ from output directory')
        destinations.append(args.repeat_output_dir)
    for destination, policies in zip(destinations, outputs, strict=True):
        destination.mkdir(parents=True, exist_ok=True)
        for name, contents in policies.items():
            (destination / name).write_text(contents, encoding='utf-8', newline='\n')


def compare_base(campaigns, base, *, cpu=None, jobs=None):
    """Compare the base in one-CPU mode, this campaign, and two concurrent passes.

    This is maintained verification orchestration: the base is an unchanged
    generator script in its own checkout, with its own sibling helper imports.
    Every launch uses the shared admitted runner; no private controller is needed.
    """
    if not base.is_file():
        raise ValueError(f'base generator does not exist: {base}')
    if any(not args.repeat_output_dir for _, args, _, _ in campaigns):
        raise ValueError('--compare-base needs repeat_output_dir on every campaign entry')
    allowed = sorted(host.allowed_cpus())
    selected = allowed[0] if cpu is None else cpu
    if selected not in allowed:
        raise ValueError(f'CPU {selected} is outside the allowed affinity set')
    baseline, single = [], []
    with ExitStack() as scratch:
        for backend, args, libraries, archives in campaigns:
            root = Path(scratch.enter_context(tempfile.TemporaryDirectory(prefix='comparison-', dir=args.work_dir)))
            original = copy(args)
            original.output_dir, original.repeat_output_dir = root / 'base', None
            candidate = copy(args)
            candidate.output_dir, candidate.repeat_output_dir = root / 'parallel', None
            single.append((backend, candidate, libraries, archives))
            def run(context, args=original, libraries=libraries, archives=archives):
                command = [sys.executable, '-X', 'lazy_imports=normal', str(base)]
                for option, mapping in (('--library', libraries), ('--staticlib', archives)):
                    for facade, path in mapping.items():
                        command += [option, f'{facade}={path}']
                for option in ('bisect', 'system_dir', 'system_identity', 'profiles_dir', 'work_dir', 'output_dir'):
                    command += ['--' + option.replace('_', '-'), str(getattr(args, option))]
                command += ['--cpu', str(selected)]
                result = context.launch(command)
                if result.status:
                    raise ValueError(f'base generator failed ({result.status}): '
                                     + result.stderr[-4000:].decode(errors='backslashreplace'))
                return {f'lib{facade}.order': (args.output_dir / f'lib{facade}.order').read_bytes()
                        for facade in FACADES}
            baseline.append((backend, run))
        summary = {'base_script': str(base), 'baseline_mode': f'--cpu {selected}: one worker',
                   'policies': len(campaigns) * len(FACADES)}
        started = time.monotonic()
        with slot.cancellation_handlers():
            expected = pool.run_admitted(baseline, 'function-order serial base',
                                         cancelled=slot.CANCELLED.is_set, cpus=[selected],
                                         max_workers=1, cancel_error=slot.Cancelled)
        summary['serial_seconds'] = round(time.monotonic() - started, 3)
        started = time.monotonic()
        parallel_report = {}
        actual = collect_campaigns(single, cpu=cpu, jobs=jobs, report=parallel_report)
        for (_, args, _, _), policies in zip(single, actual, strict=True):
            write_outputs(args, policies)
        summary['parallel_seconds'] = round(time.monotonic() - started, 3)
        summary['parallel_workers'] = parallel_report['workers']
        for (backend, _, _, _), wanted, got in zip(campaigns, expected, actual, strict=True):
            if wanted != {name: value.encode('utf-8') for name, value in got[0].items()}:
                raise ValueError(f'{backend}: serial/parallel policies differ')
        started = time.monotonic()
        repeat_report = {}
        repeated = collect_campaigns(campaigns, cpu=cpu, jobs=jobs, report=repeat_report)
        for (backend, _, _, _), wanted, got in zip(campaigns, expected, repeated, strict=True):
            if any(wanted != {name: value.encode('utf-8') for name, value in policies.items()} for policies in got):
                raise ValueError(f'{backend}: concurrent repeat policies differ')
        for (_, args, _, _), policies in zip(campaigns, repeated, strict=True):
            write_outputs(args, policies)
        summary['repeat_seconds'] = round(time.monotonic() - started, 3)
        summary['repeat_workers'] = repeat_report['workers']
        print(json.dumps(summary, sort_keys=True), flush=True)


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


def policy(facade, startup, steady, empty=frozenset()):
    """Init: the populated startup set, then the empty profile's own; then steady."""
    rows = [f"oxpinyin-function-order-v1\tlib{facade}.so.15"]
    for tier, names in (("init", startup), ("init", empty - startup),
                        ("steady", steady - startup - empty)):
        rows.extend(f"{tier}\t{name}" for name in sorted(names))
    return "\n".join(rows) + "\n"


def run_generation(args, parser):
    """Execute validated host work after the CLI's platform/tool preflight."""
    if args.compare_base and not args.campaign:
        parser.error('--compare-base requires --campaign')
    if args.campaign and args.print_identity:
        parser.error('--campaign cannot be combined with --print-identity')
    if args.print_identity:
        try:
            print(STARTUP_PROFILE["identity"](args.print_identity))
        except (OSError, ValueError) as error:
            parser.exit(1, f"function ordering: {error}\n")
        return
    if args.campaign:
        if (args.library or args.staticlib or args.init or args.steady or args.output_dir or
                args.repeat_output_dir or args.bisect or args.system_dir or args.system_identity or
                args.profiles_dir or args.work_dir or args.print_identity):
            parser.error('--campaign takes its inputs and output directories from the JSON file')
        try:
            campaigns = read_campaign(args.campaign.resolve())
            if args.compare_base:
                compare_base(campaigns, args.compare_base.resolve(), cpu=args.cpu, jobs=args.jobs)
                return
            outputs = collect_campaigns(campaigns, cpu=args.cpu, jobs=args.jobs)
            for (_, campaign, _, _), policies in zip(campaigns, outputs, strict=True):
                write_outputs(campaign, policies)
        except (OSError, ValueError, TypeError) as error:
            parser.exit(1, f'function ordering: {error}\n')
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
                for profile in POPULATED_PROFILES:
                    init |= measured_paths(startup[f"{facade}:{profile}"], library, symbol_table, available)
                empty = measured_paths(startup[f"{facade}:{EMPTY_PROFILE}"], library, symbol_table, available)
                hot = measured_paths(steady[facade], library, symbol_table, available)
                outputs[f"lib{facade}.order"] = policy(facade, init, hot, empty)
        # Validate all streams before writing either policy. No raw data is
        # copied; the only outputs are the production-layout policy inputs.
        if not args.bisect:
            outputs = [outputs]
        write_outputs(args, outputs)
    except (OSError, ValueError) as error:
        parser.exit(1, f"function ordering: {error}\n")


def generation_refusal(platform, which):
    """Return one missing host requirement, or None; inspect no campaign inputs."""
    if platform != 'linux':
        return f'function ordering: generation requires Linux (host: {platform})'
    for tool in ('taskset', 'valgrind'):
        if which(tool) is None:
            return f'function ordering: generation requires {tool} on PATH'
    return None


def main(argv=None, *, platform=sys.platform, which=shutil.which,
         execute=run_generation, err=sys.stderr):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--campaign', type=Path, help='JSON backend inputs; all collections share one admitted pool')
    parser.add_argument('--compare-base', type=Path, help='verify serial base, parallel campaign and concurrent repeats')
    parser.add_argument('--jobs', type=positive, help='maximum collection workers; admission may lower it')
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
    args = parser.parse_args(argv)
    refusal = generation_refusal(platform, which)
    if refusal:
        print(refusal, file=err, flush=True)
        return 2
    return execute(args, parser)


if __name__ == "__main__":
    raise SystemExit(main())
