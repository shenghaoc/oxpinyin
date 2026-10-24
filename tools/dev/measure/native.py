"""Filesystem and stopped-child memory readers. No measured code runs here."""
import os
from pathlib import Path
import re
import subprocess
import sys

COMMAND_TIMEOUT_SECONDS = 30

MEMORY_FIELDS = ('rss', 'rss_delta', 'peak_rss', 'anonymous', 'file_backed',
                 'private_dirty', 'pss', 'virtual_size', 'minor_faults', 'major_faults')


def filesystem(path):
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
    """Check roots and, by default, their files (nested mounts count)."""
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


def read_memory(pid, *, proc_root=Path('/proc')):
    result = dict.fromkeys(MEMORY_FIELDS)
    if sys.platform == 'darwin':
        values = subprocess.check_output(['ps', '-o', 'rss=', '-o', 'vsz=', '-p', str(pid)], text=True, timeout=COMMAND_TIMEOUT_SECONDS).split()
        if len(values) != 2:
            raise ValueError('ps did not return the stopped child RSS and virtual size')
        result.update(rss=int(values[0]), virtual_size=int(values[1]))
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
    if sys.platform.startswith('linux'):
        allowed = os.sched_getaffinity(0)
        cpu = min(allowed) if cpu is None else cpu
        if cpu not in allowed:
            raise ValueError('CPU is outside this process affinity')
        return cpu
    if cpu is not None:
        raise ValueError('CPU pinning is unavailable on native macOS')
    return None
