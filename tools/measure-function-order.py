#!/usr/bin/env -S uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal
"""Linux function-layout campaign using the in-tree bisect worker.

Run through just heavy. No observations or profiler dumps are saved. The only
scratch files are private library/profile copies, deleted on exit and signals.
The worker is tools/bisection/bisect.c in its teardown mode: native timing uses
its time stop, memory uses its parent-observed checkpoints, and Callgrind uses
toggles around the load constructors and the generator's startup toggles:
the facade init function and the exports the worker calls from there to M1.
"""

import argparse
from collections import defaultdict
import ctypes
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


GEN = runpy.run_path(str(Path(__file__).with_name("generate-function-order.py")))
ORDER = runpy.run_path(str(Path(__file__).parent / 'packaging/function_order.py'))
ARMS = ("base", "pr", "control", "pin")
FACADES = ("pinyin", "zhuyin")
PROFILES = ("phrases", "journals")
PAGE = os.sysconf("SC_PAGE_SIZE")


def run(command, env=None):
    """Keep bytes until success is known; reap a child before scratch cleanup."""
    with subprocess.Popen(list(map(str, command)), stdout=subprocess.PIPE,
                          stderr=subprocess.PIPE, env=env, start_new_session=True) as child:
        try:
            out, err = child.communicate(timeout=1800)
        except BaseException:
            os.killpg(child.pid, signal.SIGKILL)
            child.wait()
            raise
        if child.returncode:
            raise RuntimeError(f"child exit {child.returncode}: {command}\n"
                               + out[-2000:].decode(errors="backslashreplace")
                               + err[-4000:].decode(errors="backslashreplace"))
        return out, err


def digest(path):
    with path.open("rb") as handle:
        return hashlib.file_digest(handle, "sha256").hexdigest()


def cache_pages(path):
    """mincore(PROT_NONE mapping) observes file cache without faulting it in."""
    libc = ctypes.CDLL(None, use_errno=True)
    libc.mmap.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_int,
                          ctypes.c_int, ctypes.c_int, ctypes.c_long]
    libc.mmap.restype = ctypes.c_void_p
    libc.mincore.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p]
    libc.munmap.argtypes = [ctypes.c_void_p, ctypes.c_size_t]
    size = path.stat().st_size
    with path.open("rb") as handle:
        address = libc.mmap(None, size, 0, 1, handle.fileno(), 0)  # PROT_NONE, MAP_SHARED
        if address == ctypes.c_void_p(-1).value:
            raise OSError(ctypes.get_errno(), "mmap for mincore")
        try:
            vector = (ctypes.c_ubyte * math.ceil(size / PAGE))()
            if libc.mincore(address, size, vector):
                raise OSError(ctypes.get_errno(), "mincore")
            return sum(bool(value & 1) for value in vector), len(vector)
        finally:
            if libc.munmap(address, size):
                raise OSError(ctypes.get_errno(), "munmap")


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
    load, init = snapshots["M0"], snapshots["M1"]
    return {
        "load_text_pages": load["text_pages"], "load_text_pss_kib": load["text_pss_kib"],
        "init_text_pages": init["text_pages"], "init_text_pss_kib": init["text_pss_kib"],
        "init_total_pss_kib": init["total_pss_kib"],
        "load_minor_faults": load["minor_faults"],
        "init_minor_faults": init["minor_faults"] - load["minor_faults"],
        "init_heap_bytes": (init["allocator"] or {}).get("uordblks"),
    }


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


def toggle_profile(args, facade, library, user, toggle, env, sizes):
    command = [*GEN["callgrind_prefix"](args.bisect, args.cpu, toggle),
               *teardown_command(args, facade, library, user, "time")[1:]]
    _, err = run(command, env)
    return executed_profile(err, library, sizes)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    ORDER['add_arguments'](parser)
    parser.add_argument("--subject", action="append", required=True, help="base|pr|control|pin=installed prefix")
    for name in ("bisect", "system-dir", "profiles-dir", "work-dir"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--system-identity", required=True)
    parser.add_argument("--cpu", type=int, required=True)
    parser.add_argument("--rounds", type=int, default=20)
    args = parser.parse_args()
    if args.rounds < 20 or PAGE != 4096:
        parser.error("campaign requires >=20 rounds and a 4 KiB host page")
    prefixes = GEN["pairs"](args.subject, set(ARMS), "--subject")
    libraries = {(arm, facade): (prefixes[arm] / "lib" / f"lib{facade}.so.15.0.0").resolve()
                 for arm in ARMS for facade in FACADES}
    ORDER['check_arms'](args, {arm: {facade: libraries[arm, facade] for facade in FACADES}
                               for arm in ARMS}, references=('pin',))
    hashes = {key: digest(lib) for key, lib in libraries.items()}
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
        for facade in FACADES:
            env["OXPINYIN_TEARDOWN_EXPORTS"] = GEN["teardown_exports"](libraries[("pr", facade)], facade)
            for profile in PROFILES:
                rows = defaultdict(list)
                traces = set()
                cold_proofs = 0
                for round_no in range(-2, args.rounds):
                    order = ARMS[round_no % 4:] + ARMS[:round_no % 4]
                    for arm in order:
                        for cold in (False, True):
                            work, library, user = fresh(arm, facade, profile)
                            try:
                                prepare_cache(library, cold)
                                output, _ = run(timing_child(facade, library, user), env)
                                sample = time_sample(output)
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
                            reading = teardown_memory(memory_child(facade, library, user), library, env)
                            if round_no >= 0:
                                rows[arm, "memory"].append(reading)
                        finally:
                            shutil.rmtree(work)
                    if round_no >= 0 and (round_no + 1) % 5 == 0:
                        print("native progress", facade, profile, round_no + 1, "/", args.rounds, flush=True)
                if len(traces) != 1:
                    raise ValueError("native first-candidate answer traces differ")
                print("CELL", facade, profile, "trace", traces.pop(), "verified cold samples", cold_proofs, flush=True)
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
                # Deterministic counters use the startup toggles the policy
                # was generated from and the existing steady toggle, outside
                # native timing samples.
                for arm in ARMS:
                    # These sizes are for libraries[arm, facade]; fresh()
                    # copies exactly those bytes to each private lib.so.
                    sizes = instruction_sizes(libraries[arm, facade])
                    startup = defaultdict(list)
                    for toggle in startup_toggles(libraries[arm, facade], facade,
                                                  env["OXPINYIN_TEARDOWN_EXPORTS"]):
                        work, library, user = fresh(arm, facade, profile)
                        try:
                            startup[toggle].append(toggle_profile(args, facade, library, user, toggle, env, sizes))
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
                                        "PERF_WORKLOAD_CHECKSUM": "1"})
                        steady = executed_profile(error, library, sizes)
                    finally:
                        shutil.rmtree(work)
                    work, library, user = fresh(arm, facade, profile)
                    try:
                        _, error = run(["taskset", "-c", str(args.cpu), "valgrind", "--tool=dhat",
                                        "--dhat-out-file=/dev/null",
                                        *teardown_command(args, facade, library, user, "time")], env)
                        peaks = re.findall(rb"At t-gmax:\s+([0-9,]+) bytes in ([0-9,]+) blocks", error)
                        if len(peaks) != 1:
                            raise ValueError("DHAT peak missing or repeated")
                    finally:
                        shutil.rmtree(work)
                    constructors = GEN["init_array_symbols"](libraries[arm, facade])
                    load_pages = set().union(*(p["pages"] for toggle in constructors
                                               for p in startup.get(toggle, []))) if constructors else set()
                    print("profile", facade, profile, arm, "load executed pages", len(load_pages),
                          "through init pages", len(init["pages"]),
                          "through M1 pages", len(combined["pages"]),
                          "init Ir", init["ir"], "first-candidate Ir", first["ir"],
                          "startup Ir", combined["ir"], "steady Ir", steady["ir"],
                          "steady pages", len(steady["pages"]),
                          "peak heap B", int(peaks[0][0].replace(b",", b"")), flush=True)
        for facade in FACADES:
            measurements = defaultdict(list)
            checksums = set()
            for round_no in range(-2, args.rounds):
                order = ARMS[round_no % 4:] + ARMS[:round_no % 4]
                for arm in order:
                    e = {**env, "PERF_FACADE": facade, "PERF_MODE": "speed",
                         "PERF_CYCLES": "8", "PERF_REPEATS": "1",
                         "PERF_WORKLOAD_CHECKSUM": "1"}
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
        for (arm, facade), library in libraries.items():
            temporary = root / "strip.so"
            shutil.copyfile(library, temporary)
            run(["strip", "--strip-unneeded", temporary], env)
            print("stripped bytes", arm, facade, temporary.stat().st_size, flush=True)
            temporary.unlink()
    fixtures()
    if any(digest(lib) != hashes[key] for key, lib in libraries.items()):
        raise ValueError("measured library changed during campaign")
    print("campaign complete; fixture manifests and all library hashes unchanged; no raw records retained", flush=True)


if __name__ == "__main__":
    main()
