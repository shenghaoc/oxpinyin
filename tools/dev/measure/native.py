"""Filesystem and stopped-child memory readers. No measured code runs here."""
import os
from pathlib import Path
import re
import subprocess
import sys

from .. import darwin, host

COMMAND_TIMEOUT_SECONDS = 30

LINUX_MEMORY_FIELDS = ('rss', 'rss_delta', 'peak_rss', 'anonymous', 'file_backed',
                       'private_dirty', 'pss', 'virtual_size', 'minor_faults', 'major_faults')
DARWIN_COUNTER_FIELDS = ('instructions', 'cycles')
DARWIN_AVAILABILITY_FIELDS = ('instructions_available', 'cycles_available')
DARWIN_MEMORY_FIELDS = ('resident_kib', 'resident_peak_kib', 'physical_footprint_kib',
                        'physical_footprint_interval_max_kib',
                        'physical_footprint_lifetime_max_kib', 'faults', 'pageins',
                        'disk_bytes_read', 'disk_bytes_written')
MEMORY_FIELDS = LINUX_MEMORY_FIELDS + DARWIN_MEMORY_FIELDS


def filesystem(path):
    """Return the filesystem type and volume identity for an existing path.

    On macOS, query df and diskutil; otherwise select the longest matching
    /proc/self/mountinfo mount. Raise ValueError when no mount matches;
    path and subprocess errors propagate.
    """
    path = Path(path).resolve(strict=True)
    if sys.platform == 'darwin':
        import plistlib
        device = subprocess.check_output(['df', '-P', str(path)], text=True, timeout=COMMAND_TIMEOUT_SECONDS).splitlines()[-1].split()[0]
        result = subprocess.run(['diskutil', 'info', '-plist', device], check=True, capture_output=True,
                                timeout=COMMAND_TIMEOUT_SECONDS)
        info = plistlib.loads(result.stdout)
        return info.get('FilesystemType'), info.get('VolumeUUID')
    matches = []
    for line in Path('/proc/self/mountinfo').read_text().splitlines():
        fields, _, trailer = line.partition(' - ')
        mount = re.sub(r'\\([0-7]{3})', lambda m: chr(int(m[1], 8)), fields.split()[4])
        if path.is_relative_to(mount):
            matches.append((len(mount), trailer.split()[0], fields.split()[2]))
    if not matches:
        raise ValueError('filesystem type is unknown: ' + str(path))
    _, kind, volume = max(matches)
    return kind, volume


def check_filesystems(paths, *, recursive=True):
    """Validate filesystem roots and, by default, all recursively found entries.

    Resolve symlink targets and require ext4 on Linux or one APFS volume
    on macOS. Return the volume identities; raise ValueError for wrong or
    unknown filesystems/volumes and propagate filesystem read errors.
    """
    volumes = set()
    for root in paths:
        root = Path(root)
        entries = [root, *root.rglob('*')] if recursive and root.is_dir() else [root]
        for path in entries:
            if path.is_symlink():
                # Library aliases are allowed, but their actual object must qualify.
                path = path.resolve(strict=True)
            kind, volume = filesystem(path)
            expected = 'apfs' if sys.platform == 'darwin' else 'ext4'
            if kind.lower() != expected:
                raise ValueError(f'{path}: requires {expected}, found {kind} (tmpfs is unsupported)')
            if volume is None:
                raise ValueError('filesystem volume identity is unavailable: ' + str(path))
            volumes.add(volume)
    if sys.platform == 'darwin' and len(volumes) != 1:
        raise ValueError('all startup inputs and work must be on one APFS volume')
    return volumes


def _kib(value):
    """Convert a byte count to whole KiB, preserving None for an unavailable value."""
    return None if value is None else int(value) // 1024


def read_memory(pid, *, proc_root=Path('/proc'), resident_peak_bytes=None):
    """Read memory observations for a child paused at a checkpoint.

    Sizes are KiB and counts are counts. Linux reads /proc status,
    smaps_rollup, and stat. macOS reads the shared Darwin libproc binding and
    reports only Darwin-named fields: resident and its peak, physical
    footprint and its interval/lifetime maxima, faults, page-ins, disk bytes
    read and written, and the host instruction/cycle counters with their
    availability flags. The counters are not Valgrind Ir and are never written
    to a Linux-named field. The worker supplies its own resident peak at the
    handshake because another process's task port is normally unavailable;
    without that value the binding's task-port attempt decides. Unavailable
    fields, including rss_delta, remain None. Read/parse errors propagate
    rather than returning partial Linux observations.
    """
    result = dict.fromkeys(MEMORY_FIELDS)
    if sys.platform == 'darwin':
        try:
            observed = darwin.observation(pid)
        except (OSError, AttributeError, ValueError):
            observed = dict.fromkeys(darwin.OBSERVATION_FIELDS)
        resident_peak = (resident_peak_bytes if resident_peak_bytes is not None
                         else observed.get('resident_peak_bytes'))
        result.update(
            resident_kib=_kib(observed.get('resident_bytes')),
            resident_peak_kib=_kib(resident_peak),
            physical_footprint_kib=_kib(observed.get('physical_footprint_bytes')),
            physical_footprint_interval_max_kib=_kib(
                observed.get('physical_footprint_interval_max_bytes')),
            physical_footprint_lifetime_max_kib=_kib(
                observed.get('physical_footprint_lifetime_max_bytes')),
            faults=observed.get('faults'),
            pageins=observed.get('pageins'),
            disk_bytes_read=observed.get('disk_bytes_read'),
            disk_bytes_written=observed.get('disk_bytes_written'),
            instructions=observed.get('instructions'),
            cycles=observed.get('cycles'),
            instructions_available=bool(observed.get('instructions_available')),
            cycles_available=bool(observed.get('cycles_available')),
        )
        return result
    proc = proc_root / str(pid)
    status = {}
    for line in (proc / 'status').read_text().splitlines():
        key, _, value = line.partition(':')
        if key in ('VmRSS', 'VmHWM', 'VmSize'):
            status[key] = int(value.split()[0])
    rollup = {}
    for line in (proc / 'smaps_rollup').read_text().splitlines():
        key, _, value = line.partition(':')
        if key in ('Rss', 'Anonymous', 'Private_Dirty', 'Pss'):
            rollup[key] = int(value.split()[0])
    # comm may contain spaces or parentheses. Fields after its final ')' start at state (3).
    stat = (proc / 'stat').read_text().rsplit(')', 1)[1].split()
    result.update(rss=rollup['Rss'], peak_rss=status['VmHWM'], virtual_size=status['VmSize'],
                  anonymous=rollup['Anonymous'], file_backed=rollup['Rss'] - rollup['Anonymous'],
                  private_dirty=rollup['Private_Dirty'], pss=rollup['Pss'],
                  minor_faults=int(stat[7]), major_faults=int(stat[9]))
    return result


def pin_cpu(cpu):
    """Select an allowed CPU index without changing affinity.

    On Linux, default to the smallest allowed index and reject an index
    outside the affinity set. Elsewhere return None, rejecting explicit
    pinning with ValueError.
    """
    if sys.platform.startswith('linux'):
        allowed = host.allowed_cpus()
        cpu = min(allowed) if cpu is None else cpu
        if cpu not in allowed:
            raise ValueError('CPU is outside this process affinity')
        return cpu
    if cpu is not None:
        raise ValueError('CPU pinning is unavailable on native macOS')
    return None
