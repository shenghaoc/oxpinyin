#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Linux function-layout campaign using the in-tree bisect worker.

Run through just heavy. --profiler-output-dir retains profiler streams locally. The only
scratch files are private library/profile copies, deleted on exit and signals.
The worker is tools/bisection/bisect.c in its teardown mode: native timing uses
its time stop, memory uses its parent-observed checkpoints, and Callgrind uses
toggles around the load constructors and the generator's startup toggles:
the facade init function and the exports the worker calls from there to M1.
"""

import argparse
from collections import defaultdict
from concurrent.futures import ThreadPoolExecutor
import hashlib
import io
import json
import math
import os
from pathlib import Path
import re
import runpy
import shutil
import signal
import statistics
import subprocess
import tempfile


check_tkrzw = runpy.run_path(str(Path(__file__).parent / "tkrzw/runtime.py"))["check"]
GEN = runpy.run_path(str(Path(__file__).with_name("generate-function-order.py")))
ORDER = runpy.run_path(str(Path(__file__).parent / 'packaging/function_order.py'))
MMAN = runpy.run_path(str(Path(__file__).parent / 'dev/mman.py'))
from tools.dev.measure import registration, startup as startup_controller, worker
ARMS = ("base", "pr", "control", "pin")
FACADES = ("pinyin", "zhuyin")
PROFILES = ("phrases", "journals")
PAGE = MMAN["PAGE_SIZE"]


def run(command, env=None, *, profiler=False, native_stderr=None, profiler_output_dir=None):
    """Reap each child; keep profiler output separate from strict native stderr."""
    command = list(map(str, command))
    out = err = b''
    reader = writer = None
    if profiler:
        reader, writer = os.pipe()
        position = next(i for i, arg in enumerate(command) if Path(arg).name == 'valgrind')
        command.insert(position + 1, '--log-fd=' + str(writer))
        command = [('--callgrind-out-file=/dev/fd/' + str(writer)
                    if arg.startswith('--callgrind-out-file=') else arg) for arg in command]
    with ThreadPoolExecutor(max_workers=1) as drain:
        with subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env,
                              pass_fds=() if writer is None else (writer,), start_new_session=True) as child:
            if writer is not None:
                os.close(writer)
                stream = os.fdopen(reader, 'rb')
                profile = drain.submit(stream.read)
            try:
                out, err = child.communicate(timeout=1800)
            except BaseException:
                os.killpg(child.pid, signal.SIGKILL)
                child.wait()
                raise
            finally:
                if reader is not None:
                    collected = profile.result()
                    stream.close()
                    if profiler_output_dir is not None:
                        with tempfile.NamedTemporaryFile(dir=profiler_output_dir, prefix='profile-',
                                                         suffix='.json', mode='w', delete=False) as saved:
                            json.dump(dict(command=command, status=child.returncode,
                                           stdout_hex=out.hex(), stderr_hex=err.hex(),
                                           profiler_hex=collected.hex()), saved)
            if child.returncode:
                raise RuntimeError(f'child exit {child.returncode}: {command}\n'
                                   + out[-2000:].decode(errors='backslashreplace')
                                   + err[-4000:].decode(errors='backslashreplace')
                                   + (collected[-4000:].decode(errors='backslashreplace') if profiler else ''))
            if native_stderr is not None and err != native_stderr:
                raise ValueError('instrumented native stderr differs from verification')
            return out, collected if profiler else err


def digest(path):
    with path.open("rb") as handle:
        return hashlib.file_digest(handle, "sha256").hexdigest()


def cache_pages(path):
    """mincore(PROT_NONE mapping) observes file cache without faulting it in (tools/dev/mman.py)."""
    with path.open("rb") as handle:
        observed = MMAN["residency"](handle.fileno())
    return observed["resident"], observed["pages"]


def prepare_cache(path, cold):
    if cold:
        # A new copy can already be resident. Flush its own dirty pages, advise
        # eviction, then require actual zero-resident mincore before execution.
        with path.open("rb+") as handle:
            os.fsync(handle.fileno())
            os.posix_fadvise(handle.fileno(), 0, math.ceil(path.stat().st_size / PAGE) * PAGE,
                             os.POSIX_FADV_DONTNEED)
    else:
        with path.open("rb") as handle:
            while handle.read(1024 * 1024):
                pass
    resident, total = cache_pages(path)
    if resident != (0 if cold else total):
        raise RuntimeError(f"cache precondition failed: cold={cold}, {resident}/{total} pages")
    return total


def fields(text):
    return {key: int(value) for key, value in re.findall(r"^(\w+):\s+(\d+) kB$", text, re.M)}


def text_memory(smaps, inode):
    totals = defaultdict(int)
    selected = False
    mappings = 0
    for line in smaps.splitlines():
        match = re.match(r"^[0-9a-f]+-[0-9a-f]+ (\S+) [0-9a-f]+ \S+ (\d+)(?: |$)", line)
        if match:
            selected = "x" in match[1] and int(match[2]) == inode
            mappings += selected
        elif selected:
            totals.update({key: totals[key] + value for key, value in fields(line).items()})
    if not mappings:
        return {"text_pages": 0, "text_pss_kib": 0}
    if totals["Rss"] * 1024 % PAGE:
        raise ValueError("text Rss is not page aligned")
    return {"text_pages": totals["Rss"] * 1024 // PAGE, "text_pss_kib": totals["Pss"]}


def teardown_memory(command, library, env):
    """Drive the worker's memory checkpoints and read smaps from the parent."""
    snapshots = {}
    samples = []
    with subprocess.Popen(list(map(str, command)), stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                          stderr=subprocess.PIPE, env=env, start_new_session=True) as child:
        try:
            while True:
                line = child.stdout.readline()
                if not line:
                    break
                try:
                    row = json.loads(line)
                except json.JSONDecodeError:
                    continue
                if row.get("teardown") == "ready":
                    child.stdin.write(b"\n")
                    child.stdin.flush()
                    continue
                if row.get('teardown') == 'sample':
                    samples.append(row)
                point = row.get("checkpoint")
                if point is not None:
                    smaps = Path(f"/proc/{child.pid}/smaps").read_text()
                    snapshots[point] = {
                        **text_memory(smaps, library.stat().st_ino),
                        "total_pss_kib": fields(Path(f"/proc/{child.pid}/smaps_rollup").read_text())["Pss"],
                        # minflt is field 10 of proc_pid_stat(5); the fields
                        # after comm start past its closing parenthesis.
                        "minor_faults": int(Path(f"/proc/{child.pid}/stat").read_text()
                                            .rsplit(")", 1)[1].split()[7]),
                        "allocator": row.get("allocator"),
                    }
                    child.stdin.write(b"\n")
                    child.stdin.flush()
            err = child.stderr.read()
            child.wait()
        except BaseException:
            os.killpg(child.pid, signal.SIGKILL)
            child.wait()
            raise
    if child.returncode:
        raise RuntimeError(f"memory child exit={child.returncode}: {err[-4000:]!r}")
    missing = [point for point in ("M0", "M1") if point not in snapshots]
    if missing:
        raise ValueError(f"missing teardown memory checkpoints: {missing}")
    if len(samples) != 1:
        raise ValueError('memory worker supplied no complete observation')
    load, init = snapshots["M0"], snapshots["M1"]
    return {
        "load_text_pages": load["text_pages"], "load_text_pss_kib": load["text_pss_kib"],
        "init_text_pages": init["text_pages"], "init_text_pss_kib": init["text_pss_kib"],
        "init_total_pss_kib": init["total_pss_kib"],
        "load_minor_faults": load["minor_faults"],
        "init_minor_faults": init["minor_faults"] - load["minor_faults"],
        "init_heap_bytes": (init["allocator"] or {}).get("uordblks"),
    }, samples[0]['observations'], err


def instruction_sizes(library):
    output, _ = run(["objdump", "-d", "-w", library])
    result = {}
    for line in output.decode().splitlines():
        match = re.match(r"^\s*([0-9a-f]+):\s+((?:[0-9a-f]{2} )*[0-9a-f]{2})\s", line)
        if match:
            result[int(match[1], 16)] = len(match[2].split())
    if not result:
        raise ValueError("objdump supplied no instruction lengths")
    return result


def executed_profile(stderr, library, sizes):
    """Decode exclusive instruction positions, including inclusive-row bases.

    Callgrind addresses correspond to the original ELF (cl-format section 3.1.6).
    Every executed address must therefore match objdump; no guessed load bias.
    """
    text = stderr.decode("utf-8", errors="strict")
    try:
        GEN["callgrind_functions"](io.StringIO(text), library)  # reconciles all exclusive Ir
    except ValueError as error:
        # As in the generator's collect_toggle: a CRT stub such as
        # frame_dummy may run without touching an orderable library body.
        if "no executed functions" not in str(error):
            raise
    tables = {"ob": {}}
    current = ""
    positions = []
    previous = []
    skip = False
    addresses = set()
    for line in text.splitlines():
        if line.startswith("positions:"):
            positions = line.split()[1:]
            previous = [0] * len(positions)
            if "instr" not in positions:
                raise ValueError("Callgrind lacks instruction positions")
        elif match := re.match(r"^(c?)ob=(.*)$", line):
            body = match[2]
            compressed = re.fullmatch(r"\((\d+)\)(?: (.*))?", body)
            if compressed:
                number, name = compressed.groups()
                if name is not None:
                    tables["ob"][number] = name
                body = tables["ob"][number]
            if not match[1]:
                current = body
        elif line.startswith("calls="):
            skip = True
        elif line and line[0] in "0123456789+-*":
            parts = line.split()
            for i, token in enumerate(parts[:len(positions)]):
                if token == "*":
                    continue
                if token.startswith(("+", "-")):
                    previous[i] += int(token, 0)
                else:
                    previous[i] = int(token, 0) if token.startswith("0x") else int(token)
            if not skip and int(parts[len(positions)]) > 0 and Path(current).resolve() == library.resolve():
                addresses.add(previous[positions.index("instr")])
            skip = False
    pages = set()
    byte_ranges = set()
    for address in addresses:
        if address not in sizes:
            raise ValueError(f"executed address {address:#x} does not match ELF instruction start")
        size = sizes[address]
        pages.update(range(address // PAGE, (address + size - 1) // PAGE + 1))
        byte_ranges.update(range(address, address + size))
    totals = re.findall(r"^totals: (\d+)\s*$", text, re.M)
    if len(totals) != 1:
        raise ValueError("expected one Ir-only Callgrind total")
    return {"ir": int(totals[0]), "pages": pages, "bytes": len(byte_ranges)}


def summarize(rows):
    return {key: (statistics.median([row[key] for row in rows]),
                  min(row[key] for row in rows), max(row[key] for row in rows)) for key in rows[0]}


def meets_residency_target(pr_kib, pin_kib, control_spread_kib):
    """The target is no excess above the pin beyond the 48 KiB ceiling.

    Residency below the pin satisfies the target too. A control spread wider
    than the accepted envelope cannot establish that target.
    """
    return control_spread_kib <= 48 and pr_kib <= pin_kib + 48


def teardown_command(args, facade, library, user, mode):
    return [args.bisect, "--teardown", facade, str(library), str(args.system_dir),
            str(user), mode, "0", "1", "1", "first-candidate", "-"]


def startup_toggles(library, facade, exports):
    """The policy's startup set: load constructors, then the init-to-M1 toggles."""
    return (*GEN["init_array_symbols"](library), *GEN["startup_toggles"](facade, exports))


def time_sample(output):
    """The worker's one cycle row; a closed row with dlclose time follows it."""
    rows = [json.loads(line) for line in output.splitlines() if line.strip()]
    samples = [row for row in rows if row.get("teardown") == "sample"]
    if len(samples) != 1:
        raise ValueError(f"expected one teardown sample row, got {len(samples)}")
    return samples[0]


def toggle_profile(args, facade, library, user, toggle, env, sizes, verify=None, stderr=None):
    command = [*GEN["callgrind_prefix"](args.bisect, args.cpu, toggle),
               *teardown_command(args, facade, library, user, "time")[1:]]
    output, err = run(command, env, profiler=True, native_stderr=stderr,
                      profiler_output_dir=args.profiler_output_dir)
    if verify is not None:
        verify(output)
    return executed_profile(err, library, sizes)


def stripped_sizes(libraries, work_dir, env):
    """Count stripped shipping copies in one run without changing source objects."""
    hashes = {key: digest(path) for key, path in libraries.items()}
    with GEN['collection_scratch'](work_dir) as root:
        for (arm, facade), library in libraries.items():
            temporary = root / 'strip.so'
            shutil.copyfile(library, temporary)
            run(['strip', '--strip-unneeded', temporary], env)
            print('stripped bytes', arm, facade, temporary.stat().st_size, flush=True)
            temporary.unlink()
    if any(digest(path) != hashes[key] for key, path in libraries.items()):
        raise ValueError('source library changed during stripping')


def allocation_totals(stderr):
    """DHAT's whole-process totals, without retaining its allocation profile."""
    total = re.findall(rb'Total:\s+([0-9,]+) bytes in ([0-9,]+) blocks', stderr)
    peak = re.findall(rb'At t-gmax:\s+([0-9,]+) bytes in ([0-9,]+) blocks', stderr)
    if len(total) != 1 or len(peak) != 1:
        raise ValueError('missing or repeated DHAT allocation totals')
    return dict(allocated_bytes=int(total[0][0].replace(b',', b'')),
                allocations=int(total[0][1].replace(b',', b'')),
                peak_heap_bytes=int(peak[0][0].replace(b',', b'')))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    ORDER['add_arguments'](parser)
    parser.add_argument("--subject", action="append", required=True,
                        help="base|pr|control|pin=prefix; additional names supply shipping sizes only")
    for name in ("bisect", "system-dir", "profiles-dir", "work-dir"):
        parser.add_argument("--" + name, type=Path, required=name == 'work-dir')
    parser.add_argument("--system-identity")
    parser.add_argument("--cpu", type=int)
    parser.add_argument("--rounds", type=int, default=20)
    parser.add_argument('--stripped-sizes-only', action='store_true',
                        help='compare stripped copies only; named subjects may span backends')
    registrations = parser.add_mutually_exclusive_group()
    registrations.add_argument('--record-registration', type=Path,
                               help='write a new run input from this bench’s own complete observations')
    registrations.add_argument('--registration', type=Path, help='require the bound observation registration')
    parser.add_argument('--base-checkout', type=Path, help='checkout that produced the baseline install')
    parser.add_argument('--profiler-output-dir', type=Path, help='retain complete local profiler streams; must be empty')
    args = parser.parse_args()
    if args.profiler_output_dir is not None:
        args.profiler_output_dir.mkdir(parents=True, exist_ok=True)
        if any(args.profiler_output_dir.iterdir()):
            parser.error('--profiler-output-dir must be empty')
    names = {value.split('=', 1)[0] for value in args.subject}
    prefixes = GEN['pairs'](args.subject, names, '--subject')
    all_libraries = {(arm, facade): (prefix / 'lib' / f'lib{facade}.so.15.0.0').resolve()
                     for arm, prefix in prefixes.items() for facade in FACADES}
    if args.stripped_sizes_only:
        if args.record_registration or args.registration:
            parser.error('registrations apply to the full bench')
        ORDER['check_arms'](args, {arm: {facade: all_libraries[arm, facade] for facade in FACADES}
                                   for arm in prefixes}, references=())
        stripped_sizes(all_libraries, args.work_dir, {**os.environ, 'LC_ALL': 'C'})
        return
    for name in ('bisect', 'system_dir', 'profiles_dir', 'system_identity', 'cpu'):
        if getattr(args, name) is None:
            parser.error('--' + name.replace('_', '-') + ' is required for a full campaign')
    if args.rounds < 20 or PAGE != 4096:
        parser.error("campaign requires >=20 rounds and a 4 KiB host page")
    if not set(ARMS) <= names:
        parser.error('--subject requires base, pr, control and pin')
    libraries = {key: library for key, library in all_libraries.items() if key[0] in ARMS}
    ORDER['check_arms'](args, {arm: {facade: all_libraries[arm, facade] for facade in FACADES}
                               for arm in prefixes}, references=('pin',))
    hashes = {key: digest(lib) for key, lib in all_libraries.items()}
    if any(hashes["pr", f] != hashes["control", f] for f in FACADES):
        raise ValueError("same-bytes control differs from PR")
    fs, _ = run(["findmnt", "-n", "-o", "FSTYPE", "-T", args.work_dir])
    if fs.strip() != b"ext4":
        raise ValueError("private measurement libraries must be on ext4")
    manifest = GEN["manifest"]
    if hashlib.sha256(manifest(args.system_dir).encode()).hexdigest() != args.system_identity:
        raise ValueError("system data identity differs")
    def fixtures():
        for profile in PROFILES:
            fixture = args.profiles_dir / profile
            if manifest(fixture / "user") != (fixture / "profile.manifest").read_text():
                raise ValueError("fixture manifest differs: " + profile)
    fixtures()
    env = {**os.environ, "LC_ALL": "C"}
    identities = None
    if args.record_registration or args.registration:
        if args.base_checkout is None:
            parser.error('--base-checkout is required with a registration')
        identities = registration.git_identity(args.base_checkout, Path(__file__).resolve().parents[1])
        binding = dict(**identities, libraries={f'{arm}/{facade}': value for (arm, facade), value in hashes.items()},
                       system_identity=args.system_identity,
                       profiles_identity=hashlib.sha256(manifest(args.profiles_dir).encode()).hexdigest(),
                       worker_source=worker.sha256((worker.ROOT / 'tools/workers/startup.c').read_bytes()),
                       bisect_sha256=digest(args.bisect))
        expected = (registration.Registration(binding, recording=True) if args.record_registration else
                    registration.Registration.load(args.registration, binding))
    else:
        expected = None
    for key, value in hashes.items():
        print("library", *key, value, flush=True)
    print("cold proof: private copy + fsync + POSIX_FADV_DONTNEED + zero-resident mincore; libraries only", flush=True)
    with GEN["collection_scratch"](args.work_dir) as root:
        env["TMPDIR"] = str(root)
        serial = 0
        def fresh(arm, facade, profile):
            nonlocal serial
            serial += 1
            work = root / f"s{serial:06d}"
            work.mkdir()
            library = work / "lib.so"
            shutil.copyfile(libraries[arm, facade], library)
            user = work / "user"
            shutil.copytree(args.profiles_dir / profile / "user", user)
            return work, library, user
        def timing_child(facade, library, user):
            return ["taskset", "-c", str(args.cpu),
                    *teardown_command(args, facade, library, user, "time")]
        def memory_child(facade, library, user):
            return ["taskset", "-c", str(args.cpu),
                    *teardown_command(args, facade, library, user, "memory")]
        native_reference = {}
        instruction_references = {}
        if expected is not None:
            executable, identity = worker.build(root, env, ())
            affinity = os.sched_getaffinity(0)
            os.sched_setaffinity(0, {args.cpu})
            try:
                campaign = worker.Worker(executable, identity, args.system_dir, root, env, (), args.cpu, 'now')
                library_arms = {arm: {facade: libraries[arm, facade] for facade in FACADES} for arm in ARMS}
                profiles = {name: args.profiles_dir / name / 'user' for name in PROFILES}
                for facade in FACADES:
                    for profile in PROFILES:
                        instruction_references[facade, profile] = startup_controller.verify(campaign, library_arms, facade, profile, profiles,
                                       arms=ARMS, registration=expected, hostile=False)
            finally:
                os.sched_setaffinity(0, affinity)
            # The same complete first-candidate observation is checked in
            # record mode and in every subsequent timed sample.
            for facade in FACADES:
                env['OXPINYIN_TEARDOWN_EXPORTS'] = GEN['teardown_exports'](libraries['pr', facade], facade)
                for profile in PROFILES:
                    observations = {}
                    for arm in ARMS:
                        work, library, user = fresh(arm, facade, profile)
                        try:
                            output, error = run(timing_child(facade, library, user), env)
                            observations[arm] = dict(observations=time_sample(output)['observations'],
                                                     stderr_hex=error.hex(), userfiles=worker.userfiles(user))
                        finally:
                            shutil.rmtree(work)
                    expected.check('first-candidate', facade, profile, observations)
                    native_reference[facade, profile] = observations
            if args.record_registration:
                fixtures()
                if any(digest(lib) != hashes[key] for key, lib in all_libraries.items()):
                    raise ValueError('measured library changed during recording')
                if registration.git_identity(args.base_checkout, Path(__file__).resolve().parents[1]) != identities:
                    raise ValueError('Git identity changed during recording')
                count = expected.write(args.record_registration)
                print('registration entries', count, flush=True)
                return
            print('registration entries', len(expected.entries), flush=True)
            # Use main's shared fresh-process/RandomState validation unchanged,
            # adding the pin as context while keeping PR/control qualification.
            affinity = os.sched_getaffinity(0)
            os.sched_setaffinity(0, {args.cpu})
            try:
                for facade in FACADES:
                    backends = {arm: ORDER['library_backend'](libraries[arm, facade]) for arm in ARMS}
                    for profile in PROFILES:
                        startup_controller.instrument(campaign, library_arms, facade, profile,
                            profiles[profile], instruction_references[facade, profile], arms=ARMS,
                            backends=backends)
            finally:
                os.sched_setaffinity(0, affinity)
        stripped_sizes(all_libraries, args.work_dir, env)
        for facade in FACADES:
            env["OXPINYIN_TEARDOWN_EXPORTS"] = GEN["teardown_exports"](libraries[("pr", facade)], facade)
            for profile in PROFILES:
                rows = defaultdict(list)
                traces = set()
                cold_proofs = 0
                for round_no in range(-2, args.rounds):
                    order = ARMS[round_no % 4:] + ARMS[:round_no % 4]
                    round_observations = {}
                    for arm in order:
                        for cold in (False, True):
                            work, library, user = fresh(arm, facade, profile)
                            check_tkrzw("auto", "measure-function-order.py", arm, library, env=env)
                            try:
                                prepare_cache(library, cold)
                                output, error = run(timing_child(facade, library, user), env)
                                sample = time_sample(output)
                                observation = dict(observations=sample['observations'], stderr_hex=error.hex(),
                                                   userfiles=worker.userfiles(user))
                                if arm in round_observations and round_observations[arm] != observation:
                                    raise ValueError('warm/cold observations differ')
                                round_observations[arm] = observation
                                traces.add(hashlib.sha256(json.dumps(
                                    sample["observations"], sort_keys=True).encode()).hexdigest())
                                timing = sample["times"]
                                reading = {
                                    "init_ns": timing["init_ns"]["wall_ns"],
                                    "load_ns": timing["load_ns"]["wall_ns"],
                                    "startup_ns": timing["init_ns"]["wall_ns"] + timing["load_ns"]["wall_ns"],
                                    "first_candidate_ns": timing["first_candidate_ns"]["wall_ns"],
                                    "lifecycle_ns": timing["lifecycle_ns"]["wall_ns"],
                                }
                                if round_no >= 0:
                                    rows[arm, "cold" if cold else "warm"].append(reading)
                                    cold_proofs += cold
                            finally:
                                shutil.rmtree(work)
                        work, library, user = fresh(arm, facade, profile)
                        try:
                            prepare_cache(library, False)
                            reading, answers, error = teardown_memory(memory_child(facade, library, user), library, env)
                            observation = dict(observations=answers, stderr_hex=error.hex(), userfiles=worker.userfiles(user))
                            if observation != round_observations[arm]:
                                raise ValueError('time/memory observations differ')
                            if round_no >= 0:
                                rows[arm, "memory"].append(reading)
                        finally:
                            shutil.rmtree(work)
                    if expected is not None:
                        expected.check('first-candidate', facade, profile, round_observations)
                    if round_no >= 0 and (round_no + 1) % 5 == 0:
                        print("native progress", facade, profile, round_no + 1, "/", args.rounds, flush=True)
                if expected is None and len(traces) != 1:
                    raise ValueError("native first-candidate answer traces differ")
                print("CELL", facade, profile, "traces", len(traces), "verified cold samples", cold_proofs, flush=True)
                for key, samples in rows.items():
                    print("native", facade, profile, *key, "median,min,max", summarize(samples), flush=True)
                memory = {arm: rows[arm, "memory"] for arm in ARMS}
                spread = max(abs(p["init_text_pages"] - c["init_text_pages"]) * PAGE / 1024
                             for p, c in zip(memory["pr"], memory["control"], strict=True))
                effective = max(48, spread)
                med = {arm: statistics.median(r["init_text_pages"] for r in samples) * PAGE / 1024
                       for arm, samples in memory.items()}
                print("residency", facade, profile, "same-byte paired max spread KiB", spread,
                      "effective envelope KiB", effective, "base-PR gain KiB", med["base"] - med["pr"],
                      "PR-pin gap KiB", med["pr"] - med["pin"],
                      "gain established", med["base"] - med["pr"] >= effective,
                      "pin envelope met", meets_residency_target(med["pr"], med["pin"], spread), flush=True)
                profile_metrics = {}
                # Deterministic counters use the startup toggles the policy
                # was generated from and the existing steady toggle, outside
                # native timing samples.
                for arm in ARMS:
                    # These sizes are for libraries[arm, facade]; fresh()
                    # copies exactly those bytes to each private lib.so.
                    sizes = instruction_sizes(libraries[arm, facade])
                    reference = native_reference.get((facade, profile), {}).get(arm)
                    def verify_output(output):
                        if reference is not None:
                            observed = dict(observations=time_sample(output)['observations'],
                                            stderr_hex=reference['stderr_hex'], userfiles=worker.userfiles(user))
                            if observed != reference:
                                raise ValueError('instrumented observations differ from verification')
                    native_error = bytes.fromhex(reference['stderr_hex']) if reference is not None else None
                    startup = defaultdict(list)
                    for toggle in startup_toggles(libraries[arm, facade], facade,
                                                  env["OXPINYIN_TEARDOWN_EXPORTS"]):
                        work, library, user = fresh(arm, facade, profile)
                        try:
                            startup[toggle].append(toggle_profile(args, facade, library, user, toggle, env, sizes, verify_output, native_error))
                        finally:
                            shutil.rmtree(work)
                    first_candidate = GEN["first_candidate_toggles"](facade, env["OXPINYIN_TEARDOWN_EXPORTS"])
                    def total(toggles):
                        profiles = [p for toggle in toggles for p in startup[toggle]]
                        return {"ir": sum(p["ir"] for p in profiles),
                                "pages": set().union(*(p["pages"] for p in profiles))}
                    combined = total(startup)
                    init = total(toggle for toggle in startup if toggle not in first_candidate)
                    first = total(first_candidate)
                    work, library, user = fresh(arm, facade, profile)
                    try:
                        _, error = run([*GEN["callgrind_prefix"](args.bisect, args.cpu, "bisect_perf_steady_unit"),
                                        "--perf", str(library), str(args.system_dir)],
                                       {**env, "PERF_FACADE": facade, "PERF_MODE": "speed",
                                        "PERF_CYCLES": "4", "PERF_REPEATS": "1",
                                        "PERF_WORKLOAD_CHECKSUM": "1"}, profiler=True,
                                       profiler_output_dir=args.profiler_output_dir)
                        steady = executed_profile(error, library, sizes)
                    finally:
                        shutil.rmtree(work)
                    work, library, user = fresh(arm, facade, profile)
                    try:
                        heap_profile = (args.profiler_output_dir / f'dhat-{facade}-{profile}-{arm}.json'
                                        if args.profiler_output_dir is not None else Path('/dev/null'))
                        output, error = run(["taskset", "-c", str(args.cpu), "valgrind", "--tool=dhat",
                                        '--dhat-out-file=' + str(heap_profile),
                                        *teardown_command(args, facade, library, user, "time")], env,
                                            profiler=True, native_stderr=native_error,
                                            profiler_output_dir=args.profiler_output_dir)
                        verify_output(output)
                        allocations = allocation_totals(error)
                    finally:
                        shutil.rmtree(work)
                    profile_metrics[arm] = dict(startup_ir=combined['ir'],
                        allocated_bytes=allocations['allocated_bytes'], peak_heap_bytes=allocations['peak_heap_bytes'],
                        through_first_ns=statistics.median(row['startup_ns'] + row['first_candidate_ns']
                                                          for row in rows[arm, 'warm']))
                    constructors = GEN["init_array_symbols"](libraries[arm, facade])
                    load_pages = set().union(*(p["pages"] for toggle in constructors
                                               for p in startup.get(toggle, []))) if constructors else set()
                    print("profile", facade, profile, arm, "load executed pages", len(load_pages),
                          "through init pages", len(init["pages"]),
                          "through M1 pages", len(combined["pages"]),
                          "init Ir", init["ir"], "first-candidate Ir", first["ir"],
                          "startup Ir", combined["ir"], "steady Ir", steady["ir"],
                          "steady pages", len(steady["pages"]),
                          "peak heap B", allocations['peak_heap_bytes'],
                          "allocations", allocations['allocations'],
                          "allocated bytes", allocations['allocated_bytes'], flush=True)
                print('base/PR/pin metrics', facade, profile, json.dumps(profile_metrics, sort_keys=True), flush=True)
                if (profile_metrics['pr']['peak_heap_bytes'] > profile_metrics['pin']['peak_heap_bytes'] and
                        (profile_metrics['pr']['startup_ir'] > profile_metrics['pin']['startup_ir'] or
                         profile_metrics['pr']['through_first_ns'] > profile_metrics['pin']['through_first_ns'])):
                    raise RuntimeError('STOP: PR time/instructions and peak heap are both worse than pin')
        for facade in FACADES:
            measurements = defaultdict(list)
            checksums = set()
            for round_no in range(-2, args.rounds):
                order = ARMS[round_no % 4:] + ARMS[:round_no % 4]
                for arm in order:
                    e = {**env, "PERF_FACADE": facade, "PERF_MODE": "speed",
                         "PERF_CYCLES": "8", "PERF_REPEATS": "1",
                         "PERF_WORKLOAD_CHECKSUM": "1"}
                    check_tkrzw("auto", "measure-function-order.py", arm,
                                libraries[arm, facade], env=e)
                    output, _ = run(["taskset", "-c", str(args.cpu), args.bisect, "--perf",
                                     libraries[arm, facade], args.system_dir], e)
                    row = json.loads(output)
                    if len(row["cycles_ns"]) != 8:
                        raise ValueError("steady cycle count differs")
                    checksums.add(row["workload_checksum"])
                    if round_no >= 0:
                        measurements[arm].append(statistics.median(row["cycles_ns"][1:]))
            if len(checksums) != 1:
                raise ValueError("steady workload fingerprints differ")
            spread = max(abs(p - c) for p, c in zip(measurements["pr"], measurements["control"], strict=True))
            delta = statistics.median(measurements["pr"]) - statistics.median(measurements["base"])
            print("steady", facade, "checksum", checksums.pop(), "same-byte paired max spread ns", spread,
                  "PR-base median delta ns", delta, flush=True)
            for arm, values in measurements.items():
                print("steady", facade, arm, "median,min,max ns", statistics.median(values), min(values), max(values), flush=True)
            if delta > spread:
                raise RuntimeError("STOP: steady guess-path slowdown exceeds same-bytes control spread")
    fixtures()
    if any(digest(lib) != hashes[key] for key, lib in all_libraries.items()):
        raise ValueError("measured library changed during campaign")
    if identities is not None and registration.git_identity(args.base_checkout, Path(__file__).resolve().parents[1]) != identities:
        raise ValueError('Git identity changed during measurement')
    print("campaign complete; fixture manifests and all library hashes unchanged; no raw records retained", flush=True)


if __name__ == "__main__":
    main()
