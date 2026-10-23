"""Teardown workload for contract-diff.py and bisect.c. Standard library only.

The controller never runs in the measured native child. Observations, smaps and
profiler output remain in memory. Only private fixture inputs/manifests and the
native executable are materialized, under the caller's temporary work directory.
"""
import collections
import contextlib
import ctypes
import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path
import random
import re
import runpy
import selectors
import shutil
import stat
import statistics
import subprocess
import sys
import threading
import time

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
FIELDS = ('Rss', 'Pss', 'Anonymous', 'Private_Clean', 'Private_Dirty', 'Shared_Clean')
PROFILES = ('empty', 'modest', 'phrases', 'journals')
CASES = (*PROFILES, 'modest-unsaved', 'modest-saved')
# Accepted before the first sample. Do not tune these from campaign output.
CONTROL_INTERVAL = (0.95, 1.05)
RESIDENCY_FLOOR_KIB = 64
RESIDENCY_FRACTION = 0.01
# The control is the subject's bytes at a second path. With `libraries/ox`
# against `libraries/control`, #1030 measured the control at 15 B more heap and
# 12-354 more Ir, consistent with its 5-byte longer path (the loader and the
# library keep path strings). `subject`, `control` and `pinlibs` are 7 bytes
# each, so neither control-ox nor pin-ox figures carry a path-length difference.
LIBRARY_DIRECTORIES = dict(ox='subject', control='control', pin='pinlibs')
TIMING_ROUNDS, TIMING_WARMUPS = 60, 3
MEMORY_ROUNDS, MEMORY_WARMUPS = 30, 2


def sibling(name):
    spec = importlib.util.spec_from_file_location(name.replace('-', '_'), HERE / (name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


SMAPS = sibling('rss-smaps')
STARTUP_PROFILE = sibling('startup_profile')
FUNCTION_ORDER = runpy.run_path(str(ROOT / 'tools/packaging/function_order.py'))


def digest_bytes(value):
    return hashlib.sha256(value).hexdigest()


def file_set(directory):
    return {str(path.relative_to(directory)): (path.stat().st_size, digest_bytes(path.read_bytes()))
            for path in sorted(Path(directory).rglob('*')) if path.is_file()}


def identity(files):
    return digest_bytes(json.dumps(files, sort_keys=True, separators=(',', ':')).encode())


def profile_contents(driver, directory):
    """Reuse contract-diff's DB row/padding rules, retaining every file/row.

    BDB page headers include random file identities and LSNs; Bigram::save_db
    recreates the container (src/storage/ngram_bdb.cpp:81-105 at 074a2219).
    Raw input identities are recorded separately, never used as semantic DB
    equality. The existing registered padding/timestamp rules also apply.
    """
    result = {}
    for path in sorted(Path(directory).rglob('*')):
        if not path.is_file():
            continue
        raw = path.read_bytes()
        name = str(path.relative_to(directory))
        if raw[12:16] in (b'\x62\x31\x05\x00', b'\x61\x15\x06\x00'):
            rows = driver.index_file_rows(path)
            if path.name == 'user_pinyin_index.bin':
                rows = [[key, driver.zero_item_padding(key, value)] for key, value in rows]
            result[name] = dict(format='bdb-rows', rows=rows)
        else:
            if path.name in ('user.bin', 'addon.bin', 'network.bin'):
                raw = raw[:4] + b'\0\0\0\0' + raw[8:]
            result[name] = dict(bytes=len(raw), sha256=digest_bytes(raw))
    return result


def exports(facade):
    crate, version = ('oxpinyin-capi', 'libpinyin.ver') if facade == 'pinyin' else ('oxpinyin-zhuyin-capi', 'libzhuyin.ver')
    return re.findall(r'\b' + facade + r'_\w+(?=\s*;)', (ROOT / 'crates' / crate / version).read_text())


def build_worker(work, instrumentation=None):
    if instrumentation not in (None, 'massif', 'memcheck'):
        raise ValueError('unknown worker instrumentation')
    executable = work / ('bisect-teardown' + ('-' + instrumentation if instrumentation else ''))
    command = ['cc', '-std=gnu11', '-Wall', '-Wextra', '-Werror', '-O2', '-g',
               '-o', str(executable), str(HERE / 'bisect.c')]
    if sys.platform != 'darwin':
        command.append('-ldl')
    if instrumentation:
        command.append('-DOXPINYIN_TEARDOWN_' + instrumentation.upper() + '=1')
    subprocess.run(command, check=True)
    return executable, dict(command=command, sha256=digest_bytes(executable.read_bytes()),
                            bytes=executable.stat().st_size,
                            sources={name: digest_bytes((HERE / name).read_bytes())
                                     for name in ('bisect.c', 'teardown.h')})


def mapping_group(row, library, data):
    name, perms = row['name'], row['perms']
    if not name or name.startswith('[anon') or name in ('[heap]', '[stack]'):
        return 'ANON:' + (name or '[anonymous]')
    access = 'text' if 'x' in perms else 'writable' if 'w' in perms else 'readonly'
    path = name.removesuffix(' (deleted)')
    if path == str(library):
        return 'SUBJECT:' + access
    if path.startswith(str(data) + '/'):
        return 'DATA:' + str(Path(path).relative_to(data)) + ':' + access
    if '.so' in Path(path).name:
        return 'LIB:' + Path(path).name + ':' + access
    return 'OTHER:' + name + ':' + access


def read_memory(pid, library, data, *, proc_root=Path('/proc')):
    proc = proc_root / str(pid)
    with (proc / 'smaps').open() as handle:
        rows = SMAPS.parse_lines(handle, fields=FIELDS)
    totals = {field: sum(row[field] for row in rows) for field in FIELDS}
    rollup = {}
    for line in (proc / 'smaps_rollup').read_text().splitlines():
        key, _, value = line.partition(':')
        if key in FIELDS:
            rollup[key] = int(value.split()[0])
    # These are sequential kernel reads. Even a blocked child cannot freeze
    # shared-page classification changes caused by another process. Use the
    # same smaps read for totals and groups; retain the later rollup only as a
    # diagnostic. Pss here sums the kernel's per-mapping rounded KiB values.
    groups = {}
    for row in rows:
        group = groups.setdefault(mapping_group(row, library, data), dict.fromkeys(FIELDS, 0))
        for field in FIELDS:
            group[field] += row[field]
    return dict(totals=totals, mapping_totals=totals, rollup=rollup,
                rollup_minus_smaps=signed_delta(rollup, totals), groups=groups,
                mappings=len(rows), fds=len(list((proc / 'fd').iterdir())))


def signed_delta(a, b):
    """a minus b, keeping negative values, including disappearing groups."""
    return {key: a.get(key, 0) - b.get(key, 0) for key in sorted(set(a) | set(b))}


def memory_deltas(checkpoints):
    result = {}
    for label, a, b in (('M1-M2', 'M1', 'M2'), ('M2-M3', 'M2', 'M3'), ('M3-M0', 'M3', 'M0')):
        left, right = checkpoints[a], checkpoints[b]
        groups = {key: signed_delta(left['groups'].get(key, {}), right['groups'].get(key, {}))
                  for key in sorted(set(left['groups']) | set(right['groups']))}
        result[label] = dict(totals=signed_delta(left['totals'], right['totals']), groups=groups,
                             fds=left['fds'] - right['fds'], mappings=left['mappings'] - right['mappings'])
        if left['allocator'] is not None and right['allocator'] is not None:
            result[label]['allocator'] = signed_delta(left['allocator'], right['allocator'])
    return result


def percentile(values, fraction):
    ordered = sorted(values)
    position = (len(ordered) - 1) * fraction
    lo, hi = math.floor(position), math.ceil(position)
    return ordered[lo] + (ordered[hi] - ordered[lo]) * (position - lo)


def summary(values):
    if not values or any(not math.isfinite(value) for value in values):
        raise ValueError('missing or nonfinite observations')
    middle = statistics.median(values)
    return dict(n=len(values), median=middle, minimum=min(values), maximum=max(values),
                p25=percentile(values, .25), p75=percentile(values, .75),
                range_over_median=(max(values) - min(values)) / abs(middle) if middle else None)


def paired_bootstrap(values):
    generator = random.Random(0x074A2219)
    resamples = [statistics.median(generator.choices(values, k=len(values))) for _ in range(4000)]
    return dict(median=statistics.median(values), ci95=[percentile(resamples, .025), percentile(resamples, .975)])


def paired_ratio(numerator, denominator):
    if len(numerator) != len(denominator) or not numerator or any(value <= 0 for value in denominator):
        raise ValueError('paired ratios require equal, positive-denominator samples')
    return paired_bootstrap([a / b for a, b in zip(numerator, denominator)])


def paired_difference(minuend, subtrahend):
    if len(minuend) != len(subtrahend) or not minuend:
        raise ValueError('paired differences require equal, nonempty samples')
    return paired_bootstrap([a - b for a, b in zip(minuend, subtrahend)])


def within(estimate, lo, hi):
    return lo <= estimate['ci95'][0] <= estimate['median'] <= estimate['ci95'][1] <= hi


def stable_control(ox, control):
    ratio = paired_ratio(control, ox)
    return dict(ratio=ratio, stable=within(ratio, *CONTROL_INTERVAL), rule=list(CONTROL_INTERVAL))


def stable_residency(ox, control):
    # §7.5 item 2: d = control - ox in KiB; the median and the whole
    # interval lie within +-max(64 KiB, 1% of |median(ox)|), inclusive.
    difference = paired_difference(control, ox)
    band = max(RESIDENCY_FLOOR_KIB, RESIDENCY_FRACTION * abs(statistics.median(ox)))
    return dict(difference=difference, stable=within(difference, -band, band), bound_kib=band,
                rule=dict(floor_kib=RESIDENCY_FLOOR_KIB, fraction=RESIDENCY_FRACTION))


def exact_control(ox, control):
    # §7.5 item 3: scoped instruction counts and deterministic heap
    # accounting must be equal. No tolerance: any difference is INVALID.
    return dict(rule='exact', ox=ox, control=control, difference=control - ox,
                verdict='VALID' if control == ox else 'INVALID')


def filesystem(path):
    """Read the actual mount type; ext2/3/4 share statfs's magic number."""
    path = Path(path).resolve()
    matches = []
    for line in Path('/proc/self/mountinfo').read_text().splitlines():
        fields, separator, trailer = line.partition(' - ')
        if not separator:
            continue
        mount = re.sub(r'\\([0-7]{3})', lambda match: chr(int(match[1], 8)), fields.split()[4])
        if path.is_relative_to(mount):
            matches.append((len(mount), trailer.split()[0]))
    if not matches:
        raise ValueError('filesystem type is unknown')
    return max(matches)[1]


# glibc and its dynamic loader remain warm. C++/GCC runtimes and all other
# DT_NEEDED dependencies (including transitive ones) belong to the cold set.
C_RUNTIME = re.compile(r'^(?:lib(?:c|m|dl|pthread|rt|resolv|util|anl)\.so\.[0-9]+|ld-linux[^/]*\.so\.[0-9]+)$')


def library_dependencies(library):
    """Resolve the ELF DT_NEEDED closure without loading it in the controller.

    ldd uses the native loader's search rules, including LD_LIBRARY_PATH and
    RPATH/RUNPATH. readelf restricts the result to actual DT_NEEDED edges.
    All inspection finishes before eviction; no library bytes are read after it.
    """
    output = subprocess.check_output(['ldd', str(library)], text=True)
    resolved = {}
    for line in output.splitlines():
        if '=> not found' in line:
            raise ValueError('unresolved cold dependency: ' + line.strip())
        match = re.match(r'\s*(\S+)\s+=>\s+(/.*?)\s+\(0x[0-9a-fA-F]+\)\s*$', line)
        if match:
            resolved[match[1]] = Path(match[2]).resolve(strict=True)
    pending, visited, files = [Path(library).resolve(strict=True)], set(), []
    while pending:
        path = pending.pop()
        if path in visited:
            continue
        visited.add(path)
        dynamic = subprocess.check_output(['readelf', '-d', str(path)], text=True)
        for name in re.findall(r'\(NEEDED\).*?\[([^\]]+)\]', dynamic):
            if C_RUNTIME.fullmatch(name):
                continue
            if name not in resolved:
                raise ValueError(f'unresolved cold DT_NEEDED dependency: {path}: {name}')
            dependency = resolved[name]
            files.append(dependency)
            pending.append(dependency)
    return sorted(set(files))


def cold_files(library, data, user):
    # Use the worker's private, restored user tree, never its source profile.
    # Include every regular data/profile file, not a hard-coded stock filename
    # list: table.conf and non-stock library layouts are valid inputs too.
    return [Path(library), *library_dependencies(library),
            *(path for root in (data, user) for path in sorted(root.rglob('*')) if path.is_file())]


def resident_pages(fd):
    """Linux mincore over an unfaulted PROT_NONE mapping; no file bytes read.

    Linux can conservatively report all pages resident when the caller lacks
    mincore permissions. That must refuse a sample, never count as eviction.
    """
    size = os.fstat(fd).st_size
    if not size:
        return dict(pages=0, resident=0)
    libc = ctypes.CDLL(None, use_errno=True)
    libc.mmap.argtypes = (ctypes.c_void_p, ctypes.c_size_t, ctypes.c_int,
                          ctypes.c_int, ctypes.c_int, ctypes.c_long)
    libc.mmap.restype = ctypes.c_void_p
    libc.mincore.argtypes = (ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p)
    libc.mincore.restype = ctypes.c_int
    libc.munmap.argtypes = (ctypes.c_void_p, ctypes.c_size_t)
    libc.munmap.restype = ctypes.c_int
    # PROT_NONE=0, MAP_SHARED=1 on Linux. This mapping never pins/faults pages.
    address = libc.mmap(None, size, 0, 1, fd, 0)
    if address == ctypes.c_void_p(-1).value:
        raise OSError(ctypes.get_errno(), 'cold mincore mmap failed')
    try:
        pages = (size + os.sysconf('SC_PAGE_SIZE') - 1) // os.sysconf('SC_PAGE_SIZE')
        vector = (ctypes.c_ubyte * pages)()
        if libc.mincore(address, size, vector):
            raise OSError(ctypes.get_errno(), 'cold mincore failed')
        return dict(pages=pages, resident=sum(value & 1 for value in vector))
    finally:
        if libc.munmap(address, size):
            raise OSError(ctypes.get_errno(), 'cold mincore munmap failed')


def evict_files(files):
    """Evict only named ext4 files, then verify the complete set before launch.

    Keep descriptors open so every check and fadvise refers to the same inode.
    Flush only these files: newly copied private profiles can have dirty pages,
    which DONTNEED would otherwise leave resident. No global cache operation.
    """
    if sys.platform != 'linux':
        raise ValueError('cold teardown requires native Linux and ext4')
    records, opened, seen = [], [], set()
    with contextlib.ExitStack() as stack:
        for original in files:
            path = Path(original).resolve(strict=True)
            fs = filesystem(path)
            if fs != 'ext4':
                raise ValueError(f'cold file is not on ext4: {path} ({fs})')
            handle = stack.enter_context(path.open('rb'))
            info = os.fstat(handle.fileno())
            if not stat.S_ISREG(info.st_mode):
                raise ValueError('cold input is not a regular file: ' + str(path))
            inode = (info.st_dev, info.st_ino)
            if inode in seen:
                continue
            seen.add(inode)
            opened.append((path, handle))
        # Validate the whole file set before evicting any of it.
        for path, handle in opened:
            fd = handle.fileno()
            try:
                before = resident_pages(fd)
                os.fsync(fd)
                os.posix_fadvise(fd, 0, 0, os.POSIX_FADV_DONTNEED)
                after = resident_pages(fd)
            except OSError as error:
                raise ValueError(f'cold eviction failed: {path}: {error}') from error
            records.append(dict(path=str(path), filesystem='ext4', before=before, after=after))
        # A later eviction/check must not conceal refaulting of an earlier file.
        for record, (path, handle) in zip(records, opened):
            try:
                record['verified'] = resident_pages(handle.fileno())
            except OSError as error:
                raise ValueError(f'cold residency verification failed: {path}: {error}') from error
        print_record('cache-eviction', cache='cold', files=records)
        resident = [record for record in records if record['after']['resident'] or record['verified']['resident']]
        if resident:
            details = '; '.join(f"{row['path']} (after={row['after']['resident']}, "
                                f"verified={row['verified']['resident']}/{row['verified']['pages']} pages)"
                                for row in resident)
            raise ValueError('cold sample refused: file stays resident: ' + details +
                             '; another process may map it, or mincore access may be restricted')
    return records


def select_rows(exported, balanced=False):
    seen, selected, counts = set(), [], [0, 0, 0, 0]
    quotas = (3000, 3000, 3000, 1000)
    for library, phrase, pinyin, _ in exported:
        if phrase in seen or (balanced and counts[library - 1] == quotas[library - 1]):
            continue
        seen.add(phrase)
        counts[library - 1] += 1
        selected.append((library, phrase, pinyin, 20000))
        if len(selected) == 10000:
            break
    if len(selected) != 10000 or (balanced and tuple(counts) != quotas):
        raise ValueError('export cannot populate recovered fixture recipe')
    # Balanced selection joins the library groups in order, as the recipe does.
    return sorted(selected, key=lambda row: row[0]) if balanced else selected


def modest_rows():
    return [(library, phrase, pinyin, 5) for library in range(1, 5) for _ in range(10)
            for phrase, pinyin in (('测试', "ce4'shi4"), ('测侧', "ce4'ce4"))]


def row_text(rows):
    return ''.join(f'{library}\t{phrase}\t{pinyin}\t{count}\n' for library, phrase, pinyin, count in rows)


def witness_inputs(rows):
    found = []
    for _, _, pronunciation, _ in rows:
        value = re.sub(r'[0-9]', '', pronunciation)
        if value and len(value.encode()) <= 24 and value not in found:
            found.append(value)
            if len(found) == 400:
                break
    return found


class Campaign:
    def __init__(self, worker, data, work, cpu, driver, cache='warm'):
        self.worker, self.data, self.work, self.cpu, self.driver = worker, data.resolve(), work, cpu, driver
        self.serial = 0
        self.cache = cache

    def run(self, facade, library, profile, *, mode='time', save=False, scenario='first-candidate',
            rows=None, instances=1, cycles=1, inputs=None, reopen=False, profiler=None):
        cold = self.cache == 'cold' and mode in ('time', 'memory')
        if cold and (cycles != 1 or profiler):
            raise ValueError('cold samples require one native lifecycle without a profiler')
        self.serial += 1
        scratch = self.work / ('sample-%06d' % self.serial)
        scratch.mkdir()
        user = scratch / 'user'
        shutil.copytree(profile, user)
        expected_source = file_set(profile)
        checkpoints, samples, closed = {}, [], None
        command = ['taskset', '-c', str(self.cpu)] if self.cpu is not None else []
        if profiler:
            command += ['valgrind', '--tool=' + profiler, '--vgdb-prefix=' + str(scratch / 'vgdb')]
            if profiler == 'callgrind':
                command += ['--collect-atstart=no', '--cache-sim=no', '--branch-sim=no',
                            '--read-inline-info=yes', '--callgrind-out-file=/dev/stderr',
                            '--toggle-collect=bisect_teardown_free_unit',
                            '--toggle-collect=bisect_teardown_fini_unit']
            elif profiler == 'dhat':
                command += ['--dhat-out-file=/dev/stderr']
            elif profiler == 'massif':
                command += ['--massif-out-file=/dev/stderr', '--time-unit=i', '--stacks=no',
                            '--detailed-freq=1', '--max-snapshots=1000', '--peak-inaccuracy=0']
            elif profiler == 'memcheck':
                command += ['--leak-check=no', '--show-leak-kinds=all', '--num-callers=24',
                            '--undef-value-errors=no']
            else:
                raise ValueError('unknown profiler')
        command += [str(self.worker), '--teardown', facade, str(library), str(self.data), str(user),
                    mode, str(int(save)), str(instances), str(cycles), scenario, str(rows) if rows else '-']
        env = dict(os.environ, OXPINYIN_TEARDOWN_EXPORTS=','.join(exports(facade)))
        if inputs is not None:
            env['OXPINYIN_TEARDOWN_INPUTS'] = str(inputs)
        errors, documents, boundaries = bytearray(), [], []
        self.driver.check_cancelled()
        try:
            if cold:
                evict_files(cold_files(library, self.data, user))
            self.driver.check_cancelled()
            with subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                  env=env, start_new_session=True) as process:
                def drain():
                    while block := process.stderr.read(65536):
                        errors.extend(block)
                reader = threading.Thread(target=drain)
                reader.start()
                selector = selectors.DefaultSelector()
                selector.register(process.stdout, selectors.EVENT_READ)
                buffered = bytearray()
                deadline = time.monotonic() + (1800 if profiler else 120)
                try:
                    while True:
                        self.driver.check_cancelled()
                        if time.monotonic() >= deadline:
                            raise subprocess.TimeoutExpired(command, deadline)
                        if not selector.select(.1):
                            continue
                        block = os.read(process.stdout.fileno(), 65536)
                        if not block:
                            if buffered:
                                status = process.wait(timeout=10)
                                while reader.is_alive():
                                    self.driver.check_cancelled()
                                    if time.monotonic() >= deadline:
                                        raise subprocess.TimeoutExpired(command, deadline)
                                    reader.join(.1)
                                raise ValueError(f'unterminated native response (exit {status}): '
                                                 f'{bytes(errors)[-3000:]!r}')
                            break
                        buffered.extend(block)
                        while b'\n' in buffered:
                            line, _, rest = buffered.partition(b'\n')
                            buffered = bytearray(rest)
                            if mode == 'export':
                                documents.append(line.decode())
                                continue
                            document = json.loads(line)
                            if document.get('teardown') == 'ready':
                                process.stdin.write(b'\n')
                                process.stdin.flush()
                            elif 'checkpoint' in document:
                                point, cycle = document['checkpoint'], document['cycle']
                                boundaries.append((point, cycle))
                                if point == 'reset':
                                    shutil.rmtree(user)
                                    shutil.copytree(profile, user)
                                elif cycles == 1 or cycle in (0, 1, 10, 50, 100):
                                    memory = read_memory(process.pid, library.resolve(), self.data)
                                    memory['allocator'] = document['allocator']
                                    checkpoints.setdefault(cycle, {})[point] = memory
                                process.stdin.write(b'\n')
                                process.stdin.flush()
                            elif document.get('teardown') == 'sample':
                                samples.append(document)
                            elif document.get('teardown') == 'closed':
                                closed = document
                            else:
                                documents.append(document)
                    status = process.wait(timeout=10)
                    while reader.is_alive():
                        self.driver.check_cancelled()
                        if time.monotonic() >= deadline:
                            raise subprocess.TimeoutExpired(command, deadline)
                        reader.join(.1)
                except BaseException:
                    self.driver.terminate_group(process)
                    raise
                finally:
                    selector.close()
                    process.stdin.close()
                    reader.join()
            self.driver.check_cancelled()
            if status:
                raise ValueError(f'native teardown failed ({status}): {bytes(errors)[-3000:]!r}')
            if file_set(profile) != expected_source:
                raise ValueError('source profile was changed')
            stderr = errors.decode('utf-8', 'surrogateescape').replace(str(user), '$USER')
            observation = dict(samples=[{key: row[key] for key in ('options', 'saved', 'observations')} for row in samples],
                               stderr=stderr, userfiles=profile_contents(self.driver, user), documents=documents)
            if mode not in ('export', 'populate'):
                if len(samples) != cycles or closed is None or closed['status'] != 0:
                    raise ValueError('native lifecycle did not complete')
                if mode == 'memory':
                    expected = [('M0', 0)]
                    for cycle in range(1, cycles + 1):
                        expected.extend((point, cycle) for point in ('M1', 'M2', 'M3'))
                        if cycle < cycles:
                            expected.append(('reset', cycle))
                    if boundaries != expected:
                        raise ValueError('native checkpoint boundaries are out of order')
                    initial = checkpoints[0]['M0']
                    for points in checkpoints.values():
                        if 'M3' in points:
                            if set(points) != {'M1', 'M2', 'M3'}:
                                raise ValueError('native checkpoint boundaries are incomplete')
                            points['M0'] = initial
                for row in samples:
                    frees = [value['call'] for value in row['free_instance_ns']]
                    if len(frees) != instances:
                        raise ValueError('not every instance was freed')
                    for clock in ('wall_ns', 'cpu_ns'):
                        total = sum(value[clock] for value in frees)
                        if total != row['times']['free_all_instances_ns'][clock] or total + row['times']['fini_ns'][clock] != row['times']['teardown_total_ns'][clock]:
                            raise ValueError('native teardown totals are inconsistent')
                samples[-1]['times'].update(closed['times'])
                for row in samples:
                    for index, value in enumerate(row['free_instance_ns']):
                        row['times'][f'free_instance_{index}_ns'] = value['call']
                if cycles == 1:
                    samples[0]['times']['full_lifecycle_ns'] = {
                        clock: sum(samples[0]['times'][name][clock] for name in ('load_ns', 'lifecycle_ns', 'dlclose_ns'))
                        for clock in ('wall_ns', 'cpu_ns', 'majflt')}
            if reopen:
                observation['reopen'] = self.run(facade, library, user, mode='observe', inputs=inputs)['observation']
            return dict(observation=observation, samples=samples, checkpoints=checkpoints, command=command,
                        raw_profile_identity=identity(file_set(user)))
        finally:
            shutil.rmtree(scratch)


def print_record(kind, **values):
    print(json.dumps(dict(teardown=kind, **values), sort_keys=True, ensure_ascii=True), flush=True)


def provision(campaign, pin, root):
    if root.exists():
        profiles = {name: root / name / 'user' for name in PROFILES}
        for user in profiles.values():
            STARTUP_PROFILE.check(user, user.parent / 'profile.manifest')
        return profiles
    root.mkdir()
    export_user = root / 'export-user'
    export_user.mkdir()
    export = campaign.run('pinyin', pin, export_user, mode='export')['observation']['documents']
    exported = []
    for line in export:
        library, phrase, pinyin, count = line.split('\t')
        exported.append((int(library), phrase, pinyin, int(count)))
    shutil.rmtree(export_user)
    recipes = dict(empty=[], modest=modest_rows(), phrases=select_rows(exported), journals=select_rows(exported, True))
    profiles = {}
    for name, rows in recipes.items():
        destination = root / name
        destination.mkdir()
        user = destination / 'user'
        user.mkdir()
        path = destination / 'rows.tsv'
        path.write_text(row_text(rows))
        if rows:
            # Populate through a private working copy, then move its resulting
            # profile into place. run() intentionally never mutates its input.
            populate_user(campaign, pin, user, path)
        files = file_set(user)
        (destination / 'profile.manifest').write_text(STARTUP_PROFILE.manifest(user))
        (destination / 'witness.txt').write_text('')
        profiles[name] = user
        print_record('fixture', profile=name, files=files, identity=identity(files),
                     rows=len(rows), recipe_sha256=digest_bytes(path.read_bytes()), historical_byte_identity=False)
    return profiles


def populate_user(campaign, pin, user, rows):
    # Fixture generation is untimed. This is the same native executable and
    # facade machinery, with no external workload dependency.
    command = [str(campaign.worker), '--teardown', 'pinyin', str(pin), str(campaign.data), str(user),
               'populate', '1', '1', '1', 'first-candidate', str(rows)]
    env = dict(os.environ, OXPINYIN_TEARDOWN_EXPORTS=','.join(exports('pinyin')))
    campaign.driver.check_cancelled()
    with subprocess.Popen(command, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                          text=True, start_new_session=True) as process:
        deadline = time.monotonic() + 300
        try:
            while True:
                campaign.driver.check_cancelled()
                if time.monotonic() >= deadline:
                    raise subprocess.TimeoutExpired(command, 300)
                try:
                    stdout, stderr = process.communicate(timeout=.1)
                    break
                except subprocess.TimeoutExpired:
                    continue
            campaign.driver.check_cancelled()
        except BaseException:
            campaign.driver.terminate_group(process)
            raise
    if process.returncode or json.loads(stdout) != dict(populated=True, saved=True):
        raise ValueError('fixture population failed: ' + stderr)


def run(args, driver, parser):
    if args.per_call or args.cases or args.issue or args.family or args.expect_parent or args.observations or args.plan_only or args.axis_c_replacement or args.axis_c_mutation or args.axis_c_snapshot_check or args.regenerate_sentence_ties:
        parser.error('--teardown is a separate workload; contract/profiler modes cannot be combined')
    cache = getattr(args, 'teardown_cache', 'warm')
    if cache == 'cold':
        if sys.platform != 'linux':
            parser.error('--teardown-cache cold requires native Linux and ext4')
        if args.teardown_stage not in ('verify', 'pilot', 'confirm'):
            parser.error('--teardown-cache cold supports only verify, pilot and confirm stages')
    # contract-diff checks the runtime before dispatching this workload.
    # The provision stage only drives the cell's own pin through the worker, so
    # every cell can write its startup profiles (#1076). The later stages
    # compare final user files through profile_contents, which reads Berkeley
    # DB containers only.
    if args.cell != 'bdb' and args.teardown_stage != 'provision':
        parser.error(f'--teardown-stage {args.teardown_stage} is a BDB workload; '
                     f'the {args.cell} cell supports only --teardown-stage provision')
    if not args.teardown_work_dir or not args.teardown_work_dir.is_dir():
        parser.error('--teardown-work-dir must name an owned temporary directory')
    cases = tuple(args.teardown_cases.split(','))
    if not cases or len(set(cases)) != len(cases) or any(name not in CASES for name in cases):
        parser.error('unknown or repeated --teardown-cases')
    cpu = args.teardown_cpu if args.teardown_cpu is not None else min(os.sched_getaffinity(0))
    if cpu not in os.sched_getaffinity(0):
        parser.error('--teardown-cpu is outside this process affinity')
    work = args.teardown_work_dir.resolve()
    fs = filesystem(work)
    if fs != 'ext4':
        parser.error('teardown work and measured libraries must reside on ext4 (stat reported ' + fs + ')')
    facades = ('pinyin', 'zhuyin') if args.teardown_facades == 'both' else (args.teardown_facades,)
    subjects = {'ox': dict(pinyin=args.pinyin_so, zhuyin=args.zhuyin_so),
                'pin': {facade: args.prefix / 'lib' / ('lib' + facade + '.so') for facade in facades}}
    for paths in subjects.values():
        for facade in facades:
            if paths.get(facade) is None or not paths[facade].is_file():
                parser.error('missing ' + facade + ' library')
    # Copy all measured bytes onto ext4, including the same-bytes control.
    libraries = work / 'libraries'
    libraries.mkdir(exist_ok=True)
    subjects['control'] = {}
    provenance = {}
    for subject in ('ox', 'pin', 'control'):
        directory = libraries / LIBRARY_DIRECTORIES[subject]
        directory.mkdir(exist_ok=True)
        provenance[subject] = {}
        for facade in facades:
            source = subjects['ox'][facade] if subject == 'control' else subjects[subject][facade]
            target = directory / ('lib' + facade + '.so')
            if source.resolve() != target.resolve():
                shutil.copyfile(source, target)
            library_fs = filesystem(target)
            if library_fs != 'ext4':
                parser.error('measured library is not on ext4: ' + str(target) + ' (' + library_fs + ')')
            subjects[subject][facade] = target
            provenance[subject][facade] = dict(path=str(target), sha256=digest_bytes(target.read_bytes()),
                                              bytes=target.stat().st_size, filesystem=library_fs)
    FUNCTION_ORDER['check_arms'](args, {side: {facade: paths[facade] for facade in facades}
                                      for side, paths in subjects.items()}, references=('pin',))
    worker, worker_identity = build_worker(work)
    data = args.prefix / 'lib/libpinyin/data'
    campaign = Campaign(worker, data, work, cpu, driver, cache=cache)
    print_record('start', stage=args.teardown_stage, cache=cache, cpu=cpu, filesystem=fs, libraries=provenance,
                 worker=worker_identity, model_identity=identity(file_set(data)),
                 model_filesystem=filesystem(data), control_noise_rule=list(CONTROL_INTERVAL))
    profiles = provision(campaign, subjects['pin']['pinyin'], work / 'profiles')
    if args.teardown_stage == 'provision':
        return 0
    return execute(campaign, args, subjects, profiles, cases, facades, driver)


def execute(campaign, args, subjects, profiles, cases, facades, driver):
    # zhuyin.cpp:515-526 at 074a2219 uses ZhuyinDirectParser2, not
    # pinyin's romanized importer. Subjects receive identical inputs within
    # each facade, with the same phrase, library and frequency on both.
    dirty_rows = {}
    for facade, reading in (('pinyin', "ce4'shi4"), ('zhuyin', 'ㄘㄜˋ ㄕˋ')):
        dirty_rows[facade] = campaign.work / ('dirty-' + facade + '.tsv')
        dirty_rows[facade].write_text(row_text([(1, '测试', reading, 20000)]))
    witnesses = prepare_witnesses(campaign, subjects['pin']['pinyin'], profiles)
    comparable, references = {}, {}
    for facade in facades:
        for case in cases:
            options = case_options(case, profiles, dirty_rows[facade])
            readings = {}
            for side in ('pin', 'ox', 'control'):
                readings[side] = campaign.run(facade, subjects[side][facade], mode='observe',
                                              inputs=witnesses[case.split('-')[0]], reopen=True, **options)
            reference = readings['pin']['observation']
            matches = all(row['observation'] == reference for row in readings.values())
            comparable[facade, case] = matches
            references[facade, case] = {side: workload_observation(row) for side, row in readings.items()}
            print_record('equivalence', facade=facade, case=case,
                         verdict='COMPARABLE' if matches else 'NOT COMPARABLE',
                         digests={side: identity(row['observation']) for side, row in readings.items()},
                         raw_final_profile_identities={side: row['raw_profile_identity'] for side, row in readings.items()},
                         differences={} if matches else {side: first_difference(reference, row['observation'])
                                                        for side, row in readings.items() if row['observation'] != reference})
    # Report a safe checkpoint before starting any ratio campaign. A mismatch
    # is an observation to report, never permission to repair shipping behavior.
    if not all(comparable.values()):
        print_record('checkpoint', status='equivalence-blocked', comparable=[list(key) for key, value in comparable.items() if value])
        return 2
    if args.teardown_stage == 'verify':
        return 0
    if args.teardown_stage in ('pilot', 'confirm'):
        pilot = args.teardown_stage == 'pilot'
        for facade in facades:
            for case in cases:
                options = case_options(case, profiles, dirty_rows[facade])
                for mode, rounds, warmups in (('time', 5 if pilot else TIMING_ROUNDS, 1 if pilot else TIMING_WARMUPS),
                                              ('memory', 3 if pilot else MEMORY_ROUNDS, 1 if pilot else MEMORY_WARMUPS)):
                    samples = {side: [] for side in subjects}
                    for index in range(rounds + warmups):
                        order = ['ox', 'control', 'pin']
                        order = order[index % 3:] + order[:index % 3]
                        for side in order:
                            row = campaign.run(facade, subjects[side][facade], mode=mode, **options)
                            if workload_observation(row) != references[facade, case][side]:
                                raise ValueError(f'timed workload observation changed: {facade} {case} {side}')
                            if index >= warmups:
                                samples[side].append(row)
                    if mode == 'time':
                        report_times(facade, case, samples, pilot)
                    else:
                        report_memory(facade, case, samples, pilot)
        return 0
    if args.teardown_stage in ('repeat', 'multi'):
        populated = args.teardown_attribution_case.split('-')[0]
        if populated == 'empty':
            populated = 'journals'
        for facade in facades:
            for case in ('empty', populated):
                options = case_options(case, profiles, dirty_rows[facade])
                for side in ('ox', 'control', 'pin'):
                    repeated = args.teardown_stage == 'repeat'
                    row = campaign.run(facade, subjects[side][facade], mode='memory', cycles=100 if repeated else 1,
                                       instances=1 if repeated else 2,
                                       **dict(options, scenario='first-candidate' if repeated else 'multi'))
                    points = {str(cycle): value for cycle, value in row['checkpoints'].items() if cycle}
                    print_record(args.teardown_stage, facade=facade, case=case, side=side, checkpoints=points,
                                 observations_equal=all(value == row['observation']['samples'][0] for value in row['observation']['samples']))
        return 0
    if args.teardown_stage == 'attribute':
        diagnostic_workers = {tool: build_worker(campaign.work, tool) for tool in ('massif', 'memcheck')}
        for tool, (_, worker_identity) in diagnostic_workers.items():
            print_record('diagnostic-worker', tool=tool, profiler_only=True, worker=worker_identity)
        plain_worker = campaign.worker
        invalid = False
        for facade in facades:
            options = case_options(args.teardown_attribution_case, profiles, dirty_rows[facade])
            shown, heaps = {}, {}
            for side in ('ox', 'control', 'pin'):
                row = campaign.run(facade, subjects[side][facade], profiler='callgrind', **options)
                require_profile_controls(row, references[facade, args.teardown_attribution_case][side])
                profile = driver.per_call_profile(row['observation']['stderr'])
                shown[side] = dict(driver.per_call_attribution(profile, 1, 25), scoped_ir=profile['total'])
                campaign.worker = diagnostic_workers['massif'][0]
                try:
                    heap = campaign.run(facade, subjects[side][facade], mode='memory', profiler='massif', **options)
                finally:
                    campaign.worker = plain_worker
                require_profile_controls(heap, references[facade, args.teardown_attribution_case][side])
                heaps[side] = massif_profile(heap['observation']['stderr'])
            invalid |= report_exact('callgrind', facade, args.teardown_attribution_case, shown, callgrind_counts)
            invalid |= report_exact('massif', facade, args.teardown_attribution_case, heaps, massif_counts)
        # The native repeated-lifecycle counters can include glibc tcache and
        # arena retention. Memcheck's live-block accounting distinguishes those
        # from unfreed client allocations, including native libdb/GLib blocks.
        populated = args.teardown_attribution_case.split('-')[0]
        for facade in facades:
            for case in dict.fromkeys(('empty', populated)):
                if (facade, case) not in references:
                    raise ValueError('heap repetition needs an equivalence-verified case')
                options = case_options(case, profiles, dirty_rows[facade])
                live = {}
                for side in ('ox', 'control', 'pin'):
                    campaign.worker = diagnostic_workers['memcheck'][0]
                    try:
                        heap = campaign.run(facade, subjects[side][facade], mode='memory',
                                            cycles=100, profiler='memcheck', **options)
                    finally:
                        campaign.worker = plain_worker
                    require_profile_controls(heap, references[facade, case][side])
                    points = [point for point in heap['observation']['documents'] if point.get('heap_live')]
                    expected = [('M0', 0), ('M1', 1), ('M2', 1), ('M3', 1),
                                ('M3', 10), ('M3', 50), ('M3', 100)]
                    if [(point['point'], point['cycle']) for point in points] != expected:
                        raise ValueError('incomplete live-heap checkpoint sequence')
                    live[side] = dict(points=points, sites=memcheck_sites(heap['observation']['stderr'], 'M3', 100))
                invalid |= report_exact('live-heap', facade, case, live, live_heap_counts)
        # Every record prints first; an INVALID exact control fails the stage.
        return 1 if invalid else 0
    raise ValueError('unknown teardown stage')


def case_options(case, profiles, dirty_rows):
    return dict(profile=profiles[case.split('-')[0]], scenario='dirty' if '-' in case else 'first-candidate',
                rows=dirty_rows if '-' in case else None, save=case.endswith('-saved'))


def require_profile_controls(row, reference):
    if (any(sample != reference['samples'][0] for sample in row['observation']['samples']) or
            row['observation']['userfiles'] != reference['userfiles']):
        raise ValueError('NOT COMPARABLE: instrumented workload observation changed')


def massif_snapshots(text):
    snapshots = []
    for block in re.split(r'^snapshot=\d+\s*$', text, flags=re.M)[1:]:
        values = {name: int(value) for name, value in re.findall(
            r'^(time|mem_heap_B|mem_heap_extra_B|mem_stacks_B)=(\d+)\s*$', block, re.M)}
        if set(values) != {'time', 'mem_heap_B', 'mem_heap_extra_B', 'mem_stacks_B'}:
            raise ValueError('incomplete Massif heap snapshot')
        values['tree'] = [line for line in block.splitlines() if re.match(r'^\s*n\d+:', line)][:20]
        snapshots.append(values)
    return snapshots


def massif_profile(text):
    checkpoints = {}
    for point, block in re.findall(r'TEARDOWN_HEAP (M[0-3]) BEGIN\n(.*?)TEARDOWN_HEAP \1 END', text, re.S):
        values = massif_snapshots(block)
        if point in checkpoints or len(values) != 1:
            raise ValueError('invalid Massif checkpoint')
        checkpoints[point] = values[0]
    if set(checkpoints) != {'M0', 'M1', 'M2', 'M3'}:
        raise ValueError('missing Massif teardown checkpoints')
    snapshots = massif_snapshots(text)
    interval = [value for value in snapshots
                if checkpoints['M1']['time'] <= value['time'] <= checkpoints['M3']['time']]
    return dict(checkpoints=checkpoints, sampled_teardown_peak=max(interval, key=lambda value: value['mem_heap_B']),
                whole_lifecycle_peak=max(snapshots, key=lambda value: value['mem_heap_B']),
                interval_snapshots=len(interval), peak_basis='sampled snapshots plus exact ABI-boundary snapshots; not an exact transient-peak claim',
                freed_heap_bytes={label: checkpoints[a]['mem_heap_B'] - checkpoints[b]['mem_heap_B']
                                  for label, a, b in (('M1-M2', 'M1', 'M2'), ('M2-M3', 'M2', 'M3'))})


def memcheck_sites(text, point, cycle):
    marker = f'TEARDOWN_LEAK {point} {cycle}'
    matches = re.findall(re.escape(marker + ' BEGIN\n') + r'(.*?)' + re.escape(marker + ' END'), text, re.S)
    if len(matches) != 1:
        raise ValueError('missing Memcheck live-heap report')
    plain = re.sub(r'^==\d+== ?', '', matches[0], flags=re.M)
    sites = []
    pattern = r'^([\d,]+)(?: \([^\n]*?\))? bytes in ([\d,]+) blocks are ([^\n]+?) in loss record[^\n]*\n((?:(?: +(?:at|by) [^\n]*|[^\S\n]*)\n)*)'
    for size, blocks, kind, stack in re.findall(pattern, plain, re.M):
        sites.append(dict(bytes=int(size.replace(',', '')), blocks=int(blocks.replace(',', '')),
                          kind=kind, frames=[line.strip() for line in stack.splitlines() if line.strip()]))
    if not sites:
        raise ValueError('missing Memcheck allocation sites')
    # Loss records with direct+indirect bytes can overlap. They are named
    # roots, not an additive partition; authoritative totals are the requests.
    return sorted(sites, key=lambda value: (-value['bytes'], value['kind']))[:20]


def workload_observation(row):
    return {key: row['observation'][key] for key in ('samples', 'stderr', 'userfiles')}


def first_difference(left, right, path='$'):
    if type(left) is not type(right):
        return dict(path=path, pin=left, subject=right)
    if isinstance(left, dict):
        if left.keys() != right.keys():
            return dict(path=path, pin_keys=sorted(left), subject_keys=sorted(right))
        for key in left:
            if left[key] != right[key]:
                return first_difference(left[key], right[key], path + '.' + str(key))
    elif isinstance(left, (list, tuple)):
        for index, (a, b) in enumerate(zip(left, right)):
            if a != b:
                return first_difference(a, b, path + '[' + str(index) + ']')
        if len(left) != len(right):
            return dict(path=path + '.length', pin=len(left), subject=len(right))
    elif left != right:
        return dict(path=path, pin=left, subject=right)
    return None


def prepare_witnesses(campaign, pin, profiles):
    result = {}
    for name in PROFILES:
        path = profiles[name].parent / 'witness.txt'
        if name == 'empty':
            result[name] = path
            continue
        if not path.read_text():
            rows = []
            for line in (path.parent / 'rows.tsv').read_text().splitlines():
                library, phrase, pronunciation, count = line.split('\t')
                rows.append((int(library), phrase, pronunciation, int(count)))
            candidates = witness_inputs(rows)
            candidate_file = path.parent / 'candidate-inputs.txt'
            candidate_file.write_text(''.join(value + '\n' for value in candidates))
            empty = campaign.run('pinyin', pin, profiles['empty'], mode='observe', inputs=candidate_file)
            populated = campaign.run('pinyin', pin, profiles[name], mode='observe', inputs=candidate_file)
            def by_input(row):
                return {value['transcript']: value for value in row['observation']['documents']
                        if value.get('parser') == 'full'}
            a, b = by_input(empty), by_input(populated)
            chosen = [value for value in sorted(candidates) if a.get(value) != b.get(value)][:40]
            if not chosen:
                raise ValueError('populated profile changes no witness answer: ' + name)
            path.write_text(''.join(value + '\n' for value in chosen))
        result[name] = path
        print_record('witness', profile=name, inputs=path.read_text().splitlines(), sha256=digest_bytes(path.read_bytes()))
    return result


def report_times(facade, case, samples, pilot):
    for metric in samples['ox'][0]['samples'][0]['times']:
        for clock in ('wall_ns', 'cpu_ns'):
            values = {side: [row['samples'][0]['times'][metric][clock] for row in rows]
                      for side, rows in samples.items()}
            control = stable_control(values['ox'], values['control'])
            print_record('time', facade=facade, case=case, metric=metric, clock=clock, pilot=pilot,
                         summaries={side: summary(value) for side, value in values.items()}, control=control,
                         verdict='STABLE' if control['stable'] else 'INCONCLUSIVE',
                         ox_pin_ratio=paired_ratio(values['ox'], values['pin']) if control['stable'] else None,
                         control_pin_ratio=paired_ratio(values['control'], values['pin']) if control['stable'] else None)
        # Major faults are event counts, not durations: no control rule and
        # no ratio, which a zero count could not carry. Reported as observed.
        counts = {side: [row['samples'][0]['times'][metric]['majflt'] for row in rows]
                  for side, rows in samples.items()}
        print_record('faults', facade=facade, case=case, metric=metric, counter='majflt', pilot=pilot,
                     summaries={side: summary(value) for side, value in counts.items()})


def report_memory(facade, case, samples, pilot):
    # Absolute checkpoint levels have meaningful positive-denominator ratios.
    # Signed release/residual deltas are reported in KiB, never divided by a
    # negative/zero baseline. No VmHWM observation enters this path.
    for point in ('M0', 'M1', 'M2', 'M3'):
        for metric in FIELDS:
            values = {side: [row['checkpoints'][0 if point == 'M0' else 1][point]['totals'][metric] for row in rows]
                      for side, rows in samples.items()}
            control_differences = [b - a for a, b in zip(values['ox'], values['control'])]
            control = stable_residency(values['ox'], values['control'])
            print_record('memory', facade=facade, case=case, point=point, metric=metric, unit='KiB', pilot=pilot,
                         summaries={side: summary(value) for side, value in values.items()},
                         control_paired_difference=summary(control_differences),
                         control=control, verdict='STABLE' if control['stable'] else 'INCONCLUSIVE',
                         ox_pin_ratio=paired_ratio(values['ox'], values['pin']) if control['stable'] and all(value > 0 for value in values['pin']) else None)
    for side, rows in samples.items():
        deltas = [memory_deltas(dict(row['checkpoints'][1], M0=row['checkpoints'][0]['M0'])) for row in rows]
        shown = {label: {metric: summary([delta[label]['totals'][metric] for delta in deltas]) for metric in FIELDS}
                 for label in ('M1-M2', 'M2-M3', 'M3-M0')}
        checkpoints = {}
        for point in ('M0', 'M1', 'M2', 'M3'):
            observations = [row['checkpoints'][0 if point == 'M0' else 1][point] for row in rows]
            groups = set().union(*(observation['groups'] for observation in observations))
            checkpoints[point] = dict(
                fds=summary([observation['fds'] for observation in observations]),
                mappings=summary([observation['mappings'] for observation in observations]),
                groups={group: {metric: summary([observation['groups'].get(group, {}).get(metric, 0) for observation in observations])
                                for metric in FIELDS} for group in sorted(groups)},
                allocator={key: summary([observation['allocator'][key] for observation in observations])
                           for key in observations[0]['allocator']} if all(observation['allocator'] is not None for observation in observations) else None,
                rollup_minus_smaps={key: summary([observation['rollup_minus_smaps'][key] for observation in observations])
                                    for key in FIELDS})
        print_record('memory-deltas', facade=facade, case=case, side=side, pilot=pilot, signed_kib=shown,
                     checkpoints=checkpoints)


def callgrind_counts(shown):
    return dict(scoped_ir=shown['scoped_ir'])


def massif_counts(heap):
    # The ABI-boundary snapshots are exact; the sampled peaks are not
    # deterministic accounting and stay out of the comparison.
    return {point + '.mem_heap_B': value['mem_heap_B'] for point, value in heap['checkpoints'].items()}


def live_heap_counts(live):
    # Memcheck's four leak classes partition the live blocks.
    counts = {}
    for point in live['points']:
        label = f"{point['point']}.{point['cycle']}"
        for unit in ('bytes', 'blocks'):
            counts[label + '.live_' + unit] = sum(point[kind + '_' + unit] for kind in
                                                  ('lost', 'possible', 'reachable', 'suppressed'))
    return counts


def report_exact(kind, facade, case, rows, counts):
    """Print one exact control record per count, then the profiler_only rows.
    An INVALID count withholds the ox and pin rows (§7.5 item 3). Returns
    whether any count was INVALID."""
    ox, control = counts(rows['ox']), counts(rows['control'])
    if ox.keys() != control.keys():
        raise ValueError(kind + ' control counts do not name the same checkpoints')
    invalid = False
    for name in ox:
        result = exact_control(ox[name], control[name])
        invalid |= result['verdict'] == 'INVALID'
        print_record(kind + '-control', facade=facade, case=case, count=name, **result)
    for side in ('control',) if invalid else ('ox', 'control', 'pin'):
        print_record(kind, facade=facade, case=case, side=side, profiler_only=True, **rows[side])
    return invalid
