"""Host CPU reads and process-group liveness. Standard library only.

It never spawns a shell tool when a system call or a standard-library read can
answer.
"""
import math
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys



def allowed_cpus() -> set[int]:
    """Read process affinity, or all reported CPUs where affinity is unavailable.

    This is a capacity read, not evidence that CPU pinning tools are available.
    """
    affinity = getattr(os, 'sched_getaffinity', None)
    return affinity(0) if affinity is not None else set(range(os.cpu_count() or 1))


def linux_cgroup_dirs(controller):
    """Find the current cgroup and ancestors up to the visible mount root.

    The default retains the v2 unified path. V1 controller mounts can live
    anywhere and can expose only a subtree of the controller hierarchy.
    """
    proc, root = Path('/proc'), Path('/sys/fs/cgroup')
    entries = {}
    for line in (proc / 'self/cgroup').read_text().splitlines():
        hierarchy, controllers, entry = line.split(':', 2)
        for name in controllers.split(','):
            if name or hierarchy == '0':
                entries[name] = entry
    entry = entries.get(controller or '')
    if entry is None:
        return []
    relative = PurePosixPath(entry)
    if not relative.is_absolute() or '..' in relative.parts:
        raise ValueError('invalid cgroup path')
    if controller is not None:
        mounts = []
        for line in (proc / 'self/mountinfo').read_text().splitlines():
            before, after = line.split(' - ', 1)
            fields, filesystem = before.split(), after.split()
            if filesystem[0] != 'cgroup' or controller not in filesystem[2].split(','):
                continue
            # mountinfo escapes whitespace and backslashes with octal codes.
            mount_root, mount_point = (re.sub(r'\\([0-7]{3})', lambda match: chr(int(match[1], 8)), field)
                                       for field in fields[3:5])
            # The root is a path inside the cgroup tree (POSIX form on every host);
            # the mount point is a path on this host.
            mount_root, mount_point = PurePosixPath(mount_root), Path(mount_point)
            if (not mount_root.is_absolute() or not mount_point.is_absolute()
                    or '..' in mount_root.parts or '..' in mount_point.parts):
                raise ValueError('invalid cgroup mount path')
            if relative.is_relative_to(mount_root):
                mounts.append((mount_root, mount_point))
        if not mounts:
            raise ValueError(f'no visible {controller} cgroup mount')
        # Prefer the mount exposing the most ancestors if bind mounts overlap.
        mount_root, root = min(mounts, key=lambda mount: len(mount[0].parts))
        relative = relative.relative_to(mount_root)
    else:
        relative = relative.relative_to('/')
    current = root / relative
    return [current, *list(current.parents)[:len(current.relative_to(root).parts)]]


def processors() -> int:
    """CPU capacity available to Cargo, rounded up after applying affinity and ancestor quotas."""
    if not sys.platform.startswith('linux'):
        return os.cpu_count() or 1
    try:
        affinity = len(allowed_cpus())
    except (OSError, ValueError, AttributeError):
        affinity = 0
    count = affinity or os.cpu_count()
    limits = [count] if count else []
    for controller in (None, 'cpu'):
        try:
            directories = linux_cgroup_dirs(controller=controller)
        except (OSError, ValueError, IndexError, StopIteration):
            continue
        for directory in directories:
            try:
                if controller == 'cpu':
                    quota = (directory / 'cpu.cfs_quota_us').read_text()
                    period = (directory / 'cpu.cfs_period_us').read_text()
                else:
                    quota, period = (directory / 'cpu.max').read_text().split()
                quota, period = int(quota), int(period)
                if quota > 0 and period > 0:
                    limits.append(quota / period)
            except (OSError, ValueError):
                continue
    return max(1, math.ceil(min(limits, default=1)))


def process_group_live(group: int) -> bool | None:
    """Whether the POSIX group has a non-zombie member, or None if observation failed."""
    try:
        if sys.platform.startswith('linux'):
            for entry in Path('/proc').iterdir():
                if not entry.name.isdigit():
                    continue
                try:
                    fields = (entry / 'stat').read_text().rsplit(')', 1)[1].split()
                except FileNotFoundError:
                    continue
                if int(fields[2]) == group and fields[0] not in ('Z', 'X'):
                    return True
            return False
        if sys.platform == 'darwin':
            ps_output = subprocess.run(['ps', '-axo', 'pgid=,stat='], capture_output=True,
                                       text=True, check=True, timeout=5).stdout
            for line in ps_output.splitlines():
                fields = line.split()
                if fields and int(fields[0]) == group and not fields[1].startswith(('Z', 'X')):
                    return True
            return False
    except (OSError, ValueError, IndexError, subprocess.SubprocessError):
        return None
    return None
