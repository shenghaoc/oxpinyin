"""Teardown workload for contract-diff.py and bisect.c. Standard library only.

The controller never runs in the measured native child. Observations, smaps and
profiler output remain in memory. Only private fixture inputs/manifests and the
native executable are materialized, under the caller's temporary work directory.
"""
import bisect
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
# The worker's per-phase getrusage/clock fields (teardown.h, td_print_time).
CLOCK_FIELDS = ('wall_ns', 'cpu_ns', 'user_ns', 'sys_ns', 'minflt', 'majflt', 'nvcsw', 'nivcsw')
# init-attribute (#1090): diagnostic samples per probe as (rounds, warm-ups),
# in the rotating order. None of them is a primary timing sample.
INIT_PROBE_ROUNDS = {'count': (30, 1), 'sample': (30, 1), 'sample-coarse': (60, 1), 'faults': (10, 0),
                     'trace': (6, 0), 'pages': (1, 0)}
INIT_IR_ROUNDS = 3
# cpu-clock periods. 50 kHz stays below the kernel's default 100 kHz sampling
# limit; each sample's handler is charged to the task, so the coarse period
# checks that the fine one's perturbation does not change the shares.
INIT_SAMPLE_PERIOD_NS = {'sample': 20000, 'sample-coarse': 200000}
# Fixed simulated caches, so the diagnostic does not depend on the host CPUID.
INIT_CACHE_SIM = ('--I1=32768,8,64', '--D1=32768,8,64', '--LL=8388608,16,64')


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


def profile_contents(driver, directory, *, library=None, cell=None):
    """Reuse contract-diff's DB row/padding rules, retaining every file/row.

    BDB page headers include random file identities and LSNs; Bigram::save_db
    recreates the container (src/storage/ngram_bdb.cpp:81-105 at 074a2219).
    KC user containers are snapshots, tkrzw indexes are TreeDBM and the
    user bigram is HashDBM. Read peer records in the existing isolated worker
    so backend CDLLs do not stay mapped in the controller. Raw input identities
    stay separate. The registered padding/timestamp rules still apply.
    """
    cell = cell or os.environ.get('CONTRACT_DIFF_CELL', getattr(driver, 'CELL', 'bdb'))
    if cell not in ('bdb', 'kc', 'tkrzw'):
        raise ValueError('unknown profile cell: ' + cell)
    result = {}
    for path in sorted(Path(directory).rglob('*')):
        if not path.is_file():
            continue
        raw = path.read_bytes()
        name = str(path.relative_to(directory))
        peer_db = cell != 'bdb' and name in (*driver.USER_INDEX_FILES, 'user_bigram.db')
        bdb_db = cell == 'bdb' and raw[12:16] in (b'\x62\x31\x05\x00', b'\x61\x15\x06\x00')
        if peer_db or bdb_db:
            if peer_db:
                if library is None:
                    raise ValueError('profile reader requires the measured library for runtime certification')
                kind = 'hash' if name == 'user_bigram.db' else 'tree'
                # Reuse the driver's cancellation-safe worker. Native readers
                # exit before the next measured launch (also in cold mode).
                row = driver.run_worker('profile-rows', library, path, kind, Path(directory).parent,
                                        extra_env=dict(CONTRACT_DIFF_CELL=cell), timeout=120)
                if row['exit'] or row['stderr_lines'] or not isinstance(row['result'], dict) or set(row['result']) != {'rows'}:
                    raise ValueError(f'{cell} profile reader failed: {path}: exit={row["exit"]}, '
                                     f'stderr={row.get("stderr", "")[-500:]}')
                rows = row['result']['rows']
                if not isinstance(rows, list) or any(not isinstance(pair, list) or len(pair) != 2 or
                        any(not isinstance(value, str) or len(value) % 2 or
                            re.fullmatch('[0-9a-f]*', value) is None for value in pair) for pair in rows):
                    raise ValueError('invalid profile row document: ' + str(path))
            else:
                rows = driver.index_file_rows(path)
            if path.name == 'user_pinyin_index.bin':
                rows = [[key, driver.zero_item_padding(key, value)] for key, value in rows]
            result[name] = dict(format=cell + '-rows', rows=rows)
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


# glibc, its dynamic loader and libgcc_s remain warm. libgcc_s joins the C
# runtime because the controller's own interpreter maps it (maintainer ruling
# on #1091): it cannot leave the page cache, for either side. The C++ runtime
# and all other DT_NEEDED dependencies (including transitive ones) belong to
# the cold set.
C_RUNTIME = re.compile(r'^(?:lib(?:c|m|dl|pthread|rt|resolv|util|anl)\.so\.[0-9]+|ld-linux[^/]*\.so\.[0-9]+'
                       r'|libgcc_s\.so\.[0-9]+)$')


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


def cold_files(library, data, user, rows=None):
    # Use the worker's private, restored user tree, never its source profile.
    # Include every regular data/profile file, not a hard-coded stock filename
    # list: table.conf and non-stock library layouts are valid inputs too.
    # Dirty scenarios also read their rows file inside the lifecycle.
    return [Path(library), *library_dependencies(library),
            *(path for root in (data, user) for path in sorted(root.rglob('*')) if path.is_file()),
            *([Path(rows)] if rows else [])]


def file_residency(files, library, data, user):
    """Resident pages of each cold file, keyed by a sample-independent label.

    Read at a paused checkpoint of a cold memory sample; resident_pages reads
    no file bytes, so the reading does not fault the pages it counts.
    """
    rows = {}
    for path in map(Path, files):
        if path == Path(library):
            label = 'facade'
        elif path.is_relative_to(data):
            label = 'data/' + path.relative_to(data).as_posix()
        elif path.is_relative_to(user):
            label = 'user/' + path.relative_to(user).as_posix()
        else:
            label = path.name
        with path.open('rb') as handle:
            rows[label] = resident_pages(handle.fileno())
    return rows


def resident_pages(fd, ranges=False):
    """Linux mincore over an unfaulted PROT_NONE mapping; no file bytes read.
    With `ranges`, also the resident page indices as [start, end) runs.

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
        result = dict(pages=pages, resident=sum(value & 1 for value in vector))
        if ranges:
            result['ranges'] = page_ranges(index for index, value in enumerate(vector) if value & 1)
        return result
    finally:
        if libc.munmap(address, size):
            raise OSError(ctypes.get_errno(), 'cold mincore munmap failed')


def page_ranges(indices):
    """Sorted page indices as [start, end) runs."""
    runs = []
    for index in sorted(indices):
        if runs and runs[-1][1] == index:
            runs[-1][1] = index + 1
        else:
            runs.append([index, index + 1])
    return runs


def range_pages(runs):
    return {index for start, end in runs for index in range(start, end)}


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


def file_role(path, library, data, user):
    """Name a measured file by its role, not its per-sample private path."""
    path = Path(path)
    if path == Path(library):
        return 'SUBJECT'
    for root, role in ((user, 'USER'), (data, 'DATA')):
        if path.is_relative_to(root):
            return role + ':' + str(path.relative_to(root))
    return 'LIB:' + path.name


def residency(files, library, data, user):
    """Page-cache residency of each measured file (mincore); reads no bytes."""
    result = {}
    for path in files:
        with Path(path).open('rb') as handle:
            role = file_role(path, library, data, user)
            # The facade's own page bitmap names which of its pages init reads.
            result[role] = resident_pages(handle.fileno(), ranges=role == 'SUBJECT')
    return result


def process_maps(pid, *, proc_root=Path('/proc')):
    """Mappings of a paused child: (start, end, perms, path, file offset)."""
    rows = []
    for line in (proc_root / str(pid) / 'maps').read_text().splitlines():
        fields = line.split(maxsplit=5)
        if len(fields) >= 5:
            start, end = (int(value, 16) for value in fields[0].split('-'))
            rows.append((start, end, fields[1], fields[5] if len(fields) > 5 else '', int(fields[2], 16)))
    return rows


STRACE_TIME = re.compile(r'\s<(\d+\.\d+)>$')
STRACE_CALL = re.compile(r'^(\d+)\s+(?:<\.\.\. (\w+) resumed>\s?|(\w+)\()(.*)$')
# Calls whose first string argument is a path (after an optional dirfd).
PATH_CALLS = frozenset(('open', 'openat', 'openat2', 'stat', 'lstat', 'newfstatat', 'statx', 'access',
                        'faccessat', 'faccessat2', 'readlink', 'readlinkat', 'mkdir', 'mkdirat',
                        'unlink', 'unlinkat', 'rename', 'renameat', 'renameat2', 'execve'))


def init_trace(text, marker, names):
    """Aggregate an in-memory `strace -f -y -T` trace between the two marker
    writes. The markers themselves are excluded. Per-call times are the
    tracer's (ptrace stops included), so they bound, not measure, kernel time.
    """
    def name_paths(value):
        for original, role in sorted(names.items(), key=lambda item: -len(item[0])):
            if value == original or value.startswith(original + '/'):
                return role + value[len(original):]
        return value
    calls, targets, unparsed, unfinished, pids, pending = {}, {}, 0, 0, set(), {}
    state = 'before'
    for line in text.splitlines():
        match = STRACE_CALL.match(line)
        if not match:
            unparsed += state == 'inside'
            continue
        name, rest = match[2] or match[3], match[4]
        if rest.endswith('<unfinished ...>'):
            unfinished += state == 'inside'
            # A call belongs to the window it entered in, wherever another
            # thread's markers fall before strace prints its result.
            pending[match[1], name] = state, rest.removesuffix('<unfinished ...>').rstrip()
            continue
        entered = state
        if match[2]:
            entered, begun = pending.pop((match[1], name), (state, ''))
            rest = begun + rest
        is_marker = name == 'write' and not match[2] and re.match(str(marker) + r'[<,]', rest)
        if is_marker:
            state = {'before': 'inside', 'inside': 'after'}.get(state, 'extra')
            continue
        if entered != 'inside':
            continue
        timed = STRACE_TIME.search(rest)
        arguments, separator, value = (rest[:timed.start()] if timed else rest).rpartition(' = ')
        if not separator:
            unparsed += 1
            continue
        pids.add(match[1])
        error = re.match(r'-1 (E[A-Z0-9]+)', value)
        call = calls.setdefault(name, dict(count=0, errors=0, seconds=0.0))
        call['count'] += 1
        call['errors'] += bool(error)
        call['seconds'] += float(timed[1]) if timed else 0.0
        # -y annotates descriptors with their paths: `3</path>`.
        descriptor = re.match(r'-?\d+<([^>]*)>', arguments)
        quoted = re.match(r'(?:AT_FDCWD(?:<[^>]*>)?, |-?\d+<[^>]*>, )?"([^"]*)"', arguments)
        if name == 'mmap':
            mapped = re.match(r'(?:NULL|0x[0-9a-f]+), \d+, [^,]+, [^,]+, -?\d+(?:<([^>]*)>)?', arguments)
            target = name_paths(mapped[1]) if mapped and mapped[1] else 'anonymous'
        elif name in ('munmap', 'mprotect', 'madvise', 'brk', 'mremap'):
            target = 'address-range'
        elif name in PATH_CALLS and quoted and quoted[1]:
            target = name_paths(quoted[1])
        elif descriptor:
            target = name_paths(descriptor[1])
        else:
            target = '-'
        entry = targets.setdefault(target, {}).setdefault(name, dict(count=0, bytes=0))
        entry['count'] += 1
        if name in ('read', 'pread64') and not error and value.split('<')[0].isdigit():
            entry['bytes'] += int(value.split('<')[0])
        elif name in ('mmap', 'munmap') and not error:
            length = re.match(r'(?:NULL|0x[0-9a-f]+), (\d+)', arguments)
            entry['bytes'] += int(length[1]) if length else 0
    return dict(window=state == 'after', calls=calls, targets=targets, unparsed=unparsed,
                unfinished=unfinished, threads=len(pids),
                total=dict(count=sum(value['count'] for value in calls.values()),
                           seconds=sum(value['seconds'] for value in calls.values())))


def kernel_symbols(path=Path('/proc/kallsyms')):
    """Text symbols, sorted; None when the kernel hides addresses (kptr_restrict)."""
    try:
        lines = path.read_text().splitlines()
    except OSError:
        return None
    rows = sorted((int(fields[0], 16), fields[2]) for fields in (line.split() for line in lines)
                  if len(fields) >= 3 and fields[1] in ('t', 'T', 'w', 'W'))
    if not rows or not rows[-1][0]:
        return None
    return [row[0] for row in rows], [row[1] for row in rows]


def kernel_symbol(symbols, address):
    index = bisect.bisect_right(symbols[0], address) - 1
    return symbols[1][index] if index >= 0 else hex(address)


PAGE_FAULT_FRAMES = frozenset(('asm_exc_page_fault', 'exc_page_fault', 'do_user_addr_fault', 'handle_mm_fault'))


def kernel_category(sample, symbols):
    """Innermost decisive frame: a fault inside a syscall counts as the fault."""
    if symbols is None:
        return 'unsymbolized', hex(sample['ip'])
    frames = [kernel_symbol(symbols, address) for address in (sample['ip'], *sample['chain'])]
    for frame in frames:
        match = re.fullmatch(r'__(?:x64|ia32|do|se)_sys_(\w+)', frame)
        if match:
            return 'syscall:' + match[1], frames[0]
        if frame in PAGE_FAULT_FRAMES:
            return 'page-fault', frames[0]
    if any(frame in ('do_syscall_64', 'entry_SYSCALL_64', 'entry_SYSCALL_64_after_hwframe') for frame in frames):
        return 'syscall:entry-exit', frames[0]
    if any(frame.startswith(('asm_sysvec_', 'sysvec_', 'asm_common_interrupt', 'common_interrupt')) for frame in frames):
        return 'interrupt', frames[0]
    return 'other', frames[0]


def user_object(address, maps, library, worker):
    for start, end, perms, path, *_ in maps:
        if 'x' in perms and start <= address < end:
            if path == str(library):
                return 'SUBJECT'
            if path == str(worker):
                return 'WORKER'
            return Path(path).name if path.startswith('/') else path
    return 'unmapped'


ELF_SYMBOLS = {}


def elf_symbols(path):
    """Sorted (address, name) function symbols of one object, from nm: the
    symbol table when present, else the dynamic one. Cached per path."""
    if path not in ELF_SYMBOLS:
        rows = []
        for dynamic in ([], ['--dynamic']):
            try:
                output = subprocess.run(['nm', '--defined-only', '--demangle', *dynamic, path],
                                        capture_output=True, text=True, check=True).stdout
            except (OSError, subprocess.CalledProcessError):
                continue
            rows = sorted((int(fields[0], 16), fields[2]) for fields in
                          (line.split(' ', 2) for line in output.splitlines())
                          if len(fields) == 3 and fields[1] in ('t', 'T', 'w', 'W', 'i'))
            if rows:
                break
        ELF_SYMBOLS[path] = ([row[0] for row in rows], [row[1] for row in rows])
    return ELF_SYMBOLS[path]


def user_function(address, maps):
    """The function holding a user IP. A shared object's lowest mapping is
    its load base: its segments map in address order from file offset 0."""
    for start, end, perms, path, *_ in maps:
        if 'x' in perms and start <= address < end:
            if not path.startswith('/'):
                return path
            base = min(row[0] for row in maps if row[3] == path)
            addresses, names = elf_symbols(path)
            index = bisect.bisect_right(addresses, address - base) - 1
            return names[index] if index >= 0 else hex(address - base)
    return 'unmapped'


def region_label(path, access, library, data, user):
    """A faulted mapping by role and access class, as mapping_group names it."""
    path = path.removesuffix(' (deleted)')
    if path == str(library):
        return 'SUBJECT:' + access
    for root, role in ((user, 'USER'), (data, 'DATA')):
        if path.startswith(str(root) + '/'):
            return role + ':' + path[len(str(root)) + 1:] + ':' + access
    if not path or path == '//anon':
        return 'ANON:' + access
    if path.startswith('['):
        return path
    return Path(path).name + ':' + access


def init_faults(records, before, library, data, user):
    """Attribute each fault to the mapping that held its address when it
    happened: M0's mappings, then the init's own mmaps, latest first."""
    def access(text=None, prot=0):
        executable, writable = ('x' in text, 'w' in text) if text is not None else (prot & 4, prot & 2)
        return 'text' if executable else 'writable' if writable else 'readonly'
    regions = [(start, end, region_label(path, access(perms), library, data, user))
               for start, end, perms, path, *_ in reversed(before)]
    faults = []
    for record in records:
        if 'mmap' in record:
            regions.insert(0, (record['mmap'], record['mmap'] + record['len'],
                               region_label(record['file'], access(prot=record['prot']), library, data, user)))
            continue
        label = next((name for start, end, name in regions if start <= record['addr'] < end), 'unmapped')
        faults.append(dict(record, region=label))
    return faults


LACKEY_LINE = re.compile(rb'^(I | [LSM]) +([0-9a-f]+),([0-9]+)$', re.M)


class LackeyAccesses:
    """Lackey `--trace-mem=yes` output, read from a pipe as it streams.

    The worker's marker bytes `B` and `E` land at line starts of the same pipe
    (each Lackey line is one write). Only accesses between them are kept:
    distinct instruction addresses and distinct 64-byte data lines."""

    def __init__(self):
        self.state, self.rest, self.lines = 'before', b'', 0
        self.code, self.data = set(), set()

    def feed(self, block):
        text = self.rest + block
        cut = text.rfind(b'\n') + 1
        self.rest, text = text[cut:], text[:cut]
        if self.state == 'before':
            begin = re.search(rb'(?m)^B', text)
            if not begin:
                return
            self.state, text = 'inside', text[begin.end():]
        if self.state != 'inside':
            return
        end = re.search(rb'(?m)^E', text)
        for kind, address, size in LACKEY_LINE.findall(text[:end.start()] if end else text):
            start = int(address, 16)
            if kind == b'I ':
                self.code.add(start)
            else:
                self.data.update(range(start >> 6, ((start + int(size) - 1) >> 6) + 1))
            self.lines += 1
        if end:
            self.state = 'after'

    def result(self):
        return dict(window=self.state == 'after', lines=self.lines, code=sorted(self.code), data_lines=sorted(self.data))


ELF_LAYOUTS = {}


def library_layout(path):
    """Allocated file-backed sections and sized symbols of one ELF object."""
    path = str(path)
    if path not in ELF_LAYOUTS:
        sections = []
        for line in subprocess.check_output(['readelf', '-SW', path], text=True).splitlines():
            match = re.match(r'\s*\[\s*\d+\]\s+(\S+)\s+(\S+)\s+([0-9a-f]+)\s+([0-9a-f]+)\s+([0-9a-f]+)\s+\S+\s+([A-Z]*)\s', line)
            if match and 'A' in match[6] and match[2] != 'NOBITS':
                sections.append(dict(name=match[1], address=int(match[3], 16), offset=int(match[4], 16),
                                     size=int(match[5], 16)))
        symbols = []
        output = subprocess.check_output(['nm', '-S', '--defined-only', '--demangle', path], text=True)
        for line in output.splitlines():
            fields = line.split(' ', 3)
            if len(fields) == 4 and re.fullmatch('[0-9a-f]+', fields[1]) and int(fields[1], 16):
                symbols.append((int(fields[0], 16), int(fields[1], 16), fields[2], fields[3]))
        ELF_LAYOUTS[path] = dict(sections=sections, symbols=sorted(symbols))
    return ELF_LAYOUTS[path]


def section_at(layout, offset):
    return next((section['name'] for section in layout['sections']
                 if section['offset'] <= offset < section['offset'] + section['size']), None)


def page_section(layout, page, page_size):
    """The section holding most bytes of a file page of `page_size` bytes;
    page headers and padding outside every section are 'none'."""
    overlap = {section['name']: min(section['offset'] + section['size'], (page + 1) * page_size) -
               max(section['offset'], page * page_size) for section in layout['sections']}
    name, size = max(overlap.items(), key=lambda item: item[1], default=('none', 0))
    return name if size > 0 else 'none'


def symbol_span(layout, address):
    """(start, size, name) of the sized symbol holding a virtual address."""
    symbols = layout['symbols']
    index = bisect.bisect_right(symbols, (address, float('inf')))
    for start, size, _, name in reversed(symbols[max(0, index - 8):index]):
        if start <= address < start + size:
            return start, size, name
    return None


def symbol_at(layout, address):
    span = symbol_span(layout, address)
    return span[2] if span else None


def policy_prefix(library, facade):
    """[start, end) of the ordered startup prefix of .text, from the policy the
    packaging check verifies; None when the object has no applicable policy."""
    try:
        backend = FUNCTION_ORDER['library_backend'](library)
        policy = ROOT / 'tools/packaging/function-order' / backend / f'lib{facade}.order'
        FUNCTION_ORDER['verify'](library, policy, facade)
    except (OSError, ValueError):
        return None
    names = set(FUNCTION_ORDER['command'](['bash', ROOT / 'tools/packaging/function-order.sh', 'policy', policy,
                                          f'lib{facade}.so.15']).splitlines())
    generator = FUNCTION_ORDER['GEN']
    nm = ['nm', '--format=sysv', '--defined-only', '--no-sort']
    raw = generator['symbols'](FUNCTION_ORDER['command']([*nm, '--no-demangle', library]))
    decoded = generator['symbols'](FUNCTION_ORDER['command']([*nm, '--demangle=rust', library]))
    spans = [(int(original[1], 16), int(original[1], 16) + int(original[4] or '0', 16))
             for original, symbol in zip(raw, decoded, strict=True)
             if symbol[6] == '.text' and generator['stable_identity'](symbol[0], original[0]) in names]
    return (min(start for start, _ in spans), max(end for _, end in spans)) if spans else None


def file_offset(address, maps, library):
    for start, end, perms, path, offset in maps:
        if path == str(library) and start <= address < end:
            return address - start + offset
    return None


def callgrind_costs(text, library):
    """Exclusive costs of every collected event, by object and function, from
    one in-memory Callgrind profile. The cost line after `calls=` is the
    callee's inclusive cost and is skipped; the exclusive sums must equal the
    profile's totals.
    """
    events = re.findall(r'^events:\s*(.*?)\s*$', text, re.M)
    totals = re.findall(r'^totals:\s*(.*?)\s*$', text, re.M)
    if len(events) != 1 or len(totals) != 1:
        raise ValueError('expected one Callgrind events/totals pair')
    names = events[0].split()
    total = [int(value) for value in totals[0].split()]
    total += [0] * (len(names) - len(total))
    positions = 1
    ids = {'ob': {}, 'fn': {}}
    obj = function = '???'
    skip = False
    exclusive = {}
    for line in text.splitlines():
        if line.startswith('positions:'):
            positions = len(line.split(':', 1)[1].split())
            continue
        match = re.match(r'^(c?)(ob|fn)=(?:\((\d+)\))?\s*(.*)$', line)
        if match:
            table = ids[match[2]]
            value = match[4]
            if match[3] is not None:
                if value:
                    table[match[3]] = value
                elif match[3] not in table:
                    raise ValueError('unresolved Callgrind name ID')
                value = table[match[3]]
            if not match[1]:
                if match[2] == 'ob':
                    obj = value
                else:
                    function = value
            continue
        if line.startswith('calls='):
            skip = True
            continue
        if not re.match(r'^(?:[+\-]?\d+|\*|0x[0-9a-fA-F]+)(?:\s|$)', line):
            continue
        if skip:
            skip = False
            continue
        costs = exclusive.setdefault((obj, function), [0] * len(names))
        for index, value in enumerate(line.split()[positions:]):
            costs[index] += int(value)
    sums = [sum(costs[index] for costs in exclusive.values()) for index in range(len(names))]
    if sums != total or 'Ir' not in names or not total[names.index('Ir')]:
        raise ValueError('Callgrind exclusive costs do not reconcile with totals')
    def label(path):
        return 'SUBJECT' if Path(path) == Path(library) else Path(path).name
    objects = {}
    for (path, _), costs in exclusive.items():
        target = objects.setdefault(label(path), [0] * len(names))
        for index, value in enumerate(costs):
            target[index] += value
    ir = names.index('Ir')
    functions = sorted(((label(path), name, costs[ir]) for (path, name), costs in exclusive.items() if costs[ir]),
                       key=lambda item: (-item[2], item[0], item[1]))
    return dict(totals=dict(zip(names, total)),
                objects={key: dict(zip(names, value)) for key, value in sorted(objects.items())},
                functions=[dict(object=obj, function=name, Ir=value) for obj, name, value in functions])


class Campaign:
    def __init__(self, worker, data, work, cpu, driver, cache='warm'):
        self.worker, self.data, self.work, self.cpu, self.driver = worker, data.resolve(), work, cpu, driver
        self.serial = 0
        self.cache = cache

    def run(self, facade, library, profile, *, mode='time', save=False, scenario='first-candidate',
            rows=None, instances=1, cycles=1, inputs=None, reopen=False, profiler=None, probe=None):
        cold = self.cache == 'cold' and mode in ('time', 'memory', 'init')
        if cold and (cycles != 1 or profiler):
            raise ValueError('cold samples require one native lifecycle without a profiler')
        if probe not in (None, 'none', 'count', 'faults', 'trace', 'pages', *INIT_SAMPLE_PERIOD_NS) or \
                (probe in ('trace', 'pages') and profiler):
            raise ValueError('unknown init probe or a traced profiler')
        if probe not in (None, 'none') and mode != 'init':
            raise ValueError('init probes require init mode')
        self.serial += 1
        scratch = self.work / ('sample-%06d' % self.serial)
        scratch.mkdir()
        user = scratch / 'user'
        shutil.copytree(profile, user)
        expected_source = file_set(profile)
        checkpoints, samples, closed = {}, [], None
        command = ['taskset', '-c', str(self.cpu)] if self.cpu is not None else []
        trace_read = trace_write = marker = marker_number = None
        if probe == 'trace':
            # strace runs unpinned; the traced worker keeps taskset's CPU. The
            # trace goes to its own pipe so the worker's stderr stays comparable.
            trace_read, trace_write = os.pipe()
            marker = os.open(os.devnull, os.O_WRONLY)
            marker_number = marker
            command = ['strace', '-f', '-qq', '-y', '-T', '-s', '256', '-e', 'signal=none',
                       '-o', '/dev/fd/%d' % trace_write, *command]
        if probe == 'pages':
            # Lackey logs every access to its own pipe; the worker's marker is
            # a second descriptor on that pipe, so the bytes keep their order.
            trace_read, trace_write = os.pipe()
            marker = marker_number = os.dup(trace_write)
            command += ['valgrind', '--tool=lackey', '--trace-mem=yes', '--log-fd=%d' % trace_write,
                        '--vgdb-prefix=' + str(scratch / 'vgdb')]
        if profiler:
            command += ['valgrind', '--tool=' + profiler.removesuffix('-init'), '--vgdb-prefix=' + str(scratch / 'vgdb')]
            if profiler == 'callgrind':
                command += ['--collect-atstart=no', '--cache-sim=no', '--branch-sim=no',
                            '--read-inline-info=yes', '--callgrind-out-file=/dev/stderr',
                            '--toggle-collect=bisect_teardown_free_unit',
                            '--toggle-collect=bisect_teardown_fini_unit']
            elif profiler == 'callgrind-init':
                # Diagnostic: exclusive costs inside the facade's init export only.
                command += ['--collect-atstart=no', '--cache-sim=yes', '--branch-sim=yes', *INIT_CACHE_SIM,
                            '--callgrind-out-file=/dev/stderr', '--toggle-collect=' + facade + '_init']
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
        env.pop('OXPINYIN_TEARDOWN_INIT_PROBE', None)
        env.pop('OXPINYIN_TEARDOWN_MARKER_FD', None)
        if inputs is not None:
            env['OXPINYIN_TEARDOWN_INPUTS'] = str(inputs)
        if probe not in (None, 'none'):
            env['OXPINYIN_TEARDOWN_INIT_PROBE'] = ('sample:%d' % INIT_SAMPLE_PERIOD_NS[probe]
                                                   if probe in INIT_SAMPLE_PERIOD_NS else
                                                   'trace' if probe == 'pages' else probe)
        if marker is not None:
            env['OXPINYIN_TEARDOWN_MARKER_FD'] = str(marker)
        errors, documents, boundaries, trace = bytearray(), [], [], bytearray()
        accesses = LackeyAccesses() if probe == 'pages' else None
        files = None
        self.driver.check_cancelled()
        try:
            self.driver.check_tkrzw(os.environ.get('CONTRACT_DIFF_CELL', 'bdb'),
                                    'teardown.py', facade, library, env=env)
            cold_set = None
            if cold or mode == 'init':
                # Resolve every measured file before eviction; init reads the
                # same list's page-cache residency at M0 and I1.
                files = cold_files(library, self.data, user, rows)
            if cold:
                cold_set = files
                evict_files(cold_set)
            self.driver.check_cancelled()
            passed = tuple(value for value in (trace_write, marker) if value is not None)
            with subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                  env=env, start_new_session=True, pass_fds=passed) as process:
                for value in passed:
                    os.close(value)
                trace_write = marker = None
                def drain():
                    while block := process.stderr.read(65536):
                        errors.extend(block)
                reader = threading.Thread(target=drain)
                reader.start()
                tracer = None
                if trace_read is not None:
                    def drain_trace():
                        while block := os.read(trace_read, 1 << 20):
                            if accesses is not None:
                                accesses.feed(block)
                            else:
                                trace.extend(block)
                    tracer = threading.Thread(target=drain_trace)
                    tracer.start()
                selector = selectors.DefaultSelector()
                selector.register(process.stdout, selectors.EVENT_READ)
                buffered = bytearray()
                deadline = time.monotonic() + (1800 if profiler or probe == 'pages' else 120)
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
                                    if cold_set is not None and mode == 'memory':
                                        memory['residency'] = file_residency(cold_set, library, self.data, user)
                                    if mode == 'init':
                                        # Private paths differ per sample; name them by role.
                                        memory['groups'] = {key.replace(str(user), '$USER'): value
                                                            for key, value in memory['groups'].items()}
                                        memory['residency'] = residency(files, library, self.data, user)
                                        memory['maps'] = process_maps(process.pid)
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
                    if tracer is not None:
                        tracer.join()
            self.driver.check_cancelled()
            if status:
                raise ValueError(f'native teardown failed ({status}): {bytes(errors)[-3000:]!r}')
            if file_set(profile) != expected_source:
                raise ValueError('source profile was changed')
            stderr = errors.decode('utf-8', 'surrogateescape').replace(str(user), '$USER')
            observation = dict(samples=[{key: row[key] for key in ('options', 'saved', 'observations')} for row in samples],
                               stderr=stderr, userfiles=profile_contents(self.driver, user, library=library), documents=documents)
            if mode not in ('export', 'populate'):
                if len(samples) != cycles or closed is None or closed['status'] != 0:
                    raise ValueError('native lifecycle did not complete')
                if mode == 'init' and boundaries != [('M0', 0), ('I1', 1)]:
                    raise ValueError('native init checkpoint boundaries are out of order')
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
                        for clock in CLOCK_FIELDS}
            if reopen:
                observation['reopen'] = self.run(facade, library, user, mode='observe', inputs=inputs)['observation']
            result = dict(observation=observation, samples=samples, checkpoints=checkpoints, command=command,
                          raw_profile_identity=identity(file_set(user)))
            if mode == 'init':
                probes = [value for value in documents if isinstance(value, dict) and 'init_probe' in value]
                if len(probes) != (probe in ('count', 'faults', *INIT_SAMPLE_PERIOD_NS)):
                    raise ValueError('native init probe report is missing or repeated')
                observation['documents'] = [value for value in documents if value not in probes]
                names = {str(user): '$USER', str(self.data): '$DATA', str(library.resolve()): '$LIB',
                         str(Path(library).parent.resolve()): '$LIBDIR', str(self.work): '$WORK'}
                # The private user path names the sample's profile mappings.
                result['init'] = dict(probe=probes[0] if probes else None, user=str(user),
                                      trace=init_trace(trace.decode('utf-8', 'surrogateescape'), marker_number, names)
                                      if probe == 'trace' else None,
                                      accesses=accesses.result() if accesses is not None else None)
            return result
        finally:
            for value in (trace_read, trace_write, marker):
                if value is not None:
                    os.close(value)
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
    campaign.driver.check_tkrzw(os.environ.get('CONTRACT_DIFF_CELL', 'bdb'),
                               'teardown.py/populate', 'pin', pin, env=env)
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
        if args.teardown_stage not in ('verify', 'pilot', 'confirm', 'init-attribute'):
            parser.error('--teardown-cache cold supports only verify, pilot, confirm and init-attribute stages')
    # All cells use backend-aware logical records for final-profile
    # equivalence, including empty rows. Runtime checks also run on each
    # copied object immediately before every native or comparison worker.
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
    # Profiles and witnesses come from the pin's pinyin facade, whichever
    # facades are measured (`--teardown-facades zhuyin` included).
    campaign.provisioner = subjects['pin'].get('pinyin')
    if campaign.provisioner is None:
        campaign.provisioner = libraries / LIBRARY_DIRECTORIES['pin'] / 'libpinyin.so'
        shutil.copyfile(args.prefix / 'lib' / 'libpinyin.so', campaign.provisioner)
    profiles = provision(campaign, campaign.provisioner, work / 'profiles')
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
    witnesses = prepare_witnesses(campaign, getattr(campaign, 'provisioner', None) or subjects['pin']['pinyin'], profiles)
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
    if args.teardown_stage == 'init-attribute':
        return init_attribute(campaign, subjects, profiles, cases, facades, references, dirty_rows)
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


def rotation(index):
    order = ['ox', 'control', 'pin']
    return order[index % 3:] + order[:index % 3]


def init_attribute(campaign, subjects, profiles, cases, facades, references, dirty_rows):
    """Diagnostic attribution of the init interval only (#1090).

    Probe samples run in `init` mode, in the rotating order, after the stage's
    equivalence check; each must keep its side's reference observation. None
    is a primary timing sample. Init Ir uses Callgrind with collection toggled
    on the facade's init export: exact on KC and tkrzw (an INVALID control
    fails the stage), diagnostic on BDB, where libdb seeds srandom_r from
    PID, time and address. Cold mode skips Ir, which the page cache cannot
    change. The `pages` probe (Lackey) maps the facade pages init reads, runs
    and touches onto the object's layout and ordered startup prefix.
    """
    cell = os.environ.get('CONTRACT_DIFF_CELL', 'bdb')
    if shutil.which('strace') is None:
        raise ValueError('init-attribute traces syscalls with strace; none is on PATH')
    symbols = kernel_symbols()
    invalid = False
    for facade in facades:
        for case in cases:
            options = case_options(case, profiles, dirty_rows[facade])
            bitmaps = None
            for probe, (rounds, warmups) in INIT_PROBE_ROUNDS.items():
                rows = {side: [] for side in subjects}
                for index in range(rounds + warmups):
                    for side in rotation(index):
                        row = campaign.run(facade, subjects[side][facade], mode='init', probe=probe, **options)
                        if workload_observation(row) != references[facade, case][side]:
                            raise ValueError(f'init workload observation changed: {facade} {case} {side} {probe}')
                        if index >= warmups:
                            rows[side].append(row)
                if probe == 'count':
                    report_init_counts(facade, case, rows, campaign.cache)
                    bitmaps = rows
                elif probe == 'pages':
                    report_init_pages(facade, case, rows, bitmaps, subjects, campaign.cache)
                elif probe in INIT_SAMPLE_PERIOD_NS:
                    report_init_samples(facade, case, rows, symbols, subjects, campaign.worker, campaign.cache, probe)
                elif probe == 'faults':
                    report_init_faults(facade, case, rows, subjects, campaign, campaign.cache)
                else:
                    report_init_trace(facade, case, rows, campaign.cache)
            if campaign.cache == 'cold':
                print_record('init-ir', cell=cell, facade=facade, case=case, status='NOT RUN',
                             reason='instruction counts do not depend on the page cache; run the warm stage')
                continue
            costs = {side: [] for side in subjects}
            for index in range(INIT_IR_ROUNDS):
                for side in rotation(index):
                    row = campaign.run(facade, subjects[side][facade], profiler='callgrind-init', **options)
                    require_profile_controls(row, references[facade, case][side])
                    costs[side].append(callgrind_costs(row['observation']['stderr'], subjects[side][facade]))
            invalid |= report_init_ir(cell, facade, case, costs)
    return 1 if invalid else 0


def report_init_counts(facade, case, rows, cache):
    values = {side: {field: [row['samples'][0]['times']['init_ns'][field] for row in samples]
                     for field in CLOCK_FIELDS} for side, samples in rows.items()}
    print_record('init-resources', facade=facade, case=case, cache=cache, rounds=len(rows['ox']),
                 note='diagnostic init-mode samples; user_ns/sys_ns follow the kernel CPU accounting',
                 summaries={side: {field: summary(value) for field, value in fields.items()}
                            for side, fields in values.items()},
                 paired={label: {field: paired_difference(values[a][field], values[b][field]) for field in CLOCK_FIELDS}
                         for label, a, b in (('ox-pin', 'ox', 'pin'), ('control-ox', 'control', 'ox'))})
    events = {}
    for side, samples in rows.items():
        named = {}
        for row in samples:
            for event in row['init']['probe']['events']:
                named.setdefault(event['name'], []).append(event)
        events[side] = {}
        for name, readings in named.items():
            refused = sorted({event['errno'] for event in readings if event['errno']})
            if refused:
                events[side][name] = dict(status='NOT RUN', errno=refused, error=[os.strerror(value) for value in refused])
                continue
            events[side][name] = dict(
                status='counted', user_only=any(event['user_only'] for event in readings),
                value=summary([event['value'] for event in readings]),
                bracket=summary([event['bracket'] for event in readings]),
                multiplexed=any(event['running_ns'] < event['enabled_ns'] for event in readings))
    print_record('init-perf', facade=facade, case=case, cache=cache, rounds=len(rows['ox']), events=events,
                 note='value is the init bracket minus an empty calibration bracket; NOT RUN is never zero')
    for side, samples in rows.items():
        growth, cached = {}, {}
        for row in samples:
            before, after = row['checkpoints'][0]['M0'], row['checkpoints'][1]['I1']
            for group in set(before['groups']) | set(after['groups']):
                growth.setdefault(group, []).append(after['groups'].get(group, {}).get('Rss', 0) -
                                                    before['groups'].get(group, {}).get('Rss', 0))
            for role, pages in after['residency'].items():
                cached.setdefault(role, []).append((before['residency'][role]['resident'], pages['resident'], pages['pages']))
        print_record('init-residency', facade=facade, case=case, side=side, cache=cache, rounds=len(samples),
                     rss_kib_growth={group: summary(value) for group, value in sorted(growth.items()) if any(value)},
                     page_cache_pages={role: dict(pages=value[0][2], m0=summary([item[0] for item in value]),
                                                  i1=summary([item[1] for item in value]),
                                                  growth=summary([item[1] - item[0] for item in value]))
                                       for role, value in sorted(cached.items())})


def report_init_samples(facade, case, rows, symbols, subjects, worker, cache, probe='sample'):
    for side, samples in rows.items():
        library = subjects[side][facade].resolve()
        user, kernel, leaves, functions = collections.Counter(), collections.Counter(), {}, {}
        status = dict(lost=0, throttled=0, valid=True, user_only=False, errno=[])
        count = 0
        for row in samples:
            report = row['init']['probe']
            if report['errno']:
                status['errno'].append(report['errno'])
                continue
            status['lost'] += report['lost']
            status['throttled'] += report['throttled']
            status['valid'] &= report['valid']
            status['user_only'] |= report['user_only']
            maps = row['checkpoints'][1]['I1']['maps']
            for sample in report['samples']:
                count += 1
                if sample['kernel']:
                    category, leaf = kernel_category(sample, symbols)
                    kernel[category] += 1
                    leaves.setdefault(category, collections.Counter())[leaf] += 1
                else:
                    owner = user_object(sample['ip'], maps, library, worker)
                    user[owner] += 1
                    functions.setdefault(owner, collections.Counter())[user_function(sample['ip'], maps)] += 1
        if status['errno']:
            print_record('init-samples', facade=facade, case=case, side=side, cache=cache, probe=probe, status='NOT RUN',
                         errno=sorted(set(status['errno'])))
            continue
        print_record('init-samples', facade=facade, case=case, side=side, cache=cache, probe=probe, rounds=len(samples),
                     period_ns=INIT_SAMPLE_PERIOD_NS[probe], samples=count, user_samples=sum(user.values()),
                     kernel_samples=sum(kernel.values()), user=dict(user.most_common()), kernel=dict(kernel.most_common()),
                     kernel_leaves={category: dict(value.most_common(8)) for category, value in sorted(leaves.items())},
                     user_functions={owner: dict(value.most_common(15)) for owner, value in sorted(functions.items())},
                     symbolized=symbols is not None,
                     cpu_ns=summary([row['samples'][0]['times']['init_ns']['cpu_ns'] for row in samples]), **status)


def report_init_faults(facade, case, rows, subjects, campaign, cache):
    for side, samples in rows.items():
        library = subjects[side][facade].resolve()
        regions, code, status = {}, {}, dict(lost=0, throttled=0, valid=True, errno=[])
        for round_index, row in enumerate(samples):
            probe = row['init']['probe']
            if probe['errno']:
                status['errno'].append(probe['errno'])
                continue
            status['lost'] += probe['lost']
            status['throttled'] += probe['throttled']
            status['valid'] &= probe['valid']
            faults = init_faults(probe['samples'], row['checkpoints'][0]['M0']['maps'], library, campaign.data,
                                 Path(row['init']['user']))
            after = row['checkpoints'][1]['I1']['maps']
            for fault in faults:
                kind = fault['fault']
                origin = 'kernel' if fault['kernel'] else user_object(fault['ip'], after, library, campaign.worker)
                for table, key in ((regions, fault['region']), (code, origin)):
                    table.setdefault(kind, {}).setdefault(key, [0] * len(samples))[round_index] += 1
        if status['errno']:
            print_record('init-faults', facade=facade, case=case, side=side, cache=cache, status='NOT RUN',
                         errno=sorted(set(status['errno'])))
            continue
        def shown(table):
            return {kind: {key: summary(counts) for key, counts in sorted(values.items(), key=lambda item: -sum(item[1]))}
                    for kind, values in sorted(table.items())}
        print_record('init-faults', facade=facade, case=case, side=side, cache=cache, rounds=len(samples),
                     by_region=shown(regions), by_code=shown(code), **status)


def report_init_pages(facade, case, rows, bitmaps, subjects, cache):
    """Which facade pages init reads into the page cache (count samples' M0
    and I1 bitmaps), against what init executes and the data it touches
    (Lackey), the object's section and symbol layout and its ordered prefix.
    Every page index is a host page (`SC_PAGE_SIZE`), the unit of the mincore
    bitmaps."""
    page_size = os.sysconf('SC_PAGE_SIZE')
    for side in subjects:
        library = subjects[side][facade].resolve()
        layout = library_layout(library)
        prefix = policy_prefix(library, facade) if side != 'pin' else None
        def address_offset(address):
            section = next((value for value in layout['sections']
                            if value['address'] <= address < value['address'] + value['size']), None)
            return address - section['address'] + section['offset'] if section else None
        prefix_pages = ([address_offset(prefix[0]) // page_size, address_offset(prefix[1] - 1) // page_size + 1]
                        if prefix else None)
        record = dict(facade=facade, case=case, side=side, cache=cache, page_size=page_size,
                      sections={section['name']: [section['offset'] // page_size,
                                                  (section['offset'] + section['size'] - 1) // page_size + 1]
                                for section in layout['sections']},
                      prefix=dict(start=hex(prefix[0]), end=hex(prefix[1]), pages=prefix_pages) if prefix else None)
        read_in = None
        if bitmaps:
            sets = []
            for row in bitmaps[side]:
                before = row['checkpoints'][0]['M0']['residency']['SUBJECT']
                after = row['checkpoints'][1]['I1']['residency']['SUBJECT']
                sets.append(frozenset(range_pages(after['ranges']) - range_pages(before['ranges'])))
            modal = collections.Counter(sets).most_common(1)[0]
            read_in = set(modal[0])
            record['read_in'] = dict(pages=len(read_in), rounds=len(sets), rounds_with_this_set=modal[1],
                                     sizes=summary([len(value) for value in sets]), ranges=page_ranges(read_in))
        executed, touched, functions = set(), collections.Counter(), {}
        if rows:
            row = rows[side][0]
            accesses = row['init']['accesses']
            if not accesses['window']:
                raise ValueError('Lackey init markers were not found')
            maps = row['checkpoints'][1]['I1']['maps']
            base = min(start for start, _, _, path, _ in maps if path == str(library))
            for address in accesses['code']:
                offset = file_offset(address, maps, library)
                if offset is None:
                    continue
                executed.add(offset // page_size)
                span = symbol_span(layout, address - base)
                # Keyed by the symbol's start: local symbols of different
                # units can share a demangled name. The name is for display.
                key = span[0] if span else section_at(layout, offset) or 'unknown'
                entry = functions.setdefault(key, dict(name=span[2] if span else key, instructions=0,
                                                       pages=set(), in_prefix=None, bytes=span[1] if span else 0))
                entry['instructions'] += 1
                entry['pages'].add(offset // page_size)
                if prefix:
                    entry['in_prefix'] = prefix[0] <= address - base < prefix[1]
            data_pages = {}
            for line in accesses['data_lines']:
                offset = file_offset(line << 6, maps, library)
                if offset is not None:
                    data_pages.setdefault(offset // page_size, set()).add(symbol_at(layout, (line << 6) - base) or '-')
                    touched[section_at(layout, offset) or 'unknown'] += 1
            # The whole bodies of the executed functions: what their pages
            # would be if the layout placed init's functions contiguously.
            function_bytes = sum(entry['bytes'] for entry in functions.values())
            record['executed'] = dict(
                pages=len(executed), functions=len(functions), all_objects_instructions=len(accesses['code']),
                instructions=sum(entry['instructions'] for entry in functions.values()),
                function_bytes=function_bytes, contiguous_pages=-(-function_bytes // page_size),
                largest=[dict(function=entry['name'], instructions=entry['instructions'],
                              pages=page_ranges(entry['pages']), in_prefix=entry['in_prefix'])
                         for entry in sorted(functions.values(), key=lambda entry: -entry['instructions'])[:40]],
                outside_prefix=None if not prefix else sorted(
                    (dict(function=entry['name'], pages=sorted(entry['pages']), instructions=entry['instructions'])
                     for entry in functions.values() if entry['in_prefix'] is False),
                    key=lambda value: -value['instructions']),
                pages_in_prefix=None if not prefix else len({page for entry in functions.values() if entry['in_prefix']
                                                             for page in entry['pages']}))
            record['data_touched'] = dict(pages=len(data_pages), lines_by_section=dict(touched.most_common()),
                                          pages_by_section=dict(collections.Counter(
                                              page_section(layout, page, page_size) for page in data_pages).most_common()))
            if read_in is not None:
                record['read_in']['executed'] = len(read_in & executed)
                record['read_in']['data_touched'] = len(read_in & set(data_pages))
                record['read_in']['neither'] = len(read_in - executed - set(data_pages))
        if read_in is not None or rows:
            pages = set(read_in or ()) | (executed | set(data_pages) if rows else set())
            # Bytes of each named symbol on each listed page, largest first.
            held = {page: collections.Counter() for page in pages}
            for start, size, _, name in layout['symbols']:
                offset = address_offset(start)
                if offset is None:
                    continue
                for page in range(offset // page_size, (offset + size - 1) // page_size + 1):
                    if page in held:
                        held[page][name] += min(offset + size, (page + 1) * page_size) - max(offset, page * page_size)
            record['pages'] = [dict(page=page, section=page_section(layout, page, page_size),
                                    read_in=None if read_in is None else page in read_in,
                                    executed=page in executed, data=bool(rows) and page in data_pages,
                                    in_prefix=bool(prefix_pages) and prefix_pages[0] <= page < prefix_pages[1],
                                    symbols=[name for name, _ in held[page].most_common(3)])
                               for page in sorted(pages)]
        print_record('init-pages', **record)


def report_init_trace(facade, case, rows, cache):
    for side, samples in rows.items():
        traces = [row['init']['trace'] for row in samples]
        if not all(trace['window'] for trace in traces):
            raise ValueError('init trace markers were not found exactly twice')
        names = sorted(set().union(*(trace['calls'] for trace in traces)))
        calls = {name: {key: summary([trace['calls'].get(name, {}).get(key, 0) for trace in traces])
                        for key in ('count', 'errors', 'seconds')} for name in names}
        print_record('init-syscalls', facade=facade, case=case, side=side, cache=cache, rounds=len(traces),
                     note='strace -T times include ptrace stops; counts are exact',
                     total_count=summary([trace['total']['count'] for trace in traces]),
                     total_seconds=summary([trace['total']['seconds'] for trace in traces]),
                     calls=calls, targets=traces[0]['targets'],
                     targets_identical=all(trace['targets'] == traces[0]['targets'] for trace in traces),
                     threads=max(trace['threads'] for trace in traces),
                     unparsed=sum(trace['unparsed'] for trace in traces),
                     unfinished=sum(trace['unfinished'] for trace in traces))


def report_init_ir(cell, facade, case, costs):
    readings = {side: [value['totals']['Ir'] for value in values] for side, values in costs.items()}
    exact = cell != 'bdb'
    equal = len(set(readings['ox'])) == 1 and set(readings['ox']) == set(readings['control'])
    verdict = ('VALID' if equal else 'INVALID') if exact else 'DIAGNOSTIC'
    print_record('init-ir-control', cell=cell, facade=facade, case=case, ox=readings['ox'], control=readings['control'],
                 rule='exact' if exact else 'diagnostic: BDB init Ir is not reproducible (libdb seeds srandom_r)',
                 verdict=verdict)
    for side in ('control',) if verdict == 'INVALID' else ('ox', 'control', 'pin'):
        first = costs[side][0]
        print_record('init-ir', cell=cell, facade=facade, case=case, side=side, profiler_only=True,
                     readings=readings[side], simulated=list(INIT_CACHE_SIM),
                     events_identical=all(value['totals'] == first['totals'] for value in costs[side]),
                     totals=first['totals'], objects=first['objects'], functions=first['functions'][:25])
    return verdict == 'INVALID'


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
        # Cold samples only: which files' pages each phase brought in. M0 is
        # after load and before init; M1 after the workload.
        points = [dict(row['checkpoints'][1], M0=row['checkpoints'][0]['M0']) for row in rows]
        if all('residency' in observation[point] for observation in points for point in ('M0', 'M1', 'M2', 'M3')):
            files = {label: dict(pages=summary([observation['M1']['residency'][label]['pages'] for observation in points]),
                                 **{point: summary([observation[point]['residency'][label]['resident'] for observation in points])
                                    for point in ('M0', 'M1', 'M2', 'M3')})
                     for label in points[0]['M0']['residency']}
            print_record('residency', facade=facade, case=case, side=side, pilot=pilot, unit='pages', files=files)


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
